//! INV-039: a bankrupt portfolio's recreation cannot discard its cohort's loss.
//!
//! A public matched reduction creates an outstanding close residual and a flat weight holder.
//! A bystander's withdrawal followed by debtor close/rent refill/same-address InitPortfolio
//! rejects atomically until the residual is booked. Recreation then succeeds BEFORE the holder
//! settles B: its old obligation must outlive the debtor's erased close ledger. This differs
//! from the Recovery, collateral-transfer, partial-liquidation and resolved-detach witnesses.
//! Two mirrored, fee/funding-free worlds check the close partition, exact B debit, senior
//! payouts, claim-gate retries and terminal deletion. This is bounded evidence relevant to the
//! still-refuted counterexample 419, not a claim to reproduce or close that counterexample.

use super::*;
use solana_sdk::{instruction::InstructionError, rent::Rent, transaction::TransactionError};

#[path = "inv_039_pending_loss_close_preemption.rs"]
mod close_preemption;

#[path = "inv_039_pending_loss_cure_resolution.rs"]
mod cure_resolution;

fn pending_bankruptcy(reverse_sides: bool, peak_crank_cu: &mut u64) -> (AttributionWorld, u128) {
    let world = AttributionWorld::new_with_params(
        reverse_sides,
        V16CuMarketParams {
            max_portfolio_assets: 3,
            initial_price: 1_000_000,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_bankrupt_close_lifetime_slots: 1_000,
            ..V16CuMarketParams::default()
        },
    );
    let mut mark = 1_000_000u64;
    let marks = (1..=20).map(move |_| {
        mark = mark * if reverse_sides { 9_500 } else { 10_500 } / 10_000;
        mark
    });
    enter_pending_bankruptcy(world, marks, peak_crank_cu)
}

fn enter_pending_bankruptcy(
    mut world: AttributionWorld,
    marks: impl IntoIterator<Item = u64>,
    peak_crank_cu: &mut u64,
) -> (AttributionWorld, u128) {
    let q = world.quantities[0];
    let cu = world.env.trade_asset_with_cu(
        1,
        &world.actors[0].owner,
        world.actors[0].portfolio,
        &world.actors[1].owner,
        world.actors[1].portfolio,
        q,
        1_000_000,
        0,
    );
    assert_cu_within("INV-039 close/reopen initial trade", cu, TRADE_CU_LIMIT);
    let mut mark = 1_000_000u64;
    for (index, next_mark) in marks.into_iter().enumerate() {
        let slot = index as u64 + 1;
        mark = next_mark;
        world.env.svm.warp_to_slot(slot);
        world.env.push_auth_mark_for_asset_as_admin(1, slot, mark);
        let cu = world.env.crank(
            world.actors[4].portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(1),
            },
        );
        *peak_crank_cu = (*peak_crank_cu).max(cu);
        assert_cu_within(
            "INV-039 close/reopen authenticated accrual",
            cu,
            CRANK_CU_LIMIT,
        );
    }
    assert_eq!(world.env.market_state().1.assets[1].effective_price, mark);
    let gain = (i128::from(mark) - 1_000_000).unsigned_abs();
    assert!(gain > ATTRIBUTION_DEPOSITS[1]);
    let cu = world.env.trade_asset_with_cu(
        1,
        &world.actors[0].owner,
        world.actors[0].portfolio,
        &world.actors[1].owner,
        world.actors[1].portfolio,
        -q,
        mark,
        0,
    );
    assert_cu_within(
        "INV-039 matched reduction creates close residual",
        cu,
        TRADE_CU_LIMIT,
    );
    (world, gain)
}

fn close_instruction(world: &AttributionWorld, actor: usize) -> Instruction {
    let a = &world.actors[actor];
    Instruction {
        program_id: world.env.program_id,
        data: world.env.close_portfolio_ix(a.portfolio).encode(),
        accounts: vec![
            AccountMeta::new(a.owner.pubkey(), true),
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(a.portfolio, false),
        ],
    }
}

fn payout_and_recreate(world: &AttributionWorld) -> Vec<Instruction> {
    let env = &world.env;
    let debtor = &world.actors[1];
    let bystander = &world.actors[4];
    vec![
        Instruction {
            program_id: env.program_id,
            data: env
                .withdraw_ix(bystander.portfolio, ATTRIBUTION_DEPOSITS[4])
                .encode(),
            accounts: vec![
                AccountMeta::new(bystander.owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(bystander.portfolio, false),
                AccountMeta::new(bystander.token, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        },
        close_instruction(world, 1),
        // ClosePortfolio retains program ownership inside this transaction. Public rent
        // replenishment lets InitPortfolio reallocate the same address without a byte writer.
        system_instruction::transfer(
            &debtor.owner.pubkey(),
            &debtor.portfolio,
            env.svm
                .get_sysvar::<Rent>()
                .minimum_balance(env.portfolio_account_len),
        ),
        Instruction {
            program_id: env.program_id,
            data: ProgInstruction::InitPortfolio.encode(),
            accounts: vec![
                AccountMeta::new(debtor.owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(debtor.portfolio, false),
            ],
        },
    ]
}

fn assert_close_partition(close: CloseProgressLedgerV16, residual: u128) {
    assert_eq!(close.gross_loss_at_close_start, residual);
    assert_eq!(close.drift_consumed, 0);
    assert_eq!(close.support_consumed, 0);
    assert_eq!(close.junior_face_burned, 0);
    assert_eq!(close.insurance_spent, 0);
    assert_eq!(close.explicit_loss_assigned, 0);
    assert_eq!(close.residual_remaining + close.b_loss_booked, residual);
}

fn assert_no_junior_claim_retry(world: &mut AttributionWorld, actor: usize) {
    // The sole remaining claim was fully source-backed and converted into senior capital.
    assert!(!world.env.market_state().1.payout_snapshot_captured);
    let a = &world.actors[actor];
    assert!(!resolved_receipt(&world.env.portfolio_state(a.portfolio)).present);
    let ix = Instruction {
        program_id: world.env.program_id,
        data: ProgInstruction::ClaimResolvedPayoutTopup.encode(),
        accounts: vec![
            AccountMeta::new_readonly(a.owner.pubkey(), false),
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(a.portfolio, false),
            AccountMeta::new(a.token, false),
            AccountMeta::new(world.env.vault, false),
            AccountMeta::new_readonly(world.env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
    };
    let before = world.frame();
    world.env.svm.expire_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), ix],
        Some(&world.env.payer.pubkey()),
        &[&world.env.payer],
        world.env.svm.latest_blockhash(),
    );
    let failure = world
        .env
        .svm
        .send_transaction(tx)
        .expect_err("no junior snapshot to claim twice");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(
            2,
            InstructionError::Custom(PercolatorError::EngineLockActive as u32),
        ),
    );
    assert_cu_within(
        "INV-039 no-junior-claim retry",
        failure.meta.compute_units_consumed,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        world.frame(),
        before,
        "no duplicate payout or resurrected old debt"
    );
}

#[test]
fn v16_program_pending_loss_survives_debtor_recreation_and_bystander_payout() {
    let mut peak_route_cu = 0;
    let mut peak_crank_cu = 0;
    let mut peak_payout_cu = 0;
    let mut recreate = |world: &mut AttributionWorld| {
        world.env.svm.expire_blockhash();
        let mut instructions = vec![heap_ix(), cu_ix()];
        instructions.extend(payout_and_recreate(world));
        let tx = Transaction::new_signed_with_payer(
            &instructions,
            Some(&world.env.payer.pubkey()),
            &[
                &world.env.payer,
                &world.actors[1].owner,
                &world.actors[4].owner,
            ],
            world.env.svm.latest_blockhash(),
        );
        let result = world.env.svm.send_transaction(tx);
        let meta = match &result {
            Ok(meta) => meta,
            Err(failure) => &failure.meta,
        };
        peak_route_cu = peak_route_cu.max(meta.compute_units_consumed);
        assert_cu_within(
            "INV-039 payout/close/refill/reinitialize transaction",
            meta.compute_units_consumed,
            // Three wrapper custody instructions share one transaction budget.
            3 * CUSTODY_CU_LIMIT,
        );
        result
    };

    for reverse_sides in [false, true] {
        let (mut world, gain) = pending_bankruptcy(reverse_sides, &mut peak_crank_cu);
        let holder = world.actors[0].portfolio;
        let debtor = world.actors[1].portfolio;
        let bystander = world.actors[4].portfolio;
        // The never-traded bystander retains the freshly initialized source-domain frame.
        let empty_sources = world.env.portfolio_state(bystander).source_domains;
        let residual = gain.checked_sub(ATTRIBUTION_DEPOSITS[1]).unwrap();
        assert!(residual > 0);
        world.check([0; 4], [true, false, false, false]);
        let original_holder = world.env.portfolio_state(holder);
        let original_leg = active_leg_for_asset(&original_holder, 1);
        assert_eq!(original_holder.capital.get(), ATTRIBUTION_DEPOSITS[0]);
        assert_eq!(original_holder.pnl.get(), gain as i128);
        assert_eq!(world.env.portfolio_state(debtor).capital.get(), 0);
        assert_eq!(
            world.env.portfolio_state(debtor).pnl.get(),
            -(residual as i128)
        );
        let pending_close = close_progress(&world.env.portfolio_state(debtor));
        assert!(pending_close.active && !pending_close.finalized && !pending_close.canceled);
        assert_eq!(pending_close.asset_index, 1);
        assert_eq!(pending_close.domain_side, original_leg.side);
        assert_close_partition(pending_close, residual);
        assert_eq!(pending_close.residual_remaining, residual);

        // Failure at the second economic instruction proves the valid SPL payout prefix ran.
        let before_reject = world.frame();
        let failure = recreate(&mut world).expect_err("an outstanding close is not disposable");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                3,
                InstructionError::Custom(PercolatorError::EngineLockActive as u32),
            )
        );
        assert_eq!(
            world.frame(),
            before_reject,
            "rollback includes payout, custody, rent and sequences"
        );

        // Book the residual into B, without touching the holder or paying its junior claim.
        let holder_before_booking = world.env.svm.get_account(&holder);
        for _ in 0..4 {
            if close_progress(&world.env.portfolio_state(debtor)).finalized {
                break;
            }
            let cu = world.env.crank(
                debtor,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 20,
                    observations: crank_observations(1),
                },
            );
            peak_crank_cu = peak_crank_cu.max(cu);
            assert_cu_within("INV-039 residual booking", cu, CRANK_CU_LIMIT);
            assert_close_partition(close_progress(&world.env.portfolio_state(debtor)), residual);
            world.check([0; 4], [true, false, false, false]);
        }
        let booked = close_progress(&world.env.portfolio_state(debtor));
        assert!(booked.finalized && !booked.canceled);
        assert_eq!(booked.residual_remaining, 0);
        assert_eq!(booked.b_loss_booked, residual);
        assert_eq!(world.env.portfolio_state(debtor).pnl.get(), 0);
        assert_eq!(world.env.svm.get_account(&holder), holder_before_booking);
        let booked_group = world.env.market_state().1;
        let target_b = if reverse_sides {
            booked_group.assets[1].b_short_num
        } else {
            booked_group.assets[1].b_long_num
        };
        assert!(
            target_b > original_leg.b_snap,
            "the retained holder still owes real B"
        );

        let stale_close = close_instruction(&world, 1);
        let old_id = world.env.portfolio_id(debtor);
        let next_id = world.env.market_state().0.next_portfolio_id;
        let before_recreate = world.frame();
        let old_rent = world.env.svm.get_account(&debtor).unwrap().lamports;
        let owner_lamports = world
            .env
            .svm
            .get_account(&world.actors[1].owner.pubkey())
            .unwrap()
            .lamports;
        let new_rent = world
            .env
            .svm
            .get_sysvar::<Rent>()
            .minimum_balance(world.env.portfolio_account_len);
        recreate(&mut world)
            .expect("booked loss survives debtor close and same-address recreation");
        world.check([0; 4], [true, false, false, false]);
        let recreated = world.env.portfolio_state(debtor);
        assert_ne!(world.env.portfolio_id(debtor), old_id);
        assert_eq!(world.env.portfolio_id(debtor), next_id);
        assert_eq!(recreated.capital.get(), 0);
        assert_eq!(recreated.pnl.get(), 0);
        assert_eq!(close_progress(&recreated), CloseProgressLedgerV16::EMPTY);
        assert_eq!(recreated.source_domains, empty_sources);
        assert_eq!(
            world.env.svm.get_account(&debtor).unwrap().lamports,
            new_rent
        );
        assert_eq!(
            world
                .env
                .svm
                .get_account(&world.actors[1].owner.pubkey())
                .unwrap()
                .lamports,
            owner_lamports - new_rent
        );
        assert_eq!(world.env.svm.get_account(&holder), holder_before_booking);
        let after_recreate = world.env.market_state().1;
        assert_eq!(after_recreate.assets, booked_group.assets);
        assert_eq!(after_recreate.source_credit, booked_group.source_credit);
        assert_eq!(after_recreate.materialized_portfolio_count, 5);
        assert_eq!(
            after_recreate.c_tot,
            booked_group.c_tot - ATTRIBUTION_DEPOSITS[4]
        );
        assert_eq!(
            world.env.token_amount(world.actors[4].token) as u128,
            ATTRIBUTION_DEPOSITS[4]
        );
        for (key, account) in before_recreate {
            if key == world.env.market {
                assert_eq!(
                    world.env.svm.get_account(&key).unwrap().lamports,
                    account.unwrap().lamports + old_rent
                );
            } else if ![
                debtor,
                world.actors[1].owner.pubkey(),
                bystander,
                world.actors[4].token,
                world.env.vault,
            ]
            .contains(&key)
            {
                assert_eq!(world.env.svm.get_account(&key), account);
            }
        }

        let before_stale = world.frame();
        world.env.svm.expire_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[heap_ix(), cu_ix(), stale_close],
            Some(&world.env.payer.pubkey()),
            &[&world.env.payer, &world.actors[1].owner],
            world.env.svm.latest_blockhash(),
        );
        let failure = world
            .env
            .svm
            .send_transaction(tx)
            .expect_err("old close cannot delete the new incarnation");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                2,
                InstructionError::Custom(PercolatorError::EngineStale as u32)
            )
        );
        assert_eq!(world.frame(), before_stale);

        // Erasing the finalized debtor ledger did not erase its original cohort's B debit.
        for _ in 0..4 {
            if !has_active_leg_for_asset(&world.env.portfolio_state(holder), 1) {
                break;
            }
            let cu = world.env.crank(
                holder,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 20,
                    observations: crank_observations(1),
                },
            );
            peak_crank_cu = peak_crank_cu.max(cu);
            assert_cu_within(
                "INV-039 B debit and obligation release after debtor recreation",
                cu,
                CRANK_CU_LIMIT,
            );
            let account = world.env.portfolio_state(holder);
            assert!((ATTRIBUTION_DEPOSITS[1] as i128..=gain as i128).contains(&account.pnl.get()));
            world.check(
                [0; 4],
                [has_active_leg_for_asset(&account, 1), false, false, false],
            );
        }
        world.check([0; 4], [false; 4]);
        let settled = world.env.portfolio_state(holder);
        assert_eq!(settled.capital, original_holder.capital);
        assert_eq!(
            original_holder.pnl.get() - settled.pnl.get(),
            residual as i128
        );
        assert_eq!(settled.pnl.get(), ATTRIBUTION_DEPOSITS[1] as i128);
        assert_eq!(
            close_progress(&world.env.portfolio_state(debtor)),
            CloseProgressLedgerV16::EMPTY
        );

        let expected = [
            ATTRIBUTION_DEPOSITS[0] + ATTRIBUTION_DEPOSITS[1],
            0,
            ATTRIBUTION_DEPOSITS[2],
            ATTRIBUTION_DEPOSITS[3],
            ATTRIBUTION_DEPOSITS[4],
        ];
        world.env.resolve();
        world.env.svm.warp_to_slot(25);
        for _ in 0..8 {
            for actor in [2, 1, 0, 3, 4] {
                if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                    continue;
                }
                let before = world.frame();
                match world.payout(actor, false) {
                    Ok(cu) => {
                        peak_payout_cu = peak_payout_cu.max(cu);
                        assert_cu_within(
                            "INV-039 recreated-debtor terminal payout",
                            cu,
                            CUSTODY_CU_LIMIT,
                        );
                        assert_ne!(world.frame(), before);
                    }
                    Err(error) => {
                        assert!(is_engine_non_progress_error(&error), "{error}");
                        assert_eq!(world.frame(), before);
                    }
                }
                world.check([0; 4], [false; 4]);
                for (i, limit) in expected.into_iter().enumerate() {
                    assert!(world.env.token_amount(world.actors[i].token) as u128 <= limit);
                }
            }
            if world
                .actors
                .iter()
                .all(|a| resolved_portfolio_is_terminal(&world.env, a.portfolio))
            {
                break;
            }
        }
        for (i, entitlement) in expected.into_iter().enumerate() {
            assert_eq!(
                world.env.token_amount(world.actors[i].token) as u128,
                entitlement
            );
            assert!(resolved_portfolio_is_terminal(
                &world.env,
                world.actors[i].portfolio
            ));
            assert_no_junior_claim_retry(&mut world, i);
        }
        assert_eq!(world.env.market_state().1.vault, 0);
        assert_eq!(world.env.market_state().1.c_tot, 0);
        for actor in &world.actors {
            let cu = world
                .env
                .close_portfolio_with_cu(&actor.owner, actor.portfolio);
            assert_cu_within(
                "INV-039 recreated-debtor terminal deletion",
                cu,
                CUSTODY_CU_LIMIT,
            );
        }
        assert_eq!(world.env.market_state().1.materialized_portfolio_count, 0);
    }
    println!("INV-039: 2 bankruptcy close/reopen worlds; peak composed route {peak_route_cu}, crank {peak_crank_cu}, payout {peak_payout_cu} CU");
}
