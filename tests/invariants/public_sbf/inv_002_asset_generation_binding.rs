//! INV-002 - Asset generation binding.
//!
//! Normative obligation: Asset-scoped consent cannot cross retirement, slot reuse, or asset-generation changes.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr231_asset_generation_replay_rejects_on_every_route`, `v16_program_pr279_stale_insurance_top_up_rejects_across_asset_generation`, `v16_program_pr279_asset_zero_top_up_rejects_after_restart`, `v16_program_pr321_stale_backing_top_up_rejects_across_asset_generation`, `v16_program_pr328_stale_withdrawal_drains_replacement_reserve`, `v16_program_pr318_stale_backing_fee_extracts_victim_capital`, `v16_program_pr311_stale_resolve_crystallizes_replacement_loss`, `v16_program_pr275_stale_mark_pushes_reject_across_asset_generation`, `v16_program_pr277_pr322_stale_oracle_controls_reject_across_asset_generation`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant. Oracle controls are
//! retained with `u64::MAX` sequences so sequence reset or forward-gap ordering cannot make the
//! result pass; the restart route deliberately reuses the same authorized asset admin in both
//! generations and proves a fresh generation-B restart remains live.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;
use crate::support::v16_svm::{MarketConfig, V16Svm};
use percolator::AssetLifecycleV16;
use percolator_prog::error::PercolatorError;

#[test]
fn v16_program_pr231_asset_generation_replay_rejects_on_every_route() {
    for kind in [
        AssetIntentKind::TradeNoCpi,
        AssetIntentKind::TradeCpi,
        AssetIntentKind::BatchTradeNoCpi,
        AssetIntentKind::BatchTradeCpi,
    ] {
        let protection = discover_asset_generation_replay([0x31; 32], kind)
            .unwrap_or_else(|error| panic!("PR 231 {kind:?} protection failed: {error}"));
        assert_eq!(protection.kind, kind);
        assert!(protection.new_asset_id > protection.old_asset_id);
        assert!(!protection.accepted_stale_intent);
        assert!(!protection.mutated_economic_state);
        assert_eq!(protection.compute_units, None);
        assert!(protection.rejection_was_generation_mismatch);
        assert!(protection.fresh_intent_landed);
        assert!(protection.fresh_intent_mutated_economic_state);
    }
}

#[test]
fn v16_program_pr279_stale_insurance_top_up_rejects_across_asset_generation() {
    let protection = discover_asset_generation_replay([0x79; 32], AssetIntentKind::InsuranceTopUp)
        .unwrap_or_else(|error| panic!("PR 279 protection failed: {error}"));
    assert_eq!(protection.kind, AssetIntentKind::InsuranceTopUp);
    assert!(protection.new_asset_id > protection.old_asset_id);
    assert!(!protection.accepted_stale_intent);
    assert!(!protection.mutated_economic_state);
    assert_eq!(protection.compute_units, None);
    assert!(protection.rejection_was_generation_mismatch);
    assert!(protection.fresh_intent_landed);
    assert!(protection.fresh_intent_mutated_economic_state);
}

#[test]
fn v16_program_pr321_stale_backing_top_up_rejects_across_asset_generation() {
    let protection = discover_asset_generation_replay([0x21; 32], AssetIntentKind::BackingTopUp)
        .unwrap_or_else(|error| panic!("PR 321 protection failed: {error}"));
    assert_eq!(protection.kind, AssetIntentKind::BackingTopUp);
    assert!(protection.new_asset_id > protection.old_asset_id);
    assert!(!protection.accepted_stale_intent);
    assert!(!protection.mutated_economic_state);
    assert_eq!(protection.compute_units, None);
    assert!(protection.rejection_was_generation_mismatch);
    assert!(protection.fresh_intent_landed);
    assert!(protection.fresh_intent_mutated_economic_state);
}

#[test]
fn v16_program_pr279_asset_zero_top_up_rejects_after_restart() {
    const ASSET: u16 = 0;
    const REUSED_INSURANCE_AUTHORITY: usize = 2;
    const AMOUNT: u128 = 1_000;
    const PRICE: u64 = 100;

    let mut env = V16Svm::new([0x7a; 32], MarketConfig::default());
    env.configure_permissionless_resolve(100, 1)
        .expect("configure finite recovery delay");
    env.update_asset_authority_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_INSURANCE,
        REUSED_INSURANCE_AUTHORITY,
    )
    .expect("install generation-A insurance authority");
    let old_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    let retained =
        env.build_retained_insurance_top_up_for_actor(REUSED_INSURANCE_AUTHORITY, AMOUNT);

    env.warp_to_slot(1);
    env.shutdown_asset(ASSET, 1)
        .expect("put generation A into Recovery");
    env.warp_to_slot(2);
    env.restart_asset_oracle(ASSET, 2, PRICE)
        .expect("restart empty asset 0 into generation B");
    let new_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    assert!(new_market_id > old_market_id);

    let market_before = env.market_data(false);
    let tokens_before = env.all_token_account_data();
    let supply_before = env.token_supply_observed();
    let error = env
        .land_retained(retained)
        .expect_err("generation-A top-up must not fund generation B");
    let expected = format!(
        "Custom({})",
        PercolatorError::AssetGenerationMismatch as u32
    );
    assert!(
        error.contains(&expected),
        "stale top-up must return {expected}, got {error}"
    );
    assert_eq!(env.market_data(false), market_before);
    assert_eq!(env.all_token_account_data(), tokens_before);
    assert_eq!(env.token_supply_observed(), supply_before);

    let source = env.actors[REUSED_INSURANCE_AUTHORITY].source_token;
    let source_before = env.token_amount(source);
    let vault_before = env.token_amount(env.vault);
    let fresh = env.build_retained_insurance_top_up_for_actor(REUSED_INSURANCE_AUTHORITY, AMOUNT);
    env.land_retained(fresh)
        .expect("current-generation asset-0 top-up remains live");
    assert_eq!(
        env.token_amount(source),
        source_before - u64::try_from(AMOUNT).unwrap()
    );
    assert_eq!(
        env.token_amount(env.vault),
        vault_before + u64::try_from(AMOUNT).unwrap()
    );
    assert_eq!(env.token_supply_observed(), supply_before);
}

#[test]
fn v16_program_pr328_stale_withdrawal_drains_replacement_reserve() {
    let reproduction = reproduce_insurance_withdrawal_generation_replay([0x28; 32])
        .unwrap_or_else(|error| panic!("PR 328 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::InsuranceWithdrawalGenerationReplay
    );
    assert_ne!(reproduction.old_market_id, reproduction.new_market_id);
    assert_eq!(reproduction.replacement_provider_loss, 50_000);
    assert_eq!(reproduction.attacker_extraction, 50_000);
    assert!(reproduction.replay_cu < 1_400_000);
}

#[test]
fn v16_program_pr318_stale_backing_fee_extracts_victim_capital() {
    let reproduction = reproduce_backing_fee_generation_replay([0x18; 32])
        .unwrap_or_else(|error| panic!("PR 318 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::BackingFeeGenerationReplay
    );
    assert_ne!(reproduction.old_market_id, reproduction.new_market_id);
    assert_eq!(reproduction.backing_earnings, 75);
    assert_eq!(reproduction.victim_loss, 75);
    assert_eq!(reproduction.attacker_extraction, reproduction.victim_loss);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.trade_cu < 1_400_000);
    assert!(reproduction.withdrawal_cu < 1_400_000);
}

#[test]
fn v16_program_pr311_stale_resolve_crystallizes_replacement_loss() {
    let reproduction = reproduce_resolve_generation_replay([0x11; 32])
        .unwrap_or_else(|error| panic!("PR 311 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::ResolveGenerationReplay);
    assert!(reproduction.new_market_id > reproduction.old_market_id);
    assert_eq!(reproduction.victim_loss, 100_000);
    assert_eq!(reproduction.beneficiary_gain, 100_000);
    assert_eq!(reproduction.control_victim_payout, 1_000_000);
    assert_eq!(reproduction.replay_victim_payout, 900_000);
    assert_eq!(reproduction.control_winner_payout, 1_000_000);
    assert_eq!(reproduction.replay_winner_payout, 1_100_000);
    assert!(reproduction.replay_cu < 1_400_000);
}

#[test]
fn v16_program_pr275_stale_mark_pushes_reject_across_asset_generation() {
    for kind in [AssetIntentKind::PushAuthMark, AssetIntentKind::PushEwmaMark] {
        let protection = discover_asset_generation_replay([0x75; 32], kind)
            .unwrap_or_else(|error| panic!("PR 275 {kind:?} protection failed: {error}"));
        assert_eq!(protection.kind, kind);
        assert!(protection.new_asset_id > protection.old_asset_id);
        assert!(!protection.accepted_stale_intent);
        assert!(!protection.mutated_economic_state);
        assert_eq!(protection.compute_units, None);
        assert!(protection.rejection_was_generation_mismatch);
        assert!(protection.fresh_intent_landed);
        assert!(protection.fresh_intent_mutated_economic_state);
    }
}

#[test]
fn v16_program_pr277_pr322_stale_oracle_controls_reject_across_asset_generation() {
    for kind in [
        AssetIntentKind::ConfigureAuthMark,
        AssetIntentKind::ConfigureEwmaMark,
        AssetIntentKind::ConfigureHybridOracle,
    ] {
        let protection = discover_asset_generation_replay([0x77; 32], kind)
            .unwrap_or_else(|error| panic!("PR 277/322 {kind:?} protection failed: {error}"));
        assert_eq!(protection.kind, kind);
        assert!(protection.new_asset_id > protection.old_asset_id);
        assert!(!protection.accepted_stale_intent);
        assert!(!protection.mutated_economic_state);
        assert_eq!(protection.compute_units, None);
        assert!(protection.rejection_was_generation_mismatch);
        assert!(protection.fresh_intent_landed);
        assert!(protection.fresh_intent_mutated_economic_state);
    }
}

#[test]
fn v16_program_pr277_restart_rejects_old_generation_even_with_future_sequence() {
    const ASSET: u16 = 1;
    const PRICE: u64 = 100;
    const REUSED_ADMIN: usize = 2;

    let mut env = V16Svm::new([0x69; 32], MarketConfig::default());
    env.update_market_init_fee_policy(1)
        .expect("configure permissionless activation fee");
    env.configure_permissionless_resolve(100, 1)
        .expect("configure finite recovery delay");
    env.update_asset_authority_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_ADMIN,
        REUSED_ADMIN,
    )
    .expect("install the generation-A asset admin reused by generation B");
    env.warp_to_slot(1);
    env.shutdown_asset(ASSET, 1)
        .expect("put generation A into restartable Recovery");
    let old_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    let retained = env.build_retained_restart_asset_oracle_for_actor_with_sequence(
        REUSED_ADMIN,
        ASSET,
        5,
        PRICE,
        u64::MAX,
    );

    env.restart_asset_oracle_for_actor(REUSED_ADMIN, ASSET, 1, PRICE)
        .expect("restore generation A before retirement");
    env.warp_to_slot(2);
    env.retire_asset(ASSET, 2).expect("retire generation A");
    env.warp_to_slot(3);
    env.activate_permissionless_asset(REUSED_ADMIN, ASSET, 3, PRICE, 1)
        .expect("activate generation B");
    let new_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    assert!(new_market_id > old_market_id);
    env.warp_to_slot(4);
    env.shutdown_asset(ASSET, 4)
        .expect("put generation B into the same restartable state");

    env.warp_to_slot(5);
    let market_before = env.market_data(false);
    let supply_before = env.token_supply_observed();
    let error = env
        .land_retained(retained)
        .expect_err("generation-A restart must not control generation B");
    let expected = format!(
        "Custom({})",
        PercolatorError::AssetGenerationMismatch as u32
    );
    assert!(
        error.contains(&expected),
        "stale restart must return {expected}, got {error}"
    );
    assert_eq!(env.market_data(false), market_before);
    assert_eq!(env.token_supply_observed(), supply_before);

    env.restart_asset_oracle_for_actor(REUSED_ADMIN, ASSET, 5, PRICE)
        .expect("current-generation restart remains live");
    let replacement = &env.primary_market_state().1.assets[ASSET as usize];
    assert!(replacement.market_id > new_market_id);
    assert_eq!(replacement.lifecycle, AssetLifecycleV16::Active);
}
