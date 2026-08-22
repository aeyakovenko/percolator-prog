//! INV-074 - Scope locality (wrapper domain-withdrawal loss gate).
//!
//! The engine owns source-credit realizability and the exact backing withdrawal transition. This
//! wrapper proof exhausts the full-width active-loss inputs used before backing and insurance
//! withdrawal transitions. Withdrawal is blocked iff there is a live negative account, a
//! selected-domain loss barrier, threshold
//! stress, stale loss state, or Recovery. Historical bankruptcy is intentionally not an input: the
//! public LiteSVM terminal route keeps that audit bit set while proving all active blockers are zero,
//! exact provider and remaining-insurance settlements succeed, and the empty asset restarts.

use super::*;

#[kani::proof]
fn kani_v16_domain_withdrawal_uses_active_loss_not_bankruptcy_history() {
    let negative_pnl_account_count: u64 = kani::any();
    let pending_domain_loss_barrier: u64 = kani::any();
    let threshold_stress_active: bool = kani::any();
    let loss_stale_active: bool = kani::any();
    let recovery_active: bool = kani::any();

    let blocked = policy_v16::domain_withdrawal_has_active_loss(
        negative_pnl_account_count,
        pending_domain_loss_barrier,
        threshold_stress_active,
        loss_stale_active,
        recovery_active,
    );
    assert_eq!(
        blocked,
        negative_pnl_account_count != 0
            || pending_domain_loss_barrier != 0
            || threshold_stress_active
            || loss_stale_active
            || recovery_active
    );

    kani::cover!(!blocked, "settled loss state admits the exact engine gate");
    kani::cover!(
        negative_pnl_account_count != 0 && blocked,
        "a live negative account blocks withdrawal"
    );
    kani::cover!(
        pending_domain_loss_barrier != 0 && blocked,
        "the selected domain's live loss barrier blocks withdrawal"
    );
    kani::cover!(
        threshold_stress_active && blocked,
        "threshold stress blocks withdrawal"
    );
    kani::cover!(
        loss_stale_active && blocked,
        "stale loss state blocks withdrawal"
    );
    kani::cover!(recovery_active && blocked, "Recovery blocks withdrawal");
}
