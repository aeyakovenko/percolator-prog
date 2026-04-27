#![cfg(feature = "legacy-tests")]
//! Public-path stale-market gates + init-time prevalidation
//! (security.md seventh / eighth passes).
//!
//! `TopUpInsurance` and `InitUser` are permissionless public paths that
//! mutate slab state. Under v12.19 they must reject before any token
//! movement when the market has matured into the permissionless-resolve
//! window (`permissionless_stale_matured`). Otherwise an attacker (or
//! confused honest user) could move tokens into a market that has
//! already exited the live regime, leaving funds stranded behind the
//! resolution flow.
//!
//! These tests fire the matured-gate path. The §9.2 `CatchupRequired`
//! backstop is structurally only reachable after a partial CatchupAccrue
//! advances `last_good_oracle_slot` ahead of `engine.last_market_slot`
//! (covered indirectly by the F1 partial-catchup regression).

mod common;
#[allow(unused_imports)]
use common::*;

use solana_sdk::signature::{Keypair, Signer};

/// Custom error code for `PercolatorError::OracleStale`.
const ORACLE_STALE: u32 = 6;

#[test]
fn test_attack_top_up_insurance_rejected_after_stale_matured() {
    let mut env = TestEnv::new();
    // perm_resolve = 80 (test default, < MAX_ACCRUAL_DT_SLOTS = 100).
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 1_000_000_000);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    env.crank();

    let insurance_before = env.read_insurance_balance();
    let last_slot = env.svm.get_sysvar::<solana_sdk::clock::Clock>().slot;

    // Jump well past perm_resolve (80) without cranking. Oracle account is
    // also re-stamped at the new slot, but `last_good_oracle_slot` is the
    // stored cursor; without a wrapper read it stays at the pre-jump value.
    env.set_slot_and_price_raw_no_walk(last_slot + 200, 138_000_000);

    let payer = Keypair::new();
    env.svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    let result = env.try_top_up_insurance(&payer, 1_000_000);
    assert!(
        result.is_err(),
        "TopUpInsurance must reject on a stale-matured market: result={:?}",
        result
    );
    let err_str = result.unwrap_err();
    assert!(
        err_str.contains(&format!("Custom({})", ORACLE_STALE)),
        "expected OracleStale (Custom({})), got: {}",
        ORACLE_STALE,
        err_str
    );

    // Atomicity: rejected TopUp must not move tokens or insurance.
    assert_eq!(
        env.read_insurance_balance(),
        insurance_before,
        "rejected TopUp must not move insurance balance"
    );
}

#[test]
fn test_attack_init_user_rejected_after_stale_matured() {
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 1_000_000_000);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    env.crank();

    let last_slot = env.svm.get_sysvar::<solana_sdk::clock::Clock>().slot;
    env.set_slot_and_price_raw_no_walk(last_slot + 200, 138_000_000);

    let new_user = Keypair::new();
    let result = env.try_init_user(&new_user);
    assert!(
        result.is_err(),
        "InitUser must reject on a stale-matured market: result={:?}",
        result
    );
    let err_str = result.unwrap_err();
    assert!(
        err_str.contains(&format!("Custom({})", ORACLE_STALE)),
        "expected OracleStale (Custom({})), got: {}",
        ORACLE_STALE,
        err_str
    );
}

// ---------------------------------------------------------------------------
// Eighth pass — init-time prevalidation gates
// ---------------------------------------------------------------------------

/// Custom error code for `PercolatorError::InvalidConfigParam`.
const INVALID_CONFIG_PARAM: u32 = 26;

/// Wire-format byte offset of `RiskParams.max_price_move_bps_per_slot`
/// in the InitMarket payload (after tag, admin, mint, feed_id, all the
/// pre-`max_price_move_bps_per_slot` config fields, and three u128 floor
/// fields). Encoders write a u64 here; we patch to 0 to exercise the
/// wrapper's prevalidation gate (commit 83078bb).
const MAX_PRICE_MOVE_OFFSET: usize = 288;

fn send_init_market_raw(env: &mut TestEnv, data: Vec<u8>) -> Result<(), String> {
    let admin = solana_sdk::signature::Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // init_market_with_cap pre-creates a dummy ATA — mirror that here.
    let dummy_ata = solana_sdk::pubkey::Pubkey::new_unique();
    env.svm
        .set_account(
            dummy_ata,
            solana_sdk::account::Account {
                lamports: 1_000_000,
                data: vec![0u8; spl_token::state::Account::LEN],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let ix = solana_sdk::instruction::Instruction {
        program_id: env.program_id,
        accounts: vec![
            solana_sdk::instruction::AccountMeta::new(admin.pubkey(), true),
            solana_sdk::instruction::AccountMeta::new(env.slab, false),
            solana_sdk::instruction::AccountMeta::new_readonly(env.mint, false),
            solana_sdk::instruction::AccountMeta::new(env.vault, false),
            solana_sdk::instruction::AccountMeta::new_readonly(
                solana_sdk::sysvar::clock::ID,
                false,
            ),
            solana_sdk::instruction::AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data,
    };
    let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    env.svm
        .send_transaction(tx)
        .map(|_| ())
        .map_err(|e| format!("{:?}", e))
}

/// Iteration 2.1 — wrapper rejects `max_price_move_bps_per_slot == 0`
/// before reaching the engine. The wrapper-side prevalidation
/// (`InvalidConfigParam`) shadows the engine-side `Overflow` so callers
/// get a stable, surface-level error code.
#[test]
fn test_attack_init_market_zero_max_price_move_rejected() {
    let mut env = TestEnv::new();
    let admin_pk = env.payer.pubkey();
    let mint = env.mint;

    let mut data = encode_init_market_with_cap(&admin_pk, &mint, &TEST_FEED_ID, 0, 80);
    // Sanity: the offset must currently hold the test's nonzero default.
    let original = u64::from_le_bytes(
        data[MAX_PRICE_MOVE_OFFSET..MAX_PRICE_MOVE_OFFSET + 8]
            .try_into()
            .unwrap(),
    );
    assert_ne!(
        original, 0,
        "encoder should emit a nonzero max_price_move (got {}); \
         offset has drifted — update MAX_PRICE_MOVE_OFFSET",
        original
    );
    data[MAX_PRICE_MOVE_OFFSET..MAX_PRICE_MOVE_OFFSET + 8].copy_from_slice(&0u64.to_le_bytes());

    let result = send_init_market_raw(&mut env, data);
    assert!(
        result.is_err(),
        "InitMarket with max_price_move_bps_per_slot=0 must reject: result={:?}",
        result
    );
    let err_str = result.unwrap_err();
    assert!(
        err_str.contains(&format!("Custom({})", INVALID_CONFIG_PARAM)),
        "expected wrapper-level InvalidConfigParam (Custom({})); engine-side \
         rejection would be Custom(18) Overflow. Got: {}",
        INVALID_CONFIG_PARAM,
        err_str
    );
}

/// Iteration 2.2 — `ResolveMarket` wire-format mode byte must reject
/// values > 1. Verifies the explicit Ordinary | Degenerate split
/// (commit a7186d5) doesn't silently fall through to a default arm
/// when a malformed payload arrives.
#[test]
fn test_attack_resolve_market_invalid_mode_rejected() {
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 80);
    env.crank();
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Build a raw ResolveMarket payload with mode = 2 (illegal).
    let mut data = vec![19u8]; // tag
    data.push(2u8); // mode = 2 — must reject before reaching the handler

    let ix = solana_sdk::instruction::Instruction {
        program_id: env.program_id,
        accounts: vec![
            solana_sdk::instruction::AccountMeta::new(admin.pubkey(), true),
            solana_sdk::instruction::AccountMeta::new(env.slab, false),
            solana_sdk::instruction::AccountMeta::new_readonly(
                solana_sdk::sysvar::clock::ID,
                false,
            ),
            solana_sdk::instruction::AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data,
    };
    let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ResolveMarket with mode=2 must reject as invalid instruction data"
    );
    // Rejection must happen in the parser (InvalidInstructionData), not in
    // the handler (which would imply mode=2 leaked through to a branch).
    let err_str = format!("{:?}", result.unwrap_err());
    assert!(
        err_str.contains("InvalidInstructionData"),
        "expected InvalidInstructionData on mode=2; got: {}",
        err_str
    );
    assert!(
        !env.is_market_resolved(),
        "rejected ResolveMarket must not flip the resolved flag"
    );
}
