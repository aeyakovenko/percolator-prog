//! INV-014 - Delayed-policy and policy-epoch safety.
//!
//! Normative obligation: Delayed requests remain bounded by the policy and economics the signer authorized.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_superseded_control_matrix_rejects_stale_overwrites` generates retained controls,
//! installs a distinct newer authorized value, then applies the stale bytes. The matrix covers
//! matcher consent, every mark mode, both backing sides, and every market-wide fee/resolve lane.
//! Every stale request must reject with an exact whole-account rollback.
//! `v16_program_fee_consent_operation_matrix_discovers_unsigned_debits` varies fresh-signed,
//! retained, unsigned-LP, and activation routes and compares each affected signer's actual debit
//! with the fee terms that signer authorized. The fresh-signed live control proves a policy update
//! is not mislabeled when both traders sign the updated fee and the exact debit stays in bounds.
//! Secondary coverage: INV-036 where those debits become an unauthorized fee destination or
//! redirect value away from the signer-approved policy.
//! `v16_program_backing_provider_consent_order_matrix_discovers_fee_redirection` varies fee-policy
//! changes before and after a retained backing top-up, then traces the generated LP fee through
//! provider/insurance ledgers and an operator SPL withdrawal.
//! Direct impact regressions remain below. These tests exercise the deployed public
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
                "proptest-regressions/inv_014_supersession_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_superseded_control_matrix_rejects_stale_overwrites(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_superseded_intents(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), SupersededIntentKind::ALL.len());
        for (expected, discovery) in SupersededIntentKind::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.kind, expected);
        }
        for discovery in discoveries {
            prop_assert!(!discovery.accepted_stale_intent, "{:?} accepted stale signed bytes", discovery.kind);
            prop_assert!(!discovery.overwrote_newer_state, "{:?} overwrote the newer state", discovery.kind);
            prop_assert_eq!(discovery.compute_units, None, "{:?} unexpectedly committed", discovery.kind);
            prop_assert!(!discovery.is_violation(), "{:?} violated INV-014", discovery.kind);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_014_backing_provider_consent_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_backing_provider_consent_order_matrix_discovers_fee_redirection(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_backing_provider_consent_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), BackingProviderConsentOrder::ALL.len());
        for (expected, discovery) in BackingProviderConsentOrder::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.order, expected);
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.order)
            .collect();
        eprintln!("independent backing-provider consent discoveries: {violations:?}");
        prop_assert_eq!(
            violations,
            BackingProviderConsentOrder::ALL.to_vec(),
            "vulnerable-pin backing-provider consent corpus changed"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_014_fee_consent_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_fee_consent_operation_matrix_discovers_unsigned_debits(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_fee_consent_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), FeeConsentKind::ALL.len());
        for (expected, discovery) in FeeConsentKind::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.kind, expected);
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.kind)
            .collect();
        eprintln!("independent fee-consent discoveries: {violations:?}");
        prop_assert!(
            violations.is_empty(),
            "fee-consent classification changed; fresh-signed fees must remain bounded and retained no-CPI, CPI LP/caller, and permissionless activation fees must remain protected: {violations:?}"
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
    fn v16_program_pr325_maintenance_policy_generation_replay_fuzz(
        seed in maintenance_policy_generation_replay_seed_strategy()
    ) {
        let result = reproduce_maintenance_policy_generation_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 325 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr326_liquidation_policy_generation_replay_fuzz(
        seed in liquidation_policy_generation_replay_seed_strategy()
    ) {
        let result = reproduce_liquidation_policy_generation_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 326 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

}
