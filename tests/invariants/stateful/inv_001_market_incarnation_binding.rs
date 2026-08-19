//! INV-001 - Market incarnation binding.
//!
//! Normative obligation: Retained requests cannot cross a market close, recreation, or generation change.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_market_incarnation_operation_matrix_discovers_stale_intents` enumerates a
//! finding-agnostic retained-operation registry over public market close/recreate. Direct impact
//! regressions remain below.
//! Secondary coverage: INV-014 for retained maintenance- and liquidation-fee policies whose
//! authorization must not survive a market-generation change.
//! `v16_program_market_generation_terminal_matrix_discovers_replacement_value_transfer` strengthens
//! terminal routes beyond acceptance: a retained old-generation resolve or resolve policy
//! crystallizes replacement-user PnL and transfers the exact victim loss to the winner. These
//! tests exercise the deployed public
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
                "proptest-regressions/inv_001_market_incarnation_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_market_incarnation_operation_matrix_discovers_stale_intents(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_market_incarnation_replays(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), MarketIntentKind::ALL.len());
        for (expected, discovery) in MarketIntentKind::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.kind, expected);
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.kind)
            .collect();
        eprintln!("independent INV-001 discoveries: {violations:?}");
        prop_assert_eq!(
            violations,
            MarketIntentKind::ALL.to_vec(),
            "vulnerable-pin market-incarnation discovery corpus changed"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_001_terminal_generation_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_market_generation_terminal_matrix_discovers_replacement_value_transfer(
        seed in any::<[u8; 32]>()
    ) {
        for kind in TerminalGenerationKind::MARKET {
            let discovery = discover_terminal_generation_replay(seed, kind)
                .map_err(TestCaseError::fail)?;
            prop_assert!(
                discovery.is_violation(),
                "old-generation terminal capability did not transfer replacement value: {:?}",
                discovery
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
                "proptest-regressions/v16_program_stateful_fuzz.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_pr294_matcher_grant_market_generation_replay_fuzz(
        seed in matcher_grant_market_generation_replay_seed_strategy()
    ) {
        let result = reproduce_matcher_grant_market_generation_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 294 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr296_trade_fee_market_generation_nonextraction_fuzz(
        seed in trade_fee_market_generation_replay_seed_strategy()
    ) {
        let protection = verify_trade_fee_market_generation_nonextraction(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(protection.stale_policy_landed);
        prop_assert!(protection.stale_trade_rejected);
        prop_assert!(protection.rejected_exact_rollback);
        prop_assert!(protection.recovery_trade_landed);
        prop_assert_eq!(protection.victim_loss, 0);
        prop_assert_eq!(protection.attacker_profit, 0);
        prop_assert_eq!(protection.extracted_fee, 0);
    }

    #[test]
    fn v16_program_pr295_forfeit_market_generation_replay_fuzz(
        seed in forfeit_market_generation_replay_seed_strategy()
    ) {
        let result = reproduce_forfeit_market_generation_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 295 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr317_fee_redirect_generation_replay_fuzz(
        seed in fee_redirect_generation_replay_seed_strategy()
    ) {
        let result = reproduce_fee_redirect_generation_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 317 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr307_market_incarnation_deposit_fuzz(
        seed in market_incarnation_deposit_seed_strategy()
    ) {
        let result = reproduce_market_incarnation_deposit(seed);
        prop_assert!(
            result.is_ok(),
            "PR 307 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr315_shutdown_generation_rejection_fuzz(
        seed in shutdown_generation_replay_seed_strategy()
    ) {
        let protection = discover_asset_generation_replay(
            seed,
            AssetIntentKind::LifecycleShutdown,
        ).map_err(TestCaseError::fail)?;
        prop_assert!(protection.new_asset_id > protection.old_asset_id);
        prop_assert!(!protection.accepted_stale_intent);
        prop_assert!(!protection.mutated_economic_state);
        prop_assert_eq!(protection.compute_units, None);
        prop_assert!(protection.rejection_was_generation_mismatch);
        prop_assert!(protection.fresh_intent_landed);
        prop_assert!(protection.fresh_intent_mutated_economic_state);
    }
}
