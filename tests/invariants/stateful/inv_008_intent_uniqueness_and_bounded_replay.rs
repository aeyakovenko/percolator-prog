//! INV-008 - Intent uniqueness and bounded replay.
//!
//! Normative obligation: One retained economic intent can execute at most once across routes and retries.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_retry_operation_matrix_discovers_duplicate_economic_execution` generates
//! signature-distinct retries from one economic-operation registry without finding metadata.
//! Direct impact regressions remain below. These tests exercise the deployed public
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
                "proptest-regressions/inv_008_intent_retry_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_retry_operation_matrix_discovers_duplicate_economic_execution(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_intent_retries(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), RetryIntentKind::ALL.len());
        for (expected, discovery) in RetryIntentKind::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.kind, expected);
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.kind)
            .collect();
        eprintln!("independent INV-008 discoveries: {violations:?}");
        let expected_violations: Vec<_> = RetryIntentKind::ALL
            .into_iter()
            .filter(|kind| *kind != RetryIntentKind::RebalanceReduce)
            .collect();
        prop_assert_eq!(
            violations,
            expected_violations,
            "exact-once discovery/protection corpus changed"
        );
        let rebalance = discoveries
            .iter()
            .find(|discovery| discovery.kind == RetryIntentKind::RebalanceReduce)
            .expect("rebalance retry discovery");
        prop_assert!(!rebalance.accepted_retry);
        prop_assert!(!rebalance.duplicated_economic_effect);
        prop_assert_eq!(rebalance.retry_compute_units, None);
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

    #[test]
    fn v16_program_conversion_retry_replay_fuzz(seed in any::<[u8; 32]>()) {
        let result = reproduce_convert_retry_replay(seed);
        prop_assert!(
            result.is_ok(),
            "conversion retry no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }
}
