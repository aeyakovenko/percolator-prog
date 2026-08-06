//! INV-002 - Asset generation binding.
//!
//! Normative obligation: a retained asset-specific trade binds the program-assigned asset
//! generation, so retirement and reuse of the same slot cannot redirect old consent to the
//! replacement asset.
//!
//! Evidence in this file (I/C):
//! `v16_attack_signed_trade_cannot_replay_across_asset_slot_reuse` constructs and signs each of
//! the four public trade routes against generation A, retires and permissionlessly reactivates the
//! same slot as generation B, and lands the retained transaction. Every route must return the
//! dedicated generation error with byte-exact rollback, including matcher context on CPI routes.
//! A freshly encoded generation-B trade must then land and create only generation-B legs, proving
//! the guard is not a blanket trading DoS. No program-owned state is injected.
//!
//! Guarantee boundary: this covers signed trade consent. Other retained asset-scoped controls are
//! tracked independently by the public-SBF and stateful INV-002 operation matrix.

use super::*;

#[test]
fn v16_attack_signed_trade_cannot_replay_across_asset_slot_reuse() {
    assert_signed_trade_cannot_replay_across_asset_slot_reuse();
}
