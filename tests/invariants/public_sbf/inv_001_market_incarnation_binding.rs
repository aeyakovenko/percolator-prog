//! INV-001 - Market incarnation binding.
//!
//! Normative obligation: Retained requests cannot cross a market close, recreation, or generation change.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr294_stale_matcher_grant_liquidates_reinitialized_market`, `v16_program_pr296_stale_trade_fee_policy_extracts_from_reinitialized_market`, `v16_program_pr295_stale_forfeit_discards_reinitialized_market_winner_payout`, `v16_program_pr317_stale_fee_redirect_extracts_victim_fee`, `v16_program_pr307_stale_deposit_funds_reinitialized_market_winner`, `v16_program_pr315_stale_shutdown_force_closes_replacement_loss`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

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
fn v16_program_pr296_stale_trade_fee_policy_extracts_from_reinitialized_market() {
    let reproduction = reproduce_trade_fee_market_generation_replay([0x96; 32])
        .unwrap_or_else(|error| panic!("PR 296 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::TradeFeeMarketGenerationReplay
    );
    assert!(reproduction.old_market_id > 0);
    assert!(reproduction.new_market_id > 0);
    assert_eq!(reproduction.victim_loss, 1_000);
    assert_eq!(reproduction.attacker_profit, reproduction.victim_loss);
    assert_eq!(reproduction.extracted_fee, 2_000);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.trade_cu < 1_400_000);
    assert!(reproduction.max_cu < 1_400_000);
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
fn v16_program_pr315_stale_shutdown_force_closes_replacement_loss() {
    let reproduction = reproduce_shutdown_generation_replay([0x15; 32])
        .unwrap_or_else(|error| panic!("PR 315 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::ShutdownGenerationReplay);
    assert!(reproduction.new_market_id > reproduction.old_market_id);
    assert_eq!(reproduction.victim_loss, 100_000);
    assert_eq!(reproduction.beneficiary_gain, 100_000);
    assert_eq!(reproduction.control_victim_payout, 1_000_000);
    assert_eq!(reproduction.replay_victim_payout, 900_000);
    assert_eq!(reproduction.control_winner_payout, 1_000_000);
    assert_eq!(reproduction.replay_winner_payout, 1_100_000);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.force_close_cu < 1_400_000);
}
