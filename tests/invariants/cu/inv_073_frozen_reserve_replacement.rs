//! INV-018/021/024/070/073/081, row433: fresh beneficiary-owned SPL custody
//! permits terminal reserve disposition while the original ATAs remain frozen.
//! Account creation and a completed payout roll back together on an invalid suffix.

use super::*;
use terminal_public_reserves::reserve_payout;
use terminal_reserve_destination_recovery::land;

pub(crate) fn verify_frozen_reserve_replacement() {
    const CU_BOUND: u64 = 600_000;
    let freeze_authority = Keypair::new();
    let TerminalEarningsWorld {
        mut env,
        admin,
        incumbent,
        successor,
        mut wallets,
        mut tokens,
        portfolios,
        mint_frame,
    } = terminal_earnings_world_with_freeze_authority(true, Some(freeze_authority.pubkey()));
    let beneficiary = Keypair::new();
    for signer in [&beneficiary, &freeze_authority] {
        env.svm.airdrop(&signer.pubkey(), 1_000_000_000).unwrap();
    }
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
    let replacements = [Keypair::new(), Keypair::new()];
    let mut destinations = tokens;
    destinations[2] = replacements[0].pubkey();
    destinations[4] = replacements[1].pubkey();
    let tracked = [
        env.market,
        env.vault,
        env.vault_authority,
        env.mint,
        ledger,
        admin.pubkey(),
        admin_token,
        freeze_authority.pubkey(),
        solana_sdk::sysvar::clock::id(),
    ]
    .into_iter()
    .chain(wallets)
    .chain(tokens)
    .chain(destinations)
    .chain(portfolios)
    .collect::<Vec<_>>();
    let mut peaks = [0; 4]; // freeze, rejected creation/payout, replacement payout, close
    for actor in [2, 4] {
        let freeze = spl_token::instruction::freeze_account(
            &spl_token::ID,
            &tokens[actor],
            &env.mint,
            &freeze_authority.pubkey(),
            &[],
        )
        .unwrap();
        peaks[0] = peaks[0].max(land(
            &mut env,
            &[freeze],
            &[&freeze_authority],
            &tracked,
            &[tokens[actor]],
            0,
            None,
            None,
        ));
        let account = env.svm.get_account(&tokens[actor]).unwrap();
        let token = TokenAccount::unpack(&account.data).unwrap();
        assert_eq!(token.state, spl_token::state::AccountState::Frozen);
        assert_eq!(
            (token.owner, token.mint, token.amount),
            (wallets[actor], env.mint, 0)
        );
    }
    let mint = Mint::unpack(&mint_frame.data).unwrap();
    assert_eq!(
        mint.freeze_authority,
        COption::Some(freeze_authority.pubkey())
    );
    assert_eq!((mint.mint_authority, mint.supply), (COption::None, SUPPLY));
    assert!(!wallets.contains(&env.payer.pubkey()));
    for role in [
        wallets[2],
        wallets[3],
        wallets[4],
        freeze_authority.pubkey(),
    ] {
        assert_ne!(role, admin.pubkey());
        assert_ne!(role, env.payer.pubkey());
        assert!(replacements.iter().all(|key| key.pubkey() != role));
    }
    drop((incumbent, successor, beneficiary, freeze_authority));

    let original_frames = tokens.map(|key| env.svm.get_account(&key));
    let vault_frame = env.svm.get_account(&env.vault).unwrap();
    let (config, terminal) = env.market_state();
    let profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    let authority = env.control_sequences(0);
    let rent = env
        .svm
        .minimum_balance_for_rent_exemption(TokenAccount::LEN);
    let stock = |env: &V16CuEnv, paid: [u64; 3]| {
        let image = env.svm.get_account(&env.market).unwrap();
        let (current_config, group) = env.market_state();
        let remaining = BACKING + EARNINGS + INSURANCE - paid.iter().sum::<u64>();
        assert_eq!(current_config, config);
        let mut expected = terminal.clone();
        expected.vault = remaining.into();
        expected.insurance = (INSURANCE - paid[2]).into();
        expected.insurance_domain_budget[0] = expected.insurance;
        expected.insurance_domain_budget_remaining_total = expected.insurance;
        expected.backing_provider_earnings_total = (EARNINGS - paid[1]).into();
        expected.source_backing_buckets[1].utilization_fee_earnings =
            expected.backing_provider_earnings_total;
        let principal = u128::from(BACKING - paid[0]) * BOUND_SCALE;
        expected.source_backing_buckets[1].fresh_unliened_backing_num = principal;
        expected.source_credit[1].fresh_reserved_backing_num = principal;
        if principal == 0 {
            expected.source_backing_buckets[1].status = BackingBucketStatusV16::Expired;
            expected.source_credit[1].credit_epoch += 1;
            expected.risk_epoch += 1;
        }
        assert_eq!(group, expected);
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
        assert_eq!(env.control_sequences(0), authority);
        assert_eq!(
            state::read_asset_oracle_profile(&image.data, 0).unwrap(),
            profile
        );
        assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
        for (key, frame) in tokens.into_iter().zip(&original_frames) {
            assert_eq!(env.svm.get_account(&key), *frame);
        }
        assert_eq!(env.token_amount(admin_token), 0);
        for (actor, amount) in [(2, paid[0] + paid[1]), (4, paid[2])] {
            let key = destinations[actor];
            assert_ne!(key, canonical_vault_ata(wallets[actor], env.mint));
            if amount == 0 {
                assert!(env.svm.get_account(&key).is_none());
                continue;
            }
            let account = env.svm.get_account(&key).unwrap();
            assert_eq!(
                (
                    account.owner,
                    account.lamports,
                    account.executable,
                    account.rent_epoch
                ),
                (spl_token::ID, rent, false, 0)
            );
            assert_eq!(
                TokenAccount::unpack(&account.data).unwrap(),
                TokenAccount {
                    mint: env.mint,
                    owner: wallets[actor],
                    amount,
                    delegate: COption::None,
                    state: spl_token::state::AccountState::Initialized,
                    is_native: COption::None,
                    delegated_amount: 0,
                    close_authority: COption::None,
                }
            );
        }
        let mut expected_vault = vault_frame.clone();
        let mut token = TokenAccount::unpack(&expected_vault.data).unwrap();
        token.amount = remaining;
        TokenAccount::pack(token, &mut expected_vault.data).unwrap();
        assert_eq!(env.svm.get_account(&env.vault), Some(expected_vault));
        assert_eq!(
            PAYOUTS.iter().sum::<u64>() + paid.iter().sum::<u64>() + remaining,
            SUPPLY
        );
        let record = env.svm.get_account(&ledger).unwrap();
        if paid[1] == 0 {
            assert_eq!(record, empty_ledger);
        } else {
            assert_eq!(
                (
                    record.owner,
                    record.lamports,
                    record.executable,
                    record.rent_epoch
                ),
                (
                    empty_ledger.owner,
                    empty_ledger.lamports,
                    empty_ledger.executable,
                    empty_ledger.rent_epoch
                )
            );
            let record = state::read_backing_domain_ledger(&record.data).unwrap();
            assert_eq!(record.market_group, env.market.to_bytes());
            assert_eq!(record.authority, wallets[2].to_bytes());
            assert_eq!(record.total_earnings_withdrawn_atoms, paid[1].into());
            assert_eq!(
                record.last_observed_bucket_earnings_atoms,
                u128::from(EARNINGS - paid[1])
            );
        }
        crate::support::fuzz_model::assert_market_stock_census(
            "frozen reserve replacement",
            &group,
            &image.data,
            &[],
            remaining.into(),
        )
        .unwrap();
        crate::support::fuzz_model::assert_reservation_encumbrance_census(
            "frozen reserve replacement",
            &group,
            &[],
        )
        .unwrap();
    };
    stock(&env, [0; 3]);

    // The new account signs System creation only; SPL assigns custody to the absent holder.
    let mut paid = [0; 3];
    for (replacement, actor, kind, invalid_kind) in [(1, 4, 2, 0), (0, 2, 0, 1)] {
        let key = &replacements[replacement];
        let amount = [BACKING, EARNINGS, INSURANCE][kind];
        let payout = reserve_payout(&env, wallets, destinations, ledger, kind, amount);
        assert!(payout.accounts.iter().all(|meta| !meta.is_signer));
        let mut completion = vec![
            system_instruction::create_account(
                &env.payer.pubkey(),
                &key.pubkey(),
                rent,
                TokenAccount::LEN as u64,
                &spl_token::ID,
            ),
            spl_token::instruction::initialize_account3(
                &spl_token::ID,
                &key.pubkey(),
                &env.mint,
                &wallets[actor],
            )
            .unwrap(),
            payout,
        ];
        let completion_bytes = bincode::serialize(&completion).unwrap();
        let mut rejected = completion.clone();
        rejected.push(reserve_payout(
            &env,
            wallets,
            tokens,
            ledger,
            invalid_kind,
            1,
        ));
        peaks[1] = peaks[1].max(land(
            &mut env,
            &rejected,
            &[key],
            &tracked,
            &[],
            0,
            None,
            Some((5, PercolatorError::InvalidTokenAccount)),
        ));
        stock(&env, paid);
        assert_eq!(bincode::serialize(&completion).unwrap(), completion_bytes);
        if kind == 0 {
            let earnings = reserve_payout(&env, wallets, destinations, ledger, 1, EARNINGS);
            assert!(earnings.accounts.iter().all(|meta| !meta.is_signer));
            completion.push(earnings);
        }
        let allowed = [env.market, env.vault, key.pubkey(), ledger];
        peaks[2] = peaks[2].max(land(
            &mut env,
            &completion,
            &[key],
            &tracked,
            &allowed,
            rent,
            None,
            None,
        ));
        paid[kind] = amount;
        if kind == 0 {
            paid[1] = EARNINGS;
        }
        stock(&env, paid);
    }
    drop(replacements);
    assert_eq!(paid, [BACKING, EARNINGS, INSURANCE]);

    let close = Instruction {
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
            authority_epoch: authority.authority_epoch,
        }
        .encode(),
    };
    let tombstone_rent = env
        .svm
        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
    let refund =
        env.svm.get_account(&env.market).unwrap().lamports + vault_frame.lamports - tombstone_rent;
    let allowed = [env.market, env.vault];
    peaks[3] = land(
        &mut env,
        &[close],
        &[&admin],
        &tracked,
        &allowed,
        0,
        Some((admin.pubkey(), refund)),
        None,
    );
    let tombstone = env.svm.get_account(&env.market).unwrap();
    assert_closed_market_tombstone(&tombstone);
    assert_eq!(tombstone.lamports, tombstone_rent);
    assert!(env.svm.get_account(&env.vault).is_none_or(|account| {
        account.lamports == 0
            && account.data.is_empty()
            && account.owner == solana_sdk::system_program::ID
            && !account.executable
    }));
    assert_eq!(env.token_amount(destinations[2]), BACKING + EARNINGS);
    assert_eq!(env.token_amount(destinations[4]), INSURANCE);
    for (key, frame) in tokens.into_iter().zip(original_frames) {
        assert_eq!(env.svm.get_account(&key), frame);
    }
    assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame));
    for (label, cu) in [
        "freeze",
        "creation/payout rollback",
        "replacement payout",
        "slab close",
    ]
    .into_iter()
    .zip(peaks)
    {
        assert_cu_within(label, cu, CU_BOUND);
    }
    eprintln!("row433 frozen reserve replacement: 2 exact creation/payout rollbacks, 2 replacement accounts, 3 unsigned reserve payments, 1 slab close; peak CU [freeze, rejected bundle, replacement payout, close]={peaks:?}");
}
