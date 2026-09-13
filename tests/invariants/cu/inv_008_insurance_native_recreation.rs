//! INV-008/010/024/031/064/080/081, reopening 428:
//! a-value-withdrawal-intent-cannot-spend-insurance-stock-after-its-bound-epoch-changes.
//!
//! Native insurance redemption closes a recipient, returns SOL and rent, and
//! publicly recreates the same seeded address before an oracle-role epoch change
//! and independent insurance refill. An old withdrawal suffix must restore that
//! entire prefix; its unchanged signed alternative then commits. Another stale
//! suffix restores a current-epoch payout and a second recipient recreation.
//! Exact lamport, token, stock, ledger and role frames accompany fresh exits.
//!
//! One bounded Live history, not intrinsic withdrawal-stock consumption: only
//! the shared authority epoch binds this withdrawal. Arbitrary histories,
//! liabilities, other rails, terminal recredit and durable nonces remain open.
//! The existing fixture supplies native-mint genesis; all economic transitions
//! and recipient lifetimes use public System/SPL/wrapper instructions.

use super::*;
use crate::inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[test]
fn v16_retained_insurance_epoch_survives_native_redemption_and_recipient_recreation() {
    const ORIGINAL: u64 = 37;
    const REFILL: u64 = 83;
    const LIMIT: u64 = 200_000;
    let mut env = inv081_public_native_market();
    let admin = env.admin.insecure_clone();
    let successor = Keypair::new();
    env.svm.airdrop(&successor.pubkey(), 1_000_000).unwrap();
    let program_id = env.program_id;
    let market = env.market;
    let vault = env.vault;
    let mint = env.mint;
    let vault_authority = env.vault_authority;
    let wallet = admin.pubkey();
    let source = create_ata_for_test(&mut env.svm, &env.payer, wallet, mint);
    let seed = "row428-native-recipient";
    let recipient = Pubkey::create_with_seed(&wallet, seed, &spl_token::ID).unwrap();
    let rent = env
        .svm
        .minimum_balance_for_rent_exemption(TokenAccount::LEN);
    let recreate = vec![
        system_instruction::create_account_with_seed(
            &wallet,
            &recipient,
            &wallet,
            seed,
            rent,
            TokenAccount::LEN as u64,
            &spl_token::ID,
        ),
        spl_token::instruction::initialize_account3(&spl_token::ID, &recipient, &mint, &wallet)
            .unwrap(),
    ];
    send_raw_ixs(&mut env.svm, &env.payer, recreate.clone(), &[&admin]).unwrap();
    let ledger_key = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &ledger_key,
        state::insurance_ledger_account_len(),
        program_id,
    );
    let ledger = ledger_key.pubkey();
    let empty_ledger = env.svm.get_account(&ledger).unwrap();
    let token_keys = [vault, source, recipient];
    let empty_tokens = token_keys.map(|key| env.svm.get_account(&key).unwrap());
    send_raw_ixs(
        &mut env.svm,
        &env.payer,
        vec![
            system_instruction::transfer(&wallet, &source, ORIGINAL + REFILL),
            spl_token::instruction::sync_native(&spl_token::ID, &source).unwrap(),
        ],
        &[&admin],
    )
    .unwrap();
    let market_id = env.asset_market_id(0);
    let epoch = env.control_sequences(0).authority_epoch;
    let initial_intent = env.control_sequences(0).insurance_top_up + 1;
    let top_up = |domain, authority_epoch, intent_id, amount| Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new_readonly(wallet, true),
            AccountMeta::new(market, false),
            AccountMeta::new(source, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        data: ProgInstruction::TopUpInsuranceDomain {
            domain,
            market_id,
            authority_epoch,
            intent_id,
            amount,
        }
        .encode(),
    };
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        top_up(0, epoch, initial_intent, ORIGINAL.into()),
        &[&admin],
    )
    .unwrap();
    let initial_market = env.market_state();
    let initial_market_account = env.svm.get_account(&market).unwrap();
    let controls = env.control_sequences(0);
    let profile = state::read_asset_oracle_profile(&initial_market_account.data, 0).unwrap();
    let mint_frame = env.svm.get_account(&mint).unwrap();
    let wallet_frame = env.svm.get_account(&wallet).unwrap();
    let successor_frame = env.svm.get_account(&successor.pubkey());
    let tracked = [
        market,
        vault,
        source,
        recipient,
        ledger,
        wallet,
        mint,
        vault_authority,
        successor.pubkey(),
    ];
    let withdrawal = |authority_epoch, amount| Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new_readonly(wallet, true),
            AccountMeta::new(market, false),
            AccountMeta::new(recipient, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        data: ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 0,
            market_id,
            authority_epoch,
            amount,
        }
        .encode(),
    };
    let original = withdrawal(epoch, ORIGINAL.into());
    let fresh = withdrawal(epoch + 1, ORIGINAL.into());
    assert_eq!(original.accounts, fresh.accounts);
    assert_ne!(original.data, fresh.data);
    let redeem =
        spl_token::instruction::close_account(&spl_token::ID, &recipient, &wallet, &wallet, &[])
            .unwrap();
    let handoff = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new_readonly(wallet, true),
            AccountMeta::new_readonly(successor.pubkey(), true),
            AccountMeta::new(market, false),
        ],
        data: ProgInstruction::UpdateAssetAuthority {
            asset_index: 0,
            market_id,
            authority_epoch: epoch,
            kind: processor::ASSET_AUTH_ORACLE,
            new_pubkey: successor.pubkey().to_bytes(),
        }
        .encode(),
    };
    let transaction = |env: &V16CuEnv, instructions: &[Instruction], nonce: u32, handoff: bool| {
        // Distinct compute limits make pre-signed envelopes unique without
        // changing the retained instruction, slot or authorization fields.
        let mut batch = vec![
            heap_ix(),
            ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32 - nonce),
        ];
        batch.extend_from_slice(instructions);
        let mut signers = vec![&env.payer, &admin];
        if handoff {
            signers.push(&successor);
        }
        let tx = Transaction::new_signed_with_payer(
            &batch,
            Some(&env.payer.pubkey()),
            &signers,
            env.svm.latest_blockhash(),
        );
        tx.verify().unwrap();
        assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
        tx
    };
    let retained = [1, 2].map(|nonce| transaction(&env, &[original.clone()], nonce, false));
    let retained_bytes = retained
        .each_ref()
        .map(|tx| bincode::serialize(tx).unwrap());
    let mut prefix = vec![original.clone(), redeem.clone()];
    prefix.extend(recreate.clone());
    prefix.extend([
        handoff,
        top_up(1, epoch + 1, initial_intent + 1, REFILL.into()),
    ]);
    let valid_prefix = transaction(&env, &prefix, 3, true);
    let prefix_bytes = bincode::serialize(&valid_prefix).unwrap();
    env.svm
        .simulate_transaction(retained[0].clone().into())
        .unwrap();
    env.svm
        .simulate_transaction(valid_prefix.clone().into())
        .unwrap();

    let check = |env: &V16CuEnv, replenished: bool, redeemed: u64, closed: bool| {
        let deposited = ORIGINAL + if replenished { REFILL } else { 0 };
        let stock = deposited - redeemed;
        let mut expected = initial_market.clone();
        expected.1.insurance = stock.into();
        expected.1.vault = stock.into();
        expected.1.insurance_domain_budget[0] = if replenished { 0 } else { stock.into() };
        expected.1.insurance_domain_budget[1] = if replenished { stock.into() } else { 0 };
        expected.1.insurance_domain_budget_remaining_total = stock.into();
        assert_eq!(env.market_state(), expected);
        let market_account = env.svm.get_account(&market).unwrap();
        assert_eq!(market_account.lamports, initial_market_account.lamports);
        let mut expected_controls = controls;
        let mut expected_profile = profile;
        if replenished {
            expected_controls.authority_epoch += 1;
            expected_controls.insurance_top_up += 1;
            expected_profile.oracle_authority = successor.pubkey().to_bytes();
        }
        assert_eq!(env.control_sequences(0), expected_controls);
        assert_eq!(
            state::read_asset_oracle_profile(&market_account.data, 0).unwrap(),
            expected_profile
        );
        let record = env.svm.get_account(&ledger).unwrap();
        assert_eq!(
            state::read_insurance_ledger(&record.data).unwrap(),
            state::InsuranceLedgerAccountV16 {
                market_group: market.to_bytes(),
                authority: wallet.to_bytes(),
                total_principal_atoms: stock.into(),
                total_deposited_atoms: deposited.into(),
                total_withdrawn_atoms: redeemed.into(),
                cumulative_profit_atoms: 0,
                cumulative_loss_atoms: 0,
                last_observed_insurance_atoms: stock.into(),
            }
        );
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
        for (index, key) in token_keys.into_iter().enumerate() {
            if index == 2 && closed {
                assert!(env
                    .svm
                    .get_account(&key)
                    .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                continue;
            }
            let amount = match index {
                0 => stock,
                1 => {
                    if replenished {
                        0
                    } else {
                        REFILL
                    }
                }
                _ => 0,
            };
            let mut expected = empty_tokens[index].clone();
            let mut token = TokenAccount::unpack(&expected.data).unwrap();
            assert_eq!(token.is_native, COption::Some(expected.lamports));
            assert_eq!(token.amount, 0);
            token.amount = amount;
            TokenAccount::pack(token, &mut expected.data).unwrap();
            expected.lamports += amount;
            assert_eq!(
                env.svm.get_account(&key),
                Some(expected),
                "native Account {key}"
            );
        }
        let mut expected_wallet = wallet_frame.clone();
        expected_wallet.lamports += redeemed + if closed { rent } else { 0 };
        assert_eq!(env.svm.get_account(&wallet), Some(expected_wallet));
        assert_eq!(env.svm.get_account(&mint), Some(mint_frame.clone()));
        assert_eq!(env.svm.get_account(&successor.pubkey()), successor_frame);
        assert_market_stock_census(
            "native retained stock",
            &expected.1,
            &market_account.data,
            &[],
            stock.into(),
        )
        .unwrap();
        assert_reservation_encumbrance_census("native retained stock", &expected.1, &[]).unwrap();
    };
    let mut rollbacks = 0;
    let mut successes = 0;
    let mut peak = 0;
    let mut deliver =
        |env: &mut V16CuEnv, tx: Transaction, error_index: Option<u8>, completed: [usize; 3]| {
            tx.verify().unwrap();
            let mut keys = tx.message.account_keys.clone();
            keys.extend_from_slice(&tracked);
            keys.sort_unstable();
            keys.dedup();
            let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
            let mut expected_payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
            expected_payer.lamports -= FeeStructure::default().lamports_per_signature
                * u64::from(tx.message.header.num_required_signatures);
            let result = env.svm.send_transaction(tx);
            let meta = if let Some(index) = error_index {
                let failure = result.expect_err("old authority epoch must propagate EngineStale");
                assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(
                        index,
                        InstructionError::Custom(PercolatorError::EngineStale as u32)
                    )
                );
                for (key, account) in keys.iter().zip(before) {
                    if *key != env.payer.pubkey() {
                        assert_eq!(env.svm.get_account(key), account, "complete rollback {key}");
                    }
                }
                rollbacks += 1;
                failure.meta
            } else {
                successes += 1;
                result.expect("unchanged signed public continuation")
            };
            assert_eq!(
                env.svm.get_account(&env.payer.pubkey()),
                Some(expected_payer)
            );
            for (program, count) in [program_id, spl_token::ID, solana_sdk::system_program::ID]
                .into_iter()
                .zip(completed)
            {
                assert_eq!(
                    meta.logs
                        .iter()
                        .filter(|line| **line == format!("Program {program} success"))
                        .count(),
                    count,
                    "completed public prefix: {:?}",
                    meta.logs
                );
            }
            assert!(meta.compute_units_consumed > 0);
            assert_cu_within(
                "native recipient recreation retry",
                meta.compute_units_consumed,
                LIMIT,
            );
            peak = peak.max(meta.compute_units_consumed);
        };

    check(&env, false, 0, false);
    prefix.push(original.clone());
    let aborted = transaction(&env, &prefix, 4, true);
    deliver(&mut env, aborted, Some(8), [3, 4, 1]);
    check(&env, false, 0, false);
    env.svm
        .simulate_transaction(retained[0].clone().into())
        .unwrap();
    assert_eq!(bincode::serialize(&valid_prefix).unwrap(), prefix_bytes);
    deliver(&mut env, valid_prefix, None, [3, 4, 1]);
    check(&env, true, ORIGINAL, false);
    assert_eq!(bincode::serialize(&retained[0]).unwrap(), retained_bytes[0]);
    deliver(&mut env, retained[0].clone(), Some(2), [0, 0, 0]);
    check(&env, true, ORIGINAL, false);

    let mut fresh_prefix = vec![fresh, redeem.clone()];
    fresh_prefix.extend(recreate);
    let valid_fresh = transaction(&env, &fresh_prefix, 5, false);
    let fresh_bytes = bincode::serialize(&valid_fresh).unwrap();
    env.svm
        .simulate_transaction(valid_fresh.clone().into())
        .unwrap();
    fresh_prefix.push(original);
    let aborted_fresh = transaction(&env, &fresh_prefix, 6, false);
    deliver(&mut env, aborted_fresh, Some(6), [1, 3, 1]);
    check(&env, true, ORIGINAL, false);
    assert_eq!(bincode::serialize(&valid_fresh).unwrap(), fresh_bytes);
    deliver(&mut env, valid_fresh, None, [1, 3, 1]);
    check(&env, true, 2 * ORIGINAL, false);
    assert!(
        REFILL - ORIGINAL >= ORIGINAL,
        "the retained debit is fully funded"
    );
    assert_eq!(bincode::serialize(&retained[1]).unwrap(), retained_bytes[1]);
    deliver(&mut env, retained[1].clone(), Some(2), [0, 0, 0]);
    check(&env, true, 2 * ORIGINAL, false);
    let drain = transaction(
        &env,
        &[withdrawal(epoch + 1, (REFILL - ORIGINAL).into()), redeem],
        7,
        false,
    );
    deliver(&mut env, drain, None, [1, 2, 0]);
    check(&env, true, ORIGINAL + REFILL, true);
    assert_eq!((rollbacks, successes), (4, 3));
    println!("INV-008 native insurance recreation: worlds=1, exact_rollbacks={rollbacks}, signed_continuations={successes}, peak_CU={peak}, redeemed_atoms={}", ORIGINAL + REFILL);
}
