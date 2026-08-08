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
//! all trade routes, public crank progress, deposits, token frames, lifecycle
//! and authority changes, account substitution rejection, normal exits,
//! liquidation, and CU ceilings.
//!
//! A separate bounded graph exhausts every action word through depth two over
//! eleven wrapper actions. It replays each edge from the same public genesis,
//! applies the independent state oracles after every transition, records exact
//! rollback as a self-edge, and distinguishes normalized economic states. This
//! is finite reachability evidence, not equivalence over unbounded sequences or
//! omitted payout/receipt state.

use super::*;
use crate::support::fuzz_model::run_bounded_reference_equivalence_graph;

#[test]
fn v16_program_bounded_reference_graph_exhausts_public_action_words() {
    let evidence = run_bounded_reference_equivalence_graph()
        .expect("INV-086 bounded deployed/reference graph");

    assert_eq!(
        evidence.word_count, 133,
        "must exhaust 11^0 + 11^1 + 11^2 words"
    );
    assert_eq!(
        evidence.transition_count, 253,
        "must replay every edge in every bounded word"
    );
    assert!(
        evidence.unique_node_count >= 50 && evidence.unique_edge_count >= 100,
        "bounded graph collapsed to vacuous state coverage: {evidence:?}"
    );
    assert!(
        evidence
            .action_attempts
            .iter()
            .all(|attempts| *attempts == 23),
        "every action must occupy every first/second word position: {evidence:?}"
    );
    assert!(
        evidence
            .action_state_changes
            .iter()
            .all(|changes| *changes != 0),
        "every action class must produce a real normalized state transition: {evidence:?}"
    );
    assert_ne!(
        evidence.coverage.loaded_program_hash, [0; 32],
        "bounded evidence must bind the production SBF artifact"
    );
    assert!(
        evidence.coverage.route_success.iter().sum::<u64>() != 0
            && evidence.coverage.token_frame_checks != 0
            && evidence.coverage.matcher_config_updates != 0
            && evidence.coverage.backing_topups != 0
            && evidence.coverage.authority_updates != 0
            && evidence.coverage.resolve_policy_updates != 0
            && evidence.coverage.lifecycle_updates != 0,
        "bounded graph must exercise value, trade, policy, authority, and lifecycle edges: {evidence:?}"
    );
}

#[test]
fn v16_program_rebalance_then_terminal_exit_preserves_position_attribution() {
    let scenario = Scenario {
        seed: [0x86; 32],
        config: SmallMarketConfig {
            maintenance_fee_per_slot: 1,
            ..SmallMarketConfig::default()
        },
        actions: vec![Action::RebalanceReduce { actor: 3, asset: 1 }],
    };

    let coverage = run_scenario(&scenario)
        .expect("unilateral reduction followed by public terminal exits must reconcile");
    assert_ne!(
        coverage.rebalance_reductions, 0,
        "setup must execute a real public unilateral reduction"
    );
    assert_ne!(
        coverage.user_positions_closed, 0,
        "the resulting asymmetric position set must still reach public terminal exits"
    );
}

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
