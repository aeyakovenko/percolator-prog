//! INV-048/051/058/080/081: whole-route OI, fees and custody form one commit.
//! Unlike the one-asset fresh-pair handoff and INV-047's flat two-leg opening,
//! this starts with two capped books, clears one stored leg and resizes another,
//! then transfers their unequal headroom in real two-leg batches or singles.
//! A late cap rejection and a failed SPL tail must restore both books, all four
//! fee domains, position episodes and matcher response state. Accepted routes
//! must agree on input-derived economics and each owner's final SPL entitlement.
//! Fixed marks/unit ADL and no elapsed liabilities deliberately exclude row413
//! first-risk admission and row425 fractional settlement carry.

use super::*;

#[path = "inv_058_generated_side_oi_composition.rs"]
mod generated_side_oi_composition;

#[path = "inv_058_existing_leg_fee_competition.rs"]
mod existing_leg_fee_competition;

const ASSETS: usize = 2;
const HANDOFF_FEE_BPS: u64 = 100;
type Legs = Vec<(u16, i128)>;

struct World {
    env: V16CuEnv,
    owners: [Keypair; ACTORS],
    portfolios: [Pubkey; ACTORS],
    tokens: [Pubkey; ACTORS],
    matchers: [(Pubkey, Pubkey, Pubkey); 3],
    positions: [[i128; ASSETS]; ACTORS],
    fees: [u128; ACTORS],
    domains: [u128; 2 * ASSETS],
    epochs: [u64; ACTORS],
    paid: [u128; ACTORS],
    peak: [u64; 3],
}

impl World {
    fn new() -> Self {
        let mut env = inv018_public_spl_market_with_params(
            6,
            V16CuMarketParams {
                max_portfolio_assets: ASSETS as u16,
                initial_price: PRICE,
                ..V16CuMarketParams::default()
            },
        );
        for asset in 0..ASSETS {
            env.configure_auth_mark_for_asset_as_admin(asset as u16, 0, PRICE);
        }
        let owners: [Keypair; ACTORS] = std::array::from_fn(|_| Keypair::new());
        let mut portfolios = [Pubkey::default(); ACTORS];
        let mut tokens = [Pubkey::default(); ACTORS];
        for actor in 0..ACTORS {
            let owner = &owners[actor];
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                system_instruction::transfer(&env.payer.pubkey(), &owner.pubkey(), 1_000_000_000),
                &[],
            )
            .unwrap();
            let key = Keypair::new();
            portfolios[actor] = key.pubkey();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &key,
                env.portfolio_account_len,
                env.program_id,
            );
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(key.pubkey(), false),
                ],
                &[owner],
            )
            .unwrap();
            env.portfolios.push(key.pubkey());
            tokens[actor] = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &tokens[actor],
                    &env.admin.pubkey(),
                    &[],
                    CAPITAL as u64,
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            env.send(
                env.deposit_ix(key.pubkey(), CAPITAL),
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(key.pubkey(), false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[owner],
            )
            .unwrap();
        }
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::set_authority(
                &spl_token::ID,
                &env.mint,
                None,
                spl_token::instruction::AuthorityType::MintTokens,
                &env.admin.pubkey(),
                &[],
            )
            .unwrap(),
            &[&env.admin],
        )
        .unwrap();
        let matchers = std::array::from_fn(|pair| {
            auth_matcher_for_lp_via_system_create(
                &mut env,
                &owners[2 * pair + 1],
                portfolios[2 * pair + 1],
            )
        });
        let epochs = portfolios.map(|key| env.portfolio_position_epoch(key));
        Self {
            env,
            owners,
            portfolios,
            tokens,
            matchers,
            epochs,
            positions: [[0; ASSETS]; ACTORS],
            fees: [0; ACTORS],
            domains: [0; 2 * ASSETS],
            paid: [0; ACTORS],
            peak: [0; 3],
        }
    }

    fn keys(&self) -> Vec<Pubkey> {
        let mut keys = vec![
            self.env.market,
            self.env.mint,
            self.env.vault,
            self.env.vault_authority,
            self.env.admin.pubkey(),
        ];
        keys.extend(self.portfolios);
        keys.extend(self.tokens);
        keys.extend(self.owners.iter().map(Signer::pubkey));
        for (program, context, delegate) in self.matchers {
            keys.extend([program, context, delegate]);
        }
        keys
    }

    fn instructions(
        &self,
        pair: usize,
        route: TradeRoute,
        legs: &[(u16, i128)],
        bps: u64,
    ) -> Vec<Instruction> {
        let a = self.portfolios[pair];
        let b = self.portfolios[pair + 1];
        let cpi = matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi);
        let batch = matches!(route, TradeRoute::BatchNoCpi | TradeRoute::BatchCpi);
        let mut accounts = vec![AccountMeta::new(self.owners[pair].pubkey(), true)];
        if !cpi {
            accounts.push(AccountMeta::new(self.owners[pair + 1].pubkey(), true));
        }
        accounts.extend([
            AccountMeta::new(self.env.market, false),
            AccountMeta::new(a, false),
            AccountMeta::new(b, false),
        ]);
        if cpi {
            let (program, context, delegate) = self.matchers[pair / 2];
            accounts.extend([
                AccountMeta::new_readonly(program, false),
                AccountMeta::new(context, false),
                AccountMeta::new_readonly(delegate, false),
            ]);
        }
        legs.chunks(if batch { legs.len() } else { 1 })
            .enumerate()
            .map(|(episode, chunk)| {
                let mut ix = match route {
                    TradeRoute::NoCpi => self
                        .env
                        .trade_no_cpi_ix(a, b, chunk[0].0, chunk[0].1, PRICE, bps),
                    TradeRoute::Cpi => self
                        .env
                        .trade_cpi_ix(a, b, chunk[0].0, chunk[0].1, bps, PRICE),
                    TradeRoute::BatchNoCpi => self.env.batch_trade_no_cpi_ix(
                        a,
                        b,
                        chunk
                            .iter()
                            .map(|&(asset_index, size_q)| BatchTradeLeg {
                                asset_index,
                                market_id: self.env.asset_market_id(asset_index),
                                size_q,
                                exec_price: PRICE,
                                fee_bps: bps,
                            })
                            .collect(),
                    ),
                    TradeRoute::BatchCpi => self.env.batch_trade_cpi_ix_with_caps(
                        a,
                        b,
                        chunk
                            .iter()
                            .map(|&(asset_index, size_q)| BatchTradeCpiLeg {
                                asset_index,
                                market_id: self.env.asset_market_id(asset_index),
                                size_q,
                                limit_price: PRICE,
                                fee_bps: bps,
                            })
                            .collect(),
                        0,
                        chunk
                            .iter()
                            .map(|&(_, q)| ceil_ratio(notional(q) * u128::from(bps), 10_000))
                            .sum(),
                    ),
                };
                // Singles in one transaction sign the subsequent position episode in advance.
                match &mut ix {
                    ProgInstruction::TradeNoCpi {
                        account_a_position_epoch,
                        account_b_position_epoch,
                        ..
                    }
                    | ProgInstruction::TradeCpi {
                        account_a_position_epoch,
                        account_b_position_epoch,
                        ..
                    } => {
                        *account_a_position_epoch += episode as u64;
                        *account_b_position_epoch += episode as u64;
                    }
                    _ => assert_eq!(episode, 0),
                }
                Instruction {
                    program_id: self.env.program_id,
                    accounts: accounts.clone(),
                    data: ix.encode(),
                }
            })
            .collect()
    }

    fn transaction(&mut self, instructions: &[Instruction]) -> Transaction {
        self.env.svm.expire_blockhash();
        let mut ixs = vec![heap_ix(), cu_ix()];
        ixs.extend_from_slice(instructions);
        let mut signers = vec![&self.env.payer];
        signers.extend(self.owners.iter().filter(|owner| {
            ixs.iter().any(|ix| {
                ix.accounts
                    .iter()
                    .any(|m| m.is_signer && m.pubkey == owner.pubkey())
            })
        }));
        Transaction::new_signed_with_payer(
            &ixs,
            Some(&self.env.payer.pubkey()),
            &signers,
            self.env.svm.latest_blockhash(),
        )
    }

    fn record(&mut self, pair: usize, legs: &[(u16, i128)], bps: u64, episodes: usize) {
        for &(asset, q) in legs {
            let fee = ceil_ratio(notional(q) * u128::from(bps), 10_000);
            for (actor, delta) in [(pair, q), (pair + 1, -q)] {
                self.positions[actor][asset as usize] += delta;
                self.fees[actor] += fee;
            }
            for side in 0..2 {
                self.domains[2 * asset as usize + side] += fee;
            }
        }
        for actor in [pair, pair + 1] {
            self.epochs[actor] += episodes as u64;
        }
    }

    fn check(&self) {
        let (_, group) = self.env.market_state();
        assert_eq!(group.mode, MarketModeV16::Live);
        let mut oi = [[0u128; 2]; ASSETS];
        let mut counts = [[0u64; 2]; ASSETS];
        for actor in 0..ACTORS {
            let account = self.env.portfolio_state(self.portfolios[actor]);
            assert_eq!(
                account.capital.get(),
                CAPITAL - self.fees[actor] - self.paid[actor]
            );
            assert_eq!(account.pnl.get(), 0);
            assert_eq!(
                self.env.token_amount(self.tokens[actor]) as u128,
                self.paid[actor]
            );
            assert_eq!(
                self.env.portfolio_position_epoch(self.portfolios[actor]),
                self.epochs[actor]
            );
            let mut observed = [0; ASSETS];
            for encoded in &account.legs {
                let leg = encoded.try_to_runtime().unwrap();
                if !leg.active {
                    continue;
                }
                let asset = leg.asset_index as usize;
                assert!(asset < ASSETS);
                assert_eq!(observed[asset], 0, "one canonical leg per asset");
                let side = usize::from(leg.basis_pos_q < 0);
                let slot = group.assets[asset];
                assert_eq!(leg.a_basis, ADL_ONE);
                assert_eq!(leg.market_id, slot.market_id);
                assert_eq!(
                    leg.side,
                    if side == 0 {
                        SideV16::Long
                    } else {
                        SideV16::Short
                    }
                );
                assert_eq!(
                    leg.epoch_snap,
                    if side == 0 {
                        slot.epoch_long
                    } else {
                        slot.epoch_short
                    }
                );
                observed[asset] = leg.basis_pos_q;
                oi[asset][side] += leg.basis_pos_q.unsigned_abs();
                counts[asset][side] += 1;
            }
            assert_eq!(observed, self.positions[actor]);
            assert_eq!(
                percolator::active_bitmap_count_ones(active_bitmap(&account)),
                observed.iter().filter(|&&q| q != 0).count() as u32
            );
            assert!(observed
                .iter()
                .all(|q| q.unsigned_abs() < percolator::MAX_POSITION_ABS_Q));
            let risk: u128 = observed.into_iter().map(notional).sum();
            assert!(risk <= percolator::MAX_ACCOUNT_NOTIONAL);
            let cert = health_cert(&account);
            if risk != 0 {
                assert!(cert.valid);
            }
            if cert.valid {
                assert_eq!(cert.certified_worst_case_loss, risk);
            }
        }
        for asset in 0..ASSETS {
            let slot = group.assets[asset];
            assert_eq!(slot.lifecycle, AssetLifecycleV16::Active);
            assert_eq!([slot.mode_long, slot.mode_short], [SideModeV16::Normal; 2]);
            assert_eq!([slot.a_long, slot.a_short], [ADL_ONE; 2]);
            assert_eq!(
                [slot.effective_price, slot.raw_oracle_target_price],
                [PRICE; 2]
            );
            assert_eq!([slot.oi_eff_long_q, slot.oi_eff_short_q], oi[asset]);
            assert_eq!(oi[asset][0], oi[asset][1]);
            assert!(oi[asset][0] <= percolator::MAX_OI_SIDE_Q);
            assert_eq!(
                [slot.stored_pos_count_long, slot.stored_pos_count_short],
                counts[asset]
            );
        }
        let fees: u128 = self.fees.iter().sum();
        let paid: u128 = self.paid.iter().sum();
        assert_eq!(group.insurance, fees);
        assert_eq!(&group.insurance_domain_budget[..2 * ASSETS], &self.domains);
        assert!(group.insurance_domain_budget[2 * ASSETS..]
            .iter()
            .all(|&n| n == 0));
        assert_eq!(group.c_tot, SUPPLY - fees - paid);
        assert_eq!(group.vault, group.c_tot + fees);
        assert_eq!(self.env.token_amount(self.env.vault) as u128, group.vault);
        let mint = Mint::unpack(&self.env.svm.get_account(&self.env.mint).unwrap().data).unwrap();
        assert_eq!(
            (mint.supply as u128, mint.mint_authority),
            (SUPPLY, COption::None)
        );
        assert_eq!(group.vault + paid, SUPPLY);
    }

    fn accept(&mut self, instructions: &[Instruction], pairs: &[usize]) {
        let tx = self.transaction(instructions);
        let before = frame(&self.env, &tx, &self.keys());
        let fee = FeeStructure::default().lamports_per_signature
            * u64::from(tx.message.header.num_required_signatures);
        let meta = self
            .env
            .svm
            .send_transaction(tx)
            .unwrap_or_else(|e| panic!("accepted handoff: {e:?}"));
        let mut changed = vec![self.env.market];
        for &pair in pairs {
            changed.extend([self.portfolios[pair], self.portfolios[pair + 1]]);
            let (program, context, _) = self.matchers[pair / 2];
            if meta
                .logs
                .iter()
                .any(|line| *line == format!("Program {program} success"))
            {
                changed.push(context);
            }
        }
        check_frame(&self.env, before, fee, &changed);
        assert_cu_within(
            "two-asset handoff",
            meta.compute_units_consumed,
            instructions.len() as u64 * TRADE_CU_LIMIT,
        );
        self.peak[1] = self.peak[1].max(meta.compute_units_consumed);
    }

    fn reject(
        &mut self,
        instructions: &[Instruction],
        code: u32,
        wrapper_successes: usize,
        matcher_successes: usize,
    ) {
        let tx = self.transaction(instructions);
        let before = frame(&self.env, &tx, &self.keys());
        let fee = FeeStructure::default().lamports_per_signature
            * u64::from(tx.message.header.num_required_signatures);
        let err = self
            .env
            .svm
            .send_transaction(tx)
            .expect_err("late tail must roll back the complete handoff");
        assert_eq!(
            err.err,
            TransactionError::InstructionError(
                instructions.len() as u8 + 1,
                InstructionError::Custom(code)
            ),
            "{:?}",
            err.meta.logs
        );
        assert_eq!(
            err.meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", self.env.program_id))
                .count(),
            wrapper_successes
        );
        assert_eq!(
            err.meta
                .logs
                .iter()
                .filter(|line| self
                    .matchers
                    .iter()
                    .any(|(program, _, _)| **line == format!("Program {program} success")))
                .count(),
            matcher_successes
        );
        check_frame(&self.env, before, fee, &[]);
        self.check();
        assert_cu_within(
            "two-asset handoff rejection",
            err.meta.compute_units_consumed,
            instructions.len() as u64 * TRADE_CU_LIMIT,
        );
        self.peak[0] = self.peak[0].max(err.meta.compute_units_consumed);
    }
}

#[test]
fn v16_program_two_asset_oi_fee_handoff_is_atomic_across_clear_resize_and_route_switch() {
    let half = i128::try_from(percolator::MAX_OI_SIDE_Q / 2).unwrap();
    let small = 7 * POS_SCALE as i128;
    assert_eq!(half as u128 * 2, percolator::MAX_OI_SIDE_Q);
    assert!(small < half && (half + 1).unsigned_abs() < percolator::MAX_POSITION_ABS_Q);
    let routes = [
        (TradeRoute::BatchCpi, TradeRoute::NoCpi),
        (TradeRoute::Cpi, TradeRoute::BatchNoCpi),
        (TradeRoute::BatchNoCpi, TradeRoute::Cpi),
        (TradeRoute::NoCpi, TradeRoute::BatchCpi),
    ];
    let mut peaks = [0; 3];
    for direction in [-1i128, 1] {
        let mut reference = None;
        for order in [[0u16, 1], [1, 0]] {
            for (release_route, refill_route) in routes {
                let label = format!(
                    "direction={direction} order={order:?} {release_route:?}/{refill_route:?}"
                );
                let mut w = World::new();
                w.check();
                let opening: Legs = order
                    .map(|asset| (asset, direction * if asset == 0 { half } else { -half }))
                    .to_vec();
                for pair in [0, 2] {
                    let ixs = w.instructions(pair, TradeRoute::BatchNoCpi, &opening, 0);
                    w.accept(&ixs, &[pair]);
                    w.record(pair, &opening, 0, 1);
                    w.check();
                }
                // Publicly renew only the release LP's grant revoked by the signed opening.
                let (program, context, delegate) = w.matchers[0];
                w.env.set_matcher_config(
                    program,
                    &w.owners[1],
                    w.portfolios[1],
                    context,
                    delegate,
                    1,
                );
                w.env.update_trade_fee_policy_with_cu(HANDOFF_FEE_BPS);
                w.check();
                let refill: Legs = order
                    .map(|asset| (asset, direction * if asset == 0 { half } else { -small }))
                    .to_vec();
                let release: Legs = refill.iter().map(|&(a, q)| (a, -q)).collect();
                let reduce_ixs = w.instructions(0, release_route, &release, HANDOFF_FEE_BPS);
                let refill_ixs = w.instructions(4, refill_route, &refill, HANDOFF_FEE_BPS);
                let mut good = reduce_ixs.clone();
                good.extend(refill_ixs.clone());
                let mut over = refill.clone();
                over.last_mut().unwrap().1 += over.last().unwrap().1.signum();
                assert!(over.iter().all(|&(_, q)| {
                    q.unsigned_abs() < percolator::MAX_POSITION_ABS_Q
                        && q.unsigned_abs() <= percolator::MAX_TRADE_SIZE_Q
                }));
                assert!(
                    over.iter().map(|&(_, q)| notional(q)).sum::<u128>()
                        < percolator::MAX_ACCOUNT_NOTIONAL
                );
                let mut bad = reduce_ixs.clone();
                bad.extend(w.instructions(4, refill_route, &over, HANDOFF_FEE_BPS));
                let release_cpis =
                    if matches!(release_route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
                        reduce_ixs.len()
                    } else {
                        0
                    };
                let refill_cpis = if matches!(refill_route, TradeRoute::Cpi | TradeRoute::BatchCpi)
                {
                    refill_ixs.len()
                } else {
                    0
                };
                w.reject(
                    &bad,
                    PercolatorError::EngineInvalidLeg as u32,
                    bad.len() - 1,
                    release_cpis + refill_cpis,
                );

                // Every economic instruction completes before the empty source fails in SPL.
                let mut failed_spl = good.clone();
                failed_spl.push(
                    spl_token::instruction::transfer(
                        &spl_token::ID,
                        &w.tokens[4],
                        &w.tokens[5],
                        &w.owners[4].pubkey(),
                        &[],
                        1,
                    )
                    .unwrap(),
                );
                w.reject(
                    &failed_spl,
                    spl_token::error::TokenError::InsufficientFunds as u32,
                    good.len(),
                    release_cpis + refill_cpis,
                );

                // Retry the exact already-built instruction bytes, including signed epochs/caps.
                w.accept(&good, &[0, 4]);
                w.record(0, &release, HANDOFF_FEE_BPS, reduce_ixs.len());
                w.record(4, &refill, HANDOFF_FEE_BPS, refill_ixs.len());
                w.check();
                assert_eq!(w.positions[0], [0, -direction * (half - small)]);
                assert_eq!(
                    w.fees[2..4],
                    [0; 2],
                    "untouched pair never pays handoff fees"
                );
                assert_eq!(
                    w.domains,
                    [
                        2 * ceil_ratio(notional(half), 100),
                        2 * ceil_ratio(notional(half), 100),
                        14,
                        14
                    ]
                );
                let group = w.env.market_state().1;
                assert!(group.assets[..ASSETS]
                    .iter()
                    .all(|a| a.oi_eff_long_q == percolator::MAX_OI_SIDE_Q));
                let observed = (
                    w.portfolios.map(|key| {
                        let p = w.env.portfolio_state(key);
                        (
                            p.capital.get(),
                            p.pnl.get(),
                            health_cert(&p).certified_worst_case_loss,
                        )
                    }),
                    w.positions,
                    group.c_tot,
                    group.vault,
                    group.insurance,
                    w.domains,
                );
                if let Some(expected) = &reference {
                    assert_eq!(&observed, expected, "{label}: route/order equivalence");
                } else {
                    reference = Some(observed);
                }

                w.env.update_trade_fee_policy_with_cu(0);
                for pair in [4, 0, 2] {
                    let close: Legs = order
                        .iter()
                        .filter_map(|&a| {
                            let q = w.positions[pair][a as usize];
                            (q != 0).then_some((a, -q))
                        })
                        .collect();
                    let ixs = w.instructions(pair, TradeRoute::BatchNoCpi, &close, 0);
                    w.accept(&ixs, &[pair]);
                    w.record(pair, &close, 0, 1);
                    w.check();
                }
                for actor in 0..ACTORS {
                    let amount = CAPITAL - w.fees[actor];
                    let cu = w
                        .env
                        .send(
                            w.env.withdraw_ix(w.portfolios[actor], amount),
                            vec![
                                AccountMeta::new(w.owners[actor].pubkey(), true),
                                AccountMeta::new(w.env.market, false),
                                AccountMeta::new(w.portfolios[actor], false),
                                AccountMeta::new(w.tokens[actor], false),
                                AccountMeta::new(w.env.vault, false),
                                AccountMeta::new_readonly(w.env.vault_authority, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                            &[&w.owners[actor]],
                        )
                        .unwrap();
                    w.paid[actor] = amount;
                    w.check();
                    assert_cu_within("two-asset handoff payout", cu, CUSTODY_CU_LIMIT);
                    w.peak[2] = w.peak[2].max(cu);
                }
                assert_eq!(w.env.market_state().1.c_tot, 0);
                for i in 0..3 {
                    peaks[i] = peaks[i].max(w.peak[i]);
                }
                println!(
                    "INV-048/051/058/080/081 {label}: two exact rollbacks, six exact SPL payouts"
                );
            }
        }
    }
    println!("two-asset OI/fee handoff: 16 worlds, 32 rollbacks, 96 payouts; peak CU [reject, trade, custody]={peaks:?}");
}
