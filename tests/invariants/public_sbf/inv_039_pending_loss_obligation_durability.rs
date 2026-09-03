//! INV-039 - Pending-loss obligation durability.
//!
//! Normative obligation: Pending accrual and loss obligations cannot be erased by route choice or lifecycle changes.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr380_trade_order_preserves_elapsed_funding`, `v16_program_pr255_stale_resolve_requires_public_catchup`, `v16_program_pr254_shutdown_preserves_committed_funding`, `v16_program_pr271_cpi_close_preserves_elapsed_funding`, `v16_program_pr272_unilateral_reduce_preserves_elapsed_funding`, `v16_program_pr273_recovery_forfeit_preserves_elapsed_funding`, `v16_program_multi_segment_funding_requires_catchup_before_reduction`, `v16_program_pending_zero_move_mark_requires_terminal_funding_catchup`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: these are fixed-pin whole-route certifications. Position-changing routes
//! settle deterministic zero-move funding before changing OI. Terminal resolve rejects exactly,
//! retains a permissionless stored-state catch-up, and then preserves the same terminal payout.

use super::*;

#[test]
fn v16_program_pr380_trade_order_preserves_elapsed_funding() {
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        let reproduction = reproduce_prospective_funding_rewrite([0x80; 32], route)
            .unwrap_or_else(|error| panic!("PR 380 {route:?} fixed route failed: {error}"));
        assert_eq!(
            reproduction.blocker,
            KnownBlocker::ProspectiveFundingRewrite
        );
        assert_eq!(reproduction.route, route);
        assert!(reproduction.control_f_short_num > 0);
        assert_eq!(
            reproduction.attack_f_short_num,
            reproduction.control_f_short_num
        );
        assert!(reproduction.stamp_fee > 0);
        assert_eq!(reproduction.victim_payout_loss, 0);
        if matches!(route, TradeRoute::NoCpi | TradeRoute::BatchNoCpi) {
            assert_eq!(reproduction.attacker_coalition_gain, 0);
            assert_eq!(
                reproduction.control_total_payout,
                reproduction.attack_total_payout
            );
        }
    }
}

#[test]
fn v16_program_pr255_stale_resolve_requires_public_catchup() {
    let reproduction = reproduce_resolve_before_committed_accrual([0x55; 32])
        .unwrap_or_else(|error| panic!("PR 255 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::ResolveBeforeCommittedAccrual
    );
    assert_eq!(reproduction.control_mark, reproduction.attack_mark);
    assert!(reproduction.unsafe_resolve_rejected);
    assert!(reproduction.rejected_exact_rollback);
    assert_eq!(reproduction.victim_payout_loss, 0);
    assert_eq!(reproduction.attacker_payout_gain, 0);
    assert_eq!(
        reproduction.control_total_payout,
        reproduction.attack_total_payout
    );
    assert_eq!(reproduction.attack_total_payout, 4_000_000_000);
    assert!(reproduction.catchup_steps > 0);
    assert!(reproduction.catchup_steps <= 16);
    assert!(reproduction.catchup_cu < 1_400_000);
    assert!(reproduction.attack_resolve_cu < 1_400_000);
}

#[test]
fn v16_program_pr254_shutdown_preserves_committed_funding() {
    let discovery = discover_shutdown_commit_ordering([0x54; 32])
        .unwrap_or_else(|error| panic!("PR 254 fixed route failed: {error}"));
    assert_ne!(discovery.control_f_long_num, 0);
    assert_ne!(discovery.control_f_short_num, 0);
    assert_eq!(discovery.shutdown_f_long_num, discovery.control_f_long_num);
    assert_eq!(
        discovery.shutdown_f_short_num,
        discovery.control_f_short_num
    );
    assert_eq!(discovery.victim_payout_loss, 0);
    assert_eq!(discovery.counterparty_payout_gain, 0);
    assert!(!discovery.is_violation(), "{discovery:?}");
    assert!(discovery.certifies_terminal_ordering(), "{discovery:?}");
}

#[test]
fn v16_program_shutdown_stale_rejection_has_bounded_public_catchup() {
    let discovery = discover_shutdown_catchup_liveness([0x5c; 32])
        .unwrap_or_else(|error| panic!("shutdown catch-up liveness failed: {error}"));
    assert!(discovery.initial_shutdown_rejected, "{discovery:?}");
    assert!(discovery.rejected_exact_rollback, "{discovery:?}");
    assert!(discovery.catchup_steps > 0, "{discovery:?}");
    assert!(discovery.catchup_steps <= 16, "{discovery:?}");
    assert!(discovery.max_catchup_cu < 1_400_000, "{discovery:?}");
    assert!(discovery.retry_landed, "{discovery:?}");
    assert_ne!(discovery.f_long_num, 0, "{discovery:?}");
    assert_ne!(discovery.f_short_num, 0, "{discovery:?}");
    assert!(discovery.users_terminal, "{discovery:?}");
    assert_eq!(discovery.total_payout, 2_000_000);
    assert!(discovery.token_supply_conserved, "{discovery:?}");
}

#[test]
fn v16_program_pr271_cpi_close_preserves_elapsed_funding() {
    for route in [TradeRoute::Cpi, TradeRoute::BatchCpi] {
        let reproduction = reproduce_trade_funding_erasure([0x71; 32], route)
            .unwrap_or_else(|error| panic!("PR 271 {route:?} no longer reproduces: {error}"));
        assert_eq!(reproduction.blocker, KnownBlocker::TradeFundingErasure);
        assert!(reproduction.control_f_long_num > 0);
        assert!(reproduction.control_f_short_num < 0);
        assert_eq!(
            reproduction.attack_f_long_num,
            reproduction.control_f_long_num
        );
        assert_eq!(
            reproduction.attack_f_short_num,
            reproduction.control_f_short_num
        );
        assert_eq!(reproduction.victim_payout_loss, 0);
        assert_eq!(reproduction.attacker_payout_gain, 0);
    }
}

#[test]
fn v16_program_pr272_unilateral_reduce_preserves_elapsed_funding() {
    let reproduction = reproduce_rebalance_funding_erasure([0x72; 32])
        .unwrap_or_else(|error| panic!("PR 272 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::RebalanceFundingErasure);
    assert_eq!(
        reproduction.control_attacker_paid,
        reproduction.control_victim_received
    );
    assert!(reproduction.control_attacker_paid > 0);
    assert_eq!(
        reproduction.attack_attacker_paid,
        reproduction.control_attacker_paid
    );
    assert_eq!(
        reproduction.attack_victim_received,
        reproduction.control_victim_received
    );
    assert_eq!(reproduction.victim_claim_loss, 0);
    assert_eq!(reproduction.attacker_payout_gain, 0);
}

#[test]
fn v16_program_pr273_recovery_forfeit_preserves_elapsed_funding() {
    let reproduction = reproduce_forfeit_funding_erasure([0x73; 32])
        .unwrap_or_else(|error| panic!("PR 273 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::ForfeitFundingErasure);
    assert_eq!(
        reproduction.control_attacker_paid,
        reproduction.control_victim_received
    );
    assert!(reproduction.control_attacker_paid > 0);
    assert_eq!(
        reproduction.attack_attacker_paid,
        reproduction.control_attacker_paid
    );
    assert_eq!(
        reproduction.attack_victim_received,
        reproduction.control_victim_received
    );
    assert_eq!(reproduction.victim_claim_loss, 0);
    assert_eq!(reproduction.attacker_payout_gain, 0);
}

#[test]
fn v16_program_multi_segment_funding_requires_catchup_before_reduction() {
    let discoveries = discover_multi_segment_accrual_ordering_violations([0x9d; 32])
        .unwrap_or_else(|error| panic!("multi-segment fixed route failed: {error}"));
    assert_eq!(discoveries.len(), AccrualOrderingKind::ALL.len());
    for (expected, discovery) in AccrualOrderingKind::ALL.into_iter().zip(discoveries) {
        assert_eq!(discovery.kind, expected);
        assert!(discovery.unsafe_action_rejected, "{discovery:?}");
        assert!(discovery.rejected_exact_rollback, "{discovery:?}");
        assert!(discovery.retry_landed, "{discovery:?}");
        assert!(!discovery.is_violation(), "{discovery:?}");
        assert!(discovery.certifies_terminal_value(), "{discovery:?}");
        assert_eq!(discovery.reordered_paid, discovery.control_paid);
        assert_eq!(discovery.reordered_received, discovery.control_received);
        assert_eq!(discovery.victim_payout_loss, 0);
        assert_eq!(discovery.coalition_payout_gain, 0);
    }
}

#[test]
fn v16_program_pending_zero_move_mark_requires_terminal_funding_catchup() {
    let discovery = discover_pending_zero_move_terminal_ordering([0x3f; 32])
        .unwrap_or_else(|error| panic!("zero-move terminal ordering failed: {error}"));
    assert!(discovery.unsafe_resolve_rejected, "{discovery:?}");
    assert!(discovery.rejected_exact_rollback, "{discovery:?}");
    assert!(discovery.catchup_steps >= 2, "{discovery:?}");
    assert!(discovery.catchup_steps <= 16, "{discovery:?}");
    assert!(discovery.max_catchup_cu < 1_400_000, "{discovery:?}");
    assert_ne!(discovery.control_f_long_num, 0, "{discovery:?}");
    assert_ne!(discovery.control_f_short_num, 0, "{discovery:?}");
    assert_eq!(discovery.reordered_f_long_num, discovery.control_f_long_num);
    assert_eq!(
        discovery.reordered_f_short_num,
        discovery.control_f_short_num
    );
    assert!(!discovery.has_funding_divergence(), "{discovery:?}");
    assert!(!discovery.is_violation(), "{discovery:?}");
    assert!(discovery.certifies_terminal_ordering(), "{discovery:?}");
}
