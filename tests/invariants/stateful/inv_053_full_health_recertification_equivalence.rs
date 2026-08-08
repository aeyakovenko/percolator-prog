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
//! wrapper's selected-leg ordering.
//!
//! Guarantee boundary: this is bounded generated public-route evidence over four trade routes and
//! both active-leg orders. It does not replace a proof over every reachable portfolio state.

use super::*;

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
