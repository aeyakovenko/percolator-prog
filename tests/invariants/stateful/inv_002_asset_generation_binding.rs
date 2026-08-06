//! INV-002 - Asset generation binding.
//!
//! Normative obligation: Asset-scoped consent cannot cross retirement, slot reuse, or asset-generation changes.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_asset_generation_operation_matrix_discovers_stale_intents` enumerates a
//! finding-agnostic retained-operation registry over public retirement/reactivation. Direct impact
//! regressions remain below.
//! `v16_program_asset_generation_terminal_policy_discovers_replacement_value_transfer` proves
//! economic impact by replaying an old asset-generation resolve policy only after replacement
//! users have opened and accrued opposite PnL. These tests exercise the deployed public
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
                "proptest-regressions/inv_002_asset_generation_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_asset_generation_operation_matrix_discovers_stale_intents(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_asset_generation_replays(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), AssetIntentKind::ALL.len());
        for (expected, discovery) in AssetIntentKind::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.kind, expected);
            prop_assert!(discovery.new_asset_id > discovery.old_asset_id);
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.kind)
            .collect();
        let protected: Vec<_> = discoveries
            .iter()
            .filter(|discovery| !discovery.is_violation())
            .map(|discovery| discovery.kind)
            .collect();
        eprintln!("independent INV-002 discoveries: {violations:?}");
        prop_assert_eq!(
            violations,
            vec![
                AssetIntentKind::TradeNoCpi,
                AssetIntentKind::TradeCpi,
                AssetIntentKind::BatchTradeNoCpi,
                AssetIntentKind::BatchTradeCpi,
                AssetIntentKind::ConfigureAuthMark,
                AssetIntentKind::ConfigureEwmaMark,
                AssetIntentKind::ConfigureHybridOracle,
                AssetIntentKind::InsuranceTopUp,
                AssetIntentKind::BackingTopUp,
                AssetIntentKind::InsuranceWithdrawal,
                AssetIntentKind::BackingFeePolicy,
                AssetIntentKind::ResolveMarket,
                AssetIntentKind::ResolvePolicy,
            ],
            "asset-generation discovery/protection split changed"
        );
        prop_assert_eq!(
            protected,
            vec![AssetIntentKind::PushAuthMark, AssetIntentKind::PushEwmaMark],
            "only generation-A mark pushes with a replacement mode configuration are sequence-protected"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_002_terminal_generation_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_asset_generation_terminal_policy_discovers_replacement_value_transfer(
        seed in any::<[u8; 32]>()
    ) {
        for kind in TerminalGenerationKind::ASSET {
            let discovery = discover_terminal_generation_replay(seed, kind)
                .map_err(TestCaseError::fail)?;
            prop_assert!(
                discovery.is_violation(),
                "old asset-generation terminal policy did not transfer replacement value: {:?}",
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
    fn v16_program_pr231_asset_generation_replay_fuzz(
        (seed, route) in asset_generation_replay_strategy()
    ) {
        let result = reproduce_asset_generation_trade_replay(seed, route);
        prop_assert!(
            result.is_ok(),
            "PR 231 {:?} no longer reproduces for seed {:?}: {}",
            route,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr279_collateral_top_up_generation_replay_fuzz(
        seed in collateral_top_up_generation_replay_seed_strategy()
    ) {
        let result = reproduce_collateral_top_up_generation_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 279 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr321_backing_top_up_generation_replay_fuzz(
        seed in backing_top_up_generation_replay_seed_strategy()
    ) {
        let result = reproduce_backing_top_up_generation_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 321 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr328_insurance_withdrawal_generation_replay_fuzz(
        seed in insurance_withdrawal_generation_replay_seed_strategy()
    ) {
        let result = reproduce_insurance_withdrawal_generation_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 328 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr318_backing_fee_generation_replay_fuzz(
        seed in backing_fee_generation_replay_seed_strategy()
    ) {
        let result = reproduce_backing_fee_generation_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 318 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr311_resolve_generation_replay_fuzz(
        seed in resolve_generation_replay_seed_strategy()
    ) {
        let result = reproduce_resolve_generation_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 311 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr277_pr322_asset_generation_config_replay_fuzz(
        (seed, path) in asset_generation_config_replay_strategy()
    ) {
        let result = reproduce_asset_generation_config_replay(seed, path);
        prop_assert!(
            result.is_ok(),
            "PR 277/322 {:?} no longer reproduces for seed {:?}: {}",
            path,
            seed,
            result.unwrap_err()
        );
    }
}
