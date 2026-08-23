//! INV-065 - Reset, recovery, and retired-state isolation.
//!
//! Normative obligation: lifecycle transitions cannot admit new risk into an
//! inconsistent episode or orphan existing user legs.
//!
//! Evidence in this file (F over public I routes): the shared stateful runner
//! configures permissionless recovery, shuts down a publicly active asset, and
//! then runs independent permissionless-progress and owner-exit campaigns. The
//! shutdown must be a real successful wrapper transition; every later success
//! is checked against the complete position/OI/source-credit/custody model, and
//! every rejection must roll back all tracked program bytes, SPL data, and
//! economic-account lamports. Generated scenarios also place ordinary public
//! actions before and after these lifecycle actions.
//! `v16_program_unilateral_zero_oi_reset_route_side_matrix_finalizes_permissionlessly`
//! reaches `ResetPending` without state injection in all sixteen
//! base/dynamic-asset/trade-route/reducer-side worlds: one owner fully reduces a matched leg, an
//! early global finalizer rejects atomically while the prior-epoch counterparty leg remains,
//! bounded public auto-cranks detach that leg, and the permissionless finalizer restores Normal. A
//! fresh matched open/close through the same route and full owner withdrawals prove the reset does
//! not orphan funds or permanently disable the asset. Each dynamic-asset world additionally enters
//! DrainOnly and Retired, proving reset history cannot become a terminal retirement lock.
//! `v16_program_shutdown_during_reset_pending_retains_permissionless_progress` covers another
//! sixteen public worlds: all four trade routes, both reset sides, and a stale pre-shutdown oracle
//! hint either absent or landing after shutdown. Recovery must preserve the prior-generation reset
//! obligation as an immediately crankable action, ignore the now-inapplicable external observation,
//! detach the stale leg, finalize and restart with a new generation, admit a fresh same-route
//! roundtrip, and return every owner's capital. This is the wrapper composition regression for the
//! engine selector/dispatch mismatch fixed by engine commit `7387e7a9`.
//! `v16_program_shutdown_after_reset_cleanup_is_order_safe` closes the adjacent phase boundary:
//! shutdown lands after the prior-epoch leg has detached, either immediately before or immediately
//! after `FinalizeResetSide`. All four trade routes and both reset sides must converge through
//! Recovery, reject a retained old-generation trade without mutation, admit the same fresh request
//! against the restarted generation, and return every owner's capital.
//! `v16_program_retained_reduction_landing_after_shutdown_has_a_bounded_recovery_fallback` covers
//! the reverse ordering. An owner signs a complete unilateral reduction while Active. It lands
//! before or after shutdown; a post-shutdown rejection must frame the complete economic state, and
//! the canonical owner Recovery operations must then remove both legs without sacrificing senior
//! capital. Both schedules restart the asset, admit a fresh same-route roundtrip, and converge on
//! identical owner payouts.
//! `v16_program_two_asset_reset_recovery_orders_progress_without_crossing_scope` composes two
//! simultaneous reset/Recovery episodes. It exhausts every pair of trade routes, both side
//! orientations on each asset, and both lifecycle orders. Each successful transition must frame
//! the other asset's profile, users, matcher state, backing ledger, and SPL accounts before both
//! episodes restart and all four users exit with order-independent payouts.
//!
//! This is bounded generated coverage, not exhaustive reset/recovery/retirement
//! reachability.

use super::*;
use crate::support::fuzz_model::{
    assert_public_encumbrance_census, assert_public_stock_census, execute_trade_route,
};
use crate::support::v16_svm::{MarketConfig, V16Svm, INITIAL_PRICE, TX_CU_LIMIT};
use percolator::{AssetLifecycleV16, SideModeV16, POS_SCALE};
use percolator_prog::ix::CrankObservationHint;

#[test]
fn v16_program_generated_shutdown_reaches_recovery_then_all_positions_exit() {
    let scenario = Scenario {
        seed: [0x65; 32],
        config: SmallMarketConfig::default(),
        actions: vec![
            Action::ConfigurePermissionlessResolve {
                stale_slots: 1_000,
                force_close_delay_slots: 100,
            },
            Action::ShutdownAsset { asset: 0, dt: 0 },
        ],
    };

    let coverage = run_scenario(&scenario)
        .expect("public recovery transition must preserve bounded progress and owner exits");
    assert_ne!(
        coverage.resolve_policy_updates, 0,
        "the recovery policy must be installed through the public wrapper"
    );
    assert_ne!(
        coverage.lifecycle_updates, 0,
        "the active asset must enter Recovery through the public wrapper"
    );
    assert_ne!(
        coverage.user_positions_closed, 0,
        "the post-shutdown owner-exit campaign must clear live positions"
    );
    assert!(
        coverage
            .known_blocker_hits
            .iter()
            .chain(coverage.known_blocker_exit_locks.iter())
            .all(|hits| *hits == 0),
        "the lifecycle witness must not rely on blocker quarantine: {coverage:?}"
    );
}

fn assert_public_reset_lifecycle(
    route: TradeRoute,
    reducer_long: bool,
    asset_index: u16,
    seed: [u8; 32],
) {
    const REDUCER: usize = 0;
    const STALE_COUNTERPARTY: usize = 1;
    const FRESH_LONG: usize = 2;
    const FRESH_SHORT: usize = 3;
    const SIZE_Q: u128 = POS_SCALE;

    let signed_size_q = if reducer_long {
        SIZE_Q as i128
    } else {
        -(SIZE_Q as i128)
    };
    let reset_side = u8::from(reducer_long);
    let case = format!("{route:?}/asset={asset_index}/reducer_long={reducer_long}");
    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply_before = env.token_supply_observed();
    let reducer_destination_before = env.token_amount(env.actors[REDUCER].destination_token);
    let counterparty_destination_before =
        env.token_amount(env.actors[STALE_COUNTERPARTY].destination_token);
    let mut max_compute_units = execute_trade_route(
        &mut env,
        route,
        REDUCER,
        STALE_COUNTERPARTY,
        asset_index,
        signed_size_q,
        INITIAL_PRICE,
        0,
    )
    .unwrap_or_else(|error| {
        panic!("{case}: public matched open must establish both side episodes: {error}")
    })
    .compute_units;
    assert_public_stock_census("INV-065 before unilateral reset", &env)
        .expect("matched open stock census");
    assert_public_encumbrance_census("INV-065 before unilateral reset", &env)
        .expect("matched open encumbrance census");

    let reduce = env
        .rebalance_reduce(REDUCER, asset_index, SIZE_Q)
        .expect("full owner reduction must enter the opposite-side reset episode");
    max_compute_units = max_compute_units.max(reduce.compute_units);
    let reset = env.primary_market_state().1.assets[asset_index as usize];
    assert_eq!(reset.oi_eff_long_q, 0);
    assert_eq!(reset.oi_eff_short_q, 0);
    let (reset_mode, reset_stored_count) = if reducer_long {
        (reset.mode_short, reset.stored_pos_count_short)
    } else {
        (reset.mode_long, reset.stored_pos_count_long)
    };
    assert_eq!(reset_mode, SideModeV16::ResetPending, "{case}");
    assert_eq!(reset_stored_count, 1, "{case}");
    assert_public_stock_census("INV-065 after unilateral reset begins", &env)
        .expect("reset-begin stock census");
    assert_public_encumbrance_census("INV-065 after unilateral reset begins", &env)
        .expect("reset-begin encumbrance census");

    let market_before_early_finalize = env.market_data(false);
    let portfolios_before_early_finalize = (0..env.actors.len())
        .map(|actor| env.primary_portfolio_data(actor))
        .collect::<Vec<_>>();
    let tokens_before_early_finalize = env.all_token_account_data();
    env.finalize_reset_side(asset_index, reset_side)
        .expect_err("the old counterparty leg must block premature reset finalization");
    assert_eq!(env.market_data(false), market_before_early_finalize);
    assert_eq!(
        (0..env.actors.len())
            .map(|actor| env.primary_portfolio_data(actor))
            .collect::<Vec<_>>(),
        portfolios_before_early_finalize
    );
    assert_eq!(env.all_token_account_data(), tokens_before_early_finalize);

    let observation = vec![CrankObservationHint {
        asset_index,
        oracle_accounts: env.primary_profile(asset_index as usize).oracle_leg_count,
    }];
    let mut crank_calls = 0usize;
    for _ in 0..8 {
        let asset = env.primary_market_state().1.assets[asset_index as usize];
        let stored_count = if reducer_long {
            asset.stored_pos_count_short
        } else {
            asset.stored_pos_count_long
        };
        if stored_count == 0 {
            break;
        }
        let crank = env
            .crank(STALE_COUNTERPARTY, env.current_slot(), observation.clone())
            .expect("the prior-epoch counterparty leg must have bounded permissionless progress");
        max_compute_units = max_compute_units.max(crank.compute_units);
        crank_calls += 1;
        assert_public_stock_census("INV-065 during stale-leg detach", &env)
            .expect("stale-leg stock census");
        assert_public_encumbrance_census("INV-065 during stale-leg detach", &env)
            .expect("stale-leg encumbrance census");
    }
    assert!(
        crank_calls > 0,
        "reset cleanup must execute real public work"
    );
    let ready = env.primary_market_state().1.assets[asset_index as usize];
    let (stored_count, stale_count, pending_count) = if reducer_long {
        (
            ready.stored_pos_count_short,
            ready.stale_account_count_short,
            ready.pending_obligation_count_short,
        )
    } else {
        (
            ready.stored_pos_count_long,
            ready.stale_account_count_long,
            ready.pending_obligation_count_long,
        )
    };
    assert_eq!(stored_count, 0, "{case}");
    assert_eq!(stale_count, 0, "{case}");
    assert_eq!(pending_count, 0, "{case}");
    assert!(
        env.primary_portfolio(STALE_COUNTERPARTY)
            .active_bitmap
            .iter()
            .all(|word| word.get() == 0),
        "permissionless cleanup must detach the old side episode"
    );

    let risk_epoch_before = env.primary_market_state().1.risk_epoch;
    let finalize = env
        .finalize_reset_side(asset_index, reset_side)
        .expect("a clean ResetPending side must finalize permissionlessly");
    max_compute_units = max_compute_units.max(finalize.compute_units);
    let finalized = env.primary_market_state().1;
    let finalized_mode = if reducer_long {
        finalized.assets[asset_index as usize].mode_short
    } else {
        finalized.assets[asset_index as usize].mode_long
    };
    assert_eq!(finalized_mode, SideModeV16::Normal, "{case}");
    assert!(finalized.risk_epoch > risk_epoch_before);
    assert_public_stock_census("INV-065 after reset finalization", &env)
        .expect("finalized reset stock census");
    assert_public_encumbrance_census("INV-065 after reset finalization", &env)
        .expect("finalized reset encumbrance census");

    let reopen = execute_trade_route(
        &mut env,
        route,
        FRESH_LONG,
        FRESH_SHORT,
        asset_index,
        signed_size_q,
        INITIAL_PRICE,
        0,
    )
    .unwrap_or_else(|error| {
        panic!("{case}: finalized side must admit fresh matched risk: {error}")
    });
    max_compute_units = max_compute_units.max(reopen.compute_units);
    let reclose = execute_trade_route(
        &mut env,
        route,
        FRESH_LONG,
        FRESH_SHORT,
        asset_index,
        -signed_size_q,
        INITIAL_PRICE,
        0,
    )
    .unwrap_or_else(|error| {
        panic!("{case}: fresh matched risk must retain a bounded bilateral exit: {error}")
    });
    max_compute_units = max_compute_units.max(reclose.compute_units);
    let after_fresh_close = env.primary_market_state().1.assets[asset_index as usize];
    assert_eq!(after_fresh_close.oi_eff_long_q, 0);
    assert_eq!(after_fresh_close.oi_eff_short_q, 0);
    assert_public_stock_census("INV-065 after fresh post-reset roundtrip", &env)
        .expect("post-reset roundtrip stock census");
    assert_public_encumbrance_census("INV-065 after fresh post-reset roundtrip", &env)
        .expect("post-reset roundtrip encumbrance census");

    if asset_index != 0 {
        let generation_before_retire = after_fresh_close.market_id;
        let drain = env
            .drain_only_asset(asset_index, 0)
            .expect("an empty post-reset asset must enter DrainOnly");
        max_compute_units = max_compute_units.max(drain.compute_units);
        assert_eq!(
            env.primary_market_state().1.assets[asset_index as usize].lifecycle,
            AssetLifecycleV16::DrainOnly,
            "{case}"
        );
        let retire_slot = env.current_slot().checked_add(1).expect("bounded slot");
        env.warp_to_slot(retire_slot);
        let retire = env
            .retire_asset(asset_index, retire_slot)
            .expect("reset history must not block empty-asset retirement");
        max_compute_units = max_compute_units.max(retire.compute_units);
        let retired = env.primary_market_state().1.assets[asset_index as usize];
        assert_eq!(retired.lifecycle, AssetLifecycleV16::Retired, "{case}");
        assert_eq!(retired.market_id, generation_before_retire, "{case}");
        assert_eq!(retired.oi_eff_long_q, 0, "{case}");
        assert_eq!(retired.oi_eff_short_q, 0, "{case}");
        assert_eq!(retired.stored_pos_count_long, 0, "{case}");
        assert_eq!(retired.stored_pos_count_short, 0, "{case}");
        assert_public_stock_census("INV-065 after reset-history retirement", &env)
            .expect("reset-history retirement stock census");
        assert_public_encumbrance_census("INV-065 after reset-history retirement", &env)
            .expect("reset-history retirement encumbrance census");
    }

    let reducer_capital = env.primary_portfolio(REDUCER).capital.get();
    let counterparty_capital = env.primary_portfolio(STALE_COUNTERPARTY).capital.get();
    let reducer_withdraw = env
        .withdraw_primary(REDUCER, reducer_capital)
        .expect("the reducing owner must recover all remaining capital");
    max_compute_units = max_compute_units.max(reducer_withdraw.compute_units);
    let counterparty_withdraw = env
        .withdraw_primary(STALE_COUNTERPARTY, counterparty_capital)
        .expect("the detached counterparty must recover all remaining capital");
    max_compute_units = max_compute_units.max(counterparty_withdraw.compute_units);
    for actor in [FRESH_LONG, FRESH_SHORT] {
        let capital = env.primary_portfolio(actor).capital.get();
        let withdraw = env
            .withdraw_primary(actor, capital)
            .expect("a flat post-reset portfolio must retain its complete public exit");
        max_compute_units = max_compute_units.max(withdraw.compute_units);
    }
    assert_eq!(
        env.token_amount(env.actors[REDUCER].destination_token),
        reducer_destination_before + reducer_capital as u64
    );
    assert_eq!(
        env.token_amount(env.actors[STALE_COUNTERPARTY].destination_token),
        counterparty_destination_before + counterparty_capital as u64
    );
    assert_eq!(env.token_supply_observed(), supply_before);
    assert_public_stock_census("INV-065 after reset owner exits", &env)
        .expect("owner-exit stock census");
    assert_public_encumbrance_census("INV-065 after reset owner exits", &env)
        .expect("owner-exit encumbrance census");
    assert!(
        max_compute_units < TX_CU_LIMIT,
        "{case}: {max_compute_units}"
    );
}

fn assert_shutdown_during_reset_pending(
    route: TradeRoute,
    reducer_long: bool,
    include_stale_hint: bool,
    seed: [u8; 32],
) {
    const REDUCER: usize = 0;
    const STALE_COUNTERPARTY: usize = 1;
    const FRESH_LONG: usize = 2;
    const FRESH_SHORT: usize = 3;
    const ASSET: u16 = 0;
    const SIZE_Q: u128 = POS_SCALE;

    let signed_size_q = if reducer_long {
        SIZE_Q as i128
    } else {
        -(SIZE_Q as i128)
    };
    let reset_side = u8::from(reducer_long);
    let case = format!("{route:?}/reducer_long={reducer_long}/hint={include_stale_hint}");
    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply_before = env.token_supply_observed();
    env.configure_permissionless_resolve(1_000, 100)
        .expect("configure the authenticated Recovery route");
    let mut max_compute_units = execute_trade_route(
        &mut env,
        route,
        REDUCER,
        STALE_COUNTERPARTY,
        ASSET,
        signed_size_q,
        INITIAL_PRICE,
        0,
    )
    .unwrap_or_else(|error| panic!("{case}: establish a public matched position: {error}"))
    .compute_units;
    let reduce = env
        .rebalance_reduce(REDUCER, ASSET, SIZE_Q)
        .unwrap_or_else(|error| panic!("{case}: enter ResetPending: {error}"));
    max_compute_units = max_compute_units.max(reduce.compute_units);
    let pending = env.primary_market_state().1.assets[ASSET as usize];
    let (pending_mode, pending_count) = if reducer_long {
        (pending.mode_short, pending.stored_pos_count_short)
    } else {
        (pending.mode_long, pending.stored_pos_count_long)
    };
    assert_eq!(pending_mode, SideModeV16::ResetPending, "{case}");
    assert_eq!(pending_count, 1, "{case}");

    env.warp_to_slot(1);
    let shutdown = env
        .shutdown_asset(ASSET, 1)
        .unwrap_or_else(|error| panic!("{case}: shutdown over reset episode: {error}"));
    max_compute_units = max_compute_units.max(shutdown.compute_units);
    let recovery = env.primary_market_state().1.assets[ASSET as usize];
    let old_market_id = recovery.market_id;
    let (recovery_mode, recovery_count) = if reducer_long {
        (recovery.mode_short, recovery.stored_pos_count_short)
    } else {
        (recovery.mode_long, recovery.stored_pos_count_long)
    };
    assert_eq!(recovery.lifecycle, AssetLifecycleV16::Recovery, "{case}");
    assert_eq!(recovery_mode, SideModeV16::ResetPending, "{case}");
    assert_eq!(recovery_count, 1, "{case}");
    assert_public_stock_census("INV-065 Recovery overlaps ResetPending", &env)
        .expect("overlap stock census");
    assert_public_encumbrance_census("INV-065 Recovery overlaps ResetPending", &env)
        .expect("overlap encumbrance census");

    let observation = if include_stale_hint {
        vec![CrankObservationHint {
            asset_index: ASSET,
            oracle_accounts: env.primary_profile(ASSET as usize).oracle_leg_count,
        }]
    } else {
        Vec::new()
    };
    let crank = env
        .crank(STALE_COUNTERPARTY, env.current_slot(), observation)
        .unwrap_or_else(|error| panic!("{case}: reset obligation must remain crankable: {error}"));
    max_compute_units = max_compute_units.max(crank.compute_units);
    let ready = env.primary_market_state().1.assets[ASSET as usize];
    let (stored_count, stale_count, pending_count) = if reducer_long {
        (
            ready.stored_pos_count_short,
            ready.stale_account_count_short,
            ready.pending_obligation_count_short,
        )
    } else {
        (
            ready.stored_pos_count_long,
            ready.stale_account_count_long,
            ready.pending_obligation_count_long,
        )
    };
    assert_eq!(stored_count, 0, "{case}");
    assert_eq!(stale_count, 0, "{case}");
    assert_eq!(pending_count, 0, "{case}");
    assert!(
        env.primary_portfolio(STALE_COUNTERPARTY)
            .active_bitmap
            .iter()
            .all(|word| word.get() == 0),
        "{case}: crank must detach the prior-generation leg"
    );
    let finalize = env
        .finalize_reset_side(ASSET, reset_side)
        .unwrap_or_else(|error| panic!("{case}: finalize reset side: {error}"));
    max_compute_units = max_compute_units.max(finalize.compute_units);

    env.warp_to_slot(2);
    let restart = env
        .restart_asset_oracle(ASSET, 2, INITIAL_PRICE)
        .unwrap_or_else(|error| panic!("{case}: restart empty Recovery asset: {error}"));
    max_compute_units = max_compute_units.max(restart.compute_units);
    let restarted = env.primary_market_state().1.assets[ASSET as usize];
    assert_eq!(restarted.lifecycle, AssetLifecycleV16::Active, "{case}");
    assert!(restarted.market_id > old_market_id, "{case}");
    assert_eq!(restarted.mode_long, SideModeV16::Normal, "{case}");
    assert_eq!(restarted.mode_short, SideModeV16::Normal, "{case}");
    assert_eq!(restarted.oi_eff_long_q, 0, "{case}");
    assert_eq!(restarted.oi_eff_short_q, 0, "{case}");

    let reopen = execute_trade_route(
        &mut env,
        route,
        FRESH_LONG,
        FRESH_SHORT,
        ASSET,
        signed_size_q,
        INITIAL_PRICE,
        0,
    )
    .unwrap_or_else(|error| panic!("{case}: fresh-generation open: {error}"));
    max_compute_units = max_compute_units.max(reopen.compute_units);
    let reclose = execute_trade_route(
        &mut env,
        route,
        FRESH_LONG,
        FRESH_SHORT,
        ASSET,
        -signed_size_q,
        INITIAL_PRICE,
        0,
    )
    .unwrap_or_else(|error| panic!("{case}: fresh-generation close: {error}"));
    max_compute_units = max_compute_units.max(reclose.compute_units);

    for actor in [REDUCER, STALE_COUNTERPARTY, FRESH_LONG, FRESH_SHORT] {
        let capital = env.primary_portfolio(actor).capital.get();
        if capital != 0 {
            let withdraw = env
                .withdraw_primary(actor, capital)
                .unwrap_or_else(|error| panic!("{case}: actor {actor} withdrawal: {error}"));
            max_compute_units = max_compute_units.max(withdraw.compute_units);
        }
        assert_eq!(env.primary_portfolio(actor).capital.get(), 0, "{case}");
    }
    assert_eq!(env.token_supply_observed(), supply_before, "{case}");
    assert_public_stock_census("INV-065 after reset/Recovery restart", &env)
        .expect("restart stock census");
    assert_public_encumbrance_census("INV-065 after reset/Recovery restart", &env)
        .expect("restart encumbrance census");
    assert!(
        max_compute_units < TX_CU_LIMIT,
        "{case}: {max_compute_units}"
    );
}

fn assert_shutdown_after_reset_cleanup(
    route: TradeRoute,
    reducer_long: bool,
    finalize_before_shutdown: bool,
    seed: [u8; 32],
) {
    const REDUCER: usize = 0;
    const STALE_COUNTERPARTY: usize = 1;
    const FRESH_LONG: usize = 2;
    const FRESH_SHORT: usize = 3;
    const ASSET: u16 = 0;
    const SIZE_Q: u128 = POS_SCALE;

    let signed_size_q = if reducer_long {
        SIZE_Q as i128
    } else {
        -(SIZE_Q as i128)
    };
    let reset_side = u8::from(reducer_long);
    let case = format!(
        "{route:?}/reducer_long={reducer_long}/finalize_before_shutdown={finalize_before_shutdown}"
    );
    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply_before = env.token_supply_observed();
    env.configure_permissionless_resolve(1_000, 100)
        .expect("configure the authenticated Recovery route");

    let mut max_compute_units = execute_trade_route(
        &mut env,
        route,
        REDUCER,
        STALE_COUNTERPARTY,
        ASSET,
        signed_size_q,
        INITIAL_PRICE,
        0,
    )
    .unwrap_or_else(|error| panic!("{case}: establish a public matched position: {error}"))
    .compute_units;
    let reduce = env
        .rebalance_reduce(REDUCER, ASSET, SIZE_Q)
        .unwrap_or_else(|error| panic!("{case}: enter ResetPending: {error}"));
    max_compute_units = max_compute_units.max(reduce.compute_units);

    let observation = vec![CrankObservationHint {
        asset_index: ASSET,
        oracle_accounts: env.primary_profile(ASSET as usize).oracle_leg_count,
    }];
    for _ in 0..8 {
        let asset = env.primary_market_state().1.assets[ASSET as usize];
        let stored_count = if reducer_long {
            asset.stored_pos_count_short
        } else {
            asset.stored_pos_count_long
        };
        if stored_count == 0 {
            break;
        }
        let crank = env
            .crank(STALE_COUNTERPARTY, env.current_slot(), observation.clone())
            .unwrap_or_else(|error| panic!("{case}: detach the prior-epoch leg: {error}"));
        max_compute_units = max_compute_units.max(crank.compute_units);
    }
    let detached = env.primary_market_state().1.assets[ASSET as usize];
    let (detached_mode, stored_count, stale_count, pending_count) = if reducer_long {
        (
            detached.mode_short,
            detached.stored_pos_count_short,
            detached.stale_account_count_short,
            detached.pending_obligation_count_short,
        )
    } else {
        (
            detached.mode_long,
            detached.stored_pos_count_long,
            detached.stale_account_count_long,
            detached.pending_obligation_count_long,
        )
    };
    assert_eq!(detached_mode, SideModeV16::ResetPending, "{case}");
    assert_eq!(
        (stored_count, stale_count, pending_count),
        (0, 0, 0),
        "{case}"
    );
    assert!(
        env.primary_portfolio(STALE_COUNTERPARTY)
            .active_bitmap
            .iter()
            .all(|word| word.get() == 0),
        "{case}: cleanup must detach the prior-generation leg"
    );
    assert_public_stock_census("INV-065 reset cleanup before shutdown", &env)
        .expect("pre-shutdown stock census");
    assert_public_encumbrance_census("INV-065 reset cleanup before shutdown", &env)
        .expect("pre-shutdown encumbrance census");

    if finalize_before_shutdown {
        let finalize = env
            .finalize_reset_side(ASSET, reset_side)
            .unwrap_or_else(|error| panic!("{case}: finalize before shutdown: {error}"));
        max_compute_units = max_compute_units.max(finalize.compute_units);
    }

    let old_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    let stale_generation_trade = match route {
        TradeRoute::NoCpi => env.build_retained_no_cpi_trade(
            FRESH_LONG,
            FRESH_SHORT,
            ASSET,
            signed_size_q,
            INITIAL_PRICE,
        ),
        TradeRoute::Cpi => env.build_retained_cpi_trade(
            FRESH_LONG,
            FRESH_SHORT,
            ASSET,
            signed_size_q,
            INITIAL_PRICE,
        ),
        TradeRoute::BatchNoCpi => env.build_retained_batch_no_cpi_trade(
            FRESH_LONG,
            FRESH_SHORT,
            ASSET,
            signed_size_q,
            INITIAL_PRICE,
        ),
        TradeRoute::BatchCpi => env.build_retained_batch_cpi_trade(
            FRESH_LONG,
            FRESH_SHORT,
            ASSET,
            signed_size_q,
            INITIAL_PRICE,
        ),
    };

    env.warp_to_slot(1);
    let shutdown = env
        .shutdown_asset(ASSET, 1)
        .unwrap_or_else(|error| panic!("{case}: shutdown after reset cleanup: {error}"));
    max_compute_units = max_compute_units.max(shutdown.compute_units);
    let recovery = env.primary_market_state().1.assets[ASSET as usize];
    assert_eq!(recovery.lifecycle, AssetLifecycleV16::Recovery, "{case}");
    assert_eq!(recovery.market_id, old_market_id, "{case}");
    let recovery_mode = if reducer_long {
        recovery.mode_short
    } else {
        recovery.mode_long
    };
    assert_eq!(
        recovery_mode,
        if finalize_before_shutdown {
            SideModeV16::Normal
        } else {
            SideModeV16::ResetPending
        },
        "{case}"
    );

    if !finalize_before_shutdown {
        let finalize = env
            .finalize_reset_side(ASSET, reset_side)
            .unwrap_or_else(|error| panic!("{case}: finalize during Recovery: {error}"));
        max_compute_units = max_compute_units.max(finalize.compute_units);
    }
    let ready = env.primary_market_state().1.assets[ASSET as usize];
    assert_eq!(ready.lifecycle, AssetLifecycleV16::Recovery, "{case}");
    assert_eq!(
        if reducer_long {
            ready.mode_short
        } else {
            ready.mode_long
        },
        SideModeV16::Normal,
        "{case}"
    );
    assert_public_stock_census("INV-065 finalized Recovery reset", &env)
        .expect("Recovery-finalized stock census");
    assert_public_encumbrance_census("INV-065 finalized Recovery reset", &env)
        .expect("Recovery-finalized encumbrance census");

    env.warp_to_slot(2);
    let restart = env
        .restart_asset_oracle(ASSET, 2, INITIAL_PRICE)
        .unwrap_or_else(|error| panic!("{case}: restart finalized Recovery asset: {error}"));
    max_compute_units = max_compute_units.max(restart.compute_units);
    let restarted = env.primary_market_state().1.assets[ASSET as usize];
    assert_eq!(restarted.lifecycle, AssetLifecycleV16::Active, "{case}");
    assert!(restarted.market_id > old_market_id, "{case}");
    assert_eq!(restarted.mode_long, SideModeV16::Normal, "{case}");
    assert_eq!(restarted.mode_short, SideModeV16::Normal, "{case}");

    let market_before_replay = env.market_data(false);
    let portfolios_before_replay = (0..env.actors.len())
        .map(|actor| env.primary_portfolio_data(actor))
        .collect::<Vec<_>>();
    let tokens_before_replay = env.all_token_account_data();
    env.land_retained(stale_generation_trade)
        .expect_err("an old-generation trade must reject after Recovery restart");
    assert_eq!(env.market_data(false), market_before_replay, "{case}");
    assert_eq!(
        (0..env.actors.len())
            .map(|actor| env.primary_portfolio_data(actor))
            .collect::<Vec<_>>(),
        portfolios_before_replay,
        "{case}"
    );
    assert_eq!(env.all_token_account_data(), tokens_before_replay, "{case}");

    let reopen = execute_trade_route(
        &mut env,
        route,
        FRESH_LONG,
        FRESH_SHORT,
        ASSET,
        signed_size_q,
        INITIAL_PRICE,
        0,
    )
    .unwrap_or_else(|error| panic!("{case}: fresh-generation open: {error}"));
    max_compute_units = max_compute_units.max(reopen.compute_units);
    let reclose = execute_trade_route(
        &mut env,
        route,
        FRESH_LONG,
        FRESH_SHORT,
        ASSET,
        -signed_size_q,
        INITIAL_PRICE,
        0,
    )
    .unwrap_or_else(|error| panic!("{case}: fresh-generation close: {error}"));
    max_compute_units = max_compute_units.max(reclose.compute_units);

    for actor in [REDUCER, STALE_COUNTERPARTY, FRESH_LONG, FRESH_SHORT] {
        let capital = env.primary_portfolio(actor).capital.get();
        if capital != 0 {
            let withdraw = env
                .withdraw_primary(actor, capital)
                .unwrap_or_else(|error| panic!("{case}: actor {actor} withdrawal: {error}"));
            max_compute_units = max_compute_units.max(withdraw.compute_units);
        }
        assert_eq!(env.primary_portfolio(actor).capital.get(), 0, "{case}");
    }
    assert_eq!(env.token_supply_observed(), supply_before, "{case}");
    assert_public_stock_census("INV-065 phase-order terminal stock", &env)
        .expect("terminal stock census");
    assert_public_encumbrance_census("INV-065 phase-order terminal encumbrance", &env)
        .expect("terminal encumbrance census");
    assert!(
        max_compute_units < TX_CU_LIMIT,
        "{case}: {max_compute_units}"
    );
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LifecycleRollbackSnapshot {
    market: Vec<u8>,
    foreign_market: Vec<u8>,
    portfolios: Vec<Vec<u8>>,
    foreign_portfolio: Vec<u8>,
    backing_ledger: Vec<u8>,
    tokens: Vec<(solana_sdk::pubkey::Pubkey, Vec<u8>)>,
    matcher_contexts: Vec<Vec<u8>>,
    economic_lamports: Vec<(solana_sdk::pubkey::Pubkey, u64)>,
}

fn lifecycle_rollback_snapshot(env: &V16Svm) -> LifecycleRollbackSnapshot {
    LifecycleRollbackSnapshot {
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

fn has_asset_leg(env: &V16Svm, actor: usize, asset_index: u16) -> bool {
    env.primary_portfolio(actor)
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .any(|leg| leg.active && leg.asset_index == u32::from(asset_index))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RetainedReductionOrder {
    ReduceThenShutdown,
    ShutdownThenReduce,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RetainedReductionLifecycleOutcome {
    destination_balances: [u64; 2],
    token_supply: u128,
    restarted_generation: u64,
    retained_reduce_landed: bool,
    recovery_forfeits: usize,
    cleanup_cranks: usize,
}

fn run_retained_reduction_lifecycle_world(
    route: TradeRoute,
    reducer_long: bool,
    order: RetainedReductionOrder,
    seed: [u8; 32],
) -> Result<RetainedReductionLifecycleOutcome, String> {
    const REDUCER: usize = 0;
    const COUNTERPARTY: usize = 1;
    const ASSET: u16 = 0;

    let case = format!("{route:?}/reducer_long={reducer_long}/{order:?}");
    let signed_size_q = if reducer_long {
        POS_SCALE as i128
    } else {
        -(POS_SCALE as i128)
    };
    let mut env = V16Svm::new(seed, MarketConfig::default());
    let token_supply = env.token_supply_observed();
    env.configure_permissionless_resolve(1_000, 100)
        .map_err(|error| format!("{case}: configure Recovery route: {error}"))?;
    let mut max_compute_units = execute_trade_route(
        &mut env,
        route,
        REDUCER,
        COUNTERPARTY,
        ASSET,
        signed_size_q,
        INITIAL_PRICE,
        0,
    )
    .map_err(|error| format!("{case}: establish matched position: {error}"))?
    .compute_units;
    let old_generation = env.primary_market_state().1.assets[ASSET as usize].market_id;
    let retained_reduce = env.build_retained_rebalance_reduce(REDUCER, ASSET, POS_SCALE);

    let mut retained_reduce_landed = false;
    match order {
        RetainedReductionOrder::ReduceThenShutdown => {
            let reduce = env
                .land_retained(retained_reduce)
                .map_err(|error| format!("{case}: pre-shutdown retained reduction: {error}"))?;
            max_compute_units = max_compute_units.max(reduce.compute_units);
            retained_reduce_landed = true;
            env.warp_to_slot(1);
            let shutdown = env
                .shutdown_asset(ASSET, 1)
                .map_err(|error| format!("{case}: shutdown after reduction: {error}"))?;
            max_compute_units = max_compute_units.max(shutdown.compute_units);
        }
        RetainedReductionOrder::ShutdownThenReduce => {
            env.warp_to_slot(1);
            let shutdown = env
                .shutdown_asset(ASSET, 1)
                .map_err(|error| format!("{case}: shutdown before reduction: {error}"))?;
            max_compute_units = max_compute_units.max(shutdown.compute_units);
            let before_reduce = lifecycle_rollback_snapshot(&env);
            match env.land_retained(retained_reduce) {
                Ok(reduce) => {
                    max_compute_units = max_compute_units.max(reduce.compute_units);
                    retained_reduce_landed = true;
                }
                Err(_) => {
                    if lifecycle_rollback_snapshot(&env) != before_reduce {
                        return Err(format!(
                            "{case}: rejected post-shutdown reduction did not roll back exactly"
                        ));
                    }
                }
            }
        }
    }
    let recovery = env.primary_market_state().1.assets[ASSET as usize];
    if recovery.lifecycle != AssetLifecycleV16::Recovery || recovery.market_id != old_generation {
        return Err(format!(
            "{case}: shutdown did not preserve the active generation in Recovery"
        ));
    }
    assert_public_stock_census("INV-065 shutdown/reduction landing prefix", &env)?;
    assert_public_encumbrance_census("INV-065 shutdown/reduction landing prefix", &env)?;

    let mut recovery_forfeits = 0usize;
    if !retained_reduce_landed {
        for actor in [COUNTERPARTY, REDUCER] {
            if !has_asset_leg(&env, actor, ASSET) {
                continue;
            }
            let forfeit = env
                .forfeit_recovery_leg(actor, ASSET, u128::MAX)
                .map_err(|error| format!("{case}: actor {actor} Recovery forfeit: {error}"))?;
            max_compute_units = max_compute_units.max(forfeit.compute_units);
            recovery_forfeits += 1;
        }
    }

    let mut cleanup_cranks = 0usize;
    for actor in [REDUCER, COUNTERPARTY] {
        for _ in 0..8 {
            if !has_asset_leg(&env, actor, ASSET) {
                break;
            }
            let before = lifecycle_rollback_snapshot(&env);
            let crank = env
                .crank(actor, env.current_slot(), Vec::new())
                .map_err(|error| format!("{case}: actor {actor} cleanup crank: {error}"))?;
            max_compute_units = max_compute_units.max(crank.compute_units);
            cleanup_cranks += 1;
            if lifecycle_rollback_snapshot(&env) == before {
                return Err(format!("{case}: actor {actor} cleanup crank was a no-op"));
            }
        }
        if has_asset_leg(&env, actor, ASSET) {
            return Err(format!(
                "{case}: actor {actor} retained an asset leg after bounded cleanup"
            ));
        }
    }
    for side in [0u8, 1u8] {
        let asset = env.primary_market_state().1.assets[ASSET as usize];
        let mode = if side == 0 {
            asset.mode_long
        } else {
            asset.mode_short
        };
        if mode == SideModeV16::ResetPending {
            let finalize = env
                .finalize_reset_side(ASSET, side)
                .map_err(|error| format!("{case}: finalize side {side}: {error}"))?;
            max_compute_units = max_compute_units.max(finalize.compute_units);
        }
    }
    let ready = env.primary_market_state().1.assets[ASSET as usize];
    if ready.mode_long != SideModeV16::Normal
        || ready.mode_short != SideModeV16::Normal
        || ready.oi_eff_long_q != 0
        || ready.oi_eff_short_q != 0
        || ready.stored_pos_count_long != 0
        || ready.stored_pos_count_short != 0
        || ready.pending_obligation_count_long != 0
        || ready.pending_obligation_count_short != 0
    {
        return Err(format!(
            "{case}: Recovery cleanup was not terminal: {ready:?}"
        ));
    }

    env.warp_to_slot(2);
    let restart = env
        .restart_asset_oracle(ASSET, 2, INITIAL_PRICE)
        .map_err(|error| format!("{case}: restart asset: {error}"))?;
    max_compute_units = max_compute_units.max(restart.compute_units);
    let restarted = env.primary_market_state().1.assets[ASSET as usize];
    if restarted.lifecycle != AssetLifecycleV16::Active || restarted.market_id <= old_generation {
        return Err(format!("{case}: asset did not restart monotonically"));
    }

    let reopen = execute_trade_route(
        &mut env,
        route,
        REDUCER,
        COUNTERPARTY,
        ASSET,
        signed_size_q,
        INITIAL_PRICE,
        0,
    )
    .map_err(|error| format!("{case}: fresh-generation open: {error}"))?;
    max_compute_units = max_compute_units.max(reopen.compute_units);
    let reclose = execute_trade_route(
        &mut env,
        route,
        REDUCER,
        COUNTERPARTY,
        ASSET,
        -signed_size_q,
        INITIAL_PRICE,
        0,
    )
    .map_err(|error| format!("{case}: fresh-generation close: {error}"))?;
    max_compute_units = max_compute_units.max(reclose.compute_units);

    for actor in [REDUCER, COUNTERPARTY] {
        let capital = env.primary_portfolio(actor).capital.get();
        let withdrawal = env
            .withdraw_primary(actor, capital)
            .map_err(|error| format!("{case}: actor {actor} withdrawal: {error}"))?;
        max_compute_units = max_compute_units.max(withdrawal.compute_units);
        if env.primary_portfolio(actor).capital.get() != 0 {
            return Err(format!("{case}: actor {actor} retained capital"));
        }
    }
    if max_compute_units >= TX_CU_LIMIT || env.token_supply_observed() != token_supply {
        return Err(format!(
            "{case}: lifecycle exceeded CU or changed token supply: {max_compute_units}"
        ));
    }
    assert_public_stock_census("INV-065 shutdown/reduction landing terminal", &env)?;
    assert_public_encumbrance_census("INV-065 shutdown/reduction landing terminal", &env)?;

    Ok(RetainedReductionLifecycleOutcome {
        destination_balances: std::array::from_fn(|actor| {
            env.token_amount(env.actors[actor].destination_token)
        }),
        token_supply,
        restarted_generation: restarted.market_id,
        retained_reduce_landed,
        recovery_forfeits,
        cleanup_cranks,
    })
}

#[test]
fn v16_program_retained_reduction_landing_after_shutdown_has_a_bounded_recovery_fallback() {
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
            let mut seed = [0x48; 32];
            seed[0] ^= route_index as u8;
            seed[1] ^= u8::from(reducer_long);
            let reduce_first = run_retained_reduction_lifecycle_world(
                route,
                reducer_long,
                RetainedReductionOrder::ReduceThenShutdown,
                seed,
            )
            .unwrap_or_else(|error| panic!("reduce-first lifecycle: {error}"));
            let shutdown_first = run_retained_reduction_lifecycle_world(
                route,
                reducer_long,
                RetainedReductionOrder::ShutdownThenReduce,
                seed,
            )
            .unwrap_or_else(|error| panic!("shutdown-first lifecycle: {error}"));
            assert!(
                reduce_first.retained_reduce_landed,
                "{route:?}/{reducer_long}"
            );
            assert_eq!(
                reduce_first.recovery_forfeits, 0,
                "{route:?}/{reducer_long}"
            );
            assert!(
                !shutdown_first.retained_reduce_landed,
                "Recovery must use its explicit owner-exit route: {route:?}/{reducer_long}/{shutdown_first:?}"
            );
            assert_eq!(
                shutdown_first.recovery_forfeits, 2,
                "{route:?}/{reducer_long}/{shutdown_first:?}"
            );
            assert_eq!(
                reduce_first.destination_balances, shutdown_first.destination_balances,
                "{route:?}/{reducer_long}"
            );
            assert_eq!(reduce_first.token_supply, shutdown_first.token_supply);
            assert_eq!(
                reduce_first.restarted_generation, shutdown_first.restarted_generation,
                "{route:?}/{reducer_long}"
            );
            assert!(reduce_first.cleanup_cranks > 0, "{reduce_first:?}");
            assert!(shutdown_first.cleanup_cranks > 0, "{shutdown_first:?}");
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OtherAssetScopeSnapshot {
    asset: percolator::AssetStateV16,
    profile: percolator_prog::state::AssetOracleProfileV16,
    portfolios: [Vec<u8>; 2],
    foreign_market: Vec<u8>,
    foreign_portfolio: Vec<u8>,
    backing_ledger: Vec<u8>,
    tokens: Vec<(solana_sdk::pubkey::Pubkey, Vec<u8>)>,
    matcher_contexts: [Vec<u8>; 2],
}

fn other_asset_scope_snapshot(
    env: &V16Svm,
    asset_index: u16,
    actors: [usize; 2],
) -> OtherAssetScopeSnapshot {
    OtherAssetScopeSnapshot {
        asset: env.primary_market_state().1.assets[asset_index as usize],
        profile: env.primary_profile(asset_index as usize),
        portfolios: actors.map(|actor| env.primary_portfolio_data(actor)),
        foreign_market: env.market_data(true),
        foreign_portfolio: env.foreign_portfolio_data(),
        backing_ledger: env.backing_domain_ledger_data(),
        tokens: env.all_token_account_data(),
        matcher_contexts: actors.map(|actor| {
            env.svm
                .get_account(&env.actors[actor].matcher_context)
                .expect("tracked matcher context")
                .data
        }),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TwoAssetLifecycleOrder {
    AssetZeroThenOne,
    AssetOneThenZero,
}

impl TwoAssetLifecycleOrder {
    fn assets(self) -> [u16; 2] {
        match self {
            Self::AssetZeroThenOne => [0, 1],
            Self::AssetOneThenZero => [1, 0],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TwoAssetLifecycleOutcome {
    destination_balances: [u64; 4],
    token_supply: u128,
    restarted_generations: [u64; 2],
    cleanup_cranks: [usize; 2],
}

fn asset_pair(asset_index: u16) -> [usize; 2] {
    match asset_index {
        0 => [0, 1],
        1 => [2, 3],
        _ => unreachable!("two-asset lifecycle fixture uses assets 0 and 1"),
    }
}

fn clear_reset_scope_while_framing_other(
    env: &mut V16Svm,
    asset_index: u16,
    case: &str,
) -> Result<(u64, usize), String> {
    let [_, stale_actor] = asset_pair(asset_index);
    let other_asset = 1 - asset_index;
    let other_pair = asset_pair(other_asset);
    let mut max_compute_units = 0u64;
    let mut calls = 0usize;
    for _ in 0..8 {
        if !has_asset_leg(env, stale_actor, asset_index) {
            break;
        }
        let whole_before = lifecycle_rollback_snapshot(env);
        let other_before = other_asset_scope_snapshot(env, other_asset, other_pair);
        let crank = env
            .crank(stale_actor, env.current_slot(), Vec::new())
            .map_err(|error| {
                format!("{case}: asset {asset_index} actor {stale_actor} crank: {error}")
            })?;
        max_compute_units = max_compute_units.max(crank.compute_units);
        calls += 1;
        if lifecycle_rollback_snapshot(env) == whole_before {
            return Err(format!(
                "{case}: asset {asset_index} cleanup crank was a successful no-op"
            ));
        }
        if other_asset_scope_snapshot(env, other_asset, other_pair) != other_before {
            return Err(format!(
                "{case}: asset {asset_index} cleanup crossed into asset {other_asset} scope"
            ));
        }
    }
    if calls == 0 || has_asset_leg(env, stale_actor, asset_index) {
        return Err(format!(
            "{case}: asset {asset_index} did not clear in bounded permissionless work"
        ));
    }
    let asset = env.primary_market_state().1.assets[asset_index as usize];
    if asset.stored_pos_count_long != 0
        || asset.stored_pos_count_short != 0
        || asset.stale_account_count_long != 0
        || asset.stale_account_count_short != 0
        || asset.pending_obligation_count_long != 0
        || asset.pending_obligation_count_short != 0
        || asset.oi_eff_long_q != 0
        || asset.oi_eff_short_q != 0
    {
        return Err(format!(
            "{case}: asset {asset_index} retained lifecycle work: {asset:?}"
        ));
    }
    Ok((max_compute_units, calls))
}

fn run_two_asset_lifecycle_world(
    routes: [TradeRoute; 2],
    reducer_long: [bool; 2],
    order: TwoAssetLifecycleOrder,
    seed: [u8; 32],
) -> Result<TwoAssetLifecycleOutcome, String> {
    let case = format!("routes={routes:?}/sides={reducer_long:?}/{order:?}");
    let mut env = V16Svm::new(seed, MarketConfig::default());
    let token_supply = env.token_supply_observed();
    env.configure_permissionless_resolve(1_000, 100)
        .map_err(|error| format!("{case}: configure Recovery route: {error}"))?;
    let old_generations: [u64; 2] = std::array::from_fn(|asset_index| {
        env.primary_market_state().1.assets[asset_index].market_id
    });
    let mut max_compute_units = 0u64;

    for asset_index in [0u16, 1u16] {
        let [reducer, counterparty] = asset_pair(asset_index);
        let signed_size_q = if reducer_long[asset_index as usize] {
            POS_SCALE as i128
        } else {
            -(POS_SCALE as i128)
        };
        let open = execute_trade_route(
            &mut env,
            routes[asset_index as usize],
            reducer,
            counterparty,
            asset_index,
            signed_size_q,
            INITIAL_PRICE,
            0,
        )
        .map_err(|error| format!("{case}: asset {asset_index} open: {error}"))?;
        max_compute_units = max_compute_units.max(open.compute_units);
        let reduce = env
            .rebalance_reduce(reducer, asset_index, POS_SCALE)
            .map_err(|error| format!("{case}: asset {asset_index} reduction: {error}"))?;
        max_compute_units = max_compute_units.max(reduce.compute_units);
        let asset = env.primary_market_state().1.assets[asset_index as usize];
        let reset_mode = if reducer_long[asset_index as usize] {
            asset.mode_short
        } else {
            asset.mode_long
        };
        if reset_mode != SideModeV16::ResetPending {
            return Err(format!(
                "{case}: asset {asset_index} did not enter ResetPending"
            ));
        }
    }
    assert_public_stock_census("INV-065 simultaneous reset prefix", &env)?;
    assert_public_encumbrance_census("INV-065 simultaneous reset prefix", &env)?;

    env.warp_to_slot(1);
    for asset_index in order.assets() {
        let other_asset = 1 - asset_index;
        let other_pair = asset_pair(other_asset);
        let other_before = other_asset_scope_snapshot(&env, other_asset, other_pair);
        let shutdown = env
            .shutdown_asset(asset_index, 1)
            .map_err(|error| format!("{case}: asset {asset_index} shutdown: {error}"))?;
        max_compute_units = max_compute_units.max(shutdown.compute_units);
        if other_asset_scope_snapshot(&env, other_asset, other_pair) != other_before {
            return Err(format!(
                "{case}: asset {asset_index} shutdown crossed into asset {other_asset} scope"
            ));
        }
    }
    for asset_index in [0usize, 1usize] {
        if env.primary_market_state().1.assets[asset_index].lifecycle != AssetLifecycleV16::Recovery
        {
            return Err(format!("{case}: asset {asset_index} missed Recovery"));
        }
    }

    let mut cleanup_cranks = [0usize; 2];
    for asset_index in order.assets() {
        let (compute_units, calls) =
            clear_reset_scope_while_framing_other(&mut env, asset_index, &case)?;
        max_compute_units = max_compute_units.max(compute_units);
        cleanup_cranks[asset_index as usize] = calls;
    }
    for asset_index in order.assets() {
        let other_asset = 1 - asset_index;
        let other_pair = asset_pair(other_asset);
        let other_before = other_asset_scope_snapshot(&env, other_asset, other_pair);
        let side = u8::from(reducer_long[asset_index as usize]);
        let finalize = env
            .finalize_reset_side(asset_index, side)
            .map_err(|error| format!("{case}: asset {asset_index} finalization: {error}"))?;
        max_compute_units = max_compute_units.max(finalize.compute_units);
        if other_asset_scope_snapshot(&env, other_asset, other_pair) != other_before {
            return Err(format!(
                "{case}: asset {asset_index} finalization crossed into asset {other_asset} scope"
            ));
        }
    }

    env.warp_to_slot(2);
    for asset_index in order.assets() {
        let other_asset = 1 - asset_index;
        let other_pair = asset_pair(other_asset);
        let other_before = other_asset_scope_snapshot(&env, other_asset, other_pair);
        let restart = env
            .restart_asset_oracle(asset_index, 2, INITIAL_PRICE)
            .map_err(|error| format!("{case}: asset {asset_index} restart: {error}"))?;
        max_compute_units = max_compute_units.max(restart.compute_units);
        if other_asset_scope_snapshot(&env, other_asset, other_pair) != other_before {
            return Err(format!(
                "{case}: asset {asset_index} restart crossed into asset {other_asset} scope"
            ));
        }
    }
    let restarted_generations: [u64; 2] = std::array::from_fn(|asset_index| {
        env.primary_market_state().1.assets[asset_index].market_id
    });
    if (0..2).any(|asset_index| restarted_generations[asset_index] <= old_generations[asset_index])
    {
        return Err(format!("{case}: one asset generation did not advance"));
    }

    for asset_index in order.assets() {
        let [reducer, counterparty] = asset_pair(asset_index);
        let signed_size_q = if reducer_long[asset_index as usize] {
            POS_SCALE as i128
        } else {
            -(POS_SCALE as i128)
        };
        let other_asset = 1 - asset_index;
        let other_pair = asset_pair(other_asset);
        let other_before = other_asset_scope_snapshot(&env, other_asset, other_pair);
        let open = execute_trade_route(
            &mut env,
            routes[asset_index as usize],
            reducer,
            counterparty,
            asset_index,
            signed_size_q,
            INITIAL_PRICE,
            0,
        )
        .map_err(|error| format!("{case}: asset {asset_index} fresh open: {error}"))?;
        max_compute_units = max_compute_units.max(open.compute_units);
        let close = execute_trade_route(
            &mut env,
            routes[asset_index as usize],
            reducer,
            counterparty,
            asset_index,
            -signed_size_q,
            INITIAL_PRICE,
            0,
        )
        .map_err(|error| format!("{case}: asset {asset_index} fresh close: {error}"))?;
        max_compute_units = max_compute_units.max(close.compute_units);
        if other_asset_scope_snapshot(&env, other_asset, other_pair) != other_before {
            return Err(format!(
                "{case}: asset {asset_index} fresh roundtrip crossed into asset {other_asset} scope"
            ));
        }
    }

    for actor in 0..4 {
        let capital = env.primary_portfolio(actor).capital.get();
        let withdrawal = env
            .withdraw_primary(actor, capital)
            .map_err(|error| format!("{case}: actor {actor} withdrawal: {error}"))?;
        max_compute_units = max_compute_units.max(withdrawal.compute_units);
        if env.primary_portfolio(actor).capital.get() != 0 {
            return Err(format!("{case}: actor {actor} retained capital"));
        }
    }
    if max_compute_units >= TX_CU_LIMIT || env.token_supply_observed() != token_supply {
        return Err(format!(
            "{case}: simultaneous lifecycle exceeded CU or changed supply: {max_compute_units}"
        ));
    }
    assert_public_stock_census("INV-065 simultaneous lifecycle terminal", &env)?;
    assert_public_encumbrance_census("INV-065 simultaneous lifecycle terminal", &env)?;

    Ok(TwoAssetLifecycleOutcome {
        destination_balances: std::array::from_fn(|actor| {
            env.token_amount(env.actors[actor].destination_token)
        }),
        token_supply,
        restarted_generations,
        cleanup_cranks,
    })
}

#[test]
fn v16_program_two_asset_reset_recovery_orders_progress_without_crossing_scope() {
    let routes = [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ];
    for (route_zero_index, route_zero) in routes.into_iter().enumerate() {
        for (route_one_index, route_one) in routes.into_iter().enumerate() {
            for reducer_zero_long in [false, true] {
                for reducer_one_long in [false, true] {
                    let mut seed = [0x49; 32];
                    seed[0] ^= route_zero_index as u8;
                    seed[1] ^= route_one_index as u8;
                    seed[2] ^= u8::from(reducer_zero_long);
                    seed[3] ^= u8::from(reducer_one_long);
                    let zero_first = run_two_asset_lifecycle_world(
                        [route_zero, route_one],
                        [reducer_zero_long, reducer_one_long],
                        TwoAssetLifecycleOrder::AssetZeroThenOne,
                        seed,
                    )
                    .unwrap_or_else(|error| panic!("asset-zero-first lifecycle: {error}"));
                    let one_first = run_two_asset_lifecycle_world(
                        [route_zero, route_one],
                        [reducer_zero_long, reducer_one_long],
                        TwoAssetLifecycleOrder::AssetOneThenZero,
                        seed,
                    )
                    .unwrap_or_else(|error| panic!("asset-one-first lifecycle: {error}"));
                    assert_eq!(
                        zero_first.destination_balances,
                        one_first.destination_balances,
                        "routes={:?}/sides={:?}",
                        [route_zero, route_one],
                        [reducer_zero_long, reducer_one_long]
                    );
                    assert_eq!(zero_first.token_supply, one_first.token_supply);
                    assert_eq!(zero_first.cleanup_cranks, one_first.cleanup_cranks);
                    let mut zero_first_generations = zero_first.restarted_generations;
                    let mut one_first_generations = one_first.restarted_generations;
                    zero_first_generations.sort_unstable();
                    one_first_generations.sort_unstable();
                    assert_eq!(zero_first_generations, one_first_generations);
                    assert_ne!(
                        zero_first.restarted_generations[0],
                        zero_first.restarted_generations[1]
                    );
                    assert_ne!(
                        one_first.restarted_generations[0],
                        one_first.restarted_generations[1]
                    );
                    assert!(zero_first.cleanup_cranks.into_iter().all(|calls| calls > 0));
                }
            }
        }
    }
}

#[test]
fn v16_program_shutdown_during_reset_pending_retains_permissionless_progress() {
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
            for include_stale_hint in [false, true] {
                let mut seed = [0x46; 32];
                seed[0] ^= route_index as u8;
                seed[1] ^= u8::from(reducer_long);
                seed[2] ^= u8::from(include_stale_hint);
                assert_shutdown_during_reset_pending(route, reducer_long, include_stale_hint, seed);
            }
        }
    }
}

#[test]
fn v16_program_shutdown_after_reset_cleanup_is_order_safe() {
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
            for finalize_before_shutdown in [false, true] {
                let mut seed = [0x47; 32];
                seed[0] ^= route_index as u8;
                seed[1] ^= u8::from(reducer_long);
                seed[2] ^= u8::from(finalize_before_shutdown);
                assert_shutdown_after_reset_cleanup(
                    route,
                    reducer_long,
                    finalize_before_shutdown,
                    seed,
                );
            }
        }
    }
}

#[test]
fn v16_program_unilateral_zero_oi_reset_route_side_matrix_finalizes_permissionlessly() {
    for asset_index in [0u16, 1u16] {
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
                let mut seed = [0x45; 32];
                seed[0] ^= route_index as u8;
                seed[1] ^= u8::from(reducer_long);
                seed[2] ^= asset_index as u8;
                assert_public_reset_lifecycle(route, reducer_long, asset_index, seed);
            }
        }
    }
}
