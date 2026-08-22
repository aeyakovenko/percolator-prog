//! INV-074 - Scope locality.
//!
//! Normative obligation: account-local close state and side/domain barriers may constrain only
//! the economic scope they protect. They cannot prevent an unrelated owner from reducing an
//! existing position on the same asset.
//!
//! Evidence (F over public I routes): the probe opens a small healthy pair, then independently
//! creates a real active bankruptcy close for another pair on the same asset. The healthy pair's
//! full bilateral reduction must land through all four deployed trade routes in both long/short
//! orientations, remove exactly its own effective OI, preserve both close participants and the
//! complete close ledger byte-for-byte, and move no internal or SPL custody. The eight fresh
//! worlds must also converge to identical normalized economics.
//!
//! A second probe creates active closes on two different assets and advances each independently;
//! each crank must strictly reduce only its selected ledger while framing the other close and all
//! custody.
//!
//! A third paired probe lands an authenticated asset shutdown while a close is active, either
//! before or after one close continuation. Both schedules must preserve a bounded public exit for
//! every funded portfolio and converge to identical owner payouts and terminal accounting.
//!
//! Guarantee boundary: these are the same-asset risk-reduction and two-asset/two-account close
//! cells. Risk increase while a domain loss barrier is active is intentionally outside the
//! guarantee; same-domain competing closes and broader lifecycle combinations remain open.

use crate::support::fuzz_model::{
    run_active_close_shutdown_liveness_probe, run_concurrent_close_locality_probe,
    run_same_asset_close_locality_probe,
};

#[test]
fn v16_program_active_close_preserves_unrelated_same_asset_reduction() {
    let evidence = run_same_asset_close_locality_probe()
        .expect("INV-074 public same-asset close locality probe");

    assert_eq!(evidence.world_count, 8, "{evidence:?}");
    assert_eq!(evidence.route_worlds, [2; 4], "{evidence:?}");
    assert_eq!(evidence.orientation_worlds, [4; 2], "{evidence:?}");
    assert_ne!(evidence.close_residual_before, 0, "{evidence:?}");
    assert_eq!(
        evidence.close_residual_after, evidence.close_residual_before,
        "{evidence:?}"
    );
    assert_ne!(evidence.unrelated_position_q_before, 0, "{evidence:?}");
    assert_eq!(evidence.unrelated_position_q_after, 0, "{evidence:?}");
    assert_eq!(
        evidence.oi_long_before - evidence.oi_long_after,
        evidence.unrelated_position_q_before,
        "{evidence:?}"
    );
    assert_eq!(
        evidence.oi_short_before - evidence.oi_short_after,
        evidence.unrelated_position_q_before,
        "{evidence:?}"
    );
    assert!(
        evidence
            .coverage
            .route_success
            .iter()
            .all(|count| *count > 0),
        "{evidence:?}"
    );
    assert_ne!(evidence.coverage.token_frame_checks, 0, "{evidence:?}");
}

#[test]
fn v16_program_two_asset_closes_advance_without_crossing_scope() {
    let evidence = run_concurrent_close_locality_probe()
        .expect("INV-074 public concurrent-close locality probe");

    assert!(
        evidence.first_residual_after < evidence.first_residual_before,
        "{evidence:?}"
    );
    assert!(
        evidence.second_residual_after < evidence.second_residual_before,
        "{evidence:?}"
    );
    assert!(evidence.coverage.crank_progress >= 2, "{evidence:?}");
    assert_ne!(evidence.coverage.token_frame_checks, 0, "{evidence:?}");
}

#[test]
fn v16_program_active_close_shutdown_order_preserves_all_funded_exits() {
    let evidence = run_active_close_shutdown_liveness_probe()
        .expect("INV-074 active-close shutdown liveness probe");

    assert_eq!(evidence.world_count, 2, "{evidence:?}");
    assert_eq!(evidence.pre_shutdown_progress_worlds, 1, "{evidence:?}");
    assert_ne!(evidence.live_position_abs_q, 0, "{evidence:?}");
    assert_eq!(evidence.final_capital_total, 0, "{evidence:?}");
    assert_ne!(
        evidence.destination_payouts.iter().sum::<u128>(),
        0,
        "{evidence:?}"
    );
    assert_ne!(evidence.coverage.lifecycle_updates, 0, "{evidence:?}");
    assert_eq!(
        evidence.coverage.recovery_forfeit_successes, 0,
        "healthy Recovery exits must not require destructive forfeiture: {evidence:?}"
    );
    assert_ne!(
        evidence.coverage.crank_rank_component_reduced[4], 0,
        "the public crank must settle derived B work: {evidence:?}"
    );
    assert_ne!(evidence.coverage.user_positions_closed, 0, "{evidence:?}");
    assert_ne!(evidence.coverage.withdrawals, 0, "{evidence:?}");
}
