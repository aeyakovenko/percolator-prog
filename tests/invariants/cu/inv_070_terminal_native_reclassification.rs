//! Row 424 / INV-070: external denomination changes after a persisted terminal scan.
//! INV-024/025/041/069/071/086/088 require current custody classification to
//! determine disposal without recreating an earlier asset's paid insurance budget.
//! Unlike the post-prefix SPL donation test, SyncNative adds no custody lamports;
//! unlike the native surplus/insurance controls, the market has a persisted prefix
//! and the identical signed final close has already simulated successfully.
//! This is finite native-quote conformance, not earlier-slot expiry or engine-scan
//! rediscovery coverage. Row 424 remains OPEN; INV-063 is not newly exercised.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};

const INSURANCE: u64 = 37;
const BACKING: u64 = 23;
const RAW: u64 = 19;
const LIMIT: u64 = 300_000;
const SLOTS: usize = percolator::TERMINAL_SLAB_SCAN_ASSETS_PER_CALL + 1;
const BACKING_DOMAIN: usize = 2 * (SLOTS - 1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SyncAt {
    Never,
    BeforeScan,
    AfterPreview,
}

fn transaction(env: &V16CuEnv, ixs: &[Instruction], signers: &[&Keypair]) -> Transaction {
    let mut instructions = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32),
    ];
    instructions.extend_from_slice(ixs);
    let mut signing = vec![&env.payer];
    signing.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&env.payer.pubkey()),
        &signing,
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    tx
}

fn native_account(initial: &Account, amount: u64, unsynced: u64) -> Account {
    let mut expected = initial.clone();
    let mut token = TokenAccount::unpack(&expected.data).unwrap();
    assert_eq!(token.amount, 0);
    assert_eq!(token.is_native, COption::Some(initial.lamports));
    token.amount = amount;
    TokenAccount::pack(token, &mut expected.data).unwrap();
    expected.lamports += amount + unsynced;
    expected
}

#[test]
fn v16_program_native_sync_after_terminal_prefix_reclassifies_only_external_surplus() {
    use inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market_with_capacity;

    let mut outcomes = Vec::new();
    for sync_at in [SyncAt::Never, SyncAt::BeforeScan, SyncAt::AfterPreview] {
        // The existing helper supplies only the omitted native-mint genesis account.
        // All market, authority, stock and token changes below use public instructions.
        let mut env = inv081_public_native_market_with_capacity(SLOTS);
        let admin = env.admin.insecure_clone();
        let beneficiary = Keypair::new();
        env.svm
            .airdrop(&beneficiary.pubkey(), 1_000_000_000)
            .unwrap();
        for asset in 1..SLOTS {
            env.activate_asset(asset as u16, asset as u64 + 1, 100);
        }
        env.try_update_per_asset_authority_with_cu(
            &admin,
            Some(&beneficiary),
            0,
            processor::ASSET_AUTH_INSURANCE,
            beneficiary.pubkey().to_bytes(),
        )
        .unwrap();
        let recipient =
            create_ata_for_test(&mut env.svm, &env.payer, beneficiary.pubkey(), env.mint);
        let destination = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
        let empty_vault = env.svm.get_account(&env.vault).unwrap();
        let empty_recipient = env.svm.get_account(&recipient).unwrap();
        let empty_destination = env.svm.get_account(&destination).unwrap();
        send_raw_ixs(
            &mut env.svm,
            &env.payer,
            vec![
                system_instruction::transfer(&beneficiary.pubkey(), &recipient, INSURANCE),
                spl_token::instruction::sync_native(&spl_token::ID, &recipient).unwrap(),
                system_instruction::transfer(&admin.pubkey(), &destination, BACKING),
                spl_token::instruction::sync_native(&spl_token::ID, &destination).unwrap(),
                system_instruction::transfer(&admin.pubkey(), &env.vault, RAW),
            ],
            &[&beneficiary, &admin],
        )
        .unwrap();
        env.send(
            ProgInstruction::TopUpInsuranceDomain {
                domain: 0,
                market_id: env.asset_market_id(0),
                authority_epoch: env.control_sequences(0).authority_epoch,
                intent_id: env.control_sequences(0).insurance_top_up + 1,
                amount: INSURANCE.into(),
            },
            vec![
                AccountMeta::new(beneficiary.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(recipient, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&beneficiary],
        )
        .unwrap();
        env.svm.warp_to_slot(300);
        env.top_up_backing_bucket_from_admin_token_with_cu(
            destination,
            BACKING_DOMAIN as u16,
            BACKING.into(),
            400,
        );
        env.resolve();
        let beneficiary_key = beneficiary.pubkey();
        drop(beneficiary);
        let market_key = env.market;
        let vault_key = env.vault;
        let tracked = [
            market_key,
            vault_key,
            env.mint,
            env.vault_authority,
            destination,
            recipient,
            admin.pubkey(),
            beneficiary_key,
        ];
        let terminal_lamports = tracked
            .iter()
            .filter_map(|key| env.svm.get_account(key))
            .map(|account| account.lamports)
            .sum::<u64>();
        let admin_before = env.svm.get_account(&admin.pubkey()).unwrap();
        let market_before = env.svm.get_account(&market_key).unwrap();
        let mint_before = env.svm.get_account(&env.mint);
        let beneficiary_before = env.svm.get_account(&beneficiary_key);
        let close = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(market_key, false),
                AccountMeta::new(vault_key, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(destination, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: ProgInstruction::CloseSlab {
                authority_epoch: env.control_sequences(0).authority_epoch,
            }
            .encode(),
        };
        let withdrawal = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new_readonly(beneficiary_key, false),
                AccountMeta::new(market_key, false),
                AccountMeta::new(recipient, false),
                AccountMeta::new(vault_key, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: env
                .withdraw_insurance_asset_instruction(beneficiary_key, 0, INSURANCE.into())
                .encode(),
        };
        let sync = spl_token::instruction::sync_native(&spl_token::ID, &vault_key).unwrap();
        let backing_withdrawal = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(market_key, false),
                AccountMeta::new(destination, false),
                AccountMeta::new(vault_key, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: ProgInstruction::WithdrawBackingBucket {
                domain: BACKING_DOMAIN as u16,
                market_id: env.asset_market_id((SLOTS - 1) as u16),
                authority_epoch: env.control_sequences(SLOTS - 1).authority_epoch,
                amount: BACKING.into(),
            }
            .encode(),
        };

        let commit = |env: &mut V16CuEnv, tx: Transaction, changed: &[Pubkey], spl_calls: usize| {
            let mut keys = tx.message.account_keys.clone();
            keys.extend(tracked);
            keys.sort_unstable();
            keys.dedup();
            let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
            let fee = u64::from(tx.message.header.num_required_signatures)
                * solana_sdk::fee::FeeStructure::default().lamports_per_signature;
            let meta = env
                .svm
                .send_transaction(tx)
                .expect("native terminal continuation");
            for (key, mut expected) in keys.iter().zip(before) {
                if *key == env.payer.pubkey() {
                    expected.as_mut().unwrap().lamports -= fee;
                }
                if !changed.contains(key) {
                    assert_eq!(
                        env.svm.get_account(key),
                        expected,
                        "complete Account frame: {key}"
                    );
                }
            }
            assert_eq!(
                meta.logs
                    .iter()
                    .filter(|line| **line == format!("Program {} success", spl_token::ID))
                    .count(),
                spl_calls
            );
            assert_cu_within(
                "native terminal classification",
                meta.compute_units_consumed,
                LIMIT,
            );
            meta.compute_units_consumed
        };
        let check =
            |env: &V16CuEnv, cursor: usize, paid: bool, backing_paid: bool, synced: bool| {
                let market = env.svm.get_account(&market_key).unwrap();
                let (cfg, group) = state::read_market(&market.data).unwrap();
                let remaining = if paid { 0 } else { INSURANCE };
                let backing = if backing_paid { 0 } else { BACKING };
                assert_eq!(cfg.terminal_slab_scan_progress, cursor as u128);
                assert_eq!(group.mode, MarketModeV16::Resolved);
                assert_eq!(
                    (
                        group.c_tot,
                        group.pnl_pos_tot,
                        group.materialized_portfolio_count
                    ),
                    (0, 0, 0)
                );
                assert_eq!(
                    (group.vault, group.insurance),
                    ((remaining + backing).into(), remaining.into())
                );
                assert_eq!(group.insurance_domain_budget[0], remaining.into());
                assert!(group.insurance_domain_budget[1..]
                    .iter()
                    .all(|amount| *amount == 0));
                assert!(group
                    .insurance_domain_spent
                    .iter()
                    .all(|amount| *amount == 0));
                assert_eq!(group.backing_provider_earnings_total, 0);
                for (domain, bucket) in group.source_backing_buckets.iter().enumerate() {
                    let fresh = if domain == BACKING_DOMAIN {
                        u128::from(backing) * BOUND_SCALE
                    } else {
                        0
                    };
                    assert_eq!(bucket.fresh_unliened_backing_num, fresh);
                    assert_eq!(
                        group.source_credit[domain].fresh_reserved_backing_num,
                        fresh
                    );
                    assert_eq!(bucket.valid_liened_backing_num, 0);
                }
                let (wrapped, raw) = if synced { (RAW, 0) } else { (0, RAW) };
                assert_eq!(
                    env.svm.get_account(&vault_key),
                    Some(native_account(
                        &empty_vault,
                        remaining + backing + wrapped,
                        raw
                    ))
                );
                assert_eq!(
                    env.svm.get_account(&recipient),
                    Some(native_account(&empty_recipient, INSURANCE - remaining, 0))
                );
                assert_eq!(
                    env.svm.get_account(&destination),
                    Some(native_account(&empty_destination, BACKING - backing, 0))
                );
                assert_market_stock_census(
                    "native scan denomination",
                    &group,
                    &market.data,
                    &[],
                    (env.token_amount(vault_key) - wrapped).into(),
                )
                .unwrap();
                assert_reservation_encumbrance_census("native scan denomination", &group, &[])
                    .unwrap();
            };

        let mut peak = 0;
        check(&env, 0, false, false, false);
        if sync_at == SyncAt::BeforeScan {
            env.svm.expire_blockhash();
            let tx = transaction(&env, &[sync.clone()], &[]);
            peak = peak.max(commit(&mut env, tx, &[vault_key], 1));
        }
        env.svm.expire_blockhash();
        let tx = transaction(&env, &[close.clone()], &[&admin]);
        peak = peak.max(commit(&mut env, tx, &[market_key], 0));
        let prefix = SLOTS - 1;
        check(&env, prefix, false, false, sync_at == SyncAt::BeforeScan);
        assert!(
            prefix > 0,
            "insurance belongs to asset zero behind the prefix"
        );

        // The unpaid local budget remains exactly 37 even if raw lamports were synchronized.
        // An absent beneficiary receives only that budget; the external 19 remain surplus.
        env.svm.expire_blockhash();
        let tx = transaction(&env, &[withdrawal], &[]);
        assert_eq!(tx.message.header.num_required_signatures, 1);
        peak = peak.max(commit(&mut env, tx, &[market_key, vault_key, recipient], 1));
        check(&env, prefix, true, false, sync_at == SyncAt::BeforeScan);

        // Unexpired backing establishes the scan boundary, then leaves by its public
        // principal route. Nothing is expired or left for native-token burning.
        env.svm.expire_blockhash();
        let tx = transaction(&env, &[backing_withdrawal], &[&admin]);
        peak = peak.max(commit(
            &mut env,
            tx,
            &[market_key, vault_key, destination],
            1,
        ));
        check(&env, prefix, true, true, sync_at == SyncAt::BeforeScan);

        env.svm.expire_blockhash();
        let retained = transaction(&env, &[close], &[&admin]);
        let retained_bytes = bincode::serialize(&retained).unwrap();
        let preview_keys: Vec<_> = retained
            .message
            .account_keys
            .iter()
            .chain(tracked.iter())
            .copied()
            .collect();
        let before_preview: Vec<_> = preview_keys
            .iter()
            .map(|key| env.svm.get_account(key))
            .collect();
        let preview = env
            .svm
            .simulate_transaction(retained.clone().into())
            .unwrap();
        assert_eq!(
            preview_keys
                .iter()
                .map(|key| env.svm.get_account(key))
                .collect::<Vec<_>>(),
            before_preview
        );
        assert_eq!(
            preview
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", spl_token::ID))
                .count(),
            if sync_at == SyncAt::BeforeScan { 2 } else { 1 },
            "already-ready preview transfers only currently wrapped surplus"
        );
        peak = peak.max(preview.compute_units_consumed);
        let scanned = env.svm.get_account(&market_key).unwrap();
        let vault_lamports = env.svm.get_account(&vault_key).unwrap().lamports;
        if sync_at == SyncAt::AfterPreview {
            // No market/admin/beneficiary account or signature participates in SyncNative.
            let tx = transaction(&env, &[sync], &[]);
            assert_eq!(tx.message.header.num_required_signatures, 1);
            assert!(!tx.message.account_keys.contains(&market_key));
            peak = peak.max(commit(&mut env, tx, &[vault_key], 1));
        }
        env.svm.warp_to_slot(301);
        assert_eq!(env.svm.get_account(&market_key), Some(scanned));
        assert_eq!(
            env.svm.get_account(&vault_key).unwrap().lamports,
            vault_lamports
        );
        let synced = sync_at != SyncAt::Never;
        check(&env, prefix, true, true, synced);
        assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);
        peak = peak.max(commit(
            &mut env,
            retained,
            &[market_key, vault_key, destination, admin.pubkey()],
            if synced { 2 } else { 1 },
        ));

        let tombstone = env.svm.get_account(&market_key).unwrap();
        assert_closed_market_tombstone(&tombstone);
        assert_eq!(
            tombstone.lamports,
            env.svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN)
        );
        assert!(env.svm.get_account(&vault_key).is_none_or(
            |account| account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)
        ));
        let sweep = if synced { RAW } else { 0 };
        let mut expected_admin = admin_before.clone();
        expected_admin.lamports +=
            market_before.lamports + empty_vault.lamports + RAW - sweep - tombstone.lamports;
        assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
        assert_eq!(
            env.svm.get_account(&destination),
            Some(native_account(&empty_destination, BACKING + sweep, 0))
        );
        assert_eq!(
            env.svm.get_account(&recipient),
            Some(native_account(&empty_recipient, INSURANCE, 0))
        );
        assert_eq!(env.svm.get_account(&env.mint), mint_before);
        assert_eq!(env.svm.get_account(&beneficiary_key), beneficiary_before);
        assert_eq!(
            tracked
                .iter()
                .filter_map(|key| env.svm.get_account(key))
                .map(|account| account.lamports)
                .sum::<u64>(),
            terminal_lamports
        );
        outcomes.push((
            env.svm.get_account(&admin.pubkey()).unwrap().lamports - admin_before.lamports
                + env.token_amount(destination),
            env.token_amount(recipient),
            tombstone.lamports,
        ));
        assert_cu_within("native classification history peak", peak, LIMIT);
        println!("row424 native {sync_at:?}: cursor=0->{prefix}->tombstone, insurance={INSURANCE}, backing={BACKING}, wrapped surplus={sweep}, peak={peak} CU");
    }
    assert!(
        outcomes.windows(2).all(|pair| pair[0] == pair[1]),
        "classification timing preserves each beneficiary's total lamports"
    );
}
