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
//! A third seeded frontier starts from a publicly constructed active close in both position
//! orientations at the authenticated slots immediately before, exactly at, and immediately after
//! `max_close_slot`. It exhausts every one- and two-action ordering across complete/empty-hint
//! close progress, an unrelated crank, exact public cure, owner deposit/withdrawal, unrelated
//! reduction, same- and cross-asset marks, shutdown, authority resolution, policy update, and
//! permissionless stale resolution. Actions outside close progress and cure must frame the exact
//! close episode; every reached state must retain a bounded value-moving owner exit.
//! A fourth seeded frontier publicly creates a real counterparty-backed source lien in both source
//! side orientations, expires it into exact `Impaired` attribution, and exhausts every empty,
//! one-action, and ordered two-action word over thirteen progress, funding, reduction, mark, and
//! resolution actions. The exact source-credit-dependent increase rejects from both initial seeds,
//! Live actions cannot silently erase impaired provider attribution, and every reached world must
//! clear the lien through a bounded value-moving terminal path.
//! A fifth seeded frontier starts from public nonfinal payout receipts immediately before and at a
//! backing-expiry boundary. Every rebuilt seed rejects premature portfolio dematerialization, then
//! crosses claimant claim/close/crank, every peer close/claim/crank route, and premature slab
//! closure in every empty, one-action, and ordered two-action word. Receipt identity must remain
//! immutable until exact terminal payment, paid value monotonic, rejected destruction exact, and
//! every ordering must converge to the same seed-local terminal engine and SPL outcome.
//! A sixth seeded frontier starts from funded Hybrid positions after the final authenticated feed
//! accounts become unavailable. It crosses the authenticated slots immediately before and exactly
//! at stale-resolution maturity with oracle-free and empty-hint cranks, declared-missing,
//! wrong-owner, stale, and newly recovered feed tails, all four risk-reducing trade transports,
//! permissionless resolution, and resolved close in every empty, one-action, and ordered two-action
//! word. Invalid feed edges must roll back exactly, a newly valid feed must restore Live progress,
//! and every ordering must retain an oracle-free, value-moving terminal continuation.
//! A seventh seeded frontier starts from a prior-epoch `ResetPending` leg created by an owner
//! reduction, in both side orientations. It exhausts complete/empty crank shapes, both owner
//! progress and explicit side finalization, owner value actions, mark movement, matcher revocation,
//! all four fresh-risk transports, asset shutdown, and market resolution in every empty,
//! one-action, and ordered two-action word. Early finalization and stale account cranks must reject
//! exactly, explicit finalization must lower reset rank after prior-epoch account work clears, and
//! fresh risk must reject while the reset episode remains active. Every ordering must retain a
//! bounded, value-moving terminal exit.
//! A minimized generated public trace also retains simultaneous same-slot mark and account work.
//! Empty hints reject with exact rollback, while both the indiscriminate all-asset set and a proper
//! authenticated subset decrease rank before every funded owner exits. The independent liveness
//! oracle searches the bounded nonempty subsets rather than relying on one observation shape. This
//! is scheduler coverage, not a wrapper exception: each attempted transition is still the sole
//! deployed public crank.
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
//! The source-complete composition gate closes the remaining current-surface dimension question:
//! it binds the generated transition runner's per-step independent oracles to the production-derived
//! public instruction and wrapper-to-engine callsite censuses, then requires executable owners for
//! identity, value, source/insurance, position/OI, route/partition, lifecycle/terminal, seeded
//! frontier, and maximum-shape dimensions. A new public route, engine callsite, supported shape,
//! engine pin, or missing owner reopens the gate.
//! This is finite reachability evidence, not equivalence over unbounded sequences.

use super::*;
use crate::support::fuzz_model::{
    run_bounded_active_close_reference_frontier, run_bounded_b_reference_frontier,
    run_bounded_lien_impairment_reference_frontier, run_bounded_oracle_failure_reference_frontier,
    run_bounded_receipt_conflict_reference_frontier, run_bounded_recovery_reference_frontier,
    run_bounded_reference_equivalence_graph, run_bounded_reset_pending_reference_frontier,
    verify_close_to_partial_receipt_composition, verify_constructible_crank_observation_subset,
    verify_expired_backing_terminal_cleanup_compositions,
    verify_insurance_liquidation_to_partial_receipt_compositions,
    verify_liquidation_to_partial_receipt_compositions,
    verify_terminal_slab_revisits_prior_recredit_after_later_expiry,
};

#[test]
fn v16_program_reset_pending_seeded_frontier_is_exact_and_terminal() {
    let evidence = run_bounded_reset_pending_reference_frontier()
        .expect("INV-055/057/065/071/072/073/078/082/086 ResetPending public frontier");

    assert_eq!(evidence.word_count, 546, "{evidence:?}");
    assert_eq!(evidence.transition_count, 1_056, "{evidence:?}");
    assert_eq!(evidence.long_reset_seed_world_count, 273, "{evidence:?}");
    assert_eq!(evidence.short_reset_seed_world_count, 273, "{evidence:?}");
    assert_eq!(evidence.actionable_seed_world_count, 546, "{evidence:?}");
    assert_eq!(evidence.bounded_exit_world_count, 546, "{evidence:?}");
    assert_eq!(evidence.value_moving_exit_world_count, 546, "{evidence:?}");
    assert_eq!(evidence.unique_node_count, 72, "{evidence:?}");
    assert_eq!(evidence.unique_edge_count, 224, "{evidence:?}");
    assert!(
        evidence.action_attempts.iter().all(|count| *count == 66),
        "every action must occupy every one/two-action position: {evidence:?}"
    );
    assert!(
        evidence
            .second_position_attempts
            .iter()
            .all(|count| *count == 32),
        "every action must land second after every possible first action: {evidence:?}"
    );
    assert!(
        evidence.reset_rank_reducing_edges.iter().sum::<u64>() != 0,
        "the frontier must contain strict reset-rank progress: {evidence:?}"
    );
    assert_ne!(
        evidence.reset_rank_reducing_edges[4], 0,
        "FinalizeResetSide must lower reset work after an earlier crank clears the prior-epoch leg: {evidence:?}"
    );
    for route in 0..4 {
        assert_ne!(
            evidence.pending_fresh_risk_attempts[route], 0,
            "route {route} never probed active ResetPending admission: {evidence:?}"
        );
        assert_eq!(
            evidence.pending_fresh_risk_rejections[route],
            evidence.pending_fresh_risk_attempts[route],
            "route {route} admitted fresh risk before reset completion: {evidence:?}"
        );
    }
    assert!(
        evidence.coverage.max_cu != 0
            && evidence.coverage.max_cu < crate::support::v16_svm::TX_CU_LIMIT,
        "every public edge and terminal campaign must remain bounded: {evidence:?}"
    );
}

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
fn terminal_slab_revisits_earlier_recredit_after_later_backing_expiry() {
    let evidence = verify_terminal_slab_revisits_prior_recredit_after_later_expiry()
        .expect("public terminal cleanup with an earlier overlap and later expiring backing");

    // Public calls first park the bounded scan on live backing at a later asset. Once that
    // backing expires, the released residual must be applied to every earlier insurance overlap
    // before any remaining custody is retired.
    assert_eq!(
        (
            evidence.insurance_spent,
            evidence.terminal.terminal_backing_withdrawn,
            evidence.terminal.terminal_backing_expired,
            evidence.terminal.terminal_insurance_withdrawn,
            evidence.terminal.slab_progress_steps,
            evidence.terminal.slab_custody_burned,
            evidence.terminal.slab_closed,
        ),
        (123, 0, 750, 123, 4, 628, true),
        "backing expiry must restart the terminal scan before custody can be retired: {evidence:?}",
    );
    assert!(
        evidence.terminal.max_compute_units < 1_400_000
            && evidence.terminal.slab_close_compute_units != 0
            && evidence.terminal.slab_close_compute_units < 1_400_000,
        "the restarted bounded scan must retain transaction headroom: {evidence:?}",
    );
}

#[test]
fn v16_program_bounded_reference_graph_exhausts_public_action_words() {
    let evidence = run_bounded_reference_equivalence_graph()
        .expect("INV-086 bounded deployed/reference graph");
    assert_eq!(
        evidence.depth_three_exact_state_count, 685,
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
fn v16_program_active_close_seeded_frontier_preserves_episode_and_bounded_owner_exit() {
    let evidence = run_bounded_active_close_reference_frontier()
        .expect("INV-086 public active-close seeded frontier");

    assert_eq!(
        evidence.word_count, 1_098,
        "must exhaust two sides x three expiry boundaries x (13^0 + 13^1 + 13^2) words"
    );
    assert_eq!(
        evidence.transition_count, 2_106,
        "must replay every edge in every one- and two-action active-close word"
    );
    assert_eq!(evidence.long_seed_world_count, 549);
    assert_eq!(evidence.short_seed_world_count, 549);
    assert_eq!(evidence.before_expiry_seed_world_count, 366);
    assert_eq!(evidence.at_expiry_seed_world_count, 366);
    assert_eq!(evidence.after_expiry_seed_world_count, 366);
    assert_eq!(
        evidence.active_close_seed_world_count, evidence.word_count,
        "every word must start from a public active close with a nonzero residual"
    );
    assert_eq!(
        evidence.bounded_exit_world_count, evidence.word_count,
        "every reached active-close state must retain a bounded owner exit"
    );
    assert_eq!(
        evidence.value_moving_exit_world_count, evidence.word_count,
        "every active-close exit witness must move real funded user value"
    );
    assert!(
        evidence.unique_node_count >= 40 && evidence.unique_edge_count >= 80,
        "active-close frontier collapsed to vacuous exact-state coverage: {evidence:?}"
    );
    assert!(
        evidence
            .action_attempts
            .iter()
            .all(|attempts| *attempts == 162),
        "every active-close action must occupy every first and second position: {evidence:?}"
    );
    assert!(
        evidence
            .second_position_attempts
            .iter()
            .all(|attempts| *attempts == 78),
        "every active-close action must follow every first action in all six seeds: {evidence:?}"
    );
    assert!(
        evidence
            .action_state_changes
            .iter()
            .all(|changes| *changes != 0)
            && evidence
                .second_position_state_changes
                .iter()
                .all(|changes| *changes != 0),
        "every action must have a nonvacuous first- and second-position composition: {evidence:?}"
    );
    assert!(
        evidence.close_rank_reducing_edges[0] != 0
            && evidence.close_rank_reducing_edges[1] != 0
            && evidence.close_rank_reducing_edges[3] != 0,
        "both honest hint shapes and an exact cure must reduce active-close rank somewhere: {evidence:?}"
    );
    for action_index in [2usize, 4, 5, 6, 7, 8, 9, 10, 11, 12] {
        assert_eq!(
            evidence.close_frame_edges[action_index],
            evidence.action_attempts[action_index],
            "non-close action {action_index} must frame the exact close episode on every ordering: {evidence:?}"
        );
    }
    assert!(
        evidence.cure_success_count != 0 && evidence.cure_rejection_count != 0,
        "the expiry/order product must cover both valid cure and exact rollback after cure becomes inadmissible: {evidence:?}"
    );
    assert_ne!(evidence.coverage.loaded_program_hash, [0; 32]);
    assert!(
        evidence.coverage.crank_progress != 0
            && evidence.coverage.deposits != 0
            && evidence.coverage.withdrawals != 0
            && evidence.coverage.route_success[0] != 0
            && evidence.coverage.mark_updates != 0
            && evidence.coverage.extended_action_attempts[6] == 162
            && evidence.coverage.lifecycle_updates == 0
            && evidence.coverage.resolve_policy_updates != 0
            && evidence.coverage.permissionless_resolves != 0
            && evidence.coverage.terminal_resolves != 0
            && evidence.coverage.user_positions_closed != 0,
        "active-close frontier did not traverse its intended public action classes or admitted a forbidden shutdown: {evidence:?}"
    );
}

#[test]
fn v16_program_lien_impairment_seeded_frontier_preserves_bounded_owner_exit() {
    let evidence = run_bounded_lien_impairment_reference_frontier()
        .expect("INV-086 public lien-impairment seeded frontier");

    assert_eq!(
        evidence.word_count, 366,
        "must exhaust two side seeds x (13^0 + 13^1 + 13^2) impaired-lien words"
    );
    assert_eq!(
        evidence.transition_count, 702,
        "must replay every edge in every one- and two-action impaired-lien word"
    );
    assert_eq!(evidence.long_seed_world_count, 183);
    assert_eq!(evidence.short_seed_world_count, 183);
    assert_eq!(
        evidence.impaired_seed_world_count, evidence.word_count,
        "every word must start from a public nonzero impaired counterparty lien"
    );
    assert_eq!(
        evidence.bounded_exit_world_count, evidence.word_count,
        "every reached impaired-lien state must retain a bounded owner exit"
    );
    assert_eq!(
        evidence.value_moving_exit_world_count, evidence.word_count,
        "every impaired-lien exit witness must move funded user value"
    );
    assert!(
        evidence.unique_node_count >= 20 && evidence.unique_edge_count >= 40,
        "impaired-lien frontier collapsed to vacuous exact-state coverage: {evidence:?}"
    );
    assert!(
        evidence
            .action_attempts
            .iter()
            .all(|attempts| *attempts == 54),
        "every impaired-lien action must occupy every first and second position: {evidence:?}"
    );
    assert!(
        evidence
            .second_position_attempts
            .iter()
            .all(|attempts| *attempts == 26),
        "every impaired-lien action must follow every first action in both side seeds: {evidence:?}"
    );
    assert_eq!(
        evidence.action_state_changes[5],
        evidence.second_position_state_changes[5],
        "the exact source-credit-dependent increase must reject from both initial impaired seeds: {evidence:?}"
    );
    assert!(
        evidence
            .impaired_lien_reducing_edges
            .iter()
            .all(|reductions| *reductions == 0),
        "Live-state actions must not silently erase impaired provider attribution before terminal reconciliation: {evidence:?}"
    );
    assert_ne!(evidence.coverage.loaded_program_hash, [0; 32]);
    assert!(
        evidence.coverage.crank_progress != 0
            && evidence.coverage.deposits != 0
            && evidence.coverage.withdrawals != 0
            && evidence.coverage.route_success[0] != 0
            && evidence.coverage.route_reject[0] != 0
            && evidence.coverage.backing_topups != 0
            && evidence.coverage.mark_updates != 0
            && evidence.coverage.resolve_policy_updates != 0
            && evidence.coverage.permissionless_resolves != 0
            && evidence.coverage.user_positions_closed != 0,
        "impaired-lien frontier did not traverse its intended public action classes: {evidence:?}"
    );
}

#[test]
fn v16_program_receipt_conflict_seeded_frontier_is_exact_and_terminal() {
    let evidence = run_bounded_receipt_conflict_reference_frontier()
        .expect("INV-086 public receipt-conflict seeded frontier");

    assert_eq!(
        evidence.word_count, 366,
        "must exhaust two expiry seeds x (13^0 + 13^1 + 13^2) receipt-conflict words"
    );
    assert_eq!(evidence.transition_count, 702);
    assert_eq!(evidence.before_expiry_seed_world_count, 183);
    assert_eq!(evidence.exact_expiry_seed_world_count, 183);
    assert_eq!(
        evidence.partial_receipt_seed_world_count, evidence.word_count,
        "every word must begin from a genuine public nonfinal receipt"
    );
    assert_eq!(evidence.bounded_terminal_world_count, evidence.word_count);
    assert_eq!(
        evidence.value_moving_terminal_world_count, evidence.word_count,
        "every receipt-conflict world must retain a funded value-moving terminal continuation"
    );
    assert_eq!(
        evidence.terminal_outcome_count_by_seed,
        [1, 1],
        "terminal engine/SPL economics must be order-independent within each expiry seed"
    );
    assert!(
        evidence.unique_node_count >= 8 && evidence.unique_edge_count >= 60,
        "receipt-conflict frontier collapsed below its canonical exact-state/route coverage: {evidence:?}"
    );
    assert!(
        evidence
            .action_attempts
            .iter()
            .all(|attempts| *attempts == 54),
        "every receipt action must occupy every first and second position: {evidence:?}"
    );
    assert!(
        evidence
            .second_position_attempts
            .iter()
            .all(|attempts| *attempts == 26),
        "every receipt action must follow every first action in both expiry seeds: {evidence:?}"
    );
    assert_eq!(
        evidence.premature_portfolio_close_rejections,
        evidence.word_count as u64
    );
    assert_eq!(evidence.premature_slab_close_rejections, 54);
    for action in [5usize, 7, 9, 12] {
        assert_eq!(
            evidence.action_state_changes[action], 0,
            "already-terminal close and premature slab-close controls must be exact nonmutations: {evidence:?}"
        );
    }
    assert!(
        [0usize, 1, 2, 3, 4, 6, 8, 10, 11]
            .into_iter()
            .all(|action| evidence.action_state_changes[action] != 0),
        "a live receipt/peer route never produced a real state transition: {evidence:?}"
    );
    assert!(
        evidence.receipt_completion_edges != 0
            && evidence.payout_edges.iter().any(|edges| *edges != 0)
            && evidence.coverage.resolved_claim_mutations != 0
            && evidence.coverage.resolved_close_mutations != 0
            && evidence.coverage.resolved_crank_mutations != 0
            && evidence.coverage.resolved_payout_atoms != 0,
        "receipt-conflict frontier did not exercise paying claim/close/crank transitions: {evidence:?}"
    );
    assert_ne!(evidence.coverage.loaded_program_hash, [0; 32]);
}

#[test]
fn v16_program_oracle_failure_seeded_frontier_is_exact_and_terminal() {
    let evidence = run_bounded_oracle_failure_reference_frontier()
        .expect("INV-086 public oracle-failure seeded frontier");

    assert_eq!(
        evidence.word_count, 366,
        "must exhaust two stale-boundary seeds x (13^0 + 13^1 + 13^2) oracle-failure words"
    );
    assert_eq!(evidence.transition_count, 702);
    assert_eq!(evidence.before_maturity_seed_world_count, 183);
    assert_eq!(evidence.exact_maturity_seed_world_count, 183);
    assert_eq!(
        evidence.unavailable_feed_seed_world_count, evidence.word_count,
        "every word must begin after all configured external feed accounts are unavailable"
    );
    assert_eq!(evidence.bounded_terminal_world_count, evidence.word_count);
    assert_eq!(
        evidence.value_moving_terminal_world_count, evidence.word_count,
        "every unavailable-feed ordering must retain a funded value-moving terminal path"
    );
    assert!(
        evidence.unique_node_count > 2 && evidence.unique_edge_count > 20,
        "oracle-failure frontier collapsed below substantive exact-state coverage: {evidence:?}"
    );
    assert!(
        evidence
            .action_attempts
            .iter()
            .all(|attempts| *attempts == 54),
        "every oracle-failure action must occupy every first and second position: {evidence:?}"
    );
    assert!(
        evidence
            .second_position_attempts
            .iter()
            .all(|attempts| *attempts == 26),
        "every oracle-failure action must follow every first action in both boundary seeds: {evidence:?}"
    );
    assert!(
        evidence.fallback_progress_edges != 0 && evidence.fresh_feed_recovery_edges != 0,
        "the frontier must exercise both oracle-free settlement and authenticated feed recovery: {evidence:?}"
    );
    for malformed_action in [3usize, 4] {
        assert!(
            evidence.action_rejections[malformed_action] != 0
                && evidence.action_state_changes[malformed_action] == 0,
            "missing and wrong-owner oracle tails must reject exactly: {evidence:?}"
        );
    }
    assert!(
        evidence.action_rejections[5] != 0 || evidence.action_state_changes[5] != 0,
        "stale tails must either reject exactly or take a safe retained-mark fallback edge: {evidence:?}"
    );
    assert!(
        evidence
            .coverage
            .route_success
            .iter()
            .all(|successes| *successes != 0),
        "all four signed risk-reducing transports must work before stale terminal maturity: {evidence:?}"
    );
    assert!(
        evidence.coverage.permissionless_resolves != 0
            && evidence.coverage.resolved_crank_mutations != 0
            && evidence.coverage.resolved_payout_atoms != 0,
        "the unavailable-feed graph did not compose through public resolution and funded custody: {evidence:?}"
    );
    assert_ne!(evidence.coverage.loaded_program_hash, [0; 32]);
}

#[test]
fn v16_program_same_slot_pending_mark_has_constructible_crank_and_exit() {
    let scenario = Scenario {
        seed: [
            0, 0, 0, 0, 0, 0, 0, 0, 3, 226, 99, 16, 163, 116, 29, 14, 168, 15, 164, 234, 213, 45,
            97, 71, 127, 215, 188, 146, 248, 193, 223, 249,
        ],
        config: SmallMarketConfig {
            max_price_move_bps_per_slot: 1,
            max_accrual_dt_slots: 4,
            max_abs_funding_e9_per_slot: 10_000,
            maintenance_fee_per_slot: 0,
        },
        actions: vec![
            Action::SyncMaintenanceFee { actor: 53, dt: 2 },
            Action::PushMark {
                asset: 43,
                dt: 1,
                move_bps: 62,
            },
            Action::TopUpInsurance {
                domain: 24,
                amount: 305,
            },
            Action::ResolveStalePermissionless { dt: 196 },
            Action::Crank {
                actor: 149,
                hints: HintMode::Complete,
            },
            Action::ConfigurePermissionlessResolve {
                stale_slots: 27_345,
                force_close_delay_slots: 28_028,
            },
            Action::RestartAssetOracle {
                asset: 113,
                dt: 79,
                initial_price: 100,
            },
            Action::PushMark {
                asset: 1,
                dt: 3,
                move_bps: -204,
            },
        ],
    };

    let coverage =
        verify_constructible_crank_observation_subset(&scenario, 59).unwrap_or_else(|error| {
            panic!(
                "same-slot pending-mark progress failed: {}",
                error.chars().take(3_000).collect::<String>()
            )
        });
    assert_ne!(
        coverage.crank_proper_observation_subset_progress, 0,
        "the minimized public trace must require and execute a rank-decreasing proper observation subset"
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

#[derive(Clone, Copy)]
struct Inv086ModelDimension {
    dimension: &'static str,
    witnesses: &'static [(&'static str, &'static str)],
}

fn inv086_source_defines_function(source: &str, function: &str) -> bool {
    let marker = format!("fn {function}");
    source.lines().any(|line| {
        line.trim()
            .strip_prefix(&marker)
            .is_some_and(|tail| tail.trim_start().starts_with('('))
    })
}

fn inv086_source_section<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start_offset = source
        .find(start)
        .unwrap_or_else(|| panic!("missing source section start {start}"));
    let tail = &source[start_offset..];
    let end_offset = tail
        .find(end)
        .unwrap_or_else(|| panic!("missing source section end {end}"));
    &tail[..end_offset]
}

#[test]
fn v16_program_reference_model_dimension_composition_is_source_complete() {
    const ENGINE_PIN: &str = "495a5590c97055bd71c6f94d849ff0298f243145";
    const DIMENSIONS: &[Inv086ModelDimension] = &[
        Inv086ModelDimension {
            dimension: "public transition census and per-step independent oracle",
            witnesses: &[
                (
                    "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
                    "v16_program_bounded_reference_graph_exhausts_public_action_words",
                ),
                (
                    "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
                    "v16_program_reference_model_matches_deployed_public_sequence",
                ),
                (
                    "tests/invariants/public_sbf/inv_079_public_reachability_evidence.rs",
                    "v16_public_instruction_coverage_registry_matches_production_roster",
                ),
                (
                    "tests/invariants/cu/inv_088_global_summaries_are_not_account_local_proofs.rs",
                    "v16_program_every_wrapper_engine_transition_callsite_has_summary_disposition_and_witness",
                ),
                (
                    "tests/invariants/cu/inv_080_error_propagation_and_exact_rollback.rs",
                    "v16_program_dispatch_and_entrypoints_preserve_every_handler_error",
                ),
            ],
        },
        Inv086ModelDimension {
            dimension: "identity incarnation replay and authority epoch",
            witnesses: &[
                (
                    "tests/invariants/public_sbf/inv_001_market_incarnation_binding.rs",
                    "v16_program_closed_market_incarnation_cannot_be_recreated",
                ),
                (
                    "tests/invariants/cu/inv_002_asset_generation_binding.rs",
                    "v16_program_asset_generation_field_and_guard_roster_is_source_complete",
                ),
                (
                    "tests/invariants/cu/inv_003_portfolio_incarnation_binding.rs",
                    "v16_program_retained_portfolio_binding_roster_is_source_complete",
                ),
                (
                    "tests/invariants/cu/inv_004_position_episode_binding.rs",
                    "v16_program_retained_position_binding_and_writer_rosters_are_source_complete",
                ),
                (
                    "tests/invariants/cu/inv_005_authority_incarnation_binding.rs",
                    "v16_program_authority_epoch_matrix_is_source_complete",
                ),
                (
                    "tests/invariants/public_sbf/inv_006_program_chain_message_type_and_version_binding.rs",
                    "retained_transaction_binds_program_market_kind_schema_and_blockhash",
                ),
                (
                    "tests/invariants/public_sbf/inv_007_no_aba_reuse.rs",
                    "v16_program_whole_market_recreate_aba_matrix_is_public_and_nonvacuous",
                ),
            ],
        },
        Inv086ModelDimension {
            dimension: "attributed value stock and encumbrance reconciliation",
            witnesses: &[
                (
                    "tests/invariants/stateful/inv_024_attributed_quote_value_conservation.rs",
                    "v16_program_public_trace_enforces_authority_attributed_quote_flow",
                ),
                (
                    "tests/invariants/stateful/inv_025_exact_stock_reconciliation.rs",
                    "v16_program_public_value_lifecycle_reconciles_every_materialized_stock_census",
                ),
                (
                    "tests/invariants/stateful/inv_026_reservation_and_encumbrance_conservation.rs",
                    "v16_program_counterparty_encumbrance_lifecycle_is_exact_across_routes_sides_and_terminal_modes",
                ),
                (
                    "tests/invariants/cu/inv_025_exact_stock_reconciliation.rs",
                    "v16_program_value_routes_reconcile_vault_capital_insurance_and_backing_stocks",
                ),
            ],
        },
        Inv086ModelDimension {
            dimension: "source credit insurance impairment and single use",
            witnesses: &[
                (
                    "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
                    "v16_program_lien_impairment_seeded_frontier_preserves_bounded_owner_exit",
                ),
                (
                    "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
                    "insurance_spend_composes_through_liquidation_partial_receipt_and_terminal_payout",
                ),
                (
                    "tests/invariants/cu/inv_033_insurance_backed_lien_single_classification.rs",
                    "v16_program_public_source_lien_classification_never_double_counts_insurance",
                ),
                (
                    "tests/invariants/stateful/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs",
                    "v16_program_two_accounts_cannot_reserve_the_same_source_backing_atoms",
                ),
                (
                    "tests/invariants/cu/inv_064_insurance_withdrawal_policy_equivalence.rs",
                    "v16_program_live_and_resolved_insurance_withdrawals_share_one_finite_budget",
                ),
            ],
        },
        Inv086ModelDimension {
            dimension: "position OI ADL and repeated liquidation",
            witnesses: &[
                (
                    "tests/invariants/cu/inv_048_matched_trade_and_open_interest_coherence.rs",
                    "v16_program_position_mutation_composition_is_source_complete",
                ),
                (
                    "tests/invariants/cu/inv_048_matched_trade_and_open_interest_coherence.rs",
                    "v16_program_typed_matched_book_obligation_oracle_is_source_complete",
                ),
                (
                    "tests/invariants/cu/inv_061_deterministic_bounded_liquidation.rs",
                    "v16_program_liquidation_composition_is_source_complete",
                ),
                (
                    "tests/invariants/cu/inv_061_deterministic_bounded_liquidation.rs",
                    "v16_program_repeated_partial_liquidation_stops_charging_after_health_restored",
                ),
                (
                    "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
                    "v16_program_scaled_liquidation_matches_independent_selector_model",
                ),
                (
                    "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
                    "v16_program_dual_adl_force_close_clamps_stale_and_raw_work",
                ),
            ],
        },
        Inv086ModelDimension {
            dimension: "equivalent route partition and order products",
            witnesses: &[
                (
                    "tests/invariants/cu/inv_047_equivalent_route_semantics.rs",
                    "v16_program_equivalent_route_family_composition_is_source_complete",
                ),
                (
                    "tests/invariants/cu/inv_052_split_merge_invariance.rs",
                    "v16_program_split_merge_operation_family_composition_is_source_complete",
                ),
                (
                    "tests/invariants/stateful/inv_041_deterministic_allocation_and_caller_order_independence.rs",
                    "v16_program_public_source_lien_allocation_is_domain_order_canonical",
                ),
                (
                    "tests/invariants/stateful/inv_052_split_merge_invariance.rs",
                    "v16_program_public_liquidation_split_and_order_are_conservative",
                ),
                (
                    "tests/invariants/stateful/inv_024_attributed_quote_value_conservation.rs",
                    "v16_program_all_trade_route_pairs_preserve_realized_pnl_owner_attribution",
                ),
            ],
        },
        Inv086ModelDimension {
            dimension: "lifecycle close terminal and reactivation composition",
            witnesses: &[
                (
                    "tests/invariants/cu/inv_065_reset_recovery_and_retired_state_isolation.rs",
                    "v16_program_lifecycle_isolation_composition_is_source_complete",
                ),
                (
                    "tests/invariants/cu/inv_069_terminal_normalization_and_retirement.rs",
                    "v16_program_terminal_blocker_census_composes_engine_retirement_before_wrapper_cleanup",
                ),
                (
                    "tests/invariants/cu/inv_070_zero_unattributed_terminal_residue_and_close_slab.rs",
                    "v16_program_terminal_stock_and_close_slab_composition_is_source_complete",
                ),
                (
                    "tests/invariants/cu/inv_071_crank_progress.rs",
                    "v16_program_crank_progress_and_recovery_composition_is_source_complete",
                ),
                (
                    "tests/invariants/cu/inv_076_close_drift_residual_durability_and_finalization_atomicity.rs",
                    "v16_program_close_finalization_composition_is_source_complete",
                ),
                (
                    "tests/invariants/cu/inv_089_activation_reactivation_and_initialization_equivalence.rs",
                    "v16_program_reused_slot_rejects_fifteenth_leg_then_admits_replacement_at_cap",
                ),
            ],
        },
        Inv086ModelDimension {
            dimension: "independent seeded reachability frontiers",
            witnesses: &[
                (
                    "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
                    "v16_program_recovery_seeded_frontier_preserves_bounded_owner_exit",
                ),
                (
                    "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
                    "v16_program_explicit_b_seeded_frontier_preserves_bounded_owner_exit",
                ),
                (
                    "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
                    "v16_program_active_close_seeded_frontier_preserves_episode_and_bounded_owner_exit",
                ),
                (
                    "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
                    "v16_program_receipt_conflict_seeded_frontier_is_exact_and_terminal",
                ),
                (
                    "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
                    "v16_program_oracle_failure_seeded_frontier_is_exact_and_terminal",
                ),
                (
                    "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
                    "v16_program_reset_pending_seeded_frontier_is_exact_and_terminal",
                ),
            ],
        },
        Inv086ModelDimension {
            dimension: "supported maximum shape and field boundaries",
            witnesses: &[
                (
                    "tests/invariants/cu/inv_077_bounded_work_and_maximum_shape_compute.rs",
                    "v16_attack_public_14_leg_28_source_equal_risk_liquidation_stays_bounded",
                ),
                (
                    "tests/invariants/cu/inv_077_bounded_work_and_maximum_shape_compute.rs",
                    "v16_attack_public_10m_market_max_source_owner_exit_stays_bounded",
                ),
                (
                    "tests/invariants/cu/inv_083_boundary_completeness.rs",
                    "v16_program_every_public_input_field_has_a_boundary_profile_and_executable_witness",
                ),
                (
                    "tests/invariants/cu/inv_083_boundary_completeness.rs",
                    "v16_program_boundary_roster_maps_required_classes_to_owned_tests",
                ),
            ],
        },
    ];

    let cargo = include_str!("../../../Cargo.toml");
    let lock = include_str!("../../../Cargo.lock");
    assert_eq!(
        cargo.matches(&format!("rev = \"{ENGINE_PIN}\"")).count(),
        2,
        "INV-086 composition must be reviewed on every engine pin change",
    );
    assert!(
        lock.contains(&format!("rev={ENGINE_PIN}#{ENGINE_PIN}")),
        "Cargo.lock must resolve the transition-certified engine revision",
    );

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut dimensions = std::collections::BTreeSet::new();
    let mut witnesses = std::collections::BTreeSet::new();
    let mut source_cache = std::collections::BTreeMap::<&str, String>::new();
    for row in DIMENSIONS {
        assert!(
            dimensions.insert(row.dimension),
            "duplicate model dimension"
        );
        assert!(!row.witnesses.is_empty());
        for (path, witness) in row.witnesses {
            assert!(witnesses.insert(*witness), "duplicate witness {witness}");
            let source = source_cache.entry(path).or_insert_with(|| {
                std::fs::read_to_string(root.join(path))
                    .unwrap_or_else(|error| panic!("read {path}: {error}"))
            });
            assert!(
                inv086_source_defines_function(source, witness),
                "model dimension '{}' lacks executable witness {path}#{witness}",
                row.dimension,
            );
        }
    }
    assert_eq!(dimensions.len(), 9, "model dimension roster drift");
    assert_eq!(witnesses.len(), 48, "model witness roster drift");

    let model = include_str!("../../support/fuzz_model.rs");
    for required in [
        "fn account_b_is_current(",
        "fn account_source_backing_is_current(",
        "struct MatchedBookObligationCensus",
        "fn matched_book_obligation_census(",
    ] {
        assert!(
            model.contains(required),
            "independent model lost typed currentness/obligation owner {required}",
        );
    }
    let safety_prefix = inv086_source_section(
        model,
        "pub fn run_safety_prefix",
        "pub fn run_permissionless_progress_campaign",
    );
    for required in [
        "self.apply_action(action)",
        "assert_source_credit_rate_transition(",
        "self.assert_global_invariants()",
    ] {
        assert!(
            safety_prefix.contains(required),
            "every generated action must retain per-step oracle {required}",
        );
    }

    let global = inv086_source_section(
        model,
        "fn assert_global_invariants",
        "fn assert_positions_match",
    );
    for required in [
        "token_supply_observed()",
        "primary.vault != self.env.token_amount(self.env.vault)",
        "primary_capital != primary.c_tot",
        "primary.vault < primary_senior",
        "assert_source_credit_rates(",
        "assert_source_claim_bound_attribution(",
        "assert_public_stock_census(",
        "assert_public_encumbrance_census(",
        "assert_current_certificate_matches_snapshot_full_refresh(",
        "self.assert_positions_match()",
    ] {
        assert!(
            global.contains(required),
            "global deployed-transition oracle lost {required}",
        );
    }

    let replay = inv086_source_section(model, "fn replay_word", "fn replay_words");
    for required in [
        "run_safety_prefix(std::slice::from_ref(action))",
        "bounded_reference_node()",
        "authenticated_graph_state()",
    ] {
        assert!(
            replay.contains(required),
            "bounded graph no longer composes through {required}",
        );
    }

    let callsite_roster =
        include_str!("../cu/inv_088_global_summaries_are_not_account_local_proofs.rs");
    assert_eq!(
        callsite_roster
            .matches("Inv088EngineCallsite { owner:")
            .count(),
        50,
        "wrapper-to-engine transition class count drift",
    );
    assert!(callsite_roster.contains("certificate_disposition_classes,\n        [18, 16, 11, 5]"));
    assert!(callsite_roster.contains("actual, expected,"));

    let instruction_roster = include_str!("../public_sbf/inv_079_public_reachability_evidence.rs");
    assert!(instruction_roster.contains(
        "registry_roster, production_roster,\n        \"public instruction coverage registry must have exactly one row per production"
    ));
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
                && coverage.user_positions_closed != 0
                && coverage.liquidation_steps != 0,
            "reference-model run did not exercise liveness/value surfaces: {coverage:?}"
        );
    }
}
