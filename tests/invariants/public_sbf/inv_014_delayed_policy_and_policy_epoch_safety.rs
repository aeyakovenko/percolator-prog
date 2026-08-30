//! INV-014 - Delayed-policy and policy-epoch safety.
//!
//! Normative obligation: delayed signed controls cannot replace a newer authorized policy or
//! observation. The public LiteSVM matrix signs an old request, commits a distinct newer request,
//! then lands the old bytes. It covers matcher consent, AuthMark, EWMA, Hybrid, both backing sides,
//! market-init, trade, redirect, liquidation, maintenance, and permissionless-resolve controls.
//! Rejection is checked against a complete economic-account fingerprint, so the sequence guard
//! cannot partially consume state. The fresh mutation in every case is the nonvacuous control.
//!
//! PRs 335/336/337/338/340/347/349 supplied the original public economic counterexamples.
//! This fixed-pin matrix fails on those vulnerable handlers before their stale overwrite can reach
//! the downstream fee, liquidation, backing, oracle, or resolution effect. The independent PR339
//! matrix additionally crosses both backing-policy/top-up landing orders, requires exact stale
//! rejection, and traces a nonzero authorized fee to the provider-selected SPL destination.
//!
//! Whole-market reuse is separately prohibited by INV-001/INV-007's persistent tombstone; this
//! file therefore owns only same-incarnation ordering and policy-epoch behavior.

use super::*;
use crate::support::invariant_discovery::{
    discover_backing_provider_consent_violations, BackingProviderConsentOrder,
};
use crate::support::v16_svm::{MarketConfig, V16Svm};
use percolator::BOUND_SCALE;

#[test]
fn v16_program_backing_provider_fee_terms_survive_both_landing_orders() {
    let discoveries = discover_backing_provider_consent_violations([0x39; 32])
        .unwrap_or_else(|error| panic!("INV-014 backing-provider matrix failed: {error}"));
    assert_eq!(discoveries.len(), BackingProviderConsentOrder::ALL.len());
    for (expected, discovery) in BackingProviderConsentOrder::ALL
        .into_iter()
        .zip(&discoveries)
    {
        assert_eq!(discovery.order, expected);
        assert!(!discovery.is_violation(), "{expected:?} violated INV-014");
        assert!(
            discovery.satisfies_invariant(),
            "{expected:?} fixed-pin control was vacuous: {discovery:?}"
        );
    }
}

#[test]
fn v16_program_backing_provider_exit_unfreezes_policy_change() {
    const DOMAIN: u16 = 1;
    const PRINCIPAL: u128 = 500;
    let mut env = V16Svm::new([0x3a; 32], MarketConfig::default());
    let supply_before = env.token_supply_observed();

    env.update_backing_fee_policy(DOMAIN, 5_000, 0)
        .expect("install provider-approved fee terms");
    env.top_up_backing_bucket(DOMAIN, PRINCIPAL, 100)
        .expect("fund under provider-approved terms");

    let market_before = env.market_data(false);
    let vault_before = env.token_amount(env.vault);
    let source_before = env.token_amount(env.provider_source_token);
    let rejected = env.update_backing_fee_policy(DOMAIN, 5_000, 10_000);
    assert!(
        rejected.is_err(),
        "live provider principal must freeze economic fee-term changes"
    );
    assert_eq!(env.market_data(false), market_before);
    assert_eq!(env.token_amount(env.vault), vault_before);
    assert_eq!(env.token_amount(env.provider_source_token), source_before);

    env.update_backing_fee_policy(DOMAIN, 5_000, 0)
        .expect("funded provider domain permits a sequence-only policy refresh");
    assert_eq!(
        env.backing_fee_policy(DOMAIN),
        (5_000, 0),
        "consent guard must not freeze an economically unchanged policy"
    );

    env.withdraw_backing_bucket(DOMAIN, PRINCIPAL)
        .expect("provider exits its unencumbered principal");
    env.update_backing_fee_policy(DOMAIN, 5_000, 10_000)
        .expect("empty provider domain permits a new fee policy");
    assert_eq!(env.backing_fee_policy(DOMAIN), (5_000, 10_000));
    env.top_up_backing_bucket(DOMAIN, PRINCIPAL, 100)
        .expect("provider can fund under the newly visible terms");
    assert_eq!(
        env.primary_market_state().1.source_backing_buckets[DOMAIN as usize]
            .fresh_unliened_backing_num,
        PRINCIPAL * BOUND_SCALE
    );
    assert_eq!(env.token_supply_observed(), supply_before);
}

#[test]
fn v16_program_same_market_delayed_controls_reject_atomically() {
    let discoveries = discover_superseded_intents([0x14; 32])
        .unwrap_or_else(|error| panic!("INV-014 supersession matrix failed: {error}"));
    assert_eq!(discoveries.len(), SupersededIntentKind::ALL.len());
    for (expected, discovery) in SupersededIntentKind::ALL.into_iter().zip(&discoveries) {
        assert_eq!(discovery.kind, expected);
        assert!(
            !discovery.accepted_stale_intent,
            "{expected:?} accepted stale signed bytes"
        );
        assert!(
            !discovery.overwrote_newer_state,
            "{expected:?} overwrote the newer state"
        );
        assert_eq!(
            discovery.compute_units, None,
            "{expected:?} unexpectedly committed"
        );
        assert!(
            discovery.fresh_intent_landed,
            "{expected:?} current-sequence control did not land"
        );
        assert!(
            discovery.fresh_mutated_economic_state,
            "{expected:?} current-sequence control was vacuous"
        );
        assert!(
            discovery.fresh_compute_units.is_some(),
            "{expected:?} current-sequence control needs a successful CU result"
        );
        assert!(!discovery.is_violation(), "{expected:?} violated INV-014");
    }

    // Fixed-pin certification maps the finding-agnostic controls to the dated holdout roster.
    // Oracle and backing rows require every semantic lane owned by the same sequence domain.
    let certifications: &[(u16, &[SupersededIntentKind])] = &[
        (334, &[SupersededIntentKind::MatcherConfig]),
        (
            335,
            &[
                SupersededIntentKind::PushAuthMark,
                SupersededIntentKind::ConfigureAuthMark,
                SupersededIntentKind::PushEwmaMark,
                SupersededIntentKind::ConfigureEwmaMark,
                SupersededIntentKind::ConfigureHybridOracle,
            ],
        ),
        (336, &[SupersededIntentKind::LiquidationFeePolicy]),
        (337, &[SupersededIntentKind::MaintenanceFeePolicy]),
        (338, &[SupersededIntentKind::TradeFeePolicy]),
        (340, &[SupersededIntentKind::FeeRedirectPolicy]),
        (347, &[SupersededIntentKind::ResolvePolicy]),
        (
            349,
            &[
                SupersededIntentKind::BackingFeePolicy,
                SupersededIntentKind::BackingFeePolicyShort,
            ],
        ),
    ];
    for (pr, kinds) in certifications {
        for kind in *kinds {
            let evidence = discoveries
                .iter()
                .find(|discovery| discovery.kind == *kind)
                .unwrap_or_else(|| panic!("PR {pr}: missing {kind:?} sequence evidence"));
            assert!(
                !evidence.accepted_stale_intent && !evidence.overwrote_newer_state,
                "PR {pr}: {kind:?} stale control must reject atomically",
            );
            assert!(
                evidence.fresh_intent_landed && evidence.fresh_mutated_economic_state,
                "PR {pr}: {kind:?} current-sequence control must remain live",
            );
        }
    }
}

#[test]
fn v16_program_pr334_delayed_matcher_enable_rejects_after_revoke() {
    let protection = verify_matcher_mutation_order_safety([0x34; 32])
        .unwrap_or_else(|error| panic!("PR 334 fixed route failed: {error}"));
    assert!(protection.satisfies_invariant(), "{protection:?}");
}

#[test]
fn v16_program_revoked_matcher_cannot_recover_terminal_value_with_stale_consent() {
    let discovery = discover_matcher_revocation_terminal_loss([0x35; 32])
        .unwrap_or_else(|error| panic!("matcher-revocation terminal world failed: {error}"));
    assert!(
        discovery.certifies_revocation_and_bounded_exit(),
        "stale matcher consent changed LP terminal value: {discovery:?}"
    );
}

#[test]
fn v16_program_maintenance_share_supersession_changes_attribution_not_payer_value() {
    let discovery = discover_maintenance_share_supersession([0x37; 32])
        .unwrap_or_else(|error| panic!("maintenance-share terminal world failed: {error}"));
    assert!(
        discovery.certifies_attribution_only(),
        "maintenance share changed fee or payer terminal value: {discovery:?}"
    );
}

#[test]
fn v16_program_liquidation_share_supersession_changes_attribution_not_victim_value() {
    let discovery = discover_liquidation_share_supersession([0x36; 32])
        .unwrap_or_else(|error| panic!("liquidation-share terminal world failed: {error}"));
    assert!(
        discovery.certifies_attribution_only(),
        "liquidation share changed fee or victim terminal value: {discovery:?}"
    );
}
