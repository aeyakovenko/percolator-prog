//! Row433 / INV-024/073: native-secondary earnings share the SPL-paid ledger.
//! Repaired native custody pays the fee tail, while unsynced vault donations and
//! displaced primary liquidity remain surplus through exact terminal closure.
//! A redeemed native fee prefix remains counted after wallet loss and ATA repair.

use super::*;
use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_create_public_spl_mint;
use inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market_with_params;
use percolator_prog::constants;
use terminal_public_reserves::reserve_payout;

#[test]
fn v16_program_native_secondary_earnings_repair_excludes_donations_and_closes_both_rails() {
    verify_native_secondary_earnings(false);
}

#[test]
fn v16_program_redeemed_native_secondary_earnings_preserve_ledger_through_repair_and_close() {
    verify_native_secondary_earnings(true);
}

fn verify_native_secondary_earnings(redeem_prefix: bool) {
    const DONATION: u64 = 19;
    const TAIL: u64 = EARNINGS - PREFIX;
    let redeemed = if redeem_prefix { PREFIX } else { 0 };
    let mut peak = [0; 4]; // setup, payment, rejected suffix, final closure
    let mut env = inv081_public_native_market_with_params(
        1,
        V16CuMarketParams {
            max_portfolio_assets: 1,
            initial_margin_bps: 5_000,
            maintenance_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            ..V16CuMarketParams::default()
        },
    );
    peak[0] = env.init_market_cu;
    let admin = env.admin.insecure_clone();
    let native_mint = env.mint;
    let native_vault = env.vault;
    let spl_mint = inv018_create_public_spl_mint(
        &mut env.svm,
        &env.payer,
        admin.pubkey(),
        spl_token::native_mint::DECIMALS,
    );
    peak[0] = peak[0].max(
        env.send(
            ProgInstruction::UpdateBaseUnitMints {
                primary_mint: spl_mint.to_bytes(),
                secondary_mint: native_mint.to_bytes(),
                authority_epoch: env.control_sequences(0).authority_epoch,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new_readonly(spl_mint, false),
                AccountMeta::new_readonly(native_mint, false),
                AccountMeta::new_readonly(native_vault, false),
            ],
            &[&admin],
        )
        .unwrap(),
    );
    // Host handles follow the public configuration; no account bytes are installed.
    env.mint = spl_mint;
    env.vault = create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, spl_mint);
    let provider = Keypair::new();
    let operator = Keypair::new();
    let users = [Keypair::new(), Keypair::new()];
    let wallets = [
        users[0].pubkey(),
        users[1].pubkey(),
        provider.pubkey(),
        operator.pubkey(),
        admin.pubkey(),
    ];
    let tokens =
        wallets.map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner, spl_mint));
    let native_provider = create_ata_for_test(&mut env.svm, &env.payer, wallets[2], native_mint);
    let native_admin = create_ata_for_test(&mut env.svm, &env.payer, wallets[4], native_mint);
    for wallet in &wallets[..4] {
        env.svm.airdrop(wallet, 1_000_000_000).unwrap();
    }
    for (kind, holder) in [
        (processor::ASSET_AUTH_BACKING_BUCKET, &provider),
        (processor::ASSET_AUTH_INSURANCE_OPERATOR, &operator),
    ] {
        peak[0] = peak[0].max(
            env.try_update_per_asset_authority_with_cu(
                &admin,
                Some(holder),
                0,
                kind,
                holder.pubkey().to_bytes(),
            )
            .unwrap(),
        );
    }
    env.svm.warp_to_slot(1);
    peak[0] = peak[0].max(env.configure_permissionless_resolve_with_cu(100, 5));
    peak[0] = peak[0].max(env.configure_auth_mark_for_asset_as_admin(0, 1, 100));
    peak[0] = peak[0].max(env.update_backing_fee_policy_with_cu(1, RATE, 0));
    for (token, amount) in tokens
        .into_iter()
        .zip([CAPITAL[0], CAPITAL[1], BACKING, 0, INSURANCE])
    {
        if amount != 0 {
            peak[0] = peak[0].max(
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &spl_mint,
                        &token,
                        &admin.pubkey(),
                        &[],
                        amount,
                    )
                    .unwrap(),
                    &[&admin],
                )
                .unwrap(),
            );
        }
    }
    peak[0] = peak[0].max(
        send_raw_ixs(
            &mut env.svm,
            &env.payer,
            vec![
                spl_token::instruction::set_authority(
                    &spl_token::ID,
                    &spl_mint,
                    None,
                    spl_token::instruction::AuthorityType::MintTokens,
                    &admin.pubkey(),
                    &[],
                )
                .unwrap(),
                system_instruction::transfer(&admin.pubkey(), &native_vault, EARNINGS),
                spl_token::instruction::sync_native(&spl_token::ID, &native_vault).unwrap(),
                system_instruction::transfer(&admin.pubkey(), &native_vault, DONATION),
            ],
            &[&admin],
        )
        .unwrap(),
    );
    let mint_frames = [spl_mint, native_mint].map(|key| env.svm.get_account(&key).unwrap());
    let fixed_mint = Mint::unpack(&mint_frames[0].data).unwrap();
    assert_eq!(
        (fixed_mint.supply, fixed_mint.mint_authority),
        (SUPPLY, COption::None)
    );
    let portfolios = users.each_ref().map(|owner| {
        let key = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &key,
            env.portfolio_account_len,
            env.program_id,
        );
        peak[0] = peak[0].max(
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(key.pubkey(), false),
                ],
                &[owner],
            )
            .unwrap(),
        );
        env.portfolios.push(key.pubkey());
        key.pubkey()
    });
    for i in 0..2 {
        peak[0] = peak[0].max(
            env.send(
                env.deposit_ix(portfolios[i], CAPITAL[i].into()),
                vec![
                    AccountMeta::new(wallets[i], true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                    AccountMeta::new(tokens[i], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&users[i]],
            )
            .unwrap(),
        );
    }
    for (actor, instruction, signer) in [
        (
            2,
            ProgInstruction::TopUpBackingBucket {
                domain: 1,
                market_id: env.asset_market_id(0),
                authority_epoch: env.control_sequences(0).authority_epoch,
                intent_id: 0,
                backing_fee_bps: RATE,
                insurance_share_bps: 0,
                amount: BACKING.into(),
                expiry_slot: 100,
            },
            &provider,
        ),
        (
            4,
            ProgInstruction::TopUpInsuranceDomain {
                domain: 0,
                market_id: env.asset_market_id(0),
                authority_epoch: env.control_sequences(0).authority_epoch,
                intent_id: 0,
                amount: INSURANCE.into(),
            },
            &admin,
        ),
    ] {
        peak[0] = peak[0].max(
            env.send(
                instruction,
                vec![
                    AccountMeta::new(wallets[actor], true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[signer],
            )
            .unwrap(),
        );
    }
    peak[0] = peak[0].max(env.trade_asset_with_cu(
        0,
        &users[0],
        portfolios[0],
        &users[1],
        portfolios[1],
        1_000 * POS_SCALE as i128,
        100,
        0,
    ));
    env.svm.warp_to_slot(2);
    peak[0] = peak[0].max(env.push_auth_mark_for_asset_as_admin(0, 2, 105));
    for i in [1, 0] {
        peak[0] = peak[0].max(env.crank(
            portfolios[i],
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(0),
            },
        ));
    }
    peak[0] = peak[0].max(
        env.try_trade_asset_with_backing_fee_cap_with_cu(
            0,
            &users[0],
            portfolios[0],
            &users[1],
            portfolios[1],
            50 * POS_SCALE as i128,
            105,
            0,
            RATE,
        )
        .unwrap(),
    );
    assert_eq!(EARNINGS, 875);
    assert_eq!(
        env.market_state().1.backing_provider_earnings_total,
        EARNINGS.into()
    );
    peak[0] = peak[0].max(env.resolve());
    env.svm.warp_to_slot(7);
    for _ in 0..8 {
        for i in [1, 0] {
            if !resolved_portfolio_is_terminal(&env, portfolios[i]) {
                env.svm.expire_blockhash();
                peak[0] = peak[0].max(
                    env.send(
                        ProgInstruction::CloseResolved {
                            fee_rate_per_slot: 0,
                        },
                        vec![
                            AccountMeta::new_readonly(wallets[i], false),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[i], false),
                            AccountMeta::new(tokens[i], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[],
                    )
                    .unwrap(),
                );
            }
        }
        if portfolios
            .iter()
            .all(|key| resolved_portfolio_is_terminal(&env, *key))
        {
            break;
        }
    }
    for i in 0..2 {
        assert!(resolved_portfolio_is_terminal(&env, portfolios[i]));
        assert_eq!(env.token_amount(tokens[i]), PAYOUTS[i]);
        peak[0] = peak[0].max(env.close_portfolio_with_cu(&users[i], portfolios[i]));
    }
    let ledger = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &ledger,
        state::backing_domain_ledger_account_len(),
        env.program_id,
    );
    let ledger = ledger.pubkey();
    let ledger_frame = env.svm.get_account(&ledger).unwrap();
    let market_frame = env.svm.get_account(&env.market).unwrap();
    let (config, initial_group) = env.market_state();
    let profile = state::read_asset_oracle_profile(&market_frame.data, 0).unwrap();
    let sequences = env.control_sequences(0);
    let custody_keys = [
        env.vault,
        native_vault,
        tokens[2],
        native_provider,
        tokens[4],
        native_admin,
    ];
    let custody_frames = custody_keys.map(|key| env.svm.get_account(&key).unwrap());
    let rent = env
        .svm
        .minimum_balance_for_rent_exemption(TokenAccount::LEN);
    assert_eq!(custody_frames[1].lamports, rent + EARNINGS + DONATION);
    assert_eq!(env.token_amount(native_vault), EARNINGS);
    let tracked = [
        env.market,
        env.mint,
        native_mint,
        env.vault_authority,
        ledger,
    ]
    .into_iter()
    .chain(wallets)
    .chain(tokens)
    .chain(portfolios)
    .chain(custody_keys)
    .collect::<Vec<_>>();
    if redeem_prefix {
        let mut first = reserve_payout(&env, wallets, tokens, ledger, 1, redeemed);
        first.accounts[3].pubkey = native_provider;
        first.accounts[4].pubkey = native_vault;
        assert!(first.accounts.iter().all(|meta| !meta.is_signer));
        let allowed = [env.market, ledger, native_vault, native_provider];
        peak[1] = peak[1].max(land(
            &mut env,
            &[first],
            &[],
            &tracked,
            &allowed,
            0,
            None,
            None,
        ));
        assert_eq!(env.token_amount(native_provider), redeemed);
        assert_eq!(
            env.svm.get_account(&native_provider).unwrap().lamports,
            rent + redeemed
        );
    }
    let remove = spl_token::instruction::close_account(
        &spl_token::ID,
        &native_provider,
        &wallets[2],
        &wallets[2],
        &[],
    )
    .unwrap();
    peak[0] = peak[0].max(land(
        &mut env,
        &[remove],
        &[&provider],
        &tracked,
        &[native_provider],
        0,
        Some((wallets[2], rent + redeemed)),
        None,
    ));
    let balance = env.svm.get_account(&wallets[2]).unwrap().lamports;
    let drain = system_instruction::transfer(&wallets[2], &wallets[3], balance);
    peak[0] = peak[0].max(land(
        &mut env,
        &[drain],
        &[&provider],
        &tracked,
        &[wallets[2]],
        0,
        Some((wallets[3], balance)),
        None,
    ));
    drop((provider, operator, users));

    let token_image = |index: usize, amount: u64| {
        // Expected images only; these bytes are never written to SVM accounts.
        let mut expected = custody_frames[index].clone();
        let mut token = TokenAccount::unpack(&expected.data).unwrap();
        if token.is_native.is_some() {
            expected.lamports = expected.lamports - token.amount + amount;
        }
        token.amount = amount;
        TokenAccount::pack(token, &mut expected.data).unwrap();
        expected
    };
    let assert_absent = |env: &V16CuEnv, key: Pubkey| {
        assert!(env.svm.get_account(&key).is_none_or(|a| {
            a.lamports == 0
                && a.data.is_empty()
                && a.owner == solana_sdk::system_program::ID
                && !a.executable
        }));
    };
    let check = |env: &V16CuEnv, fees: [u64; 2], principal: u64, insurance: u64| {
        assert_absent(env, wallets[2]);
        for key in portfolios {
            if let Some(account) = env.svm.get_account(&key) {
                assert_eq!(account.lamports, 0);
                assert!(account.data.is_empty());
                assert_eq!(account.owner, env.program_id);
                assert!(!account.executable);
            }
        }
        for (key, frame) in [spl_mint, native_mint].into_iter().zip(&mint_frames) {
            assert_eq!(env.svm.get_account(&key), Some(frame.clone()));
        }
        let paid = fees.iter().sum::<u64>();
        let remaining = BACKING + EARNINGS + INSURANCE - paid - principal - insurance;
        let group = env.market_state().1;
        assert_eq!(env.market_state().0, config);
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(
            (
                group.c_tot,
                group.pnl_pos_tot,
                group.materialized_portfolio_count
            ),
            (0, 0, 0)
        );
        assert_eq!(group.vault, remaining.into());
        assert_eq!(
            group.backing_provider_earnings_total,
            u128::from(EARNINGS - paid)
        );
        let bucket = group.source_backing_buckets[1];
        assert_eq!(bucket.utilization_fee_earnings, u128::from(EARNINGS - paid));
        assert_eq!(
            bucket.fresh_unliened_backing_num,
            u128::from(BACKING - principal) * BOUND_SCALE
        );
        assert_eq!(
            group.source_credit[1].fresh_reserved_backing_num,
            bucket.fresh_unliened_backing_num
        );
        assert_eq!(
            group.source_backing_buckets[0],
            initial_group.source_backing_buckets[0]
        );
        assert_eq!(group.source_credit[0], initial_group.source_credit[0]);
        assert_eq!(group.insurance, u128::from(INSURANCE - insurance));
        assert_eq!(group.insurance_domain_budget[0], group.insurance);
        assert_eq!(
            group.insurance_domain_budget[1..],
            initial_group.insurance_domain_budget[1..]
        );
        assert_eq!(
            group.insurance_domain_spent,
            initial_group.insurance_domain_spent
        );
        assert_eq!(group.source_claim_bound_total_num, 0);
        assert_eq!(
            env.svm.get_account(&env.market).unwrap().lamports,
            market_frame.lamports
        );
        assert_eq!(
            state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
                .unwrap(),
            profile
        );
        let mut expected_sequences = sequences;
        expected_sequences.authority_epoch += u64::from(insurance != 0);
        assert_eq!(env.control_sequences(0), expected_sequences);
        for (i, amount) in [
            remaining + fees[1],
            EARNINGS - fees[1],
            principal + fees[0],
            fees[1] - redeemed,
            insurance,
            0,
        ]
        .into_iter()
        .enumerate()
        {
            if i == 3 && fees[1] == redeemed {
                assert_absent(env, native_provider);
            } else {
                assert_eq!(
                    env.svm.get_account(&custody_keys[i]),
                    Some(token_image(i, amount))
                );
            }
        }
        assert_eq!(
            tokens.map(|key| env.token_amount(key)).iter().sum::<u64>()
                + env.token_amount(env.vault),
            SUPPLY
        );
        let native_custody = if fees[1] == redeemed {
            0
        } else {
            env.token_amount(native_provider)
        };
        assert_eq!(
            env.token_amount(native_vault) + native_custody + redeemed,
            EARNINGS
        );
        assert_eq!(env.token_amount(tokens[0]), PAYOUTS[0]);
        assert_eq!(env.token_amount(tokens[1]), PAYOUTS[1]);
        if paid == 0 {
            assert_eq!(env.svm.get_account(&ledger), Some(ledger_frame.clone()));
        } else {
            let account = env.svm.get_account(&ledger).unwrap();
            assert_eq!(account.lamports, ledger_frame.lamports);
            assert_eq!(account.owner, env.program_id);
            let record = state::read_backing_domain_ledger(&account.data).unwrap();
            assert_eq!(record.market_group, env.market.to_bytes());
            assert_eq!(record.authority, wallets[2].to_bytes());
            assert_eq!(record.domain, 1);
            assert_eq!(
                (record.total_principal_atoms, record.total_earnings_atoms),
                (0, 0)
            );
            assert_eq!(record.total_earnings_withdrawn_atoms, paid.into());
            assert_eq!(
                record.last_observed_bucket_earnings_atoms,
                u128::from(EARNINGS - paid)
            );
        }
        assert_market_stock_census(
            "native secondary earnings",
            &group,
            &env.svm.get_account(&env.market).unwrap().data,
            &[],
            remaining.into(),
        )
        .unwrap();
        assert_reservation_encumbrance_census("native secondary earnings", &group, &[]).unwrap();
    };
    check(&env, [0, redeemed], 0, 0);
    let first = reserve_payout(&env, wallets, tokens, ledger, 1, PREFIX);
    let first_allowed = [env.market, ledger, env.vault, tokens[2]];
    assert!(first.accounts.iter().all(|meta| !meta.is_signer));
    peak[1] = peak[1].max(land(
        &mut env,
        &[first],
        &[],
        &tracked,
        &first_allowed,
        0,
        None,
        None,
    ));
    check(&env, [PREFIX, redeemed], 0, 0);
    let paid_prefix_ledger = env.svm.get_account(&ledger).unwrap();
    let repair = Instruction {
        program_id: associated_token_program_id(),
        accounts: vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(native_provider, false),
            AccountMeta::new_readonly(wallets[2], false),
            AccountMeta::new_readonly(native_mint, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: vec![1],
    };
    let mut tail = reserve_payout(&env, wallets, tokens, ledger, 1, TAIL - redeemed);
    tail.accounts[3].pubkey = native_provider;
    tail.accounts[4].pubkey = native_vault;
    let prefix = [repair, tail];
    assert!(prefix
        .iter()
        .flat_map(|ix| &ix.accounts)
        .all(|meta| !meta.is_signer || meta.pubkey == env.payer.pubkey()));
    let overclaim = reserve_payout(&env, wallets, tokens, ledger, 1, 1);
    let rejected = [prefix[0].clone(), prefix[1].clone(), overclaim];
    peak[2] = land(
        &mut env,
        &rejected,
        &[],
        &tracked,
        &[],
        0,
        None,
        Some((4, PercolatorError::EngineLockActive)),
    );
    check(&env, [PREFIX, redeemed], 0, 0);
    assert_eq!(
        env.svm.get_account(&ledger),
        Some(paid_prefix_ledger.clone())
    );
    let allowed = [env.market, ledger, native_vault, native_provider];
    peak[1] = peak[1].max(land(
        &mut env,
        &prefix,
        &[],
        &tracked,
        &allowed,
        rent,
        None,
        None,
    ));
    check(&env, [PREFIX, TAIL], 0, 0);
    let mut expected_ledger = paid_prefix_ledger;
    let mut record = state::read_backing_domain_ledger(&expected_ledger.data).unwrap();
    record.total_earnings_withdrawn_atoms = EARNINGS.into();
    record.last_observed_bucket_earnings_atoms = 0;
    state::write_backing_domain_ledger(&mut expected_ledger.data, &record).unwrap();
    assert_eq!(env.svm.get_account(&ledger), Some(expected_ledger.clone()));
    for (kind, amount, actor) in [(0, BACKING, 2), (2, INSURANCE, 4)] {
        let ix = reserve_payout(&env, wallets, tokens, ledger, kind, amount);
        assert!(ix.accounts.iter().all(|meta| !meta.is_signer));
        let allowed = [env.market, env.vault, tokens[actor]];
        peak[1] = peak[1].max(land(
            &mut env,
            &[ix],
            &[],
            &tracked,
            &allowed,
            0,
            None,
            None,
        ));
        check(
            &env,
            [PREFIX, TAIL],
            BACKING,
            if kind == 2 { INSURANCE } else { 0 },
        );
        assert_eq!(env.svm.get_account(&ledger), Some(expected_ledger.clone()));
    }
    let admin_frame = env.svm.get_account(&wallets[4]).unwrap();
    let close = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(wallets[4], true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(tokens[4], false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(native_vault, false),
            AccountMeta::new(native_admin, false),
        ],
        data: ProgInstruction::CloseSlab {
            authority_epoch: env.control_sequences(0).authority_epoch,
        }
        .encode(),
    };
    let allowed = [
        env.market,
        env.vault,
        native_vault,
        tokens[4],
        native_admin,
        wallets[4],
    ];
    peak[3] = land(
        &mut env,
        &[close],
        &[&admin],
        &tracked,
        &allowed,
        0,
        None,
        None,
    );
    let tombstone = env.svm.get_account(&env.market).unwrap();
    assert_closed_market_tombstone(&tombstone);
    assert_eq!(
        tombstone.lamports,
        env.svm
            .minimum_balance_for_rent_exemption(constants::HEADER_LEN)
    );
    let mut expected_admin = admin_frame;
    expected_admin.lamports += market_frame.lamports - tombstone.lamports + 2 * rent + DONATION;
    assert_eq!(env.svm.get_account(&wallets[4]), Some(expected_admin));
    assert_absent(&env, env.vault);
    assert_absent(&env, native_vault);
    assert_absent(&env, wallets[2]);
    for (i, amount) in [
        (2, BACKING + PREFIX),
        (3, TAIL - redeemed),
        (4, INSURANCE + TAIL),
        (5, PREFIX),
    ] {
        assert_eq!(
            env.svm.get_account(&custody_keys[i]),
            Some(token_image(i, amount))
        );
    }
    assert_eq!(env.svm.get_account(&ledger), Some(expected_ledger));
    for (key, frame) in [spl_mint, native_mint].into_iter().zip(mint_frames) {
        assert_eq!(env.svm.get_account(&key), Some(frame));
    }
    assert_eq!(
        tokens.map(|key| env.token_amount(key)).iter().sum::<u64>(),
        SUPPLY
    );
    assert_eq!(
        env.token_amount(native_provider) + env.token_amount(native_admin) + redeemed,
        EARNINGS
    );
    for value in peak {
        assert_cu_within("native secondary earnings", value, 1_200_000);
    }
    let payments = 4 + usize::from(redeem_prefix);
    eprintln!("row433 native-secondary earnings: 1 world, {payments} unsigned payments, 1 exact rollback, 1 closure; fee={EARNINGS}, SPL={PREFIX}, native={TAIL}, redeemed={redeemed}, unsynced={DONATION}, peak CU [setup, payment, rejection, close]={peak:?}");
}
