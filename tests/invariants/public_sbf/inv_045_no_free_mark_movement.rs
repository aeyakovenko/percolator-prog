//! INV-045 - No free mark movement.
//!
//! Normative obligation: Every mark movement remains elapsed-time bounded and economically paid across every trade route.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr260_pending_ewma_inheritance_extracts_on_every_route`, `v16_program_pr282_pending_ewma_target_override_extracts_on_every_route`, `v16_program_pr264_pr265_pr332_pr333_unstaged_targets_open_stale_cpi_window`, `v16_program_pr356_pending_mark_fee_sync_rejects_then_preserves_terminal_value`, `v16_program_pr369_one_sided_cpi_fee_cannot_subsidize_mark_gain`, `v16_program_pr225_reclaimed_ewma_fee_extracts_on_every_route`, `v16_program_pr280_trade_driven_liquidation_reward_is_extractable`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: PR356 and PR369 are fixed-pin regressions covering authenticated mark/fee
//! ordering and bilateral CPI fee support. The other named exploit adapters remain quarantined
//! counterexamples and do not certify their sub-routes.

use super::*;

#[test]
fn v16_program_pr260_pending_ewma_inheritance_extracts_on_every_route() {
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        let reproduction = reproduce_pending_ewma_inheritance([0x60; 32], route)
            .unwrap_or_else(|error| panic!("PR 260 {route:?} no longer reproduces: {error}"));
        assert_eq!(reproduction.blocker, KnownBlocker::PendingEwmaInheritance);
        assert!(reproduction.pending_mark > 1_000_000);
        assert!(reproduction.applied_mark > 1_000_000);
        assert_eq!(reproduction.attacker_gain, reproduction.victim_loss);
        assert!(reproduction.attacker_gain > reproduction.seed_cost);
        assert_eq!(
            u128::from(reproduction.net_extracted_tokens),
            reproduction.attacker_gain - reproduction.seed_cost
        );
    }
}

#[test]
fn v16_program_pr282_pending_ewma_target_override_extracts_on_every_route() {
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        let reproduction = reproduce_pending_ewma_target_override([0x82; 32], route)
            .unwrap_or_else(|error| panic!("PR 282 {route:?} no longer reproduces: {error}"));
        assert_eq!(
            reproduction.blocker,
            KnownBlocker::PendingEwmaTargetOverride
        );
        assert_eq!(reproduction.route, route);
        assert!(reproduction.attack_target < reproduction.control_target);
        assert!(reproduction.movement_fee < reproduction.displaced_victim_pnl);
        assert!(reproduction.attacker_profit > 0);
        assert!(reproduction.attacker_withdrawn > 24_000_000_000);
        assert!(reproduction.victim_withdrawn < 20_000_000_000);
    }
}

#[test]
fn v16_program_pr264_pr265_pr332_pr333_unstaged_targets_open_stale_cpi_window() {
    for case in [
        TargetStagingCase::AuthMarkPush,
        TargetStagingCase::EwmaMarkPush,
        TargetStagingCase::EwmaSingleTrade,
        TargetStagingCase::EwmaBatchTrade,
    ] {
        let reproduction = reproduce_unstaged_mark_target([0x32; 32], case)
            .unwrap_or_else(|error| panic!("{case:?} no longer reproduces: {error}"));
        assert_eq!(reproduction.blocker, KnownBlocker::UnstagedMarkTarget);
        assert_eq!(reproduction.case, case);
        assert_eq!(reproduction.stale_engine_target, 100);
        assert_eq!(reproduction.moved_engine_mark, reproduction.wrapper_target);
        assert!(reproduction.attacker_profit > 0);
        assert_eq!(
            reproduction.attacker_profit,
            reproduction.victim_capital_loss
        );
        assert!(u128::from(reproduction.attacker_withdrawn) > reproduction.attacker_profit);
        assert!(reproduction.attack_cu < support::v16_svm::TX_CU_LIMIT);
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
fn v16_program_pr225_reclaimed_ewma_fee_extracts_on_every_route() {
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        let reproduction = reproduce_reclaimable_ewma_fee([0x25; 32], route)
            .unwrap_or_else(|error| panic!("PR 225 {route:?} no longer reproduces: {error}"));
        assert_eq!(reproduction.blocker, KnownBlocker::ReclaimableEwmaFee);
        assert_eq!(reproduction.fee_reclaimed, reproduction.fee_paid);
        assert_eq!(reproduction.attacker_gain + 1, reproduction.victim_loss);
        assert!(reproduction.attacker_gain > 0);
        assert!(reproduction.effective_mark < 1_000_000);
    }
}

#[test]
fn v16_program_pr280_trade_driven_liquidation_reward_is_extractable() {
    for mode in [
        TradeDrivenLiquidationMode::Ewma,
        TradeDrivenLiquidationMode::HybridAfterHours,
    ] {
        for route in [TradeRoute::NoCpi, TradeRoute::BatchNoCpi] {
            let reproduction = reproduce_trade_driven_liquidation_reward([0x80; 32], mode, route)
                .unwrap_or_else(|error| {
                    panic!("PR 280 {mode:?} {route:?} no longer reproduces: {error}")
                });
            assert_eq!(
                reproduction.blocker,
                KnownBlocker::TradeDrivenLiquidationReward
            );
            assert!(reproduction.cranker_reward > reproduction.movement_fee);
            assert!(reproduction.victim_penalty > 0);
            assert!(reproduction.victim_capital_loss > 0);
            assert!(reproduction.attacker_profit > 0);
            assert!(reproduction.attacker_extracted > 2_001);
        }
    }
}
