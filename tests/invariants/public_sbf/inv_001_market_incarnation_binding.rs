//! INV-001 - Market incarnation binding.
//!
//! Normative obligation: Retained requests cannot cross a market close, recreation, or generation change.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr294_stale_matcher_grant_liquidates_reinitialized_market`, `v16_program_pr296_stale_trade_fee_policy_cannot_silently_debit_reinitialized_market`, `v16_program_pr295_stale_forfeit_discards_reinitialized_market_winner_payout`, `v16_program_pr317_stale_fee_redirect_extracts_victim_fee`, `v16_program_pr307_stale_deposit_funds_reinitialized_market_winner`, `v16_program_pr315_same_market_restart_rejects_stale_shutdown`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.
//! PR 296's economic extraction is blocked by signed base-fee consent, but the test deliberately
//! records that the stale policy itself still lands; whole-market generation binding remains open.
//! PR 315's same-market restart path is protected by INV-002's asset generation, but recreation of
//! the entire market at the same pubkey can reset that counter and remains an INV-001 gap.

use super::*;

#[test]
fn v16_program_pr294_stale_matcher_grant_liquidates_reinitialized_market() {
    let reproduction = reproduce_matcher_grant_market_generation_replay([0x94; 32])
        .unwrap_or_else(|error| panic!("PR 294 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::MatcherGrantMarketGenerationReplay
    );
    assert!(reproduction.old_market_id > 0);
    assert!(reproduction.new_market_id > 0);
    assert!(reproduction.control_trade_blocked);
    assert!(reproduction.liquidation_slot > 11);
    assert_eq!(
        reproduction.cranker_reward,
        u128::from(reproduction.extracted_reward)
    );
    assert_eq!(reproduction.cranker_reward, 15_835);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.max_cu < 1_400_000);
}

#[test]
fn v16_program_pr296_stale_trade_fee_policy_cannot_silently_debit_reinitialized_market() {
    let protection = verify_trade_fee_market_generation_nonextraction([0x96; 32])
        .unwrap_or_else(|error| panic!("PR 296 non-extraction protection failed: {error}"));
    assert_eq!(
        protection.blocker,
        KnownBlocker::TradeFeeMarketGenerationReplay
    );
    assert!(protection.old_market_id > 0);
    assert!(protection.new_market_id > 0);
    assert!(protection.stale_policy_landed);
    assert!(protection.stale_trade_rejected);
    assert!(protection.rejected_exact_rollback);
    assert!(protection.recovery_trade_landed);
    assert_eq!(protection.victim_loss, 0);
    assert_eq!(protection.attacker_profit, 0);
    assert_eq!(protection.extracted_fee, 0);
    assert!(protection.replay_cu < 1_400_000);
    assert!(protection.trade_cu < 1_400_000);
    assert!(protection.max_cu < 1_400_000);
}

#[test]
fn v16_program_pr295_stale_forfeit_discards_reinitialized_market_winner_payout() {
    let reproduction = reproduce_forfeit_market_generation_replay([0x95; 32])
        .unwrap_or_else(|error| panic!("PR 295 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::ForfeitMarketGenerationReplay
    );
    assert!(reproduction.old_market_id > 0);
    assert!(reproduction.new_market_id > 0);
    assert!(reproduction.victim_loss > 0);
    assert_eq!(reproduction.stranded_vault, reproduction.victim_loss.into());
    assert!(reproduction.control_slab_closed);
    assert!(reproduction.replay_slab_blocked);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.max_cu < 1_400_000);
}

#[test]
fn v16_program_pr317_stale_fee_redirect_extracts_victim_fee() {
    let reproduction = reproduce_fee_redirect_generation_replay([0x17; 32])
        .unwrap_or_else(|error| panic!("PR 317 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::FeeRedirectGenerationReplay
    );
    assert_eq!(reproduction.old_market_id, reproduction.new_market_id);
    assert_eq!(reproduction.redirected_fee, 2_000);
    assert_eq!(reproduction.victim_loss, 1_000);
    assert_eq!(reproduction.attacker_profit, reproduction.victim_loss);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.trade_cu < 1_400_000);
    assert!(reproduction.withdrawal_cu < 1_400_000);
}

#[test]
fn v16_program_pr307_stale_deposit_funds_reinitialized_market_winner() {
    let reproduction = reproduce_market_incarnation_deposit([0x07; 32])
        .unwrap_or_else(|error| panic!("PR 307 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::MarketIncarnationDeposit);
    assert_eq!(
        reproduction.old_asset_market_id,
        reproduction.new_asset_market_id
    );
    assert_eq!(reproduction.stale_deposit, 100_000);
    assert_eq!(reproduction.beneficiary_extra_payout, 100_000);
    assert_eq!(reproduction.control_winner_payout, 100_150_000);
    assert_eq!(reproduction.replay_winner_payout, 100_250_000);
    assert!(reproduction.replay_cu < 1_400_000);
}

#[test]
fn v16_program_pr315_same_market_restart_rejects_stale_shutdown() {
    let protection =
        discover_asset_generation_replay([0x15; 32], AssetIntentKind::LifecycleShutdown)
            .unwrap_or_else(|error| panic!("PR 315 protection probe failed: {error}"));
    assert!(protection.new_asset_id > protection.old_asset_id);
    assert!(!protection.accepted_stale_intent);
    assert!(!protection.mutated_economic_state);
    assert_eq!(protection.compute_units, None);
    assert!(protection.rejection_was_generation_mismatch);
    assert!(protection.fresh_intent_landed);
    assert!(protection.fresh_intent_mutated_economic_state);
}
