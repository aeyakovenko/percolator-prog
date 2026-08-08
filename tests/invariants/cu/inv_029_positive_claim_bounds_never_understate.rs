//! INV-029 - Positive claim bounds never understate.
//!
//! Normative obligation: market-level source-credit claim bounds must equal
//! the complete set of live portfolio-attributed positive claims. A conversion
//! or settlement route may burn a bound only with the matching account-local
//! claim delta.
//!
//! Evidence in this file (I/F): this deterministic LiteSVM wrapper test runs
//! the shared public-route lifecycle oracle with fixed non-boundary parameters:
//! two winners create claims in the same source domain, authenticated marks
//! partially burn those claims, the positions close, backing is added, and both
//! conversion orders are checked against an independent portfolio census after
//! every public transition.
//!
//! Guarantee boundary: this is one non-random whole-route witness for the same
//! invariant enforced by the stateful generator. It does not replace exhaustive
//! production account enumeration.

#[test]
fn v16_program_positive_claim_bounds_match_public_lifecycle_census() {
    crate::support::fuzz_model::verify_positive_claim_bound_attribution_lifecycle(
        [0x29; 32], 3, 13, true,
    )
    .expect("positive-claim bound public lifecycle census");
}
