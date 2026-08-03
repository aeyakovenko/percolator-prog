//! INV-038 - Rounding and ratio conservation.
//!
//! Normative obligation: Every rounded allocation plus explicit residue equals its exact source amount.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_composite_scale_matrix_discovers_route_rounding_value` holds the exact rational
//! composite price constant while changing its factorization at large and micro scales. It then
//! requires wrapper target, engine mark, liquidation eligibility, and extracted reward to agree
//! with exact single-round arithmetic.
//! `v16_program_selected_observation_omission_discovers_rounded_transfer_loss` compares identical
//! public worlds with and without the selected asset observation after an unrelated epoch advance;
//! a successful omission must preserve funding indexes and terminal payouts exactly.
//! `v16_program_fractional_max_dt_cranks_discover_terminal_value_stall` repeatedly executes the
//! bounded public crank at maximum elapsed time and requires fractional cap residue to accumulate
//! until the target is reached; it also reconciles any stalled price against terminal payouts.
//! Direct impact tests remain below. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

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
    fn v16_program_fractional_max_dt_cranks_discover_terminal_value_stall(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_fractional_movement_stall(seed)
            .map_err(TestCaseError::fail)?;
        eprintln!("independent fractional-movement discovery: {discovery:?}");
        prop_assert!(
            discovery.is_violation(),
            "vulnerable-pin fractional movement changed: {:?}",
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
    fn v16_program_selected_observation_omission_discovers_rounded_transfer_loss(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_observation_omission_violation(seed)
            .map_err(TestCaseError::fail)?;
        eprintln!("independent observation-omission discovery: {discovery:?}");
        prop_assert!(
            discovery.is_violation(),
            "vulnerable-pin observation omission changed: {:?}",
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
    fn v16_program_composite_scale_matrix_discovers_route_rounding_value(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_composite_rounding_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), CompositeRoundingScale::ALL.len());
        for (expected, discovery) in CompositeRoundingScale::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.scale, expected);
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.scale)
            .collect();
        eprintln!("independent composite-rounding discoveries: {violations:?}");
        prop_assert_eq!(
            violations,
            CompositeRoundingScale::ALL.to_vec(),
            "vulnerable-pin composite-rounding corpus changed"
        );
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
}
