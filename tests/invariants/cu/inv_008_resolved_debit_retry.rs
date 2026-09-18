//! Row 428 / INV-008/010/024/064/080/081: retained terminal debit consent
//! survives aborted payouts and is consumed across signed/permissionless delivery.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};
use std::collections::BTreeMap;

#[test]
fn v16_resolved_retained_debit_restores_atomic_thaw_before_consuming_epoch() {
    use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_freeze_authority;

    const FUNDED: u64 = 60;
    const DEBIT: u64 = 29;
    const LIMIT: u64 = 200_000;
    let freezer = Keypair::new();
    let params = V16CuMarketParams::default();
    let mut env = inv018_public_spl_market_with_freeze_authority(
        0,
        params,
        params.max_portfolio_assets as usize,
        Some(freezer.pubkey()),
    );
    let admin = env.admin.insecure_clone();
    assert_ne!(freezer.pubkey(), admin.pubkey());
    let wallet = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
    send_raw_ixs(
        &mut env.svm,
        &env.payer,
        vec![
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &wallet,
                &admin.pubkey(),
                &[],
                FUNDED,
            )
            .unwrap(),
            spl_token::instruction::set_authority(
                &spl_token::ID,
                &env.mint,
                None,
                spl_token::instruction::AuthorityType::MintTokens,
                &admin.pubkey(),
                &[],
            )
            .unwrap(),
        ],
        &[&admin],
    )
    .unwrap();
    let controls = env.control_sequences(0);
    env.send(
        ProgInstruction::TopUpInsuranceDomain {
            domain: 0,
            market_id: env.asset_market_id(0),
            authority_epoch: controls.authority_epoch,
            intent_id: next_control_sequence(controls.insurance_top_up),
            amount: FUNDED.into(),
        },
        vec![
            AccountMeta::new_readonly(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(wallet, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    )
    .unwrap();
    let ledger = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &ledger,
        state::insurance_ledger_account_len(),
        env.program_id,
    );
    env.resolve();
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::freeze_account(
            &spl_token::ID,
            &wallet,
            &env.mint,
            &freezer.pubkey(),
            &[],
        )
        .unwrap(),
        &[&freezer],
    )
    .unwrap();
    let controls = env.control_sequences(0);
    let initial_market = env.market_state();
    assert_eq!(initial_market.1.mode, MarketModeV16::Resolved);
    let withdrawal = |authority_epoch, amount: u64| Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(admin.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(wallet, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger.pubkey(), false),
        ],
        data: ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 0,
            market_id: env.asset_market_id(0),
            authority_epoch,
            amount: amount.into(),
        }
        .encode(),
    };
    let retained = withdrawal(controls.authority_epoch, DEBIT);
    let remainder = withdrawal(controls.authority_epoch + 1, FUNDED - DEBIT);
    let thaw = spl_token::instruction::thaw_account(
        &spl_token::ID,
        &wallet,
        &env.mint,
        &freezer.pubkey(),
        &[],
    )
    .unwrap();
    let stale = PercolatorError::EngineStale as u32;
    let requests = [
        (
            vec![retained.clone()],
            Some((2, PercolatorError::InvalidTokenAccount as u32)),
            [0, 0],
            0,
        ),
        (
            vec![thaw.clone(), retained.clone(), retained.clone()],
            Some((4, stale)),
            [1, 2],
            0,
        ),
        (vec![thaw, retained.clone()], None, [1, 2], DEBIT),
        (vec![retained], Some((2, stale)), [0, 0], DEBIT),
        (vec![remainder], None, [1, 1], FUNDED),
    ];
    // Retain every wire envelope before delivery; only the mint freezer signs repairs.
    let requests: Vec<_> = requests
        .into_iter()
        .enumerate()
        .map(|(step, (ixs, error, completed, paid))| {
            let repairs = ixs[0].program_id == spl_token::ID;
            let mut all = vec![
                heap_ix(),
                ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32 - step as u32),
            ];
            all.extend(ixs);
            let mut signers = vec![&env.payer];
            if repairs {
                signers.push(&freezer);
            }
            let tx = Transaction::new_signed_with_payer(
                &all,
                Some(&env.payer.pubkey()),
                &signers,
                env.svm.latest_blockhash(),
            );
            assert_eq!(
                tx.message.header.num_required_signatures,
                if repairs { 2 } else { 1 }
            );
            assert!(
                !tx.message.account_keys[..tx.message.header.num_required_signatures as usize]
                    .contains(&admin.pubkey())
            );
            (bincode::serialize(&tx).unwrap(), error, completed, paid)
        })
        .collect();
    drop(admin);
    let keys = [
        env.market,
        env.mint,
        env.vault,
        wallet,
        ledger.pubkey(),
        env.admin.pubkey(),
        env.vault_authority,
        freezer.pubkey(),
    ];
    let baseline: BTreeMap<_, _> = keys
        .iter()
        .map(|key| (*key, env.svm.get_account(key)))
        .collect();
    let mint = Mint::unpack(&baseline[&env.mint].as_ref().unwrap().data).unwrap();
    assert_eq!((mint.supply, mint.mint_authority), (FUNDED, COption::None));
    assert_eq!(mint.freeze_authority, COption::Some(freezer.pubkey()));
    assert_eq!(
        TokenAccount::unpack(&baseline[&wallet].as_ref().unwrap().data)
            .unwrap()
            .state,
        AccountState::Frozen
    );
    let mut debits = 0;
    let mut rollbacks = 0;
    let mut peak = 0;
    for (wire, error, completed, paid) in requests {
        let tx: Transaction = bincode::deserialize(&wire).unwrap();
        tx.verify().unwrap();
        assert_eq!(bincode::serialize(&tx).unwrap(), wire);
        assert!(wire.len() <= solana_sdk::packet::PACKET_DATA_SIZE);
        let before: BTreeMap<_, _> = keys
            .iter()
            .chain(&tx.message.account_keys)
            .map(|key| (*key, env.svm.get_account(key)))
            .collect();
        let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
        payer.lamports -= FeeStructure::default().lamports_per_signature
            * u64::from(tx.message.header.num_required_signatures);
        let result = env.svm.send_transaction(tx);
        let meta = if let Some((index, code)) = error {
            let failure = result.expect_err("frozen custody or consumed epoch must reject");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(index, InstructionError::Custom(code))
            );
            for (key, account) in &before {
                if *key != env.payer.pubkey() {
                    assert_eq!(
                        env.svm.get_account(key),
                        *account,
                        "complete rollback {key}"
                    );
                }
            }
            rollbacks += 1;
            failure.meta
        } else {
            debits += 1;
            result.expect("retained repair and successor payment must commit")
        };
        for (program, count) in [env.program_id, spl_token::ID].into_iter().zip(completed) {
            assert_eq!(
                meta.logs
                    .iter()
                    .filter(|line| **line == format!("Program {program} success"))
                    .count(),
                count
            );
        }
        assert_eq!(env.svm.get_account(&env.payer.pubkey()), Some(payer));
        assert!(meta.compute_units_consumed > 0);
        assert_cu_within("row428 atomic thaw", meta.compute_units_consumed, LIMIT);
        peak = peak.max(meta.compute_units_consumed);
        let remaining = FUNDED - paid;
        if paid == DEBIT {
            assert!(remaining >= DEBIT, "stale debit still fully funded");
        }
        let mut expected = initial_market.clone();
        expected.1.insurance = remaining.into();
        expected.1.vault = remaining.into();
        expected.1.insurance_domain_budget[0] = remaining.into();
        expected.1.insurance_domain_budget_remaining_total = remaining.into();
        assert_eq!(env.market_state(), expected);
        let mut expected_controls = controls;
        expected_controls.authority_epoch += debits;
        assert_eq!(env.control_sequences(0), expected_controls);
        for (key, amount) in [(env.vault, remaining), (wallet, paid)] {
            let mut expected = baseline[&key].clone().unwrap();
            let mut token = TokenAccount::unpack(&expected.data).unwrap();
            token.amount = amount;
            if key == wallet && paid > 0 {
                token.state = AccountState::Initialized;
            }
            TokenAccount::pack(token, &mut expected.data).unwrap();
            assert_eq!(env.svm.get_account(&key), Some(expected));
        }
        let mut expected_ledger = baseline[&ledger.pubkey()].clone().unwrap();
        if paid > 0 {
            state::init_insurance_ledger(
                &mut expected_ledger.data,
                &state::InsuranceLedgerAccountV16 {
                    market_group: env.market.to_bytes(),
                    authority: env.admin.pubkey().to_bytes(),
                    total_principal_atoms: 0,
                    total_deposited_atoms: 0,
                    total_withdrawn_atoms: paid.into(),
                    cumulative_profit_atoms: 0,
                    cumulative_loss_atoms: 0,
                    last_observed_insurance_atoms: remaining.into(),
                },
            )
            .unwrap();
        }
        assert_eq!(env.svm.get_account(&ledger.pubkey()), Some(expected_ledger));
        for (key, account) in &before {
            if ![
                env.payer.pubkey(),
                env.market,
                env.vault,
                wallet,
                ledger.pubkey(),
            ]
            .contains(key)
            {
                assert_eq!(env.svm.get_account(key), *account, "success frame {key}");
            }
        }
        let market = env.svm.get_account(&env.market).unwrap();
        assert_market_stock_census(
            "row428 atomic thaw",
            &expected.1,
            &market.data,
            &[],
            remaining.into(),
        )
        .unwrap();
        assert_reservation_encumbrance_census("row428 atomic thaw", &expected.1, &[]).unwrap();
    }
    assert_eq!((debits, rollbacks), (2, 3));
    eprintln!("row428 atomic thaw: 1 history, 5 transactions, 3 exact rollbacks, 1 restored thaw and payout, 2 payouts, peak {peak} CU");
}

#[test]
fn v16_resolved_retained_debit_retries_restore_epochs_across_signed_and_unsigned_delivery() {
    const INITIAL: [u128; 4] = [19, 41, 23, 47];
    const SUPPLY: u64 = 130;
    const DEBIT: u128 = 29;
    const NEXT: u128 = 7;
    let mut transactions = 0;
    let mut rollbacks = 0;
    let mut restored_transfers = 0;
    let mut payouts = 0;
    let mut peak_cu = 0;
    let mut max_packet = 0;
    for asset in 0..2 {
        for signed_first in [false, true] {
            for observed_first in [false, true] {
                let context =
                    format!("asset={asset}, signed={signed_first}, ledger={observed_first}");
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        ..V16CuMarketParams::default()
                    },
                );
                let admin = env.admin.insecure_clone();
                let wallet =
                    create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
                let empty =
                    create_ata_for_test(&mut env.svm, &env.payer, env.payer.pubkey(), env.mint);
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &wallet,
                        &admin.pubkey(),
                        &[],
                        SUPPLY,
                    )
                    .unwrap(),
                    &[&admin],
                )
                .unwrap();
                for (domain, amount) in INITIAL.into_iter().enumerate() {
                    let controls = env.control_sequences(domain / 2);
                    env.send(
                        ProgInstruction::TopUpInsuranceDomain {
                            domain: domain as u16,
                            market_id: env.asset_market_id(domain as u16 / 2),
                            authority_epoch: controls.authority_epoch,
                            intent_id: next_control_sequence(controls.insurance_top_up),
                            amount,
                        },
                        vec![
                            AccountMeta::new(admin.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(wallet, false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&admin],
                    )
                    .unwrap();
                }
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
                let ledger = Keypair::new();
                system_create_account_for_test(
                    &mut env.svm,
                    &env.payer,
                    &ledger,
                    state::insurance_ledger_account_len(),
                    env.program_id,
                );
                env.resolve();
                let controls = [env.control_sequences(0), env.control_sequences(1)];
                let ids = [env.asset_market_id(0), env.asset_market_id(1)];
                let profiles = [0, 1].map(|index| {
                    state::read_asset_oracle_profile(
                        &env.svm.get_account(&env.market).unwrap().data,
                        index,
                    )
                    .unwrap()
                });
                assert_ne!(admin.pubkey(), env.payer.pubkey());
                let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
                assert_eq!(mint.supply, SUPPLY);
                assert_eq!(mint.mint_authority, COption::None);
                let withdrawal = |target: usize, epoch, amount, signed, observed| {
                    let mut accounts = vec![
                        AccountMeta::new_readonly(admin.pubkey(), signed),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(wallet, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ];
                    if observed {
                        accounts.push(AccountMeta::new(ledger.pubkey(), false));
                    }
                    Instruction {
                        program_id: env.program_id,
                        accounts,
                        data: ProgInstruction::WithdrawInsuranceAsset {
                            asset_index: target as u16,
                            market_id: ids[target],
                            authority_epoch: epoch,
                            amount,
                        }
                        .encode(),
                    }
                };
                let epoch = controls[asset].authority_epoch;
                let first = withdrawal(asset, epoch, DEBIT, signed_first, observed_first);
                let alternate = withdrawal(asset, epoch, DEBIT, !signed_first, !observed_first);
                let fresh = withdrawal(asset, epoch + 1, NEXT, signed_first, true);
                let successor = withdrawal(asset, epoch + 2, NEXT, signed_first, true);
                let fresh_alternate = withdrawal(asset, epoch + 1, NEXT, !signed_first, false);
                let remainder = withdrawal(
                    asset,
                    epoch + 3,
                    INITIAL[2 * asset] + INITIAL[2 * asset + 1] - DEBIT - 2 * NEXT,
                    false,
                    true,
                );
                let peer = withdrawal(
                    1 - asset,
                    controls[1 - asset].authority_epoch,
                    INITIAL[2 * (1 - asset)] + INITIAL[2 * (1 - asset) + 1],
                    false,
                    false,
                );
                // The payer owns this empty source, so the failure never adds an
                // insurance-authority signature to a permissionless request.
                let late_error = spl_token::instruction::transfer(
                    &spl_token::ID,
                    &empty,
                    &wallet,
                    &env.payer.pubkey(),
                    &[],
                    1,
                )
                .unwrap();
                let stale = PercolatorError::EngineStale as u32;
                let insufficient = spl_token::error::TokenError::InsufficientFunds as u32;
                let requests = [
                    (
                        vec![first.clone(), late_error.clone()],
                        Some((3, insufficient)),
                        1,
                    ),
                    (
                        vec![alternate.clone(), late_error.clone()],
                        Some((3, insufficient)),
                        1,
                    ),
                    (vec![first.clone(), first.clone()], Some((3, stale)), 1),
                    (vec![alternate.clone()], None, 1),
                    (vec![first.clone()], Some((2, stale)), 0),
                    (vec![alternate], Some((2, stale)), 0),
                    (
                        vec![fresh.clone(), successor.clone(), late_error],
                        Some((4, insufficient)),
                        2,
                    ),
                    (vec![fresh.clone(), first], Some((3, stale)), 1),
                    (vec![fresh, successor], None, 2),
                    (vec![fresh_alternate], Some((2, stale)), 0),
                    (vec![remainder], None, 1),
                    (vec![peer], None, 1),
                ];
                // Retain every signed wire envelope before the first attempt,
                // including predicted successor epochs and the final full drains.
                let requests: Vec<_> = requests
                    .into_iter()
                    .enumerate()
                    .map(|(step, (ixs, error, prefix))| {
                        let signed = ixs.iter().any(|ix| {
                            ix.accounts
                                .iter()
                                .any(|meta| meta.pubkey == admin.pubkey() && meta.is_signer)
                        });
                        let mut all = vec![
                            heap_ix(),
                            ComputeBudgetInstruction::set_compute_unit_limit(300_000 - step as u32),
                        ];
                        all.extend(ixs);
                        let mut signers = vec![&env.payer];
                        if signed {
                            signers.push(&admin);
                        }
                        let tx = Transaction::new_signed_with_payer(
                            &all,
                            Some(&env.payer.pubkey()),
                            &signers,
                            env.svm.latest_blockhash(),
                        );
                        assert_eq!(
                            tx.message.header.num_required_signatures,
                            if signed { 2 } else { 1 }
                        );
                        (bincode::serialize(&tx).unwrap(), error, prefix)
                    })
                    .collect();
                let keys = [
                    env.market,
                    env.mint,
                    env.vault,
                    wallet,
                    empty,
                    ledger.pubkey(),
                    admin.pubkey(),
                    env.vault_authority,
                ];
                let baseline: BTreeMap<_, _> = keys
                    .iter()
                    .map(|key| (*key, env.svm.get_account(key)))
                    .collect();
                let token_keys = [env.vault, wallet, empty];
                let mut budgets = INITIAL;
                let mut expected_controls = controls;
                let mut expected_ledger: Option<state::InsuranceLedgerAccountV16> = None;
                let mut paid = 0;
                for (step, (wire, error, prefix)) in requests.into_iter().enumerate() {
                    let tx: Transaction = bincode::deserialize(&wire).unwrap();
                    tx.verify().unwrap();
                    assert_eq!(bincode::serialize(&tx).unwrap(), wire);
                    max_packet = max_packet.max(wire.len());
                    assert!(wire.len() <= solana_sdk::packet::PACKET_DATA_SIZE);
                    let before: BTreeMap<_, _> = keys
                        .iter()
                        .chain(&tx.message.account_keys)
                        .map(|key| (*key, env.svm.get_account(key)))
                        .collect();
                    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
                    payer.lamports -= FeeStructure::default().lamports_per_signature
                        * u64::from(tx.message.header.num_required_signatures);
                    let result = env.svm.send_transaction(tx);
                    let meta = if let Some((index, code)) = error {
                        let failure = result.expect_err("retained or late-error request must fail");
                        assert_eq!(
                            failure.err,
                            TransactionError::InstructionError(
                                index,
                                InstructionError::Custom(code)
                            ),
                            "{context}, step={step}"
                        );
                        for (key, account) in &before {
                            if *key != env.payer.pubkey() {
                                assert_eq!(
                                    env.svm.get_account(key),
                                    *account,
                                    "{context}, step={step}, rollback={key}"
                                );
                            }
                        }
                        rollbacks += 1;
                        restored_transfers += prefix;
                        failure.meta
                    } else {
                        let meta = result
                            .expect("retained continuation remains executable after rollback");
                        let debits = match step {
                            3 => vec![(asset, DEBIT, !observed_first)],
                            8 => vec![(asset, NEXT, true), (asset, NEXT, true)],
                            10 => vec![(
                                asset,
                                INITIAL[2 * asset] + INITIAL[2 * asset + 1] - DEBIT - 2 * NEXT,
                                true,
                            )],
                            11 => vec![(
                                1 - asset,
                                INITIAL[2 * (1 - asset)] + INITIAL[2 * (1 - asset) + 1],
                                false,
                            )],
                            _ => unreachable!(),
                        };
                        for (target, amount, observed) in debits {
                            let available = budgets[2 * target] + budgets[2 * target + 1];
                            if observed {
                                let record = expected_ledger.get_or_insert(
                                    state::InsuranceLedgerAccountV16 {
                                        market_group: env.market.to_bytes(),
                                        authority: admin.pubkey().to_bytes(),
                                        total_principal_atoms: 0,
                                        total_deposited_atoms: 0,
                                        total_withdrawn_atoms: 0,
                                        cumulative_profit_atoms: 0,
                                        cumulative_loss_atoms: 0,
                                        last_observed_insurance_atoms: available,
                                    },
                                );
                                assert_eq!(record.last_observed_insurance_atoms, available);
                                record.total_withdrawn_atoms += amount;
                                record.last_observed_insurance_atoms -= amount;
                            }
                            let long = amount.min(budgets[2 * target]);
                            budgets[2 * target] -= long;
                            budgets[2 * target + 1] -= amount - long;
                            expected_controls[target].authority_epoch += 1;
                            paid += amount;
                            payouts += 1;
                        }
                        // Every compiled account outside the documented writes is
                        // unchanged even on success, including non-payer lamports.
                        for (key, account) in &before {
                            if ![
                                env.payer.pubkey(),
                                env.market,
                                env.vault,
                                wallet,
                                ledger.pubkey(),
                            ]
                            .contains(key)
                            {
                                assert_eq!(
                                    env.svm.get_account(key),
                                    *account,
                                    "success frame {key}"
                                );
                            }
                        }
                        meta
                    };
                    for program in [env.program_id, spl_token::ID] {
                        assert_eq!(
                            meta.logs
                                .iter()
                                .filter(|line| **line == format!("Program {program} success"))
                                .count(),
                            prefix,
                            "{context}, step={step}"
                        );
                    }
                    assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
                    assert_cu_within(&context, meta.compute_units_consumed, 300_000);
                    peak_cu = peak_cu.max(meta.compute_units_consumed);
                    transactions += 1;
                    let remaining = budgets.iter().sum::<u128>();
                    let group = env.market_state().1;
                    assert_eq!(group.mode, MarketModeV16::Resolved);
                    assert_eq!(group.insurance_domain_budget, budgets);
                    assert_eq!(group.insurance_domain_spent, [0; 4]);
                    assert_eq!(group.insurance_domain_budget_remaining_total, remaining);
                    assert_eq!(
                        (group.insurance, group.vault, group.c_tot),
                        (remaining, remaining, 0)
                    );
                    assert_eq!(
                        (
                            group.materialized_portfolio_count,
                            group.pnl_pos_tot,
                            group.source_claim_bound_total_num,
                            group.source_insurance_credit_reserved_total_atoms
                        ),
                        (0, 0, 0, 0)
                    );
                    assert_eq!(
                        [env.control_sequences(0), env.control_sequences(1)],
                        expected_controls
                    );
                    assert_eq!([env.asset_market_id(0), env.asset_market_id(1)], ids);
                    assert_eq!(remaining + paid, u128::from(SUPPLY));
                    for (key, amount) in token_keys.into_iter().zip([remaining, paid, 0]) {
                        let mut expected = baseline[&key].clone().unwrap();
                        let mut token = TokenAccount::unpack(&expected.data).unwrap();
                        token.amount = amount as u64;
                        TokenAccount::pack(token, &mut expected.data).unwrap();
                        assert_eq!(env.svm.get_account(&key).unwrap(), expected);
                    }
                    let mut ledger_frame = baseline[&ledger.pubkey()].clone().unwrap();
                    if let Some(record) = &expected_ledger {
                        state::init_insurance_ledger(&mut ledger_frame.data, record).unwrap();
                    }
                    assert_eq!(env.svm.get_account(&ledger.pubkey()).unwrap(), ledger_frame);
                    for key in [env.mint, admin.pubkey(), env.vault_authority] {
                        assert_eq!(env.svm.get_account(&key), baseline[&key]);
                    }
                    let market = env.svm.get_account(&env.market).unwrap();
                    for (index, profile) in profiles.iter().enumerate() {
                        assert_eq!(
                            state::read_asset_oracle_profile(&market.data, index).unwrap(),
                            *profile
                        );
                    }
                    assert_market_stock_census(&context, &group, &market.data, &[], remaining)
                        .unwrap();
                    assert_reservation_encumbrance_census(&context, &group, &[]).unwrap();
                    let mut frame = market.clone();
                    frame.data = baseline[&env.market].as_ref().unwrap().data.clone();
                    assert_eq!(frame, *baseline[&env.market].as_ref().unwrap());
                    let mut data = market.data;
                    state::market_view_mut(&mut data)
                        .unwrap()
                        .1
                        .validate_shape()
                        .unwrap();
                }
                assert_eq!(budgets, [0; 4]);
                assert_eq!(paid, u128::from(SUPPLY));
            }
        }
    }
    assert_eq!(
        (transactions, rollbacks, restored_transfers, payouts),
        (96, 64, 48, 40)
    );
    eprintln!("row428 terminal retries: 8 histories, {transactions} transactions, {rollbacks} exact rollbacks, {restored_transfers} restored SPL transfers, {payouts} payouts, peak {peak_cu} CU, max packet {max_packet} bytes");
}
