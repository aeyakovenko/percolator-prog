//! INV-008 - Intent uniqueness and bounded replay.
//!
//! Normative obligation: One retained economic intent can execute at most once across routes and retries.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): the PR343/350/355
//! fixed-pin tests reject stale trade/deposit/withdraw retries exactly and land a newly bound
//! operation; the PR344/351 tests require both authority-top-up routes to reject stale retries;
//! `v16_program_same_transaction_cross_route_retry_is_atomic_and_exact_once` bundles the direct
//! and domain insurance variants in both orders, while the all-family matrix duplicates each of
//! the eleven retained intents in one transaction. The retained-trade matrix additionally covers
//! all sixteen ordered pairs of single/batch CPI/no-CPI routes from one pre-state. These prove
//! whole-transaction rollback, then prove exactly one standalone request can land; PR362 and the
//! issue387/389 tests cover generation/position-bound activation, conversion, and reduction.
//! These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: PRs 343/344/350/351/355/362 are fixed-pin certifications of the currently
//! deployed retained families, not a claim that absent message fields exist. Successful partial
//! fills have their own INV-009 matrix; retained expiry and aggregate signed budgets remain open
//! schema requirements.

use super::*;
use crate::support::v16_svm::{MarketConfig, V16Svm};

#[test]
fn v16_program_pr343_trade_retry_variants_reject_stale_and_land_fresh() {
    for kind in [
        RetryIntentKind::TradeNoCpi,
        RetryIntentKind::TradeCpi,
        RetryIntentKind::BatchTradeNoCpi,
        RetryIntentKind::BatchTradeCpi,
    ] {
        let protection = discover_intent_retry([0x43; 32], kind)
            .unwrap_or_else(|error| panic!("PR 343 {kind:?} protection failed: {error}"));
        assert!(!protection.accepted_retry);
        assert!(!protection.duplicated_economic_effect);
        assert_eq!(protection.retry_compute_units, None);
        assert!(protection.fresh_compute_units.is_some());
    }
}

#[test]
fn v16_program_pr344_insurance_top_up_retry_rejects_stale_and_lands_fresh() {
    let protection = discover_intent_retry([0x44; 32], RetryIntentKind::InsuranceTopUp)
        .unwrap_or_else(|error| panic!("PR 344 protection failed: {error}"));
    assert!(protection.first_compute_units > 0);
    assert!(!protection.accepted_retry);
    assert!(!protection.duplicated_economic_effect);
    assert_eq!(protection.retry_compute_units, None);
    assert!(protection.fresh_compute_units.is_some());
}

#[test]
fn v16_program_insurance_top_up_routes_share_one_replay_watermark() {
    for direct_first in [false, true] {
        let protection = discover_cross_route_insurance_top_up_retry(
            [0x48 ^ u8::from(direct_first); 32],
            direct_first,
        )
        .unwrap_or_else(|error| panic!("cross-route top-up protection failed: {error}"));
        assert!(!protection.accepted_retry);
        assert!(!protection.duplicated_economic_effect);
        assert_eq!(protection.retry_compute_units, None);
        assert!(protection.fresh_compute_units.is_some());
    }
}

#[test]
fn v16_program_same_transaction_cross_route_retry_is_atomic_and_exact_once() {
    const AUTHORITY: usize = 2;
    const AMOUNT: u128 = 1_000;

    for direct_first in [false, true] {
        let mut env = V16Svm::new([0x58 ^ u8::from(direct_first); 32], MarketConfig::default());
        env.update_asset_authority_from_admin(
            0,
            percolator_prog::processor::ASSET_AUTH_INSURANCE,
            AUTHORITY,
        )
        .expect("install independent insurance authority");

        let direct = env.build_retained_insurance_top_up_for_actor(AUTHORITY, AMOUNT);
        let domain = env.build_retained_insurance_domain_top_up_for_actor(AUTHORITY, 0, AMOUNT);
        let bundled =
            env.build_retained_insurance_top_up_pair_for_actor(AUTHORITY, 0, AMOUNT, direct_first);
        let (first, retry) = if direct_first {
            (direct, domain)
        } else {
            (domain, direct)
        };

        let market_before = env.market_data(false);
        let tokens_before = env.all_token_account_data();
        let supply_before = env.token_supply_observed();
        let source_before = env.token_amount(env.actors[AUTHORITY].source_token);
        let vault_before = env.token_amount(env.vault);
        let accounting_vault_before = env.primary_market_state().1.vault;
        let sequence_before = env.primary_control_sequences(0).insurance_top_up;
        env.begin_public_trace();

        let bundled_error = env
            .land_retained(bundled)
            .expect_err("the second same-watermark instruction must abort the bundle");
        assert!(
            bundled_error.contains("Custom(19)")
                || bundled_error.contains("custom program error: 0x13"),
            "bundle must reject at the stale replay guard: {bundled_error}"
        );
        assert_eq!(env.market_data(false), market_before);
        assert_eq!(env.all_token_account_data(), tokens_before);
        assert_eq!(env.token_supply_observed(), supply_before);
        assert_eq!(
            env.primary_control_sequences(0).insurance_top_up,
            sequence_before,
            "the aborted bundle must not consume the intent watermark"
        );

        env.land_retained(first)
            .expect("one standalone variant must remain executable after bundle rollback");
        assert_eq!(
            env.token_amount(env.actors[AUTHORITY].source_token),
            source_before - AMOUNT as u64
        );
        assert_eq!(env.token_amount(env.vault), vault_before + AMOUNT as u64);
        assert_eq!(
            env.primary_market_state().1.vault,
            accounting_vault_before + AMOUNT
        );
        assert_eq!(
            env.primary_control_sequences(0).insurance_top_up,
            sequence_before + 1
        );
        assert_eq!(env.token_supply_observed(), supply_before);

        let market_after_first = env.market_data(false);
        let tokens_after_first = env.all_token_account_data();
        let retry_error = env
            .land_retained(retry)
            .expect_err("the alternate route must reject after the shared intent lands");
        assert!(
            retry_error.contains("Custom(19)")
                || retry_error.contains("custom program error: 0x13"),
            "cross-route retry must reject at the stale replay guard: {retry_error}"
        );
        assert_eq!(env.market_data(false), market_after_first);
        assert_eq!(env.all_token_account_data(), tokens_after_first);
        assert_eq!(env.token_supply_observed(), supply_before);

        let trace = env.finish_public_trace();
        trace
            .validate_public_execution()
            .expect("retry-order trace must be public and rollback-exact");
        assert_eq!(trace.out_of_band_economic_mutations, 0);
        assert_eq!(trace.steps.len(), 3);
        assert!(!trace.steps[0].succeeded);
        assert!(trace.steps[1].succeeded);
        assert!(!trace.steps[2].succeeded);
        assert_eq!(trace.steps[0].rejected_exact_writable_rollback, Some(true));
        assert_eq!(trace.steps[2].rejected_exact_writable_rollback, Some(true));
    }
}

#[test]
fn v16_program_every_retained_family_is_atomic_when_duplicated_in_one_transaction() {
    let discoveries = discover_same_transaction_intent_retries([0x59; 32])
        .expect("finding-blind same-transaction retry matrix");
    assert_eq!(discoveries.len(), RetryIntentKind::ALL.len());
    for (expected, discovery) in RetryIntentKind::ALL.into_iter().zip(discoveries) {
        assert_eq!(discovery.kind, expected);
        assert!(
            discovery.bundle_rejected,
            "{expected:?} bundle landed twice"
        );
        assert!(
            discovery.bundle_exact_rollback,
            "{expected:?} bundle rejection committed an economic mutation"
        );
        assert!(
            discovery.standalone_mutated && discovery.standalone_compute_units < 1_400_000,
            "{expected:?} did not retain one bounded standalone execution: {discovery:?}"
        );
        assert!(
            discovery.duplicate_rejected && discovery.duplicate_exact_rollback,
            "{expected:?} standalone execution did not consume the duplicate exactly: {discovery:?}"
        );
        assert!(
            discovery.token_supply_conserved,
            "{expected:?} replay matrix changed SPL supply"
        );
    }
}

#[test]
fn v16_program_retained_trade_intent_is_exact_once_across_every_route_pair() {
    let discoveries = discover_cross_route_trade_intent_retries([0x5a; 32])
        .expect("finding-blind cross-route retained-trade matrix");
    assert_eq!(discoveries.len(), 16);
    assert_eq!(
        discoveries
            .iter()
            .filter(|discovery| discovery.first_route != discovery.duplicate_route)
            .count(),
        12,
        "matrix must contain twelve real route switches and four diagonal controls"
    );
    for discovery in discoveries {
        assert!(
            discovery.bundle_rejected && discovery.bundle_exact_rollback,
            "cross-route bundle was not atomic: {discovery:?}"
        );
        assert!(
            discovery.standalone_compute_units < 1_400_000,
            "standalone route exceeded the transaction limit: {discovery:?}"
        );
        assert!(
            discovery.duplicate_rejected && discovery.duplicate_exact_rollback,
            "alternate route did not consume the same intent: {discovery:?}"
        );
        assert!(
            discovery.exact_bilateral_position
                && discovery.exact_open_interest
                && discovery.token_supply_conserved,
            "cross-route retry changed trade economics: {discovery:?}"
        );
    }
}

#[test]
fn v16_program_pr362_activation_retry_rejects_after_generation_consumed() {
    let discoveries = discover_intent_retries([0x62; 32])
        .unwrap_or_else(|error| panic!("PR 362 protection probe failed: {error}"));
    let activation = discoveries
        .iter()
        .find(|discovery| discovery.kind == RetryIntentKind::AssetActivation)
        .expect("asset activation is in the retained-intent matrix");

    assert!(activation.first_compute_units > 0);
    assert!(!activation.accepted_retry);
    assert!(!activation.duplicated_economic_effect);
    assert_eq!(activation.retry_compute_units, None);
    assert!(activation.fresh_compute_units.is_some());
}

#[test]
fn v16_program_pr351_backing_top_up_retry_rejects_stale_and_lands_fresh() {
    let protection = discover_intent_retry([0x35; 32], RetryIntentKind::BackingTopUp)
        .unwrap_or_else(|error| panic!("PR 351 protection failed: {error}"));
    assert!(protection.first_compute_units > 0);
    assert!(!protection.accepted_retry);
    assert!(!protection.duplicated_economic_effect);
    assert_eq!(protection.retry_compute_units, None);
    assert!(protection.fresh_compute_units.is_some());
}

#[test]
fn v16_program_pr350_deposit_retry_rejects_stale_and_lands_fresh() {
    let protection = discover_intent_retry([0x50; 32], RetryIntentKind::Deposit)
        .unwrap_or_else(|error| panic!("PR 350 protection failed: {error}"));
    assert!(!protection.accepted_retry);
    assert!(!protection.duplicated_economic_effect);
    assert_eq!(protection.retry_compute_units, None);
    assert!(protection.fresh_compute_units.is_some());
}

#[test]
fn v16_program_pr355_withdrawal_retry_rejects_stale_and_lands_fresh() {
    let protection = discover_intent_retry([0x55; 32], RetryIntentKind::Withdraw)
        .unwrap_or_else(|error| panic!("PR 355 protection failed: {error}"));
    assert!(!protection.accepted_retry);
    assert!(!protection.duplicated_economic_effect);
    assert_eq!(protection.retry_compute_units, None);
    assert!(protection.fresh_compute_units.is_some());
}

#[test]
fn v16_program_issue387_stale_conversion_rejects_without_redirecting_later_earnings() {
    let protection = verify_convert_retry_replay_protection([0x87; 32])
        .unwrap_or_else(|error| panic!("conversion retry protection failed: {error}"));
    assert!(protection.released_pnl > 0);
    assert!(protection.stale_retry_rejected);
    assert!(protection.rejected_exact_rollback);
    assert!(protection.fresh_intent_landed);
    assert_eq!(
        protection.control_victim_payout,
        protection.replay_victim_payout
    );
    assert_eq!(protection.cranker_extraction, 0);
    assert!(protection.sync_cu < 1_400_000);
    assert!(protection.max_cu < 1_400_000);
}

#[test]
fn v16_program_issue389_rebalance_retry_rejects_exactly_on_current_pin() {
    let discoveries = discover_intent_retries([0x89; 32])
        .unwrap_or_else(|error| panic!("rebalance retry probe failed: {error}"));
    let rebalance = discoveries
        .iter()
        .find(|discovery| discovery.kind == RetryIntentKind::RebalanceReduce)
        .expect("rebalance retry route is in the complete retained-intent matrix");

    assert!(rebalance.first_compute_units > 0);
    assert!(!rebalance.accepted_retry);
    assert!(!rebalance.duplicated_economic_effect);
    assert_eq!(rebalance.retry_compute_units, None);
    assert!(rebalance.fresh_compute_units.is_some());
}
