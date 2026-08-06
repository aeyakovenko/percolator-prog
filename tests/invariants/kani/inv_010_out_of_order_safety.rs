//! INV-010 - Out-of-order safety.
//!
//! Normative obligation: a retained matcher mutation executes only against the exact
//! portfolio-scoped sequence it observed, and every successful mutation advances that sequence.
//!
//! Evidence in this file (P): Kani checks the pure sequence transition used by the deployed
//! `SetMatcherConfig` handler over every pair of `u64` values. Mismatched and exhausted inputs
//! reject; the sole successful case advances exactly once.
//!
//! Guarantee boundary: this is the wrapper's local sequence contract. INV-010 stateful LiteSVM
//! coverage proves composition with portfolio identity, owner authorization, exact rollback, and
//! a non-vacuous fresh matcher enable/fill route.

use percolator_prog::state;

#[kani::proof]
fn kani_v16_matcher_sequence_accepts_only_current_expected_value() {
    let current: u64 = kani::any();
    let expected: u64 = kani::any();
    let result = state::next_portfolio_matcher_sequence(current, expected);

    if current != expected || current == u64::MAX {
        assert!(result.is_err());
    } else {
        assert_eq!(result.unwrap(), current + 1);
    }
}
