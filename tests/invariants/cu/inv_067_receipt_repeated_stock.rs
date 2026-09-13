//! Row 417: the same paid receipts survive two committed source-stock releases.
//! A six-owner public history splits one winner's claim across staggered domains;
//! claimant priority reverses after the first top-up and before final bound replacement.

use super::{late_expiry::World, *};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const FACES: [u128; 6] = [700, 0, 1_000, 0, 1_300, 0];
const RESIDUALS: [u128; 3] = [501, 501 + 61 + 100, 501 + 61 + 100 + 39 + 150];
const SUPPLY: u128 = 3_852;
const ORDERS: [[usize; 3]; 6] = [
    [0, 2, 4],
    [0, 4, 2],
    [2, 0, 4],
    [2, 4, 0],
    [4, 0, 2],
    [4, 2, 0],
];

fn junior(actor: usize, stage: usize) -> u128 {
    FACES[actor] * RESIDUALS[stage] / FACES.iter().sum::<u128>()
}

fn check(
    world: &World,
    original: &[ResolvedPayoutReceiptV16; 2],
    paid: &[u128; 6],
    stage: usize,
    replaced: bool,
) {
    let group = world.env.market_state().1;
    let ledger = group.resolved_payout_ledger;
    assert_eq!(ledger.snapshot_slot, 12);
    assert_eq!(ledger.snapshot_residual, RESIDUALS[stage]);
    assert_eq!(
        ledger.current_payout_rate_num,
        RESIDUALS[stage] * BOUND_SCALE
    );
    assert_eq!(ledger.current_payout_rate_den, 3_000 * BOUND_SCALE);
    assert_eq!(
        ledger.terminal_claim_bound_unreceipted_num,
        if replaced { 0 } else { 1_000 * BOUND_SCALE }
    );
    assert_eq!(
        ledger.terminal_claim_exact_receipts_num,
        if replaced { 3_000 } else { 2_000 } * BOUND_SCALE
    );
    assert!(!ledger.payout_halted && !ledger.finalized);
    assert_eq!(group.c_tot, if replaced { 0 } else { 1_000 });
    assert_eq!(group.insurance, 0);
    assert_eq!(group.backing_provider_earnings_total, 0);
    assert_eq!(group.materialized_portfolio_count, 6);
    assert_eq!(group.vault, SUPPLY - 1 - paid.iter().sum::<u128>());
    assert_eq!(world.env.token_amount(world.provider_token), 1);
    for (index, (domain, reserve, expiry)) in [(3, 161, 13), (5, 189, 15)].into_iter().enumerate() {
        let fresh = stage <= index;
        let bucket = group.source_backing_buckets[domain];
        assert_eq!(bucket.expiry_slot, expiry);
        assert_eq!(
            bucket.status,
            if fresh {
                BackingBucketStatusV16::Fresh
            } else {
                BackingBucketStatusV16::Expired
            }
        );
        assert_eq!(bucket.consumed_liened_backing_num, 0);
        assert_eq!(group.source_credit[domain].provider_receivable_num, 0);
        assert_eq!(
            group.source_credit[domain].fresh_reserved_backing_num,
            if fresh { reserve * BOUND_SCALE } else { 0 }
        );
    }
    for (index, actor) in [0, 4].into_iter().enumerate() {
        let mut expected = original[index];
        expected.paid_effective = paid[actor] - 1_000;
        assert_eq!(
            world.receipt(actor),
            expected,
            "immutable receipt for {actor}"
        );
    }
    assert!(!world.receipt(2).present);
    if replaced {
        assert!(resolved_portfolio_is_terminal(
            &world.env,
            world.actors[2].portfolio
        ));
    }
    for actor in 0..6 {
        assert_eq!(
            u128::from(world.env.token_amount(world.actors[actor].token)),
            paid[actor]
        );
    }
    world.custody();
}

fn pay(
    world: &mut World,
    instruction: &Instruction,
    actor: usize,
    paid: &mut [u128; 6],
    due: u128,
) {
    let before = world.frame();
    let vault = world.env.token_amount(world.env.vault);
    let meta = world
        .land(&[instruction.clone()], false)
        .expect("public receipt payment");
    assert_eq!(
        meta.logs
            .iter()
            .filter(|line| **line == format!("Program {} success", spl_token::ID))
            .count(),
        usize::from(due != 0)
    );
    paid[actor] += due;
    assert_eq!(
        u128::from(world.env.token_amount(world.actors[actor].token)),
        paid[actor]
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
    world.custody();
}

#[test]
fn v16_program_receipts_preserve_identity_through_two_stock_releases_and_reversed_priority() {
    assert_eq!([junior(0, 0), junior(0, 1), junior(0, 2)], [116, 154, 198]);
    assert_eq!([junior(4, 0), junior(4, 1), junior(4, 2)], [217, 286, 368]);
    let mut peak_cu = 0;
    let mut slab_calls = 0;
    for late in [0, 1] {
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
                paid[actor] = 1_000 + junior(actor, 0);
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
                assert_eq!(receipt.paid_effective, junior(actor, 0));
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
            let normalize = world.payout(2, false);
            let mut first_order: Vec<_> = order.into_iter().filter(|&actor| actor != 2).collect();
            first_order.reverse();
            check(&world, &original, &paid, 0, false);

            for stage in 1..=2 {
                world.env.svm.warp_to_slot([0, 13, 15][stage] + late);
                let before = world.frame();
                world.land(&retained, false).unwrap();
                world.assert_frame_except(&before, &[world.env.market]);
                check(&world, &original, &paid, stage - 1, false);

                if stage == 2 {
                    // Roll back the second release and both new payments while preserving
                    // the first release and its already-committed, unequal paid counters.
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
                                normalize.clone(),
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
                        .expect_err(
                            "invalid suffix after the second stock release and two top-ups",
                        );
                    assert_eq!(
                        failure.err,
                        TransactionError::InstructionError(
                            5,
                            InstructionError::InvalidInstructionData
                        )
                    );
                    for (program, count) in [(world.env.program_id, 3), (spl_token::ID, 2)] {
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
                        world
                            .env
                            .svm
                            .get_account(&world.env.payer.pubkey())
                            .unwrap(),
                        payer
                    );
                    check(&world, &original, &paid, 1, false);
                }
                pay(&mut world, &normalize, 2, &mut paid, 0);
                check(&world, &original, &paid, stage, false);
                if stage == 1 {
                    for &actor in &first_order {
                        let due = junior(actor, 1) - junior(actor, 0);
                        pay(&mut world, &retained[actor / 4], actor, &mut paid, due);
                        check(&world, &original, &paid, 1, false);
                    }
                    let before = world.frame();
                    world
                        .land(&[retained[1].clone(), retained[0].clone()], false)
                        .unwrap();
                    assert_eq!(
                        world.frame(),
                        before,
                        "first-stage payment retries preserve both future claims"
                    );
                }
            }

            let mut replaced = false;
            for actor in order {
                if actor == 2 {
                    // Each public close removes one of the two expired source claims.
                    // The second replaces the whole remaining face, pays, and clears it.
                    for source in 0..2 {
                        replaced = source == 1;
                        let due = if replaced { 1_000 + junior(2, 2) } else { 0 };
                        pay(&mut world, &normalize, 2, &mut paid, due);
                        check(&world, &original, &paid, 2, replaced);
                    }
                } else {
                    pay(
                        &mut world,
                        &retained[actor / 4],
                        actor,
                        &mut paid,
                        junior(actor, 2) - junior(actor, 1),
                    );
                    check(&world, &original, &paid, 2, replaced);
                }
            }
            assert_eq!(paid, [1_198, 0, 1_283, 0, 1_368, 0]);
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
                let instruction = world.payout(actor, true);
                pay(&mut world, &instruction, actor, &mut paid, 0);
                assert!(!world.receipt(actor).present);
            }
            let before = world.frame();
            world
                .land(
                    &[
                        retained[0].clone(),
                        retained[1].clone(),
                        world.payout(2, true),
                    ],
                    false,
                )
                .unwrap();
            assert_eq!(
                world.frame(),
                before,
                "terminal retries cannot revive either stock release"
            );

            for actor in 0..6 {
                let portfolio = world.actors[actor].portfolio;
                assert!(resolved_portfolio_is_terminal(&world.env, portfolio));
                let before = world.frame();
                let mut expected = world.env.svm.get_account(&portfolio).unwrap();
                let market_lamports = world
                    .env
                    .svm
                    .get_account(&world.env.market)
                    .unwrap()
                    .lamports;
                let rent = expected.lamports;
                let cu = world
                    .env
                    .close_portfolio_with_cu(&world.actors[actor].owner, portfolio);
                world.peak_cu = world.peak_cu.max(cu);
                expected.lamports = 0;
                expected.data.clear();
                assert_eq!(world.env.svm.get_account(&portfolio), Some(expected));
                assert_eq!(
                    world
                        .env
                        .svm
                        .get_account(&world.env.market)
                        .unwrap()
                        .lamports,
                    market_lamports + rent
                );
                world.assert_frame_except(&before, &[world.env.market, portfolio]);
                world.custody();
            }
            let group = world.env.market_state().1;
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
            assert_eq!(group.materialized_portfolio_count, 0);
            let rounding =
                RESIDUALS[2] - [0, 2, 4].map(|actor| junior(actor, 2)).iter().sum::<u128>();
            assert_eq!(rounding, 2);
            assert_eq!(group.vault, rounding);
            crate::support::fuzz_model::assert_reservation_encumbrance_census(
                "INV-067 repeated stock terminal",
                &group,
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
                .expect("fully disposed three-asset market closes");
            slab_calls += 1;
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
                admin_lamports + market_rent + vault_rent - tombstone_rent
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
            assert_cu_within("INV-067 repeated stock history", world.peak_cu, 600_000);
            peak_cu = peak_cu.max(world.peak_cu);
        }
    }
    println!("INV-067 repeated stock: 12 worlds, 24 releases, 12 rollbacks, 72 portfolio closes, {slab_calls} slab calls; peak {peak_cu} CU");
}
