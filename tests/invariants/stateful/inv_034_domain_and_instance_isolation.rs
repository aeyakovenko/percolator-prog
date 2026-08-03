//! INV-034 - Domain and instance isolation.
//!
//! Normative obligation: Value and liabilities cannot cross market instances or source domains without an explicit rule.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_cross_asset_debt_discovers_foreign_insurance_spend` funds one source domain,
//! creates debt on another asset in a cross-margin portfolio, and drives only public liquidation
//! and terminal routes. Its oracle rejects any foreign-domain spend and verifies coalition SPL
//! profit from that spend. Direct impact tests remain below. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

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
    fn v16_program_cross_asset_debt_discovers_foreign_insurance_spend(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_cross_domain_insurance_violation(seed)
            .map_err(TestCaseError::fail)?;
        eprintln!("independent cross-domain insurance discovery: {discovery:?}");
        prop_assert!(
            discovery.is_violation(),
            "vulnerable-pin cross-domain insurance behavior changed: {:?}",
            discovery
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/v16_program_stateful_fuzz.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_pr290_cross_margin_insurance_drain_fuzz(
        seed in cross_margin_insurance_drain_seed_strategy()
    ) {
        let result = reproduce_cross_margin_insurance_drain(seed);
        prop_assert!(
            result.is_ok(),
            "PR 290 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }
}
