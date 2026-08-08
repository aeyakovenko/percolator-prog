//! INV-004 - Position episode binding.
//!
//! Normative obligation: retained reduction and recovery-forfeit consent binds
//! the exact economic position/recovery episode that existed when the owner
//! signed. Closing or forfeiting an old episode and opening a replacement at
//! the same portfolio/asset must not let the old signed request touch the new
//! exposure.
//!
//! Evidence in this file (I/F): this deterministic LiteSVM wrapper witness runs
//! the shared public-route position-episode matrix. For rebalance-reduce and
//! recovery-forfeit, it requires stale retained consent to reject with exact
//! market, portfolio, vault, and SPL-supply rollback; it also requires freshly
//! signed current consent to land and change exposure so the guard is not a
//! blanket risk-reduction DoS.
//!
//! Guarantee boundary: this is a non-random whole-route witness for the same
//! two retained position-consent routes enforced by the stateful generator.

#[test]
fn v16_program_position_episode_matrix_rejects_stale_consent_fixed_case() {
    let discoveries =
        crate::support::invariant_discovery::discover_position_episode_replays([0x04; 32])
            .expect("position-episode replay discovery");
    assert_eq!(
        discoveries.len(),
        crate::support::invariant_discovery::PositionEpisodeKind::ALL.len()
    );
    for discovery in discoveries {
        assert!(
            discovery.satisfies_invariant(),
            "position-episode binding failed: {discovery:?}"
        );
    }
}
