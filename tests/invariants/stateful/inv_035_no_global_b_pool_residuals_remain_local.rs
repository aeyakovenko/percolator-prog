//! INV-035 - No global B pool; residuals remain local.
//!
//! Normative obligation: Bankruptcy residuals stay in the exact asset and opposing-side domain that created them.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_two_asset_bankruptcy_preserves_domain_local_settlement_and_exit` creates claims in
//! two source domains, books bankruptcy B in one asset, and independently recomputes both claim
//! deltas. It requires the unrelated claim to remain unchanged, the affected claim to absorb the
//! exact B loss, bounded public reductions to flatten the affected leg, and principal withdrawal
//! with conserved SPL supply.
//!
//! Guarantee boundary: this randomized public-route oracle certifies the exercised two-domain
//! topology. The deterministic TDD route lives in the public-SBF INV-035 file, while engine Kani
//! proves the domain-first partition kernel.

use super::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_035_cross_domain_b_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_two_asset_bankruptcy_preserves_domain_local_settlement_and_exit(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_cross_domain_b_violation(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(
            discovery.preserves_domain_locality_and_exit(),
            "domain-local B settlement or bounded exit failed: {:?}",
            discovery
        );
    }
}
