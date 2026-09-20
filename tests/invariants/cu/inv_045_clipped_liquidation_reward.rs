//! Row 422: fresh authentication cannot reward an uncollected liquidation fee.
//! Public System/SPL/wrapper setup; only Clock and external reports are inputs.

use super::*;

#[test]
fn v16_program_fresh_report_rewards_only_collectible_full_close_penalty() {
    const SIZE: u128 = 100 * POS_SCALE;
    const PRICE: u64 = ENTRY - ENTRY * 24 * 23 / 10_000;
    const RESIDUAL: u128 = 100;
    const SHARE: u128 = 3_333;
    const DEPOSITS: [u64; 5] = [
        100 * (ENTRY - PRICE) + RESIDUAL as u64,
        FUNDS[1],
        FUNDS[2],
        FUNDS[3],
        FUNDS[4],
    ];

    // Every nonflat remainder owes at least 599 maintenance atoms, exceeding
    // the 100-atom equity even before fees. Thus only the full close can work.
    let nominal_fee = fee(SIZE, PRICE, 5);
    assert!(RESIDUAL < production_risk_params().min_nonzero_mm_req);
    assert!(nominal_fee > RESIDUAL);
    let mut peak_cu = 0;
    let mut liquidations = 0;
    for authenticated in [false, true] {
        let report_price = if authenticated { PRICE } else { 940_000 };
        let reward = if authenticated {
            RESIDUAL * SHARE / 10_000
        } else {
            0
        };
        let mut reference = None;
        for publish_first in [false, true] {
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_abs_funding_e9_per_slot: 0,
                    ..production_risk_params()
                },
            );
            set_test_clock(&mut env, 1, 100);
            env.update_liquidation_fee_policy_with_cu(SHARE as u16);
            let feed = [0x55; 32];
            let initial = env.set_pyth_price_with_conf(&feed, ENTRY as i64, -6, 0, 100);
            env.try_configure_hybrid_asset_with_conf_filter_cu(
                0,
                1,
                0,
                [feed, [0; 32], [0; 32]],
                &[initial],
                1,
                100,
                0,
                0,
                1,
                0,
            )
            .unwrap();
            let owners: [Keypair; 5] = std::array::from_fn(|_| Keypair::new());
            let funded =
                std::array::from_fn::<_, 5, _>(|i| fund(&mut env, &owners[i], DEPOSITS[i]));
            let portfolios = funded.map(|pair| pair.0);
            let tokens = funded.map(|pair| pair.1);
            let [target, peer, trader_a, trader_b, keeper] = portfolios;
            let mut tracked = vec![env.market, env.mint, env.vault, initial];
            tracked.extend(portfolios);
            tracked.extend(tokens);
            env.trade_asset_with_cu(
                0,
                &owners[0],
                target,
                &owners[1],
                peer,
                SIZE as i128,
                ENTRY,
                0,
            );

            set_test_clock(&mut env, 5, 1_000);
            let advance = observe(&env, keeper, owners[4].pubkey(), Some(initial), None);
            peak_cu = peak_cu.max(submit(&mut env, &owners[4], &[advance], &tracked, None));
            let accepted = ENTRY - ENTRY * 24 * 4 / 10_000;
            let mark = ENTRY - (ENTRY - accepted) * (10_000 * 4 / 5) / 10_000;
            let move_bps = (u128::from(ENTRY - mark) * 10_000).div_ceil(u128::from(ENTRY));
            let required = (2 * 100 * u128::from(ENTRY) * move_bps).div_ceil(10_000);
            let trade_bps = (required * 10_000).div_ceil(2 * u128::from(accepted));
            let paid = fee(POS_SCALE, accepted, trade_bps);
            let discovery = 2 * paid;
            let trade_cu = env.trade_asset_with_cu(
                0,
                &owners[2],
                trader_a,
                &owners[3],
                trader_b,
                POS_SCALE as i128,
                900_000,
                0,
            );
            assert_cu_within("clipped reward paid discovery", trade_cu, TRADE_CU_LIMIT);
            let staged = env.market_state().1;
            assert_eq!(staged.assets[0].effective_price, ENTRY);
            assert_eq!(staged.assets[0].raw_oracle_target_price, mark);
            assert_eq!(staged.insurance, discovery);
            assert_eq!(staged.insurance_domain_budget_remaining_total, 0);
            assert!(discovery > nominal_fee);

            set_test_clock(&mut env, 28, 1_001);
            let fresh = env.set_pyth_price_with_conf(&feed, report_price as i64, -6, 0, 1_001);
            let conflicting =
                env.set_pyth_price_with_conf(&feed, report_price as i64 + 1, -6, 0, 1_001);
            tracked.extend([fresh, conflicting]);
            let custody = [env.mint, env.vault].map(|key| env.svm.get_account(&key));
            if publish_first {
                let before = values(&env, portfolios);
                let publish = observe(&env, keeper, owners[4].pubkey(), Some(fresh), None);
                peak_cu = peak_cu.max(submit(&mut env, &owners[4], &[publish], &tracked, None));
                assert_eq!(values(&env, portfolios), before);
            }
            let valid = observe(&env, target, owners[4].pubkey(), Some(fresh), Some(keeper));
            let invalid = observe(
                &env,
                target,
                owners[4].pubkey(),
                Some(conflicting),
                Some(keeper),
            );
            let mut landed = false;
            for _ in 0..6 {
                let before = env.market_state().1;
                let before_target = env.portfolio_state(target);
                let untouched = [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key));
                peak_cu = peak_cu.max(submit(
                    &mut env,
                    &owners[4],
                    &[valid.clone(), invalid.clone()],
                    &tracked,
                    Some((
                        3,
                        InstructionError::Custom(PercolatorError::OracleInvalid as u32),
                    )),
                ));
                peak_cu = peak_cu.max(submit(
                    &mut env,
                    &owners[4],
                    &[valid.clone()],
                    &tracked,
                    None,
                ));
                let after = env.market_state().1;
                let profile = state::read_asset_oracle_profile(
                    &env.svm.get_account(&env.market).unwrap().data,
                    0,
                )
                .unwrap();
                let elapsed = after.assets[0].slot_last - 5;
                assert_eq!(
                    after.assets[0].effective_price,
                    (ENTRY - ENTRY * 24 * elapsed / 10_000).max(report_price)
                );
                assert_eq!(after.assets[0].raw_oracle_target_price, report_price);
                assert_eq!(profile.last_good_oracle_slot, 28);
                assert_eq!(profile.oracle_target_publish_time, 1_001);
                assert_eq!(
                    profile.effective_price_provenance,
                    if authenticated && after.assets[0].effective_price == report_price {
                        percolator_prog::constants::EFFECTIVE_PRICE_PROVENANCE_AUTHENTICATED
                    } else {
                        percolator_prog::constants::EFFECTIVE_PRICE_PROVENANCE_TRADE_DRIVEN
                    }
                );
                assert_eq!(
                    [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key)),
                    untouched
                );
                assert_eq!(
                    [env.mint, env.vault].map(|key| env.svm.get_account(&key)),
                    custody
                );
                census(&env, portfolios);
                let closed = before.assets[0].oi_eff_long_q - after.assets[0].oi_eff_long_q;
                if closed == 0 {
                    assert_eq!(after.insurance, discovery);
                    assert_eq!(values(&env, portfolios)[4], DEPOSITS[4] as i128);
                    continue;
                }
                assert_eq!(before_target.capital.get(), RESIDUAL);
                assert_eq!(after.assets[0].effective_price, PRICE);
                assert_eq!(after.assets[0].slot_last, 28);
                assert_eq!(before_target.pnl.get(), 0);
                assert!(health_cert(&before_target).certified_liq_deficit > 0);
                assert_eq!(
                    closed, SIZE,
                    "input-derived full close, independent of observed fee"
                );
                let account = env.portfolio_state(target);
                assert_eq!(account.capital.get(), 0);
                assert_eq!(account.pnl.get(), 0);
                assert_eq!(health_cert(&account).certified_liq_deficit, 0);
                assert_eq!(
                    values(&env, portfolios)[4],
                    DEPOSITS[4] as i128 + reward as i128
                );
                assert_eq!(after.insurance, discovery + RESIDUAL - reward);
                let budget = if authenticated { RESIDUAL - reward } else { 0 };
                assert_eq!(after.insurance_domain_budget_remaining_total, budget);
                assert_eq!(
                    &after.insurance_domain_budget[..2],
                    &[budget / 2, budget.div_ceil(2)]
                );
                assert!(after.insurance_domain_budget[2..]
                    .iter()
                    .all(|amount| *amount == 0));
                assert_eq!(after.assets[0].oi_eff_long_q, POS_SCALE);
                assert_eq!(after.assets[0].oi_eff_short_q, POS_SCALE);
                liquidations += 1;
                landed = true;
                break;
            }
            assert!(landed, "a fully collected 100-atom penalty must land");
            let retained = env.market_state().1.insurance;
            peak_cu = peak_cu.max(submit(
                &mut env,
                &owners[4],
                &[valid],
                &tracked,
                Some((
                    2,
                    InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
                )),
            ));
            let payout = u128::from(DEPOSITS[4]) + reward;
            let cu = env
                .send(
                    env.withdraw_ix(keeper, payout),
                    vec![
                        AccountMeta::new(owners[4].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(keeper, false),
                        AccountMeta::new(tokens[4], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&owners[4]],
                )
                .unwrap();
            assert_cu_within("clipped reward SPL exit", cu, CUSTODY_CU_LIMIT);
            assert_eq!(u128::from(env.token_amount(tokens[4])), payout);
            assert_eq!(values(&env, portfolios)[4], 0);
            assert_eq!(env.market_state().1.insurance, retained);
            assert_eq!(env.svm.get_account(&env.mint), custody[0]);
            assert_eq!(
                u128::from(env.token_amount(env.vault)) + payout,
                DEPOSITS.iter().map(|v| u128::from(*v)).sum::<u128>()
            );
            census(&env, portfolios);
            let outcome = (values(&env, portfolios), retained, payout);
            if let Some(expected) = &reference {
                assert_eq!(
                    &outcome, expected,
                    "publication order cannot change clipped entitlement"
                );
            } else {
                reference = Some(outcome);
            }
            println!("clipped reward authenticated={authenticated} publish_first={publish_first}: price={PRICE}, nominal={nominal_fee}, collected={RESIDUAL}, reward={reward}, discovery={discovery}, payout={payout}, peak={peak_cu} CU");
        }
    }
    assert_eq!(liquidations, 4);
}
