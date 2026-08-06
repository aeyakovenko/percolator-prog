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
//! Secondary coverage: INV-014 because the same route matrix varies the policy state retained by
//! the signer and rejects economic terms introduced after consent.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.
//! PRs 223, 224, and 310 are fixed-pin assertions here; PR 314 remains a counterexample.

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
            vec![SourceFeeConsentKind::NoCpi],
            "source-fee route differential changed; CPI must require matcher consent"
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
    fn v16_program_pr224_cpi_caller_fee_protection_fuzz(
        (seed, route) in cpi_caller_fee_strategy()
    ) {
        let protection = verify_cpi_caller_fee_protection(seed, route)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(protection.route, route);
        prop_assert_eq!(protection.requested_fee_bps, 10_000);
        prop_assert_eq!(protection.attacker_profit, 0);
        prop_assert_eq!(protection.lp_loss, 0);
        prop_assert_eq!(protection.withdrawable_insurance, 0);
        prop_assert!(protection.insurance_withdraw_rejected);
        prop_assert!(protection.rejected_exact_rollback);
        prop_assert_eq!(protection.total_payout, 2_000_000);
        prop_assert!(protection.token_supply_conserved);
        prop_assert!(protection.max_trade_cu < crate::support::v16_svm::TX_CU_LIMIT);
    }

    #[test]
    fn v16_program_pr223_cpi_backing_fee_consent_fuzz(
        seed in cpi_backing_fee_seed_strategy()
    ) {
        let protection = verify_cpi_backing_fee_consent(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(protection.matcher_cap_bps, 5_000);
        prop_assert!(protection.rejected_without_consent);
        prop_assert!(protection.rejected_exact_rollback);
        prop_assert_eq!(protection.unconsented_provider_earnings, 0);
        prop_assert_eq!(protection.lp_capital_loss, protection.provider_earnings);
        prop_assert!(protection.provider_earnings > 0);
        prop_assert_eq!(protection.provider_earnings, u128::from(protection.extracted_tokens));
        prop_assert_eq!(protection.attacker_capital_delta, 0);
        prop_assert!(protection.zero_cap_risk_reduction_landed);
        prop_assert!(protection.max_route_cu < crate::support::v16_svm::TX_CU_LIMIT);
        prop_assert!(protection.token_supply_conserved);
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
    fn v16_program_pr310_bilateral_base_fee_consent_protection_fuzz(
        (seed, route) in bilateral_base_fee_consent_strategy()
    ) {
        let protection = verify_bilateral_base_fee_consent(seed, route)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(protection.route, route);
        prop_assert!(protection.stale_open_rejected);
        prop_assert!(protection.stale_close_rejected);
        prop_assert!(protection.rejected_exact_rollback);
        prop_assert_eq!(protection.unconsented_victim_loss, 0);
        prop_assert_eq!(protection.unconsented_insurance_delta, 0);
        prop_assert_eq!(protection.consented_victim_fee, 100_000);
        prop_assert_eq!(protection.consented_insurance_fee, 200_000);
        prop_assert_eq!(protection.total_payout, 200_000_000);
        prop_assert!(protection.open_cu < crate::support::v16_svm::TX_CU_LIMIT);
        prop_assert!(protection.close_cu < crate::support::v16_svm::TX_CU_LIMIT);
        prop_assert!(protection.token_supply_conserved);
    }
}
