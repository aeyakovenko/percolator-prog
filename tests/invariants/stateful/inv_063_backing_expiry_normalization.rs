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
//! `v16_program_backing_principal_release_respects_authenticated_expiry` retains a provider
//! withdrawal while the bucket is fresh and lands it at all three authenticated boundaries. Only
//! the pre-expiry request may recover principal; equal/late requests must roll back rather than
//! bypass expiry forfeiture.
//!
//! Guarantee boundary: the trade, conversion, and retained-top-up consumers have fixed-pin bounded
//! evidence over the generated route and expiry boundaries represented here.

use super::*;
use crate::support::v16_svm::{MarketConfig, V16Svm};
use percolator::{BOUND_SCALE, POS_SCALE};
use percolator_prog::ix::CrankObservationHint;

#[derive(Clone, Debug, PartialEq, Eq)]
struct EconomicSnapshot {
    markets: [Vec<u8>; 2],
    portfolios: Vec<Vec<u8>>,
    foreign_portfolio: Vec<u8>,
    backing_ledger: Vec<u8>,
    matcher_contexts: Vec<Vec<u8>>,
    tokens: Vec<(solana_sdk::pubkey::Pubkey, Vec<u8>)>,
    lamports: Vec<(solana_sdk::pubkey::Pubkey, u64)>,
}

fn snapshot(env: &V16Svm) -> EconomicSnapshot {
    EconomicSnapshot {
        markets: [env.market_data(false), env.market_data(true)],
        portfolios: env.all_primary_portfolio_data(),
        foreign_portfolio: env.foreign_portfolio_data(),
        backing_ledger: env.backing_domain_ledger_data(),
        matcher_contexts: env.all_matcher_context_data(),
        tokens: env.all_token_account_data(),
        lamports: env.all_economic_account_lamports(),
    }
}

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

#[test]
fn v16_program_backing_principal_release_respects_authenticated_expiry() {
    const WINNER: usize = 0;
    const LOSER: usize = 1;
    const PROVIDER: usize = 2;
    const ASSET: u16 = 0;
    const DOMAIN: u16 = 1;
    const BACKING: u128 = 150;
    const WITHDRAWAL: u128 = 25;
    const EXPIRY_SLOT: u64 = 5;
    const INITIAL_PRICE: u64 = 100;
    const WINNING_PRICE: u64 = 105;
    const SIZE_Q: i128 = 20 * POS_SCALE as i128;

    for landing in BackingExpiryLanding::ALL {
        let mut seed = [0x66; 32];
        seed[0] ^= match landing {
            BackingExpiryLanding::Before => 1,
            BackingExpiryLanding::At => 2,
            BackingExpiryLanding::After => 3,
        };
        let mut env = V16Svm::new(
            seed,
            MarketConfig {
                initial_price: INITIAL_PRICE,
                maintenance_margin_bps: 1_000,
                initial_margin_bps: 1_000,
                max_price_move_bps_per_slot: 500,
                max_accrual_dt_slots: 1,
                min_funding_lifetime_slots: 1,
                actor_deposits: [1_000, 1_000, 0, 0, 0],
                ..MarketConfig::default()
            },
        );
        let supply_before = env.token_supply_observed();
        env.update_asset_authority_from_admin(
            ASSET,
            percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET,
            PROVIDER,
        )
        .expect("install the independent backing provider");
        env.top_up_backing_bucket_for_actor(PROVIDER, DOMAIN, BACKING, EXPIRY_SLOT)
            .expect("fund the expiring backing bucket");
        env.trade_no_cpi(WINNER, LOSER, ASSET, SIZE_Q, INITIAL_PRICE, 0)
            .expect("open a position whose favorable PnL uses the source domain");
        env.warp_to_slot(2);
        env.push_auth_mark(ASSET, 2, WINNING_PRICE)
            .expect("publish the favorable authenticated mark");
        let oracle_accounts = env.primary_profile(ASSET as usize).oracle_leg_count;
        let observations = || {
            vec![CrankObservationHint {
                asset_index: ASSET,
                oracle_accounts,
            }]
        };
        for actor in [LOSER, WINNER] {
            env.crank(actor, 2, observations())
                .expect("refresh both sides at the favorable mark");
        }
        env.trade_no_cpi(WINNER, LOSER, ASSET, -SIZE_Q, WINNING_PRICE, 0)
            .expect("flatten and retain a real source-backed winner claim");
        let winner_claim =
            env.primary_market_state().1.source_credit[DOMAIN as usize].positive_claim_bound_num;
        assert!(
            winner_claim != 0,
            "fixture must create an independent claim"
        );
        let fresh_backing_before = env.primary_market_state().1.source_backing_buckets
            [DOMAIN as usize]
            .fresh_unliened_backing_num;
        assert!(
            fresh_backing_before
                .checked_sub(WITHDRAWAL * BOUND_SCALE)
                .is_some_and(|remaining| remaining >= winner_claim),
            "fresh control withdrawal must remove only backing excess above the live claim"
        );
        let retained =
            env.build_retained_backing_bucket_withdrawal_for_actor(PROVIDER, DOMAIN, WITHDRAWAL);
        let destination = env.actors[PROVIDER].destination_token;
        let destination_before = env.token_amount(destination);
        let vault_before = env.token_amount(env.vault);
        let internal_vault_before = env.primary_market_state().1.vault;
        let before_landing = snapshot(&env);
        let landing_slot = match landing {
            BackingExpiryLanding::Before => EXPIRY_SLOT - 1,
            BackingExpiryLanding::At => EXPIRY_SLOT,
            BackingExpiryLanding::After => EXPIRY_SLOT + 1,
        };
        env.warp_to_slot(landing_slot);
        let result = env.land_retained(retained);

        match landing {
            BackingExpiryLanding::Before => {
                result.expect("fresh retained backing withdrawal must land");
                assert_eq!(
                    env.token_amount(destination) - destination_before,
                    WITHDRAWAL as u64
                );
                assert_eq!(
                    vault_before - env.token_amount(env.vault),
                    WITHDRAWAL as u64
                );
                assert_eq!(
                    internal_vault_before - env.primary_market_state().1.vault,
                    WITHDRAWAL
                );
                assert_eq!(
                    env.primary_market_state().1.source_backing_buckets[DOMAIN as usize]
                        .fresh_unliened_backing_num,
                    fresh_backing_before - WITHDRAWAL * BOUND_SCALE
                );
            }
            BackingExpiryLanding::At | BackingExpiryLanding::After => {
                let error = result.expect_err("expired retained backing withdrawal must reject");
                assert!(
                    error.contains("Custom(19)") || error.contains("custom program error: 0x13"),
                    "expired withdrawal must reject as EngineStale: {error}"
                );
                assert_eq!(
                    snapshot(&env),
                    before_landing,
                    "expired provider withdrawal must roll back exactly at {landing:?}"
                );
                assert_eq!(env.token_amount(destination), destination_before);
                assert_eq!(env.token_amount(env.vault), vault_before);
                assert_eq!(env.primary_market_state().1.vault, internal_vault_before);

                let mut expiry_steps = 0usize;
                while env.primary_market_state().1.source_backing_buckets[DOMAIN as usize].status
                    == percolator::BackingBucketStatusV16::Fresh
                    && expiry_steps < 8
                {
                    env.crank(WINNER, landing_slot, observations())
                        .expect("a bounded claimant crank must progress expiry");
                    expiry_steps += 1;
                }
                let expired = env.primary_market_state().1.source_backing_buckets[DOMAIN as usize];
                assert_ne!(
                    expired.status,
                    percolator::BackingBucketStatusV16::Fresh,
                    "the canonical expiry continuation must remove freshness"
                );
                assert_eq!(expired.fresh_unliened_backing_num, 0);
                assert!(expiry_steps != 0 && expiry_steps <= 8);
                assert_eq!(
                    env.primary_market_state().1.source_credit[DOMAIN as usize]
                        .fresh_reserved_backing_num,
                    0
                );
                assert_eq!(env.token_amount(destination), destination_before);
                assert_eq!(env.token_amount(env.vault), vault_before);
                assert_eq!(env.primary_market_state().1.vault, internal_vault_before);

                env.withdraw_backing_bucket_for_actor(PROVIDER, DOMAIN, WITHDRAWAL)
                    .expect_err("expired provider principal must remain non-withdrawable");
                assert_eq!(env.token_amount(destination), destination_before);
                assert_eq!(env.token_amount(env.vault), vault_before);
            }
        }
        assert_eq!(env.token_supply_observed(), supply_before);
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
