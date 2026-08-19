//! INV-008 - Intent uniqueness and bounded replay.
//!
//! Normative obligation: One retained economic intent can execute at most once across routes and retries.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): the PR343/350/355
//! fixed-pin tests reject stale trade/deposit/withdraw retries exactly and land a newly bound
//! operation; the PR344/351 tests retain the two authority-top-up counterexamples; PR362 and the
//! issue387/389 tests cover generation/position-bound activation, conversion, and reduction.
//! These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

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
fn v16_program_pr344_insurance_top_up_retry_extracts_duplicate() {
    let reproduction = reproduce_insurance_top_up_retry_replay([0x44; 32])
        .unwrap_or_else(|error| panic!("PR 344 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::InsuranceTopUpRetryReplay
    );
    assert_eq!(reproduction.intended_contribution, 50_000);
    assert_eq!(reproduction.duplicate_loss, 50_000);
    assert_eq!(reproduction.operator_extraction, 50_000);
    assert_eq!(reproduction.insured_remainder, 50_000);
    assert!(reproduction.first_cu < 1_400_000);
    assert!(reproduction.replay_cu < 1_400_000);
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
fn v16_program_pr351_backing_top_up_retry_funds_independent_winner() {
    let reproduction = reproduce_backing_top_up_retry_replay([0x35; 32])
        .unwrap_or_else(|error| panic!("PR 351 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::BackingTopUpRetryReplay);
    assert_eq!(reproduction.intended_contribution, 500);
    assert_eq!(reproduction.duplicate_loss, 500);
    assert_eq!(reproduction.beneficiary_extra_payout, 500);
    assert_eq!(reproduction.control_winner_payout, 2_500);
    assert_eq!(reproduction.replay_winner_payout, 3_000);
    assert!(reproduction.replay_cu < 1_400_000);
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
