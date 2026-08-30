//! INV-001 - Market incarnation binding.
//!
//! The deployed wrapper enforces a stricter policy than reusable generation IDs: once
//! `CloseSlab` succeeds, that market pubkey is permanently retired behind a typed,
//! rent-exempt tombstone. This representative public route proves same-address
//! `InitMarket` and a retained terminal control both reject with exact rollback.
//! INV-007 owns the complete retained-operation matrix and fresh-address liveness.

use crate::support::invariant_discovery::{discover_market_incarnation_replay, MarketIntentKind};

#[test]
fn v16_program_closed_market_incarnation_cannot_be_recreated() {
    let protection =
        discover_market_incarnation_replay([0x01; 32], MarketIntentKind::ResolveMarket)
            .unwrap_or_else(|error| panic!("INV-001 market retirement failed: {error}"));
    assert!(protection.certifies_no_reuse(), "{protection:?}");
    assert!(protection.recreation_rejected);
    assert!(protection.recreation_exact_rollback);
    assert!(protection.retained_intent_rejected);
    assert!(protection.retained_intent_exact_rollback);
}
