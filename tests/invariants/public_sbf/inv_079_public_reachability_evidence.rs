//! INV-079 - Public reachability evidence.
//!
//! Normative obligation: Accepted LoF and DoS findings reproduce through valid public instructions and exact external effects.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_public_trace_schema_detects_out_of_band_economic_mutation`, `v16_program_fixed_blockers_remain_progressing`, `v16_program_open_lof_manifest_snapshot_is_structurally_honest`, `v16_open_security_finding_benchmark_is_complete_and_non_overclaiming`, `v16_invariant_charter_and_index_are_complete`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;
use crate::support::v16_svm::{MarketConfig, V16Svm};
use solana_sdk::signature::Signer;

#[test]
fn v16_public_trace_schema_detects_out_of_band_economic_mutation() {
    let mut env = V16Svm::new([0x79; 32], MarketConfig::default());
    env.begin_public_trace();

    let mut foreign_market = env
        .svm
        .get_account(&env.foreign_market)
        .expect("foreign market fixture");
    foreign_market.lamports = foreign_market
        .lamports
        .checked_add(1)
        .expect("mutation sentinel lamports");
    env.svm
        .set_account(env.foreign_market, foreign_market)
        .expect("install deliberate out-of-band mutation sentinel");

    env.deposit_primary(0, 1)
        .expect("public call after mutation sentinel");
    env.withdraw_primary(0, u128::MAX)
        .expect_err("unrepresentable public withdrawal must reject");
    let trace = env.finish_public_trace();
    assert_eq!(
        trace.out_of_band_economic_mutations, 1,
        "trace schema must reject hidden state-injection evidence"
    );
    assert_eq!(trace.steps.len(), 2);
    let step = &trace.steps[0];
    assert_eq!(step.program_id, percolator_prog::id());
    assert!(step.succeeded);
    assert!(step.compute_units.is_some());
    assert!(step
        .transaction_signers
        .contains(&env.actors[0].signer.pubkey()));
    assert!(step
        .accounts
        .iter()
        .any(|meta| meta.key == env.actors[0].signer.pubkey() && meta.is_signer));
    assert!(step
        .token_deltas
        .contains(&(env.actors[0].source_token, -1)));
    assert!(step.token_deltas.contains(&(env.vault, 1)));
    assert!(!step.lamport_deltas.is_empty());

    let rejected = &trace.steps[1];
    assert_eq!(rejected.program_id, percolator_prog::id());
    assert!(!rejected.succeeded);
    assert_eq!(rejected.compute_units, None);
    assert_eq!(rejected.rejected_exact_writable_rollback, Some(true));
    assert_eq!(rejected.rejected_no_program_lamport_delta, Some(true));
    assert!(rejected.token_deltas.iter().all(|(_, delta)| *delta == 0));
    assert!(rejected.lamport_deltas.iter().all(|(key, delta)| {
        if *key == rejected.fee_payer {
            *delta < 0
        } else {
            *delta == 0
        }
    }));
}

#[test]
fn v16_program_fixed_blockers_remain_progressing() {
    for (name, scenario) in fixed_blocker_scenarios() {
        let coverage = run_scenario(&scenario).unwrap_or_else(|error| {
            panic!(
                "fixed blocker scenario {name} no longer converges\nscenario={}\n{error}",
                serde_json::to_string_pretty(&scenario).unwrap()
            )
        });
        let index = KnownBlocker::LiveLapsedSourceBacking.index();
        assert_eq!(
            coverage.known_blocker_hits[index], 0,
            "{name} still reaches the PR 204 quarantine"
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
    let mut nonqualifying = 0usize;
    let mut benchmark_evidence = std::collections::BTreeMap::new();
    let mut benchmark_invariants = std::collections::BTreeMap::new();

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
            "nonqualifying" => nonqualifying += 1,
            "certified" => {}
            evidence => panic!("unknown evidence level {evidence}"),
        }
        benchmark_evidence.insert(pr, fields[4]);
        benchmark_invariants.insert(pr, invariant);
        rows += 1;
    }

    assert_eq!(rows, 143, "refresh the dated GitHub finding snapshot");
    assert_eq!(direct, 0, "direct adapter inventory changed");
    assert_eq!(missing, 0, "all benchmark rows need executable disposition");
    assert_eq!(
        independent, 126,
        "promote only genuinely finding-agnostic invariant discoveries"
    );
    assert_eq!(nonqualifying, 17, "nonqualifying evidence roster changed");

    let independent_sources: &[(u16, &[u16], &str)] = &[
        (
            1,
            &[1, 14],
            include_str!("../stateful/inv_001_market_incarnation_binding.rs"),
        ),
        (
            2,
            &[2],
            include_str!("../stateful/inv_002_asset_generation_binding.rs"),
        ),
        (
            3,
            &[3],
            include_str!("../stateful/inv_003_portfolio_incarnation_binding.rs"),
        ),
        (
            4,
            &[4],
            include_str!("../stateful/inv_004_position_episode_binding.rs"),
        ),
        (
            5,
            &[5],
            include_str!("../stateful/inv_005_authority_incarnation_binding.rs"),
        ),
        (
            8,
            &[8],
            include_str!("../stateful/inv_008_intent_uniqueness_and_bounded_replay.rs"),
        ),
        (
            10,
            &[10],
            include_str!("../stateful/inv_010_out_of_order_safety.rs"),
        ),
        (
            14,
            &[14, 36],
            include_str!("../stateful/inv_014_delayed_policy_and_policy_epoch_safety.rs"),
        ),
        (
            20,
            &[20],
            include_str!("../stateful/inv_020_authenticated_clock_slot_and_oracle_provenance.rs"),
        ),
        (
            27,
            &[27, 39],
            include_str!("../stateful/inv_027_protected_principal_seniority.rs"),
        ),
        (
            28,
            &[28],
            include_str!("../stateful/inv_028_source_domain_realizability_cap.rs"),
        ),
        (
            28,
            &[28, 30],
            include_str!("../cu/inv_028_source_domain_realizability_cap.rs"),
        ),
        (
            31,
            &[31],
            include_str!(
                "../stateful/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs"
            ),
        ),
        (
            34,
            &[34],
            include_str!("../stateful/inv_034_domain_and_instance_isolation.rs"),
        ),
        (
            35,
            &[35],
            include_str!("../stateful/inv_035_no_global_b_pool_residuals_remain_local.rs"),
        ),
        (
            36,
            &[36, 14],
            include_str!("../stateful/inv_036_fee_destination_and_policy_version_integrity.rs"),
        ),
        (
            36,
            &[36],
            include_str!("../cu/inv_036_fee_destination_and_policy_version_integrity.rs"),
        ),
        (
            38,
            &[38],
            include_str!("../stateful/inv_038_rounding_and_ratio_conservation.rs"),
        ),
        (
            39,
            &[39],
            include_str!("../stateful/inv_039_pending_loss_obligation_durability.rs"),
        ),
        (
            45,
            &[45],
            include_str!("../stateful/inv_045_no_free_mark_movement.rs"),
        ),
        (
            51,
            &[51, 73],
            include_str!("../cu/inv_051_canonical_adl_effective_quantity.rs"),
        ),
        (
            53,
            &[53],
            include_str!("../stateful/inv_053_full_health_recertification_equivalence.rs"),
        ),
        (
            61,
            &[61],
            include_str!("../stateful/inv_061_deterministic_bounded_liquidation.rs"),
        ),
        (
            61,
            &[61],
            include_str!("../cu/inv_061_deterministic_bounded_liquidation.rs"),
        ),
        (
            63,
            &[63],
            include_str!("../stateful/inv_063_backing_expiry_normalization.rs"),
        ),
        (
            63,
            &[63],
            include_str!("../cu/inv_063_backing_expiry_normalization.rs"),
        ),
        (
            67,
            &[67],
            include_str!(
                "../stateful/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs"
            ),
        ),
        (
            67,
            &[67],
            include_str!("../cu/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs"),
        ),
        (71, &[71], include_str!("../cu/inv_071_crank_progress.rs")),
        (
            73,
            &[73],
            include_str!("../cu/inv_073_no_permanent_user_lock.rs"),
        ),
        (74, &[74], include_str!("../cu/inv_074_scope_locality.rs")),
        (
            77,
            &[77],
            include_str!("../cu/inv_077_bounded_work_and_maximum_shape_compute.rs"),
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
            independent_sources.iter().any(|(owner, covered, source)| {
                covered.contains(&invariant)
                    && source_defines_test(source, fields[2])
                    && (*owner == invariant
                        || source.contains(&format!("Secondary coverage: INV-{invariant:03}")))
            }),
            "discovery generator is not an executable INV-{invariant:03}-owned or explicitly \
             secondary test: {}",
            fields[2],
        );
        assert!(
            include_str!("../README.md").lines().any(|line| {
                line.starts_with(&format!("| INV-{invariant:03} |"))
                    && line.contains("| Independent")
            }),
            "INV-{invariant:03} has discovery metadata but its coverage row is not Independent"
        );
        assert!(
            matches!(
                fields[3],
                "stale-intent-must-reject-and-roll-back-exactly"
                    | "same-economic-intent-executes-at-most-once-and-rejection-rolls-back"
                    | "newer-authorized-control-cannot-be-overwritten-by-stale-intent"
                    | "signer-debit-never-exceeds-consented-fee-terms"
                    | "provider-approved-fee-split-is-durable-and-attributed"
                    | "account-oriented-fees-must-map-to-economic-side-before-terminal-use"
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
                    | "terminal-residual-cannot-double-charge-provider-principal"
                    | "insurance-spend-remains-source-domain-local"
                    | "backing-atoms-cannot-support-claims-from-another-source"
                    | "b-loss-reduces-only-the-originating-source-domain-and-owner-exits"
                    | "liquidation-certificate-cannot-be-healthier-than-full-refresh"
                    | "expired-backing-cannot-create-withdrawable-provider-value"
                    | "expired-backing-cannot-capitalize-and-extract-independent-principal"
                    | "expired-retained-operation-cannot-consume-principal-and-lock-terminal-users"
                    | "lapsed-backing-must-not-lock-resolved-user-exit"
                    | "impaired-domain-prospective-loss-must-have-bounded-terminal-reconciliation"
                    | "vanished-source-claim-must-have-a-bounded-public-unwind"
                    | "expired-source-lien-must-have-bounded-public-reconciliation"
                    | "fractional-source-domains-must-have-a-bounded-public-unwind"
                    | "flat-backed-claim-must-have-bounded-terminal-conversion"
                    | "admitted-live-leg-must-reserve-a-settlement-source-slot"
                    | "expired-provider-lien-retirement-must-preserve-provenance-and-terminal-progress"
                    | "max-source-backed-claim-conversion-must-fit-one-bounded-step"
                    | "max-source-terminal-claim-must-have-a-bounded-close-step"
                    | "max-source-liquidatable-account-must-have-a-bounded-public-reduction"
                    | "old-generation-terminal-capability-cannot-crystallize-replacement-value"
                    | "stale-position-episode-consent-cannot-transfer-or-orphan-value"
                    | "stale-matcher-enable-cannot-revive-revoked-value-authority"
                    | "funded-role-principal-cannot-be-redirected-without-incumbent-consent"
                    | "committed-funding-must-accrue-before-lifecycle-terminalization"
                    | "pending-mark-boundary-must-activate-before-post-boundary-funding"
                    | "stale-shutdown-must-retain-a-bounded-public-progress-path"
                    | "terminal-snapshot-must-use-current-authenticated-oracle-state"
                    | "funded-resolved-adl-winner-has-bounded-public-exit"
                    | "fractional-reset-carry-cannot-block-permissionless-liquidation"
                    | "post-adl-split-cannot-increase-withdrawable-backing-funded-value"
                    | "zero-effective-oi-funded-residue-must-enter-bounded-cleanup"
                    | "partial-adl-recovery-residue-has-permissionless-bounded-cleanup"
                    | "fractional-social-loss-carry-cannot-lock-funded-owner-exit"
                    | "fragmented-recovery-must-have-a-permissionless-pairwise-close-path"
                    | "forfeit-order-cannot-lock-provider-backed-recovery"
                    | "recovered-provider-backing-must-have-withdraw-or-restart-progress"
                    | "permissionless-asset-local-close-cannot-freeze-unrelated-funded-users"
                    | "account-local-expired-close-preserves-unrelated-resolved-exit"
                    | "recovery-escalation-reaches-public-resolved-continuation"
                    | "successful-crank-cannot-consume-zero-delta-price-time"
                    | "recovery-required-transition-must-not-rollback-funded-survivor-progress"
                    | "prospective-loss-must-not-create-backing-in-lapsed-domain"
                    | "asset-local-bankruptcy-cannot-lock-unrelated-backed-claim"
                    | "unsigned-lp-cannot-inherit-preexisting-settlement-cohort"
                    | "fresh-counterparty-must-not-inherit-preexisting-settlement-cohort"
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
            assert_eq!(
                benchmark_invariants.get(&pr),
                Some(&invariant),
                "discovery and benchmark must agree on PR {pr}'s primary invariant"
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

    let nonqualifying_sources = [
        include_str!("inv_079_public_reachability_evidence.rs"),
        include_str!("../cu/inv_005_authority_incarnation_binding.rs"),
        include_str!("../cu/inv_028_source_domain_realizability_cap.rs"),
        include_str!("../cu/inv_051_canonical_adl_effective_quantity.rs"),
        include_str!("../cu/inv_061_deterministic_bounded_liquidation.rs"),
        include_str!("../cu/inv_063_backing_expiry_normalization.rs"),
        include_str!("../cu/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs"),
        include_str!("../cu/inv_071_crank_progress.rs"),
        include_str!("../cu/inv_073_no_permanent_user_lock.rs"),
        include_str!("../cu/inv_077_bounded_work_and_maximum_shape_compute.rs"),
    ];
    let mut classified_prs = std::collections::BTreeSet::new();
    for line in include_str!("../nonqualifying_findings.tsv").lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.splitn(4, '\t').collect();
        assert_eq!(fields.len(), 4, "malformed nonqualifying row: {line}");
        let pr: u16 = fields[0].parse().expect("numeric nonqualifying PR ID");
        assert!(
            classified_prs.insert(pr),
            "duplicate nonqualifying PR row: {pr}"
        );
        assert!(matches!(
            fields[1],
            "current-pin-safe"
                | "bounded-public-exit"
                | "correct-input-progress"
                | "nonextractable"
                | "privileged-self-action"
                | "duplicate"
                | "prerequisite-unreachable"
                | "transient-only"
        ));
        assert!(
            nonqualifying_sources
                .iter()
                .any(|source| source.contains(&format!("fn {}", fields[2]))),
            "nonqualifying claim lacks executable public-route evidence: {}",
            fields[2]
        );
        assert!(
            !fields[3].trim().is_empty(),
            "nonqualifying reason is empty"
        );
        assert_eq!(
            benchmark_evidence.get(&pr),
            Some(&"nonqualifying"),
            "classification must point to a nonqualifying benchmark row"
        );
    }
    let benchmark_nonqualifying: std::collections::BTreeSet<_> = benchmark_evidence
        .iter()
        .filter_map(|(pr, evidence)| (*evidence == "nonqualifying").then_some(*pr))
        .collect();
    assert_eq!(
        classified_prs, benchmark_nonqualifying,
        "every nonqualifying benchmark row needs machine-checked evidence"
    );
}

fn source_defines_test(source: &str, function: &str) -> bool {
    let expected = format!("fn {function}");
    let mut test_attribute = false;

    for line in source.lines() {
        let line = line.trim();
        if line == "#[test]" {
            test_attribute = true;
        } else if line.starts_with("fn ") {
            if test_attribute
                && line
                    .strip_prefix(&expected)
                    .is_some_and(|tail| tail.trim_start().starts_with('('))
            {
                return true;
            }
            test_attribute = false;
        } else if test_attribute && !line.is_empty() && !line.starts_with("#") {
            test_attribute = false;
        }
    }

    false
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublicInstructionRoute {
    tag: u8,
    variant: String,
}

#[derive(Debug)]
struct PublicInstructionCoverageRow<'a> {
    tag: u8,
    variant: &'a str,
    public_route_coverage: &'a str,
    cu_coverage: &'a str,
    omission_reason: &'a str,
}

#[test]
fn v16_public_instruction_coverage_registry_matches_production_roster() {
    let production_roster =
        production_public_instruction_roster(include_str!("../../../src/v16_program.rs"));
    let registry = parse_public_instruction_coverage_registry(include_str!(
        "../public_instruction_coverage.tsv"
    ));

    let registry_roster: Vec<PublicInstructionRoute> = registry
        .iter()
        .map(|row| PublicInstructionRoute {
            tag: row.tag,
            variant: row.variant.to_string(),
        })
        .collect();

    assert_eq!(
        registry_roster, production_roster,
        "public instruction coverage registry must have exactly one row per production \
         ProgInstruction tag/variant"
    );
}

fn production_public_instruction_roster(source: &str) -> Vec<PublicInstructionRoute> {
    let variants = instruction_enum_variants(source);
    let tags = instruction_decode_tags(source);
    assert_eq!(
        variants.len(),
        tags.len(),
        "every Instruction enum variant must have a decode tag"
    );

    let mut roster = Vec::with_capacity(variants.len());
    for variant in variants {
        let tag = tags
            .get(&variant)
            .unwrap_or_else(|| panic!("Instruction::{variant} lacks a decode tag"));
        roster.push(PublicInstructionRoute { tag: *tag, variant });
    }
    roster.sort_by_key(|route| route.tag);
    roster
}

fn instruction_enum_variants(source: &str) -> Vec<String> {
    let block = braced_block_after(source, "pub enum Instruction");
    let mut variants = Vec::new();
    let mut depth = 0i32;

    for raw_line in block.lines() {
        let line = raw_line.split("//").next().unwrap_or("").trim();
        if depth == 0 {
            let name: String = line
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                .collect();
            if name
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_alphabetic())
            {
                variants.push(name);
            }
        }
        depth += brace_delta(line);
        assert!(depth >= 0, "Instruction enum parser underflowed at {line}");
    }
    assert_eq!(depth, 0, "Instruction enum parser ended inside a variant");
    variants
}

fn instruction_decode_tags(source: &str) -> std::collections::BTreeMap<String, u8> {
    let block = braced_block_after(source, "pub fn decode(input: &[u8])");
    let mut tags = std::collections::BTreeMap::new();
    let mut pending_tag = None;

    for raw_line in block.lines() {
        let line = raw_line.split("//").next().unwrap_or("").trim();
        if let Some((tag, tail)) = parse_decode_tag_arm(line) {
            if let Some(variant) = self_variant_name(tail) {
                assert!(
                    tags.insert(variant.clone(), tag).is_none(),
                    "duplicate decode tag for Instruction::{variant}"
                );
                pending_tag = None;
            } else {
                pending_tag = Some(tag);
            }
            continue;
        }

        if let Some(tag) = pending_tag {
            if let Some(variant) = self_variant_name(line) {
                assert!(
                    tags.insert(variant.clone(), tag).is_none(),
                    "duplicate decode tag for Instruction::{variant}"
                );
                pending_tag = None;
            }
        }
    }

    tags
}

fn braced_block_after<'a>(source: &'a str, marker: &str) -> &'a str {
    let start = source
        .find(marker)
        .unwrap_or_else(|| panic!("missing source marker {marker}"));
    let open = start
        + source[start..]
            .find('{')
            .unwrap_or_else(|| panic!("missing opening brace after {marker}"));
    let mut depth = 0i32;
    for (offset, ch) in source[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[(open + 1)..(open + offset)];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated source block after {marker}");
}

fn brace_delta(line: &str) -> i32 {
    line.chars().filter(|ch| *ch == '{').count() as i32
        - line.chars().filter(|ch| *ch == '}').count() as i32
}

fn parse_decode_tag_arm(line: &str) -> Option<(u8, &str)> {
    let (raw_tag, tail) = line.split_once("=>")?;
    let raw_tag = raw_tag.trim();
    if raw_tag.chars().all(|ch| ch.is_ascii_digit()) {
        Some((raw_tag.parse().expect("decode tag fits in u8"), tail.trim()))
    } else {
        None
    }
}

fn self_variant_name(line: &str) -> Option<String> {
    let tail = line.split_once("Self::")?.1;
    let name: String = tail
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

fn parse_public_instruction_coverage_registry(tsv: &str) -> Vec<PublicInstructionCoverageRow<'_>> {
    const HEADER: &str = "tag\tvariant\tpublic_route_coverage\tcu_coverage\tomission_reason";
    let mut rows = Vec::new();
    let mut seen_tags = std::collections::BTreeSet::new();
    let mut seen_variants = std::collections::BTreeSet::new();
    let mut prior_tag = None;
    let mut saw_header = false;

    for (line_index, line) in tsv.lines().enumerate() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if !saw_header {
            assert_eq!(line, HEADER, "public instruction registry header changed");
            saw_header = true;
            continue;
        }

        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(
            fields.len(),
            5,
            "malformed public instruction registry row {}: {line}",
            line_index + 1
        );
        let tag: u8 = fields[0]
            .parse()
            .unwrap_or_else(|_| panic!("non-numeric instruction tag on row {}", line_index + 1));
        let row = PublicInstructionCoverageRow {
            tag,
            variant: fields[1],
            public_route_coverage: fields[2],
            cu_coverage: fields[3],
            omission_reason: fields[4],
        };

        if let Some(prior) = prior_tag {
            assert!(
                row.tag > prior,
                "public instruction registry rows must be sorted by tag"
            );
        }
        prior_tag = Some(row.tag);
        assert!(
            seen_tags.insert(row.tag),
            "duplicate public instruction tag {}",
            row.tag
        );
        assert!(
            seen_variants.insert(row.variant),
            "duplicate public instruction variant {}",
            row.variant
        );
        assert!(
            row.variant
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_uppercase()),
            "variant names must be Rust enum variants: {}",
            row.variant
        );

        let public_omitted = validate_public_instruction_coverage_cell(
            row.public_route_coverage,
            "public_route_coverage",
            row.variant,
        );
        let cu_omitted =
            validate_public_instruction_coverage_cell(row.cu_coverage, "cu_coverage", row.variant);
        if public_omitted || cu_omitted {
            assert!(
                !matches!(row.omission_reason, "" | "-"),
                "{} omits coverage but lacks an explicit omission reason",
                row.variant
            );
        } else {
            assert_eq!(
                row.omission_reason, "-",
                "{} has no omitted coverage; keep omission_reason as '-'",
                row.variant
            );
        }
        rows.push(row);
    }

    assert!(!rows.is_empty(), "public instruction registry is empty");
    rows
}

fn validate_public_instruction_coverage_cell(cell: &str, column: &str, variant: &str) -> bool {
    if cell == "OMITTED" {
        return true;
    }

    for evidence in cell.split(';') {
        let (kind, rest) = evidence
            .split_once(':')
            .unwrap_or_else(|| panic!("{variant} {column} evidence lacks prefix: {evidence}"));
        assert!(
            matches!(kind, "OWNED" | "SHARED" | "CROSS"),
            "{variant} {column} has unknown evidence prefix {kind}"
        );
        let (path, function) = rest
            .split_once('#')
            .unwrap_or_else(|| panic!("{variant} {column} evidence lacks function: {evidence}"));
        assert!(
            path.starts_with("tests/invariants/") && path.ends_with(".rs"),
            "{variant} {column} evidence must stay under tests/invariants: {path}"
        );
        assert!(
            function.starts_with("v16_"),
            "{variant} {column} evidence must name a v16 test function: {function}"
        );
        let full_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
        let source = std::fs::read_to_string(&full_path)
            .unwrap_or_else(|error| panic!("read evidence source {path}: {error}"));
        assert!(
            source_defines_function(&source, function),
            "{variant} {column} evidence {path} does not define fn {function}"
        );
    }

    false
}

fn source_defines_function(source: &str, function: &str) -> bool {
    let expected = format!("fn {function}");
    source.lines().any(|line| {
        line.trim_start()
            .strip_prefix(&expected)
            .is_some_and(|tail| tail.trim_start().starts_with('('))
    })
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SpecialMethodRequirement {
    invariant: String,
    method: String,
}

#[test]
fn v16_special_verification_method_registry_matches_charter() {
    let required = charter_special_method_requirements(include_str!("../../../INVARIANTS.md"));
    let indexed = parse_special_method_registry(include_str!("../special_method_coverage.tsv"));
    let indexed_requirements = indexed
        .iter()
        .map(|row| SpecialMethodRequirement {
            invariant: row.invariant.to_string(),
            method: row.method.to_string(),
        })
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(
        indexed_requirements, required,
        "every charter-required M/R/C method must have exactly one explicit registry row"
    );
    assert_eq!(
        indexed.len(),
        required.len(),
        "duplicate method registry row"
    );
}

fn charter_special_method_requirements(
    charter: &str,
) -> std::collections::BTreeSet<SpecialMethodRequirement> {
    let mut current_invariant = None;
    let mut required = std::collections::BTreeSet::new();

    for line in charter.lines() {
        if let Some(rest) = line.strip_prefix("### INV-") {
            let digits = rest
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect::<String>();
            assert_eq!(digits.len(), 3, "malformed invariant heading: {line}");
            current_invariant = Some(format!("INV-{digits}"));
            continue;
        }
        let Some(methods) = line.strip_prefix("**Verification:** ") else {
            continue;
        };
        let invariant = current_invariant
            .as_ref()
            .unwrap_or_else(|| panic!("verification methods precede an invariant heading"));
        for method in methods.split(',').map(str::trim) {
            if matches!(method, "M" | "R" | "C") {
                assert!(
                    required.insert(SpecialMethodRequirement {
                        invariant: invariant.clone(),
                        method: method.to_string(),
                    }),
                    "duplicate {method} requirement for {invariant}"
                );
            }
        }
    }

    assert_eq!(required.iter().filter(|row| row.method == "M").count(), 32);
    assert_eq!(required.iter().filter(|row| row.method == "R").count(), 22);
    assert_eq!(required.iter().filter(|row| row.method == "C").count(), 2);
    required
}

#[derive(Debug)]
struct SpecialMethodCoverageRow<'a> {
    invariant: &'a str,
    method: &'a str,
}

fn parse_special_method_registry(tsv: &str) -> Vec<SpecialMethodCoverageRow<'_>> {
    const HEADER: &str = "invariant\tmethod\tstatus\tevidence\tremaining_gap";
    let mut rows = Vec::new();
    let mut saw_header = false;

    for (line_index, line) in tsv.lines().enumerate() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if !saw_header {
            assert_eq!(line, HEADER, "special method registry header changed");
            saw_header = true;
            continue;
        }

        let fields = line.split('\t').collect::<Vec<_>>();
        assert_eq!(
            fields.len(),
            5,
            "malformed special method registry row {}: {line}",
            line_index + 1
        );
        let (invariant, method, status, evidence, remaining_gap) =
            (fields[0], fields[1], fields[2], fields[3], fields[4]);
        assert!(
            invariant.len() == 7
                && invariant.starts_with("INV-")
                && invariant[4..].chars().all(|ch| ch.is_ascii_digit()),
            "invalid invariant id on row {}: {invariant}",
            line_index + 1
        );
        assert!(
            matches!(method, "M" | "R" | "C"),
            "invalid method on row {}: {method}",
            line_index + 1
        );
        assert!(
            !remaining_gap.trim().is_empty(),
            "row {} must name the remaining completion gap",
            line_index + 1
        );

        match status {
            "PARTIAL" => {
                let (path, function) = evidence.split_once('#').unwrap_or_else(|| {
                    panic!(
                        "row {} PARTIAL evidence lacks path#function",
                        line_index + 1
                    )
                });
                assert!(
                    path.starts_with("tests/invariants/") && path.ends_with(".rs"),
                    "row {} evidence is not invariant-owned: {path}",
                    line_index + 1
                );
                let full_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
                let source = std::fs::read_to_string(&full_path)
                    .unwrap_or_else(|error| panic!("read method evidence {path}: {error}"));
                assert!(
                    source_defines_function(&source, function),
                    "row {} evidence {path} does not define fn {function}",
                    line_index + 1
                );
            }
            "OMITTED" => assert_eq!(
                evidence,
                "-",
                "row {} OMITTED method cannot claim evidence",
                line_index + 1
            ),
            _ => panic!(
                "row {} status must be PARTIAL or OMITTED, got {status}",
                line_index + 1
            ),
        }

        rows.push(SpecialMethodCoverageRow { invariant, method });
    }

    assert!(saw_header, "special method registry header is missing");
    rows
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
    assert_eq!(
        invariant_ids(include_str!("../README.md"), "| AUDIT-"),
        expected,
        "the exhaustiveness audit must classify every normative invariant exactly once"
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
