//! INV-014: pending funding is not a retained trade-fee allowance. A real partial
//! fill, authority/policy return, maintenance collection and four closing routes
//! share an input ledger and exact transaction rollback. Public construction only.
//! A split close settles funding once and charges each retained fill's ceiling.
//! Inter-reduction accrual charges only the surviving position on the next close.

use super::*;

#[path = "inv_014_retained_funding_route_renewal.rs"]
mod retained_funding_route_renewal;

#[path = "inv_014_retained_multi_asset_funding.rs"]
mod retained_multi_asset_funding;

const MAINTENANCE: u64 = 307;
// Funding accrues exactly (prog#458): one slot at the 10_000 e9 cap moves 0.1 bp of notional,
// so funding can only exceed a close fee over a multi-slot window at low fee rates. These
// funded fixtures shadow the parent's 37/71 bps with 1/2 bps, use 9x the parent's 95-unit
// fill, and leave each funding window open for FUNDING_WINDOW slots (at most the 32-step
// canonical accrual path one call may catch up). Two windows stay below 100 slots, so the
// 1-bps circuit breaker (0.01 atom/slot at price 100) never moves the price.
const OPEN_CAP: u64 = 1;
const CLOSE_FRESH: u64 = 2;
const QUANTITY: i128 = 855 * POS_SCALE as i128;
// The hostile matcher fills 95/255 of the request.
const OPEN_REQUEST: i128 = QUANTITY / 95 * 255;
const FUNDING_RATE: u64 = 10_000;
const FUNDING_WINDOW: u64 = 24;
const OBSERVE_SLOT: u64 = 1;
const FIRST_CLOSE_SLOT: u64 = OBSERVE_SLOT + FUNDING_WINDOW;
const SECOND_OBSERVE_SLOT: u64 = FIRST_CLOSE_SLOT + 1;
const SECOND_CLOSE_SLOT: u64 = SECOND_OBSERVE_SLOT + FUNDING_WINDOW;
const FUNDING_SCALE: u128 = ADL_ONE * POS_SCALE;

fn funded_params(max_portfolio_assets: u16, maintenance: u64) -> V16CuMarketParams {
    V16CuMarketParams {
        max_portfolio_assets,
        trade_fee_base_bps: OPEN_CAP,
        maintenance_fee_per_slot: maintenance.into(),
        max_abs_funding_e9_per_slot: FUNDING_RATE,
        max_accrual_dt_slots: FUNDING_WINDOW,
        min_funding_lifetime_slots: FUNDING_WINDOW,
        // Margin is not under test; 10% keeps the 10x notional healthy throughout.
        maintenance_margin_bps: 1_000,
        initial_margin_bps: 1_000,
        max_price_move_bps_per_slot: 1,
        h_max: 1,
        ..V16CuMarketParams::default()
    }
}

/// A-scaled index move of one capped negative-premium window: exact here, so both sides'
/// single floors coincide (longs +, shorts -).
fn funding_index() -> i128 {
    let num =
        i128::from(FUNDING_RATE) * i128::from(FUNDING_WINDOW) * i128::from(PRICE) * ADL_ONE as i128;
    assert_eq!(num % percolator::FUNDING_DEN as i128, 0);
    num / percolator::FUNDING_DEN as i128
}

/// Settlement of one window: the payer is charged the ceiling of the exact transfer.
fn funding_debit(quantity: i128) -> u64 {
    let num = quantity.unsigned_abs() * funding_index() as u128;
    u64::try_from(num.div_ceil(FUNDING_SCALE)).unwrap()
}

/// Settlement of one window: the receiver is credited the floor of the exact transfer.
fn funding_credit(quantity: i128) -> u64 {
    let num = quantity.unsigned_abs() * funding_index() as u128;
    u64::try_from(num / FUNDING_SCALE).unwrap()
}

/// Catch the asset up over a whole funding window through a flat, publicly created observer.
/// One call may walk at most 32 canonical accrual steps, which a CPI trade cannot also afford
/// inside the parent's 500k CU envelope. The observer holds no leg, so the funding stays
/// pending on both traders' legs until a retained route settles it. The observer then closes.
fn accrue_funding_window(
    w: &mut World,
    slot: u64,
    assets: &[u16],
    nonce: u32,
    counts: &mut Counts,
) {
    let owner = Keypair::new();
    let portfolio = Keypair::new();
    send_raw_tx(
        &mut w.env.svm,
        &w.env.payer,
        system_instruction::transfer(&w.env.payer.pubkey(), &owner.pubkey(), 1_000_000),
        &[],
    )
    .unwrap();
    system_create_account_for_test(
        &mut w.env.svm,
        &w.env.payer,
        &portfolio,
        w.env.portfolio_account_len,
        w.env.program_id,
    );
    w.env
        .send(
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(w.env.market, false),
                AccountMeta::new(portfolio.pubkey(), false),
            ],
            &[&owner],
        )
        .unwrap();
    let before = w.env.market_state().1;
    let crank = Instruction {
        program_id: w.env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(w.env.payer.pubkey(), true),
            AccountMeta::new(w.env.market, false),
            AccountMeta::new(portfolio.pubkey(), false),
        ],
        data: ProgInstruction::PermissionlessCrank {
            now_slot: slot,
            observations: crank_observations_for_assets(assets),
        }
        .encode(),
    };
    let tx = w.sign_with_nonce(&[crank], nonce);
    counts.record(w.deliver(tx, false, &[w.env.market, portfolio.pubkey()], [1, 0, 0]));
    let after = w.env.market_state().1;
    for &asset in assets {
        let asset = usize::from(asset);
        assert_eq!(after.assets[asset].slot_last, slot);
        assert_eq!(
            after.assets[asset].f_long_num - before.assets[asset].f_long_num,
            funding_index()
        );
    }
    assert_eq!(
        (after.c_tot, after.insurance),
        (before.c_tot, before.insurance)
    );
    // The flat observer exits through the owner's public close; nothing else changes.
    w.env.close_portfolio_with_cu(&owner, portfolio.pubkey());
    assert_eq!(w.env.market_state().1.materialized_portfolio_count, 2);
    assert_eq!(w.env.market_state().1.c_tot, after.c_tot);
    assert_eq!(w.env.market_state().1.insurance, after.insurance);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CloseHistory {
    Full,
    SplitSameSlot,
    SplitWithAccrual,
}

struct Ledger {
    direction: i128,
    remaining: i128,
    trade_fees: u128,
    fee_slots: [u64; 2],
    maintenance_domains: [u128; 2],
    funding_debit: u64,
    funding_credit: u64,
    converted: bool,
    paid: [u64; 2],
    custody: [Account; 3],
}

impl Ledger {
    fn new(w: &World, direction: i128) -> Self {
        Self {
            direction,
            remaining: QUANTITY,
            trade_fees: fee(QUANTITY, OPEN_CAP),
            fee_slots: [0; 2],
            maintenance_domains: [0; 2],
            funding_debit: 0,
            funding_credit: 0,
            converted: false,
            paid: [0; 2],
            custody: [w.tokens[0], w.tokens[1], w.env.vault]
                .map(|key| w.env.svm.get_account(&key).unwrap()),
        }
    }

    fn winner(&self) -> usize {
        usize::from(self.direction < 0)
    }

    /// Settle one window on the position held before a size change (which drops any
    /// sub-atom remainder).
    fn settle(&mut self, quantity: i128) {
        self.funding_debit += funding_debit(quantity);
        self.funding_credit += funding_credit(quantity);
    }

    /// Conservative funding rounding: the loser pays at least what the winner receives.
    fn funding_dust(&self) -> u64 {
        self.funding_debit - self.funding_credit
    }

    fn reduce(&mut self, quantity: i128) {
        assert!(quantity > 0 && quantity <= self.remaining);
        self.remaining -= quantity;
        self.trade_fees += fee(quantity, OPEN_CAP);
    }

    fn collect(&mut self, actor: usize, slot: u64) {
        let amount = u128::from((slot - self.fee_slots[actor]) * MAINTENANCE);
        self.maintenance_domains[0] += amount / 2;
        self.maintenance_domains[1] += amount - amount / 2;
        self.fee_slots[actor] = slot;
    }

    fn capital(&self, actor: usize) -> u64 {
        PRINCIPAL[actor] + u64::from(actor == 0) * DEPOSIT
            - self.trade_fees as u64
            - self.fee_slots[actor] * MAINTENANCE
            - u64::from(actor != self.winner()) * self.funding_debit
            + u64::from(self.converted && actor == self.winner()) * self.funding_credit
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
                i128::from(!self.converted && actor == self.winner())
                    * i128::from(self.funding_credit),
                "funding PnL {actor}"
            );
            assert_eq!(p.fee_credits.get(), 0);
            assert_eq!(p.last_fee_slot.get(), self.fee_slots[actor]);
            if self.remaining == 0 {
                assert!(!has_active_leg_for_asset(p, 0));
            } else {
                assert_eq!(
                    active_leg_for_asset(p, 0).basis_pos_q,
                    self.direction * self.remaining * if actor == 0 { 1 } else { -1 }
                );
            }
        }
        let (cfg, group) = w.env.market_state();
        let domains = self
            .maintenance_domains
            .map(|amount| amount + self.trade_fees);
        assert_eq!(&group.insurance_domain_budget[..2], &domains);
        assert!(group.insurance_domain_budget[2..].iter().all(|v| *v == 0));
        assert_eq!(group.insurance, domains.iter().sum());
        assert_eq!(cfg.fee_redirect_to_market_0_bps, 0);
        assert_eq!(group.assets[0].effective_price, PRICE);
        let oi = self.remaining as u128;
        assert_eq!(group.assets[0].oi_eff_long_q, oi);
        assert_eq!(group.assets[0].oi_eff_short_q, oi);
        let vault = PRINCIPAL.iter().sum::<u64>() + DEPOSIT - self.paid.iter().sum::<u64>();
        assert_eq!(group.vault, u128::from(vault));
        assert_eq!(group.c_tot, accounts.iter().map(|p| p.capital.get()).sum());
        let residual = u128::from(!self.converted) * u128::from(self.funding_credit)
            + u128::from(self.funding_dust());
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

fn retained_funding_close_history(history: CloseHistory) {
    let split_close = history != CloseHistory::Full;
    let accrue_between = history == CloseHistory::SplitWithAccrual;
    let close_slot = if accrue_between {
        SECOND_CLOSE_SLOT
    } else {
        FIRST_CLOSE_SLOT
    };
    let payout_slot = close_slot + 1;
    let matcher = std::fs::read(hostile_matcher_program_path()).unwrap();
    let mut counts = Counts::default();
    assert_eq!(fee(QUANTITY, OPEN_CAP), 9);
    // 855 units * 100 * 10_000 * 24 / 1e9 = 20.52 atoms: payer 21, receiver 20.
    assert_eq!(
        (funding_debit(QUANTITY), funding_credit(QUANTITY)),
        (21, 20)
    );
    // Even the receiver's floored credit exceeds the fresh close fee it could be mistaken for.
    assert!(u128::from(funding_credit(QUANTITY)) > fee(QUANTITY, CLOSE_FRESH));
    assert!(u128::from(MAINTENANCE) > fee(QUANTITY, OPEN_CAP));
    let partial_quantity = if split_close { QUANTITY * 153 / 255 } else { 0 };
    let final_quantity = QUANTITY - partial_quantity;
    let close_budget = fee(partial_quantity, OPEN_CAP) + fee(final_quantity, OPEN_CAP);
    assert_eq!(
        close_budget,
        fee(QUANTITY, OPEN_CAP) + u128::from(split_close)
    );
    let total_debit =
        funding_debit(QUANTITY) + u64::from(accrue_between) * funding_debit(final_quantity);
    let total_funding =
        funding_credit(QUANTITY) + u64::from(accrue_between) * funding_credit(final_quantity);
    // The 342-unit residual window is 8.208 atoms: payer 9, receiver 8.
    assert_eq!(
        (total_debit, total_funding),
        if accrue_between { (30, 28) } else { (21, 20) }
    );
    for direction in [-1, 1] {
        let mut endpoint = None;
        for route in [
            Route::SingleCpi,
            Route::BatchCpi,
            Route::SingleNoCpi,
            Route::BatchNoCpi,
        ] {
            eprintln!(
                "retained funding close: {history:?}, direction={direction}, route={route:?}"
            );
            let mut w = World::with_params(&matcher, funded_params(1, MAINTENANCE));
            counts.record(w.env.configure_ewma_mark_with_cu(0, PRICE, 1, 0));
            counts.record(w.partial(95));
            let opening = w.bundle_instructions(
                Route::PartialCpi,
                direction * OPEN_REQUEST,
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
                    data: if split_close {
                        vec![11, 19, 153]
                    } else {
                        vec![11, 9, 0]
                    },
                }],
                2,
            );
            counts.record(w.deliver(tx, false, &[w.context], [0, 0, 1]));
            book.check(&w);

            let mut close = w.bundle_instructions(
                route,
                -direction * final_quantity,
                -direction * final_quantity,
                OPEN_CAP,
            )[1]
            .clone();
            if split_close {
                // Future epochs are signed request fields, never account writes.
                let mut request = ProgInstruction::decode(&close.data).unwrap();
                match &mut request {
                    ProgInstruction::TradeCpi {
                        account_a_position_epoch,
                        account_b_position_epoch,
                        ..
                    }
                    | ProgInstruction::BatchTradeCpi {
                        account_a_position_epoch,
                        account_b_position_epoch,
                        ..
                    }
                    | ProgInstruction::TradeNoCpi {
                        account_a_position_epoch,
                        account_b_position_epoch,
                        ..
                    }
                    | ProgInstruction::BatchTradeNoCpi {
                        account_a_position_epoch,
                        account_b_position_epoch,
                        ..
                    } => {
                        *account_a_position_epoch += 1;
                        *account_b_position_epoch += 1;
                    }
                    _ => unreachable!(),
                }
                close.data = request.encode();
            }
            let partial = split_close.then(|| {
                let ix = w.bundle_instructions(
                    Route::PartialCpi,
                    -direction * QUANTITY,
                    -direction * partial_quantity,
                    OPEN_CAP,
                )[1]
                .clone();
                [40, 41, 42].map(|nonce| w.sign_with_nonce(&[ix.clone()], nonce))
            });
            let partial_wires = partial
                .as_ref()
                .map(|txs| txs.each_ref().map(|tx| bincode::serialize(tx).unwrap()));
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
            probe(
                &mut w,
                partial.as_ref().map_or(&retained[2], |txs| &txs[1]),
                true,
                2,
                &mut counts,
            );
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
            // breaker keeps execution at 100; the following FUNDING_WINDOW slots
            // accrue capped negative-premium funding exactly.
            w.env.svm.warp_to_slot(OBSERVE_SLOT);
            counts.record(w.env.push_ewma_mark_with_cu(OBSERVE_SLOT, 98));
            book.check(&w);
            let crank = Instruction {
                program_id: w.env.program_id,
                accounts: vec![
                    AccountMeta::new_readonly(w.env.payer.pubkey(), true),
                    AccountMeta::new(w.env.market, false),
                    AccountMeta::new(w.portfolios[0], false),
                ],
                data: ProgInstruction::PermissionlessCrank {
                    now_slot: OBSERVE_SLOT,
                    observations: crank_observations(0),
                }
                .encode(),
            };
            let tx = w.sign_with_nonce(&[crank], 23);
            counts.record(w.deliver(tx, false, &[w.env.market, w.portfolios[0]], [1, 0, 0]));
            book.collect(0, OBSERVE_SLOT);
            book.check(&w);
            assert_eq!(w.env.market_state().1.assets[0].f_long_num, 0);
            w.env.svm.warp_to_slot(FIRST_CLOSE_SLOT);
            // Restore the quote for all four execution routes without erasing
            // the preceding window's funding checkpoint (EWMA 99 -> 100).
            counts.record(w.env.push_ewma_mark_with_cu(FIRST_CLOSE_SLOT, 101));
            book.check(&w);
            accrue_funding_window(&mut w, FIRST_CLOSE_SLOT, &[0], 50, &mut counts);
            book.check(&w);
            assert_eq!(w.env.market_state().0.mark_ewma_e6, PRICE);
            if let Some(partial) = &partial {
                counts.record(w.deliver_with_error(
                    partial[0].clone(),
                    Some((
                        2,
                        InstructionError::Custom(PercolatorError::InvalidInstruction as u32),
                    )),
                    &[],
                    [0, 0, 0],
                ));
                counts.rollbacks += 1;
                book.check(&w);
                counts.record(policy(&mut w, OPEN_CAP, 43));
                book.check(&w);
                counts.record(w.deliver(
                    partial[1].clone(),
                    false,
                    &[w.env.market, w.portfolios[0], w.portfolios[1], w.context],
                    [1, 0, 1],
                ));
                counts.fills += 1;
                book.settle(book.remaining);
                book.reduce(partial_quantity);
                book.collect(0, FIRST_CLOSE_SLOT);
                book.collect(1, FIRST_CLOSE_SLOT);
                book.check(&w);
                assert_eq!(book.trade_fees, 9 + 6);
                let fill =
                    read_matcher_return(&w.env.svm.get_account(&w.context).unwrap().data).unwrap();
                assert_eq!(
                    (fill.exec_size, fill.exec_price_e6),
                    (-direction * partial_quantity, PRICE)
                );
                assert_ne!(fill.flags & FLAG_PARTIAL_OK, 0);
                // One funding epoch per funded canonical accrual slot.
                assert_eq!(w.env.market_state().1.funding_epoch, FUNDING_WINDOW);
                assert_eq!(
                    w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                    epochs.map(|e| e + 1)
                );
                counts.record(w.deliver_with_error(
                    partial[2].clone(),
                    Some((
                        2,
                        InstructionError::Custom(PercolatorError::EngineStale as u32),
                    )),
                    &[],
                    [0, 0, 0],
                ));
                counts.rollbacks += 1;
                book.check(&w);
                // Only the matcher mode changes; the retained residual stays byte-identical.
                let reset = w.sign_with_nonce(
                    &[Instruction {
                        program_id: w.matcher,
                        accounts: vec![
                            AccountMeta::new_readonly(w.owners[1].pubkey(), true),
                            AccountMeta::new(w.context, false),
                        ],
                        data: vec![11, 9, 0],
                    }],
                    44,
                );
                counts.record(w.deliver(reset, false, &[w.context], [0, 0, 1]));
                book.check(&w);
                counts.record(policy(&mut w, CLOSE_FRESH, 45));
                book.check(&w);
            }
            if accrue_between {
                // Checkpoint the zero-rate interval, then leave a fresh negative
                // window pending against only the 342 surviving units. Neither
                // the consumed partial nor the pre-signed residual is rewritten.
                assert_eq!(book.remaining, 342 * POS_SCALE as i128);
                w.env.svm.warp_to_slot(SECOND_OBSERVE_SLOT);
                counts.record(w.env.push_ewma_mark_with_cu(SECOND_OBSERVE_SLOT, 98));
                book.check(&w);
                let crank = w.sign_with_nonce(
                    &[Instruction {
                        program_id: w.env.program_id,
                        accounts: vec![
                            AccountMeta::new_readonly(w.env.payer.pubkey(), true),
                            AccountMeta::new(w.env.market, false),
                            AccountMeta::new(w.portfolios[0], false),
                        ],
                        data: ProgInstruction::PermissionlessCrank {
                            now_slot: SECOND_OBSERVE_SLOT,
                            observations: crank_observations(0),
                        }
                        .encode(),
                    }],
                    46,
                );
                counts.record(w.deliver(crank, false, &[w.env.market, w.portfolios[0]], [1, 0, 0]));
                book.collect(0, SECOND_OBSERVE_SLOT);
                book.check(&w);
                assert_eq!(w.env.market_state().1.assets[0].f_long_num, funding_index());
                // One funding epoch per funded canonical accrual slot.
                assert_eq!(w.env.market_state().1.funding_epoch, FUNDING_WINDOW);
                w.env.svm.warp_to_slot(close_slot);
                counts.record(w.env.push_ewma_mark_with_cu(close_slot, 101));
                book.check(&w);
                accrue_funding_window(&mut w, close_slot, &[0], 51, &mut counts);
                book.check(&w);
                assert_eq!(w.env.market_state().0.mark_ewma_e6, PRICE);
                assert_eq!(
                    (book.funding_debit, book.funding_credit),
                    (funding_debit(QUANTITY), funding_credit(QUANTITY))
                );
            }
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
            assert_eq!(
                w.env.control_sequences(0).trade_fee,
                initial.trade_fee + 2 + 2 * u64::from(split_close)
            );
            // The close pays its own signed fee; the obsolete policy suffix
            // rolls back this transaction while preserving any committed partial.
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
            if !split_close || accrue_between {
                book.settle(book.remaining);
            }
            book.reduce(final_quantity);
            book.collect(0, close_slot);
            book.collect(1, close_slot);
            book.check(&w);
            assert_eq!(book.trade_fees, fee(QUANTITY, OPEN_CAP) + close_budget);
            assert_eq!(
                (book.funding_debit, book.funding_credit),
                (total_debit, total_funding)
            );
            let (_, group) = w.env.market_state();
            let intervals = 1 + u64::from(accrue_between);
            assert_eq!(
                group.assets[0].f_long_num,
                funding_index() * i128::from(intervals)
            );
            assert_eq!(
                group.assets[0].f_short_num,
                -funding_index() * i128::from(intervals)
            );
            assert_eq!(group.funding_epoch, intervals * FUNDING_WINDOW);
            assert_eq!(
                w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                epochs.map(|e| e + 1 + u64::from(split_close))
            );
            assert_eq!(
                w.env.market_state().0.matcher_req_seq,
                requests + u64::from(route.cpi()) + u64::from(split_close)
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
                    (-direction * final_quantity, PRICE)
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
            if let Some(partial) = &partial {
                for (tx, wire) in partial.iter().zip(partial_wires.unwrap()) {
                    tx.verify().unwrap();
                    assert_eq!(bincode::serialize(tx).unwrap(), wire);
                }
            }
            w.env.svm.warp_to_slot(payout_slot);
            for actor in 0..2 {
                let tx = w.sign_with_nonce(&[collect(&w, actor, payout_slot)], 30 + actor as u32);
                counts.record(w.deliver(
                    tx,
                    false,
                    &[w.env.market, w.portfolios[actor]],
                    [1, 0, 0],
                ));
                book.collect(actor, payout_slot);
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
                        now_slot: payout_slot,
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
                        .convert_released_pnl_ix(w.portfolios[winner], total_funding.into())
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
            // Only insurance (both sides' trade fees plus collected maintenance) and the
            // conservative funding dust remain; CloseSlab sweeps the dust once c_tot == 0.
            let maintenance = book.maintenance_domains.iter().sum::<u128>();
            assert_eq!(
                u128::from(outcome.1),
                2 * book.trade_fees + maintenance + u128::from(book.funding_dust())
            );
            assert_eq!(book.funding_dust(), total_debit - total_funding);
            if let Some(expected) = endpoint {
                assert_eq!(outcome, expected);
            } else {
                endpoint = Some(outcome);
            }
            counts.worlds += 1;
        }
    }
    assert_eq!(
        (counts.worlds, counts.rollbacks, counts.fills),
        (
            8,
            24 + 16 * usize::from(split_close),
            16 + 8 * usize::from(split_close)
        )
    );
    assert_eq!(counts.payouts, 16);
    assert_eq!(counts.permitted, 8);
    eprintln!("INV-014 retained funding collection: {history:?}, {counts:?}");
}

#[test]
fn v16_retained_partial_close_separates_funding_and_maintenance_from_fee_consent() {
    retained_funding_close_history(CloseHistory::Full);
}

#[test]
fn v16_retained_split_close_bounds_fees_and_collects_funding_once_across_routes() {
    retained_funding_close_history(CloseHistory::SplitSameSlot);
}

#[test]
fn v16_retained_residual_bounds_fees_after_inter_reduction_funding_accrual() {
    retained_funding_close_history(CloseHistory::SplitWithAccrual);
}
