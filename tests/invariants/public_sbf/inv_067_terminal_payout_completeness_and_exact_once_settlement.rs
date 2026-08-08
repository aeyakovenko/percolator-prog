//! INV-067 - Terminal payout completeness and exact-once settlement.
//!
//! Normative obligation: Each valid claim is paid, forfeited, or receipted exactly once without silent loss.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr283_one_atom_erases_terminal_victim_payout`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_pr283_one_atom_erases_terminal_victim_payout() {
    for route in [TradeRoute::NoCpi, TradeRoute::BatchNoCpi] {
        let reproduction = reproduce_terminal_dust_payout_erasure([0x83; 32], route)
            .unwrap_or_else(|error| panic!("PR 283 {route:?} no longer reproduces: {error}"));
        assert_eq!(
            reproduction.blocker,
            KnownBlocker::TerminalDustPayoutErasure
        );
        assert_eq!(reproduction.route, route);
        assert_eq!(reproduction.attacker_loss, 1);
        assert!(reproduction.victim_loss > 8_000_000_000);
        assert_eq!(
            reproduction.vault_remaining,
            reproduction.victim_loss + reproduction.attacker_loss
        );
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
