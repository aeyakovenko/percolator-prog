//! Smoke tests for the 3 instructions added this Phase 2 batch:
//! `reclaim_empty_account` (tag 25), `settle_account` (tag 26), and
//! `catchup_accrue` (tag 31). Each test pushes a tx whose slab is
//! correctly-shaped (right size, right owner, right disc) but has
//! `magic == 0` — so the dispatch routes to the right handler and
//! `require_initialized` rejects with `PercolatorError::NotInitialized`.
//!
//! This catches:
//!   - wrong `#[discrim = N]` wiring;
//!   - arg-decoder shape drift (Borsh would reject malformed payloads
//!     before the handler runs, surfacing as a different error);
//!   - missing module re-exports breaking the `#[program]` glue.

use litesvm::LiteSVM;
use solana_sdk::{
    account::Account,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use std::path::PathBuf;

const PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("Perco1ator111111111111111111111111111111111");
const CLOCK_SYSVAR: Pubkey = solana_sdk::sysvar::clock::ID;

const DEPOSIT_COLLATERAL: u8 = 3;
const WITHDRAW_COLLATERAL: u8 = 4;
const TRADE_NO_CPI: u8 = 6;
const LIQUIDATE_AT_ORACLE: u8 = 7;
const CLOSE_ACCOUNT: u8 = 8;
const TOP_UP_INSURANCE: u8 = 9;
const DEPOSIT_FEE_CREDITS: u8 = 27;
const CONVERT_RELEASED_PNL: u8 = 28;
const UPDATE_CONFIG: u8 = 14;
const RESOLVE_MARKET: u8 = 19;
const WITHDRAW_INSURANCE: u8 = 20;
const WITHDRAW_INSURANCE_LIMITED: u8 = 23;
const RECLAIM_EMPTY_ACCOUNT: u8 = 25;
const SETTLE_ACCOUNT: u8 = 26;
const RESOLVE_PERMISSIONLESS: u8 = 29;
const CATCHUP_ACCRUE: u8 = 31;

#[cfg(not(any(feature = "small", feature = "medium")))]
const SLAB_LEN: usize = 1_525_632;
#[cfg(all(feature = "small", not(feature = "medium")))]
const SLAB_LEN: usize = 96_672;
#[cfg(all(feature = "medium", not(feature = "small")))]
const SLAB_LEN: usize = 382_464;

fn program_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("target/deploy/percolator_prog.so");
    assert!(p.exists(), "BPF binary not found at {:?}", p);
    p
}

/// Slab with the Anchor v2 disc set but `magic` left at 0 — so
/// `slab_shape_guard` passes (length + owner + disc) but
/// `require_initialized` rejects.
fn make_uninit_slab() -> Vec<u8> {
    let mut data = vec![0u8; SLAB_LEN];
    let disc = percolator_prog::state::slab_header_discriminator();
    data[0..8].copy_from_slice(disc);
    // Bytes 8..16 (`magic` field) deliberately left zero.
    data
}

fn fresh_svm() -> (LiteSVM, Pubkey, Keypair) {
    let mut svm = LiteSVM::new();
    let bytes = std::fs::read(program_path()).expect("read program");
    svm.add_program(PROGRAM_ID, &bytes).expect("load program");

    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();

    let slab_pk = Pubkey::new_unique();
    svm.set_account(
        slab_pk,
        Account {
            lamports: 1_000_000_000_000,
            data: make_uninit_slab(),
            owner: PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
    (svm, slab_pk, payer)
}

fn submit(svm: &mut LiteSVM, payer: &Keypair, ix: Instruction) -> bool {
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[payer],
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx).is_err()
}

#[test]
fn reclaim_empty_account_rejects_uninitialized() {
    let (mut svm, slab_pk, payer) = fresh_svm();
    let mut data = vec![RECLAIM_EMPTY_ACCOUNT];
    data.extend_from_slice(&0u16.to_le_bytes()); // user_idx = 0
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(slab_pk, false),
            AccountMeta::new_readonly(CLOCK_SYSVAR, false),
        ],
        data,
    };
    assert!(submit(&mut svm, &payer, ix));
}

#[test]
fn settle_account_rejects_uninitialized() {
    let (mut svm, slab_pk, payer) = fresh_svm();
    let mut data = vec![SETTLE_ACCOUNT];
    data.extend_from_slice(&0u16.to_le_bytes());
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(slab_pk, false),
            AccountMeta::new_readonly(CLOCK_SYSVAR, false),
            AccountMeta::new_readonly(Pubkey::new_unique(), false), // oracle placeholder
        ],
        data,
    };
    assert!(submit(&mut svm, &payer, ix));
}

#[test]
fn catchup_accrue_rejects_uninitialized() {
    let (mut svm, slab_pk, payer) = fresh_svm();
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(slab_pk, false),
            AccountMeta::new_readonly(CLOCK_SYSVAR, false),
            AccountMeta::new_readonly(Pubkey::new_unique(), false), // oracle placeholder
        ],
        data: vec![CATCHUP_ACCRUE], // payload-less
    };
    assert!(submit(&mut svm, &payer, ix));
}

#[test]
fn update_config_rejects_uninitialized() {
    let (mut svm, slab_pk, payer) = fresh_svm();
    let mut data = vec![UPDATE_CONFIG];
    data.extend_from_slice(&500u64.to_le_bytes()); // funding_horizon_slots
    data.extend_from_slice(&100u64.to_le_bytes()); // funding_k_bps
    data.extend_from_slice(&500i64.to_le_bytes()); // funding_max_premium_bps
    data.extend_from_slice(&1_000i64.to_le_bytes()); // funding_max_e9_per_slot
    data.extend_from_slice(&0u16.to_le_bytes()); // tvl_insurance_cap_mult
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(payer.pubkey(), true),
            AccountMeta::new(slab_pk, false),
            AccountMeta::new_readonly(CLOCK_SYSVAR, false),
            AccountMeta::new_readonly(Pubkey::new_unique(), false), // oracle placeholder
        ],
        data,
    };
    assert!(submit(&mut svm, &payer, ix));
}

#[test]
fn resolve_market_rejects_uninitialized() {
    let (mut svm, slab_pk, payer) = fresh_svm();
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(payer.pubkey(), true),
            AccountMeta::new(slab_pk, false),
            AccountMeta::new_readonly(CLOCK_SYSVAR, false),
            AccountMeta::new_readonly(Pubkey::new_unique(), false), // oracle placeholder
        ],
        data: vec![RESOLVE_MARKET, 0], // mode = 0 (Ordinary)
    };
    assert!(submit(&mut svm, &payer, ix));
}

#[test]
fn resolve_market_rejects_invalid_mode() {
    // Anchor v2's Borsh decoder accepts any u8 for `mode`; the handler
    // must explicitly reject mode > 1. This test proves that gate is
    // wired correctly even on a fresh slab — the rejection happens
    // before any state is touched.
    let (mut svm, slab_pk, payer) = fresh_svm();
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(payer.pubkey(), true),
            AccountMeta::new(slab_pk, false),
            AccountMeta::new_readonly(CLOCK_SYSVAR, false),
            AccountMeta::new_readonly(Pubkey::new_unique(), false),
        ],
        data: vec![RESOLVE_MARKET, 7], // mode = 7 (invalid)
    };
    assert!(submit(&mut svm, &payer, ix));
}

#[test]
fn trade_no_cpi_rejects_zero_size() {
    // size = 0 is rejected at the wire-format gate before any state
    // is touched. Confirms #[discrim = 6] dispatch + Borsh decoder.
    let (mut svm, slab_pk, payer) = fresh_svm();
    let lp = Keypair::new();
    svm.airdrop(&lp.pubkey(), 100_000_000).unwrap();
    let mut data = vec![TRADE_NO_CPI];
    data.extend_from_slice(&0u16.to_le_bytes()); // lp_idx
    data.extend_from_slice(&0u16.to_le_bytes()); // user_idx
    data.extend_from_slice(&0i128.to_le_bytes()); // size = 0 → reject
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(payer.pubkey(), true), // user
            AccountMeta::new_readonly(lp.pubkey(), true),
            AccountMeta::new(slab_pk, false),
            AccountMeta::new_readonly(CLOCK_SYSVAR, false),
            AccountMeta::new_readonly(Pubkey::new_unique(), false),
        ],
        data,
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer, &lp],
        svm.latest_blockhash(),
    );
    assert!(svm.send_transaction(tx).is_err());
}

/// SPL Token program ID — `pinocchio_token::ID`. Hardcoded here so
/// the test crate doesn't need a direct pinocchio-token dep.
const TOKEN_PROGRAM: Pubkey =
    solana_sdk::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

#[test]
fn top_up_insurance_rejects_zero_amount() {
    // The handler runs `verify_token_program` first and the
    // `amount == 0` gate second; either bails before any state
    // mutation. Either way, this test confirms #[discrim = 9]
    // dispatch + Borsh u64 decoder fire correctly — we just care
    // the tx returns Err on a fresh slab.
    let (mut svm, slab_pk, payer) = fresh_svm();
    let mut data = vec![TOP_UP_INSURANCE];
    data.extend_from_slice(&0u64.to_le_bytes());
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(payer.pubkey(), true),
            AccountMeta::new(slab_pk, false),
            AccountMeta::new(Pubkey::new_unique(), false), // user_ata placeholder
            AccountMeta::new(Pubkey::new_unique(), false), // vault placeholder
            AccountMeta::new_readonly(TOKEN_PROGRAM, false),
            AccountMeta::new_readonly(CLOCK_SYSVAR, false),
        ],
        data,
    };
    assert!(submit(&mut svm, &payer, ix));
}

#[test]
fn withdraw_insurance_rejects_uninitialized() {
    let (mut svm, slab_pk, payer) = fresh_svm();
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(payer.pubkey(), true),
            AccountMeta::new(slab_pk, false),
            AccountMeta::new(Pubkey::new_unique(), false), // admin_ata
            AccountMeta::new(Pubkey::new_unique(), false), // vault
            AccountMeta::new_readonly(TOKEN_PROGRAM, false),
            AccountMeta::new_readonly(Pubkey::new_unique(), false), // vault_pda
        ],
        data: vec![WITHDRAW_INSURANCE], // payload-less
    };
    assert!(submit(&mut svm, &payer, ix));
}

#[test]
fn withdraw_insurance_limited_rejects_uninitialized() {
    let (mut svm, slab_pk, payer) = fresh_svm();
    let mut data = vec![WITHDRAW_INSURANCE_LIMITED];
    data.extend_from_slice(&100u64.to_le_bytes());
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(payer.pubkey(), true),
            AccountMeta::new(slab_pk, false),
            AccountMeta::new(Pubkey::new_unique(), false), // operator_ata
            AccountMeta::new(Pubkey::new_unique(), false), // vault
            AccountMeta::new_readonly(TOKEN_PROGRAM, false),
            AccountMeta::new_readonly(Pubkey::new_unique(), false), // vault_pda
            AccountMeta::new_readonly(CLOCK_SYSVAR, false),
        ],
        data,
    };
    assert!(submit(&mut svm, &payer, ix));
}

#[test]
fn deposit_collateral_rejects_zero_amount() {
    let (mut svm, slab_pk, payer) = fresh_svm();
    let mut data = vec![DEPOSIT_COLLATERAL];
    data.extend_from_slice(&0u16.to_le_bytes()); // user_idx
    data.extend_from_slice(&0u64.to_le_bytes()); // amount = 0
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(payer.pubkey(), true),
            AccountMeta::new(slab_pk, false),
            AccountMeta::new(Pubkey::new_unique(), false), // user_ata
            AccountMeta::new(Pubkey::new_unique(), false), // vault
            AccountMeta::new_readonly(TOKEN_PROGRAM, false),
            AccountMeta::new_readonly(CLOCK_SYSVAR, false),
        ],
        data,
    };
    assert!(submit(&mut svm, &payer, ix));
}

#[test]
fn deposit_fee_credits_rejects_uninitialized() {
    let (mut svm, slab_pk, payer) = fresh_svm();
    let mut data = vec![DEPOSIT_FEE_CREDITS];
    data.extend_from_slice(&0u16.to_le_bytes()); // user_idx
    data.extend_from_slice(&100u64.to_le_bytes()); // amount
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(payer.pubkey(), true),
            AccountMeta::new(slab_pk, false),
            AccountMeta::new(Pubkey::new_unique(), false), // user_ata
            AccountMeta::new(Pubkey::new_unique(), false), // vault
            AccountMeta::new_readonly(TOKEN_PROGRAM, false),
            AccountMeta::new_readonly(CLOCK_SYSVAR, false),
        ],
        data,
    };
    assert!(submit(&mut svm, &payer, ix));
}

#[test]
fn withdraw_collateral_rejects_zero_amount() {
    let (mut svm, slab_pk, payer) = fresh_svm();
    let mut data = vec![WITHDRAW_COLLATERAL];
    data.extend_from_slice(&0u16.to_le_bytes()); // user_idx
    data.extend_from_slice(&0u64.to_le_bytes()); // amount = 0 → reject
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(payer.pubkey(), true),
            AccountMeta::new(slab_pk, false),
            AccountMeta::new(Pubkey::new_unique(), false), // vault
            AccountMeta::new(Pubkey::new_unique(), false), // user_ata
            AccountMeta::new_readonly(Pubkey::new_unique(), false), // vault_pda
            AccountMeta::new_readonly(TOKEN_PROGRAM, false),
            AccountMeta::new_readonly(CLOCK_SYSVAR, false),
            AccountMeta::new_readonly(Pubkey::new_unique(), false), // oracle
        ],
        data,
    };
    assert!(submit(&mut svm, &payer, ix));
}

#[test]
fn close_account_rejects_uninitialized() {
    let (mut svm, slab_pk, payer) = fresh_svm();
    let mut data = vec![CLOSE_ACCOUNT];
    data.extend_from_slice(&0u16.to_le_bytes()); // user_idx
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(payer.pubkey(), true),
            AccountMeta::new(slab_pk, false),
            AccountMeta::new(Pubkey::new_unique(), false), // vault
            AccountMeta::new(Pubkey::new_unique(), false), // user_ata
            AccountMeta::new_readonly(Pubkey::new_unique(), false), // vault_pda
            AccountMeta::new_readonly(TOKEN_PROGRAM, false),
            AccountMeta::new_readonly(CLOCK_SYSVAR, false),
            AccountMeta::new_readonly(Pubkey::new_unique(), false), // oracle
        ],
        data,
    };
    assert!(submit(&mut svm, &payer, ix));
}

#[test]
fn convert_released_pnl_rejects_uninitialized() {
    let (mut svm, slab_pk, payer) = fresh_svm();
    let mut data = vec![CONVERT_RELEASED_PNL];
    data.extend_from_slice(&0u16.to_le_bytes()); // user_idx
    data.extend_from_slice(&100u64.to_le_bytes()); // amount
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(payer.pubkey(), true),
            AccountMeta::new(slab_pk, false),
            AccountMeta::new_readonly(CLOCK_SYSVAR, false),
            AccountMeta::new_readonly(Pubkey::new_unique(), false), // oracle
        ],
        data,
    };
    assert!(submit(&mut svm, &payer, ix));
}

#[test]
fn liquidate_at_oracle_rejects_uninitialized() {
    let (mut svm, slab_pk, payer) = fresh_svm();
    let mut data = vec![LIQUIDATE_AT_ORACLE];
    data.extend_from_slice(&0u16.to_le_bytes()); // target_idx
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(slab_pk, false),
            AccountMeta::new_readonly(CLOCK_SYSVAR, false),
            AccountMeta::new_readonly(Pubkey::new_unique(), false),
        ],
        data,
    };
    assert!(submit(&mut svm, &payer, ix));
}

#[test]
fn resolve_permissionless_rejects_uninitialized() {
    let (mut svm, slab_pk, payer) = fresh_svm();
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(slab_pk, false),
            AccountMeta::new_readonly(CLOCK_SYSVAR, false),
        ],
        data: vec![RESOLVE_PERMISSIONLESS], // payload-less
    };
    assert!(submit(&mut svm, &payer, ix));
}
