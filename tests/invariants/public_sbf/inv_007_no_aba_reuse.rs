//! INV-007 - No ABA reuse.
//!
//! The matrix in this file runs public close/recreate sequences for the whole
//! market and then replays retained requests from the prior incarnation. It is
//! intentionally a discovery owner, not a green certification: fixed routes must
//! reject with exact rollback, while any still-accepted stale route is recorded as
//! a bounded public ABA counterexample. This prevents market/asset/portfolio
//! generation regressions from being hidden in leaf tests.

use crate::support::invariant_discovery::{discover_market_incarnation_replays, MarketIntentKind};

#[test]
fn v16_program_whole_market_recreate_aba_matrix_is_public_and_nonvacuous() {
    let discoveries = discover_market_incarnation_replays([0x07; 32])
        .unwrap_or_else(|error| panic!("INV-007 whole-market ABA matrix failed: {error}"));
    assert_eq!(discoveries.len(), MarketIntentKind::ALL.len());

    let mut accepted = Vec::new();
    let mut rejected = Vec::new();
    for discovery in &discoveries {
        assert!(
            discovery.new_market_id >= discovery.old_market_id,
            "{:?}: replacement market id regressed: {} -> {}",
            discovery.kind,
            discovery.old_market_id,
            discovery.new_market_id,
        );
        if discovery.accepted_stale_intent {
            assert!(
                discovery.mutated_economic_state,
                "{:?}: stale retained market request landed without an observable delta",
                discovery.kind,
            );
            assert!(
                discovery.compute_units.is_some_and(|cu| cu < 1_400_000),
                "{:?}: accepted stale retained request must have bounded CU evidence",
                discovery.kind,
            );
            accepted.push(discovery.kind);
        } else {
            assert!(
                !discovery.mutated_economic_state,
                "{:?}: rejected stale retained request failed exact rollback",
                discovery.kind,
            );
            assert_eq!(
                discovery.compute_units, None,
                "{:?}: rejected stale retained request should not report success CU",
                discovery.kind,
            );
            rejected.push(discovery.kind);
        }
    }

    assert!(
        !accepted.is_empty(),
        "matrix should remain non-vacuous until whole-market incarnation binding is fixed",
    );
    eprintln!(
        "INV-007 whole-market ABA accepted stale routes: {accepted:?}; rejected stale routes: {rejected:?}",
    );
}
