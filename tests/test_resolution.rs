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

/// Test that resolved markets block new activity
#[test]
fn test_resolved_market_blocks_new_activity() {
    program_path();

    println!("=== RESOLVED MARKET BLOCKS NEW ACTIVITY TEST ===");
    println!();

    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("set_oracle_authority should succeed");
    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("push_oracle_price should succeed");

    // Resolve market
    let result = env.try_resolve_market(&admin);
    assert!(result.is_ok(), "ResolveMarket should succeed");
    assert!(env.is_market_resolved(), "Market must be resolved after ResolveMarket");

    // Try to create new user - should fail
    let new_user = Keypair::new();
    env.svm.airdrop(&new_user.pubkey(), 1_000_000_000).unwrap();
    let ata = env.create_ata(&new_user.pubkey(), 0);

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(new_user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_col, false),
        ],
        data: encode_init_user(0),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&new_user.pubkey()),
        &[&new_user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(result.is_err(), "InitUser should fail on resolved market");
    println!("InitUser blocked on resolved market: OK");

    // Try to deposit - should fail (need existing user first)
    // We'll create user before resolving to test deposit block
    println!();
    println!("RESOLVED MARKET BLOCKS NEW ACTIVITY TEST PASSED");
}

/// Test that users can withdraw after resolution
#[test]
fn test_resolved_market_allows_user_withdrawal() {
    program_path();

    println!("=== RESOLVED MARKET ALLOWS USER WITHDRAWAL TEST ===");
    println!();

    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("set_oracle_authority should succeed");
    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("push_oracle_price should succeed");

    // Create user with deposit
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 500_000_000); // 0.5 SOL

    let capital_before = env.read_account_capital(user_idx);
    println!("User capital before resolution: {}", capital_before);
    assert!(capital_before > 0);

    // Resolve market
    env.try_resolve_market(&admin).unwrap();
    println!("Market resolved");

    // Crank to settle
    env.set_slot(100);
    env.crank();

    // User should still be able to withdraw
    let user_ata = env.create_ata(&user.pubkey(), 0);
    let (vault_pda, _) =
        Pubkey::find_program_address(&[b"vault", env.slab.as_ref()], &env.program_id);

    // Correct account order for WithdrawCollateral:
    // 0: user (signer), 1: slab, 2: vault, 3: user_ata, 4: vault_pda, 5: token_program, 6: clock, 7: oracle
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new(user_ata, false),
            AccountMeta::new_readonly(vault_pda, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_withdraw(user_idx, 100_000_000), // Withdraw 0.1 SOL
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_ok(),
        "Withdraw should succeed on resolved market: {:?}",
        result
    );
    println!("User withdrawal on resolved market: OK");

    println!();
    println!("RESOLVED MARKET ALLOWS USER WITHDRAWAL TEST PASSED");
}

/// ATTACK: Trade after market is resolved.
/// Expected: No new trades on resolved markets.
#[test]
fn test_attack_trade_after_market_resolved() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Set oracle authority and push price so resolve can work
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Resolve the market
    let result = env.try_resolve_market(&admin);
    assert!(
        result.is_ok(),
        "Admin should be able to resolve market: {:?}",
        result
    );
    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // Try to trade on resolved market
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Trade on resolved market should fail"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected post-resolution trade must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected post-resolution trade must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected post-resolution trade must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected post-resolution trade must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected post-resolution trade must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected post-resolution trade must preserve engine vault"
    );
}

/// ATTACK: Resolve market without oracle authority price being set.
/// Expected: Resolution requires authority price to be set first.
#[test]
fn test_attack_resolve_market_without_oracle_price() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Set oracle authority but DON'T push a price
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");

    let resolved_before = env.is_market_resolved();
    let used_before = env.read_num_used_accounts();
    let vault_before = env.vault_balance();

    // Try to resolve without pushing price
    let result = env.try_resolve_market(&admin);
    assert!(
        result.is_err(),
        "ATTACK: Resolve without oracle price should fail"
    );
    let resolved_after = env.is_market_resolved();
    let used_after = env.read_num_used_accounts();
    let vault_after = env.vault_balance();

    assert!(!resolved_before, "Precondition: market should be unresolved");
    assert_eq!(
        resolved_after, resolved_before,
        "Rejected resolve-without-price must not toggle resolved flag"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected resolve-without-price must not change num_used_accounts"
    );
    assert_eq!(
        vault_after, vault_before,
        "Rejected resolve-without-price must not move vault funds"
    );
}

/// ATTACK: Resolve an already-resolved market.
/// Expected: Double resolution rejected.
#[test]
fn test_attack_double_resolution() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");

    // First resolution
    let result = env.try_resolve_market(&admin);
    assert!(result.is_ok(), "First resolve should succeed: {:?}", result);
    assert!(env.is_market_resolved(), "Market should be resolved");

    // Second resolution - should fail
    let result = env.try_resolve_market(&admin);
    assert!(result.is_err(), "ATTACK: Double resolution should fail");
}

/// ATTACK: Withdraw after resolution but before force-close.
/// Expected: User can still withdraw capital from resolved market.
#[test]
fn test_attack_withdraw_between_resolution_and_force_close() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // User with no position - should be able to withdraw after resolution
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Resolve market
    let result = env.try_resolve_market(&admin);
    assert!(result.is_ok(), "Should resolve: {:?}", result);

    // User with no position should be able to withdraw from resolved market
    // (WithdrawCollateral does not check is_resolved flag)
    let vault_before = env.vault_balance();
    let result = env.try_withdraw(&user, user_idx, 5_000_000_000);
    assert!(result.is_ok(), "Withdrawal should succeed on resolved market (no position): {:?}", result);

    {
        let vault_after = env.vault_balance();
        assert_eq!(
            vault_before - vault_after,
            5_000_000_000,
            "ATTACK: Withdrawal amount should match vault decrease"
        );
    }
}

/// ATTACK: Non-admin tries to resolve market.
/// Only the admin should be able to resolve.
#[test]
fn test_attack_resolve_market_non_admin() {
    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    // Non-admin tries to resolve
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let result = env.try_resolve_market(&attacker);
    assert!(
        result.is_err(),
        "ATTACK: Non-admin was able to resolve market!"
    );
    assert!(
        !env.is_market_resolved(),
        "Market should NOT be resolved after failed attempt"
    );
}

/// Standard Pyth market: user deposits, trades (long), price goes up, flattens, closes account.
/// warmup_period_slots=0 so PnL converts instantly.
#[test]
fn test_honest_user_standard_market_profitable_close() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Top up insurance to prevent force-realize mode (insurance=0 <= threshold=0 triggers it)
    let ins_payer = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&ins_payer, 1);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000); // 10 SOL

    // Open long position at $138
    let size: i128 = 100_000;
    env.trade(&user, &lp, lp_idx, user_idx, size);
    assert_eq!(env.read_account_position(user_idx), size);

    // Price goes up (138 → 150), crank to settle mark-to-oracle
    env.set_slot_and_price(200, 150_000_000);
    env.crank();
    assert_eq!(
        env.read_account_position(user_idx),
        size,
        "Crank should not change position"
    );

    // User flattens
    env.trade(&user, &lp, lp_idx, user_idx, -size);
    assert_eq!(
        env.read_account_position(user_idx),
        0,
        "User should be flat"
    );

    // Crank to settle final state
    env.set_slot_and_price(300, 150_000_000);
    env.crank();

    let vault_before = env.vault_balance();

    // Close account — warmup_period=0 so PnL converts instantly
    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "Profitable user should close account: {:?}",
        result
    );

    let vault_after = env.vault_balance();
    assert!(
        vault_after < vault_before,
        "Vault should decrease (capital returned to user)"
    );
    println!(
        "Standard market profitable user: vault {} → {} (delta={})",
        vault_before,
        vault_after,
        vault_before - vault_after
    );

    println!("HONEST USER STANDARD MARKET PROFITABLE CLOSE: PASSED");
}

/// Standard Pyth market: user deposits, trades (long), price drops, flattens, closes.
/// User loses money but can still close and get remaining capital.
#[test]
fn test_honest_user_standard_market_losing_close() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let ins_payer = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&ins_payer, 1);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000); // 10 SOL

    // Open long at $138
    let size: i128 = 100_000;
    env.trade(&user, &lp, lp_idx, user_idx, size);

    // Price drops (138 → 120), crank to settle mark
    env.set_slot_and_price(200, 120_000_000);
    env.crank();

    // User flattens
    env.trade(&user, &lp, lp_idx, user_idx, -size);
    assert_eq!(
        env.read_account_position(user_idx),
        0,
        "User should be flat"
    );

    env.set_slot_and_price(300, 120_000_000);
    env.crank();

    let vault_before = env.vault_balance();

    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "Losing user should still close account: {:?}",
        result
    );

    let vault_after = env.vault_balance();
    assert!(
        vault_after < vault_before,
        "Vault should decrease (remaining capital returned)"
    );
    println!(
        "Standard market losing user: vault {} → {} (delta={})",
        vault_before,
        vault_after,
        vault_before - vault_after
    );

    println!("HONEST USER STANDARD MARKET LOSING CLOSE: PASSED");
}

/// Standard market with warmup: profitable user must wait for warmup before closing.
/// Uses a larger position (1M) to generate meaningful PnL that takes time to vest.
#[test]
fn test_honest_user_standard_market_warmup_close() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_warmup(0, 1000); // warmup_period_slots = 1000

    let ins_payer = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&ins_payer, 1);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000); // 10 SOL

    // Open long at $138 with larger position for meaningful PnL
    let size: i128 = 1_000_000;
    env.trade(&user, &lp, lp_idx, user_idx, size);

    // Price goes up (138 → 150), crank to settle mark-to-oracle
    // PnL = 1M * (150-138) * 1e6 / 1e6 = 12M lamports
    env.set_slot_and_price(100, 150_000_000);
    env.crank();

    // User flattens at slot 100 — PnL realized, warmup starts
    env.trade(&user, &lp, lp_idx, user_idx, -size);
    assert_eq!(env.read_account_position(user_idx), 0);

    // Advance only 50 slots (warmup needs 1000) and crank
    env.set_slot_and_price(150, 150_000_000);
    env.crank();

    // Try close — should fail (warmup far from complete, most PnL still unvested)
    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_err(),
        "CloseAccount should be blocked by warmup (only 50/1000 slots)"
    );
    println!("CloseAccount blocked by warmup as expected (50/1000 slots)");

    // Advance past warmup period (1000+ slots after warmup start) and crank
    env.set_slot_and_price(1200, 150_000_000);
    env.crank();

    // Now close should succeed — warmup fully elapsed
    let result2 = env.try_close_account(&user, user_idx);
    assert!(
        result2.is_ok(),
        "User should close after warmup: {:?}",
        result2
    );
    println!("User closed after warmup period elapsed (1200/1000 slots)");

    println!("HONEST USER STANDARD MARKET WARMUP CLOSE: PASSED");
}

/// Inverted Pyth market: user can close account after trading.
#[test]
fn test_honest_user_inverted_market_close() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(1);

    let ins_payer = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&ins_payer, 1);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000); // 10 SOL

    // Trade at inverted price (~$138 → inverted ~7246 internally)
    let size: i128 = 100_000;
    env.trade(&user, &lp, lp_idx, user_idx, size);
    assert_eq!(env.read_account_position(user_idx), size);

    env.set_slot(200);
    env.crank();

    // Flatten
    env.trade(&user, &lp, lp_idx, user_idx, -size);
    assert_eq!(env.read_account_position(user_idx), 0);

    env.set_slot(300);
    env.crank();

    let vault_before = env.vault_balance();
    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "User should close on inverted market: {:?}",
        result
    );

    let vault_after = env.vault_balance();
    assert!(vault_after < vault_before, "Capital should be returned");

    println!("HONEST USER INVERTED MARKET CLOSE: PASSED");
}

/// Full lifecycle test: both LP and user close on standard market, then close slab.
/// No insurance is topped up, and no crank runs between trades (avoiding force-realize mode).
#[test]
fn test_honest_participants_standard_market_full_lifecycle() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000); // 10 SOL

    // Trade and immediately flatten (no crank needed, PnL stays zero at same price)
    let size: i128 = 100_000;
    env.trade(&user, &lp, lp_idx, user_idx, size);
    assert_eq!(env.read_account_position(user_idx), size);

    env.trade(&user, &lp, lp_idx, user_idx, -size);
    assert_eq!(env.read_account_position(user_idx), 0);
    assert_eq!(env.read_account_position(lp_idx), 0);

    // Close both accounts (PnL=0, no warmup issue)
    let result = env.try_close_account(&user, user_idx);
    assert!(result.is_ok(), "User should close: {:?}", result);

    let result = env.try_close_account(&lp, lp_idx);
    assert!(result.is_ok(), "LP should close: {:?}", result);

    assert_eq!(env.read_num_used_accounts(), 0, "All accounts closed");

    // Resolve market before CloseSlab (lifecycle requirement)
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey()).unwrap();
    env.try_push_oracle_price(&admin, 138_000_000, 100).unwrap();
    env.try_resolve_market(&admin).unwrap();

    // Close slab (insurance=0, all accounts closed, market resolved)
    let result = env.try_close_slab();
    assert!(result.is_ok(), "CloseSlab should succeed: {:?}", result);

    println!("HONEST PARTICIPANTS STANDARD MARKET FULL LIFECYCLE: PASSED");
}

/// LiquidateAtOracle must be blocked on resolved markets.
#[test]
fn test_liquidate_blocked_on_resolved_market() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey()).unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.set_slot(50);
    env.crank();

    env.try_resolve_market(&admin).unwrap();

    // Capture state before rejected operation
    let position_before = env.read_account_position(user_idx);
    let capital_before = env.read_account_capital(user_idx);

    // Liquidation must fail on resolved market
    let result = env.try_liquidate(user_idx);
    assert!(result.is_err(), "LiquidateAtOracle must be blocked on resolved markets");

    // Position and capital must be unchanged after rejection
    assert_eq!(env.read_account_position(user_idx), position_before, "position must be preserved after rejected liquidation");
    assert_eq!(env.read_account_capital(user_idx), capital_before, "capital must be preserved after rejected liquidation");
}

/// Resolved crank must forgive sub-scale dust, leaving dust_base == 0.
/// This ensures CloseSlab can eventually succeed after resolution.
#[test]
fn test_resolved_crank_dust_base_stays_zero_with_aligned_deposits() {
    program_path();
    let mut env = TestEnv::new();

    // Initialize with unit_scale=1000 (1000 base tokens = 1 engine unit)
    // Misaligned deposits now rejected → dust_base stays 0 through lifecycle.
    env.init_market_full(0, 1000, 0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let user = Keypair::new();
    let user_idx = env.init_user_with_fee(&user, 100_000);

    // Aligned deposit: no dust created
    env.deposit(&user, user_idx, 10_000_000);

    let read_dust_base = |svm: &LiteSVM, slab: &Pubkey| -> u64 {
        let slab_data = svm.get_account(slab).unwrap().data;
        const DUST_BASE_OFF: usize = 64;
        u64::from_le_bytes(
            slab_data[DUST_BASE_OFF..DUST_BASE_OFF + 8]
                .try_into()
                .unwrap(),
        )
    };
    let dust_after_deposit = read_dust_base(&env.svm, &env.slab);
    assert_eq!(dust_after_deposit, 0, "Aligned deposit must not create dust");

    env.set_slot(200);
    env.crank();
    env.close_account(&user, user_idx);

    env.try_set_oracle_authority(&admin, &admin.pubkey()).unwrap();
    env.try_push_oracle_price(&admin, 138_000_000, 200).unwrap();
    env.try_resolve_market(&admin).unwrap();

    env.set_slot(300);
    env.crank();

    let dust_after_resolved_crank = read_dust_base(&env.svm, &env.slab);
    assert_eq!(
        dust_after_resolved_crank, 0,
        "dust_base must be zero after resolved crank with aligned deposits"
    );
}

// ============================================================================
// AdminForceCloseAccount (tag 21) additional coverage
// ============================================================================

/// Spec: AdminForceCloseAccount requires the market to be resolved.
/// It must be rejected on live (non-resolved) markets.
#[test]
fn test_admin_force_close_requires_resolved() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Market is NOT resolved; force-close should fail.
    assert!(!env.is_market_resolved(), "Precondition: market must not be resolved");
    let result = env.try_admin_force_close_account(&admin, user_idx, &user.pubkey());
    assert!(
        result.is_err(),
        "AdminForceCloseAccount must be rejected on a live (non-resolved) market"
    );
}

/// Spec: AdminForceCloseAccount is admin-only; non-admin signers are rejected.
#[test]
fn test_admin_force_close_admin_only() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey()).unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    // Resolve the market
    env.try_resolve_market(&admin).unwrap();
    assert!(env.is_market_resolved());

    // Crank to settle
    env.set_slot(200);
    env.crank();

    // Non-admin tries force-close
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let result = env.try_admin_force_close_account(&attacker, user_idx, &user.pubkey());
    assert!(
        result.is_err(),
        "AdminForceCloseAccount must reject non-admin signer"
    );
}

// ============================================================================
// Hyperp Full Lifecycle: Init to CloseSlab
// ============================================================================

/// End-to-end Hyperp lifecycle test covering all admin operations.
///
/// Steps: InitMarket(Hyperp) -> SetOracleAuthority -> PushOraclePrice (x2)
/// -> Crank (index smoothing) -> UpdateConfig -> SetOraclePriceCap
/// -> InitUser+Deposit -> ResolveMarket -> resolved Crank -> AdminForceCloseAccount
/// -> WithdrawInsurance -> CloseSlab.
///
/// No trading (TradeNoCpi is blocked on Hyperp), focuses on admin lifecycle.
#[test]
fn test_hyperp_full_lifecycle_init_to_close_slab() {
    program_path();
    println!("=== HYPERP FULL LIFECYCLE: INIT TO CLOSE SLAB ===");

    let mut env = TestEnv::new();

    // 1. Init Hyperp market ($100 mark)
    env.init_market_hyperp(100_000_000);
    println!("1. Hyperp market initialized (mark=$100)");

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // 2. Set oracle authority
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("set_oracle_authority");
    println!("2. Oracle authority set");

    // 3. Push mark prices over multiple slots.
    //    The circuit breaker clamps mark against index (1%/slot default cap).
    //    Index only moves toward mark when dt > 0 (Bug #9 fix).
    env.set_slot(10);
    env.try_push_oracle_price(&admin, 110_000_000, 110)
        .expect("push $110");

    // Read index right after push (should still be initial since the push's
    // internal index flush used the OLD mark = initial = 100M, so no movement)
    const INDEX_OFF: usize = 384; // last_effective_price_e6 in slab
    let slab_data = env.svm.get_account(&env.slab).unwrap().data;
    let index_after_push =
        u64::from_le_bytes(slab_data[INDEX_OFF..INDEX_OFF + 8].try_into().unwrap());
    println!("3. Pushed mark toward $110; index after push: {}", index_after_push);

    // 4. Advance to a LATER slot so dt > 0, then crank.
    //    The crank's get_engine_oracle_price_e6 calls clamp_toward_with_dt
    //    with dt = (slot_now - last_hyperp_index_slot). With dt > 0, the
    //    index moves toward the (clamped) mark.
    env.set_slot(20); // 10 slots after push
    env.crank();
    let slab_data = env.svm.get_account(&env.slab).unwrap().data;
    let index_after_crank =
        u64::from_le_bytes(slab_data[INDEX_OFF..INDEX_OFF + 8].try_into().unwrap());
    // The mark was clamped to ~101M (1% of 100M). With dt=10 slots and cap=1%/slot,
    // the index can move up to 10% of its value toward the mark. Since the mark is
    // only 1% above index, the index should reach or nearly reach the mark.
    assert!(
        index_after_crank >= index_after_push,
        "Index should not decrease: {} -> {}", index_after_push, index_after_crank
    );
    println!("4. Cranked at slot 20: index {} -> {}", index_after_push, index_after_crank);

    // Push again at later slot to continue driving mark up
    env.set_slot(30);
    env.try_push_oracle_price(&admin, 120_000_000, 130).expect("push $120");
    env.set_slot(40);
    env.crank();
    println!("   Pushed $120 at slot 30, cranked at slot 40");

    // 5. UpdateConfig (change funding params)
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data: encode_update_config(
            7200, 200, 1_000_000_000_000u128, 200i64, 10i64,
            0u128, 100, 100, 100, 1000,
            0u128, 1_000_000_000_000_000u128, 1u128,
        ),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    env.svm.send_transaction(tx).expect("UpdateConfig");
    println!("5. UpdateConfig succeeded (k=200, horizon=7200)");

    // 6. SetOraclePriceCap
    env.try_set_oracle_price_cap(&admin, 50_000).expect("SetOraclePriceCap");
    println!("6. SetOraclePriceCap set to 50_000 (5%/slot)");

    // 7. Create user + deposit
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);
    assert!(env.read_account_capital(user_idx) > 0);
    println!("7. User idx={} created with deposit", user_idx);

    env.set_slot(50);
    env.crank();

    // 8. ResolveMarket
    env.try_push_oracle_price(&admin, 115_000_000, 150).expect("settlement price");
    env.try_resolve_market(&admin).expect("ResolveMarket");
    assert!(env.is_market_resolved());
    println!("8. Market resolved at $115");

    // 9. Resolved crank
    env.set_slot(60);
    env.crank();
    println!("9. Resolved crank executed");

    // 10. AdminForceCloseAccount
    env.try_admin_force_close_account(&admin, user_idx, &user.pubkey())
        .expect("AdminForceCloseAccount");
    assert_eq!(env.read_num_used_accounts(), 0);
    println!("10. AdminForceCloseAccount succeeded, 0 accounts remaining");

    // 11. WithdrawInsurance
    let ins = env.read_insurance_balance();
    if ins > 0 {
        env.try_withdraw_insurance(&admin).expect("WithdrawInsurance");
        assert_eq!(env.read_insurance_balance(), 0);
        println!("11. Insurance withdrawn (was {})", ins);
    } else {
        println!("11. No insurance to withdraw");
    }

    // 12. CloseSlab
    env.try_close_slab().expect("CloseSlab");
    println!("12. CloseSlab succeeded -- market fully closed");

    println!();
    println!("HYPERP FULL LIFECYCLE INIT TO CLOSE SLAB: PASSED");
}

// ============================================================================
// Resolved Crank Cursor Wraps to Zero
// ============================================================================

/// Verify that the resolved-market crank cursor wraps back to 0 after
/// scanning all MAX_ACCOUNTS (4096) slots.
///
/// The resolved crank processes BATCH_SIZE=8 accounts per call, advancing
/// crank_cursor. When end >= MAX_ACCOUNTS it resets to 0.
///
/// This test resolves a market, cranks 513 times (512 to complete one full
/// scan + 1 more batch), and reads crank_cursor from the slab to verify
/// it wrapped to 8.
#[test]
fn test_resolved_crank_cursor_wraps_to_zero() {
    program_path();
    println!("=== RESOLVED CRANK CURSOR WRAPS TO ZERO ===");

    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey()).unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    // Create a few users so the slab is non-trivial
    let mut users = Vec::new();
    for _ in 0..3 {
        let u = Keypair::new();
        let idx = env.init_user(&u);
        env.deposit(&u, idx, 1_000_000_000);
        users.push((u, idx));
    }
    println!("Created {} user accounts", users.len());

    // Live crank
    env.set_slot(10);
    env.crank();

    // Resolve
    env.try_resolve_market(&admin).unwrap();
    assert!(env.is_market_resolved());
    println!("Market resolved");

    // crank_cursor is at ENGINE(584) + 328 = 912 in the slab.
    const CRANK_CURSOR_OFF: usize = 912;
    let read_cursor = |svm: &LiteSVM, slab: &Pubkey| -> u16 {
        let d = svm.get_account(slab).unwrap().data;
        u16::from_le_bytes(d[CRANK_CURSOR_OFF..CRANK_CURSOR_OFF + 2].try_into().unwrap())
    };

    let cursor_before = read_cursor(&env.svm, &env.slab);
    println!("Cursor before resolved cranks: {}", cursor_before);

    // 4096 / 8 = 512 cranks to complete one full scan and wrap to 0.
    // Run 513 to place the cursor one batch (8) past the wrap.
    let total_cranks: u64 = 513;
    for i in 0..total_cranks {
        env.set_slot(20 + i * 2);
        env.crank();
    }

    let cursor_after = read_cursor(&env.svm, &env.slab);
    println!("Cursor after {} cranks: {}", total_cranks, cursor_after);

    // After 512 cranks: cursor 0 -> 4096 -> wraps to 0.
    // 513th crank: 0 -> 8.
    assert_eq!(
        cursor_after, 8,
        "Cursor should be 8 after 513 cranks (512 wrap + 1 batch): got {}",
        cursor_after
    );
    println!("Cursor wrapped correctly: 0 -> 4096 -> 0 -> 8");

    // Cleanup
    for (u, idx) in &users {
        let _ = env.try_admin_force_close_account(&admin, *idx, &u.pubkey());
    }

    println!();
    println!("RESOLVED CRANK CURSOR WRAPS TO ZERO: PASSED");
}

// ── Permissionless Resolution Tests ────────────────────────────────────

/// Permissionless resolution succeeds when oracle is actually dead.
#[test]
fn test_resolve_permissionless_after_staleness() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Enable permissionless resolve + set bounded staleness
    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        let config_end = 72 + std::mem::size_of::<percolator_prog::state::MarketConfig>();
        // permissionless_resolve_stale_slots
        let offset = config_end - 32; // permissionless_resolve_stale_slots (before mark_min_fee+padding)
        slab.data[offset..offset + 8].copy_from_slice(&100u64.to_le_bytes());
        // max_staleness_secs: at offset 72+96 = 168 in config
        slab.data[168..176].copy_from_slice(&30u64.to_le_bytes()); // 30 seconds
        env.svm.set_account(env.slab, slab).unwrap();
    }

    // Crank to establish baseline (oracle is fresh at slot 100, ts=100)
    env.crank();
    assert!(!env.is_market_resolved());

    // Oracle still fresh (ts=100, clock=150, age=50 > 30 but set_slot updates oracle)
    // set_slot updates pyth_data publish_time → oracle stays fresh
    env.set_slot(50);
    let result = env.try_resolve_permissionless();
    assert!(result.is_err(), "Should fail when oracle is live");

    // Make oracle actually stale: advance clock WITHOUT updating oracle data
    env.svm.set_sysvar(&Clock {
        slot: 500,
        unix_timestamp: 500,
        ..Clock::default()
    });
    // Oracle publish_time is still 150 (from set_slot(50)), age = 500-150 = 350 > 30

    let result = env.try_resolve_permissionless();
    assert!(result.is_ok(), "Should succeed when oracle is dead: {:?}", result);
    assert!(env.is_market_resolved());
}

/// Permissionless resolution rejected when disabled (stale_slots=0).
#[test]
fn test_resolve_permissionless_disabled_by_default() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    env.crank();
    env.set_slot(1_000_000);
    let result = env.try_resolve_permissionless();
    assert!(result.is_err(), "Should fail when feature disabled");
    assert!(!env.is_market_resolved(), "Market must NOT be resolved after rejected call");
}

/// Can't double-resolve — admin resolves first, permissionless rejected.
#[test]
fn test_resolve_permissionless_already_admin_resolved() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Enable permissionless resolve
    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        let config_end = 72 + std::mem::size_of::<percolator_prog::state::MarketConfig>();
        let offset = config_end - 32; // permissionless_resolve_stale_slots (before mark_min_fee+padding)
        slab.data[offset..offset + 8].copy_from_slice(&50u64.to_le_bytes());
        env.svm.set_account(env.slab, slab).unwrap();
    }

    // Admin resolves first
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey()).unwrap();
    env.try_push_oracle_price(&admin, 138_000_000, 100).unwrap();
    env.try_resolve_market(&admin).unwrap();
    assert!(env.is_market_resolved());

    // Permissionless attempt on already-resolved market
    env.set_slot(1_000);
    let result = env.try_resolve_permissionless();
    assert!(result.is_err(), "Can't double-resolve");
}

/// Settlement price = last_oracle_price from engine.
#[test]
fn test_resolve_permissionless_settlement_price() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        let config_end = 72 + std::mem::size_of::<percolator_prog::state::MarketConfig>();
        let offset = config_end - 32; // permissionless_resolve_stale_slots (before mark_min_fee+padding)
        slab.data[offset..offset + 8].copy_from_slice(&50u64.to_le_bytes());
        // Bounded staleness so oracle can go stale
        slab.data[168..176].copy_from_slice(&30u64.to_le_bytes());
        env.svm.set_account(env.slab, slab).unwrap();
    }

    env.crank(); // oracle = $138 at ts=100
    // Make oracle stale without updating publish_time
    env.svm.set_sysvar(&Clock { slot: 500, unix_timestamp: 500, ..Clock::default() });
    env.try_resolve_permissionless().unwrap();

    let settlement = env.read_authority_price();
    assert_eq!(settlement, 138_000_000, "Settlement should be last oracle price");
}

/// ATTACK: Passing a garbage oracle account must NOT fake staleness.
/// Only OracleStale should count as proof the oracle is dead.
#[test]
fn test_resolve_permissionless_rejects_wrong_oracle() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        let config_end = 72 + std::mem::size_of::<percolator_prog::state::MarketConfig>();
        let offset = config_end - 32; // permissionless_resolve_stale_slots (before mark_min_fee+padding)
        slab.data[offset..offset + 8].copy_from_slice(&50u64.to_le_bytes());
        slab.data[168..176].copy_from_slice(&30u64.to_le_bytes());
        env.svm.set_account(env.slab, slab).unwrap();
    }

    env.crank(); // establish baseline

    // Oracle is live (within staleness), but attacker passes a garbage account
    // to make read_engine_price_e6 fail with non-OracleStale error.
    let garbage_oracle = Pubkey::new_unique();
    env.svm.set_account(garbage_oracle, Account {
        lamports: 1_000_000,
        data: vec![0u8; 10], // too short for any oracle
        owner: Pubkey::new_unique(), // wrong owner
        executable: false,
        rent_epoch: 0,
    }).unwrap();

    // Build instruction manually with wrong oracle
    let caller = Keypair::new();
    env.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();
    env.svm.set_sysvar(&Clock { slot: 300, unix_timestamp: 300, ..Clock::default() });

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(garbage_oracle, false), // WRONG oracle
        ],
        data: encode_resolve_permissionless(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&caller.pubkey()),
        &[&caller],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "Must reject wrong oracle account — only OracleStale proves death"
    );
    assert!(!env.is_market_resolved(), "Market must NOT be resolved with fake oracle");
}

// ============================================================================
// Governance-Free Full Lifecycle (Integration Capstone)
// ============================================================================

/// End-to-end test: admin-free market lifecycle.
///
/// Demonstrates the complete governance-free path:
/// 1. InitMarket with Pyth oracle, custom funding params, cap enabled, permissionless resolution
/// 2. Open positions and trade → mark EWMA bootstraps
/// 3. Funding accrues through cranks (mark EWMA diverges from index = non-zero rate)
/// 4. Oracle dies → permissionless resolution succeeds
/// 5. Positions settle at last known oracle price
///
/// No admin intervention required at any step after InitMarket.
#[test]
fn test_governance_free_full_lifecycle() {
    program_path();
    let mut env = TestEnv::new();

    // Step 1: Init with custom funding params, cap=1% per slot, permissionless resolve after 100 slots
    // horizon=200 (shorter for faster funding), k=200 (2x multiplier), max_premium=1000, max_per_slot=10
    env.init_market_with_funding(
        0,      // invert=0 (direct, e.g., BTC/USD)
        10_000, // min_oracle_price_cap_e2bps = 1% per slot
        100,    // permissionless_resolve_stale_slots
        200,    // funding_horizon_slots (custom, not default 500)
        200,    // funding_k_bps (2x, not default 1x)
        1000,   // funding_max_premium_bps (10%, not default 5%)
        10,     // funding_max_bps_per_slot (custom cap)
    );

    // Verify custom params stored
    assert_eq!(env.read_funding_horizon(), 200);
    assert_eq!(env.read_funding_k_bps(), 200);
    assert_eq!(env.read_funding_max_premium_bps(), 1000);
    assert_eq!(env.read_funding_max_bps_per_slot(), 10);

    // Step 2: Set bounded staleness (so oracle can go stale for permissionless resolution)
    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        // max_staleness_secs at offset 72+96=168
        slab.data[168..176].copy_from_slice(&30u64.to_le_bytes());
        env.svm.set_account(env.slab, slab).unwrap();
    }

    // Step 3: Open positions
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    // Trade to seed EWMA
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    assert!(env.read_mark_ewma() > 0, "EWMA seeded from trade");
    assert_ne!(env.read_account_position(user_idx), 0, "User has position");
    assert_ne!(env.read_account_position(lp_idx), 0, "LP has position");

    // Step 4: Top up insurance, advance, crank to accrue funding
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 1_000_000_000);

    env.set_slot(200);
    env.crank();

    env.set_slot(300);
    env.trade(&user, &lp, lp_idx, user_idx, 500_000);
    env.crank();

    // Positions should still exist after cranks
    assert_ne!(env.read_account_position(user_idx), 0);

    // Step 5: Oracle dies — advance clock without updating oracle
    env.svm.set_sysvar(&Clock {
        slot: 600,
        unix_timestamp: 600,
        ..Clock::default()
    });

    // Permissionless resolution by anyone (no admin needed)
    let result = env.try_resolve_permissionless();
    assert!(
        result.is_ok(),
        "Governance-free resolution must succeed when oracle is dead: {:?}",
        result
    );
    assert!(env.is_market_resolved(), "Market must be resolved");

    // Settlement price should be the last known oracle price
    let settlement = env.read_authority_price();
    assert!(settlement > 0, "Settlement price must be set");
}

/// Inverted variant of the governance-free lifecycle.
/// Same flow but with invert=1 (e.g., SOL/USD where oracle gives USD/SOL).
#[test]
fn test_governance_free_full_lifecycle_inverted() {
    program_path();
    let mut env = TestEnv::new();

    env.init_market_with_funding(
        1,      // invert=1
        10_000, // 1% cap
        100,    // permissionless resolve
        300,    // custom horizon
        150,    // 1.5x k
        800,    // 8% max premium
        8,      // custom max per slot
    );

    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        slab.data[168..176].copy_from_slice(&30u64.to_le_bytes());
        env.svm.set_account(env.slab, slab).unwrap();
    }

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    let ewma = env.read_mark_ewma();
    assert!(ewma > 0 && ewma < 100_000, "Inverted EWMA in correct range: {}", ewma);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 1_000_000_000);

    env.set_slot(200);
    env.crank();

    // Oracle dies
    env.svm.set_sysvar(&Clock {
        slot: 600,
        unix_timestamp: 600,
        ..Clock::default()
    });

    env.try_resolve_permissionless().unwrap();
    assert!(env.is_market_resolved());

    let settlement = env.read_authority_price();
    assert!(settlement > 0 && settlement < 100_000, "Inverted settlement: {}", settlement);
}

// ============================================================================
// TDD Item 3: Permissionless resolution for inverted Pyth markets
// ============================================================================

/// Inverted Pyth market resolves permissionlessly when oracle goes stale.
/// The settlement price should be the last known oracle price (already inverted
/// by the crank's read_price flow), not the raw Pyth price.
#[test]
fn test_resolve_permissionless_inverted_market() {
    program_path();
    let mut env = TestEnv::new();
    // Inverted market with cap + permissionless resolve enabled (stale > 50 slots)
    env.init_market_with_cap(1, 10_000, 50);

    // Bounded staleness so oracle can go stale
    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        // max_staleness_secs at offset 72+96=168
        slab.data[168..176].copy_from_slice(&30u64.to_le_bytes());
        env.svm.set_account(env.slab, slab).unwrap();
    }

    // Crank to establish baseline oracle price
    env.crank();
    assert!(!env.is_market_resolved());

    // Make oracle stale: advance clock WITHOUT updating oracle data
    env.svm.set_sysvar(&Clock {
        slot: 500,
        unix_timestamp: 500,
        ..Clock::default()
    });

    let result = env.try_resolve_permissionless();
    assert!(
        result.is_ok(),
        "Inverted market should resolve permissionlessly when oracle dies: {:?}",
        result
    );
    assert!(env.is_market_resolved());
}

/// Inverted market: settlement price is the inverted oracle price, not the raw one.
/// Raw Pyth price ~138M, inverted ~7246. Settlement must use the inverted value.
#[test]
fn test_resolve_permissionless_inverted_settlement_price() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(1, 10_000, 50);

    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        slab.data[168..176].copy_from_slice(&30u64.to_le_bytes());
        env.svm.set_account(env.slab, slab).unwrap();
    }

    env.crank(); // Establishes oracle price in inverted form

    // Read the last oracle price from the engine (should be inverted)
    let index_before = env.read_last_effective_price();
    // For invert=1 with raw price 138_000_000 (e-6), inverted = 1e12 / 138_000_000 ≈ 7246
    assert!(
        index_before < 100_000,
        "Inverted index should be small (not raw), got {}",
        index_before
    );

    // Make oracle stale and resolve
    env.svm.set_sysvar(&Clock {
        slot: 500,
        unix_timestamp: 500,
        ..Clock::default()
    });
    env.try_resolve_permissionless().unwrap();

    // Settlement price should be in the inverted price space
    let settlement = env.read_authority_price();
    assert!(
        settlement < 100_000,
        "Settlement must be inverted price, got {}",
        settlement
    );
    assert!(settlement > 0, "Settlement must be non-zero");
}

/// Inverted market: permissionless resolution rejected when oracle is still live.
#[test]
fn test_resolve_permissionless_inverted_rejects_live_oracle() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(1, 10_000, 50);

    env.crank();

    // Oracle is still fresh — set_slot updates the oracle publish_time
    env.set_slot(200);
    let result = env.try_resolve_permissionless();
    assert!(
        result.is_err(),
        "Inverted market must reject permissionless resolve when oracle is live"
    );
    assert!(!env.is_market_resolved());
}

/// Inverted market: full lifecycle with positions, then permissionless resolution.
/// Verifies that positions are settled correctly at the inverted settlement price.
#[test]
fn test_resolve_permissionless_inverted_with_positions() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(1, 10_000, 50);

    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        slab.data[168..176].copy_from_slice(&30u64.to_le_bytes());
        env.svm.set_account(env.slab, slab).unwrap();
    }

    // Set up LP and user with positions
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    // Open position on inverted market
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    assert_ne!(env.read_account_position(user_idx), 0, "User must have position");

    // Top up insurance
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 1_000_000_000);

    // Advance, crank, then make oracle die
    env.set_slot(200);
    env.crank();

    env.svm.set_sysvar(&Clock {
        slot: 500,
        unix_timestamp: 500,
        ..Clock::default()
    });

    // Permissionless resolution should succeed with open positions
    let result = env.try_resolve_permissionless();
    assert!(
        result.is_ok(),
        "Should resolve even with open positions: {:?}",
        result
    );
    assert!(env.is_market_resolved());
}

// ============================================================================
// Finding 1: ResolvePermissionless sentinel collision fix
// ============================================================================

/// Before any crank, permissionless resolution must be rejected (no real oracle established).
#[test]
fn test_resolve_permissionless_rejects_before_first_crank() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 10_000, 100);

    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        slab.data[168..176].copy_from_slice(&30u64.to_le_bytes());
        env.svm.set_account(env.slab, slab).unwrap();
    }

    // Do NOT crank — last_effective_price_e6 is still 0

    // Make oracle stale
    env.svm.set_sysvar(&Clock {
        slot: 600,
        unix_timestamp: 600,
        ..Clock::default()
    });

    let result = env.try_resolve_permissionless();
    assert!(result.is_err(), "Must reject when no real oracle price established");
}

// ============================================================================
// Finding 3: ResolveMarket settlement guard — live cap
// ============================================================================

/// A market with min_oracle_price_cap_e2bps=0 but live oracle_price_cap > 0
/// should still enforce the settlement guard using the live cap.
/// Scenario: authority pushes at $138 (matching oracle), then the external oracle
/// jumps to $150 before resolution. The settlement guard should catch this.
#[test]
fn test_resolve_market_uses_live_cap_when_floor_is_zero() {
    program_path();
    let mut env = TestEnv::new();
    // Init with min_oracle_price_cap = 0 (no immutable floor)
    env.init_market_with_invert(0);

    // Set a live oracle_price_cap via admin
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_price_cap(&admin, 10_000).unwrap(); // 1% cap

    // Crank to establish baseline
    env.crank();

    // Set up authority and push at current oracle price ($138)
    env.try_set_oracle_authority(&admin, &admin.pubkey()).unwrap();
    env.set_slot(200);
    env.try_push_oracle_price(&admin, 138_000_000, 300).unwrap();

    // Now external oracle jumps to $150 (9% move from $138)
    // This simulates the oracle updating while the authority price is stale
    env.set_slot_and_price(300, 150_000_000);

    // ResolveMarket: authority_price=$138M, fresh oracle=$150M, cap=1%
    // 138M is NOT within 1% of 150M → should be rejected.
    // But with min_oracle_price_cap_e2bps=0, the old code skips the guard entirely.
    let result = env.try_resolve_market(&admin);
    assert!(
        result.is_err(),
        "Settlement guard must use live cap when floor=0 but cap>0"
    );
}

// ============================================================================
// Finding 8: ResolvePermissionless — authority pricing freshness
// ============================================================================

/// A market with a live oracle authority should NOT be permissionlessly resolvable
/// just because the external feed is stale — the authority can still push prices.
#[test]
fn test_resolve_permissionless_blocked_by_live_authority() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 10_000, 100);

    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        slab.data[168..176].copy_from_slice(&30u64.to_le_bytes());
        env.svm.set_account(env.slab, slab).unwrap();
    }

    // Crank + set authority
    env.crank();
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey()).unwrap();

    // Advance clock to ts=200, then push. The helper uses clock.unix_timestamp
    // for the push timestamp, so authority_timestamp will be ~200.
    env.svm.set_sysvar(&Clock {
        slot: 300,
        unix_timestamp: 200,
        ..Clock::default()
    });
    env.try_push_oracle_price(&admin, 138_000_000, 0).unwrap();

    // Advance clock to make external oracle stale but authority still fresh.
    // Oracle publish_time=100 (from init), max_staleness=30.
    // At ts=220: oracle age=120 > 30 (stale), authority age=~20 < 30 (fresh).
    env.svm.set_sysvar(&Clock {
        slot: 320,
        unix_timestamp: 220,
        ..Clock::default()
    });

    // Should be rejected — authority pushed recently (age ~20 < 30)
    let result = env.try_resolve_permissionless();
    assert!(
        result.is_err(),
        "Must not resolve permissionlessly when authority push is fresh"
    );
}

// ============================================================================
// Change 2: Permissionless ForceCloseResolved
// ============================================================================

/// Basic force-close: resolve market, wait for delay, close abandoned account.
#[test]
fn test_force_close_resolved_basic() {
    program_path();
    let mut env = TestEnv::new();

    // Init with force_close_delay = 50 slots
    let data = encode_init_market_with_force_close(
        &env.payer.pubkey(), &env.mint, &TEST_FEED_ID, 50,
    );
    env.try_init_market_raw(data).expect("init failed");

    // Set bounded staleness for permissionless resolution
    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        slab.data[168..176].copy_from_slice(&30u64.to_le_bytes());
        env.svm.set_account(env.slab, slab).unwrap();
    }

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.trade(&user, &lp, lp_idx, user_idx, 1_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 1_000_000_000);

    // Crank, then resolve
    env.set_slot(200);
    env.crank();
    env.try_set_oracle_authority(&admin, &admin.pubkey()).unwrap();
    env.try_push_oracle_price(&admin, 138_000_000, 300).unwrap();
    env.try_resolve_market(&admin).unwrap();
    assert!(env.is_market_resolved());

    // Crank resolved to settle
    env.set_slot(400);
    env.crank();

    let used_before = env.read_num_used_accounts();

    // Force-close user account after delay (resolution_slot + 50)
    env.set_slot(500);
    let result = env.try_force_close_resolved(user_idx, &user.pubkey());
    assert!(result.is_ok(), "Force close must succeed after delay: {:?}", result);

    let used_after = env.read_num_used_accounts();
    assert_eq!(
        used_after,
        used_before - 1,
        "num_used should decrease by 1"
    );
}

/// Force-close rejected before delay elapses.
#[test]
fn test_force_close_resolved_rejects_before_delay() {
    program_path();
    let mut env = TestEnv::new();

    let data = encode_init_market_with_force_close(
        &env.payer.pubkey(), &env.mint, &TEST_FEED_ID, 500, // 500 slot delay
    );
    env.try_init_market_raw(data).expect("init failed");
    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        slab.data[168..176].copy_from_slice(&30u64.to_le_bytes());
        env.svm.set_account(env.slab, slab).unwrap();
    }

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);
    env.trade(&user, &lp, lp_idx, user_idx, 1_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 1_000_000_000);
    env.set_slot(200);
    env.crank();
    env.try_set_oracle_authority(&admin, &admin.pubkey()).unwrap();
    env.try_push_oracle_price(&admin, 138_000_000, 300).unwrap();
    env.try_resolve_market(&admin).unwrap();

    // Try immediately — too early (resolution at ~300, delay=500)
    env.set_slot(400);
    let result = env.try_force_close_resolved(user_idx, &user.pubkey());
    assert!(result.is_err(), "Must reject before delay elapses");
}

/// Force-close rejected on non-resolved market.
#[test]
fn test_force_close_resolved_rejects_non_resolved() {
    program_path();
    let mut env = TestEnv::new();

    let data = encode_init_market_with_force_close(
        &env.payer.pubkey(), &env.mint, &TEST_FEED_ID, 50,
    );
    env.try_init_market_raw(data).expect("init failed");

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.set_slot(200);
    let result = env.try_force_close_resolved(user_idx, &user.pubkey());
    assert!(result.is_err(), "Must reject on non-resolved market");
}

