//! INV-026 - reservation and encumbrance conservation is separate from token value.
//!
//! This bounded public-SBF matrix covers all four trade families and both source
//! sides. Every world must create a nonzero counterparty-backed initial-margin
//! lien, then preserve these independently recomputed equations after each public
//! transition:
//!
//! - source fresh backing equals bucket fresh-unliened plus valid-liened backing;
//! - source valid, impaired, consumed, and provider-receivable classes agree with
//!   the backing bucket without overlap;
//! - account-local counterparty backing equals the market's valid plus impaired
//!   counterparty backing, including the expiry representation;
//! - account-local insurance backing agrees separately with valid and impaired
//!   insurance reservations; and
//! - classified backing equals effective reserved credit times `BOUND_SCALE`.
//!
//! The terminal half resolves the live market and requires the persisted account
//! lien to disappear, valid/impaired market labels to clear, and backing to enter
//! the consumed/provider-receivable class exactly once. SPL supply must not move.
//! The shared census also runs after every generated public action in the stateful
//! runner. Direct insurance-backed lien creation remains unavailable through the
//! wrapper API and is therefore not claimed by this test.

use super::*;

#[test]
fn v16_program_counterparty_encumbrance_lifecycle_is_exact_across_routes_and_sides() {
    verify_counterparty_encumbrance_route_matrix([0x26; 32])
        .unwrap_or_else(|error| panic!("INV-026 encumbrance route matrix: {error}"));
}
