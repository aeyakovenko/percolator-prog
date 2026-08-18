//! INV-013 - Destructive-consent scope.
//!
//! Normative obligation: returning to the same visible empty portfolio state after a later funded
//! episode must not revive an earlier ClosePortfolio authorization.
//!
//! Evidence in this file (F over public I routes): the generated test retains a close at sequence
//! `s`, deposits and withdraws an arbitrary nonzero amount in the same incarnation, and proves the
//! old close rejects with exact market, portfolio, SPL, and supply rollback. A close bound to the
//! advanced sequence must still succeed, excluding a blanket close DoS.

use crate::support::v16_svm::{MarketConfig, V16Svm};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: super::env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: super::env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_013_close_empty_state_aba.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_close_empty_state_aba_requires_fresh_sequence(
        seed in any::<[u8; 32]>(),
        amount in 1u64..=10_000_000u64,
    ) {
        const OWNER: usize = 0;
        let mut env = V16Svm::new(
            seed,
            MarketConfig {
                actor_deposits: [0, 1, 1, 1, 1],
                ..MarketConfig::default()
            },
        );
        let portfolio_id = env.primary_portfolio_id(OWNER);
        let sequence_before = env.primary_portfolio_matcher_sequence(OWNER);
        let retained_close = env.build_retained_close_primary_portfolio(OWNER);

        env.deposit_primary(OWNER, u128::from(amount))
            .map_err(TestCaseError::fail)?;
        let sequence_after = env.primary_portfolio_matcher_sequence(OWNER);
        prop_assert_eq!(sequence_after, sequence_before + 1);
        prop_assert_eq!(env.primary_portfolio_id(OWNER), portfolio_id);
        env.withdraw_primary(OWNER, u128::from(amount))
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(env.primary_portfolio(OWNER).capital.get(), 0);
        prop_assert_eq!(env.primary_portfolio(OWNER).pnl.get(), 0);

        let market_before = env.market_data(false);
        let portfolio_before = env.primary_portfolio_data(OWNER);
        let supply_before = env.token_supply_observed();
        let source_before = env.token_amount(env.actors[OWNER].source_token);
        let destination_before = env.token_amount(env.actors[OWNER].destination_token);
        let stale = env.land_retained(retained_close);
        prop_assert!(stale.is_err());
        prop_assert_eq!(env.market_data(false), market_before);
        prop_assert_eq!(env.primary_portfolio_data(OWNER), portfolio_before);
        prop_assert_eq!(env.token_supply_observed(), supply_before);
        prop_assert_eq!(env.token_amount(env.actors[OWNER].source_token), source_before);
        prop_assert_eq!(
            env.token_amount(env.actors[OWNER].destination_token),
            destination_before
        );

        let fresh = env.close_primary_portfolio(OWNER).map_err(TestCaseError::fail)?;
        prop_assert!(fresh.compute_units < 1_400_000);
    }
}
