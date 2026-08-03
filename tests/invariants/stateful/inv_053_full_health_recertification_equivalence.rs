//! INV-053 - Full-health recertification equivalence.
//!
//! Normative obligation: Fast or incremental certification is never more favorable than full recomputation.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_full_refresh_equivalence_discovers_omitted_rescue_liquidation` builds two
//! identical multi-leg portfolios through public instructions. The control refreshes every
//! economically relevant leg; the adversarial ordering omits the later rescue observation.
//! A violation requires the omitted route to certify and execute liquidation with an insurance
//! transfer while the fully refreshed route preserves the same position with zero deficit.
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
                "proptest-regressions/inv_053_full_refresh_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_full_refresh_equivalence_discovers_omitted_rescue_liquidation(
        seed in any::<[u8; 32]>()
    ) {
        let result = discover_full_refresh_omission_violation(seed);
        prop_assert!(
            result.is_ok(),
            "full-refresh discovery failed for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
        let discovery = result.unwrap();
        prop_assert!(
            discovery.is_violation(),
            "partial refresh did not create a false liquidation relative to full refresh: {:?}",
            discovery
        );
    }
}
