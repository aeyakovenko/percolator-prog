//! INV-020 - Authenticated clock, slot, and oracle provenance.
//!
//! Normative obligation: Time and oracle observations are authenticated, coherent, and cannot be caller-rewound.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_composite_timestamp_coherence_discovers_cross_epoch_liquidation` keeps a two-leg
//! cross-rate mathematically constant, advances only one leg, and requires the public wrapper to
//! reject the incoherent observation before it can certify liquidation or extract a reward.
//! `v16_program_hybrid_terminal_snapshot_discovers_expired_leg_settlement` builds a two-leg Hybrid
//! feed at the exact freshness boundary, advances one expired leg through a valid external report,
//! and compares terminal payouts with and without public ingestion. The oracle requires an exact
//! victim loss and counterparty gain under the stale administrative snapshot.
//! Direct impact tests remain below. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_020_composite_time_coherence_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_composite_timestamp_coherence_discovers_cross_epoch_liquidation(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_composite_time_coherence_violation(seed)
            .map_err(TestCaseError::fail)?;
        eprintln!("independent composite-time discovery: {discovery:?}");
        prop_assert!(
            discovery.is_violation(),
            "vulnerable-pin composite timestamp handling changed: {:?}",
            discovery
        );
    }

    #[test]
    fn v16_program_hybrid_terminal_snapshot_discovers_expired_leg_settlement(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_hybrid_terminal_snapshot_violation(seed)
            .map_err(TestCaseError::fail)?;
        eprintln!("independent Hybrid terminal snapshot discovery: {discovery:?}");
        prop_assert!(
            discovery.is_violation(),
            "vulnerable-pin Hybrid terminal snapshot changed: {discovery:?}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/v16_program_stateful_fuzz.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_pr331_composite_oracle_time_skew_fuzz(
        seed in composite_time_skew_seed_strategy()
    ) {
        let result = reproduce_composite_oracle_time_skew(seed);
        prop_assert!(
            result.is_ok(),
            "PR 331 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }
}
