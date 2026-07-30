mod support;

use proptest::prelude::*;
use support::fuzz_model::{
    omitted_rescue_seed_strategy, post_expiry_backing_case_strategy,
    reproduce_omitted_rescue_liquidation, reproduce_post_expiry_backing_fee,
    reproduce_trade_retry_replay, run_scenario, scenario_strategy, trade_retry_replay_strategy,
};

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value != 0)
        .unwrap_or(default)
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
    fn v16_program_stateful_public_interface_fuzz(
        scenario in scenario_strategy(env_usize("PERCOLATOR_FUZZ_ACTIONS", 12))
    ) {
        let serialized = serde_json::to_string_pretty(&scenario).unwrap();
        let result = run_scenario(&scenario);
        prop_assert!(result.is_ok(), "stateful public-interface scenario failed: {}\n{}",
            result.unwrap_err(), serialized);
    }

    #[test]
    fn v16_program_pr367_post_expiry_backing_fee_fuzz(
        (seed, case) in post_expiry_backing_case_strategy()
    ) {
        let result = reproduce_post_expiry_backing_fee(seed, case);
        prop_assert!(
            result.is_ok(),
            "PR 367 no longer reproduces for case {:?}: {}",
            case,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr220_omitted_rescue_liquidation_fuzz(
        seed in omitted_rescue_seed_strategy()
    ) {
        let result = reproduce_omitted_rescue_liquidation(seed);
        prop_assert!(
            result.is_ok(),
            "PR 220 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr343_trade_retry_replay_fuzz(
        (seed, route) in trade_retry_replay_strategy()
    ) {
        let result = reproduce_trade_retry_replay(seed, route);
        prop_assert!(
            result.is_ok(),
            "PR 343 {:?} no longer reproduces for seed {:?}: {}",
            route,
            seed,
            result.unwrap_err()
        );
    }
}
