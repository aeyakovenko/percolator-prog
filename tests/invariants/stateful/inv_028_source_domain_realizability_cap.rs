//! INV-028 - Source-domain realizability cap.
//!
//! Normative obligation: Source-backed credit cannot survive beyond its realizable backing, and
//! reconciliation of a vanished claim cannot permanently lock funded user exposure.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_source_lien_reversal_exit_matrix_preserves_bounded_exit` generates a positive
//! source-backed claim, liens it through a public risk increase, reverses the authenticated mark,
//! and tests canonical crank, unilateral reduction, and all four trade routes from independent
//! worlds. Every route must unwind the vanished claim and reduce exposure in bounded calls; any
//! rejected attempt must preserve exact SVM rollback while real capital and custody remain.
//! `v16_program_cross_domain_rounding_exit_matrix_discovers_funded_lock` independently constructs
//! two fractional source domains in both asset orders, reverses one source, and requires all six
//! public exit families plus a later honest crank to remain blocked before accepting a finding.
//! `v16_program_flat_source_lien_route_matrix_discovers_backed_claim_lock` flattens all exposure
//! while retaining a real source lien, then requires full/partial conversion, close, later honest
//! cranks, and CPI/no-CPI single/batch reopen-and-flatten escapes all to leave the backed PnL claim
//! uncollectible before accepting a finding.
//!
//! Guarantee boundary: the reversal matrix certifies the fixed source-lien unwind across all six
//! wrapper routes. The remaining tests in this file retain separate counterexamples for unrelated
//! cross-domain rounding and flat-lien findings.

use super::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_028_source_lien_reversal_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_source_lien_reversal_exit_matrix_preserves_bounded_exit(
        seed in any::<[u8; 32]>(),
        // These sizes all pass public admission and create a nonzero source-credit lien. A 10%
        // increase is correctly rejected as LockActive during setup, before the reversal state
        // this property is intended to exercise.
        increase_divisor in prop::sample::select(vec![20u8, 25, 40]),
    ) {
        let discoveries = discover_source_lien_reversal_exit_locks(seed, increase_divisor);
        prop_assert!(
            discoveries.is_ok(),
            "source-lien reversal matrix failed for divisor {increase_divisor}: {}",
            discoveries.unwrap_err()
        );
        let discoveries = discoveries.unwrap();
        prop_assert_eq!(
            discoveries.len(),
            SourceLienReversalExitRoute::ALL.len(),
            "every public exit route needs an independent reversal world"
        );
        let violations = discoveries
            .iter()
            .filter(|discovery| !discovery.preserves_bounded_funded_exit())
            .collect::<Vec<_>>();
        prop_assert!(
            violations.is_empty(),
            "source-lien reversal failed to preserve bounded public exits: {violations:#?}"
        );
    }

    #[test]
    fn v16_program_cross_domain_rounding_exit_matrix_discovers_funded_lock(
        seed in any::<[u8; 32]>(),
    ) {
        let discoveries = discover_cross_domain_rounding_exit_locks(seed);
        prop_assert!(
            discoveries.is_ok(),
            "cross-domain rounding matrix setup failed: {}",
            discoveries.unwrap_err()
        );
        let discoveries = discoveries.unwrap();
        prop_assert_eq!(
            discoveries.len(),
            CrossDomainRoundingOrder::ALL.len(),
            "both asset orders need independent public worlds"
        );
        for discovery in discoveries {
            prop_assert!(
                discovery.is_persistent_funded_exit_lock(),
                "cross-domain rounding retained a public exit: {:?}",
                discovery
            );
        }
    }

    #[test]
    fn v16_program_flat_source_lien_route_matrix_discovers_backed_claim_lock(
        seed in any::<[u8; 32]>(),
        provider_withdrawal in prop::sample::select(vec![50u128]),
    ) {
        let discoveries = discover_flat_source_lien_claim_locks(seed, provider_withdrawal);
        prop_assert!(
            discoveries.is_ok(),
            "flat source-lien setup failed: {}",
            discoveries.unwrap_err()
        );
        let discoveries = discoveries.unwrap();
        prop_assert_eq!(
            discoveries.len(),
            FlatSourceLienEscapeRoute::ALL.len(),
            "every trade family needs an independent flat-lien escape world"
        );
        for discovery in discoveries {
            prop_assert!(
                discovery.is_persistent_backed_claim_lock(),
                "flat source lien retained a terminal claim route: {:?}",
                discovery
            );
        }
    }
}
