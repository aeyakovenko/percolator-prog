//! INV-028 - Source-domain realizability cap.
//!
//! Normative obligation: Source-backed credit cannot survive beyond its realizable backing, and
//! reconciliation of a vanished claim cannot permanently lock funded user exposure.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_source_lien_reversal_exit_matrix_discovers_funded_lock` generates a positive
//! source-backed claim, liens it through a public risk increase, reverses the authenticated mark,
//! and tests canonical crank, unilateral reduction, and all four trade routes from independent
//! worlds. A finding requires every route to reject as LockActive with exact SVM rollback while
//! real capital, exposure, and canonical SPL liquidity remain.
//! `v16_program_cross_domain_rounding_exit_matrix_discovers_funded_lock` independently constructs
//! two fractional source domains in both asset orders, reverses one source, and requires all six
//! public exit families plus a later honest crank to remain blocked before accepting a finding.
//!
//! Guarantee boundary: this is a public counterexample on the vulnerable pin, not certification
//! of every source-credit lifecycle. Fixed-pin certification still requires the proof, model, and
//! reachability methods named by the invariant charter.

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
    fn v16_program_source_lien_reversal_exit_matrix_discovers_funded_lock(
        seed in any::<[u8; 32]>(),
        increase_divisor in prop::sample::select(vec![10u8, 20, 25, 40]),
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
        for discovery in discoveries {
            prop_assert!(
                discovery.is_persistent_funded_exit_lock(),
                "source-lien reversal retained a public exit: {:?}",
                discovery
            );
        }
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
}
