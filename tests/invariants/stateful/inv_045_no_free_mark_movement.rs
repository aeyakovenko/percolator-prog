//! INV-045 - No free mark movement.
//!
//! Normative obligation: Every mark movement remains elapsed-time bounded and economically paid across every trade route.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_mark_publication_matrix_rejects_stale_risk_and_recovers` publishes marks through
//! authenticated, EWMA, single-trade, and batch-trade routes. It proves the engine target is staged
//! immediately, stale risk increase rejects with exact rollback, and a post-catch-up round trip
//! remains live without value transfer. `v16_program_trade_route_matrix_rejects_pending_mark_inheritance`
//! signs exposure before a paid mark move and requires the retained request to reject on every
//! route, then proves the same intent shape can trade and exit after catch-up. The pending-target
//! override matrix independently requires a cheap round trip to reject without changing the
//! eventual target or terminal payouts.
//! `v16_program_pending_mark_fee_ordering_rejects_and_preserves_terminal_value` permutes fee
//! synchronization against mark commitment. It requires the pending-order attempt to reject with
//! exact rollback, then verifies the post-commit retry and terminal payouts equal the canonical
//! ordering. `v16_program_trade_route_matrix_keeps_mark_reserve_nonwithdrawable` creates a paid mark
//! move and proves withdrawal rejects with exact rollback both before and after commitment, while
//! the fee keeps the controlling coalition economically non-positive across all trade routes.
//! `v16_program_mark_mode_route_matrix_keeps_liquidation_penalties_nonreclaimable` crosses EWMA and
//! hybrid-after-hours modes with all single/batch CPI/no-CPI routes. It requires liquidation to
//! remain live while the resulting penalty stays out of cranker rewards and withdrawable domain
//! budgets and the controlling coalition remains economically negative.
//! `v16_program_matcher_route_matrix_rejects_one_sided_mark_subsidy` crosses the same modes with
//! single and batch CPI matcher exits and requires every mark-moving fee to be bilaterally funded;
//! it measures independent victim loss, fee-counterparty loss, insurance credit, and external
//! coalition profit. `v16_program_accepted_mark_boundary_matrix_is_paid_atomic_and_exit_live`
//! composes all four trade routes and all four deployed mark regimes with same-slot/maximum-dt
//! landings. Valid raw/quoted prices `1` and `MAX_ORACLE_PRICE` execute as two repeated partial
//! reductions, every same-slot follow-up is movement-free, and any movement is independently
//! bounded by both the accepted-price envelope and collected fee. Raw zero and `u64::MAX` no-CPI
//! inputs, plus equivalent zero/above-maximum CPI quotes, reject with exact economic rollback
//! before a control reduction proves the owner exit remains live. Direct impact tests remain below.
//! `v16_program_generated_interior_mark_envelopes_are_paid_and_exit_live` reuses that whole-route
//! oracle over generated non-boundary anchors, up/down target spreads, per-slot caps, elapsed slots,
//! modes, and routes. Every accepted movement remains inside the independent elapsed-time clamp,
//! is covered by retained insurance, cannot compound on the second same-slot partial reduction,
//! and ends with zero OI and exact coalition/custody conservation.
//! `v16_program_trade_driven_mark_route_orders_converge_economically` exhausts every ordered pair
//! of partial-reduction routes in both trade-driven mark modes and both price directions. Reversing
//! the two routes from an identical public setup must produce the same per-user value, mark/target,
//! insurance, capital, vault, and token-supply outcome. A CPI route following an out-of-matcher
//! fill must first reject with exact rollback; the LP then refreshes its matcher capability through
//! the public config route and the identical reduction must succeed.
//! `v16_program_repeated_trade_driven_mark_steps_are_paid_and_exit_live` chains four differently
//! spaced paid movements and all four trade routes in each EWMA/fallback direction. Every staged
//! target is caught up before the next move, every movement is independently fee-backed, and both
//! owners convert and withdraw fully. A flat stale winner's no-observation crank is required to
//! reject with exact rollback before one authenticated asset observation recertifies it, making the
//! public liveness precondition explicit instead of mistaking `NonProgress` for a funded lock.
//! `v16_program_low_price_ewma_discovery_is_not_pinned_by_clock_first_cranks` crosses EWMA and
//! hybrid-after-hours modes with all four trade routes, both movement directions, and trade-first
//! versus clock-first landing schedules. Its 32 public worlds require identical mark, target,
//! insurance, and owner-value outcomes; elapsed mark movement is nonzero, bounded by the configured
//! maximum-dt horizon, and fully fee-backed. A second same-slot reduction cannot move the mark
//! again, and both owners subsequently close every position. This proves that a permissionless
//! clock-only crank cannot erase trade-discovery capacity by landing first in each slot.
//! `v16_program_pending_trade_mark_replacement_preserves_funding_fee_and_exit` leaves the first
//! paid mark target partially pending, lands a second paid reduction one slot later, and then
//! completes canonical catch-up. Across both trade-driven modes, all four routes, and both price
//! directions, the first funding boundary remains immutable, each move is independently funded,
//! catch-up activates both marks in order, route economics are identical, and both owners convert
//! and withdraw all remaining value.
//! These tests exercise the deployed public wrapper with real SBF/LiteSVM account construction and
//! assert economic state, token, rollback, liveness, or compute outcomes appropriate to the
//! invariant.
//!
//! Guarantee boundary: these matrices are fixed-pin certification over generated seeds and public
//! LiteSVM routes; they are not exhaustive proofs over all full-width state combinations.

use super::*;
use crate::support::v16_svm::{MarketConfig, PublicTerminalClassification, TxSuccess, V16Svm};
use percolator::POS_SCALE;
use percolator_prog::ix::{BatchTradeCpiLeg, BatchTradeLeg, CrankObservationHint};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AcceptedMarkMode {
    AuthMark,
    EwmaMark,
    HybridFresh,
    HybridAfterHours,
}

impl AcceptedMarkMode {
    const ALL: [Self; 4] = [
        Self::AuthMark,
        Self::EwmaMark,
        Self::HybridFresh,
        Self::HybridAfterHours,
    ];

    fn updates_from_trade(self) -> bool {
        matches!(self, Self::EwmaMark | Self::HybridAfterHours)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AcceptedMarkDt {
    SameSlot,
    Maximum,
}

impl AcceptedMarkDt {
    const ALL: [Self; 2] = [Self::SameSlot, Self::Maximum];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AcceptedMarkBoundary {
    Zero,
    One,
    Maximum,
    U64MaximumOrCpiAboveMaximum,
}

impl AcceptedMarkBoundary {
    const VALID: [Self; 2] = [Self::One, Self::Maximum];
    const INVALID: [Self; 2] = [Self::Zero, Self::U64MaximumOrCpiAboveMaximum];

    fn anchor(self) -> u64 {
        match self {
            Self::Zero | Self::One => 10_000,
            Self::Maximum => percolator::MAX_ORACLE_PRICE / 2,
            Self::U64MaximumOrCpiAboveMaximum => percolator::MAX_ORACLE_PRICE,
        }
    }

    fn signed_size(self) -> i128 {
        match self {
            Self::Zero | Self::One => -(POS_SCALE as i128),
            Self::Maximum | Self::U64MaximumOrCpiAboveMaximum => 2,
        }
    }

    fn no_cpi_price(self) -> u64 {
        match self {
            Self::Zero => 0,
            Self::One => 1,
            Self::Maximum => percolator::MAX_ORACLE_PRICE,
            Self::U64MaximumOrCpiAboveMaximum => u64::MAX,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AcceptedMarkSnapshot {
    market: Vec<u8>,
    foreign_market: Vec<u8>,
    portfolios: Vec<Vec<u8>>,
    foreign_portfolio: Vec<u8>,
    tokens: Vec<(solana_sdk::pubkey::Pubkey, Vec<u8>)>,
    lamports: Vec<(solana_sdk::pubkey::Pubkey, u64)>,
}

fn accepted_mark_snapshot(env: &V16Svm) -> AcceptedMarkSnapshot {
    AcceptedMarkSnapshot {
        market: env.market_data(false),
        foreign_market: env.market_data(true),
        portfolios: env.all_primary_portfolio_data(),
        foreign_portfolio: env.foreign_portfolio_data(),
        tokens: env.all_token_account_data(),
        lamports: env.all_economic_account_lamports(),
    }
}

fn accepted_mark_reference_clamp(anchor: u64, target: u64, cap_bps: u64, dt: u64) -> u64 {
    if anchor == 0 || target == 0 {
        return target;
    }
    if cap_bps == 0 || dt == 0 {
        return anchor;
    }
    let max_delta = (anchor as u128)
        .checked_mul(cap_bps as u128)
        .and_then(|value| value.checked_mul(dt as u128))
        .expect("accepted-mark reference delta")
        / 10_000;
    let max_delta = u64::try_from(max_delta.min(u64::MAX as u128)).unwrap();
    if target > anchor {
        target.min(anchor.saturating_add(max_delta))
    } else {
        target.max(anchor.saturating_sub(max_delta))
    }
}

fn accepted_mark_reference_move_bps(old: u64, new: u64) -> u64 {
    if old == new {
        return 0;
    }
    let numerator = (old.abs_diff(new) as u128)
        .checked_mul(10_000)
        .and_then(|value| value.checked_add(old as u128 - 1))
        .expect("accepted-mark reference movement");
    u64::try_from(numerator / old as u128).expect("accepted-mark movement fits u64")
}

fn accepted_mark_required_externality_fee(
    oi_eff_long_q: u128,
    oi_eff_short_q: u128,
    effective_price: u64,
    old_mark: u64,
    trade_size_q_abs: u128,
    accepted_price: u64,
    new_mark: u64,
) -> Result<u128, String> {
    let trade_notional = trade_size_q_abs
        .checked_mul(accepted_price as u128)
        .and_then(|value| value.checked_add(POS_SCALE - 1))
        .ok_or_else(|| "accepted-mark trade notional overflow".to_string())?
        / POS_SCALE;
    let externality_price = effective_price.max(old_mark);
    let max_side_notional = oi_eff_long_q
        .max(oi_eff_short_q)
        .checked_mul(externality_price as u128)
        .and_then(|value| value.checked_add(POS_SCALE - 1))
        .ok_or_else(|| "accepted-mark side notional overflow".to_string())?
        / POS_SCALE;
    let move_bps = accepted_mark_reference_move_bps(old_mark, new_mark);
    max_side_notional
        .max(trade_notional)
        .checked_mul(2)
        .and_then(|value| value.checked_mul(move_bps as u128))
        .and_then(|value| value.checked_add(9_999))
        .map(|value| value / 10_000)
        .ok_or_else(|| "accepted-mark externality fee overflow".to_string())
}

fn accepted_mark_pair_value_and_insurance(env: &V16Svm) -> i128 {
    let group = env.primary_market_state().1;
    let account_value = [0usize, 1]
        .into_iter()
        .map(|actor| {
            let account = env.primary_portfolio(actor);
            i128::try_from(account.capital.get()).expect("test capital fits i128")
                + account.pnl.get()
        })
        .sum::<i128>();
    account_value + i128::try_from(group.insurance).expect("test insurance fits i128")
}

fn accepted_mark_actor_values(env: &V16Svm) -> [i128; 2] {
    [0usize, 1].map(|actor| {
        let account = env.primary_portfolio(actor);
        i128::try_from(account.capital.get()).expect("test capital fits i128") + account.pnl.get()
    })
}

fn accepted_mark_route_is_cpi(route: DiscoveryTradeRoute) -> bool {
    matches!(
        route,
        DiscoveryTradeRoute::Cpi | DiscoveryTradeRoute::BatchCpi
    )
}

fn configure_accepted_mark_quote(
    env: &mut V16Svm,
    route: DiscoveryTradeRoute,
    boundary: AcceptedMarkBoundary,
) -> Result<u64, String> {
    if !accepted_mark_route_is_cpi(route) {
        return Ok(boundary.no_cpi_price());
    }
    let (bid_spread_bps, ask_spread_bps, quoted_price) = match boundary {
        AcceptedMarkBoundary::Zero => (10_000, 0, 0),
        AcceptedMarkBoundary::One => (9_999, 0, 1),
        AcceptedMarkBoundary::Maximum => (0, 10_000, percolator::MAX_ORACLE_PRICE),
        AcceptedMarkBoundary::U64MaximumOrCpiAboveMaximum => (
            0,
            10_000,
            percolator::MAX_ORACLE_PRICE
                .checked_mul(2)
                .expect("CPI above-maximum quote"),
        ),
    };
    env.set_matcher_spreads(1, bid_spread_bps, ask_spread_bps)
        .map_err(|error| format!("configure {route:?} boundary matcher: {error}"))?;
    Ok(quoted_price)
}

fn configure_accepted_mark_target_quote(
    env: &mut V16Svm,
    route: DiscoveryTradeRoute,
    anchor: u64,
    target: u64,
) -> Result<(), String> {
    if !accepted_mark_route_is_cpi(route) {
        return Ok(());
    }
    let (bid_spread_bps, ask_spread_bps) = if target < anchor {
        let spread = u64::try_from((anchor - target) as u128 * 10_000 / anchor as u128)
            .map_err(|_| "generated bid spread exceeds u64".to_string())?;
        (spread, 0)
    } else {
        let spread = u64::try_from((target - anchor) as u128 * 10_000 / anchor as u128)
            .map_err(|_| "generated ask spread exceeds u64".to_string())?;
        (0, spread)
    };
    env.set_matcher_spreads(1, bid_spread_bps, ask_spread_bps)
        .map(|_| ())
        .map_err(|error| format!("configure {route:?} target matcher: {error}"))
}

fn submit_accepted_mark_trade(
    env: &mut V16Svm,
    route: DiscoveryTradeRoute,
    size_q: i128,
    no_cpi_price: u64,
) -> Result<TxSuccess, String> {
    let market_id = env.primary_market_state().1.assets[0].market_id;
    match route {
        DiscoveryTradeRoute::NoCpi => env.trade_no_cpi(0, 1, 0, size_q, no_cpi_price, 0),
        DiscoveryTradeRoute::BatchNoCpi => env.batch_trade_no_cpi(
            0,
            1,
            vec![BatchTradeLeg {
                asset_index: 0,
                market_id,
                size_q,
                exec_price: no_cpi_price,
                fee_bps: 0,
            }],
        ),
        DiscoveryTradeRoute::Cpi => env.trade_cpi(0, 1, 0, size_q, 0, 0),
        DiscoveryTradeRoute::BatchCpi => env.batch_trade_cpi(
            0,
            1,
            vec![BatchTradeCpiLeg {
                asset_index: 0,
                market_id,
                size_q,
                fee_bps: 0,
                limit_price: 0,
            }],
        ),
    }
}

fn configure_accepted_mark_mode(
    env: &mut V16Svm,
    mode: AcceptedMarkMode,
    anchor: u64,
) -> Result<Option<solana_sdk::pubkey::Pubkey>, String> {
    let oracle = match mode {
        AcceptedMarkMode::AuthMark => None,
        AcceptedMarkMode::EwmaMark => {
            env.configure_ewma_mark(0, 1, anchor, 1, 0)
                .map_err(|error| format!("configure EWMA boundary world: {error}"))?;
            None
        }
        AcceptedMarkMode::HybridFresh | AcceptedMarkMode::HybridAfterHours => {
            env.set_clock(1, 100);
            let mut feed = [0x45; 32];
            feed[0] = match mode {
                AcceptedMarkMode::HybridFresh => 0xf1,
                AcceptedMarkMode::HybridAfterHours => 0xa1,
                _ => unreachable!(),
            };
            let oracle = env.set_pyth_price(&feed, anchor as i64, -6, 0, 100);
            let soft_stale_slots = match mode {
                AcceptedMarkMode::HybridFresh => 100,
                AcceptedMarkMode::HybridAfterHours => 1,
                _ => unreachable!(),
            };
            env.configure_hybrid_oracle(
                0,
                1,
                100,
                0,
                [feed, [0; 32], [0; 32]],
                &[oracle],
                soft_stale_slots,
                0,
            )
            .map_err(|error| format!("configure {mode:?} boundary world: {error}"))?;
            Some(oracle)
        }
    };

    Ok(oracle)
}

fn prepare_accepted_mark_landing_dt(
    env: &mut V16Svm,
    mode: AcceptedMarkMode,
    dt: u64,
    oracle: Option<solana_sdk::pubkey::Pubkey>,
) -> Result<u64, String> {
    let base_slot = if mode == AcceptedMarkMode::HybridAfterHours {
        // Enter the after-hours regime through an authenticated public crank first. Measuring the
        // generated dt from this committed state prevents a small dt from accidentally testing the
        // still-fresh hybrid branch while claiming trade-driven fallback semantics.
        env.set_clock(3, 1_000);
        let oracle = oracle.ok_or_else(|| "stale-hybrid oracle missing".to_string())?;
        env.crank_with_oracles(
            0,
            3,
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 1,
            }],
            &[oracle],
        )
        .map_err(|error| format!("prime stale-hybrid crank: {error}"))?;
        let slot_last = env.primary_market_state().1.assets[0].slot_last;
        if slot_last != 3 {
            return Err(format!(
                "stale-hybrid primer did not advance the asset to slot 3: {slot_last}"
            ));
        }
        slot_last
    } else {
        1
    };
    let landing_slot = base_slot
        .checked_add(dt)
        .ok_or_else(|| "accepted-mark landing slot overflow".to_string())?;
    match mode {
        AcceptedMarkMode::HybridFresh => env.set_clock(landing_slot, 100 + landing_slot as i64),
        AcceptedMarkMode::HybridAfterHours => env.set_clock(landing_slot, 1_000),
        AcceptedMarkMode::AuthMark | AcceptedMarkMode::EwmaMark => env.warp_to_slot(landing_slot),
    }
    Ok(landing_slot)
}

fn prepare_accepted_mark_landing(
    env: &mut V16Svm,
    mode: AcceptedMarkMode,
    dt: AcceptedMarkDt,
    oracle: Option<solana_sdk::pubkey::Pubkey>,
) -> Result<u64, String> {
    let dt = match dt {
        AcceptedMarkDt::SameSlot => 0,
        AcceptedMarkDt::Maximum => 4,
    };
    prepare_accepted_mark_landing_dt(env, mode, dt, oracle)
}

fn advance_accepted_mark_landing_from_current(
    env: &mut V16Svm,
    mode: AcceptedMarkMode,
    dt: u64,
) -> Result<u64, String> {
    let asset_slot = env.primary_market_state().1.assets[0].slot_last;
    let landing_slot = asset_slot
        .checked_add(dt)
        .ok_or_else(|| "accepted-mark repeated landing slot overflow".to_string())?;
    match mode {
        AcceptedMarkMode::HybridAfterHours => {
            let unix_timestamp = i64::try_from(1_000u64.saturating_add(landing_slot))
                .map_err(|_| "accepted-mark repeated timestamp overflow".to_string())?;
            env.set_clock(landing_slot, unix_timestamp);
        }
        AcceptedMarkMode::EwmaMark => env.warp_to_slot(landing_slot),
        AcceptedMarkMode::AuthMark | AcceptedMarkMode::HybridFresh => {
            return Err(format!(
                "{mode:?} does not admit repeated trade-driven mark landings"
            ));
        }
    }
    Ok(landing_slot)
}

fn crank_accepted_mark_target(
    env: &mut V16Svm,
    mode: AcceptedMarkMode,
    dt: u64,
    oracle: Option<solana_sdk::pubkey::Pubkey>,
    context: &str,
) -> Result<TxSuccess, String> {
    let before_profile = env.primary_profile(0);
    let before_group = env.primary_market_state().1;
    let before_supply = env.token_supply_observed();
    let before_foreign = env.market_data(true);
    let landing_slot = advance_accepted_mark_landing_from_current(env, mode, dt)?;
    let observations = vec![CrankObservationHint {
        asset_index: 0,
        oracle_accounts: u8::from(mode == AcceptedMarkMode::HybridAfterHours),
    }];
    let crank = if mode == AcceptedMarkMode::HybridAfterHours {
        let oracle = oracle.ok_or_else(|| format!("{context}: hybrid oracle missing"))?;
        env.crank_with_oracles(0, landing_slot, observations, &[oracle])
    } else {
        env.crank(0, landing_slot, observations)
    }
    .map_err(|error| format!("{context}: permissionless mark catch-up: {error}"))?;
    let after_profile = env.primary_profile(0);
    let after_group = env.primary_market_state().1;
    if after_group.assets[0].slot_last != landing_slot
        || after_group.assets[0].effective_price != before_profile.mark_ewma_e6
        || after_group.assets[0].raw_oracle_target_price != before_profile.mark_ewma_e6
        || after_profile.mark_ewma_e6 != before_profile.mark_ewma_e6
        || after_group.assets[0].oi_eff_long_q != before_group.assets[0].oi_eff_long_q
        || after_group.assets[0].oi_eff_short_q != before_group.assets[0].oi_eff_short_q
        || after_group.vault != before_group.vault
        || after_group.insurance != before_group.insurance
        || env.token_supply_observed() != before_supply
        || env.market_data(true) != before_foreign
    {
        return Err(format!(
            "{context}: bounded catch-up mismatch: slot {}->{}/want {landing_slot}, effective {}->{}/want {}, raw {}->{}, mark {}->{}, oi ({},{})->({},{}), vault {}->{}, insurance {}->{}, supply {}->{}, foreign_equal={}",
            before_group.assets[0].slot_last,
            after_group.assets[0].slot_last,
            before_group.assets[0].effective_price,
            after_group.assets[0].effective_price,
            before_profile.mark_ewma_e6,
            before_group.assets[0].raw_oracle_target_price,
            after_group.assets[0].raw_oracle_target_price,
            before_profile.mark_ewma_e6,
            after_profile.mark_ewma_e6,
            before_group.assets[0].oi_eff_long_q,
            before_group.assets[0].oi_eff_short_q,
            after_group.assets[0].oi_eff_long_q,
            after_group.assets[0].oi_eff_short_q,
            before_group.vault,
            after_group.vault,
            before_group.insurance,
            after_group.insurance,
            before_supply,
            env.token_supply_observed(),
            env.market_data(true) == before_foreign,
        ));
    }
    crate::support::fuzz_model::assert_public_stock_census(context, env)?;
    crate::support::fuzz_model::assert_public_encumbrance_census(context, env)?;
    Ok(crank)
}

#[derive(Clone, Copy, Debug)]
struct AcceptedMarkCase {
    seed: [u8; 32],
    anchor: u64,
    target: u64,
    total_size: i128,
    cap_bps: u64,
    max_dt: u64,
    landing_dt: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AcceptedMarkEconomicOutcome {
    actor_values: [i128; 2],
    mark: u64,
    raw_target: u64,
    effective_price: u64,
    insurance: u128,
    capital_total: u128,
    vault: u128,
    token_supply: u128,
}

#[derive(Clone, Debug)]
struct AcceptedMarkRun {
    outcome: AcceptedMarkEconomicOutcome,
    max_cu: u64,
    stale_cpi_rejections: usize,
}

#[derive(Debug)]
struct AcceptedMarkSubmission {
    trade: TxSuccess,
    refresh: Option<TxSuccess>,
    stale_cpi_rejected: bool,
}

fn submit_accepted_mark_trade_after_route(
    env: &mut V16Svm,
    previous_route: DiscoveryTradeRoute,
    route: DiscoveryTradeRoute,
    size_q: i128,
    no_cpi_price: u64,
    context: &str,
) -> Result<AcceptedMarkSubmission, String> {
    if accepted_mark_route_is_cpi(route) && !accepted_mark_route_is_cpi(previous_route) {
        let before = accepted_mark_snapshot(env);
        if submit_accepted_mark_trade(env, route, size_q, no_cpi_price).is_ok() {
            return Err(format!(
                "{context}: stale matcher capability survived an out-of-matcher position mutation"
            ));
        }
        if accepted_mark_snapshot(env) != before {
            return Err(format!(
                "{context}: stale matcher-capability rejection did not roll back exactly"
            ));
        }
        let refresh = env
            .set_matcher_config(1, 1)
            .map_err(|error| format!("{context}: refresh matcher capability: {error}"))?;
        let trade = submit_accepted_mark_trade(env, route, size_q, no_cpi_price)
            .map_err(|error| format!("{context}: refreshed matcher fill: {error}"))?;
        return Ok(AcceptedMarkSubmission {
            trade,
            refresh: Some(refresh),
            stale_cpi_rejected: true,
        });
    }

    submit_accepted_mark_trade(env, route, size_q, no_cpi_price)
        .map(|trade| AcceptedMarkSubmission {
            trade,
            refresh: None,
            stale_cpi_rejected: false,
        })
        .map_err(|error| format!("{context}: route fill: {error}"))
}

fn run_valid_accepted_mark_route_sequence(
    mode: AcceptedMarkMode,
    setup_route: DiscoveryTradeRoute,
    first_route: DiscoveryTradeRoute,
    second_route: DiscoveryTradeRoute,
    case: AcceptedMarkCase,
) -> Result<AcceptedMarkRun, String> {
    let context = format!(
        "{mode:?}/{setup_route:?}->{first_route:?}->{second_route:?}/anchor={}/target={}/cap={}/max_dt={}/dt={}",
        case.anchor, case.target, case.cap_bps, case.max_dt, case.landing_dt
    );
    let mut env = V16Svm::new(
        case.seed,
        MarketConfig {
            initial_price: case.anchor,
            max_trading_fee_bps: 100,
            max_price_move_bps_per_slot: case.cap_bps,
            max_accrual_dt_slots: case.max_dt,
            min_funding_lifetime_slots: case.max_dt,
            ..MarketConfig::default()
        },
    );
    let oracle = configure_accepted_mark_mode(&mut env, mode, case.anchor)?;
    let half_size = case.total_size / 2;
    if half_size == 0 || half_size.checked_mul(2) != Some(case.total_size) {
        return Err(format!("{context}: split-fill size is not exact"));
    }
    if accepted_mark_route_is_cpi(setup_route) {
        env.set_matcher_spreads(1, 0, 0)
            .map_err(|error| format!("configure {setup_route:?} setup matcher: {error}"))?;
    }
    let setup = submit_accepted_mark_trade(&mut env, setup_route, -case.total_size, case.anchor)
        .map_err(|error| format!("{context} setup open: {error}"))?;
    let setup_group = env.primary_market_state().1;
    if setup_group.assets[0].oi_eff_long_q != case.total_size.unsigned_abs()
        || setup_group.assets[0].oi_eff_short_q != case.total_size.unsigned_abs()
    {
        return Err(format!("{context}: setup did not create exact matched OI"));
    }
    let landing_slot = prepare_accepted_mark_landing_dt(&mut env, mode, case.landing_dt, oracle)?;
    configure_accepted_mark_target_quote(&mut env, first_route, case.anchor, case.target)?;

    let before_profile = env.primary_profile(0);
    let before_group = env.primary_market_state().1;
    let before_pair_value = accepted_mark_pair_value_and_insurance(&env);
    let before_supply = env.token_supply_observed();
    let before_foreign = env.market_data(true);
    let asset_dt = landing_slot
        .checked_sub(before_group.assets[0].slot_last)
        .ok_or_else(|| "accepted-mark landing preceded asset slot".to_string())?;
    if asset_dt != case.landing_dt {
        return Err(format!(
            "{context}: expected asset dt {}, got {asset_dt}",
            case.landing_dt
        ));
    }
    let mark_dt = landing_slot
        .checked_sub(before_profile.mark_ewma_last_slot)
        .ok_or_else(|| "accepted-mark landing preceded mark slot".to_string())?
        .min(case.max_dt);

    let first = submit_accepted_mark_trade_after_route(
        &mut env,
        setup_route,
        first_route,
        half_size,
        case.target,
        &format!("{context} first fill"),
    )?;
    let first_profile = env.primary_profile(0);
    let first_group = env.primary_market_state().1;
    let expected_accepted = if mode.updates_from_trade() {
        accepted_mark_reference_clamp(case.anchor, case.target, case.cap_bps, mark_dt)
    } else {
        before_group.assets[0].effective_price
    };
    let low = before_profile.mark_ewma_e6.min(expected_accepted);
    let high = before_profile.mark_ewma_e6.max(expected_accepted);
    if first_profile.mark_ewma_e6 < low || first_profile.mark_ewma_e6 > high {
        return Err(format!(
            "{context}: mark {} escaped accepted interval [{low}, {high}]",
            first_profile.mark_ewma_e6
        ));
    }
    if (!mode.updates_from_trade() || mark_dt == 0)
        && first_profile.mark_ewma_e6 != before_profile.mark_ewma_e6
    {
        return Err(format!(
            "{context}: non-updating/same-slot trade moved mark"
        ));
    }
    if mode.updates_from_trade()
        && mark_dt != 0
        && case.target != case.anchor
        && first_profile.mark_ewma_e6 == before_profile.mark_ewma_e6
    {
        return Err(format!("{context}: paid mark-movement case was vacuous"));
    }
    let move_bps =
        accepted_mark_reference_move_bps(before_profile.mark_ewma_e6, first_profile.mark_ewma_e6);
    let insurance_gain = first_group
        .insurance
        .checked_sub(before_group.insurance)
        .ok_or_else(|| "accepted-mark fill reduced insurance".to_string())?;
    let trade_notional = half_size
        .unsigned_abs()
        .checked_mul(expected_accepted as u128)
        .and_then(|value| value.checked_add(POS_SCALE - 1))
        .ok_or_else(|| "accepted-mark trade notional overflow".to_string())?
        / POS_SCALE;
    let externality_price = before_group.assets[0]
        .effective_price
        .max(before_profile.mark_ewma_e6);
    let max_side_notional = before_group.assets[0]
        .oi_eff_long_q
        .max(before_group.assets[0].oi_eff_short_q)
        .checked_mul(externality_price as u128)
        .and_then(|value| value.checked_add(POS_SCALE - 1))
        .ok_or_else(|| "accepted-mark passive notional overflow".to_string())?
        / POS_SCALE;
    let externality_notional = max_side_notional
        .max(trade_notional)
        .checked_mul(2)
        .ok_or_else(|| "accepted-mark externality overflow".to_string())?;
    let required_movement_fee = externality_notional
        .checked_mul(move_bps as u128)
        .and_then(|value| value.checked_add(9_999))
        .ok_or_else(|| "accepted-mark required fee overflow".to_string())?
        / 10_000;
    if insurance_gain < required_movement_fee || (move_bps != 0 && insurance_gain == 0) {
        return Err(format!(
            "{context}: {move_bps} bps movement required {required_movement_fee}, collected {insurance_gain}"
        ));
    }
    if mode.updates_from_trade() {
        if first_group.assets[0].raw_oracle_target_price != first_profile.mark_ewma_e6 {
            return Err(format!("{context}: wrapper/engine target staging diverged"));
        }
    } else if first_group.assets[0].raw_oracle_target_price
        != before_group.assets[0].raw_oracle_target_price
    {
        return Err(format!(
            "{context}: trade rewrote a non-trade-driven target"
        ));
    }

    configure_accepted_mark_target_quote(&mut env, second_route, case.anchor, case.target)?;
    let second = submit_accepted_mark_trade_after_route(
        &mut env,
        first_route,
        second_route,
        half_size,
        case.target,
        &format!("{context} second fill"),
    )?;
    let second_profile = env.primary_profile(0);
    if second_profile.mark_ewma_e6 != first_profile.mark_ewma_e6 {
        return Err(format!(
            "{context}: repeated same-slot partial fill compounded mark movement"
        ));
    }
    let exited = env.primary_market_state().1;
    if exited.assets[0].oi_eff_long_q != 0 || exited.assets[0].oi_eff_short_q != 0 {
        return Err(format!("{context}: bounded exit left OI"));
    }
    if accepted_mark_pair_value_and_insurance(&env) != before_pair_value
        || env.token_supply_observed() != before_supply
        || env.market_data(true) != before_foreign
    {
        return Err(format!(
            "{context}: partial-reduction sequence changed attributed pair value, token supply, or foreign state"
        ));
    }
    let max_cu = first
        .trade
        .compute_units
        .max(second.trade.compute_units)
        .max(setup.compute_units)
        .max(
            first
                .refresh
                .as_ref()
                .map_or(0, |success| success.compute_units),
        )
        .max(
            second
                .refresh
                .as_ref()
                .map_or(0, |success| success.compute_units),
        );
    if max_cu >= crate::support::v16_svm::TX_CU_LIMIT {
        return Err(format!("{context}: route consumed {max_cu} CU"));
    }
    Ok(AcceptedMarkRun {
        outcome: AcceptedMarkEconomicOutcome {
            actor_values: accepted_mark_actor_values(&env),
            mark: second_profile.mark_ewma_e6,
            raw_target: exited.assets[0].raw_oracle_target_price,
            effective_price: exited.assets[0].effective_price,
            insurance: exited.insurance,
            capital_total: exited.c_tot,
            vault: exited.vault,
            token_supply: env.token_supply_observed(),
        },
        max_cu,
        stale_cpi_rejections: usize::from(first.stale_cpi_rejected)
            + usize::from(second.stale_cpi_rejected),
    })
}

fn run_valid_accepted_mark_case(
    mode: AcceptedMarkMode,
    route: DiscoveryTradeRoute,
    case: AcceptedMarkCase,
) -> Result<u64, String> {
    run_valid_accepted_mark_route_sequence(mode, route, route, route, case).map(|run| run.max_cu)
}

fn run_valid_accepted_mark_boundary(
    mode: AcceptedMarkMode,
    route: DiscoveryTradeRoute,
    dt: AcceptedMarkDt,
    boundary: AcceptedMarkBoundary,
) -> Result<u64, String> {
    let landing_dt = match dt {
        AcceptedMarkDt::SameSlot => 0,
        AcceptedMarkDt::Maximum => 4,
    };
    let mut seed = [0x45; 32];
    seed[0] = mode as u8;
    seed[1] = route as u8;
    seed[2] = dt as u8;
    seed[3] = boundary as u8;
    run_valid_accepted_mark_case(
        mode,
        route,
        AcceptedMarkCase {
            seed,
            anchor: boundary.anchor(),
            target: boundary.no_cpi_price(),
            total_size: boundary.signed_size(),
            cap_bps: 50,
            max_dt: 4,
            landing_dt,
        },
    )
}

fn run_invalid_accepted_mark_boundary(
    route: DiscoveryTradeRoute,
    dt: AcceptedMarkDt,
    boundary: AcceptedMarkBoundary,
) -> Result<u64, String> {
    const MAX_DT: u64 = 4;
    let mode = AcceptedMarkMode::EwmaMark;
    let anchor = boundary.anchor();
    let mut seed = [0xb5; 32];
    seed[0] = route as u8;
    seed[1] = dt as u8;
    seed[2] = boundary as u8;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: anchor,
            max_trading_fee_bps: 100,
            max_price_move_bps_per_slot: 50,
            max_accrual_dt_slots: MAX_DT,
            min_funding_lifetime_slots: MAX_DT,
            ..MarketConfig::default()
        },
    );
    let oracle = configure_accepted_mark_mode(&mut env, mode, anchor)?;
    if accepted_mark_route_is_cpi(route) {
        env.set_matcher_spreads(1, 0, 0)
            .map_err(|error| format!("configure invalid {route:?} setup matcher: {error}"))?;
    }
    let setup = submit_accepted_mark_trade(&mut env, route, -boundary.signed_size(), anchor)
        .map_err(|error| format!("{route:?}/{dt:?}/{boundary:?} setup open: {error}"))?;
    prepare_accepted_mark_landing(&mut env, mode, dt, oracle)?;
    let _quoted_price = configure_accepted_mark_quote(&mut env, route, boundary)?;
    let before = accepted_mark_snapshot(&env);
    let rejected = submit_accepted_mark_trade(
        &mut env,
        route,
        boundary.signed_size(),
        boundary.no_cpi_price(),
    );
    if rejected.is_ok() || accepted_mark_snapshot(&env) != before {
        return Err(format!(
            "{route:?}/{dt:?}/{boundary:?}: invalid raw/quoted price did not reject with exact rollback"
        ));
    }

    if accepted_mark_route_is_cpi(route) {
        env.set_matcher_spreads(1, 0, 0)
            .map_err(|error| format!("reset invalid {route:?} matcher: {error}"))?;
    }
    let pair_before = accepted_mark_pair_value_and_insurance(&env);
    let supply_before = env.token_supply_observed();
    let close = submit_accepted_mark_trade(&mut env, route, boundary.signed_size(), anchor)
        .map_err(|error| format!("{route:?}/{dt:?}/{boundary:?} control close: {error}"))?;
    let group = env.primary_market_state().1;
    if group.assets[0].oi_eff_long_q != 0
        || group.assets[0].oi_eff_short_q != 0
        || accepted_mark_pair_value_and_insurance(&env) != pair_before
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "{route:?}/{dt:?}/{boundary:?}: rejected boundary poisoned the control exit"
        ));
    }
    let max_cu = setup.compute_units.max(close.compute_units);
    if max_cu >= crate::support::v16_svm::TX_CU_LIMIT {
        return Err(format!(
            "{route:?}/{dt:?}/{boundary:?}: control route consumed {max_cu} CU"
        ));
    }
    Ok(max_cu)
}

#[test]
fn v16_program_accepted_mark_boundary_matrix_is_paid_atomic_and_exit_live() {
    let mut valid_cells = 0usize;
    let mut invalid_cells = 0usize;
    let mut max_cu = 0u64;
    for mode in AcceptedMarkMode::ALL {
        for route in DiscoveryTradeRoute::ALL {
            for dt in AcceptedMarkDt::ALL {
                for boundary in AcceptedMarkBoundary::VALID {
                    max_cu = max_cu.max(
                        run_valid_accepted_mark_boundary(mode, route, dt, boundary)
                            .unwrap_or_else(|error| panic!("valid accepted-mark cell: {error}")),
                    );
                    valid_cells += 1;
                }
            }
        }
    }
    for route in DiscoveryTradeRoute::ALL {
        for dt in AcceptedMarkDt::ALL {
            for boundary in AcceptedMarkBoundary::INVALID {
                max_cu = max_cu.max(
                    run_invalid_accepted_mark_boundary(route, dt, boundary)
                        .unwrap_or_else(|error| panic!("invalid accepted-mark cell: {error}")),
                );
                invalid_cells += 1;
            }
        }
    }
    assert_eq!(valid_cells, 64, "complete valid mode/route/dt matrix");
    assert_eq!(invalid_cells, 16, "complete invalid route/dt matrix");
    assert!(max_cu < crate::support::v16_svm::TX_CU_LIMIT);
}

#[test]
fn v16_program_trade_driven_mark_route_orders_converge_economically() {
    let mut world_count = 0usize;
    let mut reversal_count = 0usize;
    let mut stale_cpi_rejections = 0usize;
    let mut max_cu = 0u64;

    for mode in [
        AcceptedMarkMode::EwmaMark,
        AcceptedMarkMode::HybridAfterHours,
    ] {
        for rises in [false, true] {
            for first_index in 0..DiscoveryTradeRoute::ALL.len() {
                for second_index in first_index..DiscoveryTradeRoute::ALL.len() {
                    let setup_index = (first_index + second_index) % DiscoveryTradeRoute::ALL.len();
                    let setup_route = DiscoveryTradeRoute::ALL[setup_index];
                    let first_route = DiscoveryTradeRoute::ALL[first_index];
                    let second_route = DiscoveryTradeRoute::ALL[second_index];
                    let mut seed = [0x4f; 32];
                    seed[0] = mode as u8;
                    seed[1] = rises as u8;
                    seed[2] = first_index as u8;
                    seed[3] = second_index as u8;
                    let case = AcceptedMarkCase {
                        seed,
                        anchor: 1_000_000,
                        target: if rises { 1_250_000 } else { 750_000 },
                        total_size: if rises {
                            POS_SCALE as i128
                        } else {
                            -(POS_SCALE as i128)
                        },
                        cap_bps: 25,
                        max_dt: 6,
                        landing_dt: 3,
                    };
                    let forward = run_valid_accepted_mark_route_sequence(
                        mode,
                        setup_route,
                        first_route,
                        second_route,
                        case,
                    )
                    .unwrap_or_else(|error| panic!("forward route-order cell failed: {error}"));
                    max_cu = max_cu.max(forward.max_cu);
                    stale_cpi_rejections += forward.stale_cpi_rejections;
                    world_count += 1;

                    if first_index != second_index {
                        let reverse = run_valid_accepted_mark_route_sequence(
                            mode,
                            setup_route,
                            second_route,
                            first_route,
                            case,
                        )
                        .unwrap_or_else(|error| panic!("reverse route-order cell failed: {error}"));
                        assert_eq!(
                            reverse.outcome, forward.outcome,
                            "{mode:?}/{rises:?}/{first_route:?}<->{second_route:?}: route landing order changed the normalized economic outcome"
                        );
                        max_cu = max_cu.max(reverse.max_cu);
                        stale_cpi_rejections += reverse.stale_cpi_rejections;
                        reversal_count += 1;
                        world_count += 1;
                    }
                }
            }
        }
    }

    assert_eq!(reversal_count, 24, "two modes/directions x six route pairs");
    assert_eq!(world_count, 64, "complete ordered route-pair world count");
    assert_eq!(
        stale_cpi_rejections, 32,
        "every no-CPI-to-CPI transition rejects atomically before public refresh"
    );
    assert!(max_cu < crate::support::v16_svm::TX_CU_LIMIT);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LowPriceCrankOrderOutcome {
    mark_ewma_e6: u64,
    mark_ewma_last_slot: u64,
    raw_target_price: u64,
    effective_price: u64,
    insurance: u128,
    pair_value_and_insurance: i128,
    max_cu: u64,
}

fn run_low_price_crank_order(
    mode: AcceptedMarkMode,
    route: DiscoveryTradeRoute,
    rises: bool,
    crank_before_trade: bool,
) -> Result<LowPriceCrankOrderOutcome, String> {
    const ANCHOR: u64 = 100;
    const ELAPSED_SLOTS: u64 = 9;
    const CAP_BPS: u64 = 24;
    const REDUCE_Q: i128 = POS_SCALE as i128;

    let reported_price = if rises { 200 } else { 50 };
    let reduce_q = if rises { REDUCE_Q } else { -REDUCE_Q };
    let open_q = -reduce_q
        .checked_mul(10)
        .ok_or_else(|| "low-price setup quantity overflow".to_string())?;
    let context = format!("{mode:?}/{route:?}/rises={rises}/clock-first={crank_before_trade}");

    let mut seed = [0x5e; 32];
    seed[0] ^= mode as u8;
    seed[1] ^= route as u8;
    seed[2] ^= u8::from(rises);
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: ANCHOR,
            max_trading_fee_bps: 1_000,
            max_price_move_bps_per_slot: CAP_BPS,
            max_accrual_dt_slots: 20,
            min_funding_lifetime_slots: 20,
            ..MarketConfig::default()
        },
    );
    let oracle = configure_accepted_mark_mode(&mut env, mode, ANCHOR)
        .map_err(|error| format!("{context}: configure low-price world: {error}"))?;
    if accepted_mark_route_is_cpi(route) {
        env.set_matcher_spreads(1, 0, 0)
            .map_err(|error| format!("{context}: configure setup matcher: {error}"))?;
    }
    let open = submit_accepted_mark_trade(&mut env, route, open_q, ANCHOR)
        .map_err(|error| format!("{context}: open low-price matched position: {error}"))?;
    let mut max_cu = open.compute_units;
    let base_slot = prepare_accepted_mark_landing_dt(&mut env, mode, 0, oracle)
        .map_err(|error| format!("{context}: prepare mark regime: {error}"))?;
    let landing_slot = base_slot
        .checked_add(ELAPSED_SLOTS)
        .ok_or_else(|| format!("{context}: landing slot overflow"))?;

    if crank_before_trade {
        for slot in (base_slot + 1)..=landing_slot {
            match mode {
                AcceptedMarkMode::EwmaMark => env.warp_to_slot(slot),
                AcceptedMarkMode::HybridAfterHours => {
                    env.set_clock(slot, 1_000 + i64::try_from(slot).unwrap())
                }
                AcceptedMarkMode::AuthMark | AcceptedMarkMode::HybridFresh => {
                    return Err(format!("{context}: mode is not trade-driven"));
                }
            }
            let observations = vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: u8::from(mode == AcceptedMarkMode::HybridAfterHours),
            }];
            let crank = if mode == AcceptedMarkMode::HybridAfterHours {
                env.crank_with_oracles(
                    4,
                    slot,
                    observations,
                    &[oracle.ok_or_else(|| format!("{context}: missing hybrid oracle"))?],
                )
            } else {
                env.crank(4, slot, observations)
            }
            .map_err(|error| format!("{context}: advance engine clock at slot {slot}: {error}"))?;
            max_cu = max_cu.max(crank.compute_units);
            let group = env.primary_market_state().1;
            let profile = env.primary_profile(0);
            if profile.mark_ewma_e6 != ANCHOR
                || group.assets[0].effective_price != ANCHOR
                || group.assets[0].raw_oracle_target_price != ANCHOR
            {
                return Err(format!(
                    "{context}: clock-only crank unexpectedly moved the low-price mark at slot {slot}: profile={profile:?}, asset={:?}",
                    group.assets[0]
                ));
            }
        }
    } else {
        let prepared = advance_accepted_mark_landing_from_current(&mut env, mode, ELAPSED_SLOTS)
            .map_err(|error| format!("{context}: prepare trade-first landing: {error}"))?;
        if prepared != landing_slot {
            return Err(format!(
                "{context}: prepared landing {prepared}, expected {landing_slot}"
            ));
        }
    }

    configure_accepted_mark_target_quote(&mut env, route, ANCHOR, reported_price)
        .map_err(|error| format!("{context}: configure discovery quote: {error}"))?;
    let before_profile = env.primary_profile(0);
    let before_group = env.primary_market_state().1;
    let before_pair_value = accepted_mark_pair_value_and_insurance(&env);
    let before_supply = env.token_supply_observed();
    let before_foreign = env.market_data(true);
    let mark_dt = landing_slot
        .checked_sub(before_profile.mark_ewma_last_slot)
        .ok_or_else(|| format!("{context}: EWMA movement slot is in the future"))?;
    let accepted = accepted_mark_reference_clamp(ANCHOR, reported_price, CAP_BPS, mark_dt);
    let reduce = submit_accepted_mark_trade(&mut env, route, reduce_q, reported_price)
        .map_err(|error| format!("{context}: submit low-price reduction: {error}"))?;
    max_cu = max_cu.max(reduce.compute_units);
    let moved_group = env.primary_market_state().1;
    let moved_profile = env.primary_profile(0);
    let low = ANCHOR.min(accepted);
    let high = ANCHOR.max(accepted);
    let moved_in_direction = if rises {
        moved_profile.mark_ewma_e6 > ANCHOR
    } else {
        moved_profile.mark_ewma_e6 < ANCHOR
    };
    let move_bps = accepted_mark_reference_move_bps(ANCHOR, moved_profile.mark_ewma_e6);
    let insurance_gain = moved_group
        .insurance
        .checked_sub(before_group.insurance)
        .ok_or_else(|| format!("{context}: movement reduced insurance"))?;
    let trade_notional = reduce_q
        .unsigned_abs()
        .checked_mul(accepted as u128)
        .and_then(|value| value.checked_add(POS_SCALE - 1))
        .ok_or_else(|| format!("{context}: trade notional overflow"))?
        / POS_SCALE;
    let max_side_notional = before_group.assets[0]
        .oi_eff_long_q
        .max(before_group.assets[0].oi_eff_short_q)
        .checked_mul(ANCHOR as u128)
        .and_then(|value| value.checked_add(POS_SCALE - 1))
        .ok_or_else(|| format!("{context}: side notional overflow"))?
        / POS_SCALE;
    let required_fee = max_side_notional
        .max(trade_notional)
        .checked_mul(2)
        .and_then(|value| value.checked_mul(move_bps as u128))
        .and_then(|value| value.checked_add(9_999))
        .ok_or_else(|| format!("{context}: movement fee overflow"))?
        / 10_000;
    if !moved_in_direction
        || !(low..=high).contains(&moved_profile.mark_ewma_e6)
        || moved_profile.mark_ewma_last_slot != landing_slot
        || moved_group.assets[0].raw_oracle_target_price != moved_profile.mark_ewma_e6
        || moved_group.assets[0].oi_eff_long_q != 9 * POS_SCALE
        || moved_group.assets[0].oi_eff_short_q != 9 * POS_SCALE
        || move_bps == 0
        || insurance_gain < required_fee
        || accepted_mark_pair_value_and_insurance(&env) != before_pair_value
        || env.token_supply_observed() != before_supply
        || env.market_data(true) != before_foreign
        || max_cu >= crate::support::v16_svm::TX_CU_LIMIT
    {
        return Err(format!(
            "{context}: low-price movement mismatch: accepted={accepted}, mark_dt={mark_dt}, move_bps={move_bps}, insurance_gain={insurance_gain}, required_fee={required_fee}, profile={moved_profile:?}, group={moved_group:?}, CU={max_cu}"
        ));
    }

    configure_accepted_mark_target_quote(&mut env, route, ANCHOR, reported_price)
        .map_err(|error| format!("{context}: configure exit quote: {error}"))?;
    let close = submit_accepted_mark_trade(&mut env, route, reduce_q * 9, reported_price)
        .map_err(|error| format!("{context}: complete same-slot owner exit: {error}"))?;
    max_cu = max_cu.max(close.compute_units);
    let group = env.primary_market_state().1;
    let profile = env.primary_profile(0);
    if profile.mark_ewma_e6 != moved_profile.mark_ewma_e6
        || profile.mark_ewma_last_slot != moved_profile.mark_ewma_last_slot
        || group.assets[0].raw_oracle_target_price != moved_group.assets[0].raw_oracle_target_price
        || group.assets[0].oi_eff_long_q != 0
        || group.assets[0].oi_eff_short_q != 0
        || accepted_mark_pair_value_and_insurance(&env) != before_pair_value
        || env.token_supply_observed() != before_supply
        || env.market_data(true) != before_foreign
        || max_cu >= crate::support::v16_svm::TX_CU_LIMIT
    {
        return Err(format!(
            "{context}: complete exit changed the mark frame or left exposure: profile={profile:?}, group={group:?}, CU={max_cu}"
        ));
    }
    crate::support::fuzz_model::assert_public_stock_census(&context, &env)?;
    crate::support::fuzz_model::assert_public_encumbrance_census(&context, &env)?;
    Ok(LowPriceCrankOrderOutcome {
        mark_ewma_e6: profile.mark_ewma_e6,
        mark_ewma_last_slot: profile.mark_ewma_last_slot,
        raw_target_price: group.assets[0].raw_oracle_target_price,
        effective_price: group.assets[0].effective_price,
        insurance: group.insurance,
        pair_value_and_insurance: accepted_mark_pair_value_and_insurance(&env),
        max_cu,
    })
}

#[test]
fn v16_program_low_price_ewma_discovery_is_not_pinned_by_clock_first_cranks() {
    let mut world_count = 0usize;
    let mut max_cu = 0u64;
    for mode in [
        AcceptedMarkMode::EwmaMark,
        AcceptedMarkMode::HybridAfterHours,
    ] {
        for route in DiscoveryTradeRoute::ALL {
            for rises in [false, true] {
                let context = format!("{mode:?}/{route:?}/rises={rises}");
                let trade_first = run_low_price_crank_order(mode, route, rises, false)
                    .unwrap_or_else(|error| panic!("{context}/trade-first: {error}"));
                let crank_first = run_low_price_crank_order(mode, route, rises, true)
                    .unwrap_or_else(|error| panic!("{context}/clock-first: {error}"));

                assert_eq!(crank_first.mark_ewma_e6, trade_first.mark_ewma_e6);
                assert_eq!(
                    crank_first.mark_ewma_last_slot,
                    trade_first.mark_ewma_last_slot
                );
                assert_eq!(crank_first.raw_target_price, trade_first.raw_target_price);
                assert_eq!(crank_first.effective_price, trade_first.effective_price);
                assert_eq!(crank_first.insurance, trade_first.insurance);
                assert_eq!(
                    crank_first.pair_value_and_insurance, trade_first.pair_value_and_insurance,
                    "{context}: clock-first cranks erased elapsed paid mark capacity"
                );
                max_cu = max_cu.max(crank_first.max_cu).max(trade_first.max_cu);
                world_count += 2;
            }
        }
    }
    assert_eq!(world_count, 32);
    assert!(max_cu < crate::support::v16_svm::TX_CU_LIMIT);
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingTargetReplacementOutcome {
    owner_payouts: [u64; 2],
    insurance: u128,
    vault: u128,
    vault_tokens: u64,
    token_supply: u128,
    final_mark: u64,
    max_cu: u64,
}

fn run_pending_target_replacement(
    mode: AcceptedMarkMode,
    route: DiscoveryTradeRoute,
    rises: bool,
) -> Result<PendingTargetReplacementOutcome, String> {
    const ANCHOR: u64 = 1_000_000;
    const CAP_BPS: u64 = 50;
    const MAX_DT: u64 = 8;
    const FIRST_DT: u64 = 3;
    const TOTAL_Q: i128 = 6 * POS_SCALE as i128;
    const CHUNK_Q: i128 = POS_SCALE as i128;

    let target = if rises { 1_400_000 } else { 600_000 };
    let reduce_q = if rises { CHUNK_Q } else { -CHUNK_Q };
    let open_q = if rises { -TOTAL_Q } else { TOTAL_Q };
    let context = format!("{mode:?}/{route:?}/rises={rises}/pending-replacement");
    let mut seed = [0x7a; 32];
    seed[0] ^= mode as u8;
    seed[1] ^= route as u8;
    seed[2] ^= u8::from(rises);
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: ANCHOR,
            max_trading_fee_bps: 1_000,
            max_price_move_bps_per_slot: CAP_BPS,
            max_abs_funding_e9_per_slot: 1_000,
            max_accrual_dt_slots: MAX_DT,
            min_funding_lifetime_slots: MAX_DT,
            ..MarketConfig::default()
        },
    );
    let oracle = configure_accepted_mark_mode(&mut env, mode, ANCHOR)
        .map_err(|error| format!("{context}: configure mode: {error}"))?;
    if accepted_mark_route_is_cpi(route) {
        env.set_matcher_spreads(1, 0, 0)
            .map_err(|error| format!("{context}: configure setup matcher: {error}"))?;
    }
    let setup = submit_accepted_mark_trade(&mut env, route, open_q, ANCHOR)
        .map_err(|error| format!("{context}: open matched position: {error}"))?;
    let mut max_cu = setup.compute_units;
    let baseline_supply = env.token_supply_observed();
    let baseline_foreign = env.market_data(true);
    let baseline_pair_value = accepted_mark_pair_value_and_insurance(&env);

    let first_slot = prepare_accepted_mark_landing_dt(&mut env, mode, FIRST_DT, oracle)
        .map_err(|error| format!("{context}: prepare first landing: {error}"))?;
    configure_accepted_mark_target_quote(&mut env, route, ANCHOR, target)
        .map_err(|error| format!("{context}: configure first quote: {error}"))?;
    let first_before_profile = env.primary_profile(0);
    let first_before_group = env.primary_market_state().1;
    let first_mark_dt = first_slot
        .checked_sub(first_before_profile.mark_ewma_last_slot)
        .ok_or_else(|| format!("{context}: first mark slot regressed"))?
        .min(MAX_DT);
    let first_accepted = accepted_mark_reference_clamp(
        first_before_profile.mark_ewma_e6,
        target,
        CAP_BPS,
        first_mark_dt,
    );
    let first = submit_accepted_mark_trade(&mut env, route, reduce_q, target)
        .map_err(|error| format!("{context}: first reduction: {error}"))?;
    max_cu = max_cu.max(first.compute_units);
    let first_profile = env.primary_profile(0);
    let first_group = env.primary_market_state().1;
    let first_required_fee = accepted_mark_required_externality_fee(
        first_before_group.assets[0].oi_eff_long_q,
        first_before_group.assets[0].oi_eff_short_q,
        first_before_group.assets[0].effective_price,
        first_before_profile.mark_ewma_e6,
        CHUNK_Q as u128,
        first_accepted,
        first_profile.mark_ewma_e6,
    )?;
    let first_insurance_gain = first_group
        .insurance
        .checked_sub(first_before_group.insurance)
        .ok_or_else(|| format!("{context}: first movement reduced insurance"))?;
    let first_directional = if rises {
        first_profile.mark_ewma_e6 > ANCHOR
    } else {
        first_profile.mark_ewma_e6 < ANCHOR
    };
    if !first_directional
        || first_profile.mark_ewma_last_slot != first_slot
        || first_profile.funding_mark_e6 != ANCHOR
        || first_profile.funding_mark_pending_e6 != first_profile.mark_ewma_e6
        || first_profile.funding_mark_pending_slot != first_slot
        || first_group.assets[0].effective_price != ANCHOR
        || first_group.assets[0].raw_oracle_target_price != first_profile.mark_ewma_e6
        || first_insurance_gain < first_required_fee
    {
        return Err(format!(
            "{context}: first pending target mismatch: accepted={first_accepted}, required_fee={first_required_fee}, insurance_gain={first_insurance_gain}, profile={first_profile:?}, asset={:?}",
            first_group.assets[0]
        ));
    }

    let second_slot = first_slot
        .checked_add(1)
        .ok_or_else(|| format!("{context}: second landing overflow"))?;
    match mode {
        AcceptedMarkMode::EwmaMark => env.warp_to_slot(second_slot),
        AcceptedMarkMode::HybridAfterHours => env.set_clock(
            second_slot,
            i64::try_from(1_000u64 + second_slot)
                .map_err(|_| format!("{context}: second timestamp overflow"))?,
        ),
        AcceptedMarkMode::AuthMark | AcceptedMarkMode::HybridFresh => {
            return Err(format!("{context}: mode is not trade-driven"));
        }
    }
    configure_accepted_mark_target_quote(&mut env, route, ANCHOR, target)
        .map_err(|error| format!("{context}: configure second quote: {error}"))?;
    let second_before_profile = env.primary_profile(0);
    let second_before_group = env.primary_market_state().1;
    let second_mark_dt = second_slot
        .checked_sub(second_before_profile.mark_ewma_last_slot)
        .ok_or_else(|| format!("{context}: second mark slot regressed"))?
        .min(MAX_DT);
    let second_accepted = accepted_mark_reference_clamp(
        second_before_profile.mark_ewma_e6,
        target,
        CAP_BPS,
        second_mark_dt,
    );
    let second = submit_accepted_mark_trade(&mut env, route, reduce_q, target)
        .map_err(|error| format!("{context}: second reduction over pending target: {error}"))?;
    max_cu = max_cu.max(second.compute_units);
    let second_profile = env.primary_profile(0);
    let second_group = env.primary_market_state().1;
    let second_required_fee = accepted_mark_required_externality_fee(
        second_before_group.assets[0].oi_eff_long_q,
        second_before_group.assets[0].oi_eff_short_q,
        second_before_group.assets[0].effective_price,
        second_before_profile.mark_ewma_e6,
        CHUNK_Q as u128,
        second_accepted,
        second_profile.mark_ewma_e6,
    )?;
    let second_insurance_gain = second_group
        .insurance
        .checked_sub(second_before_group.insurance)
        .ok_or_else(|| format!("{context}: second movement reduced insurance"))?;
    let second_directional = if rises {
        second_profile.mark_ewma_e6 > first_profile.mark_ewma_e6
    } else {
        second_profile.mark_ewma_e6 < first_profile.mark_ewma_e6
    };
    if !second_directional
        || second_profile.mark_ewma_last_slot != second_slot
        || second_profile.funding_mark_e6 != ANCHOR
        || second_profile.funding_mark_pending_e6 != first_profile.mark_ewma_e6
        || second_profile.funding_mark_pending_slot != first_slot
        || second_group.assets[0].effective_price != ANCHOR
        || second_group.assets[0].raw_oracle_target_price != second_profile.mark_ewma_e6
        || second_group.assets[0].oi_eff_long_q != 4 * POS_SCALE
        || second_group.assets[0].oi_eff_short_q != 4 * POS_SCALE
        || second_insurance_gain < second_required_fee
        || accepted_mark_pair_value_and_insurance(&env) != baseline_pair_value
        || env.token_supply_observed() != baseline_supply
        || env.market_data(true) != baseline_foreign
    {
        return Err(format!(
            "{context}: replacement target mismatch: accepted={second_accepted}, required_fee={second_required_fee}, insurance_gain={second_insurance_gain}, first_profile={first_profile:?}, second_profile={second_profile:?}, asset={:?}",
            second_group.assets[0]
        ));
    }
    crate::support::fuzz_model::assert_public_stock_census(
        &format!("{context}/pending-replacement"),
        &env,
    )?;
    crate::support::fuzz_model::assert_public_encumbrance_census(
        &format!("{context}/pending-replacement"),
        &env,
    )?;

    let catch_up = crank_accepted_mark_target(
        &mut env,
        mode,
        MAX_DT,
        oracle,
        &format!("{context}/catch-up"),
    )?;
    max_cu = max_cu.max(catch_up.compute_units);
    let caught_profile = env.primary_profile(0);
    let caught_group = env.primary_market_state().1;
    if caught_profile.funding_mark_e6 != second_profile.mark_ewma_e6
        || caught_profile.funding_mark_pending_e6 != 0
        || caught_profile.funding_mark_pending_slot != 0
        || caught_group.assets[0].effective_price != second_profile.mark_ewma_e6
        || caught_group.assets[0].raw_oracle_target_price != second_profile.mark_ewma_e6
    {
        return Err(format!(
            "{context}: catch-up did not activate checkpoints in order: before={second_profile:?}, after={caught_profile:?}, asset={:?}",
            caught_group.assets[0]
        ));
    }

    configure_accepted_mark_target_quote(
        &mut env,
        route,
        caught_profile.mark_ewma_e6,
        caught_profile.mark_ewma_e6,
    )
    .map_err(|error| format!("{context}: configure flat exit quote: {error}"))?;
    let close = submit_accepted_mark_trade(
        &mut env,
        route,
        reduce_q
            .checked_mul(4)
            .ok_or_else(|| format!("{context}: close quantity overflow"))?,
        caught_profile.mark_ewma_e6,
    )
    .map_err(|error| format!("{context}: close remaining position: {error}"))?;
    max_cu = max_cu.max(close.compute_units);
    let closed_profile = env.primary_profile(0);
    let closed_group = env.primary_market_state().1;
    if closed_profile.mark_ewma_e6 != caught_profile.mark_ewma_e6
        || closed_profile.mark_ewma_last_slot != caught_profile.mark_ewma_last_slot
        || closed_group.assets[0].oi_eff_long_q != 0
        || closed_group.assets[0].oi_eff_short_q != 0
        || accepted_mark_pair_value_and_insurance(&env) != baseline_pair_value
        || env.token_supply_observed() != baseline_supply
        || env.market_data(true) != baseline_foreign
    {
        return Err(format!(
            "{context}: flat exit changed mark economics or left exposure: profile={closed_profile:?}, group={closed_group:?}"
        ));
    }

    for actor in [0usize, 1] {
        let released = env.primary_portfolio(actor).pnl.get().max(0) as u128;
        if released != 0 {
            let conversion = env
                .convert_released_pnl(actor, released)
                .map_err(|error| format!("{context}/actor={actor}: convert PnL: {error}"))?;
            max_cu = max_cu.max(conversion.compute_units);
        }
    }
    let destination_before =
        [0usize, 1].map(|actor| env.token_amount(env.actors[actor].destination_token));
    for actor in [0usize, 1] {
        let portfolio = env.primary_portfolio(actor);
        if portfolio.pnl.get() != 0 || portfolio.capital.get() == 0 {
            return Err(format!(
                "{context}/actor={actor}: nonterminal portfolio before withdrawal: {portfolio:?}"
            ));
        }
        let withdrawal = env
            .withdraw_primary(actor, portfolio.capital.get())
            .map_err(|error| format!("{context}/actor={actor}: withdraw capital: {error}"))?;
        max_cu = max_cu.max(withdrawal.compute_units);
    }
    let owner_payouts = [0usize, 1].map(|actor| {
        env.token_amount(env.actors[actor].destination_token) - destination_before[actor]
    });
    let final_group = env.primary_market_state().1;
    for actor in [0usize, 1] {
        let portfolio = env.primary_portfolio(actor);
        if portfolio.capital.get() != 0 || portfolio.pnl.get() != 0 {
            return Err(format!(
                "{context}/actor={actor}: value remained after withdrawal: {portfolio:?}"
            ));
        }
    }
    crate::support::fuzz_model::assert_public_stock_census(&format!("{context}/withdrawn"), &env)?;
    crate::support::fuzz_model::assert_public_encumbrance_census(
        &format!("{context}/withdrawn"),
        &env,
    )?;
    if env.token_supply_observed() != baseline_supply
        || env.market_data(true) != baseline_foreign
        || max_cu >= crate::support::v16_svm::TX_CU_LIMIT
    {
        return Err(format!(
            "{context}: terminal frame mismatch: CU={max_cu}, supply={}, foreign_equal={}",
            env.token_supply_observed(),
            env.market_data(true) == baseline_foreign
        ));
    }
    Ok(PendingTargetReplacementOutcome {
        owner_payouts,
        insurance: final_group.insurance,
        vault: final_group.vault,
        vault_tokens: env.token_amount(env.vault),
        token_supply: env.token_supply_observed(),
        final_mark: closed_profile.mark_ewma_e6,
        max_cu,
    })
}

#[test]
fn v16_program_pending_trade_mark_replacement_preserves_funding_fee_and_exit() {
    let mut world_count = 0usize;
    let mut max_cu = 0u64;
    for mode in [
        AcceptedMarkMode::EwmaMark,
        AcceptedMarkMode::HybridAfterHours,
    ] {
        for rises in [false, true] {
            let baseline = run_pending_target_replacement(mode, DiscoveryTradeRoute::NoCpi, rises)
                .unwrap_or_else(|error| panic!("{mode:?}/NoCpi/rises={rises}: {error}"));
            max_cu = max_cu.max(baseline.max_cu);
            world_count += 1;
            for route in [
                DiscoveryTradeRoute::BatchNoCpi,
                DiscoveryTradeRoute::Cpi,
                DiscoveryTradeRoute::BatchCpi,
            ] {
                let outcome = run_pending_target_replacement(mode, route, rises)
                    .unwrap_or_else(|error| panic!("{mode:?}/{route:?}/rises={rises}: {error}"));
                assert_eq!(
                    outcome.owner_payouts, baseline.owner_payouts,
                    "{mode:?}/{route:?}/rises={rises}: route changed owner payouts"
                );
                assert_eq!(outcome.insurance, baseline.insurance);
                assert_eq!(outcome.vault, baseline.vault);
                assert_eq!(outcome.vault_tokens, baseline.vault_tokens);
                assert_eq!(outcome.token_supply, baseline.token_supply);
                assert_eq!(outcome.final_mark, baseline.final_mark);
                max_cu = max_cu.max(outcome.max_cu);
                world_count += 1;
            }
        }
    }
    assert_eq!(world_count, 16);
    assert!(max_cu < crate::support::v16_svm::TX_CU_LIMIT);
}

#[test]
fn v16_program_repeated_trade_driven_mark_steps_are_paid_and_exit_live() {
    const STEP_COUNT: usize = 4;
    const CAP_BPS: u64 = 25;
    const MAX_DT: u64 = STEP_COUNT as u64;

    let mut world_count = 0usize;
    let mut movement_count = 0usize;
    let mut catch_up_count = 0usize;
    let mut missing_hint_rejection_count = 0usize;
    let mut terminal_refresh_count = 0usize;
    let mut stale_cpi_rejections = 0usize;
    let mut withdrawal_count = 0usize;
    let mut max_cu = 0u64;

    for mode in [
        AcceptedMarkMode::EwmaMark,
        AcceptedMarkMode::HybridAfterHours,
    ] {
        for rises in [false, true] {
            for route_offset in 0..DiscoveryTradeRoute::ALL.len() {
                let mut seed = [0x6d; 32];
                seed[0] = mode as u8;
                seed[1] = rises as u8;
                seed[2] = route_offset as u8;
                let anchor = 1_000_000u64;
                let target = if rises { 1_400_000 } else { 600_000 };
                let total_size = if rises {
                    (STEP_COUNT as i128) * POS_SCALE as i128
                } else {
                    -((STEP_COUNT as i128) * POS_SCALE as i128)
                };
                let chunk_size = total_size / STEP_COUNT as i128;
                let setup_route =
                    DiscoveryTradeRoute::ALL[(route_offset + DiscoveryTradeRoute::ALL.len() - 1)
                        % DiscoveryTradeRoute::ALL.len()];
                let context =
                    format!("{mode:?}/{rises:?}/offset={route_offset}/setup={setup_route:?}");
                let mut env = V16Svm::new(
                    seed,
                    MarketConfig {
                        initial_price: anchor,
                        max_trading_fee_bps: 100,
                        max_price_move_bps_per_slot: CAP_BPS,
                        max_accrual_dt_slots: MAX_DT,
                        min_funding_lifetime_slots: MAX_DT,
                        ..MarketConfig::default()
                    },
                );
                let oracle = configure_accepted_mark_mode(&mut env, mode, anchor)
                    .unwrap_or_else(|error| panic!("{context}: configure mode: {error}"));
                if accepted_mark_route_is_cpi(setup_route) {
                    env.set_matcher_spreads(1, 0, 0)
                        .unwrap_or_else(|error| panic!("{context}: setup matcher: {error}"));
                }
                let setup = submit_accepted_mark_trade(&mut env, setup_route, -total_size, anchor)
                    .unwrap_or_else(|error| panic!("{context}: setup trade: {error}"));
                max_cu = max_cu.max(setup.compute_units);
                crate::support::fuzz_model::assert_public_stock_census(
                    &format!("{context}/setup"),
                    &env,
                )
                .unwrap_or_else(|error| panic!("repeated mark setup stock: {error}"));
                crate::support::fuzz_model::assert_public_encumbrance_census(
                    &format!("{context}/setup"),
                    &env,
                )
                .unwrap_or_else(|error| panic!("repeated mark setup encumbrance: {error}"));

                let baseline_supply = env.token_supply_observed();
                let baseline_foreign = env.market_data(true);
                let mut previous_route = setup_route;

                for step_index in 0..STEP_COUNT {
                    if step_index != 0 {
                        let catch_up = crank_accepted_mark_target(
                            &mut env,
                            mode,
                            MAX_DT,
                            oracle,
                            &format!("{context}/before-step={step_index}"),
                        )
                        .unwrap_or_else(|error| panic!("repeated mark catch-up: {error}"));
                        max_cu = max_cu.max(catch_up.compute_units);
                        catch_up_count += 1;
                    }
                    let dt = (step_index + 1) as u64;
                    let landing_slot = if step_index == 0 {
                        prepare_accepted_mark_landing_dt(&mut env, mode, dt, oracle)
                    } else {
                        advance_accepted_mark_landing_from_current(&mut env, mode, dt)
                    }
                    .unwrap_or_else(|error| {
                        panic!("{context}/step={step_index}: advance landing: {error}")
                    });
                    let route = DiscoveryTradeRoute::ALL
                        [(route_offset + step_index) % DiscoveryTradeRoute::ALL.len()];
                    let before_profile = env.primary_profile(0);
                    let before_group = env.primary_market_state().1;
                    let before_insurance = before_group.insurance;
                    let before_asset = before_group.assets[0];
                    assert_eq!(
                        before_asset.raw_oracle_target_price, before_asset.effective_price,
                        "{context}/step={step_index}: prior target was not caught up"
                    );
                    let asset_dt = landing_slot
                        .checked_sub(before_asset.slot_last)
                        .unwrap_or_else(|| {
                            panic!("{context}/step={step_index}: landing preceded asset state")
                        });
                    assert_eq!(asset_dt, dt, "{context}/step={step_index}: exact dt");
                    configure_accepted_mark_target_quote(
                        &mut env,
                        route,
                        before_asset.effective_price,
                        target,
                    )
                    .unwrap_or_else(|error| {
                        panic!("{context}/step={step_index}: configure quote: {error}")
                    });
                    let submission = submit_accepted_mark_trade_after_route(
                        &mut env,
                        previous_route,
                        route,
                        chunk_size,
                        target,
                        &format!("{context}/step={step_index}"),
                    )
                    .unwrap_or_else(|error| panic!("repeated accepted-mark step: {error}"));
                    max_cu = max_cu.max(submission.trade.compute_units).max(
                        submission
                            .refresh
                            .as_ref()
                            .map_or(0, |success| success.compute_units),
                    );
                    stale_cpi_rejections += usize::from(submission.stale_cpi_rejected);

                    let after_profile = env.primary_profile(0);
                    let after_group = env.primary_market_state().1;
                    let after_asset = after_group.assets[0];
                    assert_eq!(
                        after_asset.slot_last, before_asset.slot_last,
                        "{context}/step={step_index}: a caught-up target created phantom engine accrual"
                    );
                    assert_eq!(
                        after_asset.effective_price, before_profile.mark_ewma_e6,
                        "{context}/step={step_index}: bounded accrual did not catch the prior staged mark"
                    );
                    let accepted = accepted_mark_reference_clamp(
                        before_asset.effective_price,
                        target,
                        CAP_BPS,
                        dt,
                    );
                    let low = before_profile.mark_ewma_e6.min(accepted);
                    let high = before_profile.mark_ewma_e6.max(accepted);
                    assert!(
                        (low..=high).contains(&after_profile.mark_ewma_e6),
                        "{context}/step={step_index}: mark {} escaped [{low}, {high}]",
                        after_profile.mark_ewma_e6
                    );
                    if rises {
                        assert!(
                            after_profile.mark_ewma_e6 > before_profile.mark_ewma_e6,
                            "{context}/step={step_index}: upward movement was vacuous"
                        );
                    } else {
                        assert!(
                            after_profile.mark_ewma_e6 < before_profile.mark_ewma_e6,
                            "{context}/step={step_index}: downward movement was vacuous"
                        );
                    }
                    assert_eq!(
                        after_profile.mark_ewma_last_slot, landing_slot,
                        "{context}/step={step_index}: movement slot"
                    );
                    assert_eq!(
                        after_asset.raw_oracle_target_price, after_profile.mark_ewma_e6,
                        "{context}/step={step_index}: staged target"
                    );

                    let move_bps = accepted_mark_reference_move_bps(
                        before_profile.mark_ewma_e6,
                        after_profile.mark_ewma_e6,
                    );
                    let insurance_gain = after_group
                        .insurance
                        .checked_sub(before_insurance)
                        .unwrap_or_else(|| {
                            panic!("{context}/step={step_index}: insurance decreased")
                        });
                    let trade_notional = chunk_size
                        .unsigned_abs()
                        .checked_mul(accepted as u128)
                        .and_then(|value| value.checked_add(POS_SCALE - 1))
                        .expect("repeated accepted-mark trade notional")
                        / POS_SCALE;
                    let externality_price = before_asset
                        .effective_price
                        .max(before_profile.mark_ewma_e6);
                    let max_side_notional = before_asset
                        .oi_eff_long_q
                        .max(before_asset.oi_eff_short_q)
                        .checked_mul(externality_price as u128)
                        .and_then(|value| value.checked_add(POS_SCALE - 1))
                        .expect("repeated accepted-mark side notional")
                        / POS_SCALE;
                    let externality_notional = max_side_notional
                        .max(trade_notional)
                        .checked_mul(2)
                        .expect("repeated accepted-mark externality");
                    let required_fee = externality_notional
                        .checked_mul(move_bps as u128)
                        .and_then(|value| value.checked_add(9_999))
                        .expect("repeated accepted-mark required fee")
                        / 10_000;
                    assert!(
                        move_bps != 0 && insurance_gain >= required_fee,
                        "{context}/step={step_index}: {move_bps} bps movement collected {insurance_gain}, required {required_fee}"
                    );

                    let remaining_q = total_size
                        .unsigned_abs()
                        .checked_sub(
                            chunk_size
                                .unsigned_abs()
                                .checked_mul((step_index + 1) as u128)
                                .expect("repeated accepted-mark reduced quantity"),
                        )
                        .expect("repeated accepted-mark remaining quantity");
                    assert_eq!(
                        after_asset.oi_eff_long_q, remaining_q,
                        "{context}/step={step_index}: long OI"
                    );
                    assert_eq!(
                        after_asset.oi_eff_short_q, remaining_q,
                        "{context}/step={step_index}: short OI"
                    );
                    assert_eq!(
                        after_group.vault, before_group.vault,
                        "{context}/step={step_index}: trade changed custody stock"
                    );
                    assert_eq!(
                        env.token_supply_observed(),
                        baseline_supply,
                        "{context}/step={step_index}: token supply"
                    );
                    assert_eq!(
                        env.market_data(true),
                        baseline_foreign,
                        "{context}/step={step_index}: foreign market frame"
                    );
                    crate::support::fuzz_model::assert_public_stock_census(
                        &format!("{context}/step={step_index}"),
                        &env,
                    )
                    .unwrap_or_else(|error| panic!("repeated mark stock census: {error}"));
                    crate::support::fuzz_model::assert_public_encumbrance_census(
                        &format!("{context}/step={step_index}"),
                        &env,
                    )
                    .unwrap_or_else(|error| panic!("repeated mark encumbrance census: {error}"));
                    movement_count += 1;
                    previous_route = route;
                }

                let final_catch_up = crank_accepted_mark_target(
                    &mut env,
                    mode,
                    MAX_DT,
                    oracle,
                    &format!("{context}/final"),
                )
                .unwrap_or_else(|error| panic!("final repeated mark catch-up: {error}"));
                max_cu = max_cu.max(final_catch_up.compute_units);
                catch_up_count += 1;

                let before_missing_hint = accepted_mark_snapshot(&env);
                assert!(
                    env.crank(1, env.current_slot(), Vec::new()).is_err(),
                    "{context}: a flat stale account unexpectedly refreshed without an asset observation"
                );
                assert_eq!(
                    accepted_mark_snapshot(&env),
                    before_missing_hint,
                    "{context}: missing-observation rejection did not roll back exactly"
                );
                missing_hint_rejection_count += 1;

                let terminal_observations = vec![CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: u8::from(mode == AcceptedMarkMode::HybridAfterHours),
                }];
                let terminal_refresh = if mode == AcceptedMarkMode::HybridAfterHours {
                    env.crank_with_oracles(
                        1,
                        env.current_slot(),
                        terminal_observations,
                        &[oracle.expect("hybrid oracle")],
                    )
                } else {
                    env.crank(1, env.current_slot(), terminal_observations)
                }
                .unwrap_or_else(|error| {
                    panic!(
                        "{context}: hinted refresh second owner before terminal conversion: {error}"
                    )
                });
                max_cu = max_cu.max(terminal_refresh.compute_units);
                terminal_refresh_count += 1;
                crate::support::fuzz_model::assert_public_stock_census(
                    &format!("{context}/terminal-refresh"),
                    &env,
                )
                .unwrap_or_else(|error| panic!("repeated mark terminal refresh stock: {error}"));
                crate::support::fuzz_model::assert_public_encumbrance_census(
                    &format!("{context}/terminal-refresh"),
                    &env,
                )
                .unwrap_or_else(|error| {
                    panic!("repeated mark terminal refresh encumbrance: {error}")
                });

                for actor in [0usize, 1] {
                    let released = env.primary_portfolio(actor).pnl.get().max(0) as u128;
                    if released != 0 {
                        let conversion =
                            env.convert_released_pnl(actor, released)
                                .unwrap_or_else(|error| {
                                    panic!("{context}/actor={actor}: convert PnL: {error}")
                                });
                        max_cu = max_cu.max(conversion.compute_units);
                    }
                }
                let destination_before =
                    [0usize, 1].map(|actor| env.token_amount(env.actors[actor].destination_token));
                for actor in [0usize, 1] {
                    let portfolio = env.primary_portfolio(actor);
                    assert_eq!(
                        portfolio.pnl.get(),
                        0,
                        "{context}/actor={actor}: terminal PnL"
                    );
                    let capital = portfolio.capital.get();
                    assert_ne!(capital, 0, "{context}/actor={actor}: exit was vacuous");
                    let withdrawal = env
                        .withdraw_primary(actor, capital)
                        .unwrap_or_else(|error| {
                            panic!("{context}/actor={actor}: withdraw all capital: {error}")
                        });
                    max_cu = max_cu.max(withdrawal.compute_units);
                    let destination_after = env.token_amount(env.actors[actor].destination_token);
                    assert_eq!(
                        u128::from(destination_after - destination_before[actor]),
                        capital,
                        "{context}/actor={actor}: exact owner payout"
                    );
                    assert_eq!(
                        env.primary_portfolio(actor).capital.get(),
                        0,
                        "{context}/actor={actor}: capital did not exit"
                    );
                    crate::support::fuzz_model::assert_public_stock_census(
                        &format!("{context}/actor={actor}/withdrawn"),
                        &env,
                    )
                    .unwrap_or_else(|error| panic!("repeated mark exit stock census: {error}"));
                    crate::support::fuzz_model::assert_public_encumbrance_census(
                        &format!("{context}/actor={actor}/withdrawn"),
                        &env,
                    )
                    .unwrap_or_else(|error| {
                        panic!("repeated mark exit encumbrance census: {error}")
                    });
                    withdrawal_count += 1;
                }
                assert_eq!(
                    env.token_supply_observed(),
                    baseline_supply,
                    "{context}: supply"
                );
                assert_eq!(env.market_data(true), baseline_foreign, "{context}: frame");
                world_count += 1;
            }
        }
    }

    assert_eq!(
        world_count, 16,
        "two modes x two directions x four route cycles"
    );
    assert_eq!(movement_count, 64, "four paid mark movements per world");
    assert_eq!(
        catch_up_count, 64,
        "every staged mark receives one bounded crank"
    );
    assert_eq!(
        missing_hint_rejection_count, 16,
        "every flat stale account rejects an omitted observation with exact rollback"
    );
    assert_eq!(
        terminal_refresh_count, 16,
        "the second owner receives one bounded hinted terminal refresh per world"
    );
    assert_eq!(
        stale_cpi_rejections, 16,
        "each full route cycle crosses one stale no-CPI-to-CPI capability"
    );
    assert_eq!(withdrawal_count, 32, "both owners exit every world");
    assert!(max_cu < crate::support::v16_svm::TX_CU_LIMIT);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_045_interior_mark_envelope.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_generated_interior_mark_envelopes_are_paid_and_exit_live(
        seed in any::<[u8; 32]>(),
        mode_index in 0u8..4,
        route_index in 0u8..4,
        anchor_units in 1u64..=1_000,
        target_move_bps in 1u64..=9_000,
        rises in any::<bool>(),
        cap_bps in 1u64..=10,
        max_dt in 3u64..=10,
    ) {
        let mode = AcceptedMarkMode::ALL[mode_index as usize];
        let route = DiscoveryTradeRoute::ALL[route_index as usize];
        let anchor = anchor_units * 10_000;
        let target_factor_bps = if rises {
            10_000 + target_move_bps
        } else {
            10_000 - target_move_bps
        };
        let target = anchor * target_factor_bps / 10_000;
        let landing_dt = 1 + u64::from(seed[31]) % (max_dt - 1);
        let total_size = if rises {
            POS_SCALE as i128
        } else {
            -(POS_SCALE as i128)
        };
        let max_cu = run_valid_accepted_mark_case(
            mode,
            route,
            AcceptedMarkCase {
                seed,
                anchor,
                target,
                total_size,
                cap_bps,
                max_dt,
                landing_dt,
            },
        )
        .map_err(TestCaseError::fail)?;
        prop_assert!(max_cu < crate::support::v16_svm::TX_CU_LIMIT);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_045_bilateral_mark_fee_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_matcher_route_matrix_rejects_one_sided_mark_subsidy(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_bilateral_mark_fee_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), TradeDrivenMarkMode::ALL.len() * 2);
        let covered: Vec<_> = discoveries
            .iter()
            .map(|discovery| (discovery.mode, discovery.route))
            .collect();
        let expected: Vec<_> = TradeDrivenMarkMode::ALL
            .into_iter()
            .flat_map(|mode| {
                [DiscoveryTradeRoute::Cpi, DiscoveryTradeRoute::BatchCpi]
                    .map(|route| (mode, route))
            })
            .collect();
        eprintln!("independent bilateral mark-fee coverage: {covered:?}");
        prop_assert_eq!(
            covered,
            expected,
            "bilateral mark-fee route corpus changed"
        );
        for discovery in discoveries {
            prop_assert!(!discovery.is_violation(), "{discovery:?}");
            prop_assert!(discovery.certifies_terminal_nonextraction(), "{discovery:?}");
            prop_assert!(discovery.queued_mark >= discovery.setup_mark);
            prop_assert_eq!(discovery.coalition_excess, 0);
            if discovery.queued_mark == discovery.setup_mark {
                prop_assert_eq!(discovery.mark_victim_equity_loss, 0);
            }
            prop_assert!(discovery.extracted_tokens <= discovery.coalition_equity_before);
            prop_assert!(discovery.fee_counterparty_loss > 0);
            prop_assert!(discovery.insurance_gain > 0);
            prop_assert!(discovery.terminal_evidence.users_terminal);
            prop_assert!(matches!(
                discovery.terminal_evidence.terminal_classification,
                PublicTerminalClassification::BoundedExit
            ));
            prop_assert_eq!(
                discovery
                    .terminal_evidence
                    .public_trace
                    .token_delta_for_accounts(&discovery.terminal_evidence.victim_destinations)
                    .map_err(TestCaseError::fail)?,
                i128::try_from(discovery.terminal_evidence.victim_terminal_payout)
                    .map_err(|_| TestCaseError::fail("victim payout exceeds i128"))?
            );
            prop_assert_eq!(
                discovery
                    .terminal_evidence
                    .public_trace
                    .token_delta_for_accounts(
                        &discovery.terminal_evidence.beneficiary_destinations
                    )
                    .map_err(TestCaseError::fail)?,
                i128::try_from(discovery.terminal_evidence.beneficiary_terminal_payout)
                    .map_err(|_| TestCaseError::fail("coalition payout exceeds i128"))?
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_045_trade_driven_liquidation_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_mark_mode_route_matrix_keeps_liquidation_penalties_nonreclaimable(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_trade_driven_liquidation_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(
            discoveries.len(),
            TradeDrivenMarkMode::ALL.len() * ProspectiveAccrualRoute::ALL.len()
        );
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| (discovery.mode, discovery.route))
            .collect();
        eprintln!("independent trade-driven liquidation violations: {violations:?}");
        prop_assert!(violations.is_empty(), "{violations:?}");
        for discovery in discoveries {
            prop_assert!(discovery.certifies_nonreclaimable_liquidation_penalty());
            prop_assert_eq!(discovery.liquidation_reward, 0);
            prop_assert!(discovery.retained_penalty > 0);
            prop_assert_eq!(discovery.budgeted_penalty, 0);
            prop_assert!(discovery.oi_reduction_q > 0);
            prop_assert_eq!(discovery.coalition_gain, 0);
            prop_assert!(discovery.coalition_loss > 0);
            prop_assert!(discovery.victim_loss > 0);
            prop_assert!(discovery.terminal_evidence.users_terminal);
            prop_assert!(matches!(
                discovery.terminal_evidence.terminal_classification,
                PublicTerminalClassification::BoundedExit
            ));
            prop_assert_eq!(
                discovery
                    .terminal_evidence
                    .public_trace
                    .token_delta_for_accounts(
                        &discovery.terminal_evidence.beneficiary_destinations
                    )
                    .map_err(TestCaseError::fail)?,
                i128::try_from(discovery.extracted_tokens)
                    .map_err(|_| TestCaseError::fail("coalition payout exceeds i128"))?
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_045_mark_movement_reserve_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_trade_route_matrix_keeps_mark_reserve_nonwithdrawable(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_mark_movement_reserve_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), DiscoveryTradeRoute::ALL.len());
        for (expected, discovery) in DiscoveryTradeRoute::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.route, expected);
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.route)
            .collect();
        eprintln!("independent mark-movement reserve violations: {violations:?}");
        prop_assert!(violations.is_empty(), "mark-movement reserve regressed");
        for discovery in discoveries {
            prop_assert!(
                discovery.certifies_nonwithdrawable_reserve(),
                "{discovery:?}"
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_045_mark_admission_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_mark_publication_matrix_rejects_stale_risk_and_recovers(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_pending_mark_admission_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), PendingMarkSource::ALL.len());
        for (expected, discovery) in PendingMarkSource::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.source, expected);
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.source)
            .collect();
        eprintln!("independent pending-mark admission violations: {violations:?}");
        prop_assert!(violations.is_empty(), "pending-mark admission regressed");
        for discovery in discoveries {
            prop_assert!(discovery.certifies_guard_and_liveness(), "{discovery:?}");
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_045_pending_mark_fee_ordering_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_pending_mark_fee_ordering_rejects_and_preserves_terminal_value(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_pending_mark_fee_ordering(seed)
            .map_err(TestCaseError::fail)?;
        eprintln!("pending-mark fee-order verification: {discovery:?}");
        prop_assert_eq!(discovery.control_reward, 0);
        prop_assert_eq!(discovery.reordered_reward, 0);
        prop_assert!(
            discovery.rejects_pending_sync_and_preserves_terminal_value(),
            "pending-mark fee ordering did not reject and preserve value: {:?}",
            discovery
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_045_pending_target_override_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_trade_route_matrix_rejects_pending_target_override(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_pending_target_override_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), DiscoveryTradeRoute::ALL.len());
        for (expected, discovery) in DiscoveryTradeRoute::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.route, expected);
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.route)
            .collect();
        eprintln!("independent pending-target override violations: {violations:?}");
        prop_assert!(violations.is_empty(), "pending-target override regressed");
        for discovery in discoveries {
            prop_assert!(
                discovery.certifies_guard_and_terminal_value(),
                "{discovery:?}"
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_045_pending_mark_inheritance_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_trade_route_matrix_rejects_pending_mark_inheritance(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_pending_mark_inheritance_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), DiscoveryTradeRoute::ALL.len());
        for (expected, discovery) in DiscoveryTradeRoute::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.route, expected);
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.route)
            .collect();
        eprintln!("independent pending-mark inheritance violations: {violations:?}");
        prop_assert!(violations.is_empty(), "pending-mark inheritance regressed");
        for discovery in discoveries {
            prop_assert!(discovery.certifies_guard_and_liveness(), "{discovery:?}");
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/v16_program_stateful_fuzz.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_pr260_pending_ewma_inheritance_guard_fuzz(
        (seed, route) in pending_ewma_inheritance_strategy()
    ) {
        let protection = reproduce_pending_ewma_inheritance(seed, route)
            .map_err(TestCaseError::fail)?;
        prop_assert!(protection.pending_admission_rejected);
        prop_assert!(protection.rejected_exact_rollback);
        prop_assert!(protection.post_commit_trade_landed);
        prop_assert!(protection.post_commit_exit_landed);
        prop_assert_eq!(protection.attacker_gain, 0);
        prop_assert_eq!(protection.victim_loss, 0);
    }

    #[test]
    fn v16_program_pr282_pending_ewma_target_override_guard_fuzz(
        (seed, route) in pending_ewma_target_override_strategy()
    ) {
        let protection = reproduce_pending_ewma_target_override(seed, route)
            .map_err(TestCaseError::fail)?;
        prop_assert!(protection.override_rejected);
        prop_assert!(protection.rejected_exact_rollback);
        prop_assert_eq!(protection.attack_target, protection.control_target);
        prop_assert_eq!(protection.attacker_profit, 0);
        prop_assert_eq!(protection.displaced_victim_pnl, 0);
    }

    #[test]
    fn v16_program_pr264_pr265_pr332_pr333_target_staging_guard_fuzz(
        (seed, case) in target_staging_strategy()
    ) {
        let protection = reproduce_unstaged_mark_target(seed, case)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(protection.engine_target, protection.wrapper_target);
        prop_assert!(protection.engine_epoch_advanced);
        prop_assert!(protection.stale_increase_rejected);
        prop_assert!(protection.rejected_exact_rollback);
        prop_assert!(protection.lagging_risk_reduction_landed);
        prop_assert!(protection.post_commit_trade_landed);
        prop_assert!(protection.post_commit_exit_landed);
        prop_assert_eq!(protection.attacker_profit, 0);
        prop_assert_eq!(protection.victim_capital_loss, 0);
    }

    #[test]
    fn v16_program_pr356_pending_mark_fee_guard_fuzz(
        seed in pending_mark_fee_reward_seed_strategy()
    ) {
        let result = reproduce_pending_mark_fee_reward(seed);
        prop_assert!(
            result.is_ok(),
            "PR 356 fixed route failed for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr369_bilateral_fee_support_fuzz(
        (seed, mode, route) in bilateral_fee_support_strategy()
    ) {
        let reproduction = reproduce_bilateral_fee_support(seed, mode, route)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(reproduction.mode, mode);
        prop_assert_eq!(reproduction.route, route);
        prop_assert_eq!(reproduction.coalition_excess, 0);
        prop_assert!(reproduction.extracted_tokens <= reproduction.coalition_equity_before);
        prop_assert!(reproduction.queued_mark >= reproduction.setup_mark);
        if reproduction.queued_mark == reproduction.setup_mark {
            prop_assert_eq!(reproduction.victim_loss, 0);
        }
        prop_assert!(reproduction.fee_lp_loss > 0);
        prop_assert!(reproduction.insurance_gain > 0);
        prop_assert!(reproduction.max_cu < crate::support::v16_svm::TX_CU_LIMIT);
    }

    #[test]
    fn v16_program_pr225_nonwithdrawable_ewma_fee_fuzz(
        (seed, route) in reclaimable_ewma_fee_strategy()
    ) {
        let protection = reproduce_reclaimable_ewma_fee(seed, route)
            .map_err(TestCaseError::fail)?;
        prop_assert!(protection.pending_withdraw_rejected);
        prop_assert!(protection.rejected_exact_rollback);
        prop_assert!(protection.committed_withdraw_rejected);
        prop_assert!(protection.committed_rejected_exact_rollback);
        prop_assert_eq!(protection.fee_reclaimed, 0);
        prop_assert_eq!(protection.attacker_gain, 0);
        prop_assert!(protection.attacker_loss > 0);
        prop_assert!(protection.victim_loss <= protection.fee_paid);
        prop_assert!(protection.terminal_close_landed);
        prop_assert_eq!(protection.terminal_fee_burned, protection.fee_paid);
        prop_assert!(protection.close_cu < crate::support::v16_svm::TX_CU_LIMIT);
    }

    #[test]
    fn v16_program_pr280_trade_driven_liquidation_penalty_fuzz(
        (seed, mode, route) in trade_driven_liquidation_reward_strategy()
    ) {
        let protection = reproduce_trade_driven_liquidation_reward(seed, mode, route)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(protection.cranker_reward, 0);
        prop_assert!(protection.retained_penalty > 0);
        prop_assert_eq!(protection.budgeted_penalty, 0);
        prop_assert!(protection.victim_capital_loss > 0);
        prop_assert_eq!(protection.attacker_gain, 0);
        prop_assert!(protection.attacker_loss > 0);
        prop_assert!(protection.liquidation_landed);
        prop_assert!(protection.max_crank_cu < crate::support::v16_svm::TX_CU_LIMIT);
    }
}
