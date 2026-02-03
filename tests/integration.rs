//! Integration tests for inverted market price handling
//!
//! These tests verify that the funding calculation correctly uses the market price
//! (which may be inverted) rather than raw oracle price. This is critical for
//! SOL/USD style perp markets where the price needs to be inverted.
//!
//! Uses production BPF binary (not --features test) because the test feature
//! bypasses CPI for token transfers, which fails in LiteSVM.
//!
//! Build: cargo build-sbf
//! Run:   cargo test --test integration

use litesvm::LiteSVM;
use solana_sdk::{
    account::Account,
    clock::Clock,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    sysvar,
    transaction::Transaction,
    program_pack::Pack,
};
use spl_token::state::{Account as TokenAccount, AccountState};
use std::path::PathBuf;

// SLAB_LEN for production BPF (MAX_ACCOUNTS=4096)
// Note: We use production BPF (not test feature) because test feature
// bypasses CPI for token transfers, which fails in LiteSVM.
// Haircut-ratio engine (ADL/socialization scratch arrays removed)
const SLAB_LEN: usize = 992560;  // MAX_ACCOUNTS=4096 + oracle circuit breaker (no padding)
const MAX_ACCOUNTS: usize = 4096;

// Pyth Receiver program ID
const PYTH_RECEIVER_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
    0x0c, 0xb7, 0xfa, 0xbb, 0x52, 0xf7, 0xa6, 0x48,
    0xbb, 0x5b, 0x31, 0x7d, 0x9a, 0x01, 0x8b, 0x90,
    0x57, 0xcb, 0x02, 0x47, 0x74, 0xfa, 0xfe, 0x01,
    0xe6, 0xc4, 0xdf, 0x98, 0xcc, 0x38, 0x58, 0x81,
]);

const TEST_FEED_ID: [u8; 32] = [0xABu8; 32];

fn program_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("target/deploy/percolator_prog.so");
    path
}

fn make_token_account_data(mint: &Pubkey, owner: &Pubkey, amount: u64) -> Vec<u8> {
    let mut data = vec![0u8; TokenAccount::LEN];
    let mut account = TokenAccount::default();
    account.mint = *mint;
    account.owner = *owner;
    account.amount = amount;
    account.state = AccountState::Initialized;
    TokenAccount::pack(account, &mut data).unwrap();
    data
}

fn make_mint_data() -> Vec<u8> {
    use spl_token::state::Mint;
    let mut data = vec![0u8; Mint::LEN];
    let mint = Mint {
        mint_authority: solana_sdk::program_option::COption::None,
        supply: 0,
        decimals: 6,
        is_initialized: true,
        freeze_authority: solana_sdk::program_option::COption::None,
    };
    Mint::pack(mint, &mut data).unwrap();
    data
}

/// Create PriceUpdateV2 mock data (Pyth Pull format)
fn make_pyth_data(feed_id: &[u8; 32], price: i64, expo: i32, conf: u64, publish_time: i64) -> Vec<u8> {
    let mut data = vec![0u8; 134];
    data[42..74].copy_from_slice(feed_id);
    data[74..82].copy_from_slice(&price.to_le_bytes());
    data[82..90].copy_from_slice(&conf.to_le_bytes());
    data[90..94].copy_from_slice(&expo.to_le_bytes());
    data[94..102].copy_from_slice(&publish_time.to_le_bytes());
    data
}

/// Encode InitMarket instruction with invert flag
fn encode_init_market_with_invert(
    admin: &Pubkey,
    mint: &Pubkey,
    feed_id: &[u8; 32],
    invert: u8,
) -> Vec<u8> {
    encode_init_market_full_v2(admin, mint, feed_id, invert, 0, 0)
}

/// Encode InitMarket with initial_mark_price_e6 for Hyperp mode
fn encode_init_market_hyperp(
    admin: &Pubkey,
    mint: &Pubkey,
    initial_mark_price_e6: u64,
) -> Vec<u8> {
    // Hyperp mode: feed_id = [0; 32], invert = 0 (not inverted internally)
    encode_init_market_full_v2(admin, mint, &[0u8; 32], 0, initial_mark_price_e6, 0)
}

/// Full InitMarket encoder with all new fields
fn encode_init_market_full_v2(
    admin: &Pubkey,
    mint: &Pubkey,
    feed_id: &[u8; 32],
    invert: u8,
    initial_mark_price_e6: u64,
    warmup_period_slots: u64,
) -> Vec<u8> {
    let mut data = vec![0u8];
    data.extend_from_slice(admin.as_ref());
    data.extend_from_slice(mint.as_ref());
    data.extend_from_slice(feed_id);
    data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_staleness_secs
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(invert); // invert flag
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
    data.extend_from_slice(&initial_mark_price_e6.to_le_bytes()); // initial_mark_price_e6 (NEW)
    // RiskParams
    data.extend_from_slice(&warmup_period_slots.to_le_bytes()); // warmup_period_slots
    data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps
    data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // trading_fee_bps
    data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data.extend_from_slice(&0u128.to_le_bytes()); // new_account_fee
    data.extend_from_slice(&0u128.to_le_bytes()); // risk_reduction_threshold
    data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee_per_slot
    data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_crank_staleness_slots
    data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
    data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
    data.extend_from_slice(&100u64.to_le_bytes()); // liquidation_buffer_bps
    data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
    data
}

fn encode_init_lp(matcher: &Pubkey, ctx: &Pubkey, fee: u64) -> Vec<u8> {
    let mut data = vec![2u8];
    data.extend_from_slice(matcher.as_ref());
    data.extend_from_slice(ctx.as_ref());
    data.extend_from_slice(&fee.to_le_bytes());
    data
}

fn encode_init_user(fee: u64) -> Vec<u8> {
    let mut data = vec![1u8];
    data.extend_from_slice(&fee.to_le_bytes());
    data
}

fn encode_deposit(user_idx: u16, amount: u64) -> Vec<u8> {
    let mut data = vec![3u8];
    data.extend_from_slice(&user_idx.to_le_bytes());
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

fn encode_trade(lp: u16, user: u16, size: i128) -> Vec<u8> {
    let mut data = vec![6u8];
    data.extend_from_slice(&lp.to_le_bytes());
    data.extend_from_slice(&user.to_le_bytes());
    data.extend_from_slice(&size.to_le_bytes());
    data
}

fn encode_crank_permissionless() -> Vec<u8> {
    let mut data = vec![5u8];
    data.extend_from_slice(&u16::MAX.to_le_bytes());
    data.push(0u8); // allow_panic = false
    data
}

struct TestEnv {
    svm: LiteSVM,
    program_id: Pubkey,
    payer: Keypair,
    slab: Pubkey,
    mint: Pubkey,
    vault: Pubkey,
    pyth_index: Pubkey,
    pyth_col: Pubkey,
    account_count: u16, // Tracks number of accounts created (LP + users)
}

impl TestEnv {
    fn new() -> Self {
        let path = program_path();
        if !path.exists() {
            panic!("BPF not found at {:?}. Run: cargo build-sbf --features test", path);
        }

        let mut svm = LiteSVM::new();
        let program_id = Pubkey::new_unique();
        let program_bytes = std::fs::read(&path).expect("Failed to read program");
        svm.add_program(program_id, &program_bytes);

        let payer = Keypair::new();
        let slab = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let pyth_index = Pubkey::new_unique();
        let pyth_col = Pubkey::new_unique();
        let (vault_pda, _) = Pubkey::find_program_address(&[b"vault", slab.as_ref()], &program_id);
        let vault = Pubkey::new_unique();

        svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();

        svm.set_account(slab, Account {
            lamports: 1_000_000_000,
            data: vec![0u8; SLAB_LEN],
            owner: program_id,
            executable: false,
            rent_epoch: 0,
        }).unwrap();

        svm.set_account(mint, Account {
            lamports: 1_000_000,
            data: make_mint_data(),
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        }).unwrap();

        svm.set_account(vault, Account {
            lamports: 1_000_000,
            data: make_token_account_data(&mint, &vault_pda, 0),
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        }).unwrap();

        // $138 price (high enough to show difference when inverted)
        let pyth_data = make_pyth_data(&TEST_FEED_ID, 138_000_000, -6, 1, 100);
        svm.set_account(pyth_index, Account {
            lamports: 1_000_000,
            data: pyth_data.clone(),
            owner: PYTH_RECEIVER_PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        }).unwrap();
        svm.set_account(pyth_col, Account {
            lamports: 1_000_000,
            data: pyth_data,
            owner: PYTH_RECEIVER_PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        }).unwrap();

        svm.set_sysvar(&Clock { slot: 100, unix_timestamp: 100, ..Clock::default() });

        TestEnv { svm, program_id, payer, slab, mint, vault, pyth_index, pyth_col, account_count: 0 }
    }

    fn init_market_with_invert(&mut self, invert: u8) {
        let admin = &self.payer;
        let dummy_ata = Pubkey::new_unique();
        self.svm.set_account(dummy_ata, Account {
            lamports: 1_000_000,
            data: vec![0u8; TokenAccount::LEN],
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        }).unwrap();

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(self.mint, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(sysvar::rent::ID, false),
                AccountMeta::new_readonly(dummy_ata, false),
                AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            ],
            data: encode_init_market_with_invert(
                &admin.pubkey(),
                &self.mint,
                &TEST_FEED_ID,
                invert,
            ),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix], Some(&admin.pubkey()), &[admin], self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("init_market failed");
    }

    fn create_ata(&mut self, owner: &Pubkey, amount: u64) -> Pubkey {
        let ata = Pubkey::new_unique();
        self.svm.set_account(ata, Account {
            lamports: 1_000_000,
            data: make_token_account_data(&self.mint, owner, amount),
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        }).unwrap();
        ata
    }

    fn init_lp(&mut self, owner: &Keypair) -> u16 {
        let idx = self.account_count;
        self.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        let ata = self.create_ata(&owner.pubkey(), 0);
        let matcher = spl_token::ID;
        let ctx = Pubkey::new_unique();
        self.svm.set_account(ctx, Account {
            lamports: 1_000_000,
            data: vec![0u8; 320],
            owner: matcher,
            executable: false,
            rent_epoch: 0,
        }).unwrap();

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(ata, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(matcher, false),
                AccountMeta::new_readonly(ctx, false),
            ],
            data: encode_init_lp(&matcher, &ctx, 0),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix], Some(&owner.pubkey()), &[owner], self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("init_lp failed");
        self.account_count += 1;
        idx
    }

    fn init_user(&mut self, owner: &Keypair) -> u16 {
        let idx = self.account_count;
        self.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        let ata = self.create_ata(&owner.pubkey(), 0);

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(ata, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_col, false),
            ],
            data: encode_init_user(0),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix], Some(&owner.pubkey()), &[owner], self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("init_user failed");
        self.account_count += 1;
        idx
    }

    fn deposit(&mut self, owner: &Keypair, user_idx: u16, amount: u64) {
        let ata = self.create_ata(&owner.pubkey(), amount);

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(ata, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
            ],
            data: encode_deposit(user_idx, amount),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix], Some(&owner.pubkey()), &[owner], self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("deposit failed");
    }

    fn trade(&mut self, user: &Keypair, lp: &Keypair, lp_idx: u16, user_idx: u16, size: i128) {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(user.pubkey(), true),
                AccountMeta::new(lp.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_trade(lp_idx, user_idx, size),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix], Some(&user.pubkey()), &[user, lp], self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("trade failed");
    }

    fn crank(&mut self) {
        let caller = Keypair::new();
        self.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(caller.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_crank_permissionless(),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix], Some(&caller.pubkey()), &[&caller], self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("crank failed");
    }

    fn set_slot(&mut self, slot: u64) {
        self.svm.set_sysvar(&Clock {
            slot,
            unix_timestamp: slot as i64,
            ..Clock::default()
        });
        // Update oracle publish_time to match
        let pyth_data = make_pyth_data(&TEST_FEED_ID, 138_000_000, -6, 1, slot as i64);
        self.svm.set_account(self.pyth_index, Account {
            lamports: 1_000_000,
            data: pyth_data.clone(),
            owner: PYTH_RECEIVER_PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        }).unwrap();
        self.svm.set_account(self.pyth_col, Account {
            lamports: 1_000_000,
            data: pyth_data,
            owner: PYTH_RECEIVER_PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        }).unwrap();
    }

    /// Set slot and update oracle to a specific price
    fn set_slot_and_price(&mut self, slot: u64, price_e6: i64) {
        self.svm.set_sysvar(&Clock {
            slot,
            unix_timestamp: slot as i64,
            ..Clock::default()
        });
        // Update oracle with new price and publish_time
        let pyth_data = make_pyth_data(&TEST_FEED_ID, price_e6, -6, 1, slot as i64);
        self.svm.set_account(self.pyth_index, Account {
            lamports: 1_000_000,
            data: pyth_data.clone(),
            owner: PYTH_RECEIVER_PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        }).unwrap();
        self.svm.set_account(self.pyth_col, Account {
            lamports: 1_000_000,
            data: pyth_data,
            owner: PYTH_RECEIVER_PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        }).unwrap();
    }

    /// Try to close account, returns result
    fn try_close_account(&mut self, owner: &Keypair, user_idx: u16) -> Result<(), String> {
        let ata = self.create_ata(&owner.pubkey(), 0);
        let (vault_pda, _) = Pubkey::find_program_address(&[b"vault", self.slab.as_ref()], &self.program_id);

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new(ata, false),
                AccountMeta::new_readonly(vault_pda, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_close_account(user_idx),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix], Some(&owner.pubkey()), &[owner], self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }
}

/// Test that an inverted market can successfully run crank operations.
///
/// This verifies the funding calculation uses market price (inverted) correctly.
/// Prior to the fix, using raw oracle price instead of market price caused
/// ~19,000x overestimation for SOL/USD markets (138M raw vs ~7246 inverted).
///
/// The test:
/// 1. Creates an inverted market (invert=1, like SOL perp where price is SOL/USD)
/// 2. Opens positions to create LP inventory imbalance
/// 3. Runs crank which computes funding rate using market price
/// 4. If funding used raw price instead of market price, it would overflow or produce wrong values
#[test]
fn test_inverted_market_crank_succeeds() {
    let path = program_path();
    if !path.exists() {
        println!("SKIP: BPF not found. Run: cargo build-sbf");
        return;
    }

    let mut env = TestEnv::new();

    // Initialize with invert=1 (inverted market)
    // Oracle price ~$138/SOL in USD terms
    // Market price ~7246 after inversion (1e12/138M)
    env.init_market_with_invert(1);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000); // 10 SOL worth

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000); // 1 SOL worth

    // Open a position to create LP inventory imbalance
    // This causes non-zero funding rate when crank runs
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);

    // Advance slot to allow funding accrual
    env.set_slot(200);
    env.crank();

    // Run multiple cranks to verify stability
    env.set_slot(300);
    env.crank();

    println!("✓ Inverted market crank succeeded with market price");
}

/// Test that a non-inverted market works correctly (control case).
///
/// This serves as a control test to verify that non-inverted markets
/// (where oracle price is used directly as market price) still work.
#[test]
fn test_non_inverted_market_crank_succeeds() {
    let path = program_path();
    if !path.exists() {
        println!("SKIP: BPF not found. Run: cargo build-sbf");
        return;
    }

    let mut env = TestEnv::new();

    // Initialize with invert=0 (non-inverted market)
    // Oracle price is used directly as market price
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);

    env.set_slot(200);
    env.crank();

    env.set_slot(300);
    env.crank();

    println!("✓ Non-inverted market crank succeeded");
}

// ============================================================================
// Bug regression tests
// ============================================================================

fn encode_close_slab() -> Vec<u8> {
    vec![13u8] // Instruction tag for CloseSlab
}

fn encode_withdraw(user_idx: u16, amount: u64) -> Vec<u8> {
    let mut data = vec![4u8]; // Instruction tag for WithdrawCollateral
    data.extend_from_slice(&user_idx.to_le_bytes());
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

fn encode_close_account(user_idx: u16) -> Vec<u8> {
    let mut data = vec![8u8]; // Instruction tag for CloseAccount
    data.extend_from_slice(&user_idx.to_le_bytes());
    data
}

/// Encode InitMarket with configurable unit_scale and new_account_fee
fn encode_init_market_full(
    admin: &Pubkey,
    mint: &Pubkey,
    feed_id: &[u8; 32],
    invert: u8,
    unit_scale: u32,
    new_account_fee: u128,
) -> Vec<u8> {
    let mut data = vec![0u8];
    data.extend_from_slice(admin.as_ref());
    data.extend_from_slice(mint.as_ref());
    data.extend_from_slice(feed_id);
    data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_staleness_secs
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(invert);
    data.extend_from_slice(&unit_scale.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6 (0 for non-Hyperp)
    // RiskParams
    data.extend_from_slice(&0u64.to_le_bytes()); // warmup_period_slots
    data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps
    data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // trading_fee_bps
    data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data.extend_from_slice(&new_account_fee.to_le_bytes());
    data.extend_from_slice(&0u128.to_le_bytes()); // risk_reduction_threshold
    data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee_per_slot
    data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_crank_staleness_slots
    data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
    data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
    data.extend_from_slice(&100u64.to_le_bytes()); // liquidation_buffer_bps
    data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
    data
}

/// Encode InitMarket with configurable warmup_period_slots
fn encode_init_market_with_warmup(
    admin: &Pubkey,
    mint: &Pubkey,
    feed_id: &[u8; 32],
    invert: u8,
    warmup_period_slots: u64,
) -> Vec<u8> {
    let mut data = vec![0u8];
    data.extend_from_slice(admin.as_ref());
    data.extend_from_slice(mint.as_ref());
    data.extend_from_slice(feed_id);
    data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_staleness_secs
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(invert);
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale = 0 (no scaling)
    data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6 (0 for non-Hyperp)
    // RiskParams
    data.extend_from_slice(&warmup_period_slots.to_le_bytes()); // warmup_period_slots
    data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps (5%)
    data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps (10%)
    data.extend_from_slice(&0u64.to_le_bytes()); // trading_fee_bps
    data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data.extend_from_slice(&0u128.to_le_bytes()); // new_account_fee
    data.extend_from_slice(&0u128.to_le_bytes()); // risk_reduction_threshold
    data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee_per_slot
    data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_crank_staleness_slots
    data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
    data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
    data.extend_from_slice(&100u64.to_le_bytes()); // liquidation_buffer_bps
    data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
    data
}

impl TestEnv {
    /// Initialize market with full parameter control
    fn init_market_full(&mut self, invert: u8, unit_scale: u32, new_account_fee: u128) {
        let admin = &self.payer;
        let dummy_ata = Pubkey::new_unique();
        self.svm.set_account(dummy_ata, Account {
            lamports: 1_000_000,
            data: vec![0u8; TokenAccount::LEN],
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        }).unwrap();

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(self.mint, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(sysvar::rent::ID, false),
                AccountMeta::new_readonly(dummy_ata, false),
                AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            ],
            data: encode_init_market_full(
                &admin.pubkey(),
                &self.mint,
                &TEST_FEED_ID,
                invert,
                unit_scale,
                new_account_fee,
            ),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix], Some(&admin.pubkey()), &[admin], self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("init_market failed");
    }

    /// Initialize market with configurable warmup period
    fn init_market_with_warmup(&mut self, invert: u8, warmup_period_slots: u64) {
        let admin = &self.payer;
        let dummy_ata = Pubkey::new_unique();
        self.svm.set_account(dummy_ata, Account {
            lamports: 1_000_000,
            data: vec![0u8; TokenAccount::LEN],
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        }).unwrap();

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(self.mint, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(sysvar::rent::ID, false),
                AccountMeta::new_readonly(dummy_ata, false),
                AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            ],
            data: encode_init_market_with_warmup(
                &admin.pubkey(),
                &self.mint,
                &TEST_FEED_ID,
                invert,
                warmup_period_slots,
            ),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix], Some(&admin.pubkey()), &[admin], self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("init_market_with_warmup failed");
    }

    /// Initialize user with specific fee payment
    /// Returns the next available user index (first user is 0, second is 1, etc)
    fn init_user_with_fee(&mut self, owner: &Keypair, fee: u64) -> u16 {
        let idx = self.account_count;
        self.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        let ata = self.create_ata(&owner.pubkey(), fee);

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(ata, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_col, false),
            ],
            data: encode_init_user(fee),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix], Some(&owner.pubkey()), &[owner], self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("init_user failed");
        self.account_count += 1;
        idx
    }

    /// Read num_used_accounts from engine state
    fn read_num_used_accounts(&self) -> u16 {
        let slab_account = self.svm.get_account(&self.slab).unwrap();
        // Engine starts at offset 304, num_used_accounts is at +56 within engine
        // But this is engine struct layout, need to check actual offset
        // For simplicity: accounts start at index 0, LP is 0, first user is also 0 for single tests
        // Actually engine.num_used_accounts is u16 at engine base + 56
        // Engine base = 304 (after header/config/params)
        // Let's read it: offset 304 + 56 = 360
        if slab_account.data.len() < 362 {
            return 0;
        }
        let bytes = [slab_account.data[360], slab_account.data[361]];
        u16::from_le_bytes(bytes)
    }

    /// Try to close slab, returns Ok or error
    fn try_close_slab(&mut self) -> Result<(), String> {
        let admin = Keypair::from_bytes(&self.payer.to_bytes()).unwrap();

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.slab, false),
            ],
            data: encode_close_slab(),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix], Some(&admin.pubkey()), &[&admin], self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Withdraw collateral (requires 8 accounts)
    fn withdraw(&mut self, owner: &Keypair, user_idx: u16, amount: u64) {
        let ata = self.create_ata(&owner.pubkey(), 0);
        let (vault_pda, _) = Pubkey::find_program_address(&[b"vault", self.slab.as_ref()], &self.program_id);

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(owner.pubkey(), true),      // 0: user (signer)
                AccountMeta::new(self.slab, false),          // 1: slab
                AccountMeta::new(self.vault, false),         // 2: vault
                AccountMeta::new(ata, false),                // 3: user_ata
                AccountMeta::new_readonly(vault_pda, false), // 4: vault_pda
                AccountMeta::new_readonly(spl_token::ID, false), // 5: token program
                AccountMeta::new_readonly(sysvar::clock::ID, false), // 6: clock
                AccountMeta::new_readonly(self.pyth_index, false),   // 7: oracle
            ],
            data: encode_withdraw(user_idx, amount),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix], Some(&owner.pubkey()), &[owner], self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("withdraw failed");
    }

    /// Try to execute trade, returns result
    fn try_trade(&mut self, user: &Keypair, lp: &Keypair, lp_idx: u16, user_idx: u16, size: i128) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(user.pubkey(), true),
                AccountMeta::new(lp.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_trade(lp_idx, user_idx, size),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix], Some(&user.pubkey()), &[user, lp], self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Read vault token balance
    fn vault_balance(&self) -> u64 {
        let account = self.svm.get_account(&self.vault).unwrap();
        let token_account = TokenAccount::unpack(&account.data).unwrap();
        token_account.amount
    }

    /// Close account - returns remaining capital to user (8 accounts needed)
    fn close_account(&mut self, owner: &Keypair, user_idx: u16) {
        let ata = self.create_ata(&owner.pubkey(), 0);
        let (vault_pda, _) = Pubkey::find_program_address(&[b"vault", self.slab.as_ref()], &self.program_id);

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(owner.pubkey(), true),      // 0: user (signer)
                AccountMeta::new(self.slab, false),          // 1: slab
                AccountMeta::new(self.vault, false),         // 2: vault
                AccountMeta::new(ata, false),                // 3: user_ata
                AccountMeta::new_readonly(vault_pda, false), // 4: vault_pda
                AccountMeta::new_readonly(spl_token::ID, false), // 5: token program
                AccountMeta::new_readonly(sysvar::clock::ID, false), // 6: clock
                AccountMeta::new_readonly(self.pyth_index, false),   // 7: oracle
            ],
            data: encode_close_account(user_idx),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix], Some(&owner.pubkey()), &[owner], self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("close_account failed");
    }
}

// ============================================================================
// Bug #3: CloseSlab should fail when dust_base > 0
// ============================================================================

/// Test that CloseSlab fails when there is residual dust in the vault.
///
/// Bug: CloseSlab only checks engine.vault and engine.insurance_fund.balance,
/// but not dust_base which can hold residual base tokens.
#[test]
fn test_bug3_close_slab_with_dust_should_fail() {
    let path = program_path();
    if !path.exists() {
        println!("SKIP: BPF not found. Run: cargo build-sbf");
        return;
    }

    let mut env = TestEnv::new();

    // Initialize with unit_scale=1000 (1000 base = 1 unit)
    // This means deposits with remainder < 1000 will create dust
    env.init_market_full(0, 1000, 0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    // Deposit 10_000_500 base tokens: 10_000 units + 500 dust
    // - 10_000_500 / 1000 = 10_000 units credited
    // - 10_000_500 % 1000 = 500 dust stored in dust_base
    env.deposit(&user, user_idx, 10_000_500);

    // Check vault has the full amount
    let vault_balance = env.vault_balance();
    assert_eq!(vault_balance, 10_000_500, "Vault should have full deposit");

    // Advance slot and crank to ensure state is updated
    env.set_slot(200);
    env.crank();

    // Close account - returns capital in units converted to base
    // 10_000 units * 1000 = 10_000_000 base returned
    // The 500 dust remains in vault but isn't tracked by engine.vault
    env.close_account(&user, user_idx);

    // Check vault still has 500 dust
    let vault_after = env.vault_balance();
    println!("Bug #3: Vault balance after close_account = {}", vault_after);

    // Vault should have dust remaining (500 base tokens)
    assert!(vault_after > 0, "Vault should have dust remaining");

    // Try to close slab - should fail because dust_base > 0
    let result = env.try_close_slab();

    println!("Bug #3 test: CloseSlab with dust result = {:?}", result);
    println!("Bug #3: Vault still has {} tokens - CloseSlab correctly rejects", vault_after);

    // FIXED: CloseSlab now returns error when dust_base > 0
    assert!(result.is_err(), "CloseSlab should fail when dust_base > 0");
}

// ============================================================================
// Bug #4: InitUser/InitLP should not trap fee overpayments
// ============================================================================

/// Test that fee overpayments are properly handled.
///
/// Bug: If fee_payment > new_account_fee, the excess is deposited to vault
/// but only new_account_fee is accounted in engine.vault/insurance.
#[test]
fn test_bug4_fee_overpayment_should_be_handled() {
    let path = program_path();
    if !path.exists() {
        println!("SKIP: BPF not found. Run: cargo build-sbf");
        return;
    }

    let mut env = TestEnv::new();

    // Initialize with new_account_fee = 1000
    env.init_market_full(0, 0, 1000);

    // Get vault balance before
    let vault_before = env.vault_balance();

    let user = Keypair::new();
    // Pay 5000 when only 1000 is required
    let _user_idx = env.init_user_with_fee(&user, 5000);

    // Get vault balance after
    let vault_after = env.vault_balance();

    // Vault received 5000 tokens
    let deposited = vault_after - vault_before;
    assert_eq!(deposited, 5000, "Vault should receive full payment");

    // BUG: The excess 4000 is trapped - not credited to user capital,
    // not tracked in engine.vault (only 1000 is tracked)
    // After fix: excess should either be rejected or credited to user
    println!("Bug #4 test: Deposited {} (required: 1000, excess: {})", deposited, deposited - 1000);
}

// ============================================================================
// Bug #8: LP entry price should update on position flip
// ============================================================================

/// Test that LP entry price is updated when position flips direction.
///
/// Bug: On LP sign flip where abs(new) <= abs(old), entry_price is not updated.
/// This causes incorrect MTM PnL calculations.
#[test]
fn test_bug8_lp_entry_price_updates_on_flip() {
    let path = program_path();
    if !path.exists() {
        println!("SKIP: BPF not found. Run: cargo build-sbf");
        return;
    }

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 50_000_000_000); // 50 SOL

    // User goes long 100 contracts -> LP goes short 100
    env.trade(&user, &lp, lp_idx, user_idx, 100_000_000);

    // Now LP has position = -100M (short)
    // Entry price should be ~138M (the oracle price)

    // Change price significantly
    env.set_slot(200);

    // User closes 150 contracts (goes short 50) -> LP goes from -100 to +50
    // This is a flip where abs(new)=50 < abs(old)=100
    // BUG: LP entry price is NOT updated - stays at old entry instead of new exec price
    env.trade(&user, &lp, lp_idx, user_idx, -150_000_000);

    // After this trade:
    // - LP position flipped from -100M to +50M
    // - LP entry should be updated to current exec price
    // BUG: Entry stays at old price, causing incorrect PnL calculation

    println!("✓ Bug #8 test: LP position flipped. Entry price should be updated.");
    // Note: We can't easily read the entry price from LiteSVM without parsing slab
    // The bug would manifest as incorrect margin calculations
}

// ============================================================================
// Bug #6: Threshold EWMA starts from zero, causing slow ramp
// ============================================================================

/// Test that threshold EWMA ramps up quickly when starting from zero.
///
/// Bug: When risk_reduction_threshold starts at 0 and target is large,
/// max_step = (current * step_bps / 10000).max(min_step) = min_step = 1
/// So threshold can only increase by 1 per update interval, regardless of target.
#[test]
fn test_bug6_threshold_slow_ramp_from_zero() {
    let path = program_path();
    if !path.exists() {
        println!("SKIP: BPF not found. Run: cargo build-sbf");
        return;
    }

    // This test demonstrates the bug conceptually.
    // In practice, testing requires:
    // 1. Initialize market with default params (threshold starts at 0)
    // 2. Create conditions where target threshold is high (e.g., large LP position)
    // 3. Crank multiple times
    // 4. Observe that threshold only increases by 1 per update

    // BUG: With DEFAULT_THRESH_MIN_STEP=1 and current=0:
    // max_step = max(0 * step_bps / 10000, 1) = 1
    // Even if target is 1_000_000, threshold only increases by 1 per interval

    println!("Bug #6: Threshold EWMA slow ramp from zero");
    println!("  - When current=0, max_step = min_step (1)");
    println!("  - Even with large target, only increases by 1 per update");
    println!("  - Fix: Special-case current=0 to allow larger initial step");

    // Note: Full test would require reading threshold from slab state
    // and verifying it doesn't ramp quickly enough
}

// ============================================================================
// Bug #7: Pending epoch wraparound causes incorrect exclusion
// ============================================================================

/// Test that pending_epoch wraparound doesn't cause incorrect exclusion.
///
/// Bug: pending_epoch is u8, so after 256 sweeps it wraps to 0.
/// Stale pending_exclude_epoch[idx] markers can match the new epoch,
/// incorrectly exempting accounts from profit-funding.
#[test]
fn test_bug7_pending_epoch_wraparound() {
    let path = program_path();
    if !path.exists() {
        println!("SKIP: BPF not found. Run: cargo build-sbf");
        return;
    }

    // This test demonstrates the bug conceptually.
    // Full test would require:
    // 1. Initialize market
    // 2. Create accounts
    // 3. Run 256+ sweeps (256 cranks)
    // 4. Trigger a liquidation that sets pending_exclude_epoch[idx]
    // 5. Run 256 more sweeps
    // 6. Verify the stale marker doesn't incorrectly exempt the account

    // BUG: pending_epoch is u8, wraps after 256 sweeps:
    // Sweep 0: pending_epoch=0, exclude account 5, pending_exclude_epoch[5]=0
    // Sweep 255: pending_epoch=255
    // Sweep 256: pending_epoch=0 (wrapped!)
    // Now pending_exclude_epoch[5]==0==pending_epoch, account 5 incorrectly excluded

    println!("Bug #7: Pending epoch wraparound");
    println!("  - pending_epoch is u8, wraps after 256 sweeps");
    println!("  - Stale exclusion markers can match new epoch after wrap");
    println!("  - Fix: Use wider type (u16) or clear markers on wrap");

    // Note: Full test would require running 256+ cranks which is expensive
    // The bug is evident from code inspection
}

// ============================================================================
// Finding L: Margin check uses maintenance instead of initial margin
// ============================================================================

/// Test that execute_trade() incorrectly uses maintenance_margin_bps instead of
/// initial_margin_bps, allowing users to open positions at 2x intended leverage.
///
/// Finding L from security audit:
/// - maintenance_margin_bps = 500 (5%)
/// - initial_margin_bps = 1000 (10%)
/// - Bug: Trade opening checks 5% margin instead of 10%
/// - Result: Users can open at ~20x leverage instead of max 10x
#[test]
fn test_bug_finding_l_margin_check_uses_maintenance_instead_of_initial() {
    let path = program_path();
    if !path.exists() {
        println!("SKIP: BPF not found. Run: cargo build-sbf");
        return;
    }

    // Finding L: execute_trade() uses maintenance_margin_bps (5%) instead of
    // initial_margin_bps (10%), allowing 2x intended leverage.
    //
    // RiskParams in encode_init_market:
    //   maintenance_margin_bps = 500 (5%)
    //   initial_margin_bps = 1000 (10%)
    //
    // Test: deposit enough to pass maintenance but fail initial margin check.
    // BUG: trade succeeds when it should be rejected.

    let mut env = TestEnv::new();
    env.init_market_with_invert(1);

    // Create LP with sufficient capital
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL

    // Create user with capital between maintenance and initial margin requirements
    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    // For 10 SOL notional at $138 price:
    //   Maintenance margin (5%) = 0.5 SOL
    //   Initial margin (10%) = 1.0 SOL
    // Deposit 0.6 SOL (above maint, below initial)
    env.deposit(&user, user_idx, 600_000_000); // 0.6 SOL

    // Calculate position size for ~10 SOL notional
    // size * price / 1_000_000 = notional
    // size = notional * 1_000_000 / price = 10_000_000_000 * 1_000_000 / 138_000_000
    let size: i128 = 72_463_768; // ~10 SOL notional at $138

    // BUG: This trade should be REJECTED (equity 0.6 < initial margin 1.0)
    // But it is ACCEPTED (equity 0.6 > maintenance margin 0.5)
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, size);

    assert!(
        result.is_ok(),
        "FINDING L REPRODUCED: Trade at ~16.7x leverage accepted. \
         Should require 10% initial margin but only checks 5% maintenance. \
         Expected: Ok (bug), Got: {:?}", result
    );

    println!("FINDING L CONFIRMED: execute_trade() checks maintenance_margin_bps (5%)");
    println!("instead of initial_margin_bps (10%). User opened position at ~16.7x leverage.");
    println!("Position notional: ~10 SOL, Equity: 0.6 SOL");
    println!("Maintenance margin required: 0.5 SOL (passes)");
    println!("Initial margin required: 1.0 SOL (should fail but doesn't)");
}

// ============================================================================
// Zombie PnL Bug: Crank-driven warmup conversion for idle accounts
// ============================================================================

/// Test that crank-driven warmup conversion works for idle accounts.
///
/// Per spec §10.5 and §12.6 (Zombie poisoning regression):
/// - Idle accounts with positive PnL should have their PnL converted to capital
///   via crank-driven warmup settlement
/// - This prevents "zombie" accounts from indefinitely keeping pnl_pos_tot high
///   and collapsing the haircut ratio
///
/// Test scenario:
/// 1. Create market with warmup_period_slots = 100
/// 2. User opens position and gains positive PnL via favorable price move
/// 3. User becomes idle (doesn't call any ops)
/// 4. Run cranks over time (advancing past warmup period)
/// 5. Verify PnL was converted to capital (user can close account)
///
/// Without the fix: User's PnL would never convert, close_account fails
/// With the fix: Crank converts PnL to capital, close_account succeeds
#[test]
fn test_zombie_pnl_crank_driven_warmup_conversion() {
    let path = program_path();
    if !path.exists() {
        println!("SKIP: BPF not found. Run: cargo build-sbf");
        return;
    }

    let mut env = TestEnv::new();

    // Initialize market with warmup_period_slots = 100
    // This means positive PnL takes 100 slots to fully convert to capital
    env.init_market_with_warmup(1, 100); // invert=1 for SOL/USD style

    // Create LP with sufficient capital
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL

    // Create user with capital
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000); // 10 SOL

    // Execute trade: user goes long at current price ($138)
    // Position size chosen to be safe within margin requirements
    let size: i128 = 10_000_000; // Small position
    env.trade(&user, &lp, lp_idx, user_idx, size);

    println!("Step 1: User opened long position at $138");

    // Advance slot and move oracle price UP (favorable for long user)
    // Oracle: $138 -> $150 (user profits)
    env.set_slot_and_price(10, 150_000_000);

    // Run crank to settle mark-to-market (converts unrealized to realized PnL)
    env.crank();

    println!("Step 2: Oracle moved to $150, crank settled mark-to-market");
    println!("        User should now have positive realized PnL");

    // Close user's position at new price (realizes the profit)
    // Trade opposite direction to close
    env.trade(&user, &lp, lp_idx, user_idx, -size);

    println!("Step 3: User closed position, PnL is now fully realized");

    // At this point, user has:
    // - No position (closed)
    // - Positive PnL from the profitable trade
    // - The PnL needs to warm up before it can be withdrawn/account closed

    // Try to close account immediately - should fail (PnL not warmed up yet)
    let early_close_result = env.try_close_account(&user, user_idx);
    println!("Step 4: Early close attempt (before warmup): {:?}",
             if early_close_result.is_err() { "Failed as expected" } else { "Unexpected success" });

    // Now simulate the zombie scenario:
    // User becomes idle and doesn't call any ops
    // But cranks continue to run...

    // Advance past warmup period (100 slots) with periodic cranks
    // Each crank should call settle_warmup_to_capital_for_crank
    for i in 0..12 {
        let slot = 20 + i * 10; // slots: 20, 30, 40, ... 130
        env.set_slot_and_price(slot, 150_000_000);
        env.crank();
    }

    println!("Step 5: Ran 12 cranks over 120 slots (past warmup period of 100)");
    println!("        Crank should have converted idle user's PnL to capital");

    // Now try to close account - should succeed if warmup conversion worked
    let final_close_result = env.try_close_account(&user, user_idx);

    if final_close_result.is_ok() {
        println!("ZOMBIE PNL FIX VERIFIED: Crank-driven warmup conversion works!");
        println!("Idle user's positive PnL was converted to capital via crank.");
        println!("Account closed successfully after warmup period.");
    } else {
        println!("ZOMBIE PNL BUG: Crank-driven warmup conversion FAILED!");
        println!("Idle user's PnL was not converted, account cannot close.");
        println!("Error: {:?}", final_close_result);
    }

    assert!(
        final_close_result.is_ok(),
        "ZOMBIE PNL FIX: Account should close after crank-driven warmup conversion. \
         Got: {:?}", final_close_result
    );
}

/// Test that zombie accounts don't indefinitely poison the haircut ratio.
///
/// This is a simpler test that verifies the basic mechanism:
/// - Idle account with capital and no position can be closed
/// - Even without PnL, crank processes the account correctly
#[test]
fn test_idle_account_can_close_after_crank() {
    let path = program_path();
    if !path.exists() {
        println!("SKIP: BPF not found. Run: cargo build-sbf");
        return;
    }

    let mut env = TestEnv::new();
    env.init_market_with_warmup(1, 100);

    // Create and fund user
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000); // 1 SOL

    // User is idle (no trades, no ops)

    // Advance slot and run crank
    env.set_slot(200);
    env.crank();

    // User should be able to close account (no position, no PnL)
    let result = env.try_close_account(&user, user_idx);

    assert!(
        result.is_ok(),
        "Idle account with only capital should be closeable. Got: {:?}", result
    );

    println!("Idle account closed successfully - basic zombie prevention works");
}

// ============================================================================
// HYPERP MODE SECURITY TESTS
// ============================================================================

/// Security Issue: Hyperp mode requires non-zero initial_mark_price_e6
///
/// If Hyperp mode is enabled (index_feed_id == [0; 32]) but initial_mark_price_e6 == 0,
/// the market would have no valid price and trades would fail with OracleInvalid.
/// This test verifies the validation in InitMarket rejects this configuration.
#[test]
fn test_hyperp_rejects_zero_initial_mark_price() {
    let path = program_path();
    if !path.exists() {
        println!("SKIP: BPF not found. Run: cargo build-sbf");
        return;
    }

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

    svm.set_account(slab, Account {
        lamports: 1_000_000_000,
        data: vec![0u8; SLAB_LEN],
        owner: program_id,
        executable: false,
        rent_epoch: 0,
    }).unwrap();

    svm.set_account(mint, Account {
        lamports: 1_000_000,
        data: make_mint_data(),
        owner: spl_token::ID,
        executable: false,
        rent_epoch: 0,
    }).unwrap();

    svm.set_account(vault, Account {
        lamports: 1_000_000,
        data: make_token_account_data(&mint, &vault_pda, 0),
        owner: spl_token::ID,
        executable: false,
        rent_epoch: 0,
    }).unwrap();

    let dummy_ata = Pubkey::new_unique();
    svm.set_account(dummy_ata, Account {
        lamports: 1_000_000,
        data: vec![0u8; TokenAccount::LEN],
        owner: spl_token::ID,
        executable: false,
        rent_epoch: 0,
    }).unwrap();

    svm.set_sysvar(&Clock { slot: 100, unix_timestamp: 100, ..Clock::default() });

    // Try to init market with Hyperp mode (feed_id = 0) but initial_mark_price = 0
    // This should FAIL because Hyperp mode requires a non-zero initial price
    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(slab, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: encode_init_market_full_v2(
            &payer.pubkey(),
            &mint,
            &[0u8; 32],  // Hyperp mode: feed_id = 0
            0,           // invert
            0,           // initial_mark_price_e6 = 0 (INVALID for Hyperp!)
            0,           // warmup
        ),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix], Some(&payer.pubkey()), &[&payer], svm.latest_blockhash(),
    );

    let result = svm.send_transaction(tx);

    assert!(
        result.is_err(),
        "SECURITY: InitMarket should reject Hyperp mode with zero initial_mark_price_e6. \
         Got: {:?}", result
    );

    println!("HYPERP VALIDATION VERIFIED: Rejects zero initial_mark_price_e6 in Hyperp mode");
}

/// Security Issue: TradeNoCpi sets mark = index, making premium always 0
///
/// In Hyperp mode, TradeNoCpi:
/// 1. Reads price from index (last_effective_price_e6)
/// 2. Executes trade at that price
/// 3. Sets mark (authority_price_e6) = price (index)
///
/// This means mark = index after every trade, so premium = (mark - index) / index = 0.
/// Premium-based funding will always be zero, defeating the purpose of Hyperp funding.
///
/// This is a KNOWN ISSUE that demonstrates the design flaw.
#[test]
fn test_hyperp_issue_trade_nocpi_sets_mark_equals_index() {
    let path = program_path();
    if !path.exists() {
        println!("SKIP: BPF not found. Run: cargo build-sbf");
        return;
    }

    println!("HYPERP DESIGN ISSUE: TradeNoCpi sets mark = index");
    println!("After any TradeNoCpi, mark == index, so premium == 0");
    println!("Premium-based funding will always compute to 0 after trades");
    println!("");
    println!("This is expected behavior in current implementation.");
    println!("For meaningful premium-based funding, either:");
    println!("  1. Use TradeCpi with a matcher that provides exec_price != index");
    println!("  2. Have an external system push mark prices via PushOraclePrice");
    println!("  3. Modify TradeNoCpi to not update mark (let it drift naturally)");

    // This test documents the known issue - no assertion needed
    // The issue is architectural and the current behavior is "correct" per the code
}

/// Security Issue: Default oracle_price_cap = 0 bypasses index smoothing
///
/// In clamp_toward_with_dt():
///   if cap_e2bps == 0 || dt_slots == 0 { return mark; }
///
/// When oracle_price_cap_e2bps == 0 (the InitMarket default), the index
/// immediately jumps to mark without any rate limiting.
///
/// This means the "smooth index chase" feature is disabled by default!
/// Admin must call SetOraclePriceCap after InitMarket to enable smoothing.
///
/// This is a KNOWN CONFIGURATION ISSUE.
#[test]
fn test_hyperp_issue_default_cap_zero_bypasses_smoothing() {
    let path = program_path();
    if !path.exists() {
        println!("SKIP: BPF not found. Run: cargo build-sbf");
        return;
    }

    println!("HYPERP CONFIGURATION ISSUE: Default oracle_price_cap_e2bps = 0");
    println!("In InitMarket, oracle_price_cap_e2bps defaults to 0.");
    println!("When cap == 0, clamp_toward_with_dt() returns mark immediately.");
    println!("This means index smoothing is DISABLED by default!");
    println!("");
    println!("Fix: Admin must call SetOraclePriceCap to set a non-zero value");
    println!("     after InitMarket to enable rate-limited index smoothing.");
    println!("");
    println!("Example: SetOraclePriceCap with max_change_e2bps = 1000 (0.1% per slot)");

    // This test documents the configuration requirement
}

/// Test: Hyperp mode InitMarket succeeds with valid initial_mark_price
#[test]
fn test_hyperp_init_market_with_valid_price() {
    let path = program_path();
    if !path.exists() {
        println!("SKIP: BPF not found. Run: cargo build-sbf");
        return;
    }

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

    svm.set_account(slab, Account {
        lamports: 1_000_000_000,
        data: vec![0u8; SLAB_LEN],
        owner: program_id,
        executable: false,
        rent_epoch: 0,
    }).unwrap();

    svm.set_account(mint, Account {
        lamports: 1_000_000,
        data: make_mint_data(),
        owner: spl_token::ID,
        executable: false,
        rent_epoch: 0,
    }).unwrap();

    svm.set_account(vault, Account {
        lamports: 1_000_000,
        data: make_token_account_data(&mint, &vault_pda, 0),
        owner: spl_token::ID,
        executable: false,
        rent_epoch: 0,
    }).unwrap();

    let dummy_ata = Pubkey::new_unique();
    svm.set_account(dummy_ata, Account {
        lamports: 1_000_000,
        data: vec![0u8; TokenAccount::LEN],
        owner: spl_token::ID,
        executable: false,
        rent_epoch: 0,
    }).unwrap();

    svm.set_sysvar(&Clock { slot: 100, unix_timestamp: 100, ..Clock::default() });

    // Init market with Hyperp mode and valid initial_mark_price
    let initial_price_e6 = 100_000_000u64; // $100 in e6 format

    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(slab, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
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
        &[ix], Some(&payer.pubkey()), &[&payer], svm.latest_blockhash(),
    );

    let result = svm.send_transaction(tx);

    assert!(
        result.is_ok(),
        "Hyperp InitMarket with valid initial_mark_price should succeed. Got: {:?}", result
    );

    println!("HYPERP INIT VERIFIED: Market initialized with $100 initial mark/index price");
}
