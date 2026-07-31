mod support;

use proptest::prelude::*;
use support::fuzz_model::{
    asset_generation_config_replay_strategy, asset_generation_mark_replay_strategy,
    asset_generation_replay_strategy, composite_rounding_strategy,
    composite_time_skew_seed_strategy, cpi_backing_fee_seed_strategy, cpi_caller_fee_strategy,
    cross_domain_b_settlement_seed_strategy, cross_domain_backing_seed_strategy,
    cross_margin_insurance_drain_seed_strategy, forfeit_funding_erasure_seed_strategy,
    fractional_cap_settlement_seed_strategy, omitted_rescue_seed_strategy,
    pending_ewma_inheritance_strategy, pending_ewma_target_override_strategy,
    pending_mark_fee_reward_seed_strategy, post_expiry_backing_case_strategy,
    rebalance_funding_erasure_seed_strategy, reclaimable_ewma_fee_strategy,
    reproduce_asset_generation_config_replay, reproduce_asset_generation_mark_replay,
    reproduce_asset_generation_trade_replay, reproduce_composite_oracle_rounding,
    reproduce_composite_oracle_time_skew, reproduce_cpi_backing_fee_siphon,
    reproduce_cpi_caller_fee_siphon, reproduce_cross_domain_b_settlement,
    reproduce_cross_domain_backing_double_spend, reproduce_cross_margin_insurance_drain,
    reproduce_forfeit_funding_erasure, reproduce_fractional_cap_settlement,
    reproduce_omitted_rescue_liquidation, reproduce_pending_ewma_inheritance,
    reproduce_pending_ewma_target_override, reproduce_pending_mark_fee_reward,
    reproduce_post_expiry_backing_fee, reproduce_rebalance_funding_erasure,
    reproduce_reclaimable_ewma_fee, reproduce_rounded_funding_omission,
    reproduce_terminal_dust_payout_erasure, reproduce_trade_driven_liquidation_reward,
    reproduce_trade_funding_erasure, reproduce_trade_retry_replay, reproduce_unstaged_mark_target,
    rounded_funding_seed_strategy, run_scenario, scenario_strategy, target_staging_strategy,
    terminal_dust_payout_erasure_strategy, trade_driven_liquidation_reward_strategy,
    trade_funding_erasure_strategy, trade_retry_replay_strategy,
};

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value != 0)
        .unwrap_or(default)
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/v16_program_stateful_fuzz.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_stateful_public_interface_fuzz(
        scenario in scenario_strategy(env_usize("PERCOLATOR_FUZZ_ACTIONS", 12))
    ) {
        let serialized = serde_json::to_string_pretty(&scenario).unwrap();
        let result = run_scenario(&scenario);
        prop_assert!(result.is_ok(), "stateful public-interface scenario failed: {}\n{}",
            result.unwrap_err(), serialized);
    }

    #[test]
    fn v16_program_pr367_post_expiry_backing_fee_fuzz(
        (seed, case) in post_expiry_backing_case_strategy()
    ) {
        let result = reproduce_post_expiry_backing_fee(seed, case);
        prop_assert!(
            result.is_ok(),
            "PR 367 no longer reproduces for case {:?}: {}",
            case,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr220_pr366_omitted_rescue_liquidation_fuzz(
        seed in omitted_rescue_seed_strategy()
    ) {
        let result = reproduce_omitted_rescue_liquidation(seed);
        prop_assert!(
            result.is_ok(),
            "PR 220/366 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr343_trade_retry_replay_fuzz(
        (seed, route) in trade_retry_replay_strategy()
    ) {
        let result = reproduce_trade_retry_replay(seed, route);
        prop_assert!(
            result.is_ok(),
            "PR 343 {:?} no longer reproduces for seed {:?}: {}",
            route,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr231_asset_generation_replay_fuzz(
        (seed, route) in asset_generation_replay_strategy()
    ) {
        let result = reproduce_asset_generation_trade_replay(seed, route);
        prop_assert!(
            result.is_ok(),
            "PR 231 {:?} no longer reproduces for seed {:?}: {}",
            route,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr224_cpi_caller_fee_siphon_fuzz(
        (seed, route) in cpi_caller_fee_strategy()
    ) {
        let result = reproduce_cpi_caller_fee_siphon(seed, route);
        prop_assert!(
            result.is_ok(),
            "PR 224 {:?} no longer reproduces for seed {:?}: {}",
            route,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr223_cpi_backing_fee_siphon_fuzz(
        seed in cpi_backing_fee_seed_strategy()
    ) {
        let result = reproduce_cpi_backing_fee_siphon(seed);
        prop_assert!(
            result.is_ok(),
            "PR 223 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr329_pr381_composite_rounding_fuzz(
        (seed, case) in composite_rounding_strategy()
    ) {
        let result = reproduce_composite_oracle_rounding(seed, case);
        prop_assert!(
            result.is_ok(),
            "{:?} no longer reproduces for seed {:?}: {}",
            case,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr253_rounded_funding_omission_fuzz(
        seed in rounded_funding_seed_strategy()
    ) {
        let result = reproduce_rounded_funding_omission(seed);
        prop_assert!(
            result.is_ok(),
            "PR 253 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr260_pending_ewma_inheritance_fuzz(
        (seed, route) in pending_ewma_inheritance_strategy()
    ) {
        let result = reproduce_pending_ewma_inheritance(seed, route);
        prop_assert!(
            result.is_ok(),
            "PR 260 {:?} no longer reproduces for seed {:?}: {}",
            route,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr282_pending_ewma_target_override_fuzz(
        (seed, route) in pending_ewma_target_override_strategy()
    ) {
        let result = reproduce_pending_ewma_target_override(seed, route);
        prop_assert!(
            result.is_ok(),
            "PR 282 {:?} no longer reproduces for seed {:?}: {}",
            route,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr283_terminal_dust_payout_erasure_fuzz(
        (seed, route) in terminal_dust_payout_erasure_strategy()
    ) {
        let result = reproduce_terminal_dust_payout_erasure(seed, route);
        prop_assert!(
            result.is_ok(),
            "PR 283 {:?} no longer reproduces for seed {:?}: {}",
            route,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr290_cross_margin_insurance_drain_fuzz(
        seed in cross_margin_insurance_drain_seed_strategy()
    ) {
        let result = reproduce_cross_margin_insurance_drain(seed);
        prop_assert!(
            result.is_ok(),
            "PR 290 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr331_composite_oracle_time_skew_fuzz(
        seed in composite_time_skew_seed_strategy()
    ) {
        let result = reproduce_composite_oracle_time_skew(seed);
        prop_assert!(
            result.is_ok(),
            "PR 331 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr332_pr333_unstaged_mark_target_fuzz(
        (seed, case) in target_staging_strategy()
    ) {
        let result = reproduce_unstaged_mark_target(seed, case);
        prop_assert!(
            result.is_ok(),
            "{:?} no longer reproduces for seed {:?}: {}",
            case,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr356_pending_mark_fee_reward_fuzz(
        seed in pending_mark_fee_reward_seed_strategy()
    ) {
        let result = reproduce_pending_mark_fee_reward(seed);
        prop_assert!(
            result.is_ok(),
            "PR 356 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr365_fractional_cap_settlement_fuzz(
        seed in fractional_cap_settlement_seed_strategy()
    ) {
        let result = reproduce_fractional_cap_settlement(seed);
        prop_assert!(
            result.is_ok(),
            "PR 365 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr225_reclaimable_ewma_fee_fuzz(
        (seed, route) in reclaimable_ewma_fee_strategy()
    ) {
        let result = reproduce_reclaimable_ewma_fee(seed, route);
        prop_assert!(
            result.is_ok(),
            "PR 225 {:?} no longer reproduces for seed {:?}: {}",
            route,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr271_trade_funding_erasure_fuzz(
        (seed, route) in trade_funding_erasure_strategy()
    ) {
        let result = reproduce_trade_funding_erasure(seed, route);
        prop_assert!(
            result.is_ok(),
            "PR 271 {:?} no longer reproduces for seed {:?}: {}",
            route,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr272_rebalance_funding_erasure_fuzz(
        seed in rebalance_funding_erasure_seed_strategy()
    ) {
        let result = reproduce_rebalance_funding_erasure(seed);
        prop_assert!(
            result.is_ok(),
            "PR 272 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr273_forfeit_funding_erasure_fuzz(
        seed in forfeit_funding_erasure_seed_strategy()
    ) {
        let result = reproduce_forfeit_funding_erasure(seed);
        prop_assert!(
            result.is_ok(),
            "PR 273 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr280_trade_driven_liquidation_reward_fuzz(
        (seed, mode, route) in trade_driven_liquidation_reward_strategy()
    ) {
        let result = reproduce_trade_driven_liquidation_reward(seed, mode, route);
        prop_assert!(
            result.is_ok(),
            "PR 280 {:?} {:?} no longer reproduces for seed {:?}: {}",
            mode,
            route,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr267_cross_domain_backing_double_spend_fuzz(
        seed in cross_domain_backing_seed_strategy()
    ) {
        let result = reproduce_cross_domain_backing_double_spend(seed);
        prop_assert!(
            result.is_ok(),
            "PR 267 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr275_asset_generation_mark_replay_fuzz(
        (seed, path) in asset_generation_mark_replay_strategy()
    ) {
        let result = reproduce_asset_generation_mark_replay(seed, path);
        prop_assert!(
            result.is_ok(),
            "PR 275 {:?} no longer reproduces for seed {:?}: {}",
            path,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr277_asset_generation_config_replay_fuzz(
        (seed, path) in asset_generation_config_replay_strategy()
    ) {
        let result = reproduce_asset_generation_config_replay(seed, path);
        prop_assert!(
            result.is_ok(),
            "PR 277 {:?} no longer reproduces for seed {:?}: {}",
            path,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr281_cross_domain_b_settlement_fuzz(
        seed in cross_domain_b_settlement_seed_strategy()
    ) {
        let result = reproduce_cross_domain_b_settlement(seed);
        prop_assert!(
            result.is_ok(),
            "PR 281 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }
}
