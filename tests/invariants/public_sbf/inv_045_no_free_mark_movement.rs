//! INV-045 - No free mark movement.
//!
//! Normative obligation: Every mark movement remains elapsed-time bounded and economically paid across every trade route.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr260_pending_ewma_inheritance_rejects_then_trades_on_every_route`, `v16_program_pr282_pending_ewma_target_override_rejects_without_value_drift`, `v16_program_pr264_pr265_pr332_pr333_targets_stage_before_stale_cpi`, `v16_program_pr356_pending_mark_fee_sync_rejects_then_preserves_terminal_value`, `v16_program_pr369_one_sided_cpi_fee_cannot_subsidize_mark_gain`, `v16_program_pr225_mark_movement_fee_is_nonwithdrawable_and_terminally_burned`, `v16_program_pr280_trade_driven_liquidation_penalty_is_not_reclaimable`, `v16_program_fresh_hybrid_oracle_liquidation_reward_remains_enabled`, and `v16_program_fresh_hybrid_report_does_not_reenable_stale_trade_liquidation_reward`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: PR260, PR264/265/332/333, PR282, PR356, and PR369 are fixed-pin regressions
//! covering target staging, stale-admission rollback, post-catch-up liveness, target-aware fees,
//! authenticated mark/fee ordering, bilateral CPI fee support, nonwithdrawable movement reserves,
//! and nonreclaimable trade-driven liquidation penalties. The cross-mode Hybrid regression proves
//! freshness cannot erase stale-trade provenance before effective-price catch-up, while its fresh
//! control and terminal catch-up prove ordinary authenticated rewards are preserved.

use super::*;

#[test]
fn v16_program_pr260_pending_ewma_inheritance_rejects_then_trades_on_every_route() {
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        let reproduction = reproduce_pending_ewma_inheritance([0x60; 32], route)
            .unwrap_or_else(|error| panic!("PR 260 {route:?} protection failed: {error}"));
        assert_eq!(reproduction.blocker, KnownBlocker::PendingEwmaInheritance);
        assert!(reproduction.pending_mark > 1_000_000);
        assert!(reproduction.applied_mark > 1_000_000);
        assert!(reproduction.seed_cost > 0);
        assert!(reproduction.pending_admission_rejected);
        assert!(reproduction.rejected_exact_rollback);
        assert!(reproduction.post_commit_trade_landed);
        assert!(reproduction.post_commit_exit_landed);
        assert_eq!(reproduction.attacker_gain, 0);
        assert_eq!(reproduction.victim_loss, 0);
        assert_eq!(reproduction.attacker_principal_withdrawn, 100_000_000);
    }
}

#[test]
fn v16_program_pr282_pending_ewma_target_override_rejects_without_value_drift() {
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        let reproduction = reproduce_pending_ewma_target_override([0x82; 32], route)
            .unwrap_or_else(|error| panic!("PR 282 {route:?} protection failed: {error}"));
        assert_eq!(
            reproduction.blocker,
            KnownBlocker::PendingEwmaTargetOverride
        );
        assert_eq!(reproduction.route, route);
        assert_eq!(reproduction.attack_target, reproduction.control_target);
        assert!(reproduction.override_rejected);
        assert!(reproduction.rejected_exact_rollback);
        assert_eq!(reproduction.movement_fee, 0);
        assert_eq!(reproduction.displaced_victim_pnl, 0);
        assert_eq!(reproduction.attacker_profit, 0);
        assert_eq!(reproduction.attacker_withdrawn, 24_000_000_000);
        assert_eq!(reproduction.victim_withdrawn, 20_000_000_000);
    }
}

#[test]
fn v16_program_pr264_pr265_pr332_pr333_targets_stage_before_stale_cpi() {
    for case in [
        TargetStagingCase::AuthMarkPush,
        TargetStagingCase::EwmaMarkPush,
        TargetStagingCase::EwmaSingleTrade,
        TargetStagingCase::EwmaBatchTrade,
    ] {
        let reproduction = reproduce_unstaged_mark_target([0x32; 32], case)
            .unwrap_or_else(|error| panic!("{case:?} target-staging protection failed: {error}"));
        assert_eq!(reproduction.blocker, KnownBlocker::UnstagedMarkTarget);
        assert_eq!(reproduction.case, case);
        assert_eq!(reproduction.engine_target, reproduction.wrapper_target);
        assert!(reproduction.engine_epoch_advanced);
        assert!(reproduction.stale_increase_rejected);
        assert!(reproduction.rejected_exact_rollback);
        assert!(reproduction.lagging_risk_reduction_landed);
        assert!(reproduction.post_commit_trade_landed);
        assert!(reproduction.post_commit_exit_landed);
        assert_eq!(reproduction.moved_engine_mark, reproduction.wrapper_target);
        assert_eq!(reproduction.attacker_profit, 0);
        assert_eq!(reproduction.victim_capital_loss, 0);
        assert!(reproduction.max_cu < support::v16_svm::TX_CU_LIMIT);
    }
}

#[test]
fn v16_program_pr356_pending_mark_fee_sync_rejects_then_preserves_terminal_value() {
    let reproduction = reproduce_pending_mark_fee_reward([0x56; 32])
        .unwrap_or_else(|error| panic!("PR 356 fixed route failed: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::PendingMarkFeeReward);
    assert!(reproduction.pending_sync_rejected_lock);
    assert!(reproduction.pending_sync_exact_rollback);
    assert_eq!(reproduction.control_reward, 0);
    assert_eq!(reproduction.reordered_reward, 0);
    assert_eq!(reproduction.reordered_reward, reproduction.control_reward);
    assert_eq!(
        reproduction.reordered_winner_payout,
        reproduction.control_winner_payout
    );
    assert_eq!(reproduction.extracted_reward, reproduction.control_reward);
}

#[test]
fn v16_program_pr369_one_sided_cpi_fee_cannot_subsidize_mark_gain() {
    for mode in [BilateralFeeMode::Ewma, BilateralFeeMode::HybridAfterHours] {
        for route in [TradeRoute::Cpi, TradeRoute::BatchCpi] {
            let reproduction = reproduce_bilateral_fee_support([0x69; 32], mode, route)
                .unwrap_or_else(|error| {
                    panic!("PR 369 {mode:?} {route:?} fixed route failed: {error}")
                });
            assert_eq!(reproduction.blocker, KnownBlocker::BilateralFeeSupport);
            assert_eq!(reproduction.mode, mode);
            assert_eq!(reproduction.route, route);
            assert!(reproduction.queued_mark >= reproduction.setup_mark);
            assert_eq!(reproduction.coalition_excess, 0, "{reproduction:?}");
            assert!(
                reproduction.extracted_tokens <= reproduction.coalition_equity_before,
                "one-sided fee support extracted coalition value"
            );
            if reproduction.queued_mark == reproduction.setup_mark {
                assert_eq!(reproduction.victim_loss, 0);
            }
            assert!(reproduction.fee_lp_loss > 0);
            assert!(reproduction.insurance_gain > 0);
            assert!(reproduction.max_cu < 1_400_000);
        }
    }
}

#[test]
fn v16_program_pr225_mark_movement_fee_is_nonwithdrawable_and_terminally_burned() {
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        let reproduction = reproduce_reclaimable_ewma_fee([0x25; 32], route)
            .unwrap_or_else(|error| panic!("PR 225 {route:?} protection failed: {error}"));
        assert_eq!(reproduction.blocker, KnownBlocker::ReclaimableEwmaFee);
        assert!(reproduction.pending_withdraw_rejected);
        assert!(reproduction.rejected_exact_rollback);
        assert!(reproduction.committed_withdraw_rejected);
        assert!(reproduction.committed_rejected_exact_rollback);
        assert_eq!(reproduction.fee_reclaimed, 0);
        assert_eq!(reproduction.attacker_gain, 0);
        assert!(reproduction.attacker_loss > 0);
        assert!(reproduction.victim_loss <= reproduction.fee_paid);
        assert!(reproduction.effective_mark < 1_000_000);
        assert!(reproduction.terminal_close_landed);
        assert_eq!(reproduction.terminal_fee_burned, reproduction.fee_paid);
        assert!(reproduction.close_cu < 1_400_000);
    }
}

#[test]
fn v16_program_pr280_trade_driven_liquidation_penalty_is_not_reclaimable() {
    for mode in [
        TradeDrivenLiquidationMode::Ewma,
        TradeDrivenLiquidationMode::HybridAfterHours,
    ] {
        for route in [TradeRoute::NoCpi, TradeRoute::BatchNoCpi] {
            let reproduction = reproduce_trade_driven_liquidation_reward([0x80; 32], mode, route)
                .unwrap_or_else(|error| {
                    panic!("PR 280 {mode:?} {route:?} protection failed: {error}")
                });
            assert_eq!(
                reproduction.blocker,
                KnownBlocker::TradeDrivenLiquidationReward
            );
            assert_eq!(reproduction.cranker_reward, 0);
            assert!(reproduction.retained_penalty > 0);
            assert_eq!(reproduction.budgeted_penalty, 0);
            assert!(reproduction.victim_penalty > 0);
            assert!(reproduction.victim_capital_loss > 0);
            assert_eq!(reproduction.attacker_gain, 0);
            assert!(reproduction.attacker_loss > 0);
            assert!(reproduction.liquidation_landed);
            assert!(reproduction.max_crank_cu < 1_400_000);
        }
    }
}

#[test]
fn v16_program_fresh_hybrid_oracle_liquidation_reward_remains_enabled() {
    use crate::support::v16_svm::{MarketConfig, V16Svm};
    use percolator::POS_SCALE;

    const MARK: u64 = 1_000_000;
    const FRESH_MARK: u64 = 999_900;

    fn asset_q(env: &V16Svm, actor: usize) -> u128 {
        env.primary_portfolio(actor)
            .legs
            .iter()
            .filter_map(|leg| leg.try_to_runtime().ok())
            .find(|leg| leg.active && leg.asset_index == 0)
            .map(|leg| leg.basis_pos_q.unsigned_abs())
            .unwrap_or(0)
    }

    let mut env = V16Svm::new(
        [0x92; 32],
        MarketConfig {
            initial_price: MARK,
            h_max: 6_480_000,
            min_nonzero_mm_req: 599,
            min_nonzero_im_req: 600,
            maintenance_margin_bps: 500,
            initial_margin_bps: 500,
            liquidation_fee_bps: 5,
            liquidation_fee_cap: percolator::MAX_PROTOCOL_FEE_ABS,
            min_liquidation_abs: 500,
            max_price_move_bps_per_slot: 24,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 1_000,
            min_funding_lifetime_slots: 10_000_000,
            actor_deposits: [50_000, 2_000_000, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    env.update_liquidation_fee_policy(10_000).unwrap();
    env.set_clock(1, 100);
    let feed = [0xefu8; 32];
    let pyth = env.set_pyth_price(&feed, MARK as i64, -6, 0, 100);
    env.configure_hybrid_oracle(0, 1, 100, 0, [feed, [0; 32], [0; 32]], &[pyth], 10, 0)
        .unwrap();
    env.trade_no_cpi(0, 1, 0, POS_SCALE as i128, MARK, 0)
        .unwrap();

    let supply_before = env.token_supply_observed();
    let mint_supply_before = env.mint_supply();
    let cranker_before = env.primary_portfolio(2).capital.get();
    env.set_clock(2, 101);
    env.update_pyth_price(pyth, &feed, FRESH_MARK as i64, -6, 0, 101);
    let observation = vec![percolator_prog::ix::CrankObservationHint {
        asset_index: 0,
        oracle_accounts: 1,
    }];
    let mut max_cu = 0;
    for attempt in 0..4 {
        let result = env
            .crank_with_reward(
                2,
                0,
                2,
                if attempt == 0 {
                    observation.clone()
                } else {
                    Vec::new()
                },
                if attempt == 0 {
                    std::slice::from_ref(&pyth)
                } else {
                    &[]
                },
            )
            .unwrap_or_else(|error| panic!("fresh Hybrid crank {attempt}: {error}"));
        max_cu = max_cu.max(result.compute_units);
        if asset_q(&env, 0) < POS_SCALE {
            break;
        }
    }

    let (wrapper, group) = env.primary_market_state();
    let profile = env.primary_profile(0);
    let reward = env.primary_portfolio(2).capital.get() - cranker_before;
    assert!(asset_q(&env, 0) < POS_SCALE, "fresh mark must liquidate");
    assert!(
        reward > 0,
        "authenticated Hybrid liquidation keeps its reward"
    );
    assert_eq!(wrapper.last_good_oracle_slot, 2);
    assert_eq!(profile.last_good_oracle_slot, 2);
    assert_eq!(
        profile.effective_price_provenance,
        percolator_prog::constants::EFFECTIVE_PRICE_PROVENANCE_AUTHENTICATED
    );
    assert_eq!(group.assets[0].raw_oracle_target_price, FRESH_MARK);
    assert_eq!(group.assets[0].effective_price, FRESH_MARK);
    assert_eq!(env.token_supply_observed(), supply_before);
    assert_eq!(env.mint_supply(), mint_supply_before);
    assert!(max_cu < crate::support::v16_svm::TX_CU_LIMIT);
    eprintln!("fresh Hybrid liquidation reward={reward}, max_cu={max_cu}");
}

#[test]
fn v16_program_fresh_hybrid_report_does_not_reenable_stale_trade_liquidation_reward() {
    use crate::support::v16_svm::{MarketConfig, V16Svm};
    use percolator::POS_SCALE;

    const MARK: u64 = 1_000_000;
    const VICTIM_DEPOSIT: u128 = 50_000;
    const HONEST_DEPOSIT: u128 = 2_000_000;
    const ATTACK_DEPOSIT: u128 = 1_000;
    const CRANKER_DEPOSIT: u128 = 1;
    const TINY_Q: i128 = (POS_SCALE / 10_000) as i128;

    fn asset_q(env: &V16Svm, actor: usize) -> u128 {
        env.primary_portfolio(actor)
            .legs
            .iter()
            .filter_map(|leg| leg.try_to_runtime().ok())
            .find(|leg| leg.active && leg.asset_index == 0)
            .map(|leg| leg.basis_pos_q.unsigned_abs())
            .unwrap_or(0)
    }

    fn effective_asset_q(env: &V16Svm, actor: usize) -> u128 {
        let (_, group) = env.primary_market_state();
        let Some(leg) = env
            .primary_portfolio(actor)
            .legs
            .iter()
            .filter_map(|leg| leg.try_to_runtime().ok())
            .find(|leg| leg.active && leg.asset_index == 0)
        else {
            return 0;
        };
        let asset = &group.assets[0];
        let (current_a, current_epoch, effective_oi, pending_count, mode) = match leg.side {
            percolator::SideV16::Long => (
                asset.a_long,
                asset.epoch_long,
                asset.oi_eff_long_q,
                asset.pending_obligation_count_long,
                asset.mode_long,
            ),
            percolator::SideV16::Short => (
                asset.a_short,
                asset.epoch_short,
                asset.oi_eff_short_q,
                asset.pending_obligation_count_short,
                asset.mode_short,
            ),
        };
        if (mode == percolator::SideModeV16::ResetPending
            && leg.epoch_snap.checked_add(1) == Some(current_epoch))
            || (effective_oi == 0
                && pending_count == 0
                && mode != percolator::SideModeV16::ResetPending)
        {
            return 0;
        }
        assert_eq!(leg.epoch_snap, current_epoch);
        crate::support::reference_math::mul_div_ceil(
            leg.basis_pos_q.unsigned_abs(),
            current_a,
            leg.a_basis,
        )
        .unwrap()
    }

    let mut env = V16Svm::new(
        [0x91; 32],
        MarketConfig {
            initial_price: MARK,
            h_max: 6_480_000,
            min_nonzero_mm_req: 599,
            min_nonzero_im_req: 600,
            maintenance_margin_bps: 500,
            initial_margin_bps: 500,
            liquidation_fee_bps: 5,
            liquidation_fee_cap: percolator::MAX_PROTOCOL_FEE_ABS,
            min_liquidation_abs: 500,
            max_price_move_bps_per_slot: 24,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 1_000,
            min_funding_lifetime_slots: 10_000_000,
            actor_deposits: [
                VICTIM_DEPOSIT,
                HONEST_DEPOSIT,
                ATTACK_DEPOSIT,
                ATTACK_DEPOSIT,
                CRANKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    env.update_liquidation_fee_policy(10_000).unwrap();
    env.begin_public_trace();
    env.set_clock(1, 100);
    let feed = [0xedu8; 32];
    let pyth = env.set_pyth_price(&feed, MARK as i64, -6, 0, 100);
    env.configure_hybrid_oracle(0, 1, 100, 0, [feed, [0; 32], [0; 32]], &[pyth], 1, 0)
        .unwrap();
    env.trade_no_cpi(0, 1, 0, POS_SCALE as i128, MARK, 0)
        .unwrap();

    let supply_before = env.token_supply_observed();
    let mint_supply_before = env.mint_supply();
    env.set_clock(3, 1_000);
    let insurance_before_move = env.primary_market_state().1.insurance;
    env.trade_no_cpi(2, 3, 0, TINY_Q, 999_850, 0).unwrap();
    let (profile_after_move, group_after_move) = env.primary_market_state();
    let movement_fee = group_after_move.insurance - insurance_before_move;
    assert!(movement_fee > 0);
    assert!(profile_after_move.mark_ewma_e6 < MARK);

    let observation = vec![percolator_prog::ix::CrankObservationHint {
        asset_index: 0,
        oracle_accounts: 1,
    }];
    let mut max_crank_cu = 0;
    let mut stale_crank_cus = Vec::new();
    for _ in 0..4 {
        let (_, group) = env.primary_market_state();
        if group.assets[0].slot_last == 3
            && group.assets[0].effective_price == profile_after_move.mark_ewma_e6
        {
            break;
        }
        let result = env
            .crank_with_oracles(4, 3, observation.clone(), &[pyth])
            .unwrap();
        max_crank_cu = max_crank_cu.max(result.compute_units);
        stale_crank_cus.push(result.compute_units);
    }
    let (_, stale_settled) = env.primary_market_state();
    assert_eq!(stale_settled.assets[0].slot_last, 3);
    assert_eq!(
        stale_settled.assets[0].effective_price,
        profile_after_move.mark_ewma_e6
    );

    env.update_pyth_price(pyth, &feed, MARK as i64, -6, 0, 1_000);
    let victim_capital_before = env.primary_portfolio(0).capital.get();
    let cranker_capital_before = env.primary_portfolio(4).capital.get();
    let insurance_before_liquidation = env.primary_market_state().1.insurance;
    let victim_q_before = asset_q(&env, 0);
    let mut liquidation_landed = false;
    let mut fresh_crank_steps = Vec::new();
    for attempt in 0..6 {
        let oracle_accounts = if attempt == 0 {
            std::slice::from_ref(&pyth)
        } else {
            &[]
        };
        let result = env.crank_with_reward(
            4,
            0,
            3,
            if attempt == 0 {
                observation.clone()
            } else {
                Vec::new()
            },
            oracle_accounts,
        );
        let result = result.unwrap_or_else(|error| panic!("fresh crank {attempt}: {error}"));
        max_crank_cu = max_crank_cu.max(result.compute_units);
        let (profile, group) = env.primary_market_state();
        fresh_crank_steps.push((
            result.compute_units,
            profile.last_good_oracle_slot,
            group.assets[0].effective_price,
            asset_q(&env, 0),
            env.primary_portfolio(4).capital.get() - cranker_capital_before,
        ));
        if asset_q(&env, 0) < victim_q_before {
            liquidation_landed = true;
            break;
        }
    }
    assert!(liquidation_landed);
    let (fresh_profile, after_liquidation) = env.primary_market_state();
    let cranker_reward = env.primary_portfolio(4).capital.get() - cranker_capital_before;
    let retained_penalty = after_liquidation.insurance - insurance_before_liquidation;
    let victim_capital_loss = victim_capital_before - env.primary_portfolio(0).capital.get();
    eprintln!(
        "hybrid provenance probe: movement_fee={movement_fee}, cranker_reward={cranker_reward}, retained_penalty={retained_penalty}, victim_capital_loss={victim_capital_loss}, effective={}, target={}, mark={}, last_good={}, max_cu={max_crank_cu}",
        after_liquidation.assets[0].effective_price,
        after_liquidation.assets[0].raw_oracle_target_price,
        fresh_profile.mark_ewma_e6,
        fresh_profile.last_good_oracle_slot,
    );
    eprintln!(
        "hybrid provenance cranks: stale_settle_cu={stale_crank_cus:?}, fresh_steps=(cu,last_good,effective,victim_q,reward)={fresh_crank_steps:?}"
    );
    assert_eq!(fresh_profile.last_good_oracle_slot, 3);
    assert!(after_liquidation.assets[0].effective_price < MARK);
    assert_eq!(after_liquidation.assets[0].raw_oracle_target_price, MARK);

    env.set_clock(4, 1_001);
    let catchup = env
        .crank_with_oracles(4, 4, observation.clone(), &[pyth])
        .expect("fresh authenticated target catch-up");
    max_crank_cu = max_crank_cu.max(catchup.compute_units);
    let caught_up_profile = env.primary_profile(0);
    let (_, caught_up_group) = env.primary_market_state();
    assert_eq!(caught_up_group.assets[0].effective_price, MARK);
    assert_eq!(caught_up_group.assets[0].raw_oracle_target_price, MARK);
    assert_eq!(
        caught_up_profile.effective_price_provenance,
        percolator_prog::constants::EFFECTIVE_PRICE_PROVENANCE_AUTHENTICATED,
        "rewards become eligible again once effective price reaches the authenticated target"
    );

    for actor in [0usize, 1, 2, 3] {
        for attempt in 0..8 {
            if asset_q(&env, actor) == 0 {
                break;
            }
            let effective_q = effective_asset_q(&env, actor);
            let result = if effective_q == 0 {
                env.crank(actor, 4, Vec::new())
            } else {
                env.rebalance_reduce(actor, 0, effective_q)
            };
            let result = result.unwrap_or_else(|error| {
                panic!("terminal reduction actor {actor} attempt {attempt}: {error}")
            });
            max_crank_cu = max_crank_cu.max(result.compute_units);
        }
        assert_eq!(asset_q(&env, actor), 0, "actor {actor} retained a leg");
    }

    for actor in 0..5 {
        match env.crank_with_oracles(actor, 4, observation.clone(), &[pyth]) {
            Ok(result) => max_crank_cu = max_crank_cu.max(result.compute_units),
            Err(error)
                if error.contains("Custom(22)") || error.contains("custom program error: 0x16") => {
            }
            Err(error) => panic!("terminal refresh actor {actor}: {error}"),
        }
        let pnl = env.primary_portfolio(actor).pnl.get();
        if pnl > 0 {
            let result = env
                .convert_released_pnl(actor, pnl as u128)
                .unwrap_or_else(|error| panic!("convert actor {actor} pnl {pnl}: {error}"));
            max_crank_cu = max_crank_cu.max(result.compute_units);
        }
        let capital = env.primary_portfolio(actor).capital.get();
        if capital != 0 {
            let result = env.withdraw_primary(actor, capital).unwrap();
            max_crank_cu = max_crank_cu.max(result.compute_units);
        }
    }
    let victim_payout = u128::from(env.token_amount(env.actors[0].destination_token));
    let honest_payout = u128::from(env.token_amount(env.actors[1].destination_token));
    let attacker_withdrawn = [2usize, 3, 4]
        .into_iter()
        .map(|actor| u128::from(env.token_amount(env.actors[actor].destination_token)))
        .sum::<u128>();
    let attacker_deposited = ATTACK_DEPOSIT * 2 + CRANKER_DEPOSIT;
    let attacker_gain = attacker_withdrawn.saturating_sub(attacker_deposited);
    let attacker_loss = attacker_deposited.saturating_sub(attacker_withdrawn);
    let victim_loss = VICTIM_DEPOSIT.saturating_sub(victim_payout);
    let trace = env.finish_public_trace();
    trace.validate_public_execution().unwrap();
    assert_eq!(trace.out_of_band_economic_mutations, 0);
    let public_trace_cu = trace
        .steps
        .iter()
        .map(|step| {
            (
                step.instruction_data.first().copied().unwrap_or(u8::MAX),
                step.compute_units,
            )
        })
        .collect::<Vec<_>>();
    eprintln!(
        "hybrid terminal accounting: attacker deposited={attacker_deposited}, withdrawn={attacker_withdrawn}, gain={attacker_gain}, loss={attacker_loss}; victim deposited={VICTIM_DEPOSIT}, withdrawn={victim_payout}, loss={victim_loss}; honest deposited={HONEST_DEPOSIT}, withdrawn={honest_payout}; movement_fee={movement_fee}, cranker_reward={cranker_reward}, observed_supply={}/{supply_before}, mint_supply={}/{mint_supply_before}, public_steps={}, max_cu={max_crank_cu}",
        env.token_supply_observed(),
        env.mint_supply(),
        trace.steps.len(),
    );
    eprintln!("hybrid public trace (instruction tag, CU): {public_trace_cu:?}");
    assert_eq!(env.token_supply_observed(), supply_before);
    assert_eq!(env.mint_supply(), mint_supply_before);
    assert!(max_crank_cu < crate::support::v16_svm::TX_CU_LIMIT);
    assert!(victim_loss > 0);
    assert_eq!(
        cranker_reward, 0,
        "a fresh feed must not make the inherited trade-driven liquidation penalty reclaimable"
    );
    assert_eq!(attacker_gain, 0);
    assert!(attacker_loss > 0);
    assert!(retained_penalty > 0);
}
