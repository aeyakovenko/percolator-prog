//! INV-063 - Backing-expiry normalization.
//!
//! Normative obligation: Expired backing is normalized before every consumer and cannot remain economically fresh.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_backing_expiry_boundary_discovers_extractable_stale_fee` constructs a retained
//! trade while backing is fresh, lands it after authenticated Clock expiry, and requires an
//! invariant failure only when stale engine time debits victim capital and the backing provider
//! withdraws that exact debit as SPL tokens.
//! `v16_program_retained_maturity_matrix_discovers_terminal_funded_lock` generates signed expiry
//! boundaries independently of any finding manifest, compares omitted and delayed operations,
//! and requires a finding only when the delayed operation consumes independent principal and
//! leaves funded resolved users unable to progress through owner or permissionless routes.
//! `v16_program_expired_backing_consumer_matrix_discovers_principal_extraction` generates released
//! source-backed claims and varies the expiry boundary. It reports a violation only when a
//! favorable consumer lands after authenticated expiry, consumes the provider ledger, and moves
//! the exact credited amount into the claimant's external SPL account.
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
                "proptest-regressions/inv_063_backing_expiry_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_backing_expiry_boundary_discovers_extractable_stale_fee(
        seed in any::<[u8; 32]>(),
        expiry_offset in prop::sample::select(vec![2u8, 3, 5, 8]),
    ) {
        let case = BackingExpiryCase {
            fee_bps: 5_000,
            expiry_offset,
            mark_move_bps: 500,
            increase_divisor: 20,
        };
        let result = discover_backing_expiry_violation(seed, case);
        prop_assert!(
            result.is_ok(),
            "backing-expiry discovery failed for case {:?}: {}",
            case,
            result.unwrap_err()
        );
        let discovery = result.unwrap();
        prop_assert!(
            discovery.is_violation(),
            "expired backing did not create an externally extractable victim debit: {:?}",
            discovery
        );
    }

    #[test]
    fn v16_program_retained_maturity_matrix_discovers_terminal_funded_lock(
        seed in any::<[u8; 32]>(),
        expiry_offset in prop::sample::select(vec![2u8, 3, 4, 6]),
    ) {
        let discoveries = discover_retained_maturity_terminal_locks(seed, expiry_offset);
        prop_assert!(
            discoveries.is_ok(),
            "retained-maturity discovery failed at offset {expiry_offset}: {}",
            discoveries.unwrap_err()
        );
        let discoveries = discoveries.unwrap();
        prop_assert_eq!(
            discoveries.len(),
            RetainedMaturityKind::ALL.len(),
            "every retained maturity operation needs a generated world"
        );
        for discovery in discoveries {
            prop_assert!(
                discovery.is_persistent_funded_lock(),
                "retained operation did not reproduce an exact funded terminal lock: {:?}",
                discovery
            );
        }
    }

    #[test]
    fn v16_program_expired_backing_consumer_matrix_discovers_principal_extraction(
        seed in any::<[u8; 32]>(),
        expiry_offset in prop::sample::select(vec![1u8, 2, 4, 6]),
    ) {
        let discoveries = discover_expired_backing_consumers(seed, expiry_offset);
        prop_assert!(
            discoveries.is_ok(),
            "expired-backing consumer discovery failed at offset {expiry_offset}: {}",
            discoveries.unwrap_err()
        );
        let discoveries = discoveries.unwrap();
        prop_assert_eq!(
            discoveries.len(),
            ExpiredBackingConsumerKind::ALL.len(),
            "every favorable backing consumer needs a generated expiry world"
        );
        for discovery in discoveries {
            prop_assert!(
                discovery.is_expired_principal_extraction(),
                "expired backing consumer did not extract exact provider principal: {:?}",
                discovery
            );
        }
    }
}
