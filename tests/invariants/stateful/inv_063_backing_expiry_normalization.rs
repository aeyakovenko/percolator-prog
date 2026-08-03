//! INV-063 - Backing-expiry normalization.
//!
//! Normative obligation: Expired backing is normalized before every consumer and cannot remain economically fresh.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_backing_expiry_boundary_discovers_extractable_stale_fee` constructs a retained
//! trade while backing is fresh, lands it after authenticated Clock expiry, and requires an
//! invariant failure only when stale engine time debits victim capital and the backing provider
//! withdraws that exact debit as SPL tokens.
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
}
