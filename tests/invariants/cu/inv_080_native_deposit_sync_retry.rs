//! INV-080: a failed native deposit restores a preceding public SyncNative.
//! INV-018/024/081 support: raw SOL and rent are not depositable token balance;
//! retry credits only the signed amount and owner withdrawal redeems exact SOL.
//! Two bounded Live histories use the existing native-mint genesis fixture and
//! public System/ATA/SPL/wrapper operations for every economic state transition.
//! No terminal, maximum-shape, arbitrary-history or whole-invariant claim.

use super::*;
use crate::inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[test]
fn v16_program_native_deposit_cpi_failure_restores_sync_and_signed_retry() {
    let mut peak_cu = 0;
    let mut rejections = 0;
    for amount in [1u64, 137] {
        let mut env = inv081_public_native_market();
        let owner = Keypair::new();
        env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        let portfolio_key = Keypair::new();
        let portfolio = portfolio_key.pubkey();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &portfolio_key,
            env.portfolio_account_len,
            env.program_id,
        );
        env.send(
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new_readonly(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[&owner],
        )
        .unwrap();
        env.portfolios.push(portfolio);
        let source = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
        let empty_source = env.svm.get_account(&source).unwrap();
        let empty_vault = env.svm.get_account(&env.vault).unwrap();
        let rent = env
            .svm
            .minimum_balance_for_rent_exemption(TokenAccount::LEN);
        assert_eq!(empty_source.lamports, rent);
        assert_eq!(empty_vault.lamports, rent);
        let funded = 2 * amount + 19;
        send_raw_ixs(
            &mut env.svm,
            &env.payer,
            vec![
                system_instruction::transfer(&owner.pubkey(), &source, funded),
                spl_token::instruction::approve(
                    &spl_token::ID,
                    &source,
                    &owner.pubkey(),
                    &owner.pubkey(),
                    &[],
                    amount - 1,
                )
                .unwrap(),
            ],
            &[&owner],
        )
        .unwrap();
        let unsynced = env.svm.get_account(&source).unwrap();
        let token = TokenAccount::unpack(&unsynced.data).unwrap();
        assert_eq!(token.amount, 0);
        assert_eq!(token.is_native, COption::Some(rent));
        assert_eq!(token.delegate, COption::Some(owner.pubkey()));
        assert_eq!(token.delegated_amount, amount - 1);
        assert_eq!(unsynced.lamports, rent + funded);

        let deposit = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new_readonly(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: env.deposit_ix(portfolio, amount.into()).encode(),
        };
        let sync = spl_token::instruction::sync_native(&spl_token::ID, &source).unwrap();
        let prefix = [sync.clone(), deposit.clone()];
        let sequence = env.portfolio_matcher_sequence(portfolio);
        let identity = env.portfolio_id(portfolio);
        let position_epoch = env.portfolio_position_epoch(portfolio);
        let reject = |env: &mut V16CuEnv,
                      instructions: &[Instruction],
                      code: u32,
                      synced: bool,
                      reached_cpi: bool| {
            env.svm.expire_blockhash();
            let mut ixs = vec![heap_ix(), cu_ix()];
            ixs.extend_from_slice(instructions);
            let tx = Transaction::new_signed_with_payer(
                &ixs,
                Some(&env.payer.pubkey()),
                &[&env.payer, &owner],
                env.svm.latest_blockhash(),
            );
            let fee = FeeStructure::default().lamports_per_signature
                * u64::from(tx.message.header.num_required_signatures);
            let mut keys = tx.message.account_keys.clone();
            keys.extend([env.mint, env.admin.pubkey(), env.vault_authority]);
            keys.sort_unstable();
            keys.dedup();
            let before: Vec<_> = keys
                .iter()
                .map(|key| (*key, env.svm.get_account(key)))
                .collect();
            let failure = env
                .svm
                .send_transaction(tx)
                .expect_err("deposit must reject");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    (ixs.len() - 1) as u8,
                    InstructionError::Custom(code),
                )
            );
            let logs = &failure.meta.logs;
            assert_eq!(
                logs.iter()
                    .filter(|line| **line == format!("Program {} success", spl_token::ID))
                    .count(),
                usize::from(synced),
                "SyncNative must complete before the wrapper rejection"
            );
            assert_eq!(
                logs.contains(&format!("Program {} invoke [2]", spl_token::ID)),
                reached_cpi,
                "the late failure must come from the deposit's token CPI"
            );
            assert!(!logs.contains(&format!("Program {} success", env.program_id)));
            if reached_cpi {
                assert!(logs.contains(&"Program log: Instruction: Transfer".to_owned()));
                assert!(logs.contains(&format!(
                    "Program {} failed: custom program error: 0x1",
                    spl_token::ID
                )));
            }
            for (key, mut account) in before {
                if key == env.payer.pubkey() {
                    account.as_mut().unwrap().lamports -= fee;
                }
                assert_eq!(env.svm.get_account(&key), account, "exact rollback {key}");
            }
            assert_cu_within(
                "native deposit rejection",
                failure.meta.compute_units_consumed,
                CUSTODY_CU_LIMIT,
            );
            failure.meta.compute_units_consumed
        };

        // Raw SOL is not SPL spendable balance until synchronization succeeds.
        peak_cu = peak_cu.max(reject(
            &mut env,
            std::slice::from_ref(&deposit),
            PercolatorError::InvalidTokenAccount as u32,
            false,
            false,
        ));
        rejections += 1;
        // Sync makes the balance sufficient; the self-delegate allowance fails
        // inside SPL after the wrapper has credited capital and advanced consent.
        peak_cu = peak_cu.max(reject(
            &mut env,
            &prefix,
            spl_token::error::TokenError::InsufficientFunds as u32,
            true,
            true,
        ));
        rejections += 1;
        assert_eq!(env.svm.get_account(&source), Some(unsynced));
        assert_eq!(env.svm.get_account(&env.vault), Some(empty_vault.clone()));
        assert_eq!(env.portfolio_matcher_sequence(portfolio), sequence);
        assert_eq!(env.portfolio_state(portfolio).capital.get(), 0);
        assert_eq!(
            (env.market_state().1.vault, env.market_state().1.c_tot),
            (0, 0)
        );

        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::revoke(&spl_token::ID, &source, &owner.pubkey(), &[]).unwrap(),
            &[&owner],
        )
        .unwrap();
        assert_eq!(
            env.token_amount(source),
            0,
            "repair must not synchronize SOL"
        );
        let revoked_source = env.svm.get_account(&source).unwrap();
        assert_eq!(
            TokenAccount::unpack(&revoked_source.data).unwrap(),
            TokenAccount::unpack(&empty_source.data).unwrap()
        );
        let retry_cu = send_raw_ixs(&mut env.svm, &env.payer, prefix.to_vec(), &[&owner])
            .expect("the unchanged SyncNative/deposit prefix remains executable");
        assert_cu_within("native deposit retry", retry_cu, CUSTODY_CU_LIMIT);
        peak_cu = peak_cu.max(retry_cu);
        assert_eq!(env.portfolio_id(portfolio), identity);
        assert_eq!(env.portfolio_position_epoch(portfolio), position_epoch);
        assert_eq!(env.portfolio_matcher_sequence(portfolio), sequence + 1);
        assert_eq!(env.portfolio_state(portfolio).capital.get(), amount.into());
        let (_, group) = env.market_state();
        assert_eq!(
            (group.vault, group.c_tot, group.insurance),
            (amount.into(), amount.into(), 0)
        );
        assert_eq!(group.mode, MarketModeV16::Live);
        for (key, empty, balance) in [
            (source, &revoked_source, funded - amount),
            (env.vault, &empty_vault, amount),
        ] {
            let mut expected = empty.clone();
            let mut token = TokenAccount::unpack(&expected.data).unwrap();
            token.amount = balance;
            TokenAccount::pack(token, &mut expected.data).unwrap();
            expected.lamports = rent + balance;
            assert_eq!(env.svm.get_account(&key), Some(expected));
        }
        assert!(
            env.token_amount(source) > amount,
            "stale replay is not underfunded"
        );
        peak_cu = peak_cu.max(reject(
            &mut env,
            &prefix,
            PercolatorError::EngineStale as u32,
            true,
            false,
        ));
        rejections += 1;

        let withdrawal = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new_readonly(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: env.withdraw_ix(portfolio, amount.into()).encode(),
        };
        let mut redeemed_owner = env.svm.get_account(&owner.pubkey()).unwrap();
        redeemed_owner.lamports += rent + funded;
        let exit_cu = send_raw_ixs(
            &mut env.svm,
            &env.payer,
            vec![
                withdrawal,
                spl_token::instruction::close_account(
                    &spl_token::ID,
                    &source,
                    &owner.pubkey(),
                    &owner.pubkey(),
                    &[],
                )
                .unwrap(),
            ],
            &[&owner],
        )
        .expect("withdraw the exact credit and redeem all native SOL and token rent");
        assert_cu_within(
            "native withdrawal and redemption",
            exit_cu,
            CUSTODY_CU_LIMIT,
        );
        peak_cu = peak_cu.max(exit_cu);
        assert_eq!(env.svm.get_account(&owner.pubkey()), Some(redeemed_owner));
        assert!(env
            .svm
            .get_account(&source)
            .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
        assert_eq!(env.svm.get_account(&env.vault), Some(empty_vault));
        assert_eq!(env.portfolio_state(portfolio).capital.get(), 0);
        assert_eq!(env.portfolio_matcher_sequence(portfolio), sequence + 2);
        assert_eq!(
            (env.market_state().1.vault, env.market_state().1.c_tot),
            (0, 0)
        );
    }
    assert_eq!(rejections, 6);
    println!("INV-080 native sync/deposit: 2 worlds, 6 exact rollbacks, 2 unchanged retries, 2 funded withdrawals/redemptions; peak CU {peak_cu}");
}
