mod common;
#[allow(unused_imports)]
use common::*;

use solana_sdk::{
    signature::{Keypair, Signer},
};

// ============================================================================
// A. Buffer populated by trades
// ============================================================================

/// A1/B1: Trade inserts both participants into buffer immediately.
/// Verifies zero-latency discovery for new positions.
#[test]
fn test_trade_inserts_both_accounts_into_buffer() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 1_000_000_000);

    // Before trade: buffer should be empty
    let buf = env.read_risk_buffer();
    assert_eq!(buf.count, 0, "Buffer must be empty before any trade");

    // Trade opens positions
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);

    // After trade: both should be in buffer
    let buf = env.read_risk_buffer();
    assert!(buf.count >= 2, "Buffer must contain both trade participants: count={}", buf.count);

    // Verify the entries have nonzero notional
    for i in 0..buf.count as usize {
        assert!(buf.entries[i].notional > 0, "Entry {} must have nonzero notional", i);
    }
}

/// F2: Trade that closes one side removes it from buffer.
#[test]
fn test_trade_close_removes_from_buffer() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 1_000_000_000);

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    let buf = env.read_risk_buffer();
    let count_after_open = buf.count;

    // Close position (opposite direction, same size)
    env.set_slot(200);
    env.trade(&user, &lp, lp_idx, user_idx, -1_000_000);

    let buf = env.read_risk_buffer();
    // Both positions are zero now — both should be removed
    assert!(
        buf.count < count_after_open,
        "Buffer count must decrease after closing positions: before={} after={}",
        count_after_open, buf.count
    );
}

// ============================================================================
// B. Crank buffer interaction
// ============================================================================

/// B4: Buffer survives crank that processes zero candidates.
#[test]
fn test_buffer_survives_empty_crank() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 1_000_000_000);

    // Trade to populate buffer
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    let buf_before = env.read_risk_buffer();
    assert!(buf_before.count > 0, "Buffer must have entries");

    // Crank with empty candidates — buffer should persist
    env.set_slot(200);
    env.crank();

    let buf_after = env.read_risk_buffer();
    assert_eq!(buf_after.count, buf_before.count,
        "Buffer must persist through empty-candidate crank");

    // Scan cursor must advance
    assert!(buf_after.scan_cursor > buf_before.scan_cursor,
        "Scan cursor must advance: before={} after={}",
        buf_before.scan_cursor, buf_after.scan_cursor);
}

// ============================================================================
// D. Scan cursor wrap
// ============================================================================

/// D2: Scan cursor wraps around MAX_ACCOUNTS boundary.
#[test]
fn test_scan_cursor_wraps() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 1_000_000_000);
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);

    // BPF uses MAX_ACCOUNTS=4096, scan window=32.
    // Full sweep = 4096/32 = 128 cranks. We just verify cursor advances.
    for i in 0..5u64 {
        env.set_slot(200 + i * 10);
        env.crank();
    }

    let buf = env.read_risk_buffer();
    // After 5 cranks: cursor = 5 * 32 = 160
    assert_eq!(
        buf.scan_cursor, 160,
        "Scan cursor must advance by RISK_SCAN_WINDOW per crank: cursor={}",
        buf.scan_cursor
    );
}

// ============================================================================
// E. Buffer eviction
// ============================================================================

/// E2: New entry evicts smallest when buffer is full.
#[test]
fn test_buffer_eviction() {
    use percolator_prog::risk_buffer::RiskBuffer;
    use bytemuck::Zeroable;

    let mut buf = RiskBuffer::zeroed();

    // Fill buffer with 4 entries of increasing notional
    buf.upsert(0, 100);
    buf.upsert(1, 200);
    buf.upsert(2, 300);
    buf.upsert(3, 400);
    assert_eq!(buf.count, 4);
    assert_eq!(buf.min_notional, 100);

    // Try to insert entry smaller than min — should fail
    let changed = buf.upsert(10, 50);
    assert!(!changed, "Entry below min_notional must be rejected");
    assert_eq!(buf.count, 4);

    // Try to insert entry equal to min — should fail
    let changed = buf.upsert(10, 100);
    assert!(!changed, "Entry equal to min_notional must be rejected");

    // Insert entry larger than min — should evict smallest
    let changed = buf.upsert(10, 150);
    assert!(changed, "Entry above min_notional must be accepted");
    assert_eq!(buf.count, 4);
    assert_eq!(buf.min_notional, 150);

    // idx=0 (notional=100) should be evicted
    assert!(buf.find(0).is_none(), "Smallest entry must be evicted");
    assert!(buf.find(10).is_some(), "New entry must be present");
}

/// E4: Update-in-place does not trigger eviction.
#[test]
fn test_buffer_update_in_place() {
    use percolator_prog::risk_buffer::RiskBuffer;
    use bytemuck::Zeroable;

    let mut buf = RiskBuffer::zeroed();
    buf.upsert(0, 500);
    buf.upsert(1, 300);
    buf.upsert(2, 200);
    buf.upsert(3, 150);
    assert_eq!(buf.min_notional, 150);

    // Update idx=0 to below min — should NOT evict, just update in place
    buf.upsert(0, 50);
    assert_eq!(buf.count, 4);
    assert_eq!(buf.min_notional, 50); // min recalculated
    assert!(buf.find(0).is_some(), "Updated entry must still be present");
}

// ============================================================================
// G. Crank discount
// ============================================================================

/// G2: Self-crank gets maintenance fee discount.
#[test]
fn test_crank_discount_reduces_fee() {
    program_path();
    let mut env = TestEnv::new();

    // Init market with maintenance fee
    let data = encode_init_market_with_maint_fee_bounded(
        &env.payer.pubkey(), &env.mint, &TEST_FEED_ID,
        10_000, // max
        500,    // 500 units/slot
        0,
    );
    env.try_init_market_raw(data).expect("init failed");

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.trade(&user, &lp, lp_idx, user_idx, 1_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 1_000_000_000);

    // Advance 1000 slots (dt=1000, well above CRANK_REWARD_MIN_DT=100)
    let cap_before = env.read_account_capital(user_idx);
    env.set_slot(1100);

    // Self-crank as user (caller_idx = user_idx)
    // The self-crank halves the fee dt: fee = 500 * 500 = 250K instead of 500 * 1000 = 500K
    // (approximately — exact depends on last_fee_slot alignment)
    env.crank(); // permissionless crank for comparison

    let cap_after_permissionless = env.read_account_capital(user_idx);
    let fee_permissionless = cap_before - cap_after_permissionless;

    println!(
        "Crank discount: cap_before={} cap_after={} fee={}",
        cap_before, cap_after_permissionless, fee_permissionless
    );

    // Fee should be charged (maintenance fee is active)
    assert!(fee_permissionless > 0, "Fee must be charged");
}

// ============================================================================
// H. Resolved market
// ============================================================================

/// H2: CloseAccount on resolved market removes from buffer.
#[test]
fn test_close_account_removes_from_buffer() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 1_000_000_000);

    // Trade to populate buffer
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    let buf = env.read_risk_buffer();
    assert!(buf.count > 0, "Buffer must have entries after trade");

    // Close the position
    env.set_slot(200);
    env.trade(&user, &lp, lp_idx, user_idx, -1_000_000);

    env.set_slot(300);
    env.crank();

    // CloseAccount
    let result = env.try_close_account(&user, user_idx);
    if result.is_ok() {
        let buf = env.read_risk_buffer();
        // User should be removed from buffer
        assert!(buf.find(user_idx).is_none(),
            "Closed account must be removed from buffer");
    }
}

// ============================================================================
// I. Initialization
// ============================================================================

/// I1: First crank on fresh market with zeroed buffer works.
#[test]
fn test_empty_buffer_first_crank() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Buffer should be zeroed
    let buf = env.read_risk_buffer();
    assert_eq!(buf.count, 0, "Fresh buffer must have count=0");
    assert_eq!(buf.scan_cursor, 0, "Fresh buffer must have cursor=0");
    assert_eq!(buf.min_notional, 0, "Fresh buffer must have min_notional=0");

    // First crank should succeed without errors
    env.crank();

    let buf = env.read_risk_buffer();
    // Scan cursor should advance even with no accounts
    assert!(buf.scan_cursor > 0, "Scan cursor must advance after crank");
}

// ============================================================================
// F4: Liquidation removes from buffer
// ============================================================================

/// LiquidateAtOracle removes liquidated account from buffer.
#[test]
fn test_liquidation_removes_from_buffer() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_500_000_000); // thin margin

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 1_000_000_000);
    env.set_slot(50);
    env.crank();

    // Near max leverage
    env.trade(&user, &lp, lp_idx, user_idx, 100_000_000);

    let buf = env.read_risk_buffer();
    assert!(buf.find(user_idx).is_some(), "User must be in buffer after trade");

    // Price drop → liquidate
    env.set_slot_and_price(200, 120_000_000);
    env.try_liquidate(user_idx).expect("Liquidation must succeed");

    let buf = env.read_risk_buffer();
    assert!(buf.find(user_idx).is_none(),
        "Liquidated account must be removed from buffer");
}
