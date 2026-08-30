//! INV-039 - Pending-loss obligation durability.
//!
//! This CU/SVM owner invokes the shared public Recovery-order world formerly owned by INV-073.
//! Both owner landing orders create a real zero-basis, nonzero-loss-weight obligation. While it is
//! retained, `ClosePortfolio` must return an instruction error with exact market, portfolio, vault,
//! and lamport rollback. The opposite owner then exits, permissionless cranks release the
//! obligation in bounded work, every loss-weight/count aggregate reaches zero, all users receive
//! their exact terminal entitlement, and all portfolios close. A valid pending obligation does not
//! coexist with `ResetPending`: the pinned engine's retain/release/clear contracts and
//! `proof_v16_public_finalize_side_reset_rejects_each_blocker_without_mutation` own that
//! unreachability and exact reset-gate frame, while the wrapper exposes no direct state writer.
//! INV-088 source-rosters every wrapper-to-engine transition, so a new removal route reopens this
//! composition. The same run remains cross-owned evidence for INV-073 without duplicating an
//! expensive public lifecycle.

#[test]
fn v16_program_pending_obligation_blocks_close_then_releases() {
    super::inv_073_no_permanent_user_lock::
        verify_recovery_forfeit_orders_preserve_loss_and_terminal_exit();
}
