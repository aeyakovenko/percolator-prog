//! Phase 2 integration test for `push_hyperp_mark` (tag 17).
//!
//! Covers the early-rejection paths that don't require a live engine:
//!   - non-Hyperp market (`config.index_feed_id != 0`) → EngineUnauthorized.
//!   - Hyperp market but wrong signer → EngineUnauthorized.

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

/// Anchor v2 `#[discrim = 17]` on `push_hyperp_mark`.
const PUSH_HYPERP_MARK_DISCRIM: u8 = 17;

/// `crate::constants::MAGIC = 0x504552434f4c4154` ("PERCOLAT", LE u64).
const MAGIC: u64 = 0x504552434f4c4154;

/// Account-data offsets — match `state.rs` field order.
/// disc(8) + magic(0..8) ⇒ MAGIC at 8.
const MAGIC_OFFSET: usize = 8;
/// disc(8) + magic(8) + version(4) + bump(1) + _padding(3) = 24.
const ADMIN_OFFSET: usize = 24;
/// MarketConfig starts after disc + SlabHeader. SlabHeader is 136 bytes
/// (8 + 4 + 1 + 3 + 32 + 24 + 32 + 32). Config base = 8 + 136 = 144.
const CONFIG_BASE: usize = 144;
/// `index_feed_id` is the third field in MarketConfig at config offset 64
/// (collateral_mint(32) + vault_pubkey(32)).
const INDEX_FEED_ID_OFFSET: usize = CONFIG_BASE + 64;
/// `hyperp_authority` sits at config offset 144 (after the 144-byte
/// preamble of mints/feeds/staleness/funding-config).
const HYPERP_AUTHORITY_OFFSET: usize = CONFIG_BASE + 144;

#[cfg(not(any(feature = "small", feature = "medium")))]
const SLAB_LEN: usize = 1_525_632;
#[cfg(all(feature = "small", not(feature = "medium")))]
const SLAB_LEN: usize = 96_672;
#[cfg(all(feature = "medium", not(feature = "small")))]
const SLAB_LEN: usize = 382_464;

fn program_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("target/deploy/percolator_prog.so");
    assert!(
        p.exists(),
        "BPF binary not found at {:?} — run `cargo build-sbf`",
        p,
    );
    p
}

/// Build a slab fixture. `index_feed_id` and `hyperp_authority` are
/// optional overrides on the otherwise-zeroed body.
fn make_slab_data(
    admin: &Pubkey,
    index_feed_id: Option<&[u8; 32]>,
    hyperp_authority: Option<&Pubkey>,
) -> Vec<u8> {
    let mut data = vec![0u8; SLAB_LEN];
    let disc = percolator_prog::state::slab_header_discriminator();
    data[0..8].copy_from_slice(disc);
    data[MAGIC_OFFSET..MAGIC_OFFSET + 8].copy_from_slice(&MAGIC.to_le_bytes());
    data[ADMIN_OFFSET..ADMIN_OFFSET + 32].copy_from_slice(&admin.to_bytes());
    if let Some(feed) = index_feed_id {
        data[INDEX_FEED_ID_OFFSET..INDEX_FEED_ID_OFFSET + 32].copy_from_slice(feed);
    }
    if let Some(auth) = hyperp_authority {
        data[HYPERP_AUTHORITY_OFFSET..HYPERP_AUTHORITY_OFFSET + 32]
            .copy_from_slice(&auth.to_bytes());
    }
    data
}

fn encode_push(price_e6: u64, timestamp: i64) -> Vec<u8> {
    let mut buf = Vec::with_capacity(1 + 8 + 8);
    buf.push(PUSH_HYPERP_MARK_DISCRIM);
    buf.extend_from_slice(&price_e6.to_le_bytes());
    buf.extend_from_slice(&timestamp.to_le_bytes());
    buf
}

fn fresh_svm(slab_data: Vec<u8>) -> (LiteSVM, Pubkey) {
    let mut svm = LiteSVM::new();
    let bytes = std::fs::read(program_path()).expect("read program");
    svm.add_program(PROGRAM_ID, &bytes).expect("load program");
    let slab_pk = Pubkey::new_unique();
    svm.set_account(
        slab_pk,
        Account {
            lamports: 1_000_000_000_000,
            data: slab_data,
            owner: PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .expect("set slab");
    (svm, slab_pk)
}

#[test]
fn rejects_non_hyperp_market() {
    let admin = Keypair::new();
    // Non-zero index_feed_id ⇒ external-oracle mode, PushHyperpMark
    // must reject regardless of whatever signer signs.
    let feed = [0xABu8; 32];
    let auth = Keypair::new();
    let data = make_slab_data(&admin.pubkey(), Some(&feed), Some(&auth.pubkey()));
    let (mut svm, slab_pk) = fresh_svm(data);
    svm.airdrop(&auth.pubkey(), 1_000_000_000).unwrap();

    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(auth.pubkey(), true),
            AccountMeta::new(slab_pk, false),
        ],
        data: encode_push(1_000_000, 0),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&auth.pubkey()),
        &[&auth],
        svm.latest_blockhash(),
    );
    assert!(
        svm.send_transaction(tx).is_err(),
        "non-Hyperp market must reject PushHyperpMark"
    );
}

#[test]
fn rejects_wrong_authority() {
    let admin = Keypair::new();
    let registered = Keypair::new();
    let imposter = Keypair::new();
    // Hyperp mode (index_feed_id = 0) but `hyperp_authority` is the
    // registered key. The imposter signs and is rejected.
    let data = make_slab_data(&admin.pubkey(), None, Some(&registered.pubkey()));
    let (mut svm, slab_pk) = fresh_svm(data);
    svm.airdrop(&imposter.pubkey(), 1_000_000_000).unwrap();

    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(imposter.pubkey(), true),
            AccountMeta::new(slab_pk, false),
        ],
        data: encode_push(1_000_000, 0),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&imposter.pubkey()),
        &[&imposter],
        svm.latest_blockhash(),
    );
    assert!(
        svm.send_transaction(tx).is_err(),
        "wrong-authority signer must be rejected"
    );
}
