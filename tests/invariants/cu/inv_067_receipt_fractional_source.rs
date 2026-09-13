//! INV-067 / row 417: fractional conversion and later expiry of the SAME source.
//! Two owners share a 350-atom reserve against 1,000 face. One receives floor(122.5)
//! or floor(227.5); the remaining stock later improves three already-paid receipts.
//! Expected values use only public deposits/trades, never the engine's payout helpers.

use super::{late_expiry::World, *};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const FACE: [u128; 6] = [700, 0, 350, 0, 1_300, 650];
const CAPITAL: [u128; 6] = [1_000, 0, 350, 0, 1_000, 650];
const SOURCE_STOCK: u128 = 100 + 250;
const INITIAL_RESIDUAL: u128 = 500 + 1;
const SUPPLY: u128 = 3_852;

fn junior(actor: usize, first: usize, expired: bool) -> u128 {
    let converted = FACE[first] * SOURCE_STOCK / (FACE[2] + FACE[5]);
    let face = FACE[actor] - if actor == first { converted } else { 0 };
    let residual = INITIAL_RESIDUAL + if expired { SOURCE_STOCK - converted } else { 0 };
    face * residual / (FACE.iter().sum::<u128>() - converted)
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
        "actor {actor}, expected transfer {due}"
    );
    paid[actor] += due;
    assert_eq!(
        u128::from(vault - world.env.token_amount(world.env.vault)),
        due
    );
    for (actor, amount) in paid.iter().enumerate() {
        assert_eq!(
            u128::from(world.env.token_amount(world.actors[actor].token)),
            *amount
        );
    }
    let mut allowed = vec![world.env.market, world.actors[actor].portfolio];
    if due != 0 {
        allowed.extend([world.env.vault, world.actors[actor].token]);
    }
    world.assert_frame_except(&before, &allowed);
    world.custody();
}

fn materialize(world: &mut World, actor: usize, due: u128, paid: &mut [u128; 6]) {
    let ix = world.payout(actor, false);
    for _ in 0..8 {
        if world.receipt(actor).present {
            break;
        }
        let before = world.frame();
        world.land(&[ix.clone()], false).unwrap();
        assert_ne!(
            world.frame(),
            before,
            "bounded receipt preparation must progress"
        );
        if world.receipt(actor).present {
            paid[actor] += due;
        }
        for (owner, amount) in paid.iter().enumerate() {
            assert_eq!(
                u128::from(world.env.token_amount(world.actors[owner].token)),
                *amount
            );
        }
        world.assert_frame_except(
            &before,
            &[
                world.env.market,
                world.env.vault,
                world.actors[actor].portfolio,
                world.actors[actor].token,
            ],
        );
        world.custody();
    }
    assert!(
        world.receipt(actor).present,
        "receipt must materialize within eight calls"
    );
}

fn check_receipts(
    world: &World,
    claimants: [usize; 3],
    originals: &[ResolvedPayoutReceiptV16; 3],
    paid: &[u128; 6],
    first: usize,
) {
    let converted = FACE[first] * SOURCE_STOCK / (FACE[2] + FACE[5]);
    for (index, actor) in claimants.into_iter().enumerate() {
        let mut expected = originals[index];
        expected.paid_effective =
            paid[actor] - CAPITAL[actor] - if actor == first { converted } else { 0 };
        assert_eq!(
            world.receipt(actor),
            expected,
            "immutable receipt identity for {actor}"
        );
    }
}

fn check_stock(world: &World, first: usize, expired: bool, second_paid: bool) {
    let second = if first == 2 { 5 } else { 2 };
    let converted = FACE[first] * SOURCE_STOCK / (FACE[2] + FACE[5]);
    let denominator = FACE.iter().sum::<u128>() - converted;
    let residual = INITIAL_RESIDUAL + if expired { SOURCE_STOCK - converted } else { 0 };
    let unreceipted = if second_paid { 0 } else { FACE[second] };
    let group = world.env.market_state().1;
    let ledger = group.resolved_payout_ledger;
    assert_eq!(ledger.snapshot_slot, 12);
    assert_eq!(ledger.snapshot_residual, residual);
    assert_eq!(ledger.current_payout_rate_num, residual * BOUND_SCALE);
    assert_eq!(ledger.current_payout_rate_den, denominator * BOUND_SCALE);
    assert_eq!(
        ledger.terminal_claim_bound_unreceipted_num,
        unreceipted * BOUND_SCALE
    );
    assert_eq!(
        ledger.terminal_claim_exact_receipts_num,
        (denominator - unreceipted) * BOUND_SCALE
    );
    assert!(!ledger.payout_halted && !ledger.finalized);
    assert_eq!(group.c_tot, if second_paid { 0 } else { CAPITAL[second] });
    assert_eq!(
        (group.insurance, group.backing_provider_earnings_total),
        (0, 0)
    );
    assert_eq!(
        group.source_claim_bound_total_num,
        unreceipted * BOUND_SCALE
    );
    let source = group.source_credit[3];
    assert_eq!(source.positive_claim_bound_num, unreceipted * BOUND_SCALE);
    assert_eq!(source.provider_receivable_num, converted * BOUND_SCALE);
    assert_eq!(
        source.fresh_reserved_backing_num,
        if expired { 0 } else { SOURCE_STOCK - converted } * BOUND_SCALE
    );
    let bucket = group.source_backing_buckets[3];
    assert_eq!(bucket.expiry_slot, 13);
    assert_eq!(bucket.consumed_liened_backing_num, converted * BOUND_SCALE);
    assert_eq!(
        bucket.status,
        if expired {
            BackingBucketStatusV16::Expired
        } else {
            BackingBucketStatusV16::Fresh
        }
    );
    assert_eq!(world.env.token_amount(world.provider_token), 1);
    world.custody();
}

#[test]
fn v16_program_fractional_source_conversion_preserves_receipts_through_same_bucket_expiry() {
    let mut peak_cu = 0;
    let mut worlds = 0;
    for first in [2, 5] {
        let second = if first == 2 { 5 } else { 2 };
        let converted = FACE[first] * SOURCE_STOCK / (FACE[2] + FACE[5]);
        assert_eq!(FACE[first] * SOURCE_STOCK % (FACE[2] + FACE[5]), 500);
        assert_eq!(converted, if first == 2 { 122 } else { 227 });
        let mut endpoint = None;
        for landing in [13, 15] {
            let mut world = World::before_receipts_with_split_source_claimants();
            let mut paid = [0; 6];
            for actor in [0, 4] {
                let due = CAPITAL[actor] + FACE[actor] * INITIAL_RESIDUAL / 3_000;
                materialize(&mut world, actor, due, &mut paid);
                assert_eq!(world.receipt(actor).paid_effective, due - CAPITAL[actor]);
            }
            let old_receipts = [world.receipt(0), world.receipt(4)];
            let claimants = [0, first, 4];
            let retained = claimants.map(|actor| world.payout(actor, true));
            let source_close = world.payout(second, false);
            let identities: Vec<_> = world
                .actors
                .iter()
                .map(|actor| {
                    (
                        world.env.portfolio_id(actor.portfolio),
                        world.env.portfolio_position_epoch(actor.portfolio),
                        state::read_portfolio_owner_preflight(
                            &world.env.svm.get_account(&actor.portfolio).unwrap().data,
                        )
                        .unwrap(),
                    )
                })
                .collect();
            assert_eq!(
                world.env.market_state().1.source_credit[3].fresh_reserved_backing_num,
                SOURCE_STOCK * BOUND_SCALE
            );
            assert_eq!(
                world.env.market_state().1.source_credit[3].positive_claim_bound_num,
                1_000 * BOUND_SCALE
            );
            world.peak_cu = 0;

            // This fractional conversion and payment freeze a third receipt while
            // leaving the peer's share in the very same Fresh backing bucket.
            materialize(
                &mut world,
                first,
                CAPITAL[first] + converted + junior(first, first, false),
                &mut paid,
            );
            assert_eq!([world.receipt(0), world.receipt(4)], old_receipts);
            let originals = claimants.map(|actor| world.receipt(actor));
            for (index, actor) in claimants.into_iter().enumerate() {
                let receipt = originals[index];
                let face = FACE[actor] - if actor == first { converted } else { 0 };
                assert!(receipt.present && !receipt.finalized);
                assert_eq!(receipt.terminal_positive_claim_face, face);
                assert_eq!(receipt.prior_bound_contribution_num, face * BOUND_SCALE);
                assert_eq!(receipt.live_released_face_at_receipt, 0);
            }
            check_receipts(&world, claimants, &originals, &paid, first);
            check_stock(&world, first, false, false);
            for (index, actor) in claimants.into_iter().enumerate() {
                let prior =
                    paid[actor] - CAPITAL[actor] - if actor == first { converted } else { 0 };
                let due = junior(actor, first, false) - prior;
                assert_eq!(due == 0, actor == first);
                pay(&mut world, &retained[index], actor, due, &mut paid);
                check_receipts(&world, claimants, &originals, &paid, first);
                check_stock(&world, first, false, false);
            }
            let early_receipts = claimants.map(|actor| world.receipt(actor));
            world.env.svm.warp_to_slot(landing);
            let before = world.frame();
            world.land(&retained, false).unwrap();
            world.assert_frame_except(&before, &[world.env.market]);
            assert_eq!(claimants.map(|actor| world.receipt(actor)), early_receipts);
            check_stock(&world, first, false, false);

            // Expiry, bound replacement and all four payments execute before a
            // normal rejected suffix. Retry must start from the committed conversion.
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
                        retained[2].clone(),
                        Instruction {
                            program_id: solana_sdk::system_program::ID,
                            accounts: vec![],
                            data: vec![],
                        },
                    ],
                    false,
                )
                .expect_err("reject after fractional-source expiry and receipt payments");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(7, InstructionError::InvalidInstructionData)
            );
            for (program, count) in [(world.env.program_id, 5), (spl_token::ID, 4)] {
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
            assert_eq!(world.frame(), before);
            assert_eq!(
                world.env.svm.get_account(&world.env.payer.pubkey()),
                Some(payer)
            );
            check_stock(&world, first, false, false);

            pay(&mut world, &source_close, second, 0, &mut paid);
            assert_eq!(claimants.map(|actor| world.receipt(actor)), early_receipts);
            check_stock(&world, first, true, false);
            pay(
                &mut world,
                &source_close,
                second,
                CAPITAL[second] + junior(second, first, true),
                &mut paid,
            );
            assert!(!world.receipt(second).present);
            assert_eq!(claimants.map(|actor| world.receipt(actor)), early_receipts);
            check_stock(&world, first, true, true);
            for (index, actor) in claimants.into_iter().enumerate().rev() {
                let due = junior(actor, first, true) - early_receipts[index].paid_effective;
                assert!(
                    due > 0,
                    "even the fractional-conversion receipt still has due"
                );
                pay(&mut world, &retained[index], actor, due, &mut paid);
                check_receipts(&world, claimants, &originals, &paid, first);
                check_stock(&world, first, true, true);
            }
            let expected = std::array::from_fn(|actor| {
                CAPITAL[actor]
                    + junior(actor, first, true)
                    + if actor == first { converted } else { 0 }
            });
            assert_eq!(paid, expected);
            let residue = INITIAL_RESIDUAL + SOURCE_STOCK
                - converted
                - (0..6).map(|actor| junior(actor, first, true)).sum::<u128>();
            assert_eq!(residue, 2);
            assert_eq!(world.env.market_state().1.vault, residue);
            assert_eq!(paid.iter().sum::<u128>() + residue + 1, SUPPLY);
            let result = (paid, world.env.market_state().1.resolved_payout_ledger);
            assert_eq!(
                *endpoint.get_or_insert(result),
                result,
                "exact and late expiry agree"
            );
            for (actor, identity) in world.actors.iter().zip(&identities) {
                assert_eq!(
                    &(
                        world.env.portfolio_id(actor.portfolio),
                        world.env.portfolio_position_epoch(actor.portfolio),
                        state::read_portfolio_owner_preflight(
                            &world.env.svm.get_account(&actor.portfolio).unwrap().data
                        )
                        .unwrap(),
                    ),
                    identity
                );
            }

            for (index, actor) in claimants.into_iter().enumerate() {
                pay(&mut world, &retained[index], actor, 0, &mut paid);
                assert_eq!(world.receipt(actor), ResolvedPayoutReceiptV16::default());
            }
            let retries: Vec<_> = [0, 2, 4, 5].map(|actor| world.payout(actor, true)).into();
            let before = world.frame();
            world.land(&retries, false).unwrap();
            world.land(&retries, false).unwrap();
            assert_eq!(
                world.frame(),
                before,
                "expired stock cannot repay cleared claims"
            );
            check_stock(&world, first, true, true);
            for actor in 0..6 {
                let portfolio = world.actors[actor].portfolio;
                assert!(resolved_portfolio_is_terminal(&world.env, portfolio));
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
                world.peak_cu = world.peak_cu.max(cu);
                assert_cu_within("fractional-source portfolio close", cu, CUSTODY_CU_LIMIT);
                assert!(world
                    .env
                    .svm
                    .get_account(&portfolio)
                    .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
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
            assert_eq!(
                (
                    group.materialized_portfolio_count,
                    group.c_tot,
                    group.pnl_pos_tot,
                    group.source_claim_bound_total_num
                ),
                (0, 0, 0, 0)
            );
            check_stock(&world, first, true, true);
            crate::support::fuzz_model::assert_reservation_encumbrance_census(
                "INV-067 fractional source terminal",
                &group,
                &[],
            )
            .unwrap();
            assert_cu_within("fractional-source settlement", world.peak_cu, 900_000);
            peak_cu = peak_cu.max(world.peak_cu);
            worlds += 1;
            println!("INV-067 fractional source: first={first}, landing={landing}, converted={converted}, paid={paid:?}, residue={residue}");
        }
    }
    assert_eq!(worlds, 4);
    println!("INV-067 fractional source: {worlds} worlds, 4 exact rollbacks, 24 portfolio closes, peak {peak_cu} CU");
}
