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
//! A 16-world source-credit matrix creates a live counterparty-backing lien from each source side
//! through every trade transport, then branches before and after exact-expiry impairment. Nonzero
//! account and market lien ledgers witness the relevant fast recertifier domain; both live-lien
//! creation and impaired-lien strict reduction must equal a subsequent public full refresh.
//! An eight-world pending-obligation matrix creates a real final-leg bankruptcy close through
//! public trades and compares the committed certificate with the pinned engine's full refresh over
//! cloned on-chain bytes. It also proves fresh risk cannot create a later incremental-cert domain.
//! A final 20-world matrix retains nonzero target/effective lag and a real maintenance debit on one
//! leg while every structural delta changes another leg through every public transport; the
//! incremental certificate must still equal full refresh on the exact committed snapshot.
//! The shared stateful runner now applies that same pinned-engine differential after every public
//! transition to every current primary and foreign certificate. The cloned full refresh must leave
//! the portfolio and all market state byte-exact except for the engine's typed touched-asset
//! `loss_stale_active` observation cache; its safety consumers own independent complete scans.
//! INV-088's production-derived 50-class/62-call wrapper-to-engine roster separately assigns every
//! callsite a global-epoch, touched-account, health-independent, or terminal certificate duty.
//!
//! Guarantee boundary: this is bounded generated public-route evidence over four trade routes and
//! both active-leg orders plus every checkpoint reached by the stateful generator. It does not
//! replace a proof over every reachable portfolio state.

use super::*;
use crate::support::v16_svm::{snapshot_engine_full_refresh, MarketConfig, V16Svm};
use percolator::{
    AssetLifecycleV16, BackingBucketStatusV16, HealthCertV16, PortfolioAccountV16Account,
    PortfolioLegV16, SideV16, POS_SCALE,
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

fn crank_assets_to_fixed_point(env: &mut V16Svm, actor: usize, slot: u64, assets: &[u16]) {
    let observations: Vec<CrankObservationHint> = assets
        .iter()
        .map(|asset_index| CrankObservationHint {
            asset_index: *asset_index,
            oracle_accounts: env.primary_profile(*asset_index as usize).oracle_leg_count,
        })
        .collect();
    let mut progressed = false;
    for _ in 0..24 {
        match env.crank(actor, slot, observations.clone()) {
            Ok(_) => progressed = true,
            Err(error) if progressed && error.contains("Custom(22)") => return,
            Err(error) => panic!(
                "actor {actor} assets {assets:?} failed before reaching a public fixed point: {error}"
            ),
        }
    }
    assert!(
        progressed,
        "actor {actor} assets {assets:?} made no public progress"
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

fn snapshot_portfolio_full_refresh(env: &V16Svm, actor: usize) -> HealthCertV16 {
    snapshot_engine_full_refresh(&env.market_data(false), &env.primary_portfolio_data(actor))
        .expect("pinned engine full refresh must accept the committed public state")
        .0
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
            crank_assets_to_fixed_point(&mut env, BANKRUPT_COUNTERPARTY, 6, &[ADL_ASSET]);
            crank_assets_to_fixed_point(&mut env, TARGET, 6, &[ADL_ASSET]);

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

#[test]
fn v16_program_source_lien_fast_certificate_matches_public_full_refresh() {
    const WINNER: usize = 0;
    const COUNTERPARTY: usize = 1;
    const MARKET_CRANKER: usize = 4;
    const WINNING_ASSET: u16 = 0;
    const ADVERSE_ASSET: u16 = 1;
    const INVALIDATION_ASSET: u16 = 2;
    const START_PRICE: u64 = 100;
    const WINNING_SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const ADVERSE_SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const RISK_INCREASE_Q: i128 = 2 * POS_SCALE as i128;

    for route in DiscoveryTradeRoute::ALL {
        for winner_long in [false, true] {
            for impaired_case in [false, true] {
                let direction = if winner_long { 1 } else { -1 };
                let winning_mark = if winner_long { 105 } else { 95 };
                let adverse_mark = if winner_long { 95 } else { 105 };
                let expiry_winning_mark = if winner_long { 106 } else { 94 };
                let source_domain = if winner_long { 1usize } else { 0usize };
                let mut seed = [0x9c; 32];
                seed[0] ^= match route {
                    DiscoveryTradeRoute::NoCpi => 0,
                    DiscoveryTradeRoute::BatchNoCpi => 1,
                    DiscoveryTradeRoute::Cpi => 2,
                    DiscoveryTradeRoute::BatchCpi => 3,
                };
                seed[1] ^= u8::from(winner_long);
                seed[2] ^= u8::from(impaired_case);
                let mut env = V16Svm::new(
                    seed,
                    MarketConfig {
                        initial_price: START_PRICE,
                        h_max: 4,
                        maintenance_margin_bps: 1_000,
                        initial_margin_bps: 1_000,
                        max_price_move_bps_per_slot: 500,
                        max_accrual_dt_slots: 1,
                        max_abs_funding_e9_per_slot: 0,
                        min_funding_lifetime_slots: 1,
                        maintenance_fee_per_slot: 0,
                        actor_deposits: [313, 1_000, 1, 1, 1],
                        actor_token_balances: [313, 1_000, 1, 1, 1],
                        ..MarketConfig::default()
                    },
                );
                env.top_up_backing_bucket(source_domain as u16, 150, 3)
                    .expect("fund the source domain before creating a lien");
                execute_trade_delta_route(
                    &mut env,
                    route,
                    WINNING_ASSET,
                    WINNER,
                    COUNTERPARTY,
                    direction * WINNING_SIZE_Q,
                    START_PRICE,
                )
                .expect("open the source-claim leg");
                execute_trade_delta_route(
                    &mut env,
                    route,
                    ADVERSE_ASSET,
                    WINNER,
                    COUNTERPARTY,
                    direction * ADVERSE_SIZE_Q,
                    START_PRICE,
                )
                .expect("open the margin-consuming leg");

                env.warp_to_slot(2);
                env.push_auth_mark(WINNING_ASSET, 2, winning_mark)
                    .expect("publish the source-claim mark");
                env.push_auth_mark(ADVERSE_ASSET, 2, adverse_mark)
                    .expect("publish the adverse mark");
                for actor in [MARKET_CRANKER, COUNTERPARTY, WINNER] {
                    crank_assets_to_fixed_point(
                        &mut env,
                        actor,
                        2,
                        &[WINNING_ASSET, ADVERSE_ASSET],
                    );
                }
                assert!(
                    env.primary_portfolio(WINNER).pnl.get() > 0,
                    "fixture must retain a positive attributed source claim"
                );

                if matches!(
                    route,
                    DiscoveryTradeRoute::Cpi | DiscoveryTradeRoute::BatchCpi
                ) {
                    env.ensure_primary_matcher_enabled(COUNTERPARTY)
                        .expect("enable matcher before tracing the liened trade");
                }
                if !impaired_case {
                    env.begin_public_trace();
                }
                execute_trade_delta_route(
                    &mut env,
                    route,
                    ADVERSE_ASSET,
                    WINNER,
                    COUNTERPARTY,
                    direction * RISK_INCREASE_Q,
                    adverse_mark,
                )
                .unwrap_or_else(|error| {
                    panic!("{route:?}/winner_long={winner_long} liened trade failed: {error}")
                });

                let (_, liened_market) = env.primary_market_state();
                let liened_account = env.primary_portfolio(WINNER);
                let account_source = liened_account
                    .source_domains
                    .iter()
                    .find(|source| {
                        source.is_occupied() && source.domain.get() as usize == source_domain
                    })
                    .expect("winner must retain the source-domain attribution");
                assert!(account_source.source_claim_liened_num.get() > 0);
                assert!(account_source.source_lien_counterparty_backing_num.get() > 0);
                assert_eq!(
                    account_source.source_lien_counterparty_backing_num.get(),
                    liened_market.source_credit[source_domain].valid_liened_backing_num
                );
                assert_eq!(
                    liened_market.source_credit[source_domain].impaired_liened_backing_num,
                    0
                );
                assert_eq!(
                    liened_market.source_backing_buckets[source_domain].status,
                    BackingBucketStatusV16::Fresh
                );
                let (incremental, refresh_slot) = if impaired_case {
                    env.warp_to_slot(3);
                    env.push_auth_mark(WINNING_ASSET, 3, expiry_winning_mark)
                        .expect("publish the exact-expiry source mark");
                    env.push_auth_mark(ADVERSE_ASSET, 3, adverse_mark)
                        .expect("publish the exact-expiry adverse mark");
                    crank_assets_to_fixed_point(
                        &mut env,
                        WINNER,
                        3,
                        &[WINNING_ASSET, ADVERSE_ASSET],
                    );
                    let (_, impaired_market) = env.primary_market_state();
                    assert_eq!(
                        impaired_market.source_backing_buckets[source_domain].status,
                        BackingBucketStatusV16::Impaired
                    );
                    assert_eq!(
                        impaired_market.source_credit[source_domain].valid_liened_backing_num,
                        0
                    );
                    assert!(
                        impaired_market.source_credit[source_domain].impaired_liened_backing_num
                            > 0
                    );

                    if matches!(
                        route,
                        DiscoveryTradeRoute::Cpi | DiscoveryTradeRoute::BatchCpi
                    ) {
                        env.ensure_primary_matcher_enabled(COUNTERPARTY)
                            .expect("enable matcher before tracing the impaired-lien reduction");
                    }
                    env.begin_public_trace();
                    let reduction = execute_trade_delta_route(
                        &mut env,
                        route,
                        ADVERSE_ASSET,
                        WINNER,
                        COUNTERPARTY,
                        -direction * RISK_INCREASE_Q,
                        adverse_mark,
                    );
                    reduction.unwrap_or_else(|error| {
                        panic!(
                            "{route:?}/winner_long={winner_long} impaired-lien reduction failed: {error}"
                        )
                    });
                    let (_, after_reduction_market) = env.primary_market_state();
                    assert!(
                        after_reduction_market.source_credit[source_domain]
                            .impaired_liened_backing_num
                            > 0,
                        "strict reduction must retain the still-attributed impaired lien"
                    );
                    (
                        env.primary_portfolio(WINNER)
                            .health_cert
                            .try_to_runtime()
                            .expect(
                                "impaired-lien reduction must leave an incremental certificate",
                            ),
                        3,
                    )
                } else {
                    (
                        liened_account
                            .health_cert
                            .try_to_runtime()
                            .expect("source-lien creation must leave a valid fast certificate"),
                        2,
                    )
                };

                let full = invalidate_and_publicly_full_refresh(
                    &mut env,
                    WINNER,
                    INVALIDATION_ASSET,
                    refresh_slot,
                );
                let full_lanes = health_lanes(full);
                let incremental_lanes = health_lanes(incremental);
                assert_eq!(
                    full_lanes,
                    incremental_lanes,
                    "{route:?}/winner_long={winner_long}/impaired={impaired_case} source-lien fast certificate diverged from full recomputation"
                );
                let trace = env.finish_public_trace();
                trace.validate_public_execution().expect(
                    "source-lien certificate differential must use public exact transitions",
                );
                assert_eq!(trace.out_of_band_economic_mutations, 0);
                assert_eq!(trace.steps.len(), 3);
                assert!(trace.steps.iter().all(|step| step.succeeded));
            }
        }
    }
}

#[test]
fn v16_program_pending_obligation_certificates_match_snapshot_full_refresh() {
    const WINNER: usize = 0;
    const LOSER: usize = 1;
    const FRESH_RISK_COUNTERPARTY: usize = 2;
    const MARKET_CRANKER: usize = 4;
    const BANKRUPTCY_ASSET: u16 = 0;
    const FRESH_RISK_ASSET: u16 = 1;
    const OPEN_PRICE: u64 = 100;
    const BANKRUPTCY_Q: i128 = 10 * POS_SCALE as i128;

    for route in DiscoveryTradeRoute::ALL {
        for winner_long in [false, true] {
            let direction = if winner_long { 1 } else { -1 };
            let bankruptcy_price = if winner_long { 150 } else { 50 };
            let mut seed = [0x6d; 32];
            seed[0] ^= match route {
                DiscoveryTradeRoute::NoCpi => 0,
                DiscoveryTradeRoute::BatchNoCpi => 1,
                DiscoveryTradeRoute::Cpi => 2,
                DiscoveryTradeRoute::BatchCpi => 3,
            };
            seed[1] ^= u8::from(winner_long);
            let mut env = V16Svm::new(
                seed,
                MarketConfig {
                    initial_price: OPEN_PRICE,
                    maintenance_margin_bps: 1_000,
                    initial_margin_bps: 1_000,
                    max_price_move_bps_per_slot: 500,
                    max_accrual_dt_slots: 1,
                    max_abs_funding_e9_per_slot: 0,
                    min_funding_lifetime_slots: 1,
                    maintenance_fee_per_slot: 0,
                    actor_deposits: [1_000, 250, 1_000, 1, 1],
                    actor_token_balances: [1_000, 250, 1_000, 1, 1],
                    ..MarketConfig::default()
                },
            );
            execute_trade_delta_route(
                &mut env,
                route,
                BANKRUPTCY_ASSET,
                WINNER,
                LOSER,
                direction * BANKRUPTCY_Q,
                OPEN_PRICE,
            )
            .expect("open the future bankruptcy pair");

            for step in 1..=10u64 {
                let final_slot = 1 + step;
                let move_amount = 5 * step;
                let mark = if winner_long {
                    OPEN_PRICE + move_amount
                } else {
                    OPEN_PRICE - move_amount
                };
                env.warp_to_slot(final_slot);
                env.push_auth_mark(BANKRUPTCY_ASSET, final_slot, mark)
                    .expect("publish bounded bankruptcy mark step");
                crank_assets_to_fixed_point(
                    &mut env,
                    MARKET_CRANKER,
                    final_slot,
                    &[BANKRUPTCY_ASSET],
                );
            }

            if matches!(
                route,
                DiscoveryTradeRoute::Cpi | DiscoveryTradeRoute::BatchCpi
            ) {
                env.ensure_primary_matcher_enabled(LOSER)
                    .expect("enable bankruptcy matcher before tracing");
                env.ensure_primary_matcher_enabled(FRESH_RISK_COUNTERPARTY)
                    .expect("enable rejected-risk matcher before tracing");
            }
            env.begin_public_trace();
            execute_trade_delta_route(
                &mut env,
                route,
                BANKRUPTCY_ASSET,
                WINNER,
                LOSER,
                -direction * BANKRUPTCY_Q,
                bankruptcy_price,
            )
            .unwrap_or_else(|error| {
                panic!("{route:?}/winner_long={winner_long} bankruptcy close failed: {error}")
            });

            let pending_account = env.primary_portfolio(LOSER);
            let pending = pending_account
                .close_progress
                .try_to_runtime()
                .expect("decode public pending close");
            assert!(pending.active && !pending.finalized && pending.residual_remaining > 0);
            assert!(
                pending_account
                    .legs
                    .iter()
                    .filter_map(|leg| leg.try_to_runtime().ok())
                    .all(|leg| !leg.active),
                "terminal residual begins only as the sole final leg is cleared"
            );
            let (_, pending_market) = env.primary_market_state();
            assert_eq!(
                pending_market.assets[BANKRUPTCY_ASSET as usize].pending_obligation_count_long
                    + pending_market.assets[BANKRUPTCY_ASSET as usize]
                        .pending_obligation_count_short,
                1
            );
            let pending_incremental = pending_account
                .health_cert
                .try_to_runtime()
                .expect("pending close must commit a valid certificate");
            assert_eq!(
                health_lanes(pending_incremental),
                health_lanes(snapshot_portfolio_full_refresh(&env, LOSER)),
                "{route:?}/winner_long={winner_long} pending-close certificate diverged from full refresh"
            );

            let market_before_reject = env.market_data(false);
            let portfolios_before_reject = env.all_primary_portfolio_data();
            let tokens_before_reject = env.all_token_account_data();
            let rejected = execute_trade_delta_route(
                &mut env,
                route,
                FRESH_RISK_ASSET,
                LOSER,
                FRESH_RISK_COUNTERPARTY,
                POS_SCALE as i128,
                OPEN_PRICE,
            );
            assert!(
                rejected.is_err(),
                "a pending final-leg residual must not admit a new incremental-certification domain"
            );
            assert_eq!(env.market_data(false), market_before_reject);
            assert_eq!(env.all_primary_portfolio_data(), portfolios_before_reject);
            assert_eq!(env.all_token_account_data(), tokens_before_reject);

            let trace = env.finish_public_trace();
            trace
                .validate_public_execution()
                .expect("pending-obligation certificate trace must be public and exact");
            assert_eq!(trace.out_of_band_economic_mutations, 0);
            assert_eq!(trace.steps.len(), 2);
            assert!(trace.steps[0].succeeded);
            assert!(!trace.steps[1].succeeded);
            assert_eq!(trace.steps[1].rejected_exact_writable_rollback, Some(true));
        }
    }
}

#[test]
fn v16_program_combined_fee_and_lag_certificates_match_snapshot_full_refresh() {
    const TARGET: usize = 0;
    const COUNTERPARTY: usize = 1;
    const LAG_ASSET: u16 = 0;
    const TRADE_ASSET: u16 = 1;
    const START_PRICE: u64 = 100_000;
    const RAW_TARGET_PRICE: u64 = 90_000;
    const LAG_POSITION_Q: i128 = 10 * POS_SCALE as i128;
    const DEPOSIT: u128 = 10_000_000;
    const MAINTENANCE_FEE_PER_SLOT: u128 = 37;

    for route in DiscoveryTradeRoute::ALL {
        for shape in TradeDeltaShape::ALL {
            let mut seed = [0x3e; 32];
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
                    initial_price: START_PRICE,
                    maintenance_margin_bps: 5_000,
                    initial_margin_bps: 10_000,
                    max_price_move_bps_per_slot: 24,
                    max_accrual_dt_slots: 1,
                    max_abs_funding_e9_per_slot: 0,
                    min_funding_lifetime_slots: 1,
                    maintenance_fee_per_slot: MAINTENANCE_FEE_PER_SLOT,
                    actor_deposits: [DEPOSIT, DEPOSIT, 1, 1, 1],
                    actor_token_balances: [DEPOSIT as u64, DEPOSIT as u64, 1, 1, 1],
                    ..MarketConfig::default()
                },
            );
            let (pre_q, delta_q) = shape.pre_and_delta_q();
            execute_trade_delta_route(
                &mut env,
                route,
                LAG_ASSET,
                TARGET,
                COUNTERPARTY,
                LAG_POSITION_Q,
                START_PRICE,
            )
            .expect("open the leg that will retain target/effective lag");
            if pre_q != 0 {
                execute_trade_delta_route(
                    &mut env,
                    route,
                    TRADE_ASSET,
                    TARGET,
                    COUNTERPARTY,
                    pre_q,
                    START_PRICE,
                )
                .expect("install the pre-delta unrelated leg");
            }

            env.warp_to_slot(2);
            crank_assets_to_fixed_point(&mut env, 2, 2, &[LAG_ASSET, TRADE_ASSET]);
            let capital_before_fee = env.primary_portfolio(TARGET).capital.get();
            env.sync_maintenance_fee(TARGET, 2)
                .expect("public fee synchronization must collect the combined penalty");
            let fee_charged = env.primary_portfolio(TARGET);
            assert_eq!(
                capital_before_fee - fee_charged.capital.get(),
                MAINTENANCE_FEE_PER_SLOT,
                "dedicated public synchronization must collect one exact maintenance debit"
            );

            env.push_auth_mark(LAG_ASSET, 2, RAW_TARGET_PRICE)
                .expect("publish the same-slot lagging raw target");
            crank_assets_to_fixed_point(&mut env, TARGET, 2, &[LAG_ASSET]);
            crank_assets_to_fixed_point(&mut env, COUNTERPARTY, 2, &[LAG_ASSET]);
            let (_, lagged_market) = env.primary_market_state();
            let lagged_asset = lagged_market.assets[LAG_ASSET as usize];
            assert_eq!(lagged_asset.raw_oracle_target_price, RAW_TARGET_PRICE);
            assert!(
                lagged_asset.effective_price > RAW_TARGET_PRICE
                    && lagged_asset.effective_price <= START_PRICE,
                "fixture must retain a nonzero authenticated target/effective lag"
            );
            let penalized = env.primary_portfolio(TARGET);
            assert_eq!(penalized.capital.get(), fee_charged.capital.get());
            let penalized_cert = penalized
                .health_cert
                .try_to_runtime()
                .expect("penalty setup must leave a valid certificate");
            let lag_notional_floor = 10u128
                .checked_mul(u128::from(lagged_asset.effective_price))
                .expect("bounded lag notional");
            assert!(
                penalized_cert.certified_worst_case_loss > lag_notional_floor,
                "raw-target lag must contribute a nonzero requirement penalty"
            );

            if matches!(
                route,
                DiscoveryTradeRoute::Cpi | DiscoveryTradeRoute::BatchCpi
            ) {
                env.ensure_primary_matcher_enabled(COUNTERPARTY)
                    .expect("enable matcher before tracing the combined-penalty delta");
            }
            env.begin_public_trace();
            execute_trade_delta_route(
                &mut env,
                route,
                TRADE_ASSET,
                TARGET,
                COUNTERPARTY,
                delta_q,
                START_PRICE,
            )
            .unwrap_or_else(|error| {
                panic!("{route:?}/{shape:?} combined-penalty delta failed: {error}")
            });
            let after = env.primary_portfolio(TARGET);
            assert_eq!(
                after.capital.get(),
                penalized.capital.get(),
                "same-slot zero-fee delta must not hide another maintenance debit"
            );
            assert_eq!(
                leg_for_asset(after, u32::from(TRADE_ASSET)).map_or(0, |leg| leg.basis_pos_q),
                pre_q + delta_q,
                "combined-penalty route did not land the requested structural delta"
            );
            let incremental = after
                .health_cert
                .try_to_runtime()
                .expect("combined-penalty delta must leave a valid certificate");
            assert_eq!(
                health_lanes(incremental),
                health_lanes(snapshot_portfolio_full_refresh(&env, TARGET)),
                "{route:?}/{shape:?} combined fee/lag certificate diverged from full refresh"
            );

            let trace = env.finish_public_trace();
            trace
                .validate_public_execution()
                .expect("combined-penalty certificate trace must be public and exact");
            assert_eq!(trace.out_of_band_economic_mutations, 0);
            assert_eq!(trace.steps.len(), 1);
            assert!(trace.steps[0].succeeded);
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
