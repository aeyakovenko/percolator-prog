//! INV-046 - trade availability without unsafe mark admission.
//!
//! The caller-priced public routes are exercised at raw price boundaries after
//! reaching `Active`, `DrainOnly`, and `Recovery` through public transitions.
//! A zero price may reject, but it must roll back exactly and leave the same
//! position closeable at price one. `MAX_ORACLE_PRICE` must also permit the
//! strict bilateral reduction. Successful exits must flatten OI without moving
//! authenticated mark state, custody, or aggregate pair equity.
//!
//! CPI routes have separate off-mark single/batch coverage in the invariant's
//! CU owner; a matcher controls their raw execution quote, so caller-price
//! boundary enumeration belongs to the no-CPI routes here.
//! `v16_program_extreme_price_route_lifecycle_matrix_preserves_exit_or_terminal_fallback`
//! composes both boundaries and both strict-reduction/cross-zero request shapes with all four trade
//! routes and all four deployed lifecycle states. Active must admit both shapes and preserve a
//! later complete exit. DrainOnly and Recovery must reject the risk-increasing cross-zero suffix
//! atomically while preserving the strict reduction and withdrawals. Resolved must reject either
//! trade atomically, including matcher state, then pay both owners through the terminal close route.

use crate::support::fuzz_model::{
    assert_public_encumbrance_census, assert_public_stock_census, execute_trade_route, TradeRoute,
};
use crate::support::v16_svm::{MarketConfig, V16Svm, TX_CU_LIMIT};
use percolator::{AssetLifecycleV16, MarketModeV16, POS_SCALE};
use percolator_prog::ix::BatchTradeLeg;
use solana_sdk::pubkey::Pubkey;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExitLifecycle {
    Active,
    DrainOnly,
    Recovery,
}

impl ExitLifecycle {
    const ALL: [Self; 3] = [Self::Active, Self::DrainOnly, Self::Recovery];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CallerPricedRoute {
    Single,
    Batch,
}

impl CallerPricedRoute {
    const ALL: [Self; 2] = [Self::Single, Self::Batch];
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EconomicSnapshot {
    market: Vec<u8>,
    foreign_market: Vec<u8>,
    portfolios: Vec<Vec<u8>>,
    foreign_portfolio: Vec<u8>,
    tokens: Vec<(Pubkey, Vec<u8>)>,
    lamports: Vec<(Pubkey, u64)>,
    matcher_contexts: Vec<Vec<u8>>,
}

fn snapshot(env: &V16Svm) -> EconomicSnapshot {
    EconomicSnapshot {
        market: env.market_data(false),
        foreign_market: env.market_data(true),
        portfolios: env.all_primary_portfolio_data(),
        foreign_portfolio: env.foreign_portfolio_data(),
        tokens: env.all_token_account_data(),
        lamports: env.all_economic_account_lamports(),
        matcher_contexts: env.all_matcher_context_data(),
    }
}

fn pair_equity(env: &V16Svm) -> i128 {
    [0usize, 1]
        .into_iter()
        .map(|actor| {
            let portfolio = env.primary_portfolio(actor);
            portfolio.capital.get() as i128 + portfolio.pnl.get()
        })
        .sum()
}

fn prepare_exit_world(lifecycle: ExitLifecycle) -> Result<(V16Svm, MarketConfig), String> {
    let config = MarketConfig::default();
    let mut env = V16Svm::new([0x46 ^ lifecycle as u8; 32], config);
    env.trade_no_cpi(0, 1, 0, POS_SCALE as i128, config.initial_price, 0)
        .map_err(|error| format!("open matched exit position: {error}"))?;
    match lifecycle {
        ExitLifecycle::Active => {}
        ExitLifecycle::DrainOnly => {
            env.drain_only_asset(0, 0)
                .map_err(|error| format!("enter DrainOnly: {error}"))?;
        }
        ExitLifecycle::Recovery => {
            env.configure_permissionless_resolve(1_000, 100)
                .map_err(|error| format!("configure Recovery: {error}"))?;
            env.shutdown_asset(0, 1)
                .map_err(|error| format!("enter Recovery: {error}"))?;
        }
    }
    let actual = env.primary_market_state().1.assets[0].lifecycle;
    let expected = match lifecycle {
        ExitLifecycle::Active => AssetLifecycleV16::Active,
        ExitLifecycle::DrainOnly => AssetLifecycleV16::DrainOnly,
        ExitLifecycle::Recovery => AssetLifecycleV16::Recovery,
    };
    if actual != expected {
        return Err(format!("public setup missed {lifecycle:?}: got {actual:?}"));
    }
    Ok((env, config))
}

fn submit_exit(
    env: &mut V16Svm,
    route: CallerPricedRoute,
    exec_price: u64,
) -> Result<crate::support::v16_svm::TxSuccess, String> {
    match route {
        CallerPricedRoute::Single => env.trade_no_cpi(0, 1, 0, -(POS_SCALE as i128), exec_price, 0),
        CallerPricedRoute::Batch => {
            let market_id = env.primary_market_state().1.assets[0].market_id;
            env.batch_trade_no_cpi(
                0,
                1,
                vec![BatchTradeLeg {
                    asset_index: 0,
                    market_id,
                    size_q: -(POS_SCALE as i128),
                    exec_price,
                    fee_bps: 0,
                }],
            )
        }
    }
}

fn verify_boundary_exit(
    lifecycle: ExitLifecycle,
    route: CallerPricedRoute,
    exec_price: u64,
    first_try_zero: bool,
) -> Result<(), String> {
    let (mut env, _) = prepare_exit_world(lifecycle)?;
    if first_try_zero {
        let before_zero = snapshot(&env);
        if submit_exit(&mut env, route, 0).is_ok() {
            return Err(format!(
                "{lifecycle:?}/{route:?}: zero price unexpectedly landed"
            ));
        }
        if snapshot(&env) != before_zero {
            return Err(format!(
                "{lifecycle:?}/{route:?}: rejected zero price poisoned the later exit"
            ));
        }
    }

    let before = env.primary_market_state().1;
    let pair_equity_before = pair_equity(&env);
    let vault_before = env.token_amount(env.vault);
    let foreign_before = env.market_data(true);
    submit_exit(&mut env, route, exec_price).map_err(|error| {
        format!("{lifecycle:?}/{route:?}: boundary price {exec_price} blocked exit: {error}")
    })?;
    let after = env.primary_market_state().1;

    if after.assets[0].oi_eff_long_q != 0 || after.assets[0].oi_eff_short_q != 0 {
        return Err(format!(
            "{lifecycle:?}/{route:?}: boundary exit left OI {}/{}",
            after.assets[0].oi_eff_long_q, after.assets[0].oi_eff_short_q
        ));
    }
    if after.assets[0].effective_price != before.assets[0].effective_price
        || after.assets[0].raw_oracle_target_price != before.assets[0].raw_oracle_target_price
    {
        return Err(format!(
            "{lifecycle:?}/{route:?}: caller price changed authenticated mark state"
        ));
    }
    if after.vault != before.vault
        || after.c_tot != before.c_tot
        || after.insurance != before.insurance
        || env.token_amount(env.vault) != vault_before
        || pair_equity(&env) != pair_equity_before
    {
        return Err(format!(
            "{lifecycle:?}/{route:?}: boundary exit changed custody or pair value"
        ));
    }
    if env.market_data(true) != foreign_before {
        return Err(format!(
            "{lifecycle:?}/{route:?}: boundary exit mutated foreign market"
        ));
    }
    Ok(())
}

#[test]
fn v16_program_caller_priced_boundary_exit_lifecycle_matrix() {
    for lifecycle in ExitLifecycle::ALL {
        for route in CallerPricedRoute::ALL {
            verify_boundary_exit(lifecycle, route, 1, true)
                .unwrap_or_else(|error| panic!("price-one fallback: {error}"));
            verify_boundary_exit(lifecycle, route, percolator::MAX_ORACLE_PRICE, false)
                .unwrap_or_else(|error| panic!("maximum-price exit: {error}"));
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WholeExitState {
    Active,
    DrainOnly,
    Recovery,
    Resolved,
}

impl WholeExitState {
    const ALL: [Self; 4] = [
        Self::Active,
        Self::DrainOnly,
        Self::Recovery,
        Self::Resolved,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WholeExitBoundary {
    One,
    Maximum,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WholeExitShape {
    StrictReduction,
    CrossZero,
}

impl WholeExitShape {
    const ALL: [Self; 2] = [Self::StrictReduction, Self::CrossZero];
}

impl WholeExitBoundary {
    const ALL: [Self; 2] = [Self::One, Self::Maximum];

    fn anchor(self) -> u64 {
        match self {
            Self::One => 10_000,
            Self::Maximum => percolator::MAX_ORACLE_PRICE / 2,
        }
    }

    fn target(self) -> u64 {
        match self {
            Self::One => 1,
            Self::Maximum => percolator::MAX_ORACLE_PRICE,
        }
    }

    fn close_size(self) -> i128 {
        match self {
            Self::One => -(POS_SCALE as i128),
            Self::Maximum => 2,
        }
    }

    fn matcher_spreads(self) -> (u64, u64) {
        match self {
            Self::One => (9_999, 0),
            Self::Maximum => (0, 10_000),
        }
    }
}

const WHOLE_EXIT_ROUTES: [TradeRoute; 4] = [
    TradeRoute::NoCpi,
    TradeRoute::Cpi,
    TradeRoute::BatchNoCpi,
    TradeRoute::BatchCpi,
];

fn whole_exit_route_is_cpi(route: TradeRoute) -> bool {
    matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi)
}

fn enter_whole_exit_state(env: &mut V16Svm, state: WholeExitState) -> Result<(), String> {
    match state {
        WholeExitState::Active => {}
        WholeExitState::DrainOnly => {
            env.drain_only_asset(0, 0)
                .map_err(|error| format!("enter DrainOnly: {error}"))?;
        }
        WholeExitState::Recovery => {
            env.configure_permissionless_resolve(1_000, 100)
                .map_err(|error| format!("configure Recovery: {error}"))?;
            env.shutdown_asset(0, env.current_slot())
                .map_err(|error| format!("enter Recovery: {error}"))?;
        }
        WholeExitState::Resolved => {
            env.resolve_market()
                .map_err(|error| format!("enter Resolved: {error}"))?;
        }
    }

    let group = env.primary_market_state().1;
    let reached = match state {
        WholeExitState::Active => {
            group.mode == MarketModeV16::Live
                && group.assets[0].lifecycle == AssetLifecycleV16::Active
        }
        WholeExitState::DrainOnly => {
            group.mode == MarketModeV16::Live
                && group.assets[0].lifecycle == AssetLifecycleV16::DrainOnly
        }
        WholeExitState::Recovery => {
            group.mode == MarketModeV16::Live
                && group.assets[0].lifecycle == AssetLifecycleV16::Recovery
        }
        WholeExitState::Resolved => group.mode == MarketModeV16::Resolved,
    };
    if !reached {
        return Err(format!("public setup did not reach {state:?}"));
    }
    Ok(())
}

fn verify_extreme_price_route_lifecycle(
    state: WholeExitState,
    route: TradeRoute,
    boundary: WholeExitBoundary,
    shape: WholeExitShape,
) -> Result<u64, String> {
    let mut seed = [0xa6; 32];
    seed[0] = state as u8;
    seed[1] = match route {
        TradeRoute::NoCpi => 0,
        TradeRoute::Cpi => 1,
        TradeRoute::BatchNoCpi => 2,
        TradeRoute::BatchCpi => 3,
    };
    seed[2] = boundary as u8;
    seed[3] = shape as u8;
    let anchor = boundary.anchor();
    let target = boundary.target();
    let close_size = boundary.close_size();
    let context = format!("{state:?}/{route:?}/{boundary:?}/{shape:?}");
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: anchor,
            ..MarketConfig::default()
        },
    );
    if whole_exit_route_is_cpi(route) {
        env.set_matcher_spreads(1, 0, 0)
            .map_err(|error| format!("{context}: neutral setup matcher: {error}"))?;
    }
    let setup = execute_trade_route(&mut env, route, 0, 1, 0, -close_size, anchor, 0)
        .map_err(|error| format!("{context}: setup open: {error}"))?;
    let opened = env.primary_market_state().1;
    let expected_oi = close_size.unsigned_abs();
    if opened.assets[0].oi_eff_long_q != expected_oi
        || opened.assets[0].oi_eff_short_q != expected_oi
    {
        return Err(format!("{context}: setup did not create exact matched OI"));
    }
    if whole_exit_route_is_cpi(route) {
        let (bid, ask) = boundary.matcher_spreads();
        env.set_matcher_spreads(1, bid, ask)
            .map_err(|error| format!("{context}: extreme matcher quote: {error}"))?;
        let quoted = anchor
            .checked_mul(if close_size < 0 {
                10_000 - bid
            } else {
                10_000 + ask
            })
            .and_then(|value| value.checked_div(10_000))
            .ok_or_else(|| format!("{context}: quote arithmetic overflow"))?;
        if quoted != target {
            return Err(format!(
                "{context}: configured matcher quote {quoted} != target {target}"
            ));
        }
    }
    enter_whole_exit_state(&mut env, state)?;
    assert_public_stock_census(&format!("{context}/before-exit"), &env)?;
    assert_public_encumbrance_census(&format!("{context}/before-exit"), &env)?;

    let before = env.primary_market_state().1;
    let before_pair_equity = pair_equity(&env);
    let before_vault_tokens = env.token_amount(env.vault);
    let before_supply = env.token_supply_observed();
    let before_foreign = env.market_data(true);
    let destination_before =
        [0usize, 1].map(|actor| env.token_amount(env.actors[actor].destination_token));

    let mut max_cu = setup.compute_units;
    let mut final_close_size = close_size;
    let mut final_close_price = target;

    if shape == WholeExitShape::CrossZero {
        let cross_size = close_size
            .checked_mul(2)
            .ok_or_else(|| format!("{context}: cross-zero size overflow"))?;
        let cross_before = snapshot(&env);
        match state {
            WholeExitState::Active => {
                let cross = execute_trade_route(&mut env, route, 0, 1, 0, cross_size, target, 0)
                    .map_err(|error| format!("{context}: active cross-zero: {error}"))?;
                max_cu = max_cu.max(cross.compute_units);
                let after_cross = env.primary_market_state().1;
                if after_cross.assets[0].oi_eff_long_q != expected_oi
                    || after_cross.assets[0].oi_eff_short_q != expected_oi
                    || after_cross.assets[0].effective_price != before.assets[0].effective_price
                    || after_cross.assets[0].raw_oracle_target_price
                        != before.assets[0].raw_oracle_target_price
                    || after_cross.vault != before.vault
                    || after_cross.c_tot != before.c_tot
                    || after_cross.insurance != before.insurance
                    || env.token_amount(env.vault) != before_vault_tokens
                    || pair_equity(&env) != before_pair_equity
                {
                    return Err(format!(
                        "{context}: active cross-zero changed mark, custody, value, or OI magnitude"
                    ));
                }
                assert_public_stock_census(&format!("{context}/crossed"), &env)?;
                assert_public_encumbrance_census(&format!("{context}/crossed"), &env)?;
                if whole_exit_route_is_cpi(route) {
                    env.set_matcher_spreads(1, 0, 0)
                        .map_err(|error| format!("{context}: restore neutral matcher: {error}"))?;
                }
                final_close_size = -close_size;
                final_close_price = anchor;
            }
            WholeExitState::DrainOnly | WholeExitState::Recovery | WholeExitState::Resolved => {
                if execute_trade_route(&mut env, route, 0, 1, 0, cross_size, target, 0).is_ok() {
                    return Err(format!(
                        "{context}: risk-increasing cross-zero suffix unexpectedly landed"
                    ));
                }
                if snapshot(&env) != cross_before {
                    return Err(format!(
                        "{context}: rejected cross-zero request did not roll back exactly"
                    ));
                }
            }
        }
    }

    if state == WholeExitState::Resolved {
        if shape == WholeExitShape::StrictReduction {
            let rejected_before = snapshot(&env);
            if execute_trade_route(&mut env, route, 0, 1, 0, close_size, target, 0).is_ok() {
                return Err(format!("{context}: trade landed after market resolution"));
            }
            if snapshot(&env) != rejected_before {
                return Err(format!(
                    "{context}: resolved trade rejection did not roll back every tracked account"
                ));
            }
        }

        let expected_payouts = [0usize, 1].map(|actor| -> Result<u128, String> {
            let portfolio = env.primary_portfolio(actor);
            if portfolio.pnl.get() != 0 {
                return Err(format!(
                    "{context}/actor={actor}: setup created nonzero PnL"
                ));
            }
            Ok(portfolio.capital.get())
        });
        let expected_payouts = [expected_payouts[0].clone()?, expected_payouts[1].clone()?];
        let first = env
            .close_resolved_primary(0)
            .map_err(|error| format!("{context}: first terminal close: {error}"))?;
        let second = env
            .close_resolved_primary(1)
            .map_err(|error| format!("{context}: second terminal close: {error}"))?;
        let after = env.primary_market_state().1;
        if after.assets[0].oi_eff_long_q != 0 || after.assets[0].oi_eff_short_q != 0 {
            return Err(format!("{context}: terminal close left effective OI"));
        }
        for actor in [0usize, 1] {
            let paid =
                env.token_amount(env.actors[actor].destination_token) - destination_before[actor];
            if u128::from(paid) != expected_payouts[actor] {
                return Err(format!(
                    "{context}/actor={actor}: terminal payout {paid} != {}",
                    expected_payouts[actor]
                ));
            }
        }
        assert_public_stock_census(&format!("{context}/terminal"), &env)?;
        assert_public_encumbrance_census(&format!("{context}/terminal"), &env)?;
        if env.token_supply_observed() != before_supply || env.market_data(true) != before_foreign {
            return Err(format!(
                "{context}: terminal route escaped its economic frame"
            ));
        }
        return Ok(max_cu.max(first.compute_units).max(second.compute_units));
    }

    let close = execute_trade_route(
        &mut env,
        route,
        0,
        1,
        0,
        final_close_size,
        final_close_price,
        0,
    )
    .map_err(|error| format!("{context}: extreme-price strict close: {error}"))?;
    let after_close = env.primary_market_state().1;
    if after_close.assets[0].oi_eff_long_q != 0
        || after_close.assets[0].oi_eff_short_q != 0
        || after_close.assets[0].effective_price != before.assets[0].effective_price
        || after_close.assets[0].raw_oracle_target_price != before.assets[0].raw_oracle_target_price
        || after_close.vault != before.vault
        || after_close.c_tot != before.c_tot
        || after_close.insurance != before.insurance
        || env.token_amount(env.vault) != before_vault_tokens
        || pair_equity(&env) != before_pair_equity
    {
        return Err(format!(
            "{context}: strict close changed mark, custody, value, or left OI"
        ));
    }
    assert_public_stock_census(&format!("{context}/closed"), &env)?;
    assert_public_encumbrance_census(&format!("{context}/closed"), &env)?;

    max_cu = max_cu.max(close.compute_units);
    for actor in [0usize, 1] {
        let portfolio = env.primary_portfolio(actor);
        if portfolio.pnl.get() != 0 {
            return Err(format!("{context}/actor={actor}: strict close left PnL"));
        }
        let capital = portfolio.capital.get();
        let withdrawal = env
            .withdraw_primary(actor, capital)
            .map_err(|error| format!("{context}/actor={actor}: withdraw: {error}"))?;
        max_cu = max_cu.max(withdrawal.compute_units);
        let paid =
            env.token_amount(env.actors[actor].destination_token) - destination_before[actor];
        if u128::from(paid) != capital {
            return Err(format!(
                "{context}/actor={actor}: owner payout {paid} != {capital}"
            ));
        }
    }
    assert_public_stock_census(&format!("{context}/withdrawn"), &env)?;
    assert_public_encumbrance_census(&format!("{context}/withdrawn"), &env)?;
    if env.token_supply_observed() != before_supply || env.market_data(true) != before_foreign {
        return Err(format!("{context}: live exit escaped its economic frame"));
    }
    Ok(max_cu)
}

#[test]
fn v16_program_extreme_price_route_lifecycle_matrix_preserves_exit_or_terminal_fallback() {
    let mut world_count = 0usize;
    let mut max_cu = 0u64;
    for state in WholeExitState::ALL {
        for route in WHOLE_EXIT_ROUTES {
            for boundary in WholeExitBoundary::ALL {
                for shape in WholeExitShape::ALL {
                    let cu = verify_extreme_price_route_lifecycle(state, route, boundary, shape)
                        .unwrap_or_else(|error| panic!("extreme-price lifecycle cell: {error}"));
                    max_cu = max_cu.max(cu);
                    world_count += 1;
                }
            }
        }
    }
    assert_eq!(
        world_count, 64,
        "four states x four routes x two prices x two request shapes"
    );
    assert!(max_cu < TX_CU_LIMIT, "maximum observed CU: {max_cu}");
}
