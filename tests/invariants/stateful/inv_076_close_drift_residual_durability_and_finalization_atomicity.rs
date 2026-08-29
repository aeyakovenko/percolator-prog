//! INV-076 - Close drift, residual durability, and finalization atomicity.
//!
//! Normative obligation: same-asset mark or funding accrual after an immutable bankruptcy-close
//! anchor cannot silently rewrite the close, free exposure, or strand the owner. Once the trade
//! exposure is flat, the public crank must preserve the exact residual partition, strictly advance
//! the close in Live, and retain every owner's normal withdrawal path.
//!
//! Evidence (F over public I routes): four fresh LiteSVM worlds create a real active bankruptcy
//! close through each deployed trade route. A flat keeper then lands two authenticated accruals on
//! the close asset before the close account is touched. The matrix crosses upward/downward price
//! movement and funding enabled/disabled, requires an actual funding-index delta in enabled worlds,
//! exact close-ledger conservation, strict value-neutral Live progress, and complete normal owner
//! withdrawals. Four malformed hint words per world must reject with byte/token/lamport-exact
//! rollback before the canonical retry. Effective OI is checked at close creation, after both
//! accruals, after residual booking, and after final owner exits. No program-owned bytes are
//! synthesized or mutated out of band.
//!
//! Guarantee boundary: this closes the publicly reachable flat-close same-asset price/funding
//! drift cell. Uncovered loss with open risk is prevented from entering this state by the separate
//! INV-061/071 liquidation-to-Recovery regressions. Table-driven failure injection at every
//! internal close phase and a whole-body atomic OI/basis proof remain separate obligations.

use crate::support::fuzz_model::run_same_asset_close_drift_progress_probe;

#[test]
fn v16_program_same_asset_price_and_funding_drift_preserves_close_and_owner_exit() {
    let evidence = run_same_asset_close_drift_progress_probe()
        .expect("INV-076 public same-asset drift/progress matrix");

    assert_eq!(evidence.world_count, 4, "{evidence:?}");
    assert_eq!(evidence.route_worlds, [1; 4], "{evidence:?}");
    assert_eq!(evidence.direction_worlds, [2; 2], "{evidence:?}");
    assert_eq!(evidence.funding_enabled_worlds, 2, "{evidence:?}");
    assert_eq!(evidence.same_asset_slot_advances, 4, "{evidence:?}");
    assert_eq!(evidence.funding_index_move_worlds, 2, "{evidence:?}");
    assert_eq!(evidence.rejected_close_hint_words, 16, "{evidence:?}");
    assert_eq!(evidence.oi_basis_frame_worlds, 4, "{evidence:?}");
    assert_eq!(evidence.exact_partition_pre_worlds, 4, "{evidence:?}");
    assert_eq!(evidence.exact_partition_post_worlds, 4, "{evidence:?}");
    assert_eq!(evidence.live_close_progresses, 4, "{evidence:?}");
    assert_eq!(evidence.owner_exit_worlds, 4, "{evidence:?}");
    assert_ne!(evidence.minimum_initial_residual, 0, "{evidence:?}");
    assert_ne!(evidence.total_owner_payout, 0, "{evidence:?}");
    assert!(
        evidence
            .coverage
            .route_success
            .iter()
            .all(|count| *count >= 2),
        "each route must both open and reduce the public position: {evidence:?}"
    );
    assert!(evidence.coverage.crank_progress >= 8, "{evidence:?}");
    assert_ne!(evidence.coverage.withdrawals, 0, "{evidence:?}");
    assert_ne!(evidence.coverage.token_frame_checks, 0, "{evidence:?}");
}
