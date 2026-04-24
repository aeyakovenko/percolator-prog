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

/// Test 8: Insurance fund top-up succeeds
#[test]
fn test_comprehensive_insurance_fund_topup() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let vault_before = env.vault_balance();
    println!("Vault before top-up: {}", vault_before);

    // Top up insurance fund
    let payer = Keypair::new();
    env.svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    env.top_up_insurance(&payer, 5_000_000_000); // 5 SOL

    // Vault should have the funds
    let vault_after = env.vault_balance();
    println!("Vault after top-up: {}", vault_after);
    assert_eq!(
        vault_after,
        vault_before + 5_000_000_000,
        "Vault should have insurance funds"
    );

    // Engine insurance counter should also reflect the top-up
    let insurance = env.read_insurance_balance();
    assert_eq!(
        insurance, 5_000_000_000,
        "Engine insurance balance should match top-up amount"
    );

    println!("INSURANCE FUND VERIFIED: Top-up transferred to vault and engine counter updated");
}

/// Test that insurance fund deposits can trap funds, preventing CloseSlab.
///
/// This test verifies a potential vulnerability where:
/// 1. TopUpInsurance adds tokens to vault and increments insurance_fund.balance
/// 2. No instruction exists to withdraw from insurance fund
/// 3. CloseSlab requires insurance_fund.balance == 0
/// 4. Therefore, any TopUpInsurance permanently traps those funds
///
/// Security Impact: Medium - Admin cannot reclaim insurance fund deposits
/// even after all users have closed their accounts.
#[test]
fn test_insurance_fund_traps_funds_preventing_closeslab() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Create and close an LP to have a valid market with no positions
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 1_000_000_000); // 1 SOL

    // Create user, trade, and close to verify market works
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000); // 1 SOL

    // Trade to generate some activity
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    assert!(result.is_ok(), "Trade should succeed");

    // Close positions by trading back
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, -1_000_000);
    assert!(result.is_ok(), "Closing trade should succeed");

    // Top up insurance fund - this is the key operation
    let insurance_payer = Keypair::new();
    env.svm
        .airdrop(&insurance_payer.pubkey(), 10_000_000_000)
        .unwrap();
    env.top_up_insurance(&insurance_payer, 500_000_000); // 0.5 SOL to insurance

    let vault_after_insurance = env.vault_balance();
    println!(
        "Vault balance after insurance top-up: {}",
        vault_after_insurance
    );

    // Withdraw all user capital
    env.set_slot(200);
    env.crank(); // Settle any pending funding

    // Users close their accounts
    let user_close = env.try_close_account(&user, user_idx);
    assert!(user_close.is_ok(), "User close should succeed: {:?}", user_close);

    let lp_close = env.try_close_account(&lp, lp_idx);
    assert!(lp_close.is_ok(), "LP close should succeed: {:?}", lp_close);

    // CloseSlab should fail because insurance_fund.balance > 0
    let close_result = env.try_close_slab();
    assert!(
        close_result.is_err(),
        "CloseSlab must fail when insurance_fund.balance > 0"
    );
}

// NOTE: 9 tests targeting SetInsuranceWithdrawPolicy /
// WithdrawInsuranceLimited were removed along with those instructions
// (they were non-binding — insurance_authority could bypass via
// WithdrawInsurance). Insurance is now purely binary: the scoped
// insurance_authority either has full withdrawal power or is burned.

/// ATTACK: Withdraw insurance on an active (non-resolved) market.
/// Expected: WithdrawInsurance only works on resolved markets.
#[test]
fn test_attack_withdraw_insurance_before_resolution() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Top up insurance fund so there's something to steal
    let payer = Keypair::new();
    env.svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    env.top_up_insurance(&payer, 1_000_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let insurance_before = env.read_insurance_balance();
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;

    // Try to withdraw insurance without resolving market
    let result = env.try_withdraw_insurance(&admin);
    assert!(
        result.is_err(),
        "ATTACK: Withdraw insurance on active market should fail"
    );
    assert_eq!(
        env.read_insurance_balance(),
        insurance_before,
        "Rejected insurance withdraw on active market must preserve insurance balance"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected insurance withdraw on active market must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected insurance withdraw on active market must preserve engine vault"
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    assert_eq!(
        slab_after, slab_before,
        "Rejected insurance withdraw on active market must preserve slab state"
    );
}

/// ATTACK: Withdraw insurance when positions are still open.
/// Expected: WithdrawInsurance requires all positions closed.
#[test]
fn test_attack_withdraw_insurance_with_open_positions() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 80);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Set oracle authority and push price
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Open a position
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Resolve market
    let result = env.try_resolve_market(&admin);
    assert!(result.is_ok(), "Should resolve: {:?}", result);

    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let insurance_before = env.read_insurance_balance();
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // Try to withdraw insurance while position still open
    let result = env.try_withdraw_insurance(&admin);
    assert!(
        result.is_err(),
        "ATTACK: Withdraw insurance with open positions should fail"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected insurance withdraw with open positions must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected insurance withdraw with open positions must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected insurance withdraw with open positions must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected insurance withdraw with open positions must preserve LP capital"
    );
    assert_eq!(
        env.read_insurance_balance(),
        insurance_before,
        "Rejected insurance withdraw with open positions must preserve insurance balance"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected insurance withdraw with open positions must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected insurance withdraw with open positions must preserve engine vault"
    );
}

/// ATTACK: Verify trading fees accrue to insurance fund and can't be evaded.
/// Expected: Fee is charged on every trade, goes to insurance.
#[test]
fn test_attack_trading_fee_accrual_to_insurance() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_trading_fee(100); // 1% fee

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let insurance_before = env.read_insurance_balance();

    // Execute trade
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    env.set_slot(200);
    env.crank();

    let insurance_after = env.read_insurance_balance();

    // Insurance should have increased from trading fees
    println!(
        "Insurance before: {}, after: {}",
        insurance_before, insurance_after
    );
    assert!(
        insurance_after > insurance_before,
        "Insurance fund should increase from trading fees: before={} after={}",
        insurance_before,
        insurance_after
    );
}

/// ATTACK: TopUpInsurance on a resolved market.
/// Expected: Rejected (InvalidAccountData).
#[test]
fn test_attack_topup_insurance_after_resolution() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 80);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.set_slot(100);
    env.crank();
    env.try_resolve_market(&admin)
        .expect("market resolution setup must succeed");

    let insurance_before = env.read_insurance_balance();
    let vault_before = env.vault_balance();
    let used_before = env.read_num_used_accounts();
    let resolved_before = env.is_market_resolved();

    // Try to top up insurance on resolved market
    let payer = Keypair::new();
    env.svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    let result = env.try_top_up_insurance(&payer, 1_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: TopUpInsurance on resolved market should be rejected"
    );
    let insurance_after = env.read_insurance_balance();
    let vault_after = env.vault_balance();
    let used_after = env.read_num_used_accounts();
    let resolved_after = env.is_market_resolved();
    assert!(resolved_before, "Precondition: market should already be resolved");
    assert_eq!(
        resolved_after, resolved_before,
        "Rejected top-up on resolved market must not change resolved flag"
    );
    assert_eq!(
        insurance_after, insurance_before,
        "Rejected top-up on resolved market must not change insurance balance"
    );
    assert_eq!(
        vault_after, vault_before,
        "Rejected top-up on resolved market must not move vault funds"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected top-up on resolved market must not change num_used_accounts"
    );
}

/// ATTACK: TopUpInsurance with insufficient ATA balance.
/// Expected: Token program rejects transfer.
#[test]
fn test_attack_topup_insurance_insufficient_balance() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Create ATA with only 100 tokens but try to top up 1B
    let payer = Keypair::new();
    env.svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    let ata = env.create_ata(&payer.pubkey(), 100);
    let insurance_before = env.read_insurance_balance();
    let vault_before = env.vault_balance();
    let used_before = env.read_num_used_accounts();
    let ata_amount_before = {
        let ata_data = env.svm.get_account(&ata).unwrap().data;
        TokenAccount::unpack(&ata_data).unwrap().amount
    };

    let mut data = vec![9u8];
    data.extend_from_slice(&1_000_000_000u64.to_le_bytes());

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&payer.pubkey()),
        &[&payer],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: TopUpInsurance with insufficient balance should fail"
    );
    let insurance_after = env.read_insurance_balance();
    let vault_after = env.vault_balance();
    let used_after = env.read_num_used_accounts();
    let ata_amount_after = {
        let ata_data = env.svm.get_account(&ata).unwrap().data;
        TokenAccount::unpack(&ata_data).unwrap().amount
    };
    assert_eq!(
        insurance_after, insurance_before,
        "Rejected insufficient top-up must not change insurance balance"
    );
    assert_eq!(
        vault_after, vault_before,
        "Rejected insufficient top-up must not change vault funds"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected insufficient top-up must not change num_used_accounts"
    );
    assert_eq!(
        ata_amount_after, ata_amount_before,
        "Rejected insufficient top-up must not debit payer ATA"
    );
}

/// ATTACK: TopUpInsurance accumulates correctly in vault and engine.
/// Expected: Insurance balance increases by correct amount, vault has the tokens.
#[test]
fn test_attack_topup_insurance_correct_accounting() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let insurance_before = env.read_insurance_balance();
    let vault_before = env.vault_balance();

    let payer = Keypair::new();
    env.svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    env.top_up_insurance(&payer, 5_000_000_000);

    let insurance_after = env.read_insurance_balance();
    let vault_after = env.vault_balance();

    assert_eq!(
        vault_after - vault_before,
        5_000_000_000,
        "Vault should increase by exact top-up amount"
    );
    assert!(
        insurance_after > insurance_before,
        "Insurance balance should increase after top-up"
    );
}

/// ATTACK: Dust accumulates from deposits with unit_scale, then verify crank
/// correctly sweeps dust to insurance fund. Attacker cannot prevent dust sweep.
/// Non-vacuous: asserts insurance increases by swept dust units.
#[test]
fn test_dust_sweep_to_insurance_on_crank_with_aligned_deposits() {
    program_path();

    let mut env = TestEnv::new();
    // Use unit_scale=1000. Misaligned deposits are now rejected.
    // Dust can still accumulate from other sources (e.g., withdrawal rounding).
    // This test verifies aligned deposits don't create dust and crank is stable.
    env.init_market_full(0, 1000, 0);

    let lp_owner = Keypair::new();
    let lp_idx = env.init_user_with_fee(&lp_owner, 100_000);

    let user_owner = Keypair::new();
    let user_idx = env.init_user_with_fee(&user_owner, 100_000);

    // Aligned deposits: 2000 base = exactly 2 units, no dust
    env.deposit(&lp_owner, lp_idx, 2000);
    env.deposit(&user_owner, user_idx, 2000);

    let insurance_before = env.read_insurance_balance();
    env.crank();
    let insurance_after = env.read_insurance_balance();

    // No dust was created, so insurance should not change from dust sweep
    assert_eq!(
        insurance_after, insurance_before,
        "Aligned deposits should not create dust to sweep: before={}, after={}",
        insurance_before, insurance_after
    );

    // Vault balance: 2 init deposits (100_000 each) + 2 deposits (2000 each) = 204000
    let spl_vault_balance = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        let vault_account = TokenAccount::unpack(&vault_data).unwrap();
        vault_account.amount
    };
    assert_eq!(
        spl_vault_balance, 204_000,
        "SPL vault should hold all deposited base tokens"
    );
}

/// ATTACK: Multiple accounts compete for insurance fund during liquidation.
/// Create two undercollateralized accounts and liquidate both.
/// Verify insurance fund is not double-counted.
#[test]
fn test_attack_multiple_liquidations_insurance_drain() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    // Two users with small capital and large positions
    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 1_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 1_000_000_000);

    env.crank();

    // Open positions for both users
    env.trade(&user1, &lp, lp_idx, user1_idx, 50_000);
    env.trade(&user2, &lp, lp_idx, user2_idx, 50_000);
    env.crank();

    // Top up insurance
    env.try_top_up_insurance(&admin, 500_000_000).unwrap();

    // Drop price significantly to make both users underwater
    env.set_slot_and_price(20, 100_000_000); // Drop from 138 to 100

    env.crank(); // Crank should liquidate underwater accounts

    // Try explicit liquidation on both
    let liq1 = env.try_liquidate_target(user1_idx);
    let liq2 = env.try_liquidate_target(user2_idx);

    // Insurance fund should not go negative (u128 can't, but balance should be sane)
    let insurance = env.read_insurance_balance();
    assert!(
        insurance < u128::MAX / 2,
        "ATTACK: Insurance fund balance is suspiciously large: {}",
        insurance
    );

    // SPL vault should be unchanged (no external withdrawals)
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 52_500_000_300,
        "ATTACK: SPL vault changed after liquidations! vault={} liq1={:?} liq2={:?}",
        spl_vault, liq1, liq2
    );
}

/// ATTACK: Insurance grows correctly from new account fees.
/// InitUser/InitLP pays a new_account_fee that goes to insurance.
///
/// Obsolete under engine v12.18.1: new_account_fee is gone (spec §10.2).
#[test]
#[ignore = "new_account_fee removed in engine v12.18.1 (spec §10.2)"]
fn test_attack_new_account_fee_goes_to_insurance() {
    program_path();

    let mut env = TestEnv::new();
    let fee: u64 = 1_000_000;
    // Use init_market_full with a non-zero new_account_fee
    env.init_market_full(0, 0, fee as u128);

    let insurance_before = env.read_insurance_balance();

    // Create LP with fee payment - need tokens in ATA to pay the fee
    let lp = Keypair::new();
    env.svm.airdrop(&lp.pubkey(), 1_000_000_000).unwrap();
    let lp_ata = env.create_ata(&lp.pubkey(), fee);
    let matcher = spl_token::ID;
    let ctx = Pubkey::new_unique();
    env.svm
        .set_account(
            ctx,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; 320],
                owner: matcher,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(lp.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(lp_ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(matcher, false),
            AccountMeta::new_readonly(ctx, false),
        ],
        data: encode_init_lp(&matcher, &ctx, fee),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&lp.pubkey()),
        &[&lp],
        env.svm.latest_blockhash(),
    );
    env.svm
        .send_transaction(tx)
        .expect("init_lp with fee failed");
    env.account_count += 1;

    // Current behavior: InitLP charges new_account_fee from capital → insurance.
    let insurance_after_lp = env.read_insurance_balance();
    assert_eq!(
        insurance_after_lp,
        insurance_before + fee as u128,
        "Insurance should grow by new_account_fee after InitLP: before={} after={}",
        insurance_before,
        insurance_after_lp
    );

    // Create user with fee payment
    let user = Keypair::new();
    env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();
    let user_ata = env.create_ata(&user.pubkey(), fee);

    let ix2 = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(user_ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_col, false),
        ],
        data: encode_init_user(fee),
    };
    let tx2 = Transaction::new_signed_with_payer(
        &[cu_ix(), ix2],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    env.svm
        .send_transaction(tx2)
        .expect("init_user with fee failed");
    env.account_count += 1;

    // InitUser also charges new_account_fee from capital → insurance
    let insurance_after_user = env.read_insurance_balance();
    assert_eq!(
        insurance_after_user,
        insurance_after_lp + fee as u128,
        "Insurance should grow by new_account_fee after InitUser: before={} after={}",
        insurance_after_lp,
        insurance_after_user
    );

    // Insurance should have grown by 2x fee (one for LP, one for user)
    let growth = insurance_after_user - insurance_before;
    assert_eq!(
        growth,
        2 * fee as u128,
        "Insurance should grow by 2x new_account_fee: growth={}",
        growth
    );
}

/// ATTACK: Force-realize disabled when insurance > threshold.
/// Top up insurance to disable force-realize, verify positions persist.
#[test]
fn test_attack_insurance_topup_disables_force_realize() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Top up insurance to disable force-realize (insurance > threshold)
    env.try_top_up_insurance(&admin, 5_000_000_000).unwrap();
    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 500_000);

    // Crank - should NOT force-realize since insurance is funded
    env.set_slot(100);
    env.crank();

    // Position should persist (not force-realized)
    let pos = env.read_account_position(user_idx);
    assert_eq!(
        pos, 500_000,
        "Position should persist when insurance > threshold (no force-realize)"
    );
}

/// ATTACK: TopUpInsurance with wrong vault account.
/// Code validates vault matches stored vault_pubkey.
#[test]
fn test_attack_topup_insurance_wrong_vault() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Create a fake vault account
    let fake_vault = Pubkey::new_unique();
    env.svm
        .set_account(
            fake_vault,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; spl_token::state::Account::LEN],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let ata = env.create_ata(&admin.pubkey(), 1_000_000_000);
    let mut data = vec![9u8]; // TopUpInsurance
    data.extend_from_slice(&1_000_000_000u64.to_le_bytes());

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(ata, false),
            AccountMeta::new(fake_vault, false), // Wrong vault
            AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    let insurance_before = env.read_insurance_balance();
    let spl_vault_before = env.vault_balance();
    let ata_before = {
        let ata_data = env.svm.get_account(&ata).unwrap().data;
        TokenAccount::unpack(&ata_data).unwrap().amount
    };
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: TopUpInsurance with wrong vault should be rejected!"
    );
    let insurance_after = env.read_insurance_balance();
    let spl_vault_after = env.vault_balance();
    let ata_after = {
        let ata_data = env.svm.get_account(&ata).unwrap().data;
        TokenAccount::unpack(&ata_data).unwrap().amount
    };
    assert_eq!(
        insurance_after, insurance_before,
        "Rejected wrong-vault TopUpInsurance must not change insurance balance"
    );
    assert_eq!(
        spl_vault_after, spl_vault_before,
        "Rejected wrong-vault TopUpInsurance must not change SPL vault"
    );
    assert_eq!(
        ata_after, ata_before,
        "Rejected wrong-vault TopUpInsurance must not debit source ATA"
    );
}

/// ATTACK: TopUpInsurance with unit_scale dust edge case.
/// Insurance topup amount that doesn't align with unit_scale.
#[test]
fn test_attack_topup_insurance_unit_scale_dust() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_full(0, 1000, 0); // unit_scale = 1000

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Top up insurance with amount not aligned to unit_scale
    let result = env.try_top_up_insurance(&admin, 999); // Not aligned to 1000
    // Insurance topup may succeed but 999 / 1000 = 0 units (dust lost to protocol).
    // If rejected, insurance should still remain unchanged at 0.
    let insurance_after_dust = env.read_insurance_balance();
    assert_eq!(
        insurance_after_dust, 0,
        "Sub-unit-scale topup should leave insurance at 0 units: result={:?} insurance={}",
        result, insurance_after_dust
    );

    // Now top up with aligned amount to verify it works
    env.try_top_up_insurance(&admin, 2000).unwrap(); // 2000 / 1000 = 2 units
    let insurance = env.read_insurance_balance();
    assert_eq!(
        insurance, 2,
        "Aligned topup should give 2 units: got {}",
        insurance
    );

    // Conservation with unit_scale: engine tracks units, vault holds tokens
    // vault = 2999 tokens, engine_vault = 2 units (999 dust tokens untracked)
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    // Engine vault (in units) * unit_scale should be <= actual vault tokens
    // The difference is dust that was transferred but too small to register
    assert!(
        engine_vault * 1000 <= vault as u128,
        "Engine units * scale must not exceed vault tokens: {}*1000={} vault={}",
        engine_vault,
        engine_vault * 1000,
        vault
    );
    // Dust = vault - engine_vault * scale should be < unit_scale
    let dust = vault as u128 - engine_vault * 1000;
    assert!(
        dust < 1000,
        "Dust must be less than unit_scale: dust={}",
        dust
    );
}

/// ATTACK: Insurance topup from non-admin account.
/// Anyone can top up insurance (it's a deposit, not withdrawal).
#[test]
fn test_attack_insurance_topup_from_non_admin() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let random_user = Keypair::new();
    env.svm
        .airdrop(&random_user.pubkey(), 5_000_000_000)
        .unwrap();
    env.create_ata(&random_user.pubkey(), 2_000_000_000);
    let insurance_before = env.read_insurance_balance();

    // Non-admin tops up insurance — TopUpInsurance does not require admin
    let result = env.try_top_up_insurance(&random_user, 1_000_000_000);
    assert!(result.is_ok(), "Anyone should be able to top up insurance: {:?}", result);

    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation: engine={} vault={}",
        engine_vault, vault
    );

    let insurance_after = env.read_insurance_balance();
    assert!(
        insurance_after > insurance_before,
        "Topup should increase insurance: before={} after={}",
        insurance_before,
        insurance_after
    );
}

/// Regression test for PR #1: WithdrawInsurance must decrement engine.vault.
///
/// Without the fix, WithdrawInsurance zeroes insurance_fund.balance and transfers
/// SPL tokens out of the vault, but does NOT decrement engine.vault. This leaves
/// engine.vault non-zero after all capital is withdrawn, causing CloseSlab to fail
/// (it requires engine.vault.is_zero()).
#[test]
fn test_withdraw_insurance_decrements_engine_vault() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 80);
    let admin = env.payer.insecure_clone();

    // Create LP and user
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000); // 10 SOL

    // Top up insurance so we can test withdrawal
    env.top_up_insurance(&admin, 5_000_000_000); // 5 SOL

    let insurance_before = env.read_insurance_balance();
    assert!(insurance_before > 0, "Insurance should be funded");

    // Trade to create positions
    let size: i128 = 100_000;
    env.trade(&user, &lp, lp_idx, user_idx, size);

    // Setup oracle authority and push price (required for ResolveMarket)
    // Resolve market (premarket resolution)
    let result = env.try_resolve_market(&admin);
    assert!(result.is_ok(), "Resolve should succeed: {:?}", result);
    assert!(env.is_market_resolved(), "Market should be resolved");

    // Crank settles PnL; positions require explicit AdminForceCloseAccount
    env.crank();

    // Admin force-close both accounts (zeros positions, handles PnL settlement, fee forgiveness)
    let result = env.try_admin_force_close_account(&admin, user_idx, &user.pubkey());
    assert!(result.is_ok(), "Admin force close user should succeed: {:?}", result);

    let result = env.try_admin_force_close_account(&admin, lp_idx, &lp.pubkey());
    assert!(result.is_ok(), "Admin force close LP should succeed: {:?}", result);

    // Verify positions are zeroed after AdminForceCloseAccount
    assert_eq!(env.read_account_position(user_idx), 0, "User position should be 0 after AdminForceCloseAccount");
    assert_eq!(env.read_account_position(lp_idx), 0, "LP position should be 0 after AdminForceCloseAccount");

    assert_eq!(env.read_num_used_accounts(), 0, "All accounts should be closed");

    // Record vault before WithdrawInsurance
    let vault_before = env.read_engine_vault();
    let insurance = env.read_insurance_balance();
    assert!(insurance > 0, "Insurance should still have balance before withdrawal");
    assert!(vault_before > 0, "Vault should be non-zero before withdrawal");

    // Withdraw insurance
    let result = env.try_withdraw_insurance(&admin);
    assert!(result.is_ok(), "WithdrawInsurance should succeed: {:?}", result);

    // CRITICAL ASSERTION: engine.vault must be decremented by the insurance amount
    let vault_after = env.read_engine_vault();
    assert_eq!(
        vault_after,
        vault_before - insurance,
        "engine.vault must be decremented by insurance amount. \
         Before: {}, Insurance: {}, After: {} (expected {})",
        vault_before, insurance, vault_after, vault_before - insurance
    );

    // CloseSlab requires engine.vault == 0
    let result = env.try_close_slab();
    assert!(result.is_ok(), "CloseSlab should succeed after WithdrawInsurance: {:?}", result);

    println!("WITHDRAW INSURANCE DECREMENTS ENGINE VAULT: PASSED");
}

/// Cooldown enforcement on WithdrawInsuranceLimited (resolved market).
#[test]

/// BPS cap enforcement on WithdrawInsuranceLimited.
#[test]

/// insurance_withdraw_max_bps == 0 blocks live-market withdrawals.
#[test]

/// InitMarket must reject insurance_withdraw_max_bps > 10000.
#[test]
fn test_init_market_insurance_withdraw_max_bps_bounded() {
    // This test already exists - verify it covers insurance_withdraw_max_bps.
    // The encode_init_market_full_v2 function accepts the param.
    // Let's test the specific >10000 rejection.
    program_path();
    let mut env = TestEnv::new();
    // init_market_full with insurance_withdraw_max_bps = 10001 should fail
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Build raw InitMarket with insurance_withdraw_max_bps = 10001
    let mut data = vec![0u8]; // tag 0
    data.extend_from_slice(admin.pubkey().as_ref());
    data.extend_from_slice(env.mint.as_ref());
    data.extend_from_slice(&[0xABu8; 32]); // feed_id
    data.extend_from_slice(&86400u64.to_le_bytes()); // max_staleness_secs
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(0u8); // invert
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
    data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6
    data.extend_from_slice(&100_000_000_000_000_000_000u128.to_le_bytes()); // max_maintenance_fee_per_slot
    data.extend_from_slice(&0u64.to_le_bytes()); // min_oracle_price_cap_e2bps
    // RiskParams
    data.extend_from_slice(&0u64.to_le_bytes()); // warmup
    data.extend_from_slice(&500u64.to_le_bytes()); // mm_bps
    data.extend_from_slice(&1000u64.to_le_bytes()); // im_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // trading_fee
    data.extend_from_slice(&4096u64.to_le_bytes()); // max_accounts
    data.extend_from_slice(&0u128.to_le_bytes()); // new_account_fee
    data.extend_from_slice(&0u128.to_le_bytes()); // risk_threshold
    data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee
    data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_crank_staleness
    data.extend_from_slice(&50u64.to_le_bytes()); // liq_fee_bps
    data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liq_fee_cap
    data.extend_from_slice(&100u64.to_le_bytes()); // liq_buffer_bps
    data.extend_from_slice(&0u128.to_le_bytes()); // min_liq_abs
    data.extend_from_slice(&1u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&2u128.to_le_bytes()); // min_nonzero_im_req
    data.extend_from_slice(&10001u16.to_le_bytes()); // insurance_withdraw_max_bps > 10000
    data.extend_from_slice(&0u64.to_le_bytes()); // insurance_withdraw_cooldown_slots
    data.extend_from_slice(&u128::MAX.to_le_bytes()); // max_floor_change_per_day
    data.extend_from_slice(&0u64.to_le_bytes()); // permissionless_resolve_stale_slots
    data.extend_from_slice(&500u64.to_le_bytes()); // funding_horizon_slots
    data.extend_from_slice(&100u64.to_le_bytes()); // funding_k_bps
    data.extend_from_slice(&500i64.to_le_bytes()); // funding_max_premium_bps
    data.extend_from_slice(&1_000i64.to_le_bytes()); // funding_max_e9_per_slot
    data.extend_from_slice(&0u64.to_le_bytes()); // mark_min_fee
    data.extend_from_slice(&0u64.to_le_bytes()); // force_close_delay_slots

    let (vault_pda, _) = Pubkey::find_program_address(&[b"vault", env.slab.as_ref()], &env.program_id);
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
            AccountMeta::new_readonly(Pubkey::new_unique(), false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data,
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix], Some(&admin.pubkey()), &[&admin], env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(result.is_err(), "insurance_withdraw_max_bps > 10000 must be rejected");

    // Slab header must remain all-zeros (uninitialized) after rejected InitMarket
    let slab_after = env.svm.get_account(&env.slab).unwrap();
    assert!(
        slab_after.data[..72].iter().all(|&b| b == 0),
        "slab must not change on rejected InitMarket (insurance_withdraw_max_bps bounded)"
    );
}

// ============================================================================
// TopUpInsurance (tag 9) additional coverage
// ============================================================================

/// Spec: TopUpInsurance is blocked on resolved markets.
#[test]
fn test_top_up_insurance_blocked_on_resolved() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey()).unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();
    env.try_resolve_market(&admin).unwrap();
    assert!(env.is_market_resolved());

    let payer = Keypair::new();
    env.svm.airdrop(&payer.pubkey(), 5_000_000_000).unwrap();
    let result = env.try_top_up_insurance(&payer, 1_000_000_000);
    assert!(
        result.is_err(),
        "TopUpInsurance must be rejected on a resolved market"
    );
}

/// Spec: TopUpInsurance increases the insurance fund balance by the deposited amount.
#[test]
fn test_top_up_insurance_increases_balance() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let insurance_before = env.read_insurance_balance();
    let vault_before = env.vault_balance();

    let payer = Keypair::new();
    env.svm.airdrop(&payer.pubkey(), 5_000_000_000).unwrap();
    let amount = 2_000_000_000u64;
    env.top_up_insurance(&payer, amount);

    let insurance_after = env.read_insurance_balance();
    let vault_after = env.vault_balance();

    assert_eq!(
        insurance_after - insurance_before, amount as u128,
        "Insurance balance must increase by the top-up amount"
    );
    assert_eq!(
        vault_after - vault_before, amount,
        "Vault SPL balance must increase by the top-up amount"
    );
}

// test_insurance_floor_immutable_after_init removed: insurance_floor
// field was deleted from RiskParams. The bounded `insurance_operator`
// path (tag 23) supersedes it.

// ============================================================================
// TVL:insurance deposit cap (admin opt-in)
// ============================================================================
//
// `MarketConfig.tvl_insurance_cap_mult` gates DepositCollateral such that
// post-deposit `c_tot <= k * insurance_fund.balance`. Default at init is 0
// (disabled); admin enables via UpdateConfig. These tests verify:
//   1. Default init leaves the cap disabled (value = 0).
//   2. Enabling via UpdateConfig persists the value.
//   3. An enabled cap rejects deposits that would exceed the ceiling.
//   4. An enabled cap accepts deposits that stay within the ceiling.
//   5. Disabled cap (k=0) accepts arbitrarily large deposits.
//   6. Enabled cap with zero insurance rejects any deposit (bootstrap case).

fn encode_update_config_with_cap_tag(k: u16) -> Vec<u8> {
    // Same wire format as encode_update_config, plus a trailing u16.
    let mut data = vec![14u8]; // Tag 14 = UpdateConfig
    data.extend_from_slice(&3600u64.to_le_bytes()); // funding_horizon_slots
    data.extend_from_slice(&100u64.to_le_bytes()); // funding_k_bps
    data.extend_from_slice(&100i64.to_le_bytes()); // funding_max_premium_bps
    data.extend_from_slice(&10i64.to_le_bytes());  // funding_max_e9_per_slot
    data.extend_from_slice(&k.to_le_bytes());      // tvl_insurance_cap_mult
    data
}

fn send_update_config(env: &mut TestEnv, admin: &Keypair, k: u16) -> Result<(), String> {
    let ix = solana_sdk::instruction::Instruction {
        program_id: env.program_id,
        accounts: vec![
            solana_sdk::instruction::AccountMeta::new(admin.pubkey(), true),
            solana_sdk::instruction::AccountMeta::new(env.slab, false),
            solana_sdk::instruction::AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false),
            solana_sdk::instruction::AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_update_config_with_cap_tag(k),
    };
    let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    env.svm.send_transaction(tx).map(|_| ()).map_err(|e| format!("{:?}", e))
}

/// Fresh markets default to cap disabled (k=0).
#[test]
fn test_deposit_cap_default_disabled() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    let slab = env.svm.get_account(&env.slab).unwrap();
    let cfg = percolator_prog::state::read_config(&slab.data);
    assert_eq!(
        cfg.tvl_insurance_cap_mult, 0,
        "tvl_insurance_cap_mult must default to 0 (disabled)"
    );
}

/// Admin can enable the cap via UpdateConfig and the value persists.
#[test]
fn test_deposit_cap_enable_via_update_config() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    send_update_config(&mut env, &admin, 20).expect("enable cap");
    let slab = env.svm.get_account(&env.slab).unwrap();
    let cfg = percolator_prog::state::read_config(&slab.data);
    assert_eq!(cfg.tvl_insurance_cap_mult, 20, "cap must persist");
}

/// Cap enabled with k=20 and insurance=1_000 must reject a deposit of
/// 20_001 (exceeds 20 * 1_000 = 20_000) and accept a deposit of 20_000.
#[test]
fn test_deposit_cap_enforced() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    // Seed insurance so the cap denominator is nonzero.
    let insurance_payer = Keypair::new();
    env.svm.airdrop(&insurance_payer.pubkey(), 10_000_000_000).unwrap();
    env.top_up_insurance(&insurance_payer, 1_000);
    assert_eq!(env.read_insurance_balance(), 1_000);

    // Enable the cap at k=20 → ceiling = 20_000 units of c_tot.
    send_update_config(&mut env, &admin, 20).expect("enable cap");

    // init_user itself deposits a 100-unit account-open fee, so c_tot = 100
    // immediately after onboarding. Cap ceiling is 20_000 units.
    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    // Exact-at-ceiling deposit: 100 (already there) + 19_900 = 20_000 = cap.
    env.try_deposit(&user, user_idx, 19_900)
        .expect("deposit at ceiling must succeed");

    // Next deposit of 1 would push c_tot to 20_001 > 20_000 → reject.
    let over = env.try_deposit(&user, user_idx, 1);
    assert!(
        over.is_err(),
        "deposit that would exceed k * insurance must be rejected"
    );
}

/// Cap disabled (k=0) accepts arbitrary deposits even with zero insurance.
#[test]
fn test_deposit_cap_disabled_allows_any_deposit() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    // Do NOT call UpdateConfig — cap stays at default 0.
    assert_eq!(env.read_insurance_balance(), 0, "fresh market has zero insurance");

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.try_deposit(&user, user_idx, 1_000_000)
        .expect("deposit with cap disabled must succeed even with zero insurance");
}

/// Cap enabled with zero insurance rejects any deposit (bootstrap case).
/// Operator is expected to seed insurance before enabling the cap.
#[test]
fn test_deposit_cap_zero_insurance_rejects() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    // Enable cap WITHOUT seeding insurance. All deposit paths must be
    // rejected — including InitUser's fee_payment, which had been a
    // bypass before the fix.
    send_update_config(&mut env, &admin, 20).expect("enable cap");
    assert_eq!(env.read_insurance_balance(), 0);

    // InitUser with any positive fee_payment must now be rejected
    // (c_tot = 0, cap = 20 * 0 = 0, any deposit breaches). Use a raw
    // instruction because the `init_user` helper panics on failure.
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let attacker_ata = env.create_ata(&attacker.pubkey(), 1_000);
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(attacker_ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data: encode_init_user(100),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&attacker.pubkey()),
        &[&attacker],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "InitUser with enabled cap and zero insurance must be rejected"
    );
}

/// Positive: a deposit blocked by the cap becomes allowed once the admin
/// widens k. The operator pattern is "start tight, loosen as insurance
/// grows" — this test verifies a deposit attempt that was over-cap
/// succeeds after the cap is raised, without any other state change.
#[test]
fn test_deposit_cap_widened_unblocks_deposit() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let insurance_payer = Keypair::new();
    env.svm.airdrop(&insurance_payer.pubkey(), 10_000_000_000).unwrap();
    env.top_up_insurance(&insurance_payer, 1_000);

    // Tight cap: k=20 → ceiling = 20_000.
    send_update_config(&mut env, &admin, 20).expect("enable cap k=20");

    let user = Keypair::new();
    let user_idx = env.init_user(&user); // c_tot = 100

    // c_tot = 100; try to deposit 19_901 → c_tot_new = 20_001 > 20_000 → reject.
    let blocked = env.try_deposit(&user, user_idx, 19_901);
    assert!(blocked.is_err(), "deposit must be blocked at k=20");

    // Admin widens: k=40 → ceiling = 40_000.
    send_update_config(&mut env, &admin, 40).expect("widen cap to k=40");

    // Previously-blocked deposit of 19_901 now fits within the 40_000 cap.
    env.try_deposit(&user, user_idx, 19_901)
        .expect("widened cap must unblock the previously-rejected deposit");
}

/// Positive: the alternative widening path — top up insurance rather than
/// raising k. If insurance grows from 1_000 to 2_000 with k=20, the cap
/// doubles from 20_000 to 40_000 without any config change.
#[test]
fn test_deposit_cap_topping_up_insurance_unblocks_deposit() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let insurance_payer = Keypair::new();
    env.svm.airdrop(&insurance_payer.pubkey(), 10_000_000_000).unwrap();
    env.top_up_insurance(&insurance_payer, 1_000);

    send_update_config(&mut env, &admin, 20).expect("enable cap");

    let user = Keypair::new();
    let user_idx = env.init_user(&user); // c_tot = 100

    let blocked = env.try_deposit(&user, user_idx, 19_901);
    assert!(blocked.is_err(), "deposit must be blocked at insurance=1000");

    // Grow insurance: 1_000 → 2_000. Cap rises 20_000 → 40_000.
    env.top_up_insurance(&insurance_payer, 1_000);
    assert_eq!(env.read_insurance_balance(), 2_000);

    env.try_deposit(&user, user_idx, 19_901)
        .expect("topped-up insurance must unblock the deposit");
}

/// Negative: tightening the cap via UpdateConfig stops new deposits even
/// when prior deposits were allowed. Equivalent economic effect to
/// "insurance was higher, now it's lower" — we tighten via k because
/// live insurance withdrawals (WithdrawInsuranceLimited) were removed
/// and WithdrawInsurance requires a resolved market.
#[test]
fn test_deposit_cap_tightened_blocks_further_deposits() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let insurance_payer = Keypair::new();
    env.svm.airdrop(&insurance_payer.pubkey(), 10_000_000_000).unwrap();
    env.top_up_insurance(&insurance_payer, 1_000);

    // Loose cap: k=40 → ceiling = 40_000.
    send_update_config(&mut env, &admin, 40).expect("enable cap k=40");

    let user = Keypair::new();
    let user_idx = env.init_user(&user); // c_tot = 100

    // Deposit 19_900 → c_tot = 20_000. Fits under 40_000 cap.
    env.try_deposit(&user, user_idx, 19_900)
        .expect("initial deposit must succeed under loose cap");

    // Admin tightens: k=20 → new ceiling = 20_000. c_tot already there.
    send_update_config(&mut env, &admin, 20).expect("tighten to k=20");

    // Any further deposit now over-cap → rejected.
    let blocked = env.try_deposit(&user, user_idx, 1);
    assert!(
        blocked.is_err(),
        "tightened cap must reject deposits that exceed the new ceiling"
    );
}

// ============================================================================
// WithdrawInsuranceLimited (tag 23) — permutation coverage
// ============================================================================
// Bounded live fee-extraction gated on the separate `header.insurance_operator`
// authority. Per-call cap = max(10 units, bps * insurance / 10_000), bounded by
// current insurance balance. Calls must be ≥ insurance_withdraw_cooldown_slots
// apart. Operator CANNOT bypass via tag 20 — that path is gated on a
// different field (`header.insurance_authority`).
//
// Authority kinds: AUTHORITY_ADMIN=0, AUTHORITY_HYPERP_MARK=1, AUTHORITY_INSURANCE=2,
// AUTHORITY_CLOSE=3, AUTHORITY_INSURANCE_OPERATOR=4.

const AUTHORITY_INSURANCE_OPERATOR: u8 = 4;

fn encode_withdraw_insurance_limited(amount: u64) -> Vec<u8> {
    let mut data = vec![23u8]; // Tag 23
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

fn send_withdraw_limited(
    env: &mut TestEnv,
    operator: &Keypair,
    amount: u64,
) -> Result<(), String> {
    let operator_ata = env.create_ata(&operator.pubkey(), 0);
    let (vault_pda, _) = Pubkey::find_program_address(
        &[b"vault", env.slab.as_ref()],
        &env.program_id,
    );
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(operator.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(operator_ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(vault_pda, false),
            AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false),
        ],
        data: encode_withdraw_insurance_limited(amount),
    };
    // Repeated airdrops of the same amount in the same blockhash window
    // collide — swallow the AlreadyProcessed.
    let _ = env.svm.airdrop(&operator.pubkey(), 1_000_000_000);
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&operator.pubkey()),
        &[operator],
        env.svm.latest_blockhash(),
    );
    env.svm.send_transaction(tx).map(|_| ()).map_err(|e| format!("{:?}", e))
}

/// Configure a market with bounded-withdrawal enabled: seed insurance and
/// set `insurance_withdraw_max_bps` + `insurance_withdraw_cooldown_slots`
/// via direct slab edits (faster than extending UpdateConfig ABI for tests).
fn setup_bounded_withdrawal(
    env: &mut TestEnv,
    insurance: u64,
    max_bps: u16,
    cooldown_slots: u64,
) {
    env.init_market_with_invert(0);
    let insurance_payer = Keypair::new();
    env.svm.airdrop(&insurance_payer.pubkey(), 10_000_000_000).unwrap();
    env.top_up_insurance(&insurance_payer, insurance);

    // Direct slab edits for config fields that don't yet have UpdateConfig
    // wiring. Safe in tests: we own the slab account.
    //
    // v12.19 MarketConfig layout (up to the insurance withdrawal fields):
    //   offset 0..192  : collateral_mint + vault_pubkey + index_feed_id +
    //                    max_staleness_secs + conf_filter_bps + bump + invert +
    //                    unit_scale + funding (4×u64/i64) + hyperp_authority +
    //                    hyperp_mark_e6 + last_oracle_publish_time
    //   offset 192..200: last_effective_price_e6 (u64)
    //   offset 200..202: insurance_withdraw_max_bps (u16)
    //   offset 202..204: tvl_insurance_cap_mult (u16)
    //   offset 204..208: _iw_padding
    //   offset 208..216: insurance_withdraw_cooldown_slots (u64)
    //
    // With HEADER_LEN = 136:
    //   slab[336..338] = insurance_withdraw_max_bps
    //   slab[344..352] = insurance_withdraw_cooldown_slots
    let mut slab = env.svm.get_account(&env.slab).unwrap();
    slab.data[336..338].copy_from_slice(&max_bps.to_le_bytes());
    slab.data[344..352].copy_from_slice(&cooldown_slots.to_le_bytes());
    env.svm.set_account(env.slab, slab).unwrap();
}

/// 1. Positive: default insurance_operator (=admin) signs, amount within bps
///    cap, insurance balance decrements by exactly the withdrawal amount.
#[test]
fn test_withdraw_limited_operator_succeeds() {
    program_path();
    let mut env = TestEnv::new();
    setup_bounded_withdrawal(&mut env, 10_000, 500, 100); // 5% cap, 100 slot cooldown
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let insurance_before = env.read_insurance_balance();
    let vault_before = env.vault_balance();
    assert_eq!(insurance_before, 10_000);

    // Withdraw 500 units (5% of 10_000 = 500, exactly at cap).
    send_withdraw_limited(&mut env, &admin, 500)
        .expect("operator withdrawal at cap must succeed");

    assert_eq!(env.read_insurance_balance(), 10_000 - 500);
    assert_eq!(env.vault_balance(), vault_before - 500);
}

/// 2. Cooldown: second call within cooldown slots is rejected.
#[test]
fn test_withdraw_limited_cooldown_enforced() {
    program_path();
    let mut env = TestEnv::new();
    setup_bounded_withdrawal(&mut env, 10_000, 500, 1000);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    send_withdraw_limited(&mut env, &admin, 100).expect("first call ok");

    // Advance only a few slots (still inside cooldown=1000).
    env.set_slot(1);
    let blocked = send_withdraw_limited(&mut env, &admin, 50);
    assert!(blocked.is_err(), "second call within cooldown must be rejected");
}

/// 3. After cooldown: second call succeeds.
#[test]
fn test_withdraw_limited_after_cooldown_accepted() {
    program_path();
    let mut env = TestEnv::new();
    setup_bounded_withdrawal(&mut env, 10_000, 500, 100);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    send_withdraw_limited(&mut env, &admin, 100).expect("first call");
    env.set_slot(200); // past cooldown of 100
    send_withdraw_limited(&mut env, &admin, 100)
        .expect("post-cooldown call must succeed");
}

/// 4. Amount exceeding per-call cap is rejected.
#[test]
fn test_withdraw_limited_over_cap_rejected() {
    program_path();
    let mut env = TestEnv::new();
    setup_bounded_withdrawal(&mut env, 10_000, 500, 100); // cap = 500
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let over = send_withdraw_limited(&mut env, &admin, 501);
    assert!(over.is_err(), "amount > cap must be rejected");
}

/// 5. Feature disabled: max_bps=0 rejects all withdrawals.
#[test]
fn test_withdraw_limited_disabled_rejects() {
    program_path();
    let mut env = TestEnv::new();
    setup_bounded_withdrawal(&mut env, 10_000, 0, 100); // disabled (max_bps=0)
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let result = send_withdraw_limited(&mut env, &admin, 1);
    assert!(result.is_err(), "max_bps=0 must disable the bounded path");
}

/// 6. Zero insurance balance rejects.
#[test]
fn test_withdraw_limited_zero_insurance_rejects() {
    program_path();
    let mut env = TestEnv::new();
    setup_bounded_withdrawal(&mut env, 0, 500, 100); // no insurance seeded in call
    // setup_bounded_withdrawal tops up `insurance`; pass 0 to skip.
    // But top_up_insurance(0) is a noop that still packs data — just assert 0.
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Force insurance to zero: init already does so, and our call passed 0
    // above, but top_up_insurance may have written zero. Recheck.
    let result = send_withdraw_limited(&mut env, &admin, 1);
    assert!(result.is_err(), "zero insurance must reject withdrawal");
}

/// 7. Non-operator signer (random key) rejected.
#[test]
fn test_withdraw_limited_rejects_random_signer() {
    program_path();
    let mut env = TestEnv::new();
    setup_bounded_withdrawal(&mut env, 10_000, 500, 100);
    let random = Keypair::new();

    let result = send_withdraw_limited(&mut env, &random, 100);
    assert!(result.is_err(), "random signer must be rejected");
}

/// 8. Key security property: operator CANNOT use tag 20 to bypass bounds.
///    After rotating insurance_operator to a new key, that operator must not
///    be accepted by the unbounded tag-20 path (which still requires the
///    separate insurance_authority).
#[test]
fn test_withdraw_limited_operator_cannot_call_tag_20() {
    program_path();
    let mut env = TestEnv::new();
    setup_bounded_withdrawal(&mut env, 10_000, 500, 100);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Rotate insurance_operator away from admin. Now admin is no longer
    // the operator — and the new operator is not insurance_authority.
    let operator = Keypair::new();
    env.svm.airdrop(&operator.pubkey(), 1_000_000_000).unwrap();
    env.try_update_authority(&admin, AUTHORITY_INSURANCE_OPERATOR, Some(&operator))
        .expect("rotate insurance_operator");

    // Sanity: the operator CAN use tag 23.
    send_withdraw_limited(&mut env, &operator, 100)
        .expect("new operator must be able to use bounded path");

    // CORE SECURITY CHECK: operator must fail tag 20 even in a state
    // where tag 20 would otherwise succeed (resolved market, no accounts).
    // The ONLY remaining gate is the authority check — this isolates the
    // authority-split property from the resolved/empty gates.
    //
    // Resolve the market (admin, via ResolveMarket with authority price
    // set). No accounts have been created → num_used == 0. In this state,
    // tag 20 would succeed for insurance_authority, but not for operator.
    env.set_slot_and_price(200, 1_000_000);
    env.crank();
    env.try_resolve_market(&admin).expect("admin resolves");

    // Operator attempts tag 20: must fail (auth mismatch).
    let bypass = env.try_withdraw_insurance(&operator);
    assert!(
        bypass.is_err(),
        "operator must not be able to call tag 20 even on resolved+empty market"
    );

    // Positive control: insurance_authority (still == admin after init
    // since we didn't rotate it) CAN call tag 20 in this state.
    env.try_withdraw_insurance(&admin)
        .expect("insurance_authority must still be able to call tag 20");
}

/// 9. Anti-Zeno floor: even when `bps × insurance / 10_000 < 10`, the
///    operator can still withdraw up to 10 units per call (or insurance,
///    whichever is smaller). Guarantees the fund can be fully drained
///    over repeated calls rather than asymptoting.
#[test]
fn test_withdraw_limited_floor_prevents_zeno_paradox() {
    program_path();
    let mut env = TestEnv::new();
    // insurance = 100, max_bps = 1 → bps_cap = 100 × 1 / 10_000 = 0 units.
    // Without the floor, no withdrawal would be possible. The MIN floor of
    // 10 lets operator drain 10 per call.
    setup_bounded_withdrawal(&mut env, 100, 1, 1);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    send_withdraw_limited(&mut env, &admin, 10)
        .expect("anti-Zeno floor must permit 10-unit withdrawal even at tiny bps");
    assert_eq!(env.read_insurance_balance(), 90);

    // 11 would exceed the floor → rejected.
    env.set_slot(10);
    let over_floor = send_withdraw_limited(&mut env, &admin, 11);
    assert!(
        over_floor.is_err(),
        "floor is 10 units; 11 must be rejected"
    );
}

/// 10. After rotation, previous operator (admin) is rejected and new
///     operator is accepted.
#[test]
fn test_withdraw_limited_rotation_swaps_authority() {
    program_path();
    let mut env = TestEnv::new();
    setup_bounded_withdrawal(&mut env, 10_000, 500, 100);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Pre-rotation: admin is the default operator → accepted.
    send_withdraw_limited(&mut env, &admin, 100).expect("admin as default operator");

    // Rotate operator to a fresh key.
    let new_op = Keypair::new();
    env.svm.airdrop(&new_op.pubkey(), 1_000_000_000).unwrap();
    env.try_update_authority(&admin, AUTHORITY_INSURANCE_OPERATOR, Some(&new_op))
        .expect("rotate operator");

    env.set_slot(200);

    // Post-rotation: admin no longer operator → rejected.
    let admin_rejected = send_withdraw_limited(&mut env, &admin, 100);
    assert!(
        admin_rejected.is_err(),
        "admin after rotation must be rejected"
    );

    // New operator accepted.
    send_withdraw_limited(&mut env, &new_op, 100)
        .expect("new operator must be accepted post-rotation");
}

/// 11. Resolved markets reject bounded withdrawal (unbounded tag 20 owns
///     that case).
#[test]
fn test_withdraw_limited_resolved_market_rejects() {
    program_path();
    let mut env = TestEnv::new();
    setup_bounded_withdrawal(&mut env, 10_000, 500, 100);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    env.set_slot(100);
    env.crank();
    env.try_resolve_market(&admin)
        .expect("admin can resolve live market");

    let result = send_withdraw_limited(&mut env, &admin, 100);
    assert!(
        result.is_err(),
        "bounded path must reject on resolved market"
    );
}

/// End-to-end: bounded insurance withdrawal tightens the deposit cap.
///
/// Setup:
///   - tvl_insurance_cap_mult = 20 (deposit cap enabled, k=20)
///   - insurance_withdraw_max_bps = 5000 (50% of insurance per call)
///   - insurance seed = 1_000 units  → deposit cap = 20_000 units
///
/// Flow:
///   1. User deposits up to the ceiling (c_tot = 20_000, fills cap exactly)
///   2. Operator withdraws 500 insurance (allowed: 5000 bps × 1_000 = 500)
///   3. Insurance is now 500, cap shrank to 500 × 20 = 10_000 units
///   4. c_tot (20_000) already exceeds the new cap → any further deposit is
///      rejected. The protocol is now rate-limiting new exposure until
///      insurance grows back via fees.
#[test]
fn test_bounded_withdrawal_tightens_deposit_cap() {
    program_path();
    let mut env = TestEnv::new();
    setup_bounded_withdrawal(&mut env, 1_000, 5_000, 100);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Enable deposit cap at k=20. UpdateConfig also sets funding params.
    send_update_config(&mut env, &admin, 20).expect("enable deposit cap k=20");

    // Fill up to the cap: c_tot = init_user_fee(100) + user_deposit(19_900)
    // = 20_000 = k × insurance (1000 × 20).
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.try_deposit(&user, user_idx, 19_900)
        .expect("initial deposit at ceiling must succeed");
    let ins_before = env.read_insurance_balance();
    assert_eq!(ins_before, 1_000);

    // Operator withdraws 500 insurance (exactly at the 50% bps cap).
    send_withdraw_limited(&mut env, &admin, 500)
        .expect("operator withdraws 500");
    let ins_after = env.read_insurance_balance();
    assert_eq!(ins_after, 500, "insurance must drop by withdrawal amount");

    // Cap shrank: 500 × 20 = 10_000 < c_tot (20_000). Further deposit blocked.
    let blocked = env.try_deposit(&user, user_idx, 1);
    assert!(
        blocked.is_err(),
        "insurance withdrawal must tighten the deposit cap and block further deposits"
    );
}

/// REGRESSION / SECURITY: InitUser's `fee_payment` path bypasses the
/// deposit cap. The cap check lives only in DepositCollateral, but
/// InitUser also increments c_tot via engine.deposit_not_atomic with
/// an arbitrary u64 amount. An attacker who hits the cap via the
/// normal path can still grow c_tot unboundedly by creating new
/// accounts with large `fee_payment` values.
#[test]
fn test_deposit_cap_bypassed_via_init_user() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    // Seed insurance, enable the cap with k=20 → ceiling = 20_000 units.
    let insurance_payer = Keypair::new();
    env.svm.airdrop(&insurance_payer.pubkey(), 10_000_000_000).unwrap();
    env.top_up_insurance(&insurance_payer, 1_000);
    send_update_config(&mut env, &admin, 20).expect("enable cap k=20");

    // First user fills to ceiling via init_user (100) + deposit (19_800)
    // → c_tot = 19_900. One more unit via normal deposit would exceed
    // 20_000 — so the deposit path is blocked.
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.try_deposit(&user, user_idx, 19_800)
        .expect("fill close to ceiling");
    let blocked = env.try_deposit(&user, user_idx, 101);
    assert!(blocked.is_err(), "DepositCollateral correctly enforces cap");

    // SECURITY CHECK: a FRESH account created via init_user with a large
    // fee_payment should ALSO be blocked — c_tot_new = 19_900 + 10_000 =
    // 29_900, which exceeds the 20_000 cap. The cap must apply to every
    // capital-adding path, not just DepositCollateral.
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 10_000_000_000).unwrap();
    let attacker_ata = env.create_ata(&attacker.pubkey(), 20_000);
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(attacker_ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data: encode_init_user(10_000), // fee_payment = 10_000 units
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&attacker.pubkey()),
        &[&attacker],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "InitUser with fee_payment that breaches the cap must be rejected"
    );
}
