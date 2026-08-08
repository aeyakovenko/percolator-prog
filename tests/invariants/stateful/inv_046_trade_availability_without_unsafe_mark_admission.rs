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

use crate::support::v16_svm::{MarketConfig, V16Svm};
use percolator::{AssetLifecycleV16, POS_SCALE};
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
}

fn snapshot(env: &V16Svm) -> EconomicSnapshot {
    EconomicSnapshot {
        market: env.market_data(false),
        foreign_market: env.market_data(true),
        portfolios: env.all_primary_portfolio_data(),
        foreign_portfolio: env.foreign_portfolio_data(),
        tokens: env.all_token_account_data(),
        lamports: env.all_economic_account_lamports(),
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
