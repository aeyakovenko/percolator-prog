//! INV-080: a second backing-funding CPI failure must restore the first transfer,
//! both newly created sidecars, rent, and the asset's shared top-up sequence.
//! Public SPL allowance repair permits the captured bundle to fund both domains
//! exactly once and return all provider principal without spending user capital.
//! INV-008/018/024/034/073 receive bounded replay, custody, and exit evidence.
//! This four-world test does not cover backing utilization, expiry, or all errors.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

fn execute(
    env: &mut V16CuEnv,
    instructions: &[Instruction],
    signers: &[&Keypair],
    framed: &[Pubkey],
    expected_error: Option<TransactionError>,
) -> litesvm::types::TransactionMetadata {
    env.svm.expire_blockhash();
    let mut all = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(CUSTODY_CU_LIMIT as u32),
    ];
    all.extend_from_slice(instructions);
    let mut all_signers = vec![&env.payer];
    all_signers.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &all,
        Some(&env.payer.pubkey()),
        &all_signers,
        env.svm.latest_blockhash(),
    );
    let fee = FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(framed);
    keys.sort_unstable();
    keys.dedup();
    keys.retain(|key| *key != env.payer.pubkey());
    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let mut payer_before = env.svm.get_account(&env.payer.pubkey()).unwrap();
    let result = env.svm.send_transaction(tx);
    let meta = if let Some(expected) = expected_error {
        let failure = result.expect_err("captured request must fail at the specified boundary");
        assert_eq!(failure.err, expected);
        for (key, account) in keys.iter().zip(before) {
            assert_eq!(env.svm.get_account(key), account, "exact rollback: {key}");
        }
        payer_before.lamports -= fee;
        assert_eq!(env.svm.get_account(&env.payer.pubkey()), Some(payer_before));
        failure.meta
    } else {
        result.expect("public continuation must succeed")
    };
    assert_cu_within(
        "backing bundle CPI retry",
        meta.compute_units_consumed,
        CUSTODY_CU_LIMIT,
    );
    meta
}

#[test]
fn v16_program_second_backing_cpi_failure_restores_sidecars_and_shared_intent_retry() {
    const AMOUNTS: [u64; 2] = [37, 41];
    const BACKING: u64 = AMOUNTS[0] + AMOUNTS[1];
    const CAPITAL: u64 = 103;
    let mut peak_cu = 0;
    for order in [[0usize, 1], [1, 0]] {
        // Keep a positive allowance after the first transfer: SPL clears a fully
        // consumed delegate, allowing the owner branch to authorize the next one.
        for remaining_allowance in [1, AMOUNTS[order[1]] - 1] {
            let mut env = inv018_public_spl_market(6);
            let admin = env.admin.insecure_clone();
            let owner = Keypair::new();
            env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
            let portfolio_key = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio_key,
                env.portfolio_account_len,
                env.program_id,
            );
            let portfolio = portfolio_key.pubkey();
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[&owner],
            )
            .unwrap();
            let source = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
            let user_token =
                create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
            let mut funding: Vec<_> = [(source, BACKING), (user_token, CAPITAL)]
                .map(|(dest, amount)| {
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &dest,
                        &admin.pubkey(),
                        &[],
                        amount,
                    )
                    .unwrap()
                })
                .into();
            funding.push(
                spl_token::instruction::set_authority(
                    &spl_token::ID,
                    &env.mint,
                    None,
                    spl_token::instruction::AuthorityType::MintTokens,
                    &admin.pubkey(),
                    &[],
                )
                .unwrap(),
            );
            send_raw_ixs(&mut env.svm, &env.payer, funding, &[&admin]).unwrap();
            env.send(
                env.deposit_ix(portfolio, CAPITAL.into()),
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(user_token, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owner],
            )
            .unwrap();
            let user_before = env.svm.get_account(&portfolio);
            let mint_before = env.svm.get_account(&env.mint);
            let sequences = env.control_sequences(0);
            let ledgers = [Keypair::new(), Keypair::new()];
            let ledger_len = state::backing_domain_ledger_account_len();
            let rent = env.svm.minimum_balance_for_rent_exemption(ledger_len);
            let framed = [
                env.market,
                env.vault,
                env.mint,
                source,
                user_token,
                owner.pubkey(),
                portfolio,
                ledgers[0].pubkey(),
                ledgers[1].pubkey(),
            ];
            let retained: Vec<_> = order
                .iter()
                .enumerate()
                .map(|(offset, &domain)| Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(source, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                        AccountMeta::new(ledgers[domain].pubkey(), false),
                    ],
                    data: ProgInstruction::TopUpBackingBucket {
                        authority_epoch: 0,
                        intent_id: sequences.backing_top_up + offset as u64 + 1,
                        market_id: env.asset_market_id(0),
                        domain: domain as u16,
                        backing_fee_bps: 0,
                        insurance_share_bps: 0,
                        amount: AMOUNTS[domain].into(),
                        expiry_slot: 100,
                    }
                    .encode(),
                })
                .collect();
            let mut bundle: Vec<_> = ledgers
                .iter()
                .map(|ledger| {
                    assert_eq!(env.svm.get_account(&ledger.pubkey()), None);
                    system_instruction::create_account(
                        &env.payer.pubkey(),
                        &ledger.pubkey(),
                        rent,
                        ledger_len as u64,
                        &env.program_id,
                    )
                })
                .collect();
            bundle.extend_from_slice(&retained);
            let signers = [&admin, &ledgers[0], &ledgers[1]];

            // The entire retained bundle is feasible before the external allowance changes.
            let mut simulated = vec![heap_ix(), cu_ix()];
            simulated.extend_from_slice(&bundle);
            let tx = Transaction::new_signed_with_payer(
                &simulated,
                Some(&env.payer.pubkey()),
                &[&env.payer, &admin, &ledgers[0], &ledgers[1]],
                env.svm.latest_blockhash(),
            );
            env.svm
                .simulate_transaction(tx.into())
                .expect("unrestricted bundle simulation");
            let allowance = AMOUNTS[order[0]] + remaining_allowance;
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::approve(
                    &spl_token::ID,
                    &source,
                    &admin.pubkey(),
                    &admin.pubkey(),
                    &[],
                    allowance,
                )
                .unwrap(),
                &[&admin],
            )
            .unwrap();
            let token = TokenAccount::unpack(&env.svm.get_account(&source).unwrap().data).unwrap();
            assert_eq!(
                (token.amount, token.delegate, token.delegated_amount),
                (BACKING, COption::Some(admin.pubkey()), allowance)
            );
            let failed = execute(
                &mut env,
                &bundle,
                &signers,
                &framed,
                Some(TransactionError::InstructionError(
                    5,
                    InstructionError::Custom(
                        spl_token::error::TokenError::InsufficientFunds as u32,
                    ),
                )),
            );
            let token_invoke = format!("Program {} invoke [2]", spl_token::ID);
            let token_success = format!("Program {} success", spl_token::ID);
            assert_eq!(
                failed
                    .logs
                    .iter()
                    .filter(|line| **line == token_invoke)
                    .count(),
                2
            );
            assert_eq!(
                failed
                    .logs
                    .iter()
                    .filter(|line| **line == token_success)
                    .count(),
                1
            );
            assert_eq!(
                failed
                    .logs
                    .iter()
                    .filter(|line| *line == "Program log: Instruction: Transfer")
                    .count(),
                2
            );
            assert_eq!(env.control_sequences(0), sequences);
            peak_cu = peak_cu.max(failed.compute_units_consumed);

            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::revoke(&spl_token::ID, &source, &admin.pubkey(), &[])
                    .unwrap(),
                &[&admin],
            )
            .unwrap();
            let payer_before = env.svm.get_account(&env.payer.pubkey()).unwrap().lamports;
            let paid = execute(&mut env, &bundle, &signers, &framed, None);
            peak_cu = peak_cu.max(paid.compute_units_consumed);
            assert_eq!(
                payer_before - env.svm.get_account(&env.payer.pubkey()).unwrap().lamports,
                2 * rent + 4 * FeeStructure::default().lamports_per_signature
            );
            let mut expected_sequences = sequences;
            expected_sequences.backing_top_up += 2;
            assert_eq!(env.control_sequences(0), expected_sequences);
            assert_eq!(env.token_amount(source), 0);
            assert_eq!(env.token_amount(env.vault), CAPITAL + BACKING);
            let group = env.market_state().1;
            assert_eq!(
                (group.vault, group.c_tot, group.insurance),
                ((CAPITAL + BACKING).into(), CAPITAL.into(), 0)
            );
            let expected_ledgers = std::array::from_fn::<_, 2, _>(|domain| {
                let expected = state::BackingDomainLedgerAccountV16 {
                    market_group: env.market.to_bytes(),
                    authority: admin.pubkey().to_bytes(),
                    domain: domain as u16,
                    total_principal_atoms: AMOUNTS[domain].into(),
                    total_deposited_atoms: AMOUNTS[domain].into(),
                    ..Default::default()
                };
                let account = env.svm.get_account(&ledgers[domain].pubkey()).unwrap();
                assert_eq!(
                    (account.owner, account.lamports, account.data.len()),
                    (env.program_id, rent, ledger_len)
                );
                assert_eq!(
                    state::read_backing_domain_ledger(&account.data).unwrap(),
                    expected
                );
                assert_eq!(
                    group.source_backing_buckets[domain].fresh_unliened_backing_num,
                    u128::from(AMOUNTS[domain]) * BOUND_SCALE
                );
                assert_eq!(
                    group.source_credit[domain].fresh_reserved_backing_num,
                    u128::from(AMOUNTS[domain]) * BOUND_SCALE
                );
                expected
            });

            for &domain in order.iter().rev() {
                let peer_before = env.svm.get_account(&ledgers[1 - domain].pubkey());
                let before = env.token_amount(source);
                let ix = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(source, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                        AccountMeta::new(ledgers[domain].pubkey(), false),
                    ],
                    data: ProgInstruction::WithdrawBackingBucket {
                        domain: domain as u16,
                        market_id: env.asset_market_id(0),
                        authority_epoch: 0,
                        amount: AMOUNTS[domain].into(),
                    }
                    .encode(),
                };
                peak_cu = peak_cu
                    .max(execute(&mut env, &[ix], &[&admin], &framed, None).compute_units_consumed);
                assert_eq!(env.token_amount(source) - before, AMOUNTS[domain]);
                let mut expected = expected_ledgers[domain];
                expected.total_principal_atoms = 0;
                expected.total_principal_withdrawn_atoms = AMOUNTS[domain].into();
                let account = env.svm.get_account(&ledgers[domain].pubkey()).unwrap();
                assert_eq!(
                    state::read_backing_domain_ledger(&account.data).unwrap(),
                    expected
                );
                assert_eq!(
                    env.svm.get_account(&ledgers[1 - domain].pubkey()),
                    peer_before
                );
            }
            // Refunded source balance makes both old top-ups financially executable;
            // their consumed shared sequence must still reject in either domain.
            for old in &retained {
                peak_cu = peak_cu.max(
                    execute(
                        &mut env,
                        &[old.clone()],
                        &[&admin],
                        &framed,
                        Some(TransactionError::InstructionError(
                            2,
                            InstructionError::Custom(PercolatorError::EngineStale as u32),
                        )),
                    )
                    .compute_units_consumed,
                );
            }
            assert_eq!(env.svm.get_account(&portfolio), user_before);
            assert_eq!(env.svm.get_account(&env.mint), mint_before);
            assert_eq!(env.token_amount(source), BACKING);
            assert_eq!(env.token_amount(env.vault), CAPITAL);
            let group = env.market_state().1;
            for domain in 0..2 {
                assert_eq!(
                    group.source_backing_buckets[domain].fresh_unliened_backing_num,
                    0
                );
                assert_eq!(group.source_credit[domain].fresh_reserved_backing_num, 0);
            }
            let withdrawal = Instruction {
                program_id: env.program_id,
                data: env.withdraw_ix(portfolio, CAPITAL.into()).encode(),
                accounts: vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(user_token, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            };
            peak_cu = peak_cu.max(
                execute(&mut env, &[withdrawal], &[&owner], &framed, None).compute_units_consumed,
            );
            let group = env.market_state().1;
            assert_eq!((group.vault, group.c_tot, group.insurance), (0, 0, 0));
            assert_eq!(env.token_amount(env.vault), 0);
            assert_eq!(env.token_amount(user_token), CAPITAL);
            assert_eq!(env.token_amount(source), BACKING);
            assert_eq!(env.control_sequences(0), expected_sequences);
        }
    }
    println!("INV-080 backing bundle: 4 worlds, 4 second-CPI rollbacks, 4 captured retries, 8 stale replays, 8 provider payouts, 4 owner exits; peak CU={peak_cu}");
}
