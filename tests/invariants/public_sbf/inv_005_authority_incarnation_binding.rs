//! INV-005 - Authority incarnation binding.
//!
//! These deterministic public-SBF traces retain authority operations across A-to-B-to-A
//! rotations. They require stale consent to reject with exact rollback, then prove that current
//! authority and incumbent-owner exits remain available. The stateful INV-005 suite runs the same
//! finding-blind oracles over generated seeds; Kani exhausts the scalar epoch predicates.

use crate::support::invariant_discovery::AuthorityIntentKind;

#[test]
fn v16_row416_discovery_metadata_requires_more_than_epoch_or_direct_role_evidence() {
    // These older generators/oracles lack funded oracle succession followed by
    // current-authority economic effects. Row 416's current discovery mapping
    // must stay on the stronger funded-oracle evidence, not on either bounded
    // epoch/direct-role witness alone.
    let generators = [
        "v16_program_authority_incarnation_operation_matrix_rejects_aba_replays",
        "v16_program_funded_role_matrix_preserves_incumbent_principal",
    ];
    let oracles = [
        "stale-intent-must-reject-and-roll-back-exactly",
        "funded-role-principal-cannot-be-redirected-without-incumbent-consent",
    ];
    let overclaims_row416 = |generator: &str, oracle: &str, benchmark_prs: &str| {
        benchmark_prs
            .split(',')
            .any(|pr| pr.parse::<u16>().expect("numeric benchmark PR") == 416)
            && (generators.contains(&generator) || oracles.contains(&oracle))
    };

    for line in include_str!("../independent_discoveries.tsv")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
    {
        let fields = line.splitn(5, '\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 5, "malformed discovery row: {line}");
        assert!(
            !overclaims_row416(fields[2], fields[3], fields[4]),
            "row 416 requires funded succession and an independent current-authority \
             economic oracle; existing epoch/direct-role evidence is insufficient: {line}"
        );
    }

    // Exercise the guard against relabeling either column alone; changing just
    // the generator or just the oracle must not conceal reuse of the other
    // bounded witness.
    for (generator, oracle) in generators.into_iter().zip(oracles) {
        for prs in ["416", "375,416", "416,375", "375,416,353"] {
            assert!(overclaims_row416(generator, oracle, prs));
            assert!(overclaims_row416("new-generator", oracle, prs));
            assert!(overclaims_row416(generator, "new-oracle", prs));
        }
        assert!(!overclaims_row416(generator, oracle, "251,345,346,353,375"));
        assert!(!overclaims_row416(generator, oracle, "1416"));
    }
    // Passing this exclusion is not certification; generic evidence gates still apply.
    assert!(!overclaims_row416("new-generator", "new-oracle", "416"));
}

#[test]
fn v16_program_authority_incarnation_matrix_rejects_stale_consent() {
    let discoveries =
        crate::support::invariant_discovery::discover_authority_incarnation_replays([0x51; 32])
            .unwrap_or_else(|error| panic!("authority-incarnation matrix failed: {error}"));
    assert_eq!(
        discoveries.len(),
        crate::support::invariant_discovery::AuthorityIntentKind::ALL.len()
    );
    for discovery in &discoveries {
        assert!(
            discovery.certifies_epoch_rejection(),
            "stale authority consent did not reject exactly: {discovery:?}"
        );
    }

    // Fixed-pin holdout certification remains separate from the finding-blind generator. Asset
    // authority rows require every configured asset role rather than one representative handoff.
    let asset_handoffs = [
        AuthorityIntentKind::AssetAdminHandoff,
        AuthorityIntentKind::InsuranceAuthorityHandoff,
        AuthorityIntentKind::InsuranceOperatorHandoff,
        AuthorityIntentKind::BackingAuthorityHandoff,
        AuthorityIntentKind::OracleAuthorityHandoff,
    ];
    let certifications: &[(u16, &[AuthorityIntentKind])] = &[
        (251, &asset_handoffs),
        (345, &[AuthorityIntentKind::MarketAuthorityHandoff]),
        (346, &asset_handoffs),
        (353, &[AuthorityIntentKind::ResolveMarket]),
    ];
    for (pr, kinds) in certifications {
        for kind in *kinds {
            let evidence = discoveries
                .iter()
                .find(|discovery| discovery.kind == *kind)
                .unwrap_or_else(|| panic!("PR {pr}: missing {kind:?} authority evidence"));
            assert!(
                evidence.certifies_epoch_rejection(),
                "PR {pr}: {kind:?} lacks stale rollback or fresh liveness",
            );
        }
    }
}

#[test]
fn v16_program_stale_resolve_rejects_before_fresh_terminal_exit() {
    let discovery =
        crate::support::invariant_discovery::discover_authority_resolve_terminal_replay([0x53; 32])
            .unwrap_or_else(|error| panic!("funded authority-resolve trace failed: {error}"));
    assert!(
        discovery.certifies_epoch_and_bounded_exit(),
        "stale resolve was not rejected before a fresh bounded exit: {discovery:?}"
    );
    assert_eq!(discovery.victim_loss, 100_000);
    assert_eq!(discovery.winner_gain, discovery.victim_loss);
}

#[test]
fn v16_program_stale_backing_handoff_rejects_before_incumbent_exit() {
    let discovery =
        crate::support::invariant_discovery::discover_authority_funded_handoff_replay([0x45; 32])
            .unwrap_or_else(|error| panic!("funded authority-handoff trace failed: {error}"));
    assert!(
        discovery.certifies_epoch_and_bounded_exit(),
        "stale handoff was not rejected before the incumbent exit: {discovery:?}"
    );
    assert_eq!(discovery.provider_source_debit, 500);
    assert_eq!(discovery.replacement_gain, 0);
    assert_eq!(discovery.provider_exit_gain, 500);
}

#[test]
fn v16_program_funded_role_matrix_preserves_incumbent_principal() {
    let discoveries =
        crate::support::invariant_discovery::discover_funded_role_seizures([0x75; 32])
            .unwrap_or_else(|error| panic!("funded-role matrix failed: {error}"));
    assert_eq!(
        discoveries.len(),
        crate::support::invariant_discovery::FundedRoleKind::ALL.len()
    );
    for discovery in discoveries {
        assert!(
            discovery.certifies_funded_role_protection(),
            "funded role did not preserve incumbent principal: {discovery:?}"
        );
    }
}
