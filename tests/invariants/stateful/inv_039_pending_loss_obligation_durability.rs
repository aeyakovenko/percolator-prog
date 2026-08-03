//! INV-039 - Pending-loss obligation durability.
//!
//! Normative obligation: Pending accrual and loss obligations cannot be erased by route choice or lifecycle changes.
//!
//! Evidence in this file (F over public I routes): `v16_program_pr380_prospective_funding_rewrite_fuzz`, `v16_program_pr255_resolve_before_committed_accrual_fuzz`, `v16_program_pr271_trade_funding_erasure_fuzz`, `v16_program_pr272_rebalance_funding_erasure_fuzz`, `v16_program_pr273_forfeit_funding_erasure_fuzz`. These tests exercise the deployed public
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
    fn v16_program_pr380_prospective_funding_rewrite_fuzz(
        (seed, route) in prospective_funding_rewrite_strategy()
    ) {
        let result = reproduce_prospective_funding_rewrite(seed, route);
        prop_assert!(
            result.is_ok(),
            "PR 380 {:?} no longer reproduces for seed {:?}: {}",
            route,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr255_resolve_before_committed_accrual_fuzz(
        seed in resolve_before_committed_accrual_seed_strategy()
    ) {
        let result = reproduce_resolve_before_committed_accrual(seed);
        prop_assert!(
            result.is_ok(),
            "PR 255 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr271_trade_funding_erasure_fuzz(
        (seed, route) in trade_funding_erasure_strategy()
    ) {
        let result = reproduce_trade_funding_erasure(seed, route);
        prop_assert!(
            result.is_ok(),
            "PR 271 {:?} no longer reproduces for seed {:?}: {}",
            route,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr272_rebalance_funding_erasure_fuzz(
        seed in rebalance_funding_erasure_seed_strategy()
    ) {
        let result = reproduce_rebalance_funding_erasure(seed);
        prop_assert!(
            result.is_ok(),
            "PR 272 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr273_forfeit_funding_erasure_fuzz(
        seed in forfeit_funding_erasure_seed_strategy()
    ) {
        let result = reproduce_forfeit_funding_erasure(seed);
        prop_assert!(
            result.is_ok(),
            "PR 273 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }
}
