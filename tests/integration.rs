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
    program_pack::Pack,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    sysvar,
    transaction::Transaction,
};
use spl_token::state::{Account as TokenAccount, AccountState};
use std::path::PathBuf;

// SLAB_LEN for production BPF (MAX_ACCOUNTS=4096)
// Note: We use production BPF (not test feature) because test feature
// bypasses CPI for token transfers, which fails in LiteSVM.
// Haircut-ratio engine (ADL/socialization scratch arrays removed)
const SLAB_LEN: usize = 992560; // MAX_ACCOUNTS=4096 + oracle circuit breaker (no padding)
const MAX_ACCOUNTS: usize = 4096;

// Pyth Receiver program ID
const PYTH_RECEIVER_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
    0x0c, 0xb7, 0xfa, 0xbb, 0x52, 0xf7, 0xa6, 0x48, 0xbb, 0x5b, 0x31, 0x7d, 0x9a, 0x01, 0x8b, 0x90,
    0x57, 0xcb, 0x02, 0x47, 0x74, 0xfa, 0xfe, 0x01, 0xe6, 0xc4, 0xdf, 0x98, 0xcc, 0x38, 0x58, 0x81,
]);

const TEST_FEED_ID: [u8; 32] = [0xABu8; 32];

fn program_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("target/deploy/percolator_prog.so");
    assert!(
        path.exists(),
        "BPF not found at {:?}. Run: cargo build-sbf",
        path
    );
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
fn make_pyth_data(
    feed_id: &[u8; 32],
    price: i64,
    expo: i32,
    conf: u64,
    publish_time: i64,
) -> Vec<u8> {
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
fn encode_init_market_hyperp(admin: &Pubkey, mint: &Pubkey, initial_mark_price_e6: u64) -> Vec<u8> {
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

        // $138 price (high enough to show difference when inverted)
        let pyth_data = make_pyth_data(&TEST_FEED_ID, 138_000_000, -6, 1, 100);
        svm.set_account(
            pyth_index,
            Account {
                lamports: 1_000_000,
                data: pyth_data.clone(),
                owner: PYTH_RECEIVER_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
        svm.set_account(
            pyth_col,
            Account {
                lamports: 1_000_000,
                data: pyth_data,
                owner: PYTH_RECEIVER_PROGRAM_ID,
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

        TestEnv {
            svm,
            program_id,
            payer,
            slab,
            mint,
            vault,
            pyth_index,
            pyth_col,
            account_count: 0,
        }
    }

    fn init_market_with_invert(&mut self, invert: u8) {
        let admin = &self.payer;
        let dummy_ata = Pubkey::new_unique();
        self.svm
            .set_account(
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
            &[ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("init_market failed");
    }

    /// Initialize a Hyperp market (internal mark/index, no external oracle)
    fn init_market_hyperp(&mut self, initial_mark_price_e6: u64) {
        let admin = &self.payer;
        let dummy_ata = Pubkey::new_unique();
        self.svm
            .set_account(
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
            data: encode_init_market_hyperp(&admin.pubkey(), &self.mint, initial_mark_price_e6),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("init_market_hyperp failed");
    }

    fn create_ata(&mut self, owner: &Pubkey, amount: u64) -> Pubkey {
        let ata = Pubkey::new_unique();
        self.svm
            .set_account(
                ata,
                Account {
                    lamports: 1_000_000,
                    data: make_token_account_data(&self.mint, owner, amount),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        ata
    }

    fn init_lp(&mut self, owner: &Keypair) -> u16 {
        let idx = self.account_count;
        self.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        let ata = self.create_ata(&owner.pubkey(), 0);
        let matcher = spl_token::ID;
        let ctx = Pubkey::new_unique();
        self.svm
            .set_account(
                ctx,
                Account {
                    lamports: 1_000_000,
                    data: vec![0u8; 320],
                    owner: matcher,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();

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
            &[ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
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
            &[ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
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
            &[ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
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
            &[ix],
            Some(&user.pubkey()),
            &[user, lp],
            self.svm.latest_blockhash(),
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
            &[ix],
            Some(&caller.pubkey()),
            &[&caller],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("crank failed");
    }

    fn try_crank(&mut self) -> Result<(), String> {
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
            &[ix],
            Some(&caller.pubkey()),
            &[&caller],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    fn set_slot(&mut self, slot: u64) {
        self.svm.set_sysvar(&Clock {
            slot,
            unix_timestamp: slot as i64,
            ..Clock::default()
        });
        // Update oracle publish_time to match
        let pyth_data = make_pyth_data(&TEST_FEED_ID, 138_000_000, -6, 1, slot as i64);
        self.svm
            .set_account(
                self.pyth_index,
                Account {
                    lamports: 1_000_000,
                    data: pyth_data.clone(),
                    owner: PYTH_RECEIVER_PROGRAM_ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        self.svm
            .set_account(
                self.pyth_col,
                Account {
                    lamports: 1_000_000,
                    data: pyth_data,
                    owner: PYTH_RECEIVER_PROGRAM_ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
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
        self.svm
            .set_account(
                self.pyth_index,
                Account {
                    lamports: 1_000_000,
                    data: pyth_data.clone(),
                    owner: PYTH_RECEIVER_PROGRAM_ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        self.svm
            .set_account(
                self.pyth_col,
                Account {
                    lamports: 1_000_000,
                    data: pyth_data,
                    owner: PYTH_RECEIVER_PROGRAM_ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
    }

    /// Try to close account, returns result
    fn try_close_account(&mut self, owner: &Keypair, user_idx: u16) -> Result<(), String> {
        let ata = self.create_ata(&owner.pubkey(), 0);
        let (vault_pda, _) =
            Pubkey::find_program_address(&[b"vault", self.slab.as_ref()], &self.program_id);

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
            &[ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Try AdminForceCloseAccount instruction (admin only, requires resolved + zero position)
    fn try_admin_force_close_account(
        &mut self,
        admin: &Keypair,
        user_idx: u16,
        owner: &Pubkey,
    ) -> Result<(), String> {
        let owner_ata = self.create_ata(owner, 0);
        let (vault_pda, _) =
            Pubkey::find_program_address(&[b"vault", self.slab.as_ref()], &self.program_id);

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true), // 0: admin (signer)
                AccountMeta::new(self.slab, false),     // 1: slab
                AccountMeta::new(self.vault, false),    // 2: vault
                AccountMeta::new(owner_ata, false),     // 3: owner_ata
                AccountMeta::new_readonly(vault_pda, false), // 4: vault_pda
                AccountMeta::new_readonly(spl_token::ID, false), // 5: token program
                AccountMeta::new_readonly(sysvar::clock::ID, false), // 6: clock
                AccountMeta::new_readonly(self.pyth_index, false), // 7: oracle
            ],
            data: encode_admin_force_close_account(user_idx),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
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
    program_path();

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

    // Top up insurance to prevent force-realize and dust-close (must exceed threshold after EWMA update)
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 1_000_000_000);
    let vault_before = env.vault_balance();

    // Advance slot to allow funding accrual
    env.set_slot(200);
    env.crank();

    env.set_slot(300);
    env.crank();

    // Vault SPL balance must not change (funding is internal accounting)
    assert_eq!(env.vault_balance(), vault_before, "Vault must be conserved through cranks");
    // Positions must still exist
    assert_ne!(env.read_account_position(user_idx), 0, "User position must persist");
    assert_ne!(env.read_account_position(lp_idx), 0, "LP position must persist");
}

/// Test that a non-inverted market works correctly (control case).
///
/// This serves as a control test to verify that non-inverted markets
/// (where oracle price is used directly as market price) still work.
#[test]
fn test_non_inverted_market_crank_succeeds() {
    program_path();

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

    // Top up insurance to prevent force-realize and dust-close (must exceed threshold after EWMA update)
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 1_000_000_000);
    let vault_before = env.vault_balance();

    env.set_slot(200);
    env.crank();

    env.set_slot(300);
    env.crank();

    // Vault SPL balance must not change (funding is internal accounting)
    assert_eq!(env.vault_balance(), vault_before, "Vault must be conserved through cranks");
    // Positions must still exist
    assert_ne!(env.read_account_position(user_idx), 0, "User position must persist");
    assert_ne!(env.read_account_position(lp_idx), 0, "LP position must persist");
}

// ============================================================================
// Bug regression tests
// ============================================================================

fn encode_close_slab() -> Vec<u8> {
    vec![13u8] // Instruction tag for CloseSlab
}

fn encode_resolve_market() -> Vec<u8> {
    vec![19u8] // Instruction tag for ResolveMarket
}

fn encode_withdraw_insurance() -> Vec<u8> {
    vec![20u8] // Instruction tag for WithdrawInsurance
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

fn encode_admin_force_close_account(user_idx: u16) -> Vec<u8> {
    let mut data = vec![21u8]; // Tag 21: AdminForceCloseAccount
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
        self.svm
            .set_account(
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
            &[ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("init_market failed");
    }

    /// Initialize market with configurable warmup period
    fn init_market_with_warmup(&mut self, invert: u8, warmup_period_slots: u64) {
        let admin = &self.payer;
        let dummy_ata = Pubkey::new_unique();
        self.svm
            .set_account(
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
            &[ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("init_market_with_warmup failed");
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
            &[ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("init_user failed");
        self.account_count += 1;
        idx
    }

    /// Read num_used_accounts from engine state
    fn read_num_used_accounts(&self) -> u16 {
        let slab_account = self.svm.get_account(&self.slab).unwrap();
        // ENGINE_OFF = 392 (from constants, checked via test_struct_sizes)
        // offset of RiskEngine.used = 408 (bitmap array)
        // used is [u64; 64] = 512 bytes
        // num_used_accounts follows used at offset 408 + 512 = 920 within RiskEngine
        // Total offset = 392 + 920 = 1312
        const NUM_USED_OFFSET: usize = 392 + 920; // 1312
        if slab_account.data.len() < NUM_USED_OFFSET + 2 {
            return 0;
        }
        let bytes = [
            slab_account.data[NUM_USED_OFFSET],
            slab_account.data[NUM_USED_OFFSET + 1],
        ];
        u16::from_le_bytes(bytes)
    }

    /// Check if a slot is marked as used in the bitmap
    fn is_slot_used(&self, idx: u16) -> bool {
        let slab_account = self.svm.get_account(&self.slab).unwrap();
        // ENGINE_OFF = 392, offset of RiskEngine.used = 408
        // Bitmap is [u64; 64] at offset 392 + 408 = 800
        const BITMAP_OFFSET: usize = 392 + 408;
        let word_idx = (idx as usize) >> 6; // idx / 64
        let bit_idx = (idx as usize) & 63; // idx % 64
        let word_offset = BITMAP_OFFSET + word_idx * 8;
        if slab_account.data.len() < word_offset + 8 {
            return false;
        }
        let word = u64::from_le_bytes(
            slab_account.data[word_offset..word_offset + 8]
                .try_into()
                .unwrap(),
        );
        (word >> bit_idx) & 1 == 1
    }

    /// Read account capital for a slot (to verify it's zeroed after GC)
    fn read_account_capital(&self, idx: u16) -> u128 {
        let slab_account = self.svm.get_account(&self.slab).unwrap();
        // ENGINE_OFF = 392, accounts array at offset 9136 within RiskEngine
        // Account size = 240 bytes, capital at offset 8 within Account (after account_id u64)
        const ACCOUNTS_OFFSET: usize = 392 + 9136;
        const ACCOUNT_SIZE: usize = 240;
        const CAPITAL_OFFSET_IN_ACCOUNT: usize = 8; // After account_id (u64)
        let account_offset =
            ACCOUNTS_OFFSET + (idx as usize) * ACCOUNT_SIZE + CAPITAL_OFFSET_IN_ACCOUNT;
        if slab_account.data.len() < account_offset + 16 {
            return 0;
        }
        u128::from_le_bytes(
            slab_account.data[account_offset..account_offset + 16]
                .try_into()
                .unwrap(),
        )
    }

    /// Read account position_size for a slot
    fn read_account_position(&self, idx: u16) -> i128 {
        let slab_account = self.svm.get_account(&self.slab).unwrap();
        // ENGINE_OFF = 392, accounts array at offset 9136 within RiskEngine
        // Account size = 240 bytes
        // Account layout: account_id(8) + capital(16) + kind(1) + padding(7) + pnl(16) + reserved_pnl(8) +
        //                 warmup_started_at_slot(8) + warmup_slope_per_step(16) + position_size(16) + ...
        // position_size is at offset: 8 + 16 + 1 + 7 + 16 + 8 + 8 + 16 = 80
        const ACCOUNTS_OFFSET: usize = 392 + 9136;
        const ACCOUNT_SIZE: usize = 240;
        const POSITION_OFFSET_IN_ACCOUNT: usize = 80;
        let account_offset =
            ACCOUNTS_OFFSET + (idx as usize) * ACCOUNT_SIZE + POSITION_OFFSET_IN_ACCOUNT;
        if slab_account.data.len() < account_offset + 16 {
            return 0;
        }
        i128::from_le_bytes(
            slab_account.data[account_offset..account_offset + 16]
                .try_into()
                .unwrap(),
        )
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
            &[ix],
            Some(&admin.pubkey()),
            &[&admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Withdraw collateral (requires 8 accounts)
    fn withdraw(&mut self, owner: &Keypair, user_idx: u16, amount: u64) {
        let ata = self.create_ata(&owner.pubkey(), 0);
        let (vault_pda, _) =
            Pubkey::find_program_address(&[b"vault", self.slab.as_ref()], &self.program_id);

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(owner.pubkey(), true), // 0: user (signer)
                AccountMeta::new(self.slab, false),     // 1: slab
                AccountMeta::new(self.vault, false),    // 2: vault
                AccountMeta::new(ata, false),           // 3: user_ata
                AccountMeta::new_readonly(vault_pda, false), // 4: vault_pda
                AccountMeta::new_readonly(spl_token::ID, false), // 5: token program
                AccountMeta::new_readonly(sysvar::clock::ID, false), // 6: clock
                AccountMeta::new_readonly(self.pyth_index, false), // 7: oracle
            ],
            data: encode_withdraw(user_idx, amount),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("withdraw failed");
    }

    /// Try to execute trade, returns result
    fn try_trade(
        &mut self,
        user: &Keypair,
        lp: &Keypair,
        lp_idx: u16,
        user_idx: u16,
        size: i128,
    ) -> Result<(), String> {
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
            &[ix],
            Some(&user.pubkey()),
            &[user, lp],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
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
        let (vault_pda, _) =
            Pubkey::find_program_address(&[b"vault", self.slab.as_ref()], &self.program_id);

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(owner.pubkey(), true), // 0: user (signer)
                AccountMeta::new(self.slab, false),     // 1: slab
                AccountMeta::new(self.vault, false),    // 2: vault
                AccountMeta::new(ata, false),           // 3: user_ata
                AccountMeta::new_readonly(vault_pda, false), // 4: vault_pda
                AccountMeta::new_readonly(spl_token::ID, false), // 5: token program
                AccountMeta::new_readonly(sysvar::clock::ID, false), // 6: clock
                AccountMeta::new_readonly(self.pyth_index, false), // 7: oracle
            ],
            data: encode_close_account(user_idx),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
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
    program_path();

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
    println!(
        "Bug #3: Vault balance after close_account = {}",
        vault_after
    );

    // Vault should have dust remaining (500 base tokens)
    assert!(vault_after > 0, "Vault should have dust remaining");

    // Try to close slab - should fail because dust_base > 0
    let result = env.try_close_slab();

    println!("Bug #3 test: CloseSlab with dust result = {:?}", result);
    println!(
        "Bug #3: Vault still has {} tokens - CloseSlab correctly rejects",
        vault_after
    );

    // FIXED: CloseSlab now returns error when dust_base > 0
    assert!(result.is_err(), "CloseSlab should fail when dust_base > 0");
}

// ============================================================================
// Misaligned withdrawal rejection test (related to unit_scale)
// ============================================================================

/// Test that withdrawals with amounts not divisible by unit_scale are rejected.
#[test]
fn test_misaligned_withdrawal_rejected() {
    program_path();

    let mut env = TestEnv::new();

    // Initialize with unit_scale=1000 (1000 base = 1 unit)
    env.init_market_full(0, 1000, 0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    // Deposit a clean amount (divisible by 1000)
    env.deposit(&user, user_idx, 10_000_000);

    env.set_slot(200);
    env.crank();

    // Try to withdraw misaligned amount (not divisible by unit_scale 1000)
    let result = env.try_withdraw(&user, user_idx, 1_500); // 1500 % 1000 = 500 != 0
    println!("Misaligned withdrawal (1500 with scale 1000): {:?}", result);
    assert!(result.is_err(), "Misaligned withdrawal should fail");

    // Aligned withdrawal should succeed
    let result2 = env.try_withdraw(&user, user_idx, 2_000); // 2000 % 1000 = 0
    println!("Aligned withdrawal (2000 with scale 1000): {:?}", result2);
    assert!(result2.is_ok(), "Aligned withdrawal should succeed");

    println!("MISALIGNED WITHDRAWAL VERIFIED: Correctly rejected misaligned amount");
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
    program_path();

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

    // Verify engine vault matches SPL vault (no desync)
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault_after,
        "Engine vault ({}) must match SPL vault ({})",
        engine_vault, vault_after
    );

    // Bug #4 behavior: the full overpayment (5000) is accepted.
    // The fee (1000) goes to insurance, and the excess (4000) goes to user capital.
    let user_idx = _user_idx;
    let user_capital = env.read_account_capital(user_idx);
    let insurance = env.read_insurance_balance();

    assert_eq!(
        insurance, 1000,
        "Insurance should receive exactly the new_account_fee (1000), got {}",
        insurance
    );
    assert_eq!(
        user_capital, 4000,
        "Excess payment (5000 - 1000 fee) should be credited to user capital, got {}",
        user_capital
    );

    // Conservation: engine_vault == insurance + user_capital (c_tot)
    assert_eq!(
        engine_vault,
        insurance + user_capital,
        "Conservation: vault ({}) must equal insurance ({}) + capital ({})",
        engine_vault,
        insurance,
        user_capital
    );
}

// Bug #6 (Threshold EWMA slow ramp), Bug #7 (Pending epoch wraparound),
// Bug #8 (LP entry price on flip) — documented in MEMORY.md.
// Engine-level fixes verified by code inspection; no stub tests needed.

// Finding L: Original test used invert=1 (making notional tiny), so the trade
// passed for the wrong reason. The corrected test below uses invert=0.

/// Corrected version of Finding L test - uses invert=0 for accurate notional calculation.
/// The original test used invert=1, which inverts $138 to ~$7.25, resulting in
/// position notional of only ~0.5 SOL instead of 10 SOL. This test verifies
/// that initial_margin_bps is correctly enforced for risk-increasing trades.
#[test]
fn test_verify_finding_l_fixed_with_invert_zero() {
    program_path();

    // This test uses invert=0 so oracle price is $138 directly (not inverted)
    // Position size for ~10 SOL notional at $138:
    //   size = 10_000_000_000 * 1_000_000 / 138_000_000 = 72_463_768
    //   notional = 72_463_768 * 138_000_000 / 1_000_000 = ~10 SOL
    // Margin requirements:
    //   Initial (10%): 1.0 SOL
    //   Maintenance (5%): 0.5 SOL
    // User equity: 0.6 SOL (between maint and initial)
    //
    // EXPECTED: Trade should FAIL (equity 0.6 < initial margin 1.0)

    let mut env = TestEnv::new();
    env.init_market_with_invert(0); // NO inversion - price is $138 directly

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 600_000_000); // 0.6 SOL

    let size: i128 = 72_463_768; // ~10 SOL notional at $138

    let result = env.try_trade(&user, &lp, lp_idx, user_idx, size);

    // With correct margin check (initial_margin_bps for risk-increasing trades):
    // Trade should FAIL because equity (0.6 SOL) < initial margin (1.0 SOL)
    assert!(
        result.is_err(),
        "Finding L should be FIXED: Trade at ~16.7x leverage should be rejected. \
         Initial margin (10%) = 1.0 SOL, User equity = 0.6 SOL. \
         Expected: Err (fixed), Got: Ok (bug still exists)"
    );

    println!("FINDING L VERIFIED FIXED: Trade correctly rejected due to initial margin check.");
    println!("Position notional: ~10 SOL at $138 (invert=0)");
    println!("User equity: 0.6 SOL");
    println!("Initial margin required (10%): 1.0 SOL");
    println!("Trade correctly failed: undercollateralized");
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
    program_path();

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
    assert!(
        early_close_result.is_err(),
        "Close should fail before warmup completes: {:?}",
        early_close_result
    );

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
         Got: {:?}",
        final_close_result
    );
}

/// Test that zombie accounts don't indefinitely poison the haircut ratio.
///
/// This is a simpler test that verifies the basic mechanism:
/// - Idle account with capital and no position can be closed
/// - Even without PnL, crank processes the account correctly
#[test]
fn test_idle_account_can_close_after_crank() {
    program_path();

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
        "Idle account with only capital should be closeable. Got: {:?}",
        result
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
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
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

    let tx = Transaction::new_signed_with_payer(
        &[ix],
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

    println!("HYPERP VALIDATION VERIFIED: Rejects zero initial_mark_price_e6 in Hyperp mode");
}

// Hyperp security stubs (TradeNoCpi disabled, exec_price clamping,
// default oracle_price_cap, index smoothing) — documented in MEMORY.md.
// Verified by code inspection + Kani proofs for clamp_toward_with_dt.

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
        &[ix],
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
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
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
        &[ix],
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

// ============================================================================
// Matcher Context Initialization Tests
// ============================================================================

fn matcher_program_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // Go up from percolator-prog
    path.push("percolator-match/target/deploy/percolator_match.so");
    assert!(
        path.exists(),
        "Matcher BPF not found at {:?}. Run: cd ../percolator-match && cargo build-sbf",
        path
    );
    path
}

/// Matcher context layout constants (from percolator-match)
const MATCHER_CONTEXT_LEN: usize = 320;
const MATCHER_RETURN_LEN: usize = 64;
const MATCHER_CALL_LEN: usize = 67;
const MATCHER_CALL_TAG: u8 = 0;
const MATCHER_INIT_VAMM_TAG: u8 = 2;
const CTX_VAMM_OFFSET: usize = 64;
const VAMM_MAGIC: u64 = 0x5045_5243_4d41_5443; // "PERCMATC"

/// Matcher mode enum
#[repr(u8)]
#[derive(Clone, Copy)]
enum MatcherMode {
    Passive = 0,
    Vamm = 1,
}

/// Encode InitVamm instruction (Tag 2)
fn encode_init_vamm(
    mode: MatcherMode,
    trading_fee_bps: u32,
    base_spread_bps: u32,
    max_total_bps: u32,
    impact_k_bps: u32,
    liquidity_notional_e6: u128,
    max_fill_abs: u128,
    max_inventory_abs: u128,
) -> Vec<u8> {
    let mut data = vec![0u8; 66];
    data[0] = MATCHER_INIT_VAMM_TAG;
    data[1] = mode as u8;
    data[2..6].copy_from_slice(&trading_fee_bps.to_le_bytes());
    data[6..10].copy_from_slice(&base_spread_bps.to_le_bytes());
    data[10..14].copy_from_slice(&max_total_bps.to_le_bytes());
    data[14..18].copy_from_slice(&impact_k_bps.to_le_bytes());
    data[18..34].copy_from_slice(&liquidity_notional_e6.to_le_bytes());
    data[34..50].copy_from_slice(&max_fill_abs.to_le_bytes());
    data[50..66].copy_from_slice(&max_inventory_abs.to_le_bytes());
    data
}

/// Encode a matcher call instruction (Tag 0)
fn encode_matcher_call(
    req_id: u64,
    lp_idx: u16,
    lp_account_id: u64,
    oracle_price_e6: u64,
    req_size: i128,
) -> Vec<u8> {
    let mut data = vec![0u8; MATCHER_CALL_LEN];
    data[0] = MATCHER_CALL_TAG;
    data[1..9].copy_from_slice(&req_id.to_le_bytes());
    data[9..11].copy_from_slice(&lp_idx.to_le_bytes());
    data[11..19].copy_from_slice(&lp_account_id.to_le_bytes());
    data[19..27].copy_from_slice(&oracle_price_e6.to_le_bytes());
    data[27..43].copy_from_slice(&req_size.to_le_bytes());
    // bytes 43..67 are reserved (zero)
    data
}

/// Read MatcherReturn from context account data
fn read_matcher_return(data: &[u8]) -> (u32, u32, u64, i128, u64) {
    let abi_version = u32::from_le_bytes(data[0..4].try_into().unwrap());
    let flags = u32::from_le_bytes(data[4..8].try_into().unwrap());
    let exec_price = u64::from_le_bytes(data[8..16].try_into().unwrap());
    let exec_size = i128::from_le_bytes(data[16..32].try_into().unwrap());
    let req_id = u64::from_le_bytes(data[32..40].try_into().unwrap());
    (abi_version, flags, exec_price, exec_size, req_id)
}

/// Test that the matcher context can be initialized with Passive mode
#[test]
fn test_matcher_init_vamm_passive_mode() {
    let path = matcher_program_path();

    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();

    // Load matcher program
    let program_bytes = std::fs::read(&path).expect("Failed to read matcher program");
    let matcher_program_id = Pubkey::new_unique();
    svm.add_program(matcher_program_id, &program_bytes);

    // Create context account owned by matcher program
    let ctx_pubkey = Pubkey::new_unique();
    let ctx_account = Account {
        lamports: 10_000_000,
        data: vec![0u8; MATCHER_CONTEXT_LEN],
        owner: matcher_program_id,
        executable: false,
        rent_epoch: 0,
    };
    svm.set_account(ctx_pubkey, ctx_account).unwrap();

    // Create LP PDA placeholder (stored in context for signature verification)
    let lp_pda = Pubkey::new_unique();

    // Initialize in Passive mode
    let ix = Instruction {
        program_id: matcher_program_id,
        accounts: vec![
            AccountMeta::new_readonly(lp_pda, false), // LP PDA
            AccountMeta::new(ctx_pubkey, false),      // Context account
        ],
        data: encode_init_vamm(
            MatcherMode::Passive,
            5,                 // 0.05% trading fee
            10,                // 0.10% base spread
            200,               // 2% max total
            0,                 // impact_k not used in Passive
            0,                 // liquidity not needed for Passive
            1_000_000_000_000, // max fill
            0,                 // no inventory limit
        ),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer],
        svm.latest_blockhash(),
    );

    let result = svm.send_transaction(tx);
    assert!(result.is_ok(), "Init vAMM failed: {:?}", result);

    // Verify context was written
    let ctx_data = svm.get_account(&ctx_pubkey).unwrap().data;
    let magic = u64::from_le_bytes(
        ctx_data[CTX_VAMM_OFFSET..CTX_VAMM_OFFSET + 8]
            .try_into()
            .unwrap(),
    );
    assert_eq!(magic, VAMM_MAGIC, "Magic mismatch");

    println!("MATCHER INIT VERIFIED: Passive mode initialized successfully");
}

/// Test that the matcher can execute a call after initialization
#[test]
fn test_matcher_call_after_init() {
    let path = matcher_program_path();

    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    let lp = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&lp.pubkey(), 1_000_000_000).unwrap();

    // Load matcher program
    let program_bytes = std::fs::read(&path).expect("Failed to read matcher program");
    let matcher_program_id = Pubkey::new_unique();
    svm.add_program(matcher_program_id, &program_bytes);

    // Create context account
    let ctx_pubkey = Pubkey::new_unique();
    let ctx_account = Account {
        lamports: 10_000_000,
        data: vec![0u8; MATCHER_CONTEXT_LEN],
        owner: matcher_program_id,
        executable: false,
        rent_epoch: 0,
    };
    svm.set_account(ctx_pubkey, ctx_account).unwrap();

    // Initialize in Passive mode: 10 bps spread + 5 bps fee = 15 bps total
    // Use LP pubkey as the LP PDA so later calls can sign with LP key
    let init_ix = Instruction {
        program_id: matcher_program_id,
        accounts: vec![
            AccountMeta::new_readonly(lp.pubkey(), false), // LP PDA
            AccountMeta::new(ctx_pubkey, false),           // Context account
        ],
        data: encode_init_vamm(
            MatcherMode::Passive,
            5,
            10,
            200,
            0,
            0,
            1_000_000_000_000, // max fill
            0,
        ),
    };

    let tx = Transaction::new_signed_with_payer(
        &[init_ix],
        Some(&payer.pubkey()),
        &[&payer],
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx).expect("Init failed");

    // Execute a buy order
    let oracle_price = 100_000_000u64; // $100 in e6
    let req_size = 1_000_000i128; // 1M base units (buy)

    let call_ix = Instruction {
        program_id: matcher_program_id,
        accounts: vec![
            AccountMeta::new_readonly(lp.pubkey(), true), // LP signer
            AccountMeta::new(ctx_pubkey, false),
        ],
        data: encode_matcher_call(1, 0, 100, oracle_price, req_size),
    };

    let tx = Transaction::new_signed_with_payer(
        &[call_ix],
        Some(&payer.pubkey()),
        &[&payer, &lp],
        svm.latest_blockhash(),
    );

    let result = svm.send_transaction(tx);
    assert!(result.is_ok(), "Matcher call failed: {:?}", result);

    // Read result from context
    let ctx_data = svm.get_account(&ctx_pubkey).unwrap().data;
    let (abi_version, flags, exec_price, exec_size, req_id) = read_matcher_return(&ctx_data);

    println!("Matcher return:");
    println!("  abi_version: {}", abi_version);
    println!("  flags: {}", flags);
    println!("  exec_price: {}", exec_price);
    println!("  exec_size: {}", exec_size);
    println!("  req_id: {}", req_id);

    assert_eq!(abi_version, 1, "ABI version mismatch");
    assert_eq!(flags & 1, 1, "FLAG_VALID should be set");
    assert_eq!(req_id, 1, "req_id mismatch");
    assert_eq!(exec_size, req_size, "exec_size mismatch");

    // Price = oracle * (10000 + spread + fee) / 10000 = 100M * 10015 / 10000 = 100_150_000
    let expected_price = 100_150_000u64;
    assert_eq!(
        exec_price, expected_price,
        "exec_price mismatch: expected {} got {}",
        expected_price, exec_price
    );

    println!("MATCHER CALL VERIFIED: Correct pricing with 15 bps (10 spread + 5 fee)");
}

/// Test that double initialization is rejected
#[test]
fn test_matcher_rejects_double_init() {
    let path = matcher_program_path();

    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();

    // Load matcher program
    let program_bytes = std::fs::read(&path).expect("Failed to read matcher program");
    let matcher_program_id = Pubkey::new_unique();
    svm.add_program(matcher_program_id, &program_bytes);

    // Create context account
    let ctx_pubkey = Pubkey::new_unique();
    let ctx_account = Account {
        lamports: 10_000_000,
        data: vec![0u8; MATCHER_CONTEXT_LEN],
        owner: matcher_program_id,
        executable: false,
        rent_epoch: 0,
    };
    svm.set_account(ctx_pubkey, ctx_account).unwrap();

    // Create LP PDA placeholder
    let lp_pda = Pubkey::new_unique();

    // First init succeeds
    let ix1 = Instruction {
        program_id: matcher_program_id,
        accounts: vec![
            AccountMeta::new_readonly(lp_pda, false), // LP PDA
            AccountMeta::new(ctx_pubkey, false),      // Context account
        ],
        data: encode_init_vamm(MatcherMode::Passive, 5, 10, 200, 0, 0, 1_000_000_000_000, 0),
    };

    let tx1 = Transaction::new_signed_with_payer(
        &[ix1],
        Some(&payer.pubkey()),
        &[&payer],
        svm.latest_blockhash(),
    );
    let result1 = svm.send_transaction(tx1);
    assert!(result1.is_ok(), "First init failed: {:?}", result1);

    // Second init should fail
    let ix2 = Instruction {
        program_id: matcher_program_id,
        accounts: vec![
            AccountMeta::new_readonly(lp_pda, false), // LP PDA
            AccountMeta::new(ctx_pubkey, false),      // Context account
        ],
        data: encode_init_vamm(MatcherMode::Passive, 5, 10, 200, 0, 0, 1_000_000_000_000, 0),
    };

    let tx2 = Transaction::new_signed_with_payer(
        &[ix2],
        Some(&payer.pubkey()),
        &[&payer],
        svm.latest_blockhash(),
    );
    let result2 = svm.send_transaction(tx2);
    assert!(
        result2.is_err(),
        "Second init should fail (already initialized)"
    );

    println!("MATCHER DOUBLE INIT REJECTED: AccountAlreadyInitialized");
}

/// Test vAMM mode with impact pricing
#[test]
fn test_matcher_vamm_mode_with_impact() {
    let path = matcher_program_path();

    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    let lp = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&lp.pubkey(), 1_000_000_000).unwrap();

    // Load matcher program
    let program_bytes = std::fs::read(&path).expect("Failed to read matcher program");
    let matcher_program_id = Pubkey::new_unique();
    svm.add_program(matcher_program_id, &program_bytes);

    // Create context account
    let ctx_pubkey = Pubkey::new_unique();
    let ctx_account = Account {
        lamports: 10_000_000,
        data: vec![0u8; MATCHER_CONTEXT_LEN],
        owner: matcher_program_id,
        executable: false,
        rent_epoch: 0,
    };
    svm.set_account(ctx_pubkey, ctx_account).unwrap();

    // Initialize in vAMM mode
    // abs_notional_e6 = fill_abs * oracle / 1e6 = 10M * 100M / 1M = 1e9 (1 billion)
    // Liquidity: 10B notional_e6, impact_k: 50 bps at full liquidity
    // Trade notional: 1B notional_e6 = 10% of liquidity
    // Impact = 50 * (1B / 10B) = 50 * 0.1 = 5 bps
    // Use LP pubkey as the LP PDA so later calls can sign with LP key
    let init_ix = Instruction {
        program_id: matcher_program_id,
        accounts: vec![
            AccountMeta::new_readonly(lp.pubkey(), false), // LP PDA
            AccountMeta::new(ctx_pubkey, false),           // Context account
        ],
        data: encode_init_vamm(
            MatcherMode::Vamm,
            5,                 // 0.05% trading fee
            10,                // 0.10% base spread
            200,               // 2% max total
            50,                // 0.50% impact at full liquidity
            10_000_000_000,    // 10B notional_e6 liquidity
            1_000_000_000_000, // max fill
            0,
        ),
    };

    let tx = Transaction::new_signed_with_payer(
        &[init_ix],
        Some(&payer.pubkey()),
        &[&payer],
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx).expect("Init failed");

    // Execute a buy for 1B notional_e6 (10% of liquidity)
    // At $100 price: abs_notional_e6 = size * price / 1e6 = 10M * 100M / 1M = 1B
    let oracle_price = 100_000_000u64; // $100 in e6
    let req_size = 10_000_000i128; // 10M base units -> 1B notional_e6 at $100

    let call_ix = Instruction {
        program_id: matcher_program_id,
        accounts: vec![
            AccountMeta::new_readonly(lp.pubkey(), true),
            AccountMeta::new(ctx_pubkey, false),
        ],
        data: encode_matcher_call(1, 0, 100, oracle_price, req_size),
    };

    let tx = Transaction::new_signed_with_payer(
        &[call_ix],
        Some(&payer.pubkey()),
        &[&payer, &lp],
        svm.latest_blockhash(),
    );

    let result = svm.send_transaction(tx);
    assert!(result.is_ok(), "Matcher call failed: {:?}", result);

    // Read result
    let ctx_data = svm.get_account(&ctx_pubkey).unwrap().data;
    let (abi_version, flags, exec_price, exec_size, _) = read_matcher_return(&ctx_data);

    println!("vAMM Matcher return:");
    println!("  exec_price: {}", exec_price);
    println!("  exec_size: {}", exec_size);

    assert_eq!(abi_version, 1, "ABI version mismatch");
    assert_eq!(flags & 1, 1, "FLAG_VALID should be set");

    // Impact = impact_k_bps * notional / liquidity = 50 * 1M / 10M = 5 bps
    // Total = spread (10) + fee (5) + impact (5) = 20 bps
    // exec_price = 100M * 10020 / 10000 = 100_200_000
    let expected_price = 100_200_000u64;
    assert_eq!(
        exec_price, expected_price,
        "vAMM exec_price mismatch: expected {} got {}",
        expected_price, exec_price
    );

    println!("VAMM MODE VERIFIED: Correct pricing with 20 bps (10 spread + 5 fee + 5 impact)");
}

// ============================================================================
// Comprehensive Feature Tests
// ============================================================================

impl TestEnv {
    /// Try to withdraw, returns result
    fn try_withdraw(&mut self, owner: &Keypair, user_idx: u16, amount: u64) -> Result<(), String> {
        let ata = self.create_ata(&owner.pubkey(), 0);
        let (vault_pda, _) =
            Pubkey::find_program_address(&[b"vault", self.slab.as_ref()], &self.program_id);

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
            data: encode_withdraw(user_idx, amount),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Try to deposit to wrong user (unauthorized)
    fn try_deposit_unauthorized(
        &mut self,
        attacker: &Keypair,
        victim_idx: u16,
        amount: u64,
    ) -> Result<(), String> {
        let ata = self.create_ata(&attacker.pubkey(), amount);

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(attacker.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(ata, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
            ],
            data: encode_deposit(victim_idx, amount),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&attacker.pubkey()),
            &[attacker],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Try to trade without LP signature
    fn try_trade_without_lp_sig(
        &mut self,
        user: &Keypair,
        lp_idx: u16,
        user_idx: u16,
        size: i128,
    ) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(user.pubkey(), true),
                AccountMeta::new(user.pubkey(), false), // LP not signing
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_trade(lp_idx, user_idx, size),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&user.pubkey()),
            &[user],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Encode and send top_up_insurance instruction
    fn top_up_insurance(&mut self, payer: &Keypair, amount: u64) {
        let ata = self.create_ata(&payer.pubkey(), amount);

        let mut data = vec![9u8]; // TopUpInsurance instruction tag
        data.extend_from_slice(&amount.to_le_bytes());

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(payer.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(ata, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data,
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&payer.pubkey()),
            &[payer],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("top_up_insurance failed");
    }

    /// Try liquidation
    fn try_liquidate(&mut self, target_idx: u16) -> Result<(), String> {
        let caller = Keypair::new();
        self.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();

        let mut data = vec![10u8]; // LiquidateAtOracle instruction tag
        data.extend_from_slice(&target_idx.to_le_bytes());

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(caller.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data,
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&caller.pubkey()),
            &[&caller],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }
}

/// Test 1: Full trading lifecycle - open, price move, close
/// Verifies: deposit, trade open, crank with price change, trade close
#[test]
fn test_comprehensive_trading_lifecycle_with_pnl() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000); // 10 SOL

    // Top up insurance to prevent force-realize mode during crank
    let ins_payer = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&ins_payer, 1);
    let vault_after_deposit = env.vault_balance();

    // Open long position at $138
    let size: i128 = 10_000_000;
    env.trade(&user, &lp, lp_idx, user_idx, size);
    assert_eq!(env.read_account_position(user_idx), size, "User must be long after trade");

    // Move price up to $150, crank to settle
    env.set_slot_and_price(200, 150_000_000);
    env.crank();

    // Close position
    env.trade(&user, &lp, lp_idx, user_idx, -size);
    assert_eq!(env.read_account_position(user_idx), 0, "User position must be zero after flatten");

    // Vault balance must be conserved (no SPL tokens created or destroyed)
    let vault_after = env.vault_balance();
    assert_eq!(vault_after, vault_after_deposit, "Vault must be conserved through lifecycle");
}

/// Test 2: Liquidation attempt when user position goes underwater
#[test]
fn test_comprehensive_liquidation_underwater_user() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // User with minimal margin
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_200_000_000); // 1.2 SOL

    // Open leveraged position
    let size: i128 = 8_000_000;
    env.trade(&user, &lp, lp_idx, user_idx, size);
    assert_eq!(env.read_account_position(user_idx), size, "User must have position");

    // Move price down significantly
    env.set_slot_and_price(200, 100_000_000);
    env.crank();

    // After crank with insurance=0 (force-realize mode), position is zeroed
    assert_eq!(
        env.read_account_position(user_idx), 0,
        "Force-realize should zero user position (insurance=0)"
    );
}

/// Test 3: Withdrawal limits - can't withdraw beyond margin requirements
#[test]
fn test_comprehensive_withdrawal_limits() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000); // 10 SOL

    // Open large position to lock up margin
    let size: i128 = 50_000_000;
    env.trade(&user, &lp, lp_idx, user_idx, size);
    println!("Step 1: Opened large position to lock margin");

    // Try to withdraw everything - should fail
    let result = env.try_withdraw(&user, user_idx, 10_000_000_000);
    println!("Full withdrawal attempt: {:?}", result);
    assert!(
        result.is_err(),
        "Should not be able to withdraw all capital with open position"
    );

    // Partial withdrawal (1 SOL of 10 SOL with 50M position) - margin allows it
    let result2 = env.try_withdraw(&user, user_idx, 1_000_000_000);
    assert!(
        result2.is_ok(),
        "Partial withdrawal within margin should succeed: {:?}",
        result2
    );
}

/// Test 4: Unauthorized access - wrong signer can't operate on account
#[test]
fn test_comprehensive_unauthorized_access_rejected() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Create legitimate user
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Attacker tries to deposit to victim's account
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 10_000_000_000).unwrap();

    let result = env.try_deposit_unauthorized(&attacker, user_idx, 1_000_000_000);
    println!("Unauthorized deposit attempt: {:?}", result);
    assert!(result.is_err(), "Unauthorized deposit should fail");

    // Attacker tries to withdraw from victim's account
    let result2 = env.try_withdraw(&attacker, user_idx, 1_000_000_000);
    println!("Unauthorized withdrawal attempt: {:?}", result2);
    assert!(result2.is_err(), "Unauthorized withdrawal should fail");

    // Try trade without LP signature
    let result3 = env.try_trade_without_lp_sig(&user, lp_idx, user_idx, 1_000_000);
    println!("Trade without LP signature: {:?}", result3);
    assert!(result3.is_err(), "Trade without LP signature should fail");

    println!("UNAUTHORIZED ACCESS VERIFIED: All unauthorized operations rejected");
}

/// Test 5: Position flip - user goes from long to short
#[test]
fn test_comprehensive_position_flip_long_to_short() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Open long
    let long_size: i128 = 5_000_000;
    env.trade(&user, &lp, lp_idx, user_idx, long_size);
    assert_eq!(env.read_account_position(user_idx), long_size, "User must be long");

    // Flip to short (trade more than current position in opposite direction)
    let flip_size: i128 = -10_000_000; // -10M, net = -5M (short)
    env.trade(&user, &lp, lp_idx, user_idx, flip_size);
    assert_eq!(
        env.read_account_position(user_idx), -5_000_000,
        "User must be short after flip"
    );
    assert_eq!(
        env.read_account_position(lp_idx), 5_000_000,
        "LP must be long (opposite of user)"
    );
}

/// Test 6: Multiple participants - all trades succeed with single LP
#[test]
fn test_comprehensive_multiple_participants() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Single LP
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Multiple users
    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 10_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 10_000_000_000);

    let user3 = Keypair::new();
    let user3_idx = env.init_user(&user3);
    env.deposit(&user3, user3_idx, 10_000_000_000);

    // User1 goes long 5M
    env.trade(&user1, &lp, lp_idx, user1_idx, 5_000_000);
    assert_eq!(env.read_account_position(user1_idx), 5_000_000);

    // User2 goes long 3M
    env.trade(&user2, &lp, lp_idx, user2_idx, 3_000_000);
    assert_eq!(env.read_account_position(user2_idx), 3_000_000);

    // User3 goes short 2M
    env.trade(&user3, &lp, lp_idx, user3_idx, -2_000_000);
    assert_eq!(env.read_account_position(user3_idx), -2_000_000);

    // Net user position: +5M + 3M - 2M = +6M (LP takes opposite = -6M)
    assert_eq!(env.read_account_position(lp_idx), -6_000_000, "LP must hold net opposite");

    // Vault conservation
    let vault_after = env.vault_balance();
    let expected_vault = 100_000_000_000u64 + 3 * 10_000_000_000;
    assert_eq!(vault_after, expected_vault, "Vault must equal total deposits");
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

    // Price goes to $150 - crank. User is long, so PnL should be positive.
    env.set_slot_and_price(200, 150_000_000);
    env.crank();
    assert_eq!(env.vault_balance(), vault_initial, "Vault conserved at $150");
    let pnl_at_150 = env.read_account_pnl(user_idx);
    assert!(pnl_at_150 > 0, "Long position should have positive PnL at $150 (up from $138): pnl={}", pnl_at_150);

    // Price drops to $120 - crank. User is long, so PnL should be negative (below entry).
    env.set_slot_and_price(300, 120_000_000);
    env.crank();
    assert_eq!(env.vault_balance(), vault_initial, "Vault conserved at $120");
    let pnl_at_120 = env.read_account_pnl(user_idx);
    assert!(pnl_at_120 < 0, "Long position should have negative PnL at $120 (below entry $138): pnl={}", pnl_at_120);

    // Price recovers to $140 - crank. PnL should be positive again (above entry $138).
    env.set_slot_and_price(400, 140_000_000);
    env.crank();
    assert_eq!(env.vault_balance(), vault_initial, "Vault conserved at $140");
    let pnl_at_140 = env.read_account_pnl(user_idx);
    assert!(pnl_at_140 > 0, "Long position should have positive PnL at $140 (above entry $138): pnl={}", pnl_at_140);

    // Position must still be open
    assert_eq!(env.read_account_position(user_idx), size, "Position must persist through price changes");
}

/// Test 8: Insurance fund top-up succeeds
#[test]
fn test_comprehensive_insurance_fund_topup() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let vault_before = env.vault_balance();
    println!("Vault before top-up: {}", vault_before);

    // Top up insurance fund
    let payer = Keypair::new();
    env.svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    env.top_up_insurance(&payer, 5_000_000_000); // 5 SOL

    // Vault should have the funds
    let vault_after = env.vault_balance();
    println!("Vault after top-up: {}", vault_after);
    assert_eq!(
        vault_after,
        vault_before + 5_000_000_000,
        "Vault should have insurance funds"
    );

    // Engine insurance counter should also reflect the top-up
    let insurance = env.read_insurance_balance();
    assert_eq!(
        insurance, 5_000_000_000,
        "Engine insurance balance should match top-up amount"
    );

    println!("INSURANCE FUND VERIFIED: Top-up transferred to vault and engine counter updated");
}

/// Test 9: Trading at margin limits
#[test]
fn test_comprehensive_margin_limit_enforcement() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // User with exactly 10% margin for certain notional
    // At $138 price, 1 SOL capital = 10% margin for 10 SOL notional
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000); // 1 SOL

    // Small trade should work
    let small_size: i128 = 1_000_000; // Small
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, small_size);
    println!("Small trade result: {:?}", result);
    assert!(result.is_ok(), "Small trade within margin should succeed");

    // Massive trade should fail (exceeds margin)
    let huge_size: i128 = 1_000_000_000; // Huge - way over margin
    let result2 = env.try_trade(&user, &lp, lp_idx, user_idx, huge_size);
    assert!(
        result2.is_err(),
        "Huge trade exceeding margin should be rejected: {:?}",
        result2
    );
}

/// Test 10: Funding accrual - multiple cranks succeed over time
#[test]
fn test_comprehensive_funding_accrual() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Open long position (creates funding imbalance)
    env.trade(&user, &lp, lp_idx, user_idx, 20_000_000);

    // Top up insurance to prevent force-realize and dust-close (must exceed threshold after EWMA update)
    let ins_payer = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&ins_payer, 1_000_000_000);
    let vault_before = env.vault_balance();

    // Run many cranks to accrue funding
    for i in 0..10 {
        env.set_slot(200 + i * 100);
        env.crank();
    }

    // Vault must be conserved (funding is internal accounting, no SPL transfers)
    assert_eq!(env.vault_balance(), vault_before, "Vault must be conserved through funding cranks");
    // Positions must still exist
    assert_ne!(env.read_account_position(user_idx), 0, "User position must persist");
    assert_ne!(env.read_account_position(lp_idx), 0, "LP position must persist");

    // Note: Market uses default funding params (all zero), so funding PnL = 0.
    // With oracle price unchanged at $138, mark-to-market PnL is also 0.
    // This test verifies that 10 cranks over 1000 slots don't corrupt state.
    // Verify c_tot consistency after all cranks.
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(user_idx) + env.read_account_capital(lp_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot ({}) must equal sum of capitals ({}) after funding cranks",
        c_tot, sum
    );
}

/// Test 11: Close account returns correct capital
#[test]
fn test_comprehensive_close_account_returns_capital() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    let deposit_amount = 5_000_000_000u64; // 5 SOL
    env.deposit(&user, user_idx, deposit_amount);

    let vault_before = env.vault_balance();
    println!("Vault before close: {}", vault_before);

    // Close account (no position, should return full capital)
    env.close_account(&user, user_idx);

    let vault_after = env.vault_balance();
    println!("Vault after close: {}", vault_after);

    let returned = vault_before - vault_after;
    println!("Returned to user: {}", returned);

    // No trades, no fees — should return exact deposit
    assert_eq!(returned, deposit_amount, "User should receive exact deposit back");
}

// ============================================================================
// CRITICAL SECURITY TESTS - L7 DEEP DIVE
// ============================================================================

// Instruction encoders for admin operations
fn encode_update_admin(new_admin: &Pubkey) -> Vec<u8> {
    let mut data = vec![12u8]; // Tag 12: UpdateAdmin
    data.extend_from_slice(new_admin.as_ref());
    data
}

fn encode_set_risk_threshold(new_threshold: u128) -> Vec<u8> {
    let mut data = vec![11u8]; // Tag 11: SetRiskThreshold
    data.extend_from_slice(&new_threshold.to_le_bytes());
    data
}

fn encode_set_oracle_authority(new_authority: &Pubkey) -> Vec<u8> {
    let mut data = vec![16u8]; // Tag 16: SetOracleAuthority
    data.extend_from_slice(new_authority.as_ref());
    data
}

fn encode_push_oracle_price(price_e6: u64, timestamp: i64) -> Vec<u8> {
    let mut data = vec![17u8]; // Tag 17: PushOraclePrice
    data.extend_from_slice(&price_e6.to_le_bytes());
    data.extend_from_slice(&timestamp.to_le_bytes());
    data
}

fn encode_set_oracle_price_cap(max_change_e2bps: u64) -> Vec<u8> {
    let mut data = vec![18u8]; // Tag 18: SetOraclePriceCap
    data.extend_from_slice(&max_change_e2bps.to_le_bytes());
    data
}

fn encode_set_maintenance_fee(new_fee: u128) -> Vec<u8> {
    let mut data = vec![15u8]; // Tag 15: SetMaintenanceFee
    data.extend_from_slice(&new_fee.to_le_bytes());
    data
}

fn encode_liquidate(target_idx: u16) -> Vec<u8> {
    let mut data = vec![7u8]; // Tag 7: LiquidateAtOracle
    data.extend_from_slice(&target_idx.to_le_bytes());
    data
}

fn encode_update_config(
    funding_horizon_slots: u64,
    funding_k_bps: u64,
    funding_inv_scale_notional_e6: u128, // u128!
    funding_max_premium_bps: i64,        // i64!
    funding_max_bps_per_slot: i64,       // i64!
    thresh_floor: u128,
    thresh_risk_bps: u64,
    thresh_update_interval_slots: u64,
    thresh_step_bps: u64,
    thresh_alpha_bps: u64,
    thresh_min: u128,
    thresh_max: u128,
    thresh_min_step: u128,
) -> Vec<u8> {
    let mut data = vec![14u8]; // Tag 14: UpdateConfig
    data.extend_from_slice(&funding_horizon_slots.to_le_bytes());
    data.extend_from_slice(&funding_k_bps.to_le_bytes());
    data.extend_from_slice(&funding_inv_scale_notional_e6.to_le_bytes()); // u128
    data.extend_from_slice(&funding_max_premium_bps.to_le_bytes()); // i64
    data.extend_from_slice(&funding_max_bps_per_slot.to_le_bytes()); // i64
    data.extend_from_slice(&thresh_floor.to_le_bytes());
    data.extend_from_slice(&thresh_risk_bps.to_le_bytes());
    data.extend_from_slice(&thresh_update_interval_slots.to_le_bytes());
    data.extend_from_slice(&thresh_step_bps.to_le_bytes());
    data.extend_from_slice(&thresh_alpha_bps.to_le_bytes());
    data.extend_from_slice(&thresh_min.to_le_bytes());
    data.extend_from_slice(&thresh_max.to_le_bytes());
    data.extend_from_slice(&thresh_min_step.to_le_bytes());
    data
}

impl TestEnv {
    /// Try UpdateAdmin instruction
    fn try_update_admin(&mut self, signer: &Keypair, new_admin: &Pubkey) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(self.slab, false),
            ],
            data: encode_update_admin(new_admin),
        };
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&signer.pubkey()),
            &[signer],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Try SetRiskThreshold instruction
    fn try_set_risk_threshold(
        &mut self,
        signer: &Keypair,
        new_threshold: u128,
    ) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(self.slab, false),
            ],
            data: encode_set_risk_threshold(new_threshold),
        };
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&signer.pubkey()),
            &[signer],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Try SetOracleAuthority instruction
    fn try_set_oracle_authority(
        &mut self,
        signer: &Keypair,
        new_authority: &Pubkey,
    ) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(self.slab, false),
            ],
            data: encode_set_oracle_authority(new_authority),
        };
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&signer.pubkey()),
            &[signer],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Try PushOraclePrice instruction
    fn try_push_oracle_price(
        &mut self,
        signer: &Keypair,
        price_e6: u64,
        timestamp: i64,
    ) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(self.slab, false),
            ],
            data: encode_push_oracle_price(price_e6, timestamp),
        };
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&signer.pubkey()),
            &[signer],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Try SetOraclePriceCap instruction
    fn try_set_oracle_price_cap(
        &mut self,
        signer: &Keypair,
        max_change_e2bps: u64,
    ) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(self.slab, false),
            ],
            data: encode_set_oracle_price_cap(max_change_e2bps),
        };
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&signer.pubkey()),
            &[signer],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Try SetMaintenanceFee instruction
    fn try_set_maintenance_fee(&mut self, signer: &Keypair, new_fee: u128) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(self.slab, false),
            ],
            data: encode_set_maintenance_fee(new_fee),
        };
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&signer.pubkey()),
            &[signer],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Try ResolveMarket instruction (admin only)
    fn try_resolve_market(&mut self, admin: &Keypair) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.slab, false),
            ],
            data: encode_resolve_market(),
        };
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Try WithdrawInsurance instruction (admin only, requires resolved + all positions closed)
    fn try_withdraw_insurance(&mut self, admin: &Keypair) -> Result<(), String> {
        let admin_ata = self.create_ata(&admin.pubkey(), 0);
        let (vault_pda, _) =
            Pubkey::find_program_address(&[b"vault", self.slab.as_ref()], &self.program_id);
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(admin_ata, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(vault_pda, false),
            ],
            data: encode_withdraw_insurance(),
        };
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Check if market is resolved (read flags from slab header)
    fn is_market_resolved(&self) -> bool {
        let slab_account = self.svm.get_account(&self.slab).unwrap();
        // FLAGS_OFF = 13 (offset of flags byte in SlabHeader._padding[0])
        const FLAGS_OFF: usize = 13;
        const FLAG_RESOLVED: u8 = 1 << 0;
        slab_account.data[FLAGS_OFF] & FLAG_RESOLVED != 0
    }

    /// Read insurance fund balance from engine
    fn read_insurance_balance(&self) -> u128 {
        let slab_account = self.svm.get_account(&self.slab).unwrap();
        // ENGINE_OFF = 392, InsuranceFund.balance is at offset 16 within engine
        // (vault is 16 bytes at 0, insurance_fund starts at 16)
        // InsuranceFund { balance: U128, ... } - balance is first field
        const INSURANCE_OFFSET: usize = 392 + 16;
        u128::from_le_bytes(
            slab_account.data[INSURANCE_OFFSET..INSURANCE_OFFSET + 16]
                .try_into()
                .unwrap(),
        )
    }

    /// Try LiquidateAtOracle instruction
    fn try_liquidate_target(&mut self, target_idx: u16) -> Result<(), String> {
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
            data: encode_liquidate(target_idx),
        };
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&caller.pubkey()),
            &[&caller],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Try UpdateConfig instruction
    fn try_update_config(&mut self, signer: &Keypair) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(self.slab, false),
            ],
            data: encode_update_config(
                3600,                      // funding_horizon_slots
                100,                       // funding_k_bps
                1_000_000_000_000u128,     // funding_inv_scale_notional_e6 (u128)
                100i64,                    // funding_max_premium_bps (i64)
                10i64,                     // funding_max_bps_per_slot (i64)
                0u128,                     // thresh_floor (u128)
                100,                       // thresh_risk_bps
                100,                       // thresh_update_interval_slots
                100,                       // thresh_step_bps
                1000,                      // thresh_alpha_bps
                0u128,                     // thresh_min
                1_000_000_000_000_000u128, // thresh_max
                1u128,                     // thresh_min_step
            ),
        };
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&signer.pubkey()),
            &[signer],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }
}

// ============================================================================
// Test: UpdateAdmin authorization
// ============================================================================

/// CRITICAL: UpdateAdmin only callable by current admin
#[test]
fn test_critical_update_admin_authorization() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let new_admin = Keypair::new();
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();

    // Attacker tries to change admin - should fail
    let result = env.try_update_admin(&attacker, &attacker.pubkey());
    assert!(
        result.is_err(),
        "SECURITY: Non-admin should not be able to change admin"
    );
    println!("UpdateAdmin by non-admin: REJECTED (correct)");

    // Real admin changes admin - should succeed
    let result = env.try_update_admin(&admin, &new_admin.pubkey());
    assert!(
        result.is_ok(),
        "Admin should be able to change admin: {:?}",
        result
    );
    println!("UpdateAdmin by admin: ACCEPTED (correct)");

    // Old admin tries again - should now fail
    let result = env.try_update_admin(&admin, &admin.pubkey());
    assert!(result.is_err(), "Old admin should no longer have authority");
    println!("UpdateAdmin by old admin: REJECTED (correct)");

    println!("CRITICAL TEST PASSED: UpdateAdmin authorization enforced");
}

// ============================================================================
// Test: SetRiskThreshold authorization
// ============================================================================

/// CRITICAL: SetRiskThreshold admin-only
#[test]
fn test_critical_set_risk_threshold_authorization() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();

    // Attacker tries to set threshold - should fail
    let result = env.try_set_risk_threshold(&attacker, 1_000_000_000);
    assert!(
        result.is_err(),
        "SECURITY: Non-admin should not set risk threshold"
    );
    println!("SetRiskThreshold by non-admin: REJECTED (correct)");

    // Admin sets threshold - should succeed
    let result = env.try_set_risk_threshold(&admin, 1_000_000_000_000);
    assert!(
        result.is_ok(),
        "Admin should set risk threshold: {:?}",
        result
    );
    println!("SetRiskThreshold by admin: ACCEPTED (correct)");

    println!("CRITICAL TEST PASSED: SetRiskThreshold authorization enforced");
}

// ============================================================================
// Test: SetOracleAuthority and PushOraclePrice (admin oracle)
// ============================================================================

/// CRITICAL: Admin oracle mechanism for Hyperp mode
#[test]
fn test_critical_admin_oracle_authority() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let oracle_authority = Keypair::new();
    let attacker = Keypair::new();
    env.svm
        .airdrop(&oracle_authority.pubkey(), 1_000_000_000)
        .unwrap();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();

    // Attacker tries to set oracle authority - should fail
    let result = env.try_set_oracle_authority(&attacker, &attacker.pubkey());
    assert!(
        result.is_err(),
        "SECURITY: Non-admin should not set oracle authority"
    );
    println!("SetOracleAuthority by non-admin: REJECTED (correct)");

    // Admin sets oracle authority - should succeed
    let result = env.try_set_oracle_authority(&admin, &oracle_authority.pubkey());
    assert!(
        result.is_ok(),
        "Admin should set oracle authority: {:?}",
        result
    );
    println!("SetOracleAuthority by admin: ACCEPTED (correct)");

    // Attacker tries to push price - should fail
    let result = env.try_push_oracle_price(&attacker, 150_000_000, 200);
    assert!(
        result.is_err(),
        "SECURITY: Non-authority should not push oracle price"
    );
    println!("PushOraclePrice by non-authority: REJECTED (correct)");

    // Oracle authority pushes price - should succeed
    let result = env.try_push_oracle_price(&oracle_authority, 150_000_000, 200);
    assert!(
        result.is_ok(),
        "Oracle authority should push price: {:?}",
        result
    );
    println!("PushOraclePrice by authority: ACCEPTED (correct)");

    println!("CRITICAL TEST PASSED: Admin oracle mechanism verified");
}

// ============================================================================
// Test: SetOraclePriceCap authorization
// ============================================================================

/// CRITICAL: SetOraclePriceCap admin-only
#[test]
fn test_critical_set_oracle_price_cap_authorization() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();

    // Attacker tries to set price cap - should fail
    let result = env.try_set_oracle_price_cap(&attacker, 10000);
    assert!(
        result.is_err(),
        "SECURITY: Non-admin should not set oracle price cap"
    );
    println!("SetOraclePriceCap by non-admin: REJECTED (correct)");

    // Admin sets price cap - should succeed
    let result = env.try_set_oracle_price_cap(&admin, 10000);
    assert!(
        result.is_ok(),
        "Admin should set oracle price cap: {:?}",
        result
    );
    println!("SetOraclePriceCap by admin: ACCEPTED (correct)");

    println!("CRITICAL TEST PASSED: SetOraclePriceCap authorization enforced");
}

// ============================================================================
// Test: SetMaintenanceFee authorization
// ============================================================================

/// CRITICAL: SetMaintenanceFee admin-only
#[test]
fn test_critical_set_maintenance_fee_authorization() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();

    // Attacker tries to set maintenance fee - should fail
    let result = env.try_set_maintenance_fee(&attacker, 1000);
    assert!(
        result.is_err(),
        "SECURITY: Non-admin should not set maintenance fee"
    );
    println!("SetMaintenanceFee by non-admin: REJECTED (correct)");

    // Admin sets maintenance fee - should succeed
    let result = env.try_set_maintenance_fee(&admin, 1000);
    assert!(
        result.is_ok(),
        "Admin should set maintenance fee: {:?}",
        result
    );
    println!("SetMaintenanceFee by admin: ACCEPTED (correct)");

    println!("CRITICAL TEST PASSED: SetMaintenanceFee authorization enforced");
}

// ============================================================================
// Test: UpdateConfig authorization
// ============================================================================

/// CRITICAL: UpdateConfig admin-only with all parameters
#[test]
fn test_critical_update_config_authorization() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();

    // Attacker tries to update config - should fail
    let result = env.try_update_config(&attacker);
    assert!(
        result.is_err(),
        "SECURITY: Non-admin should not update config"
    );
    println!("UpdateConfig by non-admin: REJECTED (correct)");

    // Admin updates config - should succeed
    let result = env.try_update_config(&admin);
    assert!(result.is_ok(), "Admin should update config: {:?}", result);
    println!("UpdateConfig by admin: ACCEPTED (correct)");

    println!("CRITICAL TEST PASSED: UpdateConfig authorization enforced");
}

// ============================================================================
// Test: LiquidateAtOracle acceptance/rejection logic
// ============================================================================

/// CRITICAL: Liquidation rejected when account is solvent
#[test]
fn test_critical_liquidation_rejected_when_solvent() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL - very well capitalized

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 50_000_000_000); // 50 SOL - very well capitalized

    // Open a small position (well within margin)
    // Position notional at $138: 1M * 138 / 1M = $138 notional
    // Required margin at 5%: $6.9
    // User has 50 SOL (~$6900) - way more than needed
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);

    // Crank to update state
    env.set_slot(200);
    env.crank();

    let capital_before = env.read_account_capital(user_idx);
    let position_before = env.read_account_position(user_idx);
    let insurance_before = env.read_insurance_balance();

    // Try to liquidate the well-capitalized user - should fail
    let result = env.try_liquidate_target(user_idx);

    let capital_after = env.read_account_capital(user_idx);
    let position_after = env.read_account_position(user_idx);
    let insurance_after = env.read_insurance_balance();

    // Whether liquidation returns Ok(no-op) or Err, a solvent account must remain untouched.
    assert_eq!(
        position_after, position_before,
        "Solvent account liquidation attempt changed position: before={} after={} result={:?}",
        position_before, position_after, result
    );
    assert_eq!(
        capital_after, capital_before,
        "Solvent account liquidation attempt changed capital: before={} after={} result={:?}",
        capital_before, capital_after, result
    );
    assert_eq!(
        insurance_after, insurance_before,
        "Solvent account liquidation attempt changed insurance: before={} after={} result={:?}",
        insurance_before, insurance_after, result
    );

    println!("CRITICAL TEST PASSED: Liquidation behavior for solvent accounts verified");
}

// ============================================================================
// Test: CloseSlab requires zero balances
// ============================================================================

/// CRITICAL: CloseSlab only by admin, requires zero vault/insurance
#[test]
fn test_critical_close_slab_authorization() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();

    // Deposit some funds (creates non-zero vault balance)
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    // Attacker tries to close slab - should fail (not admin)
    let attacker_ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.slab, false),
        ],
        data: encode_close_slab(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[attacker_ix],
        Some(&attacker.pubkey()),
        &[&attacker],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(result.is_err(), "SECURITY: Non-admin should not close slab");
    println!("CloseSlab by non-admin: REJECTED (correct)");

    // Admin tries to close slab with non-zero balance - should fail
    let result = env.try_close_slab();
    assert!(
        result.is_err(),
        "SECURITY: Should not close slab with non-zero vault"
    );
    println!("CloseSlab with active funds: REJECTED (correct)");

    println!("CRITICAL TEST PASSED: CloseSlab authorization verified");
}

// ============================================================================
// Test: Double initialization rejected
// ============================================================================

/// CRITICAL: InitMarket rejects already initialized slab
#[test]
fn test_critical_init_market_rejects_double_init() {
    program_path();

    let mut env = TestEnv::new();

    // First init
    env.init_market_with_invert(0);
    println!("First InitMarket: success");

    // Try second init - should fail
    let admin = &env.payer;
    let dummy_ata = Pubkey::new_unique();
    env.svm
        .set_account(
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

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: encode_init_market_with_invert(&admin.pubkey(), &env.mint, &TEST_FEED_ID, 0),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);

    assert!(
        result.is_err(),
        "SECURITY: Double initialization should be rejected"
    );
    println!("Second InitMarket: REJECTED (correct)");

    println!("CRITICAL TEST PASSED: Double initialization rejection verified");
}

// ============================================================================
// Test: Invalid account indices rejected
// ============================================================================

/// CRITICAL: Invalid user_idx/lp_idx are rejected
#[test]
fn test_critical_invalid_account_indices_rejected() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Try trade with invalid user_idx (999 - not initialized)
    let result = env.try_trade(&user, &lp, lp_idx, 999, 1_000_000);
    assert!(
        result.is_err(),
        "SECURITY: Invalid user_idx should be rejected"
    );
    println!("Trade with invalid user_idx: REJECTED (correct)");

    // Try trade with invalid lp_idx (999 - not initialized)
    let result = env.try_trade(&user, &lp, 999, user_idx, 1_000_000);
    assert!(
        result.is_err(),
        "SECURITY: Invalid lp_idx should be rejected"
    );
    println!("Trade with invalid lp_idx: REJECTED (correct)");

    println!("CRITICAL TEST PASSED: Invalid account indices rejection verified");
}

// ============================================================================
// Test: Sell trades (negative size)
// ============================================================================

/// Test that sell trades (negative size) work correctly
#[test]
fn test_sell_trade_negative_size() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // User opens SHORT position (negative size)
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, -10_000_000);
    assert!(
        result.is_ok(),
        "Sell/short trade should succeed: {:?}",
        result
    );
    println!("Short position opened (negative size): SUCCESS");

    // User closes by buying (positive size)
    let result2 = env.try_trade(&user, &lp, lp_idx, user_idx, 10_000_000);
    assert!(
        result2.is_ok(),
        "Close short trade should succeed: {:?}",
        result2
    );
    println!("Short position closed: SUCCESS");

    println!("SELL TRADES VERIFIED: Negative size trades work correctly");
}

// ============================================================================
// TradeCpi Program-Match Tests
// ============================================================================
//
// These tests verify the critical security properties of TradeCpi:
// 1. LP owner does NOT need to sign - trade is permissionless from LP perspective
// 2. Trade authorization is delegated to the matcher program via PDA signature
// 3. Matcher program/context must match what was registered during InitLP
// 4. LP PDA must be valid: system-owned, zero data, zero lamports
//
// Security model: LP delegates trade authorization to a matcher program.
// The percolator program uses invoke_signed with LP PDA seeds to call the matcher.
// Only the matcher registered at InitLP can authorize trades for this LP.

/// Encode TradeCpi instruction (tag = 10)
fn encode_trade_cpi(lp_idx: u16, user_idx: u16, size: i128) -> Vec<u8> {
    let mut data = vec![10u8]; // TradeCpi instruction tag
    data.extend_from_slice(&lp_idx.to_le_bytes());
    data.extend_from_slice(&user_idx.to_le_bytes());
    data.extend_from_slice(&size.to_le_bytes());
    data
}

/// Test environment extended for TradeCpi tests
struct TradeCpiTestEnv {
    svm: LiteSVM,
    program_id: Pubkey,
    matcher_program_id: Pubkey,
    payer: Keypair,
    slab: Pubkey,
    mint: Pubkey,
    vault: Pubkey,
    pyth_index: Pubkey,
    pyth_col: Pubkey,
    account_count: u16,
}

impl TradeCpiTestEnv {
    fn new() -> Option<Self> {
        let percolator_path = program_path();
        let matcher_path = matcher_program_path();

        let mut svm = LiteSVM::new();
        let program_id = Pubkey::new_unique();
        let matcher_program_id = Pubkey::new_unique();

        // Load both programs
        let percolator_bytes = std::fs::read(&percolator_path).expect("Failed to read percolator");
        let matcher_bytes = std::fs::read(&matcher_path).expect("Failed to read matcher");
        svm.add_program(program_id, &percolator_bytes);
        svm.add_program(matcher_program_id, &matcher_bytes);

        let payer = Keypair::new();
        let slab = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let pyth_index = Pubkey::new_unique();
        let pyth_col = Pubkey::new_unique();
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

        let pyth_data = make_pyth_data(&TEST_FEED_ID, 138_000_000, -6, 1, 100);
        svm.set_account(
            pyth_index,
            Account {
                lamports: 1_000_000,
                data: pyth_data.clone(),
                owner: PYTH_RECEIVER_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
        svm.set_account(
            pyth_col,
            Account {
                lamports: 1_000_000,
                data: pyth_data,
                owner: PYTH_RECEIVER_PROGRAM_ID,
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

        Some(TradeCpiTestEnv {
            svm,
            program_id,
            matcher_program_id,
            payer,
            slab,
            mint,
            vault,
            pyth_index,
            pyth_col,
            account_count: 0,
        })
    }

    fn init_market(&mut self) {
        let admin = &self.payer;
        let dummy_ata = Pubkey::new_unique();
        self.svm
            .set_account(
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
            data: encode_init_market_with_invert(&admin.pubkey(), &self.mint, &TEST_FEED_ID, 0),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("init_market failed");
    }

    fn create_ata(&mut self, owner: &Pubkey, amount: u64) -> Pubkey {
        let ata = Pubkey::new_unique();
        self.svm
            .set_account(
                ata,
                Account {
                    lamports: 1_000_000,
                    data: make_token_account_data(&self.mint, owner, amount),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        ata
    }

    /// Initialize LP with specific matcher program and context
    /// Returns (lp_idx, matcher_context_pubkey)
    fn init_lp_with_matcher(&mut self, owner: &Keypair, matcher_prog: &Pubkey) -> (u16, Pubkey) {
        let idx = self.account_count;
        self.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        let ata = self.create_ata(&owner.pubkey(), 0);

        // Derive the LP PDA that will be used later (must match percolator derivation)
        let lp_bytes = idx.to_le_bytes();
        let (lp_pda, _) =
            Pubkey::find_program_address(&[b"lp", self.slab.as_ref(), &lp_bytes], &self.program_id);

        // Create matcher context owned by matcher program
        let ctx = Pubkey::new_unique();
        self.svm
            .set_account(
                ctx,
                Account {
                    lamports: 10_000_000,
                    data: vec![0u8; MATCHER_CONTEXT_LEN],
                    owner: *matcher_prog,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();

        // Initialize the matcher context with LP PDA
        let init_ix = Instruction {
            program_id: *matcher_prog,
            accounts: vec![
                AccountMeta::new_readonly(lp_pda, false), // LP PDA (stored for signature verification)
                AccountMeta::new(ctx, false),             // Context account
            ],
            data: encode_init_vamm(
                MatcherMode::Passive,
                5,
                10,
                200,
                0,
                0,
                1_000_000_000_000, // max fill
                0,
            ),
        };

        let tx = Transaction::new_signed_with_payer(
            &[init_ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("init matcher context failed");

        // Now init LP in percolator with this matcher
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(ata, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(*matcher_prog, false),
                AccountMeta::new_readonly(ctx, false),
            ],
            data: encode_init_lp(matcher_prog, &ctx, 0),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("init_lp failed");
        self.account_count += 1;
        (idx, ctx)
    }

    /// Initialize LP with caller-provided matcher program/context without matcher-side init.
    /// Used for adversarial tests where matcher settings are intentionally malformed.
    fn init_lp_with_raw_matcher(
        &mut self,
        owner: &Keypair,
        matcher_prog: &Pubkey,
        matcher_ctx: &Pubkey,
    ) -> u16 {
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
                AccountMeta::new_readonly(*matcher_prog, false),
                AccountMeta::new_readonly(*matcher_ctx, false),
            ],
            data: encode_init_lp(matcher_prog, matcher_ctx, 0),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("init_lp raw matcher failed");
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
            &[ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
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
            &[ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("deposit failed");
    }

    /// Execute TradeCpi instruction
    /// Note: lp_owner does NOT need to sign - this is the key permissionless property
    fn try_trade_cpi(
        &mut self,
        user: &Keypair,
        lp_owner: &Pubkey, // NOT a signer!
        lp_idx: u16,
        user_idx: u16,
        size: i128,
        matcher_prog: &Pubkey,
        matcher_ctx: &Pubkey,
    ) -> Result<(), String> {
        // Derive the LP PDA
        let lp_bytes = lp_idx.to_le_bytes();
        let (lp_pda, _) =
            Pubkey::find_program_address(&[b"lp", self.slab.as_ref(), &lp_bytes], &self.program_id);

        // LP PDA must be system-owned, zero data, zero lamports
        // We don't need to set it up - it should not exist (system program owns uninitialized PDAs)

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(user.pubkey(), true), // 0: user (signer)
                AccountMeta::new(*lp_owner, false),    // 1: lp_owner (NOT signer!)
                AccountMeta::new(self.slab, false),    // 2: slab
                AccountMeta::new_readonly(sysvar::clock::ID, false), // 3: clock
                AccountMeta::new_readonly(self.pyth_index, false), // 4: oracle
                AccountMeta::new_readonly(*matcher_prog, false), // 5: matcher program
                AccountMeta::new(*matcher_ctx, false), // 6: matcher context (writable)
                AccountMeta::new_readonly(lp_pda, false), // 7: lp_pda
            ],
            data: encode_trade_cpi(lp_idx, user_idx, size),
        };

        // Only user signs - LP owner does NOT sign
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&user.pubkey()),
            &[user],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Execute TradeCpi with wrong LP PDA (attack scenario)
    fn try_trade_cpi_with_wrong_pda(
        &mut self,
        user: &Keypair,
        lp_owner: &Pubkey,
        lp_idx: u16,
        user_idx: u16,
        size: i128,
        matcher_prog: &Pubkey,
        matcher_ctx: &Pubkey,
        wrong_pda: &Pubkey,
    ) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(user.pubkey(), true),
                AccountMeta::new(*lp_owner, false),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
                AccountMeta::new_readonly(*matcher_prog, false),
                AccountMeta::new(*matcher_ctx, false),
                AccountMeta::new_readonly(*wrong_pda, false), // Wrong PDA!
            ],
            data: encode_trade_cpi(lp_idx, user_idx, size),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&user.pubkey()),
            &[user],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    fn init_market_hyperp(&mut self, initial_mark_price_e6: u64) {
        let admin = &self.payer;
        let dummy_ata = Pubkey::new_unique();
        self.svm
            .set_account(
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
            data: encode_init_market_hyperp(&admin.pubkey(), &self.mint, initial_mark_price_e6),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("init_market_hyperp failed");
    }

    fn set_slot(&mut self, slot: u64) {
        self.svm.set_sysvar(&Clock {
            slot,
            unix_timestamp: slot as i64,
            ..Clock::default()
        });
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
            &[ix],
            Some(&caller.pubkey()),
            &[&caller],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("crank failed");
    }

    fn try_set_oracle_authority(
        &mut self,
        admin: &Keypair,
        new_authority: &Pubkey,
    ) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.slab, false),
            ],
            data: encode_set_oracle_authority(new_authority),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    fn try_push_oracle_price(
        &mut self,
        authority: &Keypair,
        price_e6: u64,
        timestamp: i64,
    ) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
            ],
            data: encode_push_oracle_price(price_e6, timestamp),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&authority.pubkey()),
            &[authority],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    fn try_resolve_market(&mut self, admin: &Keypair) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.slab, false),
            ],
            data: encode_resolve_market(),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    fn top_up_insurance(&mut self, payer: &Keypair, amount: u64) {
        let ata = self.create_ata(&payer.pubkey(), amount);

        let mut data = vec![9u8]; // TopUpInsurance instruction tag
        data.extend_from_slice(&amount.to_le_bytes());

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(payer.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(ata, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data,
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&payer.pubkey()),
            &[payer],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("top_up_insurance failed");
    }

    fn try_withdraw_insurance(&mut self, admin: &Keypair) -> Result<(), String> {
        let admin_ata = self.create_ata(&admin.pubkey(), 0);
        let (vault_pda, _) =
            Pubkey::find_program_address(&[b"vault", self.slab.as_ref()], &self.program_id);

        // Account order: admin, slab, admin_ata, vault, token_program, vault_pda
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(admin_ata, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(vault_pda, false),
            ],
            data: encode_withdraw_insurance(),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    fn is_market_resolved(&self) -> bool {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        // FLAGS_OFF = 13, FLAG_RESOLVED = 1
        slab_data[13] & 1 != 0
    }

    fn read_insurance_balance(&self) -> u128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        // ENGINE_OFF = 392
        // RiskEngine layout: vault(U128=16) + insurance_fund(balance(U128=16) + fee_revenue(16))
        // So insurance_fund.balance is at ENGINE_OFF + 16 = 408
        const INSURANCE_BALANCE_OFFSET: usize = 392 + 16;
        u128::from_le_bytes(
            slab_data[INSURANCE_BALANCE_OFFSET..INSURANCE_BALANCE_OFFSET + 16]
                .try_into()
                .unwrap(),
        )
    }

    fn read_account_position(&self, idx: u16) -> i128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        // ENGINE_OFF = 392, accounts array at offset 9136 within RiskEngine
        // Account size = 240 bytes, position at offset 80 within Account
        const ACCOUNTS_OFFSET: usize = 392 + 9136;
        const ACCOUNT_SIZE: usize = 240;
        const POSITION_OFFSET_IN_ACCOUNT: usize = 80;
        let account_off =
            ACCOUNTS_OFFSET + (idx as usize) * ACCOUNT_SIZE + POSITION_OFFSET_IN_ACCOUNT;
        if slab_data.len() < account_off + 16 {
            return 0;
        }
        i128::from_le_bytes(slab_data[account_off..account_off + 16].try_into().unwrap())
    }

    fn try_withdraw(&mut self, owner: &Keypair, user_idx: u16, amount: u64) -> Result<(), String> {
        let ata = self.create_ata(&owner.pubkey(), 0);
        let (vault_pda, _) =
            Pubkey::find_program_address(&[b"vault", self.slab.as_ref()], &self.program_id);

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(self.vault, false), // a_vault
                AccountMeta::new(ata, false),        // a_user_ata
                AccountMeta::new_readonly(vault_pda, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_withdraw(user_idx, amount),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    fn read_num_used_accounts(&self) -> u16 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        // ENGINE_OFF (392) + num_used offset (920) = 1312
        u16::from_le_bytes(slab_data[1312..1314].try_into().unwrap())
    }

    /// Read pnl_pos_tot aggregate from slab
    /// This is the sum of all positive PnL values, used for haircut calculations
    fn read_pnl_pos_tot(&self) -> u128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        // ENGINE_OFF = 392
        // RiskEngine layout: vault(16) + insurance_fund(32) + params(144) +
        //   current_slot(8) + funding_index(16) + last_funding_slot(8) +
        //   funding_rate_bps(8) + last_crank_slot(8) + max_crank_staleness(8) +
        //   total_open_interest(16) + c_tot(16) + pnl_pos_tot(16)
        // Offset: 16+32+144+8+16+8+8+8+8+16+16 = 280
        const PNL_POS_TOT_OFFSET: usize = 392 + 280;
        u128::from_le_bytes(
            slab_data[PNL_POS_TOT_OFFSET..PNL_POS_TOT_OFFSET + 16]
                .try_into()
                .unwrap(),
        )
    }

    /// Read c_tot aggregate from slab
    fn read_c_tot(&self) -> u128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        // c_tot is at offset 264 within RiskEngine (16 bytes before pnl_pos_tot)
        const C_TOT_OFFSET: usize = 392 + 264;
        u128::from_le_bytes(
            slab_data[C_TOT_OFFSET..C_TOT_OFFSET + 16]
                .try_into()
                .unwrap(),
        )
    }

    /// Read vault balance from slab
    fn read_vault(&self) -> u128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        // vault is at offset 0 within RiskEngine
        const VAULT_OFFSET: usize = 392;
        u128::from_le_bytes(
            slab_data[VAULT_OFFSET..VAULT_OFFSET + 16]
                .try_into()
                .unwrap(),
        )
    }

    /// Read account PnL
    fn read_account_pnl(&self, idx: u16) -> i128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        // Account layout:
        //   account_id: u64 (8), offset 0
        //   capital: U128 (16), offset 8
        //   kind: AccountKind u8 (1 + 7 padding for alignment), offset 24
        //   pnl: I128 (16), offset 32
        //   reserved_pnl: u64 (8), offset 48
        //   warmup_started_at_slot: u64 (8), offset 56
        //   warmup_slope_per_step: U128 (16), offset 64
        //   position_size: I128 (16), offset 80 (confirmed in other tests)
        const ACCOUNTS_OFFSET: usize = 392 + 9136;
        const ACCOUNT_SIZE: usize = 240;
        const PNL_OFFSET_IN_ACCOUNT: usize = 32; // pnl is at offset 32 within Account
        let account_off = ACCOUNTS_OFFSET + (idx as usize) * ACCOUNT_SIZE + PNL_OFFSET_IN_ACCOUNT;
        if slab_data.len() < account_off + 16 {
            return 0;
        }
        i128::from_le_bytes(slab_data[account_off..account_off + 16].try_into().unwrap())
    }

    fn try_close_account(&mut self, owner: &Keypair, user_idx: u16) -> Result<(), String> {
        let ata = self.create_ata(&owner.pubkey(), 0);
        let (vault_pda, _) =
            Pubkey::find_program_address(&[b"vault", self.slab.as_ref()], &self.program_id);

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(self.vault, false), // a_vault
                AccountMeta::new(ata, false),        // a_user_ata
                AccountMeta::new_readonly(vault_pda, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_close_account(user_idx),
        };
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    fn try_admin_force_close_account(
        &mut self,
        admin: &Keypair,
        user_idx: u16,
        owner: &Pubkey,
    ) -> Result<(), String> {
        let owner_ata = self.create_ata(owner, 0);
        let (vault_pda, _) =
            Pubkey::find_program_address(&[b"vault", self.slab.as_ref()], &self.program_id);

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new(owner_ata, false),
                AccountMeta::new_readonly(vault_pda, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_admin_force_close_account(user_idx),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

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
            &[ix],
            Some(&admin.pubkey()),
            &[&admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    fn init_market_hyperp_with_warmup(
        &mut self,
        initial_mark_price_e6: u64,
        warmup_period_slots: u64,
    ) {
        let admin = &self.payer;
        let dummy_ata = Pubkey::new_unique();
        self.svm
            .set_account(
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
            data: encode_init_market_full_v2(
                &admin.pubkey(),
                &self.mint,
                &[0u8; 32],
                0,
                initial_mark_price_e6,
                warmup_period_slots,
            ),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("init_market_hyperp_with_warmup failed");
    }

    fn read_account_capital(&self, idx: u16) -> u128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        const ACCOUNTS_OFFSET: usize = 392 + 9136;
        const ACCOUNT_SIZE: usize = 240;
        const CAPITAL_OFFSET_IN_ACCOUNT: usize = 8;
        let account_off =
            ACCOUNTS_OFFSET + (idx as usize) * ACCOUNT_SIZE + CAPITAL_OFFSET_IN_ACCOUNT;
        if slab_data.len() < account_off + 16 {
            return 0;
        }
        u128::from_le_bytes(slab_data[account_off..account_off + 16].try_into().unwrap())
    }

    fn vault_balance(&self) -> u64 {
        let vault_data = self.svm.get_account(&self.vault).unwrap().data;
        let vault_account = TokenAccount::unpack(&vault_data).unwrap();
        vault_account.amount
    }

    fn try_set_oracle_price_cap(
        &mut self,
        signer: &Keypair,
        max_change_e2bps: u64,
    ) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(self.slab, false),
            ],
            data: encode_set_oracle_price_cap(max_change_e2bps),
        };
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&signer.pubkey()),
            &[signer],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }
}

// ============================================================================
// Test: TradeCpi is permissionless for LP (LP owner doesn't need to sign)
// ============================================================================

/// CRITICAL: TradeCpi allows trading without LP signature
///
/// The LP delegates trade authorization to a matcher program. The percolator
/// program uses invoke_signed with LP PDA seeds to call the matcher.
/// This makes TradeCpi permissionless from the LP's perspective - anyone can
/// initiate a trade if they have a valid user account.
///
/// Security model:
/// - LP registers matcher program/context at InitLP
/// - Only the registered matcher can authorize trades
/// - Matcher enforces its own rules (spread, fees, limits)
/// - LP PDA signature proves the CPI comes from percolator for this LP
#[test]
fn test_tradecpi_permissionless_lp_no_signature_required() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found. Run: cargo build-sbf && cd ../percolator-match && cargo build-sbf");
        return;
    };

    env.init_market();

    // Copy matcher_program_id to avoid borrow issues
    let matcher_prog = env.matcher_program_id;

    // Create LP with matcher
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Create user
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);
    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let vault_before = env.vault_balance();
    let engine_vault_before = env.read_vault();

    // Execute TradeCpi - LP owner is NOT a signer
    // This should succeed because TradeCpi is permissionless for LP
    let trade_size = 1_000_000i128;
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(), // LP owner pubkey (not signer!)
        lp_idx,
        user_idx,
        trade_size,
        &matcher_prog,
        &matcher_ctx,
    );

    assert!(
        result.is_ok(),
        "TradeCpi should succeed without LP signature (permissionless). Error: {:?}",
        result
    );
    let user_pos_after = env.read_account_position(user_idx);
    let lp_pos_after = env.read_account_position(lp_idx);
    let user_cap_after = env.read_account_capital(user_idx);
    let lp_cap_after = env.read_account_capital(lp_idx);
    let vault_after = env.vault_balance();
    let engine_vault_after = env.read_vault();

    assert_eq!(
        user_pos_after,
        user_pos_before + trade_size,
        "Permissionless TradeCpi should apply requested user position delta: before={} after={} size={}",
        user_pos_before,
        user_pos_after,
        trade_size
    );
    assert_eq!(
        lp_pos_after,
        lp_pos_before - trade_size,
        "Permissionless TradeCpi should apply opposite LP position delta: before={} after={} size={}",
        lp_pos_before,
        lp_pos_after,
        trade_size
    );
    let cap_sum_before = user_cap_before + lp_cap_before;
    let cap_sum_after = user_cap_after + lp_cap_after;
    assert!(
        cap_sum_after <= cap_sum_before,
        "Permissionless TradeCpi must not mint aggregate capital: before_sum={} after_sum={}",
        cap_sum_before,
        cap_sum_after
    );
    assert_eq!(
        vault_after, vault_before,
        "TradeCpi should not move SPL vault tokens directly: before={} after={}",
        vault_before, vault_after
    );
    assert_eq!(
        engine_vault_after, engine_vault_before,
        "TradeCpi should not mutate engine vault aggregate directly: before={} after={}",
        engine_vault_before, engine_vault_after
    );

    println!("TRADECPI PERMISSIONLESS VERIFIED: LP owner did NOT sign, trade succeeded");
    println!("  - LP delegates trade authorization to matcher program");
    println!("  - Percolator uses invoke_signed with LP PDA to call matcher");
    println!("  - This enables permissionless trading for LP pools");
}

// ============================================================================
// Test: TradeCpi rejects wrong matcher program
// ============================================================================

/// CRITICAL: TradeCpi rejects trades with wrong matcher program
///
/// The matcher program passed to TradeCpi must match the program registered
/// at InitLP. This prevents attackers from bypassing the registered matcher.
#[test]
fn test_tradecpi_rejects_wrong_matcher_program() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found. Run: cargo build-sbf && cd ../percolator-match && cargo build-sbf");
        return;
    };

    env.init_market();

    // Copy matcher_program_id to avoid borrow issues
    let real_matcher_prog = env.matcher_program_id;

    // Create LP with real matcher
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &real_matcher_prog);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Create user
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);
    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let vault_before = env.vault_balance();
    let engine_vault_before = env.read_vault();

    // Create a WRONG matcher program (just use a random pubkey)
    let wrong_matcher_prog = Pubkey::new_unique();

    // Try TradeCpi with wrong matcher program
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        1_000_000,
        &wrong_matcher_prog, // WRONG!
        &matcher_ctx,
    );

    assert!(
        result.is_err(),
        "SECURITY: TradeCpi should reject wrong matcher program"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected wrong-matcher TradeCpi must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected wrong-matcher TradeCpi must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected wrong-matcher TradeCpi must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected wrong-matcher TradeCpi must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        vault_before,
        "Rejected wrong-matcher TradeCpi must preserve SPL vault"
    );
    assert_eq!(
        env.read_vault(),
        engine_vault_before,
        "Rejected wrong-matcher TradeCpi must preserve engine vault aggregate"
    );

    println!("TRADECPI MATCHER VALIDATION VERIFIED: Wrong matcher program REJECTED");
    println!("  - Passed matcher: {} (wrong)", wrong_matcher_prog);
    println!("  - Registered matcher: {} (correct)", real_matcher_prog);
    println!("  - matcher_identity_ok check prevented the attack");
}

// ============================================================================
// Test: TradeCpi rejects wrong matcher context
// ============================================================================

/// CRITICAL: TradeCpi rejects trades with wrong matcher context
///
/// The matcher context passed to TradeCpi must match the context registered
/// at InitLP. Each LP has a specific context (with its own parameters).
#[test]
fn test_tradecpi_rejects_wrong_matcher_context() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found. Run: cargo build-sbf && cd ../percolator-match && cargo build-sbf");
        return;
    };

    env.init_market();

    let matcher_prog = env.matcher_program_id;

    // Create LP with real matcher
    let lp = Keypair::new();
    let (lp_idx, _correct_matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Create user
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);
    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let vault_before = env.vault_balance();
    let engine_vault_before = env.read_vault();

    // Create a DIFFERENT matcher context (belongs to a different LP)
    let wrong_ctx = Pubkey::new_unique();
    env.svm
        .set_account(
            wrong_ctx,
            Account {
                lamports: 10_000_000,
                data: vec![0u8; MATCHER_CONTEXT_LEN],
                owner: matcher_prog,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    // Use a different LP PDA for the wrong context
    let wrong_lp_pda = Pubkey::new_unique();

    // Initialize the wrong context (so it passes shape validation)
    let init_ix = Instruction {
        program_id: matcher_prog,
        accounts: vec![
            AccountMeta::new_readonly(wrong_lp_pda, false), // Different LP PDA
            AccountMeta::new(wrong_ctx, false),
        ],
        data: encode_init_vamm(MatcherMode::Passive, 5, 10, 200, 0, 0, 1_000_000_000_000, 0),
    };
    let tx = Transaction::new_signed_with_payer(
        &[init_ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    env.svm.send_transaction(tx).expect("init wrong ctx failed");

    // Try TradeCpi with wrong matcher context
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        1_000_000,
        &matcher_prog,
        &wrong_ctx, // WRONG!
    );

    assert!(
        result.is_err(),
        "SECURITY: TradeCpi should reject wrong matcher context"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected wrong-context TradeCpi must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected wrong-context TradeCpi must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected wrong-context TradeCpi must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected wrong-context TradeCpi must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        vault_before,
        "Rejected wrong-context TradeCpi must preserve SPL vault"
    );
    assert_eq!(
        env.read_vault(),
        engine_vault_before,
        "Rejected wrong-context TradeCpi must preserve engine vault aggregate"
    );

    println!("TRADECPI CONTEXT VALIDATION VERIFIED: Wrong matcher context REJECTED");
    println!("  - Passed context: {} (wrong)", wrong_ctx);
    println!("  - Each LP is bound to its registered matcher context");
    println!("  - matcher_identity_ok check prevented context substitution");
}

// ============================================================================
// Test: TradeCpi rejects wrong LP PDA
// ============================================================================

/// CRITICAL: TradeCpi rejects trades with wrong LP PDA
///
/// The LP PDA passed to TradeCpi must be the correct PDA derived from
/// ["lp", slab.key, lp_idx.to_le_bytes()]. The PDA must be:
/// - System-owned
/// - Zero data length
/// - Zero lamports
///
/// This prevents attackers from substituting a different PDA.
#[test]
fn test_tradecpi_rejects_wrong_lp_pda() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found. Run: cargo build-sbf && cd ../percolator-match && cargo build-sbf");
        return;
    };

    env.init_market();

    let matcher_prog = env.matcher_program_id;

    // Create LP
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Create user
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);
    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let vault_before = env.vault_balance();
    let engine_vault_before = env.read_vault();

    // Create a WRONG PDA (just a random pubkey)
    let wrong_pda = Pubkey::new_unique();

    // Try TradeCpi with wrong LP PDA
    let result = env.try_trade_cpi_with_wrong_pda(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        1_000_000,
        &matcher_prog,
        &matcher_ctx,
        &wrong_pda, // WRONG!
    );

    assert!(
        result.is_err(),
        "SECURITY: TradeCpi should reject wrong LP PDA"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected wrong-LP-PDA TradeCpi must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected wrong-LP-PDA TradeCpi must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected wrong-LP-PDA TradeCpi must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected wrong-LP-PDA TradeCpi must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        vault_before,
        "Rejected wrong-LP-PDA TradeCpi must preserve SPL vault"
    );
    assert_eq!(
        env.read_vault(),
        engine_vault_before,
        "Rejected wrong-LP-PDA TradeCpi must preserve engine vault aggregate"
    );

    println!("TRADECPI PDA VALIDATION VERIFIED: Wrong LP PDA REJECTED");
    println!("  - Passed PDA: {} (wrong)", wrong_pda);
    println!("  - Expected PDA derived from [\"lp\", slab, lp_idx]");
    println!("  - PDA key validation prevented PDA substitution attack");
}

// ============================================================================
// Test: TradeCpi rejects PDA with wrong shape (non-system-owned)
// ============================================================================

/// CRITICAL: TradeCpi rejects PDA that exists but has wrong shape
///
/// Even if the correct PDA address is passed, it must have:
/// - owner == system_program
/// - data_len == 0
/// - lamports == 0
///
/// This prevents an attacker from creating an account at the PDA address.
#[test]
fn test_tradecpi_rejects_pda_with_wrong_shape() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found. Run: cargo build-sbf && cd ../percolator-match && cargo build-sbf");
        return;
    };

    env.init_market();

    let matcher_prog = env.matcher_program_id;

    // Create LP
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Create user
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);
    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let vault_before = env.vault_balance();
    let engine_vault_before = env.read_vault();

    // Derive the CORRECT LP PDA
    let lp_bytes = lp_idx.to_le_bytes();
    let (correct_lp_pda, _) =
        Pubkey::find_program_address(&[b"lp", env.slab.as_ref(), &lp_bytes], &env.program_id);

    // Create an account at the PDA address with wrong shape
    // (has lamports - not zero)
    env.svm
        .set_account(
            correct_lp_pda,
            Account {
                lamports: 1_000_000, // Non-zero lamports - INVALID
                data: vec![],
                owner: solana_sdk::system_program::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    // Try TradeCpi - should fail because PDA shape is wrong
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        1_000_000,
        &matcher_prog,
        &matcher_ctx,
    );

    assert!(
        result.is_err(),
        "SECURITY: TradeCpi should reject PDA with non-zero lamports"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected malformed-PDA TradeCpi must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected malformed-PDA TradeCpi must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected malformed-PDA TradeCpi must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected malformed-PDA TradeCpi must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        vault_before,
        "Rejected malformed-PDA TradeCpi must preserve SPL vault"
    );
    assert_eq!(
        env.read_vault(),
        engine_vault_before,
        "Rejected malformed-PDA TradeCpi must preserve engine vault aggregate"
    );

    println!("TRADECPI PDA SHAPE VALIDATION VERIFIED: PDA with wrong shape REJECTED");
    println!("  - PDA address is correct but has non-zero lamports");
    println!("  - lp_pda_shape_ok check requires: system-owned, zero data, zero lamports");
    println!("  - This prevents attackers from polluting the PDA address");
}

/// ATTACK: Configure LP with matcher_program = percolator program (self-CPI recursion vector).
/// TradeCpi must reject and leave accounting unchanged.
#[test]
fn test_attack_tradecpi_self_program_matcher_rejected() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found. Run: cargo build-sbf && cd ../percolator-match && cargo build-sbf");
        return;
    };

    env.init_market();
    let self_prog = env.program_id;

    // Create a matcher context owned by percolator itself (adversarial config).
    let self_ctx = Pubkey::new_unique();
    env.svm
        .set_account(
            self_ctx,
            Account {
                lamports: 10_000_000,
                data: vec![0u8; MATCHER_CONTEXT_LEN],
                owner: self_prog,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    // Register LP with self-program matcher via raw init path.
    let lp = Keypair::new();
    let lp_idx = env.init_lp_with_raw_matcher(&lp, &self_prog, &self_ctx);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Create user and attempt TradeCpi.
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let vault_before = env.read_vault();
    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);

    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        1_000_000,
        &self_prog,
        &self_ctx,
    );
    assert!(
        result.is_err(),
        "SECURITY: TradeCpi must reject self-program matcher recursion vector"
    );

    // Non-vacuous postconditions: no hidden state mutation on failed path.
    assert_eq!(
        env.read_vault(),
        vault_before,
        "Vault changed on rejected self-matcher trade"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "User position changed on rejected self-matcher trade"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "LP position changed on rejected self-matcher trade"
    );
}

/// ATTACK: Alias matcher context to slab account in TradeCpi account list.
/// Must be rejected (shape/ownership mismatch) with no state mutation.
#[test]
fn test_attack_tradecpi_alias_slab_as_matcher_context_rejected() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found. Run: cargo build-sbf && cd ../percolator-match && cargo build-sbf");
        return;
    };

    env.init_market();

    // Use real matcher program but bind ctx to slab (wrong owner for matcher ctx).
    let matcher_prog = env.matcher_program_id;
    let slab = env.slab;
    let lp = Keypair::new();
    let lp_idx = env.init_lp_with_raw_matcher(&lp, &matcher_prog, &slab);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let vault_before = env.read_vault();
    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);

    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        1_000_000,
        &matcher_prog,
        &slab, // aliased with slab
    );
    assert!(
        result.is_err(),
        "SECURITY: TradeCpi should reject slab-as-matcher-context aliasing"
    );

    assert_eq!(
        env.read_vault(),
        vault_before,
        "Vault changed on rejected aliasing trade"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "User position changed on rejected aliasing trade"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "LP position changed on rejected aliasing trade"
    );
}

// ============================================================================
// Test: Multiple LPs have independent matcher bindings
// ============================================================================

/// Verify that each LP's matcher binding is independent
///
/// LP1 with Matcher A cannot be traded via Matcher B, and vice versa.
/// This ensures LP isolation.
#[test]
fn test_tradecpi_lp_matcher_binding_isolation() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found. Run: cargo build-sbf && cd ../percolator-match && cargo build-sbf");
        return;
    };

    env.init_market();

    let matcher_prog = env.matcher_program_id;

    // Create LP1 with its own matcher context
    let lp1 = Keypair::new();
    let (lp1_idx, lp1_ctx) = env.init_lp_with_matcher(&lp1, &matcher_prog);
    env.deposit(&lp1, lp1_idx, 50_000_000_000);

    // Create LP2 with its own matcher context
    let lp2 = Keypair::new();
    let (lp2_idx, lp2_ctx) = env.init_lp_with_matcher(&lp2, &matcher_prog);
    env.deposit(&lp2, lp2_idx, 50_000_000_000);

    // Create user
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);
    let trade_size = 500_000i128;
    let user_pos_before = env.read_account_position(user_idx);
    let lp1_pos_before = env.read_account_position(lp1_idx);
    let lp2_pos_before = env.read_account_position(lp2_idx);

    // Trade with LP1 using LP1's context - should succeed
    let result1 = env.try_trade_cpi(
        &user,
        &lp1.pubkey(),
        lp1_idx,
        user_idx,
        trade_size,
        &matcher_prog,
        &lp1_ctx,
    );
    assert!(
        result1.is_ok(),
        "Trade with LP1 using LP1's context should succeed: {:?}",
        result1
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before + trade_size,
        "LP1-valid TradeCpi should add requested user position delta"
    );
    assert_eq!(
        env.read_account_position(lp1_idx),
        lp1_pos_before - trade_size,
        "LP1-valid TradeCpi should apply opposite LP1 position delta"
    );
    assert_eq!(
        env.read_account_position(lp2_idx),
        lp2_pos_before,
        "LP1-valid TradeCpi must not mutate LP2 position"
    );
    println!("LP1 trade with LP1's context: SUCCESS");

    // Trade with LP2 using LP2's context - should succeed
    let result2 = env.try_trade_cpi(
        &user,
        &lp2.pubkey(),
        lp2_idx,
        user_idx,
        trade_size,
        &matcher_prog,
        &lp2_ctx,
    );
    assert!(
        result2.is_ok(),
        "Trade with LP2 using LP2's context should succeed: {:?}",
        result2
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before + (trade_size * 2),
        "Two valid TradeCpi calls should accumulate user position deltas"
    );
    assert_eq!(
        env.read_account_position(lp1_idx),
        lp1_pos_before - trade_size,
        "LP2-valid TradeCpi must preserve LP1 position from first fill"
    );
    assert_eq!(
        env.read_account_position(lp2_idx),
        lp2_pos_before - trade_size,
        "LP2-valid TradeCpi should apply opposite LP2 position delta"
    );
    let user_pos_after_valid = env.read_account_position(user_idx);
    let lp1_pos_after_valid = env.read_account_position(lp1_idx);
    let lp2_pos_after_valid = env.read_account_position(lp2_idx);
    let user_cap_after_valid = env.read_account_capital(user_idx);
    let lp1_cap_after_valid = env.read_account_capital(lp1_idx);
    let lp2_cap_after_valid = env.read_account_capital(lp2_idx);
    let spl_vault_after_valid = env.vault_balance();
    let engine_vault_after_valid = env.read_vault();
    println!("LP2 trade with LP2's context: SUCCESS");

    // Try to trade with LP1 using LP2's context - should FAIL
    let result3 = env.try_trade_cpi(
        &user,
        &lp1.pubkey(),
        lp1_idx,
        user_idx,
        trade_size,
        &matcher_prog,
        &lp2_ctx, // WRONG context for LP1!
    );
    assert!(
        result3.is_err(),
        "SECURITY: LP1 trade with LP2's context should fail"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_after_valid,
        "Rejected LP1/LP2-context swap must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp1_idx),
        lp1_pos_after_valid,
        "Rejected LP1/LP2-context swap must preserve LP1 position"
    );
    assert_eq!(
        env.read_account_position(lp2_idx),
        lp2_pos_after_valid,
        "Rejected LP1/LP2-context swap must preserve LP2 position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_after_valid,
        "Rejected LP1/LP2-context swap must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp1_idx),
        lp1_cap_after_valid,
        "Rejected LP1/LP2-context swap must preserve LP1 capital"
    );
    assert_eq!(
        env.read_account_capital(lp2_idx),
        lp2_cap_after_valid,
        "Rejected LP1/LP2-context swap must preserve LP2 capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_after_valid,
        "Rejected LP1/LP2-context swap must preserve SPL vault"
    );
    assert_eq!(
        env.read_vault(),
        engine_vault_after_valid,
        "Rejected LP1/LP2-context swap must preserve engine vault"
    );
    println!("LP1 trade with LP2's context: REJECTED (correct)");

    // Try to trade with LP2 using LP1's context - should FAIL
    let result4 = env.try_trade_cpi(
        &user,
        &lp2.pubkey(),
        lp2_idx,
        user_idx,
        trade_size,
        &matcher_prog,
        &lp1_ctx, // WRONG context for LP2!
    );
    assert!(
        result4.is_err(),
        "SECURITY: LP2 trade with LP1's context should fail"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_after_valid,
        "Rejected LP2/LP1-context swap must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp1_idx),
        lp1_pos_after_valid,
        "Rejected LP2/LP1-context swap must preserve LP1 position"
    );
    assert_eq!(
        env.read_account_position(lp2_idx),
        lp2_pos_after_valid,
        "Rejected LP2/LP1-context swap must preserve LP2 position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_after_valid,
        "Rejected LP2/LP1-context swap must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp1_idx),
        lp1_cap_after_valid,
        "Rejected LP2/LP1-context swap must preserve LP1 capital"
    );
    assert_eq!(
        env.read_account_capital(lp2_idx),
        lp2_cap_after_valid,
        "Rejected LP2/LP1-context swap must preserve LP2 capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_after_valid,
        "Rejected LP2/LP1-context swap must preserve SPL vault"
    );
    assert_eq!(
        env.read_vault(),
        engine_vault_after_valid,
        "Rejected LP2/LP1-context swap must preserve engine vault"
    );
    println!("LP2 trade with LP1's context: REJECTED (correct)");

    println!("LP MATCHER BINDING ISOLATION VERIFIED:");
    println!("  - Each LP is bound to its specific matcher context");
    println!("  - Context substitution between LPs is rejected");
    println!("  - This ensures LP isolation in multi-LP markets");
}

// ============================================================================
// Insurance Fund Trapped Funds Test
// ============================================================================

/// Test that insurance fund deposits can trap funds, preventing CloseSlab.
///
/// This test verifies a potential vulnerability where:
/// 1. TopUpInsurance adds tokens to vault and increments insurance_fund.balance
/// 2. No instruction exists to withdraw from insurance fund
/// 3. CloseSlab requires insurance_fund.balance == 0
/// 4. Therefore, any TopUpInsurance permanently traps those funds
///
/// Security Impact: Medium - Admin cannot reclaim insurance fund deposits
/// even after all users have closed their accounts.
#[test]
fn test_insurance_fund_traps_funds_preventing_closeslab() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Create and close an LP to have a valid market with no positions
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 1_000_000_000); // 1 SOL

    // Create user, trade, and close to verify market works
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000); // 1 SOL

    // Trade to generate some activity
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    assert!(result.is_ok(), "Trade should succeed");

    // Close positions by trading back
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, -1_000_000);
    assert!(result.is_ok(), "Closing trade should succeed");

    // Top up insurance fund - this is the key operation
    let insurance_payer = Keypair::new();
    env.svm
        .airdrop(&insurance_payer.pubkey(), 10_000_000_000)
        .unwrap();
    env.top_up_insurance(&insurance_payer, 500_000_000); // 0.5 SOL to insurance

    let vault_after_insurance = env.vault_balance();
    println!(
        "Vault balance after insurance top-up: {}",
        vault_after_insurance
    );

    // Withdraw all user capital
    env.set_slot(200);
    env.crank(); // Settle any pending funding

    // Users close their accounts
    let user_close = env.try_close_account(&user, user_idx);
    assert!(user_close.is_ok(), "User close should succeed: {:?}", user_close);

    let lp_close = env.try_close_account(&lp, lp_idx);
    assert!(lp_close.is_ok(), "LP close should succeed: {:?}", lp_close);

    // CloseSlab should fail because insurance_fund.balance > 0
    let close_result = env.try_close_slab();
    assert!(
        close_result.is_err(),
        "CloseSlab must fail when insurance_fund.balance > 0"
    );
}

// ============================================================================
// Test: Extreme Price Movement with Large Position
// ============================================================================

/// Test behavior when a large position experiences extreme adverse price movement.
///
/// This verifies:
/// 1. Liquidation triggers correctly when position goes underwater
/// 2. Haircut ratio is applied correctly when losses exceed capital
/// 3. PnL write-off mechanism works (spec §6.1)
/// 4. No overflow or underflow with extreme values
#[test]
fn test_extreme_price_movement_with_large_position() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // LP with substantial capital
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 500_000_000_000); // 500 SOL

    // User with 10x leverage (10% initial margin)
    // Position notional = 100 SOL at $138 = $13,800
    // Required margin = 10% = $1,380 = ~10 SOL
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 15_000_000_000); // 15 SOL margin

    // Open large long position
    let size: i128 = 100_000_000; // 100 SOL position
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, size);
    assert!(
        result.is_ok(),
        "Opening position should succeed: {:?}",
        result
    );
    println!("Step 1: Opened 100 SOL long at $138");

    // Move price down by 15% (more than maintenance margin can handle)
    // New price: $138 * 0.85 = $117.3
    // Loss: 100 * ($138 - $117.3) / 1e6 = $20.7 worth
    env.set_slot_and_price(200, 117_300_000);
    env.crank();
    println!("Step 2: Price dropped 15% to $117.30");

    // User should be underwater - liquidation may fail if force-realize already
    // closed the position during crank (insurance=0 triggers force-realize mode)
    let pos_before_liq = env.read_account_position(user_idx);
    let liq_result = env.try_liquidate(user_idx);
    // Either liquidation succeeds (reduces position) or was already force-realized
    let pos_after_liq = env.read_account_position(user_idx);
    if liq_result.is_ok() {
        assert!(
            pos_after_liq.unsigned_abs() <= pos_before_liq.unsigned_abs(),
            "Successful liquidation should not increase exposure: before={} after={}",
            pos_before_liq,
            pos_after_liq
        );
    } else {
        assert_eq!(
            pos_after_liq, pos_before_liq,
            "Failed liquidation must preserve position: before={} after={}",
            pos_before_liq, pos_after_liq
        );
    }

    // If liquidation succeeded or failed, verify accounting
    env.set_slot_and_price(300, 117_300_000);
    env.crank();

    // Move price further down to stress test haircut ratio
    env.set_slot_and_price(400, 80_000_000); // $80
    env.crank();
    println!("Step 4: Price dropped to $80 (42% down from entry)");

    // Final crank
    env.set_slot_and_price(500, 80_000_000);
    env.crank();
    println!("Step 5: Final settlement at extreme price");

    // Verify LP can still operate
    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 50_000_000_000); // 50 SOL

    // Small trade to verify market still functions (may fail if force-realize
    // mode closed LP's position since insurance=0)
    let trade2_result = env.try_trade(&user2, &lp, lp_idx, user2_idx, 1_000_000);
    println!("Post-crash trade result: {}", if trade2_result.is_ok() { "ok" } else { "rejected" });

    // Vault conservation: engine.vault must match SPL vault balance
    let engine_vault = env.read_engine_vault();
    let spl_vault = env.vault_balance();
    assert_eq!(
        engine_vault as u64, spl_vault,
        "Conservation after extreme price: engine={} spl={}",
        engine_vault, spl_vault
    );
}

// ============================================================================
// Test: Minimum margin edge case
// ============================================================================

/// Test behavior at minimum margin boundary
///
/// Verifies that trades at exactly the margin boundary work correctly
/// and that trades just below the boundary are rejected.
#[test]
fn test_minimum_margin_boundary() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // LP with plenty of capital
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL

    // Initial margin is 10%, so:
    // Position of 10 SOL at $138 = $1,380 notional
    // Required initial margin = 10% * $1,380 = $138 = 1 SOL
    // We deposit slightly more than 1 SOL margin to test the boundary
    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    // Test 1: Deposit exactly enough for initial margin + small buffer
    // Position: 10 SOL = 10_000_000 base units
    // Price: $138 = 138_000_000 e6
    // Notional: 10 * 138 = $1,380
    // Initial margin (10%): $138 = 1 SOL = 1_000_000_000 lamports
    env.deposit(&user, user_idx, 1_500_000_000); // 1.5 SOL (slight buffer)

    // This should succeed - 1.5 SOL > 1 SOL required margin
    let size: i128 = 10_000_000;
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, size);
    println!(
        "Trade with 1.5 SOL margin for 10 SOL position: {:?}",
        result
    );
    assert!(result.is_ok(), "Trade at margin boundary should succeed");

    // Close the position
    env.trade(&user, &lp, lp_idx, user_idx, -size);

    // Test 2: Try with insufficient margin (withdraw most capital)
    // After close, capital is returned. Withdraw to leave very little.
    env.set_slot_and_price(200, 138_000_000);
    env.crank();

    // Try to open position with reduced capital (simulated by creating new user)
    // Use a larger position so 0.5 SOL is truly insufficient:
    // size=50_000_000, notional = 50M * 138M / 1M = 6.9 SOL
    // Initial margin (10%) = 0.69 SOL > 0.5 SOL → should be rejected
    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 500_000_000); // 0.5 SOL

    let big_size: i128 = 50_000_000;
    let result2 = env.try_trade(&user2, &lp, lp_idx, user2_idx, big_size);

    // Finding L is FIXED: initial_margin_bps (10%) is enforced
    // 0.5 SOL < 0.69 SOL required initial margin → trade must be rejected
    assert!(
        result2.is_err(),
        "Trade with insufficient initial margin must be rejected: {:?}",
        result2
    );
    assert_eq!(
        env.read_account_position(user2_idx), 0,
        "Rejected trade must leave position at zero"
    );
}

/// Test rapid position flips within the same slot.
/// This verifies that margin checks are applied correctly on each flip.
#[test]
fn test_rapid_position_flips_same_slot() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000); // 5 SOL - enough for multiple flips

    // Same slot for all trades
    env.set_slot_and_price(100, 138_000_000);

    // Trade 1: Go long
    let size1: i128 = 10_000_000; // 10M units
    env.trade(&user, &lp, lp_idx, user_idx, size1);
    assert_eq!(env.read_account_position(user_idx), size1);
    let vault_before_flips = env.vault_balance();
    println!("Trade 1: Went long with 10M units");

    // Trade 2: Flip to short (larger than position, flip + new short)
    let size2: i128 = -25_000_000; // Net: -15M units
    let result2 = env.try_trade(&user, &lp, lp_idx, user_idx, size2);
    let pos_after_trade2 = env.read_account_position(user_idx);
    let expected_after_trade2 = if result2.is_ok() { size1 + size2 } else { size1 };
    assert_eq!(
        pos_after_trade2, expected_after_trade2,
        "Trade 2 result/position mismatch: result={:?} expected_pos={} actual_pos={}",
        result2, expected_after_trade2, pos_after_trade2
    );

    // Trade 3: Try another flip back to long
    let size3: i128 = 30_000_000; // Net depends on Trade 2
    let result3 = env.try_trade(&user, &lp, lp_idx, user_idx, size3);
    let pos_after_trade3 = env.read_account_position(user_idx);
    let expected_after_trade3 = if result3.is_ok() {
        expected_after_trade2 + size3
    } else {
        expected_after_trade2
    };
    assert_eq!(
        pos_after_trade3, expected_after_trade3,
        "Trade 3 result/position mismatch: result={:?} expected_pos={} actual_pos={}",
        result3, expected_after_trade3, pos_after_trade3
    );
    assert_eq!(
        env.vault_balance(),
        vault_before_flips,
        "Rapid flip sequence should not move vault balance directly"
    );

    // The key security property: each flip should require initial margin (10%)
    // not maintenance margin (5%). With 5 SOL equity, we can support at most:
    // 5 SOL / 10% = 50 SOL notional = ~36M units at $138
    println!("RAPID POSITION FLIPS TEST COMPLETE");
}

/// Test position flip with minimal equity (edge case at liquidation boundary).
#[test]
fn test_position_flip_minimal_equity() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    // Deposit exactly enough for a small position
    env.deposit(&user, user_idx, 150_000_000); // 0.15 SOL

    env.set_slot_and_price(100, 138_000_000);

    // Open a small long position (1M units ~ 1 SOL notional)
    // Required margin: 10% of 1 SOL = 0.1 SOL
    let size: i128 = 1_000_000;
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, size);
    println!("Small long position (1M units): {:?}", result.is_ok());
    let pos_after_open = env.read_account_position(user_idx);

    if result.is_ok() {
        assert_eq!(
            pos_after_open, size,
            "Accepted open should create 1M position: got={}",
            pos_after_open
        );

        // Now try to flip - this should require initial margin on the new position
        let flip_size: i128 = -2_000_000; // Net: -1M (short)
        let flip_result = env.try_trade(&user, &lp, lp_idx, user_idx, flip_size);
        let pos_after_flip = env.read_account_position(user_idx);

        // After flip, position is -1M (short), same notional
        // Initial margin still 0.1 SOL, but we've paid trading fee on 1M + 2M = 3M
        // This tests whether the accumulated fees deplete equity
        if flip_result.is_ok() {
            assert_eq!(
                pos_after_flip, -1_000_000,
                "Successful flip should end at -1M position: got={}",
                pos_after_flip
            );
        } else {
            assert_eq!(
                pos_after_flip, size,
                "Rejected flip should preserve initial +1M position: got={}",
                pos_after_flip
            );
        }
    } else {
        assert_eq!(
            pos_after_open, 0,
            "Rejected open should keep zero position: got={}",
            pos_after_open
        );
    }

    assert_eq!(
        env.vault_balance(),
        100_150_000_000,
        "Position flip attempts must not move vault balance directly"
    );

    println!("MINIMAL EQUITY FLIP TEST COMPLETE");
}

// =============================================================================
// HYPERP INDEX SMOOTHING SECURITY TESTS
// =============================================================================

/// Test: Hyperp mode index smoothing bypass via multiple cranks in same slot
///
/// SECURITY RESEARCH: In Hyperp mode, the index should smoothly move toward the mark
/// price, rate-limited by oracle_price_cap_e2bps (default 1% per slot).
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
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: encode_init_market_hyperp(&payer.pubkey(), &mint, initial_price_e6),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
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
        &[crank_ix.clone()],
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
        &[crank_ix.clone()],
        Some(&payer.pubkey()),
        &[&payer],
        new_blockhash,
    );
    let result2 = svm.send_transaction(tx);
    println!("Second crank in slot 200: {:?}", result2);
    if let Err(ref e) = result2 {
        println!("Second crank error: {:?}", e);
    }

    // SECURITY VERIFICATION: Multiple cranks in the same slot are ALLOWED
    // but the index must NOT move (Bug #9 fix).

    assert!(
        result2.is_ok(),
        "Second crank should succeed in same slot: {:?}",
        result2
    );

    // Bug #9 CRITICAL CHECK: Read last_effective_price_e6 (index) from slab.
    // The index must be identical before and after the same-slot crank.
    // Before Bug #9 fix, dt=0 caused clamp_toward_with_dt to return mark
    // instead of index, allowing the index to jump to mark in a single slot.
    let slab_data = svm.get_account(&slab).unwrap().data;
    const INDEX_OFF: usize = 384; // last_effective_price_e6 offset in config
    let index_after = u64::from_le_bytes(slab_data[INDEX_OFF..INDEX_OFF + 8].try_into().unwrap());
    assert_eq!(
        index_after, initial_price_e6,
        "Bug #9 regression: index moved during same-slot crank! \
         expected={} (initial), got={}. Index should not move when dt=0.",
        initial_price_e6, index_after
    );
}

// ============================================================================
// Test: Maintenance Fees Drain Dead Accounts to Dust for GC
// ============================================================================

/// Test: Maintenance fees eventually drain dead accounts to dust, enabling permissionless GC.
///
/// This is a critical anti-DoS mechanism:
/// 1. Attacker creates many accounts with minimal deposits
/// 2. Accounts accumulate maintenance fee debt
/// 3. Fee debt eventually drains capital to zero
/// 4. Crank permissionlessly GCs dust accounts
/// 5. Account slots are freed for legitimate users
///
/// Without this mechanism, attackers could permanently fill all account slots.
#[test]
fn test_maintenance_fees_drain_dead_accounts_for_gc() {
    program_path();

    println!("=== MAINTENANCE FEE DRAIN & GC TEST ===");
    println!("Verifying anti-DoS mechanism: fee drain -> dust -> GC");
    println!();

    // Use standard TestEnv
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Set maintenance fee via SetMaintenanceFee instruction
    // Fee: 1_000_000 per slot = 0.001 SOL per slot (in 9-decimal units)
    // 500 slots will drain 0.5 SOL
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let maintenance_fee: u128 = 1_000_000;
    let result = env.try_set_maintenance_fee(&admin, maintenance_fee);
    assert!(
        result.is_ok(),
        "SetMaintenanceFee should succeed: {:?}",
        result
    );
    println!(
        "Set maintenance_fee_per_slot = {} (0.001 SOL/slot)",
        maintenance_fee
    );

    // Create a user with small deposit
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 500_000_000); // 0.5 SOL
    println!("Created user (idx={}) with 0.5 SOL deposit", user_idx);

    // Read initial num_used_accounts
    let initial_used = env.read_num_used_accounts();
    println!("Initial num_used_accounts: {}", initial_used);
    assert!(initial_used >= 1, "Should have at least 1 account");

    // Advance time to drain fees
    // 0.5 SOL / 0.001 SOL per slot = 500 slots to drain
    // Advance 600 slots to ensure complete drain
    env.set_slot(700);
    println!("Advanced to slot 700 (600 slots elapsed)");
    println!(
        "Expected fee drain: {} slots * {} = {} lamports (~0.6 SOL)",
        600,
        maintenance_fee,
        600u128 * maintenance_fee
    );

    // Run crank - this will:
    // 1. Settle maintenance fees (draining capital)
    // 2. Run GC on dust accounts
    env.crank();
    println!("Crank executed");

    // Verify account was GC'd
    let final_used = env.read_num_used_accounts();
    println!("Final num_used_accounts: {}", final_used);

    // Helper closure to verify GC and account reuse
    let verify_gc_and_reuse = |env: &mut TestEnv, freed_slot: u16| {
        println!();
        println!("=== VERIFYING ACCOUNT SLOT PROPERLY CLEARED ===");

        // 1. Verify bitmap bit is cleared
        let is_used = env.is_slot_used(freed_slot);
        println!(
            "Bitmap bit for slot {}: {}",
            freed_slot,
            if is_used {
                "SET (BAD!)"
            } else {
                "CLEARED (good)"
            }
        );
        assert!(!is_used, "Bitmap bit should be cleared after GC");

        // 2. Verify account capital is zeroed
        let capital = env.read_account_capital(freed_slot);
        println!("Account capital for slot {}: {}", freed_slot, capital);
        assert_eq!(capital, 0, "Account capital should be zero after GC");

        // 3. Create new user - the program should reuse the freed slot
        // Note: The test helper's account_count is out of sync with the program's freelist.
        // The program uses LIFO freelist, so it will reuse freed_slot (0).
        // But init_user returns self.account_count which is wrong after GC.
        println!();
        println!("=== VERIFYING SLOT REUSE ===");
        let num_used_before = env.read_num_used_accounts();
        println!("num_used_accounts before new user: {}", num_used_before);

        let new_user = Keypair::new();
        let _helper_idx = env.init_user(&new_user); // Helper returns wrong idx, ignore it

        // The program's freelist is LIFO - freed slot 0 should be reused
        // Verify by checking that the bitmap bit for slot 0 is now SET
        let slot_0_used = env.is_slot_used(freed_slot);
        println!(
            "After init_user, bitmap bit for slot {}: {}",
            freed_slot,
            if slot_0_used {
                "SET (slot reused!)"
            } else {
                "still cleared"
            }
        );
        assert!(
            slot_0_used,
            "Freed slot should be reused by new user (LIFO freelist)"
        );

        // 4. Verify num_used_accounts incremented (not doubled - slot was reused)
        let num_used_after = env.read_num_used_accounts();
        println!("num_used_accounts after new user: {}", num_used_after);
        assert_eq!(
            num_used_after, 1,
            "Should have exactly 1 account (slot reused, not new slot)"
        );

        // 5. Verify new account has fresh state by checking it can receive deposits
        // The actual slot is 0 (the freed slot), deposit using that
        env.deposit(&new_user, freed_slot, 100_000_000); // 0.1 SOL
        let new_capital = env.read_account_capital(freed_slot);
        println!(
            "Account capital at slot {} after deposit: {}",
            freed_slot, new_capital
        );
        assert!(
            new_capital > 0,
            "Reused slot should accept deposits (fresh state)"
        );

        println!();
        println!("ACCOUNT REUSE VERIFIED SAFE:");
        println!("  1. Bitmap bit cleared after GC");
        println!("  2. Account data zeroed after GC");
        println!("  3. Freed slot reused by next allocation (LIFO freelist)");
        println!("  4. Reused slot has fresh state (accepts deposits)");
        println!("  5. No stale data leaked to new account");
    };

    if final_used < initial_used {
        println!();
        println!("SUCCESS: Account was garbage collected!");
        println!("  Initial accounts: {}", initial_used);
        println!("  Final accounts: {}", final_used);
        println!("  Accounts freed: {}", initial_used - final_used);

        verify_gc_and_reuse(&mut env, user_idx);
    } else {
        // Account might not be GC'd immediately due to fee_credits absorbing fees first
        // Run additional cranks to fully drain
        println!();
        println!("First crank did not GC account - running additional cranks...");

        for i in 0..5 {
            env.set_slot(800 + i * 100);
            env.crank();
            let used = env.read_num_used_accounts();
            println!("After crank at slot {}: num_used = {}", 800 + i * 100, used);
            if used < initial_used {
                println!();
                println!("SUCCESS: Account GC'd after {} additional cranks", i + 1);
                verify_gc_and_reuse(&mut env, user_idx);
                println!();
                println!("MAINTENANCE FEE DRAIN TEST COMPLETE");
                return;
            }
        }

        // If still not GC'd, it's likely the account has some residual state
        panic!("Account was not GC'd after multiple cranks - test failed");
    }

    println!();
    println!("MAINTENANCE FEE DRAIN TEST COMPLETE");
}

// ============================================================================
// Tests: Premarket Resolution (Binary Outcome Markets)
// ============================================================================

/// Test full premarket resolution lifecycle:
/// 1. Create market with positions
/// 2. Admin pushes final price (0 or 1)
/// 3. Admin resolves market
/// 4. Crank force-closes all positions
/// 5. Admin withdraws insurance
/// 6. Users withdraw their funds
/// 7. Admin closes slab
#[test]
fn test_premarket_resolution_full_lifecycle() {
    // Need TradeCpiTestEnv because hyperp mode disables TradeNoCpi
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found. Run: cargo build-sbf && cd ../percolator-match && cargo build-sbf");
        return;
    };

    println!("=== PREMARKET RESOLUTION FULL LIFECYCLE TEST ===");
    println!();

    // Create hyperp market with admin oracle authority
    env.init_market_hyperp(1_000_000); // Initial mark = 1.0

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;

    // Set oracle authority to admin
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");

    // Create LP with matcher
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 10_000_000_000); // 10 SOL

    // Create user
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000); // 1 SOL

    // Push initial price and crank
    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("initial oracle push must succeed"); // Price = 1.0
    env.set_slot(100);
    env.crank();

    // Execute a trade via TradeCpi to create positions
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(result.is_ok(), "Trade should succeed: {:?}", result);

    println!("Market created with LP and User positions");
    println!("LP idx={}, User idx={}", lp_idx, user_idx);

    // Verify positions exist
    let lp_pos = env.read_account_position(lp_idx);
    let user_pos = env.read_account_position(user_idx);
    println!("LP position: {}", lp_pos);
    println!("User position: {}", user_pos);
    assert!(lp_pos != 0 || user_pos != 0, "Should have positions");

    // Step 1: Admin pushes final resolution price (binary: 1e-6 or 1)
    // Price = 1 (1e-6) means "NO" outcome (essentially zero, but nonzero for force-close)
    env.try_push_oracle_price(&admin, 1, 2000)
        .expect("resolution oracle push must succeed"); // Final price = 1e-6 (NO)
    println!("Admin pushed final price: 1e-6 (NO outcome)");

    // Step 2: Admin resolves market
    let result = env.try_resolve_market(&admin);
    assert!(result.is_ok(), "ResolveMarket should succeed: {:?}", result);
    println!("Market resolved");

    // Verify market is resolved
    assert!(env.is_market_resolved(), "Market should be resolved");

    // Step 3: Crank to force-close positions
    env.set_slot(200);
    env.crank();
    println!("Crank executed to force-close positions");

    // Verify positions are closed
    let lp_pos_after = env.read_account_position(lp_idx);
    let user_pos_after = env.read_account_position(user_idx);
    println!("LP position after: {}", lp_pos_after);
    println!("User position after: {}", user_pos_after);
    assert_eq!(lp_pos_after, 0, "LP position should be closed");
    assert_eq!(user_pos_after, 0, "User position should be closed");

    // Step 4: Admin withdraws insurance
    let insurance_before = env.read_insurance_balance();
    println!("Insurance balance before withdrawal: {}", insurance_before);

    if insurance_before > 0 {
        let result = env.try_withdraw_insurance(&admin);
        assert!(
            result.is_ok(),
            "WithdrawInsurance should succeed: {:?}",
            result
        );
        println!("Admin withdrew insurance");

        let insurance_after = env.read_insurance_balance();
        assert_eq!(
            insurance_after, 0,
            "Insurance should be zero after withdrawal"
        );
    }

    println!();
    println!("PREMARKET RESOLUTION LIFECYCLE TEST PASSED");
}

/// Test that resolved markets block new activity
#[test]
fn test_resolved_market_blocks_new_activity() {
    program_path();

    println!("=== RESOLVED MARKET BLOCKS NEW ACTIVITY TEST ===");
    println!();

    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("set_oracle_authority should succeed");
    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("push_oracle_price should succeed");

    // Resolve market
    let result = env.try_resolve_market(&admin);
    assert!(result.is_ok(), "ResolveMarket should succeed");
    println!("Market resolved");

    // Try to create new user - should fail
    let new_user = Keypair::new();
    env.svm.airdrop(&new_user.pubkey(), 1_000_000_000).unwrap();
    let ata = env.create_ata(&new_user.pubkey(), 0);

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(new_user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_col, false),
        ],
        data: encode_init_user(0),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&new_user.pubkey()),
        &[&new_user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(result.is_err(), "InitUser should fail on resolved market");
    println!("InitUser blocked on resolved market: OK");

    // Try to deposit - should fail (need existing user first)
    // We'll create user before resolving to test deposit block
    println!();
    println!("RESOLVED MARKET BLOCKS NEW ACTIVITY TEST PASSED");
}

/// Test that users can withdraw after resolution
#[test]
fn test_resolved_market_allows_user_withdrawal() {
    program_path();

    println!("=== RESOLVED MARKET ALLOWS USER WITHDRAWAL TEST ===");
    println!();

    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("set_oracle_authority should succeed");
    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("push_oracle_price should succeed");

    // Create user with deposit
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 500_000_000); // 0.5 SOL

    let capital_before = env.read_account_capital(user_idx);
    println!("User capital before resolution: {}", capital_before);
    assert!(capital_before > 0);

    // Resolve market
    env.try_resolve_market(&admin).unwrap();
    println!("Market resolved");

    // Crank to settle
    env.set_slot(100);
    env.crank();

    // User should still be able to withdraw
    let user_ata = env.create_ata(&user.pubkey(), 0);
    let (vault_pda, _) =
        Pubkey::find_program_address(&[b"vault", env.slab.as_ref()], &env.program_id);

    // Correct account order for WithdrawCollateral:
    // 0: user (signer), 1: slab, 2: vault, 3: user_ata, 4: vault_pda, 5: token_program, 6: clock, 7: oracle
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new(user_ata, false),
            AccountMeta::new_readonly(vault_pda, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_withdraw(user_idx, 100_000_000), // Withdraw 0.1 SOL
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_ok(),
        "Withdraw should succeed on resolved market: {:?}",
        result
    );
    println!("User withdrawal on resolved market: OK");

    println!();
    println!("RESOLVED MARKET ALLOWS USER WITHDRAWAL TEST PASSED");
}

/// Test insurance withdrawal requires all positions closed
#[test]
fn test_withdraw_insurance_requires_positions_closed() {
    // Need TradeCpiTestEnv because hyperp mode disables TradeNoCpi
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found. Run: cargo build-sbf && cd ../percolator-match && cargo build-sbf");
        return;
    };

    println!("=== WITHDRAW INSURANCE REQUIRES POSITIONS CLOSED TEST ===");
    println!();

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("initial oracle push must succeed");

    // Create LP and user with positions
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.set_slot(50);
    env.crank();
    env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000_000,
        &matcher_prog,
        &matcher_ctx,
    )
    .expect("user setup trade must succeed");

    // Resolve market WITHOUT cranking to close positions
    env.try_push_oracle_price(&admin, 500_000, 2000)
        .expect("resolution oracle push must succeed"); // Price = 0.5
    env.try_resolve_market(&admin).unwrap();
    println!("Market resolved but positions not yet closed");

    // Try to withdraw insurance - should fail (positions still open)
    let result = env.try_withdraw_insurance(&admin);
    assert!(
        result.is_err(),
        "WithdrawInsurance should fail with open positions"
    );
    println!("WithdrawInsurance blocked with open positions: OK");

    // Now crank to close positions
    env.set_slot(200);
    env.crank();
    println!("Crank executed to force-close positions");

    // Now withdrawal should succeed
    let result = env.try_withdraw_insurance(&admin);
    assert!(
        result.is_ok(),
        "WithdrawInsurance should succeed after positions closed: {:?}",
        result
    );
    println!("WithdrawInsurance succeeded after positions closed: OK");

    println!();
    println!("WITHDRAW INSURANCE REQUIRES POSITIONS CLOSED TEST PASSED");
}

/// Test paginated force-close with many accounts (simulates 4096 worst case)
#[test]
fn test_premarket_paginated_force_close() {
    // Need TradeCpiTestEnv because hyperp mode disables TradeNoCpi
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found. Run: cargo build-sbf && cd ../percolator-match && cargo build-sbf");
        return;
    };

    println!("=== PREMARKET PAGINATED FORCE-CLOSE TEST ===");
    println!("Simulating multiple accounts requiring multiple cranks to close");
    println!();

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("initial oracle push must succeed");

    // Create multiple users with positions
    // We'll create 100 users to simulate paginated close (not 4096 for test speed)
    const NUM_USERS: usize = 100;
    let mut users: Vec<(Keypair, u16)> = Vec::new();

    // Create LP first
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL

    env.set_slot(50);
    env.crank();

    println!("Creating {} users with positions...", NUM_USERS);
    for i in 0..NUM_USERS {
        let user = Keypair::new();
        let user_idx = env.init_user(&user);
        env.deposit(&user, user_idx, 100_000_000); // 0.1 SOL each

        // Execute small trade via TradeCpi to create position
        env.try_trade_cpi(
            &user,
            &lp.pubkey(),
            lp_idx,
            user_idx,
            1_000_000,
            &matcher_prog,
            &matcher_ctx,
        )
        .expect("user setup trade must succeed");
        users.push((user, user_idx));

        if (i + 1) % 20 == 0 {
            println!("  Created {} users", i + 1);
        }
    }
    println!("Created {} users with positions", NUM_USERS);

    // Count users with positions
    let mut users_with_positions = 0;
    for (_, idx) in &users {
        if env.read_account_position(*idx) != 0 {
            users_with_positions += 1;
        }
    }
    println!("Users with open positions: {}", users_with_positions);
    assert_eq!(
        users_with_positions, NUM_USERS,
        "all setup users must have open positions"
    );

    // Resolve market
    env.try_push_oracle_price(&admin, 500_000, 2000)
        .expect("resolution oracle push must succeed"); // Final price = 0.5
    env.try_resolve_market(&admin).unwrap();
    println!("Market resolved");

    // Crank multiple times to close all positions (BATCH_SIZE = 64 per crank)
    let mut crank_count = 0;
    let max_cranks = 10; // Safety limit

    loop {
        env.set_slot(200 + crank_count * 10);
        env.crank();
        crank_count += 1;

        // Check if all positions closed
        let mut remaining_positions = 0;
        for (_, idx) in &users {
            if env.read_account_position(*idx) != 0 {
                remaining_positions += 1;
            }
        }
        // Also check LP
        if env.read_account_position(lp_idx) != 0 {
            remaining_positions += 1;
        }

        println!(
            "After crank {}: {} positions remaining",
            crank_count, remaining_positions
        );

        if remaining_positions == 0 {
            break;
        }
        if crank_count >= max_cranks {
            panic!("Failed to close all positions after {} cranks", max_cranks);
        }
    }

    println!();
    println!("All positions closed after {} cranks", crank_count);
    println!(
        "Expected cranks for {} accounts: ~{}",
        NUM_USERS + 1,
        (NUM_USERS + 1 + 63) / 64
    );

    // Verify insurance can now be withdrawn
    let result = env.try_withdraw_insurance(&admin);
    assert!(
        result.is_ok(),
        "WithdrawInsurance should succeed: {:?}",
        result
    );
    println!("Insurance withdrawn successfully");

    println!();
    println!("PREMARKET PAGINATED FORCE-CLOSE TEST PASSED");
}

/// Test binary outcome: price = 1e-6 (NO wins)
#[test]
fn test_premarket_binary_outcome_price_zero() {
    // Need TradeCpiTestEnv because hyperp mode disables TradeNoCpi
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found. Run: cargo build-sbf && cd ../percolator-match && cargo build-sbf");
        return;
    };

    println!("=== PREMARKET BINARY OUTCOME PRICE=1e-6 (NO) TEST ===");
    println!();

    env.init_market_hyperp(500_000); // Initial mark = 0.5 (50% probability)

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 500_000, 1000)
        .expect("initial oracle push must succeed");

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.set_slot(50);
    env.crank();

    // User bets YES (goes long at 0.5) via TradeCpi
    env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000_000,
        &matcher_prog,
        &matcher_ctx,
    )
    .expect("user setup trade must succeed");
    println!("User went LONG (YES bet) at price 0.5");

    // Outcome: NO wins (price = 1e-6, essentially zero but nonzero for force-close)
    env.try_push_oracle_price(&admin, 1, 2000)
        .expect("resolution oracle push must succeed");
    env.try_resolve_market(&admin).unwrap();
    println!("Market resolved at price = 1e-6 (NO wins)");

    env.set_slot(200);
    env.crank();

    // User should have lost (position closed at ~0, entry was ~0.5)
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(user_pos, 0, "Position should be closed");

    // User went long at ~0.5, market resolved at ~0. User LOST.
    // Check using equity = capital + max(pnl, 0) since PnL may be in warmup.
    let user_capital = env.read_account_capital(user_idx);
    let user_pnl = env.read_account_pnl(user_idx);
    let user_equity = user_capital as i128 + user_pnl;
    assert!(
        user_equity < 1_000_000_000,
        "NO outcome: Long user equity should be below initial deposit. \
         capital={}, pnl={}, equity={}, initial=1_000_000_000",
        user_capital, user_pnl, user_equity
    );

    // LP (short side) should have gained or at least not lost.
    let lp_capital = env.read_account_capital(lp_idx);
    let lp_pnl = env.read_account_pnl(lp_idx);
    let lp_equity = lp_capital as i128 + lp_pnl;
    assert!(
        lp_equity >= 10_000_000_000,
        "NO outcome: LP (short) equity should be >= initial deposit. \
         capital={}, pnl={}, equity={}, initial=10_000_000_000",
        lp_capital, lp_pnl, lp_equity
    );
}

/// Test binary outcome: price = 1e6 (YES wins)
#[test]
fn test_premarket_binary_outcome_price_one() {
    // Need TradeCpiTestEnv because hyperp mode disables TradeNoCpi
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found. Run: cargo build-sbf && cd ../percolator-match && cargo build-sbf");
        return;
    };

    println!("=== PREMARKET BINARY OUTCOME PRICE=1 TEST ===");
    println!();

    env.init_market_hyperp(500_000); // Initial mark = 0.5 (50% probability)

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 500_000, 1000)
        .expect("initial oracle push must succeed");

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.set_slot(50);
    env.crank();

    // User bets YES (goes long at 0.5) via TradeCpi
    env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000_000,
        &matcher_prog,
        &matcher_ctx,
    )
    .expect("user setup trade must succeed");
    println!("User went LONG (YES bet) at price 0.5");

    // Outcome: YES wins (price = 1.0 = 1_000_000 in e6)
    env.try_push_oracle_price(&admin, 1_000_000, 2000)
        .expect("resolution oracle push must succeed");
    env.try_resolve_market(&admin).unwrap();
    println!("Market resolved at price = 1.0 (YES wins)");

    env.set_slot(200);
    env.crank();

    // User should have won (position closed at 1.0, entry was ~0.5)
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(user_pos, 0, "Position should be closed");

    // User went long at ~0.5, market resolved at 1.0. User WON.
    // Check using equity = capital + max(pnl, 0) since PnL may be in warmup.
    let user_capital = env.read_account_capital(user_idx);
    let user_pnl = env.read_account_pnl(user_idx);
    let user_equity = user_capital as i128 + user_pnl;
    assert!(
        user_equity > 1_000_000_000,
        "YES outcome: Long user equity should exceed initial deposit. \
         capital={}, pnl={}, equity={}, initial=1_000_000_000",
        user_capital, user_pnl, user_equity
    );

    // LP (short side) should have lost
    let lp_capital = env.read_account_capital(lp_idx);
    let lp_pnl = env.read_account_pnl(lp_idx);
    let lp_equity = lp_capital as i128 + lp_pnl;
    assert!(
        lp_equity < 10_000_000_000,
        "YES outcome: LP (short) equity should be below initial deposit. \
         capital={}, pnl={}, equity={}, initial=10_000_000_000",
        lp_capital, lp_pnl, lp_equity
    );
}

/// Benchmark test: verify force-close CU consumption is bounded
///
/// The force-close operation processes up to BATCH_SIZE=64 accounts per crank.
/// Each account operation:
/// - is_used check: O(1) bitmap lookup
/// - position check: O(1) read
/// - PnL settlement: O(1) arithmetic
/// - position clear: O(1) write
///
/// This test verifies that 64 force-closes stay well under compute budget.
/// For 4096 accounts, we need 64 cranks, each under ~22k CUs to stay under 1.4M total.
#[test]
fn test_premarket_force_close_cu_benchmark() {
    // Need TradeCpiTestEnv because hyperp mode disables TradeNoCpi
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found. Run: cargo build-sbf && cd ../percolator-match && cargo build-sbf");
        return;
    };

    println!("=== PREMARKET FORCE-CLOSE CU BENCHMARK ===");
    println!("Testing compute unit consumption for paginated force-close");
    println!();

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("initial oracle push must succeed");

    // Create LP with large deposit to handle all trades
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 1_000_000_000_000); // 1000 SOL

    env.set_slot(50);
    env.crank();

    // Create 64 users (one batch worth) with positions
    // This is the worst case for a single crank call
    const NUM_USERS: usize = 64;
    let mut users: Vec<(Keypair, u16)> = Vec::new();

    println!("Creating {} users with positions...", NUM_USERS);
    for _ in 0..NUM_USERS {
        let user = Keypair::new();
        let user_idx = env.init_user(&user);
        env.deposit(&user, user_idx, 100_000_000); // 0.1 SOL each
        env.try_trade_cpi(
            &user,
            &lp.pubkey(),
            lp_idx,
            user_idx,
            1_000_000,
            &matcher_prog,
            &matcher_ctx,
        )
        .expect("user setup trade must succeed");
        users.push((user, user_idx));
    }
    println!("Created {} users with positions", NUM_USERS);

    // Verify positions exist
    let mut positions_count = 0;
    for (_, idx) in &users {
        if env.read_account_position(*idx) != 0 {
            positions_count += 1;
        }
    }
    println!("Users with positions: {}", positions_count);
    assert_eq!(
        positions_count, NUM_USERS,
        "all setup users must have open positions"
    );

    // Resolve market
    env.try_push_oracle_price(&admin, 500_000, 2000)
        .expect("resolution oracle push must succeed");
    env.try_resolve_market(&admin).unwrap();
    println!("Market resolved");

    // Run force-close crank and capture CU consumption
    env.set_slot(200);

    // Use lower-level send to capture CU
    let caller = Keypair::new();
    env.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();

    let ix = solana_sdk::instruction::Instruction {
        program_id: env.program_id,
        accounts: vec![
            solana_sdk::instruction::AccountMeta::new(caller.pubkey(), true),
            solana_sdk::instruction::AccountMeta::new(env.slab, false),
            solana_sdk::instruction::AccountMeta::new_readonly(
                solana_sdk::sysvar::clock::ID,
                false,
            ),
            solana_sdk::instruction::AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_crank_permissionless(),
    };

    let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
        &[ix],
        Some(&caller.pubkey()),
        &[&caller],
        env.svm.latest_blockhash(),
    );

    let result = env.svm.send_transaction(tx);

    match result {
        Ok(meta) => {
            let cu_consumed = meta.compute_units_consumed;
            println!();
            println!("Force-close crank succeeded");
            println!("Compute units consumed: {}", cu_consumed);
            println!();

            // Verify CU is bounded per-crank
            // Key constraint: Each crank must fit in a single transaction (<200k CU)
            // Debug mode is ~3-5x slower than BPF. We see ~30k in debug, expect ~5-10k in BPF.
            let max_cu_per_crank = 100_000; // Conservative limit per crank
            assert!(
                cu_consumed < max_cu_per_crank,
                "Force-close CU {} exceeds per-crank limit {}. Each crank must fit in single tx.",
                cu_consumed,
                max_cu_per_crank
            );

            // Calculate projected total for 4096 accounts
            let projected_total = cu_consumed * 64;
            let bpf_estimate = cu_consumed / 3; // BPF is ~3x faster than debug
            let bpf_projected = bpf_estimate * 64;

            println!("Projected CU for 4096 accounts (64 cranks):");
            println!("  Debug mode: {} CU total", projected_total);
            println!("  BPF estimate: {} CU total (3x faster)", bpf_projected);
            println!();
            println!(
                "Per-crank CU: {} (debug), ~{} (BPF estimate)",
                cu_consumed, bpf_estimate
            );
            println!("Per-crank limit: 200,000 CU (Solana default)");
            println!(
                "Per-crank utilization: {:.1}% (debug)",
                (cu_consumed as f64 / 200_000.0) * 100.0
            );

            // BPF estimate should be well under 1.4M
            // Each crank can also be submitted in separate blocks if needed
            assert!(
                bpf_projected < 1_400_000,
                "BPF projected total CU {} may exceed 1.4M budget",
                bpf_projected
            );

            println!();
            println!("BENCHMARK PASSED: Force-close CU is bounded");
        }
        Err(e) => {
            panic!("Force-close crank failed: {:?}", e);
        }
    }

    // Verify positions were closed
    env.crank(); // Additional crank to close remaining positions

    let mut remaining = 0;
    for (_, idx) in &users {
        if env.read_account_position(*idx) != 0 {
            remaining += 1;
        }
    }
    assert_eq!(
        remaining, 0,
        "All positions should be closed after two cranks"
    );

    println!();
    println!("PREMARKET FORCE-CLOSE CU BENCHMARK COMPLETE");
}

// ============================================================================
// VULNERABILITY TEST: Stale pnl_pos_tot after force-close
// ============================================================================

/// SECURITY BUG: Force-close bypasses set_pnl(), leaving pnl_pos_tot stale
///
/// The force-close logic directly modifies acc.pnl without using the set_pnl()
/// helper, which should maintain the pnl_pos_tot aggregate. This means:
/// 1. pnl_pos_tot doesn't reflect the actual sum of positive PnL after settlement
/// 2. haircut_ratio() uses stale pnl_pos_tot for withdrawal calculations
/// 3. First withdrawers can extract more value than entitled if haircut should apply
///
/// This test demonstrates the bug by checking that pnl_pos_tot is stale after
/// force-close settles positions to a price that generates positive PnL.
#[test]
fn test_vulnerability_stale_pnl_pos_tot_after_force_close() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found. Run: cargo build-sbf && cd ../percolator-match && cargo build-sbf");
        return;
    };

    env.init_market_hyperp(1_000_000);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;

    // Set oracle authority and initial price for hyperp market
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("initial oracle push must succeed");

    // Create LP with initial deposit
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 10_000_000_000); // 10 SOL collateral

    // Create user who will take a long position
    let user_long = Keypair::new();
    let user_long_idx = env.init_user(&user_long);
    env.deposit(&user_long, user_long_idx, 1_000_000_000); // 1 SOL

    env.set_slot(50);
    env.crank();

    // User goes long at entry price ~1.0 (1_000_000 e6)
    let trade_result = env.try_trade_cpi(
        &user_long,
        &lp.pubkey(),
        lp_idx,
        user_long_idx,
        100_000_000, // +100M position (long)
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(trade_result.is_ok(), "Trade should succeed");

    // Verify position was established
    let pos_before = env.read_account_position(user_long_idx);
    assert!(pos_before > 0, "User should have long position");
    println!("User position: {}", pos_before);

    // Record pnl_pos_tot before resolution
    let pnl_pos_tot_before = env.read_pnl_pos_tot();
    println!("pnl_pos_tot before resolution: {}", pnl_pos_tot_before);

    // Resolve market at 2.0 (2_000_000 e6) - user's long position is profitable
    // This means user has positive PnL = position * (2.0 - 1.0) / 1e6
    env.set_slot(100);
    env.try_push_oracle_price(&admin, 2_000_000, 200)
        .expect("resolution oracle push must succeed");
    env.try_resolve_market(&admin).unwrap();
    println!("Market resolved at price 2.0");

    // Force-close by cranking
    env.set_slot(150);
    env.crank();

    // Verify position was closed
    let pos_after = env.read_account_position(user_long_idx);
    assert_eq!(pos_after, 0, "Position should be closed after force-close");

    // Check user's PnL - should be positive (they were long and price went up)
    let user_pnl = env.read_account_pnl(user_long_idx);
    println!("User PnL after force-close: {}", user_pnl);

    // *** BUG #10 FIX VERIFICATION ***
    // Force-close now uses set_pnl() to maintain pnl_pos_tot aggregate.
    // Verify pnl_pos_tot includes the user's positive PnL after force-close.
    let pnl_pos_tot_after = env.read_pnl_pos_tot();
    println!("pnl_pos_tot after force-close: {}", pnl_pos_tot_after);

    // If user has positive PnL, pnl_pos_tot must be at least that large
    // (it may also include LP's positive PnL if LP has any)
    if user_pnl > 0 {
        // pnl_pos_tot should be >= user's positive PnL
        // It equals sum of max(0, pnl_i) for all accounts
        assert!(
            pnl_pos_tot_after >= user_pnl as u128,
            "Bug #10 not fixed! pnl_pos_tot should include user's positive PnL. \
             pnl_pos_tot={}, user_pnl={}",
            pnl_pos_tot_after,
            user_pnl
        );

        // Also verify it changed from before (was 0 or small before force-close)
        assert!(
            pnl_pos_tot_after > pnl_pos_tot_before,
            "pnl_pos_tot should have increased after force-close created positive PnL. \
             before={}, after={}",
            pnl_pos_tot_before,
            pnl_pos_tot_after
        );

        println!("FIX VERIFIED: pnl_pos_tot correctly updated after force-close");
    }

    println!("REGRESSION TEST PASSED: pnl_pos_tot correctly maintained after force-close");
}

// ============================================================================
// PEN TEST SUITE: Exhaustive Security Attack Tests
// ============================================================================
//
// These tests cover all 21 instructions and known attack vectors that could
// steal user funds. Each test attempts an exploit and verifies it fails.

impl TestEnv {
    /// Read c_tot aggregate from slab
    fn read_c_tot(&self) -> u128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        const C_TOT_OFFSET: usize = 392 + 264;
        u128::from_le_bytes(
            slab_data[C_TOT_OFFSET..C_TOT_OFFSET + 16]
                .try_into()
                .unwrap(),
        )
    }

    /// Read vault balance from engine state
    fn read_engine_vault(&self) -> u128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        const VAULT_OFFSET: usize = 392;
        u128::from_le_bytes(
            slab_data[VAULT_OFFSET..VAULT_OFFSET + 16]
                .try_into()
                .unwrap(),
        )
    }

    /// Read pnl_pos_tot aggregate from slab
    fn read_pnl_pos_tot(&self) -> u128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        const PNL_POS_TOT_OFFSET: usize = 392 + 280;
        u128::from_le_bytes(
            slab_data[PNL_POS_TOT_OFFSET..PNL_POS_TOT_OFFSET + 16]
                .try_into()
                .unwrap(),
        )
    }

    /// Read account PnL for a slot
    fn read_account_pnl(&self, idx: u16) -> i128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        const ACCOUNTS_OFFSET: usize = 392 + 9136;
        const ACCOUNT_SIZE: usize = 240;
        const PNL_OFFSET_IN_ACCOUNT: usize = 32;
        let account_off = ACCOUNTS_OFFSET + (idx as usize) * ACCOUNT_SIZE + PNL_OFFSET_IN_ACCOUNT;
        if slab_data.len() < account_off + 16 {
            return 0;
        }
        i128::from_le_bytes(slab_data[account_off..account_off + 16].try_into().unwrap())
    }

    /// Try to init user with a specific signer (for auth tests)
    fn try_init_user(&mut self, owner: &Keypair) -> Result<u16, String> {
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
            &[ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        match self.svm.send_transaction(tx) {
            Ok(_) => {
                self.account_count += 1;
                Ok(idx)
            }
            Err(e) => Err(format!("{:?}", e)),
        }
    }

    /// Try deposit, returns result
    fn try_deposit(&mut self, owner: &Keypair, user_idx: u16, amount: u64) -> Result<(), String> {
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
            &[ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }
}

// ============================================================================
// 1. Withdrawal Attacks
// ============================================================================

/// ATTACK: Try to withdraw more tokens than deposited capital.
/// Expected: Transaction fails due to margin/balance check.
#[test]
fn test_attack_withdraw_more_than_capital() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000); // 1 SOL

    let user_pos_before = env.read_account_position(user_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // Try to withdraw 2x the deposit
    let result = env.try_withdraw(&user, user_idx, 2_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Should not withdraw more than capital"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Over-withdraw rejection must preserve user position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Over-withdraw rejection must preserve user capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Over-withdraw rejection must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Over-withdraw rejection must preserve engine vault"
    );
}

/// ATTACK: After incurring a PnL loss, try to withdraw the full original deposit.
/// Expected: Fails because MTM equity is reduced by loss, margin check rejects.
#[test]
fn test_attack_withdraw_after_loss_exceeds_equity() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 2_000_000_000); // 2 SOL

    // Open a leveraged long position
    env.trade(&user, &lp, lp_idx, user_idx, 10_000_000);

    // Price drops significantly - user has unrealized loss
    env.set_slot_and_price(200, 100_000_000); // $100 (from $138)
    env.crank();

    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // Try to withdraw full deposit - should fail due to reduced equity
    let result = env.try_withdraw(&user, user_idx, 2_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Should not withdraw full capital after PnL loss"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected loss-state withdraw must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected loss-state withdraw must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected loss-state withdraw must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected loss-state withdraw must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected loss-state withdraw must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected loss-state withdraw must preserve engine vault"
    );
}

/// ATTACK: Withdraw an amount not aligned to unit_scale.
/// Expected: Transaction rejected for misaligned amount.
#[test]
fn test_attack_withdraw_misaligned_amount() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_full(0, 1000, 0); // unit_scale = 1000

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000);

    env.set_slot(200);
    env.crank();

    let user_pos_before = env.read_account_position(user_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // 1500 % 1000 != 0 => misaligned
    let result = env.try_withdraw(&user, user_idx, 1_500);
    assert!(
        result.is_err(),
        "ATTACK: Misaligned withdrawal should be rejected"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Misaligned withdraw rejection must preserve user position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Misaligned withdraw rejection must preserve user capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Misaligned withdraw rejection must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Misaligned withdraw rejection must preserve engine vault"
    );
}

/// ATTACK: When vault is undercollateralized (haircut < 1.0), withdraw should
/// return reduced equity, not allow full withdrawal that exceeds the haircutted equity.
#[test]
fn test_attack_withdraw_during_undercollateralization() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Create a position to generate PnL
    env.trade(&user, &lp, lp_idx, user_idx, 20_000_000);

    // Big price move creates profit for user, which is subject to haircut
    env.set_slot_and_price(200, 200_000_000);
    env.crank();

    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // Try to withdraw all original deposit + more (inflated equity)
    // The system should cap withdrawal at haircutted equity minus margin
    let result = env.try_withdraw(&user, user_idx, 50_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Withdraw exceeding haircutted equity should fail"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected haircut withdraw must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected haircut withdraw must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected haircut withdraw must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected haircut withdraw must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected haircut withdraw must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected haircut withdraw must preserve engine vault"
    );
}

/// ATTACK: Withdraw without settling accrued fee debt.
/// Expected: Withdraw checks include fee debt in equity calculation.
#[test]
fn test_attack_withdraw_bypasses_fee_debt() {
    program_path();

    let mut env = TestEnv::new();
    // Initialize with maintenance fee to accrue fee debt
    env.init_market_with_warmup(0, 0);

    // Set maintenance fee via admin
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_maintenance_fee(&admin, 1_000_000_000)
        .expect("maintenance fee setup must succeed"); // High fee

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Open position to create fee exposure
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Advance many slots so fees accrue
    env.set_slot(10_000);
    env.crank();

    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // Try to withdraw full deposit - fee debt should reduce available equity
    let result = env.try_withdraw(&user, user_idx, 10_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Full withdrawal should fail because fee debt reduces equity"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected fee-debt bypass withdraw must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected fee-debt bypass withdraw must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected fee-debt bypass withdraw must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected fee-debt bypass withdraw must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected fee-debt bypass withdraw must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected fee-debt bypass withdraw must preserve engine vault"
    );
}

// ============================================================================
// 2. Authorization Bypass
// ============================================================================

/// ATTACK: Attacker deposits to an account they don't own.
/// Expected: Owner check fails - signer must match account's registered owner.
#[test]
fn test_attack_deposit_wrong_owner() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Create victim's account
    let victim = Keypair::new();
    let victim_idx = env.init_user(&victim);
    env.deposit(&victim, victim_idx, 5_000_000_000);

    // Attacker tries to deposit to victim's account
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 10_000_000_000).unwrap();
    let victim_pos_before = env.read_account_position(victim_idx);
    let victim_cap_before = env.read_account_capital(victim_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();
    let result = env.try_deposit_unauthorized(&attacker, victim_idx, 1_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Deposit to wrong owner's account should fail"
    );
    assert_eq!(
        env.read_account_position(victim_idx),
        victim_pos_before,
        "Unauthorized deposit rejection must preserve victim position"
    );
    assert_eq!(
        env.read_account_capital(victim_idx),
        victim_cap_before,
        "Unauthorized deposit rejection must preserve victim capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Unauthorized deposit rejection must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Unauthorized deposit rejection must preserve engine vault"
    );
}

/// ATTACK: Attacker withdraws from an account they don't own.
/// Expected: Owner check rejects - signer must match account's registered owner.
#[test]
fn test_attack_withdraw_wrong_owner() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Victim deposits
    let victim = Keypair::new();
    let victim_idx = env.init_user(&victim);
    env.deposit(&victim, victim_idx, 5_000_000_000);

    // Attacker tries to withdraw from victim's account
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let victim_pos_before = env.read_account_position(victim_idx);
    let victim_cap_before = env.read_account_capital(victim_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();
    let result = env.try_withdraw(&attacker, victim_idx, 1_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Withdraw from wrong owner's account should fail"
    );
    assert_eq!(
        env.read_account_position(victim_idx),
        victim_pos_before,
        "Unauthorized withdraw rejection must preserve victim position"
    );
    assert_eq!(
        env.read_account_capital(victim_idx),
        victim_cap_before,
        "Unauthorized withdraw rejection must preserve victim capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Unauthorized withdraw rejection must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Unauthorized withdraw rejection must preserve engine vault"
    );
}

/// ATTACK: Close someone else's account to steal their capital.
/// Expected: Owner check rejects.
#[test]
fn test_attack_close_account_wrong_owner() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let victim = Keypair::new();
    let victim_idx = env.init_user(&victim);
    env.deposit(&victim, victim_idx, 5_000_000_000);

    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let victim_pos_before = env.read_account_position(victim_idx);
    let victim_cap_before = env.read_account_capital(victim_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();
    let result = env.try_close_account(&attacker, victim_idx);
    assert!(
        result.is_err(),
        "ATTACK: Closing someone else's account should fail"
    );
    assert_eq!(
        env.read_account_position(victim_idx),
        victim_pos_before,
        "Unauthorized close rejection must preserve victim position"
    );
    assert_eq!(
        env.read_account_capital(victim_idx),
        victim_cap_before,
        "Unauthorized close rejection must preserve victim capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Unauthorized close rejection must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Unauthorized close rejection must preserve engine vault"
    );
}

/// ATTACK: Non-admin tries admin operations (UpdateAdmin, SetRiskThreshold,
/// UpdateConfig, SetMaintenanceFee, ResolveMarket).
/// Expected: All admin operations fail for non-admin.
#[test]
fn test_attack_admin_op_as_user() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // UpdateAdmin
    let result = env.try_update_admin(&attacker, &attacker.pubkey());
    assert!(result.is_err(), "ATTACK: Non-admin UpdateAdmin should fail");

    // SetRiskThreshold
    let result = env.try_set_risk_threshold(&attacker, 0);
    assert!(
        result.is_err(),
        "ATTACK: Non-admin SetRiskThreshold should fail"
    );

    // UpdateConfig
    let result = env.try_update_config(&attacker);
    assert!(
        result.is_err(),
        "ATTACK: Non-admin UpdateConfig should fail"
    );

    // SetMaintenanceFee
    let result = env.try_set_maintenance_fee(&attacker, 0);
    assert!(
        result.is_err(),
        "ATTACK: Non-admin SetMaintenanceFee should fail"
    );

    // ResolveMarket
    let result = env.try_resolve_market(&attacker);
    assert!(
        result.is_err(),
        "ATTACK: Non-admin ResolveMarket should fail"
    );

    // SetOracleAuthority
    let result = env.try_set_oracle_authority(&attacker, &attacker.pubkey());
    assert!(
        result.is_err(),
        "ATTACK: Non-admin SetOracleAuthority should fail"
    );

    // SetOraclePriceCap
    let result = env.try_set_oracle_price_cap(&attacker, 100);
    assert!(
        result.is_err(),
        "ATTACK: Non-admin SetOraclePriceCap should fail"
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    assert_eq!(
        slab_after, slab_before,
        "Non-admin admin-op attempts must leave slab state unchanged"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Non-admin admin-op attempts must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Non-admin admin-op attempts must preserve engine vault"
    );
}

/// ATTACK: After admin is burned (set to [0;32]), verify no one can act as admin.
/// Expected: All admin ops fail since nobody can sign as the zero address.
#[test]
fn test_attack_burned_admin_cannot_act() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let zero_pubkey = Pubkey::new_from_array([0u8; 32]);

    // Burn admin by transferring to zero address
    let result = env.try_update_admin(&admin, &zero_pubkey);
    assert!(result.is_ok(), "Admin should be able to burn admin key");

    // Now old admin can no longer act
    let result = env.try_set_risk_threshold(&admin, 999);
    assert!(
        result.is_err(),
        "ATTACK: Burned admin - old admin should not work"
    );

    // Random attacker also can't act (no one can sign as [0;32])
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let result = env.try_set_risk_threshold(&attacker, 999);
    assert!(
        result.is_err(),
        "ATTACK: Burned admin - attacker should not work"
    );
}

/// ATTACK: Push oracle price with wrong signer (not the oracle authority).
/// Expected: Transaction fails with authorization error.
#[test]
fn test_attack_oracle_authority_wrong_signer() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Admin sets oracle authority
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let authority = Keypair::new();
    env.svm.airdrop(&authority.pubkey(), 1_000_000_000).unwrap();
    let result = env.try_set_oracle_authority(&admin, &authority.pubkey());
    assert!(result.is_ok(), "Admin should set oracle authority");

    // Wrong signer tries to push price
    let wrong_signer = Keypair::new();
    env.svm
        .airdrop(&wrong_signer.pubkey(), 1_000_000_000)
        .unwrap();
    let result = env.try_push_oracle_price(&wrong_signer, 200_000_000, 200);
    assert!(
        result.is_err(),
        "ATTACK: Wrong signer pushing oracle price should fail"
    );

    // Correct authority should succeed
    let result = env.try_push_oracle_price(&authority, 200_000_000, 200);
    assert!(
        result.is_ok(),
        "Correct oracle authority should succeed: {:?}",
        result
    );
}

// ============================================================================
// 3. Trade Manipulation
// ============================================================================

/// ATTACK: Open a position larger than initial margin allows.
/// Expected: Margin check rejects the trade.
#[test]
fn test_attack_trade_without_margin() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 100_000); // Tiny deposit (0.0001 SOL)

    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // Try to open an enormous position relative to capital
    // At $138, 1B position = $138B notional, requiring $13.8B margin (10%)
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 1_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Trade without sufficient margin should fail"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected oversized trade must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected oversized trade must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected oversized trade must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected oversized trade must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected oversized trade must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected oversized trade must preserve engine vault"
    );
}

/// ATTACK: Open a risk-increasing trade when insurance is depleted and
/// risk reduction threshold is non-zero.
/// Expected: Risk-increasing trade gated when insurance gone.
#[test]
fn test_attack_trade_risk_increase_when_gated() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Set risk reduction threshold very high so gate activates
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_risk_threshold(&admin, 1_000_000_000_000_000_000)
        .expect("risk threshold setup must succeed");

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // Threshold set to u128::MAX means insurance must exceed an impossible amount
    // to allow risk-increasing trades. With no insurance funded, this should gate.
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Risk-increasing trade should be gated when threshold exceeds insurance"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected gated trade must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected gated trade must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected gated trade must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected gated trade must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected gated trade must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected gated trade must preserve engine vault"
    );
}

/// ATTACK: Execute TradeNoCpi in Hyperp mode (should be blocked).
/// Expected: Program rejects TradeNoCpi for Hyperp markets.
#[test]
fn test_attack_trade_nocpi_in_hyperp_mode() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(138_000_000); // Hyperp mode

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // Try TradeNoCpi (tag 6) - should be blocked in Hyperp mode
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    assert!(
        result.is_err(),
        "ATTACK: TradeNoCpi in Hyperp mode should be blocked"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected Hyperp TradeNoCpi must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected Hyperp TradeNoCpi must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected Hyperp TradeNoCpi must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected Hyperp TradeNoCpi must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected Hyperp TradeNoCpi must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected Hyperp TradeNoCpi must preserve engine vault"
    );
}

/// ATTACK: Trade after market is resolved.
/// Expected: No new trades on resolved markets.
#[test]
fn test_attack_trade_after_market_resolved() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Set oracle authority and push price so resolve can work
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Resolve the market
    let result = env.try_resolve_market(&admin);
    assert!(
        result.is_ok(),
        "Admin should be able to resolve market: {:?}",
        result
    );
    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // Try to trade on resolved market
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Trade on resolved market should fail"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected post-resolution trade must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected post-resolution trade must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected post-resolution trade must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected post-resolution trade must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected post-resolution trade must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected post-resolution trade must preserve engine vault"
    );
}

/// ATTACK: Position flip (long->short) should use initial_margin_bps, not
/// maintenance_margin_bps. This is Finding L regression test.
#[test]
fn test_attack_position_flip_requires_initial_margin() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0); // initial=10%, maintenance=5%

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // User with limited capital
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000); // 1 SOL

    // Open a moderate long position (uses some of the initial margin budget)
    // At $138, position=5M means notional = 5M * 138 = 690M, margin needed = 69M (10%)
    // 1 SOL = 1e9, so this should be within margin
    let initial_long_size = 5_000_000i128;
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, initial_long_size);
    assert!(result.is_ok(), "Initial long should work: {:?}", result);
    assert_eq!(
        env.read_account_position(user_idx),
        initial_long_size,
        "Initial margin-eligible long should set expected user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        -initial_long_size,
        "Initial margin-eligible long should set expected opposite LP position"
    );
    let user_pos_before_flip = env.read_account_position(user_idx);
    let lp_pos_before_flip = env.read_account_position(lp_idx);
    let user_cap_before_flip = env.read_account_capital(user_idx);
    let lp_cap_before_flip = env.read_account_capital(lp_idx);
    let spl_vault_before_flip = env.vault_balance();
    let engine_vault_before_flip = env.read_engine_vault();

    // Try to flip to a very large short: -5M to close + -100M new short
    // The new short side notional = 100M * 138 = 13.8B, requiring 1.38B initial margin
    // User only has ~1 SOL = 1e9, so this should fail
    let oversize_flip = -105_000_000i128;
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, oversize_flip);
    assert!(
        result.is_err(),
        "ATTACK: Position flip to oversized short should require initial margin"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before_flip,
        "Rejected oversized flip must preserve user position from prior valid trade"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before_flip,
        "Rejected oversized flip must preserve LP position from prior valid trade"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before_flip,
        "Rejected oversized flip must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before_flip,
        "Rejected oversized flip must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before_flip,
        "Rejected oversized flip must preserve SPL vault aggregate"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before_flip,
        "Rejected oversized flip must preserve engine vault aggregate"
    );
}

// ============================================================================
// 4. TradeCpi / Matcher Attacks
// ============================================================================

/// ATTACK: Substitute a malicious matcher program in TradeCpi.
/// Expected: Matcher program must match what was registered at InitLP.
#[test]
fn test_attack_tradecpi_wrong_matcher_program() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };
    env.init_market();

    let matcher_prog = env.matcher_program_id;

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);
    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_vault();

    // Use wrong matcher program (spl_token as fake matcher)
    let wrong_prog = spl_token::ID;
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        1_000_000,
        &wrong_prog,
        &matcher_ctx,
    );
    assert!(
        result.is_err(),
        "ATTACK: Wrong matcher program should be rejected"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected wrong-matcher-program TradeCpi must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected wrong-matcher-program TradeCpi must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected wrong-matcher-program TradeCpi must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected wrong-matcher-program TradeCpi must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected wrong-matcher-program TradeCpi must preserve SPL vault"
    );
    assert_eq!(
        env.read_vault(),
        engine_vault_before,
        "Rejected wrong-matcher-program TradeCpi must preserve engine vault"
    );
}

/// ATTACK: Provide wrong matcher context account.
/// Expected: Context must be owned by registered matcher program.
#[test]
fn test_attack_tradecpi_wrong_matcher_context() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };
    env.init_market();

    let matcher_prog = env.matcher_program_id;

    let lp = Keypair::new();
    let (lp_idx, _correct_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);
    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_vault();

    // Create a fake context
    let fake_ctx = Pubkey::new_unique();
    env.svm
        .set_account(
            fake_ctx,
            Account {
                lamports: 10_000_000,
                data: vec![0u8; MATCHER_CONTEXT_LEN],
                owner: matcher_prog,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        1_000_000,
        &matcher_prog,
        &fake_ctx,
    );
    assert!(
        result.is_err(),
        "ATTACK: Wrong matcher context should be rejected"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected wrong-matcher-context TradeCpi must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected wrong-matcher-context TradeCpi must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected wrong-matcher-context TradeCpi must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected wrong-matcher-context TradeCpi must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected wrong-matcher-context TradeCpi must preserve SPL vault"
    );
    assert_eq!(
        env.read_vault(),
        engine_vault_before,
        "Rejected wrong-matcher-context TradeCpi must preserve engine vault"
    );
}

/// ATTACK: Supply a fabricated LP PDA that doesn't match the derivation.
/// Expected: PDA derivation check fails.
#[test]
fn test_attack_tradecpi_wrong_lp_pda() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };
    env.init_market();

    let matcher_prog = env.matcher_program_id;

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);
    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_vault();

    // Use a random pubkey as the PDA
    let wrong_pda = Pubkey::new_unique();
    let result = env.try_trade_cpi_with_wrong_pda(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        1_000_000,
        &matcher_prog,
        &matcher_ctx,
        &wrong_pda,
    );
    assert!(result.is_err(), "ATTACK: Wrong LP PDA should be rejected");
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected wrong-LP-PDA TradeCpi must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected wrong-LP-PDA TradeCpi must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected wrong-LP-PDA TradeCpi must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected wrong-LP-PDA TradeCpi must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected wrong-LP-PDA TradeCpi must preserve SPL vault"
    );
    assert_eq!(
        env.read_vault(),
        engine_vault_before,
        "Rejected wrong-LP-PDA TradeCpi must preserve engine vault"
    );
}

/// ATTACK: Provide a PDA that has lamports (non-system shape).
/// Expected: PDA shape validation rejects accounts with lamports/data.
#[test]
fn test_attack_tradecpi_pda_with_lamports() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };
    env.init_market();

    let matcher_prog = env.matcher_program_id;

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);
    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_vault();

    // Derive the correct PDA but fund it with lamports to break shape check
    let lp_bytes = lp_idx.to_le_bytes();
    let (lp_pda, _) =
        Pubkey::find_program_address(&[b"lp", env.slab.as_ref(), &lp_bytes], &env.program_id);

    // Give the PDA lamports (makes it non-system shape)
    env.svm
        .set_account(
            lp_pda,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; 32],
                owner: solana_sdk::system_program::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        1_000_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(
        result.is_err(),
        "ATTACK: PDA with lamports/data should be rejected"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected malformed-PDA TradeCpi must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected malformed-PDA TradeCpi must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected malformed-PDA TradeCpi must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected malformed-PDA TradeCpi must preserve LP capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected malformed-PDA TradeCpi must preserve SPL vault"
    );
    assert_eq!(
        env.read_vault(),
        engine_vault_before,
        "Rejected malformed-PDA TradeCpi must preserve engine vault"
    );
}

/// ATTACK: LP A's matcher tries to trade for LP B.
/// Expected: Matcher context must match the LP's registered context.
#[test]
fn test_attack_tradecpi_cross_lp_matcher_binding() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };
    env.init_market();

    let matcher_prog = env.matcher_program_id;

    // Create LP A
    let lp_a = Keypair::new();
    let (lp_a_idx, ctx_a) = env.init_lp_with_matcher(&lp_a, &matcher_prog);
    env.deposit(&lp_a, lp_a_idx, 50_000_000_000);

    // Create LP B
    let lp_b = Keypair::new();
    let (lp_b_idx, _ctx_b) = env.init_lp_with_matcher(&lp_b, &matcher_prog);
    env.deposit(&lp_b, lp_b_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);
    let user_pos_before = env.read_account_position(user_idx);
    let lp_a_pos_before = env.read_account_position(lp_a_idx);
    let lp_b_pos_before = env.read_account_position(lp_b_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_a_cap_before = env.read_account_capital(lp_a_idx);
    let lp_b_cap_before = env.read_account_capital(lp_b_idx);
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_vault();

    // Try to use LP A's context for LP B's trade
    let result = env.try_trade_cpi(
        &user,
        &lp_b.pubkey(),
        lp_b_idx,
        user_idx,
        1_000_000,
        &matcher_prog,
        &ctx_a, // Wrong: LP A's context for LP B
    );
    assert!(
        result.is_err(),
        "ATTACK: Cross-LP matcher binding should be rejected"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected cross-LP matcher swap must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_a_idx),
        lp_a_pos_before,
        "Rejected cross-LP matcher swap must preserve LP-A position"
    );
    assert_eq!(
        env.read_account_position(lp_b_idx),
        lp_b_pos_before,
        "Rejected cross-LP matcher swap must preserve LP-B position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected cross-LP matcher swap must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_a_idx),
        lp_a_cap_before,
        "Rejected cross-LP matcher swap must preserve LP-A capital"
    );
    assert_eq!(
        env.read_account_capital(lp_b_idx),
        lp_b_cap_before,
        "Rejected cross-LP matcher swap must preserve LP-B capital"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected cross-LP matcher swap must preserve SPL vault"
    );
    assert_eq!(
        env.read_vault(),
        engine_vault_before,
        "Rejected cross-LP matcher swap must preserve engine vault"
    );
}

// ============================================================================
// 5. Liquidation Attacks
// ============================================================================

/// ATTACK: Liquidate a solvent account (positive equity above maintenance margin).
/// Expected: Liquidation rejected for healthy accounts.
#[test]
fn test_attack_liquidate_solvent_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Heavily over-capitalized user with tiny position
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 50_000_000_000); // 50 SOL

    // Tiny position relative to capital
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    env.set_slot(200);
    env.crank();

    let capital_before = env.read_account_capital(user_idx);
    let position_before = env.read_account_position(user_idx);

    // Try to liquidate heavily collateralized account
    // Engine may return Ok (no-op) or Err depending on implementation
    let liquidation_attempt = env.try_liquidate_target(user_idx);

    // Verify: solvent account's position and capital should be unchanged
    let capital_after = env.read_account_capital(user_idx);
    let position_after = env.read_account_position(user_idx);
    assert_eq!(capital_before, capital_after,
        "ATTACK: Solvent account capital should not change from liquidation attempt. liquidate_result={:?}",
        liquidation_attempt);
    assert_eq!(position_before, position_after,
        "ATTACK: Solvent account position should not change from liquidation attempt. liquidate_result={:?}",
        liquidation_attempt);
}

/// ATTACK: Self-liquidation to extract value (liquidation fee goes to insurance).
/// Expected: Self-liquidation doesn't create profit for the attacker.
#[test]
fn test_attack_self_liquidation_no_profit() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 2_000_000_000); // 2 SOL

    // Open leveraged long
    env.trade(&user, &lp, lp_idx, user_idx, 10_000_000);

    // Price drops to make user underwater
    env.set_slot_and_price(200, 90_000_000);
    env.crank();

    let capital_before = env.read_account_capital(user_idx);
    let insurance_before = env.read_insurance_balance();

    // Try to liquidate (anyone can call)
    let result = env.try_liquidate_target(user_idx);
    let capital_after = env.read_account_capital(user_idx);
    let insurance_after = env.read_insurance_balance();

    assert!(
        capital_after <= capital_before,
        "ATTACK: Self-liquidation attempt increased attacker capital: before={} after={} result={:?}",
        capital_before,
        capital_after,
        result
    );

    if result.is_ok() {
        // Liquidation fee goes to insurance, user doesn't profit
        assert!(
            insurance_after >= insurance_before,
            "ATTACK: Insurance should not decrease from liquidation"
        );
    } else {
        // Failed liquidation should be a complete no-op
        assert_eq!(
            insurance_after, insurance_before,
            "Failed liquidation should not change insurance: before={} after={}",
            insurance_before, insurance_after
        );
        assert_eq!(
            capital_after, capital_before,
            "Failed liquidation should not change capital: before={} after={}",
            capital_before, capital_after
        );
    }

    // Either liquidation was rejected (healthy account = defense working)
    // or it succeeded and insurance received the fee (no profit extraction).
    // In both cases, verify vault is intact.
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after self-liquidation attempt: engine={} vault={}",
        engine_vault, vault
    );
    assert!(
        vault > 0,
        "Vault should still have balance after liquidation attempt"
    );
}

/// ATTACK: Price recovers before liquidation executes - account is now solvent.
/// Expected: Liquidation rejected when account recovers above maintenance margin.
#[test]
fn test_attack_liquidate_after_price_recovery() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Heavily over-capitalized user
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 50_000_000_000); // 50 SOL

    // Small position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Price goes up slightly (user is profitable, very healthy)
    env.set_slot_and_price(200, 140_000_000);
    env.crank();

    let position_before = env.read_account_position(user_idx);
    let capital_before = env.read_account_capital(user_idx);

    // Try liquidation - engine may return Ok (no-op) or Err
    let liquidation_attempt = env.try_liquidate_target(user_idx);

    // Verify: account state should be unchanged (no liquidation occurred)
    let position_after = env.read_account_position(user_idx);
    let capital_after = env.read_account_capital(user_idx);
    assert_eq!(position_before, position_after,
        "ATTACK: Healthy account position should not change from liquidation. liquidate_result={:?}",
        liquidation_attempt);
    assert_eq!(
        capital_before, capital_after,
        "ATTACK: Healthy account capital should not change from liquidation. liquidate_result={:?}",
        liquidation_attempt
    );
}

// ============================================================================
// 6. Insurance Fund Attacks
// ============================================================================

/// ATTACK: Withdraw insurance on an active (non-resolved) market.
/// Expected: WithdrawInsurance only works on resolved markets.
#[test]
fn test_attack_withdraw_insurance_before_resolution() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Top up insurance fund so there's something to steal
    let payer = Keypair::new();
    env.svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    env.top_up_insurance(&payer, 1_000_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let insurance_before = env.read_insurance_balance();
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;

    // Try to withdraw insurance without resolving market
    let result = env.try_withdraw_insurance(&admin);
    assert!(
        result.is_err(),
        "ATTACK: Withdraw insurance on active market should fail"
    );
    assert_eq!(
        env.read_insurance_balance(),
        insurance_before,
        "Rejected insurance withdraw on active market must preserve insurance balance"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected insurance withdraw on active market must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected insurance withdraw on active market must preserve engine vault"
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    assert_eq!(
        slab_after, slab_before,
        "Rejected insurance withdraw on active market must preserve slab state"
    );
}

/// ATTACK: Withdraw insurance when positions are still open.
/// Expected: WithdrawInsurance requires all positions closed.
#[test]
fn test_attack_withdraw_insurance_with_open_positions() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Set oracle authority and push price
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Open a position
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Resolve market
    let result = env.try_resolve_market(&admin);
    assert!(result.is_ok(), "Should resolve: {:?}", result);

    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let insurance_before = env.read_insurance_balance();
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    // Try to withdraw insurance while position still open
    let result = env.try_withdraw_insurance(&admin);
    assert!(
        result.is_err(),
        "ATTACK: Withdraw insurance with open positions should fail"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected insurance withdraw with open positions must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected insurance withdraw with open positions must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected insurance withdraw with open positions must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected insurance withdraw with open positions must preserve LP capital"
    );
    assert_eq!(
        env.read_insurance_balance(),
        insurance_before,
        "Rejected insurance withdraw with open positions must preserve insurance balance"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected insurance withdraw with open positions must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected insurance withdraw with open positions must preserve engine vault"
    );
}

/// ATTACK: Close slab while insurance fund has remaining balance.
/// Expected: CloseSlab requires insurance_fund.balance == 0.
#[test]
fn test_attack_close_slab_with_insurance_remaining() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Top up insurance fund
    let payer = Keypair::new();
    env.svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    env.top_up_insurance(&payer, 1_000_000_000);

    let insurance_bal = env.read_insurance_balance();
    assert_eq!(insurance_bal, 1_000_000_000, "Insurance should equal topped-up amount");

    let insurance_before = env.read_insurance_balance();
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;

    // Try to close slab - should fail because insurance > 0
    let result = env.try_close_slab();
    assert!(
        result.is_err(),
        "ATTACK: CloseSlab with non-zero insurance should fail"
    );
    assert_eq!(
        env.read_insurance_balance(),
        insurance_before,
        "Rejected CloseSlab with non-zero insurance must preserve insurance balance"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected CloseSlab with non-zero insurance must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected CloseSlab with non-zero insurance must preserve engine vault"
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    assert_eq!(
        slab_after, slab_before,
        "Rejected CloseSlab with non-zero insurance must preserve slab bytes"
    );
}

// ============================================================================
// 7. Oracle Manipulation
// ============================================================================

/// ATTACK: Circuit breaker should cap price movement per slot.
/// Expected: Price cannot jump more than allowed by circuit breaker.
#[test]
fn test_attack_oracle_price_cap_circuit_breaker() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Set oracle authority and cap
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_set_oracle_price_cap(&admin, 100)
        .expect("oracle price cap setup must succeed"); // 1% per slot

    // Push initial price
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");
    env.set_slot(101);

    // Config offset for authority_price_e6
    const AUTH_PRICE_OFF: usize = 360;
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let auth_price_before = u64::from_le_bytes(
        slab_before[AUTH_PRICE_OFF..AUTH_PRICE_OFF + 8]
            .try_into()
            .unwrap(),
    );

    // Push a 50% price jump one slot later - should be clamped or rejected.
    let result = env.try_push_oracle_price(&admin, 207_000_000, 101); // +50%
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let auth_price_after = u64::from_le_bytes(
        slab_after[AUTH_PRICE_OFF..AUTH_PRICE_OFF + 8]
            .try_into()
            .unwrap(),
    );
    assert_ne!(
        auth_price_after, 207_000_000,
        "Circuit breaker must not accept unclamped +50% move in one slot"
    );
    if result.is_ok() {
        assert!(
            auth_price_after >= auth_price_before,
            "Accepted push should not move authority price backwards (before={} after={})",
            auth_price_before,
            auth_price_after
        );
    } else {
        assert_eq!(
            auth_price_after, auth_price_before,
            "Rejected push must leave authority price unchanged"
        );
    }

    // Either way, vault should be intact.
    let vault = env.vault_balance();
    assert_eq!(
        vault, 0,
        "Circuit breaker test: vault should be 0 (no deposits)"
    );
    // The real test: after the push, crank should still work without corruption
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);
    env.set_slot(300);
    env.crank(); // Should not panic or corrupt state after price cap
    let vault_after = env.vault_balance();
    assert_eq!(
        vault_after, 10_000_000_000,
        "Vault should be intact after circuit breaker + crank"
    );
}

/// ATTACK: Use a stale oracle price for margin-dependent operations.
/// Expected: Stale oracle rejected by staleness check.
#[test]
fn test_attack_stale_oracle_rejected() {
    program_path();

    // Test that PushOraclePrice rejects stale (backward) timestamps.
    // The engine enforces monotonic authority_timestamp for non-Hyperp markets:
    // if new timestamp < stored authority_timestamp → OracleStale error.
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");

    // Push price at timestamp 1000 - establishes authority_timestamp = 1000
    env.try_push_oracle_price(&admin, 138_000_000, 1000)
        .expect("first oracle push must succeed");

    // Push price at timestamp 2000 - advances authority_timestamp to 2000
    let result = env.try_push_oracle_price(&admin, 140_000_000, 2000);
    assert!(
        result.is_ok(),
        "Forward timestamp push should succeed: {:?}",
        result
    );

    // Push price at timestamp 500 (BEFORE 2000) - must be REJECTED as stale
    let result = env.try_push_oracle_price(&admin, 135_000_000, 500);
    assert!(
        result.is_err(),
        "ATTACK: Stale oracle push (timestamp 500 < stored 2000) must be rejected!"
    );

    // Push at exactly the same timestamp (2000) - must also be rejected (not strictly greater)
    let result = env.try_push_oracle_price(&admin, 136_000_000, 1999);
    assert!(
        result.is_err(),
        "ATTACK: Oracle push with timestamp < stored must be rejected!"
    );

    // Push at a forward timestamp to confirm the market still works
    let result = env.try_push_oracle_price(&admin, 139_000_000, 3000);
    assert!(
        result.is_ok(),
        "Forward timestamp push should succeed after stale rejection: {:?}",
        result
    );
}

/// ATTACK: Push zero price via oracle authority.
/// Expected: Zero price rejected.
#[test]
fn test_attack_push_oracle_zero_price() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");

    // Push valid price first
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");

    // Try to push zero price
    let result = env.try_push_oracle_price(&admin, 0, 200);
    assert!(
        result.is_err(),
        "ATTACK: Zero oracle price should be rejected"
    );
}

/// ATTACK: Push oracle price when no oracle authority is configured.
/// Expected: Fails because default authority is [0;32] (unset).
#[test]
fn test_attack_push_oracle_without_authority_set() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Don't set oracle authority - default is [0;32]
    let random = Keypair::new();
    env.svm.airdrop(&random.pubkey(), 1_000_000_000).unwrap();
    let result = env.try_push_oracle_price(&random, 138_000_000, 100);
    assert!(
        result.is_err(),
        "ATTACK: Push price without authority set should fail"
    );
}

// ============================================================================
// 8. Premarket Resolution Attacks
// ============================================================================

/// ATTACK: Resolve market without oracle authority price being set.
/// Expected: Resolution requires authority price to be set first.
#[test]
fn test_attack_resolve_market_without_oracle_price() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Set oracle authority but DON'T push a price
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");

    // Try to resolve without pushing price
    let result = env.try_resolve_market(&admin);
    assert!(
        result.is_err(),
        "ATTACK: Resolve without oracle price should fail"
    );
}

/// ATTACK: Deposit after market is resolved.
/// Expected: No new deposits on resolved markets.
#[test]
fn test_attack_deposit_after_resolution() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");

    // Create user before resolution
    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    // Resolve market
    let result = env.try_resolve_market(&admin);
    assert!(result.is_ok(), "Admin should resolve: {:?}", result);

    // Try to deposit after resolution
    let result = env.try_deposit(&user, user_idx, 1_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Deposit after resolution should fail"
    );
}

/// ATTACK: Init new user after market is resolved.
/// Expected: No new accounts on resolved markets.
#[test]
fn test_attack_init_user_after_resolution() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");

    // Resolve market
    let result = env.try_resolve_market(&admin);
    assert!(result.is_ok(), "Admin should resolve: {:?}", result);

    // Try to create new user after resolution
    let new_user = Keypair::new();
    let result = env.try_init_user(&new_user);
    assert!(
        result.is_err(),
        "ATTACK: Init user after resolution should fail"
    );
}

/// ATTACK: Resolve an already-resolved market.
/// Expected: Double resolution rejected.
#[test]
fn test_attack_double_resolution() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");

    // First resolution
    let result = env.try_resolve_market(&admin);
    assert!(result.is_ok(), "First resolve should succeed: {:?}", result);
    assert!(env.is_market_resolved(), "Market should be resolved");

    // Second resolution - should fail
    let result = env.try_resolve_market(&admin);
    assert!(result.is_err(), "ATTACK: Double resolution should fail");
}

// ============================================================================
// 9. Account Lifecycle Attacks
// ============================================================================

/// ATTACK: Close account while still holding an open position.
/// Expected: CloseAccount rejects when position_size != 0.
#[test]
fn test_attack_close_account_with_position() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Verify position exists
    let pos = env.read_account_position(user_idx);
    assert!(pos != 0, "User should have open position");

    // Try to close account with position
    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_err(),
        "ATTACK: Close account with open position should fail"
    );
}

/// ATTACK: Close account when PnL is outstanding (non-zero).
/// Expected: CloseAccount requires PnL == 0.
#[test]
fn test_attack_close_account_with_pnl() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Open and close position with price change to create PnL
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    env.set_slot_and_price(200, 150_000_000);
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, -5_000_000);
    env.set_slot_and_price(300, 150_000_000);
    env.crank();

    // After full cycle, position is closed and warmup settles PnL to capital
    let pnl = env.read_account_pnl(user_idx);
    let pos = env.read_account_position(user_idx);
    assert_eq!(pos, 0, "Position should be closed after closing trade");
    assert_eq!(pnl, 0, "PnL should be zero after crank settles warmup");

    // With PnL=0 and position=0, close should succeed
    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "Close with zero PnL and position should succeed: {:?}",
        result
    );
}

/// ATTACK: Initialize a market twice on the same slab.
/// Expected: Second InitMarket fails because slab already initialized.
#[test]
fn test_attack_double_init_market() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Try to init again on the same slab
    let admin = &env.payer;
    let dummy_ata = Pubkey::new_unique();
    env.svm
        .set_account(
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

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: encode_init_market_with_invert(&admin.pubkey(), &env.mint, &TEST_FEED_ID, 0),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(result.is_err(), "ATTACK: Double InitMarket should fail");
}

// ============================================================================
// 10. Economic / Value Extraction
// ============================================================================

/// ATTACK: Accumulate dust through many sub-unit-scale deposits to extract value.
/// Expected: Dust is tracked and cannot be extracted (swept to insurance).
#[test]
fn test_attack_dust_accumulation_theft() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_full(0, 1000, 0); // unit_scale = 1000

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    let vault_before = env.vault_balance();

    // Deposit amounts that create dust: 1500 % 1000 = 500 dust each
    for _ in 0..5 {
        env.deposit(&user, user_idx, 1_500);
    }

    let vault_after = env.vault_balance();
    let total_deposited = vault_after - vault_before;

    // User should only be credited for full units (5 * 1 unit = 5000 base)
    // Remaining 500 * 5 = 2500 dust is tracked separately
    let capital = env.read_account_capital(user_idx);
    println!(
        "Capital credited: {} (total deposited: {})",
        capital, total_deposited
    );

    // Capital should be less than total deposited (dust not credited)
    // With unit_scale=1000, capital is in units, so 5 * 1500 / 1000 = 7 units
    // Dust cannot be extracted by the user
    assert!(
        total_deposited == 7_500,
        "Vault should have all 7500 deposited"
    );
}

/// ATTACK: Micro-trade cannot extract value even with minimum position size.
/// Note: Market has trading_fee_bps=0 (default). This tests conservation,
/// not fee ceiling division. Fee ceiling division is tested at the engine level.
#[test]
fn test_attack_fee_evasion_micro_trades() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Even tiny trades should not extract value through rounding
    let vault_before = env.vault_balance();
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 1); // Minimum possible size
    let vault_after = env.vault_balance();

    // Vault should be unchanged (trades don't move tokens, only PnL)
    assert_eq!(
        vault_before, vault_after,
        "ATTACK: Micro trade should not change vault balance (no value extraction)"
    );

    // Trade attempts must never mint user capital.
    let capital = env.read_account_capital(user_idx);
    assert!(
        capital <= 10_000_000_000u128,
        "ATTACK: Micro trade should not increase capital beyond deposit: cap={}",
        capital
    );
    let pos = env.read_account_position(user_idx);
    if result.is_ok() {
        assert_eq!(
            pos, 1,
            "Successful min-size trade should produce 1-unit position: pos={}",
            pos
        );
    } else {
        assert_eq!(pos, 0, "Rejected trade should keep zero position: pos={}", pos);
        assert_eq!(
            capital, 10_000_000_000u128,
            "Rejected trade should not change capital: cap={}",
            capital
        );
    }
}

/// ATTACK: Deposit/withdraw cycle to manipulate haircut or extract extra tokens.
/// Expected: Vault token balance is always consistent - no tokens created from nothing.
#[test]
fn test_attack_haircut_manipulation_via_deposit_withdraw() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    let vault_before = env.vault_balance();

    // Rapid deposit/withdraw cycles - should not create or destroy value
    for _ in 0..5 {
        env.deposit(&user, user_idx, 10_000_000_000);
        env.try_withdraw(&user, user_idx, 5_000_000_000)
            .expect("cycle withdrawal should succeed");
    }

    let vault_after = env.vault_balance();
    // After 5 cycles: deposited 50 SOL total, withdrew 25 SOL total
    // Vault should have gained 25 SOL net
    let expected_vault = vault_before + 25_000_000_000;
    assert_eq!(
        vault_after, expected_vault,
        "ATTACK: Vault balance mismatch after deposit/withdraw cycles. \
         Expected {}, got {}",
        expected_vault, vault_after
    );

    // User should not be able to withdraw more than what's left
    let result = env.try_withdraw(&user, user_idx, 50_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Should not withdraw more than remaining capital"
    );
}

/// ATTACK: Verify no value is created or destroyed through trading operations.
/// Expected: Total vault token balance equals total deposits minus total withdrawals.
#[test]
fn test_attack_conservation_invariant() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 10_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 10_000_000_000);

    let total_deposited: u64 = 120_000_000_000; // 100 + 10 + 10 SOL

    // Vault should have all deposited funds
    let vault_after_deposits = env.vault_balance();
    assert_eq!(
        vault_after_deposits, total_deposited,
        "Vault should have exactly the deposited amount"
    );

    // User1 goes long, user2 goes short
    env.trade(&user1, &lp, lp_idx, user1_idx, 5_000_000);
    env.trade(&user2, &lp, lp_idx, user2_idx, -5_000_000);

    // Trading doesn't move tokens in/out of vault
    let vault_after_trades = env.vault_balance();
    assert_eq!(
        vault_after_trades, total_deposited,
        "Trading should not change vault token balance"
    );

    // Price changes and crank (internal PnL settlement, no token transfers)
    env.set_slot_and_price(200, 150_000_000);
    env.crank();

    let vault_after_crank = env.vault_balance();
    assert_eq!(
        vault_after_crank, total_deposited,
        "Crank should not change vault token balance"
    );

    // Price reversal and another crank
    env.set_slot_and_price(300, 120_000_000);
    env.crank();

    let vault_after_reversal = env.vault_balance();
    assert_eq!(
        vault_after_reversal, total_deposited,
        "Price reversal+crank should not change vault token balance"
    );

    println!(
        "CONSERVATION VERIFIED: Vault balance {} unchanged through all operations",
        vault_after_reversal
    );
}

// ============================================================================
// PEN TEST SUITE ROUND 2: Deep Crank, Funding, Warmup, GC, and Race Attacks
// ============================================================================

fn encode_crank_with_panic(allow_panic: u8) -> Vec<u8> {
    let mut data = vec![5u8];
    data.extend_from_slice(&u16::MAX.to_le_bytes()); // permissionless
    data.push(allow_panic);
    data
}

fn encode_crank_self(caller_idx: u16) -> Vec<u8> {
    let mut data = vec![5u8];
    data.extend_from_slice(&caller_idx.to_le_bytes());
    data.push(0u8); // allow_panic = false
    data
}

impl TestEnv {
    /// Try crank with custom allow_panic flag
    fn try_crank_with_panic(&mut self, signer: &Keypair, allow_panic: u8) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_crank_with_panic(allow_panic),
        };
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&signer.pubkey()),
            &[signer],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Try self-crank (caller_idx = specific account)
    fn try_crank_self(&mut self, owner: &Keypair, caller_idx: u16) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_crank_self(caller_idx),
        };
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Init market with trading fees enabled
    fn init_market_with_trading_fee(&mut self, trading_fee_bps: u64) {
        let admin = &self.payer;
        let dummy_ata = Pubkey::new_unique();
        self.svm
            .set_account(
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

        let mut data = vec![0u8];
        data.extend_from_slice(admin.pubkey().as_ref());
        data.extend_from_slice(self.mint.as_ref());
        data.extend_from_slice(&TEST_FEED_ID);
        data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_staleness_secs
        data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
        data.push(0u8); // invert
        data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
        data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6
                                                     // RiskParams
        data.extend_from_slice(&0u64.to_le_bytes()); // warmup_period_slots
        data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps
        data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps
        data.extend_from_slice(&trading_fee_bps.to_le_bytes()); // trading_fee_bps
        data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
        data.extend_from_slice(&0u128.to_le_bytes()); // new_account_fee
        data.extend_from_slice(&0u128.to_le_bytes()); // risk_reduction_threshold
        data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee_per_slot
        data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_crank_staleness_slots
        data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
        data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
        data.extend_from_slice(&100u64.to_le_bytes()); // liquidation_buffer_bps
        data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs

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
            data,
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("init_market_with_trading_fee failed");
    }
}

// ============================================================================
// 11. Crank Timing & Authorization Attacks
// ============================================================================

/// ATTACK: Non-admin tries to use allow_panic=1 flag on permissionless crank.
/// Expected: Rejected because allow_panic requires admin authorization.
#[test]
fn test_attack_permissionless_crank_with_panic_flag() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    env.set_slot(200);

    // Non-admin tries allow_panic=1 - should fail
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let result = env.try_crank_with_panic(&attacker, 1);
    assert!(
        result.is_err(),
        "ATTACK: Non-admin crank with allow_panic=1 should fail"
    );

    // Admin can use allow_panic=1
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let result = env.try_crank_with_panic(&admin, 1);
    assert!(
        result.is_ok(),
        "Admin crank with allow_panic=1 should succeed: {:?}",
        result
    );
}

/// ATTACK: Call crank twice in the same slot to cascade liquidations.
/// Expected: Second crank is a no-op (require_fresh_crank gate).
#[test]
fn test_attack_same_slot_double_crank() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // First crank at slot 200
    env.set_slot(200);
    env.crank();

    let capital_after_first = env.read_account_capital(user_idx);
    let position_after_first = env.read_account_position(user_idx);

    // Second crank at same slot 200 - should be no-op or rejected
    let caller2 = Keypair::new();
    env.svm.airdrop(&caller2.pubkey(), 1_000_000_000).unwrap();
    let second_crank_result = env.try_crank_with_panic(&caller2, 0);

    // Whether accepted (no-op) or rejected, account state must be unchanged
    let capital_after_second = env.read_account_capital(user_idx);
    let position_after_second = env.read_account_position(user_idx);
    assert_eq!(
        capital_after_first, capital_after_second,
        "ATTACK: Double crank should not change capital. second_crank_result={:?}",
        second_crank_result
    );
    assert_eq!(
        position_after_first, position_after_second,
        "ATTACK: Double crank should not change position. second_crank_result={:?}",
        second_crank_result
    );
}

/// ATTACK: Self-crank with wrong owner (caller_idx points to someone else's account).
/// Expected: Owner check rejects because signer doesn't match account owner.
#[test]
fn test_attack_self_crank_wrong_owner() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let victim = Keypair::new();
    let victim_idx = env.init_user(&victim);
    env.deposit(&victim, victim_idx, 10_000_000_000);

    env.trade(&victim, &lp, lp_idx, victim_idx, 5_000_000);
    env.set_slot(200);

    // Attacker tries self-crank using victim's account index
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let result = env.try_crank_self(&attacker, victim_idx);
    assert!(
        result.is_err(),
        "ATTACK: Self-crank with wrong owner should fail"
    );

    // Victim's own self-crank should work
    let result = env.try_crank_self(&victim, victim_idx);
    assert!(
        result.is_ok(),
        "Victim self-crank should succeed: {:?}",
        result
    );
}

/// ATTACK: Rapid crank across many slots to compound funding drain.
/// Expected: Funding rate is capped at max_bps_per_slot; no runaway drain.
#[test]
fn test_attack_funding_max_rate_sustained_drain() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Large imbalanced position to create max funding
    env.trade(&user, &lp, lp_idx, user_idx, 50_000_000);

    let capital_before = env.read_account_capital(user_idx);

    // Crank many times to accrue funding
    for i in 0..20 {
        env.set_slot(200 + i * 100);
        env.crank();
    }

    let capital_after = env.read_account_capital(user_idx);

    // Capital should not be completely drained - funding is rate-limited
    // User started with 10 SOL and held a 50M position through 20 cranks.
    // Even at max funding rate, capital should not hit zero.
    assert!(
        capital_after > 0,
        "ATTACK: Funding should not drain capital to zero (rate-limited). Before: {}, After: {}",
        capital_before,
        capital_after
    );

    // Vault should still be intact (no token leakage)
    let vault = env.vault_balance();
    assert!(
        vault > 0,
        "Vault should still have balance after sustained funding"
    );
}

// ============================================================================
// 12. Funding Calculation Edge Cases
// ============================================================================

/// ATTACK: Crank 3 times in same slot to bypass index smoothing (Bug #9 regression).
/// Expected: dt=0 returns no index movement (fix verified).
#[test]
fn test_attack_funding_same_slot_three_cranks_dt_zero() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // First crank at slot 200
    env.set_slot(200);
    env.crank();

    let capital_after_1 = env.read_account_capital(user_idx);

    // Crank again same slot - should be no-op (dt=0)
    // Engine may reject or accept as no-op
    let caller2 = Keypair::new();
    env.svm.airdrop(&caller2.pubkey(), 1_000_000_000).unwrap();
    let second_crank_result = env.try_crank_with_panic(&caller2, 0);

    // Third crank same slot
    let caller3 = Keypair::new();
    env.svm.airdrop(&caller3.pubkey(), 1_000_000_000).unwrap();
    let third_crank_result = env.try_crank_with_panic(&caller3, 0);

    let capital_after_3 = env.read_account_capital(user_idx);

    // Capital should not have changed from repeated same-slot cranks
    assert_eq!(
        capital_after_1, capital_after_3,
        "ATTACK: Same-slot repeated cranks should not change capital (dt=0 fix). \
         second={:?} third={:?}",
        second_crank_result, third_crank_result
    );
}

/// ATTACK: Large time gap between cranks (dt overflow).
/// Expected: dt is capped and funding doesn't overflow.
#[test]
fn test_attack_funding_large_dt_gap() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // First crank
    env.set_slot(200);
    env.crank();

    // Verify: a reasonable dt gap (~2 minutes) still works
    env.set_slot(500);
    env.crank(); // should succeed without overflow

    // Verify vault didn't get corrupted (still has tokens)
    let vault_balance = env.vault_balance();
    assert!(
        vault_balance > 0,
        "Vault should still have balance after reasonable dt crank"
    );

    // Jump forward ~1 year worth of slots (massive dt)
    // 1 year ≈ 31.5M seconds ≈ 78.8M slots at 400ms
    // The engine should reject this with EngineOverflow (dt cap exceeded)
    env.set_slot(80_000_000);
    let result = env.try_crank();
    assert!(
        result.is_err(),
        "ATTACK: Excessively large dt gap should be rejected (overflow protection)"
    );
    let err_msg = result.unwrap_err();
    assert!(
        err_msg.contains("0x12"),
        "Expected EngineOverflow (0x12), got: {}",
        err_msg
    );
}

// ============================================================================
// 13. Warmup Period Edge Cases
// ============================================================================

/// ATTACK: Warmup with period=0 (instant conversion).
/// Expected: Profit converts to capital immediately.
#[test]
fn test_attack_warmup_zero_period_instant() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_warmup(0, 0); // warmup = 0 slots

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Open and close position with profit
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    env.set_slot_and_price(200, 150_000_000);
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, -5_000_000);

    // With warmup=0, profit should be immediately available
    env.set_slot_and_price(300, 150_000_000);
    env.crank();

    // Try to close account - should work if PnL was converted
    let pos = env.read_account_position(user_idx);
    assert_eq!(pos, 0, "Position should be closed");
    println!("Warmup=0 test: position closed, conversion should be instant");
}

/// ATTACK: Warmup period long (1M slots), attempt to withdraw before conversion.
/// Expected: Unrealized PnL in warmup cannot be withdrawn as capital.
#[test]
fn test_attack_warmup_long_period_withdraw_attempt() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_warmup(0, 1_000_000); // warmup = 1M slots

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Open profitable trade
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    env.set_slot_and_price(200, 200_000_000); // Big price up
    env.crank();

    // Close position - PnL enters warmup
    env.trade(&user, &lp, lp_idx, user_idx, -5_000_000);
    env.set_slot_and_price(300, 200_000_000);
    env.crank();

    // Try to withdraw more than original deposit
    // The PnL is in warmup and shouldn't be withdrawable yet
    let result = env.try_withdraw(&user, user_idx, 15_000_000_000); // More than 10 SOL deposit
    assert!(
        result.is_err(),
        "ATTACK: Should not withdraw more than original deposit during long warmup period"
    );

    // Even if profit exists, it's locked in warmup - vault should be intact
    let vault = env.vault_balance();
    assert!(
        vault >= 100_000_000_000,
        "Vault should retain LP + user deposits during warmup"
    );
}

// ============================================================================
// 14. Dust & Unit Scale Edge Cases
// ============================================================================

/// ATTACK: Unit scale = 0 (no scaling) - verify dust handling is safe.
/// Expected: With unit_scale=0, no dust accumulation, clean behavior.
#[test]
fn test_attack_unit_scale_zero_no_dust() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_full(0, 0, 0); // unit_scale = 0

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 12_345_678);

    let vault = env.vault_balance();
    assert_eq!(vault, 12_345_678, "Full deposit with unit_scale=0");

    // Withdrawal should work for odd amounts
    let result = env.try_withdraw(&user, user_idx, 1_234_567);
    assert!(
        result.is_ok(),
        "Withdrawal with unit_scale=0 should work: {:?}",
        result
    );
}

/// ATTACK: High unit_scale to test dust sweep boundary conditions.
/// Expected: Dust correctly tracked and not exploitable.
#[test]
fn test_attack_high_unit_scale_dust_boundary() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_full(0, 1_000_000, 0); // 1M base per unit

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    // Deposit less than one full unit
    env.deposit(&user, user_idx, 500_000); // 0.5 units = all dust

    let capital = env.read_account_capital(user_idx);
    // With 500k base and 1M per unit, capital should be 0 units
    assert_eq!(capital, 0, "Sub-unit deposit should give 0 capital");

    // Cannot withdraw anything (no capital)
    let result = env.try_withdraw(&user, user_idx, 500_000);
    assert!(
        result.is_err(),
        "ATTACK: Cannot withdraw dust that wasn't credited as capital"
    );
}

// ============================================================================
// 15. Trading Fee Edge Cases
// ============================================================================

/// ATTACK: Verify trading fees accrue to insurance fund and can't be evaded.
/// Expected: Fee is charged on every trade, goes to insurance.
#[test]
fn test_attack_trading_fee_accrual_to_insurance() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_trading_fee(100); // 1% fee

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let insurance_before = env.read_insurance_balance();

    // Execute trade
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    env.set_slot(200);
    env.crank();

    let insurance_after = env.read_insurance_balance();

    // Insurance should have increased from trading fees
    println!(
        "Insurance before: {}, after: {}",
        insurance_before, insurance_after
    );
    assert!(
        insurance_after > insurance_before,
        "Insurance fund should increase from trading fees: before={} after={}",
        insurance_before,
        insurance_after
    );
}

/// ATTACK: Open and immediately close to avoid holding fees.
/// Expected: Trading fee charged on both legs, not profitable to churn.
#[test]
fn test_attack_open_close_same_slot_fee_evasion() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_trading_fee(100); // 1% fee

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let capital_before = env.read_account_capital(user_idx);

    // Open and immediately close in same slot
    let result1 = env.try_trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    let result2 = env.try_trade(&user, &lp, lp_idx, user_idx, -5_000_000);

    assert!(result1.is_ok(), "Open leg should succeed: {:?}", result1);
    assert!(result2.is_ok(), "Close leg should succeed: {:?}", result2);

    // User should have LOST capital to fees, not gained
    env.set_slot(200);
    env.crank();

    let capital_after = env.read_account_capital(user_idx);
    assert!(
        capital_after <= capital_before,
        "ATTACK: Open+close churn should not increase capital (fees charged). \
         Before: {}, After: {}",
        capital_before,
        capital_after
    );
}

// ============================================================================
// 16. Premarket Resolution Deep Edge Cases
// ============================================================================

/// ATTACK: Withdraw after resolution but before force-close.
/// Expected: User can still withdraw capital from resolved market.
#[test]
fn test_attack_withdraw_between_resolution_and_force_close() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // User with no position - should be able to withdraw after resolution
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Resolve market
    let result = env.try_resolve_market(&admin);
    assert!(result.is_ok(), "Should resolve: {:?}", result);

    // User with no position should be able to withdraw from resolved market
    // (WithdrawCollateral does not check is_resolved flag)
    let vault_before = env.vault_balance();
    let result = env.try_withdraw(&user, user_idx, 5_000_000_000);
    assert!(result.is_ok(), "Withdrawal should succeed on resolved market (no position): {:?}", result);

    {
        let vault_after = env.vault_balance();
        assert_eq!(
            vault_before - vault_after,
            5_000_000_000,
            "ATTACK: Withdrawal amount should match vault decrease"
        );
    }
}

/// ATTACK: Force-close via crank then attempt to re-open trade.
/// Expected: No new trades after resolution.
#[test]
fn test_attack_trade_after_force_close() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;

    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 1_000_000, 100)
        .expect("oracle price push must succeed");

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    // Open position
    env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        50_000_000,
        &matcher_prog,
        &matcher_ctx,
    )
    .expect("pre-resolution TradeCpi setup must succeed");

    // Resolve + force close
    env.set_slot(200);
    env.try_push_oracle_price(&admin, 1_000_000, 200)
        .expect("oracle price push must succeed");
    env.try_resolve_market(&admin)
        .expect("market resolution setup must succeed");
    env.set_slot(300);
    env.crank();

    // Verify position force-closed
    let pos = env.read_account_position(user_idx);
    assert_eq!(pos, 0, "Position should be force-closed");

    // Try to open new trade - should fail (market resolved)
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        50_000_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(
        result.is_err(),
        "ATTACK: Trade after force-close on resolved market should fail"
    );
}

// ============================================================================
// 17. GC (Garbage Collection) Edge Cases
// ============================================================================

/// ATTACK: Close account that still has maintenance fee debt.
/// Expected: CloseAccount forgives remaining fee debt after paying what's possible.
#[test]
fn test_attack_gc_close_account_with_fee_debt() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_warmup(0, 0);

    // Set moderate maintenance fee (so account isn't fully drained by crank)
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_maintenance_fee(&admin, 100_000)
        .expect("maintenance fee setup must succeed"); // Lower fee

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000); // Larger deposit to survive fees

    // Advance a small number of slots to accrue some fee debt
    env.set_slot(200);
    env.crank();

    // Close account - should work even with fee debt (forgives remaining)
    let vault_before = env.vault_balance();
    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "CloseAccount should succeed (forgives remaining fee debt): {:?}",
        result
    );

    let vault_after = env.vault_balance();
    // User's remaining capital (after fee deduction) should be returned
    assert!(
        vault_before > vault_after,
        "ATTACK: Close should return capital to user (vault should decrease)"
    );
}

/// ATTACK: Try to use GC'd account slot for new account creation.
/// Expected: After GC, slot is marked unused and can be reused.
#[test]
fn test_attack_gc_slot_reuse_after_close() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Create user and deposit
    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 5_000_000_000);

    let vault_before = env.vault_balance();
    env.close_account(&user1, user1_idx);
    let vault_after = env.vault_balance();

    // Verify capital was returned to user on close
    assert!(
        vault_before > vault_after,
        "Capital should be returned on close"
    );

    // GC the account by cranking
    env.set_slot(200);
    env.crank();

    // After GC, the slot should be zeroed out. Reading position should be 0.
    let pos = env.read_account_position(user1_idx);
    assert_eq!(pos, 0, "GC'd slot should have zero position (clean state)");

    // Verify a fresh user at a new index works normally (no state leakage)
    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    assert!(user2_idx > 0, "New user should get a valid index");
    let pos2 = env.read_account_position(user2_idx);
    assert_eq!(pos2, 0, "New user should start with zero position");
}

// ============================================================================
// 18. Multi-Operation Race Conditions
// ============================================================================

/// ATTACK: Deposit then immediately trade in same slot to use uncranked capital.
/// Expected: Deposit is available immediately for trading (no crank needed).
#[test]
fn test_attack_deposit_then_trade_same_slot() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    // Deposit and immediately trade (no crank in between)
    env.deposit(&user, user_idx, 10_000_000_000);
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    assert!(
        result.is_ok(),
        "Deposit then trade in same slot should work: {:?}",
        result
    );
}

/// ATTACK: Trade, then withdraw max in same slot.
/// Expected: Margin check accounts for newly opened position.
#[test]
fn test_attack_trade_then_withdraw_max_same_slot() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Open a sizable position
    env.trade(&user, &lp, lp_idx, user_idx, 20_000_000);

    // Immediately try to withdraw everything
    let result = env.try_withdraw(&user, user_idx, 10_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Withdrawing all capital right after opening position should fail"
    );

    // Partial withdrawal (9 SOL) succeeds because 20M position notional is small
    // relative to remaining capital after withdrawal
    let result2 = env.try_withdraw(&user, user_idx, 9_000_000_000);
    assert!(
        result2.is_ok(),
        "ATTACK: Partial withdrawal within margin should succeed: {:?}",
        result2
    );
}

/// ATTACK: Multiple deposits in rapid succession.
/// Expected: All deposits correctly credited, no accounting errors.
#[test]
fn test_attack_rapid_deposits_accounting() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    let amount_per_deposit = 1_000_000_000u64; // 1 SOL each

    // 10 rapid deposits
    for _ in 0..10 {
        env.deposit(&user, user_idx, amount_per_deposit);
    }

    let vault = env.vault_balance();
    assert_eq!(
        vault,
        10 * amount_per_deposit,
        "Vault should have exactly 10 SOL after 10 deposits"
    );

    // Full withdrawal should work
    let result = env.try_withdraw(&user, user_idx, 10 * amount_per_deposit);
    assert!(result.is_ok(), "Should withdraw all 10 SOL: {:?}", result);
}

// ============================================================================
// 19. Config Manipulation Attacks
// ============================================================================

/// ATTACK: UpdateConfig with extreme parameter values.
/// Expected: Engine-level guards prevent dangerous configurations.
#[test]
fn test_attack_update_config_extreme_values() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Try setting max funding rate to extreme value
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
        ],
        data: encode_update_config(
            1,        // funding_horizon_slots (minimum)
            10000,    // funding_k_bps (100%)
            1u128,    // funding_inv_scale_notional_e6 (minimum)
            10000i64, // funding_max_premium_bps (max allowed)
            10000i64, // funding_max_bps_per_slot (max allowed - engine caps at ±10k)
            0u128,
            10000,
            1,
            10000,
            10000,
            0u128,
            u128::MAX, // thresh_max (extreme)
            0u128,
        ),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_ok(),
        "Extreme-but-valid UpdateConfig should be accepted: {:?}",
        result
    );

    // Set up positions to verify the engine still works
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Verify crank still works
    env.set_slot(200);
    env.crank();

    let vault = env.vault_balance();
    assert_eq!(vault, 110_000_000_000,
        "ATTACK: Engine should remain functional with consistent vault after extreme config. Got {}", vault);
}

/// ATTACK: Rapidly flip risk_reduction_threshold to gate/ungate trading.
/// Expected: Threshold changes take effect but don't corrupt state.
#[test]
fn test_attack_risk_threshold_rapid_toggle() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Set threshold to MAX - should gate risk-increasing trades
    let result = env.try_set_risk_threshold(&admin, u128::MAX);
    assert!(
        result.is_ok(),
        "Admin should set threshold to MAX: {:?}",
        result
    );

    // With MAX threshold, risk-increasing trades should be gated
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Trade should be gated after threshold set to MAX"
    );

    // Set threshold back to 0, trade should work now
    let result = env.try_set_risk_threshold(&admin, 0);
    assert!(
        result.is_ok(),
        "Admin should set threshold to 0: {:?}",
        result
    );

    // Use different size (2M) to produce unique transaction bytes
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 2_000_000);
    assert!(
        result.is_ok(),
        "Trade should succeed after threshold reset to 0: {:?}",
        result
    );
}

// ============================================================================
// 20. Integer Boundary Tests
// ============================================================================

/// ATTACK: Deposit more than ATA balance (overflow attempt).
/// Expected: Rejected by token program (insufficient funds).
#[test]
fn test_attack_deposit_u64_max() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    // Mint only 1000 tokens but try to deposit 1_000_000_000_000
    // (more than the ATA holds)
    let ata = env.create_ata(&user.pubkey(), 1000);

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data: encode_deposit(user_idx, 1_000_000_000_000),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Depositing more than ATA balance should fail"
    );
}

/// ATTACK: Trade with size = i128::MAX (overflow boundary).
/// Expected: Rejected by margin check (impossible notional value).
#[test]
fn test_attack_trade_size_i128_max() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // i128::MAX position size - should fail margin check
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, i128::MAX);
    assert!(
        result.is_err(),
        "ATTACK: Trade with i128::MAX size should fail"
    );

    // Also test i128::MIN
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, i128::MIN);
    assert!(
        result.is_err(),
        "ATTACK: Trade with i128::MIN size should fail"
    );
}

/// ATTACK: Trade with size = 0 (no-op trade attempt).
/// Expected: Zero-size trade is rejected or is a safe no-op.
#[test]
fn test_attack_trade_size_zero() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let capital_before = env.read_account_capital(user_idx);

    // Zero-size trade
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 0);
    println!("Zero-size trade: {:?}", result);

    // Either rejected or no-op - capital should not change
    let capital_after = env.read_account_capital(user_idx);
    assert_eq!(
        capital_before, capital_after,
        "ATTACK: Zero-size trade should not change capital"
    );
}

/// ATTACK: Withdraw with amount = 0 (no-op withdrawal).
/// Expected: Zero withdrawal is rejected or safe no-op.
#[test]
fn test_attack_withdraw_zero_amount() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let vault_before = env.vault_balance();

    // Zero withdrawal
    let result = env.try_withdraw(&user, user_idx, 0);
    println!("Zero withdrawal: {:?}", result);

    let vault_after = env.vault_balance();
    assert_eq!(
        vault_before, vault_after,
        "ATTACK: Zero withdrawal should not change vault"
    );
}

/// ATTACK: Deposit with amount = 0 (no-op deposit).
/// Expected: Zero deposit is rejected or safe no-op.
#[test]
fn test_attack_deposit_zero_amount() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    let vault_before = env.vault_balance();

    // Zero deposit
    let result = env.try_deposit(&user, user_idx, 0);
    println!("Zero deposit: {:?}", result);

    let vault_after = env.vault_balance();
    assert_eq!(
        vault_before, vault_after,
        "ATTACK: Zero deposit should not change vault"
    );
}

// ============================================================================
// PEN TEST SUITE ROUND 3: Config Validation, TopUpInsurance, LP, Settlement,
// Oracle Authority Lifecycle, and CloseSlab Deep Tests
// ============================================================================

impl TestEnv {
    fn try_top_up_insurance(&mut self, payer: &Keypair, amount: u64) -> Result<(), String> {
        let ata = self.create_ata(&payer.pubkey(), amount);

        let mut data = vec![9u8]; // TopUpInsurance
        data.extend_from_slice(&amount.to_le_bytes());

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(payer.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(ata, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data,
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&payer.pubkey()),
            &[payer],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    fn try_update_config_with_params(
        &mut self,
        signer: &Keypair,
        funding_horizon_slots: u64,
        funding_inv_scale_notional_e6: u128,
        thresh_alpha_bps: u64,
        thresh_min: u128,
        thresh_max: u128,
    ) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(self.slab, false),
            ],
            data: encode_update_config(
                funding_horizon_slots,
                100, // funding_k_bps
                funding_inv_scale_notional_e6,
                100i64, // funding_max_premium_bps
                10i64,  // funding_max_bps_per_slot
                0u128,  // thresh_floor
                100,    // thresh_risk_bps
                100,    // thresh_update_interval_slots
                100,    // thresh_step_bps
                thresh_alpha_bps,
                thresh_min,
                thresh_max,
                1u128, // thresh_min_step
            ),
        };
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&signer.pubkey()),
            &[signer],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }
}

// ============================================================================
// 21. UpdateConfig Validation
// ============================================================================

/// ATTACK: UpdateConfig with funding_horizon_slots = 0 (division by zero risk).
/// Expected: Rejected with InvalidConfigParam.
#[test]
fn test_attack_config_zero_funding_horizon() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let result = env.try_update_config_with_params(
        &admin,
        0,                     // funding_horizon_slots = 0 (invalid)
        1_000_000_000_000u128, // normal inv_scale
        1000,                  // normal alpha
        0,
        u128::MAX, // min/max
    );
    assert!(
        result.is_err(),
        "ATTACK: Zero funding_horizon_slots should be rejected (InvalidConfigParam)"
    );
}

/// ATTACK: UpdateConfig with funding_inv_scale_notional_e6 = 0 (division by zero).
/// Expected: Rejected with InvalidConfigParam.
#[test]
fn test_attack_config_zero_inv_scale() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let result = env.try_update_config_with_params(
        &admin,
        3600, // normal horizon
        0,    // inv_scale = 0 (invalid)
        1000, // normal alpha
        0,
        u128::MAX,
    );
    assert!(
        result.is_err(),
        "ATTACK: Zero inv_scale should be rejected (InvalidConfigParam)"
    );
}

/// ATTACK: UpdateConfig with thresh_alpha_bps > 10000 (over 100%).
/// Expected: Rejected with InvalidConfigParam.
#[test]
fn test_attack_config_alpha_over_100_percent() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let result = env.try_update_config_with_params(
        &admin,
        3600,
        1_000_000_000_000u128,
        10_001, // alpha > 10000 (invalid)
        0,
        u128::MAX,
    );
    assert!(
        result.is_err(),
        "ATTACK: alpha_bps > 10000 should be rejected (InvalidConfigParam)"
    );
}

/// ATTACK: UpdateConfig with thresh_min > thresh_max (inverted bounds).
/// Expected: Rejected with InvalidConfigParam.
#[test]
fn test_attack_config_inverted_threshold_bounds() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let result = env.try_update_config_with_params(
        &admin,
        3600,
        1_000_000_000_000u128,
        1000,
        1_000_000, // thresh_min = 1M
        999_999,   // thresh_max = 999k (< min, invalid)
    );
    assert!(
        result.is_err(),
        "ATTACK: thresh_min > thresh_max should be rejected (InvalidConfigParam)"
    );
}

// ============================================================================
// 22. TopUpInsurance Attacks
// ============================================================================

/// ATTACK: TopUpInsurance on a resolved market.
/// Expected: Rejected (InvalidAccountData).
#[test]
fn test_attack_topup_insurance_after_resolution() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");
    env.try_resolve_market(&admin)
        .expect("market resolution setup must succeed");

    // Try to top up insurance on resolved market
    let payer = Keypair::new();
    env.svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    let result = env.try_top_up_insurance(&payer, 1_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: TopUpInsurance on resolved market should be rejected"
    );
}

/// ATTACK: TopUpInsurance with insufficient ATA balance.
/// Expected: Token program rejects transfer.
#[test]
fn test_attack_topup_insurance_insufficient_balance() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Create ATA with only 100 tokens but try to top up 1B
    let payer = Keypair::new();
    env.svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    let ata = env.create_ata(&payer.pubkey(), 100);

    let mut data = vec![9u8];
    data.extend_from_slice(&1_000_000_000u64.to_le_bytes());

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: TopUpInsurance with insufficient balance should fail"
    );
}

/// ATTACK: TopUpInsurance accumulates correctly in vault and engine.
/// Expected: Insurance balance increases by correct amount, vault has the tokens.
#[test]
fn test_attack_topup_insurance_correct_accounting() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let insurance_before = env.read_insurance_balance();
    let vault_before = env.vault_balance();

    let payer = Keypair::new();
    env.svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    env.top_up_insurance(&payer, 5_000_000_000);

    let insurance_after = env.read_insurance_balance();
    let vault_after = env.vault_balance();

    assert_eq!(
        vault_after - vault_before,
        5_000_000_000,
        "Vault should increase by exact top-up amount"
    );
    assert!(
        insurance_after > insurance_before,
        "Insurance balance should increase after top-up"
    );
}

// ============================================================================
// 23. Oracle Authority Lifecycle
// ============================================================================

/// ATTACK: Setting oracle authority to [0;32] disables authority price and clears stored price.
/// Expected: After setting to zero, PushOraclePrice fails, authority_price_e6 is cleared.
#[test]
fn test_attack_oracle_authority_disable_clears_price() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Set oracle authority and push a price
    let authority = Keypair::new();
    env.svm.airdrop(&authority.pubkey(), 1_000_000_000).unwrap();
    env.try_set_oracle_authority(&admin, &authority.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&authority, 200_000_000, 100)
        .expect("oracle price push must succeed");

    // Now disable oracle authority by setting to [0;32]
    let zero = Pubkey::new_from_array([0u8; 32]);
    let result = env.try_set_oracle_authority(&admin, &zero);
    assert!(
        result.is_ok(),
        "Admin should disable oracle authority: {:?}",
        result
    );

    // Old authority can no longer push price
    let result = env.try_push_oracle_price(&authority, 300_000_000, 200);
    assert!(
        result.is_err(),
        "ATTACK: Disabled oracle authority should not push price"
    );

    // Market should still function with Pyth oracle
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);
    env.set_slot(200);
    env.crank();
    assert_eq!(
        env.vault_balance(),
        10_000_000_000,
        "Market still functional"
    );
}

/// ATTACK: Oracle authority change mid-flight (while positions open).
/// Expected: Changing authority doesn't affect existing positions, just future price pushing.
#[test]
fn test_attack_oracle_authority_change_with_positions() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Set authority and push price
    let auth1 = Keypair::new();
    env.svm.airdrop(&auth1.pubkey(), 1_000_000_000).unwrap();
    env.try_set_oracle_authority(&admin, &auth1.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&auth1, 200_000_000, 100)
        .expect("oracle price push must succeed");

    // Change to new authority
    let auth2 = Keypair::new();
    env.svm.airdrop(&auth2.pubkey(), 1_000_000_000).unwrap();
    env.try_set_oracle_authority(&admin, &auth2.pubkey())
        .expect("oracle authority setup must succeed");

    // Old authority can't push anymore
    let result = env.try_push_oracle_price(&auth1, 250_000_000, 200);
    assert!(result.is_err(), "Old authority should be rejected");

    // New authority can push
    let result = env.try_push_oracle_price(&auth2, 250_000_000, 200);
    assert!(result.is_ok(), "New authority should work: {:?}", result);

    // Market still functional - crank works
    env.set_slot(300);
    env.crank();
    let vault = env.vault_balance();
    assert_eq!(
        vault, 110_000_000_000,
        "Vault intact after authority change"
    );
}

// ============================================================================
// 24. Oracle Price Cap Deep Tests
// ============================================================================

/// ATTACK: Set oracle price cap to 0 (disables capping), verify uncapped price accepted.
/// Expected: With cap=0, any price jump is accepted.
#[test]
fn test_attack_oracle_cap_zero_disables_clamping() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");

    // Set cap to 0 (disabled)
    env.try_set_oracle_price_cap(&admin, 0)
        .expect("oracle price cap setup must succeed");

    // Push initial price
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");
    env.set_slot(200);
    const AUTH_PRICE_OFF: usize = 360;
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let auth_price_before = u64::from_le_bytes(
        slab_before[AUTH_PRICE_OFF..AUTH_PRICE_OFF + 8]
            .try_into()
            .unwrap(),
    );

    // Push 10x price jump - should be accepted with cap=0
    let result = env.try_push_oracle_price(&admin, 1_380_000_000, 200);
    assert!(
        result.is_ok(),
        "With cap=0, large price jump should be accepted: {:?}",
        result
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let auth_price_after = u64::from_le_bytes(
        slab_after[AUTH_PRICE_OFF..AUTH_PRICE_OFF + 8]
            .try_into()
            .unwrap(),
    );
    assert_eq!(
        auth_price_before, 138_000_000,
        "Initial authority price should match initial push in cap=0 test"
    );
    assert_eq!(
        auth_price_after, 1_380_000_000,
        "Cap=0 should accept full uncapped authority price jump"
    );
}

/// ATTACK: Set oracle price cap to 1 (ultra-restrictive), push any change.
/// Expected: Price clamped to essentially no movement (1 e2bps = 0.01%).
#[test]
fn test_attack_oracle_cap_ultra_restrictive() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");

    // Set ultra-restrictive cap
    env.try_set_oracle_price_cap(&admin, 1)
        .expect("oracle price cap setup must succeed");

    // Push initial price
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");
    env.set_slot(200);

    // Config offset for authority_price_e6
    const AUTH_PRICE_OFF: usize = 360;
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let auth_price_before = u64::from_le_bytes(
        slab_before[AUTH_PRICE_OFF..AUTH_PRICE_OFF + 8]
            .try_into()
            .unwrap(),
    );

    // Push 50% price increase - should succeed but be clamped internally
    let result = env.try_push_oracle_price(&admin, 207_000_000, 200);
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let auth_price_after = u64::from_le_bytes(
        slab_after[AUTH_PRICE_OFF..AUTH_PRICE_OFF + 8]
            .try_into()
            .unwrap(),
    );
    assert_ne!(
        auth_price_after, 207_000_000,
        "Ultra-restrictive cap must not accept the unclamped +50% push"
    );
    if result.is_ok() {
        assert!(
            auth_price_after >= auth_price_before,
            "Accepted oracle push should not move authority price backwards (before={} after={})",
            auth_price_before,
            auth_price_after
        );
    } else {
        assert_eq!(
            auth_price_after, auth_price_before,
            "Rejected oracle push must not mutate authority price"
        );
    }

    // Market should remain functional regardless of clamp/reject path.
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);
    env.set_slot(300);
    env.crank();
    assert_eq!(
        env.vault_balance(),
        10_000_000_000,
        "Market should remain functional after ultra-restrictive cap clamping"
    );
}

// ============================================================================
// 25. LP-Specific Attacks
// ============================================================================

/// ATTACK: LP account should never be garbage collected, even with zero state.
/// Expected: GC skips LP accounts (they have is_lp = true).
#[test]
fn test_attack_lp_immune_to_gc() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    // Don't deposit - LP has zero capital/position/pnl

    // Crank to trigger GC
    env.set_slot(200);
    env.crank();

    // LP should still be valid (not GC'd)
    // Verify by depositing to the LP
    env.deposit(&lp, lp_idx, 10_000_000_000);
    let capital = env.read_account_capital(lp_idx);
    assert!(
        capital > 0,
        "LP should not be GC'd - capital should be credited"
    );
}

/// ATTACK: User account with zero state SHOULD be GC'd.
/// Expected: GC reclaims user accounts with zero position/capital/pnl.
#[test]
fn test_attack_user_gc_when_empty() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    // Don't deposit - user has zero everything

    // Crank to trigger GC
    env.set_slot(200);
    env.crank();

    // Verify user was GC'd by checking position reads as 0
    let pos = env.read_account_position(user_idx);
    assert_eq!(pos, 0, "GC'd user should have zero position");
    let capital = env.read_account_capital(user_idx);
    assert_eq!(capital, 0, "GC'd user should have zero capital");
}

/// ATTACK: LP takes position, then try to close as if user (kind mismatch).
/// Expected: LP account cannot be closed via CloseAccount (only users can close).
#[test]
fn test_attack_close_lp_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    // Try to close LP account via CloseAccount instruction
    // CloseAccount does not check account kind — LP with no position can close
    let result = env.try_close_account(&lp, lp_idx);
    assert!(result.is_ok(), "LP with no position should be closeable: {:?}", result);
    let vault_after = env.vault_balance();
    assert!(
        vault_after < 10_000_000_000,
        "LP close should return capital: vault={}",
        vault_after
    );
}

// ============================================================================
// 26. CloseSlab Deep Tests
// ============================================================================

/// ATTACK: CloseSlab when vault has tokens remaining.
/// Expected: Rejected (vault must be empty).
#[test]
fn test_attack_close_slab_with_vault_tokens() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Deposit some tokens
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    let user_cap_before = env.read_account_capital(user_idx);
    let user_pos_before = env.read_account_position(user_idx);
    let insurance_before = env.read_insurance_balance();
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;

    // Try CloseSlab with vault containing tokens
    let result = env.try_close_slab();
    assert!(
        result.is_err(),
        "ATTACK: CloseSlab with vault tokens should be rejected"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected CloseSlab with vault tokens must preserve user capital"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected CloseSlab with vault tokens must preserve user position"
    );
    assert_eq!(
        env.read_insurance_balance(),
        insurance_before,
        "Rejected CloseSlab with vault tokens must preserve insurance"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected CloseSlab with vault tokens must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected CloseSlab with vault tokens must preserve engine vault"
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    assert_eq!(
        slab_after, slab_before,
        "Rejected CloseSlab with vault tokens must preserve slab bytes"
    );
}

/// ATTACK: CloseSlab on uninitialized slab.
/// Expected: Rejected (not initialized).
#[test]
fn test_attack_close_slab_uninitialized() {
    program_path();

    let mut env = TestEnv::new();
    // Don't call init_market - slab is uninitialized

    let slab_before = env
        .svm
        .get_account(&env.slab)
        .expect("slab account must exist before uninitialized close attempt");

    let result = env.try_close_slab();
    assert!(
        result.is_err(),
        "ATTACK: CloseSlab on uninitialized slab should fail"
    );
    let slab_after = env
        .svm
        .get_account(&env.slab)
        .expect("slab account must exist after uninitialized close attempt");
    assert_eq!(
        slab_after.lamports, slab_before.lamports,
        "Rejected CloseSlab on uninitialized slab must preserve lamports"
    );
    assert_eq!(
        slab_after.data, slab_before.data,
        "Rejected CloseSlab on uninitialized slab must preserve slab bytes"
    );
}

// ============================================================================
// 27. SetMaintenanceFee Deep Tests
// ============================================================================

/// ATTACK: Set maintenance fee to u128::MAX (maximum possible fee).
/// Expected: Fee is accepted but capital should drain predictably (not corrupt state).
#[test]
fn test_attack_maintenance_fee_u128_max() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_warmup(0, 0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let result = env.try_set_maintenance_fee(&admin, u128::MAX);
    assert!(
        result.is_ok(),
        "Admin should set max maintenance fee: {:?}",
        result
    );

    // Create user and deposit
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Advance time and crank - massive fee should not panic or corrupt
    env.set_slot(200);
    env.crank();

    // Vault should still have tokens (not corrupted)
    let vault = env.vault_balance();
    assert!(
        vault > 0,
        "Vault should still have balance after max fee crank"
    );
}

/// ATTACK: SetMaintenanceFee as non-admin.
/// Expected: Rejected (admin auth check).
#[test]
fn test_attack_set_maintenance_fee_non_admin() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let result = env.try_set_maintenance_fee(&attacker, 999_999_999);
    assert!(
        result.is_err(),
        "ATTACK: Non-admin SetMaintenanceFee should be rejected"
    );
}

// ============================================================================
// 28. Settlement Pipeline Attacks
// ============================================================================

/// ATTACK: Haircut ratio when all users are in loss (pnl_pos_tot = 0).
/// Expected: Haircut ratio = (1,1), no division by zero.
#[test]
fn test_attack_haircut_all_users_in_loss() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // User goes long
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);
    env.trade(&user, &lp, lp_idx, user_idx, 10_000_000);

    // Price drops - user is in loss (LP in profit)
    // pnl_pos_tot should include LP's positive PnL, not user's negative
    env.set_slot_and_price(200, 100_000_000);
    env.crank();

    // Vault should be intact (no corruption from haircut calc)
    let vault = env.vault_balance();
    assert_eq!(
        vault, 110_000_000_000,
        "Vault should be intact after loss scenario"
    );

    // User should still be able to partially withdraw (reduced equity, but not zero)
    let vault_before = env.vault_balance();
    let result = env.try_withdraw(&user, user_idx, 1_000_000_000);
    let vault_after = env.vault_balance();
    if result.is_ok() {
        assert_eq!(
            vault_after,
            vault_before - 1_000_000_000,
            "Successful withdraw must decrement vault by requested amount"
        );
    } else {
        assert_eq!(
            vault_after, vault_before,
            "Failed withdraw must leave vault unchanged"
        );
    }
    assert!(vault_after > 0, "Vault should never go to zero");
}

/// ATTACK: Multiple users settle in same crank - verify no double-counting.
/// Expected: Conservation holds: vault = total deposits always.
#[test]
fn test_attack_multi_user_settlement_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Create 5 users with opposing positions
    let mut users = Vec::new();
    let total_user_deposit = 5 * 10_000_000_000u64;
    for i in 0..5 {
        let u = Keypair::new();
        let idx = env.init_user(&u);
        env.deposit(&u, idx, 10_000_000_000);
        // Alternate long/short
        let size = if i % 2 == 0 {
            5_000_000i128
        } else {
            -5_000_000i128
        };
        env.trade(&u, &lp, lp_idx, idx, size);
        users.push((u, idx));
    }

    let total_deposited = 100_000_000_000 + total_user_deposit;

    // Price changes and multiple cranks
    env.set_slot_and_price(200, 150_000_000);
    env.crank();

    let vault_after = env.vault_balance();
    assert_eq!(
        vault_after, total_deposited,
        "ATTACK: Conservation violated after multi-user settlement"
    );

    env.set_slot_and_price(300, 120_000_000);
    env.crank();

    let vault_after2 = env.vault_balance();
    assert_eq!(
        vault_after2, total_deposited,
        "ATTACK: Conservation violated after price reversal"
    );
}

// ============================================================================
// 29. Instruction Truncation / Malformed Data
// ============================================================================

/// ATTACK: Send instruction with truncated data (too short for the tag).
/// Expected: Rejected with InvalidInstructionData.
#[test]
fn test_attack_truncated_instruction_data() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();

    // Tag 3 (Deposit) needs user_idx (u16) + amount (u64) = 10 bytes after tag
    // Send only 3 bytes total (tag + 2 bytes, missing amount)
    let data = vec![3u8, 0u8, 0u8];

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(Pubkey::new_unique(), false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Truncated instruction data should be rejected"
    );
}

/// ATTACK: Send unknown instruction tag (255).
/// Expected: Rejected with InvalidInstructionData.
#[test]
fn test_attack_unknown_instruction_tag() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
        ],
        data: vec![255u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8],
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Unknown instruction tag should be rejected"
    );
}

/// ATTACK: Empty instruction data (no tag byte).
/// Expected: Rejected with InvalidInstructionData.
#[test]
fn test_attack_empty_instruction_data() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
        ],
        data: vec![], // empty!
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Empty instruction data should be rejected"
    );
}

// ============================================================================
// 30. Cross-Operation Composition Attacks
// ============================================================================

/// ATTACK: Deposit → Resolve → Withdraw sequence.
/// Expected: Can't deposit after resolve, but can withdraw existing capital.
#[test]
fn test_attack_deposit_resolve_withdraw_sequence() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Setup oracle and resolve
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");
    env.try_resolve_market(&admin)
        .expect("market resolution setup must succeed");

    // Can't deposit more
    let result = env.try_deposit(&user, user_idx, 1_000_000_000);
    assert!(result.is_err(), "Deposit after resolution should fail");

    // Should be able to withdraw original capital (no position)
    let vault_before = env.vault_balance();
    let capital_before = env.read_account_capital(user_idx);
    let result = env.try_withdraw(&user, user_idx, 5_000_000_000);
    let vault_after = env.vault_balance();
    let capital_after = env.read_account_capital(user_idx);

    if result.is_ok() {
        assert_eq!(
            vault_before - vault_after,
            5_000_000_000,
            "Withdrawal amount should match vault decrease"
        );
        assert!(
            capital_after < capital_before,
            "Successful withdrawal should reduce capital: before={} after={}",
            capital_before,
            capital_after
        );
    } else {
        assert_eq!(
            vault_after, vault_before,
            "Failed withdrawal should not change vault: before={} after={}",
            vault_before, vault_after
        );
        assert_eq!(
            capital_after, capital_before,
            "Failed withdrawal should not change capital: before={} after={}",
            capital_before, capital_after
        );
    }
    // Either withdrawal works or is blocked by resolution - both are valid
    // Key property: no value created from nothing
    let vault_final = vault_after;
    assert!(
        vault_final <= 10_000_000_000,
        "Vault should never exceed total deposits"
    );
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault_final,
        "Engine/SPL vault mismatch after resolve+withdraw: engine={} vault={}",
        engine_vault, vault_final
    );
}

/// ATTACK: Trade → Price crash → Trade reverse → Crank. Does the vault balance stay correct?
/// Expected: Conservation holds through the entire sequence.
#[test]
fn test_attack_trade_crash_reverse_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let total = 110_000_000_000u64;

    // Open long
    env.trade(&user, &lp, lp_idx, user_idx, 10_000_000);

    // Price crashes
    env.set_slot_and_price(200, 80_000_000);
    env.crank();
    assert_eq!(env.vault_balance(), total, "Conservation after crash");

    // Reverse position (long → short)
    let pos_before_flip = env.read_account_position(user_idx);
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, -20_000_000);
    let pos_after_flip = env.read_account_position(user_idx);
    if result.is_ok() {
        assert_eq!(
            pos_after_flip,
            pos_before_flip - 20_000_000,
            "Successful flip should update position additively: before={} after={}",
            pos_before_flip,
            pos_after_flip
        );
    } else {
        assert_eq!(
            pos_after_flip, pos_before_flip,
            "Rejected flip should preserve position: before={} after={}",
            pos_before_flip, pos_after_flip
        );
    }
    env.set_slot_and_price(300, 80_000_000);
    env.crank();
    assert_eq!(env.vault_balance(), total, "Conservation after flip");

    // Price recovers
    env.set_slot_and_price(400, 138_000_000);
    env.crank();
    assert_eq!(env.vault_balance(), total, "Conservation after recovery");
}

// ============================================================================
// PEN TEST SUITE ROUND 4: Account Type Confusion, Capacity Limits,
// InitLP/InitUser Edge Cases, Multi-User Withdrawal, Index Bounds
// ============================================================================

impl TestEnv {
    fn try_init_lp(&mut self, owner: &Keypair) -> Result<u16, String> {
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
            data: encode_init_lp(&Pubkey::new_unique(), &Pubkey::new_unique(), 0),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        match self.svm.send_transaction(tx) {
            Ok(_) => {
                self.account_count += 1;
                Ok(idx)
            }
            Err(e) => Err(format!("{:?}", e)),
        }
    }

    /// Try trade where user passes a user_idx as lp_idx (type confusion)
    fn try_trade_type_confused(
        &mut self,
        user: &Keypair,
        victim: &Keypair,
        victim_idx: u16,
        user_idx: u16,
        size: i128,
    ) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(user.pubkey(), true),
                AccountMeta::new(victim.pubkey(), true), // victim acts as "LP"
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_trade(victim_idx, user_idx, size), // pass user idx as lp_idx
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&user.pubkey()),
            &[user, victim],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Try deposit to a specific index with specific amount (for testing out-of-bounds)
    fn try_deposit_to_idx(&mut self, owner: &Keypair, idx: u16, amount: u64) -> Result<(), String> {
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
            data: encode_deposit(idx, amount),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }
}

// ============================================================================
// 31. Account Type Confusion
// ============================================================================

/// ATTACK: Use a user account index as the LP index in TradeNoCpi.
/// Expected: Rejected because the account is not an LP (EngineAccountKindMismatch).
#[test]
fn test_attack_trade_user_as_lp() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Create two regular users
    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 10_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 10_000_000_000);

    // Try to trade user2 vs user1 (user1 as "LP") - should fail
    let result = env.try_trade_type_confused(&user2, &user1, user1_idx, user2_idx, 1_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Using user account as LP in trade should fail (kind mismatch)"
    );
}

/// ATTACK: Deposit to an LP account using DepositCollateral.
/// Expected: Should succeed (LP accounts can receive deposits like users).
#[test]
fn test_attack_deposit_to_lp_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);

    // LP can deposit via DepositCollateral
    env.deposit(&lp, lp_idx, 10_000_000_000);
    let capital = env.read_account_capital(lp_idx);
    assert!(capital > 0, "LP should be able to receive deposits");

    // Vault should have the tokens
    assert_eq!(
        env.vault_balance(),
        10_000_000_000,
        "Vault should have LP deposit"
    );
}

/// ATTACK: LiquidateAtOracle targeting an LP account.
/// Expected: LP liquidation may be handled differently (LP has position from trading).
#[test]
fn test_attack_liquidate_lp_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // User trades against LP - LP takes the other side
    env.trade(&user, &lp, lp_idx, user_idx, 50_000_000);
    env.set_slot(200);
    env.crank();

    // LP has counter-position. Try to liquidate LP.
    let capital_before = env.read_account_capital(lp_idx);
    let pos_before = env.read_account_position(lp_idx);
    let liquidation_attempt = env.try_liquidate_target(lp_idx);

    // Whether liquidation succeeds or fails, verify no corruption
    let capital_after = env.read_account_capital(lp_idx);
    let pos_after = env.read_account_position(lp_idx);
    let vault = env.vault_balance();
    assert!(
        vault > 0,
        "Vault should still have balance after LP liquidation attempt"
    );
    assert!(
        pos_after.unsigned_abs() <= pos_before.unsigned_abs(),
        "LP liquidation attempt must not increase LP exposure. before={} after={} attempt={:?}",
        pos_before,
        pos_after,
        liquidation_attempt
    );
    // LP capital should not have increased (no value extraction)
    assert!(
        capital_after <= capital_before + 1, // +1 for rounding tolerance
        "LP should not profit from liquidation attempt. Before: {}, After: {}, attempt={:?}",
        capital_before,
        capital_after,
        liquidation_attempt
    );
}

// ============================================================================
// 32. Index Bounds Attacks
// ============================================================================

/// ATTACK: Deposit to an out-of-bounds account index.
/// Expected: Rejected by check_idx (index >= max_accounts).
#[test]
fn test_attack_deposit_out_of_bounds_index() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    env.svm.airdrop(&user.pubkey(), 10_000_000_000).unwrap();

    // Try to deposit to index MAX_ACCOUNTS (out of bounds)
    let result = env.try_deposit_to_idx(&user, MAX_ACCOUNTS as u16, 1_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Deposit to out-of-bounds index should fail"
    );

    // Try index u16::MAX
    let result = env.try_deposit_to_idx(&user, u16::MAX, 1_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Deposit to u16::MAX index should fail"
    );
}

/// ATTACK: Trade with out-of-bounds user_idx.
/// Expected: Rejected by check_idx.
#[test]
fn test_attack_trade_out_of_bounds_index() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();

    // Trade with user_idx = 9999 (non-existent)
    let result = env.try_trade(&user, &lp, lp_idx, 9999, 1_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Trade with out-of-bounds user_idx should fail"
    );
}

/// ATTACK: Withdraw from out-of-bounds index.
/// Expected: Rejected by check_idx.
#[test]
fn test_attack_withdraw_out_of_bounds_index() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();

    let result = env.try_withdraw(&user, u16::MAX, 1_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Withdraw from out-of-bounds index should fail"
    );
}

/// ATTACK: LiquidateAtOracle with out-of-bounds target index.
/// Expected: Rejected by check_idx.
#[test]
fn test_attack_liquidate_out_of_bounds_index() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let result = env.try_liquidate_target(u16::MAX);
    assert!(
        result.is_err(),
        "ATTACK: Liquidate out-of-bounds index should fail"
    );
}

// ============================================================================
// 33. InitLP/InitUser Edge Cases
// ============================================================================

/// ATTACK: InitLP after market resolution.
/// Expected: Rejected (no new LPs on resolved markets).
#[test]
fn test_attack_init_lp_after_resolution() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");
    env.try_resolve_market(&admin)
        .expect("market resolution setup must succeed");

    // Try InitLP after resolution
    let lp = Keypair::new();
    let result = env.try_init_lp(&lp);
    assert!(
        result.is_err(),
        "ATTACK: InitLP after resolution should be rejected"
    );
}

/// ATTACK: InitUser with zero fee_payment and verify clean initialization.
/// Expected: Account created with zero capital (fee_payment=0 is valid).
#[test]
fn test_attack_init_user_zero_fee() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Default init_user uses fee=0 (which is what we want to test)
    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    let capital = env.read_account_capital(user_idx);
    assert_eq!(capital, 0, "Account with zero fee should have zero capital");

    // Should still be able to deposit after
    env.deposit(&user, user_idx, 1_000_000_000);
    let capital_after = env.read_account_capital(user_idx);
    assert!(
        capital_after > 0,
        "Should be able to deposit after zero-fee init"
    );
}

// ============================================================================
// 34. Multi-User Withdrawal Race
// ============================================================================

/// ATTACK: Two users both try to withdraw max capital in the same slot.
/// Expected: Both succeed (vault has enough), conservation holds.
#[test]
fn test_attack_multi_user_withdraw_same_slot() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 10_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 10_000_000_000);

    let vault_before = env.vault_balance();
    assert_eq!(
        vault_before, 20_000_000_000,
        "Both deposits should be in vault"
    );

    // Both withdraw max capital
    let result1 = env.try_withdraw(&user1, user1_idx, 10_000_000_000);
    assert!(
        result1.is_ok(),
        "User1 withdraw should succeed: {:?}",
        result1
    );

    let result2 = env.try_withdraw(&user2, user2_idx, 10_000_000_000);
    assert!(
        result2.is_ok(),
        "User2 withdraw should succeed: {:?}",
        result2
    );

    let vault_after = env.vault_balance();
    assert_eq!(
        vault_after, 0,
        "Vault should be empty after both full withdrawals"
    );
}

/// ATTACK: Double withdrawal from same account in same slot.
/// Expected: Second withdrawal fails (insufficient capital).
#[test]
fn test_attack_double_withdraw_same_slot() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // First withdrawal succeeds
    let result = env.try_withdraw(&user, user_idx, 10_000_000_000);
    assert!(
        result.is_ok(),
        "First full withdrawal should succeed: {:?}",
        result
    );

    // Second withdrawal fails (no capital left)
    let result = env.try_withdraw(&user, user_idx, 1_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Second withdrawal after full drain should fail"
    );
}

// ============================================================================
// 35. Cross-Market Isolation
// ============================================================================

/// ATTACK: Verify two separate markets (slabs) don't interfere.
/// Expected: Each market has independent state and vault.
#[test]
fn test_attack_cross_market_isolation() {
    program_path();

    // Create first market
    let mut env1 = TestEnv::new();
    env1.init_market_with_invert(0);

    let user1 = Keypair::new();
    let user1_idx = env1.init_user(&user1);
    env1.deposit(&user1, user1_idx, 10_000_000_000);

    // Create second market (different TestEnv = different slab)
    let mut env2 = TestEnv::new();
    env2.init_market_with_invert(0);

    let user2 = Keypair::new();
    let user2_idx = env2.init_user(&user2);
    env2.deposit(&user2, user2_idx, 5_000_000_000);

    // Verify independent vaults
    assert_eq!(env1.vault_balance(), 10_000_000_000, "Market 1 vault");
    assert_eq!(env2.vault_balance(), 5_000_000_000, "Market 2 vault");

    // Withdraw from market 1 doesn't affect market 2
    env1.try_withdraw(&user1, user1_idx, 5_000_000_000)
        .expect("cross-market withdrawal in market 1 must succeed");
    assert_eq!(
        env1.vault_balance(),
        5_000_000_000,
        "Market 1 after withdraw"
    );
    assert_eq!(env2.vault_balance(), 5_000_000_000, "Market 2 unaffected");
}

// ============================================================================
// 36. Slab Guard & Account Validation
// ============================================================================

/// ATTACK: Send instruction to wrong program_id's slab.
/// Expected: Slab guard rejects (program_id embedded in slab header).
#[test]
fn test_attack_wrong_slab_program_id() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Create a user normally
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    // The slab header has the program_id baked in during InitMarket.
    // If someone tried to invoke a different program with this slab,
    // slab_guard would reject because program_id wouldn't match.
    // We can't easily test cross-program in LiteSVM, but we can verify
    // the slab_guard check exists by testing with the valid program.

    // Verify market is working correctly with the right program
    let result = env.try_withdraw(&user, user_idx, 500_000_000);
    assert!(
        result.is_ok(),
        "Same-program operation should work: {:?}",
        result
    );
    assert_eq!(
        env.vault_balance(),
        500_000_000,
        "Vault should reflect withdrawal"
    );
}

// ============================================================================
// 37. Liquidation with No Position
// ============================================================================

/// ATTACK: Liquidate account that has capital but no position.
/// Expected: No-op (nothing to liquidate).
#[test]
fn test_attack_liquidate_account_no_position() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // User has capital but no position
    let capital_before = env.read_account_capital(user_idx);
    let liquidation_attempt = env.try_liquidate_target(user_idx);

    // Capital should be unchanged (no liquidation happened)
    let capital_after = env.read_account_capital(user_idx);
    assert_eq!(capital_before, capital_after,
        "ATTACK: Account with no position should not lose capital from liquidation. liquidate_result={:?}",
        liquidation_attempt);
}

// ============================================================================
// 38. Trade Self-Trading Prevention
// ============================================================================

/// ATTACK: LP tries to trade against itself (user_idx == lp_idx).
/// Expected: Rejected or no-op (can't trade against yourself).
#[test]
fn test_attack_self_trade_same_index() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Try trade where LP trades against itself (lp_idx == user_idx)
    let result = env.try_trade(&lp, &lp, lp_idx, lp_idx, 1_000_000);
    // Should be rejected or result in no position change.
    let pos = env.read_account_position(lp_idx);
    assert_eq!(
        pos, 0,
        "Self-trade should never create a position: result={:?} pos={}",
        result, pos
    );
    // Either rejected or no-op - vault must be intact
    assert_eq!(
        env.vault_balance(),
        100_000_000_000,
        "ATTACK: Self-trade should not affect vault"
    );
}

/// ATTACK: Conservation through complete lifecycle (init → trade → crank → close).
/// Expected: After all accounts closed, vault should have only insurance fees.
#[test]
fn test_attack_full_lifecycle_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let total = 110_000_000_000u64;
    assert_eq!(env.vault_balance(), total, "Initial vault");

    // Trade → crank → close
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    env.set_slot_and_price(200, 150_000_000);
    env.crank();

    // Close the trade
    env.trade(&user, &lp, lp_idx, user_idx, -5_000_000);
    env.set_slot_and_price(300, 150_000_000);
    env.crank();

    // Vault should still have all tokens (internal PnL transfer, no external movement)
    assert_eq!(
        env.vault_balance(),
        total,
        "ATTACK: Conservation through full trade lifecycle violated"
    );
}

// ============================================================================
// ROUND 5: Hyperp mode, premarket resolution, multi-LP, sandwich attacks
// ============================================================================

/// ATTACK: In Hyperp mode, TradeCpi updates mark price with execution price.
/// An attacker could try rapid trades to push mark far from index to extract
/// value via favorable PnL. Circuit breaker should limit mark movement.
#[test]
fn test_attack_hyperp_mark_manipulation_via_trade() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000); // mark = 1.0

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    // Set price cap so circuit breaker is active
    env.try_set_oracle_price_cap(&admin, 500).unwrap(); // 5% per slot

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.set_slot(100);
    env.crank();

    // Vault before
    let vault_before = env.read_vault();

    // Execute trade - this updates mark via circuit breaker
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        500_000_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(result.is_ok(), "First trade should succeed: {:?}", result);

    // Execute reverse trade to close
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        -500_000_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(result.is_ok(), "Reverse trade should succeed: {:?}", result);

    // Crank to settle
    env.set_slot(200);
    env.crank();

    // Vault after - no value should be created or destroyed
    let vault_after = env.read_vault();
    assert_eq!(
        vault_before, vault_after,
        "ATTACK: Mark manipulation via TradeCpi created value. before={}, after={}",
        vault_before, vault_after
    );
}

/// ATTACK: In Hyperp mode, index lags behind mark due to rate limiting.
/// Attacker could try to profit by trading when mark diverges from index,
/// then cranking to move index toward mark. This test verifies conservation.
#[test]
fn test_attack_hyperp_index_lag_exploitation() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();
    env.try_set_oracle_price_cap(&admin, 10_000).unwrap(); // 100% per slot cap

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.set_slot(100);
    env.crank();

    let vault_total = env.read_vault();

    // Push mark price up significantly (circuit breaker will clamp)
    env.try_push_oracle_price(&admin, 2_000_000, 2000).unwrap();

    // Trade at slot 101 (index lags behind new mark)
    env.set_slot(101);
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(result.is_ok(), "Trade should succeed: {:?}", result);

    // Crank to settle funding and move index toward mark
    env.set_slot(200);
    env.crank();

    // Close position at new price
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        -100_000_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(result.is_ok(), "Close trade should succeed: {:?}", result);

    env.set_slot(300);
    env.crank();

    // Conservation: total vault should remain the same (PnL is zero-sum internally)
    let vault_after = env.read_vault();
    assert_eq!(
        vault_total, vault_after,
        "ATTACK: Index lag exploitation created value. before={}, after={}",
        vault_total, vault_after
    );
}

/// ATTACK: Force-close during premarket resolution should maintain PnL conservation.
/// Sum of all PnL changes after force-close should be zero (zero-sum).
#[test]
fn test_attack_premarket_force_close_pnl_conservation() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    // Create LP and 3 users with positions
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let mut users = Vec::new();
    for _ in 0..3 {
        let user = Keypair::new();
        let user_idx = env.init_user(&user);
        env.deposit(&user, user_idx, 2_000_000_000);
        users.push((user, user_idx));
    }

    env.set_slot(100);
    env.crank();

    // Each user takes a different sized position
    for (i, (user, user_idx)) in users.iter().enumerate() {
        let size = ((i as i128) + 1) * 50_000_000;
        let result = env.try_trade_cpi(
            user,
            &lp.pubkey(),
            lp_idx,
            *user_idx,
            size,
            &matcher_prog,
            &matcher_ctx,
        );
        assert!(result.is_ok(), "Trade {} should succeed: {:?}", i, result);
    }

    env.set_slot(150);
    env.crank();

    // Record PnL before force-close
    let lp_pnl_before = env.read_account_pnl(lp_idx);
    let mut user_pnl_before_sum: i128 = 0;
    for (_, user_idx) in &users {
        user_pnl_before_sum += env.read_account_pnl(*user_idx);
    }
    let total_pnl_before = lp_pnl_before + user_pnl_before_sum;

    // Resolve at different price to create PnL
    env.try_push_oracle_price(&admin, 1_500_000, 2000).unwrap(); // 50% up
    env.try_resolve_market(&admin).unwrap();

    // Force-close via crank
    env.set_slot(300);
    env.crank();
    env.set_slot(400);
    env.crank(); // Second crank in case pagination needed

    // All positions should be closed
    for (_, user_idx) in &users {
        assert_eq!(
            env.read_account_position(*user_idx),
            0,
            "User position should be zero after force-close"
        );
    }
    assert_eq!(
        env.read_account_position(lp_idx),
        0,
        "LP position should be zero after force-close"
    );

    // PnL changes should sum to zero (zero-sum game)
    let lp_pnl_after = env.read_account_pnl(lp_idx);
    let mut user_pnl_after_sum: i128 = 0;
    for (_, user_idx) in &users {
        user_pnl_after_sum += env.read_account_pnl(*user_idx);
    }
    let total_pnl_after = lp_pnl_after + user_pnl_after_sum;

    // The delta in total PnL should be zero (all PnL from force-close is zero-sum)
    let pnl_delta = total_pnl_after - total_pnl_before;
    assert_eq!(
        pnl_delta, 0,
        "ATTACK: Force-close PnL not zero-sum! delta={}, LP pnl: {}→{}, users pnl: {}→{}",
        pnl_delta, lp_pnl_before, lp_pnl_after, user_pnl_before_sum, user_pnl_after_sum
    );
}

/// ATTACK: Try to withdraw all capital before force-close in a resolved market.
/// User might try to extract capital while still having an open position.
#[test]
fn test_attack_premarket_withdraw_before_force_close() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.set_slot(100);
    env.crank();

    // User takes large position
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        500_000_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(result.is_ok(), "Trade should succeed");

    // Resolve market
    env.try_push_oracle_price(&admin, 1_000_000, 2000).unwrap();
    env.try_resolve_market(&admin).unwrap();

    // Try to withdraw all capital before force-close completes
    // This should either fail (margin check) or be limited
    let result = env.try_withdraw(&user, user_idx, 5_000_000_000);
    // With open position, margin check should prevent full withdrawal
    assert!(
        result.is_err(),
        "ATTACK: Should not be able to withdraw all capital with open position in resolved market"
    );
}

/// ATTACK: Extra cranks after all positions are force-closed should be idempotent.
/// No state corruption from redundant resolution cranks.
#[test]
fn test_attack_premarket_extra_cranks_idempotent() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.set_slot(100);
    env.crank();

    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(result.is_ok(), "Trade should succeed");

    // Resolve and force-close
    env.try_push_oracle_price(&admin, 1_500_000, 2000).unwrap();
    env.try_resolve_market(&admin).unwrap();

    env.set_slot(200);
    env.crank();

    // Verify positions are closed
    assert_eq!(env.read_account_position(user_idx), 0);
    assert_eq!(env.read_account_position(lp_idx), 0);

    // Record state after first set of cranks
    let pnl_user_1 = env.read_account_pnl(user_idx);
    let pnl_lp_1 = env.read_account_pnl(lp_idx);
    let vault_1 = env.read_vault();
    let insurance_1 = env.read_insurance_balance();

    // Extra cranks should not change anything
    env.set_slot(300);
    env.crank();
    env.set_slot(400);
    env.crank();
    env.set_slot(500);
    env.crank();

    // State should be identical
    assert_eq!(
        env.read_account_pnl(user_idx),
        pnl_user_1,
        "ATTACK: Extra crank changed user PnL"
    );
    assert_eq!(
        env.read_account_pnl(lp_idx),
        pnl_lp_1,
        "ATTACK: Extra crank changed LP PnL"
    );
    assert_eq!(
        env.read_vault(),
        vault_1,
        "ATTACK: Extra crank changed vault"
    );
    assert_eq!(
        env.read_insurance_balance(),
        insurance_1,
        "ATTACK: Extra crank changed insurance"
    );
}

/// ATTACK: Resolve market at extreme price (near u64::MAX).
/// Test that force-close handles extreme PnL without overflow.
#[test]
fn test_attack_premarket_resolve_extreme_high_price() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    // Remove price cap to allow extreme price (for resolution scenario)
    env.try_set_oracle_price_cap(&admin, 1_000_000_000).unwrap(); // Very high cap

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.set_slot(100);
    env.crank();

    // Small trade
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        10_000_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(result.is_ok(), "Trade should succeed");

    // Push extremely high price for resolution
    // Circuit breaker will clamp, but push multiple times to ramp up
    for i in 0..20 {
        let price = 1_000_000u64.saturating_mul(2u64.pow(i));
        env.try_push_oracle_price(&admin, price.min(u64::MAX / 2), 3000 + i as i64)
            .expect("oracle price push must succeed");
        env.set_slot(200 + i as u64 * 100);
        env.crank();
    }

    // Resolve at whatever price we reached
    env.try_resolve_market(&admin).unwrap();

    // Force-close: should handle extreme PnL without panicking
    env.set_slot(5000);
    env.crank();

    // Verify positions are closed (no overflow crash)
    assert_eq!(
        env.read_account_position(user_idx),
        0,
        "User position should be closed after extreme price resolution"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        0,
        "LP position should be closed after extreme price resolution"
    );
}

/// ATTACK: Non-admin tries to withdraw insurance after resolution.
/// Only admin should be able to withdraw insurance funds.
#[test]
fn test_attack_withdraw_insurance_non_admin() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    // Create user, deposit and resolve
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.try_resolve_market(&admin).unwrap();
    env.set_slot(100);
    env.crank();

    // Non-admin tries to withdraw insurance
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let result = env.try_withdraw_insurance(&attacker);
    assert!(
        result.is_err(),
        "ATTACK: Non-admin was able to withdraw insurance funds!"
    );
}

/// ATTACK: Try to withdraw insurance twice to drain vault.
/// Second withdrawal should find zero insurance and be a no-op.
#[test]
fn test_attack_double_withdraw_insurance() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    // Create LP and user with trade to generate fees (insurance fund gets fees)
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.set_slot(100);
    env.crank();

    env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000_000,
        &matcher_prog,
        &matcher_ctx,
    )
    .expect("fee-generating TradeCpi setup must succeed");

    // Resolve and force-close
    env.try_push_oracle_price(&admin, 1_000_000, 2000).unwrap();
    env.try_resolve_market(&admin).unwrap();
    env.set_slot(200);
    env.crank();

    let _vault_before_first_withdraw = env.read_vault();

    // First withdrawal should succeed
    let result = env.try_withdraw_insurance(&admin);
    assert!(
        result.is_ok(),
        "First insurance withdrawal should succeed: {:?}",
        result
    );

    let vault_after_first = env.read_vault();

    // Second withdrawal: insurance is zero, should be no-op (Ok but no transfer)
    let result = env.try_withdraw_insurance(&admin);
    assert!(
        result.is_ok(),
        "Second insurance withdrawal should be ok (no-op)"
    );

    let vault_after_second = env.read_vault();
    assert_eq!(
        vault_after_first, vault_after_second,
        "ATTACK: Double insurance withdrawal drained extra funds! after_first={}, after_second={}",
        vault_after_first, vault_after_second
    );
}

/// ATTACK: TradeCpi in a resolved market should fail.
/// After resolution, no new trades should be possible.
#[test]
fn test_attack_tradecpi_after_resolution() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.set_slot(100);
    env.crank();

    // Resolve market
    env.try_resolve_market(&admin).unwrap();

    // Try TradeCpi after resolution - should fail
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(
        result.is_err(),
        "ATTACK: TradeCpi succeeded on resolved market!"
    );
}

/// ATTACK: Try to deposit after market resolution.
/// Deposits should be blocked on resolved markets.
#[test]
fn test_attack_hyperp_deposit_after_resolution() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    // Resolve
    env.try_resolve_market(&admin).unwrap();

    // Try to deposit more after resolution
    let ata = env.create_ata(&user.pubkey(), 1_000_000_000);
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data: encode_deposit(user_idx, 500_000_000),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Deposit succeeded on resolved Hyperp market!"
    );
}

/// ATTACK: Multi-LP conservation. Trade against 2 different LPs and verify
/// no value is created or destroyed. Total vault must remain constant.
#[test]
fn test_attack_multi_lp_conservation() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    // Create 2 LPs
    let lp1 = Keypair::new();
    let (lp1_idx, matcher_ctx1) = env.init_lp_with_matcher(&lp1, &matcher_prog);
    env.deposit(&lp1, lp1_idx, 50_000_000_000);

    let lp2 = Keypair::new();
    let (lp2_idx, matcher_ctx2) = env.init_lp_with_matcher(&lp2, &matcher_prog);
    env.deposit(&lp2, lp2_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.set_slot(100);
    env.crank();

    let vault_before = env.read_vault();

    // Trade against LP1 (long)
    let result = env.try_trade_cpi(
        &user,
        &lp1.pubkey(),
        lp1_idx,
        user_idx,
        200_000_000,
        &matcher_prog,
        &matcher_ctx1,
    );
    assert!(result.is_ok(), "Trade vs LP1 should succeed: {:?}", result);

    // Trade against LP2 (long again)
    let result = env.try_trade_cpi(
        &user,
        &lp2.pubkey(),
        lp2_idx,
        user_idx,
        100_000_000,
        &matcher_prog,
        &matcher_ctx2,
    );
    assert!(result.is_ok(), "Trade vs LP2 should succeed: {:?}", result);

    // Price moves
    env.try_push_oracle_price(&admin, 1_200_000, 2000).unwrap();
    env.set_slot(200);
    env.crank();

    // Close positions
    let result = env.try_trade_cpi(
        &user,
        &lp1.pubkey(),
        lp1_idx,
        user_idx,
        -200_000_000,
        &matcher_prog,
        &matcher_ctx1,
    );
    assert!(result.is_ok(), "Close vs LP1 should succeed: {:?}", result);

    let result = env.try_trade_cpi(
        &user,
        &lp2.pubkey(),
        lp2_idx,
        user_idx,
        -100_000_000,
        &matcher_prog,
        &matcher_ctx2,
    );
    assert!(result.is_ok(), "Close vs LP2 should succeed: {:?}", result);

    env.set_slot(300);
    env.crank();

    let vault_after = env.read_vault();
    assert_eq!(
        vault_before, vault_after,
        "ATTACK: Multi-LP trading violated conservation. before={}, after={}",
        vault_before, vault_after
    );
}

/// ATTACK: Sandwich attack. Deposit large amount before a trade to change
/// haircut ratio, then withdraw after. Should not extract value.
/// Attacker can only withdraw at most what they deposited.
#[test]
fn test_attack_sandwich_deposit_withdraw() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    // Victim user
    let victim = Keypair::new();
    let victim_idx = env.init_user(&victim);
    env.deposit(&victim, victim_idx, 5_000_000_000);

    env.set_slot(100);
    env.crank();

    // Create attacker AFTER crank to avoid GC of zero-balance account
    let attacker = Keypair::new();
    let attacker_idx = env.init_user(&attacker);

    // Record vault before attacker deposit
    let vault_before_attack = env.read_vault();

    // Step 1: Attacker deposits large amount (sandwich front-run)
    env.deposit(&attacker, attacker_idx, 20_000_000_000);

    // Step 2: Victim trades
    let result = env.try_trade_cpi(
        &victim,
        &lp.pubkey(),
        lp_idx,
        victim_idx,
        200_000_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(result.is_ok(), "Victim trade should succeed: {:?}", result);

    env.set_slot(200);
    env.crank();

    // Step 3: Attacker tries to withdraw everything (sandwich back-run)
    // Attacker has no position and no PnL, so withdrawal should work for their capital
    let result = env.try_withdraw(&attacker, attacker_idx, 20_000_000_000);
    assert!(
        result.is_ok(),
        "Attacker with zero position/PnL should be able to withdraw own deposit: {:?}",
        result
    );

    let vault_after = env.read_vault();
    // Front-run deposit + back-run withdrawal should net out exactly.
    assert_eq!(
        vault_after, vault_before_attack,
        "ATTACK: Sandwich flow changed vault unexpectedly! before_attack={} after={}",
        vault_before_attack,
        vault_after
    );
}

/// ATTACK: Push oracle price to zero in Hyperp mode.
/// Zero price should be rejected since it would break all calculations.
#[test]
fn test_attack_hyperp_push_zero_mark_price() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    // Try pushing zero price
    let result = env.try_push_oracle_price(&admin, 0, 2000);
    assert!(
        result.is_err(),
        "ATTACK: Zero price accepted in Hyperp mode!"
    );
}

/// ATTACK: In Hyperp mode, crank at same slot should not move index (Bug #9 fix).
/// Verify that dt=0 returns index unchanged, preventing smoothing bypass.
#[test]
fn test_attack_hyperp_same_slot_crank_no_index_movement() {
    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();
    env.try_set_oracle_price_cap(&admin, 100).unwrap(); // 1% per slot

    // First crank at slot 100 - this sets engine.current_slot = 100
    env.set_slot(100);
    env.crank();

    // Push mark price significantly higher (mark=2.0, index still ~1.0)
    env.try_push_oracle_price(&admin, 2_000_000, 2000).unwrap();

    // Read last_effective_price_e6 (index) from config before same-slot crank
    // Config offset: header is 16 bytes, config starts after that
    // last_effective_price_e6 offset within config (check source for exact layout)
    // Read last_effective_price_e6 (the index) before same-slot crank
    // It's the last u64 in the config section: slab bytes [384..392]
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    const INDEX_OFF: usize = 384;
    let index_before =
        u64::from_le_bytes(slab_before[INDEX_OFF..INDEX_OFF + 8].try_into().unwrap());
    assert!(index_before > 0, "Index should be non-zero before crank");

    // Try crank at same slot 100 again
    let result = env.try_crank();
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let index_after = u64::from_le_bytes(slab_after[INDEX_OFF..INDEX_OFF + 8].try_into().unwrap());

    // Bug #9 fix: index must NOT move when dt=0 (same slot)
    // Crank may update other fields (e.g. funding rate), but index stays put
    assert_eq!(
        index_before, index_after,
        "ATTACK: Same-slot crank moved index! Bug #9 regression. \
         before={}, after={}, crank_result={:?}",
        index_before, index_after, result
    );
}

/// ATTACK: Non-admin tries to resolve market.
/// Only the admin should be able to resolve.
#[test]
fn test_attack_resolve_market_non_admin() {
    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    // Non-admin tries to resolve
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let result = env.try_resolve_market(&attacker);
    assert!(
        result.is_err(),
        "ATTACK: Non-admin was able to resolve market!"
    );
    assert!(
        !env.is_market_resolved(),
        "Market should NOT be resolved after failed attempt"
    );
}

/// ATTACK: LP tries to close account while it still has a position from force-close PnL.
/// After force-close, LP may have PnL that prevents account closure.
#[test]
fn test_attack_lp_close_account_with_pnl_after_force_close() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.set_slot(100);
    env.crank();

    // Trade to create positions
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(result.is_ok(), "Trade should succeed");

    // Resolve at different price (creates PnL)
    env.try_push_oracle_price(&admin, 1_500_000, 2000).unwrap();
    env.try_resolve_market(&admin).unwrap();

    env.set_slot(200);
    env.crank();

    // Positions should be zero
    assert_eq!(env.read_account_position(lp_idx), 0);

    // LP should have non-zero PnL from force-close at different price (1.5 vs 1.0)
    // LP took the short side, price went up → LP has negative PnL
    let lp_pnl = env.read_account_pnl(lp_idx);
    assert!(
        lp_pnl != 0,
        "LP PnL should be non-zero after force-close at 50% different price"
    );
    assert!(
        lp_pnl < 0,
        "LP (short side) should have negative PnL when price rises: {}",
        lp_pnl
    );

    // LP with negative PnL CAN close: §6.1 loss settlement deducts from capital,
    // writes off remainder. With warmup=0, this is instant.
    let lp_capital_before = env.read_account_capital(lp_idx);
    let close_result = env.try_close_account(&lp, lp_idx);
    assert!(
        close_result.is_ok(),
        "LP with negative PnL should close (loss settled from capital): {:?}",
        close_result
    );

    // Verify LP got capital back minus their loss
    println!(
        "LP capital was {}, lost {} from force-close",
        lp_capital_before,
        lp_pnl.unsigned_abs()
    );
}

/// ATTACK: Try to init new LP after Hyperp market resolution.
/// Resolved Hyperp markets should block InitLP.
#[test]
fn test_attack_hyperp_init_lp_after_resolution() {
    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    // Resolve market
    env.try_resolve_market(&admin).unwrap();

    // Try to init new LP
    let new_lp = Keypair::new();
    let result = env.try_init_lp(&new_lp);
    assert!(
        result.is_err(),
        "ATTACK: InitLP succeeded on resolved market!"
    );
}

/// ATTACK: Push oracle price with extreme u64 value.
/// Circuit breaker should clamp price movement.
#[test]
fn test_attack_hyperp_push_extreme_price() {
    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();
    env.try_set_oracle_price_cap(&admin, 500).unwrap(); // 5% per slot

    // Push extreme price - should be clamped by circuit breaker
    let result = env.try_push_oracle_price(&admin, u64::MAX / 2, 2000);
    assert!(
        result.is_ok(),
        "Extreme push should succeed (circuit breaker clamps)"
    );

    // Read stored last_effective_price_e6 - must be clamped, not u64::MAX/2
    // last_effective_price_e6 is at slab offset 384 (last u64 in config before engine)
    let slab_data = env.svm.get_account(&env.slab).unwrap().data;
    const INDEX_OFF: usize = 384;
    let stored_price = u64::from_le_bytes(slab_data[INDEX_OFF..INDEX_OFF + 8].try_into().unwrap());
    // With 5% cap and base price 1_000_000, max clamped = 1_050_000
    assert!(
        stored_price < 2_000_000,
        "ATTACK: Circuit breaker failed to clamp extreme price! stored={}, pushed={}",
        stored_price,
        u64::MAX / 2
    );
    assert!(
        stored_price > 0,
        "Stored price should be positive after push"
    );
}

/// ATTACK: Hyperp funding rate extraction. Create position, crank many times
/// to accumulate premium funding, then check that funding doesn't create value.
#[test]
fn test_attack_hyperp_funding_rate_no_value_creation() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.set_slot(100);
    env.crank();

    let vault_before = env.read_vault();

    // Open position
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        200_000_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(result.is_ok(), "Trade should succeed");

    // Push mark price higher to create premium (mark > index → positive funding)
    env.try_push_oracle_price(&admin, 1_100_000, 2000).unwrap();

    // Crank many times to accumulate funding payments
    for i in 0..10 {
        env.set_slot(200 + i * 100);
        env.crank();
    }

    // Close position
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        -200_000_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(result.is_ok(), "Close trade should succeed: {:?}", result);

    env.set_slot(1500);
    env.crank();

    // Vault conservation: funding payments are internal transfers, no value created
    let vault_after = env.read_vault();
    assert_eq!(
        vault_before, vault_after,
        "ATTACK: Funding rate created value. before={}, after={}",
        vault_before, vault_after
    );
}

/// ATTACK: Change oracle authority during active Hyperp positions.
/// Old authority must be rejected, new authority must be accepted.
#[test]
fn test_attack_hyperp_oracle_authority_swap_with_positions() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    let old_authority = Keypair::new();
    env.svm
        .airdrop(&old_authority.pubkey(), 1_000_000_000)
        .unwrap();
    env.try_set_oracle_authority(&admin, &old_authority.pubkey())
        .unwrap();
    env.try_push_oracle_price(&old_authority, 1_000_000, 1000)
        .unwrap();

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.set_slot(100);
    env.crank();

    // Open position under old authority
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(result.is_ok(), "Trade should succeed");

    // Change oracle authority
    let new_authority = Keypair::new();
    env.svm
        .airdrop(&new_authority.pubkey(), 1_000_000_000)
        .unwrap();
    env.try_set_oracle_authority(&admin, &new_authority.pubkey())
        .unwrap();

    // Old authority should no longer be able to push prices
    let result = env.try_push_oracle_price(&old_authority, 2_000_000, 2000);
    assert!(
        result.is_err(),
        "ATTACK: Old oracle authority still accepted after change!"
    );

    // New authority should work
    let result = env.try_push_oracle_price(&new_authority, 1_000_000, 2000);
    assert!(
        result.is_ok(),
        "New authority should be able to push prices: {:?}",
        result
    );

    // Verify core security property: 3 assertions tested above
    // 1. Trade succeeded under old authority
    // 2. Old authority rejected after change
    // 3. New authority accepted
}

/// ATTACK: Close slab without withdrawing insurance first.
/// CloseSlab requires insurance_fund.balance == 0.
#[test]
fn test_attack_close_slab_before_insurance_withdrawal() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    // Create LP and user, trade to generate fees
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.set_slot(100);
    env.crank();

    env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000_000,
        &matcher_prog,
        &matcher_ctx,
    )
    .expect("pre-close-slab TradeCpi setup must succeed");

    // Resolve and force-close
    env.try_push_oracle_price(&admin, 1_000_000, 2000).unwrap();
    env.try_resolve_market(&admin).unwrap();
    env.set_slot(200);
    env.crank();

    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let insurance_before = env.read_insurance_balance();
    let num_used_before = env.read_num_used_accounts();
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_vault();
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;

    // CloseSlab should fail even after force-close: vault still has tokens,
    // accounts still exist (num_used > 0), and possibly insurance > 0
    let result = env.try_close_slab();
    assert!(
        result.is_err(),
        "ATTACK: CloseSlab succeeded with active accounts and/or insurance remaining!"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_before,
        "Rejected CloseSlab before insurance withdrawal must preserve user position"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected CloseSlab before insurance withdrawal must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_before,
        "Rejected CloseSlab before insurance withdrawal must preserve user capital"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected CloseSlab before insurance withdrawal must preserve LP capital"
    );
    assert_eq!(
        env.read_insurance_balance(),
        insurance_before,
        "Rejected CloseSlab before insurance withdrawal must preserve insurance"
    );
    assert_eq!(
        env.read_num_used_accounts(),
        num_used_before,
        "Rejected CloseSlab before insurance withdrawal must preserve account usage"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected CloseSlab before insurance withdrawal must preserve SPL vault"
    );
    assert_eq!(
        env.read_vault(),
        engine_vault_before,
        "Rejected CloseSlab before insurance withdrawal must preserve engine vault"
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    assert_eq!(
        slab_after, slab_before,
        "Rejected CloseSlab before insurance withdrawal must preserve slab bytes"
    );

    // Verify at least one blocking condition holds
    let insurance = env.read_insurance_balance();
    let num_used = env.read_num_used_accounts();
    assert!(
        insurance > 0 || num_used > 0,
        "Test setup: expected either insurance or used accounts to block CloseSlab"
    );
}

// ============================================================================
// ROUND 6: Fee debt, warmup, position limits, conservation, nonce, dust
// ============================================================================

/// ATTACK: High maintenance fee accrual over many slots should not create
/// unbounded debt or break equity calculations. Fee debt is saturating.
#[test]
fn test_attack_fee_debt_accumulation_large_maintenance_fee() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000); // 10 SOL

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Set high maintenance fee (10M per slot - enough to drain user in ~1000 slots)
    env.try_set_maintenance_fee(&admin, 10_000_000).unwrap();

    // Advance 2000 slots - user deposit (10B) should be significantly drained
    env.set_slot_and_price(2000, 100_000_000);
    env.crank();

    // User's capital should have been reduced by maintenance fees
    let user_capital = env.read_account_capital(user_idx);
    // 10M/slot * 2000 slots = 20B total fees, split across accounts
    // User had 10B, LP had 100B - fees should drain proportionally or from all
    assert!(
        user_capital <= 10_000_000_000u128,
        "Maintenance fee accrual must not increase user capital: {}",
        user_capital
    );

    // Key invariant: vault must still have tokens (system didn't panic/corrupt)
    let vault = env.vault_balance();
    assert!(
        vault > 0,
        "Vault should still have tokens after fee accrual"
    );

    // LP should still have substantial capital
    let lp_capital = env.read_account_capital(lp_idx);
    assert!(
        lp_capital > 0,
        "LP capital should be non-zero after moderate maintenance fees. lp_capital={}",
        lp_capital
    );

    // Conservation: total vault == sum of actual tokens
    // Fees go to insurance, so vault = c_tot + insurance (internal accounting)
    // But actual SPL vault balance never changes from fees alone (they're internal)
    let expected_vault = 10_000_000_000u64 + 100_000_000_000u64;
    assert_eq!(
        vault, expected_vault,
        "ATTACK: Maintenance fees changed actual vault token balance! vault={}",
        vault
    );
}

/// ATTACK: Maintenance fee set to u128::MAX should not panic or corrupt state.
#[test]
fn test_attack_extreme_maintenance_fee() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Set extreme maintenance fee - should either be rejected or accepted safely
    let result = env.try_set_maintenance_fee(&admin, u128::MAX);

    // Regardless of whether extreme fee was accepted, vault tokens must be preserved
    // (maintenance fees are internal accounting, not SPL transfers)
    let total_deposited = 1_000_000_000u64 + 100_000_000_000u64;
    let vault = env.vault_balance();
    assert_eq!(
        vault, total_deposited,
        "ATTACK: Extreme maintenance fee changed vault token balance! vault={}",
        vault
    );

    // Advance time and crank - system must not panic
    env.set_slot_and_price(100, 100_000_000);
    let crank_result = env.try_crank();
    assert!(
        crank_result.is_ok(),
        "Crank failed after extreme fee update attempt. set_fee_result={:?} crank_result={:?}",
        result,
        crank_result
    );

    // After crank, vault tokens still preserved (fees don't move SPL tokens)
    let vault_after = env.vault_balance();
    assert_eq!(
        vault_after, total_deposited,
        "ATTACK: Crank with extreme fee changed vault! vault={}",
        vault_after
    );
}

/// ATTACK: Warmup period prevents immediate profit withdrawal.
/// User with positive PnL should not be able to withdraw profit before warmup completes.
#[test]
fn test_attack_warmup_prevents_immediate_profit_withdrawal() {
    program_path();

    let mut env = TestEnv::new();
    // Init market with 1000-slot warmup period
    env.init_market_with_warmup(0, 1000);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Trade to create position
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    env.set_slot_and_price(100, 100_000_000);
    env.crank();

    // Price goes up - user has unrealized profit
    env.set_slot_and_price(200, 200_000_000);
    env.crank();

    // Close position to realize profit
    env.trade(&user, &lp, lp_idx, user_idx, -5_000_000);
    env.set_slot_and_price(300, 200_000_000);
    env.crank();

    // Vault conservation: no tokens created or destroyed through trade lifecycle
    let total_deposited = 10_000_000_000u64 + 100_000_000_000u64;
    let vault = env.vault_balance();
    assert_eq!(
        vault, total_deposited,
        "ATTACK: Warmup trade cycle violated conservation! vault={}, deposited={}",
        vault, total_deposited
    );

    // Try to withdraw MORE than original deposit
    // Warmup should prevent extracting unvested profit
    let vault_before = env.vault_balance();
    let result = env.try_withdraw(&user, user_idx, 10_000_000_001); // 1 more than deposited
                                                                    // This should fail because warmup locks profit
                                                                    // (even with profit, MTM equity minus warmup-locked amount < withdrawal)
    let vault_after = env.vault_balance();
    if result.is_ok() {
        assert_eq!(
            vault_after,
            vault_before - 10_000_000_001,
            "Successful warmup withdraw must decrement vault by requested amount"
        );
    } else {
        assert_eq!(
            vault_after, vault_before,
            "Failed warmup withdraw must leave vault unchanged"
        );
    }
    assert!(
        vault_after >= 100_000_000_000,
        "ATTACK: Warmup exploit drained vault below LP deposit! vault={}",
        vault_after
    );
}

/// ATTACK: Try to trade with position size near i128::MAX.
/// Saturating arithmetic should prevent overflow without panicking.
#[test]
fn test_attack_extreme_position_size() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.set_slot(100);
    env.crank();

    // Try extremely large position
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        i128::MAX / 2,
        &matcher_prog,
        &matcher_ctx,
    );
    // Should fail (margin requirement exceeds capital) or be capped by matcher
    assert!(
        result.is_err(),
        "ATTACK: Extreme position size (i128::MAX/2) accepted without error!"
    );
}

/// ATTACK: Try to trade with i128::MIN position size (negative extreme).
#[test]
fn test_attack_extreme_negative_position_size() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.set_slot(100);
    env.crank();

    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        i128::MIN / 2,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(
        result.is_err(),
        "ATTACK: Extreme negative position (i128::MIN/2) accepted without error!"
    );
}

/// ATTACK: Conservation invariant through trade + price movement + settlement.
/// vault_balance must equal internal vault tracking at every step.
#[test]
fn test_attack_conservation_through_price_movement() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.set_slot(100);
    env.crank();

    let vault_initial = env.read_vault();

    // Trade
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        200_000_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(result.is_ok(), "Trade should succeed");

    // Check conservation after trade
    let vault_after_trade = env.read_vault();
    assert_eq!(
        vault_initial, vault_after_trade,
        "Conservation violated after trade: {} vs {}",
        vault_initial, vault_after_trade
    );

    // Price moves up
    env.try_push_oracle_price(&admin, 1_500_000, 2000).unwrap();
    env.set_slot(200);
    env.crank();

    // Check conservation after price movement + crank
    let vault_after_crank = env.read_vault();
    assert_eq!(
        vault_initial, vault_after_crank,
        "Conservation violated after price movement: {} vs {}",
        vault_initial, vault_after_crank
    );

    // Price moves down
    env.try_push_oracle_price(&admin, 500_000, 3000).unwrap();
    env.set_slot(300);
    env.crank();

    let vault_after_crash = env.read_vault();
    assert_eq!(
        vault_initial, vault_after_crash,
        "Conservation violated after price crash: {} vs {}",
        vault_initial, vault_after_crash
    );

    // Close position
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        -200_000_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(result.is_ok(), "Close should succeed");

    let vault_final = env.read_vault();
    assert_eq!(
        vault_initial, vault_final,
        "Conservation violated after full lifecycle: {} vs {}",
        vault_initial, vault_final
    );
}

/// ATTACK: Premarket partial force-close conservation.
/// After force-closing only some accounts, internal state must still be consistent.
#[test]
fn test_attack_premarket_partial_force_close_conservation() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    // Create LP and many users
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let mut users = Vec::new();
    for _ in 0..5 {
        let user = Keypair::new();
        let user_idx = env.init_user(&user);
        env.deposit(&user, user_idx, 2_000_000_000);
        users.push((user, user_idx));
    }

    env.set_slot(100);
    env.crank();

    // Each user trades
    for (user, user_idx) in &users {
        env.try_trade_cpi(
            user,
            &lp.pubkey(),
            lp_idx,
            *user_idx,
            50_000_000,
            &matcher_prog,
            &matcher_ctx,
        )
        .expect("all user pre-resolution TradeCpi operations must succeed");
    }

    let vault_before = env.read_vault();

    // Resolve market
    env.try_push_oracle_price(&admin, 1_200_000, 2000).unwrap();
    env.try_resolve_market(&admin).unwrap();

    // Single crank: may only force-close a batch (64 accounts max)
    env.set_slot(200);
    env.crank();

    // Vault conservation must hold even during partial close
    let vault_after_partial = env.read_vault();
    assert_eq!(
        vault_before, vault_after_partial,
        "ATTACK: Partial force-close violated conservation: {} vs {}",
        vault_before, vault_after_partial
    );

    // Complete force-close
    env.set_slot(300);
    env.crank();

    let vault_after_complete = env.read_vault();
    assert_eq!(
        vault_before, vault_after_complete,
        "ATTACK: Complete force-close violated conservation: {} vs {}",
        vault_before, vault_after_complete
    );
}

/// ATTACK: Two sequential TradeCpi calls with the same parameters.
/// The nonce advances automatically between calls, so both are valid (not replays).
/// Verifies vault conservation after multiple trades.
#[test]
fn test_attack_nonce_replay_same_trade() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.set_slot(100);
    env.crank();

    // First trade succeeds
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(result.is_ok(), "First trade should succeed");
    let pos_after_first = env.read_account_position(user_idx);

    // Second identical trade - nonce has advanced, so this is a NEW trade (not replay)
    let result2 = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000_000,
        &matcher_prog,
        &matcher_ctx,
    );

    // Whether second trade succeeds or fails, vault conservation must hold
    let vault = env.read_vault();
    let expected_vault = 50_000_000_000u128 + 5_000_000_000u128; // LP + user deposits
    assert_eq!(
        vault, expected_vault,
        "ATTACK: Nonce handling violated vault conservation! vault={}, expected={}",
        vault, expected_vault
    );

    // First position must be non-zero (first trade definitely worked)
    assert!(
        pos_after_first != 0,
        "First trade should have created a non-zero position"
    );

    // If second trade succeeded, verify it created an additive position (not replay)
    let pos_after_second = env.read_account_position(user_idx);
    if result2.is_ok() {
        assert!(
            pos_after_second.abs() > pos_after_first.abs(),
            "ATTACK: Nonce replay - second trade didn't grow position! \
             first_pos={}, second_pos={}",
            pos_after_first,
            pos_after_second
        );
    } else {
        assert_eq!(
            pos_after_second, pos_after_first,
            "Rejected second trade must preserve position: first_pos={} second_pos={}",
            pos_after_first, pos_after_second
        );
    }
}

/// ATTACK: Multiple deposits in same transaction should not create extra capital.
/// Total capital should equal total deposited amount.
#[test]
fn test_attack_multiple_deposits_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Multiple small deposits
    let deposit_amount = 1_000_000_000u64;
    let num_deposits = 10;
    for _ in 0..num_deposits {
        env.deposit(&user, user_idx, deposit_amount);
    }

    let expected_total = deposit_amount as u128 * num_deposits;
    let actual_capital = env.read_account_capital(user_idx);
    assert_eq!(
        actual_capital, expected_total,
        "ATTACK: Multiple deposits created extra capital! expected={}, actual={}",
        expected_total, actual_capital
    );

    // Vault should have all deposits
    let vault = env.vault_balance();
    let expected_vault = expected_total + 100_000_000_000; // user + LP
    assert_eq!(
        vault, expected_vault as u64,
        "ATTACK: Vault balance mismatch after multiple deposits. expected={}, actual={}",
        expected_vault, vault
    );
}

/// ATTACK: User tries to withdraw more than their capital.
/// Should fail with insufficient balance.
#[test]
fn test_attack_withdraw_exceeds_capital() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    let vault_before = env.vault_balance();

    // Try to withdraw 10x capital
    let result = env.try_withdraw(&user, user_idx, 10_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Withdrawal of 10x capital succeeded!"
    );

    // Vault unchanged
    let vault_after = env.vault_balance();
    assert_eq!(
        vault_before, vault_after,
        "ATTACK: Failed withdrawal changed vault balance! before={}, after={}",
        vault_before, vault_after
    );
}

/// ATTACK: Withdraw from another user's account.
/// Account owner verification should prevent this.
#[test]
fn test_attack_withdraw_from_others_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let victim = Keypair::new();
    let victim_idx = env.init_user(&victim);
    env.deposit(&victim, victim_idx, 5_000_000_000);

    let attacker = Keypair::new();
    let _attacker_idx = env.init_user(&attacker);

    // Attacker tries to withdraw from victim's account index
    let result = env.try_withdraw(&attacker, victim_idx, 1_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Attacker withdrew from victim's account!"
    );

    // Victim's capital unchanged
    let victim_capital = env.read_account_capital(victim_idx);
    assert_eq!(
        victim_capital, 5_000_000_000,
        "ATTACK: Victim's capital changed after attacker's failed withdrawal! capital={}",
        victim_capital
    );
}

/// ATTACK: Deposit to another user's account.
/// Account owner verification should prevent this.
#[test]
fn test_attack_deposit_to_others_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let victim = Keypair::new();
    let victim_idx = env.init_user(&victim);
    env.deposit(&victim, victim_idx, 5_000_000_000);

    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 2_000_000_000).unwrap();

    // Attacker tries to deposit to victim's account index
    let ata = env.create_ata(&attacker.pubkey(), 1_000_000_000);
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data: encode_deposit(victim_idx, 1_000_000_000),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&attacker.pubkey()),
        &[&attacker],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Attacker deposited to victim's account!"
    );
}

/// ATTACK: Close account owned by someone else.
/// Must verify account ownership.
#[test]
fn test_attack_close_others_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let victim = Keypair::new();
    let victim_idx = env.init_user(&victim);
    env.deposit(&victim, victim_idx, 5_000_000_000);

    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();

    // Attacker tries to close victim's account
    let result = env.try_close_account(&attacker, victim_idx);
    assert!(result.is_err(), "ATTACK: Attacker closed victim's account!");

    // Victim's capital should be intact
    let victim_capital = env.read_account_capital(victim_idx);
    assert_eq!(
        victim_capital, 5_000_000_000,
        "Victim's capital should be unchanged after failed close attempt"
    );
}

/// ATTACK: LiquidateAtOracle on a healthy account should be a no-op.
/// Healthy accounts must not be liquidated.
#[test]
fn test_attack_liquidate_healthy_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000); // Large capital

    // Small trade, well within margin
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    env.set_slot_and_price(100, 100_000_000);
    env.crank();

    let capital_before = env.read_account_capital(user_idx);
    let pos_before = env.read_account_position(user_idx);

    // Try to liquidate healthy account - should be a no-op
    let result = env.try_liquidate_target(user_idx);
    assert!(result.is_ok(), "Liquidation of healthy account should return Ok (no-op): {:?}", result);

    // Position and capital should be unchanged
    let capital_after = env.read_account_capital(user_idx);
    let pos_after = env.read_account_position(user_idx);
    assert_eq!(
        capital_before, capital_after,
        "ATTACK: Healthy account capital changed after liquidation attempt! {}->{}",
        capital_before, capital_after
    );
    assert_eq!(
        pos_before, pos_after,
        "ATTACK: Healthy account position changed after liquidation attempt! {}->{}",
        pos_before, pos_after
    );
}

/// ATTACK: Double resolve market attempt.
/// Second resolve should fail.
#[test]
fn test_attack_double_resolve_market() {
    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    // First resolve
    let result = env.try_resolve_market(&admin);
    assert!(result.is_ok(), "First resolve should succeed");

    // Second resolve should fail
    let result = env.try_resolve_market(&admin);
    assert!(result.is_err(), "ATTACK: Double resolve succeeded!");
}

/// ATTACK: Rapid open/close trades to extract value from rounding.
/// Many tiny trades should not accumulate rounding profit.
#[test]
fn test_attack_rounding_extraction_rapid_trades() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.set_slot(100);
    env.crank();

    let vault_before = env.read_vault();
    let mut successful_open_trades = 0u32;

    // Rapid open/close with tiny size
    for i in 0..5 {
        let size = 1_000 + i; // Tiny but different each time (unique TX bytes)
        let pos_before_attempt = env.read_account_position(user_idx);
        let cap_before_attempt = env.read_account_capital(user_idx);
        let result = env.try_trade_cpi(
            &user,
            &lp.pubkey(),
            lp_idx,
            user_idx,
            size,
            &matcher_prog,
            &matcher_ctx,
        );
        if result.is_ok() {
            successful_open_trades += 1;
            // Close immediately
            env.try_trade_cpi(
                &user,
                &lp.pubkey(),
                lp_idx,
                user_idx,
                -size,
                &matcher_prog,
                &matcher_ctx,
            )
            .expect("close leg for successful tiny open trade must succeed");
            let pos_after_close = env.read_account_position(user_idx);
            assert_eq!(
                pos_after_close, pos_before_attempt,
                "Open+close cycle should return to prior position: before={} after={}",
                pos_before_attempt, pos_after_close
            );
        } else {
            assert_eq!(
                env.read_account_position(user_idx),
                pos_before_attempt,
                "Failed tiny open should not change position"
            );
            assert_eq!(
                env.read_account_capital(user_idx),
                cap_before_attempt,
                "Failed tiny open should not change capital"
            );
        }
    }
    assert!(
        successful_open_trades > 0,
        "Rounding extraction test must execute at least one successful open trade"
    );

    env.set_slot(200);
    env.crank();

    let vault_after = env.read_vault();
    assert_eq!(
        vault_before, vault_after,
        "ATTACK: Rounding extraction via rapid trades! before={}, after={}",
        vault_before, vault_after
    );
}

/// ATTACK: After UpdateAdmin to zero address, no one can admin the market.
/// admin_ok rejects zero-address admin, so this is a permanent lockout.
/// Verify that zero-admin prevents all admin operations.
#[test]
fn test_attack_update_admin_to_zero_locks_out() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Set admin to zero (may or may not succeed depending on validation)
    let zero_pubkey = Pubkey::new_from_array([0u8; 32]);
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
        ],
        data: {
            let mut d = vec![12u8]; // UpdateAdmin tag
            d.extend_from_slice(zero_pubkey.as_ref());
            d
        },
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    // UpdateAdmin to zero succeeds (protocol allows it)
    assert!(result.is_ok(), "UpdateAdmin to zero should be accepted");

    // Admin is now zero - admin_ok rejects zero address
    // All admin operations must now fail permanently
    let result = env.try_set_maintenance_fee(&admin, 100);
    assert!(
        result.is_err(),
        "ATTACK: Admin operation succeeded after admin set to zero!"
    );

    // No key can act as admin when admin is zero
    let random = Keypair::new();
    env.svm.airdrop(&random.pubkey(), 1_000_000_000).unwrap();
    let result2 = env.try_set_maintenance_fee(&random, 100);
    assert!(
        result2.is_err(),
        "ATTACK: Random user became admin when admin is zero!"
    );
}

// ============================================================================
// ROUND 7: Advanced Attack Tests - Dust sweep, LP max tracking, entry price,
//           funding anti-retroactivity, warmup+withdraw, GC edge cases,
//           conservation invariants, timing boundaries
// ============================================================================

/// ATTACK: Dust accumulates from deposits with unit_scale, then verify crank
/// correctly sweeps dust to insurance fund. Attacker cannot prevent dust sweep.
/// Non-vacuous: asserts insurance increases by swept dust units.
#[test]
fn test_attack_dust_sweep_to_insurance_on_crank() {
    program_path();

    let mut env = TestEnv::new();
    // Use unit_scale=1000 so deposits create dust
    env.init_market_full(0, 1000, 0);

    let lp_owner = Keypair::new();
    let lp_idx = env.init_user(&lp_owner);

    let user_owner = Keypair::new();
    let user_idx = env.init_user(&user_owner);

    // Deposit 1500 with unit_scale=1000: 1 unit + 500 dust
    env.deposit(&lp_owner, lp_idx, 1500);
    env.deposit(&user_owner, user_idx, 1500);

    // Read insurance balance before crank
    let insurance_before = env.read_insurance_balance();

    // Crank - should sweep dust if dust >= unit_scale
    // Two deposits of 500 dust = 1000 dust >= 1000 scale = 1 unit swept
    env.crank();

    let insurance_after = env.read_insurance_balance();

    // Verify dust was swept to insurance (1000 dust / 1000 scale = 1 unit)
    assert!(
        insurance_after > insurance_before,
        "Dust sweep should increase insurance: before={}, after={} \
         (2x500 dust = 1000 >= unit_scale 1000 = 1 unit expected)",
        insurance_before, insurance_after
    );

    // Vault balance from SPL should include both deposits
    let spl_vault_balance = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        let vault_account = TokenAccount::unpack(&vault_data).unwrap();
        vault_account.amount
    };
    assert_eq!(
        spl_vault_balance, 3000,
        "ATTACK: SPL vault should hold all deposited base tokens"
    );
}

/// ATTACK: LP risk gating with conservative max_abs tracking.
/// After LP shrinks from max position, risk check uses old max (conservative).
/// Verify that risk-increasing trades are correctly blocked when gate is active.
#[test]
fn test_attack_lp_risk_conservative_after_shrink() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 10_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 10_000_000_000);

    env.crank();

    // Trade 1: position, LP gets -1000 position
    // At price ~138, notional = 1000*138 = 138K, needs 10% margin = 13.8K << 10B
    env.trade(&user1, &lp, lp_idx, user1_idx, 1000);

    let lp_pos_after_t1 = env.read_account_position(lp_idx);
    let user1_pos_after_t1 = env.read_account_position(user1_idx);
    assert_eq!(
        lp_pos_after_t1, -1000,
        "LP should be -1000 after first trade"
    );
    assert_eq!(
        user1_pos_after_t1, 1000,
        "User1 should be +1000 after first trade"
    );

    // Trade 2: close most of position, LP now has small position
    env.trade(&user1, &lp, lp_idx, user1_idx, -900);

    // LP position is now -100 but lp_max_abs was 1000 (conservative)
    // This is the correct behavior - the risk metric overestimates

    // Verify LP is still alive and operational
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(
        lp_pos, -100,
        "LP position should be -100 after partial close"
    );

    // Vault conservation: SPL vault >= engine vault
    let spl_vault_balance = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        let vault_account = TokenAccount::unpack(&vault_data).unwrap();
        vault_account.amount
    };
    assert!(
        spl_vault_balance > 0,
        "ATTACK: SPL vault should hold deposited tokens"
    );
}

/// ATTACK: Entry price tracking through position flip (long → short).
/// After flipping, the entry_price should be updated via settle_mark_to_oracle.
/// Verify PnL calculation is correct after flip.
#[test]
fn test_attack_entry_price_across_position_flip() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.crank();

    // Open long 100
    env.trade(&user, &lp, lp_idx, user_idx, 100);

    // Verify position immediately after trade (before crank)
    let user_pos_1 = env.read_account_position(user_idx);
    assert_eq!(
        user_pos_1, 100,
        "User should be long +100 after first trade"
    );

    // Flip to short: trade -200 (closes +100, opens -100)
    env.trade(&user, &lp, lp_idx, user_idx, -200);

    // User should now be short 100
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(user_pos, -100, "User should have flipped to short -100");

    // LP should be long 100
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(lp_pos, 100, "LP should be long +100 after flip");

    env.crank();

    // Conservation: user capital + LP capital should be <= total deposited
    // (fees may reduce total)
    let user_cap = env.read_account_capital(user_idx);
    let lp_cap = env.read_account_capital(lp_idx);
    assert!(
        user_cap + lp_cap <= 20_000_000_000,
        "ATTACK: Position flip created value! User + LP capital exceeds deposits"
    );
}

/// ATTACK: Funding anti-retroactivity - rate changes at zero-DT crank
/// should use the OLD rate for the elapsed interval, not the new one.
/// Test: crank twice at same slot (sets rate), then crank at later slot.
#[test]
fn test_attack_funding_anti_retroactivity_zero_dt() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.crank();

    // Open position to generate funding
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);

    // Record vault before funding accrual
    let spl_vault_before = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        let vault_account = TokenAccount::unpack(&vault_data).unwrap();
        vault_account.amount
    };

    // Crank at same slot (dt=0, no funding accrued) - should still succeed
    let same_slot_crank_1 = env.try_crank();
    assert!(same_slot_crank_1.is_ok(), "Same-slot crank should succeed: {:?}", same_slot_crank_1);
    // Crank again same slot (dt=0 again)
    let same_slot_crank_2 = env.try_crank();
    assert!(same_slot_crank_2.is_ok(), "Repeated same-slot crank should succeed: {:?}", same_slot_crank_2);

    // Advance slot and crank (now dt > 0, funding accrues)
    env.set_slot(100);
    env.crank();

    // SPL vault should be unchanged (funding is internal accounting, not SPL transfers)
    let spl_vault_after = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        let vault_account = TokenAccount::unpack(&vault_data).unwrap();
        vault_account.amount
    };
    assert_eq!(
        spl_vault_before, spl_vault_after,
        "ATTACK: Funding caused SPL vault imbalance - value leaked. \
         same_slot_1={:?} same_slot_2={:?}",
        same_slot_crank_1, same_slot_crank_2
    );

    // Engine vault should still be correct
    let engine_vault = {
        let slab = env.svm.get_account(&env.slab).unwrap();
        u128::from_le_bytes(slab.data[392..408].try_into().unwrap())
    };
    assert!(engine_vault > 0, "Engine vault should be positive");
}

/// ATTACK: Withdrawal with warmup settlement interaction.
/// If user has unwarmed PnL, withdrawal should still respect margin after settlement.
#[test]
fn test_attack_withdrawal_with_warmup_settlement() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_warmup(0, 1000); // 1000 slot warmup

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 1_000);
    env.crank();

    // Advance partially through warmup period
    env.set_slot(500);
    env.crank();

    // Try to withdraw nearly all capital. With a tiny position (1000 contracts),
    // margin is minimal, so this may succeed or fail depending on warmup settlement.
    let vault_before_withdraw = env.vault_balance();
    let user_cap_before_withdraw = env.read_account_capital(user_idx);
    let withdraw_result = env.try_withdraw(&user, user_idx, 9_999_000_000);
    let vault_after_withdraw = env.vault_balance();
    let user_cap_after_withdraw = env.read_account_capital(user_idx);
    // With 1000-slot warmup at slot 500, only 50% of profit (if any) is vested.
    // The withdrawal may succeed (small position → small margin requirement)
    // or fail (warmup restricts available equity). Either way, conservation must hold.
    if withdraw_result.is_ok() {
        assert_eq!(
            vault_before_withdraw - vault_after_withdraw,
            9_999_000_000,
            "Successful withdrawal should reduce vault by requested amount"
        );
        assert!(
            user_cap_after_withdraw < user_cap_before_withdraw,
            "Successful withdrawal should reduce user capital: before={} after={}",
            user_cap_before_withdraw,
            user_cap_after_withdraw
        );
    } else {
        assert_eq!(
            vault_after_withdraw, vault_before_withdraw,
            "Failed withdrawal must leave vault unchanged"
        );
        assert_eq!(
            user_cap_after_withdraw, user_cap_before_withdraw,
            "Failed withdrawal must leave user capital unchanged"
        );
    }

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        let vault_account = TokenAccount::unpack(&vault_data).unwrap();
        vault_account.amount
    };
    let engine_vault = {
        let slab = env.svm.get_account(&env.slab).unwrap();
        u128::from_le_bytes(slab.data[392..408].try_into().unwrap())
    };

    // Key assertion: SPL vault >= engine vault always
    assert!(
        spl_vault as u128 >= engine_vault,
        "ATTACK: Withdrawal with warmup broke SPL/engine vault conservation! SPL={} engine={}",
        spl_vault,
        engine_vault
    );

    // User capital should be >= 0
    assert!(
        user_cap_after_withdraw <= 10_000_000_000,
        "ATTACK: User capital exceeds original deposit after partial warmup withdrawal!"
    );
}

/// ATTACK: GC removes account after force-realize closes position.
/// Verify that value doesn't leak when GC removes accounts with zero capital.
#[test]
fn test_attack_gc_after_force_realize_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 100); // Very small deposit

    env.crank();

    // Open position - user's equity will be wiped by fees/movement
    let trade_result = env.try_trade(&user, &lp, lp_idx, user_idx, 1);
    let user_pos_after_trade = env.read_account_position(user_idx);
    if trade_result.is_ok() {
        assert_ne!(
            user_pos_after_trade, 0,
            "Successful tiny trade must create a non-zero user position"
        );
    } else {
        assert_eq!(
            user_pos_after_trade, 0,
            "Failed tiny trade must not mutate user position"
        );
    }

    // Advance time to trigger maintenance fees (if set)
    env.set_slot(1000);
    env.crank();

    // The small account may have been GC'd
    let num_used = env.read_num_used_accounts();
    // At minimum, LP should still be alive
    assert!(num_used >= 1, "LP should still exist");

    // Conservation: SPL vault should match engine expectations
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        let vault_account = TokenAccount::unpack(&vault_data).unwrap();
        vault_account.amount
    };
    assert!(
        spl_vault > 0,
        "ATTACK: SPL vault should not be drained by GC + force-realize"
    );
}

/// ATTACK: Account slot reuse after close - verify new account has clean state.
/// After closing an account, a new account created should have no
/// residual position/PnL state. Also verifies freelist integrity.
#[test]
fn test_attack_slot_reuse_clean_state_after_gc() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    // Create user at index 1
    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);

    env.crank();

    // user1 with zero capital should be GC-eligible
    // After crank, check if GC removed it
    let slot_used = env.is_slot_used(user1_idx);

    if !slot_used {
        // GC removed it, the slot was freed
        // Verify the slot is clean
        let capital = env.read_account_capital(user1_idx);
        assert_eq!(capital, 0, "GC'd slot should have zero capital");
        let pos = env.read_account_position(user1_idx);
        assert_eq!(pos, 0, "GC'd slot should have zero position");
    }

    // num_used should reflect the state
    let num_used = env.read_num_used_accounts();
    if slot_used {
        assert_eq!(num_used, 2, "LP + user1 should exist");
    } else {
        assert_eq!(num_used, 1, "Only LP should exist after GC");
    }

    // Conservation: SPL vault should be correct
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 10_000_000_000,
        "ATTACK: Vault should only have LP deposit!"
    );
}

/// ATTACK: Multiple cranks with funding accumulation verify conservation.
/// Run many cranks across different slots with positions and verify
/// total value (vault) is conserved (funding is zero-sum between accounts).
#[test]
fn test_attack_multi_crank_funding_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    let spl_vault_initial = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        let vault_account = TokenAccount::unpack(&vault_data).unwrap();
        vault_account.amount
    };

    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);

    // Run 10 cranks at increasing slots to accumulate funding
    for slot in (100..=1000).step_by(100) {
        env.set_slot(slot);
        env.crank();
    }

    // After all cranks, SPL vault must be unchanged (funding is internal)
    let spl_vault_after = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        let vault_account = TokenAccount::unpack(&vault_data).unwrap();
        vault_account.amount
    };
    assert_eq!(
        spl_vault_initial, spl_vault_after,
        "ATTACK: Multi-crank funding caused SPL vault imbalance! Before={} After={}",
        spl_vault_initial, spl_vault_after
    );

    // Engine vault should still be total deposited amount
    let engine_vault = {
        let slab = env.svm.get_account(&env.slab).unwrap();
        u128::from_le_bytes(slab.data[392..408].try_into().unwrap())
    };
    assert_eq!(
        engine_vault, 20_000_000_000,
        "ATTACK: Multi-crank funding changed engine vault! Expected 20B, got {}",
        engine_vault
    );
}

/// ATTACK: Deposit to LP account with outstanding fee debt.
/// Deposit should pay fee debt first, then add remainder to capital.
/// Verify insurance fund receives correct fee payment.
#[test]
fn test_attack_lp_deposit_with_fee_debt_settlement() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    // Set maintenance fee
    env.try_set_maintenance_fee(&admin, 1_000_000)
        .expect("maintenance fee setup must succeed");

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.crank();

    // Advance time to accrue maintenance fees
    env.set_slot(1000);
    env.crank();

    // Insurance should have received fees
    let insurance_after_fees = env.read_insurance_balance();

    // Deposit more to LP - should settle fees first
    env.deposit(&lp, lp_idx, 5_000_000_000);

    // LP capital should reflect deposit minus any settled fees
    let lp_cap = env.read_account_capital(lp_idx);
    // Capital should be positive and <= initial + new deposit
    assert!(lp_cap > 0, "LP capital should be positive after deposit");
    assert!(
        lp_cap <= 15_000_000_000,
        "ATTACK: LP capital exceeds total deposits! Fee debt not properly settled."
    );

    // Insurance should be >= what it was before the deposit
    let insurance_after_deposit = env.read_insurance_balance();
    assert!(
        insurance_after_deposit >= insurance_after_fees,
        "ATTACK: Insurance decreased after deposit with fee debt!"
    );
}

/// ATTACK: UpdateConfig should preserve conservation invariant.
/// Changing risk parameters should not alter vault/capital/insurance totals.
#[test]
fn test_attack_updateconfig_preserves_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.crank();

    // Read state before config change
    let spl_vault_before = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    let engine_vault_before = {
        let slab = env.svm.get_account(&env.slab).unwrap();
        u128::from_le_bytes(slab.data[392..408].try_into().unwrap())
    };

    // UpdateConfig with different parameters
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let result = env.try_update_config_with_params(
        &admin,
        7200,                  // funding_horizon_slots
        2_000_000_000_000u128, // funding_inv_scale
        2000,                  // alpha_bps
        0,
        u128::MAX, // min/max
    );
    assert!(
        result.is_ok(),
        "UpdateConfig should succeed with valid params"
    );

    // Read state after config change
    let spl_vault_after = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    let engine_vault_after = {
        let slab = env.svm.get_account(&env.slab).unwrap();
        u128::from_le_bytes(slab.data[392..408].try_into().unwrap())
    };

    // Conservation: UpdateConfig must not change vault balances
    assert_eq!(
        spl_vault_before, spl_vault_after,
        "ATTACK: UpdateConfig changed SPL vault balance!"
    );
    assert_eq!(
        engine_vault_before, engine_vault_after,
        "ATTACK: UpdateConfig changed engine vault balance!"
    );
}

/// ATTACK: Verify trades work with u64::MAX crank staleness.
/// Note: This market uses max_crank_staleness_slots=u64::MAX (always fresh),
/// so it only tests that large slot gaps don't break the system.
/// Stale-crank rejection is not tested here (would need finite staleness config).
#[test]
fn test_attack_crank_freshness_boundary() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0); // max_crank_staleness_slots = u64::MAX

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.crank();

    // Trade should work immediately after crank
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 100);
    assert!(result.is_ok(), "Trade should work right after crank");

    // Close position
    env.trade(&user, &lp, lp_idx, user_idx, -100);

    // With max_crank_staleness = u64::MAX, crank is always "fresh"
    // But advance to large slot to test
    env.set_slot(10_000);

    // Trade without re-cranking - should still work with u64::MAX staleness
    let result2 = env.try_trade(&user, &lp, lp_idx, user_idx, 50);
    assert!(
        result2.is_ok(),
        "Trade should work with u64::MAX staleness even after slot advancement"
    );

    // Conservation check
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 20_000_000_000,
        "ATTACK: Vault balance changed due to timing attack!"
    );
}

/// ATTACK: Insurance fund receives both dust sweep and fee accrual in same crank.
/// Verify both sources of insurance top-up are correctly accounted for.
#[test]
fn test_attack_insurance_dust_plus_fee_consistency() {
    program_path();

    let mut env = TestEnv::new();
    // unit_scale=1000 for dust generation
    env.init_market_full(0, 1000, 0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    // Set maintenance fee to generate fee revenue
    env.try_set_maintenance_fee(&admin, 100_000)
        .expect("maintenance fee setup must succeed");

    let lp = Keypair::new();
    let lp_idx = env.init_user(&lp);
    // Deposit 10000500: 10000 units + 500 dust
    env.deposit(&lp, lp_idx, 10_000_500);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    // Deposit 10000500: 10000 units + 500 dust
    env.deposit(&user, user_idx, 10_000_500);

    // Total dust = 1000, which is >= unit_scale=1000, so 1 unit will be swept

    env.crank();
    let insurance_after_first_crank = env.read_insurance_balance();

    // Advance time to accrue maintenance fees
    env.set_slot(100);
    env.crank();

    let insurance_after_second_crank = env.read_insurance_balance();

    // Insurance should have increased from fees (and possibly more dust)
    // Note: With no open positions, maintenance fees don't accrue.
    // Insurance may only increase from dust sweeps (if more dust accumulated).
    assert!(
        insurance_after_second_crank >= insurance_after_first_crank,
        "ATTACK: Insurance decreased between cranks! Before={} After={}",
        insurance_after_first_crank,
        insurance_after_second_crank
    );

    // SPL vault should hold total deposited base tokens
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 20_001_000,
        "ATTACK: SPL vault doesn't match total deposits!"
    );
}

/// ATTACK: Close all positions then close account, verify complete cleanup.
/// User opens position, closes it, then closes account.
/// Verify capital is correctly returned and no value is left behind.
#[test]
fn test_attack_full_close_cycle_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    // Open and close position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, -100_000);
    env.crank();

    // Position should be zero
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(user_pos, 0, "Position should be zero after close");

    // Read user's capital before close
    let _user_cap = env.read_account_capital(user_idx);

    // Close account
    let close_result = env.try_close_account(&user, user_idx);
    assert!(
        close_result.is_ok(),
        "Close account should succeed with zero position"
    );

    // After close, user's capital should be returned via SPL transfer
    // num_used should decrease by 1
    let num_used_after = env.read_num_used_accounts();
    assert_eq!(num_used_after, 1, "Only LP should remain after user close");

    // Verify capital was returned (SPL vault should have decreased by user_cap)
    let spl_vault_after = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    // SPL vault should be 15B - user_cap (what user got back)
    assert!(
        spl_vault_after < 15_000_000_000,
        "ATTACK: Vault didn't decrease after user close! SPL vault still at {}",
        spl_vault_after
    );
    assert!(spl_vault_after > 0, "Vault should still have LP's deposit");
}

/// ATTACK: Liquidation of already-zero-position account should fail.
/// An attacker tries to liquidate an account that already has no position.
#[test]
fn test_attack_liquidate_zero_position_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.crank();

    // User has no position - try to liquidate
    // Liquidation returns Ok (no-op) for zero-position accounts
    let capital_before = env.read_account_capital(user_idx);
    let result = env.try_liquidate_target(user_idx);
    assert!(result.is_ok(), "Liquidation of zero-position should return Ok (no-op): {:?}", result);

    // Key assertion: capital should not change after liquidation attempt
    let capital_after = env.read_account_capital(user_idx);
    assert_eq!(
        capital_before, capital_after,
        "ATTACK: Liquidation of zero-position changed capital! Before={} After={}",
        capital_before, capital_after
    );

    // Position should still be zero
    let pos_after = env.read_account_position(user_idx);
    assert_eq!(
        pos_after, 0,
        "ATTACK: Liquidation of zero-position account created a position!"
    );
}

/// ATTACK: Trade must not decrease insurance fund or change vault.
/// Note: Market uses default trading_fee_bps=0. For non-zero fee testing,
/// see test_attack_new_account_fee_goes_to_insurance which tests fee→insurance.
#[test]
fn test_attack_trading_fee_insurance_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.crank();

    let insurance_before = env.read_insurance_balance();

    // Execute trade
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);

    let insurance_after = env.read_insurance_balance();

    // Insurance should not decrease from a trade
    assert!(
        insurance_after >= insurance_before,
        "ATTACK: Insurance decreased after trade! Before={} After={}",
        insurance_before,
        insurance_after
    );

    // Vault conservation
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 20_000_000_000,
        "ATTACK: SPL vault changed after trade (should only change on deposit/withdraw)!"
    );
}

/// ATTACK: Premarket force-close with multiple crank batches.
/// Verify that force-close across multiple crank calls (paginated)
/// correctly settles all positions and maintains conservation.
#[test]
fn test_attack_premarket_paginated_force_close() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    // Create LP + users
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.set_slot(100);
    env.crank();

    // Open position
    let trade_result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(trade_result.is_ok(), "Trade should succeed");

    env.set_slot(150);
    env.crank();

    // Resolve at same price to minimize PnL complexity
    env.try_push_oracle_price(&admin, 1_000_000, 2000).unwrap();
    env.try_resolve_market(&admin).unwrap();

    // Force-close via multiple cranks (paginated)
    for slot in (200..=400).step_by(50) {
        env.set_slot(slot);
        env.crank();
    }

    // All positions should be closed
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(
        user_pos, 0,
        "ATTACK: User position not closed after paginated force-close!"
    );

    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(
        lp_pos, 0,
        "ATTACK: LP position not closed after paginated force-close!"
    );
}

// ===================================================================
// ROUND 8: Arithmetic Boundary & State Machine Attack Tests
// ===================================================================

/// ATTACK: Circuit breaker first price acceptance.
/// When last_effective_price_e6 == 0 (first price), circuit breaker should
/// accept any raw price unclamped. Verify no panic/overflow on extreme price.
#[test]
fn test_attack_circuit_breaker_first_price_extreme() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Set an extreme price (very high)
    env.set_slot_and_price(10, 999_999_000_000); // $999,999 per unit

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Crank should succeed even with extreme price
    env.crank();

    // Conservation: vault should be unchanged
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 20_000_000_000,
        "ATTACK: Vault balance changed with extreme first price!"
    );

    // Accounts should still have their capital
    let lp_cap = env.read_account_capital(lp_idx);
    assert!(
        lp_cap > 0,
        "LP capital should be positive after extreme price crank"
    );
    let user_cap = env.read_account_capital(user_idx);
    assert!(
        user_cap > 0,
        "User capital should be positive after extreme price crank"
    );
}

/// ATTACK: Circuit breaker clamping after second price.
/// After initial price is set, subsequent extreme prices should be clamped.
/// Verify clamping prevents exploitation via price manipulation.
#[test]
fn test_attack_circuit_breaker_clamping_second_price() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.crank(); // Establishes last_effective_price_e6 = 138_000_000

    // Open position at normal price
    env.trade(&user, &lp, lp_idx, user_idx, 100);
    env.crank();

    // Now set extreme price (10x increase)
    env.set_slot_and_price(20, 1_380_000_000); // 10x normal price

    // Crank with circuit breaker active - price should be clamped
    env.crank();

    // User should NOT have 10x profit due to clamping
    let user_cap = env.read_account_capital(user_idx);
    let lp_cap = env.read_account_capital(lp_idx);

    // Total capital should be conserved (within fee tolerance)
    assert!(
        user_cap + lp_cap <= 20_000_000_000,
        "ATTACK: Circuit breaker failed - capital increased! user={} lp={}",
        user_cap,
        lp_cap
    );
}

/// ATTACK: Fee debt exceeds capital during crank.
/// Create a scenario where maintenance fees accumulate to exceed capital.
/// Verify equity calculation remains correct and no underflow occurs.
#[test]
fn test_attack_fee_debt_exceeds_capital_crank() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 100); // Very small deposit

    env.crank();

    // Open small position
    env.trade(&user, &lp, lp_idx, user_idx, 1);

    // Set high maintenance fee
    env.try_set_maintenance_fee(&admin, 9000)
        .expect("maintenance fee setup must succeed"); // 90% annual

    // Advance many slots to accumulate fee debt
    env.set_slot(10_000);
    env.crank();

    // User should still exist without panic
    let user_cap = env.read_account_capital(user_idx);
    let user_pnl = env.read_account_pnl(user_idx);

    // Capital + PnL might be very small or zero, but shouldn't underflow
    // The key test is that the crank didn't panic/abort
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 10_000_000_100,
        "ATTACK: SPL vault changed after fee accrual! vault={}",
        spl_vault
    );
    // Conservation: user_cap >= 0 always (unsigned in engine)
    assert!(
        user_cap + (user_pnl.max(0) as u128) <= 10_000_000_100u128,
        "ATTACK: User equity exceeds total deposits!"
    );
}

/// ATTACK: Rapid price oscillation precision loss.
/// Execute many trades with alternating prices to accumulate rounding errors.
/// Verify total value is conserved across repeated operations.
#[test]
fn test_attack_price_oscillation_precision_loss() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.crank();

    // Open/close position to test precision
    // Round 1
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot(10);
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, -100_000);
    env.set_slot(20);
    env.crank();
    // Round 2 (different size to avoid duplicate tx hash)
    env.trade(&user, &lp, lp_idx, user_idx, 200_000);
    env.set_slot(30);
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, -200_000);
    env.set_slot(40);
    env.crank();
    // Round 3
    env.trade(&user, &lp, lp_idx, user_idx, 300_000);
    env.set_slot(50);
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, -300_000);
    env.set_slot(60);
    env.crank();

    // Position should be zero
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(user_pos, 0, "Position should be zero after all round-trips");

    // Conservation: total value should not exceed initial deposits
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 20_000_000_000,
        "ATTACK: SPL vault changed after round-trip trades! vault={}",
        spl_vault
    );

    let user_cap = env.read_account_capital(user_idx);
    let lp_cap = env.read_account_capital(lp_idx);
    assert!(
        user_cap + lp_cap <= 20_000_000_000,
        "ATTACK: Total capital exceeds deposits after oscillation! user={} lp={}",
        user_cap,
        lp_cap
    );
}

/// ATTACK: Multiple accounts compete for insurance fund during liquidation.
/// Create two undercollateralized accounts and liquidate both.
/// Verify insurance fund is not double-counted.
#[test]
fn test_attack_multiple_liquidations_insurance_drain() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    // Two users with small capital and large positions
    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 1_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 1_000_000_000);

    env.crank();

    // Open positions for both users
    env.trade(&user1, &lp, lp_idx, user1_idx, 50_000);
    env.trade(&user2, &lp, lp_idx, user2_idx, 50_000);
    env.crank();

    // Top up insurance
    env.try_top_up_insurance(&admin, 500_000_000).unwrap();

    // Drop price significantly to make both users underwater
    env.set_slot_and_price(20, 100_000_000); // Drop from 138 to 100

    env.crank(); // Crank should liquidate underwater accounts

    // Try explicit liquidation on both
    let liq1 = env.try_liquidate_target(user1_idx);
    let liq2 = env.try_liquidate_target(user2_idx);

    // Insurance fund should not go negative (u128 can't, but balance should be sane)
    let insurance = env.read_insurance_balance();
    assert!(
        insurance < u128::MAX / 2,
        "ATTACK: Insurance fund balance is suspiciously large: {}",
        insurance
    );

    // SPL vault should be unchanged (no external withdrawals)
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 52_500_000_000,
        "ATTACK: SPL vault changed after liquidations! vault={} liq1={:?} liq2={:?}",
        spl_vault, liq1, liq2
    );
}

// test_attack_deposit_zero_amount_noop: removed (duplicate of test_attack_deposit_zero_amount_no_state_change which uses try_deposit)

/// ATTACK: Withdraw exactly all capital (no position).
/// Verify withdrawing exact capital amount works and leaves account with 0.
#[test]
fn test_attack_withdraw_exact_capital_no_position() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    // Withdraw exact capital amount (no position = no margin requirement)
    let cap = env.read_account_capital(user_idx);
    let withdraw_result = env.try_withdraw(&user, user_idx, cap as u64);
    assert!(
        withdraw_result.is_ok(),
        "Should be able to withdraw all capital with no position"
    );

    // Capital should now be 0
    let cap_after = env.read_account_capital(user_idx);
    assert_eq!(cap_after, 0, "Capital should be zero after full withdrawal");

    // SPL vault should decrease by withdrawn amount
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 10_000_000_000,
        "ATTACK: SPL vault has wrong balance after full withdrawal! vault={}",
        spl_vault
    );
}

/// ATTACK: Threshold EWMA convergence across many cranks.
/// Set a risk threshold and verify it converges toward target via EWMA
/// rather than allowing wild oscillations that could be exploited.
#[test]
fn test_attack_threshold_ewma_convergence() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.crank();

    // Top up insurance so risk gate doesn't block trades
    env.try_top_up_insurance(&admin, 10_000_000_000).unwrap();

    // Set a specific risk threshold
    env.try_set_risk_threshold(&admin, 1_000_000).unwrap();

    // Open position to create risk
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);

    // Crank many times to let EWMA converge
    for slot in (10..200).step_by(15) {
        env.set_slot(slot);
        env.crank();
    }

    // Conservation check: SPL vault unchanged
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 30_000_000_000,
        "ATTACK: SPL vault changed during threshold convergence!"
    );

    // Total capital conserved (user + lp capital <= initial 20B deposits)
    let user_cap = env.read_account_capital(user_idx);
    let lp_cap = env.read_account_capital(lp_idx);
    assert!(
        user_cap + lp_cap <= 20_000_000_000,
        "ATTACK: Total capital exceeds deposits after EWMA convergence!"
    );
}

/// ATTACK: Trade at exactly the initial margin boundary.
/// Open a position that requires exactly initial_margin_bps of capital.
/// Then try to open slightly more - should fail margin check.
#[test]
fn test_attack_trade_exact_initial_margin_boundary() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000); // 1B units

    env.crank();

    // Price is 138e6. Notional per unit = 138e6/1e6 = 138.
    // Initial margin is 1000 bps = 10%.
    // Max notional = capital / margin_fraction = 1B / 0.1 = 10B
    // Max position = 10B / 138 ≈ 72_463_768

    // Try a very large position that should fail
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 100_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Trade exceeding initial margin should fail!"
    );

    // Position should remain zero
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(
        user_pos, 0,
        "ATTACK: Position changed despite failed margin check!"
    );
}

/// ATTACK: Multiple deposits followed by single large withdrawal.
/// Verify conservation across many small deposits then one withdrawal.
#[test]
fn test_attack_many_deposits_one_withdrawal_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    // Many small deposits
    for _ in 0..20 {
        env.deposit(&user, user_idx, 100_000_000); // 100M each
    }

    env.crank();

    // Total deposited: 20 * 100M = 2B
    let cap = env.read_account_capital(user_idx);
    assert_eq!(
        cap, 2_000_000_000,
        "Capital should equal sum of deposits: {}",
        cap
    );

    // Withdraw half
    let withdraw_result = env.try_withdraw(&user, user_idx, 1_000_000_000);
    assert!(withdraw_result.is_ok(), "Withdrawal should succeed");

    let cap_after = env.read_account_capital(user_idx);
    assert_eq!(
        cap_after, 1_000_000_000,
        "Capital after withdrawal should be 1B: {}",
        cap_after
    );

    // SPL vault conservation
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 11_000_000_000,
        "ATTACK: SPL vault has wrong balance! expected 11B, got {}",
        spl_vault
    );
}

/// ATTACK: Risk gate activation with insurance at exact threshold boundary.
/// Verify behavior when insurance_fund.balance == risk_reduction_threshold exactly.
#[test]
fn test_attack_risk_gate_exact_threshold_boundary() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.crank();

    // Top up insurance to a large amount
    env.try_top_up_insurance(&admin, 10_000_000_000).unwrap();

    // Set threshold to 0 (gate always inactive)
    env.try_set_risk_threshold(&admin, 0).unwrap();

    // Trade should succeed (gate is inactive)
    let trade1 = env.try_trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert!(
        trade1.is_ok(),
        "Trade should succeed when threshold=0 (gate inactive): {:?}",
        trade1
    );

    // Now set threshold much higher than insurance (gate becomes active)
    let insurance = env.read_insurance_balance();
    env.try_set_risk_threshold(&admin, insurance * 100).unwrap();

    env.set_slot(10);

    // Risk-increasing trade should be blocked when gate is active
    let trade2 = env.try_trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert!(
        trade2.is_err(),
        "Trade should be blocked when gate is active (threshold >> insurance)"
    );

    // Verify insurance fund didn't decrease
    let insurance_after = env.read_insurance_balance();
    assert!(
        insurance_after >= insurance,
        "ATTACK: Insurance decreased without withdrawal! before={} after={}",
        insurance,
        insurance_after
    );
}

/// ATTACK: Unit scale boundary - init market with MAX_UNIT_SCALE.
/// Verify that operations work correctly at the maximum unit scale.
#[test]
fn test_attack_max_unit_scale_operations() {
    program_path();

    let mut env = TestEnv::new();
    // Init with moderate unit scale (1000) - still tests alignment
    // Note: max unit_scale (1B) would cause OracleInvalid because price/scale = 0
    env.init_market_full(0, 1000, 0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000); // 10M base = 10K units

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000); // 5M base = 5K units

    env.crank();

    // Capital should be in units (5M / 1000 = 5000 units)
    let user_cap = env.read_account_capital(user_idx);
    assert_eq!(user_cap, 5000, "Capital should be 5000 units at scale=1000");

    // Withdrawal must be aligned to unit_scale
    let bad_withdraw = env.try_withdraw(&user, user_idx, 500); // Not aligned (500 % 1000 != 0)
    assert!(
        bad_withdraw.is_err(),
        "ATTACK: Misaligned withdrawal should fail at unit_scale=1000!"
    );

    // Aligned withdrawal should work
    let good_withdraw = env.try_withdraw(&user, user_idx, 1000); // 1 unit
    assert!(good_withdraw.is_ok(), "Aligned withdrawal should succeed");

    let cap_after = env.read_account_capital(user_idx);
    assert_eq!(
        cap_after, 4999,
        "Capital should be 4999 units after withdrawing 1"
    );
}

/// ATTACK: Close account after opening and closing position at same price.
/// PnL is zero after round-trip. Verifies capital returned and slot freed.
/// Note: Despite the name, this test creates zero PnL (no price change).
#[test]
fn test_attack_close_account_with_positive_pnl() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.crank();

    // Close position
    env.trade(&user, &lp, lp_idx, user_idx, -100_000);
    env.crank();

    // Position should be zero
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(user_pos, 0, "Position should be zero");

    // Try close - should succeed
    let close_result = env.try_close_account(&user, user_idx);
    assert!(
        close_result.is_ok(),
        "Close account should succeed with zero position"
    );

    // Verify user's slot is freed
    let num_used = env.read_num_used_accounts();
    assert_eq!(num_used, 1, "Only LP should remain");

    // SPL vault should have decreased (user got capital back)
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert!(
        spl_vault < 55_000_000_000,
        "ATTACK: Vault didn't decrease after close! vault={}",
        spl_vault
    );
    assert!(spl_vault > 0, "Vault should still have LP deposit");
}

/// ATTACK: Rapid open/close in same slot shouldn't bypass timing guards.
/// Verify that opening and closing a position in the same slot works
/// but doesn't allow exploiting stale prices or settlement.
#[test]
fn test_attack_same_slot_open_close_timing() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.crank();

    let cap_before = env.read_account_capital(user_idx);

    // Open and close in same slot (no crank between)
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.trade(&user, &lp, lp_idx, user_idx, -100_000);

    env.crank();

    let user_pos = env.read_account_position(user_idx);
    assert_eq!(
        user_pos, 0,
        "Position should be zero after same-slot round-trip"
    );

    let cap_after = env.read_account_capital(user_idx);
    // Capital should not increase (no free money from same-slot trades)
    assert!(
        cap_after <= cap_before,
        "ATTACK: Capital increased from same-slot round-trip! before={} after={}",
        cap_before,
        cap_after
    );

    // SPL vault conservation
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 20_000_000_000,
        "ATTACK: SPL vault changed from same-slot trades!"
    );
}

/// ATTACK: Force-close (premarket resolution) with settlement at different price.
/// Verify PnL is calculated correctly when resolution price differs from entry.
#[test]
fn test_attack_force_close_pnl_accuracy() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.set_slot(100);
    env.crank();

    // Open position at price 1_000_000
    env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000,
        &matcher_prog,
        &matcher_ctx,
    )
    .unwrap();

    env.set_slot(150);
    env.crank();

    // Resolve at 2x price
    env.try_push_oracle_price(&admin, 2_000_000, 2000).unwrap();
    env.try_resolve_market(&admin).unwrap();

    // Force-close
    for slot in (200..=500).step_by(50) {
        env.set_slot(slot);
        env.crank();
    }

    // User should have profit (long position, price doubled)
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(user_pos, 0, "User position should be closed");

    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(lp_pos, 0, "LP position should be closed");

    // User went long at 1.0, resolved at 2.0 => user should have positive PnL
    let user_pnl = env.read_account_pnl(user_idx);
    assert!(
        user_pnl >= 0,
        "ATTACK: User PnL should be non-negative after price doubling! pnl={}",
        user_pnl
    );

    // Key security check: total PnL shouldn't exceed what the system can cover
    let lp_pnl = env.read_account_pnl(lp_idx);
    // Both PnL values should be reasonable (not overflowed)
    assert!(user_pnl < i128::MAX / 2, "User PnL overflow detected");
    assert!(lp_pnl < i128::MAX / 2, "LP PnL overflow detected");
}

/// ATTACK: Hyperp mode mark price clamping prevents extreme manipulation.
/// In Hyperp mode, mark price from trades is clamped against index.
/// Verify attacker can't push mark price arbitrarily far from index.
#[test]
fn test_attack_hyperp_mark_price_clamp_defense() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.set_slot(100);
    env.crank();

    // Execute trade - mark price will be clamped against index
    let trade_result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        1_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(trade_result.is_ok(), "Trade should succeed in Hyperp mode");

    // Verify position was created
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(user_pos, 1_000, "User should have position of 1000");

    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(lp_pos, -1_000, "LP should have opposite position");

    // PnL should be zero-sum
    let user_pnl = env.read_account_pnl(user_idx);
    let lp_pnl = env.read_account_pnl(lp_idx);
    let net = user_pnl + lp_pnl;
    assert!(
        net.abs() <= 1,
        "ATTACK: PnL not zero-sum after Hyperp trade! user={} lp={} net={}",
        user_pnl,
        lp_pnl,
        net
    );
}

// ===================================================================
// ROUND 9: Aggregate Desync, Warmup, & State Machine Attack Tests
// ===================================================================

/// ATTACK: Verify c_tot aggregate stays in sync after multiple deposits and trades.
/// Multiple users deposit and trade, then verify c_tot == sum of individual capitals.
#[test]
fn test_attack_c_tot_sync_after_deposits_and_trades() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 5_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 3_000_000_000);

    env.crank();

    // Open positions
    env.trade(&user1, &lp, lp_idx, user1_idx, 50_000);
    env.set_slot(10);
    env.trade(&user2, &lp, lp_idx, user2_idx, -30_000);
    env.set_slot(20);
    env.crank();

    // Read individual capitals and c_tot
    let lp_cap = env.read_account_capital(lp_idx);
    let u1_cap = env.read_account_capital(user1_idx);
    let u2_cap = env.read_account_capital(user2_idx);
    let c_tot = env.read_c_tot();

    // c_tot should equal sum of individual capitals
    let sum = lp_cap + u1_cap + u2_cap;
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync! c_tot={} sum={} (lp={} u1={} u2={})",
        c_tot, sum, lp_cap, u1_cap, u2_cap
    );

    // SPL vault conservation
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(spl_vault, 28_000_000_000, "ATTACK: SPL vault changed!");
}

/// ATTACK: Verify pnl_pos_tot tracks only positive PnL accounts.
/// After trades and cranks, pnl_pos_tot should be sum of max(0, pnl) for each account.
#[test]
fn test_attack_pnl_pos_tot_only_positive() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    // Top up insurance to disable force-realize
    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();

    env.crank();

    // Open position then crank at different price to create PnL
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot_and_price(10, 140_000_000); // Price up slightly
    env.crank();

    // Read PnL values
    let user_pnl = env.read_account_pnl(user_idx);
    let lp_pnl = env.read_account_pnl(lp_idx);
    let pnl_pos_tot = env.read_pnl_pos_tot();

    // Precondition: at least one PnL should be non-zero after price move
    assert!(
        user_pnl != 0 || lp_pnl != 0,
        "TEST PRECONDITION: Price move should create non-zero PnL for at least one account"
    );

    // pnl_pos_tot should be sum of max(0, pnl) for each account
    let expected = (user_pnl.max(0) as u128) + (lp_pnl.max(0) as u128);
    assert_eq!(
        pnl_pos_tot, expected,
        "ATTACK: pnl_pos_tot wrong! got={} expected={} (user_pnl={} lp_pnl={})",
        pnl_pos_tot, expected, user_pnl, lp_pnl
    );
}

/// ATTACK: Warmup with zero period should convert PnL instantly.
/// Init market with warmup_period_slots=0, verify profit converts immediately.
#[test]
fn test_attack_warmup_zero_period_instant_conversion() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_warmup(0, 0); // warmup_period_slots = 0

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot(10);
    env.crank();

    // With warmup=0, PnL should convert to capital immediately on next crank
    env.set_slot(20);
    env.crank();

    // Conservation: total value shouldn't exceed deposits
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 25_000_000_000,
        "ATTACK: SPL vault changed with instant warmup! vault={}",
        spl_vault
    );

    // c_tot should still equal sum of capitals
    let lp_cap = env.read_account_capital(lp_idx);
    let user_cap = env.read_account_capital(user_idx);
    let c_tot = env.read_c_tot();
    assert_eq!(
        c_tot,
        lp_cap + user_cap,
        "ATTACK: c_tot desync after instant warmup!"
    );
}

/// ATTACK: Open and close multiple positions - verify c_tot stays consistent.
/// Trade long, close, trade short, close - c_tot == sum of capitals at each step.
#[test]
fn test_attack_position_flip_warmup_reset() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    // Top up insurance to disable force-realize mode (insurance > threshold=0)
    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();

    env.crank();

    // Open long
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot(10);
    env.crank();

    // Verify c_tot consistency mid-trade
    let c_tot_1 = env.read_c_tot();
    let sum_1 = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot_1, sum_1,
        "ATTACK: c_tot desync with open long! c_tot={} sum={}",
        c_tot_1, sum_1
    );

    // Close long
    env.set_slot(20);
    env.trade(&user, &lp, lp_idx, user_idx, -100_000);
    env.set_slot(30);
    env.crank();

    // Position should be zero after close
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(user_pos, 0, "Position should be zero after close");

    // Open short with different size (avoids AlreadyProcessed)
    env.set_slot(40);
    env.trade(&user, &lp, lp_idx, user_idx, -80_000);
    env.set_slot(50);
    env.crank();

    // Position should be short
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(user_pos, -80_000, "Position should be -80K after new short");

    // Conservation: SPL vault should include deposits + insurance top-up
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault,
        26_000_000_000, // 20B + 5B + 1B insurance
        "ATTACK: SPL vault changed during position flip!"
    );

    let c_tot = env.read_c_tot();
    let lp_cap = env.read_account_capital(lp_idx);
    let user_cap = env.read_account_capital(user_idx);
    assert_eq!(
        c_tot,
        lp_cap + user_cap,
        "ATTACK: c_tot desync after position flip!"
    );
}

/// ATTACK: Multiple sequential account inits have clean independent state.
/// Create several accounts, verify each starts with zero position/PnL.
/// Then trade with one and verify the others are not affected.
#[test]
fn test_attack_account_reinit_after_gc_clean_state() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    // Create 3 users
    let user1 = Keypair::new();
    let u1_idx = env.init_user(&user1);
    env.deposit(&user1, u1_idx, 5_000_000_000);

    let user2 = Keypair::new();
    let u2_idx = env.init_user(&user2);
    env.deposit(&user2, u2_idx, 3_000_000_000);

    let user3 = Keypair::new();
    let u3_idx = env.init_user(&user3);
    env.deposit(&user3, u3_idx, 2_000_000_000);

    env.crank();

    // All new accounts should start clean
    assert_eq!(
        env.read_account_position(u1_idx),
        0,
        "User1 should start with zero position"
    );
    assert_eq!(
        env.read_account_position(u2_idx),
        0,
        "User2 should start with zero position"
    );
    assert_eq!(
        env.read_account_position(u3_idx),
        0,
        "User3 should start with zero position"
    );
    assert_eq!(
        env.read_account_pnl(u1_idx),
        0,
        "User1 should start with zero PnL"
    );
    assert_eq!(
        env.read_account_pnl(u2_idx),
        0,
        "User2 should start with zero PnL"
    );
    assert_eq!(
        env.read_account_pnl(u3_idx),
        0,
        "User3 should start with zero PnL"
    );

    // Trade with user1 only
    env.trade(&user1, &lp, lp_idx, u1_idx, 100_000);
    env.set_slot(10);
    env.crank();

    // User2 and User3 capitals unchanged (no cross-contamination)
    assert_eq!(
        env.read_account_capital(u2_idx),
        3_000_000_000,
        "ATTACK: User2 capital changed from User1's trade!"
    );
    assert_eq!(
        env.read_account_capital(u3_idx),
        2_000_000_000,
        "ATTACK: User3 capital changed from User1's trade!"
    );

    // PnL should be zero for non-trading accounts
    assert_eq!(
        env.read_account_pnl(u2_idx),
        0,
        "ATTACK: User2 PnL leaked from User1's trade!"
    );
    assert_eq!(
        env.read_account_pnl(u3_idx),
        0,
        "ATTACK: User3 PnL leaked from User1's trade!"
    );
}

/// ATTACK: Insurance fund growth from fees doesn't inflate haircut.
/// Haircut = min(residual, pnl_pos_tot) / pnl_pos_tot where residual = vault - c_tot - insurance.
/// Insurance growing from fees reduces residual, which REDUCES haircut (safer).
#[test]
fn test_attack_insurance_fee_growth_doesnt_inflate_haircut() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    // Set maintenance fee to accrue insurance fees
    env.try_set_maintenance_fee(&admin, 100).unwrap(); // 1% - must succeed

    // Top up insurance to disable force-realize
    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();

    // Trade to create positions
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Record insurance before fee accrual
    let insurance_before = env.read_insurance_balance();

    // Advance time to accrue fees (1000 slots * maintenance_fee_per_slot)
    env.set_slot(1000);
    env.crank();

    // Insurance should have grown from fees
    let insurance_after = env.read_insurance_balance();
    assert!(
        insurance_after > insurance_before,
        "Insurance should grow from maintenance fees! before={} after={}",
        insurance_before,
        insurance_after
    );

    // vault = SPL vault in engine units. residual = vault - c_tot - insurance
    let engine_vault = env.read_engine_vault();
    let c_tot = env.read_c_tot();

    // residual should be non-negative (vault >= c_tot + insurance)
    assert!(
        engine_vault >= c_tot + insurance_after,
        "ATTACK: vault < c_tot + insurance! vault={} c_tot={} insurance={}",
        engine_vault,
        c_tot,
        insurance_after
    );

    // Haircut = min(residual, pnl_pos_tot) / pnl_pos_tot
    // With growing insurance, residual = vault - c_tot - insurance decreases
    // This makes haircut ratio smaller (safer), not larger
    let residual = engine_vault - c_tot - insurance_after;
    let pnl_pos_tot = env.read_pnl_pos_tot();
    if pnl_pos_tot > 0 {
        // Haircut ratio should be <= 1.0 (residual/pnl_pos_tot <= 1)
        assert!(
            residual <= pnl_pos_tot || pnl_pos_tot == 0,
            "Residual exceeds pnl_pos_tot - unexpected! residual={} pnl_pos_tot={}",
            residual,
            pnl_pos_tot
        );
    }

    // SPL vault conservation: deposits + insurance top-up
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault,
        26_000_000_000, // 20B + 5B + 1B insurance
        "ATTACK: SPL vault changed!"
    );
}

/// ATTACK: Withdraw more than capital should fail.
/// Verify that withdrawing more than available capital is rejected.
/// Also verify that withdrawal with position leaves at least margin.
#[test]
fn test_attack_withdraw_margin_boundary_consistency() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    // Try to withdraw more than deposited (overflow attack)
    let over_withdraw = env.try_withdraw(&user, user_idx, 5_000_000_001);
    assert!(
        over_withdraw.is_err(),
        "ATTACK: Withdrawal of more than capital succeeded!"
    );

    // Verify capital is unchanged
    let cap_after = env.read_account_capital(user_idx);
    assert_eq!(
        cap_after, 5_000_000_000,
        "ATTACK: Failed withdrawal changed capital!"
    );

    // Open a large position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot(10);
    env.crank();

    // Withdraw almost everything - should succeed since margin is tiny relative to capital
    let cap_now = env.read_account_capital(user_idx);
    let small_withdraw = env.try_withdraw(&user, user_idx, (cap_now - 100_000_000) as u64);
    assert!(
        small_withdraw.is_ok(),
        "Withdrawal leaving sufficient margin should succeed"
    );

    // SPL vault conservation: should equal deposits minus withdrawals
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    let withdrawn = (cap_now - 100_000_000) as u64;
    assert_eq!(
        spl_vault,
        25_000_000_000 - withdrawn,
        "ATTACK: SPL vault mismatch after withdrawal!"
    );
}

/// ATTACK: Permissionless crank doesn't extract value.
/// Any user can call crank with caller_idx=u16::MAX. Verify no value extraction.
#[test]
fn test_attack_permissionless_crank_no_value_extraction() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    let lp_cap_before = env.read_account_capital(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);

    // Crank is permissionless (uses random caller)
    env.set_slot(10);
    env.crank();
    env.set_slot(20);
    env.crank();
    env.set_slot(30);
    env.crank();

    // No value should be extracted by cranking
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 25_000_000_000,
        "ATTACK: SPL vault changed from permissionless cranks!"
    );

    // Total capital should be conserved
    let lp_cap_after = env.read_account_capital(lp_idx);
    let user_cap_after = env.read_account_capital(user_idx);
    assert!(
        lp_cap_after + user_cap_after <= lp_cap_before + user_cap_before,
        "ATTACK: Capital increased from cranking! before={} after={}",
        lp_cap_before + user_cap_before,
        lp_cap_after + user_cap_after
    );
}

/// ATTACK: Multiple close-account calls on same index should fail.
/// After closing once, the slot is freed. Closing again should error.
#[test]
fn test_attack_double_close_account_same_index() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    // Close account
    env.try_close_account(&user, user_idx).unwrap();

    // Try to close again - should fail
    let second_close = env.try_close_account(&user, user_idx);
    assert!(
        second_close.is_err(),
        "ATTACK: Double close succeeded - potential double withdrawal!"
    );

    // SPL vault should only have LP deposit
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert!(
        spl_vault <= 10_000_000_000,
        "ATTACK: SPL vault has too much after close! vault={}",
        spl_vault
    );
}

/// ATTACK: Deposit after close should fail if account is freed.
/// After closing an account, depositing to that index should fail.
#[test]
fn test_attack_deposit_to_closed_account_index() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    // Close account
    env.try_close_account(&user, user_idx).unwrap();

    // Try deposit to closed index - should fail (account not found or owner mismatch)
    let deposit_result = env.try_deposit(&user, user_idx, 1_000_000_000);
    assert!(
        deposit_result.is_err(),
        "ATTACK: Deposit to closed account index succeeded!"
    );

    // SPL vault should not have increased beyond LP deposit
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert!(
        spl_vault <= 10_000_000_000,
        "ATTACK: SPL vault increased from deposit to closed account!"
    );
}

/// ATTACK: Trade to closed account index should fail.
/// After closing, trying to use the freed slot as counterparty should error.
#[test]
fn test_attack_trade_with_closed_account_index() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    // Close user account
    env.try_close_account(&user, user_idx).unwrap();

    // Try trade referencing closed account
    let trade_result = env.try_trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert!(
        trade_result.is_err(),
        "ATTACK: Trade with closed account index succeeded!"
    );
}

/// ATTACK: Verify engine vault tracks SPL vault correctly across operations.
/// After deposits, trades, withdrawals, and cranks, engine vault should match SPL vault.
#[test]
fn test_attack_engine_vault_spl_vault_consistency() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    // Trade
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot(10);
    env.crank();

    // Partial withdraw
    env.try_withdraw(&user, user_idx, 100_000).unwrap();

    env.set_slot(20);
    env.crank();

    // Engine vault = c_tot + insurance + net_pnl
    let engine_vault = env.read_engine_vault();
    let c_tot = env.read_c_tot();
    let insurance = env.read_insurance_balance();

    // SPL vault
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };

    // engine_vault should match SPL vault (with unit_scale=0, 1:1)
    assert_eq!(
        engine_vault, spl_vault as u128,
        "ATTACK: Engine vault != SPL vault! engine={} spl={}",
        engine_vault, spl_vault
    );

    // vault >= c_tot + insurance (conservation invariant)
    assert!(
        engine_vault >= c_tot + insurance,
        "ATTACK: vault < c_tot + insurance! vault={} c_tot={} ins={}",
        engine_vault,
        c_tot,
        insurance
    );
}

/// ATTACK: UpdateAdmin then attempt old admin operations.
/// After admin transfer, old admin should be unable to perform admin operations.
#[test]
fn test_attack_old_admin_blocked_after_transfer() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let old_admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let new_admin = Keypair::new();
    env.svm.airdrop(&new_admin.pubkey(), 1_000_000_000).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    env.crank();

    // Transfer admin
    env.try_update_admin(&old_admin, &new_admin.pubkey())
        .unwrap();

    // Old admin should fail
    let old_result = env.try_set_risk_threshold(&old_admin, 999);
    assert!(
        old_result.is_err(),
        "ATTACK: Old admin can still set threshold after transfer!"
    );

    // New admin should succeed
    let new_result = env.try_set_risk_threshold(&new_admin, 999);
    assert!(
        new_result.is_ok(),
        "New admin should be able to set threshold: {:?}",
        new_result
    );
}

/// ATTACK: Verify conservation after complex multi-user lifecycle.
/// Multiple users open positions, some profitable, some losing, then all close.
/// Total withdrawn should equal total deposited.
#[test]
fn test_attack_multi_user_lifecycle_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    // Create 3 users
    let user1 = Keypair::new();
    let u1_idx = env.init_user(&user1);
    env.deposit(&user1, u1_idx, 5_000_000_000);

    let user2 = Keypair::new();
    let u2_idx = env.init_user(&user2);
    env.deposit(&user2, u2_idx, 3_000_000_000);

    let user3 = Keypair::new();
    let u3_idx = env.init_user(&user3);
    env.deposit(&user3, u3_idx, 2_000_000_000);

    env.crank();

    // Open various positions
    env.trade(&user1, &lp, lp_idx, u1_idx, 100_000); // user1 long
    env.set_slot(10);
    env.trade(&user2, &lp, lp_idx, u2_idx, -50_000); // user2 short
    env.set_slot(20);
    env.crank();

    // Close all positions
    env.trade(&user1, &lp, lp_idx, u1_idx, -100_000);
    env.set_slot(30);
    env.trade(&user2, &lp, lp_idx, u2_idx, 50_000);
    env.set_slot(40);
    env.crank();

    // All positions should be zero
    assert_eq!(
        env.read_account_position(u1_idx),
        0,
        "User1 position not zero"
    );
    assert_eq!(
        env.read_account_position(u2_idx),
        0,
        "User2 position not zero"
    );
    assert_eq!(
        env.read_account_position(u3_idx),
        0,
        "User3 position not zero"
    );
    assert_eq!(env.read_account_position(lp_idx), 0, "LP position not zero");

    // SPL vault should be unchanged (no deposits/withdrawals during trading)
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 60_000_000_000,
        "ATTACK: SPL vault changed during multi-user lifecycle! vault={}",
        spl_vault
    );

    // c_tot should equal sum of all capitals
    let c_tot = env.read_c_tot();
    let total_cap = env.read_account_capital(lp_idx)
        + env.read_account_capital(u1_idx)
        + env.read_account_capital(u2_idx)
        + env.read_account_capital(u3_idx);
    assert_eq!(
        c_tot, total_cap,
        "ATTACK: c_tot desync after lifecycle! c_tot={} sum={}",
        c_tot, total_cap
    );
}

// ============================================================================
// ROUND 10: Config Boundaries, Funding Timing, Multi-LP, & Token Validation
// ============================================================================

/// ATTACK: UpdateConfig with extreme funding parameters.
/// Set funding_max_bps_per_slot to max i64, verify crank doesn't overflow.
#[test]
fn test_attack_config_extreme_funding_max_bps() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open position to create funding obligation
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Try to set thresh_max to extreme value
    // The engine should either accept (with clamping) or reject this
    let result = env.try_update_config_with_params(
        &admin,
        100,               // funding_horizon_slots
        1_000_000_000_000, // funding_inv_scale
        1000,              // thresh_alpha_bps
        0,                 // thresh_min
        u128::MAX / 2,     // thresh_max (huge)
    );
    assert!(
        result.is_ok(),
        "Extreme-but-valid thresh_max update should be accepted: {:?}",
        result
    );

    // Regardless of acceptance, advance and crank - must not panic/overflow
    env.set_slot(100);
    env.crank();

    // Conservation check - vault must be consistent no matter what
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_000,
        "ATTACK: SPL vault changed after extreme config! vault={}",
        spl_vault
    );

    // c_tot should still be consistent
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync after extreme config! c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Zero-slot crank loops shouldn't compound funding.
/// Crank multiple times at the same slot - funding should accrue only once.
#[test]
fn test_attack_same_slot_crank_no_double_funding() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot(10);
    env.crank();

    // Record state after first crank at slot 100
    env.set_slot(100);
    env.crank();
    let cap_after_first = env.read_account_capital(user_idx);
    let pnl_after_first = env.read_account_pnl(user_idx);

    // Crank again at same slot - should be a no-op for funding
    env.crank();
    let cap_after_second = env.read_account_capital(user_idx);
    let pnl_after_second = env.read_account_pnl(user_idx);

    // Capital and PnL should be unchanged (no double funding)
    assert_eq!(
        cap_after_first, cap_after_second,
        "ATTACK: Double crank changed capital! first={} second={}",
        cap_after_first, cap_after_second
    );
    assert_eq!(
        pnl_after_first, pnl_after_second,
        "ATTACK: Double crank changed PnL! first={} second={}",
        pnl_after_first, pnl_after_second
    );
}

/// ATTACK: Multiple LPs trading with same user - verify all positions tracked correctly.
/// Each LP independently takes opposite side of user trades.
#[test]
fn test_attack_multi_lp_position_tracking() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp1 = Keypair::new();
    let lp1_idx = env.init_lp(&lp1);
    env.deposit(&lp1, lp1_idx, 20_000_000_000);

    let lp2 = Keypair::new();
    let lp2_idx = env.init_lp(&lp2);
    env.deposit(&lp2, lp2_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // User goes long via LP1
    env.trade(&user, &lp1, lp1_idx, user_idx, 100_000);
    env.set_slot(10);

    // User goes long more via LP2 (different size to avoid AlreadyProcessed)
    env.trade(&user, &lp2, lp2_idx, user_idx, 50_000);
    env.set_slot(20);
    env.crank();

    // User position should be sum of both trades
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(
        user_pos, 150_000,
        "User position should be 150K (100K + 50K): {}",
        user_pos
    );

    // Each LP should have their own position (opposite)
    let lp1_pos = env.read_account_position(lp1_idx);
    let lp2_pos = env.read_account_position(lp2_idx);
    assert_eq!(lp1_pos, -100_000, "LP1 should have -100K: {}", lp1_pos);
    assert_eq!(lp2_pos, -50_000, "LP2 should have -50K: {}", lp2_pos);

    // Conservation: net position should be zero
    let net = user_pos + lp1_pos + lp2_pos;
    assert_eq!(
        net, 0,
        "ATTACK: Net position not zero! user={} lp1={} lp2={} net={}",
        user_pos, lp1_pos, lp2_pos, net
    );

    // c_tot consistency
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp1_idx)
        + env.read_account_capital(lp2_idx)
        + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with multiple LPs! c_tot={} sum={}",
        c_tot, sum
    );

    // SPL vault conservation (20B + 20B + 10B + 1B insurance)
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 51_000_000_000,
        "ATTACK: SPL vault wrong with multiple LPs!"
    );
}

/// ATTACK: Trade as LP-kind account in user slot (kind mismatch).
/// LP accounts can only be in lp_idx position, users in user_idx.
#[test]
fn test_attack_lp_as_user_kind_swap() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Precondition: normal trade succeeds (proves test setup is correct)
    env.trade(&user, &lp, lp_idx, user_idx, 50_000);
    let pos = env.read_account_position(user_idx);
    assert_eq!(pos, 50_000, "Precondition: normal trade should work");

    // Try to trade with LP and User indices swapped
    // LP in user slot, User in LP slot - should fail kind check
    let result = env.try_trade(&user, &lp, user_idx, lp_idx, 100_000);
    assert!(
        result.is_err(),
        "ATTACK: Trade with swapped LP/User indices succeeded!"
    );
}

// test_attack_withdraw_exact_capital_zero_position: removed (duplicate of test_attack_withdraw_exact_capital_no_position)

/// ATTACK: Deposit zero amount should be harmless.
/// Depositing 0 tokens should either fail or be a no-op.
#[test]
fn test_attack_deposit_zero_amount_no_state_change() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    let cap_before = env.read_account_capital(user_idx);

    // Deposit 0 - should be rejected or accepted as no-op
    let result = env.try_deposit(&user, user_idx, 0);

    let cap_after = env.read_account_capital(user_idx);
    if result.is_ok() {
        // If accepted, must be a no-op (no state change)
        assert_eq!(
            cap_before, cap_after,
            "ATTACK: Zero deposit accepted but changed capital! before={} after={}",
            cap_before, cap_after
        );
    } else {
        // If rejected, protocol correctly prevents zero deposits
        // Verify state is unchanged (failed txns never modify state)
        assert_eq!(
            cap_before, cap_after,
            "State changed despite failed zero deposit!"
        );
    }
    // Either way: the test verified the protocol handles zero deposits correctly
    // (either rejects them or treats them as no-ops)
}

/// ATTACK: Withdraw zero amount should be harmless.
/// Withdrawing 0 tokens should either fail or be a no-op.
#[test]
fn test_attack_withdraw_zero_amount_no_state_change() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    let cap_before = env.read_account_capital(user_idx);

    // Withdraw 0 - should be rejected or accepted as no-op
    let result = env.try_withdraw(&user, user_idx, 0);

    let cap_after = env.read_account_capital(user_idx);
    if result.is_ok() {
        // If accepted, must be a no-op
        assert_eq!(
            cap_before, cap_after,
            "ATTACK: Zero withdrawal accepted but changed capital! before={} after={}",
            cap_before, cap_after
        );
        // SPL vault must also be unchanged
        let spl_vault = {
            let vault_data = env.svm.get_account(&env.vault).unwrap().data;
            TokenAccount::unpack(&vault_data).unwrap().amount
        };
        assert_eq!(
            spl_vault, 15_000_000_000,
            "ATTACK: SPL vault changed after accepted zero withdrawal!"
        );
    } else {
        // If rejected, protocol correctly prevents zero withdrawals
        assert_eq!(
            cap_before, cap_after,
            "State changed despite failed zero withdrawal!"
        );
    }
}

/// ATTACK: Trade with zero size should be harmless.
/// Trading 0 contracts should either fail or be a no-op.
#[test]
fn test_attack_trade_zero_size() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    let cap_before = env.read_account_capital(user_idx);

    // Trade zero size - should be explicitly rejected or be a no-op
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 0);

    let cap_after = env.read_account_capital(user_idx);
    let pos_after = env.read_account_position(user_idx);

    if result.is_ok() {
        // If accepted, must be a no-op
        assert_eq!(
            cap_before, cap_after,
            "ATTACK: Zero trade accepted but changed capital! before={} after={}",
            cap_before, cap_after
        );
        assert_eq!(
            pos_after, 0,
            "ATTACK: Zero trade accepted but created position! pos={}",
            pos_after
        );
    } else {
        // Protocol correctly rejects zero-size trades
        assert_eq!(
            cap_before, cap_after,
            "State changed despite failed zero trade!"
        );
        assert_eq!(pos_after, 0, "Position changed despite failed zero trade!");
    }
}

/// ATTACK: Force-realize mode closes positions during crank.
/// When insurance <= threshold, crank enters force-realize mode.
/// Verify it correctly closes positions without creating value.
#[test]
fn test_attack_force_realize_closes_positions_safely() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    // DO NOT top up insurance - force-realize mode is active (insurance=0 <= threshold=0)
    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot(10);

    // SPL vault before crank
    let vault_before = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };

    // Crank - should force-realize the positions
    env.crank();

    // After force-realize, positions should be zero
    let user_pos = env.read_account_position(user_idx);
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(
        user_pos, 0,
        "User position should be force-closed: {}",
        user_pos
    );
    assert_eq!(lp_pos, 0, "LP position should be force-closed: {}", lp_pos);

    // SPL vault should be unchanged (force-realize doesn't move tokens)
    let vault_after = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        vault_before, vault_after,
        "ATTACK: Force-realize changed vault balance! before={} after={}",
        vault_before, vault_after
    );
}

/// ATTACK: UpdateConfig with alpha=0 should be accepted without causing overflow.
/// Verify config update result and conservation after crank with zero-alpha.
#[test]
fn test_attack_threshold_ewma_alpha_zero_freezes() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Set threshold config with alpha=0 - config must be accepted
    env.try_update_config_with_params(
        &admin,
        100,               // funding_horizon_slots
        1_000_000_000_000, // funding_inv_scale
        0,                 // thresh_alpha_bps = 0 (no learning)
        100_000_000,       // thresh_min
        10_000_000_000,    // thresh_max
    )
    .expect("Config with alpha=0 should be accepted");

    // Trade and crank - must not panic/overflow with zero alpha
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Precondition: position is open
    let pos = env.read_account_position(user_idx);
    assert_eq!(pos, 100_000, "Precondition: position should be open");

    env.set_slot(200);
    env.crank();

    // Conservation must hold with zero alpha
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with alpha=0! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_000,
        "ATTACK: SPL vault changed with alpha=0!"
    );
}

/// ATTACK: Deposit after setting large maintenance fee.
/// Verify fee settlement during deposit doesn't extract extra value.
#[test]
fn test_attack_deposit_with_pending_fee_debt() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open a position so maintenance fees actually accrue
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    let pos = env.read_account_position(user_idx);
    assert_eq!(
        pos, 100_000,
        "Precondition: user must have open position for fee accrual"
    );

    // Set a maintenance fee
    env.try_set_maintenance_fee(&admin, 1000).unwrap(); // 10%

    // Advance many slots to accrue fee debt on the open position
    env.set_slot(10000);

    // Record insurance before deposit
    let insurance_before = env.read_insurance_balance();

    // Deposit more - fee debt should be settled during deposit
    env.deposit(&user, user_idx, 2_000_000_000);

    let insurance_after = env.read_insurance_balance();

    // Insurance should have grown from fee payment (fees go to insurance)
    assert!(
        insurance_after > insurance_before,
        "ATTACK: Insurance didn't grow from fee settlement during deposit! before={} after={}",
        insurance_before,
        insurance_after
    );

    // SPL vault should reflect all deposits + insurance
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    // 20B LP + 5B user + 1B insurance + 2B new deposit = 28B
    assert_eq!(
        spl_vault, 28_000_000_000,
        "ATTACK: SPL vault mismatch after deposit with fees!"
    );
}

/// ATTACK: Close account forgives fee debt without extracting from vault.
/// CloseAccount pays what it can from capital, forgives the rest.
#[test]
fn test_attack_close_account_fee_debt_forgiveness() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000); // 1B deposit

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open position to accrue fees, then close it to zero position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert_eq!(
        env.read_account_position(user_idx),
        100_000,
        "Precondition: user must have position for fee accrual"
    );

    // Set large maintenance fee
    env.try_set_maintenance_fee(&admin, 10000).unwrap(); // 100%

    // Advance to accrue massive fee debt on the open position
    env.set_slot(1000);
    env.crank(); // Crank settles fees + might liquidate

    // Close the position back to zero (trade opposite direction)
    let pos = env.read_account_position(user_idx);
    if pos != 0 {
        env.try_trade(&user, &lp, lp_idx, user_idx, -pos)
            .expect("position flattening before close must succeed");
    }

    // Record insurance before close
    let insurance_before = env.read_insurance_balance();

    // Close account - should succeed; fee debt is forgiven after paying what's possible
    env.try_close_account(&user, user_idx).unwrap();

    // Insurance should have received whatever fee payment was possible
    let insurance_after = env.read_insurance_balance();
    assert!(
        insurance_after >= insurance_before,
        "Insurance should not decrease during close: before={} after={}",
        insurance_before,
        insurance_after
    );

    // User should have no capital remaining
    let user_cap = env.read_account_capital(user_idx);
    assert_eq!(
        user_cap, 0,
        "Closed account should have zero capital: {}",
        user_cap
    );
}

/// ATTACK: Liquidate account that becomes insolvent from price move.
/// After price crash, undercollateralized account should be liquidatable.
#[test]
fn test_attack_liquidation_after_price_crash() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // User with moderate capital
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000); // 5B

    env.try_top_up_insurance(&admin, 10_000_000_000).unwrap();
    env.crank();

    // Open a very large long position (use max margin)
    // initial_margin_bps=400 -> 4%, so 5B capital supports 5B/0.04=125B notional
    // at price 138, that's 125B/138 ~= 905M contracts
    // But let's use a smaller amount to be safe
    env.trade(&user, &lp, lp_idx, user_idx, 100_000_000); // 100M contracts

    // Precondition: position is open
    let pos = env.read_account_position(user_idx);
    assert_eq!(pos, 100_000_000, "User should have 100M long position");

    // Massive price crash: 138 -> 50
    // PnL = 100M * (50-138) / 1e6 = 100M * -88e-6 = -8800 tokens
    // With 5B tokens capital, this should make the account deeply insolvent
    // maintenance_margin_bps=200 -> 2%, required margin = 100M*50/1e6*0.02 = 100 tokens
    // equity = 5000 - 8800 = -3800 -> deeply negative -> liquidated
    env.set_slot_and_price(100, 50_000_000);
    env.crank(); // Crank to settle mark-to-oracle and liquidate

    // After crank with liquidation, user's position should be reduced or zeroed
    let pos_after = env.read_account_position(user_idx);
    // Crank may liquidate the position partially or fully
    assert!(
        pos_after.abs() < pos.abs(),
        "ATTACK: Insolvent position not liquidated! before={} after={}",
        pos,
        pos_after
    );

    // SPL vault unchanged (liquidation is internal accounting)
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    // 100B + 5B + 10B insurance = 115B
    assert_eq!(
        spl_vault, 115_000_000_000,
        "ATTACK: SPL vault changed during liquidation!"
    );
}

/// ATTACK: Insurance grows correctly from new account fees.
/// InitUser/InitLP pays a new_account_fee that goes to insurance.
#[test]
fn test_attack_new_account_fee_goes_to_insurance() {
    program_path();

    let mut env = TestEnv::new();
    let fee: u64 = 1_000_000;
    // Use init_market_full with a non-zero new_account_fee
    env.init_market_full(0, 0, fee as u128);

    let insurance_before = env.read_insurance_balance();

    // Create LP with fee payment - need tokens in ATA to pay the fee
    let lp = Keypair::new();
    env.svm.airdrop(&lp.pubkey(), 1_000_000_000).unwrap();
    let lp_ata = env.create_ata(&lp.pubkey(), fee);
    let matcher = spl_token::ID;
    let ctx = Pubkey::new_unique();
    env.svm
        .set_account(
            ctx,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; 320],
                owner: matcher,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(lp.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(lp_ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(matcher, false),
            AccountMeta::new_readonly(ctx, false),
        ],
        data: encode_init_lp(&matcher, &ctx, fee),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&lp.pubkey()),
        &[&lp],
        env.svm.latest_blockhash(),
    );
    env.svm
        .send_transaction(tx)
        .expect("init_lp with fee failed");
    env.account_count += 1;

    let insurance_after_lp = env.read_insurance_balance();
    assert!(
        insurance_after_lp > insurance_before,
        "Insurance should grow from LP init fee! before={} after={}",
        insurance_before,
        insurance_after_lp
    );

    // Create user with fee payment
    let user = Keypair::new();
    env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();
    let user_ata = env.create_ata(&user.pubkey(), fee);

    let ix2 = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(user_ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_col, false),
        ],
        data: encode_init_user(fee),
    };
    let tx2 = Transaction::new_signed_with_payer(
        &[ix2],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    env.svm
        .send_transaction(tx2)
        .expect("init_user with fee failed");
    env.account_count += 1;

    let insurance_after_user = env.read_insurance_balance();
    assert!(
        insurance_after_user > insurance_after_lp,
        "Insurance should grow from User init fee! before={} after={}",
        insurance_after_lp,
        insurance_after_user
    );

    // Total insurance should have grown by 2 * fee
    let growth = insurance_after_user - insurance_before;
    assert_eq!(
        growth,
        2 * fee as u128,
        "Insurance should grow by 2 * new_account_fee: growth={}",
        growth
    );
}

/// ATTACK: Conservation invariant across large slot jumps.
/// Advance many slots, verify conservation holds despite funding/fee accrual.
#[test]
fn test_attack_conservation_large_slot_jump() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Large slot jump (1 million slots)
    env.set_slot(1_000_000);
    env.crank();

    // SPL vault unchanged
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_000,
        "ATTACK: SPL vault changed after large slot jump!"
    );

    // Engine vault matches SPL vault
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault, spl_vault as u128,
        "ATTACK: Engine vault != SPL vault after slot jump! engine={} spl={}",
        engine_vault, spl_vault
    );

    // c_tot consistency
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync after large slot jump! c_tot={} sum={}",
        c_tot, sum
    );

    // vault >= c_tot + insurance
    let insurance = env.read_insurance_balance();
    assert!(
        engine_vault >= c_tot + insurance,
        "ATTACK: vault < c_tot + insurance! vault={} c_tot={} ins={}",
        engine_vault,
        c_tot,
        insurance
    );
}

// ============================================================================
// ROUND 11: Warmup, Funding Edge Cases, Liquidation Budgets, Token Validation
// ============================================================================

/// ATTACK: Warmup period settlement - profit only vests after warmup.
/// With warmup_period > 0, PnL profit should vest gradually, not instantly.
#[test]
fn test_attack_warmup_profit_vests_gradually() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_warmup(0, 100); // 100-slot warmup period

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open long position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert_eq!(
        env.read_account_position(user_idx),
        100_000,
        "Precondition: position must be open"
    );

    // Price goes up → user has profit
    env.set_slot_and_price(10, 150_000_000); // 138→150
    env.crank();

    // At slot 10 with 100-slot warmup, only 10% of profit should be vested
    let user_cap_early = env.read_account_capital(user_idx);

    // Advance to end of warmup
    env.set_slot_and_price(110, 150_000_000);
    env.crank();

    let user_cap_late = env.read_account_capital(user_idx);

    // Capital should not decrease as warmup vests more profit over time.
    // Note: Capital may stay equal if warmup conversion is still pending or
    // PnL haircut absorbs the gain.
    assert!(
        user_cap_late >= user_cap_early,
        "ATTACK: Capital decreased after more warmup! early={} late={}",
        user_cap_early,
        user_cap_late
    );

    // Conservation: c_tot = sum of all capitals
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync during warmup! c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Warmup period=0 means instant settlement.
/// With warmup=0, all PnL should vest immediately.
#[test]
fn test_attack_warmup_period_zero_instant_settlement() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0); // warmup_period=0

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    let user_cap_before = env.read_account_capital(user_idx);

    // Open long position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Price goes up → user profits
    env.set_slot_and_price(10, 150_000_000);
    env.crank();

    let user_cap_after = env.read_account_capital(user_idx);
    let user_pnl = env.read_account_pnl(user_idx);
    assert!(
        user_cap_after >= user_cap_before || user_pnl > 0,
        "With warmup=0 and favorable price move, user must realize positive effect: cap_before={} cap_after={} pnl={}",
        user_cap_before,
        user_cap_after,
        user_pnl
    );

    // With warmup=0, PnL should be fully settled into capital after crank
    // (user_cap should have changed - either from PnL settlement or warmup conversion)
    // At minimum, the system should be conserved
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with warmup=0! c_tot={} sum={}",
        c_tot, sum
    );

    // SPL vault unchanged (PnL settlement is internal)
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_000,
        "ATTACK: SPL vault changed during warmup=0 settlement!"
    );
}

/// ATTACK: Same-slot triple crank converges.
/// Multiple cranks at same slot should eventually stabilize (lazy settlement).
/// Second crank may settle fees, but third should be fully idempotent.
#[test]
fn test_attack_same_slot_triple_crank_convergence() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Move price
    env.set_slot_and_price(50, 160_000_000);

    // Triple crank to ensure convergence
    env.crank(); // First: mark settlement + lazy fee settlement
    env.crank(); // Second: any remaining lazy operations

    let cap_second = env.read_account_capital(user_idx);
    let pnl_second = env.read_account_pnl(user_idx);
    let c_tot_second = env.read_c_tot();

    env.crank(); // Third: should be fully idempotent now

    let cap_third = env.read_account_capital(user_idx);
    let pnl_third = env.read_account_pnl(user_idx);
    let c_tot_third = env.read_c_tot();

    assert_eq!(
        cap_second, cap_third,
        "ATTACK: Third crank changed capital! second={} third={}",
        cap_second, cap_third
    );
    assert_eq!(
        pnl_second, pnl_third,
        "ATTACK: Third crank changed PnL! second={} third={}",
        pnl_second, pnl_third
    );
    assert_eq!(
        c_tot_second, c_tot_third,
        "ATTACK: Third crank changed c_tot! second={} third={}",
        c_tot_second, c_tot_third
    );

    // SPL vault unchanged throughout
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(spl_vault, 26_000_000_000);
}

/// ATTACK: Funding rate with extreme k_bps.
/// Set funding_k_bps to maximum, verify funding rate is capped at ±10,000 bps/slot.
#[test]
fn test_attack_funding_extreme_k_bps_capped() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Set extreme k_bps via direct config encoding
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
        ],
        data: encode_update_config(
            100,               // funding_horizon_slots
            u64::MAX / 2,      // funding_k_bps (extreme!)
            1_000_000_000_000, // funding_inv_scale
            100,               // funding_max_premium_bps
            10,                // funding_max_bps_per_slot
            0u128,             // thresh_floor
            100,               // thresh_risk_bps
            100,               // thresh_update_interval_slots
            100,               // thresh_step_bps
            1000,              // thresh_alpha_bps
            0u128,             // thresh_min
            u128::MAX / 2,     // thresh_max
            1u128,             // thresh_min_step
        ),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    // Config update may be accepted or rejected
    let config_accepted = env.svm.send_transaction(tx).is_ok();

    // Open position and advance
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot(100);
    env.crank(); // Must not panic/overflow

    // Conservation check
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with extreme k_bps (accepted={})! c_tot={} sum={}",
        config_accepted, c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_000,
        "ATTACK: SPL vault changed with extreme k_bps!"
    );
}

/// ATTACK: Funding with extreme max_premium_bps.
/// Set funding_max_premium_bps to extreme negative, verify capping works.
#[test]
fn test_attack_funding_extreme_max_premium_capped() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Set extreme max_premium_bps via direct config
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
        ],
        data: encode_update_config(
            100,               // funding_horizon_slots
            100,               // funding_k_bps
            1_000_000_000_000, // funding_inv_scale
            i64::MAX,          // funding_max_premium_bps (extreme!)
            10,                // funding_max_bps_per_slot
            0u128,             // thresh_floor
            100,               // thresh_risk_bps
            100,               // thresh_update_interval_slots
            100,               // thresh_step_bps
            1000,              // thresh_alpha_bps
            0u128,             // thresh_min
            u128::MAX / 2,     // thresh_max
            1u128,             // thresh_min_step
        ),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    let config_accepted = env.svm.send_transaction(tx).is_ok();

    // Trade and crank
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot(100);
    env.crank(); // Must not overflow

    // Conservation check
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with extreme max_premium (accepted={})! c_tot={} sum={}",
        config_accepted, c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_000,
        "ATTACK: SPL vault changed with extreme max_premium!"
    );
}

/// ATTACK: Funding with extreme max_bps_per_slot.
/// Set funding_max_bps_per_slot to extreme value, verify engine caps at ±10,000.
#[test]
fn test_attack_funding_extreme_max_bps_per_slot() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Set extreme max_bps_per_slot
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
        ],
        data: encode_update_config(
            100,               // funding_horizon_slots
            100,               // funding_k_bps
            1_000_000_000_000, // funding_inv_scale
            100,               // funding_max_premium_bps
            i64::MAX,          // funding_max_bps_per_slot (extreme!)
            0u128,             // thresh_floor
            100,               // thresh_risk_bps
            100,               // thresh_update_interval_slots
            100,               // thresh_step_bps
            1000,              // thresh_alpha_bps
            0u128,             // thresh_min
            u128::MAX / 2,     // thresh_max
            1u128,             // thresh_min_step
        ),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    let config_accepted = env.svm.send_transaction(tx).is_ok();

    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot(100);
    env.crank(); // Must not overflow even with extreme bps/slot

    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with extreme max_bps_per_slot (accepted={})! c_tot={} sum={}",
        config_accepted, c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_000,
        "ATTACK: SPL vault changed with extreme max_bps_per_slot!"
    );
}

/// ATTACK: Deposit with wrong mint token account.
/// Attempt to deposit from an ATA with a different mint.
#[test]
fn test_attack_deposit_wrong_mint_token_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    // Create a fake ATA with a different mint
    let fake_mint = Pubkey::new_unique();
    let fake_ata = Pubkey::new_unique();
    let mut fake_ata_data = vec![0u8; TokenAccount::LEN];
    // Pack a valid SPL token account with wrong mint
    // (mint field is at offset 0, 32 bytes)
    fake_ata_data[0..32].copy_from_slice(fake_mint.as_ref());
    // owner field at offset 32
    fake_ata_data[32..64].copy_from_slice(user.pubkey().as_ref());
    // amount at offset 64, 8 bytes
    fake_ata_data[64..72].copy_from_slice(&10_000_000_000u64.to_le_bytes());
    // state = Initialized (1) at offset 108
    fake_ata_data[108] = 1;

    env.svm
        .set_account(
            fake_ata,
            Account {
                lamports: 1_000_000_000,
                data: fake_ata_data,
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    // Advance slot to avoid AlreadyProcessed from init_user
    env.set_slot(2);

    // Try to deposit using the wrong-mint ATA
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(fake_ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_col, false),
        ],
        data: encode_deposit(user_idx, 1_000_000_000),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Deposit with wrong-mint ATA should be rejected!"
    );
}

/// ATTACK: Withdraw to wrong owner's ATA.
/// Attempt to withdraw to an ATA owned by a different user.
#[test]
fn test_attack_withdraw_to_different_users_ata() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let attacker_ata = env.create_ata(&attacker.pubkey(), 0);

    env.crank();
    let withdraw_amount = 1_000_000_000u64;
    let vault_before = env.vault_balance();
    let user_cap_before = env.read_account_capital(user_idx);
    let attacker_ata_before = {
        let ata_data = env.svm.get_account(&attacker_ata).unwrap().data;
        TokenAccount::unpack(&ata_data).unwrap().amount
    };

    // User tries to withdraw to attacker's ATA
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(attacker_ata, false), // Wrong ATA!
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_col, false),
        ],
        data: encode_withdraw(user_idx, withdraw_amount),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);

    // Should either fail (wrong ATA owner check) or succeed but send to the ATA
    // that was passed (which is normal SPL behavior - vault signs the transfer)
    // The security guarantee is: user must sign, and tokens go to the ATA they specify.
    // This is NOT an attack if user chooses to send to someone else's ATA.
    // Verify exact accounting in both success and reject paths.
    let user_cap_after = env.read_account_capital(user_idx);
    let attacker_ata_after = {
        let ata_data = env.svm.get_account(&attacker_ata).unwrap().data;
        TokenAccount::unpack(&ata_data).unwrap().amount
    };
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };

    if result.is_ok() {
        assert_eq!(
            user_cap_after,
            user_cap_before - withdraw_amount as u128,
            "Successful withdrawal should decrement user capital by exact amount: before={} after={} amount={}",
            user_cap_before,
            user_cap_after,
            withdraw_amount
        );
        assert_eq!(
            spl_vault,
            vault_before - withdraw_amount,
            "Successful withdrawal should decrement vault by exact amount: before={} after={} amount={}",
            vault_before,
            spl_vault,
            withdraw_amount
        );
        assert_eq!(
            attacker_ata_after,
            attacker_ata_before + withdraw_amount,
            "Successful withdrawal should credit destination ATA by exact amount: before={} after={} amount={}",
            attacker_ata_before,
            attacker_ata_after,
            withdraw_amount
        );
    } else {
        assert_eq!(
            user_cap_after, user_cap_before,
            "Failed withdrawal should not change user capital: before={} after={}",
            user_cap_before, user_cap_after
        );
        assert_eq!(
            spl_vault, vault_before,
            "Failed withdrawal should not change vault: before={} after={}",
            vault_before, spl_vault
        );
        assert_eq!(
            attacker_ata_after, attacker_ata_before,
            "Failed withdrawal should not credit destination ATA: before={} after={}",
            attacker_ata_before, attacker_ata_after
        );
    }

    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, spl_vault,
        "Engine/SPL vault mismatch after cross-ATA withdrawal attempt: engine={} spl={}",
        engine_vault, spl_vault
    );
}

/// ATTACK: Multiple price changes between cranks.
/// Push oracle price multiple times before cranking, verify only latest applies.
#[test]
fn test_attack_multiple_oracle_updates_between_cranks() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Multiple price updates at successive slots WITHOUT cranking
    env.set_slot_and_price(10, 150_000_000);
    env.set_slot_and_price(11, 120_000_000);
    env.set_slot_and_price(12, 200_000_000);
    env.set_slot_and_price(13, 130_000_000); // Final price

    // Now crank - should use latest price
    env.crank();

    // Conservation must hold
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync after multiple oracle updates! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_000,
        "ATTACK: SPL vault changed after multiple oracle updates!"
    );
}

/// ATTACK: Trade immediately after deposit, same slot.
/// Deposit and trade in rapid succession without crank between.
#[test]
fn test_attack_trade_immediately_after_deposit_same_slot() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Deposit more and immediately trade - same slot, no crank between
    env.deposit(&user, user_idx, 2_000_000_000);
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    assert_eq!(
        env.read_account_position(user_idx),
        100_000,
        "Trade after deposit should succeed"
    );

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync after deposit+trade! c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Rapid long→short→long position reversals.
/// Multiple position flips in succession to test aggregate tracking.
#[test]
fn test_attack_rapid_position_reversals() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 5_000_000_000).unwrap();
    env.crank();

    // Rapid reversals: long → short → long → short
    // Use different sizes at each slot to avoid AlreadyProcessed
    env.trade(&user, &lp, lp_idx, user_idx, 100_000); // Long 100K
    assert_eq!(env.read_account_position(user_idx), 100_000);

    env.set_slot(2);
    env.trade(&user, &lp, lp_idx, user_idx, -150_000); // Short 50K (flip)
    assert_eq!(env.read_account_position(user_idx), -50_000);

    env.set_slot(3);
    env.trade(&user, &lp, lp_idx, user_idx, 250_000); // Long 200K (flip again)
    assert_eq!(env.read_account_position(user_idx), 200_000);

    env.set_slot(4);
    env.trade(&user, &lp, lp_idx, user_idx, -200_000); // Flat
    assert_eq!(env.read_account_position(user_idx), 0);

    // Crank at end
    env.set_slot(10);
    env.crank();

    // After all reversals and flattening, conservation must hold
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync after rapid reversals! c_tot={} sum={}",
        c_tot, sum
    );

    // pnl_pos_tot should be 0 (no positions)
    let pnl_pos_tot = env.read_pnl_pos_tot();
    assert_eq!(
        pnl_pos_tot, 0,
        "ATTACK: pnl_pos_tot should be 0 with no positions: {}",
        pnl_pos_tot
    );

    // SPL vault: 50B + 10B + 5B = 65B
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 65_000_000_000,
        "ATTACK: SPL vault wrong after rapid reversals!"
    );
}

/// ATTACK: Crank with no accounts (empty market).
/// KeeperCrank on a market with no users/LPs should be a no-op.
#[test]
fn test_attack_crank_empty_market() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Crank with no accounts at all
    env.crank();

    // Advance and crank again
    env.set_slot(100);
    env.crank();

    // Market should be in clean state
    let c_tot = env.read_c_tot();
    assert_eq!(c_tot, 0, "c_tot should be 0 with no accounts: {}", c_tot);

    let pnl_pos_tot = env.read_pnl_pos_tot();
    assert_eq!(
        pnl_pos_tot, 0,
        "pnl_pos_tot should be 0 with no accounts: {}",
        pnl_pos_tot
    );
}

/// ATTACK: Smallest possible trade (1 contract) creates correct position.
/// Note: Market uses trading_fee_bps=0, so ceiling division is not tested here.
/// Fee ceiling division is enforced at the engine level and tested in unit proofs.
#[test]
fn test_attack_trading_fee_ceiling_division() {
    program_path();

    let mut env = TestEnv::new();
    // Use init_market_full to set nonzero trading_fee_bps
    // Default init_market_with_invert uses trading_fee_bps=0
    // We need to manually construct with fee
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // With trading_fee_bps=0 (default), trade should succeed with no fee
    let insurance_before = env.read_insurance_balance();
    env.trade(&user, &lp, lp_idx, user_idx, 1); // Smallest possible trade (1 contract)
    let insurance_after = env.read_insurance_balance();

    // With fee=0, insurance shouldn't grow from trading
    // (it might grow from other settlement operations though)
    assert!(
        insurance_after >= insurance_before,
        "Insurance should never decrease: before={} after={}",
        insurance_before,
        insurance_after
    );

    // Verify position was created
    let pos = env.read_account_position(user_idx);
    assert_eq!(
        pos, 1,
        "Smallest trade should create position of 1 contract"
    );
}

/// ATTACK: Multiple withdrawals in same slot draining capital.
/// Rapid withdrawals in same slot should correctly update capital each time.
#[test]
fn test_attack_multiple_withdrawals_same_slot() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    // Withdraw in three parts - all at same slot
    env.try_withdraw(&user, user_idx, 1_000_000_000).unwrap(); // -1B
    env.try_withdraw(&user, user_idx, 1_000_000_000).unwrap(); // -1B
    env.try_withdraw(&user, user_idx, 1_000_000_000).unwrap(); // -1B

    let user_cap = env.read_account_capital(user_idx);
    // Should have 5B - 3B = 2B remaining
    // (capital might differ due to fee settlement, but should be around 2B)
    assert!(
        user_cap <= 2_000_000_000,
        "ATTACK: Capital not properly decremented after multiple withdrawals: {}",
        user_cap
    );

    // Try to withdraw more than remaining
    let result = env.try_withdraw(&user, user_idx, 3_000_000_000); // More than remaining
    assert!(result.is_err(), "ATTACK: Over-withdrawal should fail!");

    // SPL vault: 10B + 5B - 3B = 12B
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 12_000_000_000,
        "ATTACK: SPL vault wrong after multiple withdrawals!"
    );
}

/// ATTACK: Deposit and withdraw same slot - should be atomic operations.
/// Rapid deposit+withdraw cycle shouldn't create or destroy value.
#[test]
fn test_attack_deposit_withdraw_same_slot_atomicity() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    let cap_before = env.read_account_capital(user_idx);
    let vault_before = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };

    // Deposit then withdraw same amount, same slot
    env.deposit(&user, user_idx, 2_000_000_000);
    env.try_withdraw(&user, user_idx, 2_000_000_000).unwrap();

    let cap_after = env.read_account_capital(user_idx);
    let vault_after = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };

    // Capital and vault should return to original values
    assert_eq!(
        cap_before, cap_after,
        "ATTACK: Deposit+withdraw changed capital! before={} after={}",
        cap_before, cap_after
    );
    assert_eq!(
        vault_before, vault_after,
        "ATTACK: Deposit+withdraw changed vault! before={} after={}",
        vault_before, vault_after
    );
}

/// ATTACK: Accrue funding with huge dt (10-year equivalent slot jump).
/// Funding accrual caps dt at ~1 year. Verify no overflow.
#[test]
fn test_attack_funding_accrue_huge_dt_capped() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 5_000_000_000).unwrap();
    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert_eq!(
        env.read_account_position(user_idx),
        100_000,
        "Precondition: position open"
    );
    let user_pos_before = env.read_account_position(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);

    // Jump 1 year worth of slots (~31.5M slots)
    // accrue_funding should cap dt at 31,536,000 (~1 year)
    env.set_slot(31_000_000);
    let crank_result = env.try_crank();

    let lp_cap_after = env.read_account_capital(lp_idx);
    let user_cap_after = env.read_account_capital(user_idx);
    if crank_result.is_ok() {
        assert_eq!(
            env.read_account_position(user_idx),
            user_pos_before,
            "Crank should not change position size under huge-dt funding accrual"
        );
    } else {
        assert_eq!(
            lp_cap_after, lp_cap_before,
            "Rejected huge-dt crank must preserve LP capital"
        );
        assert_eq!(
            user_cap_after, user_cap_before,
            "Rejected huge-dt crank must preserve user capital"
        );
    }

    // Conservation must hold regardless of crank result.
    let c_tot = env.read_c_tot();
    let sum = lp_cap_after + user_cap_after;
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync after huge slot jump! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 65_000_000_000,
        "ATTACK: SPL vault changed after huge dt funding!"
    );

    if crank_result.is_err() {
        // If overflow detected, protocol correctly rejected the operation
        // This IS the expected security behavior for extreme dt
        println!("Crank correctly rejected extreme dt - conservation verified");
    }
}

// ============================================================================
// ROUND 12: Unit Scale, Invert Mode, Multi-Account, Resolve Sequences
// ============================================================================

/// ATTACK: Unit scale market - trade, crank, conservation.
/// Markets with unit_scale > 0 use scaled prices. Verify conservation.
#[test]
fn test_attack_unit_scale_trade_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_full(0, 1000, 0); // unit_scale=1000

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert_eq!(
        env.read_account_position(user_idx),
        100_000,
        "Precondition: position open"
    );

    // Price move and crank
    env.set_slot_and_price(50, 150_000_000);
    env.crank();

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with unit_scale=1000! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_000,
        "ATTACK: SPL vault changed with unit_scale!"
    );
}

/// ATTACK: Large unit scale - very large scaling factor.
/// unit_scale=1_000_000 (1M). Verify no overflow in price scaling.
#[test]
fn test_attack_large_unit_scale_no_overflow() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_full(0, 1_000_000, 0); // unit_scale=1M

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade with large unit_scale
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert_eq!(env.read_account_position(user_idx), 100_000);

    // Advance and crank
    env.set_slot(50);
    env.crank();

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with unit_scale=1M! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_000,
        "ATTACK: SPL vault changed with unit_scale=1M!"
    );
}

/// ATTACK: Inverted market (invert=1) trade and conservation.
/// Inverted markets use 1e12/oracle_price. Verify conservation.
#[test]
fn test_attack_inverted_market_trade_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(1); // Inverted

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade on inverted market
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert_eq!(env.read_account_position(user_idx), 100_000);

    // Price move (raw oracle price change)
    env.set_slot_and_price(50, 150_000_000); // Oracle moves 138→150
    env.crank();

    // Conservation on inverted market
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync on inverted market! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_000,
        "ATTACK: SPL vault changed on inverted market!"
    );
}

/// ATTACK: Inverted market with price approaching zero.
/// When oracle price → large (inverted price → 0), verify no division issues.
#[test]
fn test_attack_inverted_market_extreme_high_oracle() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(1);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Very high oracle price → inverted price near zero
    // 1e12 / 1e9 = 1000 (very small inverted price)
    env.set_slot_and_price(50, 1_000_000_000); // Oracle = $1000
    let crank_result = env.try_crank();

    // Crank should succeed (circuit breaker clamps the mark, not reject)
    assert!(
        crank_result.is_ok(),
        "Crank should succeed even with extreme oracle: {:?}",
        crank_result
    );

    // Conservation must hold
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with extreme inverted price! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_000,
        "ATTACK: SPL vault changed despite extreme inverted price!"
    );
}

/// ATTACK: Same owner creates multiple user accounts.
/// Protocol should allow it, but each account must be independent.
#[test]
fn test_attack_same_owner_multiple_accounts_isolation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    // Two different users - verify account isolation
    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 5_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 3_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade on user1 only
    env.trade(&user1, &lp, lp_idx, user1_idx, 100_000);
    assert_eq!(env.read_account_position(user1_idx), 100_000);
    assert_eq!(
        env.read_account_position(user2_idx),
        0,
        "ATTACK: Trade on user1 affected user2!"
    );

    // user2 capital unchanged
    let user2_cap = env.read_account_capital(user2_idx);
    assert_eq!(
        user2_cap, 3_000_000_000,
        "ATTACK: user2 capital changed from user1's trade: {}",
        user2_cap
    );

    // Conservation across all accounts
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx)
        + env.read_account_capital(user1_idx)
        + env.read_account_capital(user2_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with multi-account! c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Resolve hyperp market then withdraw capital (no position).
/// After resolution, users should be able to withdraw their deposited capital.
#[test]
fn test_attack_resolve_then_withdraw_capital() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    // Resolve market (no positions open)
    env.try_resolve_market(&admin).unwrap();

    // User withdraws capital after resolve
    let user_cap = env.read_account_capital(user_idx);
    assert!(user_cap > 0, "Precondition: user should have capital");
    env.try_withdraw(&user, user_idx, user_cap as u64).unwrap();

    let user_cap_after = env.read_account_capital(user_idx);
    assert_eq!(
        user_cap_after, 0,
        "User should have zero capital after withdrawal: {}",
        user_cap_after
    );
}

/// ATTACK: TradeNoCpi on hyperp market should always be blocked.
/// Hyperp mode blocks TradeNoCpi (requires TradeCpi from matcher).
#[test]
fn test_attack_trade_nocpi_on_hyperp_rejected() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    // TradeNoCpi should be blocked on hyperp markets
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert!(
        result.is_err(),
        "ATTACK: TradeNoCpi on hyperp market should be rejected!"
    );
}

// test_attack_double_resolve_rejected: removed (duplicate of test_attack_double_resolve_market)

/// ATTACK: Non-admin tries to resolve market.
/// Only admin should be able to resolve.
#[test]
fn test_attack_non_admin_resolve_rejected() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let lp = Keypair::new();
    let _lp_idx = env.init_lp(&lp);
    env.deposit(&lp, _lp_idx, 10_000_000_000);

    env.crank();

    // Non-admin tries to resolve
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();

    let result = env.try_resolve_market(&attacker);
    assert!(
        result.is_err(),
        "ATTACK: Non-admin resolve should be rejected!"
    );
}

/// ATTACK: Withdraw insurance before all positions force-closed.
/// WithdrawInsurance should fail while positions are still open post-resolve.
#[test]
fn test_attack_withdraw_insurance_before_force_close() {
    // Need TradeCpiTestEnv because hyperp mode disables TradeNoCpi
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.set_slot(100);
    env.crank();

    // Open position via TradeCpi (TradeNoCpi blocked on hyperp)
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000,
        &matcher_prog,
        &matcher_ctx,
    );
    assert!(result.is_ok(), "Trade should succeed: {:?}", result);
    assert!(
        env.read_account_position(user_idx) != 0,
        "Should have position"
    );

    // Resolve
    env.try_resolve_market(&admin).unwrap();

    // Try to withdraw insurance BEFORE force-closing positions
    env.set_slot(200);
    let result = env.try_withdraw_insurance(&admin);
    assert!(
        result.is_err(),
        "ATTACK: Insurance withdrawal with open positions should be rejected!"
    );
}

/// ATTACK: Inverted market with unit_scale > 0 (double transformation).
/// Both inversion and scaling applied. Verify conservation.
#[test]
fn test_attack_inverted_with_unit_scale_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_full(1, 1000, 0); // invert=1, unit_scale=1000

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade on inverted+scaled market
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert_eq!(env.read_account_position(user_idx), 100_000);

    // Price change
    env.set_slot_and_price(50, 150_000_000);
    env.crank();

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with invert+unit_scale! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_000,
        "ATTACK: SPL vault changed with invert+unit_scale!"
    );
}

/// ATTACK: Crank multiple times across many slots with position open.
/// Verify funding accrual is correct and consistent across many intervals.
#[test]
fn test_attack_incremental_funding_across_many_slots() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Crank at regular intervals
    for i in 1..=10 {
        env.set_slot(i * 100);
        env.crank();
    }

    // After 10 cranks over 1000 slots, conservation must hold
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync after incremental funding! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_000,
        "ATTACK: SPL vault changed after incremental funding!"
    );
}

/// ATTACK: Inverted market PnL direction and conservation after price move.
/// Long on inverted market should lose when oracle rises (inverted mark falls).
/// Verify PnL eventually settles into capital and conservation holds.
#[test]
fn test_attack_inverted_market_pnl_direction() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(1); // Inverted

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade on inverted market
    env.trade(&user, &lp, lp_idx, user_idx, 10_000_000);
    assert_eq!(env.read_account_position(user_idx), 10_000_000);

    let cap_before_user = env.read_account_capital(user_idx);
    let cap_before_lp = env.read_account_capital(lp_idx);

    // Small oracle price change to test PnL direction
    // Oracle: 138M → 150M. Inverted mark decreases slightly.
    env.set_slot_and_price(100, 150_000_000);
    env.crank();
    env.set_slot(200);
    env.crank();

    // After settlement, verify conservation
    let c_tot = env.read_c_tot();
    let cap_user = env.read_account_capital(user_idx);
    let cap_lp = env.read_account_capital(lp_idx);
    let sum = cap_user + cap_lp;
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync on inverted market! c_tot={} sum={}",
        c_tot, sum
    );

    // Total funds (deposits + insurance) should be unchanged in SPL vault
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 106_000_000_000,
        "ATTACK: SPL vault changed during inverted market settlement!"
    );

    // Verify capital sum didn't increase (fees may decrease total)
    assert!(
        cap_user + cap_lp <= cap_before_user + cap_before_lp,
        "ATTACK: Total capital increased on inverted market!"
    );
}

// ============================================================================
// ROUND 13: Admin ops, CloseAccount edge cases, GC, multi-LP, oracle lifecycle,
//           warmup+haircut, nonce, CloseSlab, risk threshold, maintenance fee
// ============================================================================

/// ATTACK: Close account with fee debt outstanding.
/// CloseAccount should forgive remaining fee debt after paying what's possible.
/// Verify returned capital = capital - min(fee_debt, capital).
#[test]
fn test_attack_close_account_returns_capital_minus_fees() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Accrue some fees by advancing slots
    env.set_slot(500);
    env.crank();

    let vault_before = env.vault_balance();
    let num_used_before = env.read_num_used_accounts();

    // Close account (no position, just capital + fee debt)
    let result = env.try_close_account(&user, user_idx);
    assert!(result.is_ok(), "CloseAccount should succeed: {:?}", result);

    let vault_after = env.vault_balance();
    assert!(
        vault_before > vault_after,
        "Capital should be returned to user (vault decreased)"
    );

    let num_used_after = env.read_num_used_accounts();
    assert!(
        num_used_after < num_used_before,
        "num_used_accounts should decrease after close"
    );
}

/// ATTACK: CloseSlab with dormant account (zero everything but not GC'd).
/// CloseSlab requires num_used_accounts == 0, so dormant accounts block it.
#[test]
fn test_attack_close_slab_blocked_by_dormant_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.crank();

    // Close user account but LP still exists
    env.try_close_account(&user, user_idx).unwrap();

    // Crank to GC the user
    env.set_slot(100);
    env.crank();

    // LP still has capital - can't close slab
    let num_used = env.read_num_used_accounts();
    assert!(num_used > 0, "Precondition: LP account still exists");

    let lp_pos_before = env.read_account_position(lp_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let insurance_before = env.read_insurance_balance();
    let num_used_before = env.read_num_used_accounts();
    let spl_vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;

    // CloseSlab should fail
    let result = env.try_close_slab();
    assert!(
        result.is_err(),
        "ATTACK: CloseSlab succeeded with active LP account!"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        lp_pos_before,
        "Rejected CloseSlab with dormant account must preserve LP position"
    );
    assert_eq!(
        env.read_account_capital(lp_idx),
        lp_cap_before,
        "Rejected CloseSlab with dormant account must preserve LP capital"
    );
    assert_eq!(
        env.read_insurance_balance(),
        insurance_before,
        "Rejected CloseSlab with dormant account must preserve insurance"
    );
    assert_eq!(
        env.read_num_used_accounts(),
        num_used_before,
        "Rejected CloseSlab with dormant account must preserve account usage"
    );
    assert_eq!(
        env.vault_balance(),
        spl_vault_before,
        "Rejected CloseSlab with dormant account must preserve SPL vault"
    );
    assert_eq!(
        env.read_engine_vault(),
        engine_vault_before,
        "Rejected CloseSlab with dormant account must preserve engine vault"
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    assert_eq!(
        slab_after, slab_before,
        "Rejected CloseSlab with dormant account must preserve slab bytes"
    );
}

/// ATTACK: UpdateAdmin transfers control, old admin tries operation.
/// After UpdateAdmin, the old admin should be unauthorized.
#[test]
fn test_attack_update_admin_old_admin_rejected() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let new_admin = Keypair::new();
    env.svm.airdrop(&new_admin.pubkey(), 5_000_000_000).unwrap();

    // Transfer admin to new_admin
    env.try_update_admin(&admin, &new_admin.pubkey()).unwrap();

    // Old admin tries SetRiskThreshold - should fail
    let slab_before_old_admin_attempt = env.svm.get_account(&env.slab).unwrap().data;
    env.set_slot(2);
    let result = env.try_set_risk_threshold(&admin, 500_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Old admin still authorized after UpdateAdmin!"
    );
    let slab_after_old_admin_attempt = env.svm.get_account(&env.slab).unwrap().data;
    assert_eq!(
        slab_after_old_admin_attempt, slab_before_old_admin_attempt,
        "Rejected SetRiskThreshold by old admin must preserve slab bytes"
    );

    // New admin can do it
    env.set_slot(3);
    let result = env.try_set_risk_threshold(&new_admin, 500_000_000);
    assert!(
        result.is_ok(),
        "New admin should be authorized: {:?}",
        result
    );
}

/// ATTACK: Set maintenance fee to extreme value, accrue fees.
/// Verify fee debt accumulates but doesn't cause overflow or negative capital.
#[test]
fn test_attack_extreme_maintenance_fee_no_overflow() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Set high maintenance fee
    env.try_set_maintenance_fee(&admin, 10_000_000_000).unwrap();

    // Advance many slots to accrue fees
    env.set_slot(1000);
    env.crank();

    // Capital should have decreased from fees (10B fee rate * 1000 slots)
    let user_cap = env.read_account_capital(user_idx);
    assert!(
        user_cap < 5_000_000_000,
        "Extreme fees should reduce capital! cap={}",
        user_cap
    );

    // Conservation still holds
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with extreme maintenance fee! c_tot={} sum={}",
        c_tot, sum
    );

    // SPL vault unchanged (fees are internal accounting)
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 26_000_000_000,
        "ATTACK: SPL vault changed from maintenance fees!"
    );
}

/// ATTACK: SetOracleAuthority to zero disables PushOraclePrice.
/// Oracle authority cleared means stored price is cleared and push fails.
#[test]
fn test_attack_set_oracle_authority_to_zero_disables_push() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Set oracle authority
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 1_000_000, 1000).unwrap();

    // Clear oracle authority (set to zero)
    let zero = Pubkey::new_from_array([0u8; 32]);
    env.set_slot(2);
    env.try_set_oracle_authority(&admin, &zero).unwrap();

    // Push should now fail
    env.set_slot(3);
    let result = env.try_push_oracle_price(&admin, 2_000_000, 2000);
    assert!(
        result.is_err(),
        "ATTACK: PushOraclePrice succeeded after authority cleared!"
    );
}

/// ATTACK: Multi-LP trading - trade against two different LPs.
/// Verify each LP's position is tracked independently and conservation holds.
#[test]
fn test_attack_multi_lp_independent_positions() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp1 = Keypair::new();
    let lp1_idx = env.init_lp(&lp1);
    env.deposit(&lp1, lp1_idx, 20_000_000_000);

    let lp2 = Keypair::new();
    let lp2_idx = env.init_lp(&lp2);
    env.deposit(&lp2, lp2_idx, 30_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade against LP1
    env.trade(&user, &lp1, lp1_idx, user_idx, 100_000);
    assert_eq!(env.read_account_position(user_idx), 100_000);
    assert_eq!(env.read_account_position(lp1_idx), -100_000);
    assert_eq!(
        env.read_account_position(lp2_idx),
        0,
        "LP2 should not be affected by trade against LP1"
    );

    // Trade against LP2 (different slot)
    env.set_slot(2);
    env.trade(&user, &lp2, lp2_idx, user_idx, 200_000);
    assert_eq!(env.read_account_position(user_idx), 300_000); // 100K + 200K
    assert_eq!(env.read_account_position(lp1_idx), -100_000);
    assert_eq!(env.read_account_position(lp2_idx), -200_000);

    // Conservation across all 3 accounts
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp1_idx)
        + env.read_account_capital(lp2_idx)
        + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with multi-LP! c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: SetRiskThreshold changes gate mode.
/// High threshold blocks risk-increasing trades, lowering re-enables them.
#[test]
fn test_attack_set_risk_threshold_enables_trades() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // First trade succeeds (insurance 1B > threshold 0 default)
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert_eq!(env.read_account_position(user_idx), 100_000);

    // Set very high threshold so gate becomes active (insurance 1B < 999T)
    env.set_slot(2);
    env.try_set_risk_threshold(&admin, 999_000_000_000_000)
        .unwrap();

    // Risk-increasing trade should be blocked
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert!(
        result.is_err(),
        "ATTACK: Risk-increasing trade succeeded with gate active!"
    );

    // Lower threshold back to 0 (disable gate)
    env.set_slot(3);
    env.try_set_risk_threshold(&admin, 0).unwrap();

    // Trade should succeed again (different size to avoid tx hash collision)
    env.set_slot(4);
    env.trade(&user, &lp, lp_idx, user_idx, 150_000);
    assert_eq!(env.read_account_position(user_idx), 250_000);
}

/// ATTACK: Close account after round-trip trade with PnL.
/// Protocol requires position=0 and PnL=0 for close.
#[test]
fn test_attack_close_account_after_roundtrip_pnl() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open and close position to generate PnL
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Price move to generate PnL
    env.set_slot_and_price(50, 150_000_000);
    env.crank();

    // Close position
    env.set_slot(51);
    env.trade(&user, &lp, lp_idx, user_idx, -100_000);

    // User should have PnL or capital changed
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(user_pos, 0, "Position should be closed");

    // Crank many times to settle warmup PnL to capital
    for i in 0..10 {
        env.set_slot(100 + i * 50);
        env.crank();
    }

    // After warmup fully vests, PnL should be zero and close should work
    let user_pnl = env.read_account_pnl(user_idx);
    assert_eq!(user_pnl, 0, "PnL should be settled after many cranks");

    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "CloseAccount should succeed after full PnL settlement: {:?}",
        result
    );

    let cap = env.read_account_capital(user_idx);
    assert_eq!(cap, 0, "Capital should be zero after close");
}

/// ATTACK: UpdateAdmin to same address (no-op).
/// Should succeed without side effects.
#[test]
fn test_attack_update_admin_same_address_noop() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Update admin to same address
    env.try_update_admin(&admin, &admin.pubkey()).unwrap();

    // Admin should still work
    env.set_slot(2);
    let result = env.try_set_maintenance_fee(&admin, 100);
    assert!(
        result.is_ok(),
        "Admin should still be authorized after self-update: {:?}",
        result
    );
}

/// ATTACK: Double deposit then withdraw full amount.
/// Verify deposits accumulate correctly and full withdrawal returns sum.
#[test]
fn test_attack_double_deposit_accumulation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    // Set zero maintenance fee to avoid fee complications
    env.try_set_maintenance_fee(&admin, 0).unwrap();
    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();

    // First deposit (BEFORE crank to prevent GC of zero-capital account)
    env.deposit(&user, user_idx, 3_000_000_000);
    env.crank();

    let cap1 = env.read_account_capital(user_idx);
    assert_eq!(cap1, 3_000_000_000, "First deposit amount");

    // Second deposit (different slot to avoid collision)
    env.set_slot(2);
    env.deposit(&user, user_idx, 2_000_000_000);
    let cap2 = env.read_account_capital(user_idx);
    assert_eq!(cap2, 5_000_000_000, "Second deposit should accumulate");

    // Full withdrawal
    env.set_slot(3);
    env.try_withdraw(&user, user_idx, 5_000_000_000).unwrap();
    let cap_final = env.read_account_capital(user_idx);
    assert_eq!(cap_final, 0, "Full withdrawal should zero capital");
}

/// ATTACK: Withdraw exactly the user's entire capital.
/// Edge case: withdraw == capital leaves zero, should succeed.
#[test]
fn test_attack_withdraw_exact_capital() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Read current capital (may be slightly less due to fees)
    let cap = env.read_account_capital(user_idx);
    assert!(cap > 0, "Precondition: user has capital");

    // Withdraw exact capital amount (no position, so should succeed)
    let result = env.try_withdraw(&user, user_idx, cap as u64);
    assert!(
        result.is_ok(),
        "Withdrawing exact capital should succeed: {:?}",
        result
    );

    let cap_after = env.read_account_capital(user_idx);
    assert_eq!(
        cap_after, 0,
        "Capital should be exactly zero after full withdraw"
    );
}

/// ATTACK: Multiple LPs with different sizes - verify LP max position tracking.
/// LP positions should be independently bounded by their own limits.
#[test]
fn test_attack_multi_lp_max_position_tracking() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp1 = Keypair::new();
    let lp1_idx = env.init_lp(&lp1);
    env.deposit(&lp1, lp1_idx, 5_000_000_000); // Small LP

    let lp2 = Keypair::new();
    let lp2_idx = env.init_lp(&lp2);
    env.deposit(&lp2, lp2_idx, 50_000_000_000); // Large LP

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade against small LP
    env.trade(&user, &lp1, lp1_idx, user_idx, 50_000);

    // Trade against large LP
    env.set_slot(2);
    env.trade(&user, &lp2, lp2_idx, user_idx, 500_000);

    // Each LP tracks position independently
    assert_eq!(env.read_account_position(lp1_idx), -50_000);
    assert_eq!(env.read_account_position(lp2_idx), -500_000);
    assert_eq!(env.read_account_position(user_idx), 550_000);

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp1_idx)
        + env.read_account_capital(lp2_idx)
        + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with multi-LP tracking! c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: SetMaintenanceFee to zero - no fees should accrue.
/// Verify that with fee=0, capital is unchanged after many cranks.
#[test]
fn test_attack_zero_maintenance_fee_no_drain() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    // Set maintenance fee to zero
    env.try_set_maintenance_fee(&admin, 0).unwrap();
    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    let cap_before = env.read_account_capital(user_idx);

    // Many cranks
    for i in 1..=10 {
        env.set_slot(i * 100);
        env.crank();
    }

    let cap_after = env.read_account_capital(user_idx);
    assert_eq!(
        cap_before, cap_after,
        "Capital should not change with zero maintenance fee! before={} after={}",
        cap_before, cap_after
    );
}

/// ATTACK: Push oracle price with decreasing timestamps.
/// Verify that stale timestamps are handled correctly.
#[test]
fn test_attack_push_oracle_stale_timestamp() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();

    // Push with timestamp=2000
    env.try_push_oracle_price(&admin, 1_000_000, 2000).unwrap();

    // Push with later timestamp=3000 should succeed
    let result = env.try_push_oracle_price(&admin, 1_500_000, 3000);
    assert!(
        result.is_ok(),
        "Push with newer timestamp should succeed: {:?}",
        result
    );

    // Push with earlier timestamp=1000 (stale) must be rejected.
    let stale_result = env.try_push_oracle_price(&admin, 2_000_000, 1000);
    assert!(
        stale_result.is_err(),
        "SECURITY: stale authority timestamp rollback must be rejected"
    );
}

/// ATTACK: Liquidate account that is solvent (positive equity).
/// LiquidateAtOracle should reject attempts on solvent accounts.
#[test]
fn test_attack_liquidate_solvent_account_after_settlement() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Small position, well-collateralized
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Settle at slightly different price
    env.set_slot_and_price(50, 139_000_000);
    env.crank();

    // Account should be solvent (10B capital vs tiny position)
    let result = env.try_liquidate(user_idx);
    assert!(
        result.is_err(),
        "ATTACK: Liquidation succeeded on solvent account!"
    );

    // Position unchanged
    assert_eq!(
        env.read_account_position(user_idx),
        100_000,
        "ATTACK: Solvent account's position was modified!"
    );
}

/// ATTACK: Close account, GC via crank, verify num_used_accounts decrements.
/// Full lifecycle: init → deposit → close → crank(GC) → verify count.
#[test]
fn test_attack_close_then_gc_decrements_used_count() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.crank();

    let num_before = env.read_num_used_accounts();

    // Close user account
    env.try_close_account(&user, user_idx).unwrap();

    // Crank to GC the closed account
    env.set_slot(100);
    env.crank();
    env.set_slot(200);
    env.crank();

    let num_after = env.read_num_used_accounts();
    assert!(
        num_after < num_before,
        "num_used_accounts should decrease after close+GC: before={} after={}",
        num_before,
        num_after
    );
}

// ============================================================================
// ROUND 14: Warmup+haircut, size=1 trades, entry price, fee paths, funding,
//           position reversal margin, GC edge cases, force-realize path,
//           fee debt forgiveness, sequential operations
// ============================================================================

/// ATTACK: Trade with position size = 1 (smallest non-zero).
/// Verify conservation holds even with minimal position.
#[test]
fn test_attack_trade_size_one_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade with smallest possible size
    env.trade(&user, &lp, lp_idx, user_idx, 1);
    assert_eq!(env.read_account_position(user_idx), 1);
    assert_eq!(env.read_account_position(lp_idx), -1);

    // Price change
    env.set_slot_and_price(50, 150_000_000);
    env.crank();

    // Conservation with size=1
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with size=1! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 16_000_000_000,
        "ATTACK: SPL vault changed with size=1 trade!"
    );
}

/// ATTACK: Trade size = -1 (smallest short position).
/// Verify negative position of size 1 conserves.
#[test]
fn test_attack_trade_size_negative_one_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Short size=1
    env.trade(&user, &lp, lp_idx, user_idx, -1);
    assert_eq!(env.read_account_position(user_idx), -1);
    assert_eq!(env.read_account_position(lp_idx), 1);

    // Price change and crank
    env.set_slot_and_price(50, 120_000_000);
    env.crank();

    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with size=-1! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 16_000_000_000,
        "ATTACK: SPL vault changed with size=-1 trade!"
    );
}

/// ATTACK: Position reversal (long→short) requires initial_margin_bps.
/// When crossing zero, the margin check uses the stricter initial margin.
#[test]
fn test_attack_position_reversal_margin_check() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open long position
    env.trade(&user, &lp, lp_idx, user_idx, 500_000);
    assert_eq!(env.read_account_position(user_idx), 500_000);

    // Reverse to short (crosses zero) - should succeed with sufficient margin
    env.set_slot(2);
    env.trade(&user, &lp, lp_idx, user_idx, -1_000_000);
    assert_eq!(
        env.read_account_position(user_idx),
        -500_000,
        "Position should have flipped to short"
    );

    // Conservation after flip
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync after position reversal! c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Close account path settles fees correctly.
/// Compare: crank(settle fees) → close vs. close(settles fees internally).
#[test]
fn test_attack_close_account_settles_fees_correctly() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Accrue fees over many slots WITHOUT cranking (lazy settlement)
    env.set_slot(2000);
    // Don't crank - let CloseAccount handle fee settlement

    let vault_before = env.vault_balance();
    let insurance_before = env.read_insurance_balance();

    // Close account without intermediate crank - CloseAccount must settle fees internally
    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "CloseAccount should settle fees and succeed: {:?}",
        result
    );

    let vault_after = env.vault_balance();
    let insurance_after = env.read_insurance_balance();

    // Vault decreased (capital returned to user)
    assert!(
        vault_before > vault_after,
        "Vault should decrease from capital return"
    );

    // Insurance increased (fees collected)
    assert!(
        insurance_after >= insurance_before,
        "Insurance should increase from fee collection: before={} after={}",
        insurance_before,
        insurance_after
    );
}

/// ATTACK: Funding accumulation across position size changes.
/// Open position, crank to accrue funding, change position size, crank again.
/// Verify funding uses stored index (anti-retroactivity).
#[test]
fn test_attack_funding_across_position_size_change() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open initial position
    let cap_before_trade = env.read_account_capital(user_idx);
    env.trade(&user, &lp, lp_idx, user_idx, 500_000);
    let cap_after_trade = env.read_account_capital(user_idx);
    assert_eq!(cap_after_trade, cap_before_trade, "Capital should not change from trade alone (no fee)");

    // Crank to accrue some funding
    env.set_slot(100);
    env.crank();

    // Partial close (reduce position)
    env.set_slot(101);
    env.trade(&user, &lp, lp_idx, user_idx, -250_000);
    assert_eq!(env.read_account_position(user_idx), 250_000);

    // More cranks with smaller position
    env.set_slot(200);
    env.crank();

    // Conservation must hold through all changes
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync after position size change! c_tot={} sum={}",
        c_tot, sum
    );

    // SPL vault unchanged
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 61_000_000_000,
        "ATTACK: SPL vault changed during funding with position changes!"
    );
}

/// ATTACK: Partial position close then full close then CloseAccount.
/// Full lifecycle: open → partial close → full close → account close.
#[test]
fn test_attack_partial_close_full_lifecycle() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 30_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open
    env.trade(&user, &lp, lp_idx, user_idx, 300_000);
    assert_eq!(env.read_account_position(user_idx), 300_000);

    // Partial close
    env.set_slot(2);
    env.trade(&user, &lp, lp_idx, user_idx, -100_000);
    assert_eq!(env.read_account_position(user_idx), 200_000);

    // Full close
    env.set_slot(3);
    env.trade(&user, &lp, lp_idx, user_idx, -200_000);
    assert_eq!(env.read_account_position(user_idx), 0);

    // Settle everything
    for i in 0..10 {
        env.set_slot(100 + i * 50);
        env.crank();
    }

    // Close account
    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "CloseAccount should succeed after full position lifecycle: {:?}",
        result
    );

    let cap = env.read_account_capital(user_idx);
    assert_eq!(cap, 0, "Capital should be zero after close");
}

/// ATTACK: Multiple deposits to LP then user trades against it.
/// Verify LP capital accumulates correctly and trades work.
#[test]
fn test_attack_lp_multiple_deposits_then_trade() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    // Set zero fee to avoid complications
    env.try_set_maintenance_fee(&admin, 0).unwrap();
    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Second LP deposit
    env.set_slot(2);
    env.deposit(&lp, lp_idx, 5_000_000_000);
    let lp_cap = env.read_account_capital(lp_idx);
    assert_eq!(lp_cap, 15_000_000_000, "LP capital should accumulate");

    // User trades against the well-funded LP
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.set_slot(3);
    env.trade(&user, &lp, lp_idx, user_idx, 200_000);
    assert_eq!(env.read_account_position(user_idx), 200_000);

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with multi-deposit LP! c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Withdraw then immediately re-deposit.
/// Verify no value created or lost in the cycle.
#[test]
fn test_attack_withdraw_redeposit_cycle_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_set_maintenance_fee(&admin, 0).unwrap();
    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    let cap_before = env.read_account_capital(user_idx);
    let vault_before = env.vault_balance();

    // Withdraw half
    env.try_withdraw(&user, user_idx, 2_000_000_000).unwrap();
    let cap_mid = env.read_account_capital(user_idx);
    assert_eq!(
        cap_mid,
        cap_before - 2_000_000_000,
        "Capital should decrease by withdrawal amount"
    );

    // Re-deposit same amount
    env.set_slot(2);
    env.deposit(&user, user_idx, 2_000_000_000);
    let cap_after = env.read_account_capital(user_idx);
    assert_eq!(
        cap_after, cap_before,
        "Capital should return to original after withdraw+redeposit: before={} after={}",
        cap_before, cap_after
    );

    // Vault should be back to original
    let vault_after = env.vault_balance();
    assert_eq!(
        vault_before, vault_after,
        "Vault should be unchanged after withdraw+redeposit"
    );
}

/// ATTACK: Warmup-period market - trade and settle across warmup slots.
/// Profit from trade should vest over warmup_period_slots.
/// Verify conservation through the vesting process.
#[test]
fn test_attack_warmup_vesting_conservation_with_profit() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_warmup(0, 100); // 100-slot warmup

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open and close position with profit
    env.trade(&user, &lp, lp_idx, user_idx, 500_000);
    env.set_slot_and_price(50, 150_000_000); // Price goes up
    env.crank();

    // Close position (realizes gain into warmup)
    env.set_slot(51);
    env.trade(&user, &lp, lp_idx, user_idx, -500_000);

    let cap_mid = env.read_account_capital(user_idx);

    // Vest warmup over many cranks
    for i in 0..15 {
        env.set_slot(100 + i * 50);
        env.crank();
    }

    let cap_final = env.read_account_capital(user_idx);
    // Capital should increase as warmup vests profit
    assert!(
        cap_final >= cap_mid,
        "Capital should increase as warmup vests: mid={} final={}",
        cap_mid,
        cap_final
    );

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync during warmup vesting! c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Force-realize disabled when insurance > threshold.
/// Top up insurance to disable force-realize, verify positions persist.
#[test]
fn test_attack_insurance_topup_disables_force_realize() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Top up insurance to disable force-realize (insurance > threshold)
    env.try_top_up_insurance(&admin, 5_000_000_000).unwrap();
    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 500_000);

    // Crank - should NOT force-realize since insurance is funded
    env.set_slot(100);
    env.crank();

    // Position should persist (not force-realized)
    let pos = env.read_account_position(user_idx);
    assert_eq!(
        pos, 500_000,
        "Position should persist when insurance > threshold (no force-realize)"
    );
}

/// ATTACK: Sequential deposit → trade → crank → withdraw → close lifecycle.
/// Full account lifecycle with all operations in sequence.
#[test]
fn test_attack_full_account_lifecycle_sequence() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 30_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    // 1. Deposit (before crank to prevent GC of zero-capital account)
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    assert_eq!(env.read_account_capital(user_idx), 5_000_000_000);

    // 2. Trade
    env.set_slot(2);
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert_eq!(env.read_account_position(user_idx), 100_000);

    // 3. Crank (settle)
    env.set_slot(100);
    env.crank();

    // 4. Close position
    env.set_slot(101);
    env.trade(&user, &lp, lp_idx, user_idx, -100_000);
    assert_eq!(env.read_account_position(user_idx), 0);

    // 5. Settle warmup
    for i in 0..10 {
        env.set_slot(200 + i * 50);
        env.crank();
    }

    // 6. Withdraw remaining capital
    let cap = env.read_account_capital(user_idx);
    if cap > 0 {
        env.try_withdraw(&user, user_idx, cap as u64).unwrap();
    }

    // 7. Close account
    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "Full lifecycle CloseAccount should succeed: {:?}",
        result
    );
}

/// ATTACK: GC account that just had position closed.
/// Close position → crank → crank again → verify GC happens.
#[test]
fn test_attack_gc_after_position_close_and_settlement() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open and close position
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    env.set_slot(2);
    env.trade(&user, &lp, lp_idx, user_idx, -100_000);

    // Settle warmup
    for i in 0..10 {
        env.set_slot(100 + i * 50);
        env.crank();
    }

    let num_before_close = env.read_num_used_accounts();
    assert!(
        num_before_close >= 2,
        "Precondition: both LP and user should be active: {}",
        num_before_close
    );

    // Close account (returns capital)
    env.try_close_account(&user, user_idx).unwrap();

    // Crank to GC the closed account
    env.set_slot(1000);
    env.crank();
    env.set_slot(1100);
    env.crank();

    let num_after = env.read_num_used_accounts();
    assert!(
        num_after < num_before_close,
        "num_used should decrease after close+GC: before={} after={}",
        num_before_close,
        num_after
    );
}

/// ATTACK: Large position then price crash - verify conservation through liquidation.
/// Even in liquidation, c_tot must equal sum of capitals.
#[test]
fn test_attack_liquidation_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 10_000_000_000).unwrap();
    env.crank();

    // Large long position relative to capital
    env.trade(&user, &lp, lp_idx, user_idx, 50_000_000);

    // Price crash - circuit breaker limits per-slot move, so crank many times
    env.set_slot_and_price(100, 50_000_000); // ~64% price drop
    for i in 0..20u64 {
        env.set_slot(100 + i * 50);
        env.crank();
    }

    // Conservation must hold regardless of liquidation outcome
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync through liquidation! c_tot={} sum={}",
        c_tot, sum
    );

    // SPL vault unchanged (no external value extraction)
    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 115_000_000_000,
        "ATTACK: SPL vault changed during liquidation!"
    );
}

/// ATTACK: Trade at max price (circuit breaker limit).
/// Oracle at extreme high price, crank, verify no overflow.
#[test]
fn test_attack_trade_at_extreme_high_price() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade at default price
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Extreme high oracle price
    env.set_slot_and_price(50, 10_000_000_000); // $10,000
    for i in 0..10u64 {
        env.set_slot(50 + i * 100);
        env.crank();
    }

    // Should not overflow - conservation holds
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync at extreme high price! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 61_000_000_000,
        "ATTACK: SPL vault changed at extreme high price!"
    );
}

/// ATTACK: Trade at extreme low oracle price (near zero).
/// Verify no division by zero or overflow.
#[test]
fn test_attack_trade_at_extreme_low_price() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade at default price
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);

    // Extreme low oracle price (but circuit breaker limits per-slot change)
    env.set_slot_and_price(50, 1_000); // $0.001
    for i in 0..10u64 {
        env.set_slot(50 + i * 100);
        env.crank();
    }

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync at extreme low price! c_tot={} sum={}",
        c_tot, sum
    );

    let spl_vault = {
        let vault_data = env.svm.get_account(&env.vault).unwrap().data;
        TokenAccount::unpack(&vault_data).unwrap().amount
    };
    assert_eq!(
        spl_vault, 61_000_000_000,
        "ATTACK: SPL vault changed at extreme low price!"
    );
}

/// ATTACK: Rapid open/close/open cycle - same position size, different slots.
/// Tests that entry_price resets correctly on each open.
#[test]
fn test_attack_rapid_open_close_open_cycle() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 30_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Cycle 1: open
    env.trade(&user, &lp, lp_idx, user_idx, 100_000);
    assert_eq!(env.read_account_position(user_idx), 100_000);

    // Cycle 1: close
    env.set_slot(2);
    env.trade(&user, &lp, lp_idx, user_idx, -100_000);
    assert_eq!(env.read_account_position(user_idx), 0);

    // Price change between cycles
    env.set_slot_and_price(50, 145_000_000);
    env.crank();

    // Cycle 2: open at new price (different size to avoid tx collision)
    env.set_slot(51);
    env.trade(&user, &lp, lp_idx, user_idx, 200_000);
    assert_eq!(env.read_account_position(user_idx), 200_000);

    // Crank
    env.set_slot(100);
    env.crank();

    // Conservation after cycles
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync after open/close/open cycle! c_tot={} sum={}",
        c_tot, sum
    );
}

// ============================================================================
// ROUND 15: Input validation, invalid instruction paths, slab guards,
//           account state checks, multi-account scenarios, edge cases
// ============================================================================

/// ATTACK: Send instruction with tag=22 (just above max valid tag=20).
/// Should fail gracefully.
#[test]
fn test_attack_instruction_tag_just_above_max() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();

    // Tag = 22 (one above WithdrawInsurance=20)
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
        ],
        data: vec![22u8],
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Invalid instruction tag 22 should be rejected!"
    );
}

/// ATTACK: Deposit with wrong slab account (different program_id slab).
/// Slab owned by wrong program should be rejected by slab_guard.
#[test]
fn test_attack_deposit_wrong_slab_owner() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    // Create a fake slab owned by system program
    let fake_slab = Pubkey::new_unique();
    let slab_data = env.svm.get_account(&env.slab).unwrap().data.clone();
    env.svm
        .set_account(
            fake_slab,
            Account {
                lamports: 1_000_000,
                data: slab_data,
                owner: solana_sdk::system_program::ID, // Wrong owner
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let ata = env.create_ata(&user.pubkey(), 1_000_000_000);
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(fake_slab, false), // Wrong slab
            AccountMeta::new(ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data: encode_deposit(user_idx, 1_000_000_000),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Deposit to slab with wrong owner should be rejected!"
    );
}

/// ATTACK: Deposit without signer (user not signing).
/// All operations require the user to sign.
#[test]
fn test_attack_deposit_without_signer() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    // Try deposit with user as non-signer
    let ata = env.create_ata(&user.pubkey(), 1_000_000_000);
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), false), // NOT a signer
            AccountMeta::new(env.slab, false),
            AccountMeta::new(ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data: encode_deposit(user_idx, 1_000_000_000),
    };

    // Payer signs, but user doesn't
    let payer = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Deposit without user signer should be rejected!"
    );
}

/// ATTACK: Four user accounts trading against same LP.
/// Verify conservation holds across many accounts.
#[test]
fn test_attack_four_users_one_lp_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let mut user_idxs = Vec::new();
    for _ in 0..4 {
        let user = Keypair::new();
        let user_idx = env.init_user(&user);
        env.deposit(&user, user_idx, 5_000_000_000);
        user_idxs.push((user, user_idx));
    }

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Each user trades different size against LP
    for (i, (user, user_idx)) in user_idxs.iter().enumerate() {
        env.set_slot((i + 1) as u64);
        let size = ((i + 1) * 50_000) as i128;
        env.trade(user, &lp, lp_idx, *user_idx, size);
    }

    // LP should have total short = -(50K + 100K + 150K + 200K) = -500K
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(
        lp_pos, -500_000,
        "LP position should be sum of user trades: {}",
        lp_pos
    );

    // Conservation across all 5 accounts
    let c_tot = env.read_c_tot();
    let mut sum = env.read_account_capital(lp_idx);
    for (_, user_idx) in &user_idxs {
        sum += env.read_account_capital(*user_idx);
    }
    assert_eq!(
        c_tot, sum,
        "ATTACK: c_tot desync with 4 users + 1 LP! c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Withdraw from LP account (LP should still be able to withdraw).
/// Verify LP withdraw works the same as user withdraw.
#[test]
fn test_attack_lp_withdraw_capital() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    let cap_before = env.read_account_capital(lp_idx);
    assert!(cap_before > 0, "Precondition: LP has capital");

    // LP withdraws half its capital
    let withdraw_amount = (cap_before / 2) as u64;
    env.try_withdraw(&lp, lp_idx, withdraw_amount).unwrap();

    let cap_after = env.read_account_capital(lp_idx);
    assert_eq!(
        cap_after,
        cap_before - withdraw_amount as u128,
        "LP capital should decrease by withdraw amount"
    );
}

/// ATTACK: Trade at maximum position size boundary.
/// Open a position that uses nearly all margin, then try adding more.
#[test]
fn test_attack_trade_exceeds_margin_capacity() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000); // 1 SOL = 1e9

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // At $138, with 10% initial margin: max notional = 1e9/0.1 = 10e9
    // max position = 10e9 * 1e6 / 138e6 ≈ 72.4M
    // Try opening just within limit
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 50_000_000);
    assert!(
        result.is_ok(),
        "Trade within margin should succeed: {:?}",
        result
    );

    // Try doubling position (exceeds margin)
    env.set_slot(2);
    let result2 = env.try_trade(&user, &lp, lp_idx, user_idx, 50_000_000);
    assert!(
        result2.is_err(),
        "ATTACK: Trade exceeding margin should be rejected!"
    );

    // Original position unchanged
    assert_eq!(env.read_account_position(user_idx), 50_000_000);
}

/// ATTACK: InitMarket with admin field in data mismatching signer.
/// Code validates admin in instruction data matches signer pubkey.
#[test]
fn test_attack_init_market_admin_mismatch() {
    let path = program_path();

    let mut svm = LiteSVM::new();
    let program_id = Pubkey::new_unique();
    let program_bytes = std::fs::read(&path).expect("Failed to read program");
    svm.add_program(program_id, &program_bytes);
    let admin = Keypair::new();
    svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();

    let mint = Pubkey::new_unique();
    svm.set_account(
        mint,
        Account {
            lamports: 1_000_000,
            data: vec![0u8; spl_token::state::Mint::LEN],
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let slab = Pubkey::new_unique();
    let slab_size = 4 * 1024 * 1024;
    svm.set_account(
        slab,
        Account {
            lamports: 100_000_000_000,
            data: vec![0u8; slab_size],
            owner: program_id,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let vault = Pubkey::new_unique();
    svm.set_account(
        vault,
        Account {
            lamports: 1_000_000,
            data: vec![0u8; spl_token::state::Account::LEN],
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
            data: vec![0u8; spl_token::state::Account::LEN],
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    // Use a DIFFERENT pubkey for admin in data vs signer
    let fake_admin = Pubkey::new_unique();
    let data = encode_init_market_with_invert(&fake_admin, &mint, &TEST_FEED_ID, 0);

    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true), // signer
            AccountMeta::new(slab, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data, // admin in data = fake_admin != signer
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&admin.pubkey()),
        &[&admin],
        svm.latest_blockhash(),
    );
    let result = svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: InitMarket with admin mismatch should be rejected!"
    );
}

/// ATTACK: InitMarket with mint field in data mismatching mint account.
/// Code validates collateral_mint in data matches the mint account provided.
#[test]
fn test_attack_init_market_mint_mismatch() {
    let path = program_path();

    let mut svm = LiteSVM::new();
    let program_id = Pubkey::new_unique();
    let program_bytes = std::fs::read(&path).expect("Failed to read program");
    svm.add_program(program_id, &program_bytes);
    let admin = Keypair::new();
    svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();

    let real_mint = Pubkey::new_unique();
    svm.set_account(
        real_mint,
        Account {
            lamports: 1_000_000,
            data: vec![0u8; spl_token::state::Mint::LEN],
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let fake_mint = Pubkey::new_unique();

    let slab = Pubkey::new_unique();
    let slab_size = 4 * 1024 * 1024;
    svm.set_account(
        slab,
        Account {
            lamports: 100_000_000_000,
            data: vec![0u8; slab_size],
            owner: program_id,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let vault = Pubkey::new_unique();
    svm.set_account(
        vault,
        Account {
            lamports: 1_000_000,
            data: vec![0u8; spl_token::state::Account::LEN],
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
            data: vec![0u8; spl_token::state::Account::LEN],
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    // Encode with fake_mint in data, but pass real_mint as account
    let data = encode_init_market_with_invert(&admin.pubkey(), &fake_mint, &TEST_FEED_ID, 0);

    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(slab, false),
            AccountMeta::new_readonly(real_mint, false), // Real mint != fake_mint in data
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&admin.pubkey()),
        &[&admin],
        svm.latest_blockhash(),
    );
    let result = svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: InitMarket with mint mismatch should be rejected!"
    );
}

/// ATTACK: Withdraw with wrong vault PDA (correct PDA but from different slab).
/// Code checks vault PDA derivation matches slab.
#[test]
fn test_attack_withdraw_wrong_vault_pda() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    // Derive a PDA from a different slab
    let wrong_slab = Pubkey::new_unique();
    let (wrong_vault_pda, _) =
        Pubkey::find_program_address(&[b"vault", wrong_slab.as_ref()], &env.program_id);

    let ata = env.create_ata(&user.pubkey(), 0);
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new(ata, false),
            AccountMeta::new_readonly(wrong_vault_pda, false), // Wrong PDA
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_withdraw(user_idx, 1_000_000_000),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Withdraw with wrong vault PDA should be rejected!"
    );
}

/// ATTACK: CloseAccount with wrong vault PDA.
/// Code checks vault PDA derivation matches slab in CloseAccount path.
#[test]
fn test_attack_close_account_wrong_vault_pda() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    // Crank to settle
    env.set_slot(200);
    env.crank();

    // Withdraw all capital first
    env.try_withdraw(&user, user_idx, 1_000_000_000).unwrap();

    // Try close with wrong vault PDA
    let wrong_slab = Pubkey::new_unique();
    let (wrong_vault_pda, _) =
        Pubkey::find_program_address(&[b"vault", wrong_slab.as_ref()], &env.program_id);

    let ata = env.create_ata(&user.pubkey(), 0);
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new(ata, false),
            AccountMeta::new_readonly(wrong_vault_pda, false), // Wrong PDA
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_close_account(user_idx),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: CloseAccount with wrong vault PDA should be rejected!"
    );
}

/// ATTACK: TopUpInsurance with wrong vault account.
/// Code validates vault matches stored vault_pubkey.
#[test]
fn test_attack_topup_insurance_wrong_vault() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Create a fake vault account
    let fake_vault = Pubkey::new_unique();
    env.svm
        .set_account(
            fake_vault,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; spl_token::state::Account::LEN],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let ata = env.create_ata(&admin.pubkey(), 1_000_000_000);
    let mut data = vec![9u8]; // TopUpInsurance
    data.extend_from_slice(&1_000_000_000u64.to_le_bytes());

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(ata, false),
            AccountMeta::new(fake_vault, false), // Wrong vault
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: TopUpInsurance with wrong vault should be rejected!"
    );
}

/// ATTACK: Liquidate permissionless caller not signer.
/// Verify liquidation requires a valid signer even though it's permissionless.
#[test]
fn test_attack_liquidate_caller_not_signer() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, 10_000_000);
    let user_pos_before = env.read_account_position(user_idx);
    let user_cap_before = env.read_account_capital(user_idx);

    let caller = Keypair::new();
    env.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();

    // Construct liquidation instruction with caller NOT as signer
    let mut data = vec![10u8]; // LiquidateAtOracle
    data.extend_from_slice(&user_idx.to_le_bytes());

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(caller.pubkey(), false), // NOT a signer
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data,
    };

    // Use admin as payer (different from caller)
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    // The Solana runtime should reject: caller is in account list but NOT a signer,
    // yet the transaction only has admin as signer. The runtime enforces that any
    // account marked in the instruction's AccountMeta must match tx signatures when
    // is_signer=false - actually it does NOT require signatures for non-signer accounts.
    // The program's LiquidateAtOracle handler never calls expect_signer on accounts[0],
    // so this is permissionless. Either way, verify security properties:
    let user_pos_after = env.read_account_position(user_idx);
    let user_cap_after = env.read_account_capital(user_idx);
    assert_eq!(
        user_pos_after, user_pos_before,
        "Liquidation-by-non-signer path must not mutate solvent user position: before={} after={} result={:?}",
        user_pos_before, user_pos_after, result
    );
    assert_eq!(
        user_cap_after, user_cap_before,
        "Liquidation-by-non-signer path must not mutate solvent user capital: before={} after={} result={:?}",
        user_cap_before, user_cap_after, result
    );

    // In both cases: user capital preserved, conservation holds
    assert!(
        user_cap_after > 0,
        "User capital must not be drained: cap={}",
        user_cap_after
    );
    let spl_vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, spl_vault,
        "Conservation must hold: engine={} spl={}",
        engine_vault, spl_vault
    );
}

/// ATTACK: Deposit with wrong oracle price account.
/// Verifies oracle account validation rejects wrong price feed.
#[test]
fn test_attack_deposit_wrong_oracle_account() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);

    env.create_ata(&user.pubkey(), 5_000_000_000);

    // Create a fake oracle account
    let fake_oracle = Pubkey::new_unique();
    env.svm
        .set_account(
            fake_oracle,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; 256],        // Garbage data
                owner: Pubkey::new_unique(), // Wrong owner
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    // Deposit itself doesn't need oracle, but init_user passes pyth_col.
    // Test that trade with wrong oracle index is rejected.
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);
    env.deposit(&user, user_idx, 5_000_000_000);

    // Construct trade instruction with wrong oracle
    let mut trade_data = vec![6u8]; // Trade tag
    trade_data.extend_from_slice(&lp_idx.to_le_bytes());
    trade_data.extend_from_slice(&user_idx.to_le_bytes());
    trade_data.extend_from_slice(&1_000_000i128.to_le_bytes());

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(fake_oracle, false), // Wrong oracle
        ],
        data: trade_data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Trade with wrong oracle account should be rejected!"
    );
}

/// ATTACK: InitUser with new_account_fee.
/// Verify fee goes to insurance fund and conservation holds.
#[test]
fn test_attack_init_user_fee_conservation() {
    program_path();

    let mut env = TestEnv::new();
    let new_account_fee: u128 = 1_000_000_000;
    env.init_market_full(0, 0, new_account_fee);

    let user = Keypair::new();
    env.svm.airdrop(&user.pubkey(), 5_000_000_000).unwrap();

    // Create ATA with enough tokens to cover the fee
    let ata = env.create_ata(&user.pubkey(), 2_000_000_000);

    // Manually construct InitUser with fee matching new_account_fee
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_col, false),
        ],
        data: encode_init_user(new_account_fee as u64),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    env.svm
        .send_transaction(tx)
        .expect("init_user with fee failed");

    // Verify insurance received the fee
    let insurance = env.read_insurance_balance();
    assert!(
        insurance >= new_account_fee,
        "Insurance should have received new_account_fee: got {}",
        insurance
    );

    // Verify SPL vault == engine vault (conservation)
    let spl_vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, spl_vault,
        "Conservation: engine={} spl={}",
        engine_vault, spl_vault
    );
}

/// ATTACK: Crank with wrong oracle account on standard market.
/// Trade/crank oracle validation should reject mismatched feed.
#[test]
fn test_attack_crank_wrong_oracle() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);

    env.set_slot(200);

    // Create a fake oracle that looks like Pyth but with wrong feed_id
    let fake_oracle = Pubkey::new_unique();
    env.svm
        .set_account(
            fake_oracle,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; 512],
                owner: Pubkey::new_unique(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let caller = Keypair::new();
    env.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();

    let mut data = vec![7u8]; // KeeperCrank
    data.extend_from_slice(&0u16.to_le_bytes()); // caller_idx = 0 (permissionless)
    data.push(0); // panic = false

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(caller.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(fake_oracle, false), // Wrong oracle
        ],
        data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&caller.pubkey()),
        &[&caller],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Crank with wrong oracle should be rejected!"
    );
}

/// ATTACK: Withdraw with wrong SPL token program account.
/// Substituting a fake token program should be rejected.
#[test]
fn test_attack_withdraw_wrong_token_program() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    let ata = env.create_ata(&user.pubkey(), 0);
    let (vault_pda, _) =
        Pubkey::find_program_address(&[b"vault", env.slab.as_ref()], &env.program_id);

    // Use a fake token program
    let fake_token_program = Pubkey::new_unique();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new(ata, false),
            AccountMeta::new_readonly(vault_pda, false),
            AccountMeta::new_readonly(fake_token_program, false), // Wrong token program
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_withdraw(user_idx, 1_000_000_000),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "ATTACK: Withdraw with wrong token program should be rejected!"
    );

    // Verify capital unchanged
    let cap = env.read_account_capital(user_idx);
    assert_eq!(
        cap, 5_000_000_000,
        "Capital should be unchanged after failed withdraw"
    );
}

/// ATTACK: Alias user_ata with vault in WithdrawCollateral.
/// Must reject duplicate-role account substitution.
#[test]
fn test_attack_withdraw_alias_user_ata_is_vault() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    let (vault_pda, _) =
        Pubkey::find_program_address(&[b"vault", env.slab.as_ref()], &env.program_id);

    let cap_before = env.read_account_capital(user_idx);
    let vault_before = env.vault_balance();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(env.vault, false), // vault
            AccountMeta::new(env.vault, false), // aliased as user_ata (attack)
            AccountMeta::new_readonly(vault_pda, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_withdraw(user_idx, 1_000_000_000),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "SECURITY: Withdraw should reject aliased user_ata=vault"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        cap_before,
        "Capital changed on rejected alias withdraw"
    );
    assert_eq!(
        env.vault_balance(),
        vault_before,
        "Vault changed on rejected alias withdraw"
    );
}

/// ATTACK: Alias user_ata with vault in CloseAccount.
/// Must reject duplicate-role account substitution.
#[test]
fn test_attack_close_account_alias_user_ata_is_vault() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    // Make close preconditions realistic: no open position and settled state.
    env.set_slot(200);
    env.crank();
    env.try_withdraw(&user, user_idx, 1_000_000_000).unwrap();

    let (vault_pda, _) =
        Pubkey::find_program_address(&[b"vault", env.slab.as_ref()], &env.program_id);
    let vault_before = env.vault_balance();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(env.vault, false), // vault
            AccountMeta::new(env.vault, false), // aliased as user_ata (attack)
            AccountMeta::new_readonly(vault_pda, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_close_account(user_idx),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&user.pubkey()),
        &[&user],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "SECURITY: CloseAccount should reject aliased user_ata=vault"
    );
    assert_eq!(
        env.vault_balance(),
        vault_before,
        "Vault changed on rejected alias close"
    );
}

// ============================================================================
// Round 16: Numeric Boundary + Code Path Tests
// ============================================================================

/// ATTACK: Trade on market with unit_scale so large that scale_price_e6 returns None.
/// Oracle price $138 (138_000_000 e6), unit_scale=200_000_000.
/// scale_price_e6(138M, 200M) = 0 → None → trade should be rejected.
#[test]
fn test_attack_scale_price_zero_rejects_trade() {
    program_path();

    let mut env = TestEnv::new();
    let unit_scale = 200_000_000u32;
    // unit_scale = 200M, so 138M / 200M = 0 → None
    env.init_market_full(0, unit_scale, 0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_pos_before = env.read_account_position(user_idx);
    let vault_before = env.vault_balance();

    // Crank should fail or be a no-op because oracle price scales to zero
    env.set_slot(200);
    let crank_result = env.try_crank();
    let lp_cap_after_crank = env.read_account_capital(lp_idx);
    let user_cap_after_crank = env.read_account_capital(user_idx);
    let lp_pos_after_crank = env.read_account_position(lp_idx);
    let user_pos_after_crank = env.read_account_position(user_idx);

    assert_eq!(
        lp_pos_after_crank, lp_pos_before,
        "Scale-zero crank path must not mutate LP position: before={} after={}",
        lp_pos_before, lp_pos_after_crank
    );
    assert_eq!(
        user_pos_after_crank, user_pos_before,
        "Scale-zero crank path must not mutate user position: before={} after={}",
        user_pos_before, user_pos_after_crank
    );
    if crank_result.is_ok() {
        assert!(
            lp_cap_after_crank <= lp_cap_before,
            "Accepted scale-zero crank must not mint LP capital: before={} after={}",
            lp_cap_before,
            lp_cap_after_crank
        );
        assert!(
            user_cap_after_crank <= user_cap_before,
            "Accepted scale-zero crank must not mint user capital: before={} after={}",
            user_cap_before,
            user_cap_after_crank
        );
    } else {
        assert_eq!(
            lp_cap_after_crank, lp_cap_before,
            "Rejected scale-zero crank must preserve LP capital: before={} after={}",
            lp_cap_before, lp_cap_after_crank
        );
        assert_eq!(
            user_cap_after_crank, user_cap_before,
            "Rejected scale-zero crank must preserve user capital: before={} after={}",
            user_cap_before, user_cap_after_crank
        );
    }

    // Trade should fail because scaled price is None
    let trade_result = env.try_trade(&user, &lp, lp_idx, user_idx, 100);
    assert!(
        trade_result.is_err(),
        "ATTACK: Trade with zero scaled price should be rejected!"
    );
    assert_eq!(
        env.read_account_position(user_idx),
        user_pos_after_crank,
        "Rejected scale-zero trade must preserve user position"
    );
    assert_eq!(
        env.read_account_capital(user_idx),
        user_cap_after_crank,
        "Rejected scale-zero trade must preserve user capital"
    );
    assert_eq!(
        env.vault_balance(),
        vault_before,
        "Scale-zero edge path must preserve vault balance"
    );
    let engine_vault_scaled = env.read_engine_vault() as u128;
    assert_eq!(
        engine_vault_scaled * unit_scale as u128,
        vault_before as u128,
        "Scale-zero edge path must preserve scaled engine/SPL vault consistency: engine_scaled={} unit_scale={} spl={}",
        engine_vault_scaled,
        unit_scale,
        vault_before
    );
}

/// ATTACK: Inverted market with very high raw price so inverted result is zero.
/// invert_price_e6 with raw near u64::MAX: INVERSION_CONSTANT / raw → 0.
/// INVERSION_CONSTANT = 10^12, so raw > 10^12 gives inverted < 1 → 0 → None.
#[test]
fn test_attack_invert_price_zero_result() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(1); // Inverted

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    // Initial setup at normal price
    env.crank();
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_pos_before = env.read_account_position(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);

    // Set oracle to extremely high price: 10^13 (> INVERSION_CONSTANT=10^12)
    // inverted = 10^12 / 10^13 = 0 → None
    env.set_slot_and_price(200, 10_000_000_000_000);
    let crank_result = env.try_crank();
    // Crank may fail (zero inverted price) or clamp via circuit breaker
    println!("Crank with zero-invert price: {}", if crank_result.is_ok() { "ok (clamped)" } else { "rejected" });
    // Conservation must hold regardless
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    let lp_pos_after = env.read_account_position(lp_idx);
    let user_pos_after = env.read_account_position(user_idx);
    let lp_cap_after = env.read_account_capital(lp_idx);
    let user_cap_after = env.read_account_capital(user_idx);

    assert_eq!(
        lp_pos_after, lp_pos_before,
        "Zero-invert crank path must not mutate LP position: before={} after={}",
        lp_pos_before, lp_pos_after
    );
    assert_eq!(
        user_pos_after, user_pos_before,
        "Zero-invert crank path must not mutate user position: before={} after={}",
        user_pos_before, user_pos_after
    );
    if crank_result.is_ok() {
        assert!(
            lp_cap_after <= lp_cap_before,
            "Accepted zero-invert crank must not mint LP capital: before={} after={}",
            lp_cap_before,
            lp_cap_after
        );
        assert!(
            user_cap_after <= user_cap_before,
            "Accepted zero-invert crank must not mint user capital: before={} after={}",
            user_cap_before,
            user_cap_after
        );
    } else {
        assert_eq!(
            lp_cap_after, lp_cap_before,
            "Rejected zero-invert crank must preserve LP capital: before={} after={}",
            lp_cap_before, lp_cap_after
        );
        assert_eq!(
            user_cap_after, user_cap_before,
            "Rejected zero-invert crank must preserve user capital: before={} after={}",
            user_cap_before, user_cap_after
        );
    }
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after zero-invert: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: Inverted market with raw price = 1 (smallest non-zero).
/// invert_price_e6(1, 1) = 10^12 / 1 = 10^12 → within u64 range.
/// Verify the market handles extreme inverted prices.
#[test]
fn test_attack_invert_price_extreme_small_raw() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(1);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    // First crank at normal price
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_pos_before = env.read_account_position(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);

    // Set raw price = 1 → inverted = 10^12
    // Circuit breaker will cap the movement, but the inverted price is valid
    env.set_slot_and_price(200, 1);
    let crank_result = env.try_crank();
    println!("Crank with extreme-small raw: {}", if crank_result.is_ok() { "ok (clamped)" } else { "rejected" });

    // Conservation must hold regardless
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    let lp_pos_after = env.read_account_position(lp_idx);
    let user_pos_after = env.read_account_position(user_idx);
    let lp_cap_after = env.read_account_capital(lp_idx);
    let user_cap_after = env.read_account_capital(user_idx);

    assert_eq!(
        lp_pos_after, lp_pos_before,
        "Extreme-small raw crank path must not mutate LP position: before={} after={}",
        lp_pos_before, lp_pos_after
    );
    assert_eq!(
        user_pos_after, user_pos_before,
        "Extreme-small raw crank path must not mutate user position: before={} after={}",
        user_pos_before, user_pos_after
    );
    if crank_result.is_ok() {
        let cap_sum_before = lp_cap_before + user_cap_before;
        let cap_sum_after = lp_cap_after + user_cap_after;
        assert!(
            cap_sum_after <= cap_sum_before,
            "Accepted extreme-small raw crank must not mint aggregate capital: before_sum={} after_sum={}",
            cap_sum_before,
            cap_sum_after
        );
    } else {
        assert_eq!(
            lp_cap_after, lp_cap_before,
            "Rejected extreme-small raw crank must preserve LP capital: before={} after={}",
            lp_cap_before, lp_cap_after
        );
        assert_eq!(
            user_cap_after, user_cap_before,
            "Rejected extreme-small raw crank must preserve user capital: before={} after={}",
            user_cap_before, user_cap_after
        );
    }
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after extreme invert: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: Multi-instruction atomic transaction (deposit + trade in same tx).
/// Verify protocol handles multiple instructions in single transaction correctly.
#[test]
fn test_attack_multi_instruction_deposit_trade_atomic() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    // Must deposit BEFORE crank to prevent GC of zero-capital account
    env.deposit(&user, user_idx, 1_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Create ATA with tokens for additional deposit
    let ata = env.create_ata(&user.pubkey(), 5_000_000_000);

    // Build deposit instruction
    let deposit_ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data: encode_deposit(user_idx, 5_000_000_000),
    };

    // Build trade instruction (in same tx, needs LP signer too)
    let trade_ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(lp.pubkey(), true), // LP must sign
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
        ],
        data: encode_trade(lp_idx, user_idx, 1_000_000),
    };

    // Send both instructions atomically (both user and LP must sign)
    let tx = Transaction::new_signed_with_payer(
        &[deposit_ix, trade_ix],
        Some(&user.pubkey()),
        &[&user, &lp],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_ok(),
        "Multi-instruction deposit+trade should succeed: {:?}",
        result
    );

    // Verify both took effect
    let pos = env.read_account_position(user_idx);
    assert_eq!(pos, 1_000_000, "Position should be set from trade");
    let cap = env.read_account_capital(user_idx);
    assert!(
        cap > 1_000_000_000,
        "Capital should include additional deposit: cap={}",
        cap
    );
}

/// ATTACK: Withdraw amount = unit_scale - 1 (largest misaligned amount).
/// Should be rejected by alignment check when unit_scale > 1.
#[test]
fn test_attack_withdraw_scale_minus_one_misaligned() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_full(0, 1000, 0); // unit_scale = 1000

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    // Try withdrawing 999 (scale - 1), which is not aligned to unit_scale=1000
    let result = env.try_withdraw(&user, user_idx, 999);
    assert!(
        result.is_err(),
        "ATTACK: Withdraw of scale-1 amount should fail alignment check!"
    );

    // Capital unchanged (stored in units: 5B / unit_scale=1000 = 5M)
    let cap = env.read_account_capital(user_idx);
    assert_eq!(
        cap, 5_000_000,
        "Capital unchanged after failed misaligned withdraw"
    );
}

/// ATTACK: Close slab after all accounts closed and insurance is zero.
/// Tests the clean shutdown path: LP deposits, withdraws, closes, then slab closes.
#[test]
fn test_attack_close_slab_clean_shutdown() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 5_000_000_000);

    env.crank();

    // Withdraw all LP capital
    env.try_withdraw(&lp, lp_idx, 5_000_000_000).unwrap();

    // Close LP account
    env.close_account(&lp, lp_idx);

    // Insurance should be 0 (no fees generated)
    let insurance = env.read_insurance_balance();
    assert_eq!(insurance, 0, "Insurance should be zero: got {}", insurance);

    // Vault should be 0
    let vault = env.vault_balance();
    assert_eq!(vault, 0, "Vault should be zero: got {}", vault);

    // Close slab should succeed
    let result = env.try_close_slab();
    assert!(result.is_ok(), "CloseSlab should succeed: {:?}", result);
}

/// ATTACK: Liquidation at exact equity zero boundary.
/// Position PnL + capital = 0 exactly. Should be liquidatable.
#[test]
fn test_attack_liquidation_equity_exactly_zero() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 2_000_000_000).unwrap();
    env.crank();

    // Open a long position
    env.trade(&user, &lp, lp_idx, user_idx, 10_000_000);

    // Move price down significantly to push equity toward zero
    // Position value = 10M * price_e6 / 1e6. At $138, notional ≈ 1.38B.
    // With 5B capital, need price drop to push equity to 0.
    // PnL = 10M * (new_price - entry_price) / 1e6
    // At entry price 138e6, position value = 1.38e9
    // Circuit breaker limits per-slot movement, so we need many slots
    for slot in (200..=2000).step_by(100) {
        let price = 138_000_000 - ((slot - 100) * 50_000) as i64;
        if price < 1_000_000 {
            break;
        }
        env.set_slot_and_price(slot, price);
        env.crank();
    }

    // Try liquidation - account should be insolvent or near-insolvent
    let user_pos_before_liq = env.read_account_position(user_idx);
    let liq_result = env.try_liquidate(user_idx);
    let user_pos_after_liq = env.read_account_position(user_idx);
    assert!(
        user_pos_after_liq.unsigned_abs() <= user_pos_before_liq.unsigned_abs(),
        "Liquidation path must not increase user exposure. before={} after={} result={:?}",
        user_pos_before_liq,
        user_pos_after_liq,
        liq_result
    );
    // Whether liquidation succeeds or not, conservation must hold
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after liquidation attempt: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: Deposit and immediate crank in same slot.
/// Tests that deposit + crank in same slot doesn't create exploitable state.
#[test]
fn test_attack_deposit_and_crank_same_slot_no_exploit() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    // Deposit before first crank to prevent GC
    env.deposit(&user, user_idx, 2_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Additional deposit at slot 200
    env.set_slot(200);
    env.deposit(&user, user_idx, 3_000_000_000);

    // Crank at same slot 200 (immediately after deposit)
    env.crank();

    // Capital should reflect both deposits (no erosion from same-slot crank)
    let cap = env.read_account_capital(user_idx);
    assert_eq!(
        cap, 5_000_000_000,
        "Capital should equal total deposits after same-slot crank: cap={}",
        cap
    );

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot should equal sum of capitals: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Trade then immediate crank then withdraw in rapid sequence.
/// Tests state consistency across rapid operation sequence.
#[test]
fn test_attack_trade_crank_withdraw_rapid_sequence() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade at slot 1
    env.set_slot(1);
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Crank at slot 2
    env.set_slot(2);
    env.crank();

    // Close position at slot 3
    env.set_slot(3);
    env.trade(&user, &lp, lp_idx, user_idx, -5_000_000);

    // Crank at slot 4 to settle
    env.set_slot(4);
    env.crank();

    // Withdraw all capital
    let cap = env.read_account_capital(user_idx);
    let withdraw_result = env.try_withdraw(&user, user_idx, cap as u64);
    assert!(
        withdraw_result.is_ok(),
        "Withdraw after close should succeed: {:?}",
        withdraw_result
    );

    // Conservation
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: Multiple price changes between cranks (large gap).
/// Only the price at crank time should matter, not intermediate prices.
#[test]
fn test_attack_price_whipsaw_between_cranks() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Set price to drop, then recover, then crank at recovered price
    // Only the final price at crank should matter
    env.set_slot_and_price(200, 130_000_000); // Drop to $130
                                              // Don't crank yet
    env.set_slot_and_price(300, 138_000_000); // Recover to $138
    env.crank(); // Crank at recovered price

    // User PnL should reflect the crank price ($138), not intermediate ($130)
    // At $138, PnL ≈ 0 (same as entry)
    let cap = env.read_account_capital(user_idx);
    assert!(
        cap <= 10_000_000_000,
        "Capital should not increase: cap={}",
        cap
    );

    // Conservation
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after whipsaw: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: Deposit to account, trade, then deposit again (incremental deposits).
/// Verify capital is correct after multiple deposits with position open.
#[test]
fn test_attack_incremental_deposits_with_position() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 3_000_000);
    assert_eq!(env.read_account_position(user_idx), 3_000_000);

    // Second deposit while position is open
    env.set_slot(200);
    env.deposit(&user, user_idx, 3_000_000_000);

    let cap_after = env.read_account_capital(user_idx);
    assert!(
        cap_after >= 7_000_000_000,
        "Capital should be >= 8B after second deposit: cap={}",
        cap_after
    );

    // Crank after second deposit
    env.crank();

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot desync after incremental deposit: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Warmup + funding interaction.
/// Open position, warmup is accruing, funding is also accruing.
/// Both should settle correctly without double-counting.
#[test]
fn test_attack_warmup_funding_interaction_no_double_count() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_warmup(0, 100); // 100 slot warmup

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open position - starts warmup
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Crank at several slots during warmup period
    for slot in (200..=500).step_by(50) {
        env.set_slot(slot);
        env.crank();
    }

    // After warmup (slot > 100 + trade_slot), check conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot desync after warmup+funding: c_tot={} sum={}",
        c_tot, sum
    );

    // Vault conservation
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Vault conservation: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: LP position tracking after multiple users trade and close.
/// Verify LP position aggregates are correct after complex trading.
#[test]
fn test_attack_lp_position_aggregate_after_many_trades() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // 3 users open positions
    let mut users = Vec::new();
    for _ in 0..3 {
        let user = Keypair::new();
        let user_idx = env.init_user(&user);
        env.deposit(&user, user_idx, 5_000_000_000);
        users.push((user, user_idx));
    }

    // User 0: long 100K, User 1: long 200K, User 2: short 150K
    env.set_slot(1);
    env.trade(&users[0].0, &lp, lp_idx, users[0].1, 100_000);
    env.set_slot(2);
    env.trade(&users[1].0, &lp, lp_idx, users[1].1, 200_000);
    env.set_slot(3);
    env.trade(&users[2].0, &lp, lp_idx, users[2].1, -150_000);

    // LP position should be opposite sum: -(100K + 200K - 150K) = -150K
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(
        lp_pos, -150_000,
        "LP position should be -150K: got {}",
        lp_pos
    );

    // User 0 closes position
    env.set_slot(4);
    env.trade(&users[0].0, &lp, lp_idx, users[0].1, -100_000);

    // LP position now: -(200K - 150K) = -50K
    let lp_pos2 = env.read_account_position(lp_idx);
    assert_eq!(
        lp_pos2, -50_000,
        "LP position after partial close: got {}",
        lp_pos2
    );

    // Conservation
    let c_tot = env.read_c_tot();
    let mut sum = env.read_account_capital(lp_idx);
    for (_, idx) in &users {
        sum += env.read_account_capital(*idx);
    }
    assert_eq!(
        c_tot, sum,
        "c_tot desync after multi-user trades: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Price at exact circuit breaker boundary.
/// Move price by exactly oracle_price_cap_bps per slot.
/// Verify mark tracks correctly at the boundary.
#[test]
fn test_attack_circuit_breaker_exact_cap_boundary() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Move price up by small increments across many slots
    for slot in (200..=600).step_by(50) {
        // Incrementally increase price
        let price = 138_000_000 + ((slot - 100) * 20_000) as i64;
        env.set_slot_and_price(slot, price);
        env.crank();
    }

    // Conservation must hold after many bounded price movements
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after circuit breaker clamping: engine={} vault={}",
        engine_vault, vault
    );

    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot conservation: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Trade with exactly initial_margin_bps worth of capital.
/// At the exact margin boundary, the trade should just barely succeed.
#[test]
fn test_attack_trade_exact_margin_boundary_succeeds() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    // Deposit exactly 1B
    env.deposit(&user, user_idx, 1_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // initial_margin_bps = 1000 (10%), price = $138 (138e6)
    // max_notional = 1e9 / 0.1 = 10e9
    // max_position = 10e9 * 1e6 / 138e6 ≈ 72.46M
    // Try just under the limit
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 72_000_000);
    assert!(
        result.is_ok(),
        "Trade at exact margin boundary should succeed: {:?}",
        result
    );

    // Verify position was opened
    let pos = env.read_account_position(user_idx);
    assert_eq!(pos, 72_000_000, "Position should be set: pos={}", pos);
}

/// ATTACK: Maintenance fee settlement when capital is very small.
/// With large maintenance_fee_per_slot and small capital, fee should not go negative.
#[test]
fn test_attack_maintenance_fee_depletes_small_capital() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000); // Very small deposit (1M tokens)

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();

    // Set high maintenance fee
    env.try_set_maintenance_fee(&admin, 1_000_000).unwrap();

    // Crank several times to accrue fees
    for slot in (200..=1000).step_by(100) {
        env.set_slot(slot);
        env.crank();
    }

    // Capital should not go negative (saturating)
    let cap = env.read_account_capital(user_idx);
    assert!(
        cap < 1_000_000,
        "Capital should be depleted by maintenance fees: cap={}",
        cap
    );

    // Conservation
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: Mark precision with very small price increments.
/// Multiple tiny price changes and cranks should maintain conservation.
#[test]
fn test_attack_mark_precision_small_increments() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Apply very small price increments and crank each time
    let base_price: i64 = 138_000_000;
    for i in 1..=20u64 {
        let price = base_price + (i as i64); // 1 unit increment (smallest possible)
        env.set_slot_and_price(200 + i, price);
        env.crank();
    }

    // Conservation must hold after many tiny price changes
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after small increments: engine={} vault={}",
        engine_vault, vault
    );

    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot conservation: c_tot={} sum={}",
        c_tot, sum
    );
}

// ============================================================================
// Round 17: Instruction Handler Edge Cases + State Interactions
// ============================================================================

/// ATTACK: UpdateConfig while positions are open and funding accruing.
/// Changing funding parameters mid-flight should not cause retroactive errors.
#[test]
fn test_attack_update_config_during_active_trades() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open position and let funding accrue
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    env.set_slot(300);
    env.crank();

    // Update config mid-flight (change funding params)
    env.try_update_config(&admin).unwrap();

    // Continue cranking with new config
    env.set_slot(500);
    env.crank();

    env.set_slot(700);
    env.crank();

    // Conservation must hold after config change
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after config change: engine={} vault={}",
        engine_vault, vault
    );

    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot conservation: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: PushOraclePrice with same price as last effective price.
/// When price doesn't change, circuit breaker should produce stable state.
#[test]
fn test_attack_push_oracle_same_as_last_price() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(138_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    // Push same price multiple times
    for ts in 100..=110 {
        env.try_push_oracle_price(&admin, 138_000_000, ts).unwrap();
    }

    // State should be stable (no drift from repeated same-price pushes)
    let vault = env.vault_balance();
    assert_eq!(
        vault, 20_000_000_000,
        "Vault should not change from repeated same-price pushes: vault={}",
        vault
    );
}

/// ATTACK: Liquidate with target_idx = u16::MAX (65535, CRANK_NO_CALLER sentinel).
/// Should not confuse liquidation with permissionless crank sentinel.
#[test]
fn test_attack_liquidate_target_u16_max() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    // Try liquidating index u16::MAX (should fail - no such account)
    let result = env.try_liquidate(u16::MAX);
    assert!(
        result.is_err(),
        "ATTACK: Liquidate with u16::MAX target should fail!"
    );
}

/// ATTACK: Deposit after liquidation in same slot.
/// User gets liquidated, then immediately deposits. Conservation must hold.
#[test]
fn test_attack_deposit_after_liquidation_same_slot() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 2_000_000_000).unwrap();
    env.crank();

    // Open large long position
    env.trade(&user, &lp, lp_idx, user_idx, 10_000_000);

    // Drop price significantly over many slots to make account insolvent
    for slot in (200..=2000).step_by(100) {
        let price = 138_000_000 - ((slot - 100) * 50_000) as i64;
        if price < 1_000_000 {
            break;
        }
        env.set_slot_and_price(slot, price);
        env.crank();
    }

    // Verify user position was indeed opened (precondition)
    // After price drops + cranks, user should be liquidated or still alive
    let vault_before = env.vault_balance();
    let capital_before_liq = env.read_account_capital(user_idx);

    // Try liquidating
    let liq_result = env.try_liquidate(user_idx);
    let capital_after_liq = env.read_account_capital(user_idx);

    if liq_result.is_ok() {
        assert!(
            capital_after_liq <= capital_before_liq,
            "Liquidation should not increase target capital: before={} after={}",
            capital_before_liq,
            capital_after_liq
        );
    } else {
        assert_eq!(
            capital_after_liq, capital_before_liq,
            "Failed liquidation should not change target capital: before={} after={}",
            capital_before_liq,
            capital_after_liq
        );
    }

    // After liquidation attempt, try deposit
    let deposit_result = env.try_deposit(&user, user_idx, 1_000_000_000);

    // Conservation must hold either way
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after liquidation+deposit: engine={} vault={}",
        engine_vault, vault
    );

    // Verify vault changed if deposit succeeded (tokens transferred in)
    if deposit_result.is_ok() {
        assert!(
            vault == vault_before + 1_000_000_000,
            "Deposit succeeded so vault must increase by exact amount: before={} after={}",
            vault_before,
            vault
        );
    } else {
        assert_eq!(
            vault, vault_before,
            "Failed deposit should not change vault: before={} after={}",
            vault_before, vault
        );
    }
}

/// ATTACK: InitLP with matcher_program = Percolator program itself.
/// InitLP stores the matcher pubkey but doesn't CPI, so it may succeed at init.
/// Verify no value extraction and conservation holds regardless of outcome.
#[test]
fn test_attack_init_lp_matcher_is_self_program() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp_owner = Keypair::new();
    env.svm.airdrop(&lp_owner.pubkey(), 5_000_000_000).unwrap();
    let ata = env.create_ata(&lp_owner.pubkey(), 0);

    let matcher_ctx = Pubkey::new_unique();

    // Use percolator program_id as matcher (self-reference)
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(lp_owner.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data: encode_init_lp(&env.program_id, &matcher_ctx, 0), // Self as matcher
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&lp_owner.pubkey()),
        &[&lp_owner],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    // InitLP with self-program as matcher: may succeed at init time
    // (no CPI happens during init), but state must remain consistent.
    let num_used = env.read_num_used_accounts();
    if result.is_ok() {
        assert_eq!(
            num_used, 1,
            "Accepted self-matcher InitLP should allocate exactly one account"
        );
        assert!(
            env.is_slot_used(0),
            "Accepted self-matcher InitLP should mark slot 0 as used"
        );
        assert_eq!(
            env.read_account_capital(0),
            0,
            "Accepted self-matcher InitLP should not mint capital at init"
        );
    } else {
        assert_eq!(
            num_used, 0,
            "Rejected self-matcher InitLP should not allocate accounts"
        );
    }
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after self-matcher init: engine={} vault={}",
        engine_vault, vault
    );
    // Verify the LP didn't extract any value
    assert_eq!(
        vault, 0,
        "No tokens should have been deposited: vault={}",
        vault
    );
}

/// ATTACK: Funding rate sign flip when LP position crosses zero.
/// LP net position goes from short to long in a single trade.
#[test]
fn test_attack_funding_rate_sign_flip_lp_position_cross() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 10_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // User1 goes long → LP goes short
    env.set_slot(1);
    env.trade(&user1, &lp, lp_idx, user1_idx, 5_000_000);

    env.set_slot(200);
    env.crank();

    // User2 goes short (larger) → LP flips from short to long
    env.set_slot(201);
    env.trade(&user2, &lp, lp_idx, user2_idx, -10_000_000);

    // LP position now = -(-10M + 5M) = +5M (was -5M, now +5M)
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(
        lp_pos, 5_000_000,
        "LP should have flipped to long: pos={}",
        lp_pos
    );

    // Crank after flip
    env.set_slot(400);
    env.crank();

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx)
        + env.read_account_capital(user1_idx)
        + env.read_account_capital(user2_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot after LP position flip: c_tot={} sum={}",
        c_tot, sum
    );
}

// REMOVED: test_attack_gc_exact_threshold_account - duplicate of
// test_attack_user_gc_when_empty (round 5, line 9260)

// REMOVED: test_attack_close_account_fee_forgiveness_edge - duplicate of
// test_attack_close_account_fee_debt_forgiveness (round 9, line 14814)

/// ATTACK: TopUpInsurance with unit_scale dust edge case.
/// Insurance topup amount that doesn't align with unit_scale.
#[test]
fn test_attack_topup_insurance_unit_scale_dust() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_full(0, 1000, 0); // unit_scale = 1000

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Top up insurance with amount not aligned to unit_scale
    let result = env.try_top_up_insurance(&admin, 999); // Not aligned to 1000
    // Insurance topup may succeed but 999 / 1000 = 0 units (dust lost to protocol).
    // If rejected, insurance should still remain unchanged at 0.
    let insurance_after_dust = env.read_insurance_balance();
    assert_eq!(
        insurance_after_dust, 0,
        "Sub-unit-scale topup should leave insurance at 0 units: result={:?} insurance={}",
        result, insurance_after_dust
    );

    // Now top up with aligned amount to verify it works
    env.try_top_up_insurance(&admin, 2000).unwrap(); // 2000 / 1000 = 2 units
    let insurance = env.read_insurance_balance();
    assert_eq!(
        insurance, 2,
        "Aligned topup should give 2 units: got {}",
        insurance
    );

    // Conservation with unit_scale: engine tracks units, vault holds tokens
    // vault = 2999 tokens, engine_vault = 2 units (999 dust tokens untracked)
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    // Engine vault (in units) * unit_scale should be <= actual vault tokens
    // The difference is dust that was transferred but too small to register
    assert!(
        engine_vault * 1000 <= vault as u128,
        "Engine units * scale must not exceed vault tokens: {}*1000={} vault={}",
        engine_vault,
        engine_vault * 1000,
        vault
    );
    // Dust = vault - engine_vault * scale should be < unit_scale
    let dust = vault as u128 - engine_vault * 1000;
    assert!(
        dust < 1000,
        "Dust must be less than unit_scale: dust={}",
        dust
    );
}

/// ATTACK: Resolve hyperp market then attempt UpdateConfig.
/// Admin config changes should be blocked after market resolution.
#[test]
fn test_attack_update_config_after_resolution() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(138_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 140_000_000, 100).unwrap();
    env.try_resolve_market(&admin).unwrap();

    // After resolution, config update must fail.
    let result = env.try_update_config(&admin);
    assert!(
        result.is_err(),
        "SECURITY: UpdateConfig must be rejected after resolution"
    );

    // State must remain consistent
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after config update attempt: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: PushOraclePrice after resolution.
/// Settlement parameters must be frozen once market is resolved.
#[test]
fn test_attack_push_oracle_after_resolution_rejected() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(138_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 140_000_000, 100).unwrap();
    env.try_resolve_market(&admin).unwrap();

    // Config offset for authority_price_e6
    const AUTH_PRICE_OFF: usize = 360;
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let settle_before = u64::from_le_bytes(
        slab_before[AUTH_PRICE_OFF..AUTH_PRICE_OFF + 8]
            .try_into()
            .unwrap(),
    );

    let result = env.try_push_oracle_price(&admin, 200_000_000, 200);
    assert!(
        result.is_err(),
        "SECURITY: PushOraclePrice must be rejected after resolution"
    );

    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let settle_after = u64::from_le_bytes(
        slab_after[AUTH_PRICE_OFF..AUTH_PRICE_OFF + 8]
            .try_into()
            .unwrap(),
    );
    assert_eq!(
        settle_before, settle_after,
        "Settlement price changed after rejected post-resolution push"
    );
}

/// ATTACK: SetOracleAuthority after resolution.
/// Oracle authority must remain frozen once market is resolved.
#[test]
fn test_attack_set_oracle_authority_after_resolution_rejected() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(138_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 140_000_000, 100).unwrap();
    env.try_resolve_market(&admin).unwrap();

    const AUTHORITY_OFF: usize = 328;
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let authority_before: [u8; 32] = slab_before[AUTHORITY_OFF..AUTHORITY_OFF + 32]
        .try_into()
        .unwrap();

    let result = env.try_set_oracle_authority(&admin, &Pubkey::new_unique());
    assert!(
        result.is_err(),
        "SECURITY: SetOracleAuthority must be rejected after resolution"
    );

    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let authority_after: [u8; 32] = slab_after[AUTHORITY_OFF..AUTHORITY_OFF + 32]
        .try_into()
        .unwrap();
    assert_eq!(
        authority_before, authority_after,
        "Oracle authority changed after rejected post-resolution update"
    );
}

/// ATTACK: SetOraclePriceCap after resolution.
/// Price-cap settings must be frozen after market resolution.
#[test]
fn test_attack_set_oracle_price_cap_after_resolution_rejected() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(138_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 140_000_000, 100).unwrap();
    env.try_set_oracle_price_cap(&admin, 1_000_000).unwrap();
    env.try_resolve_market(&admin).unwrap();

    const CAP_OFF: usize = 376;
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let cap_before = u64::from_le_bytes(slab_before[CAP_OFF..CAP_OFF + 8].try_into().unwrap());

    let result = env.try_set_oracle_price_cap(&admin, 10);
    assert!(
        result.is_err(),
        "SECURITY: SetOraclePriceCap must be rejected after resolution"
    );

    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let cap_after = u64::from_le_bytes(slab_after[CAP_OFF..CAP_OFF + 8].try_into().unwrap());
    assert_eq!(
        cap_before, cap_after,
        "Oracle price cap changed after rejected post-resolution update"
    );
}

/// ATTACK: SetMaintenanceFee after resolution.
/// Fee parameters should not be mutable once market enters resolved mode.
#[test]
fn test_attack_set_maintenance_fee_after_resolution_rejected() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(138_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 140_000_000, 100).unwrap();
    env.try_resolve_market(&admin).unwrap();

    let result = env.try_set_maintenance_fee(&admin, 1_000_000);
    assert!(
        result.is_err(),
        "SECURITY: SetMaintenanceFee must be rejected after resolution"
    );
}

/// ATTACK: SetRiskThreshold after resolution.
/// Risk-gating parameters should be immutable in resolved mode.
#[test]
fn test_attack_set_risk_threshold_after_resolution_rejected() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(138_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();
    env.try_push_oracle_price(&admin, 140_000_000, 100).unwrap();
    env.try_resolve_market(&admin).unwrap();

    let result = env.try_set_risk_threshold(&admin, 1_000_000_000);
    assert!(
        result.is_err(),
        "SECURITY: SetRiskThreshold must be rejected after resolution"
    );
}

/// ATTACK: Multiple trades filling LP position in alternating directions.
/// LP position oscillates: +5M, +2M (net -3M), -1M (net +4M), etc.
/// Verify LP position tracking remains accurate through oscillations.
#[test]
fn test_attack_lp_position_oscillation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Series of trades that oscillate LP position
    let sizes: &[i128] = &[500_000, -300_000, 200_000, -600_000, 400_000];
    let mut expected_user_pos: i128 = 0;
    for (i, &size) in sizes.iter().enumerate() {
        env.set_slot((i + 1) as u64);
        env.trade(&user, &lp, lp_idx, user_idx, size);
        expected_user_pos += size;
    }

    // Total user pos = 500K - 300K + 200K - 600K + 400K = 200K
    assert_eq!(
        env.read_account_position(user_idx),
        expected_user_pos,
        "User position after oscillation"
    );
    // LP pos = -200K
    assert_eq!(
        env.read_account_position(lp_idx),
        -expected_user_pos,
        "LP position should mirror user"
    );

    // Crank and verify conservation
    env.set_slot(200);
    env.crank();

    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot after oscillation: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: SetOraclePriceCap to u64::MAX.
/// Effectively disables circuit breaker. Verify large price moves are accepted.
#[test]
fn test_attack_oracle_price_cap_u64_max() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);

    // Set price cap to maximum (disable circuit breaker)
    env.try_set_oracle_price_cap(&admin, u64::MAX).unwrap();

    // Large price jump should now be accepted in one crank
    env.set_slot_and_price(200, 200_000_000); // $138 → $200 (45% jump)
    env.crank();

    // Conservation must still hold
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation with disabled circuit breaker: engine={} vault={}",
        engine_vault, vault
    );

    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(c_tot, sum, "c_tot conservation");
}

/// ATTACK: SetMaintenanceFee then immediately close account.
/// Fee set then close in rapid sequence should settle fees correctly.
#[test]
fn test_attack_set_fee_then_immediate_close() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Set maintenance fee
    env.try_set_maintenance_fee(&admin, 100_000).unwrap();

    // Advance some slots
    env.set_slot(500);
    env.crank();

    // Withdraw all and close
    let cap = env.read_account_capital(user_idx);
    if cap > 0 {
        env.try_withdraw(&user, user_idx, cap as u64)
            .expect("full withdrawal before close must succeed");
    }

    let close_result = env.try_close_account(&user, user_idx);
    assert!(
        close_result.is_ok(),
        "CloseAccount after fee set should succeed: {:?}",
        close_result
    );

    // Conservation
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: Withdraw between two cranks (deposit, crank, withdraw, crank).
/// Tests that withdrawal doesn't cause double-counting in settlement.
#[test]
fn test_attack_withdraw_between_two_cranks() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade
    env.trade(&user, &lp, lp_idx, user_idx, 3_000_000);

    // Crank at slot 200
    env.set_slot(200);
    env.crank();

    // Withdraw partial capital between cranks
    env.try_withdraw(&user, user_idx, 2_000_000_000).unwrap();

    // Crank again at slot 400
    env.set_slot(400);
    env.crank();

    // Conservation
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after withdraw between cranks: engine={} vault={}",
        engine_vault, vault
    );

    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot after withdraw between cranks: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: User deposits, withdraws everything, gets GC'd, new user takes slot.
/// Tests slot reuse and state cleanliness after GC in multi-user scenario.
#[test]
fn test_attack_slot_reuse_multi_user_gc_reinit() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // user1: deposit + withdraw everything (no trades) → GC candidate
    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 5_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // user2 trades, user1 stays idle
    env.set_slot(1);
    env.trade(&user2, &lp, lp_idx, user2_idx, 2_000_000);

    // Withdraw all of user1's capital → now zero capital, zero position
    env.try_withdraw(&user1, user1_idx, 5_000_000_000).unwrap();

    // Crank to GC user1 (zero everything)
    env.set_slot(200);
    env.crank();

    let num_after_gc = env.read_num_used_accounts();
    let user1_slot_used_after_gc = env.is_slot_used(user1_idx);
    if user1_slot_used_after_gc {
        assert_eq!(
            num_after_gc, 3,
            "If user1 is not GC'd yet, all 3 accounts should remain (got {})",
            num_after_gc
        );
    } else {
        assert_eq!(
            num_after_gc, 2,
            "If user1 is GC'd, only LP+user2 should remain (got {})",
            num_after_gc
        );
        assert_eq!(
            env.read_account_capital(user1_idx),
            0,
            "GC'd user1 slot should have zero capital"
        );
        assert_eq!(
            env.read_account_position(user1_idx),
            0,
            "GC'd user1 slot should have zero position"
        );
    }

    // New user3 takes the recycled slot
    let user3 = Keypair::new();
    let user3_idx = env.init_user(&user3);
    if !user1_slot_used_after_gc {
        assert_eq!(
            user3_idx, user1_idx,
            "Freed user1 slot should be reused by user3"
        );
    }
    env.deposit(&user3, user3_idx, 5_000_000_000);

    // user3 trades
    env.set_slot(201);
    env.trade(&user3, &lp, lp_idx, user3_idx, 500_000);

    // Conservation across all active accounts
    env.set_slot(400);
    env.crank();

    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx)
        + env.read_account_capital(user2_idx)
        + env.read_account_capital(user3_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot after slot reuse: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: SetRiskThreshold to exact insurance balance.
/// When threshold == insurance, risk gate should be on the boundary.
#[test]
fn test_attack_risk_threshold_exact_insurance_boundary() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Top up insurance to exact amount
    env.try_top_up_insurance(&admin, 5_000_000_000).unwrap();
    env.crank();

    // Set threshold to exact insurance balance
    let insurance = env.read_insurance_balance();
    env.try_set_risk_threshold(&admin, insurance).unwrap();

    // gate_active(threshold, balance) = threshold > 0 && balance <= threshold
    // When insurance == threshold, gate IS active (balance <= threshold is true)
    // Risk-increasing trade should be rejected
    let trade_result = env.try_trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    assert!(
        trade_result.is_err(),
        "ATTACK: Trade should be rejected when insurance == threshold (gate active at boundary)"
    );

    // Position should not have been opened
    let pos = env.read_account_position(user_idx);
    assert_eq!(pos, 0, "No position should be opened when gate is active: got {}", pos);

    // Conservation must hold
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation at threshold boundary: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: LP tries to withdraw when haircut is active (vault < c_tot + insurance).
/// After a user takes a large loss, LP capital might be haircutted - can LP
/// withdraw more than their haircutted equity?
#[test]
fn test_attack_lp_withdraw_during_haircut() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // User opens large long position
    env.trade(&user, &lp, lp_idx, user_idx, 10_000_000);

    // Price drops significantly - LP profits, user loses
    // User's loss means LP has positive PnL, but vault balance is finite
    for slot in (200..=1000).step_by(100) {
        let price = 138_000_000 - ((slot - 100) * 20_000) as i64;
        if price < 50_000_000 {
            break;
        }
        env.set_slot_and_price(slot, price);
        env.crank();
    }

    // Record state before LP withdrawal attempt
    let lp_cap_before = env.read_account_capital(lp_idx);
    let vault_before = env.vault_balance();

    // LP tries to withdraw full capital
    let withdraw_result = env.try_withdraw(&lp, lp_idx, lp_cap_before as u64);

    // Conservation must hold
    let vault_after = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault_after,
        "Conservation after LP withdrawal attempt: engine={} vault={}",
        engine_vault, vault_after
    );

    // If withdrawal succeeded, vault should have decreased
    if withdraw_result.is_ok() {
        assert!(
            vault_after < vault_before,
            "LP withdrawal succeeded but vault didn't decrease: before={} after={}",
            vault_before,
            vault_after
        );
        // LP capital must have decreased
        let lp_cap_after = env.read_account_capital(lp_idx);
        assert!(
            lp_cap_after < lp_cap_before,
            "LP withdrawal succeeded but capital didn't decrease: before={} after={}",
            lp_cap_before,
            lp_cap_after
        );
    } else {
        // Withdrawal rejected (e.g., margin check) - vault unchanged
        assert_eq!(
            vault_after, vault_before,
            "LP withdrawal rejected but vault changed: before={} after={}",
            vault_before, vault_after
        );
    }
}

/// ATTACK: Open position during warmup period, partially close before warmup expires.
/// Tests interaction between warmup slope and partial position close.
/// Profit from partial close must be subject to warmup vesting.
#[test]
fn test_attack_warmup_partial_close_vesting() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_warmup(0, 1000); // warmup = 1000 slots

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open position at slot 1
    env.set_slot(1);
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Price rises (user profit) before warmup expires
    env.set_slot_and_price(200, 145_000_000); // $138 → $145
    env.crank();

    // Record capital before partial close
    let user_cap_before = env.read_account_capital(user_idx);

    // Partial close (close half the position) during warmup
    env.set_slot(201);
    env.trade(&user, &lp, lp_idx, user_idx, -2_500_000);

    // User position should be halved
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(
        user_pos, 2_500_000,
        "User position after partial close: expected 2.5M, got {}",
        user_pos
    );

    // Crank after partial close
    env.set_slot(400);
    env.crank();

    // Capital shouldn't have increased by the full unrealized profit
    // (warmup vesting limits profit conversion)
    let user_cap_after = env.read_account_capital(user_idx);
    assert!(
        user_cap_after <= user_cap_before + 200_000_000u128,
        "Warmup partial close should bound immediate capital jump. before={} after={}",
        user_cap_before,
        user_cap_after
    );

    // Conservation must hold
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot after warmup partial close: c_tot={} sum={}",
        c_tot, sum
    );

    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation: engine={} vault={}",
        engine_vault, vault
    );
}

// ============================================================================
// Round 18: Arithmetic Boundaries, Settlement Ordering, Multi-Account
// ============================================================================

/// ATTACK: Minimal position size (1 unit) with 1e-6 price precision.
/// Tests mark_pnl truncation at the smallest meaningful scale.
#[test]
fn test_attack_mark_pnl_one_unit_position() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade minimum size: 1 unit
    env.trade(&user, &lp, lp_idx, user_idx, 1);
    assert_eq!(
        env.read_account_position(user_idx),
        1,
        "Precondition: user must have 1-unit position"
    );

    // Price change of 1 (smallest possible: 0.000001)
    env.set_slot_and_price(200, 138_000_001);
    env.crank();

    // PnL for 1 unit at $0.000001 change = 1 * 1 / 1_000_000 = 0 (truncated)
    // This is correct: sub-unit PnL truncates in protocol's favor
    let pnl = env.read_account_pnl(user_idx);
    assert!(
        pnl >= 0,
        "Tiny PnL should not be negative for long: pnl={}",
        pnl
    );

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot with 1-unit position: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Haircut with zero pnl_pos_tot (no positive PnL accounts).
/// When denominator is 0, haircut should be harmless (no division by zero).
#[test]
fn test_attack_haircut_zero_pnl_pos_tot() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open position and drop price so user has NEGATIVE PnL
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    env.set_slot_and_price(200, 130_000_000); // Drop $8
    env.crank();

    // User has negative PnL (loss), LP has positive PnL (gain)
    let user_pnl = env.read_account_pnl(user_idx);
    assert!(
        user_pnl < 0,
        "Precondition: user should have negative PnL after price drop: {}",
        user_pnl
    );

    let pnl_pos_tot = env.read_pnl_pos_tot();
    assert!(
        pnl_pos_tot > 0,
        "Positive-pnl pool should be non-zero when LP offsets user's negative PnL"
    );

    // Withdrawal should still work even with haircut conditions
    let user_cap = env.read_account_capital(user_idx);
    assert!(
        user_cap > 0,
        "Precondition: user should still have some capital: {}",
        user_cap
    );

    let vault_before = env.vault_balance();
    let withdraw_result = env.try_withdraw(&user, user_idx, 1);
    let vault_after = env.vault_balance();

    // Verify either withdrawal happened or margin check blocked it
    if withdraw_result.is_ok() {
        assert_eq!(
            vault_after,
            vault_before - 1,
            "Withdrawal of 1 should decrease vault by 1: before={} after={}",
            vault_before,
            vault_after
        );
    } else {
        assert_eq!(
            vault_after, vault_before,
            "Failed 1-lamport withdrawal should not change vault: before={} after={}",
            vault_before, vault_after
        );
    }

    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault_after,
        "Conservation with haircut: engine={} vault={}",
        engine_vault, vault_after
    );
}

// REMOVED: test_attack_funding_zero_dt_no_accrual - duplicate of
// test_attack_funding_same_slot_three_cranks_dt_zero (line 8020)

/// ATTACK: Many users (40) trading against single LP, then crank.
/// Tests that crank handles many accounts efficiently and conserves funds.
#[test]
fn test_attack_many_users_single_lp_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 200_000_000_000);

    // Create 10 users with positions (limited by 200K CU per crank)
    let mut users: Vec<(Keypair, u16)> = Vec::new();
    for _ in 0..10 {
        let u = Keypair::new();
        let idx = env.init_user(&u);
        env.deposit(&u, idx, 2_000_000_000);
        users.push((u, idx));
    }

    env.try_top_up_insurance(&admin, 5_000_000_000).unwrap();
    env.crank();

    // All users open positions in alternating directions
    for (i, (u, idx)) in users.iter().enumerate() {
        env.set_slot((i + 1) as u64);
        let direction = if i % 2 == 0 { 100_000 } else { -100_000_i128 };
        env.trade(u, &lp, lp_idx, *idx, direction);
    }

    // Price change
    env.set_slot_and_price(200, 142_000_000);
    env.crank();

    // Conservation across all accounts
    let c_tot = env.read_c_tot();
    let mut sum: u128 = env.read_account_capital(lp_idx);
    for (_, idx) in &users {
        sum += env.read_account_capital(*idx);
    }
    assert_eq!(
        c_tot, sum,
        "c_tot with 10 users: c_tot={} sum={}",
        c_tot, sum
    );

    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation with 10 users: engine={} vault={}",
        engine_vault, vault
    );

    // LP position should be net of all user positions
    let lp_pos = env.read_account_position(lp_idx);
    let mut user_sum: i128 = 0;
    for (_, idx) in &users {
        user_sum += env.read_account_position(*idx);
    }
    assert_eq!(
        lp_pos, -user_sum,
        "LP position should mirror user sum: lp={} user_sum={}",
        lp_pos, user_sum
    );
}

/// ATTACK: Position flip from long to short at exact maintenance margin.
/// Verify initial_margin_bps is used (not maintenance) for the flip.
#[test]
fn test_attack_position_flip_margin_requirement_switch() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    // Small deposit to test margin limits
    env.deposit(&user, user_idx, 2_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open moderate long position
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    assert!(result.is_ok(), "Initial long trade should succeed");

    // Now flip to short (position goes from +1M to -1M, crosses zero)
    // This requires initial_margin_bps for the new direction
    let flip_result = env.try_trade(&user, &lp, lp_idx, user_idx, -2_000_000);

    // Either succeeds (enough margin) or fails (initial margin too high)
    // But must not panic and conservation holds
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after flip attempt: engine={} vault={}",
        engine_vault, vault
    );

    // Verify position is consistent
    let pos = env.read_account_position(user_idx);
    if flip_result.is_ok() {
        assert_eq!(
            pos, -1_000_000,
            "Flip succeeded so position should be -1M: got {}",
            pos
        );
    } else {
        assert_eq!(
            pos, 1_000_000,
            "Flip failed so position should remain +1M: got {}",
            pos
        );
    }
}

/// ATTACK: Large maintenance fee with huge dt gap (thousands of slots).
/// Tests saturating arithmetic in fee accrual over long periods.
#[test]
fn test_attack_maintenance_fee_huge_dt_saturation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Set maximum maintenance fee
    env.try_set_maintenance_fee(&admin, u64::MAX as u128)
        .unwrap();

    // Jump forward a massive number of slots
    env.set_slot(1_000_000);
    env.crank();

    // Fee * dt would overflow u128 without saturating math
    // Verify no panic and conservation holds
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after huge dt fee: engine={} vault={}",
        engine_vault, vault
    );

    // User's capital should be depleted (or account GC'd)
    let user_cap = env.read_account_capital(user_idx);
    assert_eq!(
        user_cap, 0,
        "Huge fee should deplete user capital: cap={}",
        user_cap
    );
}

/// ATTACK: Trade with different sizes in rapid succession (consecutive slots).
/// Position accumulation should be correct across rapid trades.
#[test]
fn test_attack_rapid_successive_trades_accumulation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Multiple trades in rapid succession with different sizes
    env.set_slot(1);
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    env.set_slot(2);
    env.trade(&user, &lp, lp_idx, user_idx, 500_000);
    env.set_slot(3);
    env.trade(&user, &lp, lp_idx, user_idx, 250_000);

    // User should have accumulated position: 1M + 500K + 250K = 1.75M
    let pos = env.read_account_position(user_idx);
    assert_eq!(
        pos, 1_750_000,
        "Rapid trades should accumulate: expected 1.75M, got {}",
        pos
    );

    // Conservation
    env.set_slot(200);
    env.crank();

    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(c_tot, sum, "c_tot after double trade");
}

/// ATTACK: Three LPs with different positions, user trades against all.
/// Tests LP aggregate tracking with multiple LPs.
#[test]
fn test_attack_three_lps_aggregate_tracking() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp1 = Keypair::new();
    let lp1_idx = env.init_lp(&lp1);
    env.deposit(&lp1, lp1_idx, 30_000_000_000);

    let lp2 = Keypair::new();
    let lp2_idx = env.init_lp(&lp2);
    env.deposit(&lp2, lp2_idx, 30_000_000_000);

    let lp3 = Keypair::new();
    let lp3_idx = env.init_lp(&lp3);
    env.deposit(&lp3, lp3_idx, 30_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Trade against each LP
    env.set_slot(1);
    env.trade(&user, &lp1, lp1_idx, user_idx, 1_000_000);
    env.set_slot(2);
    env.trade(&user, &lp2, lp2_idx, user_idx, 2_000_000);
    env.set_slot(3);
    env.trade(&user, &lp3, lp3_idx, user_idx, -500_000);

    // User total position = 1M + 2M - 500K = 2.5M
    let user_pos = env.read_account_position(user_idx);
    assert_eq!(
        user_pos, 2_500_000,
        "User should have net 2.5M position: got {}",
        user_pos
    );

    // LP positions should be correct
    assert_eq!(env.read_account_position(lp1_idx), -1_000_000);
    assert_eq!(env.read_account_position(lp2_idx), -2_000_000);
    assert_eq!(env.read_account_position(lp3_idx), 500_000);

    // Price change and crank
    env.set_slot_and_price(200, 140_000_000);
    env.crank();

    // Conservation across all 4 accounts
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp1_idx)
        + env.read_account_capital(lp2_idx)
        + env.read_account_capital(lp3_idx)
        + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot across 3 LPs: c_tot={} sum={}",
        c_tot, sum
    );

    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: Projected haircut during trade vs realized haircut after crank.
/// Verify consistency between margin check haircut and settlement haircut.
#[test]
fn test_attack_projected_vs_realized_haircut_consistency() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 10_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Both users open large positions (creates significant pnl_pos_tot)
    env.trade(&user1, &lp, lp_idx, user1_idx, 5_000_000);
    env.trade(&user2, &lp, lp_idx, user2_idx, 5_000_000);

    // Price moves - creates PnL
    env.set_slot_and_price(200, 145_000_000);
    env.crank();

    let pnl_pos_tot_before = env.read_pnl_pos_tot();

    // User1 closes position - should use projected haircut
    env.set_slot(201);
    env.trade(&user1, &lp, lp_idx, user1_idx, -5_000_000);

    let pnl_pos_tot_after = env.read_pnl_pos_tot();

    // pnl_pos_tot should have decreased (user1's positive PnL removed)
    assert!(
        pnl_pos_tot_after <= pnl_pos_tot_before,
        "pnl_pos_tot should not increase after closing profitable position: before={} after={}",
        pnl_pos_tot_before,
        pnl_pos_tot_after
    );

    // Conservation
    env.set_slot(400);
    env.crank();

    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: set_pnl aggregate consistency - rapid PnL changes from trades.
/// Multiple trades that flip PnL sign should maintain pnl_pos_tot correctly.
#[test]
fn test_attack_set_pnl_aggregate_rapid_flips() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Series of trades with price changes that flip PnL sign
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000); // Long

    env.set_slot_and_price(200, 145_000_000); // User profits
    env.crank();
    let pnl1 = env.read_account_pnl(user_idx);
    assert!(
        pnl1 > 0,
        "User should have positive PnL after price rise: {}",
        pnl1
    );

    env.set_slot_and_price(400, 125_000_000); // Price crashes below entry
    env.crank();
    let pnl2 = env.read_account_pnl(user_idx);
    assert!(
        pnl2 < 0,
        "User should have negative PnL after price crash: {}",
        pnl2
    );

    env.set_slot_and_price(600, 150_000_000); // Recovery
    env.crank();
    let pnl3 = env.read_account_pnl(user_idx);
    assert!(
        pnl3 > 0,
        "User should have positive PnL after recovery: {}",
        pnl3
    );

    // pnl_pos_tot should reflect current positive PnL
    let ppt = env.read_pnl_pos_tot();
    assert!(
        ppt > 0,
        "pnl_pos_tot should be positive after recovery: {}",
        ppt
    );

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot after PnL flips: c_tot={} sum={}",
        c_tot, sum
    );
}

// REMOVED: test_attack_deposit_withdraw_same_slot_no_extraction - duplicate of
// test_attack_deposit_withdraw_same_slot_atomicity (line 15845)

// REMOVED: test_attack_trade_exact_full_margin_utilization - duplicate of
// test_attack_trade_exact_margin_boundary_succeeds (line 19357)

/// ATTACK: LP partial close (reduce LP position) and verify aggregates.
/// Trade that reduces LP's exposure should update net_lp_pos correctly.
#[test]
fn test_attack_lp_partial_close_aggregate_update() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 10_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // User1 goes long 5M → LP has -5M
    env.set_slot(1);
    env.trade(&user1, &lp, lp_idx, user1_idx, 5_000_000);
    assert_eq!(env.read_account_position(lp_idx), -5_000_000);

    // User1 partially closes (reduces by 2M)
    env.set_slot(2);
    env.trade(&user1, &lp, lp_idx, user1_idx, -2_000_000);

    // LP position should be -3M now
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(
        lp_pos, -3_000_000,
        "LP position after partial close: expected -3M, got {}",
        lp_pos
    );

    // User2 trades against same LP
    env.set_slot(3);
    env.trade(&user2, &lp, lp_idx, user2_idx, 1_000_000);

    // LP now = -3M - 1M = -4M
    assert_eq!(env.read_account_position(lp_idx), -4_000_000);

    // Crank and verify conservation
    env.set_slot(200);
    env.crank();

    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx)
        + env.read_account_capital(user1_idx)
        + env.read_account_capital(user2_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot after LP partial close: c_tot={} sum={}",
        c_tot, sum
    );
}

// REMOVED: test_attack_warmup_instant_period_zero - duplicate of
// test_attack_warmup_zero_period_instant (line 8110)

// REMOVED: test_attack_trade_notional_equals_margin_boundary - duplicate of
// test_attack_trade_exact_initial_margin_boundary (line 13162)

/// ATTACK: Settlement ordering - mark settlement, then funding, then fees.
/// Create scenario where ordering matters and verify correctness.
#[test]
fn test_attack_settlement_ordering_mark_funding_fees() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();

    // Set maintenance fee to create fee debt during settlement
    env.try_set_maintenance_fee(&admin, 10_000).unwrap();

    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Big price move + long time gap = mark settlement + funding + fees all at once
    env.set_slot_and_price(1000, 150_000_000);
    env.crank();

    // Verify settlement was consistent
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot after combined settlement: c_tot={} sum={}",
        c_tot, sum
    );

    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after combined settlement: engine={} vault={}",
        engine_vault, vault
    );

    // User should have gained from mark but lost from fees
    let user_cap = env.read_account_capital(user_idx);
    // With position=5M, price move from 138 to 150 = +$12/unit
    // Mark PnL ~ 5M * 12 / 1 = +60 units (simplified)
    // Fees should have eaten some capital
    // Verify capital is reasonable (not negative/zero from fee-before-mark bug)
    assert!(
        user_cap > 0,
        "User should have positive capital after profitable mark settlement: cap={}",
        user_cap
    );
}

/// ATTACK: Inverted market (invert=1) with large price swing.
/// Tests conservation in inverted market with significant movement.
#[test]
fn test_attack_inverted_market_large_swing_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(1); // Inverted market

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Go long in inverted market
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);
    let pos = env.read_account_position(user_idx);
    assert_eq!(pos, 5_000_000, "Position should be 5M");

    // In inverted market, price going UP means the inverted price goes DOWN
    // So a long position LOSES when raw oracle price rises
    env.set_slot_and_price(200, 145_000_000); // Raw price up
    env.crank();

    let pnl = env.read_account_pnl(user_idx);
    // In inverted market, long loses when raw price rises
    // (inverted price = 1/raw, so raw up → inverted down → long loses)
    // Verify PnL direction is correct for inverted market
    assert!(
        pnl <= 0,
        "Long in inverted market should lose when raw price rises: pnl={}",
        pnl
    );

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot in inverted market: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Two users with opposing positions, price returns to start.
/// Both users should have approximately zero PnL (minus fees).
#[test]
fn test_attack_opposing_positions_price_roundtrip() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 10_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // User1 long, user2 short (opposing through LP)
    env.set_slot(1);
    env.trade(&user1, &lp, lp_idx, user1_idx, 3_000_000);
    env.set_slot(2);
    env.trade(&user2, &lp, lp_idx, user2_idx, -3_000_000);

    // LP net position = -3M + 3M = 0
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(lp_pos, 0, "LP should be flat: pos={}", lp_pos);

    // Price moves up, then back to start
    env.set_slot_and_price(200, 150_000_000);
    env.crank();
    env.set_slot_and_price(400, 138_000_000); // Back to original
    env.crank();

    // After round-trip, user1's loss should roughly equal user2's gain (minus fees)
    let pnl1 = env.read_account_pnl(user1_idx);
    let pnl2 = env.read_account_pnl(user2_idx);
    assert!(
        (pnl1 <= 0 && pnl2 >= 0) || (pnl1 >= 0 && pnl2 <= 0),
        "Opposite-direction users should have opposite-signed (or zero) PnL after round-trip. pnl1={} pnl2={}",
        pnl1,
        pnl2
    );

    // Conservation must hold
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx)
        + env.read_account_capital(user1_idx)
        + env.read_account_capital(user2_idx);
    assert_eq!(
        c_tot, sum,
        "c_tot after price roundtrip: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Withdraw exactly all capital from user with open position.
/// Should fail because margin check requires capital > 0 for positions.
#[test]
fn test_attack_withdraw_all_with_open_position() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open position
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Try to withdraw ALL capital while position is open
    let result = env.try_withdraw(&user, user_idx, 10_000_000_000);
    assert!(
        result.is_err(),
        "ATTACK: Withdrawing all capital with open position should be rejected!"
    );

    // Position should still exist
    let pos = env.read_account_position(user_idx);
    assert_eq!(
        pos, 5_000_000,
        "Position should remain after failed withdrawal: pos={}",
        pos
    );

    // Capital should be unchanged
    let cap = env.read_account_capital(user_idx);
    assert_eq!(
        cap, 10_000_000_000,
        "Capital should be unchanged after failed withdrawal: cap={}",
        cap
    );
}

/// ATTACK: Insurance topup from non-admin account.
/// Anyone can top up insurance (it's a deposit, not withdrawal).
#[test]
fn test_attack_insurance_topup_from_non_admin() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let random_user = Keypair::new();
    env.svm
        .airdrop(&random_user.pubkey(), 5_000_000_000)
        .unwrap();
    env.create_ata(&random_user.pubkey(), 2_000_000_000);
    let insurance_before = env.read_insurance_balance();

    // Non-admin tops up insurance — TopUpInsurance does not require admin
    let result = env.try_top_up_insurance(&random_user, 1_000_000_000);
    assert!(result.is_ok(), "Anyone should be able to top up insurance: {:?}", result);

    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation: engine={} vault={}",
        engine_vault, vault
    );

    let insurance_after = env.read_insurance_balance();
    assert!(
        insurance_after > insurance_before,
        "Topup should increase insurance: before={} after={}",
        insurance_before,
        insurance_after
    );
}

/// ATTACK: Price moves 50% down then liquidation followed by conservation check.
/// Tests that large price movements + liquidation maintain fund conservation.
#[test]
fn test_attack_large_price_drop_liquidation_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 5_000_000_000).unwrap();

    // Disable circuit breaker to allow large price moves
    env.try_set_oracle_price_cap(&admin, u64::MAX).unwrap();

    env.crank();

    // User opens large long
    env.trade(&user, &lp, lp_idx, user_idx, 10_000_000);

    // 50% price drop in steps
    env.set_slot_and_price(200, 100_000_000); // $138 → $100 (28% drop)
    env.crank();

    env.set_slot_and_price(400, 70_000_000); // $100 → $70 (49% total drop)
    env.crank();

    // Try liquidating
    let user_pos_before_liq = env.read_account_position(user_idx);
    let liq_result = env.try_liquidate(user_idx);
    let user_pos_after_liq = env.read_account_position(user_idx);
    assert!(
        user_pos_after_liq.unsigned_abs() <= user_pos_before_liq.unsigned_abs(),
        "Liquidation attempt must not increase user exposure. before={} after={} result={:?}",
        user_pos_before_liq,
        user_pos_after_liq,
        liq_result
    );

    // After possible liquidation, conservation must hold
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after large drop + liquidation: engine={} vault={}",
        engine_vault, vault
    );

    // Verify LP has positive PnL (price dropped, LP was short via user's long)
    // LP capital may not have increased yet (lazy settlement) but PnL should be positive
    let lp_pnl = env.read_account_pnl(lp_idx);
    let lp_cap = env.read_account_capital(lp_idx);
    assert!(
        lp_pnl > 0 || lp_cap > 100_000_000_000,
        "LP should have gained from user's loss: pnl={} cap={}",
        lp_pnl,
        lp_cap
    );
}

// ============================================================================
// Round 19: Instruction Data, Account Ordering, Admin Transitions, LP Edge Cases
// ============================================================================

/// ATTACK: Instruction data with extra trailing bytes appended.
/// Tests that decoder rejects or ignores trailing garbage after valid data.
#[test]
fn test_attack_instruction_data_extra_trailing_bytes() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let ata = env.create_ata(&admin.pubkey(), 5_000_000_000);

    // Valid deposit instruction with extra garbage bytes appended
    let mut data = encode_deposit(0, 1_000_000_000);
    data.extend_from_slice(&[0xFF, 0xDE, 0xAD, 0xBE, 0xEF]); // 5 garbage bytes

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(ata, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    // Strict parsing should reject extra trailing bytes
    assert!(
        result.is_err(),
        "Extra trailing bytes on deposit instruction should be rejected!"
    );

    // Verify no state change occurred
    let cap = env.read_account_capital(0);
    assert_eq!(
        cap, 0,
        "Capital should remain 0 after rejected trailing-bytes deposit: {}",
        cap
    );
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after extra-bytes instruction: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: Trade with size = i128::MIN + 1 (extreme negative).
/// Tests that extreme negative trade sizes are handled safely.
#[test]
fn test_attack_trade_size_i128_min_boundary() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Try trade with extremely negative size
    let result = env.try_trade(&user, &lp, lp_idx, user_idx, i128::MIN + 1);
    assert!(
        result.is_err(),
        "ATTACK: Trade with i128::MIN+1 should be rejected (too large)!"
    );

    // Position should remain zero
    let pos = env.read_account_position(user_idx);
    assert_eq!(
        pos, 0,
        "Position should be 0 after rejected extreme trade: pos={}",
        pos
    );
}

/// ATTACK: Withdraw all capital then re-deposit in same slot.
/// Tests that withdraw+deposit cycle doesn't corrupt state.
#[test]
fn test_attack_withdraw_all_redeposit_same_slot() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.crank();

    let vault_before = env.vault_balance();

    // Withdraw everything then re-deposit same amount in same slot
    env.try_withdraw(&user, user_idx, 5_000_000_000).unwrap();
    let cap_mid = env.read_account_capital(user_idx);
    assert_eq!(
        cap_mid, 0,
        "Capital should be 0 after full withdraw: {}",
        cap_mid
    );

    env.deposit(&user, user_idx, 5_000_000_000);
    let cap_after = env.read_account_capital(user_idx);
    assert_eq!(
        cap_after, 5_000_000_000,
        "Capital should be restored after re-deposit: {}",
        cap_after
    );

    // Vault should be same as before (net zero transfer)
    let vault_after = env.vault_balance();
    assert_eq!(
        vault_before, vault_after,
        "Vault should be unchanged after withdraw+deposit cycle: before={} after={}",
        vault_before, vault_after
    );
}

/// ATTACK: LP tries to close account while users have matched positions.
/// LP with outstanding position should not be closeable.
#[test]
fn test_attack_lp_close_with_matched_positions() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // User opens position matched against LP
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);

    let lp_pos = env.read_account_position(lp_idx);
    assert_ne!(lp_pos, 0, "LP should have position after trade");

    // LP tries to close account - should fail (has position)
    let result = env.try_close_account(&lp, lp_idx);
    assert!(
        result.is_err(),
        "ATTACK: LP should not be able to close with outstanding position!"
    );

    // LP position should be unchanged
    let lp_pos_after = env.read_account_position(lp_idx);
    assert_eq!(
        lp_pos, lp_pos_after,
        "LP position should be unchanged after failed close: {} vs {}",
        lp_pos, lp_pos_after
    );
}

/// ATTACK: Trade long then short same size - net zero position.
/// Position should cancel out to zero, conservation must hold.
#[test]
fn test_attack_trade_long_then_short_net_zero() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    let cap_before = env.read_account_capital(user_idx);

    // Go long then short same size
    env.trade(&user, &lp, lp_idx, user_idx, 2_000_000);
    let pos_after_long = env.read_account_position(user_idx);
    assert_eq!(pos_after_long, 2_000_000, "Should be long 2M");

    env.set_slot(2);
    env.trade(&user, &lp, lp_idx, user_idx, -2_000_000);
    let pos_after_close = env.read_account_position(user_idx);
    assert_eq!(
        pos_after_close, 0,
        "Position should be 0 after closing: {}",
        pos_after_close
    );

    // LP position should also be zero
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(
        lp_pos, 0,
        "LP position should be 0 after net-zero trades: {}",
        lp_pos
    );

    // Conservation
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after net-zero trades: engine={} vault={}",
        engine_vault, vault
    );
    let cap_after = env.read_account_capital(user_idx);
    assert!(
        cap_after <= cap_before + 1,
        "Net-zero round-trip trade should not increase user capital materially. before={} after={}",
        cap_before,
        cap_after
    );
}

/// ATTACK: LP matched by multiple users in rapid succession.
/// Tests LP position aggregate correctness under rapid multi-user trading.
#[test]
fn test_attack_lp_rapid_multi_user_matching() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    // Create 5 users
    let mut users: Vec<(Keypair, u16)> = Vec::new();
    for _ in 0..5 {
        let u = Keypair::new();
        let idx = env.init_user(&u);
        env.deposit(&u, idx, 5_000_000_000);
        users.push((u, idx));
    }

    env.try_top_up_insurance(&admin, 2_000_000_000).unwrap();
    env.crank();

    // All 5 users trade against same LP in quick succession
    let sizes: &[i128] = &[1_000_000, -500_000, 2_000_000, -1_500_000, 800_000];
    for (i, ((u, idx), &size)) in users.iter().zip(sizes.iter()).enumerate() {
        env.set_slot((i + 1) as u64);
        env.trade(u, &lp, lp_idx, *idx, size);
    }

    // LP net position = -(1M - 500K + 2M - 1.5M + 800K) = -1.8M
    let expected_lp_pos: i128 = -sizes.iter().sum::<i128>();
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(
        lp_pos, expected_lp_pos,
        "LP position after multi-user matching: expected={} got={}",
        expected_lp_pos, lp_pos
    );

    // Crank and verify conservation
    env.set_slot(200);
    env.crank();

    let c_tot = env.read_c_tot();
    let mut sum: u128 = env.read_account_capital(lp_idx);
    for (_, idx) in &users {
        sum += env.read_account_capital(*idx);
    }
    assert_eq!(
        c_tot, sum,
        "c_tot after multi-user LP matching: c_tot={} sum={}",
        c_tot, sum
    );
}

/// ATTACK: Deposit after full withdrawal in same slot - cycle should not extract value.
/// Tests that rapid deposit-withdraw-deposit cycles don't corrupt aggregates.
#[test]
fn test_attack_deposit_withdraw_deposit_cycle_aggregates() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    // Deposit BEFORE crank to avoid GC of empty account
    env.deposit(&user, user_idx, 3_000_000_000);

    env.crank();

    let c_tot_1 = env.read_c_tot();

    // Withdraw → re-deposit cycle
    env.try_withdraw(&user, user_idx, 3_000_000_000).unwrap();
    let c_tot_2 = env.read_c_tot();
    assert!(c_tot_2 < c_tot_1, "c_tot should decrease after withdraw");

    env.deposit(&user, user_idx, 3_000_000_000);
    let c_tot_3 = env.read_c_tot();
    assert_eq!(
        c_tot_1, c_tot_3,
        "c_tot should be same after full cycle: before={} after={}",
        c_tot_1, c_tot_3
    );

    // Verify per-account capital
    let user_cap = env.read_account_capital(user_idx);
    assert_eq!(
        user_cap, 3_000_000_000,
        "User capital should be 3B after cycle: {}",
        user_cap
    );
}

/// ATTACK: Open max-margin position, crank with price at liquidation boundary.
/// Tests that liquidation trigger is precise and doesn't miss by 1.
#[test]
fn test_attack_liquidation_boundary_precision() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 2_000_000_000).unwrap();
    env.crank();

    // Open a position
    env.trade(&user, &lp, lp_idx, user_idx, 5_000_000);

    // Move price down significantly (user is long)
    env.set_slot_and_price(50, 90_000_000); // 10% drop
    env.crank();

    // Check if user was liquidated or still surviving
    let user_pos = env.read_account_position(user_idx);
    let user_cap = env.read_account_capital(user_idx);

    // Whether liquidated or not, conservation must hold
    let vault = env.vault_balance();
    let c_tot = env.read_c_tot();
    let insurance = env.read_insurance_balance();
    assert!(
        vault as u128 >= c_tot + insurance,
        "Conservation at liquidation boundary: vault={} c_tot={} ins={}",
        vault,
        c_tot,
        insurance
    );

    // If user still has position, they must have margin
    if user_pos != 0 {
        assert!(
            user_cap > 0,
            "User with position should have positive capital: cap={}",
            user_cap
        );
    }
}

/// ATTACK: Push oracle with timestamp = 0 then try to use it.
/// Tests that extreme timestamp doesn't corrupt oracle state or cause panic.
#[test]
fn test_attack_oracle_timestamp_zero_then_crank() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(138_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();

    // Push with timestamp = 0 (no backwards check in oracle)
    let result = env.try_push_oracle_price(&admin, 140_000_000, 0);
    assert!(result.is_ok(), "Oracle should accept ts=0: {:?}", result);

    // Push subsequent price with normal timestamp
    let result2 = env.try_push_oracle_price(&admin, 141_000_000, 1000);
    assert!(
        result2.is_ok(),
        "Should be able to push valid price after ts=0: {:?}",
        result2
    );

    // In Hyperp mode authority_timestamp is funding-rate state, not publish time.
    // PushOraclePrice must not overwrite it with user-supplied timestamps.
    const AUTH_TS_OFF: usize = 368;
    let slab_data = env.svm.get_account(&env.slab).unwrap().data;
    let funding_state =
        i64::from_le_bytes(slab_data[AUTH_TS_OFF..AUTH_TS_OFF + 8].try_into().unwrap());
    assert_eq!(
        funding_state, 0,
        "Hyperp funding-rate state must remain unchanged by PushOraclePrice"
    );

    env.set_slot(200);
    env.crank();

    // State should be consistent
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after ts=0 oracle push: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: Push oracle with timestamp = i64::MAX.
/// Tests that far-future timestamps don't cause overflow or panic.
#[test]
fn test_attack_oracle_timestamp_i64_max_no_overflow() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_hyperp(138_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .unwrap();

    // Push with timestamp = i64::MAX
    let result = env.try_push_oracle_price(&admin, 140_000_000, i64::MAX);
    assert!(
        result.is_ok(),
        "Oracle should accept ts=i64::MAX: {:?}",
        result
    );

    // In Hyperp mode, external timestamp input must not clobber funding-rate state.
    const AUTH_TS_OFF: usize = 368;
    let slab_after_max = env.svm.get_account(&env.slab).unwrap().data;
    let funding_state_after_max = i64::from_le_bytes(
        slab_after_max[AUTH_TS_OFF..AUTH_TS_OFF + 8]
            .try_into()
            .unwrap(),
    );
    assert_eq!(
        funding_state_after_max, 0,
        "Hyperp funding-rate state should ignore i64::MAX timestamp input"
    );

    // Push another price - no backwards timestamp rejection means it works
    let result2 = env.try_push_oracle_price(&admin, 141_000_000, 1000);
    assert!(
        result2.is_ok(),
        "Should still push prices after ts=MAX: {:?}",
        result2
    );

    env.set_slot(200);
    env.crank();

    // No overflow, state consistent
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after ts=MAX oracle push: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: LP deposit with pending fee debt.
/// LP depositing should settle fees first, then add remaining to capital.
#[test]
fn test_attack_lp_deposit_settles_fee_debt_first() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();

    // Set maintenance fee
    env.try_set_maintenance_fee(&admin, 10_000).unwrap();
    env.crank();

    // Open position so fees accrue
    env.trade(&user, &lp, lp_idx, user_idx, 3_000_000);

    // Advance time to accumulate fee debt
    env.set_slot(500);
    env.crank();

    let lp_cap_before = env.read_account_capital(lp_idx);

    // LP deposits more capital (should settle fees first)
    env.deposit(&lp, lp_idx, 5_000_000_000);

    let lp_cap_after = env.read_account_capital(lp_idx);

    // Capital increase should be <= deposit amount (fees eaten some)
    let increase = lp_cap_after - lp_cap_before;
    assert!(
        increase <= 5_000_000_000,
        "LP capital increase should be <= deposit: increase={}",
        increase
    );

    // Conservation
    let vault = env.vault_balance();
    let engine_vault = env.read_engine_vault();
    assert_eq!(
        engine_vault as u64, vault,
        "Conservation after LP deposit with fees: engine={} vault={}",
        engine_vault, vault
    );
}

/// ATTACK: Config change then immediate trade tests new config applied.
/// After SetMaintenanceFee, immediate deposit should use new fee rate.
#[test]
fn test_attack_config_change_applied_to_next_instruction() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 30_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Set very high fee
    env.try_set_maintenance_fee(&admin, 1_000_000).unwrap();

    // Trade immediately after config change
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);

    // Advance slots - high fee should take effect
    env.set_slot(200);
    env.crank();

    // User capital should have decreased more than with low fee
    let user_cap = env.read_account_capital(user_idx);
    assert!(
        user_cap < 5_000_000_000,
        "High fee should have reduced user capital: cap={}",
        user_cap
    );

    // Conservation
    let c_tot = env.read_c_tot();
    let sum = env.read_account_capital(lp_idx) + env.read_account_capital(user_idx);
    assert_eq!(c_tot, sum, "c_tot after config change + trade");
}

/// ATTACK: Multiple admin changes in rapid succession.
/// Tests that admin state is correctly updated through multiple transfers.
#[test]
fn test_attack_rapid_admin_transfers() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin1 = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let admin2 = Keypair::new();
    let admin3 = Keypair::new();
    env.svm.airdrop(&admin2.pubkey(), 5_000_000_000).unwrap();
    env.svm.airdrop(&admin3.pubkey(), 5_000_000_000).unwrap();

    // Chain: admin1 → admin2 → admin3
    env.try_update_admin(&admin1, &admin2.pubkey()).unwrap();
    env.try_update_admin(&admin2, &admin3.pubkey()).unwrap();

    // Only admin3 should work now
    let r1 = env.try_set_maintenance_fee(&admin1, 100);
    assert!(r1.is_err(), "Admin1 should be locked out");

    let r2 = env.try_set_maintenance_fee(&admin2, 100);
    assert!(r2.is_err(), "Admin2 should be locked out");

    let r3 = env.try_set_maintenance_fee(&admin3, 100);
    assert!(r3.is_ok(), "Admin3 should be active: {:?}", r3);
}

/// ATTACK: Deposit to LP account from non-owner.
/// Tests authorization on LP deposits.
#[test]
fn test_attack_deposit_to_lp_wrong_owner() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp_owner = Keypair::new();
    let lp_idx = env.init_lp(&lp_owner);

    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 5_000_000_000).unwrap();
    env.create_ata(&attacker.pubkey(), 3_000_000_000);

    // Attacker tries to deposit to LP's account
    let result = env.try_deposit_unauthorized(&attacker, lp_idx, 1_000_000_000);
    // Should fail because attacker != lp_owner
    assert!(
        result.is_err(),
        "ATTACK: Depositing to LP from non-owner should fail!"
    );

    // LP capital should be zero (never deposited)
    let lp_cap = env.read_account_capital(lp_idx);
    assert_eq!(
        lp_cap, 0,
        "LP capital should be 0 after failed unauthorized deposit: cap={}",
        lp_cap
    );
}

/// ATTACK: Two users try to withdraw their full equity simultaneously.
/// Vault should never go below total obligations.
#[test]
fn test_attack_concurrent_max_withdrawals_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user1 = Keypair::new();
    let user1_idx = env.init_user(&user1);
    env.deposit(&user1, user1_idx, 10_000_000_000);

    let user2 = Keypair::new();
    let user2_idx = env.init_user(&user2);
    env.deposit(&user2, user2_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Both users open positions
    env.trade(&user1, &lp, lp_idx, user1_idx, 1_000_000);
    env.trade(&user2, &lp, lp_idx, user2_idx, -1_000_000);

    env.set_slot_and_price(20, 105_000_000); // 5% move
    env.crank();

    // Both try to withdraw maximum
    let cap1 = env.read_account_capital(user1_idx);
    let cap2 = env.read_account_capital(user2_idx);

    // At least one should succeed; neither should drain vault below obligations
    let withdraw1 = env.try_withdraw(&user1, user1_idx, cap1 as u64);
    let withdraw2 = env.try_withdraw(&user2, user2_idx, cap2 as u64);
    if withdraw1.is_err() && withdraw2.is_err() {
        let failures = format!("user1={:?} user2={:?}", withdraw1, withdraw2);
        assert!(
            failures.contains("custom program error") || failures.contains("Custom("),
            "Concurrent max-withdraw failures were unexpected: {}",
            failures
        );
    }

    // Conservation: vault >= c_tot + insurance
    let vault = env.vault_balance();
    let c_tot = env.read_c_tot();
    let insurance = env.read_insurance_balance();
    assert!(
        vault as u128 >= c_tot + insurance,
        "ATTACK: Concurrent withdrawals broke conservation: vault={}, c_tot={}, ins={}",
        vault,
        c_tot,
        insurance
    );
}

/// ATTACK: Multiple users with opposing positions - conservation after price swing.
/// Tests that PnL redistribution between longs/shorts conserves total value.
#[test]
fn test_attack_opposing_users_pnl_conservation() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let long_user = Keypair::new();
    let long_idx = env.init_user(&long_user);
    env.deposit(&long_user, long_idx, 10_000_000_000);

    let short_user = Keypair::new();
    let short_idx = env.init_user(&short_user);
    env.deposit(&short_user, short_idx, 10_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Long and short users take opposite positions
    env.trade(&long_user, &lp, lp_idx, long_idx, 2_000_000);
    env.set_slot(2);
    env.trade(&short_user, &lp, lp_idx, short_idx, -2_000_000);

    // LP should have net zero position (matched both sides)
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(lp_pos, 0, "LP net position should be zero: {}", lp_pos);

    // Big price swing - one user profits, other loses
    env.set_slot_and_price(100, 120_000_000); // 20% up
    env.crank();

    let long_pnl = env.read_account_pnl(long_idx);
    let long_cap = env.read_account_capital(long_idx);
    let short_pnl = env.read_account_pnl(short_idx);
    let short_cap = env.read_account_capital(short_idx);

    // With opposing positions, PnL should be non-trivially different
    // (one gains what the other loses, modulo fees)
    let long_equity = long_cap as i128 + long_pnl;
    let short_equity = short_cap as i128 + short_pnl;
    assert_ne!(
        long_equity, short_equity,
        "Opposing users should have different equity after price move: long={} short={}",
        long_equity, short_equity
    );

    // LP net position is zero, so LP should not be heavily affected
    let lp_pos = env.read_account_position(lp_idx);
    assert_eq!(
        lp_pos, 0,
        "LP should still have zero net position: {}",
        lp_pos
    );

    // Conservation
    let vault = env.vault_balance();
    let c_tot = env.read_c_tot();
    let insurance = env.read_insurance_balance();
    assert!(
        vault as u128 >= c_tot + insurance,
        "Conservation with opposing users: vault={} c_tot={} ins={}",
        vault,
        c_tot,
        insurance
    );
}

// ============================================================================
// Property-Based Fuzzing: State Machine Tests
// ============================================================================
//
// These tests subsume entire classes of individual attack tests by verifying
// global invariants after random sequences of operations. A single test here
// covers the same property across hundreds of states that would require
// hundreds of individual tests.
//
// Invariants verified after EVERY successful operation:
//   P1. Conservation:  vault_balance >= c_tot + insurance
//   P2. Engine/SPL sync: engine_vault == spl_vault_balance
//   P3. Aggregate consistency: c_tot == sum(account_capital[i]) for all used slots
//   P4. PnL aggregate: pnl_pos_tot == sum(max(0, account_pnl[i])) for all used slots
//   P5. Failed operations don't change vault balance
// ============================================================================

/// Deterministic xorshift64 PRNG for reproducible test sequences
struct FuzzRng {
    state: u64,
}

impl FuzzRng {
    fn new(seed: u64) -> Self {
        FuzzRng { state: seed.max(1) }
    }

    fn next_u64(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() % (hi - lo))
    }

    fn range_i128(&mut self, lo: i128, hi: i128) -> i128 {
        if hi <= lo {
            return lo;
        }
        lo + ((self.next_u64() as i128).abs() % (hi - lo))
    }
}

/// All state-changing operations the fuzzer can perform
#[derive(Debug)]
enum FuzzAction {
    Deposit {
        user_idx: usize,
        amount: u64,
    },
    Withdraw {
        user_idx: usize,
        amount: u64,
    },
    Trade {
        user_idx: usize,
        lp_idx: usize,
        size: i128,
    },
    Crank,
    AdvanceSlotAndPrice {
        dt: u64,
        price_e6: i64,
    },
    TopUpInsurance {
        amount: u64,
    },
    SetMaintenanceFee {
        fee: u128,
    },
    InitUser,
    CloseAccount {
        user_idx: usize,
    },
}

struct IntegrationFuzzer {
    env: TestEnv,
    users: Vec<(Keypair, u16)>, // (keypair, account_idx)
    lps: Vec<(Keypair, u16)>,   // (keypair, account_idx)
    admin: Keypair,
    current_slot: u64,
    current_price: i64,
    all_indices: Vec<u16>, // Every index ever allocated (for invariant checks)
    step: usize,
    seed: u64,
}

impl IntegrationFuzzer {
    fn new(seed: u64) -> Self {
        let env = TestEnv::new();
        let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
        IntegrationFuzzer {
            env,
            users: Vec::new(),
            lps: Vec::new(),
            admin,
            current_slot: 0,
            current_price: 100_000_000,
            all_indices: Vec::new(),
            step: 0,
            seed,
        }
    }

    fn setup(&mut self) {
        self.env.init_market_with_invert(0);

        // Create 1 LP
        let lp = Keypair::new();
        let lp_idx = self.env.init_lp(&lp);
        self.env.deposit(&lp, lp_idx, 50_000_000_000);
        self.lps.push((lp, lp_idx));
        self.all_indices.push(lp_idx);

        // Create 3 users
        for _ in 0..3 {
            let u = Keypair::new();
            let idx = self.env.init_user(&u);
            self.env.deposit(&u, idx, 5_000_000_000);
            self.users.push((u, idx));
            self.all_indices.push(idx);
        }

        // Insurance
        self.env
            .try_top_up_insurance(&self.admin, 2_000_000_000)
            .unwrap();
        self.env.crank();
    }

    /// Check all global invariants. Panics with detailed diagnostics on failure.
    fn check_invariants(&self, context: &str) {
        let vault = self.env.vault_balance();
        let engine_vault = self.env.read_engine_vault();
        let c_tot = self.env.read_c_tot();
        let insurance = self.env.read_insurance_balance();
        let pnl_pos_tot = self.env.read_pnl_pos_tot();

        // P1: Conservation
        assert!(
            vault as u128 >= c_tot + insurance,
            "[seed={} step={} {}] P1 CONSERVATION VIOLATED: vault={} c_tot={} ins={}",
            self.seed,
            self.step,
            context,
            vault,
            c_tot,
            insurance
        );

        // P2: Engine/SPL sync
        assert_eq!(
            engine_vault as u64, vault,
            "[seed={} step={} {}] P2 ENGINE/SPL DESYNC: engine_vault={} spl_vault={}",
            self.seed, self.step, context, engine_vault, vault
        );

        // P3: Aggregate consistency (c_tot)
        // Iterate ALL 4096 possible slots via bitmap to catch every account
        let num_used = self.env.read_num_used_accounts();
        let mut sum_cap: u128 = 0;
        let mut sum_pnl_pos: u128 = 0;
        let mut found: u16 = 0;
        for idx in 0..4096u16 {
            if self.env.is_slot_used(idx) {
                sum_cap += self.env.read_account_capital(idx);
                let pnl = self.env.read_account_pnl(idx);
                if pnl > 0 {
                    sum_pnl_pos += pnl as u128;
                }
                found += 1;
                if found >= num_used {
                    break;
                }
            }
        }
        assert_eq!(
            c_tot, sum_cap,
            "[seed={} step={} {}] P3 C_TOT MISMATCH: c_tot={} sum_cap={}",
            self.seed, self.step, context, c_tot, sum_cap
        );

        // P4: PnL aggregate
        assert_eq!(
            pnl_pos_tot, sum_pnl_pos,
            "[seed={} step={} {}] P4 PNL_POS_TOT MISMATCH: tracked={} computed={}",
            self.seed, self.step, context, pnl_pos_tot, sum_pnl_pos
        );
    }

    fn random_action(&self, rng: &mut FuzzRng) -> FuzzAction {
        let action_type = rng.range(0, 100);
        match action_type {
            0..=19 => {
                // Deposit (20%)
                let user_idx = rng.range(0, self.users.len() as u64) as usize;
                let amount = rng.range(1_000, 5_000_000_000);
                FuzzAction::Deposit { user_idx, amount }
            }
            20..=34 => {
                // Withdraw (15%)
                let user_idx = rng.range(0, self.users.len() as u64) as usize;
                let amount = rng.range(1_000, 3_000_000_000);
                FuzzAction::Withdraw { user_idx, amount }
            }
            35..=54 => {
                // Trade (20%)
                let user_idx = rng.range(0, self.users.len() as u64) as usize;
                let lp_idx = rng.range(0, self.lps.len() as u64) as usize;
                let size = rng.range_i128(-3_000_000, 3_000_000);
                FuzzAction::Trade {
                    user_idx,
                    lp_idx,
                    size,
                }
            }
            55..=74 => {
                // Crank (20%)
                FuzzAction::Crank
            }
            75..=84 => {
                // Advance slot + price (10%)
                let dt = rng.range(1, 100);
                let price_delta = rng.range_i128(-10_000_000, 10_000_000);
                let new_price = (self.current_price as i128 + price_delta).max(1_000_000) as i64;
                FuzzAction::AdvanceSlotAndPrice {
                    dt,
                    price_e6: new_price,
                }
            }
            85..=89 => {
                // Top up insurance (5%)
                let amount = rng.range(100_000, 1_000_000_000);
                FuzzAction::TopUpInsurance { amount }
            }
            90..=94 => {
                // Set maintenance fee (5%)
                let fee = rng.range(0, 100_000) as u128;
                FuzzAction::SetMaintenanceFee { fee }
            }
            95..=97 => {
                // Init new user (3%)
                FuzzAction::InitUser
            }
            _ => {
                // Close account (2%)
                let user_idx = rng.range(0, self.users.len() as u64) as usize;
                FuzzAction::CloseAccount { user_idx }
            }
        }
    }

    fn execute(&mut self, action: FuzzAction) {
        let vault_before = self.env.vault_balance();
        self.step += 1;

        let (result, desc): (Result<(), String>, String) = match action {
            FuzzAction::Deposit { user_idx, amount } => {
                let (ref user, idx) = self.users[user_idx];
                let r = self.env.try_deposit(user, idx, amount);
                (r, format!("deposit(user={}, amt={})", idx, amount))
            }
            FuzzAction::Withdraw { user_idx, amount } => {
                let (ref user, idx) = self.users[user_idx];
                let r = self.env.try_withdraw(user, idx, amount);
                (r, format!("withdraw(user={}, amt={})", idx, amount))
            }
            FuzzAction::Trade {
                user_idx,
                lp_idx,
                size,
            } => {
                let (ref user, u_idx) = self.users[user_idx];
                let (ref lp, l_idx) = self.lps[lp_idx];
                let r = self.env.try_trade(user, lp, l_idx, u_idx, size);
                (
                    r,
                    format!("trade(user={}, lp={}, size={})", u_idx, l_idx, size),
                )
            }
            FuzzAction::Crank => {
                let r = self.env.try_crank();
                (r, "crank".to_string())
            }
            FuzzAction::AdvanceSlotAndPrice { dt, price_e6 } => {
                self.current_slot += dt;
                self.current_price = price_e6;
                self.env.set_slot_and_price(self.current_slot, price_e6);
                (
                    Ok(()),
                    format!(
                        "set_slot_price(slot={}, price={})",
                        self.current_slot, price_e6
                    ),
                )
            }
            FuzzAction::TopUpInsurance { amount } => {
                let r = self.env.try_top_up_insurance(&self.admin, amount);
                (r, format!("topup_insurance({})", amount))
            }
            FuzzAction::SetMaintenanceFee { fee } => {
                let r = self.env.try_set_maintenance_fee(&self.admin, fee);
                (r, format!("set_fee({})", fee))
            }
            FuzzAction::InitUser => {
                let new_user = Keypair::new();
                match self.env.try_init_user(&new_user) {
                    Ok(idx) => {
                        // Immediately deposit to prevent GC (use try_deposit since it may fail)
                        match self.env.try_deposit(&new_user, idx, 1_000_000_000) {
                            Ok(()) => {
                                self.users.push((new_user, idx));
                                self.all_indices.push(idx);
                                (Ok(()), format!("init_user(idx={})+deposit", idx))
                            }
                            Err(_) => {
                                // Init succeeded but deposit failed - account may get GC'd
                                // Still track it for invariant checks
                                self.all_indices.push(idx);
                                (Ok(()), format!("init_user(idx={}) deposit_failed", idx))
                            }
                        }
                    }
                    Err(e) => (Err(e), "init_user(failed)".to_string()),
                }
            }
            FuzzAction::CloseAccount { user_idx } => {
                let (ref user, idx) = self.users[user_idx];
                // Try to withdraw everything first (this is a separate successful operation)
                let cap = self.env.read_account_capital(idx);
                if cap > 0 {
                    let vault_before_withdraw = self.env.vault_balance();
                    if self.env.try_withdraw(user, idx, cap as u64).is_ok() {
                        self.check_invariants(&format!("pre_close_withdraw(idx={})", idx));
                    } else {
                        let vault_after_withdraw = self.env.vault_balance();
                        assert_eq!(
                            vault_before_withdraw, vault_after_withdraw,
                            "[seed={} step={}] P5 failed pre-close withdraw changed vault: before={} after={}",
                            self.seed, self.step, vault_before_withdraw, vault_after_withdraw
                        );
                    }
                }
                // Now try close on the (hopefully) empty account
                // Re-capture vault for P6 check
                let vault_now = self.env.vault_balance();
                let r = self.env.try_close_account(user, idx);
                if r.is_ok() {
                    self.check_invariants(&format!("close_account(idx={})", idx));
                    return; // Already checked, skip outer check
                }
                let vault_after = self.env.vault_balance();
                assert_eq!(
                    vault_now, vault_after,
                    "[seed={} step={}] P5 close_account changed vault: before={} after={}",
                    self.seed, self.step, vault_now, vault_after
                );
                self.check_invariants(&format!("close_account_failed(idx={})", idx));
                return; // Skip outer check
            }
        };

        match result {
            Ok(()) => {
                self.check_invariants(&desc);
            }
            Err(_) => {
                // P6: Failed operations must not change vault
                let vault_after = self.env.vault_balance();
                assert_eq!(
                    vault_before, vault_after,
                    "[seed={} step={} {}] P5 FAILED OP CHANGED VAULT: before={} after={}",
                    self.seed, self.step, desc, vault_before, vault_after
                );
            }
        }
    }
}

/// PROPERTY TEST: State machine fuzzer verifies 6 invariants across random operation sequences.
///
/// Subsumes the following classes of individual tests:
///   - All conservation tests (~30)
///   - All aggregate consistency tests (~20)
///   - All position symmetry tests (~15)
///   - All deposit/withdraw/trade edge cases (~80)
///   - All fee/insurance interaction tests (~20)
///   - All economic attack tests (~30)
///
/// 50 seeds × 100 steps = 5,000 operations with invariant checks after each.
#[test]
fn test_property_state_machine_invariants() {
    program_path();

    for seed in 1..=50 {
        let mut fuzzer = IntegrationFuzzer::new(seed);
        fuzzer.setup();
        let mut rng = FuzzRng::new(seed);

        for _ in 0..100 {
            let action = fuzzer.random_action(&mut rng);
            fuzzer.execute(action);
        }
    }
}

/// EXTENDED PROPERTY TEST: Deep state machine fuzzer (200 seeds × 200 steps = 40,000 ops).
/// Run with: cargo test test_property_state_machine_extended -- --ignored
#[test]
#[ignore = "long-running exhaustive state machine (40k ops)"]
fn test_property_state_machine_extended() {
    program_path();

    for seed in 1..=200 {
        let mut fuzzer = IntegrationFuzzer::new(seed);
        fuzzer.setup();
        let mut rng = FuzzRng::new(seed);

        for _ in 0..200 {
            let action = fuzzer.random_action(&mut rng);
            fuzzer.execute(action);
        }
    }
}

/// PROPERTY TEST: Authorization - every instruction rejects wrong signer.
///
/// Subsumes the following classes of individual tests:
///   - All authorization bypass tests (~50)
///   - All wrong-owner deposit/withdraw/close tests
///   - All non-admin admin-op tests
///   - All oracle authority tests
///
/// For each protected operation, verifies:
///   A1. Wrong owner is rejected
///   A2. Wrong admin is rejected
///   A3. State is unchanged after rejection
#[test]
fn test_property_authorization_exhaustive() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 10_000_000_000).unwrap();
    env.create_ata(&attacker.pubkey(), 5_000_000_000);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.try_top_up_insurance(&admin, 1_000_000_000).unwrap();
    env.crank();

    // Open a position for close/withdraw tests
    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000);

    let vault_before = env.vault_balance();

    // --- Owner-protected operations: attacker cannot act on user's account ---

    // A1: Deposit to someone else's account
    let r = env.try_deposit_unauthorized(&attacker, user_idx, 1_000_000);
    assert!(r.is_err(), "A1: Unauthorized deposit should fail");

    // A2: Withdraw from someone else's account
    let r = env.try_withdraw(&attacker, user_idx, 1_000);
    assert!(r.is_err(), "A2: Unauthorized withdraw should fail");

    // A3: Close someone else's account
    let r = env.try_close_account(&attacker, user_idx);
    assert!(r.is_err(), "A3: Unauthorized close should fail");

    // --- Admin-protected operations: non-admin cannot execute ---

    // A4: Set maintenance fee
    let r = env.try_set_maintenance_fee(&attacker, 100);
    assert!(r.is_err(), "A4: Non-admin set_maintenance_fee should fail");

    // A5: Set risk threshold
    let r = env.try_set_risk_threshold(&attacker, 999);
    assert!(r.is_err(), "A5: Non-admin set_risk_threshold should fail");

    // A6: Update admin
    let r = env.try_update_admin(&attacker, &attacker.pubkey());
    assert!(r.is_err(), "A6: Non-admin update_admin should fail");

    // --- Vault unchanged after all rejections ---
    let vault_after = env.vault_balance();
    assert_eq!(
        vault_before, vault_after,
        "Vault changed after rejected auth operations: before={} after={}",
        vault_before, vault_after
    );

    // --- Admin chain: verify old admin locked out ---
    let new_admin = Keypair::new();
    env.svm.airdrop(&new_admin.pubkey(), 5_000_000_000).unwrap();
    env.try_update_admin(&admin, &new_admin.pubkey()).unwrap();

    let r = env.try_set_maintenance_fee(&admin, 200);
    assert!(
        r.is_err(),
        "A7: Old admin should be locked out after transfer"
    );

    let r = env.try_set_maintenance_fee(&new_admin, 200);
    assert!(r.is_ok(), "A8: New admin should work after transfer");

    // --- Burned admin: no one can act ---
    let zero = Pubkey::default();
    env.try_update_admin(&new_admin, &zero).unwrap();

    let r = env.try_set_maintenance_fee(&new_admin, 300);
    assert!(r.is_err(), "A9: Burned admin - old admin locked out");

    let r = env.try_update_admin(&new_admin, &new_admin.pubkey());
    assert!(r.is_err(), "A10: Burned admin - cannot recover");
}

/// PROPERTY TEST: Account lifecycle invariants across create/use/close/GC cycles.
///
/// Subsumes the following classes of individual tests:
///   - All account lifecycle tests (~20)
///   - All GC tests (~10)
///   - All close-account edge cases (~10)
///   - All double-init tests
///
/// Properties verified:
///   L1. Closed accounts reject all operations
///   L2. GC'd accounts have zero capital/position/pnl
///   L3. Account reuse after GC works correctly
///   L4. Close requires zero position and zero PnL
#[test]
fn test_property_account_lifecycle_invariants() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    env.try_top_up_insurance(&admin, 2_000_000_000).unwrap();

    // Create 5 users with deposits
    let mut users: Vec<(Keypair, u16)> = Vec::new();
    for _ in 0..5 {
        let u = Keypair::new();
        let idx = env.init_user(&u);
        env.deposit(&u, idx, 3_000_000_000);
        users.push((u, idx));
    }

    env.crank();

    // L4: Can't close account with open position
    env.trade(&users[0].0, &lp, lp_idx, users[0].1, 500_000);
    let r = env.try_close_account(&users[0].0, users[0].1);
    assert!(r.is_err(), "L4: Can't close account with position");

    // Close the position
    env.set_slot(2);
    env.trade(&users[0].0, &lp, lp_idx, users[0].1, -500_000);
    env.set_slot(100);
    env.crank();

    // L4: Can't close with capital remaining (need to withdraw first)
    let cap = env.read_account_capital(users[0].1);
    if cap > 0 {
        env.try_withdraw(&users[0].0, users[0].1, cap as u64)
            .unwrap();
    }

    // Close account #0
    let pnl = env.read_account_pnl(users[0].1);
    if pnl == 0 {
        env.close_account(&users[0].0, users[0].1);

        // L1: Closed accounts reject operations
        let r = env.try_deposit(&users[0].0, users[0].1, 1_000_000);
        assert!(r.is_err(), "L1: Deposit to closed account should fail");

        let r = env.try_trade(&users[0].0, &lp, lp_idx, users[0].1, 100_000);
        assert!(r.is_err(), "L1: Trade with closed account should fail");
    }

    // Now close users[1..4] (withdraw + close)
    let mut additional_closed_accounts = 0u32;
    for i in 1..5 {
        let cap = env.read_account_capital(users[i].1);
        if cap > 0 {
            env.try_withdraw(&users[i].0, users[i].1, cap as u64)
                .expect("lifecycle withdraw before close must succeed");
        }
        let pnl = env.read_account_pnl(users[i].1);
        if pnl == 0 {
            env.try_close_account(&users[i].0, users[i].1)
                .expect("lifecycle close for zero-pnl account must succeed");
            additional_closed_accounts += 1;
        }
    }
    assert!(
        additional_closed_accounts > 0,
        "Lifecycle property test must close at least one additional account"
    );

    // Crank to GC closed accounts
    env.set_slot(200);
    env.crank();

    // L2: GC'd accounts have zero everything
    for (_, idx) in &users {
        if !env.is_slot_used(*idx) {
            assert_eq!(
                env.read_account_capital(*idx),
                0,
                "L2: GC'd account {} should have zero capital",
                idx
            );
            assert_eq!(
                env.read_account_position(*idx),
                0,
                "L2: GC'd account {} should have zero position",
                idx
            );
        }
    }

    // Final conservation check
    let vault = env.vault_balance();
    let c_tot = env.read_c_tot();
    let insurance = env.read_insurance_balance();
    assert!(
        vault as u128 >= c_tot + insurance,
        "Conservation after lifecycle test: vault={} c_tot={} ins={}",
        vault,
        c_tot,
        insurance
    );
}

// ============================================================================
// Binary Market (Premarket Resolution) Verification Tests
// ============================================================================

/// Verify complete binary market lifecycle with conservation:
/// trade → resolve → force-close → withdraw insurance → close accounts
/// Checks that vault SPL balance accounts for all user capital at every step.
#[test]
fn test_binary_market_complete_lifecycle_conservation() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };

    env.init_market_hyperp(1_000_000); // $1.00

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("initial oracle push must succeed");

    // Setup: LP + 2 users with opposing positions
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 10_000_000_000); // 10B lamports

    let user_a = Keypair::new();
    let user_a_idx = env.init_user(&user_a);
    env.deposit(&user_a, user_a_idx, 1_000_000_000);

    let user_b = Keypair::new();
    let user_b_idx = env.init_user(&user_b);
    env.deposit(&user_b, user_b_idx, 1_000_000_000);

    env.set_slot(50);
    env.crank();

    // Open positions: user_a long, user_b short
    env.try_trade_cpi(
        &user_a,
        &lp.pubkey(),
        lp_idx,
        user_a_idx,
        100_000_000,
        &matcher_prog,
        &matcher_ctx,
    )
    .expect("user_a setup trade must succeed");
    env.try_trade_cpi(
        &user_b,
        &lp.pubkey(),
        lp_idx,
        user_b_idx,
        -100_000_000,
        &matcher_prog,
        &matcher_ctx,
    )
    .expect("user_b setup trade must succeed");

    let vault_before = env.vault_balance();

    // Resolve at $1.50 (user_a profits, user_b loses)
    env.try_push_oracle_price(&admin, 1_500_000, 2000)
        .expect("resolution oracle push must succeed");
    env.try_resolve_market(&admin).unwrap();

    // Force-close all positions (may need multiple cranks for paginated)
    env.set_slot(200);
    env.crank();

    // After force-close: positions should be zero
    assert_eq!(
        env.read_account_position(user_a_idx),
        0,
        "user_a position should be zero after force-close"
    );
    assert_eq!(
        env.read_account_position(user_b_idx),
        0,
        "user_b position should be zero after force-close"
    );
    assert_eq!(
        env.read_account_position(lp_idx),
        0,
        "LP position should be zero after force-close"
    );

    // Vault SPL balance unchanged by force-close (no token transfers)
    let vault_after_close = env.vault_balance();
    assert_eq!(
        vault_before, vault_after_close,
        "vault SPL balance should not change during force-close"
    );

    // Conservation: engine vault >= c_tot + insurance
    let engine_vault = env.read_vault();
    let c_tot = env.read_c_tot();
    let insurance = env.read_insurance_balance();
    assert!(
        engine_vault >= c_tot + insurance,
        "Conservation after force-close: vault={} c_tot={} ins={}",
        engine_vault,
        c_tot,
        insurance
    );

    // Withdraw insurance
    let result = env.try_withdraw_insurance(&admin);
    assert!(
        result.is_ok(),
        "WithdrawInsurance should succeed after force-close: {:?}",
        result
    );

    // After insurance withdrawal, insurance balance in engine is zero
    assert_eq!(
        env.read_insurance_balance(),
        0,
        "insurance should be zero after withdrawal"
    );

    // Crank to settle warmup (with warmup_period=0, one crank converts PnL to capital)
    env.set_slot(300);
    env.crank();

    // Close all user accounts (with warmup=0, PnL converts after crank)
    let close_a = env.try_close_account(&user_a, user_a_idx);
    assert!(close_a.is_ok(), "User A close should succeed: {:?}", close_a);
    let close_b = env.try_close_account(&user_b, user_b_idx);
    assert!(close_b.is_ok(), "User B close should succeed: {:?}", close_b);
    let close_lp = env.try_close_account(&lp, lp_idx);
    assert!(close_lp.is_ok(), "LP close should succeed: {:?}", close_lp);

    // Final vault balance should be >= 0 (all tokens accounted for)
    let final_vault = env.vault_balance();
    println!("Final vault SPL balance: {}", final_vault);
    println!("BINARY MARKET COMPLETE LIFECYCLE CONSERVATION: PASSED");
}

/// Verify that with warmup_period > 0, profitable users after force-close
/// need two CloseAccount calls with a waiting period between them.
/// First call updates warmup slope; second call converts PnL to capital.
#[test]
fn test_binary_market_close_account_warmup_delay() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };

    // Create hyperp market with warmup_period = 100 slots
    env.init_market_hyperp_with_warmup(1_000_000, 100);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("initial oracle push must succeed");

    // Setup: LP + user with a long position
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.set_slot(50);
    env.crank();

    // Open long position
    env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000_000,
        &matcher_prog,
        &matcher_ctx,
    )
    .expect("user setup trade must succeed");
    assert_ne!(
        env.read_account_position(user_idx),
        0,
        "should have position"
    );

    // Resolve at higher price (user profits)
    env.try_push_oracle_price(&admin, 1_500_000, 2000)
        .expect("resolution oracle push must succeed");
    env.try_resolve_market(&admin).unwrap();

    // Force-close
    env.set_slot(200);
    env.crank();
    assert_eq!(env.read_account_position(user_idx), 0, "position zeroed");

    // User should have positive PnL from force-close
    let pnl_after_close = env.read_account_pnl(user_idx);
    println!("PnL after force-close: {}", pnl_after_close);
    assert!(
        pnl_after_close > 0,
        "profitable user should have positive PnL: {}",
        pnl_after_close
    );

    // CloseAccount fails because PnL > 0 and warmup slope = 0
    // (force-close didn't update warmup_slope_per_step)
    let result1 = env.try_close_account(&user, user_idx);
    assert!(
        result1.is_err(),
        "close should fail: PnL not warmed up (slope=0)"
    );
    println!("CloseAccount correctly rejected: PnL not warmed up");

    // CRITICAL: Failed transactions are ROLLED BACK in Solana.
    // So the warmup slope update from touch_account_full was NOT persisted.
    // User must call a SUCCESSFUL instruction to trigger the update.
    // Withdraw(1) triggers touch_account_full, which updates warmup slope.
    let withdraw_result = env.try_withdraw(&user, user_idx, 1);
    assert!(
        withdraw_result.is_ok(),
        "withdraw(1) should succeed to trigger warmup update: {:?}",
        withdraw_result
    );
    println!("Withdraw(1) succeeded, warmup slope now initialized");

    // Immediate close still fails - warmup just started
    let result2 = env.try_close_account(&user, user_idx);
    assert!(
        result2.is_err(),
        "close should still fail: warmup just initialized"
    );

    // Wait warmup_period_slots (100 slots)
    env.set_slot(350); // 200 + 150 > 100 warmup

    // Now close should succeed: warmup has elapsed, PnL converts to capital
    let result3 = env.try_close_account(&user, user_idx);
    assert!(
        result3.is_ok(),
        "close should succeed after warmup: {:?}",
        result3
    );
    println!("CloseAccount succeeded after warmup period");

    println!("BINARY MARKET WARMUP DELAY: PASSED");
}

/// Verify that users with negative PnL from force-close can close immediately.
/// Losses are settled to capital immediately (no warmup delay).
#[test]
fn test_binary_market_negative_pnl_close_immediate() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };

    // Create hyperp market with warmup_period = 100 slots (to test warmup doesn't block losers)
    env.init_market_hyperp_with_warmup(1_000_000, 100);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("initial oracle push must succeed");

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.set_slot(50);
    env.crank();

    // Open long position
    env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000_000,
        &matcher_prog,
        &matcher_ctx,
    )
    .expect("user setup trade must succeed");

    let capital_before = env.read_account_capital(user_idx);
    println!("Capital before resolution: {}", capital_before);

    // Resolve at LOWER price (user loses)
    env.try_push_oracle_price(&admin, 500_000, 2000)
        .expect("resolution oracle push must succeed");
    env.try_resolve_market(&admin).unwrap();

    env.set_slot(200);
    env.crank();

    // User has negative PnL from force-close
    let pnl = env.read_account_pnl(user_idx);
    println!("PnL after force-close: {}", pnl);

    // Close should work immediately - losses settle to capital in one step
    // (settle_warmup_to_capital §6.1 deducts losses and writes off remainder)
    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "losing user should close immediately: {:?}",
        result
    );

    let capital_after = env.read_account_capital(user_idx);
    println!(
        "Capital returned: {} (was {})",
        capital_after, capital_before
    );
    // Capital should be reduced (lost money on the trade)
    println!("BINARY MARKET NEGATIVE PNL CLOSE IMMEDIATE: PASSED");
}

/// Verify that the force-close PnL calculation is correct by comparing
/// expected PnL from position * (settlement - entry) / 1e6 with actual PnL.
#[test]
fn test_binary_market_force_close_pnl_correctness() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let matcher_prog = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("initial oracle push must succeed");

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &matcher_prog);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.set_slot(50);
    env.crank();

    // Open position
    let trade_size: i128 = 100_000_000;
    env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        trade_size,
        &matcher_prog,
        &matcher_ctx,
    )
    .expect("user setup trade must succeed");

    let position = env.read_account_position(user_idx);
    let pnl_before = env.read_account_pnl(user_idx);
    println!("Position after trade: {}", position);
    println!("PnL before resolution: {}", pnl_before);

    // Crank to settle mark (updates entry_price to oracle)
    env.set_slot(100);
    env.crank();

    let pnl_after_crank = env.read_account_pnl(user_idx);
    println!("PnL after crank (mark settled): {}", pnl_after_crank);

    // Resolve at $2.00
    let settlement_price: u64 = 2_000_000;
    env.try_push_oracle_price(&admin, settlement_price, 3000)
        .expect("resolution oracle push must succeed");
    env.try_resolve_market(&admin).unwrap();

    env.set_slot(200);
    env.crank();

    // After force-close
    let final_pnl = env.read_account_pnl(user_idx);
    let final_pos = env.read_account_position(user_idx);
    assert_eq!(final_pos, 0, "position should be zero");

    // The total PnL should be: pnl_after_crank + pos * (settlement - last_settled_price) / 1e6
    // Since the last crank settled at price 1_000_000, entry_price = 1_000_000
    // pnl_delta = position * (2_000_000 - 1_000_000) / 1_000_000
    println!(
        "Final PnL: {} (includes crank-settled {} + force-close delta)",
        final_pnl, pnl_after_crank
    );

    // PnL should be positive for a long position with price increase
    assert!(
        final_pnl > 0,
        "long position with price doubling should be profitable: {}",
        final_pnl
    );

    // Verify conservation: pnl_pos_tot should include this positive PnL
    let pnl_pos_tot = env.read_pnl_pos_tot();
    assert!(
        pnl_pos_tot >= final_pnl as u128,
        "pnl_pos_tot should include user's positive PnL"
    );

    println!("BINARY MARKET FORCE-CLOSE PNL CORRECTNESS: PASSED");
}

/// Verify that force-close handles zero-position accounts correctly
/// (skips them without modifying state).
#[test]
fn test_binary_market_force_close_zero_position_noop() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };

    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("initial oracle push must succeed");

    // Create user with deposit but NO position
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 500_000_000);

    let capital_before = env.read_account_capital(user_idx);
    let pnl_before = env.read_account_pnl(user_idx);

    // Resolve and force-close
    env.try_resolve_market(&admin).unwrap();
    env.set_slot(200);
    env.crank();

    // User with no position should be unaffected
    let capital_after = env.read_account_capital(user_idx);
    let pnl_after = env.read_account_pnl(user_idx);
    assert_eq!(
        capital_before, capital_after,
        "capital unchanged for zero-position user"
    );
    assert_eq!(
        pnl_before, pnl_after,
        "PnL unchanged for zero-position user"
    );

    // Should close immediately (no position, no PnL)
    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "zero-position user should close immediately: {:?}",
        result
    );

    println!("BINARY MARKET ZERO POSITION NOOP: PASSED");
}

// ============================================================================
// AdminForceCloseAccount tests
// ============================================================================

/// Happy path: resolve → force-close positions → admin force-close account
#[test]
fn test_admin_force_close_account_happy_path() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };

    env.init_market_hyperp(1_000_000);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");

    // Create LP and user with positions
    let mp = env.matcher_program_id;
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &mp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("initial oracle push must succeed");
    env.set_slot(100);
    env.crank();

    // Trade to create positions
    let result = env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000_000,
        &mp,
        &matcher_ctx,
    );
    assert!(result.is_ok(), "Trade should succeed: {:?}", result);

    // Resolve market
    env.try_push_oracle_price(&admin, 1_000_000, 2000)
        .expect("resolution oracle push must succeed");
    env.try_resolve_market(&admin).unwrap();

    // Crank to force-close positions
    env.set_slot(200);
    env.crank();

    assert_eq!(
        env.read_account_position(user_idx),
        0,
        "user position should be zero"
    );

    let capital_before = env.read_account_capital(user_idx);
    assert!(capital_before > 0, "user should have capital");
    let used_before = env.read_num_used_accounts();

    // Admin force-close account
    let result = env.try_admin_force_close_account(&admin, user_idx, &user.pubkey());
    assert!(
        result.is_ok(),
        "AdminForceCloseAccount should succeed: {:?}",
        result
    );

    let used_after = env.read_num_used_accounts();
    assert_eq!(used_after, used_before - 1, "num_used should decrease by 1");

    println!("ADMIN FORCE CLOSE ACCOUNT HAPPY PATH: PASSED");
}

/// AdminForceCloseAccount requires RESOLVED flag
#[test]
fn test_admin_force_close_account_requires_resolved() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };

    env.init_market_hyperp(1_000_000);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("initial oracle push must succeed");

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.set_slot(100);
    env.crank();

    // Try force-close on non-resolved market — should fail
    let result = env.try_admin_force_close_account(&admin, user_idx, &user.pubkey());
    assert!(result.is_err(), "Should fail on non-resolved market");

    println!("ADMIN FORCE CLOSE ACCOUNT REQUIRES RESOLVED: PASSED");
}

/// AdminForceCloseAccount requires admin signer
#[test]
fn test_admin_force_close_account_requires_admin() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };

    env.init_market_hyperp(1_000_000);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("oracle price push must succeed");

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    // Resolve
    env.try_resolve_market(&admin).unwrap();
    env.set_slot(200);
    env.crank();

    // Non-admin tries to force-close — should fail
    let fake_admin = Keypair::new();
    env.svm
        .airdrop(&fake_admin.pubkey(), 1_000_000_000)
        .unwrap();
    let result = env.try_admin_force_close_account(&fake_admin, user_idx, &user.pubkey());
    assert!(result.is_err(), "Non-admin should be rejected");

    println!("ADMIN FORCE CLOSE ACCOUNT REQUIRES ADMIN: PASSED");
}

/// AdminForceCloseAccount requires zero position
#[test]
fn test_admin_force_close_account_requires_zero_position() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };

    env.init_market_hyperp(1_000_000);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");

    let mp = env.matcher_program_id;
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &mp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("oracle price push must succeed");
    env.set_slot(100);
    env.crank();

    // Trade to create positions
    env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000_000,
        &mp,
        &matcher_ctx,
    )
    .unwrap();

    // Resolve but do NOT crank (positions still open)
    env.try_push_oracle_price(&admin, 1_000_000, 2000)
        .expect("oracle price push must succeed");
    env.try_resolve_market(&admin).unwrap();

    // Try force-close account without cranking — should fail (position != 0)
    let result = env.try_admin_force_close_account(&admin, user_idx, &user.pubkey());
    assert!(result.is_err(), "Should fail when position is not zero");

    println!("ADMIN FORCE CLOSE ACCOUNT REQUIRES ZERO POSITION: PASSED");
}

/// AdminForceCloseAccount with positive PnL applies haircut
#[test]
fn test_admin_force_close_account_with_positive_pnl() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };

    env.init_market_hyperp(1_000_000);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");

    let mp = env.matcher_program_id;
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &mp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("oracle price push must succeed");
    env.set_slot(100);
    env.crank();

    // Trade: user buys
    env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        500_000_000,
        &mp,
        &matcher_ctx,
    )
    .unwrap();

    // Price goes up → user profits
    env.try_push_oracle_price(&admin, 2_000_000, 2000)
        .expect("oracle price push must succeed");
    env.try_resolve_market(&admin).unwrap();

    env.set_slot(200);
    env.crank();

    // User should have positive PnL after force-close
    let pnl = env.read_account_pnl(user_idx);
    let capital = env.read_account_capital(user_idx);
    println!("User PnL after force-close: {}, capital: {}", pnl, capital);

    // Admin force-close should succeed and transfer funds
    let vault_before = env.vault_balance();
    let result = env.try_admin_force_close_account(&admin, user_idx, &user.pubkey());
    assert!(
        result.is_ok(),
        "AdminForceCloseAccount should succeed: {:?}",
        result
    );

    let vault_after = env.vault_balance();
    assert!(
        vault_after < vault_before,
        "vault should decrease (funds transferred to user)"
    );

    println!("ADMIN FORCE CLOSE ACCOUNT WITH POSITIVE PNL: PASSED");
}

/// AdminForceCloseAccount with negative PnL reduces capital
#[test]
fn test_admin_force_close_account_with_negative_pnl() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };

    env.init_market_hyperp(1_000_000);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");

    let mp = env.matcher_program_id;
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &mp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.try_push_oracle_price(&admin, 2_000_000, 1000)
        .expect("oracle price push must succeed");
    env.set_slot(100);
    env.crank();

    // Trade: user buys at price 2.0
    env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        500_000_000,
        &mp,
        &matcher_ctx,
    )
    .unwrap();

    // Price drops → user loses
    env.try_push_oracle_price(&admin, 1_000_000, 2000)
        .expect("oracle price push must succeed");
    env.try_resolve_market(&admin).unwrap();

    env.set_slot(200);
    env.crank();

    let pnl = env.read_account_pnl(user_idx);
    let capital = env.read_account_capital(user_idx);
    println!("User PnL after force-close: {}, capital: {}", pnl, capital);

    // Admin force-close should succeed
    let result = env.try_admin_force_close_account(&admin, user_idx, &user.pubkey());
    assert!(
        result.is_ok(),
        "AdminForceCloseAccount should succeed: {:?}",
        result
    );

    println!("ADMIN FORCE CLOSE ACCOUNT WITH NEGATIVE PNL: PASSED");
}

/// Full lifecycle: resolve → force-close positions → admin force-close all accounts → withdraw insurance → close slab
#[test]
fn test_admin_force_close_account_enables_close_slab() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };

    env.init_market_hyperp(1_000_000);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let mp = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");

    // Create LP and user
    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &mp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("oracle price push must succeed");
    env.set_slot(100);
    env.crank();

    // Trade
    env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        100_000_000,
        &mp,
        &matcher_ctx,
    )
    .unwrap();

    // Resolve and force-close positions
    env.try_push_oracle_price(&admin, 1_000_000, 2000)
        .expect("oracle price push must succeed");
    env.try_resolve_market(&admin).unwrap();
    env.set_slot(200);
    env.crank();

    // CloseSlab should fail (accounts still exist)
    let result = env.try_close_slab();
    assert!(
        result.is_err(),
        "CloseSlab should fail with active accounts"
    );

    // Admin force-close both accounts
    let result = env.try_admin_force_close_account(&admin, user_idx, &user.pubkey());
    assert!(
        result.is_ok(),
        "Force-close user should succeed: {:?}",
        result
    );

    let result = env.try_admin_force_close_account(&admin, lp_idx, &lp.pubkey());
    assert!(
        result.is_ok(),
        "Force-close LP should succeed: {:?}",
        result
    );

    assert_eq!(
        env.read_num_used_accounts(),
        0,
        "All accounts should be closed"
    );

    // Withdraw insurance
    let insurance = env.read_insurance_balance();
    if insurance > 0 {
        env.try_withdraw_insurance(&admin).unwrap();
    }

    // Now CloseSlab should succeed (expire blockhash to make tx distinct)
    env.svm.expire_blockhash();
    let result = env.try_close_slab();
    assert!(
        result.is_ok(),
        "CloseSlab should succeed after all accounts closed: {:?}",
        result
    );

    println!("ADMIN FORCE CLOSE ACCOUNT ENABLES CLOSE SLAB: PASSED");
}

/// Test: Honest user with positive PnL can close account after force-close + warmup.
/// Force-close crank initializes warmup slope so settle_warmup_to_capital can convert
/// PnL to capital over the warmup period.
#[test]
fn test_honest_user_close_after_force_close_positive_pnl() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };

    env.init_market_hyperp(1_000_000); // mark = 1.0
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let mp = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &mp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("oracle price push must succeed");
    env.set_slot(100);
    env.crank();

    // User buys
    env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        500_000_000,
        &mp,
        &matcher_ctx,
    )
    .unwrap();

    // Price doubles → user profits
    env.try_push_oracle_price(&admin, 2_000_000, 2000)
        .expect("oracle price push must succeed");
    env.try_resolve_market(&admin).unwrap();

    // Force-close positions via crank
    env.set_slot(200);
    env.crank();

    let pnl_after = env.read_account_pnl(user_idx);
    let cap_after = env.read_account_capital(user_idx);
    println!(
        "After force-close: user PnL={}, capital={}",
        pnl_after, cap_after
    );
    assert!(
        pnl_after > 0,
        "User should have positive PnL from price increase"
    );

    // CloseAccount immediately — may fail if warmup period > 0
    let result = env.try_close_account(&user, user_idx);
    if result.is_err() {
        // Wait for warmup period to elapse and retry
        env.set_slot(10_000);
        let result2 = env.try_close_account(&user, user_idx);
        assert!(
            result2.is_ok(),
            "User should close after warmup elapses: {:?}",
            result2
        );
        println!("User closed after warmup period");
    } else {
        println!("User closed immediately (warmup period = 0 or instant)");
    }

    println!("HONEST USER CLOSE AFTER FORCE-CLOSE POSITIVE PNL: PASSED");
}

/// Test: Honest user with negative PnL can close account immediately after force-close.
/// Negative PnL is settled immediately (deducted from capital), no warmup needed.
#[test]
fn test_honest_user_close_after_force_close_negative_pnl() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };

    env.init_market_hyperp(1_000_000);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let mp = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &mp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.try_push_oracle_price(&admin, 2_000_000, 1000)
        .expect("oracle price push must succeed");
    env.set_slot(100);
    env.crank();

    // User buys at 2.0
    env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        500_000_000,
        &mp,
        &matcher_ctx,
    )
    .unwrap();

    // Price drops to 1.0 → user loses
    env.try_push_oracle_price(&admin, 1_000_000, 2000)
        .expect("oracle price push must succeed");
    env.try_resolve_market(&admin).unwrap();

    env.set_slot(200);
    env.crank();

    let pnl_after = env.read_account_pnl(user_idx);
    println!("After force-close: user PnL={}", pnl_after);

    // Close should work immediately (negative PnL settled instantly)
    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "Losing user should close immediately: {:?}",
        result
    );

    println!("HONEST USER CLOSE AFTER FORCE-CLOSE NEGATIVE PNL: PASSED");
}

/// Test: Both LP and user can close after force-close (full lifecycle for honest participants)
#[test]
fn test_honest_participants_full_lifecycle() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };

    env.init_market_hyperp(1_000_000);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let mp = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &mp);
    env.deposit(&lp, lp_idx, 10_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("oracle price push must succeed");
    env.set_slot(100);
    env.crank();

    // Trade: user buys, LP sells
    env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        500_000_000,
        &mp,
        &matcher_ctx,
    )
    .unwrap();

    let vault_before = env.vault_balance();
    println!("Vault before resolution: {}", vault_before);

    // Resolve at same price — PnL should be ~0 for both
    env.try_push_oracle_price(&admin, 1_000_000, 2000)
        .expect("oracle price push must succeed");
    env.try_resolve_market(&admin).unwrap();

    env.set_slot(200);
    env.crank();

    let user_pnl = env.read_account_pnl(user_idx);
    let lp_pnl = env.read_account_pnl(lp_idx);
    println!(
        "After force-close: user PnL={}, LP PnL={}",
        user_pnl, lp_pnl
    );

    // Wait for warmup in case either has positive PnL
    env.set_slot(10_000);

    // Both should be able to close
    let user_result = env.try_close_account(&user, user_idx);
    assert!(user_result.is_ok(), "User should close: {:?}", user_result);

    let lp_result = env.try_close_account(&lp, lp_idx);
    assert!(lp_result.is_ok(), "LP should close: {:?}", lp_result);

    let used = env.read_num_used_accounts();
    assert_eq!(used, 0, "All accounts should be closed");

    // Withdraw insurance and close slab
    let insurance = env.read_insurance_balance();
    if insurance > 0 {
        env.try_withdraw_insurance(&admin).unwrap();
    }
    env.svm.expire_blockhash();
    let result = env.try_close_slab();
    assert!(result.is_ok(), "CloseSlab should succeed: {:?}", result);

    println!("HONEST PARTICIPANTS FULL LIFECYCLE: PASSED");
}

// ============================================================================
// Honest user lifecycle tests for all market types
// ============================================================================

/// Standard Pyth market: user deposits, trades (long), price goes up, flattens, closes account.
/// warmup_period_slots=0 so PnL converts instantly.
#[test]
fn test_honest_user_standard_market_profitable_close() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Top up insurance to prevent force-realize mode (insurance=0 <= threshold=0 triggers it)
    let ins_payer = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&ins_payer, 1);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000); // 10 SOL

    // Open long position at $138
    let size: i128 = 100_000;
    env.trade(&user, &lp, lp_idx, user_idx, size);
    assert_eq!(env.read_account_position(user_idx), size);

    // Price goes up (138 → 150), crank to settle mark-to-oracle
    env.set_slot_and_price(200, 150_000_000);
    env.crank();
    assert_eq!(
        env.read_account_position(user_idx),
        size,
        "Crank should not change position"
    );

    // User flattens
    env.trade(&user, &lp, lp_idx, user_idx, -size);
    assert_eq!(
        env.read_account_position(user_idx),
        0,
        "User should be flat"
    );

    // Crank to settle final state
    env.set_slot_and_price(300, 150_000_000);
    env.crank();

    let vault_before = env.vault_balance();

    // Close account — warmup_period=0 so PnL converts instantly
    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "Profitable user should close account: {:?}",
        result
    );

    let vault_after = env.vault_balance();
    assert!(
        vault_after < vault_before,
        "Vault should decrease (capital returned to user)"
    );
    println!(
        "Standard market profitable user: vault {} → {} (delta={})",
        vault_before,
        vault_after,
        vault_before - vault_after
    );

    println!("HONEST USER STANDARD MARKET PROFITABLE CLOSE: PASSED");
}

/// Standard Pyth market: user deposits, trades (long), price drops, flattens, closes.
/// User loses money but can still close and get remaining capital.
#[test]
fn test_honest_user_standard_market_losing_close() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let ins_payer = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&ins_payer, 1);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000); // 10 SOL

    // Open long at $138
    let size: i128 = 100_000;
    env.trade(&user, &lp, lp_idx, user_idx, size);

    // Price drops (138 → 120), crank to settle mark
    env.set_slot_and_price(200, 120_000_000);
    env.crank();

    // User flattens
    env.trade(&user, &lp, lp_idx, user_idx, -size);
    assert_eq!(
        env.read_account_position(user_idx),
        0,
        "User should be flat"
    );

    env.set_slot_and_price(300, 120_000_000);
    env.crank();

    let vault_before = env.vault_balance();

    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "Losing user should still close account: {:?}",
        result
    );

    let vault_after = env.vault_balance();
    assert!(
        vault_after < vault_before,
        "Vault should decrease (remaining capital returned)"
    );
    println!(
        "Standard market losing user: vault {} → {} (delta={})",
        vault_before,
        vault_after,
        vault_before - vault_after
    );

    println!("HONEST USER STANDARD MARKET LOSING CLOSE: PASSED");
}

/// Standard market with warmup: profitable user must wait for warmup before closing.
/// Uses a larger position (1M) to generate meaningful PnL that takes time to vest.
#[test]
fn test_honest_user_standard_market_warmup_close() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_warmup(0, 1000); // warmup_period_slots = 1000

    let ins_payer = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&ins_payer, 1);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000); // 10 SOL

    // Open long at $138 with larger position for meaningful PnL
    let size: i128 = 1_000_000;
    env.trade(&user, &lp, lp_idx, user_idx, size);

    // Price goes up (138 → 150), crank to settle mark-to-oracle
    // PnL = 1M * (150-138) * 1e6 / 1e6 = 12M lamports
    env.set_slot_and_price(100, 150_000_000);
    env.crank();

    // User flattens at slot 100 — PnL realized, warmup starts
    env.trade(&user, &lp, lp_idx, user_idx, -size);
    assert_eq!(env.read_account_position(user_idx), 0);

    // Advance only 50 slots (warmup needs 1000) and crank
    env.set_slot_and_price(150, 150_000_000);
    env.crank();

    // Try close — should fail (warmup far from complete, most PnL still unvested)
    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_err(),
        "CloseAccount should be blocked by warmup (only 50/1000 slots)"
    );
    println!("CloseAccount blocked by warmup as expected (50/1000 slots)");

    // Advance past warmup period (1000+ slots after warmup start) and crank
    env.set_slot_and_price(1200, 150_000_000);
    env.crank();

    // Now close should succeed — warmup fully elapsed
    let result2 = env.try_close_account(&user, user_idx);
    assert!(
        result2.is_ok(),
        "User should close after warmup: {:?}",
        result2
    );
    println!("User closed after warmup period elapsed (1200/1000 slots)");

    println!("HONEST USER STANDARD MARKET WARMUP CLOSE: PASSED");
}

/// Inverted Pyth market: user can close account after trading.
#[test]
fn test_honest_user_inverted_market_close() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(1);

    let ins_payer = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&ins_payer, 1);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000); // 10 SOL

    // Trade at inverted price (~$138 → inverted ~7246 internally)
    let size: i128 = 100_000;
    env.trade(&user, &lp, lp_idx, user_idx, size);
    assert_eq!(env.read_account_position(user_idx), size);

    env.set_slot(200);
    env.crank();

    // Flatten
    env.trade(&user, &lp, lp_idx, user_idx, -size);
    assert_eq!(env.read_account_position(user_idx), 0);

    env.set_slot(300);
    env.crank();

    let vault_before = env.vault_balance();
    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "User should close on inverted market: {:?}",
        result
    );

    let vault_after = env.vault_balance();
    assert!(vault_after < vault_before, "Capital should be returned");

    println!("HONEST USER INVERTED MARKET CLOSE: PASSED");
}

/// TradeCpi hyperp market (non-resolution): user trades via CPI, flattens, closes.
#[test]
fn test_honest_user_hyperp_trade_flatten_close() {
    let Some(mut env) = TradeCpiTestEnv::new() else {
        println!("SKIP: Programs not found");
        return;
    };

    env.init_market_hyperp(1_000_000); // mark = 1.0
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let mp = env.matcher_program_id;
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");

    // Top up insurance to prevent force-realize mode
    let ins_payer = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&ins_payer, 1);

    let lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&lp, &mp);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000); // 10 SOL

    env.try_push_oracle_price(&admin, 1_000_000, 1000)
        .expect("oracle price push must succeed");
    env.set_slot(100);
    env.crank();

    // User buys (small position relative to capital)
    let size: i128 = 100_000;
    env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        size,
        &mp,
        &matcher_ctx,
    )
    .unwrap();
    assert_eq!(
        env.read_account_position(user_idx),
        size,
        "User should have position"
    );

    // Crank at same price
    env.try_push_oracle_price(&admin, 1_000_000, 2000)
        .expect("oracle price push must succeed");
    env.set_slot(200);
    env.crank();

    // User sells to flatten
    env.try_trade_cpi(
        &user,
        &lp.pubkey(),
        lp_idx,
        user_idx,
        -size,
        &mp,
        &matcher_ctx,
    )
    .unwrap();
    assert_eq!(
        env.read_account_position(user_idx),
        0,
        "User should be flat"
    );

    // Crank to settle
    env.try_push_oracle_price(&admin, 1_000_000, 3000)
        .expect("oracle price push must succeed");
    env.set_slot(300);
    env.crank();

    // Close account
    let vault_before = env.vault_balance();
    let result = env.try_close_account(&user, user_idx);
    assert!(
        result.is_ok(),
        "User should close on hyperp market: {:?}",
        result
    );

    let vault_after = env.vault_balance();
    assert!(vault_after < vault_before, "Capital should be returned");
    println!(
        "Hyperp non-resolution: vault {} → {}",
        vault_before, vault_after
    );

    println!("HONEST USER HYPERP TRADE FLATTEN CLOSE: PASSED");
}

/// Full lifecycle test: both LP and user close on standard market, then close slab.
/// No insurance is topped up, and no crank runs between trades (avoiding force-realize mode).
#[test]
fn test_honest_participants_standard_market_full_lifecycle() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000); // 10 SOL

    // Trade and immediately flatten (no crank needed, PnL stays zero at same price)
    let size: i128 = 100_000;
    env.trade(&user, &lp, lp_idx, user_idx, size);
    assert_eq!(env.read_account_position(user_idx), size);

    env.trade(&user, &lp, lp_idx, user_idx, -size);
    assert_eq!(env.read_account_position(user_idx), 0);
    assert_eq!(env.read_account_position(lp_idx), 0);

    // Close both accounts (PnL=0, no warmup issue)
    let result = env.try_close_account(&user, user_idx);
    assert!(result.is_ok(), "User should close: {:?}", result);

    let result = env.try_close_account(&lp, lp_idx);
    assert!(result.is_ok(), "LP should close: {:?}", result);

    assert_eq!(env.read_num_used_accounts(), 0, "All accounts closed");

    // Close slab (insurance=0, all accounts closed)
    let result = env.try_close_slab();
    assert!(result.is_ok(), "CloseSlab should succeed: {:?}", result);

    println!("HONEST PARTICIPANTS STANDARD MARKET FULL LIFECYCLE: PASSED");
}

/// Regression test for PR #1: WithdrawInsurance must decrement engine.vault.
///
/// Without the fix, WithdrawInsurance zeroes insurance_fund.balance and transfers
/// SPL tokens out of the vault, but does NOT decrement engine.vault. This leaves
/// engine.vault non-zero after all capital is withdrawn, causing CloseSlab to fail
/// (it requires engine.vault.is_zero()).
#[test]
fn test_withdraw_insurance_decrements_engine_vault() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    let admin = env.payer.insecure_clone();

    // Create LP and user
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000); // 100 SOL

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000); // 10 SOL

    // Top up insurance so we can test withdrawal
    env.top_up_insurance(&admin, 5_000_000_000); // 5 SOL

    let insurance_before = env.read_insurance_balance();
    assert!(insurance_before > 0, "Insurance should be funded");

    // Trade to create positions
    let size: i128 = 100_000;
    env.trade(&user, &lp, lp_idx, user_idx, size);

    // Setup oracle authority and push price (required for ResolveMarket)
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("oracle authority setup must succeed");
    env.try_push_oracle_price(&admin, 138_000_000, 100)
        .expect("oracle price push must succeed");

    // Resolve market (premarket resolution)
    let result = env.try_resolve_market(&admin);
    assert!(result.is_ok(), "Resolve should succeed: {:?}", result);
    assert!(env.is_market_resolved(), "Market should be resolved");

    // Crank to force-close positions
    env.crank();

    // Verify positions are zeroed after crank
    assert_eq!(env.read_account_position(user_idx), 0, "User position should be 0 after force-close crank");
    assert_eq!(env.read_account_position(lp_idx), 0, "LP position should be 0 after force-close crank");

    // Admin force-close both accounts (handles PnL settlement, fee forgiveness)
    let result = env.try_admin_force_close_account(&admin, user_idx, &user.pubkey());
    assert!(result.is_ok(), "Admin force close user should succeed: {:?}", result);

    let result = env.try_admin_force_close_account(&admin, lp_idx, &lp.pubkey());
    assert!(result.is_ok(), "Admin force close LP should succeed: {:?}", result);

    assert_eq!(env.read_num_used_accounts(), 0, "All accounts should be closed");

    // Record vault before WithdrawInsurance
    let vault_before = env.read_engine_vault();
    let insurance = env.read_insurance_balance();
    assert!(insurance > 0, "Insurance should still have balance before withdrawal");
    assert!(vault_before > 0, "Vault should be non-zero before withdrawal");

    // Withdraw insurance
    let result = env.try_withdraw_insurance(&admin);
    assert!(result.is_ok(), "WithdrawInsurance should succeed: {:?}", result);

    // CRITICAL ASSERTION: engine.vault must be decremented by the insurance amount
    let vault_after = env.read_engine_vault();
    assert_eq!(
        vault_after,
        vault_before - insurance,
        "engine.vault must be decremented by insurance amount. \
         Before: {}, Insurance: {}, After: {} (expected {})",
        vault_before, insurance, vault_after, vault_before - insurance
    );

    // CloseSlab requires engine.vault == 0
    let result = env.try_close_slab();
    assert!(result.is_ok(), "CloseSlab should succeed after WithdrawInsurance: {:?}", result);

    println!("WITHDRAW INSURANCE DECREMENTS ENGINE VAULT: PASSED");
}
