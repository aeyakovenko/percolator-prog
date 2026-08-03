//! INV-063 - Backing-expiry normalization.
//!
//! Normative obligation: Expired backing is normalized before every consumer and cannot remain economically fresh.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr367_post_expiry_backing_fee_is_extractable`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_pr367_post_expiry_backing_fee_is_extractable() {
    let reproduction = reproduce_post_expiry_backing_fee(
        [0x67; 32],
        PostExpiryBackingCase {
            fee_bps: 5_000,
            expiry_offset: 2,
            mark_move_bps: 500,
            increase_divisor: 20,
        },
    )
    .expect("PR 367 no longer reproduces; remove its quarantine and promote the seed");

    assert_eq!(reproduction.blocker, KnownBlocker::PostExpiryBackingFee);
    assert_eq!(
        reproduction.provider_earnings,
        u128::from(reproduction.extracted_tokens),
        "the protocol ledger and extracted SPL amount diverged"
    );
    assert_eq!(
        reproduction.victim_capital_loss, reproduction.provider_earnings,
        "the public reproduction did not transfer the trader's loss to the provider"
    );
}
