//! INV-039 - Pending-loss obligation durability.
//!
//! Normative obligation: Pending accrual and loss obligations cannot be erased by route choice or lifecycle changes.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_accrual_boundary_operation_matrix_discovers_erased_transfers` builds one
//! zero-price-move funding checkpoint and permutes settlement against CPI close, batch CPI close,
//! unilateral reduction, and recovery forfeit. The common oracle requires both sides to book the
//! same nonzero transfer and compares conserved claims across the two orders.
//! `v16_program_prospective_accrual_route_matrix_discovers_timestamp_rewrite` independently
//! varies single and batch no-CPI trade routes around the same funding catch-up boundary. It
//! requires identical terminal prices and total payout while detecting an erased funding index,
//! an equal victim payout loss, and coalition gain.
//! `v16_program_shutdown_commit_ordering_discovers_erased_funding` applies the same ordering
//! oracle to asset shutdown while constraining the effective price to remain unchanged. Any payout
//! difference is therefore a committed funding transfer erased by the lifecycle transition.
//! `v16_program_stale_cohort_route_matrix_discovers_unsigned_historical_loss` realizes
//! source-backed PnL while the losing cohort is stale, varies the unsigned CPI route used to
//! novate the winning exposure, and then settles and exits every participant. Its token oracle
//! requires the fresh LP's exact principal loss plus the original loser's loss to equal the
//! stale winner's extracted profit.
//! Direct impact tests remain below. These tests
//! exercise the deployed public
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
                "proptest-regressions/inv_039_accrual_ordering_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_accrual_boundary_operation_matrix_discovers_erased_transfers(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_accrual_ordering_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), AccrualOrderingKind::ALL.len());
        for (expected, discovery) in AccrualOrderingKind::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.kind, expected);
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.kind)
            .collect();
        eprintln!("independent accrual-ordering discoveries: {violations:?}");
        prop_assert_eq!(
            violations,
            AccrualOrderingKind::ALL.to_vec(),
            "vulnerable-pin accrual-ordering corpus changed"
        );
    }

    #[test]
    fn v16_program_shutdown_commit_ordering_discovers_erased_funding(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_shutdown_commit_ordering(seed)
            .map_err(TestCaseError::fail)?;
        eprintln!("independent shutdown-commit discovery: {discovery:?}");
        prop_assert!(
            discovery.is_violation(),
            "vulnerable-pin shutdown ordering changed: {discovery:?}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 4) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 8) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_039_stale_cohort_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_stale_cohort_route_matrix_discovers_unsigned_historical_loss(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_stale_cohort_novations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), StaleCohortRoute::ALL.len());
        for (expected, discovery) in StaleCohortRoute::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.route, expected);
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.route)
            .collect();
        eprintln!("independent stale-cohort novation discoveries: {discoveries:?}");
        prop_assert_eq!(
            violations,
            StaleCohortRoute::ALL.to_vec(),
            "vulnerable-pin stale-cohort route behavior changed"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_039_prospective_accrual_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_prospective_accrual_route_matrix_discovers_timestamp_rewrite(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_prospective_accrual_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), ProspectiveAccrualRoute::ALL.len());
        for (expected, discovery) in ProspectiveAccrualRoute::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.route, expected);
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.route)
            .collect();
        eprintln!("independent prospective-accrual discoveries: {violations:?}");
        prop_assert_eq!(
            violations,
            ProspectiveAccrualRoute::ALL.to_vec(),
            "vulnerable-pin prospective-accrual corpus changed"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_039_terminal_commit_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_terminal_commit_ordering_discovers_discarded_pending_value(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_terminal_commit_ordering(seed)
            .map_err(TestCaseError::fail)?;
        eprintln!("independent terminal-commit discovery: {discovery:?}");
        prop_assert!(
            discovery.is_violation(),
            "vulnerable-pin terminal-commit ordering changed: {:?}",
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
