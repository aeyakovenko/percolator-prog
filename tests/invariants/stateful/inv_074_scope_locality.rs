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
//! Guarantee boundary: these are the same-asset risk-reduction and two-asset/two-account close
//! cells. Risk increase while a domain loss barrier is active is intentionally outside the
//! guarantee; same-domain competing closes and broader lifecycle combinations remain open.

use crate::support::fuzz_model::{
    run_concurrent_close_locality_probe, run_same_asset_close_locality_probe,
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
