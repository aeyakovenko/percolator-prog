//! INV-063 - Backing-expiry normalization (wrapper landing gate).
//!
//! The engine owns backing accounting and the bounded expiry transition. This wrapper proof owns
//! the authenticated instruction boundary: provider principal is withdrawable only while the
//! bucket is strictly fresh. It exhausts both full-width slot inputs without assumptions, including
//! the equal-slot boundary that the public LiteSVM regression composes with engine progress.

use super::*;

#[kani::proof]
fn kani_v16_backing_principal_withdrawal_is_strictly_pre_expiry() {
    let expiry_slot: u64 = kani::any();
    let authenticated_slot: u64 = kani::any();

    let accepted =
        policy_v16::backing_principal_withdrawal_is_fresh(expiry_slot, authenticated_slot);
    assert_eq!(accepted, authenticated_slot < expiry_slot);
    if authenticated_slot >= expiry_slot {
        assert!(!accepted);
    }

    kani::cover!(
        authenticated_slot < expiry_slot,
        "strictly pre-expiry withdrawal is admitted"
    );
    kani::cover!(
        authenticated_slot == expiry_slot,
        "equal-slot withdrawal is rejected"
    );
    kani::cover!(
        authenticated_slot > expiry_slot,
        "post-expiry withdrawal is rejected"
    );
}
