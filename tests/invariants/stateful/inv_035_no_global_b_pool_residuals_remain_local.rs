//! INV-035 - No global B pool; residuals remain local.
//!
//! Normative obligation: Bankruptcy residuals stay in the exact asset and opposing-side domain that created them.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_two_asset_bankruptcy_discovers_cross_domain_b_settlement` creates claims in two
//! source domains, books bankruptcy B in one asset, and independently recomputes both claim deltas.
//! It then searches every bounded public reduction candidate and honest crank before declaring a
//! persistent funded exit lock. Direct impact tests remain below. These tests exercise the deployed public
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
                "proptest-regressions/inv_035_cross_domain_b_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_two_asset_bankruptcy_discovers_cross_domain_b_settlement(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_cross_domain_b_violation(seed)
            .map_err(TestCaseError::fail)?;
        eprintln!("independent cross-domain B discovery: {discovery:?}");
        prop_assert!(
            discovery.is_violation(),
            "vulnerable-pin B attribution changed: {:?}",
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
    fn v16_program_pr281_cross_domain_b_settlement_fuzz(
        seed in cross_domain_b_settlement_seed_strategy()
    ) {
        let result = reproduce_cross_domain_b_settlement(seed);
        prop_assert!(
            result.is_ok(),
            "PR 281 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }
}
