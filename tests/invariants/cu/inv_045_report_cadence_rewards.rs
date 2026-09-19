//! INV-020/024/036/041/045/061/062/080: report cadence across paid-price catchup.
//! Fixed-target renewals and eager/delayed market cranks precede the same liquidation.
//! A still-fresh, older equal-price report rejects even after a liquidation prefix.
//! System/SPL/wrapper setup; only Clock and external oracle accounts are harness inputs.

use super::*;

#[test]
fn v16_program_paid_price_catchup_report_cadence_preserves_reward_and_rejects_regression() {
    const REPORT: u64 = 980_000;
    const SHARE: u128 = 3_333;
    let mut peak_cu = 0;
    let mut liquidations = 0;
    let mut rollbacks = 0;
    for endpoint in [8, 14] {
        let mut reference = None;
        for eager in [false, true] {
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_abs_funding_e9_per_slot: 0,
                    ..production_risk_params()
                },
            );
            set_test_clock(&mut env, 1, 100);
            env.update_liquidation_fee_policy_with_cu(SHARE as u16);
            let feed = [0x53; 32];
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
            let funded = std::array::from_fn::<_, 5, _>(|i| fund(&mut env, &owners[i], FUNDS[i]));
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
            let mut tracked = vec![env.market, env.mint, env.vault, initial, env.admin.pubkey()];
            tracked.extend(portfolios);
            tracked.extend(tokens);
            tracked.extend(owners.each_ref().map(Signer::pubkey));

            set_test_clock(&mut env, 5, 1_000);
            let clock_only = observe(&env, keeper, owners[4].pubkey(), Some(initial), None);
            submit(&mut env, &owners[4], &[clock_only], &tracked, None);
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
            // Four discovery slots allow 96 bps; alpha=4/5 moves the paid mark 7,680.
            let required = (2 * 100 * u128::from(ENTRY) * 77).div_ceil(10_000);
            let trade_bps = (required * 10_000).div_ceil(2 * u128::from(ACCEPTED_PRINT));
            let paid = fee(POS_SCALE, ACCEPTED_PRINT, trade_bps);
            let discovery = 2 * paid;
            let mut expected = FUNDS.map(i128::from);
            expected[2] -= paid as i128;
            expected[3] -= paid as i128;
            assert_eq!(values(&env, portfolios), expected);
            assert_eq!(env.market_state().1.assets[0].raw_oracle_target_price, MARK);
            assert_eq!(env.market_state().1.assets[0].effective_price, ENTRY);
            assert_eq!(env.market_state().1.insurance, discovery);
            let custody = [env.mint, env.vault].map(|key| env.svm.get_account(&key));

            let mut fresh = initial;
            let mut publications = 0;
            for slot in 6..=endpoint {
                if !eager && slot != 6 && slot != endpoint {
                    continue;
                }
                let now = 995 + slot as i64;
                set_test_clock(&mut env, slot, now);
                fresh = env.set_pyth_price_with_conf(&feed, REPORT as i64, -6, 0, now);
                tracked.push(fresh);
                let publication = observe(&env, keeper, owners[4].pubkey(), Some(fresh), None);
                peak_cu = peak_cu.max(submit(&mut env, &owners[4], &[publication], &tracked, None));
                publications += 1;
                // Quotient from the fixed public anchor, independent of crank count and cache.
                let price = (ENTRY - ENTRY * 24 * (slot - 5) / 10_000).max(REPORT);
                let (profile, group) = env.market_state();
                assert_eq!(
                    group.assets[0].effective_price, price,
                    "slot={slot}, eager={eager}"
                );
                assert_eq!(group.assets[0].raw_oracle_target_price, REPORT);
                assert_eq!(group.assets[0].slot_last, slot);
                assert_eq!(profile.mark_ewma_e6, price);
                assert_eq!(profile.oracle_target_publish_time, now);
                assert_eq!(profile.last_good_oracle_slot, slot);
                assert_eq!(
                    values(&env, portfolios),
                    expected,
                    "market-only discovery cannot pay"
                );
                assert_eq!(group.insurance, discovery);
                assert_eq!(group.insurance_domain_budget_remaining_total, 0);
            }
            assert_eq!(publications, if eager { endpoint - 5 } else { 2 });

            let price = (ENTRY - ENTRY * 24 * (endpoint - 5) / 10_000).max(REPORT);
            let caught_up = price == REPORT;
            let profile = state::read_asset_oracle_profile(
                &env.svm.get_account(&env.market).unwrap().data,
                0,
            )
            .unwrap();
            assert_eq!(
                profile.effective_price_provenance,
                if caught_up {
                    percolator_prog::constants::EFFECTIVE_PRICE_PROVENANCE_AUTHENTICATED
                } else {
                    percolator_prog::constants::EFFECTIVE_PRICE_PROVENANCE_TRADE_DRIVEN
                },
            );
            let now = 995 + endpoint as i64;
            let older = env.set_pyth_price_with_conf(&feed, REPORT as i64, -6, 0, now - 1);
            tracked.push(older);
            let valid = observe(&env, target, owners[4].pubkey(), Some(fresh), Some(keeper));
            let regressed = observe(&env, target, owners[4].pubkey(), Some(older), Some(keeper));
            let mut result = None;
            for _ in 0..6 {
                let before = env.market_state().1;
                let before_values = values(&env, portfolios);
                let before_cert = health_cert(&env.portfolio_state(target));
                // The suffix is temporally regressed, not expired or price-equivocating.
                peak_cu = peak_cu.max(submit(
                    &mut env,
                    &owners[4],
                    &[valid.clone(), regressed.clone()],
                    &tracked,
                    Some((
                        3,
                        InstructionError::Custom(PercolatorError::OracleStale as u32),
                    )),
                ));
                rollbacks += 1;
                peak_cu = peak_cu.max(submit(
                    &mut env,
                    &owners[4],
                    &[valid.clone()],
                    &tracked,
                    None,
                ));
                let after = env.market_state().1;
                assert_eq!(after.assets[0].effective_price, price);
                assert_eq!(after.assets[0].raw_oracle_target_price, REPORT);
                assert_eq!(
                    [env.mint, env.vault].map(|key| env.svm.get_account(&key)),
                    custody
                );
                let closed = before.assets[0].oi_eff_long_q - after.assets[0].oi_eff_long_q;
                if closed == 0 {
                    assert_eq!(values(&env, portfolios)[4], FUNDS[4] as i128);
                    assert_eq!(after.insurance, discovery);
                    continue;
                }
                assert!(before_cert.certified_liq_deficit > 0);
                assert!(closed < 100 * POS_SCALE);
                let penalty = fee(closed, price, 5);
                let potential_reward = penalty * SHARE / 10_000;
                assert!(potential_reward > 0);
                let reward = if caught_up { potential_reward } else { 0 };
                if !caught_up {
                    assert_ne!(penalty, fee(closed, REPORT, 5));
                }
                let mut after_values = before_values;
                after_values[0] -= penalty as i128;
                after_values[4] += reward as i128;
                assert_eq!(values(&env, portfolios), after_values);
                assert_eq!(
                    after_values[0],
                    FUNDS[0] as i128 - 100 * i128::from(ENTRY - price) - penalty as i128
                );
                assert_eq!(after.insurance, discovery + penalty - reward);
                let retained = if caught_up { penalty - reward } else { 0 };
                assert_eq!(
                    &after.insurance_domain_budget[..2],
                    &[retained / 2, retained.div_ceil(2)]
                );
                assert_eq!(after.insurance_domain_budget_remaining_total, retained);
                census(&env, portfolios);
                result = Some((closed, penalty, reward, after_values, after.insurance));
                liquidations += 1;
                break;
            }
            let result = result.expect("bounded liquidation must collect a nonzero fee");
            let payout = u128::from(FUNDS[4]) + result.2;
            let withdraw = env.withdraw_ix(keeper, payout);
            let cu = env
                .send(
                    withdraw,
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
            assert_cu_within("cadence keeper payout", cu, CUSTODY_CU_LIMIT);
            assert_eq!(u128::from(env.token_amount(tokens[4])), payout);
            assert_eq!(
                u128::from(env.token_amount(env.vault)) + payout,
                FUNDS.iter().map(|v| u128::from(*v)).sum::<u128>()
            );
            assert_eq!(values(&env, portfolios)[4], 0);
            census(&env, portfolios);
            if let Some(expected) = &reference {
                assert_eq!(
                    &result, expected,
                    "renewal/crank cadence changed liquidation economics"
                );
            } else {
                reference = Some(result);
            }
            println!("endpoint={endpoint} eager={eager}: price={price}, liquidation={result:?}, payout={payout}");
        }
    }
    assert_eq!(liquidations, 4);
    assert!(rollbacks >= liquidations);
    println!("paid-price cadence: liquidations={liquidations}, exact rollbacks={rollbacks}, peak CU={peak_cu}");
}
