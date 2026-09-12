//! INV-070 / row 418: late native wrapping across a pending PnL claim and atomic close.
//! A committed lamport donation survives a failed suffix; its wrapping, final user
//! payout, dematerialization and slab close roll back together before valid retry.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[test]
fn v16_program_native_pnl_terminal_sync_and_close_retry_preserves_unsynced_donation() {
    use inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;

    const CAPITAL: [u64; 2] = [1_000, 1_300];
    const ENTRY: u64 = 100;
    const EXIT: u64 = 110;
    const PAYOUT: [u64; 2] = [CAPITAL[0] + EXIT - ENTRY, CAPITAL[1] - (EXIT - ENTRY)];
    const DONATION: u64 = 37;
    const LIMIT: u64 = 500_000;

    let mut env = inv081_public_native_market();
    let admin = env.admin.insecure_clone();
    let owners = [Keypair::new(), Keypair::new()];
    let owner_keys = owners.each_ref().map(Signer::pubkey);
    let destinations =
        owner_keys.map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner, env.mint));
    let admin_token = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
    let empty_tokens = [destinations[0], destinations[1], admin_token, env.vault]
        .map(|key| env.svm.get_account(&key).unwrap());
    let mint_before = env.svm.get_account(&env.mint);
    let rent = env
        .svm
        .minimum_balance_for_rent_exemption(TokenAccount::LEN);
    let native_account = |index: usize, amount: u64, unsynced: u64| {
        let mut expected = empty_tokens[index].clone();
        let mut token = TokenAccount::unpack(&expected.data).unwrap();
        assert_eq!(token.mint, spl_token::native_mint::ID);
        assert_eq!(token.is_native, COption::Some(rent));
        assert_eq!(token.amount, 0);
        assert_eq!(expected.lamports, rent);
        token.amount = amount;
        TokenAccount::pack(token, &mut expected.data).unwrap();
        expected.lamports += amount + unsynced;
        expected
    };
    let mut setup_peak = env.init_market_cu;
    let mut portfolios = [Pubkey::default(); 2];
    for i in 0..2 {
        env.svm.airdrop(&owner_keys[i], 1_000_000_000).unwrap();
        let key = Keypair::new();
        portfolios[i] = key.pubkey();
        setup_peak = setup_peak.max(system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &key,
            env.portfolio_account_len,
            env.program_id,
        ));
        setup_peak = setup_peak.max(
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owner_keys[i], true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                ],
                &[&owners[i]],
            )
            .unwrap(),
        );
        env.portfolios.push(portfolios[i]);
        setup_peak = setup_peak.max(
            send_raw_ixs(
                &mut env.svm,
                &env.payer,
                vec![
                    system_instruction::transfer(&owner_keys[i], &destinations[i], CAPITAL[i]),
                    spl_token::instruction::sync_native(&spl_token::ID, &destinations[i]).unwrap(),
                ],
                &[&owners[i]],
            )
            .unwrap(),
        );
        setup_peak = setup_peak.max(
            env.send(
                env.deposit_ix(portfolios[i], CAPITAL[i].into()),
                vec![
                    AccountMeta::new(owner_keys[i], true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                    AccountMeta::new(destinations[i], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[i]],
            )
            .unwrap(),
        );
    }
    setup_peak = setup_peak.max(env.configure_auth_mark_for_asset_as_admin(0, 0, ENTRY));
    setup_peak = setup_peak.max(env.configure_permissionless_resolve_with_cu(100, 3));
    setup_peak = setup_peak.max(env.trade_with_cu(
        &owners[0],
        portfolios[0],
        &owners[1],
        portfolios[1],
        POS_SCALE as i128,
        ENTRY,
        0,
    ));
    env.svm.warp_to_slot(1);
    setup_peak = setup_peak.max(env.push_auth_mark_with_cu(1, EXIT));
    setup_peak = setup_peak.max(env.crank(
        portfolios[1],
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    ));
    setup_peak = setup_peak.max(env.resolve());
    env.svm.warp_to_slot(4);
    drop(owners);
    let payouts: [Instruction; 2] = std::array::from_fn(|i| Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(owner_keys[i], false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolios[i], false),
            AccountMeta::new(destinations[i], false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        }
        .encode(),
    });
    let loser_cu = send_raw_ixs(
        &mut env.svm,
        &env.payer,
        vec![heap_ix(), cu_ix(), payouts[1].clone()],
        &[],
    )
    .unwrap();
    let pending = |env: &V16CuEnv, donated: bool| {
        let market = env.svm.get_account(&env.market).unwrap();
        let (cfg, group) = env.market_state();
        let ps = portfolios.map(|key| env.portfolio_state(key));
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(group.assets[0].effective_price, EXIT);
        assert_eq!(group.assets[0].oi_eff_long_q, POS_SCALE);
        assert_eq!(group.assets[0].oi_eff_short_q, 0);
        assert_eq!(group.materialized_portfolio_count, 2);
        assert_eq!(group.vault, PAYOUT[0].into());
        assert_eq!(group.insurance, 0);
        assert_eq!(cfg.terminal_slab_scan_progress, 0);
        assert!(!resolved_portfolio_is_terminal(env, portfolios[0]));
        assert!(resolved_portfolio_is_terminal(env, portfolios[1]));
        for (key, expected) in [
            (destinations[0], native_account(0, 0, 0)),
            (destinations[1], native_account(1, PAYOUT[1], 0)),
            (admin_token, native_account(2, 0, 0)),
            (
                env.vault,
                native_account(3, PAYOUT[0], if donated { DONATION } else { 0 }),
            ),
        ] {
            assert_eq!(env.svm.get_account(&key), Some(expected));
        }
        assert_eq!(env.svm.get_account(&env.mint), mint_before);
        assert_market_stock_census(
            "native pending PnL",
            &group,
            &market.data,
            &ps,
            PAYOUT[0].into(),
        )
        .unwrap();
        assert_reservation_encumbrance_census("native pending PnL", &group, &ps).unwrap();
        let mut data = market.data;
        let (_, view) = state::market_view_mut(&mut data).unwrap();
        view.validate_shape().unwrap();
        for key in portfolios {
            let mut data = env.svm.get_account(&key).unwrap().data;
            state::portfolio_view_mut_for_market_slots(&mut data, 1)
                .unwrap()
                .validate_with_market(&view.as_view())
                .unwrap();
        }
    };
    pending(&env, false);
    let claim_frame =
        [env.market, portfolios[0], portfolios[1]].map(|key| env.svm.get_account(&key));
    let donation_cu = send_raw_tx(
        &mut env.svm,
        &env.payer,
        system_instruction::transfer(&admin.pubkey(), &env.vault, DONATION),
        &[&admin],
    )
    .unwrap();
    assert_eq!(
        [env.market, portfolios[0], portfolios[1]].map(|key| env.svm.get_account(&key)),
        claim_frame
    );
    pending(&env, true);

    let mut prefix = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32),
        spl_token::instruction::sync_native(&spl_token::ID, &env.vault).unwrap(),
        payouts[0].clone(),
    ];
    for portfolio in portfolios {
        prefix.push(Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            data: env.close_portfolio_ix(portfolio).encode(),
        });
    }
    prefix.push(Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(admin_token, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::CloseSlab {
            authority_epoch: env.control_sequences(0).authority_epoch,
        }
        .encode(),
    });
    let mut aborted = prefix.clone();
    // The admin owns only the donation after close. A one-atom overdraft is a
    // deterministic public suffix failure after the entire valid prefix executes.
    aborted.push(
        spl_token::instruction::transfer(
            &spl_token::ID,
            &admin_token,
            &destinations[1],
            &admin.pubkey(),
            &[],
            DONATION + 1,
        )
        .unwrap(),
    );
    env.svm.expire_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &aborted,
        Some(&env.payer.pubkey()),
        &[&env.payer, &admin],
        env.svm.latest_blockhash(),
    );
    assert_eq!(tx.message.header.num_required_signatures, 2);
    assert!(bincode::serialize(&tx).unwrap().len() <= 1_232);
    let mut tracked = tx.message.account_keys.clone();
    tracked.extend([env.mint, env.vault_authority, owner_keys[0], owner_keys[1]]);
    tracked.sort_unstable();
    tracked.dedup();
    let frame: Vec<_> = tracked.iter().map(|key| env.svm.get_account(key)).collect();
    let fee = 2 * FeeStructure::default().lamports_per_signature;
    let failed = env
        .svm
        .send_transaction(tx)
        .expect_err("late overdraft must roll back close");
    assert_eq!(
        failed.err,
        TransactionError::InstructionError(
            prefix.len() as u8,
            InstructionError::Custom(spl_token::error::TokenError::InsufficientFunds as u32),
        )
    );
    let wrapper_success = format!("Program {} success", env.program_id);
    assert_eq!(failed.meta.logs.iter().filter(|log| **log == wrapper_success).count(), 4,
        "winner payout, both portfolio closes and slab close must succeed before the suffix rejects");
    let token_success = format!("Program {} success", spl_token::ID);
    assert_eq!(
        failed
            .meta
            .logs
            .iter()
            .filter(|log| **log == token_success)
            .count(),
        4,
        "native sync, user transfer, surplus transfer and vault close must all execute"
    );
    for (key, mut expected) in tracked.iter().zip(frame.clone()) {
        if *key == env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee;
        }
        assert_eq!(
            env.svm.get_account(key),
            expected,
            "complete failed Account frame {key}"
        );
    }
    pending(&env, true);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let portfolio_rent: u64 = portfolios
        .iter()
        .map(|key| env.svm.get_account(key).unwrap().lamports)
        .sum();
    let tombstone_rent = env
        .svm
        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
    let tracked_lamports = |env: &V16CuEnv| {
        tracked
            .iter()
            .filter_map(|key| env.svm.get_account(key))
            .map(|account| account.lamports)
            .sum::<u64>()
    };
    let before_lamports = tracked_lamports(&env);
    env.svm.expire_blockhash();
    let retry = Transaction::new_signed_with_payer(
        &prefix,
        Some(&env.payer.pubkey()),
        &[&env.payer, &admin],
        env.svm.latest_blockhash(),
    );
    assert_eq!(retry.message.header.num_required_signatures, 2);
    assert!(bincode::serialize(&retry).unwrap().len() <= 1_232);
    let committed = env
        .svm
        .send_transaction(retry)
        .expect("unchanged prefix must complete after rollback");
    for (key, mut expected) in tracked.iter().zip(frame) {
        if *key == env.market {
            let tombstone = env.svm.get_account(key).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(tombstone.lamports, tombstone_rent);
            continue;
        }
        if portfolios.contains(key) || *key == env.vault {
            assert!(env.svm.get_account(key).is_none_or(|account| {
                account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)
            }));
            continue;
        }
        if *key == destinations[0] {
            expected = Some(native_account(0, PAYOUT[0], 0));
        } else if *key == admin_token {
            expected = Some(native_account(2, DONATION, 0));
        } else if *key == admin.pubkey() {
            expected.as_mut().unwrap().lamports +=
                portfolio_rent + market_before.lamports + vault_before.lamports
                    - PAYOUT[0]
                    - DONATION
                    - tombstone_rent;
        } else if *key == env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= 2 * fee;
        }
        assert_eq!(
            env.svm.get_account(key),
            expected,
            "complete committed Account frame {key}"
        );
    }
    assert_eq!(tracked_lamports(&env), before_lamports - fee);
    assert_eq!(PAYOUT.iter().sum::<u64>(), CAPITAL.iter().sum::<u64>());
    let rollback_cu = failed.meta.compute_units_consumed;
    let retry_cu = committed.compute_units_consumed;
    for cu in [setup_peak, loser_cu, donation_cu, rollback_cu, retry_cu] {
        assert_cu_within("native PnL sync/close retry", cu, LIMIT);
    }
    println!("INV-070 native PnL sync retry: paid={PAYOUT:?}, synced_surplus={DONATION}, exact_rollbacks=1, close_calls=1, CU[setup,loser,donation,rollback,retry]=[{setup_peak},{loser_cu},{donation_cu},{rollback_cu},{retry_cu}]");
}
