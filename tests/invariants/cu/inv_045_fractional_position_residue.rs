//! INV-045 / row 425: price-cap carry composes with fractional K settlement.
//! Public split/aggregate reductions must preserve owner-local floors and leave
//! complementary rounding residue outside every owner's payout. Zero funding,
//! fees and unit ADL isolate this bounded INV-024/038/041/052 composition.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

struct Ledger {
    q: [[i128; 2]; 4],
    value: [i128; 4],
    price: [u64; 2],
    slot: u64,
    residue_num: i128,
    latent_checks: usize,
}

impl Ledger {
    fn new() -> Self {
        Self {
            q: OPEN_LOTS.map(|legs| legs.map(|q| q * POS_SCALE as i128)),
            value: PRINCIPAL.map(i128::from),
            price: ANCHORS,
            slot: 0,
            residue_num: 0,
            latent_checks: 0,
        }
    }

    fn accrue(&mut self, direction: i128, slot: u64) {
        for asset in 0..2 {
            let sign = direction * if asset == 0 { 1 } else { -1 };
            let price = ANCHORS[asset] as i128
                + sign * i128::from(ANCHORS[asset] * CAP_BPS * slot / 10_000);
            // Each asset moves once in this family; no incremental K-floor law is assumed.
            assert!((price - ANCHORS[asset] as i128).abs() <= 1);
            let delta = price - i128::from(self.price[asset]);
            for actor in 0..4 {
                let numerator = self.q[actor][asset] * delta;
                let atoms = numerator.div_euclid(POS_SCALE as i128);
                let remainder = numerator.rem_euclid(POS_SCALE as i128);
                assert_eq!(numerator, atoms * POS_SCALE as i128 + remainder);
                self.value[actor] += atoms;
                self.residue_num += remainder;
            }
            self.price[asset] = price as u64;
        }
        self.slot = slot;
    }

    fn check(&mut self, world: &World, direction: i128) {
        let env = &world.env;
        let g = env.market_state().1;
        let market = env.svm.get_account(&env.market).unwrap();
        let mut capital = 0;
        let mut positive_pnl = 0;
        for asset in 0..2 {
            let a = g.assets[asset];
            let profile = state::read_asset_oracle_profile(&market.data, asset).unwrap();
            assert_eq!(a.slot_last, self.slot);
            assert_eq!(a.effective_price, self.price[asset]);
            assert_eq!(a.fund_px_last, ANCHORS[asset]);
            assert_eq!(
                a.raw_oracle_target_price as i128,
                ANCHORS[asset] as i128 + direction * if asset == 0 { 20 } else { -20 }
            );
            assert_eq!(
                u64::from(profile.price_move_remainder_bps_num),
                ANCHORS[asset] * CAP_BPS * self.slot % 10_000
            );
            let k = (self.price[asset] as i128 - ANCHORS[asset] as i128) * ADL_ONE as i128;
            assert_eq!((a.k_long, a.k_short), (k, -k));
            assert_eq!((a.a_long, a.a_short), (ADL_ONE, ADL_ONE));
            assert_eq!((a.f_long_num, a.f_short_num), (0, 0));
            assert_eq!((a.b_long_num, a.b_short_num), (0, 0));
            assert_eq!(
                (
                    a.pending_obligation_count_long,
                    a.pending_obligation_count_short
                ),
                (0, 0)
            );
            let long = self.q.iter().map(|legs| legs[asset].max(0) as u128).sum();
            assert_eq!((a.oi_eff_long_q, a.oi_eff_short_q), (long, long));
        }
        for actor in 0..4 {
            let account = env.portfolio_state(world.portfolios[actor]);
            let mut latent = 0;
            for asset in 0..2 {
                let leg = account
                    .legs
                    .iter()
                    .map(|leg| leg.try_to_runtime().expect("decode stored leg"))
                    .find(|leg| leg.active && leg.asset_index as usize == asset);
                let Some(leg) = leg else {
                    assert_eq!(self.q[actor][asset], 0);
                    continue;
                };
                assert_eq!(leg.basis_pos_q, self.q[actor][asset]);
                assert_eq!(leg.a_basis, ADL_ONE);
                assert_eq!((leg.b_rem, leg.f_snap), (0, 0));
                let k = if leg.basis_pos_q > 0 {
                    g.assets[asset].k_long
                } else {
                    g.assets[asset].k_short
                };
                latent += (leg.basis_pos_q.abs() * (k - leg.k_snap))
                    .div_euclid(POS_SCALE as i128 * ADL_ONE as i128);
            }
            self.latent_checks += usize::from(latent != 0);
            let paid = i128::from(env.token_amount(world.tokens[actor]));
            assert_eq!(
                account.capital.get() as i128 + account.pnl.get() + latent + paid,
                self.value[actor],
                "actor={actor}; {:?}",
                world.trace
            );
            capital += account.capital.get();
            positive_pnl += account.pnl.get().max(0) as u128;
        }
        assert_eq!((g.c_tot, g.pnl_pos_tot), (capital, positive_pnl));
        assert_eq!(g.insurance, 0);
        assert!(g.insurance_domain_budget.iter().all(|amount| *amount == 0));
        assert_eq!(g.backing_provider_earnings_total, 0);
        for bucket in &g.source_backing_buckets {
            assert_eq!(bucket.utilization_fee_earnings, 0);
        }
        let total = PRINCIPAL.map(u128::from).iter().sum::<u128>();
        assert_eq!(self.residue_num % POS_SCALE as i128, 0);
        assert_eq!(
            self.value.iter().sum::<i128>() + self.residue_num / POS_SCALE as i128,
            total as i128
        );
        assert_eq!(g.vault, env.token_amount(env.vault) as u128);
        assert_eq!(
            g.vault
                + world
                    .tokens
                    .map(|key| env.token_amount(key) as u128)
                    .iter()
                    .sum::<u128>(),
            total
        );
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!(mint.supply as u128, total);
        assert_eq!(
            mint.mint_authority,
            solana_program::program_option::COption::None
        );
    }
}

fn crank_ix(world: &World, actor: usize, assets: &[u16]) -> Instruction {
    Instruction {
        program_id: world.env.program_id,
        accounts: vec![
            AccountMeta::new(world.env.payer.pubkey(), true),
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(world.portfolios[actor], false),
        ],
        data: ProgInstruction::PermissionlessCrank {
            now_slot: world.env.svm.get_sysvar::<Clock>().slot,
            observations: crank_observations_for_assets(assets),
        }
        .encode(),
    }
}

fn reject_suffix(world: &mut World, prefix: Instruction, actors: &[usize]) -> u64 {
    let suffix = Instruction {
        program_id: world.env.program_id,
        accounts: vec![],
        data: vec![],
    };
    let env = &mut world.env;
    env.svm.expire_blockhash();
    let signers: Vec<_> = std::iter::once(&env.payer)
        .chain(actors.iter().map(|actor| &world.owners[*actor]))
        .collect();
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), prefix, suffix],
        Some(&env.payer.pubkey()),
        &signers,
        env.svm.latest_blockhash(),
    );
    let mut keys = tx.message.account_keys.clone();
    keys.extend([env.market, env.mint, env.vault]);
    keys.extend(world.portfolios);
    keys.extend(world.tokens);
    keys.sort_unstable();
    keys.dedup();
    let mut before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let payer_index = keys
        .iter()
        .position(|key| *key == env.payer.pubkey())
        .unwrap();
    before[payer_index].as_mut().unwrap().lamports -=
        u64::from(tx.message.header.num_required_signatures)
            * FeeStructure::default().lamports_per_signature;
    let failure = env
        .svm
        .send_transaction(tx)
        .expect_err("empty instruction suffix");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(3, InstructionError::InvalidInstructionData)
    );
    assert_eq!(
        failure
            .meta
            .logs
            .iter()
            .filter(|line| **line == format!("Program {} success", env.program_id))
            .count(),
        1
    );
    if actors.len() == 1 {
        assert!(failure
            .meta
            .logs
            .iter()
            .any(|line| *line == format!("Program {} success", spl_token::ID)));
    }
    assert_eq!(
        keys.iter()
            .map(|key| env.svm.get_account(key))
            .collect::<Vec<_>>(),
        before,
        "complete Account rollback, including carry, settlement residue and SPL prefixes"
    );
    failure.meta.compute_units_consumed
}

fn crank(world: &mut World, ledger: &mut Ledger, history: History, actor: usize, slot: u64) {
    world
        .trace
        .push(format!("slot={slot}: crank actor={actor}"));
    let ix = crank_ix(world, actor, &if history.reverse { [1, 0] } else { [0, 1] });
    let keys: Vec<_> = [world.env.market, world.env.mint, world.env.vault]
        .into_iter()
        .chain(world.portfolios)
        .chain(world.tokens)
        .collect();
    let before: Vec<_> = keys
        .iter()
        .map(|key| world.env.svm.get_account(key))
        .collect();
    world.env.svm.expire_blockhash();
    match send_raw_tx(&mut world.env.svm, &world.env.payer, ix, &[]) {
        Ok(cu) => {
            world.max_cu = world.max_cu.max(cu);
            assert_ne!(
                keys.iter()
                    .map(|key| world.env.svm.get_account(key))
                    .collect::<Vec<_>>(),
                before
            );
        }
        Err(error) => {
            assert!(is_engine_non_progress_error(&error), "{error}");
            assert_eq!(
                keys.iter()
                    .map(|key| world.env.svm.get_account(key))
                    .collect::<Vec<_>>(),
                before
            );
        }
    }
    ledger.accrue(history.direction, slot);
    ledger.check(world, history.direction);
}

fn reduce(
    world: &mut World,
    ledger: &mut Ledger,
    history: History,
    actor: usize,
    amounts: [i128; 2],
    rollback: bool,
    rejection_cu: &mut u64,
) {
    let order = if history.reverse { [1, 0] } else { [0, 1] };
    let legs: Vec<_> = order
        .into_iter()
        .map(|asset| BatchTradeLeg {
            asset_index: asset as u16,
            market_id: world.env.asset_market_id(asset as u16),
            size_q: -amounts[asset],
            exec_price: ledger.price[asset],
            fee_bps: 0,
        })
        .collect();
    let chunks = if history.batch {
        vec![legs]
    } else {
        legs.into_iter().map(|leg| vec![leg]).collect()
    };
    for (index, legs) in chunks.into_iter().enumerate() {
        world.trace.push(format!(
            "slot={}; frontier={}; reduce pair={actor}, legs={legs:?}",
            world.env.svm.get_sysvar::<Clock>().slot,
            ledger.slot
        ));
        let env = &world.env;
        let op = if history.batch {
            env.batch_trade_no_cpi_ix(
                world.portfolios[actor],
                world.portfolios[actor + 1],
                legs.clone(),
            )
        } else {
            let leg = &legs[0];
            env.trade_no_cpi_ix(
                world.portfolios[actor],
                world.portfolios[actor + 1],
                leg.asset_index,
                leg.size_q,
                leg.exec_price,
                0,
            )
        };
        let ix = Instruction {
            program_id: env.program_id,
            data: op.encode(),
            accounts: vec![
                AccountMeta::new(world.owners[actor].pubkey(), true),
                AccountMeta::new(world.owners[actor + 1].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(world.portfolios[actor], false),
                AccountMeta::new(world.portfolios[actor + 1], false),
            ],
        };
        if rollback && index == 0 {
            *rejection_cu =
                (*rejection_cu).max(reject_suffix(world, ix.clone(), &[actor, actor + 1]));
            ledger.check(world, history.direction);
        }
        let market = world.env.svm.get_account(&world.env.market).unwrap();
        let profiles =
            [0, 1].map(|asset| state::read_asset_oracle_profile(&market.data, asset).unwrap());
        let absent: Vec<_> = (0..4)
            .filter(|who| *who != actor && *who != actor + 1)
            .collect();
        let before: Vec<_> = absent
            .iter()
            .map(|who| world.env.svm.get_account(&world.portfolios[*who]))
            .collect();
        world.env.svm.expire_blockhash();
        let cu = send_raw_tx(
            &mut world.env.svm,
            &world.env.payer,
            ix,
            &[&world.owners[actor], &world.owners[actor + 1]],
        )
        .unwrap();
        world.max_cu = world.max_cu.max(cu);
        for leg in legs {
            ledger.q[actor][leg.asset_index as usize] += leg.size_q;
            ledger.q[actor + 1][leg.asset_index as usize] -= leg.size_q;
        }
        ledger.check(world, history.direction);
        let market = world.env.svm.get_account(&world.env.market).unwrap();
        assert_eq!(
            [0, 1].map(|asset| state::read_asset_oracle_profile(&market.data, asset).unwrap()),
            profiles
        );
        assert_eq!(
            absent
                .iter()
                .map(|who| world.env.svm.get_account(&world.portfolios[*who]))
                .collect::<Vec<_>>(),
            before
        );
    }
}

#[test]
fn v16_program_fractional_positions_partition_carry_into_exact_owner_payouts_and_residue() {
    let mut worlds = 0;
    let mut peak_cu = 0;
    let mut rejection_cu = 0;
    let mut spl_rollbacks = 0;
    for direction in [-1, 1] {
        for trade_first in [false, true] {
            let mut baseline = None;
            for batch in [false, true] {
                for split in [false, true] {
                    for reverse in [false, true] {
                        let history = History {
                            direction,
                            batch,
                            split,
                            placement: 0,
                            reverse,
                        };
                        let mut world = World::new(history);
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
                        ledger.check(&world, direction);
                        world.env.svm.warp_to_slot(1);
                        crank(&mut world, &mut ledger, history, 2, 1);
                        reduce(
                            &mut world,
                            &mut ledger,
                            history,
                            0,
                            [(POS_SCALE / 4) as i128, (POS_SCALE / 2) as i128],
                            false,
                            &mut rejection_cu,
                        );
                        for slot in 2..=3 {
                            world.env.svm.warp_to_slot(slot);
                            crank(&mut world, &mut ledger, history, 2, slot);
                        }
                        world.env.svm.warp_to_slot(4);
                        if !trade_first {
                            crank(&mut world, &mut ledger, history, 2, 4);
                        }
                        for part in 0..if split { 2 } else { 1 } {
                            reduce(
                                &mut world,
                                &mut ledger,
                                history,
                                0,
                                [POS_SCALE as i128 * if split { 1 } else { 2 }; 2],
                                part == 0,
                                &mut rejection_cu,
                            );
                        }
                        if trade_first {
                            crank(&mut world, &mut ledger, history, 2, 4);
                        }
                        for actor in if reverse { [3, 1, 0] } else { [0, 1, 3] } {
                            crank(&mut world, &mut ledger, history, actor, 4);
                        }
                        world.env.svm.warp_to_slot(5);
                        crank(&mut world, &mut ledger, history, 2, 5);
                        for actor in if reverse { [3, 1, 0] } else { [0, 1, 3] } {
                            crank(&mut world, &mut ledger, history, actor, 5);
                        }
                        assert!(ledger.latent_checks > 0);
                        assert_eq!(ledger.residue_num, 2 * POS_SCALE as i128);
                        let active_pnl = if trade_first { [-5, 3] } else { [-7, 5] };
                        let active_pnl = if direction > 0 {
                            active_pnl
                        } else {
                            [active_pnl[1], active_pnl[0]]
                        };
                        assert_eq!(
                            ledger.value,
                            [
                                PRINCIPAL[0] as i128 + active_pnl[0],
                                PRINCIPAL[1] as i128 + active_pnl[1],
                                PRINCIPAL[2] as i128 - 4 * direction,
                                PRINCIPAL[3] as i128 + 4 * direction
                            ]
                        );
                        for actor in if reverse { [2, 0] } else { [0, 2] } {
                            let quantities = ledger.q[actor];
                            reduce(
                                &mut world,
                                &mut ledger,
                                history,
                                actor,
                                quantities,
                                false,
                                &mut rejection_cu,
                            );
                        }
                        let endpoint = Economics {
                            price: ledger.price,
                            carry: [2_000, 5_000],
                            lots: [[0; 2]; 4],
                            entitlement: ledger.value,
                            vault: PRINCIPAL.map(u128::from).iter().sum(),
                        };
                        let paid = carry_transport_exit::pay_resolved_with_residue(
                            &mut world,
                            &endpoint,
                            reverse,
                            106,
                            2,
                            |world, actor, ix| {
                                let tx = Transaction::new_signed_with_payer(
                                    &[heap_ix(), cu_ix(), ix.clone()],
                                    Some(&world.env.payer.pubkey()),
                                    &[&world.env.payer, &world.owners[actor]],
                                    world.env.svm.latest_blockhash(),
                                );
                                if let Ok(simulation) =
                                    world.env.svm.simulate_transaction(tx.into())
                                {
                                    if simulation.logs.iter().any(|line| {
                                        *line == format!("Program {} success", spl_token::ID)
                                    }) {
                                        rejection_cu = rejection_cu.max(reject_suffix(
                                            world,
                                            ix.clone(),
                                            &[actor],
                                        ));
                                        spl_rollbacks += 1;
                                    }
                                }
                            },
                        )
                        .map(i128::from);
                        assert_eq!(paid, ledger.value);
                        let g = world.env.market_state().1;
                        assert_eq!((g.vault, g.c_tot, g.pnl_pos_tot, g.insurance), (2, 0, 0, 0));
                        assert_eq!(g.backing_provider_earnings_total, 0);
                        for bucket in &g.source_backing_buckets {
                            assert!(
                                bucket.status != percolator::BackingBucketStatusV16::Fresh
                                    || bucket.fresh_unliened_backing_num == 0
                            );
                            assert_eq!(bucket.valid_liened_backing_num, 0);
                            assert_eq!(bucket.impaired_liened_backing_num, 0);
                        }
                        if let Some(expected) = baseline {
                            assert_eq!(paid, expected);
                        } else {
                            baseline = Some(paid);
                        }
                        peak_cu = peak_cu.max(world.max_cu);
                        worlds += 1;
                    }
                }
            }
        }
    }
    assert_eq!(worlds, 32);
    assert!(spl_rollbacks >= 128);
    assert_cu_within("fractional carry owner exits", peak_cu, 1_400_000);
    assert_cu_within("fractional carry prefix rollback", rejection_cu, 1_400_000);
    eprintln!("INV-045 fractional positions: {worlds} worlds, 32 trade rollbacks, {spl_rollbacks} SPL rollbacks, 128 owner payouts, residue=2/world, successful CU={peak_cu}, rejection CU={rejection_cu}");
}
