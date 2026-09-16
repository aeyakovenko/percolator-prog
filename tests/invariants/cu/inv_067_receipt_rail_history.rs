//! Bounded row 417 coverage: alternate quote rails across two stock releases while
//! one unequal receipt may defer its first top-up. The same retained requests must
//! share cumulative entitlement, including in a transaction that later rolls back.

use super::*;
use solana_sdk::fee::FeeStructure;

#[path = "inv_067_receipt_close_slab_rail.rs"]
mod close_slab;

const RESIDUALS: [u128; 3] = [501, 662, 851];

#[derive(Debug, PartialEq)]
struct Checkpoint {
    ledger: ResolvedPayoutLedgerV16,
    receipts: [ResolvedPayoutReceiptV16; 5],
    paid: [u128; 5],
    secondary_paid: [u64; 5],
    vaults: [u64; 2],
    engine_vault: u128,
}

fn checkpoint(world: &World, rail: &Rail, paid: [u128; 5]) -> Checkpoint {
    rail.check(world);
    assert_eq!(world.env.token_amount(world.actors[5].token), 0);
    for actor in 0..5 {
        assert_eq!(rail.paid(world, actor), paid[actor]);
    }
    let group = world.env.market_state().1;
    assert_eq!(group.vault, PRIMARY_SUPPLY - 1 - paid.iter().sum::<u128>());
    Checkpoint {
        ledger: group.resolved_payout_ledger,
        receipts: std::array::from_fn(|actor| world.receipt(actor)),
        paid,
        secondary_paid: rail.destinations.map(|key| world.env.token_amount(key)),
        vaults: [
            world.env.token_amount(world.env.vault),
            world.env.token_amount(rail.vault),
        ],
        engine_vault: group.vault,
    }
}

fn check_receipts(
    world: &World,
    rail: &Rail,
    original: &[ResolvedPayoutReceiptV16; 2],
    paid: [u128; 5],
    stage: usize,
) -> Checkpoint {
    let point = checkpoint(world, rail, paid);
    assert_eq!(point.ledger.snapshot_slot, 12);
    assert_eq!(point.ledger.snapshot_residual, RESIDUALS[stage]);
    assert_eq!(
        point.ledger.current_payout_rate_num,
        RESIDUALS[stage] * BOUND_SCALE
    );
    assert_eq!(point.ledger.current_payout_rate_den, 3_000 * BOUND_SCALE);
    assert_eq!(
        point.ledger.terminal_claim_bound_unreceipted_num,
        1_000 * BOUND_SCALE
    );
    assert_eq!(
        point.ledger.terminal_claim_exact_receipts_num,
        2_000 * BOUND_SCALE
    );
    assert!(!point.ledger.payout_halted && !point.ledger.finalized);
    for (index, actor) in [0, 4].into_iter().enumerate() {
        let mut expected = original[index];
        expected.paid_effective = paid[actor] - CAPITAL[actor];
        assert!(expected.paid_effective <= entitlement(actor, RESIDUALS[stage]));
        assert_eq!(
            point.receipts[actor], expected,
            "only cumulative paid may change"
        );
    }
    let group = world.env.market_state().1;
    for (index, (domain, stock, expiry)) in [(3, 161, 13), (5, 189, 15)].into_iter().enumerate() {
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
            if fresh { stock * BOUND_SCALE } else { 0 }
        );
    }
    point
}

fn successes(meta: &litesvm::types::TransactionMetadata, program: Pubkey, expected: usize) {
    assert_eq!(
        meta.logs
            .iter()
            .filter(|line| **line == format!("Program {program} success"))
            .count(),
        expected
    );
}

fn reject_suffix(world: &mut World, rail: &Rail, prefix: &[Instruction], transfers: usize) {
    let before = rail.frame(world);
    let mut payer = world
        .env
        .svm
        .get_account(&world.env.payer.pubkey())
        .unwrap();
    payer.lamports -= FeeStructure::default().lamports_per_signature;
    let mut aborted = prefix.to_vec();
    aborted.push(Instruction {
        program_id: solana_sdk::system_program::ID,
        accounts: vec![],
        data: vec![],
    });
    let failure = world
        .land(&aborted, false)
        .expect_err("reject after the complete public prefix");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(
            2 + prefix.len() as u8,
            InstructionError::InvalidInstructionData,
        )
    );
    successes(&failure.meta, world.env.program_id, prefix.len());
    successes(&failure.meta, spl_token::ID, transfers);
    assert_eq!(
        rail.frame(world),
        before,
        "exact receipt, stock, SPL and lamport rollback"
    );
    assert_eq!(
        world.env.svm.get_account(&world.env.payer.pubkey()),
        Some(payer)
    );
}

fn commit_payments(
    world: &mut World,
    rail: &Rail,
    batch: &[Instruction],
    deltas: [u128; 5],
    secondary: bool,
) {
    let before = rail.frame(world);
    let before_paid: [u128; 5] = std::array::from_fn(|actor| rail.paid(world, actor));
    let before_secondary = rail.destinations.map(|key| world.env.token_amount(key));
    let vaults = [
        world.env.token_amount(world.env.vault),
        world.env.token_amount(rail.vault),
    ];
    let engine_vault = world.env.market_state().1.vault;
    let meta = world
        .land(batch, false)
        .expect("unchanged failed prefix can commit");
    successes(&meta, world.env.program_id, batch.len());
    successes(
        &meta,
        spl_token::ID,
        deltas.iter().filter(|&&delta| delta != 0).count(),
    );
    let total = deltas.iter().sum::<u128>();
    assert_eq!(engine_vault - world.env.market_state().1.vault, total);
    assert_eq!(
        u128::from(vaults[0] - world.env.token_amount(world.env.vault)),
        if secondary { 0 } else { total }
    );
    assert_eq!(
        u128::from(vaults[1] - world.env.token_amount(rail.vault)),
        if secondary { total } else { 0 }
    );
    let mut allowed = vec![world.env.market];
    allowed.extend(batch.iter().map(|ix| ix.accounts[2].pubkey));
    for actor in 0..5 {
        assert_eq!(rail.paid(world, actor), before_paid[actor] + deltas[actor]);
        assert_eq!(
            u128::from(world.env.token_amount(rail.destinations[actor]) - before_secondary[actor]),
            if secondary { deltas[actor] } else { 0 }
        );
        if deltas[actor] != 0 {
            allowed.extend(if secondary {
                [rail.destinations[actor], rail.vault]
            } else {
                [world.actors[actor].token, world.env.vault]
            });
        }
    }
    world.assert_frame_except(&before, &allowed);
    rail.check(world);
}

fn run_history(native: bool) -> (Vec<Vec<Checkpoint>>, u64) {
    let mut histories = Vec::new();
    let mut peak = 0;
    for late in [0, 1] {
        for first_secondary in [false, true] {
            for eager in [0, 4] {
                for defer_peer in [false, true] {
                    let peer = 4 - eager;
                    let setup = if native {
                        install_native_secondary
                    } else {
                        install_secondary
                    };
                    let mut world = World::before_receipts_with_staggered_quote_rails(
                        setup,
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
                    }
                    let original = [world.receipt(0), world.receipt(4)];
                    for (index, actor) in [0, 4].into_iter().enumerate() {
                        assert!(original[index].present && !original[index].finalized);
                        assert_eq!(original[index].terminal_positive_claim_face, FACES[actor]);
                        assert_eq!(
                            original[index].prior_bound_contribution_num,
                            FACES[actor] * BOUND_SCALE
                        );
                        assert_eq!(original[index].live_released_face_at_receipt, 0);
                        assert_eq!(
                            original[index].paid_effective,
                            entitlement(actor, INITIAL_RESIDUAL)
                        );
                    }
                    let identities = [0, 2, 4].map(|actor| {
                        (
                            world.env.portfolio_id(world.actors[actor].portfolio),
                            world
                                .env
                                .portfolio_position_epoch(world.actors[actor].portfolio),
                        )
                    });
                    let mut rail = Rail::new(&mut world);
                    rail.fund(&mut world, SECONDARY_SUPPLY);
                    let mut paid = [1_116, 0, 0, 0, 1_217];
                    let mut points = vec![check_receipts(&world, &rail, &original, paid, 0)];
                    // Freeze both handlers on both rails before either expiry. All later
                    // replays reuse these instruction bytes with a fresh blockhash.
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
                    let normalize = world.payout(2, false);
                    for stage in 1..=2 {
                        let expiry = [0, 13, 15][stage];
                        let secondary = if stage == 1 {
                            first_secondary
                        } else {
                            !first_secondary
                        };
                        // Only the already-paid claimant is retried before the second
                        // boundary: the deferred peer still has legitimate first-wave due.
                        let pre_boundary =
                            [claims[0][eager / 4].clone(), claims[1][eager / 4].clone()];
                        world.env.svm.warp_to_slot(expiry - 1);
                        commit_payments(&mut world, &rail, &pre_boundary, [0; 5], secondary);
                        points.push(check_receipts(&world, &rail, &original, paid, stage - 1));
                        world.env.svm.warp_to_slot(expiry + late);
                        commit_payments(&mut world, &rail, &pre_boundary, [0; 5], secondary);
                        points.push(check_receipts(&world, &rail, &original, paid, stage - 1));

                        let order = if stage == 1 {
                            [eager, peer]
                        } else {
                            [peer, eager]
                        };
                        let mut batch = vec![normalize.clone()];
                        let mut deltas = [0; 5];
                        for actor in order {
                            if stage == 1 && defer_peer && actor == peer {
                                continue;
                            }
                            deltas[actor] =
                                CAPITAL[actor] + entitlement(actor, RESIDUALS[stage]) - paid[actor];
                            assert!(deltas[actor] > 0);
                            batch.push(if actor == eager {
                                closes[usize::from(secondary)][actor / 4].clone()
                            } else {
                                claims[usize::from(secondary)][actor / 4].clone()
                            });
                            batch.push(claims[usize::from(!secondary)][actor / 4].clone());
                            batch.push(claims[usize::from(secondary)][actor / 4].clone());
                        }
                        reject_suffix(
                            &mut world,
                            &rail,
                            &batch,
                            deltas.iter().filter(|&&d| d != 0).count(),
                        );
                        check_receipts(&world, &rail, &original, paid, stage - 1);
                        commit_payments(&mut world, &rail, &batch, deltas, secondary);
                        for actor in 0..5 {
                            paid[actor] += deltas[actor];
                        }
                        points.push(check_receipts(&world, &rail, &original, paid, stage));
                        for actor in order {
                            if stage == 1 && defer_peer && actor == peer {
                                assert_eq!(
                                    world.receipt(actor),
                                    original[actor / 4],
                                    "deferred receipt keeps its first paid counter"
                                );
                                continue;
                            }
                            let before = rail.frame(&world);
                            world
                                .land(
                                    &[claims[1][actor / 4].clone(), claims[0][actor / 4].clone()],
                                    false,
                                )
                                .unwrap();
                            assert_eq!(
                                rail.frame(&world),
                                before,
                                "funded alternate rail cannot repay this rate"
                            );
                        }
                    }
                    assert_eq!(paid, [1_198, 0, 0, 0, 1_368]);
                    // Replace the remaining two-source bound and pay the source claimant.
                    // Aborting its payment must restore both prior cross-rail receipts.
                    commit_payments(&mut world, &rail, &[normalize.clone()], [0; 5], false);
                    reject_suffix(&mut world, &rail, &[normalize.clone()], 1);
                    commit_payments(&mut world, &rail, &[normalize], [0, 0, 1_283, 0, 0], false);
                    paid[2] = 1_283;
                    let clears = [claims[0][0].clone(), claims[1][1].clone()];
                    reject_suffix(&mut world, &rail, &clears, 0);
                    commit_payments(&mut world, &rail, &clears, [0; 5], false);
                    for actor in 0..6 {
                        assert!(!world.receipt(actor).present);
                        assert!(resolved_portfolio_is_terminal(
                            &world.env,
                            world.actors[actor].portfolio
                        ));
                    }
                    let replay = [
                        claims[0][0].clone(),
                        claims[1][0].clone(),
                        claims[1][1].clone(),
                        claims[0][1].clone(),
                    ];
                    let terminal = checkpoint(&world, &rail, paid);
                    for slot in [16 + late, 100] {
                        world.env.svm.warp_to_slot(slot);
                        reject_suffix(&mut world, &rail, &replay, 0);
                        commit_payments(&mut world, &rail, &replay, [0; 5], false);
                        assert_eq!(
                            checkpoint(&world, &rail, paid),
                            terminal,
                            "cleared receipts cannot regain entitlement at a later clock or rail"
                        );
                        let before = rail.frame(&world);
                        world.land(&replay, false).unwrap();
                        assert_eq!(rail.frame(&world), before);
                    }
                    assert_eq!(terminal.engine_vault, 2);
                    assert_eq!(terminal.ledger.terminal_claim_bound_unreceipted_num, 0);
                    for (index, actor) in [0, 2, 4].into_iter().enumerate() {
                        assert_eq!(
                            (
                                world.env.portfolio_id(world.actors[actor].portfolio),
                                world
                                    .env
                                    .portfolio_position_epoch(world.actors[actor].portfolio)
                            ),
                            identities[index]
                        );
                    }
                    points.push(terminal);
                    assert_cu_within(
                        "two-expiry alternating-rail receipt history",
                        world.peak_cu,
                        900_000,
                    );
                    peak = peak.max(world.peak_cu);
                    histories.push(points);
                }
            }
        }
    }
    assert_eq!(histories.len(), 16);
    (histories, peak)
}

#[test]
fn v16_program_two_expiry_receipts_share_exact_once_entitlement_across_quote_rails() {
    let (classic, classic_cu) = run_history(false);
    let (native, native_cu) = run_history(true);
    assert_eq!(
        classic, native,
        "classic/native prefix ledgers, receipt identities, paid floors and rail custody agree"
    );
    println!("INV-067 row417: 32 two-expiry rail histories, 192 exact suffix rollbacks; peak CU classic={classic_cu}, native={native_cu}");
}
