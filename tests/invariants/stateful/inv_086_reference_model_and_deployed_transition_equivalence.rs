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
//! A separate bounded graph exhausts every action word through depth three over
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
//! The ADL reduction-clamp matrix creates a scaled live leg through every trade transport, submits
//! owner reductions at `effective - 1`, `effective`, `effective + 1`, and retained raw basis, and
//! derives the permitted reduction from authenticated pre-state. Both side OI counters and the
//! leg's independently recomputed post-reduction effective quantity must match that clamp before
//! both terminal claimant orders return exactly the funded value. This covers accepted overshoot
//! as a liveness requirement: a stale or raw-basis work request is safely clamped, not rejected.
//! The paired Recovery matrix repeats those boundaries through delayed permissionless force-close,
//! crosses both public account orders, and requires the same pre-state clamp, two-sided OI delta,
//! no SPL movement, and exact terminal payout after shutdown.
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
        evidence.word_count, 2_380,
        "must exhaust 13^0 + 13^1 + 13^2 + 13^3 words"
    );
    assert_eq!(
        evidence.transition_count, 6_942,
        "must replay every edge in every bounded word"
    );
    assert!(
        evidence.unique_node_count >= 60 && evidence.unique_edge_count >= 140,
        "bounded graph collapsed to vacuous state coverage: {evidence:?}"
    );
    assert!(
        evidence.unique_node_count > evidence.depth_two_unique_node_count
            && evidence.unique_edge_count > evidence.depth_two_unique_edge_count,
        "third actions must discover normalized states and edges beyond depth two: {evidence:?}"
    );
    assert!(
        evidence
            .action_attempts
            .iter()
            .all(|attempts| *attempts == 534),
        "every action must occupy every first/second/third word position: {evidence:?}"
    );
    assert!(
        evidence
            .action_state_changes
            .iter()
            .all(|changes| *changes != 0),
        "every action class must produce a real normalized state transition: {evidence:?}"
    );
    assert!(
        evidence
            .third_position_state_changes
            .iter()
            .all(|changes| *changes != 0),
        "every action class must produce a real third-position state transition: {evidence:?}"
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
    assert_eq!(
        evidence.backing_rate_recovery_world_count, 1,
        "bounded graph must include a post-claim backing recovery edge"
    );
    assert!(
        evidence.claim_changing_edge_count != 0,
        "bounded public words and terminal schedules must traverse real exact-claim changes: {evidence:?}"
    );
    assert!(
        evidence.receipt_replacement_count >= evidence.partial_receipt_seed_count as u64,
        "every partial-receipt seed must replace an unreceipted claim bound exactly: {evidence:?}"
    );
    assert!(
        evidence.source_credit_formula_input_change_count != 0,
        "bounded public edges must exercise source-credit formula-input changes: {evidence:?}"
    );
    assert!(
        evidence.source_credit_rate_change_count != 0
            && evidence.source_credit_rate_increase_count != 0
            && evidence.source_credit_rate_decrease_count != 0,
        "bounded public edges must exercise both improving and degrading credit rates: {evidence:?}"
    );
    assert!(
        evidence.source_credit_backing_supported_increase_count != 0
            && evidence.source_credit_claim_reduction_increase_count != 0,
        "bounded rate improvements must cover both backing additions and claim reductions: {evidence:?}"
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

#[test]
fn v16_program_adl_reduction_clamp_matrix_matches_public_terminal_routes() {
    let discoveries =
        verify_adl_reduction_clamp_matrix([0x86; 32]).expect("INV-086 ADL reduction clamp matrix");
    assert_eq!(
        discoveries.len(),
        DiscoveryTradeRoute::ALL.len()
            * AdlReductionBoundary::ALL.len()
            * ResolvedAdlCloseOrder::ALL.len(),
        "must cross every trade route, reduction boundary, and terminal claimant order"
    );
    for discovery in discoveries {
        assert!(
            discovery.satisfies_invariant(),
            "ADL reduction clamp composition failed: {discovery:?}"
        );
        assert_eq!(
            discovery.request_overshot_effective,
            matches!(
                discovery.boundary,
                AdlReductionBoundary::AboveEffective | AdlReductionBoundary::RawBasis
            ),
            "only the two overshoot rows may exceed authenticated effective exposure: {discovery:?}"
        );
        assert_eq!(
            discovery.expected_effective_after_q,
            u128::from(matches!(
                discovery.boundary,
                AdlReductionBoundary::BelowEffective
            )),
            "only effective-minus-one must retain one live quantity atom: {discovery:?}"
        );
    }
}

#[test]
fn v16_program_adl_force_close_clamp_matrix_matches_recovery_terminal_routes() {
    let discoveries = verify_adl_force_close_clamp_matrix([0xfc; 32])
        .expect("INV-086 ADL force-close clamp matrix");
    assert_eq!(
        discoveries.len(),
        DiscoveryTradeRoute::ALL.len()
            * AdlReductionBoundary::ALL.len()
            * AdlForceCloseAccountOrder::ALL.len(),
        "must cross every opening route, force-close boundary, and account order"
    );
    for discovery in discoveries {
        assert!(
            discovery.satisfies_invariant(),
            "ADL force-close clamp composition failed: {discovery:?}"
        );
        assert_eq!(
            discovery.winner_effective_after_q,
            u128::from(matches!(
                discovery.boundary,
                AdlReductionBoundary::BelowEffective
            )),
            "only effective-minus-one must retain one live quantity atom: {discovery:?}"
        );
    }
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
