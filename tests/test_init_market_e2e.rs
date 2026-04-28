//! Phase 6 proof-of-concept — drives a real `InitMarket` (tag 0)
//! end-to-end against the v2 BPF binary under LiteSVM, asserts that
//! the slab is materialized correctly, then exercises a follow-up
//! ResolveMarket reject path that depends on a real (post-init) slab.
//!
//! Self-contained: no `mod common`. Demonstrates the v2 wire format
//! (wincode/Borsh-encoded `InitMarketArgs`) and the
//! disc-prefixed slab + Mint + Vault + PriceUpdateV2 fixture pattern
//! that the rest of the legacy test suite will lift.
//!
//! Slab layout (v2): `[8 byte disc] [SlabHeader] [MarketConfig]
//! [RiskEngine] [RiskBuffer] [generation table]`. For `#[account(zeroed)]`
//! to pass, the pre-allocated buffer MUST be all zeros — Anchor stamps
//! the disc on entry.

use litesvm::LiteSVM;
use solana_sdk::{
    account::Account,
    clock::Clock,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    sysvar,
    transaction::Transaction,
};
use std::path::PathBuf;

const PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("Perco1ator111111111111111111111111111111111");
const TOKEN_PROGRAM: Pubkey =
    solana_sdk::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const PYTH_RECEIVER_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
    0x0c, 0xb7, 0xfa, 0xbb, 0x52, 0xf7, 0xa6, 0x48, 0xbb, 0x5b, 0x31, 0x7d, 0x9a, 0x01, 0x8b, 0x90,
    0x57, 0xcb, 0x02, 0x47, 0x74, 0xfa, 0xfe, 0x01, 0xe6, 0xc4, 0xdf, 0x98, 0xcc, 0x38, 0x58, 0x81,
]);

const TEST_FEED_ID: [u8; 32] = [0xAB; 32];

// BPF-target slab length per deployment tier. u128 align=8 on sbf vs
// 16 on x86_64, so this can't be derived from `size_of` at host
// compile time. Includes the 8-byte Anchor v2 disc prefix.
#[cfg(not(any(feature = "small", feature = "medium")))]
const SLAB_LEN: usize = 1_525_632;
#[cfg(all(feature = "small", not(feature = "medium")))]
const SLAB_LEN: usize = 96_672;
#[cfg(all(feature = "medium", not(feature = "small")))]
const SLAB_LEN: usize = 382_464;

#[cfg(not(any(feature = "small", feature = "medium")))]
const MAX_ACCOUNTS: u64 = 4096;
#[cfg(all(feature = "small", not(feature = "medium")))]
const MAX_ACCOUNTS: u64 = 256;
#[cfg(all(feature = "medium", not(feature = "small")))]
const MAX_ACCOUNTS: u64 = 1024;

const TEST_MAX_PRICE_MOVE_BPS_PER_SLOT: u64 = 4;
const TEST_MAX_STALENESS_SECS: u64 = 60;

/// Body offsets inside the disc-prefixed slab. Body starts at
/// `DISC_LEN = 8`; offsets here are absolute (data-relative).
const HEADER_OFF: usize = 8;
const MAGIC_OFF: usize = HEADER_OFF + 0;
const ADMIN_OFF: usize = HEADER_OFF + 16; // disc(8) + magic(8) + version(4) + bump(1) + _padding[3]
const CONFIG_OFF: usize = HEADER_OFF + 136; // SlabHeader = 8+4+1+3+32+24+32+32

/// `crate::constants::MAGIC = 0x504552434f4c4154` ("PERCOLAT", LE).
const MAGIC: u64 = 0x504552434f4c4154;

fn program_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("target/deploy/percolator_prog.so");
    assert!(p.exists(), "BPF binary not found at {:?} — run cargo build-sbf", p);
    p
}

/// `[SetComputeUnitLimit(2u8) | units (u32 LE)]` — direct compute-budget
/// instruction encoding, avoids pulling in
/// `solana-compute-budget-interface` as a dev-dep.
fn cu_ix() -> Instruction {
    const COMPUTE_BUDGET_PROGRAM: Pubkey =
        solana_sdk::pubkey!("ComputeBudget111111111111111111111111111111");
    let mut data = vec![2u8];
    data.extend_from_slice(&1_400_000u32.to_le_bytes());
    Instruction {
        program_id: COMPUTE_BUDGET_PROGRAM,
        accounts: vec![],
        data,
    }
}

// ── SPL Token v1 fixture data ────────────────────────────────────────────────

/// SPL Token Mint v1 layout: 82 bytes total.
///   mint_authority (COption<Pubkey>): 4 + 32
///   supply (u64): 8
///   decimals (u8): 1
///   is_initialized (u8): 1
///   freeze_authority (COption<Pubkey>): 4 + 32
fn make_mint_data() -> Vec<u8> {
    let mut data = vec![0u8; 82];
    // mint_authority = None: COption tag 0 + 32 zero bytes.
    // supply = 0
    data[36..44].copy_from_slice(&0u64.to_le_bytes());
    data[44] = 6; // decimals
    data[45] = 1; // is_initialized
    // freeze_authority = None.
    data
}

/// SPL Token Account v1 layout: 165 bytes total.
fn make_token_account_data(mint: &Pubkey, owner: &Pubkey, amount: u64) -> Vec<u8> {
    let mut data = vec![0u8; 165];
    data[0..32].copy_from_slice(&mint.to_bytes());
    data[32..64].copy_from_slice(&owner.to_bytes());
    data[64..72].copy_from_slice(&amount.to_le_bytes());
    // delegate COption: tag at 72 = 0 (None), bytes 76..108 zero.
    data[108] = 1; // state = Initialized
    // is_native COption (109..121): None.
    // delegated_amount (121..129): 0.
    // close_authority COption (129..165): None.
    data
}

/// PriceUpdateV2 mock — Pyth Solana Receiver v2 account, Full
/// verification level. Layout matches mainnet snapshots.
fn make_pyth_data(feed_id: &[u8; 32], price: i64, expo: i32, conf: u64, publish_time: i64) -> Vec<u8> {
    let mut data = vec![0u8; 134];
    data[0..8].copy_from_slice(&[0x22, 0xf1, 0x23, 0x63, 0x9d, 0x7e, 0xf4, 0xcd]);
    data[40] = 1; // VerificationLevel::Full
    data[41..73].copy_from_slice(feed_id);
    data[73..81].copy_from_slice(&price.to_le_bytes());
    data[81..89].copy_from_slice(&conf.to_le_bytes());
    data[89..93].copy_from_slice(&expo.to_le_bytes());
    data[93..101].copy_from_slice(&publish_time.to_le_bytes());
    data
}

// ── v2 InitMarketArgs encoder ────────────────────────────────────────────────

/// Hand-encode the v2 `InitMarketArgs` struct via Borsh's fixed-int LE
/// schema (mirrors wincode `BORSH_CONFIG`). Emits:
///   admin[32] mint[32] feed_id[32]
///   max_staleness_secs(u64) conf_filter_bps(u16) invert(u8) unit_scale(u32)
///   initial_mark_price_e6(u64) maintenance_fee_per_slot(u128)
///   risk_params{ … 14 fields … }
///   insurance_withdraw_max_bps(u16) insurance_withdraw_cooldown_slots(u64)
///   permissionless_resolve_stale_slots(u64)
///   funding_horizon_slots(Option<u64>) funding_k_bps(Option<u64>)
///   funding_max_premium_bps(Option<i64>) funding_max_e9_per_slot(Option<i64>)
///   mark_min_fee(u64) force_close_delay_slots(u64)
fn encode_init_market(admin: &Pubkey, mint: &Pubkey, feed_id: &[u8; 32]) -> Vec<u8> {
    let mut data = vec![0u8]; // tag 0
    data.extend_from_slice(&admin.to_bytes());
    data.extend_from_slice(&mint.to_bytes());
    data.extend_from_slice(feed_id);
    data.extend_from_slice(&TEST_MAX_STALENESS_SECS.to_le_bytes());
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(0u8); // invert
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
    data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6 (non-Hyperp uses oracle)
    data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee_per_slot

    // RiskParamsArgs (14 fields, declaration order):
    data.extend_from_slice(&1u64.to_le_bytes()); // h_min
    data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps
    data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // trading_fee_bps
    data.extend_from_slice(&MAX_ACCOUNTS.to_le_bytes());
    data.extend_from_slice(&1u128.to_le_bytes()); // new_account_fee
    data.extend_from_slice(&1u64.to_le_bytes()); // h_max
    data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
    data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
    data.extend_from_slice(&100u64.to_le_bytes()); // resolve_price_deviation_bps
    data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
    data.extend_from_slice(&21u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&22u128.to_le_bytes()); // min_nonzero_im_req
    data.extend_from_slice(&TEST_MAX_PRICE_MOVE_BPS_PER_SLOT.to_le_bytes());

    // Tail
    data.extend_from_slice(&0u16.to_le_bytes()); // insurance_withdraw_max_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // insurance_withdraw_cooldown_slots
    data.extend_from_slice(&80u64.to_le_bytes()); // permissionless_resolve_stale_slots

    // Option<u64>/Option<i64> — Borsh: 1-byte tag + payload if Some.
    // All four set to None (use wrapper defaults).
    data.push(0); // funding_horizon_slots: None
    data.push(0); // funding_k_bps: None
    data.push(0); // funding_max_premium_bps: None
    data.push(0); // funding_max_e9_per_slot: None

    data.extend_from_slice(&0u64.to_le_bytes()); // mark_min_fee
    data.extend_from_slice(&50u64.to_le_bytes()); // force_close_delay_slots
    data
}

// ── Test fixture builder ─────────────────────────────────────────────────────

struct TestEnv {
    svm: LiteSVM,
    payer: Keypair,
    slab: Pubkey,
    mint: Pubkey,
    vault: Pubkey,
    pyth: Pubkey,
}

impl TestEnv {
    fn new() -> Self {
        let mut svm = LiteSVM::new();
        let bytes = std::fs::read(program_path()).expect("read program");
        svm.add_program(PROGRAM_ID, &bytes).expect("load program");

        let payer = Keypair::new();
        svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();

        let slab = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let pyth = Pubkey::new_unique();
        let (vault_authority, _) =
            Pubkey::find_program_address(&[b"vault", slab.as_ref()], &PROGRAM_ID);
        let vault = Pubkey::new_unique();

        // Slab — pre-allocated, fully zero (so #[account(zeroed)] passes).
        svm.set_account(
            slab,
            Account {
                lamports: 1_000_000_000_000,
                data: vec![0u8; SLAB_LEN],
                owner: PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
        // Mint — owned by SPL Token program.
        svm.set_account(
            mint,
            Account {
                lamports: 1_000_000,
                data: make_mint_data(),
                owner: TOKEN_PROGRAM,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
        // Vault token account — owner is vault PDA, balance 0.
        svm.set_account(
            vault,
            Account {
                lamports: 1_000_000,
                data: make_token_account_data(&mint, &vault_authority, 0),
                owner: TOKEN_PROGRAM,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
        // Pyth oracle.
        svm.set_account(
            pyth,
            Account {
                lamports: 1_000_000,
                data: make_pyth_data(&TEST_FEED_ID, 138_000_000, -6, 1, 100),
                owner: PYTH_RECEIVER_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
        svm.set_sysvar(&Clock { slot: 100, unix_timestamp: 100, ..Clock::default() });

        TestEnv { svm, payer, slab, mint, vault, pyth }
    }

    fn init_market_ix(&self) -> Instruction {
        let admin = self.payer.pubkey();
        Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(admin, true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(self.mint, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth, false),
            ],
            data: encode_init_market(&admin, &self.mint, &TEST_FEED_ID),
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn init_market_succeeds_and_writes_header() {
    let mut env = TestEnv::new();
    let admin = env.payer.pubkey();
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), env.init_market_ix()],
        Some(&admin),
        &[&env.payer],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_ok(),
        "InitMarket happy-path tx must succeed; got: {:#?}",
        result.err()
    );

    let slab = env.svm.get_account(&env.slab).expect("slab still exists").data;
    assert_eq!(slab.len(), SLAB_LEN);

    // Anchor v2 stamps the 8-byte disc on entry.
    let disc = percolator_prog::state::slab_header_discriminator();
    assert_eq!(&slab[0..8], disc, "Anchor v2 disc must be stamped");

    // Body MAGIC + admin must be set by the handler.
    let magic =
        u64::from_le_bytes(slab[MAGIC_OFF..MAGIC_OFF + 8].try_into().unwrap());
    assert_eq!(magic, MAGIC, "MAGIC must be written");
    assert_eq!(
        &slab[ADMIN_OFF..ADMIN_OFF + 32],
        &admin.to_bytes(),
        "admin field must equal the signer"
    );

    // Spot-check: collateral_mint at MarketConfig offset 0.
    assert_eq!(
        &slab[CONFIG_OFF..CONFIG_OFF + 32],
        &env.mint.to_bytes(),
        "MarketConfig.collateral_mint must equal the mint passed in"
    );
}

#[test]
fn init_market_rejects_admin_signer_mismatch() {
    // Args.admin field must equal the signer's pubkey.
    let mut env = TestEnv::new();
    let admin = env.payer.pubkey();
    let bogus_admin = Pubkey::new_unique();
    let mut data = encode_init_market(&bogus_admin, &env.mint, &TEST_FEED_ID);
    // tag(1) + admin field starts at offset 1, runs 32 bytes.
    assert_eq!(&data[1..33], &bogus_admin.to_bytes());
    // Sanity: lift the malformed arg through the dispatcher.
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: env.init_market_ix().accounts,
        data: std::mem::take(&mut data),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin),
        &[&env.payer],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "InitMarket with admin field != signer must reject"
    );
}

#[test]
fn resolve_market_post_init_rejects_mode_two() {
    // Init the market, then try ResolveMarket with mode=2 (illegal).
    // The legacy `test_attack_resolve_market_invalid_mode_rejected`
    // exercises this exact path; the gate now lives in the
    // resolve_market handler.
    let mut env = TestEnv::new();
    let admin = env.payer.pubkey();
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), env.init_market_ix()],
        Some(&admin),
        &[&env.payer],
        env.svm.latest_blockhash(),
    );
    env.svm.send_transaction(tx).expect("init_market succeeds");

    // Now try ResolveMarket with mode = 2.
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(admin, true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth, false),
        ],
        data: vec![19u8, 2u8],
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin),
        &[&env.payer],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ResolveMarket with mode=2 on initialized market must reject"
    );
}
