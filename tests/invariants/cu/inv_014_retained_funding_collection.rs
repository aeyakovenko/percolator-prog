//! INV-014: pending funding is not a retained trade-fee allowance. A real partial
//! fill, authority/policy return, maintenance collection and four closing routes
//! share an input ledger and exact transaction rollback. Public construction only.

use super::*;

const MAINTENANCE: u64 = 307;
const QUANTITY: i128 = 95 * POS_SCALE as i128;
const FUNDING_RATE: u64 = 1_000;

fn funding_quote() -> u64 {
    // A negative premium floors the signed index, so the absolute per-unit
    // transfer is the ceiling. No observed balances seed this oracle.
    let per_unit = (u128::from(PRICE) * u128::from(FUNDING_RATE)).div_ceil(percolator::FUNDING_DEN);
    u64::try_from(QUANTITY.unsigned_abs() / POS_SCALE * per_unit).unwrap()
}

struct Ledger {
    direction: i128,
    closed: bool,
    fee_slots: [u64; 2],
    maintenance_domains: [u128; 2],
    converted: bool,
    paid: [u64; 2],
    custody: [Account; 3],
}

impl Ledger {
    fn new(w: &World, direction: i128) -> Self {
        Self {
            direction,
            closed: false,
            fee_slots: [0; 2],
            maintenance_domains: [0; 2],
            converted: false,
            paid: [0; 2],
            custody: [w.tokens[0], w.tokens[1], w.env.vault]
                .map(|key| w.env.svm.get_account(&key).unwrap()),
        }
    }

    fn winner(&self) -> usize {
        usize::from(self.direction < 0)
    }

    fn collect(&mut self, actor: usize, slot: u64) {
        let amount = u128::from((slot - self.fee_slots[actor]) * MAINTENANCE);
        self.maintenance_domains[0] += amount / 2;
        self.maintenance_domains[1] += amount - amount / 2;
        self.fee_slots[actor] = slot;
    }

    fn capital(&self, actor: usize) -> u64 {
        let fees = fee(QUANTITY, OPEN_CAP) as u64 * (1 + u64::from(self.closed));
        PRINCIPAL[actor] + u64::from(actor == 0) * DEPOSIT
            - fees
            - self.fee_slots[actor] * MAINTENANCE
            - u64::from(self.closed && actor != self.winner()) * funding_quote()
            + u64::from(self.converted && actor == self.winner()) * funding_quote()
            - self.paid[actor]
    }

    fn check(&self, w: &World) {
        let accounts = w.portfolios.map(|key| w.env.portfolio_state(key));
        for (actor, p) in accounts.iter().enumerate() {
            assert_eq!(p.owner, w.owners[actor].pubkey().to_bytes());
            assert_eq!(
                p.capital.get(),
                u128::from(self.capital(actor)),
                "capital {actor}"
            );
            assert_eq!(
                p.pnl.get(),
                i128::from(self.closed && !self.converted && actor == self.winner())
                    * i128::from(funding_quote()),
                "funding PnL {actor}"
            );
            assert_eq!(p.fee_credits.get(), 0);
            assert_eq!(p.last_fee_slot.get(), self.fee_slots[actor]);
            if self.closed {
                assert!(!has_active_leg_for_asset(p, 0));
            } else {
                assert_eq!(
                    active_leg_for_asset(p, 0).basis_pos_q,
                    self.direction * QUANTITY * if actor == 0 { 1 } else { -1 }
                );
            }
        }
        let (cfg, group) = w.env.market_state();
        let fees = fee(QUANTITY, OPEN_CAP) * (1 + u128::from(self.closed));
        let domains = self.maintenance_domains.map(|amount| amount + fees);
        assert_eq!(&group.insurance_domain_budget[..2], &domains);
        assert!(group.insurance_domain_budget[2..].iter().all(|v| *v == 0));
        assert_eq!(group.insurance, domains.iter().sum());
        assert_eq!(cfg.fee_redirect_to_market_0_bps, 0);
        assert_eq!(group.assets[0].effective_price, PRICE);
        let oi = if self.closed { 0 } else { QUANTITY as u128 };
        assert_eq!(group.assets[0].oi_eff_long_q, oi);
        assert_eq!(group.assets[0].oi_eff_short_q, oi);
        let vault = PRINCIPAL.iter().sum::<u64>() + DEPOSIT - self.paid.iter().sum::<u64>();
        assert_eq!(group.vault, u128::from(vault));
        assert_eq!(group.c_tot, accounts.iter().map(|p| p.capital.get()).sum());
        let residual = u128::from(self.closed && !self.converted) * u128::from(funding_quote());
        assert_eq!(group.vault, group.c_tot + group.insurance + residual);
        let balances = [self.paid[0], self.paid[1], vault];
        for ((key, before), amount) in [w.tokens[0], w.tokens[1], w.env.vault]
            .into_iter()
            .zip(&self.custody)
            .zip(balances)
        {
            let mut expected = before.clone();
            let mut token = TokenAccount::unpack(&expected.data).unwrap();
            token.amount = amount;
            TokenAccount::pack(token, &mut expected.data).unwrap();
            assert_eq!(w.env.svm.get_account(&key), Some(expected));
        }
        let mint = Mint::unpack(&w.env.svm.get_account(&w.env.mint).unwrap().data).unwrap();
        assert_eq!(mint.mint_authority, COption::None);
        assert_eq!(mint.supply, PRINCIPAL.iter().sum::<u64>() + DEPOSIT);
        assert_eq!(balances.iter().sum::<u64>(), mint.supply);
        assert_market_stock_census(
            "retained close with funding and maintenance",
            &group,
            &w.env.svm.get_account(&w.env.market).unwrap().data,
            &accounts,
            vault.into(),
        )
        .unwrap();
        assert_reservation_encumbrance_census("retained funding close", &group, &accounts).unwrap();
    }
}

fn collect(w: &World, actor: usize, slot: u64) -> Instruction {
    Instruction {
        program_id: w.env.program_id,
        accounts: vec![
            AccountMeta::new(w.env.market, false),
            AccountMeta::new(w.portfolios[actor], false),
        ],
        data: ProgInstruction::SyncMaintenanceFee { now_slot: slot }.encode(),
    }
}

#[test]
fn v16_retained_partial_close_separates_funding_and_maintenance_from_fee_consent() {
    let matcher = std::fs::read(hostile_matcher_program_path()).unwrap();
    let mut counts = Counts::default();
    assert_eq!(fee(QUANTITY, OPEN_CAP), 36);
    assert_eq!(funding_quote(), 95);
    assert!(u128::from(funding_quote()) > fee(QUANTITY, CLOSE_FRESH));
    assert!(u128::from(MAINTENANCE) > fee(QUANTITY, OPEN_CAP));
    for direction in [-1, 1] {
        let mut endpoint = None;
        for route in [
            Route::SingleCpi,
            Route::BatchCpi,
            Route::SingleNoCpi,
            Route::BatchNoCpi,
        ] {
            eprintln!("retained funding close: direction={direction}, route={route:?}");
            let mut w = World::with_params(
                &matcher,
                V16CuMarketParams {
                    trade_fee_base_bps: OPEN_CAP,
                    maintenance_fee_per_slot: MAINTENANCE.into(),
                    max_abs_funding_e9_per_slot: FUNDING_RATE,
                    max_price_move_bps_per_slot: 1,
                    h_max: 1,
                    ..V16CuMarketParams::default()
                },
            );
            counts.record(w.env.configure_ewma_mark_with_cu(0, PRICE, 1, 0));
            counts.record(w.partial(95));
            let opening = w.bundle_instructions(
                Route::PartialCpi,
                direction * 255 * POS_SCALE as i128,
                direction * QUANTITY,
                OPEN_CAP,
            );
            let tx = w.sign_with_nonce(&opening, 1);
            counts.record(w.deliver(
                tx,
                false,
                &[
                    w.env.market,
                    w.portfolios[0],
                    w.portfolios[1],
                    w.tokens[0],
                    w.env.vault,
                    w.context,
                ],
                [2, 1, 1],
            ));
            counts.fills += 1;
            let fill =
                read_matcher_return(&w.env.svm.get_account(&w.context).unwrap().data).unwrap();
            assert_eq!(
                (fill.exec_size, fill.exec_price_e6),
                (direction * QUANTITY, PRICE)
            );
            assert_ne!(fill.flags & FLAG_PARTIAL_OK, 0);
            let mut book = Ledger::new(&w, direction);
            book.check(&w);
            let tx = w.sign_with_nonce(
                &[Instruction {
                    program_id: w.matcher,
                    accounts: vec![
                        AccountMeta::new_readonly(w.owners[1].pubkey(), true),
                        AccountMeta::new(w.context, false),
                    ],
                    data: vec![11, 9, 0],
                }],
                2,
            );
            counts.record(w.deliver(tx, false, &[w.context], [0, 0, 1]));
            book.check(&w);

            let close = w.bundle_instructions(
                route,
                -direction * QUANTITY,
                -direction * QUANTITY,
                OPEN_CAP,
            )[1]
            .clone();
            let ixs = [close];
            let initial = w.env.control_sequences(0);
            let stale = fee_control(
                &w,
                w.env.admin.pubkey(),
                OPEN_CAP,
                initial.trade_fee + 7,
                initial.authority_epoch,
            );
            let mut suffix = ixs.to_vec();
            suffix.push(stale);
            let retained = [
                w.sign_with_nonce(&ixs, 10),
                w.sign_with_nonce(&suffix, 11),
                w.sign_with_nonce(&ixs, 12),
                w.sign_with_nonce(&ixs, 13),
            ];
            let wires = retained
                .each_ref()
                .map(|tx| bincode::serialize(tx).unwrap());
            for (index, tx) in retained.iter().enumerate() {
                assert_eq!(
                    tx.message.header.num_required_signatures,
                    (if route.cpi() { 2 } else { 3 }) + u8::from(index == 1)
                );
            }
            probe(&mut w, &retained[2], true, 2, &mut counts);
            book.check(&w);
            let epochs = w.portfolios.map(|key| w.env.portfolio_position_epoch(key));
            let grant = w.env.portfolio_matcher_sequence(w.portfolios[1]);
            let requests = w.env.market_state().0.matcher_req_seq;
            let a = w.env.admin.pubkey();
            let b = w.owners[1].pubkey();
            counts.record(handoff(&mut w, a, b, 20));
            book.check(&w);
            let high = fee_control(
                &w,
                b,
                CLOSE_FRESH,
                initial.trade_fee + 1,
                initial.authority_epoch + 1,
            );
            let tx = w.sign_with_nonce(&[high], 21);
            counts.record(w.deliver(tx, false, &[w.env.market], [1, 0, 0]));
            book.check(&w);
            counts.record(handoff(&mut w, b, a, 22));
            book.check(&w);
            assert_eq!(
                w.env.control_sequences(0).authority_epoch,
                initial.authority_epoch + 2
            );
            assert_eq!(w.env.control_sequences(0).trade_fee, initial.trade_fee + 1);

            // The mark's first slot is non-retroactive. The one-bps circuit
            // breaker keeps execution at 100; the next slot floors negative
            // funding (-100 * 1000 / 1e9) to -1 per position unit.
            w.env.svm.warp_to_slot(1);
            counts.record(w.env.push_ewma_mark_with_cu(1, 98));
            book.check(&w);
            let crank = Instruction {
                program_id: w.env.program_id,
                accounts: vec![
                    AccountMeta::new_readonly(w.env.payer.pubkey(), true),
                    AccountMeta::new(w.env.market, false),
                    AccountMeta::new(w.portfolios[0], false),
                ],
                data: ProgInstruction::PermissionlessCrank {
                    now_slot: 1,
                    observations: crank_observations(0),
                }
                .encode(),
            };
            let tx = w.sign_with_nonce(&[crank], 23);
            counts.record(w.deliver(tx, false, &[w.env.market, w.portfolios[0]], [1, 0, 0]));
            book.collect(0, 1);
            book.check(&w);
            assert_eq!(w.env.market_state().1.assets[0].f_long_num, 0);
            w.env.svm.warp_to_slot(2);
            // Restore the quote for all four execution routes without erasing
            // the preceding slot's funding checkpoint (EWMA 99 -> 100).
            counts.record(w.env.push_ewma_mark_with_cu(2, 101));
            book.check(&w);
            assert_eq!(w.env.market_state().0.mark_ewma_e6, PRICE);
            counts.record(w.deliver_with_error(
                retained[0].clone(),
                Some((
                    2,
                    InstructionError::Custom(PercolatorError::InvalidInstruction as u32),
                )),
                &[],
                [0, 0, usize::from(route == Route::BatchCpi)],
            ));
            counts.rollbacks += 1;
            book.check(&w);
            counts.record(policy(&mut w, OPEN_CAP, 24));
            book.check(&w);
            assert_eq!(w.env.control_sequences(0).trade_fee, initial.trade_fee + 2);
            // A successful close settles funding and pays the signed fee;
            // the obsolete policy suffix must roll all of it back.
            counts.record(w.deliver_with_error(
                retained[1].clone(),
                Some((
                    3,
                    InstructionError::Custom(PercolatorError::EngineStale as u32),
                )),
                &[],
                [1, 0, usize::from(route.cpi())],
            ));
            counts.rollbacks += 1;
            book.check(&w);
            let mut changed = vec![w.env.market, w.portfolios[0], w.portfolios[1]];
            if route == Route::SingleCpi {
                changed.push(w.context);
            }
            counts.record(w.deliver(
                retained[2].clone(),
                false,
                &changed,
                [1, 0, usize::from(route.cpi())],
            ));
            counts.fills += 1;
            book.closed = true;
            book.collect(0, 2);
            book.collect(1, 2);
            book.check(&w);
            let (_, group) = w.env.market_state();
            assert_eq!(group.assets[0].f_long_num, ADL_ONE as i128);
            assert_eq!(group.assets[0].f_short_num, -(ADL_ONE as i128));
            assert_eq!(group.funding_epoch, 1);
            assert_eq!(
                w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                epochs.map(|e| e + 1)
            );
            assert_eq!(
                w.env.market_state().0.matcher_req_seq,
                requests + u64::from(route.cpi())
            );
            assert_eq!(w.env.portfolio_matcher_sequence(w.portfolios[1]), grant);
            assert_eq!(
                w.env.portfolio_matcher_config(w.portfolios[1]).enabled(),
                u64::from(route.cpi())
            );
            if route == Route::SingleCpi {
                let fill =
                    read_matcher_return(&w.env.svm.get_account(&w.context).unwrap().data).unwrap();
                assert_eq!(
                    (fill.exec_size, fill.exec_price_e6),
                    (-direction * QUANTITY, PRICE)
                );
            }
            counts.record(w.deliver_with_error(
                retained[3].clone(),
                Some((
                    2,
                    InstructionError::Custom(PercolatorError::EngineStale as u32),
                )),
                &[],
                [0, 0, 0],
            ));
            counts.rollbacks += 1;
            book.check(&w);
            for (tx, wire) in retained.iter().zip(wires) {
                tx.verify().unwrap();
                assert_eq!(bincode::serialize(tx).unwrap(), wire);
            }
            w.env.svm.warp_to_slot(3);
            for actor in 0..2 {
                let tx = w.sign_with_nonce(&[collect(&w, actor, 3)], 30 + actor as u32);
                counts.record(w.deliver(
                    tx,
                    false,
                    &[w.env.market, w.portfolios[actor]],
                    [1, 0, 0],
                ));
                book.collect(actor, 3);
                book.check(&w);
            }
            let winner = book.winner();
            let refresh = w.sign_with_nonce(
                &[Instruction {
                    program_id: w.env.program_id,
                    accounts: vec![
                        AccountMeta::new_readonly(w.env.payer.pubkey(), true),
                        AccountMeta::new(w.env.market, false),
                        AccountMeta::new(w.portfolios[winner], false),
                    ],
                    data: ProgInstruction::PermissionlessCrank {
                        now_slot: 3,
                        observations: crank_observations(0),
                    }
                    .encode(),
                }],
                35,
            );
            counts.record(w.deliver(
                refresh,
                false,
                &[w.env.market, w.portfolios[winner]],
                [1, 0, 0],
            ));
            book.check(&w);
            let tx = w.sign_with_nonce(
                &[Instruction {
                    program_id: w.env.program_id,
                    accounts: vec![
                        AccountMeta::new(w.owners[winner].pubkey(), true),
                        AccountMeta::new(w.env.market, false),
                        AccountMeta::new(w.portfolios[winner], false),
                    ],
                    data: w
                        .env
                        .convert_released_pnl_ix(w.portfolios[winner], funding_quote().into())
                        .encode(),
                }],
                32,
            );
            counts.record(w.deliver(tx, false, &[w.env.market, w.portfolios[winner]], [1, 0, 0]));
            book.converted = true;
            book.check(&w);
            for actor in 0..2 {
                let amount = book.capital(actor);
                let tx = w.sign_with_nonce(
                    &[Instruction {
                        program_id: w.env.program_id,
                        accounts: vec![
                            AccountMeta::new(w.owners[actor].pubkey(), true),
                            AccountMeta::new(w.env.market, false),
                            AccountMeta::new(w.portfolios[actor], false),
                            AccountMeta::new(w.tokens[actor], false),
                            AccountMeta::new(w.env.vault, false),
                            AccountMeta::new_readonly(w.env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: w
                            .env
                            .withdraw_ix(w.portfolios[actor], amount.into())
                            .encode(),
                    }],
                    33 + actor as u32,
                );
                counts.record(w.deliver(
                    tx,
                    false,
                    &[
                        w.env.market,
                        w.portfolios[actor],
                        w.tokens[actor],
                        w.env.vault,
                    ],
                    [1, 1, 0],
                ));
                book.paid[actor] = amount;
                counts.payouts += 1;
                book.check(&w);
            }
            let outcome = (book.paid, w.env.token_amount(w.env.vault));
            assert_eq!(outcome.1, 1_986);
            if let Some(expected) = endpoint {
                assert_eq!(outcome, expected);
            } else {
                endpoint = Some(outcome);
            }
            counts.worlds += 1;
        }
    }
    assert_eq!((counts.worlds, counts.rollbacks, counts.fills), (8, 24, 16));
    assert_eq!(counts.payouts, 16);
    assert_eq!(counts.permitted, 8);
    eprintln!("INV-014 retained funding collection: {counts:?}");
}
