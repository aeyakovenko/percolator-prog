//! INV-049 - Canonical single net leg per asset generation.
//!
//! Normative obligation: each portfolio has at most one active canonical net
//! leg for the current asset generation. Same-asset trades must resize or cross
//! that leg; they must not create hidden duplicate current-generation legs.
//!
//! This file is the primary CU/SBF owner for that guarantee. It repeats
//! same-asset fills through all four public trade routes and checks the exact
//! active-leg census after increase, reduction, and cross-zero transitions.

use super::*;

#[derive(Clone, Copy, Debug)]
enum NetLegRoute {
    TradeNoCpi,
    BatchTradeNoCpi,
    TradeCpi,
    BatchTradeCpi,
}

impl NetLegRoute {
    const ALL: [Self; 4] = [
        Self::TradeNoCpi,
        Self::BatchTradeNoCpi,
        Self::TradeCpi,
        Self::BatchTradeCpi,
    ];
}

fn active_leg_census_for_asset(portfolio: &PortfolioAccountV16, asset_index: usize) -> Vec<i128> {
    portfolio
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .filter(|leg| leg.active && leg.asset_index as usize == asset_index)
        .map(|leg| leg.basis_pos_q)
        .collect()
}

fn assert_single_net_leg(
    env: &V16CuEnv,
    portfolio: Pubkey,
    asset_index: usize,
    expected_basis_q: i128,
) {
    let state = env.portfolio_state(portfolio);
    let legs = active_leg_census_for_asset(&state, asset_index);
    assert_eq!(
        legs,
        vec![expected_basis_q],
        "portfolio must have exactly one active net leg for asset {asset_index}",
    );
    assert_eq!(
        active_leg_for_asset(&state, asset_index).basis_pos_q,
        expected_basis_q,
        "canonical helper must observe the same net leg",
    );
}

fn apply_netting_trade(
    env: &mut V16CuEnv,
    route: NetLegRoute,
    taker: &Keypair,
    taker_account: Pubkey,
    lp: &Keypair,
    lp_account: Pubkey,
    matcher: Option<(Pubkey, Pubkey, Pubkey)>,
    size_q: i128,
) {
    const PRICE: u64 = 100;
    match route {
        NetLegRoute::TradeNoCpi => {
            env.trade_asset_with_cu(0, taker, taker_account, lp, lp_account, size_q, PRICE, 0);
        }
        NetLegRoute::BatchTradeNoCpi => {
            env.send(
                env.batch_trade_no_cpi_ix(
                    taker_account,
                    lp_account,
                    vec![BatchTradeLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q,
                        exec_price: PRICE,
                        fee_bps: 0,
                    }],
                ),
                vec![
                    AccountMeta::new(taker.pubkey(), true),
                    AccountMeta::new(lp.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(taker_account, false),
                    AccountMeta::new(lp_account, false),
                ],
                &[taker, lp],
            )
            .expect("BatchTradeNoCpi same-asset netting trade");
        }
        NetLegRoute::TradeCpi => {
            let (matcher_program, ctx, delegate) = matcher.expect("CPI matcher");
            env.trade_cpi_with_cu_on_asset(
                taker,
                taker_account,
                lp,
                lp_account,
                matcher_program,
                ctx,
                delegate,
                0,
                size_q,
                0,
            );
        }
        NetLegRoute::BatchTradeCpi => {
            let (matcher_program, ctx, delegate) = matcher.expect("CPI matcher");
            env.send(
                env.batch_trade_cpi_ix(
                    taker_account,
                    lp_account,
                    vec![BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q,
                        fee_bps: 0,
                        limit_price: 0,
                    }],
                ),
                vec![
                    AccountMeta::new(taker.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(taker_account, false),
                    AccountMeta::new(lp_account, false),
                    AccountMeta::new_readonly(matcher_program, false),
                    AccountMeta::new(ctx, false),
                    AccountMeta::new_readonly(delegate, false),
                ],
                &[taker],
            )
            .expect("BatchTradeCpi same-asset netting trade");
        }
    }
}

fn run_single_net_leg_route(route: NetLegRoute) {
    const PRICE: u64 = 100;
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_for_asset_as_admin(0, 1, PRICE);
    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_account = env.create_portfolio(&taker);
    let lp_account = env.create_portfolio(&lp);
    env.deposit(&taker, taker_account, 1_000_000);
    env.deposit(&lp, lp_account, 1_000_000);

    let matcher = if matches!(route, NetLegRoute::TradeCpi | NetLegRoute::BatchTradeCpi) {
        let matcher_program = Pubkey::new_unique();
        let matcher_bytes =
            std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF");
        env.svm.add_program(matcher_program, &matcher_bytes);
        let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp, lp_account);
        Some((matcher_program, ctx, delegate))
    } else {
        None
    };

    apply_netting_trade(
        &mut env,
        route,
        &taker,
        taker_account,
        &lp,
        lp_account,
        matcher,
        2 * POS_SCALE as i128,
    );
    assert_single_net_leg(&env, taker_account, 0, 2 * POS_SCALE as i128);
    assert_single_net_leg(&env, lp_account, 0, -(2 * POS_SCALE as i128));

    apply_netting_trade(
        &mut env,
        route,
        &taker,
        taker_account,
        &lp,
        lp_account,
        matcher,
        POS_SCALE as i128,
    );
    assert_single_net_leg(&env, taker_account, 0, 3 * POS_SCALE as i128);
    assert_single_net_leg(&env, lp_account, 0, -(3 * POS_SCALE as i128));

    apply_netting_trade(
        &mut env,
        route,
        &taker,
        taker_account,
        &lp,
        lp_account,
        matcher,
        -(2 * POS_SCALE as i128),
    );
    assert_single_net_leg(&env, taker_account, 0, POS_SCALE as i128);
    assert_single_net_leg(&env, lp_account, 0, -(POS_SCALE as i128));

    apply_netting_trade(
        &mut env,
        route,
        &taker,
        taker_account,
        &lp,
        lp_account,
        matcher,
        -(3 * POS_SCALE as i128),
    );
    assert_single_net_leg(&env, taker_account, 0, -(2 * POS_SCALE as i128));
    assert_single_net_leg(&env, lp_account, 0, 2 * POS_SCALE as i128);

    let group = env.market_state().1;
    assert_eq!(group.assets[0].oi_eff_long_q, 2 * POS_SCALE);
    assert_eq!(group.assets[0].oi_eff_short_q, 2 * POS_SCALE);
}

#[test]
fn v16_program_same_asset_trades_resize_one_canonical_net_leg() {
    for route in NetLegRoute::ALL {
        run_single_net_leg_route(route);
    }
}
