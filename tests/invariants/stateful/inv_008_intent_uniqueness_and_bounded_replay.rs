//! INV-008 - Intent uniqueness and bounded replay.
//!
//! Normative obligation: One retained economic intent can execute at most once across routes and retries.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_retry_operation_matrix_rejects_every_stale_retry` generates signature-distinct
//! retries from one economic-operation registry without finding metadata. These tests exercise
//! the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//! A second generated probe duplicates a randomly selected retained family inside one atomic
//! transaction, requires bundle-wide rollback, then lands exactly one standalone request and
//! rejects the remaining duplicate without state or SPL-supply drift.
//! A third probe selects any ordered pair of single/batch CPI/no-CPI encodings for one retained
//! bilateral trade and requires the same rollback, exact-once, OI, basis, and supply properties.
//! The operation registry also retains two insurance-withdrawal requests before either lands,
//! executes one, replenishes the same stock through a separate public top-up, and requires the
//! stale request to reject before it can consume the newly available stock.
//!
//! Guarantee boundary: PRs 343/344/350/351/355/362 are fixed-pin certifications of the currently
//! deployed retained families, not a claim that absent message fields exist. Successful partial
//! fills have their own INV-009 matrix; retained expiry and aggregate signed budgets remain open
//! schema requirements.

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
    fn v16_program_retry_operation_matrix_rejects_every_stale_retry(
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
        let expected_violations = Vec::<RetryIntentKind>::new();
        prop_assert_eq!(
            &violations,
            &expected_violations,
            "exact-once discovery/protection corpus changed"
        );
        for discovery in &discoveries {
            prop_assert!(!discovery.accepted_retry);
            prop_assert!(!discovery.duplicated_economic_effect);
            prop_assert_eq!(discovery.retry_compute_units, None);
            prop_assert!(discovery.fresh_compute_units.is_some());
        }
        let rebalance = discoveries
            .iter()
            .find(|discovery| discovery.kind == RetryIntentKind::RebalanceReduce)
            .expect("rebalance retry discovery");
        prop_assert!(!rebalance.accepted_retry);
        prop_assert!(!rebalance.duplicated_economic_effect);
        prop_assert_eq!(rebalance.retry_compute_units, None);
        prop_assert!(rebalance.fresh_compute_units.is_some());
        let conversion = discoveries
            .iter()
            .find(|discovery| discovery.kind == RetryIntentKind::ConvertReleasedPnl)
            .expect("conversion retry discovery");
        prop_assert!(!conversion.accepted_retry);
        prop_assert!(!conversion.duplicated_economic_effect);
        prop_assert_eq!(conversion.retry_compute_units, None);
        prop_assert!(conversion.fresh_compute_units.is_some());
        let activation = discoveries
            .iter()
            .find(|discovery| discovery.kind == RetryIntentKind::AssetActivation)
            .expect("activation retry discovery");
        prop_assert!(!activation.accepted_retry);
        prop_assert!(!activation.duplicated_economic_effect);
        prop_assert_eq!(activation.retry_compute_units, None);
        prop_assert!(activation.fresh_compute_units.is_some());
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
        let kind = match route {
            TradeRoute::NoCpi => RetryIntentKind::TradeNoCpi,
            TradeRoute::Cpi => RetryIntentKind::TradeCpi,
            TradeRoute::BatchNoCpi => RetryIntentKind::BatchTradeNoCpi,
            TradeRoute::BatchCpi => RetryIntentKind::BatchTradeCpi,
        };
        let discovery = discover_intent_retry(seed, kind).map_err(TestCaseError::fail)?;
        prop_assert_eq!(discovery.kind, kind);
        prop_assert!(discovery.first_compute_units > 0);
        prop_assert!(!discovery.accepted_retry);
        prop_assert!(!discovery.duplicated_economic_effect);
        prop_assert_eq!(discovery.retry_compute_units, None);
        prop_assert!(discovery.fresh_compute_units.is_some());
    }

    #[test]
    fn v16_program_pr344_insurance_top_up_retry_rejection_fuzz(
        seed in insurance_top_up_retry_replay_seed_strategy()
    ) {
        let discovery = discover_intent_retry(seed, RetryIntentKind::InsuranceTopUp)
            .map_err(TestCaseError::fail)?;
        prop_assert!(!discovery.accepted_retry);
        prop_assert!(!discovery.duplicated_economic_effect);
        prop_assert_eq!(discovery.retry_compute_units, None);
        prop_assert!(discovery.fresh_compute_units.is_some());
    }

    #[test]
    fn v16_program_insurance_top_up_cross_route_retry_rejection_fuzz(
        seed in any::<[u8; 32]>(),
        direct_first in any::<bool>(),
    ) {
        let discovery = discover_cross_route_insurance_top_up_retry(seed, direct_first)
            .map_err(TestCaseError::fail)?;
        prop_assert!(!discovery.accepted_retry);
        prop_assert!(!discovery.duplicated_economic_effect);
        prop_assert_eq!(discovery.retry_compute_units, None);
        prop_assert!(discovery.fresh_compute_units.is_some());
    }

    #[test]
    fn v16_program_same_transaction_retry_atomicity_fuzz(
        seed in any::<[u8; 32]>(),
        kind_index in 0usize..RetryIntentKind::ALL.len(),
    ) {
        let kind = RetryIntentKind::ALL[kind_index];
        let discovery = discover_same_transaction_intent_retry(seed, kind)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discovery.kind, kind);
        prop_assert!(discovery.bundle_rejected);
        prop_assert!(discovery.bundle_exact_rollback);
        prop_assert!(discovery.standalone_mutated);
        prop_assert!(discovery.standalone_compute_units < 1_400_000);
        prop_assert!(discovery.duplicate_rejected);
        prop_assert!(discovery.duplicate_exact_rollback);
        prop_assert!(discovery.token_supply_conserved);
    }

    #[test]
    fn v16_program_cross_route_trade_retry_atomicity_fuzz(
        seed in any::<[u8; 32]>(),
        first_index in 0usize..DiscoveryTradeRoute::ALL.len(),
        duplicate_index in 0usize..DiscoveryTradeRoute::ALL.len(),
    ) {
        let first_route = DiscoveryTradeRoute::ALL[first_index];
        let duplicate_route = DiscoveryTradeRoute::ALL[duplicate_index];
        let discovery = discover_cross_route_trade_intent_retry(
            seed,
            first_route,
            duplicate_route,
        ).map_err(TestCaseError::fail)?;
        prop_assert_eq!(discovery.first_route, first_route);
        prop_assert_eq!(discovery.duplicate_route, duplicate_route);
        prop_assert!(discovery.bundle_rejected);
        prop_assert!(discovery.bundle_exact_rollback);
        prop_assert!(discovery.standalone_compute_units < 1_400_000);
        prop_assert!(discovery.duplicate_rejected);
        prop_assert!(discovery.duplicate_exact_rollback);
        prop_assert!(discovery.exact_bilateral_position);
        prop_assert!(discovery.exact_open_interest);
        prop_assert!(discovery.token_supply_conserved);
    }

    #[test]
    fn v16_program_pr362_activation_retry_rejection_fuzz(
        seed in activation_retry_replay_seed_strategy()
    ) {
        let discoveries = discover_intent_retries(seed).map_err(TestCaseError::fail)?;
        let activation = discoveries
            .iter()
            .find(|discovery| discovery.kind == RetryIntentKind::AssetActivation)
            .expect("activation retry discovery");
        prop_assert!(activation.first_compute_units > 0);
        prop_assert!(!activation.accepted_retry);
        prop_assert!(!activation.duplicated_economic_effect);
        prop_assert_eq!(activation.retry_compute_units, None);
    }

    #[test]
    fn v16_program_pr351_backing_top_up_retry_rejection_fuzz(
        seed in backing_top_up_retry_replay_seed_strategy()
    ) {
        let discovery = discover_intent_retry(seed, RetryIntentKind::BackingTopUp)
            .map_err(TestCaseError::fail)?;
        prop_assert!(!discovery.accepted_retry);
        prop_assert!(!discovery.duplicated_economic_effect);
        prop_assert_eq!(discovery.retry_compute_units, None);
        prop_assert!(discovery.fresh_compute_units.is_some());
    }

    #[test]
    fn v16_program_pr350_deposit_retry_replay_fuzz(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_intent_retry(seed, RetryIntentKind::Deposit)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discovery.kind, RetryIntentKind::Deposit);
        prop_assert!(discovery.first_compute_units > 0);
        prop_assert!(!discovery.accepted_retry);
        prop_assert!(!discovery.duplicated_economic_effect);
        prop_assert_eq!(discovery.retry_compute_units, None);
        prop_assert!(discovery.fresh_compute_units.is_some());
    }

    #[test]
    fn v16_program_pr355_withdrawal_retry_liquidation_fuzz(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_intent_retry(seed, RetryIntentKind::Withdraw)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discovery.kind, RetryIntentKind::Withdraw);
        prop_assert!(discovery.first_compute_units > 0);
        prop_assert!(!discovery.accepted_retry);
        prop_assert!(!discovery.duplicated_economic_effect);
        prop_assert_eq!(discovery.retry_compute_units, None);
        prop_assert!(discovery.fresh_compute_units.is_some());
    }

    #[test]
    fn v16_program_conversion_retry_protection_fuzz(seed in any::<[u8; 32]>()) {
        let result = verify_convert_retry_replay_protection(seed);
        prop_assert!(
            result.is_ok(),
            "conversion retry protection failed for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_retained_trade_retry_preserves_terminal_value(
        seed in any::<[u8; 32]>(),
        route_index in 0usize..4,
    ) {
        let kind = [
            RetryIntentKind::TradeNoCpi,
            RetryIntentKind::TradeCpi,
            RetryIntentKind::BatchTradeNoCpi,
            RetryIntentKind::BatchTradeCpi,
        ][route_index];
        let discovery = discover_trade_intent_retry_terminal(seed, kind)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discovery.kind, kind);
        prop_assert!(
            discovery.certifies_exact_once_and_bounded_exit(),
            "retained trade retry changed terminal value: {discovery:?}"
        );
    }

    #[test]
    fn v16_program_direct_debit_retries_are_atomic(
        seed in any::<[u8; 32]>(),
        kind_index in 0usize..2,
    ) {
        let kind = [
            RetryIntentKind::InsuranceTopUp,
            RetryIntentKind::AssetActivation,
        ][kind_index];
        let discovery = discover_debited_intent_retry(seed, kind)
            .map_err(TestCaseError::fail)?;
        prop_assert!(
            discovery.certifies_atomic_rejection(),
            "direct-debit retry was not atomic: {discovery:?}"
        );
    }
}
