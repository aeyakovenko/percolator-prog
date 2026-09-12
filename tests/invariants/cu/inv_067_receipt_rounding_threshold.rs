//! Row 417: a late rate increase can consume prior rounding stock and empty custody
//! before paid receipts are cleared. Receipt retirement must remain exact at that boundary.

use super::{late_expiry::World, *};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const CLAIMANTS: [usize; 3] = [0, 2, 4];
const FACES: [u128; 3] = [700, 1_000, 1_300];
const TOTAL_FACE: u128 = 3_000;
const INITIAL: u128 = 501;
const SUPPLY: u128 = 3_852;
const ORDERS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

fn floors(residual: u128) -> [u128; 3] {
    FACES.map(|face| face * residual / TOTAL_FACE)
}

fn reject_tail(world: &mut World, prefix: &[Instruction], admin: bool, transfers: usize) {
    let before = world.frame();
    let mut payer = world
        .env
        .svm
        .get_account(&world.env.payer.pubkey())
        .unwrap();
    payer.lamports -= FeeStructure::default().lamports_per_signature * if admin { 2 } else { 1 };
    let mut instructions = prefix.to_vec();
    instructions.push(Instruction {
        program_id: solana_sdk::system_program::ID,
        accounts: vec![],
        data: vec![],
    });
    let failure = world.land(&instructions, admin).unwrap_err();
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(
            2 + prefix.len() as u8,
            InstructionError::InvalidInstructionData,
        )
    );
    for (program, count) in [
        (world.env.program_id, prefix.len()),
        (spl_token::ID, transfers),
    ] {
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {program} success"))
                .count(),
            count,
            "the entire successful prefix must execute before rollback"
        );
    }
    assert_eq!(world.frame(), before);
    assert_eq!(
        world
            .env
            .svm
            .get_account(&world.env.payer.pubkey())
            .unwrap(),
        payer
    );
    world.custody();
}

fn check_ledger(world: &World, residual: u128, unreceipted: u128) {
    let ledger = world.env.market_state().1.resolved_payout_ledger;
    assert_eq!(ledger.snapshot_slot, 12);
    assert_eq!(ledger.snapshot_residual, residual);
    assert_eq!(ledger.current_payout_rate_num, residual * BOUND_SCALE);
    assert_eq!(ledger.current_payout_rate_den, TOTAL_FACE * BOUND_SCALE);
    assert_eq!(
        ledger.terminal_claim_bound_unreceipted_num,
        unreceipted * BOUND_SCALE
    );
    assert_eq!(
        ledger.terminal_claim_exact_receipts_num,
        (TOTAL_FACE - unreceipted) * BOUND_SCALE
    );
    assert!(!ledger.payout_halted && !ledger.finalized);
}

#[test]
fn v16_program_late_receipt_rounding_threshold_preserves_zero_vault_settlement() {
    // One new backing atom unlocks three owner atoms at this threshold. The other
    // two come from prior floor residue, never from a changed face or an overpayment.
    assert_eq!(floors(839), [195, 279, 363]);
    assert_eq!(floors(840), [196, 280, 364]);
    assert_eq!(floors(841), floors(840));
    let mut peak_cu = 0;
    let mut worlds = 0;
    let mut rejections = 0;
    for backing in [88u128, 89, 90] {
        let residual = INITIAL + 250 + backing;
        let expected = floors(residual);
        let rounding = residual - expected.iter().sum::<u128>();
        assert_eq!(rounding, [2, 0, 1][(backing - 88) as usize]);
        for landing in [13, 15] {
            for order in ORDERS {
                let mut world = World::before_receipts_with_backing(backing);
                for actor in [0, 4] {
                    for _ in 0..8 {
                        if world.receipt(actor).present {
                            break;
                        }
                        world.land(&[world.payout(actor, false)], false).unwrap();
                    }
                }
                let original = [world.receipt(0), world.receipt(4)];
                for (index, receipt) in original.iter().enumerate() {
                    let claimant = index * 2;
                    assert!(receipt.present && !receipt.finalized);
                    assert_eq!(receipt.terminal_positive_claim_face, FACES[claimant]);
                    assert_eq!(
                        receipt.prior_bound_contribution_num,
                        FACES[claimant] * BOUND_SCALE
                    );
                    assert_eq!(receipt.live_released_face_at_receipt, 0);
                    assert_eq!(receipt.paid_effective, floors(INITIAL)[claimant]);
                }
                check_ledger(&world, INITIAL, FACES[1]);
                let identities = CLAIMANTS.map(|actor| {
                    let portfolio = world.actors[actor].portfolio;
                    (
                        world.env.portfolio_id(portfolio),
                        world.env.portfolio_position_epoch(portfolio),
                        state::read_portfolio_owner_preflight(
                            &world.env.svm.get_account(&portfolio).unwrap().data,
                        )
                        .unwrap(),
                    )
                });
                let retained = CLAIMANTS.map(|actor| world.payout(actor, actor != 2));
                world.peak_cu = 0;
                world.env.svm.warp_to_slot(landing);
                let before = world.frame();
                world
                    .land(&[retained[0].clone(), retained[2].clone()], false)
                    .unwrap();
                world.assert_frame_except(&before, &[world.env.market]);
                assert_eq!([world.receipt(0), world.receipt(4)], original);
                check_ledger(&world, INITIAL, FACES[1]);

                // The first source close only normalizes expiry. Its exact replay must
                // still be able to replace the last bound and pay the third claimant.
                let before = world.frame();
                world.land(&[retained[1].clone()], false).unwrap();
                world.assert_frame_except(&before, &[world.env.market, world.actors[2].portfolio]);
                assert_eq!([world.receipt(0), world.receipt(4)], original);
                assert!(!world.receipt(2).present);
                check_ledger(&world, residual, FACES[1]);
                let group = world.env.market_state().1;
                assert_eq!(
                    group.source_backing_buckets[3].status,
                    BackingBucketStatusV16::Expired
                );
                assert_eq!(group.source_credit[3].fresh_reserved_backing_num, 0);
                assert_eq!(world.env.token_amount(world.actors[2].token), 0);

                let payments = order.map(|index| retained[index].clone());
                reject_tail(&mut world, &payments, false, 3);
                rejections += 1;
                let mut paid = [floors(INITIAL)[0], 0, floors(INITIAL)[2]];
                let mut capital_paid = [true, false, true];
                let mut unreceipted = FACES[1];
                for index in order {
                    let actor = CLAIMANTS[index];
                    let before = world.frame();
                    let due =
                        expected[index] - paid[index] + if capital_paid[index] { 0 } else { 1_000 };
                    let tokens = world.env.token_amount(world.actors[actor].token);
                    let vault = world.env.token_amount(world.env.vault);
                    world.land(&[retained[index].clone()], false).unwrap();
                    paid[index] = expected[index];
                    capital_paid[index] = true;
                    if index == 1 {
                        unreceipted = 0;
                        assert!(!world.receipt(actor).present);
                    }
                    assert_eq!(
                        u128::from(world.env.token_amount(world.actors[actor].token) - tokens),
                        due
                    );
                    assert_eq!(
                        u128::from(vault - world.env.token_amount(world.env.vault)),
                        due
                    );
                    world.assert_frame_except(
                        &before,
                        &[
                            world.env.market,
                            world.env.vault,
                            world.actors[actor].portfolio,
                            world.actors[actor].token,
                        ],
                    );
                    check_ledger(&world, residual, unreceipted);
                    for (old_index, old_actor) in [0, 4].into_iter().enumerate() {
                        let mut receipt = original[old_index];
                        receipt.paid_effective = paid[old_index * 2];
                        assert_eq!(world.receipt(old_actor), receipt);
                    }
                    for (claimant, claim_actor) in CLAIMANTS.into_iter().enumerate() {
                        let portfolio = world.actors[claim_actor].portfolio;
                        assert_eq!(
                            (
                                world.env.portfolio_id(portfolio),
                                world.env.portfolio_position_epoch(portfolio),
                                state::read_portfolio_owner_preflight(
                                    &world.env.svm.get_account(&portfolio).unwrap().data
                                )
                                .unwrap()
                            ),
                            identities[claimant]
                        );
                        assert_eq!(
                            u128::from(world.env.token_amount(world.actors[claim_actor].token)),
                            paid[claimant] + if capital_paid[claimant] { 1_000 } else { 0 }
                        );
                    }
                    assert_eq!(
                        world.env.market_state().1.vault,
                        3_000 + residual
                            - paid.iter().sum::<u128>()
                            - 1_000 * capital_paid.into_iter().filter(|paid| *paid).count() as u128
                    );
                    world.custody();
                }
                assert_eq!(world.env.market_state().1.vault, rounding);
                assert_eq!(world.env.market_state().1.c_tot, 0);
                assert_eq!(
                    world.env.token_amount(world.provider_token) as u128,
                    101 - backing
                );
                assert_eq!(
                    SUPPLY,
                    3_000 + expected.iter().sum::<u128>() + rounding + 101 - backing
                );

                // Even at zero custody, a paid receipt still has one cleanup transition.
                // Roll back both clears together, then retire in reversed claimant order.
                let clears: Vec<_> = order
                    .into_iter()
                    .rev()
                    .filter(|index| *index != 1)
                    .map(|index| retained[index].clone())
                    .collect();
                reject_tail(&mut world, &clears, false, 0);
                rejections += 1;
                let before = world.frame();
                world.land(&clears, false).unwrap();
                world.assert_frame_except(
                    &before,
                    &[world.actors[0].portfolio, world.actors[4].portfolio],
                );
                for actor in CLAIMANTS {
                    assert!(!world.receipt(actor).present);
                    assert!(resolved_portfolio_is_terminal(
                        &world.env,
                        world.actors[actor].portfolio
                    ));
                }
                let retries = [
                    world.payout(0, true),
                    world.payout(2, true),
                    world.payout(4, true),
                ];
                let before = world.frame();
                world.land(&retries, false).unwrap();
                world.land(&retries, false).unwrap();
                assert_eq!(world.frame(), before);
                check_ledger(&world, residual, 0);

                for actor in order
                    .map(|index| CLAIMANTS[index])
                    .into_iter()
                    .chain([1, 3])
                {
                    let portfolio = world.actors[actor].portfolio;
                    let before = world.frame();
                    let rent = world.env.svm.get_account(&portfolio).unwrap().lamports;
                    let market_rent = world
                        .env
                        .svm
                        .get_account(&world.env.market)
                        .unwrap()
                        .lamports;
                    let cu = world
                        .env
                        .close_portfolio_with_cu(&world.actors[actor].owner, portfolio);
                    peak_cu = peak_cu.max(cu);
                    assert_cu_within("threshold portfolio close", cu, CUSTODY_CU_LIMIT);
                    assert_eq!(
                        world
                            .env
                            .svm
                            .get_account(&world.env.market)
                            .unwrap()
                            .lamports,
                        market_rent + rent
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
                    [
                        terminal.c_tot,
                        terminal.pnl_pos_tot,
                        terminal.insurance,
                        terminal.source_claim_bound_total_num,
                        terminal.backing_provider_earnings_total
                    ],
                    [0; 5]
                );
                assert_eq!(terminal.vault, rounding);
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
                let admin_rent = world
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
                world.land(&[close], true).unwrap();
                let tombstone = world.env.svm.get_account(&world.env.market).unwrap();
                assert_closed_market_tombstone(&tombstone);
                let rent = world
                    .env
                    .svm
                    .get_sysvar::<solana_sdk::rent::Rent>()
                    .minimum_balance(percolator_prog::constants::HEADER_LEN);
                assert_eq!(tombstone.lamports, rent);
                assert_eq!(
                    world
                        .env
                        .svm
                        .get_account(&world.env.admin.pubkey())
                        .unwrap()
                        .lamports,
                    admin_rent + market_rent + vault_rent - rent
                );
                assert!(world
                    .env
                    .svm
                    .get_account(&world.env.vault)
                    .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
                let supply =
                    Mint::unpack(&world.env.svm.get_account(&world.env.mint).unwrap().data)
                        .unwrap()
                        .supply;
                assert_eq!(u128::from(supply), SUPPLY - rounding);
                assert_eq!(
                    u128::from(supply),
                    CLAIMANTS
                        .into_iter()
                        .map(|actor| u128::from(world.env.token_amount(world.actors[actor].token)))
                        .sum::<u128>()
                        + u128::from(world.env.token_amount(world.provider_token))
                );
                world.assert_frame_except(
                    &before,
                    &[
                        world.env.market,
                        world.env.vault,
                        world.env.mint,
                        world.env.admin.pubkey(),
                    ],
                );
                peak_cu = peak_cu.max(world.peak_cu);
                assert_cu_within("threshold settlement transaction", world.peak_cu, 600_000);
                worlds += 1;
            }
        }
    }
    assert_eq!((worlds, rejections), (36, 72));
    println!("INV-067 rounding threshold: {worlds} worlds, {rejections} exact rollbacks, 36 slab closes, peak {peak_cu} CU");
}
