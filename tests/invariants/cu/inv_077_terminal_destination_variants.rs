//! INV-018/021/025/069/070/073/077/078/080/081: fixed-supply terminal stock with
//! an admin-owned non-ATA sweep destination carrying existing SPL authorities.
//! Administrative retirement requires the market authority; external token disposal
//! requires the named owner/delegate/close authority. No user claims are present.

use super::*;

#[test]
fn v16_program_terminal_sweep_preserves_destination_authorities_and_exact_disposal() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market;
    use solana_sdk::{
        fee::FeeStructure, instruction::InstructionError, transaction::TransactionError,
    };

    const BACKING: u64 = 307;
    const SURPLUS: u64 = 17;
    const EXISTING: u64 = 11;
    const VAULT_EXTRA: u64 = 43;
    const DEST_EXTRA: u64 = 59;
    const EXPIRY: u64 = 5;
    const CU_LIMIT: u64 = 150_000;

    for (delegated, separate_closer) in [(true, false), (false, true), (true, true)] {
        for split in [false, true] {
            let mut env = inv018_public_spl_market(6);
            let admin = env.admin.insecure_clone();
            let delegate = Keypair::new();
            let closer = Keypair::new();
            for signer in [&delegate, &closer] {
                env.svm.airdrop(&signer.pubkey(), 1_000_000_000).unwrap();
            }
            let destination = Keypair::new();
            let sink = Keypair::new();
            let funding = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
            assert_ne!(destination.pubkey(), funding, "exercise non-ATA custody");
            let watched = [
                env.market,
                env.mint,
                env.vault,
                destination.pubkey(),
                sink.pubkey(),
                funding,
                admin.pubkey(),
                delegate.pubkey(),
                closer.pubkey(),
                env.vault_authority,
            ];
            let mut peak_cu = env.init_market_cu;
            assert_cu_within("variant InitMarket", peak_cu, CU_LIMIT);
            let mut submit =
                |env: &mut V16CuEnv,
                 instructions: Vec<Instruction>,
                 signers: &[&Keypair],
                 expected_error: Option<(u8, InstructionError)>| {
                    env.svm.expire_blockhash();
                    let mut ixs = vec![
                        heap_ix(),
                        ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT as u32),
                    ];
                    ixs.extend(instructions);
                    let mut all_signers = vec![&env.payer];
                    all_signers.extend_from_slice(signers);
                    let tx = Transaction::new_signed_with_payer(
                        &ixs,
                        Some(&env.payer.pubkey()),
                        &all_signers,
                        env.svm.latest_blockhash(),
                    );
                    assert!(bincode::serialize(&tx).unwrap().len() <= 1232);
                    let fee = u64::from(tx.message.header.num_required_signatures)
                        * FeeStructure::default().lamports_per_signature;
                    let mut keys = tx.message.account_keys.clone();
                    keys.extend(watched);
                    keys.sort_unstable();
                    keys.dedup();
                    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
                    let result = env.svm.send_transaction(tx);
                    let cu = if let Some((index, error)) = expected_error {
                        let failure = result.expect_err("the terminal bundle must roll back");
                        assert_eq!(
                            failure.err,
                            TransactionError::InstructionError(index, error)
                        );
                        for (key, mut account) in keys.into_iter().zip(before) {
                            if key == env.payer.pubkey() {
                                account.as_mut().unwrap().lamports -= fee;
                            }
                            assert_eq!(env.svm.get_account(&key), account, "exact rollback {key}");
                        }
                        failure.meta.compute_units_consumed
                    } else {
                        result
                            .expect("public conformance continuation")
                            .compute_units_consumed
                    };
                    assert_cu_within("terminal destination variant transaction", cu, CU_LIMIT);
                    peak_cu = peak_cu.max(cu);
                    fee
                };

            let token_rent = env
                .svm
                .minimum_balance_for_rent_exemption(TokenAccount::LEN);
            let mut create = Vec::new();
            for (account, extra) in [(&destination, DEST_EXTRA), (&sink, 0)] {
                create.push(system_instruction::create_account(
                    &env.payer.pubkey(),
                    &account.pubkey(),
                    token_rent + extra,
                    TokenAccount::LEN as u64,
                    &spl_token::ID,
                ));
                create.push(
                    spl_token::instruction::initialize_account3(
                        &spl_token::ID,
                        &account.pubkey(),
                        &env.mint,
                        &admin.pubkey(),
                    )
                    .unwrap(),
                );
            }
            submit(&mut env, create, &[&destination, &sink], None);
            let mut fund = vec![
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &funding,
                    &admin.pubkey(),
                    &[],
                    BACKING + SURPLUS + EXISTING,
                )
                .unwrap(),
                spl_token::instruction::set_authority(
                    &spl_token::ID,
                    &env.mint,
                    None,
                    spl_token::instruction::AuthorityType::MintTokens,
                    &admin.pubkey(),
                    &[],
                )
                .unwrap(),
            ];
            if delegated {
                fund.push(
                    spl_token::instruction::approve(
                        &spl_token::ID,
                        &destination.pubkey(),
                        &delegate.pubkey(),
                        &admin.pubkey(),
                        &[],
                        EXISTING,
                    )
                    .unwrap(),
                );
            }
            if separate_closer {
                fund.push(
                    spl_token::instruction::set_authority(
                        &spl_token::ID,
                        &destination.pubkey(),
                        Some(&closer.pubkey()),
                        spl_token::instruction::AuthorityType::CloseAccount,
                        &admin.pubkey(),
                        &[],
                    )
                    .unwrap(),
                );
            }
            submit(&mut env, fund, &[&admin], None);
            env.svm.warp_to_slot(1);
            let backing_cu = env.top_up_backing_bucket_from_admin_token_with_cu(
                funding,
                1,
                BACKING.into(),
                EXPIRY,
            );
            assert_cu_within("variant backing funding", backing_cu, CU_LIMIT);
            let transfer = |source: Pubkey, dest: Pubkey, signer: Pubkey, amount: u64| {
                spl_token::instruction::transfer(
                    &spl_token::ID,
                    &source,
                    &dest,
                    &signer,
                    &[],
                    amount,
                )
                .unwrap()
            };
            let transfers = vec![
                transfer(funding, env.vault, admin.pubkey(), SURPLUS),
                transfer(funding, destination.pubkey(), admin.pubkey(), EXISTING),
                system_instruction::transfer(&admin.pubkey(), &env.vault, VAULT_EXTRA),
            ];
            submit(&mut env, transfers, &[&admin], None);
            let resolve_cu = env.resolve();
            assert_cu_within("variant ResolveMarket", resolve_cu, CU_LIMIT);

            let close = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new(destination.pubkey(), false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(env.mint, false),
                ],
                data: ProgInstruction::CloseSlab {
                    authority_epoch: env.control_sequences(0).authority_epoch,
                }
                .encode(),
            };
            let assert_stock = |env: &V16CuEnv, expired: bool| {
                let market = env.svm.get_account(&env.market).unwrap();
                let (_, group) = env.market_state();
                assert_eq!(group.mode, MarketModeV16::Resolved);
                assert_eq!(
                    (group.c_tot, group.insurance, group.vault),
                    (0, 0, BACKING.into())
                );
                assert_eq!(group.materialized_portfolio_count, 0);
                let bucket = group.source_backing_buckets[1];
                assert_eq!(
                    bucket.status,
                    if expired {
                        BackingBucketStatusV16::Expired
                    } else {
                        BackingBucketStatusV16::Fresh
                    }
                );
                assert_eq!(
                    bucket.fresh_unliened_backing_num,
                    if expired {
                        0
                    } else {
                        u128::from(BACKING) * BOUND_SCALE
                    }
                );
                assert_eq!(
                    group.source_credit[1].fresh_reserved_backing_num,
                    if expired {
                        0
                    } else {
                        u128::from(BACKING) * BOUND_SCALE
                    }
                );
                crate::support::fuzz_model::assert_market_stock_census(
                    "destination variant stock",
                    &group,
                    &market.data,
                    &[],
                    BACKING.into(),
                )
                .unwrap();
                crate::support::fuzz_model::assert_reservation_encumbrance_census(
                    "destination variant labels",
                    &group,
                    &[],
                )
                .unwrap();
                assert_eq!(env.token_amount(env.vault), BACKING + SURPLUS);
                assert_eq!(env.token_amount(destination.pubkey()), EXISTING);
            };
            assert_stock(&env, false);
            env.svm.warp_to_slot(EXPIRY - 1);
            let early = vec![
                system_instruction::transfer(&admin.pubkey(), &env.vault, 13),
                close.clone(),
            ];
            submit(
                &mut env,
                early,
                &[&admin],
                Some((
                    3,
                    InstructionError::Custom(PercolatorError::EngineLockActive as u32),
                )),
            );

            env.svm.warp_to_slot(EXPIRY);
            let before_normalization = watched.map(|key| env.svm.get_account(&key));
            let mut payer_expected = env.svm.get_account(&env.payer.pubkey()).unwrap();
            payer_expected.lamports -= submit(&mut env, vec![close.clone()], &[&admin], None);
            assert_eq!(
                env.svm.get_account(&env.payer.pubkey()),
                Some(payer_expected)
            );
            let mut expected = watched.map(|key| env.svm.get_account(&key));
            for index in 1..watched.len() {
                assert_eq!(expected[index], before_normalization[index]);
            }
            let mut market_metadata = expected[0].clone().unwrap();
            assert_ne!(
                market_metadata.data,
                before_normalization[0].as_ref().unwrap().data
            );
            market_metadata.data = before_normalization[0].as_ref().unwrap().data.clone();
            assert_eq!(Some(market_metadata), before_normalization[0]);
            assert_stock(&env, true);
            let mint = Mint::unpack(&expected[1].as_ref().unwrap().data).unwrap();
            assert_eq!(mint.supply, BACKING + SURPLUS + EXISTING);
            assert_eq!(
                (mint.mint_authority, mint.freeze_authority),
                (COption::None, COption::None)
            );
            assert_eq!(mint.decimals, 6);
            for (index, amount, extra) in [
                (2, BACKING + SURPLUS, VAULT_EXTRA),
                (3, EXISTING, DEST_EXTRA),
                (4, 0, 0),
                (5, 0, 0),
            ] {
                let account = expected[index].as_ref().unwrap();
                let token = TokenAccount::unpack(&account.data).unwrap();
                assert_eq!(account.owner, spl_token::ID);
                assert_eq!(account.lamports, token_rent + extra);
                assert_eq!(
                    (token.mint, token.amount, token.state, token.is_native),
                    (env.mint, amount, AccountState::Initialized, COption::None)
                );
                assert_eq!(
                    token.owner,
                    if index == 2 {
                        env.vault_authority
                    } else {
                        admin.pubkey()
                    }
                );
                assert_eq!(
                    token.delegate,
                    if index == 3 && delegated {
                        COption::Some(delegate.pubkey())
                    } else {
                        COption::None
                    }
                );
                assert_eq!(
                    token.delegated_amount,
                    if index == 3 && delegated { EXISTING } else { 0 }
                );
                assert_eq!(
                    token.close_authority,
                    if index == 3 && separate_closer {
                        COption::Some(closer.pubkey())
                    } else {
                        COption::None
                    }
                );
            }

            let spender = if delegated { &delegate } else { &admin };
            let rent_authority = if separate_closer { &closer } else { &admin };
            let disposal = vec![
                close,
                transfer(
                    destination.pubkey(),
                    sink.pubkey(),
                    spender.pubkey(),
                    EXISTING,
                ),
                transfer(destination.pubkey(), sink.pubkey(), admin.pubkey(), SURPLUS),
                spl_token::instruction::close_account(
                    &spl_token::ID,
                    &destination.pubkey(),
                    &rent_authority.pubkey(),
                    &rent_authority.pubkey(),
                    &[],
                )
                .unwrap(),
            ];
            let mut all_signers = vec![&admin];
            if delegated {
                all_signers.push(&delegate);
            }
            if separate_closer {
                all_signers.push(&closer);
            }
            // The suffix fails only after retirement, both token transfers and both
            // custody closes. The complete frame includes all transaction accounts.
            let mut rejected = disposal.clone();
            rejected.push(transfer(
                sink.pubkey(),
                funding,
                admin.pubkey(),
                EXISTING + SURPLUS + 1,
            ));
            submit(
                &mut env,
                rejected,
                &all_signers,
                Some((
                    6,
                    InstructionError::Custom(
                        spl_token::error::TokenError::InsufficientFunds as u32,
                    ),
                )),
            );
            assert_stock(&env, true);

            let tombstone_rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            let mut payer_expected = env.svm.get_account(&env.payer.pubkey()).unwrap();
            for step in 0..disposal.len() {
                if split {
                    let signer = match step {
                        1 => spender,
                        3 => rent_authority,
                        _ => &admin,
                    };
                    payer_expected.lamports -=
                        submit(&mut env, vec![disposal[step].clone()], &[signer], None);
                } else if step == 0 {
                    payer_expected.lamports -=
                        submit(&mut env, disposal.clone(), &all_signers, None);
                }
                match step {
                    0 => {
                        let refund = expected[0].as_ref().unwrap().lamports - tombstone_rent
                            + token_rent
                            + VAULT_EXTRA;
                        expected[6].as_mut().unwrap().lamports += refund;
                        let market = expected[0].as_mut().unwrap();
                        market.lamports = tombstone_rent;
                        market.data =
                            crate::support::v16_svm::closed_market_tombstone_data().to_vec();
                        expected[2] = None;
                        let account = expected[1].as_mut().unwrap();
                        let mut mint = mint;
                        mint.supply = EXISTING + SURPLUS;
                        Mint::pack(mint, &mut account.data).unwrap();
                        let account = expected[3].as_mut().unwrap();
                        let mut token = TokenAccount::unpack(&account.data).unwrap();
                        token.amount = EXISTING + SURPLUS;
                        TokenAccount::pack(token, &mut account.data).unwrap();
                    }
                    1 | 2 => {
                        let amount = if step == 1 { EXISTING } else { SURPLUS };
                        let account = expected[3].as_mut().unwrap();
                        let mut token = TokenAccount::unpack(&account.data).unwrap();
                        token.amount -= amount;
                        if step == 1 && delegated {
                            token.delegated_amount = 0;
                            token.delegate = COption::None;
                        }
                        TokenAccount::pack(token, &mut account.data).unwrap();
                        let account = expected[4].as_mut().unwrap();
                        let mut token = TokenAccount::unpack(&account.data).unwrap();
                        token.amount += amount;
                        TokenAccount::pack(token, &mut account.data).unwrap();
                    }
                    3 => {
                        expected[if separate_closer { 8 } else { 6 }]
                            .as_mut()
                            .unwrap()
                            .lamports += token_rent + DEST_EXTRA;
                        expected[3] = None;
                    }
                    _ => unreachable!(),
                }
                if split || step == disposal.len() - 1 {
                    for (key, account) in watched.iter().zip(&expected) {
                        let actual = env.svm.get_account(key);
                        if account.is_none() {
                            assert!(
                                actual.is_none_or(
                                    |a| a.lamports == 0 && a.data.iter().all(|byte| *byte == 0)
                                ),
                                "closed custody {key}"
                            );
                        } else {
                            assert_eq!(actual, *account, "disposition step {step}: {key}");
                        }
                    }
                    assert_eq!(
                        env.svm.get_account(&env.payer.pubkey()),
                        Some(payer_expected.clone())
                    );
                    assert_closed_market_tombstone(&env.svm.get_account(&env.market).unwrap());
                }
            }
            assert_eq!(env.token_amount(sink.pubkey()), EXISTING + SURPLUS);
            println!("INV-070/077 destination delegated={delegated}, separate_closer={separate_closer}, split={split}: retired={BACKING}, swept={SURPLUS}, retained_allowance={}, close_calls=2, exact_rollbacks=2, peak_CU={}", if delegated { EXISTING } else { 0 }, peak_cu.max(backing_cu).max(resolve_cu));
        }
    }
}
