//! INV-034 - Domain and instance isolation.
//!
//! Normative obligation: Value and liabilities cannot cross market instances or source domains without an explicit rule.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_cross_margin_debt_cannot_drain_unrelated_insurance`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: this certifies the independently derived cross-margin loss-detach route.
//! The complete cross-instance account-substitution matrix remains tracked separately.

use crate::support::invariant_discovery::discover_cross_domain_insurance_violation;

#[test]
fn v16_program_cross_margin_debt_cannot_drain_unrelated_insurance() {
    let discovery = discover_cross_domain_insurance_violation([0x90; 32])
        .unwrap_or_else(|error| panic!("cross-domain certification failed: {error}"));
    assert!(
        discovery.preserves_domain_isolation_and_exit(),
        "{discovery:?}"
    );
}
