//! Row 421 / INV-073: terminal insurance payout remains live when beneficiary
//! succession, missing successor custody, and a stale former-beneficiary ledger
//! collide. A keeper-created successor ATA plus an unsigned payout must roll
//! back exactly if a later suffix carries the old ledger, then the same public
//! repair/payment shape must finish once the stale optional ledger is omitted.

use super::*;
use solana_sdk::{
    fee::FeeStructure,
    instruction::{Instruction, InstructionError},
    transaction::{Transaction, TransactionError},
};

const BUDGETS: [u64; 2] = [19, 28];
const FUNDED: u64 = BUDGETS[0] + BUDGETS[1];
const PREFIX: u64 = 17;

fn ata(wallet: Pubkey, mint: Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[wallet.as_ref(), spl_token::ID.as_ref(), mint.as_ref()],
        &associated_token_program_id(),
    )
    .0
}

#[test]
fn v16_program_successor_custody_repair_retries_after_stale_former_insurance_ledger() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

    let mut env = inv018_public_spl_market_with_params(0, V16CuMarketParams::default());
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
    let ledger_frame = env.svm.get_account(&ledger.pubkey()).unwrap();
    let ledger_state = state::read_insurance_ledger(&ledger_frame.data).unwrap();
    assert_eq!(ledger_state.authority, former.pubkey().to_bytes());
    assert_eq!(ledger_state.last_observed_insurance_atoms, FUNDED.into());
    assert_eq!(ledger_state.total_withdrawn_atoms, 0);

    env.resolve();
    env.try_update_per_asset_authority_with_cu(
        &former,
        Some(&successor),
        0,
        processor::ASSET_AUTH_INSURANCE,
        successor.pubkey().to_bytes(),
    )
    .expect("funded terminal insurance succession requires both role holders");

    let mint_frame = env.svm.get_account(&env.mint).unwrap();
    let vault_frame = env.svm.get_account(&env.vault).unwrap();
    let former_key = former.pubkey();
    let operator_key = operator.pubkey();
    let successor_key = successor.pubkey();
    let former_frame = env.svm.get_account(&former_key);
    let operator_frame = env.svm.get_account(&operator_key);
    let successor_frame = env.svm.get_account(&successor_key);
    drop((former, operator, successor));

    let expected_stock = |env: &V16CuEnv, paid: u64| {
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
        assert_eq!(env.token_amount(env.vault), FUNDED - paid);
        if paid == 0 {
            assert_eq!(env.svm.get_account(&successor_token), None);
        } else {
            let custody = env.svm.get_account(&successor_token).unwrap();
            let token = TokenAccount::unpack(&custody.data).unwrap();
            assert_eq!(token.mint, env.mint);
            assert_eq!(token.owner, successor_key);
            assert_eq!(token.amount, paid);
        }
    };
    expected_stock(&env, 0);

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

    let start_epoch = env.control_sequences(0).authority_epoch;
    let prefix = withdrawal(PREFIX, false, start_epoch);
    let invalid_suffix = withdrawal(FUNDED - PREFIX, true, start_epoch + 1);
    let valid_suffix = withdrawal(FUNDED - PREFIX, false, start_epoch + 1);
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
    ];
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
    expected_stock(&env, 0);

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
    expected_stock(&env, FUNDED);
    assert_eq!(env.svm.get_account(&former_token), before[3].clone());
    assert_eq!(env.svm.get_account(&admin_token), before[5].clone());
    assert_eq!(env.svm.get_account(&ledger.pubkey()), Some(ledger_frame));
    assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame));
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
    eprintln!(
        "INV-073 successor custody retry: rollback={} CU, payout={} CU",
        failure.meta.compute_units_consumed, payout.compute_units_consumed
    );
}
