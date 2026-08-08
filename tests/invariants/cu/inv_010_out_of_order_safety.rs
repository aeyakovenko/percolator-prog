//! INV-010 - Out-of-order safety.
//!
//! Normative obligation: retained public requests that land out of order either
//! reject atomically or remain inside every affected signer’s latest authority
//! and economic bounds.
//!
//! Evidence in this file (I/F): this deterministic LiteSVM witness exercises an
//! LP-signed retained matcher-enable request. After the LP revokes matcher
//! authority, CPI trade attempts and the stale retained enable must reject with
//! exact rollback and unchanged matcher sequence. A fresh enable then lands,
//! CPI open/close succeeds, both parties withdraw, and SPL supply is conserved.
//!
//! Guarantee boundary: this covers portfolio-scoped matcher capability
//! supersession. Other retained policy domains are owned by INV-014.

#[test]
fn v16_program_matcher_mutation_order_rejects_revoked_capability_fixed_case() {
    let protection =
        crate::support::invariant_discovery::verify_matcher_mutation_order_safety([0x10; 32])
            .expect("matcher mutation order safety");
    assert!(
        protection.satisfies_invariant(),
        "matcher mutation order invariant failed: {protection:?}"
    );
}
