//! INV-045 - No free mark movement.
//!
//! Normative obligation: Every mark movement remains elapsed-time bounded and economically paid across every trade route.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr260_pending_ewma_inheritance_rejects_then_trades_on_every_route`, `v16_program_pr282_pending_ewma_target_override_rejects_without_value_drift`, `v16_program_pr264_pr265_pr332_pr333_targets_stage_before_stale_cpi`, `v16_program_pr356_pending_mark_fee_sync_rejects_then_preserves_terminal_value`, `v16_program_pr369_one_sided_cpi_fee_cannot_subsidize_mark_gain`, `v16_program_pr225_mark_movement_fee_is_nonwithdrawable_and_terminally_burned`, `v16_program_pr280_trade_driven_liquidation_penalty_is_not_reclaimable`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: PR260, PR264/265/332/333, PR282, PR356, and PR369 are fixed-pin regressions
//! covering target staging, stale-admission rollback, post-catch-up liveness, target-aware fees,
//! authenticated mark/fee ordering, bilateral CPI fee support, nonwithdrawable movement reserves,
//! and nonreclaimable trade-driven liquidation penalties.

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
