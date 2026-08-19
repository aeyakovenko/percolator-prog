//! INV-038 - Rounding and ratio conservation.
//!
//! Normative obligation: Every rounded allocation plus explicit residue equals its exact source amount.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_composite_scale_matrix_preserves_exact_composition` holds the exact rational
//! composite price constant while changing its factorization at large and micro scales. It then
//! requires wrapper target, engine mark, liquidation eligibility, and extracted reward to agree
//! with exact single-round arithmetic.
//! `v16_program_selected_observation_omission_rejects_and_preserves_rounded_transfer` compares identical
//! public worlds with and without the selected asset observation after an unrelated epoch advance;
//! omission must reject with exact rollback, after which the observed continuation must preserve
//! funding indexes and terminal payouts exactly.
//! `v16_program_fractional_max_dt_cranks_reach_target_and_preserve_terminal_value` repeatedly executes the
//! bounded public crank at maximum elapsed time and requires fractional cap residue to accumulate
//! until the target is reached; it also reconciles any stalled price against terminal payouts.
//! Direct impact tests remain below. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: all three matrices are fixed-pin public-route certifications.

use super::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_038_fractional_movement_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_fractional_max_dt_cranks_reach_target_and_preserve_terminal_value(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = verify_fractional_movement_convergence(seed)
            .map_err(TestCaseError::fail)?;
        eprintln!("independent fractional-movement discovery: {discovery:?}");
        prop_assert!(
            discovery.preserves_fractional_settlement(),
            "fractional movement failed to converge and conserve value: {:?}",
            discovery
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_038_observation_omission_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_selected_observation_omission_rejects_and_preserves_rounded_transfer(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_observation_omission_violation(seed)
            .map_err(TestCaseError::fail)?;
        eprintln!("independent observation-omission verification: {discovery:?}");
        prop_assert!(
            discovery.preserves_rounded_transfer(),
            "observation omission did not reject and recover safely: {:?}",
            discovery
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_038_composite_rounding_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_composite_scale_matrix_preserves_exact_composition(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_composite_rounding_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), CompositeRoundingScale::ALL.len());
        for (expected, discovery) in CompositeRoundingScale::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.scale, expected);
        }
        for discovery in discoveries {
            prop_assert!(!discovery.is_violation(), "{discovery:?}");
            prop_assert_eq!(discovery.rounded_target, discovery.exact_mark);
            prop_assert_eq!(discovery.rounded_mark, discovery.exact_mark);
            prop_assert_eq!(discovery.certified_liq_deficit, 0);
            prop_assert_eq!(discovery.victim_capital_loss, 0);
            prop_assert_eq!(discovery.oi_reduction_q, 0);
            prop_assert_eq!(discovery.cranker_reward, 0);
            prop_assert_eq!(discovery.extracted_tokens, 0);
        }
    }
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
    fn v16_program_pr329_pr381_composite_rounding_preservation_fuzz(
        (seed, case) in composite_rounding_strategy()
    ) {
        let reproduction = reproduce_composite_oracle_rounding(seed, case)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(reproduction.case, case);
        prop_assert_eq!(reproduction.rounded_target, reproduction.exact_mark);
        prop_assert_eq!(reproduction.rounded_mark, reproduction.exact_mark);
        prop_assert_eq!(reproduction.certified_liq_deficit, 0);
        prop_assert_eq!(reproduction.victim_capital_loss, 0);
        prop_assert_eq!(reproduction.oi_reduction_q, 0);
        prop_assert_eq!(reproduction.cranker_reward, 0);
        prop_assert_eq!(reproduction.extracted_tokens, 0);
    }

    #[test]
    fn v16_program_pr253_rounded_funding_omission_rejection_fuzz(
        seed in rounded_funding_seed_strategy()
    ) {
        let reproduction = reproduce_rounded_funding_omission(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(reproduction.omitted_rejected_nonprogress);
        prop_assert!(reproduction.omitted_exact_rollback);
        prop_assert_eq!(reproduction.attack_f_long_num, reproduction.control_f_long_num);
        prop_assert_eq!(reproduction.attack_f_short_num, reproduction.control_f_short_num);
        prop_assert_eq!(reproduction.victim_payout_loss, 0);
        prop_assert_eq!(reproduction.attacker_payout_gain, 0);
    }

    #[test]
    fn v16_program_pr365_fractional_cap_settlement_fuzz(
        seed in fractional_cap_settlement_seed_strategy()
    ) {
        let reproduction = reproduce_fractional_cap_settlement(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(reproduction.reached_target);
        prop_assert_eq!(reproduction.settlement_price, reproduction.target_price);
        prop_assert_eq!(reproduction.long_overpayment, 0);
        prop_assert_eq!(reproduction.short_underpayment, 0);
        prop_assert_eq!(
            u128::from(reproduction.long_payout) + u128::from(reproduction.short_payout),
            2_000_000
        );
    }
}
