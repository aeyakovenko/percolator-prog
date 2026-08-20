//! INV-045 - No free mark movement.
//!
//! Normative obligation: Every mark movement remains elapsed-time bounded and economically paid across every trade route.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_mark_publication_matrix_rejects_stale_risk_and_recovers` publishes marks through
//! authenticated, EWMA, single-trade, and batch-trade routes. It proves the engine target is staged
//! immediately, stale risk increase rejects with exact rollback, and a post-catch-up round trip
//! remains live without value transfer. `v16_program_trade_route_matrix_rejects_pending_mark_inheritance`
//! signs exposure before a paid mark move and requires the retained request to reject on every
//! route, then proves the same intent shape can trade and exit after catch-up. The pending-target
//! override matrix independently requires a cheap round trip to reject without changing the
//! eventual target or terminal payouts.
//! `v16_program_pending_mark_fee_ordering_rejects_and_preserves_terminal_value` permutes fee
//! synchronization against mark commitment. It requires the pending-order attempt to reject with
//! exact rollback, then verifies the post-commit retry and terminal payouts equal the canonical
//! ordering. `v16_program_trade_route_matrix_keeps_mark_reserve_nonwithdrawable` creates a paid mark
//! move and proves withdrawal rejects with exact rollback both before and after commitment, while
//! the fee keeps the controlling coalition economically non-positive across all trade routes.
//! `v16_program_mark_mode_route_matrix_keeps_liquidation_penalties_nonreclaimable` crosses EWMA and
//! hybrid-after-hours modes with all single/batch CPI/no-CPI routes. It requires liquidation to
//! remain live while the resulting penalty stays out of cranker rewards and withdrawable domain
//! budgets and the controlling coalition remains economically negative.
//! `v16_program_matcher_route_matrix_rejects_one_sided_mark_subsidy` crosses the same modes with
//! single and batch CPI matcher exits and requires every mark-moving fee to be bilaterally funded;
//! it measures independent victim loss, fee-counterparty loss, insurance credit, and external
//! coalition profit. Direct impact tests remain below. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: these matrices are fixed-pin certification over generated seeds and public
//! LiteSVM routes; they are not exhaustive proofs over all full-width state combinations.

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
    fn v16_program_mark_mode_route_matrix_keeps_liquidation_penalties_nonreclaimable(
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
        eprintln!("independent trade-driven liquidation violations: {violations:?}");
        prop_assert!(violations.is_empty(), "{violations:?}");
        for discovery in discoveries {
            prop_assert!(discovery.certifies_nonreclaimable_liquidation_penalty());
            prop_assert_eq!(discovery.liquidation_reward, 0);
            prop_assert!(discovery.retained_penalty > 0);
            prop_assert_eq!(discovery.budgeted_penalty, 0);
            prop_assert!(discovery.oi_reduction_q > 0);
            prop_assert_eq!(discovery.coalition_gain, 0);
            prop_assert!(discovery.coalition_loss > 0);
        }
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
    fn v16_program_trade_route_matrix_keeps_mark_reserve_nonwithdrawable(
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
        eprintln!("independent mark-movement reserve violations: {violations:?}");
        prop_assert!(violations.is_empty(), "mark-movement reserve regressed");
        for discovery in discoveries {
            prop_assert!(
                discovery.certifies_nonwithdrawable_reserve(),
                "{discovery:?}"
            );
        }
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
    fn v16_program_mark_publication_matrix_rejects_stale_risk_and_recovers(
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
        eprintln!("independent pending-mark admission violations: {violations:?}");
        prop_assert!(violations.is_empty(), "pending-mark admission regressed");
        for discovery in discoveries {
            prop_assert!(discovery.certifies_guard_and_liveness(), "{discovery:?}");
        }
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
    fn v16_program_trade_route_matrix_rejects_pending_target_override(
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
        eprintln!("independent pending-target override violations: {violations:?}");
        prop_assert!(violations.is_empty(), "pending-target override regressed");
        for discovery in discoveries {
            prop_assert!(
                discovery.certifies_guard_and_terminal_value(),
                "{discovery:?}"
            );
        }
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
    fn v16_program_trade_route_matrix_rejects_pending_mark_inheritance(
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
        eprintln!("independent pending-mark inheritance violations: {violations:?}");
        prop_assert!(violations.is_empty(), "pending-mark inheritance regressed");
        for discovery in discoveries {
            prop_assert!(discovery.certifies_guard_and_liveness(), "{discovery:?}");
        }
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
    fn v16_program_pr260_pending_ewma_inheritance_guard_fuzz(
        (seed, route) in pending_ewma_inheritance_strategy()
    ) {
        let protection = reproduce_pending_ewma_inheritance(seed, route)
            .map_err(TestCaseError::fail)?;
        prop_assert!(protection.pending_admission_rejected);
        prop_assert!(protection.rejected_exact_rollback);
        prop_assert!(protection.post_commit_trade_landed);
        prop_assert!(protection.post_commit_exit_landed);
        prop_assert_eq!(protection.attacker_gain, 0);
        prop_assert_eq!(protection.victim_loss, 0);
    }

    #[test]
    fn v16_program_pr282_pending_ewma_target_override_guard_fuzz(
        (seed, route) in pending_ewma_target_override_strategy()
    ) {
        let protection = reproduce_pending_ewma_target_override(seed, route)
            .map_err(TestCaseError::fail)?;
        prop_assert!(protection.override_rejected);
        prop_assert!(protection.rejected_exact_rollback);
        prop_assert_eq!(protection.attack_target, protection.control_target);
        prop_assert_eq!(protection.attacker_profit, 0);
        prop_assert_eq!(protection.displaced_victim_pnl, 0);
    }

    #[test]
    fn v16_program_pr264_pr265_pr332_pr333_target_staging_guard_fuzz(
        (seed, case) in target_staging_strategy()
    ) {
        let protection = reproduce_unstaged_mark_target(seed, case)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(protection.engine_target, protection.wrapper_target);
        prop_assert!(protection.engine_epoch_advanced);
        prop_assert!(protection.stale_increase_rejected);
        prop_assert!(protection.rejected_exact_rollback);
        prop_assert!(protection.lagging_risk_reduction_landed);
        prop_assert!(protection.post_commit_trade_landed);
        prop_assert!(protection.post_commit_exit_landed);
        prop_assert_eq!(protection.attacker_profit, 0);
        prop_assert_eq!(protection.victim_capital_loss, 0);
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
    fn v16_program_pr225_nonwithdrawable_ewma_fee_fuzz(
        (seed, route) in reclaimable_ewma_fee_strategy()
    ) {
        let protection = reproduce_reclaimable_ewma_fee(seed, route)
            .map_err(TestCaseError::fail)?;
        prop_assert!(protection.pending_withdraw_rejected);
        prop_assert!(protection.rejected_exact_rollback);
        prop_assert!(protection.committed_withdraw_rejected);
        prop_assert!(protection.committed_rejected_exact_rollback);
        prop_assert_eq!(protection.fee_reclaimed, 0);
        prop_assert_eq!(protection.attacker_gain, 0);
        prop_assert!(protection.attacker_loss > 0);
        prop_assert!(protection.victim_loss <= protection.fee_paid);
        prop_assert!(protection.terminal_close_landed);
        prop_assert_eq!(protection.terminal_fee_burned, protection.fee_paid);
        prop_assert!(protection.close_cu < crate::support::v16_svm::TX_CU_LIMIT);
    }

    #[test]
    fn v16_program_pr280_trade_driven_liquidation_penalty_fuzz(
        (seed, mode, route) in trade_driven_liquidation_reward_strategy()
    ) {
        let protection = reproduce_trade_driven_liquidation_reward(seed, mode, route)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(protection.cranker_reward, 0);
        prop_assert!(protection.retained_penalty > 0);
        prop_assert_eq!(protection.budgeted_penalty, 0);
        prop_assert!(protection.victim_capital_loss > 0);
        prop_assert_eq!(protection.attacker_gain, 0);
        prop_assert!(protection.attacker_loss > 0);
        prop_assert!(protection.liquidation_landed);
        prop_assert!(protection.max_crank_cu < crate::support::v16_svm::TX_CU_LIMIT);
    }
}
