//! INV-003 - Portfolio incarnation binding.
//!
//! Normative obligation: every retained portfolio-scoped request binds the current
//! program-assigned `portfolio_id`, not only the portfolio pubkey. Closing and
//! recreating the same pubkey must make old consent unusable before any economic
//! or lamport mutation.
//!
//! Evidence in this file (I): one public SBF/LiteSVM matrix builds retained
//! requests for every portfolio-scoped public route, cycles the same portfolio
//! pubkey through owners A -> B -> A, then replays A's original transaction. The assertion is
//! route-local: the stale request must reject, exact tracked state must roll
//! back, SPL supply must stay fixed, and the replacement account's new
//! `portfolio_id` must be larger than both prior incarnations. Each operation is
//! then rebuilt against the current incarnation and must land with an observable
//! economic delta, preventing an always-rejecting implementation from satisfying
//! the matrix. The trace schema additionally proves every lifecycle edge is a
//! real public transaction.

use crate::support::invariant_discovery::{
    discover_portfolio_incarnation_replays, PortfolioIntentKind,
};

#[test]
fn v16_program_all_retained_portfolio_intents_reject_after_same_pubkey_recreate() {
    let discoveries = discover_portfolio_incarnation_replays([0x03; 32])
        .unwrap_or_else(|error| panic!("INV-003 matrix failed: {error}"));
    assert_eq!(discoveries.len(), PortfolioIntentKind::ALL.len());

    for (expected, discovery) in PortfolioIntentKind::ALL.into_iter().zip(&discoveries) {
        assert_eq!(discovery.kind, expected);
        assert!(
            discovery.intermediate_portfolio_id > discovery.old_portfolio_id
                && discovery.new_portfolio_id > discovery.intermediate_portfolio_id,
            "{expected:?}: portfolio id did not advance across A-B-A recreation: {} -> {} -> {}",
            discovery.old_portfolio_id,
            discovery.intermediate_portfolio_id,
            discovery.new_portfolio_id,
        );
        assert!(
            !discovery.accepted_stale_intent,
            "{expected:?}: stale retained portfolio intent landed on a replacement account"
        );
        assert!(
            !discovery.mutated_economic_state,
            "{expected:?}: rejected stale intent failed exact rollback"
        );
        assert_eq!(
            discovery.compute_units, None,
            "{expected:?}: stale replay should have no successful CU result"
        );
        assert_eq!(
            discovery.public_trace.out_of_band_economic_mutations, 0,
            "{expected:?}: replay evidence must use public transitions only",
        );
        let replay = discovery
            .public_trace
            .steps
            .last()
            .expect("A-B-A trace includes the stale replay");
        assert!(!replay.succeeded, "{expected:?}: stale replay trace");
        assert_eq!(
            replay.rejected_exact_writable_rollback,
            Some(true),
            "{expected:?}: stale replay must roll back every writable account",
        );
        assert!(
            replay.token_deltas.iter().all(|(_, delta)| *delta == 0),
            "{expected:?}: stale replay must move no SPL value",
        );
        assert!(
            discovery.fresh_intent_landed,
            "{expected:?}: current-incarnation control must remain executable: {:?}",
            discovery.fresh_error,
        );
        assert!(
            discovery.fresh_mutated_economic_state,
            "{expected:?}: current-incarnation control must produce a nonvacuous economic delta",
        );
        assert!(
            discovery.fresh_compute_units.is_some(),
            "{expected:?}: current-incarnation control needs a successful CU result",
        );
        let fresh_trace = discovery
            .fresh_public_trace
            .as_ref()
            .unwrap_or_else(|| panic!("{expected:?}: missing fresh trace"));
        assert_eq!(
            fresh_trace.out_of_band_economic_mutations, 0,
            "{expected:?}: current control must use public transitions only",
        );
        let fresh = fresh_trace
            .steps
            .last()
            .unwrap_or_else(|| panic!("{expected:?}: empty fresh trace"));
        assert!(fresh.succeeded, "{expected:?}: current control trace");
    }
}
