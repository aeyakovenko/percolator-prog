//! INV-067 - Terminal payout completeness and exact-once settlement.
//!
//! Normative obligation: Each valid claim is paid, forfeited, or receipted exactly once without silent loss.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_one_atom_source_haircut_preserves_terminal_victim_payout`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//! A metadata guard binds row 417 to its late-backing-expiry receipt witness and mounted
//! stateful owner, while retaining the broader claim-identity obligation as OPEN.
//!
//! Guarantee boundary: this pins the fixed public source-haircut route; broader claim episodes
//! and terminal reachability remain tracked in the invariant roadmap.

use super::*;

#[test]
fn v16_row417_metadata_retains_late_expiry_receipt_evidence() {
    fn row417(tsv: &str, width: usize) -> Vec<&str> {
        let rows = tsv
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
            .map(|line| line.split('\t').collect::<Vec<_>>())
            .filter(|fields| fields[0] == "417")
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 1, "row 417 must have one metadata entry");
        assert_eq!(rows[0].len(), width, "row 417 metadata schema changed");
        rows.into_iter().next().unwrap()
    }

    let finding = row417(include_str!("../open_findings.tsv"), 6);
    let reopening = row417(include_str!("../coverage_reopenings.tsv"), 8);
    assert_eq!(finding[3], "INV-067");
    assert_eq!(finding[4], "independent-discovery");
    assert_eq!(reopening[3], finding[3]);
    assert!(reopening[4].split(',').any(|id| id == finding[3]));
    assert_eq!(reopening[5], "receipt+x-late-expiry+x-claimant-order");
    assert_eq!(
        reopening[6], "terminal-claim-identity-survives-every-later-stock-reclassification",
        "a single expiry trace cannot replace the whole claim-identity obligation"
    );
    assert_eq!(
        reopening[7], "OPEN",
        "bounded mechanism coverage does not close arbitrary receipt histories"
    );

    let discoveries = include_str!("../independent_discoveries.tsv")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(|fields| fields.len() == 5 && fields[4].split(',').any(|pr| pr == "417"))
        .collect::<Vec<_>>();
    assert!(
        discoveries.iter().any(|fields| fields[..4] == [
            "INV-067",
            "resolved-receipt/late-backing-expiry",
            "v16_program_late_unrelated_backing_cannot_outlive_and_erase_resolved_receipt",
            "resolved-receipt-must-survive-late-backing-expiry-and-pay-exactly-once",
        ]),
        "row 417 must retain its late-expiry receipt generator and oracle, not an unrelated payout witness"
    );

    assert!(
        include_str!("../../v16_program_stateful_fuzz.rs").contains(concat!(
            "#[path = \"invariants/stateful/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs\"]\n",
            "mod inv_067_terminal_payout_completeness_and_exact_once_settlement;",
        )),
        "the row 417 discovery owner must remain mounted in the stateful harness"
    );
}

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
