//! INV-048 - Matched trade and open-interest coherence.
//!
//! Normative obligation: every matched public trade preserves signed quantity
//! and keeps stored market open interest coherent with the complete set of
//! active portfolio legs.
//!
//! This file is the primary directed CU/SBF owner for that guarantee. It executes
//! the four public trade routes from fresh state, then independently scans the two
//! affected portfolios and compares observed long/short basis with the maintained
//! O(1) market counters. The stateful INV-086 owner extends the oracle beyond fresh
//! state: its independent transition ledger tracks exact effective OI across
//! matched trades, retained trades, liquidation, rebalance, reset cleanup, and
//! forfeit without treating ADL-retained raw basis as effective OI.

use super::*;

#[derive(Clone, Copy, Debug)]
enum MatchedTradeRoute {
    TradeNoCpi,
    BatchTradeNoCpi,
    TradeCpi,
    BatchTradeCpi,
}

impl MatchedTradeRoute {
    const ALL: [Self; 4] = [
        Self::TradeNoCpi,
        Self::BatchTradeNoCpi,
        Self::TradeCpi,
        Self::BatchTradeCpi,
    ];
}

fn observed_oi_for_asset(portfolios: &[PortfolioAccountV16], asset_index: usize) -> (u128, u128) {
    let mut long = 0u128;
    let mut short = 0u128;
    for portfolio in portfolios {
        for leg in portfolio
            .legs
            .iter()
            .filter_map(|leg| leg.try_to_runtime().ok())
        {
            if !leg.active || leg.asset_index as usize != asset_index {
                continue;
            }
            if leg.basis_pos_q > 0 {
                long = long
                    .checked_add(leg.basis_pos_q as u128)
                    .expect("observed long OI overflow");
            } else if leg.basis_pos_q < 0 {
                short = short
                    .checked_add(leg.basis_pos_q.unsigned_abs())
                    .expect("observed short OI overflow");
            }
        }
    }
    (long, short)
}

fn assert_matched_oi_equals_portfolio_scan(
    env: &V16CuEnv,
    asset_index: usize,
    portfolios: &[Pubkey],
    expected_q: u128,
) {
    let states: Vec<_> = portfolios
        .iter()
        .map(|portfolio| env.portfolio_state(*portfolio))
        .collect();
    let (observed_long, observed_short) = observed_oi_for_asset(&states, asset_index);
    let group = env.market_state().1;
    let asset = group.assets[asset_index];
    assert_eq!(
        observed_long, expected_q,
        "portfolio scan long OI must equal expected route size",
    );
    assert_eq!(
        observed_short, expected_q,
        "portfolio scan short OI must equal expected route size",
    );
    assert_eq!(
        asset.oi_eff_long_q, observed_long,
        "stored long OI must equal complete active-leg scan",
    );
    assert_eq!(
        asset.oi_eff_short_q, observed_short,
        "stored short OI must equal complete active-leg scan",
    );
    assert_eq!(
        asset.oi_eff_long_q, asset.oi_eff_short_q,
        "live matched trade OI must remain balanced",
    );
}

fn run_matched_trade_route(route: MatchedTradeRoute) {
    const PRICE: u64 = 100;
    const SIZE_Q: i128 = 3 * POS_SCALE as i128;

    let mut env = V16CuEnv::new();
    env.configure_auth_mark_for_asset_as_admin(0, 1, PRICE);
    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_account = env.create_portfolio(&taker);
    let lp_account = env.create_portfolio(&lp);
    env.deposit(&taker, taker_account, 1_000_000);
    env.deposit(&lp, lp_account, 1_000_000);

    let matcher = if matches!(
        route,
        MatchedTradeRoute::TradeCpi | MatchedTradeRoute::BatchTradeCpi
    ) {
        let matcher_program = Pubkey::new_unique();
        let matcher_bytes =
            std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF");
        env.svm.add_program(matcher_program, &matcher_bytes);
        let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp, lp_account);
        Some((matcher_program, ctx, delegate))
    } else {
        None
    };

    match route {
        MatchedTradeRoute::TradeNoCpi => {
            env.trade_asset_with_cu(0, &taker, taker_account, &lp, lp_account, SIZE_Q, PRICE, 0);
        }
        MatchedTradeRoute::BatchTradeNoCpi => {
            env.send(
                env.batch_trade_no_cpi_ix(
                    taker_account,
                    lp_account,
                    vec![BatchTradeLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q: SIZE_Q,
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
                &[&taker, &lp],
            )
            .expect("BatchTradeNoCpi matched open");
        }
        MatchedTradeRoute::TradeCpi => {
            let (matcher_program, ctx, delegate) = matcher.unwrap();
            env.trade_cpi_with_cu_on_asset(
                &taker,
                taker_account,
                &lp,
                lp_account,
                matcher_program,
                ctx,
                delegate,
                0,
                SIZE_Q,
                0,
            );
        }
        MatchedTradeRoute::BatchTradeCpi => {
            let (matcher_program, ctx, delegate) = matcher.unwrap();
            env.send(
                env.batch_trade_cpi_ix(
                    taker_account,
                    lp_account,
                    vec![BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q: SIZE_Q,
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
                &[&taker],
            )
            .expect("BatchTradeCpi matched open");
        }
    }

    assert_matched_oi_equals_portfolio_scan(&env, 0, &[taker_account, lp_account], SIZE_Q as u128);
}

#[test]
fn v16_program_all_trade_routes_keep_oi_equal_to_active_leg_scan() {
    for route in MatchedTradeRoute::ALL {
        run_matched_trade_route(route);
    }
}
