//! INV-036 - Fee destination and policy-version integrity.
//!
//! Normative obligation: Charged fees reach only the authorized destination under the bound policy version.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr224_unsigned_lp_caller_fee_is_ignored`, `v16_program_pr223_unsigned_lp_backing_fee_requires_matcher_consent`, `v16_program_pr314_permissionless_activation_fee_requires_creator_consent`, `v16_program_pr310_bilateral_base_fee_requires_fresh_consent`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.
//! PRs 223, 224, 310, and 314 are fixed-pin assertions here.

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
fn v16_program_pr223_unsigned_lp_backing_fee_requires_matcher_consent() {
    let protection = verify_cpi_backing_fee_consent([0x23; 32])
        .unwrap_or_else(|error| panic!("PR 223 protection failed: {error}"));
    assert_eq!(protection.blocker, KnownBlocker::CpiBackingFeeSiphon);
    assert_eq!(protection.matcher_cap_bps, 5_000);
    assert!(protection.rejected_without_consent);
    assert!(protection.rejected_exact_rollback);
    assert_eq!(protection.unconsented_provider_earnings, 0);
    assert_eq!(protection.lp_capital_loss, protection.provider_earnings);
    assert!(protection.provider_earnings > 0);
    assert_eq!(
        protection.provider_earnings,
        u128::from(protection.extracted_tokens)
    );
    assert_eq!(protection.attacker_capital_delta, 0);
    assert!(protection.zero_cap_risk_reduction_landed);
    assert!(protection.max_route_cu < support::v16_svm::TX_CU_LIMIT);
    assert!(protection.token_supply_conserved);
}

#[test]
fn v16_program_pr314_permissionless_activation_fee_requires_creator_consent() {
    let protection = verify_activation_fee_consent([0x14; 32])
        .unwrap_or_else(|error| panic!("PR 314 protection failed: {error}"));
    assert_eq!(protection.blocker, KnownBlocker::ActivationFeeConsent);
    assert_eq!(protection.signed_max_fee, 1);
    assert_eq!(protection.installed_unauthorized_fee, 1_000);
    assert!(protection.stale_activation_rejected);
    assert!(protection.rejected_exact_rollback);
    assert_eq!(protection.unconsented_creator_loss, 0);
    assert_eq!(protection.unconsented_insurance_delta, 0);
    assert_eq!(protection.consented_max_fee, 1_000);
    assert_eq!(protection.current_fee, 7);
    assert_eq!(protection.charged_fee, 7);
    assert_eq!(protection.insured_fee, 7);
    assert!(protection.asset_active);
    assert!(protection.policy_replay_cu < 1_400_000);
    assert!(protection.activation_cu < 1_400_000);
    assert!(protection.token_supply_conserved);
}

#[test]
fn v16_program_pr310_bilateral_base_fee_requires_fresh_consent() {
    for route in [TradeRoute::NoCpi, TradeRoute::BatchNoCpi] {
        let protection = verify_bilateral_base_fee_consent([0x10; 32], route)
            .unwrap_or_else(|error| panic!("PR 310 {route:?} protection failed: {error}"));
        assert_eq!(protection.blocker, KnownBlocker::BilateralBaseFeeConsent);
        assert_eq!(protection.route, route);
        assert_eq!(protection.signed_fee_bps, 0);
        assert_eq!(protection.installed_fee_bps, 500);
        assert!(protection.stale_open_rejected);
        assert!(protection.stale_close_rejected);
        assert!(protection.rejected_exact_rollback);
        assert_eq!(protection.unconsented_victim_loss, 0);
        assert_eq!(protection.unconsented_insurance_delta, 0);
        assert_eq!(protection.consented_victim_fee, 100_000);
        assert_eq!(protection.consented_insurance_fee, 200_000);
        assert_eq!(protection.total_payout, 200_000_000);
        assert!(protection.open_cu < 1_400_000);
        assert!(protection.close_cu < 1_400_000);
        assert!(protection.token_supply_conserved);
    }
}
