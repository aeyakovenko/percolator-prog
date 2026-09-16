//! Row 417: retain partially paid/deferred receipt requests through owner deletion
//! and both quote vault closes. Every rejecting suffix restores the entire frame.

use super::*;

#[path = "inv_067_receipt_post_close_custody.rs"]
mod post_close_custody;

#[derive(Default)]
struct Evidence {
    rollbacks: usize,
    paying_rollbacks: usize,
    slab_rollbacks: usize,
    peak_bytes: usize,
    close_cu: u64,
}

fn submit(
    world: &mut World,
    evidence: &mut Evidence,
    instructions: &[Instruction],
    admin: bool,
    owners: &[usize],
) -> Result<litesvm::types::TransactionMetadata, litesvm::types::FailedTransactionMetadata> {
    world.env.svm.expire_blockhash();
    let all: Vec<_> = [heap_ix(), cu_ix()]
        .into_iter()
        .chain(instructions.iter().cloned())
        .collect();
    let mut signers = vec![&world.env.payer];
    if admin {
        signers.push(&world.env.admin);
    }
    signers.extend(owners.iter().map(|&actor| &world.actors[actor].owner));
    let tx = Transaction::new_signed_with_payer(
        &all,
        Some(&world.env.payer.pubkey()),
        &signers,
        world.env.svm.latest_blockhash(),
    );
    assert_eq!(
        usize::from(tx.message.header.num_required_signatures),
        1 + usize::from(admin) + owners.len()
    );
    for (actor, entry) in world.actors.iter().enumerate() {
        let signed = tx
            .message
            .account_keys
            .iter()
            .position(|key| *key == entry.owner.pubkey())
            .is_some_and(|index| tx.message.is_signer(index));
        assert_eq!(signed, owners.contains(&actor));
    }
    let bytes = bincode::serialize(&tx).unwrap().len();
    assert!(bytes <= 1_232, "public packet size: {bytes}");
    evidence.peak_bytes = evidence.peak_bytes.max(bytes);
    let mut payer = world
        .env
        .svm
        .get_account(&world.env.payer.pubkey())
        .unwrap();
    payer.lamports -= u64::from(tx.message.header.num_required_signatures)
        * FeeStructure::default().lamports_per_signature;
    let result = world.env.svm.send_transaction(tx);
    let meta = match &result {
        Ok(meta) => meta,
        Err(failure) => &failure.meta,
    };
    world.peak_cu = world.peak_cu.max(meta.compute_units_consumed);
    assert_cu_within(
        "receipt terminal cleanup transaction",
        meta.compute_units_consumed,
        700_000,
    );
    assert_eq!(
        world.env.svm.get_account(&world.env.payer.pubkey()),
        Some(payer)
    );
    result
}

fn reject(
    world: &mut World,
    rail: &Rail,
    evidence: &mut Evidence,
    instructions: &[Instruction],
    admin: bool,
    owners: &[usize],
    error: InstructionError,
    token_calls: usize,
) {
    let before = rail.frame(world);
    let failure = submit(world, evidence, instructions, admin, owners)
        .expect_err("the retained suffix must reject");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError((instructions.len() + 1) as u8, error)
    );
    successes(&failure.meta, world.env.program_id, instructions.len() - 1);
    successes(&failure.meta, spl_token::ID, token_calls);
    assert_eq!(
        rail.frame(world),
        before,
        "exact data, owner, SPL, rent and lamport rollback"
    );
    evidence.rollbacks += 1;
    evidence.paying_rollbacks += usize::from(token_calls != 0);
}

fn close_portfolio(world: &World, actor: usize) -> Instruction {
    Instruction {
        program_id: world.env.program_id,
        accounts: vec![
            AccountMeta::new(world.actors[actor].owner.pubkey(), true),
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(world.actors[actor].portfolio, false),
        ],
        data: world
            .env
            .close_portfolio_ix(world.actors[actor].portfolio)
            .encode(),
    }
}

fn clear_paid_receipt(
    world: &mut World,
    rail: &Rail,
    evidence: &mut Evidence,
    actor: usize,
    retained: &Instruction,
    bad: &Instruction,
) {
    let receipt = world.receipt(actor);
    assert!(receipt.present);
    assert_eq!(receipt.terminal_positive_claim_face, FACES[actor]);
    assert_eq!(
        receipt.prior_bound_contribution_num,
        FACES[actor] * BOUND_SCALE
    );
    assert_eq!(receipt.paid_effective, entitlement(actor, FINAL_RESIDUAL));
    // A paying claim retains its paid receipt. The next zero-due claim clears it.
    reject(
        world,
        rail,
        evidence,
        &[retained.clone(), bad.clone()],
        false,
        &[],
        InstructionError::InvalidInstructionData,
        0,
    );
    commit_payments(world, rail, &[retained.clone()], [0; 5], false);
    assert!(!world.receipt(actor).present);
}

fn close_slab(world: &World, rail: &Rail) -> Instruction {
    Instruction {
        program_id: world.env.program_id,
        accounts: vec![
            AccountMeta::new(world.env.admin.pubkey(), true),
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(world.env.vault, false),
            AccountMeta::new_readonly(world.env.vault_authority, false),
            AccountMeta::new(world.provider_token, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(rail.vault, false),
            AccountMeta::new(rail.source, false),
            AccountMeta::new(world.env.mint, false),
        ],
        data: ProgInstruction::CloseSlab {
            authority_epoch: world.env.control_sequences(0).authority_epoch,
        }
        .encode(),
    }
}

fn tombstone_error(instruction: &Instruction) -> InstructionError {
    // Claim checks the typed header first; CloseResolved/CloseSlab first require
    // enough bytes for a mutable market view.
    InstructionError::Custom(
        if instruction.data == ProgInstruction::ClaimResolvedPayoutTopup.encode() {
            PercolatorError::InvalidAccountKind as u32
        } else {
            PercolatorError::InvalidAccountLen as u32
        },
    )
}

fn delete(
    world: &mut World,
    rail: &Rail,
    evidence: &mut Evidence,
    actor: usize,
    retained: &Instruction,
) {
    let close = close_portfolio(world, actor);
    // The owner's deletion succeeds before their pre-expiry request is replayed.
    reject(
        world,
        rail,
        evidence,
        &[close.clone(), retained.clone()],
        false,
        &[actor],
        InstructionError::Custom(PercolatorError::NotInitialized as u32),
        0,
    );
    let before = rail.frame(world);
    let portfolio = world.actors[actor].portfolio;
    let mut expected_portfolio = world.env.svm.get_account(&portfolio).unwrap();
    let market_lamports = world
        .env
        .svm
        .get_account(&world.env.market)
        .unwrap()
        .lamports;
    let mut group = world.env.market_state().1;
    submit(world, evidence, &[close], false, &[actor]).unwrap();
    assert_eq!(
        world
            .env
            .svm
            .get_account(&world.env.market)
            .unwrap()
            .lamports,
        market_lamports + expected_portfolio.lamports
    );
    expected_portfolio.lamports = 0;
    expected_portfolio.data.clear();
    assert_eq!(
        world.env.svm.get_account(&portfolio),
        Some(expected_portfolio)
    );
    group.materialized_portfolio_count -= 1;
    assert_eq!(world.env.market_state().1, group);
    world.assert_frame_except(&before, &[world.env.market, portfolio]);
}

#[test]
fn v16_program_partial_receipts_cannot_repay_across_owner_deletion_and_close_slab_rails() {
    let mut evidence = Evidence::default();
    let mut peaks = [0; 2];
    let bad = Instruction {
        program_id: solana_sdk::system_program::ID,
        accounts: vec![],
        data: vec![],
    };
    for native in [false, true] {
        for eager in [0, 4] {
            for final_secondary in [false, true] {
                let peer = 4 - eager;
                let mut world = World::before_receipts_with_staggered_quote_rails(
                    if native {
                        install_native_secondary
                    } else {
                        install_secondary
                    },
                    if native {
                        spl_token::native_mint::DECIMALS
                    } else {
                        0
                    },
                );
                for actor in [0, 4] {
                    for _ in 0..8 {
                        if world.receipt(actor).present {
                            break;
                        }
                        world.land(&[world.payout(actor, false)], false).unwrap();
                    }
                    assert!(world.receipt(actor).present);
                }
                let original = [world.receipt(0), world.receipt(4)];
                let mut rail = Rail::new(&mut world);
                rail.fund(&mut world, SECONDARY_SUPPLY);
                // Freeze requests before either expiry or any owner exit.
                let claims = [false, true].map(|secondary| {
                    [0, 4].map(|actor| {
                        if secondary {
                            rail.payout(&world, actor, true)
                        } else {
                            world.payout(actor, true)
                        }
                    })
                });
                let closes = [false, true].map(|secondary| {
                    [0, 4].map(|actor| {
                        if secondary {
                            rail.payout(&world, actor, false)
                        } else {
                            world.payout(actor, false)
                        }
                    })
                });
                let slab = close_slab(&world, &rail);
                let normalize = world.payout(2, false);
                let mut paid = [1_116, 0, 0, 0, 1_217];
                check_receipts(&world, &rail, &original, paid, 0);
                world.peak_cu = 0;
                world.env.svm.warp_to_slot(14);
                let mut deltas = [0; 5];
                deltas[eager] = entitlement(eager, 662) - entitlement(eager, 501);
                commit_payments(
                    &mut world,
                    &rail,
                    &[normalize.clone(), claims[1][eager / 4].clone()],
                    deltas,
                    true,
                );
                paid[eager] += deltas[eager];
                check_receipts(&world, &rail, &original, paid, 1);
                assert_eq!(world.receipt(peer), original[peer / 4]);

                world.env.svm.warp_to_slot(17);
                commit_payments(&mut world, &rail, &[normalize.clone()], [0; 5], false);
                check_receipts(&world, &rail, &original, paid, 2);
                // Source attribution and its own receipt finish while both older receipts
                // still have unpaid entitlement. Clearing either may pay only that difference.
                commit_payments(&mut world, &rail, &[normalize.clone()], [0; 5], false);
                commit_payments(&mut world, &rail, &[normalize], [0, 0, 1_283, 0, 0], false);
                paid[2] = 1_283;
                assert_eq!(
                    world
                        .env
                        .market_state()
                        .1
                        .resolved_payout_ledger
                        .terminal_claim_bound_unreceipted_num,
                    0
                );
                for actor in [0, 4] {
                    let mut receipt = original[actor / 4];
                    receipt.paid_effective = paid[actor] - CAPITAL[actor];
                    assert_eq!(world.receipt(actor), receipt);
                }
                let eager_clear = claims[usize::from(final_secondary)][eager / 4].clone();
                let peer_clear = claims[usize::from(final_secondary)][peer / 4].clone();
                let locked = InstructionError::Custom(PercolatorError::EngineLockActive as u32);
                reject(
                    &mut world,
                    &rail,
                    &mut evidence,
                    &[eager_clear.clone(), slab.clone()],
                    true,
                    &[],
                    locked.clone(),
                    1,
                );
                reject(
                    &mut world,
                    &rail,
                    &mut evidence,
                    &[eager_clear.clone(), bad.clone()],
                    false,
                    &[],
                    InstructionError::InvalidInstructionData,
                    1,
                );
                deltas = [0; 5];
                deltas[eager] = CAPITAL[eager] + entitlement(eager, 851) - paid[eager];
                commit_payments(&mut world, &rail, &[eager_clear], deltas, final_secondary);
                paid[eager] += deltas[eager];
                clear_paid_receipt(
                    &mut world,
                    &rail,
                    &mut evidence,
                    eager,
                    &claims[usize::from(!final_secondary)][eager / 4],
                    &bad,
                );
                assert_eq!(
                    world.receipt(peer),
                    original[peer / 4],
                    "deferred face survives peer clear"
                );
                for secondary in 0..2 {
                    let before = rail.frame(&world);
                    submit(
                        &mut world,
                        &mut evidence,
                        &[claims[secondary][eager / 4].clone()],
                        false,
                        &[],
                    )
                    .unwrap();
                    assert_eq!(rail.frame(&world), before, "retained clear is inert");
                    reject(
                        &mut world,
                        &rail,
                        &mut evidence,
                        &[closes[secondary][eager / 4].clone()],
                        false,
                        &[],
                        InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
                        0,
                    );
                }
                delete(
                    &mut world,
                    &rail,
                    &mut evidence,
                    eager,
                    &claims[0][eager / 4],
                );
                // Real peer payouts must roll back when either retained handler targets
                // the deleted owner, on either still-funded quote rail.
                for secondary in 0..2 {
                    for retained in [&claims[secondary][eager / 4], &closes[secondary][eager / 4]] {
                        reject(
                            &mut world,
                            &rail,
                            &mut evidence,
                            &[peer_clear.clone(), retained.clone()],
                            false,
                            &[],
                            InstructionError::Custom(PercolatorError::NotInitialized as u32),
                            1,
                        );
                    }
                }
                reject(
                    &mut world,
                    &rail,
                    &mut evidence,
                    &[peer_clear.clone(), slab.clone()],
                    true,
                    &[],
                    locked,
                    1,
                );
                deltas = [0; 5];
                deltas[peer] = CAPITAL[peer] + entitlement(peer, 851) - paid[peer];
                commit_payments(&mut world, &rail, &[peer_clear], deltas, final_secondary);
                paid[peer] += deltas[peer];
                assert_eq!(paid, [1_198, 0, 1_283, 0, 1_368]);
                clear_paid_receipt(
                    &mut world,
                    &rail,
                    &mut evidence,
                    peer,
                    &claims[usize::from(!final_secondary)][peer / 4],
                    &bad,
                );
                for actor in [peer, 2, 1, 3, 5] {
                    assert!(resolved_portfolio_is_terminal(
                        &world.env,
                        world.actors[actor].portfolio
                    ));
                    let retained = world.payout(actor, true);
                    delete(&mut world, &rail, &mut evidence, actor, &retained);
                }
                let group = world.env.market_state().1;
                assert_eq!(group.materialized_portfolio_count, 0);
                assert_eq!(
                    [
                        group.c_tot,
                        group.pnl_pos_tot,
                        group.source_claim_bound_total_num,
                        group.insurance
                    ],
                    [0; 4]
                );
                assert_eq!(group.vault, 2);
                rail.check(&world);
                let secondary_paid: u64 = rail
                    .destinations
                    .iter()
                    .map(|key| world.env.token_amount(*key))
                    .sum();
                assert_eq!(
                    secondary_paid,
                    if final_secondary {
                        SECONDARY_SUPPLY
                    } else {
                        (entitlement(eager, 662) - entitlement(eager, 501)) as u64
                    }
                );
                assert_eq!(world.env.token_amount(world.env.vault), 2 + secondary_paid);
                assert_eq!(
                    world.env.token_amount(rail.vault),
                    SECONDARY_SUPPLY - secondary_paid
                );

                // Native raw lamports remain unsynchronized through closure, including
                // the case where the secondary token balance has been spent exactly.
                let raw = if native { 7 } else { 0 };
                if raw != 0 {
                    let before = rail.frame(&world);
                    let from = world.env.admin.pubkey();
                    submit(
                        &mut world,
                        &mut evidence,
                        &[system_instruction::transfer(&from, &rail.vault, raw)],
                        true,
                        &[],
                    )
                    .unwrap();
                    world.assert_frame_except(&before, &[from, rail.vault]);
                    assert_eq!(
                        world.env.token_amount(rail.vault),
                        SECONDARY_SUPPLY - secondary_paid
                    );
                }
                let before_slab = rail.frame(&world);
                let market_lamports = world
                    .env
                    .svm
                    .get_account(&world.env.market)
                    .unwrap()
                    .lamports;
                let primary_rent = world
                    .env
                    .svm
                    .get_account(&world.env.vault)
                    .unwrap()
                    .lamports;
                let secondary_account = world.env.svm.get_account(&rail.vault).unwrap();
                let secondary_rent = secondary_account.lamports
                    - if native {
                        u64::from(SECONDARY_SUPPLY - secondary_paid) + raw
                    } else {
                        0
                    };
                let admin_lamports = world
                    .env
                    .svm
                    .get_account(&world.env.admin.pubkey())
                    .unwrap()
                    .lamports;
                let source_lamports = world.env.svm.get_account(&rail.source).unwrap().lamports;
                let final_token_calls = 4 + usize::from(secondary_paid < SECONDARY_SUPPLY);
                let mut closed = false;
                for _ in 0..8 {
                    // A retained request fails after CloseSlab, including after both SPL
                    // closes and the tombstone write. Probe all old rail/handler pairs.
                    let mut calls = None;
                    for secondary in 0..2 {
                        for retained in
                            [&claims[secondary][eager / 4], &closes[secondary][peer / 4]]
                        {
                            let before = rail.frame(&world);
                            let failure = submit(
                                &mut world,
                                &mut evidence,
                                &[slab.clone(), retained.clone()],
                                true,
                                &[],
                            )
                            .unwrap_err();
                            successes(&failure.meta, world.env.program_id, 1);
                            let count = failure
                                .meta
                                .logs
                                .iter()
                                .filter(|line| {
                                    **line == format!("Program {} success", spl_token::ID)
                                })
                                .count();
                            assert!(count == 0 || count == final_token_calls);
                            assert_eq!(
                                failure.err,
                                TransactionError::InstructionError(
                                    3,
                                    if count == 0 {
                                        InstructionError::Custom(
                                            PercolatorError::NotInitialized as u32,
                                        )
                                    } else {
                                        tombstone_error(retained)
                                    }
                                )
                            );
                            assert_eq!(*calls.get_or_insert(count), count);
                            assert_eq!(
                                rail.frame(&world),
                                before,
                                "slab and retained-claim rollback"
                            );
                            evidence.rollbacks += 1;
                            evidence.slab_rollbacks += usize::from(count != 0);
                            evidence.close_cu =
                                evidence.close_cu.max(failure.meta.compute_units_consumed);
                        }
                    }
                    let before = rail.frame(&world);
                    let meta =
                        submit(&mut world, &mut evidence, &[slab.clone()], true, &[]).unwrap();
                    successes(&meta, spl_token::ID, calls.unwrap());
                    evidence.close_cu = evidence.close_cu.max(meta.compute_units_consumed);
                    let market = world.env.svm.get_account(&world.env.market).unwrap();
                    if market.data.len() == percolator_prog::constants::HEADER_LEN {
                        assert_closed_market_tombstone(&market);
                        assert_eq!(calls, Some(final_token_calls));
                        closed = true;
                        break;
                    }
                    assert_ne!(rail.frame(&world), before);
                    world.assert_frame_except(&before, &[world.env.market]);
                    assert_eq!(world.env.market_state().1.vault, 2);
                }
                assert!(closed, "bounded terminal cleanup reaches tombstone");
                world.assert_frame_except(
                    &before_slab,
                    &[
                        world.env.market,
                        world.env.vault,
                        world.env.mint,
                        world.provider_token,
                        rail.vault,
                        rail.source,
                        world.env.admin.pubkey(),
                    ],
                );
                for vault in [world.env.vault, rail.vault] {
                    assert!(world
                        .env
                        .svm
                        .get_account(&vault)
                        .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
                }
                let rent = world
                    .env
                    .svm
                    .get_sysvar::<solana_sdk::rent::Rent>()
                    .minimum_balance(percolator_prog::constants::HEADER_LEN);
                assert_eq!(
                    world
                        .env
                        .svm
                        .get_account(&world.env.market)
                        .unwrap()
                        .lamports,
                    rent
                );
                assert_eq!(
                    world
                        .env
                        .svm
                        .get_account(&world.env.admin.pubkey())
                        .unwrap()
                        .lamports,
                    admin_lamports + market_lamports + primary_rent + secondary_rent + raw - rent
                );
                assert_eq!(
                    world.env.token_amount(world.provider_token),
                    1 + secondary_paid
                );
                assert_eq!(
                    world.env.token_amount(rail.source),
                    SECONDARY_SUPPLY - secondary_paid
                );
                assert_eq!(
                    world.env.svm.get_account(&rail.source).unwrap().lamports,
                    source_lamports
                        + if native {
                            SECONDARY_SUPPLY - secondary_paid
                        } else {
                            0
                        }
                );
                for (mint, supply) in [
                    (world.env.mint, PRIMARY_SUPPLY as u64 - 2),
                    (rail.mint, if native { 0 } else { SECONDARY_SUPPLY }),
                ] {
                    let mut expected = before_slab
                        .iter()
                        .find(|(key, _)| *key == mint)
                        .unwrap()
                        .1
                        .clone()
                        .unwrap();
                    let mut token_mint = Mint::unpack(&expected.data).unwrap();
                    token_mint.supply = supply;
                    Mint::pack(token_mint, &mut expected.data).unwrap();
                    assert_eq!(world.env.svm.get_account(&mint), Some(expected));
                }
                for actor in 0..5 {
                    assert_eq!(rail.paid(&world, actor), paid[actor]);
                }
                for slot in [17, 100] {
                    world.env.svm.warp_to_slot(slot);
                    for secondary in 0..2 {
                        for actor in 0..2 {
                            for retained in [&claims[secondary][actor], &closes[secondary][actor]] {
                                reject(
                                    &mut world,
                                    &rail,
                                    &mut evidence,
                                    &[retained.clone()],
                                    false,
                                    &[],
                                    tombstone_error(retained),
                                    0,
                                );
                            }
                        }
                    }
                    reject(
                        &mut world,
                        &rail,
                        &mut evidence,
                        &[slab.clone()],
                        true,
                        &[],
                        tombstone_error(&slab),
                        0,
                    );
                }
                peaks[usize::from(native)] = peaks[usize::from(native)].max(world.peak_cu);
            }
        }
    }
    assert_eq!(evidence.slab_rollbacks, 8 * 4);
    assert_eq!(evidence.paying_rollbacks, 8 * 7);
    println!("INV-067 receipt CloseSlab: 8 histories; {} exact rollbacks, {} paying receipt rollbacks, {} tombstone rollbacks; peak CU classic={}, native={}, slab={}; peak bytes={}",
        evidence.rollbacks, evidence.paying_rollbacks, evidence.slab_rollbacks,
        peaks[0], peaks[1], evidence.close_cu, evidence.peak_bytes);
}
