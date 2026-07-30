mod support;

use support::{
    blocker_corpus::{blocker_scenarios, known_blocker_scenarios},
    fuzz_model::{
        reproduce_asset_generation_trade_replay, reproduce_composite_oracle_rounding,
        reproduce_cpi_backing_fee_siphon, reproduce_cpi_caller_fee_siphon,
        reproduce_omitted_rescue_liquidation, reproduce_pending_ewma_inheritance,
        reproduce_post_expiry_backing_fee, reproduce_reclaimable_ewma_fee,
        reproduce_rounded_funding_omission, reproduce_trade_retry_replay, run_scenario,
        CompositeRoundingCase, KnownBlocker, PostExpiryBackingCase, Scenario, TradeRoute,
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
            coverage.known_blocker_exit_locks[index], 1,
            "{name} did not prove both normal exits and the sole public crank are blocked"
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
fn v16_program_open_lof_manifest_is_complete_and_honest() {
    validate_manifest().expect("open LoF manifest structure");
    assert_eq!(
        quarantined_prs(),
        [220, 223, 224, 225, 231, 253, 260, 329, 343, 367, 381]
    );
    let missing = missing_prs();
    assert_eq!(
        missing.len(),
        88,
        "update the explicit evidence state when an executable adapter lands"
    );
    assert!(!missing.contains(&220));
    assert!(!missing.contains(&223));
    assert!(!missing.contains(&224));
    assert!(!missing.contains(&225));
    assert!(!missing.contains(&231));
    assert!(!missing.contains(&253));
    assert!(!missing.contains(&260));
    assert!(!missing.contains(&329));
    assert!(!missing.contains(&343));
    assert!(!missing.contains(&367));
    assert!(!missing.contains(&381));
}
