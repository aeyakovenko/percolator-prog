//! INV-045 / row425: generated fractional K/F settlement across public routes.
//! Independent signed-numerator accounting distinguishes price carry, K and F
//! settlement floors, latent owner value, and unallocated custody residue.
//! Fixed AuthMark targets, unit ADL, zero fees and solvent reductions only.

use super::*;
use rand::{Rng, SeedableRng};
use rand_xorshift::XorShiftRng;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const SCALE: i128 = POS_SCALE as i128;

struct Ledger {
    q: [[i128; 2]; 4],
    value: [i128; 4],
    ideal_num: [i128; 4],
    // Each owner's two lanes are K and F; these are not transferable claims.
    residue_num: [[i128; 2]; 4],
    price: [u64; 2],
    funding: [i128; 2],
    snap_price: [[u64; 2]; 4],
    snap_funding: [[i128; 2]; 4],
    funding_flows: [[u128; 4]; 4],
    slot: u64,
    direction: i128,
    rate_cap: i128,
    latent_checks: usize,
    separate_floor_checks: usize,
}

impl Ledger {
    fn new(direction: i128, rate_cap: i128) -> Self {
        Self {
            q: OPEN_LOTS.map(|q| q.map(|lots| lots * SCALE)),
            value: PRINCIPAL.map(i128::from),
            ideal_num: PRINCIPAL.map(|p| i128::from(p) * SCALE),
            residue_num: [[0; 2]; 4],
            price: ANCHORS,
            funding: [0; 2],
            snap_price: [ANCHORS; 4],
            snap_funding: [[0; 2]; 4],
            funding_flows: [[0; 4]; 4],
            slot: 0,
            direction,
            rate_cap,
            latent_checks: 0,
            separate_floor_checks: 0,
        }
    }

    fn target(&self, asset: usize) -> i128 {
        i128::from(ANCHORS[asset]) + self.direction * if asset == 0 { 20 } else { -20 }
    }

    fn carry(&self) -> [u64; 2] {
        ANCHORS.map(|anchor| anchor * CAP_BPS * self.slot % 10_000)
    }

    fn advance(&mut self, slot: u64) {
        for now in self.slot + 1..=slot {
            for asset in 0..2 {
                let sign = self.direction * if asset == 0 { 1 } else { -1 };
                let price = i128::from(ANCHORS[asset])
                    + sign * i128::from(ANCHORS[asset] * CAP_BPS * now / 10_000);
                assert!((price - i128::from(ANCHORS[asset])).abs() < 20);
                let rate = ((self.target(asset) - price) * 1_000_000_000 / price)
                    .clamp(-self.rate_cap, self.rate_cap);
                let funding = -(rate * price).div_euclid(1_000_000_000);
                for actor in 0..4 {
                    self.ideal_num[actor] +=
                        self.q[actor][asset] * (price - i128::from(self.price[asset]) + funding);
                }
                self.price[asset] = price as u64;
                self.funding[asset] += funding;
            }
        }
        self.slot = slot;
    }

    fn pending(&self, actor: usize, asset: usize) -> [i128; 2] {
        [
            self.q[actor][asset]
                * (i128::from(self.price[asset]) - i128::from(self.snap_price[actor][asset])),
            self.q[actor][asset] * (self.funding[asset] - self.snap_funding[actor][asset]),
        ]
    }

    fn settle(&mut self, actor: usize) {
        for asset in 0..2 {
            let pending = self.pending(actor, asset);
            let atoms = pending.map(|n| n.div_euclid(SCALE));
            self.separate_floor_checks += usize::from(
                atoms.iter().sum::<i128>() != pending.iter().sum::<i128>().div_euclid(SCALE),
            );
            for lane in 0..2 {
                self.value[actor] += atoms[lane];
                self.residue_num[actor][lane] += pending[lane].rem_euclid(SCALE);
            }
            let side = if self.q[actor][asset] >= 0 { 0 } else { 2 };
            self.funding_flows[actor][side + usize::from(atoms[1] >= 0)] += atoms[1].unsigned_abs();
            self.snap_price[actor][asset] = self.price[asset];
            self.snap_funding[actor][asset] = self.funding[asset];
        }
    }

    fn check(&mut self, world: &World) {
        let env = &world.env;
        let group = env.market_state().1;
        let profiles = carry_transport_exit::profiles(world);
        for asset in 0..2 {
            let a = group.assets[asset];
            assert_eq!(
                (a.slot_last, a.effective_price),
                (self.slot, self.price[asset])
            );
            assert_eq!(a.fund_px_last, ANCHORS[asset]);
            assert_eq!(a.raw_oracle_target_price as i128, self.target(asset));
            let profile = profiles[asset];
            assert_eq!(
                profile.price_move_remainder_bps_num as u64,
                self.carry()[asset]
            );
            assert_eq!(profile.mark_ewma_e6 as i128, self.target(asset));
            assert_eq!(profile.oracle_target_price_e6, profile.mark_ewma_e6);
            assert_eq!(profile.funding_mark_e6, profile.mark_ewma_e6);
            assert_eq!(
                (
                    profile.funding_mark_pending_e6,
                    profile.funding_mark_pending_slot
                ),
                (0, 0)
            );
            let k = (i128::from(self.price[asset]) - i128::from(ANCHORS[asset])) * ADL_ONE as i128;
            let f = self.funding[asset] * ADL_ONE as i128;
            assert_eq!(
                (a.k_long, a.k_short, a.f_long_num, a.f_short_num),
                (k, -k, f, -f)
            );
            assert_eq!(
                (a.a_long, a.a_short, a.b_long_num, a.b_short_num),
                (ADL_ONE, ADL_ONE, 0, 0)
            );
            assert_eq!(
                (
                    a.pending_obligation_count_long,
                    a.pending_obligation_count_short
                ),
                (0, 0)
            );
            let oi = self.q.iter().map(|q| q[asset].max(0) as u128).sum::<u128>();
            assert_eq!((a.oi_eff_long_q, a.oi_eff_short_q), (oi, oi));
        }
        let mut capital = 0;
        let mut positive_pnl = 0;
        for actor in 0..4 {
            let account = env.portfolio_state(world.portfolios[actor]);
            let mut latent_num = 0;
            for asset in 0..2 {
                let leg = account
                    .legs
                    .iter()
                    .map(|l| l.try_to_runtime().unwrap())
                    .find(|l| l.active && l.asset_index as usize == asset);
                if let Some(leg) = leg {
                    assert_eq!(leg.basis_pos_q, self.q[actor][asset]);
                    assert_eq!((leg.a_basis, leg.b_rem), (ADL_ONE, 0));
                    let sign = self.q[actor][asset].signum();
                    assert_eq!(
                        leg.k_snap,
                        sign * (i128::from(self.snap_price[actor][asset])
                            - i128::from(ANCHORS[asset]))
                            * ADL_ONE as i128
                    );
                    assert_eq!(
                        leg.f_snap,
                        sign * self.snap_funding[actor][asset] * ADL_ONE as i128
                    );
                } else {
                    assert_eq!(self.q[actor][asset], 0);
                }
                let pending = self.pending(actor, asset);
                self.latent_checks += usize::from(pending != [0; 2]);
                latent_num += pending.iter().sum::<i128>();
            }
            assert_eq!(
                account.capital.get() as i128
                    + account.pnl.get()
                    + i128::from(env.token_amount(world.tokens[actor])),
                self.value[actor],
                "owner={actor}: {:?}",
                world.trace
            );
            assert_eq!(
                self.value[actor] * SCALE
                    + latent_num
                    + self.residue_num[actor].iter().sum::<i128>(),
                self.ideal_num[actor]
            );
            assert_eq!(
                [
                    account.funding_long_paid_atoms_total.get(),
                    account.funding_long_received_atoms_total.get(),
                    account.funding_short_paid_atoms_total.get(),
                    account.funding_short_received_atoms_total.get(),
                ],
                self.funding_flows[actor]
            );
            capital += account.capital.get();
            positive_pnl += account.pnl.get().max(0) as u128;
        }
        let total = PRINCIPAL.map(u128::from).iter().sum::<u128>();
        assert_eq!(self.ideal_num.iter().sum::<i128>(), total as i128 * SCALE);
        assert_eq!((group.c_tot, group.pnl_pos_tot), (capital, positive_pnl));
        assert_eq!(
            (group.insurance, group.backing_provider_earnings_total),
            (0, 0)
        );
        assert!(group.insurance_domain_budget.iter().all(|x| *x == 0));
        assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
        assert_eq!(
            group.vault
                + world
                    .tokens
                    .map(|key| u128::from(env.token_amount(key)))
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

fn reject_suffix(world: &mut World, ix: Instruction, actors: &[usize]) -> u64 {
    let env = &mut world.env;
    env.svm.expire_blockhash();
    let signers: Vec<_> = std::iter::once(&env.payer)
        .chain(actors.iter().map(|actor| &world.owners[*actor]))
        .collect();
    let tx = Transaction::new_signed_with_payer(
        &[
            heap_ix(),
            cu_ix(),
            ix,
            Instruction {
                program_id: env.program_id,
                accounts: vec![],
                data: vec![],
            },
        ],
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
    let payer = keys
        .iter()
        .position(|key| *key == env.payer.pubkey())
        .unwrap();
    before[payer].as_mut().unwrap().lamports -=
        u64::from(tx.message.header.num_required_signatures)
            * FeeStructure::default().lamports_per_signature;
    let failure = env.svm.send_transaction(tx).expect_err("invalid suffix");
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
    assert_eq!(
        keys.iter()
            .map(|key| env.svm.get_account(key))
            .collect::<Vec<_>>(),
        before,
        "full Account rollback after successful economic prefix"
    );
    failure.meta.compute_units_consumed
}

fn crank(world: &mut World, ledger: &mut Ledger, actor: usize, reverse: bool) {
    world.trace.push(format!(
        "slot={}: crank owner={actor}, reverse={reverse}",
        ledger.slot
    ));
    let absent: Vec<_> = (0..4).filter(|i| *i != actor).collect();
    let before: Vec<_> = absent
        .iter()
        .map(|i| world.env.svm.get_account(&world.portfolios[*i]))
        .collect();
    world.env.svm.expire_blockhash();
    if let Some(cu) = world.env.crank_if_actionable(
        world.portfolios[actor],
        ProgInstruction::PermissionlessCrank {
            now_slot: ledger.slot,
            observations: crank_observations_for_assets(&if reverse { [1, 0] } else { [0, 1] }),
        },
    ) {
        world.max_cu = world.max_cu.max(cu);
    }
    ledger.settle(actor);
    ledger.check(world);
    assert_eq!(
        absent
            .iter()
            .map(|i| world.env.svm.get_account(&world.portfolios[*i]))
            .collect::<Vec<_>>(),
        before
    );
}

fn reduce(
    world: &mut World,
    ledger: &mut Ledger,
    actor: usize,
    amounts: [i128; 2],
    batch: bool,
    reverse: bool,
    matcher: Option<(Pubkey, Pubkey, Pubkey)>,
    rollback: bool,
) -> u64 {
    let profiles = carry_transport_exit::profiles(world);
    if let Some((program, context, delegate)) = matcher {
        world.env.set_matcher_config_with_trade_fee_cap(
            program,
            &world.owners[actor + 1],
            world.portfolios[actor + 1],
            context,
            delegate,
            1,
            0,
        );
        ledger.check(world);
        assert_eq!(carry_transport_exit::profiles(world), profiles);
    }
    let order = if reverse { [1, 0] } else { [0, 1] };
    let chunks = if batch {
        vec![order.to_vec()]
    } else {
        order.map(|a| vec![a]).to_vec()
    };
    let mut rejection_cu = 0;
    for (part, assets) in chunks.into_iter().enumerate() {
        world.trace.push(format!(
            "slot={}: pair={actor}, assets={assets:?}, amounts={amounts:?}, cpi={}",
            ledger.slot,
            matcher.is_some()
        ));
        let env = &world.env;
        let a = world.portfolios[actor];
        let b = world.portfolios[actor + 1];
        let asset = assets[0];
        let mut accounts = vec![AccountMeta::new(world.owners[actor].pubkey(), true)];
        let op = if let Some((program, context, delegate)) = matcher {
            accounts.extend([
                AccountMeta::new(env.market, false),
                AccountMeta::new(a, false),
                AccountMeta::new(b, false),
                AccountMeta::new_readonly(program, false),
                AccountMeta::new(context, false),
                AccountMeta::new_readonly(delegate, false),
            ]);
            if batch {
                env.batch_trade_cpi_ix(
                    a,
                    b,
                    assets
                        .iter()
                        .map(|asset| BatchTradeCpiLeg {
                            asset_index: *asset as u16,
                            market_id: env.asset_market_id(*asset as u16),
                            size_q: -amounts[*asset],
                            fee_bps: 0,
                            limit_price: ledger.price[*asset],
                        })
                        .collect(),
                )
            } else {
                env.trade_cpi_ix(a, b, asset as u16, -amounts[asset], 0, ledger.price[asset])
            }
        } else {
            accounts.extend([
                AccountMeta::new(world.owners[actor + 1].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(a, false),
                AccountMeta::new(b, false),
            ]);
            if batch {
                env.batch_trade_no_cpi_ix(
                    a,
                    b,
                    assets
                        .iter()
                        .map(|asset| BatchTradeLeg {
                            asset_index: *asset as u16,
                            market_id: env.asset_market_id(*asset as u16),
                            size_q: -amounts[*asset],
                            fee_bps: 0,
                            exec_price: ledger.price[*asset],
                        })
                        .collect(),
                )
            } else {
                env.trade_no_cpi_ix(a, b, asset as u16, -amounts[asset], ledger.price[asset], 0)
            }
        };
        let ix = Instruction {
            program_id: env.program_id,
            accounts,
            data: op.encode(),
        };
        let actors = if matcher.is_some() {
            vec![actor]
        } else {
            vec![actor, actor + 1]
        };
        if rollback && part == 0 {
            rejection_cu = reject_suffix(world, ix.clone(), &actors);
            ledger.check(world);
        }
        let absent: Vec<_> = (0..4).filter(|i| *i != actor && *i != actor + 1).collect();
        let before: Vec<_> = absent
            .iter()
            .map(|i| world.env.svm.get_account(&world.portfolios[*i]))
            .collect();
        world.env.svm.expire_blockhash();
        let signers: Vec<_> = actors.iter().map(|i| &world.owners[*i]).collect();
        let cu = send_raw_tx(&mut world.env.svm, &world.env.payer, ix, &signers).unwrap();
        world.max_cu = world.max_cu.max(cu);
        ledger.settle(actor);
        ledger.settle(actor + 1);
        for asset in assets {
            ledger.q[actor][asset] -= amounts[asset];
            ledger.q[actor + 1][asset] += amounts[asset];
        }
        ledger.check(world);
        assert_eq!(carry_transport_exit::profiles(world), profiles);
        assert_eq!(
            absent
                .iter()
                .map(|i| world.env.svm.get_account(&world.portfolios[*i]))
                .collect::<Vec<_>>(),
            before
        );
    }
    rejection_cu
}

#[test]
fn v16_program_generated_fractional_kf_routes_preserve_carry_owner_value_and_residue() {
    let mut worlds = 0;
    let mut peak_cu = 0;
    let mut rejection_cu = 0;
    let mut separate_floors = 0;
    let mut residues = std::collections::BTreeSet::new();
    for seed in [425, 425_010, 425_038, 425_052] {
        let mut rng = XorShiftRng::seed_from_u64(seed);
        let schedule: Vec<_> = (0..6)
            .map(|_| {
                (
                    rng.gen_range(2..=4u64),
                    [
                        rng.gen_range(1..=3i128) * SCALE / 8,
                        rng.gen_range(1..=3i128) * SCALE / 8,
                    ],
                    rng.gen_bool(0.5),
                )
            })
            .collect();
        for direction in [-1, 1] {
            let rate = 10_000;
            let mut reference = None;
            for partitioned in [false, true] {
                let history = History {
                    direction,
                    batch: !partitioned,
                    split: partitioned,
                    placement: 0,
                    reverse: partitioned,
                };
                let mut world = World::with_funding_and_accrual_limit(history, rate as u64, 4);
                world
                    .trace
                    .push(format!("seed={seed}, rate={rate}, schedule={schedule:?}"));
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
                let mut ledger = Ledger::new(direction, rate);
                ledger.check(&world);
                reduce(
                    &mut world,
                    &mut ledger,
                    0,
                    [SCALE / 4, SCALE / 2],
                    true,
                    false,
                    None,
                    false,
                );
                for (event, (dt, amounts, settle_first)) in schedule.iter().enumerate() {
                    let end = ledger.slot + dt;
                    let frontiers = if partitioned {
                        (ledger.slot + 1..=end).collect::<Vec<_>>()
                    } else {
                        vec![end]
                    };
                    for slot in frontiers {
                        world.env.svm.warp_to_slot(slot);
                        ledger.advance(slot);
                        crank(&mut world, &mut ledger, 2, partitioned);
                    }
                    if *settle_first {
                        for actor in if partitioned { [1, 0] } else { [0, 1] } {
                            crank(&mut world, &mut ledger, actor, partitioned);
                        }
                    }
                    let parts = if partitioned { 2 } else { 1 };
                    for part in 0..parts {
                        rejection_cu = rejection_cu.max(reduce(
                            &mut world,
                            &mut ledger,
                            0,
                            amounts.map(|q| q / parts),
                            !partitioned || event % 2 == 0,
                            partitioned,
                            (partitioned && event % 4 < 2).then_some(matcher),
                            part == 0,
                        ));
                        if partitioned && part == 0 {
                            crank(&mut world, &mut ledger, 1, true);
                            crank(&mut world, &mut ledger, 0, true);
                        }
                    }
                    ledger.check(&world);
                }
                assert!(ledger.latent_checks > 0);
                assert!(ledger
                    .price
                    .iter()
                    .zip(ANCHORS)
                    .all(|(p, a)| p.abs_diff(a) >= 3));
                assert!(ledger.carry().iter().any(|carry| *carry > 0));
                for actor in if partitioned { [2, 0] } else { [0, 2] } {
                    let quantities = ledger.q[actor];
                    reduce(
                        &mut world,
                        &mut ledger,
                        actor,
                        quantities,
                        true,
                        partitioned,
                        None,
                        false,
                    );
                }
                let residue_num = ledger.residue_num.iter().flatten().sum::<i128>();
                assert_eq!(residue_num % SCALE, 0);
                let residue = (residue_num / SCALE) as u128;
                assert!(residue > 2, "multiple K/F floors must contribute");
                assert!(ledger.residue_num.iter().any(|r| r[0] > 0 && r[1] > 0));
                separate_floors += ledger.separate_floor_checks;
                residues.insert(residue);
                let endpoint = Economics {
                    price: ledger.price,
                    carry: ledger.carry(),
                    lots: [[0; 2]; 4],
                    entitlement: ledger.value,
                    vault: PRINCIPAL.map(u128::from).iter().sum(),
                };
                let payout_slot = ledger.slot + 101;
                let paid = carry_transport_exit::pay_resolved_with_residue(
                    &mut world,
                    &endpoint,
                    partitioned,
                    payout_slot,
                    residue,
                    |_, _, _| {},
                );
                assert_eq!(paid.map(i128::from), ledger.value);
                let group = world.env.market_state().1;
                assert_eq!(
                    (group.insurance, group.backing_provider_earnings_total),
                    (0, 0)
                );
                for bucket in &group.source_backing_buckets {
                    assert!(
                        bucket.status != percolator::BackingBucketStatusV16::Fresh
                            || bucket.fresh_unliened_backing_num == 0
                    );
                    assert_eq!(
                        (
                            bucket.valid_liened_backing_num,
                            bucket.impaired_liened_backing_num,
                            bucket.utilization_fee_earnings
                        ),
                        (0, 0, 0)
                    );
                }
                let result = (endpoint, ledger.residue_num, ledger.funding_flows, paid);
                if let Some(expected) = &reference {
                    assert_eq!(&result, expected);
                } else {
                    reference = Some(result);
                }
                peak_cu = peak_cu.max(world.max_cu);
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 16);
    assert!(
        separate_floors > 0,
        "merging K/F before flooring must change the oracle"
    );
    assert!(
        residues.len() > 1,
        "generated histories must produce distinct residue totals"
    );
    assert_cu_within("generated fractional K/F routes", peak_cu, 1_400_000);
    assert_cu_within("generated fractional K/F rollback", rejection_cu, 1_400_000);
    eprintln!("row425: {worlds} worlds, 96 economic-prefix rollbacks, 64 exact owner payouts, residues={residues:?}, separate-floor witnesses={separate_floors}, success CU={peak_cu}, rejection CU={rejection_cu}");
}
