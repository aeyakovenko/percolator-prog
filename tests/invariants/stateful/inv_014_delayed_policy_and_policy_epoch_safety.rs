//! INV-014 - Delayed-policy and policy-epoch safety.
//!
//! Normative obligation: Delayed requests remain bounded by the policy and economics the signer authorized.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_superseded_control_matrix_rejects_stale_overwrites` generates retained controls,
//! installs a distinct newer authorized value, then applies the stale bytes. The matrix covers
//! matcher consent, every mark mode, both backing sides, and every market-wide fee/resolve lane in
//! both retained-higher/current-lower and retained-lower/current-higher payload orders. Every stale
//! request must reject with an exact whole-account rollback.
//! `v16_program_fee_consent_operation_matrix_discovers_unsigned_debits` varies fresh-signed,
//! retained, unsigned-LP, and activation routes and compares each affected signer's actual debit
//! with the fee terms that signer authorized. The fresh-signed live control proves a policy update
//! is not mislabeled when both traders sign the updated fee and the exact debit stays in bounds.
//! Secondary coverage: INV-036 where those debits become an unauthorized fee destination or
//! redirect value away from the signer-approved policy.
//! `v16_program_backing_provider_consent_order_matrix_preserves_provider_terms` varies fee-policy
//! changes before and after a retained backing top-up. Each stale transition rejects with exact
//! rollback, then a current provider-authorized control generates a nonzero LP fee and traces it
//! through the selected provider/insurance ledger to an exact SPL withdrawal.
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
        let discoveries = discover_bidirectional_superseded_intents(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(
            discoveries.len(),
            SupersededIntentKind::ALL.len() * SupersessionPayloadOrder::ALL.len()
        );
        for (index, discovery) in discoveries.into_iter().enumerate() {
            let order_index = index / SupersededIntentKind::ALL.len();
            let kind_index = index % SupersededIntentKind::ALL.len();
            prop_assert_eq!(discovery.payload_order, SupersessionPayloadOrder::ALL[order_index]);
            prop_assert_eq!(discovery.kind, SupersededIntentKind::ALL[kind_index]);
            prop_assert!(!discovery.accepted_stale_intent, "{:?}/{:?} accepted stale signed bytes", discovery.kind, discovery.payload_order);
            prop_assert!(!discovery.overwrote_newer_state, "{:?}/{:?} overwrote the newer state", discovery.kind, discovery.payload_order);
            prop_assert_eq!(discovery.compute_units, None, "{:?}/{:?} unexpectedly committed", discovery.kind, discovery.payload_order);
            prop_assert!(discovery.fresh_intent_landed, "{:?}/{:?} current-sequence control did not land", discovery.kind, discovery.payload_order);
            prop_assert!(discovery.fresh_mutated_economic_state, "{:?}/{:?} current-sequence control was vacuous", discovery.kind, discovery.payload_order);
            prop_assert!(discovery.fresh_compute_units.is_some(), "{:?}/{:?} current-sequence control needs a successful CU result", discovery.kind, discovery.payload_order);
            prop_assert!(!discovery.is_violation(), "{:?}/{:?} violated INV-014", discovery.kind, discovery.payload_order);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_014_oracle_supersession_terminal.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_oracle_supersession_retains_terminal_value(seed in any::<[u8; 32]>()) {
        let discoveries = discover_oracle_supersession_terminal_losses(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(
            discoveries.len(),
            SupersededIntentKind::ORACLE_TERMINAL_CANDIDATES.len()
        );
        for (expected, discovery) in SupersededIntentKind::ORACLE_TERMINAL_CANDIDATES
            .into_iter()
            .zip(&discoveries)
        {
            prop_assert_eq!(discovery.kind, expected);
            prop_assert!(
                discovery.certifies_terminal_supersession(),
                "{expected:?} terminal supersession evidence failed: {discovery:?}"
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_014_liquidation_share_terminal.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_liquidation_share_supersession_preserves_victim_value(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_liquidation_share_supersession(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(
            discovery.certifies_attribution_only(),
            "liquidation share changed fee or victim terminal value: {discovery:?}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_014_maintenance_share_terminal.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_maintenance_share_supersession_preserves_payer_value(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_maintenance_share_supersession(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(
            discovery.certifies_attribution_only(),
            "maintenance share changed fee or payer terminal value: {discovery:?}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_014_matcher_revocation_terminal.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_revoked_matcher_retains_terminal_value(seed in any::<[u8; 32]>()) {
        let discovery = discover_matcher_revocation_terminal_loss(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(
            discovery.certifies_revocation_and_bounded_exit(),
            "stale matcher consent changed LP terminal value: {discovery:?}"
        );
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
    fn v16_program_backing_provider_consent_order_matrix_preserves_provider_terms(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_backing_provider_consent_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), BackingProviderConsentOrder::ALL.len());
        for (expected, discovery) in BackingProviderConsentOrder::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.order, expected);
        }
        for discovery in &discoveries {
            prop_assert!(!discovery.is_violation(), "{:?} violated INV-014", discovery.order);
            prop_assert!(discovery.satisfies_invariant(), "{:?} was vacuous: {discovery:?}", discovery.order);
        }
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
            prop_assert!(
                discovery.satisfies_invariant(),
                "fee-consent terminal evidence was incomplete: {discovery:?}"
            );
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
