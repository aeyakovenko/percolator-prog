//! INV-045: batching must preserve each asset's elapsed discovery capacity.
//! Unlike the uniform-age maximum-shape case, one mark predates the accrual horizon
//! while the other is younger. Setup and reductions use only public instructions.

use super::*;

const MARKS: [u64; 2] = [1_000_000, 2_000_000];
const MARK_SLOTS: [u64; 2] = [1, 4];
const LANDING_SLOT: u64 = 6;
const CAP_BPS: u64 = 100;
const MAX_DT: u64 = 3;
const DEPOSIT: u128 = 50_000_000;
const OPEN_Q: i128 = 4 * POS_SCALE as i128;

fn profiles(env: &V16CuEnv) -> [state::AssetOracleProfileV16; 2] {
    let account = env.svm.get_account(&env.market).unwrap();
    [0, 1].map(|index| state::read_asset_oracle_profile(&account.data, index).unwrap())
}

fn batch(
    env: &mut V16CuEnv,
    owners: [&Keypair; 2],
    portfolios: [Pubkey; 2],
    size_q: i128,
    prices: [u64; 2],
) -> u64 {
    let legs = (0..2)
        .map(|index| BatchTradeLeg {
            asset_index: index as u16,
            market_id: env.asset_market_id(index as u16),
            size_q,
            exec_price: prices[index],
            fee_bps: 0,
        })
        .collect();
    env.svm.expire_blockhash();
    env.send(
        env.batch_trade_no_cpi_ix(portfolios[0], portfolios[1], legs),
        vec![
            AccountMeta::new(owners[0].pubkey(), true),
            AccountMeta::new(owners[1].pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolios[0], false),
            AccountMeta::new(portfolios[1], false),
        ],
        &owners,
    )
    .expect("public two-asset batch")
}

#[test]
fn v16_program_batch_and_single_marks_preserve_staggered_elapsed_envelopes() {
    let raw_prices = [percolator::MAX_ORACLE_PRICE, 1];
    let elapsed = MARK_SLOTS.map(|slot| LANDING_SLOT - slot);
    assert_eq!(elapsed, [5, 2]);
    assert!(elapsed[0] > MAX_DT && elapsed[1] < MAX_DT);

    // Independent bounded integer oracle, not the wrapper's clamp/EWMA/fee helpers.
    let accepted_prices = [0, 1].map(|index| {
        let delta = MARKS[index] * CAP_BPS * elapsed[index].min(MAX_DT) / 10_000;
        raw_prices[index].clamp(MARKS[index] - delta, MARKS[index] + delta)
    });
    assert_eq!(accepted_prices, [1_030_000, 1_960_000]);
    let expected_marks = [0, 1].map(|index| {
        let alpha_bps = 10_000 * elapsed[index] / (elapsed[index] + 1);
        let delta = MARKS[index].abs_diff(accepted_prices[index]) * alpha_bps / 10_000;
        if accepted_prices[index] > MARKS[index] {
            MARKS[index] + delta
        } else {
            MARKS[index] - delta
        }
    });
    assert_eq!(expected_marks, [1_024_999, 1_973_336]);
    let expected_fees = [0, 1].map(|index| {
        let move_bps = (u128::from(MARKS[index].abs_diff(expected_marks[index])) * 10_000)
            .div_ceil(u128::from(MARKS[index]));
        let externality_notional = 2 * OPEN_Q.unsigned_abs() * u128::from(MARKS[index]) / POS_SCALE;
        let required = (externality_notional * move_bps).div_ceil(10_000);
        let notional = u128::from(accepted_prices[index]); // reduction is exactly POS_SCALE
        (0..=10_000u128)
            .map(|bps| 2 * (notional * bps).div_ceil(10_000))
            .find(|paid| *paid >= required)
            .expect("funded movement fits configured fee ceiling")
    });
    let mut outcomes = Vec::new();

    for route in [
        NoCpiReportedPricePath::Single,
        NoCpiReportedPricePath::Batch,
    ] {
        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            max_portfolio_assets: 2,
            initial_price: MARKS[0],
            max_price_move_bps_per_slot: CAP_BPS,
            max_accrual_dt_slots: MAX_DT,
            min_funding_lifetime_slots: MAX_DT,
            ..V16CuMarketParams::default()
        });
        for index in 0..2 {
            env.svm.warp_to_slot(MARK_SLOTS[index]);
            configure_max_shape_ewma_asset(&mut env, index as u16, MARK_SLOTS[index], MARKS[index]);
        }
        let owners = [Keypair::new(), Keypair::new()];
        let portfolios = owners.each_ref().map(|owner| env.create_portfolio(owner));
        for index in 0..2 {
            env.deposit(&owners[index], portfolios[index], DEPOSIT);
        }
        batch(&mut env, owners.each_ref(), portfolios, OPEN_Q, MARKS);
        for portfolio in portfolios {
            env.svm.expire_blockhash();
            env.crank_if_actionable(
                portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: MARK_SLOTS[1],
                    observations: crank_observations_for_assets(&[0, 1]),
                },
            );
        }
        let before = env.market_state().1;
        assert_eq!(before.insurance, 0);
        assert_eq!(profiles(&env).map(|profile| profile.mark_ewma_e6), MARKS);
        assert_eq!(
            profiles(&env).map(|profile| profile.mark_ewma_last_slot),
            MARK_SLOTS
        );
        assert_eq!([0, 1].map(|index| before.assets[index].slot_last), [4, 4]);
        assert_eq!(before.config.max_price_move_bps_per_slot, CAP_BPS);
        assert_eq!(before.config.max_accrual_dt_slots, MAX_DT);

        env.svm.warp_to_slot(LANDING_SLOT);
        let mut cus = Vec::new();
        match route {
            NoCpiReportedPricePath::Single => {
                for index in 0..2 {
                    let untouched = profiles(&env)[1 - index];
                    let insurance_before = env.market_state().1.insurance;
                    env.svm.expire_blockhash();
                    cus.push(env.trade_asset_with_cu(
                        index as u16,
                        &owners[0],
                        portfolios[0],
                        &owners[1],
                        portfolios[1],
                        -(POS_SCALE as i128),
                        raw_prices[index],
                        0,
                    ));
                    assert_eq!(
                        env.market_state().1.insurance - insurance_before,
                        expected_fees[index],
                        "{route:?} asset {index}: exact collected movement fee"
                    );
                    let after = profiles(&env);
                    assert_eq!(after[index].mark_ewma_e6, expected_marks[index]);
                    assert_eq!(after[1 - index].mark_ewma_e6, untouched.mark_ewma_e6);
                    assert_eq!(
                        after[1 - index].mark_ewma_last_slot,
                        untouched.mark_ewma_last_slot
                    );
                }
            }
            NoCpiReportedPricePath::Batch => cus.push(batch(
                &mut env,
                owners.each_ref(),
                portfolios,
                -(POS_SCALE as i128),
                raw_prices,
            )),
        }
        let after = env.market_state().1;
        let after_profiles = profiles(&env);
        for index in 0..2 {
            let mark = after_profiles[index].mark_ewma_e6;
            let max_delta = MARKS[index] * CAP_BPS * elapsed[index].min(MAX_DT) / 10_000;
            assert!(
                mark.abs_diff(MARKS[index]) > 0,
                "{route:?} asset {index}: real movement"
            );
            assert!(
                mark.abs_diff(MARKS[index]) <= max_delta,
                "{route:?} asset {index}: movement exceeds its own elapsed-time envelope"
            );
            assert_eq!(
                mark, expected_marks[index],
                "{route:?} asset {index}: exact bounded EWMA"
            );
            assert_eq!(after_profiles[index].mark_ewma_last_slot, LANDING_SLOT);
            assert_eq!(after_profiles[index].oracle_target_price_e6, mark);
            assert_eq!(after.assets[index].raw_oracle_target_price, mark);
            assert_eq!(after.assets[index].oi_eff_long_q, 3 * POS_SCALE);
            assert_eq!(after.assets[index].oi_eff_short_q, 3 * POS_SCALE);
        }
        assert_eq!(after.insurance, expected_fees.iter().sum::<u128>());
        assert_eq!(after.vault, 2 * DEPOSIT);
        assert_eq!(u128::from(env.token_amount(env.vault)), after.vault);
        assert_eq!(after.insurance_domain_budget.iter().sum::<u128>(), 0);
        outcomes.push((
            after_profiles.map(|profile| (profile.mark_ewma_e6, profile.mark_ewma_last_slot)),
            [0, 1].map(|index| after.assets[index].raw_oracle_target_price),
            after.insurance,
            after.vault,
        ));
        eprintln!(
            "INV-045 staggered elapsed envelopes {route:?}: CU={cus:?}, fees={expected_fees:?}"
        );
        for cu in cus {
            assert_cu_within(
                "staggered two-asset reduction",
                cu,
                MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
            );
        }
    }
    assert_eq!(
        outcomes[0], outcomes[1],
        "batch and singles preserve per-asset paid marks"
    );
}
