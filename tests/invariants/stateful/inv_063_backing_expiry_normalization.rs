//! INV-063 - Backing-expiry normalization.
//!
//! Normative obligation: Expired backing is normalized before every consumer and cannot remain economically fresh.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_backing_expiry_boundary_rejects_stale_fee_and_preserves_exit` constructs a retained
//! trade while backing is fresh and lands it after authenticated Clock expiry. The unsafe increase
//! must return `EngineStale` with exact rollback, zero provider fee, and no victim loss; a reducing
//! trade must remain executable.
//! `v16_program_expired_backing_trade_route_matrix` repeats the freshness check through all four
//! public trade routes. It rejects newly-created counterparty-backed liens independently of fee
//! routing and separately proves that a risk-reducing trade remains available after expiry.
//! `v16_program_retained_maturity_matrix_discovers_terminal_funded_lock` generates signed expiry
//! boundaries independently of any finding manifest, compares omitted and delayed operations,
//! and requires a finding only when the delayed operation consumes independent principal and
//! leaves funded resolved users unable to progress through owner or permissionless routes.
//! `v16_program_expired_backing_consumer_matrix_rejects_lapsed_conversion` generates released
//! source-backed claims and varies the expiry boundary. Conversion must reject at authenticated
//! expiry with exact rollback and zero provider-principal movement, while the flat claimant can
//! still withdraw all senior capital.
//!
//! Guarantee boundary: the trade and conversion consumers have fixed-pin bounded evidence. The
//! retained-maturity test remains public counterexample discovery for a separate open finding and
//! does not certify that sub-route until its fix is integrated.

use super::*;

fn assert_expired_backing_trade_route(discovery: &ExpiredBackingTradeRouteDiscovery) {
    assert!(
        discovery.rejects_expired_risk_increase_safely(),
        "{:?} did not reject an authenticated-expiry lien with exact rollback: {discovery:?}",
        discovery.route
    );
    assert!(
        discovery.preserves_risk_reduction(),
        "{:?} did not preserve a post-expiry reducing trade: {discovery:?}",
        discovery.route
    );
}

#[test]
fn v16_program_expired_backing_trade_route_matrix() {
    let discoveries = discover_expired_backing_trade_routes([0x63; 32], 2)
        .expect("build every public trade-route expiry world");
    assert_eq!(discoveries.len(), DiscoveryTradeRoute::ALL.len());
    for discovery in &discoveries {
        assert_expired_backing_trade_route(discovery);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_063_backing_expiry_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_backing_expiry_boundary_rejects_stale_fee_and_preserves_exit(
        seed in any::<[u8; 32]>(),
        expiry_offset in prop::sample::select(vec![2u8, 3, 5, 8]),
    ) {
        let case = BackingExpiryCase {
            fee_bps: 5_000,
            expiry_offset,
            mark_move_bps: 500,
            increase_divisor: 20,
        };
        let result = discover_backing_expiry_violation(seed, case);
        prop_assert!(
            result.is_ok(),
            "backing-expiry verification failed for case {:?}: {}",
            case,
            result.unwrap_err()
        );
        let discovery = result.unwrap();
        prop_assert!(
            discovery.preserves_expiry_normalization(),
            "expired backing was not rejected without value movement while preserving exit: {:?}",
            discovery
        );
    }

    #[test]
    fn v16_program_expired_backing_trade_routes_reject_stale_lien_creation(
        seed in any::<[u8; 32]>(),
        route in prop::sample::select(DiscoveryTradeRoute::ALL.to_vec()),
        expiry_offset in prop::sample::select(vec![1u8, 2, 4, 6]),
    ) {
        let discovery = discover_expired_backing_trade_route(seed, route, expiry_offset)
            .map_err(TestCaseError::fail)?;
        prop_assert!(
            discovery.rejects_expired_risk_increase_safely(),
            "{route:?} did not reject an authenticated-expiry lien safely: {discovery:?}"
        );
        prop_assert!(
            discovery.preserves_risk_reduction(),
            "{route:?} did not preserve post-expiry risk reduction: {discovery:?}"
        );
    }

    #[test]
    fn v16_program_retained_maturity_matrix_discovers_terminal_funded_lock(
        seed in any::<[u8; 32]>(),
        expiry_offset in prop::sample::select(vec![2u8, 3, 4, 6]),
    ) {
        let discoveries = discover_retained_maturity_terminal_locks(seed, expiry_offset);
        prop_assert!(
            discoveries.is_ok(),
            "retained-maturity discovery failed at offset {expiry_offset}: {}",
            discoveries.unwrap_err()
        );
        let discoveries = discoveries.unwrap();
        prop_assert_eq!(
            discoveries.len(),
            RetainedMaturityKind::ALL.len(),
            "every retained maturity operation needs a generated world"
        );
        for discovery in discoveries {
            prop_assert!(
                discovery.is_persistent_funded_lock(),
                "retained operation did not reproduce an exact funded terminal lock: {:?}",
                discovery
            );
        }
    }

    #[test]
    fn v16_program_expired_backing_consumer_matrix_rejects_lapsed_conversion(
        seed in any::<[u8; 32]>(),
        expiry_offset in prop::sample::select(vec![1u8, 2, 4, 6]),
    ) {
        let discoveries = discover_expired_backing_consumers(seed, expiry_offset);
        prop_assert!(
            discoveries.is_ok(),
            "expired-backing consumer verification failed at offset {expiry_offset}: {}",
            discoveries.unwrap_err()
        );
        let discoveries = discoveries.unwrap();
        prop_assert_eq!(
            discoveries.len(),
            ExpiredBackingConsumerKind::ALL.len(),
            "every favorable backing consumer needs a generated expiry world"
        );
        for discovery in discoveries {
            prop_assert!(
                discovery.rejects_lapsed_conversion_and_preserves_senior_exit(),
                "expired backing consumer was not rejected safely with a senior exit: {:?}",
                discovery
            );
        }
    }
}
