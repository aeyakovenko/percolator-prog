//! INV-002 - Asset generation binding.
//!
//! Normative obligation: Asset-scoped consent cannot cross retirement, slot reuse, or asset-generation changes.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_asset_generation_operation_matrix_discovers_stale_intents` enumerates a
//! finding-agnostic retained-operation registry over public retirement/reactivation. Direct impact
//! regressions remain below. Oracle controls use a retained `u64::MAX` sequence, proving the
//! generation property independently of the monotonic control-sequence layer.
//! `v16_program_asset_generation_terminal_policy_rejects_before_replacement_value_transfer`
//! retains an old resolve policy until replacement users have opened and accrued opposite PnL,
//! then requires generation-mismatch rejection and exact rollback. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;
use crate::support::v16_svm::PublicTerminalClassification;

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
        prop_assert!(violations.is_empty(), "every retained generation-scoped control must reject after slot reuse");
        prop_assert_eq!(protected, AssetIntentKind::ALL.to_vec());
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
    fn v16_program_asset_generation_terminal_policy_rejects_before_replacement_value_transfer(
        seed in any::<[u8; 32]>()
    ) {
        for kind in TerminalGenerationKind::ASSET {
            let discovery = discover_terminal_generation_replay(seed, kind)
                .map_err(TestCaseError::fail)?;
            prop_assert!(!discovery.is_violation());
            prop_assert!(discovery.stale_intent_rejected);
            prop_assert!(discovery.exact_rollback);
            prop_assert!(discovery.rejection_was_generation_mismatch);
            prop_assert!(discovery.fresh_intent_landed);
            prop_assert_eq!(
                discovery.terminal_classification,
                PublicTerminalClassification::Progressing
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
        let kind = match route {
            TradeRoute::NoCpi => AssetIntentKind::TradeNoCpi,
            TradeRoute::Cpi => AssetIntentKind::TradeCpi,
            TradeRoute::BatchNoCpi => AssetIntentKind::BatchTradeNoCpi,
            TradeRoute::BatchCpi => AssetIntentKind::BatchTradeCpi,
        };
        let protection = discover_asset_generation_replay(seed, kind)
            .map_err(TestCaseError::fail)?;
        prop_assert!(protection.new_asset_id > protection.old_asset_id);
        prop_assert!(!protection.accepted_stale_intent);
        prop_assert!(!protection.mutated_economic_state);
        prop_assert_eq!(protection.compute_units, None);
        prop_assert!(protection.rejection_was_generation_mismatch);
        prop_assert!(protection.fresh_intent_landed);
        prop_assert!(protection.fresh_intent_mutated_economic_state);
    }

    #[test]
    fn v16_program_pr279_insurance_top_up_generation_binding_fuzz(
        seed in collateral_top_up_generation_replay_seed_strategy()
    ) {
        let protection = discover_asset_generation_replay(seed, AssetIntentKind::InsuranceTopUp)
            .map_err(TestCaseError::fail)?;
        prop_assert!(protection.new_asset_id > protection.old_asset_id);
        prop_assert!(!protection.accepted_stale_intent);
        prop_assert!(!protection.mutated_economic_state);
        prop_assert_eq!(protection.compute_units, None);
        prop_assert!(protection.rejection_was_generation_mismatch);
        prop_assert!(protection.fresh_intent_landed);
        prop_assert!(protection.fresh_intent_mutated_economic_state);
    }

    #[test]
    fn v16_program_pr321_backing_top_up_generation_binding_fuzz(
        seed in backing_top_up_generation_replay_seed_strategy()
    ) {
        let protection = discover_asset_generation_replay(seed, AssetIntentKind::BackingTopUp)
            .map_err(TestCaseError::fail)?;
        prop_assert!(protection.new_asset_id > protection.old_asset_id);
        prop_assert!(!protection.accepted_stale_intent);
        prop_assert!(!protection.mutated_economic_state);
        prop_assert_eq!(protection.compute_units, None);
        prop_assert!(protection.rejection_was_generation_mismatch);
        prop_assert!(protection.fresh_intent_landed);
        prop_assert!(protection.fresh_intent_mutated_economic_state);
    }

    #[test]
    fn v16_program_pr328_insurance_withdrawal_generation_binding_fuzz(
        seed in insurance_withdrawal_generation_replay_seed_strategy()
    ) {
        let protection = discover_asset_generation_replay(seed, AssetIntentKind::InsuranceWithdrawal)
            .map_err(TestCaseError::fail)?;
        prop_assert!(protection.new_asset_id > protection.old_asset_id);
        prop_assert!(!protection.accepted_stale_intent);
        prop_assert!(!protection.mutated_economic_state);
        prop_assert_eq!(protection.compute_units, None);
        prop_assert!(protection.rejection_was_generation_mismatch);
        prop_assert!(protection.fresh_intent_landed);
        prop_assert!(protection.fresh_intent_mutated_economic_state);
    }

    #[test]
    fn v16_program_pr318_backing_fee_generation_binding_fuzz(
        seed in backing_fee_generation_replay_seed_strategy()
    ) {
        let protection = discover_asset_generation_replay(seed, AssetIntentKind::BackingFeePolicy)
            .map_err(TestCaseError::fail)?;
        prop_assert!(protection.new_asset_id > protection.old_asset_id);
        prop_assert!(!protection.accepted_stale_intent);
        prop_assert!(!protection.mutated_economic_state);
        prop_assert_eq!(protection.compute_units, None);
        prop_assert!(protection.rejection_was_generation_mismatch);
        prop_assert!(protection.fresh_intent_landed);
        prop_assert!(protection.fresh_intent_mutated_economic_state);
    }

    #[test]
    fn v16_program_pr311_pr312_marketwide_generation_binding_fuzz(
        seed in resolve_generation_replay_seed_strategy()
    ) {
        for kind in [AssetIntentKind::ResolveMarket, AssetIntentKind::ResolvePolicy] {
            let protection = discover_asset_generation_replay(seed, kind)
                .map_err(TestCaseError::fail)?;
            prop_assert!(protection.new_asset_id > protection.old_asset_id);
            prop_assert!(!protection.accepted_stale_intent);
            prop_assert!(!protection.mutated_economic_state);
            prop_assert_eq!(protection.compute_units, None);
            prop_assert!(protection.rejection_was_generation_mismatch);
            prop_assert!(protection.fresh_intent_landed);
            prop_assert!(protection.fresh_intent_mutated_economic_state);
        }
    }

    #[test]
    fn v16_program_pr277_pr322_asset_generation_config_binding_fuzz(
        (seed, path) in asset_generation_config_replay_strategy()
    ) {
        let kind = match path {
            AssetGenerationConfigPath::Auth => AssetIntentKind::ConfigureAuthMark,
            AssetGenerationConfigPath::Ewma => AssetIntentKind::ConfigureEwmaMark,
            AssetGenerationConfigPath::Hybrid => AssetIntentKind::ConfigureHybridOracle,
        };
        let protection = discover_asset_generation_replay(seed, kind)
            .map_err(TestCaseError::fail)?;
        prop_assert!(protection.new_asset_id > protection.old_asset_id);
        prop_assert!(!protection.accepted_stale_intent);
        prop_assert!(!protection.mutated_economic_state);
        prop_assert_eq!(protection.compute_units, None);
        prop_assert!(protection.rejection_was_generation_mismatch);
        prop_assert!(protection.fresh_intent_landed);
        prop_assert!(protection.fresh_intent_mutated_economic_state);
    }
}
