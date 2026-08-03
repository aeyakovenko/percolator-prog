//! INV-067 - Terminal payout completeness and exact-once settlement.
//!
//! Normative obligation: Each valid claim is paid, forfeited, or receipted exactly once without silent loss.
//!
//! Evidence in this file (F over public I routes): `v16_program_pr283_terminal_dust_payout_erasure_fuzz`. These tests exercise the deployed public
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
                "proptest-regressions/v16_program_stateful_fuzz.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_pr283_terminal_dust_payout_erasure_fuzz(
        (seed, route) in terminal_dust_payout_erasure_strategy()
    ) {
        let result = reproduce_terminal_dust_payout_erasure(seed, route);
        prop_assert!(
            result.is_ok(),
            "PR 283 {:?} no longer reproduces for seed {:?}: {}",
            route,
            seed,
            result.unwrap_err()
        );
    }
}
