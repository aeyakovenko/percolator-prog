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
//! Guarantee boundary: these are the same-asset risk-reduction and two-asset/two-account close
//! cells. Risk increase while a domain loss barrier is active is intentionally outside the
//! guarantee; broader side/domain/lifecycle combinations remain open.

use super::inv_052_split_merge_invariance::run_resolved_claim_partition;
use crate::support::fuzz_model::{
    assert_public_encumbrance_census, assert_public_stock_census, execute_trade_route,
    run_active_close_shutdown_liveness_probe, run_concurrent_close_locality_probe,
    run_same_asset_close_locality_probe, TradeRoute,
};
use crate::support::v16_svm::{MarketConfig, V16Svm, INITIAL_PRICE, TX_CU_LIMIT};
use percolator::{AssetLifecycleV16, SideModeV16, POS_SCALE};
use percolator_prog::ix::CrankObservationHint;

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
