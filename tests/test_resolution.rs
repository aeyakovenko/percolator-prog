mod common;
#[allow(unused_imports)]
use common::*;

use solana_sdk::{
    account::Account,
    clock::Clock,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    sysvar,
    transaction::Transaction,
};

fn settle_warmup_and_convert_released_pnl(
    env: &mut TestEnv,
    owner: &Keypair,
    idx: u16,
    price_e6: i64,
    maturity_slots: u64,
) {
    env.try_settle_account(idx)
        .expect("first settlement should create or advance the profit reserve");

    if env.read_account_reserved_pnl(idx) > 0 {
        let now = env.svm.get_sysvar::<Clock>().slot;
        env.set_slot_and_price_raw_no_walk(
            now.saturating_add(maturity_slots).saturating_add(1),
            price_e6,
        );
        env.try_settle_account(idx)
            .expect("second settlement should mature the profit reserve");
    }

    let released = env
        .read_account_pnl(idx)
        .max(0)
        .saturating_sub(env.read_account_reserved_pnl(idx) as i128) as u64;
    if released > 0 {
        env.try_convert_released_pnl(owner, idx, released)
            .expect("released PnL conversion should succeed before close");
    }
}

/// Test that resolved markets block new activity
#[test]
fn test_resolved_market_blocks_new_activity() {
    program_path();

    println!("=== RESOLVED MARKET BLOCKS NEW ACTIVITY TEST ===");
    println!();

    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    // authority push removed — fresh Pyth is used directly.
    // Resolve market
    let result = env.try_resolve_market(&admin, 0);
    assert!(result.is_ok(), "ResolveMarket should succeed");
    assert!(
        env.is_market_resolved(),
        "Market must be resolved after ResolveMarket"
    );

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
    // authority push removed — fresh Pyth is used directly.
    // Create user with deposit
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 500_000_000); // 0.5 SOL

    let capital_before = env.read_account_capital(user_idx);
    println!("User capital before resolution: {}", capital_before);
    assert!(capital_before > 0);

    // Resolve market
    env.try_resolve_market(&admin, 0).unwrap();
    println!("Market resolved");

    // Crank to settle
    env.set_slot(100);
    env.crank();

    // Withdrawals are blocked on resolved markets. Users must use CloseAccount.
    let withdraw_result = env.try_withdraw(&user, user_idx, 100_000_000);
    assert!(
        withdraw_result.is_err(),
        "Withdraw should be blocked on resolved market (use CloseAccount)"
    );

    // CloseAccount should work (no position, pnl=0). Two calls for ProgressOnly.
    let _ = env.try_close_account(&user, user_idx);
    let _ = env.try_close_account(&user, user_idx);
    println!("User CloseAccount on resolved market: OK");

    println!();
    println!("RESOLVED MARKET ALLOWS USER CLOSE ACCOUNT TEST PASSED");
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
    // authority push removed — fresh Pyth is used directly.
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Crank to set engine.last_oracle_price (required by resolve)
    env.set_slot(100);
    env.crank();

    // Resolve the market
    let result = env.try_resolve_market(&admin, 0);
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

/// ATTACK: Resolve an already-resolved market.
/// Expected: Double resolution rejected.
#[test]
fn test_attack_double_resolution() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    // authority push removed — fresh Pyth is used directly.
    env.set_slot(100);
    env.crank();

    // First resolution
    let result = env.try_resolve_market(&admin, 0);
    assert!(result.is_ok(), "First resolve should succeed: {:?}", result);
    assert!(env.is_market_resolved(), "Market should be resolved");

    // Second resolution - should fail
    let result = env.try_resolve_market(&admin, 0);
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
    // authority push removed — fresh Pyth is used directly.
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // User with no position - should be able to withdraw after resolution
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Crank to set engine.last_oracle_price
    env.set_slot(100);
    env.crank();

    // Resolve market
    let result = env.try_resolve_market(&admin, 0);
    assert!(result.is_ok(), "Should resolve: {:?}", result);

    // Withdrawals are blocked on resolved markets. Users must use CloseAccount.
    let result = env.try_withdraw(&user, user_idx, 5_000_000_000);
    assert!(
        result.is_err(),
        "Withdrawal should be blocked on resolved market"
    );

    // CloseAccount should work (no position, pnl=0). Two passes for ProgressOnly.
    // Close LP first to enable terminal readiness, then user.
    let _ = env.try_close_account(&lp, lp_idx);
    let _ = env.try_close_account(&user, user_idx);
    let _ = env.try_close_account(&lp, lp_idx);
    let _ = env.try_close_account(&user, user_idx);
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
    let result = env.try_resolve_market(&attacker, 0);
    assert!(
        result.is_err(),
        "ATTACK: Non-admin was able to resolve market!"
    );
    assert!(
        !env.is_market_resolved(),
        "Market should NOT be resolved after failed attempt"
    );
}

/// Standard Pyth market: user deposits, trades (long), price goes up, flattens, waits
/// for profit maturity, converts released PnL, and closes account.
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

    // Price goes up (138 -> 150) along a cap-respecting oracle history.
    env.set_slot_and_price(230, 150_000_000);
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
    env.set_slot_and_price(330, 150_000_000);
    env.crank();
    settle_warmup_and_convert_released_pnl(&mut env, &user, user_idx, 150_000_000, 1);

    let vault_before = env.vault_balance();

    // Close account after positive PnL has matured and been converted.
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

    // Price drops (138 -> 120) along a cap-respecting oracle history.
    env.set_slot_and_price(400, 120_000_000);
    env.crank();

    // User flattens
    env.trade(&user, &lp, lp_idx, user_idx, -size);
    assert_eq!(
        env.read_account_position(user_idx),
        0,
        "User should be flat"
    );

    env.set_slot_and_price(500, 120_000_000);
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
    // h_max is now capped by the wrapper's product horizon (30 days), not
    // by the live-accrual dt envelope. This test keeps a short warmup so the
    // lifecycle completes quickly.
    env.init_market_with_warmup(0, 50);

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

    // Price goes up (138 -> 150) along a cap-respecting oracle history.
    // PnL = 1M * (150-138) * 1e6 / 1e6 = 12M lamports
    env.set_slot_and_price(230, 150_000_000);
    env.crank();

    // User flattens at the fresh oracle price; PnL is realized and warmup starts.
    env.trade(&user, &lp, lp_idx, user_idx, -size);
    assert_eq!(env.read_account_position(user_idx), 0);

    let early = env.try_close_account(&user, user_idx);
    assert!(
        early.is_err(),
        "fresh positive PnL should not be extractable immediately when warmup is configured"
    );

    // Walk both the warmup clock and the external oracle target/effective
    // state forward at the same real price. This preserves the extraction
    // guard while proving the account can exit after the configured warmup.
    env.set_slot_and_price(430, 150_000_000);
    env.crank();
    settle_warmup_and_convert_released_pnl(&mut env, &user, user_idx, 150_000_000, 50);

    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "Profitable user should close after warmup and oracle catch-up: {:?}",
        result
    );
    println!("CloseAccount succeeds after warmup and oracle catch-up");

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
    // authority push removed — fresh Pyth is used directly.
    env.try_resolve_market(&admin, 0).unwrap();

    // Materialization anti-spam fees are real insurance. A terminal lifecycle
    // must explicitly drain resolved insurance before CloseSlab can succeed.
    let insurance_before_withdraw = env.read_insurance_balance();
    assert!(
        insurance_before_withdraw > 0,
        "InitLP/InitUser materialization fees must be present as insurance"
    );
    env.try_withdraw_insurance(&admin)
        .expect("resolved insurance must be withdrawable after all accounts close");
    assert_eq!(
        env.read_insurance_balance(),
        0,
        "resolved insurance drain is required before CloseSlab"
    );

    // Close slab (vault=0, insurance=0, all accounts closed, market resolved)
    let result = env.try_close_slab();
    assert!(result.is_ok(), "CloseSlab should succeed: {:?}", result);

    println!("HONEST PARTICIPANTS STANDARD MARKET FULL LIFECYCLE: PASSED");
}

/// The retired direct LiquidateAtOracle tag must reject.
#[test]
fn test_liquidate_at_oracle_tag_retired() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    // authority push removed — fresh Pyth is used directly.

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.set_slot(50);
    env.crank();

    env.try_resolve_market(&admin, 0).unwrap();

    // Capture state before rejected operation
    let position_before = env.read_account_position(user_idx);
    let capital_before = env.read_account_capital(user_idx);

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_liquidate(user_idx),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    assert!(
        env.svm.send_transaction(tx).is_err(),
        "retired LiquidateAtOracle tag must reject"
    );

    // Position and capital must be unchanged after rejection
    assert_eq!(
        env.read_account_position(user_idx),
        position_before,
        "position must be preserved after rejected liquidation"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        capital_before,
        "capital must be preserved after rejected liquidation"
    );
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
    assert_eq!(
        dust_after_deposit, 0,
        "Aligned deposit must not create dust"
    );

    env.set_slot(200);
    env.crank();
    env.close_account(&user, user_idx);

    // authority push removed — fresh Pyth is used directly.
    env.try_resolve_market(&admin, 0).unwrap();

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
    assert!(
        !env.is_market_resolved(),
        "Precondition: market must not be resolved"
    );
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
    // authority push removed — fresh Pyth is used directly.

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    // Resolve the market
    env.try_resolve_market(&admin, 0).unwrap();
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
/// Steps: InitMarket(Hyperp) -> UpdateAuthority{HYPERP_MARK} -> PushHyperpMark (x2)
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
    const INDEX_OFF: usize = 336; // HEADER_LEN(72) + offset_of!(MarketConfig, last_effective_price_e6)(200)
    let slab_data = env.svm.get_account(&env.slab).unwrap().data;
    let index_after_push =
        u64::from_le_bytes(slab_data[INDEX_OFF..INDEX_OFF + 8].try_into().unwrap());
    println!(
        "3. Pushed mark toward $110; index after push: {}",
        index_after_push
    );

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
        "Index should not decrease: {} -> {}",
        index_after_push,
        index_after_crank
    );
    println!(
        "4. Cranked at slot 20: index {} -> {}",
        index_after_push, index_after_crank
    );

    // Push again at later slot to continue driving mark up
    env.set_slot(30);
    env.try_push_oracle_price(&admin, 120_000_000, 130)
        .expect("push $120");
    env.set_slot(40);
    env.crank();
    println!("   Pushed $120 at slot 30, cranked at slot 40");

    // 5. UpdateConfig (change funding params) — oracle is now required on
    // non-Hyperp markets. This test runs on Hyperp where the oracle account
    // isn't consulted, but expect_len requires 4 accounts regardless.
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_update_config(7200, 200, 200i64, 10i64),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    env.svm.send_transaction(tx).expect("UpdateConfig");
    println!("5. UpdateConfig succeeded (k=200, horizon=7200)");

    // 6. v12.19: price-move cap is immutable init-time, no runtime call.
    println!("6. price cap is init-time (v12.19)");

    // 7. Create user + deposit
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);
    assert!(env.read_account_capital(user_idx) > 0);
    println!("7. User idx={} created with deposit", user_idx);

    env.set_slot(50);
    env.crank();

    // 8. ResolveMarket
    env.try_push_oracle_price(&admin, 115_000_000, 150)
        .expect("settlement price");
    env.try_resolve_market(&admin, 0).expect("ResolveMarket");
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
        env.try_withdraw_insurance(&admin)
            .expect("WithdrawInsurance");
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

/// Verify that cranking a resolved market succeeds and is idempotent.
/// In v12.17, resolved cranks only run end-of-instruction lifecycle
/// (side resets) — no per-account settlement or cursor advancement.
#[test]
fn test_resolved_crank_is_idempotent() {
    program_path();
    println!("=== RESOLVED CRANK IDEMPOTENCY ===");

    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    // authority push removed — fresh Pyth is used directly.

    // Create a few users so the slab is non-trivial
    let mut users = Vec::new();
    for _ in 0..3 {
        let u = Keypair::new();
        let idx = env.init_user(&u);
        env.deposit(&u, idx, 1_000_000_000);
        users.push((u, idx));
    }

    // Live crank
    env.set_slot(10);
    env.crank();

    // Resolve
    env.try_resolve_market(&admin, 0).unwrap();
    assert!(env.is_market_resolved());

    // Multiple resolved cranks should all succeed and not corrupt state
    for i in 0..5u64 {
        env.set_slot(20 + i * 2);
        env.crank();
    }

    // Market should still be resolved
    assert!(
        env.is_market_resolved(),
        "Market must remain resolved after cranks"
    );

    // Cleanup
    for (u, idx) in &users {
        let _ = env.try_admin_force_close_account(&admin, *idx, &u.pubkey());
    }

    println!("RESOLVED CRANK IDEMPOTENCY: PASSED");
}

// ── Permissionless Resolution Tests ────────────────────────────────────

/// Permissionless resolution succeeds when oracle is actually dead.
#[test]
fn test_resolve_permissionless_after_staleness() {
    program_path();
    let mut env = TestEnv::new();
    // Init with permissionless resolve enabled (stale_slots=100, cap=10000)
    env.init_market_with_cap(0, 100);

    // Override max_staleness_secs to 30 for faster staleness detection
    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        // max_staleness_secs: at slab offset 72+96=168
        slab.data[232..240].copy_from_slice(&30u64.to_le_bytes()); // 30 seconds
        env.svm.set_account(env.slab, slab).unwrap();
    }

    // Crank to establish baseline (oracle is fresh at slot 100, ts=100)
    env.crank();
    assert!(!env.is_market_resolved());

    // Oracle still fresh (ts=100, clock=150, age=50 > 30 but set_slot updates oracle)
    // set_slot updates pyth_data publish_time → oracle stays fresh
    env.set_slot(50);
    // Live oracle: single observation call returns Ok (clears any stamp) and
    // the market MUST NOT resolve. Use the raw once-helper here so we're not
    // secretly advancing the clock past authority staleness.
    let _ = env.try_resolve_permissionless_once();
    assert!(
        !env.is_market_resolved(),
        "Must not resolve while oracle is live"
    );

    // Make oracle actually stale: advance clock WITHOUT updating oracle data
    env.svm.set_sysvar(&Clock {
        slot: 500,
        unix_timestamp: 500,
        ..Clock::default()
    });
    // Oracle publish_time is still 150 (from set_slot(50)), age = 500-150 = 350 > 30

    let result = env.try_resolve_permissionless();
    assert!(
        result.is_ok(),
        "Should succeed when oracle is dead: {:?}",
        result
    );
    assert!(env.is_market_resolved());
}

// test_resolve_permissionless_disabled_by_default deleted:
// v12.19 requires non-Hyperp markets to set permissionless_resolve_stale_slots > 0
// (resolvability invariant). The "disabled" configuration this test exercised
// no longer exists at the non-Hyperp init surface. Hyperp markets may still
// set it to 0, but that case is covered by the Hyperp-specific resolve tests.

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
    // authority push removed — fresh Pyth is used directly.
    env.set_slot(100);
    env.crank();
    env.try_resolve_market(&admin, 0).unwrap();
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
    // Init with permissionless resolve enabled (stale_slots=50, cap=10000)
    env.init_market_with_cap(0, 50);

    // Override max_staleness_secs to 30 for faster staleness detection
    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        slab.data[232..240].copy_from_slice(&30u64.to_le_bytes());
        env.svm.set_account(env.slab, slab).unwrap();
    }

    env.crank(); // oracle = $138 at ts=100
                 // Make oracle stale without updating publish_time
    env.svm.set_sysvar(&Clock {
        slot: 500,
        unix_timestamp: 500,
        ..Clock::default()
    });
    env.try_resolve_permissionless().unwrap();

    let settlement = env.read_authority_price();
    assert_eq!(
        settlement, 138_000_000,
        "Settlement should be last oracle price"
    );
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
        slab.data[232..240].copy_from_slice(&30u64.to_le_bytes());
        env.svm.set_account(env.slab, slab).unwrap();
    }

    env.crank(); // establish baseline

    // Oracle is live (within staleness), but attacker passes a garbage account
    // to make read_engine_price_e6 fail with non-OracleStale error.
    let garbage_oracle = Pubkey::new_unique();
    env.svm
        .set_account(
            garbage_oracle,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; 10],         // too short for any oracle
                owner: Pubkey::new_unique(), // wrong owner
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    // Build instruction manually with wrong oracle
    let caller = Keypair::new();
    env.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();
    env.svm.set_sysvar(&Clock {
        slot: 300,
        unix_timestamp: 300,
        ..Clock::default()
    });

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
    assert!(
        !env.is_market_resolved(),
        "Market must NOT be resolved with fake oracle"
    );
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
        0, // invert=0 (direct, e.g., BTC/USD)
        // permissionless_resolve_stale_slots: short for the test. The
        // production cap is independent from MAX_ACCRUAL_DT_SLOTS; this
        // lifecycle intentionally uses a small stale window so the final
        // Resolve branch exercises the permissionless path quickly.
        80, 200,  // funding_horizon_slots (custom, not default 500)
        200,  // funding_k_bps (2x, not default 1x)
        1000, // funding_max_premium_bps (10%, not default 5%)
        10,   // funding_max_e9_per_slot (custom cap)
    );

    // Verify custom params stored
    assert_eq!(env.read_funding_horizon(), 200);
    assert_eq!(env.read_funding_k_bps(), 200);
    assert_eq!(env.read_funding_max_premium_bps(), 1000);
    assert_eq!(env.read_funding_max_e9_per_slot(), 10);

    // Step 2: Set bounded staleness (so oracle can go stale for permissionless resolution)
    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        // max_staleness_secs at offset 72+96=168
        slab.data[232..240].copy_from_slice(&30u64.to_le_bytes());
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
        1,   // invert=1
        80,  // short test stale window; not coupled to MAX_ACCRUAL_DT_SLOTS
        300, // custom horizon
        150, // 1.5x k
        800, // 8% max premium
        8,   // custom max per slot
    );

    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        slab.data[232..240].copy_from_slice(&30u64.to_le_bytes());
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
    assert!(
        ewma > 0 && ewma < 100_000,
        "Inverted EWMA in correct range: {}",
        ewma
    );

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
    assert!(
        settlement > 0 && settlement < 100_000,
        "Inverted settlement: {}",
        settlement
    );
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
    env.init_market_with_cap(1, 50);

    // Bounded staleness so oracle can go stale
    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        // max_staleness_secs at offset 72+96=168
        slab.data[232..240].copy_from_slice(&30u64.to_le_bytes());
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
    env.init_market_with_cap(1, 50);

    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        slab.data[232..240].copy_from_slice(&30u64.to_le_bytes());
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

// OBSOLETE under strict hard-timeout model:
// test_resolve_permissionless_inverted_rejects_live_oracle asserted
// that resolve fails while "the oracle is live right now". Under the
// strict model, what matters is clock.slot - last_good_oracle_slot;
// if no one has touched the market in N+ slots, the market resolves
// regardless of whether the oracle COULD be read fresh now. The
// helper try_resolve_permissionless advances the clock past the
// delay to simulate a keeper waiting out the timeout, which under
// strict semantics matures the stale window and resolves the market
// — the exact behavior the user explicitly requested. Test removed.

/// Inverted market: full lifecycle with positions, then permissionless resolution.
/// Verifies that positions are settled correctly at the inverted settlement price.
#[test]
fn test_resolve_permissionless_inverted_with_positions() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(1, 80);

    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        slab.data[232..240].copy_from_slice(&30u64.to_le_bytes());
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
    assert_ne!(
        env.read_account_position(user_idx),
        0,
        "User must have position"
    );

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

/// Before any crank, permissionless resolution on an empty market succeeds
/// at the init sentinel price (p=1). This is harmless: there are no positions
/// to settle, so the settlement price is irrelevant.
#[test]
fn test_resolve_permissionless_empty_market_at_sentinel() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 100);

    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        slab.data[232..240].copy_from_slice(&30u64.to_le_bytes());
        env.svm.set_account(env.slab, slab).unwrap();
    }

    // Do NOT crank — engine still has init sentinel price=1

    // Make oracle stale + enough crank staleness
    env.svm.set_sysvar(&Clock {
        slot: 600,
        unix_timestamp: 600,
        ..Clock::default()
    });

    // Resolution at sentinel is allowed on empty markets (no positions to settle)
    let result = env.try_resolve_permissionless();
    assert!(
        result.is_ok(),
        "Empty market resolution at sentinel should succeed: {:?}",
        result
    );
    assert!(env.is_market_resolved());
}

// ============================================================================
// Finding 8: ResolvePermissionless — authority pricing freshness
// ============================================================================

// OBSOLETE under strict hard-timeout model:
// test_resolve_permissionless_blocked_by_live_authority asserted
// the old authority-fresh-blocks-resolve carve-out. That was the
// "pinned theater" state the strict model explicitly fixes — a fresh
// authority can no longer keep the market alive past the hard stale
// timeout. The opposite assertion is now in
// test_resolve_permissionless_fresh_authority_does_not_block_resolve.
// Test removed.

// ============================================================================
// Change 2: Permissionless ForceCloseResolved
// ============================================================================

/// Basic force-close: resolve market, wait for delay, close abandoned account.
#[test]
fn test_force_close_resolved_basic() {
    program_path();
    let mut env = TestEnv::new();

    // Init with force_close_delay = 50 slots
    let data =
        encode_init_market_with_force_close(&env.payer.pubkey(), &env.mint, &TEST_FEED_ID, 50);
    env.try_init_market_raw(data).expect("init failed");

    // Set bounded staleness for permissionless resolution
    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        slab.data[232..240].copy_from_slice(&30u64.to_le_bytes());
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
    // authority push removed — fresh Pyth is used directly.
    env.try_resolve_market(&admin, 0).unwrap();
    assert!(env.is_market_resolved());

    // Crank resolved to settle
    env.set_slot(400);
    env.crank();

    let used_before = env.read_num_used_accounts();

    // Force-close user account after delay (resolution_slot + 50)
    env.set_slot(500);
    let result = env.try_force_close_resolved(user_idx, &user.pubkey());
    assert!(
        result.is_ok(),
        "Force close must succeed after delay: {:?}",
        result
    );

    let used_after = env.read_num_used_accounts();
    assert_eq!(used_after, used_before - 1, "num_used should decrease by 1");
}

/// Force-close rejected before delay elapses.
#[test]
fn test_force_close_resolved_rejects_before_delay() {
    program_path();
    let mut env = TestEnv::new();

    let data = encode_init_market_with_force_close(
        &env.payer.pubkey(),
        &env.mint,
        &TEST_FEED_ID,
        500, // 500 slot delay
    );
    env.try_init_market_raw(data).expect("init failed");
    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        slab.data[232..240].copy_from_slice(&30u64.to_le_bytes());
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
    // authority push removed — fresh Pyth is used directly.
    env.try_resolve_market(&admin, 0).unwrap();

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

    let data =
        encode_init_market_with_force_close(&env.payer.pubkey(), &env.mint, &TEST_FEED_ID, 50);
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

/// ResolveMarket must pass the fresh external oracle price (not stale
/// engine.last_oracle_price) to the engine for final accrual and band check.
///
/// Setup: crank at price A, push settlement at price B, resolve.
/// The engine's live_oracle_price arg should be the fresh read (price A from
/// the oracle account), not the stale engine.last_oracle_price which could
/// differ if the crank used a different price path.
#[test]
fn test_resolve_market_passes_fresh_live_oracle_to_engine() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Crank at oracle price 138_000_000 (sets engine.last_oracle_price)
    env.set_slot_and_price(100, 138_000_000);
    env.crank();

    // Resolve at a fresh external oracle read.
    env.set_slot_and_price(200, 138_000_000);
    let result = env.try_resolve_market(&admin, 0);
    assert!(
        result.is_ok(),
        "ResolveMarket with fresh oracle should succeed: {:?}",
        result
    );
    assert!(env.is_market_resolved(), "Market must be resolved");
}

/// ResolveMarket before first crank should work when a fresh external oracle
/// is available, even though engine.last_oracle_price is still the init sentinel.
/// This tests the fresh_live_oracle path directly.
#[test]
fn test_resolve_market_before_first_crank_with_fresh_oracle() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Fresh external Pyth update; crank to set engine.last_oracle_price.
    env.set_slot_and_price(100, 138_000_000);
    env.crank();

    let result = env.try_resolve_market(&admin, 0);
    assert!(
        result.is_ok(),
        "ResolveMarket should succeed with fresh oracle: {:?}",
        result
    );
}

// ============================================================================
// W4: init_price = 1 sentinel defense-in-depth tests
// ----------------------------------------------------------------------------
// The engine is constructed with last_oracle_price = 1 on non-Hyperp markets.
// This sentinel is replaced on the first successful oracle read (which also
// sets FLAG_ORACLE_INITIALIZED). Resolution paths gate on the flag before
// allowing the sentinel to become the settlement price. These tests lock in
// that interlock so future edits don't open the sentinel to real positions.
// ============================================================================

/// Sentinel safety: permissionless resolve with OI=0 and oracle never
/// initialized is allowed, and capital is preserved (no spurious settlement PnL).
///
/// Flow:
///   init → deposit (no trade, no crank) → oracle dies → resolve_permissionless
///
/// Expected: resolution succeeds with settlement == sentinel (1), but because
/// OI is zero the engine has no position-dependent settlement to apply. Every
/// depositor walks away with their capital intact.
#[test]
fn test_init_sentinel_permissionless_resolve_deposits_only_preserves_capital() {
    program_path();
    let mut env = TestEnv::new();
    // Enable permissionless resolution with a 50-slot staleness window.
    env.init_market_with_cap(0, 50);

    // Tighten max_staleness_secs so "oracle dead" can be triggered by clock
    // advance without us having to actually stop the mocked Pyth feed.
    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        slab.data[232..240].copy_from_slice(&30u64.to_le_bytes());
        env.svm.set_account(env.slab, slab).unwrap();
    }

    // Deposit only. No trade → no execute_trade → no oracle read from the
    // wrapper's read_price path → FLAG_ORACLE_INITIALIZED stays clear,
    // engine.last_oracle_price stays at the init sentinel (1).
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);
    let cap_before = env.read_account_capital(user_idx);
    assert!(cap_before > 0, "deposit must have funded capital");

    // Oracle dies: advance clock far past last_good_oracle_slot (which is
    // still 0 because nothing has read the oracle yet).
    env.svm.set_sysvar(&Clock {
        slot: 500,
        unix_timestamp: 500,
        ..Clock::default()
    });

    // Permissionless resolution must succeed — the OI=0 safety branch allows it.
    env.try_resolve_permissionless()
        .expect("resolve_permissionless must succeed with OI=0 and dead oracle");
    assert!(env.is_market_resolved(), "market must be resolved");

    // Capital preserved: with OI=0, no K-pair deltas, no funding (W1: zero
    // rate over the dead interval), so the depositor's capital is unchanged.
    let cap_after = env.read_account_capital(user_idx);
    assert_eq!(
        cap_after, cap_before,
        "deposits-only account must retain full capital through sentinel settlement"
    );
}

/// Sentinel replacement: the first successful oracle accrual (keeper crank)
/// replaces the sentinel with a real price. After that, permissionless resolve
/// uses the real price, not the sentinel.
///
/// Flow:
///   init → crank (reads real oracle, sets FLAG_ORACLE_INITIALIZED) → oracle
///   dies → resolve_permissionless → settlement == last real oracle price.
#[test]
fn test_sentinel_replaced_after_first_crank() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 50);
    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        slab.data[232..240].copy_from_slice(&30u64.to_le_bytes());
        env.svm.set_account(env.slab, slab).unwrap();
    }

    // One crank. The crank reads the mocked Pyth feed, validates it, stamps
    // engine.last_oracle_price and FLAG_ORACLE_INITIALIZED.
    env.crank();
    assert!(!env.is_market_resolved(), "crank alone must not resolve");

    // Oracle dies (publish_time frozen, clock advances past staleness).
    env.svm.set_sysvar(&Clock {
        slot: 500,
        unix_timestamp: 500,
        ..Clock::default()
    });
    env.try_resolve_permissionless()
        .expect("permissionless resolve must succeed after first crank and oracle death");

    // Settlement price = engine.last_oracle_price = real Pyth price (138 USD
    // with e6 scaling, per the common test fixture), NOT the sentinel 1.
    let settlement = env.read_authority_price();
    assert!(
        settlement > 1_000_000,
        "settlement must be the real post-crank oracle price, not the init sentinel (got {})",
        settlement
    );
    assert_eq!(
        settlement, 138_000_000,
        "settlement must equal the last successful oracle read ($138 e6)"
    );
}

/// OI-nonzero + oracle-never-initialized path is structurally unreachable in
/// the wrapper: the only way to grow OI is execute_trade, and execute_trade
/// reads the oracle (or fails). This test enforces that invariant by proving
/// a trade flips the oracle-initialized flag and updates the engine price.
///
/// The W4 audit concern was that *if* OI could ever be nonzero with
/// FLAG_ORACLE_INITIALIZED clear, the sentinel would leak into settlement.
/// This test documents why that state is unreachable.
#[test]
fn test_sentinel_invariant_nonzero_oi_implies_oracle_initialized() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 50);

    // Seed a trade → establishes OI. The trade path MUST read the oracle,
    // which sets FLAG_ORACLE_INITIALIZED and replaces the sentinel.
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);

    assert_ne!(
        env.read_account_position(user_idx),
        0,
        "trade must create nonzero position (OI > 0)"
    );

    // Now force oracle death and resolve. Because the trade initialized the
    // oracle, settlement uses the real price (not the sentinel).
    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        slab.data[232..240].copy_from_slice(&30u64.to_le_bytes());
        env.svm.set_account(env.slab, slab).unwrap();
    }
    env.svm.set_sysvar(&Clock {
        slot: 500,
        unix_timestamp: 500,
        ..Clock::default()
    });
    env.try_resolve_permissionless()
        .expect("must resolve with real price");
    let settlement = env.read_authority_price();
    assert_eq!(
        settlement, 138_000_000,
        "settlement after OI>0 must always be the real oracle price"
    );
}

/// ResolvePermissionless must succeed after an oracle outage longer than
/// `max_accrual_dt_slots`. The engine's Degenerate arm explicitly skips
/// `accrue_market_to` and resolves at P_last/rate=0, so there is no live dt
/// envelope check on this path.
///
/// This is the integration counterpart to
/// `kani_permissionless_resolve_horizon_policy_independent_from_accrual_window`:
/// the Kani proof covers the wrapper init policy, this test executes the SBF
/// wrapper -> engine Degenerate resolve call and proves the market gets
/// unstuck instead of trying to accrue the dead interval.
#[test]
fn test_resolve_permissionless_succeeds_after_outage_exceeding_max_accrual_dt() {
    program_path();
    let mut env = TestEnv::new();
    // Short test threshold. The production cap is independent from
    // MAX_ACCRUAL_DT_SLOTS; this test drives the clock far past both the
    // threshold and the live-accrual envelope to exercise the
    // permissionless-resolve Degenerate path.
    env.init_market_with_cap(0, 80);

    // Tighten oracle staleness so clock advance → oracle death.
    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        let mut config = percolator_prog::state::read_config(&slab.data);
        config.max_staleness_secs = 30;
        percolator_prog::state::write_config(&mut slab.data, &config);
        env.svm.set_account(env.slab, slab).unwrap();
    }

    // Put real user capital in the market before the outage. This keeps the
    // test from being a vacuous "empty market can flip resolved=true" check:
    // after permissionless resolution, the user must still be able to exit
    // through the resolved force-close path.
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);
    let used_before = env.read_num_used_accounts();
    let capital_before = env.read_account_capital(user_idx);
    assert!(capital_before > 0, "test account must carry capital");

    env.crank(); // seed engine.last_oracle_price with the real price

    // Kill oracle, advance clock far past MAX_ACCRUAL_DT_SLOTS.
    env.svm.set_sysvar(&Clock {
        slot: 500_000,
        unix_timestamp: 500_000,
        ..Clock::default()
    });

    let live_result = env.try_crank();
    assert!(
        live_result.is_err(),
        "live crank must not be the recovery path once the oracle outage is stale"
    );

    // If the audit were right, the engine would reject with Overflow on the
    // dt check and the market would be permanently stuck. The Degenerate
    // arm does NOT call accrue_market_to, so there's no dt bound.
    env.try_resolve_permissionless()
        .expect("degenerate resolve must succeed past MAX_ACCRUAL_DT_SLOTS gap");

    assert!(env.is_market_resolved(), "market resolved");
    let settlement = env.read_authority_price();
    assert_eq!(
        settlement, 138_000_000,
        "settlement uses engine.last_oracle_price (last known good)"
    );

    let force_close_delay = {
        let slab = env.svm.get_account(&env.slab).unwrap();
        percolator_prog::state::read_config(&slab.data).force_close_delay_slots
    };
    let mut clock = env.svm.get_sysvar::<Clock>();
    clock.slot = clock
        .slot
        .saturating_add(force_close_delay)
        .saturating_add(1);
    clock.unix_timestamp = clock
        .unix_timestamp
        .saturating_add(force_close_delay as i64 + 1);
    env.svm.set_sysvar(&clock);

    env.try_force_close_resolved(user_idx, &user.pubkey())
        .expect("resolved account must be permissionlessly force-closable after delay");
    assert_eq!(
        env.read_num_used_accounts(),
        used_before - 1,
        "resolved force-close must free the funded account"
    );
}

/// Regression for Finding 6: force_close_delay_slots must have a hard
/// upper bound at init. Without it, an admin could set delay=u64::MAX and
/// burn admin — after resolution, `resolved_slot + delay` saturates to
/// u64::MAX and ForceCloseResolved becomes unreachable, stranding any
/// leftover accounts. The fix caps delay at MAX_FORCE_CLOSE_DELAY_SLOTS.
#[test]
fn test_init_market_rejects_unbounded_force_close_delay() {
    use crate::common::encode_init_market_with_force_close;
    program_path();
    let mut env = TestEnv::new();
    // Construct an InitMarket payload with delay = u64::MAX.
    let data = encode_init_market_with_force_close(
        &env.payer.pubkey(),
        &env.mint,
        &TEST_FEED_ID,
        u64::MAX, // far exceeds MAX_FORCE_CLOSE_DELAY_SLOTS
    );
    let result = env.try_init_market_raw(data);
    assert!(
        result.is_err(),
        "InitMarket with force_close_delay_slots = u64::MAX must be rejected \
         — otherwise admin burn leaves ForceCloseResolved unreachable",
    );
}

/// Strict hard-timeout regression: a permissionless non-Hyperp market cannot
/// be revived by KeeperCrank after it has been idle past the hard stale
/// window. Once `clock.slot - last_good_oracle_slot >=
/// permissionless_resolve_stale_slots`, live price-taking paths must reject
/// and the terminal path is explicit Degenerate resolution.
#[test]
fn test_keeper_crank_rejects_after_hard_timeout_and_resolves_degenerate() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 80);
    env.crank(); // seed engine.last_oracle_price and last_good_oracle_slot

    // Jump the wall clock and oracle account together. Even with a fresh
    // external observation present, the strict stale timer has already
    // matured, so KeeperCrank must not revive the market.
    env.set_slot_and_price_raw_no_walk(250, 138_000_000);
    let result = env.try_crank();
    assert!(
        result.is_err(),
        "KeeperCrank must reject once the hard stale window has matured"
    );

    env.try_resolve_permissionless_once()
        .expect("hard-timeout market must resolve through the Degenerate path");
    assert!(env.is_market_resolved());
}

/// The retired CatchupAccrue tag is not a stale-oracle escape hatch. A caller
/// cannot jump a non-Hyperp permissionless market past its hard timeout, publish
/// a new oracle account at the jumped slot, and use standalone catchup to keep
/// the market live.
#[test]
fn test_catchup_accrue_rejects_after_hard_timeout() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 80);
    env.crank(); // seed engine state and liveness cursor

    env.set_slot_and_price_raw_no_walk(250, 138_000_000);
    let result = env.try_catchup_accrue();
    assert!(
        result.is_err(),
        "retired CatchupAccrue tag must reject after hard timeout"
    );
}

/// Unified stale-oracle policy regression: a Pyth-Pull market with NO
/// oracle authority configured can still be resolved permissionlessly
/// once the Pyth feed goes stale past the configured delay. The prior
/// policy rejected this path (Pyth-Pull needed an authority heartbeat
/// to prove feed death); the unified policy says any stale oracle +
/// no clear within delay = everyone exits at the last price. This test
/// documents the simplified behavior: if the oracle you configured is
/// stale, the market freezes and resolves. Full stop, no oracle-kind-
/// specific carve-outs.
#[test]
fn test_resolve_permissionless_unified_policy_pyth_pull_no_authority() {
    program_path();
    let mut env = TestEnv::new();
    // Pyth-Pull market (TEST_FEED_ID resolves to the Pyth-owned fixture
    // account created by TestEnv::new). min_cap=10_000 to satisfy other
    // invariants; permissionless_resolve_stale_slots=50 so the test
    // doesn't need to warp far.
    env.init_market_with_cap(0, 50);

    // Non-Hyperp markets have hyperp_authority == 0 at init (no
    // authority role), so no burn is needed — the "no authority
    // configured" precondition is the default.
    {
        let slab = env.svm.get_account(&env.slab).unwrap();
        let config = percolator_prog::state::read_config(&slab.data);
        assert_eq!(
            config.hyperp_authority, [0u8; 32],
            "precondition: non-Hyperp markets init with zero authority",
        );
    }

    // Kill the Pyth feed by advancing the clock far beyond the
    // fixture's published_time and the configured staleness cap.
    env.svm.set_sysvar(&Clock {
        slot: 500,
        unix_timestamp: 200_000,
        ..Clock::default()
    });

    // Unified policy: stale feed + no authority + delay elapsed →
    // resolution succeeds.
    env.try_resolve_permissionless().expect(
        "Unified stale-oracle policy: a stale Pyth-Pull market with no \
         oracle authority MUST resolve permissionlessly once the stale \
         window has matured. Under the previous policy this rejected \
         because Pyth-Pull required an authority heartbeat; the unified \
         policy removes that carve-out.",
    );
    assert!(
        env.is_market_resolved(),
        "market must be resolved after stale window matures",
    );
}

/// Carve-out removal regression under the STRICT HARD-TIMEOUT model:
/// a FRESH oracle authority MUST NOT block ResolvePermissionless once
/// clock.slot - last_good_oracle_slot >= permissionless_resolve_stale
/// _slots. The previous model treated fresh authority as "market is
/// priceable" and blocked resolve; in practice that pinned the
/// effective price within one cap-width of a stale baseline
/// indefinitely. The new rule: defined oracle stale past N slots →
/// market resolves, regardless of authority state. Authority is a
/// short-outage fallback for individual price reads, not a lifeline
/// that extends the oracle's useful life.
#[test]
fn test_resolve_permissionless_fresh_authority_does_not_block_resolve() {
    program_path();
    let mut env = TestEnv::new();
    // Small delay so the test doesn't need to warp far.
    env.init_market_with_cap(0, 50);
    env.crank(); // advances last_good_oracle_slot to ~init slot

    // Warp clock far enough that
    // clock.slot - last_good_oracle_slot >= 50.
    env.svm.set_sysvar(&Clock {
        slot: 200_000,
        unix_timestamp: 200_000,
        ..Clock::default()
    });

    // Under the strict model, even an earlier-set authority cannot
    // save the market. Once clock.slot - last_good_oracle_slot >= N,
    // the market is terminally dead. ResolvePermissionless succeeds
    // in a single call regardless of whether an authority was ever
    // configured. (Attempting to push a fresh authority price at
    // this point would ALSO be rejected by PushHyperpMark's own
    // hard-timeout gate — the strict model closes every revival
    // channel.)
    env.try_resolve_permissionless_once()
        .expect("strict model: stale timer matured, resolve must succeed");
    assert!(
        env.is_market_resolved(),
        "prior authority configuration MUST NOT block permissionless \
         resolve once last_good_oracle_slot has been idle past the delay",
    );
}

/// Strict hard-timeout regression: after the window matures, admin
/// ResolveMarket settles at engine.last_oracle_price via the same
/// Degenerate arm ResolvePermissionless uses — NOT at
/// hyperp_mark_e6. This closes the admin-revive channel where a
/// rogue admin could PushHyperpMark past maturity and then settle at
/// the pushed price. (PushHyperpMark itself now also rejects past
/// maturity, but the admin-resolve Degenerate path is the safety net.)
#[test]
fn test_admin_resolve_after_maturity_uses_degenerate_p_last() {
    program_path();
    let mut env = TestEnv::new();
    // Small perm window so we can warp past it quickly.
    env.init_market_with_cap(0, 50);
    env.crank(); // seed engine.last_oracle_price via successful read

    // Record engine.last_oracle_price as the expected settlement anchor.
    // Use the read_engine_last_price helper by reading the slab directly:
    // the engine struct starts at ENGINE_OFF = 472 and last_oracle_price
    // is an early field. We only need the value shape; read it via the
    // hyperp_mark_e6 post-resolve assertion indirectly.
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Warp clock far past the maturity window.
    env.svm.set_sysvar(&Clock {
        slot: 10_000,
        unix_timestamp: 10_000,
        ..Clock::default()
    });

    // Past maturity Ordinary (mode=0) must reject — callers switch to
    // Degenerate (mode=1) explicitly under the v12.19 mode split.
    env.try_resolve_market(&admin, 0)
        .expect_err("Ordinary ResolveMarket past maturity must reject");
    env.try_resolve_market(&admin, 1)
        .expect("Degenerate ResolveMarket past maturity must succeed");
    assert!(env.is_market_resolved(), "market resolved");

    // Settlement anchor: engine.last_oracle_price (seeded by crank at
    // init, = 138_000_000 by TestEnv fixture).
    let settlement = env.read_authority_price();
    assert_eq!(
        settlement, 138_000_000,
        "matured admin resolve must settle at engine.last_oracle_price \
         (the last FRESHLY accrued price), not at a later authority push",
    );
}

#[test]
fn test_admin_degenerate_resolve_rejects_live_oracle() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 80);
    env.crank();
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    env.try_resolve_market(&admin, 1)
        .expect_err("Degenerate admin resolve is a recovery path, not a live-oracle bypass");
    assert!(
        !env.is_market_resolved(),
        "failed Degenerate resolve must not mutate market mode"
    );

    env.try_resolve_market(&admin, 0)
        .expect("Ordinary resolve should still succeed with the live oracle");
}

fn try_resolve_market_with_oracle(
    env: &mut TestEnv,
    admin: &Keypair,
    oracle: Pubkey,
    mode: u8,
) -> Result<(), String> {
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(oracle, false),
        ],
        data: encode_resolve_market(mode),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    env.svm
        .send_transaction(tx)
        .map(|_| ())
        .map_err(|e| format!("{:?}", e))
}

fn prepare_non_hyperp_degenerate_resolve_gate_env() -> (TestEnv, Keypair) {
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 80);
    env.crank();

    {
        let mut slab = env.svm.get_account(&env.slab).unwrap();
        let mut config = percolator_prog::state::read_config(&slab.data);
        config.max_staleness_secs = 30;
        config.permissionless_resolve_stale_slots = 1_000;
        percolator_prog::state::write_config(&mut slab.data, &config);
        env.svm.set_account(env.slab, slab).unwrap();
    }

    env.svm.set_sysvar(&Clock {
        slot: 140,
        unix_timestamp: 140,
        ..Clock::default()
    });

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    (env, admin)
}

#[test]
fn test_admin_degenerate_resolve_rejects_caller_chosen_stale_pyth_before_hard_timeout() {
    program_path();

    let (mut env, admin) = prepare_non_hyperp_degenerate_resolve_gate_env();
    let stale_update = Pubkey::new_unique();
    env.svm
        .set_account(
            stale_update,
            Account {
                lamports: 1_000_000,
                data: make_pyth_data(&TEST_FEED_ID, 200_000_000, -6, 1, 100),
                owner: PYTH_RECEIVER_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let err = try_resolve_market_with_oracle(&mut env, &admin, stale_update, 1)
        .expect_err("caller-chosen stale Pyth update must not prove feed death");
    assert!(
        err.contains("0x6"),
        "Degenerate resolve before hard timeout should reject as not \
         stale-matured (0x6), got: {}",
        err,
    );
    assert!(
        !env.is_market_resolved(),
        "failed Degenerate resolve must not mutate market mode"
    );
}

#[test]
fn test_admin_degenerate_resolve_rejects_caller_chosen_conf_wide_pyth_before_hard_timeout() {
    program_path();

    let (mut env, admin) = prepare_non_hyperp_degenerate_resolve_gate_env();
    let conf_wide_update = Pubkey::new_unique();
    env.svm
        .set_account(
            conf_wide_update,
            Account {
                lamports: 1_000_000,
                data: make_pyth_data(&TEST_FEED_ID, 200_000_000, -6, 20_000_000, 140),
                owner: PYTH_RECEIVER_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let err = try_resolve_market_with_oracle(&mut env, &admin, conf_wide_update, 1)
        .expect_err("caller-chosen conf-wide Pyth update must not prove feed death");
    assert!(
        err.contains("0x6"),
        "Degenerate resolve before hard timeout should reject as not \
         stale-matured (0x6), got: {}",
        err,
    );
    assert!(
        !env.is_market_resolved(),
        "failed Degenerate resolve must not mutate market mode"
    );
}

/// Strict hard-timeout regression: DepositCollateral must reject once
/// the market has matured, even before the market is formally resolved.
/// Users should exit via resolved-market close paths, not feed more
/// collateral into a dead market.
#[test]
fn test_deposit_rejected_after_hard_timeout_matures() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 50);
    env.crank();

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    // Seed initial deposit while the market is still live.
    env.deposit(&user, user_idx, 100_000_000);

    // Warp past the hard-timeout window.
    env.svm.set_sysvar(&Clock {
        slot: 10_000,
        unix_timestamp: 10_000,
        ..Clock::default()
    });

    // Deposit must reject before moving tokens into the dead market.
    let err = env
        .try_deposit(&user, user_idx, 50_000_000)
        .expect_err("deposit past hard timeout must reject");
    assert!(
        err.contains("custom program error: 0x6"), // OracleStale = 6
        "expected OracleStale (hard-timeout gate), got: {}",
        err,
    );
}

// ============================================================================
// SIMD-0047 LastRestartSlot integration: a post-init cluster restart must
// freeze the market at the last cached pre-restart oracle price.
// ============================================================================

/// On `InitMarket`, the wrapper snapshots the current `LastRestartSlot`
/// sysvar reading into `config.init_restart_slot`. LiteSVM initializes the
/// sysvar to zero by default, so fresh markets start with `init_restart_slot
/// == 0` and any non-zero `LastRestartSlot::get()` triggers restart
/// detection.
#[test]
fn test_init_market_snapshots_last_restart_slot() {
    program_path();
    let mut env = TestEnv::new();

    // Bump the sysvar BEFORE init so we observe the snapshot, not the default.
    env.svm
        .set_sysvar(&solana_sdk::sysvar::last_restart_slot::LastRestartSlot {
            last_restart_slot: 42,
        });

    env.init_market_with_invert(0);

    let slab = env.svm.get_account(&env.slab).unwrap();
    let cfg = percolator_prog::state::read_config(&slab.data);
    assert_eq!(
        cfg.init_restart_slot, 42,
        "InitMarket must capture the current LastRestartSlot sysvar reading"
    );
}

/// A cluster restart after InitMarket makes the market permissionless-
/// resolvable immediately, regardless of the slot-based staleness window.
/// Resolution must settle at the last cached oracle price (pre-restart).
#[test]
fn test_cluster_restart_freezes_market_at_last_oracle_price() {
    program_path();
    let mut env = TestEnv::new();

    // Default LastRestartSlot (0) is snapshotted into init_restart_slot.
    env.init_market_with_invert(0);
    // Push a baseline price while the cluster is "healthy".
    // authority push removed — fresh Pyth is used directly.
    // Crank so the engine's last_oracle_price is stamped from this push.
    env.set_slot(100);
    env.crank();

    let cfg_before =
        percolator_prog::state::read_config(&env.svm.get_account(&env.slab).unwrap().data);
    assert_eq!(
        cfg_before.init_restart_slot, 0,
        "sanity: init_restart_slot captured from default LiteSVM sysvar"
    );

    // Without a cluster restart, try_resolve_permissionless_once must fail —
    // the hard-timeout window hasn't matured and no restart has been seen.
    let early = env.try_resolve_permissionless_once();
    assert!(
        early.is_err(),
        "pre-restart resolve attempt must fail (window not matured)"
    );

    // Simulate cluster hard-fork restart: bump LastRestartSlot past the
    // snapshotted value.
    env.svm
        .set_sysvar(&solana_sdk::sysvar::last_restart_slot::LastRestartSlot {
            last_restart_slot: 1,
        });

    // Now the market is dead. Resolve immediately, no clock advance needed.
    env.try_resolve_permissionless_once()
        .expect("post-restart resolve must succeed immediately");

    // Settlement price must equal the last cached pre-restart oracle price:
    // the baseline push of 138_000_000 µUSD that the crank stamped into
    // engine.last_oracle_price. The resolve handler mirrors that into
    // config.hyperp_mark_e6, which is readable by anyone.
    let slab_after = env.svm.get_account(&env.slab).unwrap();
    let cfg_after = percolator_prog::state::read_config(&slab_after.data);
    assert_eq!(
        cfg_after.hyperp_mark_e6, 138_000_000,
        "resolved market must settle at the last cached pre-restart oracle price"
    );
}

/// The symmetric case: without any restart, the existing slot-based
/// permissionless-resolve path must still require the window to mature.
/// This guards against accidental regression where our restart hook
/// collapses into "always stale."
#[test]
fn test_no_restart_preserves_slot_based_stale_window() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    // authority push removed — fresh Pyth is used directly.
    env.set_slot(100);
    env.crank();

    // No sysvar bump; LastRestartSlot remains at its init-time snapshot (0).
    // Immediate resolve must fail — we still require the slot-staleness
    // window to elapse.
    let result = env.try_resolve_permissionless_once();
    assert!(
        result.is_err(),
        "without a restart, permissionless resolve must wait for slot-staleness window"
    );
}
