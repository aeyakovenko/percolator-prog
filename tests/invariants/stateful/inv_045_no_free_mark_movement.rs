//! INV-045 - No free mark movement.
//!
//! Normative obligation: Every mark movement remains elapsed-time bounded and economically paid across every trade route.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_mark_publication_matrix_discovers_stale_risk_admission` publishes marks through
//! authenticated, EWMA, single-trade, and batch-trade routes, then applies one common oracle:
//! wrapper/engine mark lag cannot admit stale-price risk whose later close transfers and extracts
//! another user's capital.
//! `v16_program_trade_route_matrix_discovers_pending_mark_inheritance` signs exposure before a
//! paid mark move and lands it through every trade route while the move is pending. Its oracle
//! requires movement cost to cover any later third-party value transfer and verifies net SPL
//! extraction. Direct impact tests remain below. These tests exercise the deployed public
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
                "proptest-regressions/inv_045_mark_admission_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_mark_publication_matrix_discovers_stale_risk_admission(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_pending_mark_admission_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), PendingMarkSource::ALL.len());
        for (expected, discovery) in PendingMarkSource::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.source, expected);
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.source)
            .collect();
        eprintln!("independent pending-mark admission discoveries: {violations:?}");
        prop_assert_eq!(
            violations,
            PendingMarkSource::ALL.to_vec(),
            "vulnerable-pin pending-mark admission corpus changed"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_045_pending_mark_inheritance_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_trade_route_matrix_discovers_pending_mark_inheritance(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_pending_mark_inheritance_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), DiscoveryTradeRoute::ALL.len());
        for (expected, discovery) in DiscoveryTradeRoute::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.route, expected);
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.route)
            .collect();
        eprintln!("independent pending-mark inheritance discoveries: {violations:?}");
        prop_assert_eq!(
            violations,
            DiscoveryTradeRoute::ALL.to_vec(),
            "vulnerable-pin pending-mark inheritance corpus changed"
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
