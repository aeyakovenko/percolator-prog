//! INV-027 - Protected principal seniority.
//!
//! Normative obligation: Existing junior claims and pending losses cannot consume fresh user
//! principal. Aggregate token conservation is necessary but not sufficient; attribution must show
//! that a new entrant pays only obligations created by that entrant's own episode.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_stale_cohort_route_matrix_discovers_fresh_principal_subordination` creates a
//! historical source-backed winner and stale loser entirely through public instructions, then
//! novates the exposure to a fresh funded entrant through each single/batch CPI/no-CPI route. It
//! settles every account to a terminal token payout and requires the winner's profit to equal the
//! fresh entrant's principal loss plus the original loser's loss while total SPL supply remains
//! conserved. That is an owner-attribution violation even though aggregate stock balances.
//!
//! Guarantee boundary: this is a retained vulnerable-pin counterexample, not a certification.
//! Current, half-backed, stale-certificate, loss-stale, pending-close, resolved-payout, and
//! insurance-withdrawal positive/control rows still need one normalized public route-by-state
//! matrix before INV-027 can be promoted beyond partial coverage.
//!
//! Secondary coverage: INV-039. The same trace proves that novation cannot erase or transfer a
//! pre-existing cohort's pending loss obligation, while INV-027 owns the terminal principal
//! attribution equation.

use super::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 4) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 8) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_027_stale_cohort_seniority.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_stale_cohort_route_matrix_discovers_fresh_principal_subordination(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_stale_cohort_novations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), StaleCohortRoute::ALL.len());
        for (expected, discovery) in StaleCohortRoute::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.route, expected);
            prop_assert_eq!(discovery.pre_stale_long_count, 0);
            prop_assert_eq!(discovery.pre_stale_short_count, 0);
            prop_assert_eq!(discovery.pre_negative_pnl_count, 0);
            prop_assert!(discovery.novation_landed);
            prop_assert!(discovery.settlement_cranks > 0);
            prop_assert!(discovery.winner_profit > 0);
            prop_assert!(discovery.entrant_principal_loss > 0);
            prop_assert!(discovery.loser_principal_loss > 0);
            prop_assert_eq!(
                discovery.winner_profit,
                discovery
                    .entrant_principal_loss
                    .checked_add(discovery.loser_principal_loss)
                    .expect("bounded principal losses add")
            );
            prop_assert!(discovery.all_positions_terminal);
            prop_assert!(discovery.token_supply_conserved);
            prop_assert!(
                discovery.is_violation(),
                "vulnerable-pin seniority behavior changed: {discovery:?}"
            );
        }
    }
}
