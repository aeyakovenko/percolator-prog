//! INV-005 - Authority incarnation binding.
//!
//! Normative obligation: Authority consent cannot revive after rotation, disablement, or A-to-B-to-A reuse.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_authority_incarnation_operation_matrix_rejects_aba_replays` enumerates market
//! and asset authority scopes without finding metadata and proves stale requests reject with exact
//! rollback after A-to-B-to-A rotation.
//! The same generated seed drives funded terminal-resolve and backing-handoff traces. Both retain
//! old consent across A-to-B-to-A, prove rejection and rollback, and then execute the current
//! authority or incumbent owner's bounded public exit.
//! `v16_program_funded_role_matrix_preserves_incumbent_principal` independently funds each
//! value-bearing asset role after proving an empty-role cold-admin handoff remains available. It
//! then requires a funded takeover to reject with exact rollback, rejects the replacement's
//! withdrawal, and proves the incumbent can withdraw the exact principal.
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
    fn v16_program_authority_incarnation_operation_matrix_rejects_aba_replays(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_authority_incarnation_replays(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), AuthorityIntentKind::ALL.len());
        for (expected, discovery) in AuthorityIntentKind::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.kind, expected);
        }
        for discovery in &discoveries {
            prop_assert!(
                discovery.certifies_epoch_rejection(),
                "authority-incarnation route did not reject with exact rollback: {:?}",
                discovery
            );
        }
        let terminal = crate::support::invariant_discovery::discover_authority_resolve_terminal_replay(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(
            terminal.certifies_epoch_and_bounded_exit(),
            "old-authority resolve was not rejected before a fresh bounded exit: {:?}",
            terminal
        );
        prop_assert_eq!(terminal.victim_loss, 100_000);
        prop_assert_eq!(terminal.winner_gain, terminal.victim_loss);

        let funded_handoff =
            crate::support::invariant_discovery::discover_authority_funded_handoff_replay(seed)
                .map_err(TestCaseError::fail)?;
        prop_assert!(
            funded_handoff.certifies_epoch_and_bounded_exit(),
            "old-authority handoff was not rejected before the incumbent bounded exit: {:?}",
            funded_handoff
        );
        prop_assert_eq!(funded_handoff.provider_source_debit, 500);
        prop_assert_eq!(funded_handoff.replacement_gain, 0);
        prop_assert_eq!(funded_handoff.provider_exit_gain, funded_handoff.provider_source_debit);
    }

    #[test]
    fn v16_program_funded_role_matrix_preserves_incumbent_principal(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_funded_role_seizures(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), FundedRoleKind::ALL.len());
        for (expected, discovery) in FundedRoleKind::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.kind, expected);
            prop_assert!(
                discovery.certifies_funded_role_protection(),
                "funded role did not preserve incumbent principal: {discovery:?}"
            );
        }
    }
}
