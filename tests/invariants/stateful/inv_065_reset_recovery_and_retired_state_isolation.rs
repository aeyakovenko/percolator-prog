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
//! reaches `ResetPending` without state injection in all eight trade-route/reducer-side worlds: one
//! owner fully reduces a matched leg, an early global finalizer rejects atomically while the
//! prior-epoch counterparty leg remains, bounded public auto-cranks detach that leg, and the
//! permissionless finalizer restores Normal. A fresh matched open/close through the same route and
//! full owner withdrawals prove the reset does not orphan funds or permanently disable the asset.
//!
//! This is bounded generated coverage, not exhaustive reset/recovery/retirement
//! reachability.

use super::*;
use crate::support::fuzz_model::{
    assert_public_encumbrance_census, assert_public_stock_census, execute_trade_route,
};
use crate::support::v16_svm::{MarketConfig, V16Svm, INITIAL_PRICE, TX_CU_LIMIT};
use percolator::{SideModeV16, POS_SCALE};
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

fn assert_public_reset_lifecycle(route: TradeRoute, reducer_long: bool, seed: [u8; 32]) {
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
    let case = format!("{route:?}/reducer_long={reducer_long}");
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
        ASSET,
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
        .rebalance_reduce(REDUCER, ASSET, SIZE_Q)
        .expect("full owner reduction must enter the opposite-side reset episode");
    max_compute_units = max_compute_units.max(reduce.compute_units);
    let reset = env.primary_market_state().1.assets[ASSET as usize];
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
    env.finalize_reset_side(ASSET, reset_side)
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
        asset_index: ASSET,
        oracle_accounts: env.primary_profile(ASSET as usize).oracle_leg_count,
    }];
    let mut crank_calls = 0usize;
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
        "permissionless cleanup must detach the old side episode"
    );

    let risk_epoch_before = env.primary_market_state().1.risk_epoch;
    let finalize = env
        .finalize_reset_side(ASSET, reset_side)
        .expect("a clean ResetPending side must finalize permissionlessly");
    max_compute_units = max_compute_units.max(finalize.compute_units);
    let finalized = env.primary_market_state().1;
    let finalized_mode = if reducer_long {
        finalized.assets[ASSET as usize].mode_short
    } else {
        finalized.assets[ASSET as usize].mode_long
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
        ASSET,
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
        ASSET,
        -signed_size_q,
        INITIAL_PRICE,
        0,
    )
    .unwrap_or_else(|error| {
        panic!("{case}: fresh matched risk must retain a bounded bilateral exit: {error}")
    });
    max_compute_units = max_compute_units.max(reclose.compute_units);
    let after_fresh_close = env.primary_market_state().1.assets[ASSET as usize];
    assert_eq!(after_fresh_close.oi_eff_long_q, 0);
    assert_eq!(after_fresh_close.oi_eff_short_q, 0);
    assert_public_stock_census("INV-065 after fresh post-reset roundtrip", &env)
        .expect("post-reset roundtrip stock census");
    assert_public_encumbrance_census("INV-065 after fresh post-reset roundtrip", &env)
        .expect("post-reset roundtrip encumbrance census");

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

#[test]
fn v16_program_unilateral_zero_oi_reset_route_side_matrix_finalizes_permissionlessly() {
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
            assert_public_reset_lifecycle(route, reducer_long, seed);
        }
    }
}
