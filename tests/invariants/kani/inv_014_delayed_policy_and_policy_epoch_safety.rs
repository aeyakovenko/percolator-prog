//! INV-014: delayed policy and policy-epoch safety.
//!
//! Retained wrapper controls carry a sequence selected by their authority. The
//! production admission predicate must accept every strictly newer sequence and
//! reject replays or older controls. Public SBF tests cover route composition and
//! rollback; this proof pins the shared scalar predicate used by those routes.

use percolator::{BackingBucketStatusV16, BackingBucketV16};
use percolator_prog::{
    policy_v16::backing_fee_policy_change_allowed, state::require_newer_control_sequence,
};

#[kani::proof]
fn kani_v16_control_sequence_accepts_exactly_strictly_newer_values() {
    let current: u64 = kani::any();
    let proposed: u64 = kani::any();

    assert_eq!(
        require_newer_control_sequence(current, proposed).is_ok(),
        proposed > current
    );
}

#[kani::proof]
fn kani_v16_backing_fee_policy_changes_require_an_empty_provider_domain() {
    let current_fee_bps: u16 = kani::any();
    let current_insurance_share_bps: u16 = kani::any();
    let proposed_fee_bps: u16 = kani::any();
    let proposed_insurance_share_bps: u16 = kani::any();
    let bucket = BackingBucketV16 {
        market_id: kani::any(),
        fresh_unliened_backing_num: kani::any(),
        valid_liened_backing_num: kani::any(),
        consumed_liened_backing_num: kani::any(),
        impaired_liened_backing_num: kani::any(),
        utilization_fee_earnings: kani::any(),
        expiry_slot: kani::any(),
        status: BackingBucketStatusV16::Empty,
    };
    let terms_unchanged = (current_fee_bps, current_insurance_share_bps)
        == (proposed_fee_bps, proposed_insurance_share_bps);
    let provider_domain_empty = bucket.fresh_unliened_backing_num == 0
        && bucket.valid_liened_backing_num == 0
        && bucket.consumed_liened_backing_num == 0
        && bucket.impaired_liened_backing_num == 0
        && bucket.utilization_fee_earnings == 0;

    assert_eq!(
        backing_fee_policy_change_allowed(
            current_fee_bps,
            current_insurance_share_bps,
            proposed_fee_bps,
            proposed_insurance_share_bps,
            &bucket,
        ),
        terms_unchanged || provider_domain_empty,
    );
}
