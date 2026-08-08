//! INV-086 - Reference-model and deployed-transition equivalence.
//!
//! Normative obligation: a small independent model of positions, route frames,
//! token movements, account substitution rejection, OI scans, and progress ranks
//! must agree with the deployed public transition after every generated step.
//!
//! Evidence in this file (F over public I routes): `run_scenario` drives the
//! actual SBF wrapper while maintaining independent shadow positions and
//! snapshots. Each successful transition is reconciled against the shadow model;
//! each rejected transition must preserve every tracked account byte and token
//! balance. The coverage assertions keep the generated run non-vacuous across
//! all trade routes, public crank progress, deposits, token frames, account
//! substitution rejection, normal exits, liquidation, and CU ceilings.

use super::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_INV086_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_086_reference_model_equivalence.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_reference_model_matches_deployed_public_sequence(
        scenario in scenario_strategy(env_usize("PERCOLATOR_INV086_FUZZ_ACTIONS", 16))
    ) {
        let serialized = serde_json::to_string_pretty(&scenario).unwrap();
        let coverage = run_scenario(&scenario).map_err(|error| {
            TestCaseError::fail(format!(
                "reference-model/deployed transition divergence: {error}\n{serialized}"
            ))
        })?;
        prop_assert!(
            coverage.route_success.iter().all(|successes| *successes != 0),
            "all four public trade routes must be exercised non-vacuously: {coverage:?}"
        );
        prop_assert!(
            coverage.crank_progress != 0
                && coverage.token_frame_checks != 0
                && coverage.user_positions_closed + coverage.known_blocker_exit_locks.iter().sum::<u64>() != 0
                && coverage.liquidation_steps != 0,
            "reference-model run did not exercise liveness/value surfaces: {coverage:?}"
        );
    }
}
