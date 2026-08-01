mod support;

use support::{
    blocker_corpus::{blocker_scenarios, known_blocker_scenarios},
    fuzz_model::{
        reproduce_activation_fee_consent, reproduce_activation_retry_replay,
        reproduce_asset_generation_config_replay, reproduce_asset_generation_mark_replay,
        reproduce_asset_generation_trade_replay, reproduce_authority_handoff_aba_replay,
        reproduce_backing_fee_consent_replay, reproduce_backing_fee_generation_replay,
        reproduce_backing_top_up_generation_replay, reproduce_backing_top_up_retry_replay,
        reproduce_bilateral_base_fee_consent, reproduce_bilateral_fee_support,
        reproduce_collateral_top_up_generation_replay, reproduce_composite_oracle_rounding,
        reproduce_composite_oracle_time_skew, reproduce_convert_portfolio_incarnation_replay,
        reproduce_cpi_backing_fee_siphon, reproduce_cpi_caller_fee_siphon,
        reproduce_cross_domain_b_settlement, reproduce_cross_domain_backing_double_spend,
        reproduce_cross_margin_insurance_drain, reproduce_delayed_asset_authority_revival,
        reproduce_delayed_backing_fee_policy_replay, reproduce_delayed_fee_redirect_policy_replay,
        reproduce_delayed_liquidation_policy_replay, reproduce_delayed_maintenance_policy_replay,
        reproduce_delayed_matcher_enable_replay, reproduce_delayed_oracle_intent_replay,
        reproduce_delayed_resolve_policy_replay, reproduce_delayed_trade_fee_policy_replay,
        reproduce_deposit_retry_replay, reproduce_fee_redirect_generation_replay,
        reproduce_forfeit_funding_erasure, reproduce_forfeit_market_generation_replay,
        reproduce_forfeit_portfolio_incarnation_replay, reproduce_fractional_cap_settlement,
        reproduce_insurance_top_up_retry_replay, reproduce_insurance_withdrawal_generation_replay,
        reproduce_liquidation_policy_generation_replay,
        reproduce_maintenance_policy_generation_replay, reproduce_market_incarnation_deposit,
        reproduce_matcher_grant_market_generation_replay,
        reproduce_matcher_grant_portfolio_incarnation_replay, reproduce_omitted_rescue_liquidation,
        reproduce_pending_ewma_inheritance, reproduce_pending_ewma_target_override,
        reproduce_pending_mark_fee_reward, reproduce_portfolio_close_incarnation_replay,
        reproduce_portfolio_incarnation_deposit, reproduce_portfolio_incarnation_withdrawal,
        reproduce_post_expiry_backing_fee, reproduce_prospective_funding_rewrite,
        reproduce_rebalance_funding_erasure, reproduce_reclaimable_ewma_fee,
        reproduce_resolve_authority_incarnation_replay, reproduce_resolve_before_committed_accrual,
        reproduce_resolve_generation_replay, reproduce_rounded_funding_omission,
        reproduce_shutdown_generation_replay, reproduce_terminal_dust_payout_erasure,
        reproduce_trade_driven_liquidation_reward, reproduce_trade_fee_market_generation_replay,
        reproduce_trade_funding_erasure, reproduce_trade_portfolio_incarnation_replay,
        reproduce_trade_retry_replay, reproduce_unstaged_mark_target,
        reproduce_withdrawal_retry_liquidation, run_scenario, AssetGenerationConfigPath,
        AssetGenerationMarkPath, AuthorityHandoffAbaPath, BackingFeeConsentOrder, BilateralFeeMode,
        CompositeRoundingCase, DelayedOracleIntentPath, KnownBlocker,
        PortfolioIncarnationTradeSide, PostExpiryBackingCase, Scenario, TargetStagingCase,
        TradeDrivenLiquidationMode, TradeRoute,
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
            "{name} must not claim a persistent user-exit lock when authenticated same-price \
             observations let the owner exit"
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
fn v16_program_pr220_pr366_omitted_rescue_accrual_liquidates_healthy_control() {
    let reproduction = reproduce_omitted_rescue_liquidation([0x22; 32])
        .expect("PR 220/366 no longer reproduces; remove their quarantines and promote the seed");

    assert_eq!(
        reproduction.blocker,
        KnownBlocker::OmittedRescueAccrualLiquidation
    );
    assert!(
        reproduction.omitted_position_after_q < reproduction.omitted_position_before_q,
        "omitted world did not liquidate the victim"
    );
    assert!(reproduction.omitted_insurance_delta > 0);
    assert_eq!(reproduction.omitted_position_before_q, 50_000_000);
    assert_eq!(reproduction.omitted_position_after_q, 47_995_187);
    assert_eq!(reproduction.omitted_insurance_delta, 1_001);
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
fn v16_program_pr282_pending_ewma_target_override_extracts_on_every_route() {
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        let reproduction = reproduce_pending_ewma_target_override([0x82; 32], route)
            .unwrap_or_else(|error| panic!("PR 282 {route:?} no longer reproduces: {error}"));
        assert_eq!(
            reproduction.blocker,
            KnownBlocker::PendingEwmaTargetOverride
        );
        assert_eq!(reproduction.route, route);
        assert!(reproduction.attack_target < reproduction.control_target);
        assert!(reproduction.movement_fee < reproduction.displaced_victim_pnl);
        assert!(reproduction.attacker_profit > 0);
        assert!(reproduction.attacker_withdrawn > 24_000_000_000);
        assert!(reproduction.victim_withdrawn < 20_000_000_000);
    }
}

#[test]
fn v16_program_pr283_one_atom_erases_terminal_victim_payout() {
    for route in [TradeRoute::NoCpi, TradeRoute::BatchNoCpi] {
        let reproduction = reproduce_terminal_dust_payout_erasure([0x83; 32], route)
            .unwrap_or_else(|error| panic!("PR 283 {route:?} no longer reproduces: {error}"));
        assert_eq!(
            reproduction.blocker,
            KnownBlocker::TerminalDustPayoutErasure
        );
        assert_eq!(reproduction.route, route);
        assert_eq!(reproduction.attacker_loss, 1);
        assert!(reproduction.victim_loss > 8_000_000_000);
        assert_eq!(
            reproduction.vault_remaining,
            reproduction.victim_loss + reproduction.attacker_loss
        );
        assert_eq!(
            reproduction.attacker_withdrawn + reproduction.attacker_loss,
            20_000_002_000
        );
        assert_eq!(
            reproduction.victim_withdrawn + reproduction.victim_loss,
            20_000_000_000
        );
    }
}

#[test]
fn v16_program_pr290_cross_margin_debt_drains_unrelated_insurance() {
    let reproduction = reproduce_cross_margin_insurance_drain([0x90; 32])
        .unwrap_or_else(|error| panic!("PR 290 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::CrossMarginInsuranceDrain
    );
    assert!(reproduction.unrelated_insurance_spent >= 100_000);
    assert!(reproduction.attacker_payout > 20_200);
    assert!(reproduction.attacker_profit > 90_000);
    assert!(reproduction.liquidation_calls > 0);
    assert!(reproduction.loser_close_calls < 512);
    assert!(reproduction.counterparty_close_calls > 1);
    assert!(reproduction.counterparty_close_calls < 512);
    assert!(reproduction.winner_close_calls > 0 && reproduction.winner_close_calls < 512);
}

#[test]
fn v16_program_pr331_temporally_skewed_composite_liquidates_at_false_price() {
    let reproduction = reproduce_composite_oracle_time_skew([0x31; 32])
        .unwrap_or_else(|error| panic!("PR 331 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::CompositeOracleTimeSkew);
    assert_eq!(reproduction.coherent_price, 1_500_000);
    assert!(reproduction.skewed_target > reproduction.coherent_price);
    assert!(reproduction.skewed_mark > reproduction.coherent_price);
    assert!(reproduction.victim_capital_loss > 0);
    assert!(reproduction.oi_reduction_q > 0);
    assert!(reproduction.cranker_reward > 0);
    assert_eq!(
        u128::from(reproduction.extracted_tokens),
        reproduction.cranker_reward
    );
    assert!(reproduction.max_crank_cu < support::v16_svm::TX_CU_LIMIT);
}

#[test]
fn v16_program_pr264_pr265_pr332_pr333_unstaged_targets_open_stale_cpi_window() {
    for case in [
        TargetStagingCase::AuthMarkPush,
        TargetStagingCase::EwmaMarkPush,
        TargetStagingCase::EwmaSingleTrade,
        TargetStagingCase::EwmaBatchTrade,
    ] {
        let reproduction = reproduce_unstaged_mark_target([0x32; 32], case)
            .unwrap_or_else(|error| panic!("{case:?} no longer reproduces: {error}"));
        assert_eq!(reproduction.blocker, KnownBlocker::UnstagedMarkTarget);
        assert_eq!(reproduction.case, case);
        assert_eq!(reproduction.stale_engine_target, 100);
        assert_eq!(reproduction.moved_engine_mark, reproduction.wrapper_target);
        assert!(reproduction.attacker_profit > 0);
        assert_eq!(
            reproduction.attacker_profit,
            reproduction.victim_capital_loss
        );
        assert!(u128::from(reproduction.attacker_withdrawn) > reproduction.attacker_profit);
        assert!(reproduction.attack_cu < support::v16_svm::TX_CU_LIMIT);
    }
}

#[test]
fn v16_program_pr356_fee_sync_front_run_diverts_terminal_value() {
    let reproduction = reproduce_pending_mark_fee_reward([0x56; 32])
        .unwrap_or_else(|error| panic!("PR 356 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::PendingMarkFeeReward);
    assert!(reproduction.attack_reward > reproduction.control_reward);
    assert!(reproduction.control_winner_payout > reproduction.attack_winner_payout);
    assert_eq!(
        reproduction.attack_reward - reproduction.control_reward,
        reproduction.diverted_value
    );
    assert_eq!(
        reproduction.control_winner_payout - reproduction.attack_winner_payout,
        reproduction.diverted_value
    );
    assert_eq!(reproduction.extracted_reward, reproduction.attack_reward);
}

#[test]
fn v16_program_pr365_fractional_cap_floor_changes_terminal_payouts() {
    let reproduction = reproduce_fractional_cap_settlement([0x65; 32])
        .unwrap_or_else(|error| panic!("PR 365 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::FractionalCapSettlement);
    assert_eq!(reproduction.target_price, 1);
    assert!(reproduction.stalled_price > reproduction.target_price);
    assert!(reproduction.successful_cranks > 0);
    assert_eq!(
        reproduction.long_overpayment,
        reproduction.short_underpayment
    );
    assert!(reproduction.short_underpayment > 0);
    assert_eq!(
        u128::from(reproduction.long_payout) + u128::from(reproduction.short_payout),
        2_000_000
    );
}

#[test]
fn v16_program_pr380_trade_first_rewrites_elapsed_funding() {
    for route in [TradeRoute::NoCpi, TradeRoute::BatchNoCpi] {
        let reproduction = reproduce_prospective_funding_rewrite([0x80; 32], route)
            .unwrap_or_else(|error| panic!("PR 380 {route:?} no longer reproduces: {error}"));
        assert_eq!(
            reproduction.blocker,
            KnownBlocker::ProspectiveFundingRewrite
        );
        assert_eq!(reproduction.route, route);
        assert!(reproduction.control_f_short_num > 0);
        assert_eq!(reproduction.attack_f_short_num, 0);
        assert!(reproduction.stamp_fee > 0);
        assert_eq!(
            reproduction.victim_payout_loss,
            reproduction.attacker_coalition_gain
        );
        assert!(reproduction.victim_payout_loss > 0);
        assert_eq!(
            reproduction.control_total_payout,
            reproduction.attack_total_payout
        );
    }
}

#[test]
fn v16_program_pr255_stale_resolve_discards_pending_authenticated_mark() {
    let reproduction = reproduce_resolve_before_committed_accrual([0x55; 32])
        .unwrap_or_else(|error| panic!("PR 255 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::ResolveBeforeCommittedAccrual
    );
    assert!(reproduction.control_mark > reproduction.attack_mark);
    assert_eq!(
        reproduction.victim_payout_loss,
        reproduction.attacker_payout_gain
    );
    assert_eq!(reproduction.victim_payout_loss, 10_000_000);
    assert_eq!(
        reproduction.control_total_payout,
        reproduction.attack_total_payout
    );
    assert_eq!(reproduction.attack_total_payout, 4_000_000_000);
    assert!(reproduction.attack_resolve_cu < 1_400_000);
}

#[test]
fn v16_program_pr369_one_sided_cpi_fee_subsidizes_attacker_mark_gain() {
    for mode in [BilateralFeeMode::Ewma, BilateralFeeMode::HybridAfterHours] {
        for route in [TradeRoute::Cpi, TradeRoute::BatchCpi] {
            let reproduction = reproduce_bilateral_fee_support([0x69; 32], mode, route)
                .unwrap_or_else(|error| {
                    panic!("PR 369 {mode:?} {route:?} no longer reproduces: {error}")
                });
            assert_eq!(reproduction.blocker, KnownBlocker::BilateralFeeSupport);
            assert_eq!(reproduction.mode, mode);
            assert_eq!(reproduction.route, route);
            let expected = match mode {
                BilateralFeeMode::Ewma => (1_988_158, 781_589, 881_590),
                BilateralFeeMode::HybridAfterHours => (2_090_398, 903_989, 903_990),
            };
            assert_eq!(reproduction.queued_mark, expected.0);
            assert_eq!(reproduction.attacker_profit, expected.1);
            assert_eq!(reproduction.victim_loss, expected.2);
            assert!(reproduction.fee_lp_loss > 0);
            assert!(reproduction.insurance_gain > 0);
            assert!(reproduction.max_cu < 1_400_000);
        }
    }
}

#[test]
fn v16_program_pr251_delayed_admin_handoff_revives_withdrawal_authority() {
    let reproduction = reproduce_delayed_asset_authority_revival([0x51; 32])
        .unwrap_or_else(|error| panic!("PR 251 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::DelayedAssetAuthorityRevival
    );
    assert_eq!(reproduction.funded_reserve, 50_000);
    assert_eq!(reproduction.provider_loss, 50_000);
    assert_eq!(reproduction.attacker_extraction, 50_000);
    assert_eq!(reproduction.reserve_after, 0);
    assert!(reproduction.handoff_cu < 1_400_000);
    assert!(reproduction.withdrawal_cu < 1_400_000);
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
fn v16_program_pr344_insurance_top_up_retry_extracts_duplicate() {
    let reproduction = reproduce_insurance_top_up_retry_replay([0x44; 32])
        .unwrap_or_else(|error| panic!("PR 344 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::InsuranceTopUpRetryReplay
    );
    assert_eq!(reproduction.intended_contribution, 50_000);
    assert_eq!(reproduction.duplicate_loss, 50_000);
    assert_eq!(reproduction.operator_extraction, 50_000);
    assert_eq!(reproduction.insured_remainder, 50_000);
    assert!(reproduction.first_cu < 1_400_000);
    assert!(reproduction.replay_cu < 1_400_000);
}

#[test]
fn v16_program_pr362_activation_retry_extracts_duplicate_fee() {
    let reproduction = reproduce_activation_retry_replay([0x62; 32])
        .unwrap_or_else(|error| panic!("PR 362 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::ActivationRetryReplay);
    assert_ne!(reproduction.first_market_id, reproduction.replay_market_id);
    assert_eq!(reproduction.intended_fee, 500);
    assert_eq!(reproduction.duplicate_loss, 500);
    assert_eq!(reproduction.beneficiary_extraction, 500);
    assert_eq!(reproduction.insured_remainder, 500);
    assert!(reproduction.replay_cu < 1_400_000);
}

#[test]
fn v16_program_pr314_activation_fee_consent_extracts_unsigned_increase() {
    let reproduction = reproduce_activation_fee_consent([0x14; 32])
        .unwrap_or_else(|error| panic!("PR 314 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::ActivationFeeConsent);
    assert_eq!(reproduction.advertised_fee, 1);
    assert_eq!(reproduction.charged_fee, 1_000);
    assert_eq!(reproduction.unexpected_loss, 999);
    assert_eq!(reproduction.beneficiary_extraction, 999);
    assert_eq!(reproduction.insured_remainder, 1);
    assert!(reproduction.policy_replay_cu < 1_400_000);
    assert!(reproduction.activation_cu < 1_400_000);
}

#[test]
fn v16_program_pr310_bilateral_base_fee_consent_extracts_victim_fee() {
    for route in [TradeRoute::NoCpi, TradeRoute::BatchNoCpi] {
        let reproduction = reproduce_bilateral_base_fee_consent([0x10; 32], route)
            .unwrap_or_else(|error| panic!("PR 310 {route:?} no longer reproduces: {error}"));
        assert_eq!(reproduction.blocker, KnownBlocker::BilateralBaseFeeConsent);
        assert_eq!(reproduction.route, route);
        assert_eq!(reproduction.signed_fee_bps, 0);
        assert_eq!(reproduction.installed_fee_bps, 500);
        assert_eq!(reproduction.victim_loss, 100_000);
        assert_eq!(reproduction.beneficiary_profit, 100_000);
        assert_eq!(reproduction.insurance_extraction, 200_000);
        assert_eq!(reproduction.total_payout, 200_000_000);
        assert!(reproduction.open_cu < 1_400_000);
        assert!(reproduction.close_cu < 1_400_000);
    }
}

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
fn v16_program_pr338_delayed_trade_fee_policy_extracts_user_fee() {
    let reproduction = reproduce_delayed_trade_fee_policy_replay([0x38; 32])
        .unwrap_or_else(|error| panic!("PR 338 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::DelayedTradeFeePolicyReplay
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
fn v16_program_pr345_pr346_authority_aba_replays_drain_new_reserves() {
    for path in [
        AuthorityHandoffAbaPath::Market,
        AuthorityHandoffAbaPath::AssetInsuranceOperator,
    ] {
        let reproduction = reproduce_authority_handoff_aba_replay([0x45; 32], path)
            .unwrap_or_else(|error| panic!("PR 345/346 {path:?} no longer reproduces: {error}"));
        assert_eq!(
            reproduction.blocker,
            KnownBlocker::AuthorityHandoffAbaReplay
        );
        assert_eq!(reproduction.path, path);
        assert!(reproduction.control_withdrawal_blocked);
        assert_eq!(reproduction.reserve_before, 50_000);
        assert_eq!(reproduction.reserve_after, 0);
        assert_eq!(reproduction.attacker_extraction, 50_000);
        assert!(reproduction.replay_cu < 1_400_000);
        assert!(reproduction.withdrawal_cu < 1_400_000);
    }
}

#[test]
fn v16_program_pr347_delayed_resolve_policy_freezes_authenticated_mark() {
    let reproduction = reproduce_delayed_resolve_policy_replay([0x47; 32])
        .unwrap_or_else(|error| panic!("PR 347 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::DelayedResolvePolicyReplay
    );
    assert!(reproduction.replay_crank_blocked);
    assert_eq!(reproduction.control_price, 110);
    assert_eq!(reproduction.frozen_price, 100);
    assert_eq!(reproduction.victim_loss, 100_000);
    assert_eq!(reproduction.attacker_gain, 100_000);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.resolve_cu < 1_400_000);
}

#[test]
fn v16_program_pr353_prior_authority_resolve_crystallizes_victim_loss() {
    let reproduction = reproduce_resolve_authority_incarnation_replay([0x53; 32])
        .unwrap_or_else(|error| panic!("PR 353 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::ResolveAuthorityIncarnationReplay
    );
    assert_eq!(reproduction.control_price, 100);
    assert_eq!(reproduction.replay_price, 110);
    assert_eq!(reproduction.victim_loss, 100_000);
    assert_eq!(reproduction.winner_gain, 100_000);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.max_crank_cu < 1_400_000);
}

#[test]
fn v16_program_pr309_stale_close_drains_replacement_account_lamports() {
    let reproduction = reproduce_portfolio_close_incarnation_replay([0x09; 32])
        .unwrap_or_else(|error| panic!("PR 309 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::PortfolioCloseIncarnationReplay
    );
    assert!(reproduction.replacement_portfolio_id > reproduction.original_portfolio_id);
    assert_eq!(reproduction.drained_lamports, 1_000_000_000);
    assert_eq!(reproduction.market_lamport_gain, 1_000_000_000);
    assert!(reproduction.replay_cu < 1_400_000);
}

#[test]
fn v16_program_pr304_stale_matcher_grant_liquidates_reinitialized_portfolio() {
    let reproduction = reproduce_matcher_grant_portfolio_incarnation_replay([0x04; 32])
        .unwrap_or_else(|error| panic!("PR 304 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::MatcherGrantPortfolioIncarnationReplay
    );
    assert!(reproduction.replacement_portfolio_id > reproduction.original_portfolio_id);
    assert!(reproduction.control_trade_blocked);
    assert!(reproduction.liquidation_slot > 0);
    assert_eq!(
        reproduction.cranker_reward,
        u128::from(reproduction.extracted_reward)
    );
    assert_eq!(reproduction.cranker_reward, 15_835);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.max_cu < 1_400_000);
}

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
fn v16_program_pr303_stale_trades_liquidate_reinitialized_portfolio() {
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        for side in [
            PortfolioIncarnationTradeSide::AccountA,
            PortfolioIncarnationTradeSide::AccountB,
        ] {
            let reproduction =
                reproduce_trade_portfolio_incarnation_replay([0x03; 32], route, side)
                    .unwrap_or_else(|error| {
                        panic!("PR 303 {route:?}/{side:?} no longer reproduces: {error}")
                    });
            assert_eq!(
                reproduction.blocker,
                KnownBlocker::TradePortfolioIncarnationReplay
            );
            assert_eq!(reproduction.route, route);
            assert_eq!(reproduction.replacement_side, side);
            assert!(reproduction.replacement_portfolio_id > reproduction.original_portfolio_id);
            assert_eq!(reproduction.control_position_q, 0);
            assert!(reproduction.liquidation_slot > 0);
            assert_eq!(
                reproduction.cranker_reward,
                u128::from(reproduction.extracted_reward)
            );
            assert_eq!(reproduction.cranker_reward, 453);
            assert!(reproduction.replay_cu < 1_400_000);
            assert!(reproduction.max_cu < 1_400_000);
        }
    }
}

#[test]
fn v16_program_pr301_stale_pnl_conversion_pays_cranker_from_replacement() {
    let reproduction = reproduce_convert_portfolio_incarnation_replay([0x01; 32])
        .unwrap_or_else(|error| panic!("PR 301 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::ConvertPortfolioIncarnationReplay
    );
    assert!(reproduction.replacement_portfolio_id > reproduction.original_portfolio_id);
    assert_eq!(reproduction.released_pnl, 100);
    assert_eq!(reproduction.victim_loss, reproduction.cranker_extraction);
    assert_eq!(reproduction.victim_loss, 8);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.sync_cu < 1_400_000);
    assert!(reproduction.max_cu < 1_400_000);
}

#[test]
fn v16_program_pr278_stale_forfeit_discards_replacement_winner_payout() {
    let reproduction = reproduce_forfeit_portfolio_incarnation_replay([0x78; 32])
        .unwrap_or_else(|error| panic!("PR 278 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::ForfeitPortfolioIncarnationReplay
    );
    assert!(reproduction.replacement_portfolio_id > reproduction.original_portfolio_id);
    assert_eq!(reproduction.victim_loss, 100_000);
    assert_eq!(reproduction.stranded_vault, 100_000);
    assert!(reproduction.control_slab_closed);
    assert!(reproduction.replay_slab_blocked);
    assert!(reproduction.replay_cu < 1_400_000);
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
    assert_eq!(reproduction.victim_loss, 100_000);
    assert_eq!(reproduction.stranded_vault, 100_000);
    assert!(reproduction.control_slab_closed);
    assert!(reproduction.replay_slab_blocked);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.max_cu < 1_400_000);
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
fn v16_program_pr334_delayed_matcher_enable_extracts_lp_collateral() {
    let reproduction = reproduce_delayed_matcher_enable_replay([0x34; 32])
        .unwrap_or_else(|error| panic!("PR 334 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::DelayedMatcherEnableReplay
    );
    assert!(reproduction.control_fill_blocked);
    assert_eq!(reproduction.victim_loss, 1_000_000);
    assert_eq!(reproduction.attacker_gain, 1_000_000);
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
fn v16_program_pr351_backing_top_up_retry_funds_independent_winner() {
    let reproduction = reproduce_backing_top_up_retry_replay([0x35; 32])
        .unwrap_or_else(|error| panic!("PR 351 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::BackingTopUpRetryReplay);
    assert_eq!(reproduction.intended_contribution, 500);
    assert_eq!(reproduction.duplicate_loss, 500);
    assert_eq!(reproduction.beneficiary_extra_payout, 500);
    assert_eq!(reproduction.control_winner_payout, 2_500);
    assert_eq!(reproduction.replay_winner_payout, 3_000);
    assert!(reproduction.replay_cu < 1_400_000);
}

#[test]
fn v16_program_pr350_deposit_retry_funds_independent_winner() {
    let reproduction = reproduce_deposit_retry_replay([0x50; 32])
        .unwrap_or_else(|error| panic!("PR 350 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::DepositRetryReplay);
    assert_eq!(reproduction.intended_contribution, 500);
    assert_eq!(reproduction.duplicate_loss, 500);
    assert_eq!(reproduction.beneficiary_extra_payout, 500);
    assert_eq!(reproduction.control_winner_payout, 2_500);
    assert_eq!(reproduction.replay_winner_payout, 3_000);
    assert!(reproduction.replay_cu < 1_400_000);
}

#[test]
fn v16_program_pr299_stale_withdrawal_liquidates_reinitialized_portfolio() {
    let reproduction = reproduce_portfolio_incarnation_withdrawal([0x99; 32])
        .unwrap_or_else(|error| panic!("PR 299 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::PortfolioIncarnationWithdrawal
    );
    assert!(reproduction.new_portfolio_id > reproduction.old_portfolio_id);
    assert_eq!(reproduction.stale_withdrawal, 100_000_000);
    assert!(reproduction.restored_equity_surplus > 0);
    assert_eq!(
        reproduction.cranker_reward,
        u128::from(reproduction.extracted_reward)
    );
    assert!(reproduction.cranker_reward > 0);
    assert!(reproduction.replay_cu < 1_400_000);
}

#[test]
fn v16_program_pr305_stale_deposit_funds_reinitialized_portfolio_winner() {
    let reproduction = reproduce_portfolio_incarnation_deposit([0x05; 32])
        .unwrap_or_else(|error| panic!("PR 305 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::PortfolioIncarnationDeposit
    );
    assert!(reproduction.new_portfolio_id > reproduction.old_portfolio_id);
    assert_eq!(reproduction.stale_deposit, 100_000);
    assert_eq!(reproduction.beneficiary_extra_payout, 100_000);
    assert_eq!(reproduction.control_winner_payout, 300_000);
    assert_eq!(reproduction.replay_winner_payout, 400_000);
    assert!(reproduction.replay_cu < 1_400_000);
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

#[test]
fn v16_program_pr355_withdrawal_retry_liquidates_fresh_risk() {
    let reproduction = reproduce_withdrawal_retry_liquidation([0x55; 32])
        .unwrap_or_else(|error| panic!("PR 355 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::WithdrawalRetryLiquidation
    );
    assert_eq!(reproduction.intended_withdrawal, 50_000_000);
    assert_eq!(reproduction.duplicate_withdrawal, 50_000_000);
    assert!(reproduction.restored_equity_surplus > 0);
    assert_eq!(reproduction.cranker_reward, 7_917);
    assert_eq!(reproduction.extracted_reward, 7_917);
    assert!(reproduction.replay_cu < 1_400_000);
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
            220, 223, 224, 225, 231, 251, 253, 255, 260, 264, 265, 267, 271, 272, 273, 274, 275,
            276, 277, 278, 279, 280, 281, 282, 283, 285, 290, 294, 295, 296, 299, 301, 303, 304,
            305, 307, 309, 310, 311, 314, 315, 317, 318, 320, 321, 322, 325, 326, 328, 329, 331,
            332, 333, 334, 335, 336, 337, 338, 339, 340, 343, 344, 345, 346, 347, 349, 350, 351,
            353, 355, 356, 362, 365, 366, 367, 369, 380, 381
        ]
    );
    let missing = missing_prs();
    assert_eq!(
        missing.len(),
        21,
        "update the explicit evidence state when an executable adapter lands"
    );
    assert!(!missing.contains(&220));
    assert!(!missing.contains(&223));
    assert!(!missing.contains(&224));
    assert!(!missing.contains(&225));
    assert!(!missing.contains(&231));
    assert!(!missing.contains(&251));
    assert!(!missing.contains(&253));
    assert!(!missing.contains(&255));
    assert!(!missing.contains(&260));
    assert!(!missing.contains(&264));
    assert!(!missing.contains(&265));
    assert!(!missing.contains(&267));
    assert!(!missing.contains(&271));
    assert!(!missing.contains(&272));
    assert!(!missing.contains(&273));
    assert!(!missing.contains(&274));
    assert!(!missing.contains(&275));
    assert!(!missing.contains(&276));
    assert!(!missing.contains(&277));
    assert!(!missing.contains(&278));
    assert!(!missing.contains(&279));
    assert!(!missing.contains(&280));
    assert!(!missing.contains(&281));
    assert!(!missing.contains(&282));
    assert!(!missing.contains(&283));
    assert!(!missing.contains(&285));
    assert!(!missing.contains(&290));
    assert!(!missing.contains(&294));
    assert!(!missing.contains(&295));
    assert!(!missing.contains(&296));
    assert!(!missing.contains(&299));
    assert!(!missing.contains(&301));
    assert!(!missing.contains(&303));
    assert!(!missing.contains(&304));
    assert!(!missing.contains(&305));
    assert!(!missing.contains(&307));
    assert!(!missing.contains(&309));
    assert!(!missing.contains(&310));
    assert!(!missing.contains(&311));
    assert!(!missing.contains(&314));
    assert!(!missing.contains(&315));
    assert!(!missing.contains(&317));
    assert!(!missing.contains(&318));
    assert!(!missing.contains(&320));
    assert!(!missing.contains(&321));
    assert!(!missing.contains(&322));
    assert!(!missing.contains(&325));
    assert!(!missing.contains(&326));
    assert!(!missing.contains(&328));
    assert!(!missing.contains(&329));
    assert!(!missing.contains(&331));
    assert!(!missing.contains(&332));
    assert!(!missing.contains(&333));
    assert!(!missing.contains(&334));
    assert!(!missing.contains(&335));
    assert!(!missing.contains(&336));
    assert!(!missing.contains(&337));
    assert!(!missing.contains(&338));
    assert!(!missing.contains(&339));
    assert!(!missing.contains(&340));
    assert!(!missing.contains(&343));
    assert!(!missing.contains(&344));
    assert!(!missing.contains(&345));
    assert!(!missing.contains(&346));
    assert!(!missing.contains(&347));
    assert!(!missing.contains(&349));
    assert!(!missing.contains(&350));
    assert!(!missing.contains(&351));
    assert!(!missing.contains(&353));
    assert!(!missing.contains(&355));
    assert!(!missing.contains(&356));
    assert!(!missing.contains(&362));
    assert!(!missing.contains(&365));
    assert!(!missing.contains(&366));
    assert!(!missing.contains(&367));
    assert!(!missing.contains(&369));
    assert!(!missing.contains(&380));
    assert!(!missing.contains(&381));
}
