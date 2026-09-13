//! INV-010/024/029/063/066/067/068/070, row 417: paid receipt identity survives
//! a committed denominator refinement followed by a different source's late expiry.
//! Unlike repeated_stock (two expiry releases) and aborted_realization (first expiry,
//! then rejected Fresh conversion), this history retains the first conversion and its
//! top-ups across the later numerator increase. Neither converted stock nor its removed
//! claim face may rejoin the pool. All setup and changes use public System/SPL/wrapper
//! instructions; expected amounts below come from the fixture's deposits and trades.
//! This finite mixed-disposition product adds evidence; row 417 remains OPEN.

use super::{late_expiry::World, *};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const FACES: [u128; 6] = [700, 0, 1_000, 0, 1_300, 0];
const CONVERTED: u128 = 61 + 100;
const LATE_STOCK: u128 = 39 + 150;
const DENOMINATOR: u128 = 3_000 - CONVERTED;
const SUPPLY: u128 = 3_852;
const ORDERS: [[usize; 3]; 6] = [
    [0, 2, 4],
    [0, 4, 2],
    [2, 0, 4],
    [2, 4, 0],
    [4, 0, 2],
    [4, 2, 0],
];

fn junior(actor: usize, expired: bool) -> u128 {
    let face = FACES[actor] - if actor == 2 { CONVERTED } else { 0 };
    face * (501 + if expired { LATE_STOCK } else { 0 }) / DENOMINATOR
}

fn pay(world: &mut World, ix: &Instruction, actor: usize, due: u128, paid: &mut [u128; 6]) {
    let before = world.frame();
    let vault = world.env.token_amount(world.env.vault);
    let meta = world.land(&[ix.clone()], false).unwrap();
    assert_eq!(
        meta.logs
            .iter()
            .filter(|line| **line == format!("Program {} success", spl_token::ID))
            .count(),
        usize::from(due != 0),
        "only a positive due transfers tokens"
    );
    paid[actor] += due;
    assert_eq!(
        u128::from(vault - world.env.token_amount(world.env.vault)),
        due
    );
    let mut allowed = vec![world.env.market, world.actors[actor].portfolio];
    if due != 0 {
        allowed.extend([world.env.vault, world.actors[actor].token]);
    }
    world.assert_frame_except(&before, &allowed);
    for (actor, amount) in paid.iter().enumerate() {
        assert_eq!(
            u128::from(world.env.token_amount(world.actors[actor].token)),
            *amount
        );
    }
    world.custody();
}

fn check(
    world: &World,
    original: &[ResolvedPayoutReceiptV16; 2],
    paid: &[u128; 6],
    expired: bool,
    replaced: bool,
) {
    let group = world.env.market_state().1;
    let ledger = group.resolved_payout_ledger;
    let residual = 501 + if expired { LATE_STOCK } else { 0 };
    assert_eq!(ledger.snapshot_slot, 12);
    assert_eq!(ledger.snapshot_residual, residual);
    assert_eq!(ledger.current_payout_rate_num, residual * BOUND_SCALE);
    assert_eq!(ledger.current_payout_rate_den, DENOMINATOR * BOUND_SCALE);
    assert_eq!(
        ledger.terminal_claim_bound_unreceipted_num,
        if replaced {
            0
        } else {
            (1_000 - CONVERTED) * BOUND_SCALE
        }
    );
    assert_eq!(
        ledger.terminal_claim_exact_receipts_num,
        if replaced { DENOMINATOR } else { 2_000 } * BOUND_SCALE
    );
    assert!(!ledger.payout_halted && !ledger.finalized);
    assert_eq!(group.c_tot, if replaced { 0 } else { 1_000 + CONVERTED });
    assert_eq!(
        group.source_claim_bound_total_num,
        if replaced { 0 } else { 600 * BOUND_SCALE }
    );
    assert_eq!(
        (group.insurance, group.backing_provider_earnings_total),
        (0, 0)
    );
    assert_eq!(group.materialized_portfolio_count, 6);
    assert_eq!(group.vault, SUPPLY - 1 - paid.iter().sum::<u128>());
    assert_eq!(world.env.token_amount(world.provider_token), 1);
    for (domain, expiry, consumed, reserve, bound) in [
        (3, 13, CONVERTED, 0, 0),
        (
            5,
            15,
            0,
            if expired { 0 } else { LATE_STOCK },
            if replaced { 0 } else { 600 },
        ),
    ] {
        let bucket = group.source_backing_buckets[domain];
        let source = group.source_credit[domain];
        assert_eq!(bucket.expiry_slot, expiry);
        assert_eq!(
            bucket.status,
            if reserve == 0 {
                BackingBucketStatusV16::Expired
            } else {
                BackingBucketStatusV16::Fresh
            }
        );
        assert_eq!(bucket.consumed_liened_backing_num, consumed * BOUND_SCALE);
        assert_eq!(source.provider_receivable_num, consumed * BOUND_SCALE);
        assert_eq!(source.fresh_reserved_backing_num, reserve * BOUND_SCALE);
        assert_eq!(source.positive_claim_bound_num, bound * BOUND_SCALE);
    }
    for (index, actor) in [0, 4].into_iter().enumerate() {
        let mut receipt = original[index];
        receipt.paid_effective = paid[actor] - 1_000;
        assert_eq!(
            world.receipt(actor),
            receipt,
            "immutable receipt for claimant {actor}"
        );
        assert!(receipt.present && !receipt.finalized);
    }
    assert!(!world.receipt(2).present);
    let source = world.env.portfolio_state(world.actors[2].portfolio);
    assert_eq!(
        source.pnl.get(),
        if replaced {
            0
        } else {
            (1_000 - CONVERTED) as i128
        }
    );
    assert_eq!(
        source.reserved_pnl.get(),
        if replaced { 0 } else { 400 - CONVERTED }
    );
    assert_eq!(
        resolved_portfolio_is_terminal(&world.env, world.actors[2].portfolio),
        replaced
    );
    world.custody();
}

#[test]
fn v16_program_committed_conversion_then_late_expiry_preserves_receipt_attribution() {
    let mut peak_cu = 0;
    let mut endpoint = None;
    for landing in [15, 17] {
        for order in ORDERS {
            let mut world = World::before_receipts_with_staggered_sources();
            let mut paid = [0; 6];
            for actor in [0, 4] {
                for _ in 0..8 {
                    if world.receipt(actor).present {
                        break;
                    }
                    world.land(&[world.payout(actor, false)], false).unwrap();
                }
                paid[actor] = 1_000 + FACES[actor] * 501 / 3_000;
                let receipt = world.receipt(actor);
                assert!(receipt.present && !receipt.finalized);
                assert_eq!(receipt.terminal_positive_claim_face, FACES[actor]);
                assert_eq!(
                    receipt.prior_bound_contribution_num,
                    FACES[actor] * BOUND_SCALE
                );
                assert_eq!(receipt.live_released_face_at_receipt, 0);
                assert_eq!(receipt.paid_effective, paid[actor] - 1_000);
            }
            let original = [world.receipt(0), world.receipt(4)];
            let identities = [0, 2, 4].map(|actor| {
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
            // Retain all request bytes before either stock change.
            let retained = [world.payout(0, true), world.payout(4, true)];
            let source_close = world.payout(2, false);
            let source_topup = world.payout(2, true);
            world.peak_cu = 0;
            pay(&mut world, &source_close, 2, 0, &mut paid);
            check(&world, &original, &paid, false, false);
            assert_eq!(
                world
                    .env
                    .portfolio_state(world.actors[2].portfolio)
                    .capital
                    .get(),
                1_161
            );

            // The first conversion shrinks only the unreceipted denominator. Pay its
            // rate increase now, in reverse of each world's eventual claimant order.
            for actor in order.into_iter().rev().filter(|&actor| actor != 2) {
                let due = junior(actor, false) - (paid[actor] - 1_000);
                assert!(due > 0);
                pay(&mut world, &retained[actor / 4], actor, due, &mut paid);
                check(&world, &original, &paid, false, false);
            }
            assert_eq!(paid, [1_123, 0, 0, 0, 1_229, 0]);

            world.env.svm.warp_to_slot(landing);
            let before = world.frame();
            world.land(&retained, false).unwrap();
            world.assert_frame_except(&before, &[world.env.market]);
            check(&world, &original, &paid, false, false);

            // Atomic rollback must restore this already-converted, already-paid
            // checkpoint, including the 839 remaining face and the 161 receivable.
            // The prefix normalizes expiry, replaces the remaining bound, and pays
            // all three owners before an ordinary invalid transaction suffix rejects.
            let before = world.frame();
            let mut payer = world
                .env
                .svm
                .get_account(&world.env.payer.pubkey())
                .unwrap();
            payer.lamports -= FeeStructure::default().lamports_per_signature;
            let failure = world
                .land(
                    &[
                        source_close.clone(),
                        source_close.clone(),
                        retained[0].clone(),
                        retained[1].clone(),
                        Instruction {
                            program_id: solana_sdk::system_program::ID,
                            accounts: vec![],
                            data: vec![],
                        },
                    ],
                    false,
                )
                .expect_err("reject after mixed-stock receipt replacement and top-ups");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(6, InstructionError::InvalidInstructionData)
            );
            for (program, count) in [(world.env.program_id, 4), (spl_token::ID, 3)] {
                assert_eq!(
                    failure
                        .meta
                        .logs
                        .iter()
                        .filter(|line| **line == format!("Program {program} success"))
                        .count(),
                    count
                );
            }
            assert_eq!(world.frame(), before, "exact mixed-stock rollback");
            assert_eq!(
                world.env.svm.get_account(&world.env.payer.pubkey()),
                Some(payer)
            );
            check(&world, &original, &paid, false, false);

            pay(&mut world, &source_close, 2, 0, &mut paid);
            check(&world, &original, &paid, true, false);
            let mut replaced = false;
            for actor in order {
                if actor == 2 {
                    pay(
                        &mut world,
                        &source_close,
                        2,
                        1_000 + CONVERTED + junior(2, true),
                        &mut paid,
                    );
                    replaced = true;
                } else {
                    let due = junior(actor, true) - (paid[actor] - 1_000);
                    assert!(due > 0);
                    pay(&mut world, &retained[actor / 4], actor, due, &mut paid);
                }
                check(&world, &original, &paid, true, replaced);
            }
            assert_eq!(paid, [1_170, 0, 1_364, 0, 1_315, 0]);
            let rounding = 501 + LATE_STOCK
                - [0, 2, 4]
                    .map(|actor| junior(actor, true))
                    .iter()
                    .sum::<u128>();
            assert_eq!(rounding, 2);
            let ledger = world.env.market_state().1.resolved_payout_ledger;
            let result = (paid, ledger, world.env.token_amount(world.env.vault));
            assert_eq!(
                *endpoint.get_or_insert(result),
                result,
                "claimant order and late landing preserve economics"
            );

            for (index, actor) in [0, 2, 4].into_iter().enumerate() {
                let portfolio = world.actors[actor].portfolio;
                assert_eq!(
                    (
                        world.env.portfolio_id(portfolio),
                        world.env.portfolio_position_epoch(portfolio),
                        state::read_portfolio_owner_preflight(
                            &world.env.svm.get_account(&portfolio).unwrap().data
                        )
                        .unwrap()
                    ),
                    identities[index]
                );
                let ix = world.payout(actor, true);
                pay(&mut world, &ix, actor, 0, &mut paid);
                assert!(!world.receipt(actor).present);
                assert!(resolved_portfolio_is_terminal(&world.env, portfolio));
            }
            let before = world.frame();
            world
                .land(
                    &[
                        retained[1].clone(),
                        source_topup.clone(),
                        retained[0].clone(),
                    ],
                    false,
                )
                .unwrap();
            assert_eq!(
                world.frame(),
                before,
                "retired mixed-disposition claims cannot pay again"
            );

            for actor in order.into_iter().chain([1, 3, 5]) {
                let portfolio = world.actors[actor].portfolio;
                assert!(resolved_portfolio_is_terminal(&world.env, portfolio));
                let before = world.frame();
                let mut closed = world.env.svm.get_account(&portfolio).unwrap();
                let market_rent = world
                    .env
                    .svm
                    .get_account(&world.env.market)
                    .unwrap()
                    .lamports;
                let rent = closed.lamports;
                let cu = world
                    .env
                    .close_portfolio_with_cu(&world.actors[actor].owner, portfolio);
                world.peak_cu = world.peak_cu.max(cu);
                closed.lamports = 0;
                closed.data.clear();
                assert_eq!(world.env.svm.get_account(&portfolio), Some(closed));
                assert_eq!(
                    world
                        .env
                        .svm
                        .get_account(&world.env.market)
                        .unwrap()
                        .lamports,
                    market_rent + rent
                );
                world.assert_frame_except(&before, &[world.env.market, portfolio]);
                world.custody();
            }
            let group = world.env.market_state().1;
            assert_eq!(group.materialized_portfolio_count, 0);
            assert_eq!(
                (
                    group.c_tot,
                    group.pnl_pos_tot,
                    group.source_claim_bound_total_num,
                    group.insurance,
                    group.backing_provider_earnings_total
                ),
                (0, 0, 0, 0, 0)
            );
            assert_eq!(group.vault, rounding);
            assert_eq!(
                group.source_credit[3].provider_receivable_num,
                CONVERTED * BOUND_SCALE
            );
            assert_eq!(group.source_credit[5].provider_receivable_num, 0);
            crate::support::fuzz_model::assert_reservation_encumbrance_census(
                "INV-067 conversion then expiry terminal",
                &group,
                &[],
            )
            .unwrap();

            // The consumed receivable is historical attribution, not withdrawable
            // stock. CloseSlab must burn only the two unpaid rounding atoms and return
            // exact rent, without giving the converted 161 atoms to the provider again.
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
            let tombstone_rent = world
                .env
                .svm
                .get_sysvar::<solana_sdk::rent::Rent>()
                .minimum_balance(percolator_prog::constants::HEADER_LEN);
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
            world
                .land(&[close], true)
                .expect("mixed consumed/expired source history reaches slab retirement");
            let market = world.env.svm.get_account(&world.env.market).unwrap();
            assert_closed_market_tombstone(&market);
            assert_eq!(market.lamports, tombstone_rent);
            assert!(world
                .env
                .svm
                .get_account(&world.env.vault)
                .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
            assert_eq!(
                world
                    .env
                    .svm
                    .get_account(&world.env.admin.pubkey())
                    .unwrap()
                    .lamports,
                admin_rent + market_rent + vault_rent - tombstone_rent
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
            let mint =
                Mint::unpack(&world.env.svm.get_account(&world.env.mint).unwrap().data).unwrap();
            assert_eq!(u128::from(mint.supply), SUPPLY - rounding);
            assert_eq!(paid.iter().sum::<u128>() + 1 + rounding, SUPPLY);
            assert_cu_within("INV-067 conversion then expiry", world.peak_cu, 600_000);
            peak_cu = peak_cu.max(world.peak_cu);
        }
    }
    println!("INV-067 conversion then expiry: 12 worlds, 12 exact rollbacks, 72 portfolio closes, 12 slab closes; peak {peak_cu} CU");
}
