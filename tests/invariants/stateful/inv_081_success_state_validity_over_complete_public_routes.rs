//! INV-081 - Success-state validity over complete public routes.
//!
//! Normative obligation: Every successful wrapper-plus-engine route preserves global invariants and authorized deltas.
//!
//! Evidence in this file (F over public I routes): `v16_program_stateful_public_interface_fuzz`
//! generates deposits, withdrawals, all four trade routes, retained transactions, oracle changes,
//! cranks, fee synchronization, matcher-capability changes, insurance/backing top-ups and
//! withdrawals, released-PnL conversion, unilateral rebalance reduction, asset shutdown, owner
//! recovery-leg forfeit, permissionless abandoned-asset force close, oracle-authority rotation,
//! DrainOnly admission/reduction, empty-asset retirement, market resolution, resolved
//! crank/close/claim transitions, and hostile account substitution.
//! Terminal routes independently reconcile position and effective-OI removal, receipt monotonicity,
//! and exact destination/SPL-vault/engine-vault deltas. After every public transition the shared oracle
//! rejects undecodable or hidden legs, duplicate same-asset legs, stale
//! generation bindings, source-lien classification mismatches, stored-position/OI drift, and
//! net-position drift. It also rebuilds per-side stale/pending counts and social-loss weights from
//! every leg, and rebuilds stale-certificate, B-stale, and negative-PnL account counts from every
//! materialized portfolio. Successful non-token routes must preserve every tracked SPL account
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
fn v16_program_permissionless_stale_resolution_reaches_terminal_user_disposition() {
    let coverage = run_permissionless_stale_resolution_terminal_oracle().expect(
        "permissionless stale resolution must compose with the exact terminal payout campaign",
    );
    assert_eq!(coverage.permissionless_resolve_attempts, 1);
    assert_eq!(coverage.permissionless_resolves, 1);
    assert!(coverage.resolved_crank_successes != 0);
    assert!(coverage.resolved_close_attempts != 0);
}

#[test]
fn v16_program_drain_only_allows_reduction_then_empty_asset_retires() {
    let coverage = run_drain_reduce_retire_route_oracle().expect(
        "DrainOnly must reject new exposure, admit bilateral reduction, and permit empty retirement",
    );
    assert_eq!(coverage.drain_only_updates, 1);
    assert_eq!(coverage.asset_retirements, 1);
    assert_eq!(coverage.route_reject[0], 1);
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

/// Issue #449, the case the public-interface fuzz drew on 2026-11-19. A unilateral reduction
/// lowers the short side's A index on asset 1 to 2/7. The two remaining shorts have exact sizes
/// 357,142.86 and 142,857.14 and are each rounded up on their own, to 357,143 and 142,858,
/// against a side counter of 500,000. The reference model types the extra unit as ADL rounding
/// residue, not as an unmatched position, and every portfolio still exits by both campaigns.
#[test]
fn v16_program_adl_rounding_residue_is_typed_and_every_portfolio_exits() {
    let scenario = Scenario {
        seed: [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 39, 45, 184, 69, 152, 85, 152, 123, 3, 229, 144, 164, 180,
            35, 206, 24, 234, 168, 234, 41, 183, 55, 3,
        ],
        config: SmallMarketConfig {
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 2,
            max_abs_funding_e9_per_slot: 0,
            maintenance_fee_per_slot: 1,
        },
        actions: vec![
            Action::TopUpInsurance {
                domain: 124,
                amount: 244,
            },
            Action::AdvanceBlockhash,
            // Within the two-slot window (INV-045's catch-up rule). At dt 3 the push needs a
            // catch-up crank first, and with that crank in the history this seed leaves no
            // rounding residue on asset 1, whatever the push's price.
            Action::PushMark {
                asset: 132,
                dt: 2,
                move_bps: -62,
            },
            Action::Trade {
                route: TradeRoute::BatchNoCpi,
                taker: 235,
                maker: 139,
                asset: 142,
                units: 3,
                fee_bps: 19,
                price_move_bps: 279,
                prefer_reduce: false,
            },
            Action::RebalanceReduce {
                actor: 143,
                asset: 223,
            },
            Action::DrainOnlyAsset { asset: 151 },
            Action::Trade {
                route: TradeRoute::NoCpi,
                taker: 162,
                maker: 93,
                asset: 134,
                units: 2,
                fee_bps: 21,
                price_move_bps: -975,
                prefer_reduce: true,
            },
            Action::Crank {
                actor: 25,
                hints: HintMode::Empty,
            },
            Action::Withdraw {
                actor: 224,
                amount: 391,
            },
            Action::ResolveStalePermissionless { dt: 108 },
        ],
    };

    assert_eq!(
        adl_rounding_residue_after(&scenario, 4).expect("history before the unilateral reduction"),
        [[0, 0]; 3],
        "no side is haircut yet, so legs and counters agree exactly"
    );
    assert_eq!(
        adl_rounding_residue_after(&scenario, 5).expect("the unilateral reduction is admitted"),
        [[0, 0], [0, 1], [0, 0]],
        "the haircut short side of asset 1 carries exactly one unit of rounding residue"
    );

    let coverage = run_scenario(&scenario).expect("every portfolio exits with the residue typed");
    assert_ne!(
        coverage.rebalance_reductions, 0,
        "setup must execute a real unilateral reduction"
    );
    assert_ne!(
        coverage.user_positions_closed, 0,
        "the haircut legs must reach public terminal exits"
    );
}

/// Issue #449, the engine's side rule in the reference model. Two longs share a side that a
/// departing short has haircut. One reduces in odd quantities through the direct and the batch
/// trade route, the other once as owner, and the model's own copy of the rule has to reproduce
/// the raw basis the
/// deployed program keeps at every step. The generated histories of this suite never round a
/// partial reduction on a haircut side, so this directed one carries that part of the model:
/// with the rounding of `SideBook::raw_after_reduction` turned the other way it fails at the
/// first fill.
#[test]
fn v16_program_haircut_side_reductions_follow_the_side_rule_and_every_portfolio_exits() {
    let (evidence, coverage) =
        crate::support::fuzz_model::reproduce_haircut_side_reductions_follow_the_side_rule()
            .expect("haircut-side reductions match the reference model and every portfolio exits");
    println!("haircut-side reductions: {evidence:?}");
    // Ten matched fills reduce the first long, and the last two also reduce the short that is
    // alone on its haircut side; the owner reduction makes one more.
    assert_eq!(evidence.reductions, 13, "{evidence:?}");
    assert!(
        evidence.rounded_reductions >= 10,
        "the directed sizes must round: {evidence:?}"
    );
    assert!(
        evidence.index_rises > 0 && evidence.peer_quantity_rises > 0,
        "the untouched long must see the index and its own quantity move: {evidence:?}"
    );
    assert_ne!(
        coverage.user_positions_closed, 0,
        "the haircut legs must reach public terminal exits"
    );
}

/// Issue #449, the same rule where the engine picks the size: a partial liquidation of one of
/// two shorts that share a haircut side. The model reads the liquidated quantity off the side
/// counter, requires the raw basis the leg keeps to be the one its side rule derives for that
/// quantity, and requires the opposite side to lose exactly the same.
#[test]
fn v16_program_liquidation_on_a_shared_haircut_side_follows_the_side_rule() {
    let coverage = crate::support::fuzz_model::reproduce_liquidation_on_a_shared_haircut_side()
        .expect("a liquidation on a shared haircut side matches the reference model");
    println!(
        "haircut-side liquidation: steps={} liquidated={} reductions on a haircut side={} rounded={}",
        coverage.liquidation_steps,
        coverage.liquidated_abs_q,
        coverage.haircut_side_reductions,
        coverage.haircut_side_rounded_reductions
    );
    assert_ne!(coverage.liquidation_steps, 0);
    assert_ne!(
        coverage.haircut_side_reductions, 0,
        "a liquidation must leave the leg open on the haircut side"
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
                domain: 3,
                amount: 500,
                expiry_delta: 200,
            },
            Action::WithdrawInsuranceAsset {
                asset: 0,
                amount: 3,
            },
            Action::WithdrawBacking {
                domain: 3,
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
            Action::DrainOnlyAsset { asset: 2 },
            Action::RetireAsset { asset: 2, dt: 1 },
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
            Action::ResolveStalePermissionless { dt: 1 },
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
