//! INV-005 - Authority incarnation binding.
//!
//! These deterministic public-SBF traces retain authority operations across A-to-B-to-A
//! rotations. They require stale consent to reject with exact rollback, then prove that current
//! authority and incumbent-owner exits remain available. The stateful INV-005 suite runs the same
//! finding-blind oracles over generated seeds; Kani exhausts the scalar epoch predicates.

use crate::support::invariant_discovery::AuthorityIntentKind;

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
