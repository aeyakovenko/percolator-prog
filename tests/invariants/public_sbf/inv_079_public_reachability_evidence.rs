//! INV-079 - Public reachability evidence.
//!
//! Normative obligation: Accepted LoF and DoS findings reproduce through valid public instructions and exact external effects.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_known_blockers_remain_explicit_until_fixed`, `v16_program_open_lof_manifest_snapshot_is_structurally_honest`, `v16_open_security_finding_benchmark_is_complete_and_non_overclaiming`, `v16_invariant_charter_and_index_are_complete`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_known_blockers_remain_explicit_until_fixed() {
    for (name, scenario) in known_blocker_scenarios() {
        let coverage = run_scenario(&scenario).unwrap_or_else(|error| {
            panic!(
                "known blocker scenario {name} changed failure class\nscenario={}\n{error}",
                serde_json::to_string_pretty(&scenario).unwrap()
            )
        });
        let index = KnownBlocker::LiveLapsedSourceBacking.index();
        assert_ne!(
            coverage.known_blocker_hits[index], 0,
            "{name} no longer reproduces PR 204; remove its quarantine and promote the seed"
        );
        assert_eq!(
            coverage.known_blocker_exit_locks[index], 0,
            "{name} must not claim a persistent user-exit lock when authenticated same-price \
             observations let the owner exit"
        );
    }
}

#[test]
fn v16_program_open_lof_manifest_snapshot_is_structurally_honest() {
    validate_manifest().expect("open LoF manifest structure");
    assert_eq!(
        quarantined_prs(),
        [
            220, 223, 224, 225, 231, 251, 253, 255, 260, 264, 265, 267, 271, 272, 273, 274, 275,
            276, 277, 278, 279, 280, 281, 282, 283, 285, 290, 294, 295, 296, 299, 301, 303, 304,
            305, 307, 309, 310, 311, 314, 315, 317, 318, 320, 321, 322, 325, 326, 328, 329, 331,
            332, 333, 334, 335, 336, 337, 338, 339, 340, 343, 344, 345, 346, 347, 349, 350, 351,
            353, 355, 356, 362, 365, 366, 367, 369, 380, 381
        ]
    );
    let missing = missing_prs();
    assert_eq!(
        missing.len(),
        21,
        "update the explicit evidence state when an executable adapter lands"
    );
    assert!(!missing.contains(&220));
    assert!(!missing.contains(&223));
    assert!(!missing.contains(&224));
    assert!(!missing.contains(&225));
    assert!(!missing.contains(&231));
    assert!(!missing.contains(&251));
    assert!(!missing.contains(&253));
    assert!(!missing.contains(&255));
    assert!(!missing.contains(&260));
    assert!(!missing.contains(&264));
    assert!(!missing.contains(&265));
    assert!(!missing.contains(&267));
    assert!(!missing.contains(&271));
    assert!(!missing.contains(&272));
    assert!(!missing.contains(&273));
    assert!(!missing.contains(&274));
    assert!(!missing.contains(&275));
    assert!(!missing.contains(&276));
    assert!(!missing.contains(&277));
    assert!(!missing.contains(&278));
    assert!(!missing.contains(&279));
    assert!(!missing.contains(&280));
    assert!(!missing.contains(&281));
    assert!(!missing.contains(&282));
    assert!(!missing.contains(&283));
    assert!(!missing.contains(&285));
    assert!(!missing.contains(&290));
    assert!(!missing.contains(&294));
    assert!(!missing.contains(&295));
    assert!(!missing.contains(&296));
    assert!(!missing.contains(&299));
    assert!(!missing.contains(&301));
    assert!(!missing.contains(&303));
    assert!(!missing.contains(&304));
    assert!(!missing.contains(&305));
    assert!(!missing.contains(&307));
    assert!(!missing.contains(&309));
    assert!(!missing.contains(&310));
    assert!(!missing.contains(&311));
    assert!(!missing.contains(&314));
    assert!(!missing.contains(&315));
    assert!(!missing.contains(&317));
    assert!(!missing.contains(&318));
    assert!(!missing.contains(&320));
    assert!(!missing.contains(&321));
    assert!(!missing.contains(&322));
    assert!(!missing.contains(&325));
    assert!(!missing.contains(&326));
    assert!(!missing.contains(&328));
    assert!(!missing.contains(&329));
    assert!(!missing.contains(&331));
    assert!(!missing.contains(&332));
    assert!(!missing.contains(&333));
    assert!(!missing.contains(&334));
    assert!(!missing.contains(&335));
    assert!(!missing.contains(&336));
    assert!(!missing.contains(&337));
    assert!(!missing.contains(&338));
    assert!(!missing.contains(&339));
    assert!(!missing.contains(&340));
    assert!(!missing.contains(&343));
    assert!(!missing.contains(&344));
    assert!(!missing.contains(&345));
    assert!(!missing.contains(&346));
    assert!(!missing.contains(&347));
    assert!(!missing.contains(&349));
    assert!(!missing.contains(&350));
    assert!(!missing.contains(&351));
    assert!(!missing.contains(&353));
    assert!(!missing.contains(&355));
    assert!(!missing.contains(&356));
    assert!(!missing.contains(&362));
    assert!(!missing.contains(&365));
    assert!(!missing.contains(&366));
    assert!(!missing.contains(&367));
    assert!(!missing.contains(&369));
    assert!(!missing.contains(&380));
    assert!(!missing.contains(&381));
}

#[test]
fn v16_open_security_finding_benchmark_is_complete_and_non_overclaiming() {
    let mut prior_pr = 0u16;
    let mut rows = 0usize;
    let mut direct = 0usize;
    let mut missing = 0usize;
    let mut independent = 0usize;
    let mut benchmark_evidence = std::collections::BTreeMap::new();

    for line in include_str!("../open_findings.tsv").lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.splitn(6, '\t').collect();
        assert_eq!(fields.len(), 6, "malformed finding row: {line}");
        let pr: u16 = fields[0].parse().expect("numeric PR ID");
        assert!(pr > prior_pr, "finding PRs must be unique and sorted");
        prior_pr = pr;
        assert!(matches!(fields[1], "LoF" | "DoS"));
        assert!(matches!(
            fields[2],
            "BLOCKER" | "REAL" | "HARDENING" | "PRIVILEGED"
        ));
        let invariant: u16 = fields[3]
            .strip_prefix("INV-")
            .expect("mapped invariant")
            .parse()
            .expect("numeric invariant ID");
        assert!((1..=89).contains(&invariant));
        match fields[4] {
            "direct-regression" => direct += 1,
            "missing" => missing += 1,
            "independent-discovery" => independent += 1,
            "certified" => {}
            evidence => panic!("unknown evidence level {evidence}"),
        }
        benchmark_evidence.insert(pr, fields[4]);
        rows += 1;
    }

    assert_eq!(rows, 143, "refresh the dated GitHub finding snapshot");
    assert_eq!(direct, 0, "direct adapter inventory changed");
    assert_eq!(missing, 47, "explicit finding gaps changed");
    assert_eq!(
        independent, 96,
        "promote only genuinely finding-agnostic invariant discoveries"
    );

    let independent_sources = [
        include_str!("../stateful/inv_001_market_incarnation_binding.rs"),
        include_str!("../stateful/inv_002_asset_generation_binding.rs"),
        include_str!("../stateful/inv_003_portfolio_incarnation_binding.rs"),
        include_str!("../stateful/inv_004_position_episode_binding.rs"),
        include_str!("../stateful/inv_005_authority_incarnation_binding.rs"),
        include_str!("../stateful/inv_008_intent_uniqueness_and_bounded_replay.rs"),
        include_str!("../stateful/inv_010_out_of_order_safety.rs"),
        include_str!("../stateful/inv_014_delayed_policy_and_policy_epoch_safety.rs"),
        include_str!("../stateful/inv_020_authenticated_clock_slot_and_oracle_provenance.rs"),
        include_str!("../stateful/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs"),
        include_str!("../stateful/inv_034_domain_and_instance_isolation.rs"),
        include_str!("../stateful/inv_035_no_global_b_pool_residuals_remain_local.rs"),
        include_str!("../stateful/inv_036_fee_destination_and_policy_version_integrity.rs"),
        include_str!("../stateful/inv_038_rounding_and_ratio_conservation.rs"),
        include_str!("../stateful/inv_039_pending_loss_obligation_durability.rs"),
        include_str!("../stateful/inv_045_no_free_mark_movement.rs"),
        include_str!("../stateful/inv_053_full_health_recertification_equivalence.rs"),
        include_str!("../stateful/inv_061_deterministic_bounded_liquidation.rs"),
        include_str!("../stateful/inv_063_backing_expiry_normalization.rs"),
        include_str!(
            "../stateful/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs"
        ),
    ];
    let mut fingerprints = std::collections::BTreeSet::new();
    let mut mapped_prs = std::collections::BTreeSet::new();
    for line in include_str!("../independent_discoveries.tsv").lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.splitn(5, '\t').collect();
        assert_eq!(fields.len(), 5, "malformed discovery row: {line}");
        let invariant: u16 = fields[0]
            .strip_prefix("INV-")
            .expect("mapped discovery invariant")
            .parse()
            .expect("numeric discovery invariant ID");
        assert!((1..=89).contains(&invariant));
        assert!(
            fingerprints.insert((invariant, fields[1])),
            "duplicate invariant discovery fingerprint: {} {}",
            fields[0],
            fields[1]
        );
        assert!(
            independent_sources
                .iter()
                .any(|source| source.contains(&format!("fn {}", fields[2]))),
            "discovery generator is not an executable invariant-owned test: {}",
            fields[2]
        );
        assert!(
            matches!(
                fields[3],
                "stale-intent-must-reject-and-roll-back-exactly"
                    | "same-economic-intent-executes-at-most-once-and-rejection-rolls-back"
                    | "newer-authorized-control-cannot-be-overwritten-by-stale-intent"
                    | "signer-debit-never-exceeds-consented-fee-terms"
                    | "provider-approved-fee-split-is-durable-and-attributed"
                    | "pending-balanced-transfer-cannot-be-erased-by-operation-order"
                    | "pending-value-must-commit-before-terminal-snapshot"
                    | "prospective-accrual-cannot-be-rewritten-by-trade-order"
                    | "pending-mark-cannot-authorize-stale-price-risk-increase"
                    | "mark-movement-cost-must-cover-later-third-party-transfer"
                    | "later-trade-cannot-cheaply-rewrite-prior-pending-mark"
                    | "fee-reward-cannot-outrank-uncommitted-adverse-value"
                    | "mark-movement-reserve-must-remain-encumbered"
                    | "mark-movement-cost-must-cover-liquidation-extraction"
                    | "mark-movement-fees-must-be-bilaterally-supported"
                    | "composite-price-is-rounded-once-after-exact-composition"
                    | "omitted-observation-cannot-erase-balanced-rounded-transfer"
                    | "fractional-cap-residue-must-accumulate-to-target"
                    | "composite-oracle-legs-must-share-one-coherent-observation-epoch"
                    | "terminal-payout-is-invariant-to-flattened-dust-position"
                    | "insurance-spend-remains-source-domain-local"
                    | "backing-atoms-cannot-support-claims-from-another-source"
                    | "b-loss-reduces-only-the-originating-source-domain"
                    | "liquidation-certificate-cannot-be-healthier-than-full-refresh"
                    | "expired-backing-cannot-create-withdrawable-provider-value"
                    | "expired-backing-cannot-capitalize-and-extract-independent-principal"
                    | "expired-retained-operation-cannot-consume-principal-and-lock-terminal-users"
                    | "old-generation-terminal-capability-cannot-crystallize-replacement-value"
                    | "stale-position-episode-consent-cannot-transfer-or-orphan-value"
                    | "stale-matcher-enable-cannot-revive-revoked-value-authority"
                    | "funded-role-principal-cannot-be-redirected-without-incumbent-consent"
                    | "committed-funding-must-accrue-before-lifecycle-terminalization"
                    | "terminal-snapshot-must-use-current-authenticated-oracle-state"
                    | "funded-resolved-adl-winner-has-bounded-public-exit"
                    | "unsigned-lp-cannot-inherit-preexisting-settlement-cohort"
            ),
            "unknown independent oracle: {}",
            fields[3]
        );
        for raw_pr in fields[4].split(',') {
            let pr: u16 = raw_pr.parse().expect("numeric discovery PR ID");
            assert_eq!(
                benchmark_evidence.get(&pr),
                Some(&"independent-discovery"),
                "discovery mapping must point to a promoted benchmark row"
            );
            mapped_prs.insert(pr);
        }
    }

    let promoted_prs: std::collections::BTreeSet<_> = benchmark_evidence
        .iter()
        .filter_map(|(pr, evidence)| (*evidence == "independent-discovery").then_some(*pr))
        .collect();
    assert_eq!(
        mapped_prs, promoted_prs,
        "every promoted benchmark row needs a finding-agnostic fingerprint"
    );
}

fn invariant_ids(markdown: &str, prefix: &str) -> Vec<u16> {
    markdown
        .lines()
        .filter_map(|line| {
            line.strip_prefix(prefix)
                .and_then(|tail| tail.get(..3))
                .and_then(|digits| digits.parse().ok())
        })
        .collect()
}

#[test]
fn v16_invariant_charter_and_index_are_complete() {
    let expected: Vec<u16> = (1..=89).collect();
    assert_eq!(
        invariant_ids(include_str!("../../../INVARIANTS.md"), "### INV-"),
        expected,
        "the normative charter must define INV-001 through INV-089 exactly once and in order"
    );
    assert_eq!(
        invariant_ids(include_str!("../README.md"), "| INV-"),
        expected,
        "the executable coverage index must account for every normative invariant"
    );
}

#[test]
fn v16_program_invariant_harnesses_are_test_free_roots() {
    for (name, source) in [
        (
            "deterministic",
            include_str!("../../v16_program_fuzz_regressions.rs"),
        ),
        (
            "stateful",
            include_str!("../../v16_program_stateful_fuzz.rs"),
        ),
    ] {
        assert!(
            source.lines().all(|line| line.trim() != "#[test]"),
            "{} harness contains an unowned test; move it to \
             tests/invariants/<suite>/INV-NNN",
            name
        );
    }
}
