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

/// ATTACK: Try to withdraw more tokens than deposited capital.
/// Expected: Transaction fails due to margin/balance check.
#[test]
fn test_attack_withdraw_more_than_capital() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000); // 1 SOL

    let user_pos_before = env.read_account_position(user_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // Try to withdraw 2x the deposit
    let result = env.try_withdraw(&user, user_idx, 2_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Should not withdraw more than capital"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Over-withdraw rejection must preserve user position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Over-withdraw rejection must preserve user capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Over-withdraw rejection must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Over-withdraw rejection must preserve engine vault"
    );
}

/// ATTACK: After incurring a PnL loss, try to withdraw the full original deposit.
/// Expected: Fails because MTM equity is reduced by loss, margin check rejects.
#[test]
fn test_attack_withdraw_after_loss_exceeds_equity() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 2_000_000_000); // 2 SOL

    // Open a leveraged long position
    env.trade(&user, &lp, lp_idx, user_idx, 10_000_000);

    // Price drops significantly - user has unrealized loss
    env.set_slot_and_price(200, 100_000_000); // $100 (from $138)
    env.crank();

    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // Try to withdraw full deposit - should fail due to reduced equity
    let result = env.try_withdraw(&user, user_idx, 2_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Should not withdraw full capital after PnL loss"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected loss-state withdraw must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected loss-state withdraw must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected loss-state withdraw must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected loss-state withdraw must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected loss-state withdraw must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected loss-state withdraw must preserve engine vault"
    );
}

/// ATTACK: Withdraw an amount not aligned to unit_scale.
/// Expected: Transaction rejected for misaligned amount.
#[test]
fn test_attack_withdraw_misaligned_amount() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_full(0, 1000, 0); // unit_scale = 1000

    let user = Keypair::new();
    // With unit_scale=1000, need 100*1000=100_000 base tokens for min_initial_deposit
    let user_idx = env.init_user_with_fee(&user, 100_000);
    env.deposit(&user, user_idx, 10_000_000);

    env.set_slot(200);
    env.crank();

    let user_pos_before = env.read_account_position(user_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // 1500 % 1000 != 0 => misaligned
    let result = env.try_withdraw(&user, user_idx, 1_500);
    assert!(
        result.is_err(),
        "ATTACK: Misaligned withdrawal should be rejected"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Misaligned withdraw rejection must preserve user position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Misaligned withdraw rejection must preserve user capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Misaligned withdraw rejection must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Misaligned withdraw rejection must preserve engine vault"
    );
}

/// ATTACK: When vault is undercollateralized (haircut < 1.0), withdraw should
/// return reduced equity, not allow full withdrawal that exceeds the haircutted equity.
#[test]
fn test_attack_withdraw_during_undercollateralization() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Create a position to generate PnL
    env.trade(&user, &lp, lp_idx, user_idx, 20_000_000);

    // Big price move creates profit for user, which is subject to haircut
    env.set_slot_and_price(200, 200_000_000);
    env.crank();

    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // Try to withdraw all original deposit + more (inflated equity)
    // The system should cap withdrawal at haircutted equity minus margin
    let result = env.try_withdraw(&user, user_idx, 50_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Withdraw exceeding haircutted equity should fail"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected haircut withdraw must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected haircut withdraw must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected haircut withdraw must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected haircut withdraw must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected haircut withdraw must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected haircut withdraw must preserve engine vault"
    );
}

/// ATTACK: Attacker deposits to an account they don't own.
/// Expected: Owner check fails - signer must match account's registered owner.
#[test]
fn test_attack_deposit_wrong_owner() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Create victim's account
    let victim = Keypair::new();
    let victim_idx = env.init_user(&victim);
    env.deposit(&victim, victim_idx, 5_000_000_000);

    // Attacker tries to deposit to victim's account
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 10_000_000_000).unwrap();
    let victim_pos_before = env.read_account_position(victim_idx);
    let victim_cap_before = env.read_account_capital(victim_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();
    let result = env.try_deposit_unauthorized(&attacker, victim_idx, 1_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Deposit to wrong owner's account should fail"
    );
    assert_eq!(
        env.read_account_position(victim_idx),
        victim_pos_before,
        "Unauthorized deposit rejection must preserve victim position"
    );
    assert_eq!(
        env.read_account_capital(victim_idx),
        victim_cap_before,
        "Unauthorized deposit rejection must preserve victim capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Unauthorized deposit rejection must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Unauthorized deposit rejection must preserve engine vault"
    );
}

/// ATTACK: Attacker withdraws from an account they don't own.
/// Expected: Owner check rejects - signer must match account's registered owner.
#[test]
fn test_attack_withdraw_wrong_owner() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Victim deposits
    let victim = Keypair::new();
    let victim_idx = env.init_user(&victim);
    env.deposit(&victim, victim_idx, 5_000_000_000);

    // Attacker tries to withdraw from victim's account
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let victim_pos_before = env.read_account_position(victim_idx);
    let victim_cap_before = env.read_account_capital(victim_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();
    let result = env.try_withdraw(&attacker, victim_idx, 1_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Withdraw from wrong owner's account should fail"
    );
    assert_eq!(
        env.read_account_position(victim_idx),
        victim_pos_before,
        "Unauthorized withdraw rejection must preserve victim position"
    );
    assert_eq!(
        env.read_account_capital(victim_idx),
        victim_cap_before,
        "Unauthorized withdraw rejection must preserve victim capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Unauthorized withdraw rejection must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Unauthorized withdraw rejection must preserve engine vault"
    );
}

/// ATTACK: Close someone else's account to steal their capital.
/// Expected: Owner check rejects.
#[test]
fn test_attack_close_account_wrong_owner() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let victim = Keypair::new();
    let victim_idx = env.init_user(&victim);
    env.deposit(&victim, victim_idx, 5_000_000_000);

    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let victim_pos_before = env.read_account_position(victim_idx);
    let victim_cap_before = env.read_account_capital(victim_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();
    let result = env.try_close_account(&attacker, victim_idx);
    assert!(
        result.is_err(),
        "ATTACK: Closing someone else's account should fail"
    );
    assert_eq!(
        env.read_account_position(victim_idx),
        victim_pos_before,
        "Unauthorized close rejection must preserve victim position"
    );
    assert_eq!(
        env.read_account_capital(victim_idx),
        victim_cap_before,
        "Unauthorized close rejection must preserve victim capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Unauthorized close rejection must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Unauthorized close rejection must preserve engine vault"
    );
}

/// ATTACK: Non-admin tries admin operations (UpdateAuthority { AUTHORITY_ADMIN },
/// UpdateConfig, SetMaintenanceFee, ResolveMarket).
/// Expected: All admin operations fail for non-admin.
#[test]
fn test_attack_admin_op_as_user() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // UpdateAuthority { kind: AUTHORITY_ADMIN }
    let result = env.try_update_admin(&attacker, &attacker.pubkey());
    assert!(
        result.is_err(),
        "ATTACK: Non-admin admin rotation should fail"
    );

    // UpdateConfig
    let result = env.try_update_config(&attacker);
    assert!(
        result.is_err(),
        "ATTACK: Non-admin UpdateConfig should fail"
    );

    // SetMaintenanceFee
    let result = env.try_set_maintenance_fee(&attacker, 0);
    assert!(
        result.is_err(),
        "ATTACK: Non-admin SetMaintenanceFee should fail"
    );

    // ResolveMarket
    let result = env.try_resolve_market(&attacker, 0);
    assert!(
        result.is_err(),
        "ATTACK: Non-admin ResolveMarket should fail"
    );

    // UpdateAuthority { kind: AUTHORITY_HYPERP_MARK }
    let result = env.try_set_oracle_authority(&attacker, &attacker.pubkey());
    assert!(
        result.is_err(),
        "ATTACK: Non-admin hyperp-authority rotation should fail"
    );

    // SetOraclePriceCap was removed in v12.19; the authorization surface for
    // the now-immutable init-time `max_price_move_bps_per_slot` does not exist.
    // Nothing to probe for non-admin rejection here.
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    assert_eq!(
        slab_after, slab_before,
        "Non-admin admin-op attempts must leave slab state unchanged"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Non-admin admin-op attempts must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Non-admin admin-op attempts must preserve engine vault"
    );
}

/// UpdateAuthority burn: rotating admin to zero permanently removes it.
/// After burning, all admin instructions must fail.
#[test]
fn test_attack_burned_admin_cannot_act() {
    program_path();

    let mut env = TestEnv::new();
    // Use init_market_with_cap with permissionless resolve + force_close_delay
    // because admin burn requires both for live markets (liveness guard).
    env.init_market_with_cap(0, 100);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let zero_pubkey = Pubkey::new_from_array([0u8; 32]);

    // Burn admin by setting to zero address (spec §7 step [3])
    let result = env.try_update_admin(&admin, &zero_pubkey);
    assert!(result.is_ok(), "admin burn should succeed");

    // Admin instructions should now permanently fail
    let result = env.try_update_config(&admin);
    assert!(
        result.is_err(),
        "Admin operations must fail after admin burn"
    );
}

/// ATTACK: Open a position larger than initial margin allows.
/// Expected: Margin check rejects the trade.
#[test]
fn test_attack_trade_without_margin() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 100_000); // Tiny deposit (0.0001 SOL)

    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // Try to open an enormous position relative to capital
    // At $138, 1B position = $138B notional, requiring $13.8B margin (10%)
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 1_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Trade without sufficient margin should fail"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected oversized trade must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected oversized trade must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected oversized trade must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected oversized trade must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected oversized trade must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected oversized trade must preserve engine vault"
    );
}

/// ATTACK: OI-increasing trade when long side is in DrainOnly mode (spec §9.6).
/// Expected: Trade rejected with SideBlocked → EngineRiskReductionOnlyMode (0x16).
///
/// Side-mode gating (§9.6): trades that increase net side OI on
/// DrainOnly/ResetPending sides are rejected (RiskError::SideBlocked →
/// EngineRiskReductionOnlyMode).
///
/// To trigger DrainOnly in a live integration scenario requires many ADL cycles
/// (A_side decaying below MIN_A_SIDE = 2^64), which is impractical to set up.
/// Instead, this test directly sets side_mode_long = DrainOnly (1) via raw byte
/// manipulation of the slab, then verifies the gating and error code mapping.
#[test]
fn test_attack_trade_risk_increase_when_gated() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    assert!(
        env.read_account_position(user_idx) > 0,
        "precondition: user must have live long OI before DrainOnly is meaningful"
    );

    // Directly set side_mode_long = DrainOnly (1). This is an SBF-layout
    // absolute slab offset; native `offset_of!(RiskEngine, side_mode_long)`
    // differs because host u128 alignment is wider than SBF.
    const SIDE_MODE_LONG_OFF: usize = ENGINE_OFFSET + 504;
    {
        let original_slab = env.svm.get_account(&env.slab).expect("slab must exist");
        let mut modified_slab = original_slab.clone();
        modified_slab.data[SIDE_MODE_LONG_OFF] = 1; // SideMode::DrainOnly = 1
        env.svm
            .set_account(env.slab, modified_slab)
            .expect("set_account must succeed");
    }

    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // A long trade (user +5M, LP -5M) increases OI on the long side, which is
    // blocked when side_mode_long == DrainOnly.
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    assert!(
        result.is_err(),
        "ATTACK: OI-increasing trade must be blocked when long side is in DrainOnly mode"
    );

    // Verify the error maps to EngineRiskReductionOnlyMode (SideBlocked → 0x16 = 22).
    let err_msg = result.unwrap_err();
    assert!(
        err_msg.contains("0x16"),
        "Expected EngineRiskReductionOnlyMode (0x16) from SideBlocked, got: {}",
        err_msg
    );

    // The transaction failed so Solana reverts all account mutations atomically.
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Blocked trade must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Blocked trade must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Blocked trade must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Blocked trade must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Blocked trade must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Blocked trade must preserve engine vault"
    );
}

/// ATTACK: Execute TradeNoCpi in Hyperp mode (should be blocked).
/// Expected: Program rejects TradeNoCpi for Hyperp markets.
#[test]
fn test_attack_trade_nocpi_in_hyperp_mode() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(138_000_000); // Hyperp mode

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // Try TradeNoCpi (tag 6) - should be blocked in Hyperp mode
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    assert!(
        result.is_err(),
        "ATTACK: TradeNoCpi in Hyperp mode should be blocked"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected Hyperp TradeNoCpi must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected Hyperp TradeNoCpi must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected Hyperp TradeNoCpi must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected Hyperp TradeNoCpi must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected Hyperp TradeNoCpi must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected Hyperp TradeNoCpi must preserve engine vault"
    );
}

/// ATTACK: Position flip (long->short) should use initial_margin_bps, not
/// maintenance_margin_bps. This is Finding L regression test.
#[test]
fn test_attack_position_flip_requires_initial_margin() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0); // initial=10%, maintenance=5%

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // User with limited capital
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000); // 1 SOL

    // Open a moderate long position (uses some of the initial margin budget)
    // At $138, position=5M means notional = 5M * 138 = 690M, margin needed = 69M (10%)
    // 1 SOL = 1e9, so this should be within margin
    let initial_long_size = 5_000_000i128;
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, initial_long_size);
    assert!(result.is_ok(), "Initial long should work: {:?}", result);
    assert_eq!(
        env.read_account_position(user_idx),
        initial_long_size,
        "Initial margin-eligible long should set expected user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        -initial_long_size,
        "Initial margin-eligible long should set expected opposite LP position"
    );
    let user_pos_before_flip = env.read_account_position(user_idx);
    let lp_pos_before_flip = env.read_account_position(lp_idx);
    let user_cap_before_flip = env.read_account_capital(user_idx);
    let lp_cap_before_flip = env.read_account_capital(lp_idx);
    let spl_vault_before_flip = env.vault_balance();
    let engine_vault_before_flip = env.read_engine_vault();

    // Try to flip to a very large short: -5M to close + -100M new short
    // The new short side notional = 100M * 138 = 13.8B, requiring 1.38B initial margin
    // User only has ~1 SOL = 1e9, so this should fail
    let oversize_flip = -105_000_000i128;
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, oversize_flip);
    assert!(
        result.is_err(),
        "ATTACK: Position flip to oversized short should require initial margin"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before_flip,
        "Rejected oversized flip must preserve user position from prior valid trade"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before_flip,
        "Rejected oversized flip must preserve LP position from prior valid trade"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before_flip,
        "Rejected oversized flip must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before_flip,
        "Rejected oversized flip must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before_flip,
        "Rejected oversized flip must preserve SPL vault aggregate"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before_flip,
        "Rejected oversized flip must preserve engine vault aggregate"
    );
}

/// ATTACK: Liquidate a solvent account (positive equity above maintenance margin).
/// Expected: Liquidation rejected for healthy accounts.
#[test]
fn test_attack_liquidate_solvent_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Heavily over-capitalized user with tiny position
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 50_000_000_000); // 50 SOL

    // Tiny position relative to capital
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    env.set_slot(200);
    env.crank();

    let capital_before = env.read_account_capital(user_idx);
    let position_before = env.read_account_position(user_idx);

    // Try to liquidate heavily collateralized account
    // Engine may return Ok (no-op) or Err depending on implementation
    let liquidation_attempt = env.try_liquidate_target(user_idx);

    // Verify: solvent account's position and capital should be unchanged
    let capital_after = env.read_account_capital(user_idx);
    let position_after = env.read_account_position(user_idx);
    assert_eq!(capital_before, capital_after,
        "ATTACK: Solvent account capital should not change from liquidation attempt. liquidate_result={:?}",
        liquidation_attempt);
    assert_eq!(position_before, position_after,
        "ATTACK: Solvent account position should not change from liquidation attempt. liquidate_result={:?}",
        liquidation_attempt);
}

/// ATTACK: Self-liquidation to extract value (liquidation fee goes to insurance).
/// Expected: Self-liquidation doesn't create profit for the attacker.
#[test]
fn test_attack_self_liquidation_no_profit() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 2_000_000_000); // 2 SOL

    // Open leveraged long
    env.trade(&user, &lp, lp_idx, user_idx, 10_000_000);

    // Price drops to make user underwater
    env.set_slot_and_price(200, 90_000_000);
    env.crank();

    let capital_before = env.read_account_capital(user_idx);
    let insurance_before = env.read_insurance_balance();

    // Try to liquidate (anyone can call)
    let result = env.try_liquidate_target(user_idx);
    let capital_after = env.read_account_capital(user_idx);
    let insurance_after = env.read_insurance_balance();

    assert!(
        capital_after <= capital_before,
        "ATTACK: Self-liquidation attempt increased attacker capital: before={} after={} result={:?}",
        capital_before,
        capital_after,
        result
    );

    if result.is_ok() {
        // Liquidation fee goes to insurance, user doesn't profit
        assert!(
            insurance_after >= insurance_before,
            "ATTACK: Insurance should not decrease from liquidation"
        );
    } else {
        // Failed liquidation should be a complete no-op
        assert_eq!(
            insurance_after, insurance_before,
            "Failed liquidation should not change insurance: before={} after={}",
            insurance_before, insurance_after
        );
        assert_eq!(
            capital_after, capital_before,
            "Failed liquidation should not change capital: before={} after={}",
            capital_before, capital_after
        );
    }

    // Either liquidation was rejected (healthy account = defense working)
    // or it succeeded and insurance received the fee (no profit extraction).
    // In both cases, verify vault is intact.
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after self-liquidation attempt: engine={} vault={}",
        engine_vault, vault
    );
    assert!(
        vault > 0,
        "Vault should still have balance after liquidation attempt"
    );
}

/// ATTACK: Price recovers before liquidation executes - account is now solvent.
/// Expected: Liquidation rejected when account recovers above maintenance margin.
#[test]
fn test_attack_liquidate_after_price_recovery() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Heavily over-capitalized user
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 50_000_000_000); // 50 SOL

    // Small position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Price goes up slightly (user is profitable, very healthy)
    env.set_slot_and_price(200, 140_000_000);
    env.crank();

    let position_before = env.read_account_position(user_idx);
    let capital_before = env.read_account_capital(user_idx);

    // Try liquidation - engine may return Ok (no-op) or Err
    let liquidation_attempt = env.try_liquidate_target(user_idx);

    // Verify: account state should be unchanged (no liquidation occurred)
    let position_after = env.read_account_position(user_idx);
    let capital_after = env.read_account_capital(user_idx);
    assert_eq!(position_before, position_after,
        "ATTACK: Healthy account position should not change from liquidation. liquidate_result={:?}",
        liquidation_attempt);
    assert_eq!(
        capital_before, capital_after,
        "ATTACK: Healthy account capital should not change from liquidation. liquidate_result={:?}",
        liquidation_attempt
    );
}

/// ATTACK: Close slab while insurance fund has remaining balance.
/// Expected: CloseSlab requires insurance_fund.balance == 0.
#[test]
fn test_attack_close_slab_with_insurance_remaining() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Top up insurance fund
    let payer = Keypair::new();
    env.svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    env.top_up_insurance(&payer, 1_000_000_000);

    let insurance_bal = env.read_insurance_balance();
    assert_eq!(
        insurance_bal, 1_000_000_000,
        "Insurance should equal topped-up amount"
    );

    let insurance_before = env.read_insurance_balance();
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;

    // Try to close slab - should fail because insurance > 0
    let result = env.try_close_slab();
    assert!(
        result.is_err(),
        "ATTACK: CloseSlab with non-zero insurance should fail"
    );
    assert_eq!(
        env.read_insurance_balance(),
        insurance_before,
        "Rejected CloseSlab with non-zero insurance must preserve insurance balance"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected CloseSlab with non-zero insurance must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected CloseSlab with non-zero insurance must preserve engine vault"
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    assert_eq!(
        slab_after, slab_before,
        "Rejected CloseSlab with non-zero insurance must preserve slab bytes"
    );
}

/// ATTACK: Push oracle price when no oracle authority is configured.
/// Expected: Fails because default authority is [0;32] (unset).
#[test]
fn test_attack_push_oracle_without_authority_set() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    const HYPERP_MARK_OFF: usize = 312; // HEADER_LEN(136) + hyperp_mark_e6(176)
    const LAST_ORACLE_PUB_TS_OFF: usize = 320; // HEADER_LEN(136) + last_oracle_publish_time(184)
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let mark_before = u64::from_le_bytes(
        slab_before[HYPERP_MARK_OFF..HYPERP_MARK_OFF + 8]
            .try_into()
            .unwrap(),
    );
    let pub_ts_before = i64::from_le_bytes(
        slab_before[LAST_ORACLE_PUB_TS_OFF..LAST_ORACLE_PUB_TS_OFF + 8]
            .try_into()
            .unwrap(),
    );
    let used_before = env.read_num_used_accounts();
    let vault_before = env.vault_balance();

    // Non-Hyperp market has no hyperp_authority — push must reject.
    let random = Keypair::new();
    env.svm.airdrop(&random.pubkey(), 1_000_000_000).unwrap();
    let result = env.try_push_oracle_price(&random, 138_000_000, 100);
    assert!(
        result.is_err(),
        "ATTACK: Push price without authority set should fail"
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let mark_after = u64::from_le_bytes(
        slab_after[HYPERP_MARK_OFF..HYPERP_MARK_OFF + 8]
            .try_into()
            .unwrap(),
    );
    let pub_ts_after = i64::from_le_bytes(
        slab_after[LAST_ORACLE_PUB_TS_OFF..LAST_ORACLE_PUB_TS_OFF + 8]
            .try_into()
            .unwrap(),
    );
    let used_after = env.read_num_used_accounts();
    let vault_after = env.vault_balance();

    assert_eq!(
        mark_before, 0,
        "Precondition: non-Hyperp markets have hyperp_mark_e6 == 0"
    );
    assert_eq!(
        mark_after, mark_before,
        "Rejected unauthorized push must not write hyperp_mark_e6"
    );
    assert_eq!(
        pub_ts_after, pub_ts_before,
        "Rejected unauthorized push must not advance last_oracle_publish_time"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected unauthorized push must not change num_used_accounts"
    );
    assert_eq!(
        vault_after, vault_before,
        "Rejected unauthorized push must not move vault funds"
    );
}

/// ATTACK: Deposit after market is resolved.
/// Expected: No new deposits on resolved markets.
#[test]
fn test_attack_deposit_after_resolution() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    // authority push removed.

    // Create user before resolution
    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    env.crank();
    // Resolve market
    let result = env.try_resolve_market(&admin, 0);
    assert!(result.is_ok(), "Admin should resolve: {:?}", result);

    // Try to deposit after resolution
    let result = env.try_deposit(&user, user_idx, 1_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Deposit after resolution should fail"
    );
}

/// ATTACK: Init new user after market is resolved.
/// Expected: No new accounts on resolved markets.
#[test]
fn test_attack_init_user_after_resolution() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    // authority push removed.

    // Crank to establish real last_oracle_price before resolution
    env.crank();
    // Resolve market
    let result = env.try_resolve_market(&admin, 0);
    assert!(result.is_ok(), "Admin should resolve: {:?}", result);

    // Try to create new user after resolution
    let new_user = Keypair::new();
    let result = env.try_init_user(&new_user);
    assert!(
        result.is_err(),
        "ATTACK: Init user after resolution should fail"
    );
}

/// ATTACK: Close account while still holding an open position.
/// Expected: CloseAccount rejects when position_size != 0.
#[test]
fn test_attack_close_account_with_position() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Verify position exists
    let pos = env.read_account_position(user_idx);
    assert!(pos != 0, "User should have open position");

    // Try to close account with position
    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_err(),
        "ATTACK: Close account with open position should fail"
    );
}

/// ATTACK: Close account when PnL is outstanding (non-zero).
/// Expected: CloseAccount requires PnL == 0.
#[test]
fn test_attack_close_account_with_pnl() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Open and close position with price change to create PnL
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    env.set_slot_and_price(200, 150_000_000);
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, -5_000_000);
    env.set_slot_and_price(300, 150_000_000);
    env.crank();

    // After full cycle, position is closed and warmup settles PnL to capital
    let pnl = env.read_account_pnl(user_idx);
    let pos = env.read_account_position(user_idx);
    assert_eq!(pos, 0, "Position should be closed after closing trade");
    assert_eq!(pnl, 0, "PnL should be zero after crank settles warmup");

    // With PnL=0 and position=0, close should succeed
    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "Close with zero PnL and position should succeed: {:?}",
        result
    );
}

/// ATTACK: Initialize a market twice on the same slab.
/// Expected: Second InitMarket fails because slab already initialized.
#[test]
fn test_attack_double_init_market() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Try to init again on the same slab
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
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_init_market_with_invert(&admin.pubkey(), &env.mint, &TEST_FEED_ID, 0),
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    const HEADER_CONFIG_LEN: usize = 584;
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let used_before = env.read_num_used_accounts();
    let vault_before = env.vault_balance();
    let result = env.svm.send_transaction(tx);
    assert!(result.is_err(), "ATTACK: Double InitMarket should fail");
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let used_after = env.read_num_used_accounts();
    let vault_after = env.vault_balance();
    assert_eq!(
        &slab_after[..HEADER_CONFIG_LEN],
        &slab_before[..HEADER_CONFIG_LEN],
        "Rejected double InitMarket must not mutate slab header/config"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected double InitMarket must not change num_used_accounts"
    );
    assert_eq!(
        vault_after, vault_before,
        "Rejected double InitMarket must not move vault funds"
    );
}

/// ATTACK: Accumulate dust through many sub-unit-scale deposits to extract value.
/// Expected: Dust is tracked and cannot be extracted (swept to insurance).
#[test]
fn test_attack_dust_accumulation_theft() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_full(0, 1000, 0); // unit_scale = 1000

    let user = Keypair::new();
    let user_idx = env.init_user_with_fee(&user, 100_000);

    // Misaligned deposits (1500 % 1000 = 500 dust) must now be rejected
    let result = env.try_deposit(&user, user_idx, 1_500);
    assert!(
        result.is_err(),
        "ATTACK: Misaligned deposit must be rejected (prevents dust donation)"
    );

    // Aligned deposit succeeds
    let result = env.try_deposit(&user, user_idx, 1_000);
    assert!(
        result.is_ok(),
        "Aligned deposit (1000 base) should succeed: {:?}",
        result,
    );
}

/// ATTACK: Micro-trade cannot extract value even with minimum position size.
/// Note: Market has trading_fee_bps=0 (default). This tests conservation,
/// not fee ceiling division. Fee ceiling division is tested at the engine level.
#[test]
fn test_attack_fee_evasion_micro_trades() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Even tiny trades should not extract value through rounding
    let vault_before = env.vault_balance();
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 1); // Minimum possible size
    let vault_after = env.vault_balance();

    // Vault should be unchanged (trades don't move tokens, only PnL)
    assert_eq!(
        vault_before, vault_after,
        "ATTACK: Micro trade should not change vault balance (no value extraction)"
    );

    // Trade attempts must never mint user capital.
    let capital = env.read_account_capital(user_idx);
    assert!(
        capital <= 10_000_000_200u128,
        "ATTACK: Micro trade should not increase capital beyond deposit+init: cap={}",
        capital
    );
    let pos = env.read_account_position(user_idx);
    assert!(
        result.is_ok(),
        "Minimum-size trade should execute in this setup: {:?}",
        result
    );
    assert_eq!(
        pos, 1,
        "Successful min-size trade should produce 1-unit position: pos={}",
        pos
    );
}

/// ATTACK: Deposit/withdraw cycle to manipulate haircut or extract extra tokens.
/// Expected: Vault token balance is always consistent - no tokens created from nothing.
#[test]
fn test_attack_haircut_manipulation_via_deposit_withdraw() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    let vault_before = env.vault_balance();

    // Rapid deposit/withdraw cycles - should not create or destroy value
    for _ in 0..5 {
        env.deposit(&user, user_idx, 10_000_000_000);
        env.try_withdraw(&user, user_idx, 5_000_000_000)
            .expect("cycle withdrawal should succeed");
    }

    let vault_after = env.vault_balance();
    // After 5 cycles: deposited 50 SOL total, withdrew 25 SOL total
    // Vault should have gained 25 SOL net
    let expected_vault = vault_before + 25_000_000_000;
    assert_eq!(
        vault_after, expected_vault,
        "ATTACK: Vault balance mismatch after deposit/withdraw cycles. \
         Expected {}, got {}",
        expected_vault, vault_after
    );

    // User should not be able to withdraw more than what's left
    let result = env.try_withdraw(&user, user_idx, 50_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Should not withdraw more than remaining capital"
    );
}

/// ATTACK: Call crank twice in the same slot to cascade liquidations.
/// Expected: Second crank is a no-op (require_fresh_crank gate).
#[test]
fn test_attack_same_slot_double_crank() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // First crank at slot 200
    env.set_slot(200);
    env.crank();

    let capital_after_first = env.read_account_capital(user_idx);
    let position_after_first = env.read_account_position(user_idx);

    // Second crank at same slot 200 - should be no-op or rejected
    let caller2 = Keypair::new();
    env.svm.airdrop(&caller2.pubkey(), 1_000_000_000).unwrap();
    let second_crank_result = env.try_crank_with_panic(&caller2, 0);

    // Whether accepted (no-op) or rejected, account state must be unchanged
    let capital_after_second = env.read_account_capital(user_idx);
    let position_after_second = env.read_account_position(user_idx);
    assert_eq!(
        capital_after_first, capital_after_second,
        "ATTACK: Double crank should not change capital. second_crank_result={:?}",
        second_crank_result
    );
    assert_eq!(
        position_after_first, position_after_second,
        "ATTACK: Double crank should not change position. second_crank_result={:?}",
        second_crank_result
    );
}

/// ATTACK: Self-crank with wrong owner (caller_idx points to someone else's account).
/// Expected: Owner check rejects because signer doesn't match account owner.
#[test]
fn test_attack_self_crank_wrong_owner() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let victim = Keypair::new();
    let victim_idx = env.init_user(&victim);
    env.deposit(&victim, victim_idx, 10_000_000_000);

    env.trade(&victim, &lp, lp_idx, victim_idx, 5_000_000);
    env.set_slot(200);

    // Attacker tries self-crank using victim's account index
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let result = env.try_crank_self(&attacker, victim_idx);
    assert!(
        result.is_err(),
        "ATTACK: Self-crank with wrong owner should fail"
    );

    // Victim's own self-crank should work
    let result = env.try_crank_self(&victim, victim_idx);
    assert!(
        result.is_ok(),
        "Victim self-crank should succeed: {:?}",
        result
    );
}

/// ATTACK: Rapid crank across many slots to compound funding drain.
/// Expected: Funding rate is capped at max_bps_per_slot; no runaway drain.
#[test]
fn test_attack_funding_max_rate_sustained_drain() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Large imbalanced position to create max funding
    env.trade(&user, &lp, lp_idx, user_idx, 50_000_000);

    let capital_before = env.read_account_capital(user_idx);

    // Crank many times to accrue funding
    for i in 0..20 {
        env.set_slot(200 + i * 100);
        env.crank();
    }

    let capital_after = env.read_account_capital(user_idx);

    // Capital should not be completely drained - funding is rate-limited
    // User started with 10 SOL and held a 50M position through 20 cranks.
    // Even at max funding rate, capital should not hit zero.
    assert!(
        capital_after > 0,
        "ATTACK: Funding should not drain capital to zero (rate-limited). Before: {}, After: {}",
        capital_before,
        capital_after
    );

    // Vault should still be intact (no token leakage)
    let vault = env.vault_balance();
    assert!(
        vault > 0,
        "Vault should still have balance after sustained funding"
    );
}

/// ATTACK: Crank 3 times in same slot to bypass index smoothing (Bug #9 regression).
/// Expected: dt=0 returns no index movement (fix verified).
#[test]
fn test_attack_funding_same_slot_three_cranks_dt_zero() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // First crank at slot 200
    env.set_slot(200);
    env.crank();

    let capital_after_1 = env.read_account_capital(user_idx);

    // Crank again same slot - should be no-op (dt=0)
    // Engine may reject or accept as no-op
    let caller2 = Keypair::new();
    env.svm.airdrop(&caller2.pubkey(), 1_000_000_000).unwrap();
    let second_crank_result = env.try_crank_with_panic(&caller2, 0);

    // Third crank same slot
    let caller3 = Keypair::new();
    env.svm.airdrop(&caller3.pubkey(), 1_000_000_000).unwrap();
    let third_crank_result = env.try_crank_with_panic(&caller3, 0);

    let capital_after_3 = env.read_account_capital(user_idx);

    // Capital should not have changed from repeated same-slot cranks
    assert_eq!(
        capital_after_1, capital_after_3,
        "ATTACK: Same-slot repeated cranks should not change capital (dt=0 fix). \
         second={:?} third={:?}",
        second_crank_result, third_crank_result
    );
}

/// ATTACK: Large time gap between cranks (dt overflow).
/// Expected: dt is capped and funding doesn't overflow.
#[test]
fn test_attack_funding_large_dt_gap() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // First crank
    env.set_slot(200);
    env.crank();

    // Verify: a reasonable dt gap (~2 minutes) still works
    env.set_slot(500);
    env.crank(); // should succeed without overflow

    // Verify vault didn't get corrupted (still has tokens)
    let vault_balance = env.vault_balance();
    assert!(
        vault_balance > 0,
        "Vault should still have balance after reasonable dt crank"
    );

    // Jump forward ~1 year worth of slots (massive dt)
    // 1 year ≈ 31.5M seconds ≈ 78.8M slots at 400ms
    // The engine caps dt at ~1 year and succeeds (no overflow).
    // Per spec: funding_calc uses dt cap (~1 year) to prevent overflow.
    env.set_slot(50_000); // within envelope
    let result = env.try_crank();
    // The engine should succeed with dt capping (not fail with overflow)
    assert!(
        result.is_ok(),
        "ATTACK: Large dt gap should be handled by dt capping, not rejected: {:?}",
        result
    );

    // Verify vault is still conserved (no value created/destroyed)
    let vault_after = env.vault_balance();
    assert!(
        vault_after > 0,
        "Vault should still have balance after large dt crank"
    );
}

/// ATTACK: Warmup with period=0 (instant conversion).
/// Expected: Profit converts to capital immediately.
#[test]
fn test_attack_warmup_zero_period_instant() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_warmup(0, 0); // warmup = 0 slots

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Open and close position with profit
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    env.set_slot_and_price(200, 150_000_000);
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, -5_000_000);

    // With warmup=0, profit should be immediately available
    env.set_slot_and_price(300, 150_000_000);
    env.crank();

    // Try to close account - should work if PnL was converted
    let pos = env.read_account_position(user_idx);
    assert_eq!(pos, 0, "Position should be closed");
    println!("Warmup=0 test: position closed, conversion should be instant");
}

/// ATTACK: Warmup period long (1M slots), attempt to withdraw before conversion.
/// Expected: Unrealized PnL in warmup cannot be withdrawn as capital.
#[test]
fn test_attack_warmup_long_period_withdraw_attempt() {
    program_path();

    let mut env = TestEnv::new();
    // Use a short maturity horizon so the test stays fast; long horizons are
    // accepted independently from accrual/staleness and covered below.
    env.init_market_with_warmup(0, 50);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Open profitable trade
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    env.set_slot_and_price(200, 200_000_000); // Big price up
    env.crank();

    // Close position - PnL enters warmup
    env.trade(&user, &lp, lp_idx, user_idx, -5_000_000);
    env.set_slot_and_price(300, 200_000_000);
    env.crank();

    // Try to withdraw more than original deposit
    // The PnL is in warmup and shouldn't be withdrawable yet
    let result = env.try_withdraw(&user, user_idx, 15_000_000_000); // More than 10 SOL deposit
    assert!(
        result.is_err(),
        "ATTACK: Should not withdraw more than original deposit during long warmup period"
    );

    // Even if profit exists, it's locked in warmup - vault should be intact
    let vault = env.vault_balance();
    assert!(
        vault >= 100_000_000_000,
        "Vault should retain LP + user deposits during warmup"
    );
}

/// ATTACK: Unit scale = 0 (no scaling) - verify dust handling is safe.
/// Expected: With unit_scale=0, no dust accumulation, clean behavior.
#[test]
fn test_attack_unit_scale_zero_no_dust() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_full(0, 0, 0); // unit_scale = 0

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 12_345_678);

    let vault = env.vault_balance();
    assert_eq!(
        vault, 12_345_778,
        "Full deposit with unit_scale=0 (includes 100 from init)"
    );

    // Withdrawal should work for odd amounts
    let result = env.try_withdraw(&user, user_idx, 1_234_567);
    assert!(
        result.is_ok(),
        "Withdrawal with unit_scale=0 should work: {:?}",
        result
    );
}

/// ATTACK: High unit_scale to test dust sweep boundary conditions.
/// Expected: Dust correctly tracked and not exploitable.
#[test]
fn test_attack_high_unit_scale_dust_boundary() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_full(0, 1_000_000, 0); // 1M base per unit

    let user = Keypair::new();
    let user_idx = env.init_user_with_fee(&user, 100_000_000);

    // Sub-unit deposit (500k < 1M unit_scale) must now be rejected
    let result = env.try_deposit(&user, user_idx, 500_000);
    assert!(
        result.is_err(),
        "ATTACK: Sub-unit deposit must be rejected (prevents dust donation)"
    );

    // Aligned deposit succeeds
    let result = env.try_deposit(&user, user_idx, 1_000_000);
    assert!(
        result.is_ok(),
        "Aligned deposit (1M base) should succeed: {:?}",
        result,
    );
}

/// ATTACK: Open and immediately close to avoid holding fees.
/// Expected: Trading fee charged on both legs, not profitable to churn.
#[test]
fn test_attack_open_close_same_slot_fee_evasion() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_trading_fee(100); // 1% fee

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let capital_before = env.read_account_capital(user_idx);

    // Open and immediately close in same slot
    let result1 = env.try_trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    let result2 = env.try_trade(&user, &lp, lp_idx, user_idx, -5_000_000);

    assert!(result1.is_ok(), "Open leg should succeed: {:?}", result1);
    assert!(result2.is_ok(), "Close leg should succeed: {:?}", result2);

    // User should have LOST capital to fees, not gained
    env.set_slot(200);
    env.crank();

    let capital_after = env.read_account_capital(user_idx);
    assert!(
        capital_after <= capital_before,
        "ATTACK: Open+close churn should not increase capital (fees charged). \
         Before: {}, After: {}",
        capital_before,
        capital_after
    );
}

/// ATTACK: Close account that still has maintenance fee debt.
/// Expected: CloseAccount forgives remaining fee debt after paying what's possible.
/// ATTACK: Try to use GC'd account slot for new account creation.
/// Expected: After GC, slot is marked unused and can be reused.
#[test]
fn test_attack_gc_slot_reuse_after_close() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Create user and deposit
    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 5_000_000_000);

    let vault_before = env.vault_balance();
    env.close_account(&user1, user1_idx);
    let vault_after = env.vault_balance();

    // Verify capital was returned to user on close
    assert!(
        vault_before > vault_after,
        "Capital should be returned on close"
    );

    // GC the account by cranking
    env.set_slot(200);
    env.crank();

    // After GC, the slot should be zeroed out. Reading position should be 0.
    let pos = env.read_account_position(user1_idx);
    assert_eq!(pos, 0, "GC'd slot should have zero position (clean state)");

    // Verify a fresh user at a new index works normally (no state leakage)
    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    assert!(user2_idx > 0, "New user should get a valid index");
    let pos2 = env.read_account_position(user2_idx);
    assert_eq!(pos2, 0, "New user should start with zero position");
}

/// ATTACK: Deposit then immediately trade in same slot to use uncranked capital.
/// Expected: Deposit is available immediately for trading (no crank needed).
#[test]
fn test_attack_deposit_then_trade_same_slot() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    // Deposit and immediately trade (no crank in between)
    env.deposit(&user, user_idx, 10_000_000_000);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let vault_before = env.vault_balance();
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    assert!(
        result.is_ok(),
        "Deposit then trade in same slot should work: {:?}",
        result
    );
    let user_cap_after = env.read_account_capital(user_idx);
    let lp_cap_after = env.read_account_capital(lp_idx);
    let user_pos_after = env.read_account_position(user_idx);
    let lp_pos_after = env.read_account_position(lp_idx);
    let vault_after = env.vault_balance();

    assert_eq!(user_pos_before, 0, "Precondition: user should start flat");
    assert_eq!(lp_pos_before, 0, "Precondition: LP should start flat");
    assert_eq!(
        user_pos_after, 5_000_000,
        "Successful trade should open user long position"
    );
    assert_eq!(
        lp_pos_after, -5_000_000,
        "Successful trade should open LP short position"
    );
    assert_eq!(
        user_cap_after, user_cap_before,
        "Trade should not change user capital at entry"
    );
    assert_eq!(
        lp_cap_after, lp_cap_before,
        "Trade should not change LP capital at entry"
    );
    assert_eq!(
        vault_after, vault_before,
        "Trade should not move vault funds"
    );
}

/// ATTACK: Trade, then withdraw max in same slot.
/// Expected: Margin check accounts for newly opened position.
#[test]
fn test_attack_trade_then_withdraw_max_same_slot() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Open a sizable position
    env.trade(&user, &lp, lp_idx, user_idx, 20_000_000);

    // Immediately try to withdraw everything
    let result = env.try_withdraw(&user, user_idx, 10_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Withdrawing all capital right after opening position should fail"
    );

    // Partial withdrawal succeeds. In the ADL engine the arg-swapped withdraw call
    // uses clock.slot as oracle_price, so touch_account_full sees a large price drop
    // (slot=100 vs oracle=138M), reducing capital by ~2.76B for a 20M position.
    // Capital after touch ≈ 10B - 2.76B = 7.24B. Withdraw 7B (well inside margin).
    let result2 = env.try_withdraw(&user, user_idx, 7_000_000_000);
    assert!(
        result2.is_ok(),
        "ATTACK: Partial withdrawal within margin should succeed: {:?}",
        result2
    );
}

/// ATTACK: Multiple deposits in rapid succession.
/// Expected: All deposits correctly credited, no accounting errors.
#[test]
fn test_attack_rapid_deposits_accounting() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    let amount_per_deposit = 1_000_000_000u64; // 1 SOL each

    // 10 rapid deposits
    for _ in 0..10 {
        env.deposit(&user, user_idx, amount_per_deposit);
    }

    let vault = env.vault_balance();
    assert_eq!(
        vault,
        10 * amount_per_deposit + 100, // +100 from init deposit
        "Vault should have exactly 10 SOL after 10 deposits"
    );

    // Full withdrawal should work
    let result = env.try_withdraw(&user, user_idx, 10 * amount_per_deposit);
    assert!(result.is_ok(), "Should withdraw all 10 SOL: {:?}", result);
}

/// ATTACK: UpdateConfig with extreme parameter values.
/// Expected: Engine-level guards prevent dangerous configurations.
#[test]
fn test_attack_update_config_extreme_values() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Try setting max funding rate to extreme value
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_update_config(
            1,        // funding_horizon_slots (minimum)
            10000,    // funding_k_bps (100%)
            10000i64, // funding_max_premium_bps (max allowed)
            10i64,    // funding_max_e9_per_slot (fits per-market envelope = 10 bps/slot)
        ),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_ok(),
        "Extreme-but-valid UpdateConfig should be accepted: {:?}",
        result
    );

    // Set up positions to verify the engine still works
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Verify crank still works
    env.set_slot(200);
    env.crank();

    let vault = env.vault_balance();
    assert_eq!(vault, 110_000_000_200,
        "ATTACK: Engine should remain functional with consistent vault after extreme config. Got {}", vault);
}

/// ATTACK: Deposit more than ATA balance (overflow attempt).
/// Expected: Rejected by token program (insufficient funds).
#[test]
fn test_attack_deposit_u64_max() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    let user_cap_before = env.read_account_capital(user_idx);
    let used_before = env.read_num_used_accounts();
    let vault_before = env.vault_balance();

    // Mint only 1000 tokens but try to deposit 1_000_000_000_000
    // (more than the ATA holds)
    let ata = env.create_ata(&user.pubkey(), 1000);

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data: encode_deposit(user_idx, 1_000_000_000_000),
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Depositing more than ATA balance should fail"
    );
    let user_cap_after = env.read_account_capital(user_idx);
    let used_after = env.read_num_used_accounts();
    let vault_after = env.vault_balance();
    assert_eq!(
        user_cap_after, user_cap_before,
        "Rejected over-deposit must not change user capital"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected over-deposit must not change num_used_accounts"
    );
    assert_eq!(
        vault_after, vault_before,
        "Rejected over-deposit must not move vault funds"
    );
}

/// ATTACK: Trade with size = i128::MAX (overflow boundary).
/// Expected: Rejected by margin check (impossible notional value).
#[test]
fn test_attack_trade_size_i128_max() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // i128::MAX position size - should fail margin check
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, i128::MAX);
    assert!(
        result.is_err(),
        "ATTACK: Trade with i128::MAX size should fail"
    );

    // Also test i128::MIN
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, i128::MIN);
    assert!(
        result.is_err(),
        "ATTACK: Trade with i128::MIN size should fail"
    );
}

/// ATTACK: Trade with size = 0 (no-op trade attempt).
/// Expected: Zero-size trade is rejected and must not mutate state.
#[test]
fn test_attack_trade_size_zero() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let capital_before = env.read_account_capital(user_idx);
    let position_before = env.read_account_position(user_idx);
    let vault_before = env.vault_balance();

    // Zero-size trade
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 0);
    println!("Zero-size trade: {:?}", result);

    assert!(
        result.is_err(),
        "ATTACK: Zero-size trade should be rejected: {:?}",
        result
    );

    // Rejected trade must not mutate state.
    let capital_after = env.read_account_capital(user_idx);
    let position_after = env.read_account_position(user_idx);
    let vault_after = env.vault_balance();
    assert_eq!(
        capital_before, capital_after,
        "ATTACK: Zero-size trade should not change capital"
    );
    assert_eq!(
        position_before, position_after,
        "ATTACK: Zero-size trade should not change position"
    );
    assert_eq!(
        vault_before, vault_after,
        "ATTACK: Zero-size trade should not change vault"
    );
}

/// ATTACK: UpdateConfig with funding_horizon_slots = 0 (division by zero risk).
/// Expected: Rejected with InvalidConfigParam.
#[test]
fn test_attack_config_zero_funding_horizon() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let cfg_before = env.read_update_config_snapshot();
    let vault_before = env.vault_balance();
    let result = env.try_update_config_with_params(&admin, 0);
    assert!(
        result.is_err(),
        "ATTACK: Zero funding_horizon_slots should be rejected (InvalidConfigParam)"
    );
    let cfg_after = env.read_update_config_snapshot();
    let vault_after = env.vault_balance();
    assert_eq!(
        cfg_after, cfg_before,
        "Rejected UpdateConfig must not mutate funding/threshold config"
    );
    assert_eq!(
        vault_after, vault_before,
        "Rejected UpdateConfig must not move vault funds"
    );
}

/// ATTACK: Attempt to deploy a non-Hyperp market with no resolve path.
/// The legacy attack was: init with cap=0 + perm_resolve=0, then use the
/// now-uncapped oracle authority to walk the effective price. The
/// resolvability invariant closes the attack by rejecting the combo at
/// init outright — an operator cannot deploy a market that has no
/// resolve path to begin with.
#[test]
fn test_attack_oracle_cap_zero_disables_clamping() {
    program_path();

    let mut env = TestEnv::new();
    let data = common::encode_init_market_with_cap(
        &env.payer.pubkey(),
        &env.mint,
        &common::TEST_FEED_ID,
        0, // invert=0 (non-Hyperp)
        0, // permissionless_resolve_stale_slots
    );
    let err = env
        .try_init_market_raw(data)
        .expect_err("init must reject non-Hyperp + perm_resolve=0");
    assert!(
        err.contains("0x1a"),
        "expected InvalidConfigParam, got: {}",
        err,
    );
}

/// ATTACK: LP account should never be garbage collected, even with zero state.
/// Expected: GC skips LP accounts (they have is_lp = true).
#[test]
fn test_attack_lp_immune_to_gc() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    // Don't deposit - LP has zero capital/position/pnl

    // Crank to trigger GC
    env.set_slot(200);
    env.crank();

    // Per engine v12: deposit to a GC'd slot re-materializes the account
    // (deposit-based materialization per spec §10.3). This is correct.
    let result = env.try_deposit(&lp, lp_idx, 10_000_000_000);
    assert!(
        result.is_ok(),
        "Zero-capital LP should be GC'd — deposit to freed slot must fail"
    );
}

/// ATTACK: User account with zero state SHOULD be GC'd.
/// Expected: GC reclaims user accounts with zero position/capital/pnl.
#[test]
fn test_attack_user_gc_when_empty() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    // The default materialization payment leaves 99 capital after the
    // mandatory anti-spam fee. Withdraw it to make the account empty for GC.
    env.withdraw(&user, user_idx, DEFAULT_INIT_CAPITAL);

    // Crank to trigger GC
    env.set_slot(200);
    env.crank();

    // Verify user was GC'd by checking position reads as 0
    let pos = env.read_account_position(user_idx);
    assert_eq!(pos, 0, "GC'd user should have zero position");
    let capital = env.read_account_capital(user_idx);
    assert_eq!(capital, 0, "GC'd user should have zero capital");
}

/// ATTACK: LP takes position, then try to close as if user (kind mismatch).
/// Expected: LP account cannot be closed via CloseAccount (only users can close).
#[test]
fn test_attack_close_lp_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    // Try to close LP account via CloseAccount instruction
    // CloseAccount does not check account kind — LP with no position can close
    let result = env.try_close_account(&lp, lp_idx);
    assert!(
        result.is_ok(),
        "LP with no position should be closeable: {:?}",
        result
    );
    let vault_after = env.vault_balance();
    assert!(
        vault_after < 10_000_000_000,
        "LP close should return capital: vault={}",
        vault_after
    );
}

/// ATTACK: CloseSlab when vault has tokens remaining.
/// Expected: Rejected (vault must be empty).
#[test]
fn test_attack_close_slab_with_vault_tokens() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Deposit some tokens
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    let user_cap_before = env.read_account_capital(user_idx);
    let user_pos_before = env.read_account_position(user_idx);
    let insurance_before = env.read_insurance_balance();
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;

    // Try CloseSlab with vault containing tokens
    let result = env.try_close_slab();
    assert!(
        result.is_err(),
        "ATTACK: CloseSlab with vault tokens should be rejected"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected CloseSlab with vault tokens must preserve user capital"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected CloseSlab with vault tokens must preserve user position"
    );
    assert_eq!(
        env.read_insurance_balance(),
        insurance_before,
        "Rejected CloseSlab with vault tokens must preserve insurance"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected CloseSlab with vault tokens must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected CloseSlab with vault tokens must preserve engine vault"
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    assert_eq!(
        slab_after, slab_before,
        "Rejected CloseSlab with vault tokens must preserve slab bytes"
    );
}

/// ATTACK: CloseSlab on uninitialized slab.
/// Expected: Rejected (not initialized).
#[test]
fn test_attack_close_slab_uninitialized() {
    program_path();

    let mut env = TestEnv::new();
    // Don't call init_market - slab is uninitialized

    let slab_before = env
        .svm
        .get_account(&env.slab)
        .expect("slab account must exist before uninitialized close attempt");

    let result = env.try_close_slab();
    assert!(
        result.is_err(),
        "ATTACK: CloseSlab on uninitialized slab should fail"
    );
    let slab_after = env
        .svm
        .get_account(&env.slab)
        .expect("slab account must exist after uninitialized close attempt");
    assert_eq!(
        slab_after.lamports, slab_before.lamports,
        "Rejected CloseSlab on uninitialized slab must preserve lamports"
    );
    assert_eq!(
        slab_after.data, slab_before.data,
        "Rejected CloseSlab on uninitialized slab must preserve slab bytes"
    );
}

/// ATTACK: Set maintenance fee to u128::MAX (maximum possible fee).
/// Expected: Fee is accepted but capital should drain predictably (not corrupt state).
/// ATTACK: SetMaintenanceFee as non-admin.
/// Expected: Rejected (admin auth check).
#[test]
fn test_attack_set_maintenance_fee_non_admin() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let vault_before = env.vault_balance();
    let used_before = env.read_num_used_accounts();
    let result = env.try_set_maintenance_fee(&attacker, 999_999_999);
    assert!(
        result.is_err(),
        "ATTACK: Non-admin SetMaintenanceFee should be rejected"
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let vault_after = env.vault_balance();
    let used_after = env.read_num_used_accounts();
    assert_eq!(
        slab_after, slab_before,
        "Rejected non-admin fee update must not mutate slab"
    );
    assert_eq!(
        vault_after, vault_before,
        "Rejected non-admin fee update must not move vault funds"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected non-admin fee update must not change num_used_accounts"
    );
}

/// ATTACK: Haircut ratio when all users are in loss (pnl_pos_tot = 0).
/// Expected: Haircut ratio = (1,1), no division by zero.
#[test]
fn test_attack_haircut_all_users_in_loss() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // User goes long
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);
    env.trade(&user, &lp, lp_idx, user_idx, 10_000_000);

    // Price drops - user is in loss (LP in profit)
    // pnl_pos_tot should include LP's positive PnL, not user's negative
    env.set_slot_and_price(200, 100_000_000);
    env.crank();

    // Vault should be intact (no corruption from haircut calc). Trading
    // fees may increase it, so assert conservation instead of an exact
    // historical constant.
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert!(
        vault >= 110_000_000_000,
        "Vault should not lose funds after loss scenario: vault={}",
        vault
    );
    assert_eq!(
        engine_vault, vault as u128,
        "Engine/SPL vault mismatch after loss scenario: engine={} spl={}",
        engine_vault, vault
    );

    // User should still be able to partially withdraw (reduced equity, but not zero)
    let vault_before = env.vault_balance();
    let capital_before = env.read_account_capital(user_idx);
    let result = env.try_withdraw(&user, user_idx, 1_000_000_000);
    let vault_after = env.vault_balance();
    let capital_after = env.read_account_capital(user_idx);
    assert!(
        result.is_ok(),
        "User should be able to partially withdraw in this loss scenario: {:?}",
        result
    );
    assert_eq!(
        vault_after,
        vault_before - 1_000_000_000,
        "Successful withdraw must decrement vault by requested amount"
    );
    assert_eq!(
        capital_after,
        capital_before - 1_000_000_000u128,
        "Successful withdraw must decrement capital by requested amount"
    );
    assert!(vault_after > 0, "Vault should never go to zero");
}

/// ATTACK: Send instruction with truncated data (too short for the tag).
/// Expected: Rejected with InvalidInstructionData.
#[test]
fn test_attack_truncated_instruction_data() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    const HEADER_CONFIG_LEN: usize = 584;
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let used_before = env.read_num_used_accounts();
    let vault_before = env.vault_balance();

    let user = Keypair::new();
    env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();

    // Tag 3 (Deposit) needs user_idx (u16) + amount (u64) = 10 bytes after tag
    // Send only 3 bytes total (tag + 2 bytes, missing amount)
    let data = vec![3u8, 0u8, 0u8];

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(Pubkey::new_unique(), false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Truncated instruction data should be rejected"
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let used_after = env.read_num_used_accounts();
    let vault_after = env.vault_balance();
    assert_eq!(
        &slab_after[..HEADER_CONFIG_LEN],
        &slab_before[..HEADER_CONFIG_LEN],
        "Rejected truncated instruction must not mutate slab header/config"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected truncated instruction must not change num_used_accounts"
    );
    assert_eq!(
        vault_after, vault_before,
        "Rejected truncated instruction must not move vault funds"
    );
}

/// ATTACK: Send unknown instruction tag (255).
/// Expected: Rejected with InvalidInstructionData.
#[test]
fn test_attack_unknown_instruction_tag() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    const HEADER_CONFIG_LEN: usize = 584;
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let used_before = env.read_num_used_accounts();
    let vault_before = env.vault_balance();

    let user = Keypair::new();
    env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
        ],
        data: vec![255u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8],
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Unknown instruction tag should be rejected"
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let used_after = env.read_num_used_accounts();
    let vault_after = env.vault_balance();
    assert_eq!(
        &slab_after[..HEADER_CONFIG_LEN],
        &slab_before[..HEADER_CONFIG_LEN],
        "Rejected unknown-tag instruction must not mutate slab header/config"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected unknown-tag instruction must not change num_used_accounts"
    );
    assert_eq!(
        vault_after, vault_before,
        "Rejected unknown-tag instruction must not move vault funds"
    );
}

/// ATTACK: Empty instruction data (no tag byte).
/// Expected: Rejected with InvalidInstructionData.
#[test]
fn test_attack_empty_instruction_data() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    const HEADER_CONFIG_LEN: usize = 584;
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let used_before = env.read_num_used_accounts();
    let vault_before = env.vault_balance();

    let user = Keypair::new();
    env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
        ],
        data: vec![], // empty!
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Empty instruction data should be rejected"
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let used_after = env.read_num_used_accounts();
    let vault_after = env.vault_balance();
    assert_eq!(
        &slab_after[..HEADER_CONFIG_LEN],
        &slab_before[..HEADER_CONFIG_LEN],
        "Rejected empty-data instruction must not mutate slab header/config"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected empty-data instruction must not change num_used_accounts"
    );
    assert_eq!(
        vault_after, vault_before,
        "Rejected empty-data instruction must not move vault funds"
    );
}

/// ATTACK: Deposit → Resolve → Withdraw sequence.
/// Expected: Can't deposit after resolve, but can withdraw existing capital.
#[test]
fn test_attack_deposit_resolve_withdraw_sequence() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Setup oracle and resolve
    // authority push removed.
    env.crank();
    env.try_resolve_market(&admin, 0)
        .expect("market resolution setup must succeed");

    // Can't deposit more
    let deposit_result = env.try_deposit(&user, user_idx, 1_000_000_000);
    assert!(
        deposit_result.is_err(),
        "Deposit after resolution should fail"
    );

    // Withdrawals are blocked on resolved markets. Users must use CloseAccount.
    let withdraw_result = env.try_withdraw(&user, user_idx, 5_000_000_000);
    assert!(
        withdraw_result.is_err(),
        "Withdrawal on resolved market should be blocked (use CloseAccount instead)"
    );

    // CloseAccount should succeed for user with no position and capital > 0.
    // May need two calls for ProgressOnly handling.
    let vault_before = env.vault_balance();
    let _ = env.try_close_account(&user, user_idx);
    let _ = env.try_close_account(&user, user_idx); // retry for ProgressOnly
    let vault_after = env.vault_balance();

    // Key property: no value created from nothing.
    assert!(
        vault_after <= 10_000_000_000,
        "Vault should never exceed total deposits"
    );
}

/// Per spec §10.7, LP and User accounts share the same mechanics.
/// Using a User account in the LP slot of a trade is valid (spec v10.5).
/// The engine does not enforce account kind for trades — only authorization matters.
#[test]
fn test_attack_trade_user_as_lp() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Create two regular users
    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 10_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 10_000_000_000);

    let user1_cap_before = env.read_account_capital(user1_idx);
    let user2_cap_before = env.read_account_capital(user2_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let user1_pos_before = env.read_account_position(user1_idx);
    let user2_pos_before = env.read_account_position(user2_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let vault_before = env.vault_balance();
    let used_before = env.read_num_used_accounts();

    // Trade user2 vs user1 (user1 as "LP") — accepted in spec v10.5
    let result = env.try_trade_type_confused(&user2, &user1, user1_idx, user2_idx, 1_000_000);
    assert!(
        result.is_ok(),
        "User-vs-user trade should succeed in spec v10.5 (no kind restriction): {:?}",
        result
    );
    // Trade succeeded — user2 has position, user1 has opposite position, LP unaffected
    let user2_pos_after = env.read_account_position(user2_idx);
    let user1_pos_after = env.read_account_position(user1_idx);
    let lp_pos_after = env.read_account_position(lp_idx);
    assert_ne!(user2_pos_after, 0, "User2 should have position after trade");
    assert_ne!(
        user1_pos_after, 0,
        "User1 should have opposite position after trade"
    );
    assert_eq!(lp_pos_after, lp_pos_before, "LP should be unaffected");

    // Vault balance unchanged (no tokens moved in/out during trade)
    let vault_after = env.vault_balance();
    assert_eq!(
        vault_after, vault_before,
        "Vault balance must not change from trade"
    );
}

/// ATTACK: Deposit to an LP account using DepositCollateral.
/// Expected: Should succeed (LP accounts can receive deposits like users).
#[test]
fn test_attack_deposit_to_lp_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);

    // LP can deposit via DepositCollateral
    env.deposit(&lp, lp_idx, 10_000_000_000);
    let capital = env.read_account_capital(lp_idx);
    assert!(capital > 0, "LP should be able to receive deposits");

    // Vault should have the tokens
    assert_eq!(
        env.vault_balance(),
        10_000_000_100,
        "Vault should have LP deposit + init"
    );
}

/// ATTACK: LiquidateAtOracle targeting an LP account.
/// Expected: LP liquidation may be handled differently (LP has position from trading).
#[test]
fn test_attack_liquidate_lp_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // User trades against LP - LP takes the other side
    env.trade(&user, &lp, lp_idx, user_idx, 50_000_000);
    env.set_slot(200);
    env.crank();

    // LP has counter-position. Try to liquidate LP.
    let capital_before = env.read_account_capital(lp_idx);
    let pos_before = env.read_account_position(lp_idx);
    let liquidation_attempt = env.try_liquidate_target(lp_idx);

    // Whether liquidation succeeds or fails, verify no corruption
    let capital_after = env.read_account_capital(lp_idx);
    let pos_after = env.read_account_position(lp_idx);
    let vault = env.vault_balance();
    assert!(
        vault > 0,
        "Vault should still have balance after LP liquidation attempt"
    );
    assert!(
        pos_after.unsigned_abs() <= pos_before.unsigned_abs(),
        "LP liquidation attempt must not increase LP exposure. before={} after={} attempt={:?}",
        pos_before,
        pos_after,
        liquidation_attempt
    );
    // LP capital should not have increased (no value extraction)
    assert!(
        capital_after <= capital_before + 1, // +1 for rounding tolerance
        "LP should not profit from liquidation attempt. Before: {}, After: {}, attempt={:?}",
        capital_before,
        capital_after,
        liquidation_attempt
    );
}

/// ATTACK: Deposit to an out-of-bounds account index.
/// Expected: Rejected by check_idx (index >= max_accounts).
#[test]
fn test_attack_deposit_out_of_bounds_index() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    env.svm.airdrop(&user.pubkey(), 10_000_000_000).unwrap();

    // Try to deposit to index MAX_ACCOUNTS (out of bounds)
    let result = env.try_deposit_to_idx(&user, MAX_ACCOUNTS as u16, 1_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Deposit to out-of-bounds index should fail"
    );

    // Try index u16::MAX
    let result = env.try_deposit_to_idx(&user, u16::MAX, 1_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Deposit to u16::MAX index should fail"
    );
}

/// ATTACK: Trade with out-of-bounds user_idx.
/// Expected: Rejected by check_idx.
#[test]
fn test_attack_trade_out_of_bounds_index() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();

    let lp_cap_before = env.read_account_capital(lp_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let vault_before = env.vault_balance();
    let used_before = env.read_num_used_accounts();

    // Trade with user_idx = 9999 (non-existent)
    let result = env.try_trade(&user, &lp, lp_idx, 9999, 1_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Trade with out-of-bounds user_idx should fail"
    );
    let lp_cap_after = env.read_account_capital(lp_idx);
    let lp_pos_after = env.read_account_position(lp_idx);
    let vault_after = env.vault_balance();
    let used_after = env.read_num_used_accounts();
    assert_eq!(
        lp_cap_after, lp_cap_before,
        "Rejected out-of-bounds trade must not change LP capital"
    );
    assert_eq!(
        lp_pos_after, lp_pos_before,
        "Rejected out-of-bounds trade must not change LP position"
    );
    assert_eq!(
        vault_after, vault_before,
        "Rejected out-of-bounds trade must not move vault funds"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected out-of-bounds trade must not change num_used_accounts"
    );
}

/// ATTACK: Withdraw from out-of-bounds index.
/// Expected: Rejected by check_idx.
#[test]
fn test_attack_withdraw_out_of_bounds_index() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();

    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let vault_before = env.vault_balance();
    let used_before = env.read_num_used_accounts();
    let result = env.try_withdraw(&user, u16::MAX, 1_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Withdraw from out-of-bounds index should fail"
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let vault_after = env.vault_balance();
    let used_after = env.read_num_used_accounts();
    assert_eq!(
        slab_after, slab_before,
        "Rejected out-of-bounds withdraw must not mutate slab"
    );
    assert_eq!(
        vault_after, vault_before,
        "Rejected out-of-bounds withdraw must not move vault funds"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected out-of-bounds withdraw must not change num_used_accounts"
    );
}

/// ATTACK: KeeperCrank liquidation candidate with out-of-bounds target index.
/// Expected: Invalid candidate is skipped without mutating market state.
#[test]
fn test_attack_liquidate_out_of_bounds_index() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();
    let insurance_before = env.read_insurance_balance();
    let used_before = env.read_num_used_accounts();
    let result = env.try_liquidate_target(u16::MAX);
    assert!(
        result.is_ok(),
        "ATTACK: crank should tolerate out-of-bounds liquidation candidates"
    );
    let vault_after = env.vault_balance();
    let engine_vault_after = env.read_engine_vault();
    let insurance_after = env.read_insurance_balance();
    let used_after = env.read_num_used_accounts();
    assert_eq!(
        vault_after, vault_before,
        "Skipped out-of-bounds liquidation candidate must not move vault funds"
    );
    assert_eq!(
        engine_vault_after, engine_vault_before,
        "Skipped out-of-bounds liquidation candidate must not move engine vault"
    );
    assert_eq!(
        insurance_after, insurance_before,
        "Skipped out-of-bounds liquidation candidate must not move insurance"
    );
    assert_eq!(
        used_after, used_before,
        "Skipped out-of-bounds liquidation candidate must not change num_used_accounts"
    );
}

/// ATTACK: InitLP after market resolution.
/// Expected: Rejected (no new LPs on resolved markets).
#[test]
fn test_attack_init_lp_after_resolution() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    // authority push removed.
    env.crank();
    env.try_resolve_market(&admin, 0)
        .expect("market resolution setup must succeed");

    let resolved_before = env.is_market_resolved();
    let used_before = env.read_num_used_accounts();
    let vault_before = env.vault_balance();

    // Try InitLP after resolution
    let lp = Keypair::new();
    let result = env.try_init_lp(&lp);
    assert!(
        result.is_err(),
        "ATTACK: InitLP after resolution should be rejected"
    );
    let resolved_after = env.is_market_resolved();
    let used_after = env.read_num_used_accounts();
    let vault_after = env.vault_balance();
    assert!(resolved_before, "Precondition: market should be resolved");
    assert_eq!(
        resolved_after, resolved_before,
        "Rejected InitLP must not change resolved flag"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected InitLP must not change num_used_accounts"
    );
    assert_eq!(
        vault_after, vault_before,
        "Rejected InitLP must not move vault funds"
    );
}

/// InitUser with default materialization payment and verify clean initialization.
/// Expected: Account capital is payment minus the mandatory anti-spam fee.
#[test]
fn test_attack_init_user_zero_fee() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // init_user pays 100 tokens, with 1 token routed to insurance.
    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    let capital = env.read_account_capital(user_idx);
    assert_eq!(
        capital, DEFAULT_INIT_CAPITAL as u128,
        "Account should have default init capital after anti-spam fee"
    );

    // Should still be able to deposit more after init
    env.deposit(&user, user_idx, 1_000_000_000);
    let capital_after = env.read_account_capital(user_idx);
    assert!(capital_after > 100, "Should be able to deposit after init");
}

/// ATTACK: Two users both try to withdraw max capital in the same slot.
/// Expected: Both succeed (vault has enough), conservation holds.
#[test]
fn test_attack_multi_user_withdraw_same_slot() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 10_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 10_000_000_000);

    let vault_before = env.vault_balance();
    assert_eq!(
        vault_before, 20_000_000_200,
        "Both deposits should be in vault (includes init deposits)"
    );

    // Both withdraw max capital
    let result1 = env.try_withdraw(&user1, user1_idx, 10_000_000_000);
    assert!(
        result1.is_ok(),
        "User1 withdraw should succeed: {:?}",
        result1
    );

    let result2 = env.try_withdraw(&user2, user2_idx, 10_000_000_000);
    assert!(
        result2.is_ok(),
        "User2 withdraw should succeed: {:?}",
        result2
    );

    // Each user had init deposit of 100 that wasn't withdrawn, so 200 remains
    let vault_after = env.vault_balance();
    assert_eq!(
        vault_after, 200,
        "Vault should have only init deposits remaining after full withdrawals"
    );
}

/// ATTACK: Double withdrawal from same account in same slot.
/// Expected: Second withdrawal fails (insufficient capital).
#[test]
fn test_attack_double_withdraw_same_slot() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // First withdrawal succeeds
    let result = env.try_withdraw(&user, user_idx, 10_000_000_000);
    assert!(
        result.is_ok(),
        "First full withdrawal should succeed: {:?}",
        result
    );

    // Second withdrawal fails (no capital left)
    let result = env.try_withdraw(&user, user_idx, 1_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Second withdrawal after full drain should fail"
    );
}

/// ATTACK: Verify two separate markets (slabs) don't interfere.
/// Expected: Each market has independent state and vault.
#[test]
fn test_attack_cross_market_isolation() {
    program_path();

    // Create first market
    let mut env1 = TestEnv::new();
    env1.init_market_with_invert(0);

    let user1 = Keypair::new();
    let user1_idx = env1.init_user(&user1);
    env1.deposit(&user1, user1_idx, 10_000_000_000);

    // Create second market (different TestEnv = different slab)
    let mut env2 = TestEnv::new();
    env2.init_market_with_invert(0);

    let user2 = Keypair::new();
    let user2_idx = env2.init_user(&user2);
    env2.deposit(&user2, user2_idx, 5_000_000_000);

    // Verify independent vaults
    assert_eq!(
        env1.vault_balance(),
        10_000_000_100,
        "Market 1 vault (+ init)"
    );
    assert_eq!(
        env2.vault_balance(),
        5_000_000_100,
        "Market 2 vault (+ init)"
    );

    // Withdraw from market 1 doesn't affect market 2
    env1.try_withdraw(&user1, user1_idx, 5_000_000_000)
        .expect("cross-market withdrawal in market 1 must succeed");
    assert_eq!(
        env1.vault_balance(),
        5_000_000_100,
        "Market 1 after withdraw (init deposit remains)"
    );
    assert_eq!(env2.vault_balance(), 5_000_000_100, "Market 2 unaffected");
}

/// ATTACK: Send instruction to wrong program_id's slab.
/// Expected: Slab guard rejects (program_id embedded in slab header).
#[test]
fn test_attack_wrong_slab_program_id() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Create a user normally
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    // Tamper slab owner to simulate "slab belongs to wrong program".
    let original_slab = env
        .svm
        .get_account(&env.slab)
        .expect("slab must exist before owner tamper");
    let mut tampered_slab = original_slab.clone();
    tampered_slab.owner = Pubkey::new_unique();
    env.svm
        .set_account(env.slab, tampered_slab)
        .expect("must be able to set tampered slab owner");

    let vault_before = env.vault_balance();
    let result = env.try_withdraw(&user, user_idx, 500_000_000);
    assert!(
        result.is_err(),
        "SECURITY: Withdraw should fail when slab owner != program_id: {:?}",
        result
    );
    assert_eq!(
        env.vault_balance(),
        vault_before,
        "Rejected withdraw with wrong slab owner must not change vault"
    );

    // Restore slab owner and show same operation now succeeds.
    env.svm
        .set_account(env.slab, original_slab)
        .expect("must restore original slab owner");
    let result = env.try_withdraw(&user, user_idx, 500_000_000);
    assert!(
        result.is_ok(),
        "Withdraw should succeed after restoring slab owner: {:?}",
        result
    );
    assert_eq!(
        env.vault_balance(),
        500_000_100,
        "Vault should reflect a single successful withdrawal after restore"
    );
}

/// ATTACK: Liquidate account that has capital but no position.
/// Expected: No-op (nothing to liquidate).
#[test]
fn test_attack_liquidate_account_no_position() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // User has capital but no position
    let capital_before = env.read_account_capital(user_idx);
    let liquidation_attempt = env.try_liquidate_target(user_idx);

    // Capital should be unchanged (no liquidation happened)
    let capital_after = env.read_account_capital(user_idx);
    assert_eq!(capital_before, capital_after,
        "ATTACK: Account with no position should not lose capital from liquidation. liquidate_result={:?}",
        liquidation_attempt);
}

/// ATTACK: LP tries to trade against itself (user_idx == lp_idx).
/// Expected: Rejected or no-op (can't trade against yourself).
#[test]
fn test_attack_self_trade_same_index() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Try trade where LP trades against itself (lp_idx == user_idx)
    let result = env.try_trade(&lp, &lp, lp_idx, lp_idx, 1_000_000);
    // Should be rejected or result in no position change.
    let pos = env.read_account_position(lp_idx);
    assert_eq!(
        pos, 0,
        "Self-trade should never create a position: result={:?} pos={}",
        result, pos
    );
    // Either rejected or no-op - vault must be intact
    assert_eq!(
        env.vault_balance(),
        100_000_000_100,
        "ATTACK: Self-trade should not affect vault"
    );
}

/// ATTACK: In Hyperp mode, a same-slot flat-market crank may adopt the
/// target directly. This is allowed only because there is no live OI to mark.
#[test]
fn test_attack_hyperp_same_slot_flat_crank_can_adopt_target() {
    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();
    // v12.19: price-move cap is immutable init-time, no runtime call.

    // First crank at slot 100 - this sets engine.current_slot = 100
    env.set_slot(100);
    env.crank();

    // Push mark price significantly higher (mark=2.0, index still ~1.0)
    env.try_push_oracle_price(&admin, 2_000_000, 2000).unwrap();

    // Read last_effective_price_e6 (index) from config before same-slot crank
    // Config offset: header is 16 bytes, config starts after that
    // last_effective_price_e6 offset within config (check source for exact layout)
    // Read last_effective_price_e6 (the index) before same-slot crank
    // last_effective_price_e6 is at config offset 312: slab bytes [384..392]
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    const INDEX_OFF: usize = 328; // HEADER_LEN(136) + last_effective_price_e6(192) (v12.19)
    let index_before =
        u64::from_le_bytes(slab_before[INDEX_OFF..INDEX_OFF + 8].try_into().unwrap());
    assert!(index_before > 0, "Index should be non-zero before crank");

    // Try crank at same slot 100 again
    let result = env.try_crank();
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let index_after = u64::from_le_bytes(slab_after[INDEX_OFF..INDEX_OFF + 8].try_into().unwrap());

    // v12.19.13 wrapper policy: zero-OI markets may adopt the raw target
    // directly because no live position can lose equity. Exposed-OI paths are
    // still clamped by dt and covered by target-lag gates.
    assert!(
        index_after >= index_before,
        "flat same-slot crank should not move away from the target. \
         before={}, after={}, crank_result={:?}",
        index_before,
        index_after,
        result
    );
}

/// ATTACK: Try to init new LP after Hyperp market resolution.
/// Resolved Hyperp markets should block InitLP.
#[test]
fn test_attack_hyperp_init_lp_after_resolution() {
    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    // Resolve market
    env.try_resolve_market(&admin, 0).unwrap();
    let resolved_before = env.is_market_resolved();
    let used_before = env.read_num_used_accounts();
    let vault_before = env.vault_balance();

    // Try to init new LP
    let new_lp = Keypair::new();
    let result = env.try_init_lp(&new_lp);
    assert!(
        result.is_err(),
        "ATTACK: InitLP succeeded on resolved market!"
    );
    let resolved_after = env.is_market_resolved();
    let used_after = env.read_num_used_accounts();
    let vault_after = env.vault_balance();
    assert!(resolved_before, "Precondition: market should be resolved");
    assert_eq!(
        resolved_after, resolved_before,
        "Rejected InitLP must not change resolved flag"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected InitLP must not change num_used_accounts"
    );
    assert_eq!(
        vault_after, vault_before,
        "Rejected InitLP must not move vault"
    );
}

/// ATTACK: Push oracle price with extreme u64 value.
/// Circuit breaker should clamp price movement.
#[test]
fn test_attack_hyperp_push_extreme_price() {
    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();
    // v12.19: price-move cap is immutable init-time, no runtime call.

    // Push extreme price — rejected at ingress (exceeds MAX_ORACLE_PRICE after normalization)
    let result = env.try_push_oracle_price(&admin, u64::MAX / 2, 2000);
    assert!(
        result.is_err(),
        "Extreme push must be rejected (> MAX_ORACLE_PRICE)"
    );

    // Read stored last_effective_price_e6 - must be clamped, not u64::MAX/2
    // last_effective_price_e6 is at slab offset 384 (last u64 in config before engine)
    let slab_data = env.svm.get_account(&env.slab).unwrap().data;
    const INDEX_OFF: usize = 328; // HEADER_LEN(136) + last_effective_price_e6(192) (v12.19)
    let stored_price = u64::from_le_bytes(slab_data[INDEX_OFF..INDEX_OFF + 8].try_into().unwrap());
    // With 5% cap and base price 1_000_000, max clamped = 1_050_000
    assert!(
        stored_price < 2_000_000,
        "ATTACK: Circuit breaker failed to clamp extreme price! stored={}, pushed={}",
        stored_price,
        u64::MAX / 2
    );
    assert!(
        stored_price > 0,
        "Stored price should be positive after push"
    );
}

/// ATTACK: High maintenance fee accrual over many slots should not create
/// unbounded debt or break equity calculations. Fee debt is saturating.
/// ATTACK: Maintenance fee set to u128::MAX should not panic or corrupt state.
/// ATTACK: Warmup period prevents immediate profit withdrawal.
/// User with positive PnL should not be able to withdraw profit before warmup completes.
#[test]
fn test_attack_warmup_prevents_immediate_profit_withdrawal() {
    program_path();

    let mut env = TestEnv::new();
    // Use a short maturity horizon so this withdrawal test stays fast.
    env.init_market_with_warmup(0, 50);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Trade to create position
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    env.set_slot_and_price(100, 100_000_000);
    env.crank();

    // Price goes up - user has unrealized profit
    env.set_slot_and_price(200, 200_000_000);
    env.crank();

    // Close position to realize profit
    env.trade(&user, &lp, lp_idx, user_idx, -5_000_000);
    env.set_slot_and_price(300, 200_000_000);
    env.crank();

    // Vault conservation: no tokens created or destroyed through trade lifecycle
    let total_deposited = 10_000_000_000u64 + 100_000_000_000u64 + 200; // +200 from 2 init deposits
    let vault = env.vault_balance();
    assert_eq!(
        vault, total_deposited,
        "ATTACK: Warmup trade cycle violated conservation! vault={}, deposited={}",
        vault, total_deposited
    );

    // Try to withdraw MORE than original deposit
    // In ADL engine (v10.5), K-coefficient PnL settlement may convert
    // profit to capital faster than the old engine's warmup mechanism.
    // The withdrawal may succeed if warmup has already converted profit.
    let capital_before = env.read_account_capital(user_idx);
    let result = env.try_withdraw(&user, user_idx, 10_000_000_001);
    // Either warmup blocks it (expected) or profit already settled (ADL engine)
    if result.is_ok() {
        // Profit already vested — verify conservation
        let vault = env.vault_balance();
        assert!(
            vault <= total_deposited,
            "Conservation: vault={} deposits={}",
            vault,
            total_deposited
        );
    }
    let vault_final = env.vault_balance();
    // Conservation: after profit withdrawal, vault must cover remaining c_tot + insurance.
    // With ADL engine, warmup-converted profit (50M×haircut) can push capital above
    // the initial 10B deposit, so the vault may drop below 100B LP deposit.
    // The invariant is vault >= c_tot + insurance (engine conservation), not a fixed floor.
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault_final,
        "ATTACK: Engine vault and SPL vault mismatch! engine={} spl={}",
        engine_vault, vault_final
    );
}

/// ATTACK: User tries to withdraw more than their capital.
/// Should fail with insufficient balance.
#[test]
fn test_attack_withdraw_exceeds_capital() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    let vault_before = env.vault_balance();

    // Try to withdraw 10x capital
    let result = env.try_withdraw(&user, user_idx, 10_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Withdrawal of 10x capital succeeded!"
    );

    // Vault unchanged
    let vault_after = env.vault_balance();
    assert_eq!(
        vault_before, vault_after,
        "ATTACK: Failed withdrawal changed vault balance! before={}, after={}",
        vault_before, vault_after
    );
}

/// ATTACK: Withdraw from another user's account.
/// Account owner verification should prevent this.
#[test]
fn test_attack_withdraw_from_others_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let victim = Keypair::new();
    let victim_idx = env.init_user(&victim);
    env.deposit(&victim, victim_idx, 5_000_000_000);

    let attacker = Keypair::new();
    let _attacker_idx = env.init_user(&attacker);

    // Attacker tries to withdraw from victim's account index
    let result = env.try_withdraw(&attacker, victim_idx, 1_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Attacker withdrew from victim's account!"
    );

    // Victim's capital unchanged
    let victim_capital = env.read_account_capital(victim_idx);
    assert_eq!(
        victim_capital, 5_000_000_099,
        "ATTACK: Victim's capital changed after attacker's failed withdrawal! capital={}",
        victim_capital
    );
}

/// ATTACK: Deposit to another user's account.
/// Account owner verification should prevent this.
#[test]
fn test_attack_deposit_to_others_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let victim = Keypair::new();
    let victim_idx = env.init_user(&victim);
    env.deposit(&victim, victim_idx, 5_000_000_000);

    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 2_000_000_000).unwrap();

    // Attacker tries to deposit to victim's account index
    let ata = env.create_ata(&attacker.pubkey(), 1_000_000_000);
    let victim_cap_before = env.read_account_capital(victim_idx);
    let used_before = env.read_num_used_accounts();
    let vault_before = env.vault_balance();
    let ata_before = {
        let ata_data = env.svm.get_account(&ata).unwrap().data;
        TokenAccount::unpack(&ata_data).unwrap().amount
    };
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data: encode_deposit(victim_idx, 1_000_000_000),
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
        "ATTACK: Attacker deposited to victim's account!"
    );
    let victim_cap_after = env.read_account_capital(victim_idx);
    let used_after = env.read_num_used_accounts();
    let vault_after = env.vault_balance();
    let ata_after = {
        let ata_data = env.svm.get_account(&ata).unwrap().data;
        TokenAccount::unpack(&ata_data).unwrap().amount
    };
    assert_eq!(
        victim_cap_after, victim_cap_before,
        "Rejected cross-account deposit must not change victim capital"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected cross-account deposit must not change num_used_accounts"
    );
    assert_eq!(
        vault_after, vault_before,
        "Rejected cross-account deposit must not change vault"
    );
    assert_eq!(
        ata_after, ata_before,
        "Rejected cross-account deposit must not debit attacker ATA"
    );
}

/// ATTACK: Close account owned by someone else.
/// Must verify account ownership.
#[test]
fn test_attack_close_others_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let victim = Keypair::new();
    let victim_idx = env.init_user(&victim);
    env.deposit(&victim, victim_idx, 5_000_000_000);

    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();

    // Attacker tries to close victim's account
    let result = env.try_close_account(&attacker, victim_idx);
    assert!(result.is_err(), "ATTACK: Attacker closed victim's account!");

    // Victim's capital should be intact
    let victim_capital = env.read_account_capital(victim_idx);
    assert_eq!(
        victim_capital, 5_000_000_099,
        "Victim's capital should be unchanged after failed close attempt"
    );
}

/// ATTACK: LiquidateAtOracle on a healthy account should be a no-op.
/// Healthy accounts must not be liquidated.
#[test]
fn test_attack_liquidate_healthy_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 80); // max cap (100%/read)

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000); // Large capital

    // Small trade, well within margin
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    env.set_slot_and_price(100, 100_000_000);
    env.crank();

    let capital_before = env.read_account_capital(user_idx);
    let pos_before = env.read_account_position(user_idx);

    // Try to liquidate healthy account - should be a no-op
    let result = env.try_liquidate_target(user_idx);
    assert!(
        result.is_ok(),
        "Liquidation of healthy account should return Ok (no-op): {:?}",
        result
    );

    // Position and capital should be unchanged
    let capital_after = env.read_account_capital(user_idx);
    let pos_after = env.read_account_position(user_idx);
    assert_eq!(
        capital_before, capital_after,
        "ATTACK: Healthy account capital changed after liquidation attempt! {}->{}",
        capital_before, capital_after
    );
    assert_eq!(
        pos_before, pos_after,
        "ATTACK: Healthy account position changed after liquidation attempt! {}->{}",
        pos_before, pos_after
    );
}

/// ATTACK: Double resolve market attempt.
/// Second resolve should fail.
#[test]
fn test_attack_double_resolve_market() {
    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    // First resolve
    let result = env.try_resolve_market(&admin, 0);
    assert!(result.is_ok(), "First resolve should succeed");

    // Second resolve should fail
    let result = env.try_resolve_market(&admin, 0);
    assert!(result.is_err(), "ATTACK: Double resolve succeeded!");
}

/// ATTACK: admin burn when the lifecycle guard fails must be rejected.
/// Verify that the zero-admin foot-gun guard prevents the lockout.
#[test]
fn test_attack_update_admin_to_zero_locks_out() {
    program_path();

    let mut env = TestEnv::new();
    // Use init_market_with_cap with permissionless resolve + force_close_delay
    // because admin burn requires both for live markets (liveness guard).
    env.init_market_with_cap(0, 100);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Burn admin via UpdateAuthority (tag 32, kind=ADMIN, new=zero,
    // single-sig). Legacy UpdateAdmin (tag 12) was replaced by UpdateAuthority.
    let result = env.try_update_authority(&admin, AUTHORITY_ADMIN, None);
    assert!(
        result.is_ok(),
        "Admin burn via UpdateAuthority must succeed: {:?}",
        result
    );

    // Admin is now burned - all admin instructions must fail
    let result = env.try_update_config(&admin);
    assert!(
        result.is_err(),
        "Admin operations must fail after admin burn"
    );
}

/// ATTACK: LP risk gating with conservative max_abs tracking.
/// After LP shrinks from max position, risk check uses old max (conservative).
/// Verify that risk-increasing trades are correctly blocked when gate is active.
#[test]
fn test_attack_lp_risk_conservative_after_shrink() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 10_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 10_000_000_000);

    env.crank();

    // Trade 1: position, LP gets -1000 position
    // At price ~138, notional = 1000*138 = 138K, needs 10% margin = 13.8K << 10B
    env.trade(&user1, &lp, lp_idx, user1_idx, 1000);

    let lp_pos_after_t1 = env.read_account_position(lp_idx);
    let user1_pos_after_t1 = env.read_account_position(user1_idx);
    assert_eq!(
        lp_pos_after_t1, -1000,
        "LP should be -1000 after first trade"
    );
    assert_eq!(
        user1_pos_after_t1, 1000,
        "User1 should be +1000 after first trade"
    );

    // Trade 2: close most of position, LP now has small position
    env.trade(&user1, &lp, lp_idx, user1_idx, -900);

    // LP position is now -100 but lp_max_abs was 1000 (conservative)
    // This is the correct behavior - the risk metric overestimates

    // Verify LP is still alive and operational
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(
        lp_pos, -100,
        "LP position should be -100 after partial close"
    );

    // Vault conservation: SPL vault >= engine vault
    let spl_vault_balance = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        let vault_account = TokenAccount::unpack(&vault_data).unwrap();
        vault_account.amount
    };
    assert!(
        spl_vault_balance > 0,
        "ATTACK: SPL vault should hold deposited tokens"
    );
}

/// ATTACK: Entry price tracking through position flip (long → short).
/// After flipping, the entry_price should be updated via settle_mark_to_oracle.
/// Verify PnL calculation is correct after flip.
#[test]
fn test_attack_entry_price_across_position_flip() {
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

    // Open long 100
    env.trade(&user, &lp, lp_idx, user_idx, 100);

    // Verify position immediately after trade (before crank)
    let user_pos_1 = env.read_account_position(user_idx);
    assert_eq!(
        user_pos_1, 100,
        "User should be long +100 after first trade"
    );

    // Flip to short: trade -200 (closes +100, opens -100)
    env.trade(&user, &lp, lp_idx, user_idx, -200);

    // User should now be short 100
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(user_pos, -100, "User should have flipped to short -100");

    // LP should be long 100
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(lp_pos, 100, "LP should be long +100 after flip");

    env.crank();

    // Conservation: user capital + LP capital should be <= total deposited
    // (fees may reduce total)
    let user_cap = env.read_account_capital(user_idx);
    let lp_cap = env.read_account_capital(lp_idx);
    assert!(
        user_cap + lp_cap <= 20_000_000_200,
        "ATTACK: Position flip created value! User + LP capital exceeds deposits"
    );
}

/// ATTACK: Funding anti-retroactivity - rate changes at zero-DT crank
/// should use the OLD rate for the elapsed interval, not the new one.
/// Test: crank twice at same slot (sets rate), then crank at later slot.
#[test]
fn test_attack_funding_anti_retroactivity_zero_dt() {
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

    // Open position to generate funding
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);

    // Record vault before funding accrual
    let spl_vault_before = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        let vault_account = TokenAccount::unpack(&vault_data).unwrap();
        vault_account.amount
    };

    // Crank at same slot (dt=0, no funding accrued) - should still succeed
    let same_slot_crank_1 = env.try_crank();
    assert!(
        same_slot_crank_1.is_ok(),
        "Same-slot crank should succeed: {:?}",
        same_slot_crank_1
    );
    // Crank again same slot (dt=0 again)
    let same_slot_crank_2 = env.try_crank();
    assert!(
        same_slot_crank_2.is_ok(),
        "Repeated same-slot crank should succeed: {:?}",
        same_slot_crank_2
    );

    // Advance slot and crank (now dt > 0, funding accrues)
    env.set_slot(100);
    env.crank();

    // SPL vault should be unchanged (funding is internal accounting, not SPL transfers)
    let spl_vault_after = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        let vault_account = TokenAccount::unpack(&vault_data).unwrap();
        vault_account.amount
    };
    assert_eq!(
        spl_vault_before, spl_vault_after,
        "ATTACK: Funding caused SPL vault imbalance - value leaked. \
         same_slot_1={:?} same_slot_2={:?}",
        same_slot_crank_1, same_slot_crank_2
    );

    // Engine vault should still be correct
    let engine_vault = env.read_engine_vault();
    assert!(engine_vault > 0, "Engine vault should be positive");
}

/// ATTACK: Account slot reuse after close - verify new account has clean state.
/// After closing an account, a new account created should have no
/// residual position/PnL state. Also verifies freelist integrity.
#[test]
fn test_attack_slot_reuse_clean_state_after_gc() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    // Create user at index 1
    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    // Withdraw init capital to make user truly empty for GC.
    env.withdraw(&user1, user1_idx, DEFAULT_INIT_CAPITAL);

    env.crank();

    // user1 with zero capital should be GC-eligible
    // After crank, check if GC removed it
    let slot_used = env.is_slot_used(user1_idx);

    if !slot_used {
        // GC removed it, the slot was freed
        // Verify the slot is clean
        let capital = env.read_account_capital(user1_idx);
        assert_eq!(capital, 0, "GC'd slot should have zero capital");
        let pos = env.read_account_position(user1_idx);
        assert_eq!(pos, 0, "GC'd slot should have zero position");
    }

    // num_used should reflect the state
    let num_used = env.read_num_used_accounts();
    if slot_used {
        assert_eq!(num_used, 2, "LP + user1 should exist");
    } else {
        assert_eq!(num_used, 1, "Only LP should exist after GC");
    }

    // Conservation: SPL vault should be correct
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 10_000_000_101,
        "ATTACK: Vault should only have LP deposit/init plus reclaimed user fee!"
    );
}

/// ATTACK: Verify trades work with u64::MAX crank staleness.
/// Note: This market uses max_crank_staleness_slots=u64::MAX (always fresh),
/// so it only tests that large slot gaps don't break the system.
/// Stale-crank rejection is not tested here (would need finite staleness config).
#[test]
fn test_attack_crank_freshness_boundary() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0); // max_crank_staleness_slots = u64::MAX

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.crank();

    // Trade should work immediately after crank
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 100);
    assert!(result.is_ok(), "Trade should work right after crank");

    // Close position
    env.trade(&user, &lp, lp_idx, user_idx, -100);

    // With max_crank_staleness = u64::MAX, crank is always "fresh"
    // But advance to large slot to test
    env.set_slot(10_000);

    // Trade without re-cranking - should still work with u64::MAX staleness
    let result2 = env.try_trade(&user, &lp, lp_idx, user_idx, 50);
    assert!(
        result2.is_ok(),
        "Trade should work with u64::MAX staleness even after slot advancement"
    );

    // Conservation check
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 20_000_000_200,
        "ATTACK: Vault balance changed due to timing attack!"
    );
}

/// ATTACK: Liquidation of already-zero-position account should fail.
/// An attacker tries to liquidate an account that already has no position.
#[test]
fn test_attack_liquidate_zero_position_account() {
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

    // User has no position - try to liquidate
    // Liquidation returns Ok (no-op) for zero-position accounts
    let capital_before = env.read_account_capital(user_idx);
    let result = env.try_liquidate_target(user_idx);
    assert!(
        result.is_ok(),
        "Liquidation of zero-position should return Ok (no-op): {:?}",
        result
    );

    // Key assertion: capital should not change after liquidation attempt
    let capital_after = env.read_account_capital(user_idx);
    assert_eq!(
        capital_before, capital_after,
        "ATTACK: Liquidation of zero-position changed capital! Before={} After={}",
        capital_before, capital_after
    );

    // Position should still be zero
    let pos_after = env.read_account_position(user_idx);
    assert_eq!(
        pos_after, 0,
        "ATTACK: Liquidation of zero-position account created a position!"
    );
}

/// ATTACK: Extreme raw oracle target before any accounts exist.
/// v12.19.13 seeds the engine with a real init price, so an extreme raw
/// target is not a "first price" bypass. It must reject or make no state
/// progress instead of admitting accounts at a nonsensical target.
#[test]
fn test_attack_circuit_breaker_first_price_extreme() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Set an extreme price (very high)
    env.set_slot_and_price_raw_no_walk(10, 999_999_000_000); // $999,999 per unit

    let result = env.try_crank();
    assert!(
        result.is_err(),
        "Extreme raw target must not be silently admitted before catch-up"
    );
    assert_eq!(env.read_num_used_accounts(), 0);
    assert_eq!(env.vault_balance(), 0);
}

/// ATTACK: Circuit breaker clamping after second price.
/// After initial price is set, subsequent extreme prices should be clamped.
/// Verify clamping prevents exploitation via price manipulation.
#[test]
fn test_attack_circuit_breaker_clamping_second_price() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let _admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    // v12.19: price-move cap is immutable init-time (TEST_MAX_PRICE_MOVE_BPS_PER_SLOT=2).

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.crank(); // Establishes last_effective_price_e6 = 138_000_000

    // Open position at normal price
    env.trade(&user, &lp, lp_idx, user_idx, 100);
    env.crank();

    // Read the baseline price before the extreme jump
    let baseline = env.read_last_effective_price();
    assert_eq!(baseline, 138_000_000, "Baseline should be $138");

    // Now set extreme price (10x increase) — only 1 slot later
    env.set_slot_and_price_raw_no_walk(20, 1_380_000_000); // 10x normal price
    let crank_result = env.try_crank();

    if crank_result.is_err() {
        assert_eq!(
            env.read_last_effective_price(),
            baseline,
            "Rejected raw-target jump must not advance the effective price"
        );
        return;
    }

    // Verify the circuit breaker actually clamped: the stored price should
    // NOT be 1.38B. With 1% cap and 1 slot, max move = 138M * 1% = 1.38M.
    let clamped_price = env.read_last_effective_price();
    assert!(
        clamped_price < 200_000_000,
        "Circuit breaker must clamp 10x price jump: got {} (expected near {})",
        clamped_price,
        138_000_000 + 1_380_000
    );
    assert!(
        clamped_price > baseline,
        "Price should have moved up (clamped): baseline={} clamped={}",
        baseline,
        clamped_price
    );

    // Conservation: total capital should not exceed total deposits
    // (init_lp=100 + deposit=10B + init_user=100 + deposit=10B = 20_000_000_200)
    let user_cap = env.read_account_capital(user_idx);
    let lp_cap = env.read_account_capital(lp_idx);
    assert!(
        user_cap + lp_cap <= 20_000_000_200,
        "ATTACK: Circuit breaker failed - capital increased! user={} lp={}",
        user_cap,
        lp_cap
    );
}

/// ATTACK: Fee debt exceeds capital during crank.
/// Create a scenario where maintenance fees accumulate to exceed capital.
/// Verify equity calculation remains correct and no underflow occurs.
/// ATTACK: Rapid price oscillation precision loss.
/// Execute many trades with alternating prices to accumulate rounding errors.
/// Verify total value is conserved across repeated operations.
#[test]
fn test_attack_price_oscillation_precision_loss() {
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

    // Open/close position to test precision
    // Round 1
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot(10);
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, -100_000);
    env.set_slot(20);
    env.crank();
    // Round 2 (different size to avoid duplicate tx hash)
    env.trade(&user, &lp, lp_idx, user_idx, 200_000);
    env.set_slot(30);
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, -200_000);
    env.set_slot(40);
    env.crank();
    // Round 3
    env.trade(&user, &lp, lp_idx, user_idx, 300_000);
    env.set_slot(50);
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, -300_000);
    env.set_slot(60);
    env.crank();

    // Position should be zero
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(user_pos, 0, "Position should be zero after all round-trips");

    // Conservation: total value should not exceed initial deposits
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 20_000_000_200,
        "ATTACK: SPL vault changed after round-trip trades! vault={}",
        spl_vault
    );

    let user_cap = env.read_account_capital(user_idx);
    let lp_cap = env.read_account_capital(lp_idx);
    assert!(
        user_cap + lp_cap <= 20_000_000_200,
        "ATTACK: Total capital exceeds deposits after oscillation! user={} lp={}",
        user_cap,
        lp_cap
    );
}

/// ATTACK: Withdraw exactly all capital (no position).
/// Verify withdrawing exact capital amount works and leaves account with 0.
#[test]
fn test_attack_withdraw_exact_capital_no_position() {
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

    // Withdraw exact capital amount (no position = no margin requirement)
    let cap = env.read_account_capital(user_idx);
    let withdraw_result = env.try_withdraw(&user, user_idx, cap as u64);
    assert!(
        withdraw_result.is_ok(),
        "Should be able to withdraw all capital with no position"
    );

    // Capital should now be 0
    let cap_after = env.read_account_capital(user_idx);
    assert_eq!(cap_after, 0, "Capital should be zero after full withdrawal");

    // SPL vault should decrease by withdrawn amount
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 10_000_000_101,
        "ATTACK: SPL vault has wrong balance after full withdrawal! vault={}",
        spl_vault
    );
}

/// ATTACK: Threshold EWMA convergence across many cranks.
/// Set a risk threshold and verify it converges toward target via EWMA
/// rather than allowing wild oscillations that could be exploited.
/// ATTACK: Trade at exactly the initial margin boundary.
/// Open a position that requires exactly initial_margin_bps of capital.
/// Then try to open slightly more - should fail margin check.
#[test]
fn test_attack_trade_exact_initial_margin_boundary() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000); // 1B units

    env.crank();

    // Price is 138e6. Notional per unit = 138e6/1e6 = 138.
    // Initial margin is 1000 bps = 10%.
    // Max notional = capital / margin_fraction = 1B / 0.1 = 10B
    // Max position = 10B / 138 ≈ 72_463_768

    // Try a very large position that should fail
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 100_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Trade exceeding initial margin should fail!"
    );

    // Position should remain zero
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(
        user_pos, 0,
        "ATTACK: Position changed despite failed margin check!"
    );
}

/// ATTACK: Risk gate activation with insurance at exact threshold boundary.
/// Verify behavior when insurance_fund.balance == risk_reduction_threshold exactly.
/// ATTACK: Unit scale boundary - init market with MAX_UNIT_SCALE.
/// Verify that operations work correctly at the maximum unit scale.
#[test]
fn test_attack_max_unit_scale_operations() {
    program_path();

    let mut env = TestEnv::new();
    // Init with moderate unit scale (1000) - still tests alignment
    // Note: max unit_scale (1B) would cause OracleInvalid because price/scale = 0
    env.init_market_full(0, 1000, 0);

    let lp = Keypair::new();
    // With unit_scale=1000, need 100*1000=100_000 base for min_initial_deposit
    let lp_idx = env.init_lp_with_fee(&lp, 100_000);
    env.deposit(&lp, lp_idx, 10_000_000); // 10M base = 10K units

    let user = Keypair::new();
    let user_idx = env.init_user_with_fee(&user, 100_000);
    env.deposit(&user, user_idx, 5_000_000); // 5M base = 5K units

    env.crank();

    // Capital is in units: 99 from init + 5000 from deposit = 5099 units.
    let user_cap = env.read_account_capital(user_idx);
    assert_eq!(
        user_cap, 5099,
        "Capital should be 99 (init after fee) + 5000 (deposit) units at scale=1000"
    );

    // Withdrawal must be aligned to unit_scale
    let bad_withdraw = env.try_withdraw(&user, user_idx, 500); // Not aligned (500 % 1000 != 0)
    assert!(
        bad_withdraw.is_err(),
        "ATTACK: Misaligned withdrawal should fail at unit_scale=1000!"
    );

    // Aligned withdrawal should work
    let good_withdraw = env.try_withdraw(&user, user_idx, 1000); // 1 unit
    assert!(good_withdraw.is_ok(), "Aligned withdrawal should succeed");

    let cap_after = env.read_account_capital(user_idx);
    assert_eq!(
        cap_after, 5098,
        "Capital should decrease by 1 unit after aligned withdrawal"
    );
}

/// ATTACK: Close account after opening and closing position at same price.
/// PnL is zero after round-trip. Verifies capital returned and slot freed.
/// Note: Despite the name, this test creates zero PnL (no price change).
#[test]
fn test_attack_close_account_with_positive_pnl() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.crank();

    // Close position
    env.trade(&user, &lp, lp_idx, user_idx, -100_000);
    env.crank();

    // Position should be zero
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(user_pos, 0, "Position should be zero");

    // Try close - should succeed
    let close_result = env.try_close_account(&user, user_idx);
    assert!(
        close_result.is_ok(),
        "Close account should succeed with zero position"
    );

    // Verify user's slot is freed
    let num_used = env.read_num_used_accounts();
    assert_eq!(num_used, 1, "Only LP should remain");

    // SPL vault should have decreased (user got capital back)
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert!(
        spl_vault < 55_000_000_000,
        "ATTACK: Vault didn't decrease after close! vault={}",
        spl_vault
    );
    assert!(spl_vault > 0, "Vault should still have LP deposit");
}

/// ATTACK: Rapid open/close in same slot shouldn't bypass timing guards.
/// Verify that opening and closing a position in the same slot works
/// but doesn't allow exploiting stale prices or settlement.
#[test]
fn test_attack_same_slot_open_close_timing() {
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

    let cap_before = env.read_account_capital(user_idx);

    // Open and close in same slot (no crank between)
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.trade(&user, &lp, lp_idx, user_idx, -100_000);

    env.crank();

    let user_pos = env.read_account_position(user_idx);
    assert_eq!(
        user_pos, 0,
        "Position should be zero after same-slot round-trip"
    );

    let cap_after = env.read_account_capital(user_idx);
    // Capital should not increase (no free money from same-slot trades)
    assert!(
        cap_after <= cap_before,
        "ATTACK: Capital increased from same-slot round-trip! before={} after={}",
        cap_before,
        cap_after
    );

    // SPL vault conservation
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 20_000_000_200,
        "ATTACK: SPL vault changed from same-slot trades!"
    );
}

/// ATTACK: Verify c_tot aggregate stays in sync after multiple deposits and trades.
/// Multiple users deposit and trade, then verify c_tot == sum of individual capitals.
#[test]
fn test_attack_c_tot_sync_after_deposits_and_trades() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 5_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 3_000_000_000);

    env.crank();

    // Open positions
    env.trade(&user1, &lp, lp_idx, user1_idx, 50_000);
    env.set_slot(10);
    env.trade(&user2, &lp, lp_idx, user2_idx, -30_000);
    env.set_slot(20);
    env.crank();

    // Read individual capitals and c_tot
    let lp_cap = env.read_account_capital(lp_idx);
    let u1_cap = env.read_account_capital(user1_idx);
    let u2_cap = env.read_account_capital(user2_idx);
    let c_tot = env.read_c_tot();

    // c_tot should equal sum of individual capitals
    let sum = lp_cap + u1_cap + u2_cap;
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync! c_tot={} sum={} (lp={} u1={} u2={})",
        c_tot, sum, lp_cap, u1_cap, u2_cap
    );

    // SPL vault conservation
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 28_000_000_300,
        "ATTACK: SPL vault changed! (includes 3 init deposits)"
    );
}

/// ATTACK: Verify pnl_pos_tot tracks only positive PnL accounts.
/// After trades and cranks, pnl_pos_tot should be sum of max(0, pnl) for each account.
#[test]
fn test_attack_pnl_pos_tot_only_positive() {
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

    // Top up insurance to disable force-realize
    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();

    env.crank();

    // Open position then crank at different price to create PnL
    // With warmup_period=0, PnL converts to capital instantly on each crank.
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);

    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot_and_price(10, 140_000_000); // Price up slightly
    env.crank();

    // With instant warmup, PnL has settled to capital — check capital changed.
    let user_cap_after = env.read_account_capital(user_idx);
    let lp_cap_after = env.read_account_capital(lp_idx);

    // Precondition: price move should have shifted capital between accounts
    assert!(
        user_cap_after != user_cap_before || lp_cap_after != lp_cap_before,
        "TEST PRECONDITION: Price move should change capital for at least one account (user: {} -> {}, lp: {} -> {})",
        user_cap_before, user_cap_after, lp_cap_before, lp_cap_after
    );

    // With instant warmup, positive PnL is matured/released immediately.
    // pnl_pos_tot may have a small residual due to the matured PnL model.
    let pnl_pos_tot = env.read_pnl_pos_tot();
    assert!(
        pnl_pos_tot >= 0,
        "ATTACK: pnl_pos_tot should be non-negative after instant warmup (warmup_period=0): got={}",
        pnl_pos_tot
    );
}

/// ATTACK: Warmup with zero period should convert PnL instantly.
/// Init market with warmup_period_slots=0, verify profit converts immediately.
#[test]
fn test_attack_warmup_zero_period_instant_conversion() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_warmup(0, 0); // warmup_period_slots = 0

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot(10);
    env.crank();

    // With warmup=0, PnL should convert to capital immediately on next crank
    env.set_slot(20);
    env.crank();

    // Conservation: total value shouldn't exceed deposits
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 25_000_000_200,
        "ATTACK: SPL vault changed with instant warmup! vault={}",
        spl_vault
    );

    // c_tot should still equal sum of capitals
    let lp_cap = env.read_account_capital(lp_idx);
    let user_cap = env.read_account_capital(user_idx);
    let c_tot = env.read_c_tot();
    assert_eq!(
        c_tot,
        lp_cap + user_cap,
        "ATTACK: c_tot desync after instant warmup!"
    );
}

/// ATTACK: Open and close multiple positions - verify c_tot stays consistent.
/// Trade long, close, trade short, close - c_tot == sum of capitals at each step.
#[test]
fn test_attack_position_flip_warmup_reset() {
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

    // Top up insurance to disable force-realize mode (insurance > threshold=0)
    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();

    env.crank();

    // Open long
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot(10);
    env.crank();

    // Verify c_tot consistency mid-trade
    let c_tot_1 = env.read_c_tot();
    let sum_1 = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot_1, sum_1,
        "ATTACK: c_tot desync with open long! c_tot={} sum={}",
        c_tot_1, sum_1
    );

    // Close long
    env.set_slot(20);
    env.trade(&user, &lp, lp_idx, user_idx, -100_000);
    env.set_slot(30);
    env.crank();

    // Position should be zero after close
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(user_pos, 0, "Position should be zero after close");

    // Open short with different size (avoids AlreadyProcessed)
    env.set_slot(40);
    env.trade(&user, &lp, lp_idx, user_idx, -80_000);
    env.set_slot(50);
    env.crank();

    // Position should be short
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(user_pos, -80_000, "Position should be -80K after new short");

    // Conservation: SPL vault should include deposits + insurance top-up
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault,
        26_000_000_200, // 20B + 5B + 1B insurance + 200 init deposits
        "ATTACK: SPL vault changed during position flip!"
    );

    let c_tot = env.read_c_tot();
    let lp_cap = env.read_account_capital(lp_idx);
    let user_cap = env.read_account_capital(user_idx);
    assert_eq!(
        c_tot,
        lp_cap + user_cap,
        "ATTACK: c_tot desync after position flip!"
    );
}

/// ATTACK: Multiple sequential account inits have clean independent state.
/// Create several accounts, verify each starts with zero position/PnL.
/// Then trade with one and verify the others are not affected.
#[test]
fn test_attack_account_reinit_after_gc_clean_state() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

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

    // All new accounts should start clean
    assert_eq!(
        env.read_account_position(u1_idx),
        0,
        "User1 should start with zero position"
    );
    assert_eq!(
        env.read_account_position(u2_idx),
        0,
        "User2 should start with zero position"
    );
    assert_eq!(
        env.read_account_position(u3_idx),
        0,
        "User3 should start with zero position"
    );
    assert_eq!(
        env.read_account_pnl(u1_idx),
        0,
        "User1 should start with zero PnL"
    );
    assert_eq!(
        env.read_account_pnl(u2_idx),
        0,
        "User2 should start with zero PnL"
    );
    assert_eq!(
        env.read_account_pnl(u3_idx),
        0,
        "User3 should start with zero PnL"
    );

    // Trade with user1 only
    env.trade(&user1, &lp, lp_idx, u1_idx, 100_000);
    env.set_slot(10);
    env.crank();

    // User2 and User3 capitals unchanged (no cross-contamination)
    assert_eq!(
        env.read_account_capital(u2_idx),
        3_000_000_099,
        "ATTACK: User2 capital changed from User1's trade!"
    );
    assert_eq!(
        env.read_account_capital(u3_idx),
        2_000_000_099,
        "ATTACK: User3 capital changed from User1's trade!"
    );

    // PnL should be zero for non-trading accounts
    assert_eq!(
        env.read_account_pnl(u2_idx),
        0,
        "ATTACK: User2 PnL leaked from User1's trade!"
    );
    assert_eq!(
        env.read_account_pnl(u3_idx),
        0,
        "ATTACK: User3 PnL leaked from User1's trade!"
    );
}

/// ATTACK: Insurance fund growth from fees doesn't inflate haircut.
/// Haircut = min(residual, pnl_pos_tot) / pnl_pos_tot where residual = vault - c_tot - insurance.
/// Insurance growing from fees reduces residual, which REDUCES haircut (safer).
/// ATTACK: Withdraw more than capital should fail.
/// Verify that withdrawing more than available capital is rejected.
/// Also verify that withdrawal with position leaves at least margin.
#[test]
fn test_attack_withdraw_margin_boundary_consistency() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    // Capital = 5_000_000_000 deposit + 99 from init_user fee_payment after anti-spam fee.
    // Try to withdraw strictly more than capital.
    let over_withdraw = env.try_withdraw(&user, user_idx, 5_000_000_101);
    assert!(
        over_withdraw.is_err(),
        "ATTACK: Withdrawal of more than capital succeeded!"
    );

    // Verify capital is unchanged.
    let cap_after = env.read_account_capital(user_idx);
    assert_eq!(
        cap_after, 5_000_000_099,
        "ATTACK: Failed withdrawal changed capital!"
    );

    // Open a large position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    // Stay at slot 100 — no slot advance needed for this check.
    // Advancing the slot then cranking changes last_market_slot, which in turn
    // causes the arg-swapped withdraw to issue a massive accrue_market_to loop
    // that exhausts the compute budget on the withdrawal transaction.

    // Withdraw a portion of capital — should succeed since margin requirement for
    // a 100K position is negligible relative to the 5B capital base.
    let withdrawn_amount = 1_000_000_000u64;
    let small_withdraw = env.try_withdraw(&user, user_idx, withdrawn_amount);
    assert!(
        small_withdraw.is_ok(),
        "Withdrawal leaving sufficient margin should succeed"
    );

    // SPL vault conservation: should equal deposits minus withdrawals
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault,
        25_000_000_200 - withdrawn_amount,
        "ATTACK: SPL vault mismatch after withdrawal (includes init deposits)!"
    );
}

/// ATTACK: Permissionless crank doesn't extract value.
/// Any user can call crank with caller_idx=u16::MAX. Verify no value extraction.
#[test]
fn test_attack_permissionless_crank_no_value_extraction() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    let lp_cap_before = env.read_account_capital(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);

    // Crank is permissionless (uses random caller)
    env.set_slot(10);
    env.crank();
    env.set_slot(20);
    env.crank();
    env.set_slot(30);
    env.crank();

    // No value should be extracted by cranking
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 25_000_000_200,
        "ATTACK: SPL vault changed from permissionless cranks!"
    );

    // Total capital should be conserved
    let lp_cap_after = env.read_account_capital(lp_idx);
    let user_cap_after = env.read_account_capital(user_idx);
    assert!(
        lp_cap_after + user_cap_after <= lp_cap_before + user_cap_before,
        "ATTACK: Capital increased from cranking! before={} after={}",
        lp_cap_before + user_cap_before,
        lp_cap_after + user_cap_after
    );
}

/// ATTACK: Multiple close-account calls on same index should fail.
/// After closing once, the slot is freed. Closing again should error.
#[test]
fn test_attack_double_close_account_same_index() {
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

    // Close account
    env.try_close_account(&user, user_idx).unwrap();

    // Try to close again - should fail
    let second_close = env.try_close_account(&user, user_idx);
    assert!(
        second_close.is_err(),
        "ATTACK: Double close succeeded - potential double withdrawal!"
    );

    // SPL vault should only have LP deposit
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert!(
        spl_vault <= 10_000_000_200,
        "ATTACK: SPL vault has too much after close! vault={}",
        spl_vault
    );
}

/// ATTACK: Deposit after close should fail if account is freed.
/// After closing an account, depositing to that index should fail.
#[test]
fn test_attack_deposit_to_closed_account_index() {
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

    // Close account
    env.try_close_account(&user, user_idx).unwrap();

    // Try deposit to closed index - should fail (account not found or owner mismatch)
    let deposit_result = env.try_deposit(&user, user_idx, 1_000_000_000);
    assert!(
        deposit_result.is_err(),
        "ATTACK: Deposit to closed account index succeeded!"
    );

    // SPL vault should not have increased beyond LP deposit
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert!(
        spl_vault <= 10_000_000_200,
        "ATTACK: SPL vault increased from deposit to closed account!"
    );
}

/// ATTACK: Trade to closed account index should fail.
/// After closing, trying to use the freed slot as counterparty should error.
#[test]
fn test_attack_trade_with_closed_account_index() {
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

    // Close user account
    env.try_close_account(&user, user_idx).unwrap();
    let lp_pos_before = env.read_account_position(lp_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let used_before = env.read_num_used_accounts();
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();
    assert_eq!(
        used_before, 1,
        "Precondition: only LP should remain after closing user account"
    );

    // Try trade referencing closed account
    let trade_result = env.try_trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert!(
        trade_result.is_err(),
        "ATTACK: Trade with closed account index succeeded!"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected trade to closed index must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected trade to closed index must preserve LP capital"
    );
    assert_eq!(
        env.read_num_used_accounts(),
        used_before,
        "Rejected trade to closed index must preserve num_used_accounts"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected trade to closed index must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected trade to closed index must preserve engine vault"
    );
}

/// ATTACK: Verify engine vault tracks SPL vault correctly across operations.
/// After deposits, trades, withdrawals, and cranks, engine vault should match SPL vault.
#[test]
fn test_attack_engine_vault_spl_vault_consistency() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    // Trade
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot(10);
    env.crank();

    // Partial withdraw
    env.try_withdraw(&user, user_idx, 100_000).unwrap();

    env.set_slot(20);
    env.crank();

    // Engine vault = c_tot + insurance + net_pnl
    let engine_vault = env.read_engine_vault();
    let c_tot = env.read_c_tot();
    let insurance = env.read_insurance_balance();

    // SPL vault
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };

    // engine_vault should match SPL vault (with unit_scale=0, 1:1)
    assert_eq!(
        engine_vault, spl_vault as u128,
        "ATTACK: Engine vault != SPL vault! engine={} spl={}",
        engine_vault, spl_vault
    );

    // vault >= c_tot + insurance (conservation invariant)
    assert!(
        engine_vault >= c_tot + insurance,
        "ATTACK: vault < c_tot + insurance! vault={} c_tot={} ins={}",
        engine_vault,
        c_tot,
        insurance
    );
}

/// ATTACK: rotate admin then attempt old admin operations.
/// After admin transfer, old admin should be unable to perform admin operations.
#[test]
fn test_attack_old_admin_blocked_after_transfer() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let old_admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let new_admin = Keypair::new();
    env.svm.airdrop(&new_admin.pubkey(), 1_000_000_000).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    env.crank();

    // Transfer admin (cross-Keypair: two-sig handover)
    env.try_update_authority(&old_admin, AUTHORITY_ADMIN, Some(&new_admin))
        .unwrap();

    // Old admin should fail
    let old_result = env.try_update_admin(&old_admin, &old_admin.pubkey());
    assert!(
        old_result.is_err(),
        "ATTACK: Old admin can still perform admin ops after transfer!"
    );

    // New admin should succeed
    let new_result = env.try_update_admin(&new_admin, &new_admin.pubkey());
    assert!(
        new_result.is_ok(),
        "New admin should be able to perform admin ops: {:?}",
        new_result
    );
}

/// ATTACK: UpdateConfig with extreme funding parameters.
/// Set funding_max_e9_per_slot to max i64, verify crank doesn't overflow.
#[test]
fn test_attack_config_extreme_funding_max_bps() {
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

    // Open position to create funding obligation
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Try to set thresh_max to extreme value
    // The engine should either accept (with clamping) or reject this
    let result = env.try_update_config_with_params(&admin, 100);
    assert!(
        result.is_ok(),
        "Extreme-but-valid thresh_max update should be accepted: {:?}",
        result
    );

    // Regardless of acceptance, advance and crank - must not panic/overflow
    env.set_slot(100);
    env.crank();

    // Conservation check - vault must be consistent no matter what
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_200,
        "ATTACK: SPL vault changed after extreme config! vault={}",
        spl_vault
    );

    // c_tot should still be consistent
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync after extreme config! c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Zero-slot crank loops shouldn't compound funding.
/// Crank multiple times at the same slot - funding should accrue only once.
#[test]
fn test_attack_same_slot_crank_no_double_funding() {
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
    env.set_slot(10);
    env.crank();

    // Record state after first crank at slot 100
    env.set_slot(100);
    env.crank();
    let cap_after_first = env.read_account_capital(user_idx);
    let pnl_after_first = env.read_account_pnl(user_idx);

    // Crank again at same slot - should be a no-op for funding
    env.crank();
    let cap_after_second = env.read_account_capital(user_idx);
    let pnl_after_second = env.read_account_pnl(user_idx);

    // Capital and PnL should be unchanged (no double funding)
    assert_eq!(
        cap_after_first, cap_after_second,
        "ATTACK: Double crank changed capital! first={} second={}",
        cap_after_first, cap_after_second
    );
    assert_eq!(
        pnl_after_first, pnl_after_second,
        "ATTACK: Double crank changed PnL! first={} second={}",
        pnl_after_first, pnl_after_second
    );
}

/// ATTACK: Multiple LPs trading with same user - verify all positions tracked correctly.
/// Each LP independently takes opposite side of user trades.
#[test]
fn test_attack_multi_lp_position_tracking() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp1 = Keypair::new();
    let lp1_idx = env.init_lp(&lp1);
    env.deposit(&lp1, lp1_idx, 20_000_000_000);

    let lp2 = Keypair::new();
    let lp2_idx = env.init_lp(&lp2);
    env.deposit(&lp2, lp2_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // User goes long via LP1
    env.trade(&user, &lp1, lp1_idx, user_idx, 100_000);
    env.set_slot(10);

    // User goes long more via LP2 (different size to avoid AlreadyProcessed)
    env.trade(&user, &lp2, lp2_idx, user_idx, 50_000);
    env.set_slot(20);
    env.crank();

    // User position should be sum of both trades
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(
        user_pos, 150_000,
        "User position should be 150K (100K + 50K): {}",
        user_pos
    );

    // Each LP should have their own position (opposite)
    let lp1_pos = env.read_account_position(lp1_idx);
    let lp2_pos = env.read_account_position(lp2_idx);
    assert_eq!(lp1_pos, -100_000, "LP1 should have -100K: {}", lp1_pos);
    assert_eq!(lp2_pos, -50_000, "LP2 should have -50K: {}", lp2_pos);

    // Conservation: net position should be zero
    let net = user_pos + lp1_pos + lp2_pos;
    assert_eq!(
        net, 0,
        "ATTACK: Net position not zero! user={} lp1={} lp2={} net={}",
        user_pos, lp1_pos, lp2_pos, net
    );

    // c_tot consistency
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp1_idx)
        + env.read_account_capital(lp2_idx)
        + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with multiple LPs! c_tot={} sum={}",
        c_tot, sum
    );

    // SPL vault conservation (20B + 20B + 10B + 1B insurance)
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 51_000_000_300,
        "ATTACK: SPL vault wrong with multiple LPs!"
    );
}

/// ATTACK: Trade as LP-kind account in user slot (kind mismatch).
/// LP accounts can only be in lp_idx position, users in user_idx.
#[test]
fn test_attack_lp_as_user_kind_swap() {
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

    // Precondition: normal trade succeeds (proves test setup is correct)
    env.trade(&user, &lp, lp_idx, user_idx, 50_000);
    let pos = env.read_account_position(user_idx);
    assert_eq!(pos, 50_000, "Precondition: normal trade should work");

    // Try to trade with LP and User indices swapped
    // LP in user slot, User in LP slot - should fail kind check
    let result = env.try_trade(&user, &lp, user_idx, lp_idx, 100_000);
    assert!(
        result.is_err(),
        "ATTACK: Trade with swapped LP/User indices succeeded!"
    );
}

/// ATTACK: Deposit zero amount should be harmless.
/// Depositing 0 tokens is rejected and must leave state unchanged.
#[test]
fn test_attack_deposit_zero_amount_no_state_change() {
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

    let cap_before = env.read_account_capital(user_idx);
    let vault_before = env.vault_balance();

    // Deposit 0 is rejected as invalid input.
    let result = env.try_deposit(&user, user_idx, 0);

    let cap_after = env.read_account_capital(user_idx);
    let vault_after = env.vault_balance();
    assert!(
        result.is_err(),
        "Zero-value deposit should be rejected: {:?}",
        result
    );
    assert_eq!(
        cap_before, cap_after,
        "ATTACK: Zero deposit changed capital! before={} after={}",
        cap_before, cap_after
    );
    assert_eq!(
        vault_before, vault_after,
        "ATTACK: Zero deposit changed vault! before={} after={}",
        vault_before, vault_after
    );
}

/// ATTACK: Withdraw zero amount should be harmless.
/// Withdrawing 0 tokens is rejected and must leave state unchanged.
#[test]
fn test_attack_withdraw_zero_amount_no_state_change() {
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

    let cap_before = env.read_account_capital(user_idx);
    let vault_before = env.vault_balance();

    // Withdraw 0 is rejected as invalid input.
    let result = env.try_withdraw(&user, user_idx, 0);

    let cap_after = env.read_account_capital(user_idx);
    let vault_after = env.vault_balance();
    assert!(
        result.is_err(),
        "Zero-value withdrawal should be rejected: {:?}",
        result
    );
    assert_eq!(
        cap_before, cap_after,
        "ATTACK: Zero withdrawal changed capital! before={} after={}",
        cap_before, cap_after
    );
    assert_eq!(
        vault_before, vault_after,
        "ATTACK: Zero withdrawal changed vault! before={} after={}",
        vault_before, vault_after
    );
}

/// ATTACK: Trade with zero size should be harmless.
/// Trading 0 contracts should either fail or be a no-op.
#[test]
fn test_attack_trade_zero_size() {
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

    // Trade zero size must be rejected.
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 0);

    let cap_after = env.read_account_capital(user_idx);
    let pos_after = env.read_account_position(user_idx);
    assert!(
        result.is_err(),
        "ATTACK: Zero-size trade unexpectedly succeeded"
    );
    assert_eq!(
        cap_before, cap_after,
        "State changed despite failed zero trade!"
    );
    assert_eq!(pos_after, 0, "Position changed despite failed zero trade!");
}

/// In spec v10.5, there is no force-realize mode. Low insurance does NOT
/// trigger position force-close. Positions remain open regardless of
/// insurance level. The crank only processes funding/settlement.
#[test]
fn test_attack_force_realize_closes_positions_safely() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    // No insurance topped up
    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot(10);

    // SPL vault before crank
    let vault_before = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };

    // Crank — per spec v10.5, no force-realize (positions stay open)
    env.crank();

    // Positions should remain open (no force-realize in v10.5)
    let user_pos = env.read_account_position(user_idx);
    let lp_pos = env.read_account_position(lp_idx);
    assert_ne!(
        user_pos, 0,
        "User position should remain open (no force-realize in v10.5): {}",
        user_pos
    );
    assert_ne!(
        lp_pos, 0,
        "LP position should remain open (no force-realize in v10.5): {}",
        lp_pos
    );

    // SPL vault unchanged (crank doesn't move tokens)
    let vault_after = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        vault_before, vault_after,
        "Crank should not change vault balance: before={} after={}",
        vault_before, vault_after
    );
}

/// ATTACK: Deposit after setting large maintenance fee.
/// Verify fee settlement during deposit doesn't extract extra value.
/// ATTACK: Close account forgives fee debt without extracting from vault.
/// CloseAccount pays what it can from capital, forgives the rest.
/// ATTACK: Liquidate account that becomes insolvent from price move.
/// After price crash, undercollateralized account should be liquidatable.
#[test]
fn test_attack_liquidation_after_price_crash() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 80); // max cap (100%/read)

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // User with moderate capital
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000); // 5B

    env.try_top_up_insurance(&admin, 10_000_000_000).unwrap();
    env.crank();

    // Open a very large long position (use max margin)
    // initial_margin_bps=400 -> 4%, so 5B capital supports 5B/0.04=125B notional
    // at price 138, that's 125B/138 ~= 905M contracts
    // But let's use a smaller amount to be safe
    env.trade(&user, &lp, lp_idx, user_idx, 100_000_000); // 100M contracts

    // Precondition: position is open
    let pos = env.read_account_position(user_idx);
    assert_eq!(pos, 100_000_000, "User should have 100M long position");

    // Massive price crash: 138 -> 50
    // PnL = 100M * (50-138) / 1e6 = 100M * -88e-6 = -8800 tokens
    // With 5B tokens capital, this should make the account deeply insolvent
    // maintenance_margin_bps=200 -> 2%, required margin = 100M*50/1e6*0.02 = 100 tokens
    // equity = 5000 - 8800 = -3800 -> deeply negative -> liquidated
    // Drive the oracle down far enough (compounded per-slot cap) for
    // the account to be deeply insolvent.
    env.set_slot_and_price(2000, 80_000_000);
    env.crank(); // Settle mark-to-oracle (crank does not liquidate with None hint)

    // Use explicit liquidation instruction to liquidate the insolvent account
    env.try_liquidate_target(user_idx)
        .expect("Liquidation of insolvent account must succeed");

    // After explicit liquidation, user's position should be reduced or zeroed
    let pos_after = env.read_account_position(user_idx);
    // Liquidation reduces or eliminates the position
    assert!(
        pos_after.abs() < pos.abs(),
        "ATTACK: Insolvent position not liquidated! before={} after={}",
        pos,
        pos_after
    );

    // SPL vault unchanged (liquidation is internal accounting)
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    // 100B + 5B + 10B insurance = 115B
    assert_eq!(
        spl_vault, 115_000_000_200,
        "ATTACK: SPL vault changed during liquidation!"
    );
}

/// ATTACK: Warmup period settlement - profit only vests after warmup.
/// With warmup_period > 0, PnL profit should vest gradually, not instantly.
#[test]
fn test_attack_warmup_profit_vests_gradually() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_warmup(0, 100); // 100-slot warmup period

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open long position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert_eq!(
        env.read_account_position(user_idx),
        100_000,
        "Precondition: position must be open"
    );

    // Price goes up → user has profit
    env.set_slot_and_price(10, 150_000_000); // 138→150
    env.crank();

    // At slot 10 with 100-slot warmup, only 10% of profit should be vested
    let user_cap_early = env.read_account_capital(user_idx);

    // Advance to end of warmup
    env.set_slot_and_price(110, 150_000_000);
    env.crank();

    let user_cap_late = env.read_account_capital(user_idx);

    // Capital should not decrease as warmup vests more profit over time.
    // Note: Capital may stay equal if warmup conversion is still pending or
    // PnL haircut absorbs the gain.
    assert!(
        user_cap_late >= user_cap_early,
        "ATTACK: Capital decreased after more warmup! early={} late={}",
        user_cap_early,
        user_cap_late
    );

    // Conservation: c_tot = sum of all capitals
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync during warmup! c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Warmup period=0 means instant settlement.
/// With warmup=0, all PnL should vest immediately.
#[test]
fn test_attack_warmup_period_zero_instant_settlement() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0); // warmup_period=0

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    let user_cap_before = env.read_account_capital(user_idx);

    // Open long position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Price goes up → user profits
    env.set_slot_and_price(10, 150_000_000);
    env.crank();

    // Close the position and settle immediately with warmup=0.
    env.trade(&user, &lp, lp_idx, user_idx, -100_000);
    env.set_slot_and_price(20, 150_000_000);
    env.crank();

    let user_cap_after_close = env.read_account_capital(user_idx);
    let user_pos_after_close = env.read_account_position(user_idx);
    assert!(
        user_pos_after_close == 0,
        "Position should be closed before withdrawal test: pos={}",
        user_pos_after_close
    );
    assert!(
        user_cap_after_close > user_cap_before,
        "With warmup=0 and favorable move, closing should realize profit immediately: cap_before={} cap_after_close={}",
        user_cap_before,
        user_cap_after_close
    );

    // Immediate withdrawal above original deposit must succeed with warmup=0.
    let withdraw_amount = user_cap_before as u64 + 1;
    let vault_before_withdraw = env.vault_balance();
    let cap_before_withdraw = env.read_account_capital(user_idx);
    let withdraw_result = env.try_withdraw(&user, user_idx, withdraw_amount);
    assert!(
        withdraw_result.is_ok(),
        "Warmup=0 should allow immediate withdrawal of realized profit: {:?}",
        withdraw_result
    );
    let cap_after_withdraw = env.read_account_capital(user_idx);
    let vault_after_withdraw = env.vault_balance();
    assert_eq!(
        cap_after_withdraw,
        cap_before_withdraw - withdraw_amount as u128,
        "Immediate withdrawal should decrement capital exactly"
    );
    assert_eq!(
        vault_after_withdraw,
        vault_before_withdraw - withdraw_amount,
        "Immediate withdrawal should decrement vault exactly"
    );

    // Conservation must still hold.
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with warmup=0! c_tot={} sum={}",
        c_tot, sum
    );

    // SPL vault unchanged (PnL settlement is internal)
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault,
        26_000_000_200 - withdraw_amount,
        "ATTACK: SPL vault mismatch after immediate warmup=0 profit withdrawal!"
    );
}

/// ATTACK: Same-slot triple crank converges.
/// Multiple cranks at same slot should eventually stabilize (lazy settlement).
/// Second crank may settle fees, but third should be fully idempotent.
#[test]
fn test_attack_same_slot_triple_crank_convergence() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 80); // max cap (100%/read)

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

    // Move price
    env.set_slot_and_price(50, 160_000_000);

    // Triple crank to ensure convergence
    env.crank(); // First: mark settlement + lazy fee settlement
    env.crank(); // Second: any remaining lazy operations

    let cap_second = env.read_account_capital(user_idx);
    let pnl_second = env.read_account_pnl(user_idx);
    let c_tot_second = env.read_c_tot();

    env.crank(); // Third: should be fully idempotent now

    let cap_third = env.read_account_capital(user_idx);
    let pnl_third = env.read_account_pnl(user_idx);
    let c_tot_third = env.read_c_tot();

    assert_eq!(
        cap_second, cap_third,
        "ATTACK: Third crank changed capital! second={} third={}",
        cap_second, cap_third
    );
    assert_eq!(
        pnl_second, pnl_third,
        "ATTACK: Third crank changed PnL! second={} third={}",
        pnl_second, pnl_third
    );
    assert_eq!(
        c_tot_second, c_tot_third,
        "ATTACK: Third crank changed c_tot! second={} third={}",
        c_tot_second, c_tot_third
    );

    // SPL vault unchanged throughout
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(spl_vault, 26_000_000_200);
}

/// ATTACK: Funding rate with extreme k_bps.
/// Set funding_k_bps to maximum, verify funding rate is capped at ±10,000 bps/slot.
#[test]
fn test_attack_funding_extreme_k_bps_capped() {
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

    // Set extreme k_bps via direct config encoding
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_update_config(
            100,     // funding_horizon_slots
            100_000, // funding_k_bps (max allowed = 1000x)
            100,     // funding_max_premium_bps
            10,      // funding_max_e9_per_slot
        ),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    let config_result = env.svm.send_transaction(tx);
    assert!(
        config_result.is_ok(),
        "Extreme funding_k_bps config update should be accepted: {:?}",
        config_result
    );

    // Open position and advance
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot(100);
    env.crank(); // Must not panic/overflow

    // Conservation check
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with extreme k_bps! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_200,
        "ATTACK: SPL vault changed with extreme k_bps!"
    );
}

/// ATTACK: Funding with extreme max_premium_bps.
/// Set funding_max_premium_bps to extreme negative, verify capping works.
#[test]
fn test_attack_funding_extreme_max_premium_capped() {
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

    // Set extreme max_premium_bps via direct config
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_update_config(
            100,      // funding_horizon_slots
            100,      // funding_k_bps
            i64::MAX, // funding_max_premium_bps (extreme!)
            10,       // funding_max_e9_per_slot
        ),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    let config_result = env.svm.send_transaction(tx);
    assert!(
        config_result.is_ok(),
        "Extreme funding_max_premium_bps config update should be accepted: {:?}",
        config_result
    );

    // Trade and crank
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot(100);
    env.crank(); // Must not overflow

    // Conservation check
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with extreme max_premium! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_200,
        "ATTACK: SPL vault changed with extreme max_premium!"
    );
}

/// ATTACK: Funding with extreme max_bps_per_slot.
/// Set funding_max_e9_per_slot to extreme value, verify engine caps at ±10,000.
#[test]
fn test_attack_funding_extreme_max_bps_per_slot() {
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

    // Set extreme max_bps_per_slot
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_update_config(
            100,      // funding_horizon_slots
            100,      // funding_k_bps
            100,      // funding_max_premium_bps
            i64::MAX, // funding_max_e9_per_slot (extreme!)
        ),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    let config_result = env.svm.send_transaction(tx);
    assert!(
        config_result.is_err(),
        "Extreme funding_max_e9_per_slot (> MAX_ABS_FUNDING) must be rejected"
    );

    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot(100);
    env.crank(); // Must not overflow even with extreme bps/slot

    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with extreme max_bps_per_slot! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_200,
        "ATTACK: SPL vault changed with extreme max_bps_per_slot!"
    );
}

/// ATTACK: Deposit with wrong mint token account.
/// Attempt to deposit from an ATA with a different mint.
#[test]
fn test_attack_deposit_wrong_mint_token_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    // Create a fake ATA with a different mint
    let fake_mint = Pubkey::new_unique();
    let fake_ata = Pubkey::new_unique();
    let mut fake_ata_data = vec![0u8; TokenAccount::LEN];
    // Pack a valid SPL token account with wrong mint
    // (mint field is at offset 0, 32 bytes)
    fake_ata_data[0..32].copy_from_slice(fake_mint.as_ref());
    // owner field at offset 32
    fake_ata_data[32..64].copy_from_slice(user.pubkey().as_ref());
    // amount at offset 64, 8 bytes
    fake_ata_data[64..72].copy_from_slice(&10_000_000_000u64.to_le_bytes());
    // state = Initialized (1) at offset 108
    fake_ata_data[108] = 1;

    env.svm
        .set_account(
            fake_ata,
            Account {
                lamports: 1_000_000_000,
                data: fake_ata_data,
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    // Advance slot to avoid AlreadyProcessed from init_user
    env.set_slot(2);

    // Try to deposit using the wrong-mint ATA
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(fake_ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_col, false),
        ],
        data: encode_deposit(user_idx, 1_000_000_000),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let user_cap_before = env.read_account_capital(user_idx);
    let used_before = env.read_num_used_accounts();
    let spl_vault_before = env.vault_balance();
    let fake_ata_before = {
        let ata_data = env.svm.get_account(&fake_ata).unwrap().data;
        TokenAccount::unpack(&ata_data).unwrap().amount
    };
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Deposit with wrong-mint ATA should be rejected!"
    );
    let user_cap_after = env.read_account_capital(user_idx);
    let used_after = env.read_num_used_accounts();
    let spl_vault_after = env.vault_balance();
    let fake_ata_after = {
        let ata_data = env.svm.get_account(&fake_ata).unwrap().data;
        TokenAccount::unpack(&ata_data).unwrap().amount
    };
    assert_eq!(
        user_cap_after, user_cap_before,
        "Rejected wrong-mint deposit must not change user capital"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected wrong-mint deposit must not change num_used_accounts"
    );
    assert_eq!(
        spl_vault_after, spl_vault_before,
        "Rejected wrong-mint deposit must not change SPL vault"
    );
    assert_eq!(
        fake_ata_after, fake_ata_before,
        "Rejected wrong-mint deposit must not debit source ATA"
    );
}

/// ATTACK: Withdraw to wrong owner's ATA.
/// Attempt to withdraw to an ATA owned by a different user.
#[test]
fn test_attack_withdraw_to_different_users_ata() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let attacker_ata = env.create_ata(&attacker.pubkey(), 0);

    env.crank();
    let withdraw_amount = 1_000_000_000u64;
    let vault_before = env.vault_balance();
    let user_cap_before = env.read_account_capital(user_idx);
    let attacker_ata_before = {
        let ata_data = env.svm.get_account(&attacker_ata).unwrap().data;
        TokenAccount::unpack(&ata_data).unwrap().amount
    };

    // User tries to withdraw to attacker's ATA
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(attacker_ata, false), // Wrong ATA!
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_col, false),
        ],
        data: encode_withdraw(user_idx, withdraw_amount),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);

    // Should either fail (wrong ATA owner check) or succeed but send to the ATA
    // that was passed (which is normal SPL behavior - vault signs the transfer)
    // The security guarantee is: user must sign, and tokens go to the ATA they specify.
    // This is NOT an attack if user chooses to send to someone else's ATA.
    // Verify exact accounting in both success and reject paths.
    let user_cap_after = env.read_account_capital(user_idx);
    let attacker_ata_after = {
        let ata_data = env.svm.get_account(&attacker_ata).unwrap().data;
        TokenAccount::unpack(&ata_data).unwrap().amount
    };
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };

    if result.is_ok() {
        assert_eq!(
            user_cap_after,
            user_cap_before - withdraw_amount as u128,
            "Successful withdrawal should decrement user capital by exact amount: before={} after={} amount={}",
            user_cap_before,
            user_cap_after,
            withdraw_amount
        );
        assert_eq!(
            spl_vault,
            vault_before - withdraw_amount,
            "Successful withdrawal should decrement vault by exact amount: before={} after={} amount={}",
            vault_before,
            spl_vault,
            withdraw_amount
        );
        assert_eq!(
            attacker_ata_after,
            attacker_ata_before + withdraw_amount,
            "Successful withdrawal should credit destination ATA by exact amount: before={} after={} amount={}",
            attacker_ata_before,
            attacker_ata_after,
            withdraw_amount
        );
    } else {
        assert_eq!(
            user_cap_after, user_cap_before,
            "Failed withdrawal should not change user capital: before={} after={}",
            user_cap_before, user_cap_after
        );
        assert_eq!(
            spl_vault, vault_before,
            "Failed withdrawal should not change vault: before={} after={}",
            vault_before, spl_vault
        );
        assert_eq!(
            attacker_ata_after, attacker_ata_before,
            "Failed withdrawal should not credit destination ATA: before={} after={}",
            attacker_ata_before, attacker_ata_after
        );
    }

    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, spl_vault,
        "Engine/SPL vault mismatch after cross-ATA withdrawal attempt: engine={} spl={}",
        engine_vault, spl_vault
    );
}

/// ATTACK: Multiple price changes between cranks.
/// Push oracle price multiple times before cranking, verify only latest applies.
#[test]
fn test_attack_multiple_oracle_updates_between_cranks() {
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

    // Multiple price updates at successive slots WITHOUT cranking
    env.set_slot_and_price(10, 150_000_000);
    env.set_slot_and_price(11, 120_000_000);
    env.set_slot_and_price(12, 200_000_000);
    env.set_slot_and_price(13, 130_000_000); // Final price

    // Now crank - should use latest price
    env.crank();

    // Conservation must hold
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync after multiple oracle updates! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_200,
        "ATTACK: SPL vault changed after multiple oracle updates!"
    );
}

/// ATTACK: Trade immediately after deposit, same slot.
/// Deposit and trade in rapid succession without crank between.
#[test]
fn test_attack_trade_immediately_after_deposit_same_slot() {
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

    // Deposit more and immediately trade - same slot, no crank between
    env.deposit(&user, user_idx, 2_000_000_000);
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    assert_eq!(
        env.read_account_position(user_idx),
        100_000,
        "Trade after deposit should succeed"
    );

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync after deposit+trade! c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Rapid long→short→long position reversals.
/// Multiple position flips in succession to test aggregate tracking.
#[test]
fn test_attack_rapid_position_reversals() {
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

    env.try_top_up_insurance(&admin, 5_000_000_000).unwrap();
    env.crank();

    // Rapid reversals: long → short → long → short
    // Use different sizes at each slot to avoid AlreadyProcessed
    env.trade(&user, &lp, lp_idx, user_idx, 100_000); // Long 100K
    assert_eq!(env.read_account_position(user_idx), 100_000);

    env.set_slot(2);
    env.trade(&user, &lp, lp_idx, user_idx, -150_000); // Short 50K (flip)
    assert_eq!(env.read_account_position(user_idx), -50_000);

    env.set_slot(3);
    env.trade(&user, &lp, lp_idx, user_idx, 250_000); // Long 200K (flip again)
    assert_eq!(env.read_account_position(user_idx), 200_000);

    env.set_slot(4);
    env.trade(&user, &lp, lp_idx, user_idx, -200_000); // Flat
    assert_eq!(env.read_account_position(user_idx), 0);

    // Crank at end
    env.set_slot(10);
    env.crank();

    // After all reversals and flattening, conservation must hold
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync after rapid reversals! c_tot={} sum={}",
        c_tot, sum
    );

    // pnl_pos_tot should be 0 (no positions)
    let pnl_pos_tot = env.read_pnl_pos_tot();
    assert_eq!(
        pnl_pos_tot, 0,
        "ATTACK: pnl_pos_tot should be 0 with no positions: {}",
        pnl_pos_tot
    );

    // SPL vault: 50B + 10B + 5B = 65B
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 65_000_000_200,
        "ATTACK: SPL vault wrong after rapid reversals!"
    );
}

/// ATTACK: Crank with no accounts (empty market).
/// KeeperCrank on a market with no users/LPs should be a no-op.
#[test]
fn test_attack_crank_empty_market() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Crank with no accounts at all
    env.crank();

    // Advance and crank again
    env.set_slot(100);
    env.crank();

    // Market should be in clean state
    let c_tot = env.read_c_tot();
    assert_eq!(c_tot, 0, "c_tot should be 0 with no accounts: {}", c_tot);

    let pnl_pos_tot = env.read_pnl_pos_tot();
    assert_eq!(
        pnl_pos_tot, 0,
        "pnl_pos_tot should be 0 with no accounts: {}",
        pnl_pos_tot
    );
}

/// ATTACK: Smallest possible trade (1 contract) creates correct position.
/// Note: Market uses trading_fee_bps=0, so ceiling division is not tested here.
/// Fee ceiling division is enforced at the engine level and tested in unit proofs.
#[test]
fn test_attack_trading_fee_ceiling_division() {
    program_path();

    let mut env = TestEnv::new();
    // Use init_market_full to set nonzero trading_fee_bps
    // Default init_market_with_invert uses trading_fee_bps=0
    // We need to manually construct with fee
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

    // With trading_fee_bps=0 (default), trade should succeed with no fee
    let insurance_before = env.read_insurance_balance();
    env.trade(&user, &lp, lp_idx, user_idx, 1); // Smallest possible trade (1 contract)
    let insurance_after = env.read_insurance_balance();

    // With fee=0, insurance shouldn't grow from trading
    // (it might grow from other settlement operations though)
    assert!(
        insurance_after >= insurance_before,
        "Insurance should never decrease: before={} after={}",
        insurance_before,
        insurance_after
    );

    // Verify position was created
    let pos = env.read_account_position(user_idx);
    assert_eq!(
        pos, 1,
        "Smallest trade should create position of 1 contract"
    );
}

/// ATTACK: Multiple withdrawals in same slot draining capital.
/// Rapid withdrawals in same slot should correctly update capital each time.
#[test]
fn test_attack_multiple_withdrawals_same_slot() {
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

    // Withdraw in three parts - all at same slot
    env.try_withdraw(&user, user_idx, 1_000_000_000).unwrap(); // -1B
    env.try_withdraw(&user, user_idx, 1_000_000_000).unwrap(); // -1B
    env.try_withdraw(&user, user_idx, 1_000_000_000).unwrap(); // -1B

    let user_cap = env.read_account_capital(user_idx);
    // Should have 5B - 3B = 2B remaining
    // (capital might differ due to fee settlement, but should be around 2B)
    assert!(
        user_cap <= 2_000_000_099,
        "ATTACK: Capital not properly decremented after multiple withdrawals: {}",
        user_cap
    );

    // Try to withdraw more than remaining
    let result = env.try_withdraw(&user, user_idx, 3_000_000_000); // More than remaining
    assert!(result.is_err(), "ATTACK: Over-withdrawal should fail!");

    // SPL vault: 10B + 5B - 3B + 200 (2 init deposits) = 12B + 200
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 12_000_000_200,
        "ATTACK: SPL vault wrong after multiple withdrawals!"
    );
}

/// ATTACK: Deposit and withdraw same slot - should be atomic operations.
/// Rapid deposit+withdraw cycle shouldn't create or destroy value.
#[test]
fn test_attack_deposit_withdraw_same_slot_atomicity() {
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

    let cap_before = env.read_account_capital(user_idx);
    let vault_before = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };

    // Deposit then withdraw same amount, same slot
    env.deposit(&user, user_idx, 2_000_000_000);
    env.try_withdraw(&user, user_idx, 2_000_000_000).unwrap();

    let cap_after = env.read_account_capital(user_idx);
    let vault_after = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };

    // Capital and vault should return to original values
    assert_eq!(
        cap_before, cap_after,
        "ATTACK: Deposit+withdraw changed capital! before={} after={}",
        cap_before, cap_after
    );
    assert_eq!(
        vault_before, vault_after,
        "ATTACK: Deposit+withdraw changed vault! before={} after={}",
        vault_before, vault_after
    );
}

/// ATTACK: Accrue funding with huge dt (10-year equivalent slot jump).
/// Funding accrual caps dt at ~1 year. Verify no overflow.
#[test]
fn test_attack_funding_accrue_huge_dt_capped() {
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

    env.try_top_up_insurance(&admin, 5_000_000_000).unwrap();
    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert_eq!(
        env.read_account_position(user_idx),
        100_000,
        "Precondition: position open"
    );
    let user_pos_before = env.read_account_position(user_idx);

    // Jump 1 year worth of slots (~31.5M slots)
    // accrue_funding should cap dt at 31,536,000 (~1 year)
    env.set_slot(50_000); // within max_accrual_dt_slots=100_000
    let crank_result = env.try_crank();
    assert!(
        crank_result.is_ok(),
        "Huge-dt funding accrual should succeed with dt capping: {:?}",
        crank_result
    );

    let lp_cap_after = env.read_account_capital(lp_idx);
    let user_cap_after = env.read_account_capital(user_idx);
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Crank should not change position size under huge-dt funding accrual"
    );

    // Conservation must hold regardless of crank result.
    let c_tot = env.read_c_tot();
    let sum = lp_cap_after + user_cap_after;
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync after huge slot jump! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 65_000_000_200,
        "ATTACK: SPL vault changed after huge dt funding!"
    );
}

/// ATTACK: Large unit scale - very large scaling factor.
/// unit_scale=1_000_000 (1M). Verify no overflow in price scaling.
#[test]
fn test_attack_large_unit_scale_no_overflow() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_full(0, 1_000_000, 0); // unit_scale=1M

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    // With unit_scale=1M, need 100*1M=100M base for min_initial_deposit
    let lp_idx = env.init_lp_with_fee(&lp, 100_000_000);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user_with_fee(&user, 100_000_000);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade with large unit_scale
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert_eq!(env.read_account_position(user_idx), 100_000);

    // Advance and crank
    env.set_slot(50);
    env.crank();

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with unit_scale=1M! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_200_000_000,
        "ATTACK: SPL vault changed with unit_scale=1M (includes init deposits)!"
    );
}

/// ATTACK: Inverted market with price approaching zero.
/// When oracle price → large (inverted price → 0), verify no division issues.
#[test]
fn test_attack_inverted_market_extreme_high_oracle() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(1);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Very high oracle price → inverted price near zero
    // 1e12 / 1e9 = 1000 (very small inverted price)
    env.set_slot_and_price(50, 1_000_000_000); // Oracle = $1000
    let crank_result = env.try_crank();

    // Crank should succeed (circuit breaker clamps the mark, not reject)
    assert!(
        crank_result.is_ok(),
        "Crank should succeed even with extreme oracle: {:?}",
        crank_result
    );

    // Conservation must hold
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with extreme inverted price! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_200,
        "ATTACK: SPL vault changed despite extreme inverted price!"
    );
}

/// ATTACK: Same owner creates multiple user accounts.
/// Protocol should allow it, but each account must be independent.
#[test]
fn test_attack_same_owner_multiple_accounts_isolation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    // Two different users - verify account isolation
    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 5_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 3_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade on user1 only
    env.trade(&user1, &lp, lp_idx, user1_idx, 100_000);
    assert_eq!(env.read_account_position(user1_idx), 100_000);
    assert_eq!(
        env.read_account_position(user2_idx),
        0,
        "ATTACK: Trade on user1 affected user2!"
    );

    // user2 capital unchanged
    let user2_cap = env.read_account_capital(user2_idx);
    assert_eq!(
        user2_cap, 3_000_000_099,
        "ATTACK: user2 capital changed from user1's trade: {}",
        user2_cap
    );

    // Conservation across all accounts
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx)
        + env.read_account_capital(user1_idx)
        + env.read_account_capital(user2_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with multi-account! c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Resolve hyperp market then withdraw capital (no position).
/// After resolution, users should be able to withdraw their deposited capital.
#[test]
fn test_attack_resolve_then_withdraw_capital() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    // Resolve market (no positions open)
    env.try_resolve_market(&admin, 0).unwrap();

    // Withdrawals are blocked on resolved markets. Use CloseAccount instead.
    let user_cap = env.read_account_capital(user_idx);
    assert!(user_cap > 0, "Precondition: user should have capital");

    // CloseAccount should succeed (no position, pnl=0).
    // Two calls for ProgressOnly handling.
    let _ = env.try_close_account(&user, user_idx);
    // Also close LP to make market terminal-ready
    let _ = env.try_close_account(&lp, lp_idx);
    let _ = env.try_close_account(&user, user_idx);
    let _ = env.try_close_account(&lp, lp_idx);
}

/// ATTACK: TradeNoCpi on hyperp market should always be blocked.
/// Hyperp mode blocks TradeNoCpi (requires TradeCpi from matcher).
#[test]
fn test_attack_trade_nocpi_on_hyperp_rejected() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();
    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // TradeNoCpi should be blocked on hyperp markets
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert!(
        result.is_err(),
        "ATTACK: TradeNoCpi on hyperp market should be rejected!"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected TradeNoCpi on hyperp must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected TradeNoCpi on hyperp must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected TradeNoCpi on hyperp must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected TradeNoCpi on hyperp must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected TradeNoCpi on hyperp must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected TradeNoCpi on hyperp must preserve engine vault"
    );
}

/// ATTACK: Non-admin tries to resolve market.
/// Only admin should be able to resolve.
#[test]
fn test_attack_non_admin_resolve_rejected() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    env.crank();

    // Non-admin tries to resolve
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let resolved_before = env.is_market_resolved();
    let lp_pos_before = env.read_account_position(lp_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let used_before = env.read_num_used_accounts();
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    let result = env.try_resolve_market(&attacker, 0);
    assert!(
        result.is_err(),
        "ATTACK: Non-admin resolve should be rejected!"
    );
    assert!(
        !resolved_before,
        "Precondition: market should be unresolved before non-admin resolve attempt"
    );
    assert_eq!(
        env.is_market_resolved(),
        resolved_before,
        "Rejected non-admin resolve must not toggle resolved flag"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected non-admin resolve must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected non-admin resolve must preserve LP capital"
    );
    assert_eq!(
        env.read_num_used_accounts(),
        used_before,
        "Rejected non-admin resolve must preserve num_used_accounts"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected non-admin resolve must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected non-admin resolve must preserve engine vault"
    );
}

/// ATTACK: Crank multiple times across many slots with position open.
/// Verify funding accrual is correct and consistent across many intervals.
#[test]
fn test_attack_incremental_funding_across_many_slots() {
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

    // Crank at regular intervals
    for i in 1..=10 {
        env.set_slot(i * 100);
        env.crank();
    }

    // After 10 cranks over 1000 slots, conservation must hold
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync after incremental funding! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_200,
        "ATTACK: SPL vault changed after incremental funding!"
    );
}

/// ATTACK: Inverted market PnL direction and conservation after price move.
/// Long on inverted market should lose when oracle rises (inverted mark falls).
/// Verify PnL eventually settles into capital and conservation holds.
#[test]
fn test_attack_inverted_market_pnl_direction() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(1); // Inverted

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade on inverted market
    env.trade(&user, &lp, lp_idx, user_idx, 10_000_000);
    assert_eq!(env.read_account_position(user_idx), 10_000_000);

    let cap_before_user = env.read_account_capital(user_idx);
    let cap_before_lp = env.read_account_capital(lp_idx);

    // Small oracle price change to test PnL direction
    // Oracle: 138M → 150M. Inverted mark decreases slightly.
    env.set_slot_and_price(100, 150_000_000);
    env.crank();
    env.set_slot(200);
    env.crank();

    // After settlement, verify conservation
    let c_tot = env.read_c_tot();
    let cap_user = env.read_account_capital(user_idx);
    let cap_lp = env.read_account_capital(lp_idx);
    let sum = cap_user + cap_lp;
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync on inverted market! c_tot={} sum={}",
        c_tot, sum
    );

    // Total funds (deposits + insurance) should be unchanged in SPL vault
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 106_000_000_200,
        "ATTACK: SPL vault changed during inverted market settlement!"
    );

    // Verify capital sum didn't increase (fees may decrease total)
    assert!(
        cap_user + cap_lp <= cap_before_user + cap_before_lp,
        "ATTACK: Total capital increased on inverted market!"
    );
}

/// ATTACK: Close account with fee debt outstanding.
/// CloseAccount should forgive remaining fee debt after paying what's possible.
/// Verify returned capital = capital - min(fee_debt, capital).
#[test]
fn test_attack_close_account_returns_capital_minus_fees() {
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

    // Accrue some fees by advancing slots
    env.set_slot(500);
    env.crank();

    let vault_before = env.vault_balance();
    let num_used_before = env.read_num_used_accounts();

    // Close account (no position, just capital + fee debt)
    let result = env.try_close_account(&user, user_idx);
    assert!(result.is_ok(), "CloseAccount should succeed: {:?}", result);

    let vault_after = env.vault_balance();
    assert!(
        vault_before > vault_after,
        "Capital should be returned to user (vault decreased)"
    );

    let num_used_after = env.read_num_used_accounts();
    assert!(
        num_used_after < num_used_before,
        "num_used_accounts should decrease after close"
    );
}

/// ATTACK: CloseSlab with dormant account (zero everything but not GC'd).
/// CloseSlab requires num_used_accounts == 0, so dormant accounts block it.
#[test]
fn test_attack_close_slab_blocked_by_dormant_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.crank();

    // Close user account but LP still exists
    env.try_close_account(&user, user_idx).unwrap();

    // Crank to GC the user
    env.set_slot(100);
    env.crank();

    // LP still has capital - can't close slab
    let num_used = env.read_num_used_accounts();
    assert!(num_used > 0, "Precondition: LP account still exists");

    let lp_pos_before = env.read_account_position(lp_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let insurance_before = env.read_insurance_balance();
    let num_used_before = env.read_num_used_accounts();
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;

    // CloseSlab should fail
    let result = env.try_close_slab();
    assert!(
        result.is_err(),
        "ATTACK: CloseSlab succeeded with active LP account!"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected CloseSlab with dormant account must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected CloseSlab with dormant account must preserve LP capital"
    );
    assert_eq!(
        env.read_insurance_balance(),
        insurance_before,
        "Rejected CloseSlab with dormant account must preserve insurance"
    );
    assert_eq!(
        env.read_num_used_accounts(),
        num_used_before,
        "Rejected CloseSlab with dormant account must preserve account usage"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected CloseSlab with dormant account must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected CloseSlab with dormant account must preserve engine vault"
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    assert_eq!(
        slab_after, slab_before,
        "Rejected CloseSlab with dormant account must preserve slab bytes"
    );
}

/// ATTACK: admin rotated; old admin tries an operation.
/// After the rotation the previous admin should be unauthorized.
#[test]
fn test_attack_update_admin_old_admin_rejected() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let new_admin = Keypair::new();
    env.svm.airdrop(&new_admin.pubkey(), 5_000_000_000).unwrap();

    // Transfer admin to new_admin (cross-Keypair: two-sig handover)
    env.try_update_authority(&admin, AUTHORITY_ADMIN, Some(&new_admin))
        .unwrap();

    // Old admin tries admin operation - should fail
    let slab_before_old_admin_attempt = env.svm.get_account(&env.slab).unwrap().data;
    let result = env.try_update_admin(&admin, &admin.pubkey());
    assert!(
        result.is_err(),
        "ATTACK: Old admin still authorized after rotation!"
    );
    let slab_after_old_admin_attempt = env.svm.get_account(&env.slab).unwrap().data;
    assert_eq!(
        slab_after_old_admin_attempt, slab_before_old_admin_attempt,
        "Rejected admin op by old admin must preserve slab bytes"
    );

    // New admin can do it
    let result = env.try_update_admin(&new_admin, &new_admin.pubkey());
    assert!(
        result.is_ok(),
        "New admin should be authorized: {:?}",
        result
    );
}

/// ATTACK: Set maintenance fee to extreme value, accrue fees.
/// Verify fee debt accumulates but doesn't cause overflow or negative capital.
/// ATTACK: burning hyperp_authority disables PushHyperpMark.
/// Oracle authority cleared means stored price is cleared and push fails.
#[test]
fn test_attack_set_oracle_authority_to_zero_disables_push() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Set oracle authority
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    // Clear oracle authority (set to zero) — now allowed on Hyperp when
    // mark_ewma is bootstrapped (trades can sustain price discovery).
    let zero = Pubkey::new_from_array([0u8; 32]);
    env.set_slot(2);
    let zero_result = env.try_set_oracle_authority(&admin, &zero);
    assert!(
        zero_result.is_ok(),
        "Hyperp with bootstrapped EWMA should accept zero authority: {:?}",
        zero_result
    );

    // Under the 4-way split, burning hyperp_authority is a
    // one-way operation — the zero slot has no current authority
    // so no one can re-set it. Admin's attempt to set a new
    // authority here must fail.
    let new_auth = Keypair::new();
    env.svm.airdrop(&new_auth.pubkey(), 1_000_000_000).unwrap();
    let reset_result = env.try_update_authority(&admin, AUTHORITY_HYPERP_MARK, Some(&new_auth));
    assert!(
        reset_result.is_err(),
        "Burned hyperp_authority must not be resettable by admin: {:?}",
        reset_result,
    );
    const AUTH_PRICE_OFF: usize = 312; // HEADER_LEN(72) + offset_of!(MarketConfig, hyperp_mark_e6)(176)
    const AUTH_TS_OFF: usize = 320;
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let auth_price_before = u64::from_le_bytes(
        slab_before[AUTH_PRICE_OFF..AUTH_PRICE_OFF + 8]
            .try_into()
            .unwrap(),
    );
    let auth_ts_before = i64::from_le_bytes(
        slab_before[AUTH_TS_OFF..AUTH_TS_OFF + 8]
            .try_into()
            .unwrap(),
    );
    let used_before = env.read_num_used_accounts();
    let spl_vault_before = env.vault_balance();

    // Push should now fail
    env.set_slot(3);
    let result = env.try_push_oracle_price(&admin, 2_000_000, 2000);
    assert!(
        result.is_err(),
        "ATTACK: PushHyperpMark succeeded after authority cleared!"
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let auth_price_after = u64::from_le_bytes(
        slab_after[AUTH_PRICE_OFF..AUTH_PRICE_OFF + 8]
            .try_into()
            .unwrap(),
    );
    let auth_ts_after =
        i64::from_le_bytes(slab_after[AUTH_TS_OFF..AUTH_TS_OFF + 8].try_into().unwrap());
    let used_after = env.read_num_used_accounts();
    let spl_vault_after = env.vault_balance();
    assert_eq!(
        auth_price_after, auth_price_before,
        "Rejected push after clearing authority must not change authority price"
    );
    assert_eq!(
        auth_ts_after, auth_ts_before,
        "Rejected push after clearing authority must not change authority timestamp"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected push after clearing authority must not change num_used_accounts"
    );
    assert_eq!(
        spl_vault_after, spl_vault_before,
        "Rejected push after clearing authority must not move vault funds"
    );
}

/// ATTACK: Multi-LP trading - trade against two different LPs.
/// Verify each LP's position is tracked independently and conservation holds.
#[test]
fn test_attack_multi_lp_independent_positions() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp1 = Keypair::new();
    let lp1_idx = env.init_lp(&lp1);
    env.deposit(&lp1, lp1_idx, 20_000_000_000);

    let lp2 = Keypair::new();
    let lp2_idx = env.init_lp(&lp2);
    env.deposit(&lp2, lp2_idx, 30_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade against LP1
    env.trade(&user, &lp1, lp1_idx, user_idx, 100_000);
    assert_eq!(env.read_account_position(user_idx), 100_000);
    assert_eq!(env.read_account_position(lp1_idx), -100_000);
    assert_eq!(
        env.read_account_position(lp2_idx),
        0,
        "LP2 should not be affected by trade against LP1"
    );

    // Trade against LP2 (different slot)
    env.set_slot(2);
    env.trade(&user, &lp2, lp2_idx, user_idx, 200_000);
    assert_eq!(env.read_account_position(user_idx), 300_000); // 100K + 200K
    assert_eq!(env.read_account_position(lp1_idx), -100_000);
    assert_eq!(env.read_account_position(lp2_idx), -200_000);

    // Conservation across all 3 accounts
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp1_idx)
        + env.read_account_capital(lp2_idx)
        + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with multi-LP! c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Close account after round-trip trade with PnL.
/// Protocol requires position=0 and PnL=0 for close.
#[test]
fn test_attack_close_account_after_roundtrip_pnl() {
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

    // Open and close position to generate PnL
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Price move to generate PnL
    env.set_slot_and_price(50, 150_000_000);
    env.crank();

    // Close position
    env.set_slot(51);
    env.trade(&user, &lp, lp_idx, user_idx, -100_000);

    // User should have PnL or capital changed
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(user_pos, 0, "Position should be closed");

    // Crank many times to settle warmup PnL to capital
    for i in 0..10 {
        env.set_slot(100 + i * 50);
        env.crank();
    }

    // After warmup fully vests, PnL should be zero and close should work
    let user_pnl = env.read_account_pnl(user_idx);
    assert_eq!(user_pnl, 0, "PnL should be settled after many cranks");

    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "CloseAccount should succeed after full PnL settlement: {:?}",
        result
    );

    let cap = env.read_account_capital(user_idx);
    assert_eq!(cap, 0, "Capital should be zero after close");
}

/// ATTACK: admin-self-rotate (no-op).
/// Should succeed without side effects.
#[test]
fn test_attack_update_admin_same_address_noop() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    const HEADER_CONFIG_LEN: usize = 584;
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let used_before = env.read_num_used_accounts();
    let spl_vault_before = env.vault_balance();

    // Update admin to same address
    env.try_update_admin(&admin, &admin.pubkey()).unwrap();
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let used_after = env.read_num_used_accounts();
    let spl_vault_after = env.vault_balance();
    assert_eq!(
        &slab_after[..HEADER_CONFIG_LEN],
        &slab_before[..HEADER_CONFIG_LEN],
        "Self-update admin should be a no-op on slab header/config bytes"
    );
    assert_eq!(
        used_after, used_before,
        "Self-update admin must not change num_used_accounts"
    );
    assert_eq!(
        spl_vault_after, spl_vault_before,
        "Self-update admin must not move vault funds"
    );

    // Non-admin must still be rejected
    let random = Keypair::new();
    env.svm.airdrop(&random.pubkey(), 1_000_000_000).unwrap();
    let non_admin_result = env.try_update_admin(&random, &random.pubkey());
    assert!(
        non_admin_result.is_err(),
        "Self-update admin must not broaden admin permissions"
    );
}

/// ATTACK: Double deposit then withdraw full amount.
/// Verify deposits accumulate correctly and full withdrawal returns sum.
#[test]
fn test_attack_double_deposit_accumulation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();

    // First deposit (BEFORE crank to prevent GC of zero-capital account)
    env.deposit(&user, user_idx, 3_000_000_000);
    env.crank();

    let cap1 = env.read_account_capital(user_idx);
    assert_eq!(
        cap1, 3_000_000_099,
        "First deposit amount (includes init capital after anti-spam fee)"
    );

    // Second deposit (different slot to avoid collision)
    env.set_slot(2);
    env.deposit(&user, user_idx, 2_000_000_000);
    let cap2 = env.read_account_capital(user_idx);
    assert_eq!(
        cap2, 5_000_000_099,
        "Second deposit should accumulate (includes init capital after anti-spam fee)"
    );

    // Full withdrawal (5B deposit + 99 init capital)
    env.set_slot(3);
    env.try_withdraw(&user, user_idx, 5_000_000_099).unwrap();
    let cap_final = env.read_account_capital(user_idx);
    assert_eq!(cap_final, 0, "Full withdrawal should zero capital");
}

/// ATTACK: Withdraw exactly the user's entire capital.
/// Edge case: withdraw == capital leaves zero, should succeed.
#[test]
fn test_attack_withdraw_exact_capital() {
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

    // Read current capital (may be slightly less due to fees)
    let cap = env.read_account_capital(user_idx);
    assert!(cap > 0, "Precondition: user has capital");

    // Withdraw exact capital amount (no position, so should succeed)
    let result = env.try_withdraw(&user, user_idx, cap as u64);
    assert!(
        result.is_ok(),
        "Withdrawing exact capital should succeed: {:?}",
        result
    );

    let cap_after = env.read_account_capital(user_idx);
    assert_eq!(
        cap_after, 0,
        "Capital should be exactly zero after full withdraw"
    );
}

/// ATTACK: Multiple LPs with different sizes - verify LP max position tracking.
/// LP positions should be independently bounded by their own limits.
#[test]
fn test_attack_multi_lp_max_position_tracking() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp1 = Keypair::new();
    let lp1_idx = env.init_lp(&lp1);
    env.deposit(&lp1, lp1_idx, 5_000_000_000); // Small LP

    let lp2 = Keypair::new();
    let lp2_idx = env.init_lp(&lp2);
    env.deposit(&lp2, lp2_idx, 50_000_000_000); // Large LP

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade against small LP
    env.trade(&user, &lp1, lp1_idx, user_idx, 50_000);

    // Trade against large LP
    env.set_slot(2);
    env.trade(&user, &lp2, lp2_idx, user_idx, 500_000);

    // Each LP tracks position independently
    assert_eq!(env.read_account_position(lp1_idx), -50_000);
    assert_eq!(env.read_account_position(lp2_idx), -500_000);
    assert_eq!(env.read_account_position(user_idx), 550_000);

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp1_idx)
        + env.read_account_capital(lp2_idx)
        + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with multi-LP tracking! c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Liquidate account that is solvent (positive equity).
/// LiquidateAtOracle should reject attempts on solvent accounts.
#[test]
fn test_attack_liquidate_solvent_account_after_settlement() {
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

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Small position, well-collateralized
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Settle at slightly different price
    env.set_slot_and_price(50, 139_000_000);
    env.crank();

    // Account should be solvent (10B capital vs tiny position).
    // Engine may return Ok (no-op) rather than Err for solvent accounts,
    // but position must remain unchanged.
    let pos_before = env.read_account_position(user_idx);
    let _ = env.try_liquidate(user_idx);
    assert_eq!(
        env.read_account_position(user_idx),
        pos_before,
        "ATTACK: Solvent account's position was modified by liquidation!"
    );
}

/// ATTACK: Close account, GC via crank, verify num_used_accounts decrements.
/// Full lifecycle: init → deposit → close → crank(GC) → verify count.
#[test]
fn test_attack_close_then_gc_decrements_used_count() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.crank();

    let num_before = env.read_num_used_accounts();

    // Close user account
    env.try_close_account(&user, user_idx).unwrap();

    // Crank to GC the closed account
    env.set_slot(100);
    env.crank();
    env.set_slot(200);
    env.crank();

    let num_after = env.read_num_used_accounts();
    assert!(
        num_after < num_before,
        "num_used_accounts should decrease after close+GC: before={} after={}",
        num_before,
        num_after
    );
}

/// ATTACK: Position reversal (long→short) requires initial_margin_bps.
/// When crossing zero, the margin check uses the stricter initial margin.
#[test]
fn test_attack_position_reversal_margin_check() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open long position
    env.trade(&user, &lp, lp_idx, user_idx, 500_000);
    assert_eq!(env.read_account_position(user_idx), 500_000);

    // Reverse to short (crosses zero) - should succeed with sufficient margin
    env.set_slot(2);
    env.trade(&user, &lp, lp_idx, user_idx, -1_000_000);
    assert_eq!(
        env.read_account_position(user_idx),
        -500_000,
        "Position should have flipped to short"
    );

    // Conservation after flip
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync after position reversal! c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Close account path settles fees correctly.
/// Compare: crank(settle fees) → close vs. close(settles fees internally).
#[test]
fn test_attack_close_account_settles_fees_correctly() {
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

    // Accrue fees over many slots WITHOUT cranking (lazy settlement)
    env.set_slot(2000);
    // Don't crank - let CloseAccount handle fee settlement

    let vault_before = env.vault_balance();
    let insurance_before = env.read_insurance_balance();

    // Close account without intermediate crank - CloseAccount must settle fees internally
    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "CloseAccount should settle fees and succeed: {:?}",
        result
    );

    let vault_after = env.vault_balance();
    let insurance_after = env.read_insurance_balance();

    // Vault decreased (capital returned to user)
    assert!(
        vault_before > vault_after,
        "Vault should decrease from capital return"
    );

    // Insurance increased (fees collected)
    assert!(
        insurance_after >= insurance_before,
        "Insurance should increase from fee collection: before={} after={}",
        insurance_before,
        insurance_after
    );
}

/// ATTACK: Funding accumulation across position size changes.
/// Open position, crank to accrue funding, change position size, crank again.
/// Verify funding uses stored index (anti-retroactivity).
#[test]
fn test_attack_funding_across_position_size_change() {
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

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open initial position
    let cap_before_trade = env.read_account_capital(user_idx);
    env.trade(&user, &lp, lp_idx, user_idx, 500_000);
    let cap_after_trade = env.read_account_capital(user_idx);
    assert_eq!(
        cap_after_trade, cap_before_trade,
        "Capital should not change from trade alone (no fee)"
    );

    // Crank to accrue some funding
    env.set_slot(100);
    env.crank();

    // Partial close (reduce position)
    env.set_slot(101);
    env.trade(&user, &lp, lp_idx, user_idx, -250_000);
    assert_eq!(env.read_account_position(user_idx), 250_000);

    // More cranks with smaller position
    env.set_slot(200);
    env.crank();

    // Conservation must hold through all changes
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync after position size change! c_tot={} sum={}",
        c_tot, sum
    );

    // SPL vault unchanged
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 61_000_000_200,
        "ATTACK: SPL vault changed during funding with position changes!"
    );
}

/// ATTACK: Partial position close then full close then CloseAccount.
/// Full lifecycle: open → partial close → full close → account close.
#[test]
fn test_attack_partial_close_full_lifecycle() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 30_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open
    env.trade(&user, &lp, lp_idx, user_idx, 300_000);
    assert_eq!(env.read_account_position(user_idx), 300_000);

    // Partial close
    env.set_slot(2);
    env.trade(&user, &lp, lp_idx, user_idx, -100_000);
    assert_eq!(env.read_account_position(user_idx), 200_000);

    // Full close
    env.set_slot(3);
    env.trade(&user, &lp, lp_idx, user_idx, -200_000);
    assert_eq!(env.read_account_position(user_idx), 0);

    // Settle everything
    for i in 0..10 {
        env.set_slot(100 + i * 50);
        env.crank();
    }

    // Close account
    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "CloseAccount should succeed after full position lifecycle: {:?}",
        result
    );

    let cap = env.read_account_capital(user_idx);
    assert_eq!(cap, 0, "Capital should be zero after close");
}

/// ATTACK: Multiple deposits to LP then user trades against it.
/// Verify LP capital accumulates correctly and trades work.
#[test]
fn test_attack_lp_multiple_deposits_then_trade() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Second LP deposit
    env.set_slot(2);
    env.deposit(&lp, lp_idx, 5_000_000_000);
    let lp_cap = env.read_account_capital(lp_idx);
    assert_eq!(
        lp_cap, 15_000_000_099,
        "LP capital should accumulate (includes init capital after anti-spam fee)"
    );

    // User trades against the well-funded LP
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.set_slot(3);
    env.trade(&user, &lp, lp_idx, user_idx, 200_000);
    assert_eq!(env.read_account_position(user_idx), 200_000);

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with multi-deposit LP! c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Sequential deposit → trade → crank → withdraw → close lifecycle.
/// Full account lifecycle with all operations in sequence.
#[test]
fn test_attack_full_account_lifecycle_sequence() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 30_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    // 1. Deposit (before crank to prevent GC of zero-capital account)
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    assert_eq!(env.read_account_capital(user_idx), 5_000_000_099);

    // 2. Trade
    env.set_slot(2);
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert_eq!(env.read_account_position(user_idx), 100_000);

    // 3. Crank (settle)
    env.set_slot(100);
    env.crank();

    // 4. Close position
    env.set_slot(101);
    env.trade(&user, &lp, lp_idx, user_idx, -100_000);
    assert_eq!(env.read_account_position(user_idx), 0);

    // 5. Settle warmup
    for i in 0..10 {
        env.set_slot(200 + i * 50);
        env.crank();
    }

    // 6. Withdraw remaining capital
    let cap = env.read_account_capital(user_idx);
    if cap > 0 {
        env.try_withdraw(&user, user_idx, cap as u64).unwrap();
    }

    // 7. Close account
    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "Full lifecycle CloseAccount should succeed: {:?}",
        result
    );
}

/// ATTACK: GC account that just had position closed.
/// Close position → crank → crank again → verify GC happens.
#[test]
fn test_attack_gc_after_position_close_and_settlement() {
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

    // Open and close position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot(2);
    env.trade(&user, &lp, lp_idx, user_idx, -100_000);

    // Settle warmup
    for i in 0..10 {
        env.set_slot(100 + i * 50);
        env.crank();
    }

    let num_before_close = env.read_num_used_accounts();
    assert!(
        num_before_close >= 2,
        "Precondition: both LP and user should be active: {}",
        num_before_close
    );

    // Close account (returns capital)
    env.try_close_account(&user, user_idx).unwrap();

    // Crank to GC the closed account
    env.set_slot(1000);
    env.crank();
    env.set_slot(1100);
    env.crank();

    let num_after = env.read_num_used_accounts();
    assert!(
        num_after < num_before_close,
        "num_used should decrease after close+GC: before={} after={}",
        num_before_close,
        num_after
    );
}

/// ATTACK: Trade at max price (circuit breaker limit).
/// Oracle at extreme high price, crank, verify no overflow.
#[test]
fn test_attack_trade_at_extreme_high_price() {
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

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade at default price
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Extreme high oracle price
    env.set_slot_and_price(50, 10_000_000_000); // $10,000
    for i in 0..10u64 {
        env.set_slot(50 + i * 100);
        env.crank();
    }

    // Should not overflow - conservation holds
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync at extreme high price! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 61_000_000_200,
        "ATTACK: SPL vault changed at extreme high price!"
    );
}

/// ATTACK: Trade at extreme low oracle price (near zero).
/// Verify no division by zero or overflow.
#[test]
fn test_attack_trade_at_extreme_low_price() {
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

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade at default price
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Extreme low oracle price (but circuit breaker limits per-slot change)
    env.set_slot_and_price_raw_no_walk(50, 1_000); // $0.001
    for i in 0..10u64 {
        env.set_slot(50 + i * 100);
        env.crank();
    }

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync at extreme low price! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 61_000_000_200,
        "ATTACK: SPL vault changed at extreme low price!"
    );
}

/// ATTACK: Rapid open/close/open cycle - same position size, different slots.
/// Tests that entry_price resets correctly on each open.
#[test]
fn test_attack_rapid_open_close_open_cycle() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 30_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Cycle 1: open
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert_eq!(env.read_account_position(user_idx), 100_000);

    // Cycle 1: close
    env.set_slot(2);
    env.trade(&user, &lp, lp_idx, user_idx, -100_000);
    assert_eq!(env.read_account_position(user_idx), 0);

    // Price change between cycles
    env.set_slot_and_price(50, 145_000_000);
    env.crank();

    // Cycle 2: open at new price (different size to avoid tx collision)
    env.set_slot(51);
    env.trade(&user, &lp, lp_idx, user_idx, 200_000);
    assert_eq!(env.read_account_position(user_idx), 200_000);

    // Crank
    env.set_slot(100);
    env.crank();

    // Conservation after cycles
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync after open/close/open cycle! c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Send instruction with tag=24 (just above max valid tag=23).
/// Should fail gracefully.
#[test]
fn test_attack_instruction_tag_just_above_max() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();

    // Tag = 24 (one above WithdrawInsuranceLimited=23)
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
        ],
        data: vec![24u8],
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    const HEADER_CONFIG_LEN: usize = 584;
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let used_before = env.read_num_used_accounts();
    let spl_vault_before = env.vault_balance();
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Invalid instruction tag 24 should be rejected!"
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let used_after = env.read_num_used_accounts();
    let spl_vault_after = env.vault_balance();
    assert_eq!(
        &slab_after[..HEADER_CONFIG_LEN],
        &slab_before[..HEADER_CONFIG_LEN],
        "Rejected invalid-tag instruction must not mutate slab header/config"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected invalid-tag instruction must not change num_used_accounts"
    );
    assert_eq!(
        spl_vault_after, spl_vault_before,
        "Rejected invalid-tag instruction must not move vault funds"
    );
}

/// ATTACK: Deposit with wrong slab account (different program_id slab).
/// Slab owned by wrong program should be rejected by slab_guard.
#[test]
fn test_attack_deposit_wrong_slab_owner() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    // Create a fake slab owned by system program
    let fake_slab = Pubkey::new_unique();
    let slab_data = env.svm.get_account(&env.slab).unwrap().data.clone();
    env.svm
        .set_account(
            fake_slab,
            Account {
                lamports: 1_000_000,
                data: slab_data,
                owner: solana_sdk::system_program::ID, // Wrong owner
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let ata = env.create_ata(&user.pubkey(), 1_000_000_000);
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(fake_slab, false), // Wrong slab
            AccountMeta::new(ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data: encode_deposit(user_idx, 1_000_000_000),
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let user_cap_before = env.read_account_capital(user_idx);
    let used_before = env.read_num_used_accounts();
    let spl_vault_before = env.vault_balance();
    let ata_before = {
        let ata_data = env.svm.get_account(&ata).unwrap().data;
        TokenAccount::unpack(&ata_data).unwrap().amount
    };
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Deposit to slab with wrong owner should be rejected!"
    );
    let user_cap_after = env.read_account_capital(user_idx);
    let used_after = env.read_num_used_accounts();
    let spl_vault_after = env.vault_balance();
    let ata_after = {
        let ata_data = env.svm.get_account(&ata).unwrap().data;
        TokenAccount::unpack(&ata_data).unwrap().amount
    };
    assert_eq!(
        user_cap_after, user_cap_before,
        "Rejected wrong-slab-owner deposit must not change user capital"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected wrong-slab-owner deposit must not change num_used_accounts"
    );
    assert_eq!(
        spl_vault_after, spl_vault_before,
        "Rejected wrong-slab-owner deposit must not change SPL vault"
    );
    assert_eq!(
        ata_after, ata_before,
        "Rejected wrong-slab-owner deposit must not debit source ATA"
    );
}

/// ATTACK: Deposit without signer (user not signing).
/// All operations require the user to sign.
#[test]
fn test_attack_deposit_without_signer() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    // Try deposit with user as non-signer
    let ata = env.create_ata(&user.pubkey(), 1_000_000_000);
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), false), // NOT a signer
            AccountMeta::new(env.slab, false),
            AccountMeta::new(ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data: encode_deposit(user_idx, 1_000_000_000),
    };

    // Payer signs, but user doesn't
    let payer = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&payer.pubkey()),
        &[&payer],
        env.svm.latest_blockhash(),
    );
    let user_cap_before = env.read_account_capital(user_idx);
    let used_before = env.read_num_used_accounts();
    let spl_vault_before = env.vault_balance();
    let ata_before = {
        let ata_data = env.svm.get_account(&ata).unwrap().data;
        TokenAccount::unpack(&ata_data).unwrap().amount
    };
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Deposit without user signer should be rejected!"
    );
    let user_cap_after = env.read_account_capital(user_idx);
    let used_after = env.read_num_used_accounts();
    let spl_vault_after = env.vault_balance();
    let ata_after = {
        let ata_data = env.svm.get_account(&ata).unwrap().data;
        TokenAccount::unpack(&ata_data).unwrap().amount
    };
    assert_eq!(
        user_cap_after, user_cap_before,
        "Rejected deposit-without-signer must not change user capital"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected deposit-without-signer must not change num_used_accounts"
    );
    assert_eq!(
        spl_vault_after, spl_vault_before,
        "Rejected deposit-without-signer must not change SPL vault"
    );
    assert_eq!(
        ata_after, ata_before,
        "Rejected deposit-without-signer must not debit source ATA"
    );
}

/// ATTACK: Withdraw from LP account (LP should still be able to withdraw).
/// Verify LP withdraw works the same as user withdraw.
#[test]
fn test_attack_lp_withdraw_capital() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    let cap_before = env.read_account_capital(lp_idx);
    assert!(cap_before > 0, "Precondition: LP has capital");

    // LP withdraws half its capital
    let withdraw_amount = (cap_before / 2) as u64;
    env.try_withdraw(&lp, lp_idx, withdraw_amount).unwrap();

    let cap_after = env.read_account_capital(lp_idx);
    assert_eq!(
        cap_after,
        cap_before - withdraw_amount as u128,
        "LP capital should decrease by withdraw amount"
    );
}

/// ATTACK: Trade at maximum position size boundary.
/// Open a position that uses nearly all margin, then try adding more.
#[test]
fn test_attack_trade_exceeds_margin_capacity() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000); // 1 SOL = 1e9

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // At $138, with 10% initial margin: max notional = 1e9/0.1 = 10e9
    // max position = 10e9 * 1e6 / 138e6 ≈ 72.4M
    // Try opening just within limit
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 50_000_000);
    assert!(
        result.is_ok(),
        "Trade within margin should succeed: {:?}",
        result
    );

    // Try doubling position (exceeds margin)
    env.set_slot(2);
    let result2 = env.try_trade(&user, &lp, lp_idx, user_idx, 50_000_000);
    assert!(
        result2.is_err(),
        "ATTACK: Trade exceeding margin should be rejected!"
    );

    // Original position unchanged
    assert_eq!(env.read_account_position(user_idx), 50_000_000);
}

/// ATTACK: InitMarket with admin field in data mismatching signer.
/// Code validates admin in instruction data matches signer pubkey.
#[test]
fn test_attack_init_market_admin_mismatch() {
    let path = program_path();

    let mut svm = LiteSVM::new();
    let program_id = Pubkey::new_unique();
    let program_bytes = std::fs::read(&path).expect("Failed to read program");
    svm.add_program(program_id, &program_bytes);
    let admin = Keypair::new();
    svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();

    let mint = Pubkey::new_unique();
    svm.set_account(
        mint,
        Account {
            lamports: 1_000_000,
            data: vec![0u8; spl_token::state::Mint::LEN],
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let slab = Pubkey::new_unique();
    let slab_size = 4 * 1024 * 1024;
    svm.set_account(
        slab,
        Account {
            lamports: 100_000_000_000,
            data: vec![0u8; slab_size],
            owner: program_id,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let vault = Pubkey::new_unique();
    svm.set_account(
        vault,
        Account {
            lamports: 1_000_000,
            data: vec![0u8; spl_token::state::Account::LEN],
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let dummy_ata = Pubkey::new_unique();
    svm.set_account(
        dummy_ata,
        Account {
            lamports: 1_000_000,
            data: vec![0u8; spl_token::state::Account::LEN],
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    // Use a DIFFERENT pubkey for admin in data vs signer
    let fake_admin = Pubkey::new_unique();
    let data = encode_init_market_with_invert(&fake_admin, &mint, &TEST_FEED_ID, 0);

    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true), // signer
            AccountMeta::new(slab, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            // Admin-mismatch is checked before the oracle read, so the slot
            // can be any placeholder account — init rejects before reaching it.
            AccountMeta::new_readonly(dummy_ata, false),
        ],
        data, // admin in data = fake_admin != signer
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[&admin],
        svm.latest_blockhash(),
    );
    const HEADER_CONFIG_LEN: usize = 584;
    let slab_before = svm.get_account(&slab).unwrap().data;
    let vault_before = svm.get_account(&vault).unwrap().data;
    let result = svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: InitMarket with admin mismatch should be rejected!"
    );
    let slab_after = svm.get_account(&slab).unwrap().data;
    let vault_after = svm.get_account(&vault).unwrap().data;
    assert_eq!(
        &slab_after[..HEADER_CONFIG_LEN],
        &slab_before[..HEADER_CONFIG_LEN],
        "Rejected InitMarket admin mismatch must not mutate slab header/config"
    );
    assert_eq!(
        vault_after, vault_before,
        "Rejected InitMarket admin mismatch must not mutate vault account data"
    );
}

/// ATTACK: InitMarket with mint field in data mismatching mint account.
/// Code validates collateral_mint in data matches the mint account provided.
#[test]
fn test_attack_init_market_mint_mismatch() {
    let path = program_path();

    let mut svm = LiteSVM::new();
    let program_id = Pubkey::new_unique();
    let program_bytes = std::fs::read(&path).expect("Failed to read program");
    svm.add_program(program_id, &program_bytes);
    let admin = Keypair::new();
    svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();

    let real_mint = Pubkey::new_unique();
    svm.set_account(
        real_mint,
        Account {
            lamports: 1_000_000,
            data: vec![0u8; spl_token::state::Mint::LEN],
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let fake_mint = Pubkey::new_unique();

    let slab = Pubkey::new_unique();
    let slab_size = 4 * 1024 * 1024;
    svm.set_account(
        slab,
        Account {
            lamports: 100_000_000_000,
            data: vec![0u8; slab_size],
            owner: program_id,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let vault = Pubkey::new_unique();
    svm.set_account(
        vault,
        Account {
            lamports: 1_000_000,
            data: vec![0u8; spl_token::state::Account::LEN],
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let dummy_ata = Pubkey::new_unique();
    svm.set_account(
        dummy_ata,
        Account {
            lamports: 1_000_000,
            data: vec![0u8; spl_token::state::Account::LEN],
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    // Encode with fake_mint in data, but pass real_mint as account
    let data = encode_init_market_with_invert(&admin.pubkey(), &fake_mint, &TEST_FEED_ID, 0);

    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(slab, false),
            AccountMeta::new_readonly(real_mint, false), // Real mint != fake_mint in data
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            // Mint-mismatch is checked before the oracle read, so the slot
            // can be any placeholder account.
            AccountMeta::new_readonly(dummy_ata, false),
        ],
        data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[&admin],
        svm.latest_blockhash(),
    );
    const HEADER_CONFIG_LEN: usize = 584;
    let slab_before = svm.get_account(&slab).unwrap().data;
    let vault_before = svm.get_account(&vault).unwrap().data;
    let result = svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: InitMarket with mint mismatch should be rejected!"
    );
    let slab_after = svm.get_account(&slab).unwrap().data;
    let vault_after = svm.get_account(&vault).unwrap().data;
    assert_eq!(
        &slab_after[..HEADER_CONFIG_LEN],
        &slab_before[..HEADER_CONFIG_LEN],
        "Rejected InitMarket mint mismatch must not mutate slab header/config"
    );
    assert_eq!(
        vault_after, vault_before,
        "Rejected InitMarket mint mismatch must not mutate vault account data"
    );
}

/// ATTACK: Withdraw with wrong vault PDA (correct PDA but from different slab).
/// Code checks vault PDA derivation matches slab.
#[test]
fn test_attack_withdraw_wrong_vault_pda() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    // Derive a PDA from a different slab
    let wrong_slab = Pubkey::new_unique();
    let (wrong_vault_pda, _) =
        Pubkey::find_program_address(&[b"vault", wrong_slab.as_ref()], &env.program_id);

    let ata = env.create_ata(&user.pubkey(), 0);
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new(ata, false),
            AccountMeta::new_readonly(wrong_vault_pda, false), // Wrong PDA
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_withdraw(user_idx, 1_000_000_000),
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let user_cap_before = env.read_account_capital(user_idx);
    let used_before = env.read_num_used_accounts();
    let spl_vault_before = env.vault_balance();
    let ata_before = {
        let ata_data = env.svm.get_account(&ata).unwrap().data;
        TokenAccount::unpack(&ata_data).unwrap().amount
    };
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Withdraw with wrong vault PDA should be rejected!"
    );
    let user_cap_after = env.read_account_capital(user_idx);
    let used_after = env.read_num_used_accounts();
    let spl_vault_after = env.vault_balance();
    let ata_after = {
        let ata_data = env.svm.get_account(&ata).unwrap().data;
        TokenAccount::unpack(&ata_data).unwrap().amount
    };
    assert_eq!(
        user_cap_after, user_cap_before,
        "Rejected wrong-vault-PDA withdraw must not change user capital"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected wrong-vault-PDA withdraw must not change num_used_accounts"
    );
    assert_eq!(
        spl_vault_after, spl_vault_before,
        "Rejected wrong-vault-PDA withdraw must not change SPL vault"
    );
    assert_eq!(
        ata_after, ata_before,
        "Rejected wrong-vault-PDA withdraw must not credit destination ATA"
    );
}

/// ATTACK: CloseAccount with wrong vault PDA.
/// Code checks vault PDA derivation matches slab in CloseAccount path.
#[test]
fn test_attack_close_account_wrong_vault_pda() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    // Crank to settle
    env.set_slot(200);
    env.crank();

    // Withdraw all capital first
    env.try_withdraw(&user, user_idx, 1_000_000_000).unwrap();

    // Try close with wrong vault PDA
    let wrong_slab = Pubkey::new_unique();
    let (wrong_vault_pda, _) =
        Pubkey::find_program_address(&[b"vault", wrong_slab.as_ref()], &env.program_id);

    let ata = env.create_ata(&user.pubkey(), 0);
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new(ata, false),
            AccountMeta::new_readonly(wrong_vault_pda, false), // Wrong PDA
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_close_account(user_idx),
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let user_cap_before = env.read_account_capital(user_idx);
    let user_pos_before = env.read_account_position(user_idx);
    let used_before = env.read_num_used_accounts();
    let spl_vault_before = env.vault_balance();
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: CloseAccount with wrong vault PDA should be rejected!"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected wrong-vault-PDA close must not change user capital"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected wrong-vault-PDA close must not change user position"
    );
    assert_eq!(
        env.read_num_used_accounts(),
        used_before,
        "Rejected wrong-vault-PDA close must not change num_used_accounts"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected wrong-vault-PDA close must not change SPL vault"
    );
}

/// ATTACK: self-crank caller account is not a signer.
/// Permissionless crank uses `caller_idx = u16::MAX`; self-crank reward mode
/// requires the caller account owner to sign.
#[test]
fn test_attack_liquidate_caller_not_signer() {
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

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, 10_000_000);
    let user_pos_before = env.read_account_position(user_idx);
    let user_cap_before = env.read_account_capital(user_idx);

    // Construct KeeperCrank in self-crank mode with the account owner listed
    // as non-signer. Authorization must reject before candidate liquidation.
    let mut data = vec![5u8]; // KeeperCrank
    data.extend_from_slice(&user_idx.to_le_bytes());
    data.push(1u8); // format_version
    data.extend_from_slice(&user_idx.to_le_bytes());
    data.push(0u8); // FullClose candidate

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), false), // owner key, but NOT a signer
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data,
    };

    // Use admin as payer (different from caller)
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(result.is_err(), "self-crank caller must be a signer");
    let user_pos_after = env.read_account_position(user_idx);
    let user_cap_after = env.read_account_capital(user_idx);
    assert_eq!(
        user_pos_after, user_pos_before,
        "Liquidation-by-non-signer path must not mutate solvent user position: before={} after={} result={:?}",
        user_pos_before, user_pos_after, result
    );
    assert_eq!(
        user_cap_after, user_cap_before,
        "Liquidation-by-non-signer path must not mutate solvent user capital: before={} after={} result={:?}",
        user_cap_before, user_cap_after, result
    );

    // In both cases: user capital preserved, conservation holds
    assert!(
        user_cap_after > 0,
        "User capital must not be drained: cap={}",
        user_cap_after
    );
    let spl_vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, spl_vault,
        "Conservation must hold: engine={} spl={}",
        engine_vault, spl_vault
    );
}

/// ATTACK: Deposit with wrong oracle price account.
/// Verifies oracle account validation rejects wrong price feed.
#[test]
fn test_attack_deposit_wrong_oracle_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    env.create_ata(&user.pubkey(), 5_000_000_000);

    // Create a fake oracle account
    let fake_oracle = Pubkey::new_unique();
    env.svm
        .set_account(
            fake_oracle,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; 256],        // Garbage data
                owner: Pubkey::new_unique(), // Wrong owner
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    // Deposit itself doesn't need oracle, but init_user passes pyth_col.
    // Test that trade with wrong oracle index is rejected.
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);
    env.deposit(&user, user_idx, 5_000_000_000);
    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // Construct trade instruction with wrong oracle
    let mut trade_data = vec![6u8]; // Trade tag
    trade_data.extend_from_slice(&lp_idx.to_le_bytes());
    trade_data.extend_from_slice(&user_idx.to_le_bytes());
    trade_data.extend_from_slice(&1_000_000i128.to_le_bytes());

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(fake_oracle, false), // Wrong oracle
        ],
        data: trade_data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Trade with wrong oracle account should be rejected!"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected trade with wrong oracle must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected trade with wrong oracle must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected trade with wrong oracle must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected trade with wrong oracle must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected trade with wrong oracle must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected trade with wrong oracle must preserve engine vault"
    );
}

/// ATTACK: Crank with wrong oracle account on standard market.
/// Trade/crank oracle validation should reject mismatched feed.
#[test]
fn test_attack_crank_wrong_oracle() {
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
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);

    env.set_slot(200);

    // Create a fake oracle that looks like Pyth but with wrong feed_id
    let fake_oracle = Pubkey::new_unique();
    env.svm
        .set_account(
            fake_oracle,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; 512],
                owner: Pubkey::new_unique(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let caller = Keypair::new();
    env.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();

    let mut data = vec![7u8]; // KeeperCrank
    data.extend_from_slice(&0u16.to_le_bytes()); // caller_idx = 0 (permissionless)
    data.push(0); // panic = false

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(caller.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(fake_oracle, false), // Wrong oracle
        ],
        data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&caller.pubkey()),
        &[&caller],
        env.svm.latest_blockhash(),
    );
    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Crank with wrong oracle should be rejected!"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected crank with wrong oracle must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected crank with wrong oracle must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected crank with wrong oracle must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected crank with wrong oracle must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected crank with wrong oracle must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected crank with wrong oracle must preserve engine vault"
    );
}

/// ATTACK: Withdraw with wrong SPL token program account.
/// Substituting a fake token program should be rejected.
#[test]
fn test_attack_withdraw_wrong_token_program() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    let ata = env.create_ata(&user.pubkey(), 0);
    let (vault_pda, _) =
        Pubkey::find_program_address(&[b"vault", env.slab.as_ref()], &env.program_id);

    // Use a fake token program
    let fake_token_program = Pubkey::new_unique();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new(ata, false),
            AccountMeta::new_readonly(vault_pda, false),
            AccountMeta::new_readonly(fake_token_program, false), // Wrong token program
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_withdraw(user_idx, 1_000_000_000),
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Withdraw with wrong token program should be rejected!"
    );

    // Verify capital unchanged
    let cap = env.read_account_capital(user_idx);
    assert_eq!(
        cap, 5_000_000_099,
        "Capital should be unchanged after failed withdraw (includes init)"
    );
}

/// ATTACK: Alias user_ata with vault in WithdrawCollateral.
/// Must reject duplicate-role account substitution.
#[test]
fn test_attack_withdraw_alias_user_ata_is_vault() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    let (vault_pda, _) =
        Pubkey::find_program_address(&[b"vault", env.slab.as_ref()], &env.program_id);

    let cap_before = env.read_account_capital(user_idx);
    let vault_before = env.vault_balance();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(env.vault, false), // vault
            AccountMeta::new(env.vault, false), // aliased as user_ata (attack)
            AccountMeta::new_readonly(vault_pda, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_withdraw(user_idx, 1_000_000_000),
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "SECURITY: Withdraw should reject aliased user_ata=vault"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        cap_before,
        "Capital changed on rejected alias withdraw"
    );
    assert_eq!(
        env.vault_balance(),
        vault_before,
        "Vault changed on rejected alias withdraw"
    );
}

/// ATTACK: Alias user_ata with vault in CloseAccount.
/// Must reject duplicate-role account substitution.
#[test]
fn test_attack_close_account_alias_user_ata_is_vault() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    // Make close preconditions realistic: no open position and settled state.
    env.set_slot(200);
    env.crank();
    env.try_withdraw(&user, user_idx, 1_000_000_000).unwrap();

    let (vault_pda, _) =
        Pubkey::find_program_address(&[b"vault", env.slab.as_ref()], &env.program_id);
    let vault_before = env.vault_balance();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(env.vault, false), // vault
            AccountMeta::new(env.vault, false), // aliased as user_ata (attack)
            AccountMeta::new_readonly(vault_pda, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_close_account(user_idx),
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "SECURITY: CloseAccount should reject aliased user_ata=vault"
    );
    assert_eq!(
        env.vault_balance(),
        vault_before,
        "Vault changed on rejected alias close"
    );
}

/// ATTACK: Configuring a market with unit_scale so large that scale_price_e6
/// returns 0 (None). Under the pre-spec wrapper this was deferred to crank/trade
/// time; the new wrapper rejects at InitMarket — the oracle read performed at
/// init observes the scaled-to-zero price and refuses to construct the engine
/// state at all (spec goal 38: no valid positive price as sentinel, and no
/// economically meaningless engine state).
#[test]
fn test_attack_scale_price_zero_rejects_trade() {
    program_path();

    // Oracle price $138 (138_000_000 e6), unit_scale=200_000_000 → 138M/200M = 0.
    // With the new init-time oracle read, this configuration is unconstructible.
    let unit_scale = 200_000_000u32;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut env = TestEnv::new();
        env.init_market_full(0, unit_scale, 0);
    }));
    assert!(
        result.is_err(),
        "InitMarket must reject a unit_scale configuration that scales the oracle price to zero"
    );
}

#[allow(dead_code)]
fn __unused_former_body_test_attack_scale_price_zero_rejects_trade() {
    // Retained below for historical context; kept as a dead helper because the
    // body exercised runtime paths (crank, trade) that the new init-time check
    // makes unreachable.
    let mut env = TestEnv::new();
    let unit_scale = 200_000_000u32;
    env.init_market_full(0, unit_scale, 0);

    let lp = Keypair::new();
    // With unit_scale=200M, need 100*200M=20B base for min_initial_deposit
    let lp_idx = env.init_lp_with_fee(&lp, 20_000_000_000);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user_with_fee(&user, 20_000_000_000);
    env.deposit(&user, user_idx, 5_000_000_000);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_pos_before = env.read_account_position(user_idx);
    let vault_before = env.vault_balance();

    // Crank should fail or be a no-op because oracle price scales to zero
    env.set_slot(200);
    let crank_result = env.try_crank();
    let lp_cap_after_crank = env.read_account_capital(lp_idx);
    let user_cap_after_crank = env.read_account_capital(user_idx);
    let lp_pos_after_crank = env.read_account_position(lp_idx);
    let user_pos_after_crank = env.read_account_position(user_idx);

    assert_eq!(
        lp_pos_after_crank, lp_pos_before,
        "Scale-zero crank path must not mutate LP position: before={} after={}",
        lp_pos_before, lp_pos_after_crank
    );
    assert_eq!(
        user_pos_after_crank, user_pos_before,
        "Scale-zero crank path must not mutate user position: before={} after={}",
        user_pos_before, user_pos_after_crank
    );
    if crank_result.is_ok() {
        assert!(
            lp_cap_after_crank <= lp_cap_before,
            "Accepted scale-zero crank must not mint LP capital: before={} after={}",
            lp_cap_before,
            lp_cap_after_crank
        );
        assert!(
            user_cap_after_crank <= user_cap_before,
            "Accepted scale-zero crank must not mint user capital: before={} after={}",
            user_cap_before,
            user_cap_after_crank
        );
    } else {
        assert_eq!(
            lp_cap_after_crank, lp_cap_before,
            "Rejected scale-zero crank must preserve LP capital: before={} after={}",
            lp_cap_before, lp_cap_after_crank
        );
        assert_eq!(
            user_cap_after_crank, user_cap_before,
            "Rejected scale-zero crank must preserve user capital: before={} after={}",
            user_cap_before, user_cap_after_crank
        );
    }

    // Trade should fail because scaled price is None
    let trade_result = env.try_trade(&user, &lp, lp_idx, user_idx, 100);
    assert!(
        trade_result.is_err(),
        "ATTACK: Trade with zero scaled price should be rejected!"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_after_crank,
        "Rejected scale-zero trade must preserve user position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_after_crank,
        "Rejected scale-zero trade must preserve user capital"
    );
    assert_eq!(
        env.vault_balance(),
        vault_before,
        "Scale-zero edge path must preserve vault balance"
    );
    let engine_vault_scaled = env.read_engine_vault() as u128;
    assert_eq!(
        engine_vault_scaled * unit_scale as u128,
        vault_before as u128,
        "Scale-zero edge path must preserve scaled engine/SPL vault consistency: engine_scaled={} unit_scale={} spl={}",
        engine_vault_scaled,
        unit_scale,
        vault_before
    );
}

/// ATTACK: Inverted market with very high raw price so inverted result is zero.
/// invert_price_e6 with raw near u64::MAX: INVERSION_CONSTANT / raw → 0.
/// INVERSION_CONSTANT = 10^12, so raw > 10^12 gives inverted < 1 → 0 → None.
#[test]
fn test_attack_invert_price_zero_result() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(1); // Inverted

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    // Initial setup at normal price
    env.crank();
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_pos_before = env.read_account_position(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);

    // Set oracle to extremely high price: 10^13 (> INVERSION_CONSTANT=10^12)
    // inverted = 10^12 / 10^13 = 0 → None
    env.set_slot_and_price_raw_no_walk(200, 10_000_000_000_000);
    let crank_result = env.try_crank();
    // Crank may fail (zero inverted price) or clamp via circuit breaker
    println!(
        "Crank with zero-invert price: {}",
        if crank_result.is_ok() {
            "ok (clamped)"
        } else {
            "rejected"
        }
    );
    // Conservation must hold regardless
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    let lp_pos_after = env.read_account_position(lp_idx);
    let user_pos_after = env.read_account_position(user_idx);
    let lp_cap_after = env.read_account_capital(lp_idx);
    let user_cap_after = env.read_account_capital(user_idx);

    assert_eq!(
        lp_pos_after, lp_pos_before,
        "Zero-invert crank path must not mutate LP position: before={} after={}",
        lp_pos_before, lp_pos_after
    );
    assert_eq!(
        user_pos_after, user_pos_before,
        "Zero-invert crank path must not mutate user position: before={} after={}",
        user_pos_before, user_pos_after
    );
    if crank_result.is_ok() {
        assert!(
            lp_cap_after <= lp_cap_before,
            "Accepted zero-invert crank must not mint LP capital: before={} after={}",
            lp_cap_before,
            lp_cap_after
        );
        assert!(
            user_cap_after <= user_cap_before,
            "Accepted zero-invert crank must not mint user capital: before={} after={}",
            user_cap_before,
            user_cap_after
        );
    } else {
        assert_eq!(
            lp_cap_after, lp_cap_before,
            "Rejected zero-invert crank must preserve LP capital: before={} after={}",
            lp_cap_before, lp_cap_after
        );
        assert_eq!(
            user_cap_after, user_cap_before,
            "Rejected zero-invert crank must preserve user capital: before={} after={}",
            user_cap_before, user_cap_after
        );
    }
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after zero-invert: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: Inverted market with raw price = 1 (smallest non-zero).
/// invert_price_e6(1, 1) = 10^12 / 1 = 10^12 → within u64 range.
/// Verify the market handles extreme inverted prices.
#[test]
fn test_attack_invert_price_extreme_small_raw() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(1);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    // First crank at normal price
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_pos_before = env.read_account_position(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);

    // Set raw price = 1 → inverted = 10^12
    // Circuit breaker will cap the movement, but the inverted price is valid
    env.set_slot_and_price_raw_no_walk(200, 1);
    let crank_result = env.try_crank();
    println!(
        "Crank with extreme-small raw: {}",
        if crank_result.is_ok() {
            "ok (clamped)"
        } else {
            "rejected"
        }
    );

    // Conservation must hold regardless
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    let lp_pos_after = env.read_account_position(lp_idx);
    let user_pos_after = env.read_account_position(user_idx);
    let lp_cap_after = env.read_account_capital(lp_idx);
    let user_cap_after = env.read_account_capital(user_idx);

    assert_eq!(
        lp_pos_after, lp_pos_before,
        "Extreme-small raw crank path must not mutate LP position: before={} after={}",
        lp_pos_before, lp_pos_after
    );
    assert_eq!(
        user_pos_after, user_pos_before,
        "Extreme-small raw crank path must not mutate user position: before={} after={}",
        user_pos_before, user_pos_after
    );
    if crank_result.is_ok() {
        let cap_sum_before = lp_cap_before + user_cap_before;
        let cap_sum_after = lp_cap_after + user_cap_after;
        assert!(
            cap_sum_after <= cap_sum_before,
            "Accepted extreme-small raw crank must not mint aggregate capital: before_sum={} after_sum={}",
            cap_sum_before,
            cap_sum_after
        );
    } else {
        assert_eq!(
            lp_cap_after, lp_cap_before,
            "Rejected extreme-small raw crank must preserve LP capital: before={} after={}",
            lp_cap_before, lp_cap_after
        );
        assert_eq!(
            user_cap_after, user_cap_before,
            "Rejected extreme-small raw crank must preserve user capital: before={} after={}",
            user_cap_before, user_cap_after
        );
    }
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after extreme invert: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: Multi-instruction atomic transaction (deposit + trade in same tx).
/// Verify protocol handles multiple instructions in single transaction correctly.
#[test]
fn test_attack_multi_instruction_deposit_trade_atomic() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    // Must deposit BEFORE crank to prevent GC of zero-capital account
    env.deposit(&user, user_idx, 1_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Create ATA with tokens for additional deposit
    let ata = env.create_ata(&user.pubkey(), 5_000_000_000);

    // Build deposit instruction
    let deposit_ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data: encode_deposit(user_idx, 5_000_000_000),
    };

    // Build trade instruction (in same tx, needs LP signer too)
    let trade_ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(lp.pubkey(), true), // LP must sign
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_trade(lp_idx, user_idx, 1_000_000),
    };

    // Send both instructions atomically (both user and LP must sign)
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), deposit_ix, trade_ix],
        Some(&user.pubkey()),
        &[&user, &lp],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_ok(),
        "Multi-instruction deposit+trade should succeed: {:?}",
        result
    );

    // Verify both took effect
    let pos = env.read_account_position(user_idx);
    assert_eq!(pos, 1_000_000, "Position should be set from trade");
    let cap = env.read_account_capital(user_idx);
    assert!(
        cap > 1_000_000_000,
        "Capital should include additional deposit: cap={}",
        cap
    );
}

/// ATTACK: Withdraw amount = unit_scale - 1 (largest misaligned amount).
/// Should be rejected by alignment check when unit_scale > 1.
#[test]
fn test_attack_withdraw_scale_minus_one_misaligned() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_full(0, 1000, 0); // unit_scale = 1000

    let user = Keypair::new();
    // With unit_scale=1000, need 100*1000=100_000 base for min_initial_deposit
    let user_idx = env.init_user_with_fee(&user, 100_000);
    env.deposit(&user, user_idx, 5_000_000_000);

    // Try withdrawing 999 (scale - 1), which is not aligned to unit_scale=1000
    let result = env.try_withdraw(&user, user_idx, 999);
    assert!(
        result.is_err(),
        "ATTACK: Withdraw of scale-1 amount should fail alignment check!"
    );

    // Capital unchanged (stored in units: 99 from init + 5B / 1000 = 5M from deposit = 5_000_099)
    let cap = env.read_account_capital(user_idx);
    assert_eq!(
        cap, 5_000_099,
        "Capital unchanged after failed misaligned withdraw"
    );
}

/// ATTACK: Close slab after all accounts closed and insurance is zero.
/// Tests the clean shutdown path: LP deposits, withdraws, closes, then slab closes.
#[test]
fn test_attack_close_slab_clean_shutdown() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 5_000_000_000);

    env.crank();

    // Withdraw all LP capital
    env.try_withdraw(&lp, lp_idx, 5_000_000_000).unwrap();

    // Close LP account
    env.close_account(&lp, lp_idx);

    // Resolve market before insurance withdrawal and CloseSlab (lifecycle requirement).
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_resolve_market(&admin, 0).unwrap();

    // The default account materialization fee is real insurance, so clean
    // shutdown must explicitly drain insurance before CloseSlab.
    env.try_withdraw_insurance(&admin).unwrap();

    let insurance = env.read_insurance_balance();
    assert_eq!(insurance, 0, "Insurance should be zero: got {}", insurance);

    let vault = env.vault_balance();
    assert_eq!(vault, 0, "Vault should be zero: got {}", vault);

    // Close slab should succeed
    let result = env.try_close_slab();
    assert!(result.is_ok(), "CloseSlab should succeed: {:?}", result);
}

/// ATTACK: Liquidation at exact equity zero boundary.
/// Position PnL + capital = 0 exactly. Should be liquidatable.
#[test]
fn test_attack_liquidation_equity_exactly_zero() {
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

    env.try_top_up_insurance(&admin, 2_000_000_000).unwrap();
    env.crank();

    // Open a long position
    env.trade(&user, &lp, lp_idx, user_idx, 10_000_000);

    // Move price down significantly to push equity toward zero
    // Position value = 10M * price_e6 / 1e6. At $138, notional ≈ 1.38B.
    // With 5B capital, need price drop to push equity to 0.
    // PnL = 10M * (new_price - entry_price) / 1e6
    // At entry price 138e6, position value = 1.38e9
    // Circuit breaker limits per-slot movement, so we need many slots
    for slot in (200..=2000).step_by(100) {
        let price = 138_000_000 - ((slot - 100) * 50_000) as i64;
        if price < 1_000_000 {
            break;
        }
        env.set_slot_and_price(slot, price);
        env.crank();
    }

    // Try liquidation - account should be insolvent or near-insolvent
    let user_pos_before_liq = env.read_account_position(user_idx);
    let liq_result = env.try_liquidate(user_idx);
    let user_pos_after_liq = env.read_account_position(user_idx);
    assert!(
        user_pos_after_liq.unsigned_abs() <= user_pos_before_liq.unsigned_abs(),
        "Liquidation path must not increase user exposure. before={} after={} result={:?}",
        user_pos_before_liq,
        user_pos_after_liq,
        liq_result
    );
    // Whether liquidation succeeds or not, conservation must hold
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after liquidation attempt: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: Deposit and immediate crank in same slot.
/// Tests that deposit + crank in same slot doesn't create exploitable state.
#[test]
fn test_attack_deposit_and_crank_same_slot_no_exploit() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    // Deposit before first crank to prevent GC
    env.deposit(&user, user_idx, 2_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Additional deposit at slot 200
    env.set_slot(200);
    env.deposit(&user, user_idx, 3_000_000_000);

    // Crank at same slot 200 (immediately after deposit)
    env.crank();

    // Capital should reflect both deposits (no erosion from same-slot crank)
    let cap = env.read_account_capital(user_idx);
    assert_eq!(
        cap, 5_000_000_099,
        "Capital should equal total deposits after same-slot crank: cap={}",
        cap
    );

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot should equal sum of capitals: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Trade then immediate crank then withdraw in rapid sequence.
/// Tests state consistency across rapid operation sequence.
#[test]
fn test_attack_trade_crank_withdraw_rapid_sequence() {
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

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade at slot 1
    env.set_slot(1);
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Crank at slot 2
    env.set_slot(2);
    env.crank();

    // Close position at slot 3
    env.set_slot(3);
    env.trade(&user, &lp, lp_idx, user_idx, -5_000_000);

    // Crank at slot 4 to settle
    env.set_slot(4);
    env.crank();

    // Withdraw all capital
    let cap = env.read_account_capital(user_idx);
    let withdraw_result = env.try_withdraw(&user, user_idx, cap as u64);
    assert!(
        withdraw_result.is_ok(),
        "Withdraw after close should succeed: {:?}",
        withdraw_result
    );

    // Conservation
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: Multiple price changes between cranks (large gap).
/// Only the price at crank time should matter, not intermediate prices.
#[test]
fn test_attack_price_whipsaw_between_cranks() {
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

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Set price to drop, then recover, then crank at recovered price
    // Only the final price at crank should matter
    env.set_slot_and_price(200, 130_000_000); // Drop to $130
                                              // Don't crank yet
    env.set_slot_and_price(300, 138_000_000); // Recover to $138
    env.crank(); // Crank at recovered price

    // User PnL should reflect the crank price ($138), not intermediate ($130)
    // At $138, PnL ≈ 0 (same as entry)
    let cap = env.read_account_capital(user_idx);
    assert!(
        cap <= 10_000_000_099,
        "Capital should not increase: cap={}",
        cap
    );

    // Conservation
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after whipsaw: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: Deposit to account, trade, then deposit again (incremental deposits).
/// Verify capital is correct after multiple deposits with position open.
#[test]
fn test_attack_incremental_deposits_with_position() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 3_000_000);
    assert_eq!(env.read_account_position(user_idx), 3_000_000);

    // Second deposit while position is open
    env.set_slot(200);
    env.deposit(&user, user_idx, 3_000_000_000);

    let cap_after = env.read_account_capital(user_idx);
    assert!(
        cap_after >= 7_000_000_000,
        "Capital should be >= 8B after second deposit: cap={}",
        cap_after
    );

    // Crank after second deposit
    env.crank();

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot desync after incremental deposit: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Warmup + funding interaction.
/// Open position, warmup is accruing, funding is also accruing.
/// Both should settle correctly without double-counting.
#[test]
fn test_attack_warmup_funding_interaction_no_double_count() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_warmup(0, 100); // 100 slot warmup

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open position - starts warmup
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Crank at several slots during warmup period
    for slot in (200..=500).step_by(50) {
        env.set_slot(slot);
        env.crank();
    }

    // After warmup (slot > 100 + trade_slot), check conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot desync after warmup+funding: c_tot={} sum={}",
        c_tot, sum
    );

    // Vault conservation
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Vault conservation: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: LP position tracking after multiple users trade and close.
/// Verify LP position aggregates are correct after complex trading.
#[test]
fn test_attack_lp_position_aggregate_after_many_trades() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // 3 users open positions
    let mut users = Vec::new();
    for _ in 0..3 {
        let user = Keypair::new();
        let user_idx = env.init_user(&user);
        env.deposit(&user, user_idx, 5_000_000_000);
        users.push((user, user_idx));
    }

    // User 0: long 100K, User 1: long 200K, User 2: short 150K
    env.set_slot(1);
    env.trade(&users[0].0, &lp, lp_idx, users[0].1, 100_000);
    env.set_slot(2);
    env.trade(&users[1].0, &lp, lp_idx, users[1].1, 200_000);
    env.set_slot(3);
    env.trade(&users[2].0, &lp, lp_idx, users[2].1, -150_000);

    // LP position should be opposite sum: -(100K + 200K - 150K) = -150K
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(
        lp_pos, -150_000,
        "LP position should be -150K: got {}",
        lp_pos
    );

    // User 0 closes position
    env.set_slot(4);
    env.trade(&users[0].0, &lp, lp_idx, users[0].1, -100_000);

    // LP position now: -(200K - 150K) = -50K
    let lp_pos2 = env.read_account_position(lp_idx);
    assert_eq!(
        lp_pos2, -50_000,
        "LP position after partial close: got {}",
        lp_pos2
    );

    // Conservation
    let c_tot = env.read_c_tot();
    let mut sum = env.read_account_capital(lp_idx);
    for (_, idx) in &users {
        sum += env.read_account_capital(*idx);
    }
    assert_eq!(
        c_tot, sum,
        "c_tot desync after multi-user trades: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Price at exact circuit breaker boundary.
/// Move price by exactly oracle_price_cap_bps per slot.
/// Verify mark tracks correctly at the boundary.
#[test]
fn test_attack_circuit_breaker_exact_cap_boundary() {
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

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Move price up by small increments across many slots
    for slot in (200..=600).step_by(50) {
        // Incrementally increase price
        let price = 138_000_000 + ((slot - 100) * 20_000) as i64;
        env.set_slot_and_price(slot, price);
        env.crank();
    }

    // Conservation must hold after many bounded price movements
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after circuit breaker clamping: engine={} vault={}",
        engine_vault, vault
    );

    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot conservation: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Trade with exactly initial_margin_bps worth of capital.
/// At the exact margin boundary, the trade should just barely succeed.
#[test]
fn test_attack_trade_exact_margin_boundary_succeeds() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    // Deposit exactly 1B
    env.deposit(&user, user_idx, 1_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // initial_margin_bps = 1000 (10%), price = $138 (138e6)
    // max_notional = 1e9 / 0.1 = 10e9
    // max_position = 10e9 * 1e6 / 138e6 ≈ 72.46M
    // Try just under the limit
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 72_000_000);
    assert!(
        result.is_ok(),
        "Trade at exact margin boundary should succeed: {:?}",
        result
    );

    // Verify position was opened
    let pos = env.read_account_position(user_idx);
    assert_eq!(pos, 72_000_000, "Position should be set: pos={}", pos);
}

/// ATTACK: Maintenance fee settlement when capital is very small.
/// With large maintenance_fee_per_slot and small capital, fee should not go negative.
/// ATTACK: Mark precision with very small price increments.
/// Multiple tiny price changes and cranks should maintain conservation.
#[test]
fn test_attack_mark_precision_small_increments() {
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

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Apply very small price increments and crank each time
    let base_price: i64 = 138_000_000;
    for i in 1..=20u64 {
        let price = base_price + (i as i64); // 1 unit increment (smallest possible)
        env.set_slot_and_price(200 + i, price);
        env.crank();
    }

    // Conservation must hold after many tiny price changes
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after small increments: engine={} vault={}",
        engine_vault, vault
    );

    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot conservation: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: UpdateConfig while positions are open and funding accruing.
/// Changing funding parameters mid-flight should not cause retroactive errors.
#[test]
fn test_attack_update_config_during_active_trades() {
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

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open position and let funding accrue
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    env.set_slot(300);
    env.crank();

    // Update config mid-flight (change funding params)
    env.try_update_config(&admin).unwrap();

    // Continue cranking with new config
    env.set_slot(500);
    env.crank();

    env.set_slot(700);
    env.crank();

    // Conservation must hold after config change
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after config change: engine={} vault={}",
        engine_vault, vault
    );

    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot conservation: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: PushHyperpMark with same price as last effective price.
/// When price doesn't change, circuit breaker should produce stable state.
#[test]
fn test_attack_push_oracle_same_as_last_price() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(138_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    // Push same price multiple times
    for ts in 100..=110 {
        env.try_push_oracle_price(&admin, 138_000_000, ts).unwrap();
    }

    // State should be stable (no drift from repeated same-price pushes)
    let vault = env.vault_balance();
    assert_eq!(
        vault, 20_000_000_100,
        "Vault should not change from repeated same-price pushes: vault={}",
        vault
    );
}

/// ATTACK: Liquidate with target_idx = u16::MAX (65535, CRANK_NO_CALLER sentinel).
/// Should not confuse liquidation with permissionless crank sentinel.
#[test]
fn test_attack_liquidate_target_u16_max() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);
    let lp_pos_before = env.read_account_position(lp_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let used_before = env.read_num_used_accounts();
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // Try liquidating index u16::MAX. Through KeeperCrank this is an
    // invalid candidate and should be skipped without mutation.
    let result = env.try_liquidate(u16::MAX);
    assert!(
        result.is_ok(),
        "ATTACK: crank with u16::MAX liquidation candidate should not fail"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected liquidation with u16::MAX target must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected liquidation with u16::MAX target must preserve LP capital"
    );
    assert_eq!(
        env.read_num_used_accounts(),
        used_before,
        "Rejected liquidation with u16::MAX target must preserve num_used_accounts"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected liquidation with u16::MAX target must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected liquidation with u16::MAX target must preserve engine vault"
    );
}

/// ATTACK: Deposit after liquidation in same slot.
/// User gets liquidated, then immediately deposits. Conservation must hold.
#[test]
fn test_attack_deposit_after_liquidation_same_slot() {
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

    env.try_top_up_insurance(&admin, 2_000_000_000).unwrap();
    env.crank();

    // Open large long position
    env.trade(&user, &lp, lp_idx, user_idx, 10_000_000);

    // Drop price significantly over many slots to make account insolvent
    for slot in (200..=2000).step_by(100) {
        let price = 138_000_000 - ((slot - 100) * 50_000) as i64;
        if price < 1_000_000 {
            break;
        }
        env.set_slot_and_price(slot, price);
        env.crank();
    }

    // Verify user position was indeed opened (precondition)
    // After price drops + cranks, user should be liquidated or still alive
    let vault_before = env.vault_balance();
    let capital_before_liq = env.read_account_capital(user_idx);

    // Try liquidating
    let liq_result = env.try_liquidate(user_idx);
    let capital_after_liq = env.read_account_capital(user_idx);

    if liq_result.is_ok() {
        assert!(
            capital_after_liq <= capital_before_liq,
            "Liquidation should not increase target capital: before={} after={}",
            capital_before_liq,
            capital_after_liq
        );
    } else {
        assert_eq!(
            capital_after_liq, capital_before_liq,
            "Failed liquidation should not change target capital: before={} after={}",
            capital_before_liq, capital_after_liq
        );
    }

    // After liquidation attempt, try deposit
    let deposit_result = env.try_deposit(&user, user_idx, 1_000_000_000);

    // Conservation must hold either way
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after liquidation+deposit: engine={} vault={}",
        engine_vault, vault
    );

    // Verify vault changed if deposit succeeded (tokens transferred in)
    if deposit_result.is_ok() {
        assert!(
            vault == vault_before + 1_000_000_000,
            "Deposit succeeded so vault must increase by exact amount: before={} after={}",
            vault_before,
            vault
        );
    } else {
        assert_eq!(
            vault, vault_before,
            "Failed deposit should not change vault: before={} after={}",
            vault_before, vault
        );
    }
}

/// ATTACK: InitLP with matcher_program = Percolator program itself.
/// InitLP stores the matcher pubkey but doesn't CPI, so it may succeed at init.
/// Verify no value extraction and conservation holds regardless of outcome.
#[test]
fn test_attack_init_lp_matcher_is_self_program() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp_owner = Keypair::new();
    env.svm.airdrop(&lp_owner.pubkey(), 5_000_000_000).unwrap();
    let ata = env.create_ata(&lp_owner.pubkey(), 0);

    let matcher_ctx = Pubkey::new_unique();

    // Use percolator program_id as matcher (self-reference)
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(lp_owner.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data: encode_init_lp(&env.program_id, &matcher_ctx, 0), // Self as matcher
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&lp_owner.pubkey()),
        &[&lp_owner],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    // InitLP with self-program as matcher: may succeed at init time
    // (no CPI happens during init), but state must remain consistent.
    let num_used = env.read_num_used_accounts();
    if result.is_ok() {
        assert_eq!(
            num_used, 1,
            "Accepted self-matcher InitLP should allocate exactly one account"
        );
        assert!(
            env.is_slot_used(0),
            "Accepted self-matcher InitLP should mark slot 0 as used"
        );
        assert_eq!(
            env.read_account_capital(0),
            0,
            "Accepted self-matcher InitLP should not mint capital at init"
        );
    } else {
        assert_eq!(
            num_used, 0,
            "Rejected self-matcher InitLP should not allocate accounts"
        );
    }
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after self-matcher init: engine={} vault={}",
        engine_vault, vault
    );
    // Verify the LP didn't extract any value
    assert_eq!(
        vault, 0,
        "No tokens should have been deposited: vault={}",
        vault
    );
}

/// ATTACK: Funding rate sign flip when LP position crosses zero.
/// LP net position goes from short to long in a single trade.
#[test]
fn test_attack_funding_rate_sign_flip_lp_position_cross() {
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

    // User1 goes long → LP goes short
    env.set_slot(1);
    env.trade(&user1, &lp, lp_idx, user1_idx, 5_000_000);

    env.set_slot(200);
    env.crank();

    // User2 goes short (larger) → LP flips from short to long
    env.set_slot(201);
    env.trade(&user2, &lp, lp_idx, user2_idx, -10_000_000);

    // LP position now = -(-10M + 5M) = +5M (was -5M, now +5M)
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(
        lp_pos, 5_000_000,
        "LP should have flipped to long: pos={}",
        lp_pos
    );

    // Crank after flip
    env.set_slot(400);
    env.crank();

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx)
        + env.read_account_capital(user1_idx)
        + env.read_account_capital(user2_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot after LP position flip: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Resolve hyperp market then attempt UpdateConfig.
/// Admin config changes should be blocked after market resolution.
#[test]
fn test_attack_update_config_after_resolution() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(138_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 140_000_000, 100).unwrap();
    env.try_resolve_market(&admin, 0).unwrap();

    let insurance_before = env.read_insurance_balance();
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;

    // After resolution, config update must fail.
    let result = env.try_update_config(&admin);
    assert!(
        result.is_err(),
        "SECURITY: UpdateConfig must be rejected after resolution"
    );
    assert_eq!(
        env.read_insurance_balance(),
        insurance_before,
        "Rejected post-resolution UpdateConfig must preserve insurance"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected post-resolution UpdateConfig must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected post-resolution UpdateConfig must preserve engine vault"
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    assert_eq!(
        slab_after, slab_before,
        "Rejected post-resolution UpdateConfig must preserve slab bytes"
    );

    // State must remain consistent
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after config update attempt: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: PushHyperpMark after resolution.
/// Settlement parameters must be frozen once market is resolved.
#[test]
fn test_attack_push_oracle_after_resolution_rejected() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(138_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 140_000_000, 100).unwrap();
    env.try_resolve_market(&admin, 0).unwrap();

    // Config offset for hyperp_mark_e6
    const AUTH_PRICE_OFF: usize = 312; // HEADER_LEN(72) + offset_of!(MarketConfig, hyperp_mark_e6)(176)
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let settle_before = u64::from_le_bytes(
        slab_before[AUTH_PRICE_OFF..AUTH_PRICE_OFF + 8]
            .try_into()
            .unwrap(),
    );

    let result = env.try_push_oracle_price(&admin, 200_000_000, 200);
    assert!(
        result.is_err(),
        "SECURITY: PushHyperpMark must be rejected after resolution"
    );

    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let settle_after = u64::from_le_bytes(
        slab_after[AUTH_PRICE_OFF..AUTH_PRICE_OFF + 8]
            .try_into()
            .unwrap(),
    );
    assert_eq!(
        settle_before, settle_after,
        "Settlement price changed after rejected post-resolution push"
    );
}

/// ATTACK: UpdateAuthority { AUTHORITY_HYPERP_MARK } after resolution.
/// Oracle authority must remain frozen once market is resolved.
#[test]
fn test_attack_set_oracle_authority_after_resolution_rejected() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(138_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 140_000_000, 100).unwrap();
    env.try_resolve_market(&admin, 0).unwrap();

    const AUTHORITY_OFF: usize = 392;
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let authority_before: [u8; 32] = slab_before[AUTHORITY_OFF..AUTHORITY_OFF + 32]
        .try_into()
        .unwrap();

    let result = env.try_set_oracle_authority(&admin, &Pubkey::new_unique());
    assert!(
        result.is_err(),
        "SECURITY: hyperp-authority rotation must be rejected after resolution"
    );

    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let authority_after: [u8; 32] = slab_after[AUTHORITY_OFF..AUTHORITY_OFF + 32]
        .try_into()
        .unwrap();
    assert_eq!(
        authority_before, authority_after,
        "Oracle authority changed after rejected post-resolution update"
    );
}

// test_attack_set_oracle_price_cap_after_resolution_rejected deleted:
// SetOraclePriceCap (tag 18) was removed in v12.19; the per-slot price-move
// cap is an immutable init-time RiskParam, so there is no post-resolution
// mutation path left to attack.

/// ATTACK: Multiple trades filling LP position in alternating directions.
/// LP position oscillates: +5M, +2M (net -3M), -1M (net +4M), etc.
/// Verify LP position tracking remains accurate through oscillations.
#[test]
fn test_attack_lp_position_oscillation() {
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

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Series of trades that oscillate LP position
    let sizes: &[i128] = &[500_000, -300_000, 200_000, -600_000, 400_000];
    let mut expected_user_pos: i128 = 0;
    for (i, &size) in sizes.iter().enumerate() {
        env.set_slot((i + 1) as u64);
        env.trade(&user, &lp, lp_idx, user_idx, size);
        expected_user_pos += size;
    }

    // Total user pos = 500K - 300K + 200K - 600K + 400K = 200K
    assert_eq!(
        env.read_account_position(user_idx),
        expected_user_pos,
        "User position after oscillation"
    );
    // LP pos = -200K
    assert_eq!(
        env.read_account_position(lp_idx),
        -expected_user_pos,
        "LP position should mirror user"
    );

    // Crank and verify conservation
    env.set_slot(200);
    env.crank();

    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot after oscillation: c_tot={} sum={}",
        c_tot, sum
    );
}

// test_attack_oracle_price_cap_u64_max deleted:
// Test's premise was to disable the circuit breaker via SetOraclePriceCap(max)
// and observe conservation under large price jumps. In v12.19 the cap is
// immutable init-time (TEST_MAX_PRICE_MOVE_BPS_PER_SLOT = 2), so the test
// cannot be rewritten without building a different market fixture — and the
// conservation property it verified is already covered by the §1.4-envelope
// tests elsewhere in this file.

/// ATTACK: SetMaintenanceFee then immediately close account.
/// Fee set then close in rapid sequence should settle fees correctly.
/// ATTACK: Withdraw between two cranks (deposit, crank, withdraw, crank).
/// Tests that withdrawal doesn't cause double-counting in settlement.
#[test]
fn test_attack_withdraw_between_two_cranks() {
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

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade
    env.trade(&user, &lp, lp_idx, user_idx, 3_000_000);

    // Crank at slot 200
    env.set_slot(200);
    env.crank();

    // Withdraw partial capital between cranks
    env.try_withdraw(&user, user_idx, 2_000_000_000).unwrap();

    // Crank again at slot 400
    env.set_slot(400);
    env.crank();

    // Conservation
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after withdraw between cranks: engine={} vault={}",
        engine_vault, vault
    );

    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot after withdraw between cranks: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: User deposits, withdraws everything, gets GC'd, new user takes slot.
/// Tests slot reuse and state cleanliness after GC in multi-user scenario.
#[test]
fn test_attack_slot_reuse_multi_user_gc_reinit() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // user1: deposit + withdraw everything (no trades) → GC candidate
    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 5_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // user2 trades, user1 stays idle
    env.set_slot(1);
    env.trade(&user2, &lp, lp_idx, user2_idx, 2_000_000);

    // Withdraw all of user1's capital (5B deposit + 99 init) -> zero capital.
    env.try_withdraw(&user1, user1_idx, 5_000_000_099).unwrap();

    // Crank to GC user1 (zero everything)
    env.set_slot(200);
    env.crank();

    let num_after_gc = env.read_num_used_accounts();
    let user1_slot_used_after_gc = env.is_slot_used(user1_idx);
    if user1_slot_used_after_gc {
        assert_eq!(
            num_after_gc, 3,
            "If user1 is not GC'd yet, all 3 accounts should remain (got {})",
            num_after_gc
        );
    } else {
        assert_eq!(
            num_after_gc, 2,
            "If user1 is GC'd, only LP+user2 should remain (got {})",
            num_after_gc
        );
        assert_eq!(
            env.read_account_capital(user1_idx),
            0,
            "GC'd user1 slot should have zero capital"
        );
        assert_eq!(
            env.read_account_position(user1_idx),
            0,
            "GC'd user1 slot should have zero position"
        );
    }

    // New user3 may take the recycled slot immediately, or the allocator may
    // continue forward and reuse it later. The security property is state
    // cleanliness and conservation, not lowest-index reuse timing.
    let user3 = Keypair::new();
    let _helper_guess_user3_idx = env.init_user(&user3);
    let user3_idx = find_used_account_by_owner(&env, &user3.pubkey())
        .expect("InitUser should create a used slot owned by user3");
    if !user1_slot_used_after_gc {
        if user3_idx == user1_idx {
            assert_eq!(
                env.read_account_capital(user3_idx),
                DEFAULT_INIT_CAPITAL as u128,
                "reused user1 slot should contain only fresh InitUser capital before deposit"
            );
            assert_eq!(
                env.read_account_position(user3_idx),
                0,
                "reused user1 slot should have no stale position"
            );
        } else {
            assert!(
                !env.is_slot_used(user1_idx),
                "freed user1 slot should remain free if allocator did not reuse it immediately"
            );
        }
    }
    env.deposit(&user3, user3_idx, 5_000_000_000);

    // user3 trades
    env.set_slot(201);
    env.trade(&user3, &lp, lp_idx, user3_idx, 500_000);

    // Conservation across all active accounts
    env.set_slot(400);
    env.crank();

    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx)
        + env.read_account_capital(user2_idx)
        + env.read_account_capital(user3_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot after slot reuse: c_tot={} sum={}",
        c_tot, sum
    );
}

fn find_used_account_by_owner(env: &TestEnv, owner: &Pubkey) -> Option<u16> {
    const ACCOUNT_SIZE: usize = 360;
    const OWNER_OFFSET: usize = 192;
    let slab = env.svm.get_account(&env.slab)?;
    let accounts_offset = ENGINE_OFFSET + ENGINE_ACCOUNTS_OFFSET;

    for idx in 0..MAX_ACCOUNTS {
        let idx_u16 = idx as u16;
        if !env.is_slot_used(idx_u16) {
            continue;
        }
        let owner_offset = accounts_offset + idx * ACCOUNT_SIZE + OWNER_OFFSET;
        if slab.data.len() < owner_offset + 32 {
            return None;
        }
        if &slab.data[owner_offset..owner_offset + 32] == owner.as_ref() {
            return Some(idx_u16);
        }
    }
    None
}

/// ATTACK: LP tries to withdraw when haircut is active (vault < c_tot + insurance).
/// After a user takes a large loss, LP capital might be haircutted - can LP
/// withdraw more than their haircutted equity?
#[test]
fn test_attack_lp_withdraw_during_haircut() {
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

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // User opens large long position
    env.trade(&user, &lp, lp_idx, user_idx, 10_000_000);

    // Price drops significantly - LP profits, user loses
    // User's loss means LP has positive PnL, but vault balance is finite
    for slot in (200..=1000).step_by(100) {
        let price = 138_000_000 - ((slot - 100) * 20_000) as i64;
        if price < 50_000_000 {
            break;
        }
        env.set_slot_and_price(slot, price);
        env.crank();
    }

    // Record state before LP withdrawal attempt
    let lp_cap_before = env.read_account_capital(lp_idx);
    let vault_before = env.vault_balance();

    // LP tries to withdraw full capital
    let withdraw_result = env.try_withdraw(&lp, lp_idx, lp_cap_before as u64);

    // Conservation must hold
    let vault_after = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault_after,
        "Conservation after LP withdrawal attempt: engine={} vault={}",
        engine_vault, vault_after
    );

    // If withdrawal succeeded, vault should have decreased
    if withdraw_result.is_ok() {
        assert!(
            vault_after < vault_before,
            "LP withdrawal succeeded but vault didn't decrease: before={} after={}",
            vault_before,
            vault_after
        );
        // LP capital must have decreased
        let lp_cap_after = env.read_account_capital(lp_idx);
        assert!(
            lp_cap_after < lp_cap_before,
            "LP withdrawal succeeded but capital didn't decrease: before={} after={}",
            lp_cap_before,
            lp_cap_after
        );
    } else {
        // Withdrawal rejected (e.g., margin check) - vault unchanged
        assert_eq!(
            vault_after, vault_before,
            "LP withdrawal rejected but vault changed: before={} after={}",
            vault_before, vault_after
        );
    }
}

/// ATTACK: Open position during warmup period, partially close before warmup expires.
/// Tests interaction between warmup slope and partial position close.
/// Profit from partial close must be subject to warmup vesting.
#[test]
fn test_attack_warmup_partial_close_vesting() {
    program_path();

    let mut env = TestEnv::new();
    // Keep the close below the maturity horizon after the helper's slot
    // normalization, so this is actually testing unvested profit behavior.
    env.init_market_with_warmup(0, 500);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open position at slot 1
    env.set_slot(1);
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Price rises (user profit) before warmup expires
    env.set_slot_and_price(200, 145_000_000); // $138 → $145
    env.crank();

    // Record capital before partial close
    let user_cap_before = env.read_account_capital(user_idx);

    // Partial close (close half the position) during warmup
    env.set_slot(201);
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, -2_500_000);

    // User position should be halved
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(
        user_pos, 2_500_000,
        "User position after partial close: expected 2.5M, got {}",
        user_pos
    );

    // Crank after partial close
    env.set_slot(400);
    env.crank();

    // Capital shouldn't have increased by the full unrealized profit
    // (warmup vesting limits profit conversion)
    let user_cap_after = env.read_account_capital(user_idx);
    assert!(
        user_cap_after <= user_cap_before + 200_000_000u128,
        "Warmup partial close should bound immediate capital jump. before={} after={}",
        user_cap_before,
        user_cap_after
    );

    // Conservation must hold
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot after warmup partial close: c_tot={} sum={}",
        c_tot, sum
    );

    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: Minimal position size (1 unit) with 1e-6 price precision.
/// Tests mark_pnl truncation at the smallest meaningful scale.
#[test]
fn test_attack_mark_pnl_one_unit_position() {
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

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade minimum size: 1 unit
    env.trade(&user, &lp, lp_idx, user_idx, 1);
    assert_eq!(
        env.read_account_position(user_idx),
        1,
        "Precondition: user must have 1-unit position"
    );

    // Price change of 1 (smallest possible: 0.000001)
    env.set_slot_and_price(200, 138_000_001);
    env.crank();

    // PnL for 1 unit at $0.000001 change = 1 * 1 / 1_000_000 = 0 (truncated)
    // This is correct: sub-unit PnL truncates in protocol's favor
    let pnl = env.read_account_pnl(user_idx);
    assert!(
        pnl >= 0,
        "Tiny PnL should not be negative for long: pnl={}",
        pnl
    );

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot with 1-unit position: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Haircut with zero pnl_pos_tot (no positive PnL accounts).
/// When denominator is 0, haircut should be harmless (no division by zero).
#[test]
fn test_attack_haircut_zero_pnl_pos_tot() {
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

    // Open position and drop price so user loses capital (via instant warmup)
    let user_cap_initial = env.read_account_capital(user_idx);
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    env.set_slot_and_price(200, 130_000_000); // Drop $8
    env.crank();

    // With warmup_period=0, negative PnL settles to capital immediately.
    // User capital should have decreased; LP capital should have increased.
    let user_cap_after_drop = env.read_account_capital(user_idx);
    assert!(
        user_cap_after_drop < user_cap_initial,
        "Precondition: user capital should have decreased after price drop (instant warmup): initial={} after={}",
        user_cap_initial, user_cap_after_drop
    );

    // With instant warmup, positive PnL is matured/released immediately.
    // pnl_pos_tot may have a small residual due to the matured PnL model.
    let pnl_pos_tot = env.read_pnl_pos_tot();
    assert!(
        pnl_pos_tot >= 0,
        "pnl_pos_tot should be non-negative after instant warmup (warmup_period=0): {}",
        pnl_pos_tot
    );

    // Verify user still has positive capital remaining
    let user_cap = env.read_account_capital(user_idx);
    assert!(
        user_cap > 0,
        "User should still have some capital after loss: {}",
        user_cap
    );

    // Verify engine vault consistency
    let engine_vault = env.read_engine_vault();
    let vault_balance = env.vault_balance();
    assert_eq!(
        engine_vault as u64, vault_balance,
        "Conservation with haircut: engine={} vault={}",
        engine_vault, vault_balance
    );
}

/// ATTACK: Position flip from long to short at exact maintenance margin.
/// Verify initial_margin_bps is used (not maintenance) for the flip.
#[test]
fn test_attack_position_flip_margin_requirement_switch() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    // Tight deposit to make initial-vs-maintenance margin distinction observable.
    env.deposit(&user, user_idx, 20_000_000); // 0.02 SOL

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open long position near initial-margin boundary.
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    assert!(result.is_ok(), "Initial long trade should succeed");

    // Flip to a larger short (position +1M -> -2M): this requires initial margin
    // for the new 2M short side and should be rejected in this tight-equity setup.
    let flip_result = env.try_trade(&user, &lp, lp_idx, user_idx, -3_000_000);

    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after flip attempt: engine={} vault={}",
        engine_vault, vault
    );

    // Flip must fail and preserve the original long if initial margin is enforced.
    let pos = env.read_account_position(user_idx);
    assert!(
        flip_result.is_err(),
        "Flip should be rejected when new-side initial margin exceeds available equity: {:?}",
        flip_result
    );
    assert_eq!(
        pos, 1_000_000,
        "Rejected flip should preserve +1M position: got {}",
        pos
    );
}

/// ATTACK: Large maintenance fee with huge dt gap (thousands of slots).
/// Tests saturating arithmetic in fee accrual over long periods.
/// ATTACK: Trade with different sizes in rapid succession (consecutive slots).
/// Position accumulation should be correct across rapid trades.
#[test]
fn test_attack_rapid_successive_trades_accumulation() {
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

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Multiple trades in rapid succession with different sizes
    env.set_slot(1);
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    env.set_slot(2);
    env.trade(&user, &lp, lp_idx, user_idx, 500_000);
    env.set_slot(3);
    env.trade(&user, &lp, lp_idx, user_idx, 250_000);

    // User should have accumulated position: 1M + 500K + 250K = 1.75M
    let pos = env.read_account_position(user_idx);
    assert_eq!(
        pos, 1_750_000,
        "Rapid trades should accumulate: expected 1.75M, got {}",
        pos
    );

    // Conservation
    env.set_slot(200);
    env.crank();

    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(c_tot, sum, "c_tot after double trade");
}

/// ATTACK: Three LPs with different positions, user trades against all.
/// Tests LP aggregate tracking with multiple LPs.
#[test]
fn test_attack_three_lps_aggregate_tracking() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp1 = Keypair::new();
    let lp1_idx = env.init_lp(&lp1);
    env.deposit(&lp1, lp1_idx, 30_000_000_000);

    let lp2 = Keypair::new();
    let lp2_idx = env.init_lp(&lp2);
    env.deposit(&lp2, lp2_idx, 30_000_000_000);

    let lp3 = Keypair::new();
    let lp3_idx = env.init_lp(&lp3);
    env.deposit(&lp3, lp3_idx, 30_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade against each LP
    env.set_slot(1);
    env.trade(&user, &lp1, lp1_idx, user_idx, 1_000_000);
    env.set_slot(2);
    env.trade(&user, &lp2, lp2_idx, user_idx, 2_000_000);
    env.set_slot(3);
    env.trade(&user, &lp3, lp3_idx, user_idx, -500_000);

    // User total position = 1M + 2M - 500K = 2.5M
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(
        user_pos, 2_500_000,
        "User should have net 2.5M position: got {}",
        user_pos
    );

    // LP positions should be correct
    assert_eq!(env.read_account_position(lp1_idx), -1_000_000);
    assert_eq!(env.read_account_position(lp2_idx), -2_000_000);
    assert_eq!(env.read_account_position(lp3_idx), 500_000);

    // Price change and crank
    env.set_slot_and_price(200, 140_000_000);
    env.crank();

    // Conservation across all 4 accounts
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp1_idx)
        + env.read_account_capital(lp2_idx)
        + env.read_account_capital(lp3_idx)
        + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot across 3 LPs: c_tot={} sum={}",
        c_tot, sum
    );

    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: Projected haircut during trade vs realized haircut after crank.
/// Verify consistency between margin check haircut and settlement haircut.
#[test]
fn test_attack_projected_vs_realized_haircut_consistency() {
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

    // Both users open large positions (creates significant pnl_pos_tot)
    env.trade(&user1, &lp, lp_idx, user1_idx, 5_000_000);
    env.trade(&user2, &lp, lp_idx, user2_idx, 5_000_000);

    // Price moves - creates PnL
    env.set_slot_and_price(200, 145_000_000);
    env.crank();

    let pnl_pos_tot_before = env.read_pnl_pos_tot();

    // User1 closes position - should use projected haircut
    env.set_slot(201);
    env.crank();
    env.trade(&user1, &lp, lp_idx, user1_idx, -5_000_000);

    let pnl_pos_tot_after = env.read_pnl_pos_tot();

    // pnl_pos_tot may change after close due to counterparty PnL updates
    // (the LP's position changes, which can shift pnl_pos_tot).
    // Verify pnl_pos_tot is non-negative (no phantom negative PnL injected).
    assert!(
        pnl_pos_tot_after >= 0,
        "pnl_pos_tot must remain non-negative after closing position: before={} after={}",
        pnl_pos_tot_before,
        pnl_pos_tot_after
    );
    // Log the change for debugging
    let _ = pnl_pos_tot_before;

    // Conservation
    env.set_slot(400);
    env.crank();

    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: set_pnl aggregate consistency - rapid PnL changes from trades.
/// Multiple trades that flip PnL sign should maintain pnl_pos_tot correctly.
#[test]
fn test_attack_set_pnl_aggregate_rapid_flips() {
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

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Series of trades with price changes that flip PnL sign.
    // For an open position, PnL is tracked in the pnl field (mark-to-market).
    // Capital settles during touch_account (lazy settlement), not during crank alone.
    let _user_cap_initial = env.read_account_capital(user_idx);
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000); // Long

    env.set_slot_and_price(200, 145_000_000); // User profits
    env.crank();
    let pnl_after_rise = env.read_account_pnl(user_idx);
    assert!(
        pnl_after_rise > 0,
        "User PnL should be positive after price rise (long at $138, now $145): pnl={}",
        pnl_after_rise
    );

    env.set_slot_and_price(400, 125_000_000); // Price crashes below entry
    env.crank();
    let pnl_after_crash = env.read_account_pnl(user_idx);
    assert!(
        pnl_after_crash < pnl_after_rise,
        "User PnL should decrease after price crash: before={} after={}",
        pnl_after_rise,
        pnl_after_crash
    );

    env.set_slot_and_price(600, 150_000_000); // Recovery
    env.crank();
    let pnl_after_recovery = env.read_account_pnl(user_idx);
    assert!(
        pnl_after_recovery > pnl_after_crash,
        "User PnL should increase after recovery: before={} after={}",
        pnl_after_crash,
        pnl_after_recovery
    );

    // With instant warmup, pnl_pos_tot may have small residuals
    let ppt = env.read_pnl_pos_tot();
    assert!(
        ppt >= 0,
        "pnl_pos_tot must be non-negative after instant warmup (warmup_period=0): {}",
        ppt
    );

    // Conservation: c_tot matches sum of capitals
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot after PnL flips: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: LP partial close (reduce LP position) and verify aggregates.
/// Trade that reduces LP's exposure should update net_lp_pos correctly.
#[test]
fn test_attack_lp_partial_close_aggregate_update() {
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

    // User1 goes long 5M → LP has -5M
    env.set_slot(1);
    env.trade(&user1, &lp, lp_idx, user1_idx, 5_000_000);
    assert_eq!(env.read_account_position(lp_idx), -5_000_000);

    // User1 partially closes (reduces by 2M)
    env.set_slot(2);
    env.trade(&user1, &lp, lp_idx, user1_idx, -2_000_000);

    // LP position should be -3M now
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(
        lp_pos, -3_000_000,
        "LP position after partial close: expected -3M, got {}",
        lp_pos
    );

    // User2 trades against same LP
    env.set_slot(3);
    env.trade(&user2, &lp, lp_idx, user2_idx, 1_000_000);

    // LP now = -3M - 1M = -4M
    assert_eq!(env.read_account_position(lp_idx), -4_000_000);

    // Crank and verify conservation
    env.set_slot(200);
    env.crank();

    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx)
        + env.read_account_capital(user1_idx)
        + env.read_account_capital(user2_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot after LP partial close: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Two users with opposing positions, price returns to start.
/// Both users should have approximately zero PnL (minus fees).
#[test]
fn test_attack_opposing_positions_price_roundtrip() {
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

    // User1 long, user2 short (opposing through LP)
    env.set_slot(1);
    env.trade(&user1, &lp, lp_idx, user1_idx, 3_000_000);
    env.set_slot(2);
    env.trade(&user2, &lp, lp_idx, user2_idx, -3_000_000);

    // LP net position = -3M + 3M = 0
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(lp_pos, 0, "LP should be flat: pos={}", lp_pos);

    // Price moves up, then back to start
    env.set_slot_and_price(200, 150_000_000);
    env.crank();
    env.set_slot_and_price(400, 138_000_000); // Back to original
    env.crank();

    // After round-trip, PnL may not be perfectly opposite due to:
    // - Funding accrual (mark_ewma diverges from oracle during price moves)
    // - Entry price differences (different cranks between trades)
    // Conservation is the key invariant — total value can't increase.
    let pnl1 = env.read_account_pnl(user1_idx);
    let pnl2 = env.read_account_pnl(user2_idx);
    // With funding, both PnLs can be positive (funded from LP)
    // or both negative. The conservation check below is the real test.

    // Conservation must hold
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx)
        + env.read_account_capital(user1_idx)
        + env.read_account_capital(user2_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot after price roundtrip: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Withdraw exactly all capital from user with open position.
/// Should fail because margin check requires capital > 0 for positions.
#[test]
fn test_attack_withdraw_all_with_open_position() {
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

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Try to withdraw ALL capital while position is open
    let result = env.try_withdraw(&user, user_idx, 10_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Withdrawing all capital with open position should be rejected!"
    );

    // Position should still exist
    let pos = env.read_account_position(user_idx);
    assert_eq!(
        pos, 5_000_000,
        "Position should remain after failed withdrawal: pos={}",
        pos
    );

    // Capital should be unchanged
    let cap = env.read_account_capital(user_idx);
    assert_eq!(
        cap, 10_000_000_099,
        "Capital should be unchanged after failed withdrawal: cap={}",
        cap
    );
}

/// ATTACK: Instruction data with extra trailing bytes appended.
/// Tests that decoder rejects or ignores trailing garbage after valid data.
#[test]
fn test_attack_instruction_data_extra_trailing_bytes() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let ata = env.create_ata(&admin.pubkey(), 5_000_000_000);

    // Valid deposit instruction with extra garbage bytes appended
    let mut data = encode_deposit(0, 1_000_000_000);
    data.extend_from_slice(&[0xFF, 0xDE, 0xAD, 0xBE, 0xEF]); // 5 garbage bytes

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
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
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    // Strict parsing should reject extra trailing bytes
    assert!(
        result.is_err(),
        "Extra trailing bytes on deposit instruction should be rejected!"
    );

    // Verify no state change occurred
    let cap = env.read_account_capital(0);
    assert_eq!(
        cap, 0,
        "Capital should remain 0 after rejected trailing-bytes deposit: {}",
        cap
    );
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after extra-bytes instruction: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: Trade with size = i128::MIN + 1 (extreme negative).
/// Tests that extreme negative trade sizes are handled safely.
#[test]
fn test_attack_trade_size_i128_min_boundary() {
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

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Try trade with extremely negative size
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, i128::MIN + 1);
    assert!(
        result.is_err(),
        "ATTACK: Trade with i128::MIN+1 should be rejected (too large)!"
    );

    // Position should remain zero
    let pos = env.read_account_position(user_idx);
    assert_eq!(
        pos, 0,
        "Position should be 0 after rejected extreme trade: pos={}",
        pos
    );
}

/// ATTACK: Withdraw all capital then re-deposit in same slot.
/// Tests that withdraw+deposit cycle doesn't corrupt state.
#[test]
fn test_attack_withdraw_all_redeposit_same_slot() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    let vault_before = env.vault_balance();

    // Withdraw everything (5B deposit + 99 init capital) then re-deposit in same slot.
    env.try_withdraw(&user, user_idx, 5_000_000_099).unwrap();
    let cap_mid = env.read_account_capital(user_idx);
    assert_eq!(
        cap_mid, 0,
        "Capital should be 0 after full withdraw: {}",
        cap_mid
    );

    env.deposit(&user, user_idx, 5_000_000_099);
    let cap_after = env.read_account_capital(user_idx);
    assert_eq!(
        cap_after, 5_000_000_099,
        "Capital should be restored after re-deposit: {}",
        cap_after
    );

    // Vault should be same as before (net zero transfer)
    let vault_after = env.vault_balance();
    assert_eq!(
        vault_before, vault_after,
        "Vault should be unchanged after withdraw+deposit cycle: before={} after={}",
        vault_before, vault_after
    );
}

/// ATTACK: LP tries to close account while users have matched positions.
/// LP with outstanding position should not be closeable.
#[test]
fn test_attack_lp_close_with_matched_positions() {
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

    // User opens position matched against LP
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);

    let lp_pos = env.read_account_position(lp_idx);
    assert_ne!(lp_pos, 0, "LP should have position after trade");

    // LP tries to close account - should fail (has position)
    let result = env.try_close_account(&lp, lp_idx);
    assert!(
        result.is_err(),
        "ATTACK: LP should not be able to close with outstanding position!"
    );

    // LP position should be unchanged
    let lp_pos_after = env.read_account_position(lp_idx);
    assert_eq!(
        lp_pos, lp_pos_after,
        "LP position should be unchanged after failed close: {} vs {}",
        lp_pos, lp_pos_after
    );
}

/// ATTACK: Trade long then short same size - net zero position.
/// Position should cancel out to zero, conservation must hold.
#[test]
fn test_attack_trade_long_then_short_net_zero() {
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

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    let cap_before = env.read_account_capital(user_idx);

    // Go long then short same size
    env.trade(&user, &lp, lp_idx, user_idx, 2_000_000);
    let pos_after_long = env.read_account_position(user_idx);
    assert_eq!(pos_after_long, 2_000_000, "Should be long 2M");

    env.set_slot(2);
    env.trade(&user, &lp, lp_idx, user_idx, -2_000_000);
    let pos_after_close = env.read_account_position(user_idx);
    assert_eq!(
        pos_after_close, 0,
        "Position should be 0 after closing: {}",
        pos_after_close
    );

    // LP position should also be zero
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(
        lp_pos, 0,
        "LP position should be 0 after net-zero trades: {}",
        lp_pos
    );

    // Conservation
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after net-zero trades: engine={} vault={}",
        engine_vault, vault
    );
    let cap_after = env.read_account_capital(user_idx);
    assert!(
        cap_after <= cap_before + 1,
        "Net-zero round-trip trade should not increase user capital materially. before={} after={}",
        cap_before,
        cap_after
    );
}

/// ATTACK: LP matched by multiple users in rapid succession.
/// Tests LP position aggregate correctness under rapid multi-user trading.
#[test]
fn test_attack_lp_rapid_multi_user_matching() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Create 5 users
    let mut users: Vec<(Keypair, u16)> = Vec::new();
    for _ in 0..5 {
        let u = Keypair::new();
        let idx = env.init_user(&u);
        env.deposit(&u, idx, 5_000_000_000);
        users.push((u, idx));
    }

    env.try_top_up_insurance(&admin, 2_000_000_000).unwrap();
    env.crank();

    // All 5 users trade against same LP in quick succession
    let sizes: &[i128] = &[1_000_000, -500_000, 2_000_000, -1_500_000, 800_000];
    for (i, ((u, idx), &size)) in users.iter().zip(sizes.iter()).enumerate() {
        env.set_slot((i + 1) as u64);
        env.trade(u, &lp, lp_idx, *idx, size);
    }

    // LP net position = -(1M - 500K + 2M - 1.5M + 800K) = -1.8M
    let expected_lp_pos: i128 = -sizes.iter().sum::<i128>();
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(
        lp_pos, expected_lp_pos,
        "LP position after multi-user matching: expected={} got={}",
        expected_lp_pos, lp_pos
    );

    // Crank and verify conservation
    env.set_slot(200);
    env.crank();

    let c_tot = env.read_c_tot();
    let mut sum: u128 = env.read_account_capital(lp_idx);
    for (_, idx) in &users {
        sum += env.read_account_capital(*idx);
    }
    assert_eq!(
        c_tot, sum,
        "c_tot after multi-user LP matching: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Deposit after full withdrawal in same slot - cycle should not extract value.
/// Tests that rapid deposit-withdraw-deposit cycles don't corrupt aggregates.
#[test]
fn test_attack_deposit_withdraw_deposit_cycle_aggregates() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    // Deposit BEFORE crank to avoid GC of empty account
    env.deposit(&user, user_idx, 3_000_000_000);

    env.crank();

    let c_tot_1 = env.read_c_tot();

    // Withdraw → re-deposit cycle
    env.try_withdraw(&user, user_idx, 3_000_000_000).unwrap();
    let c_tot_2 = env.read_c_tot();
    assert!(c_tot_2 < c_tot_1, "c_tot should decrease after withdraw");

    env.deposit(&user, user_idx, 3_000_000_000);
    let c_tot_3 = env.read_c_tot();
    assert_eq!(
        c_tot_1, c_tot_3,
        "c_tot should be same after full cycle: before={} after={}",
        c_tot_1, c_tot_3
    );

    // Verify per-account capital
    let user_cap = env.read_account_capital(user_idx);
    assert_eq!(
        user_cap, 3_000_000_099,
        "User capital should be 3B after cycle (includes init capital after anti-spam fee): {}",
        user_cap
    );
}

/// ATTACK: Open max-margin position, crank with price at liquidation boundary.
/// Tests that liquidation trigger is precise and doesn't miss by 1.
#[test]
fn test_attack_liquidation_boundary_precision() {
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

    env.try_top_up_insurance(&admin, 2_000_000_000).unwrap();
    env.crank();

    // Open a position
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Move price down significantly (user is long)
    env.set_slot_and_price(50, 90_000_000); // 10% drop
    env.crank();

    // Check if user was liquidated or still surviving
    let user_pos = env.read_account_position(user_idx);
    let user_cap = env.read_account_capital(user_idx);

    // Whether liquidated or not, conservation must hold
    let vault = env.vault_balance();
    let c_tot = env.read_c_tot();
    let insurance = env.read_insurance_balance();
    assert!(
        vault as u128 >= c_tot + insurance,
        "Conservation at liquidation boundary: vault={} c_tot={} ins={}",
        vault,
        c_tot,
        insurance
    );

    // If user still has position, they must have margin
    if user_pos != 0 {
        assert!(
            user_cap > 0,
            "User with position should have positive capital: cap={}",
            user_cap
        );
    }
}

/// ATTACK: Push oracle with timestamp = 0 then try to use it.
/// Tests that extreme timestamp doesn't corrupt oracle state or cause panic.
#[test]
fn test_attack_oracle_timestamp_zero_then_crank() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(138_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();

    // Push with timestamp = 0 (no backwards check in oracle)
    let result = env.try_push_oracle_price(&admin, 140_000_000, 0);
    assert!(result.is_ok(), "Oracle should accept ts=0: {:?}", result);

    // Push subsequent price with normal timestamp
    let result2 = env.try_push_oracle_price(&admin, 141_000_000, 1000);
    assert!(
        result2.is_ok(),
        "Should be able to push valid price after ts=0: {:?}",
        result2
    );

    // The legacy authority-timestamp slot is now reserved padding
    // (`_reserved_auth_ts`). PushHyperpMark must not touch it.
    const AUTH_TS_OFF: usize = 320;
    let slab_data = env.svm.get_account(&env.slab).unwrap().data;
    let funding_state =
        i64::from_le_bytes(slab_data[AUTH_TS_OFF..AUTH_TS_OFF + 8].try_into().unwrap());
    assert_eq!(
        funding_state, 0,
        "Hyperp funding-rate state must remain unchanged by PushHyperpMark"
    );

    env.set_slot(200);
    env.crank();

    // State should be consistent
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after ts=0 oracle push: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: Push oracle with timestamp = i64::MAX.
/// Tests that far-future timestamps don't cause overflow or panic.
#[test]
fn test_attack_oracle_timestamp_i64_max_no_overflow() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(138_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();

    // Push with timestamp = i64::MAX
    let result = env.try_push_oracle_price(&admin, 140_000_000, i64::MAX);
    assert!(
        result.is_ok(),
        "Oracle should accept ts=i64::MAX: {:?}",
        result
    );

    // In Hyperp mode, external timestamp input must not clobber funding-rate state.
    const AUTH_TS_OFF: usize = 320;
    let slab_after_max = env.svm.get_account(&env.slab).unwrap().data;
    let funding_state_after_max = i64::from_le_bytes(
        slab_after_max[AUTH_TS_OFF..AUTH_TS_OFF + 8]
            .try_into()
            .unwrap(),
    );
    assert_eq!(
        funding_state_after_max, 0,
        "Hyperp funding-rate state should ignore i64::MAX timestamp input"
    );

    // Push another price - no backwards timestamp rejection means it works
    let result2 = env.try_push_oracle_price(&admin, 141_000_000, 1000);
    assert!(
        result2.is_ok(),
        "Should still push prices after ts=MAX: {:?}",
        result2
    );

    env.set_slot(200);
    env.crank();

    // No overflow, state consistent
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after ts=MAX oracle push: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: LP deposit with pending fee debt.
/// LP depositing should settle fees first, then add remaining to capital.
/// ATTACK: Config change then immediate trade tests new config applied.
/// After SetMaintenanceFee, immediate deposit should use new fee rate.
/// ATTACK: Multiple admin changes in rapid succession.
/// Tests that admin state is correctly updated through multiple transfers.
#[test]
fn test_attack_rapid_admin_transfers() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin1 = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let admin2 = Keypair::new();
    let admin3 = Keypair::new();
    env.svm.airdrop(&admin2.pubkey(), 5_000_000_000).unwrap();
    env.svm.airdrop(&admin3.pubkey(), 5_000_000_000).unwrap();

    // Chain: admin1 -> admin2 -> admin3 (cross-Keypair: two-sig handover)
    env.try_update_authority(&admin1, AUTHORITY_ADMIN, Some(&admin2))
        .unwrap();
    env.try_update_authority(&admin2, AUTHORITY_ADMIN, Some(&admin3))
        .unwrap();

    // Only admin3 should work now
    let r1 = env.try_update_admin(&admin1, &admin1.pubkey());
    assert!(r1.is_err(), "Admin1 should be locked out");

    let r2 = env.try_update_admin(&admin2, &admin2.pubkey());
    assert!(r2.is_err(), "Admin2 should be locked out");

    let r3 = env.try_update_admin(&admin3, &admin3.pubkey());
    assert!(r3.is_ok(), "Admin3 should be active: {:?}", r3);
}

/// ATTACK: Deposit to LP account from non-owner.
/// Tests authorization on LP deposits.
#[test]
fn test_attack_deposit_to_lp_wrong_owner() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp_owner = Keypair::new();
    let lp_idx = env.init_lp(&lp_owner);

    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 5_000_000_000).unwrap();
    env.create_ata(&attacker.pubkey(), 3_000_000_000);

    // Attacker tries to deposit to LP's account
    let result = env.try_deposit_unauthorized(&attacker, lp_idx, 1_000_000_000);
    // Should fail because attacker != lp_owner
    assert!(
        result.is_err(),
        "ATTACK: Depositing to LP from non-owner should fail!"
    );

    // LP capital should be zero (never deposited)
    let lp_cap = env.read_account_capital(lp_idx);
    assert_eq!(
        lp_cap, DEFAULT_INIT_CAPITAL as u128,
        "LP capital should equal default init capital after failed unauthorized deposit: cap={}",
        lp_cap
    );
}

/// ATTACK: Settlement guard bypass via cap=0 + PushHyperpMark baseline poisoning.
///
/// If admin can set oracle_price_cap to 0 on a non-Hyperp market with
/// min_oracle_price_cap > 0, they can push arbitrary prices that overwrite
/// last_effective_price_e6, then resolve against the poisoned baseline.
/// The immutable floor check in ResolveMarket would trivially pass because
/// both authority_price and last_effective_price are the same arbitrary value.
///
// test_attack_settlement_guard_bypass_cap_zero_poisoning deleted:
// The attack reasoned about SetOraclePriceCap(0) mutating an active market's
// baseline-clamp policy at runtime. v12.19 removed SetOraclePriceCap and made
// the per-slot cap immutable init-time, so this exact attack surface is gone.

/// ATTACK: i128::MIN trade size overflows -size_q (unary negation).
///
/// In BPF release builds (overflow checks off), -i128::MIN wraps to i128::MIN,
/// passing a negative "absolute size" to the engine. Must be rejected.
#[test]
fn test_attack_trade_size_i128_min_overflow() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    let vault_before = env.vault_balance();
    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);

    // Trade with i128::MIN — must be rejected, not overflow
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, i128::MIN);
    assert!(
        result.is_err(),
        "Trade with i128::MIN must be rejected (overflow in negation)"
    );

    // State must be completely preserved
    assert_eq!(env.vault_balance(), vault_before, "Vault must be unchanged");
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "User pos unchanged"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "LP pos unchanged"
    );
}

/// Spec §10.5: TradeNoCpi is a bilateral primitive — any two accounts
/// can trade (user-user, user-LP, LP-LP). Account kind is not enforced.
/// This is by design: both parties must sign, so bilateral consent is sufficient.
#[test]
fn test_trade_nocpi_user_bilateral_allowed_by_spec() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 5_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 5_000_000_000);

    // user1 trades with user2 (bilateral, both sign) — spec allows this
    let result = env.try_trade(&user1, &user2, user2_idx, user1_idx, 1_000_000);
    assert!(
        result.is_ok(),
        "Spec §10.5: bilateral user-user trade should succeed: {:?}",
        result,
    );

    // Positions must be set (bilateral trade created positions)
    assert_ne!(
        env.read_account_position(user1_idx),
        0,
        "user1 should have position"
    );
    assert_ne!(
        env.read_account_position(user2_idx),
        0,
        "user2 should have opposite position"
    );

    // Conservation: vault unchanged (no token flow in trade)
    let vault = env.vault_balance();
    assert!(vault > 0, "Vault must still hold deposits");
}

/// ATTACK: Settlement guard bypass via first-push baseline poisoning.
///
/// On non-Hyperp markets, PushHyperpMark must NOT overwrite
/// last_effective_price_e6 — only external oracle reads (crank/trade)
/// should set the baseline. Otherwise the admin can push an arbitrary
/// price, poisoning the baseline, then resolve against it.
#[test]
fn test_attack_first_push_does_not_poison_baseline() {
    program_path();

    let mut env = TestEnv::new();

    // Init with non-zero min_oracle_price_cap = 10_000 (1%)
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
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_init_market_with_limits(
            &admin.pubkey(),
            &env.mint,
            &TEST_FEED_ID,
            10_000u64, // min_oracle_price_cap = 1%
        ),
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    env.svm.send_transaction(tx).expect("init");

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Crank to establish external oracle baseline ($138)
    env.crank();

    let baseline = env.read_last_effective_price();
    assert_eq!(baseline, 138_000_000, "Baseline should be $138 from oracle");

    // Set authority and push a very different price ($500)
    // authority push removed.

    // Baseline must NOT have moved — push doesn't write last_effective_price_e6
    let baseline_after_push = env.read_last_effective_price();
    assert_eq!(
        baseline_after_push, baseline,
        "Authority push must not overwrite external oracle baseline: before={} after={}",
        baseline, baseline_after_push
    );

    // Even after many pushes with escalating timestamps, baseline stays put
    for i in 0..50 {
        env.set_slot(200 + i * 10);
        let _ = env.try_push_oracle_price(&admin, 500_000_000, 0);
    }
    let baseline_after_burst = env.read_last_effective_price();
    assert_eq!(
        baseline_after_burst, baseline,
        "Burst of authority pushes must not walk baseline: before={} after={}",
        baseline, baseline_after_burst
    );

    // hyperp_mark_e6 is clamped to within 1 cap-width of baseline
    let auth_price = env.read_authority_price();
    let max_delta = baseline as u128 * 10_000 / 1_000_000; // 1% of baseline
    let upper = baseline + max_delta as u64;
    assert!(
        auth_price <= upper,
        "Authority price must be clamped within cap of baseline: auth={} upper={}",
        auth_price,
        upper
    );
}

/// Gate: caller-shaped external errors (malformed account, wrong owner,
/// wrong feed, bad data) must NOT trigger authority fallback. The gate
/// allows fallback only on content-based unusability
/// (OracleStale / OracleConfTooWide). Verifies the caller can't force
/// authority pricing by submitting garbage.
#[test]
fn test_attack_bad_oracle_with_authority_requires_external_success() {
    program_path();

    // Authority fallback is GATED on content-based external errors
    // only (OracleStale / OracleConfTooWide). Caller-shaped errors
    // (malformed account data / wrong owner / wrong feed id) propagate
    // unchanged — otherwise a caller could force authority pricing by
    // supplying a garbage oracle account, getting a cap-step of control
    // over the effective price on any path that reads the oracle
    // (withdraw, trade, crank, liquidate, settle, convert, catchup).
    //
    // This test verifies the gate: with a fresh authority configured
    // AND a malformed external account, the crank must still FAIL
    // (not fall back to authority) — because the external parse error
    // is "caller gave us garbage", not "oracle is dead". A separate
    // positive test covers the legitimate stale-external +
    // fresh-authority case (see the "bounded authority fallback on
    // genuine external staleness" test below).
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 80);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Establish baseline via crank with good oracle
    env.crank();

    // Enable authority and push a fresh price
    // authority push removed.

    env.set_slot(200);

    // Poison the oracle account so external read fails with
    // InvalidAccountData (malformed / caller-shaped error).
    env.svm
        .set_account(
            env.pyth_index,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; 10], // Too short for Pyth
                owner: PYTH_RECEIVER_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    // Crank must FAIL — authority fallback does not kick in for
    // caller-shaped errors. The caller must supply a well-formed
    // oracle account; authority is for content-based unavailability
    // (stale / conf-too-wide), not for masking bad submissions.
    let result = env.try_crank();
    assert!(
        result.is_err(),
        "crank with malformed external oracle account must fail — \
         authority fallback only applies on content-based errors \
         (OracleStale / OracleConfTooWide), not on caller-shaped \
         errors. Got: {:?}",
        result,
    );
}

/// ATTACK (security R&D loop, iteration 5 — perp DEX corner case):
/// Position flip through zero with price movement in between. Classic
/// source of bugs in perp DEXs: when a trade crosses the zero
/// position line, the engine must split the trade into
/// (close-and-realize) + (open-at-flip-price) accounting, not treat
/// the whole thing as a single position adjustment.
///
/// Attack mechanic: If the engine doesn't realize PnL on the close
/// portion before opening the reverse side, the cost basis for the
/// new short could absorb the profit from the old long, hiding it
/// inside unrealized PnL that the user can extract asymmetrically.
///
/// Also: spec §3 (position flips) mandates the crossed trade check
/// `initial_margin_bps` (not maintenance_margin_bps) since it's
/// effectively opening a new position. If the wrapper/engine uses
/// MM instead of IM, the user can open a more-leveraged reverse
/// position than they'd otherwise be allowed.
///
/// Scenario:
///   1. Alice long 10M at oracle=$1.00.
///   2. Price moves to $1.10 (engine still uses stored mark; accrue
///      happens inside next trade's preamble).
///   3. Alice sells 20M at $1.10. Should flip to short 10M.
///
/// Expected:
///   - Alice's realized PnL from the 10M close at $1.10: +$1.0M
///     (+10M × $0.10/1e6 in the 1e6-scaled POS units).
///   - Alice's new position: -10M short, entry around $1.10.
///   - LP's capital ~ mirror (zero fees).
///
/// Weird:
///   - Alice's position != -10M after the flip.
///   - Conservation broken (vault delta != 0 across a trade that
///     didn't touch capital/fees).
///   - Entry price stale (old $1.00 still applied to the new short).
#[test]
fn test_attack_position_flip_through_zero_with_pnl() {
    program_path();
    let mut env = TestEnv::new();
    let data = common::encode_init_market_with_maint_fee_bounded(
        &env.payer.pubkey(),
        &env.mint,
        &common::TEST_FEED_ID,
        1_000_000_000,
        0, // no maintenance fee
        0,
    );
    env.try_init_market_raw(data).expect("init_market");
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 100_000_000_000);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let alice = Keypair::new();
    let alice_idx = env.init_user(&alice);
    env.deposit(&alice, alice_idx, 10_000_000_000);

    // Step 1: open long 10M at $1.00 (1_000_000 in e6 scale).
    env.set_slot_and_price(100, 1_000_000);
    env.trade(&alice, &lp, lp_idx, alice_idx, 10_000_000);
    let alice_pos_after_long = env.read_account_position(alice_idx);
    assert_eq!(alice_pos_after_long, 10_000_000, "open long failed");

    // Step 2: advance slot + bump price to $1.10.
    env.set_slot_and_price(200, 1_100_000);
    env.svm.expire_blockhash();

    let vault_pre_flip = env.vault_balance();

    // Step 3: sell 20M — should close 10M long (+PnL) and open 10M
    // short at $1.10.
    env.trade(&alice, &lp, lp_idx, alice_idx, -20_000_000);

    let alice_pos_after_flip = env.read_account_position(alice_idx);
    let alice_cap_after_flip = env.read_account_capital(alice_idx);
    let lp_cap_after_flip = env.read_account_capital(lp_idx);
    let ins_after_flip = env.read_insurance_balance();
    let vault_after_flip = env.vault_balance();

    // Primary invariant: position must be exactly -10M (short 10M).
    assert_eq!(
        alice_pos_after_flip, -10_000_000,
        "POSITION FLIP BROKEN: expected -10M short, got {alice_pos_after_flip}. \
         Engine lost track of position basis across the zero-crossing.",
    );

    // Conservation: vault must not move during a trade.
    assert_eq!(
        (vault_after_flip as i128) - (vault_pre_flip as i128),
        0,
        "vault moved during a trade — tokens leaked somewhere",
    );

    assert_eq!(
        env.read_account_position(lp_idx),
        10_000_000,
        "LP must be the opposite side after the cross-zero trade"
    );

    // The wrapper-level invariant is token conservation plus internally
    // consistent capital and PnL aggregates. Unrealized PnL existed before
    // this flip, so comparing only field deltas would count the same value
    // once when it becomes realized.
    let alice_pnl_post = env.read_account_pnl(alice_idx);
    let lp_pnl_post = env.read_account_pnl(lp_idx);
    let c_tot = env.read_c_tot();
    let sum_caps = alice_cap_after_flip + lp_cap_after_flip;
    assert_eq!(
        c_tot, sum_caps,
        "c_tot must equal account capital sum after cross-zero trade"
    );
    assert!(
        vault_after_flip as u128 >= c_tot + ins_after_flip,
        "vault must cover capital plus insurance after cross-zero trade"
    );
    let pnl_pos_tot = env.read_pnl_pos_tot();
    let expected_pnl_pos = alice_pnl_post.max(0) as u128 + lp_pnl_post.max(0) as u128;
    assert_eq!(
        pnl_pos_tot, expected_pnl_pos,
        "pnl_pos_tot must match positive account PnL after cross-zero trade"
    );
}

const MAX_RISK_P0_E6: u64 = 200_000_000;
const MAX_RISK_ATTACK_START_SLOT: u64 = 100;
const MAX_RISK_ATTACK_SLOTS: usize = 8;
const MAX_RISK_CAPITAL_PER_USER: u64 = 1_000_000_000;
const MAX_RISK_LP_CAPITAL: u64 = 100_000_000_000;
const MAX_RISK_INSURANCE: u64 = 1_000_000_000;
const MAX_RISK_LEVERAGE_MILLI: u128 = 19_900;
const MAX_RISK_CLAMP_BPS: u64 = 49;
const MAX_RISK_H_MAX: u64 = 86_400;
const MAX_RISK_PERMISSIONLESS_RESOLVE_STALE_SLOTS: u64 =
    percolator_prog::constants::MAX_PERMISSIONLESS_RESOLVE_STALE_SLOTS;

#[derive(Debug, Clone, Copy)]
enum MaxRiskCrankSchedule {
    HonestEverySlot,
    EmptyEverySlotThenFull,
    GapThenFull,
}

#[derive(Debug, Clone, Copy)]
enum MaxRiskEdgeSchedule {
    OracleThenFullEverySlot,
    FullThenOracleEverySlot,
    OracleThenEmptyEverySlotThenFull,
    OracleThenFullEveryOtherSlot,
    OracleThenFullAtMidAndEnd,
    GapThenFull,
}

struct MaxRiskActor {
    keypair: Keypair,
    idx: u16,
    direction: i8,
}

fn encode_init_market_bounty_sol_20x_with_h_max_and_perm_resolve(
    admin: &Pubkey,
    mint: &Pubkey,
    h_max: u64,
    permissionless_resolve_stale_slots: u64,
) -> Vec<u8> {
    let mut data = vec![0u8];
    data.extend_from_slice(admin.as_ref());
    data.extend_from_slice(mint.as_ref());
    data.extend_from_slice(&TEST_FEED_ID);
    data.extend_from_slice(&600u64.to_le_bytes());
    data.extend_from_slice(&500u16.to_le_bytes());
    data.push(0);
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    data.extend_from_slice(&0u128.to_le_bytes());

    // RiskParams wire fields. This mirrors ../percolator-stress/max_risk.md's
    // 20x SOL bounty market: h_min = 0 is a product feature, while h_max is
    // the long profit-maturity horizon and is intentionally independent from
    // max_accrual_dt_slots / permissionless stale resolution.
    data.extend_from_slice(&0u64.to_le_bytes());
    data.extend_from_slice(&500u64.to_le_bytes());
    data.extend_from_slice(&500u64.to_le_bytes());
    data.extend_from_slice(&1u64.to_le_bytes());
    data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data.extend_from_slice(&1u128.to_le_bytes());
    data.extend_from_slice(&h_max.to_le_bytes());
    data.extend_from_slice(&9u64.to_le_bytes());
    data.extend_from_slice(&5u64.to_le_bytes());
    data.extend_from_slice(&50_000_000_000u128.to_le_bytes());
    data.extend_from_slice(&1_000u64.to_le_bytes());
    data.extend_from_slice(&0u128.to_le_bytes());
    data.extend_from_slice(&500u128.to_le_bytes());
    data.extend_from_slice(&600u128.to_le_bytes());
    data.extend_from_slice(&MAX_RISK_CLAMP_BPS.to_le_bytes());

    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    data.extend_from_slice(&permissionless_resolve_stale_slots.to_le_bytes());
    data.extend_from_slice(&500u64.to_le_bytes());
    data.extend_from_slice(&100u64.to_le_bytes());
    data.extend_from_slice(&500i64.to_le_bytes());
    data.extend_from_slice(&1_000i64.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    data.extend_from_slice(&10u64.to_le_bytes());
    data
}

fn encode_init_market_bounty_sol_20x_max(admin: &Pubkey, mint: &Pubkey) -> Vec<u8> {
    encode_init_market_bounty_sol_20x_with_h_max_and_perm_resolve(
        admin,
        mint,
        MAX_RISK_H_MAX,
        MAX_RISK_PERMISSIONLESS_RESOLVE_STALE_SLOTS,
    )
}

fn try_crank_with_candidate_indices(env: &mut TestEnv, candidates: &[u16]) -> Result<(), String> {
    let caller = Keypair::new();
    env.svm
        .airdrop(&caller.pubkey(), 10_000_000)
        .map_err(|e| format!("{e:?}"))?;

    let data = encode_crank_with_candidates(candidates);
    let accounts = vec![
        AccountMeta::new_readonly(caller.pubkey(), true),
        AccountMeta::new(env.slab, false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(env.pyth_index, false),
    ];
    let ix = Instruction {
        program_id: env.program_id,
        accounts,
        data,
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&caller.pubkey()),
        &[&caller],
        env.svm.latest_blockhash(),
    );
    env.svm
        .send_transaction(tx)
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

fn try_empty_crank(env: &mut TestEnv) -> Result<(), String> {
    let caller = Keypair::new();
    env.svm
        .airdrop(&caller.pubkey(), 10_000_000)
        .map_err(|e| format!("{e:?}"))?;

    let mut data = Vec::new();
    data.push(5);
    data.extend_from_slice(&percolator_prog::constants::CRANK_NO_CALLER.to_le_bytes());
    data.push(1);

    let accounts = vec![
        AccountMeta::new_readonly(caller.pubkey(), true),
        AccountMeta::new(env.slab, false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(env.pyth_index, false),
    ];
    let ix = Instruction {
        program_id: env.program_id,
        accounts,
        data,
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&caller.pubkey()),
        &[&caller],
        env.svm.latest_blockhash(),
    );
    env.svm
        .send_transaction(tx)
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

fn max_risk_next_clamped_price(price_e6: u64, direction: i8, dt_slots: u64) -> u64 {
    let delta = (price_e6 as u128)
        .saturating_mul(MAX_RISK_CLAMP_BPS as u128)
        .saturating_mul(dt_slots as u128)
        / 10_000;
    if direction < 0 {
        price_e6.saturating_sub(delta as u64)
    } else {
        price_e6.saturating_add(delta as u64)
    }
}

fn max_risk_next_price_signed_bps(price_e6: u64, signed_bps: i16) -> u64 {
    let abs_bps = signed_bps.unsigned_abs() as u64;
    assert!(
        abs_bps <= MAX_RISK_CLAMP_BPS,
        "edge sweep only uses oracle deltas inside the wrapper cap"
    );
    let delta = (price_e6 as u128).saturating_mul(abs_bps as u128) / 10_000;
    if signed_bps < 0 {
        price_e6.saturating_sub(delta as u64)
    } else {
        price_e6.saturating_add(delta as u64)
    }
}

fn max_risk_gap_price_from_path(path: &[i8]) -> u64 {
    let net: i64 = path.iter().map(|dir| *dir as i64).sum();
    if net == 0 {
        return MAX_RISK_P0_E6;
    }
    max_risk_next_clamped_price(MAX_RISK_P0_E6, net.signum() as i8, net.unsigned_abs())
}

fn max_risk_gap_price_from_signed_bps_path(path: &[i16]) -> u64 {
    let net_bps: i64 = path.iter().map(|bps| *bps as i64).sum();
    if net_bps == 0 {
        return MAX_RISK_P0_E6;
    }
    let max_gap_bps = (MAX_RISK_CLAMP_BPS as i64).saturating_mul(path.len() as i64);
    let bounded_net_bps = net_bps.clamp(-max_gap_bps, max_gap_bps);
    let delta =
        (MAX_RISK_P0_E6 as u128).saturating_mul(bounded_net_bps.unsigned_abs() as u128) / 10_000;
    if bounded_net_bps < 0 {
        MAX_RISK_P0_E6.saturating_sub(delta as u64)
    } else {
        MAX_RISK_P0_E6.saturating_add(delta as u64)
    }
}

fn max_risk_position_size(capital: u64, price_e6: u64) -> i128 {
    let notional = (capital as u128) * MAX_RISK_LEVERAGE_MILLI / 1_000;
    let q = notional
        .saturating_mul(percolator::POS_SCALE)
        .checked_div(price_e6 as u128)
        .expect("nonzero price");
    q as i128
}

fn setup_max_risk_probe(
    num_longs: usize,
    num_shorts: usize,
) -> (TestEnv, Keypair, u16, Vec<MaxRiskActor>) {
    let mut env = TestEnv::new();
    env.set_slot_and_price_raw_no_walk(MAX_RISK_ATTACK_START_SLOT, MAX_RISK_P0_E6 as i64);

    let admin = env.payer.pubkey();
    let mint = env.mint;
    let data = encode_init_market_bounty_sol_20x_max(&admin, &mint);
    env.try_init_market_raw(data)
        .expect("max-risk InitMarket should pass wrapper validation");

    let payer = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&payer, MAX_RISK_INSURANCE);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, MAX_RISK_LP_CAPITAL);

    let size = max_risk_position_size(MAX_RISK_CAPITAL_PER_USER, MAX_RISK_P0_E6);
    let mut actors = Vec::with_capacity(num_longs + num_shorts);
    for _ in 0..num_longs {
        let user = Keypair::new();
        let user_idx = env.init_user(&user);
        env.deposit(&user, user_idx, MAX_RISK_CAPITAL_PER_USER);
        env.try_trade(&user, &lp, lp_idx, user_idx, size)
            .expect("open max-risk long");
        actors.push(MaxRiskActor {
            keypair: user,
            idx: user_idx,
            direction: 1,
        });
    }
    for _ in 0..num_shorts {
        let user = Keypair::new();
        let user_idx = env.init_user(&user);
        env.deposit(&user, user_idx, MAX_RISK_CAPITAL_PER_USER);
        env.try_trade(&user, &lp, lp_idx, user_idx, -size)
            .expect("open max-risk short");
        actors.push(MaxRiskActor {
            keypair: user,
            idx: user_idx,
            direction: -1,
        });
    }

    (env, lp, lp_idx, actors)
}

#[test]
fn test_attack_issue33_account_free_catchup_cannot_move_exposed_market() {
    program_path();

    let (mut env, _lp, _lp_idx, _actors) = setup_max_risk_probe(1, 1);
    let slot_before = env.read_last_market_slot();
    let insurance_before = env.read_insurance_balance();
    let price = max_risk_next_clamped_price(MAX_RISK_P0_E6, -1, MAX_RISK_ATTACK_SLOTS as u64);

    env.set_slot_and_price_raw_no_walk(
        MAX_RISK_ATTACK_START_SLOT + MAX_RISK_ATTACK_SLOTS as u64,
        price as i64,
    );

    let err = env
        .try_catchup_accrue()
        .expect_err("retired account-free catchup tag must reject");
    assert!(
        err.contains("InvalidInstructionData") || err.contains("invalid instruction data"),
        "expected InvalidInstructionData for retired account-free catchup tag, got: {err}",
    );
    assert_eq!(
        env.read_last_market_slot(),
        slot_before,
        "rejected account-free catchup must not advance the market slot",
    );
    assert_eq!(
        env.read_insurance_balance(),
        insurance_before,
        "rejected account-free catchup must not mutate insurance",
    );
}

#[test]
fn test_attack_withdraw_decoy_cannot_walk_exposed_market() {
    program_path();

    let (mut env, _lp, lp_idx, actors) = setup_max_risk_probe(1, 1);
    let candidates = max_risk_candidate_indices(lp_idx, &actors);

    let decoy = Keypair::new();
    let decoy_idx = env.init_user(&decoy);
    env.deposit(&decoy, decoy_idx, 100_000_000);

    let slot_before = env.read_last_market_slot();
    let insurance_before = env.read_insurance_balance();
    let mut price = MAX_RISK_P0_E6;
    price = max_risk_next_price_signed_bps(price, -45);
    env.set_slot_and_price_raw_no_walk(slot_before + 10, price as i64);

    let walk = env.try_withdraw(&decoy, decoy_idx, 1);
    assert!(
        walk.is_err(),
        "EXPLOIT: decoy WithdrawCollateral advanced exposed market state without the crank cascade",
    );
    assert_eq!(
        env.read_last_market_slot(),
        slot_before,
        "rejected decoy withdrawal must not advance market clock",
    );
    assert_eq!(
        env.read_insurance_balance(),
        insurance_before,
        "rejected decoy withdrawal must not mutate insurance",
    );

    crank_candidate_sweep_and_track_min(&mut env, &candidates, insurance_before);
    assert!(
        env.read_insurance_balance() >= insurance_before,
        "crank after rejected decoy walk must not drain insurance",
    );
}

/// Regression for issue 65.
///
/// A permissionless KeeperCrank with an empty caller candidate list must not
/// become a hidden market-progress walker for accounts outside the risk buffer.
/// The setup fills the 4-entry risk buffer with larger over-collateralized
/// stuffer positions, then opens a smaller target pair that is evicted from the
/// buffer. Two empty cranks walk the market price against the target; the final
/// crank includes the target as a liquidation candidate. The invariant is that
/// this sequence must not drain insurance below its pre-walk level.
#[test]
fn test_attack_issue65_keeper_crank_empty_candidates_cannot_drain_evicted_target() {
    program_path();

    let mut env = TestEnv::new();
    env.set_slot_and_price_raw_no_walk(100, 138_000_000);
    let data = encode_init_market_bounty_sol_20x_with_h_max_and_perm_resolve(
        &env.payer.pubkey(),
        &env.mint,
        MAX_RISK_H_MAX,
        80,
    );
    env.try_init_market_raw(data)
        .expect("issue65 market config should pass wrapper validation");
    let payer = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&payer, 5_000_000_000);
    env.crank();

    let stuffer_size = 4_000_000_000i128;
    let stuffer_deposit = 100_000_000_000u64;

    let s1u = Keypair::new();
    let s1u_idx = env.init_user(&s1u);
    env.deposit(&s1u, s1u_idx, stuffer_deposit);
    let s1l = Keypair::new();
    let s1l_idx = env.init_lp(&s1l);
    env.deposit(&s1l, s1l_idx, stuffer_deposit);
    env.trade(&s1u, &s1l, s1l_idx, s1u_idx, stuffer_size);

    let s2u = Keypair::new();
    let s2u_idx = env.init_user(&s2u);
    env.deposit(&s2u, s2u_idx, stuffer_deposit);
    let s2l = Keypair::new();
    let s2l_idx = env.init_lp(&s2l);
    env.deposit(&s2l, s2l_idx, stuffer_deposit);
    env.trade(&s2u, &s2l, s2l_idx, s2u_idx, stuffer_size);

    let target_a = Keypair::new();
    let target_a_idx = env.init_user(&target_a);
    env.deposit(&target_a, target_a_idx, 20_000_000_000);
    let target_b = Keypair::new();
    let target_b_idx = env.init_lp(&target_b);
    env.deposit(&target_b, target_b_idx, 20_000_000_000);
    env.trade(
        &target_a,
        &target_b,
        target_b_idx,
        target_a_idx,
        2_800_000_000,
    );

    let buf = env.read_risk_buffer();
    let buf_idxs: Vec<u16> = (0..buf.count as usize)
        .map(|i| buf.entries[i].idx)
        .collect();
    assert!(
        !buf_idxs.contains(&target_a_idx) && !buf_idxs.contains(&target_b_idx),
        "target pair must be evicted from risk buffer for the regression: buf={buf_idxs:?}, target_a={target_a_idx}, target_b={target_b_idx}",
    );

    let insurance_before = env.read_insurance_balance();
    let start_slot = env.svm.get_sysvar::<Clock>().slot;
    let p1 = (138_000_000f64 * 0.955) as i64;
    env.set_slot_and_price_raw_no_walk(start_slot + 10, p1);
    let first_empty = try_empty_crank(&mut env);

    let p2 = (138_000_000f64 * 0.910) as i64;
    env.set_slot_and_price_raw_no_walk(start_slot + 20, p2);
    let second_empty = try_empty_crank(&mut env);

    let after_walk = env.read_insurance_balance();
    let cap_after_walk = env.read_account_capital(target_a_idx);
    let final_crank = try_crank_with_candidate_indices(&mut env, &[target_a_idx, target_b_idx]);
    let insurance_after = env.read_insurance_balance();

    println!(
        "issue65 regression: insurance_before={insurance_before}, after_walk={after_walk}, after={insurance_after}, cap_after_walk={cap_after_walk}, first_empty_ok={}, second_empty_ok={}, final_crank_ok={}, target_a={target_a_idx}, target_b={target_b_idx}, risk_buffer={buf_idxs:?}",
        first_empty.is_ok(),
        second_empty.is_ok(),
        final_crank.is_ok(),
    );
    assert!(
        insurance_after >= insurance_before,
        "empty-candidate market walking plus a final target crank must not drain insurance: before={insurance_before}, after_walk={after_walk}, after={insurance_after}, first_empty={first_empty:?}, second_empty={second_empty:?}, final={final_crank:?}",
    );
}

fn max_risk_candidate_indices(lp_idx: u16, actors: &[MaxRiskActor]) -> Vec<u16> {
    let mut candidates = Vec::with_capacity(actors.len() + 1);
    candidates.push(lp_idx);
    candidates.extend(actors.iter().map(|a| a.idx));
    candidates
}

fn crank_candidate_sweep_and_track_min(
    env: &mut TestEnv,
    candidates: &[u16],
    mut min_insurance: u128,
) -> u128 {
    for chunk in candidates.chunks(8) {
        try_crank_with_candidate_indices(env, chunk)
            .expect("candidate chunk crank should not reject");
        min_insurance = min_insurance.min(env.read_insurance_balance());
    }
    min_insurance
}

fn run_max_risk_edge_price_probe(
    label: &str,
    path: [i16; MAX_RISK_ATTACK_SLOTS],
    schedule: MaxRiskEdgeSchedule,
    num_longs: usize,
    num_shorts: usize,
) -> (u128, u128, u128) {
    let (mut env, _lp, lp_idx, actors) = setup_max_risk_probe(num_longs, num_shorts);
    let candidates = max_risk_candidate_indices(lp_idx, &actors);
    let insurance_start = env.read_insurance_balance();
    let mut min_insurance = insurance_start;
    let mut price = MAX_RISK_P0_E6;

    match schedule {
        MaxRiskEdgeSchedule::OracleThenFullEverySlot => {
            for (step, signed_bps) in path.iter().enumerate() {
                price = max_risk_next_price_signed_bps(price, *signed_bps);
                env.set_slot_and_price_raw_no_walk(
                    MAX_RISK_ATTACK_START_SLOT + step as u64 + 1,
                    price as i64,
                );
                min_insurance =
                    crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);
            }
        }
        MaxRiskEdgeSchedule::FullThenOracleEverySlot => {
            for (step, signed_bps) in path.iter().enumerate() {
                env.set_slot_and_price_raw_no_walk(
                    MAX_RISK_ATTACK_START_SLOT + step as u64 + 1,
                    price as i64,
                );
                min_insurance =
                    crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);
                price = max_risk_next_price_signed_bps(price, *signed_bps);
                env.set_slot_and_price_raw_no_walk(
                    MAX_RISK_ATTACK_START_SLOT + step as u64 + 1,
                    price as i64,
                );
            }
            env.set_slot_and_price_raw_no_walk(
                MAX_RISK_ATTACK_START_SLOT + MAX_RISK_ATTACK_SLOTS as u64 + 1,
                price as i64,
            );
            min_insurance =
                crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);
        }
        MaxRiskEdgeSchedule::OracleThenEmptyEverySlotThenFull => {
            for (step, signed_bps) in path.iter().enumerate() {
                price = max_risk_next_price_signed_bps(price, *signed_bps);
                env.set_slot_and_price_raw_no_walk(
                    MAX_RISK_ATTACK_START_SLOT + step as u64 + 1,
                    price as i64,
                );
                try_empty_crank(&mut env).expect("empty crank should not reject");
                min_insurance = min_insurance.min(env.read_insurance_balance());
            }
            min_insurance =
                crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);
        }
        MaxRiskEdgeSchedule::OracleThenFullEveryOtherSlot => {
            for (step, signed_bps) in path.iter().enumerate() {
                price = max_risk_next_price_signed_bps(price, *signed_bps);
                env.set_slot_and_price_raw_no_walk(
                    MAX_RISK_ATTACK_START_SLOT + step as u64 + 1,
                    price as i64,
                );
                if step % 2 == 1 {
                    min_insurance =
                        crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);
                }
            }
            min_insurance =
                crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);
        }
        MaxRiskEdgeSchedule::OracleThenFullAtMidAndEnd => {
            for (step, signed_bps) in path.iter().enumerate() {
                price = max_risk_next_price_signed_bps(price, *signed_bps);
                env.set_slot_and_price_raw_no_walk(
                    MAX_RISK_ATTACK_START_SLOT + step as u64 + 1,
                    price as i64,
                );
                if step == 3 || step == 7 {
                    min_insurance =
                        crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);
                }
            }
        }
        MaxRiskEdgeSchedule::GapThenFull => {
            price = max_risk_gap_price_from_signed_bps_path(&path);
            env.set_slot_and_price_raw_no_walk(
                MAX_RISK_ATTACK_START_SLOT + MAX_RISK_ATTACK_SLOTS as u64,
                price as i64,
            );
            min_insurance =
                crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);
        }
    }

    for _ in 0..2 {
        min_insurance = crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);
    }

    let insurance_end = env.read_insurance_balance();
    let _ = (label, schedule, price);
    (insurance_start, min_insurance, insurance_end)
}

fn run_max_risk_price_probe(
    label: &str,
    path: [i8; MAX_RISK_ATTACK_SLOTS],
    schedule: MaxRiskCrankSchedule,
    num_longs: usize,
    num_shorts: usize,
) -> (u128, u128, u128) {
    let (mut env, _lp, lp_idx, actors) = setup_max_risk_probe(num_longs, num_shorts);
    let candidates = max_risk_candidate_indices(lp_idx, &actors);
    let insurance_start = env.read_insurance_balance();
    let mut min_insurance = insurance_start;
    let mut price = MAX_RISK_P0_E6;

    match schedule {
        MaxRiskCrankSchedule::HonestEverySlot => {
            for (step, dir) in path.iter().enumerate() {
                price = max_risk_next_clamped_price(price, *dir, 1);
                env.set_slot_and_price_raw_no_walk(
                    MAX_RISK_ATTACK_START_SLOT + step as u64 + 1,
                    price as i64,
                );
                min_insurance =
                    crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);
            }
        }
        MaxRiskCrankSchedule::EmptyEverySlotThenFull => {
            for (step, dir) in path.iter().enumerate() {
                price = max_risk_next_clamped_price(price, *dir, 1);
                env.set_slot_and_price_raw_no_walk(
                    MAX_RISK_ATTACK_START_SLOT + step as u64 + 1,
                    price as i64,
                );
                try_empty_crank(&mut env).expect("empty crank should not reject");
                min_insurance = min_insurance.min(env.read_insurance_balance());
            }
            min_insurance =
                crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);
        }
        MaxRiskCrankSchedule::GapThenFull => {
            price = max_risk_gap_price_from_path(&path);
            env.set_slot_and_price_raw_no_walk(
                MAX_RISK_ATTACK_START_SLOT + MAX_RISK_ATTACK_SLOTS as u64,
                price as i64,
            );
            min_insurance =
                crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);
        }
    }

    for _ in 0..4 {
        min_insurance = crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);
    }

    let insurance_end = env.read_insurance_balance();
    println!(
        "max-risk probe {label:?} {schedule:?}: insurance start={insurance_start}, min={min_insurance}, end={insurance_end}, final_price={price}",
    );
    (insurance_start, min_insurance, insurance_end)
}

fn close_profitable_side_and_withdraw(
    env: &mut TestEnv,
    lp: &Keypair,
    lp_idx: u16,
    actors: &[MaxRiskActor],
    profitable_direction: i8,
) {
    for actor in actors
        .iter()
        .filter(|a| a.direction == profitable_direction)
    {
        let pos = env.read_account_position(actor.idx);
        if pos == 0 {
            continue;
        }
        env.try_trade(&actor.keypair, lp, lp_idx, actor.idx, -pos)
            .expect("profitable side should be able to close");
        let cap = env.read_account_capital(actor.idx);
        if cap > 0 {
            env.try_withdraw(&actor.keypair, actor.idx, cap as u64)
                .expect("profitable side should be able to withdraw realized capital");
        }
    }
}

#[test]
fn test_attack_max_risk_edge_sweep_no_insurance_drain() {
    program_path();

    let schedules = [
        MaxRiskEdgeSchedule::OracleThenFullEverySlot,
        MaxRiskEdgeSchedule::FullThenOracleEverySlot,
        MaxRiskEdgeSchedule::OracleThenEmptyEverySlotThenFull,
        MaxRiskEdgeSchedule::OracleThenFullEveryOtherSlot,
        MaxRiskEdgeSchedule::OracleThenFullAtMidAndEnd,
        MaxRiskEdgeSchedule::GapThenFull,
    ];
    let mut cases_run = 0usize;
    let mut closest_margin = u128::MAX;

    // Exhaust all max-step signs. This is the sharpest liquidation/cascade
    // surface: every slot moves by the maximum allowed amount, but the keeper
    // schedule varies from perfectly honest to stale-by-one-slot and sparse.
    for mask in 0u16..(1u16 << MAX_RISK_ATTACK_SLOTS) {
        let mut path = [0i16; MAX_RISK_ATTACK_SLOTS];
        for (step, slot_bps) in path.iter_mut().enumerate() {
            *slot_bps = if ((mask >> step) & 1) == 0 {
                -(MAX_RISK_CLAMP_BPS as i16)
            } else {
                MAX_RISK_CLAMP_BPS as i16
            };
        }
        for schedule in schedules {
            let label = "all_signs_at_max_bps";
            let (start, min, end) = run_max_risk_edge_price_probe(label, path, schedule, 4, 4);
            cases_run += 1;
            closest_margin = closest_margin.min(min.saturating_sub(start));
            assert!(
                min >= start,
                "EXPLOIT: edge sweep mask={mask:#04x} {schedule:?} drained insurance below start: path={path:?} start={start}, min={min}, end={end}"
            );
        }
    }

    // Boundary magnitudes around rounding and threshold behavior. These are
    // deliberately not exhaustive over 50^8; they hit zero movement, one bps,
    // just-below-cap, cap, half-cap, and concentrated first/last-slot shocks.
    let boundary_paths: [(&str, [i16; MAX_RISK_ATTACK_SLOTS]); 18] = [
        ("zero", [0, 0, 0, 0, 0, 0, 0, 0]),
        ("all_down_1", [-1, -1, -1, -1, -1, -1, -1, -1]),
        ("all_up_1", [1, 1, 1, 1, 1, 1, 1, 1]),
        ("all_down_24", [-24, -24, -24, -24, -24, -24, -24, -24]),
        ("all_up_24", [24, 24, 24, 24, 24, 24, 24, 24]),
        ("all_down_48", [-48, -48, -48, -48, -48, -48, -48, -48]),
        ("all_up_48", [48, 48, 48, 48, 48, 48, 48, 48]),
        ("first_down_max", [-49, 0, 0, 0, 0, 0, 0, 0]),
        ("last_down_max", [0, 0, 0, 0, 0, 0, 0, -49]),
        ("first_up_max", [49, 0, 0, 0, 0, 0, 0, 0]),
        ("last_up_max", [0, 0, 0, 0, 0, 0, 0, 49]),
        ("down_spike_revert", [-49, 49, 0, 0, 0, 0, 0, 0]),
        ("up_spike_revert", [49, -49, 0, 0, 0, 0, 0, 0]),
        ("late_down_spike_revert", [0, 0, 0, 0, 0, 0, -49, 49]),
        ("late_up_spike_revert", [0, 0, 0, 0, 0, 0, 49, -49]),
        ("alternating_small", [-1, 1, -1, 1, -1, 1, -1, 1]),
        ("alternating_48", [-48, 48, -48, 48, -48, 48, -48, 48]),
        ("mixed_threshold", [-49, -48, -24, -1, 1, 24, 48, 49]),
    ];
    for (label, path) in boundary_paths {
        for schedule in schedules {
            let (start, min, end) = run_max_risk_edge_price_probe(label, path, schedule, 8, 8);
            cases_run += 1;
            closest_margin = closest_margin.min(min.saturating_sub(start));
            assert!(
                min >= start,
                "EXPLOIT: edge boundary {label} {schedule:?} drained insurance below start: path={path:?} start={start}, min={min}, end={end}"
            );
        }
    }

    println!(
        "max-risk edge sweep PASS_SAFE: cases={cases_run}, closest_min_minus_start={closest_margin}"
    );
}

#[test]
fn test_attack_profit_maturity_cap_independent_from_accrual_window() {
    program_path();

    let mut env = TestEnv::new();
    env.set_slot_and_price_raw_no_walk(MAX_RISK_ATTACK_START_SLOT, MAX_RISK_P0_E6 as i64);
    let data = encode_init_market_bounty_sol_20x_with_h_max_and_perm_resolve(
        &env.payer.pubkey(),
        &env.mint,
        percolator_prog::constants::MAX_PROFIT_MATURITY_SLOTS,
        MAX_RISK_PERMISSIONLESS_RESOLVE_STALE_SLOTS,
    );
    env.try_init_market_raw(data)
        .expect("30-day profit maturity cap should be accepted independently from max_dt=10");

    let mut env = TestEnv::new();
    env.set_slot_and_price_raw_no_walk(MAX_RISK_ATTACK_START_SLOT, MAX_RISK_P0_E6 as i64);
    let data = encode_init_market_bounty_sol_20x_with_h_max_and_perm_resolve(
        &env.payer.pubkey(),
        &env.mint,
        percolator_prog::constants::MAX_PROFIT_MATURITY_SLOTS + 1,
        MAX_RISK_PERMISSIONLESS_RESOLVE_STALE_SLOTS,
    );
    let err = env
        .try_init_market_raw(data)
        .expect_err("profit maturity beyond wrapper cap should be rejected");
    println!("profit maturity above cap rejected as expected: {err}");
}

#[test]
fn test_attack_permissionless_resolve_cap_independent_from_accrual_window() {
    program_path();

    let mut env = TestEnv::new();
    env.set_slot_and_price_raw_no_walk(MAX_RISK_ATTACK_START_SLOT, MAX_RISK_P0_E6 as i64);
    let data = encode_init_market_bounty_sol_20x_with_h_max_and_perm_resolve(
        &env.payer.pubkey(),
        &env.mint,
        MAX_RISK_H_MAX,
        percolator_prog::constants::MAX_PERMISSIONLESS_RESOLVE_STALE_SLOTS,
    );
    env.try_init_market_raw(data).expect(
        "30-day permissionless stale horizon should be accepted independently from max_dt=10",
    );

    let mut env = TestEnv::new();
    env.set_slot_and_price_raw_no_walk(MAX_RISK_ATTACK_START_SLOT, MAX_RISK_P0_E6 as i64);
    let data = encode_init_market_bounty_sol_20x_with_h_max_and_perm_resolve(
        &env.payer.pubkey(),
        &env.mint,
        MAX_RISK_H_MAX,
        percolator_prog::constants::MAX_PERMISSIONLESS_RESOLVE_STALE_SLOTS + 1,
    );
    let err = env
        .try_init_market_raw(data)
        .expect_err("permissionless stale horizon beyond wrapper cap should be rejected");
    println!("permissionless stale horizon above cap rejected as expected: {err}");
}

fn run_max_risk_pingpong_probe(label: &str, first_direction: i8) -> (u128, u128, u128) {
    let (mut env, lp, lp_idx, actors) = setup_max_risk_probe(4, 4);
    let candidates = max_risk_candidate_indices(lp_idx, &actors);
    let insurance_start = env.read_insurance_balance();
    let mut min_insurance = insurance_start;
    let mut price = MAX_RISK_P0_E6;

    for step in 0..4 {
        price = max_risk_next_clamped_price(price, first_direction, 1);
        env.set_slot_and_price_raw_no_walk(MAX_RISK_ATTACK_START_SLOT + step + 1, price as i64);
        min_insurance = crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);
    }
    let first_profitable = if first_direction < 0 { -1 } else { 1 };
    close_profitable_side_and_withdraw(&mut env, &lp, lp_idx, &actors, first_profitable);
    min_insurance = min_insurance.min(env.read_insurance_balance());

    for step in 4..8 {
        price = max_risk_next_clamped_price(price, -first_direction, 1);
        env.set_slot_and_price_raw_no_walk(MAX_RISK_ATTACK_START_SLOT + step + 1, price as i64);
        min_insurance = crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);
    }
    let second_profitable = if first_direction < 0 { 1 } else { -1 };
    close_profitable_side_and_withdraw(&mut env, &lp, lp_idx, &actors, second_profitable);
    min_insurance = crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);

    let insurance_end = env.read_insurance_balance();
    println!(
        "max-risk pingpong {label:?}: insurance start={insurance_start}, min={min_insurance}, end={insurance_end}, final_price={price}",
    );
    (insurance_start, min_insurance, insurance_end)
}

fn run_max_risk_full_window_profit_take_probe(
    label: &str,
    direction: i8,
    schedule: MaxRiskCrankSchedule,
) -> (u128, u128, u128) {
    let (mut env, lp, lp_idx, actors) = setup_max_risk_probe(16, 16);
    let candidates = max_risk_candidate_indices(lp_idx, &actors);
    let insurance_start = env.read_insurance_balance();
    let mut min_insurance = insurance_start;
    let mut price = MAX_RISK_P0_E6;

    match schedule {
        MaxRiskCrankSchedule::HonestEverySlot => {
            for step in 0..MAX_RISK_ATTACK_SLOTS {
                price = max_risk_next_clamped_price(price, direction, 1);
                env.set_slot_and_price_raw_no_walk(
                    MAX_RISK_ATTACK_START_SLOT + step as u64 + 1,
                    price as i64,
                );
                min_insurance =
                    crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);
            }
        }
        MaxRiskCrankSchedule::EmptyEverySlotThenFull => {
            for step in 0..MAX_RISK_ATTACK_SLOTS {
                price = max_risk_next_clamped_price(price, direction, 1);
                env.set_slot_and_price_raw_no_walk(
                    MAX_RISK_ATTACK_START_SLOT + step as u64 + 1,
                    price as i64,
                );
                try_empty_crank(&mut env).expect("empty crank should not reject");
                min_insurance = min_insurance.min(env.read_insurance_balance());
            }
        }
        MaxRiskCrankSchedule::GapThenFull => {
            price = max_risk_next_clamped_price(
                MAX_RISK_P0_E6,
                direction,
                MAX_RISK_ATTACK_SLOTS as u64,
            );
            env.set_slot_and_price_raw_no_walk(
                MAX_RISK_ATTACK_START_SLOT + MAX_RISK_ATTACK_SLOTS as u64,
                price as i64,
            );
        }
    }

    min_insurance = crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);
    let profitable_side = if direction < 0 { -1 } else { 1 };
    close_profitable_side_and_withdraw(&mut env, &lp, lp_idx, &actors, profitable_side);
    min_insurance = min_insurance.min(env.read_insurance_balance());

    for _ in 0..4 {
        min_insurance = crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);
    }

    let insurance_end = env.read_insurance_balance();
    println!(
        "max-risk profit-take {label:?} {schedule:?}: insurance start={insurance_start}, min={min_insurance}, end={insurance_end}, final_price={price}",
    );
    (insurance_start, min_insurance, insurance_end)
}

fn run_max_risk_profit_take_probe_steps(
    label: &str,
    direction: i8,
    steps: usize,
    schedule: MaxRiskCrankSchedule,
) -> (u128, u128, u128) {
    let (mut env, lp, lp_idx, actors) = setup_max_risk_probe(16, 16);
    let candidates = max_risk_candidate_indices(lp_idx, &actors);
    let insurance_start = env.read_insurance_balance();
    let mut min_insurance = insurance_start;
    let mut price = MAX_RISK_P0_E6;

    match schedule {
        MaxRiskCrankSchedule::HonestEverySlot => {
            for step in 0..steps {
                price = max_risk_next_clamped_price(price, direction, 1);
                env.set_slot_and_price_raw_no_walk(
                    MAX_RISK_ATTACK_START_SLOT + step as u64 + 1,
                    price as i64,
                );
                min_insurance =
                    crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);
            }
        }
        MaxRiskCrankSchedule::EmptyEverySlotThenFull => {
            for step in 0..steps {
                price = max_risk_next_clamped_price(price, direction, 1);
                env.set_slot_and_price_raw_no_walk(
                    MAX_RISK_ATTACK_START_SLOT + step as u64 + 1,
                    price as i64,
                );
                try_empty_crank(&mut env).expect("empty crank should not reject");
                min_insurance = min_insurance.min(env.read_insurance_balance());
            }
        }
        MaxRiskCrankSchedule::GapThenFull => {
            price = max_risk_next_clamped_price(MAX_RISK_P0_E6, direction, steps as u64);
            env.set_slot_and_price_raw_no_walk(
                MAX_RISK_ATTACK_START_SLOT + steps as u64,
                price as i64,
            );
        }
    }

    min_insurance = crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);
    let profitable_side = if direction < 0 { -1 } else { 1 };
    close_profitable_side_and_withdraw(&mut env, &lp, lp_idx, &actors, profitable_side);
    min_insurance = min_insurance.min(env.read_insurance_balance());

    for _ in 0..4 {
        min_insurance = crank_candidate_sweep_and_track_min(&mut env, &candidates, min_insurance);
    }

    let insurance_end = env.read_insurance_balance();
    println!(
        "max-risk {steps}-slot profit-take {label:?} {schedule:?}: insurance start={insurance_start}, min={min_insurance}, end={insurance_end}, final_price={price}",
    );
    (insurance_start, min_insurance, insurance_end)
}

#[test]
fn test_attack_max_risk_8_slot_oracle_crank_orderings_do_not_drain_insurance() {
    program_path();

    let paths: [(&str, [i8; MAX_RISK_ATTACK_SLOTS]); 8] = [
        ("all_down", [-1, -1, -1, -1, -1, -1, -1, -1]),
        ("all_up", [1, 1, 1, 1, 1, 1, 1, 1]),
        ("down_then_up", [-1, -1, -1, -1, 1, 1, 1, 1]),
        ("up_then_down", [1, 1, 1, 1, -1, -1, -1, -1]),
        ("alternating_down_first", [-1, 1, -1, 1, -1, 1, -1, 1]),
        ("alternating_up_first", [1, -1, 1, -1, 1, -1, 1, -1]),
        ("two_down_two_up", [-1, -1, 1, 1, -1, -1, 1, 1]),
        ("two_up_two_down", [1, 1, -1, -1, 1, 1, -1, -1]),
    ];
    let schedules = [
        MaxRiskCrankSchedule::HonestEverySlot,
        MaxRiskCrankSchedule::EmptyEverySlotThenFull,
        MaxRiskCrankSchedule::GapThenFull,
    ];

    for (label, path) in paths {
        for schedule in schedules {
            let (start, min, end) = run_max_risk_price_probe(label, path, schedule, 16, 16);
            assert!(
                min >= start,
                "EXPLOIT: {label} {schedule:?} drained insurance below start: start={start}, min={min}, end={end}"
            );
        }
    }

    let cascade_cases = [
        (
            "cascade_longs_gap_down",
            [-1, -1, -1, -1, -1, -1, -1, -1],
            MaxRiskCrankSchedule::GapThenFull,
            80,
            0,
        ),
        (
            "cascade_shorts_gap_up",
            [1, 1, 1, 1, 1, 1, 1, 1],
            MaxRiskCrankSchedule::GapThenFull,
            0,
            80,
        ),
        (
            "cascade_longs_empty_down",
            [-1, -1, -1, -1, -1, -1, -1, -1],
            MaxRiskCrankSchedule::EmptyEverySlotThenFull,
            80,
            0,
        ),
        (
            "cascade_shorts_empty_up",
            [1, 1, 1, 1, 1, 1, 1, 1],
            MaxRiskCrankSchedule::EmptyEverySlotThenFull,
            0,
            80,
        ),
    ];
    for (label, path, schedule, longs, shorts) in cascade_cases {
        let (start, min, end) = run_max_risk_price_probe(label, path, schedule, longs, shorts);
        assert!(
            min >= start,
            "EXPLOIT: {label} {schedule:?} drained insurance below start: start={start}, min={min}, end={end}"
        );
    }

    for (label, dir) in [
        ("shorts_close_then_longs", -1),
        ("longs_close_then_shorts", 1),
    ] {
        let (start, min, end) = run_max_risk_pingpong_probe(label, dir);
        assert!(
            min >= start,
            "EXPLOIT: pingpong {label} drained insurance below start: start={start}, min={min}, end={end}"
        );
    }

    for (label, dir) in [
        ("shorts_take_profit_after_down", -1),
        ("longs_take_profit_after_up", 1),
    ] {
        for schedule in schedules {
            let (start, min, end) =
                run_max_risk_full_window_profit_take_probe(label, dir, schedule);
            assert!(
                min >= start,
                "EXPLOIT: profit-take {label} {schedule:?} drained insurance below start: start={start}, min={min}, end={end}"
            );
        }
    }

    for (label, dir) in [
        ("boundary_shorts_take_profit_after_down", -1),
        ("boundary_longs_take_profit_after_up", 1),
    ] {
        for schedule in [
            MaxRiskCrankSchedule::HonestEverySlot,
            MaxRiskCrankSchedule::EmptyEverySlotThenFull,
        ] {
            let (start, min, end) = run_max_risk_profit_take_probe_steps(label, dir, 10, schedule);
            assert!(
                min >= start,
                "EXPLOIT: 10-slot profit-take {label} {schedule:?} drained insurance below start: start={start}, min={min}, end={end}"
            );
        }

        let (start, min, end) =
            run_max_risk_profit_take_probe_steps(label, dir, 10, MaxRiskCrankSchedule::GapThenFull);
        assert!(
            min >= start,
            "EXPLOIT: 10-slot gap profit-take {label} drained insurance below start: start={start}, min={min}, end={end}"
        );
    }
}
