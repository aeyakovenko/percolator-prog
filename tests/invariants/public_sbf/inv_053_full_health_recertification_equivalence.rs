//! INV-053 - Full-health recertification equivalence.
//!
//! Normative obligation: Fast or incremental certification is never more favorable than full recomputation.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr220_pr366_omitted_rescue_accrual_liquidates_healthy_control`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_pr220_pr366_omitted_rescue_accrual_liquidates_healthy_control() {
    let reproduction = reproduce_omitted_rescue_liquidation([0x22; 32])
        .expect("PR 220/366 no longer reproduces; remove their quarantines and promote the seed");

    assert_eq!(
        reproduction.blocker,
        KnownBlocker::OmittedRescueAccrualLiquidation
    );
    assert!(
        reproduction.omitted_position_after_q < reproduction.omitted_position_before_q,
        "omitted world did not liquidate the victim"
    );
    assert!(reproduction.omitted_insurance_delta > 0);
    assert_eq!(reproduction.omitted_position_before_q, 50_000_000);
    assert_eq!(reproduction.omitted_position_after_q, 47_995_187);
    assert_eq!(reproduction.omitted_insurance_delta, 1_001);
    assert_eq!(
        reproduction.complete_position_after_q,
        reproduction.omitted_position_before_q
    );
    assert_eq!(reproduction.complete_liquidation_deficit, 0);
    assert_eq!(reproduction.complete_insurance_delta, 0);
}
