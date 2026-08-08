//! INV-030 - Credit-rate determinism and fail-closed behavior.
//!
//! Normative obligation: the persisted source-credit rate must equal an independent recomputation
//! from the current claim bound and unencumbered backing. Removing or expiring backing cannot make
//! the rate more favorable, while adding new backing may restore credit without deleting claims.
//!
//! Evidence in this file (F over public LiteSVM routes):
//! `v16_program_source_credit_rate_lifecycle_matches_independent_oracle` generates provider amounts
//! and an authenticated winning mark, creates a discounted claim through a real trade and crank,
//! then exercises backing addition, exact-expiry normalization, owner risk reduction with zero
//! source credit, and expired-bucket refill. The shared global postcondition recomputes every
//! primary and foreign source domain after every generated public action with overflow-free u128
//! long division independent of the engine's U256 routine. The persisted minimized seed is the
//! public trace that exposed the pre-fix lapsed-Fresh crank loop.
//! `v16_program_liened_backing_expiry_impairs_credit_and_rejects_new_risk_atomically` creates a
//! real counterparty lien through public trades, expires that live lien through the public crank,
//! and proves the resulting impaired backing contributes zero credit. Repeating the formerly
//! accepted risk increase must reject with exact account, token, and lamport rollback while the
//! persisted rate continues to match the independent oracle.
//!
//! Guarantee boundary: this covers deployed serialization and the generated lifecycle. The engine
//! owns the full-width pure arithmetic proof; broader reachability still requires the charter's
//! exhaustive model and all public source-credit mutation routes.

use super::*;
use crate::support::{
    fuzz_model::assert_source_credit_rates,
    v16_svm::{MarketConfig, V16Svm},
};
use percolator::{BackingBucketStatusV16, CREDIT_RATE_SCALE, POS_SCALE};
use percolator_prog::ix::CrankObservationHint;

fn inv_030_observations(env: &V16Svm, assets: &[u16]) -> Vec<CrankObservationHint> {
    assets
        .iter()
        .map(|asset_index| CrankObservationHint {
            asset_index: *asset_index,
            oracle_accounts: env.primary_profile(*asset_index as usize).oracle_leg_count,
        })
        .collect()
}

fn inv_030_crank_actor_steps(env: &mut V16Svm, actor: usize, slot: u64, assets: &[u16]) {
    let observations = inv_030_observations(env, assets);
    let mut progressed = false;
    for _ in 0..32 {
        match env.crank(actor, slot, observations.clone()) {
            Ok(_) => progressed = true,
            Err(error) if progressed && error.contains("Custom(22)") => return,
            Err(error) => panic!("INV-030 actor {actor} crank failed before progress: {error}"),
        }
    }
    assert!(progressed, "INV-030 actor {actor} crank made no progress");
}

#[test]
fn v16_program_liened_backing_expiry_impairs_credit_and_rejects_new_risk_atomically() {
    const WINNER: usize = 0;
    const COUNTERPARTY: usize = 1;
    const MARKET_CRANKER: usize = 4;
    const WINNING_ASSET: u16 = 0;
    const ADVERSE_ASSET: u16 = 1;
    const SOURCE_DOMAIN: usize = 1;
    const START_PRICE: u64 = 100;
    const WINNING_MARK: u64 = 105;
    const EXPIRY_WINNING_MARK: u64 = 106;
    const ADVERSE_MARK: u64 = 95;
    const WINNING_SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const ADVERSE_SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const RISK_INCREASE_Q: i128 = 2 * POS_SCALE as i128;
    const BACKING_ATOMS: u128 = 150;
    const EXPIRY_SLOT: u64 = 3;

    let mut env = V16Svm::new(
        [0x30; 32],
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

    env.top_up_backing_bucket(SOURCE_DOMAIN as u16, BACKING_ATOMS, EXPIRY_SLOT)
        .expect("INV-030 fresh backing top-up");
    env.trade_no_cpi(
        WINNER,
        COUNTERPARTY,
        WINNING_ASSET,
        WINNING_SIZE_Q,
        START_PRICE,
        0,
    )
    .expect("INV-030 winning-leg open");
    env.trade_no_cpi(
        WINNER,
        COUNTERPARTY,
        ADVERSE_ASSET,
        ADVERSE_SIZE_Q,
        START_PRICE,
        0,
    )
    .expect("INV-030 adverse-leg open");

    env.warp_to_slot(2);
    env.push_auth_mark(WINNING_ASSET, 2, WINNING_MARK)
        .expect("INV-030 winning mark");
    env.push_auth_mark(ADVERSE_ASSET, 2, ADVERSE_MARK)
        .expect("INV-030 adverse mark");
    for actor in [MARKET_CRANKER, COUNTERPARTY, WINNER] {
        inv_030_crank_actor_steps(&mut env, actor, 2, &[WINNING_ASSET, ADVERSE_ASSET]);
    }
    assert_eq!(env.primary_portfolio(WINNER).pnl.get(), 50);

    env.trade_no_cpi(
        WINNER,
        COUNTERPARTY,
        ADVERSE_ASSET,
        RISK_INCREASE_Q,
        ADVERSE_MARK,
        0,
    )
    .expect("INV-030 fresh source credit admits the control risk increase");
    let (_, liened_group) = env.primary_market_state();
    assert_source_credit_rates("INV-030 before impairment", &liened_group)
        .expect("independent pre-impairment rate oracle");
    let liened_source = liened_group.source_credit[SOURCE_DOMAIN];
    let liened_bucket = liened_group.source_backing_buckets[SOURCE_DOMAIN];
    assert_eq!(liened_bucket.status, BackingBucketStatusV16::Fresh);
    assert!(liened_source.positive_claim_bound_num > 0);
    assert!(liened_source.valid_liened_backing_num > 0);
    assert!(liened_source.credit_rate_num > 0);
    assert!(liened_source.credit_rate_num <= CREDIT_RATE_SCALE);
    assert_eq!(liened_source.impaired_liened_backing_num, 0);
    let winner_with_lien = env.primary_portfolio(WINNER);
    let account_lien = winner_with_lien
        .source_domains
        .iter()
        .find(|source| source.is_occupied() && source.domain.get() as usize == SOURCE_DOMAIN)
        .expect("INV-030 winner owns the source domain created by public settlement");
    assert_eq!(
        account_lien.source_lien_counterparty_backing_num.get(),
        liened_source.valid_liened_backing_num
    );
    let custody_before_impairment = env.token_amount(env.vault);

    env.warp_to_slot(EXPIRY_SLOT);
    env.push_auth_mark(WINNING_ASSET, EXPIRY_SLOT, EXPIRY_WINNING_MARK)
        .expect("INV-030 expiry-slot winning mark");
    env.push_auth_mark(ADVERSE_ASSET, EXPIRY_SLOT, ADVERSE_MARK)
        .expect("INV-030 expiry-slot adverse mark");
    let observations = inv_030_observations(&env, &[WINNING_ASSET, ADVERSE_ASSET]);
    for _ in 0..16 {
        if env.primary_market_state().1.source_backing_buckets[SOURCE_DOMAIN].status
            == BackingBucketStatusV16::Impaired
        {
            break;
        }
        env.crank(WINNER, EXPIRY_SLOT, observations.clone())
            .expect("INV-030 permissionless expiry normalization must progress");
    }

    let (_, impaired_group) = env.primary_market_state();
    assert_source_credit_rates("INV-030 after impairment", &impaired_group)
        .expect("independent post-impairment rate oracle");
    let impaired_source = impaired_group.source_credit[SOURCE_DOMAIN];
    let impaired_bucket = impaired_group.source_backing_buckets[SOURCE_DOMAIN];
    assert_eq!(
        impaired_bucket.status,
        BackingBucketStatusV16::Impaired,
        "public account refresh did not impair the lapsed live lien"
    );
    assert_eq!(
        impaired_source.positive_claim_bound_num,
        liened_source.positive_claim_bound_num
    );
    assert_eq!(impaired_source.fresh_reserved_backing_num, 0);
    assert_eq!(impaired_source.valid_liened_backing_num, 0);
    assert_eq!(
        impaired_source.impaired_liened_backing_num,
        liened_source.valid_liened_backing_num
    );
    assert_eq!(impaired_source.credit_rate_num, 0);
    assert_eq!(env.token_amount(env.vault), custody_before_impairment);

    let market_before_rejection = env.market_data(false);
    let portfolios_before_rejection = env.all_primary_portfolio_data();
    let ledger_before_rejection = env.backing_domain_ledger_data();
    let tokens_before_rejection = env.all_token_account_data();
    env.begin_public_trace();
    let rejected = env.trade_no_cpi(
        WINNER,
        COUNTERPARTY,
        ADVERSE_ASSET,
        RISK_INCREASE_Q,
        ADVERSE_MARK,
        0,
    );
    assert!(
        rejected.is_err(),
        "INV-030 impaired source credit admitted new risk: {rejected:?}"
    );
    assert_eq!(env.market_data(false), market_before_rejection);
    assert_eq!(
        env.all_primary_portfolio_data(),
        portfolios_before_rejection
    );
    assert_eq!(env.backing_domain_ledger_data(), ledger_before_rejection);
    assert_eq!(env.all_token_account_data(), tokens_before_rejection);
    let (_, after_rejection) = env.primary_market_state();
    assert_eq!(
        after_rejection.source_credit[SOURCE_DOMAIN],
        impaired_source
    );
    assert_source_credit_rates("INV-030 after rejected risk increase", &after_rejection)
        .expect("independent rejected-route rate oracle");

    let trace = env.finish_public_trace();
    assert_eq!(trace.out_of_band_economic_mutations, 0);
    assert_eq!(trace.steps.len(), 1);
    let rejected_step = &trace.steps[0];
    assert!(!rejected_step.succeeded);
    assert_eq!(rejected_step.rejected_exact_writable_rollback, Some(true));
    assert_eq!(rejected_step.rejected_no_program_lamport_delta, Some(true));
    assert!(rejected_step
        .token_deltas
        .iter()
        .all(|(_, delta)| *delta == 0));
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_source_credit_rate_lifecycle_matches_independent_oracle(
        seed in any::<[u8; 32]>(),
        initial_backing in 1u16..=100,
        added_backing in 1u16..=100,
        price_move in 5u8..=20,
    ) {
        let result =
            verify_source_credit_rate_lifecycle(seed, initial_backing, added_backing, price_move);
        prop_assert!(
            result.is_ok(),
            "public source-credit lifecycle diverged from its independent rate oracle: {}",
            result.unwrap_err()
        );
    }
}
