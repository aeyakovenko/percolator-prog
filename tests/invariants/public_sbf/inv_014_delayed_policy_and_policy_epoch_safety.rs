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
//! the downstream fee, liquidation, backing, oracle, or resolution effect. PR339 is a distinct
//! backing-provider consent problem and remains covered as a counterexample by the stateful suite.
//!
//! The final two tests deliberately remain public counterexamples for the separate market-account
//! reincarnation gap: recreating the market account resets account-local ordering state. They are
//! INV-001/INV-005 work and must not be misreported as closed by same-incarnation sequencing.

use super::*;

#[test]
fn v16_program_same_market_delayed_controls_reject_atomically() {
    let discoveries = discover_superseded_intents([0x14; 32])
        .unwrap_or_else(|error| panic!("INV-014 supersession matrix failed: {error}"));
    assert_eq!(discoveries.len(), SupersededIntentKind::ALL.len());
    for (expected, discovery) in SupersededIntentKind::ALL.into_iter().zip(discoveries) {
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
        assert!(!discovery.is_violation(), "{expected:?} violated INV-014");
    }
}

#[test]
fn v16_program_pr334_delayed_matcher_enable_rejects_after_revoke() {
    let protection = verify_matcher_mutation_order_safety([0x34; 32])
        .unwrap_or_else(|error| panic!("PR 334 fixed route failed: {error}"));
    assert!(protection.satisfies_invariant(), "{protection:?}");
}

#[test]
fn v16_program_pr325_stale_maintenance_policy_extracts_after_market_recreate() {
    let reproduction = reproduce_maintenance_policy_generation_replay([0x25; 32])
        .unwrap_or_else(|error| panic!("PR 325 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::MaintenancePolicyGenerationReplay
    );
    assert_eq!(
        reproduction.new_asset_market_id, reproduction.old_asset_market_id,
        "same-pubkey market recreation resets account-local engine generation state"
    );
    assert!(reproduction.live_oi_q > 0);
    assert_eq!(reproduction.victim_loss, 580);
    assert_eq!(reproduction.attacker_extraction, 580);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.sync_cu < 1_400_000);
}

#[test]
fn v16_program_pr326_stale_liquidation_policy_extracts_after_market_recreate() {
    let reproduction = reproduce_liquidation_policy_generation_replay([0x26; 32])
        .unwrap_or_else(|error| panic!("PR 326 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::LiquidationPolicyGenerationReplay
    );
    assert_eq!(
        reproduction.new_asset_market_id, reproduction.old_asset_market_id,
        "same-pubkey market recreation resets account-local engine generation state"
    );
    assert!(reproduction.live_oi_q > 0);
    assert_eq!(reproduction.victim_capital_loss, 455);
    assert_eq!(reproduction.attacker_extraction, 455);
    assert_eq!(reproduction.insurance_delta, 0);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.liquidation_cu < 1_400_000);
}
