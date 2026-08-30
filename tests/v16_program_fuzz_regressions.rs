mod support;

use support::{
    blocker_corpus::{blocker_scenarios, fixed_blocker_scenarios},
    fuzz_model::{
        reproduce_bilateral_fee_support, reproduce_composite_oracle_rounding,
        reproduce_cross_domain_b_settlement, reproduce_forfeit_funding_erasure,
        reproduce_fractional_cap_settlement, reproduce_omitted_rescue_liquidation,
        reproduce_pending_ewma_inheritance, reproduce_pending_ewma_target_override,
        reproduce_pending_mark_fee_reward, reproduce_post_expiry_backing_fee,
        reproduce_prospective_funding_rewrite, reproduce_rebalance_funding_erasure,
        reproduce_reclaimable_ewma_fee, reproduce_resolve_before_committed_accrual,
        reproduce_rounded_funding_omission, reproduce_trade_driven_liquidation_reward,
        reproduce_trade_funding_erasure, reproduce_unstaged_mark_target, run_scenario,
        verify_activation_fee_consent, verify_bilateral_base_fee_consent,
        verify_convert_retry_replay_protection, verify_cpi_backing_fee_consent,
        verify_cpi_base_fee_consent, verify_cpi_caller_fee_protection,
        verify_terminal_dust_payout_protection, BilateralFeeMode, CompositeRoundingCase,
        KnownBlocker, PostExpiryBackingCase, Scenario, TargetStagingCase,
        TradeDrivenLiquidationMode, TradeRoute,
    },
    invariant_discovery::{
        discover_asset_generation_replay, discover_cross_domain_backing_single_use,
        discover_cross_route_insurance_top_up_retry, discover_cross_route_trade_intent_retries,
        discover_debited_intent_retries, discover_expired_backing_consumers,
        discover_intent_retries, discover_intent_retry, discover_liquidation_share_supersession,
        discover_maintenance_share_supersession, discover_matcher_revocation_terminal_loss,
        discover_multi_segment_accrual_ordering_violations,
        discover_oracle_supersession_terminal_losses, discover_pending_zero_move_terminal_ordering,
        discover_retained_maturity_terminal_locks, discover_same_transaction_intent_retries,
        discover_shutdown_catchup_liveness, discover_shutdown_commit_ordering,
        discover_superseded_intents, discover_trade_intent_retry_terminals,
        verify_composite_time_coherence, verify_matcher_mutation_order_safety, AccrualOrderingKind,
        AssetIntentKind, ExpiredBackingConsumerKind, RetainedMaturityKind, RetryIntentKind,
        SupersededIntentKind,
    },
    open_lof_manifest::{
        certified_prs, missing_prs, nonqualifying_prs, quarantined_prs, validate_manifest,
    },
};

#[path = "invariants/public_sbf/inv_001_market_incarnation_binding.rs"]
mod inv_001_market_incarnation_binding;

#[path = "invariants/public_sbf/inv_002_asset_generation_binding.rs"]
mod inv_002_asset_generation_binding;

#[path = "invariants/public_sbf/inv_003_portfolio_incarnation_binding.rs"]
mod inv_003_portfolio_incarnation_binding;

#[path = "invariants/public_sbf/inv_005_authority_incarnation_binding.rs"]
mod inv_005_authority_incarnation_binding;

#[path = "invariants/public_sbf/inv_006_program_chain_message_type_and_version_binding.rs"]
mod inv_006_program_chain_message_type_and_version_binding;

#[path = "invariants/public_sbf/inv_007_no_aba_reuse.rs"]
mod inv_007_no_aba_reuse;

#[path = "invariants/public_sbf/inv_008_intent_uniqueness_and_bounded_replay.rs"]
mod inv_008_intent_uniqueness_and_bounded_replay;

#[path = "invariants/public_sbf/inv_013_destructive_consent_scope.rs"]
mod inv_013_destructive_consent_scope;

#[path = "invariants/public_sbf/inv_014_delayed_policy_and_policy_epoch_safety.rs"]
mod inv_014_delayed_policy_and_policy_epoch_safety;

#[path = "invariants/public_sbf/inv_015_account_ownership_layout_discriminator_and_length_validity.rs"]
mod inv_015_account_ownership_layout_discriminator_and_length_validity;

#[path = "invariants/public_sbf/inv_020_authenticated_clock_slot_and_oracle_provenance.rs"]
mod inv_020_authenticated_clock_slot_and_oracle_provenance;

#[path = "invariants/public_sbf/inv_022_instruction_decoding_and_schema_upgrade_safety.rs"]
mod inv_022_instruction_decoding_and_schema_upgrade_safety;

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
