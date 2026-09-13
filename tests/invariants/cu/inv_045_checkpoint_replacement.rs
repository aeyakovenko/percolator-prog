//! INV-045 / row425: overlapping public funding boundaries with fractional carry.
//! Same-slot replacement cancels one prospective premium; a later publication
//! must preserve the intervening premium until its owed interval is consumed.
//! Integral positions, AuthMark, zero fees and two pending boundaries only.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};
use std::collections::BTreeMap;

const CU_LIMIT: u32 = 600_000;
type Matcher = (Pubkey, Pubkey, Pubkey);

struct Journal {
    value: Ledger,
    publications: Vec<(u64, [u64; 2])>,
}

impl Journal {
    fn new(direction: i128) -> Self {
        Self {
            value: Ledger::new(),
            publications: vec![(0, targets(direction, 20))],
        }
    }

    fn publish(&mut self, slot: u64, prices: [u64; 2]) {
        assert_eq!(
            self.value.price, ANCHORS,
            "all replacements precede K movement"
        );
        assert_ne!(self.publications.last().unwrap().1, prices);
        if self.publications.last().unwrap().0 == slot {
            self.publications.pop();
        }
        self.publications.push((slot, prices));
        self.value.carry = [0; 2];
    }

    fn advance(&mut self) {
        let next = self.value.slot + 1;
        let target = self.publications.last().unwrap().1;
        // Funding belongs to the mark in force before this slot, even when the
        // price target has already been replaced by a later authenticated input.
        let funding_mark = self
            .publications
            .iter()
            .rev()
            .find(|(slot, _)| *slot < next)
            .unwrap()
            .1;
        for asset in 0..2 {
            let numerator = self.value.carry[asset] + ANCHORS[asset] * CAP_BPS;
            let old_price = i128::from(self.value.price[asset]);
            let sign = (i128::from(target[asset]) - old_price).signum();
            let price = old_price + sign * i128::from(numerator / 10_000);
            assert!((price - i128::from(target[asset])).abs() > 10);
            let rate = ((i128::from(funding_mark[asset]) - price) * 1_000_000_000 / price)
                .clamp(-RATE, RATE);
            assert_eq!(
                rate.abs(),
                RATE,
                "both premiums saturate the public rate cap"
            );
            let funding = -(rate * price).div_euclid(1_000_000_000);
            for actor in 0..4 {
                self.value.value[actor] +=
                    self.value.lots[actor][asset] * (price - old_price + funding);
            }
            self.value.price[asset] = price as u64;
            self.value.carry[asset] = numerator % 10_000;
            self.value.funding[asset] += funding;
        }
        self.value.slot = next;
    }

    fn check(&mut self, world: &World) {
        self.value.check(world);
        let slot = self.value.slot;
        let active = self
            .publications
            .iter()
            .rev()
            .find(|p| p.0 <= slot)
            .unwrap()
            .1;
        let pending = self.publications.iter().find(|p| p.0 > slot);
        let latest = self.publications.last().unwrap();
        let group = world.env.market_state().1;
        for (asset, profile) in carry_transport_exit::profiles(world).iter().enumerate() {
            assert_eq!(group.assets[asset].raw_oracle_target_price, latest.1[asset]);
            assert_eq!(profile.mark_ewma_e6, latest.1[asset]);
            assert_eq!(profile.mark_ewma_last_slot, latest.0);
            assert_eq!(profile.oracle_target_price_e6, latest.1[asset]);
            assert_eq!(profile.funding_mark_e6, active[asset]);
            assert_eq!(
                (
                    profile.funding_mark_pending_e6,
                    profile.funding_mark_pending_slot
                ),
                pending.map_or((0, 0), |p| (p.1[asset], p.0))
            );
        }
    }
}

fn targets(direction: i128, distance: i128) -> [u64; 2] {
    [0, 1].map(|asset| {
        (i128::from(ANCHORS[asset]) + direction * if asset == 0 { distance } else { -distance })
            as u64
    })
}

fn frame(world: &World, matcher: Matcher, tx: &Transaction) -> BTreeMap<Pubkey, Option<Account>> {
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
            matcher.0,
            matcher.1,
            matcher.2,
            solana_sdk::sysvar::clock::ID,
        ])
        .map(|key| (key, world.env.svm.get_account(&key)))
        .collect()
}

#[track_caller]
fn transact(world: &mut World, matcher: Matcher, prefix: &[Instruction], reject: bool) -> usize {
    world.env.svm.expire_blockhash();
    let mut instructions = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT),
    ];
    instructions.extend_from_slice(prefix);
    if reject {
        instructions.push(Instruction {
            program_id: world.env.program_id,
            accounts: vec![],
            data: vec![],
        });
    }
    let mut signers = vec![&world.env.payer];
    for owner in world.owners.iter().chain(std::iter::once(&world.env.admin)) {
        if instructions
            .iter()
            .flat_map(|ix| &ix.accounts)
            .any(|meta| meta.is_signer && meta.pubkey == owner.pubkey())
        {
            signers.push(owner);
        }
    }
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&world.env.payer.pubkey()),
        &signers,
        world.env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
    let mut before = frame(world, matcher, &tx);
    let logs = if reject {
        before
            .get_mut(&world.env.payer.pubkey())
            .unwrap()
            .as_mut()
            .unwrap()
            .lamports -= u64::from(tx.message.header.num_required_signatures)
            * FeeStructure::default().lamports_per_signature;
        let failed = world.env.svm.send_transaction(tx.clone()).unwrap_err();
        assert_eq!(
            failed.err,
            TransactionError::InstructionError(
                (prefix.len() + 2) as u8,
                InstructionError::InvalidInstructionData
            ),
            "{:?}",
            failed.meta.logs
        );
        assert_eq!(
            failed
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", world.env.program_id))
                .count(),
            prefix.len()
        );
        assert_eq!(frame(world, matcher, &tx), before, "{:?}", world.trace);
        world.max_cu = world.max_cu.max(failed.meta.compute_units_consumed);
        failed.meta.logs
    } else {
        let result = world
            .env
            .svm
            .send_transaction(tx)
            .unwrap_or_else(|e| panic!("{e:?}"));
        world.max_cu = world.max_cu.max(result.compute_units_consumed);
        result.logs
    };
    assert!(world.max_cu <= u64::from(CU_LIMIT));
    logs.iter()
        .filter(|line| **line == format!("Program {} success", spl_token::ID))
        .count()
}

fn publish(
    world: &mut World,
    journal: &mut Journal,
    matcher: Matcher,
    prices: [u64; 2],
    reverse: bool,
    rollback: bool,
) {
    let slot = world.env.svm.get_sysvar::<Clock>().slot;
    world.trace.push(format!(
        "clock={slot}, frontier={}: publish {prices:?}",
        journal.value.slot
    ));
    let order = if reverse { [1, 0] } else { [0, 1] };
    let instructions = order.map(|asset| {
        let sequences = world.env.control_sequences(asset);
        Instruction {
            program_id: world.env.program_id,
            accounts: vec![
                AccountMeta::new(world.env.admin.pubkey(), true),
                AccountMeta::new(world.env.market, false),
            ],
            data: ProgInstruction::PushAuthMark {
                asset_index: asset as u16,
                market_id: world.env.asset_market_id(asset as u16),
                now_slot: slot,
                mark_e6: prices[asset],
                observation_sequence: next_control_sequence(sequences.oracle_observation),
                authority_epoch: sequences.authority_epoch,
            }
            .encode(),
        }
    });
    if rollback {
        transact(world, matcher, &instructions, true);
        journal.check(world);
    }
    let owners_before = world.portfolios.map(|key| world.env.svm.get_account(&key));
    transact(world, matcher, &instructions, false);
    journal.publish(slot, prices);
    journal.check(world);
    assert_eq!(
        world.portfolios.map(|key| world.env.svm.get_account(&key)),
        owners_before
    );
}

fn crank(
    world: &mut World,
    journal: &mut Journal,
    matcher: Matcher,
    reverse: bool,
    rollback: bool,
) {
    world.trace.push(format!(
        "clock={}: accrue next={}",
        world.env.svm.get_sysvar::<Clock>().slot,
        journal.value.slot + 1
    ));
    let instruction = Instruction {
        program_id: world.env.program_id,
        accounts: vec![
            AccountMeta::new(world.env.payer.pubkey(), true),
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(world.portfolios[2], false),
        ],
        data: ProgInstruction::PermissionlessCrank {
            now_slot: world.env.svm.get_sysvar::<Clock>().slot,
            observations: crank_observations_for_assets(if reverse { &[1, 0] } else { &[0, 1] }),
        }
        .encode(),
    };
    if rollback {
        transact(world, matcher, std::slice::from_ref(&instruction), true);
        journal.check(world);
    }
    let absent = [0, 1, 3].map(|actor| world.env.svm.get_account(&world.portfolios[actor]));
    transact(world, matcher, &[instruction], false);
    journal.advance();
    journal.check(world);
    assert_eq!(
        [0, 1, 3].map(|actor| world.env.svm.get_account(&world.portfolios[actor])),
        absent
    );
}

fn reduce(
    world: &mut World,
    journal: &mut Journal,
    matcher: Matcher,
    history: History,
    cpi: bool,
    rollback: bool,
) {
    let profiles = carry_transport_exit::profiles(world);
    if cpi {
        world.env.set_matcher_config_with_trade_fee_cap(
            matcher.0,
            &world.owners[1],
            world.portfolios[1],
            matcher.1,
            matcher.2,
            1,
            0,
        );
        journal.check(world);
        assert_eq!(carry_transport_exit::profiles(world), profiles);
    }
    let order = if history.reverse { [1, 0] } else { [0, 1] };
    let chunks = if history.batch {
        vec![order.to_vec()]
    } else {
        order.map(|asset| vec![asset]).to_vec()
    };
    for assets in chunks {
        world.trace.push(format!(
            "frontier={}: reduce one lot {assets:?}, cpi={cpi}",
            journal.value.slot
        ));
        let env = &world.env;
        let [a, b, _, _] = world.portfolios;
        let size = -(POS_SCALE as i128);
        let data = match (history.batch, cpi) {
            (true, true) => env.batch_trade_cpi_ix(
                a,
                b,
                assets
                    .iter()
                    .map(|asset| BatchTradeCpiLeg {
                        asset_index: *asset,
                        market_id: env.asset_market_id(*asset),
                        size_q: size,
                        fee_bps: 0,
                        limit_price: journal.value.price[*asset as usize],
                    })
                    .collect(),
            ),
            (true, false) => env.batch_trade_no_cpi_ix(
                a,
                b,
                assets
                    .iter()
                    .map(|asset| BatchTradeLeg {
                        asset_index: *asset,
                        market_id: env.asset_market_id(*asset),
                        size_q: size,
                        fee_bps: 0,
                        exec_price: journal.value.price[*asset as usize],
                    })
                    .collect(),
            ),
            (false, true) => env.trade_cpi_ix(
                a,
                b,
                assets[0],
                size,
                0,
                journal.value.price[assets[0] as usize],
            ),
            (false, false) => env.trade_no_cpi_ix(
                a,
                b,
                assets[0],
                size,
                journal.value.price[assets[0] as usize],
                0,
            ),
        };
        let accounts = if cpi {
            vec![
                AccountMeta::new(world.owners[0].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(a, false),
                AccountMeta::new(b, false),
                AccountMeta::new_readonly(matcher.0, false),
                AccountMeta::new(matcher.1, false),
                AccountMeta::new_readonly(matcher.2, false),
            ]
        } else {
            vec![
                AccountMeta::new(world.owners[0].pubkey(), true),
                AccountMeta::new(world.owners[1].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(a, false),
                AccountMeta::new(b, false),
            ]
        };
        let instruction = Instruction {
            program_id: env.program_id,
            accounts,
            data: data.encode(),
        };
        if rollback {
            transact(world, matcher, std::slice::from_ref(&instruction), true);
            journal.check(world);
        }
        let absent = [2, 3].map(|actor| world.env.svm.get_account(&world.portfolios[actor]));
        transact(world, matcher, &[instruction], false);
        for asset in assets {
            journal.value.lots[0][asset as usize] -= 1;
            journal.value.lots[1][asset as usize] += 1;
        }
        journal.check(world);
        assert_eq!(carry_transport_exit::profiles(world), profiles);
        assert_eq!(
            [2, 3].map(|actor| world.env.svm.get_account(&world.portfolios[actor])),
            absent
        );
    }
}

#[test]
fn v16_program_overlapping_target_replacements_preserve_carry_funding_and_resolved_payouts() {
    let mut worlds = 0;
    let mut peak_cu = 0;
    let mut rollbacks = 0;
    let mut spl_rollbacks = 0;
    for direction in [-1, 1] {
        let mut reference = None;
        for batch in [false, true] {
            for rollback in [false, true] {
                let history = History {
                    direction,
                    batch,
                    split: false,
                    placement: 0,
                    reverse: batch,
                };
                let mut world = World::with_funding(history, RATE as u64);
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
                let matcher = auth_matcher_for_lp_via_system_create(
                    &mut world.env,
                    &world.owners[1],
                    world.portfolios[1],
                );
                let mut journal = Journal::new(direction);
                journal.check(&world);
                let passive = world.env.svm.get_account(&world.portfolios[3]);
                for slot in 1..=2 {
                    world.env.svm.warp_to_slot(slot);
                    crank(&mut world, &mut journal, matcher, batch, false);
                }
                assert_eq!(journal.value.carry, [4_800, 6_000]);
                reduce(&mut world, &mut journal, matcher, history, true, rollback);
                world.env.svm.warp_to_slot(5);
                publish(
                    &mut world,
                    &mut journal,
                    matcher,
                    targets(direction, 30),
                    batch,
                    rollback,
                );
                crank(&mut world, &mut journal, matcher, batch, false);
                assert_eq!(journal.value.carry, [2_400, 3_000]);
                publish(
                    &mut world,
                    &mut journal,
                    matcher,
                    targets(direction, -20),
                    batch,
                    rollback,
                );
                crank(&mut world, &mut journal, matcher, batch, false);
                assert_eq!(journal.value.carry, [2_400, 3_000]);
                world.env.svm.warp_to_slot(7);
                publish(
                    &mut world,
                    &mut journal,
                    matcher,
                    targets(direction, 40),
                    batch,
                    rollback,
                );
                assert_eq!(
                    journal.publications,
                    vec![
                        (0, targets(direction, 20)),
                        (5, targets(direction, -20)),
                        (7, targets(direction, 40))
                    ]
                );
                for _ in 5..=7 {
                    crank(&mut world, &mut journal, matcher, batch, rollback);
                }
                assert_eq!(journal.value.carry, [7_200, 9_000]);
                assert_eq!(
                    journal.value.funding,
                    if direction == 1 { [2, 5] } else { [5, 2] }
                );
                reduce(&mut world, &mut journal, matcher, history, false, rollback);
                for slot in 8..=9 {
                    world.env.svm.warp_to_slot(slot);
                    crank(&mut world, &mut journal, matcher, batch, false);
                }
                assert_eq!(journal.value.carry, [2_000, 5_000]);
                assert_eq!(journal.value.price, targets(direction, 1));
                assert_eq!(
                    journal.value.funding,
                    if direction == 1 { [2, 7] } else { [7, 2] }
                );
                reduce(&mut world, &mut journal, matcher, history, true, rollback);
                assert_eq!(world.env.svm.get_account(&world.portfolios[3]), passive);
                assert!(journal.value.latent_checks >= 16);
                let endpoint = Economics {
                    price: journal.value.price,
                    carry: journal.value.carry,
                    lots: journal.value.lots,
                    entitlement: journal.value.value,
                    vault: PRINCIPAL.map(u128::from).iter().sum(),
                };
                let accrual = |world: &World| {
                    let group = world.env.market_state().1;
                    [0, 1].map(|asset| {
                        let a = group.assets[asset];
                        (
                            a.slot_last,
                            a.k_long,
                            a.k_short,
                            a.f_long_num,
                            a.f_short_num,
                        )
                    })
                };
                let frozen = accrual(&world);
                let mut close_rollbacks = 0;
                let mut close_spl_rollbacks = 0;
                let mut paying_owners = [false; 4];
                let paid = carry_transport_exit::pay_resolved_with_residue(
                    &mut world,
                    &endpoint,
                    batch,
                    100,
                    0,
                    |world, actor, instruction| {
                        if rollback {
                            let payments =
                                transact(world, matcher, std::slice::from_ref(instruction), true);
                            close_spl_rollbacks += payments;
                            paying_owners[actor] |= payments != 0;
                            close_rollbacks += 1;
                        }
                    },
                );
                assert_eq!(
                    accrual(&world),
                    frozen,
                    "resolved exits cannot accrue K/F after slot 9"
                );
                assert_eq!(
                    world.env.market_state().1.backing_provider_earnings_total,
                    0
                );
                assert!(world.max_cu <= u64::from(CU_LIMIT));
                if let Some(expected) = reference {
                    assert_eq!(paid, expected);
                } else {
                    reference = Some(paid);
                }
                if rollback {
                    assert!(
                        paying_owners.into_iter().all(|paid| paid),
                        "each owner has a paying rollback prefix"
                    );
                    rollbacks += 6 + if batch { 3 } else { 6 } + close_rollbacks;
                    spl_rollbacks += close_spl_rollbacks;
                }
                eprintln!("row425 checkpoint replacement: direction={direction}, batch={batch}, rollback={rollback}, payouts={paid:?}, CU={}", world.max_cu);
                peak_cu = peak_cu.max(world.max_cu);
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 8);
    assert_eq!(rollbacks, 88);
    eprintln!("row425 overlapping checkpoints: worlds={worlds}, rollbacks={rollbacks}, SPL prefixes rolled back={spl_rollbacks}, payouts={}, peak CU={peak_cu}, bound={CU_LIMIT}", worlds * 4);
}
