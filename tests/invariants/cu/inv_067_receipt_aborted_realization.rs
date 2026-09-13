//! Row 417: a rejected Fresh realization after an earlier committed expiry/top-up
//! must not contaminate the same retained claims when the remaining source expires.
//! All economic construction and transitions use the existing public LiteSVM seed.

use super::{late_expiry::World, *};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const FACES: [u128; 6] = [700, 0, 1_000, 0, 1_300, 0];
const FIRST_RESIDUAL: u128 = 501 + 61 + 100;
const SECOND_STOCK: u128 = 39 + 150;
const SUPPLY: u128 = 3_852;
const ORDERS: [[usize; 3]; 6] = [
    [0, 2, 4],
    [0, 4, 2],
    [2, 0, 4],
    [2, 4, 0],
    [4, 0, 2],
    [4, 2, 0],
];

fn junior(actor: usize, residual: u128, converted: u128) -> u128 {
    let face = FACES[actor] - if actor == 2 { converted } else { 0 };
    face * residual / (3_000 - converted)
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
        usize::from(due != 0)
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
    for (actor, expected) in paid.iter().enumerate() {
        assert_eq!(
            u128::from(world.env.token_amount(world.actors[actor].token)),
            *expected
        );
    }
    world.custody();
}

fn check(
    world: &World,
    original: &[ResolvedPayoutReceiptV16; 2],
    paid: &[u128; 6],
    expired: bool,
    closed: bool,
) {
    let converted = if closed && !expired { SECOND_STOCK } else { 0 };
    let residual = FIRST_RESIDUAL + if expired { SECOND_STOCK } else { 0 };
    let denominator = 3_000 - converted;
    let group = world.env.market_state().1;
    let ledger = group.resolved_payout_ledger;
    assert_eq!(ledger.snapshot_slot, 12);
    assert_eq!(ledger.snapshot_residual, residual);
    assert_eq!(ledger.current_payout_rate_num, residual * BOUND_SCALE);
    assert_eq!(ledger.current_payout_rate_den, denominator * BOUND_SCALE);
    assert_eq!(
        ledger.terminal_claim_exact_receipts_num,
        if closed { denominator } else { 2_000 } * BOUND_SCALE
    );
    assert_eq!(
        ledger.terminal_claim_bound_unreceipted_num,
        if closed { 0 } else { 1_000 * BOUND_SCALE }
    );
    assert!(!ledger.payout_halted && !ledger.finalized);
    assert_eq!(group.c_tot, if closed { 0 } else { 1_000 });
    assert_eq!(group.insurance, 0);
    assert_eq!(group.backing_provider_earnings_total, 0);
    assert_eq!(group.materialized_portfolio_count, 6);
    assert_eq!(group.vault, SUPPLY - 1 - paid.iter().sum::<u128>());
    assert_eq!(world.env.token_amount(world.provider_token), 1);
    assert_eq!(
        group.source_claim_bound_total_num,
        if closed { 0 } else { 600 * BOUND_SCALE }
    );
    for (domain, expiry, reserve, consumed, bound) in [
        (3, 13, 0, 0, 0),
        (
            5,
            15,
            if !expired && !closed { SECOND_STOCK } else { 0 },
            converted,
            if closed { 0 } else { 600 },
        ),
    ] {
        let bucket = group.source_backing_buckets[domain];
        let source = group.source_credit[domain];
        assert_eq!(bucket.expiry_slot, expiry);
        assert_eq!(
            bucket.status,
            if reserve != 0 {
                BackingBucketStatusV16::Fresh
            } else {
                BackingBucketStatusV16::Expired
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
        assert_eq!(world.receipt(actor), receipt, "immutable claimant {actor}");
        assert!(receipt.present && !receipt.finalized);
    }
    assert!(!world.receipt(2).present);
    let source = world.env.portfolio_state(world.actors[2].portfolio);
    assert_eq!(source.pnl.get(), if closed { 0 } else { 1_000 });
    assert_eq!(source.reserved_pnl.get(), if closed { 0 } else { 400 });
    assert_eq!(
        resolved_portfolio_is_terminal(&world.env, world.actors[2].portfolio),
        closed
    );
    world.custody();
}

#[test]
fn v16_program_aborted_second_source_realization_preserves_receipts_across_expiry_and_order() {
    let mut peak_cu = 0;
    let mut endpoints = [None; 3];
    for landing in [14, 15, 16] {
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
                paid[actor] = 1_000 + junior(actor, 501, 0);
            }
            let original = [world.receipt(0), world.receipt(4)];
            for (index, actor) in [0, 4].into_iter().enumerate() {
                let receipt = original[index];
                assert!(receipt.present && !receipt.finalized);
                assert_eq!(receipt.terminal_positive_claim_face, FACES[actor]);
                assert_eq!(
                    receipt.prior_bound_contribution_num,
                    FACES[actor] * BOUND_SCALE
                );
                assert_eq!(receipt.live_released_face_at_receipt, 0);
                assert_eq!(receipt.paid_effective, junior(actor, 501, 0));
            }
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
            let retained = [world.payout(0, true), world.payout(4, true)];
            let source_close = world.payout(2, false);
            world.peak_cu = 0;
            world.env.svm.warp_to_slot(13);
            pay(&mut world, &source_close, 2, 0, &mut paid);
            let group = world.env.market_state().1;
            assert_eq!(
                group.resolved_payout_ledger.snapshot_residual,
                FIRST_RESIDUAL
            );
            assert_eq!(
                group.source_credit[5].fresh_reserved_backing_num,
                SECOND_STOCK * BOUND_SCALE
            );
            assert_eq!([world.receipt(0), world.receipt(4)], original);
            for actor in order.into_iter().rev().filter(|&actor| actor != 2) {
                pay(
                    &mut world,
                    &retained[actor / 4],
                    actor,
                    junior(actor, FIRST_RESIDUAL, 0) - junior(actor, 501, 0),
                    &mut paid,
                );
            }
            // Retain the first expired source's 400-face haircut. The next close
            // now selects only the still-Fresh 600-face source with 189 backing.
            pay(&mut world, &source_close, 2, 0, &mut paid);
            check(&world, &original, &paid, false, false);
            assert_eq!(paid, [1_154, 0, 0, 0, 1_286, 0]);

            world.env.svm.warp_to_slot(14);
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
                .expect_err("rollback Fresh conversion and all three claimant payments");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(5, InstructionError::InvalidInstructionData,)
            );
            for program in [world.env.program_id, spl_token::ID] {
                assert_eq!(
                    failure
                        .meta
                        .logs
                        .iter()
                        .filter(|line| **line == format!("Program {program} success"))
                        .count(),
                    3,
                    "all three paying prefixes must execute"
                );
            }
            assert_eq!(
                world.frame(),
                before,
                "restore the already-paid first expiry checkpoint"
            );
            assert_eq!(
                world.env.svm.get_account(&world.env.payer.pubkey()),
                Some(payer)
            );
            check(&world, &original, &paid, false, false);

            let expired = landing >= 15;
            world.env.svm.warp_to_slot(landing);
            let converted = if expired { 0 } else { SECOND_STOCK };
            let residual = FIRST_RESIDUAL + if expired { SECOND_STOCK } else { 0 };
            if expired {
                // Identical retained bytes now release stock instead of converting
                // it to source-owner capital. The failed 811 face must not survive.
                pay(&mut world, &source_close, 2, 0, &mut paid);
                check(&world, &original, &paid, true, false);
            }
            let mut closed = false;
            for actor in order {
                if actor == 2 {
                    pay(
                        &mut world,
                        &source_close,
                        actor,
                        1_000 + converted + junior(actor, residual, converted),
                        &mut paid,
                    );
                    closed = true;
                } else {
                    let due = junior(actor, residual, if closed { converted } else { 0 })
                        - (paid[actor] - 1_000);
                    pay(&mut world, &retained[actor / 4], actor, due, &mut paid);
                }
                check(&world, &original, &paid, expired, closed);
            }
            // Fresh-branch claims that preceded bound refinement catch up using
            // the same requests; cumulative floors, not floors of each stock delta.
            for actor in order.into_iter().rev().filter(|&actor| actor != 2) {
                let due = junior(actor, residual, converted) - (paid[actor] - 1_000);
                if due != 0 {
                    pay(&mut world, &retained[actor / 4], actor, due, &mut paid);
                    check(&world, &original, &paid, expired, true);
                }
            }
            let expected = if expired {
                [1_198, 0, 1_283, 0, 1_368, 0]
            } else {
                [1_164, 0, 1_379, 0, 1_306, 0]
            };
            assert_eq!(paid, expected);
            let residue = residual
                - [0, 2, 4]
                    .map(|a| junior(a, residual, converted))
                    .iter()
                    .sum::<u128>();
            assert_eq!(residue, 2);
            assert_eq!(paid.iter().sum::<u128>() + residue + 1, SUPPLY);
            let ledger = world.env.market_state().1.resolved_payout_ledger;
            let endpoint = (paid, ledger, world.env.token_amount(world.env.vault));
            let baseline = &mut endpoints[(landing - 14) as usize];
            assert_eq!(
                *baseline.get_or_insert(endpoint),
                endpoint,
                "claimant order equality"
            );

            for (index, actor) in [0, 2, 4].into_iter().enumerate() {
                let portfolio = world.actors[actor].portfolio;
                assert_eq!(
                    (
                        world.env.portfolio_id(portfolio),
                        world.env.portfolio_position_epoch(portfolio),
                        state::read_portfolio_owner_preflight(
                            &world.env.svm.get_account(&portfolio).unwrap().data,
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
            // Crossing the consumed bucket's original expiry cannot release its
            // historical receivable again or resurrect a cleared claim.
            for retry_slot in [landing, 17] {
                world.env.svm.warp_to_slot(retry_slot);
                let before = world.frame();
                let mut expected_market = world.env.market_state().1;
                expected_market.current_slot = retry_slot;
                world
                    .land(
                        &[
                            retained[1].clone(),
                            world.payout(2, true),
                            retained[0].clone(),
                        ],
                        false,
                    )
                    .unwrap();
                world.assert_frame_except(&before, &[world.env.market]);
                assert_eq!(world.env.market_state().1, expected_market);
                assert_eq!(expected_market.resolved_payout_ledger, ledger);
                assert_eq!(
                    expected_market.source_credit[5].provider_receivable_num,
                    converted * BOUND_SCALE
                );
                assert_eq!(
                    expected_market.source_credit[5].fresh_reserved_backing_num,
                    0
                );
                assert_eq!(expected_market.vault, residue);
                world.custody();
            }
            assert_cu_within("aborted second-source realization", world.peak_cu, 600_000);
            peak_cu = peak_cu.max(world.peak_cu);
        }
    }
    assert_eq!(
        endpoints[1], endpoints[2],
        "exact and late expiry preserve every entitlement"
    );
    println!("INV-067 row 417: 18 worlds, 18 Fresh-realization rollbacks, 6 claimant orders; peak {peak_cu} CU");
}
