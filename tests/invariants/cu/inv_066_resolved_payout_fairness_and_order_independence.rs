//! INV-066 - Resolved-payout fairness and order independence.
//!
//! Normative obligation: Resolved entitlement is snapshot-bound and claimant-order independent except explicit residue.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_resolved_close_order_preserves_scarce_source_backing`, `v16_bpf_force_close_pair_order_preserves_terminal_user_payouts`, `v16_attack_resolved_two_public_winners_are_close_order_independent`, `v16_bpf_force_close_pair_order_preserves_unequal_partial_payouts`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//! `v16_program_resolved_payout_induction_composition_is_source_complete` joins those public
//! routes to the exact-pin engine receipt contracts and a full-`u128`, claimant-count-independent
//! induction proof. The induction is conditional on `RESOLVED_RATE_SUM_AXIOM`; deployed arithmetic
//! differential tests discharge that named axiom empirically without reintroducing the
//! solver-intractable wide multiply/divide circuit into Kani.
//! `v16_program_late_receipt_materialization_preserves_snapshot_entitlements` crosses unequal
//! claimant order with exact receipt replacement before/after a real backing release. Unlike the
//! existing all-receipts-first matrices, an earlier claimant is paid at the increased rate while
//! another claim remains unreceipted. Both schedules must preserve the same claim denominator,
//! claimant-local SPL entitlements and terminal stock, including exact rounding residue.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_late_receipt_materialization_preserves_snapshot_entitlements() {
    use super::inv_067_terminal_payout_completeness_and_exact_once_settlement::late_expiry::World;
    use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

    const FACES: [u128; 5] = [700, 0, 1_000, 0, 1_300];
    const CAPITAL: [u128; 5] = [1_000, 0, 1_000, 0, 1_000];
    const TOTAL_FACE: u128 = 3_000;
    const INITIAL_RESIDUAL: u128 = 501;
    const FINAL_RESIDUAL: u128 = 851;

    fn step(world: &mut World, actor: usize, claim: bool) -> Option<u128> {
        let frame = world.frame();
        let destination = world.actors[actor].token;
        let tokens_before = world.env.token_amount(destination);
        let vault_before = world.env.market_state().1.vault;
        let result = world.land(&[world.payout(actor, claim)], false);
        let meta = match &result {
            Ok(meta) => meta,
            Err(failure) => &failure.meta,
        };
        assert_cu_within(
            "INV-066 late receipt payout/cleanup",
            meta.compute_units_consumed,
            CUSTODY_CU_LIMIT,
        );
        match result {
            Ok(_) => {
                let paid = world
                    .env
                    .token_amount(destination)
                    .checked_sub(tokens_before)
                    .unwrap();
                assert_eq!(
                    vault_before.checked_sub(world.env.market_state().1.vault),
                    Some(u128::from(paid)),
                    "each public payout must debit exactly its SPL credit"
                );
                world.assert_frame_except(
                    &frame,
                    &[
                        world.env.market,
                        world.env.vault,
                        world.actors[actor].portfolio,
                        destination,
                    ],
                );
                world.custody();
                for actor in 0..5 {
                    assert!(
                        world.env.token_amount(world.actors[actor].token) as u128
                            <= CAPITAL[actor] + FACES[actor] * FINAL_RESIDUAL / TOTAL_FACE,
                        "no payout/cleanup prefix may overpay claimant {actor}"
                    );
                }
                Some(u128::from(paid))
            }
            Err(failure) => {
                assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(
                        2,
                        InstructionError::Custom(PercolatorError::EngineNonProgress as u32)
                    )
                );
                assert_eq!(
                    world.frame(),
                    frame,
                    "deferred cleanup must roll back exactly"
                );
                None
            }
        }
    }

    fn materialize(world: &mut World, actor: usize, residual: u128) {
        let before = world.env.market_state().1;
        assert!(!world.receipt(actor).present);
        for _ in 0..8 {
            step(world, actor, false).expect("unreceipted claimant must make bounded progress");
            if world.receipt(actor).present {
                break;
            }
        }
        let receipt = world.receipt(actor);
        assert!(receipt.present && !receipt.finalized);
        assert_eq!(receipt.terminal_positive_claim_face, FACES[actor]);
        assert_eq!(
            receipt.prior_bound_contribution_num,
            FACES[actor] * BOUND_SCALE
        );
        assert_eq!(receipt.live_released_face_at_receipt, 0);
        assert_eq!(receipt.paid_effective, FACES[actor] * residual / TOTAL_FACE);
        assert_eq!(
            world.env.token_amount(world.actors[actor].token) as u128,
            CAPITAL[actor] + receipt.paid_effective
        );
        let after = world.env.market_state().1.resolved_payout_ledger;
        assert_eq!(after.snapshot_slot, 12);
        assert_eq!(after.snapshot_residual, residual);
        assert_eq!(after.current_payout_rate_num, residual * BOUND_SCALE);
        assert_eq!(after.current_payout_rate_den, TOTAL_FACE * BOUND_SCALE);
        assert_eq!(
            after.terminal_claim_exact_receipts_num + after.terminal_claim_bound_unreceipted_num,
            TOTAL_FACE * BOUND_SCALE
        );
        if before.payout_snapshot_captured {
            let before = before.resolved_payout_ledger;
            assert_eq!(after.snapshot_slot, before.snapshot_slot);
            assert_eq!(after.snapshot_residual, before.snapshot_residual);
            assert_eq!(
                after.current_payout_rate_num,
                before.current_payout_rate_num
            );
            assert_eq!(
                after.current_payout_rate_den,
                before.current_payout_rate_den
            );
            assert_eq!(
                after.terminal_claim_exact_receipts_num,
                before.terminal_claim_exact_receipts_num + FACES[actor] * BOUND_SCALE,
                "receipt replacement must add only this claimant's exact face"
            );
            assert_eq!(
                after.terminal_claim_bound_unreceipted_num + FACES[actor] * BOUND_SCALE,
                before.terminal_claim_bound_unreceipted_num,
                "receipt replacement must remove exactly the same prior bound"
            );
        }
    }

    let expected = FACES.map(|face| face * FINAL_RESIDUAL / TOTAL_FACE);
    let residue = FINAL_RESIDUAL - expected.iter().sum::<u128>();
    assert_eq!(
        residue, 2,
        "the fixture must exercise nonzero rounding residue"
    );
    let mut baseline = None;
    let mut peak_cu = 0;
    for defer_second_receipt in [false, true] {
        for order in [[0, 4], [4, 0]] {
            let mut world = World::before_receipts();
            assert!(!world.env.market_state().1.payout_snapshot_captured);
            for actor in [0, 2, 4] {
                assert!(!world.receipt(actor).present);
                assert_eq!(
                    world
                        .env
                        .portfolio_state(world.actors[actor].portfolio)
                        .pnl
                        .get(),
                    FACES[actor] as i128
                );
            }
            materialize(&mut world, order[0], INITIAL_RESIDUAL);
            if !defer_second_receipt {
                materialize(&mut world, order[1], INITIAL_RESIDUAL);
            }

            // Release the independently backed domain only after the first snapshot. In the
            // delayed worlds, the second junior face is still a bound, not an exact receipt.
            let early_receipt = world.receipt(order[0]);
            let late_portfolio = world.env.svm.get_account(&world.actors[order[1]].portfolio);
            world.env.svm.warp_to_slot(13);
            assert_eq!(step(&mut world, 2, false), Some(0));
            let released = world.env.market_state().1;
            assert_ne!(
                released.source_backing_buckets[3].status,
                BackingBucketStatusV16::Fresh
            );
            assert_eq!(
                released.resolved_payout_ledger.snapshot_residual,
                FINAL_RESIDUAL
            );
            assert_eq!(released.resolved_payout_ledger.snapshot_slot, 12);
            assert_eq!(world.receipt(order[0]), early_receipt);
            assert_eq!(
                world.env.svm.get_account(&world.actors[order[1]].portfolio),
                late_portfolio
            );

            let due = expected[order[0]] - early_receipt.paid_effective;
            assert!(due > 0);
            assert_eq!(step(&mut world, order[0], true), Some(due));
            let mut expected_receipt = early_receipt;
            expected_receipt.paid_effective = expected[order[0]];
            assert_eq!(world.receipt(order[0]), expected_receipt);
            assert_eq!(
                world.env.market_state().1.resolved_payout_ledger,
                released.resolved_payout_ledger,
                "an early top-up must not rewrite the snapshot or remaining claim mass"
            );
            assert_eq!(
                world.env.svm.get_account(&world.actors[order[1]].portfolio),
                late_portfolio,
                "paying the early claimant must preserve the late claimant's complete account"
            );
            if defer_second_receipt {
                assert!(!world.receipt(order[1]).present);
                materialize(&mut world, order[1], FINAL_RESIDUAL);
            } else {
                let due = expected[order[1]] - world.receipt(order[1]).paid_effective;
                assert_eq!(step(&mut world, order[1], true), Some(due));
            }

            for _ in 0..16 {
                for actor in [2, order[1], order[0], 1, 3] {
                    if !resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                        step(&mut world, actor, false);
                    }
                }
                if world
                    .actors
                    .iter()
                    .all(|actor| resolved_portfolio_is_terminal(&world.env, actor.portfolio))
                {
                    break;
                }
            }
            let payouts: [u128; 5] = std::array::from_fn(|actor| {
                assert!(resolved_portfolio_is_terminal(
                    &world.env,
                    world.actors[actor].portfolio
                ));
                world.env.token_amount(world.actors[actor].token) as u128
            });
            assert_eq!(
                payouts,
                std::array::from_fn(|actor| CAPITAL[actor] + expected[actor])
            );
            for actor in [2, order[1], order[0], 1, 3] {
                let actor = &world.actors[actor];
                let cu = world
                    .env
                    .close_portfolio_with_cu(&actor.owner, actor.portfolio);
                assert_cu_within("INV-066 terminal portfolio close", cu, CUSTODY_CU_LIMIT);
                peak_cu = peak_cu.max(cu);
                world.custody();
            }
            let group = world.env.market_state().1;
            assert_eq!(group.materialized_portfolio_count, 0);
            let stocks = [
                group.c_tot,
                group.insurance,
                group.pnl_pos_tot,
                group.source_claim_bound_total_num,
                group.backing_provider_earnings_total,
                group.source_insurance_credit_reserved_total_atoms,
            ];
            assert_eq!(stocks, [0; 6]);
            assert_eq!(group.vault, residue);
            assert_eq!(world.env.token_amount(world.provider_token), 1);
            let economics = (
                payouts,
                stocks,
                group.vault,
                world.env.token_amount(world.env.vault),
                group.resolved_payout_ledger,
                group.source_credit,
                group.source_backing_buckets,
                group.insurance_credit_reservations,
            );
            if let Some(expected) = &baseline {
                assert_eq!(
                    &economics, expected,
                    "receipt timing/order changed terminal economics: deferred={defer_second_receipt}, order={order:?}"
                );
            } else {
                baseline = Some(economics);
            }
            peak_cu = peak_cu.max(world.peak_cu);
        }
    }
    println!("INV-066: 4 public receipt-materialization schedules; exact residue {residue}; peak suffix CU {peak_cu}");
}

fn close_resolved_until_terminal(
    env: &mut V16CuEnv,
    owner: &Keypair,
    portfolio: Pubkey,
    label: &str,
) -> u64 {
    let mut paid = 0u64;
    for _ in 0..32 {
        if resolved_portfolio_is_terminal(env, portfolio) {
            return paid;
        }
        let (destination, cu) = env.close_resolved_with_cu(owner, portfolio);
        assert_cu_within(label, cu, CUSTODY_CU_LIMIT);
        paid = paid
            .checked_add(env.token_amount(destination))
            .expect("resolved payout total overflow");
        if resolved_portfolio_is_terminal(env, portfolio) {
            return paid;
        }
    }
    panic!("{label} did not reach a terminal portfolio in 32 bounded calls");
}

#[test]
fn v16_attack_resolved_close_order_preserves_scarce_source_backing() {
    fn run(reverse: bool) -> ([u128; 4], u128, u128, u128, u128, u128) {
        const OPEN_PRICE: u64 = 100;
        const FROZEN_PRICE: u64 = 300;
        const BACKING: u128 = 50;

        let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
        env.configure_permissionless_resolve_with_cu(100, 1);
        env.svm.warp_to_slot(1);
        env.configure_auth_mark_for_asset_as_admin(0, 1, OPEN_PRICE);
        env.top_up_backing_bucket(1, BACKING, 100);

        let owners = [
            Keypair::new(),
            Keypair::new(),
            Keypair::new(),
            Keypair::new(),
        ];
        let portfolios = owners.each_ref().map(|owner| env.create_portfolio(owner));
        for i in 0..4 {
            env.deposit(&owners[i], portfolios[i], 150);
        }
        env.trade_asset_with_cu(
            0,
            &owners[0],
            portfolios[0],
            &owners[2],
            portfolios[2],
            POS_SCALE as i128,
            OPEN_PRICE,
            0,
        );
        env.trade_asset_with_cu(
            0,
            &owners[1],
            portfolios[1],
            &owners[3],
            portfolios[3],
            POS_SCALE as i128,
            OPEN_PRICE,
            0,
        );

        // Accrue the market through a flat account so neither winner is settled
        // before the permissionless force-close ordering under test.
        let accrual_owner = Keypair::new();
        let accrual = env.create_portfolio(&accrual_owner);
        for slot in 2..=3 {
            env.svm.warp_to_slot(slot);
            env.push_auth_mark_for_asset_as_admin(0, slot, FROZEN_PRICE);
            env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(accrual, false),
                ],
                &[],
            )
            .expect("flat-account observation must accrue the market");
        }
        assert_eq!(env.market_state().1.assets[0].effective_price, FROZEN_PRICE);
        env.close_portfolio_with_cu(&accrual_owner, accrual);

        let winner_order = if reverse { [1usize, 0] } else { [0usize, 1] };
        for i in winner_order {
            env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: 3,
                    observations: vec![],
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                ],
                &[],
            )
            .expect("winner refresh must register its source-backed claim");
        }
        let source_before_resolve = env.market_state().1.source_credit[1];
        assert!(
            source_before_resolve.positive_claim_bound_num > BACKING * BOUND_SCALE,
            "the two winners must compete for undercollateralized backing"
        );
        assert!(
            source_before_resolve.credit_rate_num < percolator::CREDIT_RATE_SCALE,
            "the source-credit rate must reflect scarce backing"
        );

        env.resolve();
        env.svm.warp_to_slot(4);
        let terminal = |env: &V16CuEnv, portfolio: Pubkey| {
            let state = env.portfolio_state(portfolio);
            let receipt = resolved_receipt(&state);
            state.capital.get() == 0
                && state.pnl.get() == 0
                && percolator::active_bitmap_is_empty(active_bitmap(&state))
                && (!receipt.present || receipt.finalized)
        };
        let mut payouts = [0u128; 4];
        let close_order = if reverse {
            [2usize, 3, 1, 0]
        } else {
            [2usize, 3, 0, 1]
        };
        for _ in 0..32 {
            for i in close_order {
                if terminal(&env, portfolios[i]) {
                    continue;
                }
                let destination = env.close_resolved(&owners[i], portfolios[i]);
                payouts[i] += env.token_amount(destination) as u128;
            }
            if portfolios
                .iter()
                .all(|portfolio| terminal(&env, *portfolio))
            {
                break;
            }
        }
        assert!(
            portfolios
                .iter()
                .all(|portfolio| terminal(&env, *portfolio)),
            "honest round-robin resolved settlement must terminate"
        );
        let terminal = env.market_state().1;
        assert!(
            terminal.source_backing_buckets[1].consumed_liened_backing_num != 0
                || terminal.source_credit[1].provider_receivable_num != 0,
            "the differential must consume scarce source backing"
        );
        (
            payouts,
            terminal.vault,
            terminal.source_credit[1].positive_claim_bound_num,
            terminal.source_credit[1].provider_receivable_num,
            terminal.source_backing_buckets[1].consumed_liened_backing_num,
            terminal.source_backing_buckets[1].fresh_unliened_backing_num,
        )
    }

    let forward = run(false);
    let reverse = run(true);
    assert_eq!(
        reverse, forward,
        "permissionless resolved-close order allocated scarce source backing between users"
    );
    assert_eq!(forward.0, [300, 300, 0, 0]);
    assert_eq!(forward.1, 50, "provider backing remains canonical custody");
    assert_eq!(
        forward.5,
        50 * BOUND_SCALE,
        "neither claimant priority may consume the provider's fresh principal"
    );
}

#[test]
fn v16_bpf_force_close_pair_order_preserves_terminal_user_payouts() {
    fn run(cross_pair: bool) -> ([(u128, i128); 4], [u64; 4], u128, u128, u128, u128, u64) {
        let mut env = V16CuEnv::new();
        let cranker = Keypair::new();
        env.configure_permissionless_resolve_with_cu(100, 1);
        env.svm.warp_to_slot(1);
        env.configure_auth_mark_with_cu(1, 100);

        let owners = [
            Keypair::new(),
            Keypair::new(),
            Keypair::new(),
            Keypair::new(),
        ];
        let portfolios = [
            env.create_portfolio(&owners[0]),
            env.create_portfolio(&owners[1]),
            env.create_portfolio(&owners[2]),
            env.create_portfolio(&owners[3]),
        ];
        for i in 0..4 {
            env.deposit(&owners[i], portfolios[i], 100_000);
        }

        // The first pair enters at 100. The authenticated mark then advances to
        // 200 before the second pair enters, leaving one old winner and one old
        // loser when the asset freezes at 200.
        env.trade_asset_with_cu(
            0,
            &owners[0],
            portfolios[0],
            &owners[1],
            portfolios[1],
            POS_SCALE as i128,
            100,
            0,
        );
        env.svm.warp_to_slot(2);
        env.push_auth_mark_with_cu(2, 200);
        env.crank(
            portfolios[0],
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(0),
            },
        );
        env.crank(
            portfolios[1],
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: vec![],
            },
        );
        assert_eq!(env.market_state().1.assets[0].effective_price, 200);
        env.trade_asset_with_cu(
            0,
            &owners[2],
            portfolios[2],
            &owners[3],
            portfolios[3],
            POS_SCALE as i128,
            200,
            0,
        );

        env.svm.warp_to_slot(3);
        env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_SHUTDOWN, 0, 3, 0);
        env.svm.warp_to_slot(4);

        if cross_pair {
            env.force_close_abandoned_asset_with_cu(
                &cranker,
                portfolios[0],
                portfolios[3],
                0,
                4,
                POS_SCALE,
            );
            env.force_close_abandoned_asset_with_cu(
                &cranker,
                portfolios[2],
                portfolios[1],
                0,
                4,
                POS_SCALE,
            );
        } else {
            env.force_close_abandoned_asset_with_cu(
                &cranker,
                portfolios[0],
                portfolios[1],
                0,
                4,
                POS_SCALE,
            );
            env.force_close_abandoned_asset_with_cu(
                &cranker,
                portfolios[2],
                portfolios[3],
                0,
                4,
                POS_SCALE,
            );
        }

        let accounts = portfolios.map(|portfolio| {
            let state = env.portfolio_state(portfolio);
            assert!(percolator::active_bitmap_is_empty(
                state.active_bitmap.map(|word| word.get())
            ));
            (state.capital.get(), state.pnl.get())
        });
        assert_eq!(
            accounts,
            [(100_000, 100), (99_900, 0), (100_000, 0), (100_000, 0),],
            "the probe must carry real realized PnL before comparing pair order"
        );
        // A flat winner's certificate can be invalidated by a later force-close
        // and cannot refresh against a Recovery asset. The configured bounded
        // permissionless market resolution must still provide an owner-independent
        // terminal exit.
        env.resolve_stale_permissionless_with_cu(103);
        env.svm.warp_to_slot(104);
        let mut payouts = [0u64; 4];
        for i in 0..4 {
            payouts[i] = close_resolved_until_terminal(
                &mut env,
                &owners[i],
                portfolios[i],
                "force-close pair-order terminal settlement",
            );
        }
        assert_eq!(payouts, [100_100, 99_900, 100_000, 100_000]);
        let (_, group) = env.market_state();
        assert_eq!(group.assets[0].oi_eff_long_q, 0);
        assert_eq!(group.assets[0].oi_eff_short_q, 0);
        (
            accounts,
            payouts,
            group.insurance,
            group.vault,
            group.insurance_domain_spent[0],
            group.insurance_domain_spent[1],
            env.token_amount(env.vault),
        )
    }

    let direct = run(false);
    let crossed = run(true);
    assert_eq!(
        crossed, direct,
        "a permissionless cranker must not allocate value by choosing force-close pairs"
    );
}

#[test]
fn v16_attack_resolved_two_public_winners_are_close_order_independent() {
    fn run(winner_attempts_first: bool) -> (u128, u128, u128) {
        let mut env = V16CuEnv::new_with_init_params(production_risk_params());
        env.configure_auth_mark_with_cu(0, 1_000_000);

        let winner_a_owner = Keypair::new();
        let winner_a = env.create_portfolio(&winner_a_owner);
        let winner_b_owner = Keypair::new();
        let winner_b = env.create_portfolio(&winner_b_owner);
        let loser_owner = Keypair::new();
        let loser = env.create_portfolio(&loser_owner);
        let accrual_owner = Keypair::new();
        let accrual = env.create_portfolio(&accrual_owner);
        for (owner, portfolio) in [
            (&winner_a_owner, winner_a),
            (&winner_b_owner, winner_b),
            (&loser_owner, loser),
        ] {
            env.deposit(owner, portfolio, 1_000_000);
        }
        env.trade_asset_with_cu(
            0,
            &winner_a_owner,
            winner_a,
            &loser_owner,
            loser,
            (POS_SCALE / 2) as i128,
            1_000_000,
            0,
        );
        env.trade_asset_with_cu(
            0,
            &winner_b_owner,
            winner_b,
            &loser_owner,
            loser,
            (POS_SCALE / 2) as i128,
            1_000_000,
            0,
        );

        // Reach +10% through the production 24-bps/slot circuit breaker without giving either
        // winner a different funding-settlement cadence.
        for slot in 1..=50u64 {
            env.svm.warp_to_slot(slot);
            env.push_auth_mark_with_cu(slot, 1_100_000);
            env.crank(
                accrual,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
            );
        }
        assert_eq!(env.market_state().1.assets[0].effective_price, 1_100_000);
        env.close_portfolio_with_cu(&accrual_owner, accrual);
        let mut settlement_steps = 0usize;
        for _ in 0..2 {
            for portfolio in [winner_a, winner_b, loser] {
                settlement_steps += usize::from(
                    env.crank_if_actionable(
                        portfolio,
                        ProgInstruction::PermissionlessCrank {
                            now_slot: 50,
                            observations: crank_observations(0),
                        },
                    )
                    .is_some(),
                );
            }
        }
        assert!(
            settlement_steps >= 3,
            "each funded portfolio must make public settlement progress"
        );
        for (owner, winner) in [(&winner_a_owner, winner_a), (&winner_b_owner, winner_b)] {
            env.trade_asset_with_cu(
                0,
                owner,
                winner,
                &loser_owner,
                loser,
                -((POS_SCALE / 2) as i128),
                1_100_000,
                0,
            );
        }
        for portfolio in [winner_a, winner_b, loser] {
            assert!(percolator::active_bitmap_is_empty(active_bitmap(
                &env.portfolio_state(portfolio)
            )));
        }
        env.resolve();

        let (winner_a_paid, winner_b_paid, loser_paid) = if winner_attempts_first {
            let paid = drain_resolved_cohort(
                &mut env,
                &[
                    (&winner_a_owner, winner_a),
                    (&winner_b_owner, winner_b),
                    (&loser_owner, loser),
                ],
                "winner-first three-party settlement",
            );
            (paid[0], paid[1], paid[2])
        } else {
            let paid = drain_resolved_cohort(
                &mut env,
                &[
                    (&loser_owner, loser),
                    (&winner_b_owner, winner_b),
                    (&winner_a_owner, winner_a),
                ],
                "loser-first three-party settlement",
            );
            (paid[2], paid[1], paid[0])
        };

        for (owner, portfolio) in [
            (&winner_a_owner, winner_a),
            (&winner_b_owner, winner_b),
            (&loser_owner, loser),
        ] {
            env.close_portfolio_with_cu(owner, portfolio);
        }
        let (_, group) = env.market_state();
        assert_eq!(group.materialized_portfolio_count, 0);
        assert_eq!(group.c_tot, 0);
        assert_eq!(group.vault, 0);
        (winner_a_paid, winner_b_paid, loser_paid)
    }

    let loser_first = run(false);
    let premature_winner_first = run(true);
    assert_eq!(premature_winner_first, loser_first);
    assert_eq!(loser_first.0, loser_first.1);
    assert_eq!(loser_first.0 + loser_first.1 + loser_first.2, 3_000_000);
    assert!(loser_first.0 > 1_000_000 && loser_first.2 < 1_000_000);
}

#[test]
fn v16_bpf_force_close_pair_order_preserves_unequal_partial_payouts() {
    fn run(cross_pair: bool) -> ([(u128, i128); 4], [u64; 4], u128, u128, u128, u128, u64) {
        let mut env = V16CuEnv::new();
        let cranker = Keypair::new();
        env.configure_permissionless_resolve_with_cu(100, 1);
        env.svm.warp_to_slot(1);
        env.configure_auth_mark_with_cu(1, 100);

        let owners = [
            Keypair::new(),
            Keypair::new(),
            Keypair::new(),
            Keypair::new(),
        ];
        let portfolios = [
            env.create_portfolio(&owners[0]),
            env.create_portfolio(&owners[1]),
            env.create_portfolio(&owners[2]),
            env.create_portfolio(&owners[3]),
        ];
        for (i, amount) in [1_000, 1_000, 2_000, 2_000].into_iter().enumerate() {
            env.deposit(&owners[i], portfolios[i], amount);
        }

        env.trade_asset_with_cu(
            0,
            &owners[0],
            portfolios[0],
            &owners[1],
            portfolios[1],
            POS_SCALE as i128,
            100,
            0,
        );
        env.trade_asset_with_cu(
            0,
            &owners[2],
            portfolios[2],
            &owners[3],
            portfolios[3],
            (2 * POS_SCALE) as i128,
            100,
            0,
        );
        for (slot, mark) in [(2, 200), (3, 300)] {
            env.svm.warp_to_slot(slot);
            env.push_auth_mark_with_cu(slot, mark);
            for portfolio in portfolios {
                env.crank(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations(0),
                    },
                );
            }
        }

        env.svm.warp_to_slot(4);
        env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_SHUTDOWN, 0, 4, 0);
        env.svm.warp_to_slot(5);
        if cross_pair {
            for (long, short) in [
                (portfolios[0], portfolios[3]),
                (portfolios[2], portfolios[1]),
                (portfolios[2], portfolios[3]),
            ] {
                env.force_close_abandoned_asset_with_cu(&cranker, long, short, 0, 5, POS_SCALE);
            }
        } else {
            env.force_close_abandoned_asset_with_cu(
                &cranker,
                portfolios[0],
                portfolios[1],
                0,
                5,
                POS_SCALE,
            );
            env.force_close_abandoned_asset_with_cu(
                &cranker,
                portfolios[2],
                portfolios[3],
                0,
                5,
                2 * POS_SCALE,
            );
        }

        let accounts = portfolios.map(|portfolio| {
            let state = env.portfolio_state(portfolio);
            assert!(percolator::active_bitmap_is_empty(
                state.active_bitmap.map(|word| word.get())
            ));
            (state.capital.get(), state.pnl.get())
        });
        assert_eq!(
            accounts,
            [(1_000, 200), (800, 0), (2_000, 400), (1_600, 0)],
            "unequal force-closes must carry nonzero realized PnL before the order comparison"
        );

        env.svm.warp_to_slot(104);
        env.resolve_stale_permissionless_with_cu(104);
        env.svm.warp_to_slot(105);
        let mut payouts = [0u64; 4];
        for i in 0..4 {
            payouts[i] = close_resolved_until_terminal(
                &mut env,
                &owners[i],
                portfolios[i],
                "unequal force-close terminal settlement",
            );
        }
        assert_eq!(payouts, [1_200, 800, 2_400, 1_600]);
        let (_, group) = env.market_state();
        (
            accounts,
            payouts,
            group.vault,
            group.insurance,
            group.assets[0].oi_eff_long_q,
            group.assets[0].oi_eff_short_q,
            env.token_amount(env.vault),
        )
    }

    let direct = run(false);
    let crossed = run(true);
    assert_eq!(
        crossed, direct,
        "a cranker allocated value by choosing an unequal partial force-close schedule"
    );
}

// security.md sweep — resolved wind-down LoF / over-claim (#22/#30/#48): a market can be resolved
// with OPEN positions (handle_resolve_market does not require flat). After resolution a long and a
// short must each recover their FAIR value via CloseResolved — neither stuck (LoF) nor able to
// over-claim. Total tokens paid out must never exceed total deposited.
#[test]
fn v16_regression_resolved_open_positions_recover_fairly_order_robust() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 1_000, 500);
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &lo_owner,
        lo,
        &sh_owner,
        sh,
        (10_000 * POS_SCALE) as i128,
        100,
        0,
    ); // notional 1M
       // move price so the long wins, settle both legs across two slots, THEN resolve with positions still open.
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    for slot in [10u64, 11] {
        env.svm.warp_to_slot(slot);
        for p in [sh, lo] {
            let _ = env.send_crank_if_actionable(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(p, false),
                ],
                &[],
            );
        }
    }
    env.resolve(); // resolve WITH open positions still on the book

    let bal = |env: &V16CuEnv, k: &Pubkey| -> u128 {
        let d = env.svm.get_account(k).unwrap().data;
        u64::from_le_bytes(d[64..72].try_into().unwrap()) as u128
    };
    // Winner (long) closes FIRST, before the loser has funded the vault. This may perform bounded
    // terminal cleanup, but it must defer payout and preserve the winner's capital and PnL exactly.
    let (dest_lo1, _) = env.close_resolved_with_cu(&lo_owner, lo);
    assert_eq!(
        bal(&env, &dest_lo1),
        0,
        "premature winner close pays nothing (vault not yet funded)"
    );
    let mid = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    assert_eq!(
        mid.capital.get(),
        1_000_000,
        "premature winner close preserves capital while payout is deferred"
    );
    assert_eq!(
        mid.pnl.get(),
        100_000,
        "premature winner close preserves parked pnl"
    );

    // Loser closes (recovers its post-loss capital, funding the vault for the winner).
    let (dest_sh, _) = env.close_resolved_with_cu(&sh_owner, sh);
    let out_sh = bal(&env, &dest_sh);
    // Winner RETRIES and now recovers full fair value.
    let (dest_lo2, _) = env.close_resolved_with_cu(&lo_owner, lo);
    let out_lo = bal(&env, &dest_lo2);

    // No LoF, exact value conservation, fair winner/loser split.
    assert_eq!(
        out_lo + out_sh,
        2_000_000,
        "every account recovers; total payout == total deposited (no LoF, no printing)"
    );
    assert_eq!(
        out_lo, 1_100_000,
        "winner recovers capital + realized profit"
    );
    assert_eq!(out_sh, 900_000, "loser recovers capital - realized loss");
    let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    assert_eq!(a.capital.get(), 0, "long fully wound down");
    assert_eq!(b.capital.get(), 0, "short fully wound down");
    // The winner's positive pnl is source-backed (the directional trade created source credit),
    // so at resolved close it is REALIZED into capital and paid as capital — no junior payout
    // receipt is parked. The account winds down completely (pnl 0, capital 0, no receipt).
    assert!(
        !resolved_receipt(&a).present,
        "winner fully paid via capital realization, no dangling receipt"
    );
    assert_eq!(a.pnl.get(), 0, "winner pnl fully realized");
    let (_, g) = env.market_state();
    assert_eq!(g.vault, 0, "vault fully drained, no funds stranded");
}

// security.md sweep — haircut payout rounding across multiple winners (#33/#37): when several
// resolved winners share ONE insufficient backing pool, each is paid floor(face * rate). The sum of
// floored payouts must NEVER exceed the backing (a rounding-up bug would let winners collectively
// extract more than the pool holds). Probe with deliberately non-divisible faces.
#[test]
fn v16_regression_resolved_multiwinner_haircut_no_overpay_no_strand() {
    const BACKING: u128 = 100;
    // three winners with non-divisible positive-pnl faces against a shared 100 backing.
    let faces: [u128; 3] = [250, 251, 253];
    let mut env = V16CuEnv::new();
    env.top_up_backing_bucket(1, BACKING, 10_000);
    let mut owners = Vec::new();
    let mut ports = Vec::new();
    for &face in faces.iter() {
        let o = Keypair::new();
        let p = env.create_portfolio(&o);
        env.deposit(&o, p, 1_000);
        env.add_source_positive_pnl(p, 1, face);
        owners.push(o);
        ports.push(p);
    }
    env.resolve();
    let actors: Vec<_> = owners.iter().zip(ports.iter().copied()).collect();
    let payouts = drain_resolved_cohort(
        &mut env,
        &actors,
        "three-winner nondivisible haircut settlement",
    );
    let total_out: u128 = payouts.iter().sum();
    let total_pnl_paid: u128 = payouts.iter().map(|paid| paid.saturating_sub(1_000)).sum();
    // CRUX 1: summed haircut pnl never exceeds the shared backing (no rounding-up over-pay).
    assert!(
        total_pnl_paid <= BACKING,
        "summed haircut pnl {} must not exceed backing {}",
        total_pnl_paid,
        BACKING
    );
    // CRUX 2 (no strand): every winner's receipt is closable and the portfolio dematerializes.
    for (o, p) in owners.iter().zip(ports.iter()) {
        let a = state::read_portfolio(&env.svm.get_account(p).unwrap().data).unwrap();
        assert_eq!(a.capital.get(), 0, "winner capital fully paid");
        assert!(
            !resolved_receipt(&a).present || resolved_receipt(&a).finalized,
            "receipt closable after retry"
        );
        env.close_portfolio_with_cu(o, *p); // panics if dematerialization is blocked
    }
    let (_, g) = env.market_state();
    assert_eq!(
        g.materialized_portfolio_count, 0,
        "all winners dematerialized — no permanent strand"
    );
    assert_eq!(g.c_tot, 0, "all capital wound down");
    assert!(
        g.vault <= 1,
        "at most conservative-rounding dust remains in vault (got {})",
        g.vault
    );
    assert!(
        total_out >= 3_000,
        "all senior capital recovered (no LoF on capital)"
    );
}

// security.md sweep — resolve mid-flight before settlement (#30 sequence/race): push a price move,
// then resolve WITHOUT any settlement crank. The resolved wind-down must still settle at the true
// post-move price — the winner recovers their gain, the loser bears their loss, value conserved.
// Attacker success = stale pre-move settlement (winner LoF, or loser escapes its loss).
#[test]
fn v16_regression_resolve_before_settlement_uses_official_price() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &lo_owner,
        lo,
        &sh_owner,
        sh,
        (10_000 * POS_SCALE) as i128,
        100,
        0,
    ); // notional 1M
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110); // pending mark; NOT yet accrued into effective_price (anti-retroactivity)
                                         // NO crank: resolve immediately. The pushed mark is unaccrued, so the official effective_price is
                                         // still 100 and the position is officially flat.
    let (_, g_pre) = env.market_state();
    assert_eq!(
        g_pre.assets[0].effective_price, 100,
        "unaccrued mark push does NOT move the official price"
    );
    env.resolve();

    fn bal(env: &V16CuEnv, k: &Pubkey) -> u128 {
        let d = env.svm.get_account(k).unwrap().data;
        u64::from_le_bytes(d[64..72].try_into().unwrap()) as u128
    }
    // loser-first, then winner (order-robust wind-down established in batch 23). Retry winner if deferred.
    let _ = env.close_resolved(&sh_owner, sh);
    let d1 = env.close_resolved(&lo_owner, lo);
    let mut won = bal(&env, &d1);
    if won == 0 {
        let d2 = env.close_resolved(&lo_owner, lo);
        won = bal(&env, &d2);
    }
    let lost = {
        let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
        assert_eq!(b.capital.get(), 0, "loser wound down");
        2_000_000u128.saturating_sub(won)
    };
    // CORRECT behavior: resolve settles at the OFFICIAL accrued price (100). The unaccrued mark push
    // is NOT retroactively applied, so no value is created or destroyed — each party recovers exactly
    // its deposit. (Contrast batch 23: crank-to-accrue BEFORE resolve, and the winner gets 1.1M.)
    assert_eq!(
        won, 1_000_000,
        "no value invented from an unaccrued mark — deposit returned"
    );
    assert_eq!(
        won + lost,
        2_000_000,
        "exact conservation across resolve-before-settlement"
    );
    let (_, g) = env.market_state();
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    assert_eq!(a.capital.get(), 0, "long fully wound down");
    assert!(
        resolved_receipt(&a).finalized || !resolved_receipt(&a).present,
        "receipt closable"
    );
}

#[derive(Clone, Copy)]
struct Inv066PayoutClass {
    class: &'static str,
    engine_proofs: &'static [&'static str],
    public_witnesses: &'static [(&'static str, &'static str)],
}

fn inv066_source_defines_function(source: &str, function: &str) -> bool {
    let marker = format!("fn {function}");
    source.lines().any(|line| {
        line.trim()
            .strip_prefix(&marker)
            .is_some_and(|tail| tail.trim_start().starts_with('('))
    })
}

fn inv066_handler_body<'a>(production: &'a str, function: &str) -> &'a str {
    let start = production
        .find(&format!("fn {function}"))
        .unwrap_or_else(|| panic!("missing production handler {function}"));
    let tail = &production[start..];
    let end = tail[1..]
        .find("\n    #[inline(never)]")
        .map_or(tail.len(), |offset| offset + 1);
    &tail[..end]
}

#[test]
fn v16_program_resolved_payout_induction_composition_is_source_complete() {
    const ENGINE_PIN: &str = "394fd0bf2cb7d73df425eb3754dc3be1a0c44336";
    const CLASSES: &[Inv066PayoutClass] = &[
        Inv066PayoutClass {
            class: "snapshot-bound receipt materialization",
            engine_proofs: &["proof_v16_resolved_receipt_bound_migration_is_exact_or_fails_closed"],
            public_witnesses: &[(
                "tests/invariants/stateful/inv_066_resolved_payout_fairness_and_order_independence.rs",
                "v16_program_full_terminal_lifecycle_is_claimant_order_independent",
            )],
        },
        Inv066PayoutClass {
            class: "rate-derived entitlement and claimant ordering",
            engine_proofs: &[
                "proof_v16_resolved_receipt_claimable_is_rate_monotone_and_overpaid_fails_closed",
                "proof_v16_two_resolved_receipts_are_order_independent_when_snapshot_funded",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/stateful/inv_066_resolved_payout_fairness_and_order_independence.rs",
                    "v16_program_four_partial_receipts_exhaust_claim_and_release_orders",
                ),
                (
                    "tests/invariants/cu/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs",
                    "v16_attack_haircut_rounding_many_winners_no_mint",
                ),
            ],
        },
        Inv066PayoutClass {
            class: "bounded payout step and attributed custody",
            engine_proofs: &[
                "contract_check_kernel_resolved_payout_step",
                "proof_v16_public_resolved_payout_topup_pays_min_claimable_and_vault",
                "proof_v16_resolved_external_payout_requires_exact_capital_plus_claim_sources",
            ],
            public_witnesses: &[(
                "tests/invariants/stateful/inv_068_receipt_uniqueness_and_monotonic_topups.rs",
                "v16_program_resolved_receipt_accepts_two_exact_topups_and_idempotent_retries",
            )],
        },
        Inv066PayoutClass {
            class: "exact-once terminal receipt disposition",
            engine_proofs: &[
                "proof_v16_resolved_receipt_payment_cannot_exceed_terminal_claim",
                "proof_v16_terminal_resolved_receipt_core_restores_dematerialization",
                "proof_v16_insolvent_resolved_receipt_clears_at_terminal_rate",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/cu/inv_068_receipt_uniqueness_and_monotonic_topups.rs",
                    "v16_program_resolved_receipt_replays_extract_no_value_on_any_public_rail",
                ),
                (
                    "tests/invariants/cu/inv_070_zero_unattributed_terminal_residue_and_close_slab.rs",
                    "v16_program_terminal_stock_and_close_slab_composition_is_source_complete",
                ),
            ],
        },
    ];

    let cargo = include_str!("../../../Cargo.toml");
    let lock = include_str!("../../../Cargo.lock");
    assert_eq!(
        cargo.matches(&format!("rev = \"{ENGINE_PIN}\"")).count(),
        2,
        "INV-066/067 proof composition must be reviewed on every engine pin change",
    );
    assert!(
        lock.contains(&format!("rev={ENGINE_PIN}#{ENGINE_PIN}")),
        "Cargo.lock must resolve the payout-certified engine revision",
    );

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut classes = std::collections::BTreeSet::new();
    let mut proofs = std::collections::BTreeSet::new();
    let mut source_cache = std::collections::BTreeMap::<&str, String>::new();
    for row in CLASSES {
        assert!(classes.insert(row.class), "duplicate payout class");
        assert!(!row.engine_proofs.is_empty());
        assert!(!row.public_witnesses.is_empty());
        for proof in row.engine_proofs {
            assert!(proofs.insert(*proof), "duplicate engine proof {proof}");
            assert!(
                proof.starts_with("contract_check_") || proof.starts_with("proof_v16_"),
                "unclassified payout proof {proof}",
            );
        }
        for (path, witness) in row.public_witnesses {
            let source = source_cache.entry(path).or_insert_with(|| {
                std::fs::read_to_string(root.join(path))
                    .unwrap_or_else(|error| panic!("read {path}: {error}"))
            });
            assert!(
                inv066_source_defines_function(source, witness),
                "payout class '{}' lacks executable witness {path}#{witness}",
                row.class,
            );
        }
    }
    assert_eq!(classes.len(), 4, "payout class roster drift");
    assert_eq!(proofs.len(), 9, "payout engine-proof roster drift");

    let induction = include_str!("../kani/inv_066_resolved_payout_fairness_and_exact_once.rs");
    assert!(induction.contains("RESOLVED_RATE_SUM_AXIOM"));
    assert!(inv066_source_defines_function(
        induction,
        "kani_inv066_inv067_funded_receipt_induction_is_order_independent_and_exact_once",
    ));
    for consequence in [
        "assert_eq!(first_paid_a, first_due)",
        "assert_eq!(first_paid_b, first_due)",
        "assert_eq!(second_paid_a, second_due)",
        "assert_eq!(second_paid_b, second_due)",
        "assert_eq!(vault_after_a, vault_after_b)",
        "assert_eq!(first_paid_after, first_face)",
    ] {
        assert!(
            induction.contains(consequence),
            "claimant induction lost consequence {consequence}",
        );
    }

    let production = include_str!("../../../src/v16_program.rs");
    let production = production
        .split("    #[cfg(test)]\n    mod tests")
        .next()
        .expect("production prefix exists");
    let close = inv066_handler_body(production, "handle_close_resolved");
    let topup = inv066_handler_body(production, "handle_claim_resolved_payout_topup");
    for (handler, engine_call) in [
        (close, "permissionless_auto_crank_not_atomic"),
        (topup, "claim_resolved_payout_topup_not_atomic"),
    ] {
        let owner_gate = handler.find("expect_portfolio_view_owner").unwrap();
        let engine = handler.find(engine_call).unwrap();
        let custody = handler.find("verify_withdrawable_token_accounts").unwrap();
        let transfer = handler.find("transfer_tokens_signed").unwrap();
        assert!(
            owner_gate < engine && engine < custody && custody < transfer,
            "resolved payout route must retain owner binding -> engine -> custody -> SPL ordering",
        );
    }
    assert!(close.contains("AutoCrankPlanV16::CloseResolved"));
    assert!(close.contains("AutoCrankOutcomeV16::ResolvedClose"));
    assert_eq!(
        production
            .matches("claim_resolved_payout_topup_not_atomic")
            .count(),
        1,
        "a new direct top-up route requires composition evidence",
    );
}
