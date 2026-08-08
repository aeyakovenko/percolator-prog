//! INV-069 - Terminal normalization and retirement.
//!
//! Normative obligation: real obligations block retirement without being
//! consumed, while an economically empty asset can reach its terminal state.
//!
//! Evidence in this file (I/bounded R): public instructions fund both an
//! insurance domain and its backing bucket, then exhaust both legal drain
//! orders. The four-state obligation lattice `{11, 10, 01, 00}` is therefore
//! covered: retirement rejects with exact account, token, and lamport rollback
//! in every nonempty state and succeeds only at `00`. This complements the
//! single-blocker CU regressions; it is not an exhaustive model of spent
//! history, pending losses, receipts, or every terminal label.

use crate::support::v16_svm::{MarketConfig, V16Svm};
use percolator::{AssetLifecycleV16, BOUND_SCALE};
use solana_sdk::pubkey::Pubkey;

const ASSET_INDEX: u16 = 1;
const LONG_DOMAIN: u16 = 2;
const INSURANCE_AMOUNT: u128 = 5_000;
const BACKING_AMOUNT: u128 = 700;

#[derive(Clone, Copy, Debug)]
enum FirstDrain {
    Insurance,
    Backing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EconomicSnapshot {
    market: Vec<u8>,
    foreign_market: Vec<u8>,
    portfolios: Vec<Vec<u8>>,
    foreign_portfolio: Vec<u8>,
    backing_ledger: Vec<u8>,
    tokens: Vec<(Pubkey, Vec<u8>)>,
    lamports: Vec<(Pubkey, u64)>,
}

fn snapshot(env: &V16Svm) -> EconomicSnapshot {
    EconomicSnapshot {
        market: env.market_data(false),
        foreign_market: env.market_data(true),
        portfolios: env.all_primary_portfolio_data(),
        foreign_portfolio: env.foreign_portfolio_data(),
        backing_ledger: env.backing_domain_ledger_data(),
        tokens: env.all_token_account_data(),
        lamports: env.all_economic_account_lamports(),
    }
}

fn assert_obligation_state(env: &V16Svm, insurance: bool, backing: bool) {
    let (_, market) = env.primary_market_state();
    assert_eq!(
        market.insurance_domain_budget[LONG_DOMAIN as usize],
        if insurance { INSURANCE_AMOUNT } else { 0 }
    );
    assert_eq!(
        market.source_backing_buckets[LONG_DOMAIN as usize].fresh_unliened_backing_num,
        if backing {
            BACKING_AMOUNT * BOUND_SCALE
        } else {
            0
        }
    );
}

fn assert_retire_rejects_atomically(env: &mut V16Svm, slot: u64) {
    env.warp_to_slot(slot);
    let before = snapshot(env);
    let error = env
        .retire_asset(ASSET_INDEX, slot)
        .expect_err("a live terminal obligation must block retirement");
    assert!(
        error.contains("Custom(") || error.contains("custom program error"),
        "unexpected retirement rejection: {error}"
    );
    assert_eq!(
        snapshot(env),
        before,
        "failed retirement must preserve every tracked economic account"
    );
    assert_eq!(
        env.primary_market_state().1.assets[ASSET_INDEX as usize].lifecycle,
        AssetLifecycleV16::Active
    );
}

fn run_drain_order(first: FirstDrain) {
    let mut seed = [0x69; 32];
    seed[0] = match first {
        FirstDrain::Insurance => 0,
        FirstDrain::Backing => 1,
    };
    let mut env = V16Svm::new(seed, MarketConfig::default());

    env.top_up_insurance_domain(LONG_DOMAIN, INSURANCE_AMOUNT)
        .expect("publicly fund insurance domain");
    env.top_up_backing_bucket(LONG_DOMAIN, BACKING_AMOUNT, 10_000)
        .expect("publicly fund backing bucket");
    assert_obligation_state(&env, true, true);
    let supply = env.token_supply_observed();
    let destination_before = env.token_amount(env.provider_destination_token);

    assert_retire_rejects_atomically(&mut env, 2);

    match first {
        FirstDrain::Insurance => {
            env.withdraw_insurance_asset_as_admin(ASSET_INDEX, INSURANCE_AMOUNT)
                .expect("publicly drain insurance domain");
            assert_obligation_state(&env, false, true);
        }
        FirstDrain::Backing => {
            env.withdraw_backing_bucket(LONG_DOMAIN, BACKING_AMOUNT)
                .expect("publicly drain backing bucket");
            assert_obligation_state(&env, true, false);
        }
    }
    assert_retire_rejects_atomically(&mut env, 3);

    match first {
        FirstDrain::Insurance => env
            .withdraw_backing_bucket(LONG_DOMAIN, BACKING_AMOUNT)
            .expect("publicly drain remaining backing bucket"),
        FirstDrain::Backing => env
            .withdraw_insurance_asset_as_admin(ASSET_INDEX, INSURANCE_AMOUNT)
            .expect("publicly drain remaining insurance domain"),
    };
    assert_obligation_state(&env, false, false);

    env.warp_to_slot(4);
    env.retire_asset(ASSET_INDEX, 4)
        .expect("empty asset must reach Retired");
    let (_, retired) = env.primary_market_state();
    assert_eq!(
        retired.assets[ASSET_INDEX as usize].lifecycle,
        AssetLifecycleV16::Retired
    );
    assert_eq!(
        env.token_amount(env.provider_destination_token),
        destination_before + INSURANCE_AMOUNT as u64 + BACKING_AMOUNT as u64,
        "both principals return exactly once regardless of drain order"
    );
    assert_eq!(
        env.token_supply_observed(),
        supply,
        "terminal normalization cannot create or destroy SPL value"
    );
}

#[test]
fn v16_program_retirement_obligation_lattice_is_order_independent() {
    for first in [FirstDrain::Insurance, FirstDrain::Backing] {
        run_drain_order(first);
    }
}
