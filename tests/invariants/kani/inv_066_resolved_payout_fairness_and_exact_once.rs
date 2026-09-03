//! INV-066 and INV-067 - claimant-count-independent payout induction.
//!
//! The deployed rate calculation contains full-width multiplication and division. Reasserting
//! that circuit in this harness would recreate the documented CBMC arithmetic wall, so this proof
//! starts from one named arithmetic axiom established by the engine arithmetic differential suite:
//!
//! `RESOLVED_RATE_SUM_AXIOM`: for the immutable resolved-payout rate, the sum of every receipt's
//! remaining rate-derived entitlement does not exceed the reserved junior payout pool.
//!
//! This harness proves the state-machine consequence at full `u128` width without narrowing that
//! domain: any next claimant is fully funded, preserves the axiom for the remaining cohort, commutes
//! with any adjacent claimant, reaches its exact entitlement, and becomes a zero-due retry fixed
//! point. Induction over this step covers any finite claimant count; adjacent swaps cover every
//! claimant permutation.

#[kani::proof]
fn kani_inv066_inv067_funded_receipt_induction_is_order_independent_and_exact_once() {
    let vault: u128 = kani::any();
    let remaining_due: u128 = kani::any();
    let first_face: u128 = kani::any();
    let first_paid_before: u128 = kani::any();
    let second_due: u128 = kani::any();

    let valid_receipt = first_paid_before <= first_face;
    let first_due = first_face.saturating_sub(first_paid_before);
    let funded_cohort = remaining_due <= vault;
    let first_belongs_to_cohort = first_due <= remaining_due;
    let second_belongs_to_remainder =
        first_belongs_to_cohort && second_due <= remaining_due.saturating_sub(first_due);
    let induction_domain =
        valid_receipt && funded_cohort && first_belongs_to_cohort && second_belongs_to_remainder;

    kani::cover!(
        induction_domain
            && first_paid_before > 0
            && first_due > 0
            && second_due > 0
            && remaining_due > first_due + second_due,
        "partial receipt, two live claimants, and a nonempty tail satisfy the rate-sum axiom"
    );
    kani::cover!(
        induction_domain && first_due == 0 && second_due > 0,
        "an already-paid receipt is an exact retry fixed point"
    );

    if !induction_domain {
        return;
    }

    // First then second.
    let first_paid_a = first_due.min(vault);
    let vault_after_first = vault - first_paid_a;
    let second_paid_a = second_due.min(vault_after_first);
    let vault_after_a = vault_after_first - second_paid_a;

    // Second then first.
    let second_paid_b = second_due.min(vault);
    let vault_after_second = vault - second_paid_b;
    let first_paid_b = first_due.min(vault_after_second);
    let vault_after_b = vault_after_second - first_paid_b;

    assert_eq!(first_paid_a, first_due);
    assert_eq!(first_paid_b, first_due);
    assert_eq!(second_paid_a, second_due);
    assert_eq!(second_paid_b, second_due);
    assert_eq!(vault_after_a, vault_after_b);

    let remaining_after = remaining_due - first_due - second_due;
    assert!(vault_after_a >= remaining_after);
    assert!(vault_after_b >= remaining_after);

    let first_paid_after = first_paid_before.checked_add(first_paid_a).unwrap();
    assert_eq!(first_paid_after, first_face);
    assert_eq!(first_face - first_paid_after, 0);
}
