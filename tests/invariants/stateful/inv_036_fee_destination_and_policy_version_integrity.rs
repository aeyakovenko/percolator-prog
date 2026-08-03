//! INV-036 - Fee destination and policy-version integrity.
//!
//! Normative obligation: Charged fees reach only the authorized destination under the bound policy version.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_source_fee_consent_route_matrix_discovers_unsigned_debits` constructs positive
//! source-backed PnL and varies the consuming trade across CPI/no-CPI and single/batch routes. Its
//! common oracle requires the LP debit to stay within prior consent and traces any debit into the
//! backing provider's earnings. Finding-specific impact regressions remain below. These tests exercise the deployed public
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
                "proptest-regressions/inv_036_source_fee_consent_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_source_fee_consent_route_matrix_discovers_unsigned_debits(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_source_fee_consent_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), SourceFeeConsentKind::ALL.len());
        for (expected, discovery) in SourceFeeConsentKind::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.kind, expected);
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.kind)
            .collect();
        eprintln!("independent source-fee consent discoveries: {violations:?}");
        prop_assert_eq!(
            violations,
            vec![SourceFeeConsentKind::NoCpi, SourceFeeConsentKind::Cpi],
            "source-fee route differential changed; inspect both newly vulnerable and newly safe paths"
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
    fn v16_program_pr314_activation_fee_consent_fuzz(
        seed in activation_fee_consent_seed_strategy()
    ) {
        let result = reproduce_activation_fee_consent(seed);
        prop_assert!(
            result.is_ok(),
            "PR 314 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr310_bilateral_base_fee_consent_fuzz(
        (seed, route) in bilateral_base_fee_consent_strategy()
    ) {
        let result = reproduce_bilateral_base_fee_consent(seed, route);
        prop_assert!(
            result.is_ok(),
            "PR 310 {:?} no longer reproduces for seed {:?}: {}",
            route,
            seed,
            result.unwrap_err()
        );
    }
}
