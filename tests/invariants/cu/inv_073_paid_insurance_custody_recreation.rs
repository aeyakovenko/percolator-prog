//! Row 421 / INV-067/071/073/078/082: custody lifetime cannot reset a partially
//! paid insurance ledger or require either insurance role to finish payment.
//! Net-new: the same beneficiary's paid ATA is redeemed and recreated while its
//! funded ledger retains a nonzero remainder. Native ledger redemption uses two
//! different recipients; native recredit recreates donated custody without an
//! insurance ledger; recreated-reserve close exercises a provider ledger.
//! Here an unchanged remainder instruction survives ATA recreation, with atomic
//! and separately committed repair. Fully liquid overclaims still reject.

use super::*;
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

#[test]
fn v16_program_partial_insurance_ledger_survives_same_address_custody_recreation() {
    use inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;

    let mut peak = 0;
    let mut rollbacks = 0;
    for separate_repair in [false, true] {
        let mut env = inv081_public_native_market();
        let admin = env.admin.insecure_clone();
        let beneficiary = Keypair::new();
        let operator = Keypair::new();
        let wallets = [beneficiary.pubkey(), operator.pubkey()];
        for (kind, holder) in [
            (processor::ASSET_AUTH_INSURANCE, &beneficiary),
            (processor::ASSET_AUTH_INSURANCE_OPERATOR, &operator),
        ] {
            env.svm.airdrop(&holder.pubkey(), 1_000_000_000).unwrap();
            env.try_update_per_asset_authority_with_cu(
                &admin,
                Some(holder),
                0,
                kind,
                holder.pubkey().to_bytes(),
            )
            .unwrap();
        }
        assert!(!wallets.contains(&env.payer.pubkey()));
        assert!(!wallets.contains(&admin.pubkey()));
        let recipient = create_ata_for_test(&mut env.svm, &env.payer, wallets[0], env.mint);
        let admin_token = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
        let empty_recipient = env.svm.get_account(&recipient).unwrap();
        let empty_vault = env.svm.get_account(&env.vault).unwrap();
        let empty_admin_token = env.svm.get_account(&admin_token).unwrap();
        let ledger_key = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &ledger_key,
            state::insurance_ledger_account_len(),
            env.program_id,
        );
        let ledger = ledger_key.pubkey();
        send_raw_ixs(
            &mut env.svm,
            &env.payer,
            vec![
                system_instruction::transfer(&wallets[0], &recipient, FUNDED),
                spl_token::instruction::sync_native(&spl_token::ID, &recipient).unwrap(),
            ],
            &[&beneficiary],
        )
        .unwrap();
        env.send(
            ProgInstruction::TopUpInsurance {
                market_id: env.asset_market_id(0),
                authority_epoch: env.control_sequences(0).authority_epoch,
                intent_id: env.control_sequences(0).insurance_top_up + 1,
                amount: FUNDED.into(),
            },
            vec![
                AccountMeta::new(wallets[0], true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(recipient, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(ledger, false),
            ],
            &[&beneficiary],
        )
        .unwrap();
        drop((operator, ledger_key));
        env.resolve();
        // Surplus keeps every rejected debit physically payable, even after the
        // entitlement is exhausted. A token-balance failure cannot mask a reset.
        send_raw_ixs(
            &mut env.svm,
            &env.payer,
            vec![
                system_instruction::transfer(&admin.pubkey(), &env.vault, FUNDED),
                spl_token::instruction::sync_native(&spl_token::ID, &env.vault).unwrap(),
            ],
            &[&admin],
        )
        .unwrap();
        let initial = env.market_state();
        let market_frame = env.svm.get_account(&env.market).unwrap();
        let ledger_frame = env.svm.get_account(&ledger).unwrap();
        let controls = env.control_sequences(0);
        let profile = state::read_asset_oracle_profile(&market_frame.data, 0).unwrap();
        let tracked = [
            env.market,
            env.vault,
            env.mint,
            env.vault_authority,
            recipient,
            ledger,
            wallets[0],
            wallets[1],
            admin.pubkey(),
            admin_token,
        ];
        let withdrawal = |epoch, amount| Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new_readonly(wallets[0], false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(recipient, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(ledger, false),
            ],
            data: ProgInstruction::WithdrawInsuranceAsset {
                asset_index: 0,
                market_id: env.asset_market_id(0),
                authority_epoch: epoch,
                amount,
            }
            .encode(),
        };
        let first = withdrawal(controls.authority_epoch, FIRST.into());
        let remainder = withdrawal(controls.authority_epoch + 1, (FUNDED - FIRST).into());
        let excessive = withdrawal(controls.authority_epoch + 1, FUNDED.into());
        let exhausted = withdrawal(controls.authority_epoch + 2, 1);
        let remainder_bytes = remainder.data.clone();
        assert!(remainder.accounts.iter().all(|meta| !meta.is_signer));
        let changed = [env.market, env.vault, recipient, ledger];
        peak = peak.max(execute(
            &mut env,
            &[first.clone()],
            &[],
            &tracked,
            &changed,
            0,
        ));

        let check = |env: &V16CuEnv, paid: u64, payments: u64| {
            let remaining = FUNDED - paid;
            let mut expected = initial.clone();
            expected.1.vault = remaining.into();
            expected.1.insurance = remaining.into();
            expected.1.insurance_domain_budget[0] = (FUNDED / 2).saturating_sub(paid).into();
            expected.1.insurance_domain_budget[1] =
                (FUNDED / 2 - paid.saturating_sub(FUNDED / 2)).into();
            expected.1.insurance_domain_budget_remaining_total = remaining.into();
            assert_eq!(env.market_state(), expected);
            let mut expected_controls = controls;
            expected_controls.authority_epoch += payments;
            assert_eq!(env.control_sequences(0), expected_controls);
            let market = env.svm.get_account(&env.market).unwrap();
            assert_eq!(
                state::read_asset_oracle_profile(&market.data, 0).unwrap(),
                profile
            );
            assert_eq!(market.lamports, market_frame.lamports);
            let record = env.svm.get_account(&ledger).unwrap();
            assert_eq!(
                state::read_insurance_ledger(&record.data).unwrap(),
                state::InsuranceLedgerAccountV16 {
                    market_group: env.market.to_bytes(),
                    authority: wallets[0].to_bytes(),
                    total_principal_atoms: remaining.into(),
                    total_deposited_atoms: FUNDED.into(),
                    total_withdrawn_atoms: paid.into(),
                    cumulative_profit_atoms: 0,
                    cumulative_loss_atoms: 0,
                    last_observed_insurance_atoms: remaining.into(),
                }
            );
            let mut metadata = record;
            metadata.data.clone_from(&ledger_frame.data);
            assert_eq!(metadata, ledger_frame);
            assert_eq!(
                env.svm.get_account(&env.vault),
                Some(native_image(&empty_vault, remaining + FUNDED, 0))
            );
            assert_market_stock_census(
                "recreated insurance custody",
                &expected.1,
                &market.data,
                &[],
                (env.token_amount(env.vault) - FUNDED).into(),
            )
            .unwrap();
            assert_reservation_encumbrance_census("recreated insurance custody", &expected.1, &[])
                .unwrap();
        };
        check(&env, FIRST, 1);
        assert_eq!(
            env.svm.get_account(&recipient),
            Some(native_image(&empty_recipient, FIRST, 0))
        );
        let mut beneficiary_frame = env.svm.get_account(&wallets[0]).unwrap();
        let redeem = spl_token::instruction::close_account(
            &spl_token::ID,
            &recipient,
            &wallets[0],
            &wallets[0],
            &[],
        )
        .unwrap();
        peak = peak.max(execute(
            &mut env,
            &[redeem],
            &[&beneficiary],
            &tracked,
            &[recipient, wallets[0]],
            0,
        ));
        beneficiary_frame.lamports += empty_recipient.lamports + FIRST;
        assert_eq!(
            env.svm.get_account(&wallets[0]),
            Some(beneficiary_frame.clone())
        );
        assert!(env
            .svm
            .get_account(&recipient)
            .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
        check(&env, FIRST, 1);
        drop(beneficiary);

        let repair = Instruction {
            program_id: associated_token_program_id(),
            accounts: vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(recipient, false),
                AccountMeta::new_readonly(wallets[0], false),
                AccountMeta::new_readonly(env.mint, false),
                AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: vec![1],
        };
        let reject = |env: &mut V16CuEnv,
                      prefix: &[Instruction],
                      error: PercolatorError,
                      successes: usize| {
            env.svm.expire_blockhash();
            let mut batch = vec![
                heap_ix(),
                ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT as u32),
            ];
            batch.extend_from_slice(prefix);
            let tx = Transaction::new_signed_with_payer(
                &batch,
                Some(&env.payer.pubkey()),
                &[&env.payer],
                env.svm.latest_blockhash(),
            );
            tx.verify().unwrap();
            assert_eq!(tx.message.header.num_required_signatures, 1);
            assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
            let mut keys = tx.message.account_keys.clone();
            keys.extend_from_slice(&tracked);
            keys.sort_unstable();
            keys.dedup();
            let frames: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
            let failure = env.svm.send_transaction(tx).unwrap_err();
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    (prefix.len() + 1) as u8,
                    InstructionError::Custom(error as u32)
                )
            );
            for (program, count) in [
                (env.program_id, successes),
                (
                    associated_token_program_id(),
                    prefix
                        .iter()
                        .filter(|ix| ix.program_id == associated_token_program_id())
                        .count(),
                ),
            ] {
                assert_eq!(
                    failure
                        .meta
                        .logs
                        .iter()
                        .filter(|line| **line == format!("Program {program} success"))
                        .count(),
                    count
                );
            }
            for (key, mut frame) in keys.iter().zip(frames) {
                if *key == env.payer.pubkey() {
                    frame.as_mut().unwrap().lamports -=
                        FeeStructure::default().lamports_per_signature;
                }
                assert_eq!(
                    env.svm.get_account(key),
                    frame,
                    "complete recreation rollback {key}"
                );
            }
            assert_cu_within(
                "insurance recreation rollback",
                failure.meta.compute_units_consumed,
                CU_LIMIT,
            );
            failure.meta.compute_units_consumed
        };
        peak = peak.max(reject(
            &mut env,
            &[repair.clone(), excessive],
            PercolatorError::EngineLockActive,
            0,
        ));
        rollbacks += 1;
        let prefix = if separate_repair {
            peak = peak.max(execute(
                &mut env,
                &[repair.clone()],
                &[],
                &tracked,
                &[recipient],
                empty_recipient.lamports,
            ));
            assert_eq!(
                env.svm.get_account(&recipient),
                Some(empty_recipient.clone())
            );
            check(&env, FIRST, 1);
            vec![remainder.clone()]
        } else {
            vec![repair, remainder.clone()]
        };
        let mut rejected = prefix.clone();
        rejected.push(first);
        peak = peak.max(reject(&mut env, &rejected, PercolatorError::EngineStale, 1));
        rollbacks += 1;
        check(&env, FIRST, 1);
        assert_eq!(remainder.data, remainder_bytes);
        peak = peak.max(execute(
            &mut env,
            &prefix,
            &[],
            &tracked,
            &changed,
            if separate_repair {
                0
            } else {
                empty_recipient.lamports
            },
        ));
        check(&env, FUNDED, 2);
        assert_eq!(
            env.svm.get_account(&recipient),
            Some(native_image(&empty_recipient, FUNDED - FIRST, 0))
        );
        assert_eq!(env.svm.get_account(&wallets[0]), Some(beneficiary_frame));
        peak = peak.max(reject(
            &mut env,
            &[exhausted],
            PercolatorError::EngineLockActive,
            0,
        ));
        rollbacks += 1;

        let close = Instruction {
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
                authority_epoch: controls.authority_epoch + 2,
            }
            .encode(),
        };
        let mut admin_frame = env.svm.get_account(&admin.pubkey()).unwrap();
        let tombstone_rent = env
            .svm
            .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
        admin_frame.lamports += market_frame.lamports + empty_vault.lamports - tombstone_rent;
        let changes = [env.market, env.vault, admin_token, admin.pubkey()];
        peak = peak.max(execute(
            &mut env,
            &[close],
            &[&admin],
            &tracked,
            &changes,
            0,
        ));
        let tombstone = env.svm.get_account(&env.market).unwrap();
        assert_closed_market_tombstone(&tombstone);
        assert_eq!(tombstone.lamports, tombstone_rent);
        assert_eq!(env.svm.get_account(&admin.pubkey()), Some(admin_frame));
        assert_eq!(
            env.svm.get_account(&admin_token),
            Some(native_image(&empty_admin_token, FUNDED, 0))
        );
        assert!(env
            .svm
            .get_account(&env.vault)
            .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
    }
    assert_eq!(rollbacks, 6);
    println!("INV-073 paid insurance custody recreation: worlds=2, keeper_payments=4, exact_rollbacks={rollbacks}, paid_per_world={FUNDED}, peak_CU={peak}");
}
