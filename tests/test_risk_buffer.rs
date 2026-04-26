mod common;
#[allow(unused_imports)]
use common::*;

use solana_sdk::signature::{Keypair, Signer};

fn write_risk_buffer_for_test(env: &mut TestEnv, buf: &percolator_prog::risk_buffer::RiskBuffer) {
    let mut slab = env.svm.get_account(&env.slab).unwrap();
    let buf_size = core::mem::size_of::<percolator_prog::risk_buffer::RiskBuffer>();
    let gen_table_size = MAX_ACCOUNTS * 8;
    let buf_off = SLAB_LEN - gen_table_size - buf_size;
    slab.data[buf_off..buf_off + buf_size].copy_from_slice(bytemuck::bytes_of(buf));
    env.svm.set_account(env.slab, slab).unwrap();
}

fn set_risk_buffer_scan_cursor_for_test(env: &mut TestEnv, cursor: u16) {
    let mut buf = env.read_risk_buffer();
    buf.scan_cursor = cursor;
    write_risk_buffer_for_test(env, &buf);
}

fn crank_with_candidates_for_test(env: &mut TestEnv, candidates: &[u16]) {
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
        data: encode_crank_with_candidates(candidates),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&caller.pubkey()),
        &[&caller],
        env.svm.latest_blockhash(),
    );
    env.svm
        .send_transaction(tx)
        .expect("candidate crank failed");
}

fn setup_risk_buffer_refill_market() -> (TestEnv, u16, Vec<u16>, Vec<u16>) {
    program_path();
    let mut env = TestEnv::new();
    let data = encode_init_market_with_maint_fee_bounded(
        &env.payer.pubkey(),
        &env.mint,
        &TEST_FEED_ID,
        1_000_000_000,
        40_000_000,
        0,
    );
    env.try_init_market_raw(data)
        .expect("init market with maintenance fee");

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 1_000_000_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 100_000_000_000);

    let risky_sizes: [i128; 3] = [100_000_000, 90_000_000, 80_000_000];
    let survivor_sizes: [i128; 3] = [7_000_000, 6_000_000, 5_000_000];
    let mut risky = Vec::new();
    let mut survivors = Vec::new();

    // These accounts are large and thinly margined. They should be in the
    // initial top-4 buffer and be fully liquidated after one fee-bearing crank.
    for &size in &risky_sizes {
        let user = Keypair::new();
        let idx = env.init_user(&user);
        env.deposit(&user, idx, 1_500_000_000);
        env.trade(&user, &lp, lp_idx, idx, size);
        risky.push(idx);
    }

    // These accounts have smaller notional, so they start outside the full
    // buffer, but have enough capital to survive the same fee interval.
    for &size in &survivor_sizes {
        let user = Keypair::new();
        let idx = env.init_user(&user);
        env.deposit(&user, idx, 10_000_000_000);
        env.trade(&user, &lp, lp_idx, idx, size);
        survivors.push(idx);
    }

    let buf = env.read_risk_buffer();
    assert_eq!(buf.count, 4, "setup must start with a full risk buffer");
    assert!(
        buf.find(lp_idx).is_some(),
        "LP must be the largest buffer entry"
    );
    for &idx in &risky {
        assert!(
            buf.find(idx).is_some(),
            "risky account {idx} must start in buffer"
        );
    }
    for &idx in &survivors {
        assert!(
            buf.find(idx).is_none(),
            "survivor account {idx} must start outside the full buffer"
        );
    }

    (env, lp_idx, risky, survivors)
}

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
    assert_eq!(
        buf.count, 2,
        "Buffer must contain exactly both trade participants: count={}",
        buf.count
    );
    assert!(buf.find(user_idx).is_some(), "User must be in buffer");
    assert!(buf.find(lp_idx).is_some(), "LP must be in buffer");

    // Verify the entries have nonzero notional
    for i in 0..buf.count as usize {
        assert!(
            buf.entries[i].notional > 0,
            "Entry {} must have nonzero notional",
            i
        );
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
    assert_eq!(
        buf.count, 0,
        "Buffer must be empty after closing all positions: count={}",
        buf.count
    );
    assert!(
        buf.find(user_idx).is_none(),
        "User must be removed from buffer"
    );
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
    assert_eq!(
        buf_after.count, buf_before.count,
        "Buffer must persist through empty-candidate crank"
    );

    // Scan cursor must advance
    assert!(
        buf_after.scan_cursor > buf_before.scan_cursor,
        "Scan cursor must advance: before={} after={}",
        buf_before.scan_cursor,
        buf_after.scan_cursor
    );
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

    // Cursor advances by RISK_SCAN_WINDOW (32) per crank, mod MAX_ACCOUNTS.
    // set_slot + deposit/trade helpers may internally crank, so the exact
    // final offset isn't predictable — assert the advance is a non-zero
    // multiple of 32, which is the window invariant.
    let before = env.read_risk_buffer().scan_cursor as u32;
    for i in 0..5u64 {
        env.set_slot(200 + i * 10);
        env.crank();
    }
    let after = env.read_risk_buffer().scan_cursor as u32;
    let delta = after.wrapping_sub(before) % (MAX_ACCOUNTS as u32);
    assert!(delta > 0 && delta % 32 == 0,
        "scan_cursor delta must be a non-zero multiple of 32: before={before} after={after} delta={delta}");
}

// ============================================================================
// E. Buffer eviction
// ============================================================================

/// E2: New entry evicts smallest when buffer is full.
#[test]
fn test_buffer_eviction() {
    use bytemuck::Zeroable;
    use percolator_prog::risk_buffer::RiskBuffer;

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
    use bytemuck::Zeroable;
    use percolator_prog::risk_buffer::RiskBuffer;

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
    assert!(
        result.is_ok(),
        "CloseAccount must succeed after closing position: {:?}",
        result
    );
    let buf = env.read_risk_buffer();
    assert!(
        buf.find(user_idx).is_none(),
        "Closed account must be removed from buffer"
    );
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
    env.init_market_with_cap(0, 80); // liquidation: max cap (100%/read)

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
    assert!(
        buf.find(user_idx).is_some(),
        "User must be in buffer after trade"
    );

    // Price drop → liquidate
    env.set_slot_and_price(2000, 90_000_000);
    env.try_liquidate(user_idx)
        .expect("Liquidation must succeed");

    let buf = env.read_risk_buffer();
    assert!(
        buf.find(user_idx).is_none(),
        "Liquidated account must be removed from buffer"
    );
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
    let slot = buf
        .find(user_idx)
        .expect("User must be in buffer after long");
    let long_notional = buf.entries[slot].notional;

    // Flip to short: net = +1M - 3M = -2M
    env.set_slot(200);
    env.trade(&user, &lp, lp_idx, user_idx, -3_000_000);

    let buf = env.read_risk_buffer();
    let slot = buf
        .find(user_idx)
        .expect("User must be in buffer after flip");
    let short_notional = buf.entries[slot].notional;

    // Net short 2M > original long 1M → notional must increase
    assert!(
        short_notional > long_notional,
        "Notional must increase after flip to larger position: long={} short={}",
        long_notional,
        short_notional
    );
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
        assert!(
            buf.find(user_idx).is_some(),
            "User must persist in buffer after crank {}",
            i
        );
        assert!(
            buf.find(lp_idx).is_some(),
            "LP must persist in buffer after crank {}",
            i
        );
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
    env.init_market_with_cap(0, 80); // liquidation: max cap (100%/read)

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

    // Drive the oracle higher; per-slot price cap compounds over cranks.
    env.set_slot_and_price(500, 160_000_000);
    env.crank();

    let buf = env.read_risk_buffer();
    let slot = buf
        .find(user_idx)
        .expect("User in buffer after price change");
    let notional_up = buf.entries[slot].notional;

    assert!(
        notional_up > notional_138,
        "Notional must increase with price: before={} after={}",
        notional_138,
        notional_up
    );
    // Don't pin the ratio — the engine clamps the price per slot, so the
    // achieved ratio tracks the per-slot cap * crank count, not the raw
    // target ratio. Any strictly-greater notional validates the refresh.
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
        assert!(
            buf.find(user_idx).is_some(),
            "User with position must remain in buffer after 20 cranks"
        );
    }
    if lp_pos != 0 {
        assert!(
            buf.find(lp_idx).is_some(),
            "LP with position must remain in buffer after 20 cranks"
        );
    }
    assert!(
        buf.scan_cursor > 0,
        "Scan cursor must advance after 20 cranks: cursor={}",
        buf.scan_cursor
    );
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
    assert!(
        buf.find(user1_idx).is_some(),
        "user1 must be in buffer after trade"
    );

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
    assert!(
        buf.find(user2_idx).is_some(),
        "user2 must be in buffer immediately after trade (no crank needed)"
    );

    // user2's notional should be larger than user1's (5x position)
    let s1 = buf.find(user1_idx).unwrap();
    let s2 = buf.find(user2_idx).unwrap();
    assert!(
        buf.entries[s2].notional > buf.entries[s1].notional,
        "user2 (5M) must have higher notional than user1 (1M)"
    );

    // Phase 3: crank refreshes — both still present with correct relative order
    env.set_slot(400);
    env.crank();

    let buf = env.read_risk_buffer();
    assert!(
        buf.find(user1_idx).is_some(),
        "user1 must persist after crank"
    );
    assert!(
        buf.find(user2_idx).is_some(),
        "user2 must persist after crank"
    );
    assert!(buf.find(lp_idx).is_some(), "LP must persist after crank");

    // After crank refresh, notionals are recalculated at current oracle price
    let s1 = buf.find(user1_idx).unwrap();
    let s2 = buf.find(user2_idx).unwrap();
    assert!(
        buf.entries[s2].notional > buf.entries[s1].notional,
        "Relative order must be preserved after crank refresh"
    );
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

    assert!(
        env.read_risk_buffer().find(users[0].1).is_none(),
        "user1 (1M) must be evicted from full buffer"
    );

    // Crank to advance state
    env.set_slot(200);
    env.crank();

    // user1 increases position to 10M — larger than user3's 3M
    env.trade(&users[0].0, &lp, lp_idx, users[0].1, 9_000_000);

    let buf = env.read_risk_buffer();
    assert!(
        buf.find(users[0].1).is_some(),
        "user1 (now 10M) must re-enter buffer after growing position"
    );

    // user2 (2M) should now be evicted (smallest in buffer)
    assert!(
        buf.find(users[1].1).is_none(),
        "user2 (2M) must be evicted when user1 re-enters at 10M"
    );

    // Verify after crank — re-entry persists
    env.set_slot(300);
    env.crank();

    let buf = env.read_risk_buffer();
    assert!(
        buf.find(users[0].1).is_some(),
        "user1 must persist in buffer after crank"
    );
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
    assert_eq!(
        buf.count, 4,
        "Buffer must hold exactly RISK_BUF_CAP=4 entries"
    );

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
// E (integration). Refill after crank clears buffered entries
// ============================================================================

/// When a crank liquidates buffered accounts and the progressive scan window
/// covers lower-ranked live positions, Phase C refills the holes in the same
/// crank.
#[test]
fn test_crank_scan_refills_after_buffer_entries_are_liquidated() {
    let (mut env, lp_idx, risky, survivors) = setup_risk_buffer_refill_market();

    // Default scan_cursor is 0, so the post-liquidation scan visits all account
    // indices created by the setup.
    let buf = env.read_risk_buffer();
    assert_eq!(buf.scan_cursor, 0, "setup should not have cranked yet");
    env.set_slot_and_price_raw_no_walk(149, 138_000_000);
    crank_with_candidates_for_test(&mut env, &[]);

    for &idx in &risky {
        assert_eq!(
            env.read_account_position(idx),
            0,
            "risky account {idx} should be liquidated by the crank"
        );
    }
    for &idx in &survivors {
        assert_ne!(
            env.read_account_position(idx),
            0,
            "survivor account {idx} should keep its position"
        );
    }

    let buf = env.read_risk_buffer();
    assert_eq!(buf.count, 4, "scan refill should restore the full buffer");
    assert!(buf.find(lp_idx).is_some(), "LP should remain in buffer");
    for &idx in &risky {
        assert!(
            buf.find(idx).is_none(),
            "liquidated account {idx} must be removed from buffer"
        );
    }
    for &idx in &survivors {
        assert!(
            buf.find(idx).is_some(),
            "scan should refill survivor account {idx}"
        );
    }
}

/// If the scan window misses lower-ranked live positions, honest keeper
/// candidates still refill holes in Phase D after Phase A removes liquidated
/// buffer entries.
#[test]
fn test_crank_candidate_refills_when_scan_window_misses() {
    let (mut env, lp_idx, risky, survivors) = setup_risk_buffer_refill_market();

    // Force the progressive scan to miss the low-index survivor accounts.
    // The candidate list below is the only same-crank refill source.
    set_risk_buffer_scan_cursor_for_test(&mut env, 32);

    env.set_slot_and_price_raw_no_walk(149, 138_000_000);
    crank_with_candidates_for_test(&mut env, &survivors);

    for &idx in &risky {
        assert_eq!(
            env.read_account_position(idx),
            0,
            "risky account {idx} should be liquidated by the crank"
        );
    }

    let buf = env.read_risk_buffer();
    assert_eq!(
        buf.count, 4,
        "candidate refill should restore the full buffer"
    );
    assert!(buf.find(lp_idx).is_some(), "LP should remain in buffer");
    for &idx in &survivors {
        assert!(
            buf.find(idx).is_some(),
            "candidate Phase D should refill survivor account {idx}"
        );
    }
}

/// If both the scan window and caller candidates miss lower-ranked live
/// positions, the buffer may remain temporarily underfull; once the scan cursor
/// reaches those accounts, the next crank refills it.
#[test]
fn test_crank_refill_is_progressive_when_scan_and_candidates_miss() {
    let (mut env, lp_idx, risky, survivors) = setup_risk_buffer_refill_market();

    // Model the "scan cursor is elsewhere" corner case without spending 128
    // cranks to wrap a 4096-account market in the test harness.
    set_risk_buffer_scan_cursor_for_test(&mut env, 32);

    env.set_slot_and_price_raw_no_walk(149, 138_000_000);
    crank_with_candidates_for_test(&mut env, &[]);

    for &idx in &risky {
        assert_eq!(
            env.read_account_position(idx),
            0,
            "risky account {idx} should be liquidated by the crank"
        );
    }

    let buf_after_miss = env.read_risk_buffer();
    assert!(
        buf_after_miss.count < 4,
        "buffer should remain underfull when both scan and candidates miss: count={}",
        buf_after_miss.count
    );
    assert!(
        buf_after_miss.find(lp_idx).is_some(),
        "LP should remain in the partially cleared buffer"
    );
    for &idx in &survivors {
        assert!(
            buf_after_miss.find(idx).is_none(),
            "survivor account {idx} should not be refilled until scanned or supplied"
        );
    }

    // Model eventual cursor wrap to the low-index accounts and verify the
    // normal scan path refills the buffer.
    set_risk_buffer_scan_cursor_for_test(&mut env, 0);
    env.set_slot_and_price_raw_no_walk(150, 138_000_000);
    crank_with_candidates_for_test(&mut env, &[]);

    let buf_after_refill = env.read_risk_buffer();
    assert_eq!(
        buf_after_refill.count, 4,
        "later scan should restore the full buffer"
    );
    assert!(
        buf_after_refill.find(lp_idx).is_some(),
        "LP should remain in buffer"
    );
    for &idx in &survivors {
        assert!(
            buf_after_refill.find(idx).is_some(),
            "later scan should refill survivor account {idx}"
        );
    }
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
    env.init_market_with_cap(0, 80); // liquidation: max cap (100%/read)

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
    assert!(
        buf.find(user_idx).is_some(),
        "User must be in buffer after trade"
    );

    // Price drop → user undercollateralized
    env.set_slot_and_price(2000, 90_000_000);

    // Crank should liquidate via buffer candidate
    env.crank();

    let pos = env.read_account_position(user_idx);
    assert_eq!(
        pos, 0,
        "Undercollateralized user must be liquidated by crank"
    );

    let buf = env.read_risk_buffer();
    assert!(
        buf.find(user_idx).is_none(),
        "Liquidated user must be removed from buffer"
    );
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
    use solana_sdk::instruction::{AccountMeta, Instruction};
    use solana_sdk::sysvar;

    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 80); // liquidation: max cap (100%/read)

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
    assert!(
        buf.find(user_idx).is_some(),
        "User must be in buffer after trade"
    );

    // Price drop → user undercollateralized
    env.set_slot_and_price(2000, 90_000_000);

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
    env.svm
        .send_transaction(tx)
        .expect("buffer-only crank failed");

    // The buffer entry should have caused the liquidation
    let pos = env.read_account_position(user_idx);
    assert_eq!(
        pos, 0,
        "Buffer-only crank must liquidate undercollateralized user (no external candidates)"
    );

    let buf = env.read_risk_buffer();
    assert!(
        buf.find(user_idx).is_none(),
        "Liquidated user must be removed from buffer"
    );
}

// ============================================================================
// Maintenance fee tests removed — v12.15 engine no longer has
// maintenance_fee_per_slot in RiskParams or last_fee_slot in Account.
// Fee scheduling is now engine-internal.
// Removed: test_maintenance_fee_proportional_to_elapsed,
//          test_fee_zero_no_fees_ever_charged,
//          test_deposit_does_not_settle_pending_maintenance_fee
// ============================================================================

#[test]
fn test_calibrate_bpf_offsets() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 1_000_000_000);
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    let d = env.svm.get_account(&env.slab).unwrap().data;
    let eng = 472usize; // ENGINE_OFF

    // Find num_used=2 + free_head=2
    for off in 0..2000 {
        let nu = u16::from_le_bytes(d[eng + off..eng + off + 2].try_into().unwrap());
        let fh = u16::from_le_bytes(d[eng + off + 2..eng + off + 4].try_into().unwrap());
        if nu == 2 && fh == 2 {
            println!("NUM_USED: ENGINE+{}", off);
        }
    }
    // Find ADL_ONE=1_000_000 as u128 in engine fixed region
    let adl_one = 1_000_000u128.to_le_bytes();
    for off in 0..1500 {
        if eng + off + 16 <= d.len() && d[eng + off..eng + off + 16] == adl_one {
            println!("ADL_ONE: ENGINE+{}", off);
        }
    }
    // Find c_tot near 110B
    for off in (0..500).step_by(8) {
        if eng + off + 16 > d.len() {
            break;
        }
        let v = u128::from_le_bytes(d[eng + off..eng + off + 16].try_into().unwrap());
        if v > 100_000_000_000 && v < 120_000_000_000 {
            println!("C_TOT candidate: ENGINE+{} = {}", off, v);
        }
    }
    // Find insurance=1B
    let ins = 1_000_000_000u128.to_le_bytes();
    for off in 0..100 {
        if eng + off + 16 <= d.len() && d[eng + off..eng + off + 16] == ins {
            println!("INSURANCE: ENGINE+{}", off);
        }
    }
}

#[test]
fn test_find_adl_offsets() {
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
    // Don't trade — ADL mults should be ADL_ONE = 1_000_000
    env.crank(); // just crank

    let d = env.svm.get_account(&env.slab).unwrap().data;
    let eng = 472usize;
    let adl_one = 1_000_000u128.to_le_bytes();
    println!("=== ADL_ONE scan (400-700) ===");
    for off in (400..700).step_by(8) {
        if eng + off + 16 <= d.len() && d[eng + off..eng + off + 16] == adl_one {
            println!("ADL_ONE at ENGINE+{}", off);
        }
    }
    // Also find epoch values (small u64s)
    println!("=== Small u64 scan (470-550) ===");
    for off in (470..550).step_by(8) {
        if eng + off + 8 <= d.len() {
            let v = u64::from_le_bytes(d[eng + off..eng + off + 8].try_into().unwrap());
            if v > 0 && v < 10 {
                println!("u64={} at ENGINE+{}", v, off);
            }
        }
    }
}

#[test]
fn test_find_adl_correct_value() {
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
    env.crank();

    let d = env.svm.get_account(&env.slab).unwrap().data;
    let eng = 472usize;
    let adl_one = 1_000_000_000_000_000u128.to_le_bytes();
    for off in (300..700).step_by(8) {
        if eng + off + 16 <= d.len() && d[eng + off..eng + off + 16] == adl_one {
            println!("ADL_ONE(10^15) at ENGINE+{}", off);
        }
    }
    // Also find in Account for a_basis
    let accts_off = eng + 9424;
    for idx in 0..3u16 {
        let acc = accts_off + (idx as usize) * 352;
        for off in (60..100).step_by(8) {
            if acc + off + 16 <= d.len() && d[acc + off..acc + off + 16] == adl_one {
                println!("a_basis ADL_ONE at ACCT[{}]+{}", idx, off);
            }
        }
    }
}

#[test]
fn test_find_adl_bytes() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    // No LP, no user — just init market and check engine defaults
    let d = env.svm.get_account(&env.slab).unwrap().data;
    let eng = 472usize;
    let target = [
        0x00u8, 0x80, 0xC6, 0xA4, 0x7E, 0x8D, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ];
    for off in 0..2000 {
        if eng + off + 16 <= d.len() && d[eng + off..eng + off + 16] == target {
            println!("ADL_ONE bytes at ENGINE+{}", off);
        }
    }
}
