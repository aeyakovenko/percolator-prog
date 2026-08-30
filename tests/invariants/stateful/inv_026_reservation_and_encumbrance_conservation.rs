//! INV-026 - reservation and encumbrance conservation is separate from token value.
//!
//! This bounded public-SBF matrix covers all four trade families, both source
//! sides, and both Resolved and Recovery terminal paths (16 worlds). Every world
//! must create a nonzero counterparty-backed initial-margin lien, then preserve
//! these independently recomputed equations after each public transition:
//!
//! - source fresh backing equals bucket fresh-unliened plus valid-liened backing;
//! - source valid, impaired, consumed, and provider-receivable classes agree with
//!   the backing bucket without overlap;
//! - account-local counterparty backing equals the market's valid plus impaired
//!   counterparty backing, including the expiry representation;
//! - account-local insurance backing agrees separately with valid and impaired
//!   insurance reservations; and
//! - classified backing equals effective reserved credit times `BOUND_SCALE`.
//! - every zero-basis loss-weight obligation has one account owner and one exact
//!   market side counter; and
//! - every close ledger preserves its exact gross-loss-plus-drift partition and
//!   valid active/canceled/finalized lifecycle shape.
//!
//! The Resolved path requires the persisted account lien to disappear,
//! valid/impaired market labels to clear, and backing to enter the
//! consumed/provider-receivable class exactly once. The Recovery path forfeits the
//! unrelated adverse leg, closes the surviving live leg at the current mark, and
//! requires bounded public cranks to release its risk lien back to the exact
//! pre-lien fresh-backing class without consuming the still-live source claim.
//! SPL supply must not move in either path. The
//! shared census also runs after every generated public action in the stateful
//! runner. Direct insurance-backed lien creation remains unavailable through the
//! wrapper API and is therefore not claimed by this test.
//! The current wrapper also has no public writer for `cancel_deposit_escrow`; the
//! shared census fails if that dormant lane becomes reachable without a new owner.

use super::*;

#[test]
fn v16_program_counterparty_encumbrance_lifecycle_is_exact_across_routes_sides_and_terminal_modes()
{
    verify_counterparty_encumbrance_route_matrix([0x26; 32])
        .unwrap_or_else(|error| panic!("INV-026 encumbrance route matrix: {error}"));
}
