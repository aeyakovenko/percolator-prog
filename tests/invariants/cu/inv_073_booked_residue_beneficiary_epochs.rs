//! Row 421 / INV-073: beneficiary epochs cross actual loss/recredit, unpaid
//! provider earnings and booked residue. Economic continuations need only the
//! payer after consented succession; final slab destruction retains its admin.

use super::*;

const RECOVERY_PREFIX: u64 = SPENT / 2;

fn assert_absent(env: &V16CuEnv, key: Pubkey) {
    assert!(env.svm.get_account(&key).is_none_or(|account| {
        account.lamports == 0
            && account.data.is_empty()
            && account.owner == solana_sdk::system_program::ID
            && !account.executable
    }));
}

fn handoff(env: &V16CuEnv, from: Pubkey, to: Pubkey, epoch: u64) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(from, true),
            AccountMeta::new_readonly(to, to != Pubkey::default()),
            AccountMeta::new(env.market, false),
        ],
        data: ProgInstruction::UpdateAssetAuthority {
            asset_index: 0,
            market_id: env.asset_market_id(0),
            authority_epoch: epoch,
            kind: processor::ASSET_AUTH_INSURANCE,
            new_pubkey: to.to_bytes(),
        }
        .encode(),
    }
}

fn repair(env: &V16CuEnv, wallet: Pubkey, token: Pubkey) -> Instruction {
    Instruction {
        program_id: associated_token_program_id(),
        accounts: vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(token, false),
            AccountMeta::new_readonly(wallet, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: vec![1],
    }
}

fn pay(env: &V16CuEnv, wallet: Pubkey, token: Pubkey, amount: u64) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(wallet, false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 0,
            market_id: env.asset_market_id(0),
            authority_epoch: env.control_sequences(0).authority_epoch,
            amount: amount.into(),
        }
        .encode(),
    }
}

fn run(native: bool, retained: u64) -> u64 {
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
    assert_eq!(
        (SPENT, AVAILABLE, PROVIDER_FEE, SOURCE_PRINCIPAL),
        (73, 176, 657, 1)
    );
    let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
    let vault_frame = env.svm.get_account(&env.vault).unwrap();
    let market_frame = env.svm.get_account(&env.market).unwrap();
    let profile_frame = state::read_asset_oracle_profile(&market_frame.data, 0).unwrap();
    let sequences = env.control_sequences(0);
    let mut epoch = sequences.authority_epoch;
    let rent = env
        .svm
        .minimum_balance_for_rent_exemption(TokenAccount::LEN);
    for (actor, signer) in [(2, &incumbent), (3, &successor), (4, &insurer)] {
        assert_eq!(env.token_amount(tokens[actor]), 0);
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::close_account(
                &spl_token::ID,
                &tokens[actor],
                &wallets[actor],
                &wallets[actor],
                &[],
            )
            .unwrap(),
            &[signer],
        )
        .unwrap();
    }
    let provider_sol = env.svm.get_account(&wallets[2]).unwrap().lamports;
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        system_instruction::transfer(&wallets[2], &env.payer.pubkey(), provider_sol),
        &[&incumbent],
    )
    .unwrap();
    drop(incumbent);
    assert_absent(&env, wallets[2]);
    let absent_tokens = tokens.map(|key| env.svm.get_account(&key));
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
    let tracked = [
        env.market,
        env.vault,
        env.mint,
        env.vault_authority,
        ledger,
        admin.pubkey(),
        admin_token,
    ]
    .into_iter()
    .chain(wallets)
    .chain(tokens)
    .chain(portfolios)
    .collect::<Vec<_>>();
    assert!(!wallets.contains(&env.payer.pubkey()));
    assert!(!wallets.contains(&admin.pubkey()));
    let check =
        |env: &V16CuEnv, book: &Book, paid_b: u64, created: [bool; 5], holder: Pubkey, epoch| {
            let group = env.market_state().1;
            let expected = book.expected();
            let mut amounts = expected.recipients;
            amounts[3] = paid_b;
            amounts[4] -= paid_b;
            assert_eq!(amounts.iter().sum::<u64>() + expected.custody, SUPPLY);
            for actor in 0..5 {
                let image = if created[actor] {
                    Some(token_image(&token_frames[actor], amounts[actor]))
                } else {
                    assert_eq!(amounts[actor], 0);
                    absent_tokens[actor].clone()
                };
                assert_eq!(env.svm.get_account(&tokens[actor]), image);
            }
            assert_eq!(
                env.svm.get_account(&env.vault),
                Some(token_image(&vault_frame, expected.custody))
            );
            assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
            assert_eq!(env.token_amount(admin_token), 0);
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
            assert_eq!(group.vault, u128::from(expected.custody));
            assert_eq!(group.insurance, u128::from(expected.stock[3]));
            assert_eq!(
                group.insurance_domain_spent,
                [0, u128::from(expected.spent)]
            );
            let long_paid = book.paid[3].min(INSURANCE);
            assert_eq!(
                group.insurance_domain_budget,
                [
                    u128::from(INSURANCE - long_paid),
                    u128::from(INSURANCE_FEE - (book.paid[3] - long_paid)),
                ]
            );
            assert_eq!(
                group.insurance_domain_budget_remaining_total,
                group.insurance
            );
            assert_eq!(
                group.backing_provider_earnings_total,
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
            assert_eq!(
                group.source_backing_buckets[1].utilization_fee_earnings,
                u128::from(expected.stock[2])
            );
            let market = env.svm.get_account(&env.market).unwrap();
            let mut profile = profile_frame;
            profile.insurance_authority = holder.to_bytes();
            assert_eq!(
                state::read_asset_oracle_profile(&market.data, 0).unwrap(),
                profile
            );
            let mut expected_sequences = sequences;
            expected_sequences.authority_epoch = epoch;
            assert_eq!(env.control_sequences(0), expected_sequences);
            assert_eq!(market.lamports, market_frame.lamports);
            assert_market_stock_census(
                "row421 booked succession",
                &group,
                &market.data,
                &[],
                group.vault,
            )
            .unwrap();
            assert_reservation_encumbrance_census("row421 booked succession", &group, &[]).unwrap();
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
    let mut land_checked = |env: &mut V16CuEnv,
                            ixs: &[Instruction],
                            signers: &[&Keypair],
                            allowed: &[Pubkey],
                            rent,
                            refund,
                            rejection: Option<(u8, PercolatorError)>| {
        // The shared runner frames every tracked/message Account and checks each
        // successful prefix on errors. Also reconcile total SOL on success.
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
        assert!(signers
            .iter()
            .all(|signer| signer.pubkey() != env.payer.pubkey()));
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
        assert_cu_within("row421 booked beneficiary epochs", cu, LIMIT);
        peak = peak.max(cu);
    };
    let close = |env: &V16CuEnv| {
        let mut ix = Instruction {
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
            ix.accounts.push(AccountMeta::new(tokens[4], false));
        }
        ix
    };
    let mut denied = close(&env);
    denied.accounts[0] = AccountMeta::new(wallets[2], false);
    let mut book = Book {
        residual: retained,
        paid: [0; 4],
        expired: false,
        recredited: 0,
    };
    let mut created = [true, true, false, false, false];
    let mut paid_b = 0;
    check(&env, &book, paid_b, created, wallets[4], epoch);

    // Recover source rounding principal and most provider principal while its
    // owner and custody are absent; keep earned fees senior to final retirement.
    for (class, amount) in [(0, SOURCE_PRINCIPAL), (1, BACKING - retained)] {
        let mut ix = reserve_payout(&env, wallets, tokens, ledger, 0, amount);
        ix.data = ProgInstruction::WithdrawBackingBucket {
            domain: class as u16,
            market_id: env.asset_market_id(0),
            authority_epoch: epoch,
            amount: amount.into(),
        }
        .encode();
        let mut batch = Vec::new();
        if !created[2] {
            batch.push(repair(&env, wallets[2], tokens[2]));
        }
        batch.push(ix);
        let mut failed = batch.clone();
        failed.push(denied.clone());
        land_checked(
            &mut env,
            &failed,
            &[],
            &[],
            0,
            None,
            Some((2 + batch.len() as u8, PercolatorError::ExpectedSigner)),
        );
        let allowed = [env.market, env.vault, tokens[2]];
        land_checked(
            &mut env,
            &batch,
            &[],
            &allowed,
            if created[2] { 0 } else { rent },
            None,
            None,
        );
        created[2] = true;
        book.pay(class, amount);
        check(&env, &book, paid_b, created, wallets[4], epoch);
    }
    let initial = pay(&env, wallets[4], tokens[4], AVAILABLE);
    let batch = [repair(&env, wallets[4], tokens[4]), initial];
    let allowed = [env.market, env.vault, tokens[4]];
    land_checked(&mut env, &batch, &[], &allowed, rent, None, None);
    book.pay(3, AVAILABLE);
    epoch += 1;
    created[4] = true;
    check(&env, &book, paid_b, created, wallets[4], epoch);
    let stale_a = pay(&env, wallets[4], tokens[4], SPENT - RECOVERY_PREFIX);
    let transfer = handoff(&env, wallets[4], wallets[3], epoch);
    let allowed = [env.market];
    land_checked(
        &mut env,
        &[transfer],
        &[&insurer, &successor],
        &allowed,
        0,
        None,
        None,
    );
    epoch += 1;
    check(&env, &book, paid_b, created, wallets[3], epoch);

    env.svm.warp_to_slot(100);
    let normalize = close(&env);
    land_checked(&mut env, &[normalize], &[&admin], &allowed, 0, None, None);
    book.expire();
    check(&env, &book, paid_b, created, wallets[3], epoch);
    let prefix = pay(&env, wallets[3], tokens[3], RECOVERY_PREFIX);
    let fix_b = repair(&env, wallets[3], tokens[3]);
    // The first B payout lazily recredits all 73 spent atoms. Reusing its old
    // epoch as a suffix must undo that recredit, SPL transfer and ATA creation.
    land_checked(
        &mut env,
        &[fix_b.clone(), prefix.clone(), prefix.clone()],
        &[],
        &[],
        0,
        None,
        Some((4, PercolatorError::EngineStale)),
    );
    check(&env, &book, paid_b, created, wallets[3], epoch);
    let allowed = [env.market, env.vault, tokens[3]];
    land_checked(&mut env, &[fix_b, prefix], &[], &allowed, rent, None, None);
    created[3] = true;
    book.pay(3, RECOVERY_PREFIX);
    paid_b = RECOVERY_PREFIX;
    epoch += 1;
    check(&env, &book, paid_b, created, wallets[3], epoch);
    assert_eq!(
        book.expected().stock[2..],
        [PROVIDER_FEE, SPENT - RECOVERY_PREFIX]
    );
    let restore = handoff(&env, wallets[3], wallets[4], epoch);
    land_checked(
        &mut env,
        &[restore.clone(), stale_a.clone()],
        &[&successor, &insurer],
        &[],
        0,
        None,
        Some((3, PercolatorError::EngineStale)),
    );
    let allowed = [env.market];
    land_checked(
        &mut env,
        &[restore],
        &[&successor, &insurer],
        &allowed,
        0,
        None,
        None,
    );
    epoch += 1;
    check(&env, &book, paid_b, created, wallets[4], epoch);
    land_checked(
        &mut env,
        &[stale_a],
        &[],
        &[],
        0,
        None,
        Some((2, PercolatorError::EngineStale)),
    );

    for signer in [&insurer, &successor] {
        let key = signer.pubkey();
        let balance = env.svm.get_account(&key).unwrap().lamports;
        let departure = system_instruction::transfer(&key, &admin.pubkey(), balance);
        land_checked(
            &mut env,
            &[departure],
            &[signer],
            &[key, admin.pubkey()],
            0,
            None,
            None,
        );
        assert_absent(&env, key);
    }
    drop((insurer, successor));
    let final_insurance = pay(&env, wallets[4], tokens[4], SPENT - RECOVERY_PREFIX);
    let premature = close(&env);
    land_checked(
        &mut env,
        &[premature],
        &[&admin],
        &[],
        0,
        None,
        Some((2, PercolatorError::EngineLockActive)),
    );
    land_checked(
        &mut env,
        &[final_insurance.clone(), denied.clone()],
        &[],
        &[],
        0,
        None,
        Some((3, PercolatorError::ExpectedSigner)),
    );
    let allowed = [env.market, env.vault, tokens[4]];
    land_checked(&mut env, &[final_insurance], &[], &allowed, 0, None, None);
    book.pay(3, SPENT - RECOVERY_PREFIX);
    epoch += 1;
    check(&env, &book, paid_b, created, wallets[4], epoch);
    // Even with insurance exhausted, booked provider earnings cannot be retired.
    let earnings = reserve_payout(&env, wallets, tokens, ledger, 1, PROVIDER_FEE);
    let premature = close(&env);
    land_checked(
        &mut env,
        &[premature],
        &[&admin],
        &[],
        0,
        None,
        Some((2, PercolatorError::EngineLockActive)),
    );
    land_checked(
        &mut env,
        &[earnings.clone(), denied.clone()],
        &[],
        &[],
        0,
        None,
        Some((3, PercolatorError::ExpectedSigner)),
    );
    let allowed = [env.market, env.vault, tokens[2], ledger];
    land_checked(&mut env, &[earnings], &[], &allowed, 0, None, None);
    book.pay(2, PROVIDER_FEE);
    check(&env, &book, paid_b, created, wallets[4], epoch);
    let residue = retained - SPENT;
    assert_eq!(book.rank(), (0, 0));
    assert_eq!(book.expected().custody, residue);
    let final_close = close(&env);
    if native {
        let mut old_destination = final_close.clone();
        old_destination.accounts[7] = AccountMeta::new(tokens[3], false);
        land_checked(
            &mut env,
            &[old_destination],
            &[&admin],
            &[],
            0,
            None,
            Some((2, PercolatorError::InvalidTokenAccount)),
        );
    }
    // This prefix actually burns/escheats the booked stock, closes the vault,
    // refunds rent and writes the tombstone before the unsigned suffix fails.
    land_checked(
        &mut env,
        &[final_close.clone(), denied],
        &[&admin],
        &[],
        0,
        None,
        Some((3, PercolatorError::ExpectedSigner)),
    );
    check(&env, &book, paid_b, created, wallets[4], epoch);
    let tombstone_rent = env
        .svm
        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
    let refund = market_frame.lamports + rent - tombstone_rent;
    let paid_wallets = wallets.map(|key| env.svm.get_account(&key));
    let paid_ledger = env.svm.get_account(&ledger);
    let allowed = [env.market, env.vault, env.mint, tokens[4]];
    land_checked(
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
    let mut amounts = book.expected().recipients;
    amounts[3] = paid_b;
    amounts[4] += if native { residue } else { 0 };
    amounts[4] -= paid_b;
    for actor in 0..5 {
        assert_eq!(
            env.svm.get_account(&tokens[actor]),
            Some(token_image(&token_frames[actor], amounts[actor]))
        );
    }
    assert_eq!(wallets.map(|key| env.svm.get_account(&key)), paid_wallets);
    assert_eq!(env.svm.get_account(&ledger), paid_ledger);
    let mut expected_mint = mint_frame;
    if !native {
        let mut mint = Mint::unpack(&expected_mint.data).unwrap();
        mint.supply -= residue;
        Mint::pack(mint, &mut expected_mint.data).unwrap();
    }
    assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
    assert_eq!(
        amounts.iter().sum::<u64>() + if native { 0 } else { residue },
        SUPPLY
    );
    assert_eq!(env.token_amount(admin_token), 0);
    assert_eq!(rollbacks, if native { 11 } else { 10 });
    eprintln!("row421 booked epochs: native={native}, retained={retained}, recovered={SPENT}, residue={residue}, rollbacks={rollbacks}, peak={peak} CU");
    peak
}

#[test]
fn v16_program_booked_recredit_and_provider_fees_survive_beneficiary_epoch_restoration() {
    for native in [false, true] {
        for retained in [SPENT + 1, 101] {
            run(native, retained);
        }
    }
}
