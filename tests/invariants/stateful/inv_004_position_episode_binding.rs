//! INV-004 - Position episode binding.
//!
//! Normative obligation: Reduction, close, conversion, claim, and forfeit consent applies only to
//! the exact economic position or recovery episode that existed when the owner signed it.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_position_episode_matrix_discovers_stale_value_transfers` creates each old episode,
//! retains owner-signed consent, completes the episode through public transitions, then creates a
//! fresh episode at the same market/asset/portfolio IDs. A rebalance violation requires exact
//! victim-to-counterparty terminal payout transfer. A Recovery violation requires deletion of a
//! fresh backed gain, the same amount left unowned in the vault, and an atomic persistent
//! `CloseSlab` failure. No program-owned bytes are injected.
//!
//! Guarantee boundary: these are vulnerable-pin counterexamples. Certification requires the fixed
//! pin to reject stale consent byte-for-byte while current-episode consent remains live.

use super::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_004_position_episode_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_position_episode_matrix_discovers_stale_value_transfers(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_position_episode_replays(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), PositionEpisodeKind::ALL.len());
        for (kind, discovery) in PositionEpisodeKind::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.kind, kind);
            prop_assert!(
                discovery.is_violation(),
                "stale position-episode consent lacked substantive impact: {:?}",
                discovery
            );
        }
    }
}
