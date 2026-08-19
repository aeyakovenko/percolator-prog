//! INV-081 - Success-state validity over complete public routes.
//!
//! Normative obligation: Every successful wrapper-plus-engine route preserves global invariants and authorized deltas.
//!
//! Evidence in this file (F over public I routes): `v16_program_stateful_public_interface_fuzz`
//! generates deposits, withdrawals, all four trade routes, retained transactions, oracle changes,
//! cranks, fee synchronization, matcher-capability changes, insurance/backing top-ups and
//! withdrawals, released-PnL conversion, unilateral rebalance reduction, asset shutdown, owner
//! recovery-leg forfeit, permissionless abandoned-asset force close, oracle-authority rotation,
//! market resolution, resolved crank/close/claim transitions, and hostile account substitution.
//! Terminal routes independently reconcile position and effective-OI removal, receipt monotonicity,
//! and exact destination/SPL-vault/engine-vault deltas. After every public transition the shared oracle
//! rejects undecodable or hidden legs, duplicate same-asset legs, stale
//! generation bindings, source-lien classification mismatches, stored-position/OI drift, and
//! net-position drift. Successful non-token routes must preserve every tracked SPL account
//! byte-for-byte; value routes may mutate only their canonical source/destination and vault, with
//! exact authorized deltas. Every rejected route must roll back all tracked program bytes, SPL
//! data, and economic-account lamports.
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
use percolator::POS_SCALE;

#[test]
fn v16_program_value_withdrawal_routes_preserve_exact_whole_route_deltas() {
    let coverage = run_value_withdrawal_route_oracle()
        .expect("insurance and backing withdrawal routes must satisfy the shared public oracle");
    assert_eq!(coverage.insurance_topups, 1);
    assert_eq!(coverage.insurance_withdrawals, 1);
    assert_eq!(coverage.backing_topups, 1);
    assert_eq!(coverage.backing_withdrawals, 1);
    assert_eq!(
        coverage.token_frame_checks, 4,
        "each value route must execute an exact SPL account-frame check"
    );
}

#[test]
fn v16_program_abandoned_asset_force_close_strictly_reduces_public_exposure() {
    let coverage = run_abandoned_asset_force_close_oracle()
        .expect("permissionless force close must satisfy the shared position/OI/frame oracle");
    assert_eq!(coverage.force_close_attempts, 1);
    assert_eq!(coverage.force_close_successes, 1);
    assert_eq!(coverage.force_closed_abs_q, POS_SCALE);
}

#[test]
fn v16_program_owner_recovery_forfeit_strictly_reduces_each_position_episode() {
    let coverage = run_recovery_forfeit_route_oracle()
        .expect("owner recovery forfeits must satisfy the shared position/OI/frame oracle");
    assert_eq!(coverage.recovery_forfeit_attempts, 2);
    assert_eq!(coverage.recovery_forfeit_successes, 2);
    assert_eq!(coverage.recovery_forfeited_abs_q, POS_SCALE);
}

#[test]
fn v16_program_recovery_exit_restart_and_fresh_generation_trade_compose() {
    let coverage = run_recovery_restart_trade_route_oracle().expect(
        "recovery exits, asset restart, and fresh-generation trade must share one exact oracle",
    );
    assert_eq!(coverage.asset_restarts, 1);
    assert_eq!(coverage.recovery_forfeit_successes, 2);
    assert!(coverage.route_success[0] >= 2);
}

#[test]
fn v16_program_designated_liquidity_provider_has_public_exit_after_unilateral_reduction() {
    let scenario = Scenario {
        seed: [0x18; 32],
        config: SmallMarketConfig {
            max_price_move_bps_per_slot: 1_000,
            max_accrual_dt_slots: 4,
            max_abs_funding_e9_per_slot: 0,
            maintenance_fee_per_slot: 0,
        },
        actions: vec![Action::RebalanceReduce { actor: 3, asset: 0 }],
    };

    let coverage = run_scenario(&scenario)
        .expect("every modeled portfolio, including the exit-liquidity provider, must exit");
    assert_ne!(
        coverage.rebalance_reductions, 0,
        "setup must execute a real unilateral reduction"
    );
    assert_ne!(
        coverage.user_positions_closed, 0,
        "the resulting asymmetric portfolio set must reach public terminal exits"
    );
}

#[test]
fn v16_program_extended_public_action_alphabet_runs_through_shared_oracles() {
    let scenario = Scenario {
        seed: [0x81; 32],
        config: SmallMarketConfig::default(),
        actions: vec![
            Action::SetMatcherConfig {
                actor: 0,
                enabled: false,
                trade_fee_cap_bps: 0,
            },
            Action::TopUpInsurance {
                domain: 0,
                amount: 7,
            },
            Action::TopUpBacking {
                domain: 1,
                amount: 500,
                expiry_delta: 200,
            },
            Action::WithdrawInsurance {
                asset: 0,
                amount: 3,
            },
            Action::WithdrawBacking {
                domain: 1,
                amount: 3,
            },
            Action::RebalanceReduce { actor: 0, asset: 2 },
            Action::PushMark {
                asset: 0,
                dt: 4,
                move_bps: 500,
            },
            Action::Trade {
                route: TradeRoute::NoCpi,
                taker: 0,
                maker: 1,
                asset: 0,
                units: 1,
                fee_bps: 0,
                price_move_bps: 0,
                prefer_reduce: true,
            },
            Action::Trade {
                route: TradeRoute::NoCpi,
                taker: 0,
                maker: 1,
                asset: 1,
                units: 1,
                fee_bps: 0,
                price_move_bps: 0,
                prefer_reduce: true,
            },
            Action::Crank {
                actor: 0,
                hints: HintMode::Complete,
            },
            Action::ConvertReleasedPnl {
                actor: 0,
                amount: 2_000,
            },
            Action::Crank {
                actor: 2,
                hints: HintMode::Complete,
            },
            Action::Trade {
                route: TradeRoute::NoCpi,
                taker: 2,
                maker: 3,
                asset: 0,
                units: 1,
                fee_bps: 0,
                price_move_bps: 0,
                prefer_reduce: true,
            },
            Action::RotateOracleAuthority {
                asset: 2,
                new_actor: 0,
            },
            Action::ConfigurePermissionlessResolve {
                stale_slots: 1_000,
                force_close_delay_slots: 100,
            },
            Action::ShutdownAsset { asset: 0, dt: 0 },
            Action::ForfeitRecoveryLeg {
                actor: 0,
                asset: 0,
                budget_units: u8::MAX,
            },
            Action::ForceCloseAbandoned {
                cranker: 2,
                account_a: 0,
                account_b: 1,
                asset: 0,
                dt: 1,
                units: 1,
            },
            Action::RestartAssetOracle {
                asset: 0,
                dt: 1,
                initial_price: 137,
            },
            Action::ResolveMarket,
            Action::Crank {
                actor: 1,
                hints: HintMode::Complete,
            },
            Action::CloseResolved { actor: 0 },
            Action::ClaimResolvedPayoutTopup { actor: 0 },
        ],
    };

    let coverage = run_scenario(&scenario).expect("extended public action scenario");
    assert!(
        coverage
            .extended_action_attempts
            .iter()
            .all(|attempts| *attempts != 0),
        "every added public action class must execute through the shared success/rollback oracle: {coverage:?}"
    );
    assert!(coverage.matcher_config_updates != 0);
    assert!(coverage.insurance_topups != 0);
    assert!(coverage.backing_topups != 0);
    assert!(coverage.pnl_conversions != 0);
    assert!(coverage.rebalance_reductions != 0);
    assert!(coverage.authority_updates != 0);
    assert!(coverage.resolve_policy_updates != 0);
    assert!(coverage.lifecycle_updates != 0);
    assert!(coverage.terminal_resolve_attempts != 0, "{coverage:?}");
    assert!(coverage.terminal_resolves != 0, "{coverage:?}");
    assert!(coverage.resolved_crank_attempts != 0, "{coverage:?}");
    assert!(coverage.resolved_crank_successes != 0, "{coverage:?}");
    assert!(coverage.resolved_close_attempts != 0, "{coverage:?}");
    assert!(coverage.resolved_close_successes != 0, "{coverage:?}");
    assert!(coverage.resolved_claim_attempts != 0, "{coverage:?}");
    assert!(
        coverage.resolved_crank_mutations + coverage.resolved_close_mutations != 0,
        "{coverage:?}"
    );
    assert!(coverage.resolved_payout_atoms != 0, "{coverage:?}");
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
    fn v16_program_stateful_public_interface_fuzz(
        scenario in scenario_strategy(env_usize("PERCOLATOR_FUZZ_ACTIONS", 12))
    ) {
        let serialized = serde_json::to_string_pretty(&scenario).unwrap();
        let result = run_scenario(&scenario);
        prop_assert!(result.is_ok(), "stateful public-interface scenario failed: {}\n{}",
            result.unwrap_err(), serialized);
    }
}
