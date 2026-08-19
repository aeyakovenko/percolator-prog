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
//! policy then begins terminal settlement. The bounded terminal matrix varies the winner/loser
//! claimant order, whether `CloseResolved` or `ClaimResolvedPayoutTopup` is attempted first, and the
//! original trade route. Every complete sweep must make progress, every rejected call must roll
//! back exactly, receipts must be monotonic, and all funded portfolios must dematerialize before
//! `CloseSlab` empties the real SPL vault. Per-owner payouts are invariant across every order.
//! A second focused route drives the shared stateful rank oracle through the same public
//! `AdvanceClose` class and requires the residual itself to be a lexicographically decreasing rank
//! component; aggregate market lock bits alone are not a sufficient progress measure.
//! `v16_program_cured_close_releases_counterparty_obligation` reaches a cancellable bankruptcy
//! close entirely through public trade, oracle, crank, and cure instructions. It requires the
//! winner's released zero-basis loss obligation to be detached by a bounded permissionless crank;
//! a successful byte-identical crank while the obligation and market lock remain is a concrete
//! liveness violation, not progress.
//!
//! Guarantee boundary: this is deployed LiteSVM evidence for the four wrapper trade routes and the
//! uniquely attributable one-asset residual class. The engine's production selector and rank
//! proofs cover the local transition; bounded reachability across every lifecycle class remains
//! shared with INV-082.

use super::*;
use crate::support::{
    fuzz_model::{
        assert_public_encumbrance_census, assert_public_stock_census, execute_trade_route,
        run_cure_pending_obligation_dos_probe, run_pending_close_rank_oracle,
    },
    v16_svm::{MarketConfig, V16Svm, PRIMARY_ACTOR_COUNT, TX_CU_LIMIT},
};
use percolator::{active_bitmap_is_empty, MarketModeV16, SideV16, POS_SCALE};
use percolator_prog::ix::CrankObservationHint;

#[derive(Clone, Debug)]
struct FlatNegativeCrankOutcome {
    route: TradeRoute,
    claimant_order: [usize; PRIMARY_ACTOR_COUNT],
    claim_first: bool,
    residual_before: u128,
    residual_after: u128,
    pending_obligation_count: u64,
    terminal_close_calls: usize,
    destination_payouts: [u128; PRIMARY_ACTOR_COUNT],
    expected_payouts: u128,
    max_compute_units: u64,
    trace_steps: usize,
}

#[test]
fn v16_program_pending_close_residual_is_part_of_the_public_crank_rank() {
    let evidence = run_pending_close_rank_oracle()
        .expect("public AdvanceClose must strictly reduce the stateful liveness rank");
    assert!(evidence.residual_before > evidence.residual_after);
    assert!(evidence.coverage.crank_progress > 0, "{evidence:?}");
}

#[test]
fn v16_program_cured_close_releases_counterparty_obligation() {
    let evidence = run_cure_pending_obligation_dos_probe()
        .expect("a public cure must leave a bounded crank path for the counterparty obligation");
    assert!(evidence.close_canceled, "{evidence:?}");
    assert_eq!(evidence.pending_obligation_count, 0, "{evidence:?}");
    assert_eq!(evidence.retained_basis_pos_q, 0, "{evidence:?}");
    assert_eq!(evidence.retained_loss_weight, 0, "{evidence:?}");
    assert!(evidence.progressing_cranks > 0, "{evidence:?}");
    assert_eq!(evidence.successful_noop_cranks, 0, "{evidence:?}");
    assert!(!evidence.unrelated_trade_rejected, "{evidence:?}");
    assert!(!evidence.owner_withdraw_rejected, "{evidence:?}");
    if evidence.bankruptcy_hlock_active {
        assert!(
            !evidence.unrelated_trade_rejected && !evidence.owner_withdraw_rejected,
            "a surviving aggregate lock must remain scope-local: {evidence:?}"
        );
    }
}

fn portfolio_is_economically_terminal(env: &V16Svm, actor: usize) -> bool {
    let group = env.primary_market_state().1;
    let account = env.primary_portfolio(actor);
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
        && active_bitmap_is_empty(account.active_bitmap.map(|word| word.get()))
        && account.stale_state == 0
        && account.b_stale_state == 0
        && account.rebalance_lock == 0
        && account.liquidation_lock == 0
        && account.last_fee_slot.get() == group.resolved_slot
        && account.health_cert.valid == 0
        && account
            .source_domains
            .iter()
            .all(|source| !source.is_occupied())
        && (!receipt.present || receipt.finalized)
        && (!close.active || (close.finalized && close.residual_remaining == 0))
}

#[derive(Clone, Copy, Debug)]
enum TerminalRoute {
    Close,
    Claim,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TerminalSnapshot {
    market: Vec<u8>,
    foreign_market: Vec<u8>,
    portfolios: Vec<Vec<u8>>,
    foreign_portfolio: Vec<u8>,
    backing_ledger: Vec<u8>,
    tokens: Vec<(solana_sdk::pubkey::Pubkey, Vec<u8>)>,
    lamports: Vec<(solana_sdk::pubkey::Pubkey, u64)>,
}

fn terminal_snapshot(env: &V16Svm) -> TerminalSnapshot {
    TerminalSnapshot {
        market: env.market_data(false),
        foreign_market: env.market_data(true),
        portfolios: env.all_primary_portfolio_data(),
        foreign_portfolio: env.foreign_portfolio_data(),
        backing_ledger: env.backing_domain_ledger_data(),
        tokens: env.all_token_account_data(),
        lamports: env.all_economic_account_lamports(),
    }
}

fn attempt_terminal_route(
    env: &mut V16Svm,
    actor: usize,
    route: TerminalRoute,
    max_compute_units: &mut u64,
) -> Result<(bool, bool, u64), String> {
    let before = terminal_snapshot(env);
    let before_group = env.primary_market_state().1;
    let before_account = env.primary_portfolio(actor);
    let before_receipt = before_account
        .resolved_payout_receipt
        .try_to_runtime()
        .map_err(|error| format!("decode actor {actor} pre-receipt: {error:?}"))?;
    let destination = env.actors[actor].destination_token;
    let destination_before = env.token_amount(destination);
    let spl_vault_before = env.token_amount(env.vault);

    let result = match route {
        TerminalRoute::Close => env.close_resolved_primary_signed(actor),
        TerminalRoute::Claim => env.claim_resolved_payout_topup_primary(actor),
    };
    let Ok(success) = result else {
        if terminal_snapshot(env) != before {
            return Err(format!(
                "actor {actor} rejected {route:?} mutated program bytes, SPL balances, or lamports"
            ));
        }
        return Ok((false, false, 0));
    };
    *max_compute_units = (*max_compute_units).max(success.compute_units);
    if success.compute_units >= TX_CU_LIMIT {
        return Err(format!(
            "actor {actor} {route:?} reached {} CU",
            success.compute_units
        ));
    }

    let after = terminal_snapshot(env);
    let after_group = env.primary_market_state().1;
    let after_account = env.primary_portfolio(actor);
    let after_receipt = after_account
        .resolved_payout_receipt
        .try_to_runtime()
        .map_err(|error| format!("decode actor {actor} post-receipt: {error:?}"))?;
    let destination_after = env.token_amount(destination);
    let spl_vault_after = env.token_amount(env.vault);
    let payout = destination_after
        .checked_sub(destination_before)
        .ok_or_else(|| format!("actor {actor} {route:?} decreased its destination"))?;
    let spl_debit = spl_vault_before
        .checked_sub(spl_vault_after)
        .ok_or_else(|| format!("actor {actor} {route:?} increased the SPL vault"))?;
    let engine_debit = before_group
        .vault
        .checked_sub(after_group.vault)
        .ok_or_else(|| format!("actor {actor} {route:?} increased the engine vault"))?;
    if payout != spl_debit || u128::from(payout) != engine_debit {
        return Err(format!(
            "actor {actor} {route:?} payout mismatch: destination={payout}, SPL debit={spl_debit}, engine debit={engine_debit}"
        ));
    }
    if after_receipt.present {
        if after_receipt.paid_effective > after_receipt.terminal_positive_claim_face
            || after_receipt.finalized
                != (after_receipt.paid_effective == after_receipt.terminal_positive_claim_face)
        {
            return Err(format!(
                "actor {actor} {route:?} produced an invalid receipt: {after_receipt:?}"
            ));
        }
        if before_receipt.present
            && (after_receipt.prior_bound_contribution_num
                != before_receipt.prior_bound_contribution_num
                || after_receipt.terminal_positive_claim_face
                    != before_receipt.terminal_positive_claim_face
                || after_receipt.paid_effective < before_receipt.paid_effective)
        {
            return Err(format!(
                "actor {actor} {route:?} rewrote or rolled back its receipt: {before_receipt:?} -> {after_receipt:?}"
            ));
        }
    }

    assert_public_stock_census(&format!("INV-071 actor {actor} {route:?} stock"), env)?;
    assert_public_encumbrance_census(&format!("INV-071 actor {actor} {route:?} encumbrance"), env)?;
    Ok((true, after != before, payout))
}

fn drain_terminal_order(
    env: &mut V16Svm,
    claimant_order: [usize; PRIMARY_ACTOR_COUNT],
    claim_first: bool,
    max_compute_units: &mut u64,
) -> Result<(usize, u128), String> {
    const TERMINAL_SWEEP_BOUND: usize = 64;
    let route_order = if claim_first {
        [TerminalRoute::Claim, TerminalRoute::Close]
    } else {
        [TerminalRoute::Close, TerminalRoute::Claim]
    };
    let mut close_calls = 0usize;
    let mut claim_payouts = 0u128;
    let mut reached_fixed_point = false;
    for round in 0..TERMINAL_SWEEP_BOUND {
        let before_round = terminal_snapshot(env);
        let mut every_route_quiescent = true;
        for actor in claimant_order {
            for route in route_order {
                let (landed, mutated, payout) =
                    attempt_terminal_route(env, actor, route, max_compute_units)?;
                every_route_quiescent &= landed && !mutated && payout == 0;
                match (route, landed && mutated) {
                    (TerminalRoute::Close, true) => close_calls += 1,
                    (TerminalRoute::Claim, true) => {
                        claim_payouts = claim_payouts
                            .checked_add(u128::from(payout))
                            .ok_or("terminal claim payout overflow")?;
                    }
                    _ => {}
                }
            }
        }
        if terminal_snapshot(env) == before_round {
            if !every_route_quiescent {
                return Err(format!(
                    "terminal sweep {round} was snapshot-stable only because at least one payout route rejected or was not individually quiescent; order={claimant_order:?}, claim_first={claim_first}"
                ));
            }
            if !(0..PRIMARY_ACTOR_COUNT).all(|actor| portfolio_is_economically_terminal(env, actor))
            {
                return Err(format!(
                    "terminal sweep {round} reached a public-route fixed point with funded or nonterminal actors; order={claimant_order:?}, claim_first={claim_first}"
                ));
            }
            reached_fixed_point = true;
            break;
        }
    }
    if !reached_fixed_point {
        return Err(format!(
            "terminal payout routes did not reach a fixed point in {TERMINAL_SWEEP_BOUND} sweeps; order={claimant_order:?}, claim_first={claim_first}"
        ));
    }
    let blocked: Vec<_> = (0..PRIMARY_ACTOR_COUNT)
        .filter(|actor| !portfolio_is_economically_terminal(env, *actor))
        .collect();
    if !blocked.is_empty() {
        return Err(format!(
            "actors {blocked:?} did not reach terminal disposition in {TERMINAL_SWEEP_BOUND} sweeps"
        ));
    }

    let fixed_point = terminal_snapshot(env);
    for actor in claimant_order {
        for route in [TerminalRoute::Close, TerminalRoute::Claim] {
            let (landed, mutated, payout) =
                attempt_terminal_route(env, actor, route, max_compute_units)?;
            if !landed || mutated || payout != 0 {
                return Err(format!(
                    "actor {actor} {route:?} was not callable and quiescent at the asserted terminal fixed point"
                ));
            }
        }
    }
    if terminal_snapshot(env) != fixed_point {
        return Err(format!(
            "terminal payout fixed point paid twice or mutated state; order={claimant_order:?}"
        ));
    }
    Ok((close_calls, claim_payouts))
}

fn verify_flat_negative_final_leg_progress(
    route: TradeRoute,
    claimant_order: [usize; PRIMARY_ACTOR_COUNT],
    claim_first: bool,
) -> Result<FlatNegativeCrankOutcome, String> {
    const WINNER: usize = 0;
    const LOSER: usize = 1;
    const KEEPER: usize = 4;
    const ASSET: u16 = 0;
    const OPEN_PRICE: u64 = 100;
    const WINNING_PRICE: u64 = 150;
    const SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const LOSER_PRINCIPAL: u128 = 250;

    let route_index = match route {
        TradeRoute::NoCpi => 0,
        TradeRoute::Cpi => 1,
        TradeRoute::BatchNoCpi => 2,
        TradeRoute::BatchCpi => 3,
    };
    let actor_deposits = [1_000, LOSER_PRINCIPAL, 1_000, 1, 1];
    let expected_destination_payouts = [1_250, 0, 1_000, 1, 1];
    let expected_payouts = actor_deposits.iter().sum();
    let mut seed = [0x71; 32];
    seed[0] ^= route_index;
    seed[1] ^= u8::from(claimant_order[0] == WINNER);
    seed[2] ^= u8::from(claim_first);
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
    let destination_balances_before: [u128; PRIMARY_ACTOR_COUNT] = std::array::from_fn(|actor| {
        u128::from(env.token_amount(env.actors[actor].destination_token))
    });
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
    let after_close_group = env.primary_market_state().1;
    let after_close_asset = after_close_group.assets[ASSET as usize];
    let after_close_stored_count = after_close_asset
        .stored_pos_count_long
        .checked_add(after_close_asset.stored_pos_count_short)
        .ok_or_else(|| format!("{route:?}: stored-position count overflow"))?;
    let after_close_pending_count = after_close_asset
        .pending_obligation_count_long
        .checked_add(after_close_asset.pending_obligation_count_short)
        .ok_or_else(|| format!("{route:?}: pending-obligation count overflow"))?;
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
        || after_close_asset.oi_eff_long_q != 0
        || after_close_asset.oi_eff_short_q != 0
        || after_close_stored_count != 1
        || after_close_pending_count != 1
    {
        return Err(format!(
            "{route:?}: final trade did not preserve the uniquely attributed residual: \
             capital={}, pnl={}, pending={pending:?}, oi={}/{}, stored={}/{}, obligations={}/{}",
            after_close.capital.get(),
            after_close.pnl.get(),
            after_close_asset.oi_eff_long_q,
            after_close_asset.oi_eff_short_q,
            after_close_asset.stored_pos_count_long,
            after_close_asset.stored_pos_count_short,
            after_close_asset.pending_obligation_count_long,
            after_close_asset.pending_obligation_count_short,
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
        || booked_group.assets[ASSET as usize].oi_eff_long_q != 0
        || booked_group.assets[ASSET as usize].oi_eff_short_q != 0
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
    let pending_obligation_count = booked_group.assets[ASSET as usize]
        .pending_obligation_count_long
        .checked_add(booked_group.assets[ASSET as usize].pending_obligation_count_short)
        .ok_or_else(|| format!("{route:?}: pending-obligation count overflow"))?;
    if pending_obligation_count == 0 {
        return Err(format!(
            "{route:?}: bankruptcy fixture did not create a pending obligation"
        ));
    }
    assert_public_stock_census("INV-071 after bankruptcy booking", &env)?;
    assert_public_encumbrance_census("INV-071 after bankruptcy booking", &env)?;

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

    assert_public_stock_census("INV-071 after public resolution", &env)?;
    assert_public_encumbrance_census("INV-071 after public resolution", &env)?;
    let vault_at_resolution = u128::from(env.token_amount(env.vault));
    let (terminal_close_calls, terminal_claim_payouts) = drain_terminal_order(
        &mut env,
        claimant_order,
        claim_first,
        &mut max_compute_units,
    )?;
    let destination_payouts = std::array::from_fn(|actor| {
        u128::from(env.token_amount(env.actors[actor].destination_token))
            .checked_sub(destination_balances_before[actor])
            .expect("terminal destination cannot decrease")
    });
    let total_destination_payouts = destination_payouts
        .iter()
        .try_fold(0u128, |sum, payout| sum.checked_add(*payout))
        .ok_or_else(|| format!("{route:?}: destination payout total overflow"))?;
    let final_vault = u128::from(env.token_amount(env.vault));
    if destination_payouts != expected_destination_payouts
        || total_destination_payouts != expected_payouts
        || total_destination_payouts != vault_at_resolution
        || terminal_claim_payouts > total_destination_payouts
        || final_vault != 0
        || env.token_supply_observed() != token_supply_before
    {
        return Err(format!(
            "{route:?}: terminal value mismatch: payouts={destination_payouts:?}, \
             expected_by_owner={expected_destination_payouts:?}, total={total_destination_payouts}, expected={expected_payouts}, \
             resolution_vault={vault_at_resolution}, final_vault={final_vault}, \
             claim_payouts={terminal_claim_payouts}, supply_before={token_supply_before}, supply_after={}",
            env.token_supply_observed()
        ));
    }
    let terminal = env.primary_market_state().1;
    if terminal.vault != 0
        || terminal.c_tot != 0
        || terminal.materialized_portfolio_count != PRIMARY_ACTOR_COUNT as u64
        || terminal.assets[ASSET as usize].pending_obligation_count_long != 0
        || terminal.assets[ASSET as usize].pending_obligation_count_short != 0
        || terminal.assets[ASSET as usize].oi_eff_long_q != 0
        || terminal.assets[ASSET as usize].oi_eff_short_q != 0
        || terminal.assets[ASSET as usize].stored_pos_count_long != 0
        || terminal.assets[ASSET as usize].stored_pos_count_short != 0
    {
        return Err(format!(
            "{route:?}: terminal engine state retained obligations: vault={}, c_tot={}, materialized={}, pending={}/{}, stored={}/{}",
            terminal.vault,
            terminal.c_tot,
            terminal.materialized_portfolio_count,
            terminal.assets[ASSET as usize].pending_obligation_count_long,
            terminal.assets[ASSET as usize].pending_obligation_count_short,
            terminal.assets[ASSET as usize].stored_pos_count_long,
            terminal.assets[ASSET as usize].stored_pos_count_short,
        ));
    }
    assert_public_stock_census("INV-071 terminal stock", &env)?;
    assert_public_encumbrance_census("INV-071 terminal encumbrance", &env)?;
    for actor in claimant_order {
        env.close_primary_portfolio(actor)
            .map_err(|error| format!("{route:?}: close terminal portfolio {actor}: {error}"))?;
    }
    let slab_close = env
        .close_primary_slab()
        .map_err(|error| format!("{route:?}: close terminal slab: {error}"))?;
    max_compute_units = max_compute_units.max(slab_close.compute_units);
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
        claimant_order,
        claim_first,
        residual_before,
        residual_after,
        pending_obligation_count,
        terminal_close_calls,
        destination_payouts,
        expected_payouts,
        max_compute_units,
        trace_steps: trace.steps.len(),
    })
}

#[test]
fn v16_program_flat_negative_final_leg_route_matrix_reaches_terminal_payout() {
    let claimant_orders = [[1, 0, 2, 3, 4], [0, 1, 2, 3, 4], [4, 3, 2, 1, 0]];
    let mut baseline = None;
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        for claimant_order in claimant_orders {
            for claim_first in [false, true] {
                let outcome =
                    verify_flat_negative_final_leg_progress(route, claimant_order, claim_first)
                        .unwrap_or_else(|error| panic!("INV-071: {error}"));
                assert_eq!(outcome.route, route);
                assert_eq!(outcome.claimant_order, claimant_order);
                assert_eq!(outcome.claim_first, claim_first);
                assert!(outcome.residual_before > outcome.residual_after);
                assert!(outcome.pending_obligation_count > 0);
                assert!(outcome.terminal_close_calls >= PRIMARY_ACTOR_COUNT);
                assert_eq!(
                    outcome.destination_payouts.iter().sum::<u128>(),
                    outcome.expected_payouts
                );
                assert!(outcome.max_compute_units < TX_CU_LIMIT);
                assert!(outcome.trace_steps >= 20);
                if let Some(expected) = baseline {
                    assert_eq!(
                        outcome.destination_payouts, expected,
                        "trade, claimant, or public payout-route order changed owner payouts for {route:?}"
                    );
                } else {
                    baseline = Some(outcome.destination_payouts);
                }
            }
        }
    }
}
