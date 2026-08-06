//! INV-051 - Canonical ADL-effective quantity.
//!
//! Normative obligation: every route uses the engine's pooled effective OI as the amount that can
//! still be reduced, while raw per-portfolio basis remains an attribution record. A route that
//! consumes the final effective OI cannot leave non-obligation raw basis in `Normal`: it must
//! atomically enter `ResetPending`, after which one bounded public auto-crank detaches the
//! prior-epoch residue without subtracting OI a second time.
//!
//! Evidence in this file (I/C/M over public routes): the crossed-trade and owner-signed unilateral
//! matrices independently create partial ADL through ordinary deposits, trades, authenticated
//! marks, maintenance, and permissionless cranks. Each then consumes exactly the remaining pooled
//! OI through a different public route. Both require `(oi_long, oi_short) == (0, 0)`, a
//! `ResetPending` side, exact rollback from direct routes that would double-subtract OI, one
//! bounded no-observation crank that removes the raw residue, conserved SPL custody, and recovery
//! of the owner's remaining capital. The stateful global oracle in `support/fuzz_model.rs` applies
//! the same zero-OI/reset condition after every successful generated public instruction.
//! Secondary coverage: INV-073, because each matrix also proves that the funded owner has a
//! bounded public cleanup and capital-exit sequence after pooled OI reaches zero.
//!
//! Guarantee boundary: this closes the exact-zero pooled-OI boundary for crossed trades and
//! unilateral rebalance. Liquidation reaches the same engine reset helper and has engine contract
//! coverage, but a distinct maximum-shape public liquidation matrix remains useful additional
//! evidence; this file does not claim exhaustive reachability over every ADL schedule.

#[test]
fn v16_program_crossed_adl_effective_exit_matrix_preserves_bounded_cleanup() {
    super::inv_073_no_permanent_user_lock::assert_inv_051_crossed_adl_effective_exit_matrix_preserves_bounded_cleanup();
}

#[test]
fn v16_program_unilateral_adl_effective_exit_matrix_preserves_bounded_cleanup() {
    super::inv_073_no_permanent_user_lock::assert_inv_051_unilateral_adl_effective_exit_matrix_preserves_bounded_cleanup();
}
