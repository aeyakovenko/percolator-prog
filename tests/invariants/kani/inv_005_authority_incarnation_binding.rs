//! INV-005 - Authority incarnation binding.
//!
//! These proofs exhaust the deployed scalar predicates used by market resolve and authority
//! handoff routes. Public SBF traces establish wrapper composition, exact rollback, and bounded
//! user exits; these proofs establish that no full-width epoch value can bypass exact matching or
//! wrap into a reusable incarnation.

use percolator_prog::state;

#[kani::proof]
fn kani_v16_authority_epoch_accepts_exactly_the_current_incarnation() {
    let current: u64 = kani::any();
    let expected: u64 = kani::any();
    let accepted = state::require_current_authority_epoch(current, expected).is_ok();

    kani::cover!(accepted, "a current authority epoch is accepted");
    kani::cover!(!accepted, "a stale authority epoch is rejected");
    assert_eq!(accepted, current == expected);
}

#[kani::proof]
fn kani_v16_authority_handoff_advances_once_and_fails_closed_at_exhaustion() {
    let current: u64 = kani::any();
    let expected: u64 = kani::any();
    let result = state::next_authority_epoch(current, expected);

    kani::cover!(
        result.is_ok(),
        "a live non-exhausted authority handoff advances"
    );
    kani::cover!(
        current != expected && result.is_err(),
        "a stale authority handoff rejects"
    );
    kani::cover!(
        current == u64::MAX && expected == current && result.is_err(),
        "an exhausted authority epoch rejects without wrapping"
    );

    match result {
        Ok(next) => {
            assert_eq!(current, expected);
            assert!(current < u64::MAX);
            assert_eq!(next, current + 1);
            assert!(state::require_current_authority_epoch(next, expected).is_err());
        }
        Err(_) => assert!(current != expected || current == u64::MAX),
    }
}

#[kani::proof]
fn kani_v16_backing_policy_migration_preserves_both_legacy_watermarks() {
    let legacy_long: u64 = kani::any();
    let legacy_short_or_epoch: u64 = kani::any();
    let proposed: u64 = kani::any();
    let floor = state::backing_fee_sequence_floor(legacy_long, legacy_short_or_epoch);

    assert!(floor >= legacy_long);
    assert!(floor >= legacy_short_or_epoch);
    assert!(floor == legacy_long || floor == legacy_short_or_epoch);
    assert_eq!(
        state::require_newer_control_sequence(floor, proposed).is_ok(),
        proposed > legacy_long && proposed > legacy_short_or_epoch
    );
}
