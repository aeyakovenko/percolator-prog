//! INV-020 - Authenticated clock, slot, and oracle provenance.
//!
//! Normative obligation: Time and oracle observations are authenticated, coherent, and cannot be caller-rewound.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_temporally_skewed_composite_rejects_atomically_and_exit_stays_live`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: this finite public matrix covers both one-leg-fresh directions and an
//! all-legs-fresh cross-epoch report, followed by a coherent control and complete owner exit.
//! The row-426 control distinguishes partial market accrual from completed live-account
//! refresh: a coherent price change leaves the account unchanged and its certificate stale
//! during partial steps, then certifies exact input-priced health and component provenance
//! at completion. That control alone does not close rows 416/422/426. The metadata guard below
//! preserves row 426's separate omission/prior-slot rescue witness and complete-observation
//! obligation; it adds no economic coverage or whole-invariant certification.

use super::*;

#[test]
fn v16_row426_metadata_retains_current_observation_omission_evidence() {
    let discoveries = include_str!("../independent_discoveries.tsv")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let fields = line.split('\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 5, "malformed discovery row: {line}");
            fields
        })
        .filter(|fields| fields[4].split(',').any(|row| row == "426"))
        .collect::<Vec<_>>();
    assert_eq!(
        discoveries.len(),
        1,
        "row 426 must retain exactly one reviewed current-observation discovery"
    );
    // Coherent supplied timestamps do not prove safety when a rescue observation
    // is omitted or a prior-slot report is replayed before liquidation.
    assert_eq!(
        &discoveries[0][..4],
        &[
            "INV-020",
            "hybrid-health/current-rescue",
            "v16_attack_liquidation_cannot_omit_fresh_external_rescue_observation",
            "current-hybrid-health-refresh-requires-fresh-authenticated-observation",
        ],
        "row 426 must preserve the omitted/prior-slot rescue and terminal-entitlement witness"
    );

    let reopenings = include_str!("../coverage_reopenings.tsv")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let fields = line.split('\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 8, "malformed reopening row: {line}");
            fields
        })
        .filter(|fields| fields[0] == "426")
        .collect::<Vec<_>>();
    assert_eq!(reopenings.len(), 1, "row 426 needs one complete obligation");
    let row = &reopenings[0];
    assert_eq!(row[3], "INV-020");
    assert!(row[4].split(',').any(|owner| owner == "INV-020"));
    assert_eq!(
        row[5], "missing-evidence+x-multi-step-refresh+x-liquidation",
        "supplied-report coherence alone cannot replace missing-evidence coverage"
    );
    assert_eq!(
        row[6], "favorable-account-actions-require-complete-current-authenticated-observations",
        "row 426 must retain complete current evidence before favorable account actions"
    );
}

#[test]
fn v16_program_temporally_skewed_composite_rejects_atomically_and_exit_stays_live() {
    let evidence = verify_composite_time_coherence([0x31; 32])
        .unwrap_or_else(|error| panic!("composite-time verification failed: {error}"));
    eprintln!("composite-time current-evidence control: {evidence:?}");
    assert!(evidence.is_protected(), "{evidence:?}");
}
