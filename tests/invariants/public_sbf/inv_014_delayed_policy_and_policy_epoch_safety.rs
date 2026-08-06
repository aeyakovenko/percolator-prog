//! INV-014 - Delayed-policy and policy-epoch safety.
//!
//! Normative obligation: Delayed requests remain bounded by the policy and economics the signer authorized.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr325_stale_maintenance_policy_extracts_user_fee`, `v16_program_pr326_stale_liquidation_policy_extracts_user_fee`, `v16_program_pr337_delayed_maintenance_policy_extracts_user_fee`, `v16_program_pr336_delayed_liquidation_policy_extracts_user_fee`, `v16_program_pr338_delayed_trade_fee_policy_cannot_silently_debit_user`, `v16_program_pr340_delayed_fee_redirect_extracts_user_fee`, `v16_program_pr349_delayed_backing_fee_extracts_user_fee`, `v16_program_pr339_reordered_backing_terms_divert_provider_fee`, `v16_program_pr347_stale_policy_cannot_freeze_authenticated_mark`, `v16_program_pr335_delayed_oracle_intents_extract_user_collateral`, `v16_program_pr334_delayed_matcher_enable_rejects_after_revoke`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.
//! PR 338's silent debit is blocked by signed base-fee consent, but the stale policy overwrite is
//! retained as an explicit ordering gap. Redirect-policy replays remain economically exploitable.
//! PR334 is fixed-pin coverage: stale matcher consent rejects, while fresh sequenced consent keeps
//! CPI trading and terminal withdrawals live.

use super::*;

#[test]
fn v16_program_pr325_stale_maintenance_policy_extracts_user_fee() {
    let reproduction = reproduce_maintenance_policy_generation_replay([0x25; 32])
        .unwrap_or_else(|error| panic!("PR 325 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::MaintenancePolicyGenerationReplay
    );
    assert!(reproduction.live_oi_q > 0);
    assert_eq!(reproduction.victim_loss, 580);
    assert_eq!(reproduction.attacker_extraction, 580);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.sync_cu < 1_400_000);
}

#[test]
fn v16_program_pr326_stale_liquidation_policy_extracts_user_fee() {
    let reproduction = reproduce_liquidation_policy_generation_replay([0x26; 32])
        .unwrap_or_else(|error| panic!("PR 326 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::LiquidationPolicyGenerationReplay
    );
    assert!(reproduction.live_oi_q > 0);
    assert_eq!(reproduction.victim_capital_loss, 455);
    assert_eq!(reproduction.attacker_extraction, 455);
    assert_eq!(reproduction.insurance_delta, 0);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.liquidation_cu < 1_400_000);
}

#[test]
fn v16_program_pr337_delayed_maintenance_policy_extracts_user_fee() {
    let reproduction = reproduce_delayed_maintenance_policy_replay([0x37; 32])
        .unwrap_or_else(|error| panic!("PR 337 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::DelayedMaintenancePolicyReplay
    );
    assert!(reproduction.live_oi_q > 0);
    assert_eq!(reproduction.victim_loss, 580);
    assert_eq!(reproduction.attacker_extraction, 580);
    assert_eq!(reproduction.insurance_delta, 0);
    assert!(reproduction.correction_cu < 1_400_000);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.sync_cu < 1_400_000);
}

#[test]
fn v16_program_pr336_delayed_liquidation_policy_extracts_user_fee() {
    let reproduction = reproduce_delayed_liquidation_policy_replay([0x36; 32])
        .unwrap_or_else(|error| panic!("PR 336 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::DelayedLiquidationPolicyReplay
    );
    assert!(reproduction.live_oi_q > 0);
    assert_eq!(reproduction.victim_capital_loss, 455);
    assert_eq!(reproduction.attacker_extraction, 455);
    assert_eq!(reproduction.insurance_delta, 0);
    assert!(reproduction.correction_cu < 1_400_000);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.liquidation_cu < 1_400_000);
}

#[test]
fn v16_program_pr338_delayed_trade_fee_policy_cannot_silently_debit_user() {
    let protection = verify_delayed_trade_fee_policy_nonextraction([0x38; 32])
        .unwrap_or_else(|error| panic!("PR 338 non-extraction protection failed: {error}"));
    assert_eq!(
        protection.blocker,
        KnownBlocker::DelayedTradeFeePolicyReplay
    );
    assert!(protection.stale_policy_landed);
    assert!(protection.stale_trade_rejected);
    assert!(protection.rejected_exact_rollback);
    assert_eq!(protection.victim_loss, 0);
    assert_eq!(protection.attacker_profit, 0);
    assert_eq!(protection.extracted_fee, 0);
    assert!(protection.correction_cu < 1_400_000);
    assert!(protection.replay_cu < 1_400_000);
    assert!(protection.trade_cu < 1_400_000);
    assert!(protection.withdrawal_cu < 1_400_000);
    assert!(protection.token_supply_conserved);
}

#[test]
fn v16_program_pr340_delayed_fee_redirect_extracts_user_fee() {
    let reproduction = reproduce_delayed_fee_redirect_policy_replay([0x40; 32])
        .unwrap_or_else(|error| panic!("PR 340 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::DelayedFeeRedirectPolicyReplay
    );
    assert_eq!(reproduction.victim_loss, 1_000);
    assert_eq!(reproduction.attacker_profit, 1_000);
    assert_eq!(reproduction.extracted_fee, 2_000);
    assert!(reproduction.correction_cu < 1_400_000);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.trade_cu < 1_400_000);
    assert!(reproduction.withdrawal_cu < 1_400_000);
}

#[test]
fn v16_program_pr349_delayed_backing_fee_extracts_user_fee() {
    let reproduction = reproduce_delayed_backing_fee_policy_replay([0x49; 32])
        .unwrap_or_else(|error| panic!("PR 349 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::DelayedBackingFeePolicyReplay
    );
    assert_eq!(reproduction.victim_loss, 75);
    assert_eq!(reproduction.provider_extraction, 75);
    assert_eq!(reproduction.backing_earnings, 75);
    assert!(reproduction.correction_cu < 1_400_000);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.trade_cu < 1_400_000);
    assert!(reproduction.withdrawal_cu < 1_400_000);
}

#[test]
fn v16_program_pr339_reordered_backing_terms_divert_provider_fee() {
    for order in [
        BackingFeeConsentOrder::FundedThenPolicy,
        BackingFeeConsentOrder::PolicyThenTopUp,
    ] {
        let reproduction = reproduce_backing_fee_consent_replay([0x39; 32], order)
            .unwrap_or_else(|error| panic!("PR 339 {order:?} no longer reproduces: {error}"));
        assert_eq!(reproduction.blocker, KnownBlocker::BackingFeeConsentReplay);
        assert_eq!(reproduction.order, order);
        assert_eq!(reproduction.charged_fee, 70);
        assert_eq!(reproduction.provider_loss, 70);
        assert_eq!(reproduction.operator_gain, 70);
        assert!(reproduction.replay_cu < 1_400_000);
        assert!(reproduction.trade_cu < 1_400_000);
        assert!(reproduction.max_cu < 1_400_000);
    }
}

#[test]
fn v16_program_pr347_stale_policy_cannot_freeze_authenticated_mark() {
    let reproduction = reproduce_delayed_resolve_policy_replay([0x47; 32])
        .unwrap_or_else(|error| panic!("PR 347 fixed route failed: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::DelayedResolvePolicyReplay
    );
    assert!(reproduction.unsafe_resolve_rejected);
    assert!(reproduction.rejected_exact_rollback);
    assert!(reproduction.catchup_steps > 0);
    assert!(reproduction.catchup_steps <= 16);
    assert!(reproduction.max_crank_cu < 1_400_000);
    assert_eq!(reproduction.control_price, 110);
    assert_eq!(reproduction.replay_price, reproduction.control_price);
    assert_eq!(reproduction.victim_loss, 0);
    assert_eq!(reproduction.attacker_gain, 0);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.resolve_cu < 1_400_000);
}

#[test]
fn v16_program_pr335_delayed_oracle_intents_extract_user_collateral() {
    for path in [
        DelayedOracleIntentPath::PushAuth,
        DelayedOracleIntentPath::ConfigureAuth,
    ] {
        let reproduction = reproduce_delayed_oracle_intent_replay([0x35; 32], path)
            .unwrap_or_else(|error| panic!("PR 335 {path:?} no longer reproduces: {error}"));
        assert_eq!(
            reproduction.blocker,
            KnownBlocker::DelayedOracleIntentReplay
        );
        assert_eq!(reproduction.path, path);
        assert_eq!(reproduction.victim_loss, 250_000);
        assert_eq!(reproduction.victim_loss, reproduction.beneficiary_gain);
        match path {
            DelayedOracleIntentPath::PushAuth => assert_eq!(reproduction.restored_mark, 50),
            DelayedOracleIntentPath::ConfigureAuth => {
                assert_eq!(reproduction.stale_mark, 50);
                assert_eq!(reproduction.restored_mark, 100);
            }
        }
        assert!(reproduction.replay_cu < 1_400_000);
        assert!(reproduction.max_crank_cu < 1_400_000);
    }
}

#[test]
fn v16_program_pr334_delayed_matcher_enable_rejects_after_revoke() {
    let protection = verify_matcher_mutation_order_safety([0x34; 32])
        .unwrap_or_else(|error| panic!("PR 334 fixed route failed: {error}"));
    assert!(protection.satisfies_invariant(), "{protection:?}");
}
