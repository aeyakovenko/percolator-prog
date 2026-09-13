//! INV-020 - Authenticated clock, slot, and oracle provenance.
//!
//! Normative obligation: Time and oracle observations are authenticated, coherent, and cannot be caller-rewound.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_composite_timestamp_coherence_rejects_cross_epoch_liquidation` starts from a
//! constant two-leg cross-rate and covers numerator-only, denominator-only, and two-fresh-
//! but-different-epoch reports. Each rejects as OracleStale with exact rollback. A coherent
//! control then changes the cross-rate by 20,000/40,000/60,000, preserving healthy positions
//! and OI, and leaves a complete owner exit and input-priced withdrawal.
//! Its live current-evidence control varies the authenticated slot backlog from 68 to
//! 124 slots. Partial cranks must advance accrual without rewriting the funded account;
//! its initially current certificate must become stale. Completion requires current
//! epochs/bitmap, changed account bytes, exact input-priced equity and margin, and both
//! authenticated component prices/timestamps. A zero deficit alone is
//! not a current-health witness. This tightens the partial-to-current control cell for
//! row 426; it does not close row 426 or add authority/reward evidence for rows 416/422.
//! `v16_program_hybrid_terminal_snapshot_requires_coherent_leg_epochs` rejects mixed-time initial
//! configuration with exact rollback and prevents a mixed-time crank hint from changing oracle or
//! user value state. A coherent control reaches the current cross-rate, resolves, and pays both
//! users exactly from the authenticated terminal mark.
//! Direct impact tests remain below. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: this finite three-word skew matrix certifies the current public composite
//! route. It does not prove every oracle provider implementation or unbounded feed schedule.

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
    fn v16_program_composite_timestamp_coherence_rejects_cross_epoch_liquidation(
        seed in any::<[u8; 32]>()
    ) {
        let evidence = verify_composite_time_coherence(seed)
            .map_err(TestCaseError::fail)?;
        eprintln!("independent composite-time evidence: {evidence:?}");
        prop_assert!(
            evidence.is_protected(),
            "composite timestamp protection failed: {:?}",
            evidence
        );
    }

    #[test]
    fn v16_program_hybrid_terminal_snapshot_requires_coherent_leg_epochs(
        seed in any::<[u8; 32]>()
    ) {
        let evidence = verify_hybrid_terminal_time_coherence(seed)
            .map_err(TestCaseError::fail)?;
        eprintln!("independent Hybrid terminal coherence evidence: {evidence:?}");
        prop_assert!(
            evidence.is_protected(),
            "Hybrid terminal coherence protection failed: {evidence:?}"
        );
    }
}
