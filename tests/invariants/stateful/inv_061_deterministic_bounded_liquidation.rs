//! INV-061 - Deterministic, bounded liquidation.
//!
//! Normative obligation: liquidation and terminal settlement are deterministic, risk reducing,
//! OI coherent, and bounded. A publicly created ADL-scaled winner must retain a finite public exit
//! after resolution regardless of whether the winner or loser submits `CloseResolved` first.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_resolved_adl_close_order_matrix_discovers_funded_exit_lock` opens an ordinary
//! matched position, moves an authenticated mark, and uses permissionless cranks to create a winner
//! whose stored basis exceeds effective long OI. For both close orders it closes the loser, then
//! searches the owner-signed resolved close, withdrawal, and portfolio-close routes. A violation
//! requires eight identical engine-underflow failures, exact program/SPL/lamport rollback, zero
//! external payout, and canonical SPL vault liquidity covering the winner's nonzero funded value.
//!
//! Guarantee boundary: this is a public reachability counterexample on the vulnerable pin. On a
//! fixed pin the same matrix must instead assert bounded terminal payout and exact custody
//! reconciliation.

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
    fn v16_program_resolved_adl_close_order_matrix_discovers_funded_exit_lock(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_resolved_adl_close_locks(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), ResolvedAdlCloseOrder::ALL.len());
        for (expected, discovery) in ResolvedAdlCloseOrder::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.order, expected);
            prop_assert!(
                discovery.is_violation(),
                "resolved-ADL close-order behavior changed: {discovery:?}"
            );
        }
    }
}
