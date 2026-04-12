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

    // After trade: exactly both should be in buffer
    let buf = env.read_risk_buffer();
    assert_eq!(buf.count, 2, "Buffer must contain exactly both trade participants: count={}", buf.count);
    assert!(buf.find(user_idx).is_some(), "User must be in buffer");
    assert!(buf.find(lp_idx).is_some(), "LP must be in buffer");

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
    let _count_after_open = buf.count;

    // Close position (opposite direction, same size)
    env.set_slot(200);
    env.trade(&user, &lp, lp_idx, user_idx, -1_000_000);

    let buf = env.read_risk_buffer();
    // Both positions are zero now — both must be removed
    assert_eq!(buf.count, 0,
        "Buffer must be empty after closing all positions: count={}", buf.count);
    assert!(buf.find(user_idx).is_none(), "User must be removed from buffer");
    assert!(buf.find(lp_idx).is_none(), "LP must be removed from buffer");
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
// G. Crank discount (removed in v12.15 — last_fee_slot gone from Account)
// Tests test_self_crank_halves_maintenance_fee and test_crank_discount_requires_min_dt
// were removed because the engine no longer has per-account last_fee_slot or
// wrapper-level crank discount. Maintenance fees moved to engine-internal scheduling.
// ============================================================================

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

    // Additional crank to fully settle
    env.set_slot(400);
    env.crank();

    // CloseAccount
    env.set_slot(500);
    let result = env.try_close_account(&user, user_idx);
    assert!(result.is_ok(),
        "CloseAccount must succeed after closing position: {:?}", result);
    let buf = env.read_risk_buffer();
    assert!(buf.find(user_idx).is_none(),
        "Closed account must be removed from buffer");
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

// ============================================================================
// F4. Position flip updates buffer
// ============================================================================

/// Position flip (long→short) updates buffer entry notional.
#[test]
fn test_position_flip_updates_buffer() {
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

    // Go long 1M
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    let buf = env.read_risk_buffer();
    let slot = buf.find(user_idx).expect("User must be in buffer after long");
    let long_notional = buf.entries[slot].notional;

    // Flip to short: net = +1M - 3M = -2M
    env.set_slot(200);
    env.trade(&user, &lp, lp_idx, user_idx, -3_000_000);

    let buf = env.read_risk_buffer();
    let slot = buf.find(user_idx).expect("User must be in buffer after flip");
    let short_notional = buf.entries[slot].notional;

    // Net short 2M > original long 1M → notional must increase
    assert!(short_notional > long_notional,
        "Notional must increase after flip to larger position: long={} short={}",
        long_notional, short_notional);
}

// ============================================================================
// B1. Buffer entries persist across cranks
// ============================================================================

/// Buffer entries with open positions survive multiple cranks.
#[test]
fn test_buffer_entries_persist_across_cranks() {
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

    // Both accounts must persist through 5 cranks
    for i in 1..=5u64 {
        env.set_slot(200 + i * 100);
        env.crank();

        let buf = env.read_risk_buffer();
        assert!(buf.find(user_idx).is_some(),
            "User must persist in buffer after crank {}", i);
        assert!(buf.find(lp_idx).is_some(),
            "LP must persist in buffer after crank {}", i);
    }
}

// ============================================================================
// B2. Buffer notional refreshed on price change
// ============================================================================

/// Crank refreshes buffer notional when oracle price moves.
#[test]
fn test_buffer_notional_refreshed_on_price_change() {
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

    // Notional at initial price (~$138)
    let buf = env.read_risk_buffer();
    let slot = buf.find(user_idx).expect("User in buffer");
    let notional_138 = buf.entries[slot].notional;

    // Price up to $200, crank refreshes notional
    env.set_slot_and_price(200, 200_000_000);
    env.crank();

    let buf = env.read_risk_buffer();
    let slot = buf.find(user_idx).expect("User in buffer after price change");
    let notional_200 = buf.entries[slot].notional;

    assert!(notional_200 > notional_138,
        "Notional must increase with price: at_138={} at_200={}",
        notional_138, notional_200);

    // Ratio ~200/138 ≈ 1.45
    let ratio = (notional_200 as f64) / (notional_138 as f64);
    assert!(ratio > 1.3 && ratio < 1.6,
        "Notional ratio must track price ratio (~1.45): ratio={:.2}", ratio);
}

// ============================================================================
// B5. Buffer correct after many cranks
// ============================================================================

/// Buffer remains consistent through 20 crank cycles.
#[test]
fn test_buffer_correct_after_many_cranks() {
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

    for i in 1..=20u64 {
        env.set_slot(200 + i * 100);
        env.crank();
    }

    let buf = env.read_risk_buffer();
    let user_pos = env.read_account_position(user_idx);
    let lp_pos = env.read_account_position(lp_idx);

    if user_pos != 0 {
        assert!(buf.find(user_idx).is_some(),
            "User with position must remain in buffer after 20 cranks");
    }
    if lp_pos != 0 {
        assert!(buf.find(lp_idx).is_some(),
            "LP with position must remain in buffer after 20 cranks");
    }
    assert!(buf.scan_cursor > 0,
        "Scan cursor must advance after 20 cranks: cursor={}", buf.scan_cursor);
}

// ============================================================================
// B6. New position between cranks enters buffer
// ============================================================================

/// A new position opened between cranks is immediately reflected in the buffer
/// (via the trade handler), and survives subsequent cranks with correct notional.
#[test]
fn test_new_position_between_cranks_enters_buffer() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 10_000_000_000);

    // Phase 1: user1 trades, enters buffer
    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 10_000_000_000);
    env.trade(&user1, &lp, lp_idx, user1_idx, 1_000_000);

    let buf = env.read_risk_buffer();
    assert!(buf.find(user1_idx).is_some(), "user1 must be in buffer after trade");

    // Crank a few times — user1 and LP persist
    env.set_slot(200);
    env.crank();
    env.set_slot(300);
    env.crank();

    // Phase 2: user2 opens a LARGER position between cranks
    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 10_000_000_000);
    env.trade(&user2, &lp, lp_idx, user2_idx, 5_000_000);

    // Trade handler should immediately insert user2
    let buf = env.read_risk_buffer();
    assert!(buf.find(user2_idx).is_some(),
        "user2 must be in buffer immediately after trade (no crank needed)");

    // user2's notional should be larger than user1's (5x position)
    let s1 = buf.find(user1_idx).unwrap();
    let s2 = buf.find(user2_idx).unwrap();
    assert!(buf.entries[s2].notional > buf.entries[s1].notional,
        "user2 (5M) must have higher notional than user1 (1M)");

    // Phase 3: crank refreshes — both still present with correct relative order
    env.set_slot(400);
    env.crank();

    let buf = env.read_risk_buffer();
    assert!(buf.find(user1_idx).is_some(), "user1 must persist after crank");
    assert!(buf.find(user2_idx).is_some(), "user2 must persist after crank");
    assert!(buf.find(lp_idx).is_some(), "LP must persist after crank");

    // After crank refresh, notionals are recalculated at current oracle price
    let s1 = buf.find(user1_idx).unwrap();
    let s2 = buf.find(user2_idx).unwrap();
    assert!(buf.entries[s2].notional > buf.entries[s1].notional,
        "Relative order must be preserved after crank refresh");
}

/// An evicted account re-enters the buffer when its position grows via new trade.
#[test]
fn test_evicted_account_reenters_on_larger_trade() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 10_000_000_000);

    // Create 5 users — user1 (smallest) gets evicted
    let sizes: [i128; 5] = [1_000_000, 2_000_000, 3_000_000, 4_000_000, 5_000_000];
    let mut users = Vec::new();
    for &size in &sizes {
        let u = Keypair::new();
        let idx = env.init_user(&u);
        env.deposit(&u, idx, 10_000_000_000);
        env.trade(&u, &lp, lp_idx, idx, size);
        users.push((u, idx));
    }

    assert!(env.read_risk_buffer().find(users[0].1).is_none(),
        "user1 (1M) must be evicted from full buffer");

    // Crank to advance state
    env.set_slot(200);
    env.crank();

    // user1 increases position to 10M — larger than user3's 3M
    env.trade(&users[0].0, &lp, lp_idx, users[0].1, 9_000_000);

    let buf = env.read_risk_buffer();
    assert!(buf.find(users[0].1).is_some(),
        "user1 (now 10M) must re-enter buffer after growing position");

    // user2 (2M) should now be evicted (smallest in buffer)
    assert!(buf.find(users[1].1).is_none(),
        "user2 (2M) must be evicted when user1 re-enters at 10M");

    // Verify after crank — re-entry persists
    env.set_slot(300);
    env.crank();

    let buf = env.read_risk_buffer();
    assert!(buf.find(users[0].1).is_some(),
        "user1 must persist in buffer after crank");
}

// ============================================================================
// E (integration). Five accounts → buffer keeps top 4
// ============================================================================

/// With 5 users of increasing position size, buffer evicts the two smallest.
#[test]
fn test_buffer_with_five_accounts_evicts_smallest() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 10_000_000_000);

    // Create 5 users with increasing position sizes
    let sizes: [i128; 5] = [1_000_000, 2_000_000, 3_000_000, 4_000_000, 5_000_000];
    let mut idxs = Vec::new();
    for &size in &sizes {
        let u = Keypair::new();
        let idx = env.init_user(&u);
        env.deposit(&u, idx, 10_000_000_000);
        env.trade(&u, &lp, lp_idx, idx, size);
        idxs.push(idx);
    }

    let buf = env.read_risk_buffer();
    assert_eq!(buf.count, 4, "Buffer must hold exactly RISK_BUF_CAP=4 entries");

    // LP (|15M| short) is the largest — must be in buffer
    assert!(buf.find(lp_idx).is_some(), "LP (largest) must be in buffer");

    // Top 3 users by size must be in buffer
    assert!(buf.find(idxs[4]).is_some(), "user5 (5M) must be in buffer");
    assert!(buf.find(idxs[3]).is_some(), "user4 (4M) must be in buffer");
    assert!(buf.find(idxs[2]).is_some(), "user3 (3M) must be in buffer");

    // Smallest two evicted
    assert!(buf.find(idxs[0]).is_none(), "user1 (1M) must be evicted");
    assert!(buf.find(idxs[1]).is_none(), "user2 (2M) must be evicted");
}

// ============================================================================
// K1. Crank liquidates undercollateralized buffer entry
// ============================================================================

/// Crank uses buffer entries as liquidation candidates.
/// An undercollateralized buffer entry gets liquidated and removed.
#[test]
fn test_crank_liquidates_undercollateralized_buffer_entry() {
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
    env.top_up_insurance(&admin, 10_000_000_000);

    env.set_slot(50);
    env.crank();

    // Near-max leverage
    env.trade(&user, &lp, lp_idx, user_idx, 100_000_000);

    let buf = env.read_risk_buffer();
    assert!(buf.find(user_idx).is_some(), "User must be in buffer after trade");

    // Price drop → user undercollateralized
    env.set_slot_and_price(200, 120_000_000);

    // Crank should liquidate via buffer candidate
    env.crank();

    let pos = env.read_account_position(user_idx);
    assert_eq!(pos, 0, "Undercollateralized user must be liquidated by crank");

    let buf = env.read_risk_buffer();
    assert!(buf.find(user_idx).is_none(),
        "Liquidated user must be removed from buffer");
}

// ============================================================================
// K2. Buffer-ONLY liquidation (no external candidates)
// ============================================================================

/// Crank with EMPTY external candidates still liquidates via buffer entries.
/// This is the buffer's core value: it discovers accounts the off-chain keeper
/// didn't include. Without buffer, an empty-candidate crank would skip all
/// liquidations.
#[test]
fn test_buffer_only_liquidation_no_external_candidates() {
    use solana_sdk::instruction::{Instruction, AccountMeta};
    use solana_sdk::sysvar;

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
    env.top_up_insurance(&admin, 10_000_000_000);

    // Baseline crank to establish state
    env.set_slot(50);
    env.crank();

    // Near-max leverage trade → user enters buffer
    env.trade(&user, &lp, lp_idx, user_idx, 100_000_000);
    let buf = env.read_risk_buffer();
    assert!(buf.find(user_idx).is_some(), "User must be in buffer after trade");

    // Price drop → user undercollateralized
    env.set_slot_and_price(200, 120_000_000);

    // Crank with EMPTY candidate list — only buffer entries drive liquidation
    let caller = Keypair::new();
    env.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(caller.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_crank_with_candidates(&[]), // EMPTY — no external candidates
    };
    let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&caller.pubkey()),
        &[&caller],
        env.svm.latest_blockhash(),
    );
    env.svm.send_transaction(tx).expect("buffer-only crank failed");

    // The buffer entry should have caused the liquidation
    let pos = env.read_account_position(user_idx);
    assert_eq!(pos, 0,
        "Buffer-only crank must liquidate undercollateralized user (no external candidates)");

    let buf = env.read_risk_buffer();
    assert!(buf.find(user_idx).is_none(),
        "Liquidated user must be removed from buffer");
}

// ============================================================================
// Maintenance fee tests removed — v12.15 engine no longer has
// maintenance_fee_per_slot in RiskParams or last_fee_slot in Account.
// Fee scheduling is now engine-internal.
// Removed: test_maintenance_fee_proportional_to_elapsed,
//          test_fee_zero_no_fees_ever_charged,
//          test_deposit_does_not_settle_pending_maintenance_fee
// ============================================================================

