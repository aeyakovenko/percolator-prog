//! INV-045 / row422: authenticated reward prices through actual target catchup.
//! The report stays fixed while the effective price moves, unlike the parent test's
//! report-to-price convergence. This does not cover trade-origin mark attribution.

use super::*;

fn run_catchup(
    upward: bool,
    catchup_slots: u64,
    liquidate_at: u64,
    reward_bps: u16,
    target_first: bool,
    renew_report: bool,
) -> RewardOutcome {
    const QUANTITY: u128 = 100 * POS_SCALE;
    const STEP: u64 = 2_400;
    const FUNDS: [u128; 3] = [5_100_000, 100_000_000, 1_000];
    let price_at = |elapsed: u64| {
        let movement = STEP * elapsed.min(catchup_slots);
        if upward {
            ENTRY + movement
        } else {
            ENTRY - movement
        }
    };
    let report_price = price_at(catchup_slots);
    let label = format!(
        "up={upward}/slots={catchup_slots}/liquidate_at={liquidate_at}/share={reward_bps}/target_first={target_first}/renew={renew_report}"
    );
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_abs_funding_e9_per_slot: 0,
        ..production_risk_params()
    });
    env.update_liquidation_fee_policy_with_cu(reward_bps);
    set_test_clock(&mut env, 1, 100);
    let feed = [0x47; 32];
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
        1_000,
        0,
    )
    .expect("public authenticated Hybrid configuration");
    let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
    let portfolios = owners.each_ref().map(|owner| env.create_portfolio(owner));
    let [target, peer, keeper] = portfolios;
    let sources: Vec<_> = (0..3)
        .map(|i| env.deposit(&owners[i], portfolios[i], FUNDS[i]))
        .collect();
    let size = if upward {
        -(QUANTITY as i128)
    } else {
        QUANTITY as i128
    };
    env.trade_asset_with_cu(0, &owners[0], target, &owners[1], peer, size, ENTRY, 0);
    assert_eq!(
        actor_values(&env, portfolios),
        FUNDS.map(|value| value as i128)
    );
    let vault = env.token_amount(env.vault);
    let mut keys = vec![env.market, env.mint, env.vault, initial, env.admin.pubkey()];
    keys.extend(portfolios);
    keys.extend(owners.each_ref().map(Signer::pubkey));
    keys.extend(sources);
    let immutable_keys: Vec<_> = keys
        .iter()
        .copied()
        .filter(|key| *key != env.market && !portfolios.contains(key))
        .collect();
    let immutable = frame(&env, &immutable_keys);
    let mut total_fee = 0;
    let mut total_reward = 0;
    let mut phases = Vec::new();
    let mut max_cu = 0;
    let mut distinguishing_fees = 0;
    let mut fresh = initial;

    // One extra slot after convergence must preserve the healthy fixed point.
    for elapsed in 1..=catchup_slots + 1 {
        let slot = elapsed + 1;
        let now = 100 + elapsed as i64;
        let expected_price = price_at(elapsed);
        set_test_clock(&mut env, slot, now);
        if elapsed == 1 || renew_report {
            fresh = env.set_pyth_price_with_conf(&feed, report_price as i64, -6, 0, now);
            keys.push(fresh);
        }
        let stale = env.set_pyth_price_with_conf(&feed, report_price as i64, -6, 0, now - 61);
        keys.push(stale);
        let before_stale = frame(&env, &keys);
        let error = reward_crank(
            &mut env,
            target,
            (&owners[2], keeper),
            crank_observations_with_accounts(0, 1),
            &[stale],
        )
        .expect_err("stale evidence cannot advance catchup or pay a reward");
        assert!(error.contains("Custom(27)"), "{label}/{slot}: {error}");
        assert_eq!(
            frame(&env, &keys),
            before_stale,
            "{label}/{slot}: stale rollback"
        );

        if !target_first || elapsed < liquidate_at {
            let before = frame(&env, &keys);
            let values = actor_values(&env, portfolios);
            env.svm.expire_blockhash();
            let result = env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations_with_accounts(0, 1),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(keeper, false),
                    AccountMeta::new_readonly(fresh, false),
                ],
                &[],
            );
            match result {
                Ok(cu) => max_cu = max_cu.max(cu),
                Err(error) => {
                    assert!(is_engine_non_progress_error(&error), "{label}: {error}");
                    assert_eq!(frame(&env, &keys), before);
                }
            }
            assert_eq!(
                actor_values(&env, portfolios),
                values,
                "publication cannot pay"
            );
        }

        if elapsed < liquidate_at {
            let after = env.market_state().1;
            assert_eq!(after.assets[0].effective_price, expected_price);
            assert_eq!(after.assets[0].raw_oracle_target_price, report_price);
            assert_eq!(after.assets[0].oi_eff_long_q, QUANTITY);
            assert_eq!(after.insurance, 0);
            assert_eq!(
                actor_values(&env, portfolios),
                FUNDS.map(|value| value as i128)
            );
            phases.push(RewardPhase {
                price: expected_price,
                remaining_q: QUANTITY,
                fee: 0,
                reward: 0,
                actor_value: actor_values(&env, portfolios),
                insurance: after.insurance,
            });
            continue;
        }

        let mut phase_fee = 0;
        let mut phase_reward = 0;
        let mut reached_health = false;
        for _ in 0..6 {
            let before = env.market_state().1;
            let account_before = env.portfolio_state(target);
            let values = actor_values(&env, portfolios);
            let peer_before = env.svm.get_account(&peer);
            let checkpoint = frame(&env, &keys);
            let result = reward_crank(
                &mut env,
                target,
                (&owners[2], keeper),
                crank_observations_with_accounts(0, 1),
                &[fresh],
            );
            match result {
                Ok(cu) => max_cu = max_cu.max(cu),
                Err(error) => {
                    assert!(
                        is_engine_non_progress_error(&error),
                        "{label}/{slot}: {error}"
                    );
                    assert_eq!(frame(&env, &keys), checkpoint);
                    assert_eq!(health_cert(&account_before).certified_liq_deficit, 0);
                    reached_health = true;
                    break;
                }
            }
            let (profile, after) = env.market_state();
            assert_eq!(
                after.assets[0].effective_price, expected_price,
                "{label}/{slot}"
            );
            assert_eq!(profile.mark_ewma_e6, expected_price);
            assert_eq!(after.assets[0].raw_oracle_target_price, report_price);
            assert_eq!(profile.oracle_target_price_e6, report_price);
            assert_eq!(
                profile.oracle_target_publish_time,
                if renew_report { now } else { 101 }
            );
            assert_eq!(
                profile.last_good_oracle_slot,
                if renew_report { slot } else { 2 }
            );
            assert_eq!(env.svm.get_account(&peer), peer_before);
            assert_eq!(env.token_amount(env.vault), vault);
            assert_eq!(after.vault, u128::from(vault));
            let closed = before.assets[0].oi_eff_long_q - after.assets[0].oi_eff_long_q;
            let values_after = actor_values(&env, portfolios);
            if closed == 0 {
                assert_eq!(values_after[2], values[2], "refresh cannot pay");
                assert_eq!(after.insurance, before.insurance);
                continue;
            }
            assert_eq!(
                before.assets[0].effective_price, expected_price,
                "liquidation must follow current-price refresh"
            );
            assert!(health_cert(&account_before).certified_liq_deficit > 0);
            assert!(closed < before.assets[0].oi_eff_long_q);
            let fee = fee_at_price(closed, expected_price);
            let reward = fee * u128::from(reward_bps) / 10_000;
            assert!(fee > 0 && reward > 0, "{label}: nonvacuous reward");
            distinguishing_fees += usize::from(fee != fee_at_price(closed, report_price));
            assert_eq!(
                values[0] - values_after[0],
                fee as i128,
                "{label}: target fee"
            );
            assert_eq!(values_after[1], values[1]);
            assert_eq!(
                values_after[2] - values[2],
                reward as i128,
                "{label}: keeper share"
            );
            assert_eq!(after.insurance - before.insurance, fee - reward);
            assert_eq!(
                after.assets[0].oi_eff_long_q,
                after.assets[0].oi_eff_short_q
            );
            assert_eq!(
                health_cert(&env.portfolio_state(target)).certified_liq_deficit,
                0
            );
            phase_fee += fee;
            phase_reward += reward;
        }
        assert!(
            reached_health,
            "{label}/{slot}: bounded healthy fixed point"
        );
        if elapsed == liquidate_at {
            assert!(
                phase_fee > 0 && phase_reward > 0,
                "{label}/{slot}: actual liquidation"
            );
        } else if elapsed > catchup_slots {
            assert_eq!(
                (phase_fee, phase_reward),
                (0, 0),
                "catchup cannot repeat a charge"
            );
        }
        total_fee += phase_fee;
        total_reward += phase_reward;
        let before_peer = frame(&env, &keys);
        match reward_crank(
            &mut env,
            peer,
            (&owners[2], keeper),
            crank_observations_with_accounts(0, 1),
            &[fresh],
        ) {
            Ok(cu) => max_cu = max_cu.max(cu),
            Err(error) => {
                assert!(is_engine_non_progress_error(&error), "{label}: {error}");
                assert_eq!(frame(&env, &keys), before_peer);
            }
        }
        assert_eq!(
            env.portfolio_state(keeper).capital.get(),
            FUNDS[2] + total_reward
        );
        assert_eq!(env.market_state().1.insurance, total_fee - total_reward);
        phases.push(RewardPhase {
            price: expected_price,
            remaining_q: env.market_state().1.assets[0].oi_eff_long_q,
            fee: phase_fee,
            reward: phase_reward,
            actor_value: actor_values(&env, portfolios),
            insurance: total_fee - total_reward,
        });
    }
    if liquidate_at < catchup_slots {
        assert!(
            distinguishing_fees > 0,
            "{label}: effective-vs-raw fee oracle must discriminate"
        );
    }
    assert_eq!(frame(&env, &immutable_keys), immutable);
    let remaining = env.market_state().1.assets[0].oi_eff_long_q as i128;
    let exit_size = if upward { remaining } else { -remaining };
    env.svm.expire_blockhash();
    let exit_cu = env.trade_asset_with_cu(
        0,
        &owners[0],
        target,
        &owners[1],
        peer,
        exit_size,
        report_price,
        0,
    );
    assert_cu_within(&label, exit_cu, TRADE_CU_LIMIT);
    for portfolio in [target, peer] {
        assert!(!has_active_leg_for_asset(
            &env.portfolio_state(portfolio),
            0
        ));
    }
    let before_payout = actor_values(&env, portfolios);
    let payout = FUNDS[2] + total_reward;
    let (destination, withdraw_cu) = env.withdraw_with_cu(&owners[2], keeper, payout);
    assert_cu_within(&label, withdraw_cu, CUSTODY_CU_LIMIT);
    assert_eq!(u128::from(env.token_amount(destination)), payout);
    assert_eq!(
        u128::from(env.token_amount(env.vault)),
        u128::from(vault) - payout
    );
    let after = env.market_state().1;
    assert_eq!(after.assets[0].oi_eff_long_q, 0);
    assert_eq!(after.assets[0].oi_eff_short_q, 0);
    assert_eq!(after.insurance, total_fee - total_reward);
    assert_eq!(after.vault, u128::from(env.token_amount(env.vault)));
    let final_values = actor_values(&env, portfolios);
    assert_eq!(final_values, [before_payout[0], before_payout[1], 0]);
    assert_cu_within(&label, max_cu, CRANK_CU_LIMIT);
    println!("{label}: fees={total_fee}, reward={total_reward}, discriminating={distinguishing_fees}, crank_max={max_cu}, exit={exit_cu}, withdraw={withdraw_cu}");
    RewardOutcome {
        phases,
        payout,
        final_actor_value: final_values,
        final_insurance: after.insurance,
        final_vault: after.vault,
    }
}

#[test]
fn v16_program_reward_price_tracks_actual_catchup_across_report_and_crank_orders() {
    let mut worlds = 0;
    for upward in [false, true] {
        for catchup_slots in [3, 5] {
            for liquidate_at in [1, catchup_slots - 1, catchup_slots] {
                for reward_bps in [3_333, 10_000] {
                    let mut baseline = None;
                    for target_first in [false, true] {
                        for renew_report in [false, true] {
                            let outcome = run_catchup(
                                upward,
                                catchup_slots,
                                liquidate_at,
                                reward_bps,
                                target_first,
                                renew_report,
                            );
                            if let Some(expected) = &baseline {
                                assert_eq!(&outcome, expected,
                                "publication order/report renewal cannot change the same price history");
                            } else {
                                baseline = Some(outcome);
                            }
                            worlds += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(worlds, 96);
}
