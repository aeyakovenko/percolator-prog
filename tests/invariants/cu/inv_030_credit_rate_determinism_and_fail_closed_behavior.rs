//! INV-030 - Credit-rate determinism and fail-closed behavior.
//!
//! Normative obligation: source-credit rates are a deterministic function of
//! current claim bounds and independently available backing. Expiry, omission,
//! or impairment cannot make the persisted rate more favorable, and must not
//! delete the underlying claim or move custody.
//!
//! Evidence in this file (I/F): this deterministic LiteSVM wrapper test runs
//! the shared public-route lifecycle oracle with fixed parameters. It creates a
//! discounted positive claim, adds fresh backing, crosses the exact expiry slot,
//! checks a public owner risk-reduction path with zero credit, and refills the
//! bucket. After each route, an independent u128 oracle recomputes the rate.
//! The generated stateful runner also applies a transition-cause oracle to every public action and
//! successful crank: formula-input changes advance the source epoch, unchanged inputs preserve the
//! rate, and a live claim's rate cannot rise without more independently available backing or a
//! smaller claim bound.
//!
//! Guarantee boundary: this is one non-random whole-route witness for the same
//! invariant enforced by the stateful generator. Full-width arithmetic remains
//! covered by engine/Kani and INV-085 arithmetic-differential tests.

#[test]
fn v16_program_source_credit_rate_lifecycle_matches_independent_oracle_fixed_case() {
    crate::support::fuzz_model::verify_source_credit_rate_lifecycle([0x30; 32], 17, 29, 11)
        .expect("source-credit rate public lifecycle oracle");
}
