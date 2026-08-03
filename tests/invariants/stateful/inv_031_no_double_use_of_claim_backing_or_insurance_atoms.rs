//! INV-031 - No double use of claim, backing, or insurance atoms.
//!
//! Normative obligation: A backing or claim atom cannot support two economic obligations.
//!
//! Evidence in this file (F over public I routes): `v16_program_pr267_cross_domain_backing_double_spend_fuzz`. These tests exercise the deployed public
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
                "proptest-regressions/v16_program_stateful_fuzz.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_pr267_cross_domain_backing_double_spend_fuzz(
        seed in cross_domain_backing_seed_strategy()
    ) {
        let result = reproduce_cross_domain_backing_double_spend(seed);
        prop_assert!(
            result.is_ok(),
            "PR 267 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }
}
