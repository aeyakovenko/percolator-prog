//! INV-008/010/011/024/031/064: a failed withdrawal does not consume consent,
//! but a committed cross-rail retry cannot spend subsequently replenished stock.
//!
//! Unlike the reserve-replacement control, the first secondary payout reaches
//! Withdraw's late custody-balance check after its engine debit and sequence advance.
//! Its rejection must also undo a successful backing top-up CPI and lazy ledger
//! initialization. The unchanged intent then succeeds through the primary rail.
//! Replenishment separately creates capital, backing, and custody-only surplus;
//! every ordering must preserve their attribution and keep both old rails stale.
//! A later partial payout tests successful SPL-prefix rollback and alternate-rail
//! recovery. This is not row428's insurance withdrawal/top-up round trip: there
//! is no insurance debit, authority change, injected state, or cache-only retry.
//! Row415 remains OPEN; this bounded portfolio history does not certify reserve
//! withdrawal stock binding or arbitrary retained histories.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::{
    inv018_create_public_spl_mint, inv018_public_spl_market,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const ORDERS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

#[test]
fn v16_underfunded_withdrawal_retry_preserves_replenished_stock_across_quote_rails() {
    const BYSTANDER: u64 = 103;
    let mut transactions = 0;
    let mut rejected = 0;
    let mut peak_cu = 0;
    for amount in [1u64, 37] {
        for order in ORDERS {
            let redeposit_amount = 2 * amount + 1;
            let backing_amount = amount + 3;
            let donation = amount + 1;
            let mut env = inv018_public_spl_market(6);
            let admin = env.admin.insecure_clone();
            let secondary =
                inv018_create_public_spl_mint(&mut env.svm, &env.payer, admin.pubkey(), 6);
            env.update_base_unit_mints_with_cu(env.mint, secondary);
            let mints = [env.mint, secondary];
            let vaults = [
                env.vault,
                create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, secondary),
            ];
            let owners = [Keypair::new(), Keypair::new()];
            let wallets = [owners[0].pubkey(), owners[1].pubkey(), admin.pubkey()];
            let tokens = wallets.map(|wallet| {
                mints.map(|mint| create_ata_for_test(&mut env.svm, &env.payer, wallet, mint))
            });
            let portfolios = std::array::from_fn::<_, 2, _>(|actor| {
                env.svm.airdrop(&wallets[actor], 1_000_000_000).unwrap();
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
                        AccountMeta::new(wallets[actor], true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(key.pubkey(), false),
                    ],
                    &[&owners[actor]],
                )
                .unwrap();
                key.pubkey()
            });
            let ledger_key = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &ledger_key,
                state::backing_domain_ledger_account_len(),
                env.program_id,
            );
            let ledger = ledger_key.pubkey();
            let blank_ledger = env.svm.get_account(&ledger);
            let mut funding: Vec<_> = [
                (0, tokens[0][0], amount + redeposit_amount),
                (0, tokens[1][0], BYSTANDER),
                (0, tokens[2][0], backing_amount),
                (1, tokens[0][1], donation),
                (1, vaults[1], amount - 1),
            ]
            .map(|(rail, token, atoms)| {
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &mints[rail],
                    &token,
                    &admin.pubkey(),
                    &[],
                    atoms,
                )
                .unwrap()
            })
            .into();
            funding.extend(mints.map(|mint| {
                spl_token::instruction::set_authority(
                    &spl_token::ID,
                    &mint,
                    None,
                    spl_token::instruction::AuthorityType::MintTokens,
                    &admin.pubkey(),
                    &[],
                )
                .unwrap()
            }));
            send_raw_ixs(&mut env.svm, &env.payer, funding, &[&admin]).unwrap();
            let deposit_accounts = |actor: usize| {
                vec![
                    AccountMeta::new(wallets[actor], true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                    AccountMeta::new(tokens[actor][0], false),
                    AccountMeta::new(vaults[0], false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ]
            };
            let deposits = [deposit_accounts(0), deposit_accounts(1)];
            for (actor, atoms) in [amount, BYSTANDER].into_iter().enumerate() {
                env.send(
                    env.deposit_ix(portfolios[actor], atoms.into()),
                    deposits[actor].clone(),
                    &[&owners[actor]],
                )
                .unwrap();
            }
            let ids = portfolios.map(|key| env.portfolio_id(key));
            let epochs = portfolios.map(|key| env.portfolio_position_epoch(key));
            let controls = env.control_sequences(0);
            let mint_frames = mints.map(|key| env.svm.get_account(&key).unwrap());
            let bystander_frame = env.svm.get_account(&portfolios[1]);
            let withdrawal = |sequence, rail: usize| Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(wallets[0], true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[0], false),
                    AccountMeta::new(tokens[0][rail], false),
                    AccountMeta::new(vaults[rail], false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: ProgInstruction::Withdraw {
                    portfolio_id: ids[0],
                    expected_sequence: sequence,
                    amount: amount.into(),
                }
                .encode(),
            };
            let retained = [withdrawal(1, 0), withdrawal(1, 1)];
            let fresh = [withdrawal(3, 0), withdrawal(3, 1)];
            assert_eq!(retained[0].data, retained[1].data);
            let (backing_fee_bps, insurance_share_bps) = env.backing_fee_policy(1);
            let backing = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(tokens[2][0], false),
                    AccountMeta::new(vaults[0], false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(ledger, false),
                ],
                data: ProgInstruction::TopUpBackingBucket {
                    domain: 1,
                    market_id: env.asset_market_id(0),
                    intent_id: controls.backing_top_up + 1,
                    authority_epoch: controls.authority_epoch,
                    backing_fee_bps,
                    insurance_share_bps,
                    amount: backing_amount.into(),
                    expiry_slot: 100,
                }
                .encode(),
            };
            let replenishment = [
                Instruction {
                    program_id: env.program_id,
                    accounts: deposits[0].clone(),
                    data: ProgInstruction::Deposit {
                        portfolio_id: ids[0],
                        expected_sequence: 2,
                        amount: redeposit_amount.into(),
                    }
                    .encode(),
                },
                backing.clone(),
                spl_token::instruction::transfer(
                    &spl_token::ID,
                    &tokens[0][1],
                    &vaults[1],
                    &wallets[0],
                    &[],
                    donation,
                )
                .unwrap(),
            ];
            let ordered: Vec<_> = order.map(|index| replenishment[index].clone()).into();
            let frame: Vec<_> = portfolios
                .into_iter()
                .chain(mints)
                .chain(vaults)
                .chain(tokens.into_iter().flatten())
                .chain(wallets)
                .chain([env.market, ledger, env.vault_authority])
                .collect();
            let mut nonce = 0;
            let mut signatures = BTreeSet::new();
            let mut execute = |env: &mut V16CuEnv,
                               ixs: Vec<Instruction>,
                               expected: Option<(u8, PercolatorError)>,
                               successes: [usize; 2]| {
                nonce += 1;
                let mut message = vec![
                    heap_ix(),
                    ComputeBudgetInstruction::set_compute_unit_limit(1_400_000 - nonce),
                ];
                message.extend(ixs);
                let mut signers = vec![&env.payer];
                for signer in [&owners[0], &admin] {
                    if message
                        .iter()
                        .flat_map(|ix| &ix.accounts)
                        .any(|meta| meta.is_signer && meta.pubkey == signer.pubkey())
                    {
                        signers.push(signer);
                    }
                }
                let tx = Transaction::new_signed_with_payer(
                    &message,
                    Some(&env.payer.pubkey()),
                    &signers,
                    env.svm.latest_blockhash(),
                );
                tx.verify().unwrap();
                assert!(
                    signatures.insert(tx.signatures[0]),
                    "signature-distinct retry"
                );
                let keys: BTreeSet<_> = frame.iter().chain(&tx.message.account_keys).collect();
                let before: Vec<_> = keys
                    .iter()
                    .map(|key| (**key, env.svm.get_account(key)))
                    .collect();
                let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
                payer.lamports -= FeeStructure::default().lamports_per_signature
                    * u64::from(tx.message.header.num_required_signatures);
                let result = env.svm.send_transaction(tx);
                let meta = if let Some((index, error)) = expected {
                    let failure =
                        result.expect_err("rejected route must restore every stock class");
                    assert_eq!(
                        failure.err,
                        TransactionError::InstructionError(
                            2 + index,
                            InstructionError::Custom(error as u32)
                        ),
                        "{:?}",
                        failure.meta.logs
                    );
                    for (key, account) in before {
                        if key != env.payer.pubkey() {
                            assert_eq!(
                                env.svm.get_account(&key),
                                account,
                                "exact rollback at {key}"
                            );
                        }
                    }
                    rejected += 1;
                    failure.meta
                } else {
                    result.expect("unconsumed consent must make public progress")
                };
                for (program, count) in [env.program_id, spl_token::ID].into_iter().zip(successes) {
                    assert_eq!(
                        meta.logs
                            .iter()
                            .filter(|line| **line == format!("Program {program} success"))
                            .count(),
                        count,
                        "expected wrapper/SPL prefix must complete"
                    );
                }
                assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
                assert!(meta.compute_units_consumed > 0);
                assert_cu_within(
                    "INV-008 underfunded rail",
                    meta.compute_units_consumed,
                    CUSTODY_CU_LIMIT,
                );
                peak_cu = peak_cu.max(meta.compute_units_consumed);
                transactions += 1;
            };
            // Model inputs are public deposits, payouts and donations, never decoded deltas.
            let check = |env: &V16CuEnv, paid: [u64; 2], refilled: bool, withdrawals: u64| {
                let deposited = u64::from(refilled) * redeposit_amount;
                let donated = u64::from(refilled) * donation;
                let backing = u128::from(u64::from(refilled) * backing_amount);
                let capital = u128::from(amount + deposited - paid.iter().sum::<u64>());
                let group = env.market_state().1;
                assert_eq!(group.mode, MarketModeV16::Live);
                assert_eq!(
                    (group.c_tot, group.vault, group.insurance),
                    (
                        capital + u128::from(BYSTANDER),
                        capital + u128::from(BYSTANDER) + backing,
                        0
                    )
                );
                assert_eq!(
                    group.source_credit[1].fresh_reserved_backing_num,
                    backing * BOUND_SCALE
                );
                assert_eq!(group.source_credit[0].fresh_reserved_backing_num, 0);
                assert_eq!(group.source_backing_buckets[1].utilization_fee_earnings, 0);
                assert_eq!(group.assets[0].oi_eff_long_q, 0);
                assert_eq!(group.assets[0].oi_eff_short_q, 0);
                assert_domain_budget_remaining_total_consistent(&group, "INV-008 underfunded rail");
                for actor in 0..2 {
                    let portfolio = env.portfolio_state(portfolios[actor]);
                    assert_eq!(
                        portfolio.capital.get(),
                        if actor == 0 {
                            capital
                        } else {
                            BYSTANDER.into()
                        }
                    );
                    assert_eq!(portfolio.pnl.get(), 0);
                    assert_eq!(portfolio.cancel_deposit_escrow.get(), 0);
                    assert_eq!(env.portfolio_id(portfolios[actor]), ids[actor]);
                    assert_eq!(
                        env.portfolio_position_epoch(portfolios[actor]),
                        epochs[actor]
                    );
                    assert_eq!(
                        env.portfolio_matcher_sequence(portfolios[actor]),
                        1 + if actor == 0 {
                            withdrawals + u64::from(refilled)
                        } else {
                            0
                        }
                    );
                }
                assert_eq!(env.svm.get_account(&portfolios[1]), bystander_frame);
                let mut expected_controls = controls;
                expected_controls.backing_top_up += u64::from(refilled);
                assert_eq!(env.control_sequences(0), expected_controls);
                if refilled {
                    assert_eq!(
                        state::read_backing_domain_ledger(
                            &env.svm.get_account(&ledger).unwrap().data
                        )
                        .unwrap(),
                        state::BackingDomainLedgerAccountV16 {
                            market_group: env.market.to_bytes(),
                            authority: admin.pubkey().to_bytes(),
                            domain: 1,
                            total_principal_atoms: backing,
                            total_deposited_atoms: backing,
                            ..state::BackingDomainLedgerAccountV16::default()
                        }
                    );
                } else {
                    assert_eq!(env.svm.get_account(&ledger), blank_ledger);
                }
                let expected_wallets = [
                    [
                        redeposit_amount + paid[0] - deposited,
                        donation + paid[1] - donated,
                    ],
                    [0, 0],
                    [backing_amount - backing as u64, 0],
                ];
                let reserves = [
                    amount + BYSTANDER + deposited + backing as u64 - paid[0],
                    amount - 1 + donated - paid[1],
                ];
                for rail in 0..2 {
                    assert_eq!(
                        env.svm.get_account(&mints[rail]).unwrap(),
                        mint_frames[rail]
                    );
                    let mint = Mint::unpack(&mint_frames[rail].data).unwrap();
                    assert_eq!(mint.mint_authority, COption::None);
                    let mut census = 0;
                    for (key, owner, atoms) in (0..3)
                        .map(|actor| {
                            (
                                tokens[actor][rail],
                                wallets[actor],
                                expected_wallets[actor][rail],
                            )
                        })
                        .chain([(vaults[rail], env.vault_authority, reserves[rail])])
                    {
                        let account = env.svm.get_account(&key).unwrap();
                        assert_eq!(account.owner, spl_token::ID);
                        let token = TokenAccount::unpack(&account.data).unwrap();
                        assert_eq!(
                            (token.mint, token.owner, token.amount),
                            (mints[rail], owner, atoms)
                        );
                        census += token.amount;
                    }
                    assert_eq!(census, mint.supply, "complete mint-{rail} custody census");
                }
                assert_eq!(u128::from(reserves[0]), group.vault + u128::from(paid[1]));
                assert_eq!(paid.iter().sum::<u64>(), withdrawals * amount);
            };

            check(&env, [0, 0], false, 0);
            execute(
                &mut env,
                vec![backing.clone(), retained[1].clone()],
                Some((1, PercolatorError::InvalidTokenAccount)),
                [1, 1],
            );
            check(&env, [0, 0], false, 0);
            execute(&mut env, vec![retained[0].clone()], None, [1, 1]);
            check(&env, [amount, 0], false, 1);

            // Both rails were retained before the first debit. Each aborted refill
            // reaches a now-funded old suffix and must undo all three stock changes.
            for old in &retained {
                let mut bundle = ordered.clone();
                bundle.push(old.clone());
                execute(
                    &mut env,
                    bundle,
                    Some((3, PercolatorError::EngineStale)),
                    [2, 3],
                );
                check(&env, [amount, 0], false, 1);
            }
            execute(&mut env, ordered, None, [2, 3]);
            check(&env, [amount, 0], true, 1);
            for old in &retained {
                execute(
                    &mut env,
                    vec![old.clone()],
                    Some((0, PercolatorError::EngineStale)),
                    [0, 0],
                );
                check(&env, [amount, 0], true, 1);
            }
            // The current payout is partial. Aborting it must restore the whole
            // allowance, even though a real secondary SPL transfer has completed.
            execute(
                &mut env,
                vec![fresh[1].clone(), retained[0].clone()],
                Some((1, PercolatorError::EngineStale)),
                [1, 1],
            );
            check(&env, [amount, 0], true, 1);
            execute(
                &mut env,
                fresh.to_vec(),
                Some((1, PercolatorError::EngineStale)),
                [1, 1],
            );
            check(&env, [amount, 0], true, 1);
            execute(&mut env, vec![fresh[1].clone()], None, [1, 1]);
            check(&env, [amount, amount], true, 2);
            assert!(env.portfolio_state(portfolios[0]).capital.get() > u128::from(amount));
            for old in retained.into_iter().chain(fresh) {
                execute(
                    &mut env,
                    vec![old],
                    Some((0, PercolatorError::EngineStale)),
                    [0, 0],
                );
                check(&env, [amount, amount], true, 2);
            }
            assert_eq!(signatures.len(), 14);
        }
    }
    assert_eq!((transactions, rejected), (168, 132));
    println!("INV-008 underfunded rail: 12 histories, {transactions} transactions, {rejected} exact rollbacks, peak {peak_cu} CU; row415 OPEN");
}
