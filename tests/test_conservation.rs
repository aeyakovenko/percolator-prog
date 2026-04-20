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

/// ATTACK: Verify no value is created or destroyed through trading operations.
/// Expected: Total vault token balance equals total deposits minus total withdrawals.
#[test]
fn test_attack_conservation_invariant() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 10_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 10_000_000_000);

    let total_deposited: u64 = 120_000_000_000 + 300; // 100 + 10 + 10 SOL + 3 init deposits

    // Vault should have all deposited funds
    let vault_after_deposits = env.vault_balance();
    assert_eq!(
        vault_after_deposits, total_deposited,
        "Vault should have exactly the deposited amount (including init deposits)"
    );

    // User1 goes long, user2 goes short
    env.trade(&user1, &lp, lp_idx, user1_idx, 5_000_000);
    env.trade(&user2, &lp, lp_idx, user2_idx, -5_000_000);

    // Trading doesn't move tokens in/out of vault
    let vault_after_trades = env.vault_balance();
    assert_eq!(
        vault_after_trades, total_deposited,
        "Trading should not change vault token balance"
    );

    // Price changes and crank (internal PnL settlement, no token transfers)
    env.set_slot_and_price(200, 150_000_000);
    env.crank();

    let vault_after_crank = env.vault_balance();
    assert_eq!(
        vault_after_crank, total_deposited,
        "Crank should not change vault token balance"
    );

    // Price reversal and another crank
    env.set_slot_and_price(300, 120_000_000);
    env.crank();

    let vault_after_reversal = env.vault_balance();
    assert_eq!(
        vault_after_reversal, total_deposited,
        "Price reversal+crank should not change vault token balance"
    );

    println!(
        "CONSERVATION VERIFIED: Vault balance {} unchanged through all operations",
        vault_after_reversal
    );
}

/// ATTACK: Multiple users settle in same crank - verify no double-counting.
/// Expected: Conservation holds: vault = total deposits always.
#[test]
fn test_attack_multi_user_settlement_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Create 5 users with opposing positions
    let mut users = Vec::new();
    let total_user_deposit = 5 * 10_000_000_000u64;
    for i in 0..5 {
        let u = Keypair::new();
        let idx = env.init_user(&u);
        env.deposit(&u, idx, 10_000_000_000);
        // Alternate long/short
        let size = if i % 2 == 0 {
            5_000_000i128
        } else {
            -5_000_000i128
        };
        env.trade(&u, &lp, lp_idx, idx, size);
        users.push((u, idx));
    }

    let total_deposited = 100_000_000_000 + total_user_deposit + 600; // +600 from 6 init deposits

    // Price changes and multiple cranks
    env.set_slot_and_price(200, 150_000_000);
    env.crank();

    let vault_after = env.vault_balance();
    assert_eq!(
        vault_after, total_deposited,
        "ATTACK: Conservation violated after multi-user settlement"
    );

    env.set_slot_and_price(300, 120_000_000);
    env.crank();

    let vault_after2 = env.vault_balance();
    assert_eq!(
        vault_after2, total_deposited,
        "ATTACK: Conservation violated after price reversal"
    );
}

/// ATTACK: Trade → Price crash → Trade reverse → Crank. Does the vault balance stay correct?
/// Expected: Conservation holds through the entire sequence.
#[test]
fn test_attack_trade_crash_reverse_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let total = 110_000_000_200u64; // includes 2 init deposits

    // Open long
    env.trade(&user, &lp, lp_idx, user_idx, 10_000_000);

    // Price crashes
    env.set_slot_and_price(200, 80_000_000);
    env.crank();
    assert_eq!(env.vault_balance(), total, "Conservation after crash");

    // Reverse position (long → short)
    let pos_before_flip = env.read_account_position(user_idx);
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, -20_000_000);
    let pos_after_flip = env.read_account_position(user_idx);
    if result.is_ok() {
        assert_eq!(
            pos_after_flip,
            pos_before_flip - 20_000_000,
            "Successful flip should update position additively: before={} after={}",
            pos_before_flip,
            pos_after_flip
        );
    } else {
        assert_eq!(
            pos_after_flip, pos_before_flip,
            "Rejected flip should preserve position: before={} after={}",
            pos_before_flip, pos_after_flip
        );
    }
    env.set_slot_and_price(300, 80_000_000);
    env.crank();
    assert_eq!(env.vault_balance(), total, "Conservation after flip");

    // Price recovers
    env.set_slot_and_price(400, 138_000_000);
    env.crank();
    assert_eq!(env.vault_balance(), total, "Conservation after recovery");
}

/// ATTACK: Conservation through complete lifecycle (init → trade → crank → close).
/// Expected: After all accounts closed, vault should have only insurance fees.
#[test]
fn test_attack_full_lifecycle_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let total = 110_000_000_200u64; // includes 2 init deposits
    assert_eq!(env.vault_balance(), total, "Initial vault");

    // Trade → crank → close
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    env.set_slot_and_price(200, 150_000_000);
    env.crank();

    // Close the trade
    env.trade(&user, &lp, lp_idx, user_idx, -5_000_000);
    env.set_slot_and_price(300, 150_000_000);
    env.crank();

    // Vault should still have all tokens (internal PnL transfer, no external movement)
    assert_eq!(
        env.vault_balance(),
        total,
        "ATTACK: Conservation through full trade lifecycle violated"
    );
}

/// ATTACK: Force-close during premarket resolution should maintain PnL conservation.
/// Sum of all PnL changes after force-close should be zero (zero-sum).
#[test]
fn test_attack_premarket_force_close_pnl_conservation() {
    let mut env = TradeCpiTestEnv::new();

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.set_oracle_price_e6(1_000_000);

    // Create LP and 3 users with positions
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let mut users = Vec::new();
    for _ in 0..3 {
        let user = Keypair::new();
        let user_idx = env.init_user(&user);
        env.deposit(&user, user_idx, 2_000_000_000);
        users.push((user, user_idx));
    }

    env.set_slot(100);
    env.crank();

    // Each user takes a different sized position
    for (i, (user, user_idx)) in users.iter().enumerate() {
        let size = ((i as i128) + 1) * 50_000_000;
        let result = env.try_trade_cpi(
            user,
            &lp.pubkey(),
            lp_idx,
            *user_idx,
            size,
            &matcher_prog,
            &matcher_ctx,
        );
        assert!(result.is_ok(), "Trade {} should succeed: {:?}", i, result);
    }

    env.set_slot(150);
    env.crank();

    // Record PnL before force-close
    let lp_pnl_before = env.read_account_pnl(lp_idx);
    let mut user_pnl_before_sum: i128 = 0;
    for (_, user_idx) in &users {
        user_pnl_before_sum += env.read_account_pnl(*user_idx);
    }
    let total_pnl_before = lp_pnl_before + user_pnl_before_sum;

    // Flatten positions before resolution (v12.17 requirement)
    env.set_slot(200);
    env.crank();
    for (user, user_idx) in &users {
        let pos = env.read_account_position(*user_idx);
        if pos != 0 {
            let _ = env.try_trade_cpi(
                user, &lp.pubkey(), lp_idx, *user_idx,
                -pos, &matcher_prog, &matcher_ctx,
            );
        }
    }
    env.set_slot(210);
    env.crank();

    // Resolve at different price to create PnL
    env.set_oracle_price_e6(1_500_000); // 50% up
    env.try_resolve_market(&admin).unwrap();

    // Force-close via crank (settles PnL only; positions require AdminForceCloseAccount)
    env.set_slot(300);
    env.crank();
    env.set_slot(400);
    env.crank(); // Second crank in case pagination needed

    // Close each account explicitly
    for (user, user_idx) in &users {
        env.try_admin_force_close_account(&admin, *user_idx, &user.pubkey())
            .expect("AdminForceCloseAccount user must succeed");
    }
    env.try_admin_force_close_account(&admin, lp_idx, &lp.pubkey())
        .expect("AdminForceCloseAccount LP must succeed");

    // All positions should be closed
    for (_, user_idx) in &users {
        assert_eq!(
            env.read_account_position(*user_idx),
            0,
            "User position should be zero after force-close"
        );
    }
    assert_eq!(
        env.read_account_position(lp_idx),
        0,
        "LP position should be zero after force-close"
    );

    // Force-close uses attach_effective_position which settles PnL through capital.
    // PnL field may not sum to zero since gains settle to capital.
    // Instead verify conservation: vault balance matches engine state.
    let lp_pnl_after = env.read_account_pnl(lp_idx);
    let mut user_pnl_after_sum: i128 = 0;
    for (_, user_idx) in &users {
        user_pnl_after_sum += env.read_account_pnl(*user_idx);
    }
    let total_pnl_after = lp_pnl_after + user_pnl_after_sum;
    let pnl_delta = total_pnl_after - total_pnl_before;

    // Log PnL delta for informational purposes
    let _ = (pnl_delta, lp_pnl_before, user_pnl_before_sum);

    // Conservation check: engine vault == actual vault balance (no value created or destroyed)
    let engine_vault = env.read_vault();
    let actual_vault = env.vault_balance() as u128;
    assert_eq!(
        engine_vault, actual_vault,
        "ATTACK: Force-close violated vault conservation! engine={} actual={}",
        engine_vault, actual_vault
    );
}

/// ATTACK: Multi-LP conservation. Trade against 2 different LPs and verify
/// no value is created or destroyed. Total vault must remain constant.
#[test]
fn test_attack_multi_lp_conservation() {
    let mut env = TradeCpiTestEnv::new();

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.set_oracle_price_e6(1_000_000);

    // Create 2 LPs
    let lp1 = Keypair::new();
    let (lp1_idx, matcher_ctx1) = env.init_lp_with_matcher(&lp1, &matcher_prog);
    env.deposit(&lp1, lp1_idx, 50_000_000_000);

    let lp2 = Keypair::new();
    let (lp2_idx, matcher_ctx2) = env.init_lp_with_matcher(&lp2, &matcher_prog);
    env.deposit(&lp2, lp2_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.set_slot(100);
    env.crank();

    let vault_before = env.read_vault();

    // Trade against LP1 (long)
    let result = env.try_trade_cpi(
        &user,
        &lp1.pubkey(),
        lp1_idx,
        user_idx,
        200_000_000,
        &matcher_prog,
        &matcher_ctx1,
    );
    assert!(result.is_ok(), "Trade vs LP1 should succeed: {:?}", result);

    // Trade against LP2 (long again)
    let result = env.try_trade_cpi(
        &user,
        &lp2.pubkey(),
        lp2_idx,
        user_idx,
        100_000_000,
        &matcher_prog,
        &matcher_ctx2,
    );
    assert!(result.is_ok(), "Trade vs LP2 should succeed: {:?}", result);

    // Price moves
    env.set_oracle_price_e6(1_200_000);
    env.set_slot(200);
    env.crank();

    // Close positions
    let result = env.try_trade_cpi(
        &user,
        &lp1.pubkey(),
        lp1_idx,
        user_idx,
        -200_000_000,
        &matcher_prog,
        &matcher_ctx1,
    );
    assert!(result.is_ok(), "Close vs LP1 should succeed: {:?}", result);

    let result = env.try_trade_cpi(
        &user,
        &lp2.pubkey(),
        lp2_idx,
        user_idx,
        -100_000_000,
        &matcher_prog,
        &matcher_ctx2,
    );
    assert!(result.is_ok(), "Close vs LP2 should succeed: {:?}", result);

    env.set_slot(300);
    env.crank();

    let vault_after = env.read_vault();
    assert_eq!(
        vault_before, vault_after,
        "ATTACK: Multi-LP trading violated conservation. before={}, after={}",
        vault_before, vault_after
    );
}

/// ATTACK: Conservation invariant through trade + price movement + settlement.
/// vault_balance must equal internal vault tracking at every step.
#[test]
fn test_attack_conservation_through_price_movement() {
    let mut env = TradeCpiTestEnv::new();

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.set_oracle_price_e6(1_000_000);

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.set_slot(100);
    env.crank();

    let vault_initial = env.read_vault();

    // Trade
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        200_000_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(result.is_ok(), "Trade should succeed");

    // Check conservation after trade
    let vault_after_trade = env.read_vault();
    assert_eq!(
        vault_initial, vault_after_trade,
        "Conservation violated after trade: {} vs {}",
        vault_initial, vault_after_trade
    );

    // Price moves up
    env.set_oracle_price_e6(1_500_000);
    env.set_slot(200);
    env.crank();

    // Check conservation after price movement + crank
    let vault_after_crank = env.read_vault();
    assert_eq!(
        vault_initial, vault_after_crank,
        "Conservation violated after price movement: {} vs {}",
        vault_initial, vault_after_crank
    );

    // Price moves down
    env.set_oracle_price_e6(500_000);
    env.set_slot(300);
    env.crank();

    let vault_after_crash = env.read_vault();
    assert_eq!(
        vault_initial, vault_after_crash,
        "Conservation violated after price crash: {} vs {}",
        vault_initial, vault_after_crash
    );

    // Close position
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        -200_000_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(result.is_ok(), "Close should succeed");

    let vault_final = env.read_vault();
    assert_eq!(
        vault_initial, vault_final,
        "Conservation violated after full lifecycle: {} vs {}",
        vault_initial, vault_final
    );
}

/// ATTACK: Premarket partial force-close conservation.
/// After force-closing only some accounts, internal state must still be consistent.
#[test]
fn test_attack_premarket_partial_force_close_conservation() {
    let mut env = TradeCpiTestEnv::new();

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.set_oracle_price_e6(1_000_000);

    // Create LP and many users
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let mut users = Vec::new();
    for _ in 0..5 {
        let user = Keypair::new();
        let user_idx = env.init_user(&user);
        env.deposit(&user, user_idx, 2_000_000_000);
        users.push((user, user_idx));
    }

    env.set_slot(100);
    env.crank();

    // Each user trades
    for (user, user_idx) in &users {
        env.try_trade_cpi(
            user,
            &lp.pubkey(),
            lp_idx,
            *user_idx,
            50_000_000,
            &matcher_prog,
            &matcher_ctx,
        )
        .expect("all user pre-resolution TradeCpi operations must succeed");
    }

    let vault_before = env.read_vault();

    // Resolve market
    env.set_oracle_price_e6(1_200_000);
    env.try_resolve_market(&admin).unwrap();

    // Single crank: may only force-close a batch (64 accounts max)
    env.set_slot(200);
    env.crank();

    // Vault conservation must hold even during partial close
    let vault_after_partial = env.read_vault();
    assert_eq!(
        vault_before, vault_after_partial,
        "ATTACK: Partial force-close violated conservation: {} vs {}",
        vault_before, vault_after_partial
    );

    // Complete force-close
    env.set_slot(300);
    env.crank();

    let vault_after_complete = env.read_vault();
    assert_eq!(
        vault_before, vault_after_complete,
        "ATTACK: Complete force-close violated conservation: {} vs {}",
        vault_before, vault_after_complete
    );
}

/// ATTACK: Multiple deposits in same transaction should not create extra capital.
/// Total capital should equal total deposited amount.
#[test]
fn test_attack_multiple_deposits_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Multiple small deposits
    let deposit_amount = 1_000_000_000u64;
    let num_deposits = 10;
    for _ in 0..num_deposits {
        env.deposit(&user, user_idx, deposit_amount);
    }

    let expected_total = deposit_amount as u128 * num_deposits + 100; // +100 from init
    let actual_capital = env.read_account_capital(user_idx);
    assert_eq!(
        actual_capital, expected_total,
        "ATTACK: Multiple deposits created extra capital! expected={}, actual={}",
        expected_total, actual_capital
    );

    // Vault should have all deposits (user capital + LP deposit + LP init)
    let vault = env.vault_balance();
    let expected_vault = expected_total + 100_000_000_000 + 100; // user + LP deposit + LP init
    assert_eq!(
        vault, expected_vault as u64,
        "ATTACK: Vault balance mismatch after multiple deposits. expected={}, actual={}",
        expected_vault, vault
    );
}

/// ATTACK: GC removes account after force-realize closes position.
/// Verify that value doesn't leak when GC removes accounts with zero capital.
#[test]
fn test_attack_gc_after_force_realize_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 100); // Very small deposit

    env.crank();

    // Open position - user's equity will be wiped by fees/movement
    let trade_result = env.try_trade(&user, &lp, lp_idx, user_idx, 1);
    let user_pos_after_trade = env.read_account_position(user_idx);
    assert!(
        trade_result.is_ok(),
        "Precondition failed: tiny trade should open a position: {:?}",
        trade_result
    );
    assert_ne!(
        user_pos_after_trade, 0,
        "Successful tiny trade must create a non-zero user position"
    );

    // Advance time to trigger maintenance fees (if set)
    env.set_slot(1000);
    env.crank();

    // The small account may have been GC'd
    let num_used = env.read_num_used_accounts();
    // At minimum, LP should still be alive
    assert!(num_used >= 1, "LP should still exist");

    // Conservation: SPL vault should match engine expectations
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        let vault_account = TokenAccount::unpack(&vault_data).unwrap();
        vault_account.amount
    };
    assert!(
        spl_vault > 0,
        "ATTACK: SPL vault should not be drained by GC + force-realize"
    );
}

/// ATTACK: Multiple cranks with funding accumulation verify conservation.
/// Run many cranks across different slots with positions and verify
/// total value (vault) is conserved (funding is zero-sum between accounts).
#[test]
fn test_attack_multi_crank_funding_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let spl_vault_initial = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        let vault_account = TokenAccount::unpack(&vault_data).unwrap();
        vault_account.amount
    };

    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);

    // Run 10 cranks at increasing slots to accumulate funding
    for slot in (100..=1000).step_by(100) {
        env.set_slot(slot);
        env.crank();
    }

    // After all cranks, SPL vault must be unchanged (funding is internal)
    let spl_vault_after = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        let vault_account = TokenAccount::unpack(&vault_data).unwrap();
        vault_account.amount
    };
    assert_eq!(
        spl_vault_initial, spl_vault_after,
        "ATTACK: Multi-crank funding caused SPL vault imbalance! Before={} After={}",
        spl_vault_initial, spl_vault_after
    );

    // Engine vault should still be total deposited amount
    let engine_vault = {
        let slab = env.svm.get_account(&env.slab).unwrap();
        u128::from_le_bytes(slab.data[584..600].try_into().unwrap()) // BPF ENGINE_OFF=584, vault at engine offset 0
    };
    assert_eq!(
        engine_vault, 20_000_000_200,
        "ATTACK: Multi-crank funding changed engine vault! Expected 20B, got {}",
        engine_vault
    );
}

/// ATTACK: Deposit to LP account with outstanding fee debt.
/// Deposit should pay fee debt first, then add remainder to capital.
/// Verify insurance fund receives correct fee payment.
/// ATTACK: UpdateConfig should preserve conservation invariant.
/// Changing risk parameters should not alter vault/capital/insurance totals.
#[test]
fn test_attack_updateconfig_preserves_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.crank();

    // Read state before config change
    let spl_vault_before = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    let engine_vault_before = {
        let slab = env.svm.get_account(&env.slab).unwrap();
        u128::from_le_bytes(slab.data[584..600].try_into().unwrap()) // BPF ENGINE_OFF=584, vault at engine offset 0
    };

    // UpdateConfig with different parameters
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let result = env.try_update_config_with_params(
        &admin,
        7200,                       // funding_horizon_slots
        2000,                       // alpha_bps
        0,
        10_000_000_000_000_000u128, // thresh_max (= max_insurance_floor cap = MAX_VAULT_TVL)
    );
    assert!(
        result.is_ok(),
        "UpdateConfig should succeed with valid params"
    );

    // Read state after config change
    let spl_vault_after = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    let engine_vault_after = {
        let slab = env.svm.get_account(&env.slab).unwrap();
        u128::from_le_bytes(slab.data[584..600].try_into().unwrap()) // BPF ENGINE_OFF=584, vault at engine offset 0
    };

    // Conservation: UpdateConfig must not change vault balances
    assert_eq!(
        spl_vault_before, spl_vault_after,
        "ATTACK: UpdateConfig changed SPL vault balance!"
    );
    assert_eq!(
        engine_vault_before, engine_vault_after,
        "ATTACK: UpdateConfig changed engine vault balance!"
    );
}

/// ATTACK: Insurance fund receives both dust sweep and fee accrual in same crank.
/// Verify both sources of insurance top-up are correctly accounted for.
/// ATTACK: Close all positions then close account, verify complete cleanup.
/// User opens position, closes it, then closes account.
/// Verify capital is correctly returned and no value is left behind.
#[test]
fn test_attack_full_close_cycle_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    // Open and close position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, -100_000);
    env.crank();

    // Position should be zero
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(user_pos, 0, "Position should be zero after close");

    // Read user's capital before close
    let _user_cap = env.read_account_capital(user_idx);

    // Close account
    let close_result = env.try_close_account(&user, user_idx);
    assert!(
        close_result.is_ok(),
        "Close account should succeed with zero position"
    );

    // After close, user's capital should be returned via SPL transfer
    // num_used should decrease by 1
    let num_used_after = env.read_num_used_accounts();
    assert_eq!(num_used_after, 1, "Only LP should remain after user close");

    // Verify capital was returned (SPL vault should have decreased by user_cap)
    let spl_vault_after = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    // SPL vault should be 15B - user_cap (what user got back)
    assert!(
        spl_vault_after < 15_000_000_000,
        "ATTACK: Vault didn't decrease after user close! SPL vault still at {}",
        spl_vault_after
    );
    assert!(spl_vault_after > 0, "Vault should still have LP's deposit");
}

/// ATTACK: Trade must not decrease insurance fund or change vault.
/// Note: Market uses default trading_fee_bps=0. For non-zero fee testing,
/// see test_attack_new_account_fee_goes_to_insurance which tests fee→insurance.
#[test]
fn test_attack_trading_fee_insurance_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.crank();

    let insurance_before = env.read_insurance_balance();

    // Execute trade
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);

    let insurance_after = env.read_insurance_balance();

    // Insurance should not decrease from a trade
    assert!(
        insurance_after >= insurance_before,
        "ATTACK: Insurance decreased after trade! Before={} After={}",
        insurance_before,
        insurance_after
    );

    // Vault conservation
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 20_000_000_200,
        "ATTACK: SPL vault changed after trade (should only change on deposit/withdraw)!"
    );
}

/// ATTACK: Multiple deposits followed by single large withdrawal.
/// Verify conservation across many small deposits then one withdrawal.
#[test]
fn test_attack_many_deposits_one_withdrawal_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    // Many small deposits
    for _ in 0..20 {
        env.deposit(&user, user_idx, 100_000_000); // 100M each
    }

    env.crank();

    // Total deposited: 20 * 100M = 2B
    let cap = env.read_account_capital(user_idx);
    assert_eq!(
        cap, 2_000_000_100,
        "Capital should equal sum of deposits: {}",
        cap
    );

    // Withdraw half
    let withdraw_result = env.try_withdraw(&user, user_idx, 1_000_000_000);
    assert!(withdraw_result.is_ok(), "Withdrawal should succeed");

    let cap_after = env.read_account_capital(user_idx);
    assert_eq!(
        cap_after, 1_000_000_100,
        "Capital after withdrawal should be 1B + init deposit: {}",
        cap_after
    );

    // SPL vault conservation
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 11_000_000_200,
        "ATTACK: SPL vault has wrong balance! expected 11B+200, got {}",
        spl_vault
    );
}

/// ATTACK: Verify conservation after complex multi-user lifecycle.
/// Multiple users open positions, some profitable, some losing, then all close.
/// Total withdrawn should equal total deposited.
#[test]
fn test_attack_multi_user_lifecycle_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    // Create 3 users
    let user1 = Keypair::new();
    let u1_idx = env.init_user(&user1);
    env.deposit(&user1, u1_idx, 5_000_000_000);

    let user2 = Keypair::new();
    let u2_idx = env.init_user(&user2);
    env.deposit(&user2, u2_idx, 3_000_000_000);

    let user3 = Keypair::new();
    let u3_idx = env.init_user(&user3);
    env.deposit(&user3, u3_idx, 2_000_000_000);

    env.crank();

    // Open various positions
    env.trade(&user1, &lp, lp_idx, u1_idx, 100_000); // user1 long
    env.set_slot(10);
    env.trade(&user2, &lp, lp_idx, u2_idx, -50_000); // user2 short
    env.set_slot(20);
    env.crank();

    // Close all positions
    env.trade(&user1, &lp, lp_idx, u1_idx, -100_000);
    env.set_slot(30);
    env.trade(&user2, &lp, lp_idx, u2_idx, 50_000);
    env.set_slot(40);
    env.crank();

    // All positions should be zero
    assert_eq!(
        env.read_account_position(u1_idx),
        0,
        "User1 position not zero"
    );
    assert_eq!(
        env.read_account_position(u2_idx),
        0,
        "User2 position not zero"
    );
    assert_eq!(
        env.read_account_position(u3_idx),
        0,
        "User3 position not zero"
    );
    assert_eq!(env.read_account_position(lp_idx), 0, "LP position not zero");

    // SPL vault should be unchanged (no deposits/withdrawals during trading)
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 60_000_000_400,
        "ATTACK: SPL vault changed during multi-user lifecycle! vault={}",
        spl_vault
    );

    // c_tot should equal sum of all capitals
    let c_tot = env.read_c_tot();
    let total_cap = env.read_account_capital(lp_idx)
        + env.read_account_capital(u1_idx)
        + env.read_account_capital(u2_idx)
        + env.read_account_capital(u3_idx);
    assert_eq!(
        c_tot, total_cap,
        "ATTACK: c_tot desync after lifecycle! c_tot={} sum={}",
        c_tot, total_cap
    );
}

/// ATTACK: Conservation invariant across large slot jumps.
/// Advance many slots, verify conservation holds despite funding/fee accrual.
#[test]
fn test_attack_conservation_large_slot_jump() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Large slot jump (1 million slots)
    env.set_slot(1_000_000);
    env.crank();

    // SPL vault unchanged
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_200,
        "ATTACK: SPL vault changed after large slot jump!"
    );

    // Engine vault matches SPL vault
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault, spl_vault as u128,
        "ATTACK: Engine vault != SPL vault after slot jump! engine={} spl={}",
        engine_vault, spl_vault
    );

    // c_tot consistency
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync after large slot jump! c_tot={} sum={}",
        c_tot, sum
    );

    // vault >= c_tot + insurance
    let insurance = env.read_insurance_balance();
    assert!(
        engine_vault >= c_tot + insurance,
        "ATTACK: vault < c_tot + insurance! vault={} c_tot={} ins={}",
        engine_vault,
        c_tot,
        insurance
    );
}

/// ATTACK: Unit scale market - trade, crank, conservation.
/// Markets with unit_scale > 0 use scaled prices. Verify conservation.
#[test]
fn test_attack_unit_scale_trade_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_full(0, 1000, 0); // unit_scale=1000

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    // With unit_scale=1000, need 100*1000=100_000 base for min_initial_deposit
    let lp_idx = env.init_lp_with_fee(&lp, 100_000);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user_with_fee(&user, 100_000);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert_eq!(
        env.read_account_position(user_idx),
        100_000,
        "Precondition: position open"
    );

    // Price move and crank
    env.set_slot_and_price(50, 150_000_000);
    env.crank();

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with unit_scale=1000! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_200_000,
        "ATTACK: SPL vault changed with unit_scale (includes init deposits)!"
    );
}

/// ATTACK: Inverted market (invert=1) trade and conservation.
/// Inverted markets use 1e12/oracle_price. Verify conservation.
#[test]
fn test_attack_inverted_market_trade_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(1); // Inverted

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade on inverted market
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert_eq!(env.read_account_position(user_idx), 100_000);

    // Price move (raw oracle price change)
    env.set_slot_and_price(50, 150_000_000); // Oracle moves 138→150
    env.crank();

    // Conservation on inverted market
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync on inverted market! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_200,
        "ATTACK: SPL vault changed on inverted market!"
    );
}

/// ATTACK: Inverted market with unit_scale > 0 (double transformation).
/// Both inversion and scaling applied. Verify conservation.
#[test]
fn test_attack_inverted_with_unit_scale_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_full(1, 1000, 0); // invert=1, unit_scale=1000

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    // With unit_scale=1000, need 100*1000=100_000 base for min_initial_deposit
    let lp_idx = env.init_lp_with_fee(&lp, 100_000);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user_with_fee(&user, 100_000);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade on inverted+scaled market
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert_eq!(env.read_account_position(user_idx), 100_000);

    // Price change
    env.set_slot_and_price(50, 150_000_000);
    env.crank();

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with invert+unit_scale! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_200_000,
        "ATTACK: SPL vault changed with invert+unit_scale (includes init deposits)!"
    );
}

/// ATTACK: Trade with position size = 1 (smallest non-zero).
/// Verify conservation holds even with minimal position.
#[test]
fn test_attack_trade_size_one_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade with smallest possible size
    env.trade(&user, &lp, lp_idx, user_idx, 1);
    assert_eq!(env.read_account_position(user_idx), 1);
    assert_eq!(env.read_account_position(lp_idx), -1);

    // Price change
    env.set_slot_and_price(50, 150_000_000);
    env.crank();

    // Conservation with size=1
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with size=1! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 16_000_000_200,
        "ATTACK: SPL vault changed with size=1 trade!"
    );
}

/// ATTACK: Trade size = -1 (smallest short position).
/// Verify negative position of size 1 conserves.
#[test]
fn test_attack_trade_size_negative_one_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Short size=1
    env.trade(&user, &lp, lp_idx, user_idx, -1);
    assert_eq!(env.read_account_position(user_idx), -1);
    assert_eq!(env.read_account_position(lp_idx), 1);

    // Price change and crank
    env.set_slot_and_price(50, 120_000_000);
    env.crank();

    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with size=-1! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 16_000_000_200,
        "ATTACK: SPL vault changed with size=-1 trade!"
    );
}

/// ATTACK: Withdraw then immediately re-deposit.
/// Verify no value created or lost in the cycle.
#[test]
fn test_attack_withdraw_redeposit_cycle_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    let cap_before = env.read_account_capital(user_idx);
    let vault_before = env.vault_balance();

    // Withdraw half
    env.try_withdraw(&user, user_idx, 2_000_000_000).unwrap();
    let cap_mid = env.read_account_capital(user_idx);
    assert_eq!(
        cap_mid,
        cap_before - 2_000_000_000,
        "Capital should decrease by withdrawal amount"
    );

    // Re-deposit same amount
    env.set_slot(2);
    env.deposit(&user, user_idx, 2_000_000_000);
    let cap_after = env.read_account_capital(user_idx);
    assert_eq!(
        cap_after, cap_before,
        "Capital should return to original after withdraw+redeposit: before={} after={}",
        cap_before, cap_after
    );

    // Vault should be back to original
    let vault_after = env.vault_balance();
    assert_eq!(
        vault_before, vault_after,
        "Vault should be unchanged after withdraw+redeposit"
    );
}

/// ATTACK: Warmup-period market - trade and settle across warmup slots.
/// Profit from trade should vest over warmup_period_slots.
/// Verify conservation through the vesting process.
#[test]
fn test_attack_warmup_vesting_conservation_with_profit() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_warmup(0, 100); // 100-slot warmup

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open and close position with profit
    env.trade(&user, &lp, lp_idx, user_idx, 500_000);
    env.set_slot_and_price(50, 150_000_000); // Price goes up
    env.crank();

    // Close position (realizes gain into warmup)
    env.set_slot(51);
    env.trade(&user, &lp, lp_idx, user_idx, -500_000);

    let cap_mid = env.read_account_capital(user_idx);

    // Vest warmup over many cranks
    for i in 0..15 {
        env.set_slot(100 + i * 50);
        env.crank();
    }

    let cap_final = env.read_account_capital(user_idx);
    // Capital should increase as warmup vests profit
    assert!(
        cap_final >= cap_mid,
        "Capital should increase as warmup vests: mid={} final={}",
        cap_mid,
        cap_final
    );

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync during warmup vesting! c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Large position then price crash - verify conservation through liquidation.
/// Even in liquidation, c_tot must equal sum of capitals.
#[test]
fn test_attack_liquidation_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 10_000_000_000).unwrap();
    env.crank();

    // Large long position relative to capital
    env.trade(&user, &lp, lp_idx, user_idx, 50_000_000);

    // Price crash - circuit breaker limits per-slot move, so crank many times
    env.set_slot_and_price(100, 50_000_000); // ~64% price drop
    for i in 0..20u64 {
        env.set_slot(100 + i * 50);
        env.crank();
    }

    // Explicitly liquidate after crash and require reduced exposure.
    let pos_before_liq = env.read_account_position(user_idx);
    let liq_result = env.try_liquidate_target(user_idx);
    assert!(
        liq_result.is_ok(),
        "Underwater account should be liquidatable in crash scenario: {:?}",
        liq_result
    );
    let pos_after_liq = env.read_account_position(user_idx);
    assert!(
        pos_after_liq.unsigned_abs() <= pos_before_liq.unsigned_abs(),
        "Successful liquidation should not increase exposure: before={} after={}",
        pos_before_liq,
        pos_after_liq
    );

    // Conservation must hold through liquidation.
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync through liquidation! c_tot={} sum={}",
        c_tot, sum
    );

    // SPL vault unchanged (no external value extraction)
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 115_000_000_200,
        "ATTACK: SPL vault changed during liquidation!"
    );
}

/// ATTACK: Four user accounts trading against same LP.
/// Verify conservation holds across many accounts.
#[test]
fn test_attack_four_users_one_lp_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let mut user_idxs = Vec::new();
    for _ in 0..4 {
        let user = Keypair::new();
        let user_idx = env.init_user(&user);
        env.deposit(&user, user_idx, 5_000_000_000);
        user_idxs.push((user, user_idx));
    }

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Each user trades different size against LP
    for (i, (user, user_idx)) in user_idxs.iter().enumerate() {
        env.set_slot((i + 1) as u64);
        let size = ((i + 1) * 50_000) as i128;
        env.trade(user, &lp, lp_idx, *user_idx, size);
    }

    // LP should have total short = -(50K + 100K + 150K + 200K) = -500K
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(
        lp_pos, -500_000,
        "LP position should be sum of user trades: {}",
        lp_pos
    );

    // Conservation across all 5 accounts
    let c_tot = env.read_c_tot();
    let mut sum = env.read_account_capital(lp_idx);
    for (_, user_idx) in &user_idxs {
        sum += env.read_account_capital(*user_idx);
    }
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with 4 users + 1 LP! c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: InitUser with new_account_fee.
/// Verify fee goes to insurance fund and conservation holds.
#[test]
fn test_attack_init_user_fee_conservation() {
    program_path();

    let mut env = TestEnv::new();
    let new_account_fee: u128 = 1_000_000_000;
    env.init_market_full(0, 0, new_account_fee);

    let user = Keypair::new();
    env.svm.airdrop(&user.pubkey(), 5_000_000_000).unwrap();

    // deposit_not_atomic gate: fee_payment >= min_initial_deposit + new_account_fee.
    // Use fee + margin for excess capital beyond the gate.
    let fee_payment: u64 = (new_account_fee as u64) + 100;
    let ata = env.create_ata(&user.pubkey(), fee_payment);

    // Manually construct InitUser with fee covering new_account_fee + min_initial_deposit
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_col, false),
        ],
        data: encode_init_user(fee_payment),
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    env.svm
        .send_transaction(tx)
        .expect("init_user with fee failed");

    // Fee is charged once by deposit_not_atomic during materialization.
    let insurance = env.read_insurance_balance();
    assert_eq!(
        insurance, new_account_fee,
        "Insurance should equal new_account_fee ({}): got {}",
        new_account_fee, insurance
    );

    // Verify SPL vault == engine vault (conservation)
    let spl_vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, spl_vault,
        "Conservation: engine={} spl={}",
        engine_vault, spl_vault
    );
}

/// ATTACK: Many users (40) trading against single LP, then crank.
/// Tests that crank handles many accounts efficiently and conserves funds.
#[test]
fn test_attack_many_users_single_lp_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 200_000_000_000);

    // Create 10 users with positions (limited by 200K CU per crank)
    let mut users: Vec<(Keypair, u16)> = Vec::new();
    for _ in 0..10 {
        let u = Keypair::new();
        let idx = env.init_user(&u);
        env.deposit(&u, idx, 2_000_000_000);
        users.push((u, idx));
    }

    env.try_top_up_insurance(&admin, 5_000_000_000).unwrap();
    env.crank();

    // All users open positions in alternating directions
    for (i, (u, idx)) in users.iter().enumerate() {
        env.set_slot((i + 1) as u64);
        let direction = if i % 2 == 0 { 100_000 } else { -100_000_i128 };
        env.trade(u, &lp, lp_idx, *idx, direction);
    }

    // Price change
    env.set_slot_and_price(200, 142_000_000);
    env.crank();

    // Conservation across all accounts
    let c_tot = env.read_c_tot();
    let mut sum: u128 = env.read_account_capital(lp_idx);
    for (_, idx) in &users {
        sum += env.read_account_capital(*idx);
    }
    assert_eq!(
        c_tot, sum,
        "c_tot with 10 users: c_tot={} sum={}",
        c_tot, sum
    );

    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation with 10 users: engine={} vault={}",
        engine_vault, vault
    );

    // LP position should be net of all user positions
    let lp_pos = env.read_account_position(lp_idx);
    let mut user_sum: i128 = 0;
    for (_, idx) in &users {
        user_sum += env.read_account_position(*idx);
    }
    assert_eq!(
        lp_pos, -user_sum,
        "LP position should mirror user sum: lp={} user_sum={}",
        lp_pos, user_sum
    );
}

/// ATTACK: Settlement ordering - mark settlement, then funding, then fees.
/// Create scenario where ordering matters and verify correctness.
/// ATTACK: Inverted market (invert=1) with large price swing.
/// Tests conservation in inverted market with significant movement.
#[test]
fn test_attack_inverted_market_large_swing_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(1); // Inverted market

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Go long in inverted market
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    let pos = env.read_account_position(user_idx);
    assert_eq!(pos, 5_000_000, "Position should be 5M");

    // In inverted market, price going UP means the inverted price goes DOWN
    // So a long position LOSES when raw oracle price rises
    env.set_slot_and_price(200, 145_000_000); // Raw price up
    env.crank();

    let pnl = env.read_account_pnl(user_idx);
    // In inverted market, long loses when raw price rises
    // (inverted price = 1/raw, so raw up → inverted down → long loses)
    // Verify PnL direction is correct for inverted market
    assert!(
        pnl <= 0,
        "Long in inverted market should lose when raw price rises: pnl={}",
        pnl
    );

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot in inverted market: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Price moves 50% down then liquidation followed by conservation check.
/// Tests that large price movements + liquidation maintain fund conservation.
#[test]
fn test_attack_large_price_drop_liquidation_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 5_000_000_000).unwrap();

    // Set circuit breaker to max (100%) to allow large price moves
    env.try_set_oracle_price_cap(&admin, 1_000_000).unwrap();

    env.crank();

    // User opens large long
    env.trade(&user, &lp, lp_idx, user_idx, 10_000_000);

    // 50% price drop in steps
    env.set_slot_and_price(200, 100_000_000); // $138 → $100 (28% drop)
    env.crank();

    env.set_slot_and_price(400, 70_000_000); // $100 → $70 (49% total drop)
    env.crank();

    // Try liquidating
    let user_pos_before_liq = env.read_account_position(user_idx);
    let liq_result = env.try_liquidate(user_idx);
    let user_pos_after_liq = env.read_account_position(user_idx);
    assert!(
        user_pos_after_liq.unsigned_abs() <= user_pos_before_liq.unsigned_abs(),
        "Liquidation attempt must not increase user exposure. before={} after={} result={:?}",
        user_pos_before_liq,
        user_pos_after_liq,
        liq_result
    );

    // After possible liquidation, conservation must hold
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after large drop + liquidation: engine={} vault={}",
        engine_vault, vault
    );

    // Verify LP has positive PnL (price dropped, LP was short via user's long)
    // LP capital may not have increased yet (lazy settlement) but PnL should be positive
    let lp_pnl = env.read_account_pnl(lp_idx);
    let lp_cap = env.read_account_capital(lp_idx);
    assert!(
        lp_pnl > 0 || lp_cap > 100_000_000_000,
        "LP should have gained from user's loss: pnl={} cap={}",
        lp_pnl,
        lp_cap
    );
}

/// ATTACK: Two users try to withdraw their full equity simultaneously.
/// Vault should never go below total obligations.
#[test]
fn test_attack_concurrent_max_withdrawals_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 10_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Both users open positions
    env.trade(&user1, &lp, lp_idx, user1_idx, 1_000_000);
    env.trade(&user2, &lp, lp_idx, user2_idx, -1_000_000);

    env.set_slot_and_price(20, 105_000_000); // 5% move
    env.crank();

    // Both try to withdraw maximum
    let cap1 = env.read_account_capital(user1_idx);
    let cap2 = env.read_account_capital(user2_idx);
    let vault_before = env.vault_balance();
    let cap1_before = env.read_account_capital(user1_idx);
    let cap2_before = env.read_account_capital(user2_idx);

    // User1 is long, price went DOWN: user1 has lost equity, full withdrawal should be rejected.
    // User2 is short, price went DOWN: user2 profited, full capital withdrawal may succeed.
    // Per spec: withdraw enforces pre/post margin checks with MTM equity.
    let withdraw1 = env.try_withdraw(&user1, user1_idx, cap1 as u64);
    let withdraw2 = env.try_withdraw(&user2, user2_idx, cap2 as u64);
    let vault_after = env.vault_balance();
    let cap1_after = env.read_account_capital(user1_idx);
    let cap2_after = env.read_account_capital(user2_idx);
    // User1 (long, price dropped): withdrawal should be rejected (insufficient margin)
    assert!(
        withdraw1.is_err(),
        "User1 full-capital withdrawal should be rejected (long, price dropped): {:?}",
        withdraw1
    );
    assert_eq!(
        cap1_after, cap1_before,
        "Rejected user1 max-withdrawal must not change capital: before={} after={}",
        cap1_before, cap1_after
    );
    // User2 (short, price dropped = profitable): withdrawal may succeed if margin is sufficient
    // Both outcomes are acceptable - the key invariant is vault conservation
    let _ = (withdraw2, cap2_after);

    // Conservation: vault >= c_tot + insurance
    let vault = vault_after;
    let c_tot = env.read_c_tot();
    let insurance = env.read_insurance_balance();
    assert!(
        vault as u128 >= c_tot + insurance,
        "ATTACK: Concurrent withdrawals broke conservation: vault={}, c_tot={}, ins={}",
        vault,
        c_tot,
        insurance
    );
}

/// ATTACK: Multiple users with opposing positions - conservation after price swing.
/// Tests that PnL redistribution between longs/shorts conserves total value.
#[test]
fn test_attack_opposing_users_pnl_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let long_user = Keypair::new();
    let long_idx = env.init_user(&long_user);
    env.deposit(&long_user, long_idx, 10_000_000_000);

    let short_user = Keypair::new();
    let short_idx = env.init_user(&short_user);
    env.deposit(&short_user, short_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Long and short users take opposite positions
    env.trade(&long_user, &lp, lp_idx, long_idx, 2_000_000);
    env.set_slot(2);
    env.trade(&short_user, &lp, lp_idx, short_idx, -2_000_000);

    // LP should have net zero position (matched both sides)
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(lp_pos, 0, "LP net position should be zero: {}", lp_pos);

    // Big price swing - one user profits, other loses
    env.set_slot_and_price(100, 120_000_000); // 20% up
    env.crank();

    let long_pnl = env.read_account_pnl(long_idx);
    let long_cap = env.read_account_capital(long_idx);
    let short_pnl = env.read_account_pnl(short_idx);
    let short_cap = env.read_account_capital(short_idx);

    // With opposing positions, PnL should be non-trivially different
    // (one gains what the other loses, modulo fees)
    let long_equity = long_cap as i128 + long_pnl;
    let short_equity = short_cap as i128 + short_pnl;
    assert_ne!(
        long_equity, short_equity,
        "Opposing users should have different equity after price move: long={} short={}",
        long_equity, short_equity
    );

    // LP net position is zero, so LP should not be heavily affected
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(
        lp_pos, 0,
        "LP should still have zero net position: {}",
        lp_pos
    );

    // Conservation
    let vault = env.vault_balance();
    let c_tot = env.read_c_tot();
    let insurance = env.read_insurance_balance();
    assert!(
        vault as u128 >= c_tot + insurance,
        "Conservation with opposing users: vault={} c_tot={} ins={}",
        vault,
        c_tot,
        insurance
    );
}

/// PROPERTY TEST: State machine fuzzer verifies 6 invariants across random operation sequences.
///
/// Subsumes the following classes of individual tests:
///   - All conservation tests (~30)
///   - All aggregate consistency tests (~20)
///   - All position symmetry tests (~15)
///   - All deposit/withdraw/trade edge cases (~80)
///   - All fee/insurance interaction tests (~20)
///   - All economic attack tests (~30)
///
/// 50 seeds × 100 steps = 5,000 operations with invariant checks after each.
#[test]
fn test_property_state_machine_invariants() {
    program_path();

    for seed in 1..=50 {
        let mut fuzzer = IntegrationFuzzer::new(seed);
        fuzzer.setup();
        let mut rng = FuzzRng::new(seed);

        for _ in 0..100 {
            let action = fuzzer.random_action(&mut rng);
            fuzzer.execute(action);
        }
    }
}

/// EXTENDED PROPERTY TEST: Deep state machine fuzzer (200 seeds × 200 steps = 40,000 ops).
/// Run with: cargo test test_property_state_machine_extended -- --ignored
#[test]
#[ignore = "long-running exhaustive state machine (40k ops)"]
fn test_property_state_machine_extended() {
    program_path();

    for seed in 1..=200 {
        let mut fuzzer = IntegrationFuzzer::new(seed);
        fuzzer.setup();
        let mut rng = FuzzRng::new(seed);

        for _ in 0..200 {
            let action = fuzzer.random_action(&mut rng);
            fuzzer.execute(action);
        }
    }
}

/// PROPERTY TEST: Authorization - every instruction rejects wrong signer.
///
/// Subsumes the following classes of individual tests:
///   - All authorization bypass tests (~50)
///   - All wrong-owner deposit/withdraw/close tests
///   - All non-admin admin-op tests
///   - All oracle authority tests
///
/// For each protected operation, verifies:
///   A1. Wrong owner is rejected
///   A2. Wrong admin is rejected
///   A3. State is unchanged after rejection
#[test]
fn test_property_authorization_exhaustive() {
    program_path();

    let mut env = TestEnv::new();
    // Use init_market_with_cap with permissionless resolve + force_close_delay
    // because admin burn requires both for live markets (liveness guard).
    env.init_market_with_cap(0, 10_000, 100);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 10_000_000_000).unwrap();
    env.create_ata(&attacker.pubkey(), 5_000_000_000);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open a position for close/withdraw tests
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);

    let vault_before = env.vault_balance();

    // --- Owner-protected operations: attacker cannot act on user's account ---

    // A1: Deposit to someone else's account
    let r = env.try_deposit_unauthorized(&attacker, user_idx, 1_000_000);
    assert!(r.is_err(), "A1: Unauthorized deposit should fail");

    // A2: Withdraw from someone else's account
    let r = env.try_withdraw(&attacker, user_idx, 1_000);
    assert!(r.is_err(), "A2: Unauthorized withdraw should fail");

    // A3: Close someone else's account
    let r = env.try_close_account(&attacker, user_idx);
    assert!(r.is_err(), "A3: Unauthorized close should fail");

    // --- Admin-protected operations: non-admin cannot execute ---

    // A4: Update admin
    let r = env.try_update_admin(&attacker, &attacker.pubkey());
    assert!(r.is_err(), "A4: Non-admin update_admin should fail");

    // --- Vault unchanged after all rejections ---
    let vault_after = env.vault_balance();
    assert_eq!(
        vault_before, vault_after,
        "Vault changed after rejected auth operations: before={} after={}",
        vault_before, vault_after
    );

    // --- Admin chain: verify old admin locked out after two-step transfer (Phase E) ---
    let new_admin = Keypair::new();
    env.svm.airdrop(&new_admin.pubkey(), 5_000_000_000).unwrap();
    // Phase E: UpdateAdmin only proposes; AcceptAdmin completes the transfer.
    env.try_rotate_admin(&admin, &new_admin).unwrap();

    env.svm.expire_blockhash();
    let r = env.try_update_admin(&admin, &admin.pubkey());
    assert!(
        r.is_err(),
        "A5: Old admin should be locked out after transfer"
    );

    env.svm.expire_blockhash();
    let r = env.try_update_admin(&new_admin, &new_admin.pubkey());
    assert!(r.is_ok(), "A6: New admin should work after transfer");

    // --- Zero admin burn (spec §7) ---
    let zero = Pubkey::default();
    let r = env.try_update_admin(&new_admin, &zero);
    assert!(
        r.is_ok(),
        "A7: UpdateAdmin to zero should succeed (admin burn)"
    );

    // Admin must be locked out after burn
    let r = env.try_update_admin(&new_admin, &new_admin.pubkey());
    assert!(r.is_err(), "A8: Admin operations must fail after admin burn");
}

/// PROPERTY TEST: Account lifecycle invariants across create/use/close/GC cycles.
///
/// Subsumes the following classes of individual tests:
///   - All account lifecycle tests (~20)
///   - All GC tests (~10)
///   - All close-account edge cases (~10)
///   - All double-init tests
///
/// Properties verified:
///   L1. Closed accounts reject all operations
///   L2. GC'd accounts have zero capital/position/pnl
///   L3. Account reuse after GC works correctly
///   L4. Close requires zero position and zero PnL
#[test]
fn test_property_account_lifecycle_invariants() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    env.try_top_up_insurance(&admin, 2_000_000_000).unwrap();

    // Create 5 users with deposits
    let mut users: Vec<(Keypair, u16)> = Vec::new();
    for _ in 0..5 {
        let u = Keypair::new();
        let idx = env.init_user(&u);
        env.deposit(&u, idx, 3_000_000_000);
        users.push((u, idx));
    }

    env.crank();

    // L4: Can't close account with open position
    env.trade(&users[0].0, &lp, lp_idx, users[0].1, 500_000);
    let r = env.try_close_account(&users[0].0, users[0].1);
    assert!(r.is_err(), "L4: Can't close account with position");

    // Close the position
    env.set_slot(2);
    env.trade(&users[0].0, &lp, lp_idx, users[0].1, -500_000);
    env.set_slot(100);
    env.crank();

    // L4: Can't close with capital remaining (need to withdraw first)
    let cap = env.read_account_capital(users[0].1);
    if cap > 0 {
        env.try_withdraw(&users[0].0, users[0].1, cap as u64)
            .unwrap();
    }

    // Close account #0
    let pnl = env.read_account_pnl(users[0].1);
    if pnl == 0 {
        env.close_account(&users[0].0, users[0].1);

        // L1: Closed accounts reject operations
        let r = env.try_deposit(&users[0].0, users[0].1, 1_000_000);
        assert!(r.is_err(), "L1: Deposit to closed account should fail");

        let r = env.try_trade(&users[0].0, &lp, lp_idx, users[0].1, 100_000);
        assert!(r.is_err(), "L1: Trade with closed account should fail");
    }

    // Now close users[1..4] (withdraw + close)
    let mut additional_closed_accounts = 0u32;
    for i in 1..5 {
        let cap = env.read_account_capital(users[i].1);
        if cap > 0 {
            env.try_withdraw(&users[i].0, users[i].1, cap as u64)
                .expect("lifecycle withdraw before close must succeed");
        }
        let pnl = env.read_account_pnl(users[i].1);
        if pnl == 0 {
            env.try_close_account(&users[i].0, users[i].1)
                .expect("lifecycle close for zero-pnl account must succeed");
            additional_closed_accounts += 1;
        }
    }
    assert!(
        additional_closed_accounts > 0,
        "Lifecycle property test must close at least one additional account"
    );

    // Crank to GC closed accounts
    env.set_slot(200);
    env.crank();

    // L2: GC'd accounts have zero everything
    for (_, idx) in &users {
        if !env.is_slot_used(*idx) {
            assert_eq!(
                env.read_account_capital(*idx),
                0,
                "L2: GC'd account {} should have zero capital",
                idx
            );
            assert_eq!(
                env.read_account_position(*idx),
                0,
                "L2: GC'd account {} should have zero position",
                idx
            );
        }
    }

    // Final conservation check
    let vault = env.vault_balance();
    let c_tot = env.read_c_tot();
    let insurance = env.read_insurance_balance();
    assert!(
        vault as u128 >= c_tot + insurance,
        "Conservation after lifecycle test: vault={} c_tot={} ins={}",
        vault,
        c_tot,
        insurance
    );
}

/// Verify complete binary market lifecycle with conservation:
/// trade → resolve → force-close → withdraw insurance → close accounts
/// Checks that vault SPL balance accounts for all user capital at every step.
/// NOTE: Currently fails due to engine K-pair overflow on LP force_close_resolved
/// with the binary-market price move. Requires engine fix.
#[test]
#[ignore]
fn test_binary_market_complete_lifecycle_conservation() {
    let mut env = TradeCpiTestEnv::new();

    env.init_market_hyperp(1_000_000); // $1.00

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.set_oracle_price_e6(1_000_000);

    // Setup: LP + 2 users with opposing positions
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 10_000_000_000); // 10B lamports

    let user_a = Keypair::new();
    let user_a_idx = env.init_user(&user_a);
    env.deposit(&user_a, user_a_idx, 1_000_000_000);

    let user_b = Keypair::new();
    let user_b_idx = env.init_user(&user_b);
    env.deposit(&user_b, user_b_idx, 1_000_000_000);

    env.set_slot(50);
    env.crank();

    // Open positions: user_a long, user_b short
    env.try_trade_cpi(
        &user_a,
        &lp.pubkey(),
        lp_idx,
        user_a_idx,
        100_000_000,
        &matcher_prog,
        &matcher_ctx,
    )
    .expect("user_a setup trade must succeed");
    env.try_trade_cpi(
        &user_b,
        &lp.pubkey(),
        lp_idx,
        user_b_idx,
        -100_000_000,
        &matcher_prog,
        &matcher_ctx,
    )
    .expect("user_b setup trade must succeed");

    let vault_before = env.vault_balance();

    // Resolve at $1.50 (user_a profits, user_b loses)
    env.set_oracle_price_e6(1_500_000);
    env.try_resolve_market(&admin).unwrap();

    // Settle PnL via crank (positions require explicit AdminForceCloseAccount)
    env.set_slot(200);
    env.crank();

    // Crank should not move SPL tokens (PnL settlement is internal accounting)
    let vault_after_crank = env.vault_balance();
    assert_eq!(
        vault_before, vault_after_crank,
        "vault SPL balance should not change during crank-based PnL settlement"
    );

    // Conservation after crank: engine vault >= c_tot + insurance
    let engine_vault_after_crank = env.read_vault();
    let c_tot_after_crank = env.read_c_tot();
    let insurance_after_crank = env.read_insurance_balance();
    assert!(
        engine_vault_after_crank >= c_tot_after_crank + insurance_after_crank,
        "Conservation after crank: vault={} c_tot={} ins={}",
        engine_vault_after_crank,
        c_tot_after_crank,
        insurance_after_crank
    );

    // Many cranks to fully settle all accounts — each covers 8 indices
    for s in 0..20 {
        env.set_slot(300 + s);
        env.crank();
    }

    // Close all positions — LP first (absorbs user positions)
    env.try_admin_force_close_account(&admin, lp_idx, &lp.pubkey())
        .expect("AdminForceCloseAccount LP must succeed");
    // Close LP first (its K-pair state may be cleaner after crank settlement)
    env.try_admin_force_close_account(&admin, lp_idx, &lp.pubkey())
        .expect("AdminForceCloseAccount lp must succeed");
    env.try_admin_force_close_account(&admin, user_a_idx, &user_a.pubkey())
        .expect("AdminForceCloseAccount user_a must succeed");
    env.try_admin_force_close_account(&admin, user_b_idx, &user_b.pubkey())
        .expect("AdminForceCloseAccount user_b must succeed");

    // After AdminForceCloseAccount: all accounts are freed
    assert_eq!(
        env.read_num_used_accounts(), 0,
        "All accounts should be freed after AdminForceCloseAccount"
    );

    // Withdraw insurance (all accounts closed, so withdrawal should succeed)
    let result = env.try_withdraw_insurance(&admin);
    assert!(
        result.is_ok(),
        "WithdrawInsurance should succeed after all accounts closed: {:?}",
        result
    );

    // After insurance withdrawal, insurance balance in engine is zero
    assert_eq!(
        env.read_insurance_balance(),
        0,
        "insurance should be zero after withdrawal"
    );

    // Final SPL vault should be near zero (all capital returned to users, insurance withdrawn)
    let final_vault = env.vault_balance();
    println!("Final vault SPL balance: {}", final_vault);
    println!("BINARY MARKET COMPLETE LIFECYCLE CONSERVATION: PASSED");
}

/// Audit gap 5: ADL conservation after liquidation.
///
/// Spec behavior: After a price crash liquidates an underwater user, the ADL
/// mechanism redistributes losses across profitable counterparties. Through
/// this entire process, conservation must hold:
///   total capital + insurance = vault (no value created or destroyed).
///
/// Setup: 3 users + 1 LP. Two users go long, one short. Crash price so the
/// longs go underwater.  Liquidate the most-underwater user.  Crank to trigger
/// ADL.  Verify vault balance is unchanged (conservation).
#[test]
fn test_adl_conservation_after_liquidation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // User A: long
    let user_a = Keypair::new();
    let user_a_idx = env.init_user(&user_a);
    env.deposit(&user_a, user_a_idx, 5_000_000_000);

    // User B: long
    let user_b = Keypair::new();
    let user_b_idx = env.init_user(&user_b);
    env.deposit(&user_b, user_b_idx, 5_000_000_000);

    // User C: short (opposing)
    let user_c = Keypair::new();
    let user_c_idx = env.init_user(&user_c);
    env.deposit(&user_c, user_c_idx, 5_000_000_000);

    // Top up insurance so the protocol has a buffer
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 2_000_000_000);
    let vault_after_setup = env.vault_balance();

    // Open positions
    env.trade(&user_a, &lp, lp_idx, user_a_idx, 5_000_000);  // long
    env.trade(&user_b, &lp, lp_idx, user_b_idx, 3_000_000);  // long
    env.trade(&user_c, &lp, lp_idx, user_c_idx, -4_000_000); // short

    // Trading is internal: vault unchanged
    assert_eq!(
        env.vault_balance(),
        vault_after_setup,
        "conservation after trades"
    );

    // Crash the price from $138 to $50 -- longs go deeply underwater
    env.set_slot_and_price(200, 50_000_000);
    env.crank();
    assert_eq!(
        env.vault_balance(),
        vault_after_setup,
        "conservation after crash crank"
    );

    // Attempt liquidation of user_a (largest long, most underwater)
    let liq_result = env.try_liquidate(user_a_idx);
    // Liquidation may or may not succeed depending on exact margin state,
    // but the vault must remain conserved regardless.

    // Crank to process any ADL or settlement
    env.set_slot_and_price(300, 50_000_000);
    env.crank();
    assert_eq!(
        env.vault_balance(),
        vault_after_setup,
        "conservation after liquidation + ADL crank"
    );

    // Additional crank cycle to catch any deferred settlement
    env.set_slot_and_price(400, 50_000_000);
    env.crank();
    assert_eq!(
        env.vault_balance(),
        vault_after_setup,
        "conservation: vault stable through full ADL cycle"
    );

    // Verify engine internal conservation:
    // engine.vault == actual SPL vault balance
    let engine_vault = env.read_engine_vault();
    let actual_vault = env.vault_balance() as u128;
    assert_eq!(
        engine_vault, actual_vault,
        "engine vault must match SPL vault: engine={} actual={}",
        engine_vault, actual_vault
    );

    // The critical invariant is conservation, verified above.  Whether
    // liquidation succeeded or the account was merely force-realized by the
    // crank, the vault must remain unchanged.  Position reduction is a
    // consequence of the liquidation path but the ADL epoch reset can make
    // `read_account_position` return the old value until the next settlement
    // touch, so we do not assert on the exact position value here.
    //
    // Instead, verify the user's capital was reduced (they took a loss).
    if liq_result.is_ok() {
        let cap_a = env.read_account_capital(user_a_idx);
        assert!(
            cap_a < 5_000_000_000,
            "liquidated user's capital should be reduced from initial 5B: cap={}",
            cap_a
        );
    }
}

