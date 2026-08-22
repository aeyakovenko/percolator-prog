//! INV-061 - Deterministic, bounded liquidation.
//!
//! Normative obligation: liquidation and terminal settlement are deterministic, risk reducing,
//! OI coherent, and bounded. A publicly created ADL-scaled winner must retain a finite public exit
//! after resolution regardless of whether the winner or loser submits `CloseResolved` first.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_resolved_adl_close_order_matrix_preserves_funded_exits` opens an ordinary
//! matched position, moves an authenticated mark, and uses permissionless cranks to create a winner
//! whose stored basis exceeds effective long OI. It drives both owner-signed automatic-crank
//! landing orders until both users are terminal. Every accepted call must
//! mutate, each user receives exactly its funded value, SPL and internal custody reconcile to zero,
//! token supply is conserved, and both portfolio accounts close.
//!
//! Guarantee boundary: this matrix covers both two-user terminal landing orders for a single ADL
//! asset. Multi-asset and partial-receipt close compositions remain in the audit ledger.

use super::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 4) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 8) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_061_resolved_adl_close_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_resolved_adl_close_order_matrix_preserves_funded_exits(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = verify_resolved_adl_close_orders(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), ResolvedAdlCloseOrder::ALL.len());
        for (expected, discovery) in ResolvedAdlCloseOrder::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.order, expected);
            prop_assert!(
                discovery.satisfies_invariant(),
                "resolved-ADL close-order invariant failed: {discovery:?}"
            );
        }
    }
}
