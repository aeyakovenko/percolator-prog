//! INV-036 - Fee destination and policy-version integrity.
//!
//! Normative obligation: Charged fees reach only the authorized destination under the bound policy version.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr224_unsigned_lp_caller_fee_is_ignored`, `v16_program_pr223_unsigned_lp_backing_fee_is_withdrawable`, `v16_program_pr314_activation_fee_consent_extracts_unsigned_increase`, `v16_program_pr310_bilateral_base_fee_consent_extracts_victim_fee`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.
//! PR 224 is a fixed-pin assertion here; the remaining finding-specific tests are counterexamples.

use super::*;

#[test]
fn v16_program_pr224_unsigned_lp_caller_fee_is_ignored() {
    for route in [TradeRoute::Cpi, TradeRoute::BatchCpi] {
        let protection = verify_cpi_caller_fee_protection([0x24; 32], route)
            .unwrap_or_else(|error| panic!("PR 224 {route:?} protection failed: {error}"));
        assert_eq!(protection.blocker, KnownBlocker::CpiCallerFeeSiphon);
        assert_eq!(protection.route, route);
        assert_eq!(protection.requested_fee_bps, 10_000);
        assert_eq!(protection.attacker_profit, 0);
        assert_eq!(protection.lp_loss, 0);
        assert_eq!(protection.withdrawable_insurance, 0);
        assert!(protection.insurance_withdraw_rejected);
        assert!(protection.rejected_exact_rollback);
        assert_eq!(protection.total_payout, 2_000_000);
        assert!(protection.token_supply_conserved);
        assert!(protection.max_trade_cu < support::v16_svm::TX_CU_LIMIT);
    }
}

#[test]
fn v16_program_pr223_unsigned_lp_backing_fee_is_withdrawable() {
    let reproduction = reproduce_cpi_backing_fee_siphon([0x23; 32])
        .unwrap_or_else(|error| panic!("PR 223 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::CpiBackingFeeSiphon);
    assert_eq!(reproduction.lp_capital_loss, reproduction.provider_earnings);
    assert_eq!(
        reproduction.provider_earnings,
        u128::from(reproduction.extracted_tokens)
    );
    assert_eq!(reproduction.attacker_capital_delta, 0);
}

#[test]
fn v16_program_pr314_activation_fee_consent_extracts_unsigned_increase() {
    let reproduction = reproduce_activation_fee_consent([0x14; 32])
        .unwrap_or_else(|error| panic!("PR 314 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::ActivationFeeConsent);
    assert_eq!(reproduction.advertised_fee, 1);
    assert_eq!(reproduction.charged_fee, 1_000);
    assert_eq!(reproduction.unexpected_loss, 999);
    assert_eq!(reproduction.beneficiary_extraction, 999);
    assert_eq!(reproduction.insured_remainder, 1);
    assert!(reproduction.policy_replay_cu < 1_400_000);
    assert!(reproduction.activation_cu < 1_400_000);
}

#[test]
fn v16_program_pr310_bilateral_base_fee_consent_extracts_victim_fee() {
    for route in [TradeRoute::NoCpi, TradeRoute::BatchNoCpi] {
        let reproduction = reproduce_bilateral_base_fee_consent([0x10; 32], route)
            .unwrap_or_else(|error| panic!("PR 310 {route:?} no longer reproduces: {error}"));
        assert_eq!(reproduction.blocker, KnownBlocker::BilateralBaseFeeConsent);
        assert_eq!(reproduction.route, route);
        assert_eq!(reproduction.signed_fee_bps, 0);
        assert_eq!(reproduction.installed_fee_bps, 500);
        assert_eq!(reproduction.victim_loss, 100_000);
        assert_eq!(reproduction.beneficiary_profit, 100_000);
        assert_eq!(reproduction.insurance_extraction, 200_000);
        assert_eq!(reproduction.total_payout, 200_000_000);
        assert!(reproduction.open_cu < 1_400_000);
        assert!(reproduction.close_cu < 1_400_000);
    }
}
