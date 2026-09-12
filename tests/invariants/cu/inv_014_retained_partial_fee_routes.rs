//! INV-014 / row 411: reducing a retained request's fill cannot relax its signed
//! fee rate, even when the smaller fill costs less than the full-request ceiling.
//! Fresh consent must produce the same economics through partial CPI and all
//! four exact transports at that executed quantity. Batch CPI uses the equivalent
//! signed atom cap; its per-leg bps field is not a taker rate ceiling.
//! Public construction only.

use super::super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use percolator_prog::matcher_abi::{read_matcher_return, FLAG_PARTIAL_OK};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};
use std::collections::BTreeMap;

const PRINCIPAL: [u64; 2] = [100_003, 200_007];
const DEPOSIT: u64 = 113;
const PRICE: u64 = 100;
const SIGNED_BPS: u64 = 37;
const CURRENT_BPS: u64 = SIGNED_BPS + 1;
const LP_CAP: u16 = 137;
const REQUEST: i128 = (255 * POS_SCALE + POS_SCALE / 2 + 1) as i128;
const CU_LIMIT: u64 = 500_000;

fn fee(size: i128, bps: u64) -> u128 {
    let notional = (size.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE);
    (notional * u128::from(bps)).div_ceil(10_000)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    PartialCpi,
    SingleCpi,
    BatchCpi,
    SingleNoCpi,
    BatchNoCpi,
}

impl Route {
    fn cpi(self) -> bool {
        matches!(self, Self::PartialCpi | Self::SingleCpi | Self::BatchCpi)
    }
}

struct World {
    env: V16CuEnv,
    owners: [Keypair; 2],
    portfolios: [Pubkey; 2],
    tokens: [Pubkey; 2],
    matcher: Pubkey,
    context: Pubkey,
    delegate: Pubkey,
}

impl World {
    fn new(matcher_bytes: &[u8]) -> Self {
        let mut env = inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params(
            6,
            V16CuMarketParams {
                trade_fee_base_bps: 19,
                ..V16CuMarketParams::default()
            },
        );
        let owners = [Keypair::new(), Keypair::new()];
        let portfolio_keys = [Keypair::new(), Keypair::new()];
        let portfolios = portfolio_keys.each_ref().map(Signer::pubkey);
        let mut tokens = [Pubkey::default(); 2];
        for actor in 0..2 {
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                system_instruction::transfer(
                    &env.payer.pubkey(),
                    &owners[actor].pubkey(),
                    1_000_000,
                ),
                &[],
            )
            .unwrap();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio_keys[actor],
                env.portfolio_account_len,
                env.program_id,
            );
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                ],
                &[&owners[actor]],
            )
            .unwrap();
            env.portfolios.push(portfolios[actor]);
            tokens[actor] =
                create_ata_for_test(&mut env.svm, &env.payer, owners[actor].pubkey(), env.mint);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &tokens[actor],
                    &env.admin.pubkey(),
                    &[],
                    PRINCIPAL[actor] + if actor == 0 { DEPOSIT } else { 0 },
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            env.send(
                env.deposit_ix(portfolios[actor], PRINCIPAL[actor].into()),
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[actor]],
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
        let matcher = Pubkey::new_unique();
        env.svm.add_program(matcher, matcher_bytes);
        let context_key = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &context_key,
            MATCHER_CONTEXT_LEN,
            matcher,
        );
        let context = context_key.pubkey();
        let delegate = matcher_delegate_key(
            &env.program_id,
            &env.market,
            &portfolios[1],
            &owners[1].pubkey(),
            &matcher,
            &context,
        );
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            Instruction {
                program_id: matcher,
                accounts: vec![
                    AccountMeta::new_readonly(owners[1].pubkey(), true),
                    AccountMeta::new(context, false),
                ],
                data: vec![10],
            },
            &[&owners[1]],
        )
        .unwrap();
        env.set_matcher_config_with_trade_fee_cap(
            matcher,
            &owners[1],
            portfolios[1],
            context,
            delegate,
            1,
            LP_CAP,
        );
        Self {
            env,
            owners,
            portfolios,
            tokens,
            matcher,
            context,
            delegate,
        }
    }

    fn sign(&self, instructions: &[Instruction]) -> Transaction {
        let mut ixs = vec![
            heap_ix(),
            ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT as u32),
        ];
        ixs.extend_from_slice(instructions);
        let mut signers = vec![&self.env.payer];
        for signer in self.owners.iter().chain([&self.env.admin]) {
            if instructions
                .iter()
                .flat_map(|ix| &ix.accounts)
                .any(|meta| meta.is_signer && meta.pubkey == signer.pubkey())
            {
                signers.push(signer);
            }
        }
        let tx = Transaction::new_signed_with_payer(
            &ixs,
            Some(&self.env.payer.pubkey()),
            &signers,
            self.env.svm.latest_blockhash(),
        );
        tx.verify().unwrap();
        assert!(
            bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64
        );
        tx
    }

    fn bundle(&self, route: Route, requested: i128, executed: i128, bps: u64) -> Transaction {
        let [a, b] = self.portfolios;
        let size = if route == Route::PartialCpi {
            requested
        } else {
            executed
        };
        let trade = match route {
            Route::PartialCpi | Route::SingleCpi => {
                self.env.trade_cpi_ix(a, b, 0, size, bps, PRICE)
            }
            Route::BatchCpi => self.env.batch_trade_cpi_ix_with_caps(
                a,
                b,
                vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: self.env.asset_market_id(0),
                    size_q: size,
                    fee_bps: u64::from(LP_CAP),
                    limit_price: PRICE,
                }],
                0,
                fee(executed, bps),
            ),
            Route::SingleNoCpi => self.env.trade_no_cpi_ix(a, b, 0, size, PRICE, bps),
            Route::BatchNoCpi => self.env.batch_trade_no_cpi_ix(
                a,
                b,
                vec![BatchTradeLeg {
                    asset_index: 0,
                    market_id: self.env.asset_market_id(0),
                    size_q: size,
                    exec_price: PRICE,
                    fee_bps: bps,
                }],
            ),
        };
        let accounts = if route.cpi() {
            vec![
                AccountMeta::new(self.owners[0].pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(a, false),
                AccountMeta::new(b, false),
                AccountMeta::new_readonly(self.matcher, false),
                AccountMeta::new(self.context, false),
                AccountMeta::new_readonly(self.delegate, false),
            ]
        } else {
            vec![
                AccountMeta::new(self.owners[0].pubkey(), true),
                AccountMeta::new(self.owners[1].pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(a, false),
                AccountMeta::new(b, false),
            ]
        };
        let tx = self.sign(&[
            Instruction {
                program_id: self.env.program_id,
                accounts: vec![
                    AccountMeta::new(self.owners[0].pubkey(), true),
                    AccountMeta::new(self.env.market, false),
                    AccountMeta::new(a, false),
                    AccountMeta::new(self.tokens[0], false),
                    AccountMeta::new(self.env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: self.env.deposit_ix(a, DEPOSIT.into()).encode(),
            },
            Instruction {
                program_id: self.env.program_id,
                accounts,
                data: trade.encode(),
            },
        ]);
        assert_eq!(
            tx.message.header.num_required_signatures,
            if route.cpi() { 2 } else { 3 }
        );
        assert_eq!(
            tx.message.account_keys[..tx.message.header.num_required_signatures as usize]
                .contains(&self.owners[1].pubkey()),
            !route.cpi(),
        );
        tx
    }

    fn frame(&self, tx: &Transaction) -> BTreeMap<Pubkey, Option<Account>> {
        [
            self.env.market,
            self.env.mint,
            self.env.vault,
            self.env.vault_authority,
            self.env.admin.pubkey(),
            self.owners[0].pubkey(),
            self.owners[1].pubkey(),
            self.portfolios[0],
            self.portfolios[1],
            self.tokens[0],
            self.tokens[1],
            self.matcher,
            self.context,
            self.delegate,
            self.env.program_id,
            spl_token::ID,
            solana_sdk::sysvar::clock::ID,
        ]
        .into_iter()
        .chain(tx.message.account_keys.iter().copied())
        .map(|key| (key, self.env.svm.get_account(&key)))
        .collect()
    }

    fn deliver(
        &mut self,
        tx: Transaction,
        reject: bool,
        changed: &[Pubkey],
        calls: [usize; 3],
    ) -> u64 {
        let before = self.frame(&tx);
        let payer = self.env.payer.pubkey();
        let mut payer_account = before[&payer].clone().unwrap();
        payer_account.lamports -= FeeStructure::default().lamports_per_signature
            * u64::from(tx.message.header.num_required_signatures);
        let result = self.env.svm.send_transaction(tx);
        let meta = if reject {
            let failure = result.expect_err("smaller fills do not relax signed fee rates");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    3,
                    InstructionError::Custom(PercolatorError::InvalidInstruction as u32),
                )
            );
            if calls[2] == 0 {
                assert!(!failure
                    .meta
                    .logs
                    .iter()
                    .any(|log| log.starts_with(&format!("Program {} invoke", self.matcher))));
            }
            failure.meta
        } else {
            result.expect("authorized public route succeeds")
        };
        for (key, account) in before {
            let actual = self.env.svm.get_account(&key);
            if key == payer {
                assert_eq!(actual, Some(payer_account.clone()));
            } else if reject || !changed.contains(&key) {
                assert_eq!(actual, account, "complete Account: {key}");
            } else {
                let mut expected = account.unwrap();
                expected.data = actual.as_ref().unwrap().data.clone();
                assert_eq!(actual, Some(expected), "only data may change: {key}");
            }
        }
        for (program, count) in [self.env.program_id, spl_token::ID, self.matcher]
            .into_iter()
            .zip(calls)
        {
            assert_eq!(
                meta.logs
                    .iter()
                    .filter(|log| **log == format!("Program {program} success"))
                    .count(),
                count
            );
        }
        assert_cu_within(
            "retained partial fee route",
            meta.compute_units_consumed,
            CU_LIMIT,
        );
        meta.compute_units_consumed
    }

    fn policy(&mut self) -> u64 {
        let mut config = self.env.market_state().0;
        let mut sequences = self.env.control_sequences(0);
        let tx = self.sign(&[Instruction {
            program_id: self.env.program_id,
            accounts: vec![
                AccountMeta::new(self.env.admin.pubkey(), true),
                AccountMeta::new(self.env.market, false),
            ],
            data: ProgInstruction::UpdateTradeFeePolicy {
                trade_fee_base_bps: CURRENT_BPS,
                policy_sequence: sequences.trade_fee + 1,
                authority_epoch: sequences.authority_epoch,
            }
            .encode(),
        }]);
        let cu = self.deliver(tx, false, &[self.env.market], [1, 0, 0]);
        config.trade_fee_base_bps = CURRENT_BPS;
        sequences.trade_fee += 1;
        assert_eq!(
            self.env.market_state().0,
            config,
            "no authority, mint, oracle or fee redirect change"
        );
        assert_eq!(self.env.control_sequences(0), sequences);
        cu
    }

    fn partial(&mut self, numerator: u8) -> u64 {
        let tx = self.sign(&[Instruction {
            program_id: self.matcher,
            accounts: vec![
                AccountMeta::new_readonly(self.owners[1].pubkey(), true),
                AccountMeta::new(self.context, false),
            ],
            data: vec![11, 19, numerator],
        }]);
        self.deliver(tx, false, &[self.context], [0, 0, 1])
    }

    fn check(&self, executed: i128) -> ([u128; 2], u128, Vec<u128>, u64, [u64; 2]) {
        let filled = executed != 0;
        let paid = fee(executed, CURRENT_BPS);
        let accounts = self.portfolios.map(|key| self.env.portfolio_state(key));
        let capitals = accounts.each_ref().map(|account| account.capital.get());
        let tokens = self.tokens.map(|key| self.env.token_amount(key));
        for actor in 0..2 {
            assert_eq!(
                accounts[actor].owner,
                self.owners[actor].pubkey().to_bytes()
            );
            assert_eq!(
                capitals[actor],
                u128::from(PRINCIPAL[actor] + if filled && actor == 0 { DEPOSIT } else { 0 })
                    - paid
            );
            assert_eq!(accounts[actor].pnl.get(), 0);
            assert_eq!(
                tokens[actor],
                if !filled && actor == 0 { DEPOSIT } else { 0 }
            );
            if filled {
                assert_eq!(
                    active_leg_for_asset(&accounts[actor], 0).basis_pos_q,
                    if actor == 0 { executed } else { -executed }
                );
            } else {
                assert!(!has_active_leg_for_asset(&accounts[actor], 0));
            }
        }
        let (config, group) = self.env.market_state();
        assert_eq!(config.fee_redirect_to_market_0_bps, 0);
        let vault = PRINCIPAL.iter().sum::<u64>() + if filled { DEPOSIT } else { 0 };
        assert_eq!(group.assets[0].effective_price, PRICE);
        assert_eq!(group.assets[0].oi_eff_long_q, executed.unsigned_abs());
        assert_eq!(group.assets[0].oi_eff_short_q, executed.unsigned_abs());
        assert_eq!(group.insurance, 2 * paid);
        assert_eq!(&group.insurance_domain_budget[..2], &[paid; 2]);
        assert!(group.insurance_domain_budget[2..]
            .iter()
            .all(|budget| *budget == 0));
        assert_eq!(group.c_tot, u128::from(vault) - 2 * paid);
        assert_eq!(group.vault, u128::from(vault));
        assert_eq!(self.env.token_amount(self.env.vault), vault);
        assert_market_stock_census(
            "retained partial fee routes",
            &group,
            &self.env.svm.get_account(&self.env.market).unwrap().data,
            &accounts,
            vault.into(),
        )
        .unwrap();
        assert_reservation_encumbrance_census("retained partial fee routes", &group, &accounts)
            .unwrap();
        let mint = Mint::unpack(&self.env.svm.get_account(&self.env.mint).unwrap().data).unwrap();
        assert_eq!(mint.supply, PRINCIPAL.iter().sum::<u64>() + DEPOSIT);
        assert_eq!(mint.mint_authority, COption::None);
        (
            capitals,
            group.insurance,
            group.insurance_domain_budget.to_vec(),
            vault,
            tokens,
        )
    }
}

#[test]
fn v16_retained_partial_fill_fee_rate_matches_exact_routes_after_funded_rejection() {
    let matcher_bytes = std::fs::read(hostile_matcher_program_path()).unwrap();
    let (
        mut simulation_cu,
        mut policy_cu,
        mut matcher_control_cu,
        mut rejection_cu,
        mut success_cu,
    ) = (0, 0, 0, 0, 0);
    let mut worlds = 0;
    for numerator in [64u8, 191] {
        // Independent floor of the offered ratio, followed by two ceiling fee operations.
        let quantity = REQUEST * i128::from(numerator) / 255;
        let paid = fee(quantity, CURRENT_BPS);
        assert!(fee(quantity, SIGNED_BPS) < paid);
        assert!(
            paid < fee(REQUEST, SIGNED_BPS),
            "the smaller atom debit cannot authorize a higher bps rate"
        );
        assert_ne!(paid, fee(quantity, 19));
        let mut endpoint = None;
        for direction in [-1, 1] {
            for route in [
                Route::PartialCpi,
                Route::SingleCpi,
                Route::BatchCpi,
                Route::SingleNoCpi,
                Route::BatchNoCpi,
            ] {
                let label = format!("{route:?}, direction={direction}, ratio={numerator}/255");
                let mut w = World::new(&matcher_bytes);
                let requested = direction * REQUEST;
                let executed = direction * quantity;
                let retained = w.bundle(route, requested, executed, SIGNED_BPS);
                let signed = bincode::serialize(&retained).unwrap();
                let before = w.frame(&retained);
                let simulation = w
                    .env
                    .svm
                    .simulate_transaction(retained.clone().into())
                    .unwrap();
                assert_cu_within(
                    "retained partial fee simulation",
                    simulation.compute_units_consumed,
                    CU_LIMIT,
                );
                simulation_cu = simulation_cu.max(simulation.compute_units_consumed);
                assert_eq!(w.frame(&retained), before);
                let epochs = w.portfolios.map(|key| w.env.portfolio_position_epoch(key));
                let grant = w.env.portfolio_matcher_config(w.portfolios[1]);
                let sequence = w.env.portfolio_matcher_sequence(w.portfolios[1]);
                w.check(0);
                if route == Route::PartialCpi {
                    matcher_control_cu = matcher_control_cu.max(w.partial(numerator));
                    w.check(0);
                }
                policy_cu = policy_cu.max(w.policy());
                w.check(0);
                assert_eq!(w.env.portfolio_matcher_config(w.portfolios[1]), grant);
                assert_eq!(w.bundle(route, requested, executed, SIGNED_BPS), retained);
                assert_eq!(bincode::serialize(&retained).unwrap(), signed);
                rejection_cu = rejection_cu.max(w.deliver(
                    retained.clone(),
                    true,
                    &[],
                    [1, 1, usize::from(route == Route::BatchCpi)],
                ));
                w.check(0);
                assert_eq!(
                    w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                    epochs
                );
                assert_eq!(w.env.portfolio_matcher_sequence(w.portfolios[1]), sequence);
                assert_eq!(w.env.portfolio_matcher_config(w.portfolios[1]), grant);

                let fresh = w.bundle(route, requested, executed, CURRENT_BPS);
                assert_ne!(fresh.signatures, retained.signatures);
                // Fresh signatures change only fee consent, preserving route, accounts,
                // quantity, price and deposit. Batch CPI's taker consent is its atom cap.
                let mut revised =
                    ProgInstruction::decode(&retained.message.instructions[3].data).unwrap();
                match &mut revised {
                    ProgInstruction::TradeCpi { fee_bps, .. }
                    | ProgInstruction::TradeNoCpi { fee_bps, .. } => *fee_bps = CURRENT_BPS,
                    ProgInstruction::BatchTradeCpi { max_fee_atoms, .. } => {
                        *max_fee_atoms = paid;
                    }
                    ProgInstruction::BatchTradeNoCpi { legs, .. } => legs[0].fee_bps = CURRENT_BPS,
                    _ => unreachable!(),
                }
                let mut expected_message = retained.message.clone();
                expected_message.instructions[3].data = revised.encode();
                assert_eq!(fresh.message, expected_message);
                let token_before = [w.tokens[0], w.tokens[1], w.env.vault]
                    .map(|key| w.env.svm.get_account(&key).unwrap());
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
                let control = w.env.control_sequences(0);
                let mut config = w.env.market_state().0;
                config.matcher_req_seq += u64::from(route.cpi());
                success_cu = success_cu.max(w.deliver(
                    fresh,
                    false,
                    &changed,
                    [2, 1, usize::from(route.cpi())],
                ));
                assert_eq!(w.env.control_sequences(0), control);
                assert_eq!(
                    w.env.market_state().0,
                    config,
                    "{label}: no policy redirect on fill"
                );
                assert_eq!(
                    w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                    epochs.map(|epoch| epoch + 1)
                );
                if route.cpi() {
                    assert_eq!(w.env.portfolio_matcher_sequence(w.portfolios[1]), sequence);
                    let current = w.env.portfolio_matcher_config(w.portfolios[1]);
                    assert_eq!(current.matcher_program, grant.matcher_program);
                    assert_eq!(current.matcher_context, grant.matcher_context);
                    assert_eq!(current.matcher_delegate, grant.matcher_delegate);
                    assert_eq!(current.trade_fee_cap_bps(), LP_CAP);
                    assert_eq!(current.enabled(), 1);
                } else {
                    assert_eq!(w.env.portfolio_matcher_config(w.portfolios[1]).enabled(), 0);
                    assert_eq!(w.env.portfolio_matcher_sequence(w.portfolios[1]), sequence);
                }
                if matches!(route, Route::PartialCpi | Route::SingleCpi) {
                    let fill =
                        read_matcher_return(&w.env.svm.get_account(&w.context).unwrap().data)
                            .unwrap();
                    assert_eq!(fill.exec_size, executed);
                    assert_eq!(fill.exec_price_e6, PRICE);
                    assert_eq!(
                        fill.flags & FLAG_PARTIAL_OK != 0,
                        route == Route::PartialCpi
                    );
                }
                for ((key, mut expected), delta) in [w.tokens[0], w.tokens[1], w.env.vault]
                    .into_iter()
                    .zip(token_before)
                    .zip([-i128::from(DEPOSIT), 0, i128::from(DEPOSIT)])
                {
                    let mut token = TokenAccount::unpack(&expected.data).unwrap();
                    token.amount = u64::try_from(i128::from(token.amount) + delta).unwrap();
                    TokenAccount::pack(token, &mut expected.data).unwrap();
                    assert_eq!(
                        w.env.svm.get_account(&key),
                        Some(expected),
                        "{label}: exact SPL Account bytes"
                    );
                }
                let outcome = w.check(executed);
                assert!(
                    paid <= fee(executed, CURRENT_BPS) && paid <= fee(executed, u64::from(LP_CAP))
                );
                if let Some(expected) = &endpoint {
                    assert_eq!(&outcome, expected, "{label}: route-equivalent economics");
                } else {
                    endpoint = Some(outcome);
                }
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 20);
    eprintln!("INV-014 row 411 partial fee routes: worlds={worlds}, initial_simulations={worlds}, funded_rollbacks={worlds}, fresh_successes={worlds}; max CU simulation={simulation_cu}, policy={policy_cu}, matcher_control={matcher_control_cu}, rejection={rejection_cu}, success={success_cu}");
}
