//! INV-072 - Order-robust crankability.
//!
//! Normative obligation: crank hints and their account tails are discovery inputs. They cannot
//! become economic truth, block a later canonical continuation, or force an account to depend on
//! an oracle parser that is inapplicable to its current lifecycle.
//!
//! Evidence in this file (F over public LiteSVM routes): every world configures a real one-feed
//! hybrid oracle, opens a matched position, enters ResetPending by owner reduction, and lands
//! shutdown so the remaining prior-epoch leg is actionable in Recovery. The matrix then crosses
//! absent, zero-account, stale-profile, malformed, overdeclared, missing, unclaimed, duplicate,
//! and out-of-range tails. Applicable tails must produce the same bounded leg-detach transition;
//! malformed schedules must reject with exact market/portfolio/SPL rollback and leave a no-hint
//! continuation live. Every world finalizes the reset, restarts the asset generation, and returns
//! both owners' capital under independent stock and encumbrance censuses.
//!
//! The Pyth account bytes are external authenticated-input fixtures. No program-owned state is
//! injected; every economically relevant state transition uses the public wrapper.

use super::*;
use crate::support::fuzz_model::{
    assert_public_encumbrance_census, assert_public_stock_census, execute_trade_route,
};
use crate::support::v16_svm::{MarketConfig, V16Svm, INITIAL_PRICE, TX_CU_LIMIT};
use percolator::{AssetLifecycleV16, SideModeV16, POS_SCALE};
use percolator_prog::ix::CrankObservationHint;

const REDUCER: usize = 0;
const STALE_COUNTERPARTY: usize = 1;
const ASSET: u16 = 0;

#[derive(Clone, Copy, Debug)]
enum RecoveryTailCase {
    NoHint,
    ZeroAccountHint,
    StaleProfileOracle,
    MalformedDeclaredTail,
    OverdeclaredIgnoredTail,
    MissingDeclaredTail,
    UnclaimedTail,
    DuplicateHint,
    OutOfRangeHint,
}

impl RecoveryTailCase {
    fn should_land(self) -> bool {
        matches!(
            self,
            Self::NoHint
                | Self::ZeroAccountHint
                | Self::StaleProfileOracle
                | Self::MalformedDeclaredTail
                | Self::OverdeclaredIgnoredTail
        )
    }
}

#[derive(Debug, PartialEq, Eq)]
struct RecoveryTailOutcome {
    old_market_id: u64,
    new_market_id: u64,
    reducer_capital: u128,
    counterparty_capital: u128,
    asset_slot: u64,
}

fn recovery_tail_world(case: RecoveryTailCase, seed: [u8; 32]) -> RecoveryTailOutcome {
    let label = format!("{case:?}");
    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply_before = env.token_supply_observed();
    env.configure_permissionless_resolve(1_000, 100)
        .expect("configure the authenticated Recovery route");

    env.set_clock(1, 100);
    let feed = [0x72; 32];
    let oracle = env.set_pyth_price(&feed, INITIAL_PRICE as i64, -6, 0, 100);
    env.configure_hybrid_oracle(
        ASSET,
        1,
        100,
        0,
        [feed, [0; 32], [0; 32]],
        &[oracle],
        100,
        0,
    )
    .unwrap_or_else(|error| panic!("{label}: configure one-feed hybrid oracle: {error}"));
    assert_eq!(
        env.primary_profile(ASSET as usize).oracle_leg_count,
        1,
        "{label}: setup must make the declared external tail non-vacuous"
    );

    let mut max_compute_units = execute_trade_route(
        &mut env,
        TradeRoute::NoCpi,
        REDUCER,
        STALE_COUNTERPARTY,
        ASSET,
        POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    )
    .unwrap_or_else(|error| panic!("{label}: establish public matched exposure: {error}"))
    .compute_units;
    let reduce = env
        .rebalance_reduce(REDUCER, ASSET, POS_SCALE)
        .unwrap_or_else(|error| panic!("{label}: enter ResetPending: {error}"));
    max_compute_units = max_compute_units.max(reduce.compute_units);
    let pending = env.primary_market_state().1.assets[ASSET as usize];
    assert_eq!(pending.mode_short, SideModeV16::ResetPending, "{label}");
    assert_eq!(pending.stored_pos_count_short, 1, "{label}");

    env.set_clock(2, 1_000);
    let shutdown = env
        .shutdown_asset(ASSET, 2)
        .unwrap_or_else(|error| panic!("{label}: shutdown over ResetPending: {error}"));
    max_compute_units = max_compute_units.max(shutdown.compute_units);
    let recovery = env.primary_market_state().1.assets[ASSET as usize];
    let old_market_id = recovery.market_id;
    assert_eq!(recovery.lifecycle, AssetLifecycleV16::Recovery, "{label}");
    assert_eq!(recovery.mode_short, SideModeV16::ResetPending, "{label}");
    assert_eq!(recovery.stored_pos_count_short, 1, "{label}");
    assert_public_stock_census("INV-072 before Recovery tail attempt", &env)
        .expect("pre-attempt stock census");
    assert_public_encumbrance_census("INV-072 before Recovery tail attempt", &env)
        .expect("pre-attempt encumbrance census");

    let mint = env.mint;
    let (observations, tail) = match case {
        RecoveryTailCase::NoHint => (Vec::new(), Vec::new()),
        RecoveryTailCase::ZeroAccountHint => (
            vec![CrankObservationHint {
                asset_index: ASSET,
                oracle_accounts: 0,
            }],
            Vec::new(),
        ),
        RecoveryTailCase::StaleProfileOracle => (
            vec![CrankObservationHint {
                asset_index: ASSET,
                oracle_accounts: 1,
            }],
            vec![oracle],
        ),
        RecoveryTailCase::MalformedDeclaredTail => (
            vec![CrankObservationHint {
                asset_index: ASSET,
                oracle_accounts: 1,
            }],
            vec![mint],
        ),
        RecoveryTailCase::OverdeclaredIgnoredTail => (
            vec![CrankObservationHint {
                asset_index: ASSET,
                oracle_accounts: 2,
            }],
            vec![oracle, mint],
        ),
        RecoveryTailCase::MissingDeclaredTail => (
            vec![CrankObservationHint {
                asset_index: ASSET,
                oracle_accounts: 1,
            }],
            Vec::new(),
        ),
        RecoveryTailCase::UnclaimedTail => (Vec::new(), vec![oracle]),
        RecoveryTailCase::DuplicateHint => (
            vec![
                CrankObservationHint {
                    asset_index: ASSET,
                    oracle_accounts: 0,
                },
                CrankObservationHint {
                    asset_index: ASSET,
                    oracle_accounts: 0,
                },
            ],
            Vec::new(),
        ),
        RecoveryTailCase::OutOfRangeHint => (
            vec![CrankObservationHint {
                asset_index: u16::MAX,
                oracle_accounts: 0,
            }],
            Vec::new(),
        ),
    };

    let market_before = env.market_data(false);
    let portfolios_before = (0..env.actors.len())
        .map(|actor| env.primary_portfolio_data(actor))
        .collect::<Vec<_>>();
    let tokens_before = env.all_token_account_data();
    let attempted =
        env.crank_with_oracles(STALE_COUNTERPARTY, env.current_slot(), observations, &tail);
    if case.should_land() {
        let landed = attempted
            .unwrap_or_else(|error| panic!("{label}: applicable Recovery tail must land: {error}"));
        max_compute_units = max_compute_units.max(landed.compute_units);
    } else {
        assert!(
            attempted.is_err(),
            "{label}: malformed Recovery tail must reject"
        );
        assert_eq!(env.market_data(false), market_before, "{label}");
        assert_eq!(
            (0..env.actors.len())
                .map(|actor| env.primary_portfolio_data(actor))
                .collect::<Vec<_>>(),
            portfolios_before,
            "{label}"
        );
        assert_eq!(env.all_token_account_data(), tokens_before, "{label}");
        let retry = env
            .crank(STALE_COUNTERPARTY, env.current_slot(), Vec::new())
            .unwrap_or_else(|error| panic!("{label}: canonical no-hint retry must land: {error}"));
        max_compute_units = max_compute_units.max(retry.compute_units);
    }

    let detached = env.primary_market_state().1.assets[ASSET as usize];
    assert_eq!(detached.lifecycle, AssetLifecycleV16::Recovery, "{label}");
    assert_eq!(detached.mode_short, SideModeV16::ResetPending, "{label}");
    assert_eq!(detached.stored_pos_count_short, 0, "{label}");
    assert_eq!(detached.stale_account_count_short, 0, "{label}");
    assert_eq!(detached.pending_obligation_count_short, 0, "{label}");
    assert!(
        env.primary_portfolio(STALE_COUNTERPARTY)
            .active_bitmap
            .iter()
            .all(|word| word.get() == 0),
        "{label}: selected continuation must detach the stale leg"
    );
    assert_public_stock_census("INV-072 after Recovery tail progress", &env)
        .expect("post-progress stock census");
    assert_public_encumbrance_census("INV-072 after Recovery tail progress", &env)
        .expect("post-progress encumbrance census");

    let finalize = env
        .finalize_reset_side(ASSET, 1)
        .unwrap_or_else(|error| panic!("{label}: finalize cleaned reset side: {error}"));
    max_compute_units = max_compute_units.max(finalize.compute_units);
    env.set_clock(3, 1_001);
    let restart = env
        .restart_asset_oracle(ASSET, 3, INITIAL_PRICE)
        .unwrap_or_else(|error| panic!("{label}: restart empty Recovery asset: {error}"));
    max_compute_units = max_compute_units.max(restart.compute_units);
    let restarted = env.primary_market_state().1.assets[ASSET as usize];
    assert_eq!(restarted.lifecycle, AssetLifecycleV16::Active, "{label}");
    assert!(restarted.market_id > old_market_id, "{label}");
    assert_eq!(restarted.mode_short, SideModeV16::Normal, "{label}");

    let reducer_capital = env.primary_portfolio(REDUCER).capital.get();
    let counterparty_capital = env.primary_portfolio(STALE_COUNTERPARTY).capital.get();
    for actor in [REDUCER, STALE_COUNTERPARTY] {
        let capital = env.primary_portfolio(actor).capital.get();
        let withdraw = env
            .withdraw_primary(actor, capital)
            .unwrap_or_else(|error| panic!("{label}: actor {actor} withdrawal: {error}"));
        max_compute_units = max_compute_units.max(withdraw.compute_units);
        assert_eq!(env.primary_portfolio(actor).capital.get(), 0, "{label}");
    }
    assert_eq!(env.token_supply_observed(), supply_before, "{label}");
    assert_public_stock_census("INV-072 terminal Recovery tail world", &env)
        .expect("terminal stock census");
    assert_public_encumbrance_census("INV-072 terminal Recovery tail world", &env)
        .expect("terminal encumbrance census");
    assert!(
        max_compute_units < TX_CU_LIMIT,
        "{label}: {max_compute_units}"
    );

    RecoveryTailOutcome {
        old_market_id,
        new_market_id: restarted.market_id,
        reducer_capital,
        counterparty_capital,
        asset_slot: restarted.slot_last,
    }
}

#[test]
fn v16_program_recovery_reset_crank_tail_matrix_is_order_robust() {
    let cases = [
        RecoveryTailCase::NoHint,
        RecoveryTailCase::ZeroAccountHint,
        RecoveryTailCase::StaleProfileOracle,
        RecoveryTailCase::MalformedDeclaredTail,
        RecoveryTailCase::OverdeclaredIgnoredTail,
        RecoveryTailCase::MissingDeclaredTail,
        RecoveryTailCase::UnclaimedTail,
        RecoveryTailCase::DuplicateHint,
        RecoveryTailCase::OutOfRangeHint,
    ];
    let mut expected = None;
    for (index, case) in cases.into_iter().enumerate() {
        let mut seed = [0x72; 32];
        seed[0] ^= index as u8;
        let outcome = recovery_tail_world(case, seed);
        if let Some(expected) = expected.as_ref() {
            assert_eq!(
                &outcome, expected,
                "{case:?}: hint form must not alter normalized terminal economics"
            );
        } else {
            expected = Some(outcome);
        }
    }
}
