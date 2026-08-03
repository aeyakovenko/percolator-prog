mod support;

use proptest::prelude::*;
use support::fuzz_model::{
    activation_fee_consent_seed_strategy, activation_retry_replay_seed_strategy,
    asset_generation_config_replay_strategy, asset_generation_mark_replay_strategy,
    asset_generation_replay_strategy, authority_handoff_aba_replay_strategy,
    backing_fee_consent_replay_strategy, backing_fee_generation_replay_seed_strategy,
    backing_top_up_generation_replay_seed_strategy, backing_top_up_retry_replay_seed_strategy,
    bilateral_base_fee_consent_strategy, bilateral_fee_support_strategy,
    collateral_top_up_generation_replay_seed_strategy, composite_rounding_strategy,
    composite_time_skew_seed_strategy, convert_portfolio_incarnation_replay_seed_strategy,
    cpi_backing_fee_seed_strategy, cpi_caller_fee_strategy,
    cross_domain_b_settlement_seed_strategy, cross_domain_backing_seed_strategy,
    cross_margin_insurance_drain_seed_strategy, delayed_asset_authority_revival_seed_strategy,
    delayed_backing_fee_policy_replay_seed_strategy,
    delayed_fee_redirect_policy_replay_seed_strategy,
    delayed_liquidation_policy_replay_seed_strategy,
    delayed_maintenance_policy_replay_seed_strategy, delayed_matcher_enable_replay_seed_strategy,
    delayed_oracle_intent_replay_strategy, delayed_resolve_policy_replay_seed_strategy,
    delayed_trade_fee_policy_replay_seed_strategy, deposit_retry_replay_seed_strategy,
    fee_redirect_generation_replay_seed_strategy, forfeit_funding_erasure_seed_strategy,
    forfeit_market_generation_replay_seed_strategy,
    forfeit_portfolio_incarnation_replay_seed_strategy, fractional_cap_settlement_seed_strategy,
    insurance_top_up_retry_replay_seed_strategy,
    insurance_withdrawal_generation_replay_seed_strategy,
    liquidation_policy_generation_replay_seed_strategy,
    maintenance_policy_generation_replay_seed_strategy, market_incarnation_deposit_seed_strategy,
    matcher_grant_market_generation_replay_seed_strategy,
    matcher_grant_portfolio_incarnation_replay_seed_strategy, pending_ewma_inheritance_strategy,
    pending_ewma_target_override_strategy, pending_mark_fee_reward_seed_strategy,
    portfolio_close_incarnation_replay_seed_strategy, portfolio_incarnation_deposit_seed_strategy,
    portfolio_incarnation_withdrawal_seed_strategy, prospective_funding_rewrite_strategy,
    rebalance_funding_erasure_seed_strategy, reclaimable_ewma_fee_strategy,
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
    reproduce_liquidation_policy_generation_replay, reproduce_maintenance_policy_generation_replay,
    reproduce_market_incarnation_deposit, reproduce_matcher_grant_market_generation_replay,
    reproduce_matcher_grant_portfolio_incarnation_replay, reproduce_pending_ewma_inheritance,
    reproduce_pending_ewma_target_override, reproduce_pending_mark_fee_reward,
    reproduce_portfolio_close_incarnation_replay, reproduce_portfolio_incarnation_deposit,
    reproduce_portfolio_incarnation_withdrawal, reproduce_prospective_funding_rewrite,
    reproduce_rebalance_funding_erasure, reproduce_reclaimable_ewma_fee,
    reproduce_resolve_authority_incarnation_replay, reproduce_resolve_before_committed_accrual,
    reproduce_resolve_generation_replay, reproduce_rounded_funding_omission,
    reproduce_shutdown_generation_replay, reproduce_terminal_dust_payout_erasure,
    reproduce_trade_driven_liquidation_reward, reproduce_trade_fee_market_generation_replay,
    reproduce_trade_funding_erasure, reproduce_trade_portfolio_incarnation_replay,
    reproduce_trade_retry_replay, reproduce_unstaged_mark_target,
    reproduce_withdrawal_retry_liquidation, resolve_authority_incarnation_replay_seed_strategy,
    resolve_before_committed_accrual_seed_strategy, resolve_generation_replay_seed_strategy,
    rounded_funding_seed_strategy, run_scenario, scenario_strategy,
    shutdown_generation_replay_seed_strategy, target_staging_strategy,
    terminal_dust_payout_erasure_strategy, trade_driven_liquidation_reward_strategy,
    trade_fee_market_generation_replay_seed_strategy, trade_funding_erasure_strategy,
    trade_portfolio_incarnation_replay_strategy, trade_retry_replay_strategy,
    withdrawal_retry_liquidation_seed_strategy,
};
use support::invariant_discovery::{
    discover_accrual_ordering_violations, discover_asset_generation_replays,
    discover_authority_incarnation_replays, discover_backing_expiry_violation,
    discover_backing_provider_consent_violations, discover_bilateral_mark_fee_violations,
    discover_composite_rounding_violations, discover_composite_time_coherence_violation,
    discover_cross_domain_b_violation, discover_cross_domain_backing_violation,
    discover_cross_domain_insurance_violation, discover_expired_backing_consumers,
    discover_fee_consent_violations, discover_fractional_movement_stall,
    discover_full_refresh_omission_violation, discover_funded_role_seizures,
    discover_hybrid_terminal_snapshot_violation, discover_intent_retries,
    discover_mark_movement_reserve_violations, discover_market_incarnation_replays,
    discover_matcher_mutation_order_violation, discover_observation_omission_violation,
    discover_pending_mark_admission_violations, discover_pending_mark_fee_ordering,
    discover_pending_mark_inheritance_violations, discover_pending_target_override_violations,
    discover_portfolio_incarnation_replays, discover_position_episode_replays,
    discover_prospective_accrual_violations, discover_resolved_adl_close_locks,
    discover_retained_maturity_terminal_locks, discover_shutdown_commit_ordering,
    discover_source_fee_consent_violations, discover_source_lien_reversal_exit_locks,
    discover_stale_cohort_novations, discover_superseded_intents,
    discover_terminal_commit_ordering, discover_terminal_dust_violations,
    discover_terminal_generation_replay, discover_trade_driven_liquidation_violations,
    AccrualOrderingKind, AssetIntentKind, AuthorityIntentKind, BackingExpiryCase,
    BackingProviderConsentOrder, CompositeRoundingScale, DiscoveryTradeRoute,
    ExpiredBackingConsumerKind, FeeConsentKind, FundedRoleKind, MarketIntentKind,
    PendingMarkSource, PortfolioIntentKind, PositionEpisodeKind, ProspectiveAccrualRoute,
    ResolvedAdlCloseOrder, RetainedMaturityKind, RetryIntentKind, SourceFeeConsentKind,
    SourceLienReversalExitRoute, StaleCohortRoute, SupersededIntentKind, TerminalGenerationKind,
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

#[path = "invariants/stateful/inv_014_delayed_policy_and_policy_epoch_safety.rs"]
mod inv_014_delayed_policy_and_policy_epoch_safety;

#[path = "invariants/stateful/inv_020_authenticated_clock_slot_and_oracle_provenance.rs"]
mod inv_020_authenticated_clock_slot_and_oracle_provenance;

#[path = "invariants/stateful/inv_028_source_domain_realizability_cap.rs"]
mod inv_028_source_domain_realizability_cap;

#[path = "invariants/stateful/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs"]
mod inv_031_no_double_use_of_claim_backing_or_insurance_atoms;

#[path = "invariants/stateful/inv_034_domain_and_instance_isolation.rs"]
mod inv_034_domain_and_instance_isolation;

#[path = "invariants/stateful/inv_035_no_global_b_pool_residuals_remain_local.rs"]
mod inv_035_no_global_b_pool_residuals_remain_local;

#[path = "invariants/stateful/inv_036_fee_destination_and_policy_version_integrity.rs"]
mod inv_036_fee_destination_and_policy_version_integrity;

#[path = "invariants/stateful/inv_038_rounding_and_ratio_conservation.rs"]
mod inv_038_rounding_and_ratio_conservation;

#[path = "invariants/stateful/inv_039_pending_loss_obligation_durability.rs"]
mod inv_039_pending_loss_obligation_durability;

#[path = "invariants/stateful/inv_045_no_free_mark_movement.rs"]
mod inv_045_no_free_mark_movement;

#[path = "invariants/stateful/inv_053_full_health_recertification_equivalence.rs"]
mod inv_053_full_health_recertification_equivalence;

#[path = "invariants/stateful/inv_061_deterministic_bounded_liquidation.rs"]
mod inv_061_deterministic_bounded_liquidation;

#[path = "invariants/stateful/inv_063_backing_expiry_normalization.rs"]
mod inv_063_backing_expiry_normalization;

#[path = "invariants/stateful/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs"]
mod inv_067_terminal_payout_completeness_and_exact_once_settlement;

#[path = "invariants/stateful/inv_081_success_state_validity_over_complete_public_routes.rs"]
mod inv_081_success_state_validity_over_complete_public_routes;
