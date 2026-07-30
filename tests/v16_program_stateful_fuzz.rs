mod support;

use proptest::prelude::*;
use support::fuzz_model::{run_scenario, scenario_strategy};

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
}
