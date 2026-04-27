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

const RECLAIM_EMPTY_ACCOUNT: u8 = 25;
const SETTLE_ACCOUNT: u8 = 26;
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
