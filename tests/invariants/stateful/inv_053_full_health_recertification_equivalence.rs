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
//!
//! Guarantee boundary: this is bounded generated public-route evidence over four trade routes and
//! both active-leg orders. It does not replace a proof over every reachable portfolio state.

use super::*;
use crate::support::v16_svm::{MarketConfig, V16Svm};
use percolator::{AssetLifecycleV16, POS_SCALE};
use percolator_prog::ix::CrankObservationHint;

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
