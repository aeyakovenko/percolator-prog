//! INV-039: a pending bankruptcy cohort survives close expiry and terminal preemption.
//! The live-B and expired-close routes must preserve the same original owner entitlement.
//! Exact prefix accounting keeps the retained weight separate from the close partition,
//! through permissionless Recovery, resolution, B booking, debit and SPL payout. Rejected
//! suffixes restore complete Accounts after both B booking and receipt/payment prefixes.
//! This finite, single-cohort comparison excludes nonzero fees/funding, adverse drift
//! during the close, insurance/backing, ADL and arbitrary histories. Row 419 stays OPEN.

use super::*;
use solana_sdk::fee::FeeStructure;

pub(super) fn terminal_instruction(world: &AttributionWorld, actor: usize) -> Instruction {
    let a = &world.actors[actor];
    Instruction {
        program_id: world.env.program_id,
        data: ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        }
        .encode(),
        accounts: vec![
            AccountMeta::new_readonly(a.owner.pubkey(), false),
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(a.portfolio, false),
            AccountMeta::new(a.token, false),
            AccountMeta::new(world.env.vault, false),
            AccountMeta::new_readonly(world.env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
    }
}

fn reject_after_close(world: &mut AttributionWorld, actor: usize) -> u64 {
    let mut suffix = close_instruction(world, 0);
    suffix.accounts[0].is_signer = false;
    reject(
        world,
        &[terminal_instruction(world, actor), suffix],
        3,
        PercolatorError::ExpectedSigner,
    )
}

pub(super) fn reject(
    world: &mut AttributionWorld,
    economic_instructions: &[Instruction],
    error_index: u8,
    error: PercolatorError,
) -> u64 {
    let mut instructions = vec![heap_ix(), cu_ix()];
    instructions.extend_from_slice(economic_instructions);
    world.env.svm.expire_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&world.env.payer.pubkey()),
        &[&world.env.payer],
        world.env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let mut keys = tx.message.account_keys.clone();
    keys.extend(world.frame().into_iter().map(|(key, _)| key));
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys
        .iter()
        .map(|key| world.env.svm.get_account(key))
        .collect();
    let fee = u64::from(tx.message.header.num_required_signatures)
        * FeeStructure::default().lamports_per_signature;
    let failure = world
        .env
        .svm
        .send_transaction(tx)
        .expect_err("the exact terminal rejection must roll back its prefix");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(error_index, InstructionError::Custom(error as u32))
    );
    assert_eq!(
        failure
            .meta
            .logs
            .iter()
            .filter(|line| { **line == format!("Program {} success", world.env.program_id) })
            .count(),
        usize::from(error_index - 2),
        "every earlier continuation must succeed before the rejection"
    );
    for (key, mut expected) in keys.into_iter().zip(before) {
        if key == world.env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee;
        }
        assert_eq!(
            world.env.svm.get_account(&key),
            expected,
            "complete Account {key}"
        );
    }
    assert_cu_within(
        "INV-039 preempted close rollback",
        failure.meta.compute_units_consumed,
        2 * CUSTODY_CU_LIMIT,
    );
    failure.meta.compute_units_consumed
}

fn assert_prefix(
    world: &AttributionWorld,
    gain: u128,
    initial_close: CloseProgressLedgerV16,
    booked: bool,
    finished: [bool; 5],
    expected: [u128; 5],
) {
    world.check([0; 4], [!finished[0], false, false, false]);
    let residual = gain - ATTRIBUTION_DEPOSITS[1];
    let mut close = initial_close;
    if booked {
        close.finalized = true;
        close.b_loss_booked = residual;
        close.residual_remaining = 0;
    }
    assert_eq!(
        close_progress(&world.env.portfolio_state(world.actors[1].portfolio)),
        close
    );
    assert_close_partition(close, residual);
    for (i, actor) in world.actors.iter().enumerate() {
        let account = world.env.portfolio_state(actor.portfolio);
        let capital = if i == 1 || finished[i] {
            0
        } else {
            ATTRIBUTION_DEPOSITS[i]
        };
        let pnl = match i {
            0 if !finished[0] => gain as i128,
            1 if !booked => -(residual as i128),
            _ => 0,
        };
        assert_eq!(
            (account.capital.get(), account.pnl.get()),
            (capital, pnl),
            "actor {i}"
        );
        assert_eq!(
            world.env.token_amount(actor.token) as u128,
            if finished[i] { expected[i] } else { 0 }
        );
        let receipt = resolved_receipt(&account);
        if receipt.present {
            assert!(i == 0 && finished[0] && booked);
            assert!(receipt.finalized);
            assert_eq!(
                (receipt.terminal_positive_claim_face, receipt.paid_effective),
                (ATTRIBUTION_DEPOSITS[1], ATTRIBUTION_DEPOSITS[1])
            );
        }
    }
}

#[test]
fn v16_program_expired_bankrupt_close_preserves_pending_cohort_entitlement_across_routes() {
    let mut peak_cu = 0;
    let mut worlds = 0;
    let expected = [
        ATTRIBUTION_DEPOSITS[0] + ATTRIBUTION_DEPOSITS[1],
        0,
        ATTRIBUTION_DEPOSITS[2],
        ATTRIBUTION_DEPOSITS[3],
        ATTRIBUTION_DEPOSITS[4],
    ];
    for reverse_sides in [false, true] {
        for expired in [false, true] {
            for order in [[0usize, 1, 2, 3, 4], [1, 0, 4, 3, 2]] {
                let (mut world, gain) = pending_bankruptcy(reverse_sides, &mut peak_cu);
                let holder = world.actors[0].portfolio;
                let debtor = world.actors[1].portfolio;
                let residual = gain - ATTRIBUTION_DEPOSITS[1];
                let initial_close = close_progress(&world.env.portfolio_state(debtor));
                assert!(
                    initial_close.active && !initial_close.finalized && !initial_close.canceled
                );
                assert_close_partition(initial_close, residual);
                assert_eq!(initial_close.residual_remaining, residual);
                world.check([0; 4], [true, false, false, false]);
                assert_eq!(world.env.portfolio_state(holder).pnl.get(), gain as i128);
                assert_eq!(
                    world.env.portfolio_state(debtor).pnl.get(),
                    -(residual as i128)
                );
                let holder_before = world.env.svm.get_account(&holder);
                let retained_leg = active_leg_for_asset(&world.env.portfolio_state(holder), 1);
                if expired {
                    world.env.svm.warp_to_slot(initial_close.max_close_slot + 1);
                    for mode in [MarketModeV16::Recovery, MarketModeV16::Resolved] {
                        let before = world.frame();
                        let group_before = world.env.market_state().1;
                        let cu = world.env.crank(
                            debtor,
                            ProgInstruction::PermissionlessCrank {
                                now_slot: 0,
                                observations: vec![],
                            },
                        );
                        peak_cu = peak_cu.max(cu);
                        assert_cu_within("INV-039 pending cohort close expiry", cu, CRANK_CU_LIMIT);
                        let group = world.env.market_state().1;
                        assert_eq!(group.mode, mode);
                        assert_eq!(
                            group.recovery_reason,
                            Some(
                                PermissionlessRecoveryReasonV16::ActiveBankruptCloseCannotProgress
                            )
                        );
                        assert_eq!(group.assets, group_before.assets);
                        assert_eq!(group.source_credit, group_before.source_credit);
                        assert_eq!(
                            close_progress(&world.env.portfolio_state(debtor)),
                            initial_close
                        );
                        for (key, account) in before {
                            if key != world.env.market {
                                assert_eq!(world.env.svm.get_account(&key), account);
                            }
                        }
                        assert_prefix(&world, gain, initial_close, false, [false; 5], expected);
                    }
                } else {
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
                        peak_cu = peak_cu.max(cu);
                        assert_cu_within("INV-039 live residual control", cu, CRANK_CU_LIMIT);
                        assert_close_partition(
                            close_progress(&world.env.portfolio_state(debtor)),
                            residual,
                        );
                        assert_eq!(world.env.svm.get_account(&holder), holder_before);
                        world.check([0; 4], [true, false, false, false]);
                    }
                    let booked = close_progress(&world.env.portfolio_state(debtor));
                    assert!(booked.finalized);
                    assert_eq!(
                        (booked.b_loss_booked, booked.residual_remaining),
                        (residual, 0)
                    );
                    // Resolve before the retained holder settles the newly booked B debit.
                    peak_cu = peak_cu.max(world.env.resolve());
                    assert_prefix(&world, gain, initial_close, true, [false; 5], expected);
                }
                assert_eq!(world.env.svm.get_account(&holder), holder_before);
                let resolved_slot = world.env.market_state().1.resolved_slot;
                world.env.svm.warp_to_slot(resolved_slot + 5);
                // Expiry rolls back residual booking; the live control rolls back B debit,
                // receipt creation and payout. Both must preserve the same retry entitlement.
                peak_cu = peak_cu.max(reject_after_close(&mut world, usize::from(expired)));
                let mut booked = !expired;
                let mut finished = [false; 5];
                for step in 0..=usize::from(expired) {
                    for actor in order {
                        if finished[actor] {
                            continue;
                        }
                        if expired && step == 1 && actor == 0 {
                            peak_cu = peak_cu.max(reject_after_close(&mut world, actor));
                        }
                        let before = world.frame();
                        let cu = world
                            .payout(actor, false)
                            .expect("the prescribed debt continuation must succeed");
                        peak_cu = peak_cu.max(cu);
                        assert_cu_within(
                            "INV-039 preempted cohort terminal step",
                            cu,
                            CUSTODY_CU_LIMIT,
                        );
                        assert_ne!(world.frame(), before);
                        for (key, account) in before {
                            if ![
                                world.env.market,
                                world.env.vault,
                                world.actors[actor].portfolio,
                                world.actors[actor].token,
                            ]
                            .contains(&key)
                            {
                                assert_eq!(
                                    world.env.svm.get_account(&key),
                                    account,
                                    "foreign Account {key}"
                                );
                            }
                        }
                        booked |= actor == 1;
                        finished[actor] = actor != 0 || !expired || step == 1;
                        assert_prefix(&world, gain, initial_close, booked, finished, expected);
                        if !finished[0] {
                            assert_eq!(active_leg_for_asset(&world.env.portfolio_state(holder), 1), retained_leg,
                                "refresh and debtor settlement cannot silently retire the original loss weight");
                        }
                        if expired && finished[0] {
                            assert!(resolved_receipt(&world.env.portfolio_state(holder)).present);
                        }
                    }
                }
                assert_eq!(finished, [true; 5]);
                world.check([0; 4], [false; 4]);
                for (i, entitlement) in expected.into_iter().enumerate() {
                    assert_eq!(
                        world.env.token_amount(world.actors[i].token) as u128,
                        entitlement,
                        "side={reverse_sides} expired={expired} order={order:?} actor={i}"
                    );
                    assert!(resolved_portfolio_is_terminal(
                        &world.env,
                        world.actors[i].portfolio
                    ));
                }
                assert_eq!(world.env.market_state().1.vault, 0);
                assert_eq!(world.env.market_state().1.c_tot, 0);
                assert_eq!(world.env.market_state().1.pnl_pos_tot, 0);
                for actor in 0..5 {
                    let retry = terminal_instruction(&world, actor);
                    peak_cu = peak_cu.max(reject(
                        &mut world,
                        &[retry],
                        2,
                        PercolatorError::EngineNonProgress,
                    ));
                }
                for actor in &world.actors {
                    let cu = world
                        .env
                        .close_portfolio_with_cu(&actor.owner, actor.portfolio);
                    peak_cu = peak_cu.max(cu);
                    assert_cu_within("INV-039 preempted cohort deletion", cu, CUSTODY_CU_LIMIT);
                }
                assert_eq!(world.env.market_state().1.materialized_portfolio_count, 0);
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 8);
    println!("INV-039 preempted bankruptcy: worlds={worlds}, prefix_rollbacks=12, terminal_retries=40, portfolio_deletions=40, peak_continuation_cu={peak_cu}");
}
