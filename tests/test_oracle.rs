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

/// Security Issue: Hyperp mode requires non-zero initial_mark_price_e6
///
/// If Hyperp mode is enabled (index_feed_id == [0; 32]) but initial_mark_price_e6 == 0,
/// the market would have no valid price and trades would fail with OracleInvalid.
/// This test verifies the validation in InitMarket rejects this configuration.
#[test]
fn test_hyperp_rejects_zero_initial_mark_price() {
    let path = program_path();

    let mut svm = LiteSVM::new();
    let program_id = Pubkey::new_unique();
    let program_bytes = std::fs::read(&path).expect("Failed to read program");
    svm.add_program(program_id, &program_bytes);

    let payer = Keypair::new();
    let slab = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let (vault_pda, _) = Pubkey::find_program_address(&[b"vault", slab.as_ref()], &program_id);
    let vault = Pubkey::new_unique();

    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();

    svm.set_account(
        slab,
        Account {
            lamports: 1_000_000_000,
            data: vec![0u8; SLAB_LEN],
            owner: program_id,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    svm.set_account(
        mint,
        Account {
            lamports: 1_000_000,
            data: make_mint_data(),
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    svm.set_account(
        vault,
        Account {
            lamports: 1_000_000,
            data: make_token_account_data(&mint, &vault_pda, 0),
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
            data: vec![0u8; TokenAccount::LEN],
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    svm.set_sysvar(&Clock {
        slot: 100,
        unix_timestamp: 100,
        ..Clock::default()
    });

    // Try to init market with Hyperp mode (feed_id = 0) but initial_mark_price = 0
    // This should FAIL because Hyperp mode requires a non-zero initial price
    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(slab, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
        ],
        data: encode_init_market_full_v2(
            &payer.pubkey(),
            &mint,
            &[0u8; 32], // Hyperp mode: feed_id = 0
            0,          // invert
            0,          // initial_mark_price_e6 = 0 (INVALID for Hyperp!)
            0,          // warmup
        ),
    };

    // Snapshot state before the failing init attempt.
    // Header+config region should remain unchanged on rejected tx.
    const HEADER_CONFIG_LEN: usize = 584;
    let NUM_USED_OFF: usize = common::ENGINE_OFFSET + common::ENGINE_NUM_USED_OFFSET;
    let slab_before = svm.get_account(&slab).unwrap().data;
    let vault_before = {
        let vault_data = svm.get_account(&vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    let used_before = u16::from_le_bytes(
        slab_before[NUM_USED_OFF..NUM_USED_OFF + 2]
            .try_into()
            .unwrap(),
    );

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&payer.pubkey()),
        &[&payer],
        svm.latest_blockhash(),
    );

    let result = svm.send_transaction(tx);

    assert!(
        result.is_err(),
        "SECURITY: InitMarket should reject Hyperp mode with zero initial_mark_price_e6. \
         Got: {:?}",
        result
    );

    let slab_after = svm.get_account(&slab).unwrap().data;
    let vault_after = {
        let vault_data = svm.get_account(&vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    let used_after = u16::from_le_bytes(
        slab_after[NUM_USED_OFF..NUM_USED_OFF + 2]
            .try_into()
            .unwrap(),
    );

    assert_eq!(
        &slab_after[..HEADER_CONFIG_LEN],
        &slab_before[..HEADER_CONFIG_LEN],
        "Rejected Hyperp init must not mutate slab header/config"
    );
    assert_eq!(
        used_after, used_before,
        "Rejected Hyperp init must not change num_used_accounts"
    );
    assert_eq!(
        vault_after, vault_before,
        "Rejected Hyperp init must not move vault tokens"
    );

    println!("HYPERP VALIDATION VERIFIED: Rejects zero initial_mark_price_e6 in Hyperp mode");
}

/// Test: Hyperp mode InitMarket succeeds with valid initial_mark_price
#[test]
fn test_hyperp_init_market_with_valid_price() {
    let path = program_path();

    let mut svm = LiteSVM::new();
    let program_id = Pubkey::new_unique();
    let program_bytes = std::fs::read(&path).expect("Failed to read program");
    svm.add_program(program_id, &program_bytes);

    let payer = Keypair::new();
    let slab = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let (vault_pda, _) = Pubkey::find_program_address(&[b"vault", slab.as_ref()], &program_id);
    let vault = Pubkey::new_unique();

    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();

    svm.set_account(
        slab,
        Account {
            lamports: 1_000_000_000,
            data: vec![0u8; SLAB_LEN],
            owner: program_id,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    svm.set_account(
        mint,
        Account {
            lamports: 1_000_000,
            data: make_mint_data(),
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    svm.set_account(
        vault,
        Account {
            lamports: 1_000_000,
            data: make_token_account_data(&mint, &vault_pda, 0),
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
            data: vec![0u8; TokenAccount::LEN],
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    svm.set_sysvar(&Clock {
        slot: 100,
        unix_timestamp: 100,
        ..Clock::default()
    });

    // Init market with Hyperp mode and valid initial_mark_price
    let initial_price_e6 = 100_000_000u64; // $100 in e6 format

    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(slab, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
        ],
        data: encode_init_market_full_v2(
            &payer.pubkey(),
            &mint,
            &[0u8; 32],       // Hyperp mode: feed_id = 0
            0,                // invert
            initial_price_e6, // Valid initial mark price
            0,                // warmup
        ),
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&payer.pubkey()),
        &[&payer],
        svm.latest_blockhash(),
    );

    let result = svm.send_transaction(tx);

    assert!(
        result.is_ok(),
        "Hyperp InitMarket with valid initial_mark_price should succeed. Got: {:?}",
        result
    );

    // Verify actual initialized config state, not just tx success.
    let slab_data = svm.get_account(&slab).unwrap().data;
    let magic = u64::from_le_bytes(slab_data[0..8].try_into().unwrap());
    let config = percolator_prog::state::read_config(&slab_data);
    let mark = config.hyperp_mark_e6;
    let index = config.last_effective_price_e6;
    let cap_off = common::ENGINE_OFFSET + 32 + 160;
    let cap = u64::from_le_bytes(slab_data[cap_off..cap_off + 8].try_into().unwrap());
    const FEED_ID_OFF: usize = 136 + 64;
    const INVERT_OFF: usize = 136 + 107;
    let used_off = common::ENGINE_OFFSET + common::ENGINE_NUM_USED_OFFSET;
    let used = u16::from_le_bytes(slab_data[used_off..used_off + 2].try_into().unwrap());

    assert_ne!(magic, 0, "InitMarket must write a non-zero slab magic");
    assert_eq!(
        &slab_data[FEED_ID_OFF..FEED_ID_OFF + 32],
        &[0u8; 32],
        "Hyperp market must store zeroed feed id"
    );
    assert_eq!(
        slab_data[INVERT_OFF], 0,
        "invert flag should be 0 for this test"
    );
    assert_eq!(
        mark, initial_price_e6,
        "Hyperp mark must equal initial_mark_price_e6 at init"
    );
    assert_eq!(
        index, initial_price_e6,
        "Hyperp index must equal initial_mark_price_e6 at init"
    );
    assert_eq!(
        cap,
        common::TEST_MAX_PRICE_MOVE_BPS_PER_SLOT,
        "Cap should match engine's max_price_move_bps_per_slot"
    );
    assert_eq!(
        used, 0,
        "No user/LP accounts should exist immediately after market init"
    );

    println!("HYPERP INIT VERIFIED: Market initialized with $100 initial mark/index price");
}

/// Test: Hyperp mode with inverted market (e.g., SOL/USD perp)
///
/// For inverted markets, the raw oracle price is inverted: inverted = 1e12 / raw
/// Example: SOL/USD oracle returns ~$138 (138_000_000 in e6)
///          Inverted = 1e12 / 138_000_000 = ~7246 (price in SOL per USD)
///
/// In Hyperp mode with invert=1:
/// - initial_mark_price_e6 provided as raw price (e.g., 138_000_000)
/// - InitMarket applies inversion internally
/// - Stored mark/index are in inverted form (~7246)
#[test]
fn test_hyperp_init_market_with_inverted_price() {
    let path = program_path();

    let mut svm = LiteSVM::new();
    let program_id = Pubkey::new_unique();
    let program_bytes = std::fs::read(&path).expect("Failed to read program");
    svm.add_program(program_id, &program_bytes);

    let payer = Keypair::new();
    let slab = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let (vault_pda, _) = Pubkey::find_program_address(&[b"vault", slab.as_ref()], &program_id);
    let vault = Pubkey::new_unique();

    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();

    svm.set_account(
        slab,
        Account {
            lamports: 1_000_000_000,
            data: vec![0u8; SLAB_LEN],
            owner: program_id,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    svm.set_account(
        mint,
        Account {
            lamports: 1_000_000,
            data: make_mint_data(),
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    svm.set_account(
        vault,
        Account {
            lamports: 1_000_000,
            data: make_token_account_data(&mint, &vault_pda, 0),
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
            data: vec![0u8; TokenAccount::LEN],
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    svm.set_sysvar(&Clock {
        slot: 100,
        unix_timestamp: 100,
        ..Clock::default()
    });

    // Hyperp mode with inverted market
    // Raw price: $138 (SOL/USD) = 138_000_000 in e6
    // After inversion: 1e12 / 138_000_000 = ~7246 (USD/SOL)
    let raw_price_e6 = 138_000_000u64; // $138 in e6 format
    let expected_inverted = 1_000_000_000_000u64 / raw_price_e6; // ~7246

    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(slab, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
        ],
        data: encode_init_market_full_v2(
            &payer.pubkey(),
            &mint,
            &[0u8; 32],   // Hyperp mode: feed_id = 0
            1,            // invert = 1 (inverted market)
            raw_price_e6, // Raw price, will be inverted internally
            0,            // warmup
        ),
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&payer.pubkey()),
        &[&payer],
        svm.latest_blockhash(),
    );

    let result = svm.send_transaction(tx);

    assert!(
        result.is_ok(),
        "Hyperp InitMarket with inverted price should succeed. Got: {:?}",
        result
    );

    // Verify inverted Hyperp initialization state.
    let slab_data = svm.get_account(&slab).unwrap().data;
    let magic = u64::from_le_bytes(slab_data[0..8].try_into().unwrap());
    let config = percolator_prog::state::read_config(&slab_data);
    let mark = config.hyperp_mark_e6;
    let index = config.last_effective_price_e6;
    let cap_off = common::ENGINE_OFFSET + 32 + 160;
    let cap = u64::from_le_bytes(slab_data[cap_off..cap_off + 8].try_into().unwrap());
    const FEED_ID_OFF: usize = 136 + 64;
    const INVERT_OFF: usize = 136 + 107;
    let used_off = common::ENGINE_OFFSET + common::ENGINE_NUM_USED_OFFSET;
    let used = u16::from_le_bytes(slab_data[used_off..used_off + 2].try_into().unwrap());

    assert_ne!(magic, 0, "InitMarket must write a non-zero slab magic");
    assert_eq!(
        &slab_data[FEED_ID_OFF..FEED_ID_OFF + 32],
        &[0u8; 32],
        "Hyperp market must store zeroed feed id"
    );
    assert_eq!(
        slab_data[INVERT_OFF], 1,
        "invert flag should be 1 for inverted Hyperp init"
    );
    assert_eq!(
        mark, expected_inverted,
        "Hyperp mark must be stored as inverted initial price"
    );
    assert_eq!(
        index, expected_inverted,
        "Hyperp index must be stored as inverted initial price"
    );
    assert_eq!(
        cap,
        common::TEST_MAX_PRICE_MOVE_BPS_PER_SLOT,
        "Cap should match engine's max_price_move_bps_per_slot"
    );
    assert_eq!(
        used, 0,
        "No user/LP accounts should exist immediately after market init"
    );

    println!("HYPERP INVERTED MARKET VERIFIED:");
    println!(
        "  Raw price: {} (${:.2})",
        raw_price_e6,
        raw_price_e6 as f64 / 1_000_000.0
    );
    println!(
        "  Expected inverted: {} (~{:.4} SOL/USD)",
        expected_inverted,
        expected_inverted as f64 / 1_000_000.0
    );
    println!("  Mark/Index stored in inverted form for SOL-denominated perp");
}

/// Test 7: Oracle price impact - crank succeeds at different prices
#[test]
fn test_comprehensive_oracle_price_impact_on_pnl() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Top up insurance to prevent force-realize and dust-close (must exceed threshold after EWMA update)
    let ins_payer = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&ins_payer, 1_000_000_000);
    let vault_initial = env.vault_balance();

    // Open long at $138
    let size: i128 = 10_000_000;
    env.trade(&user, &lp, lp_idx, user_idx, size);

    // Price goes to $150 - crank. User is long, so mark-to-market PnL should be positive.
    env.set_slot_and_price(200, 150_000_000);
    env.crank();
    assert_eq!(
        env.vault_balance(),
        vault_initial,
        "Vault conserved at $150"
    );
    // For an open position, PnL is tracked in the pnl field (mark-to-market).
    // Capital settles during touch_account (lazy), not during crank.
    // Check that PnL is positive (long position profits at higher price).
    let pnl_at_150 = env.read_account_pnl(user_idx);
    let cap_at_150 = env.read_account_capital(user_idx);
    // Either PnL is positive OR capital increased (if warmup already converted it)
    assert!(
        pnl_at_150 > 0 || cap_at_150 > 10_000_000_000,
        "Long position should have gained value at $150 (up from $138): pnl={} cap={}",
        pnl_at_150,
        cap_at_150
    );

    // Price drops to $120 - crank. User is long, PnL should be negative.
    env.set_slot_and_price(300, 120_000_000);
    env.crank();
    assert_eq!(
        env.vault_balance(),
        vault_initial,
        "Vault conserved at $120"
    );
    let pnl_at_120 = env.read_account_pnl(user_idx);
    let cap_at_120 = env.read_account_capital(user_idx);
    // At $120 (below entry $138), long position should have negative or reduced PnL
    assert!(
        pnl_at_120 < pnl_at_150,
        "Long position should lose value at $120 (below $150): pnl={} was {}, cap={} was {}",
        pnl_at_120,
        pnl_at_150,
        cap_at_120,
        cap_at_150
    );

    // Price recovers to $140 - crank. PnL should improve from $120 level.
    env.set_slot_and_price(400, 140_000_000);
    env.crank();
    assert_eq!(
        env.vault_balance(),
        vault_initial,
        "Vault conserved at $140"
    );
    let pnl_at_140 = env.read_account_pnl(user_idx);
    assert!(
        pnl_at_140 > pnl_at_120,
        "Long position should gain value at $140 (up from $120): pnl={} was {}",
        pnl_at_140,
        pnl_at_120
    );

    // Position must still be open
    assert_ne!(
        env.read_account_position(user_idx),
        0,
        "Position must persist through price changes"
    );
}

/// CRITICAL: SetOraclePriceCap admin-only
// test_critical_set_oracle_price_cap_authorization deleted:
// SetOraclePriceCap (tag 18) was removed in v12.19. The cap is now the
// immutable init-time `max_price_move_bps_per_slot` RiskParam, so there
// is no runtime admin authorization surface left to test.

/// Test: Hyperp mode index smoothing bypass via multiple cranks in same slot
///
/// SECURITY RESEARCH: In Hyperp mode, the index should smoothly move toward the mark
/// price, rate-limited by oracle_price_cap (default 1% per slot).
///
/// Potential issue: If crank is called twice in the same slot:
/// 1. First crank: dt > 0, index rate-limited toward mark
/// 2. Trade: mark moves (clamped against index)
/// 3. Second crank: dt = 0, clamp_toward_with_dt returns index (no movement)
///
/// Bug #9 fix: When dt=0, index stays unchanged instead of jumping to mark.
#[test]
fn test_hyperp_index_smoothing_multiple_cranks_same_slot() {
    let path = program_path();

    let mut svm = LiteSVM::new();
    let program_id = Pubkey::new_unique();
    let program_bytes = std::fs::read(&path).expect("Failed to read program");
    svm.add_program(program_id, &program_bytes);

    let payer = Keypair::new();
    let slab = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let (vault_pda, _) = Pubkey::find_program_address(&[b"vault", slab.as_ref()], &program_id);
    let vault = Pubkey::new_unique();
    let dummy_oracle = Pubkey::new_unique();

    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();

    svm.set_account(
        slab,
        Account {
            lamports: 1_000_000_000,
            data: vec![0u8; SLAB_LEN],
            owner: program_id,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    svm.set_account(
        mint,
        Account {
            lamports: 1_000_000,
            data: make_mint_data(),
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    svm.set_account(
        vault,
        Account {
            lamports: 1_000_000,
            data: make_token_account_data(&mint, &vault_pda, 0),
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    // Dummy oracle (not used in Hyperp mode, but account must exist)
    svm.set_account(
        dummy_oracle,
        Account {
            lamports: 1_000_000,
            data: vec![0u8; 100],
            owner: Pubkey::new_unique(),
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
            data: vec![0u8; TokenAccount::LEN],
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    // Start at slot 100
    svm.set_sysvar(&Clock {
        slot: 100,
        unix_timestamp: 100,
        ..Clock::default()
    });

    // Init market with Hyperp mode (feed_id = 0)
    let initial_price_e6 = 100_000_000u64; // $100

    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(slab, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
        ],
        data: encode_init_market_hyperp(&payer.pubkey(), &mint, initial_price_e6),
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&payer.pubkey()),
        &[&payer],
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx).expect("InitMarket failed");
    println!("Hyperp market initialized with mark=index=$100");

    // Advance to slot 200 and crank
    svm.set_sysvar(&Clock {
        slot: 200,
        unix_timestamp: 200,
        ..Clock::default()
    });

    let crank_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new_readonly(payer.pubkey(), true),
            AccountMeta::new(slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(dummy_oracle, false),
        ],
        data: encode_crank_permissionless(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), crank_ix.clone()],
        Some(&payer.pubkey()),
        &[&payer],
        svm.latest_blockhash(),
    );
    let result1 = svm.send_transaction(tx);
    println!("First crank in slot 200: {:?}", result1.is_ok());
    assert!(result1.is_ok(), "First crank should succeed: {:?}", result1);

    // Call crank again in the SAME slot (slot 200)
    // Expire old blockhash and get new one to make transaction distinct
    svm.expire_blockhash();
    let new_blockhash = svm.latest_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), crank_ix.clone()],
        Some(&payer.pubkey()),
        &[&payer],
        new_blockhash,
    );
    let result2 = svm.send_transaction(tx);
    println!("Second crank in slot 200: {:?}", result2);
    if let Err(ref e) = result2 {
        println!("Second crank error: {:?}", e);
    }

    // SECURITY VERIFICATION: Multiple cranks in the same slot are ALLOWED.
    // With zero OI, the wrapper may adopt the target directly because no
    // live position can lose equity.

    assert!(
        result2.is_ok(),
        "Second crank should succeed in same slot: {:?}",
        result2
    );

    // Read last_effective_price_e6 (index) from slab. In a flat market,
    // same-slot target adoption is permitted and should move toward the mark.
    let slab_data = svm.get_account(&slab).unwrap().data;
    const INDEX_OFF: usize = 136 + 192; // HEADER_LEN + offset_of!(MarketConfig, last_effective_price_e6) (v12.19)
    let index_after = u64::from_le_bytes(slab_data[INDEX_OFF..INDEX_OFF + 8].try_into().unwrap());
    assert!(
        index_after >= initial_price_e6,
        "flat same-slot crank should not move away from the target: initial={} after={}",
        initial_price_e6,
        index_after
    );
}

/// Audit gap 1: Hyperp index smoothing is rate-limited by cap * dt.
///
/// Spec behavior: In Hyperp mode, the index (last_effective_price_e6) moves
/// toward the mark price by at most `index * cap * dt / 1_000_000` per
/// crank.  A second crank in the same slot may adopt the target directly when
/// the market has zero OI.
///
/// This test:
/// 1. Inits a Hyperp market at $100.
/// 2. Pushes mark to ~$101 (clamped by circuit breaker from $200 push).
/// 3. Cranks with dt=10 slots, verifies index movement <= cap * dt bound.
/// 4. Cranks again in the same slot, verifies the flat market does not move
///    away from the target.
#[test]
fn test_hyperp_index_smoothing_rate_limited() {
    program_path();

    let mut env = TestEnv::new();
    let initial_price: u64 = 100_000_000; // $100 in e6

    // Init Hyperp market (feed_id = [0;32], no external oracle)
    env.init_market_hyperp(initial_price);

    // Set oracle authority so we can push prices
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("set oracle authority");

    // Read the engine-level `max_price_move_bps_per_slot` — v12.19
    // replacement for `oracle_price_cap`.
    let slab_data = env.svm.get_account(&env.slab).unwrap().data;
    const CAP_OFF: usize = common::ENGINE_OFFSET + 32 + 160;
    let cap = u64::from_le_bytes(slab_data[CAP_OFF..CAP_OFF + 8].try_into().unwrap());
    assert_eq!(
        cap,
        common::TEST_MAX_PRICE_MOVE_BPS_PER_SLOT,
        "default cap must match test fixture",
    );

    // Push $200 after enough time for EWMA to blend significantly.
    // With default halflife=100 and ~100 slots dt, alpha ≈ 50%.
    // Clamped input: min($200, $100 + 1%) = $101.
    // EWMA: $100 * 0.5 + $101 * 0.5 = $100.5M
    // The mark moves enough that the index will follow.
    env.set_slot(1);
    env.try_push_oracle_price(&admin, 200_000_000, 200)
        .expect("push");

    let slab_data = env.svm.get_account(&env.slab).unwrap().data;
    const INDEX_OFF: usize = 136 + 192; // HEADER_LEN + offset_of!(MarketConfig, last_effective_price_e6) (v12.19)

    // Advance 10 slots and crank. Index should move toward mark.
    let dt: u64 = 10;
    env.set_slot(dt); // set_slot adds 100 internally for monotonicity
    env.crank();

    let slab_data = env.svm.get_account(&env.slab).unwrap().data;
    let index_after_crank =
        u64::from_le_bytes(slab_data[INDEX_OFF..INDEX_OFF + 8].try_into().unwrap());

    // Max allowed movement: index * cap * dt / 1_000_000
    let max_delta = initial_price as u128 * cap as u128 * dt as u128 / 1_000_000;
    let actual_delta = if index_after_crank > initial_price {
        (index_after_crank - initial_price) as u128
    } else {
        (initial_price - index_after_crank) as u128
    };

    assert!(
        actual_delta <= max_delta,
        "index movement {} exceeds rate limit {} (cap={}, dt={})",
        actual_delta,
        max_delta,
        cap,
        dt
    );
    // Index should have moved toward mark (if mark > initial and dt > 0)
    // With EWMA, the mark may be only slightly above initial after pushes.
    // The index moves toward mark, so it should be >= initial.
    assert!(
        index_after_crank >= initial_price,
        "index should not decrease: index={} initial={}",
        index_after_crank,
        initial_price
    );

    // Second crank in the same slot (dt=0): zero-OI markets may adopt target.
    let index_before_same_slot = index_after_crank;
    env.svm.expire_blockhash();
    env.crank();

    let slab_data = env.svm.get_account(&env.slab).unwrap().data;
    let index_after_same_slot =
        u64::from_le_bytes(slab_data[INDEX_OFF..INDEX_OFF + 8].try_into().unwrap());

    assert!(
        index_after_same_slot >= index_before_same_slot,
        "flat same-slot crank should not move away from the target: before={} after={}",
        index_before_same_slot,
        index_after_same_slot
    );
}

/// InitMarket rejects disabling or widening the Pyth confidence filter.
#[test]
fn test_conf_filter_bps_init_range_enforced() {
    let bad_values = [
        0,
        percolator_prog::constants::MIN_CONF_FILTER_BPS - 1,
        percolator_prog::constants::MAX_CONF_FILTER_BPS + 1,
    ];
    for conf_bps in bad_values {
        let mut env = TestEnv::new();
        let data = encode_init_market_with_conf_bps(
            &env.payer.pubkey(),
            &env.mint,
            &TEST_FEED_ID,
            0,
            0,
            0,
            conf_bps,
        );
        assert!(
            env.try_init_market_raw(data).is_err(),
            "InitMarket must reject conf_filter_bps={}",
            conf_bps
        );
    }

    for conf_bps in [
        percolator_prog::constants::MIN_CONF_FILTER_BPS,
        percolator_prog::constants::MAX_CONF_FILTER_BPS,
    ] {
        let mut env = TestEnv::new();
        let data = encode_init_market_with_conf_bps(
            &env.payer.pubkey(),
            &env.mint,
            &TEST_FEED_ID,
            0,
            0,
            0,
            conf_bps,
        );
        env.try_init_market_raw(data)
            .unwrap_or_else(|e| panic!("InitMarket must accept conf_filter_bps={conf_bps}: {e}"));
    }
}

/// InitMarket rejects long oracle staleness windows.
#[test]
fn test_max_staleness_secs_init_cap_enforced() {
    const MAX_STALENESS_OFFSET: usize = 1 + 32 + 32 + 32;

    let mut env = TestEnv::new();
    let mut data = encode_init_market_with_conf_bps(
        &env.payer.pubkey(),
        &env.mint,
        &TEST_FEED_ID,
        0,
        0,
        0,
        percolator_prog::constants::MIN_CONF_FILTER_BPS,
    );
    data[MAX_STALENESS_OFFSET..MAX_STALENESS_OFFSET + 8]
        .copy_from_slice(&percolator_prog::constants::MAX_ORACLE_STALENESS_SECS.to_le_bytes());
    env.try_init_market_raw(data)
        .expect("InitMarket must accept max staleness at the cap");

    let mut env = TestEnv::new();
    let mut data = encode_init_market_with_conf_bps(
        &env.payer.pubkey(),
        &env.mint,
        &TEST_FEED_ID,
        0,
        0,
        0,
        percolator_prog::constants::MIN_CONF_FILTER_BPS,
    );
    data[MAX_STALENESS_OFFSET..MAX_STALENESS_OFFSET + 8].copy_from_slice(
        &(percolator_prog::constants::MAX_ORACLE_STALENESS_SECS + 1).to_le_bytes(),
    );
    assert!(
        env.try_init_market_raw(data).is_err(),
        "InitMarket must reject staleness above the cap"
    );
}

/// Bug #2: expo.abs() overflows for i32::MIN, bypassing the exponent bound.
///
/// In optimized Rust builds, i32::MIN.abs() wraps to i32::MIN (negative),
/// which can bypass the `> MAX_EXPO_ABS` check and produce nonsense scaling.
/// The exponent bound must use a safe range check instead.
///
/// We verify this by setting up a Pyth oracle with expo = i32::MIN and
/// confirming the oracle read is cleanly rejected (not a panic or bypass).
#[test]
fn test_pyth_expo_i32_min_rejected_safely() {
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Set oracle with expo = i32::MIN (should be rejected, not panic)
    let pyth_data = make_pyth_data(&TEST_FEED_ID, 138_000_000, i32::MIN, 1, 100);
    env.svm
        .set_account(
            env.pyth_index,
            Account {
                lamports: 1_000_000,
                data: pyth_data.clone(),
                owner: PYTH_RECEIVER_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm
        .set_account(
            env.pyth_col,
            Account {
                lamports: 1_000_000,
                data: pyth_data,
                owner: PYTH_RECEIVER_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    // Crank reads the oracle — must fail cleanly (not panic or bypass)
    let result = env.try_crank();
    assert!(
        result.is_err(),
        "Pyth oracle with expo=i32::MIN must be rejected, not accepted"
    );
}

// ============================================================================
// Funding Anti-Retroactivity: UpdateConfig
// ============================================================================

/// Anti-retroactivity: UpdateConfig must settle the idle interval at the
/// OLD stored funding rate before applying new params.
///
/// In Hyperp mode, UpdateConfig calls accrue_market_to (which uses the stored
/// rate) BEFORE updating config params and recomputing the rate. This ensures
/// funding accrued during the idle interval reflects the old k, not the new.
///
/// Steps:
///   1. Init Hyperp market, create positions via TradeCpi
///   2. Push mark above index to create premium, crank to store a rate
///   3. Advance slots without cranking (idle interval)
///   4. Call UpdateConfig to double funding_k_bps (100 -> 200)
///   5. Verify: K coefficients changed (accrual happened during UpdateConfig)
///      and stored rate is now the new one (future uses new config)
#[test]
fn test_funding_boundary_anti_retroactivity_update_config() {
    program_path();
    println!("=== FUNDING ANTI-RETROACTIVITY: UpdateConfig ===");

    let mut env = TradeCpiTestEnv::new();
    env.init_market_hyperp(1_000_000); // $1 initial mark/index

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;

    // Oracle authority (v12.19: price-move cap is immutable init-time)
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();

    // Helper: send UpdateConfig with a specific funding_k_bps.
    // Uses short horizon (100 slots) so the per-slot rate is non-zero
    // even with moderate premiums. k multiplier adjusts sensitivity.
    let admin_try_config = |env: &mut TradeCpiTestEnv, k_bps: u64| -> Result<(), String> {
        let kp = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
        let ix = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(kp.pubkey(), true),
                AccountMeta::new(env.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                // Non-Hyperp UpdateConfig requires the oracle account.
                AccountMeta::new_readonly(env.pyth_index, false),
            ],
            data: encode_update_config(
                100, k_bps, 10_000i64,
                10i64, // funding_max_e9_per_slot=10 fits engine's e9=1e6 cap
            ),
        };
        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&kp.pubkey()),
            &[&kp],
            env.svm.latest_blockhash(),
        );
        env.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    };
    let admin_send_config = |env: &mut TradeCpiTestEnv, k_bps: u64| {
        admin_try_config(env, k_bps).expect("UpdateConfig failed");
    };

    // Initial funding config: k=1000 (10x multiplier), horizon=100 slots
    // premium_bps ~1000 * k/100 / horizon = 1000 * 10 / 100 = 100 bps/slot
    admin_send_config(&mut env, 1000);
    println!("1. Funding config: k=1000, horizon=100");

    // Push price and crank to initialize
    env.try_push_oracle_price(&admin, 1_000_000, 100).unwrap();
    env.set_slot(5);
    env.crank();

    // Create LP + user, trade to create positions
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.set_slot(10);
    env.crank();

    // Trade creates positions. In Hyperp mode, TradeCpi also updates the mark
    // to exec_price (clamped against index) and recomputes the funding rate.
    // The matcher's exec_price will differ from the index, creating a premium
    // and a non-zero stored funding rate.
    env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000_000,
        &matcher_prog,
        &matcher_ctx,
    )
    .expect("Trade should succeed");
    println!("2. Positions created via TradeCpi");

    // The trade updated mark to exec_price (which may differ from index).
    // Push mark above current index to widen the premium, then let it persist.
    // Keep this in the same market slot: PushHyperpMark is not the exposed
    // market catchup path, so it must not accrue a live OI interval.
    env.try_push_oracle_price(&admin, 2_000_000, 111).unwrap();
    // Do NOT crank -- we want mark != index so the stored rate is non-zero.
    // The push already recomputes and stores the funding rate.

    // Read mark and index via bytemuck config reader (layout-independent)
    let config_before = {
        let d = env.svm.get_account(&env.slab).unwrap().data;
        percolator_prog::state::read_config(&d)
    };
    println!(
        "3. After push: mark={} index={} gap={}",
        config_before.mark_ewma_e6,
        config_before.last_effective_price_e6,
        if config_before.mark_ewma_e6 > config_before.last_effective_price_e6 {
            config_before.mark_ewma_e6 - config_before.last_effective_price_e6
        } else {
            config_before.last_effective_price_e6 - config_before.mark_ewma_e6
        }
    );
    let has_premium = config_before.mark_ewma_e6 != config_before.last_effective_price_e6
        && config_before.mark_ewma_e6 > 0;
    assert!(
        has_premium,
        "Precondition: mark must differ from index for funding anti-retroactivity test"
    );
    println!(
        "4. Premium exists: mark={} != index={}",
        config_before.mark_ewma_e6, config_before.last_effective_price_e6
    );

    // ---- Idle interval: advance 50 slots without cranking ----
    let idle_dt: u64 = 50;
    env.set_slot(11 + idle_dt);
    println!("5. Advanced {} slots without cranking", idle_dt);

    // UpdateConfig is not the keeper progress path. With live OI, a pending
    // funding interval must be realized by KeeperCrank first, so the attempted
    // config change rolls back instead of retroactively changing the old-rate
    // interval.
    let stale_update = admin_try_config(&mut env, 1999)
        .expect_err("UpdateConfig must not be the exposed funding catchup path");
    assert!(
        stale_update.contains("0x1d"),
        "UpdateConfig should surface CatchupRequired before keeper progress, got: {stale_update}",
    );

    // KeeperCrank realizes the idle interval under the old k=1000 config.
    env.crank();

    // UpdateConfig can now change k from 1000 to 2000 without doing additional
    // exposed market progress.
    admin_send_config(&mut env, 2000);
    println!("6. KeeperCrank realized old-rate interval; UpdateConfig changed k 1000 -> 2000");

    // Verify new config took effect
    let config_after = {
        let d = env.svm.get_account(&env.slab).unwrap().data;
        percolator_prog::state::read_config(&d)
    };
    assert_eq!(
        config_after.funding_k_bps, 2000,
        "UpdateConfig must write new k_bps"
    );
    println!(
        "7. Config updated: k_bps={} (was 1000)",
        config_after.funding_k_bps
    );

    // The rejected pre-crank update plus successful post-crank update proves
    // config mutation cannot be used to price an exposed historical interval
    // under new funding parameters.

    // Post-UpdateConfig crank should use new rate (k=2000)
    env.set_slot(11 + idle_dt + 10);
    env.try_push_oracle_price(&admin, 2_000_000, 190).unwrap();
    env.crank();
    let config_post = {
        let d = env.svm.get_account(&env.slab).unwrap().data;
        percolator_prog::state::read_config(&d)
    };
    println!(
        "8. Post-crank k_bps={} (should still be 2000)",
        config_post.funding_k_bps
    );
    assert_eq!(
        config_post.funding_k_bps, 2000,
        "k_bps must persist after crank"
    );

    println!();
    println!("FUNDING ANTI-RETROACTIVITY UpdateConfig: PASSED");
}

/// Oracle observation monotonicity (graceful policy): a Pyth update
/// with a `publish_time` older than the last accepted observation
/// must NOT advance the stored baseline or timestamp, but it also
/// must not fail the caller's tx. The wrapper substitutes the stored
/// `last_effective_price_e6` so callers who signed before a newer
/// update landed (offline signers, hardware wallets, multi-sigs) can
/// still execute against the freshest known price. Baseline-rewind
/// is impossible regardless of what older observation is submitted.
#[test]
fn test_oracle_older_observation_uses_stored_price_and_does_not_rewind() {
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // First crank: fresh Pyth advances baseline.
    env.set_slot_and_price(100, 138_000_000);
    env.crank();

    const LAST_ORACLE_PUB_TS_OFF: usize = 320; // HEADER_LEN(136) + last_oracle_publish_time(184)
    const LAST_EFFECTIVE_PRICE_OFF: usize = 328; // HEADER_LEN(136) + last_effective_price_e6(192)
    let read_pub_ts = |env: &TestEnv| -> i64 {
        let d = env.svm.get_account(&env.slab).unwrap().data;
        i64::from_le_bytes(
            d[LAST_ORACLE_PUB_TS_OFF..LAST_ORACLE_PUB_TS_OFF + 8]
                .try_into()
                .unwrap(),
        )
    };
    let read_baseline = |env: &TestEnv| -> u64 {
        let d = env.svm.get_account(&env.slab).unwrap().data;
        u64::from_le_bytes(
            d[LAST_EFFECTIVE_PRICE_OFF..LAST_EFFECTIVE_PRICE_OFF + 8]
                .try_into()
                .unwrap(),
        )
    };
    let pub_ts_before = read_pub_ts(&env);
    let baseline_before = read_baseline(&env);
    assert!(
        pub_ts_before > 0,
        "first crank must advance last_oracle_publish_time, got {}",
        pub_ts_before,
    );

    // Replace the on-chain Pyth account with an OLDER (still within
    // max_staleness_secs) update at a wildly different price. The
    // older `publish_time` must trigger the graceful fallback — the
    // submitted price must NOT be processed against the baseline.
    let older_publish_time = pub_ts_before - 30;
    let older_pyth = make_pyth_data(&TEST_FEED_ID, 999_999_999, -6, 1, older_publish_time);
    env.svm
        .set_account(
            env.pyth_index,
            Account {
                lamports: 1_000_000,
                data: older_pyth,
                owner: PYTH_RECEIVER_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    // Crank must SUCCEED (graceful) — the wrapper substitutes the stored baseline.
    env.crank();

    // Neither the timestamp nor the baseline moved.
    assert_eq!(
        read_pub_ts(&env),
        pub_ts_before,
        "older observation must not advance last_oracle_publish_time",
    );
    assert_eq!(
        read_baseline(&env),
        baseline_before,
        "older observation must not pull last_effective_price_e6 backward \
         even when the submitted price is wildly different",
    );
}

/// Equal `publish_time` must succeed (caller's tx doesn't fail) but
/// must NOT re-clamp the baseline. Replaying the same Pyth observation
/// N times must be a no-op — otherwise an attacker can walk
/// `last_effective_price_e6` toward the raw oracle price by one
/// cap-step per replay.
#[test]
fn test_oracle_equal_publish_time_replay_does_not_walk_baseline() {
    let mut env = TestEnv::new();
    // Set a tight 1% cap so each cap-step would be visible.
    env.init_market_with_cap(0, 80);

    const LAST_ORACLE_PUB_TS_OFF: usize = 320;
    const LAST_EFFECTIVE_PRICE_OFF: usize = 328; // HEADER_LEN(136) + last_effective_price_e6(192) (v12.19)
    let read_pub_ts = |env: &TestEnv| -> i64 {
        let d = env.svm.get_account(&env.slab).unwrap().data;
        i64::from_le_bytes(
            d[LAST_ORACLE_PUB_TS_OFF..LAST_ORACLE_PUB_TS_OFF + 8]
                .try_into()
                .unwrap(),
        )
    };
    let read_baseline = |env: &TestEnv| -> u64 {
        let d = env.svm.get_account(&env.slab).unwrap().data;
        u64::from_le_bytes(
            d[LAST_EFFECTIVE_PRICE_OFF..LAST_EFFECTIVE_PRICE_OFF + 8]
                .try_into()
                .unwrap(),
        )
    };

    // Genesis Pyth at price 100, accepted at init.
    env.set_slot_and_price(100, 100_000_000);
    env.crank();
    let baseline_before = read_baseline(&env);
    let pub_ts_before = read_pub_ts(&env);

    // Replace on-chain Pyth with the SAME publish_time but a wildly
    // different raw price (would clamp to baseline + 1% per re-read
    // if the equal-timestamp branch fell through to the clamp path).
    let attack_pyth = make_pyth_data(&TEST_FEED_ID, 999_999_999, -6, 1, pub_ts_before);
    env.svm
        .set_account(
            env.pyth_index,
            Account {
                lamports: 1_000_000,
                data: attack_pyth,
                owner: PYTH_RECEIVER_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    // Replay the same publish_time N times. Each crank must succeed
    // but baseline + timestamp must not advance.
    for _ in 0..10 {
        env.crank();
    }

    assert_eq!(
        read_baseline(&env),
        baseline_before,
        "10× replay of the same publish_time must not walk baseline",
    );
    assert_eq!(
        read_pub_ts(&env),
        pub_ts_before,
        "replay must not advance last_oracle_publish_time",
    );
}

/// Liveness cursor: stale-or-duplicate observations must NOT advance
/// `last_good_oracle_slot`. Otherwise an attacker can keep the market
/// alive past `permissionless_resolve_stale_slots` by replaying an old
/// (still-within-max_staleness_secs) Pyth account every slot.
#[test]
fn test_oracle_replay_does_not_advance_liveness_cursor() {
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 80);

    const LAST_GOOD_SLOT_OFF: usize = 136 + 312; // v12.19: HEADER_LEN + last_good_oracle_slot(312)
    let read_last_good = |env: &TestEnv| -> u64 {
        let d = env.svm.get_account(&env.slab).unwrap().data;
        u64::from_le_bytes(
            d[LAST_GOOD_SLOT_OFF..LAST_GOOD_SLOT_OFF + 8]
                .try_into()
                .unwrap(),
        )
    };

    // Fresh Pyth at slot 100 stamps last_good_oracle_slot.
    env.set_slot_and_price(100, 138_000_000);
    env.crank();
    let last_good_after_first = read_last_good(&env);
    assert!(
        last_good_after_first >= 100,
        "first crank must stamp last_good_oracle_slot, got {}",
        last_good_after_first,
    );

    // Advance the wall clock without touching Pyth. Same publish_time
    // gets re-read; liveness must NOT advance.
    let mut clk = env.svm.get_sysvar::<solana_sdk::clock::Clock>();
    clk.slot += 50;
    env.svm.set_sysvar(&clk);
    env.crank();

    assert_eq!(
        read_last_good(&env),
        last_good_after_first,
        "duplicate-publish_time read must not advance last_good_oracle_slot",
    );
}
