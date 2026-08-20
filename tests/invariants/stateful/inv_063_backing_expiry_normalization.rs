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
//! `v16_program_resolved_close_normalizes_backing_at_expiry` creates the source-backed claim through
//! public trades, resolves at all three boundaries, and permutes claimant plus terminal-route
//! order. Every rejected step rolls back exactly, every successful payout reconciles against both
//! engine and SPL custody, and equal/late resolution removes lapsed backing from the fresh-credit
//! classes before terminal disposition.
//! `v16_program_post_snapshot_expiry_topup_is_public_and_order_independent` then captures a genuinely
//! partial payout receipt before a second source domain expires. It advances authenticated Clock
//! through both terminal routes, requires a value-moving payout top-up, and proves that exact/late
//! expiry removes the lapsed backing without changing claimant-order or route-order economics.
//!
//! Guarantee boundary: the trade, conversion, and retained-top-up consumers have fixed-pin bounded
//! evidence over the generated route and expiry boundaries represented here.

use super::*;
use crate::support::v16_svm::{MarketConfig, V16Svm, TX_CU_LIMIT};
use percolator::{
    active_bitmap_is_empty, BackingBucketStatusV16, MarketModeV16, BOUND_SCALE, POS_SCALE,
};
use percolator_prog::ix::CrankObservationHint;
use percolator_prog::state;

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

fn inv063_resolved_portfolio_is_terminal(env: &V16Svm, actor: usize) -> bool {
    let group = env.primary_market_state().1;
    let account = env.primary_portfolio(actor);
    let Ok(receipt) = account.resolved_payout_receipt.try_to_runtime() else {
        return false;
    };
    let Ok(close) = account.close_progress.try_to_runtime() else {
        return false;
    };
    group.mode == MarketModeV16::Resolved
        && account.capital.get() == 0
        && account.pnl.get() == 0
        && account.reserved_pnl.get() == 0
        && account.fee_credits.get() == 0
        && account.cancel_deposit_escrow.get() == 0
        && active_bitmap_is_empty(state::portfolio_active_bitmap(&account))
        && account.stale_state == 0
        && account.b_stale_state == 0
        && account.rebalance_lock == 0
        && account.liquidation_lock == 0
        && account.last_fee_slot.get() == group.resolved_slot
        && account.health_cert.valid == 0
        && account
            .source_domains
            .iter()
            .all(|source| !source.is_occupied())
        && (!receipt.present || receipt.finalized)
        && (!close.active || (close.finalized && close.residual_remaining == 0))
}

fn drain_inv063_resolved_accounts(
    env: &mut V16Svm,
    actor_order: &[usize],
    claim_first: bool,
    label: &str,
) -> Result<u64, String> {
    const TERMINAL_SWEEP_BOUND: usize = 32;
    let route_order = if claim_first {
        [true, false]
    } else {
        [false, true]
    };
    let mut claim_route_payout = 0u64;

    for sweep in 0..TERMINAL_SWEEP_BOUND {
        if actor_order
            .iter()
            .copied()
            .all(|actor| inv063_resolved_portfolio_is_terminal(env, actor))
        {
            break;
        }
        let mut sweep_mutated = false;
        for actor in actor_order.iter().copied() {
            if inv063_resolved_portfolio_is_terminal(env, actor) {
                continue;
            }
            for is_claim in route_order {
                let before = snapshot(env);
                let engine_vault_before = env.primary_market_state().1.vault;
                let spl_vault_before = env.token_amount(env.vault);
                let destination = env.actors[actor].destination_token;
                let destination_before = env.token_amount(destination);
                let result = if is_claim {
                    env.claim_resolved_payout_topup_primary(actor)
                } else {
                    env.close_resolved_primary_signed(actor)
                };
                let Ok(success) = result else {
                    if snapshot(env) != before {
                        return Err(format!(
                            "{label} actor {actor} rejected terminal route mutated state"
                        ));
                    }
                    continue;
                };
                if success.compute_units >= TX_CU_LIMIT {
                    return Err(format!(
                        "{label} actor {actor} terminal route consumed {} CU",
                        success.compute_units
                    ));
                }
                let payout = env
                    .token_amount(destination)
                    .checked_sub(destination_before)
                    .ok_or_else(|| format!("{label} actor {actor} destination decreased"))?;
                let spl_debit = spl_vault_before
                    .checked_sub(env.token_amount(env.vault))
                    .ok_or_else(|| format!("{label} terminal route increased SPL vault"))?;
                let engine_debit = engine_vault_before
                    .checked_sub(env.primary_market_state().1.vault)
                    .ok_or_else(|| format!("{label} terminal route increased engine vault"))?;
                if payout != spl_debit || u128::from(payout) != engine_debit {
                    return Err(format!(
                        "{label} actor {actor} payout mismatch: destination={payout}, SPL={spl_debit}, engine={engine_debit}"
                    ));
                }
                if is_claim {
                    claim_route_payout = claim_route_payout
                        .checked_add(payout)
                        .ok_or_else(|| "claim-route payout overflow".to_string())?;
                }
                sweep_mutated |= snapshot(env) != before;
                if inv063_resolved_portfolio_is_terminal(env, actor) {
                    break;
                }
            }
        }
        if !sweep_mutated
            && !actor_order
                .iter()
                .copied()
                .all(|actor| inv063_resolved_portfolio_is_terminal(env, actor))
        {
            return Err(format!(
                "{label} terminal routes reached a nonterminal fixed point at sweep {sweep}"
            ));
        }
    }

    if !actor_order
        .iter()
        .copied()
        .all(|actor| inv063_resolved_portfolio_is_terminal(env, actor))
    {
        return Err(format!(
            "{label} terminal routes did not converge in {TERMINAL_SWEEP_BOUND} sweeps"
        ));
    }
    Ok(claim_route_payout)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ResolvedExpiryOutcome {
    payouts: [u64; 2],
    claim_route_payout: u64,
    bucket_status: BackingBucketStatusV16,
    fresh_unliened_backing_num: u128,
    valid_liened_backing_num: u128,
    consumed_liened_backing_num: u128,
    impaired_liened_backing_num: u128,
    fresh_reserved_backing_num: u128,
    valid_source_liened_backing_num: u128,
    impaired_source_liened_backing_num: u128,
    final_engine_vault: u128,
    final_spl_vault: u64,
}

fn run_resolved_expiry_world(
    landing: BackingExpiryLanding,
    winner_first: bool,
    claim_first: bool,
) -> Result<ResolvedExpiryOutcome, String> {
    const WINNER: usize = 0;
    const LOSER: usize = 1;
    const PROVIDER: usize = 2;
    const ASSET: u16 = 0;
    const WINNING_DOMAIN: u16 = 1;
    const INITIAL_PRICE: u64 = 100;
    const WINNING_MARK: u64 = 105;
    const SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const BACKING: u128 = 150;
    const EXPIRY_SLOT: u64 = 5;

    let mut seed = [0x67; 32];
    seed[0] ^= match landing {
        BackingExpiryLanding::Before => 1,
        BackingExpiryLanding::At => 2,
        BackingExpiryLanding::After => 3,
    };
    seed[1] ^= u8::from(winner_first);
    seed[2] ^= u8::from(claim_first);
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
    let token_supply_before = env.token_supply_observed();
    env.update_asset_authority_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET,
        PROVIDER,
    )
    .map_err(|error| format!("install backing provider: {error}"))?;
    env.top_up_backing_bucket_for_actor(PROVIDER, WINNING_DOMAIN, BACKING, EXPIRY_SLOT)
        .map_err(|error| format!("fund expiring backing: {error}"))?;
    env.trade_no_cpi(WINNER, LOSER, ASSET, SIZE_Q, INITIAL_PRICE, 0)
        .map_err(|error| format!("open source-backed position: {error}"))?;
    env.warp_to_slot(2);
    env.push_auth_mark(ASSET, 2, WINNING_MARK)
        .map_err(|error| format!("publish winning mark: {error}"))?;
    let oracle_accounts = env.primary_profile(ASSET as usize).oracle_leg_count;
    let observations = || {
        vec![CrankObservationHint {
            asset_index: ASSET,
            oracle_accounts,
        }]
    };
    for actor in [LOSER, WINNER] {
        env.crank(actor, 2, observations())
            .map_err(|error| format!("refresh source-backed actor {actor}: {error}"))?;
    }
    env.trade_no_cpi(WINNER, LOSER, ASSET, -SIZE_Q, WINNING_MARK, 0)
        .map_err(|error| format!("flatten source-backed position: {error}"))?;

    let before_resolution = env.primary_market_state().1;
    let claim_before =
        before_resolution.source_credit[WINNING_DOMAIN as usize].positive_claim_bound_num;
    let bucket_before = before_resolution.source_backing_buckets[WINNING_DOMAIN as usize];
    if claim_before == 0
        || bucket_before.status != BackingBucketStatusV16::Fresh
        || bucket_before.fresh_unliened_backing_num < claim_before
    {
        return Err(format!(
            "fixture did not create a fresh source-backed terminal claim: claim={claim_before}, bucket={bucket_before:?}"
        ));
    }

    let authenticated_slot = match landing {
        BackingExpiryLanding::Before => EXPIRY_SLOT - 1,
        BackingExpiryLanding::At => EXPIRY_SLOT,
        BackingExpiryLanding::After => EXPIRY_SLOT + 1,
    };
    env.warp_to_slot(authenticated_slot);
    env.resolve_market()
        .map_err(|error| format!("resolve at {landing:?}: {error}"))?;
    if env.primary_market_state().1.mode != MarketModeV16::Resolved {
        return Err(format!(
            "{landing:?} resolution did not enter Resolved mode"
        ));
    }
    let engine_vault_at_resolution = env.primary_market_state().1.vault;
    let spl_vault_at_resolution = env.token_amount(env.vault);
    let destinations_before = [
        env.token_amount(env.actors[WINNER].destination_token),
        env.token_amount(env.actors[LOSER].destination_token),
    ];
    let actor_order = if winner_first {
        [WINNER, LOSER]
    } else {
        [LOSER, WINNER]
    };
    let claim_route_payout = drain_inv063_resolved_accounts(
        &mut env,
        &actor_order,
        claim_first,
        &format!("{landing:?}"),
    )?;
    let group = env.primary_market_state().1;
    let bucket = group.source_backing_buckets[WINNING_DOMAIN as usize];
    let source = group.source_credit[WINNING_DOMAIN as usize];
    let payouts = [
        env.token_amount(env.actors[WINNER].destination_token)
            .checked_sub(destinations_before[0])
            .ok_or_else(|| "winner destination decreased".to_string())?,
        env.token_amount(env.actors[LOSER].destination_token)
            .checked_sub(destinations_before[1])
            .ok_or_else(|| "loser destination decreased".to_string())?,
    ];
    let payout_total = u128::from(payouts[0])
        .checked_add(u128::from(payouts[1]))
        .ok_or_else(|| "terminal payout total overflow".to_string())?;
    if engine_vault_at_resolution
        .checked_sub(group.vault)
        .ok_or_else(|| "terminal settlement increased engine vault".to_string())?
        != payout_total
        || spl_vault_at_resolution
            .checked_sub(env.token_amount(env.vault))
            .ok_or_else(|| "terminal settlement increased SPL vault".to_string())?
            != u64::try_from(payout_total).map_err(|_| "payout total exceeds u64".to_string())?
        || u128::from(env.token_amount(env.vault)) != group.vault
        || env.token_supply_observed() != token_supply_before
    {
        return Err(format!(
            "{landing:?} terminal custody did not reconcile: payouts={payouts:?}, engine={engine_vault_at_resolution}->{}, SPL={spl_vault_at_resolution}->{}, supply={token_supply_before}->{}",
            group.vault,
            env.token_amount(env.vault),
            env.token_supply_observed()
        ));
    }

    Ok(ResolvedExpiryOutcome {
        payouts,
        claim_route_payout,
        bucket_status: bucket.status,
        fresh_unliened_backing_num: bucket.fresh_unliened_backing_num,
        valid_liened_backing_num: bucket.valid_liened_backing_num,
        consumed_liened_backing_num: bucket.consumed_liened_backing_num,
        impaired_liened_backing_num: bucket.impaired_liened_backing_num,
        fresh_reserved_backing_num: source.fresh_reserved_backing_num,
        valid_source_liened_backing_num: source.valid_liened_backing_num,
        impaired_source_liened_backing_num: source.impaired_liened_backing_num,
        final_engine_vault: group.vault,
        final_spl_vault: env.token_amount(env.vault),
    })
}

#[test]
fn v16_program_resolved_close_normalizes_backing_at_expiry() {
    let mut canonical = Vec::new();
    for landing in BackingExpiryLanding::ALL {
        let mut outcomes = Vec::new();
        for winner_first in [false, true] {
            for claim_first in [false, true] {
                let outcome = run_resolved_expiry_world(landing, winner_first, claim_first)
                    .unwrap_or_else(|error| {
                        panic!(
                            "{landing:?}/winner_first={winner_first}/claim_first={claim_first}: {error}"
                        )
                    });
                outcomes.push(outcome);
            }
        }
        assert!(
            outcomes.windows(2).all(|pair| pair[0] == pair[1]),
            "{landing:?} terminal economics depend on claimant or payout-route order: {outcomes:?}"
        );
        let outcome = outcomes.remove(0);
        match landing {
            BackingExpiryLanding::Before => {
                assert!(
                    outcome.consumed_liened_backing_num != 0,
                    "fresh resolved close must consume source backing nonvacuously: {outcome:?}"
                );
            }
            BackingExpiryLanding::At | BackingExpiryLanding::After => {
                assert_ne!(outcome.bucket_status, BackingBucketStatusV16::Fresh);
                assert_eq!(outcome.fresh_unliened_backing_num, 0);
                assert_eq!(outcome.valid_liened_backing_num, 0);
                assert_eq!(outcome.consumed_liened_backing_num, 0);
                assert_eq!(outcome.fresh_reserved_backing_num, 0);
                assert_eq!(outcome.valid_source_liened_backing_num, 0);
            }
        }
        canonical.push(outcome);
    }
    assert_eq!(
        canonical[1], canonical[2],
        "exact and late expiry must have identical terminal economics"
    );
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PostSnapshotExpiryEconomicOutcome {
    payouts: [u64; 5],
    bucket_status: BackingBucketStatusV16,
    fresh_unliened_backing_num: u128,
    valid_liened_backing_num: u128,
    consumed_liened_backing_num: u128,
    fresh_reserved_backing_num: u128,
    valid_source_liened_backing_num: u128,
    final_engine_vault: u128,
    final_spl_vault: u64,
}

fn run_post_snapshot_expiry_claim_world(
    landing: BackingExpiryLanding,
    reverse_tail: bool,
    claim_first: bool,
) -> Result<(PostSnapshotExpiryEconomicOutcome, u64), String> {
    const JUNIOR_WINNER: usize = 0;
    const JUNIOR_LOSER: usize = 1;
    const BACKED_WINNER: usize = 2;
    const BACKED_LOSER: usize = 3;
    const PROVIDER: usize = 4;
    const BACKED_ASSET: u16 = 1;
    const JUNIOR_DOMAIN: u16 = 1;
    const BACKED_DOMAIN: u16 = 3;
    const INITIAL_PRICE: u64 = 100;
    const WINNING_MARK: u64 = 150;
    const SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const JUNIOR_BACKING: u128 = 1;
    const BACKING: u128 = 1_500;
    const SNAPSHOT_SLOT: u64 = 12;
    const EXPIRY_SLOT: u64 = 13;

    let mut seed = [0x68; 32];
    seed[0] ^= match landing {
        BackingExpiryLanding::Before => 1,
        BackingExpiryLanding::At => 2,
        BackingExpiryLanding::After => 3,
    };
    seed[1] ^= u8::from(reverse_tail);
    seed[2] ^= u8::from(claim_first);
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: INITIAL_PRICE,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [1_000, 250, 1_000, 250, 0],
            ..MarketConfig::default()
        },
    );
    let token_supply_before = env.token_supply_observed();
    env.update_asset_authority_from_admin(
        0,
        percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET,
        PROVIDER,
    )
    .map_err(|error| format!("install junior backing provider: {error}"))?;
    env.update_asset_authority_from_admin(
        BACKED_ASSET,
        percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET,
        PROVIDER,
    )
    .map_err(|error| format!("install post-snapshot backing provider: {error}"))?;
    env.top_up_backing_bucket_for_actor(PROVIDER, JUNIOR_DOMAIN, JUNIOR_BACKING, SNAPSHOT_SLOT)
        .map_err(|error| format!("fund expiring junior backing: {error}"))?;
    let backed_topup = env.build_retained_backing_bucket_top_up_for_actor(
        PROVIDER,
        BACKED_DOMAIN,
        BACKING,
        EXPIRY_SLOT,
    );
    env.land_retained(backed_topup)
        .map_err(|error| format!("fund post-snapshot backing: {error}"))?;
    env.trade_no_cpi(JUNIOR_WINNER, JUNIOR_LOSER, 0, SIZE_Q, INITIAL_PRICE, 0)
        .map_err(|error| format!("open junior claim: {error}"))?;
    env.trade_no_cpi(
        BACKED_WINNER,
        BACKED_LOSER,
        BACKED_ASSET,
        SIZE_Q,
        INITIAL_PRICE,
        0,
    )
    .map_err(|error| format!("open backed claim: {error}"))?;
    for (offset, mark) in (105..=WINNING_MARK).step_by(5).enumerate() {
        let slot = 2 + u64::try_from(offset).expect("bounded mark sequence");
        env.warp_to_slot(slot);
        for asset in [0, BACKED_ASSET] {
            env.push_auth_mark(asset, slot, mark)
                .map_err(|error| format!("publish asset {asset} mark {mark}: {error}"))?;
        }
        for (actor, asset) in [
            (JUNIOR_LOSER, 0),
            (JUNIOR_WINNER, 0),
            (BACKED_LOSER, BACKED_ASSET),
            (BACKED_WINNER, BACKED_ASSET),
        ] {
            let oracle_accounts = env.primary_profile(asset as usize).oracle_leg_count;
            env.crank(
                actor,
                slot,
                vec![CrankObservationHint {
                    asset_index: asset,
                    oracle_accounts,
                }],
            )
            .map_err(|error| {
                format!("refresh actor {actor} on asset {asset} at mark {mark}: {error}")
            })?;
        }
    }
    env.trade_no_cpi(JUNIOR_WINNER, JUNIOR_LOSER, 0, -SIZE_Q, WINNING_MARK, 0)
        .map_err(|error| format!("flatten junior claim: {error}"))?;
    env.trade_no_cpi(
        BACKED_WINNER,
        BACKED_LOSER,
        BACKED_ASSET,
        -SIZE_Q,
        WINNING_MARK,
        0,
    )
    .map_err(|error| format!("flatten backed claim: {error}"))?;
    let before_resolution = env.primary_market_state().1;
    if before_resolution.source_credit[JUNIOR_DOMAIN as usize].positive_claim_bound_num == 0
        || before_resolution.source_credit[BACKED_DOMAIN as usize].positive_claim_bound_num == 0
        || before_resolution.source_backing_buckets[JUNIOR_DOMAIN as usize].status
            != BackingBucketStatusV16::Fresh
        || before_resolution.source_backing_buckets[BACKED_DOMAIN as usize].status
            != BackingBucketStatusV16::Fresh
    {
        return Err(format!(
            "post-snapshot fixture did not create both junior and backed claims: junior={:?}, backed={:?}, bucket={:?}",
            before_resolution.source_credit[JUNIOR_DOMAIN as usize],
            before_resolution.source_credit[BACKED_DOMAIN as usize],
            before_resolution.source_backing_buckets[BACKED_DOMAIN as usize]
        ));
    }

    env.warp_to_slot(SNAPSHOT_SLOT);
    env.resolve_market()
        .map_err(|error| format!("resolve before backing expiry: {error}"))?;
    let engine_vault_at_resolution = env.primary_market_state().1.vault;
    let spl_vault_at_resolution = env.token_amount(env.vault);
    let destinations_before: [u64; 5] =
        std::array::from_fn(|actor| env.token_amount(env.actors[actor].destination_token));

    let premature_claim_payout = drain_inv063_resolved_accounts(
        &mut env,
        &[JUNIOR_LOSER, BACKED_LOSER, PROVIDER],
        true,
        "pre-snapshot blockers",
    )?;
    if premature_claim_payout != 0 || env.primary_market_state().1.payout_snapshot_captured {
        return Err(format!(
            "closing only nonclaimants unexpectedly paid a receipt or captured the payout snapshot: payout={premature_claim_payout}, ledger={:?}",
            env.primary_market_state().1.resolved_payout_ledger
        ));
    }

    let first_destination = env.actors[JUNIOR_WINNER].destination_token;
    let mut first_receipt = env
        .primary_portfolio(JUNIOR_WINNER)
        .resolved_payout_receipt
        .try_to_runtime()
        .map_err(|error| format!("decode initial claimant receipt: {error:?}"))?;
    for step in 0..8 {
        if first_receipt.present {
            break;
        }
        let first_before = snapshot(&env);
        let first_engine_vault = env.primary_market_state().1.vault;
        let first_spl_vault = env.token_amount(env.vault);
        let first_destination_before = env.token_amount(first_destination);
        let first = env
            .close_resolved_primary_signed(JUNIOR_WINNER)
            .map_err(|error| {
                format!("capture pre-expiry payout snapshot at step {step}: {error}")
            })?;
        if first.compute_units >= TX_CU_LIMIT {
            return Err(format!(
                "snapshot-capturing CloseResolved consumed {} CU",
                first.compute_units
            ));
        }
        let first_payout = env
            .token_amount(first_destination)
            .checked_sub(first_destination_before)
            .ok_or_else(|| "snapshot claimant destination decreased".to_string())?;
        let first_spl_debit = first_spl_vault
            .checked_sub(env.token_amount(env.vault))
            .ok_or_else(|| "snapshot close increased SPL vault".to_string())?;
        let first_engine_debit = first_engine_vault
            .checked_sub(env.primary_market_state().1.vault)
            .ok_or_else(|| "snapshot close increased engine vault".to_string())?;
        if first_payout != first_spl_debit || u128::from(first_payout) != first_engine_debit {
            return Err(format!(
                "snapshot payout mismatch: destination={first_payout}, SPL={first_spl_debit}, engine={first_engine_debit}"
            ));
        }
        if snapshot(&env) == first_before {
            let group = env.primary_market_state().1;
            let account = env.primary_portfolio(JUNIOR_WINNER);
            return Err(format!(
                "snapshot-capturing CloseResolved was a successful no-op at step {step}: snapshot={}, stale={}, b_stale={}, negative={}, blockers={}, capital={}, pnl={}, active={:?}, receipt={first_receipt:?}, ledger={:?}",
                group.payout_snapshot_captured,
                group.stale_certificate_count,
                group.b_stale_account_count,
                group.negative_pnl_account_count,
                group.resolved_payout_blocker_count,
                account.capital.get(),
                account.pnl.get(),
                state::portfolio_active_bitmap(&account),
                group.resolved_payout_ledger,
            ));
        }
        first_receipt = env
            .primary_portfolio(JUNIOR_WINNER)
            .resolved_payout_receipt
            .try_to_runtime()
            .map_err(|error| format!("decode claimant receipt at step {step}: {error:?}"))?;
    }
    if !env.primary_market_state().1.payout_snapshot_captured
        || !first_receipt.present
        || first_receipt.finalized
    {
        return Err(format!(
            "first public close did not leave a genuinely partial receipt: receipt={first_receipt:?}, ledger={:?}",
            env.primary_market_state().1.resolved_payout_ledger
        ));
    }

    let landing_slot = match landing {
        BackingExpiryLanding::Before => EXPIRY_SLOT - 1,
        BackingExpiryLanding::At => EXPIRY_SLOT,
        BackingExpiryLanding::After => EXPIRY_SLOT + 1,
    };
    env.warp_to_slot(landing_slot);
    let tail_order = if reverse_tail {
        [
            BACKED_LOSER,
            JUNIOR_LOSER,
            BACKED_WINNER,
            PROVIDER,
            JUNIOR_WINNER,
        ]
    } else {
        [
            BACKED_WINNER,
            JUNIOR_LOSER,
            BACKED_LOSER,
            PROVIDER,
            JUNIOR_WINNER,
        ]
    };
    let claim_route_payout = drain_inv063_resolved_accounts(
        &mut env,
        &tail_order,
        claim_first,
        &format!("post-snapshot {landing:?}"),
    )?;
    let group = env.primary_market_state().1;
    let bucket = group.source_backing_buckets[BACKED_DOMAIN as usize];
    let source = group.source_credit[BACKED_DOMAIN as usize];
    let payouts: [u64; 5] = std::array::from_fn(|actor| {
        env.token_amount(env.actors[actor].destination_token)
            .checked_sub(destinations_before[actor])
            .expect("terminal destination cannot decrease")
    });
    let payout_total = payouts
        .iter()
        .try_fold(0u128, |sum, payout| sum.checked_add(u128::from(*payout)));
    let Some(payout_total) = payout_total else {
        return Err("post-snapshot payout total overflow".to_string());
    };
    if engine_vault_at_resolution
        .checked_sub(group.vault)
        .ok_or_else(|| "post-snapshot settlement increased engine vault".to_string())?
        != payout_total
        || spl_vault_at_resolution
            .checked_sub(env.token_amount(env.vault))
            .ok_or_else(|| "post-snapshot settlement increased SPL vault".to_string())?
            != u64::try_from(payout_total).map_err(|_| "payout total exceeds u64".to_string())?
        || u128::from(env.token_amount(env.vault)) != group.vault
        || env.token_supply_observed() != token_supply_before
    {
        return Err(format!(
            "post-snapshot {landing:?} custody mismatch: payouts={payouts:?}, engine={engine_vault_at_resolution}->{}, SPL={spl_vault_at_resolution}->{}, supply={token_supply_before}->{}",
            group.vault,
            env.token_amount(env.vault),
            env.token_supply_observed()
        ));
    }
    Ok((
        PostSnapshotExpiryEconomicOutcome {
            payouts,
            bucket_status: bucket.status,
            fresh_unliened_backing_num: bucket.fresh_unliened_backing_num,
            valid_liened_backing_num: bucket.valid_liened_backing_num,
            consumed_liened_backing_num: bucket.consumed_liened_backing_num,
            fresh_reserved_backing_num: source.fresh_reserved_backing_num,
            valid_source_liened_backing_num: source.valid_liened_backing_num,
            final_engine_vault: group.vault,
            final_spl_vault: env.token_amount(env.vault),
        },
        claim_route_payout,
    ))
}

#[test]
fn v16_program_post_snapshot_expiry_topup_is_public_and_order_independent() {
    let mut canonical = Vec::new();
    for landing in BackingExpiryLanding::ALL {
        let mut outcomes = Vec::new();
        let mut claim_first_payouts = Vec::new();
        for reverse_tail in [false, true] {
            for claim_first in [false, true] {
                let (outcome, claim_route_payout) =
                    run_post_snapshot_expiry_claim_world(landing, reverse_tail, claim_first)
                        .unwrap_or_else(|error| {
                            panic!(
                                "{landing:?}/reverse_tail={reverse_tail}/claim_first={claim_first}: {error}"
                            )
                        });
                if claim_first {
                    claim_first_payouts.push(claim_route_payout);
                }
                outcomes.push(outcome);
            }
        }
        assert!(
            outcomes.windows(2).all(|pair| pair[0] == pair[1]),
            "post-snapshot {landing:?} economics depend on tail or route order: {outcomes:?}"
        );
        assert!(
            claim_first_payouts.iter().all(|payout| *payout != 0),
            "post-snapshot {landing:?} must exercise a value-moving ClaimResolvedPayoutTopup: {claim_first_payouts:?}"
        );
        let outcome = outcomes.remove(0);
        match landing {
            BackingExpiryLanding::Before => assert!(
                outcome.consumed_liened_backing_num != 0,
                "fresh post-snapshot support must be consumed: {outcome:?}"
            ),
            BackingExpiryLanding::At | BackingExpiryLanding::After => {
                assert_ne!(outcome.bucket_status, BackingBucketStatusV16::Fresh);
                assert_eq!(outcome.fresh_unliened_backing_num, 0);
                assert_eq!(outcome.valid_liened_backing_num, 0);
                assert_eq!(outcome.consumed_liened_backing_num, 0);
                assert_eq!(outcome.fresh_reserved_backing_num, 0);
                assert_eq!(outcome.valid_source_liened_backing_num, 0);
            }
        }
        canonical.push(outcome);
    }
    assert_eq!(
        canonical[1], canonical[2],
        "post-snapshot exact and late expiry must have identical terminal economics"
    );
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
