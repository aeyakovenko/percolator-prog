//! INV-039/048/076/081, rows 419/435: a live custody attempt must preserve
//! both pending creditor weight and the same owner's cross-asset debt.
//! The creditor-only withdrawal test has no bankruptcy or cross-asset debt;
//! mixed-role matrices do not cross this owner's live custody gate. Here a
//! successful crank/Recovery-forfeit prefix settles K and removes debt exposure
//! before Withdraw rejects. Rollback must restore support, its face burn and pending B.
//! Commit K separately, then resolve with pending weight or book/release B and
//! withdraw first: both routes owe the same input-derived owner entitlements.
//! These finite, fee/funding-free histories leave rows 419/435 and INV-086 open.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

fn withdrawal(world: &AttributionWorld) -> Instruction {
    let env = &world.env;
    let owner = &world.actors[0];
    Instruction {
        program_id: env.program_id,
        data: env.withdraw_ix(owner.portfolio, 1).encode(),
        accounts: vec![
            AccountMeta::new(owner.owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(owner.portfolio, false),
            AccountMeta::new(owner.token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
    }
}

fn reject_withdrawal(world: &mut AttributionWorld, prefix: &[Instruction]) -> u64 {
    world.env.svm.expire_blockhash();
    let prefix_len = prefix.len();
    let mut instructions = vec![heap_ix(), cu_ix()];
    instructions.extend_from_slice(prefix);
    instructions.push(withdrawal(world));
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&world.env.payer.pubkey()),
        &[&world.env.payer, &world.actors[0].owner],
        world.env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    assert_eq!(tx.message.header.num_required_signatures, 2);
    let mut keys = tx.message.account_keys.clone();
    keys.extend(world.frame().into_iter().map(|(key, _)| key));
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys
        .into_iter()
        .map(|key| (key, world.env.svm.get_account(&key)))
        .collect();
    let failure = world
        .env
        .svm
        .send_transaction(tx)
        .expect_err("pending creditor weight blocks even a one-atom withdrawal");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(
            (2 + prefix_len) as u8,
            InstructionError::Custom(PercolatorError::EngineStale as u32),
        ),
        "reach the economic gate with current owner and sequence bindings"
    );
    assert_eq!(
        failure
            .meta
            .logs
            .iter()
            .filter(|line| **line == format!("Program {} success", world.env.program_id))
            .count(),
        prefix_len,
        "the K-settlement prefix really executed"
    );
    for (key, mut expected) in before {
        if key == world.env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -=
                2 * FeeStructure::default().lamports_per_signature;
        }
        assert_eq!(
            world.env.svm.get_account(&key),
            expected,
            "full Account {key}"
        );
    }
    let cu = failure.meta.compute_units_consumed;
    assert_cu_within("mixed-role settlement/withdrawal rollback", cu, 600_000);
    cu
}

#[test]
fn v16_program_mixed_role_withdrawal_preserves_pending_weight_and_resolved_debt() {
    assert_certified_engine_pin("INV-039 mixed-role live withdrawal");
    let mut peak = 0;
    let mut worlds = 0;
    let mut rollbacks = 0;
    let mut withdrawals = 0;
    let mut receipt_retries = 0;
    for debt in [36_000, 240_000] {
        for reverse in [false, true] {
            for assets in [[1, 2], [2, 1]] {
                for live_release in [false, true] {
                    let (mut world, mut book) = setup(reverse, assets, debt);
                    let holder = world.actors[0].portfolio;
                    let owner = world.actors[0].owner.insecure_clone();
                    world.env.update_asset_lifecycle_as_admin_with_cu(
                        processor::ASSET_ACTION_SHUTDOWN,
                        assets[1] as u16,
                        10,
                        0,
                    );
                    // The peer leaves first so the owner's later Recovery exit
                    // removes debt exposure instead of retaining a second weight.
                    let cu = world.env.forfeit_recovery_leg_with_cu(
                        &world.actors[2].owner,
                        world.actors[2].portfolio,
                        assets[1] as u16,
                        u128::MAX,
                    );
                    assert_cu_within("mixed-role debt peer exit", cu, CRANK_CU_LIMIT);
                    peak = peak.max(cu);
                    book.check(&world);
                    let settle = ProgInstruction::PermissionlessCrank {
                        now_slot: 10,
                        observations: crank_observations_for_assets(&[1, 2]),
                    };
                    let detach = Instruction {
                        program_id: world.env.program_id,
                        data: ProgInstruction::ForfeitRecoveryLeg {
                            portfolio_id: world.env.portfolio_id(holder),
                            position_epoch: world.env.portfolio_position_epoch(holder),
                            asset_index: assets[1] as u16,
                            b_delta_budget: u128::MAX,
                        }
                        .encode(),
                        accounts: vec![
                            AccountMeta::new(owner.pubkey(), true),
                            AccountMeta::new(world.env.market, false),
                            AccountMeta::new(holder, false),
                        ],
                    };
                    let refresh = Instruction {
                        program_id: world.env.program_id,
                        data: settle.encode(),
                        accounts: vec![
                            AccountMeta::new(world.env.payer.pubkey(), true),
                            AccountMeta::new(world.env.market, false),
                            AccountMeta::new(holder, false),
                        ],
                    };
                    peak = peak.max(reject_withdrawal(&mut world, &[refresh, detach.clone()]));
                    rollbacks += 1;
                    book.check(&world);
                    assert!(!book.booked && !book.charged && !book.debt_settled);

                    // Committing the identical prefix proves the rejected bundle
                    // rolled back real debt settlement and exposure removal.
                    let before = world.env.portfolio_state(holder);
                    let cu = world.env.crank(holder, settle.clone());
                    assert_cu_within("mixed-role live K settlement", cu, CRANK_CU_LIMIT);
                    peak = peak.max(cu);
                    book.check(&world);
                    assert!(!book.booked && !book.charged && book.debt_settled);
                    let cu = send_raw_tx(&mut world.env.svm, &world.env.payer, detach, &[&owner])
                        .expect("commit the same Recovery debt exit");
                    assert_cu_within("mixed-role current debt detach", cu, CRANK_CU_LIMIT);
                    peak = peak.max(cu);
                    book.check(&world);
                    let settled = world.env.portfolio_state(holder);
                    assert!(!has_active_leg_for_asset(&settled, assets[1]));
                    let retained = active_leg_for_asset(&settled, assets[0]);
                    assert_eq!((retained.basis_pos_q, retained.loss_weight), (0, POS_SCALE));
                    assert_eq!(
                        settled.capital.get(),
                        before.capital.get() - (debt - book.support())
                    );
                    assert_eq!(
                        settled.pnl.get(),
                        before.pnl.get() - book.support_face() as i128
                    );
                    peak = peak.max(reject_withdrawal(&mut world, &[]));
                    rollbacks += 1;
                    book.check(&world);

                    if live_release {
                        let cu = world.env.crank(world.actors[1].portfolio, settle.clone());
                        assert_cu_within("mixed-role live B booking", cu, CRANK_CU_LIMIT);
                        peak = peak.max(cu);
                        book.check(&world);
                        assert!(book.booked && !book.charged);
                        for _ in 0..4 {
                            if !has_active_leg_for_asset(
                                &world.env.portfolio_state(holder),
                                assets[0],
                            ) {
                                break;
                            }
                            let cu = world.env.crank(holder, settle.clone());
                            assert_cu_within(
                                "mixed-role pending-weight release",
                                cu,
                                CRANK_CU_LIMIT,
                            );
                            peak = peak.max(cu);
                            book.check(&world);
                        }
                        assert!(book.charged && book.debt_settled);
                        assert!(!has_active_leg_for_asset(
                            &world.env.portfolio_state(holder),
                            assets[0]
                        ));
                        let ix = withdrawal(&world);
                        let cu = send_raw_tx(&mut world.env.svm, &world.env.payer, ix, &[&owner])
                            .expect("same owner, amount and custody succeed after B release");
                        assert_cu_within("mixed-role released withdrawal", cu, CUSTODY_CU_LIMIT);
                        peak = peak.max(cu);
                        assert_eq!(world.env.token_amount(world.actors[0].token), 1);
                        withdrawals += 1;
                        book.check(&world);
                    }

                    world.env.resolve();
                    world.env.svm.warp_to_slot(15);
                    book.check(&world);
                    assert_eq!(book.charged, live_release);
                    for _ in 0..16 {
                        for actor in [0, 2, 1, 3, 4] {
                            if !resolved_portfolio_is_terminal(
                                &world.env,
                                world.actors[actor].portfolio,
                            ) {
                                book.close(&mut world, actor, &mut peak);
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
                    assert!(book.booked && book.charged && book.debt_settled);
                    for actor in 0..5 {
                        assert!(resolved_portfolio_is_terminal(
                            &world.env,
                            world.actors[actor].portfolio
                        ));
                        assert_eq!(
                            world.env.token_amount(world.actors[actor].token) as u128,
                            book.payouts()[actor]
                        );
                        if resolved_receipt(
                            &world.env.portfolio_state(world.actors[actor].portfolio),
                        )
                        .present
                        {
                            let before = world.frame();
                            let cu = world.payout(actor, true).expect("fully paid receipt retry");
                            assert_cu_within("mixed-role receipt retry", cu, CUSTODY_CU_LIMIT);
                            peak = peak.max(cu);
                            assert_eq!(world.frame(), before);
                            receipt_retries += 1;
                        }
                    }
                    book.check(&world);
                    let group = world.env.market_state().1;
                    assert_eq!(
                        (group.vault, group.c_tot, group.pnl_pos_tot),
                        (book.face_discount(), 0, 0)
                    );
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(
        (worlds, rollbacks, withdrawals, receipt_retries),
        (16, 32, 8, 16)
    );
    println!("INV-039 mixed custody: {worlds} worlds, {rollbacks} full rollbacks, {withdrawals} released withdrawals, {receipt_retries} receipt retries; peak CU={peak}");
}
