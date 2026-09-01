mod support;

use proptest::prelude::*;
use support::fuzz_model::{
    activation_fee_consent_seed_strategy, activation_retry_replay_seed_strategy,
    asset_generation_config_replay_strategy, asset_generation_replay_strategy,
    backing_fee_generation_replay_seed_strategy, backing_top_up_generation_replay_seed_strategy,
    backing_top_up_retry_replay_seed_strategy, bilateral_base_fee_consent_strategy,
    bilateral_fee_support_strategy, collateral_top_up_generation_replay_seed_strategy,
    composite_rounding_strategy, cpi_backing_fee_seed_strategy, cpi_base_fee_consent_strategy,
    cpi_caller_fee_strategy, forfeit_funding_erasure_seed_strategy,
    fractional_cap_settlement_seed_strategy, insurance_top_up_retry_replay_seed_strategy,
    insurance_withdrawal_generation_replay_seed_strategy, pending_ewma_inheritance_strategy,
    pending_ewma_target_override_strategy, pending_mark_fee_reward_seed_strategy,
    prospective_funding_rewrite_strategy, rebalance_funding_erasure_seed_strategy,
    reclaimable_ewma_fee_strategy, reproduce_bilateral_fee_support,
    reproduce_composite_oracle_rounding, reproduce_forfeit_funding_erasure,
    reproduce_fractional_cap_settlement, reproduce_pending_ewma_inheritance,
    reproduce_pending_ewma_target_override, reproduce_pending_mark_fee_reward,
    reproduce_prospective_funding_rewrite, reproduce_rebalance_funding_erasure,
    reproduce_reclaimable_ewma_fee, reproduce_resolve_before_committed_accrual,
    reproduce_rounded_funding_omission, reproduce_trade_driven_liquidation_reward,
    reproduce_trade_funding_erasure, reproduce_unstaged_mark_target,
    resolve_before_committed_accrual_seed_strategy, resolve_generation_replay_seed_strategy,
    rounded_funding_seed_strategy, run_abandoned_asset_force_close_oracle,
    run_cure_position_episode_replay_probe, run_drain_reduce_retire_route_oracle,
    run_permissionless_stale_resolution_terminal_oracle, run_recovery_forfeit_route_oracle,
    run_recovery_restart_trade_route_oracle, run_scenario, run_value_withdrawal_route_oracle,
    scenario_strategy, target_staging_strategy, terminal_dust_payout_protection_strategy,
    trade_driven_liquidation_reward_strategy, trade_funding_erasure_strategy,
    trade_retry_replay_strategy, verify_activation_fee_consent, verify_attributed_pnl_roundtrip,
    verify_bilateral_base_fee_consent, verify_convert_retry_replay_protection,
    verify_counterparty_encumbrance_route_matrix, verify_cpi_backing_fee_consent,
    verify_cpi_base_fee_consent, verify_cpi_caller_fee_protection,
    verify_exact_stock_reconciliation_lifecycle, verify_positive_claim_bound_attribution_lifecycle,
    verify_resolved_claim_quote_delta, verify_resolved_receipt_split_topups,
    verify_source_credit_rate_lifecycle, verify_terminal_dust_payout_protection,
    verify_underfunded_authority_resolve_claim_orders, Action, AssetGenerationConfigPath, HintMode,
    Scenario, SmallMarketConfig, SubstitutionKind, TradeRoute,
};
use support::invariant_discovery::{
    discover_accrual_ordering_violations, discover_active_leg_currentness_violation,
    discover_asset_generation_replay, discover_asset_generation_replays,
    discover_authority_incarnation_replays, discover_backing_expiry_consumer_boundaries,
    discover_backing_expiry_consumer_boundary, discover_backing_expiry_trade_route_boundaries,
    discover_backing_expiry_trade_route_boundary, discover_backing_expiry_violation,
    discover_backing_provider_consent_violations, discover_bidirectional_superseded_intents,
    discover_bilateral_mark_fee_violations, discover_composite_rounding_violations,
    discover_cross_domain_b_violation, discover_cross_domain_backing_single_use,
    discover_cross_domain_insurance_violation, discover_cross_domain_rounding_exit_locks,
    discover_cross_route_insurance_top_up_retry, discover_cross_route_trade_intent_retry,
    discover_debited_intent_retry, discover_fee_consent_violations,
    discover_fee_redirect_supersession, discover_flat_source_lien_bounded_exits,
    discover_funded_role_seizures, discover_intent_retries, discover_intent_retry,
    discover_liquidation_share_supersession, discover_maintenance_share_supersession,
    discover_mark_movement_reserve_violations, discover_market_incarnation_replays,
    discover_matcher_revocation_terminal_loss, discover_multi_segment_accrual_ordering_violations,
    discover_observation_omission_violation, discover_oracle_supersession_terminal_losses,
    discover_pending_mark_admission_violations, discover_pending_mark_fee_ordering,
    discover_pending_mark_inheritance_violations, discover_pending_target_override_violations,
    discover_pending_zero_move_terminal_ordering, discover_portfolio_incarnation_replays,
    discover_position_episode_replays, discover_prospective_accrual_violations,
    discover_resolve_policy_bounded_liveness, discover_retained_maturity_boundaries,
    discover_retained_maturity_boundary, discover_same_transaction_intent_retry,
    discover_shutdown_catchup_liveness, discover_shutdown_commit_ordering,
    discover_source_fee_consent_violations, discover_source_lien_reversal_exit_locks,
    discover_terminal_commit_ordering, discover_terminal_dust_violations,
    discover_terminal_generation_replay, discover_trade_driven_liquidation_violations,
    discover_trade_intent_retry_terminal, verify_adl_force_close_clamp_matrix,
    verify_adl_reduction_clamp_matrix, verify_composite_time_coherence,
    verify_dual_adl_force_close_clamp_matrix, verify_dual_adl_liquidation_sizing,
    verify_dual_adl_prefixes, verify_dual_adl_recovery_forfeit_matrix,
    verify_fractional_movement_convergence, verify_hybrid_terminal_time_coherence,
    verify_matcher_mutation_order_safety, verify_multi_asset_adl_liquidation_permutations,
    verify_resolved_adl_close_orders, verify_stale_cohort_exact_reversal,
    verify_stale_cohort_novation_guards, verify_three_asset_locked_loss_liquidation_permutations,
    AccrualOrderingKind, ActiveLegOrder, AdlForceCloseAccountOrder, AdlReductionBoundary,
    AssetIntentKind, AuthorityIntentKind, BackingExpiryCase, BackingExpiryLanding,
    BackingProviderConsentOrder, CompositeRoundingScale, CrossDomainRoundingOrder,
    DiscoveryTradeRoute, EqualRiskAssetOrder, ExpiredBackingConsumerDiscovery,
    ExpiredBackingConsumerKind, ExpiredBackingTradeRouteDiscovery, FeeConsentKind,
    FlatSourceLienEscapeRoute, FollowupLiquidationSelection, FundedRoleKind, MarketIntentKind,
    PendingMarkSource, PortfolioIntentKind, PositionEpisodeKind, ProspectiveAccrualRoute,
    RecoveryForfeitBudget, ResolvedAdlCloseOrder, RetainedMaturityDiscovery, RetainedMaturityKind,
    RetryIntentKind, SourceFeeConsentKind, SourceFeeConsentRole, SourceLienReversalExitRoute,
    StaleCohortRoute, SupersededIntentKind, SupersessionPayloadOrder, TerminalGenerationKind,
    TradeDrivenMarkMode,
};

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value != 0)
        .unwrap_or(default)
}

#[path = "invariants/stateful/inv_001_market_incarnation_binding.rs"]
mod inv_001_market_incarnation_binding;

#[path = "invariants/stateful/inv_002_asset_generation_binding.rs"]
mod inv_002_asset_generation_binding;

#[path = "invariants/stateful/inv_003_portfolio_incarnation_binding.rs"]
mod inv_003_portfolio_incarnation_binding;

#[path = "invariants/stateful/inv_004_position_episode_binding.rs"]
mod inv_004_position_episode_binding;

#[path = "invariants/stateful/inv_005_authority_incarnation_binding.rs"]
mod inv_005_authority_incarnation_binding;

#[path = "invariants/stateful/inv_008_intent_uniqueness_and_bounded_replay.rs"]
mod inv_008_intent_uniqueness_and_bounded_replay;

#[path = "invariants/stateful/inv_010_out_of_order_safety.rs"]
mod inv_010_out_of_order_safety;

#[path = "invariants/stateful/inv_013_destructive_consent_scope.rs"]
mod inv_013_destructive_consent_scope;

#[path = "invariants/stateful/inv_014_delayed_policy_and_policy_epoch_safety.rs"]
mod inv_014_delayed_policy_and_policy_epoch_safety;

#[path = "invariants/stateful/inv_017_signer_writable_role_and_account_alias_safety.rs"]
mod inv_017_signer_writable_role_and_account_alias_safety;

#[path = "invariants/stateful/inv_020_authenticated_clock_slot_and_oracle_provenance.rs"]
mod inv_020_authenticated_clock_slot_and_oracle_provenance;

#[path = "invariants/stateful/inv_024_attributed_quote_value_conservation.rs"]
mod inv_024_attributed_quote_value_conservation;

#[path = "invariants/stateful/inv_025_exact_stock_reconciliation.rs"]
mod inv_025_exact_stock_reconciliation;

#[path = "invariants/stateful/inv_026_reservation_and_encumbrance_conservation.rs"]
mod inv_026_reservation_and_encumbrance_conservation;

#[path = "invariants/stateful/inv_027_protected_principal_seniority.rs"]
mod inv_027_protected_principal_seniority;

#[path = "invariants/stateful/inv_028_source_domain_realizability_cap.rs"]
mod inv_028_source_domain_realizability_cap;

#[path = "invariants/stateful/inv_029_positive_claim_bounds_never_understate.rs"]
mod inv_029_positive_claim_bounds_never_understate;

#[path = "invariants/stateful/inv_030_credit_rate_determinism_and_fail_closed_behavior.rs"]
mod inv_030_credit_rate_determinism_and_fail_closed_behavior;

#[path = "invariants/stateful/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs"]
mod inv_031_no_double_use_of_claim_backing_or_insurance_atoms;

#[path = "invariants/stateful/inv_034_domain_and_instance_isolation.rs"]
mod inv_034_domain_and_instance_isolation;

#[path = "invariants/stateful/inv_035_no_global_b_pool_residuals_remain_local.rs"]
mod inv_035_no_global_b_pool_residuals_remain_local;

#[path = "invariants/stateful/inv_036_fee_destination_and_policy_version_integrity.rs"]
mod inv_036_fee_destination_and_policy_version_integrity;

#[path = "invariants/stateful/inv_037_exact_residual_partition.rs"]
mod inv_037_exact_residual_partition;

#[path = "invariants/stateful/inv_038_rounding_and_ratio_conservation.rs"]
mod inv_038_rounding_and_ratio_conservation;

#[path = "invariants/stateful/inv_039_pending_loss_obligation_durability.rs"]
mod inv_039_pending_loss_obligation_durability;

#[path = "invariants/stateful/inv_041_deterministic_allocation_and_caller_order_independence.rs"]
mod inv_041_deterministic_allocation_and_caller_order_independence;

#[path = "invariants/stateful/inv_045_no_free_mark_movement.rs"]
mod inv_045_no_free_mark_movement;

#[path = "invariants/stateful/inv_046_trade_availability_without_unsafe_mark_admission.rs"]
mod inv_046_trade_availability_without_unsafe_mark_admission;

#[path = "invariants/stateful/inv_047_equivalent_route_semantics.rs"]
mod inv_047_equivalent_route_semantics;

#[path = "invariants/stateful/inv_050_cross_zero_decomposition.rs"]
mod inv_050_cross_zero_decomposition;

#[path = "invariants/stateful/inv_052_split_merge_invariance.rs"]
mod inv_052_split_merge_invariance;

#[path = "invariants/stateful/inv_053_full_health_recertification_equivalence.rs"]
mod inv_053_full_health_recertification_equivalence;

#[path = "invariants/stateful/inv_055_state_indexed_admission.rs"]
mod inv_055_state_indexed_admission;

#[path = "invariants/stateful/inv_061_deterministic_bounded_liquidation.rs"]
mod inv_061_deterministic_bounded_liquidation;

#[path = "invariants/stateful/inv_063_backing_expiry_normalization.rs"]
mod inv_063_backing_expiry_normalization;

#[path = "invariants/stateful/inv_065_reset_recovery_and_retired_state_isolation.rs"]
mod inv_065_reset_recovery_and_retired_state_isolation;

#[path = "invariants/stateful/inv_066_resolved_payout_fairness_and_order_independence.rs"]
mod inv_066_resolved_payout_fairness_and_order_independence;

#[path = "invariants/stateful/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs"]
mod inv_067_terminal_payout_completeness_and_exact_once_settlement;

#[path = "invariants/stateful/inv_068_receipt_uniqueness_and_monotonic_topups.rs"]
mod inv_068_receipt_uniqueness_and_monotonic_topups;

#[path = "invariants/stateful/inv_069_terminal_normalization_and_retirement.rs"]
mod inv_069_terminal_normalization_and_retirement;

#[path = "invariants/stateful/inv_071_crank_progress.rs"]
mod inv_071_crank_progress;

#[path = "invariants/stateful/inv_072_order_robust_crankability.rs"]
mod inv_072_order_robust_crankability;

#[path = "invariants/stateful/inv_074_scope_locality.rs"]
mod inv_074_scope_locality;

#[path = "invariants/stateful/inv_076_close_drift_residual_durability_and_finalization_atomicity.rs"]
mod inv_076_close_drift_residual_durability_and_finalization_atomicity;

#[path = "invariants/stateful/inv_078_permissionless_recovery_coverage.rs"]
mod inv_078_permissionless_recovery_coverage;

#[path = "invariants/stateful/inv_081_success_state_validity_over_complete_public_routes.rs"]
mod inv_081_success_state_validity_over_complete_public_routes;

#[path = "invariants/stateful/inv_082_state_indexed_liveness_theorem.rs"]
mod inv_082_state_indexed_liveness_theorem;

#[path = "invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs"]
mod inv_086_reference_model_and_deployed_transition_equivalence;

#[path = "invariants/stateful/inv_088_global_summaries_are_not_account_local_proofs.rs"]
mod inv_088_global_summaries_are_not_account_local_proofs;
