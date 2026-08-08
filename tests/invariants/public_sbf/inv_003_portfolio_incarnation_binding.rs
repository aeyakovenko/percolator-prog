//! INV-003 - Portfolio incarnation binding.
//!
//! Normative obligation: every retained portfolio-scoped request binds the current
//! program-assigned `portfolio_id`, not only the portfolio pubkey. Closing and
//! recreating the same pubkey must make old consent unusable before any economic
//! or lamport mutation.
//!
//! Evidence in this file (I): one public SBF/LiteSVM matrix builds retained
//! requests for every portfolio-scoped public route, closes and recreates the
//! same portfolio pubkey, then replays the old transaction. The assertion is
//! route-local: the stale request must reject, exact tracked state must roll
//! back, SPL supply must stay fixed, and the replacement account's new
//! `portfolio_id` must be larger than the old one.

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
            discovery.new_portfolio_id > discovery.old_portfolio_id,
            "{expected:?}: portfolio id did not advance across close/recreate"
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
    }
}
