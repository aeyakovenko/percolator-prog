//! Primary INV-014: retained single-CPI fee rates remain binding at every prefix
//! of a bounded generated policy history before and after an actual partial fill.
//! INV-010/011/024/036/047/081: consumed-episode retry, instruction-local fee
//! budgets, attributed stock, adjacent route economics and complete rollback.
//! Public Live/SPL construction; fixed price, funded base fees, one asset only.
//! This narrows row 411; it does not establish whole-row or universal coverage.

use super::*;

const OPEN_CAP: u64 = 37;
const OPEN_FRESH: u64 = 53;
const CLOSE_FRESH: u64 = 71;

#[derive(Default, Debug)]
struct Counts {
    worlds: usize,
    permitted: usize,
    denied: usize,
    rollbacks: usize,
    fills: usize,
    payouts: usize,
    peak_cu: u64,
}

impl Counts {
    fn record(&mut self, cu: u64) {
        self.peak_cu = self.peak_cu.max(cu);
    }
}

struct Book {
    position: i128,
    fees: u128,
    deposited: bool,
    paid: [u64; 2],
    fills: u64,
    cpi_fills: u64,
    epochs: [u64; 2],
    requests: u64,
    custody: [Account; 3],
}

impl Book {
    fn new(w: &World) -> Self {
        Self {
            position: 0,
            fees: 0,
            deposited: false,
            paid: [0; 2],
            fills: 0,
            cpi_fills: 0,
            epochs: w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
            requests: w.env.market_state().0.matcher_req_seq,
            custody: [w.tokens[0], w.tokens[1], w.env.vault]
                .map(|key| w.env.svm.get_account(&key).unwrap()),
        }
    }

    fn entitlement(&self, actor: usize) -> u64 {
        PRINCIPAL[actor] + u64::from(actor == 0 && self.deposited) * DEPOSIT
            - u64::try_from(self.fees).unwrap()
    }

    fn check(&self, w: &World) {
        let portfolios = w.portfolios.map(|key| w.env.portfolio_state(key));
        for (actor, portfolio) in portfolios.iter().enumerate() {
            assert_eq!(portfolio.owner, w.owners[actor].pubkey().to_bytes());
            assert_eq!(
                portfolio.capital.get(),
                u128::from(self.entitlement(actor) - self.paid[actor])
            );
            assert_eq!(portfolio.pnl.get(), 0);
            assert_eq!(portfolio.reserved_pnl.get(), 0);
            assert_eq!(portfolio.fee_credits.get(), 0);
            if self.position == 0 {
                assert!(!has_active_leg_for_asset(portfolio, 0));
            } else {
                assert_eq!(
                    active_leg_for_asset(portfolio, 0).basis_pos_q,
                    if actor == 0 {
                        self.position
                    } else {
                        -self.position
                    }
                );
            }
        }
        assert_eq!(
            w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
            self.epochs.map(|epoch| epoch + self.fills)
        );
        let (config, group) = w.env.market_state();
        assert_eq!(config.matcher_req_seq, self.requests + self.cpi_fills);
        assert_eq!(config.fee_redirect_to_market_0_bps, 0);
        assert_eq!(
            w.env
                .portfolio_matcher_config(w.portfolios[1])
                .trade_fee_cap_bps(),
            LP_CAP
        );
        assert_eq!(group.assets[0].effective_price, PRICE);
        assert_eq!(group.assets[0].oi_eff_long_q, self.position.unsigned_abs());
        assert_eq!(group.assets[0].oi_eff_short_q, self.position.unsigned_abs());
        assert_eq!(group.insurance, 2 * self.fees);
        assert_eq!(&group.insurance_domain_budget[..2], &[self.fees; 2]);
        assert!(group.insurance_domain_budget[2..].iter().all(|v| *v == 0));
        let vault = PRINCIPAL.iter().sum::<u64>() + u64::from(self.deposited) * DEPOSIT
            - self.paid.iter().sum::<u64>();
        assert_eq!(group.vault, u128::from(vault));
        assert_eq!(group.c_tot + group.insurance, group.vault);
        assert_eq!(
            group.c_tot,
            portfolios.iter().map(|p| p.capital.get()).sum()
        );
        let balances = [
            self.paid[0] + u64::from(!self.deposited) * DEPOSIT,
            self.paid[1],
            vault,
        ];
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
            "generated retained partial policy words",
            &group,
            &w.env.svm.get_account(&w.env.market).unwrap().data,
            &portfolios,
            vault.into(),
        )
        .unwrap();
        assert_reservation_encumbrance_census("generated fee words", &group, &portfolios).unwrap();
    }
}

fn policy(w: &mut World, rate: u64, nonce: u32) -> u64 {
    let mut controls = w.env.control_sequences(0);
    let mut config = w.env.market_state().0;
    let tx = w.sign_with_nonce(
        &[Instruction {
            program_id: w.env.program_id,
            accounts: vec![
                AccountMeta::new(w.env.admin.pubkey(), true),
                AccountMeta::new(w.env.market, false),
            ],
            data: ProgInstruction::UpdateTradeFeePolicy {
                trade_fee_base_bps: rate,
                policy_sequence: controls.trade_fee + 1,
                authority_epoch: controls.authority_epoch,
            }
            .encode(),
        }],
        nonce,
    );
    let cu = w.deliver(tx, false, &[w.env.market], [1, 0, 0]);
    controls.trade_fee += 1;
    config.trade_fee_base_bps = rate;
    assert_eq!(w.env.control_sequences(0), controls);
    assert_eq!(w.env.market_state().0, config);
    cu
}

fn probe(w: &mut World, tx: &Transaction, allowed: bool, index: u8, counts: &mut Counts) {
    let mut before = w.frame(tx);
    let result = w.env.svm.simulate_transaction(tx.clone().into());
    let meta = if allowed {
        counts.permitted += 1;
        result.expect("retained bounds still admit the current policy")
    } else {
        counts.denied += 1;
        let failure = result.expect_err("current policy exceeds retained instruction consent");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                index,
                InstructionError::Custom(PercolatorError::InvalidInstruction as u32)
            )
        );
        // LiteSVM 0.1 debits the payer on failed simulations as well as deliveries.
        before
            .get_mut(&w.env.payer.pubkey())
            .unwrap()
            .as_mut()
            .unwrap()
            .lamports -= FeeStructure::default().lamports_per_signature
            * u64::from(tx.message.header.num_required_signatures);
        failure.meta
    };
    assert_eq!(w.frame(tx), before, "complete simulation frame");
    assert_cu_within(
        "generated retained fee probe",
        meta.compute_units_consumed,
        CU_LIMIT,
    );
    counts.record(meta.compute_units_consumed);
}

fn revise_fee(w: &World, instructions: &[Instruction], rate: u64, nonce: u32) -> Transaction {
    let old = w.sign_with_nonce(instructions, nonce);
    let mut revised = instructions.to_vec();
    let index = revised.len() - 1;
    let mut trade = ProgInstruction::decode(&revised[index].data).unwrap();
    match &mut trade {
        ProgInstruction::TradeCpi { fee_bps, .. } | ProgInstruction::TradeNoCpi { fee_bps, .. } => {
            *fee_bps = rate
        }
        _ => unreachable!(),
    }
    revised[index].data = trade.encode();
    let fresh = w.sign_with_nonce(&revised, nonce);
    let mut expected = old.message.clone();
    expected.instructions[index + 2].data = revised[index].data.clone();
    assert_eq!(fresh.message, expected, "only the signed fee rate changes");
    assert_ne!(fresh.signatures, old.signatures);
    fresh
}

fn late_rollback(w: &mut World, fresh: &Transaction, calls: [usize; 3], counts: &mut Counts) {
    let mut instructions: Vec<_> = fresh.message.instructions[2..]
        .iter()
        .map(|ix| Instruction {
            program_id: fresh.message.account_keys[ix.program_id_index as usize],
            accounts: ix
                .accounts
                .iter()
                .map(|index| {
                    let index = usize::from(*index);
                    AccountMeta {
                        pubkey: fresh.message.account_keys[index],
                        is_signer: fresh.message.is_signer(index),
                        is_writable: fresh.message.is_writable(index),
                    }
                })
                .collect(),
            data: ix.data.clone(),
        })
        .collect();
    instructions.push(
        spl_token::instruction::transfer(
            &spl_token::ID,
            &w.tokens[0],
            &w.env.vault,
            &w.owners[0].pubkey(),
            &[],
            1,
        )
        .unwrap(),
    );
    let tx = w.sign_with_nonce(&instructions, 60);
    counts.record(w.deliver_with_error(
        tx,
        Some((
            (instructions.len() + 1) as u8,
            InstructionError::Custom(spl_token::error::TokenError::InsufficientFunds as u32),
        )),
        &[],
        calls,
    ));
    counts.rollbacks += 1;
}

fn consumed_retry(w: &mut World, retained: &Transaction, counts: &mut Counts) {
    counts.record(w.deliver_with_error(
        retained.clone(),
        Some((
            2,
            InstructionError::Custom(PercolatorError::EngineStale as u32),
        )),
        &[],
        [0, 0, 0],
    ));
    counts.rollbacks += 1;
}

#[test]
fn v16_generated_partial_policy_words_preserve_retained_fee_and_retry_budgets() {
    let matcher = std::fs::read(hostile_matcher_program_path()).unwrap();
    let mut counts = Counts::default();
    for code in 0..27u32 {
        let word = [code % 3, (code / 3) % 3, code / 9];
        let direction = if code % 2 == 0 { 1 } else { -1 };
        let numerator = [63u8, 95, 127][word[1] as usize];
        let request = direction
            * (((120 + 3 * u128::from(code)) * POS_SCALE + POS_SCALE / 2 + u128::from(code) + 1)
                as i128);
        let executed = request * i128::from(numerator) / 255;
        let open_fee = fee(executed, OPEN_FRESH);
        let close_fee = fee(executed, CLOSE_FRESH);
        assert!(open_fee > fee(executed, OPEN_CAP));
        assert!(open_fee < fee(request, OPEN_CAP));
        assert!(close_fee > open_fee);
        let mut endpoint = None;
        for route in [Route::SingleCpi, Route::SingleNoCpi] {
            for rollback_open in [false, true] {
                eprintln!("policy word={word:?}, route={route:?}, rollback_open={rollback_open}, numerator={numerator}, direction={direction}");
                let mut w = World::new(&matcher);
                let mut book = Book::new(&w);
                let controls = w.env.control_sequences(0);
                let grant_sequence = w.env.portfolio_matcher_sequence(w.portfolios[1]);
                counts.record(w.partial(numerator));
                let opening = w.bundle_instructions(Route::PartialCpi, request, executed, OPEN_CAP);
                let retained = w.sign_with_nonce(&opening, 10);
                let retry = w.sign_with_nonce(&[opening[1].clone()], 11);
                let wires = [&retained, &retry].map(|tx| bincode::serialize(tx).unwrap());
                assert_eq!(retained.message.header.num_required_signatures, 2);
                probe(&mut w, &retained, true, 3, &mut counts);
                book.check(&w);
                for (step, symbol) in word.into_iter().enumerate() {
                    let rate = [19, OPEN_CAP, OPEN_FRESH][symbol as usize];
                    counts.record(policy(&mut w, rate, 20 + step as u32));
                    probe(&mut w, &retained, rate <= OPEN_CAP, 3, &mut counts);
                    book.check(&w);
                }
                counts.record(policy(&mut w, OPEN_FRESH, 23));
                counts.record(w.deliver(retained.clone(), true, &[], [1, 1, 0]));
                counts.rollbacks += 1;
                book.check(&w);
                let fresh = revise_fee(&w, &opening, OPEN_FRESH, 10);
                probe(&mut w, &fresh, true, 3, &mut counts);
                if rollback_open {
                    late_rollback(&mut w, &fresh, [2, 1, 1], &mut counts);
                    book.check(&w);
                }
                let changed = [
                    w.env.market,
                    w.portfolios[0],
                    w.portfolios[1],
                    w.tokens[0],
                    w.env.vault,
                    w.context,
                ];
                counts.record(w.deliver(fresh, false, &changed, [2, 1, 1]));
                counts.fills += 1;
                book.position = executed;
                book.fees += open_fee;
                book.deposited = true;
                book.fills += 1;
                book.cpi_fills += 1;
                book.check(&w);
                let fill =
                    read_matcher_return(&w.env.svm.get_account(&w.context).unwrap().data).unwrap();
                assert_eq!((fill.exec_size, fill.exec_price_e6), (executed, PRICE));
                assert_ne!(fill.flags & FLAG_PARTIAL_OK, 0);
                assert!(executed.unsigned_abs() < request.unsigned_abs());
                if rollback_open {
                    consumed_retry(&mut w, &retry, &mut counts);
                    book.check(&w);
                }
                // The next CPI is exact. The public matcher owner changes only its fill mode.
                let exact = w.sign_with_nonce(
                    &[Instruction {
                        program_id: w.matcher,
                        accounts: vec![
                            AccountMeta::new_readonly(w.owners[1].pubkey(), true),
                            AccountMeta::new(w.context, false),
                        ],
                        data: vec![11, 9, 0],
                    }],
                    30,
                );
                let context = w.context;
                counts.record(w.deliver(exact, false, &[context], [0, 0, 1]));
                book.check(&w);
                let closing =
                    [w.bundle_instructions(route, -executed, -executed, OPEN_FRESH)[1].clone()];
                let retained_close = w.sign_with_nonce(&closing, 31);
                let close_wire = bincode::serialize(&retained_close).unwrap();
                assert_eq!(
                    retained_close.message.header.num_required_signatures,
                    if route.cpi() { 2 } else { 3 }
                );
                probe(&mut w, &retained_close, true, 2, &mut counts);
                for (step, symbol) in word.into_iter().rev().enumerate() {
                    let rate = [19, OPEN_FRESH, CLOSE_FRESH][symbol as usize];
                    counts.record(policy(&mut w, rate, 40 + step as u32));
                    probe(&mut w, &retained_close, rate <= OPEN_FRESH, 2, &mut counts);
                    book.check(&w);
                }
                counts.record(policy(&mut w, CLOSE_FRESH, 43));
                counts.record(w.deliver_with_error(
                    retained_close.clone(),
                    Some((
                        2,
                        InstructionError::Custom(PercolatorError::InvalidInstruction as u32),
                    )),
                    &[],
                    [0, 0, 0],
                ));
                counts.rollbacks += 1;
                book.check(&w);
                let fresh_close = revise_fee(&w, &closing, CLOSE_FRESH, 31);
                probe(&mut w, &fresh_close, true, 2, &mut counts);
                if !rollback_open {
                    late_rollback(
                        &mut w,
                        &fresh_close,
                        [1, 0, usize::from(route.cpi())],
                        &mut counts,
                    );
                    book.check(&w);
                }
                let mut changed = vec![w.env.market, w.portfolios[0], w.portfolios[1]];
                if route.cpi() {
                    changed.push(w.context);
                }
                counts.record(w.deliver(
                    fresh_close,
                    false,
                    &changed,
                    [1, 0, usize::from(route.cpi())],
                ));
                counts.fills += 1;
                book.position = 0;
                book.fees += close_fee;
                book.fills += 1;
                book.cpi_fills += u64::from(route.cpi());
                book.check(&w);
                assert_eq!(
                    w.env.portfolio_matcher_config(w.portfolios[1]).enabled(),
                    u64::from(route.cpi())
                );
                assert_eq!(
                    w.env.portfolio_matcher_sequence(w.portfolios[1]),
                    grant_sequence
                );
                let mut expected_controls = controls;
                expected_controls.trade_fee += 8;
                assert_eq!(w.env.control_sequences(0), expected_controls);
                if !rollback_open {
                    consumed_retry(&mut w, &retry, &mut counts);
                    book.check(&w);
                }
                for (tx, wire) in [&retained, &retry, &retained_close].into_iter().zip([
                    &wires[0],
                    &wires[1],
                    &close_wire,
                ]) {
                    tx.verify().unwrap();
                    assert_eq!(&bincode::serialize(tx).unwrap(), wire);
                }
                for actor in 0..2 {
                    let amount = book.entitlement(actor);
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
                        70 + actor as u32,
                    );
                    let changed = [
                        w.env.market,
                        w.portfolios[actor],
                        w.tokens[actor],
                        w.env.vault,
                    ];
                    counts.record(w.deliver(tx, false, &changed, [1, 1, 0]));
                    counts.payouts += 1;
                    book.paid[actor] = amount;
                    book.check(&w);
                }
                let outcome = (book.paid, book.fees, w.env.token_amount(w.env.vault));
                if let Some(expected) = endpoint {
                    assert_eq!(outcome, expected);
                } else {
                    endpoint = Some(outcome);
                }
                counts.worlds += 1;
            }
        }
    }
    assert_eq!(counts.worlds, 108);
    assert_eq!((counts.permitted, counts.denied), (864, 216));
    assert_eq!(
        (counts.rollbacks, counts.fills, counts.payouts),
        (432, 216, 216)
    );
    eprintln!("INV-014 generated partial policy words: {counts:?}");
}
