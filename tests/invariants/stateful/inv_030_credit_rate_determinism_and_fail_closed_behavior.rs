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
//! `v16_program_liened_backing_expiry_route_matrix_preserves_owner_reduction` crosses all four
//! public trade families with both source sides. Every world creates a real counterparty lien,
//! expires that live lien through the public crank, and proves the resulting impaired backing
//! contributes zero credit. Independent stock and encumbrance censuses run after every successful
//! transition, including each expiry-normalization step. Account refresh and a bilateral trade are
//! stale-gated with exact rollback, but the owner-only `RebalanceReduce` route must remove the exact
//! requested exposure within one bounded transaction. This distinguishes a temporarily unavailable
//! matched route from a persistent funded lock; PR214's larger terminal counterexample remains owned
//! by INV-028.
//!
//! Guarantee boundary: this covers deployed serialization and the generated lifecycle. The engine
//! owns the full-width pure arithmetic proof; broader reachability still requires the charter's
//! exhaustive model and all public source-credit mutation routes.

use super::*;
use crate::support::{
    fuzz_model::{
        assert_public_encumbrance_census, assert_public_stock_census, assert_source_credit_rates,
        execute_trade_route,
    },
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

fn assert_inv_030_census(label: &str, env: &V16Svm) {
    assert_public_stock_census(label, env)
        .unwrap_or_else(|error| panic!("{label} stock census failed: {error}"));
    assert_public_encumbrance_census(label, env)
        .unwrap_or_else(|error| panic!("{label} encumbrance census failed: {error}"));
}

fn inv_030_crank_actor_steps(
    env: &mut V16Svm,
    actor: usize,
    slot: u64,
    assets: &[u16],
    label: &str,
) {
    let observations = inv_030_observations(env, assets);
    let mut progressed = false;
    for step in 0..32 {
        match env.crank(actor, slot, observations.clone()) {
            Ok(_) => {
                progressed = true;
                assert_inv_030_census(&format!("{label} crank step {step}"), env);
            }
            Err(error) if progressed && error.contains("Custom(22)") => return,
            Err(error) => panic!("INV-030 actor {actor} crank failed before progress: {error}"),
        }
    }
    assert!(progressed, "INV-030 actor {actor} crank made no progress");
}

fn inv_030_position_for_asset(env: &V16Svm, actor: usize, asset: u16) -> i128 {
    env.primary_portfolio(actor)
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .filter(|leg| leg.active && leg.asset_index == u32::from(asset))
        .map(|leg| leg.basis_pos_q)
        .sum()
}

fn run_liened_backing_expiry_world(route: TradeRoute, winner_long: bool) {
    const WINNER: usize = 0;
    const COUNTERPARTY: usize = 1;
    const MARKET_CRANKER: usize = 4;
    const WINNING_ASSET: u16 = 0;
    const ADVERSE_ASSET: u16 = 1;
    const START_PRICE: u64 = 100;
    const WINNING_MARK: u64 = 105;
    const EXPIRY_WINNING_MARK: u64 = 106;
    const ADVERSE_MARK: u64 = 95;
    const WINNING_SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const ADVERSE_SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const RISK_INCREASE_Q: i128 = 2 * POS_SCALE as i128;
    const BACKING_ATOMS: u128 = 150;
    const EXPIRY_SLOT: u64 = 3;

    let route_index = match route {
        TradeRoute::NoCpi => 0,
        TradeRoute::Cpi => 1,
        TradeRoute::BatchNoCpi => 2,
        TradeRoute::BatchCpi => 3,
    };
    let direction = if winner_long { 1i128 } else { -1i128 };
    let source_domain = if winner_long { 1usize } else { 0usize };
    let winning_mark = if winner_long { WINNING_MARK } else { 95 };
    let expiry_winning_mark = if winner_long { EXPIRY_WINNING_MARK } else { 94 };
    let adverse_mark = if winner_long { ADVERSE_MARK } else { 105 };
    let label = format!("INV-030 {route:?} winner_long={winner_long}");

    let mut env = V16Svm::new(
        [0x30 ^ (route_index << 1) ^ u8::from(winner_long); 32],
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
    assert_inv_030_census(&format!("{label} initialized"), &env);

    env.top_up_backing_bucket(source_domain as u16, BACKING_ATOMS, EXPIRY_SLOT)
        .unwrap_or_else(|error| panic!("{label} fresh backing top-up: {error}"));
    assert_inv_030_census(&format!("{label} backing funded"), &env);
    execute_trade_route(
        &mut env,
        route,
        WINNER,
        COUNTERPARTY,
        WINNING_ASSET,
        direction * WINNING_SIZE_Q,
        START_PRICE,
        0,
    )
    .unwrap_or_else(|error| panic!("{label} winning-leg open: {error}"));
    assert_inv_030_census(&format!("{label} winning leg opened"), &env);
    execute_trade_route(
        &mut env,
        route,
        WINNER,
        COUNTERPARTY,
        ADVERSE_ASSET,
        direction * ADVERSE_SIZE_Q,
        START_PRICE,
        0,
    )
    .unwrap_or_else(|error| panic!("{label} adverse-leg open: {error}"));
    assert_inv_030_census(&format!("{label} adverse leg opened"), &env);

    env.warp_to_slot(2);
    env.push_auth_mark(WINNING_ASSET, 2, winning_mark)
        .unwrap_or_else(|error| panic!("{label} winning mark: {error}"));
    assert_inv_030_census(&format!("{label} winning mark published"), &env);
    env.push_auth_mark(ADVERSE_ASSET, 2, adverse_mark)
        .unwrap_or_else(|error| panic!("{label} adverse mark: {error}"));
    assert_inv_030_census(&format!("{label} adverse mark published"), &env);
    for actor in [MARKET_CRANKER, COUNTERPARTY, WINNER] {
        inv_030_crank_actor_steps(
            &mut env,
            actor,
            2,
            &[WINNING_ASSET, ADVERSE_ASSET],
            &format!("{label} settle actor {actor}"),
        );
    }
    assert_eq!(
        env.primary_portfolio(WINNER).pnl.get(),
        50,
        "{label} paired marks must produce a real source-backed claim"
    );

    execute_trade_route(
        &mut env,
        route,
        WINNER,
        COUNTERPARTY,
        ADVERSE_ASSET,
        direction * RISK_INCREASE_Q,
        adverse_mark,
        0,
    )
    .unwrap_or_else(|error| panic!("{label} fresh source-credit risk increase: {error}"));
    assert_inv_030_census(&format!("{label} lien created"), &env);
    let (_, liened_group) = env.primary_market_state();
    assert_source_credit_rates(&format!("{label} before impairment"), &liened_group)
        .expect("independent pre-impairment rate oracle");
    let liened_source = liened_group.source_credit[source_domain];
    let liened_bucket = liened_group.source_backing_buckets[source_domain];
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
        .find(|source| source.is_occupied() && source.domain.get() as usize == source_domain)
        .expect("INV-030 winner owns the source domain created by public settlement");
    assert_eq!(
        account_lien.source_lien_counterparty_backing_num.get(),
        liened_source.valid_liened_backing_num
    );
    let custody_before_impairment = env.token_amount(env.vault);

    env.warp_to_slot(EXPIRY_SLOT);
    env.push_auth_mark(WINNING_ASSET, EXPIRY_SLOT, expiry_winning_mark)
        .unwrap_or_else(|error| panic!("{label} expiry-slot winning mark: {error}"));
    assert_inv_030_census(&format!("{label} expiry winning mark published"), &env);
    env.push_auth_mark(ADVERSE_ASSET, EXPIRY_SLOT, adverse_mark)
        .unwrap_or_else(|error| panic!("{label} expiry-slot adverse mark: {error}"));
    assert_inv_030_census(&format!("{label} expiry adverse mark published"), &env);
    let observations = inv_030_observations(&env, &[WINNING_ASSET, ADVERSE_ASSET]);
    for step in 0..16 {
        if env.primary_market_state().1.source_backing_buckets[source_domain].status
            == BackingBucketStatusV16::Impaired
        {
            break;
        }
        env.crank(WINNER, EXPIRY_SLOT, observations.clone())
            .unwrap_or_else(|error| {
                panic!("{label} permissionless expiry normalization step {step}: {error}")
            });
        assert_inv_030_census(&format!("{label} expiry crank step {step}"), &env);
    }

    let (_, impaired_group) = env.primary_market_state();
    assert_source_credit_rates(&format!("{label} after impairment"), &impaired_group)
        .expect("independent post-impairment rate oracle");
    let impaired_source = impaired_group.source_credit[source_domain];
    let impaired_bucket = impaired_group.source_backing_buckets[source_domain];
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

    let market_before_refresh = env.market_data(false);
    let portfolios_before_refresh = env.all_primary_portfolio_data();
    let tokens_before_refresh = env.all_token_account_data();
    env.begin_public_trace();
    let refresh = env.crank(COUNTERPARTY, EXPIRY_SLOT, observations.clone());
    let refresh_error = refresh.expect_err("impaired-state account refresh unexpectedly landed");
    assert!(
        refresh_error.contains("Custom(21)")
            || refresh_error.contains("custom program error: 0x15"),
        "{label} account refresh failed for an unrelated reason: {refresh_error}"
    );
    assert_eq!(env.market_data(false), market_before_refresh);
    assert_eq!(env.all_primary_portfolio_data(), portfolios_before_refresh);
    assert_eq!(env.all_token_account_data(), tokens_before_refresh);
    let refresh_trace = env.finish_public_trace();
    refresh_trace
        .validate_public_execution()
        .expect("credit refresh trace must be public and rollback-exact");
    assert_eq!(refresh_trace.out_of_band_economic_mutations, 0);
    assert_eq!(refresh_trace.steps.len(), 1);
    assert!(!refresh_trace.steps[0].succeeded);
    assert_eq!(
        refresh_trace.steps[0].rejected_exact_writable_rollback,
        Some(true)
    );

    let market_before_rejection = env.market_data(false);
    let portfolios_before_rejection = env.all_primary_portfolio_data();
    let ledger_before_rejection = env.backing_domain_ledger_data();
    let tokens_before_rejection = env.all_token_account_data();
    let matcher_before_rejection = env.all_matcher_context_data();
    env.begin_public_trace();
    let rejected = execute_trade_route(
        &mut env,
        route,
        WINNER,
        COUNTERPARTY,
        ADVERSE_ASSET,
        direction * RISK_INCREASE_Q,
        adverse_mark,
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
    assert_eq!(env.all_matcher_context_data(), matcher_before_rejection);
    let (_, after_rejection) = env.primary_market_state();
    assert_eq!(
        after_rejection.source_credit[source_domain],
        impaired_source
    );
    assert_source_credit_rates(
        &format!("{label} after rejected risk increase"),
        &after_rejection,
    )
    .expect("independent rejected-route rate oracle");

    let trace = env.finish_public_trace();
    trace
        .validate_public_execution()
        .expect("credit expiry trace must be public and rollback-exact");
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

    let position_before_owner_reduction = inv_030_position_for_asset(&env, WINNER, ADVERSE_ASSET);
    let funded_capital = env.primary_portfolio(WINNER).capital.get();
    assert_ne!(
        position_before_owner_reduction, 0,
        "{label} stale-route control has no funded exposure"
    );
    assert!(
        funded_capital > 0,
        "{label} stale-route control has no funded capital"
    );

    let market_before_reduction = env.market_data(false);
    let portfolios_before_reduction = env.all_primary_portfolio_data();
    let ledger_before_reduction = env.backing_domain_ledger_data();
    let tokens_before_reduction = env.all_token_account_data();
    let matcher_before_reduction = env.all_matcher_context_data();
    env.begin_public_trace();
    let matched_reduction = execute_trade_route(
        &mut env,
        route,
        WINNER,
        COUNTERPARTY,
        ADVERSE_ASSET,
        -direction * RISK_INCREASE_Q,
        adverse_mark,
        0,
    );
    let matched_reduction_error = matched_reduction
        .expect_err("impaired-state matched reduction unexpectedly bypassed the stale gate");
    assert!(
        matched_reduction_error.contains("Custom(21)")
            || matched_reduction_error.contains("custom program error: 0x15"),
        "{label} matched reduction failed for an unrelated reason: {matched_reduction_error}"
    );
    assert_eq!(env.market_data(false), market_before_reduction);
    assert_eq!(
        env.all_primary_portfolio_data(),
        portfolios_before_reduction
    );
    assert_eq!(env.backing_domain_ledger_data(), ledger_before_reduction);
    assert_eq!(env.all_token_account_data(), tokens_before_reduction);
    assert_eq!(env.all_matcher_context_data(), matcher_before_reduction);
    let matched_trace = env.finish_public_trace();
    matched_trace
        .validate_public_execution()
        .expect("matched credit trace must be public and rollback-exact");
    assert_eq!(matched_trace.out_of_band_economic_mutations, 0);
    assert_eq!(matched_trace.steps.len(), 1);
    assert!(!matched_trace.steps[0].succeeded);
    assert_eq!(
        matched_trace.steps[0].rejected_exact_writable_rollback,
        Some(true)
    );

    let owner_reduction = env
        .rebalance_reduce(
            WINNER,
            ADVERSE_ASSET,
            u128::try_from(RISK_INCREASE_Q).expect("positive reduction size"),
        )
        .unwrap_or_else(|error| panic!("{label} impaired-state owner reduction: {error}"));
    assert!(
        owner_reduction.compute_units < crate::support::v16_svm::TX_CU_LIMIT,
        "{label} owner reduction exceeded the transaction CU limit"
    );
    assert_eq!(
        inv_030_position_for_asset(&env, WINNER, ADVERSE_ASSET),
        position_before_owner_reduction - direction * RISK_INCREASE_Q,
        "{label} owner reduction did not remove the exact requested exposure"
    );
    assert_inv_030_census(&format!("{label} owner risk reduced"), &env);
}

#[test]
fn v16_program_liened_backing_expiry_route_matrix_preserves_owner_reduction() {
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        for winner_long in [false, true] {
            run_liened_backing_expiry_world(route, winner_long);
        }
    }
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
