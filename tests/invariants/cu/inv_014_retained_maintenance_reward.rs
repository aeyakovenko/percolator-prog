//! INV-014 / row411: a maintenance reward credited to the LP cannot obscure
//! either participant's retained trade-fee consent. Gross trade fees, maintenance
//! debits and the counterparty reward have separate input-derived accounts.
//! Public partial/exact routes compose fee-prefix rollback and exact owner exits.

use super::*;

const MAINTENANCE: u64 = 307;
const SHARE: u16 = 5_000;
const RATIO: u8 = 127;

fn maintenance(w: &World, actor: usize, reward_lp: bool) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(w.env.market, false),
        AccountMeta::new(w.portfolios[actor], false),
    ];
    if reward_lp {
        assert_eq!(actor, 0);
        accounts.push(AccountMeta::new(w.portfolios[1], false));
    }
    Instruction {
        program_id: w.env.program_id,
        accounts,
        data: ProgInstruction::SyncMaintenanceFee { now_slot: 1 }.encode(),
    }
}

fn policy(w: &mut World, bps: u64, nonce: u32) -> u64 {
    let mut expected = w.env.control_sequences(0);
    let mut config = w.env.market_state().0;
    let tx = w.sign_with_nonce(
        &[Instruction {
            program_id: w.env.program_id,
            accounts: vec![
                AccountMeta::new(w.env.admin.pubkey(), true),
                AccountMeta::new(w.env.market, false),
            ],
            data: ProgInstruction::UpdateTradeFeePolicy {
                trade_fee_base_bps: bps,
                policy_sequence: expected.trade_fee + 1,
                authority_epoch: expected.authority_epoch,
            }
            .encode(),
        }],
        nonce,
    );
    let cu = w.deliver(tx, false, &[w.env.market], [1, 0, 0]);
    expected.trade_fee += 1;
    config.trade_fee_base_bps = bps;
    assert_eq!(w.env.control_sequences(0), expected);
    assert_eq!(w.env.market_state().0, config);
    cu
}

struct Ledger {
    maintenance: [bool; 2],
    fees: u128,
    quantity: i128,
    deposited: bool,
    paid: [u64; 2],
    custody: [Account; 3],
}

impl Ledger {
    fn new(w: &World) -> Self {
        Self {
            maintenance: [false; 2],
            fees: 0,
            quantity: 0,
            deposited: false,
            paid: [0; 2],
            custody: [w.tokens[0], w.tokens[1], w.env.vault]
                .map(|key| w.env.svm.get_account(&key).unwrap()),
        }
    }

    fn reward(&self) -> u64 {
        if self.maintenance[0] {
            MAINTENANCE * u64::from(SHARE) / 10_000
        } else {
            0
        }
    }

    fn entitlements(&self) -> [u64; 2] {
        [0, 1].map(|actor| {
            PRINCIPAL[actor]
                + if actor == 0 && self.deposited {
                    DEPOSIT
                } else {
                    0
                }
                + if actor == 1 { self.reward() } else { 0 }
                - u64::from(self.maintenance[actor]) * MAINTENANCE
                - u64::try_from(self.fees).unwrap()
        })
    }

    fn check(&self, w: &World) {
        let entitlements = self.entitlements();
        let accounts = w.portfolios.map(|key| w.env.portfolio_state(key));
        for actor in 0..2 {
            let p = &accounts[actor];
            assert_eq!(p.owner, w.owners[actor].pubkey().to_bytes());
            assert_eq!(
                p.capital.get(),
                u128::from(entitlements[actor] - self.paid[actor])
            );
            assert_eq!(p.pnl.get(), 0);
            assert_eq!(p.reserved_pnl.get(), 0);
            assert_eq!(p.fee_credits.get(), 0);
            assert_eq!(p.last_fee_slot.get(), u64::from(self.maintenance[actor]));
            if self.quantity == 0 {
                assert!(!has_active_leg_for_asset(p, 0));
            } else {
                assert_eq!(
                    active_leg_for_asset(p, 0).basis_pos_q,
                    if actor == 0 {
                        self.quantity
                    } else {
                        -self.quantity
                    }
                );
            }
        }
        let (_, group) = w.env.market_state();
        let retained = [
            u64::from(self.maintenance[0]) * MAINTENANCE - self.reward(),
            u64::from(self.maintenance[1]) * MAINTENANCE,
        ];
        let domains = [
            self.fees + u128::from(retained.iter().map(|fee| fee / 2).sum::<u64>()),
            self.fees + u128::from(retained.iter().map(|fee| fee - fee / 2).sum::<u64>()),
        ];
        assert_eq!(&group.insurance_domain_budget[..2], &domains);
        assert!(group.insurance_domain_budget[2..]
            .iter()
            .all(|amount| *amount == 0));
        assert_eq!(group.insurance, domains.iter().sum::<u128>());
        assert_eq!(
            group.c_tot,
            accounts.iter().map(|p| p.capital.get()).sum::<u128>()
        );
        assert_eq!(group.assets[0].effective_price, PRICE);
        assert_eq!(group.assets[0].oi_eff_long_q, self.quantity.unsigned_abs());
        assert_eq!(group.assets[0].oi_eff_short_q, self.quantity.unsigned_abs());
        let vault = PRINCIPAL.iter().sum::<u64>() + u64::from(self.deposited) * DEPOSIT
            - self.paid.iter().sum::<u64>();
        assert_eq!(group.vault, u128::from(vault));
        assert_eq!(group.vault, group.c_tot + group.insurance);
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
            assert_eq!(
                w.env.svm.get_account(&key),
                Some(expected),
                "exact custody Account {key}"
            );
        }
        let mint = Mint::unpack(&w.env.svm.get_account(&w.env.mint).unwrap().data).unwrap();
        assert_eq!(mint.supply, PRINCIPAL.iter().sum::<u64>() + DEPOSIT);
        assert_eq!(mint.mint_authority, COption::None);
        assert_eq!(balances.iter().sum::<u64>(), mint.supply);
        assert_market_stock_census(
            "retained trade beside counterparty maintenance reward",
            &group,
            &w.env.svm.get_account(&w.env.market).unwrap().data,
            &accounts,
            vault.into(),
        )
        .unwrap();
        assert_reservation_encumbrance_census("retained maintenance reward", &group, &accounts)
            .unwrap();
    }
}

#[test]
fn v16_retained_trade_fees_exclude_counterparty_maintenance_rewards_across_routes() {
    let matcher = std::fs::read(hostile_matcher_program_path()).unwrap();
    let quantity = REQUEST * i128::from(RATIO) / 255;
    let charge = fee(quantity, SIGNED_BPS);
    let reward = MAINTENANCE * u64::from(SHARE) / 10_000;
    assert_eq!((charge, reward), (48, 153));
    assert!(fee(quantity, CURRENT_BPS) > charge);
    assert!(u128::from(reward) > fee(REQUEST, CURRENT_BPS));
    assert!(u128::from(MAINTENANCE) > fee(REQUEST, SIGNED_BPS));
    let mut peaks = [0; 4]; // Simulation, controls, rejected suffixes, committed routes.
    let (mut worlds, mut rollbacks, mut fills, mut payouts) = (0, 0, 0, 0);
    let mut endpoint = None;
    for direction in [-1, 1] {
        for route in [
            Route::PartialCpi,
            Route::SingleCpi,
            Route::BatchCpi,
            Route::SingleNoCpi,
            Route::BatchNoCpi,
        ] {
            for split in [false, true] {
                eprintln!("maintenance reward: {route:?}, direction={direction}, split={split}");
                let mut w = World::with_params(
                    &matcher,
                    V16CuMarketParams {
                        trade_fee_base_bps: 19,
                        maintenance_fee_per_slot: MAINTENANCE.into(),
                        ..V16CuMarketParams::default()
                    },
                );
                let mut book = Ledger::new(&w);
                book.check(&w);
                w.env.svm.warp_to_slot(1);
                let initial_epochs = w.portfolios.map(|key| w.env.portfolio_position_epoch(key));
                // Make the LP fee-current before retention so the later reward can
                // exceed its trade debit without another maintenance fee masking it.
                let tx = w.sign_with_nonce(&[maintenance(&w, 1, false)], 1);
                assert_eq!(tx.message.header.num_required_signatures, 1);
                peaks[1] =
                    peaks[1].max(w.deliver(tx, false, &[w.env.market, w.portfolios[1]], [1, 0, 0]));
                book.maintenance[1] = true;
                book.check(&w);
                let requested = direction * REQUEST;
                let executed = direction * quantity;
                let sync = maintenance(&w, 0, true);
                let [deposit, trade] =
                    w.bundle_instructions(route, requested, executed, SIGNED_BPS);
                let instructions = [sync.clone(), deposit, trade];
                let fail = spl_token::instruction::transfer(
                    &spl_token::ID,
                    &w.tokens[0],
                    &w.env.vault,
                    &w.owners[0].pubkey(),
                    &[],
                    u64::MAX,
                )
                .unwrap();
                let mut suffix = instructions.to_vec();
                suffix.push(fail);
                let retained = [
                    w.sign_with_nonce(&instructions, 10),
                    w.sign_with_nonce(&suffix, 11),
                    w.sign_with_nonce(&instructions, 12),
                ];
                let bytes = retained
                    .each_ref()
                    .map(|tx| bincode::serialize(tx).unwrap());
                for tx in &retained {
                    let signatures = if route.cpi() { 2 } else { 3 };
                    assert_eq!(tx.message.header.num_required_signatures, signatures);
                    assert_eq!(
                        tx.message.account_keys[..signatures as usize]
                            .contains(&w.owners[1].pubkey()),
                        !route.cpi()
                    );
                }
                let before = w.frame(&retained[0]);
                let simulation = w
                    .env
                    .svm
                    .simulate_transaction(retained[0].clone().into())
                    .unwrap();
                peaks[0] = peaks[0].max(simulation.compute_units_consumed);
                assert_eq!(w.frame(&retained[0]), before);
                let sequence = w.env.portfolio_matcher_sequence(w.portfolios[1]);
                let grant = w.env.portfolio_matcher_config(w.portfolios[1]);
                let requests = w.env.market_state().0.matcher_req_seq;
                let mut controls = w.env.control_sequences(0);
                let share_tx = w.sign_with_nonce(
                    &[Instruction {
                        program_id: w.env.program_id,
                        accounts: vec![
                            AccountMeta::new(w.env.admin.pubkey(), true),
                            AccountMeta::new(w.env.market, false),
                        ],
                        data: ProgInstruction::UpdateMaintenanceFeePolicy {
                            cranker_share_bps: SHARE,
                            policy_sequence: controls.maintenance_fee + 1,
                            authority_epoch: controls.authority_epoch,
                        }
                        .encode(),
                    }],
                    20,
                );
                peaks[1] = peaks[1].max(w.deliver(share_tx, false, &[w.env.market], [1, 0, 0]));
                controls.maintenance_fee += 1;
                assert_eq!(w.env.control_sequences(0), controls);
                book.check(&w);
                if route == Route::PartialCpi {
                    peaks[1] = peaks[1].max(w.partial(RATIO));
                    book.check(&w);
                }
                peaks[1] = peaks[1].max(policy(&mut w, CURRENT_BPS, 21));
                book.check(&w);
                peaks[2] = peaks[2].max(w.deliver_with_error(
                    retained[0].clone(),
                    Some((
                        4,
                        InstructionError::Custom(PercolatorError::InvalidInstruction as u32),
                    )),
                    &[],
                    [2, 1, usize::from(route == Route::BatchCpi)],
                ));
                rollbacks += 1;
                book.check(&w);
                peaks[1] = peaks[1].max(policy(&mut w, SIGNED_BPS, 22));
                book.check(&w);
                // This ordinary SPL failure is after maintenance, real deposit and
                // the authorized fill; all fee/reward/cursor/position effects roll back.
                peaks[2] = peaks[2].max(w.deliver_with_error(
                    retained[1].clone(),
                    Some((
                        5,
                        InstructionError::Custom(
                            spl_token::error::TokenError::InsufficientFunds as u32,
                        ),
                    )),
                    &[],
                    [3, 1, usize::from(route.cpi())],
                ));
                rollbacks += 1;
                book.check(&w);
                assert_eq!(
                    w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                    initial_epochs
                );
                assert_eq!(w.env.portfolio_matcher_config(w.portfolios[1]), grant);
                assert_eq!(w.env.portfolio_matcher_sequence(w.portfolios[1]), sequence);
                assert_eq!(w.env.market_state().0.matcher_req_seq, requests);
                if split {
                    let tx = w.sign_with_nonce(&[sync], 23);
                    assert_eq!(tx.message.header.num_required_signatures, 1);
                    peaks[3] = peaks[3].max(w.deliver(
                        tx,
                        false,
                        &[w.env.market, w.portfolios[0], w.portfolios[1]],
                        [1, 0, 0],
                    ));
                    book.maintenance[0] = true;
                    book.check(&w);
                    assert_eq!(
                        w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                        initial_epochs
                    );
                    assert_eq!(w.env.portfolio_matcher_config(w.portfolios[1]), grant);
                }
                for (tx, bytes) in retained.iter().zip(&bytes) {
                    tx.verify().unwrap();
                    assert_eq!(&bincode::serialize(tx).unwrap(), bytes);
                }
                assert_eq!(w.sign_with_nonce(&instructions, 12), retained[2]);
                let mut changed = vec![
                    w.env.market,
                    w.portfolios[0],
                    w.portfolios[1],
                    w.tokens[0],
                    w.env.vault,
                ];
                if matches!(route, Route::PartialCpi | Route::SingleCpi) {
                    changed.push(w.context);
                }
                peaks[3] = peaks[3].max(w.deliver(
                    retained[2].clone(),
                    false,
                    &changed,
                    [3, 1, usize::from(route.cpi())],
                ));
                book.maintenance[0] = true;
                book.deposited = true;
                book.fees = charge;
                book.quantity = executed;
                book.check(&w);
                assert_eq!(
                    w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                    initial_epochs.map(|epoch| epoch + 1)
                );
                assert_eq!(w.env.portfolio_matcher_sequence(w.portfolios[1]), sequence);
                assert_eq!(
                    w.env.market_state().0.matcher_req_seq,
                    requests + u64::from(route.cpi())
                );
                controls.trade_fee += 2;
                assert_eq!(w.env.control_sequences(0), controls);
                assert_eq!(
                    w.env.portfolio_matcher_config(w.portfolios[1]).enabled(),
                    u64::from(route.cpi())
                );
                if matches!(route, Route::PartialCpi | Route::SingleCpi) {
                    let fill =
                        read_matcher_return(&w.env.svm.get_account(&w.context).unwrap().data)
                            .unwrap();
                    assert_eq!((fill.exec_size, fill.exec_price_e6), (executed, PRICE));
                    assert_eq!(
                        fill.flags & FLAG_PARTIAL_OK != 0,
                        route == Route::PartialCpi
                    );
                }
                assert!(charge <= fee(executed, SIGNED_BPS));
                assert!(charge <= fee(executed, u64::from(LP_CAP)));
                assert_eq!(
                    book.entitlements()[1] - (PRINCIPAL[1] - MAINTENANCE),
                    reward - charge as u64,
                    "LP net credit still contains a separately bounded gross trade fee"
                );
                fills += 1;
                let close =
                    w.bundle_instructions(Route::SingleNoCpi, -executed, -executed, SIGNED_BPS)[1]
                        .clone();
                let tx = w.sign_with_nonce(&[close], 30);
                peaks[3] = peaks[3].max(w.deliver(
                    tx,
                    false,
                    &[w.env.market, w.portfolios[0], w.portfolios[1]],
                    [1, 0, 0],
                ));
                book.quantity = 0;
                book.fees += charge;
                book.check(&w);
                fills += 1;
                for actor in 0..2 {
                    let amount = book.entitlements()[actor];
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
                        31 + actor as u32,
                    );
                    peaks[3] = peaks[3].max(w.deliver(
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
                    book.check(&w);
                    payouts += 1;
                }
                let outcome = (
                    book.paid,
                    w.env.token_amount(w.env.vault),
                    w.env.market_state().1.insurance_domain_budget,
                );
                assert_eq!(book.paid, [99_713, 199_757]);
                assert_eq!(outcome.1, 653);
                if let Some(expected) = &endpoint {
                    assert_eq!(
                        &outcome, expected,
                        "route and fee-prefix grouping preserve value"
                    );
                } else {
                    endpoint = Some(outcome);
                }
                worlds += 1;
            }
        }
    }
    assert_eq!((worlds, rollbacks, fills, payouts), (20, 40, 40, 40));
    assert!(peaks.iter().all(|cu| *cu > 0 && *cu <= CU_LIMIT));
    eprintln!("INV-014 row411 maintenance reward: worlds={worlds}, exact_rollbacks={rollbacks}, fills={fills}, payouts={payouts}, peak_CU={peaks:?}");
}
