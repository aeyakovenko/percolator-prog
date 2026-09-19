//! INV-017/019/024/047/080: a valid matcher context can also sign as taker owner.
//! The pairwise substitution matrix uses an ordinary wallet, not this publicly
//! constructible dual-role account. Fees and SPL withdrawals measure entitlement.

use super::*;

#[test]
fn v16_public_matcher_context_signer_alias_preserves_fees_stock_and_replay() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
    use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

    const CAPITAL: u64 = 20_000;
    const PRICE: u64 = 100;
    const FEE_BPS: u64 = 100;
    const SIZE: i128 = 100 * POS_SCALE as i128;
    const FEE: u64 = 100; // 100 units * 100 quote * 100 bps.
    let mut peak_cu = 0;
    for batch in [false, true] {
        for alias in [false, true] {
            let mut env = inv018_public_spl_market_with_params(
                0,
                V16CuMarketParams {
                    trade_fee_base_bps: FEE_BPS,
                    ..V16CuMarketParams::default()
                },
            );
            let matcher = Pubkey::new_unique();
            env.svm.add_program(
                matcher,
                &std::fs::read(auth_matcher_program_path()).unwrap(),
            );
            let context_key = Keypair::new();
            let context = context_key.pubkey();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &context_key,
                MATCHER_CONTEXT_LEN,
                matcher,
            );
            let ordinary_owner = Keypair::new();
            let lp_owner = Keypair::new();
            let taker_owner = if alias { &context_key } else { &ordinary_owner };
            let owners = [taker_owner, &lp_owner];
            let mut portfolios = Vec::new();
            let mut tokens = Vec::new();
            for owner in owners {
                if env.svm.get_account(&owner.pubkey()).is_none() {
                    env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
                }
                let key = Keypair::new();
                system_create_account_for_test(
                    &mut env.svm,
                    &env.payer,
                    &key,
                    env.portfolio_account_len,
                    env.program_id,
                );
                let portfolio = key.pubkey();
                env.send(
                    ProgInstruction::InitPortfolio,
                    vec![
                        AccountMeta::new(owner.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolio, false),
                    ],
                    &[owner],
                )
                .unwrap();
                let token = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &token,
                        &env.admin.pubkey(),
                        &[],
                        CAPITAL,
                    )
                    .unwrap(),
                    &[&env.admin],
                )
                .unwrap();
                env.send(
                    env.deposit_ix(portfolio, CAPITAL.into()),
                    vec![
                        AccountMeta::new_readonly(owner.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolio, false),
                        AccountMeta::new(token, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[owner],
                )
                .unwrap();
                portfolios.push(portfolio);
                tokens.push(token);
            }
            let (taker, lp) = (portfolios[0], portfolios[1]);
            let delegate = matcher_delegate_key(
                &env.program_id,
                &env.market,
                &lp,
                &lp_owner.pubkey(),
                &matcher,
                &context,
            );
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                Instruction {
                    program_id: matcher,
                    accounts: vec![
                        AccountMeta::new_readonly(lp_owner.pubkey(), true),
                        AccountMeta::new_readonly(delegate, false),
                        AccountMeta::new(context, false),
                        AccountMeta::new_readonly(env.program_id, false),
                        AccountMeta::new_readonly(env.market, false),
                        AccountMeta::new_readonly(lp, false),
                    ],
                    data: vec![2],
                },
                &[&lp_owner],
            )
            .unwrap();
            env.set_matcher_config(matcher, &lp_owner, lp, context, delegate, 1);
            let instruction = if batch {
                env.batch_trade_cpi_ix_with_caps(
                    taker,
                    lp,
                    vec![BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q: SIZE,
                        fee_bps: FEE_BPS,
                        limit_price: PRICE,
                    }],
                    0,
                    FEE.into(),
                )
            } else {
                env.trade_cpi_ix(taker, lp, 0, SIZE, FEE_BPS, PRICE)
            };
            let retained = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new_readonly(taker_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(taker, false),
                    AccountMeta::new(lp, false),
                    AccountMeta::new_readonly(matcher, false),
                    AccountMeta::new(context, false),
                    AccountMeta::new_readonly(delegate, false),
                ],
                data: instruction.encode(),
            };
            let custody = [env.mint, env.vault, tokens[0], tokens[1]];
            let custody_before = account_alias_snapshot(&env, &custody);
            for replay in [false, true] {
                env.svm.expire_blockhash();
                let tx = Transaction::new_signed_with_payer(
                    &[heap_ix(), cu_ix(), retained.clone()],
                    Some(&env.payer.pubkey()),
                    &[&env.payer, taker_owner],
                    env.svm.latest_blockhash(),
                );
                let roles = &tx.message.instructions[2].accounts;
                assert_eq!(roles[0] == roles[5], alias);
                assert_eq!(tx.message.is_signer(roles[5] as usize), alias);
                assert!(tx.message.is_writable(roles[5] as usize));
                let mut frame = tx.message.account_keys.clone();
                frame.extend(custody);
                frame.extend([env.admin.pubkey(), lp_owner.pubkey()]);
                frame.sort_unstable();
                frame.dedup();
                frame.retain(|key| *key != env.payer.pubkey());
                let before = account_alias_snapshot(&env, &frame);
                let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
                payer.lamports -=
                    2 * solana_sdk::fee::FeeStructure::default().lamports_per_signature;
                let result = env.svm.send_transaction(tx);
                let meta = if replay {
                    let failed =
                        result.expect_err("consumed position epoch rejects unchanged wire bytes");
                    assert_eq!(
                        failed.err,
                        TransactionError::InstructionError(
                            2,
                            InstructionError::Custom(PercolatorError::EngineStale as u32),
                        )
                    );
                    assert_eq!(account_alias_snapshot(&env, &frame), before);
                    failed.meta
                } else {
                    let meta = result.expect("a real signer/context alias is authorized");
                    assert!(meta.logs.contains(&format!("Program {matcher} success")));
                    for (key, account) in frame.iter().zip(before) {
                        assert_eq!(
                            env.svm.get_account(key).map(|a| a.lamports),
                            account.map(|a| a.lamports)
                        );
                    }
                    meta
                };
                assert_eq!(env.svm.get_account(&env.payer.pubkey()), Some(payer));
                assert_eq!(account_alias_snapshot(&env, &custody), custody_before);
                assert_cu_within(
                    "signer/context alias CPI and replay",
                    meta.compute_units_consumed,
                    TRADE_CU_LIMIT,
                );
                peak_cu = peak_cu.max(meta.compute_units_consumed);
            }
            for portfolio in &portfolios {
                assert_eq!(
                    env.portfolio_state(*portfolio).capital.get(),
                    (CAPITAL - FEE) as u128
                );
            }
            assert_eq!(env.market_state().1.assets[0].oi_eff_long_q, SIZE as u128);
            assert_eq!(env.market_state().1.assets[0].oi_eff_short_q, SIZE as u128);
            let context_after_cpi = env.svm.get_account(&context).unwrap();
            let cu = env.trade_asset_with_cu(
                0,
                taker_owner,
                taker,
                &lp_owner,
                lp,
                -SIZE,
                PRICE,
                FEE_BPS,
            );
            assert_cu_within("signer/context alias no-CPI close", cu, TRADE_CU_LIMIT);
            peak_cu = peak_cu.max(cu);
            for ((owner, portfolio), token) in owners.into_iter().zip(portfolios).zip(&tokens) {
                assert!(!has_active_leg_for_asset(
                    &env.portfolio_state(portfolio),
                    0
                ));
                let cu = env
                    .send(
                        env.withdraw_ix(portfolio, (CAPITAL - 2 * FEE).into()),
                        vec![
                            AccountMeta::new_readonly(owner.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolio, false),
                            AccountMeta::new(*token, false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[owner],
                    )
                    .expect("each owner can spend exactly its post-fee entitlement");
                assert_cu_within("signer/context alias owner payout", cu, CUSTODY_CU_LIMIT);
                peak_cu = peak_cu.max(cu);
                assert_eq!(env.token_amount(*token), CAPITAL - 2 * FEE);
                assert_eq!(env.portfolio_state(portfolio).capital.get(), 0);
            }
            assert_eq!(env.svm.get_account(&context), Some(context_after_cpi));
            assert_eq!(env.token_amount(env.vault), 4 * FEE);
            assert_eq!(env.market_state().1.insurance, (4 * FEE) as u128);
            let supply = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                .unwrap()
                .supply;
            assert_eq!(supply, 2 * CAPITAL);
            assert_eq!(
                env.token_amount(tokens[0])
                    + env.token_amount(tokens[1])
                    + env.token_amount(env.vault),
                supply
            );
        }
    }
    println!("INV-017 signer/context alias: 4 fills, 4 exact replay rollbacks, 8 owner payouts; peak CU={peak_cu}");
}
