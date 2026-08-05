//! INV-039 - Pending-loss obligation durability.
//!
//! Normative obligation: Pending accrual and loss obligations cannot be erased by route choice or lifecycle changes.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr380_trade_order_preserves_elapsed_funding`, `v16_program_pr255_stale_resolve_discards_pending_authenticated_mark`, `v16_program_pr271_cpi_close_erases_elapsed_funding`, `v16_program_pr272_unilateral_reduce_erases_elapsed_funding`, `v16_program_pr273_recovery_forfeit_erases_elapsed_funding`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: PR380 is fixed-pin certification across all four trade routes. The other
//! named tests remain quarantined counterexamples until their corresponding fixes are integrated.

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
fn v16_program_pr255_stale_resolve_discards_pending_authenticated_mark() {
    let reproduction = reproduce_resolve_before_committed_accrual([0x55; 32])
        .unwrap_or_else(|error| panic!("PR 255 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::ResolveBeforeCommittedAccrual
    );
    assert!(reproduction.control_mark > reproduction.attack_mark);
    assert_eq!(
        reproduction.victim_payout_loss,
        reproduction.attacker_payout_gain
    );
    assert_eq!(reproduction.victim_payout_loss, 10_000_000);
    assert_eq!(
        reproduction.control_total_payout,
        reproduction.attack_total_payout
    );
    assert_eq!(reproduction.attack_total_payout, 4_000_000_000);
    assert!(reproduction.attack_resolve_cu < 1_400_000);
}

#[test]
fn v16_program_pr271_cpi_close_erases_elapsed_funding() {
    for route in [TradeRoute::Cpi, TradeRoute::BatchCpi] {
        let reproduction = reproduce_trade_funding_erasure([0x71; 32], route)
            .unwrap_or_else(|error| panic!("PR 271 {route:?} no longer reproduces: {error}"));
        assert_eq!(reproduction.blocker, KnownBlocker::TradeFundingErasure);
        assert!(reproduction.control_f_long_num > 0);
        assert!(reproduction.control_f_short_num < 0);
        assert_eq!(reproduction.attack_f_long_num, 0);
        assert_eq!(reproduction.attack_f_short_num, 0);
        assert_eq!(
            reproduction.victim_payout_loss,
            reproduction.attacker_payout_gain
        );
    }
}

#[test]
fn v16_program_pr272_unilateral_reduce_erases_elapsed_funding() {
    let reproduction = reproduce_rebalance_funding_erasure([0x72; 32])
        .unwrap_or_else(|error| panic!("PR 272 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::RebalanceFundingErasure);
    assert_eq!(
        reproduction.control_attacker_paid,
        reproduction.control_victim_received
    );
    assert!(reproduction.control_attacker_paid > 0);
    assert_eq!(reproduction.attack_attacker_paid, 0);
    assert_eq!(reproduction.attack_victim_received, 0);
    assert_eq!(
        reproduction.victim_claim_loss,
        u128::from(reproduction.attacker_payout_gain)
    );
}

#[test]
fn v16_program_pr273_recovery_forfeit_erases_elapsed_funding() {
    let reproduction = reproduce_forfeit_funding_erasure([0x73; 32])
        .unwrap_or_else(|error| panic!("PR 273 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::ForfeitFundingErasure);
    assert_eq!(
        reproduction.control_attacker_paid,
        reproduction.control_victim_received
    );
    assert!(reproduction.control_attacker_paid > 0);
    assert_eq!(reproduction.attack_attacker_paid, 0);
    assert_eq!(reproduction.attack_victim_received, 0);
    assert_eq!(
        reproduction.victim_claim_loss,
        i128::from(reproduction.attacker_payout_gain)
    );
}
