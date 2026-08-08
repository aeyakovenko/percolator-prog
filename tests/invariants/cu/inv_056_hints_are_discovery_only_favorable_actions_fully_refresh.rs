//! INV-056 - hints are discovery only; favorable actions fully refresh.
//!
//! Normative obligation: a user-favorable route must not trust a caller-provided
//! subset of work or omit an active stale liability. Before authorizing a
//! favorable new position, it must fully discover the bounded active portfolio
//! state or use a proven-equivalent exact certificate.
//!
//! Evidence in this file (I/C plus invariant-specific route assertions):
//! BatchTradeNoCpi and BatchTradeCpi open a new asset-0 leg for an account that
//! already has a stale active asset-1 leg. The only safe success is to discover
//! and refresh the stale asset-1 leg before admitting the new favorable leg.
//! INV-053 owns the single-leg TradeNoCpi/TradeCpi variants; this file covers the
//! previously separate batch route surface.

use super::*;

#[derive(Clone, Copy, Debug)]
enum Inv056BatchRoute {
    NoCpi,
    Cpi,
}

fn run_batch_route_with_stale_related_leg(route: Inv056BatchRoute) {
    const PRICE: u64 = 100;
    const MOVED_PRICE: u64 = 105;
    const STALE_SIZE_Q: i128 = (10 * POS_SCALE) as i128;
    const NEW_SIZE_Q: i128 = POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(1, 0, PRICE);

    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 1_000_000_000);
    env.deposit(&lp_owner, lp, 1_000_000_000);
    env.trade_asset_with_cu(
        1,
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        STALE_SIZE_Q,
        PRICE,
        0,
    );

    let crank_long_owner = Keypair::new();
    let crank_short_owner = Keypair::new();
    let crank_long = env.create_portfolio(&crank_long_owner);
    let crank_short = env.create_portfolio(&crank_short_owner);
    env.deposit(&crank_long_owner, crank_long, 1_000_000_000);
    env.deposit(&crank_short_owner, crank_short, 1_000_000_000);
    env.trade_asset_with_cu(
        1,
        &crank_long_owner,
        crank_long,
        &crank_short_owner,
        crank_short,
        POS_SCALE as i128,
        PRICE,
        0,
    );

    env.svm.warp_to_slot(1);
    env.push_auth_mark_for_asset_as_admin(1, 1, MOVED_PRICE);
    env.crank(
        crank_long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(1),
        },
    );
    let (_, stale_group) = env.market_state();
    let taker_stale = env.portfolio_state(taker);
    let lp_stale = env.portfolio_state(lp);
    assert_eq!(stale_group.assets[1].effective_price, MOVED_PRICE);
    assert!(
        health_cert(&taker_stale).cert_oracle_epoch < stale_group.oracle_epoch,
        "{route:?}: taker certificate must be stale before the batch route"
    );
    assert!(
        health_cert(&lp_stale).cert_oracle_epoch < stale_group.oracle_epoch,
        "{route:?}: LP certificate must be stale before the batch route"
    );
    assert_ne!(
        active_leg_for_asset(&taker_stale, 1).k_snap,
        stale_group.assets[1].k_long,
        "{route:?}: taker stale leg snapshot must differ from current market K"
    );
    assert_ne!(
        active_leg_for_asset(&lp_stale, 1).k_snap,
        stale_group.assets[1].k_short,
        "{route:?}: LP stale leg snapshot must differ from current market K"
    );

    let cu = match route {
        Inv056BatchRoute::NoCpi => env
            .send(
                env.batch_trade_no_cpi_ix(
                    taker,
                    lp,
                    vec![BatchTradeLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q: NEW_SIZE_Q,
                        exec_price: PRICE,
                        fee_bps: 0,
                    }],
                ),
                vec![
                    AccountMeta::new(taker_owner.pubkey(), true),
                    AccountMeta::new(lp_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(taker, false),
                    AccountMeta::new(lp, false),
                ],
                &[&taker_owner, &lp_owner],
            )
            .expect("BatchTradeNoCpi must refresh stale related legs before admitting asset-0"),
        Inv056BatchRoute::Cpi => {
            let matcher_program = Pubkey::new_unique();
            let matcher_bytes =
                std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF");
            env.svm.add_program(matcher_program, &matcher_bytes);
            let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp_owner, lp);
            env.send(
                env.batch_trade_cpi_ix(
                    taker,
                    lp,
                    vec![BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q: NEW_SIZE_Q,
                        fee_bps: 0,
                        limit_price: 0,
                    }],
                ),
                vec![
                    AccountMeta::new(taker_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(taker, false),
                    AccountMeta::new(lp, false),
                    AccountMeta::new_readonly(matcher_program, false),
                    AccountMeta::new(ctx, false),
                    AccountMeta::new_readonly(delegate, false),
                ],
                &[&taker_owner],
            )
            .expect("BatchTradeCpi must refresh stale related legs before admitting asset-0")
        }
    };
    assert_cu_within(
        &format!("INV-056 {route:?} stale related-leg batch refresh"),
        cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    let (_, group_after) = env.market_state();
    let taker_after = env.portfolio_state(taker);
    let lp_after = env.portfolio_state(lp);
    assert_eq!(
        health_cert(&taker_after).cert_oracle_epoch,
        group_after.oracle_epoch,
        "{route:?}: taker is recertified against the full market epoch"
    );
    assert_eq!(
        health_cert(&lp_after).cert_oracle_epoch,
        group_after.oracle_epoch,
        "{route:?}: LP is recertified against the full market epoch"
    );
    assert_eq!(
        active_leg_for_asset(&taker_after, 1).k_snap,
        group_after.assets[1].k_long,
        "{route:?}: taker stale asset-1 leg was refreshed in-place"
    );
    assert_eq!(
        active_leg_for_asset(&lp_after, 1).k_snap,
        group_after.assets[1].k_short,
        "{route:?}: LP stale asset-1 leg was refreshed in-place"
    );
    assert!(has_active_leg_for_asset(&taker_after, 0));
    assert!(has_active_leg_for_asset(&lp_after, 0));
    assert!(has_active_leg_for_asset(&taker_after, 1));
    assert!(has_active_leg_for_asset(&lp_after, 1));
}

#[test]
fn v16_program_batch_routes_refresh_stale_related_legs_before_favorable_trade() {
    run_batch_route_with_stale_related_leg(Inv056BatchRoute::NoCpi);
    run_batch_route_with_stale_related_leg(Inv056BatchRoute::Cpi);
}
