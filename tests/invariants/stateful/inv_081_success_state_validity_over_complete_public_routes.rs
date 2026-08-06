//! INV-081 - Success-state validity over complete public routes.
//!
//! Normative obligation: Every successful wrapper-plus-engine route preserves global invariants and authorized deltas.
//!
//! Evidence in this file (F over public I routes): `v16_program_stateful_public_interface_fuzz`
//! generates deposits, withdrawals, all four trade routes, retained transactions, oracle changes,
//! cranks, fee synchronization, and hostile account substitution. After every public transition it
//! independently rejects undecodable or hidden legs, duplicate same-asset legs, stale generation
//! bindings, source-lien classification mismatches, stored-position/OI drift, and net-position
//! drift. Successful non-token routes must preserve every tracked SPL account byte-for-byte;
//! deposits and withdrawals may mutate only their canonical source/destination and vault, with
//! exact authorized deltas. Every rejected route must roll back all tracked economic state.
//!
//! Secondary coverage: INV-024, INV-031, INV-034, INV-048, INV-049, INV-051, and INV-080. The OI
//! oracle always checks live long/short equality, effective OI cannot exceed the complete raw-leg
//! census, and any Live Active/DrainOnly side with zero effective OI plus surviving non-obligation
//! basis must be `ResetPending`. Exact raw-leg equality is required only when no stale leg,
//! pending obligation, or protocol-attributed unilateral reduction makes raw basis intentionally
//! larger than pooled effective OI.
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
    fn v16_program_stateful_public_interface_fuzz(
        scenario in scenario_strategy(env_usize("PERCOLATOR_FUZZ_ACTIONS", 12))
    ) {
        let serialized = serde_json::to_string_pretty(&scenario).unwrap();
        let result = run_scenario(&scenario);
        prop_assert!(result.is_ok(), "stateful public-interface scenario failed: {}\n{}",
            result.unwrap_err(), serialized);
    }
}
