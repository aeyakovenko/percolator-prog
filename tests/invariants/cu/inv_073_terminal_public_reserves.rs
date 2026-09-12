//! INV-073: partial public reserve payments compose with expiry and mechanical close.

use super::*;
use terminal_reserve_destination_recovery::land;

fn reserve_payout(
    env: &V16CuEnv,
    wallets: [Pubkey; 5],
    tokens: [Pubkey; 5],
    ledger: Pubkey,
    kind: usize,
    amount: u64,
) -> Instruction {
    let actor = if kind == 2 { 4 } else { 2 };
    let mut accounts = vec![
        AccountMeta::new_readonly(wallets[actor], false),
        AccountMeta::new(env.market, false),
    ];
    if kind == 1 {
        accounts.push(AccountMeta::new(ledger, false));
    }
    accounts.extend([
        AccountMeta::new(tokens[actor], false),
        AccountMeta::new(env.vault, false),
        AccountMeta::new_readonly(env.vault_authority, false),
        AccountMeta::new_readonly(spl_token::ID, false),
    ]);
    let market_id = env.asset_market_id(0);
    let authority_epoch = env.control_sequences(0).authority_epoch;
    let amount = amount.into();
    let ix = match kind {
        0 => ProgInstruction::WithdrawBackingBucket {
            domain: 1,
            market_id,
            authority_epoch,
            amount,
        },
        1 => ProgInstruction::WithdrawBackingBucketEarnings {
            domain: 1,
            market_id,
            authority_epoch,
            amount,
        },
        2 => ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 0,
            market_id,
            authority_epoch,
            amount,
        },
        _ => unreachable!(),
    };
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ix.encode(),
    }
}

pub(crate) fn verify_terminal_public_reserve_seniority() {
    let TerminalEarningsWorld {
        mut env,
        incumbent,
        successor,
        wallets,
        tokens,
        portfolios,
        mint_frame,
        ..
    } = terminal_earnings_world_with_exit(false);
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
    let reserves = std::array::from_fn::<_, 3, _>(|kind| {
        reserve_payout(&env, wallets, tokens, ledger, kind, 1)
    });
    let tracked = [env.market, env.vault, env.mint, ledger]
        .into_iter()
        .chain(wallets)
        .chain(tokens)
        .chain(portfolios)
        .collect::<Vec<_>>();
    for ix in &reserves {
        land(
            &mut env,
            &[ix.clone()],
            &[],
            &tracked,
            &[],
            0,
            None,
            Some((2, PercolatorError::ExpectedSigner)),
        );
    }
    env.resolve();
    env.svm.warp_to_slot(7);
    assert!(env.market_state().1.c_tot > 0);
    for ix in &reserves {
        land(
            &mut env,
            &[ix.clone()],
            &[],
            &tracked,
            &[],
            0,
            None,
            Some((2, PercolatorError::EngineLockActive)),
        );
    }
    for _ in 0..8 {
        for actor in [1, 0] {
            if resolved_portfolio_is_terminal(&env, portfolios[actor]) {
                continue;
            }
            let ix = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new_readonly(wallets[actor], false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                }
                .encode(),
            };
            let allowed = [env.market, env.vault, portfolios[actor], tokens[actor]];
            land(&mut env, &[ix], &[], &tracked, &allowed, 0, None, None);
            assert_eq!(
                tokens.map(|key| env.token_amount(key)).iter().sum::<u64>()
                    + env.token_amount(env.vault),
                SUPPLY
            );
        }
        if portfolios
            .iter()
            .all(|key| resolved_portfolio_is_terminal(&env, *key))
        {
            break;
        }
    }
    for actor in 0..2 {
        assert!(resolved_portfolio_is_terminal(&env, portfolios[actor]));
        assert_eq!(env.token_amount(tokens[actor]), PAYOUTS[actor]);
    }
    assert_eq!(
        (
            env.market_state().1.c_tot,
            env.market_state().1.materialized_portfolio_count
        ),
        (0, 2)
    );
    // Mechanical deletion is still a separate prerequisite of the existing reserve routes.
    for ix in &reserves {
        land(
            &mut env,
            &[ix.clone()],
            &[],
            &tracked,
            &[],
            0,
            None,
            Some((2, PercolatorError::EngineLockActive)),
        );
    }
    assert_eq!(env.token_amount(env.vault), BACKING + EARNINGS + INSURANCE);
    assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame));
}

pub(crate) fn verify_terminal_public_reserve_disposition() {
    const PREFIX: [u64; 3] = [101, 17, 7];
    const STOCK: [u64; 3] = [BACKING, EARNINGS, INSURANCE];
    let mut peak = 0;
    for expire in [false, true] {
        for order in [
            [1, 0, 2],
            [0, 1, 2],
            [0, 2, 1],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
            let TerminalEarningsWorld {
                mut env,
                admin,
                incumbent,
                successor,
                mut wallets,
                mut tokens,
                portfolios,
                mint_frame,
            } = terminal_earnings_world();
            let beneficiary = Keypair::new();
            env.svm
                .airdrop(&beneficiary.pubkey(), 1_000_000_000)
                .unwrap();
            env.try_update_per_asset_authority_with_cu(
                &admin,
                Some(&beneficiary),
                0,
                processor::ASSET_AUTH_INSURANCE,
                beneficiary.pubkey().to_bytes(),
            )
            .unwrap();
            let admin_token = tokens[4];
            wallets[4] = beneficiary.pubkey();
            tokens[4] = create_ata_for_test(&mut env.svm, &env.payer, wallets[4], env.mint);
            let encumbered = [&incumbent, &beneficiary].map(|owner| {
                [false, true].map(|close_authority| {
                    let key = Keypair::new();
                    system_create_account_for_test(
                        &mut env.svm,
                        &env.payer,
                        &key,
                        TokenAccount::LEN,
                        spl_token::ID,
                    );
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::initialize_account3(
                            &spl_token::ID,
                            &key.pubkey(),
                            &env.mint,
                            &owner.pubkey(),
                        )
                        .unwrap(),
                        &[],
                    )
                    .unwrap();
                    let instruction = if close_authority {
                        spl_token::instruction::set_authority(
                            &spl_token::ID,
                            &key.pubkey(),
                            Some(&admin.pubkey()),
                            spl_token::instruction::AuthorityType::CloseAccount,
                            &owner.pubkey(),
                            &[],
                        )
                        .unwrap()
                    } else {
                        spl_token::instruction::approve(
                            &spl_token::ID,
                            &key.pubkey(),
                            &admin.pubkey(),
                            &owner.pubkey(),
                            &[],
                            SUPPLY,
                        )
                        .unwrap()
                    };
                    send_raw_tx(&mut env.svm, &env.payer, instruction, &[owner]).unwrap();
                    key.pubkey()
                })
            });
            drop((incumbent, successor, beneficiary));
            assert!(!wallets.contains(&env.payer.pubkey()));
            assert!(!wallets.contains(&admin.pubkey()));
            let ledger = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &ledger,
                state::backing_domain_ledger_account_len(),
                env.program_id,
            );
            let ledger = ledger.pubkey();
            let tracked = [
                env.market,
                env.vault,
                env.mint,
                ledger,
                admin.pubkey(),
                admin_token,
            ]
            .into_iter()
            .chain(wallets)
            .chain(tokens)
            .chain(portfolios)
            .chain(encumbered.into_iter().flatten())
            .collect::<Vec<_>>();
            let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
            let vault_frame = env.svm.get_account(&env.vault).unwrap();
            let empty_ledger = env.svm.get_account(&ledger).unwrap();
            let profile = state::read_asset_oracle_profile(
                &env.svm.get_account(&env.market).unwrap().data,
                0,
            )
            .unwrap();
            let epoch = env.control_sequences(0).authority_epoch;
            let wrap = |ix: ProgInstruction, accounts| Instruction {
                program_id: percolator_prog::id(),
                accounts,
                data: ix.encode(),
            };
            let payout = |kind, amount| reserve_payout(&env, wallets, tokens, ledger, kind, amount);
            let prefixes = std::array::from_fn::<_, 3, _>(|kind| payout(kind, PREFIX[kind]));
            let tails =
                std::array::from_fn::<_, 3, _>(|kind| payout(kind, STOCK[kind] - PREFIX[kind]));
            let close = wrap(
                ProgInstruction::CloseSlab {
                    authority_epoch: epoch,
                },
                vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new(admin_token, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(env.mint, false),
                ],
            );
            let mut unsigned_close = close.clone();
            unsigned_close.accounts[0].is_signer = false;
            let stock = |env: &V16CuEnv, paid: [u64; 3], expired: bool| {
                let amounts = [PAYOUTS[0], PAYOUTS[1], paid[0] + paid[1], 0, paid[2]];
                for ((key, frame), amount) in tokens.into_iter().zip(&token_frames).zip(amounts) {
                    let mut expected = frame.clone();
                    let mut token = TokenAccount::unpack(&expected.data).unwrap();
                    token.amount = amount;
                    TokenAccount::pack(token, &mut expected.data).unwrap();
                    assert_eq!(env.svm.get_account(&key), Some(expected));
                }
                let group = env.market_state().1;
                let remaining = STOCK.iter().sum::<u64>() - paid.iter().sum::<u64>();
                assert_eq!(group.mode, MarketModeV16::Resolved);
                assert_eq!(
                    (
                        group.c_tot,
                        group.pnl_pos_tot,
                        group.materialized_portfolio_count
                    ),
                    (0, 0, 0)
                );
                assert_eq!(group.source_claim_bound_total_num, 0);
                assert_eq!(group.vault, remaining.into());
                assert_eq!(env.token_amount(env.vault), remaining);
                assert_eq!(amounts.iter().sum::<u64>() + remaining, SUPPLY);
                assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
                let mut expected_vault = vault_frame.clone();
                let mut token = TokenAccount::unpack(&expected_vault.data).unwrap();
                token.amount = remaining;
                TokenAccount::pack(token, &mut expected_vault.data).unwrap();
                assert_eq!(env.svm.get_account(&env.vault), Some(expected_vault));
                assert_eq!(group.insurance, u128::from(INSURANCE - paid[2]));
                assert_eq!(
                    group.insurance_domain_budget_remaining_total,
                    group.insurance
                );
                assert_eq!(
                    group.backing_provider_earnings_total,
                    u128::from(EARNINGS - paid[1])
                );
                let bucket = group.source_backing_buckets[1];
                assert_eq!(
                    bucket.utilization_fee_earnings,
                    u128::from(EARNINGS - paid[1])
                );
                let principal = if expired {
                    0
                } else {
                    u128::from(BACKING - paid[0]) * BOUND_SCALE
                };
                assert_eq!(bucket.fresh_unliened_backing_num, principal);
                assert_eq!(group.source_credit[1].fresh_reserved_backing_num, principal);
                assert_eq!(bucket.valid_liened_backing_num, 0);
                assert_eq!(
                    bucket.consumed_liened_backing_num,
                    u128::from(PROFIT) * BOUND_SCALE
                );
                assert_eq!(
                    group.source_credit[1].provider_receivable_num,
                    u128::from(PROFIT) * BOUND_SCALE
                );
                assert_eq!(
                    bucket.status,
                    if principal == 0 {
                        BackingBucketStatusV16::Expired
                    } else {
                        BackingBucketStatusV16::Fresh
                    }
                );
                assert_eq!(
                    state::read_asset_oracle_profile(
                        &env.svm.get_account(&env.market).unwrap().data,
                        0
                    )
                    .unwrap(),
                    profile
                );
                assert_eq!(env.control_sequences(0).authority_epoch, epoch);
                assert_eq!(env.token_amount(admin_token), 0);
                let mut image = env.svm.get_account(&env.market).unwrap();
                state::market_view_mut(&mut image.data)
                    .unwrap()
                    .1
                    .validate_shape()
                    .unwrap();
                let image = env.svm.get_account(&ledger).unwrap();
                if paid[1] == 0 {
                    assert_eq!(image, empty_ledger);
                } else {
                    let record = state::read_backing_domain_ledger(&image.data).unwrap();
                    assert_eq!(record.market_group, env.market.to_bytes());
                    assert_eq!(record.authority, wallets[2].to_bytes());
                    assert_eq!(record.total_earnings_withdrawn_atoms, paid[1].into());
                    assert_eq!(
                        record.last_observed_bucket_earnings_atoms,
                        u128::from(EARNINGS - paid[1])
                    );
                }
            };
            let mut paid = [0; 3];
            stock(&env, paid, false);
            for kind in order {
                peak = peak.max(land(
                    &mut env,
                    &[prefixes[kind].clone(), unsigned_close.clone()],
                    &[],
                    &tracked,
                    &[],
                    0,
                    None,
                    Some((3, PercolatorError::ExpectedSigner)),
                ));
                stock(&env, paid, false);
                for destination in encumbered[usize::from(kind == 2)] {
                    let mut blocked = prefixes[kind].clone();
                    blocked.accounts[if kind == 1 { 3 } else { 2 }].pubkey = destination;
                    peak = peak.max(land(
                        &mut env,
                        &[blocked],
                        &[],
                        &tracked,
                        &[],
                        0,
                        None,
                        Some((2, PercolatorError::InvalidTokenAccount)),
                    ));
                    stock(&env, paid, false);
                }
                let actor = if kind == 2 { 4 } else { 2 };
                let allowed = [env.market, env.vault, ledger, tokens[actor]];
                peak = peak.max(land(
                    &mut env,
                    &[prefixes[kind].clone()],
                    &[],
                    &tracked,
                    &allowed,
                    0,
                    None,
                    None,
                ));
                paid[kind] += PREFIX[kind];
                stock(&env, paid, false);
            }
            if expire {
                env.svm.warp_to_slot(100);
                let allowed = [env.market];
                peak = peak.max(land(
                    &mut env,
                    &[close.clone()],
                    &[&admin],
                    &tracked,
                    &allowed,
                    0,
                    None,
                    None,
                ));
                stock(&env, paid, true);
            }
            for kind in order.into_iter().rev() {
                if expire && kind == 0 {
                    continue;
                }
                peak = peak.max(land(
                    &mut env,
                    &[close.clone()],
                    &[&admin],
                    &tracked,
                    &[],
                    0,
                    None,
                    Some((2, PercolatorError::EngineLockActive)),
                ));
                let actor = if kind == 2 { 4 } else { 2 };
                let allowed = [env.market, env.vault, ledger, tokens[actor]];
                peak = peak.max(land(
                    &mut env,
                    &[tails[kind].clone()],
                    &[],
                    &tracked,
                    &allowed,
                    0,
                    None,
                    None,
                ));
                paid[kind] = STOCK[kind];
                stock(&env, paid, expire);
            }
            let rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            let refund =
                env.svm.get_account(&env.market).unwrap().lamports + vault_frame.lamports - rent;
            let allowed = [env.market, env.vault, env.mint];
            peak = peak.max(land(
                &mut env,
                &[close],
                &[&admin],
                &tracked,
                &allowed,
                0,
                Some((admin.pubkey(), refund)),
                None,
            ));
            let tombstone = env.svm.get_account(&env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(tombstone.lamports, rent);
            assert!(env
                .svm
                .get_account(&env.vault)
                .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
            let burned = if expire { BACKING - PREFIX[0] } else { 0 };
            let mut expected_mint = mint_frame.clone();
            let mut mint = Mint::unpack(&expected_mint.data).unwrap();
            mint.supply -= burned;
            Mint::pack(mint, &mut expected_mint.data).unwrap();
            assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
            assert_eq!(
                tokens.map(|key| env.token_amount(key)).iter().sum::<u64>() + burned,
                SUPPLY
            );
            assert_eq!(env.token_amount(tokens[2]), paid[0] + EARNINGS);
            assert_eq!(env.token_amount(tokens[4]), INSURANCE);
            assert_eq!(env.token_amount(admin_token), 0);
        }
    }
    eprintln!("INV-073 terminal public reserves: 12 worlds; peak CU={peak}");
}
