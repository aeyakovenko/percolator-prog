//! INV-010 - Out-of-order safety.
//!
//! Normative obligation: every landing order either rejects atomically or remains inside every
//! affected signer’s latest authorization.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_matcher_mutation_order_rejects_revoked_capability` retains an LP-signed matcher
//! enable, lands the LP's later revoke, and proves both a CPI fill and replay of the earlier enable
//! reject with exact rollback. It then signs against the current sequence and executes a complete
//! CPI open/close and SPL withdrawal path, proving the guard does not disable fresh LP consent.
//! The matcher sequence is read from the real portfolio account before and after each transition.
//!
//! Guarantee boundary: this fixed-pin regression covers the portfolio-scoped matcher capability.
//! Other retained policy domains are owned by INV-014 and require their own scope-local sequences.

use super::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_010_matcher_order_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_matcher_mutation_order_rejects_revoked_capability(
        seed in any::<[u8; 32]>()
    ) {
        let protection = verify_matcher_mutation_order_safety(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(
            protection.satisfies_invariant(),
            "matcher supersession protection failed: {:?}",
            protection
        );
    }
}
