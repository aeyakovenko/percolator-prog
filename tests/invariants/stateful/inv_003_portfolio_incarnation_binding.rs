//! INV-003 - Portfolio incarnation binding.
//!
//! Normative obligation: Portfolio-scoped consent cannot cross close and same-pubkey recreation.
//!
//! Evidence in this file (F over public I routes): `v16_program_pr309_portfolio_close_incarnation_replay_fuzz`, `v16_program_pr304_matcher_grant_portfolio_incarnation_replay_fuzz`, `v16_program_pr303_trade_portfolio_incarnation_replay_fuzz`, `v16_program_pr301_convert_portfolio_incarnation_replay_fuzz`, `v16_program_pr278_forfeit_portfolio_incarnation_replay_fuzz`, `v16_program_pr299_portfolio_incarnation_withdrawal_fuzz`, `v16_program_pr305_portfolio_incarnation_deposit_fuzz`. These tests exercise the deployed public
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
    fn v16_program_pr278_forfeit_portfolio_incarnation_replay_fuzz(
        seed in forfeit_portfolio_incarnation_replay_seed_strategy()
    ) {
        let result = reproduce_forfeit_portfolio_incarnation_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 278 no longer reproduces for seed {:?}: {}",
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
