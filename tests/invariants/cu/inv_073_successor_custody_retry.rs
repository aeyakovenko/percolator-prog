//! Row 421 / INV-073: terminal insurance payout remains live when beneficiary
//! succession, missing successor custody, and a stale former-beneficiary ledger
//! collide. A keeper-created successor ATA plus an unsigned payout must roll
//! back exactly if a later suffix carries the old ledger, then the same public
//! repair/payment shape must finish once the stale optional ledger is omitted.
//! Paid-prefix succession additionally crosses the first domain's depletion boundary
//! on classic and native rails. The former beneficiary keeps only its committed
//! prefix; keeper repair pays the successor only the remainder, through slab close.

use super::*;
use solana_sdk::{
    fee::FeeStructure,
    instruction::{Instruction, InstructionError},
    transaction::{Transaction, TransactionError},
};

const BUDGETS: [u64; 2] = [19, 28];
const FUNDED: u64 = BUDGETS[0] + BUDGETS[1];
const PREFIX: u64 = 17;

#[path = "inv_073_successor_expiry_residue.rs"]
mod successor_expiry_residue;

#[path = "inv_073_disabled_beneficiary_restoration.rs"]
mod disabled_beneficiary_restoration;

#[path = "inv_073_split_beneficiary_custody.rs"]
mod split_beneficiary_custody;

fn ata(wallet: Pubkey, mint: Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[wallet.as_ref(), spl_token::ID.as_ref(), mint.as_ref()],
        &associated_token_program_id(),
    )
    .0
}

fn insurance_succession_tx(
    env: &mut V16CuEnv,
    instructions: &[Instruction],
    signers: &[&Keypair],
    tracked: &[Pubkey],
    failure: Option<(u8, u32, usize)>,
) -> u64 {
    env.svm.expire_blockhash();
    let mut batch = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(CUSTODY_CU_LIMIT as u32),
    ];
    batch.extend_from_slice(instructions);
    let mut signing = vec![&env.payer];
    signing.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &batch,
        Some(&env.payer.pubkey()),
        &signing,
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert_eq!(
        usize::from(tx.message.header.num_required_signatures),
        signing.len()
    );
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let result = env.svm.send_transaction(tx);
    let meta = if let Some((index, code, successes)) = failure {
        let rejected = result.expect_err("retained insurance or cleanup suffix rejects");
        assert_eq!(
            rejected.err,
            TransactionError::InstructionError(index, InstructionError::Custom(code))
        );
        assert_eq!(
            rejected
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", env.program_id))
                .count(),
            successes
        );
        for (key, mut expected) in keys.iter().zip(before) {
            if *key == env.payer.pubkey() {
                expected.as_mut().unwrap().lamports -=
                    signing.len() as u64 * FeeStructure::default().lamports_per_signature;
            }
            assert_eq!(
                env.svm.get_account(key),
                expected,
                "complete rollback {key}"
            );
        }
        rejected.meta
    } else {
        result.expect("public insurance succession continuation")
    };
    assert_cu_within(
        "insurance succession",
        meta.compute_units_consumed,
        CUSTODY_CU_LIMIT,
    );
    meta.compute_units_consumed
}

#[test]
fn v16_program_successor_custody_repair_retries_after_stale_former_insurance_ledger() {
    run_successor_custody_retry(false, 0);
}

#[test]
fn v16_program_paid_insurance_prefix_survives_beneficiary_succession_on_both_rails() {
    for native in [false, true] {
        for former_paid in [1, BUDGETS[0], BUDGETS[0] + 1] {
            run_successor_custody_retry(native, former_paid);
        }
    }
}

fn run_successor_custody_retry(native: bool, former_paid: u64) {
    use crate::support::fuzz_model::{
        assert_market_stock_census, assert_reservation_encumbrance_census,
    };
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
    use inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;

    let mut env = if native {
        inv081_public_native_market()
    } else {
        inv018_public_spl_market_with_params(0, V16CuMarketParams::default())
    };
    let admin = env.admin.insecure_clone();
    let former = Keypair::new();
    let operator = Keypair::new();
    let successor = Keypair::new();
    for role in [&former, &operator, &successor] {
        env.svm.airdrop(&role.pubkey(), 1_000_000_000).unwrap();
    }
    for (kind, incoming) in [
        (processor::ASSET_AUTH_INSURANCE, &former),
        (processor::ASSET_AUTH_INSURANCE_OPERATOR, &operator),
    ] {
        env.try_update_per_asset_authority_with_cu(
            &admin,
            Some(incoming),
            0,
            kind,
            incoming.pubkey().to_bytes(),
        )
        .unwrap();
    }

    let former_token = create_ata_for_test(&mut env.svm, &env.payer, former.pubkey(), env.mint);
    let successor_token = ata(successor.pubkey(), env.mint);
    let admin_token = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
    assert_eq!(env.svm.get_account(&successor_token), None);

    let empty_former = env.svm.get_account(&former_token).unwrap();
    let empty_vault = env.svm.get_account(&env.vault).unwrap();
    let empty_admin = env.svm.get_account(&admin_token).unwrap();
    if native {
        send_raw_ixs(
            &mut env.svm,
            &env.payer,
            vec![
                system_instruction::transfer(&former.pubkey(), &former_token, FUNDED),
                spl_token::instruction::sync_native(&spl_token::ID, &former_token).unwrap(),
            ],
            &[&former],
        )
        .unwrap();
    } else {
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &former_token,
                &admin.pubkey(),
                &[],
                FUNDED,
            )
            .unwrap(),
            &[&admin],
        )
        .unwrap();
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::set_authority(
                &spl_token::ID,
                &env.mint,
                None,
                spl_token::instruction::AuthorityType::MintTokens,
                &admin.pubkey(),
                &[],
            )
            .unwrap(),
            &[&admin],
        )
        .unwrap();
    }
    for (domain, amount) in BUDGETS.into_iter().enumerate() {
        env.send(
            ProgInstruction::TopUpInsuranceDomain {
                domain: domain as u16,
                market_id: env.asset_market_id(0),
                authority_epoch: env.control_sequences(0).authority_epoch,
                intent_id: 0,
                amount: amount.into(),
            },
            vec![
                AccountMeta::new(former.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(former_token, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&former],
        )
        .unwrap();
    }

    let ledger = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &ledger,
        state::insurance_ledger_account_len(),
        env.program_id,
    );
    env.send(
        ProgInstruction::SyncInsuranceLedger,
        vec![
            AccountMeta::new(former.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ledger.pubkey(), false),
        ],
        &[&former],
    )
    .unwrap();
    env.resolve();
    let initial = env.market_state();
    assert_eq!(initial.1.insurance, u128::from(FUNDED));
    assert_eq!(&initial.1.insurance_domain_budget[..2], &[19, 28]);
    let initial_epoch = env.control_sequences(0).authority_epoch;
    let former_request = |env: &V16CuEnv, authority_epoch| Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(former.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(former_token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger.pubkey(), false),
        ],
        data: ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 0,
            market_id: env.asset_market_id(0),
            authority_epoch,
            amount: former_paid.into(),
        }
        .encode(),
    };
    let retained_former = former_request(&env, initial_epoch);
    if former_paid > 0 {
        let cu = insurance_succession_tx(&mut env, &[retained_former.clone()], &[], &[], None);
        assert_cu_within("unsigned insurance before succession", cu, CUSTODY_CU_LIMIT);
        assert_eq!(env.token_amount(former_token), former_paid);
        assert_eq!(
            env.market_state().1.insurance,
            u128::from(FUNDED - former_paid)
        );
        assert_eq!(env.control_sequences(0).authority_epoch, initial_epoch + 1);
    }
    let ledger_frame = env.svm.get_account(&ledger.pubkey()).unwrap();
    let ledger_state = state::read_insurance_ledger(&ledger_frame.data).unwrap();
    assert_eq!(
        ledger_state,
        state::InsuranceLedgerAccountV16 {
            market_group: env.market.to_bytes(),
            authority: former.pubkey().to_bytes(),
            total_principal_atoms: 0,
            total_deposited_atoms: 0,
            total_withdrawn_atoms: former_paid.into(),
            cumulative_profit_atoms: 0,
            cumulative_loss_atoms: 0,
            last_observed_insurance_atoms: (FUNDED - former_paid).into(),
        }
    );
    let before_handoff = env.market_state();
    env.try_update_per_asset_authority_with_cu(
        &former,
        Some(&successor),
        0,
        processor::ASSET_AUTH_INSURANCE,
        successor.pubkey().to_bytes(),
    )
    .expect("funded terminal insurance succession requires both role holders");
    assert_eq!(env.market_state(), before_handoff);
    assert_eq!(
        env.control_sequences(0).authority_epoch,
        initial_epoch + u64::from(former_paid > 0) + 1
    );
    let refreshed_former = former_request(&env, env.control_sequences(0).authority_epoch);

    let mint_frame = env.svm.get_account(&env.mint).unwrap();
    let vault_frame = env.svm.get_account(&env.vault).unwrap();
    let former_key = former.pubkey();
    let operator_key = operator.pubkey();
    let successor_key = successor.pubkey();
    assert!(
        ![former_key, operator_key, successor_key, admin.pubkey()].contains(&env.payer.pubkey())
    );
    let former_frame = env.svm.get_account(&former_key);
    let operator_frame = env.svm.get_account(&operator_key);
    let successor_frame = env.svm.get_account(&successor_key);
    drop((former, operator, successor));

    let profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    assert_eq!(profile.insurance_authority, successor_key.to_bytes());
    assert_eq!(profile.insurance_operator, operator_key.to_bytes());
    let start_epoch = env.control_sequences(0).authority_epoch;
    let expected_stock = |env: &V16CuEnv, successor_paid: u64, debits: u64| {
        let paid = former_paid + successor_paid;
        let group = env.market_state().1;
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(group.materialized_portfolio_count, 0);
        assert_eq!(group.c_tot, 0);
        assert_eq!(group.insurance, u128::from(FUNDED - paid));
        assert_eq!(group.vault, u128::from(FUNDED - paid));
        assert_eq!(
            group.insurance_domain_budget_remaining_total,
            group.insurance
        );
        assert_eq!(
            &group.insurance_domain_budget[..2],
            &[
                u128::from(BUDGETS[0].saturating_sub(paid)),
                u128::from(BUDGETS[1] - paid.saturating_sub(BUDGETS[0])),
            ]
        );
        assert!(group.insurance_domain_budget[2..].iter().all(|v| *v == 0));
        assert!(group.insurance_domain_spent.iter().all(|v| *v == 0));
        let mut expected = initial.clone();
        expected.1.insurance = u128::from(FUNDED - paid);
        expected.1.vault = u128::from(FUNDED - paid);
        expected.1.insurance_domain_budget_remaining_total = u128::from(FUNDED - paid);
        expected.1.insurance_domain_budget[0] = u128::from(BUDGETS[0].saturating_sub(paid));
        expected.1.insurance_domain_budget[1] =
            u128::from(BUDGETS[1] - paid.saturating_sub(BUDGETS[0]));
        assert_eq!(env.market_state(), expected);
        assert_eq!(
            env.control_sequences(0).authority_epoch,
            start_epoch + debits
        );
        let market = env.svm.get_account(&env.market).unwrap();
        assert_eq!(
            state::read_asset_oracle_profile(&market.data, 0).unwrap(),
            profile
        );
        assert_market_stock_census(
            "insurance succession",
            &group,
            &market.data,
            &[],
            u128::from(FUNDED - paid),
        )
        .unwrap();
        assert_reservation_encumbrance_census("insurance succession", &group, &[]).unwrap();
        assert_eq!(env.token_amount(env.vault), FUNDED - paid);
        for (key, empty, amount) in [
            (former_token, &empty_former, former_paid),
            (env.vault, &empty_vault, FUNDED - paid),
            (admin_token, &empty_admin, 0),
        ] {
            let mut image = empty.clone();
            let mut token = TokenAccount::unpack(&image.data).unwrap();
            token.amount = amount;
            TokenAccount::pack(token, &mut image.data).unwrap();
            if native {
                image.lamports += amount;
            }
            assert_eq!(env.svm.get_account(&key), Some(image));
        }
        if successor_paid == 0 {
            assert_eq!(env.svm.get_account(&successor_token), None);
        } else {
            let custody = env.svm.get_account(&successor_token).unwrap();
            let token = TokenAccount::unpack(&custody.data).unwrap();
            assert_eq!(token.mint, env.mint);
            assert_eq!(token.owner, successor_key);
            assert_eq!(token.amount, successor_paid);
            assert_eq!(token.delegate, COption::None);
            assert_eq!(token.close_authority, COption::None);
            let rent = env
                .svm
                .minimum_balance_for_rent_exemption(TokenAccount::LEN);
            assert_eq!(
                token.is_native,
                if native {
                    COption::Some(rent)
                } else {
                    COption::None
                }
            );
            assert_eq!(
                custody.lamports,
                rent + if native { successor_paid } else { 0 }
            );
        }
        assert_eq!(
            env.svm.get_account(&ledger.pubkey()),
            Some(ledger_frame.clone())
        );
        assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
    };
    expected_stock(&env, 0, 0);

    let repair = Instruction {
        program_id: associated_token_program_id(),
        accounts: vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(successor_token, false),
            AccountMeta::new_readonly(successor_key, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: vec![1],
    };
    let withdrawal = |amount: u64, attach_ledger: bool, authority_epoch| {
        let mut accounts = vec![
            AccountMeta::new_readonly(successor_key, false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(successor_token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ];
        if attach_ledger {
            accounts.push(AccountMeta::new(ledger.pubkey(), false));
        }
        Instruction {
            program_id: env.program_id,
            accounts,
            data: ProgInstruction::WithdrawInsuranceAsset {
                asset_index: 0,
                market_id: env.asset_market_id(0),
                authority_epoch,
                amount: amount.into(),
            }
            .encode(),
        }
    };

    let prefix = withdrawal(PREFIX, false, start_epoch);
    let remaining = FUNDED - former_paid;
    assert!(remaining > PREFIX && PREFIX > 0);
    let invalid_suffix = withdrawal(remaining - PREFIX, true, start_epoch + 1);
    let valid_suffix = withdrawal(remaining - PREFIX, false, start_epoch + 1);
    assert_eq!(invalid_suffix.data, valid_suffix.data);
    assert_eq!(&invalid_suffix.accounts[..6], &valid_suffix.accounts);
    assert!(prefix.accounts.iter().all(|meta| !meta.is_signer));
    let tracked = [
        env.market,
        env.vault,
        env.mint,
        former_token,
        successor_token,
        admin_token,
        ledger.pubkey(),
        successor_key,
        former_key,
        operator_key,
        admin.pubkey(),
    ];
    if former_paid > 0 {
        let stale_current = withdrawal(PREFIX, false, start_epoch - 1);
        let mut former_to_current = refreshed_former.clone();
        former_to_current.accounts[2].pubkey = successor_token;
        former_to_current.accounts.truncate(6);
        // Custody validation precedes authority/epoch checks on unchanged replay bytes.
        for ix in [retained_former, refreshed_former] {
            insurance_succession_tx(
                &mut env,
                &[ix],
                &[],
                &tracked,
                Some((2, PercolatorError::InvalidTokenAccount as u32, 0)),
            );
            expected_stock(&env, 0, 0);
        }
        insurance_succession_tx(
            &mut env,
            &[repair.clone(), former_to_current],
            &[],
            &tracked,
            Some((3, PercolatorError::Unauthorized as u32, 0)),
        );
        expected_stock(&env, 0, 0);
        insurance_succession_tx(
            &mut env,
            &[repair.clone(), stale_current],
            &[],
            &tracked,
            Some((3, PercolatorError::EngineStale as u32, 0)),
        );
        expected_stock(&env, 0, 0);
    }
    let before = tracked.map(|key| env.svm.get_account(&key));
    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
    env.svm.expire_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[
            heap_ix(),
            cu_ix(),
            repair.clone(),
            prefix.clone(),
            invalid_suffix,
        ],
        Some(&env.payer.pubkey()),
        &[&env.payer],
        env.svm.latest_blockhash(),
    );
    assert_eq!(tx.message.header.num_required_signatures, 1);
    payer.lamports -= FeeStructure::default().lamports_per_signature;
    let failure = env
        .svm
        .send_transaction(tx)
        .expect_err("former ledger rejects after beneficiary succession");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(
            4,
            InstructionError::Custom(PercolatorError::Unauthorized as u32),
        )
    );
    assert_eq!(
        failure
            .meta
            .logs
            .iter()
            .filter(|log| **log == format!("Program {} success", associated_token_program_id()))
            .count(),
        1
    );
    assert_eq!(
        failure
            .meta
            .logs
            .iter()
            .filter(|log| **log == format!("Program {} success", env.program_id))
            .count(),
        1
    );
    assert_eq!(tracked.map(|key| env.svm.get_account(&key)), before);
    assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
    assert_eq!(env.svm.get_account(&env.vault), Some(vault_frame.clone()));
    assert_eq!(
        env.svm.get_account(&ledger.pubkey()),
        Some(ledger_frame.clone())
    );
    expected_stock(&env, 0, 0);

    env.svm.expire_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), repair, prefix, valid_suffix],
        Some(&env.payer.pubkey()),
        &[&env.payer],
        env.svm.latest_blockhash(),
    );
    assert_eq!(tx.message.header.num_required_signatures, 1);
    payer.lamports -= FeeStructure::default().lamports_per_signature
        + env
            .svm
            .minimum_balance_for_rent_exemption(TokenAccount::LEN);
    let unpaid_before = env.market_state().1.insurance;
    let payout = env
        .svm
        .send_transaction(tx)
        .expect("keeper-created successor custody remains terminal-live");
    assert_eq!(
        payout
            .logs
            .iter()
            .filter(|log| **log == format!("Program {} success", associated_token_program_id()))
            .count(),
        1
    );
    assert_eq!(
        payout
            .logs
            .iter()
            .filter(|log| **log == format!("Program {} success", env.program_id))
            .count(),
        2
    );
    assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
    expected_stock(&env, remaining, 2);
    assert!(env.market_state().1.insurance < unpaid_before);
    assert_eq!(env.svm.get_account(&former_token), before[3].clone());
    assert_eq!(env.svm.get_account(&admin_token), before[5].clone());
    assert_eq!(
        env.svm.get_account(&ledger.pubkey()),
        Some(ledger_frame.clone())
    );
    assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
    assert_eq!(env.svm.get_account(&successor_key), successor_frame);
    assert_eq!(env.svm.get_account(&former_key), former_frame);
    assert_eq!(env.svm.get_account(&operator_key), operator_frame);

    for cu in [
        failure.meta.compute_units_consumed,
        payout.compute_units_consumed,
    ] {
        assert_cu_within(
            "INV-073 successor custody stale-ledger retry",
            cu,
            CUSTODY_CU_LIMIT,
        );
    }
    let market_rent = env.svm.get_account(&env.market).unwrap().lamports;
    let tombstone_rent = env
        .svm
        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
    let mut admin_after_close = env.svm.get_account(&admin.pubkey()).unwrap();
    admin_after_close.lamports += market_rent + empty_vault.lamports - tombstone_rent;
    let custody_before_close =
        [former_token, successor_token, admin_token].map(|key| env.svm.get_account(&key));
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
            authority_epoch: env.control_sequences(0).authority_epoch,
        }
        .encode(),
    };
    let invalid_suffix =
        system_instruction::transfer(&env.payer.pubkey(), &admin.pubkey(), u64::MAX);
    let close_rollback_cu = insurance_succession_tx(
        &mut env,
        &[close.clone(), invalid_suffix],
        &[&admin],
        &tracked,
        Some((3, 1, 1)),
    );
    expected_stock(&env, remaining, 2);
    let close_cu = insurance_succession_tx(&mut env, &[close], &[&admin], &tracked, None);
    let tombstone = env.svm.get_account(&env.market).unwrap();
    assert_closed_market_tombstone(&tombstone);
    assert_eq!(tombstone.lamports, tombstone_rent);
    assert!(env
        .svm
        .get_account(&env.vault)
        .is_none_or(|account| account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)));
    assert_eq!(
        env.svm.get_account(&admin.pubkey()),
        Some(admin_after_close)
    );
    assert_eq!(
        [former_token, successor_token, admin_token].map(|key| env.svm.get_account(&key)),
        custody_before_close
    );
    assert_eq!(env.svm.get_account(&ledger.pubkey()), Some(ledger_frame));
    assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame));
    assert_eq!(env.svm.get_account(&former_key), former_frame);
    assert_eq!(env.svm.get_account(&operator_key), operator_frame);
    assert_eq!(env.svm.get_account(&successor_key), successor_frame);
    eprintln!(
        "INV-073 successor custody retry: native={native}, former_paid={former_paid}, successor_paid={remaining}, rollback={} CU, payout={} CU, close_rollback={close_rollback_cu} CU, close={close_cu} CU",
        failure.meta.compute_units_consumed, payout.compute_units_consumed
    );
}
