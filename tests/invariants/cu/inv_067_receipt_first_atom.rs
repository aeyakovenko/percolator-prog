//! INV-067 / row 417: a positive face with zero paid junior value survives a
//! rounding plateau. Its retained tail-free claim becomes inadmissible exactly
//! when late source stock makes the first atom payable. Public instructions only.

use super::{late_expiry::World, *};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const FACES: [u128; 6] = [4, 0, 1_000, 0, 1_996, 0];
const RESIDUAL: [u128; 3] = [501, 662, 851];

fn entitlement(actor: usize, stage: usize) -> u128 {
    FACES[actor] * RESIDUAL[stage] / 3_000
}

#[derive(Default)]
struct Evidence {
    peak_cu: u64,
    rollback_peak_cu: u64,
    peak_bytes: usize,
    rollbacks: usize,
    rolled_back_transfers: usize,
}

fn successes(logs: &[String], program: Pubkey) -> usize {
    logs.iter()
        .filter(|line| **line == format!("Program {program} success"))
        .count()
}

fn submit(
    world: &mut World,
    evidence: &mut Evidence,
    instructions: &[Instruction],
    transfers: usize,
    failure: Option<InstructionError>,
) {
    let mut all = vec![heap_ix(), cu_ix()];
    all.extend_from_slice(instructions);
    let tx = Transaction::new_signed_with_payer(
        &all,
        Some(&world.env.payer.pubkey()),
        &[&world.env.payer],
        world.env.svm.latest_blockhash(),
    );
    assert_eq!(tx.message.header.num_required_signatures, 1);
    let bytes = bincode::serialized_size(&tx).unwrap() as usize;
    assert!(bytes <= 1_232);
    evidence.peak_bytes = evidence.peak_bytes.max(bytes);
    let mut before = world.frame();
    before.push((
        solana_sdk::sysvar::clock::ID,
        world.env.svm.get_account(&solana_sdk::sysvar::clock::ID),
    ));
    for key in tx.message.account_keys {
        if key != world.env.payer.pubkey() {
            before.push((key, world.env.svm.get_account(&key)));
        }
    }
    let mut payer = world
        .env
        .svm
        .get_account(&world.env.payer.pubkey())
        .unwrap();
    payer.lamports -= FeeStructure::default().lamports_per_signature;
    let result = world.land(instructions, false);
    let rejected = failure.is_some();
    let meta = if let Some(error) = failure {
        let result = result.expect_err("retained request must reject at the stated suffix");
        assert_eq!(
            result.err,
            TransactionError::InstructionError((instructions.len() + 1) as u8, error)
        );
        for (key, account) in before {
            assert_eq!(world.env.svm.get_account(&key), account, "rollback {key}");
        }
        evidence.rollbacks += 1;
        evidence.rolled_back_transfers += transfers;
        evidence.rollback_peak_cu = evidence
            .rollback_peak_cu
            .max(result.meta.compute_units_consumed);
        result.meta
    } else {
        result.expect("valid public receipt continuation")
    };
    assert_eq!(successes(&meta.logs, spl_token::ID), transfers);
    assert_eq!(
        successes(&meta.logs, world.env.program_id),
        instructions.len() - usize::from(rejected)
    );
    assert_eq!(
        world.env.svm.get_account(&world.env.payer.pubkey()),
        Some(payer)
    );
    assert_cu_within(
        "first-atom receipt transaction",
        meta.compute_units_consumed,
        900_000,
    );
    evidence.peak_cu = evidence.peak_cu.max(meta.compute_units_consumed);
    world.custody();
}

fn check(world: &World, stage: usize, paid: &[u128; 6], cleared: [bool; 2], replaced: bool) {
    let group = world.env.market_state().1;
    let ledger = group.resolved_payout_ledger;
    assert_eq!(ledger.snapshot_slot, 12);
    assert_eq!(ledger.snapshot_residual, RESIDUAL[stage]);
    assert_eq!(
        ledger.current_payout_rate_num,
        RESIDUAL[stage] * BOUND_SCALE
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
    for (index, actor) in [0, 4].into_iter().enumerate() {
        let expected = if cleared[index] {
            ResolvedPayoutReceiptV16::EMPTY
        } else {
            ResolvedPayoutReceiptV16 {
                present: true,
                prior_bound_contribution_num: FACES[actor] * BOUND_SCALE,
                live_released_face_at_receipt: 0,
                terminal_positive_claim_face: FACES[actor],
                paid_effective: paid[actor] - 1_000,
                finalized: false,
            }
        };
        assert_eq!(world.receipt(actor), expected, "receipt {actor}");
        assert_eq!(
            resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio),
            cleared[index]
        );
    }
    assert!(!world.receipt(2).present);
    assert_eq!(
        resolved_portfolio_is_terminal(&world.env, world.actors[2].portfolio),
        replaced
    );
    for actor in 0..6 {
        assert_eq!(
            u128::from(world.env.token_amount(world.actors[actor].token)),
            paid[actor]
        );
    }
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
        assert_eq!(
            group.source_credit[domain].fresh_reserved_backing_num,
            if fresh { stock * BOUND_SCALE } else { 0 }
        );
        assert_eq!(bucket.consumed_liened_backing_num, 0);
        assert_eq!(group.source_credit[domain].provider_receivable_num, 0);
    }
    assert_eq!(world.env.token_amount(world.provider_token), 1);
    assert_eq!(group.vault, 3_851 - paid.iter().sum::<u128>());
    world.custody();
}

#[test]
fn v16_program_zero_paid_receipt_survives_plateau_and_first_atom_tail_revalidation() {
    assert_eq!(
        [entitlement(0, 0), entitlement(0, 1), entitlement(0, 2)],
        [0, 0, 1]
    );
    assert_eq!(
        [entitlement(4, 0), entitlement(4, 1), entitlement(4, 2)],
        [333, 440, 566]
    );
    let mut evidence = Evidence::default();
    let mut worlds = 0;
    for late in [0, 1] {
        for source_first in [false, true] {
            for order in [[0, 4], [4, 0]] {
                let mut world =
                    World::before_receipts_with_staggered_face([Keypair::new(), Keypair::new()], 4);
                for actor in [0, 4] {
                    for _ in 0..8 {
                        if world.receipt(actor).present {
                            break;
                        }
                        world.land(&[world.payout(actor, false)], false).unwrap();
                    }
                }
                let mut paid = [1_000, 0, 0, 0, 1_333, 0];
                let identities = [0, 2, 4].map(|actor| {
                    let key = world.actors[actor].portfolio;
                    (
                        world.env.portfolio_id(key),
                        world.env.portfolio_position_epoch(key),
                        state::read_portfolio_owner_preflight(
                            &world.env.svm.get_account(&key).unwrap().data,
                        )
                        .unwrap(),
                    )
                });
                let full = [world.payout(0, true), world.payout(4, true)];
                let normalize = world.payout(2, false);
                let mut short = full[0].clone();
                short.accounts.truncate(3);
                check(&world, 0, &paid, [false; 2], false);
                let before = world.frame();
                submit(&mut world, &mut evidence, &[short.clone()], 0, None);
                assert_eq!(
                    world.frame(),
                    before,
                    "positive face with no junior payment stays live"
                );

                world.env.svm.warp_to_slot(13 + late);
                let prefix = [
                    short.clone(),
                    normalize.clone(),
                    full[1].clone(),
                    short.clone(),
                    full[1].clone(),
                ];
                let mut rejected = prefix.to_vec();
                rejected.push(Instruction {
                    program_id: solana_sdk::system_program::ID,
                    accounts: vec![],
                    data: vec![],
                });
                submit(
                    &mut world,
                    &mut evidence,
                    &rejected,
                    1,
                    Some(InstructionError::InvalidInstructionData),
                );
                check(&world, 0, &paid, [false; 2], false);
                submit(&mut world, &mut evidence, &prefix, 1, None);
                paid[4] = 1_440;
                check(&world, 1, &paid, [false; 2], false);
                let before = world.frame();
                submit(
                    &mut world,
                    &mut evidence,
                    &[short.clone(), full[1].clone()],
                    0,
                    None,
                );
                assert_eq!(
                    world.frame(),
                    before,
                    "the first release is still below one atom"
                );

                world.env.svm.warp_to_slot(15 + late);
                // Clock passage alone leaves the short request admissible. Only public
                // source normalization crosses the payout threshold within this bundle.
                let mut prefix = vec![short.clone(), normalize.clone()];
                if source_first {
                    prefix.extend([normalize.clone(), normalize.clone()]);
                }
                prefix.push(full[1].clone());
                let mut rejected = prefix.clone();
                rejected.push(short.clone());
                for _ in 0..2 {
                    submit(
                        &mut world,
                        &mut evidence,
                        &rejected,
                        1 + usize::from(source_first),
                        Some(InstructionError::NotEnoughAccountKeys),
                    );
                    check(&world, 1, &paid, [false; 2], false);
                }
                // Retry unchanged normalization bytes and valid claims in both orders.
                prefix.pop();
                prefix.extend(order.map(|actor| full[actor / 4].clone()));
                submit(
                    &mut world,
                    &mut evidence,
                    &prefix,
                    2 + usize::from(source_first),
                    None,
                );
                paid[0] = 1_001;
                paid[4] = 1_566;
                if source_first {
                    paid[2] = 1_283;
                }
                check(&world, 2, &paid, [false; 2], source_first);

                if !source_first {
                    let before = world.frame();
                    submit(
                        &mut world,
                        &mut evidence,
                        &[short.clone(), full[1].clone()],
                        0,
                        None,
                    );
                    assert_eq!(
                        world.frame(),
                        before,
                        "first atom is paid once while the source bound remains"
                    );
                    submit(
                        &mut world,
                        &mut evidence,
                        &[normalize.clone(), normalize.clone()],
                        1,
                        None,
                    );
                    paid[2] = 1_283;
                }
                check(&world, 2, &paid, [false; 2], true);
                // With the final bound replaced, even the tail-free request can retire
                // the now-paid receipt. Rejection must also restore that clearing.
                let clears = [short.clone(), full[1].clone()];
                let mut rejected = clears.to_vec();
                rejected.push(Instruction {
                    program_id: solana_sdk::system_program::ID,
                    accounts: vec![],
                    data: vec![],
                });
                submit(
                    &mut world,
                    &mut evidence,
                    &rejected,
                    0,
                    Some(InstructionError::InvalidInstructionData),
                );
                check(&world, 2, &paid, [false; 2], true);
                submit(&mut world, &mut evidence, &clears, 0, None);
                check(&world, 2, &paid, [true; 2], true);
                let before = world.frame();
                let replays = [
                    short,
                    full[0].clone(),
                    full[1].clone(),
                    world.payout(2, true),
                ];
                submit(&mut world, &mut evidence, &replays, 0, None);
                assert_eq!(
                    world.frame(),
                    before,
                    "retired receipts cannot replay either junior or principal"
                );
                for (index, actor) in [0, 2, 4].into_iter().enumerate() {
                    let key = world.actors[actor].portfolio;
                    assert_eq!(
                        (
                            world.env.portfolio_id(key),
                            world.env.portfolio_position_epoch(key),
                            state::read_portfolio_owner_preflight(
                                &world.env.svm.get_account(&key).unwrap().data
                            )
                            .unwrap()
                        ),
                        identities[index]
                    );
                }
                for actor in 0..6 {
                    let key = world.actors[actor].portfolio;
                    assert!(resolved_portfolio_is_terminal(&world.env, key));
                    let before = world.frame();
                    let rent = world.env.svm.get_account(&key).unwrap().lamports;
                    let market_rent = world
                        .env
                        .svm
                        .get_account(&world.env.market)
                        .unwrap()
                        .lamports;
                    let cu = world
                        .env
                        .close_portfolio_with_cu(&world.actors[actor].owner, key);
                    assert_cu_within("first-atom portfolio close", cu, CUSTODY_CU_LIMIT);
                    evidence.peak_cu = evidence.peak_cu.max(cu);
                    assert!(world
                        .env
                        .svm
                        .get_account(&key)
                        .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                    assert_eq!(
                        world
                            .env
                            .svm
                            .get_account(&world.env.market)
                            .unwrap()
                            .lamports,
                        market_rent + rent
                    );
                    world.assert_frame_except(&before, &[world.env.market, key]);
                }
                let group = world.env.market_state().1;
                assert_eq!(group.materialized_portfolio_count, 0);
                assert_eq!(
                    [
                        group.c_tot,
                        group.pnl_pos_tot,
                        group.source_claim_bound_total_num,
                        group.insurance,
                        group.backing_provider_earnings_total
                    ],
                    [0; 5]
                );
                assert_eq!(paid, [1_001, 0, 1_283, 0, 1_566, 0]);
                assert_eq!(group.vault, 1, "851 - (1 + 283 + 566) is booked rounding");
                world.custody();
                worlds += 1;
            }
        }
    }
    assert_eq!(
        (worlds, evidence.rollbacks, evidence.rolled_back_transfers),
        (8, 32, 32)
    );
    println!("INV-067 first atom: {worlds} worlds, {} exact rollbacks, {} rolled-back SPL transfers; peak CU {}, rollback CU {}, bytes {}; junior floors [0, 0, 1], peer [333, 440, 566], final [1001, 0, 1283, 0, 1566, 0], rounding 1", evidence.rollbacks, evidence.rolled_back_transfers, evidence.peak_cu, evidence.rollback_peak_cu, evidence.peak_bytes);
}
