//! INV-067 - Terminal payout completeness and exact-once settlement.
//!
//! Normative obligation: Each valid claim is paid, forfeited, or receipted exactly once without silent loss.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_reported_route_matrix_preserves_terminal_value_partition` compares terminal worlds
//! with and without a one-atom round trip through both reported-price routes. It drains every
//! public close/claim continuation to quiescence and requires unchanged victim payout while the
//! sole residual equals the coalition's one-atom rounding loss. Direct impact tests remain below. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: this certifies one source-haircut composition across all deployed trade
//! routes. It does not replace the broader claim-episode and bounded-reachability work in the
//! invariant roadmap.

use super::*;
use crate::support::v16_svm::PublicTerminalClassification;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_067_terminal_dust_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_reported_route_matrix_preserves_terminal_value_partition(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_terminal_dust_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), ProspectiveAccrualRoute::ALL.len());
        for (expected, discovery) in ProspectiveAccrualRoute::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.route, expected);
            prop_assert_eq!(discovery.attacker_loss, 1);
            prop_assert_eq!(discovery.victim_loss, 0);
            prop_assert_eq!(discovery.control_vault_remaining, 0);
            prop_assert_eq!(discovery.vault_remaining, discovery.attacker_loss);
            prop_assert_eq!(discovery.control_supply, discovery.dust_supply);
            prop_assert_eq!(
                discovery.terminal_classification,
                PublicTerminalClassification::BoundedExit
            );
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.route)
            .collect();
        prop_assert!(violations.is_empty(), "terminal claim erasure returned: {violations:?}");
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
    fn v16_program_terminal_source_haircut_preserves_victim_claim_fuzz(
        (seed, route) in terminal_dust_payout_protection_strategy()
    ) {
        let result = verify_terminal_dust_payout_protection(seed, route);
        prop_assert!(
            result.is_ok(),
            "terminal source-haircut protection failed for {:?}, seed {:?}: {}",
            route,
            seed,
            result.unwrap_err()
        );
    }
}
