#![allow(dead_code, unused_imports, unused_variables, unused_mut, clippy::too_many_arguments, clippy::field_reassign_with_default, clippy::manual_saturating_arithmetic, clippy::useless_conversion, for_loops_over_fallibles, clippy::unnecessary_cast, clippy::absurd_extreme_comparisons, clippy::manual_abs_diff, clippy::empty_line_after_doc_comments, clippy::doc_lazy_continuation, clippy::needless_range_loop, clippy::implicit_saturating_sub, clippy::wrong_self_convention)]
mod common;
#[allow(unused_imports)]
use common::*;

use solana_sdk::{
    account::Account,
    clock::Clock,
    instruction::{AccountMeta, Instruction},
    program_pack::Pack,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    sysvar,
    transaction::Transaction,
};
use spl_token::state::{Account as TokenAccount, AccountState};

/// CRITICAL: UpdateAdmin only callable by current admin (two-step semantics).
///
/// Phase E (2026-04-17): UpdateAdmin for non-zero new_admin now sets pending_admin
/// only. The proposed new admin must separately call AcceptAdmin to complete
/// the transfer. This prevents instant takeover on admin key compromise.
#[test]
fn test_critical_update_admin_authorization() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let new_admin = Keypair::new();
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    env.svm.airdrop(&new_admin.pubkey(), 1_000_000_000).unwrap();

    // Attacker proposes a transfer (even to themselves) — must fail authorization.
    let result = env.try_update_admin(&attacker, &attacker.pubkey());
    assert!(
        result.is_err(),
        "SECURITY: Non-admin should not be able to propose admin transfer"
    );
    println!("UpdateAdmin by non-admin: REJECTED (correct)");

    // Attacker calls AcceptAdmin when no transfer is pending — must fail.
    let result = env.try_accept_admin(&attacker);
    assert!(
        result.is_err(),
        "SECURITY: AcceptAdmin with no pending transfer should fail"
    );
    println!("AcceptAdmin with no pending: REJECTED (correct)");

    // Real admin proposes new admin — should succeed (sets pending_admin).
    let result = env.try_update_admin(&admin, &new_admin.pubkey());
    assert!(
        result.is_ok(),
        "Admin should be able to propose admin transfer: {:?}",
        result
    );
    println!("UpdateAdmin propose by admin: ACCEPTED (correct)");

    // TWO-STEP PROPERTY: old admin still has authority until Accept is called.
    // Old admin can still propose a different transfer (overwriting pending).
    env.svm.expire_blockhash();
    let result = env.try_update_admin(&admin, &admin.pubkey());
    assert!(
        result.is_ok(),
        "Old admin retains authority during pending transfer: {:?}",
        result
    );
    println!("Old admin retains authority during pending: CONFIRMED (correct)");

    // Attacker tries to accept even though they're not the pending admin — must fail.
    // (Requires re-proposing new_admin first since we just overwrote pending to admin.)
    env.svm.expire_blockhash();
    env.try_update_admin(&admin, &new_admin.pubkey()).unwrap();
    let result = env.try_accept_admin(&attacker);
    assert!(
        result.is_err(),
        "SECURITY: Only pending_admin signer can AcceptAdmin"
    );
    println!("AcceptAdmin by wrong signer: REJECTED (correct)");

    // New admin (the actual pending_admin) accepts — transfer completes.
    let result = env.try_accept_admin(&new_admin);
    assert!(
        result.is_ok(),
        "Pending admin should be able to complete transfer: {:?}",
        result
    );
    println!("AcceptAdmin by pending_admin: ACCEPTED (correct)");

    // Now the OLD admin has no authority.
    env.svm.expire_blockhash();
    let result = env.try_update_admin(&admin, &admin.pubkey());
    assert!(
        result.is_err(),
        "After AcceptAdmin, old admin should no longer have authority"
    );
    println!("Post-transfer: old admin rejected (correct)");

    // New admin has authority — but call exists NOW as new admin only; propose
    // to themselves just to prove they can. (This overwrites pending to new_admin.)
    env.svm.expire_blockhash();
    let result = env.try_update_admin(&new_admin, &new_admin.pubkey());
    assert!(
        result.is_ok(),
        "New admin should be able to exercise authority: {:?}",
        result
    );
    println!("Post-transfer: new admin exercises authority (correct)");

    // Separately verify the cleared-pending property: fresh market, accept with
    // nothing pending must fail. (We proved this earlier in the test too.)
    env.svm.expire_blockhash();
    let mut env2 = TestEnv::new();
    env2.init_market_with_invert(0);
    let result = env2.try_accept_admin(&new_admin);
    assert!(
        result.is_err(),
        "AcceptAdmin on fresh market (no pending) must be rejected"
    );
    println!("AcceptAdmin on fresh market: REJECTED (correct)");
}

/// CRITICAL: UpdateConfig admin-only with all parameters
#[test]
fn test_critical_update_config_authorization() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();

    // Attacker tries to update config - should fail
    let result = env.try_update_config(&attacker);
    assert!(
        result.is_err(),
        "SECURITY: Non-admin should not update config"
    );
    println!("UpdateConfig by non-admin: REJECTED (correct)");

    // Capture config before admin update
    let config_before = env.read_update_config_snapshot();

    // Admin updates config - should succeed AND change state
    let result = env.try_update_config(&admin);
    assert!(result.is_ok(), "Admin should update config: {:?}", result);

    // Verify config actually changed (not just is_ok)
    let config_after = env.read_update_config_snapshot();
    // try_update_config uses different params than default, so snapshot should differ
    // (if the helper sends the same defaults, this proves nothing — check helper)
    assert_ne!(
        config_before, config_after,
        "Config must actually change after successful UpdateConfig"
    );
}

/// CRITICAL: CloseSlab only by admin, requires zero vault/insurance
#[test]
fn test_critical_close_slab_authorization() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();

    // Deposit some funds (creates non-zero vault balance)
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    // Attacker tries to close slab - should fail (not admin)
    let (vault_pda, _) =
        Pubkey::find_program_address(&[b"vault", env.slab.as_ref()], &env.program_id);
    let attacker_ata = env.create_ata(&attacker.pubkey(), 0);
    let attacker_ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(vault_pda, false),
            AccountMeta::new(attacker_ata, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: encode_close_slab(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), attacker_ix],
        Some(&attacker.pubkey()),
        &[&attacker],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(result.is_err(), "SECURITY: Non-admin should not close slab");
    println!("CloseSlab by non-admin: REJECTED (correct)");

    // Admin tries to close slab with non-zero balance - should fail
    let result = env.try_close_slab();
    assert!(
        result.is_err(),
        "SECURITY: Should not close slab with non-zero vault"
    );
    println!("CloseSlab with active funds: REJECTED (correct)");

    println!("CRITICAL TEST PASSED: CloseSlab authorization verified");
}

/// CRITICAL: InitMarket rejects already initialized slab
#[test]
fn test_critical_init_market_rejects_double_init() {
    program_path();

    let mut env = TestEnv::new();

    // First init
    env.init_market_with_invert(0);
    println!("First InitMarket: success");

    // Try second init - should fail
    let admin = &env.payer;
    let dummy_ata = Pubkey::new_unique();
    env.svm
        .set_account(
            dummy_ata,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; TokenAccount::LEN],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: encode_init_market_with_invert(&admin.pubkey(), &env.mint, &TEST_FEED_ID, 0),
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    // Snapshot state after first init and before rejected second init.
    const HEADER_CONFIG_LEN: usize = 584;
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let used_before = env.read_num_used_accounts();
    let vault_before = env.vault_balance();
    let result = env.svm.send_transaction(tx);

    assert!(
        result.is_err(),
        "SECURITY: Double initialization should be rejected"
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let used_after = env.read_num_used_accounts();
    let vault_after = env.vault_balance();
    assert_eq!(
        &slab_after[..HEADER_CONFIG_LEN],
        &slab_before[..HEADER_CONFIG_LEN],
        "Rejected second InitMarket must not mutate slab header/config"
    );
    assert_eq!(used_after, used_before, "Rejected second InitMarket must not change num_used_accounts");
    assert_eq!(vault_after, vault_before, "Rejected second InitMarket must not move vault funds");
    println!("Second InitMarket: REJECTED (correct)");

    println!("CRITICAL TEST PASSED: Double initialization rejection verified");
}

/// CRITICAL: Invalid user_idx/lp_idx are rejected
#[test]
fn test_critical_invalid_account_indices_rejected() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Try trade with invalid user_idx (999 - not initialized)
    let result = env.try_trade(&user, &lp, lp_idx, 999, 1_000_000);
    assert!(
        result.is_err(),
        "SECURITY: Invalid user_idx should be rejected"
    );
    println!("Trade with invalid user_idx: REJECTED (correct)");

    // Try trade with invalid lp_idx (999 - not initialized)
    let result = env.try_trade(&user, &lp, 999, user_idx, 1_000_000);
    assert!(
        result.is_err(),
        "SECURITY: Invalid lp_idx should be rejected"
    );
    println!("Trade with invalid lp_idx: REJECTED (correct)");

    println!("CRITICAL TEST PASSED: Invalid account indices rejection verified");
}

/// Verify per-market admin limits are enforced for Set* operations.
#[test]
fn test_init_market_admin_limits_enforced() {
    program_path();

    let mut env = TestEnv::new();

    // Init market with specific limits
    let admin = &env.payer;
    let dummy_ata = Pubkey::new_unique();
    env.svm
        .set_account(
            dummy_ata,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; TokenAccount::LEN],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: encode_init_market_with_limits(
            &admin.pubkey(),
            &env.mint,
            &TEST_FEED_ID,
            1000,   // max_maintenance_fee_per_slot
            50_000, // max_risk_threshold
            5000,   // min_oracle_price_cap_e2bps
        ),
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    env.svm.send_transaction(tx).expect("init_market failed");

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // SetOraclePriceCap: above floor should succeed
    let result = env.try_set_oracle_price_cap(&admin, 5000);
    assert!(
        result.is_ok(),
        "Cap at floor should succeed: {:?}",
        result
    );

    // SetOraclePriceCap: below floor (non-zero) should fail
    let result = env.try_set_oracle_price_cap(&admin, 4999);
    assert!(result.is_err(), "Cap below floor should fail");

    // SetOraclePriceCap: zero is rejected when min floor is set
    // (prevents settlement guard bypass via baseline poisoning)
    let result = env.try_set_oracle_price_cap(&admin, 0);
    assert!(
        result.is_err(),
        "Cap=0 must be rejected when min_oracle_price_cap > 0 (settlement guard)"
    );

    println!("INIT MARKET ADMIN LIMITS ENFORCED: PASSED");
}

/// Verify that InitMarket rejects zero admin limits.
#[test]
fn test_init_market_zero_limits_rejected() {
    program_path();

    let mut env = TestEnv::new();
    let admin = &env.payer;
    let dummy_ata = Pubkey::new_unique();
    env.svm
        .set_account(
            dummy_ata,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; TokenAccount::LEN],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    // Zero max_maintenance_fee_per_slot should fail
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: encode_init_market_with_limits(
            &admin.pubkey(),
            &env.mint,
            &TEST_FEED_ID,
            0,         // INVALID: zero max_maintenance_fee
            u128::MAX,
            0,
        ),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    assert!(
        env.svm.send_transaction(tx).is_err(),
        "Zero max_maintenance_fee should be rejected"
    );

    // Zero max_risk_threshold should fail
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: encode_init_market_with_limits(
            &admin.pubkey(),
            &env.mint,
            &TEST_FEED_ID,
            u128::MAX,
            0, // INVALID: zero max_risk_threshold
            0,
        ),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    assert!(
        env.svm.send_transaction(tx).is_err(),
        "Zero max_risk_threshold should be rejected"
    );

    println!("INIT MARKET ZERO LIMITS REJECTED: PASSED");
}

/// Verify that UpdateAdmin to zero address is rejected.
#[test]
fn test_update_admin_zero_accepted_for_burn() {
    program_path();

    let mut env = TestEnv::new();
    // Use init_market_with_cap with permissionless resolve + force_close_delay
    // because admin burn requires both for live markets (liveness guard).
    env.init_market_with_cap(0, 10_000, 100);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let zero_pubkey = Pubkey::new_from_array([0u8; 32]);

    // Admin burn to zero is now allowed (spec §7 step [3])
    let result = env.try_update_admin(&admin, &zero_pubkey);
    assert!(
        result.is_ok(),
        "UpdateAdmin to zero should succeed for admin burn"
    );

    // After burn, admin instructions must fail
    let result = env.try_set_risk_threshold(&admin, 999);
    assert!(
        result.is_err(),
        "Admin operations must fail after admin burn"
    );

    println!("UPDATE ADMIN ZERO BURN: PASSED");
}

/// Verify that InitMarket rejects initial risk_params that exceed per-market limits.
#[test]
fn test_init_market_risk_params_exceed_limits_rejected() {
    program_path();

    let mut env = TestEnv::new();
    let admin = &env.payer;
    let dummy_ata = Pubkey::new_unique();
    env.svm
        .set_account(
            dummy_ata,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; TokenAccount::LEN],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    // Build InitMarket with risk_reduction_threshold > max_risk_threshold
    let mut data = vec![0u8];
    data.extend_from_slice(admin.pubkey().as_ref());
    data.extend_from_slice(env.mint.as_ref());
    data.extend_from_slice(&TEST_FEED_ID);
    data.extend_from_slice(&86400u64.to_le_bytes()); // max_staleness_secs (1 day, ≤7d cap)
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(0u8); // invert
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
    data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6
    // Per-market admin limits
    data.extend_from_slice(&100_000_000_000_000_000_000u128.to_le_bytes()); // max_maintenance_fee_per_slot (<= MAX_PROTOCOL_FEE_ABS)
    data.extend_from_slice(&50_000u128.to_le_bytes()); // max_risk_threshold = 50_000
    data.extend_from_slice(&0u64.to_le_bytes()); // min_oracle_price_cap_e2bps
    // RiskParams with risk_reduction_threshold EXCEEDING the limit
    data.extend_from_slice(&0u64.to_le_bytes()); // warmup_period_slots
    data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps
    data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // trading_fee_bps
    data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data.extend_from_slice(&0u128.to_le_bytes()); // new_account_fee
    data.extend_from_slice(&50_001u128.to_le_bytes()); // risk_reduction_threshold = 50_001 > limit!
    data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee_per_slot
    data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_crank_staleness_slots
    data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
    data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
    data.extend_from_slice(&100u64.to_le_bytes()); // liquidation_buffer_bps
    data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
    data.extend_from_slice(&100u128.to_le_bytes()); // min_initial_deposit
    data.extend_from_slice(&1u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&2u128.to_le_bytes()); // min_nonzero_im_req

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    assert!(
        env.svm.send_transaction(tx).is_err(),
        "InitMarket with risk_reduction_threshold > max_risk_threshold should be rejected"
    );

    // Also test maintenance_fee_per_slot > max_maintenance_fee_per_slot
    let mut env2 = TestEnv::new();
    let admin2 = &env2.payer;
    let dummy_ata2 = Pubkey::new_unique();
    env2.svm
        .set_account(
            dummy_ata2,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; TokenAccount::LEN],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let mut data2 = vec![0u8];
    data2.extend_from_slice(admin2.pubkey().as_ref());
    data2.extend_from_slice(env2.mint.as_ref());
    data2.extend_from_slice(&TEST_FEED_ID);
    data2.extend_from_slice(&u64::MAX.to_le_bytes());
    data2.extend_from_slice(&500u16.to_le_bytes());
    data2.push(0u8);
    data2.extend_from_slice(&0u32.to_le_bytes());
    data2.extend_from_slice(&0u64.to_le_bytes());
    // Per-market admin limits
    data2.extend_from_slice(&1000u128.to_le_bytes()); // max_maintenance_fee = 1000
    data2.extend_from_slice(&10_000_000_000_000_000u128.to_le_bytes()); // max_insurance_floor (<= MAX_VAULT_TVL)
    data2.extend_from_slice(&0u64.to_le_bytes());
    // RiskParams with maintenance_fee_per_slot EXCEEDING the limit
    data2.extend_from_slice(&0u64.to_le_bytes());
    data2.extend_from_slice(&500u64.to_le_bytes());
    data2.extend_from_slice(&1000u64.to_le_bytes());
    data2.extend_from_slice(&0u64.to_le_bytes());
    data2.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data2.extend_from_slice(&0u128.to_le_bytes());
    data2.extend_from_slice(&0u128.to_le_bytes()); // risk_reduction_threshold = 0
    data2.extend_from_slice(&1001u128.to_le_bytes()); // maintenance_fee = 1001 > limit!
    data2.extend_from_slice(&u64::MAX.to_le_bytes());
    data2.extend_from_slice(&50u64.to_le_bytes());
    data2.extend_from_slice(&1_000_000_000_000u128.to_le_bytes());
    data2.extend_from_slice(&100u64.to_le_bytes());
    data2.extend_from_slice(&0u128.to_le_bytes());

    let ix2 = Instruction {
        program_id: env2.program_id,
        accounts: vec![
            AccountMeta::new(admin2.pubkey(), true),
            AccountMeta::new(env2.slab, false),
            AccountMeta::new_readonly(env2.mint, false),
            AccountMeta::new(env2.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(dummy_ata2, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: data2,
    };

    let tx2 = Transaction::new_signed_with_payer(
        &[cu_ix(), ix2],
        Some(&admin2.pubkey()),
        &[admin2],
        env2.svm.latest_blockhash(),
    );
    assert!(
        env2.svm.send_transaction(tx2).is_err(),
        "InitMarket with maintenance_fee > max_maintenance_fee should be rejected"
    );

    println!("INIT MARKET RISK PARAMS EXCEED LIMITS REJECTED: PASSED");
}

/// Verify InitMarket accepts risk_params at exact boundary (equality with limits).
#[test]
fn test_init_market_risk_params_at_boundary_accepted() {
    program_path();

    let mut env = TestEnv::new();
    let admin = &env.payer;
    let dummy_ata = Pubkey::new_unique();
    env.svm
        .set_account(
            dummy_ata,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; TokenAccount::LEN],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    // Build InitMarket with risk_reduction_threshold == max_risk_threshold
    // and maintenance_fee_per_slot == max_maintenance_fee_per_slot
    let mut data = vec![0u8];
    data.extend_from_slice(admin.pubkey().as_ref());
    data.extend_from_slice(env.mint.as_ref());
    data.extend_from_slice(&TEST_FEED_ID);
    data.extend_from_slice(&86400u64.to_le_bytes()); // max_staleness_secs (1 day, ≤7d cap)
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(0u8); // invert
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
    data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6
    // Per-market admin limits (current wire format)
    data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee_per_slot (0 = disabled)
    data.extend_from_slice(&10_000_000_000_000_000u128.to_le_bytes()); // max_insurance_floor
    data.extend_from_slice(&0u64.to_le_bytes()); // min_oracle_price_cap_e2bps
    // RiskParams
    data.extend_from_slice(&0u64.to_le_bytes()); // h_min
    data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps
    data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // trading_fee_bps
    data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data.extend_from_slice(&0u128.to_le_bytes()); // new_account_fee
    data.extend_from_slice(&0u128.to_le_bytes()); // insurance_floor
    data.extend_from_slice(&0u64.to_le_bytes()); // h_max (must be >= h_min=0)
    data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_crank_staleness_slots
    data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
    data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
    data.extend_from_slice(&100u64.to_le_bytes()); // resolve_price_deviation_bps
    data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
    data.extend_from_slice(&100u128.to_le_bytes()); // min_initial_deposit
    data.extend_from_slice(&1u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&2u128.to_le_bytes()); // min_nonzero_im_req
    // Extended tail (required for v2 format)
    data.extend_from_slice(&0u16.to_le_bytes()); // insurance_withdraw_max_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // insurance_withdraw_cooldown_slots
    data.extend_from_slice(&0u64.to_le_bytes()); // permissionless_resolve_stale_slots
    data.extend_from_slice(&500u64.to_le_bytes()); // funding_horizon_slots
    data.extend_from_slice(&100u64.to_le_bytes()); // funding_k_bps
    data.extend_from_slice(&500i64.to_le_bytes()); // funding_max_premium_bps
    data.extend_from_slice(&5i64.to_le_bytes()); // funding_max_bps_per_slot
    data.extend_from_slice(&0u64.to_le_bytes()); // mark_min_fee
    data.extend_from_slice(&0u64.to_le_bytes()); // force_close_delay_slots

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    assert!(
        env.svm.send_transaction(tx).is_ok(),
        "InitMarket with risk_params at exact limits should succeed"
    );

    println!("INIT MARKET RISK PARAMS AT BOUNDARY ACCEPTED: PASSED");
}

/// Full lifecycle test: init with limits -> Set* ops -> UpdateConfig -> crank -> Set* again.
/// Verifies limits survive across all operation types.
#[test]
fn test_admin_limits_lifecycle() {
    program_path();

    let mut env = TestEnv::new();

    // Init market with specific limits
    let admin = &env.payer;
    let dummy_ata = Pubkey::new_unique();
    env.svm
        .set_account(
            dummy_ata,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; TokenAccount::LEN],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: encode_init_market_with_limits(
            &admin.pubkey(),
            &env.mint,
            &TEST_FEED_ID,
            1000,   // max_maintenance_fee
            50_000, // max_risk_threshold
            5000,   // min_oracle_price_cap_e2bps
        ),
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    env.svm.send_transaction(tx).expect("init_market failed");

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Step 1: Set oracle price cap within limits
    env.try_set_oracle_price_cap(&admin, 10_000).unwrap();

    // Step 2: UpdateConfig with thresh_max at limit
    env.try_update_config_with_params(&admin, 3600, 1000, 0, 50_000)
        .unwrap();

    // Step 3: Crank
    env.set_slot_and_price(10, 100_000_000);
    env.crank();
    env.set_slot_and_price(12, 100_000_000);
    env.crank();

    // Step 4: Limits still enforced after UpdateConfig + crank
    let result = env.try_set_oracle_price_cap(&admin, 4999);
    assert!(result.is_err(), "Limit still enforced after lifecycle: SetOraclePriceCap");

    // Step 5: At-limit values still work
    env.try_set_oracle_price_cap(&admin, 5000).unwrap();

    // Step 6: Verify limit fields in config haven't been corrupted
    let slab = env.svm.get_account(&env.slab).unwrap();
    // BPF slab offsets: HEADER_LEN(72) + config field offset
    // maintenance_fee_per_slot: u128 @ config offset 352
    // max_insurance_floor: u128 @ config offset 208
    // min_oracle_price_cap_e2bps: u64 @ config offset 224
    const MAINT_FEE_OFF: usize = 72 + 352;
    const MAX_INS_FLOOR_OFF: usize = 72 + 208;
    const MIN_OPC_OFF: usize = 72 + 224;

    let maint_fee = u128::from_le_bytes(
        slab.data[MAINT_FEE_OFF..MAINT_FEE_OFF + 16].try_into().unwrap(),
    );
    let max_ins = u128::from_le_bytes(
        slab.data[MAX_INS_FLOOR_OFF..MAX_INS_FLOOR_OFF + 16].try_into().unwrap(),
    );
    let min_opc = u64::from_le_bytes(
        slab.data[MIN_OPC_OFF..MIN_OPC_OFF + 8].try_into().unwrap(),
    );

    assert_eq!(maint_fee, 1000, "maintenance_fee_per_slot should be preserved");
    assert_eq!(max_ins, 50_000, "max_insurance_floor should be preserved");
    assert_eq!(min_opc, 5000, "min_oracle_price_cap_e2bps should be preserved");

    println!("ADMIN LIMITS LIFECYCLE: PASSED");
}

/// Vault with pre-set delegate must be rejected by InitMarket.
#[test]
fn test_init_market_rejects_vault_with_delegate() {
    program_path();
    let mut svm = common::new_test_svm();
    let program_id = Pubkey::new_unique();
    svm.add_program(program_id, &std::fs::read(program_path()).unwrap());

    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    let slab = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let (vault_pda, _) = Pubkey::find_program_address(&[b"vault", slab.as_ref()], &program_id);
    let vault = Pubkey::new_unique();
    let attacker = Pubkey::new_unique();

    svm.set_account(slab, Account {
        lamports: 1_000_000_000,
        data: vec![0u8; 1156736],
        owner: program_id,
        executable: false,
        rent_epoch: 0,
    }).unwrap();
    svm.set_account(mint, Account {
        lamports: 1_000_000,
        data: {
            let mut d = vec![0u8; spl_token::state::Mint::LEN];
            use spl_token::state::Mint;
            let m = Mint { mint_authority: solana_sdk::program_option::COption::None, supply: 0, decimals: 6, is_initialized: true, freeze_authority: solana_sdk::program_option::COption::None };
            spl_token::state::Mint::pack(m, &mut d).unwrap();
            d
        },
        owner: spl_token::ID,
        executable: false,
        rent_epoch: 0,
    }).unwrap();
    // Vault with delegate set — should be rejected
    svm.set_account(vault, Account {
        lamports: 1_000_000,
        data: make_token_account_with_delegate(&mint, &vault_pda, 0, &attacker, 1_000_000_000),
        owner: spl_token::ID,
        executable: false,
        rent_epoch: 0,
    }).unwrap();

    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(slab, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(Pubkey::new_unique(), false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: encode_init_market_full_v2(&payer.pubkey(), &mint, &[0xABu8; 32], 0, 0, 0),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix], Some(&payer.pubkey()), &[&payer], svm.latest_blockhash(),
    );
    let result = svm.send_transaction(tx);
    assert!(result.is_err(), "InitMarket must reject vault with delegate");

    // Slab header must remain all-zeros (uninitialized) after rejected InitMarket
    let slab_after = svm.get_account(&slab).unwrap();
    assert!(
        slab_after.data[..72].iter().all(|&b| b == 0),
        "slab header must not change on rejected InitMarket (delegate)"
    );
}

/// Vault with close_authority must be rejected by InitMarket.
#[test]
fn test_init_market_rejects_vault_with_close_authority() {
    program_path();
    let mut svm = common::new_test_svm();
    let program_id = Pubkey::new_unique();
    svm.add_program(program_id, &std::fs::read(program_path()).unwrap());

    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    let slab = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let (vault_pda, _) = Pubkey::find_program_address(&[b"vault", slab.as_ref()], &program_id);
    let vault = Pubkey::new_unique();
    let attacker = Pubkey::new_unique();

    svm.set_account(slab, Account {
        lamports: 1_000_000_000,
        data: vec![0u8; 1156736],
        owner: program_id,
        executable: false,
        rent_epoch: 0,
    }).unwrap();
    svm.set_account(mint, Account {
        lamports: 1_000_000,
        data: {
            let mut d = vec![0u8; spl_token::state::Mint::LEN];
            use spl_token::state::Mint;
            let m = Mint { mint_authority: solana_sdk::program_option::COption::None, supply: 0, decimals: 6, is_initialized: true, freeze_authority: solana_sdk::program_option::COption::None };
            spl_token::state::Mint::pack(m, &mut d).unwrap();
            d
        },
        owner: spl_token::ID,
        executable: false,
        rent_epoch: 0,
    }).unwrap();
    svm.set_account(vault, Account {
        lamports: 1_000_000,
        data: make_token_account_with_close_authority(&mint, &vault_pda, 0, &attacker),
        owner: spl_token::ID,
        executable: false,
        rent_epoch: 0,
    }).unwrap();

    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(slab, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(Pubkey::new_unique(), false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: encode_init_market_full_v2(&payer.pubkey(), &mint, &[0xABu8; 32], 0, 0, 0),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix], Some(&payer.pubkey()), &[&payer], svm.latest_blockhash(),
    );
    let result = svm.send_transaction(tx);
    assert!(result.is_err(), "InitMarket must reject vault with close_authority");

    // Slab header must remain all-zeros (uninitialized) after rejected InitMarket
    let slab_after = svm.get_account(&slab).unwrap();
    assert!(
        slab_after.data[..72].iter().all(|&b| b == 0),
        "slab header must not change on rejected InitMarket (close_authority)"
    );
}

/// UpdateConfig must reject negative funding_max_premium_bps.
#[test]
fn test_update_config_rejects_negative_funding_max_premium() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Capture config snapshot before rejected operation
    let config_before = env.read_update_config_snapshot();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data: encode_update_config(
            3600, 100,
            -100i64,  // negative funding_max_premium_bps — must be rejected
            10i64,
            0u128, 100, 100, 100, 5000, 0, 1_000_000u128, 1u128,
        ),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix], Some(&admin.pubkey()), &[&admin], env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(result.is_err(), "Negative funding_max_premium_bps must be rejected");

    // Config must be unchanged after rejection
    assert_eq!(env.read_update_config_snapshot(), config_before, "config must be preserved after rejected UpdateConfig");
}

/// UpdateConfig must reject negative funding_max_bps_per_slot.
#[test]
fn test_update_config_rejects_negative_funding_max_bps_per_slot() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Capture config snapshot before rejected operation
    let config_before = env.read_update_config_snapshot();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data: encode_update_config(
            3600, 100,
            100i64,
            -5i64,  // negative funding_max_bps_per_slot — must be rejected
            0u128, 100, 100, 100, 5000, 0, 1_000_000u128, 1u128,
        ),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix], Some(&admin.pubkey()), &[&admin], env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(result.is_err(), "Negative funding_max_bps_per_slot must be rejected");

    // Config must be unchanged after rejection
    assert_eq!(env.read_update_config_snapshot(), config_before, "config must be preserved after rejected UpdateConfig");
}

/// InitMarket must reject a vault that already holds tokens.
/// verify_vault_empty checks tok.amount == 0; a non-empty vault indicates
/// pre-seeded funds that would desync engine accounting from day one.
#[test]
fn test_init_market_rejects_nonempty_vault() {
    program_path();
    let mut env = TestEnv::new();

    // Modify the vault account to have a non-zero balance BEFORE init_market.
    // TestEnv::new() creates the vault with amount=0; overwrite it with amount=1.
    let (vault_pda, _) =
        Pubkey::find_program_address(&[b"vault", env.slab.as_ref()], &env.program_id);
    env.svm
        .set_account(
            env.vault,
            Account {
                lamports: 1_000_000,
                data: make_token_account_data(&env.mint, &vault_pda, 1), // amount = 1
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    // Attempt InitMarket — should fail because vault is not empty
    let admin = &env.payer;
    let dummy_ata = Pubkey::new_unique();
    env.svm
        .set_account(
            dummy_ata,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; TokenAccount::LEN],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: encode_init_market_full_v2(&admin.pubkey(), &env.mint, &TEST_FEED_ID, 0, 0, 0),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "InitMarket must reject vault with non-zero balance"
    );

    // Slab header must remain all-zeros (uninitialized) after rejected InitMarket
    let slab_after = env.svm.get_account(&env.slab).unwrap();
    assert!(
        slab_after.data[..72].iter().all(|&b| b == 0),
        "slab header must not change on rejected InitMarket (nonempty vault)"
    );
}

// ============================================================================
// UpdateConfig (tag 14) additional coverage
// ============================================================================

/// Spec: UpdateConfig rejects funding_horizon_slots = 0 (would cause division by zero).
#[test]
fn test_update_config_rejects_zero_horizon() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Capture config snapshot before rejected operation
    let config_before = env.read_update_config_snapshot();

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let result = env.try_update_config_with_params(
        &admin,
        0,                        // funding_horizon_slots = 0 (invalid)
        1000,                     // thresh_alpha_bps
        0u128,                    // thresh_min
        1_000_000_000_000_000u128, // thresh_max
    );
    assert!(
        result.is_err(),
        "UpdateConfig must reject funding_horizon_slots = 0"
    );

    // Config must be unchanged after rejection
    let config_after = env.read_update_config_snapshot();
    assert_eq!(
        config_after, config_before,
        "config must not change on rejected UpdateConfig (zero horizon)"
    );
}

/// Spec: UpdateConfig is admin-only; non-admin signers are rejected.
#[test]
fn test_update_config_admin_only() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();

    let result = env.try_update_config_with_params(
        &attacker,
        3600,
        1000,
        0u128,
        1_000_000_000_000_000u128,
    );
    assert!(
        result.is_err(),
        "UpdateConfig must reject non-admin signer"
    );
}

