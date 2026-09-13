//! Row 421 / INV-073: terminal-economic-progress-does-not-require-an-adversarial-operator-signature.
//! Related INV-017/018/021/027/064/067/069/070/071/078/082: native insurance
//! reaches its beneficiary with neither insurance role signing, including when
//! the optional ledger is first created by a keeper after both wallets are drained.
//!
//! Public route: System/ATA/native SyncNative -> insurance role assignment ->
//! TopUpInsuranceDomain -> optional SyncInsuranceLedger -> voluntary wallet draining ->
//! ResolveMarket -> keeper ledger creation, donations and SyncNative -> two
//! unsigned WithdrawInsuranceAsset calls -> administrator-signed CloseSlab.
//! The existing native fixture supplies only the missing native-mint genesis
//! account; all program-owned accounts are constructed through public instructions.
//!
//! Four bounded histories cross preinitialized versus lazy ledgers with
//! native synchronization before versus between payments. Input-derived budgets,
//! complete ledger records, SPL images and lamports distinguish insurance payout
//! from donated native stock. Each payment strictly reduces the remaining claim;
//! ledger rent and paid custody survive actual market/vault closure unchanged.
//! This adds unsigned native ledger conformance beyond row421 frozen/recredited
//! SPL custody and row420/433 provider/reserve repair. It does not prove arbitrary
//! histories, active user liabilities, insurance consumption/recredit, other
//! assets/quote rails, authority succession, maximum shapes or absent-admin
//! retirement. Recipient redemption and separate ledger disposal remain unproven.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::fee::FeeStructure;

const BUDGETS: [u64; 2] = [37, 61];
const FUNDED: u64 = BUDGETS[0] + BUDGETS[1];
const FIRST: u64 = 41;
const VAULT_DONATION: u64 = 17;
const RECIPIENT_DONATION: u64 = 19;
const CU_LIMIT: u64 = 150_000;

fn execute(
    env: &mut V16CuEnv,
    instructions: &[Instruction],
    signers: &[&Keypair],
    tracked: &[Pubkey],
    changed: &[Pubkey],
    payer_outflow: u64,
) -> u64 {
    env.svm.expire_blockhash();
    let mut batch = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT as u32),
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
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    assert_eq!(
        usize::from(tx.message.header.num_required_signatures),
        signing.len()
    );
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let fee = signing.len() as u64 * FeeStructure::default().lamports_per_signature;
    let meta = env
        .svm
        .send_transaction(tx)
        .expect("public native continuation");
    for (key, mut expected) in keys.iter().zip(before) {
        if *key == env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee + payer_outflow;
        } else if changed.contains(key) {
            continue;
        }
        assert_eq!(
            env.svm.get_account(key),
            expected,
            "complete Account frame {key}"
        );
    }
    assert_cu_within(
        "INV-073 native ledger progress",
        meta.compute_units_consumed,
        CU_LIMIT,
    );
    meta.compute_units_consumed
}

fn native_image(empty: &Account, amount: u64, unsynced: u64) -> Account {
    let mut expected = empty.clone();
    let mut token = TokenAccount::unpack(&empty.data).unwrap();
    assert_eq!(token.amount, 0);
    assert_eq!(token.is_native, COption::Some(empty.lamports));
    assert_eq!(token.state, AccountState::Initialized);
    assert_eq!(
        (token.delegate, token.close_authority),
        (COption::None, COption::None)
    );
    token.amount = amount;
    TokenAccount::pack(token, &mut expected.data).unwrap();
    expected.lamports += amount + unsynced;
    expected
}

#[test]
fn v16_program_unsigned_native_insurance_ledger_excludes_donations_through_close() {
    use inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;

    let mut worlds = 0;
    let mut payments = 0;
    let mut peak = [0u64; 4];
    for preinitialized_ledger in [false, true] {
        for sync_before_first in [false, true] {
            let mut env = inv081_public_native_market();
            let admin = env.admin.insecure_clone();
            let beneficiary = Keypair::new();
            let operator = Keypair::new();
            let wallets = [beneficiary.pubkey(), operator.pubkey()];
            for (role, holder) in [
                (processor::ASSET_AUTH_INSURANCE, &beneficiary),
                (processor::ASSET_AUTH_INSURANCE_OPERATOR, &operator),
            ] {
                env.svm.airdrop(&holder.pubkey(), 1_000_000_000).unwrap();
                env.try_update_per_asset_authority_with_cu(
                    &admin,
                    Some(holder),
                    0,
                    role,
                    holder.pubkey().to_bytes(),
                )
                .unwrap();
            }
            let recipient = create_ata_for_test(&mut env.svm, &env.payer, wallets[0], env.mint);
            let admin_token =
                create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
            let empty_recipient = env.svm.get_account(&recipient).unwrap();
            let empty_vault = env.svm.get_account(&env.vault).unwrap();
            let empty_admin_token = env.svm.get_account(&admin_token).unwrap();
            let ledger_seed = "row421-native-ledger";
            let ledger =
                Pubkey::create_with_seed(&env.payer.pubkey(), ledger_seed, &env.program_id)
                    .unwrap();
            let ledger_rent = env
                .svm
                .minimum_balance_for_rent_exemption(state::insurance_ledger_account_len());
            let create_ledger = system_instruction::create_account_with_seed(
                &env.payer.pubkey(),
                &ledger,
                &env.payer.pubkey(),
                ledger_seed,
                ledger_rent,
                state::insurance_ledger_account_len() as u64,
                &env.program_id,
            );
            if preinitialized_ledger {
                execute(
                    &mut env,
                    &[create_ledger.clone()],
                    &[],
                    &[],
                    &[ledger],
                    ledger_rent,
                );
            }
            let funding = [
                system_instruction::transfer(&wallets[0], &recipient, FUNDED),
                spl_token::instruction::sync_native(&spl_token::ID, &recipient).unwrap(),
            ];
            send_raw_ixs(&mut env.svm, &env.payer, funding.to_vec(), &[&beneficiary]).unwrap();
            for (domain, amount) in BUDGETS.into_iter().enumerate() {
                let accounts = vec![
                    AccountMeta::new(wallets[0], true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(recipient, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ];
                env.send(
                    ProgInstruction::TopUpInsuranceDomain {
                        domain: domain as u16,
                        market_id: env.asset_market_id(0),
                        authority_epoch: env.control_sequences(0).authority_epoch,
                        intent_id: env.control_sequences(0).insurance_top_up + 1,
                        amount: amount.into(),
                    },
                    accounts,
                    &[&beneficiary],
                )
                .unwrap();
            }
            if preinitialized_ledger {
                env.send(
                    ProgInstruction::SyncInsuranceLedger,
                    vec![
                        AccountMeta::new(wallets[0], true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(ledger, false),
                    ],
                    &[&beneficiary],
                )
                .unwrap();
            }
            assert_eq!(
                env.svm.get_account(&recipient),
                Some(empty_recipient.clone())
            );
            for holder in [&beneficiary, &operator] {
                let balance = env.svm.get_account(&holder.pubkey()).unwrap().lamports;
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    system_instruction::transfer(&holder.pubkey(), &admin.pubkey(), balance),
                    &[holder],
                )
                .unwrap();
                let drained = env.svm.get_account(&holder.pubkey()).unwrap();
                assert_eq!(drained.lamports, 0);
                assert!(drained.data.is_empty());
                assert_eq!(drained.owner, solana_sdk::system_program::ID);
            }
            let drained_wallets = wallets.map(|key| env.svm.get_account(&key));
            drop((beneficiary, operator));
            env.resolve();
            let resolved_market = env.svm.get_account(&env.market).unwrap();
            let cfg = env.market_state().0;
            let sequences = env.control_sequences(0);
            let profile = state::read_asset_oracle_profile(&resolved_market.data, 0).unwrap();
            assert_eq!(profile.insurance_authority, wallets[0].to_bytes());
            assert_eq!(profile.insurance_operator, wallets[1].to_bytes());
            assert_ne!(wallets[0], wallets[1]);
            let tracked = [
                env.market,
                env.vault,
                env.mint,
                env.vault_authority,
                recipient,
                admin_token,
                admin.pubkey(),
                wallets[0],
                wallets[1],
                ledger,
            ];
            let admin_before = env.svm.get_account(&admin.pubkey()).unwrap();
            if !preinitialized_ledger {
                assert!(env.svm.get_account(&ledger).is_none());
                peak[0] = peak[0].max(execute(
                    &mut env,
                    &[create_ledger],
                    &[],
                    &tracked,
                    &[ledger],
                    ledger_rent,
                ));
                let fresh = env.svm.get_account(&ledger).unwrap();
                assert_eq!(fresh.owner, env.program_id);
                assert_eq!(fresh.lamports, ledger_rent);
                assert!(fresh.data.iter().all(|byte| *byte == 0));
            }
            let ledger_frame = env.svm.get_account(&ledger).unwrap();
            let donation = [
                system_instruction::transfer(&env.payer.pubkey(), &env.vault, VAULT_DONATION),
                system_instruction::transfer(&env.payer.pubkey(), &recipient, RECIPIENT_DONATION),
            ];
            let custody = [env.vault, recipient];
            peak[1] = peak[1].max(execute(
                &mut env,
                &donation,
                &[],
                &tracked,
                &custody,
                VAULT_DONATION + RECIPIENT_DONATION,
            ));
            let sync = [
                spl_token::instruction::sync_native(&spl_token::ID, &env.vault).unwrap(),
                spl_token::instruction::sync_native(&spl_token::ID, &recipient).unwrap(),
            ];
            let check = |env: &V16CuEnv, paid: u64, synced: bool| {
                let market = env.svm.get_account(&env.market).unwrap();
                let (current_cfg, group) = state::read_market(&market.data).unwrap();
                let remaining = FUNDED - paid;
                assert_eq!(current_cfg, cfg);
                assert_eq!(market.lamports, resolved_market.lamports);
                assert_eq!(group.mode, MarketModeV16::Resolved);
                assert_eq!(
                    (
                        group.c_tot,
                        group.materialized_portfolio_count,
                        group.pnl_pos_tot
                    ),
                    (0, 0, 0)
                );
                assert_eq!(
                    (
                        group.insurance,
                        group.vault,
                        group.insurance_domain_budget_remaining_total
                    ),
                    (remaining.into(), remaining.into(), remaining.into())
                );
                assert_eq!(
                    &group.insurance_domain_budget[..2],
                    &[
                        u128::from(BUDGETS[0].saturating_sub(paid)),
                        u128::from(BUDGETS[1] - paid.saturating_sub(BUDGETS[0])),
                    ]
                );
                assert!(group.insurance_domain_budget[2..]
                    .iter()
                    .all(|value| *value == 0));
                assert!(group.insurance_domain_spent.iter().all(|value| *value == 0));
                assert_eq!(env.control_sequences(0), sequences);
                assert_eq!(
                    state::read_asset_oracle_profile(&market.data, 0).unwrap(),
                    profile
                );
                for (key, empty, amount, donated) in [
                    (env.vault, &empty_vault, remaining, VAULT_DONATION),
                    (recipient, &empty_recipient, paid, RECIPIENT_DONATION),
                ] {
                    let (tokens, raw) = if synced {
                        (amount + donated, 0)
                    } else {
                        (amount, donated)
                    };
                    assert_eq!(
                        env.svm.get_account(&key),
                        Some(native_image(empty, tokens, raw))
                    );
                }
                assert_eq!(
                    env.svm.get_account(&admin_token),
                    Some(empty_admin_token.clone())
                );
                assert_eq!(
                    env.svm.get_account(&admin.pubkey()),
                    Some(admin_before.clone())
                );
                assert_eq!(
                    wallets.map(|key| env.svm.get_account(&key)),
                    drained_wallets
                );
                let record_account = env.svm.get_account(&ledger).unwrap();
                if !preinitialized_ledger && paid == 0 {
                    assert_eq!(record_account, ledger_frame);
                } else {
                    let expected = state::InsuranceLedgerAccountV16 {
                        market_group: env.market.to_bytes(),
                        authority: wallets[0].to_bytes(),
                        total_principal_atoms: 0,
                        total_deposited_atoms: 0,
                        total_withdrawn_atoms: paid.into(),
                        cumulative_profit_atoms: 0,
                        cumulative_loss_atoms: 0,
                        last_observed_insurance_atoms: remaining.into(),
                    };
                    assert_eq!(
                        state::read_insurance_ledger(&record_account.data).unwrap(),
                        expected
                    );
                    assert_eq!(
                        (
                            record_account.owner,
                            record_account.lamports,
                            record_account.executable,
                            record_account.rent_epoch
                        ),
                        (
                            ledger_frame.owner,
                            ledger_frame.lamports,
                            ledger_frame.executable,
                            ledger_frame.rent_epoch
                        )
                    );
                }
                let synchronized_donation = if synced { VAULT_DONATION } else { 0 };
                assert_market_stock_census(
                    "unsigned native insurance ledger",
                    &group,
                    &market.data,
                    &[],
                    (env.token_amount(env.vault) - synchronized_donation).into(),
                )
                .unwrap();
                assert_reservation_encumbrance_census(
                    "unsigned native insurance ledger",
                    &group,
                    &[],
                )
                .unwrap();
                let custody_lamports = custody
                    .iter()
                    .map(|key| env.svm.get_account(key).unwrap().lamports)
                    .sum::<u64>();
                assert_eq!(
                    custody_lamports - empty_vault.lamports - empty_recipient.lamports,
                    FUNDED + VAULT_DONATION + RECIPIENT_DONATION
                );
                let mut data = market.data.clone();
                state::market_view_mut(&mut data)
                    .unwrap()
                    .1
                    .validate_shape()
                    .unwrap();
            };
            let mut paid = 0;
            let mut synced = false;
            check(&env, paid, synced);
            for (index, amount) in [FIRST, FUNDED - FIRST].into_iter().enumerate() {
                if index == usize::from(!sync_before_first) {
                    peak[1] = peak[1].max(execute(&mut env, &sync, &[], &tracked, &custody, 0));
                    synced = true;
                    check(&env, paid, synced);
                }
                let ix = Instruction {
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
                        authority_epoch: sequences.authority_epoch,
                        amount: amount.into(),
                    }
                    .encode(),
                };
                assert!(ix.accounts.iter().all(|meta| !meta.is_signer));
                assert!(!ix.accounts.iter().any(|meta| meta.pubkey == wallets[1]));
                let before = env.market_state().1.insurance;
                let changes = [env.market, env.vault, recipient, ledger];
                peak[2] = peak[2].max(execute(&mut env, &[ix], &[], &tracked, &changes, 0));
                paid += amount;
                payments += 1;
                assert_eq!(before - env.market_state().1.insurance, amount.into());
                check(&env, paid, synced);
            }
            assert_eq!(paid, FUNDED);
            assert!(synced);
            let paid_ledger = env.svm.get_account(&ledger);
            let paid_custody = env.svm.get_account(&recipient);
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
                    authority_epoch: sequences.authority_epoch,
                }
                .encode(),
            };
            let changes = [admin.pubkey(), env.market, env.vault, admin_token];
            peak[3] = peak[3].max(execute(
                &mut env,
                &[close],
                &[&admin],
                &tracked,
                &changes,
                0,
            ));
            let tombstone = env.svm.get_account(&env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            let tombstone_rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            assert_eq!(tombstone.lamports, tombstone_rent);
            assert!(env.svm.get_account(&env.vault).is_none_or(
                |account| account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)
            ));
            let mut expected_admin = admin_before;
            expected_admin.lamports +=
                resolved_market.lamports + empty_vault.lamports - tombstone_rent;
            assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
            assert_eq!(
                env.svm.get_account(&admin_token),
                Some(native_image(&empty_admin_token, VAULT_DONATION, 0))
            );
            assert_eq!(env.svm.get_account(&ledger), paid_ledger);
            assert_eq!(env.svm.get_account(&recipient), paid_custody);
            assert_eq!(
                wallets.map(|key| env.svm.get_account(&key)),
                drained_wallets
            );
            worlds += 1;
        }
    }
    assert_eq!((worlds, payments), (4, 8));
    println!("INV-073 native insurance ledger: worlds={worlds}, keeper_payments={payments}, paid_per_world={FUNDED}, peak_CU(ledger,donation_sync,payment,close)={peak:?}");
}
