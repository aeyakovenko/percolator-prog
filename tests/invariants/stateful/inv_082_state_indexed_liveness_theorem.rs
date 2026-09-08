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
//!
//! The environmental-completion matrix retains publicly funded capital-only
//! checkpoints across both stale-resolution and force-close Clock deadlines.
//! It checks concrete wrapper admission, exact principal payout, and a finite
//! mode/wait/unpaid-principal rank with only a keeper fee-payer signature after
//! policy setup. Empty/current hints and both terminal entrypoints are exercised;
//! economic completion leaves the materialized, signer-gated administrative tail.
//! This finite environment slice does not cover exposure, junior claims, Recovery,
//! overlapping close/reset work, unavailable feeds, or maximum account shapes.
//!
//! The stale-exposure continuation matrix adds the next concrete wrapper slice for
//! INV-071/072/073/078/082. Each public trade transport leaves a funded position behind an
//! independently advanced oracle epoch and a pending authenticated mark. Empty, incomplete,
//! duplicate, and out-of-range hint words reject with a full economic frame before a canonical
//! same-Clock retry consumes a decoded refresh rank. After stale resolution, both terminal rails
//! reject atomically inside the owner window, then keeper-only calls with adversarial hint words
//! drain every funded account at the exact public deadline. This composes stale-account refresh
//! with terminal disposition; it does not add an engine selector model, a receipt/resource-failure
//! topology, or a supported-maximum claim.
//!
//! The partial-receipt rank witness reuses the shared public receipt seed. A keeper-only expiry
//! release lowers account-referenced fresh-backing work, then a partial top-up lowers only unpaid
//! value. The receipt remains nonterminal, and admitted duplicate claims remain exact no-ops.

use super::*;
use crate::support::fuzz_model::{
    assert_public_encumbrance_census, assert_public_stock_census, execute_trade_route,
    run_bounded_public_liveness_graph, run_close_recovery_overlap_probe,
    run_close_reset_overlap_probe, run_multileg_loss_stale_progress_regression,
};
use crate::support::v16_svm::{
    MarketConfig, PublicTraceStep, V16Svm, PRIMARY_ACTOR_COUNT, TX_CU_LIMIT,
};
use percolator::{active_bitmap_is_empty, AssetLifecycleV16, MarketModeV16, POS_SCALE};
use percolator_prog::error::PercolatorError;
use percolator_prog::ix::CrankObservationHint;
use solana_sdk::{account::Account, pubkey::Pubkey, signature::Signer};
use std::collections::{BTreeSet, VecDeque};

type Inv082AccountFrame = Vec<(Pubkey, Option<Account>)>;

fn inv082_account_frame(env: &V16Svm) -> Inv082AccountFrame {
    let mut keys: BTreeSet<_> = env
        .all_economic_account_lamports()
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    keys.extend(env.actors.iter().map(|actor| actor.signer.pubkey()));
    keys.insert(env.foreign_actor.signer.pubkey());
    keys.into_iter()
        .map(|key| (key, env.svm.get_account(&key)))
        .collect()
}

fn inv082_assert_frame(env: &V16Svm, before: &Inv082AccountFrame, allowed: &[Pubkey]) {
    for (key, account) in before {
        if !allowed.contains(key) {
            assert_eq!(
                &env.svm.get_account(key),
                account,
                "unexpected mutation: {key}"
            );
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Inv082CapitalCompletion {
    rank: (u8, u64, u128),
    economic_terminal: bool,
    administrative_portfolios: u64,
}

fn inv082_capital_completion(
    env: &V16Svm,
    deposits: [u128; PRIMARY_ACTOR_COUNT],
    destinations_before: [u64; PRIMARY_ACTOR_COUNT],
) -> Inv082CapitalCompletion {
    let (cfg, market) = env.primary_market_state();
    let mut unpaid = 0u128;
    for actor in 0..PRIMARY_ACTOR_COUNT {
        let account = env.primary_portfolio(actor);
        let paid = env
            .token_amount(env.actors[actor].destination_token)
            .checked_sub(destinations_before[actor])
            .expect("destination principal cannot decrease");
        assert_eq!(account.capital.get() + u128::from(paid), deposits[actor]);
        unpaid += account.capital.get();
        assert_eq!(account.pnl.get(), 0);
        assert_eq!(account.reserved_pnl.get(), 0);
        assert_eq!(account.fee_credits.get(), 0);
        assert_eq!(account.cancel_deposit_escrow.get(), 0);
        assert_eq!(account.stale_state, 0);
        assert_eq!(account.b_stale_state, 0);
        assert_eq!(account.rebalance_lock, 0);
        assert_eq!(account.liquidation_lock, 0);
        assert!(active_bitmap_is_empty(
            account.active_bitmap.map(|word| word.get())
        ));
        assert!(account
            .source_domains
            .iter()
            .all(|source| !source.is_occupied()));
        let receipt = account.resolved_payout_receipt.try_to_runtime().unwrap();
        assert!(!receipt.present || receipt.finalized);
        assert!(!account.close_progress.try_to_runtime().unwrap().active);
    }
    assert_eq!(market.c_tot, unpaid);
    assert_eq!(market.vault, unpaid);
    assert_eq!(u128::from(env.token_amount(env.vault)), unpaid);
    assert_eq!(market.insurance, 0);
    assert_eq!(
        market.materialized_portfolio_count,
        PRIMARY_ACTOR_COUNT as u64
    );
    let economic_terminal = market.mode == MarketModeV16::Resolved && unpaid == 0;
    let rank = match market.mode {
        MarketModeV16::Live => (
            2,
            (cfg.last_good_oracle_slot + cfg.permissionless_resolve_stale_slots)
                .saturating_sub(env.current_slot()),
            unpaid,
        ),
        MarketModeV16::Resolved if !economic_terminal => (
            1,
            (market.resolved_slot + cfg.force_close_delay_slots).saturating_sub(env.current_slot()),
            unpaid,
        ),
        MarketModeV16::Resolved => (0, 0, 0),
        mode => panic!("unconstructed mode in capital-only completion slice: {mode:?}"),
    };
    Inv082CapitalCompletion {
        rank,
        economic_terminal,
        administrative_portfolios: market.materialized_portfolio_count,
    }
}

fn inv082_completion_step_is_valid(
    before: Inv082CapitalCompletion,
    after: Inv082CapitalCompletion,
    reports_terminal: bool,
) -> bool {
    after.rank < before.rank && reports_terminal == after.economic_terminal
}

fn inv082_keeper_only(step: &PublicTraceStep, unavailable: &BTreeSet<Pubkey>) -> bool {
    !unavailable.contains(&step.fee_payer)
        && step.transaction_signers == vec![step.fee_payer]
        && step
            .accounts
            .iter()
            .all(|meta| !meta.is_signer || meta.key == step.fee_payer)
}

fn inv082_assert_rejected(error: String, expected: PercolatorError) {
    assert!(
        error.contains(&format!("Custom({})", expected as u32)),
        "unexpected wrapper rejection: {error}"
    );
}

#[test]
fn v16_program_environmental_completion_prefixes_preserve_permissionless_exit() {
    let deposits = [1, 17, 123, 1_000, 2_001];
    let mut worlds = 0;
    let mut attempts = 0;
    let mut progressing = 0;
    let mut rejected = 0;
    let mut waits = 0;
    let mut terminal = 0;
    let mut max_cu = 0;
    for (stale_slots, force_close_delay) in [(2u64, 1u64), (5, 3)] {
        for resolution_boundary in 0..3u64 {
            for payout_boundary in 0..3u64 {
                for complete_hints in [false, true] {
                    let mut seed = [0x82; 32];
                    seed[0] = worlds;
                    let mut env = V16Svm::new(
                        seed,
                        MarketConfig {
                            actor_deposits: deposits,
                            ..MarketConfig::default()
                        },
                    );
                    env.configure_permissionless_resolve(stale_slots, force_close_delay)
                        .expect("public terminal-policy setup");
                    let destinations_before = std::array::from_fn(|actor| {
                        env.token_amount(env.actors[actor].destination_token)
                    });
                    let supply_before = env.token_supply_observed();
                    let maturity = env.primary_market_state().0.last_good_oracle_slot + stale_slots;
                    let mut unavailable: BTreeSet<_> = env
                        .actors
                        .iter()
                        .map(|actor| actor.signer.pubkey())
                        .collect();
                    unavailable.insert(Pubkey::new_from_array(
                        env.primary_market_state().0.marketauth,
                    ));
                    env.begin_public_trace();
                    env.warp_to_slot(maturity - 1 + resolution_boundary);
                    let mut before = inv082_capital_completion(&env, deposits, destinations_before);
                    assert_eq!(before.rank.0, 2);
                    assert!(!before.economic_terminal);
                    assert!(!inv082_completion_step_is_valid(before, before, false));
                    if resolution_boundary == 0 {
                        let frame = inv082_account_frame(&env);
                        let slot = env.current_slot();
                        inv082_assert_rejected(
                            env.resolve_stale_permissionless(slot)
                                .expect_err("not mature yet"),
                            PercolatorError::OracleStale,
                        );
                        inv082_assert_frame(&env, &frame, &[]);
                        env.warp_to_slot(maturity);
                        let ready = inv082_capital_completion(&env, deposits, destinations_before);
                        assert_eq!(before.rank.1, 1, "only a named finite wait is admitted");
                        assert!(ready.rank < before.rank);
                        before = ready;
                        waits += 1;
                    }
                    let frame = inv082_account_frame(&env);
                    let slot = env.current_slot();
                    env.resolve_stale_permissionless(slot)
                        .expect("mature public resolution");
                    let after = inv082_capital_completion(&env, deposits, destinations_before);
                    assert_eq!(after.rank.0, 1);
                    assert!(inv082_completion_step_is_valid(before, after, false));
                    inv082_assert_frame(&env, &frame, &[env.market]);

                    let payout_maturity =
                        env.primary_market_state().1.resolved_slot + force_close_delay;
                    env.warp_to_slot(payout_maturity - 1 + payout_boundary);
                    let hints = if complete_hints {
                        (0..crate::support::v16_svm::ASSET_COUNT)
                            .map(|asset| CrankObservationHint {
                                asset_index: asset as u16,
                                oracle_accounts: env.primary_profile(asset).oracle_leg_count,
                            })
                            .collect::<Vec<_>>()
                    } else {
                        vec![]
                    };
                    if payout_boundary == 0 {
                        let waiting =
                            inv082_capital_completion(&env, deposits, destinations_before);
                        for use_crank in [true, false] {
                            let frame = inv082_account_frame(&env);
                            let result = if use_crank {
                                env.crank(0, env.current_slot(), hints.clone())
                            } else {
                                env.close_resolved_primary(0)
                            };
                            inv082_assert_rejected(
                                result.expect_err("owner window is not permissionless"),
                                PercolatorError::ExpectedSigner,
                            );
                            inv082_assert_frame(&env, &frame, &[]);
                        }
                        env.warp_to_slot(payout_maturity);
                        let ready = inv082_capital_completion(&env, deposits, destinations_before);
                        assert_eq!(waiting.rank.1, 1);
                        assert!(ready.rank < waiting.rank);
                        waits += 1;
                    }

                    // Alternate entrypoints and owner orders without owner signatures.
                    for index in 0..PRIMARY_ACTOR_COUNT {
                        let actor = if complete_hints {
                            PRIMARY_ACTOR_COUNT - 1 - index
                        } else {
                            index
                        };
                        let before = inv082_capital_completion(&env, deposits, destinations_before);
                        let frame = inv082_account_frame(&env);
                        let result = if index % 2 == usize::from(complete_hints) {
                            env.crank(actor, env.current_slot(), hints.clone())
                        } else {
                            env.close_resolved_primary(actor)
                        };
                        result.expect("one bounded permissionless principal payout per account");
                        let after = inv082_capital_completion(&env, deposits, destinations_before);
                        assert!(inv082_completion_step_is_valid(
                            before,
                            after,
                            after.economic_terminal
                        ));
                        if index == 0 {
                            assert!(!after.economic_terminal, "other funded claims remain");
                            assert!(!inv082_completion_step_is_valid(before, after, true));
                        }
                        assert_eq!(env.primary_portfolio(actor).capital.get(), 0);
                        inv082_assert_frame(
                            &env,
                            &frame,
                            &[
                                env.market,
                                env.actors[actor].portfolio,
                                env.actors[actor].destination_token,
                                env.vault,
                            ],
                        );
                        let frame = inv082_account_frame(&env);
                        for use_crank in [true, false] {
                            let retry = if use_crank {
                                env.crank(actor, env.current_slot(), hints.clone())
                            } else {
                                env.close_resolved_primary(actor)
                            };
                            inv082_assert_rejected(
                                retry.expect_err("paid account must not report progress again"),
                                PercolatorError::EngineNonProgress,
                            );
                            inv082_assert_frame(&env, &frame, &[]);
                        }
                    }
                    let done = inv082_capital_completion(&env, deposits, destinations_before);
                    assert!(done.economic_terminal);
                    assert_eq!(done.rank, (0, 0, 0));
                    assert_eq!(done.administrative_portfolios, PRIMARY_ACTOR_COUNT as u64);
                    assert_eq!(env.token_supply_observed(), supply_before);
                    let trace = env.finish_public_trace();
                    trace
                        .validate_public_execution()
                        .expect("public, rollback-exact continuation");
                    for step in &trace.steps {
                        assert!(
                            inv082_keeper_only(step, &unavailable),
                            "owner/admin shortcut"
                        );
                        let mut signed_shortcut = step.clone();
                        signed_shortcut
                            .transaction_signers
                            .push(env.actors[0].signer.pubkey());
                        assert!(!inv082_keeper_only(&signed_shortcut, &unavailable));
                        signed_shortcut.fee_payer = env.actors[0].signer.pubkey();
                        signed_shortcut.transaction_signers = vec![signed_shortcut.fee_payer];
                        assert!(!inv082_keeper_only(&signed_shortcut, &unavailable));
                        if step.succeeded {
                            max_cu = max_cu.max(step.compute_units.expect("landed instruction CU"));
                        }
                    }
                    attempts += trace.steps.len();
                    progressing += trace.steps.iter().filter(|step| step.succeeded).count();
                    rejected += trace.steps.iter().filter(|step| !step.succeeded).count();
                    terminal += usize::from(done.economic_terminal);
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(worlds, 36);
    assert_eq!(progressing, 36 * (1 + PRIMARY_ACTOR_COUNT));
    assert_eq!(rejected, 36 * 2 * PRIMARY_ACTOR_COUNT + 12 + 24);
    assert_eq!(attempts, progressing + rejected);
    assert_eq!(waits, 24);
    assert_eq!(terminal, 36);
    assert!(max_cu < TX_CU_LIMIT);
    eprintln!("INV-082 constructed={worlds} attempted={attempts} progressing={progressing} rejected={rejected} finite_waits={waits} terminal={terminal} signer_set=keeper-fee-payer-only administrative_portfolios_per_world={PRIMARY_ACTOR_COUNT} max_cu={max_cu}");
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Inv082RefreshRank {
    mark_distance: u64,
    selected_asset_clock_distance: u64,
    stale_account_flags: u64,
    stale_certificate: u8,
}

fn inv082_certificate_is_current(env: &V16Svm, actor: usize) -> bool {
    let account = env.primary_portfolio(actor);
    let Ok(cert) = account.health_cert.try_to_runtime() else {
        return false;
    };
    let group = env.primary_market_state().1;
    cert.valid
        && cert.cert_oracle_epoch == group.oracle_epoch
        && cert.cert_funding_epoch == group.funding_epoch
        && cert.cert_risk_epoch == group.risk_epoch
        && cert.cert_asset_set_epoch == group.asset_set_epoch
        && cert.active_bitmap_at_cert == account.active_bitmap.map(|word| word.get())
}

fn inv082_refresh_rank(env: &V16Svm, actor: usize, asset: usize) -> Inv082RefreshRank {
    let account = env.primary_portfolio(actor);
    let group = env.primary_market_state().1;
    let engine_asset = group.assets[asset];
    let profile = env.primary_profile(asset);
    let mark_distance = engine_asset
        .raw_oracle_target_price
        .abs_diff(engine_asset.effective_price)
        .max(profile.mark_ewma_e6.abs_diff(engine_asset.effective_price));
    let stale_certificate = u8::from(!inv082_certificate_is_current(env, actor));
    Inv082RefreshRank {
        mark_distance,
        selected_asset_clock_distance: if stale_certificate != 0 {
            env.current_slot().saturating_sub(engine_asset.slot_last)
        } else {
            0
        },
        stale_account_flags: u64::from(account.stale_state)
            + u64::from(account.b_stale_state)
            + u64::from(account.liquidation_lock)
            + u64::from(account.rebalance_lock),
        stale_certificate,
    }
}

fn inv082_economically_terminal(env: &V16Svm, actor: usize) -> bool {
    let account = env.primary_portfolio(actor);
    let group = env.primary_market_state().1;
    let Ok(receipt) = account.resolved_payout_receipt.try_to_runtime() else {
        return false;
    };
    let Ok(close) = account.close_progress.try_to_runtime() else {
        return false;
    };
    group.mode == MarketModeV16::Resolved
        && account.capital.get() == 0
        && account.pnl.get() == 0
        && account.reserved_pnl.get() == 0
        && account.fee_credits.get() == 0
        && account.cancel_deposit_escrow.get() == 0
        && account.stale_state == 0
        && account.b_stale_state == 0
        && account.rebalance_lock == 0
        && account.liquidation_lock == 0
        && account.last_fee_slot.get() == group.resolved_slot
        && account.health_cert.valid == 0
        && active_bitmap_is_empty(account.active_bitmap.map(|word| word.get()))
        && account
            .source_domains
            .iter()
            .all(|source| !source.is_occupied())
        && (!receipt.present || receipt.finalized)
        && (!close.active || (close.finalized && close.residual_remaining == 0))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Inv082TerminalRank {
    active_legs: u64,
    fresh_backed_sources: u64,
    occupied_sources: u64,
    nonzero_pnl_accounts: u64,
    unfinished_receipts: u64,
    nonterminal_accounts: u64,
    outstanding_value: u128,
}

fn inv082_terminal_rank(env: &V16Svm) -> Inv082TerminalRank {
    let market = env.primary_market_state().1;
    assert_eq!(market.mode, MarketModeV16::Resolved);
    let mut rank = Inv082TerminalRank::default();
    for actor in 0..PRIMARY_ACTOR_COUNT {
        let account = env.primary_portfolio(actor);
        rank.active_legs += account
            .legs
            .iter()
            .filter_map(|leg| leg.try_to_runtime().ok())
            .filter(|leg| leg.active)
            .count() as u64;
        for source in account
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
        {
            rank.occupied_sources += 1;
            let bucket = &market.source_backing_buckets[source.domain.get() as usize];
            // Expiry can release backing without removing its account source or paying a claim.
            // Count only account-referenced backing, not signer-gated provider cleanup.
            rank.fresh_backed_sources += u64::from(
                bucket.status == percolator::BackingBucketStatusV16::Fresh
                    && (bucket.fresh_unliened_backing_num != 0
                        || bucket.valid_liened_backing_num != 0),
            );
        }
        rank.nonzero_pnl_accounts += u64::from(account.pnl.get() != 0);
        if let Ok(receipt) = account.resolved_payout_receipt.try_to_runtime() {
            if receipt.present && !receipt.finalized {
                rank.unfinished_receipts += 1;
                let unpaid_claim = receipt
                    .terminal_positive_claim_face
                    .checked_sub(receipt.paid_effective)
                    .expect("terminal receipt paid beyond its face value");
                rank.outstanding_value = rank
                    .outstanding_value
                    .checked_add(unpaid_claim)
                    .expect("terminal rank overflowed");
            }
        }
        rank.nonterminal_accounts += u64::from(!inv082_economically_terminal(env, actor));
        for amount in [
            account.capital.get(),
            account.pnl.get().unsigned_abs(),
            account.reserved_pnl.get(),
        ] {
            rank.outstanding_value = rank
                .outstanding_value
                .checked_add(amount)
                .expect("terminal rank overflowed");
        }
    }
    rank
}

#[test]
fn v16_program_partial_receipt_topup_decreases_only_outstanding_value_rank() {
    const CLAIMANT: usize = 0;
    const BACKED_OWNER: usize = 2;
    let mut env = crate::support::fuzz_model::public_resolved_receipt_seed([100, 100], 14).unwrap();
    let receipt = |env: &V16Svm| {
        env.primary_portfolio(CLAIMANT)
            .resolved_payout_receipt
            .try_to_runtime()
            .unwrap()
    };
    let initial = receipt(&env);
    assert!(initial.present && !initial.finalized);
    assert!(initial.paid_effective < initial.terminal_positive_claim_face);
    assert!(!inv082_economically_terminal(&env, CLAIMANT));
    let supply_before = env.token_supply_observed();
    let unavailable: BTreeSet<_> = env
        .actors
        .iter()
        .map(|actor| actor.signer.pubkey())
        .chain(std::iter::once(Pubkey::new_from_array(
            env.primary_market_state().0.marketauth,
        )))
        .collect();
    env.begin_public_trace();

    let waiting = inv082_terminal_rank(&env);
    assert_eq!(waiting.fresh_backed_sources, 2);
    let frame = inv082_account_frame(&env);
    env.claim_resolved_payout_topup_primary(CLAIMANT)
        .expect("a currently exhausted claim is an admitted no-op");
    inv082_assert_frame(&env, &frame, &[]);
    assert_eq!(inv082_terminal_rank(&env), waiting);
    assert_ne!(waiting, Inv082TerminalRank::default());

    // Reuse the receipt owner's public expiry prerequisite, without its owner signature.
    assert_eq!(env.current_slot(), 12);
    env.warp_to_slot(13);
    assert_eq!(inv082_terminal_rank(&env), waiting);
    let frame = inv082_account_frame(&env);
    env.close_resolved_primary(BACKED_OWNER)
        .expect("keeper releases the first expired backing domain");
    inv082_assert_frame(
        &env,
        &frame,
        &[env.market, env.actors[BACKED_OWNER].portfolio],
    );
    let released = inv082_terminal_rank(&env);
    assert_eq!(
        released,
        Inv082TerminalRank {
            fresh_backed_sources: 1,
            ..waiting
        }
    );
    assert!(released < waiting);
    assert_eq!(receipt(&env), initial);
    assert_public_stock_census("INV-082 receipt backing release", &env).unwrap();
    assert_public_encumbrance_census("INV-082 receipt backing release", &env).unwrap();

    let before = inv082_terminal_rank(&env);
    let frame = inv082_account_frame(&env);
    let destination = env.actors[CLAIMANT].destination_token;
    let destination_before = env.token_amount(destination);
    let vault_before = env.primary_market_state().1.vault;
    env.claim_resolved_payout_topup_primary(CLAIMANT)
        .expect("keeper receives the newly payable partial claim");
    let paid = u128::from(
        env.token_amount(destination)
            .checked_sub(destination_before)
            .unwrap(),
    );
    let partial = receipt(&env);
    assert!(paid > 0);
    assert!(partial.present && !partial.finalized);
    assert_eq!(
        partial.terminal_positive_claim_face,
        initial.terminal_positive_claim_face
    );
    assert_eq!(partial.paid_effective, initial.paid_effective + paid);
    assert!(partial.paid_effective < partial.terminal_positive_claim_face);
    let after = inv082_terminal_rank(&env);
    assert_eq!(
        after,
        Inv082TerminalRank {
            outstanding_value: before.outstanding_value.checked_sub(paid).unwrap(),
            ..before
        },
        "partial payout must consume only the value lane, without clearing a receipt or account"
    );
    assert!(after < before);
    assert!(!inv082_economically_terminal(&env, CLAIMANT));
    assert_ne!(after, Inv082TerminalRank::default());
    assert_eq!(env.primary_market_state().1.vault, vault_before - paid);
    assert_eq!(u128::from(env.token_amount(env.vault)), vault_before - paid);
    inv082_assert_frame(
        &env,
        &frame,
        &[
            env.market,
            env.actors[CLAIMANT].portfolio,
            destination,
            env.vault,
        ],
    );
    assert_public_stock_census("INV-082 partial receipt rank progress", &env).unwrap();
    assert_public_encumbrance_census("INV-082 partial receipt rank progress", &env).unwrap();

    let frame = inv082_account_frame(&env);
    env.claim_resolved_payout_topup_primary(CLAIMANT)
        .expect("same-Clock paid-rate retry is an admitted no-op, not another rank edge");
    inv082_assert_frame(&env, &frame, &[]);
    assert_eq!(inv082_terminal_rank(&env), after);
    assert!(!inv082_economically_terminal(&env, CLAIMANT));
    assert_eq!(env.token_supply_observed(), supply_before);
    assert_eq!(
        env.primary_market_state().1.materialized_portfolio_count,
        PRIMARY_ACTOR_COUNT as u64
    );
    let trace = env.finish_public_trace();
    trace.validate_public_execution().unwrap();
    assert_eq!(trace.steps.len(), 4);
    for step in &trace.steps {
        assert!(step.succeeded && inv082_keeper_only(step, &unavailable));
        assert!(step.compute_units.unwrap() < TX_CU_LIMIT);
    }
    eprintln!("INV-082 partial receipt: keeper_calls=4 rank_edges=2 exact_noops=2 paid={paid} remaining_claim={}", partial.terminal_positive_claim_face - partial.paid_effective);
}

#[derive(Clone, Copy, Debug)]
enum Inv082TerminalRail {
    Crank,
    Close,
}

fn inv082_terminal_hints(word: usize) -> Vec<CrankObservationHint> {
    match word % 4 {
        0 => vec![],
        1 => vec![
            CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 0,
            },
            CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 0,
            },
        ],
        2 => vec![CrankObservationHint {
            asset_index: u16::MAX,
            oracle_accounts: u8::MAX,
        }],
        _ => vec![CrankObservationHint {
            asset_index: 1,
            oracle_accounts: u8::MAX,
        }],
    }
}

#[test]
fn v16_program_stale_exposure_refresh_retains_keeper_only_terminal_progress() {
    const TARGET: usize = 0;
    const TARGET_COUNTERPARTY: usize = 1;
    const ORACLE_ACTOR: usize = 2;
    const ORACLE_COUNTERPARTY: usize = 3;
    const TARGET_ASSET: u16 = 0;
    const ORACLE_ASSET: u16 = 1;
    const PRICE: u64 = 100;
    const FAVORABLE_MARK: u64 = 105;
    const INDEPENDENT_MARK: u64 = 95;
    const STALE_SLOTS: u64 = 4;
    const OWNER_WINDOW: u64 = 3;
    const REFRESH_SLOT: u64 = 3;
    const SWEEP_BOUND: usize = 16;
    const DEPOSITS: [u128; PRIMARY_ACTOR_COUNT] = [1_000, 1_000, 1_000, 1_000, 17];

    let routes = [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ];
    let favorable_delta = u128::from(FAVORABLE_MARK - PRICE);
    let independent_delta = u128::from(PRICE - INDEPENDENT_MARK);
    let expected_payouts = [
        u64::try_from(DEPOSITS[TARGET].checked_add(favorable_delta).unwrap()).unwrap(),
        u64::try_from(
            DEPOSITS[TARGET_COUNTERPARTY]
                .checked_sub(favorable_delta)
                .unwrap(),
        )
        .unwrap(),
        u64::try_from(
            DEPOSITS[ORACLE_ACTOR]
                .checked_sub(independent_delta)
                .unwrap(),
        )
        .unwrap(),
        u64::try_from(
            DEPOSITS[ORACLE_COUNTERPARTY]
                .checked_add(independent_delta)
                .unwrap(),
        )
        .unwrap(),
        u64::try_from(DEPOSITS[4]).unwrap(),
    ];
    let mut worlds = 0usize;
    let mut refresh_progress = 0usize;
    let mut refresh_rejections = 0usize;
    let mut terminal_progress = 0usize;
    let mut terminal_rejections = 0usize;
    let mut rail_successes = [0usize; 2];
    let mut successful_hint_words = BTreeSet::new();
    let mut max_cu = 0u64;

    for (route_index, route) in routes.into_iter().enumerate() {
        for reverse in [false, true] {
            let mut seed = [0x82; 32];
            seed[0] ^= route_index as u8;
            seed[1] ^= u8::from(reverse);
            let mut env = V16Svm::new(
                seed,
                MarketConfig {
                    initial_price: PRICE,
                    max_price_move_bps_per_slot: 500,
                    max_accrual_dt_slots: 4,
                    max_abs_funding_e9_per_slot: 0,
                    maintenance_fee_per_slot: 0,
                    actor_deposits: DEPOSITS,
                    ..MarketConfig::default()
                },
            );
            env.configure_permissionless_resolve(STALE_SLOTS, OWNER_WINDOW)
                .expect("configure public stale-resolution policy");
            let destinations_before: [u64; PRIMARY_ACTOR_COUNT] =
                std::array::from_fn(|actor| env.token_amount(env.actors[actor].destination_token));
            let supply_before = env.token_supply_observed();

            execute_trade_route(
                &mut env,
                route,
                TARGET,
                TARGET_COUNTERPARTY,
                TARGET_ASSET,
                POS_SCALE as i128,
                PRICE,
                0,
            )
            .unwrap_or_else(|error| panic!("{route:?}/{reverse}: target trade: {error}"));
            env.trade_no_cpi(
                ORACLE_ACTOR,
                ORACLE_COUNTERPARTY,
                ORACLE_ASSET,
                POS_SCALE as i128,
                PRICE,
                0,
            )
            .unwrap_or_else(|error| panic!("{route:?}/{reverse}: independent trade: {error}"));

            env.warp_to_slot(1);
            env.push_auth_mark(ORACLE_ASSET, 1, INDEPENDENT_MARK)
                .expect("publish independent authenticated mark");
            env.crank(
                ORACLE_ACTOR,
                1,
                vec![CrankObservationHint {
                    asset_index: ORACLE_ASSET,
                    oracle_accounts: 0,
                }],
            )
            .expect("advance an independent oracle epoch");
            let stale_market = env.primary_market_state().1;
            assert!(
                !inv082_certificate_is_current(&env, TARGET)
                    && env
                        .primary_portfolio(TARGET)
                        .health_cert
                        .cert_oracle_epoch
                        .get()
                        < stale_market.oracle_epoch,
                "{route:?}/{reverse}: target checkpoint must be stale before its own mark"
            );

            env.warp_to_slot(2);
            env.push_auth_mark(TARGET_ASSET, 2, FAVORABLE_MARK)
                .expect("publish target authenticated mark");
            env.warp_to_slot(REFRESH_SLOT);
            let stale_rank = inv082_refresh_rank(&env, TARGET, TARGET_ASSET as usize);
            assert!(stale_rank.mark_distance > 0 && stale_rank.stale_certificate != 0);
            assert_public_stock_census("INV-082 stale exposure checkpoint", &env).unwrap();
            assert_public_encumbrance_census("INV-082 stale exposure checkpoint", &env).unwrap();

            env.begin_public_trace();
            let incomplete_words = [
                (vec![], PercolatorError::EngineNonProgress),
                (
                    vec![CrankObservationHint {
                        asset_index: ORACLE_ASSET,
                        oracle_accounts: 0,
                    }],
                    PercolatorError::EngineNonProgress,
                ),
                (
                    vec![
                        CrankObservationHint {
                            asset_index: TARGET_ASSET,
                            oracle_accounts: 0,
                        },
                        CrankObservationHint {
                            asset_index: TARGET_ASSET,
                            oracle_accounts: 0,
                        },
                    ],
                    PercolatorError::InvalidInstruction,
                ),
                (
                    vec![CrankObservationHint {
                        asset_index: u16::MAX,
                        oracle_accounts: 0,
                    }],
                    PercolatorError::InvalidInstruction,
                ),
            ];
            for (hints, expected_error) in incomplete_words {
                let frame = inv082_account_frame(&env);
                let error = env
                    .crank(TARGET, REFRESH_SLOT, hints)
                    .expect_err("incomplete or malformed Live hint word must reject");
                inv082_assert_rejected(error, expected_error);
                inv082_assert_frame(&env, &frame, &[]);
                assert_eq!(
                    inv082_refresh_rank(&env, TARGET, TARGET_ASSET as usize),
                    stale_rank,
                    "{route:?}/{reverse}: rejected hint word changed refresh rank"
                );
                refresh_rejections += 1;
            }

            for step in 0..4 {
                if inv082_certificate_is_current(&env, TARGET)
                    && inv082_refresh_rank(&env, TARGET, TARGET_ASSET as usize).mark_distance == 0
                {
                    break;
                }
                let before = inv082_refresh_rank(&env, TARGET, TARGET_ASSET as usize);
                let frame = inv082_account_frame(&env);
                let success = env
                    .crank(
                        TARGET,
                        REFRESH_SLOT,
                        vec![CrankObservationHint {
                            asset_index: TARGET_ASSET,
                            oracle_accounts: 0,
                        }],
                    )
                    .unwrap_or_else(|error| {
                        panic!("{route:?}/{reverse}: canonical refresh step {step}: {error}")
                    });
                max_cu = max_cu.max(success.compute_units);
                let after = inv082_refresh_rank(&env, TARGET, TARGET_ASSET as usize);
                assert!(
                    after < before,
                    "{route:?}/{reverse}: successful refresh did not lower decoded rank: {before:?} -> {after:?}"
                );
                inv082_assert_frame(&env, &frame, &[env.market, env.actors[TARGET].portfolio]);
                assert_public_stock_census("INV-082 canonical stale refresh", &env).unwrap();
                assert_public_encumbrance_census("INV-082 canonical stale refresh", &env).unwrap();
                refresh_progress += 1;
            }
            let refreshed_rank = inv082_refresh_rank(&env, TARGET, TARGET_ASSET as usize);
            assert_eq!(
                refreshed_rank,
                Inv082RefreshRank {
                    mark_distance: 0,
                    selected_asset_clock_distance: 0,
                    stale_account_flags: 0,
                    stale_certificate: 0,
                },
                "{route:?}/{reverse}: canonical retry did not reach a current fixed point"
            );
            assert_eq!(
                env.primary_market_state().1.assets[TARGET_ASSET as usize].effective_price,
                FAVORABLE_MARK
            );

            // Resolution may snapshot only after every exposed counterparty has consumed the
            // already-authenticated mark epochs. Keep that prerequisite permissionless and at the
            // same Clock rather than using an owner action or relying on terminal-mode magic.
            for (actor, asset) in [
                (TARGET_COUNTERPARTY, TARGET_ASSET),
                (ORACLE_ACTOR, ORACLE_ASSET),
                (ORACLE_COUNTERPARTY, ORACLE_ASSET),
            ] {
                for step in 0..4 {
                    if inv082_certificate_is_current(&env, actor) {
                        break;
                    }
                    let before = inv082_refresh_rank(&env, actor, asset as usize);
                    let frame = inv082_account_frame(&env);
                    let success = env
                        .crank(
                            actor,
                            REFRESH_SLOT,
                            vec![CrankObservationHint {
                                asset_index: asset,
                                oracle_accounts: 0,
                            }],
                        )
                        .unwrap_or_else(|error| {
                            panic!(
                                "{route:?}/{reverse}: exposed peer {actor} refresh step {step}: {error}"
                            )
                        });
                    let after = inv082_refresh_rank(&env, actor, asset as usize);
                    assert!(
                        after < before,
                        "{route:?}/{reverse}: peer {actor} refresh did not lower rank: {before:?} -> {after:?}"
                    );
                    inv082_assert_frame(&env, &frame, &[env.market, env.actors[actor].portfolio]);
                    assert_public_stock_census("INV-082 stale peer refresh", &env).unwrap();
                    assert_public_encumbrance_census("INV-082 stale peer refresh", &env).unwrap();
                    max_cu = max_cu.max(success.compute_units);
                    refresh_progress += 1;
                }
                assert!(
                    inv082_certificate_is_current(&env, actor),
                    "{route:?}/{reverse}: exposed peer {actor} did not become current"
                );
            }

            let resolution_maturity =
                env.primary_market_state().0.last_good_oracle_slot + STALE_SLOTS;
            let resolution = env
                .resolve_stale_permissionless(resolution_maturity)
                .unwrap_or_else(|error| {
                    panic!("{route:?}/{reverse}: stale resolution at maturity: {error}")
                });
            max_cu = max_cu.max(resolution.compute_units);
            let resolved = env.primary_market_state().1;
            assert_eq!(resolved.mode, MarketModeV16::Resolved);
            let payout_maturity = resolved.resolved_slot + OWNER_WINDOW;

            env.warp_to_slot(payout_maturity - 1);
            for rail in [Inv082TerminalRail::Crank, Inv082TerminalRail::Close] {
                let frame = inv082_account_frame(&env);
                let result = match rail {
                    Inv082TerminalRail::Crank => env.crank(
                        TARGET,
                        env.current_slot(),
                        inv082_terminal_hints(route_index),
                    ),
                    Inv082TerminalRail::Close => env.close_resolved_primary(TARGET),
                };
                inv082_assert_rejected(
                    result.expect_err("owner window must reject an unsigned terminal rail"),
                    PercolatorError::ExpectedSigner,
                );
                inv082_assert_frame(&env, &frame, &[]);
                terminal_rejections += 1;
            }

            env.warp_to_slot(payout_maturity);
            let actor_order = if reverse {
                [4usize, 3, 2, 1, 0]
            } else {
                [0usize, 1, 2, 3, 4]
            };
            let mut reached_terminal = false;
            for sweep in 0..SWEEP_BOUND {
                if inv082_terminal_rank(&env) == Inv082TerminalRank::default() {
                    reached_terminal = true;
                    break;
                }
                let mut sweep_progress = 0usize;
                for (position, actor) in actor_order.into_iter().enumerate() {
                    if inv082_economically_terminal(&env, actor) {
                        continue;
                    }
                    let rail = if (route_index + usize::from(reverse)) % 2 == 0 {
                        Inv082TerminalRail::Crank
                    } else {
                        Inv082TerminalRail::Close
                    };
                    let hint_word = sweep * PRIMARY_ACTOR_COUNT + position;
                    let before_rank = inv082_terminal_rank(&env);
                    let frame = inv082_account_frame(&env);
                    let result = match rail {
                        Inv082TerminalRail::Crank => {
                            env.crank(actor, env.current_slot(), inv082_terminal_hints(hint_word))
                        }
                        Inv082TerminalRail::Close => env.close_resolved_primary(actor),
                    };
                    match result {
                        Ok(success) => {
                            let after_rank = inv082_terminal_rank(&env);
                            assert!(
                                after_rank < before_rank,
                                "{route:?}/{reverse}: successful {rail:?} did not lower terminal rank: {before_rank:?} -> {after_rank:?}"
                            );
                            inv082_assert_frame(
                                &env,
                                &frame,
                                &[
                                    env.market,
                                    env.actors[actor].portfolio,
                                    env.actors[actor].destination_token,
                                    env.vault,
                                ],
                            );
                            assert_public_stock_census("INV-082 terminal exposure progress", &env)
                                .unwrap();
                            assert_public_encumbrance_census(
                                "INV-082 terminal exposure progress",
                                &env,
                            )
                            .unwrap();
                            max_cu = max_cu.max(success.compute_units);
                            rail_successes
                                [usize::from(matches!(rail, Inv082TerminalRail::Close))] += 1;
                            if matches!(rail, Inv082TerminalRail::Crank) {
                                successful_hint_words.insert(hint_word % 4);
                            }
                            terminal_progress += 1;
                            sweep_progress += 1;
                        }
                        Err(_) => {
                            inv082_assert_frame(&env, &frame, &[]);
                            terminal_rejections += 1;
                        }
                    }
                }
                assert!(
                    sweep_progress != 0
                        || inv082_terminal_rank(&env) == Inv082TerminalRank::default(),
                    "{route:?}/{reverse}: terminal schedule reached a nonterminal fixed point"
                );
            }
            if inv082_terminal_rank(&env) == Inv082TerminalRank::default() {
                reached_terminal = true;
            }
            assert!(
                reached_terminal,
                "{route:?}/{reverse}: terminal schedule exceeded {SWEEP_BOUND} sweeps"
            );

            let payouts: [u64; PRIMARY_ACTOR_COUNT] = std::array::from_fn(|actor| {
                env.token_amount(env.actors[actor].destination_token) - destinations_before[actor]
            });
            assert_eq!(
                payouts, expected_payouts,
                "{route:?}/{reverse}: terminal payout disagrees with public mark deltas"
            );
            assert_eq!(
                payouts
                    .iter()
                    .map(|amount| u128::from(*amount))
                    .sum::<u128>(),
                DEPOSITS.iter().sum::<u128>(),
                "{route:?}/{reverse}: terminal payouts lost funded value"
            );
            assert_eq!(env.primary_market_state().1.c_tot, 0);
            assert_eq!(env.primary_market_state().1.vault, 0);
            assert_eq!(env.token_amount(env.vault), 0);
            assert_eq!(env.token_supply_observed(), supply_before);
            assert_eq!(
                env.primary_market_state().1.materialized_portfolio_count,
                PRIMARY_ACTOR_COUNT as u64,
                "economic completion must not be conflated with signer-gated deletion"
            );

            for actor in actor_order {
                for rail in [Inv082TerminalRail::Crank, Inv082TerminalRail::Close] {
                    let frame = inv082_account_frame(&env);
                    let result = match rail {
                        Inv082TerminalRail::Crank => {
                            env.crank(actor, env.current_slot(), inv082_terminal_hints(actor))
                        }
                        Inv082TerminalRail::Close => env.close_resolved_primary(actor),
                    };
                    inv082_assert_rejected(
                        result.expect_err("terminal rail must not report progress twice"),
                        PercolatorError::EngineNonProgress,
                    );
                    inv082_assert_frame(&env, &frame, &[]);
                    terminal_rejections += 1;
                }
            }

            let unavailable: BTreeSet<_> = env
                .actors
                .iter()
                .map(|actor| actor.signer.pubkey())
                .chain(std::iter::once(Pubkey::new_from_array(
                    env.primary_market_state().0.marketauth,
                )))
                .collect();
            let trace = env.finish_public_trace();
            trace
                .validate_public_execution()
                .expect("stale refresh and terminal suffix must be public and rollback-exact");
            for step in &trace.steps {
                assert!(
                    inv082_keeper_only(step, &unavailable),
                    "{route:?}/{reverse}: suffix used an owner or admin shortcut: {step:?}"
                );
            }
            worlds += 1;
        }
    }

    assert_eq!(worlds, 8);
    assert_eq!(refresh_progress, worlds * 4);
    assert_eq!(refresh_rejections, worlds * 4);
    assert_eq!(terminal_progress, worlds * (PRIMARY_ACTOR_COUNT + 1));
    assert_eq!(terminal_rejections, worlds * (2 + 2 * PRIMARY_ACTOR_COUNT));
    assert!(rail_successes.into_iter().all(|count| count != 0));
    assert_eq!(successful_hint_words, BTreeSet::from([0, 1, 2, 3]));
    assert!(max_cu < TX_CU_LIMIT);
    eprintln!(
        "INV-071/072/073/078/082 stale-exposure continuation: worlds={worlds} refresh_progress={refresh_progress} refresh_rejections={refresh_rejections} terminal_progress={terminal_progress} terminal_rejections={terminal_rejections} rail_successes={rail_successes:?} hint_words={successful_hint_words:?} max_cu={max_cu}"
    );
}
