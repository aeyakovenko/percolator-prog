//! INV-031 - No double use of claim, backing, or insurance atoms.
//!
//! Normative obligation: A backing or claim atom cannot support two economic obligations.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr267_cross_domain_backing_is_spent_twice`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_pr267_cross_domain_backing_is_spent_twice() {
    let reproduction = reproduce_cross_domain_backing_double_spend([0x67; 32])
        .unwrap_or_else(|error| panic!("PR 267 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::CrossDomainBackingDoubleSpend
    );
    assert_eq!(
        reproduction.unfunded_claim_before_num,
        100 * percolator::BOUND_SCALE
    );
    assert_eq!(
        reproduction.funded_claim_before_num,
        100 * percolator::BOUND_SCALE
    );
    assert_eq!(
        reproduction.funded_backing_consumed_num,
        200 * percolator::BOUND_SCALE
    );
    assert_eq!(reproduction.winner_capital_gain, 200);
    assert_eq!(reproduction.extracted_tokens, 1_200);
}
