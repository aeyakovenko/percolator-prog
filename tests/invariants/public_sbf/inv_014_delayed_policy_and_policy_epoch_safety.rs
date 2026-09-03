//! INV-014 - Delayed-policy and policy-epoch safety.
//!
//! Normative obligation: delayed signed controls cannot replace a newer authorized policy or
//! observation. The public LiteSVM matrix signs an old request, commits a distinct newer request,
//! then lands the old bytes. It covers matcher consent, AuthMark, EWMA, Hybrid, Recovery restart,
//! both backing sides, market-init, trade, redirect, liquidation, maintenance, and
//! permissionless-resolve controls.
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

fn inv014_braced_body<'a>(source: &'a str, marker: &str) -> &'a str {
    let start = source
        .find(marker)
        .unwrap_or_else(|| panic!("missing source marker {marker}"));
    let open = source[start..]
        .find('{')
        .map(|offset| start + offset)
        .unwrap_or_else(|| panic!("missing opening brace for {marker}"));
    let mut depth = 0usize;
    for (offset, byte) in source[open..].bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[start..=open + offset];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated source item {marker}")
}

fn inv014_variant_body<'a>(instruction_enum: &'a str, variant: &str) -> &'a str {
    inv014_braced_body(instruction_enum, &format!("{variant} {{"))
}

#[test]
fn v16_program_delayed_control_matrix_is_source_complete() {
    use std::collections::{BTreeMap, BTreeSet};

    let source = include_str!("../../../src/v16_program.rs");
    let instruction_enum = inv014_braced_body(source, "pub enum Instruction {");
    let policy_variants = [
        "UpdateLiquidationFeePolicy",
        "UpdateMaintenanceFeePolicy",
        "UpdateBackingFeePolicy",
        "UpdateTradeFeePolicy",
        "UpdateFeeRedirectPolicy",
        "UpdateMarketInitFeePolicy",
        "ConfigurePermissionlessResolve",
    ];
    let observation_variants = [
        "ConfigureHybridOracle",
        "ConfigureEwmaMark",
        "PushEwmaMark",
        "ConfigureAuthMark",
        "PushAuthMark",
        "RestartAssetOracle",
    ];

    assert_eq!(
        instruction_enum.matches("policy_sequence: u64").count(),
        policy_variants.len(),
        "a policy-sequence field was added or removed without an INV-014 matrix owner"
    );
    assert_eq!(
        instruction_enum
            .matches("observation_sequence: u64")
            .count(),
        observation_variants.len(),
        "an observation-sequence field was added or removed without an INV-014 matrix owner"
    );
    for variant in policy_variants.into_iter().chain(observation_variants) {
        let body = inv014_variant_body(instruction_enum, variant);
        assert!(
            body.contains("authority_epoch: u64"),
            "{variant} must bind the current configured-authority incarnation"
        );
    }
    assert!(
        inv014_variant_body(instruction_enum, "SetMatcherConfig")
            .contains("expected_sequence: u64"),
        "matcher policy must bind its portfolio-local control incarnation"
    );

    let production_variants = policy_variants
        .into_iter()
        .chain(observation_variants)
        .chain(["SetMatcherConfig"])
        .collect::<BTreeSet<_>>();
    let model_variants = SupersededIntentKind::ALL
        .into_iter()
        .map(SupersededIntentKind::instruction_variant)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        model_variants, production_variants,
        "every production delayed-control sequence must have a finding-blind public matrix owner"
    );

    let semantic_counts = SupersededIntentKind::ALL.into_iter().fold(
        BTreeMap::<_, usize>::new(),
        |mut counts, kind| {
            *counts.entry(kind.instruction_variant()).or_default() += 1;
            counts
        },
    );
    assert_eq!(semantic_counts.get("UpdateBackingFeePolicy"), Some(&2));
    assert!(semantic_counts
        .iter()
        .all(|(variant, count)| { *variant == "UpdateBackingFeePolicy" || *count == 1 }));

    let matcher = inv014_braced_body(source, "fn handle_set_matcher_config<'a>(");
    assert!(matcher.contains("state::advance_portfolio_matcher_sequence("));
    for handler in [
        "fn handle_update_market_authority_policy<'a>(",
        "fn handle_update_backing_fee_policy<'a>(",
        "fn handle_update_trade_fee_policy<'a>(",
        "fn handle_configure_permissionless_resolve<'a>(",
        "fn handle_configure_hybrid_oracle<'a>(",
        "fn handle_configure_managed_mark<'a>(",
        "fn handle_push_managed_mark<'a>(",
        "fn handle_restart_asset_oracle<'a>(",
    ] {
        let body = inv014_braced_body(source, handler);
        assert!(
            body.contains("require_authority_epoch_view("),
            "{handler} lost its authority-incarnation check"
        );
        assert!(
            body.contains("advance_control_sequence_view(")
                || body.contains("advance_backing_fee_sequence_view("),
            "{handler} lost its monotonic supersession check"
        );
    }

    let restart = inv014_braced_body(source, "fn handle_restart_asset_oracle<'a>(");
    assert!(restart.contains("require_asset_generation_view("));
    assert!(restart.contains("restart_empty_asset_preserving_insurance_budget_not_atomic("));

    let authority_evidence = include_str!("../cu/inv_005_authority_incarnation_binding.rs");
    assert!(authority_evidence
        .contains("fn v16_program_configured_authority_route_dispositions_are_source_complete("));
    assert!(
        authority_evidence.contains("fn v16_program_authority_epoch_matrix_is_source_complete(")
    );
    let market_evidence = include_str!("inv_007_no_aba_reuse.rs");
    assert!(
        market_evidence.contains("fn v16_wrapper_account_incarnation_census_is_source_complete(")
    );
}

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

#[test]
fn v16_program_fee_redirect_supersession_preserves_terminal_domain_value() {
    let discovery = discover_fee_redirect_supersession([0x39; 32])
        .unwrap_or_else(|error| panic!("fee-redirect terminal world failed: {error}"));
    assert!(
        discovery.certifies_terminal_supersession(),
        "stale fee redirect changed terminal domain value: {discovery:?}"
    );
}

#[test]
fn v16_program_resolve_policy_supersession_preserves_a_complete_funded_exit() {
    for payload_order in SupersessionPayloadOrder::ALL {
        let discovery = discover_resolve_policy_bounded_liveness([0x3a; 32], payload_order)
            .unwrap_or_else(|error| {
                panic!("resolve-policy {payload_order:?} liveness world failed: {error}")
            });
        assert!(
            discovery.certifies_bounded_liveness(),
            "resolve-policy supersession lost its funded exit: {discovery:?}"
        );
    }
}

#[test]
fn v16_program_oracle_supersession_is_bound_to_terminal_value() {
    let discoveries = discover_oracle_supersession_terminal_losses([0x38; 32])
        .unwrap_or_else(|error| panic!("oracle-supersession terminal matrix failed: {error}"));
    assert_eq!(
        discoveries.len(),
        SupersededIntentKind::ORACLE_TERMINAL_CANDIDATES.len()
    );
    for (expected, discovery) in SupersededIntentKind::ORACLE_TERMINAL_CANDIDATES
        .into_iter()
        .zip(&discoveries)
    {
        assert_eq!(discovery.kind, expected);
        assert!(
            discovery.certifies_terminal_supersession(),
            "{expected:?} terminal supersession evidence failed: landed={}, rollback={}, marks={}/{}/{}, payouts={:?}/{:?}/{:?}, replay delta={}/{}/{}, mutation delta={}/{}/{}",
            discovery.stale_control_landed,
            discovery.stale_control_rejected_exact_rollback,
            discovery.control_mark,
            discovery.replay_mark,
            discovery.mutation_mark,
            discovery.control_payouts,
            discovery.replay_payouts,
            discovery.mutation_payouts,
            discovery.replay_victim_loss,
            discovery.replay_counterparty_gain,
            discovery.replay_burn_increase,
            discovery.mutation_victim_loss,
            discovery.mutation_counterparty_gain,
            discovery.mutation_burn_increase,
        );
    }
}
