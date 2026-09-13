//! INV-008/064: Live insurance debits consume consent independently of telemetry.
//! Publicly funded stock, retained signed variants, complete rollback and fresh
//! continuations are checked against an input-derived book. No terminal routes.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const INITIAL: [u128; 4] = [13, 47, 19, 61];
const DEBIT: u128 = 23;
const REFILL: u128 = 43;
const SUPPLY: u64 = 183;

struct Book {
    budgets: [u128; 4],
    paid: [u128; 2],
    source: u128,
    controls: [state::AssetControlSequencesV16; 2],
    ledger: Option<state::InsuranceLedgerAccountV16>,
}

impl Book {
    fn debit(&mut self, env: &V16CuEnv, asset: usize, amount: u128, observed: bool) {
        let available = self.budgets[2 * asset] + self.budgets[2 * asset + 1];
        assert!(amount <= available);
        if observed {
            let record = self.ledger.get_or_insert(state::InsuranceLedgerAccountV16 {
                market_group: env.market.to_bytes(),
                authority: env.admin.pubkey().to_bytes(),
                total_principal_atoms: 0,
                total_deposited_atoms: 0,
                total_withdrawn_atoms: 0,
                cumulative_profit_atoms: 0,
                cumulative_loss_atoms: 0,
                last_observed_insurance_atoms: available,
            });
            if available >= record.last_observed_insurance_atoms {
                record.cumulative_profit_atoms += available - record.last_observed_insurance_atoms;
            } else {
                record.cumulative_loss_atoms += record.last_observed_insurance_atoms - available;
            }
            record.last_observed_insurance_atoms = available - amount;
            record.total_withdrawn_atoms += amount;
        }
        let long = amount.min(self.budgets[2 * asset]);
        self.budgets[2 * asset] -= long;
        self.budgets[2 * asset + 1] -= amount - long;
        self.paid[asset] += amount;
        self.controls[asset].authority_epoch += 1;
    }

    fn refill(&mut self, asset: usize) {
        self.budgets[2 * asset + 1] += REFILL;
        self.source -= REFILL;
        self.controls[asset].insurance_top_up += 1;
    }
}

#[test]
fn v16_live_insurance_debit_consumes_epoch_across_ledger_and_destination_retries() {
    let mut peak_cu = 0;
    let mut rollbacks = 0;
    let mut transactions = 0;
    for asset in 0..2 {
        for observed_first in [false, true] {
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_portfolio_assets: 2,
                    ..V16CuMarketParams::default()
                },
            );
            let admin = env.admin.insecure_clone();
            let source = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
            let destinations = [Keypair::new(), Keypair::new()];
            for destination in &destinations {
                system_create_account_for_test(
                    &mut env.svm,
                    &env.payer,
                    destination,
                    TokenAccount::LEN,
                    spl_token::ID,
                );
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::initialize_account3(
                        &spl_token::ID,
                        &destination.pubkey(),
                        &env.mint,
                        &admin.pubkey(),
                    )
                    .unwrap(),
                    &[],
                )
                .unwrap();
            }
            let destinations = destinations.map(|key| key.pubkey());
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &source,
                    &admin.pubkey(),
                    &[],
                    SUPPLY,
                )
                .unwrap(),
                &[&admin],
            )
            .unwrap();
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
            for (domain, amount) in INITIAL.into_iter().enumerate() {
                env.send(
                    ProgInstruction::TopUpInsuranceDomain {
                        domain: domain as u16,
                        market_id: env.asset_market_id(domain as u16 / 2),
                        authority_epoch: 0,
                        intent_id: 0,
                        amount,
                    },
                    vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(source, false),
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
            let empty_ledger = env.svm.get_account(&ledger.pubkey()).unwrap();
            let mint = env.svm.get_account(&env.mint).unwrap();
            assert_eq!(Mint::unpack(&mint.data).unwrap().supply, SUPPLY);
            assert_eq!(
                Mint::unpack(&mint.data).unwrap().mint_authority,
                COption::None
            );
            let profiles = [0, 1].map(|i| {
                state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, i)
                    .unwrap()
            });
            let market_ids = [env.asset_market_id(0), env.asset_market_id(1)];
            let tokens = [source, destinations[0], destinations[1], env.vault];
            let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
            let mut book = Book {
                budgets: INITIAL,
                paid: [0; 2],
                source: REFILL,
                controls: [env.control_sequences(0), env.control_sequences(1)],
                ledger: None,
            };
            let check = |env: &V16CuEnv, book: &Book| {
                let group = env.market_state().1;
                let remaining = book.budgets.iter().sum::<u128>();
                assert_eq!(group.mode, MarketModeV16::Live);
                assert_eq!(group.insurance_domain_budget, book.budgets);
                assert_eq!(group.insurance_domain_spent, [0; 4]);
                assert_eq!(group.insurance_domain_budget_remaining_total, remaining);
                assert_eq!(
                    (group.insurance, group.vault, group.c_tot),
                    (remaining, remaining, 0)
                );
                assert_eq!(group.materialized_portfolio_count, 0);
                assert_eq!(group.pnl_pos_tot, 0);
                assert_eq!(group.source_claim_bound_total_num, 0);
                assert_eq!(group.source_insurance_credit_reserved_total_atoms, 0);
                for i in 0..2 {
                    assert_eq!(
                        env.control_sequences(i),
                        book.controls[i],
                        "successful Live debit must consume its bound epoch, asset {i}"
                    );
                    assert_eq!(env.asset_market_id(i as u16), market_ids[i]);
                    assert_eq!(
                        state::read_asset_oracle_profile(
                            &env.svm.get_account(&env.market).unwrap().data,
                            i
                        )
                        .unwrap(),
                        profiles[i]
                    );
                }
                let amounts = [book.source, book.paid[0], book.paid[1], remaining];
                assert_eq!(amounts.iter().sum::<u128>(), u128::from(SUPPLY));
                for (i, key) in tokens.into_iter().enumerate() {
                    let mut expected = token_frames[i].clone();
                    let mut token = TokenAccount::unpack(&expected.data).unwrap();
                    token.amount = u64::try_from(amounts[i]).unwrap();
                    TokenAccount::pack(token, &mut expected.data).unwrap();
                    assert_eq!(env.svm.get_account(&key).unwrap(), expected);
                }
                let mut expected_ledger = empty_ledger.clone();
                if let Some(record) = &book.ledger {
                    state::init_insurance_ledger(&mut expected_ledger.data, record).unwrap();
                }
                assert_eq!(
                    env.svm.get_account(&ledger.pubkey()).unwrap(),
                    expected_ledger
                );
                assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint);
                let mut snapshot = env.svm.get_account(&env.market).unwrap().data;
                state::market_view_mut(&mut snapshot)
                    .unwrap()
                    .1
                    .validate_shape()
                    .unwrap();
            };
            let withdrawal = |target: usize, epoch, amount, observed, destination| {
                let mut accounts = vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(destination, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ];
                if observed {
                    accounts.push(AccountMeta::new(ledger.pubkey(), false));
                }
                Instruction {
                    program_id: env.program_id,
                    accounts,
                    data: ProgInstruction::WithdrawInsuranceAsset {
                        asset_index: target as u16,
                        market_id: market_ids[target],
                        authority_epoch: epoch,
                        amount,
                    }
                    .encode(),
                }
            };
            let epoch = book.controls[asset].authority_epoch;
            let retained = withdrawal(asset, epoch, DEBIT, observed_first, destinations[asset]);
            let alternate = withdrawal(
                asset,
                epoch,
                DEBIT,
                !observed_first,
                destinations[1 - asset],
            );
            let fresh = withdrawal(asset, epoch + 1, DEBIT, true, destinations[asset]);
            let peer = withdrawal(
                1 - asset,
                book.controls[1 - asset].authority_epoch,
                7,
                false,
                destinations[1 - asset],
            );
            let residual = withdrawal(
                asset,
                epoch + 2,
                INITIAL[2 * asset] + INITIAL[2 * asset + 1] + REFILL - 2 * DEBIT,
                true,
                destinations[asset],
            );
            let refill = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(source, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: ProgInstruction::TopUpInsuranceDomain {
                    domain: (2 * asset + 1) as u16,
                    market_id: market_ids[asset],
                    authority_epoch: epoch + 1,
                    intent_id: book.controls[asset].insurance_top_up + 1,
                    amount: REFILL,
                }
                .encode(),
            };
            let late_error = spl_token::instruction::transfer(
                &spl_token::ID,
                &source,
                &destinations[asset],
                &admin.pubkey(),
                &[],
                SUPPLY + 1,
            )
            .unwrap();
            let signed = |env: &V16CuEnv, ixs: &[Instruction], nonce| {
                let mut message = vec![
                    heap_ix(),
                    ComputeBudgetInstruction::set_compute_unit_limit(300_000 - nonce),
                ];
                message.extend_from_slice(ixs);
                Transaction::new_signed_with_payer(
                    &message,
                    Some(&env.payer.pubkey()),
                    &[&env.payer, &admin],
                    env.svm.latest_blockhash(),
                )
            };
            // Every delivery envelope is retained before any debit commits. Later
            // successes use explicitly predicted successor epochs, never rebinding.
            let requests = [
                (
                    vec![retained.clone(), late_error.clone()],
                    Some((3, spl_token::error::TokenError::InsufficientFunds as u32)),
                    1,
                ),
                (vec![retained.clone()], None, 1),
                (
                    vec![alternate.clone()],
                    Some((2, PercolatorError::EngineStale as u32)),
                    0,
                ),
                (
                    vec![fresh.clone(), fresh.clone()],
                    Some((3, PercolatorError::EngineStale as u32)),
                    1,
                ),
                (
                    vec![refill.clone(), fresh.clone(), retained.clone()],
                    Some((4, PercolatorError::EngineStale as u32)),
                    2,
                ),
                (
                    vec![refill.clone(), fresh.clone(), late_error],
                    Some((4, spl_token::error::TokenError::InsufficientFunds as u32)),
                    2,
                ),
                (vec![refill, fresh.clone()], None, 2),
                (
                    vec![retained],
                    Some((2, PercolatorError::EngineStale as u32)),
                    0,
                ),
                (
                    vec![alternate],
                    Some((2, PercolatorError::EngineStale as u32)),
                    0,
                ),
                (
                    vec![fresh],
                    Some((2, PercolatorError::EngineStale as u32)),
                    0,
                ),
                (vec![peer], None, 1),
                (vec![residual], None, 1),
            ];
            let requests: Vec<_> = requests
                .into_iter()
                .enumerate()
                .map(|(i, (ixs, error, prefix))| {
                    let tx = signed(&env, &ixs, i as u32 + 1);
                    (bincode::serialize(&tx).unwrap(), error, prefix)
                })
                .collect();
            let keys = [
                env.market,
                env.mint,
                env.vault,
                source,
                destinations[0],
                destinations[1],
                ledger.pubkey(),
                admin.pubkey(),
                env.vault_authority,
            ];
            check(&env, &book);
            for (step, (wire, error, prefix)) in requests.into_iter().enumerate() {
                let tx: Transaction = bincode::deserialize(&wire).unwrap();
                tx.verify().unwrap();
                assert_eq!(bincode::serialize(&tx).unwrap(), wire);
                let before: BTreeMap<_, _> = keys
                    .iter()
                    .chain(&tx.message.account_keys)
                    .map(|key| (*key, env.svm.get_account(key)))
                    .collect();
                let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
                payer.lamports -= FeeStructure::default().lamports_per_signature
                    * u64::from(tx.message.header.num_required_signatures);
                let result = env.svm.send_transaction(tx);
                let meta = if let Some((index, code)) = error {
                    let failure =
                        result.expect_err("retained or late-error request must roll back");
                    assert_eq!(
                        failure.err,
                        TransactionError::InstructionError(index, InstructionError::Custom(code)),
                        "step {step}"
                    );
                    for (key, account) in before {
                        if key != env.payer.pubkey() {
                            assert_eq!(
                                env.svm.get_account(&key),
                                account,
                                "rollback step {step}, {key}"
                            );
                        }
                    }
                    rollbacks += 1;
                    failure.meta
                } else {
                    let meta =
                        result.expect("current consent must remain executable after rollback");
                    match step {
                        1 => book.debit(&env, asset, DEBIT, observed_first),
                        6 => {
                            book.refill(asset);
                            book.debit(&env, asset, DEBIT, true);
                        }
                        10 => book.debit(&env, 1 - asset, 7, false),
                        11 => {
                            let remainder = book.budgets[2 * asset] + book.budgets[2 * asset + 1];
                            book.debit(&env, asset, remainder, true);
                        }
                        _ => unreachable!(),
                    }
                    meta
                };
                assert_eq!(
                    meta.logs
                        .iter()
                        .filter(|line| **line == format!("Program {} success", env.program_id))
                        .count(),
                    prefix
                );
                assert_eq!(
                    meta.logs
                        .iter()
                        .filter(|line| **line == format!("Program {} success", spl_token::ID))
                        .count(),
                    prefix
                );
                assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
                assert_cu_within(
                    "Live debit consumption",
                    meta.compute_units_consumed,
                    300_000,
                );
                peak_cu = peak_cu.max(meta.compute_units_consumed);
                transactions += 1;
                check(&env, &book);
            }
            assert_eq!(book.budgets[2 * asset..2 * asset + 2], [0, 0]);
            assert_eq!(
                book.paid[asset],
                INITIAL[2 * asset] + INITIAL[2 * asset + 1] + REFILL
            );
        }
    }
    eprintln!("INV-008/064 Live debit: 4 histories, {transactions} transactions, {rollbacks} exact rollbacks, peak {peak_cu} CU");
}
