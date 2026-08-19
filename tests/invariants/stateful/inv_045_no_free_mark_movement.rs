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
//! extraction. `v16_program_trade_route_matrix_discovers_pending_target_override` compares an
//! honest pending rebound with the same world plus a later round trip. It rejects any cheap target
//! rewrite that displaces more independent payout than it costs.
//! `v16_program_pending_mark_fee_ordering_rejects_and_preserves_terminal_value` permutes fee
//! synchronization against mark commitment. It requires the pending-order attempt to reject with
//! exact rollback, then verifies the post-commit retry and terminal payouts equal the canonical
//! ordering. `v16_program_trade_route_matrix_discovers_withdrawable_mark_reserve` creates a paid mark
//! move, withdraws its reserve while unrelated exposure depends on it, and verifies the resulting
//! victim loss and coalition SPL gain across all trade routes.
//! `v16_program_mark_mode_route_matrix_discovers_profitable_liquidation_moves` crosses EWMA and
//! hybrid-after-hours modes with single and batch reported-price routes, then requires total
//! movement cost to cover any liquidation reward and coalition extraction.
//! `v16_program_matcher_route_matrix_rejects_one_sided_mark_subsidy` crosses the same modes with
//! single and batch CPI matcher exits and requires every mark-moving fee to be bilaterally funded;
//! it measures independent victim loss, fee-counterparty loss, insurance credit, and external
//! coalition profit. Direct impact tests remain below. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: the pending-mark fee-order and bilateral-fee matrices are fixed-pin
//! certification over generated seeds. The other named generators still expose quarantined
//! counterexamples and do not certify their sub-routes until the corresponding fixes are integrated.

use super::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_045_bilateral_mark_fee_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_matcher_route_matrix_rejects_one_sided_mark_subsidy(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_bilateral_mark_fee_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), TradeDrivenMarkMode::ALL.len() * 2);
        let covered: Vec<_> = discoveries
            .iter()
            .map(|discovery| (discovery.mode, discovery.route))
            .collect();
        let expected: Vec<_> = TradeDrivenMarkMode::ALL
            .into_iter()
            .flat_map(|mode| {
                [DiscoveryTradeRoute::Cpi, DiscoveryTradeRoute::BatchCpi]
                    .map(|route| (mode, route))
            })
            .collect();
        eprintln!("independent bilateral mark-fee coverage: {covered:?}");
        prop_assert_eq!(
            covered,
            expected,
            "bilateral mark-fee route corpus changed"
        );
        for discovery in discoveries {
            prop_assert!(!discovery.is_violation(), "{discovery:?}");
            prop_assert!(discovery.queued_mark >= discovery.setup_mark);
            prop_assert_eq!(discovery.coalition_excess, 0);
            if discovery.queued_mark == discovery.setup_mark {
                prop_assert_eq!(discovery.victim_loss, 0);
            }
            prop_assert!(discovery.extracted_tokens <= discovery.coalition_equity_before);
            prop_assert!(discovery.fee_counterparty_loss > 0);
            prop_assert!(discovery.insurance_gain > 0);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_045_trade_driven_liquidation_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_mark_mode_route_matrix_discovers_profitable_liquidation_moves(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_trade_driven_liquidation_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(
            discoveries.len(),
            TradeDrivenMarkMode::ALL.len() * ProspectiveAccrualRoute::ALL.len()
        );
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| (discovery.mode, discovery.route))
            .collect();
        let expected: Vec<_> = TradeDrivenMarkMode::ALL
            .into_iter()
            .flat_map(|mode| ProspectiveAccrualRoute::ALL.map(|route| (mode, route)))
            .collect();
        eprintln!("independent trade-driven liquidation discoveries: {violations:?}");
        prop_assert_eq!(
            violations,
            expected,
            "vulnerable-pin trade-driven liquidation corpus changed"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_045_mark_movement_reserve_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_trade_route_matrix_discovers_withdrawable_mark_reserve(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_mark_movement_reserve_violations(seed)
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
        eprintln!("independent mark-movement reserve discoveries: {violations:?}");
        prop_assert_eq!(
            violations,
            DiscoveryTradeRoute::ALL.to_vec(),
            "vulnerable-pin mark-movement reserve corpus changed"
        );
    }
}

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
                "proptest-regressions/inv_045_pending_mark_fee_ordering_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_pending_mark_fee_ordering_rejects_and_preserves_terminal_value(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_pending_mark_fee_ordering(seed)
            .map_err(TestCaseError::fail)?;
        eprintln!("pending-mark fee-order verification: {discovery:?}");
        prop_assert_eq!(discovery.control_reward, 0);
        prop_assert_eq!(discovery.reordered_reward, 0);
        prop_assert!(
            discovery.rejects_pending_sync_and_preserves_terminal_value(),
            "pending-mark fee ordering did not reject and preserve value: {:?}",
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
                "proptest-regressions/inv_045_pending_target_override_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_trade_route_matrix_discovers_pending_target_override(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_pending_target_override_violations(seed)
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
        eprintln!("independent pending-target override discoveries: {violations:?}");
        prop_assert_eq!(
            violations,
            DiscoveryTradeRoute::ALL.to_vec(),
            "vulnerable-pin pending-target override corpus changed"
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
    fn v16_program_pr356_pending_mark_fee_guard_fuzz(
        seed in pending_mark_fee_reward_seed_strategy()
    ) {
        let result = reproduce_pending_mark_fee_reward(seed);
        prop_assert!(
            result.is_ok(),
            "PR 356 fixed route failed for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr369_bilateral_fee_support_fuzz(
        (seed, mode, route) in bilateral_fee_support_strategy()
    ) {
        let reproduction = reproduce_bilateral_fee_support(seed, mode, route)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(reproduction.mode, mode);
        prop_assert_eq!(reproduction.route, route);
        prop_assert_eq!(reproduction.coalition_excess, 0);
        prop_assert!(reproduction.extracted_tokens <= reproduction.coalition_equity_before);
        prop_assert!(reproduction.queued_mark >= reproduction.setup_mark);
        if reproduction.queued_mark == reproduction.setup_mark {
            prop_assert_eq!(reproduction.victim_loss, 0);
        }
        prop_assert!(reproduction.fee_lp_loss > 0);
        prop_assert!(reproduction.insurance_gain > 0);
        prop_assert!(reproduction.max_cu < crate::support::v16_svm::TX_CU_LIMIT);
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
