//! INV-051 / issue #449: packed same-side reductions leave no exposure that a side counter does
//! not cover, through the deployed BPF program.
//!
//! The attacker owns every portfolio on both sides of one asset. One owner key signs for all of
//! them. After a unilateral RebalanceReduce lowers `a_long`, the attacker packs as many cheap
//! same-side reducing instructions as one transaction allows: 1232-byte packet, 1.4M CU.
//!
//! Before the engine fix each such instruction could leave up to one raw unit of exposure above
//! the side counter, and the amount added up over a history. The engine now keeps each side's
//! raw-basis sum and derives the side's A index from it after every change, so two measurements
//! hold in every state, whatever came before. Per side, over current-epoch legs:
//!   G_num = sum(raw * A) - OI * ADL_ONE          in (-sum(raw), 0]
//!   D     = sum(ceil(raw * A / ADL_ONE)) - OI    in [0, legs)
//! The first says the legs' exact exposure never exceeds the counter and is short of it by less
//! than `sum(raw) / ADL_ONE`; the second that only the legs' own rounding separates their
//! quantities from the counter. Pinned here on production BPF, after every packed transaction:
//!   * both ranges, the engine's raw-basis sums and its derived A indices;
//!   * the maximum such instructions per transaction (packet-bound, not CU-bound);
//!   * that a single BatchTradeNoCpi ix cannot reduce more than one long leg per asset.

use super::*;

const PRICE: u64 = POS_SCALE as u64; // 1 raw unit of notional = 1 quote atom (progress gate)
const FEE_BPS: u64 = 10;
const N_LONGS: usize = 4;
const OPEN_BASE: u128 = 1_000 * POS_SCALE;
const CAPITAL: u128 = 4_000 * POS_SCALE;
const PACKET_BYTES: usize = 1232;

struct Book {
    env: V16CuEnv,
    owner: Keypair,
    longs: Vec<Pubkey>,
    /// shorts[0] = ADL shaper (unilateral reducer), shorts[1] = bilateral reducer
    shorts: Vec<Pubkey>,
    /// position_epoch bump per instruction kind (measured), and the per-tx epoch shadow
    rebalance_bump: u64,
    trade_bump: u64,
    epochs: std::cell::RefCell<std::collections::HashMap<Pubkey, u64>>,
}

impl Book {
    fn reset_epochs(&self) {
        let mut e = self.epochs.borrow_mut();
        e.clear();
        for p in self.all() {
            e.insert(p, self.env.portfolio_position_epoch(p));
        }
    }

    fn take_epoch(&self, p: Pubkey, bump: u64) -> u64 {
        let mut e = self.epochs.borrow_mut();
        let cur = e[&p];
        e.insert(p, cur + bump);
        cur
    }

    fn all(&self) -> Vec<Pubkey> {
        self.longs
            .iter()
            .chain(self.shorts.iter())
            .copied()
            .collect()
    }

    fn leg(&self, p: Pubkey) -> Option<percolator::PortfolioLegV16> {
        self.env
            .portfolio_state(p)
            .legs
            .iter()
            .filter_map(|l| l.try_to_runtime().ok())
            .find(|l| l.active && l.asset_index == 0)
    }

    fn eff(&self, p: Pubkey) -> u128 {
        let group = self.env.market_state().1;
        self.leg(p)
            .map(|l| reference_current_epoch_effective_abs(&group, l))
            .unwrap_or(0)
    }

    /// [long, short]: (G_num, D), after checking each side against the engine's own totals and
    /// against the two ranges the side rule guarantees in every state.
    fn census(&self) -> [(i128, i128); 2] {
        let group = self.env.market_state().1;
        let a = group.assets[0];
        let mut exact = [0u128; 2];
        let mut ceil = [0u128; 2];
        let mut raw_sum = [0u128; 2];
        let mut legs = [0i128; 2];
        for p in self.all() {
            if let Some(leg) = self.leg(p) {
                let (cur, ep, s) = match leg.side {
                    SideV16::Long => (a.a_long, a.epoch_long, 0),
                    SideV16::Short => (a.a_short, a.epoch_short, 1),
                };
                if leg.epoch_snap != ep {
                    continue;
                }
                assert_eq!(leg.a_basis, ADL_ONE);
                let raw = leg.basis_pos_q.unsigned_abs();
                exact[s] += raw * cur;
                ceil[s] += raw * cur / ADL_ONE + u128::from(raw * cur % ADL_ONE != 0);
                raw_sum[s] += raw;
                legs[s] += 1;
            }
        }
        let oi = [a.oi_eff_long_q, a.oi_eff_short_q];
        let index = [a.a_long, a.a_short];
        let engine_raw_sum = [a.raw_basis_sum_long_q, a.raw_basis_sum_short_q];
        [0, 1].map(|s| {
            // The engine's raw-basis sum is the legs' sum, and its A index is what the two
            // totals imply.
            assert_eq!(engine_raw_sum[s], raw_sum[s], "side {s} raw-basis sum");
            assert!(oi[s] != 0 && oi[s] <= raw_sum[s], "side {s} totals");
            let derived = if oi[s] == raw_sum[s] {
                ADL_ONE
            } else {
                (ADL_ONE * oi[s] / raw_sum[s]).max(1)
            };
            assert_eq!(index[s], derived, "side {s} A index");
            let g_num = exact[s] as i128 - (oi[s] * ADL_ONE) as i128;
            let d = ceil[s] as i128 - oi[s] as i128;
            // Exposure never exceeds the side counter and is short of it by less than
            // `raw_sum / ADL_ONE`.
            assert!(
                g_num <= 0 && -g_num < raw_sum[s] as i128,
                "side {s}: exposure numerator {g_num} outside (-{}, 0]",
                raw_sum[s]
            );
            // The legs' rounded-up quantities exceed the counter by less than one per leg.
            assert!(
                (0..legs[s]).contains(&d),
                "side {s}: rounded-up surplus {d} outside [0, {})",
                legs[s]
            );
            (g_num, d)
        })
    }

    fn capital_and_pnl(&self) -> i128 {
        self.all()
            .iter()
            .map(|&p| {
                let s = self.env.portfolio_state(p);
                s.capital.get() as i128 + s.pnl.get()
            })
            .sum()
    }

    fn rebalance_ix(&self, p: Pubkey, q: u128) -> Instruction {
        Instruction {
            program_id: self.env.program_id,
            accounts: vec![
                AccountMeta::new(self.owner.pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(p, false),
            ],
            data: ProgInstruction::RebalanceReduce {
                portfolio_id: self.env.portfolio_id(p),
                position_epoch: self.take_epoch(p, self.rebalance_bump),
                asset_index: 0,
                reduce_q: q,
            }
            .encode(),
        }
    }

    /// `buyer` buys `q` from `seller` (short reducer buying back from a long reduces both).
    fn trade_ix(&self, buyer: Pubkey, seller: Pubkey, q: u128) -> Instruction {
        let mut data = self
            .env
            .trade_no_cpi_ix(buyer, seller, 0, q as i128, PRICE, FEE_BPS);
        if let ProgInstruction::TradeNoCpi {
            account_a_position_epoch,
            account_b_position_epoch,
            ..
        } = &mut data
        {
            *account_a_position_epoch = self.take_epoch(buyer, self.trade_bump);
            *account_b_position_epoch = self.take_epoch(seller, self.trade_bump);
        }
        Instruction {
            program_id: self.env.program_id,
            accounts: vec![
                AccountMeta::new(self.owner.pubkey(), true),
                AccountMeta::new(self.owner.pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(buyer, false),
                AccountMeta::new(seller, false),
            ],
            data: data.encode(),
        }
    }

    fn batch_ix(&self, buyer: Pubkey, seller: Pubkey, legs: usize) -> Instruction {
        let market_id = self.env.asset_market_id(0);
        Instruction {
            program_id: self.env.program_id,
            accounts: vec![
                AccountMeta::new(self.owner.pubkey(), true),
                AccountMeta::new(self.owner.pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(buyer, false),
                AccountMeta::new(seller, false),
            ],
            data: self
                .env
                .batch_trade_no_cpi_ix(
                    buyer,
                    seller,
                    (0..legs)
                        .map(|_| BatchTradeLeg {
                            asset_index: 0,
                            market_id,
                            size_q: 1,
                            exec_price: PRICE,
                            fee_bps: FEE_BPS,
                        })
                        .collect(),
                )
                .encode(),
        }
    }

    fn tx(&mut self, ixs: &[Instruction]) -> Transaction {
        self.env.svm.expire_blockhash();
        let mut all = vec![heap_ix(), cu_ix()];
        all.extend_from_slice(ixs);
        Transaction::new_signed_with_payer(
            &all,
            Some(&self.env.payer.pubkey()),
            &[&self.env.payer, &self.owner],
            self.env.svm.latest_blockhash(),
        )
    }
}

fn tx_bytes(tx: &Transaction) -> usize {
    bincode::serialize(tx).unwrap().len()
}

fn open_book() -> Book {
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: PRICE,
        trade_fee_base_bps: FEE_BPS,
        ..V16CuMarketParams::default()
    });
    env.configure_auth_mark_with_cu(0, PRICE);
    let owner = Keypair::new();
    let longs: Vec<_> = (0..N_LONGS).map(|_| env.create_portfolio(&owner)).collect();
    let shorts: Vec<_> = (0..2).map(|_| env.create_portfolio(&owner)).collect();
    for &p in longs.iter() {
        env.deposit(&owner, p, CAPITAL);
    }
    for &p in shorts.iter() {
        env.deposit(&owner, p, CAPITAL * N_LONGS as u128);
    }
    for (i, &l) in longs.iter().enumerate() {
        let q = OPEN_BASE + 7_777 * (i as u128 + 1);
        env.trade_asset_with_cu(
            0,
            &owner,
            l,
            &owner,
            shorts[0],
            (q / 2) as i128,
            PRICE,
            FEE_BPS,
        );
        env.trade_asset_with_cu(
            0,
            &owner,
            l,
            &owner,
            shorts[1],
            (q - q / 2) as i128,
            PRICE,
            FEE_BPS,
        );
    }
    let mut book = Book {
        env,
        owner,
        longs,
        shorts,
        rebalance_bump: 0,
        trade_bump: 0,
        epochs: Default::default(),
    };
    // the one unilateral ADL that moves a_long off ADL_ONE; measure the epoch bump rules
    let (s0p, s1p, l0) = (book.shorts[0], book.shorts[1], book.longs[0]);
    let e0 = book.env.portfolio_position_epoch(s0p);
    let s0 = book.eff(s0p);
    book.env
        .rebalance_reduce_with_cu(&book.owner, s0p, 0, s0 * 2 / 3);
    book.rebalance_bump = book.env.portfolio_position_epoch(s0p) - e0;
    let e1 = book.env.portfolio_position_epoch(s1p);
    book.env
        .trade_asset_with_cu(0, &book.owner, s1p, &book.owner, l0, 1, PRICE, FEE_BPS);
    book.trade_bump = book.env.portfolio_position_epoch(s1p) - e1;
    eprintln!(
        "#449 position_epoch bump: rebalance={} trade={}",
        book.rebalance_bump, book.trade_bump
    );
    let a = book.env.market_state().1.assets[0];
    assert!(a.a_long < ADL_ONE && a.a_short == ADL_ONE);
    book
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Pack {
    /// [RebalanceReduce(shaper, 1), TradeNoCpi(reducer buys 1 from long i)] repeated: one-sided
    /// long phantom; every trade is a same-side (long) reducing op.
    ShapeAndBilateral,
    /// all TradeNoCpi(reducer buys 1 from long i): cheapest one-sided harvest after one ADL
    BilateralOnly,
    /// RebalanceReduce only, alternating long i / shaper: fee-free, harvests both sides
    RebalancePingPong,
}

fn build(book: &Book, pack: Pack, ops: usize, round: usize) -> (Vec<Instruction>, [usize; 2]) {
    book.reset_epochs();
    let mut ixs = Vec::new();
    let mut reducing = [0usize; 2];
    let (shaper, reducer) = (book.shorts[0], book.shorts[1]);
    let mut k = 0;
    while ixs.len() < ops {
        let long = book.longs[(round + k) % N_LONGS];
        match pack {
            Pack::ShapeAndBilateral => {
                if k % 2 == 0 {
                    ixs.push(book.rebalance_ix(shaper, 1));
                    reducing[1] += 1;
                } else {
                    ixs.push(book.trade_ix(reducer, long, 1));
                    reducing[0] += 1;
                    reducing[1] += 1;
                }
            }
            Pack::BilateralOnly => {
                ixs.push(book.trade_ix(reducer, long, 1));
                reducing[0] += 1;
                reducing[1] += 1;
            }
            Pack::RebalancePingPong => {
                if k % 2 == 0 {
                    ixs.push(book.rebalance_ix(long, 1));
                    reducing[0] += 1;
                } else {
                    ixs.push(book.rebalance_ix(shaper, 1));
                    reducing[1] += 1;
                }
            }
        }
        k += 1;
    }
    (ixs, reducing)
}

#[test]
fn v16_program_issue_449_packed_reductions_leave_no_uncovered_exposure() {
    for pack in [
        Pack::ShapeAndBilateral,
        Pack::BilateralOnly,
        Pack::RebalancePingPong,
    ] {
        let mut book = open_book();
        // largest op count whose signed transaction fits one packet
        let mut packet_ops = 0;
        while {
            let (ixs, _) = build(&book, pack, packet_ops + 1, 0);
            let tx = book.tx(&ixs);
            tx_bytes(&tx) <= PACKET_BYTES
        } {
            packet_ops += 1;
        }
        // then the largest that also fits 1.4M CU (failed transactions roll back atomically)
        let mut max_ops = packet_ops;
        while max_ops > 0 {
            let before = book.env.svm.get_account(&book.env.market).unwrap();
            let tx = book.tx(&build(&book, pack, max_ops, 0).0);
            if book.env.svm.simulate_transaction(tx.into()).is_ok() {
                break;
            }
            assert_eq!(book.env.svm.get_account(&book.env.market).unwrap(), before);
            max_ops -= 1;
        }
        assert!(max_ops >= 2, "{pack:?}: no room for a packed transaction");
        assert!(
            max_ops <= 19,
            "{pack:?}: packing changed ({max_ops} ops/tx); review the #449 measurements"
        );
        let probe = book.tx(&build(&book, pack, max_ops, 0).0);
        let bytes = tx_bytes(&probe);
        let sigs = probe.message.header.num_required_signatures as u64;
        let binding = if max_ops < packet_ops { "CU" } else { "packet" };

        let rounds = 40;
        let mut max_cu = 0u64;
        let mut fee_atoms = 0i128;
        let mut reducing_ops = [0usize; 2];
        // `census` holds both sides inside their ranges before the first packed transaction...
        let start = book.census();
        let mut widest = start;
        for r in 0..rounds {
            let value_before = book.capital_and_pnl();
            let (ixs, reducing) = build(&book, pack, max_ops, r);
            let tx = book.tx(&ixs);
            assert!(tx_bytes(&tx) <= PACKET_BYTES);
            let meta = book
                .env
                .svm
                .send_transaction(tx)
                .unwrap_or_else(|e| panic!("{pack:?} round {r}: {:?}", e.err));
            let cu = meta.compute_units_consumed;
            assert!(cu <= 1_400_000);
            max_cu = max_cu.max(cu);
            // ...and after every one of them, however many reductions it packed.
            let after = book.census();
            fee_atoms += value_before - book.capital_and_pnl();
            for s in 0..2 {
                reducing_ops[s] += reducing[s];
                widest[s] = (widest[s].0.min(after[s].0), widest[s].1.max(after[s].1));
            }
            let a = book.env.market_state().1.assets[0];
            assert!(
                a.oi_eff_long_q > 0 && a.oi_eff_short_q > 0,
                "book must stay live"
            );
        }
        let end = book.census();
        eprintln!(
            "#449 {pack:?}: ops/tx={max_ops} ({binding}-bound; packet allows {packet_ops}) \
             bytes={bytes} sigs={sigs} max_cu={max_cu} protocol_fee/tx={:.1} atoms \
             reducing_ops={reducing_ops:?} exposure numerator start={:?} end={:?} lowest={:?} \
             (always in (-sum(raw), 0]) rounded-up surplus end={:?} highest={:?}",
            fee_atoms as f64 / rounds as f64,
            [start[0].0, start[1].0],
            [end[0].0, end[1].0],
            [widest[0].0, widest[1].0],
            [end[0].1, end[1].1],
            [widest[0].1, widest[1].1],
        );
        assert!(
            reducing_ops[0] + reducing_ops[1] >= rounds * max_ops,
            "{pack:?}: every packed instruction is a same-side reduction on some side"
        );
    }
}

/// A single BatchTradeNoCpi instruction cannot touch the same long leg more than once on one
/// asset: batch legs must name distinct assets. The side totals stay exact either way (`census`
/// checks them after each instruction).
#[test]
fn v16_program_issue_449_batch_cannot_multi_reduce_one_asset_leg() {
    let mut book = open_book();
    let (reducer, long) = (book.shorts[1], book.longs[0]);
    let before = book.census();
    book.reset_epochs();
    let ix = book.batch_ix(reducer, long, 2);
    let tx = book.tx(&[ix]);
    let result = book.env.svm.send_transaction(tx);
    let after = book.census();
    match result {
        Err(e) => {
            eprintln!("#449 duplicate-asset batch rejected: {:?}", e.err);
            assert_eq!(before, after);
        }
        Ok(_) => panic!("a batch naming one asset twice was accepted: {before:?} -> {after:?}"),
    }
    book.reset_epochs();
    let long_before = book.eff(long);
    let ix = book.batch_ix(reducer, long, 1);
    let tx = book.tx(&[ix]);
    book.env
        .svm
        .send_transaction(tx)
        .expect("one-leg batch reduce");
    // The one-leg batch is a single same-side reduction; `census` holds its ranges after it.
    book.census();
    assert!(
        book.eff(long) < long_before,
        "the one-leg batch reduced the long leg"
    );
}
