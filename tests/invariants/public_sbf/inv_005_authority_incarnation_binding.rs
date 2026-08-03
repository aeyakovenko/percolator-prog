//! INV-005 - Authority incarnation binding.
//!
//! Normative obligation: Authority consent cannot revive after rotation, disablement, or A-to-B-to-A reuse.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr251_delayed_admin_handoff_revives_withdrawal_authority`, `v16_program_pr345_pr346_authority_aba_replays_drain_new_reserves`, `v16_program_pr353_prior_authority_resolve_crystallizes_victim_loss`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_pr251_delayed_admin_handoff_revives_withdrawal_authority() {
    let reproduction = reproduce_delayed_asset_authority_revival([0x51; 32])
        .unwrap_or_else(|error| panic!("PR 251 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::DelayedAssetAuthorityRevival
    );
    assert_eq!(reproduction.funded_reserve, 50_000);
    assert_eq!(reproduction.provider_loss, 50_000);
    assert_eq!(reproduction.attacker_extraction, 50_000);
    assert_eq!(reproduction.reserve_after, 0);
    assert!(reproduction.handoff_cu < 1_400_000);
    assert!(reproduction.withdrawal_cu < 1_400_000);
}

#[test]
fn v16_program_pr345_pr346_authority_aba_replays_drain_new_reserves() {
    for path in [
        AuthorityHandoffAbaPath::Market,
        AuthorityHandoffAbaPath::AssetInsuranceOperator,
    ] {
        let reproduction = reproduce_authority_handoff_aba_replay([0x45; 32], path)
            .unwrap_or_else(|error| panic!("PR 345/346 {path:?} no longer reproduces: {error}"));
        assert_eq!(
            reproduction.blocker,
            KnownBlocker::AuthorityHandoffAbaReplay
        );
        assert_eq!(reproduction.path, path);
        assert!(reproduction.control_withdrawal_blocked);
        assert_eq!(reproduction.reserve_before, 50_000);
        assert_eq!(reproduction.reserve_after, 0);
        assert_eq!(reproduction.attacker_extraction, 50_000);
        assert!(reproduction.replay_cu < 1_400_000);
        assert!(reproduction.withdrawal_cu < 1_400_000);
    }
}

#[test]
fn v16_program_pr353_prior_authority_resolve_crystallizes_victim_loss() {
    let reproduction = reproduce_resolve_authority_incarnation_replay([0x53; 32])
        .unwrap_or_else(|error| panic!("PR 353 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::ResolveAuthorityIncarnationReplay
    );
    assert_eq!(reproduction.control_price, 100);
    assert_eq!(reproduction.replay_price, 110);
    assert_eq!(reproduction.victim_loss, 100_000);
    assert_eq!(reproduction.winner_gain, 100_000);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.max_crank_cu < 1_400_000);
}
