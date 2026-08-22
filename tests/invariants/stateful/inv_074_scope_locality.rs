//! INV-074 - Scope locality.
//!
//! Normative obligation: account-local close state and side/domain barriers may constrain only
//! the economic scope they protect. They cannot prevent an unrelated owner from reducing an
//! existing position on the same asset.
//!
//! Evidence (F over public I routes): the probe opens a small healthy pair, then independently
//! creates a real active bankruptcy close for another pair on the same asset. The healthy pair's
//! full bilateral reduction must land through the deployed no-CPI route, remove exactly its own
//! effective OI, preserve both close participants and the complete close ledger byte-for-byte,
//! and move no internal or SPL custody.
//!
//! Guarantee boundary: this is the same-asset risk-reduction cell. Risk increase while a domain
//! loss barrier is active is intentionally outside the guarantee; other side/domain/lifecycle and
//! concurrent-close combinations remain in the audit matrix.

use crate::support::fuzz_model::run_same_asset_close_locality_probe;

#[test]
fn v16_program_active_close_preserves_unrelated_same_asset_reduction() {
    let evidence = run_same_asset_close_locality_probe()
        .expect("INV-074 public same-asset close locality probe");

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
    assert_ne!(
        evidence.coverage.route_success.iter().sum::<u64>(),
        0,
        "{evidence:?}"
    );
    assert_ne!(evidence.coverage.token_frame_checks, 0, "{evidence:?}");
}
