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

#[test]
fn test_limited_insurance_withdraw_defaults_enforced() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");

    // Seed insurance before resolution.
    env.top_up_insurance(&admin, 10_000_000_000);
    let insurance_before = env.read_insurance_balance();
    assert_eq!(insurance_before, 10_000_000_000, "precondition: insurance seeded");

    env.try_resolve_market(&admin)
        .expect("resolve must succeed before insurance withdraw");
    assert!(env.is_market_resolved(), "market should be resolved");

    let non_admin = Keypair::new();
    env.svm
        .airdrop(&non_admin.pubkey(), 1_000_000_000)
        .expect("airdrop non-admin");
    let non_admin_attempt = env.try_withdraw_insurance_limited(&non_admin, 100_000_000);
    assert!(
        non_admin_attempt.is_err(),
        "default-limited withdraw should only allow admin when no policy is configured"
    );
    assert_eq!(
        env.read_insurance_balance(),
        insurance_before,
        "rejected non-admin default withdraw must not change insurance"
    );

    // Default policy: 1% max, 400_000-slot cooldown, min amount 1.
    env.set_slot(1);
    let first_amount = 100_000_000u64; // 1% of 10_000_000_000
    let first = env.try_withdraw_insurance_limited(&admin, first_amount);
    assert!(
        first.is_ok(),
        "default-limited withdraw at 1% should succeed: {:?}",
        first
    );
    let insurance_after_first = env.read_insurance_balance();
    assert_eq!(
        insurance_after_first,
        insurance_before - first_amount as u128,
        "insurance must decrease by first limited withdraw amount"
    );

    // Same slot / before cooldown must fail.
    let second_too_soon = env.try_withdraw_insurance_limited(&admin, 99_000_000);
    assert!(
        second_too_soon.is_err(),
        "default-limited withdraw should enforce cooldown"
    );
    assert_eq!(
        env.read_insurance_balance(),
        insurance_after_first,
        "rejected cooldown withdraw must not change insurance"
    );

    // After cooldown, next <=1% withdrawal should succeed.
    env.set_slot(400_001);
    let second_amount = 99_000_000u64; // 1% of 9_900_000_000
    let second = env.try_withdraw_insurance_limited(&admin, second_amount);
    assert!(
        second.is_ok(),
        "default-limited withdraw after cooldown should succeed: {:?}",
        second
    );
    let insurance_after_second = env.read_insurance_balance();
    assert_eq!(
        insurance_after_second,
        insurance_after_first - second_amount as u128,
        "insurance must decrease by second limited withdraw amount"
    );
}

#[test]
fn test_limited_insurance_withdraw_custom_policy_enforced() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");

    env.top_up_insurance(&admin, 10_000_000_000);
    env.try_resolve_market(&admin)
        .expect("resolve must succeed before policy update");

    let delegated = Keypair::new();
    env.svm
        .airdrop(&delegated.pubkey(), 1_000_000_000)
        .expect("airdrop delegated authority");

    // Authority is not admin, so it must not be able to set policy.
    let delegated_set_attempt = env.try_set_insurance_withdraw_policy(
        &delegated,
        &delegated.pubkey(),
        1,
        10_000,
        1,
    );
    assert!(
        delegated_set_attempt.is_err(),
        "non-admin authority must not be able to configure withdraw policy"
    );

    // Policy: delegated authority, min=100M, max=5%, cooldown=10 slots.
    let set_policy =
        env.try_set_insurance_withdraw_policy(&admin, &delegated.pubkey(), 100_000_000, 500, 10);
    assert!(
        set_policy.is_ok(),
        "admin should configure limited insurance withdraw policy: {:?}",
        set_policy
    );

    // Delegated authority still cannot mutate limits/authority.
    let delegated_mutation_attempt = env.try_set_insurance_withdraw_policy(
        &delegated,
        &delegated.pubkey(),
        1,
        10_000,
        1,
    );
    assert!(
        delegated_mutation_attempt.is_err(),
        "configured withdraw authority must not be able to change policy"
    );

    env.set_slot(2);

    // Admin is no longer authorized for limited path after policy is set.
    let admin_attempt = env.try_withdraw_insurance_limited(&admin, 100_000_000);
    assert!(
        admin_attempt.is_err(),
        "non-delegated signer should be rejected for limited withdraw"
    );
    assert_eq!(
        env.read_insurance_balance(),
        10_000_000_000,
        "rejected unauthorized limited withdraw must not change insurance"
    );

    // Above max percentage should fail (5% of 10B = 500M).
    let above_max = env.try_withdraw_insurance_limited(&delegated, 600_000_000);
    assert!(above_max.is_err(), "withdraw above policy max% should fail");
    assert_eq!(
        env.read_insurance_balance(),
        10_000_000_000,
        "rejected above-max limited withdraw must not change insurance"
    );

    // Exactly at max should pass.
    let first_ok = env.try_withdraw_insurance_limited(&delegated, 500_000_000);
    assert!(
        first_ok.is_ok(),
        "withdraw at policy max% should succeed: {:?}",
        first_ok
    );
    let insurance_after_first = env.read_insurance_balance();
    assert_eq!(
        insurance_after_first, 9_500_000_000,
        "insurance should decrease by successful delegated withdraw"
    );

    // Cooldown should block immediate second withdrawal.
    let too_soon = env.try_withdraw_insurance_limited(&delegated, 100_000_000);
    assert!(too_soon.is_err(), "policy cooldown must be enforced");
    assert_eq!(
        env.read_insurance_balance(),
        insurance_after_first,
        "rejected cooldown limited withdraw must not change insurance"
    );

    env.set_slot(12);
    // Below policy min is now allowed as long as it is within the capped maximum.
    // 5% of 9.5B = 475M, so 50M should pass even though policy min is 100M.
    let second_ok = env.try_withdraw_insurance_limited(&delegated, 50_000_000);
    assert!(
        second_ok.is_ok(),
        "below-min withdraw should be allowed when under capped max: {:?}",
        second_ok
    );
    let insurance_after_second = env.read_insurance_balance();
    assert_eq!(
        insurance_after_second, 9_450_000_000,
        "insurance should decrease by second successful delegated withdraw (below-min allowed)"
    );
}

#[test]
fn test_limited_insurance_withdraw_cooldown_enforced_from_slot_zero() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");

    env.top_up_insurance(&admin, 10_000_000_000);
    env.try_resolve_market(&admin)
        .expect("resolve must succeed before policy update");

    // Use custom policy so cooldown must be enforced exactly from first successful withdraw.
    env.try_set_insurance_withdraw_policy(&admin, &admin.pubkey(), 1, 100, 10)
        .expect("policy setup should succeed");

    env.set_slot(0);
    let first = env.try_withdraw_insurance_limited(&admin, 100_000_000);
    assert!(
        first.is_ok(),
        "first withdraw at slot zero should succeed: {:?}",
        first
    );
    let insurance_after_first = env.read_insurance_balance();
    assert_eq!(
        insurance_after_first, 9_900_000_000,
        "first withdraw should debit insurance by 1%"
    );

    // Same-slot second withdraw must fail due to cooldown (pathological slot-zero case).
    let second_same_slot = env.try_withdraw_insurance_limited(&admin, 1);
    assert!(
        second_same_slot.is_err(),
        "cooldown must block second withdraw in slot zero"
    );
    assert_eq!(
        env.read_insurance_balance(),
        insurance_after_first,
        "rejected slot-zero cooldown withdraw must not change insurance"
    );

    // Boundary: at slot == last_slot + cooldown, withdraw should be allowed.
    env.set_slot(10);
    let at_boundary = env.try_withdraw_insurance_limited(&admin, 1);
    assert!(
        at_boundary.is_ok(),
        "withdraw at cooldown boundary should succeed: {:?}",
        at_boundary
    );
    assert_eq!(
        env.read_insurance_balance(),
        insurance_after_first - 1,
        "boundary withdraw should debit insurance"
    );
}

#[test]
fn test_limited_insurance_withdraw_min_floor_when_percent_cap_small() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");

    env.top_up_insurance(&admin, 10_000);
    env.try_resolve_market(&admin)
        .expect("resolve must succeed before policy update");

    // min=1000, max=1% => percent cap on 10_000 is only 100.
    env.try_set_insurance_withdraw_policy(&admin, &admin.pubkey(), 1_000, 100, 0)
        .expect("policy setup should succeed");

    env.set_slot(1);
    // Less than min is allowed as long as it stays under max(min, pct*fund).
    let below_min = env.try_withdraw_insurance_limited(&admin, 500);
    assert!(
        below_min.is_ok(),
        "below-min withdraw should be allowed under cap floor semantics: {:?}",
        below_min
    );
    assert_eq!(
        env.read_insurance_balance(),
        9_500,
        "successful below-min withdraw should debit insurance"
    );

    env.set_slot(2);
    // pct cap is 95 now; min floor still allows withdrawing up to 1000.
    let at_floor = env.try_withdraw_insurance_limited(&admin, 1_000);
    assert!(
        at_floor.is_ok(),
        "withdraw equal to floor min should be allowed when pct cap is smaller: {:?}",
        at_floor
    );
    assert_eq!(
        env.read_insurance_balance(),
        8_500,
        "withdraw at floor min should debit insurance"
    );

    env.set_slot(3);
    let above_floor = env.try_withdraw_insurance_limited(&admin, 1_001);
    assert!(
        above_floor.is_err(),
        "withdraw above floor min must be rejected when pct cap remains below min"
    );
    assert_eq!(
        env.read_insurance_balance(),
        8_500,
        "rejected above-floor withdraw must not change insurance"
    );
}

#[test]
fn test_limited_insurance_withdraw_default_min_floor_respects_unit_scale() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_full(0, 1000, 0); // unit_scale = 1000

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");

    // 50_000 base => 50 units in insurance.
    env.top_up_insurance(&admin, 50_000);
    assert_eq!(
        env.read_insurance_balance(),
        50,
        "precondition: insurance should be seeded in scaled units"
    );

    env.try_resolve_market(&admin)
        .expect("resolve must succeed before limited withdraw");

    // Sanity-check resolved config state for default-policy path.
    let slab_data = env.svm.get_account(&env.slab).unwrap().data;
    const UNIT_SCALE_OFF: usize = 180; // header(72) + unit_scale(108)
    const AUTH_TS_OFF: usize = 368; // header(72) + authority_timestamp(296)
    let unit_scale = u32::from_le_bytes(
        slab_data[UNIT_SCALE_OFF..UNIT_SCALE_OFF + 4]
            .try_into()
            .unwrap(),
    );
    assert_eq!(
        unit_scale, 1000,
        "precondition: market unit_scale must be 1000"
    );
    let authority_timestamp = i64::from_le_bytes(
        slab_data[AUTH_TS_OFF..AUTH_TS_OFF + 8]
            .try_into()
            .unwrap(),
    );
    let stored_bps = ((authority_timestamp as u64 >> 48) & 0xFFFF) as u16;
    assert_eq!(
        stored_bps, 0,
        "precondition: default path should be unconfigured before first limited withdraw"
    );

    // Default policy is 1% per withdraw. For 50 units that rounds to 0, so this test
    // proves the default min floor still permits withdrawing one aligned unit (1000 base).
    env.set_slot(1);
    let first = env.try_withdraw_insurance_limited(&admin, 1_000);
    assert!(
        first.is_ok(),
        "default policy should allow withdrawing one aligned unit when percent cap rounds to zero: {:?}",
        first
    );
    assert_eq!(
        env.read_insurance_balance(),
        49,
        "successful default floor withdraw should debit one unit"
    );

    // Cooldown should still be enforced.
    let too_soon = env.try_withdraw_insurance_limited(&admin, 1_000);
    assert!(too_soon.is_err(), "default cooldown must be enforced");
    assert_eq!(
        env.read_insurance_balance(),
        49,
        "rejected cooldown withdraw must not change insurance"
    );

    env.set_slot(400_001);
    let second = env.try_withdraw_insurance_limited(&admin, 1_000);
    assert!(
        second.is_ok(),
        "default withdraw should succeed again after cooldown: {:?}",
        second
    );
    assert_eq!(
        env.read_insurance_balance(),
        48,
        "post-cooldown withdraw should debit one unit"
    );
}

#[test]
fn test_limited_insurance_withdraw_failed_attempts_do_not_arm_cooldown() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");

    env.top_up_insurance(&admin, 10_000_000_000);
    env.try_resolve_market(&admin)
        .expect("resolve must succeed before policy update");

    let delegated = Keypair::new();
    env.svm
        .airdrop(&delegated.pubkey(), 1_000_000_000)
        .expect("airdrop delegated authority");

    // 5% cap, 10-slot cooldown.
    env.try_set_insurance_withdraw_policy(&admin, &delegated.pubkey(), 1, 500, 10)
        .expect("policy setup should succeed");

    env.set_slot(7);

    // Unauthorized signer must fail and must not consume cooldown state.
    let unauthorized = env.try_withdraw_insurance_limited(&admin, 100_000_000);
    assert!(
        unauthorized.is_err(),
        "non-policy authority must be rejected for limited withdraw"
    );
    assert_eq!(
        env.read_insurance_balance(),
        10_000_000_000,
        "rejected unauthorized withdraw must not change insurance"
    );

    // Authorized but above max must fail and must not consume cooldown state.
    let above_max = env.try_withdraw_insurance_limited(&delegated, 600_000_000);
    assert!(above_max.is_err(), "above-cap withdraw should fail");
    assert_eq!(
        env.read_insurance_balance(),
        10_000_000_000,
        "rejected above-cap withdraw must not change insurance"
    );

    // Same-slot valid withdraw should still succeed: failed attempts must not arm cooldown.
    let first_valid = env.try_withdraw_insurance_limited(&delegated, 500_000_000);
    assert!(
        first_valid.is_ok(),
        "first successful withdraw must still work in same slot after failed attempts: {:?}",
        first_valid
    );
    assert_eq!(
        env.read_insurance_balance(),
        9_500_000_000,
        "successful withdraw should debit insurance"
    );

    // Cooldown is now armed from the successful withdraw at slot 7.
    let same_slot_after_success = env.try_withdraw_insurance_limited(&delegated, 1);
    assert!(
        same_slot_after_success.is_err(),
        "cooldown must block same-slot withdraw after a successful withdraw"
    );
    env.set_slot(16);
    let before_boundary = env.try_withdraw_insurance_limited(&delegated, 1);
    assert!(
        before_boundary.is_err(),
        "cooldown must block withdraw before slot 17 boundary"
    );
    env.set_slot(17);
    let at_boundary = env.try_withdraw_insurance_limited(&delegated, 1);
    assert!(
        at_boundary.is_ok(),
        "withdraw should succeed at cooldown boundary after successful slot-7 withdraw: {:?}",
        at_boundary
    );
}

#[test]
fn test_limited_insurance_policy_validation_and_resolution_gates() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_full(0, 1000, 0); // unit_scale = 1000 for alignment checks

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let delegated = Keypair::new();
    env.svm
        .airdrop(&delegated.pubkey(), 1_000_000_000)
        .expect("airdrop delegated");

    // Policy configuration now works on live markets (for yield distribution).
    let unresolved_set =
        env.try_set_insurance_withdraw_policy(&admin, &delegated.pubkey(), 1000, 100, 2);
    assert!(
        unresolved_set.is_err(),
        "policy configuration must fail before market resolution"
    );

    // Prepare resolvable state.
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");
    env.top_up_insurance(&admin, 1_000_000_000);
    env.try_resolve_market(&admin)
        .expect("resolve should succeed");

    // Validation: min_withdraw_base > 0.
    env.set_slot(10);
    let zero_min = env.try_set_insurance_withdraw_policy(&admin, &delegated.pubkey(), 0, 100, 1);
    assert!(zero_min.is_err(), "policy min=0 must be rejected");

    // Validation: max_withdraw_bps in 1..=10_000.
    env.set_slot(11);
    let zero_bps = env.try_set_insurance_withdraw_policy(&admin, &delegated.pubkey(), 1000, 0, 1);
    assert!(zero_bps.is_err(), "policy max_bps=0 must be rejected");
    env.set_slot(12);
    let over_bps = env.try_set_insurance_withdraw_policy(&admin, &delegated.pubkey(), 1000, 10_001, 1);
    assert!(over_bps.is_err(), "policy max_bps>10_000 must be rejected");

    // Validation: min_withdraw_base must be aligned with unit_scale when unit_scale != 0.
    env.set_slot(13);
    let misaligned_min =
        env.try_set_insurance_withdraw_policy(&admin, &delegated.pubkey(), 1001, 100, 1);
    assert!(
        misaligned_min.is_err(),
        "policy min must be aligned to unit_scale"
    );

    // Valid policy should pass.
    env.set_slot(14);
    let valid = env.try_set_insurance_withdraw_policy(&admin, &delegated.pubkey(), 1000, 100, 1);
    assert!(valid.is_ok(), "valid policy should be accepted: {:?}", valid);
}

#[test]
fn test_limited_insurance_withdraw_adversarial_guards() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_full(0, 1000, 0); // unit_scale = 1000 for alignment checks

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");

    // Seed insurance while unresolved.
    let seeded_base = 10_000_000_000u64;
    env.top_up_insurance(&admin, seeded_base);
    let seeded_insurance = env.read_insurance_balance();
    let expected_seeded_units = (seeded_base / 1000) as u128;
    assert_eq!(
        seeded_insurance, expected_seeded_units,
        "precondition: insurance should be seeded"
    );

    // WithdrawInsuranceLimited blocked on live markets when max_bps=0 (default).
    let unresolved_withdraw = env.try_withdraw_insurance_limited(&admin, 2000);
    assert!(
        unresolved_withdraw.is_err(),
        "Live-market limited withdraw must be blocked when max_bps=0"
    );

    // Create open positions so resolved-mode open-position guard can be tested.
    // With unit_scale=1000, need 100*1000=100_000 base for min_initial_deposit
    let lp = Keypair::new();
    let lp_idx = env.init_lp_with_fee(&lp, 100_000);
    env.deposit(&lp, lp_idx, 20_000_000_000);
    let user = Keypair::new();
    let user_idx = env.init_user_with_fee(&user, 100_000);
    env.deposit(&user, user_idx, 5_000_000_000);
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert_ne!(
        env.read_account_position(user_idx),
        0,
        "precondition: position should be open"
    );

    // Resolve market.
    env.try_resolve_market(&admin)
        .expect("resolve should succeed");

    // SetInsuranceWithdrawPolicy requires all accounts closed (prevents
    // clobbering Hyperp pricing state that open accounts depend on).
    let policy_while_open = env.try_set_insurance_withdraw_policy(
        &admin, &admin.pubkey(), 1000, 10_000, 0,
    );
    assert!(policy_while_open.is_err(),
        "SetInsuranceWithdrawPolicy must fail while accounts are open");

    // Close positions via resolved crank path + AdminForceCloseAccount.
    env.set_slot(200);
    env.crank();
    env.try_admin_force_close_account(&admin, lp_idx, &lp.pubkey())
        .expect("AdminForceCloseAccount LP must succeed");
    env.try_admin_force_close_account(&admin, user_idx, &user.pubkey())
        .expect("AdminForceCloseAccount user must succeed");

    // Now configure policy (all accounts closed).
    // Use different cooldown to avoid AlreadyProcessed (different tx hash).
    env.try_set_insurance_withdraw_policy(&admin, &admin.pubkey(), 1000, 10_000, 1)
        .expect("valid policy should be accepted after accounts closed");
    assert_eq!(
        env.read_account_position(user_idx),
        0,
        "precondition: user position should be closed after force-close"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        0,
        "precondition: lp position should be closed after force-close"
    );

    // Misaligned amount should be rejected for unit_scale=1000.
    let misaligned_withdraw = env.try_withdraw_insurance_limited(&admin, 1001);
    assert!(
        misaligned_withdraw.is_err(),
        "limited withdraw amount must be aligned to unit_scale"
    );
    assert_eq!(
        env.read_insurance_balance(),
        seeded_insurance,
        "rejected misaligned limited withdraw must not change insurance"
    );

    // Above available insurance must be rejected.
    let total_base_available = (seeded_insurance as u64).saturating_mul(1000);
    let too_large = total_base_available.saturating_add(1000);
    let above_balance = env.try_withdraw_insurance_limited(&admin, too_large);
    assert!(
        above_balance.is_err(),
        "limited withdraw above insurance balance must fail"
    );
    assert_eq!(
        env.read_insurance_balance(),
        seeded_insurance,
        "rejected above-balance limited withdraw must not change insurance"
    );

    // Final sanity: valid aligned in-balance withdraw succeeds and debits insurance.
    let valid_amount = 1000u64;
    let valid = env.try_withdraw_insurance_limited(&admin, valid_amount);
    assert!(valid.is_ok(), "valid limited withdraw should succeed: {:?}", valid);
    let expected_units_delta = (valid_amount / 1000) as u128;
    assert_eq!(
        env.read_insurance_balance(),
        seeded_insurance - expected_units_delta,
        "successful limited withdraw must reduce insurance by amount"
    );
}

/// Verify admin can always use Tag 20 (WithdrawInsurance) to drain all insurance,
/// even after a limited policy (Tag 22) is configured with a delegated authority.
/// This is by design: admin retains ultimate authority over the insurance fund.
#[test]
fn test_admin_withdraw_insurance_bypasses_limited_policy() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push");

    // Seed insurance before resolution
    env.top_up_insurance(&admin, 10_000_000_000);
    assert_eq!(env.read_insurance_balance(), 10_000_000_000, "precondition: insurance seeded");

    env.try_resolve_market(&admin).expect("resolve");
    assert!(env.is_market_resolved(), "market should be resolved");

    // Configure a restrictive limited policy: delegated authority, 1% max, 100k-slot cooldown
    let delegated = Keypair::new();
    env.svm.airdrop(&delegated.pubkey(), 1_000_000_000).expect("airdrop delegated");
    env.try_set_insurance_withdraw_policy(
        &admin,
        &delegated.pubkey(),
        1,       // min_withdraw_base
        100,     // max_withdraw_bps = 1%
        100_000, // cooldown_slots
    )
    .expect("set policy");

    // Delegated authority can only withdraw 1%
    env.set_slot(1);
    let limited = env.try_withdraw_insurance_limited(&delegated, 100_000_000); // 1% of 10B
    assert!(limited.is_ok(), "delegated 1% withdraw should succeed: {:?}", limited);
    assert_eq!(env.read_insurance_balance(), 9_900_000_000, "insurance after limited withdraw");

    // Delegated authority cannot use Tag 20 (requires admin)
    let delegated_tag20 = env.try_withdraw_insurance(&delegated);
    assert!(
        delegated_tag20.is_err(),
        "Delegated authority must not be able to use Tag 20 full withdraw"
    );
    assert_eq!(
        env.read_insurance_balance(),
        9_900_000_000,
        "failed delegated Tag 20 must not change insurance"
    );

    // Admin uses Tag 20 to drain ALL remaining insurance in one shot
    let vault_before = env.vault_balance();
    let drain = env.try_withdraw_insurance(&admin);
    assert!(
        drain.is_ok(),
        "Admin Tag 20 should bypass limited policy and drain all insurance: {:?}",
        drain
    );
    assert_eq!(
        env.read_insurance_balance(),
        0,
        "insurance should be zero after admin full withdraw"
    );
    let vault_after = env.vault_balance();
    assert_eq!(
        vault_before - vault_after,
        9_900_000_000,
        "vault SPL balance should decrease by the drained insurance amount"
    );
}

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
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Set oracle authority and push price
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
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");
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
fn test_attack_dust_sweep_to_insurance_on_crank() {
    program_path();

    let mut env = TestEnv::new();
    // Use unit_scale=1000 so deposits create dust
    env.init_market_full(0, 1000, 0);

    let lp_owner = Keypair::new();
    // With unit_scale=1000, need 100*1000=100_000 base for min_initial_deposit
    let lp_idx = env.init_user_with_fee(&lp_owner, 100_000);

    let user_owner = Keypair::new();
    let user_idx = env.init_user_with_fee(&user_owner, 100_000);

    // Deposit 1500 with unit_scale=1000: 1 unit + 500 dust
    env.deposit(&lp_owner, lp_idx, 1500);
    env.deposit(&user_owner, user_idx, 1500);

    // Read insurance balance before crank
    let insurance_before = env.read_insurance_balance();

    // Crank - should sweep dust if dust >= unit_scale
    // Two deposits of 500 dust = 1000 dust >= 1000 scale = 1 unit swept
    env.crank();

    let insurance_after = env.read_insurance_balance();

    // Verify dust was swept to insurance (1000 dust / 1000 scale = 1 unit)
    assert!(
        insurance_after > insurance_before,
        "Dust sweep should increase insurance: before={}, after={} \
         (2x500 dust = 1000 >= unit_scale 1000 = 1 unit expected)",
        insurance_before, insurance_after
    );

    // Vault balance from SPL should include both deposits
    let spl_vault_balance = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        let vault_account = TokenAccount::unpack(&vault_data).unwrap();
        vault_account.amount
    };
    // vault = 2 init deposits (100_000 each) + 2 deposits (1500 each) = 203000
    assert_eq!(
        spl_vault_balance, 203_000,
        "ATTACK: SPL vault should hold all deposited base tokens (including inits)"
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
#[test]
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
    env.init_market_with_invert(0);
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
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");

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

/// SetRiskThreshold with max_insurance_floor_change_per_day == 0 locks the floor.
/// SetRiskThreshold rate-limit uses actual current value, not stale baseline.
/// Cooldown enforcement on WithdrawInsuranceLimited (resolved market).
#[test]
fn test_insurance_withdraw_cooldown_enforcement() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    env.try_top_up_insurance(&admin, 10_000_000_000).unwrap();

    // Resolve market to enable SetInsuranceWithdrawPolicy
    env.set_slot(1);
    env.try_set_oracle_authority(&admin, &admin.pubkey()).unwrap();
    env.try_push_oracle_price(&admin, 138_000_000, 200).unwrap();
    env.try_resolve_market(&admin).unwrap();

    // Configure: 100% per withdrawal, 1000 slot cooldown
    env.try_set_insurance_withdraw_policy(&admin, &admin.pubkey(), 1, 10_000, 1000).unwrap();

    // First withdrawal succeeds
    env.set_slot(100);
    let r1 = env.try_withdraw_insurance_limited(&admin, 100_000_000);
    assert!(r1.is_ok(), "First withdrawal: {:?}", r1);

    // Within cooldown: rejected
    env.set_slot(200);
    let r2 = env.try_withdraw_insurance_limited(&admin, 100_000_000);
    assert!(r2.is_err(), "Within cooldown must be rejected");

    // After cooldown: succeeds
    env.set_slot(1200);
    let r3 = env.try_withdraw_insurance_limited(&admin, 100_000_000);
    assert!(r3.is_ok(), "After cooldown: {:?}", r3);
}

/// BPS cap enforcement on WithdrawInsuranceLimited.
#[test]
fn test_insurance_withdraw_bps_cap() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    env.try_top_up_insurance(&admin, 10_000_000_000).unwrap();

    // Resolve
    env.set_slot(1);
    env.try_set_oracle_authority(&admin, &admin.pubkey()).unwrap();
    env.try_push_oracle_price(&admin, 138_000_000, 200).unwrap();
    env.try_resolve_market(&admin).unwrap();

    // Configure: max 50% (5000 bps) per withdrawal
    env.try_set_insurance_withdraw_policy(&admin, &admin.pubkey(), 1, 5_000, 1).unwrap();

    let insurance = env.read_insurance_balance();
    let half = (insurance / 2) as u64;

    // Exactly 50%: succeeds
    env.set_slot(100);
    let r1 = env.try_withdraw_insurance_limited(&admin, half);
    assert!(r1.is_ok(), "50% withdrawal: {:?}", r1);

    // >50% of remaining: rejected
    let remaining = env.read_insurance_balance();
    let over_half = (remaining / 2 + 1) as u64;
    env.set_slot(200);
    let r2 = env.try_withdraw_insurance_limited(&admin, over_half);
    assert!(r2.is_err(), ">50% must be rejected");
}

/// insurance_withdraw_max_bps == 0 blocks live-market withdrawals.
#[test]
fn test_insurance_withdraw_disabled_on_live_market() {
    program_path();
    let mut env = TestEnv::new();
    // Default init: insurance_withdraw_max_bps = 0 (disabled)
    env.init_market_with_invert(0);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    env.try_top_up_insurance(&admin, 10_000_000_000).unwrap();

    // Capture state before rejected operation
    let vault_before = env.vault_balance();
    let insurance_before = env.read_insurance_balance();

    // Live-market withdrawal should be rejected
    env.set_slot(1);
    let r = env.try_withdraw_insurance_limited(&admin, 1);
    assert!(r.is_err(), "Live withdrawal must be blocked when max_bps=0");

    // State must be unchanged after rejection
    assert_eq!(env.vault_balance(), vault_before, "vault_balance must be preserved after rejection");
    assert_eq!(env.read_insurance_balance(), insurance_before, "insurance_balance must be preserved after rejection");
}

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
    data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_staleness_secs
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(0u8); // invert
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
    data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6
    data.extend_from_slice(&100_000_000_000_000_000_000u128.to_le_bytes()); // max_maintenance_fee_per_slot
    data.extend_from_slice(&10_000_000_000_000_000u128.to_le_bytes()); // max_insurance_floor
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
    data.extend_from_slice(&100u128.to_le_bytes()); // min_initial_deposit
    data.extend_from_slice(&1u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&2u128.to_le_bytes()); // min_nonzero_im_req
    data.extend_from_slice(&10001u16.to_le_bytes()); // insurance_withdraw_max_bps > 10000
    data.extend_from_slice(&0u64.to_le_bytes()); // insurance_withdraw_cooldown_slots
    data.extend_from_slice(&u128::MAX.to_le_bytes()); // max_floor_change_per_day

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

/// Audit gap 3: Insurance floor is immutable after market initialization.
///
/// Spec behavior (ss2.2.1): insurance_floor is set at InitMarket and cannot
/// be changed afterwards.  SetRiskThreshold (tag 11) was removed and must
/// return InvalidInstructionData.  The stored insurance_floor must not change.
#[test]
fn test_insurance_floor_immutable_after_init() {
    program_path();

    let mut env = TestEnv::new();
    let floor_value: u128 = 1_000_000_000; // 1 SOL floor
    env.init_market_with_insurance_floor(0, floor_value);

    // Verify insurance_floor was stored correctly at init
    let floor_after_init = env.read_insurance_floor();
    assert_eq!(
        floor_after_init, floor_value,
        "insurance_floor should be set to the init value"
    );

    // Attempt to change insurance_floor via SetRiskThreshold (tag 11)
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let new_threshold: u128 = 5_000_000_000; // attempt to change to 5 SOL
    let result = env.try_set_risk_threshold(&admin, new_threshold);
    assert!(
        result.is_err(),
        "SetRiskThreshold must be rejected (insurance_floor is immutable per spec ss2.2.1)"
    );

    // Verify the floor has not changed
    let floor_after_attempt = env.read_insurance_floor();
    assert_eq!(
        floor_after_attempt, floor_value,
        "insurance_floor must remain unchanged after rejected SetRiskThreshold"
    );
}

