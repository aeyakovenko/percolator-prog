//! INV-005 - Authority incarnation binding.
//!
//! Normative obligation: Authority consent cannot revive after rotation, disablement, or A-to-B-to-A reuse.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_authority_incarnation_operation_matrix_discovers_aba_replays` enumerates market
//! and asset authority scopes without finding metadata.
//! `v16_program_funded_role_matrix_discovers_admin_seizure` independently funds each value-bearing
//! asset role, delegates a distinct cold asset admin, and measures whether that admin can redirect
//! the incumbent's principal to a replacement key. The economic oracle requires an exact provider
//! source debit and equal replacement SPL-token credit; a configuration-only handoff is not enough.
//! Direct impact regressions remain below.
//! These tests exercise the deployed public
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
                "proptest-regressions/inv_005_authority_incarnation_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_authority_incarnation_operation_matrix_discovers_aba_replays(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_authority_incarnation_replays(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), AuthorityIntentKind::ALL.len());
        for (expected, discovery) in AuthorityIntentKind::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.kind, expected);
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.kind)
            .collect();
        eprintln!("independent INV-005 discoveries: {violations:?}");
        prop_assert_eq!(
            violations,
            AuthorityIntentKind::ALL.to_vec(),
            "vulnerable-pin authority-incarnation discovery corpus changed"
        );
    }

    #[test]
    fn v16_program_funded_role_matrix_discovers_admin_seizure(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_funded_role_seizures(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), FundedRoleKind::ALL.len());
        for (expected, discovery) in FundedRoleKind::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.kind, expected);
            prop_assert!(
                discovery.is_violation(),
                "vulnerable-pin funded-role discovery changed: {discovery:?}"
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
                "proptest-regressions/v16_program_stateful_fuzz.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_pr251_delayed_asset_authority_revival_fuzz(
        seed in delayed_asset_authority_revival_seed_strategy()
    ) {
        let result = reproduce_delayed_asset_authority_revival(seed);
        prop_assert!(
            result.is_ok(),
            "PR 251 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr345_pr346_authority_handoff_aba_replay_fuzz(
        (seed, path) in authority_handoff_aba_replay_strategy()
    ) {
        let result = reproduce_authority_handoff_aba_replay(seed, path);
        prop_assert!(
            result.is_ok(),
            "PR 345/346 {:?} no longer reproduces for seed {:?}: {}",
            path,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr353_resolve_authority_incarnation_replay_fuzz(
        seed in resolve_authority_incarnation_replay_seed_strategy()
    ) {
        let result = reproduce_resolve_authority_incarnation_replay(seed);
        prop_assert!(
            result.is_ok(),
            "PR 353 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }
}
