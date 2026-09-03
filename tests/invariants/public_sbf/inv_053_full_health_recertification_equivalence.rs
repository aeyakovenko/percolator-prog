//! INV-053 - Full-health recertification equivalence.
//!
//! Normative obligation: Fast or incremental certification is never more favorable than full recomputation.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr220_pr366_omitted_rescue_accrual_rejects_before_liquidation`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: this fixed-pin regression covers the minimized PR220/PR366 public trace.
//! The generated route/order matrix in the stateful module supplies the broader bounded evidence.

use super::*;

#[test]
fn v16_program_pr220_pr366_omitted_rescue_accrual_rejects_before_liquidation() {
    let reproduction = reproduce_omitted_rescue_liquidation([0x22; 32])
        .expect("verify the fixed PR 220/366 public trace");

    assert_eq!(
        reproduction.blocker,
        KnownBlocker::OmittedRescueAccrualLiquidation
    );
    assert!(
        reproduction.omitted_rejected_nonprogress,
        "omitted later-leg funding did not reject the unsafe crank"
    );
    assert!(
        reproduction.omitted_exact_rollback,
        "rejected unsafe crank did not roll back every tracked economic account"
    );
    assert_eq!(reproduction.omitted_position_before_q, 50_000_000);
    assert_eq!(
        reproduction.omitted_position_after_q,
        reproduction.omitted_position_before_q
    );
    assert_eq!(reproduction.omitted_insurance_delta, 0);
    assert_eq!(
        reproduction.complete_position_after_q,
        reproduction.omitted_position_before_q
    );
    assert_eq!(reproduction.complete_liquidation_deficit, 0);
    assert_eq!(reproduction.complete_insurance_delta, 0);
}
