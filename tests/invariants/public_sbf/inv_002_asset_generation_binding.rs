//! INV-002 - Asset generation binding.
//!
//! Normative obligation: Asset-scoped consent cannot cross retirement, slot reuse, or asset-generation changes.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr231_asset_generation_replay_extracts_on_every_route`, `v16_program_pr279_stale_collateral_top_up_funds_replacement_operator`, `v16_program_pr321_stale_backing_top_up_funds_replacement_winner`, `v16_program_pr328_stale_withdrawal_drains_replacement_reserve`, `v16_program_pr318_stale_backing_fee_extracts_victim_capital`, `v16_program_pr311_stale_resolve_crystallizes_replacement_loss`, `v16_program_pr275_stale_mark_replays_across_asset_generation`, `v16_program_pr277_pr322_stale_config_replays_across_asset_generation`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_pr231_asset_generation_replay_extracts_on_every_route() {
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        let reproduction = reproduce_asset_generation_trade_replay([0x31; 32], route)
            .unwrap_or_else(|error| panic!("PR 231 {route:?} no longer reproduces: {error}"));
        assert_eq!(
            reproduction.blocker,
            KnownBlocker::AssetGenerationTradeReplay
        );
        assert_ne!(reproduction.old_market_id, reproduction.new_market_id);
        assert!(reproduction.victim_loss > 0);
        assert!(reproduction.attacker_payout > 1_000_000);
        assert_eq!(reproduction.total_payout, 2_000_000);
    }
}

#[test]
fn v16_program_pr279_stale_collateral_top_up_funds_replacement_operator() {
    let reproduction = reproduce_collateral_top_up_generation_replay([0x79; 32])
        .unwrap_or_else(|error| panic!("PR 279 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::CollateralTopUpGenerationReplay
    );
    assert_ne!(reproduction.old_market_id, reproduction.new_market_id);
    assert_eq!(reproduction.victim_loss, 250_000);
    assert_eq!(reproduction.attacker_extraction, 250_000);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.withdrawal_cu < 1_400_000);
}

#[test]
fn v16_program_pr321_stale_backing_top_up_funds_replacement_winner() {
    let reproduction = reproduce_backing_top_up_generation_replay([0x21; 32])
        .unwrap_or_else(|error| panic!("PR 321 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::BackingTopUpGenerationReplay
    );
    assert_ne!(reproduction.old_market_id, reproduction.new_market_id);
    assert_eq!(reproduction.provider_loss, 150);
    assert_eq!(reproduction.attacker_profit, 150);
    assert_eq!(reproduction.attacker_payout, 2_400);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.max_cu < 1_400_000);
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
fn v16_program_pr275_stale_mark_replays_across_asset_generation() {
    for path in [AssetGenerationMarkPath::Auth, AssetGenerationMarkPath::Ewma] {
        let reproduction = reproduce_asset_generation_mark_replay([0x75; 32], path)
            .unwrap_or_else(|error| panic!("PR 275 {path:?} no longer reproduces: {error}"));
        assert_eq!(
            reproduction.blocker,
            KnownBlocker::AssetGenerationMarkReplay
        );
        assert_eq!(reproduction.path, path);
        assert_ne!(reproduction.old_market_id, reproduction.new_market_id);
        assert!(reproduction.landed_mark < 100);
        assert!(reproduction.victim_equity_loss > 0);
        assert_eq!(
            reproduction.victim_equity_loss,
            u128::from(reproduction.beneficiary_extra_payout)
        );
    }
}

#[test]
fn v16_program_pr277_pr322_stale_config_replays_across_asset_generation() {
    for path in [
        AssetGenerationConfigPath::Auth,
        AssetGenerationConfigPath::Ewma,
        AssetGenerationConfigPath::Hybrid,
    ] {
        let reproduction = reproduce_asset_generation_config_replay([0x77; 32], path)
            .unwrap_or_else(|error| panic!("PR 277/322 {path:?} no longer reproduces: {error}"));
        assert_eq!(
            reproduction.blocker,
            KnownBlocker::AssetGenerationConfigReplay
        );
        assert_eq!(reproduction.path, path);
        assert_ne!(reproduction.old_market_id, reproduction.new_market_id);
        assert_eq!(
            reproduction.stale_entry_price,
            if path == AssetGenerationConfigPath::Hybrid {
                100
            } else {
                50
            }
        );
        assert!(reproduction.restored_mark > reproduction.stale_entry_price);
        assert!(reproduction.victim_equity_loss > 0);
        assert_eq!(
            reproduction.victim_equity_loss,
            u128::from(reproduction.beneficiary_extra_payout)
        );
        if path == AssetGenerationConfigPath::Hybrid {
            assert_eq!(reproduction.victim_equity_loss, 100);
            assert_eq!(reproduction.beneficiary_extra_payout, 100);
        }
    }
}
