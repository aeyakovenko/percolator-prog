//! INV-014 / row 411: retained, heterogeneous fee and price budgets compose across
//! policy detours and single/batch transports. The parent supplies public funding,
//! signing and complete-account rollback helpers, not this independent oracle.
//! Manual marks isolate base fees; matcher print slippage is observed separately.

use super::*;
use percolator_prog::matcher_abi::{read_matcher_return, MATCHER_RETURN_BYTES};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    SingleCpi,
    BatchCpi,
    SingleDirect,
    BatchDirect,
}

impl Route {
    const ALL: [Self; 4] = [
        Self::SingleCpi,
        Self::BatchCpi,
        Self::SingleDirect,
        Self::BatchDirect,
    ];

    fn cpi(self) -> bool {
        matches!(self, Self::SingleCpi | Self::BatchCpi)
    }

    fn batch(self) -> bool {
        matches!(self, Self::BatchCpi | Self::BatchDirect)
    }
}

#[derive(Clone, Copy, Debug)]
struct Leg {
    asset: u16,
    size: i128,
    bps: u64,
}

impl Leg {
    fn price(self) -> u64 {
        if self.size > 0 {
            105
        } else {
            95
        }
    }

    // No production fee, quote or slippage helper participates in the oracle.
    fn amounts(self, price: u64, bps: u64) -> [u128; 4] {
        let q = self.size.unsigned_abs();
        let notional = (q * u128::from(PRICE)).div_ceil(POS_SCALE);
        let print = q * u128::from(price);
        let adverse = if self.size > 0 {
            price.saturating_sub(PRICE)
        } else {
            PRICE.saturating_sub(price)
        };
        [
            if self.size > 0 {
                print.div_ceil(POS_SCALE)
            } else {
                0
            },
            if self.size < 0 { print / POS_SCALE } else { 0 },
            (q * u128::from(adverse)).div_ceil(POS_SCALE),
            (notional * u128::from(bps)).div_ceil(10_000),
        ]
    }
}

fn budget(legs: &[Leg], policy: Option<u64>) -> [u128; 4] {
    let mut total = [0; 4];
    for leg in legs {
        for (sum, amount) in total
            .iter_mut()
            .zip(leg.amounts(leg.price(), policy.unwrap_or(leg.bps)))
        {
            *sum += amount;
        }
    }
    total
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Ledger {
    sizes: [i128; 3],
    fees: [u128; 3],
    amounts: [u128; 4],
    deposited: u64,
    instructions: u64,
}

impl Ledger {
    fn commit(&mut self, legs: &[Leg], deposit: u64, observed: [u128; 4]) {
        assert_eq!(observed, budget(legs, None));
        for leg in legs {
            self.sizes[usize::from(leg.asset)] += leg.size;
            self.fees[usize::from(leg.asset)] += leg.amounts(leg.price(), leg.bps)[3];
        }
        for (sum, amount) in self.amounts.iter_mut().zip(observed) {
            *sum += amount;
        }
        self.deposited += deposit;
        self.instructions += 1;
    }

    fn check(&self, w: &World, signed: [u128; 4], initial_epochs: [u64; 2], requests: u64) {
        assert!(self.amounts[0] <= signed[0], "cumulative buy quote ceiling");
        assert!(
            self.amounts[1] <= signed[1],
            "only the filled sell budget is consumed"
        );
        assert!(
            self.amounts[2] <= signed[2],
            "cumulative adverse slippage ceiling"
        );
        assert!(
            self.amounts[3] <= signed[3],
            "cumulative signed fee ceiling"
        );
        let portfolios = w.portfolios.map(|key| w.env.portfolio_state(key));
        let vault = DEPOSITS.iter().sum::<u64>() + self.deposited;
        let (config, market) = w.env.market_state();
        assert_eq!(config.fee_redirect_to_market_0_bps, 0);
        assert_eq!(config.matcher_req_seq, requests);
        assert_eq!(market.insurance, 2 * self.amounts[3]);
        assert_eq!(market.c_tot, u128::from(vault) - 2 * self.amounts[3]);
        assert_eq!(market.vault, u128::from(vault));
        assert_eq!(w.env.token_amount(w.env.vault), vault);
        assert_eq!(w.env.token_amount(w.sources[0]), PREFIX - self.deposited);
        assert_eq!(w.env.token_amount(w.sources[1]), 0);
        for actor in 0..2 {
            let p = &portfolios[actor];
            assert_eq!(p.owner, w.owners[actor].pubkey().to_bytes());
            assert_eq!(
                p.capital.get(),
                u128::from(DEPOSITS[actor] + if actor == 0 { self.deposited } else { 0 })
                    - self.amounts[3]
            );
            assert_eq!(p.pnl.get(), 0, "manual mark: fee cannot hide in PnL");
            assert_eq!(
                w.env.portfolio_position_epoch(w.portfolios[actor]),
                initial_epochs[actor] + self.instructions
            );
            for (asset, size) in self.sizes.iter().enumerate() {
                if *size == 0 {
                    assert!(!has_active_leg_for_asset(p, asset));
                } else {
                    assert_eq!(
                        active_leg_for_asset(p, asset).basis_pos_q,
                        if actor == 0 { *size } else { -*size }
                    );
                }
            }
        }
        for asset in 0..3 {
            assert_eq!(market.assets[asset].effective_price, PRICE);
            assert_eq!(
                market.assets[asset].oi_eff_long_q,
                self.sizes[asset].unsigned_abs()
            );
            assert_eq!(
                market.assets[asset].oi_eff_short_q,
                self.sizes[asset].unsigned_abs()
            );
            assert_eq!(
                &market.insurance_domain_budget[2 * asset..2 * asset + 2],
                &[self.fees[asset]; 2]
            );
        }
        assert!(market.insurance_domain_budget[6..]
            .iter()
            .all(|amount| *amount == 0));
        assert_market_stock_census(
            "retained policy route budgets",
            &market,
            &w.env.svm.get_account(&w.env.market).unwrap().data,
            &portfolios,
            vault.into(),
        )
        .unwrap();
        assert_reservation_encumbrance_census(
            "retained policy route budgets",
            &market,
            &portfolios,
        )
        .unwrap();
        let mint = Mint::unpack(&w.env.svm.get_account(&w.env.mint).unwrap().data).unwrap();
        assert_eq!(mint.supply, DEPOSITS.iter().sum::<u64>() + PREFIX);
        assert_eq!(mint.mint_authority, COption::None);
    }
}

#[derive(Clone, Copy)]
enum Bound {
    Exact,
    FeeShort,
    SlippageShort,
}

fn instruction(w: &World, route: Route, legs: &[Leg], epoch: u64, bound: Bound) -> Instruction {
    let [a, b] = w.portfolios;
    let limits = budget(legs, None);
    let mut trade = if route.batch() {
        if route.cpi() {
            w.env.batch_trade_cpi_ix_with_caps(
                a,
                b,
                legs.iter()
                    .map(|leg| BatchTradeCpiLeg {
                        asset_index: leg.asset,
                        market_id: w.env.asset_market_id(leg.asset),
                        size_q: leg.size,
                        fee_bps: u64::from(LP_CAP_BPS),
                        limit_price: leg.price(),
                    })
                    .collect(),
                limits[2] - u128::from(matches!(bound, Bound::SlippageShort)),
                limits[3] - u128::from(matches!(bound, Bound::FeeShort)),
            )
        } else {
            w.env.batch_trade_no_cpi_ix(
                a,
                b,
                legs.iter()
                    .map(|leg| BatchTradeLeg {
                        asset_index: leg.asset,
                        market_id: w.env.asset_market_id(leg.asset),
                        size_q: leg.size,
                        exec_price: leg.price(),
                        fee_bps: leg.bps,
                    })
                    .collect(),
            )
        }
    } else {
        assert_eq!(legs.len(), 1);
        let leg = legs[0];
        if route.cpi() {
            let price = if matches!(bound, Bound::SlippageShort) {
                if leg.size > 0 {
                    leg.price() - 1
                } else {
                    leg.price() + 1
                }
            } else {
                leg.price()
            };
            w.env
                .trade_cpi_ix(a, b, leg.asset, leg.size, leg.bps, price)
        } else {
            w.env
                .trade_no_cpi_ix(a, b, leg.asset, leg.size, leg.price(), leg.bps)
        }
    };
    // Future epochs are signed message fields, never account-image edits.
    match &mut trade {
        ProgInstruction::TradeCpi {
            account_a_position_epoch,
            account_b_position_epoch,
            ..
        }
        | ProgInstruction::TradeNoCpi {
            account_a_position_epoch,
            account_b_position_epoch,
            ..
        }
        | ProgInstruction::BatchTradeCpi {
            account_a_position_epoch,
            account_b_position_epoch,
            ..
        }
        | ProgInstruction::BatchTradeNoCpi {
            account_a_position_epoch,
            account_b_position_epoch,
            ..
        } => {
            *account_a_position_epoch += epoch;
            *account_b_position_epoch += epoch;
        }
        _ => unreachable!(),
    }
    let accounts = if route.cpi() {
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
        data: trade.encode(),
    }
}

fn funded(w: &World, trade: Instruction, deposit: u64, sequence: u64) -> Vec<Instruction> {
    if deposit == 0 {
        return vec![trade];
    }
    let mut request = w.env.deposit_ix(w.portfolios[0], deposit.into());
    if let ProgInstruction::Deposit {
        expected_sequence, ..
    } = &mut request
    {
        *expected_sequence += sequence;
    }
    vec![
        Instruction {
            program_id: w.env.program_id,
            accounts: vec![
                AccountMeta::new(w.owners[0].pubkey(), true),
                AccountMeta::new(w.env.market, false),
                AccountMeta::new(w.portfolios[0], false),
                AccountMeta::new(w.sources[0], false),
                AccountMeta::new(w.env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: request.encode(),
        },
        trade,
    ]
}

fn observe(
    w: &World,
    route: Route,
    legs: &[Leg],
    meta: &litesvm::types::TransactionMetadata,
) -> [u128; 4] {
    if !route.cpi() {
        return budget(legs, None);
    }
    let bytes = if route.batch() {
        assert_eq!(meta.return_data.program_id, w.matcher);
        meta.return_data.data.clone()
    } else {
        w.env.svm.get_account(&w.context).unwrap().data[..MATCHER_RETURN_BYTES].to_vec()
    };
    assert_eq!(bytes.len(), legs.len() * MATCHER_RETURN_BYTES);
    let mut totals = [0; 4];
    for (bytes, leg) in bytes.chunks_exact(MATCHER_RETURN_BYTES).zip(legs) {
        let fill = read_matcher_return(bytes).unwrap();
        assert_eq!(fill.exec_size, leg.size);
        assert_eq!(fill.exec_price_e6, leg.price());
        for (sum, amount) in totals
            .iter_mut()
            .zip(leg.amounts(fill.exec_price_e6, leg.bps))
        {
            *sum += amount;
        }
    }
    totals
}

fn commit(
    w: &mut World,
    tx: Transaction,
    route: Route,
    deposit: u64,
) -> litesvm::types::TransactionMetadata {
    let before = w.frame(&tx);
    let payer = w.env.payer.pubkey();
    let mut expected_payer = before[&payer].clone().unwrap();
    expected_payer.lamports -= FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let meta = w
        .env
        .svm
        .send_transaction(tx)
        .expect("retained exact budgets execute");
    let mut changed = vec![w.env.market, w.portfolios[0], w.portfolios[1]];
    if deposit != 0 {
        changed.extend([w.sources[0], w.env.vault]);
    }
    if route == Route::SingleCpi {
        changed.push(w.context);
    }
    for (key, account) in before {
        let actual = w.env.svm.get_account(&key);
        if key == payer {
            assert_eq!(actual, Some(expected_payer.clone()));
        } else if changed.contains(&key) {
            let mut expected = account.unwrap();
            assert_ne!(actual.as_ref(), Some(&expected));
            expected.data = actual.as_ref().unwrap().data.clone();
            assert_eq!(actual, Some(expected), "only data changes: {key}");
        } else {
            assert_eq!(actual, account, "complete passive Account: {key}");
        }
    }
    assert_cu_within(
        "retained policy budget commit",
        meta.compute_units_consumed,
        BUNDLE_CU_LIMIT,
    );
    meta
}

#[test]
fn v16_retained_policy_route_budgets_bound_each_committed_prefix() {
    let matcher = std::fs::read(auth_matcher_program_path()).unwrap();
    let (mut worlds, mut commits, mut rejections, mut peak_cu) = (0, 0, 0, 0);
    for q in [POS_SCALE + 1, 100 * POS_SCALE + 1] {
        for direction in [-1, 1] {
            let mut reference = None;
            for reverse in [false, true] {
                for route in Route::ALL {
                    let case =
                        format!("q={q}, direction={direction}, reverse={reverse}, route={route:?}");
                    eprintln!("{case}");
                    let mut w = World::with_assets(&matcher, 3);
                    let mut spread = vec![4];
                    spread.extend_from_slice(&500u64.to_le_bytes());
                    spread.extend_from_slice(&500u64.to_le_bytes());
                    send_raw_tx(
                        &mut w.env.svm,
                        &w.env.payer,
                        Instruction {
                            program_id: w.matcher,
                            accounts: vec![
                                AccountMeta::new_readonly(w.owners[1].pubkey(), true),
                                AccountMeta::new(w.context, false),
                            ],
                            data: spread,
                        },
                        &[&w.owners[1]],
                    )
                    .unwrap();
                    let first = Leg {
                        asset: 2,
                        size: direction * (111 * POS_SCALE + 1) as i128,
                        bps: 37,
                    };
                    let mut rest = vec![
                        Leg {
                            asset: 0,
                            size: direction * q as i128,
                            bps: 99,
                        },
                        Leg {
                            asset: 1,
                            size: -direction * (3 * q) as i128,
                            bps: 99,
                        },
                    ];
                    if reverse {
                        rest.reverse();
                    }
                    assert_eq!(
                        budget(&rest, None)[2],
                        (4 * q * 5).div_ceil(POS_SCALE) + 1,
                        "per-leg ceilings differ from rounding the sum once"
                    );
                    let all = [vec![first], rest.clone()].concat();
                    let signed = budget(&all, None);
                    let mut chunks = vec![vec![first]];
                    if route.batch() {
                        chunks.push(rest);
                    } else {
                        chunks.extend(rest.into_iter().map(|leg| vec![leg]));
                    }
                    let epochs = w.portfolios.map(|key| w.env.portfolio_position_epoch(key));
                    let mut requests = w.env.market_state().0.matcher_req_seq;
                    let initial_controls = w.env.control_sequences(0);
                    let mut ledger = Ledger::default();
                    let deposits = [43, PREFIX - 43, 0];
                    let instructions: Vec<_> = chunks
                        .iter()
                        .enumerate()
                        .map(|(i, legs)| {
                            funded(
                                &w,
                                instruction(&w, route, legs, i as u64, Bound::Exact),
                                deposits[i],
                                i as u64,
                            )
                        })
                        .collect();
                    // All delivery alternatives, narrow caps and consumed-message checks are
                    // signed before either policy detour or any successful fill.
                    let retained: Vec<_> = instructions
                        .iter()
                        .enumerate()
                        .map(|(i, ixs)| [w.sign(ixs, 10 + i as u32), w.sign(ixs, 20 + i as u32)])
                        .collect();
                    let bytes: Vec<_> = retained
                        .iter()
                        .map(|pair| bincode::serialize(&pair[1]).unwrap())
                        .collect();
                    let narrow: Vec<_> = [Bound::FeeShort, Bound::SlippageShort]
                        .into_iter()
                        .map(|bound| {
                            w.sign(
                                &funded(
                                    &w,
                                    instruction(&w, route, &chunks[1], 1, bound),
                                    deposits[1],
                                    1,
                                ),
                                30,
                            )
                        })
                        .collect();
                    let consumed =
                        w.sign(&[instruction(&w, route, &chunks[1], 1, Bound::Exact)], 40);
                    w.simulate(&retained[0][1]);
                    ledger.check(&w, signed, epochs, requests);
                    let mut policies = 0;
                    for (i, legs) in chunks.iter().enumerate() {
                        if i < 2 {
                            assert!(budget(legs, Some(101))[3] > budget(legs, None)[3]);
                            peak_cu = peak_cu.max(w.policy(101, 100 + i as u32));
                            policies += 1;
                            ledger.check(&w, signed, epochs, requests);
                            let calls = [1, 1, usize::from(route == Route::BatchCpi)];
                            peak_cu =
                                peak_cu.max(w.deliver(retained[i][0].clone(), true, &[], calls));
                            rejections += 1;
                            ledger.check(&w, signed, epochs, requests);
                            peak_cu = peak_cu.max(w.policy(legs[0].bps, 110 + i as u32));
                            policies += 1;
                            ledger.check(&w, signed, epochs, requests);
                        }
                        if i == 1 && route.cpi() {
                            if route == Route::BatchCpi {
                                peak_cu =
                                    peak_cu.max(w.deliver(narrow[0].clone(), true, &[], [1, 1, 1]));
                                rejections += 1;
                                ledger.check(&w, signed, epochs, requests);
                            }
                            peak_cu =
                                peak_cu.max(w.deliver(narrow[1].clone(), true, &[], [1, 1, 1]));
                            rejections += 1;
                            ledger.check(&w, signed, epochs, requests);
                        }
                        assert_eq!(w.sign(&instructions[i], 20 + i as u32), retained[i][1]);
                        assert_eq!(bincode::serialize(&retained[i][1]).unwrap(), bytes[i]);
                        assert_eq!(
                            retained[i][1].message.header.num_required_signatures,
                            if route.cpi() { 2 } else { 3 }
                        );
                        let controls = w.env.control_sequences(0);
                        assert_eq!(controls.trade_fee, initial_controls.trade_fee + policies);
                        assert_eq!(controls.authority_epoch, initial_controls.authority_epoch);
                        assert_eq!(w.env.market_state().0.trade_fee_base_bps, legs[0].bps);
                        let meta = commit(&mut w, retained[i][1].clone(), route, deposits[i]);
                        peak_cu = peak_cu.max(meta.compute_units_consumed);
                        ledger.commit(legs, deposits[i], observe(&w, route, legs, &meta));
                        requests += u64::from(route.cpi());
                        ledger.check(&w, signed, epochs, requests);
                        assert_eq!(w.env.control_sequences(0), controls);
                        commits += 1;
                    }
                    peak_cu = peak_cu.max(w.deliver_with_error(
                        consumed,
                        Some((
                            2,
                            InstructionError::Custom(PercolatorError::EngineStale as u32),
                        )),
                        &[],
                        [0, 0, 0],
                    ));
                    rejections += 1;
                    ledger.check(&w, signed, epochs, requests);
                    assert_eq!(
                        ledger.amounts, signed,
                        "all signed legs consumed exactly once"
                    );
                    ledger.instructions = 0; // Epoch counts are checked above; batching changes them.
                    if let Some(expected) = &reference {
                        assert_eq!(
                            &ledger, expected,
                            "{case}: economic route and order equivalence"
                        );
                    } else {
                        reference = Some(ledger);
                    }
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!((worlds, commits, rejections), (32, 80, 120));
    eprintln!("INV-014 row 411 policy route budgets: worlds={worlds}, committed_prefixes={commits}, exact_rollbacks={rejections}, executed_legs=96, peak_CU={peak_cu}");
}
