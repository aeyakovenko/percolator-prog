//! INV-001 - Market incarnation binding.
//!
//! This finding-blind property varies the complete retained market-operation matrix over random
//! public fixtures. Every route closes the old market, publicly funds its persistent tombstone,
//! attempts same-address initialization, and lands the old signed transaction. Both attempts must
//! reject with exact economic rollback. INV-007 supplies the deterministic trace and fresh-address
//! liveness control.
//! Secondary coverage: INV-014. Maintenance- and liquidation-policy requests are members of the
//! same retained-operation matrix and cannot cross terminal market retirement.

use super::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_001_market_incarnation_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_market_incarnation_operation_matrix_rejects_address_reuse(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_market_incarnation_replays(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), MarketIntentKind::ALL.len());
        for (expected, discovery) in MarketIntentKind::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.kind, expected);
            prop_assert!(
                discovery.certifies_no_reuse(),
                "{expected:?} did not certify whole-market no-reuse: {discovery:?}"
            );
        }
    }
}
