//! INV-008 - Intent uniqueness and bounded replay.
//!
//! Normative obligation: One retained economic intent can execute at most once across routes and retries.
//!
//! Evidence in this file (F over public I routes): `v16_program_pr343_trade_retry_replay_fuzz`, `v16_program_pr344_insurance_top_up_retry_replay_fuzz`, `v16_program_pr362_activation_retry_replay_fuzz`, `v16_program_pr351_backing_top_up_retry_replay_fuzz`, `v16_program_pr350_deposit_retry_replay_fuzz`, `v16_program_pr355_withdrawal_retry_liquidation_fuzz`. These tests exercise the deployed public
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
    fn v16_program_pr343_trade_retry_replay_fuzz(
        (seed, route) in trade_retry_replay_strategy()
    ) {
        let result = reproduce_trade_retry_replay(seed, route);
        prop_assert!(
            result.is_ok(),
            "PR 343 {:?} no longer reproduces for seed {:?}: {}",
            route,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr344_insurance_top_up_retry_replay_fuzz(
        seed in insurance_top_up_retry_replay_seed_strategy()
    ) {
        let result = reproduce_insurance_top_up_retry_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 344 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr362_activation_retry_replay_fuzz(
        seed in activation_retry_replay_seed_strategy()
    ) {
        let result = reproduce_activation_retry_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 362 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr351_backing_top_up_retry_replay_fuzz(
        seed in backing_top_up_retry_replay_seed_strategy()
    ) {
        let result = reproduce_backing_top_up_retry_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 351 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr350_deposit_retry_replay_fuzz(
        seed in deposit_retry_replay_seed_strategy()
    ) {
        let result = reproduce_deposit_retry_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 350 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr355_withdrawal_retry_liquidation_fuzz(
        seed in withdrawal_retry_liquidation_seed_strategy()
    ) {
        let result = reproduce_withdrawal_retry_liquidation(seed);
        prop_assert!(
            result.is_ok(),
            "PR 355 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }
}
