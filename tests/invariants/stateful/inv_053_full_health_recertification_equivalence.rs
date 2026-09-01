//! INV-053 - Full-health recertification equivalence.
//!
//! Normative obligation: Fast or incremental certification is never more favorable than full recomputation.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_active_leg_currentness_route_order_matrix` and its generated companion build two
//! identical multi-leg portfolios through every public trade route and both active-leg orders.
//! The control refreshes every economically relevant leg; the adversarial path omits a pending
//! funding observation. Every route/order pair requires the unsafe liquidation attempt to return
//! `EngineNonProgress` with exact economic rollback, while the fully observed route preserves the
//! same position with zero deficit and no insurance transfer. This matrix is independent of the
//! wrapper's selected-leg ordering. A separate public lifecycle regression puts a current Recovery
//! leg before a live leg with pending authenticated mark work; the wrapper must reject an omitted
//! later-leg observation before whole-account recertification, while the observed retry progresses.
//! Finally, a 20-world differential compares the incremental certificate written by every public
//! trade route across attach, resize, reduce, cross-zero, and clear against a subsequent public full
//! refresh after an unrelated stale leg has been settled and the certificate epochs invalidated.
//! An adjacent eight-world matrix publicly creates a nonunit ADL leg, then proves every admitted
//! unrelated strict-reduction and clear route produces the same certificate as full recomputation.
//! Risk-increasing deltas are intentionally excluded because the loss-stale ADL gate rejects them.
//!
//! Guarantee boundary: this is bounded generated public-route evidence over four trade routes and
//! both active-leg orders. It does not replace a proof over every reachable portfolio state.

use super::*;
use crate::support::v16_svm::{MarketConfig, V16Svm};
use percolator::{
    AssetLifecycleV16, HealthCertV16, PortfolioAccountV16Account, PortfolioLegV16, SideV16,
    POS_SCALE,
};
use percolator_prog::ix::{BatchTradeCpiLeg, BatchTradeLeg, CrankObservationHint};

#[derive(Clone, Copy, Debug)]
enum TradeDeltaShape {
    Attach,
    Resize,
    Reduce,
    CrossZero,
    Clear,
}

impl TradeDeltaShape {
    const ALL: [Self; 5] = [
        Self::Attach,
        Self::Resize,
        Self::Reduce,
        Self::CrossZero,
        Self::Clear,
    ];

    fn pre_and_delta_q(self) -> (i128, i128) {
        let unit = POS_SCALE as i128;
        match self {
            Self::Attach => (0, 3 * unit),
            Self::Resize => (2 * unit, 3 * unit),
            Self::Reduce => (5 * unit, -3 * unit),
            Self::CrossZero => (2 * unit, -5 * unit),
            Self::Clear => (2 * unit, -2 * unit),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HealthLanes {
    equity: i128,
    initial_req: u128,
    maintenance_req: u128,
    liq_deficit: u128,
    worst_case_loss: u128,
    active_bitmap: Vec<u64>,
}

fn health_lanes(cert: HealthCertV16) -> HealthLanes {
    assert!(cert.valid);
    HealthLanes {
        equity: cert.certified_equity,
        initial_req: cert.certified_initial_req,
        maintenance_req: cert.certified_maintenance_req,
        liq_deficit: cert.certified_liq_deficit,
        worst_case_loss: cert.certified_worst_case_loss,
        active_bitmap: cert.active_bitmap_at_cert.to_vec(),
    }
}

fn without_health_cert(mut account: PortfolioAccountV16Account) -> PortfolioAccountV16Account {
    account.health_cert = Default::default();
    account
}

fn leg_for_asset(account: PortfolioAccountV16Account, asset_index: u32) -> Option<PortfolioLegV16> {
    account
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .find(|leg| leg.active && leg.asset_index == asset_index)
}

fn execute_trade_delta_route(
    env: &mut V16Svm,
    route: DiscoveryTradeRoute,
    asset_index: u16,
    taker: usize,
    maker: usize,
    size_q: i128,
    price: u64,
) -> Result<(), String> {
    let market_id = env.primary_market_state().1.assets[asset_index as usize].market_id;
    if matches!(
        route,
        DiscoveryTradeRoute::Cpi | DiscoveryTradeRoute::BatchCpi
    ) {
        env.ensure_primary_matcher_enabled(maker)?;
    }
    match route {
        DiscoveryTradeRoute::NoCpi => env
            .trade_no_cpi(taker, maker, asset_index, size_q, price, 0)
            .map(|_| ()),
        DiscoveryTradeRoute::BatchNoCpi => env
            .batch_trade_no_cpi(
                taker,
                maker,
                vec![BatchTradeLeg {
                    asset_index,
                    market_id,
                    size_q,
                    exec_price: price,
                    fee_bps: 0,
                }],
            )
            .map(|_| ()),
        DiscoveryTradeRoute::Cpi => env
            .trade_cpi(taker, maker, asset_index, size_q, 0, 0)
            .map(|_| ()),
        DiscoveryTradeRoute::BatchCpi => env
            .batch_trade_cpi(
                taker,
                maker,
                vec![BatchTradeCpiLeg {
                    asset_index,
                    market_id,
                    size_q,
                    fee_bps: 0,
                    limit_price: 0,
                }],
            )
            .map(|_| ()),
    }
}

fn crank_asset_to_fixed_point(env: &mut V16Svm, actor: usize, slot: u64, asset_index: u16) {
    let observations = vec![CrankObservationHint {
        asset_index,
        oracle_accounts: env.primary_profile(asset_index as usize).oracle_leg_count,
    }];
    let mut progressed = false;
    for _ in 0..24 {
        match env.crank(actor, slot, observations.clone()) {
            Ok(_) => progressed = true,
            Err(error) if progressed && error.contains("Custom(22)") => return,
            Err(error) => panic!(
                "actor {actor} asset {asset_index} failed before reaching a public fixed point: {error}"
            ),
        }
    }
    assert!(
        progressed,
        "actor {actor} asset {asset_index} made no public progress"
    );
}

fn invalidate_and_publicly_full_refresh(
    env: &mut V16Svm,
    actor: usize,
    lifecycle_asset: u16,
    slot: u64,
) -> HealthCertV16 {
    env.drain_only_asset(lifecycle_asset, 0)
        .unwrap_or_else(|error| {
            panic!("drain-only asset {lifecycle_asset} must invalidate certificate epochs: {error}")
        });
    let before = env.primary_portfolio(actor);
    let refreshed = env
        .crank_if_actionable(actor, slot, vec![])
        .unwrap_or_else(|error| panic!("public full refresh failed for actor {actor}: {error}"));
    assert!(
        refreshed.is_some(),
        "lifecycle epoch invalidation must select a public refresh"
    );
    let (_, market) = env.primary_market_state();
    let after = env.primary_portfolio(actor);
    assert_eq!(
        without_health_cert(after),
        without_health_cert(before),
        "full refresh must frame non-certificate account state"
    );
    let cert = after
        .health_cert
        .try_to_runtime()
        .expect("public full refresh must write a valid certificate");
    assert_eq!(cert.cert_oracle_epoch, market.oracle_epoch);
    assert_eq!(cert.cert_funding_epoch, market.funding_epoch);
    assert_eq!(cert.cert_risk_epoch, market.risk_epoch);
    assert_eq!(cert.cert_asset_set_epoch, market.asset_set_epoch);
    cert
}

#[test]
fn v16_program_active_leg_currentness_route_order_matrix() {
    let seed = [0x53; 32];
    for route in DiscoveryTradeRoute::ALL {
        for leg_order in ActiveLegOrder::ALL {
            let discovery = discover_active_leg_currentness_violation(seed, route, leg_order)
                .unwrap_or_else(|error| {
                    panic!("{route:?}/{leg_order:?} currentness world failed: {error}")
                });
            assert!(
                discovery.preserves_full_refresh_equivalence(),
                "{route:?}/{leg_order:?} did not reject omitted active-leg accrual safely: {discovery:?}"
            );
        }
    }
}

#[test]
fn v16_program_stale_refresh_scans_past_recovery_leg_for_pending_mark() {
    const PRICE: u64 = 1_000_000;
    const MOVED_PRICE: u64 = 1_100_000;

    let mut env = V16Svm::new([0x53; 32], MarketConfig::default());
    env.warp_to_slot(1);
    env.configure_auth_mark(false, 0, 1, PRICE)
        .expect("configure first authenticated mark");
    env.configure_auth_mark(false, 1, 1, PRICE)
        .expect("configure later authenticated mark");
    env.trade_no_cpi(0, 1, 0, POS_SCALE as i128, PRICE, 0)
        .expect("open first asset leg");
    env.trade_no_cpi(0, 2, 1, POS_SCALE as i128, PRICE, 0)
        .expect("open later asset leg");
    env.configure_permissionless_resolve(1_000, 100)
        .expect("configure public Recovery policy");

    env.warp_to_slot(2);
    env.shutdown_asset(0, 2)
        .expect("publicly move first asset to Recovery");
    for _ in 0..4 {
        if env.primary_portfolio(0).b_stale_state == 0 {
            break;
        }
        env.crank(0, 2, vec![])
            .expect("settle higher-priority B work on the Recovery leg");
    }
    assert_eq!(
        env.primary_portfolio(0).b_stale_state,
        0,
        "probe must reach stale-refresh selection rather than B settlement"
    );

    env.warp_to_slot(3);
    env.push_auth_mark(1, 3, MOVED_PRICE)
        .expect("install pending later-asset authenticated target");
    let (_, before_market) = env.primary_market_state();
    assert_eq!(
        before_market.assets[0].lifecycle,
        AssetLifecycleV16::Recovery
    );
    assert_eq!(before_market.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(
        env.primary_profile(0).mark_ewma_e6,
        before_market.assets[0].effective_price,
        "the public lifecycle boundary must leave the Recovery asset current"
    );
    assert_ne!(
        env.primary_profile(1).mark_ewma_e6,
        before_market.assets[1].effective_price,
        "later live asset must have dispatchable accrual work"
    );

    let before_market_data = env.market_data(false);
    let before_portfolio_data = env.primary_portfolio_data(0);
    let before_tokens = env.all_token_account_data();
    let before_lamports = env.all_economic_account_lamports();
    env.warp_to_slot(4);
    let omitted = env.crank(0, 4, vec![]);
    assert!(
        omitted.is_err(),
        "an omitted later-live observation must not recertify through the first current Recovery leg: {omitted:?}"
    );
    assert_eq!(env.market_data(false), before_market_data);
    assert_eq!(env.primary_portfolio_data(0), before_portfolio_data);
    assert_eq!(env.all_token_account_data(), before_tokens);
    assert_eq!(env.all_economic_account_lamports(), before_lamports);

    env.begin_public_trace();
    let observed = env.crank(
        0,
        4,
        vec![CrankObservationHint {
            asset_index: 1,
            oracle_accounts: 0,
        }],
    );
    assert!(
        observed.is_ok(),
        "supplying the later-live observation must retain bounded public progress: {observed:?}"
    );
    assert_ne!(env.market_data(false), before_market_data);
    assert_ne!(env.primary_portfolio_data(0), before_portfolio_data);
    assert_eq!(env.all_token_account_data(), before_tokens);
    let trace = env.finish_public_trace();
    trace
        .validate_public_execution()
        .expect("full-health trace must be public and rollback-exact");
    assert_eq!(trace.out_of_band_economic_mutations, 0);
    assert_eq!(trace.steps.len(), 1);
    assert!(trace.steps[0].succeeded);
    assert!(trace.steps[0]
        .token_deltas
        .iter()
        .all(|(_, delta)| *delta == 0));

    let (_, after_market) = env.primary_market_state();
    assert!(
        after_market.assets[1].effective_price > before_market.assets[1].effective_price,
        "later live asset must make bounded mark progress"
    );
}

#[test]
fn v16_program_stale_refresh_scans_all_live_legs_for_pending_mark() {
    const PRICE: u64 = 1_000_000;
    const ADVERSE_PRICE: u64 = 900_000;
    const ORACLE_PROGRESS_PRICE: u64 = 950_000;

    let mut env = V16Svm::new([0x54; 32], MarketConfig::default());
    env.warp_to_slot(1);
    for asset in 0..3 {
        env.configure_auth_mark(false, asset, 1, PRICE)
            .unwrap_or_else(|error| panic!("configure asset {asset} mark: {error}"));
    }
    env.trade_no_cpi(0, 1, 0, POS_SCALE as i128, PRICE, 0)
        .expect("open current first leg");
    env.trade_no_cpi(0, 2, 1, POS_SCALE as i128, PRICE, 0)
        .expect("open later live leg");
    env.trade_no_cpi(3, 4, 2, POS_SCALE as i128, PRICE, 0)
        .expect("open independent oracle-progress leg");

    env.warp_to_slot(2);
    env.push_auth_mark(2, 2, ORACLE_PROGRESS_PRICE)
        .expect("publish independent authenticated mark");
    env.crank(
        3,
        2,
        vec![CrankObservationHint {
            asset_index: 2,
            oracle_accounts: 0,
        }],
    )
    .expect("commit independent oracle progress");
    let (_, progressed) = env.primary_market_state();
    let target = env.primary_portfolio(0);
    assert!(
        target.health_cert.cert_oracle_epoch.get() < progressed.oracle_epoch,
        "independent public oracle progress must stale the target certificate"
    );
    assert_eq!(target.b_stale_state, 0);

    env.warp_to_slot(3);
    env.push_auth_mark(1, 3, ADVERSE_PRICE)
        .expect("publish pending adverse mark on later live leg");
    let (_, before_market) = env.primary_market_state();
    assert_eq!(
        env.primary_profile(0).mark_ewma_e6,
        before_market.assets[0].effective_price,
        "first live leg must have no pending wrapper-side accrual"
    );
    assert_ne!(
        env.primary_profile(1).mark_ewma_e6,
        before_market.assets[1].effective_price,
        "later live leg must have a pending authenticated mark"
    );

    let before_market_data = env.market_data(false);
    let before_portfolio_data = env.primary_portfolio_data(0);
    let before_tokens = env.all_token_account_data();
    let before_lamports = env.all_economic_account_lamports();
    env.warp_to_slot(4);
    let omitted = env.crank(0, 4, vec![]);
    assert!(
        omitted.is_err(),
        "an empty crank must not certify a stale multi-leg account while a later authenticated mark is pending: {omitted:?}"
    );
    assert_eq!(env.market_data(false), before_market_data);
    assert_eq!(env.primary_portfolio_data(0), before_portfolio_data);
    assert_eq!(env.all_token_account_data(), before_tokens);
    assert_eq!(env.all_economic_account_lamports(), before_lamports);

    let observed = env.crank(
        0,
        4,
        vec![CrankObservationHint {
            asset_index: 1,
            oracle_accounts: 0,
        }],
    );
    assert!(
        observed.is_ok(),
        "supplying the pending later-leg observation must keep refresh live: {observed:?}"
    );
    let (_, after_market) = env.primary_market_state();
    assert!(
        after_market.assets[1].effective_price < before_market.assets[1].effective_price,
        "the authenticated adverse mark must make bounded engine-price progress"
    );
    let after = env.primary_portfolio(0);
    assert!(after.health_cert.valid != 0);
    assert_eq!(
        after.health_cert.cert_oracle_epoch.get(),
        after_market.oracle_epoch,
        "the observed retry may certify only after the later mark is committed"
    );
}

#[test]
fn v16_program_incremental_trade_certificate_equals_public_full_refresh() {
    const PRICE: u64 = 100;
    const FAVORABLE_MARK: u64 = 105;
    const TARGET: usize = 0;
    const TARGET_COUNTERPARTY: usize = 1;
    const STALE_COUNTERPARTY: usize = 2;

    for route in DiscoveryTradeRoute::ALL {
        for shape in TradeDeltaShape::ALL {
            let mut seed = [0x53; 32];
            seed[0] ^= match route {
                DiscoveryTradeRoute::NoCpi => 0,
                DiscoveryTradeRoute::BatchNoCpi => 1,
                DiscoveryTradeRoute::Cpi => 2,
                DiscoveryTradeRoute::BatchCpi => 3,
            };
            seed[1] ^= match shape {
                TradeDeltaShape::Attach => 0,
                TradeDeltaShape::Resize => 1,
                TradeDeltaShape::Reduce => 2,
                TradeDeltaShape::CrossZero => 3,
                TradeDeltaShape::Clear => 4,
            };
            let mut env = V16Svm::new(
                seed,
                MarketConfig {
                    initial_price: PRICE,
                    max_price_move_bps_per_slot: 500,
                    max_abs_funding_e9_per_slot: 0,
                    maintenance_fee_per_slot: 0,
                    ..MarketConfig::default()
                },
            );
            env.warp_to_slot(1);
            env.configure_auth_mark(false, 1, 1, PRICE)
                .expect("configure unrelated authenticated mark");

            env.trade_no_cpi(
                TARGET,
                STALE_COUNTERPARTY,
                1,
                3 * POS_SCALE as i128,
                PRICE,
                0,
            )
            .expect("open unrelated target leg");
            env.trade_no_cpi(3, 4, 0, POS_SCALE as i128, PRICE, 0)
                .expect("keep target asset OI nonzero across clear");
            env.trade_no_cpi(3, 4, 1, POS_SCALE as i128, PRICE, 0)
                .expect("open independent winning leg for market-only accrual");
            let (pre_q, delta_q) = shape.pre_and_delta_q();
            if pre_q != 0 {
                env.trade_no_cpi(TARGET, TARGET_COUNTERPARTY, 0, pre_q, PRICE, 0)
                    .expect("install target pre-position");
            }

            env.warp_to_slot(2);
            env.push_auth_mark(1, 2, FAVORABLE_MARK)
                .expect("publish unrelated favorable mark");
            env.crank(
                3,
                2,
                vec![CrankObservationHint {
                    asset_index: 1,
                    oracle_accounts: 0,
                }],
            )
            .expect("commit unrelated market mark without refreshing target");
            let (_, stale_market) = env.primary_market_state();
            let stale_target = env.primary_portfolio(TARGET);
            assert!(
                stale_target.health_cert.cert_oracle_epoch.get() < stale_market.oracle_epoch,
                "fixture must enter the trade through a stale unrelated leg"
            );
            let stale_unrelated_leg = leg_for_asset(stale_target, 1)
                .expect("fixture must retain the unrelated target leg");
            let stale_k_target = match stale_unrelated_leg.side {
                SideV16::Long => stale_market.assets[1].k_long,
                SideV16::Short => stale_market.assets[1].k_short,
            };
            assert_ne!(
                stale_unrelated_leg.k_snap, stale_k_target,
                "fixture must exercise incremental settlement of an unrelated stale leg"
            );

            if matches!(
                route,
                DiscoveryTradeRoute::Cpi | DiscoveryTradeRoute::BatchCpi
            ) {
                env.ensure_primary_matcher_enabled(TARGET_COUNTERPARTY)
                    .expect("enable matcher before measuring the public trade route");
            }

            env.begin_public_trace();
            execute_trade_delta_route(
                &mut env,
                route,
                0,
                TARGET,
                TARGET_COUNTERPARTY,
                delta_q,
                PRICE,
            )
            .unwrap_or_else(|error| panic!("{route:?}/{shape:?} target trade failed: {error}"));
            let (_, after_trade_market) = env.primary_market_state();
            let after_trade = env.primary_portfolio(TARGET);
            let final_q = leg_for_asset(after_trade, 0).map_or(0, |leg| leg.basis_pos_q);
            assert_eq!(
                final_q,
                pre_q + delta_q,
                "{route:?}/{shape:?} did not land the intended position delta"
            );
            let refreshed_unrelated_leg =
                leg_for_asset(after_trade, 1).expect("trade must retain the unrelated target leg");
            let refreshed_k_target = match refreshed_unrelated_leg.side {
                SideV16::Long => after_trade_market.assets[1].k_long,
                SideV16::Short => after_trade_market.assets[1].k_short,
            };
            assert_eq!(
                refreshed_unrelated_leg.k_snap, refreshed_k_target,
                "{route:?}/{shape:?} trade must settle the unrelated stale leg"
            );
            let incremental = after_trade
                .health_cert
                .try_to_runtime()
                .expect("trade must write a valid incremental certificate");
            assert_eq!(
                incremental.cert_oracle_epoch,
                after_trade_market.oracle_epoch
            );
            assert_eq!(
                incremental.cert_funding_epoch,
                after_trade_market.funding_epoch
            );
            assert_eq!(incremental.cert_risk_epoch, after_trade_market.risk_epoch);
            assert_eq!(
                incremental.cert_asset_set_epoch,
                after_trade_market.asset_set_epoch
            );

            env.drain_only_asset(0, 0)
                .expect("invalidate certificate epochs without changing health arithmetic");
            let (_, invalidated_market) = env.primary_market_state();
            assert!(invalidated_market.risk_epoch > incremental.cert_risk_epoch);
            assert!(invalidated_market.asset_set_epoch > incremental.cert_asset_set_epoch);
            let before_full_refresh = env.primary_portfolio(TARGET);
            assert_eq!(
                without_health_cert(before_full_refresh),
                without_health_cert(after_trade)
            );

            let refresh = env
                .crank_if_actionable(TARGET, 2, vec![])
                .unwrap_or_else(|error| {
                    panic!("{route:?}/{shape:?} public full refresh failed: {error}")
                });
            assert!(
                refresh.is_some(),
                "{route:?}/{shape:?} risk-epoch invalidation must select refresh"
            );
            let after_full_refresh = env.primary_portfolio(TARGET);
            let full = after_full_refresh
                .health_cert
                .try_to_runtime()
                .expect("public full refresh must write a valid certificate");
            assert_eq!(
                health_lanes(full),
                health_lanes(incremental),
                "{route:?}/{shape:?} incremental certificate diverged from full recomputation"
            );
            assert_eq!(
                without_health_cert(after_full_refresh),
                without_health_cert(before_full_refresh),
                "{route:?}/{shape:?} full refresh changed non-certificate account state"
            );

            let trace = env.finish_public_trace();
            trace
                .validate_public_execution()
                .expect("incremental/full certificate trace must be public and rollback-exact");
            assert_eq!(trace.out_of_band_economic_mutations, 0);
            assert_eq!(trace.steps.len(), 3);
            assert!(trace.steps.iter().all(|step| step.succeeded));
        }
    }
}

#[test]
fn v16_program_incremental_trade_certificate_matches_full_refresh_with_nonunit_adl() {
    const PRICE: u64 = 100;
    const BANKRUPTCY_MARK: u64 = 500;
    const TARGET: usize = 0;
    const BANKRUPT_COUNTERPARTY: usize = 1;
    const TRADE_COUNTERPARTY: usize = 2;
    const ADL_ASSET: u16 = 0;
    const TRADE_ASSET: u16 = 1;

    for route in DiscoveryTradeRoute::ALL {
        for shape in [TradeDeltaShape::Reduce, TradeDeltaShape::Clear] {
            let mut seed = [0xa5; 32];
            seed[0] ^= match route {
                DiscoveryTradeRoute::NoCpi => 0,
                DiscoveryTradeRoute::BatchNoCpi => 1,
                DiscoveryTradeRoute::Cpi => 2,
                DiscoveryTradeRoute::BatchCpi => 3,
            };
            seed[1] ^= match shape {
                TradeDeltaShape::Attach => 0,
                TradeDeltaShape::Resize => 1,
                TradeDeltaShape::Reduce => 2,
                TradeDeltaShape::CrossZero => 3,
                TradeDeltaShape::Clear => 4,
            };
            let mut env = V16Svm::new(
                seed,
                MarketConfig {
                    initial_price: PRICE,
                    maintenance_margin_bps: 10_000,
                    initial_margin_bps: 10_000,
                    max_price_move_bps_per_slot: 10_000,
                    max_accrual_dt_slots: 1,
                    min_funding_lifetime_slots: 1,
                    max_abs_funding_e9_per_slot: 0,
                    maintenance_fee_per_slot: 0,
                    actor_deposits: [1_000_000, 900, 1_000_000, 1_000_000, 1_000_000],
                    ..MarketConfig::default()
                },
            );
            let (pre_q, delta_q) = shape.pre_and_delta_q();
            env.trade_no_cpi(TARGET, TRADE_COUNTERPARTY, TRADE_ASSET, pre_q, PRICE, 0)
                .expect("install target position before the loss-stale ADL episode");
            env.trade_no_cpi(3, 4, TRADE_ASSET, POS_SCALE as i128, PRICE, 0)
                .expect("keep traded-asset OI nonzero across clear");
            env.trade_no_cpi(
                TARGET,
                BANKRUPT_COUNTERPARTY,
                ADL_ASSET,
                2 * POS_SCALE as i128,
                PRICE,
                0,
            )
            .expect("open public ADL pair");
            env.warp_to_slot(6);
            env.push_auth_mark(ADL_ASSET, 6, BANKRUPTCY_MARK)
                .expect("publish bankruptcy mark");
            crank_asset_to_fixed_point(&mut env, BANKRUPT_COUNTERPARTY, 6, ADL_ASSET);
            crank_asset_to_fixed_point(&mut env, TARGET, 6, ADL_ASSET);

            let (_, adl_market) = env.primary_market_state();
            let adl_leg = leg_for_asset(env.primary_portfolio(TARGET), u32::from(ADL_ASSET))
                .expect("winner must retain the publicly ADL-scaled leg");
            assert_eq!(adl_leg.side, SideV16::Long);
            assert!(
                adl_market.assets[ADL_ASSET as usize].a_long < percolator::ADL_ONE,
                "fixture must reach a nonunit long A index"
            );
            let effective_num = adl_leg
                .basis_pos_q
                .unsigned_abs()
                .checked_mul(adl_market.assets[ADL_ASSET as usize].a_long)
                .expect("bounded ADL quantity product");
            let effective_q =
                effective_num / adl_leg.a_basis + u128::from(effective_num % adl_leg.a_basis != 0);
            assert!(
                effective_q > 0 && effective_q < adl_leg.basis_pos_q.unsigned_abs(),
                "fixture must retain raw basis above canonical ADL-effective quantity"
            );

            let baseline = invalidate_and_publicly_full_refresh(&mut env, TARGET, 2, 6);
            assert!(
                baseline.certified_worst_case_loss > 0,
                "nonunit-ADL baseline must retain nonzero risk"
            );
            let adl_leg_before_trade =
                leg_for_asset(env.primary_portfolio(TARGET), u32::from(ADL_ASSET))
                    .expect("ADL leg must survive the baseline refresh");

            if matches!(
                route,
                DiscoveryTradeRoute::Cpi | DiscoveryTradeRoute::BatchCpi
            ) {
                env.ensure_primary_matcher_enabled(TRADE_COUNTERPARTY)
                    .expect("enable matcher before measuring the post-ADL route");
            }
            env.begin_public_trace();
            execute_trade_delta_route(
                &mut env,
                route,
                TRADE_ASSET,
                TARGET,
                TRADE_COUNTERPARTY,
                delta_q,
                PRICE,
            )
            .unwrap_or_else(|error| {
                panic!("{route:?}/{shape:?} post-ADL target trade failed: {error}")
            });
            let after_trade = env.primary_portfolio(TARGET);
            assert_eq!(
                leg_for_asset(after_trade, u32::from(TRADE_ASSET)).map_or(0, |leg| leg.basis_pos_q),
                pre_q + delta_q,
                "{route:?}/{shape:?} did not land the post-ADL position delta"
            );
            assert_eq!(
                leg_for_asset(after_trade, u32::from(ADL_ASSET)),
                Some(adl_leg_before_trade),
                "{route:?}/{shape:?} unrelated trade rewrote the current ADL leg"
            );
            let incremental = after_trade
                .health_cert
                .try_to_runtime()
                .expect("post-ADL trade must write an incremental certificate");
            let full = invalidate_and_publicly_full_refresh(&mut env, TARGET, TRADE_ASSET, 6);
            assert_eq!(
                health_lanes(full),
                health_lanes(incremental),
                "{route:?}/{shape:?} post-ADL incremental certificate diverged from full recomputation"
            );

            let trace = env.finish_public_trace();
            trace
                .validate_public_execution()
                .expect("post-ADL certificate trace must be public and rollback-exact");
            assert_eq!(trace.out_of_band_economic_mutations, 0);
            assert_eq!(trace.steps.len(), 3);
            assert!(trace.steps.iter().all(|step| step.succeeded));
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_053_full_refresh_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_full_refresh_equivalence_rejects_omitted_rescue_liquidation(
        seed in any::<[u8; 32]>(),
        route in prop::sample::select(DiscoveryTradeRoute::ALL.to_vec()),
        leg_order in prop::sample::select(ActiveLegOrder::ALL.to_vec()),
    ) {
        let result = discover_active_leg_currentness_violation(seed, route, leg_order);
        prop_assert!(
            result.is_ok(),
            "full-refresh verification failed for seed {:?}, route {:?}, order {:?}: {}",
            seed,
            route,
            leg_order,
            result.unwrap_err()
        );
        let discovery = result.unwrap();
        prop_assert!(
            discovery.preserves_full_refresh_equivalence(),
            "partial refresh did not reject safely relative to full refresh: {:?}",
            discovery
        );
    }
}
