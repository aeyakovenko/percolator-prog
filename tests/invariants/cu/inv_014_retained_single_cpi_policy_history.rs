//! INV-014 / row 432: retained taker consent across repeated base-policy changes.
//! A successful SPL deposit prefix must roll back when the single-CPI taker cap
//! rejects. Restored policy preserves pre-signed alternatives; a second increase
//! requires fresh taker consent. The LP cap remains independently permissive.
//! A second relation compares retained CPI/current-policy and bilateral/explicit
//! fees after nonmonotone histories and rollback of an already successful fill.
//! Bounded Live/full-fill conformance, with no economic account-image injection.

use super::super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};
use std::collections::BTreeMap;

#[path = "inv_014_retained_fee_authority_epoch.rs"]
mod retained_fee_authority_epoch;

#[path = "inv_014_retained_policy_route_budgets.rs"]
mod retained_policy_route_budgets;

#[path = "inv_014_retained_underfunded_close.rs"]
mod retained_underfunded_close;

const DEPOSITS: [u64; 2] = [100_003, 200_007];
const PREFIX: u64 = 113;
const PRICE: u64 = 100;
const OLD_BPS: u64 = 19;
const LP_CAP_BPS: u16 = 137;
const QUANTITY: i128 = (255 * POS_SCALE + POS_SCALE / 2 + 1) as i128;
const BUNDLE_CU_LIMIT: u64 = 500_000;

fn fee(bps: u64) -> u128 {
    let notional = (QUANTITY.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE);
    (notional * u128::from(bps)).div_ceil(10_000)
}

struct World {
    env: V16CuEnv,
    owners: [Keypair; 2],
    portfolios: [Pubkey; 2],
    sources: [Pubkey; 2],
    matcher: Pubkey,
    context: Pubkey,
    delegate: Pubkey,
}

impl World {
    fn new(matcher_bytes: &[u8]) -> Self {
        Self::with_assets(matcher_bytes, 1)
    }

    fn with_assets(matcher_bytes: &[u8], assets: u16) -> Self {
        Self::with_params(
            matcher_bytes,
            V16CuMarketParams {
                max_portfolio_assets: assets,
                trade_fee_base_bps: OLD_BPS,
                ..V16CuMarketParams::default()
            },
        )
    }

    fn with_params(matcher_bytes: &[u8], params: V16CuMarketParams) -> Self {
        let mut env = inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params(
            6,
            params,
        );
        let owners = [Keypair::new(), Keypair::new()];
        let portfolio_keys = [Keypair::new(), Keypair::new()];
        let portfolios = portfolio_keys.each_ref().map(Signer::pubkey);
        let mut sources = [Pubkey::default(); 2];
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
            sources[actor] =
                create_ata_for_test(&mut env.svm, &env.payer, owners[actor].pubkey(), env.mint);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &sources[actor],
                    &env.admin.pubkey(),
                    &[],
                    DEPOSITS[actor] + if actor == 0 { PREFIX } else { 0 },
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            env.send(
                env.deposit_ix(portfolios[actor], DEPOSITS[actor].into()),
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                    AccountMeta::new(sources[actor], false),
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
        let (context, delegate, _) =
            env.init_auth_matcher_context_via_system_create(matcher, &owners[1], portfolios[1]);
        env.set_matcher_config_with_trade_fee_cap(
            matcher,
            &owners[1],
            portfolios[1],
            context,
            delegate,
            1,
            LP_CAP_BPS,
        );
        Self {
            env,
            owners,
            portfolios,
            sources,
            matcher,
            context,
            delegate,
        }
    }

    fn sign(&self, instructions: &[Instruction], nonce: u32) -> Transaction {
        let mut message = vec![
            heap_ix(),
            ComputeBudgetInstruction::set_compute_unit_limit(BUNDLE_CU_LIMIT as u32 - nonce),
        ];
        message.extend_from_slice(instructions);
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
            &message,
            Some(&self.env.payer.pubkey()),
            &signers,
            self.env.svm.latest_blockhash(),
        );
        tx.verify().unwrap();
        assert_eq!(
            usize::from(tx.message.header.num_required_signatures),
            signers.len()
        );
        assert!(
            bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64
        );
        tx
    }

    fn bundle(&self, trade: &ProgInstruction, nonce: u32) -> Transaction {
        self.sign(&self.bundle_instructions(trade), nonce)
    }

    fn bundle_instructions(&self, trade: &ProgInstruction) -> [Instruction; 2] {
        let trade_accounts = if matches!(trade, ProgInstruction::TradeNoCpi { .. }) {
            vec![
                AccountMeta::new(self.owners[0].pubkey(), true),
                AccountMeta::new(self.owners[1].pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolios[0], false),
                AccountMeta::new(self.portfolios[1], false),
            ]
        } else {
            assert!(matches!(trade, ProgInstruction::TradeCpi { .. }));
            vec![
                AccountMeta::new(self.owners[0].pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolios[0], false),
                AccountMeta::new(self.portfolios[1], false),
                AccountMeta::new_readonly(self.matcher, false),
                AccountMeta::new(self.context, false),
                AccountMeta::new_readonly(self.delegate, false),
            ]
        };
        [
            Instruction {
                program_id: self.env.program_id,
                accounts: vec![
                    AccountMeta::new(self.owners[0].pubkey(), true),
                    AccountMeta::new(self.env.market, false),
                    AccountMeta::new(self.portfolios[0], false),
                    AccountMeta::new(self.sources[0], false),
                    AccountMeta::new(self.env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: self
                    .env
                    .deposit_ix(self.portfolios[0], PREFIX.into())
                    .encode(),
            },
            Instruction {
                program_id: self.env.program_id,
                accounts: trade_accounts,
                data: trade.encode(),
            },
        ]
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
            self.sources[0],
            self.sources[1],
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

    fn simulate(&mut self, tx: &Transaction) -> u64 {
        let before = self.frame(tx);
        let result = self
            .env
            .svm
            .simulate_transaction(tx.clone().into())
            .unwrap();
        assert_eq!(
            self.frame(tx),
            before,
            "simulation preserves complete Accounts"
        );
        assert_cu_within(
            "retained single-CPI simulation",
            result.compute_units_consumed,
            BUNDLE_CU_LIMIT,
        );
        result.compute_units_consumed
    }

    fn deliver(
        &mut self,
        tx: Transaction,
        reject: bool,
        changed: &[Pubkey],
        calls: [usize; 3],
    ) -> u64 {
        let error = reject.then_some((
            3,
            InstructionError::Custom(PercolatorError::InvalidInstruction as u32),
        ));
        self.deliver_with_error(tx, error, changed, calls)
    }

    fn deliver_with_error(
        &mut self,
        tx: Transaction,
        error: Option<(u8, InstructionError)>,
        changed: &[Pubkey],
        calls: [usize; 3],
    ) -> u64 {
        let before = self.frame(&tx);
        let payer = self.env.payer.pubkey();
        let mut expected_payer = before[&payer].clone().unwrap();
        expected_payer.lamports -= FeeStructure::default().lamports_per_signature
            * u64::from(tx.message.header.num_required_signatures);
        let result = self.env.svm.send_transaction(tx);
        let reject = error.is_some();
        let meta = if let Some((index, error)) = error {
            let failure = result.expect_err("retained bundle must reject at the expected boundary");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(index, error)
            );
            failure.meta
        } else {
            result.expect("current consent executes")
        };
        assert_eq!(
            meta.logs
                .iter()
                .filter(|log| log.starts_with(&format!("Program {} invoke", self.matcher)))
                .count(),
            calls[2],
            "matcher attempts and successful returns agree"
        );
        for (key, account) in before {
            if key == payer {
                assert_eq!(self.env.svm.get_account(&key), Some(expected_payer.clone()));
            } else if reject || !changed.contains(&key) {
                assert_eq!(
                    self.env.svm.get_account(&key),
                    account,
                    "complete Account frame: {key}"
                );
            } else {
                let actual = self.env.svm.get_account(&key);
                assert_ne!(actual, account, "expected write: {key}");
                let mut expected = account.unwrap();
                expected.data = actual.as_ref().unwrap().data.clone();
                assert_eq!(actual, Some(expected), "only data changes: {key}");
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
            "retained single-CPI delivery",
            meta.compute_units_consumed,
            BUNDLE_CU_LIMIT,
        );
        meta.compute_units_consumed
    }

    fn policy(&mut self, bps: u64, nonce: u32) -> u64 {
        let sequences = self.env.control_sequences(0);
        let tx = self.sign(
            &[Instruction {
                program_id: self.env.program_id,
                accounts: vec![
                    AccountMeta::new(self.env.admin.pubkey(), true),
                    AccountMeta::new(self.env.market, false),
                ],
                data: ProgInstruction::UpdateTradeFeePolicy {
                    trade_fee_base_bps: bps,
                    policy_sequence: sequences.trade_fee + 1,
                    authority_epoch: sequences.authority_epoch,
                }
                .encode(),
            }],
            nonce,
        );
        let cu = self.deliver(tx, false, &[self.env.market], [1, 0, 0]);
        let mut expected = sequences;
        expected.trade_fee += 1;
        assert_eq!(self.env.control_sequences(0), expected);
        assert_eq!(self.env.market_state().0.trade_fee_base_bps, bps);
        cu
    }

    fn check(&self, size: i128, bps: u64) {
        self.check_economics(size, bps);
        if size != 0 {
            let fill = percolator_prog::matcher_abi::read_matcher_return(
                &self.env.svm.get_account(&self.context).unwrap().data,
            )
            .unwrap();
            assert_eq!(fill.exec_size, size);
            assert_eq!(fill.exec_price_e6, PRICE);
        }
    }

    fn check_economics(&self, size: i128, bps: u64) {
        let filled = size != 0;
        let paid = if filled { fee(bps) } else { 0 };
        let accounts = self.portfolios.map(|key| self.env.portfolio_state(key));
        for actor in 0..2 {
            assert_eq!(
                accounts[actor].owner,
                self.owners[actor].pubkey().to_bytes()
            );
            assert_eq!(
                accounts[actor].capital.get(),
                u128::from(DEPOSITS[actor] + if filled && actor == 0 { PREFIX } else { 0 }) - paid
            );
            assert_eq!(accounts[actor].pnl.get(), 0);
            assert_eq!(
                self.env.token_amount(self.sources[actor]),
                if !filled && actor == 0 { PREFIX } else { 0 }
            );
            if filled {
                assert_eq!(
                    active_leg_for_asset(&accounts[actor], 0).basis_pos_q,
                    if actor == 0 { size } else { -size }
                );
            } else {
                assert!(!has_active_leg_for_asset(&accounts[actor], 0));
            }
        }
        assert_eq!(
            self.env
                .portfolio_matcher_config(self.portfolios[1])
                .trade_fee_cap_bps(),
            LP_CAP_BPS
        );
        let (_, group) = self.env.market_state();
        let vault = DEPOSITS.iter().sum::<u64>() + if filled { PREFIX } else { 0 };
        assert_eq!(group.assets[0].effective_price, PRICE);
        assert_eq!(group.assets[0].oi_eff_long_q, size.unsigned_abs());
        assert_eq!(group.assets[0].oi_eff_short_q, size.unsigned_abs());
        assert_eq!(group.insurance, 2 * paid);
        assert_eq!(&group.insurance_domain_budget[..2], &[paid; 2]);
        assert_eq!(group.c_tot, u128::from(vault) - 2 * paid);
        assert_eq!(group.vault, u128::from(vault));
        assert_eq!(self.env.token_amount(self.env.vault), vault);
        assert_market_stock_census(
            "retained single-CPI policy history",
            &group,
            &self.env.svm.get_account(&self.env.market).unwrap().data,
            &accounts,
            vault.into(),
        )
        .unwrap();
        assert_reservation_encumbrance_census(
            "retained single-CPI policy history",
            &group,
            &accounts,
        )
        .unwrap();
        let mint = Mint::unpack(&self.env.svm.get_account(&self.env.mint).unwrap().data).unwrap();
        assert_eq!(mint.supply, DEPOSITS.iter().sum::<u64>() + PREFIX);
        assert_eq!(mint.mint_authority, COption::None);
    }
}

#[test]
fn v16_retained_single_cpi_fee_consent_survives_policy_detours_and_funded_rollback() {
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).unwrap();
    let (mut simulation_cu, mut policy_cu, mut rejection_cu, mut success_cu) = (0, 0, 0, 0);
    let (mut worlds, mut simulations, mut rejections, mut retained_successes, mut fresh_successes) =
        (0, 0, 0, 0, 0);
    for signed_bps in [37, 99] {
        assert!(fee(signed_bps + 1) > fee(signed_bps));
        assert!(fee(signed_bps + 2) > fee(signed_bps + 1));
        assert!(signed_bps + 2 < u64::from(LP_CAP_BPS));
        for direction in [-1, 1] {
            for fresh_consent in [false, true] {
                let mut w = World::new(&matcher_bytes);
                let size = direction * QUANTITY;
                let trade = w.env.trade_cpi_ix(
                    w.portfolios[0],
                    w.portfolios[1],
                    0,
                    size,
                    signed_bps,
                    PRICE,
                );
                // Both alternatives are signed before every policy write. Only their CU
                // limits differ, so failed-transaction caching cannot mask a later check.
                let retained = [w.bundle(&trade, 0), w.bundle(&trade, 1)];
                let bytes = retained
                    .each_ref()
                    .map(|tx| bincode::serialize(tx).unwrap());
                assert_ne!(retained[0].signatures, retained[1].signatures);
                for tx in &retained {
                    assert_eq!(tx.message.header.num_required_signatures, 2);
                    simulation_cu = simulation_cu.max(w.simulate(tx));
                    simulations += 1;
                }
                let epochs = w.portfolios.map(|key| w.env.portfolio_position_epoch(key));
                let matcher_sequence = w.env.portfolio_matcher_sequence(w.portfolios[1]);
                w.check(0, 0);
                policy_cu = policy_cu.max(w.policy(signed_bps + 1, 10));
                w.check(0, 0);
                assert_eq!(w.bundle(&trade, 0), retained[0]);
                assert_eq!(bincode::serialize(&retained[0]).unwrap(), bytes[0]);
                rejection_cu =
                    rejection_cu.max(w.deliver(retained[0].clone(), true, &[], [1, 1, 0]));
                rejections += 1;
                w.check(0, 0);

                policy_cu = policy_cu.max(w.policy(signed_bps, 11));
                w.check(0, 0);
                simulation_cu = simulation_cu.max(w.simulate(&retained[1]));
                simulations += 1;
                assert_eq!(w.bundle(&trade, 1), retained[1]);
                assert_eq!(bincode::serialize(&retained[1]).unwrap(), bytes[1]);
                let (accepted, accepted_bps) = if fresh_consent {
                    policy_cu = policy_cu.max(w.policy(signed_bps + 2, 12));
                    w.check(0, 0);
                    assert_eq!(w.bundle(&trade, 1), retained[1]);
                    rejection_cu =
                        rejection_cu.max(w.deliver(retained[1].clone(), true, &[], [1, 1, 0]));
                    rejections += 1;
                    w.check(0, 0);
                    let mut fresh = trade.clone();
                    let ProgInstruction::TradeCpi { fee_bps, .. } = &mut fresh else {
                        unreachable!()
                    };
                    *fee_bps = signed_bps + 2;
                    let tx = w.bundle(&fresh, 1);
                    let mut expected_message = retained[1].message.clone();
                    expected_message.instructions[3].data = fresh.encode();
                    assert_eq!(
                        tx.message, expected_message,
                        "fresh consent changes only the signed fee field"
                    );
                    assert_ne!(tx.signatures, retained[1].signatures);
                    fresh_successes += 1;
                    (tx, signed_bps + 2)
                } else {
                    retained_successes += 1;
                    (retained[1].clone(), signed_bps)
                };
                assert_eq!(
                    w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                    epochs
                );
                assert_eq!(
                    w.env.portfolio_matcher_sequence(w.portfolios[1]),
                    matcher_sequence
                );
                let controls = w.env.control_sequences(0);
                let changed = [
                    w.env.market,
                    w.portfolios[0],
                    w.portfolios[1],
                    w.context,
                    w.env.vault,
                    w.sources[0],
                ];
                success_cu = success_cu.max(w.deliver(accepted, false, &changed, [2, 1, 1]));
                w.check(size, accepted_bps);
                assert_eq!(w.env.control_sequences(0), controls);
                assert_eq!(
                    w.env.portfolio_matcher_sequence(w.portfolios[1]),
                    matcher_sequence
                );
                for (key, epoch) in w.portfolios.into_iter().zip(epochs) {
                    assert_eq!(w.env.portfolio_position_epoch(key), epoch + 1);
                }
                worlds += 1;
            }
        }
    }
    assert_eq!(
        (
            worlds,
            simulations,
            rejections,
            retained_successes,
            fresh_successes
        ),
        (8, 24, 12, 4, 4)
    );
    eprintln!("INV-014 row 432: worlds={worlds}, simulations={simulations}, exact_rollbacks={rejections}, rolled_back_SPL_deposits={rejections}, retained_successes={retained_successes}, fresh_successes={fresh_successes}; max CU simulation={simulation_cu}, policy={policy_cu}, rejection={rejection_cu}, success={success_cu}");
}

#[derive(Debug, PartialEq, Eq)]
struct FeeNormalizedRouteEconomics {
    capital: [u128; 2],
    pnl: [i128; 2],
    positions: [i128; 2],
    position_epochs: [u64; 2],
    oi: [u128; 2],
    insurance: u128,
    domain_budgets: [u128; 2],
    capital_total: u128,
    vault: u128,
    tokens: [u64; 3],
}

impl World {
    fn normalized_route_economics(&self, paid: u128) -> FeeNormalizedRouteEconomics {
        let accounts = self.portfolios.map(|key| self.env.portfolio_state(key));
        let (_, group) = self.env.market_state();
        // Remove only the independently predicted route fee, never a measured
        // residual. The unnormalized accounts have already passed check_economics.
        FeeNormalizedRouteEconomics {
            capital: accounts
                .each_ref()
                .map(|account| account.capital.get() + paid),
            pnl: accounts.each_ref().map(|account| account.pnl.get()),
            positions: accounts
                .each_ref()
                .map(|account| active_leg_for_asset(account, 0).basis_pos_q),
            position_epochs: self
                .portfolios
                .map(|key| self.env.portfolio_position_epoch(key)),
            oi: [
                group.assets[0].oi_eff_long_q,
                group.assets[0].oi_eff_short_q,
            ],
            insurance: group.insurance - 2 * paid,
            domain_budgets: [
                group.insurance_domain_budget[0] - paid,
                group.insurance_domain_budget[1] - paid,
            ],
            capital_total: group.c_tot + 2 * paid,
            vault: group.vault,
            tokens: [
                self.env.token_amount(self.sources[0]),
                self.env.token_amount(self.sources[1]),
                self.env.token_amount(self.env.vault),
            ],
        }
    }
}

#[test]
fn v16_retained_cpi_and_direct_policy_histories_differ_only_by_explicit_route_fees() {
    const SIGNED_BPS: u64 = 37;
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).unwrap();
    let (mut simulation_cu, mut policy_cu, mut rollback_cu) = (0, 0, 0);
    let mut success_cu = [0; 2];
    let (mut worlds, mut simulations, mut rollbacks, mut retries) = (0, 0, 0, 0);
    for direction in [-1, 1] {
        let mut reference = None;
        for history in [[31, 0, 7], [7, 0, 31]] {
            for cpi in [false, true] {
                let mut w = World::new(&matcher_bytes);
                let size = direction * QUANTITY;
                let trade = if cpi {
                    w.env
                        .trade_cpi_ix(w.portfolios[0], w.portfolios[1], 0, size, SIGNED_BPS, PRICE)
                } else {
                    w.env.trade_no_cpi_ix(
                        w.portfolios[0],
                        w.portfolios[1],
                        0,
                        size,
                        PRICE,
                        SIGNED_BPS,
                    )
                };
                let instructions = w.bundle_instructions(&trade);
                let retained = w.sign(&instructions, 0);
                let bytes = bincode::serialize(&retained).unwrap();
                assert_eq!(
                    retained.message.header.num_required_signatures,
                    if cpi { 2 } else { 3 }
                );
                simulation_cu = simulation_cu.max(w.simulate(&retained));
                simulations += 1;
                let epochs = w.portfolios.map(|key| w.env.portfolio_position_epoch(key));
                let grant = w.env.portfolio_matcher_config(w.portfolios[1]);
                let grant_sequence = w.env.portfolio_matcher_sequence(w.portfolios[1]);
                let grant_expiry = w.env.portfolio_matcher_expiry(w.portfolios[1]);
                let request_sequence = w.env.market_state().0.matcher_req_seq;
                for (step, bps) in history.into_iter().enumerate() {
                    policy_cu = policy_cu.max(w.policy(bps, 10 + step as u32));
                    w.check_economics(0, 0);
                    assert_eq!(w.sign(&instructions, 0), retained);
                    simulation_cu = simulation_cu.max(w.simulate(&retained));
                    simulations += 1;
                }
                let controls = w.env.control_sequences(0);
                let temporary_policy = Instruction {
                    program_id: w.env.program_id,
                    accounts: vec![
                        AccountMeta::new(w.env.admin.pubkey(), true),
                        AccountMeta::new(w.env.market, false),
                    ],
                    data: ProgInstruction::UpdateTradeFeePolicy {
                        trade_fee_base_bps: 23,
                        policy_sequence: controls.trade_fee + 1,
                        authority_epoch: controls.authority_epoch,
                    }
                    .encode(),
                };
                // A duplicate policy suffix rejects only after the temporary
                // policy, SPL deposit and the original trade have succeeded.
                let failed = w.sign(
                    &[
                        temporary_policy.clone(),
                        instructions[0].clone(),
                        instructions[1].clone(),
                        temporary_policy,
                    ],
                    20,
                );
                rollback_cu = rollback_cu.max(w.deliver_with_error(
                    failed,
                    Some((
                        5,
                        InstructionError::Custom(PercolatorError::EngineStale as u32),
                    )),
                    &[],
                    [3, 1, usize::from(cpi)],
                ));
                rollbacks += 1;
                w.check_economics(0, 0);
                assert_eq!(w.env.control_sequences(0), controls);
                assert_eq!(w.env.market_state().0.trade_fee_base_bps, history[2]);
                assert_eq!(w.env.market_state().0.matcher_req_seq, request_sequence);
                assert_eq!(
                    w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                    epochs
                );
                assert_eq!(w.env.portfolio_matcher_config(w.portfolios[1]), grant);

                assert_eq!(w.sign(&instructions, 0), retained);
                assert_eq!(bincode::serialize(&retained).unwrap(), bytes);
                let mut changed = vec![
                    w.env.market,
                    w.portfolios[0],
                    w.portfolios[1],
                    w.env.vault,
                    w.sources[0],
                ];
                if cpi {
                    changed.push(w.context);
                }
                success_cu[usize::from(cpi)] = success_cu[usize::from(cpi)].max(w.deliver(
                    retained,
                    false,
                    &changed,
                    [2, 1, usize::from(cpi)],
                ));
                retries += 1;
                let charged_bps = if cpi { history[2] } else { SIGNED_BPS };
                assert!(charged_bps <= SIGNED_BPS);
                assert!(
                    fee(history[2]) < fee(SIGNED_BPS),
                    "route difference must be nonzero"
                );
                if cpi {
                    w.check(size, charged_bps);
                } else {
                    w.check_economics(size, charged_bps);
                }
                assert_eq!(w.env.control_sequences(0), controls);
                assert_eq!(
                    w.env.market_state().0.matcher_req_seq,
                    request_sequence + u64::from(cpi)
                );
                assert_eq!(
                    w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                    epochs.map(|epoch| epoch + 1)
                );
                let mut expected_grant = grant;
                expected_grant.control = state::next_portfolio_position_control(grant.control)
                    .unwrap()
                    .1;
                expected_grant.set_enabled(u8::from(cpi)).unwrap();
                assert_eq!(
                    w.env.portfolio_matcher_config(w.portfolios[1]),
                    expected_grant
                );
                assert_eq!(
                    w.env.portfolio_matcher_sequence(w.portfolios[1]),
                    grant_sequence
                );
                assert_eq!(
                    w.env.portfolio_matcher_expiry(w.portfolios[1]),
                    if cpi { grant_expiry } else { 0 }
                );
                let normalized = w.normalized_route_economics(fee(charged_bps));
                if let Some(reference) = &reference {
                    assert_eq!(
                        &normalized, reference,
                        "policy history and route preserve the fee-normalized economic endpoint"
                    );
                } else {
                    reference = Some(normalized);
                }
                worlds += 1;
            }
        }
    }
    assert_eq!((worlds, simulations, rollbacks, retries), (8, 32, 8, 8));
    eprintln!("INV-014 row 432 route histories: worlds={worlds}, simulations={simulations}, exact_post_fill_rollbacks={rollbacks}, exact_retained_retries={retries}; max CU simulation={simulation_cu}, policy={policy_cu}, rollback={rollback_cu}, direct_success={}, cpi_success={}", success_cu[0], success_cu[1]);
}
