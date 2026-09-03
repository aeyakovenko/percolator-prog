//! INV-067 - Terminal payout completeness and exact-once settlement.
//!
//! Normative obligation: Each valid claim is paid, forfeited, or receipted exactly once without silent loss.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_one_atom_source_haircut_preserves_terminal_victim_payout`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: this pins the fixed public source-haircut route; broader claim episodes
//! and terminal reachability remain tracked in the invariant roadmap.

use super::*;

#[test]
fn v16_program_one_atom_source_haircut_preserves_terminal_victim_payout() {
    for route in [TradeRoute::NoCpi, TradeRoute::BatchNoCpi] {
        let reproduction = verify_terminal_dust_payout_protection([0x83; 32], route)
            .unwrap_or_else(|error| panic!("terminal protection failed for {route:?}: {error}"));
        assert_eq!(reproduction.route, route);
        assert_eq!(reproduction.attacker_loss, 1);
        assert_eq!(reproduction.victim_loss, 0);
        assert_eq!(reproduction.vault_remaining, reproduction.attacker_loss);
        assert_eq!(
            reproduction.attacker_withdrawn + reproduction.attacker_loss,
            20_000_002_000
        );
        assert_eq!(
            reproduction.victim_withdrawn + reproduction.victim_loss,
            20_000_000_000
        );
    }
}
