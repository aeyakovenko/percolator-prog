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
//! thirteen wrapper actions, then extends every exact authenticated tracked depth-three wrapper state
//! with all thirteen actions. The key combines byte-identical tracked account/balance state with
//! all authenticated Clock fields. This partial-order-reduced depth-four frontier includes authority
//! resolution and resolved close. Its
//! normalized node includes every portfolio's PnL, escrow, close ledger, payout receipt,
//! and account status plus the market payout snapshot, per-domain source credit,
//! backing buckets, and insurance reservations. It replays each edge from the same
//! public genesis, applies the independent state oracles after every transition,
//! records exact rollback as a self-edge, and distinguishes normalized economic states.
//! A Recovery-seeded frontier starts from independently rebuilt public positions with nonflat
//! mark/funding state, funded backing and insurance, and either fresh or exact-expiry backing.
//! It exhausts every one- and two-action word over Recovery crank, owner forfeit, abandoned-pair
//! force-close, owner deposit, backing, insurance, resolve, and resolved-close routes, plus exact
//! rollback controls for live-mode rebalance. Every reached state must retain a bounded
//! value-moving owner exit under the same state, stock, custody, and rollback oracles; this is the
//! lifecycle-prefix extension of the base Live-state graph.
//! A second seeded frontier starts from two independently rebuilt public bankruptcy schedules, one
//! for each side. An unrelated live cohort receives a real side-local B loss and retains an exact
//! `target_b > b_snap` continuation after the higher-priority close completes. Every empty,
//! one-action, and ordered two-action word crosses complete/empty-hint B cranks, unrelated cranks,
//! owner deposit/withdraw/reduction, matcher disable, mark movement, shutdown, and permissionless
//! resolution.
//! The graph counts actual B-rank reductions independently from generic state changes and requires
//! every reached state to retain a bounded value-moving owner exit.
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
//! `permissionless_liquidation_composes_into_partial_receipt_across_all_trade_routes` strengthens
//! that bridge by leaving the deeply adverse position open: only the public automatic crank may
//! remove its matched OI and create the value-bearing close. Resolution must frame and finish that
//! same close before the underfunded winner receives a genuine partial receipt, a later
//! value-moving top-up, and a terminal exit. This composes liquidation quantity/OI accounting with
//! close and receipt identity rather than testing those transitions in separate worlds.
//! A paired four-route matrix funds 123 insurance atoms in the close's exact source domain before
//! liquidation. The close must spend those atoms once, book only the remaining loss to B, and carry
//! the resulting historical spend through resolution, a partial receipt, a later payout, and all
//! five terminal portfolios with exact engine/SPL custody. The domain is derived through the same
//! canonical asset-side mapper and checked against the close ledger rather than hard-coded.
//! After all claims terminate, the exact 750+1 fresh backing remainder returns through the two
//! canonical provider withdrawals, all portfolios dematerialize, and `CloseSlab` reaches the
//! closed-market tombstone without burning provider value.
//! A paired exact-expiry matrix withholds the 750-atom backing withdrawal until its authenticated
//! expiry slot. The terminal transition must expire that backing, restore exactly the 123 atoms of
//! historically spent insurance that remain provider-backed, permit that restored insurance to be
//! withdrawn, classify the remaining 627 atoms as claim-free terminal surplus, and close the slab.
//! The 1-atom unexpired backing domain remains independently provider-withdrawable throughout.
//! The ADL reduction-clamp matrix creates a scaled live leg through every trade transport, submits
//! owner reductions at `effective - 1`, `effective`, `effective + 1`, and retained raw basis, and
//! derives the permitted reduction from authenticated pre-state. Both side OI counters and the
//! leg's independently recomputed post-reduction effective quantity must match that clamp before
//! both terminal claimant orders return exactly the funded value. This covers accepted overshoot
//! as a liveness requirement: a stale or raw-basis work request is safely clamped, not rejected.
//! The paired Recovery matrix repeats those boundaries through delayed permissionless force-close,
//! crosses both public account orders, and requires the same pre-state clamp, two-sided OI delta,
//! no SPL movement, and exact terminal payout after shutdown.
//! A finding-blind dual-ADL prefix then uses only public trades, mark updates, maintenance sync, and
//! liquidation to make both side A indices non-unit while both retained raw legs exceed canonical
//! effective OI. An independent implementation of the liquidation maintenance, fee, floor, and
//! binary-search equations consumes the authenticated pre-liquidation certificate and predicts the
//! exact selector-sized close. All four opening transports must apply precisely that amount to both
//! OI sides and to the scaled leg without moving SPL tokens or exceeding the CU ceiling. The
//! Recovery-forfeit composition then crosses both owner landing orders and one/max B work budgets.
//! Each owner must retire exactly its independently reconstructed effective quantity from only its
//! own OI side; the first exit becomes one zero-basis obligation, the second detaches, and bounded
//! permissionless work clears the obligation before each owner exits while the market remains Live.
//! The pre-exit value partitions exactly between the configured maintenance fee and SPL payout;
//! one/max B budgets and both owner orders converge to the same terminal economics. This
//! distinguishes a B settlement budget from a caller-selected position quantity. The corresponding
//! 32-world Recovery matrix crosses every opening transport, request boundary, and
//! account order. Stale overshoot and raw-basis requests are work budgets: they must land the
//! independently derived effective quantity instead of rejecting because the caller observed an
//! earlier state.
//! This is finite reachability evidence, not equivalence over unbounded sequences.

use super::*;
use crate::support::fuzz_model::{
    run_bounded_b_reference_frontier, run_bounded_recovery_reference_frontier,
    run_bounded_reference_equivalence_graph, verify_close_to_partial_receipt_composition,
    verify_expired_backing_terminal_cleanup_compositions,
    verify_insurance_liquidation_to_partial_receipt_compositions,
    verify_liquidation_to_partial_receipt_compositions,
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
fn permissionless_liquidation_composes_into_partial_receipt_across_all_trade_routes() {
    let discoveries = verify_liquidation_to_partial_receipt_compositions()
        .expect("INV-086 liquidation-to-partial-receipt composition");
    assert_eq!(discoveries.len(), 4, "every public trade route must run");
    assert_eq!(
        discoveries
            .iter()
            .map(|evidence| evidence.route)
            .collect::<Vec<_>>(),
        vec![
            TradeRoute::NoCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchNoCpi,
            TradeRoute::BatchCpi,
        ],
        "the route matrix must not duplicate a transport"
    );

    let mut liquidation_compute = Vec::new();
    for evidence in discoveries {
        assert_eq!(
            evidence.selected_route_trade_count, 5,
            "every configured public trade in the prefix must use the selected route: {evidence:?}"
        );
        assert_eq!(
            (
                evidence.pre_liquidation_effective_oi,
                evidence.post_liquidation_effective_oi,
                evidence.liquidation_steps,
                evidence.liquidated_abs_q,
            ),
            (70_000_000, 0, 1, 70_000_000),
            "the crank must remove the complete independently configured matched exposure: {evidence:?}"
        );
        assert!(
            evidence.liquidation_created_close && evidence.terminal.close_gross_loss == 2_723,
            "the public crank must perform real liquidation and create the close: {evidence:?}"
        );
        assert!(
            evidence.liquidation_compute_units != 0
                && evidence.liquidation_compute_units <= 320_000,
            "the liquidation call must retain bounded CU headroom: {evidence:?}"
        );
        liquidation_compute.push(evidence.liquidation_compute_units);
        assert!(
            evidence.terminal.resolve_framed_close
                && evidence.terminal.resolved_close_finalized
                && evidence.terminal.source_claim_domain_count == 3
                && evidence.terminal.partial_receipt_face == 1_000
                && evidence.terminal.partial_receipt_paid == 125
                && evidence.terminal.post_receipt_payout == 126
                && !evidence.terminal.terminal_receipt_present
                && evidence.terminal.terminal_receipt_paid == 0
                && !evidence.terminal.terminal_receipt_finalized
                && evidence.terminal.final_engine_vault == 750
                && evidence.terminal.final_spl_vault == 750
                && evidence.terminal.max_compute_units != 0
                && evidence.terminal.max_compute_units < 1_400_000
                && evidence.terminal.terminal_actor_count == 5,
            "liquidation must compose through a genuine partial receipt and terminal exit: {evidence:?}"
        );
    }
    assert!(
        liquidation_compute
            .windows(2)
            .all(|pair| pair[0] == pair[1]),
        "opening transport changed liquidation CU: {liquidation_compute:?}"
    );
}

#[test]
fn insurance_spend_composes_through_liquidation_partial_receipt_and_terminal_payout() {
    let discoveries = verify_insurance_liquidation_to_partial_receipt_compositions()
        .expect("INV-086 insurance liquidation-to-partial-receipt composition");
    assert_eq!(discoveries.len(), 4, "every public trade route must run");
    assert_eq!(
        discoveries
            .iter()
            .map(|evidence| evidence.route)
            .collect::<Vec<_>>(),
        vec![
            TradeRoute::NoCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchNoCpi,
            TradeRoute::BatchCpi,
        ],
        "the insurance matrix must not duplicate an opening transport"
    );

    let normalized = discoveries
        .iter()
        .map(|evidence| {
            assert_eq!(
                (evidence.insurance_funded, evidence.insurance_spent),
                (123, 123),
                "the close must consume the exact public domain top-up: {evidence:?}"
            );
            assert_eq!(
                evidence.terminal.close_gross_loss,
                evidence
                    .insurance_spent
                    .checked_add(evidence.b_loss_booked)
                    .expect("bounded close partition sum"),
                "insurance and B must partition this support-free close exactly: {evidence:?}"
            );
            assert_eq!(
                evidence.terminal.terminal_cleanup_slot,
                Some(12),
                "the pre-expiry control must land one slot before backing expiry"
            );
            assert_eq!(
                (
                    evidence.pre_liquidation_effective_oi,
                    evidence.post_liquidation_effective_oi,
                    evidence.liquidation_steps,
                    evidence.liquidated_abs_q,
                    evidence.b_loss_booked,
                    evidence.aggregate_insurance_after_liquidation,
                    evidence.terminal.partial_receipt_face,
                    evidence.terminal.partial_receipt_paid,
                    evidence.terminal.post_receipt_payout,
                    evidence.terminal.final_engine_vault,
                    evidence.terminal.final_spl_vault,
                    evidence.terminal.terminal_actor_count,
                ),
                (
                    70_000_000,
                    0,
                    1,
                    70_000_000,
                    2_600,
                    0,
                    1_125,
                    198,
                    176,
                    751,
                    751,
                    5,
                ),
                "the insurance-bearing close must reach its exact partial receipt, later payout, and terminal custody: {evidence:?}"
            );
            assert!(
                evidence.liquidation_compute_units <= 340_000
                    && evidence.terminal.max_compute_units < 1_400_000,
                "the insurance-bearing terminal graph must retain transaction headroom: {evidence:?}"
            );
            assert_eq!(
                (
                    evidence.terminal.terminal_portfolios_closed,
                    evidence.terminal.terminal_backing_withdrawn,
                    evidence.terminal.terminal_backing_expired,
                    evidence.terminal.terminal_insurance_withdrawn,
                    evidence.terminal.slab_progress_steps,
                    evidence.terminal.slab_custody_burned,
                    evidence.terminal.slab_closed,
                ),
                (5, 751, 0, 0, 1, 0, true),
                "the cursor must park before live backing, roll back while blocked, then close after provider withdrawal: {evidence:?}"
            );
            assert!(
                evidence.terminal.slab_close_compute_units != 0
                    && evidence.terminal.slab_close_compute_units < 1_400_000,
                "CloseSlab must execute with bounded compute: {evidence:?}"
            );
            [
                evidence.pre_liquidation_effective_oi,
                evidence.post_liquidation_effective_oi,
                u128::from(evidence.liquidation_steps),
                evidence.liquidated_abs_q,
                evidence.insurance_funded,
                evidence.insurance_spent,
                evidence.b_loss_booked,
                evidence.aggregate_insurance_after_liquidation,
                evidence.terminal.partial_receipt_face,
                evidence.terminal.partial_receipt_paid,
                evidence.terminal.post_receipt_payout,
                evidence.terminal.final_engine_vault,
                evidence.terminal.final_spl_vault,
                evidence.terminal.terminal_actor_count as u128,
                evidence.terminal.terminal_portfolios_closed as u128,
                evidence.terminal.terminal_backing_withdrawn,
                evidence.terminal.terminal_backing_expired,
                evidence.terminal.terminal_insurance_withdrawn,
                evidence.terminal.slab_progress_steps as u128,
                evidence.terminal.slab_custody_burned,
                u128::from(evidence.terminal.slab_closed),
            ]
        })
        .collect::<Vec<_>>();
    assert!(
        normalized.windows(2).all(|pair| pair[0] == pair[1]),
        "opening transport changed insurance attribution or terminal economics: {discoveries:?}"
    );
}

#[test]
fn expired_backing_composes_through_insurance_recredit_and_terminal_slab_cleanup() {
    let discoveries = verify_expired_backing_terminal_cleanup_compositions()
        .expect("INV-063/070/086 exact-expiry terminal cleanup composition");
    assert_eq!(
        discoveries.len(),
        8,
        "every public trade route must run at exact and late expiry"
    );
    assert_eq!(
        discoveries
            .iter()
            .map(|evidence| (evidence.terminal.terminal_cleanup_slot, evidence.route))
            .collect::<Vec<_>>(),
        vec![
            (Some(13), TradeRoute::NoCpi),
            (Some(13), TradeRoute::Cpi),
            (Some(13), TradeRoute::BatchNoCpi),
            (Some(13), TradeRoute::BatchCpi),
            (Some(14), TradeRoute::NoCpi),
            (Some(14), TradeRoute::Cpi),
            (Some(14), TradeRoute::BatchNoCpi),
            (Some(14), TradeRoute::BatchCpi),
        ],
        "the terminal-expiry matrix must not duplicate or omit a landing/transport cell"
    );

    let normalized = discoveries
        .iter()
        .map(|evidence| {
            assert_eq!(
                (
                    evidence.insurance_funded,
                    evidence.insurance_spent,
                    evidence.b_loss_booked,
                    evidence.terminal.final_engine_vault,
                    evidence.terminal.final_spl_vault,
                ),
                (123, 123, 2_600, 751, 751),
                "the public liquidation prefix must retain the exact terminal accounting world: {evidence:?}"
            );
            assert_eq!(
                (
                    evidence.terminal.terminal_portfolios_closed,
                    evidence.terminal.terminal_backing_withdrawn,
                    evidence.terminal.terminal_backing_expired,
                    evidence.terminal.terminal_insurance_withdrawn,
                    evidence.terminal.slab_progress_steps,
                    evidence.terminal.slab_custody_burned,
                    evidence.terminal.slab_closed,
                ),
                (5, 1, 750, 123, 2, 627, true),
                "expiry, insurance restoration, and terminal surplus must partition custody exactly: {evidence:?}"
            );
            assert!(
                evidence.liquidation_compute_units <= 340_000
                    && evidence.terminal.max_compute_units < 1_400_000
                    && evidence.terminal.slab_close_compute_units != 0
                    && evidence.terminal.slab_close_compute_units < 1_400_000,
                "every exact-expiry continuation must retain transaction headroom: {evidence:?}"
            );
            [
                evidence.pre_liquidation_effective_oi,
                evidence.post_liquidation_effective_oi,
                u128::from(evidence.liquidation_steps),
                evidence.liquidated_abs_q,
                evidence.insurance_funded,
                evidence.insurance_spent,
                evidence.b_loss_booked,
                evidence.aggregate_insurance_after_liquidation,
                evidence.terminal.partial_receipt_face,
                evidence.terminal.partial_receipt_paid,
                evidence.terminal.post_receipt_payout,
                evidence.terminal.final_engine_vault,
                evidence.terminal.final_spl_vault,
                evidence.terminal.terminal_portfolios_closed as u128,
                evidence.terminal.terminal_backing_withdrawn,
                evidence.terminal.terminal_backing_expired,
                evidence.terminal.terminal_insurance_withdrawn,
                evidence.terminal.slab_progress_steps as u128,
                evidence.terminal.slab_custody_burned,
                u128::from(evidence.terminal.slab_closed),
            ]
        })
        .collect::<Vec<_>>();
    assert!(
        normalized.windows(2).all(|pair| pair[0] == pair[1]),
        "opening transport or exact/late expiry changed terminal economics: {discoveries:?}"
    );
}

#[test]
fn v16_program_bounded_reference_graph_exhausts_public_action_words() {
    let evidence = run_bounded_reference_equivalence_graph()
        .expect("INV-086 bounded deployed/reference graph");
    assert_eq!(
        evidence.depth_three_exact_state_count, 551,
        "the authenticated exact depth-three state frontier changed and must be reviewed"
    );
    assert_eq!(
        evidence.depth_four_word_count,
        evidence.depth_three_exact_state_count * 13,
        "every exact depth-three state must be extended by every action"
    );
    assert_eq!(
        evidence.word_count,
        2_380 + evidence.depth_four_word_count,
        "must exhaust depth three and the authenticated-state-reduced depth-four frontier"
    );
    assert_eq!(
        evidence.transition_count,
        6_942 + evidence.depth_four_word_count * 4,
        "must replay every edge in every bounded word"
    );
    assert!(
        evidence.unique_node_count >= 60 && evidence.unique_edge_count >= 140,
        "bounded graph collapsed to vacuous state coverage: {evidence:?}"
    );
    assert!(
        evidence.depth_three_unique_node_count > evidence.depth_two_unique_node_count
            && evidence.depth_three_unique_edge_count > evidence.depth_two_unique_edge_count,
        "third actions must discover normalized states and edges beyond depth two: {evidence:?}"
    );
    assert!(
        evidence.unique_node_count > evidence.depth_three_unique_node_count
            && evidence.unique_edge_count > evidence.depth_three_unique_edge_count,
        "the reduced fourth frontier must discover normalized states and edges beyond depth three: {evidence:?}"
    );
    assert!(
        evidence
            .action_attempts
            .iter()
            .all(|attempts| *attempts >= 534),
        "every action must retain its exhaustive first/second/third-position coverage: {evidence:?}"
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
    assert!(
        evidence
            .fourth_position_attempts
            .iter()
            .all(|attempts| *attempts == evidence.depth_three_exact_state_count as u64),
        "every action must extend every exact depth-three state: {evidence:?}"
    );
    assert!(
        evidence
            .fourth_position_state_changes
            .iter()
            .all(|changes| *changes != 0),
        "every action class must produce a real fourth-position state transition: {evidence:?}"
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
fn v16_program_recovery_seeded_frontier_preserves_bounded_owner_exit() {
    let evidence =
        run_bounded_recovery_reference_frontier().expect("INV-086 public Recovery seeded frontier");

    assert_eq!(
        evidence.word_count, 366,
        "must exhaust two expiry seeds x (13^0 + 13^1 + 13^2) Recovery words"
    );
    assert_eq!(
        evidence.transition_count, 702,
        "must replay every edge in every one- and two-action Recovery word"
    );
    assert_eq!(evidence.fresh_seed_world_count, 183);
    assert_eq!(evidence.exact_expiry_seed_world_count, 183);
    assert_eq!(
        evidence.nonflat_seed_world_count, evidence.word_count,
        "every Recovery word must start with booked or pending nonflat value"
    );
    assert_eq!(
        evidence.bounded_exit_world_count, evidence.word_count,
        "every reached Recovery state must have a bounded owner exit"
    );
    assert_eq!(
        evidence.value_moving_exit_world_count, evidence.word_count,
        "every exit witness must move real funded user value"
    );
    assert!(
        evidence.unique_node_count >= 20 && evidence.unique_edge_count >= 50,
        "Recovery frontier collapsed to vacuous exact-state coverage: {evidence:?}"
    );
    assert!(
        evidence
            .action_attempts
            .iter()
            .all(|attempts| *attempts == 54),
        "every Recovery action must occupy every first and second position: {evidence:?}"
    );
    assert!(
        evidence
            .second_position_attempts
            .iter()
            .all(|attempts| *attempts == 26),
        "every Recovery action must follow every first action in both expiry states: {evidence:?}"
    );
    for action_index in [0usize, 1, 2, 3, 4, 5, 6, 7, 8, 11, 12] {
        assert_ne!(
            evidence.action_state_changes[action_index], 0,
            "every progressing Recovery action must mutate economic state in at least one reached context: {evidence:?}"
        );
        assert_ne!(
            evidence.second_position_state_changes[action_index], 0,
            "every progressing Recovery action must have a nonvacuous ordered composition: {evidence:?}"
        );
    }
    for action_index in [9usize, 10] {
        assert_eq!(
            evidence.action_state_changes[action_index], 0,
            "live-mode rebalance must reject atomically throughout Recovery: {evidence:?}"
        );
        assert_eq!(evidence.second_position_state_changes[action_index], 0);
    }
    assert_ne!(evidence.coverage.loaded_program_hash, [0; 32]);
    assert!(
        evidence.coverage.recovery_forfeit_successes != 0
            && evidence.coverage.force_close_successes != 0
            && evidence.coverage.backing_topups != 0
            && evidence.coverage.insurance_topups != 0
            && evidence.coverage.deposits != 0
            && evidence.coverage.rebalance_reductions == 0
            && evidence.coverage.terminal_resolves != 0
            && evidence.coverage.resolved_close_mutations != 0,
        "Recovery frontier did not traverse its intended public lifecycle classes: {evidence:?}"
    );
}

#[test]
fn v16_program_explicit_b_seeded_frontier_preserves_bounded_owner_exit() {
    let evidence =
        run_bounded_b_reference_frontier().expect("INV-086 public explicit-B seeded frontier");

    assert_eq!(
        evidence.word_count, 366,
        "must exhaust two side seeds x (13^0 + 13^1 + 13^2) explicit-B words"
    );
    assert_eq!(
        evidence.transition_count, 702,
        "must replay every edge in every one- and two-action explicit-B word"
    );
    assert_eq!(evidence.long_seed_world_count, 183);
    assert_eq!(evidence.short_seed_world_count, 183);
    assert_eq!(
        evidence.explicit_b_seed_world_count, evidence.word_count,
        "every word must start from a nonzero side-local B target/snapshot gap"
    );
    assert_eq!(
        evidence.bounded_exit_world_count, evidence.word_count,
        "every reached B state must retain a bounded owner exit"
    );
    assert_eq!(
        evidence.value_moving_exit_world_count, evidence.word_count,
        "every B exit witness must move real funded user value"
    );
    assert!(
        evidence.unique_node_count >= 20 && evidence.unique_edge_count >= 40,
        "explicit-B frontier collapsed to vacuous exact-state coverage: {evidence:?}"
    );
    assert!(
        evidence
            .action_attempts
            .iter()
            .all(|attempts| *attempts == 54),
        "every explicit-B action must occupy every first and second position: {evidence:?}"
    );
    assert!(
        evidence
            .second_position_attempts
            .iter()
            .all(|attempts| *attempts == 26),
        "every explicit-B action must follow every first action in both side states: {evidence:?}"
    );
    for action_index in [0usize, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 12] {
        assert_ne!(
            evidence.action_state_changes[action_index], 0,
            "each economic/lifecycle action must mutate in at least one B context: {evidence:?}"
        );
        assert_ne!(
            evidence.second_position_state_changes[action_index], 0,
            "each economic/lifecycle action must have a nonvacuous ordered B composition: {evidence:?}"
        );
    }
    assert_eq!(
        evidence.action_state_changes[10], 0,
        "authority shutdown must roll back exactly while the booked B/obligation episode remains live: {evidence:?}"
    );
    assert_eq!(evidence.second_position_state_changes[10], 0);
    assert!(
        evidence.b_rank_reducing_edges[0] != 0 && evidence.b_rank_reducing_edges[1] != 0,
        "complete and empty discovery hints must each dispatch real B-rank progress: {evidence:?}"
    );
    assert_ne!(evidence.coverage.loaded_program_hash, [0; 32]);
    assert!(
        evidence.coverage.crank_progress != 0
            && evidence.coverage.deposits != 0
            && evidence.coverage.withdrawals != 0
            && evidence.coverage.route_success[0] != 0
            && evidence.coverage.matcher_config_updates != 0
            && evidence.coverage.rebalance_reductions != 0
            && evidence.coverage.mark_updates != 0
            && evidence.coverage.lifecycle_updates == 0
            && evidence.coverage.resolve_policy_updates != 0
            && evidence.coverage.permissionless_resolves != 0
            && evidence.coverage.user_positions_closed != 0,
        "explicit-B frontier did not traverse its intended public action classes: {evidence:?}"
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

#[test]
fn v16_program_public_sequence_reaches_dual_nonunit_adl_indices() {
    let discoveries =
        verify_dual_adl_prefixes([0xda; 32]).expect("INV-086 public dual-ADL prefixes");
    assert_eq!(
        discoveries.len(),
        DiscoveryTradeRoute::ALL.len(),
        "every opening transport must reach the same dual-scaled topology"
    );
    for discovery in discoveries {
        assert!(
            discovery.satisfies_invariant(),
            "public dual-ADL prefix failed: {discovery:?}"
        );
    }
}

#[test]
fn v16_program_scaled_liquidation_matches_independent_selector_model() {
    let discoveries = verify_dual_adl_liquidation_sizing([0x1a; 32])
        .expect("INV-086 scaled liquidation sizing matrix");
    assert_eq!(
        discoveries.len(),
        DiscoveryTradeRoute::ALL.len(),
        "every opening transport must reach one independently modeled scaled liquidation"
    );
    for discovery in &discoveries {
        assert!(
            discovery.satisfies_invariant(),
            "scaled liquidation diverged from the independent pre-state model: {discovery:?}"
        );
    }
    let control = discoveries[0];
    for candidate in discoveries.iter().skip(1) {
        assert_eq!(
            (
                candidate.pre_long_a,
                candidate.pre_short_a,
                candidate.pre_raw_basis_q,
                candidate.pre_effective_q,
                candidate.pre_long_oi_q,
                candidate.pre_short_oi_q,
                candidate.pre_certified_liq_deficit,
                candidate.expected_close_q,
                candidate.observed_long_oi_reduce_q,
                candidate.observed_short_oi_reduce_q,
                candidate.expected_effective_after_q,
                candidate.observed_effective_after_q,
            ),
            (
                control.pre_long_a,
                control.pre_short_a,
                control.pre_raw_basis_q,
                control.pre_effective_q,
                control.pre_long_oi_q,
                control.pre_short_oi_q,
                control.pre_certified_liq_deficit,
                control.expected_close_q,
                control.observed_long_oi_reduce_q,
                control.observed_short_oi_reduce_q,
                control.expected_effective_after_q,
                control.observed_effective_after_q,
            ),
            "opening transport changed scaled-liquidation economics: control={control:?}, candidate={candidate:?}"
        );
    }
}

#[test]
fn v16_program_dual_adl_recovery_forfeit_matches_effective_oi_model() {
    let discoveries = verify_dual_adl_recovery_forfeit_matrix([0x4f; 32])
        .expect("INV-086 dual-ADL Recovery-forfeit matrix");
    assert_eq!(
        discoveries.len(),
        DiscoveryTradeRoute::ALL.len()
            * AdlForceCloseAccountOrder::ALL.len()
            * RecoveryForfeitBudget::ALL.len(),
        "must cross every opening route, owner order, and B work-budget boundary"
    );
    for discovery in &discoveries {
        assert!(
            discovery.satisfies_invariant(),
            "dual-ADL Recovery-forfeit composition failed: {discovery:?}"
        );
    }

    for route_worlds in discoveries
        .chunks_exact(AdlForceCloseAccountOrder::ALL.len() * RecoveryForfeitBudget::ALL.len())
    {
        for budget_pair in route_worlds.chunks_exact(RecoveryForfeitBudget::ALL.len()) {
            let one = budget_pair[0];
            let maximum = budget_pair[1];
            assert_eq!(one.budget, RecoveryForfeitBudget::One);
            assert_eq!(maximum.budget, RecoveryForfeitBudget::Maximum);
            assert_eq!(
                (
                    one.route,
                    one.account_order,
                    one.long_a_before,
                    one.short_a_before,
                    one.first_raw_basis_q,
                    one.first_effective_q,
                    one.second_raw_basis_q,
                    one.second_effective_q,
                    one.first_own_oi_reduce_q,
                    one.first_other_oi_reduce_q,
                    one.second_own_oi_reduce_q,
                    one.second_other_oi_reduce_q,
                ),
                (
                    maximum.route,
                    maximum.account_order,
                    maximum.long_a_before,
                    maximum.short_a_before,
                    maximum.first_raw_basis_q,
                    maximum.first_effective_q,
                    maximum.second_raw_basis_q,
                    maximum.second_effective_q,
                    maximum.first_own_oi_reduce_q,
                    maximum.first_other_oi_reduce_q,
                    maximum.second_own_oi_reduce_q,
                    maximum.second_other_oi_reduce_q,
                ),
                "B work budget changed Recovery-forfeit quantity or OI attribution: one={one:?}, maximum={maximum:?}"
            );
            assert_eq!(
                (
                    one.winner_external_payout,
                    one.loser_external_payout,
                    one.winner_funded_value,
                    one.loser_funded_value,
                    one.winner_exit_fee,
                    one.loser_exit_fee,
                    one.classified_protocol_value_after,
                    one.canonical_vault_after,
                    one.users_terminal,
                    one.portfolios_closed,
                ),
                (
                    maximum.winner_external_payout,
                    maximum.loser_external_payout,
                    maximum.winner_funded_value,
                    maximum.loser_funded_value,
                    maximum.winner_exit_fee,
                    maximum.loser_exit_fee,
                    maximum.classified_protocol_value_after,
                    maximum.canonical_vault_after,
                    maximum.users_terminal,
                    maximum.portfolios_closed,
                ),
                "B work budget changed the terminal economic outcome: one={one:?}, maximum={maximum:?}"
            );
        }

        for budget_index in 0..RecoveryForfeitBudget::ALL.len() {
            let winner_first = route_worlds[budget_index];
            let loser_first = route_worlds[RecoveryForfeitBudget::ALL.len() + budget_index];
            assert_eq!(
                winner_first.account_order,
                AdlForceCloseAccountOrder::WinnerFirst
            );
            assert_eq!(
                loser_first.account_order,
                AdlForceCloseAccountOrder::LoserFirst
            );
            assert_eq!(winner_first.budget, loser_first.budget);
            assert_eq!(
                (
                    winner_first.winner_external_payout,
                    winner_first.loser_external_payout,
                    winner_first.classified_protocol_value_after,
                    winner_first.canonical_vault_after,
                    winner_first.users_terminal,
                    winner_first.portfolios_closed,
                ),
                (
                    loser_first.winner_external_payout,
                    loser_first.loser_external_payout,
                    loser_first.classified_protocol_value_after,
                    loser_first.canonical_vault_after,
                    loser_first.users_terminal,
                    loser_first.portfolios_closed,
                ),
                "owner landing order changed the terminal economic outcome: winner_first={winner_first:?}, loser_first={loser_first:?}"
            );
        }
    }

    let worlds_per_route = AdlForceCloseAccountOrder::ALL.len() * RecoveryForfeitBudget::ALL.len();
    for variant_index in 0..worlds_per_route {
        let control = discoveries[variant_index];
        for route_index in 1..DiscoveryTradeRoute::ALL.len() {
            let candidate = discoveries[route_index * worlds_per_route + variant_index];
            assert_eq!(control.account_order, candidate.account_order);
            assert_eq!(control.budget, candidate.budget);
            assert_eq!(
                (
                    control.long_a_before,
                    control.short_a_before,
                    control.first_raw_basis_q,
                    control.first_effective_q,
                    control.second_raw_basis_q,
                    control.second_effective_q,
                    control.first_own_oi_reduce_q,
                    control.first_other_oi_reduce_q,
                    control.second_own_oi_reduce_q,
                    control.second_other_oi_reduce_q,
                ),
                (
                    candidate.long_a_before,
                    candidate.short_a_before,
                    candidate.first_raw_basis_q,
                    candidate.first_effective_q,
                    candidate.second_raw_basis_q,
                    candidate.second_effective_q,
                    candidate.first_own_oi_reduce_q,
                    candidate.first_other_oi_reduce_q,
                    candidate.second_own_oi_reduce_q,
                    candidate.second_other_oi_reduce_q,
                ),
                "opening transport changed Recovery-forfeit quantity or attribution: control={control:?}, candidate={candidate:?}"
            );
            assert_eq!(
                (
                    control.winner_external_payout,
                    control.loser_external_payout,
                    control.classified_protocol_value_after,
                    control.canonical_vault_after,
                ),
                (
                    candidate.winner_external_payout,
                    candidate.loser_external_payout,
                    candidate.classified_protocol_value_after,
                    candidate.canonical_vault_after,
                ),
                "opening transport changed Recovery-forfeit terminal economics: control={control:?}, candidate={candidate:?}"
            );
        }
    }
}

#[test]
fn v16_program_dual_adl_force_close_clamps_stale_and_raw_work() {
    let discoveries = verify_dual_adl_force_close_clamp_matrix([0x2a; 32])
        .expect("INV-086 dual-ADL force-close clamp matrix");
    assert_eq!(
        discoveries.len(),
        DiscoveryTradeRoute::ALL.len()
            * AdlReductionBoundary::ALL.len()
            * AdlForceCloseAccountOrder::ALL.len(),
        "must cross every opening route, force-close boundary, and account order"
    );
    for discovery in discoveries {
        assert!(
            discovery.satisfies_invariant()
                && discovery.long_a_before < percolator::ADL_ONE
                && discovery.short_a_before < percolator::ADL_ONE
                && discovery.winner_raw_basis_before_q > discovery.effective_before_q
                && discovery.loser_raw_basis_before_q > discovery.effective_before_q,
            "dual-ADL force-close composition failed: {discovery:?}"
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
