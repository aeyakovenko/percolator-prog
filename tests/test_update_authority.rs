//! Phase 2 integration test for `update_authority` (tag 32).
//!
//! Builds a minimal valid slab fixture (owner = program_id, length =
//! SLAB_LEN, MAGIC at body offset 0, admin at body offset 16) and
//! exercises the rotate / wrong-signer paths end-to-end against the
//! v2 BPF binary under LiteSVM.

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

/// Anchor v2 `#[discrim = 32]` on `update_authority`.
const UPDATE_AUTHORITY_DISCRIM: u8 = 32;
/// Account-data offset of the `admin` field inside `SlabHeader`:
/// disc(8) + magic(8) + version(4) + bump(1) + _padding(3) = 24.
const ADMIN_OFFSET: usize = 24;
/// Account-data offset of the body's `magic` field (post-disc).
const MAGIC_OFFSET: usize = 8;
/// `crate::constants::MAGIC = 0x504552434f4c4154` ("PERCOLAT", LE u64).
const MAGIC: u64 = 0x504552434f4c4154;

/// SLAB_LEN values mirror `percolator_prog::constants::SLAB_LEN` per
/// deployment tier (BPF-target u128 alignment differs from native, so we
/// hardcode rather than `use` the const). All values include the 8-byte
/// Anchor v2 disc prefix.
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

/// Build a slab buffer with the Anchor v2 disc + body MAGIC + admin
/// pre-populated and zeros elsewhere.
fn make_slab_data(admin: &Pubkey) -> Vec<u8> {
    let mut data = vec![0u8; SLAB_LEN];
    let disc = percolator_prog::state::slab_header_discriminator();
    data[0..8].copy_from_slice(disc);
    data[MAGIC_OFFSET..MAGIC_OFFSET + 8].copy_from_slice(&MAGIC.to_le_bytes());
    data[ADMIN_OFFSET..ADMIN_OFFSET + 32].copy_from_slice(&admin.to_bytes());
    data
}

/// Encode `[disc][kind][new_pubkey]` per the v2 wire format.
fn encode_update_authority(kind: u8, new_pubkey: &Pubkey) -> Vec<u8> {
    let mut buf = Vec::with_capacity(1 + 1 + 32);
    buf.push(UPDATE_AUTHORITY_DISCRIM);
    buf.push(kind);
    buf.extend_from_slice(&new_pubkey.to_bytes());
    buf
}

fn fresh_svm_with_admin(admin_kp: &Keypair) -> (LiteSVM, Pubkey) {
    let mut svm = LiteSVM::new();
    let bytes = std::fs::read(program_path()).expect("read program");
    svm.add_program(PROGRAM_ID, &bytes).expect("load program");

    svm.airdrop(&admin_kp.pubkey(), 1_000_000_000)
        .expect("airdrop admin");

    let slab_pk = Pubkey::new_unique();
    let slab_account = Account {
        lamports: 1_000_000_000_000,
        data: make_slab_data(&admin_kp.pubkey()),
        owner: PROGRAM_ID,
        executable: false,
        rent_epoch: 0,
    };
    svm.set_account(slab_pk, slab_account).expect("set slab");
    (svm, slab_pk)
}

#[test]
fn update_authority_rotates_admin() {
    let admin = Keypair::new();
    let new_admin = Keypair::new();
    let (mut svm, slab_pk) = fresh_svm_with_admin(&admin);
    svm.airdrop(&new_admin.pubkey(), 100_000_000)
        .expect("airdrop new admin");

    // kind = AUTHORITY_ADMIN (0). Both `current` and `new_authority` sign.
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(admin.pubkey(), true),
            AccountMeta::new_readonly(new_admin.pubkey(), true),
            AccountMeta::new(slab_pk, false),
        ],
        data: encode_update_authority(0, &new_admin.pubkey()),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&admin.pubkey()),
        &[&admin, &new_admin],
        svm.latest_blockhash(),
    );
    let result = svm.send_transaction(tx);
    assert!(
        result.is_ok(),
        "update_authority(rotate) tx should succeed; got: {:#?}",
        result.err(),
    );

    // Slab admin field must now hold new_admin's pubkey.
    let slab_after = svm.get_account(&slab_pk).expect("slab still exists");
    let admin_bytes = &slab_after.data[ADMIN_OFFSET..ADMIN_OFFSET + 32];
    assert_eq!(admin_bytes, &new_admin.pubkey().to_bytes());
}

#[test]
fn update_authority_rejects_wrong_signer() {
    let admin = Keypair::new();
    let imposter = Keypair::new();
    let new_admin = Keypair::new();
    let (mut svm, slab_pk) = fresh_svm_with_admin(&admin);
    svm.airdrop(&imposter.pubkey(), 100_000_000)
        .expect("airdrop imposter");
    svm.airdrop(&new_admin.pubkey(), 100_000_000)
        .expect("airdrop new admin");

    // imposter signs as `current` instead of admin — must trip
    // `require_admin` and return EngineUnauthorized.
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(imposter.pubkey(), true),
            AccountMeta::new_readonly(new_admin.pubkey(), true),
            AccountMeta::new(slab_pk, false),
        ],
        data: encode_update_authority(0, &new_admin.pubkey()),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&imposter.pubkey()),
        &[&imposter, &new_admin],
        svm.latest_blockhash(),
    );
    let result = svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "imposter tx must be rejected by require_admin",
    );

    // Admin field must remain unchanged.
    let slab_after = svm.get_account(&slab_pk).expect("slab still exists");
    let admin_bytes = &slab_after.data[ADMIN_OFFSET..ADMIN_OFFSET + 32];
    assert_eq!(admin_bytes, &admin.pubkey().to_bytes());
}
