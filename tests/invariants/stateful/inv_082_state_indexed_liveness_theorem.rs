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

use super::*;
use crate::support::fuzz_model::run_bounded_public_liveness_graph;
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
    assert!(
        coverage
            .known_blocker_hits
            .iter()
            .chain(coverage.known_blocker_exit_locks.iter())
            .all(|hits| *hits == 0),
        "INV-082 witness must not rely on known-blocker quarantine: {coverage:?}"
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
