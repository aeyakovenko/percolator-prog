mod support;

use support::{
    blocker_corpus::{blocker_scenarios, fixed_blocker_scenarios},
    fuzz_model::{
        reproduce_activation_retry_replay, reproduce_asset_generation_config_replay,
        reproduce_asset_generation_mark_replay, reproduce_asset_generation_trade_replay,
        reproduce_authority_handoff_aba_replay, reproduce_backing_fee_consent_replay,
        reproduce_backing_fee_generation_replay, reproduce_backing_top_up_generation_replay,
        reproduce_backing_top_up_retry_replay, reproduce_bilateral_fee_support,
        reproduce_collateral_top_up_generation_replay, reproduce_composite_oracle_rounding,
        reproduce_composite_oracle_time_skew, reproduce_convert_portfolio_incarnation_replay,
        reproduce_cross_domain_b_settlement, reproduce_cross_domain_backing_double_spend,
        reproduce_cross_margin_insurance_drain, reproduce_delayed_asset_authority_revival,
        reproduce_delayed_backing_fee_policy_replay, reproduce_delayed_fee_redirect_policy_replay,
        reproduce_delayed_liquidation_policy_replay, reproduce_delayed_maintenance_policy_replay,
        reproduce_delayed_matcher_enable_replay, reproduce_delayed_oracle_intent_replay,
        reproduce_delayed_resolve_policy_replay, reproduce_deposit_retry_replay,
        reproduce_fee_redirect_generation_replay, reproduce_forfeit_funding_erasure,
        reproduce_forfeit_market_generation_replay, reproduce_forfeit_portfolio_incarnation_replay,
        reproduce_fractional_cap_settlement, reproduce_insurance_top_up_retry_replay,
        reproduce_insurance_withdrawal_generation_replay,
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
        reproduce_trade_driven_liquidation_reward, reproduce_trade_funding_erasure,
        reproduce_trade_portfolio_incarnation_replay, reproduce_trade_retry_replay,
        reproduce_unstaged_mark_target, reproduce_withdrawal_retry_liquidation, run_scenario,
        verify_activation_fee_consent, verify_bilateral_base_fee_consent,
        verify_cpi_backing_fee_consent, verify_cpi_caller_fee_protection,
        verify_delayed_trade_fee_policy_nonextraction,
        verify_trade_fee_market_generation_nonextraction, AssetGenerationConfigPath,
        AssetGenerationMarkPath, AuthorityHandoffAbaPath, BackingFeeConsentOrder, BilateralFeeMode,
        CompositeRoundingCase, DelayedOracleIntentPath, KnownBlocker,
        PortfolioIncarnationTradeSide, PostExpiryBackingCase, Scenario, TargetStagingCase,
        TradeDrivenLiquidationMode, TradeRoute,
    },
    invariant_discovery::{
        discover_expired_backing_consumers, discover_multi_segment_accrual_ordering_violations,
        discover_pending_zero_move_terminal_ordering, discover_retained_maturity_terminal_locks,
        discover_shutdown_catchup_liveness, discover_shutdown_commit_ordering, AccrualOrderingKind,
        ExpiredBackingConsumerKind, RetainedMaturityKind,
    },
    open_lof_manifest::{missing_prs, quarantined_prs, validate_manifest},
};

#[path = "invariants/public_sbf/inv_001_market_incarnation_binding.rs"]
mod inv_001_market_incarnation_binding;

#[path = "invariants/public_sbf/inv_002_asset_generation_binding.rs"]
mod inv_002_asset_generation_binding;

#[path = "invariants/public_sbf/inv_003_portfolio_incarnation_binding.rs"]
mod inv_003_portfolio_incarnation_binding;

#[path = "invariants/public_sbf/inv_005_authority_incarnation_binding.rs"]
mod inv_005_authority_incarnation_binding;

#[path = "invariants/public_sbf/inv_008_intent_uniqueness_and_bounded_replay.rs"]
mod inv_008_intent_uniqueness_and_bounded_replay;

#[path = "invariants/public_sbf/inv_014_delayed_policy_and_policy_epoch_safety.rs"]
mod inv_014_delayed_policy_and_policy_epoch_safety;

#[path = "invariants/public_sbf/inv_020_authenticated_clock_slot_and_oracle_provenance.rs"]
mod inv_020_authenticated_clock_slot_and_oracle_provenance;

#[path = "invariants/public_sbf/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs"]
mod inv_031_no_double_use_of_claim_backing_or_insurance_atoms;

#[path = "invariants/public_sbf/inv_034_domain_and_instance_isolation.rs"]
mod inv_034_domain_and_instance_isolation;

#[path = "invariants/public_sbf/inv_035_no_global_b_pool_residuals_remain_local.rs"]
mod inv_035_no_global_b_pool_residuals_remain_local;

#[path = "invariants/public_sbf/inv_036_fee_destination_and_policy_version_integrity.rs"]
mod inv_036_fee_destination_and_policy_version_integrity;

#[path = "invariants/public_sbf/inv_038_rounding_and_ratio_conservation.rs"]
mod inv_038_rounding_and_ratio_conservation;

#[path = "invariants/public_sbf/inv_039_pending_loss_obligation_durability.rs"]
mod inv_039_pending_loss_obligation_durability;

#[path = "invariants/public_sbf/inv_045_no_free_mark_movement.rs"]
mod inv_045_no_free_mark_movement;

#[path = "invariants/public_sbf/inv_053_full_health_recertification_equivalence.rs"]
mod inv_053_full_health_recertification_equivalence;

#[path = "invariants/public_sbf/inv_063_backing_expiry_normalization.rs"]
mod inv_063_backing_expiry_normalization;

#[path = "invariants/public_sbf/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs"]
mod inv_067_terminal_payout_completeness_and_exact_once_settlement;

#[path = "invariants/public_sbf/inv_079_public_reachability_evidence.rs"]
mod inv_079_public_reachability_evidence;

#[path = "invariants/public_sbf/inv_081_success_state_validity_over_complete_public_routes.rs"]
mod inv_081_success_state_validity_over_complete_public_routes;

#[path = "invariants/public_sbf/inv_086_reference_model_and_deployed_transition_equivalence.rs"]
mod inv_086_reference_model_and_deployed_transition_equivalence;
