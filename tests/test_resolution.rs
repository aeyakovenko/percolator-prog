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
    println!("Market resolved");

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
fn test_resolved_crank_sweeps_dust_to_zero() {
    program_path();
    let mut env = TestEnv::new();

    // Initialize with unit_scale=1000 (1000 base tokens = 1 engine unit)
    env.init_market_full(0, 1000, 0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let user = Keypair::new();
    // With unit_scale=1000, need 100*1000=100_000 base for min_initial_deposit
    let user_idx = env.init_user_with_fee(&user, 100_000);

    // Deposit an amount that creates dust: 10_000_500 = 10_000 units + 500 dust
    env.deposit(&user, user_idx, 10_000_500);

    // Verify dust_base is non-zero (500 sub-scale remainder)
    let read_dust_base = |svm: &LiteSVM, slab: &Pubkey| -> u64 {
        let slab_data = svm.get_account(slab).unwrap().data;
        // dust_base is at RESERVED_OFF(48) + 16 = offset 64 in slab
        const DUST_BASE_OFF: usize = 64;
        u64::from_le_bytes(
            slab_data[DUST_BASE_OFF..DUST_BASE_OFF + 8]
                .try_into()
                .unwrap(),
        )
    };
    let dust_after_deposit = read_dust_base(&env.svm, &env.slab);
    assert_eq!(dust_after_deposit, 500, "Deposit should create 500 dust at unit_scale=1000");

    // Advance slot and crank to settle state
    env.set_slot(200);
    env.crank();

    // Close the user account so no positions remain
    env.close_account(&user, user_idx);

    // Dust should still be present after close_account (close returns units, not dust)
    let dust_after_close = read_dust_base(&env.svm, &env.slab);
    assert_eq!(dust_after_close, 500, "Dust should persist after close_account");

    // Set oracle authority and push a settlement price for resolution
    env.try_set_oracle_authority(&admin, &admin.pubkey()).unwrap();
    env.try_push_oracle_price(&admin, 138_000_000, 200).unwrap();

    // Resolve the market
    env.try_resolve_market(&admin).unwrap();

    // Crank the resolved market — should sweep/forgive dust
    env.set_slot(300);
    env.crank();

    // dust_base must be zero after resolved crank
    let dust_after_resolved_crank = read_dust_base(&env.svm, &env.slab);
    assert_eq!(
        dust_after_resolved_crank, 0,
        "Resolved crank must forgive sub-scale dust, leaving dust_base == 0"
    );
}

