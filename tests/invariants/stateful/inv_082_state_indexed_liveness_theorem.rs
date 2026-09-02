//! INV-082 - State-indexed liveness theorem.
//!
//! Normative obligation: every publicly reachable nonterminal state in each
//! lifecycle mode either is terminal already or has a constructible bounded
//! public action that decreases the mode-specific rank.
//!
//! Evidence in this file (F/I over public routes): the shared stateful model
//! drives a fixed public sequence that covers all trade routes, account
//! substitution rejection, mark movement, and complete-hint cranks. Its
//! permissionless-progress campaign recomputes the deployed state rank before
//! and after every successful public crank, and fails if every actionable
//! public candidate rejects or does not decrease rank. The follow-on exit
//! campaign then proves normal users can leave through public routes. This is a
//! bounded whole-route witness, not an exhaustive proof over every reachable
//! state.
//!
//! A bounded graph owner enumerates ten deterministic public prefixes across
//! two market configurations, records only successful lexicographically
//! rank-decreasing crank edges, and requires every observed actionable class
//! to reach rank zero. This is executable bounded-reachability evidence for
//! INV-071 and INV-082, but it does not quantify over unobserved lifecycle
//! classes or the complete public state space.
//!
//! A separate public regression fixes the generated model's lifecycle boundary:
//! an invalid certificate backed only by a Recovery leg is dispatchable for a
//! committed-state refresh, without accruing the frozen asset. The empty-hint
//! crank must make the certificate current while framing all economic state; an
//! apparent Recovery observation still rejects atomically, and the owner's
//! strict reduction remains live.
//!
//! The multi-domain loss-stale regression reaches three stale legs through
//! exactly three authenticated public mark pushes. On the fixed engine, the
//! selector consumes the per-domain whole-atom support that actually exists
//! and takes a strict progress edge; the old aggregate-before-rounding rule
//! classified the same state actionable while every crank returned
//! `LockActive`. Removing any one mark removes the counterexample, which keeps
//! this as a minimized public reachability witness rather than injected state.
//!
//! The close/reset composition matrix creates an active bankruptcy close on
//! asset 0 and an independent prior-epoch `ResetPending` leg on asset 1 through
//! public calls. It crosses all four trade routes, both close and reset sides,
//! and both transition landing orders. The first automatic crank must select
//! the higher-priority close continuation without mutating asset 1; bounded
//! later calls must clear and finalize the reset episode. Terminal owner-level
//! payouts must be identical across landing orders, with exact stock,
//! encumbrance, custody, supply, and CU checks. This closes one concrete
//! close-plus-lifecycle composition cell without claiming exhaustive
//! reachability over the remaining lifecycle graph.
//!
//! The adjacent Recovery matrix uses the same public worlds but shuts down the
//! reset asset before or after close creation. This preserves a prior-epoch
//! reset prerequisite inside Recovery while the independent close remains
//! active. It therefore checks the Recovery classifier/dispatcher boundary,
//! not merely another ResetPending quantity.

use super::*;
use crate::support::fuzz_model::{
    run_bounded_public_liveness_graph, run_close_recovery_overlap_probe,
    run_close_reset_overlap_probe, run_multileg_loss_stale_progress_regression,
};
use crate::support::v16_svm::{MarketConfig, V16Svm};
use percolator::{AssetLifecycleV16, POS_SCALE};
use percolator_prog::ix::CrankObservationHint;
use std::collections::{BTreeSet, VecDeque};

#[test]
fn v16_program_bounded_public_crank_graph_reaches_terminal_rank() {
    let evidence =
        run_bounded_public_liveness_graph().expect("INV-071/INV-082 bounded public graph");
    let coverage = evidence.coverage;

    assert_eq!(
        evidence.scenario_count, 10,
        "the bounded graph must retain its full public-prefix/configuration matrix"
    );
    assert!(
        coverage.crank_progress > 0,
        "the graph must contain successful rank-decreasing public cranks: {coverage:?}"
    );
    assert!(
        coverage.crank_rank_nodes.contains(&0),
        "every bounded campaign must observe terminal rank zero: {coverage:?}"
    );
    assert!(
        coverage.crank_rank_nodes.len() >= 3,
        "the graph must exercise multiple actionable rank classes: {coverage:?}"
    );

    let observed_components = coverage
        .crank_rank_component_seen
        .iter()
        .filter(|count| **count != 0)
        .count();
    assert!(
        observed_components >= 3,
        "the bounded graph must cover at least three independent rank components: {coverage:?}"
    );
    for (index, seen) in coverage.crank_rank_component_seen.iter().enumerate() {
        if *seen != 0 {
            assert!(
                coverage.crank_rank_component_reduced[index] != 0,
                "observed rank component {index} never had a public reducing edge: {coverage:?}"
            );
        }
    }

    for start in coverage
        .crank_rank_nodes
        .iter()
        .copied()
        .filter(|node| *node != 0)
    {
        let mut visited = BTreeSet::from([start]);
        let mut frontier = VecDeque::from([start]);
        while let Some(node) = frontier.pop_front() {
            for (_, next) in coverage
                .crank_rank_edges
                .iter()
                .filter(|(from, _)| *from == node)
            {
                if visited.insert(*next) {
                    frontier.push_back(*next);
                }
            }
        }
        assert!(
            visited.contains(&0),
            "observed actionable rank class {start:#08b} has no public path to zero: {coverage:?}"
        );
    }
}

#[test]
fn v16_program_multileg_loss_stale_account_has_permissionless_progress() {
    let coverage = run_multileg_loss_stale_progress_regression()
        .expect("a public multi-asset loss-stale account must retain a progressing crank");
    assert!(
        coverage.crank_progress != 0,
        "the directed public trace must execute a rank-decreasing crank: {coverage:?}"
    );
}

#[test]
fn v16_program_close_and_reset_overlap_has_bounded_terminal_schedule() {
    let evidence = run_close_reset_overlap_probe()
        .expect("INV-071/074/082 close-plus-reset public liveness matrix");

    assert_eq!(evidence.world_count, 32, "{evidence:?}");
    assert_eq!(evidence.route_worlds, [8; 4], "{evidence:?}");
    assert_eq!(evidence.close_orientation_worlds, [16; 2], "{evidence:?}");
    assert_eq!(evidence.reset_orientation_worlds, [16; 2], "{evidence:?}");
    assert_eq!(evidence.landing_order_worlds, [16; 2], "{evidence:?}");
    assert_eq!(evidence.simultaneous_class_worlds, 32, "{evidence:?}");
    assert_eq!(evidence.close_priority_worlds, 32, "{evidence:?}");
    assert_eq!(evidence.reset_completion_worlds, 32, "{evidence:?}");
    assert_eq!(evidence.recovery_worlds, 0, "{evidence:?}");
    assert_eq!(evidence.owner_exit_worlds, 32, "{evidence:?}");
    assert_ne!(evidence.total_owner_payout, 0, "{evidence:?}");
    assert_ne!(
        evidence.coverage.crank_rank_component_reduced[3], 0,
        "close work must have a strict public reducing edge: {evidence:?}"
    );
    assert_ne!(
        evidence.coverage.crank_rank_component_reduced[2], 0,
        "ResetPending work must have a strict public reducing edge: {evidence:?}"
    );
}

#[test]
fn v16_program_close_and_recovery_reset_overlap_has_bounded_terminal_schedule() {
    let evidence = run_close_recovery_overlap_probe()
        .expect("INV-071/074/082 close-plus-Recovery/reset public liveness matrix");

    assert_eq!(evidence.world_count, 32, "{evidence:?}");
    assert_eq!(evidence.route_worlds, [8; 4], "{evidence:?}");
    assert_eq!(evidence.close_orientation_worlds, [16; 2], "{evidence:?}");
    assert_eq!(evidence.reset_orientation_worlds, [16; 2], "{evidence:?}");
    assert_eq!(evidence.landing_order_worlds, [16; 2], "{evidence:?}");
    assert_eq!(evidence.simultaneous_class_worlds, 32, "{evidence:?}");
    assert_eq!(evidence.close_priority_worlds, 32, "{evidence:?}");
    assert_eq!(evidence.reset_completion_worlds, 32, "{evidence:?}");
    assert_eq!(evidence.recovery_worlds, 32, "{evidence:?}");
    assert_eq!(evidence.owner_exit_worlds, 32, "{evidence:?}");
    assert_ne!(evidence.total_owner_payout, 0, "{evidence:?}");
    assert_ne!(
        evidence.coverage.crank_rank_component_reduced[3], 0,
        "close work must remain higher-priority in Recovery: {evidence:?}"
    );
    assert_ne!(
        evidence.coverage.crank_rank_component_reduced[2], 0,
        "Recovery reset work must have a strict public reducing edge: {evidence:?}"
    );
    assert_eq!(
        evidence.coverage.lifecycle_updates, 32,
        "every world must publicly enter Recovery: {evidence:?}"
    );
}

#[test]
fn v16_program_public_sequence_has_rank_decreasing_progress_and_exit_witnesses() {
    let scenario = Scenario {
        seed: [0x82; 32],
        config: SmallMarketConfig {
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 2,
            max_abs_funding_e9_per_slot: 0,
            maintenance_fee_per_slot: 1,
        },
        actions: vec![
            Action::Deposit {
                actor: 0,
                amount: 250,
            },
            Action::Trade {
                route: TradeRoute::NoCpi,
                taker: 0,
                maker: 1,
                asset: 0,
                units: 1,
                fee_bps: 0,
                price_move_bps: 0,
                prefer_reduce: false,
            },
            Action::Trade {
                route: TradeRoute::Cpi,
                taker: 2,
                maker: 3,
                asset: 1,
                units: 1,
                fee_bps: 0,
                price_move_bps: 0,
                prefer_reduce: false,
            },
            Action::Trade {
                route: TradeRoute::BatchNoCpi,
                taker: 0,
                maker: 2,
                asset: 0,
                units: -1,
                fee_bps: 0,
                price_move_bps: 0,
                prefer_reduce: true,
            },
            Action::Trade {
                route: TradeRoute::BatchCpi,
                taker: 1,
                maker: 3,
                asset: 1,
                units: -1,
                fee_bps: 0,
                price_move_bps: 0,
                prefer_reduce: true,
            },
            Action::PushMark {
                asset: 0,
                dt: 2,
                move_bps: 500,
            },
            Action::Crank {
                actor: 0,
                hints: HintMode::Complete,
            },
            Action::SyncMaintenanceFee { actor: 0, dt: 2 },
            Action::AccountSubstitution {
                actor: 0,
                kind: SubstitutionKind::ForeignTradePortfolio,
            },
            Action::AccountSubstitution {
                actor: 1,
                kind: SubstitutionKind::ForeignDepositVault,
            },
            Action::AccountSubstitution {
                actor: 2,
                kind: SubstitutionKind::ForeignWithdrawVault,
            },
            Action::AccountSubstitution {
                actor: 3,
                kind: SubstitutionKind::ForeignCrankPortfolio,
            },
            Action::AccountSubstitution {
                actor: 0,
                kind: SubstitutionKind::MismatchedMatcherBinding,
            },
        ],
    };

    let coverage = run_scenario(&scenario).expect("INV-082 public liveness scenario");
    assert!(
        coverage.crank_progress > 0,
        "complete-hint public crank campaign must decrease a liveness rank: {coverage:?}"
    );
    assert!(
        coverage.user_positions_closed > 0,
        "normal-user exit campaign must close at least one public position: {coverage:?}"
    );
    assert!(
        coverage.liquidation_steps > 0 && coverage.liquidated_abs_q > 0,
        "independent liquidation liveness probe must make public progress: {coverage:?}"
    );
}

#[test]
fn v16_program_recovery_only_stale_certificate_retains_owner_exit() {
    let config = MarketConfig::default();
    let mut env = V16Svm::new([0x82; 32], config);
    env.trade_no_cpi(0, 1, 0, POS_SCALE as i128, config.initial_price, 0)
        .expect("open matched position");
    env.configure_permissionless_resolve(1_000, 100)
        .expect("configure public recovery policy");
    env.shutdown_asset(0, 1)
        .expect("enter asset Recovery through public route");

    let (_, market) = env.primary_market_state();
    assert_eq!(market.assets[0].lifecycle, AssetLifecycleV16::Recovery);
    assert_ne!(
        env.primary_portfolio(0).health_cert.cert_risk_epoch.get(),
        market.risk_epoch,
        "shutdown must invalidate the live certificate"
    );

    let market_before = env.market_data(false);
    let foreign_market_before = env.market_data(true);
    let portfolio_before = env.primary_portfolio(0);
    let tokens_before = env.all_token_account_data();
    let lamports_before = env.all_economic_account_lamports();
    env.crank(0, 1, vec![])
        .expect("Recovery-only stale certificate must have a committed-state refresh");
    let (_, refreshed_market) = env.primary_market_state();
    let portfolio_after_refresh = env.primary_portfolio(0);
    assert_eq!(
        portfolio_after_refresh.health_cert.cert_risk_epoch.get(),
        refreshed_market.risk_epoch,
        "Recovery refresh must consume the stale-certificate rank component"
    );
    assert_eq!(env.market_data(false), market_before);
    assert_eq!(env.market_data(true), foreign_market_before);
    assert_eq!(env.all_token_account_data(), tokens_before);
    assert_eq!(env.all_economic_account_lamports(), lamports_before);
    let mut normalized_before = portfolio_before;
    normalized_before.health_cert = portfolio_after_refresh.health_cert;
    assert_eq!(
        portfolio_after_refresh, normalized_before,
        "Recovery certificate refresh must frame every non-certificate portfolio field"
    );

    let portfolio_after_refresh = env.primary_portfolio_data(0);
    let empty_error = env
        .crank(0, 1, vec![])
        .expect_err("current Recovery account has no remaining permissionless work");
    assert!(
        empty_error.contains("Custom(22)") || empty_error.contains("custom program error: 0x16"),
        "unexpected current-account rejection: {empty_error}"
    );
    assert_eq!(env.market_data(false), market_before);
    assert_eq!(env.market_data(true), foreign_market_before);
    assert_eq!(env.primary_portfolio_data(0), portfolio_after_refresh);
    assert_eq!(env.all_token_account_data(), tokens_before);
    assert_eq!(env.all_economic_account_lamports(), lamports_before);

    let hinted_error = env
        .crank(
            0,
            1,
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 0,
            }],
        )
        .expect_err("Recovery-only hint cannot turn NoAction into successful work");
    assert!(
        hinted_error.contains("Custom(22)") || hinted_error.contains("custom program error: 0x16"),
        "unexpected Recovery-hint rejection: {hinted_error}"
    );
    assert_eq!(env.market_data(false), market_before);
    assert_eq!(env.market_data(true), foreign_market_before);
    assert_eq!(env.primary_portfolio_data(0), portfolio_after_refresh);
    assert_eq!(env.all_token_account_data(), tokens_before);
    assert_eq!(env.all_economic_account_lamports(), lamports_before);

    env.trade_no_cpi(0, 1, 0, -(POS_SCALE as i128), config.initial_price, 0)
        .expect("owner strict reduction remains live in Recovery");
    let (_, after) = env.primary_market_state();
    assert_eq!(after.assets[0].oi_eff_long_q, 0);
    assert_eq!(after.assets[0].oi_eff_short_q, 0);
}
