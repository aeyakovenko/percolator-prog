//! INV-010 - Out-of-order safety.
//!
//! Normative obligation: every landing order either rejects atomically or remains inside every
//! affected signer’s latest authorization.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_matcher_mutation_order_discovers_revoked_lp_value_transfer` retains an LP-signed
//! matcher enable, lands the LP’s later revoke, proves CPI fills are then rejected, and finally
//! replays the earlier enable. A violation requires an attacker-controlled CPI fill to become live
//! again and an honest oracle move to transfer the LP’s exact terminal SPL loss to the attacker.
//! Account bytes are captured around the rejected control fill to prove SVM rollback.
//!
//! Guarantee boundary: this is a vulnerable-pin counterexample. A fixed implementation must reject
//! the stale mutation while preserving a fresh post-revoke enable path.

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
    fn v16_program_matcher_mutation_order_discovers_revoked_lp_value_transfer(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_matcher_mutation_order_violation(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(
            discovery.is_violation(),
            "stale matcher mutation lacked independent LP loss: {:?}",
            discovery
        );
    }
}
