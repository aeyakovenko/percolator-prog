//! Scope O: retire booked residue after Scope H's fee/loss/recredit completion.
//! Paid claims, native escheat, donated surplus and rent have separate recipients.

use super::*;

fn checked_land(
    env: &mut V16CuEnv,
    ixs: &[Instruction],
    signers: &[&Keypair],
    tracked: &[Pubkey],
    allowed: &[Pubkey],
    rent: u64,
    refund: Option<(Pubkey, u64)>,
    rejection: Option<(u8, PercolatorError)>,
) -> u64 {
    let mut keys = tracked.to_vec();
    keys.push(env.payer.pubkey());
    keys.extend(
        ixs.iter()
            .flat_map(|ix| ix.accounts.iter().map(|a| a.pubkey)),
    );
    keys.sort_unstable();
    keys.dedup();
    let total = |env: &V16CuEnv| -> u128 {
        keys.iter()
            .filter_map(|key| env.svm.get_account(key))
            .map(|account| u128::from(account.lamports))
            .sum()
    };
    let before = total(env);
    let cu = land(env, ixs, signers, tracked, allowed, rent, refund, rejection);
    let fee =
        (1 + signers.len()) as u128 * u128::from(FeeStructure::default().lamports_per_signature);
    assert_eq!(total(env), before - fee, "complete lamport conservation");
    assert_cu_within("Scope O booked residue continuation", cu, LIMIT);
    cu
}

fn assert_absent(env: &V16CuEnv, key: Pubkey) {
    assert!(env.svm.get_account(&key).is_none_or(|account| {
        account.lamports == 0
            && account.data.is_empty()
            && account.owner == solana_sdk::system_program::ID
            && !account.executable
    }));
}

fn run_cleanup(native: bool) {
    let schedules: &[(u64, bool)] = if native {
        &[(0, false), (19, false), (19, true)]
    } else {
        &[(0, false)]
    };
    let availability: &[bool] = if native { &[false, true] } else { &[false] };
    let (mut worlds, mut repairs, mut rollbacks, mut peak) = (0, 0, 0, 0);
    for retained in [SPENT + 1, 101] {
        for &(donation, sync) in schedules {
            for &missing in availability {
                let (world, insurer, admin_token) = terminal_fee_loss_world_with_quote(native);
                let TerminalEarningsWorld {
                    mut env,
                    admin,
                    incumbent,
                    successor,
                    wallets,
                    tokens,
                    portfolios,
                    mint_frame,
                } = world;
                let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
                for signer in [&incumbent, &successor] {
                    let balance = env.svm.get_account(&signer.pubkey()).unwrap().lamports;
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        system_instruction::transfer(
                            &signer.pubkey(),
                            &env.payer.pubkey(),
                            balance,
                        ),
                        &[signer],
                    )
                    .unwrap();
                    assert_absent(&env, signer.pubkey());
                }
                drop((incumbent, successor));
                let ledger = Keypair::new();
                system_create_account_for_test(
                    &mut env.svm,
                    &env.payer,
                    &ledger,
                    state::backing_domain_ledger_account_len(),
                    env.program_id,
                );
                let ledger = ledger.pubkey();
                let departure = Pubkey::new_unique();
                let tracked: Vec<_> = [
                    env.market,
                    env.vault,
                    env.mint,
                    ledger,
                    admin.pubkey(),
                    admin_token,
                    departure,
                ]
                .into_iter()
                .chain(wallets)
                .chain(tokens)
                .chain(portfolios)
                .collect();
                assert!(!wallets.contains(&admin.pubkey()));
                assert!(!wallets.contains(&env.payer.pubkey()));
                let frames = Frames {
                    wallets: wallets.map(|key| env.svm.get_account(&key)),
                    tokens: token_frames,
                    absent_tokens: tokens.map(|key| env.svm.get_account(&key)),
                    vault: env.svm.get_account(&env.vault).unwrap(),
                    market: env.svm.get_account(&env.market).unwrap(),
                    ledger: env.svm.get_account(&ledger).unwrap(),
                    mint: mint_frame,
                };
                let mut close = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new(admin_token, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                        AccountMeta::new(env.mint, false),
                    ],
                    data: ProgInstruction::CloseSlab {
                        authority_epoch: env.control_sequences(0).authority_epoch,
                    }
                    .encode(),
                };
                if native {
                    close.accounts.push(AccountMeta::new(tokens[4], false));
                }
                let mut book = Book {
                    residual: retained,
                    paid: [0; 4],
                    expired: false,
                    recredited: 0,
                };
                frames.check(&env, &book, wallets, tokens, ledger, [true; 5]);
                // One representative completion word; Scope H owns the order product.
                for (class, amount) in [
                    (0usize, SOURCE_PRINCIPAL),
                    (1, BACKING - retained),
                    (3, AVAILABLE),
                    (0, 0),
                    (3, SPENT),
                    (2, PROVIDER_FEE),
                ] {
                    let mut signers = Vec::new();
                    let mut allowed = vec![env.market];
                    let ix = if amount == 0 {
                        env.svm.warp_to_slot(100);
                        signers.push(&admin);
                        close.clone()
                    } else {
                        let mut ix = reserve_payout(
                            &env,
                            wallets,
                            tokens,
                            ledger,
                            class.saturating_sub(1),
                            amount,
                        );
                        if class == 0 {
                            ix.data = ProgInstruction::WithdrawBackingBucket {
                                domain: 0,
                                market_id: env.asset_market_id(0),
                                authority_epoch: env.control_sequences(0).authority_epoch,
                                amount: amount.into(),
                            }
                            .encode();
                        }
                        assert!(ix.accounts.iter().all(|meta| !meta.is_signer));
                        allowed.extend([env.vault, tokens[if class == 3 { 4 } else { 2 }]]);
                        if class == 2 {
                            allowed.push(ledger);
                        }
                        ix
                    };
                    peak = peak.max(checked_land(
                        &mut env,
                        &[ix],
                        &signers,
                        &tracked,
                        &allowed,
                        0,
                        None,
                        None,
                    ));
                    if amount == 0 {
                        book.expire();
                    } else {
                        book.pay(class, amount);
                    }
                    frames.check(&env, &book, wallets, tokens, ledger, [true; 5]);
                }
                let residue = retained - SPENT;
                assert_eq!(book.rank(), (0, 0));
                assert_eq!(book.expected().custody, residue);
                assert_eq!(book.paid[3], INSURANCE + INSURANCE_FEE);
                let token_rent = env
                    .svm
                    .minimum_balance_for_rent_exemption(TokenAccount::LEN);
                let mut redeemed = 0;
                if missing {
                    redeemed = book.paid[3];
                    let redeem = spl_token::instruction::close_account(
                        &spl_token::ID,
                        &tokens[4],
                        &wallets[4],
                        &wallets[4],
                        &[],
                    )
                    .unwrap();
                    peak = peak.max(checked_land(
                        &mut env,
                        &[redeem],
                        &[&insurer],
                        &tracked,
                        &[tokens[4]],
                        0,
                        Some((wallets[4], token_rent + redeemed)),
                        None,
                    ));
                    assert_absent(&env, tokens[4]);
                    let balance = env.svm.get_account(&wallets[4]).unwrap().lamports;
                    peak = peak.max(checked_land(
                        &mut env,
                        &[system_instruction::transfer(
                            &wallets[4],
                            &departure,
                            balance,
                        )],
                        &[&insurer],
                        &tracked,
                        &[wallets[4], departure],
                        0,
                        None,
                        None,
                    ));
                    assert_absent(&env, wallets[4]);
                    assert_eq!(env.svm.get_account(&departure).unwrap().lamports, balance);
                }
                // The beneficiary may redeem old custody voluntarily; all reserve keys
                // are unavailable throughout the new residue-cleanup continuation.
                drop(insurer);
                let paid_tokens = tokens.map(|key| env.svm.get_account(&key));
                let paid_wallets = wallets.map(|key| env.svm.get_account(&key));
                let paid_ledger = env.svm.get_account(&ledger);
                let admin_frame = env.svm.get_account(&admin_token).unwrap();
                let completed_market = env.svm.get_account(&env.market).unwrap();
                if donation != 0 {
                    let donor = admin.pubkey();
                    let vault = env.vault;
                    let balance = env.svm.get_account(&donor).unwrap().lamports;
                    peak = peak.max(checked_land(
                        &mut env,
                        &[system_instruction::transfer(&donor, &vault, donation)],
                        &[&admin],
                        &tracked,
                        &[donor, vault],
                        0,
                        None,
                        None,
                    ));
                    assert_eq!(
                        env.svm.get_account(&donor).unwrap().lamports,
                        balance - donation
                    );
                }
                if sync {
                    let ix =
                        spl_token::instruction::sync_native(&spl_token::ID, &env.vault).unwrap();
                    let vault = env.vault;
                    peak = peak.max(checked_land(
                        &mut env,
                        &[ix],
                        &[],
                        &tracked,
                        &[vault],
                        0,
                        None,
                        None,
                    ));
                }
                let wrapped_surplus = if sync { donation } else { 0 };
                let mut expected_vault = token_image(&frames.vault, residue + wrapped_surplus);
                expected_vault.lamports += donation - wrapped_surplus;
                assert_eq!(env.svm.get_account(&env.vault), Some(expected_vault));
                assert_eq!(env.svm.get_account(&env.market), Some(completed_market));
                let mut batch = Vec::new();
                if missing {
                    batch.push(Instruction {
                        program_id: associated_token_program_id(),
                        accounts: vec![
                            AccountMeta::new(env.payer.pubkey(), true),
                            AccountMeta::new(tokens[4], false),
                            AccountMeta::new_readonly(wallets[4], false),
                            AccountMeta::new_readonly(env.mint, false),
                            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: vec![1],
                    });
                }
                batch.push(close.clone());
                if native {
                    let mut misdirected = batch.clone();
                    misdirected.last_mut().unwrap().accounts[7] =
                        AccountMeta::new(admin_token, false);
                    peak = peak.max(checked_land(
                        &mut env,
                        &misdirected,
                        &[&admin],
                        &tracked,
                        &[],
                        0,
                        None,
                        Some((1 + batch.len() as u8, PercolatorError::InvalidTokenAccount)),
                    ));
                    rollbacks += 1;
                }
                let mut denied = close.clone();
                denied.accounts[0] = AccountMeta::new(wallets[3], false);
                let mut rejected = batch.clone();
                rejected.push(denied);
                peak = peak.max(checked_land(
                    &mut env,
                    &rejected,
                    &[&admin],
                    &tracked,
                    &[],
                    0,
                    None,
                    Some((2 + batch.len() as u8, PercolatorError::ExpectedSigner)),
                ));
                rollbacks += 1;
                assert_eq!(tokens.map(|key| env.svm.get_account(&key)), paid_tokens);
                let tombstone_rent = env
                    .svm
                    .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
                let refund = frames.market.lamports + token_rent + donation
                    - wrapped_surplus
                    - tombstone_rent;
                let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
                expected_admin.lamports += refund;
                let mut allowed = vec![env.market, env.vault, env.mint, admin_token];
                if native {
                    allowed.push(tokens[4]);
                }
                peak = peak.max(checked_land(
                    &mut env,
                    &batch,
                    &[&admin],
                    &tracked,
                    &allowed,
                    if missing { token_rent } else { 0 },
                    Some((admin.pubkey(), refund)),
                    None,
                ));
                let tombstone = env.svm.get_account(&env.market).unwrap();
                assert_closed_market_tombstone(&tombstone);
                assert_eq!(tombstone.lamports, tombstone_rent);
                assert_absent(&env, env.vault);
                assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
                assert_eq!(
                    env.svm.get_account(&admin_token),
                    Some(token_image(&admin_frame, wrapped_surplus))
                );
                let mut expected_tokens = paid_tokens;
                if native {
                    expected_tokens[4] = Some(token_image(
                        &frames.tokens[4],
                        book.paid[3] - redeemed + residue,
                    ));
                }
                assert_eq!(tokens.map(|key| env.svm.get_account(&key)), expected_tokens);
                assert_eq!(wallets.map(|key| env.svm.get_account(&key)), paid_wallets);
                assert_eq!(env.svm.get_account(&ledger), paid_ledger);
                let mut expected_mint = frames.mint;
                if !native {
                    let mut mint = Mint::unpack(&expected_mint.data).unwrap();
                    mint.supply -= residue;
                    Mint::pack(mint, &mut expected_mint.data).unwrap();
                }
                assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
                let held = tokens.iter().map(|key| env.token_amount(*key)).sum::<u64>();
                assert_eq!(held + redeemed + if native { 0 } else { residue }, SUPPLY);
                if native {
                    let redeem = spl_token::instruction::close_account(
                        &spl_token::ID,
                        &admin_token,
                        &admin.pubkey(),
                        &admin.pubkey(),
                        &[],
                    )
                    .unwrap();
                    peak = peak.max(checked_land(
                        &mut env,
                        &[redeem],
                        &[&admin],
                        &tracked,
                        &[admin_token],
                        0,
                        Some((admin.pubkey(), token_rent + wrapped_surplus)),
                        None,
                    ));
                    assert_absent(&env, admin_token);
                    assert_eq!(env.svm.get_account(&env.market), Some(tombstone));
                }
                repairs += usize::from(missing);
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, if native { 12 } else { 2 });
    assert_eq!(repairs, if native { 6 } else { 0 });
    assert_eq!(rollbacks, worlds * if native { 2 } else { 1 });
    eprintln!("Scope O: native={native}, worlds={worlds}, repairs={repairs}, retirements={worlds}, exact_rollbacks={rollbacks}, peak={peak} CU");
}

#[test]
fn v16_program_native_booked_residue_escheats_after_fee_recredit_completion() {
    run_cleanup(true);
}

#[test]
fn v16_program_classic_booked_residue_burn_control_preserves_paid_claims() {
    run_cleanup(false);
}
