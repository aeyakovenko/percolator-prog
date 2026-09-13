//! INV-008: retained owner-authorized capital/insurance exchanges consume both
//! consent lanes even when the common wallet and custody return to their inputs.
//! INV-010/024/031/080/081: both orders and atomic/split delivery preserve typed
//! stock, optional telemetry, bystander capital and exact late-error rollback.
//! Eight flat Live histories, two exchanges, one SPL rail; rows 415/428 stay OPEN.
//! This checks ordinary authorized transitions, not arbitrary stock histories or
//! the complete INV-011 aggregate-budget / INV-064 policy obligations.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const CAPITAL: u128 = 101;
const INSURANCE: [u128; 2] = [13, 47];
const BYSTANDER: u128 = 103;
const SUPPLY: u64 = 265;
const CU_LIMIT: u32 = 400_000;

#[derive(Clone, Copy, Debug)]
enum Op {
    WithdrawCapital(u128),
    FundInsurance(u128),
    WithdrawInsurance(u128),
    DepositCapital(u128),
    Repair,
    ReturnRepair,
}

#[derive(Clone)]
struct Book {
    capital: u128,
    insurance: [u128; 2],
    wallet: u128,
    donor: u128,
    sequence: u64,
    controls: state::AssetControlSequencesV16,
    ledger: Option<state::InsuranceLedgerAccountV16>,
    observed: bool,
}

impl Book {
    fn apply(&mut self, op: Op, market: Pubkey, owner: Pubkey) {
        match op {
            Op::WithdrawCapital(amount) => {
                self.capital -= amount;
                self.wallet += amount;
                self.sequence += 1;
            }
            Op::DepositCapital(amount) => {
                self.wallet -= amount;
                self.capital += amount;
                self.sequence += 1;
            }
            Op::FundInsurance(amount) => {
                self.wallet -= amount;
                self.insurance[1] += amount;
                self.controls.insurance_top_up += 1;
            }
            Op::WithdrawInsurance(amount) => {
                let available = self.insurance.iter().sum::<u128>();
                assert!(amount <= available);
                if self.observed {
                    let ledger = self.ledger.get_or_insert(state::InsuranceLedgerAccountV16 {
                        market_group: market.to_bytes(),
                        authority: owner.to_bytes(),
                        last_observed_insurance_atoms: available,
                        ..state::InsuranceLedgerAccountV16::default()
                    });
                    if available >= ledger.last_observed_insurance_atoms {
                        ledger.cumulative_profit_atoms +=
                            available - ledger.last_observed_insurance_atoms;
                    } else {
                        ledger.cumulative_loss_atoms +=
                            ledger.last_observed_insurance_atoms - available;
                    }
                    ledger.last_observed_insurance_atoms = available - amount;
                    ledger.total_withdrawn_atoms += amount;
                }
                let long = amount.min(self.insurance[0]);
                self.insurance[0] -= long;
                self.insurance[1] -= amount - long;
                self.wallet += amount;
                self.controls.authority_epoch += 1;
            }
            Op::Repair => {
                self.donor -= 1;
                self.wallet += 1;
            }
            Op::ReturnRepair => {
                self.wallet -= 1;
                self.donor += 1;
            }
        }
    }

    fn accepts_stock(&self, capital: u128, insurance: [u128; 2]) -> bool {
        (capital, insurance) == (self.capital, self.insurance)
    }
}

#[test]
fn v16_retained_capital_insurance_exchange_preserves_typed_stock_and_atomic_retry() {
    let mut transactions = 0;
    let mut rollbacks = 0;
    let mut restored_transfers = 0;
    let mut peak_cu = 0;
    let mut outcomes = BTreeMap::new();
    for capital_first in [false, true] {
        for observed in [false, true] {
            for split in [false, true] {
                let context =
                    format!("capital_first={capital_first} ledger={observed} split={split}");
                let mut env = inv018_public_spl_market(6);
                let admin = env.admin.insecure_clone();
                let outsider = Keypair::new();
                env.svm.airdrop(&outsider.pubkey(), 1_000_000_000).unwrap();
                let owners = [&admin, &outsider];
                let portfolios = owners.map(|owner| {
                    let key = Keypair::new();
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
                    key.pubkey()
                });
                let wallets = owners.map(|owner| {
                    create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint)
                });
                let donor_key = Keypair::new();
                system_create_account_for_test(
                    &mut env.svm,
                    &env.payer,
                    &donor_key,
                    TokenAccount::LEN,
                    spl_token::ID,
                );
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::initialize_account3(
                        &spl_token::ID,
                        &donor_key.pubkey(),
                        &env.mint,
                        &admin.pubkey(),
                    )
                    .unwrap(),
                    &[],
                )
                .unwrap();
                let donor = donor_key.pubkey();
                for (destination, amount) in [(wallets[0], 161), (wallets[1], 103), (donor, 1)] {
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::mint_to(
                            &spl_token::ID,
                            &env.mint,
                            &destination,
                            &admin.pubkey(),
                            &[],
                            amount,
                        )
                        .unwrap(),
                        &[&admin],
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
                        &admin.pubkey(),
                        &[],
                    )
                    .unwrap(),
                    &[&admin],
                )
                .unwrap();
                for (actor, amount) in [CAPITAL, BYSTANDER].into_iter().enumerate() {
                    env.send(
                        env.deposit_ix(portfolios[actor], amount),
                        vec![
                            AccountMeta::new(owners[actor].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[actor], false),
                            AccountMeta::new(wallets[actor], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[owners[actor]],
                    )
                    .unwrap();
                }
                for (domain, amount) in INSURANCE.into_iter().enumerate() {
                    env.send(
                        ProgInstruction::TopUpInsuranceDomain {
                            domain: domain as u16,
                            market_id: env.asset_market_id(0),
                            authority_epoch: 0,
                            intent_id: 0,
                            amount,
                        },
                        vec![
                            AccountMeta::new(admin.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(wallets[0], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&admin],
                    )
                    .unwrap();
                }
                let ledger = Keypair::new();
                system_create_account_for_test(
                    &mut env.svm,
                    &env.payer,
                    &ledger,
                    state::insurance_ledger_account_len(),
                    env.program_id,
                );
                let blank_ledger = env.svm.get_account(&ledger.pubkey()).unwrap();
                let bystander = env.svm.get_account(&portfolios[1]).unwrap();
                let outsider_account = env.svm.get_account(&outsider.pubkey());
                let mint = env.svm.get_account(&env.mint).unwrap();
                assert_eq!(Mint::unpack(&mint.data).unwrap().supply, SUPPLY);
                assert_eq!(
                    Mint::unpack(&mint.data).unwrap().mint_authority,
                    COption::None
                );
                let token_keys = [wallets[0], wallets[1], donor, env.vault];
                let token_frames = token_keys.map(|key| env.svm.get_account(&key).unwrap());
                let portfolio_id = env.portfolio_id(portfolios[0]);
                let market_id = env.asset_market_id(0);
                let profile = state::read_asset_oracle_profile(
                    &env.svm.get_account(&env.market).unwrap().data,
                    0,
                )
                .unwrap();
                let mut book = Book {
                    capital: CAPITAL,
                    insurance: INSURANCE,
                    wallet: 0,
                    donor: 1,
                    sequence: env.portfolio_matcher_sequence(portfolios[0]),
                    controls: env.control_sequences(0),
                    ledger: None,
                    observed,
                };
                let check = |env: &V16CuEnv, book: &Book| {
                    let group = env.market_state().1;
                    let capital = env.portfolio_state(portfolios[0]);
                    assert!(
                        book.accepts_stock(
                            capital.capital.get(),
                            [
                                group.insurance_domain_budget[0],
                                group.insurance_domain_budget[1]
                            ]
                        ),
                        "typed stock: {context}"
                    );
                    assert_eq!(capital.pnl.get(), 0);
                    assert!(capital.active_bitmap.iter().all(|word| word.get() == 0));
                    assert_eq!(env.portfolio_id(portfolios[0]), portfolio_id);
                    assert_eq!(env.portfolio_matcher_sequence(portfolios[0]), book.sequence);
                    assert_eq!(env.control_sequences(0), book.controls);
                    assert_eq!(env.asset_market_id(0), market_id);
                    assert_eq!(
                        state::read_asset_oracle_profile(
                            &env.svm.get_account(&env.market).unwrap().data,
                            0
                        )
                        .unwrap(),
                        profile
                    );
                    let insurance = book.insurance.iter().sum::<u128>();
                    let custody = book.capital + BYSTANDER + insurance;
                    assert_eq!(
                        (group.c_tot, group.insurance, group.vault),
                        (book.capital + BYSTANDER, insurance, custody)
                    );
                    assert_eq!(group.insurance_domain_budget, book.insurance);
                    assert_eq!(group.insurance_domain_spent, [0; 2]);
                    assert_eq!(group.insurance_domain_budget_remaining_total, insurance);
                    assert_eq!(group.mode, MarketModeV16::Live);
                    assert_eq!(group.materialized_portfolio_count, 2);
                    assert_eq!(group.pnl_pos_tot, 0);
                    assert_eq!(group.source_claim_bound_total_num, 0);
                    assert_eq!(group.source_insurance_credit_reserved_total_atoms, 0);
                    assert_eq!(env.svm.get_account(&portfolios[1]).unwrap(), bystander);
                    assert_eq!(env.svm.get_account(&outsider.pubkey()), outsider_account);
                    let amounts = [book.wallet, 0, book.donor, custody];
                    assert_eq!(amounts.iter().sum::<u128>(), u128::from(SUPPLY));
                    for (index, key) in token_keys.into_iter().enumerate() {
                        let mut expected = token_frames[index].clone();
                        let mut token = TokenAccount::unpack(&expected.data).unwrap();
                        token.amount = u64::try_from(amounts[index]).unwrap();
                        TokenAccount::pack(token, &mut expected.data).unwrap();
                        assert_eq!(
                            env.svm.get_account(&key).unwrap(),
                            expected,
                            "token frame: {context}"
                        );
                    }
                    let mut expected = blank_ledger.clone();
                    if let Some(record) = &book.ledger {
                        state::init_insurance_ledger(&mut expected.data, record).unwrap();
                    }
                    assert_eq!(env.svm.get_account(&ledger.pubkey()).unwrap(), expected);
                    assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint);
                    let mut data = env.svm.get_account(&env.market).unwrap().data;
                    state::market_view_mut(&mut data)
                        .unwrap()
                        .1
                        .validate_shape()
                        .unwrap();
                };
                let instruction = |book: &Book, op| {
                    let (ix, portfolio, debit) = match op {
                        Op::WithdrawCapital(amount) => (
                            ProgInstruction::Withdraw {
                                portfolio_id,
                                expected_sequence: book.sequence,
                                amount,
                            },
                            true,
                            true,
                        ),
                        Op::DepositCapital(amount) => (
                            ProgInstruction::Deposit {
                                portfolio_id,
                                expected_sequence: book.sequence,
                                amount,
                            },
                            true,
                            false,
                        ),
                        Op::FundInsurance(amount) => (
                            ProgInstruction::TopUpInsuranceDomain {
                                domain: 1,
                                market_id,
                                authority_epoch: book.controls.authority_epoch,
                                intent_id: book.controls.insurance_top_up + 1,
                                amount,
                            },
                            false,
                            false,
                        ),
                        Op::WithdrawInsurance(amount) => (
                            ProgInstruction::WithdrawInsuranceAsset {
                                asset_index: 0,
                                market_id,
                                authority_epoch: book.controls.authority_epoch,
                                amount,
                            },
                            false,
                            true,
                        ),
                        Op::Repair | Op::ReturnRepair => {
                            return spl_token::instruction::transfer(
                                &spl_token::ID,
                                &if matches!(op, Op::Repair) {
                                    donor
                                } else {
                                    wallets[0]
                                },
                                &if matches!(op, Op::Repair) {
                                    wallets[0]
                                } else {
                                    donor
                                },
                                &admin.pubkey(),
                                &[],
                                1,
                            )
                            .unwrap()
                        }
                    };
                    let mut accounts = vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new(env.market, false),
                    ];
                    if portfolio {
                        accounts.push(AccountMeta::new(portfolios[0], false));
                    }
                    accounts.extend([
                        AccountMeta::new(wallets[0], false),
                        AccountMeta::new(env.vault, false),
                    ]);
                    if debit {
                        accounts.push(AccountMeta::new_readonly(env.vault_authority, false));
                    }
                    accounts.push(AccountMeta::new_readonly(spl_token::ID, false));
                    if observed && matches!(op, Op::WithdrawInsurance(_)) {
                        accounts.push(AccountMeta::new(ledger.pubkey(), false));
                    }
                    Instruction {
                        program_id: env.program_id,
                        accounts,
                        data: ix.encode(),
                    }
                };
                // All instruction payloads and signature-distinct delivery envelopes are
                // fixed before the history. Successful retries never refresh a guard.
                let mut requests = Vec::new();
                let mut retain = |ops: Vec<Op>, ixs: Vec<Instruction>, fails| {
                    let mut message = vec![
                        heap_ix(),
                        ComputeBudgetInstruction::set_compute_unit_limit(
                            CU_LIMIT - requests.len() as u32,
                        ),
                    ];
                    message.extend(ixs);
                    let tx = Transaction::new_signed_with_payer(
                        &message,
                        Some(&env.payer.pubkey()),
                        &[&env.payer, &admin],
                        env.svm.latest_blockhash(),
                    );
                    requests.push((ops, bincode::serialize(&tx).unwrap(), fails));
                };
                let mut planned = book.clone();
                for (round, (to_insurance, to_capital)) in
                    [(37, 23), (19, 41)].into_iter().enumerate()
                {
                    let capital_pair = [
                        Op::WithdrawCapital(to_insurance),
                        Op::FundInsurance(to_insurance),
                    ];
                    let insurance_pair = [
                        Op::WithdrawInsurance(to_capital),
                        Op::DepositCapital(to_capital),
                    ];
                    let pairs = if capital_first ^ (round == 1) {
                        [capital_pair, insurance_pair]
                    } else {
                        [insurance_pair, capital_pair]
                    };
                    let mut ops: Vec<_> = pairs.into_iter().flatten().collect();
                    let mut projected = planned.clone();
                    let mut ixs: Vec<_> = ops
                        .iter()
                        .map(|&op| {
                            let ix = instruction(&projected, op);
                            projected.apply(op, env.market, admin.pubkey());
                            ix
                        })
                        .collect();
                    assert_eq!(projected.wallet, 0);
                    ops.push(Op::ReturnRepair);
                    ixs.push(instruction(&projected, Op::ReturnRepair));
                    retain(ops.clone(), ixs.clone(), true);
                    retain(
                        vec![Op::Repair],
                        vec![instruction(&planned, Op::Repair)],
                        false,
                    );
                    planned.apply(Op::Repair, env.market, admin.pubkey());
                    if split {
                        for (&op, ix) in ops.iter().zip(&ixs) {
                            retain(vec![op], vec![ix.clone()], false);
                        }
                    } else {
                        retain(ops.clone(), ixs, false);
                    }
                    for op in ops {
                        planned.apply(op, env.market, admin.pubkey());
                    }
                }
                assert_eq!((planned.capital, planned.insurance), (109, [0, 52]));
                let exit = vec![
                    Op::WithdrawCapital(planned.capital),
                    Op::WithdrawInsurance(planned.insurance.iter().sum()),
                ];
                let exit_ixs = exit
                    .iter()
                    .map(|&op| {
                        let ix = instruction(&planned, op);
                        planned.apply(op, env.market, admin.pubkey());
                        ix
                    })
                    .collect::<Vec<_>>();
                if split {
                    for (&op, ix) in exit.iter().zip(&exit_ixs) {
                        retain(vec![op], vec![ix.clone()], false);
                    }
                } else {
                    retain(exit, exit_ixs, false);
                }
                let keys = [
                    env.market,
                    env.mint,
                    env.vault,
                    wallets[0],
                    wallets[1],
                    donor,
                    portfolios[0],
                    portfolios[1],
                    ledger.pubkey(),
                    admin.pubkey(),
                    outsider.pubkey(),
                    env.vault_authority,
                ];
                check(&env, &book);
                let mut signatures = BTreeSet::new();
                for (step, (ops, wire, fails)) in requests.into_iter().enumerate() {
                    let tx: Transaction = bincode::deserialize(&wire).unwrap();
                    tx.verify().unwrap();
                    assert_eq!(bincode::serialize(&tx).unwrap(), wire);
                    assert!(signatures.insert(tx.signatures[0]));
                    let before: BTreeMap<_, _> = keys
                        .iter()
                        .chain(&tx.message.account_keys)
                        .map(|key| (*key, env.svm.get_account(key)))
                        .collect();
                    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
                    payer.lamports -= FeeStructure::default().lamports_per_signature
                        * u64::from(tx.message.header.num_required_signatures);
                    let result = env.svm.send_transaction(tx);
                    let meta = if fails {
                        let failure = result
                            .expect_err("zero wallet cannot pay the final external SPL transfer");
                        assert_eq!(
                            failure.err,
                            TransactionError::InstructionError(
                                6,
                                InstructionError::Custom(
                                    spl_token::error::TokenError::InsufficientFunds as u32
                                )
                            ),
                            "{context} step={step}"
                        );
                        for (key, account) in before {
                            if key != env.payer.pubkey() {
                                assert_eq!(
                                    env.svm.get_account(&key),
                                    account,
                                    "rollback {context} step={step} key={key}"
                                );
                            }
                        }
                        rollbacks += 1;
                        restored_transfers += 4;
                        failure.meta
                    } else {
                        let meta = result.expect(
                            "retained current payload remains executable after wallet repair",
                        );
                        for &op in &ops {
                            book.apply(op, env.market, admin.pubkey());
                        }
                        meta
                    };
                    let wrappers = ops
                        .iter()
                        .filter(|op| !matches!(op, Op::Repair | Op::ReturnRepair))
                        .count();
                    assert_eq!(
                        meta.logs
                            .iter()
                            .filter(|line| **line == format!("Program {} success", env.program_id))
                            .count(),
                        wrappers
                    );
                    assert_eq!(
                        meta.logs
                            .iter()
                            .filter(|line| **line == format!("Program {} success", spl_token::ID))
                            .count(),
                        if fails { 4 } else { ops.len() }
                    );
                    assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
                    assert_cu_within(
                        "capital/insurance exchange",
                        meta.compute_units_consumed,
                        CU_LIMIT as u64,
                    );
                    peak_cu = peak_cu.max(meta.compute_units_consumed);
                    transactions += 1;
                    check(&env, &book);
                    if book.insurance[1] > 0 {
                        let wrong_insurance = [book.insurance[0], book.insurance[1] - 1];
                        assert_eq!(
                            book.capital + book.insurance.iter().sum::<u128>(),
                            book.capital + 1 + wrong_insurance.iter().sum::<u128>()
                        );
                        assert!(
                            !book.accepts_stock(book.capital + 1, wrong_insurance),
                            "conserved class misattribution must fail the stock oracle"
                        );
                    }
                }
                assert_eq!(
                    (book.capital, book.insurance, book.wallet, book.donor),
                    (0, [0, 0], 161, 1)
                );
                let ledger_totals = book.ledger.map(|mut record| {
                    record.market_group = [0; 32];
                    record.authority = [0; 32];
                    record
                });
                let outcome = (
                    book.capital,
                    book.insurance,
                    book.wallet,
                    book.donor,
                    book.sequence,
                    book.controls,
                    ledger_totals,
                );
                if let Some(atomic) = outcomes.insert((capital_first, observed), outcome.clone()) {
                    assert_eq!(atomic, outcome, "atomic and split delivery: {context}");
                }
            }
        }
    }
    assert_eq!((transactions, rollbacks, restored_transfers), (92, 16, 64));
    eprintln!("INV-008 capital/insurance exchange: 8 histories, {transactions} transactions, {rollbacks} exact rollbacks, {restored_transfers} restored SPL transfers, peak {peak_cu} CU");
}
