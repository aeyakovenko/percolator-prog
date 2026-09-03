//! INV-017 - Signer, writable-role, and account-alias safety.
//!
//! This test builds a genuinely partial resolved receipt entirely through public
//! instructions, releases enough backing for a nonzero top-up, and pauses before
//! `ClaimResolvedPayoutTopup`. The canonical route must pay. Every pair among its
//! seven semantic account roles and every required writable downgrade must then
//! reject with an exact full-economic-state rollback.

use crate::support::fuzz_model::verify_resolved_claim_account_alias_matrix;

#[test]
fn v16_program_resolved_claim_account_pairs_and_privileges_are_exhaustive() {
    let evidence = verify_resolved_claim_account_alias_matrix()
        .unwrap_or_else(|error| panic!("INV-017 resolved-claim alias matrix: {error}"));
    assert!(evidence.control_payout_atoms > 0);
    assert_eq!(evidence.rejected_pair_count, 21);
    assert_eq!(evidence.rejected_writable_downgrade_count, 4);
}
