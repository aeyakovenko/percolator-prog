mod support;

use support::{
    blocker_corpus::{blocker_scenarios, known_blocker_scenarios},
    fuzz_model::{
        reproduce_asset_generation_config_replay, reproduce_asset_generation_mark_replay,
        reproduce_asset_generation_trade_replay, reproduce_composite_oracle_rounding,
        reproduce_cpi_backing_fee_siphon, reproduce_cpi_caller_fee_siphon,
        reproduce_cross_domain_b_settlement, reproduce_cross_domain_backing_double_spend,
        reproduce_forfeit_funding_erasure, reproduce_omitted_rescue_liquidation,
        reproduce_pending_ewma_inheritance, reproduce_post_expiry_backing_fee,
        reproduce_rebalance_funding_erasure, reproduce_reclaimable_ewma_fee,
        reproduce_rounded_funding_omission, reproduce_trade_driven_liquidation_reward,
        reproduce_trade_funding_erasure, reproduce_trade_retry_replay, run_scenario,
        AssetGenerationConfigPath, AssetGenerationMarkPath, CompositeRoundingCase, KnownBlocker,
        PostExpiryBackingCase, Scenario, TradeDrivenLiquidationMode, TradeRoute,
    },
    open_lof_manifest::{missing_prs, quarantined_prs, validate_manifest},
};

#[test]
fn v16_program_blocker_corpus_is_public_sbf_and_exit_live() {
    for (name, scenario) in blocker_scenarios() {
        let coverage = run_scenario(&scenario).unwrap_or_else(|error| {
            panic!(
                "blocker corpus scenario {name} failed\nscenario={}\n{error}",
                serde_json::to_string_pretty(&scenario).unwrap()
            )
        });
        assert!(
            coverage
                .known_blocker_exit_locks
                .iter()
                .all(|hits| *hits == 0),
            "safe corpus scenario {name} reached a quarantined user-exit lock"
        );
    }
}

#[test]
fn v16_program_scenario_replay_is_deterministic() {
    let (_, scenario): (&str, Scenario) = blocker_scenarios()
        .into_iter()
        .next()
        .expect("blocker corpus");
    let first = run_scenario(&scenario).expect("first deterministic replay");
    let second = run_scenario(&scenario).expect("second deterministic replay");
    assert_eq!(first, second);
}

#[test]
fn v16_program_known_blockers_remain_explicit_until_fixed() {
    for (name, scenario) in known_blocker_scenarios() {
        let coverage = run_scenario(&scenario).unwrap_or_else(|error| {
            panic!(
                "known blocker scenario {name} changed failure class\nscenario={}\n{error}",
                serde_json::to_string_pretty(&scenario).unwrap()
            )
        });
        let index = KnownBlocker::LiveLapsedSourceBacking.index();
        assert_ne!(
            coverage.known_blocker_hits[index], 0,
            "{name} no longer reproduces PR 204; remove its quarantine and promote the seed"
        );
        assert_eq!(
            coverage.known_blocker_exit_locks[index], 0,
            "{name} must not be overstated as persistent user-exit DoS while bilateral exits work"
        );
    }
}

#[test]
fn v16_program_pr367_post_expiry_backing_fee_is_extractable() {
    let reproduction = reproduce_post_expiry_backing_fee(
        [0x67; 32],
        PostExpiryBackingCase {
            fee_bps: 5_000,
            expiry_offset: 2,
            mark_move_bps: 500,
            increase_divisor: 20,
        },
    )
    .expect("PR 367 no longer reproduces; remove its quarantine and promote the seed");

    assert_eq!(reproduction.blocker, KnownBlocker::PostExpiryBackingFee);
    assert_eq!(
        reproduction.provider_earnings,
        u128::from(reproduction.extracted_tokens),
        "the protocol ledger and extracted SPL amount diverged"
    );
    assert_eq!(
        reproduction.victim_capital_loss, reproduction.provider_earnings,
        "the public reproduction did not transfer the trader's loss to the provider"
    );
}

#[test]
fn v16_program_pr220_omitted_rescue_mark_liquidates_healthy_control() {
    let reproduction = reproduce_omitted_rescue_liquidation([0x22; 32])
        .expect("PR 220 no longer reproduces; remove its quarantine and promote the seed");

    assert_eq!(
        reproduction.blocker,
        KnownBlocker::OmittedRescueAccrualLiquidation
    );
    assert!(
        reproduction.omitted_position_after_q < reproduction.omitted_position_before_q,
        "omitted world did not liquidate the victim"
    );
    assert!(reproduction.omitted_insurance_delta > 0);
    assert_eq!(
        reproduction.complete_position_after_q,
        reproduction.omitted_position_before_q
    );
    assert_eq!(reproduction.complete_liquidation_deficit, 0);
    assert_eq!(reproduction.complete_insurance_delta, 0);
}

#[test]
fn v16_program_pr343_trade_retry_variants_extract_value_on_every_route() {
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        let reproduction = reproduce_trade_retry_replay([0x43; 32], route)
            .unwrap_or_else(|error| panic!("PR 343 {route:?} no longer reproduces: {error}"));
        assert_eq!(reproduction.blocker, KnownBlocker::TradeRetryReplay);
        assert_eq!(reproduction.route, route);
        assert_eq!(
            reproduction.victim_extra_loss,
            reproduction.attacker_extra_payout
        );
        assert!(reproduction.victim_extra_loss > 0);
        assert_eq!(
            reproduction.control_total_payout,
            reproduction.replay_total_payout
        );
    }
}

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
fn v16_program_pr224_unsigned_lp_caller_fee_is_withdrawable() {
    for route in [TradeRoute::Cpi, TradeRoute::BatchCpi] {
        let reproduction = reproduce_cpi_caller_fee_siphon([0x24; 32], route)
            .unwrap_or_else(|error| panic!("PR 224 {route:?} no longer reproduces: {error}"));
        assert_eq!(reproduction.blocker, KnownBlocker::CpiCallerFeeSiphon);
        assert_eq!(reproduction.attacker_profit, reproduction.lp_loss);
        assert!(reproduction.withdrawn_insurance > 0);
        assert_eq!(reproduction.total_payout, 2_000_000);
    }
}

#[test]
fn v16_program_pr223_unsigned_lp_backing_fee_is_withdrawable() {
    let reproduction = reproduce_cpi_backing_fee_siphon([0x23; 32])
        .unwrap_or_else(|error| panic!("PR 223 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::CpiBackingFeeSiphon);
    assert_eq!(reproduction.lp_capital_loss, reproduction.provider_earnings);
    assert_eq!(
        reproduction.provider_earnings,
        u128::from(reproduction.extracted_tokens)
    );
    assert_eq!(reproduction.attacker_capital_delta, 0);
}

#[test]
fn v16_program_pr329_pr381_composite_rounding_false_liquidates() {
    for case in [
        CompositeRoundingCase::Pr329LargeMove,
        CompositeRoundingCase::Pr381MicroMove,
    ] {
        let reproduction = reproduce_composite_oracle_rounding([0x29; 32], case)
            .unwrap_or_else(|error| panic!("{case:?} no longer reproduces: {error}"));
        assert_eq!(reproduction.blocker, KnownBlocker::CompositeOracleRounding);
        assert_ne!(reproduction.rounded_target, reproduction.exact_mark);
        assert_ne!(reproduction.rounded_mark, reproduction.exact_mark);
        assert!(reproduction.victim_capital_loss > 0);
        assert!(reproduction.oi_reduction_q > 0);
        assert_eq!(
            reproduction.cranker_reward,
            u128::from(reproduction.extracted_tokens)
        );
    }
}

#[test]
fn v16_program_pr253_omitted_rounded_funding_transfers_spl_value() {
    let reproduction = reproduce_rounded_funding_omission([0x53; 32])
        .unwrap_or_else(|error| panic!("PR 253 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::RoundedFundingOmission);
    assert!(reproduction.control_f_long_num > 0);
    assert!(reproduction.control_f_short_num < 0);
    assert_eq!(reproduction.attack_f_long_num, 0);
    assert_eq!(reproduction.attack_f_short_num, 0);
    assert_eq!(
        reproduction.victim_payout_loss,
        reproduction.attacker_payout_gain
    );
}

#[test]
fn v16_program_pr260_pending_ewma_inheritance_extracts_on_every_route() {
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        let reproduction = reproduce_pending_ewma_inheritance([0x60; 32], route)
            .unwrap_or_else(|error| panic!("PR 260 {route:?} no longer reproduces: {error}"));
        assert_eq!(reproduction.blocker, KnownBlocker::PendingEwmaInheritance);
        assert!(reproduction.pending_mark > 1_000_000);
        assert!(reproduction.applied_mark > 1_000_000);
        assert_eq!(reproduction.attacker_gain, reproduction.victim_loss);
        assert!(reproduction.attacker_gain > reproduction.seed_cost);
        assert_eq!(
            u128::from(reproduction.net_extracted_tokens),
            reproduction.attacker_gain - reproduction.seed_cost
        );
    }
}

#[test]
fn v16_program_pr225_reclaimed_ewma_fee_extracts_on_every_route() {
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        let reproduction = reproduce_reclaimable_ewma_fee([0x25; 32], route)
            .unwrap_or_else(|error| panic!("PR 225 {route:?} no longer reproduces: {error}"));
        assert_eq!(reproduction.blocker, KnownBlocker::ReclaimableEwmaFee);
        assert_eq!(reproduction.fee_reclaimed, reproduction.fee_paid);
        assert_eq!(reproduction.attacker_gain + 1, reproduction.victim_loss);
        assert!(reproduction.attacker_gain > 0);
        assert!(reproduction.effective_mark < 1_000_000);
    }
}

#[test]
fn v16_program_pr271_cpi_close_erases_elapsed_funding() {
    for route in [TradeRoute::Cpi, TradeRoute::BatchCpi] {
        let reproduction = reproduce_trade_funding_erasure([0x71; 32], route)
            .unwrap_or_else(|error| panic!("PR 271 {route:?} no longer reproduces: {error}"));
        assert_eq!(reproduction.blocker, KnownBlocker::TradeFundingErasure);
        assert!(reproduction.control_f_long_num > 0);
        assert!(reproduction.control_f_short_num < 0);
        assert_eq!(reproduction.attack_f_long_num, 0);
        assert_eq!(reproduction.attack_f_short_num, 0);
        assert_eq!(
            reproduction.victim_payout_loss,
            reproduction.attacker_payout_gain
        );
    }
}

#[test]
fn v16_program_pr272_unilateral_reduce_erases_elapsed_funding() {
    let reproduction = reproduce_rebalance_funding_erasure([0x72; 32])
        .unwrap_or_else(|error| panic!("PR 272 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::RebalanceFundingErasure);
    assert_eq!(
        reproduction.control_attacker_paid,
        reproduction.control_victim_received
    );
    assert!(reproduction.control_attacker_paid > 0);
    assert_eq!(reproduction.attack_attacker_paid, 0);
    assert_eq!(reproduction.attack_victim_received, 0);
    assert_eq!(
        reproduction.victim_claim_loss,
        u128::from(reproduction.attacker_payout_gain)
    );
}

#[test]
fn v16_program_pr273_recovery_forfeit_erases_elapsed_funding() {
    let reproduction = reproduce_forfeit_funding_erasure([0x73; 32])
        .unwrap_or_else(|error| panic!("PR 273 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::ForfeitFundingErasure);
    assert_eq!(
        reproduction.control_attacker_paid,
        reproduction.control_victim_received
    );
    assert!(reproduction.control_attacker_paid > 0);
    assert_eq!(reproduction.attack_attacker_paid, 0);
    assert_eq!(reproduction.attack_victim_received, 0);
    assert_eq!(
        reproduction.victim_claim_loss,
        i128::from(reproduction.attacker_payout_gain)
    );
}

#[test]
fn v16_program_pr280_trade_driven_liquidation_reward_is_extractable() {
    for mode in [
        TradeDrivenLiquidationMode::Ewma,
        TradeDrivenLiquidationMode::HybridAfterHours,
    ] {
        for route in [TradeRoute::NoCpi, TradeRoute::BatchNoCpi] {
            let reproduction = reproduce_trade_driven_liquidation_reward([0x80; 32], mode, route)
                .unwrap_or_else(|error| {
                    panic!("PR 280 {mode:?} {route:?} no longer reproduces: {error}")
                });
            assert_eq!(
                reproduction.blocker,
                KnownBlocker::TradeDrivenLiquidationReward
            );
            assert!(reproduction.cranker_reward > reproduction.movement_fee);
            assert!(reproduction.victim_penalty > 0);
            assert!(reproduction.victim_capital_loss > 0);
            assert!(reproduction.attacker_profit > 0);
            assert!(reproduction.attacker_extracted > 2_001);
        }
    }
}

#[test]
fn v16_program_pr267_cross_domain_backing_is_spent_twice() {
    let reproduction = reproduce_cross_domain_backing_double_spend([0x67; 32])
        .unwrap_or_else(|error| panic!("PR 267 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::CrossDomainBackingDoubleSpend
    );
    assert_eq!(
        reproduction.unfunded_claim_before_num,
        100 * percolator::BOUND_SCALE
    );
    assert_eq!(
        reproduction.funded_claim_before_num,
        100 * percolator::BOUND_SCALE
    );
    assert_eq!(
        reproduction.funded_backing_consumed_num,
        200 * percolator::BOUND_SCALE
    );
    assert_eq!(reproduction.winner_capital_gain, 200);
    assert_eq!(reproduction.extracted_tokens, 1_200);
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
fn v16_program_pr277_stale_config_replays_across_asset_generation() {
    for path in [
        AssetGenerationConfigPath::Auth,
        AssetGenerationConfigPath::Ewma,
    ] {
        let reproduction = reproduce_asset_generation_config_replay([0x77; 32], path)
            .unwrap_or_else(|error| panic!("PR 277 {path:?} no longer reproduces: {error}"));
        assert_eq!(
            reproduction.blocker,
            KnownBlocker::AssetGenerationConfigReplay
        );
        assert_eq!(reproduction.path, path);
        assert_ne!(reproduction.old_market_id, reproduction.new_market_id);
        assert_eq!(reproduction.stale_entry_price, 50);
        assert!(reproduction.restored_mark > reproduction.stale_entry_price);
        assert!(reproduction.victim_equity_loss > 0);
        assert_eq!(
            reproduction.victim_equity_loss,
            u128::from(reproduction.beneficiary_extra_payout)
        );
    }
}

#[test]
fn v16_program_pr281_wrong_domain_b_settlement_strands_dust_position() {
    let reproduction = reproduce_cross_domain_b_settlement([0x81; 32])
        .unwrap_or_else(|error| panic!("PR 281 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::CrossDomainBSettlement);
    assert!(reproduction.b_target_num > 0);
    assert!(reproduction.pnl_loss > 0);
    assert!(reproduction.unfunded_claim_after_num < reproduction.unfunded_claim_before_num);
    assert!(reproduction.funded_claim_after_num < reproduction.funded_claim_before_num);
    assert_eq!(
        (reproduction.unfunded_claim_before_num - reproduction.unfunded_claim_after_num)
            + (reproduction.funded_claim_before_num - reproduction.funded_claim_after_num),
        reproduction.pnl_loss * percolator::BOUND_SCALE
    );
    assert!(reproduction.wrong_domain_reduction_num > 0);
    assert!(reproduction.correct_domain_reduction_num > 0);
    assert!(reproduction.reduction_steps > 0);
    assert_eq!(reproduction.stranded_position_q, percolator::POS_SCALE);
    assert!(reproduction.failed_terminal_reductions >= 6);
    assert!(reproduction.full_withdraw_rejected);
}

#[test]
fn v16_program_open_lof_manifest_is_complete_and_honest() {
    validate_manifest().expect("open LoF manifest structure");
    assert_eq!(
        quarantined_prs(),
        [
            220, 223, 224, 225, 231, 253, 260, 267, 271, 272, 273, 275, 277, 280, 281, 329, 343,
            367, 381
        ]
    );
    let missing = missing_prs();
    assert_eq!(
        missing.len(),
        80,
        "update the explicit evidence state when an executable adapter lands"
    );
    assert!(!missing.contains(&220));
    assert!(!missing.contains(&223));
    assert!(!missing.contains(&224));
    assert!(!missing.contains(&225));
    assert!(!missing.contains(&231));
    assert!(!missing.contains(&253));
    assert!(!missing.contains(&260));
    assert!(!missing.contains(&267));
    assert!(!missing.contains(&271));
    assert!(!missing.contains(&272));
    assert!(!missing.contains(&273));
    assert!(!missing.contains(&275));
    assert!(!missing.contains(&277));
    assert!(!missing.contains(&280));
    assert!(!missing.contains(&281));
    assert!(!missing.contains(&329));
    assert!(!missing.contains(&343));
    assert!(!missing.contains(&367));
    assert!(!missing.contains(&381));
}
