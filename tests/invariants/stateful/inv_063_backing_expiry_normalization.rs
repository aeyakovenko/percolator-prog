//! INV-063 - Backing-expiry normalization.
//!
//! Normative obligation: Expired backing is normalized before every consumer and cannot remain economically fresh.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_backing_expiry_boundary_rejects_stale_fee_and_preserves_exit` constructs a retained
//! trade while backing is fresh and lands it after authenticated Clock expiry. The unsafe increase
//! must return `EngineStale` with exact rollback, zero provider fee, and no victim loss; a reducing
//! trade must remain executable.
//! `v16_program_backing_expiry_trade_route_boundary_matrix` repeats the freshness check through all
//! four public trade routes at `expiry-1`, `expiry`, and `expiry+1`. Every pre-expiry control must
//! grow a real counterparty-backed lien; single routes must also charge and extract a real provider
//! fee. Both expired boundaries reject atomically and preserve a risk-reducing trade.
//! `v16_program_retained_backing_topup_boundary_matrix` generates signed retained top-ups at all
//! three expiry boundaries and compares omitted and submitted operations. A fresh request debits
//! provider SPL and credits canonical custody/accounting exactly, then remains boundedly
//! settleable after the backing lapses. Expired requests roll back every delta and preserve
//! terminal user progress. The immediately preceding TDD commit reproduces PR291's terminal lock
//! with this same matrix on the pre-fix engine pin.
//! `v16_program_backing_expiry_conversion_boundary_matrix` generates released source-backed claims
//! at all three expiry boundaries. The pre-expiry control must consume backing, credit capital, and
//! withdraw real SPL value; both expired boundaries reject with exact rollback and zero
//! provider-principal movement while preserving withdrawal of all senior capital.
//!
//! Guarantee boundary: the trade, conversion, and retained-top-up consumers have fixed-pin bounded
//! evidence over the generated route and expiry boundaries represented here.

use super::*;

fn assert_backing_expiry_trade_route_boundary(discovery: &ExpiredBackingTradeRouteDiscovery) {
    match discovery.landing {
        BackingExpiryLanding::Before => assert!(
            discovery.uses_fresh_backing_nonvacuously(),
            "{:?} did not consume fresh backing before expiry: {discovery:?}",
            discovery.route
        ),
        BackingExpiryLanding::At | BackingExpiryLanding::After => {
            assert!(
                discovery.rejects_expired_risk_increase_safely(),
                "{:?} did not reject a {:?} authenticated-expiry lien with exact rollback: {discovery:?}",
                discovery.route,
                discovery.landing
            );
            assert!(
                discovery.preserves_risk_reduction(),
                "{:?} did not preserve a {:?} risk-reducing trade: {discovery:?}",
                discovery.route,
                discovery.landing
            );
        }
    }
}

fn assert_backing_expiry_consumer_boundary(discovery: &ExpiredBackingConsumerDiscovery) {
    match discovery.landing {
        BackingExpiryLanding::Before => assert!(
            discovery.consumes_fresh_backing_nonvacuously(),
            "{:?} did not consume fresh backing before expiry: {discovery:?}",
            discovery.kind
        ),
        BackingExpiryLanding::At | BackingExpiryLanding::After => assert!(
            discovery.rejects_lapsed_conversion_and_preserves_senior_exit(),
            "{:?} did not reject a {:?} backing conversion safely: {discovery:?}",
            discovery.kind,
            discovery.landing
        ),
    }
}

fn assert_retained_maturity_boundary(discovery: &RetainedMaturityDiscovery) {
    match discovery.landing {
        BackingExpiryLanding::Before => assert!(
            discovery.accepts_fresh_intent_and_preserves_terminal_progress(),
            "{:?} did not execute fresh retained backing and settle boundedly: {discovery:?}",
            discovery.kind
        ),
        BackingExpiryLanding::At | BackingExpiryLanding::After => assert!(
            discovery.rejects_expired_intent_and_preserves_terminal_progress(),
            "{:?} did not reject a {:?} retained request while preserving terminal progress: {discovery:?}",
            discovery.kind,
            discovery.landing
        ),
    }
}

#[test]
fn v16_program_backing_expiry_trade_route_boundary_matrix() {
    let discoveries = discover_backing_expiry_trade_route_boundaries([0x63; 32], 2)
        .expect("build every public trade-route and expiry-boundary world");
    assert_eq!(
        discoveries.len(),
        DiscoveryTradeRoute::ALL.len() * BackingExpiryLanding::ALL.len()
    );
    for discovery in &discoveries {
        assert_backing_expiry_trade_route_boundary(discovery);
    }
}

#[test]
fn v16_program_backing_expiry_conversion_boundary_matrix() {
    let discoveries = discover_backing_expiry_consumer_boundaries([0x64; 32], 2)
        .expect("build every favorable backing-consumer and expiry-boundary world");
    assert_eq!(
        discoveries.len(),
        ExpiredBackingConsumerKind::ALL.len() * BackingExpiryLanding::ALL.len()
    );
    for discovery in &discoveries {
        assert_backing_expiry_consumer_boundary(discovery);
    }
}

#[test]
fn v16_program_retained_backing_topup_boundary_matrix() {
    let discoveries = discover_retained_maturity_boundaries([0x65; 32], 3)
        .expect("build every retained maturity and expiry-boundary world");
    assert_eq!(
        discoveries.len(),
        RetainedMaturityKind::ALL.len() * BackingExpiryLanding::ALL.len()
    );
    for discovery in &discoveries {
        assert_retained_maturity_boundary(discovery);
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
    fn v16_program_backing_expiry_trade_routes_respect_boundary(
        seed in any::<[u8; 32]>(),
        route in prop::sample::select(DiscoveryTradeRoute::ALL.to_vec()),
        landing in prop::sample::select(BackingExpiryLanding::ALL.to_vec()),
        expiry_offset in prop::sample::select(vec![1u8, 2, 4, 6]),
    ) {
        let discovery = discover_backing_expiry_trade_route_boundary(
            seed,
            route,
            expiry_offset,
            landing,
        )
            .map_err(TestCaseError::fail)?;
        match landing {
            BackingExpiryLanding::Before => prop_assert!(
                discovery.uses_fresh_backing_nonvacuously(),
                "{route:?} did not use pre-expiry backing nonvacuously: {discovery:?}"
            ),
            BackingExpiryLanding::At | BackingExpiryLanding::After => {
                prop_assert!(
                    discovery.rejects_expired_risk_increase_safely(),
                    "{route:?} did not reject a {landing:?} authenticated-expiry lien safely: {discovery:?}"
                );
                prop_assert!(
                    discovery.preserves_risk_reduction(),
                    "{route:?} did not preserve {landing:?} risk reduction: {discovery:?}"
                );
            }
        }
    }

    #[test]
    fn v16_program_retained_maturity_matrix_respects_expiry_boundary(
        seed in any::<[u8; 32]>(),
        landing in prop::sample::select(BackingExpiryLanding::ALL.to_vec()),
        expiry_offset in prop::sample::select(vec![2u8, 3, 4, 6]),
    ) {
        let discoveries = discover_retained_maturity_boundary(seed, expiry_offset, landing);
        prop_assert!(
            discoveries.is_ok(),
            "retained-maturity verification failed at offset {expiry_offset}: {}",
            discoveries.unwrap_err()
        );
        let discoveries = discoveries.unwrap();
        prop_assert_eq!(
            discoveries.len(),
            RetainedMaturityKind::ALL.len(),
            "every retained maturity operation needs a generated world"
        );
        for discovery in discoveries {
            match landing {
                BackingExpiryLanding::Before => prop_assert!(
                    discovery.accepts_fresh_intent_and_preserves_terminal_progress(),
                    "fresh retained operation was not nonvacuous or terminal-safe: {discovery:?}"
                ),
                BackingExpiryLanding::At | BackingExpiryLanding::After => prop_assert!(
                    discovery.rejects_expired_intent_and_preserves_terminal_progress(),
                    "expired retained operation did not reject while preserving terminal progress: {discovery:?}"
                ),
            }
        }
    }

    #[test]
    fn v16_program_backing_expiry_consumer_matrix_respects_boundary(
        seed in any::<[u8; 32]>(),
        landing in prop::sample::select(BackingExpiryLanding::ALL.to_vec()),
        expiry_offset in prop::sample::select(vec![1u8, 2, 4, 6]),
    ) {
        let discoveries = discover_backing_expiry_consumer_boundary(
            seed,
            expiry_offset,
            landing,
        );
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
            match landing {
                BackingExpiryLanding::Before => prop_assert!(
                    discovery.consumes_fresh_backing_nonvacuously(),
                    "fresh backing consumer was not exercised nonvacuously: {discovery:?}"
                ),
                BackingExpiryLanding::At | BackingExpiryLanding::After => prop_assert!(
                    discovery.rejects_lapsed_conversion_and_preserves_senior_exit(),
                    "expired backing consumer was not rejected safely with a senior exit: {discovery:?}"
                ),
            }
        }
    }
}
