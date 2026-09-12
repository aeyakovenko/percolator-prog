//! INV-074 - Scope locality.
//!
//! Normative obligation: account-local close state and side/domain barriers may constrain only
//! the economic scope they protect. They cannot prevent an unrelated owner from reducing an
//! existing position on the same asset.
//!
//! Evidence (F over public I routes): the probe opens a small healthy pair, then independently
//! creates a real active bankruptcy close for another pair on the same asset. The healthy pair's
//! full bilateral reduction must land through all four deployed trade routes in both long/short
//! orientations, remove exactly its own effective OI, preserve both close participants and the
//! complete close ledger byte-for-byte, and move no internal or SPL custody. The eight fresh
//! worlds must also converge to identical normalized economics.
//!
//! A second probe creates active closes on two different assets and advances each independently;
//! each crank must strictly reduce only its selected ledger while framing the other close and all
//! custody.
//!
//! A third paired probe lands an authenticated asset shutdown while a close is active, either
//! before or after one close continuation. Both schedules must preserve a bounded public exit for
//! every funded portfolio and converge to identical owner payouts and terminal accounting.
//!
//! A fourth probe publicly materializes two simultaneous partial payout receipts in different
//! source domains. Substituting either claimant's valid quote destination for the other's must
//! reject with a complete economic snapshot frame. A canonical value-moving top-up for either
//! claimant must preserve the other portfolio, receipt, and destination byte-for-byte, after which
//! both receipts retain bounded terminal continuations.
//!
//! A fifth matrix crosses every trade route and both reset-side orientations with the landing order
//! of an asset-0 `ResetPending` shutdown and an unrelated asset-1 bilateral exit. Shutdown must
//! frame the unrelated asset and oracle profile; the unrelated exit must frame the complete reset
//! episode; and both schedules must reach the same owner payouts through bounded public cleanup.
//!
//! A sixth matrix puts both assets in the same two portfolios. After asset 0 enters
//! `ResetPending` and Recovery, an immediate asset-1 exit may either land or reject atomically, but
//! the canonical account crank must clear the stale asset-0 prerequisite and make the identical
//! exit succeed. Crank-first and exit-attempt-first schedules must converge economically.
//!
//! A seventh matrix proves the adjacent overlap is unreachable from the other direction: after a
//! portfolio enters an active close on asset 0, all four trade routes reject attaching fresh risk
//! on asset 1 for either account role and either requested side. Every rejection frames the whole
//! economic snapshot, and the original close still drains permissionlessly before all owners exit.
//!
//! An eighth matrix covers the inverse ordering. A one-atom asset-1 leg keeps the losing portfolio
//! nonflat while the asset-0 bankruptcy reduction lands, so terminal close creation is deferred.
//! Flattening that last leg must preserve the exact direct-close terminal economics across every
//! route, close orientation, prior account role, and side, whether normalization needs an explicit
//! close ledger or not. CPI worlds also prove that a taker-side mutation revokes stale LP authority
//! and that fresh owner consent restores the route without changing economics.
//!
//! A ninth matrix removes the fully reduced owner's portfolio before the opposite reset leg is
//! cleaned, across both sides and Active/Recovery. Early and deferred owner-signed deletion must
//! preserve the same principal payouts and permissionless cleanup, framing a still-exposed foreign
//! pair after every transaction. This is bounded lifecycle composition, not universal liveness.
//!
//! Guarantee boundary: these are the same-asset risk-reduction and two-asset/two-account close
//! cells. Risk increase while a domain loss barrier is active is intentionally outside the
//! guarantee; broader side/domain/lifecycle combinations remain open.

use super::inv_052_split_merge_invariance::run_resolved_claim_partition;
use crate::support::fuzz_model::{
    assert_public_encumbrance_census, assert_public_stock_census, execute_trade_route,
    run_active_close_cross_asset_admission_probe, run_active_close_shutdown_liveness_probe,
    run_concurrent_close_locality_probe, run_deferred_close_cross_asset_probe,
    run_same_asset_close_locality_probe, TradeRoute,
};
use crate::support::v16_svm::{MarketConfig, V16Svm, INITIAL_PRICE, TX_CU_LIMIT};
use percolator::{AssetLifecycleV16, SideModeV16, POS_SCALE};
use percolator_prog::ix::CrankObservationHint;
use solana_sdk::signature::Signer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LifecycleExitOrder {
    ExitThenShutdown,
    ShutdownThenExit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LifecycleLocalityOutcome {
    destination_balances: [u64; 4],
    token_supply: u128,
    reset_generation_after_restart: u64,
    unrelated_generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ScopedRollbackSnapshot {
    market: Vec<u8>,
    foreign_market: Vec<u8>,
    portfolios: Vec<Vec<u8>>,
    foreign_portfolio: Vec<u8>,
    backing_ledger: Vec<u8>,
    tokens: Vec<(solana_sdk::pubkey::Pubkey, Vec<u8>)>,
    matcher_contexts: Vec<Vec<u8>>,
    economic_lamports: Vec<(solana_sdk::pubkey::Pubkey, u64)>,
}

fn scoped_rollback_snapshot(env: &V16Svm) -> ScopedRollbackSnapshot {
    ScopedRollbackSnapshot {
        market: env.market_data(false),
        foreign_market: env.market_data(true),
        portfolios: env.all_primary_portfolio_data(),
        foreign_portfolio: env.foreign_portfolio_data(),
        backing_ledger: env.backing_domain_ledger_data(),
        tokens: env.all_token_account_data(),
        matcher_contexts: env.all_matcher_context_data(),
        economic_lamports: env.all_economic_account_lamports(),
    }
}

fn active_leg_count_for_asset(env: &V16Svm, actor: usize, asset_index: u16) -> usize {
    env.primary_portfolio(actor)
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .filter(|leg| leg.active && leg.asset_index == u32::from(asset_index))
        .count()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SharedPortfolioExitOrder {
    ExitAttemptThenCrank,
    CrankThenExit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SharedPortfolioLocalityOutcome {
    destination_balances: [u64; 2],
    token_supply: u128,
    reset_generation_after_restart: u64,
    unrelated_generation: u64,
    early_exit_rejected: bool,
    reset_crank_calls: usize,
}

fn clear_shared_portfolio_reset_prerequisite(
    env: &mut V16Svm,
    reducer_long: bool,
    case: &str,
) -> Result<(u64, usize), String> {
    const STALE_COUNTERPARTY: usize = 1;
    let observation = vec![CrankObservationHint {
        asset_index: 0,
        oracle_accounts: env.primary_profile(0).oracle_leg_count,
    }];
    let mut max_compute_units = 0u64;
    let mut crank_calls = 0usize;
    for _ in 0..8 {
        if active_leg_count_for_asset(env, STALE_COUNTERPARTY, 0) == 0 {
            break;
        }
        let crank = env
            .crank(STALE_COUNTERPARTY, env.current_slot(), observation.clone())
            .map_err(|error| format!("{case}: shared-account reset crank: {error}"))?;
        max_compute_units = max_compute_units.max(crank.compute_units);
        crank_calls += 1;
    }
    if active_leg_count_for_asset(env, STALE_COUNTERPARTY, 0) != 0 {
        return Err(format!(
            "{case}: shared-account reset prerequisite exceeded bounded crank budget"
        ));
    }
    let asset = env.primary_market_state().1.assets[0];
    let (stored, stale, pending) = if reducer_long {
        (
            asset.stored_pos_count_short,
            asset.stale_account_count_short,
            asset.pending_obligation_count_short,
        )
    } else {
        (
            asset.stored_pos_count_long,
            asset.stale_account_count_long,
            asset.pending_obligation_count_long,
        )
    };
    if (stored, stale, pending) != (0, 0, 0) {
        return Err(format!(
            "{case}: shared-account reset cleanup left work: {:?}",
            (stored, stale, pending)
        ));
    }
    Ok((max_compute_units, crank_calls))
}

fn run_shared_portfolio_lifecycle_world(
    route: TradeRoute,
    reducer_long: bool,
    order: SharedPortfolioExitOrder,
    seed: [u8; 32],
) -> Result<SharedPortfolioLocalityOutcome, String> {
    const REDUCER: usize = 0;
    const COUNTERPARTY: usize = 1;

    let case = format!("{route:?}/reducer_long={reducer_long}/{order:?}");
    let mut env = V16Svm::new(seed, MarketConfig::default());
    let token_supply = env.token_supply_observed();
    env.configure_permissionless_resolve(1_000, 100)
        .map_err(|error| format!("{case}: configure Recovery route: {error}"))?;

    let reset_size_q = if reducer_long {
        POS_SCALE as i128
    } else {
        -(POS_SCALE as i128)
    };
    let unrelated_size_q = -reset_size_q;
    let mut max_compute_units = execute_trade_route(
        &mut env,
        TradeRoute::NoCpi,
        REDUCER,
        COUNTERPARTY,
        0,
        reset_size_q,
        INITIAL_PRICE,
        0,
    )
    .map_err(|error| format!("{case}: establish reset-side position: {error}"))?
    .compute_units;
    max_compute_units = max_compute_units.max(
        execute_trade_route(
            &mut env,
            route,
            REDUCER,
            COUNTERPARTY,
            1,
            unrelated_size_q,
            INITIAL_PRICE,
            0,
        )
        .map_err(|error| format!("{case}: establish shared unrelated position: {error}"))?
        .compute_units,
    );
    max_compute_units = max_compute_units.max(
        env.rebalance_reduce(REDUCER, 0, POS_SCALE)
            .map_err(|error| format!("{case}: enter shared ResetPending episode: {error}"))?
            .compute_units,
    );
    max_compute_units = max_compute_units.max(
        env.shutdown_asset(0, env.current_slot())
            .map_err(|error| format!("{case}: shutdown shared reset asset: {error}"))?
            .compute_units,
    );
    assert_public_stock_census("INV-074 shared-portfolio lifecycle prefix", &env)?;
    assert_public_encumbrance_census("INV-074 shared-portfolio lifecycle prefix", &env)?;

    let mut unrelated_exit_landed = false;
    let mut early_exit_rejected = false;
    if order == SharedPortfolioExitOrder::ExitAttemptThenCrank {
        let before_attempt = scoped_rollback_snapshot(&env);
        match execute_trade_route(
            &mut env,
            route,
            REDUCER,
            COUNTERPARTY,
            1,
            -unrelated_size_q,
            INITIAL_PRICE,
            0,
        ) {
            Ok(exit) => {
                max_compute_units = max_compute_units.max(exit.compute_units);
                unrelated_exit_landed = true;
            }
            Err(_) => {
                early_exit_rejected = true;
                if scoped_rollback_snapshot(&env) != before_attempt {
                    return Err(format!(
                        "{case}: rejected early unrelated exit did not roll back exactly"
                    ));
                }
            }
        }
    }

    let (reset_crank_compute_units, reset_crank_calls) =
        clear_shared_portfolio_reset_prerequisite(&mut env, reducer_long, &case)?;
    max_compute_units = max_compute_units.max(reset_crank_compute_units);
    if early_exit_rejected && reset_crank_calls == 0 {
        return Err(format!(
            "{case}: early exit rejected without a successful prerequisite crank"
        ));
    }
    if !unrelated_exit_landed {
        max_compute_units = max_compute_units.max(
            execute_trade_route(
                &mut env,
                route,
                REDUCER,
                COUNTERPARTY,
                1,
                -unrelated_size_q,
                INITIAL_PRICE,
                0,
            )
            .map_err(|error| format!("{case}: post-crank unrelated exit: {error}"))?
            .compute_units,
        );
    }
    let unrelated = env.primary_market_state().1.assets[1];
    if unrelated.oi_eff_long_q != 0 || unrelated.oi_eff_short_q != 0 {
        return Err(format!(
            "{case}: shared-account unrelated exit retained OI: {unrelated:?}"
        ));
    }

    let reset_side = u8::from(reducer_long);
    max_compute_units = max_compute_units.max(
        env.finalize_reset_side(0, reset_side)
            .map_err(|error| format!("{case}: finalize shared reset: {error}"))?
            .compute_units,
    );
    env.warp_to_slot(2);
    max_compute_units = max_compute_units.max(
        env.restart_asset_oracle(0, 2, INITIAL_PRICE)
            .map_err(|error| format!("{case}: restart shared reset asset: {error}"))?
            .compute_units,
    );
    for actor in [REDUCER, COUNTERPARTY] {
        let capital = env.primary_portfolio(actor).capital.get();
        max_compute_units = max_compute_units.max(
            env.withdraw_primary(actor, capital)
                .map_err(|error| format!("{case}: shared actor {actor} withdrawal: {error}"))?
                .compute_units,
        );
    }
    if max_compute_units >= TX_CU_LIMIT
        || env.token_supply_observed() != token_supply
        || [REDUCER, COUNTERPARTY]
            .into_iter()
            .any(|actor| env.primary_portfolio(actor).capital.get() != 0)
    {
        return Err(format!(
            "{case}: shared-account lifecycle failed CU, supply, or terminal-capital checks"
        ));
    }
    assert_public_stock_census("INV-074 shared-portfolio lifecycle terminal", &env)?;
    assert_public_encumbrance_census("INV-074 shared-portfolio lifecycle terminal", &env)?;

    let group = env.primary_market_state().1;
    Ok(SharedPortfolioLocalityOutcome {
        destination_balances: std::array::from_fn(|actor| {
            env.token_amount(env.actors[actor].destination_token)
        }),
        token_supply,
        reset_generation_after_restart: group.assets[0].market_id,
        unrelated_generation: group.assets[1].market_id,
        early_exit_rejected,
        reset_crank_calls,
    })
}

fn shutdown_reset_asset_without_touching_unrelated_scope(
    env: &mut V16Svm,
    case: &str,
) -> Result<u64, String> {
    let unrelated_before = env.primary_market_state().1.assets[1];
    let unrelated_profile_before = env.primary_profile(1);
    let shutdown = env
        .shutdown_asset(0, env.current_slot())
        .map_err(|error| format!("{case}: shutdown reset asset: {error}"))?;
    let group_after = env.primary_market_state().1;
    if group_after.assets[1] != unrelated_before
        || env.primary_profile(1) != unrelated_profile_before
    {
        return Err(format!(
            "{case}: asset-0 shutdown mutated unrelated asset-1 state or oracle profile"
        ));
    }
    if group_after.assets[0].lifecycle != AssetLifecycleV16::Recovery {
        return Err(format!("{case}: asset-0 shutdown did not enter Recovery"));
    }
    Ok(shutdown.compute_units)
}

fn close_unrelated_pair_without_touching_reset_scope(
    env: &mut V16Svm,
    route: TradeRoute,
    case: &str,
) -> Result<u64, String> {
    let reset_before = env.primary_market_state().1.assets[0];
    let reset_profile_before = env.primary_profile(0);
    let close = execute_trade_route(env, route, 2, 3, 1, -(POS_SCALE as i128), INITIAL_PRICE, 0)
        .map_err(|error| format!("{case}: unrelated asset-1 exit: {error}"))?;
    let group_after = env.primary_market_state().1;
    if group_after.assets[0] != reset_before || env.primary_profile(0) != reset_profile_before {
        return Err(format!(
            "{case}: unrelated asset-1 exit mutated the asset-0 reset episode"
        ));
    }
    let unrelated = group_after.assets[1];
    if unrelated.lifecycle != AssetLifecycleV16::Active
        || unrelated.oi_eff_long_q != 0
        || unrelated.oi_eff_short_q != 0
    {
        return Err(format!(
            "{case}: unrelated asset-1 exit did not clear matched OI: {unrelated:?}"
        ));
    }
    Ok(close.compute_units)
}

fn run_lifecycle_locality_world(
    route: TradeRoute,
    reducer_long: bool,
    order: LifecycleExitOrder,
    seed: [u8; 32],
) -> Result<LifecycleLocalityOutcome, String> {
    const RESET_REDUCER: usize = 0;
    const RESET_COUNTERPARTY: usize = 1;
    const UNRELATED_LONG: usize = 2;
    const UNRELATED_SHORT: usize = 3;

    let case = format!("{route:?}/reducer_long={reducer_long}/{order:?}");
    let mut env = V16Svm::new(seed, MarketConfig::default());
    let token_supply = env.token_supply_observed();
    env.configure_permissionless_resolve(1_000, 100)
        .map_err(|error| format!("{case}: configure Recovery route: {error}"))?;

    let reset_size_q = if reducer_long {
        POS_SCALE as i128
    } else {
        -(POS_SCALE as i128)
    };
    let mut max_compute_units = execute_trade_route(
        &mut env,
        TradeRoute::NoCpi,
        RESET_REDUCER,
        RESET_COUNTERPARTY,
        0,
        reset_size_q,
        INITIAL_PRICE,
        0,
    )
    .map_err(|error| format!("{case}: establish reset-side position: {error}"))?
    .compute_units;
    max_compute_units = max_compute_units.max(
        execute_trade_route(
            &mut env,
            route,
            UNRELATED_LONG,
            UNRELATED_SHORT,
            1,
            POS_SCALE as i128,
            INITIAL_PRICE,
            0,
        )
        .map_err(|error| format!("{case}: establish unrelated position: {error}"))?
        .compute_units,
    );
    max_compute_units = max_compute_units.max(
        env.rebalance_reduce(RESET_REDUCER, 0, POS_SCALE)
            .map_err(|error| format!("{case}: enter asset-0 ResetPending: {error}"))?
            .compute_units,
    );
    let reset_side = usize::from(reducer_long);
    let pending = env.primary_market_state().1.assets[0];
    let (pending_mode, pending_count) = if reducer_long {
        (pending.mode_short, pending.stored_pos_count_short)
    } else {
        (pending.mode_long, pending.stored_pos_count_long)
    };
    if pending_mode != SideModeV16::ResetPending || pending_count != 1 {
        return Err(format!(
            "{case}: public unilateral reduction did not create the expected reset episode"
        ));
    }
    assert_public_stock_census("INV-074 lifecycle locality prefix", &env)?;
    assert_public_encumbrance_census("INV-074 lifecycle locality prefix", &env)?;

    match order {
        LifecycleExitOrder::ExitThenShutdown => {
            max_compute_units = max_compute_units.max(
                close_unrelated_pair_without_touching_reset_scope(&mut env, route, &case)?,
            );
            max_compute_units = max_compute_units.max(
                shutdown_reset_asset_without_touching_unrelated_scope(&mut env, &case)?,
            );
        }
        LifecycleExitOrder::ShutdownThenExit => {
            max_compute_units = max_compute_units.max(
                shutdown_reset_asset_without_touching_unrelated_scope(&mut env, &case)?,
            );
            max_compute_units = max_compute_units.max(
                close_unrelated_pair_without_touching_reset_scope(&mut env, route, &case)?,
            );
        }
    }
    assert_public_stock_census("INV-074 after unrelated exit/shutdown order", &env)?;
    assert_public_encumbrance_census("INV-074 after unrelated exit/shutdown order", &env)?;

    for actor in [UNRELATED_LONG, UNRELATED_SHORT] {
        let capital = env.primary_portfolio(actor).capital.get();
        max_compute_units = max_compute_units.max(
            env.withdraw_primary(actor, capital)
                .map_err(|error| format!("{case}: unrelated actor {actor} withdrawal: {error}"))?
                .compute_units,
        );
    }

    let observation = vec![CrankObservationHint {
        asset_index: 0,
        oracle_accounts: env.primary_profile(0).oracle_leg_count,
    }];
    for _ in 0..8 {
        let asset = env.primary_market_state().1.assets[0];
        let stored_count = if reducer_long {
            asset.stored_pos_count_short
        } else {
            asset.stored_pos_count_long
        };
        if stored_count == 0 {
            break;
        }
        max_compute_units = max_compute_units.max(
            env.crank(RESET_COUNTERPARTY, env.current_slot(), observation.clone())
                .map_err(|error| format!("{case}: clean reset counterparty: {error}"))?
                .compute_units,
        );
    }
    let cleaned = env.primary_market_state().1.assets[0];
    let cleaned_count = if reducer_long {
        cleaned.stored_pos_count_short
    } else {
        cleaned.stored_pos_count_long
    };
    if cleaned_count != 0 {
        return Err(format!(
            "{case}: reset cleanup exceeded the bounded crank budget"
        ));
    }
    max_compute_units = max_compute_units.max(
        env.finalize_reset_side(0, reset_side as u8)
            .map_err(|error| format!("{case}: finalize asset-0 reset: {error}"))?
            .compute_units,
    );
    env.warp_to_slot(2);
    max_compute_units = max_compute_units.max(
        env.restart_asset_oracle(0, 2, INITIAL_PRICE)
            .map_err(|error| format!("{case}: restart asset-0 oracle: {error}"))?
            .compute_units,
    );

    for actor in [RESET_REDUCER, RESET_COUNTERPARTY] {
        let capital = env.primary_portfolio(actor).capital.get();
        max_compute_units = max_compute_units.max(
            env.withdraw_primary(actor, capital)
                .map_err(|error| format!("{case}: reset actor {actor} withdrawal: {error}"))?
                .compute_units,
        );
    }
    if max_compute_units >= TX_CU_LIMIT {
        return Err(format!(
            "{case}: required lifecycle-local exit exceeded CU ceiling: {max_compute_units}"
        ));
    }
    if env.token_supply_observed() != token_supply
        || (0..4).any(|actor| env.primary_portfolio(actor).capital.get() != 0)
    {
        return Err(format!(
            "{case}: lifecycle-local terminal state lost supply or retained capital"
        ));
    }
    assert_public_stock_census("INV-074 lifecycle locality terminal", &env)?;
    assert_public_encumbrance_census("INV-074 lifecycle locality terminal", &env)?;

    let group = env.primary_market_state().1;
    Ok(LifecycleLocalityOutcome {
        destination_balances: std::array::from_fn(|actor| {
            env.token_amount(env.actors[actor].destination_token)
        }),
        token_supply,
        reset_generation_after_restart: group.assets[0].market_id,
        unrelated_generation: group.assets[1].market_id,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingResetLocalityFrame {
    assets: Vec<percolator::AssetStateV16>,
    profiles: [percolator_prog::state::AssetOracleProfileV16; 2],
    accounts: ScopedRollbackSnapshot,
}

fn pending_reset_locality_frame(env: &V16Svm) -> PendingResetLocalityFrame {
    let mut accounts = scoped_rollback_snapshot(env);
    // Shared counters and the two exiting accounts may change; the other asset/account scopes may not.
    accounts.market.clear();
    accounts.portfolios.drain(..2);
    accounts.tokens.retain(|(key, _)| {
        ![
            env.vault,
            env.actors[0].destination_token,
            env.actors[1].destination_token,
        ]
        .contains(key)
    });
    accounts.economic_lamports.retain(|(key, _)| {
        ![env.market, env.actors[0].portfolio, env.actors[1].portfolio].contains(key)
    });
    PendingResetLocalityFrame {
        assets: env.primary_market_state().1.assets[1..].to_vec(),
        profiles: [env.primary_profile(1), env.primary_profile(2)],
        accounts,
    }
}

struct PendingResetExitLedger {
    config: MarketConfig,
    paid: [bool; 2],
    closed: [bool; 2],
    token_supply: u128,
    locality: PendingResetLocalityFrame,
    steps: usize,
}

impl PendingResetExitLedger {
    fn check(&self, env: &V16Svm) {
        assert_eq!(pending_reset_locality_frame(env), self.locality);
        assert_eq!(env.token_supply_observed(), self.token_supply);
        let group = env.primary_market_state().1;
        let paid: u128 = (0..2)
            .filter(|actor| self.paid[*actor])
            .map(|actor| self.config.actor_deposits[actor])
            .sum();
        let remaining = self.config.actor_deposits.iter().sum::<u128>() - paid;
        assert_eq!(group.c_tot, remaining);
        assert_eq!(group.vault, remaining);
        assert_eq!(u128::from(env.token_amount(env.vault)), remaining);
        assert_eq!(group.insurance, 0);
        assert_eq!(
            group.materialized_portfolio_count,
            (env.actors.len() - self.closed.iter().filter(|closed| **closed).count()) as u64
        );
        for (actor, fixture) in env.actors.iter().enumerate() {
            let payout = if actor < 2 && self.paid[actor] {
                self.config.actor_deposits[actor]
            } else {
                0
            };
            assert_eq!(
                u128::from(env.token_amount(fixture.destination_token)),
                payout
            );
            assert_eq!(
                u128::from(env.token_amount(fixture.source_token)),
                u128::from(self.config.actor_token_balances[actor])
                    - self.config.actor_deposits[actor]
            );
            if actor < 2 && self.closed[actor] {
                assert!(env.primary_portfolio_data(actor).is_empty());
                assert_eq!(env.account_lamports(fixture.portfolio), 0);
            } else {
                let portfolio = env.primary_portfolio(actor);
                assert_eq!(
                    portfolio.capital.get(),
                    self.config.actor_deposits[actor] - payout
                );
                assert_eq!(portfolio.pnl.get(), 0);
            }
        }
        assert_public_stock_census("INV-074 pending-reset owner exit", env).unwrap();
        assert_public_encumbrance_census("INV-074 pending-reset owner exit", env).unwrap();
    }

    fn step(
        &mut self,
        env: &mut V16Svm,
        action: impl FnOnce(&mut V16Svm) -> Result<crate::support::v16_svm::TxSuccess, String>,
    ) {
        action(env).expect("INV-074 bounded pending-reset exit step");
        self.steps += 1;
        self.check(env);
    }

    fn close(&mut self, env: &mut V16Svm, actor: usize) {
        let rent = env.account_lamports(env.actors[actor].portfolio);
        let slab_lamports = env.account_lamports(env.market);
        assert!(rent > 0);
        self.closed[actor] = true;
        self.step(env, |env| env.close_primary_portfolio(actor));
        assert_eq!(env.account_lamports(env.market), slab_lamports + rent);
    }
}

#[test]
fn v16_program_owner_deletion_before_reset_cleanup_preserves_local_exit() {
    let config = MarketConfig {
        actor_deposits: [
            100_000_003,
            100_000_019,
            100_000_031,
            100_000_057,
            2_000_000_000,
        ],
        ..MarketConfig::default()
    };
    let mut worlds = 0;
    let mut checked_steps = 0;
    let mut cleanup_calls = 0;
    let mut max_compute_units = 0;
    for reducer_long in [false, true] {
        for recovery in [false, true] {
            let mut expected = None;
            for (close_early, complete_hints) in [false, true]
                .into_iter()
                .flat_map(|early| [false, true].map(|hints| (early, hints)))
            {
                let case = format!("long={reducer_long}/recovery={recovery}/early={close_early}/hints={complete_hints}");
                let mut env = V16Svm::new([0x74; 32], config);
                env.configure_permissionless_resolve(1_000, 100).unwrap();
                let size = if reducer_long {
                    POS_SCALE as i128
                } else {
                    -(POS_SCALE as i128)
                };
                env.trade_no_cpi(0, 1, 0, size, INITIAL_PRICE, 0).unwrap();
                env.trade_no_cpi(2, 3, 1, POS_SCALE as i128, INITIAL_PRICE, 0)
                    .unwrap();
                assert_eq!(active_leg_count_for_asset(&env, 2, 1), 1);
                assert_eq!(active_leg_count_for_asset(&env, 3, 1), 1);
                let mut ledger = PendingResetExitLedger {
                    config,
                    paid: [false; 2],
                    closed: [false; 2],
                    token_supply: env.token_supply_observed(),
                    locality: pending_reset_locality_frame(&env),
                    steps: 0,
                };
                ledger.check(&env);
                env.begin_public_trace();
                ledger.step(&mut env, |env| env.rebalance_reduce(0, 0, POS_SCALE));
                if recovery {
                    ledger.step(&mut env, |env| env.shutdown_asset(0, env.current_slot()));
                }
                let pending = env.primary_market_state().1.assets[0];
                assert_eq!(
                    pending.lifecycle,
                    if recovery {
                        AssetLifecycleV16::Recovery
                    } else {
                        AssetLifecycleV16::Active
                    }
                );
                let (mode, stored) = if reducer_long {
                    (pending.mode_short, pending.stored_pos_count_short)
                } else {
                    (pending.mode_long, pending.stored_pos_count_long)
                };
                assert_eq!((mode, stored), (SideModeV16::ResetPending, 1), "{case}");
                assert_eq!(active_leg_count_for_asset(&env, 0, 0), 0);
                assert_eq!(active_leg_count_for_asset(&env, 1, 0), 1);
                let counterparty_before = env.primary_portfolio_data(1);
                let reset_profile = env.primary_profile(0);
                let assert_pending_frame = |env: &V16Svm| {
                    assert_eq!(env.primary_market_state().1.assets[0], pending, "{case}");
                    assert_eq!(env.primary_profile(0), reset_profile, "{case}");
                    assert_eq!(env.primary_portfolio_data(1), counterparty_before, "{case}");
                };

                // Deletion is not a payout: the still-funded owner's attempt must roll back.
                let before = scoped_rollback_snapshot(&env);
                env.close_primary_portfolio(0)
                    .expect_err("funded portfolio cannot be deleted");
                assert_eq!(scoped_rollback_snapshot(&env), before, "{case}");
                ledger.steps += 1;
                ledger.check(&env);
                ledger.paid[0] = true;
                ledger.step(&mut env, |env| {
                    env.withdraw_primary(0, config.actor_deposits[0])
                });
                assert_pending_frame(&env);
                if close_early {
                    ledger.close(&mut env, 0);
                    assert_pending_frame(&env);
                }

                // Cleanup never supplies the deleted owner's portfolio or requires its signer.
                let hints = if complete_hints {
                    vec![CrankObservationHint {
                        asset_index: 0,
                        oracle_accounts: reset_profile.oracle_leg_count,
                    }]
                } else {
                    Vec::new()
                };
                let mut calls = 0;
                for _ in 0..8 {
                    let before = active_leg_count_for_asset(&env, 1, 0);
                    if before == 0 {
                        break;
                    }
                    ledger.step(&mut env, |env| {
                        env.crank(1, env.current_slot(), hints.clone())
                    });
                    assert_eq!(env.primary_profile(0), reset_profile, "{case}");
                    assert!(
                        active_leg_count_for_asset(&env, 1, 0) < before,
                        "{case}: reset rank"
                    );
                    calls += 1;
                }
                assert!(calls > 0, "{case}: nonvacuous reset cleanup");
                assert_eq!(active_leg_count_for_asset(&env, 1, 0), 0, "{case}");
                ledger.step(&mut env, |env| {
                    env.finalize_reset_side(0, u8::from(reducer_long))
                });
                assert_eq!(env.primary_profile(0), reset_profile, "{case}");
                let finalized = env.primary_market_state().1.assets[0];
                let assert_finalized_frame = |env: &V16Svm| {
                    assert_eq!(env.primary_market_state().1.assets[0], finalized, "{case}");
                    assert_eq!(env.primary_profile(0), reset_profile, "{case}");
                };
                assert_eq!(finalized.mode_long, SideModeV16::Normal);
                assert_eq!(finalized.mode_short, SideModeV16::Normal);
                assert_eq!((finalized.oi_eff_long_q, finalized.oi_eff_short_q), (0, 0));
                assert_eq!(
                    (
                        finalized.stored_pos_count_long,
                        finalized.stored_pos_count_short
                    ),
                    (0, 0)
                );
                assert_eq!(
                    (
                        finalized.stale_account_count_long,
                        finalized.stale_account_count_short
                    ),
                    (0, 0)
                );
                assert_eq!(
                    (
                        finalized.pending_obligation_count_long,
                        finalized.pending_obligation_count_short
                    ),
                    (0, 0)
                );
                if !close_early {
                    ledger.close(&mut env, 0);
                    assert_finalized_frame(&env);
                }
                ledger.paid[1] = true;
                ledger.step(&mut env, |env| {
                    env.withdraw_primary(1, config.actor_deposits[1])
                });
                assert_finalized_frame(&env);
                ledger.close(&mut env, 1);
                assert_finalized_frame(&env);

                let trace = env.finish_public_trace();
                trace
                    .validate_public_execution()
                    .expect("public lifecycle suffix");
                assert_eq!(trace.steps.len(), ledger.steps, "{case}");
                assert_eq!(trace.steps.iter().filter(|step| !step.succeeded).count(), 1);
                for step in &trace.steps {
                    let instruction =
                        percolator_prog::ix::Instruction::decode(&step.instruction_data).unwrap();
                    if matches!(
                        instruction,
                        percolator_prog::ix::Instruction::PermissionlessCrank { .. }
                            | percolator_prog::ix::Instruction::FinalizeResetSide { .. }
                    ) {
                        assert_eq!(step.transaction_signers, vec![step.fee_payer], "{case}");
                        assert!(
                            env.actors
                                .iter()
                                .all(|actor| actor.signer.pubkey() != step.fee_payer),
                            "{case}"
                        );
                        assert!(
                            !step
                                .accounts
                                .iter()
                                .any(|meta| meta.key == env.actors[0].portfolio),
                            "{case}"
                        );
                    }
                    if matches!(
                        instruction,
                        percolator_prog::ix::Instruction::ClosePortfolio { .. }
                    ) {
                        let owner = &step.accounts[0];
                        assert!(
                            owner.is_signer && step.transaction_signers.contains(&owner.key),
                            "{case}"
                        );
                        assert!(
                            [0, 1]
                                .iter()
                                .any(|actor| env.actors[*actor].signer.pubkey() == owner.key),
                            "{case}"
                        );
                        assert_ne!(owner.key, step.fee_payer, "{case}");
                    }
                    max_compute_units = max_compute_units.max(step.compute_units.unwrap_or(0));
                }
                let group = env.primary_market_state().1;
                let outcome = (
                    group.assets[0],
                    group.vault,
                    group.c_tot,
                    group.materialized_portfolio_count,
                    env.account_lamports(env.market),
                );
                if let Some(expected) = expected.as_ref() {
                    assert_eq!(
                        &outcome, expected,
                        "{case}: early/deferred deletion outcomes"
                    );
                } else {
                    expected = Some(outcome);
                }
                worlds += 1;
                checked_steps += ledger.steps;
                cleanup_calls += calls;
            }
        }
    }
    assert_eq!(worlds, 16);
    assert!(max_compute_units < TX_CU_LIMIT);
    eprintln!("INV-074 pending-reset deletion: worlds={worlds}, checked_steps={checked_steps}, cleanup_calls={cleanup_calls}, max_cu={max_compute_units}");
}

#[test]
fn v16_program_active_close_preserves_unrelated_same_asset_reduction() {
    let evidence = run_same_asset_close_locality_probe()
        .expect("INV-074 public same-asset close locality probe");

    assert_eq!(evidence.world_count, 8, "{evidence:?}");
    assert_eq!(evidence.route_worlds, [2; 4], "{evidence:?}");
    assert_eq!(evidence.orientation_worlds, [4; 2], "{evidence:?}");
    assert_ne!(evidence.close_residual_before, 0, "{evidence:?}");
    assert_eq!(
        evidence.close_residual_after, evidence.close_residual_before,
        "{evidence:?}"
    );
    assert_ne!(evidence.unrelated_position_q_before, 0, "{evidence:?}");
    assert_eq!(evidence.unrelated_position_q_after, 0, "{evidence:?}");
    assert_eq!(
        evidence.oi_long_before - evidence.oi_long_after,
        evidence.unrelated_position_q_before,
        "{evidence:?}"
    );
    assert_eq!(
        evidence.oi_short_before - evidence.oi_short_after,
        evidence.unrelated_position_q_before,
        "{evidence:?}"
    );
    assert!(
        evidence
            .coverage
            .route_success
            .iter()
            .all(|count| *count > 0),
        "{evidence:?}"
    );
    assert_ne!(evidence.coverage.token_frame_checks, 0, "{evidence:?}");
}

#[test]
fn v16_program_two_asset_closes_advance_without_crossing_scope() {
    let evidence = run_concurrent_close_locality_probe()
        .expect("INV-074 public concurrent-close locality probe");

    assert!(
        evidence.first_residual_after < evidence.first_residual_before,
        "{evidence:?}"
    );
    assert!(
        evidence.second_residual_after < evidence.second_residual_before,
        "{evidence:?}"
    );
    assert!(evidence.coverage.crank_progress >= 2, "{evidence:?}");
    assert_ne!(evidence.coverage.token_frame_checks, 0, "{evidence:?}");
}

#[test]
fn v16_program_active_close_shutdown_order_preserves_all_funded_exits() {
    let evidence = run_active_close_shutdown_liveness_probe()
        .expect("INV-074 active-close shutdown liveness probe");

    assert_eq!(evidence.world_count, 2, "{evidence:?}");
    assert_eq!(evidence.pre_shutdown_progress_worlds, 1, "{evidence:?}");
    assert_ne!(evidence.live_position_abs_q, 0, "{evidence:?}");
    assert_eq!(evidence.final_capital_total, 0, "{evidence:?}");
    assert_ne!(
        evidence.destination_payouts.iter().sum::<u128>(),
        0,
        "{evidence:?}"
    );
    assert_ne!(evidence.coverage.lifecycle_updates, 0, "{evidence:?}");
    assert_eq!(
        evidence.coverage.recovery_forfeit_successes, 0,
        "healthy Recovery exits must not require destructive forfeiture: {evidence:?}"
    );
    assert_ne!(
        evidence.coverage.crank_rank_component_reduced[4], 0,
        "the public crank must settle derived B work: {evidence:?}"
    );
    assert_ne!(evidence.coverage.user_positions_closed, 0, "{evidence:?}");
    assert_ne!(evidence.coverage.withdrawals, 0, "{evidence:?}");
}

#[test]
fn v16_program_active_close_rejects_cross_asset_risk_without_blocking_exit() {
    let evidence = run_active_close_cross_asset_admission_probe()
        .expect("INV-055/074 active-close cross-asset admission probe");

    assert_eq!(evidence.world_count, 32, "{evidence:?}");
    assert_eq!(evidence.route_worlds, [8; 4], "{evidence:?}");
    assert_eq!(evidence.close_orientation_worlds, [16; 2], "{evidence:?}");
    assert_eq!(evidence.active_role_worlds, [16; 2], "{evidence:?}");
    assert_eq!(evidence.requested_side_worlds, [16; 2], "{evidence:?}");
    assert_eq!(evidence.exact_rejections, 32, "{evidence:?}");
    assert_eq!(evidence.close_progress_worlds, 32, "{evidence:?}");
    assert_eq!(evidence.owner_exit_worlds, 32, "{evidence:?}");
    assert_ne!(evidence.total_owner_payout, 0, "{evidence:?}");
    assert_ne!(evidence.coverage.crank_progress, 0, "{evidence:?}");
    assert_ne!(evidence.coverage.withdrawals, 0, "{evidence:?}");
}

#[test]
fn v16_program_cross_asset_leg_cannot_erase_deferred_terminal_liability() {
    let evidence = run_deferred_close_cross_asset_probe()
        .expect("INV-057/074 deferred cross-asset close probe");

    assert_eq!(evidence.control_worlds, 8, "{evidence:?}");
    assert_eq!(evidence.deferred_worlds, 32, "{evidence:?}");
    assert_eq!(evidence.route_worlds, [10; 4], "{evidence:?}");
    assert_eq!(evidence.close_orientation_worlds, [20; 2], "{evidence:?}");
    assert_eq!(evidence.prior_role_worlds, [16; 2], "{evidence:?}");
    assert_eq!(evidence.prior_side_worlds, [16; 2], "{evidence:?}");
    assert_eq!(evidence.initial_close_deferrals, 32, "{evidence:?}");
    assert_eq!(evidence.fresh_matcher_reauthorizations, 8, "{evidence:?}");
    assert_eq!(evidence.terminal_equivalence_worlds, 32, "{evidence:?}");
    assert_eq!(evidence.close_progress_worlds, 8, "{evidence:?}");
    assert_eq!(evidence.owner_exit_worlds, 40, "{evidence:?}");
    assert_ne!(evidence.total_owner_payout, 0, "{evidence:?}");
    assert_ne!(evidence.coverage.crank_progress, 0, "{evidence:?}");
    assert_ne!(evidence.coverage.withdrawals, 0, "{evidence:?}");
}

#[test]
fn v16_program_concurrent_partial_receipts_are_claimant_local() {
    let evidence = run_resolved_claim_partition(true, TradeRoute::NoCpi, TradeRoute::BatchCpi)
        .expect("INV-074 concurrent public receipt locality probe");

    assert_eq!(evidence.concurrent_receipts, 2, "{evidence:?}");
    assert!(evidence.destination_substitution_rejected, "{evidence:?}");
    assert!(evidence.concurrent_receipt_framed, "{evidence:?}");
    assert_ne!(evidence.locality_claim_payout, 0, "{evidence:?}");
}

#[test]
fn v16_program_reset_shutdown_and_unrelated_exit_are_scope_local_and_order_safe() {
    for (route_index, route) in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ]
    .into_iter()
    .enumerate()
    {
        for reducer_long in [false, true] {
            let mut seed = [0x74; 32];
            seed[0] ^= route_index as u8;
            seed[1] ^= u8::from(reducer_long);
            let exit_first = run_lifecycle_locality_world(
                route,
                reducer_long,
                LifecycleExitOrder::ExitThenShutdown,
                seed,
            )
            .unwrap_or_else(|error| panic!("exit-before-shutdown locality: {error}"));
            let shutdown_first = run_lifecycle_locality_world(
                route,
                reducer_long,
                LifecycleExitOrder::ShutdownThenExit,
                seed,
            )
            .unwrap_or_else(|error| panic!("shutdown-before-exit locality: {error}"));
            assert_eq!(exit_first, shutdown_first, "{route:?}/{reducer_long}");
        }
    }
}

#[test]
fn v16_program_shared_portfolio_reset_prerequisite_has_bounded_exit_schedule() {
    for (route_index, route) in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ]
    .into_iter()
    .enumerate()
    {
        for reducer_long in [false, true] {
            let mut seed = [0x75; 32];
            seed[0] ^= route_index as u8;
            seed[1] ^= u8::from(reducer_long);
            let exit_attempt_first = run_shared_portfolio_lifecycle_world(
                route,
                reducer_long,
                SharedPortfolioExitOrder::ExitAttemptThenCrank,
                seed,
            )
            .unwrap_or_else(|error| panic!("early-exit shared-portfolio schedule: {error}"));
            let crank_first = run_shared_portfolio_lifecycle_world(
                route,
                reducer_long,
                SharedPortfolioExitOrder::CrankThenExit,
                seed,
            )
            .unwrap_or_else(|error| panic!("crank-first shared-portfolio schedule: {error}"));
            assert_eq!(
                exit_attempt_first.destination_balances, crank_first.destination_balances,
                "{route:?}/{reducer_long}"
            );
            assert_eq!(
                exit_attempt_first.token_supply, crank_first.token_supply,
                "{route:?}/{reducer_long}"
            );
            assert_eq!(
                exit_attempt_first.reset_generation_after_restart,
                crank_first.reset_generation_after_restart,
                "{route:?}/{reducer_long}"
            );
            assert_eq!(
                exit_attempt_first.unrelated_generation, crank_first.unrelated_generation,
                "{route:?}/{reducer_long}"
            );
            assert!(
                crank_first.reset_crank_calls > 0,
                "crank-first schedule must execute real reset cleanup: {route:?}/{reducer_long}/{crank_first:?}"
            );
            assert!(
                !exit_attempt_first.early_exit_rejected
                    || exit_attempt_first.reset_crank_calls > 0,
                "an early rejection must have a successful public prerequisite: {route:?}/{reducer_long}/{exit_attempt_first:?}"
            );
        }
    }
}
