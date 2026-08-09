//! INV-071 - Crank progress.
//!
//! Normative obligation: Every actionable live state has a permissionless successful crank that
//! strictly decreases a finite liveness rank or enters a lower terminal mode. An uncooperative
//! bankrupt owner cannot strand a market-wide lock after an ordinary risk-reducing trade.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_flat_negative_final_leg_route_matrix_reaches_terminal_payout` opens and closes the
//! same underfunded position through single/batch CPI/no-CPI routes. The adverse mark is published
//! through authenticated wrapper instructions. The final risk-reducing trade remains available,
//! preserves an asset-local close ledger before clearing the final leg, and leaves a public
//! `AdvanceClose` continuation. A permissionless crank books the residual without granting the
//! account authority to terminate unrelated markets. The configured permissionless stale-market
//! policy then begins terminal settlement, signed owner closes terminate every funded portfolio,
//! empty the real SPL vault, and preserve exact token supply. The trace rejects any out-of-band
//! economic-state mutation and requires exact rollback for every rejected instruction.
//!
//! Guarantee boundary: this is deployed LiteSVM evidence for the four wrapper trade routes and the
//! uniquely attributable one-asset residual class. The engine's production selector and rank
//! proofs cover the local transition; bounded reachability across every lifecycle class remains
//! shared with INV-082.

use super::*;
use crate::support::{
    fuzz_model::execute_trade_route,
    v16_svm::{MarketConfig, V16Svm, PRIMARY_ACTOR_COUNT, TX_CU_LIMIT},
};
use percolator::{active_bitmap_is_empty, MarketModeV16, SideV16, POS_SCALE};
use percolator_prog::ix::CrankObservationHint;

#[derive(Clone, Debug)]
struct FlatNegativeCrankOutcome {
    route: TradeRoute,
    residual_before: u128,
    residual_after: u128,
    terminal_close_calls: usize,
    destination_payouts: u128,
    expected_payouts: u128,
    max_compute_units: u64,
    trace_steps: usize,
}

fn portfolio_is_economically_terminal(env: &V16Svm, actor: usize) -> bool {
    let account = env.primary_portfolio(actor);
    account.capital.get() == 0
        && account.pnl.get() == 0
        && active_bitmap_is_empty(account.active_bitmap.map(|word| word.get()))
}

fn verify_flat_negative_final_leg_progress(
    route: TradeRoute,
) -> Result<FlatNegativeCrankOutcome, String> {
    const WINNER: usize = 0;
    const LOSER: usize = 1;
    const KEEPER: usize = 4;
    const ASSET: u16 = 0;
    const OPEN_PRICE: u64 = 100;
    const WINNING_PRICE: u64 = 150;
    const SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const LOSER_PRINCIPAL: u128 = 250;
    const TERMINAL_CLOSE_BOUND: usize = 16;

    let route_index = match route {
        TradeRoute::NoCpi => 0,
        TradeRoute::Cpi => 1,
        TradeRoute::BatchNoCpi => 2,
        TradeRoute::BatchCpi => 3,
    };
    let actor_deposits = [1_000, LOSER_PRINCIPAL, 1_000, 1, 1];
    let expected_payouts = actor_deposits.iter().sum();
    let mut seed = [0x71; 32];
    seed[0] ^= route_index;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: OPEN_PRICE,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits,
            ..MarketConfig::default()
        },
    );
    env.configure_permissionless_resolve(100, 1)
        .map_err(|error| format!("{route:?}: configure public resolution: {error}"))?;
    let token_supply_before = env.token_supply_observed();
    let destination_balances_before: u128 = env
        .actors
        .iter()
        .map(|actor| env.token_amount(actor.destination_token) as u128)
        .sum();
    env.begin_public_trace();

    let mut max_compute_units =
        execute_trade_route(&mut env, route, WINNER, LOSER, ASSET, SIZE_Q, OPEN_PRICE, 0)
            .map_err(|error| format!("{route:?}: open: {error}"))?
            .compute_units;
    let oracle_accounts = env.primary_profile(ASSET as usize).oracle_leg_count;
    let mut final_slot = 1;
    for (offset, mark) in (105u64..=WINNING_PRICE).step_by(5).enumerate() {
        final_slot = 2u64 + u64::try_from(offset).expect("bounded mark step");
        env.warp_to_slot(final_slot);
        max_compute_units = max_compute_units.max(
            env.push_auth_mark(ASSET, final_slot, mark)
                .map_err(|error| format!("{route:?}: publish mark {mark}: {error}"))?
                .compute_units,
        );
        max_compute_units = max_compute_units.max(
            env.crank(
                KEEPER,
                final_slot,
                vec![CrankObservationHint {
                    asset_index: ASSET,
                    oracle_accounts,
                }],
            )
            .map_err(|error| format!("{route:?}: observe mark {mark}: {error}"))?
            .compute_units,
        );
    }
    max_compute_units = max_compute_units.max(
        execute_trade_route(
            &mut env,
            route,
            WINNER,
            LOSER,
            ASSET,
            -SIZE_Q,
            WINNING_PRICE,
            0,
        )
        .map_err(|error| format!("{route:?}: final risk reduction: {error}"))?
        .compute_units,
    );

    let after_close = env.primary_portfolio(LOSER);
    let pending = after_close
        .close_progress
        .try_to_runtime()
        .map_err(|error| format!("{route:?}: decode pending close: {error:?}"))?;
    if after_close.capital.get() != 0
        || after_close.pnl.get() != -(LOSER_PRINCIPAL as i128)
        || !active_bitmap_is_empty(after_close.active_bitmap.map(|word| word.get()))
        || !pending.active
        || pending.finalized
        || pending.asset_index != ASSET as u32
        || pending.domain_side != SideV16::Long
        || pending.residual_remaining != LOSER_PRINCIPAL
    {
        return Err(format!(
            "{route:?}: final trade did not preserve the uniquely attributed residual: \
             capital={}, pnl={}, pending={pending:?}",
            after_close.capital.get(),
            after_close.pnl.get()
        ));
    }
    let residual_before = pending.residual_remaining;

    // AdvanceClose is state-derived and needs no oracle hint. It books the residual without
    // granting this account authority to terminate unrelated live market activity.
    max_compute_units = max_compute_units.max(
        env.crank(LOSER, final_slot, vec![])
            .map_err(|error| format!("{route:?}: advance residual: {error}"))?
            .compute_units,
    );
    let after_booking = env.primary_portfolio(LOSER);
    let booked = after_booking
        .close_progress
        .try_to_runtime()
        .map_err(|error| format!("{route:?}: decode booked close: {error:?}"))?;
    let (_, booked_group) = env.primary_market_state();
    if booked.residual_remaining >= residual_before
        || after_booking.pnl.get() != 0
        || booked_group.negative_pnl_account_count != 0
    {
        return Err(format!(
            "{route:?}: AdvanceClose did not strictly decrease rank: before={residual_before}, \
             after={}, pnl={}, negative_accounts={}",
            booked.residual_remaining,
            after_booking.pnl.get(),
            booked_group.negative_pnl_account_count
        ));
    }
    let residual_after = booked.residual_remaining;

    if env.primary_market_state().1.mode != MarketModeV16::Live {
        return Err(format!(
            "{route:?}: account residual forced market recovery"
        ));
    }
    let resolution_slot = final_slot
        .checked_add(100)
        .ok_or_else(|| format!("{route:?}: resolution slot overflow"))?;
    max_compute_units = max_compute_units.max(
        env.resolve_stale_permissionless(resolution_slot)
            .map_err(|error| format!("{route:?}: permissionless market resolution: {error}"))?
            .compute_units,
    );
    if env.primary_market_state().1.mode != MarketModeV16::Resolved {
        return Err(format!("{route:?}: stale market did not resolve"));
    }

    let mut terminal_close_calls = 0usize;
    // Close the bankrupt account first, then the B-bearing winner, then unrelated depositors.
    // Each public call is bounded; a pending B obligation can require more than one call.
    for actor in [LOSER, WINNER, 2, 3, KEEPER] {
        for _ in 0..TERMINAL_CLOSE_BOUND {
            if portfolio_is_economically_terminal(&env, actor) {
                break;
            }
            let close = env
                .close_resolved_primary_signed(actor)
                .map_err(|error| format!("{route:?}: close actor {actor}: {error}"))?;
            max_compute_units = max_compute_units.max(close.compute_units);
            terminal_close_calls += 1;
        }
        if !portfolio_is_economically_terminal(&env, actor) {
            return Err(format!(
                "{route:?}: actor {actor} did not terminate in {TERMINAL_CLOSE_BOUND} closes"
            ));
        }
    }

    let destination_payouts: u128 = env
        .actors
        .iter()
        .map(|actor| env.token_amount(actor.destination_token) as u128)
        .sum::<u128>()
        .checked_sub(destination_balances_before)
        .ok_or_else(|| format!("{route:?}: destination balance decreased"))?;
    if destination_payouts != expected_payouts
        || env.token_amount(env.vault) != 0
        || env.token_supply_observed() != token_supply_before
    {
        return Err(format!(
            "{route:?}: terminal value mismatch: payouts={destination_payouts}, \
             expected={expected_payouts}, vault={}, supply_before={token_supply_before}, \
             supply_after={}",
            env.token_amount(env.vault),
            env.token_supply_observed()
        ));
    }
    if max_compute_units >= TX_CU_LIMIT {
        return Err(format!(
            "{route:?}: required progress reached {max_compute_units} CU"
        ));
    }

    let trace = env.finish_public_trace();
    if trace.out_of_band_economic_mutations != 0
        || trace
            .steps
            .iter()
            .filter(|step| !step.succeeded)
            .any(|step| {
                step.rejected_exact_writable_rollback != Some(true)
                    || step.rejected_no_program_lamport_delta != Some(true)
                    || step.token_deltas.iter().any(|(_, delta)| *delta != 0)
            })
    {
        return Err(format!(
            "{route:?}: public trace violated atomicity: {trace:?}"
        ));
    }

    Ok(FlatNegativeCrankOutcome {
        route,
        residual_before,
        residual_after,
        terminal_close_calls,
        destination_payouts,
        expected_payouts,
        max_compute_units,
        trace_steps: trace.steps.len(),
    })
}

#[test]
fn v16_program_flat_negative_final_leg_route_matrix_reaches_terminal_payout() {
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        let outcome = verify_flat_negative_final_leg_progress(route)
            .unwrap_or_else(|error| panic!("INV-071: {error}"));
        assert_eq!(outcome.route, route);
        assert!(outcome.residual_before > outcome.residual_after);
        assert!(outcome.terminal_close_calls >= PRIMARY_ACTOR_COUNT);
        assert_eq!(outcome.destination_payouts, outcome.expected_payouts);
        assert!(outcome.max_compute_units < TX_CU_LIMIT);
        assert!(outcome.trace_steps >= 20);
    }
}
