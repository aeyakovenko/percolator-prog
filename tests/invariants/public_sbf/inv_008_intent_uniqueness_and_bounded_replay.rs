//! INV-008 - Intent uniqueness and bounded replay.
//!
//! Normative obligation: One retained economic intent can execute at most once across routes and retries.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr343_trade_retry_variants_extract_value_on_every_route`, `v16_program_pr344_insurance_top_up_retry_extracts_duplicate`, `v16_program_pr362_activation_retry_extracts_duplicate_fee`, `v16_program_pr351_backing_top_up_retry_funds_independent_winner`, `v16_program_pr350_deposit_retry_funds_independent_winner`, `v16_program_pr355_withdrawal_retry_liquidates_fresh_risk`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_pr343_trade_retry_variants_extract_value_on_every_route() {
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        let reproduction = reproduce_trade_retry_replay([0x43; 32], route)
            .unwrap_or_else(|error| panic!("PR 343 {route:?} no longer reproduces: {error}"));
        assert_eq!(reproduction.blocker, KnownBlocker::TradeRetryReplay);
        assert_eq!(reproduction.route, route);
        assert_eq!(
            reproduction.victim_extra_loss,
            reproduction.attacker_extra_payout
        );
        assert!(reproduction.victim_extra_loss > 0);
        assert_eq!(
            reproduction.control_total_payout,
            reproduction.replay_total_payout
        );
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
fn v16_program_pr362_activation_retry_extracts_duplicate_fee() {
    let reproduction = reproduce_activation_retry_replay([0x62; 32])
        .unwrap_or_else(|error| panic!("PR 362 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::ActivationRetryReplay);
    assert_ne!(reproduction.first_market_id, reproduction.replay_market_id);
    assert_eq!(reproduction.intended_fee, 500);
    assert_eq!(reproduction.duplicate_loss, 500);
    assert_eq!(reproduction.beneficiary_extraction, 500);
    assert_eq!(reproduction.insured_remainder, 500);
    assert!(reproduction.replay_cu < 1_400_000);
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
fn v16_program_pr350_deposit_retry_funds_independent_winner() {
    let reproduction = reproduce_deposit_retry_replay([0x50; 32])
        .unwrap_or_else(|error| panic!("PR 350 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::DepositRetryReplay);
    assert_eq!(reproduction.intended_contribution, 500);
    assert_eq!(reproduction.duplicate_loss, 500);
    assert_eq!(reproduction.beneficiary_extra_payout, 500);
    assert_eq!(reproduction.control_winner_payout, 2_500);
    assert_eq!(reproduction.replay_winner_payout, 3_000);
    assert!(reproduction.replay_cu < 1_400_000);
}

#[test]
fn v16_program_pr355_withdrawal_retry_liquidates_fresh_risk() {
    let reproduction = reproduce_withdrawal_retry_liquidation([0x55; 32])
        .unwrap_or_else(|error| panic!("PR 355 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::WithdrawalRetryLiquidation
    );
    assert_eq!(reproduction.intended_withdrawal, 50_000_000);
    assert_eq!(reproduction.duplicate_withdrawal, 50_000_000);
    assert!(reproduction.restored_equity_surplus > 0);
    assert_eq!(reproduction.cranker_reward, 7_917);
    assert_eq!(reproduction.extracted_reward, 7_917);
    assert!(reproduction.replay_cu < 1_400_000);
}
