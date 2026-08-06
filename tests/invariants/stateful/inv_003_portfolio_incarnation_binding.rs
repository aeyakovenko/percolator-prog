//! INV-003 - Portfolio incarnation binding.
//!
//! Normative obligation: Portfolio-scoped consent cannot cross close and same-pubkey recreation.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_portfolio_incarnation_operation_matrix_classifies_stale_intents` enumerates the
//! retained portfolio-operation registry without PR IDs or finding metadata. It requires the two
//! position-episode-bound routes to reject with exact rollback and preserves explicit counterexamples
//! for the nine routes that remain open. Finding-specific generated regressions remain below as impact
//! confirmation. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: the fixed roster certifies only rebalance and recovery-forfeit episode
//! binding. The open roster remains public counterexample evidence, not certification of INV-003.

use super::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_003_portfolio_incarnation_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_portfolio_incarnation_operation_matrix_classifies_stale_intents(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_portfolio_incarnation_replays(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), PortfolioIntentKind::ALL.len());
        for (expected, discovery) in PortfolioIntentKind::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.kind, expected);
            prop_assert!(discovery.new_portfolio_id > discovery.old_portfolio_id);
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.kind)
            .collect();
        eprintln!("independent INV-003 discoveries: {violations:?}");
        let expected_violations = vec![
            PortfolioIntentKind::Deposit,
            PortfolioIntentKind::Withdraw,
            PortfolioIntentKind::Close,
            PortfolioIntentKind::MatcherDisable,
            PortfolioIntentKind::TradeNoCpi,
            PortfolioIntentKind::TradeCpi,
            PortfolioIntentKind::BatchTradeNoCpi,
            PortfolioIntentKind::BatchTradeCpi,
            PortfolioIntentKind::ConvertReleasedPnl,
        ];
        prop_assert_eq!(
            violations,
            expected_violations,
            "INV-003 fixed/open roster changed; inspect every operation-class delta"
        );
        for fixed in [
            PortfolioIntentKind::RebalanceReduce,
            PortfolioIntentKind::ForfeitRecoveryLeg,
        ] {
            let discovery = discoveries
                .iter()
                .find(|discovery| discovery.kind == fixed)
                .expect("complete portfolio-intent registry");
            prop_assert!(!discovery.accepted_stale_intent);
            prop_assert!(!discovery.mutated_economic_state);
            prop_assert_eq!(discovery.compute_units, None);
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
    fn v16_program_pr309_portfolio_close_incarnation_replay_fuzz(
        seed in portfolio_close_incarnation_replay_seed_strategy()
    ) {
        let result = reproduce_portfolio_close_incarnation_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 309 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr304_matcher_grant_portfolio_incarnation_replay_fuzz(
        seed in matcher_grant_portfolio_incarnation_replay_seed_strategy()
    ) {
        let result = reproduce_matcher_grant_portfolio_incarnation_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 304 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr303_trade_portfolio_incarnation_replay_fuzz(
        (seed, route, side) in trade_portfolio_incarnation_replay_strategy()
    ) {
        let result = reproduce_trade_portfolio_incarnation_replay(seed, route, side);
        prop_assert!(
            result.is_ok(),
            "PR 303 {:?}/{:?} no longer reproduces for seed {:?}: {}",
            route,
            side,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr301_convert_portfolio_incarnation_replay_fuzz(
        seed in convert_portfolio_incarnation_replay_seed_strategy()
    ) {
        let result = reproduce_convert_portfolio_incarnation_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 301 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr278_forfeit_portfolio_incarnation_rejection_fuzz(
        seed in forfeit_portfolio_incarnation_replay_seed_strategy()
    ) {
        let result = reproduce_forfeit_portfolio_incarnation_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 278 fixed replay regression failed for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr299_portfolio_incarnation_withdrawal_fuzz(
        seed in portfolio_incarnation_withdrawal_seed_strategy()
    ) {
        let result = reproduce_portfolio_incarnation_withdrawal(seed);
        prop_assert!(
            result.is_ok(),
            "PR 299 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr305_portfolio_incarnation_deposit_fuzz(
        seed in portfolio_incarnation_deposit_seed_strategy()
    ) {
        let result = reproduce_portfolio_incarnation_deposit(seed);
        prop_assert!(
            result.is_ok(),
            "PR 305 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }
}
