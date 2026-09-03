//! INV-034 - Domain and instance isolation.
//!
//! Normative obligation: Value and liabilities cannot cross market instances or source domains without an explicit rule.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_cross_asset_debt_preserves_foreign_insurance_and_owner_exit` funds one source domain,
//! creates debt on another asset in a cross-margin portfolio, and drives only public liquidation
//! and terminal routes. Its oracle proves the stale keeper attempt rolls back, the owner can still
//! flatten the surviving leg, no foreign-domain insurance is spent, and no coalition SPL profit is
//! created. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: this certifies the independently derived cross-margin loss-detach route.
//! The complete cross-instance account-substitution matrix remains tracked separately.

use super::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_034_cross_domain_insurance_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_cross_asset_debt_preserves_foreign_insurance_and_owner_exit(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_cross_domain_insurance_violation(seed)
            .map_err(TestCaseError::fail)?;
        eprintln!("independent cross-domain insurance discovery: {discovery:?}");
        prop_assert!(
            discovery.preserves_domain_isolation_and_exit(),
            "cross-domain insurance isolation or owner exit regressed: {:?}",
            discovery
        );
    }
}
