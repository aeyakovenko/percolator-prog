//! INV-036 - Fee destination and policy-version integrity.
//!
//! Normative obligation: Charged fees reach only the authorized destination under the bound policy version.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_source_fee_consent_route_matrix_discovers_unsigned_debits` constructs positive
//! source-backed PnL and varies the consuming trade across CPI/no-CPI, single/batch, and both
//! participant roles. Every request is retained before the backing-fee policy change. Its common
//! oracle requires the LP debit to stay within prior consent and traces any debit through the
//! backing provider's earnings into an exact public SPL withdrawal. Finding-specific impact
//! regressions remain below. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//! Secondary coverage: INV-014 because the same route matrix varies the policy state retained by
//! the signer and rejects economic terms introduced after consent.
//!
//! Guarantee boundary: PRs 223, 224, 259, 310, 313, and 314 are fixed-pin certifications here.
//! The source-fee matrix is the independent holdout oracle for delayed trader-fee consent, while
//! INV-014 certifies PR339's distinct backing-provider consent and policy-order matrix.

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
        prop_assert_eq!(
            discoveries.len(),
            SourceFeeConsentKind::ALL.len() * SourceFeeConsentRole::ALL.len()
        );
        for ((expected_kind, expected_role), discovery) in SourceFeeConsentKind::ALL
            .into_iter()
            .flat_map(|kind| SourceFeeConsentRole::ALL.into_iter().map(move |role| (kind, role)))
            .zip(&discoveries)
        {
            prop_assert_eq!(discovery.kind, expected_kind);
            prop_assert_eq!(discovery.victim_role, expected_role);
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| (discovery.kind, discovery.victim_role))
            .collect();
        eprintln!("independent source-fee consent discoveries: {violations:?}");
        for discovery in discoveries.iter().filter(|discovery| discovery.is_violation()) {
            prop_assert_eq!(discovery.lp_capital_debit, discovery.provider_earnings_credit);
            prop_assert_eq!(
                discovery.provider_earnings_credit,
                discovery.extracted_provider_tokens
            );
            let exact_terminal_loss = matches!(
                discovery.terminal_classification,
                crate::support::v16_svm::PublicTerminalClassification::LossOfFunds {
                    victim_loss_atoms,
                    unauthorized_gain_atoms,
                } if victim_loss_atoms == discovery.lp_capital_debit
                    && unauthorized_gain_atoms == discovery.extracted_provider_tokens
            );
            prop_assert!(exact_terminal_loss);
        }
        for discovery in &discoveries {
            let single_route = matches!(
                discovery.kind,
                SourceFeeConsentKind::NoCpi | SourceFeeConsentKind::Cpi
            );
            prop_assert_eq!(discovery.authorized_retry_landed, single_route);
            prop_assert_eq!(discovery.over_cap_rejected_exact_rollback, single_route);
            if single_route {
                prop_assert!(discovery.authorized_retry_lp_capital_debit > 0);
                prop_assert_eq!(
                    discovery.authorized_retry_lp_capital_debit,
                    discovery.authorized_retry_provider_earnings_credit
                );
                prop_assert_eq!(
                    discovery.authorized_retry_provider_earnings_credit,
                    discovery.authorized_retry_extracted_provider_tokens
                );
                prop_assert!(discovery.authorized_retry_compute_units.is_some());
            } else {
                prop_assert_eq!(discovery.authorized_retry_lp_capital_debit, 0);
                prop_assert_eq!(discovery.authorized_retry_provider_earnings_credit, 0);
                prop_assert_eq!(discovery.authorized_retry_extracted_provider_tokens, 0);
                prop_assert!(discovery.authorized_retry_compute_units.is_none());
            }
        }
        prop_assert!(
            violations.is_empty(),
            "retained trade accepted a backing fee that the debited trader never authorized: {violations:?}"
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
        prop_assert_eq!(protection.attacker_capital_delta, -120);
        prop_assert!(protection.zero_cap_risk_reduction_landed);
        prop_assert!(protection.max_route_cu < crate::support::v16_svm::TX_CU_LIMIT);
        prop_assert!(protection.token_supply_conserved);
    }

    #[test]
    fn v16_program_pr314_activation_fee_consent_fuzz(
        seed in activation_fee_consent_seed_strategy()
    ) {
        let protection = verify_activation_fee_consent(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(protection.stale_policy_rejected);
        prop_assert!(protection.rejected_exact_rollback);
        prop_assert_eq!(protection.unconsented_creator_loss, 0);
        prop_assert_eq!(protection.unconsented_insurance_delta, 0);
        prop_assert_eq!(protection.charged_fee, protection.current_fee);
        prop_assert_eq!(protection.insured_fee, u128::from(protection.current_fee));
        prop_assert!(protection.current_fee <= protection.consented_max_fee);
        prop_assert!(protection.asset_active);
        prop_assert!(protection.activation_cu < crate::support::v16_svm::TX_CU_LIMIT);
        prop_assert!(protection.token_supply_conserved);
    }

    #[test]
    fn v16_program_pr313_cpi_base_fee_consent_protection_fuzz(
        (seed, route) in cpi_base_fee_consent_strategy()
    ) {
        let protection = verify_cpi_base_fee_consent(seed, route)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(protection.route, route);
        prop_assert!(protection.invalid_cap_rejected);
        prop_assert!(protection.invalid_cap_exact_rollback);
        prop_assert!(protection.stale_fill_rejected);
        prop_assert!(protection.stale_fill_exact_rollback);
        prop_assert!(protection.position_epoch_preserved);
        prop_assert_eq!(protection.unconsented_lp_loss, 0);
        prop_assert_eq!(protection.unconsented_insurance_delta, 0);
        prop_assert_eq!(protection.consented_lp_fee, 100_000);
        prop_assert_eq!(protection.consented_insurance_fee, 200_000);
        prop_assert_eq!(protection.total_payout, 200_000_000);
        prop_assert!(protection.max_route_cu < crate::support::v16_svm::TX_CU_LIMIT);
        prop_assert!(protection.token_supply_conserved);
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
