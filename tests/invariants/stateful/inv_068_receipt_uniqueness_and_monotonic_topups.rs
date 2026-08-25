//! INV-068 - Receipt uniqueness and monotonic top-ups.
//!
//! This public LiteSVM lifecycle creates an underfunded resolved receipt without writing
//! program-owned bytes. Two independent backing releases raise the terminal payout rate at
//! authenticated slots 13 and 14. `ClaimResolvedPayoutTopup` must increase the same immutable
//! receipt by exactly the SPL/vault payout after each release; an immediate retry after the
//! initial payment and both top-ups must land as an exact no-op. The shared terminal campaign then
//! proves the split schedule does not strand any funded portfolio.
//!
//! The shared route oracle also requires receipt face/prior-bound identity to remain immutable,
//! cumulative paid value to be monotonic, every claim delta to equal its external token delta, and
//! engine/SPL vault custody to reconcile after every successful instruction.

use super::*;

#[test]
fn v16_program_resolved_receipt_accepts_two_exact_topups_and_idempotent_retries() {
    let evidence = verify_resolved_receipt_split_topups()
        .expect("public split resolved-receipt top-up lifecycle");

    assert!(evidence.initial_paid < evidence.first_paid);
    assert!(evidence.first_paid < evidence.second_paid);
    assert!(evidence.second_paid < evidence.receipt_face);
    assert_eq!(
        evidence.first_paid - evidence.initial_paid,
        evidence.first_payout
    );
    assert_eq!(
        evidence.second_paid - evidence.first_paid,
        evidence.second_payout
    );
    assert_eq!(evidence.exact_noop_retries, 3);
    assert_eq!(evidence.terminal_actor_count, 5);
    assert_eq!(evidence.final_engine_vault, evidence.final_spl_vault);
}
