//! INV-041 - Deterministic allocation and caller-order independence.
//!
//! Normative obligation: caller-selected pair and continuation ordering cannot
//! change user value, source-domain claims, backing classification, or loss
//! attribution.
//!
//! Evidence in this file (I/bounded R/M): two equal-sized public positions
//! create claims against one deliberately scarce backing domain. After an
//! authenticated mark move and public shutdown, the model exhausts both pair
//! orders crossed with one-shot and dust-chunked force-close schedules. It
//! compares account-local claims and domain-level accounting, not merely total
//! vault value. Other liquidation, insurance, lien, and close-preemption order
//! spaces remain outside this bounded topology.

use crate::support::v16_svm::{MarketConfig, V16Svm};
use percolator::{BOUND_SCALE, POS_SCALE};
use percolator_prog::ix::CrankObservationHint;

const ASSET: u16 = 0;
const SOURCE_DOMAIN: usize = 1;
const OPEN_PRICE: u64 = 101;
const CLOSE_PRICE: u64 = 137;
const SIZE_Q: u128 = POS_SCALE + 17;

#[derive(Clone, Debug, PartialEq, Eq)]
struct AccountOutcome {
    capital: u128,
    pnl: i128,
    fee_credits: i128,
    source_claims: Vec<(u32, u64, u128)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AllocationOutcome {
    accounts: Vec<AccountOutcome>,
    insurance: u128,
    c_tot: u128,
    vault: u128,
    source_positive_claim_bound_num: u128,
    source_fresh_reserved_backing_num: u128,
    source_provider_receivable_num: u128,
    bucket_fresh_unliened_backing_num: u128,
    bucket_valid_liened_backing_num: u128,
    bucket_consumed_liened_backing_num: u128,
    vault_tokens: u64,
    token_supply: u128,
}

fn has_active_leg(env: &V16Svm, actor: usize) -> bool {
    env.primary_portfolio(actor)
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .any(|leg| leg.active && leg.asset_index == u32::from(ASSET))
}

fn crank_to_fixed_point(env: &mut V16Svm, actor: usize, slot: u64) {
    let observations = vec![CrankObservationHint {
        asset_index: ASSET,
        oracle_accounts: env.primary_profile(ASSET as usize).oracle_leg_count,
    }];
    let mut progressed = false;
    for _ in 0..16 {
        match env.crank(actor, slot, observations.clone()) {
            Ok(_) => progressed = true,
            Err(error) if progressed && error.contains("Custom(22)") => break,
            Err(error) => panic!("actor {actor} crank failed before fixed point: {error}"),
        }
    }
    assert!(progressed, "actor {actor} must make bounded crank progress");
}

fn account_outcome(env: &V16Svm, actor: usize) -> AccountOutcome {
    let account = env.primary_portfolio(actor);
    let mut source_claims = account
        .source_domains
        .iter()
        .filter(|source| source.is_occupied())
        .map(|source| {
            (
                source.domain.get(),
                source.source_claim_market_id.get(),
                source.source_claim_bound_num.get(),
            )
        })
        .collect::<Vec<_>>();
    source_claims.sort_unstable();
    AccountOutcome {
        capital: account.capital.get(),
        pnl: account.pnl.get(),
        fee_credits: account.fee_credits.get(),
        source_claims,
    }
}

fn run_schedule(pair_order: [usize; 2], chunks: &[u128]) -> AllocationOutcome {
    let config = MarketConfig {
        initial_price: OPEN_PRICE,
        max_price_move_bps_per_slot: 10_000,
        max_accrual_dt_slots: 1,
        min_funding_lifetime_slots: 1,
        ..MarketConfig::default()
    };
    let mut env = V16Svm::new([0x41; 32], config);
    env.configure_permissionless_resolve(100, 1)
        .expect("configure public force-close timing");
    env.top_up_backing_bucket(SOURCE_DOMAIN as u16, 50, 20)
        .expect("fund deliberately scarce source backing");

    for (winner, loser) in [(0usize, 2usize), (1, 3)] {
        env.trade_no_cpi(winner, loser, ASSET, SIZE_Q as i128, OPEN_PRICE, 0)
            .expect("open equal public position pair");
    }
    env.warp_to_slot(2);
    env.push_auth_mark(ASSET, 2, CLOSE_PRICE)
        .expect("publish authenticated favorable mark");
    crank_to_fixed_point(&mut env, 4, 2);
    for actor in 0..4 {
        crank_to_fixed_point(&mut env, actor, 2);
    }

    env.warp_to_slot(3);
    env.shutdown_asset(ASSET, 3)
        .expect("enter public Recovery lifecycle");
    env.warp_to_slot(5);
    let pairs = [(0usize, 2usize), (1usize, 3usize)];
    for pair_index in pair_order {
        let (winner, loser) = pairs[pair_index];
        for &chunk in chunks {
            if !has_active_leg(&env, winner) {
                break;
            }
            env.force_close_abandoned_asset(4, winner, loser, ASSET, 5, chunk)
                .expect("force-close continuation");
        }
        if has_active_leg(&env, winner) {
            env.force_close_abandoned_asset(4, winner, loser, ASSET, 5, u128::MAX)
                .expect("terminal force-close remainder");
        }
        assert!(!has_active_leg(&env, winner));
        assert!(!has_active_leg(&env, loser));
    }

    let (_, market) = env.primary_market_state();
    assert_eq!(market.assets[ASSET as usize].oi_eff_long_q, 0);
    assert_eq!(market.assets[ASSET as usize].oi_eff_short_q, 0);
    let source = market.source_credit[SOURCE_DOMAIN];
    let bucket = market.source_backing_buckets[SOURCE_DOMAIN];
    AllocationOutcome {
        accounts: (0..4).map(|actor| account_outcome(&env, actor)).collect(),
        insurance: market.insurance,
        c_tot: market.c_tot,
        vault: market.vault,
        source_positive_claim_bound_num: source.positive_claim_bound_num,
        source_fresh_reserved_backing_num: source.fresh_reserved_backing_num,
        source_provider_receivable_num: source.provider_receivable_num,
        bucket_fresh_unliened_backing_num: bucket.fresh_unliened_backing_num,
        bucket_valid_liened_backing_num: bucket.valid_liened_backing_num,
        bucket_consumed_liened_backing_num: bucket.consumed_liened_backing_num,
        vault_tokens: env.token_amount(env.vault),
        token_supply: env.token_supply_observed(),
    }
}

#[test]
fn v16_program_scarce_backing_force_close_exhausts_pair_and_chunk_orders() {
    let full = [u128::MAX];
    let dust = [1, POS_SCALE / 3, 7, POS_SCALE / 2, u128::MAX];
    let baseline = run_schedule([0, 1], &full);
    assert!(
        baseline.source_positive_claim_bound_num > 50 * BOUND_SCALE,
        "topology must create claims exceeding deliberately scarce backing"
    );
    for (pair_order, chunks) in [
        ([1, 0], full.as_slice()),
        ([0, 1], dust.as_slice()),
        ([1, 0], dust.as_slice()),
    ] {
        assert_eq!(
            run_schedule(pair_order, chunks),
            baseline,
            "caller-selected pair/chunk order changed allocation: pairs={pair_order:?}, chunks={chunks:?}"
        );
    }
}
