//! Row 421 / INV-073: partial recovery with unavailable beneficiary consent and
//! delegated existing custody. Keeper-created custody preserves the same owner;
//! unrecovered historical spend must not pin an otherwise empty terminal slab.

use super::*;

fn run_partial(native: bool, retained: u64) {
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
    assert_eq!((SPENT, AVAILABLE, PROVIDER_FEE), (73, 176, 657));
    assert!(retained > 1 && retained < SPENT);

    // A paid prefix stays in the original ATA when its owner loses the ability
    // to revoke delegation or consent to succession. All setup is public.
    let initial = pay(&env, wallets[4], tokens[4], AVAILABLE);
    let allowed = [env.market, env.vault, tokens[4]];
    land(&mut env, &[initial], &[], &tokens, &allowed, 0, None, None);
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::approve(
            &spl_token::ID,
            &tokens[4],
            &wallets[3],
            &wallets[4],
            &[],
            1,
        )
        .unwrap(),
        &[&insurer],
    )
    .unwrap();
    for signer in [&incumbent, &successor, &insurer] {
        let key = signer.pubkey();
        let balance = env.svm.get_account(&key).unwrap().lamports;
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            system_instruction::transfer(&key, &admin.pubkey(), balance),
            &[signer],
        )
        .unwrap();
        assert_absent(&env, key);
    }
    drop((incumbent, successor, insurer));
    let seed = "row421-partial-replacement";
    let replacement = Pubkey::create_with_seed(&env.payer.pubkey(), seed, &spl_token::ID).unwrap();
    assert_ne!(replacement, canonical_vault_ata(wallets[4], env.mint));
    assert!(env.svm.get_account(&replacement).is_none());
    let rent = env
        .svm
        .minimum_balance_for_rent_exemption(TokenAccount::LEN);
    let creation = [
        system_instruction::create_account_with_seed(
            &env.payer.pubkey(),
            &replacement,
            &env.payer.pubkey(),
            seed,
            rent,
            TokenAccount::LEN as u64,
            &spl_token::ID,
        ),
        spl_token::instruction::initialize_account3(
            &spl_token::ID,
            &replacement,
            &env.mint,
            &wallets[4],
        )
        .unwrap(),
    ];
    let ledger = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &ledger,
        state::backing_domain_ledger_account_len(),
        env.program_id,
    );
    let ledger = ledger.pubkey();
    let empty_ledger = env.svm.get_account(&ledger).unwrap();
    let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
    let disabled = TokenAccount::unpack(&token_frames[4].data).unwrap();
    assert_eq!(disabled.delegate, COption::Some(wallets[3]));
    assert_eq!((disabled.amount, disabled.delegated_amount), (AVAILABLE, 1));
    let vault_frame = env.svm.get_account(&env.vault).unwrap();
    let market_frame = env.svm.get_account(&env.market).unwrap();
    let cfg_frame = env.market_state().0;
    let profile_frame = state::read_asset_oracle_profile(&market_frame.data, 0).unwrap();
    let sequences = env.control_sequences(0);
    let tracked = [
        env.market,
        env.vault,
        env.mint,
        env.vault_authority,
        admin.pubkey(),
        admin_token,
        ledger,
        replacement,
    ]
    .into_iter()
    .chain(wallets)
    .chain(tokens)
    .chain(portfolios)
    .collect::<Vec<_>>();
    assert!(!tracked.contains(&env.payer.pubkey()));
    assert!(!wallets.contains(&admin.pubkey()));

    let check = |env: &V16CuEnv, book: &Book, created: bool, epoch: u64| {
        let expected = book.expected();
        let (cfg, group) = env.market_state();
        assert_eq!(cfg, cfg_frame);
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
        assert_eq!(group.source_insurance_credit_reserved_total_atoms, 0);
        assert_eq!(group.vault, u128::from(expected.custody));
        assert_eq!(group.insurance, u128::from(expected.stock[3]));
        assert_eq!(
            group.insurance_domain_spent,
            [0, u128::from(expected.spent)]
        );
        assert_eq!(
            group.insurance_domain_budget,
            [0, u128::from(INSURANCE_FEE - (book.paid[3] - INSURANCE))]
        );
        assert_eq!(
            group.insurance_domain_budget_remaining_total,
            group.insurance
        );
        assert_eq!(
            group.backing_provider_earnings_total,
            u128::from(expected.stock[2])
        );
        assert_eq!(
            group.source_backing_buckets[1].utilization_fee_earnings,
            u128::from(expected.stock[2])
        );
        for domain in 0..2 {
            let bucket = group.source_backing_buckets[domain];
            assert_eq!(
                bucket.fresh_unliened_backing_num,
                u128::from(expected.stock[domain]) * BOUND_SCALE
            );
            assert_eq!(
                group.source_credit[domain].fresh_reserved_backing_num,
                bucket.fresh_unliened_backing_num
            );
            assert_eq!(bucket.valid_liened_backing_num, 0);
        }
        for actor in 0..5 {
            let amount = if actor == 4 {
                AVAILABLE
            } else {
                expected.recipients[actor]
            };
            assert_eq!(
                env.svm.get_account(&tokens[actor]),
                Some(token_image(&token_frames[actor], amount))
            );
        }
        let recovered_paid = book.paid[3] - AVAILABLE;
        if created {
            // This expected image is never installed in the VM.
            let mut image = token_frames[3].clone();
            let token = TokenAccount {
                mint: env.mint,
                owner: wallets[4],
                amount: recovered_paid,
                delegate: COption::None,
                state: AccountState::Initialized,
                is_native: if native {
                    COption::Some(rent)
                } else {
                    COption::None
                },
                delegated_amount: 0,
                close_authority: COption::None,
            };
            TokenAccount::pack(token, &mut image.data).unwrap();
            image.lamports = rent + if native { recovered_paid } else { 0 };
            assert_eq!(env.svm.get_account(&replacement), Some(image));
        } else {
            assert_eq!(recovered_paid, 0);
            assert!(env.svm.get_account(&replacement).is_none());
        }
        assert_eq!(
            env.svm.get_account(&env.vault),
            Some(token_image(&vault_frame, expected.custody))
        );
        assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
        assert_eq!(env.token_amount(admin_token), 0);
        assert_eq!(
            expected.recipients.iter().sum::<u64>() + expected.custody,
            SUPPLY
        );
        for key in &wallets[2..] {
            assert_absent(env, *key);
        }
        let market = env.svm.get_account(&env.market).unwrap();
        assert_eq!(
            state::read_asset_oracle_profile(&market.data, 0).unwrap(),
            profile_frame
        );
        let mut control = sequences;
        control.authority_epoch = epoch;
        assert_eq!(env.control_sequences(0), control);
        assert_market_stock_census(
            "partial recredit disabled custody",
            &group,
            &market.data,
            &[],
            group.vault,
        )
        .unwrap();
        assert_reservation_encumbrance_census("partial recredit disabled custody", &group, &[])
            .unwrap();
        state::market_view_mut(&mut market.data.clone())
            .unwrap()
            .1
            .validate_shape()
            .unwrap();
        let account = env.svm.get_account(&ledger).unwrap();
        if book.paid[2] == 0 {
            assert_eq!(account, empty_ledger);
        } else {
            let record = state::read_backing_domain_ledger(&account.data).unwrap();
            assert_eq!(record.market_group, env.market.to_bytes());
            assert_eq!(record.authority, wallets[2].to_bytes());
            assert_eq!(
                record.total_earnings_withdrawn_atoms,
                u128::from(book.paid[2])
            );
            assert_eq!(
                record.last_observed_bucket_earnings_atoms,
                u128::from(expected.stock[2])
            );
            assert_eq!(account.lamports, empty_ledger.lamports);
        }
    };
    let mut peak = 0;
    let mut rollbacks = 0;
    let mut execute = |env: &mut V16CuEnv,
                       ixs: &[Instruction],
                       signers: &[&Keypair],
                       allowed: &[Pubkey],
                       rent,
                       refund,
                       rejection: Option<(u8, PercolatorError)>| {
        // The shared runner may add the admin if requested. Require every signer
        // meta explicitly here so permissionless continuations cannot hide that.
        assert!(signers.iter().all(|key| key.pubkey() == admin.pubkey()));
        for meta in ixs
            .iter()
            .flat_map(|ix| &ix.accounts)
            .filter(|meta| meta.is_signer)
        {
            assert!(
                meta.pubkey == env.payer.pubkey()
                    || signers.iter().any(|key| key.pubkey() == meta.pubkey)
            );
        }
        let mut keys = tracked.clone();
        keys.push(env.payer.pubkey());
        keys.extend(
            ixs.iter()
                .flat_map(|ix| ix.accounts.iter().map(|meta| meta.pubkey)),
        );
        keys.sort_unstable();
        keys.dedup();
        let total = |env: &V16CuEnv| {
            keys.iter()
                .filter_map(|key| env.svm.get_account(key))
                .map(|a| u128::from(a.lamports))
                .sum::<u128>()
        };
        let before = total(env);
        rollbacks += usize::from(rejection.is_some());
        let cu = land(
            env, ixs, signers, &tracked, allowed, rent, refund, rejection,
        );
        assert_eq!(
            total(env),
            before
                - (1 + signers.len()) as u128
                    * u128::from(FeeStructure::default().lamports_per_signature)
        );
        assert_cu_within("partial recredit disabled custody", cu, LIMIT);
        peak = peak.max(cu);
    };
    let close = |env: &V16CuEnv| {
        let mut accounts = vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(admin_token, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(env.mint, false),
        ];
        if native {
            accounts.push(AccountMeta::new(replacement, false));
        }
        Instruction {
            program_id: env.program_id,
            accounts,
            data: ProgInstruction::CloseSlab {
                authority_epoch: env.control_sequences(0).authority_epoch,
            }
            .encode(),
        }
    };
    let mut denied = close(&env);
    denied.accounts[0] = AccountMeta::new(wallets[2], false);
    let mut book = Book {
        residual: retained,
        paid: [0, 0, 0, AVAILABLE],
        expired: false,
        recredited: 0,
    };
    let mut epoch = sequences.authority_epoch;
    check(&env, &book, false, epoch);

    let mut unsigned_handoff = handoff(&env, wallets[4], wallets[3], epoch);
    unsigned_handoff.accounts[0].is_signer = false;
    unsigned_handoff.accounts[1].is_signer = false;
    execute(
        &mut env,
        &[unsigned_handoff],
        &[],
        &[],
        0,
        None,
        Some((2, PercolatorError::ExpectedSigner)),
    );
    let seizure = handoff(&env, admin.pubkey(), admin.pubkey(), epoch);
    execute(
        &mut env,
        &[seizure],
        &[&admin],
        &[],
        0,
        None,
        Some((2, PercolatorError::EngineLockActive)),
    );
    let invalid = pay(&env, wallets[4], tokens[4], 1);
    let ata = repair(&env, wallets[4], tokens[4]);
    execute(
        &mut env,
        &[ata, invalid],
        &[],
        &[],
        0,
        None,
        Some((3, PercolatorError::InvalidTokenAccount)),
    );
    check(&env, &book, false, epoch);

    for (domain, amount) in [(0, SOURCE_PRINCIPAL), (1, BACKING - retained)] {
        let mut payout = reserve_payout(&env, wallets, tokens, ledger, 0, amount);
        payout.data = ProgInstruction::WithdrawBackingBucket {
            domain: domain as u16,
            market_id: env.asset_market_id(0),
            authority_epoch: epoch,
            amount: amount.into(),
        }
        .encode();
        let allowed = [env.market, env.vault, tokens[2]];
        execute(&mut env, &[payout], &[], &allowed, 0, None, None);
        book.pay(domain, amount);
        check(&env, &book, false, epoch);
    }
    env.svm.warp_to_slot(100);
    // Expiry normalization needs an admin. It must not consume insurance recovery
    // or the provider's earned fees before the public payout is attempted.
    let mut normalize = close(&env);
    if native {
        normalize.accounts[7].pubkey = tokens[4];
    }
    let allowed = [env.market];
    execute(&mut env, &[normalize], &[&admin], &allowed, 0, None, None);
    book.expire();
    check(&env, &book, false, epoch);

    let excessive = pay(&env, wallets[4], replacement, retained + 1);
    let mut batch = creation.to_vec();
    batch.push(excessive);
    execute(
        &mut env,
        &batch,
        &[],
        &[],
        0,
        None,
        Some((4, PercolatorError::EngineLockActive)),
    );
    check(&env, &book, false, epoch);
    let prefix = pay(&env, wallets[4], replacement, 1);
    let mut batch = creation.to_vec();
    batch.extend([prefix.clone(), prefix.clone()]);
    execute(
        &mut env,
        &batch,
        &[],
        &[],
        0,
        None,
        Some((5, PercolatorError::EngineStale)),
    );
    check(&env, &book, false, epoch);
    batch.pop();
    let allowed = [env.market, env.vault, replacement];
    execute(&mut env, &batch, &[], &allowed, rent, None, None);
    book.pay(3, 1);
    epoch += 1;
    check(&env, &book, true, epoch);
    assert_eq!(book.recredited, retained);
    assert_eq!(book.expected().spent, SPENT - retained);
    execute(
        &mut env,
        &[prefix],
        &[],
        &[],
        0,
        None,
        Some((2, PercolatorError::EngineStale)),
    );
    let still_disabled = pay(&env, wallets[4], tokens[4], retained - 1);
    execute(
        &mut env,
        &[still_disabled],
        &[],
        &[],
        0,
        None,
        Some((2, PercolatorError::InvalidTokenAccount)),
    );
    let premature = close(&env);
    execute(
        &mut env,
        &[premature],
        &[&admin],
        &[],
        0,
        None,
        Some((2, PercolatorError::EngineLockActive)),
    );
    let tail = pay(&env, wallets[4], replacement, retained - 1);
    execute(
        &mut env,
        &[tail.clone(), denied.clone()],
        &[],
        &[],
        0,
        None,
        Some((3, PercolatorError::ExpectedSigner)),
    );
    check(&env, &book, true, epoch);
    execute(&mut env, &[tail], &[], &allowed, 0, None, None);
    book.pay(3, retained - 1);
    epoch += 1;
    check(&env, &book, true, epoch);

    let premature = close(&env);
    execute(
        &mut env,
        &[premature],
        &[&admin],
        &[],
        0,
        None,
        Some((2, PercolatorError::EngineLockActive)),
    );
    let fees = reserve_payout(&env, wallets, tokens, ledger, 1, PROVIDER_FEE);
    execute(
        &mut env,
        &[fees.clone(), denied.clone()],
        &[],
        &[],
        0,
        None,
        Some((3, PercolatorError::ExpectedSigner)),
    );
    let allowed = [env.market, env.vault, tokens[2], ledger];
    execute(&mut env, &[fees], &[], &allowed, 0, None, None);
    book.pay(2, PROVIDER_FEE);
    check(&env, &book, true, epoch);
    assert_eq!(book.rank(), (0, 0));
    assert_eq!(book.expected().custody, 0);
    assert_eq!(book.expected().spent, SPENT - retained);

    let final_close = close(&env);
    execute(
        &mut env,
        &[final_close.clone(), denied],
        &[&admin],
        &[],
        0,
        None,
        Some((3, PercolatorError::ExpectedSigner)),
    );
    check(&env, &book, true, epoch);
    let tombstone_rent = env
        .svm
        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
    let refund = market_frame.lamports + rent - tombstone_rent;
    let allowed = [env.market, env.vault];
    execute(
        &mut env,
        &[final_close],
        &[&admin],
        &allowed,
        0,
        Some((admin.pubkey(), refund)),
        None,
    );
    let tombstone = env.svm.get_account(&env.market).unwrap();
    assert_closed_market_tombstone(&tombstone);
    assert_eq!(tombstone.lamports, tombstone_rent);
    assert_absent(&env, env.vault);
    assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame));
    assert_eq!(
        env.svm.get_account(&tokens[4]),
        Some(token_frames[4].clone())
    );
    assert_eq!(env.token_amount(replacement), retained);
    assert_eq!(
        env.token_amount(tokens[2]),
        SOURCE_PRINCIPAL + BACKING - retained + PROVIDER_FEE
    );
    assert_eq!(env.token_amount(admin_token), 0);
    assert_eq!(rollbacks, 12);
    eprintln!("row421 partial disabled custody: native={native}, recovered={retained}, unrecovered={}, original_paid={AVAILABLE}, replacement_paid={retained}, rollbacks={rollbacks}, peak={peak} CU", SPENT - retained);
}

#[test]
fn v16_program_partial_recredit_replaces_disabled_custody_without_succession_consent() {
    for native in [false, true] {
        for retained in [37, SPENT - 1] {
            run_partial(native, retained);
        }
    }
}
