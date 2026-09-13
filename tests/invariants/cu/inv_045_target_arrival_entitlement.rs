//! Scope H / row 425: target arrival, plateau and resumed carry across owner reductions.
//! Bounded INV-045/038/052/085/086 evidence; integral lots and zero funding/fees.

use super::*;
use carry_transport_exit::{pay_resolved_with_residue, profiles};
use num_bigint::BigUint;
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

#[path = "inv_045_moving_reset_routes.rs"]
mod moving_reset_routes;

struct Book {
    economics: Economics,
    target: [u64; 2],
    start: [u64; 2],
    anchor: [u64; 2],
    episode_anchor: [u64; 2],
    since: [u64; 2],
    slot: u64,
    checks: usize,
    latent_checks: usize,
    rollbacks: usize,
}

impl Book {
    fn new(direction: i128) -> Self {
        Self {
            economics: Economics {
                price: ANCHORS,
                carry: [0; 2],
                lots: OPEN_LOTS.map(|lots| [lots[0], -lots[1]]),
                entitlement: PRINCIPAL.map(i128::from),
                vault: PRINCIPAL.map(u128::from).iter().sum(),
            },
            target: [0, 1].map(|asset| {
                (ANCHORS[asset] as i128 + direction * if asset == 0 { 20 } else { -20 }) as u64
            }),
            start: ANCHORS,
            anchor: ANCHORS,
            episode_anchor: ANCHORS,
            since: [0; 2],
            slot: 0,
            checks: 0,
            latent_checks: 0,
            rollbacks: 0,
        }
    }

    fn advance(&mut self, slot: u64) {
        assert!(slot > self.slot);
        for asset in 0..2 {
            // Whole-episode arithmetic is independent of the deployed one-slot recurrence.
            let numerator = BigUint::from(self.episode_anchor[asset])
                * BigUint::from(CAP_BPS)
                * BigUint::from(slot - self.since[asset]);
            let available = u64::try_from(&numerator / BigUint::from(10_000u64)).unwrap();
            let distance = self.start[asset].abs_diff(self.target[asset]);
            let movement = available.min(distance);
            let sign = (self.target[asset] as i128 - self.start[asset] as i128).signum();
            let price = (self.start[asset] as i128 + sign * movement as i128) as u64;
            self.economics.carry[asset] = if available >= distance {
                self.anchor[asset] = self.target[asset];
                0
            } else {
                u64::try_from(&numerator % BigUint::from(10_000u64)).unwrap()
            };
            for actor in 0..4 {
                self.economics.entitlement[actor] += self.economics.lots[actor][asset]
                    * (price as i128 - self.economics.price[asset] as i128);
            }
            self.economics.price[asset] = price;
        }
        self.slot = slot;
    }

    fn publish(&mut self, asset: usize, target: u64) {
        if target != self.target[asset] {
            self.start[asset] = self.economics.price[asset];
            self.since[asset] = self.slot;
            self.episode_anchor[asset] = self.anchor[asset];
            self.economics.carry[asset] = 0;
            self.target[asset] = target;
        }
    }

    fn check(&mut self, world: &World) {
        self.checks += 1;
        let env = &world.env;
        let group = env.market_state().1;
        for asset in 0..2 {
            let state = group.assets[asset];
            let profile = profiles(world)[asset];
            let k =
                (self.economics.price[asset] as i128 - ANCHORS[asset] as i128) * ADL_ONE as i128;
            let oi = self
                .economics
                .lots
                .iter()
                .map(|q| q[asset].max(0))
                .sum::<i128>() as u128
                * POS_SCALE;
            assert_eq!(state.slot_last, self.slot, "{:?}", world.trace);
            assert_eq!(state.effective_price, self.economics.price[asset]);
            assert_eq!(state.fund_px_last, self.anchor[asset]);
            assert_eq!(state.raw_oracle_target_price, self.target[asset]);
            assert_eq!(profile.oracle_target_price_e6, self.target[asset]);
            assert_eq!(profile.mark_ewma_e6, self.target[asset]);
            assert_eq!(
                u64::from(profile.price_move_remainder_bps_num),
                self.economics.carry[asset]
            );
            assert_eq!((state.k_long, state.k_short), (k, -k));
            assert_eq!((state.f_long_num, state.f_short_num), (0, 0));
            assert_eq!((state.a_long, state.a_short), (ADL_ONE, ADL_ONE));
            assert_eq!((state.b_long_num, state.b_short_num), (0, 0));
            assert_eq!((state.oi_eff_long_q, state.oi_eff_short_q), (oi, oi));
            assert_eq!(
                (
                    state.pending_obligation_count_long,
                    state.pending_obligation_count_short
                ),
                (0, 0)
            );
        }
        let mut capital = 0;
        let mut positive_pnl = 0;
        let mut value = [0; 4];
        for actor in 0..4 {
            let account = env.portfolio_state(world.portfolios[actor]);
            let mut latent = 0;
            for asset in 0..2 {
                let leg = account
                    .legs
                    .iter()
                    .map(|leg| leg.try_to_runtime().unwrap())
                    .find(|leg| leg.active && leg.asset_index as usize == asset)
                    .unwrap();
                let lots = self.economics.lots[actor][asset];
                assert_eq!(leg.basis_pos_q, lots * POS_SCALE as i128);
                assert_eq!(leg.a_basis, ADL_ONE);
                assert_eq!((leg.b_rem, leg.f_snap), (0, 0));
                assert_eq!(leg.k_snap % ADL_ONE as i128, 0);
                let k =
                    lots.signum() * (self.economics.price[asset] as i128 - ANCHORS[asset] as i128);
                latent += lots.abs() * (k - leg.k_snap / ADL_ONE as i128);
            }
            self.latent_checks += usize::from(latent != 0);
            value[actor] = account.capital.get() as i128 + account.pnl.get() + latent;
            assert_eq!(env.token_amount(world.tokens[actor]), 0);
            capital += account.capital.get();
            positive_pnl += account.pnl.get().max(0) as u128;
        }
        assert_eq!(
            value, self.economics.entitlement,
            "owner ledger: {:?}",
            world.trace
        );
        assert_eq!(value.iter().sum::<i128>(), self.economics.vault as i128);
        assert_eq!((group.c_tot, group.pnl_pos_tot), (capital, positive_pnl));
        assert_eq!(
            (group.insurance, group.backing_provider_earnings_total),
            (0, 0)
        );
        assert_eq!(group.vault, self.economics.vault);
        assert_eq!(env.token_amount(env.vault) as u128, self.economics.vault);
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!(mint.supply as u128, self.economics.vault);
        assert_eq!(mint.mint_authority, COption::None);
    }
}

// The identical economic instruction retries after a completed prefix is rolled back.
fn execute(world: &mut World, book: &mut Book, ix: Instruction, actors: &[usize], rollback: bool) {
    let env = &mut world.env;
    let mut signers = vec![&env.payer];
    if actors.is_empty() {
        signers.push(&env.admin);
    } else {
        signers.extend(actors.iter().map(|&actor| &world.owners[actor]));
    }
    if rollback {
        env.svm.expire_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[
                heap_ix(),
                cu_ix(),
                ix.clone(),
                Instruction {
                    program_id: env.program_id,
                    accounts: vec![],
                    data: vec![255],
                },
            ],
            Some(&env.payer.pubkey()),
            &signers,
            env.svm.latest_blockhash(),
        );
        let mut keys = vec![env.market, env.vault, env.mint];
        keys.extend(world.portfolios);
        keys.extend(world.tokens);
        keys.extend(world.owners.iter().map(Signer::pubkey));
        keys.extend(tx.message.account_keys.iter().copied());
        keys.sort();
        keys.dedup();
        let mut before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
        let payer = keys
            .iter()
            .position(|key| *key == env.payer.pubkey())
            .unwrap();
        before[payer].as_mut().unwrap().lamports -=
            5_000 * u64::from(tx.message.header.num_required_signatures);
        let error = env.svm.send_transaction(tx).expect_err("invalid suffix");
        assert_eq!(
            error.err,
            TransactionError::InstructionError(3, InstructionError::InvalidInstructionData)
        );
        assert_eq!(
            error
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", env.program_id))
                .count(),
            1
        );
        assert_eq!(
            keys.iter()
                .map(|key| env.svm.get_account(key))
                .collect::<Vec<_>>(),
            before
        );
        world.max_cu = world.max_cu.max(error.meta.compute_units_consumed);
        book.rollbacks += 1;
    }
    env.svm.expire_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), ix],
        Some(&env.payer.pubkey()),
        &signers,
        env.svm.latest_blockhash(),
    );
    let meta = env
        .svm
        .send_transaction(tx)
        .unwrap_or_else(|error| panic!("{error:?}: {:?}", world.trace));
    world.max_cu = world.max_cu.max(meta.compute_units_consumed);
}

fn publish(world: &mut World, book: &mut Book, asset: usize, target: u64, rollback: bool) {
    world.trace.push(format!(
        "slot={}: PushAuthMark(asset={asset}, target={target})",
        book.slot
    ));
    let owners = world.portfolios.map(|key| world.env.svm.get_account(&key));
    let other_profile = profiles(world)[1 - asset];
    let sequences = world.env.control_sequences(asset);
    let ix = Instruction {
        program_id: world.env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(world.env.admin.pubkey(), true),
            AccountMeta::new(world.env.market, false),
        ],
        data: ProgInstruction::PushAuthMark {
            market_id: world.env.asset_market_id(asset as u16),
            asset_index: asset as u16,
            now_slot: book.slot,
            mark_e6: target,
            observation_sequence: next_control_sequence(sequences.oracle_observation),
            authority_epoch: sequences.authority_epoch,
        }
        .encode(),
    };
    execute(world, book, ix, &[], rollback);
    book.publish(asset, target);
    book.check(world);
    assert_eq!(
        world.portfolios.map(|key| world.env.svm.get_account(&key)),
        owners
    );
    assert_eq!(profiles(world)[1 - asset], other_profile);
}

fn crank(world: &mut World, book: &mut Book, actor: usize, slot: u64, reverse: bool) {
    world
        .trace
        .push(format!("slot={slot}: PermissionlessCrank(actor={actor})"));
    let absent = world.env.svm.get_account(&world.portfolios[3]);
    let before = profiles(world);
    world.env.svm.warp_to_slot(slot);
    world.env.svm.expire_blockhash();
    let result = world.env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: slot,
            observations: crank_observations_for_assets(&if reverse { [1, 0] } else { [0, 1] }),
        },
        vec![
            AccountMeta::new(world.env.payer.pubkey(), true),
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(world.portfolios[actor], false),
        ],
        &[],
    );
    world.max_cu = world
        .max_cu
        .max(result.unwrap_or_else(|error| panic!("{error}: {:?}", world.trace)));
    if slot > book.slot {
        book.advance(slot);
    } else {
        assert_eq!(profiles(world), before);
    }
    book.check(world);
    assert_eq!(world.env.svm.get_account(&world.portfolios[3]), absent);
}

fn reduce(world: &mut World, book: &mut Book, split: bool) {
    let before = profiles(world);
    let absent = [2, 3].map(|actor| world.env.svm.get_account(&world.portfolios[actor]));
    for part in 0..if split { 2 } else { 1 } {
        let lots = if split { 1 } else { 2 };
        let order = if split { [1, 0] } else { [0, 1] };
        let legs: Vec<_> = order
            .into_iter()
            .map(|asset| BatchTradeLeg {
                asset_index: asset as u16,
                market_id: world.env.asset_market_id(asset as u16),
                size_q: -book.economics.lots[0][asset].signum()
                    * lots
                    * if asset == 1 && matches!(book.slot, 8 | 14) {
                        2
                    } else {
                        1
                    }
                    * POS_SCALE as i128,
                exec_price: book.economics.price[asset],
                fee_bps: 0,
            })
            .collect();
        let chunks = if split {
            legs.into_iter().map(|leg| vec![leg]).collect()
        } else {
            vec![legs]
        };
        for legs in chunks {
            world.trace.push(format!(
                "slot={}: reduction(part={part}, legs={legs:?})",
                book.slot
            ));
            let instruction = if split {
                let leg = &legs[0];
                world.env.trade_no_cpi_ix(
                    world.portfolios[0],
                    world.portfolios[1],
                    leg.asset_index,
                    leg.size_q,
                    leg.exec_price,
                    0,
                )
            } else {
                world.env.batch_trade_no_cpi_ix(
                    world.portfolios[0],
                    world.portfolios[1],
                    legs.clone(),
                )
            };
            let ix = Instruction {
                program_id: world.env.program_id,
                accounts: vec![
                    AccountMeta::new_readonly(world.owners[0].pubkey(), true),
                    AccountMeta::new_readonly(world.owners[1].pubkey(), true),
                    AccountMeta::new(world.env.market, false),
                    AccountMeta::new(world.portfolios[0], false),
                    AccountMeta::new(world.portfolios[1], false),
                ],
                data: instruction.encode(),
            };
            execute(world, book, ix, &[0, 1], part == 0);
            for leg in legs {
                book.economics.lots[0][leg.asset_index as usize] += leg.size_q / POS_SCALE as i128;
                book.economics.lots[1][leg.asset_index as usize] -= leg.size_q / POS_SCALE as i128;
            }
            book.check(world);
            assert_eq!(profiles(world), before);
            assert_eq!(
                [2, 3].map(|actor| world.env.svm.get_account(&world.portfolios[actor])),
                absent
            );
        }
    }
}

#[test]
fn v16_program_target_arrival_plateau_and_restart_preserve_interleaved_owner_entitlement() {
    let mut max_cu = 0;
    let mut rollbacks = 0;
    let mut checks = 0;
    for direction in [-1, 1] {
        let mut baseline = None;
        for split in [false, true] {
            let history = History {
                direction,
                batch: !split,
                split,
                placement: 0,
                reverse: split,
            };
            let mut world = World::with_funding_and_accrual_limit(history, 0, 4);
            // Align each owner's two price exposures so prior positive source claims
            // are never consumed by a later loss in this integral-entitlement oracle.
            for actor in [0, 2] {
                world.env.svm.expire_blockhash();
                world.env.trade_asset_with_cu(
                    1,
                    &world.owners[actor],
                    world.portfolios[actor],
                    &world.owners[actor + 1],
                    world.portfolios[actor + 1],
                    -2 * OPEN_LOTS[actor][1] * POS_SCALE as i128,
                    ANCHORS[1],
                    0,
                );
            }
            world.trace.push("slot=0: TradeNoCpi reverses asset-1 exposure for both owner pairs at the unchanged entry price".into());
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
            let mut book = Book::new(direction);
            book.check(&world);
            let absent = world.env.svm.get_account(&world.portfolios[3]);
            for endpoint in [5, 8, 11, 14, 17] {
                while book.slot < endpoint {
                    let next = if split {
                        book.slot + 1
                    } else {
                        (book.slot + 4).min(endpoint)
                    };
                    crank(&mut world, &mut book, 2, next, split);
                }
                if endpoint == 11 {
                    assert_eq!(book.economics.price, book.target);
                    assert_eq!(book.economics.carry, [0, 0]);
                } else {
                    assert!(book.economics.carry.iter().all(|carry| *carry != 0));
                }
                reduce(&mut world, &mut book, split);
                for asset in 0..2 {
                    let sign = direction * if asset == 0 { 1 } else { -1 };
                    if endpoint == 5 {
                        let target = (book.economics.price[asset] as i128 + sign) as u64;
                        publish(&mut world, &mut book, asset, target, true);
                    } else {
                        let target = book.target[asset];
                        publish(&mut world, &mut book, asset, target, endpoint == 8);
                        if endpoint == 11 {
                            let target = (ANCHORS[asset] as i128 + sign * 20) as u64;
                            publish(&mut world, &mut book, asset, target, true);
                        }
                    }
                }
            }
            assert_eq!(
                book.economics.carry,
                if direction == -1 {
                    [4_112, 8_288]
                } else {
                    [4_688, 7_712]
                }
            );
            assert_eq!(book.economics.lots, [[3, -3], [-3, 3], [7, -11], [-7, 11]]);
            assert_eq!(
                book.economics.entitlement,
                [
                    PRINCIPAL[0] as i128 + 60 * direction,
                    PRINCIPAL[1] as i128 - 60 * direction,
                    PRINCIPAL[2] as i128 + 54 * direction,
                    PRINCIPAL[3] as i128 - 54 * direction,
                ]
            );
            assert!(book.latent_checks > 0);
            assert_eq!(world.env.svm.get_account(&world.portfolios[3]), absent);
            let paid =
                pay_resolved_with_residue(&mut world, &book.economics, split, 100, 0, |_, _, _| {});
            if let Some(expected) = &baseline {
                assert_eq!(&(book.economics.clone(), paid), expected);
            } else {
                baseline = Some((book.economics.clone(), paid));
            }
            eprintln!("Scope H direction={direction}, split={split}: paid={paid:?}, checks={}, rollbacks={}", book.checks, book.rollbacks);
            max_cu = max_cu.max(world.max_cu);
            rollbacks += book.rollbacks;
            checks += book.checks;
        }
    }
    assert!(max_cu < 600_000, "{max_cu}");
    assert_eq!(rollbacks, 54);
    eprintln!("Scope H: 4 histories, 16 payouts, {checks} input-ledger checks, {rollbacks} complete rollbacks, peak {max_cu} CU");
}
