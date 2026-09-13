//! INV-045/038/085, row425: retained reductions across pending funding catch-up.
//! Public target replacement at Clock 5 resets carry at accrual frontier 2;
//! bounded cranks rebuild it while the old funding checkpoint remains owed.
//! Pre-signed requests survive failed catch-up and successful-prefix rollback.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};
use std::collections::BTreeMap;

const CHECKPOINT: u64 = 5;
const EXIT: u64 = 9;
const LIMIT: u32 = 600_000;

fn sign(world: &World, instructions: &[Instruction], nonce: u32) -> Transaction {
    let mut message = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(LIMIT - nonce),
    ];
    message.extend_from_slice(instructions);
    let mut signers = vec![&world.env.payer];
    for owner in &world.owners {
        if instructions
            .iter()
            .flat_map(|ix| &ix.accounts)
            .any(|meta| meta.is_signer && meta.pubkey == owner.pubkey())
        {
            signers.push(owner);
        }
    }
    let tx = Transaction::new_signed_with_payer(
        &message,
        Some(&world.env.payer.pubkey()),
        &signers,
        world.env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
    tx
}

fn frame(world: &World, tx: &Transaction) -> BTreeMap<Pubkey, Option<Account>> {
    tx.message
        .account_keys
        .iter()
        .copied()
        .chain(world.portfolios)
        .chain(world.tokens)
        .chain(world.owners.iter().map(Signer::pubkey))
        .chain([
            world.env.market,
            world.env.mint,
            world.env.vault,
            world.env.vault_authority,
            world.env.admin.pubkey(),
            solana_sdk::sysvar::clock::ID,
        ])
        .map(|key| (key, world.env.svm.get_account(&key)))
        .collect()
}

fn reject(world: &mut World, tx: &Transaction, index: u8, error: InstructionError) {
    let mut before = frame(world, tx);
    before
        .get_mut(&world.env.payer.pubkey())
        .unwrap()
        .as_mut()
        .unwrap()
        .lamports -= FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let failure = world.env.svm.send_transaction(tx.clone()).unwrap_err();
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(index, error)
    );
    assert_eq!(frame(world, tx), before, "{:?}", world.trace);
    assert_eq!(
        failure
            .meta
            .logs
            .iter()
            .filter(|log| **log == format!("Program {} success", world.env.program_id))
            .count(),
        usize::from(index == 3),
        "the late suffix must follow a successful economic instruction"
    );
    world.max_cu = world.max_cu.max(failure.meta.compute_units_consumed);
}

fn advance(ledger: &mut Ledger, direction: i128, slot: u64) {
    assert_eq!(slot, ledger.slot + 1);
    for asset in 0..2 {
        let old_sign = direction * if asset == 0 { 1 } else { -1 };
        // The public replacement is submitted with slot_last=2. Its new target
        // starts a fresh price trajectory, but funding changes only after slot 5.
        let numerator = ANCHORS[asset] * CAP_BPS * (slot - 2);
        let price = ANCHORS[asset] as i128 - old_sign * i128::from(numerator / 10_000);
        let funding_mark =
            ANCHORS[asset] as i128 + old_sign * if slot <= CHECKPOINT { 20 } else { -20 };
        let rate = ((funding_mark - price) * 1_000_000_000 / price).clamp(-RATE, RATE);
        assert_eq!(
            rate.abs(),
            RATE,
            "both public premiums saturate the configured cap"
        );
        let funding = -(rate * price).div_euclid(1_000_000_000);
        for actor in 0..4 {
            ledger.value[actor] +=
                ledger.lots[actor][asset] * (price - ledger.price[asset] as i128 + funding);
        }
        ledger.price[asset] = price as u64;
        ledger.carry[asset] = numerator % 10_000;
        ledger.funding[asset] += funding;
    }
    ledger.slot = slot;
}

fn checkpoint(world: &World, ledger: &mut Ledger, direction: i128) {
    ledger.check(world);
    for (asset, profile) in carry_transport_exit::profiles(world).iter().enumerate() {
        let sign = direction * if asset == 0 { 1 } else { -1 };
        let old = (ANCHORS[asset] as i128 + sign * 20) as u64;
        let new = (ANCHORS[asset] as i128 - sign * 20) as u64;
        assert_eq!(profile.mark_ewma_e6, new);
        assert_eq!(profile.mark_ewma_last_slot, CHECKPOINT);
        assert_eq!(profile.oracle_target_price_e6, new);
        assert_eq!(
            world.env.market_state().1.assets[asset].raw_oracle_target_price,
            new
        );
        assert_eq!(
            (
                profile.funding_mark_e6,
                profile.funding_mark_pending_e6,
                profile.funding_mark_pending_slot
            ),
            if ledger.slot < CHECKPOINT {
                (old, new, CHECKPOINT)
            } else {
                (new, 0, 0)
            }
        );
    }
}

fn crank(world: &mut World, ledger: &mut Ledger, direction: i128, reverse: bool, slot: u64) {
    let clock = world.env.svm.get_sysvar::<Clock>().slot;
    world
        .trace
        .push(format!("public crank: clock={clock}, next_accrual={slot}"));
    let absent = [0, 1, 3].map(|actor| world.env.svm.get_account(&world.portfolios[actor]));
    let ix = Instruction {
        program_id: world.env.program_id,
        accounts: vec![
            AccountMeta::new(world.env.payer.pubkey(), true),
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(world.portfolios[2], false),
        ],
        data: ProgInstruction::PermissionlessCrank {
            now_slot: clock,
            observations: crank_observations_for_assets(if reverse { &[1, 0] } else { &[0, 1] }),
        }
        .encode(),
    };
    let tx = sign(world, &[ix], 100 + slot as u32);
    let result = world
        .env
        .svm
        .send_transaction(tx)
        .expect("bounded public funding catch-up");
    world.max_cu = world.max_cu.max(result.compute_units_consumed);
    advance(ledger, direction, slot);
    checkpoint(world, ledger, direction);
    assert_eq!(
        [0, 1, 3].map(|actor| world.env.svm.get_account(&world.portfolios[actor])),
        absent
    );
    assert_eq!(world.env.svm.get_sysvar::<Clock>().slot, clock);
}

#[test]
fn v16_program_retained_reduction_preserves_carry_across_pending_funding_checkpoint() {
    let mut peak = 0;
    let mut worlds = 0;
    let mut rejected = 0;
    for direction in [-1, 1] {
        let mut reference = None;
        for reverse in [false, true] {
            for attempts in [false, true] {
                let history = History {
                    direction,
                    batch: true,
                    split: false,
                    placement: 0,
                    reverse,
                };
                let mut world = World::with_funding(history, RATE as u64);
                world.trace.push(format!(
                    "retained pending-funding retry: attempts={attempts}"
                ));
                send_raw_tx(
                    &mut world.env.svm,
                    &world.env.payer,
                    spl_token::instruction::set_authority(
                        &spl_token::ID,
                        &world.env.mint,
                        None,
                        spl_token::instruction::AuthorityType::MintTokens,
                        &world.env.admin.pubkey(),
                        &[],
                    )
                    .unwrap(),
                    &[&world.env.admin],
                )
                .unwrap();
                let mut ledger = Ledger::new();
                ledger.check(&world);
                for slot in 1..=2 {
                    world.env.svm.warp_to_slot(slot);
                    ledger.advance(direction, slot);
                    settle(&mut world, &mut ledger, 2);
                }
                assert_eq!(ledger.carry, [4_800, 6_000]);
                assert!(ledger.funding.iter().any(|value| *value != 0));
                let order = if reverse { [1, 0] } else { [0, 1] };
                let trade = Instruction {
                    program_id: world.env.program_id,
                    accounts: vec![
                        AccountMeta::new(world.owners[0].pubkey(), true),
                        AccountMeta::new(world.owners[1].pubkey(), true),
                        AccountMeta::new(world.env.market, false),
                        AccountMeta::new(world.portfolios[0], false),
                        AccountMeta::new(world.portfolios[1], false),
                    ],
                    data: world
                        .env
                        .batch_trade_no_cpi_ix(
                            world.portfolios[0],
                            world.portfolios[1],
                            order
                                .map(|asset| BatchTradeLeg {
                                    asset_index: asset as u16,
                                    market_id: world.env.asset_market_id(asset as u16),
                                    size_q: -2 * POS_SCALE as i128,
                                    exec_price: ANCHORS[asset],
                                    fee_bps: 0,
                                })
                                .to_vec(),
                        )
                        .encode(),
                };
                // Pre-sign alternatives with identical economic bytes. Different
                // CU limits avoid LiteSVM's failed-signature cache on later delivery.
                let retained =
                    [0, 1, 2].map(|nonce| sign(&world, std::slice::from_ref(&trade), nonce));
                let bytes = retained
                    .each_ref()
                    .map(|tx| bincode::serialize(tx).unwrap());
                let late = sign(
                    &world,
                    &[
                        trade.clone(),
                        Instruction {
                            program_id: world.env.program_id,
                            accounts: vec![],
                            data: vec![],
                        },
                    ],
                    3,
                );
                let late_bytes = bincode::serialize(&late).unwrap();
                let owners_before =
                    [0, 1].map(|actor| world.env.svm.get_account(&world.portfolios[actor]));
                world.env.svm.warp_to_slot(CHECKPOINT);
                for asset in order {
                    let sign = direction * if asset == 0 { 1 } else { -1 };
                    world.env.push_auth_mark_for_asset_as_admin(
                        asset as u16,
                        CHECKPOINT,
                        (ANCHORS[asset] as i128 - sign * 20) as u64,
                    );
                }
                ledger.carry = [0; 2];
                checkpoint(&world, &mut ledger, direction);
                let stale = InstructionError::Custom(PercolatorError::EngineStale as u32);
                if attempts {
                    world
                        .trace
                        .push("reject retained reduction before catch-up".into());
                    reject(&mut world, &retained[0], 2, stale.clone());
                    rejected += 1;
                    checkpoint(&world, &mut ledger, direction);
                }
                crank(&mut world, &mut ledger, direction, reverse, 3);
                assert_eq!(ledger.carry, [2_400, 3_000]);
                if attempts {
                    world
                        .trace
                        .push("reject retained reduction after partial catch-up".into());
                    reject(&mut world, &retained[1], 2, stale);
                    rejected += 1;
                    checkpoint(&world, &mut ledger, direction);
                }
                for slot in 4..=CHECKPOINT {
                    crank(&mut world, &mut ledger, direction, reverse, slot);
                }
                assert_eq!(ledger.carry, [7_200, 9_000]);
                assert_eq!(ledger.price, ANCHORS);
                assert_eq!(
                    [0, 1].map(|actor| world.env.svm.get_account(&world.portfolios[actor])),
                    owners_before
                );
                assert_eq!(
                    retained
                        .each_ref()
                        .map(|tx| bincode::serialize(tx).unwrap()),
                    bytes
                );
                assert_eq!(bincode::serialize(&late).unwrap(), late_bytes);
                if attempts {
                    world
                        .trace
                        .push("rollback successful retained reduction at invalid suffix".into());
                    reject(
                        &mut world,
                        &late,
                        3,
                        InstructionError::InvalidInstructionData,
                    );
                    rejected += 1;
                    checkpoint(&world, &mut ledger, direction);
                }
                let profile_before = carry_transport_exit::profiles(&world);
                let passive =
                    [2, 3].map(|actor| world.env.svm.get_account(&world.portfolios[actor]));
                world.trace.push(
                    "execute unchanged pre-signed reduction after checkpoint activation".into(),
                );
                let result = world
                    .env
                    .svm
                    .send_transaction(retained[2].clone())
                    .expect("retained reduction after public catch-up");
                world.max_cu = world.max_cu.max(result.compute_units_consumed);
                for asset in 0..2 {
                    ledger.lots[0][asset] -= 2;
                    ledger.lots[1][asset] += 2;
                }
                checkpoint(&world, &mut ledger, direction);
                assert_eq!(carry_transport_exit::profiles(&world), profile_before);
                assert_eq!(
                    [2, 3].map(|actor| world.env.svm.get_account(&world.portfolios[actor])),
                    passive
                );
                for slot in CHECKPOINT + 1..=EXIT {
                    world.env.svm.warp_to_slot(slot);
                    crank(&mut world, &mut ledger, direction, reverse, slot);
                }
                assert_eq!(ledger.carry, [6_800, 1_000]);
                assert!(ledger.latent_checks > 0);
                let final_profiles = carry_transport_exit::profiles(&world);
                pay(&mut world, &mut ledger, reverse);
                assert_eq!(carry_transport_exit::profiles(&world), final_profiles);
                let outcome = (ledger.price, ledger.funding, ledger.carry, ledger.value);
                if let Some(expected) = reference {
                    assert_eq!(outcome, expected);
                } else {
                    reference = Some(outcome);
                }
                peak = peak.max(world.max_cu);
                worlds += 1;
            }
        }
    }
    assert_eq!((worlds, rejected), (8, 12));
    assert_cu_within(
        "retained funding-checkpoint carry and owner payout",
        peak,
        u64::from(LIMIT),
    );
    eprintln!("row425 retained funding retry: {worlds} worlds, {rejected} exact rollbacks, 32 exact owner payouts; peak={peak} CU");
}
