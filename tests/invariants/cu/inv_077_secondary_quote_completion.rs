//! INV-070/077: secondary custody changes after primary backing normalization.
//! Cooperative SPL thaw or native sync composes with retirement and custody disposal.
//! This bounded administrative history has no user claims or absent-authority theorem.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const CU_LIMIT: u64 = 150_000;

fn submit(
    env: &mut V16CuEnv,
    watched: &[Pubkey],
    instructions: Vec<Instruction>,
    expected_error: Option<(usize, InstructionError)>,
) -> u64 {
    env.svm.expire_blockhash();
    let mut ixs = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT as u32),
    ];
    ixs.extend(instructions);
    let mut signers = vec![&env.payer];
    if ixs
        .iter()
        .flat_map(|ix| &ix.accounts)
        .any(|meta| meta.is_signer && meta.pubkey == env.admin.pubkey())
    {
        signers.push(&env.admin);
    }
    let tx = Transaction::new_signed_with_payer(
        &ixs,
        Some(&env.payer.pubkey()),
        &signers,
        env.svm.latest_blockhash(),
    );
    assert!(bincode::serialize(&tx).unwrap().len() <= 1_232);
    let fee = u64::from(tx.message.header.num_required_signatures)
        * FeeStructure::default().lamports_per_signature;
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(watched);
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let payer_before = env.svm.get_account(&env.payer.pubkey()).unwrap();
    let result = env.svm.send_transaction(tx);
    let cu = if let Some((index, error)) = expected_error {
        let failure = result.expect_err("completion bundle must restore its whole prefix");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError((index + 2) as u8, error)
        );
        for (key, mut account) in keys.into_iter().zip(before) {
            if key == env.payer.pubkey() {
                account.as_mut().unwrap().lamports -= fee;
            }
            assert_eq!(env.svm.get_account(&key), account, "full rollback {key}");
        }
        failure.meta.compute_units_consumed
    } else {
        let meta = result.expect("bounded public completion");
        let mut expected = payer_before;
        expected.lamports -= fee;
        assert_eq!(env.svm.get_account(&env.payer.pubkey()), Some(expected));
        meta.compute_units_consumed
    };
    assert_cu_within("secondary quote completion", cu, CU_LIMIT);
    cu
}

fn assert_closed(env: &V16CuEnv, key: Pubkey) {
    assert!(env
        .svm
        .get_account(&key)
        .is_none_or(|account| account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)));
}

fn with_amount(initial: &Account, amount: u64, native: bool) -> Account {
    let mut expected = initial.clone();
    let mut token = TokenAccount::unpack(&expected.data).unwrap();
    assert_eq!(token.amount, 0);
    token.amount = amount;
    TokenAccount::pack(token, &mut expected.data).unwrap();
    if native {
        expected.lamports += amount;
    }
    expected
}

#[test]
fn v16_program_secondary_quote_repair_after_expiry_has_atomic_bounded_disposition() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market;
    use inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;

    const BACKING: u64 = 307;
    const PRIMARY: u64 = 17;
    const SECONDARY: u64 = 19;
    const RAW: u64 = 23;
    const EXPIRY: u64 = 5;

    // Freeze either secondary account, or synchronize a native donation after the scan.
    for variant in 0..3 {
        for split in [false, true] {
            let native = variant == 2;
            let mut env = if native {
                inv081_public_native_market()
            } else {
                inv018_public_spl_market(spl_token::native_mint::DECIMALS)
            };
            let admin = env.admin.insecure_clone();
            let admin_key = admin.pubkey();
            let mut peak = env.init_market_cu;
            let added = Keypair::new();
            peak = peak.max(
                send_raw_ixs(
                    &mut env.svm,
                    &env.payer,
                    vec![
                        system_instruction::create_account(
                            &env.payer.pubkey(),
                            &added.pubkey(),
                            1_000_000_000,
                            Mint::LEN as u64,
                            &spl_token::ID,
                        ),
                        spl_token::instruction::initialize_mint(
                            &spl_token::ID,
                            &added.pubkey(),
                            &admin.pubkey(),
                            if native { None } else { Some(&admin_key) },
                            spl_token::native_mint::DECIMALS,
                        )
                        .unwrap(),
                    ],
                    &[&added],
                )
                .unwrap(),
            );
            let added_vault = create_ata_for_test(
                &mut env.svm,
                &env.payer,
                env.vault_authority,
                added.pubkey(),
            );
            let (mints, vaults) = if native {
                ([added.pubkey(), env.mint], [added_vault, env.vault])
            } else {
                ([env.mint, added.pubkey()], [env.vault, added_vault])
            };
            peak = peak.max(
                env.send(
                    ProgInstruction::UpdateBaseUnitMints {
                        primary_mint: mints[0].to_bytes(),
                        secondary_mint: mints[1].to_bytes(),
                        authority_epoch: env.control_sequences(0).authority_epoch,
                    },
                    vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new_readonly(mints[0], false),
                        AccountMeta::new_readonly(mints[1], false),
                        AccountMeta::new_readonly(env.vault, false),
                    ],
                    &[&admin],
                )
                .unwrap(),
            );
            env.mint = mints[0];
            env.vault = vaults[0];
            let destinations = mints
                .map(|mint| create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), mint));
            let rent = env
                .svm
                .minimum_balance_for_rent_exemption(TokenAccount::LEN);
            let sink_keys = [Keypair::new(), Keypair::new()];
            let sinks = sink_keys.each_ref().map(Keypair::pubkey);
            for (sink, mint) in sink_keys.iter().zip(mints) {
                peak = peak.max(
                    send_raw_ixs(
                        &mut env.svm,
                        &env.payer,
                        vec![
                            system_instruction::create_account(
                                &env.payer.pubkey(),
                                &sink.pubkey(),
                                rent,
                                TokenAccount::LEN as u64,
                                &spl_token::ID,
                            ),
                            spl_token::instruction::initialize_account3(
                                &spl_token::ID,
                                &sink.pubkey(),
                                &mint,
                                &admin.pubkey(),
                            )
                            .unwrap(),
                        ],
                        &[sink],
                    )
                    .unwrap(),
                );
            }
            let watched = [
                env.market,
                mints[0],
                mints[1],
                vaults[0],
                vaults[1],
                destinations[0],
                destinations[1],
                sinks[0],
                sinks[1],
                admin.pubkey(),
                env.vault_authority,
            ];
            let empty = [
                vaults[0],
                vaults[1],
                destinations[0],
                destinations[1],
                sinks[0],
                sinks[1],
            ]
            .map(|key| env.svm.get_account(&key).unwrap());
            for (i, account) in empty.iter().enumerate() {
                let token = TokenAccount::unpack(&account.data).unwrap();
                assert_eq!(account.owner, spl_token::ID);
                assert_eq!(account.lamports, rent);
                assert_eq!(token.state, AccountState::Initialized);
                assert_eq!(token.amount, 0);
                assert_eq!(token.mint, mints[i % 2]);
                assert_eq!(
                    token.owner,
                    if i < 2 {
                        env.vault_authority
                    } else {
                        admin.pubkey()
                    }
                );
                assert_eq!(token.delegate, COption::None);
                assert_eq!(token.close_authority, COption::None);
                assert_eq!(
                    token.is_native,
                    if native && i % 2 == 1 {
                        COption::Some(rent)
                    } else {
                        COption::None
                    }
                );
            }
            let mut funding = vec![
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &mints[0],
                    &destinations[0],
                    &admin.pubkey(),
                    &[],
                    BACKING + PRIMARY,
                )
                .unwrap(),
                spl_token::instruction::set_authority(
                    &spl_token::ID,
                    &mints[0],
                    None,
                    spl_token::instruction::AuthorityType::MintTokens,
                    &admin.pubkey(),
                    &[],
                )
                .unwrap(),
            ];
            if native {
                funding.extend([
                    system_instruction::transfer(&admin.pubkey(), &vaults[1], SECONDARY),
                    spl_token::instruction::sync_native(&spl_token::ID, &vaults[1]).unwrap(),
                ]);
            } else {
                funding.extend([
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &mints[1],
                        &vaults[1],
                        &admin.pubkey(),
                        &[],
                        SECONDARY,
                    )
                    .unwrap(),
                    spl_token::instruction::set_authority(
                        &spl_token::ID,
                        &mints[1],
                        None,
                        spl_token::instruction::AuthorityType::MintTokens,
                        &admin.pubkey(),
                        &[],
                    )
                    .unwrap(),
                ]);
            }
            peak = peak.max(submit(&mut env, &watched, funding, None));
            env.svm.warp_to_slot(1);
            peak = peak.max(env.top_up_backing_bucket_from_admin_token_with_cu(
                destinations[0],
                1,
                BACKING.into(),
                EXPIRY,
            ));
            peak = peak.max(submit(
                &mut env,
                &watched,
                vec![spl_token::instruction::transfer(
                    &spl_token::ID,
                    &destinations[0],
                    &vaults[0],
                    &admin.pubkey(),
                    &[],
                    PRIMARY,
                )
                .unwrap()],
                None,
            ));
            peak = peak.max(env.resolve());
            env.svm.warp_to_slot(EXPIRY);
            let close = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(vaults[0], false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new(destinations[0], false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(vaults[1], false),
                    AccountMeta::new(destinations[1], false),
                    AccountMeta::new(mints[0], false),
                ],
                data: ProgInstruction::CloseSlab {
                    authority_epoch: env.control_sequences(0).authority_epoch,
                }
                .encode(),
            };
            let before = watched.map(|key| env.svm.get_account(&key));
            peak = peak.max(submit(&mut env, &watched, vec![close.clone()], None));
            for (key, account) in watched.iter().zip(&before).skip(1) {
                assert_eq!(
                    env.svm.get_account(key),
                    *account,
                    "normalization frame {key}"
                );
            }
            let normalized = env.svm.get_account(&env.market).unwrap();
            let mut metadata = normalized.clone();
            metadata.data = before[0].as_ref().unwrap().data.clone();
            assert_eq!(Some(metadata), before[0]);
            let (_, group) = env.market_state();
            assert_eq!(group.mode, MarketModeV16::Resolved);
            assert_eq!(
                (group.vault, group.c_tot, group.insurance),
                (BACKING.into(), 0, 0)
            );
            assert_eq!(group.materialized_portfolio_count, 0);
            assert_eq!(
                group.source_backing_buckets[1].status,
                BackingBucketStatusV16::Expired
            );
            assert_eq!(
                group.source_backing_buckets[1].fresh_unliened_backing_num,
                0
            );
            assert_eq!(group.source_credit[1].fresh_reserved_backing_num, 0);
            crate::support::fuzz_model::assert_market_stock_census(
                "secondary repair normalized stock",
                &group,
                &normalized.data,
                &[],
                BACKING.into(),
            )
            .unwrap();
            crate::support::fuzz_model::assert_reservation_encumbrance_census(
                "secondary repair normalized stock",
                &group,
                &[],
            )
            .unwrap();

            let frozen = if variant == 0 {
                vaults[1]
            } else {
                destinations[1]
            };
            let disturbance = if native {
                system_instruction::transfer(&admin.pubkey(), &vaults[1], RAW)
            } else {
                spl_token::instruction::freeze_account(
                    &spl_token::ID,
                    &frozen,
                    &mints[1],
                    &admin.pubkey(),
                    &[],
                )
                .unwrap()
            };
            peak = peak.max(submit(&mut env, &watched, vec![disturbance], None));
            assert_eq!(env.svm.get_account(&env.market), Some(normalized.clone()));
            let repair = if native {
                let mut expected = with_amount(&empty[1], SECONDARY, true);
                expected.lamports += RAW;
                assert_eq!(env.svm.get_account(&vaults[1]), Some(expected));
                spl_token::instruction::sync_native(&spl_token::ID, &vaults[1]).unwrap()
            } else {
                assert_eq!(
                    TokenAccount::unpack(&env.svm.get_account(&frozen).unwrap().data)
                        .unwrap()
                        .state,
                    AccountState::Frozen
                );
                peak = peak.max(submit(
                    &mut env,
                    &watched,
                    vec![close.clone()],
                    Some((
                        0,
                        InstructionError::Custom(if variant == 0 {
                            PercolatorError::InvalidVaultAccount as u32
                        } else {
                            PercolatorError::InvalidTokenAccount as u32
                        }),
                    )),
                ));
                spl_token::instruction::thaw_account(
                    &spl_token::ID,
                    &frozen,
                    &mints[1],
                    &admin.pubkey(),
                    &[],
                )
                .unwrap()
            };
            let final_secondary = SECONDARY + if native { RAW } else { 0 };
            let mint_frame = mints.map(|key| env.svm.get_account(&key).unwrap());
            for (i, account) in mint_frame.iter().enumerate() {
                let mint = Mint::unpack(&account.data).unwrap();
                assert_eq!(mint.mint_authority, COption::None);
                assert_eq!(mint.decimals, spl_token::native_mint::DECIMALS);
                assert_eq!(
                    mint.supply,
                    if i == 0 {
                        BACKING + PRIMARY
                    } else if native {
                        0
                    } else {
                        SECONDARY
                    }
                );
                assert_eq!(
                    mint.freeze_authority,
                    if i == 1 && !native {
                        COption::Some(admin.pubkey())
                    } else {
                        COption::None
                    }
                );
            }
            let authority_frame = env.svm.get_account(&env.vault_authority);
            let admin_frame = env.svm.get_account(&admin.pubkey()).unwrap();
            let tombstone_rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            let mut completion = vec![repair.clone(), close.clone()];
            completion.push(
                spl_token::instruction::transfer(
                    &spl_token::ID,
                    &destinations[0],
                    &sinks[0],
                    &admin.pubkey(),
                    &[],
                    PRIMARY,
                )
                .unwrap(),
            );
            completion.push(
                spl_token::instruction::close_account(
                    &spl_token::ID,
                    &destinations[0],
                    &admin.pubkey(),
                    &admin.pubkey(),
                    &[],
                )
                .unwrap(),
            );
            if !native {
                completion.push(
                    spl_token::instruction::transfer(
                        &spl_token::ID,
                        &destinations[1],
                        &sinks[1],
                        &admin.pubkey(),
                        &[],
                        SECONDARY,
                    )
                    .unwrap(),
                );
            }
            completion.push(
                spl_token::instruction::close_account(
                    &spl_token::ID,
                    &destinations[1],
                    &admin.pubkey(),
                    &admin.pubkey(),
                    &[],
                )
                .unwrap(),
            );
            // A late failed debit restores thaw/sync, burn, both sweeps, all four
            // custody closes and the tombstone. The identical valid prefix follows.
            let mut rejected = completion.clone();
            rejected.push(
                spl_token::instruction::burn(
                    &spl_token::ID,
                    &sinks[0],
                    &mints[0],
                    &admin.pubkey(),
                    &[],
                    PRIMARY + 1,
                )
                .unwrap(),
            );
            peak = peak.max(submit(
                &mut env,
                &watched,
                rejected,
                Some((
                    completion.len(),
                    InstructionError::Custom(
                        spl_token::error::TokenError::InsufficientFunds as u32,
                    ),
                )),
            ));

            let mut expected_mint = mint_frame[0].clone();
            let mut mint = Mint::unpack(&expected_mint.data).unwrap();
            mint.supply = PRIMARY;
            Mint::pack(mint, &mut expected_mint.data).unwrap();
            if split {
                peak = peak.max(submit(&mut env, &watched, vec![repair], None));
                assert_eq!(env.svm.get_account(&env.market), Some(normalized.clone()));
                assert_eq!(
                    env.svm.get_account(&vaults[0]),
                    Some(with_amount(&empty[0], BACKING + PRIMARY, false))
                );
                assert_eq!(
                    env.svm.get_account(&vaults[1]),
                    Some(with_amount(&empty[1], final_secondary, native))
                );
                assert_eq!(
                    env.svm.get_account(&destinations[1]),
                    Some(empty[3].clone())
                );
                peak = peak.max(submit(&mut env, &watched, vec![close], None));
                assert_eq!(env.svm.get_account(&mints[0]), Some(expected_mint.clone()));
                assert_eq!(
                    env.svm.get_account(&destinations[0]),
                    Some(with_amount(&empty[2], PRIMARY, false))
                );
                assert_eq!(
                    env.svm.get_account(&destinations[1]),
                    Some(with_amount(&empty[3], final_secondary, native))
                );
                assert_closed(&env, vaults[0]);
                assert_closed(&env, vaults[1]);
                for ix in completion.into_iter().skip(2) {
                    peak = peak.max(submit(&mut env, &watched, vec![ix], None));
                }
            } else {
                peak = peak.max(submit(&mut env, &watched, completion, None));
            }
            let tombstone = env.svm.get_account(&env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            let mut expected_market = normalized;
            expected_market.data = tombstone.data.clone();
            expected_market.lamports = tombstone_rent;
            assert_eq!(tombstone, expected_market);
            for key in [vaults[0], vaults[1], destinations[0], destinations[1]] {
                assert_closed(&env, key);
            }
            assert_eq!(env.svm.get_account(&mints[0]), Some(expected_mint));
            assert_eq!(env.svm.get_account(&mints[1]), Some(mint_frame[1].clone()));
            assert_eq!(
                env.svm.get_account(&sinks[0]),
                Some(with_amount(&empty[4], PRIMARY, false))
            );
            assert_eq!(
                env.svm.get_account(&sinks[1]),
                Some(with_amount(
                    &empty[5],
                    if native { 0 } else { SECONDARY },
                    native
                ))
            );
            let mut expected_admin = admin_frame;
            expected_admin.lamports += before[0].as_ref().unwrap().lamports - tombstone_rent
                + 4 * rent
                + if native { final_secondary } else { 0 };
            assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
            assert_eq!(env.svm.get_account(&env.vault_authority), authority_frame);
            assert_cu_within("secondary quote measured lifecycle", peak, CU_LIMIT);
            println!("INV-070/077 secondary repair: variant={variant}, split={split}, retired={BACKING}, primary={PRIMARY}, secondary={final_secondary}, close_calls=2, exact_rejections={}, peak_CU={peak}", if native { 1 } else { 2 });
        }
    }
}
