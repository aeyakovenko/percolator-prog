//! INV-014 / row 411: retained CPI -> bilateral -> renewed CPI fee consent.
//! One shared pair changes transport and grant sequence across policy writes.
//! Public System/SPL/ATA/wrapper setup, complete rollback, and input-only fees.

use super::*;

const CPI_CAP: u64 = 37;
const DIRECT_RATE: u64 = 99;
const REDUCTION: i128 = (100 * POS_SCALE + POS_SCALE / 3 + 1) as i128;

fn fee_atoms(size: i128, bps: u64) -> u128 {
    let notional = (size.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE);
    (notional * u128::from(bps)).div_ceil(10_000)
}

fn trade(w: &World, batch: bool, step: usize, size: i128) -> Instruction {
    let [a, b] = w.portfolios;
    let cpi = step != 1;
    let mut request = match (batch, cpi) {
        (false, true) => w.env.trade_cpi_ix(a, b, 0, size, CPI_CAP, PRICE),
        (false, false) => w.env.trade_no_cpi_ix(a, b, 0, size, PRICE, DIRECT_RATE),
        (true, true) => w.env.batch_trade_cpi_ix_with_caps(
            a,
            b,
            vec![BatchTradeCpiLeg {
                asset_index: 0,
                market_id: w.env.asset_market_id(0),
                size_q: size,
                fee_bps: u64::from(LP_CAP_BPS),
                limit_price: PRICE,
            }],
            0,
            fee_atoms(size, CPI_CAP),
        ),
        (true, false) => w.env.batch_trade_no_cpi_ix(
            a,
            b,
            vec![BatchTradeLeg {
                asset_index: 0,
                market_id: w.env.asset_market_id(0),
                size_q: size,
                exec_price: PRICE,
                fee_bps: DIRECT_RATE,
            }],
        ),
    };
    // These are future authorization fields in signed messages, not state writes.
    match &mut request {
        ProgInstruction::TradeCpi {
            account_a_position_epoch,
            account_b_position_epoch,
            account_b_matcher_sequence,
            ..
        }
        | ProgInstruction::BatchTradeCpi {
            account_a_position_epoch,
            account_b_position_epoch,
            account_b_matcher_sequence,
            ..
        } => {
            *account_a_position_epoch += step as u64;
            *account_b_position_epoch += step as u64;
            *account_b_matcher_sequence += u64::from(step == 2);
        }
        ProgInstruction::TradeNoCpi {
            account_a_position_epoch,
            account_b_position_epoch,
            ..
        }
        | ProgInstruction::BatchTradeNoCpi {
            account_a_position_epoch,
            account_b_position_epoch,
            ..
        } => {
            *account_a_position_epoch += step as u64;
            *account_b_position_epoch += step as u64;
        }
        _ => unreachable!(),
    }
    let accounts = if cpi {
        vec![
            AccountMeta::new(w.owners[0].pubkey(), true),
            AccountMeta::new(w.env.market, false),
            AccountMeta::new(a, false),
            AccountMeta::new(b, false),
            AccountMeta::new_readonly(w.matcher, false),
            AccountMeta::new(w.context, false),
            AccountMeta::new_readonly(w.delegate, false),
        ]
    } else {
        vec![
            AccountMeta::new(w.owners[0].pubkey(), true),
            AccountMeta::new(w.owners[1].pubkey(), true),
            AccountMeta::new(w.env.market, false),
            AccountMeta::new(a, false),
            AccountMeta::new(b, false),
        ]
    };
    Instruction {
        program_id: w.env.program_id,
        accounts,
        data: request.encode(),
    }
}

fn assert_consent(w: &World, tx: &Transaction, batch: bool, step: usize, size: i128) {
    tx.verify().unwrap();
    let cpi = step != 1;
    let count = usize::from(tx.message.header.num_required_signatures);
    assert_eq!(count, if cpi { 2 } else { 3 });
    let signers = &tx.message.account_keys[..count];
    assert!(signers.contains(&w.owners[0].pubkey()));
    assert_eq!(signers.contains(&w.owners[1].pubkey()), !cpi);
    let request = ProgInstruction::decode(&tx.message.instructions.last().unwrap().data).unwrap();
    match request {
        ProgInstruction::TradeCpi {
            fee_bps,
            limit_price,
            backing_fee_cap_bps,
            ..
        } => {
            assert!(!batch && cpi);
            assert_eq!(
                (fee_bps, limit_price, backing_fee_cap_bps),
                (CPI_CAP, PRICE, 0)
            );
        }
        ProgInstruction::BatchTradeCpi {
            legs,
            max_fee_atoms,
            max_slippage_atoms,
            ..
        } => {
            assert!(batch && cpi);
            assert_eq!(max_fee_atoms, fee_atoms(size, CPI_CAP));
            assert_eq!(max_slippage_atoms, 0);
            assert_eq!(legs.len(), 1);
            assert_eq!(
                (legs[0].fee_bps, legs[0].limit_price),
                (u64::from(LP_CAP_BPS), PRICE)
            );
        }
        ProgInstruction::TradeNoCpi {
            fee_bps,
            exec_price,
            backing_fee_cap_bps,
            ..
        } => {
            assert!(!batch && !cpi);
            assert_eq!(
                (fee_bps, exec_price, backing_fee_cap_bps),
                (DIRECT_RATE, PRICE, 0)
            );
        }
        ProgInstruction::BatchTradeNoCpi { legs, .. } => {
            assert!(batch && !cpi);
            assert_eq!(legs.len(), 1);
            assert_eq!((legs[0].fee_bps, legs[0].exec_price), (DIRECT_RATE, PRICE));
        }
        _ => unreachable!(),
    }
}

#[derive(Default)]
struct Ledger {
    size: i128,
    fees: u128,
    deposited: u64,
    paid: [u64; 2],
}

impl Ledger {
    fn check(&self, w: &World, signed_budget: u128) {
        assert!(
            self.fees <= signed_budget,
            "sum of separately signed fee authorizations"
        );
        let accounts = w.portfolios.map(|key| w.env.portfolio_state(key));
        let (_, group) = w.env.market_state();
        for actor in 0..2 {
            let p = &accounts[actor];
            assert_eq!(p.owner, w.owners[actor].pubkey().to_bytes());
            let deposit = if actor == 0 { self.deposited } else { 0 };
            assert_eq!(
                p.capital.get(),
                u128::from(DEPOSITS[actor] + deposit - self.paid[actor]) - self.fees
            );
            assert_eq!(p.pnl.get(), 0);
            assert_eq!(p.fee_credits.get(), 0);
            assert_eq!(
                w.env.token_amount(w.sources[actor]),
                self.paid[actor]
                    + if actor == 0 {
                        PREFIX - self.deposited
                    } else {
                        0
                    }
            );
            if self.size == 0 {
                assert!(!has_active_leg_for_asset(p, 0));
            } else {
                assert_eq!(
                    active_leg_for_asset(p, 0).basis_pos_q,
                    if actor == 0 { self.size } else { -self.size }
                );
            }
        }
        assert_eq!(group.assets[0].effective_price, PRICE);
        assert_eq!(group.assets[0].oi_eff_long_q, self.size.unsigned_abs());
        assert_eq!(group.assets[0].oi_eff_short_q, self.size.unsigned_abs());
        assert_eq!(&group.insurance_domain_budget[..2], &[self.fees; 2]);
        assert!(group.insurance_domain_budget[2..].iter().all(|x| *x == 0));
        assert_eq!(group.insurance, 2 * self.fees);
        assert_eq!(
            group.c_tot,
            accounts.iter().map(|p| p.capital.get()).sum::<u128>()
        );
        let vault = DEPOSITS.iter().sum::<u64>() + self.deposited - self.paid.iter().sum::<u64>();
        assert_eq!(group.vault, u128::from(vault));
        assert_eq!(w.env.token_amount(w.env.vault), vault);
        assert_eq!(group.vault, group.c_tot + group.insurance);
        let mint = Mint::unpack(&w.env.svm.get_account(&w.env.mint).unwrap().data).unwrap();
        assert_eq!(mint.supply, DEPOSITS.iter().sum::<u64>() + PREFIX);
        assert_eq!(mint.mint_authority, COption::None);
        assert_eq!(
            vault
                + w.sources
                    .iter()
                    .map(|key| w.env.token_amount(*key))
                    .sum::<u64>(),
            mint.supply
        );
        assert_market_stock_census(
            "retained mixed route fees",
            &group,
            &w.env.svm.get_account(&w.env.market).unwrap().data,
            &accounts,
            vault.into(),
        )
        .unwrap();
        assert_reservation_encumbrance_census("retained mixed route fees", &group, &accounts)
            .unwrap();
    }
}

#[test]
fn v16_retained_mixed_route_fee_budgets_survive_bilateral_revocation_and_renewal() {
    let matcher = std::fs::read(auth_matcher_program_path()).unwrap();
    let fees = [
        fee_atoms(QUANTITY, CPI_CAP),
        fee_atoms(REDUCTION, DIRECT_RATE),
        fee_atoms(QUANTITY - REDUCTION, CPI_CAP),
    ];
    assert_eq!(fees, [95, 100, 58]);
    let signed_budget = fees.iter().sum::<u128>();
    assert!(fees[1] > fee_atoms(REDUCTION, 7));
    assert!(fee_atoms(QUANTITY - REDUCTION, 101) > fees[2]);
    let (mut worlds, mut fills, mut rollbacks, mut payouts) = (0, 0, 0, 0);
    let mut peaks = [0; 4]; // Simulation, policy/renewal, rejection, fill/payout.
    for direction in [-1, 1] {
        for cpi_batch in [false, true] {
            for direct_batch in [false, true] {
                eprintln!(
                    "direction={direction}, cpi_batch={cpi_batch}, direct_batch={direct_batch}"
                );
                let mut w = World::new(&matcher);
                let epochs = w.portfolios.map(|key| w.env.portfolio_position_epoch(key));
                let sequence = w.env.portfolio_matcher_sequence(w.portfolios[1]);
                let taker_sequence = w.env.portfolio_matcher_sequence(w.portfolios[0]);
                let requests = w.env.market_state().0.matcher_req_seq;
                let controls = w.env.control_sequences(0);
                let grant = w.env.portfolio_matcher_config(w.portfolios[1]);
                let expiry = w.env.portfolio_matcher_expiry(w.portfolios[1]);
                let sizes = [
                    direction * QUANTITY,
                    -direction * REDUCTION,
                    -direction * (QUANTITY - REDUCTION),
                ];
                let batches = [cpi_batch, direct_batch, cpi_batch];
                let instructions: Vec<_> = (0..3)
                    .map(|step| {
                        let ix = trade(&w, batches[step], step, sizes[step]);
                        if step == 2 {
                            let deposit = w.bundle_instructions(&w.env.trade_cpi_ix(
                                w.portfolios[0],
                                w.portfolios[1],
                                0,
                                sizes[step],
                                CPI_CAP,
                                PRICE,
                            ))[0]
                                .clone();
                            vec![deposit, ix]
                        } else {
                            vec![ix]
                        }
                    })
                    .collect();
                let retained: Vec<_> = instructions
                    .iter()
                    .enumerate()
                    .map(|(i, ixs)| w.sign(ixs, 10 + i as u32))
                    .collect();
                let bytes: Vec<_> = retained
                    .iter()
                    .map(|tx| bincode::serialize(tx).unwrap())
                    .collect();
                let refused = w.sign(&instructions[2], 20);
                let refused_bytes = bincode::serialize(&refused).unwrap();
                let renewal = w.sign(
                    &[Instruction {
                        program_id: w.env.program_id,
                        accounts: vec![
                            AccountMeta::new(w.owners[1].pubkey(), true),
                            AccountMeta::new_readonly(w.env.market, false),
                            AccountMeta::new(w.portfolios[1], false),
                            AccountMeta::new_readonly(w.matcher, false),
                            AccountMeta::new_readonly(w.context, false),
                            AccountMeta::new_readonly(w.delegate, false),
                        ],
                        data: ProgInstruction::SetMatcherConfig {
                            portfolio_id: w.env.portfolio_id(w.portfolios[1]),
                            expected_sequence: sequence,
                            enabled: 1,
                            trade_fee_cap_bps: LP_CAP_BPS,
                            expiry_slot: expiry,
                        }
                        .encode(),
                    }],
                    30,
                );
                let renewal_bytes = bincode::serialize(&renewal).unwrap();
                for step in 0..3 {
                    assert_consent(&w, &retained[step], batches[step], step, sizes[step]);
                }
                peaks[0] = peaks[0].max(w.simulate(&retained[0]));
                let mut ledger = Ledger::default();
                ledger.check(&w, signed_budget);
                for step in 0..3 {
                    if step == 2 {
                        assert_eq!(w.env.portfolio_matcher_config(w.portfolios[1]).enabled(), 0);
                        assert_eq!(bincode::serialize(&renewal).unwrap(), renewal_bytes);
                        peaks[1] = peaks[1].max(w.deliver(
                            renewal.clone(),
                            false,
                            &[w.portfolios[1]],
                            [1, 0, 0],
                        ));
                        assert_eq!(
                            w.env.portfolio_matcher_sequence(w.portfolios[1]),
                            sequence + 1
                        );
                        ledger.check(&w, signed_budget);
                        peaks[1] = peaks[1].max(w.policy(101, 42));
                        ledger.check(&w, signed_budget);
                        assert_eq!(bincode::serialize(&refused).unwrap(), refused_bytes);
                        peaks[2] = peaks[2].max(w.deliver(
                            refused.clone(),
                            true,
                            &[],
                            [1, 1, usize::from(cpi_batch)],
                        ));
                        rollbacks += 1;
                        ledger.check(&w, signed_budget);
                    }
                    let rate = if step == 1 { 7 } else { CPI_CAP };
                    peaks[1] = peaks[1].max(w.policy(rate, 50 + step as u32));
                    ledger.check(&w, signed_budget);
                    let tx = &retained[step];
                    assert_eq!(bincode::serialize(tx).unwrap(), bytes[step]);
                    assert_consent(&w, tx, batches[step], step, sizes[step]);
                    let cpi = step != 1;
                    let mut changed = vec![w.env.market, w.portfolios[0], w.portfolios[1]];
                    if cpi && !cpi_batch {
                        changed.push(w.context);
                    }
                    if step == 2 {
                        changed.extend([w.sources[0], w.env.vault]);
                    }
                    peaks[3] = peaks[3].max(w.deliver(
                        tx.clone(),
                        false,
                        &changed,
                        [
                            1 + usize::from(step == 2),
                            usize::from(step == 2),
                            usize::from(cpi),
                        ],
                    ));
                    ledger.size += sizes[step];
                    ledger.fees += fees[step];
                    if step == 2 {
                        ledger.deposited = PREFIX;
                    }
                    ledger.check(&w, signed_budget);
                    assert_eq!(
                        w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                        epochs.map(|epoch| epoch + step as u64 + 1)
                    );
                    assert_eq!(
                        w.env.market_state().0.matcher_req_seq,
                        requests + if step == 2 { 2 } else { 1 }
                    );
                    assert_eq!(
                        w.env.portfolio_matcher_sequence(w.portfolios[0]),
                        taker_sequence + u64::from(step == 2)
                    );
                    assert_eq!(
                        w.env.portfolio_matcher_sequence(w.portfolios[1]),
                        sequence + u64::from(step == 2)
                    );
                    let current = w.env.portfolio_matcher_config(w.portfolios[1]);
                    assert_eq!(
                        (
                            current.matcher_program,
                            current.matcher_context,
                            current.matcher_delegate
                        ),
                        (
                            grant.matcher_program,
                            grant.matcher_context,
                            grant.matcher_delegate
                        )
                    );
                    assert_eq!(current.trade_fee_cap_bps(), LP_CAP_BPS);
                    assert_eq!(current.enabled(), u64::from(cpi));
                    assert_eq!(
                        w.env.portfolio_matcher_expiry(w.portfolios[1]),
                        if cpi { expiry } else { 0 }
                    );
                    let mut expected_controls = controls;
                    expected_controls.trade_fee += step as u64 + 1 + u64::from(step == 2);
                    assert_eq!(w.env.control_sequences(0), expected_controls);
                    fills += 1;
                }
                assert_eq!(ledger.fees, signed_budget);
                assert_eq!(ledger.size, 0);
                for actor in 0..2 {
                    let amount = u128::from(DEPOSITS[actor] + if actor == 0 { PREFIX } else { 0 })
                        - signed_budget;
                    let tx = w.sign(
                        &[Instruction {
                            program_id: w.env.program_id,
                            accounts: vec![
                                AccountMeta::new(w.owners[actor].pubkey(), true),
                                AccountMeta::new(w.env.market, false),
                                AccountMeta::new(w.portfolios[actor], false),
                                AccountMeta::new(w.sources[actor], false),
                                AccountMeta::new(w.env.vault, false),
                                AccountMeta::new_readonly(w.env.vault_authority, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                            data: w.env.withdraw_ix(w.portfolios[actor], amount).encode(),
                        }],
                        60 + actor as u32,
                    );
                    peaks[3] = peaks[3].max(w.deliver(
                        tx,
                        false,
                        &[
                            w.env.market,
                            w.portfolios[actor],
                            w.sources[actor],
                            w.env.vault,
                        ],
                        [1, 1, 0],
                    ));
                    ledger.paid[actor] = amount as u64;
                    ledger.check(&w, signed_budget);
                    payouts += 1;
                }
                assert_eq!(ledger.paid, [99_863, 199_754]);
                assert_eq!(w.env.token_amount(w.env.vault), 506);
                worlds += 1;
            }
        }
    }
    assert_eq!((worlds, fills, rollbacks, payouts), (8, 24, 8, 16));
    eprintln!("INV-014 row 411 mixed route fees: worlds={worlds}, fills={fills}, exact_rollbacks={rollbacks}, payouts={payouts}, peak_CU_simulation_policy_rejection_execution={peaks:?}");
}
