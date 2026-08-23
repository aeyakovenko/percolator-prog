//! INV-086 - Reference-model and deployed-transition equivalence.
//!
//! Normative obligation: a small independent model of positions, route frames,
//! token movements, account substitution rejection, OI scans, and progress ranks
//! must agree with the deployed public transition after every generated step.
//!
//! Evidence in this file (F over public I routes): `run_scenario` drives the
//! actual SBF wrapper while maintaining independent shadow positions, pooled
//! effective OI, and snapshots. The OI ledger derives account-side deltas from
//! pre/post public leg transitions and applies the separate matched-side decrement
//! for liquidation and owner rebalance. It therefore does not confuse an
//! ADL-reduced leg's intentionally retained raw basis with its smaller effective
//! OI. Each successful transition is reconciled against the shadow model; each
//! rejected transition must preserve every tracked account byte and token balance.
//! The retained corpus includes a public ADL/rebalance sequence that first exposed
//! the old raw-basis oracle's false equality assumption. The coverage assertions
//! keep the generated run non-vacuous across
//! all trade routes, public crank progress, deposits, token frames, lifecycle
//! and authority changes, account substitution rejection, normal exits,
//! liquidation, and CU ceilings.
//!
//! A separate bounded graph exhausts every action word through depth two over
//! thirteen wrapper actions, including authority resolution and resolved close. Its
//! normalized node includes every portfolio's PnL, escrow, close ledger, payout receipt,
//! and account status plus the market payout snapshot, per-domain source credit,
//! backing buckets, and insurance reservations. It replays each edge from the same
//! public genesis, applies the independent state oracles after every transition,
//! records exact rollback as a self-edge, and distinguishes normalized economic states.
//! A second terminal subgraph starts from twelve independently replayed public prefixes
//! that create a genuinely partial underfunded receipt. It crosses all expiry boundaries,
//! two claimant orders, and both close/claim priorities; claim-priority paths must move
//! real SPL value and every terminal edge remains subject to the same reference oracles.
//! The fifth portfolio carries 777 atoms of unrelated flat principal and must receive all of it
//! before claim-snapshot capture in every world, making the same graph an INV-074 receipt-locality
//! witness rather than merely a terminal accounting exercise.
//! A separate public prefix composes the adjacent frontier directly: a flat bankrupt account has
//! an active close with nonzero residual while three source-claim domains remain live;
//! `ResolveMarket` must frame that close exactly, resolved continuations must finalize the same
//! `close_id`, and only then may the claimant receive a genuinely partial payout receipt. The
//! same public state must then produce a value-moving top-up and converge every portfolio to its
//! economic terminal predicate.
//! This is finite reachability evidence, not equivalence over unbounded sequences.

use super::*;
use crate::support::fuzz_model::{
    run_bounded_reference_equivalence_graph, verify_close_to_partial_receipt_composition,
};

#[test]
fn active_close_composes_through_resolution_into_partial_receipt() {
    let evidence = verify_close_to_partial_receipt_composition()
        .expect("INV-086 close-to-partial-receipt composition");

    assert_ne!(
        evidence.active_close_residual, 0,
        "the public prefix must carry value through the active close"
    );
    assert!(
        evidence.source_claim_domain_count >= 3
            && evidence.resolve_framed_close
            && evidence.resolved_close_finalized,
        "the close and independent claim domains must survive and compose: {evidence:?}"
    );
    assert!(
        evidence.partial_receipt_face != 0
            && evidence.partial_receipt_paid < evidence.partial_receipt_face,
        "the terminal bridge must end at a nonvacuous partial receipt: {evidence:?}"
    );
    assert!(
        evidence.post_receipt_payout != 0 && evidence.terminal_actor_count == 5,
        "the same bridge must pay and terminate every funded portfolio: {evidence:?}"
    );
}

#[test]
fn v16_program_bounded_reference_graph_exhausts_public_action_words() {
    let evidence = run_bounded_reference_equivalence_graph()
        .expect("INV-086 bounded deployed/reference graph");

    assert_eq!(
        evidence.word_count, 183,
        "must exhaust 13^0 + 13^1 + 13^2 words"
    );
    assert_eq!(
        evidence.transition_count, 351,
        "must replay every edge in every bounded word"
    );
    assert!(
        evidence.unique_node_count >= 60 && evidence.unique_edge_count >= 140,
        "bounded graph collapsed to vacuous state coverage: {evidence:?}"
    );
    assert!(
        evidence
            .action_attempts
            .iter()
            .all(|attempts| *attempts == 27),
        "every action must occupy every first/second word position: {evidence:?}"
    );
    assert!(
        evidence
            .action_state_changes
            .iter()
            .all(|changes| *changes != 0),
        "every action class must produce a real normalized state transition: {evidence:?}"
    );
    assert_eq!(
        evidence.underfunded_terminal_world_count, 12,
        "must replay 3 expiry boundaries x 2 claimant orders x 2 route priorities"
    );
    assert!(
        evidence.underfunded_terminal_transition_count
            >= evidence.underfunded_terminal_world_count * 2
            && evidence.underfunded_terminal_unique_node_count >= 12
            && evidence.underfunded_terminal_unique_edge_count >= 12,
        "underfunded terminal graph collapsed to vacuous state coverage: {evidence:?}"
    );
    assert_eq!(
        evidence.partial_receipt_seed_count, 12,
        "every terminal world must start from a genuine partial receipt"
    );
    assert_eq!(
        evidence.value_moving_claim_world_count, 6,
        "every claim-priority world must execute a value-moving payout top-up"
    );
    assert_eq!(
        evidence.expiry_normalization_world_count, 8,
        "exact- and post-expiry worlds must normalize backing on a public edge"
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
            && evidence.coverage.lifecycle_updates != 0
            && evidence.coverage.terminal_resolves != 0
            && evidence.coverage.resolved_close_mutations != 0
            && evidence.coverage.resolved_claim_mutations != 0
            && evidence.coverage.resolved_payout_atoms != 0,
        "bounded graph must exercise value, trade, policy, authority, lifecycle, and terminal edges: {evidence:?}"
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
