//! INV-045 with INV-020/024/036/041/061/062, reopening 422:
//! liquidation-reward-provenance-follows-the-effective-price-until-catchup.
//!
//! Public route: System/SPL/ATA setup, InitMarket/InitPortfolio/Deposit,
//! ConfigureHybridOracle/ConfigureAuthMark, signed TradeNoCpi, PushAuthMark,
//! PermissionlessCrank and Withdraw. Only Clock, airdrops, program loading and
//! external Pyth reports are fixture inputs; no protocol bytes are installed.
//! Four histories cross a long/short AuthMark keeper with settlement before/after
//! receiving a fresh-report Hybrid reward. They check two earned receipts during
//! lag, full catchup, exact independent fees/shares, recipient exposure framing,
//! separate AuthMark PnL and equal owner/custody endpoints across settlement order.
//! Withdrawal rejects while exposed; a signed AuthMark close enables reward payout.
//! This adds exposed recipients and a second oracle mode to the flat-keeper tests.
//! CPI, underwater/shared-owner recipients, more providers, nonzero funding or
//! maintenance, arbitrary histories and terminal redemption remain unproven.
//! Row 422 stays OPEN; this is a finite conformance family, not a generic oracle.

use super::*;

fn submit(
    env: &mut V16CuEnv,
    signer: &Keypair,
    instructions: &[Instruction],
    tracked: &[Pubkey],
    rejection: Option<(u8, InstructionError)>,
) -> u64 {
    // Same two-asset bound as inv_020_mixed_provider_liquidation.
    submit_with_cu_limit(env, signer, instructions, tracked, rejection, 500_000)
}

fn refresh_ix(env: &V16CuEnv, account: Pubkey, signer: Pubkey, report: Pubkey) -> Instruction {
    let mut ix = observe(env, account, signer, Some(report), None);
    let mut observations = crank_observations_with_accounts(0, 1);
    observations.extend(crank_observations(1));
    ix.data = ProgInstruction::PermissionlessCrank {
        now_slot: u64::MAX,
        observations,
    }
    .encode();
    ix
}

fn settle_keeper(env: &mut V16CuEnv, owner: &Keypair, keeper: Pubkey, tracked: &[Pubkey]) -> u64 {
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(keeper, false),
        ],
        data: ProgInstruction::PermissionlessCrank {
            now_slot: u64::MAX,
            observations: crank_observations(1),
        }
        .encode(),
    };
    let mut peak = 0;
    for _ in 0..6 {
        let group = env.market_state().1;
        if group.assets[1].slot_last == env.svm.get_sysvar::<Clock>().slot
            && assert_current_certificate_matches_independent(
                "exposed keeper settlement",
                &group,
                &env.portfolio_state(keeper),
            )
            .unwrap()
        {
            return peak;
        }
        peak = peak.max(submit(env, owner, &[ix.clone()], tracked, None));
    }
    panic!("exposed keeper did not become current in six public cranks");
}

#[test]
fn v16_program_exposed_auth_keeper_reward_commutes_with_settlement_through_hybrid_catchup() {
    const SHARE: u128 = 3_333;
    const KEEPER_FUNDS: u64 = 10_000_000;
    const KEEPER_LOTS: i128 = 2;
    const FINAL_TARGET: u64 = 980_000;
    let first_price = ENTRY - ENTRY * 24 / 10_000;
    let second_price = first_price - first_price * 24 / 10_000;
    let mut peak_crank = 0;
    let mut peak_payout = 0;
    let mut liquidation_count = 0;
    for direction in [-1i128, 1] {
        let mut reference = None;
        for settle_first in [false, true] {
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_portfolio_assets: 2,
                    max_abs_funding_e9_per_slot: 0,
                    ..production_risk_params()
                },
            );
            set_test_clock(&mut env, 1, 100);
            env.update_liquidation_fee_policy_with_cu(SHARE as u16);
            let feed = [0x72; 32];
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
            .expect("public Hybrid configuration");
            env.configure_auth_mark_for_asset_as_admin(1, 1, ENTRY);
            let owners: [Keypair; 5] = std::array::from_fn(|_| Keypair::new());
            let mut funds = FUNDS;
            funds[4] = KEEPER_FUNDS;
            let funded = std::array::from_fn::<_, 5, _>(|i| fund(&mut env, &owners[i], funds[i]));
            let portfolios = funded.map(|pair| pair.0);
            let tokens = funded.map(|pair| pair.1);
            let [target, peer, trader_a, trader_b, keeper] = portfolios;
            env.trade_asset_with_cu(
                0,
                &owners[0],
                target,
                &owners[1],
                peer,
                (100 * POS_SCALE) as i128,
                ENTRY,
                0,
            );
            env.trade_asset_with_cu(
                1,
                &owners[4],
                keeper,
                &owners[1],
                peer,
                direction * KEEPER_LOTS * POS_SCALE as i128,
                ENTRY,
                0,
            );
            let keeper_legs = env.portfolio_state(keeper).legs;
            assert_eq!(keeper_legs[0].asset_index.get(), 1);
            assert_eq!(
                keeper_legs[0].basis_pos_q.get(),
                direction * KEEPER_LOTS * POS_SCALE as i128
            );
            let mut tracked = vec![env.market, env.mint, env.vault, initial];
            tracked.extend(portfolios);
            tracked.extend(tokens);
            tracked.extend(owners.each_ref().map(Signer::pubkey));
            let custody = [env.mint, env.vault].map(|key| env.svm.get_account(&key));
            let supply = funds.iter().map(|amount| *amount as u128).sum::<u128>();
            census(&env, portfolios);

            set_test_clock(&mut env, 5, 1_000);
            let advance = observe(&env, trader_a, owners[4].pubkey(), Some(initial), None);
            submit(&mut env, &owners[4], &[advance], &tracked, None);
            env.trade_asset_with_cu(
                0,
                &owners[2],
                trader_a,
                &owners[3],
                trader_b,
                POS_SCALE as i128,
                900_000,
                0,
            );
            let required = (2 * 100 * u128::from(ENTRY) * 77).div_ceil(10_000);
            let trade_bps = (required * 10_000).div_ceil(2 * u128::from(ACCEPTED_PRINT));
            let discovery = 2 * fee(POS_SCALE, ACCEPTED_PRINT, trade_bps);
            let staged = env.market_state().1;
            assert_eq!(staged.assets[0].effective_price, ENTRY);
            assert_eq!(staged.assets[0].raw_oracle_target_price, MARK);
            assert_eq!(staged.insurance, discovery);
            assert_eq!(staged.insurance_domain_budget_remaining_total, 0);
            assert_eq!(env.portfolio_state(keeper).legs, keeper_legs);

            let mut total_fee = 0;
            let mut total_reward = 0;
            let mut budgets = [0u128; 2];
            let mut episodes = Vec::new();
            for (phase, slot, raw, price, auth_price) in [
                (0, 6, MARK, first_price, 1_004_000),
                (1, 7, FINAL_TARGET, second_price, 1_005_000),
                (2, 14, FINAL_TARGET, FINAL_TARGET, 1_005_000),
            ] {
                let now = 995 + slot as i64;
                set_test_clock(&mut env, slot, now);
                env.push_auth_mark_for_asset_as_admin(1, u64::MAX, auth_price);
                let report = env.set_pyth_price_with_conf(&feed, raw as i64, -6, 0, now);
                tracked.push(report);
                let auth_before = env.market_state().1.assets[1];
                assert!(
                    auth_before.slot_last < slot,
                    "AuthMark has pending elapsed work"
                );
                assert_eq!(auth_before.raw_oracle_target_price, auth_price);
                census(&env, portfolios);
                if settle_first {
                    peak_crank =
                        peak_crank.max(settle_keeper(&mut env, &owners[4], keeper, &tracked));
                    assert_eq!(env.market_state().1.assets[1].effective_price, auth_price);
                }
                let retained =
                    observe(&env, target, owners[4].pubkey(), Some(report), Some(keeper));
                let mut liquidated = false;
                for _ in 0..6 {
                    let before = env.market_state().1;
                    let before_values = values(&env, portfolios);
                    let keeper_before = env.portfolio_state(keeper);
                    let before_cert = health_cert(&env.portfolio_state(target));
                    if phase == 2
                        && before.assets[0].effective_price == price
                        && before_cert.certified_liq_deficit == 0
                        && census(&env, portfolios)[0]
                    {
                        break;
                    }
                    let foreign = [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key));
                    peak_crank = peak_crank.max(submit(
                        &mut env,
                        &owners[4],
                        &[retained.clone()],
                        &tracked,
                        None,
                    ));
                    let current = census(&env, portfolios);
                    let (profile, after) = env.market_state();
                    assert_eq!(after.assets[0].effective_price, price);
                    assert_eq!(after.assets[0].raw_oracle_target_price, raw);
                    assert_eq!(profile.oracle_target_publish_time, now);
                    assert_eq!(profile.last_good_oracle_slot, slot);
                    assert_eq!(
                        after.assets[1], before.assets[1],
                        "Hybrid reward cannot settle AuthMark"
                    );
                    assert_eq!(
                        [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key)),
                        foreign
                    );
                    assert_eq!(
                        [env.mint, env.vault].map(|key| env.svm.get_account(&key)),
                        custody
                    );
                    let keeper_after = env.portfolio_state(keeper);
                    assert_eq!(keeper_after.legs, keeper_before.legs);
                    assert_eq!(keeper_after.pnl, keeper_before.pnl);
                    let closed = before.assets[0].oi_eff_long_q - after.assets[0].oi_eff_long_q;
                    if closed == 0 {
                        assert_eq!(
                            keeper_after, keeper_before,
                            "refresh cannot award a receipt"
                        );
                        assert_eq!(after.insurance, before.insurance);
                        assert_eq!(
                            after.insurance_domain_budget,
                            before.insurance_domain_budget
                        );
                        continue;
                    }
                    assert!(phase < 2 && before_cert.certified_liq_deficit > 0 && current[0]);
                    let penalty = fee(closed, price, 5);
                    let reward = penalty * SHARE / 10_000;
                    assert!(reward > 0 && closed < before.assets[0].oi_eff_long_q);
                    for wrong_price in [ENTRY, MARK, raw, ACCEPTED_PRINT, 900_000, auth_price] {
                        assert_ne!(penalty, fee(closed, wrong_price, 5));
                    }
                    let mut expected = before_values;
                    expected[0] -= penalty as i128;
                    expected[4] += reward as i128;
                    assert_eq!(values(&env, portfolios), expected);
                    assert_eq!(
                        keeper_after.capital.get(),
                        keeper_before.capital.get() + reward
                    );
                    let mut expected_keeper = keeper_before;
                    expected_keeper.capital =
                        percolator::V16PodU128::new(keeper_before.capital.get() + reward);
                    expected_keeper.health_cert.valid = 0;
                    assert_eq!(
                        keeper_after, expected_keeper,
                        "receipt changes only capital and certificate validity"
                    );
                    assert!(
                        !health_cert(&keeper_after).valid,
                        "credit invalidates the recipient certificate"
                    );
                    assert_eq!(after.insurance - before.insurance, penalty - reward);
                    total_fee += penalty;
                    total_reward += reward;
                    budgets[0] += (penalty - reward) / 2;
                    budgets[1] += (penalty - reward).div_ceil(2);
                    episodes.push((closed, penalty, reward));
                    liquidation_count += 1;
                    liquidated = true;
                    break;
                }
                assert_eq!(liquidated, phase < 2, "two receipts and no catchup bonus");
                if !settle_first {
                    assert_eq!(env.market_state().1.assets[1], auth_before);
                }
                peak_crank = peak_crank.max(settle_keeper(&mut env, &owners[4], keeper, &tracked));
                assert_eq!(env.market_state().1.assets[1].effective_price, auth_price);
                let keeper_value = KEEPER_FUNDS as i128
                    + total_reward as i128
                    + direction * KEEPER_LOTS * i128::from(auth_price - ENTRY);
                assert_eq!(
                    values(&env, portfolios)[4],
                    keeper_value,
                    "own PnL and receipts remain separate"
                );

                for _ in 0..8 {
                    for i in [1, 2, 3, 0, 4] {
                        if !census(&env, portfolios)[i] {
                            let ix = refresh_ix(&env, portfolios[i], owners[4].pubkey(), report);
                            peak_crank =
                                peak_crank.max(submit(&mut env, &owners[4], &[ix], &tracked, None));
                        }
                    }
                    if census(&env, portfolios).iter().all(|current| *current) {
                        break;
                    }
                }
                assert!(census(&env, portfolios).iter().all(|current| *current));
                assert_eq!(values(&env, portfolios)[4], keeper_value);
                let group = env.market_state().1;
                assert_eq!(group.insurance, discovery + total_fee - total_reward);
                assert_eq!(&group.insurance_domain_budget[..2], &budgets);
                assert!(group.insurance_domain_budget[2..]
                    .iter()
                    .all(|value| *value == 0));
                assert_eq!(
                    group.insurance_domain_budget_remaining_total,
                    budgets.iter().sum::<u128>()
                );
                assert_eq!(
                    group.insurance - group.insurance_domain_budget_remaining_total,
                    discovery
                );
                peak_crank = peak_crank.max(submit(
                    &mut env,
                    &owners[4],
                    &[retained],
                    &tracked,
                    Some((
                        2,
                        InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
                    )),
                ));
            }
            let active_withdraw = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(owners[4].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(keeper, false),
                    AccountMeta::new(tokens[4], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: env.withdraw_ix(keeper, total_reward).encode(),
            };
            submit(
                &mut env,
                &owners[4],
                &[active_withdraw],
                &tracked,
                Some((
                    2,
                    InstructionError::Custom(PercolatorError::EngineStale as u32),
                )),
            );
            let before_close = env.market_state().1;
            let close_values = values(&env, portfolios);
            assert_eq!(
                before_close.assets[1].oi_eff_long_q,
                KEEPER_LOTS as u128 * POS_SCALE
            );
            env.svm.expire_blockhash();
            let close_cu = env.trade_asset_with_cu(
                1,
                &owners[4],
                keeper,
                &owners[1],
                peer,
                -direction * KEEPER_LOTS * POS_SCALE as i128,
                1_005_000,
                0,
            );
            assert_cu_within("exposed keeper signed close", close_cu, TRADE_CU_LIMIT);
            assert!(!has_active_leg_for_asset(&env.portfolio_state(keeper), 1));
            assert!(!has_active_leg_for_asset(&env.portfolio_state(peer), 1));
            assert_eq!(values(&env, portfolios), close_values);
            let closed = env.market_state().1;
            assert_eq!(closed.assets[0], before_close.assets[0]);
            assert_eq!(closed.insurance, before_close.insurance);
            assert_eq!(
                closed.insurance_domain_budget,
                before_close.insurance_domain_budget
            );
            assert_eq!(
                [env.mint, env.vault].map(|key| env.svm.get_account(&key)),
                custody
            );
            let before_payout = values(&env, portfolios);
            peak_payout = peak_payout.max(reward_policy_catchup::withdraw_reward(
                &mut env,
                &owners[4],
                keeper,
                tokens[4],
                total_reward,
            ));
            let mut expected = before_payout;
            expected[4] -= total_reward as i128;
            assert_eq!(values(&env, portfolios), expected);
            assert_eq!(expected[4], KEEPER_FUNDS as i128 + direction * 10_000);
            assert!(tokens[..4].iter().all(|key| env.token_amount(*key) == 0));
            assert_eq!(env.token_amount(tokens[4]) as u128, total_reward);
            assert_eq!(env.svm.get_account(&env.mint), custody[0]);
            census(&env, portfolios);
            let group = env.market_state().1;
            assert_eq!(group.vault + total_reward, supply);
            assert_eq!(group.assets[0].effective_price, FINAL_TARGET);
            for asset in &group.assets[..2] {
                assert_eq!(asset.oi_eff_long_q, asset.oi_eff_short_q);
            }
            assert_eq!(group.assets[1].oi_eff_long_q, 0);
            let claims = u128::try_from(expected.iter().sum::<i128>()).unwrap();
            let residual = group.vault.checked_sub(claims + group.insurance).unwrap();
            println!("exposed keeper: direction={direction}, settle_first={settle_first}, episodes={episodes:?}, payout={total_reward}, owners={expected:?}, residual={residual}");
            let outcome = (
                episodes,
                expected,
                group.insurance,
                group.vault,
                budgets,
                residual,
                portfolios.map(|key| {
                    let account = env.portfolio_state(key);
                    (account.capital.get(), account.pnl.get(), account.legs)
                }),
            );
            if let Some(reference) = &reference {
                assert_eq!(
                    &outcome, reference,
                    "keeper settlement order preserves every entitlement"
                );
            } else {
                reference = Some(outcome);
            }
        }
    }
    assert_eq!(liquidation_count, 8);
    println!("row422 exposed AuthMark keeper: 4 histories, 8 rewarded liquidations, 12 healthy rollbacks, 4 active-withdraw rollbacks; crank={peak_crank}, payout={peak_payout} CU");
}
