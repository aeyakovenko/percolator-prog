//! BPF Compute Unit benchmark using LiteSVM
//!
//! Tests worst-case CU scenarios for keeper crank:
//! 1. All empty slots - baseline scan overhead
//! 2. All dust accounts - minimal balances, no positions
//! 3. Few liquidations - some accounts underwater
//! 4. All deeply underwater - socialized losses
//! 5. 4096 knife-edge liquidations - worst case
//!
//! Build BPF: cargo build-sbf (production) or cargo build-sbf --features test (small)
//! Run: cargo test --release --test cu_benchmark -- --nocapture

use litesvm::LiteSVM;
use solana_sdk::{
    account::Account,
    clock::Clock,
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction},
    program_pack::Pack,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    sysvar,
    transaction::Transaction,
};
use spl_token::state::{Account as TokenAccount, AccountState};
use std::path::PathBuf;
// Note: Can't read BPF slab from native - struct layouts differ:
// BPF SLAB_LEN: ~1.1MB, Native SLAB_LEN: ~1.2MB (even with repr(C) and same MAX_ACCOUNTS)

// SLAB_LEN / MAX_ACCOUNTS: production BPF values. The wrapper's `"test"`
// feature (which compiled the engine with MAX_ACCOUNTS=64 for native unit
// tests) has been removed; integration tests go through the BPF binary.
// BPF-target SLAB_LEN, cfg-gated by deployment-size feature.
#[cfg(all(feature = "small", not(feature = "medium")))]
const SLAB_LEN: usize = 111288;
#[cfg(all(feature = "medium", not(feature = "small")))]
const SLAB_LEN: usize = 440088;
#[cfg(not(any(feature = "small", feature = "medium")))]
const SLAB_LEN: usize = 1755288;
#[cfg(all(feature = "small", not(feature = "medium")))]
const MAX_ACCOUNTS: usize = 256;
#[cfg(all(feature = "medium", not(feature = "small")))]
const MAX_ACCOUNTS: usize = 1024;
#[cfg(not(any(feature = "small", feature = "medium")))]
const MAX_ACCOUNTS: usize = 4096;
#[cfg(all(feature = "small", not(feature = "medium")))]
const ENGINE_ACCOUNTS_OFFSET: usize = 2064;
#[cfg(all(feature = "medium", not(feature = "small")))]
const ENGINE_ACCOUNTS_OFFSET: usize = 5232;
#[cfg(not(any(feature = "small", feature = "medium")))]
const ENGINE_ACCOUNTS_OFFSET: usize = 17904;
const TEST_MAX_STALENESS_SECS: u64 = percolator_prog::constants::MAX_ORACLE_STALENESS_SECS;
const BENCHMARK_PERMISSIONLESS_RESOLVE_STALE_SLOTS: u64 = 10_000;

// Pyth Receiver program ID (rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ)
const PYTH_RECEIVER_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
    0x0c, 0xb7, 0xfa, 0xbb, 0x52, 0xf7, 0xa6, 0x48, 0xbb, 0x5b, 0x31, 0x7d, 0x9a, 0x01, 0x8b, 0x90,
    0x57, 0xcb, 0x02, 0x47, 0x74, 0xfa, 0xfe, 0x01, 0xe6, 0xc4, 0xdf, 0x98, 0xcc, 0x38, 0x58, 0x81,
]);

/// Default feed_id for CU benchmarks
const BENCHMARK_FEED_ID: [u8; 32] = [0xABu8; 32];

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
/// Layout: discriminator(8) + write_authority(32) + verification_level(2) + feed_id(32) +
///         price(8) + conf(8) + expo(4) + publish_time(8) + ...
fn cu_ix() -> Instruction {
    ComputeBudgetInstruction::set_compute_unit_limit(1_400_000)
}

fn make_pyth_data(
    feed_id: &[u8; 32],
    price: i64,
    expo: i32,
    conf: u64,
    publish_time: i64,
) -> Vec<u8> {
    let mut data = vec![0u8; 134];
    data[0..8].copy_from_slice(&[0x22, 0xf1, 0x23, 0x63, 0x9d, 0x7e, 0xf4, 0xcd]);
    // VerificationLevel::Full = 1-byte discriminant 0x01 at offset 40.
    // PriceFeedMessage begins at byte 41 (Borsh enum variants are
    // variable-size; Full has no payload).
    data[40] = 1;
    data[41..73].copy_from_slice(feed_id);
    data[73..81].copy_from_slice(&price.to_le_bytes());
    data[81..89].copy_from_slice(&conf.to_le_bytes());
    data[89..93].copy_from_slice(&expo.to_le_bytes());
    data[93..101].copy_from_slice(&publish_time.to_le_bytes());
    data
}

// Instruction encoders
fn encode_init_market_with_params(
    admin: &Pubkey,
    mint: &Pubkey,
    feed_id: &[u8; 32],
    risk_reduction_threshold: u128,
    warmup_period_slots: u64,
) -> Vec<u8> {
    let mut data = vec![0u8];
    data.extend_from_slice(admin.as_ref());
    data.extend_from_slice(mint.as_ref());
    data.extend_from_slice(feed_id);
    data.extend_from_slice(&TEST_MAX_STALENESS_SECS.to_le_bytes()); // max_staleness_secs
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(0u8); // invert (0 = no inversion)
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale (0 = no scaling)
    data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6 (0 for non-Hyperp markets)
                                                 // maintenance_fee_per_slot (0 = disabled). The v12.19 wire format dropped
                                                 // the standalone `min_oracle_price_cap_e2bps` field in favour of a
                                                 // RiskParams entry (see max_price_move_bps_per_slot below).
    data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee_per_slot
                                                  // RiskParams
    data.extend_from_slice(&warmup_period_slots.max(1).to_le_bytes()); // h_min
    data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps (5%)
    data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps (10%)
    data.extend_from_slice(&0u64.to_le_bytes()); // trading_fee_bps
    data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data.extend_from_slice(&1u128.to_le_bytes()); // new_account_fee (anti-spam floor)
    data.extend_from_slice(&warmup_period_slots.max(1).to_le_bytes()); // h_max (must be >= h_min)
    data.extend_from_slice(&50u64.to_le_bytes()); // legacy max_crank_staleness_slots wire field
    data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
    data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
    data.extend_from_slice(&1000u64.to_le_bytes()); // resolve_price_deviation_bps
    data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
    data.extend_from_slice(&21u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&22u128.to_le_bytes()); // min_nonzero_im_req
                                                   // v12.19 solvency envelope: 2 bps/slot * 100 = 200, funding 10_000 e9/slot *
                                                   // 100 * 10_000 / 1e9 = 10, liq_fee 50 — sum 260 <= maintenance_margin 500 OK.
    data.extend_from_slice(&2u64.to_le_bytes()); // max_price_move_bps_per_slot
    data.extend_from_slice(&0u16.to_le_bytes()); // insurance_withdraw_max_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // insurance_withdraw_cooldown_slots
                                                 // Keep the benchmark market live while thousands of setup trades are
                                                 // created, so dense crank measurements exercise crank CU rather than the
                                                 // hard stale gate.
    data.extend_from_slice(&BENCHMARK_PERMISSIONLESS_RESOLVE_STALE_SLOTS.to_le_bytes());
    data.extend_from_slice(&500u64.to_le_bytes()); // funding_horizon_slots
    data.extend_from_slice(&100u64.to_le_bytes()); // funding_k_bps
    data.extend_from_slice(&500i64.to_le_bytes()); // funding_max_premium_bps
    data.extend_from_slice(&1_000i64.to_le_bytes()); // funding_max_e9_per_slot
    data.extend_from_slice(&0u64.to_le_bytes()); // mark_min_fee
    data.extend_from_slice(&50u64.to_le_bytes()); // force_close_delay_slots (perm_resolve>0 ⇒ >0)
    data
}

fn encode_init_market(admin: &Pubkey, mint: &Pubkey, feed_id: &[u8; 32]) -> Vec<u8> {
    encode_init_market_with_params(admin, mint, feed_id, 0, 1)
}

fn encode_init_user(fee: u64) -> Vec<u8> {
    let mut data = vec![1u8];
    data.extend_from_slice(&fee.to_le_bytes());
    data
}

fn encode_init_lp(matcher: &Pubkey, ctx: &Pubkey, fee: u64) -> Vec<u8> {
    let mut data = vec![2u8];
    data.extend_from_slice(matcher.as_ref());
    data.extend_from_slice(ctx.as_ref());
    data.extend_from_slice(&fee.to_le_bytes());
    data
}

fn encode_deposit(user_idx: u16, amount: u64) -> Vec<u8> {
    let mut data = vec![3u8];
    data.extend_from_slice(&user_idx.to_le_bytes());
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

fn encode_crank_permissionless(_panic: u8) -> Vec<u8> {
    let mut data = vec![5u8];
    data.extend_from_slice(&u16::MAX.to_le_bytes());
    data.push(1u8); // format_version = 1
    data
}

fn encode_crank_with_candidates(candidates: &[u16]) -> Vec<u8> {
    let mut data = vec![5u8];
    data.extend_from_slice(&u16::MAX.to_le_bytes());
    data.push(1u8); // format_version = 1
    for &idx in candidates {
        data.extend_from_slice(&idx.to_le_bytes());
        data.push(0u8); // tag 0 = FullClose
    }
    data
}

fn encode_crank_with_touch_candidates(candidates: &[u16]) -> Vec<u8> {
    let mut data = vec![5u8];
    data.extend_from_slice(&u16::MAX.to_le_bytes());
    data.push(1u8); // format_version = 1
    for &idx in candidates {
        data.extend_from_slice(&idx.to_le_bytes());
        data.push(0xFFu8); // tag 0xFF = touch-only
    }
    data
}

fn encode_trade(lp: u16, user: u16, size: i128) -> Vec<u8> {
    let mut data = vec![6u8];
    data.extend_from_slice(&lp.to_le_bytes());
    data.extend_from_slice(&user.to_le_bytes());
    data.extend_from_slice(&size.to_le_bytes());
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
    account_count: u16,
}

impl TestEnv {
    fn new() -> Self {
        let path = program_path();
        if !path.exists() {
            panic!("BPF not found at {:?}. Run: cargo build-sbf", path);
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

        let pyth_data = make_pyth_data(&BENCHMARK_FEED_ID, 100_000_000, -6, 1, 100); // $100
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

    fn init_market(&mut self) {
        self.init_market_with_params(0, 1);
    }

    fn init_market_with_params(
        &mut self,
        risk_reduction_threshold: u128,
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

        // InitMarket requires a successful oracle read at init (no sentinel).
        let _ = dummy_ata;
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(self.mint, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_init_market_with_params(
                &admin.pubkey(),
                &self.mint,
                &BENCHMARK_FEED_ID,
                risk_reduction_threshold,
                warmup_period_slots,
            ),
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("init_market failed");
    }

    fn init_market_raw(&mut self, data: Vec<u8>) {
        let admin = &self.payer;
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(self.mint, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data,
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("init_market_raw failed");
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
        self.init_lp_with_fee(owner, 100)
    }

    fn init_lp_with_fee(&mut self, owner: &Keypair, fee: u64) -> u16 {
        let idx = self.account_count;
        self.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        let ata = self.create_ata(&owner.pubkey(), fee);
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
                AccountMeta::new_readonly(sysvar::clock::ID, false),
            ],
            data: encode_init_lp(&matcher, &ctx, fee),
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("init_lp failed");
        self.account_count += 1;
        idx
    }

    fn init_user(&mut self, owner: &Keypair) -> u16 {
        self.init_user_with_fee(owner, 100)
    }

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
            ],
            data: encode_init_user(fee),
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
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
            &[cu_ix(), ix],
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
            &[cu_ix(), ix],
            Some(&user.pubkey()),
            &[user, lp],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("trade failed");
    }

    fn crank(&mut self) -> u64 {
        self.crank_with_cu_limit(1_400_000)
    }

    fn crank_with_cu_limit(&mut self, cu_limit: u32) -> u64 {
        let caller = Keypair::new();
        self.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();

        let budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(cu_limit);

        let crank_ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(caller.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_crank_permissionless(0),
        };

        let tx = Transaction::new_signed_with_payer(
            &[budget_ix, crank_ix],
            Some(&caller.pubkey()),
            &[&caller],
            self.svm.latest_blockhash(),
        );
        let result = self.svm.send_transaction(tx).expect("crank failed");
        result.compute_units_consumed
    }

    fn set_price(&mut self, price_e6: i64, slot: u64) {
        // If a benchmark jumps beyond the hard stale window, chunk with
        // best-effort cranks so the harness keeps testing crank CU instead
        // of terminal market resolution.
        let current = self.svm.get_sysvar::<Clock>().slot;
        let max_live_step = BENCHMARK_PERMISSIONLESS_RESOLVE_STALE_SLOTS.saturating_sub(1);
        if max_live_step > 0 && slot > current.saturating_add(max_live_step) {
            let mut s = current;
            while s.saturating_add(max_live_step) < slot {
                s = s.saturating_add(max_live_step);
                self.write_price_and_clock(price_e6, s);
                let _ = self.try_crank();
            }
        }
        self.write_price_and_clock(price_e6, slot);
    }

    fn write_price_and_clock(&mut self, price_e6: i64, slot: u64) {
        self.svm.set_sysvar(&Clock {
            slot,
            unix_timestamp: slot as i64,
            ..Clock::default()
        });
        let pyth_data = make_pyth_data(&BENCHMARK_FEED_ID, price_e6, -6, 1, slot as i64);

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

    fn try_crank(&mut self) -> Result<(u64, Vec<String>), String> {
        let caller = Keypair::new();
        self.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();

        let budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);

        let crank_ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(caller.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_crank_permissionless(0),
        };

        let tx = Transaction::new_signed_with_payer(
            &[budget_ix, crank_ix],
            Some(&caller.pubkey()),
            &[&caller],
            self.svm.latest_blockhash(),
        );
        match self.svm.send_transaction(tx) {
            Ok(result) => Ok((result.compute_units_consumed, result.logs)),
            Err(e) => Err(format!("{:?}", e)),
        }
    }

    fn try_crank_with_candidates(
        &mut self,
        candidates: &[u16],
    ) -> Result<(u64, Vec<String>), String> {
        self.try_crank_with_data(encode_crank_with_candidates(candidates))
    }

    fn try_crank_with_touch_candidates(
        &mut self,
        candidates: &[u16],
    ) -> Result<(u64, Vec<String>), String> {
        self.try_crank_with_data(encode_crank_with_touch_candidates(candidates))
    }

    fn try_crank_with_data(&mut self, data: Vec<u8>) -> Result<(u64, Vec<String>), String> {
        let caller = Keypair::new();
        self.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();

        let budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);

        let crank_ix = Instruction {
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
            &[budget_ix, crank_ix],
            Some(&caller.pubkey()),
            &[&caller],
            self.svm.latest_blockhash(),
        );
        match self.svm.send_transaction(tx) {
            Ok(result) => Ok((result.compute_units_consumed, result.logs)),
            Err(e) => Err(format!("{:?}", e)),
        }
    }

    fn top_up_insurance(&mut self, funder: &Keypair, amount: u64) {
        let ata = self.create_ata(&funder.pubkey(), amount);

        // Instruction 9: TopUpInsurance { amount: u64 }
        let mut data = vec![9u8];
        data.extend_from_slice(&amount.to_le_bytes());

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(funder.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(ata, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
            ],
            data,
        };
        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&funder.pubkey()),
            &[funder],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("top_up_insurance failed");
    }

    fn read_last_market_slot(&self) -> u64 {
        let d = self.svm.get_account(&self.slab).unwrap().data;
        const LAST_MARKET_SLOT_OFFSET: usize = 520 + 928;
        u64::from_le_bytes(
            d[LAST_MARKET_SLOT_OFFSET..LAST_MARKET_SLOT_OFFSET + 8]
                .try_into()
                .unwrap(),
        )
    }

    fn read_rr_cursor_position(&self) -> u64 {
        let d = self.svm.get_account(&self.slab).unwrap().data;
        const RR_CURSOR_OFFSET: usize = 520 + 840;
        u64::from_le_bytes(
            d[RR_CURSOR_OFFSET..RR_CURSOR_OFFSET + 8]
                .try_into()
                .unwrap(),
        )
    }

    fn read_account_position(&self, idx: u16) -> i128 {
        let d = self.svm.get_account(&self.slab).unwrap().data;
        const ENGINE: usize = 520;
        const ACCOUNTS_OFFSET: usize = ENGINE + ENGINE_ACCOUNTS_OFFSET;
        const ACCOUNT_SIZE: usize = 416;
        const PBQ: usize = 56;
        const A_BASIS: usize = 72;
        const EPOCH_SNAP: usize = 120;
        const ADL_MULT_LONG: usize = ENGINE + 360;
        const ADL_MULT_SHORT: usize = ENGINE + 376;
        const ADL_EPOCH_LONG: usize = ENGINE + 424;
        const ADL_EPOCH_SHORT: usize = ENGINE + 432;

        let acc_off = ACCOUNTS_OFFSET + (idx as usize) * ACCOUNT_SIZE;
        if d.len() < acc_off + ACCOUNT_SIZE {
            return 0;
        }
        let basis = i128::from_le_bytes(d[acc_off + PBQ..acc_off + PBQ + 16].try_into().unwrap());
        if basis == 0 {
            return 0;
        }
        let a_basis = u128::from_le_bytes(
            d[acc_off + A_BASIS..acc_off + A_BASIS + 16]
                .try_into()
                .unwrap(),
        );
        let epoch_snap = u64::from_le_bytes(
            d[acc_off + EPOCH_SNAP..acc_off + EPOCH_SNAP + 8]
                .try_into()
                .unwrap(),
        );
        if a_basis == 0 {
            return 0;
        }
        let (a_side, epoch_side) = if basis > 0 {
            let a = u128::from_le_bytes(d[ADL_MULT_LONG..ADL_MULT_LONG + 16].try_into().unwrap());
            let e = u64::from_le_bytes(d[ADL_EPOCH_LONG..ADL_EPOCH_LONG + 8].try_into().unwrap());
            (a, e)
        } else {
            let a = u128::from_le_bytes(d[ADL_MULT_SHORT..ADL_MULT_SHORT + 16].try_into().unwrap());
            let e = u64::from_le_bytes(d[ADL_EPOCH_SHORT..ADL_EPOCH_SHORT + 8].try_into().unwrap());
            (a, e)
        };
        if epoch_snap != epoch_side {
            return 0;
        }
        let effective = if a_side == a_basis {
            basis.unsigned_abs()
        } else {
            basis.unsigned_abs().saturating_mul(a_side) / a_basis
        };
        if basis < 0 {
            -(effective as i128)
        } else {
            effective as i128
        }
    }
}

// --- Encode helpers for all instruction types ---

fn encode_withdraw(user_idx: u16, amount: u64) -> Vec<u8> {
    let mut data = vec![4u8];
    data.extend_from_slice(&user_idx.to_le_bytes());
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

fn encode_close_account(user_idx: u16) -> Vec<u8> {
    let mut data = vec![8u8];
    data.extend_from_slice(&user_idx.to_le_bytes());
    data
}

fn encode_top_up_insurance(amount: u64) -> Vec<u8> {
    let mut data = vec![9u8];
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

fn encode_update_admin(new_admin: &Pubkey) -> Vec<u8> {
    // UpdateAuthority { kind: AUTHORITY_ADMIN = 0, new_pubkey }
    let mut data = vec![32u8];
    data.push(0u8);
    data.extend_from_slice(new_admin.as_ref());
    data
}

fn encode_close_slab() -> Vec<u8> {
    vec![13u8]
}

fn encode_update_config(
    funding_horizon_slots: u64,
    funding_k_bps: u64,
    funding_max_premium_bps: i64,
    funding_max_e9_per_slot: i64,
) -> Vec<u8> {
    // UpdateConfig wire format in v12.18.1: tag (1) + 4 u64/i64 funding params.
    // Earlier revisions had trailing threshold fields; the decoder now rejects
    // them explicitly so the benchmark must not append them either.
    let mut data = vec![14u8];
    data.extend_from_slice(&funding_horizon_slots.to_le_bytes());
    data.extend_from_slice(&funding_k_bps.to_le_bytes());
    data.extend_from_slice(&funding_max_premium_bps.to_le_bytes());
    data.extend_from_slice(&funding_max_e9_per_slot.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes()); // tvl_insurance_cap_mult (disabled)
    data
}

fn encode_set_maintenance_fee(new_fee: u128) -> Vec<u8> {
    let mut data = vec![15u8];
    data.extend_from_slice(&new_fee.to_le_bytes());
    data
}

fn encode_set_oracle_authority(new_authority: &Pubkey) -> Vec<u8> {
    // UpdateAuthority { kind: AUTHORITY_HYPERP_MARK = 1, new_pubkey }
    let mut data = vec![32u8];
    data.push(1u8);
    data.extend_from_slice(new_authority.as_ref());
    data
}

fn encode_push_oracle_price(price_e6: u64, timestamp: i64) -> Vec<u8> {
    let mut data = vec![17u8];
    data.extend_from_slice(&price_e6.to_le_bytes());
    data.extend_from_slice(&timestamp.to_le_bytes());
    data
}

fn encode_resolve_market() -> Vec<u8> {
    // Ordinary mode (0); live oracle required.
    vec![19u8, 0u8]
}

fn encode_withdraw_insurance() -> Vec<u8> {
    vec![20u8]
}

fn encode_admin_force_close_account(user_idx: u16) -> Vec<u8> {
    let mut data = vec![21u8];
    data.extend_from_slice(&user_idx.to_le_bytes());
    data
}

fn create_users(env: &mut TestEnv, count: usize, deposit_amount: u64) -> Vec<Keypair> {
    let mut users = Vec::with_capacity(count);
    for i in 0..count {
        let user = Keypair::new();
        env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();
        let ata = env.create_ata(&user.pubkey(), 100);

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
            data: encode_init_user(100),
        };
        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&user.pubkey()),
            &[&user],
            env.svm.latest_blockhash(),
        );
        env.svm.send_transaction(tx).unwrap();

        let user_idx = (i + 1) as u16;
        env.deposit(&user, user_idx, deposit_amount);
        users.push(user);

        if (i + 1) % 500 == 0 {
            println!("    Created {} users...", i + 1);
        }
    }
    users
}

const DENSE_CRANK_P0_E6: u64 = 200_000_000;
const DENSE_CRANK_START_SLOT: u64 = 100;
const DENSE_CRANK_CAP_BPS: u64 = 49;
const DENSE_CRANK_CAPITAL_PER_USER: u64 = 1_000_000_000;
const DENSE_CRANK_LP_CAPITAL: u64 = 10_000_000_000_000;
const DENSE_CRANK_INSURANCE: u64 = 1_000_000_000;
const DENSE_CRANK_LEVERAGE_MILLI: u128 = 20_000;
const DENSE_CRANK_SETUP_BUFFER: u64 = 300_000_000;

fn encode_dense_crank_market(admin: &Pubkey, mint: &Pubkey) -> Vec<u8> {
    let mut data = vec![0u8];
    data.extend_from_slice(admin.as_ref());
    data.extend_from_slice(mint.as_ref());
    data.extend_from_slice(&BENCHMARK_FEED_ID);
    data.extend_from_slice(&600u64.to_le_bytes()); // max_staleness_secs
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(0); // invert
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
    data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6
    data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee_per_slot

    // Same high-risk shape used by the security probe: h_min=0 product mode,
    // 20x notional, and a 49 bps/slot cap. The benchmark advances by a capped
    // 40-slot move, then submits a saturated candidate tail to exercise the
    // dense keeper-crank CU envelope.
    data.extend_from_slice(&0u64.to_le_bytes()); // h_min
    data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps
    data.extend_from_slice(&500u64.to_le_bytes()); // initial_margin_bps
    data.extend_from_slice(&1u64.to_le_bytes()); // trading_fee_bps
    data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data.extend_from_slice(&1u128.to_le_bytes()); // new_account_fee
    data.extend_from_slice(&86_400u64.to_le_bytes()); // h_max
    data.extend_from_slice(&9u64.to_le_bytes()); // legacy max_crank_staleness_slots
    data.extend_from_slice(&5u64.to_le_bytes()); // liquidation_fee_bps
    data.extend_from_slice(&50_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
    data.extend_from_slice(&1_000u64.to_le_bytes()); // resolve_price_deviation_bps
    data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
    data.extend_from_slice(&500u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&600u128.to_le_bytes()); // min_nonzero_im_req
    data.extend_from_slice(&DENSE_CRANK_CAP_BPS.to_le_bytes());

    data.extend_from_slice(&0u16.to_le_bytes()); // insurance_withdraw_max_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // insurance_withdraw_cooldown_slots
    data.extend_from_slice(&BENCHMARK_PERMISSIONLESS_RESOLVE_STALE_SLOTS.to_le_bytes());
    data.extend_from_slice(&500u64.to_le_bytes()); // funding_horizon_slots
    data.extend_from_slice(&100u64.to_le_bytes()); // funding_k_bps
    data.extend_from_slice(&500i64.to_le_bytes()); // funding_max_premium_bps
    data.extend_from_slice(&1_000i64.to_le_bytes()); // funding_max_e9_per_slot
    data.extend_from_slice(&0u64.to_le_bytes()); // mark_min_fee
    data.extend_from_slice(&10u64.to_le_bytes()); // force_close_delay_slots
    data
}

fn dense_crank_position_size(capital: u64, price_e6: u64) -> i128 {
    let notional = (capital as u128) * DENSE_CRANK_LEVERAGE_MILLI / 1_000;
    let q = notional
        .saturating_mul(percolator::POS_SCALE)
        .checked_div(price_e6 as u128)
        .expect("nonzero price");
    q as i128
}

fn dense_crank_next_price_for_dt(price_e6: u64, signed_bps_per_slot: i16, dt_slots: u64) -> u64 {
    let abs_bps = signed_bps_per_slot.unsigned_abs() as u64;
    assert!(abs_bps <= DENSE_CRANK_CAP_BPS);
    let delta = (price_e6 as u128)
        .saturating_mul(abs_bps as u128)
        .saturating_mul(dt_slots as u128)
        / 10_000;
    if signed_bps_per_slot < 0 {
        price_e6.saturating_sub(delta as u64)
    } else {
        price_e6.saturating_add(delta as u64)
    }
}

fn count_open_positions(env: &TestEnv, indices: &[u16]) -> usize {
    indices
        .iter()
        .filter(|&&idx| env.read_account_position(idx) != 0)
        .count()
}

fn setup_dense_crank_market(
    num_actors: usize,
    balanced_after_candidate_cap: bool,
) -> (TestEnv, Vec<u16>) {
    assert!(num_actors + 1 <= MAX_ACCOUNTS);
    let mut env = TestEnv::new();
    env.write_price_and_clock(DENSE_CRANK_P0_E6 as i64, DENSE_CRANK_START_SLOT);
    let admin = env.payer.pubkey();
    let mint = env.mint;
    env.init_market_raw(encode_dense_crank_market(&admin, &mint));

    let lp = Keypair::new();
    let lp_idx = env.init_lp_with_fee(&lp, DENSE_CRANK_LP_CAPITAL + DENSE_CRANK_SETUP_BUFFER);
    let payer = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&payer, DENSE_CRANK_INSURANCE);

    let size = dense_crank_position_size(DENSE_CRANK_CAPITAL_PER_USER, DENSE_CRANK_P0_E6);
    let mut actors = Vec::with_capacity(num_actors);
    for i in 0..num_actors {
        let user = Keypair::new();
        let user_idx = env.init_user_with_fee(
            &user,
            DENSE_CRANK_CAPITAL_PER_USER + DENSE_CRANK_SETUP_BUFFER,
        );
        let direction = if balanced_after_candidate_cap
            && i >= percolator_prog::constants::MAX_KEEPER_CANDIDATES
        {
            -1i128
        } else {
            1i128
        };
        env.trade(&user, &lp, lp_idx, user_idx, size * direction);
        actors.push(user_idx);
    }
    (env, actors)
}

fn move_dense_crank_market_to_liquidation_round(env: &mut TestEnv) {
    let start_slot = env.read_last_market_slot();
    let target_dt_slots = 40;
    let target = dense_crank_next_price_for_dt(
        DENSE_CRANK_P0_E6,
        -(DENSE_CRANK_CAP_BPS as i16),
        target_dt_slots,
    );
    env.set_price(target as i64, start_slot + target_dt_slots);
}

fn saturated_open_candidate_prefix(env: &TestEnv, actors: &[u16]) -> Vec<u16> {
    actors
        .iter()
        .copied()
        .filter(|&idx| env.read_account_position(idx) != 0)
        .take(percolator_prog::constants::MAX_KEEPER_CANDIDATES)
        .collect()
}

#[test]
fn benchmark_keeper_crank_dense_touch_only_progress_is_fixed_size() {
    println!("\n=== KEEPER DENSE TOUCH-ONLY PROGRESS CU REGRESSION ===");
    let test_sizes: &[usize] = if MAX_ACCOUNTS >= 512 {
        &[128, 256, 512]
    } else {
        &[64, 128, 192]
    };
    let mut saturated_reference: Option<u64> = None;

    for &num_actors in test_sizes {
        let (mut env, actors) = setup_dense_crank_market(num_actors, true);
        move_dense_crank_market_to_liquidation_round(&mut env);

        let mut worst_cu = 0u64;
        for round in 0..4 {
            let before_slot = env.read_last_market_slot();
            let before_rr = env.read_rr_cursor_position();
            let before_open = count_open_positions(&env, &actors);
            let candidates = saturated_open_candidate_prefix(&env, &actors);
            let (cu, _logs) = env
                .try_crank_with_touch_candidates(&candidates)
                .expect("touch-only dense crank must not brick");
            assert!(
                cu <= 1_400_000,
                "touch-only crank exceeded SVM limit: actors={num_actors}, round={round}, cu={cu}"
            );
            worst_cu = worst_cu.max(cu);
            let after_slot = env.read_last_market_slot();
            let after_rr = env.read_rr_cursor_position();
            let after_open = count_open_positions(&env, &actors);
            assert!(
                after_slot > before_slot || after_rr != before_rr || after_open < before_open,
                "touch-only dense crank made no progress: actors={num_actors}, round={round}, cu={cu}, slot={before_slot}->{after_slot}, rr={before_rr}->{after_rr}, open={before_open}->{after_open}"
            );
            println!(
                "  actors={num_actors:>3}, round={round:>2}: cu={cu:>8}, open={before_open}->{after_open}, rr={before_rr}->{after_rr}, slot={before_slot}->{after_slot}"
            );
        }

        println!("  actors={num_actors:>3}: worst_cu={worst_cu:>8}");
        if num_actors == percolator_prog::constants::MAX_KEEPER_CANDIDATES {
            saturated_reference = Some(worst_cu);
        } else if let Some(reference) = saturated_reference {
            assert!(
                worst_cu <= reference + 250_000,
                "touch-only CU should remain fixed-size after candidate cap: cap_cu={reference}, actors={num_actors}, worst_cu={worst_cu}"
            );
        }
    }
}

#[test]
fn benchmark_keeper_crank_dense_fullclose_single_opposing_side_stays_bounded() {
    println!("\n=== KEEPER DENSE FULLCLOSE SINGLE-OPPOSING-SIDE CU REGRESSION ===");
    let (mut env, actors) =
        setup_dense_crank_market(percolator_prog::constants::MAX_KEEPER_CANDIDATES, false);
    move_dense_crank_market_to_liquidation_round(&mut env);

    let mut closed_total = 0usize;
    let mut worst_cu = 0u64;
    for round in 0..4 {
        let before_open = count_open_positions(&env, &actors);
        let before_slot = env.read_last_market_slot();
        let candidates = saturated_open_candidate_prefix(&env, &actors);
        let (cu, _logs) = env
            .try_crank_with_candidates(&candidates)
            .expect("single-opposing-side fullclose crank must not brick");
        assert!(
            cu <= 1_400_000,
            "single-opposing-side fullclose crank exceeded SVM limit on round {round}: cu={cu}"
        );
        worst_cu = worst_cu.max(cu);
        let after_open = count_open_positions(&env, &actors);
        let closed = before_open.saturating_sub(after_open);
        closed_total += closed;
        assert!(
            env.read_last_market_slot() > before_slot || closed > 0,
            "single-opposing-side fullclose crank made no progress on round {round}"
        );
        println!("  round={round:>2}: cu={cu:>8}, closed={closed}, open_after={after_open}");
    }
    assert!(
        closed_total > 0,
        "fullclose lane should close at least one account"
    );
    assert!(
        worst_cu <= 1_350_000,
        "single-opposing-side fullclose lane should stay within the fixed-size headroom ceiling: worst_cu={worst_cu}"
    );
}

#[test]
fn benchmark_keeper_crank_phase2_only_dense_positions_stays_bounded() {
    println!("\n=== KEEPER PHASE2-ONLY DENSE POSITION CU REGRESSION ===");
    let test_sizes: &[usize] = if MAX_ACCOUNTS >= 512 {
        &[128, 256, 512]
    } else {
        &[64, 128, 192]
    };
    let mut saturated_reference: Option<u64> = None;

    for &num_actors in test_sizes {
        let (mut env, actors) = setup_dense_crank_market(num_actors, true);
        let start_slot = env.read_last_market_slot();
        env.set_price(DENSE_CRANK_P0_E6 as i64, start_slot + 1);

        let before_rr = env.read_rr_cursor_position();
        let (cu, _logs) = env.try_crank().expect("phase2-only crank must not brick");
        let after_rr = env.read_rr_cursor_position();
        assert_ne!(
            after_rr, before_rr,
            "phase2-only crank should advance the RR cursor for actors={num_actors}"
        );
        assert_eq!(
            count_open_positions(&env, &actors),
            actors.len(),
            "phase2-only no-price-move crank must not close accounts"
        );
        println!("  actors={num_actors:>3}: cu={cu:>8}, rr={before_rr}->{after_rr}");
        if num_actors == percolator_prog::constants::MAX_KEEPER_CANDIDATES {
            saturated_reference = Some(cu);
        } else if let Some(reference) = saturated_reference {
            assert!(
                cu <= reference + 150_000,
                "phase2-only CU should remain fixed-size after candidate cap: cap_cu={reference}, actors={num_actors}, cu={cu}"
            );
        }
    }
}

#[test]
fn benchmark_keeper_crank_dense_candidate_cap_fixed_size_and_progress() {
    println!("\n=== KEEPER DENSE CANDIDATE-CAP CU REGRESSION ===");
    println!(
        "candidate cap={}, phase1 budget={}, max accounts={}",
        percolator_prog::constants::MAX_KEEPER_CANDIDATES,
        percolator_prog::constants::LIQ_BUDGET_PER_CRANK,
        MAX_ACCOUNTS
    );

    let test_sizes: &[usize] = if MAX_ACCOUNTS >= 512 {
        &[
            percolator_prog::constants::MAX_KEEPER_CANDIDATES,
            128,
            256,
            512,
            1024,
            MAX_ACCOUNTS - 1,
        ]
    } else {
        &[64, 128, 192]
    };
    let mut saturated_reference: Option<u64> = None;
    let mut results = Vec::new();

    for &num_actors in test_sizes {
        assert!(num_actors + 1 <= MAX_ACCOUNTS);
        let mut env = TestEnv::new();
        env.write_price_and_clock(DENSE_CRANK_P0_E6 as i64, DENSE_CRANK_START_SLOT);
        let admin = env.payer.pubkey();
        let mint = env.mint;
        env.init_market_raw(encode_dense_crank_market(&admin, &mint));

        let lp = Keypair::new();
        let lp_idx = env.init_lp_with_fee(&lp, DENSE_CRANK_LP_CAPITAL + DENSE_CRANK_SETUP_BUFFER);
        let payer = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
        env.top_up_insurance(&payer, DENSE_CRANK_INSURANCE);

        let size = dense_crank_position_size(DENSE_CRANK_CAPITAL_PER_USER, DENSE_CRANK_P0_E6);
        let mut actors = Vec::with_capacity(num_actors);
        for _ in 0..num_actors {
            let user = Keypair::new();
            let user_idx = env.init_user_with_fee(
                &user,
                DENSE_CRANK_CAPITAL_PER_USER + DENSE_CRANK_SETUP_BUFFER,
            );
            let direction = if actors.len() < percolator_prog::constants::MAX_KEEPER_CANDIDATES {
                1i128
            } else {
                -1i128
            };
            env.trade(&user, &lp, lp_idx, user_idx, size * direction);
            actors.push(user_idx);
        }

        let start_slot = env.read_last_market_slot();
        let target_dt_slots = 40;
        let target = dense_crank_next_price_for_dt(
            DENSE_CRANK_P0_E6,
            -(DENSE_CRANK_CAP_BPS as i16),
            target_dt_slots,
        );
        env.set_price(target as i64, start_slot + target_dt_slots);

        let mut worst_cu = 0u64;
        let mut total_closed = 0usize;
        let rounds = 4usize.min(
            (num_actors + percolator_prog::constants::LIQ_BUDGET_PER_CRANK as usize - 1)
                / percolator_prog::constants::LIQ_BUDGET_PER_CRANK as usize,
        );
        for round in 0..rounds {
            let before_open = count_open_positions(&env, &actors);
            let before_slot = env.read_last_market_slot();
            let before_rr = env.read_rr_cursor_position();
            let candidates: Vec<u16> = actors
                .iter()
                .copied()
                .filter(|&idx| env.read_account_position(idx) != 0)
                .take(percolator_prog::constants::MAX_KEEPER_CANDIDATES)
                .collect();
            assert!(
                !candidates.is_empty(),
                "dense setup should still have open candidates on round {round}"
            );

            let (cu, _logs) = env
                .try_crank_with_candidates(&candidates)
                .expect("dense candidate-cap crank must not brick");
            assert!(
                cu <= 1_400_000,
                "keeper crank exceeded SVM tx limit: actors={num_actors}, round={round}, cu={cu}"
            );
            worst_cu = worst_cu.max(cu);

            let after_open = count_open_positions(&env, &actors);
            let after_slot = env.read_last_market_slot();
            let after_rr = env.read_rr_cursor_position();
            let closed = before_open.saturating_sub(after_open);
            total_closed += closed;
            assert!(
                after_slot > before_slot || after_rr != before_rr || closed > 0,
                "dense candidate-cap crank made no observable progress: actors={num_actors}, round={round}, cu={cu}, before_open={before_open}, after_open={after_open}, before_slot={before_slot}, after_slot={after_slot}, before_rr={before_rr}, after_rr={after_rr}"
            );
            println!(
                "  actors={num_actors:>3}, round={round:>2}: cu={cu:>8}, closed={closed:>2}, open_after={after_open:>3}, rr={before_rr}->{after_rr}, slot={before_slot}->{after_slot}"
            );
        }

        assert!(
            total_closed > 0 || env.read_last_market_slot() > start_slot,
            "dense candidate-cap sequence should make durable progress"
        );
        println!("  actors={num_actors:>3}: worst_cu={worst_cu:>8}, total_closed={total_closed}");
        if num_actors == percolator_prog::constants::MAX_KEEPER_CANDIDATES {
            saturated_reference = Some(worst_cu);
        }
        results.push((num_actors, worst_cu));
    }

    let reference = saturated_reference.expect("test sizes must include candidate cap");
    for (num_actors, worst_cu) in results {
        if num_actors <= percolator_prog::constants::MAX_KEEPER_CANDIDATES {
            continue;
        }
        assert!(
            worst_cu <= reference + 250_000,
            "keeper CU should remain budget-shaped once candidate cap is saturated: cap_cu={reference}, actors={num_actors}, worst_cu={worst_cu}"
        );
    }
}

#[test]
#[cfg(not(any(feature = "small", feature = "medium")))]
fn benchmark_worst_case_scenarios() {
    println!("\n=== WORST-CASE CRANK CU BENCHMARK ===");
    println!("MAX_ACCOUNTS: {}", MAX_ACCOUNTS);
    println!("SLAB_LEN: {}", SLAB_LEN);
    println!("Solana max CU per tx: 1,400,000\n");

    // Assert we're testing with production config (4096 accounts)
    assert_eq!(
        MAX_ACCOUNTS, 4096,
        "Expected MAX_ACCOUNTS=4096 for production benchmark"
    );
    assert!(
        SLAB_LEN > 900_000,
        "Expected SLAB_LEN > 900K for production benchmark, got {}",
        SLAB_LEN
    );

    let path = program_path();
    if !path.exists() {
        println!("SKIP: BPF not found. Run: cargo build-sbf");
        return;
    }

    // Scenario 1: All empty slots (just LP, no users)
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Scenario 1: 🟢 All empty slots (LP only) - LOWEST");
    {
        let mut env = TestEnv::new();
        env.init_market();

        let lp = Keypair::new();
        env.init_lp(&lp);
        env.deposit(&lp, 0, 1_000_000_000);

        env.set_price(100_000_000, 200);
        let cu = env.crank();
        println!(
            "  CU: {:>10} (baseline scan overhead for {} slots)",
            cu, MAX_ACCOUNTS
        );
        let cu_per_slot = cu / MAX_ACCOUNTS as u64;
        println!("  CU/slot: ~{}", cu_per_slot);
    }

    // Scenario 2: All dust accounts (no positions)
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Scenario 2: 🟡 All dust accounts (no positions)");
    {
        let mut env = TestEnv::new();
        env.init_market();

        let lp = Keypair::new();
        env.init_lp(&lp);
        env.deposit(&lp, 0, 1_000_000_000_000);

        // Create users until we hit CU limit
        let mut users_created = 0;
        for i in 0..(MAX_ACCOUNTS - 1) {
            let user = Keypair::new();
            env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();
            let ata = env.create_ata(&user.pubkey(), 100);

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
                data: encode_init_user(100),
            };
            let tx = Transaction::new_signed_with_payer(
                &[cu_ix(), ix],
                Some(&user.pubkey()),
                &[&user],
                env.svm.latest_blockhash(),
            );
            env.svm.send_transaction(tx).unwrap();
            env.deposit(&user, (i + 1) as u16, 1);
            users_created = i + 1;

            if (i + 1) % 500 == 0 {
                println!("    Created {} users...", i + 1);
            }
        }
        println!("  Created {} dust users total", users_created);

        env.set_price(100_000_000, 200);
        match env.try_crank() {
            Ok((cu, _logs)) => {
                let cu_per_account = cu / (users_created + 1) as u64;
                println!("  CU: {:>10} total, ~{} CU/account", cu, cu_per_account);
            }
            Err(_) => {
                println!("  ⚠️  EXCEEDS 1.4M CU LIMIT with {} users!", users_created);
            }
        }
    }

    // Scenario 3: Find practical limit - binary search for max users
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Scenario 3: 📊 Finding practical CU limit");
    {
        let test_sizes = [100, 500, 1000, 1500, 2000, 2500, 3000, 3500, 4000];
        let mut last_success = 0u64;
        let mut last_success_users = 0usize;

        for &num_users in &test_sizes {
            if num_users >= MAX_ACCOUNTS {
                break;
            }

            let mut env = TestEnv::new();
            env.init_market();

            let lp = Keypair::new();
            env.init_lp(&lp);
            env.deposit(&lp, 0, 1_000_000_000_000);

            // Bulk create users
            for i in 0..num_users {
                let user = Keypair::new();
                env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();
                let ata = env.create_ata(&user.pubkey(), 100);

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
                    data: encode_init_user(100),
                };
                let tx = Transaction::new_signed_with_payer(
                    &[cu_ix(), ix],
                    Some(&user.pubkey()),
                    &[&user],
                    env.svm.latest_blockhash(),
                );
                env.svm.send_transaction(tx).unwrap();
                env.deposit(&user, (i + 1) as u16, 1);
            }

            env.set_price(100_000_000, 200);
            match env.try_crank() {
                Ok((cu, _logs)) => {
                    let cu_per_account = cu / (num_users + 1) as u64;
                    println!(
                        "  {:>4} users: {:>10} CU (~{} CU/user)",
                        num_users, cu, cu_per_account
                    );
                    last_success = cu;
                    last_success_users = num_users;
                }
                Err(_) => {
                    println!("  {:>4} users: ❌ EXCEEDS 1.4M CU LIMIT", num_users);
                    break;
                }
            }
        }
        if last_success_users > 0 {
            println!(
                "  → Max practical limit: ~{} users in single tx",
                last_success_users
            );
        }
    }

    // Scenario 4: Healthy accounts with positions (limited users)
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Scenario 4: 🟡 Healthy accounts with positions");
    {
        // Scale down for positions - they add CU overhead
        let test_sizes = [50, 100, 200, 500, 1000];

        for &num_users in &test_sizes {
            if num_users >= MAX_ACCOUNTS {
                break;
            }

            let mut env = TestEnv::new();
            env.init_market();

            let lp = Keypair::new();
            env.init_lp(&lp);
            env.deposit(&lp, 0, 10_000_000_000_000);

            let users = create_users(&mut env, num_users, 1_000_000);

            // Add positions for each user
            for (i, user) in users.iter().enumerate() {
                let user_idx = (i + 1) as u16;
                let size = if i % 2 == 0 { 100i128 } else { -100i128 };
                env.trade(user, &lp, 0, user_idx, size);
            }

            env.set_price(100_000_000, 200);
            match env.try_crank() {
                Ok((cu, _logs)) => {
                    let cu_per_account = cu / (num_users + 1) as u64;
                    println!(
                        "  {:>4} users: {:>10} CU (~{} CU/user)",
                        num_users, cu, cu_per_account
                    );
                }
                Err(_) => {
                    println!("  {:>4} users: ❌ EXCEEDS 1.4M CU LIMIT", num_users);
                    break;
                }
            }
        }
    }

    // Scenario 5: 🟠 Deeply underwater (price crash triggers liquidations)
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Scenario 5: 🟠 Deeply underwater accounts (liquidations)");
    {
        let test_sizes = [100, 200, 300, 400, 500, 600, 700, 800, 900, 1000];

        for &num_users in &test_sizes {
            if num_users >= MAX_ACCOUNTS {
                break;
            }

            let mut env = TestEnv::new();
            env.init_market();

            let lp = Keypair::new();
            env.init_lp(&lp);
            env.deposit(&lp, 0, 100_000_000_000_000);

            let users = create_users(&mut env, num_users, 1_000_000);

            // All users go long with high leverage
            for (i, user) in users.iter().enumerate() {
                let user_idx = (i + 1) as u16;
                env.trade(user, &lp, 0, user_idx, 1000i128);
            }

            // Price crashes 50% - all users deeply underwater
            env.set_price(50_000_000, 200);

            match env.try_crank() {
                Ok((cu, _logs)) => {
                    let cu_per_account = cu / (num_users + 1) as u64;
                    println!(
                        "  {:>4} liquidations: {:>10} CU (~{} CU/user)",
                        num_users, cu, cu_per_account
                    );
                }
                Err(_) => {
                    println!("  {:>4} liquidations: ❌ EXCEEDS 1.4M CU LIMIT", num_users);
                    break;
                }
            }
        }
    }

    // Scenario 6: 🔴 Knife-edge liquidations (worst case)
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Scenario 6: 🔴 Knife-edge liquidations (hardest case)");
    println!("  (mixed long/short at high leverage, price moves 15%)");
    {
        let test_sizes = [10, 25, 50, 100, 200];

        for &num_users in &test_sizes {
            if num_users >= MAX_ACCOUNTS {
                break;
            }

            let mut env = TestEnv::new();
            env.init_market();

            let lp = Keypair::new();
            env.init_lp(&lp);
            env.deposit(&lp, 0, 100_000_000_000_000);

            let users = create_users(&mut env, num_users, 10_000_000);

            // Mix of long and short positions at high leverage
            for (i, user) in users.iter().enumerate() {
                let user_idx = (i + 1) as u16;
                let size = if i % 2 == 0 { 5000i128 } else { -5000i128 };
                env.trade(user, &lp, 0, user_idx, size);
            }

            // Price moves 15% - triggers some liquidations
            env.set_price(85_000_000, 200);

            match env.try_crank() {
                Ok((cu, _logs)) => {
                    let cu_per_account = cu / (num_users + 1) as u64;
                    println!(
                        "  {:>4} users at edge: {:>10} CU (~{} CU/user)",
                        num_users, cu, cu_per_account
                    );
                }
                Err(_) => {
                    println!("  {:>4} users at edge: ❌ EXCEEDS 1.4M CU LIMIT", num_users);
                    break;
                }
            }
        }
    }

    // Scenario 7: 🔥 Worst-case ADL (force_realize_losses with unpaid losses)
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Scenario 7: 🔥 Worst-case ADL");
    println!("  (half long/half short with varying sizes, 50% crash)");
    println!("  Losers have minimal capital → unpaid losses → apply_adl");
    {
        // Test sizes for ADL scenario
        let test_sizes = [200, 400, 600, 800, 1000, 1100, 1200, 1300, 1400, 1500];

        for &num_users in &test_sizes {
            if num_users >= MAX_ACCOUNTS {
                break;
            }

            let mut env = TestEnv::new();

            // Use normal threshold - insurance starts at 0 which is <= 0 threshold
            // This should trigger force_realize_losses path when there are unpaid losses
            // warmup_period > 0 so winners' PnL stays unwrapped
            // warmup≤perm_resolve(80) per §14.1: h_max must fit inside the
            // permissionless-resolve window.
            env.init_market_with_params(0, 50); // threshold=0, warmup=50 slots

            let lp = Keypair::new();
            env.init_lp(&lp);
            // LP needs huge capital to take the other side of all trades
            env.deposit(&lp, 0, 1_000_000_000_000_000);

            // Create users: half will be winners, half losers
            // Winners (shorts): deposit minimal capital, will have positive PnL after crash
            // Losers (longs): deposit zero/minimal capital, will have unpaid losses
            let half = num_users / 2;

            // Create all users first (without deposits for losers)
            let mut users = Vec::with_capacity(num_users);
            for i in 0..num_users {
                let user = Keypair::new();
                env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();
                let ata = env.create_ata(&user.pubkey(), 100);

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
                    data: encode_init_user(100),
                };
                let tx = Transaction::new_signed_with_payer(
                    &[cu_ix(), ix],
                    Some(&user.pubkey()),
                    &[&user],
                    env.svm.latest_blockhash(),
                );
                env.svm.send_transaction(tx).unwrap();

                let user_idx = (i + 1) as u16;

                // All users need enough margin to open positions
                // But losers will have capital wiped out by the crash
                // Size varies: (i%100+1)*10, so max size ~1000 contracts
                // At $100 price, position value = size * 100 = up to $100,000
                // With 10% initial margin, need ~$10,000 margin
                // Deposit slightly more than minimum so trade succeeds
                let deposit = if i < half {
                    // Losers (longs): just enough margin, will be wiped out
                    100_000 // minimal to pass margin check
                } else {
                    // Winners (shorts): more capital
                    10_000_000
                };
                env.deposit(&user, user_idx, deposit);

                users.push(user);
            }

            // Open positions with varying sizes (for dense remainders in ADL)
            // Longs: will lose on 50% price crash
            // Shorts: will win on 50% price crash
            for (i, user) in users.iter().enumerate() {
                let user_idx = (i + 1) as u16;
                // Size varies by index to create different unwrapped PnL values
                let base_size = ((i % 100) + 1) as i128 * 10;
                let size = if i < half {
                    base_size // longs (losers after crash)
                } else {
                    -base_size // shorts (winners after crash)
                };
                env.trade(user, &lp, 0, user_idx, size);
            }

            // Price crashes 50% - longs lose massively, shorts win massively
            // force_realize_losses will be triggered because insurance < threshold
            // Losers have insufficient capital → unpaid losses → apply_adl
            env.set_price(50_000_000, 200); // $100 -> $50

            match env.try_crank() {
                Ok((cu, _logs)) => {
                    let cu_per_account = cu / (num_users + 1) as u64;
                    println!(
                        "  {:>4} ADL accounts: {:>10} CU (~{} CU/user)",
                        num_users, cu, cu_per_account
                    );
                }
                Err(e) => {
                    if e.contains("exceeded CUs") || e.contains("ProgramFailedToComplete") {
                        println!("  {:>4} ADL accounts: ❌ EXCEEDS 1.4M CU LIMIT", num_users);
                    } else {
                        println!("  {:>4} ADL accounts: ❌ {}", num_users, e);
                    }
                    break;
                }
            }
        }
    }

    // Scenario 8: 🔥🔥 Full 4096 sweep - worst single crank across 16 calls
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Scenario 8: 🔥🔥 Full sweep worst-case (16 cranks)");
    println!("  Testing worst single crank CU across 16-crank full sweep");
    println!("  8a: Healthy accounts with positions (no liquidations)");
    {
        // Test with increasing account counts - find threshold
        let test_sizes = [100, 200, 256, 512, 768, 1024, 1536, 2048, 3072, 4095];

        for &num_users in &test_sizes {
            if num_users >= MAX_ACCOUNTS {
                continue;
            }

            let mut env = TestEnv::new();
            env.init_market_with_params(0, 50); // warmup<=perm_resolve(80) per §14.1

            let lp = Keypair::new();
            env.init_lp(&lp);
            env.deposit(&lp, 0, 1_000_000_000_000_000);

            // Create all users with positions (worst case = all have positions)
            let half = num_users / 2;
            let mut users = Vec::with_capacity(num_users);

            print!("  Creating {} users...", num_users);
            std::io::Write::flush(&mut std::io::stdout()).ok();

            for i in 0..num_users {
                let user = Keypair::new();
                env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();
                let ata = env.create_ata(&user.pubkey(), 100);

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
                    data: encode_init_user(100),
                };
                let tx = Transaction::new_signed_with_payer(
                    &[cu_ix(), ix],
                    Some(&user.pubkey()),
                    &[&user],
                    env.svm.latest_blockhash(),
                );
                env.svm.send_transaction(tx).unwrap();

                let user_idx = (i + 1) as u16;
                // All users get enough margin for positions
                let deposit = if i < half { 100_000 } else { 10_000_000 };
                env.deposit(&user, user_idx, deposit);
                users.push(user);
            }
            println!(" done");

            // Open positions - half long, half short (creates liquidation scenario)
            for (i, user) in users.iter().enumerate() {
                let user_idx = (i + 1) as u16;
                let base_size = ((i % 100) + 1) as i128 * 10;
                let size = if i < half { base_size } else { -base_size };
                env.trade(user, &lp, 0, user_idx, size);
            }

            // No price crash - healthy accounts
            env.set_price(100_000_000, 200);

            // Call crank 16 times and track worst CU
            let mut worst_cu: u64 = 0;
            let mut total_cu: u64 = 0;
            let mut any_failed = false;
            let mut first_error: Option<String> = None;

            for crank_num in 0..16 {
                match env.try_crank() {
                    Ok((cu, _logs)) => {
                        if cu > worst_cu {
                            worst_cu = cu;
                        }
                        total_cu += cu;
                    }
                    Err(e) => {
                        if e.contains("exceeded CUs") || e.contains("ProgramFailedToComplete") {
                            println!("\n    Crank {} EXCEEDED 1.4M CU!", crank_num + 1);
                            any_failed = true;
                            break;
                        }
                        first_error = Some(format!("crank {} failed: {}", crank_num + 1, e));
                        any_failed = true;
                        break;
                    }
                }
            }

            if any_failed {
                if let Some(e) = first_error {
                    println!("  {:>4} users: ❌ {}", num_users, e);
                } else {
                    println!(
                        "  {:>4} users: ❌ Single crank exceeded 1.4M CU limit",
                        num_users
                    );
                }
            } else {
                let pct = (worst_cu as f64 / 1_400_000.0) * 100.0;
                println!("  {:>4} users: worst={:>10} CU ({:.1}% of limit), total={} CU across 16 cranks",
                    num_users, worst_cu, pct, total_cu);
            }
        }
    }

    // Scenario 8b: Full sweep with liquidations - find threshold
    println!("\n  8b: With 50% crash (liquidations/ADL)");
    {
        let test_sizes = [100, 200, 256, 512, 768, 1024, 1536, 2048, 3072, 4095];

        for &num_users in &test_sizes {
            if num_users >= MAX_ACCOUNTS {
                continue;
            }

            let mut env = TestEnv::new();
            env.init_market_with_params(0, 50); // warmup<=perm_resolve(80) per §14.1

            let lp = Keypair::new();
            env.init_lp(&lp);
            env.deposit(&lp, 0, 1_000_000_000_000_000);

            let half = num_users / 2;
            let mut users = Vec::with_capacity(num_users);

            print!("  Creating {} users...", num_users);
            std::io::Write::flush(&mut std::io::stdout()).ok();

            for i in 0..num_users {
                let user = Keypair::new();
                env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();
                let ata = env.create_ata(&user.pubkey(), 100);

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
                    data: encode_init_user(100),
                };
                let tx = Transaction::new_signed_with_payer(
                    &[cu_ix(), ix],
                    Some(&user.pubkey()),
                    &[&user],
                    env.svm.latest_blockhash(),
                );
                env.svm.send_transaction(tx).unwrap();

                let user_idx = (i + 1) as u16;
                let deposit = if i < half { 100_000 } else { 10_000_000 };
                env.deposit(&user, user_idx, deposit);
                users.push(user);
            }
            println!(" done");

            for (i, user) in users.iter().enumerate() {
                let user_idx = (i + 1) as u16;
                let base_size = ((i % 100) + 1) as i128 * 10;
                let size = if i < half { base_size } else { -base_size };
                env.trade(user, &lp, 0, user_idx, size);
            }

            // 50% price crash - triggers liquidations
            env.set_price(50_000_000, 200);

            let mut worst_cu: u64 = 0;
            let mut total_cu: u64 = 0;
            let mut any_failed = false;
            let mut last_logs: Vec<String> = Vec::new();
            let mut first_error: Option<String> = None;

            for crank_num in 0..16 {
                match env.try_crank() {
                    Ok((cu, logs)) => {
                        if cu > worst_cu {
                            worst_cu = cu;
                        }
                        total_cu += cu;
                        last_logs = logs;
                    }
                    Err(e) => {
                        if e.contains("exceeded CUs") || e.contains("ProgramFailedToComplete") {
                            println!("\n    Crank {} EXCEEDED 1.4M CU!", crank_num + 1);
                            any_failed = true;
                            break;
                        }
                        first_error = Some(format!("crank {} failed: {}", crank_num + 1, e));
                        any_failed = true;
                        break;
                    }
                }
            }

            if any_failed {
                if let Some(e) = first_error {
                    println!("  {:>4} users: ❌ {}", num_users, e);
                } else {
                    println!(
                        "  {:>4} users: ❌ Single crank exceeded 1.4M CU limit",
                        num_users
                    );
                }
            } else {
                let pct = (worst_cu as f64 / 1_400_000.0) * 100.0;
                // Extract CRANK_STATS from logs - sol_log_64 format: "Program log: 0xtag, 0xliqs, 0xforce, 0xmax_accounts, 0x0"
                let mut liquidations: u64 = 0;
                let mut force_realize: u64 = 0;
                let mut max_accounts: u64 = 0;
                let mut found_stats = false;

                // Helper to parse hex or decimal
                fn parse_hex_or_dec(s: &str) -> u64 {
                    let s = s.trim();
                    if let Some(hex) = s.strip_prefix("0x") {
                        u64::from_str_radix(hex, 16).unwrap_or(0)
                    } else {
                        s.parse().unwrap_or(0)
                    }
                }

                for (i, log) in last_logs.iter().enumerate() {
                    if log.contains("CRANK_STATS") {
                        // Next log line should have the sol_log_64 output
                        if i + 1 < last_logs.len() {
                            let next_log = &last_logs[i + 1];
                            // Format: "Program log: 0xc8a4c, 0x0, 0x200, 0x1000, 0x0"
                            if let Some(rest) = next_log.strip_prefix("Program log: ") {
                                let parts: Vec<&str> = rest.split(", ").collect();
                                if parts.len() >= 4 {
                                    liquidations = parse_hex_or_dec(parts[1]);
                                    force_realize = parse_hex_or_dec(parts[2]);
                                    max_accounts = parse_hex_or_dec(parts[3]);
                                    found_stats = true;
                                }
                            }
                        }
                    }
                }
                if found_stats {
                    println!("  {:>4} users: worst={:>10} CU ({:.1}%), total={} | liqs={} force={} max_acc={}",
                        num_users, worst_cu, pct, total_cu, liquidations, force_realize, max_accounts);
                } else {
                    println!("  {:>4} users: worst={:>10} CU ({:.1}% of limit), total={} CU across 16 cranks",
                        num_users, worst_cu, pct, total_cu);
                }
            }
        }
    }

    // Scenario 9: Worst-case liquidation with late-window underwater accounts
    // Tests true liquidation path with MTM margin check (insurance > threshold)
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Scenario 9: 🔥🔥🔥 Worst-case liquidation (MTM margin check)");
    println!("  1 LP + 100 users, insurance > threshold for liquidation path");
    println!("  Each window: first half safe, second half deeply underwater");
    {
        let num_users = 100;

        let mut env = TestEnv::new();
        // threshold=0, warmup=0
        env.init_market_with_params(0, 1);

        // Top up insurance so force_realize is OFF and liquidation path runs
        let insurance_funder = Keypair::new();
        env.svm
            .airdrop(&insurance_funder.pubkey(), 1_000_000_000)
            .unwrap();
        env.top_up_insurance(&insurance_funder, 1_000_000_000); // 1B tokens

        let lp = Keypair::new();
        env.init_lp(&lp);
        // LP needs large collateral to absorb all user positions
        env.deposit(&lp, 0, 10_000_000_000_000); // 10B tokens

        println!("  Creating {} users with varied collateral...", num_users);

        // Create users: for each 256-account window, first 128 are "safe", last 128 are "liq"
        // Safe users: more collateral, smaller position -> stay above maintenance
        // Liq users: less collateral, same position -> fall below maintenance
        let mut users = Vec::with_capacity(num_users);

        for i in 0..num_users {
            let user = Keypair::new();
            env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();

            // Create ATA (fee=0 since new_account_fee in params is 0)
            let ata = env.create_ata(&user.pubkey(), 100);

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
                data: encode_init_user(100),
            };
            let tx = Transaction::new_signed_with_payer(
                &[cu_ix(), ix],
                Some(&user.pubkey()),
                &[&user],
                env.svm.latest_blockhash(),
            );
            env.svm.send_transaction(tx).unwrap();

            let user_idx = (i + 1) as u16;

            // Determine if this user is in "safe" or "liq" half of their window
            let window_offset = i % 256;
            let is_liq_user = window_offset >= 128;

            // Position: 1000 contracts at $100 = 100K notional
            // Initial margin 10% = 10K (need capital >= 10K)
            // After 50% crash: notional = 50K, loss = 50K
            // Maintenance 5% of 50K = 2.5K
            // For liquidation: equity = capital - 50K < 2.5K → capital < 52.5K
            // Liq users: 50K capital (after 50K loss, equity = 0 < 2.5K → liquidatable)
            // Safe users: 1M capital (after 50K loss, equity = 950K >> 2.5K → safe)
            let deposit = if is_liq_user { 50_000u64 } else { 1_000_000u64 };
            env.deposit(&user, user_idx, deposit);

            users.push(user);

            if (i + 1) % 1000 == 0 {
                println!("    Created {} users...", i + 1);
            }
        }
        println!("    Created {} users total", num_users);

        // Open positions: all users go long with same size
        // Try 1000 contracts like Scenario 5 - with 10K liq collateral should go underwater
        // Safe users (10B) will easily survive, liq users (10K) should be liquidated
        println!("  Opening positions (all users long)...");
        let position_size = 1000i128; // 1K contracts like Scenario 5

        for (i, user) in users.iter().enumerate() {
            let user_idx = (i + 1) as u16;
            env.trade(user, &lp, 0, user_idx, position_size);

            if (i + 1) % 1000 == 0 {
                println!("    Opened {} positions...", i + 1);
            }
        }

        // Price crash: $100 -> $50 (50% drop)
        // Longs lose 50% of position value ($5K loss on $10K position)
        // Safe users (1B collateral) should survive
        // Liq users (1K collateral) should be underwater and liquidated
        println!("  Crashing price 50%: $100 -> $50");
        env.set_price(50_000_000, 200);

        // Run enough cranks to close all liq users
        // FORCE_REALIZE_BUDGET_PER_CRANK=32, ~2048 liq users → need ~64 cranks
        println!("  Running 64 cranks (32 force_realize per crank)...");
        let mut worst_cu: u64 = 0;
        let mut total_cu: u64 = 0;
        let mut total_liqs: u64 = 0;
        let mut total_force: u64 = 0;
        let mut any_failed = false;
        let mut last_max_acc: u64 = 0;
        let mut last_insurance: u64 = 0;

        // Helper to parse hex or decimal
        fn parse_hex_or_dec(s: &str) -> u64 {
            let s = s.trim();
            if let Some(hex) = s.strip_prefix("0x") {
                u64::from_str_radix(hex, 16).unwrap_or(0)
            } else {
                s.parse().unwrap_or(0)
            }
        }

        for crank_num in 0..64 {
            match env.try_crank() {
                Ok((cu, logs)) => {
                    if cu > worst_cu {
                        worst_cu = cu;
                    }
                    total_cu += cu;

                    // Parse stats from logs
                    for (i, log) in logs.iter().enumerate() {
                        if log.contains("CRANK_STATS") {
                            if i + 1 < logs.len() {
                                let next_log = &logs[i + 1];
                                if let Some(rest) = next_log.strip_prefix("Program log: ") {
                                    let parts: Vec<&str> = rest.split(", ").collect();
                                    if parts.len() >= 5 {
                                        let liqs = parse_hex_or_dec(parts[1]);
                                        let force = parse_hex_or_dec(parts[2]);
                                        last_max_acc = parse_hex_or_dec(parts[3]);
                                        last_insurance = parse_hex_or_dec(parts[4]);
                                        total_liqs = liqs; // cumulative from engine
                                        total_force = force;

                                        println!(
                                            "    Crank {:>2}: {:>7} CU | liqs={} force={}",
                                            crank_num + 1,
                                            cu,
                                            liqs,
                                            force
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    if e.contains("exceeded CUs") || e.contains("ProgramFailedToComplete") {
                        println!("    Crank {} EXCEEDED 1.4M CU!", crank_num + 1);
                        any_failed = true;
                        break;
                    } else {
                        println!("    Crank {} error: {}", crank_num + 1, e);
                    }
                }
            }
        }

        println!();
        if any_failed {
            println!("  ❌ FAILED: Single crank exceeded 1.4M CU limit");
        } else {
            let pct = (worst_cu as f64 / 1_400_000.0) * 100.0;
            println!(
                "  ✓ RESULT: worst={} CU ({:.1}%), total={} CU",
                worst_cu, pct, total_cu
            );
            println!(
                "    Liquidations: {}, Force-realize: {}",
                total_liqs, total_force
            );
            println!("    MAX_ACCOUNTS: {}", last_max_acc);
            if total_liqs == 0 && total_force == 0 {
                println!("    ⚠️  WARNING: No liquidations or force-realize - check params");
            }
            // Expected: ~2048 liquidations (half of 100 users are liq users)
            let expected_liq = num_users / 2;
            if total_liqs > 0 {
                println!(
                    "    ✓ MTM margin check working - {} liquidations triggered",
                    total_liqs
                );
            }
        }
    }

    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("=== SUMMARY ===");
    println!("• Empty cranks use the greedy RR window; candidate cranks use the audited candidate window");
    println!("• Scenario 1 reports the LP-only baseline for MAX_ACCOUNTS={MAX_ACCOUNTS}");
    println!("• Key metric: worst single crank must stay under 1.4M CU");
    println!("• ADL/liquidation processing adds CU overhead per affected account");
}

/// Per-instruction CU benchmark covering all instruction types.
/// Measures CU consumed for each instruction under typical conditions.
#[test]
#[cfg(not(any(feature = "small", feature = "medium")))]
fn benchmark_all_instructions() {
    println!("\n=== PER-INSTRUCTION CU BENCHMARK ===\n");

    let mut env = TestEnv::new();
    env.init_market();

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 50_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    env.set_price(100_000_000, 200);
    env.crank();

    // Helper: send instruction, return CU consumed
    let measure =
        |svm: &mut LiteSVM, ix: Instruction, signers: &[&Keypair]| -> Result<u64, String> {
            let budget = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
            let payer = signers[0];
            let tx = Transaction::new_signed_with_payer(
                &[budget, ix],
                Some(&payer.pubkey()),
                signers,
                svm.latest_blockhash(),
            );
            match svm.send_transaction(tx) {
                Ok(r) => Ok(r.compute_units_consumed),
                Err(e) => Err(format!("{:?}", e)),
            }
        };

    let (vault_pda, _) =
        Pubkey::find_program_address(&[b"vault", env.slab.as_ref()], &env.program_id);

    // --- TradeNoCpi (Tag 6) ---
    {
        let ix = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(user.pubkey(), true),
                AccountMeta::new(lp.pubkey(), true),
                AccountMeta::new(env.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(env.pyth_index, false),
            ],
            data: encode_trade(lp_idx, user_idx, 100_000),
        };
        let cu = measure(&mut env.svm, ix, &[&user, &lp]).unwrap();
        println!("TradeNoCpi:            {:>8} CU", cu);
    }

    // --- DepositCollateral (Tag 3) ---
    {
        let ata = env.create_ata(&user.pubkey(), 1_000_000);
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
            data: encode_deposit(user_idx, 1_000_000),
        };
        let cu = measure(&mut env.svm, ix, &[&user]).unwrap();
        println!("DepositCollateral:     {:>8} CU", cu);
    }

    // --- WithdrawCollateral (Tag 4) ---
    {
        env.set_price(100_000_000, 300);
        env.crank();
        let ata = env.create_ata(&user.pubkey(), 0);
        let ix = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(user.pubkey(), true),
                AccountMeta::new(env.slab, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new(ata, false),
                AccountMeta::new_readonly(vault_pda, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(env.pyth_index, false),
            ],
            data: encode_withdraw(user_idx, 100_000),
        };
        let cu = measure(&mut env.svm, ix, &[&user]).unwrap();
        println!("WithdrawCollateral:    {:>8} CU", cu);
    }

    // --- KeeperCrank (Tag 5) ---
    {
        env.set_price(100_000_000, 400);
        let cu = env.crank();
        println!("KeeperCrank:           {:>8} CU", cu);
    }

    // --- TopUpInsurance (Tag 9) ---
    {
        let ata = env.create_ata(&admin.pubkey(), 1_000_000);
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
            data: encode_top_up_insurance(1_000_000),
        };
        let cu = measure(&mut env.svm, ix, &[&admin]).unwrap();
        println!("TopUpInsurance:        {:>8} CU", cu);
    }

    // Tag 11 (SetRiskThreshold) benchmark removed: instruction deleted.

    // --- UpdateConfig (Tag 14) ---
    {
        let ix = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                // Oracle is REQUIRED on non-Hyperp UpdateConfig — admin cannot
                // select the degenerate zero-funding arm by omission.
                AccountMeta::new_readonly(env.pyth_index, false),
            ],
            data: encode_update_config(
                3600, // funding_horizon_slots
                100,  // funding_k_bps
                500,  // funding_max_premium_bps
                5,    // funding_max_e9_per_slot
            ),
        };
        let cu = measure(&mut env.svm, ix, &[&admin]).unwrap();
        println!("UpdateConfig:          {:>8} CU", cu);
    }

    // SetMaintenanceFee (Tag 15) — removed per spec §8.2. Decoder rejects.

    // UpdateAuthority(ORACLE) benchmark removed: the kind is Hyperp-only
    // and this benchmark uses a non-Hyperp market.

    // PushOraclePrice benchmark removed: the instruction is Hyperp-only
    // and this benchmark uses a non-Hyperp market.

    // SetOraclePriceCap (Tag 18) removed in v12.19; the per-slot cap is now
    // the immutable init-time `max_price_move_bps_per_slot` RiskParam.

    // Tag 24 (QueryLpFees) removed from the wire format.

    // --- KeeperCrank candidate liquidation (direct LiquidateAtOracle retired) ---
    // Measure liquidation in an isolated market. The setup intentionally
    // creates a sharp oracle move; keeping that state out of the main
    // benchmark market prevents later live-path measurements from tripping
    // the target/effective catch-up guard for the wrong reason.
    {
        let mut liq_env = TestEnv::new();
        liq_env.init_market();
        let liq_lp = Keypair::new();
        let liq_lp_idx = liq_env.init_lp(&liq_lp);
        liq_env.deposit(&liq_lp, liq_lp_idx, 50_000_000_000);
        let liq_user = Keypair::new();
        let liq_user_idx = liq_env.init_user(&liq_user);
        liq_env.deposit(&liq_user, liq_user_idx, 10_000_000_000);
        liq_env.set_price(100_000_000, 200);
        liq_env.crank();
        liq_env.trade(&liq_user, &liq_lp, liq_lp_idx, liq_user_idx, 100_000);
        liq_env.set_price(50_000_000, 700); // $100 -> $50

        let caller = Keypair::new();
        liq_env
            .svm
            .airdrop(&caller.pubkey(), 1_000_000_000)
            .unwrap();
        let mut data = vec![5u8]; // KeeperCrank
        data.extend_from_slice(&u16::MAX.to_le_bytes());
        data.push(1u8);
        data.extend_from_slice(&liq_user_idx.to_le_bytes());
        data.push(0u8); // FullClose
        let ix = Instruction {
            program_id: liq_env.program_id,
            accounts: vec![
                AccountMeta::new(caller.pubkey(), true),
                AccountMeta::new(liq_env.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(liq_env.pyth_index, false),
            ],
            data,
        };
        match measure(&mut liq_env.svm, ix, &[&caller]) {
            Ok(cu) => println!("KeeperCrank(liq):      {:>8} CU", cu),
            Err(_) => println!("KeeperCrank(liq):      (user not liquidatable at this price)"),
        }
    }

    // --- CloseAccount (Tag 8) ---
    // Close position first, then close account
    {
        // Restore price and flatten position via opposing trade
        env.set_price(100_000_000, 800);
        env.crank();
        // Trade to flatten (negative = close long)
        env.trade(&user, &lp, lp_idx, user_idx, -100_000);
        env.set_price(100_000_000, 810);
        env.crank();

        let user_ata = env.create_ata(&user.pubkey(), 0);
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
            data: encode_close_account(user_idx),
        };
        match measure(&mut env.svm, ix, &[&user]) {
            Ok(cu) => println!("CloseAccount:          {:>8} CU", cu),
            Err(e) => println!("CloseAccount:          (failed: {})", &e[..80.min(e.len())]),
        }
    }

    // --- UpdateAuthority(ADMIN) (Tag 32) — replaces legacy Tag 12 ---
    {
        let ix = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.slab, false),
            ],
            data: encode_update_admin(&admin.pubkey()),
        };
        let cu = measure(&mut env.svm, ix, &[&admin]).unwrap();
        println!("UpdateAuthority(ADMIN): {:>8} CU", cu);
    }

    // --- ResolveMarket + resolved-path instructions ---
    {
        // Top up insurance so WithdrawInsurance has something to withdraw
        env.top_up_insurance(&Keypair::from_bytes(&admin.to_bytes()).unwrap(), 1_000_000);

        env.set_price(100_000_000, 900);
        env.crank();

        // Resolve
        let ix = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(env.pyth_index, false),
            ],
            data: encode_resolve_market(),
        };
        let cu = measure(&mut env.svm, ix, &[&admin]).unwrap();
        println!("ResolveMarket:         {:>8} CU", cu);

        // Resolved KeeperCrank
        env.set_price(100_000_000, 1000);
        let cu = env.crank();
        println!("KeeperCrank(resolved): {:>8} CU", cu);

        // AdminForceCloseAccount (Tag 21) - close LP
        let lp_ata = env.create_ata(&lp.pubkey(), 0);
        let ix = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.slab, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new(lp_ata, false),
                AccountMeta::new_readonly(vault_pda, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
            ],
            data: encode_admin_force_close_account(lp_idx),
        };
        let cu = measure(&mut env.svm, ix, &[&admin]).unwrap();
        println!("AdminForceClose:       {:>8} CU", cu);

        // WithdrawInsurance (Tag 20) - all accounts now closed
        let admin_ata = env.create_ata(&admin.pubkey(), 0);
        let ix = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.slab, false),
                AccountMeta::new(admin_ata, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(vault_pda, false),
            ],
            data: encode_withdraw_insurance(),
        };
        match measure(&mut env.svm, ix, &[&admin]) {
            Ok(cu) => println!("WithdrawInsurance:     {:>8} CU", cu),
            Err(e) => println!("WithdrawInsurance:     (failed: {})", &e[..80.min(e.len())]),
        }

        // CloseSlab (Tag 13)
        let admin_ata2 = env.create_ata(&admin.pubkey(), 0);
        let ix = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.slab, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(vault_pda, false),
                AccountMeta::new(admin_ata2, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: encode_close_slab(),
        };
        match measure(&mut env.svm, ix, &[&admin]) {
            Ok(cu) => println!("CloseSlab:             {:>8} CU", cu),
            Err(e) => println!("CloseSlab:             (failed: {})", &e[..80.min(e.len())]),
        }
    }

    println!("\n=== END PER-INSTRUCTION CU BENCHMARK ===");
}
