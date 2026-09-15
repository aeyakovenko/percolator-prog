//! Row 426: repeated rewards invalidate an active recipient's renewed loss certificate.
//! Public System/SPL/ATA/wrapper histories; only Clock, signer SOL and provider fixtures
//! are supplied externally. Interrupted and uninterrupted histories share an exact ledger.

use super::*;
use crate::support::fuzz_model::assert_current_certificate_matches_snapshot_full_refresh;

#[derive(Debug, PartialEq, Eq)]
struct Checkpoint {
    assets: Vec<percolator::AssetStateV16>,
    certificates: [percolator::HealthCertV16; 4],
    value: [(u128, i128, u64); 4],
    stock: [u128; 4],
    domains: [u128; 8],
}

fn checkpoint(env: &V16CuEnv, portfolios: [Pubkey; 4], tokens: [Pubkey; 4]) -> Checkpoint {
    census(env, portfolios, tokens);
    let group = env.market_state().1;
    Checkpoint {
        certificates: portfolios.map(|key| health_cert(&env.portfolio_state(key))),
        value: std::array::from_fn(|i| {
            let account = env.portfolio_state(portfolios[i]);
            (
                account.capital.get(),
                account.pnl.get(),
                env.token_amount(tokens[i]),
            )
        }),
        stock: [group.c_tot, group.insurance, group.vault, group.pnl_pos_tot],
        domains: group.insurance_domain_budget.as_slice().try_into().unwrap(),
        assets: group.assets,
    }
}

fn current(env: &V16CuEnv, key: Pubkey) -> bool {
    assert_current_certificate_matches_snapshot_full_refresh(
        "repeated active rewards",
        &env.svm.get_account(&env.market).unwrap().data,
        &env.svm.get_account(&key).unwrap().data,
    )
    .unwrap()
}

#[test]
fn v16_program_repeated_active_rewards_preserve_renewed_observations_and_exact_exit() {
    let mut reference = None;
    let mut peak = 0;
    let mut liquidations = 0;
    let mut rollbacks = 0;
    let mut worlds = 0;
    for reverse in [false, true] {
        for interrupted in [false, true] {
            let mut env = inv018_public_spl_market_with_params(
                0,
                V16CuMarketParams {
                    max_portfolio_assets: 4,
                    initial_price: PRICE,
                    min_nonzero_mm_req: 599,
                    min_nonzero_im_req: 600,
                    maintenance_margin_bps: 1_000,
                    initial_margin_bps: 1_000,
                    max_price_move_bps_per_slot: 10,
                    max_accrual_dt_slots: 64,
                    min_funding_lifetime_slots: 64,
                    max_abs_funding_e9_per_slot: 0,
                    liquidation_fee_bps: 100,
                    liquidation_fee_cap: 10_000,
                    ..V16CuMarketParams::default()
                },
            );
            set_test_clock(&mut env, 0, 100);
            env.update_liquidation_fee_policy_with_cu(SHARE as u16);
            let feed = [0xd4; 32];
            let initial = env.set_pyth_price_with_conf(&feed, PRICE as i64, -6, 0, 100);
            env.try_configure_hybrid_asset_with_conf_filter_cu(
                0,
                1,
                0,
                [feed, [0; 32], [0; 32]],
                &[initial],
                0,
                100,
                0,
                0,
                100,
                0,
            )
            .unwrap();
            for asset in [1, 2, 3] {
                env.configure_auth_mark_for_asset_as_admin(asset, 0, PRICE);
            }
            let owners: [Keypair; 4] = std::array::from_fn(|_| Keypair::new());
            let funded = [0, 1, 2, 3].map(|i| funded_owner(&mut env, &owners[i], ENDOWMENTS[i]));
            let portfolios = funded.map(|pair| pair.0);
            let tokens = funded.map(|pair| pair.1);
            let [peer, target, keeper, keeper_peer] = portfolios;
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::set_authority(
                    &spl_token::ID,
                    &env.mint,
                    None,
                    spl_token::instruction::AuthorityType::MintTokens,
                    &env.admin.pubkey(),
                    &[],
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            for (asset, long, short) in [(0, 0, 1), (1, 0, 1), (2, 3, 2)] {
                env.trade_asset_with_cu(
                    asset,
                    &owners[long],
                    portfolios[long],
                    &owners[short],
                    portfolios[short],
                    POS_SCALE as i128,
                    PRICE,
                    0,
                );
            }
            let mut tracked = vec![
                env.market,
                env.mint,
                env.vault,
                initial,
                env.admin.pubkey(),
                solana_sdk::sysvar::clock::ID,
            ];
            tracked.extend(portfolios);
            tracked.extend(tokens);
            tracked.extend(owners.each_ref().map(Signer::pubkey));
            let custody = frame(
                &env,
                &[
                    env.mint, env.vault, tokens[0], tokens[1], tokens[2], tokens[3],
                ],
            );
            let full = if reverse { [3, 2, 1, 0] } else { [0, 1, 2, 3] };
            let target_only = if reverse { [1, 0] } else { [0, 1] };
            let mut history = Vec::new();
            let mut previous_report = initial;
            let mut total_penalty = 0;
            let mut total_reward = 0;
            let mut domains = [0; 8];
            let mut credits = 0;
            for episode in 1..=2u64 {
                let start = 64 * (episode - 1);
                let end = start + 64;
                let prices = [PRICE + episode * 40_000, PRICE + episode * 50_000];
                let keeper_price = PRICE + episode * 50_000;
                let timestamp = 100 + 2 * episode as i64;
                set_test_clock(&mut env, start, timestamp);
                env.push_auth_mark_for_asset_as_admin(1, u64::MAX, prices[1]);
                env.push_auth_mark_for_asset_as_admin(2, u64::MAX, keeper_price);
                let report =
                    env.set_pyth_price_with_conf(&feed, prices[0] as i64, -6, 0, timestamp);
                tracked.push(report);
                let stage = observation(&env, target, &owners[2], Some(keeper), report, &full);
                peak = peak.max(transact(
                    &mut env,
                    &[&owners[2]],
                    &[stage.clone()],
                    &tracked,
                    None,
                ));
                history.push(checkpoint(&env, portfolios, tokens));
                let before = frame(&env, &portfolios);
                set_test_clock(&mut env, end, timestamp + 1);
                peak = peak.max(transact(&mut env, &[&owners[2]], &[stage], &tracked, None));
                assert_eq!(frame(&env, &portfolios), before, "market-only catchup");
                assert_eq!(
                    [0, 1, 2].map(|i| env.market_state().1.assets[i].slot_last),
                    [start + 32; 3]
                );
                assert!(!current(&env, target));
                history.push(checkpoint(&env, portfolios, tokens));
                let refresh =
                    observation(&env, target, &owners[2], Some(keeper), report, &target_only);
                peak = peak.max(transact(
                    &mut env,
                    &[&owners[2]],
                    &[refresh],
                    &tracked,
                    Some((2, PercolatorError::EngineNonProgress)),
                ));
                let report =
                    env.set_pyth_price_with_conf(&feed, prices[0] as i64, -6, 0, timestamp + 1);
                rollbacks += 1;
                tracked.push(report);
                let refresh =
                    observation(&env, target, &owners[2], Some(keeper), report, &target_only);
                peak = peak.max(transact(
                    &mut env,
                    &[&owners[2]],
                    &[refresh.clone()],
                    &tracked,
                    None,
                ));
                assert!(current(&env, target));
                assert_eq!(env.svm.get_account(&keeper).unwrap(), before[2].1);
                for (asset, price) in env.market_state().1.assets[..2].iter().zip(prices) {
                    assert_eq!(
                        (
                            asset.slot_last,
                            asset.effective_price,
                            asset.raw_oracle_target_price
                        ),
                        (end, price, price)
                    );
                    assert_eq!((asset.f_long_num, asset.f_short_num), (0, 0));
                }
                let mut deficit = health_cert(&env.portfolio_state(target)).certified_liq_deficit;
                assert!(deficit > 0);
                let mut actions = 0;
                while deficit > 0 {
                    assert!(actions < 4, "bounded two-leg liquidation");
                    if !current(&env, target) {
                        let epoch = env.portfolio_position_epoch(target);
                        peak = peak.max(transact(
                            &mut env,
                            &[&owners[2]],
                            &[refresh.clone()],
                            &tracked,
                            None,
                        ));
                        assert_eq!(
                            env.portfolio_position_epoch(target),
                            epoch,
                            "recertification only"
                        );
                    }
                    assert!(current(&env, target));
                    let before_group = env.market_state().1;
                    let before_target = env.portfolio_state(target);
                    let before_keeper = env.portfolio_state(keeper);
                    let keeper_leg = active_leg_for_asset(&before_keeper, 2);
                    let keeper_peer_before = env.svm.get_account(&keeper_peer);
                    let reward_action =
                        observation(&env, target, &owners[2], Some(keeper), report, &[]);
                    let keeper_refresh = observation(&env, keeper, &owners[2], None, report, &full);
                    if interrupted {
                        let stale =
                            observation(&env, keeper, &owners[2], None, previous_report, &[0]);
                        // The suffix follows both a new reward and complete recipient recertification.
                        peak = peak.max(transact(
                            &mut env,
                            &[&owners[2]],
                            &[reward_action.clone(), keeper_refresh.clone(), stale],
                            &tracked,
                            Some((4, PercolatorError::OracleStale)),
                        ));
                        rollbacks += 1;
                    }
                    peak = peak.max(transact(
                        &mut env,
                        &[&owners[2]],
                        &[reward_action],
                        &tracked,
                        None,
                    ));
                    let after = env.market_state().1;
                    let closed = [0, 1].map(|i| {
                        before_group.assets[i].oi_eff_short_q - after.assets[i].oi_eff_short_q
                    });
                    assert_eq!(closed.iter().filter(|&&q| q > 0).count(), 1);
                    let asset = usize::from(closed[1] > 0);
                    let penalty = (closed[asset] * u128::from(prices[asset]))
                        .div_ceil(POS_SCALE)
                        .div_ceil(100)
                        .min(10_000);
                    let reward = penalty * SHARE / 10_000;
                    assert!(reward > 0);
                    total_penalty += penalty;
                    total_reward += reward;
                    let insurance = penalty - reward;
                    domains[2 * asset] += insurance / 2;
                    domains[2 * asset + 1] += insurance - insurance / 2;
                    assert_eq!(
                        env.portfolio_state(target).capital.get(),
                        before_target.capital.get() - penalty
                    );
                    assert_eq!(
                        env.portfolio_state(keeper).capital.get(),
                        before_keeper.capital.get() + reward
                    );
                    assert!(!health_cert(&env.portfolio_state(keeper)).valid);
                    assert_eq!(
                        active_leg_for_asset(&env.portfolio_state(keeper), 2),
                        keeper_leg
                    );
                    assert_eq!(env.svm.get_account(&keeper_peer), keeper_peer_before);
                    assert_eq!(after.insurance, total_penalty - total_reward);
                    assert_eq!(after.insurance_domain_budget.as_slice(), domains);
                    assert_eq!(
                        before_group.c_tot + before_group.insurance,
                        after.c_tot + after.insurance
                    );
                    assert!(current(&env, target));
                    let remaining = health_cert(&env.portfolio_state(target)).certified_liq_deficit;
                    assert!(remaining < deficit);
                    deficit = remaining;
                    history.push(checkpoint(&env, portfolios, tokens));
                    if after.assets[2].slot_last < end {
                        let omitted =
                            observation(&env, keeper, &owners[2], None, report, &target_only);
                        peak = peak.max(transact(
                            &mut env,
                            &[&owners[2]],
                            &[omitted],
                            &tracked,
                            Some((2, PercolatorError::EngineNonProgress)),
                        ));
                        rollbacks += 1;
                    }
                    peak = peak.max(transact(
                        &mut env,
                        &[&owners[2]],
                        &[keeper_refresh],
                        &tracked,
                        None,
                    ));
                    assert!(current(&env, keeper));
                    let equity = ENDOWMENTS[2] + total_reward - u128::from(keeper_price - PRICE);
                    let account = env.portfolio_state(keeper);
                    let cert = health_cert(&account);
                    assert_eq!((account.capital.get(), account.pnl.get()), (equity, 0));
                    assert_eq!(cert.certified_equity, equity as i128);
                    assert_eq!(cert.certified_initial_req, u128::from(keeper_price) / 10);
                    assert_eq!(cert.certified_maintenance_req, cert.certified_initial_req);
                    assert_eq!(cert.certified_liq_deficit, 0);
                    assert_eq!(
                        active_leg_for_asset(&account, 2).basis_pos_q,
                        -(POS_SCALE as i128)
                    );
                    assert_eq!(
                        (
                            env.market_state().1.assets[2].slot_last,
                            env.market_state().1.assets[2].effective_price
                        ),
                        (end, keeper_price)
                    );
                    assert_eq!(
                        frame(
                            &env,
                            &[env.mint, env.vault, tokens[0], tokens[1], tokens[2], tokens[3]]
                        ),
                        custody
                    );
                    history.push(checkpoint(&env, portfolios, tokens));
                    actions += 1;
                    credits += 1;
                    liquidations += 1;
                }
                for account in [peer, keeper_peer, keeper] {
                    let ix = observation(&env, account, &owners[2], None, report, &full);
                    peak = peak.max(transact(&mut env, &[&owners[2]], &[ix], &tracked, None));
                    assert!(current(&env, account));
                }
                history.push(checkpoint(&env, portfolios, tokens));
                previous_report = report;
            }
            assert!(
                credits >= 3,
                "one active recipient must cross repeated credits and recertifications"
            );
            let target_before_exit = env.svm.get_account(&target);
            let close = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(owners[2].pubkey(), true),
                    AccountMeta::new(owners[3].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(keeper, false),
                    AccountMeta::new(keeper_peer, false),
                ],
                data: env
                    .trade_no_cpi_ix(
                        keeper,
                        keeper_peer,
                        2,
                        POS_SCALE as i128,
                        PRICE + 100_000,
                        0,
                    )
                    .encode(),
            };
            peak = peak.max(transact(
                &mut env,
                &[&owners[2], &owners[3]],
                &[close],
                &tracked,
                None,
            ));
            let payout = ENDOWMENTS[2] + total_reward - 100_000;
            assert_eq!(env.portfolio_state(keeper).capital.get(), payout);
            assert!(!has_active_leg_for_asset(&env.portfolio_state(keeper), 2));
            let withdrawal = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(owners[2].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(keeper, false),
                    AccountMeta::new(tokens[2], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: env.withdraw_ix(keeper, payout).encode(),
            };
            if interrupted {
                let stale = observation(&env, keeper, &owners[2], None, initial, &[0]);
                peak = peak.max(transact(
                    &mut env,
                    &[&owners[2]],
                    &[withdrawal.clone(), stale],
                    &tracked,
                    Some((3, PercolatorError::OracleStale)),
                ));
                rollbacks += 1;
            }
            peak = peak.max(transact(
                &mut env,
                &[&owners[2]],
                &[withdrawal],
                &tracked,
                None,
            ));
            assert_eq!(u128::from(env.token_amount(tokens[2])), payout);
            assert_eq!(env.portfolio_state(keeper).capital.get(), 0);
            assert_eq!(
                env.portfolio_state(keeper_peer).capital.get(),
                ENDOWMENTS[3]
            );
            assert_eq!(env.portfolio_state(keeper_peer).pnl.get(), 100_000);
            assert_eq!(env.svm.get_account(&target), target_before_exit);
            assert_eq!(
                env.market_state().1.insurance_domain_budget.as_slice(),
                domains
            );
            assert_eq!(
                u128::from(env.token_amount(env.vault)) + payout,
                ENDOWMENTS.iter().sum()
            );
            history.push(checkpoint(&env, portfolios, tokens));
            if let Some(expected) = &reference {
                assert_eq!(
                    &history, expected,
                    "reverse={reverse}, interrupted={interrupted}"
                );
            } else {
                reference = Some(history);
            }
            println!("repeated active recipient: reverse={reverse}, interrupted={interrupted}, credits={credits}, penalty={total_penalty}, reward={total_reward}, payout={payout}");
            worlds += 1;
        }
    }
    assert_eq!(worlds, 4);
    assert_eq!(rollbacks, 16 + liquidations / 2 + 2);
    println!("row426 repeated active rewards: {worlds} histories, {liquidations} rewards, {rollbacks} exact rollbacks; peak={peak} CU");
}
