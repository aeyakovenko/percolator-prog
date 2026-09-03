//! INV-036 - Fee destination and policy-version integrity.
//!
//! Normative obligation: Charged fees reach only the authorized destination under the bound policy version.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_retained_source_fee_caps_bind_every_single_trade_role`, `v16_program_pr224_unsigned_lp_caller_fee_is_ignored`, `v16_program_pr223_unsigned_lp_backing_fee_requires_matcher_consent`, `v16_program_pr314_permissionless_activation_fee_requires_creator_consent`, `v16_program_pr313_cpi_base_fee_requires_lp_consent`, `v16_program_pr310_bilateral_base_fee_requires_fresh_consent`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: PRs 223, 224, 259, 310, 313, and 314 are fixed-pin certifications here.
//! INV-014 separately certifies PR339's backing-provider policy scope; it is intentionally not
//! conflated with this trader-consent certification.

use super::*;
use crate::support::invariant_discovery::{
    discover_source_fee_consent_violations, SourceFeeConsentKind, SourceFeeConsentRole,
};

#[test]
fn v16_program_retained_source_fee_caps_bind_every_single_trade_role() {
    let discoveries = discover_source_fee_consent_violations([0x59; 32])
        .expect("retained source-fee consent matrix");
    assert_eq!(
        discoveries.len(),
        SourceFeeConsentKind::ALL.len() * SourceFeeConsentRole::ALL.len()
    );
    for discovery in &discoveries {
        assert!(!discovery.accepted_unconsented_fee);
        assert_eq!(discovery.lp_capital_debit, 0);
        assert_eq!(discovery.provider_earnings_credit, 0);
        assert_eq!(discovery.extracted_provider_tokens, 0);
        assert!(discovery.compute_units.is_none());
        assert!(!discovery.is_violation());
        discovery
            .public_trace
            .validate_public_execution()
            .expect("source-fee public trace");

        let single_route = matches!(
            discovery.kind,
            SourceFeeConsentKind::NoCpi | SourceFeeConsentKind::Cpi
        );
        assert_eq!(discovery.over_cap_rejected_exact_rollback, single_route);
        assert_eq!(discovery.authorized_retry_landed, single_route);
        if single_route {
            assert!(discovery.authorized_retry_lp_capital_debit > 0);
            assert_eq!(
                discovery.authorized_retry_lp_capital_debit,
                discovery.authorized_retry_provider_earnings_credit
            );
            assert_eq!(
                discovery.authorized_retry_provider_earnings_credit,
                discovery.authorized_retry_extracted_provider_tokens
            );
            assert!(discovery
                .authorized_retry_compute_units
                .is_some_and(|cu| cu < support::v16_svm::TX_CU_LIMIT));
        } else {
            assert_eq!(discovery.authorized_retry_lp_capital_debit, 0);
            assert_eq!(discovery.authorized_retry_provider_earnings_credit, 0);
            assert_eq!(discovery.authorized_retry_extracted_provider_tokens, 0);
            assert!(discovery.authorized_retry_compute_units.is_none());
        }
    }
}

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
    // The zero-cap close pays no backing fee. Its only capital delta is the
    // maintenance debt implied by the authenticated fee-cursor movement.
    assert!(protection.attacker_maintenance_fee > 0);
    assert_eq!(
        protection.attacker_capital_delta,
        -i128::try_from(protection.attacker_maintenance_fee).unwrap()
    );
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
    assert!(protection.stale_policy_rejected);
    assert!(protection.rejected_exact_rollback);
    assert_eq!(protection.unconsented_creator_loss, 0);
    assert_eq!(protection.unconsented_insurance_delta, 0);
    assert_eq!(protection.consented_max_fee, 1);
    assert_eq!(protection.current_fee, 1);
    assert_eq!(protection.charged_fee, 1);
    assert_eq!(protection.insured_fee, 1);
    assert!(protection.asset_active);
    assert!(protection.activation_cu < 1_400_000);
    assert!(protection.token_supply_conserved);
}

#[test]
fn v16_program_pr313_cpi_base_fee_requires_lp_consent() {
    for route in [TradeRoute::Cpi, TradeRoute::BatchCpi] {
        let protection = verify_cpi_base_fee_consent([0x13; 32], route)
            .unwrap_or_else(|error| panic!("PR 313 {route:?} protection failed: {error}"));
        assert_eq!(protection.blocker, KnownBlocker::BilateralBaseFeeConsent);
        assert_eq!(protection.route, route);
        assert_eq!(protection.rejecting_cap_bps, 499);
        assert_eq!(protection.installed_fee_bps, 500);
        assert!(protection.invalid_cap_rejected);
        assert!(protection.invalid_cap_exact_rollback);
        assert!(protection.stale_fill_rejected);
        assert!(protection.stale_fill_exact_rollback);
        assert!(protection.position_epoch_preserved);
        assert_eq!(protection.unconsented_lp_loss, 0);
        assert_eq!(protection.unconsented_insurance_delta, 0);
        assert_eq!(protection.consented_cap_bps, 500);
        assert_eq!(protection.consented_lp_fee, 100_000);
        assert_eq!(protection.consented_insurance_fee, 200_000);
        assert_eq!(protection.total_payout, 200_000_000);
        assert!(protection.open_cu < 1_400_000);
        assert!(protection.close_cu < 1_400_000);
        assert!(protection.max_route_cu < 1_400_000);
        assert!(protection.token_supply_conserved);
    }
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
