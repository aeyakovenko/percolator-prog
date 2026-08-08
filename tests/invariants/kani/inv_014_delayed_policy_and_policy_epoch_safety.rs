//! INV-014: delayed policy and policy-epoch safety.
//!
//! Retained wrapper controls carry a sequence selected by their authority. The
//! production admission predicate must accept every strictly newer sequence and
//! reject replays or older controls. Public SBF tests cover route composition and
//! rollback; this proof pins the shared scalar predicate used by those routes.

use percolator_prog::state::require_newer_control_sequence;

#[kani::proof]
fn kani_v16_control_sequence_accepts_exactly_strictly_newer_values() {
    let current: u64 = kani::any();
    let proposed: u64 = kani::any();

    assert_eq!(
        require_newer_control_sequence(current, proposed).is_ok(),
        proposed > current
    );
}
