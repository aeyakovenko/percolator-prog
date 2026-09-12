//! INV-066/067/068/070/080/081: spending a payout cannot replenish its receipt.
//! Public, nonzero-face histories compare claimant order and rejected/committed
//! payout-spend-replay bundles through terminal disposition. No zero-bound static probe.

use super::{late_expiry::World, *};
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

const FACES: [u128; 5] = [700, 0, 1_000, 0, 1_300];
const CAPITAL: [u128; 5] = [1_000, 0, 1_000, 0, 1_000];
const INITIAL: u128 = 501;
const FINAL: u128 = 851;
const SUPPLY: u128 = 3_852;
const TOTAL_FACE: u128 = 3_000;

fn claim(actor: usize, residual: u128) -> u128 {
    FACES[actor] * residual / TOTAL_FACE
}

fn successes(logs: &[String], program: Pubkey) -> usize {
    logs.iter()
        .filter(|line| **line == format!("Program {program} success"))
        .count()
}

fn spend(world: &World, actor: usize, amount: u128) -> Instruction {
    // The recipient is a settled debtor with no claim. Incoming SPL value must
    // neither create a receipt for it nor replace the spender's payout history.
    spl_token::instruction::transfer(
        &spl_token::ID,
        &world.actors[actor].token,
        &world.actors[1].token,
        &world.actors[actor].owner.pubkey(),
        &[],
        u64::try_from(amount).unwrap(),
    )
    .unwrap()
}

fn land_spending(
    world: &mut World,
    actor: usize,
    instructions: &[Instruction],
) -> Result<litesvm::types::TransactionMetadata, litesvm::types::FailedTransactionMetadata> {
    world.env.svm.expire_blockhash();
    let mut all = vec![heap_ix(), cu_ix()];
    all.extend_from_slice(instructions);
    let tx = Transaction::new_signed_with_payer(
        &all,
        Some(&world.env.payer.pubkey()),
        &[&world.env.payer, &world.actors[actor].owner],
        world.env.svm.latest_blockhash(),
    );
    let mut payer = world
        .env
        .svm
        .get_account(&world.env.payer.pubkey())
        .unwrap();
    payer.lamports -= u64::from(tx.message.header.num_required_signatures)
        * solana_sdk::fee::FeeStructure::default().lamports_per_signature;
    let result = world.env.svm.send_transaction(tx);
    let meta = match &result {
        Ok(meta) => meta,
        Err(failure) => &failure.meta,
    };
    assert_cu_within(
        "receipt payout/spend/replay",
        meta.compute_units_consumed,
        500_000,
    );
    world.peak_cu = world.peak_cu.max(meta.compute_units_consumed);
    assert_eq!(
        world.env.svm.get_account(&world.env.payer.pubkey()),
        Some(payer)
    );
    result
}

fn check_value(world: &World, paid: [u128; 5], spent: [u128; 5]) {
    for actor in 0..5 {
        let received = if actor == 1 { spent.iter().sum() } else { 0 };
        assert_eq!(
            u128::from(world.env.token_amount(world.actors[actor].token)),
            paid[actor] - spent[actor] + received,
            "wallet cashflows must preserve claimant {actor}'s attributed payout"
        );
        assert!(paid[actor] <= CAPITAL[actor] + claim(actor, FINAL));
    }
    assert_eq!(
        world.env.market_state().1.vault,
        SUPPLY - 1 - paid.iter().sum::<u128>()
    );
    assert_eq!(world.env.token_amount(world.provider_token), 1);
    assert!(resolved_portfolio_is_terminal(
        &world.env,
        world.actors[1].portfolio
    ));
    assert!(!world.receipt(1).present);
    world.custody();
}

#[test]
fn v16_program_spent_payouts_do_not_replenish_receipts_across_order_and_atomic_retry() {
    let expected: [u128; 5] = std::array::from_fn(|actor| CAPITAL[actor] + claim(actor, FINAL));
    assert_eq!(expected, [1_198, 0, 1_283, 0, 1_368]);
    let rounding = FINAL - (0..5).map(|actor| claim(actor, FINAL)).sum::<u128>();
    assert_eq!(rounding, 2);
    let mut outcomes = Vec::new();
    let mut peak_cu = 0;
    let mut rollbacks = 0;
    for order in [[0, 4], [4, 0]] {
        for rollback in [false, true] {
            let mut world = World::new();
            world.peak_cu = 0;
            let mut paid = [
                CAPITAL[0] + claim(0, INITIAL),
                0,
                0,
                0,
                CAPITAL[4] + claim(4, INITIAL),
            ];
            let mut spent = [0; 5];
            let original = order.map(|actor| world.receipt(actor));
            let retained = order.map(|actor| world.payout(actor, true));
            let identities = order.map(|actor| {
                (
                    world.env.portfolio_id(world.actors[actor].portfolio),
                    world
                        .env
                        .portfolio_position_epoch(world.actors[actor].portfolio),
                )
            });
            check_value(&world, paid, spent);

            // Both initial payouts leave their original ATAs through real owner
            // transfers. Empty destinations must still remember their paid receipts.
            for actor in order {
                let before = world.frame();
                let instruction = spend(&world, actor, paid[actor]);
                land_spending(&mut world, actor, &[instruction]).unwrap();
                spent[actor] = paid[actor];
                world.assert_frame_except(
                    &before,
                    &[world.actors[actor].token, world.actors[1].token],
                );
                assert_eq!(order.map(|actor| world.receipt(actor)), original);
                check_value(&world, paid, spent);
            }
            let before = world.frame();
            let meta = world.land(&retained, false).unwrap();
            assert_eq!(successes(&meta.logs, spl_token::ID), 0);
            assert_eq!(world.frame(), before);

            world.env.svm.warp_to_slot(13);
            world.land(&[world.payout(2, false)], false).unwrap();
            let ledger = world.env.market_state().1.resolved_payout_ledger;
            assert_eq!(ledger.snapshot_slot, 12);
            assert_eq!(ledger.snapshot_residual, FINAL);
            assert_eq!(ledger.current_payout_rate_num, FINAL * BOUND_SCALE);
            assert_eq!(ledger.current_payout_rate_den, TOTAL_FACE * BOUND_SCALE);
            assert_eq!(
                ledger.terminal_claim_bound_unreceipted_num,
                FACES[2] * BOUND_SCALE
            );
            assert_eq!(
                ledger.terminal_claim_exact_receipts_num,
                (FACES[0] + FACES[4]) * BOUND_SCALE
            );
            assert!(!ledger.finalized && !ledger.payout_halted);
            assert_eq!(order.map(|actor| world.receipt(actor)), original);
            check_value(&world, paid, spent);

            for (index, actor) in order.into_iter().enumerate() {
                let due = claim(actor, FINAL) - claim(actor, INITIAL);
                assert_eq!(due, if actor == 0 { 82 } else { 151 });
                let bundle = [
                    retained[index].clone(),
                    spend(&world, actor, due),
                    retained[index].clone(),
                ];
                let before = world.frame();
                if rollback {
                    let mut rejected = bundle.to_vec();
                    rejected.push(Instruction {
                        program_id: solana_sdk::system_program::ID,
                        accounts: vec![],
                        data: vec![],
                    });
                    let failure = land_spending(&mut world, actor, &rejected).unwrap_err();
                    assert_eq!(
                        failure.err,
                        TransactionError::InstructionError(
                            5,
                            InstructionError::InvalidInstructionData
                        )
                    );
                    assert_eq!(successes(&failure.meta.logs, world.env.program_id), 2);
                    assert_eq!(successes(&failure.meta.logs, spl_token::ID), 2);
                    assert_eq!(
                        world.frame(),
                        before,
                        "payout, spending and duplicate receipt call roll back together"
                    );
                    check_value(&world, paid, spent);
                    rollbacks += 1;
                }
                let meta = land_spending(&mut world, actor, &bundle).unwrap();
                assert_eq!(successes(&meta.logs, world.env.program_id), 2);
                assert_eq!(
                    successes(&meta.logs, spl_token::ID),
                    2,
                    "one payout and one owner spend, no duplicate payout"
                );
                paid[actor] += due;
                spent[actor] += due;
                let mut receipt = original[index];
                receipt.paid_effective = claim(actor, FINAL);
                assert_eq!(world.receipt(actor), receipt);
                assert_eq!(world.env.market_state().1.resolved_payout_ledger, ledger);
                assert_eq!(
                    (
                        world.env.portfolio_id(world.actors[actor].portfolio),
                        world
                            .env
                            .portfolio_position_epoch(world.actors[actor].portfolio)
                    ),
                    identities[index]
                );
                world.assert_frame_except(
                    &before,
                    &[
                        world.env.market,
                        world.env.vault,
                        world.actors[actor].portfolio,
                        world.actors[actor].token,
                        world.actors[1].token,
                    ],
                );
                check_value(&world, paid, spent);

                let before = world.frame();
                let meta = world.land(&[retained[index].clone()], false).unwrap();
                assert_eq!(successes(&meta.logs, spl_token::ID), 0);
                assert_eq!(
                    world.frame(),
                    before,
                    "fresh-blockhash retry after spending cannot pay again"
                );
            }

            // Settle the still-positive source claim, then retire the paid receipts.
            // Every nonterminal call must progress; an error or bounded-drain failure
            // is a failure of this public witness, never an accepted terminal result.
            for actor in [2, order[0], order[1], 1, 3] {
                for _ in 0..8 {
                    if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                        break;
                    }
                    let before = world.frame();
                    let tokens = world.env.token_amount(world.actors[actor].token);
                    let meta = world.land(&[world.payout(actor, false)], false).unwrap();
                    assert_cu_within(
                        "spent receipt terminal progress",
                        meta.compute_units_consumed,
                        CUSTODY_CU_LIMIT,
                    );
                    assert_ne!(world.frame(), before);
                    paid[actor] +=
                        u128::from(world.env.token_amount(world.actors[actor].token) - tokens);
                    world.assert_frame_except(
                        &before,
                        &[
                            world.env.market,
                            world.env.vault,
                            world.actors[actor].portfolio,
                            world.actors[actor].token,
                        ],
                    );
                    check_value(&world, paid, spent);
                }
                assert!(resolved_portfolio_is_terminal(
                    &world.env,
                    world.actors[actor].portfolio
                ));
            }
            assert_eq!(paid, expected);
            assert_eq!(spent, [expected[0], 0, 0, 0, expected[4]]);
            for actor in 0..5 {
                let before = world.frame();
                world.land(&[world.payout(actor, true)], false).unwrap();
                assert_eq!(
                    world.frame(),
                    before,
                    "settled recipient balances cannot revive receipts"
                );
            }
            for actor in [2, order[0], order[1], 1, 3] {
                let portfolio = world.actors[actor].portfolio;
                let before = world.frame();
                let rent = world.env.svm.get_account(&portfolio).unwrap().lamports;
                let market_lamports = world
                    .env
                    .svm
                    .get_account(&world.env.market)
                    .unwrap()
                    .lamports;
                let mut group = world.env.market_state().1;
                let cu = world
                    .env
                    .close_portfolio_with_cu(&world.actors[actor].owner, portfolio);
                assert_cu_within("spent receipt portfolio close", cu, CUSTODY_CU_LIMIT);
                world.peak_cu = world.peak_cu.max(cu);
                group.materialized_portfolio_count -= 1;
                assert_eq!(world.env.market_state().1, group);
                assert_eq!(
                    world
                        .env
                        .svm
                        .get_account(&world.env.market)
                        .unwrap()
                        .lamports,
                    market_lamports + rent
                );
                assert!(world
                    .env
                    .svm
                    .get_account(&portfolio)
                    .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
                world.assert_frame_except(&before, &[world.env.market, portfolio]);
                world.custody();
            }
            let terminal = world.env.market_state().1;
            assert_eq!(terminal.materialized_portfolio_count, 0);
            assert_eq!(
                (
                    terminal.c_tot,
                    terminal.pnl_pos_tot,
                    terminal.insurance,
                    terminal.source_claim_bound_total_num,
                    terminal.backing_provider_earnings_total
                ),
                (0, 0, 0, 0, 0)
            );
            assert_eq!(terminal.vault, rounding);
            crate::support::fuzz_model::assert_reservation_encumbrance_census(
                "spent receipt terminal stock",
                &terminal,
                &[],
            )
            .unwrap();

            let before = world.frame();
            let market_rent = world
                .env
                .svm
                .get_account(&world.env.market)
                .unwrap()
                .lamports;
            let vault_rent = world
                .env
                .svm
                .get_account(&world.env.vault)
                .unwrap()
                .lamports;
            let admin_lamports = world
                .env
                .svm
                .get_account(&world.env.admin.pubkey())
                .unwrap()
                .lamports;
            let close = Instruction {
                program_id: world.env.program_id,
                accounts: vec![
                    AccountMeta::new(world.env.admin.pubkey(), true),
                    AccountMeta::new(world.env.market, false),
                    AccountMeta::new(world.env.vault, false),
                    AccountMeta::new_readonly(world.env.vault_authority, false),
                    AccountMeta::new(world.provider_token, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(world.env.mint, false),
                ],
                data: ProgInstruction::CloseSlab {
                    authority_epoch: world.env.control_sequences(0).authority_epoch,
                }
                .encode(),
            };
            let mut closed = false;
            for _ in 0..8 {
                let step = world.frame();
                let meta = world.land(&[close.clone()], true).unwrap();
                assert_cu_within(
                    "spent receipt slab close",
                    meta.compute_units_consumed,
                    CUSTODY_CU_LIMIT,
                );
                assert_ne!(world.frame(), step);
                let market = world.env.svm.get_account(&world.env.market).unwrap();
                if market.data.len() == percolator_prog::constants::HEADER_LEN {
                    assert_closed_market_tombstone(&market);
                    closed = true;
                    break;
                }
                world.assert_frame_except(&step, &[world.env.market]);
                world.custody();
            }
            assert!(closed, "paid nonzero-face history must reach CloseSlab");
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
                admin_lamports + market_rent + vault_rent - rent
            );
            assert!(world
                .env
                .svm
                .get_account(&world.env.vault)
                .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
            world.assert_frame_except(
                &before,
                &[
                    world.env.market,
                    world.env.vault,
                    world.env.mint,
                    world.env.admin.pubkey(),
                ],
            );
            let supply = u128::from(
                Mint::unpack(&world.env.svm.get_account(&world.env.mint).unwrap().data)
                    .unwrap()
                    .supply,
            );
            assert_eq!(supply, SUPPLY - rounding);
            let wallets: [u128; 5] = std::array::from_fn(|actor| {
                u128::from(world.env.token_amount(world.actors[actor].token))
            });
            assert_eq!(wallets, [0, expected[0] + expected[4], expected[2], 0, 0]);
            assert_eq!(
                wallets.iter().sum::<u128>()
                    + u128::from(world.env.token_amount(world.provider_token)),
                supply
            );
            outcomes.push((paid, spent, wallets, supply));
            peak_cu = peak_cu.max(world.peak_cu);
            println!("INV-067 spent receipts order={order:?}, rollback={rollback}: entitlements={paid:?}, wallets={wallets:?}, burn={rounding}; terminal tombstone");
        }
    }
    assert!(outcomes.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!(rollbacks, 4);
    println!("INV-067 spent receipts: 4 worlds, {rollbacks} exact rollbacks, peak CU {peak_cu}");
}
