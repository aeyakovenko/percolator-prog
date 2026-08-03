//! INV-001 - Market incarnation binding.
//!
//! Normative obligation: Retained requests cannot cross a market close, recreation, or generation change.
//!
//! Evidence in this file (F over public I routes): `v16_program_pr294_matcher_grant_market_generation_replay_fuzz`, `v16_program_pr296_trade_fee_market_generation_replay_fuzz`, `v16_program_pr295_forfeit_market_generation_replay_fuzz`, `v16_program_pr317_fee_redirect_generation_replay_fuzz`, `v16_program_pr307_market_incarnation_deposit_fuzz`, `v16_program_pr315_shutdown_generation_replay_fuzz`. These tests exercise the deployed public
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
    fn v16_program_pr296_trade_fee_market_generation_replay_fuzz(
        seed in trade_fee_market_generation_replay_seed_strategy()
    ) {
        let result = reproduce_trade_fee_market_generation_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 296 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
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
    fn v16_program_pr315_shutdown_generation_replay_fuzz(
        seed in shutdown_generation_replay_seed_strategy()
    ) {
        let result = reproduce_shutdown_generation_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 315 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }
}
