//! INV-039 - Pending-loss obligation durability.
//!
//! Normative obligation: Pending accrual and loss obligations cannot be erased by route choice or lifecycle changes.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_accrual_boundary_operation_matrix_preserves_transfers` builds one
//! zero-price-move funding checkpoint and permutes settlement against CPI close, batch CPI close,
//! unilateral reduction, and recovery forfeit. The common oracle requires both sides to book the
//! same nonzero transfer, resolves every participant, and compares exact destination-token payouts
//! across the two orders. The recovery-forfeit route derives a two-atom aggregate terminal residue
//! bound from its two positive terminal claimants: no destination may gain and each claimant may
//! lose at most one floor atom. That residue cannot classify as LoF.
//! `v16_program_prospective_accrual_route_matrix_preserves_elapsed_funding` independently varies
//! all single/batch CPI/no-CPI trade routes around the same funding catch-up boundary. Every route
//! requires identical funding indices and unrelated-victim payout, drains every actor through the
//! public resolved rails, and recomputes both paired worlds' victim/coalition payouts from their
//! exact destination-token trace deltas. No-CPI routes additionally require identical terminal
//! prices and payouts because their signed execution price is fixed; CPI matcher quotes
//! legitimately vary with the crank-first versus trade-first oracle input.
//! `v16_program_shutdown_commit_ordering_preserves_committed_funding` applies the same ordering
//! oracle to asset shutdown while constraining the effective price to remain unchanged. Any payout
//! difference is therefore a committed funding transfer erased by the lifecycle transition.
//! `v16_program_partial_liquidation_cannot_erase_pending_funding` starts a short exactly at its
//! maintenance boundary, creates a zero-price-move nonzero funding checkpoint, and compares
//! settle-then-liquidate with direct automatic-crank dispatch. Both schedules must perform a
//! strict partial liquidation from byte-identical fixtures, book the same payer/receiver transfer,
//! terminate every funded user, and produce identical destination-token payouts under the public
//! trace oracle.
//! INV-027 owns the stale-cohort novation guard because its economic obligation is protection of
//! a fresh entrant's principal. This file retains the independent pending-obligation and
//! accrual-ordering matrices that lead to that broader seniority property.
//! Direct impact tests remain below. These tests
//! exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: the prospective-accrual, exposure-removal, asset-shutdown, and
//! terminal-resolve matrices are fixed-pin certification. INV-027 certifies stale-cohort rejection,
//! exact rollback, finite settlement, and owner-reduction liveness. INV-039 composes these public
//! traces with the pinned engine's retain/release/clear contracts, reset blocker proof, and INV-088's
//! source-complete wrapper transition roster. A new position-removal route, obligation field writer,
//! reset gate, layout, or engine pin reopens certification.

use super::*;
use crate::support::{
    invariant_discovery::discover_partial_liquidation_accrual_ordering, v16_svm::TX_CU_LIMIT,
};

#[test]
fn v16_host_market_roundtrip_preserves_funding_mark_checkpoint() {
    let mut env = crate::support::v16_svm::V16Svm::new(
        [0x39; 32],
        crate::support::v16_svm::MarketConfig::default(),
    );
    let slot = env.current_slot();
    env.configure_ewma_mark(0, slot, 1_000_000, 600, 0).unwrap();
    let mut data = env.svm.get_account(&env.market).unwrap().data;
    let (cfg, group) = percolator_prog::state::read_market(&data).unwrap();
    let mut profile = percolator_prog::state::read_asset_oracle_profile(&data, 0).unwrap();
    profile.funding_mark_e6 = profile.mark_ewma_e6;
    profile.funding_mark_pending_e6 = profile.mark_ewma_e6 + 1;
    profile.funding_mark_pending_slot = group.current_slot + 1;
    percolator_prog::state::write_asset_oracle_profile(&mut data, 0, &profile).unwrap();

    percolator_prog::state::write_market(&mut data, &cfg, &group).unwrap();
    let after = percolator_prog::state::read_asset_oracle_profile(&data, 0).unwrap();
    assert_eq!(after.funding_mark_e6, profile.funding_mark_e6);
    assert_eq!(
        after.funding_mark_pending_e6,
        profile.funding_mark_pending_e6
    );
    assert_eq!(
        after.funding_mark_pending_slot,
        profile.funding_mark_pending_slot
    );
}

#[test]
fn v16_program_partial_liquidation_cannot_erase_pending_funding() {
    let discovery = discover_partial_liquidation_accrual_ordering([0x39; 32])
        .unwrap_or_else(|error| panic!("partial-liquidation accrual ordering failed: {error}"));
    assert_eq!(discovery.control_paid, 200, "{discovery:?}");
    assert_eq!(discovery.control_paid, discovery.control_received);
    assert_eq!(discovery.reordered_paid, discovery.control_paid);
    assert_eq!(discovery.reordered_received, discovery.control_received);
    assert!(
        discovery.control_effective_oi_q < discovery.initial_effective_oi_q,
        "{discovery:?}"
    );
    assert!(
        discovery.reordered_effective_oi_q < discovery.initial_effective_oi_q,
        "{discovery:?}"
    );
    assert_ne!(discovery.control_effective_oi_q, 0, "{discovery:?}");
    assert_ne!(discovery.reordered_effective_oi_q, 0, "{discovery:?}");
    assert_eq!(
        discovery.reordered_effective_oi_q,
        discovery.control_effective_oi_q
    );
    assert_eq!(
        discovery
            .initial_effective_oi_q
            .checked_sub(discovery.control_effective_oi_q),
        Some(200_000),
        "{discovery:?}"
    );
    assert_eq!(discovery.reordered_payouts, discovery.control_payouts);
    assert_eq!(
        discovery.control_payouts,
        [99_800, 1_000_200, 1_000_000, 1_000_000, 1_000_000]
    );
    assert!(discovery.control_terminal && discovery.reordered_terminal);
    assert!(discovery.control_max_crank_cu < TX_CU_LIMIT);
    assert!(discovery.reordered_max_crank_cu < TX_CU_LIMIT);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_039_accrual_ordering_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_accrual_boundary_operation_matrix_preserves_transfers(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_accrual_ordering_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), AccrualOrderingKind::ALL.len());
        for (expected, discovery) in AccrualOrderingKind::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.kind, expected);
            prop_assert!(discovery.certifies_terminal_value(), "{discovery:?}");
            let mut wrong_payout_identity = discovery.clone();
            wrong_payout_identity.terminal_evidence.victim_destinations =
                vec![discovery.terminal_evidence.counterparty_destinations[0]];
            prop_assert!(!wrong_payout_identity.is_violation());
            prop_assert!(!wrong_payout_identity.certifies_terminal_value());
            if discovery.terminal_evidence.terminal_rounding_residue_bound != 0 {
                prop_assert_eq!(discovery.kind, AccrualOrderingKind::RecoveryForfeit);
                prop_assert_eq!(discovery.terminal_positive_claimants, 2);
                prop_assert_eq!(discovery.max_destination_payout_loss, 1);
                prop_assert_eq!(discovery.destination_payout_gain, 0);
                prop_assert_eq!(
                    discovery.terminal_evidence.terminal_rounding_residue_bound,
                    u128::from(discovery.terminal_positive_claimants)
                );
                let observed_residue = discovery.control_total_payout
                    .checked_sub(discovery.reordered_total_payout)
                    .ok_or_else(|| TestCaseError::fail(
                        "reordered recovery payout exceeded its control"
                    ))?;
                prop_assert_ne!(observed_residue, 0);
                prop_assert!(observed_residue
                    <= discovery.terminal_evidence.terminal_rounding_residue_bound);
                let mut insufficient_residue_bound = discovery.clone();
                insufficient_residue_bound.terminal_evidence.terminal_rounding_residue_bound =
                    observed_residue - 1;
                prop_assert!(!insufficient_residue_bound.is_violation());
                prop_assert!(!insufficient_residue_bound.certifies_terminal_value());
            }
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.kind)
            .collect();
        prop_assert!(
            violations.is_empty(),
            "accrual-ordering invariant violations remain: {violations:?}"
        );
    }

    #[test]
    fn v16_program_multi_segment_accrual_ordering_preserves_transfers(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_multi_segment_accrual_ordering_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), AccrualOrderingKind::ALL.len());
        for (expected, discovery) in AccrualOrderingKind::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.kind, expected);
            prop_assert!(discovery.unsafe_action_rejected, "{discovery:?}");
            prop_assert!(discovery.rejected_exact_rollback, "{discovery:?}");
            prop_assert!(discovery.retry_landed, "{discovery:?}");
            prop_assert!(discovery.certifies_terminal_value(), "{discovery:?}");
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.kind)
            .collect();
        prop_assert!(
            violations.is_empty(),
            "multi-segment accrual-ordering violations remain: {violations:?}; \
             discoveries={discoveries:?}"
        );
    }

    #[test]
    fn v16_program_shutdown_commit_ordering_preserves_committed_funding(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_shutdown_commit_ordering(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_ne!(discovery.control_f_long_num, 0);
        prop_assert_ne!(discovery.control_f_short_num, 0);
        prop_assert_eq!(
            discovery.shutdown_f_long_num,
            discovery.control_f_long_num
        );
        prop_assert_eq!(
            discovery.shutdown_f_short_num,
            discovery.control_f_short_num
        );
        prop_assert_eq!(discovery.victim_payout_loss, 0);
        prop_assert_eq!(discovery.counterparty_payout_gain, 0);
        prop_assert!(!discovery.is_violation(), "{discovery:?}");
        prop_assert!(discovery.certifies_terminal_ordering(), "{discovery:?}");
        let mut wrong_payout_identity = discovery.clone();
        wrong_payout_identity.terminal_evidence.victim_destinations =
            vec![discovery.terminal_evidence.counterparty_destinations[0]];
        prop_assert!(!wrong_payout_identity.is_violation());
        prop_assert!(!wrong_payout_identity.certifies_terminal_ordering());
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_039_shutdown_catchup.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_shutdown_rejection_retains_bounded_public_progress(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_shutdown_catchup_liveness(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(discovery.initial_shutdown_rejected);
        prop_assert!(discovery.rejected_exact_rollback);
        prop_assert!(discovery.catchup_steps > 0);
        prop_assert!(discovery.catchup_steps <= 16);
        prop_assert!(discovery.max_catchup_cu < 1_400_000);
        prop_assert!(discovery.retry_landed);
        prop_assert_ne!(discovery.f_long_num, 0);
        prop_assert_ne!(discovery.f_short_num, 0);
        prop_assert!(discovery.users_terminal);
        prop_assert_eq!(discovery.total_payout, 2_000_000);
        prop_assert!(discovery.token_supply_conserved);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_039_pending_zero_move_terminal.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_pending_zero_move_terminal_ordering_fuzz(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_pending_zero_move_terminal_ordering(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(discovery.unsafe_resolve_rejected);
        prop_assert!(discovery.rejected_exact_rollback);
        prop_assert!(discovery.catchup_steps >= 2);
        prop_assert!(discovery.catchup_steps <= 16);
        prop_assert!(discovery.max_catchup_cu < 1_400_000);
        prop_assert_ne!(discovery.control_f_long_num, 0);
        prop_assert_ne!(discovery.control_f_short_num, 0);
        prop_assert_eq!(
            discovery.reordered_f_long_num,
            discovery.control_f_long_num
        );
        prop_assert_eq!(
            discovery.reordered_f_short_num,
            discovery.control_f_short_num
        );
        prop_assert!(!discovery.has_funding_divergence(), "{discovery:?}");
        prop_assert!(!discovery.is_violation(), "{discovery:?}");
        prop_assert!(discovery.certifies_terminal_ordering(), "{discovery:?}");
        let mut wrong_payout_identity = discovery.clone();
        wrong_payout_identity.terminal_evidence.victim_destinations =
            vec![discovery.terminal_evidence.counterparty_destinations[0]];
        prop_assert!(!wrong_payout_identity.is_violation());
        prop_assert!(!wrong_payout_identity.certifies_terminal_ordering());
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_039_prospective_accrual_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_prospective_accrual_route_matrix_preserves_elapsed_funding(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_prospective_accrual_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), ProspectiveAccrualRoute::ALL.len());
        for (expected, discovery) in ProspectiveAccrualRoute::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.route, expected);
        }
        for discovery in discoveries {
            prop_assert!(!discovery.is_violation(), "{discovery:?}");
            prop_assert!(discovery.certifies_terminal_ordering(), "{discovery:?}");
            let mut wrong_payout_identity = discovery.clone();
            wrong_payout_identity.terminal_evidence.victim_destinations =
                vec![discovery.terminal_evidence.counterparty_destinations[0]];
            prop_assert!(!wrong_payout_identity.is_violation());
            prop_assert!(!wrong_payout_identity.certifies_terminal_ordering());
            prop_assert!(discovery.control_f_short_num > 0);
            prop_assert_eq!(
                discovery.reordered_f_short_num,
                discovery.control_f_short_num
            );
            prop_assert_eq!(discovery.victim_payout_loss, 0);
            if matches!(
                discovery.route,
                ProspectiveAccrualRoute::NoCpi | ProspectiveAccrualRoute::BatchNoCpi
            ) {
                prop_assert_eq!(discovery.coalition_payout_gain, 0);
                prop_assert_eq!(
                    discovery.reordered_total_payout,
                    discovery.control_total_payout
                );
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_039_terminal_commit_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_terminal_commit_ordering_preserves_pending_value(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_terminal_commit_ordering(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(
            !discovery.is_violation(),
            "terminal-commit ordering still discards value: {discovery:?}"
        );
        prop_assert!(discovery.certifies_terminal_ordering(), "{discovery:?}");
        let mut wrong_payout_identity = discovery.clone();
        wrong_payout_identity.terminal_evidence.victim_destinations =
            vec![discovery.terminal_evidence.counterparty_destinations[0]];
        prop_assert!(!wrong_payout_identity.is_violation());
        prop_assert!(!wrong_payout_identity.certifies_terminal_ordering());
        prop_assert!(discovery.unsafe_resolve_rejected);
        prop_assert!(discovery.rejected_exact_rollback);
        prop_assert!(discovery.catchup_steps > 0);
        prop_assert!(discovery.catchup_steps <= 16);
        prop_assert!(discovery.max_catchup_cu < 1_400_000);
        prop_assert_eq!(discovery.committed_mark, discovery.reordered_mark);
        prop_assert_eq!(discovery.victim_payout_loss, 0);
        prop_assert_eq!(discovery.counterparty_payout_gain, 0);
        prop_assert_eq!(
            discovery.committed_total_payout,
            discovery.reordered_total_payout
        );
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
    fn v16_program_pr380_prospective_funding_preservation_fuzz(
        (seed, route) in prospective_funding_rewrite_strategy()
    ) {
        let reproduction = reproduce_prospective_funding_rewrite(seed, route)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(reproduction.route, route);
        prop_assert!(reproduction.control_f_short_num > 0);
        prop_assert_eq!(
            reproduction.attack_f_short_num,
            reproduction.control_f_short_num
        );
        prop_assert_eq!(reproduction.victim_payout_loss, 0);
        if matches!(route, TradeRoute::NoCpi | TradeRoute::BatchNoCpi) {
            prop_assert_eq!(reproduction.attacker_coalition_gain, 0);
            prop_assert_eq!(
                reproduction.attack_total_payout,
                reproduction.control_total_payout
            );
        }
    }

    #[test]
    fn v16_program_pr255_resolve_requires_public_catchup_fuzz(
        seed in resolve_before_committed_accrual_seed_strategy()
    ) {
        let result = reproduce_resolve_before_committed_accrual(seed);
        prop_assert!(
            result.is_ok(),
            "PR 255 fixed invariant failed for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr271_trade_funding_preservation_fuzz(
        (seed, route) in trade_funding_erasure_strategy()
    ) {
        let result = reproduce_trade_funding_erasure(seed, route);
        prop_assert!(
            result.is_ok(),
            "PR 271 {:?} fixed invariant failed for seed {:?}: {}",
            route,
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr272_rebalance_funding_preservation_fuzz(
        seed in rebalance_funding_erasure_seed_strategy()
    ) {
        let result = reproduce_rebalance_funding_erasure(seed);
        prop_assert!(
            result.is_ok(),
            "PR 272 fixed invariant failed for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }

    #[test]
    fn v16_program_pr273_forfeit_funding_preservation_fuzz(
        seed in forfeit_funding_erasure_seed_strategy()
    ) {
        let result = reproduce_forfeit_funding_erasure(seed);
        prop_assert!(
            result.is_ok(),
            "PR 273 fixed invariant failed for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }
}
