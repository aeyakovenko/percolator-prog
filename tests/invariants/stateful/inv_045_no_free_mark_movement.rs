//! INV-045 - No free mark movement.
//!
//! Normative obligation: Every mark movement remains elapsed-time bounded and economically paid across every trade route.
//!
//! Evidence in this file (F over public I routes): `v16_program_pr260_pending_ewma_inheritance_fuzz`, `v16_program_pr282_pending_ewma_target_override_fuzz`, `v16_program_pr264_pr265_pr332_pr333_unstaged_mark_target_fuzz`, `v16_program_pr356_pending_mark_fee_reward_fuzz`, `v16_program_pr369_bilateral_fee_support_fuzz`, `v16_program_pr225_reclaimable_ewma_fee_fuzz`, `v16_program_pr280_trade_driven_liquidation_reward_fuzz`. These tests exercise the deployed public
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
    fn v16_program_pr260_pending_ewma_inheritance_fuzz(
        (seed, route) in pending_ewma_inheritance_strategy()
    ) {
        let result = reproduce_pending_ewma_inheritance(seed, route);
        prop_assert!(
            result.is_ok(),
            "PR 260 {:?} no longer reproduces for seed {:?}: {}",
            route,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr282_pending_ewma_target_override_fuzz(
        (seed, route) in pending_ewma_target_override_strategy()
    ) {
        let result = reproduce_pending_ewma_target_override(seed, route);
        prop_assert!(
            result.is_ok(),
            "PR 282 {:?} no longer reproduces for seed {:?}: {}",
            route,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr264_pr265_pr332_pr333_unstaged_mark_target_fuzz(
        (seed, case) in target_staging_strategy()
    ) {
        let result = reproduce_unstaged_mark_target(seed, case);
        prop_assert!(
            result.is_ok(),
            "{:?} no longer reproduces for seed {:?}: {}",
            case,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr356_pending_mark_fee_reward_fuzz(
        seed in pending_mark_fee_reward_seed_strategy()
    ) {
        let result = reproduce_pending_mark_fee_reward(seed);
        prop_assert!(
            result.is_ok(),
            "PR 356 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr369_bilateral_fee_support_fuzz(
        (seed, mode, route) in bilateral_fee_support_strategy()
    ) {
        let result = reproduce_bilateral_fee_support(seed, mode, route);
        prop_assert!(
            result.is_ok(),
            "PR 369 {:?} {:?} no longer reproduces for seed {:?}: {}",
            mode,
            route,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr225_reclaimable_ewma_fee_fuzz(
        (seed, route) in reclaimable_ewma_fee_strategy()
    ) {
        let result = reproduce_reclaimable_ewma_fee(seed, route);
        prop_assert!(
            result.is_ok(),
            "PR 225 {:?} no longer reproduces for seed {:?}: {}",
            route,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr280_trade_driven_liquidation_reward_fuzz(
        (seed, mode, route) in trade_driven_liquidation_reward_strategy()
    ) {
        let result = reproduce_trade_driven_liquidation_reward(seed, mode, route);
        prop_assert!(
            result.is_ok(),
            "PR 280 {:?} {:?} no longer reproduces for seed {:?}: {}",
            mode,
            route,
            seed,
            result.unwrap_err()
        );
    }
}
