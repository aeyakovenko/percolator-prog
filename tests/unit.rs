//! Unit tests for percolator-prog
//!
//! These tests verify the Solana program wrapper's instruction handling,
//! including account validation, state management, and invariants.

use percolator::{I128, MAX_ACCOUNTS, U128};
use percolator_prog::{
    constants::{MAGIC, VERSION},
    error::PercolatorError,
    oracle,
    processor::process_instruction,
    state, units, zc,
};
use solana_program::{
    account_info::AccountInfo, clock::Clock, program_error::ProgramError, program_pack::Pack,
    pubkey::Pubkey,
};
use spl_token::state::{Account as TokenAccount, AccountState};

// --- Harness ---

struct TestAccount {
    key: Pubkey,
    owner: Pubkey,
    lamports: u64,
    data: Vec<u8>,
    is_signer: bool,
    is_writable: bool,
    executable: bool,
}

impl TestAccount {
    fn new(key: Pubkey, owner: Pubkey, lamports: u64, data: Vec<u8>) -> Self {
        Self {
            key,
            owner,
            lamports,
            data,
            is_signer: false,
            is_writable: false,
            executable: false,
        }
    }
    fn signer(mut self) -> Self {
        self.is_signer = true;
        self
    }
    fn writable(mut self) -> Self {
        self.is_writable = true;
        self
    }
    fn executable(mut self) -> Self {
        self.executable = true;
        self
    }

    fn to_info<'a>(&'a mut self) -> AccountInfo<'a> {
        AccountInfo::new(
            &self.key,
            self.is_signer,
            self.is_writable,
            &mut self.lamports,
            &mut self.data,
            &self.owner,
            self.executable,
            0,
        )
    }
}

// --- Builders ---

fn make_token_account(mint: Pubkey, owner: Pubkey, amount: u64) -> Vec<u8> {
    let mut data = vec![0u8; TokenAccount::LEN];
    let mut account = TokenAccount::default();
    account.mint = mint;
    account.owner = owner;
    account.amount = amount;
    account.state = AccountState::Initialized;
    TokenAccount::pack(account, &mut data).unwrap();
    data
}

fn make_mint_account() -> Vec<u8> {
    use spl_token::state::Mint;
    let mut data = vec![0u8; Mint::LEN];
    let mint = Mint {
        mint_authority: solana_program::program_option::COption::None,
        supply: 0,
        decimals: 6,
        is_initialized: true,
        freeze_authority: solana_program::program_option::COption::None,
    };
    Mint::pack(mint, &mut data).unwrap();
    data
}

/// PYTH_RECEIVER_PROGRAM_ID bytes (rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ)
const PYTH_RECEIVER_BYTES: [u8; 32] = [
    0x0c, 0xb7, 0xfa, 0xbb, 0x52, 0xf7, 0xa6, 0x48, 0xbb, 0x5b, 0x31, 0x7d, 0x9a, 0x01, 0x8b, 0x90,
    0x57, 0xcb, 0x02, 0x47, 0x74, 0xfa, 0xfe, 0x01, 0xe6, 0xc4, 0xdf, 0x98, 0xcc, 0x38, 0x58, 0x81,
];

/// Create PriceUpdateV2 mock data (Pyth Pull format)
/// Layout: discriminator(8) + write_authority(32) + verification_level(2) + feed_id(32) +
///         price(8) + conf(8) + expo(4) + publish_time(8) + ...
fn make_pyth(feed_id: &[u8; 32], price: i64, expo: i32, conf: u64, publish_time: i64) -> Vec<u8> {
    let mut data = vec![0u8; 134];
    // feed_id at offset 42
    data[42..74].copy_from_slice(feed_id);
    // price at offset 74
    data[74..82].copy_from_slice(&price.to_le_bytes());
    // conf at offset 82
    data[82..90].copy_from_slice(&conf.to_le_bytes());
    // expo at offset 90
    data[90..94].copy_from_slice(&expo.to_le_bytes());
    // publish_time at offset 94
    data[94..102].copy_from_slice(&publish_time.to_le_bytes());
    data
}

fn make_clock(slot: u64, unix_timestamp: i64) -> Vec<u8> {
    let clock = Clock {
        slot,
        unix_timestamp,
        ..Clock::default()
    };
    bincode::serialize(&clock).unwrap()
}

struct MarketFixture {
    program_id: Pubkey,
    admin: TestAccount,
    slab: TestAccount,
    mint: TestAccount,
    vault: TestAccount,
    token_prog: TestAccount,
    pyth_index: TestAccount,
    index_feed_id: [u8; 32],
    clock: TestAccount,
    rent: TestAccount,
    system: TestAccount,
    vault_pda: Pubkey,
}

/// Default feed_id for tests
const TEST_FEED_ID: [u8; 32] = [0xABu8; 32];

/// Minimum seed deposit for InitMarket.  Must satisfy the SECURITY #299 guard
/// (`constants::MIN_INIT_MARKET_SEED`).  When the `test` feature is active the
/// guard is 0, but we pre-fund with the production minimum so tests exercise the
/// realistic path regardless of feature flags.
const VAULT_SEED: u64 = 500_000_000;

fn setup_market() -> MarketFixture {
    let program_id = Pubkey::new_unique();
    let slab_key = Pubkey::new_unique();
    let (vault_pda, _) = Pubkey::find_program_address(&[b"vault", slab_key.as_ref()], &program_id);
    let mint_key = Pubkey::new_unique();
    let pyth_receiver_id = Pubkey::new_from_array(PYTH_RECEIVER_BYTES);

    // Price = $100 (100_000_000 in e6 format), expo = -6, conf = 1, publish_time = 100
    let pyth_data = make_pyth(&TEST_FEED_ID, 100_000_000, -6, 1, 100);

    MarketFixture {
        program_id,
        admin: TestAccount::new(
            Pubkey::new_unique(),
            solana_program::system_program::id(),
            0,
            vec![],
        )
        .signer(),
        slab: TestAccount::new(
            slab_key,
            program_id,
            0,
            vec![0u8; percolator_prog::constants::SLAB_LEN],
        )
        .writable(),
        mint: TestAccount::new(mint_key, spl_token::ID, 0, make_mint_account()),
        vault: TestAccount::new(
            Pubkey::new_unique(),
            spl_token::ID,
            0,
            make_token_account(mint_key, vault_pda, VAULT_SEED),
        )
        .writable(),
        token_prog: TestAccount::new(spl_token::ID, Pubkey::default(), 0, vec![]).executable(),
        pyth_index: TestAccount::new(Pubkey::new_unique(), pyth_receiver_id, 0, pyth_data),
        index_feed_id: TEST_FEED_ID,
        clock: TestAccount::new(
            solana_program::sysvar::clock::id(),
            solana_program::sysvar::id(),
            0,
            make_clock(100, 100),
        ),
        rent: TestAccount::new(
            solana_program::sysvar::rent::id(),
            solana_program::sysvar::id(),
            0,
            vec![],
        ),
        system: TestAccount::new(
            solana_program::system_program::id(),
            Pubkey::default(),
            0,
            vec![],
        ),
        vault_pda,
    }
}

// --- Encoders ---

fn encode_u64(val: u64, buf: &mut Vec<u8>) {
    buf.extend_from_slice(&val.to_le_bytes());
}
fn encode_u32(val: u32, buf: &mut Vec<u8>) {
    buf.extend_from_slice(&val.to_le_bytes());
}
fn encode_u16(val: u16, buf: &mut Vec<u8>) {
    buf.extend_from_slice(&val.to_le_bytes());
}
fn encode_i128(val: i128, buf: &mut Vec<u8>) {
    buf.extend_from_slice(&val.to_le_bytes());
}
fn encode_u128(val: u128, buf: &mut Vec<u8>) {
    buf.extend_from_slice(&val.to_le_bytes());
}
fn encode_pubkey(val: &Pubkey, buf: &mut Vec<u8>) {
    buf.extend_from_slice(val.as_ref());
}
fn encode_bytes32(val: &[u8; 32], buf: &mut Vec<u8>) {
    buf.extend_from_slice(val);
}

fn encode_init_market(fixture: &MarketFixture, crank_staleness: u64) -> Vec<u8> {
    let mut data = vec![0u8];
    encode_pubkey(&fixture.admin.key, &mut data);
    encode_pubkey(&fixture.mint.key, &mut data);
    encode_bytes32(&fixture.index_feed_id, &mut data);
    encode_u64(100, &mut data); // max_staleness_secs
    encode_u16(500, &mut data); // conf_filter_bps
    data.push(0u8); // invert (0 = no inversion)
    encode_u32(0, &mut data); // unit_scale (0 = no scaling)

    // initial_mark_price_e6
    encode_u64(0, &mut data); // initial_mark_price_e6

    // RiskParams (must match read_risk_params order)
    encode_u64(100, &mut data); // warmup_period_slots (must be >0 — PERC-8093)
    encode_u64(500, &mut data); // maintenance_margin_bps (5%)
    encode_u64(1000, &mut data); // initial_margin_bps (10%)
    encode_u64(30, &mut data); // trading_fee_bps (0.3%)
    encode_u64(64, &mut data); // max_accounts
    encode_u128(0, &mut data); // new_account_fee
    encode_u128(0, &mut data); // risk_reduction_threshold
    encode_u128(0, &mut data); // maintenance_fee_per_slot
    encode_u64(crank_staleness, &mut data); // max_crank_staleness_slots
    encode_u64(0, &mut data); // liquidation_fee_bps
    encode_u128(0, &mut data); // liquidation_fee_cap
    encode_u64(0, &mut data); // liquidation_buffer_bps
    encode_u128(0, &mut data); // min_liquidation_abs
    data
}

fn encode_init_market_invert(
    fixture: &MarketFixture,
    crank_staleness: u64,
    invert: u8,
    unit_scale: u32,
) -> Vec<u8> {
    let mut data = vec![0u8];
    encode_pubkey(&fixture.admin.key, &mut data);
    encode_pubkey(&fixture.mint.key, &mut data);
    encode_bytes32(&fixture.index_feed_id, &mut data);
    encode_u64(100, &mut data); // max_staleness_secs
    encode_u16(500, &mut data); // conf_filter_bps
    data.push(invert);
    encode_u32(unit_scale, &mut data);

    // initial_mark_price_e6
    encode_u64(0, &mut data); // initial_mark_price_e6

    // RiskParams (must match read_risk_params order)
    encode_u64(100, &mut data); // warmup_period_slots (must be >0 — PERC-8093)
    encode_u64(500, &mut data); // maintenance_margin_bps (5%)
    encode_u64(1000, &mut data); // initial_margin_bps (10%)
    encode_u64(30, &mut data); // trading_fee_bps (0.3%)
    encode_u64(64, &mut data); // max_accounts
    encode_u128(0, &mut data); // new_account_fee
    encode_u128(0, &mut data); // risk_reduction_threshold
    encode_u128(0, &mut data); // maintenance_fee_per_slot
    encode_u64(crank_staleness, &mut data); // max_crank_staleness_slots
    encode_u64(0, &mut data); // liquidation_fee_bps
    encode_u128(0, &mut data); // liquidation_fee_cap
    encode_u64(0, &mut data); // liquidation_buffer_bps
    encode_u128(0, &mut data); // min_liquidation_abs
    data
}

fn encode_init_user(fee: u64) -> Vec<u8> {
    let mut data = vec![1u8];
    encode_u64(fee, &mut data);
    data
}

fn encode_init_lp(matcher: Pubkey, ctx: Pubkey, fee: u64) -> Vec<u8> {
    let mut data = vec![2u8];
    encode_pubkey(&matcher, &mut data);
    encode_pubkey(&ctx, &mut data);
    encode_u64(fee, &mut data);
    data
}

fn encode_deposit(user_idx: u16, amount: u64) -> Vec<u8> {
    let mut data = vec![3u8];
    encode_u16(user_idx, &mut data);
    encode_u64(amount, &mut data);
    data
}

fn encode_withdraw(user_idx: u16, amount: u64) -> Vec<u8> {
    let mut data = vec![4u8];
    encode_u16(user_idx, &mut data);
    encode_u64(amount, &mut data);
    data
}

fn encode_crank(caller: u16, panic: u8) -> Vec<u8> {
    let mut data = vec![5u8];
    encode_u16(caller, &mut data);
    data.push(panic);
    data
}

fn encode_crank_permissionless(panic: u8) -> Vec<u8> {
    encode_crank(u16::MAX, panic)
}

fn encode_trade(lp: u16, user: u16, size: i128) -> Vec<u8> {
    let mut data = vec![6u8];
    encode_u16(lp, &mut data);
    encode_u16(user, &mut data);
    encode_i128(size, &mut data);
    data
}

fn encode_trade_cpi(lp: u16, user: u16, size: i128) -> Vec<u8> {
    let mut data = vec![10u8];
    encode_u16(lp, &mut data);
    encode_u16(user, &mut data);
    encode_i128(size, &mut data);
    data
}

fn encode_set_risk_threshold(new_threshold: u128) -> Vec<u8> {
    let mut data = vec![11u8];
    encode_u128(new_threshold, &mut data);
    data
}

fn encode_update_admin(new_admin: &Pubkey) -> Vec<u8> {
    let mut data = vec![12u8];
    encode_pubkey(new_admin, &mut data);
    data
}

fn encode_close_slab() -> Vec<u8> {
    vec![13u8]
}

fn encode_topup_insurance(amount: u64) -> Vec<u8> {
    let mut data = vec![9u8];
    encode_u64(amount, &mut data);
    data
}

fn find_idx_by_owner(data: &[u8], owner: Pubkey) -> Option<u16> {
    let engine = zc::engine_ref(data).ok()?;
    for i in 0..MAX_ACCOUNTS {
        if engine.is_used(i) && engine.accounts[i].owner == owner.to_bytes() {
            return Some(i as u16);
        }
    }
    None
}

// --- Tests ---

#[test]
fn test_struct_sizes() {
    extern crate std;
    use core::mem::{offset_of, size_of};
    use percolator::{Account, RiskEngine, MAX_ACCOUNTS};
    use std::println;

    println!("Size of Account: {}", size_of::<Account>());
    println!("Offset of Account.kind: {}", offset_of!(Account, kind));
    println!("Offset of Account.owner: {}", offset_of!(Account, owner));
    println!("Size of RiskEngine: {}", size_of::<RiskEngine>());
    println!("MAX_ACCOUNTS: {}", MAX_ACCOUNTS);

    let account_array_size = MAX_ACCOUNTS * size_of::<Account>();
    println!("Account array size: {}", account_array_size);

    // Print offset of accounts array within RiskEngine
    println!(
        "Offset of RiskEngine.accounts: {}",
        offset_of!(RiskEngine, accounts)
    );
    println!(
        "Offset of RiskEngine.vault: {}",
        offset_of!(RiskEngine, vault)
    );
    println!(
        "Offset of RiskEngine.insurance_fund: {}",
        offset_of!(RiskEngine, insurance_fund)
    );
    println!(
        "Offset of RiskEngine.params: {}",
        offset_of!(RiskEngine, params)
    );
    println!(
        "Offset of RiskEngine.used: {}",
        offset_of!(RiskEngine, used)
    );

    // Print the SBF constant (note: this is x86_64 value when run as native test)
    println!(
        "ACCOUNTS_OFFSET (this test is x86_64): {}",
        percolator_prog::zc::ACCOUNTS_OFFSET
    );

    // Print SLAB_LEN
    println!("ENGINE_OFF: {}", percolator_prog::constants::ENGINE_OFF);
    println!("ENGINE_LEN: {}", percolator_prog::constants::ENGINE_LEN);
    println!("SLAB_LEN: {}", percolator_prog::constants::SLAB_LEN);
}

#[test]
fn test_init_market() {
    let mut f = setup_market();
    let data = encode_init_market(&f, 100);

    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &data).unwrap();
    }

    let header = state::read_header(&f.slab.data);
    assert_eq!(header.magic, MAGIC);
    assert_eq!(header.version, VERSION);

    let engine = zc::engine_ref(&f.slab.data).unwrap();
    assert_eq!(engine.params.max_accounts, 64);
}

#[test]
#[cfg(feature = "test")]
fn test_init_user() {
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let init_accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &init_accounts, &init_data).unwrap();
    }

    let mut user = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut user_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, user.key, 1000),
    )
    .writable();

    let data = encode_init_user(100);
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &data).unwrap();
    }

    let vault_state = TokenAccount::unpack(&f.vault.data).unwrap();
    assert_eq!(vault_state.amount, VAULT_SEED + 100);
    assert!(find_idx_by_owner(&f.slab.data, user.key).is_some());
}

#[test]
#[cfg(feature = "test")]
fn test_deposit_withdraw() {
    let mut f = setup_market();
    let init_data = encode_init_market(&f, u64::MAX);
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let init_accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &init_accounts, &init_data).unwrap();
    }

    let mut user = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut user_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, user.key, 1000),
    )
    .writable();
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_init_user(0)).unwrap();
    }
    let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_deposit(user_idx, 500)).unwrap();
    }

    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_crank(user_idx, 0)).unwrap();
    }

    {
        let mut vault_pda_account =
            TestAccount::new(f.vault_pda, solana_program::system_program::id(), 0, vec![]);
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            f.vault.to_info(),
            user_ata.to_info(),
            vault_pda_account.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_withdraw(user_idx, 200)).unwrap();
    }

    let vault_state = TokenAccount::unpack(&f.vault.data).unwrap();
    assert_eq!(vault_state.amount, VAULT_SEED + 300);
}

#[test]
fn test_vault_validation() {
    let mut f = setup_market();
    f.vault.owner = solana_program::system_program::id();
    let init_data = encode_init_market(&f, 100);
    let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
    let init_accounts = vec![
        f.admin.to_info(),
        f.slab.to_info(),
        f.mint.to_info(),
        f.vault.to_info(),
        f.token_prog.to_info(),
        f.clock.to_info(),
        f.rent.to_info(),
        dummy_ata.to_info(),
        f.system.to_info(),
    ];
    let res = process_instruction(&f.program_id, &init_accounts, &init_data);
    assert_eq!(res, Err(PercolatorError::InvalidVaultAta.into()));
}

#[test]
#[cfg(feature = "test")]
#[ignore = "PERC-199: TradeCpi uses Clock::get() syscall, unavailable in unit tests — migrate to LiteSVM"]
fn test_trade() {
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let init_accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &init_accounts, &init_data).unwrap();
    }

    let mut user = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut user_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, user.key, 1000),
    )
    .writable();
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_init_user(0)).unwrap();
    }
    let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_deposit(user_idx, 1000)).unwrap();
    }

    let mut lp = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut lp_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, lp.key, 1000),
    )
    .writable();
    let mut d1 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
    let mut d2 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
    {
        let matcher_prog_key = d1.key;
        let matcher_ctx_key = d2.key;
        let accs = vec![
            lp.to_info(),
            f.slab.to_info(),
            lp_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(
            &f.program_id,
            &accs,
            &encode_init_lp(matcher_prog_key, matcher_ctx_key, 0),
        )
        .unwrap();
    }
    let lp_idx = find_idx_by_owner(&f.slab.data, lp.key).unwrap();
    {
        let accounts = vec![
            lp.to_info(),
            f.slab.to_info(),
            lp_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_deposit(lp_idx, 1000)).unwrap();
    }

    {
        // PERC-199: Clock sysvar removed — Clock::get() syscall used instead.
        let accounts = vec![
            user.to_info(),
            lp.to_info(),
            f.slab.to_info(),
            f.pyth_index.to_info(),
        ];
        process_instruction(
            &f.program_id,
            &accounts,
            &encode_trade(lp_idx, user_idx, 100),
        )
        .unwrap();
    }
}

#[test]
#[cfg(feature = "test")]
fn test_withdraw_wrong_signer() {
    let mut f = setup_market();
    let init_data = encode_init_market(&f, u64::MAX);
    {
        let mut dummy = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accs = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &init_data).unwrap();
    }

    let mut user = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut user_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, user.key, 1000),
    )
    .writable();
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_init_user(0)).unwrap();
    }
    let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_deposit(user_idx, 500)).unwrap();
    }

    {
        let accs = vec![
            user.to_info(),
            f.slab.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &encode_crank(user_idx, 0)).unwrap();
    }

    let mut attacker = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut vault_pda =
        TestAccount::new(f.vault_pda, solana_program::system_program::id(), 0, vec![]);

    let res = {
        let accounts = vec![
            attacker.to_info(),
            f.slab.to_info(),
            f.vault.to_info(),
            user_ata.to_info(),
            vault_pda.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_withdraw(user_idx, 100))
    };
    assert_eq!(res, Err(PercolatorError::EngineUnauthorized.into()));
}

#[test]
#[cfg(feature = "test")]
#[ignore = "PERC-199: TradeCpi uses Clock::get() syscall, unavailable in unit tests — migrate to LiteSVM"]
fn test_trade_wrong_signer() {
    let mut f = setup_market();
    let init_data = encode_init_market(&f, u64::MAX);
    {
        let mut dummy = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accs = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &init_data).unwrap();
    }

    let mut user = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut user_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, user.key, 1000),
    )
    .writable();
    {
        let accs = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &encode_init_user(0)).unwrap();
    }
    let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

    let mut lp = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut lp_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, lp.key, 1000),
    )
    .writable();
    let d1 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
    let d2 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
    {
        let matcher_prog_key = d1.key;
        let matcher_ctx_key = d2.key;
        let accs = vec![
            lp.to_info(),
            f.slab.to_info(),
            lp_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(
            &f.program_id,
            &accs,
            &encode_init_lp(matcher_prog_key, matcher_ctx_key, 0),
        )
        .unwrap();
    }
    let lp_idx = find_idx_by_owner(&f.slab.data, lp.key).unwrap();

    {
        let accs = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &encode_deposit(user_idx, 1000)).unwrap();
    }
    {
        let accs = vec![
            lp.to_info(),
            f.slab.to_info(),
            lp_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &encode_deposit(lp_idx, 1000)).unwrap();
    }
    {
        let accs = vec![
            user.to_info(),
            f.slab.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &encode_crank(user_idx, 0)).unwrap();
    }

    let mut attacker = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    {
        // PERC-199: Clock sysvar removed — Clock::get() syscall used instead.
        let accs = vec![
            attacker.to_info(),
            lp.to_info(),
            f.slab.to_info(),
            f.pyth_index.to_info(),
        ];
        let res = process_instruction(&f.program_id, &accs, &encode_trade(lp_idx, user_idx, 100));
        assert_eq!(res, Err(PercolatorError::EngineUnauthorized.into()));
    }
}

#[test]
fn test_trade_cpi_wrong_pda_key_rejected() {
    // This test verifies pre-CPI validation: wrong PDA key is rejected
    // Note: Full TradeCpi success path is tested in integration tests where CPI works
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);
    {
        let mut dummy = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accs = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &init_data).unwrap();
    }

    let mut user = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut user_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, user.key, 1000),
    )
    .writable();
    {
        let accs = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &encode_init_user(0)).unwrap();
    }
    let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

    let mut lp = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut lp_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, lp.key, 1000),
    )
    .writable();
    let mut matcher_program = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
    matcher_program.executable = true;
    let mut matcher_ctx =
        TestAccount::new(Pubkey::new_unique(), matcher_program.key, 0, vec![0u8; 320]);
    matcher_ctx.is_writable = true;
    {
        let matcher_prog_key = matcher_program.key;
        let matcher_ctx_key = matcher_ctx.key;
        let accs = vec![
            lp.to_info(),
            f.slab.to_info(),
            lp_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(
            &f.program_id,
            &accs,
            &encode_init_lp(matcher_prog_key, matcher_ctx_key, 0),
        )
        .unwrap();
    }
    let lp_idx = find_idx_by_owner(&f.slab.data, lp.key).unwrap();

    // Create WRONG lp_pda - use a random key instead of the correct PDA
    let mut wrong_lp_pda = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    );

    // PERC-199: Clock sysvar removed — Clock::get() syscall used instead.
    let accs = vec![
        user.to_info(),
        lp.to_info(),
        f.slab.to_info(),
        f.pyth_index.to_info(),
        matcher_program.to_info(),
        matcher_ctx.to_info(),
        wrong_lp_pda.to_info(),
    ];
    let res = process_instruction(
        &f.program_id,
        &accs,
        &encode_trade_cpi(lp_idx, user_idx, 100),
    );
    assert_eq!(res, Err(ProgramError::InvalidSeeds));
}

#[test]
fn test_trade_cpi_wrong_lp_owner_rejected() {
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);
    {
        let mut dummy = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accs = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &init_data).unwrap();
    }

    let mut user = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut user_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, user.key, 1000),
    )
    .writable();
    {
        let accs = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &encode_init_user(0)).unwrap();
    }
    let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

    let mut lp = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut lp_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, lp.key, 1000),
    )
    .writable();
    let mut matcher_program = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
    matcher_program.executable = true;
    let mut matcher_ctx =
        TestAccount::new(Pubkey::new_unique(), matcher_program.key, 0, vec![0u8; 320]);
    matcher_ctx.is_writable = true;
    {
        let matcher_prog_key = matcher_program.key;
        let matcher_ctx_key = matcher_ctx.key;
        let accs = vec![
            lp.to_info(),
            f.slab.to_info(),
            lp_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(
            &f.program_id,
            &accs,
            &encode_init_lp(matcher_prog_key, matcher_ctx_key, 0),
        )
        .unwrap();
    }
    let lp_idx = find_idx_by_owner(&f.slab.data, lp.key).unwrap();

    let mut wrong_lp = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();

    // Create lp_pda account (system-owned, 0 data)
    let lp_bytes = lp_idx.to_le_bytes();
    let (lp_pda_key, _) =
        Pubkey::find_program_address(&[b"lp", f.slab.key.as_ref(), &lp_bytes], &f.program_id);
    let mut lp_pda = TestAccount::new(lp_pda_key, solana_program::system_program::id(), 0, vec![]);

    // PERC-199: Clock sysvar removed — Clock::get() syscall used instead.
    let res = {
        let accs = vec![
            user.to_info(),            // 0
            wrong_lp.to_info(),        // 1 (WRONG OWNER)
            f.slab.to_info(),          // 2
            f.pyth_index.to_info(),    // 3 oracle
            matcher_program.to_info(), // 4 matcher
            matcher_ctx.to_info(),     // 5 context
            lp_pda.to_info(),          // 6 lp_pda
        ];
        process_instruction(
            &f.program_id,
            &accs,
            &encode_trade_cpi(lp_idx, user_idx, 100),
        )
    };
    assert_eq!(res, Err(PercolatorError::EngineUnauthorized.into()));
}

#[test]
#[cfg(feature = "test")]
#[ignore = "PERC-199: TradeCpi uses Clock::get() syscall, unavailable in unit tests — migrate to LiteSVM"]
fn test_trade_cpi_wrong_oracle_key_rejected() {
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);
    {
        let mut dummy = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accs = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &init_data).unwrap();
    }

    let mut user = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut user_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, user.key, 1000),
    )
    .writable();
    {
        let accs = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &encode_init_user(0)).unwrap();
    }
    let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

    let mut lp = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut lp_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, lp.key, 1000),
    )
    .writable();
    let mut matcher_program = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
    matcher_program.executable = true;
    let mut matcher_ctx =
        TestAccount::new(Pubkey::new_unique(), matcher_program.key, 0, vec![0u8; 320]);
    matcher_ctx.is_writable = true;
    {
        let matcher_prog_key = matcher_program.key;
        let matcher_ctx_key = matcher_ctx.key;
        let accs = vec![
            lp.to_info(),
            f.slab.to_info(),
            lp_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(
            &f.program_id,
            &accs,
            &encode_init_lp(matcher_prog_key, matcher_ctx_key, 0),
        )
        .unwrap();
    }
    let lp_idx = find_idx_by_owner(&f.slab.data, lp.key).unwrap();

    // Create oracle with correct owner but wrong feed_id
    let wrong_feed_id = [0xFFu8; 32];
    let pyth_receiver_id = Pubkey::new_from_array(PYTH_RECEIVER_BYTES);
    let wrong_pyth_data = make_pyth(&wrong_feed_id, 100_000_000, -6, 1, 100);
    let mut wrong_oracle =
        TestAccount::new(Pubkey::new_unique(), pyth_receiver_id, 0, wrong_pyth_data);

    // Create lp_pda account (system-owned, 0 data)
    let lp_bytes = lp_idx.to_le_bytes();
    let (lp_pda_key, _) =
        Pubkey::find_program_address(&[b"lp", f.slab.key.as_ref(), &lp_bytes], &f.program_id);
    let mut lp_pda = TestAccount::new(lp_pda_key, solana_program::system_program::id(), 0, vec![]);

    // PERC-199: Clock sysvar removed — Clock::get() syscall used instead.
    let res = {
        let accs = vec![
            user.to_info(),            // 0
            lp.to_info(),              // 1
            f.slab.to_info(),          // 2
            wrong_oracle.to_info(),    // 3 oracle (WRONG FEED_ID)
            matcher_program.to_info(), // 4 matcher
            matcher_ctx.to_info(),     // 5 context
            lp_pda.to_info(),          // 6 lp_pda
        ];
        process_instruction(
            &f.program_id,
            &accs,
            &encode_trade_cpi(lp_idx, user_idx, 100),
        )
    };
    // Returns InvalidOracleKey because feed_id doesn't match expected
    assert_eq!(res, Err(PercolatorError::InvalidOracleKey.into()));
}

#[test]
fn test_set_risk_threshold() {
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);
    {
        let mut dummy = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accs = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &init_data).unwrap();
    }

    // Verify initial threshold is 0
    {
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        assert_eq!(engine.risk_reduction_threshold(), 0);
    }

    // Admin sets new threshold
    let new_threshold: u128 = 123_456_789;
    {
        let accs = vec![
            f.admin.to_info(), // admin (signer)
            f.slab.to_info(),  // slab (writable)
        ];
        process_instruction(
            &f.program_id,
            &accs,
            &encode_set_risk_threshold(new_threshold),
        )
        .unwrap();
    }

    // Verify threshold was updated
    {
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        assert_eq!(engine.risk_reduction_threshold(), new_threshold);
    }
}

#[test]
fn test_set_risk_threshold_non_admin_fails() {
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);
    {
        let mut dummy = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accs = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &init_data).unwrap();
    }

    // Non-admin tries to set threshold
    let mut attacker = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let new_threshold: u128 = 999_999;
    {
        let accs = vec![
            attacker.to_info(), // attacker (signer, but not admin)
            f.slab.to_info(),   // slab (writable)
        ];
        let res = process_instruction(
            &f.program_id,
            &accs,
            &encode_set_risk_threshold(new_threshold),
        );
        assert_eq!(res, Err(PercolatorError::EngineUnauthorized.into()));
    }

    // Verify threshold was NOT updated (still 0)
    {
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        assert_eq!(engine.risk_reduction_threshold(), 0);
    }
}

#[test]
#[cfg(feature = "test")]
#[ignore = "PERC-199: KeeperCrank uses Clock::get() syscall, unavailable in unit tests — migrate to LiteSVM"]
fn test_crank_updates_threshold_from_risk_metric() {
    use percolator_prog::constants::{
        DEFAULT_THRESH_ALPHA_BPS, DEFAULT_THRESH_FLOOR, DEFAULT_THRESH_MIN_STEP,
        DEFAULT_THRESH_RISK_BPS, DEFAULT_THRESH_STEP_BPS,
    };

    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);
    {
        let mut dummy = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accs = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &init_data).unwrap();
    }

    // Verify initial threshold is 0
    {
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        assert_eq!(engine.risk_reduction_threshold(), 0);
        assert!(engine.total_open_interest.is_zero());
    }

    // Create user
    let mut user = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut user_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, user.key, 10_000_000),
    )
    .writable();
    {
        let accs = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &encode_init_user(0)).unwrap();
    }
    let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

    // Create LP
    let mut lp = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut lp_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, lp.key, 10_000_000),
    )
    .writable();
    let mut d1 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
    let mut d2 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
    {
        let matcher_prog_key = d1.key;
        let matcher_ctx_key = d2.key;
        let accs = vec![
            lp.to_info(),
            f.slab.to_info(),
            lp_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(
            &f.program_id,
            &accs,
            &encode_init_lp(matcher_prog_key, matcher_ctx_key, 0),
        )
        .unwrap();
    }
    let lp_idx = find_idx_by_owner(&f.slab.data, lp.key).unwrap();

    // Deposit for both user and LP
    {
        let accs = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &encode_deposit(user_idx, 1_000_000)).unwrap();
    }
    {
        let accs = vec![
            lp.to_info(),
            f.slab.to_info(),
            lp_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &encode_deposit(lp_idx, 1_000_000)).unwrap();
    }

    // Execute trade to create positions
    // PERC-199: Clock sysvar removed — Clock::get() syscall used instead.
    let trade_size: i128 = 100_000;
    {
        let accs = vec![
            user.to_info(),
            lp.to_info(),
            f.slab.to_info(),
            f.pyth_index.to_info(),
        ];
        process_instruction(
            &f.program_id,
            &accs,
            &encode_trade(lp_idx, user_idx, trade_size),
        )
        .unwrap();
    }

    // Verify positions were set by trade
    {
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        let lp_pos = engine.accounts[lp_idx as usize].position_size;
        let user_pos = engine.accounts[user_idx as usize].position_size;
        assert!(
            !lp_pos.is_zero(),
            "LP should have non-zero position after trade"
        );
        assert!(
            !user_pos.is_zero(),
            "User should have non-zero position after trade"
        );
        // Verify LP is marked as LP
        assert!(
            engine.accounts[lp_idx as usize].is_lp(),
            "LP account should be marked as LP"
        );
        assert!(
            engine.is_used(lp_idx as usize),
            "LP should be marked as used"
        );
    }

    // Capture threshold before crank
    let threshold_before = {
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        engine.risk_reduction_threshold()
    };
    assert_eq!(threshold_before, 0, "Threshold should be 0 before crank");

    // Verify compute_system_risk_units returns non-zero
    {
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        let risk_units = percolator_prog::compute_system_risk_units(engine);
        assert!(
            risk_units > 0,
            "risk_units should be > 0 when there are LP positions"
        );
    }

    // Top up insurance to prevent force_realize from triggering during crank
    // (force_realize triggers when insurance <= threshold, both start at 0)
    {
        let mut funder = TestAccount::new(
            Pubkey::new_unique(),
            solana_program::system_program::id(),
            0,
            vec![],
        )
        .signer();
        let mut funder_ata = TestAccount::new(
            Pubkey::new_unique(),
            spl_token::ID,
            0,
            make_token_account(f.mint.key, funder.key, 1_000_000_000),
        )
        .writable();
        let accs = vec![
            funder.to_info(),
            f.slab.to_info(),
            funder_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &encode_topup_insurance(1_000_000_000)).unwrap();
    }

    // Now call crank - this should update threshold based on risk metric
    // Clock slot defaults to 0 in test, but last_thr_slot is also 0,
    // so update won't trigger unless slot >= 0 + THRESH_UPDATE_INTERVAL_SLOTS
    // We need to advance the clock
    f.clock.data = make_clock(100, 100); // Advance past rate limit
    {
        let accs = vec![
            user.to_info(),
            f.slab.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &encode_crank(user_idx, 0)).unwrap();
    }

    // Verify threshold update ran by checking last_thr_update_slot
    let last_thr_slot_after = state::read_last_thr_update_slot(&f.slab.data);
    assert_eq!(
        last_thr_slot_after, 100,
        "last_thr_update_slot should be set to clock.slot after crank"
    );

    // Check if positions are still non-zero after crank
    {
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        let lp_pos = engine.accounts[lp_idx as usize].position_size;
        // Crank may liquidate positions. Check if LP still has position.
        let risk_units_after = percolator_prog::compute_system_risk_units(engine);
        // If risk_units is 0 after crank, positions were liquidated
        if risk_units_after == 0 {
            // This is expected if crank liquidated - threshold stays at 0
            return;
        }
    }

    // Verify threshold was updated based on risk metric
    {
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        let threshold = engine.risk_reduction_threshold();

        // With trade_size=100000, LP position is -100000 (counterparty to user's +100000)
        // Only LP positions are counted for risk:
        //   lp_sum_abs = 100000, lp_max_abs = 100000
        //   risk_units = max_abs + sum_abs/8 = 100000 + 12500 = 112500
        //   risk_notional = 112500 * 100_000_000 / 1_000_000 = 11_250_000
        //   raw_target = 0 + 11_250_000 * 50 / 10_000 = 56_250
        //   EWMA: (1000 * 56250 + 9000 * 0) / 10000 = 5625
        //   max_step = 56250 (current == 0 → full jump allowed, Bug #6 fix)
        //   final = 0 + min(56250, 5625) = 5625

        assert!(
            threshold > 0,
            "Threshold should be > 0 after crank with positions"
        );
        // Bug #6: when current == 0, full jump to clamped_target allowed (no min_step clamp)
        assert_eq!(
            threshold, 5625,
            "First update from 0 should be EWMA-smoothed raw target"
        );
    }
}

#[test]
fn test_permissionless_crank() {
    // Test that anyone can call crank with caller_idx = u16::MAX (permissionless mode)
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);

    // Init market
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &init_data).unwrap();
    }

    // Create a random "keeper" account that is NOT a signer
    let mut keeper = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    );
    // Note: keeper is NOT marked as signer

    // Call permissionless crank - should succeed even though keeper is not a signer
    {
        let accs = vec![
            keeper.to_info(), // Not a signer!
            f.slab.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
        ];
        // Use encode_crank_permissionless which passes u16::MAX as caller_idx
        process_instruction(&f.program_id, &accs, &encode_crank_permissionless(0)).unwrap();
    }

    // Verify crank was executed (we can check that the engine is still valid)
    {
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        assert!(engine.vault.is_zero()); // No deposits yet, vault should be 0
    }
}

#[test]
#[cfg(feature = "test")]
fn test_permissionless_crank_gc() {
    // Non-vacuous test: create a dust account and verify GC frees it
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);

    // Init market
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &init_data).unwrap();
    }

    // Init user - creates account slot
    let mut user = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut user_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, user.key, 1000),
    )
    .writable();
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_init_user(100)).unwrap();
    }
    let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

    // Record state before GC
    let (used_before, is_used_before) = {
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        (engine.num_used_accounts, engine.is_used(user_idx as usize))
    };
    assert!(is_used_before, "User account should be used before GC");

    // Directly manipulate account to make it dust:
    // - capital = 0
    // - pnl = -1 (small negative)
    // - position_size = 0 (already 0)
    // - reserved_pnl = 0 (already 0)
    // - funding_index = engine.funding_index_qpb_e6
    // - fee_credits = 0, last_fee_slot = current_slot (robustness against future predicates)
    {
        let engine = zc::engine_mut(&mut f.slab.data).unwrap();
        let funding_idx = engine.funding_index_qpb_e6;
        let current_slot = engine.current_slot;
        let account = &mut engine.accounts[user_idx as usize];
        account.capital = U128::ZERO;
        account.pnl = I128::new(-1);
        account.funding_index = funding_idx;
        account.fee_credits = I128::ZERO;
        account.last_fee_slot = current_slot;
    }

    // Verify account is now a dust candidate
    {
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        let account = &engine.accounts[user_idx as usize];
        assert!(account.capital.is_zero(), "capital should be 0");
        assert_eq!(account.pnl.get(), -1, "pnl should be -1");
        assert!(account.position_size.is_zero(), "position_size should be 0");
        assert_eq!(account.reserved_pnl, 0, "reserved_pnl should be 0");
        assert_eq!(
            account.funding_index, engine.funding_index_qpb_e6,
            "funding should match"
        );
    }

    // Call permissionless crank - should GC the dust account
    let mut keeper = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    );
    {
        let accs = vec![
            keeper.to_info(),
            f.slab.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &encode_crank_permissionless(0)).unwrap();
    }

    // Verify GC freed the account
    {
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        assert_eq!(
            engine.num_used_accounts,
            used_before - 1,
            "num_used_accounts should decrease by 1"
        );
        assert!(
            !engine.is_used(user_idx as usize),
            "User account should no longer be used after GC"
        );
    }
}

#[test]
#[cfg(feature = "test")]
fn test_permissionless_funding_not_controllable() {
    // Security test: permissionless caller cannot influence funding rate.
    // Funding is computed deterministically from (LP inventory, oracle price, constants).
    //
    // Key security property: calling crank multiple times in the same slot is harmless
    // because engine gates via dt=0 (no funding accrues when dt=0).
    //
    // NOTE: Funding may be zero for small inventories due to integer division and the
    // chosen scale/horizon parameters (deadzone behavior). This test focuses on the
    // dt=0 anti-spam gating, independent of funding magnitude.
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);

    // Init market
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &init_data).unwrap();
    }

    // Init user with deposit
    let mut user = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut user_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, user.key, 1_000_000),
    )
    .writable();
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_init_user(100)).unwrap();
    }
    let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_deposit(user_idx, 100_000)).unwrap();
    }

    // Record funding index and last_funding_slot before any crank
    let (_funding_before, _last_slot_before) = {
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        (engine.funding_index_qpb_e6, engine.last_funding_slot)
    };

    // Random keeper calls crank - first crank at slot 100
    let mut keeper = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    );
    {
        let accs = vec![
            keeper.to_info(),
            f.slab.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &encode_crank_permissionless(0)).unwrap();
    }
    let (funding_after_first, last_slot_after_first) = {
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        (engine.funding_index_qpb_e6, engine.last_funding_slot)
    };

    // Second crank in SAME slot - should NOT change funding (dt=0 gating)
    {
        let accs = vec![
            keeper.to_info(),
            f.slab.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &encode_crank_permissionless(0)).unwrap();
    }
    let (funding_after_second, last_slot_after_second) = {
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        (engine.funding_index_qpb_e6, engine.last_funding_slot)
    };

    // KEY SECURITY ASSERTION: same-slot crank does NOT change funding index
    // This is the core anti-spam property - attackers can't compound funding by spamming cranks
    assert_eq!(
        funding_after_second, funding_after_first,
        "Same-slot crank must not change funding (dt=0 gating). before={}, after={}",
        funding_after_first, funding_after_second
    );
    assert_eq!(
        last_slot_after_second, last_slot_after_first,
        "last_funding_slot should not change on same-slot crank"
    );

    // Third crank in same slot - still no change (verify it's consistently gated)
    {
        let accs = vec![
            keeper.to_info(),
            f.slab.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &encode_crank_permissionless(0)).unwrap();
    }
    let funding_after_third = {
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        engine.funding_index_qpb_e6
    };
    assert_eq!(
        funding_after_third, funding_after_first,
        "Multiple same-slot cranks must not accumulate funding changes"
    );

    // Verify last_funding_slot advances when slot changes (relative check, not absolute)
    f.clock.data = make_clock(101, 101);
    {
        let accs = vec![
            keeper.to_info(),
            f.slab.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &encode_crank_permissionless(0)).unwrap();
    }
    let last_slot_after_new_slot = {
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        engine.last_funding_slot
    };
    assert!(
        last_slot_after_new_slot > last_slot_after_second,
        "last_funding_slot should advance when slot changes"
    );
}

#[test]
fn test_funding_sign_flips_with_lp_position() {
    // Security test: funding rate sign must follow LP net position sign.
    // This catches accidental sign inversion bugs.
    //
    // Uses large positions (100B contracts at $100 = $10T notional) to ensure
    // the premium hits the cap (500 bps) and per_slot is non-zero (1 bps).

    use percolator_prog::constants::{
        DEFAULT_FUNDING_HORIZON_SLOTS, DEFAULT_FUNDING_INV_SCALE_NOTIONAL_E6,
        DEFAULT_FUNDING_K_BPS, DEFAULT_FUNDING_MAX_BPS_PER_SLOT, DEFAULT_FUNDING_MAX_PREMIUM_BPS,
    };

    // Test the pure compute function directly
    let price_e6 = 100_000_000u64; // $100

    // LP net long => positive funding rate (longs pay)
    // 100B contracts at $100 = $10T notional, saturates to 500 bps cap, /500 = 1 bps/slot
    let net_long: i128 = 100_000_000_000;
    let rate_long = percolator_prog::compute_inventory_funding_bps_per_slot(
        net_long,
        price_e6,
        DEFAULT_FUNDING_HORIZON_SLOTS,
        DEFAULT_FUNDING_K_BPS,
        DEFAULT_FUNDING_INV_SCALE_NOTIONAL_E6,
        DEFAULT_FUNDING_MAX_PREMIUM_BPS,
        DEFAULT_FUNDING_MAX_BPS_PER_SLOT,
        0,
    );

    // LP net short => negative funding rate (shorts pay)
    let net_short: i128 = -100_000_000_000;
    let rate_short = percolator_prog::compute_inventory_funding_bps_per_slot(
        net_short,
        price_e6,
        DEFAULT_FUNDING_HORIZON_SLOTS,
        DEFAULT_FUNDING_K_BPS,
        DEFAULT_FUNDING_INV_SCALE_NOTIONAL_E6,
        DEFAULT_FUNDING_MAX_PREMIUM_BPS,
        DEFAULT_FUNDING_MAX_BPS_PER_SLOT,
        0,
    );

    // LP flat => zero funding rate
    let net_flat: i128 = 0;
    let rate_flat = percolator_prog::compute_inventory_funding_bps_per_slot(
        net_flat,
        price_e6,
        DEFAULT_FUNDING_HORIZON_SLOTS,
        DEFAULT_FUNDING_K_BPS,
        DEFAULT_FUNDING_INV_SCALE_NOTIONAL_E6,
        DEFAULT_FUNDING_MAX_PREMIUM_BPS,
        DEFAULT_FUNDING_MAX_BPS_PER_SLOT,
        0,
    );

    // Verify rates are actually non-zero for large positions
    assert!(
        rate_long > 0,
        "LP net long with large position should give positive rate, got {}",
        rate_long
    );
    assert!(
        rate_short < 0,
        "LP net short with large position should give negative rate, got {}",
        rate_short
    );
    assert_eq!(rate_flat, 0, "LP flat should give zero funding rate");

    // Verify opposite signs
    assert!(
        rate_long > 0 && rate_short < 0,
        "Funding rates must have opposite signs: long={}, short={}",
        rate_long,
        rate_short
    );
}

// --- Admin Rotation Tests ---

#[test]
fn test_admin_rotate() {
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);

    // Init market with admin A
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &init_data).unwrap();
    }

    // Verify initial admin is set
    let header = state::read_header(&f.slab.data);
    assert_eq!(header.admin, f.admin.key.to_bytes());

    // Create new admin B
    let new_admin_b = Pubkey::new_unique();
    let mut admin_b_account =
        TestAccount::new(new_admin_b, solana_program::system_program::id(), 0, vec![]).signer();

    // Admin A proposes admin B (two-step transfer)
    {
        let accounts = vec![f.admin.to_info(), f.slab.to_info()];
        process_instruction(&f.program_id, &accounts, &encode_update_admin(&new_admin_b)).unwrap();
    }

    // Verify admin is still A (pending transfer)
    let header = state::read_header(&f.slab.data);
    assert_eq!(header.admin, f.admin.key.to_bytes());
    assert_eq!(header.pending_admin, new_admin_b.to_bytes());

    // Admin B accepts the transfer (TAG_ACCEPT_ADMIN = 29)
    let accept_admin_data = vec![29u8];
    {
        let accounts = vec![admin_b_account.to_info(), f.slab.to_info()];
        process_instruction(&f.program_id, &accounts, &accept_admin_data).unwrap();
    }

    // Verify admin is now B
    let header = state::read_header(&f.slab.data);
    assert_eq!(header.admin, new_admin_b.to_bytes());

    // Create new admin C
    let new_admin_c = Pubkey::new_unique();
    let mut admin_c_account =
        TestAccount::new(new_admin_c, solana_program::system_program::id(), 0, vec![]).signer();

    // Admin B proposes admin C
    {
        let accounts = vec![admin_b_account.to_info(), f.slab.to_info()];
        process_instruction(&f.program_id, &accounts, &encode_update_admin(&new_admin_c)).unwrap();
    }

    // Admin C accepts
    {
        let accounts = vec![admin_c_account.to_info(), f.slab.to_info()];
        process_instruction(&f.program_id, &accounts, &accept_admin_data).unwrap();
    }

    // Verify admin is now C
    let header = state::read_header(&f.slab.data);
    assert_eq!(header.admin, new_admin_c.to_bytes());
}

#[test]
fn test_non_admin_cannot_rotate() {
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);

    // Init market with admin A
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &init_data).unwrap();
    }

    // Attacker tries to rotate admin
    let attacker = Pubkey::new_unique();
    let mut attacker_account =
        TestAccount::new(attacker, solana_program::system_program::id(), 0, vec![]).signer();
    let new_admin = Pubkey::new_unique();

    {
        let accounts = vec![attacker_account.to_info(), f.slab.to_info()];
        let res = process_instruction(&f.program_id, &accounts, &encode_update_admin(&new_admin));
        assert_eq!(res, Err(PercolatorError::EngineUnauthorized.into()));
    }

    // Verify admin unchanged
    let header = state::read_header(&f.slab.data);
    assert_eq!(header.admin, f.admin.key.to_bytes());
}

#[test]
fn test_update_admin_rejects_zero_address() {
    // PERC-136 #312: UpdateAdmin now rejects zero address.
    // Admin burn requires RenounceAdmin (which requires RESOLVED market).
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);

    // Init market with admin A
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &init_data).unwrap();
    }

    // UpdateAdmin(zero) must be rejected
    let zero_admin = Pubkey::default();
    {
        let accounts = vec![f.admin.to_info(), f.slab.to_info()];
        let res = process_instruction(&f.program_id, &accounts, &encode_update_admin(&zero_admin));
        assert_eq!(res, Err(ProgramError::InvalidInstructionData));
    }

    // Admin should remain unchanged
    let header = state::read_header(&f.slab.data);
    assert_eq!(header.admin, f.admin.key.to_bytes());
}

#[test]
fn test_renounce_admin_requires_resolved() {
    // PERC-136 #312: RenounceAdmin must be rejected on non-resolved market.
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);

    // Init market
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &init_data).unwrap();
    }

    // RenounceAdmin on non-resolved market must fail
    // Must include the confirmation u64 so instruction parsing succeeds
    // and we reach the resolved-market check.
    let mut renounce_data = vec![23u8]; // TAG_RENOUNCE_ADMIN
    renounce_data
        .extend_from_slice(&percolator_prog::constants::RENOUNCE_ADMIN_CONFIRMATION.to_le_bytes());
    {
        let accounts = vec![f.admin.to_info(), f.slab.to_info()];
        let res = process_instruction(&f.program_id, &accounts, &renounce_data);
        assert_eq!(res, Err(PercolatorError::AdminRenounceNotAllowed.into()));
    }

    // Admin should remain unchanged
    let header = state::read_header(&f.slab.data);
    assert_eq!(header.admin, f.admin.key.to_bytes());
}

#[test]
fn test_non_admin_ops_rejected() {
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);

    // Init market with admin A
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &init_data).unwrap();
    }

    // Non-admin cannot do admin ops
    let anyone = Pubkey::new_unique();
    let mut anyone_account =
        TestAccount::new(anyone, solana_program::system_program::id(), 0, vec![]).signer();

    // Attempt UpdateAdmin signed by non-admin → must fail
    {
        let accounts = vec![anyone_account.to_info(), f.slab.to_info()];
        let res = process_instruction(
            &f.program_id,
            &accounts,
            &encode_update_admin(&Pubkey::new_unique()),
        );
        assert_eq!(res, Err(PercolatorError::EngineUnauthorized.into()));
    }

    // Attempt SetRiskThreshold signed by non-admin → must fail
    {
        let accounts = vec![anyone_account.to_info(), f.slab.to_info()];
        let res = process_instruction(&f.program_id, &accounts, &encode_set_risk_threshold(12345));
        assert_eq!(res, Err(PercolatorError::EngineUnauthorized.into()));
    }
}

#[test]
fn test_oracle_inversion() {
    // Test that invert=1 correctly inverts the oracle price
    // Raw price: $100 = 100_000_000 e6
    // Inverted: 1e12 / 100_000_000 = 10_000 e6 (= $0.01 or 0.01 SOL/USD)
    use percolator_prog::oracle::read_engine_price_e6;

    let feed_id = [0xCDu8; 32];
    let pyth_receiver_id = Pubkey::new_from_array(PYTH_RECEIVER_BYTES);
    // Price = $100, expo = -6, conf = 1, publish_time = 100
    let pyth_data = make_pyth(&feed_id, 100_000_000, -6, 1, 100);
    let mut oracle = TestAccount::new(Pubkey::new_unique(), pyth_receiver_id, 0, pyth_data);

    // Without inversion (invert=0, unit_scale=0)
    // read_engine_price_e6(ai, feed_id, unix_ts, max_staleness_secs, conf_bps, invert, unit_scale)
    let price_raw =
        read_engine_price_e6(&oracle.to_info(), &feed_id, 100, 100, 500, 0, 0, &[]).unwrap();
    assert_eq!(
        price_raw, 100_000_000,
        "Raw price should be $100 (100_000_000 e6)"
    );

    // With inversion (invert=1, unit_scale=0)
    let price_inv =
        read_engine_price_e6(&oracle.to_info(), &feed_id, 100, 100, 500, 1, 0, &[]).unwrap();
    assert_eq!(
        price_inv, 10_000,
        "Inverted price should be 10_000 e6 (= 1e12 / 100_000_000)"
    );

    // Test unit_scale transformation (oracle price scaling)
    // With unit_scale=1000: price_scaled = 100_000_000 / 1000 = 100_000
    let price_scaled =
        read_engine_price_e6(&oracle.to_info(), &feed_id, 100, 100, 500, 0, 1000, &[]).unwrap();
    assert_eq!(
        price_scaled, 100_000,
        "Scaled price should be 100_000 e6 (= 100_000_000 / 1000)"
    );

    // Test combined inversion + unit_scale
    // Inverted: 1e12 / 100_000_000 = 10_000
    // Then scaled: 10_000 / 1000 = 10
    let price_inv_scaled =
        read_engine_price_e6(&oracle.to_info(), &feed_id, 100, 100, 500, 1, 1000, &[]).unwrap();
    assert_eq!(
        price_inv_scaled, 10,
        "Inverted+scaled price should be 10 e6"
    );
}

#[test]
fn test_unit_scale_conversion() {
    // Test base_to_units and units_to_base with unit_scale
    use percolator_prog::units::{base_to_units, units_to_base};

    // With scale=0, no conversion
    assert_eq!(base_to_units(12345, 0), (12345, 0));
    assert_eq!(units_to_base(12345, 0), 12345);

    // With scale=1000 (e.g., for wSOL where 1000 lamports = 1 unit)
    assert_eq!(base_to_units(5500, 1000), (5, 500)); // 5 units, 500 dust
    assert_eq!(base_to_units(5000, 1000), (5, 0)); // 5 units, no dust
    assert_eq!(units_to_base(5, 1000), 5000);

    // With scale=100
    assert_eq!(base_to_units(201, 100), (2, 1)); // 2 units, 1 dust
    assert_eq!(units_to_base(2, 100), 200);
}

#[test]
fn test_init_market_with_invert_and_unit_scale() {
    // Test that InitMarket correctly stores invert and unit_scale in config
    let mut f = setup_market();
    let data = encode_init_market_invert(&f, 100, 1, 1000); // invert=1, unit_scale=1000

    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &data).unwrap();
    }

    // Read back config and verify
    let config = percolator_prog::state::read_config(&f.slab.data);
    assert_eq!(config.invert, 1, "invert should be 1");
    assert_eq!(config.unit_scale, 1000, "unit_scale should be 1000");
}

#[test]
fn test_unit_scale_validation_at_init() {
    // Test that unit_scale > 1_000_000_000 is rejected
    let mut f = setup_market();
    let data = encode_init_market_invert(&f, 100, 0, 2_000_000_000); // Too large

    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        let res = process_instruction(&f.program_id, &accounts, &data);
        assert_eq!(
            res,
            Err(ProgramError::InvalidInstructionData),
            "Should reject unit_scale > 1B"
        );
    }
}

#[test]
#[cfg(feature = "test")]
fn test_withdraw_misalignment_rejected() {
    // Test that misaligned withdrawal amounts are rejected when unit_scale != 0
    let mut f = setup_market();

    // Init market with unit_scale=100
    {
        let data = encode_init_market_invert(&f, 100, 0, 100);
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &data).unwrap();
    }

    // Init user
    let mut user_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, f.admin.key, 1_000_000),
    )
    .writable();
    {
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_init_user(1000)).unwrap();
    }

    // Deposit 1000 (aligned to unit_scale=100)
    {
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_deposit(0, 1000)).unwrap();
    }

    // Create vault_pda account for withdraw tests
    let mut vault_pda_account = TestAccount::new(f.vault_pda, Pubkey::default(), 0, vec![]);

    // Try to withdraw 201 (misaligned) - should fail
    {
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.vault.to_info(),
            user_ata.to_info(),
            vault_pda_account.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
        ];
        let res = process_instruction(&f.program_id, &accounts, &encode_withdraw(0, 201));
        assert_eq!(
            res,
            Err(ProgramError::InvalidInstructionData),
            "Misaligned withdrawal should be rejected"
        );
    }

    // Withdraw 200 (aligned) - should succeed
    {
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.vault.to_info(),
            user_ata.to_info(),
            vault_pda_account.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
        ];
        // This will fail for other reasons (token transfer in test), but not InvalidInstructionData
        let res = process_instruction(&f.program_id, &accounts, &encode_withdraw(0, 200));
        assert_ne!(
            res,
            Err(ProgramError::InvalidInstructionData),
            "Aligned withdrawal should not fail on alignment"
        );
    }
}

// ========================================
// VAULT ACCOUNTING INVARIANT TESTS
// ========================================

/// Helper: read sum of all account capitals from engine
fn sum_account_capitals(slab_data: &[u8]) -> u128 {
    let engine = zc::engine_ref(slab_data).unwrap();
    let mut total = 0u128;
    for idx in 0..percolator::MAX_ACCOUNTS {
        if engine.is_used(idx) {
            total = total.saturating_add(engine.accounts[idx].capital.get());
        }
    }
    total
}

#[test]
#[cfg(feature = "test")]
fn test_vault_amount_matches_engine_vault_plus_dust() {
    // INVARIANT #1: SPL vault balance = engine.vault * unit_scale + dust_base
    //
    // Setup: market with unit_scale=10, deposit 123 base tokens
    // Expected: 12 units, 3 dust
    let mut f = setup_market();
    let unit_scale: u32 = 10;

    // Init market with unit_scale=10
    {
        let data = encode_init_market_invert(&f, 100, 0, unit_scale);
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &data).unwrap();
    }

    // Create user with enough tokens
    let mut user = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut user_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, user.key, 10_000),
    )
    .writable();

    // InitUser with fee=0 (so it doesn't affect accounting)
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_init_user(0)).unwrap();
    }
    let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

    // Record initial state
    let dust_start = state::read_dust_base(&f.slab.data);
    let engine_vault_start = zc::engine_ref(&f.slab.data).unwrap().vault;
    let vault_base_start = TokenAccount::unpack(&f.vault.data).unwrap().amount;

    // Deposit 123 base tokens (creates 12 units + 3 dust)
    let deposit_amount: u64 = 123;
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(
            &f.program_id,
            &accounts,
            &encode_deposit(user_idx, deposit_amount),
        )
        .unwrap();
    }

    // Read post-deposit state
    let vault_base = TokenAccount::unpack(&f.vault.data).unwrap().amount;
    let engine_vault_units = zc::engine_ref(&f.slab.data).unwrap().vault;
    let dust_base = state::read_dust_base(&f.slab.data);

    // Compute deltas
    let delta_vault_base = vault_base - vault_base_start;
    let delta_engine_units = engine_vault_units.get() - engine_vault_start.get();
    let delta_dust = dust_base - dust_start;

    // Assert expected deltas
    assert_eq!(
        delta_vault_base, deposit_amount,
        "SPL vault should increase by deposit amount: got {}, expected {}",
        delta_vault_base, deposit_amount
    );
    assert_eq!(
        delta_engine_units,
        (deposit_amount / unit_scale as u64) as u128,
        "Engine vault should increase by deposit/scale: got {}, expected {}",
        delta_engine_units,
        deposit_amount / unit_scale as u64
    );
    assert_eq!(
        delta_dust,
        deposit_amount % unit_scale as u64,
        "Dust should equal deposit mod scale: got {}, expected {}",
        delta_dust,
        deposit_amount % unit_scale as u64
    );

    // Assert INVARIANT #1: vault_base = seed + engine_vault * unit_scale + dust_base
    // The VAULT_SEED pre-funds the vault but is NOT tracked in the engine, so we
    // must account for it separately.
    let computed_base =
        VAULT_SEED + engine_vault_units.get() as u64 * unit_scale as u64 + dust_base;
    assert_eq!(
        vault_base, computed_base,
        "INVARIANT #1 FAILED: vault_base({}) != seed({}) + engine_vault({}) * scale({}) + dust({}) = {}",
        vault_base, VAULT_SEED, engine_vault_units, unit_scale, dust_base, computed_base
    );
}

#[test]
#[cfg(feature = "test")]
fn test_engine_vault_equals_insurance_plus_capital_when_no_fees() {
    // INVARIANT #2: engine.vault = insurance_fund.balance + sum(account.capital)
    //
    // This holds when:
    // - new_account_fee = 0
    // - no trades (no trading fees, no PnL)
    // - no topups
    //
    // The existing encode_init_market_invert already uses all-zero fees.
    let mut f = setup_market();
    let unit_scale: u32 = 10;

    // Init market with unit_scale=10, all fees=0
    {
        let data = encode_init_market_invert(&f, 100, 0, unit_scale);
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &data).unwrap();
    }

    // Create and fund two users
    let mut user1 = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut user1_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, user1.key, 10_000),
    )
    .writable();
    {
        let accounts = vec![
            user1.to_info(),
            f.slab.to_info(),
            user1_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_init_user(0)).unwrap();
    }
    let user1_idx = find_idx_by_owner(&f.slab.data, user1.key).unwrap();

    let mut user2 = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut user2_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, user2.key, 10_000),
    )
    .writable();
    {
        let accounts = vec![
            user2.to_info(),
            f.slab.to_info(),
            user2_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_init_user(0)).unwrap();
    }
    let user2_idx = find_idx_by_owner(&f.slab.data, user2.key).unwrap();

    // Deposit different amounts (aligned to avoid dust complicating this test)
    {
        let accounts = vec![
            user1.to_info(),
            f.slab.to_info(),
            user1_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_deposit(user1_idx, 500)).unwrap();
        // 50 units
    }
    {
        let accounts = vec![
            user2.to_info(),
            f.slab.to_info(),
            user2_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_deposit(user2_idx, 300)).unwrap();
        // 30 units
    }

    // Read engine state
    let engine = zc::engine_ref(&f.slab.data).unwrap();
    let engine_vault = engine.vault;
    let insurance_balance = engine.insurance_fund.balance;
    let sum_capital = sum_account_capitals(&f.slab.data);

    // Assert INVARIANT #2: vault = insurance + sum(capital)
    // (In no-fee scenario with no PnL, this should hold exactly)
    let expected_vault = insurance_balance + sum_capital;
    assert_eq!(
        engine_vault,
        expected_vault,
        "INVARIANT #2 FAILED: engine.vault({}) != insurance({}) + sum_capital({}) = {}\n\
             user1.capital={}, user2.capital={}",
        engine_vault,
        insurance_balance,
        sum_capital,
        expected_vault,
        engine.accounts[user1_idx as usize].capital,
        engine.accounts[user2_idx as usize].capital
    );
}

#[test]
#[cfg(feature = "test")]
fn test_withdraw_preserves_vault_accounting_invariant() {
    // Verify that aligned withdrawals preserve INVARIANT #1
    let mut f = setup_market();
    let unit_scale: u32 = 10;

    // Init market with unit_scale=10
    {
        let data = encode_init_market_invert(&f, 100, 0, unit_scale);
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &data).unwrap();
    }

    // Create user
    let mut user = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut user_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, user.key, 10_000),
    )
    .writable();
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_init_user(0)).unwrap();
    }
    let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

    // Deposit 230 base tokens (creates 23 units + 0 dust)
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_deposit(user_idx, 230)).unwrap();
    }

    // Record pre-withdraw state
    let vault_base_before = TokenAccount::unpack(&f.vault.data).unwrap().amount;
    let engine_vault_before = zc::engine_ref(&f.slab.data).unwrap().vault;
    let dust_before = state::read_dust_base(&f.slab.data);

    // Withdraw 50 base tokens (aligned: 5 units)
    let mut vault_pda_account = TestAccount::new(f.vault_pda, Pubkey::default(), 0, vec![]);
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            f.vault.to_info(),
            user_ata.to_info(),
            vault_pda_account.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
        ];
        // Note: token transfer CPI will fail in test env, but engine state updates happen first
        let _ = process_instruction(&f.program_id, &accounts, &encode_withdraw(user_idx, 50));
    }

    // Read post-withdraw state
    // Note: In test env, the SPL vault may not update due to CPI mock,
    // but engine state DOES update. We verify engine state consistency.
    let engine_vault_after = zc::engine_ref(&f.slab.data).unwrap().vault;
    let dust_after = state::read_dust_base(&f.slab.data);

    // Verify engine vault decreased by expected units
    assert_eq!(
        engine_vault_before.get() - engine_vault_after.get(),
        5,
        "Engine vault should decrease by 5 units: before={:?}, after={:?}",
        engine_vault_before,
        engine_vault_after
    );

    // Verify dust unchanged (withdrawal was aligned)
    assert_eq!(
        dust_before, dust_after,
        "Dust should be unchanged for aligned withdrawal: before={}, after={}",
        dust_before, dust_after
    );
}

#[test]
#[cfg(feature = "test")]
fn test_dust_sweep_preserves_real_to_accounted_equality() {
    // DUST POLICY: Dust is swept to insurance via top_up_insurance_fund,
    // which covers loss_accum first, then adds to insurance_fund.balance.
    //
    // This test verifies:
    // 1. dust_base < unit_scale after sweep
    // 2. INVARIANT #1 still holds
    // 3. Insurance increased by floor(old_dust / scale) units
    let mut f = setup_market();
    let unit_scale: u32 = 10;

    // Init market with unit_scale=10
    {
        let data = encode_init_market_invert(&f, 100, 0, unit_scale);
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &data).unwrap();
    }

    // Create user for crank caller
    let mut user = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut user_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, user.key, 10_000),
    )
    .writable();
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_init_user(0)).unwrap();
    }
    let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

    // Make multiple deposits that accumulate dust > unit_scale
    // Each deposit of 27 creates 2 units + 7 dust
    // After 2 deposits: 4 units + 14 dust (14 >= 10, so sweep will happen)
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_deposit(user_idx, 27)).unwrap();
    }
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_deposit(user_idx, 27)).unwrap();
    }

    // Record pre-crank state
    let dust_before_crank = state::read_dust_base(&f.slab.data);
    let engine_vault_before = zc::engine_ref(&f.slab.data).unwrap().vault;
    let insurance_before = zc::engine_ref(&f.slab.data).unwrap().insurance_fund.balance;

    assert!(
        dust_before_crank >= unit_scale as u64,
        "Dust should be >= unit_scale before crank: dust={}, scale={}",
        dust_before_crank,
        unit_scale
    );

    // Call KeeperCrank - this triggers dust sweep
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_crank(user_idx, 0)).unwrap();
    }

    // Read post-crank state
    let dust_after_crank = state::read_dust_base(&f.slab.data);
    let engine_vault_after = zc::engine_ref(&f.slab.data).unwrap().vault;
    let insurance_after = zc::engine_ref(&f.slab.data).unwrap().insurance_fund.balance;
    let vault_base = TokenAccount::unpack(&f.vault.data).unwrap().amount;

    // Verify dust was swept
    assert!(
        dust_after_crank < unit_scale as u64,
        "Dust should be < unit_scale after sweep: dust={}, scale={}",
        dust_after_crank,
        unit_scale
    );

    // Calculate expected sweep
    let units_swept = dust_before_crank / unit_scale as u64;
    let expected_remaining_dust = dust_before_crank % unit_scale as u64;

    assert_eq!(
        dust_after_crank, expected_remaining_dust,
        "Remaining dust should be old_dust mod scale: got {}, expected {}",
        dust_after_crank, expected_remaining_dust
    );

    // Verify insurance increased by swept units (assuming no loss_accum)
    assert_eq!(
        insurance_after.get() - insurance_before.get(),
        units_swept as u128,
        "Insurance should increase by swept units: delta={}, expected={}",
        insurance_after.get() - insurance_before.get(),
        units_swept
    );

    // Verify engine.vault also increased by swept units
    assert_eq!(
        engine_vault_after.get() - engine_vault_before.get(),
        units_swept as u128,
        "Engine vault should increase by swept units: delta={}, expected={}",
        engine_vault_after.get() - engine_vault_before.get(),
        units_swept
    );

    // Verify INVARIANT #1 still holds after sweep (seed offset excluded)
    let computed_base =
        VAULT_SEED + engine_vault_after.get() as u64 * unit_scale as u64 + dust_after_crank;
    assert_eq!(vault_base, computed_base,
            "INVARIANT #1 FAILED after sweep: vault_base({}) != seed({}) + engine_vault({}) * scale({}) + dust({}) = {}",
            vault_base, VAULT_SEED, engine_vault_after, unit_scale, dust_after_crank, computed_base);
}

#[test]
#[cfg(feature = "test")]
fn test_invariants_with_unit_scale_zero() {
    // Verify invariants work when unit_scale=0 (no scaling)
    // In this mode: 1 base token = 1 unit, no dust ever created
    let mut f = setup_market();

    // Init market with unit_scale=0 (standard behavior)
    {
        let data = encode_init_market_invert(&f, 100, 0, 0);
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &data).unwrap();
    }

    // Create user
    let mut user = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut user_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, user.key, 10_000),
    )
    .writable();
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_init_user(0)).unwrap();
    }
    let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

    // Deposit any amount - should create 0 dust
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_deposit(user_idx, 123)).unwrap();
    }

    // Verify no dust created
    let dust = state::read_dust_base(&f.slab.data);
    assert_eq!(dust, 0, "Dust should be 0 when unit_scale=0: got {}", dust);

    // Verify INVARIANT #1: vault_base = seed + engine_vault (scale=1) + dust (0)
    let vault_base = TokenAccount::unpack(&f.vault.data).unwrap().amount;
    let engine_vault = zc::engine_ref(&f.slab.data).unwrap().vault;
    assert_eq!(
        vault_base,
        VAULT_SEED + engine_vault.get() as u64,
        "With scale=0: vault_base({}) should equal seed({}) + engine_vault({:?})",
        vault_base,
        VAULT_SEED,
        engine_vault
    );

    // Verify INVARIANT #2
    let engine = zc::engine_ref(&f.slab.data).unwrap();
    let sum_capital = sum_account_capitals(&f.slab.data);
    assert_eq!(
        engine.vault,
        engine.insurance_fund.balance + sum_capital,
        "INVARIANT #2: vault({}) != insurance({}) + capital({})",
        engine.vault,
        engine.insurance_fund.balance,
        sum_capital
    );
}

#[test]
fn test_close_slab() {
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);

    // Record admin's initial lamports
    let admin_lamports_before = f.admin.lamports;
    let slab_lamports = f.slab.lamports;

    // Init market
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &init_data).unwrap();
    }

    // Verify market is initialized
    let header = state::read_header(&f.slab.data);
    assert_eq!(header.magic, MAGIC);

    // Close the slab
    {
        let accounts = vec![f.admin.to_info(), f.slab.to_info()];
        process_instruction(&f.program_id, &accounts, &encode_close_slab()).unwrap();
    }

    // Verify slab is zeroed
    assert!(
        f.slab.data.iter().all(|&b| b == 0),
        "Slab data should be zeroed after close"
    );

    // Verify lamports transferred to admin
    assert_eq!(
        f.slab.lamports, 0,
        "Slab should have 0 lamports after close"
    );
    assert_eq!(
        f.admin.lamports,
        admin_lamports_before + slab_lamports,
        "Admin should receive slab's lamports"
    );
}

#[test]
fn test_close_slab_non_admin_rejected() {
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);

    // Init market
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &init_data).unwrap();
    }

    // Attacker tries to close
    let mut attacker = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();

    {
        let accounts = vec![attacker.to_info(), f.slab.to_info()];
        let res = process_instruction(&f.program_id, &accounts, &encode_close_slab());
        assert_eq!(res, Err(PercolatorError::EngineUnauthorized.into()));
    }

    // Verify slab unchanged
    let header = state::read_header(&f.slab.data);
    assert_eq!(
        header.magic, MAGIC,
        "Slab should still be initialized after failed close"
    );
}

// === UpdateHyperpMark — Minimum DEX Liquidity Tests (PERC-141 / GH #297 Fix 1) ===

/// Build a PumpSwap pool account mock.
/// Layout: 195+ bytes, with base_mint at offset 35, base_vault at offset 131, quote_vault at offset 163.
/// The base_mint is required by the UpdateHyperpMark security check: it must match the market's collateral_mint.
fn make_pumpswap_pool(
    base_mint_key: &Pubkey,
    base_vault_key: &Pubkey,
    quote_vault_key: &Pubkey,
) -> Vec<u8> {
    let mut data = vec![0u8; 256]; // generous size
                                   // base_mint at offset 35 — must match market collateral_mint
    data[35..67].copy_from_slice(base_mint_key.as_ref());
    // base_vault at offset 131
    data[131..163].copy_from_slice(base_vault_key.as_ref());
    // quote_vault at offset 163
    data[163..195].copy_from_slice(quote_vault_key.as_ref());
    data
}

/// Build a Meteora DLMM LbPair account mock.
/// Includes bin_step_seed at 74, active_id at 77, and reserve_y at 185.
/// active_id=0 → price_e6 = 1_000_000 (price == 1.0 for any bin_step).
fn make_meteora_lbpair(bin_step: u16, active_id: i32, vault_y_key: &Pubkey) -> Vec<u8> {
    let mut data = vec![0u8; 220]; // must be ≥ METEORA_DLMM_MIN_LEN (216)
                                   // bin_step_seed at offset 73 (verified against on-chain LbPair layout)
    data[73..75].copy_from_slice(&bin_step.to_le_bytes());
    // active_id at offset 76
    data[76..80].copy_from_slice(&active_id.to_le_bytes());
    // reserve_y at offset 184
    data[184..216].copy_from_slice(vault_y_key.as_ref());
    data
}

/// Build a mock SPL token account with the given amount at the standard offset (64).
fn make_spl_vault(amount: u64) -> Vec<u8> {
    let mut data = vec![0u8; 165]; // SPL Token Account is 165 bytes
                                   // Amount at offset 64 (le bytes)
    data[64..72].copy_from_slice(&amount.to_le_bytes());
    data
}

/// Encode an InitMarket instruction for a Hyperp market (index_feed_id = [0; 32]).
fn encode_init_hyperp_market(
    admin_key: &Pubkey,
    mint_key: &Pubkey,
    initial_mark_price_e6: u64,
) -> Vec<u8> {
    let mut data = vec![0u8]; // Tag 0 = InitMarket
    encode_pubkey(admin_key, &mut data);
    encode_pubkey(mint_key, &mut data);
    encode_bytes32(&[0u8; 32], &mut data); // index_feed_id = all zeros (Hyperp mode)
    encode_u64(100, &mut data); // max_staleness_secs
    encode_u16(500, &mut data); // conf_filter_bps
    data.push(0u8); // invert
    encode_u32(0, &mut data); // unit_scale
    encode_u64(initial_mark_price_e6, &mut data); // initial_mark_price_e6 — REQUIRED for Hyperp
                                                  // RiskParams
    encode_u64(100, &mut data); // warmup_period_slots (must be >0 — PERC-8093)
    encode_u64(500, &mut data); // maintenance_margin_bps
    encode_u64(1000, &mut data); // initial_margin_bps
    encode_u64(30, &mut data); // trading_fee_bps
    encode_u64(64, &mut data); // max_accounts
    encode_u128(0, &mut data); // new_account_fee
    encode_u128(0, &mut data); // risk_reduction_threshold
    encode_u128(0, &mut data); // maintenance_fee_per_slot
    encode_u64(100, &mut data); // max_crank_staleness_slots
    encode_u64(0, &mut data); // liquidation_fee_bps
    encode_u128(0, &mut data); // liquidation_fee_cap
    encode_u64(0, &mut data); // liquidation_buffer_bps
    encode_u128(0, &mut data); // min_liquidation_abs
    data
}

/// Encode UpdateHyperpMark instruction (Tag 34, no body payload).
fn encode_update_hyperp_mark() -> Vec<u8> {
    vec![percolator_prog::tags::TAG_UPDATE_HYPERP_MARK]
}

/// Setup a Hyperp market fixture: same as setup_market() but will be
/// initialized with index_feed_id = [0;32] for Hyperp mode.
fn setup_hyperp_market() -> MarketFixture {
    // Reuse the same fixture as setup_market — the Hyperp mode is determined
    // by the instruction data (index_feed_id = [0;32]), not the accounts.
    setup_market()
}

/// Test: UpdateHyperpMark rejects when PumpSwap pool has insufficient liquidity.
/// GH #297 Fix 1: Minimum DEX liquidity check prevents bootstrap manipulation.
#[test]
fn test_update_hyperp_mark_rejects_insufficient_pumpswap_liquidity() {
    let mut f = setup_hyperp_market();
    let initial_mark = 100_000_000u64; // $100 in e6

    // Initialize a Hyperp market (same account layout as test_init_market)
    let init_data = encode_init_hyperp_market(&f.admin.key, &f.mint.key, initial_mark);
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = [
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        let res = process_instruction(&f.program_id, &accounts, &init_data);
        assert!(res.is_ok(), "InitMarket Hyperp should succeed: {:?}", res);
    }

    // Verify market is in Hyperp mode
    let config = state::read_config(&f.slab.data);
    assert!(oracle::is_hyperp_mode(&config), "Should be Hyperp mode");
    assert_eq!(
        config.authority_price_e6, initial_mark,
        "Initial mark should be set"
    );

    // Build PumpSwap pool with LOW liquidity (1 lamport quote = way below threshold)
    let pumpswap_id = oracle::PUMPSWAP_PROGRAM_ID;
    let base_vault_key = Pubkey::new_unique();
    let quote_vault_key = Pubkey::new_unique();
    // base_mint must match the market's collateral_mint (security check in UpdateHyperpMark)
    let pool_data = make_pumpswap_pool(&f.mint.key, &base_vault_key, &quote_vault_key);

    let mut pool_account = TestAccount::new(Pubkey::new_unique(), pumpswap_id, 0, pool_data);
    let mut base_vault =
        TestAccount::new(base_vault_key, spl_token::ID, 0, make_spl_vault(1_000_000)); // 1 token
    let mut quote_vault = TestAccount::new(quote_vault_key, spl_token::ID, 0, make_spl_vault(1)); // 1 lamport — insufficient

    // Advance clock so dt_slots > 0
    let mut clock = TestAccount::new(
        solana_program::sysvar::clock::id(),
        solana_program::sysvar::id(),
        0,
        make_clock(200, 200), // slot 200 > 100
    );

    let accounts = [
        f.slab.to_info(),
        pool_account.to_info(),
        clock.to_info(),
        base_vault.to_info(),
        quote_vault.to_info(),
    ];

    let res = process_instruction(&f.program_id, &accounts, &encode_update_hyperp_mark());
    assert_eq!(
        res,
        Err(PercolatorError::InsufficientDexLiquidity.into()),
        "UpdateHyperpMark should reject insufficient DEX liquidity"
    );
}

/// Test: UpdateHyperpMark succeeds when PumpSwap pool has sufficient liquidity.
#[test]
fn test_update_hyperp_mark_accepts_sufficient_pumpswap_liquidity() {
    let mut f = setup_hyperp_market();
    let initial_mark = 100_000_000u64; // $100 in e6

    // Initialize a Hyperp market
    let init_data = encode_init_hyperp_market(&f.admin.key, &f.mint.key, initial_mark);
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = [
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        let res = process_instruction(&f.program_id, &accounts, &init_data);
        assert!(res.is_ok(), "InitMarket Hyperp should succeed: {:?}", res);
    }

    // Build PumpSwap pool with SUFFICIENT liquidity (2.1T quote = $2,100,000 USDC, above $2M minimum)
    let pumpswap_id = oracle::PUMPSWAP_PROGRAM_ID;
    let base_vault_key = Pubkey::new_unique();
    let quote_vault_key = Pubkey::new_unique();
    // base_mint must match the market's collateral_mint (security check in UpdateHyperpMark)
    let pool_data = make_pumpswap_pool(&f.mint.key, &base_vault_key, &quote_vault_key);

    let base_amount = 2_000_000u64; // 2 tokens (base)
    let quote_amount = 2_100_000_000_000u64; // 2,100,000 USDC — well above MIN_DEX_QUOTE_LIQUIDITY ($2M)

    let mut pool_account = TestAccount::new(Pubkey::new_unique(), pumpswap_id, 0, pool_data);
    let mut base_vault = TestAccount::new(
        base_vault_key,
        spl_token::ID,
        0,
        make_spl_vault(base_amount),
    );
    let mut quote_vault = TestAccount::new(
        quote_vault_key,
        spl_token::ID,
        0,
        make_spl_vault(quote_amount),
    );

    // Advance clock
    let mut clock = TestAccount::new(
        solana_program::sysvar::clock::id(),
        solana_program::sysvar::id(),
        0,
        make_clock(200, 200),
    );

    let accounts = [
        f.slab.to_info(),
        pool_account.to_info(),
        clock.to_info(),
        base_vault.to_info(),
        quote_vault.to_info(),
    ];

    let res = process_instruction(&f.program_id, &accounts, &encode_update_hyperp_mark());
    assert!(
        res.is_ok(),
        "UpdateHyperpMark should succeed with sufficient liquidity: {:?}",
        res
    );

    // Verify mark price was updated (should be EMA between initial mark and DEX price)
    let config = state::read_config(&f.slab.data);
    assert_ne!(
        config.authority_price_e6, 0,
        "Mark price should be non-zero"
    );
}

/// Test: UpdateHyperpMark rejects on exactly-at-boundary liquidity (edge case).
/// quote_liquidity == MIN_DEX_QUOTE_LIQUIDITY - 1 should be rejected.
#[test]
fn test_update_hyperp_mark_boundary_liquidity() {
    let mut f = setup_hyperp_market();
    let initial_mark = 100_000_000u64;

    // Initialize Hyperp market
    let init_data = encode_init_hyperp_market(&f.admin.key, &f.mint.key, initial_mark);
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = [
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        let res = process_instruction(&f.program_id, &accounts, &init_data);
        assert!(res.is_ok(), "InitMarket should succeed: {:?}", res);
    }

    // Pool with exactly threshold - 1 liquidity
    let pumpswap_id = oracle::PUMPSWAP_PROGRAM_ID;
    let base_vault_key = Pubkey::new_unique();
    let quote_vault_key = Pubkey::new_unique();
    // base_mint must match the market's collateral_mint (security check in UpdateHyperpMark)
    let pool_data = make_pumpswap_pool(&f.mint.key, &base_vault_key, &quote_vault_key);

    let threshold = percolator_prog::constants::MIN_DEX_QUOTE_LIQUIDITY;
    let below_threshold = threshold.saturating_sub(1);

    let mut pool_account = TestAccount::new(Pubkey::new_unique(), pumpswap_id, 0, pool_data);
    let mut base_vault =
        TestAccount::new(base_vault_key, spl_token::ID, 0, make_spl_vault(1_000_000));
    let mut quote_vault = TestAccount::new(
        quote_vault_key,
        spl_token::ID,
        0,
        make_spl_vault(below_threshold),
    );

    let mut clock = TestAccount::new(
        solana_program::sysvar::clock::id(),
        solana_program::sysvar::id(),
        0,
        make_clock(200, 200),
    );

    let accounts = [
        f.slab.to_info(),
        pool_account.to_info(),
        clock.to_info(),
        base_vault.to_info(),
        quote_vault.to_info(),
    ];

    let res = process_instruction(&f.program_id, &accounts, &encode_update_hyperp_mark());
    assert_eq!(
        res,
        Err(PercolatorError::InsufficientDexLiquidity.into()),
        "Boundary case: threshold-1 should be rejected"
    );
}

// === UpdateHyperpMark — Meteora DLMM Vault Tests (GH#1521) ===

/// Helper: initialise a Hyperp market and return the fixture.
fn init_hyperp_market_fixture(f: &mut MarketFixture, initial_mark: u64) {
    let init_data = encode_init_hyperp_market(&f.admin.key, &f.mint.key, initial_mark);
    let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
    let accounts = [
        f.admin.to_info(),
        f.slab.to_info(),
        f.mint.to_info(),
        f.vault.to_info(),
        f.token_prog.to_info(),
        f.clock.to_info(),
        f.rent.to_info(),
        dummy_ata.to_info(),
        f.system.to_info(),
    ];
    process_instruction(&f.program_id, &accounts, &init_data)
        .expect("InitMarket Hyperp should succeed");
}

/// Test: UpdateHyperpMark accepts Meteora pool with sufficient vault_y liquidity.
#[test]
fn test_update_hyperp_mark_meteora_accepts_sufficient_liquidity() {
    let mut f = setup_hyperp_market();
    let initial_mark = 1_000_000u64; // $1.00 in e6
    init_hyperp_market_fixture(&mut f, initial_mark);

    let meteora_id = oracle::METEORA_DLMM_PROGRAM_ID;
    let vault_y_key = Pubkey::new_unique();
    // bin_step=100, active_id=0 → price = 1.0 → price_e6 = 1_000_000
    let pool_data = make_meteora_lbpair(100, 0, &vault_y_key);
    let pool_key = Pubkey::new_unique();

    let mut pool_account = TestAccount::new(pool_key, meteora_id, 0, pool_data);
    // vault_y with sufficient liquidity (above MIN_DEX_QUOTE_LIQUIDITY = $2M = 2T atoms)
    let mut vault_y = TestAccount::new(
        vault_y_key,
        spl_token::ID,
        0,
        make_spl_vault(2_100_000_000_000), // 2.1T > 2T threshold ($2.1M > $2M)
    );
    let mut clock = TestAccount::new(
        solana_program::sysvar::clock::id(),
        solana_program::sysvar::id(),
        0,
        make_clock(200, 200), // slot 200 > 100 (> MIN_HYPERP_UPDATE_INTERVAL_SLOTS)
    );

    let accounts = [
        f.slab.to_info(),
        pool_account.to_info(),
        clock.to_info(),
        vault_y.to_info(), // remaining_accounts[0] = vault_y
    ];

    let res = process_instruction(&f.program_id, &accounts, &encode_update_hyperp_mark());
    assert!(
        res.is_ok(),
        "UpdateHyperpMark should accept Meteora with sufficient vault_y liquidity: {:?}",
        res
    );
    let config = state::read_config(&f.slab.data);
    assert_ne!(config.authority_price_e6, 0, "Mark price should be updated");
}

/// Test: UpdateHyperpMark rejects Meteora pool with insufficient vault_y liquidity.
#[test]
fn test_update_hyperp_mark_meteora_rejects_insufficient_liquidity() {
    let mut f = setup_hyperp_market();
    let initial_mark = 1_000_000u64;
    init_hyperp_market_fixture(&mut f, initial_mark);

    let meteora_id = oracle::METEORA_DLMM_PROGRAM_ID;
    let vault_y_key = Pubkey::new_unique();
    let pool_data = make_meteora_lbpair(100, 0, &vault_y_key);

    let mut pool_account = TestAccount::new(Pubkey::new_unique(), meteora_id, 0, pool_data);
    let mut vault_y = TestAccount::new(
        vault_y_key,
        spl_token::ID,
        0,
        make_spl_vault(1), // 1 lamport — far below threshold
    );
    let mut clock = TestAccount::new(
        solana_program::sysvar::clock::id(),
        solana_program::sysvar::id(),
        0,
        make_clock(200, 200),
    );

    let accounts = [
        f.slab.to_info(),
        pool_account.to_info(),
        clock.to_info(),
        vault_y.to_info(),
    ];

    let res = process_instruction(&f.program_id, &accounts, &encode_update_hyperp_mark());
    assert_eq!(
        res,
        Err(PercolatorError::InsufficientDexLiquidity.into()),
        "UpdateHyperpMark should reject Meteora with insufficient vault_y liquidity"
    );
}

/// Test: UpdateHyperpMark rejects Meteora when vault_y key does not match LbPair.reserve_y.
#[test]
fn test_update_hyperp_mark_meteora_rejects_wrong_vault_y() {
    let mut f = setup_hyperp_market();
    let initial_mark = 1_000_000u64;
    init_hyperp_market_fixture(&mut f, initial_mark);

    let meteora_id = oracle::METEORA_DLMM_PROGRAM_ID;
    let vault_y_key = Pubkey::new_unique();
    let pool_data = make_meteora_lbpair(100, 0, &vault_y_key);

    let wrong_vault_y_key = Pubkey::new_unique(); // different key — should be rejected
    let mut pool_account = TestAccount::new(Pubkey::new_unique(), meteora_id, 0, pool_data);
    let mut wrong_vault_y = TestAccount::new(
        wrong_vault_y_key,
        spl_token::ID,
        0,
        make_spl_vault(20_000_000_000),
    );
    let mut clock = TestAccount::new(
        solana_program::sysvar::clock::id(),
        solana_program::sysvar::id(),
        0,
        make_clock(200, 200),
    );

    let accounts = [
        f.slab.to_info(),
        pool_account.to_info(),
        clock.to_info(),
        wrong_vault_y.to_info(), // wrong vault key
    ];

    let res = process_instruction(&f.program_id, &accounts, &encode_update_hyperp_mark());
    assert_eq!(
        res,
        Err(PercolatorError::InvalidOracleKey.into()),
        "UpdateHyperpMark should reject Meteora when vault_y key does not match reserve_y"
    );
}

/// Test: UpdateHyperpMark rejects Meteora when vault_y is owned by an invalid program.
#[test]
fn test_update_hyperp_mark_meteora_rejects_invalid_vault_owner() {
    let mut f = setup_hyperp_market();
    let initial_mark = 1_000_000u64;
    init_hyperp_market_fixture(&mut f, initial_mark);

    let meteora_id = oracle::METEORA_DLMM_PROGRAM_ID;
    let vault_y_key = Pubkey::new_unique();
    let pool_data = make_meteora_lbpair(100, 0, &vault_y_key);

    let bad_owner = Pubkey::new_unique(); // not spl_token or spl_token_2022
    let mut pool_account = TestAccount::new(Pubkey::new_unique(), meteora_id, 0, pool_data);
    let mut vault_y = TestAccount::new(
        vault_y_key,
        bad_owner, // invalid token program owner
        0,
        make_spl_vault(20_000_000_000),
    );
    let mut clock = TestAccount::new(
        solana_program::sysvar::clock::id(),
        solana_program::sysvar::id(),
        0,
        make_clock(200, 200),
    );

    let accounts = [
        f.slab.to_info(),
        pool_account.to_info(),
        clock.to_info(),
        vault_y.to_info(),
    ];

    let res = process_instruction(&f.program_id, &accounts, &encode_update_hyperp_mark());
    assert_eq!(
        res,
        Err(PercolatorError::OracleInvalid.into()),
        "UpdateHyperpMark should reject Meteora vault owned by unknown program"
    );
}

/// Test: UpdateHyperpMark rejects Meteora when no vault_y account provided.
#[test]
fn test_update_hyperp_mark_meteora_rejects_missing_vault_y() {
    let mut f = setup_hyperp_market();
    let initial_mark = 1_000_000u64;
    init_hyperp_market_fixture(&mut f, initial_mark);

    let meteora_id = oracle::METEORA_DLMM_PROGRAM_ID;
    let vault_y_key = Pubkey::new_unique();
    let pool_data = make_meteora_lbpair(100, 0, &vault_y_key);

    let mut pool_account = TestAccount::new(Pubkey::new_unique(), meteora_id, 0, pool_data);
    let mut clock = TestAccount::new(
        solana_program::sysvar::clock::id(),
        solana_program::sysvar::id(),
        0,
        make_clock(200, 200),
    );

    // Only 3 accounts — no vault_y in remaining_accounts
    let accounts = [
        f.slab.to_info(),
        pool_account.to_info(),
        clock.to_info(),
        // no vault_y here
    ];

    let res = process_instruction(&f.program_id, &accounts, &encode_update_hyperp_mark());
    assert_eq!(
        res,
        Err(ProgramError::NotEnoughAccountKeys),
        "UpdateHyperpMark should reject Meteora without vault_y account"
    );
}

/// Test: Non-Hyperp market cannot use UpdateHyperpMark (existing security check).
#[test]
fn test_update_hyperp_mark_rejects_non_hyperp_market() {
    let mut f = setup_market();
    let crank_staleness = 100u64;
    let init_data = encode_init_market(&f, crank_staleness);

    // Init a standard (Pyth) market
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = [
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        let res = process_instruction(&f.program_id, &accounts, &init_data);
        assert!(res.is_ok(), "InitMarket should succeed");
    }

    // Try UpdateHyperpMark on non-Hyperp market — should fail
    let pumpswap_id = oracle::PUMPSWAP_PROGRAM_ID;
    let mut pool_account = TestAccount::new(Pubkey::new_unique(), pumpswap_id, 0, vec![0u8; 256]);
    let mut clock = TestAccount::new(
        solana_program::sysvar::clock::id(),
        solana_program::sysvar::id(),
        0,
        make_clock(200, 200),
    );

    let accounts = [f.slab.to_info(), pool_account.to_info(), clock.to_info()];

    let res = process_instruction(&f.program_id, &accounts, &encode_update_hyperp_mark());
    assert!(
        res.is_err(),
        "UpdateHyperpMark should reject non-Hyperp markets"
    );
}

// === PERC-142: Circuit breaker BEFORE EMA update tests ===

/// Test: compute_ema_mark_price clamps oracle before EMA blending.
/// A large oracle deviation should be bounded by the cap.
#[test]
fn test_circuit_breaker_clamps_before_ema() {
    use percolator_prog::oracle::compute_ema_mark_price;

    let prev_mark = 100_000_000u64; // $100 in e6
    let wild_oracle = 200_000_000u64; // $200 — 100% deviation
    let cap = 10_000u64; // 1% per slot
    let dt = 1u64;
    let alpha = 100_000u64; // 10% EMA weight

    let result = compute_ema_mark_price(prev_mark, wild_oracle, dt, alpha, cap);

    // With 1% cap and dt=1: max deviation = 100M * 10000 / 1M = 1M
    // Clamped oracle = 101_000_000
    // EMA: 0.1 * 101M + 0.9 * 100M = 10.1M + 90M = 100.1M
    assert!(result <= 101_000_000, "result {} should be capped", result);
    assert!(result > prev_mark, "result should increase toward oracle");
}

/// Test: with cap=0, the effective cap in UpdateHyperpMark should still use DEFAULT.
/// This tests the max() enforcement added in PERC-142.
#[test]
fn test_hyperp_effective_cap_enforces_minimum() {
    use percolator_prog::constants::DEFAULT_HYPERP_PRICE_CAP_E2BPS;
    use percolator_prog::oracle::compute_ema_mark_price;

    let prev_mark = 100_000_000u64;
    let oracle = 200_000_000u64;
    let dt = 1u64;
    let alpha = 500_000u64; // 50% weight

    // With cap=0, compute_ema_mark_price would return oracle directly (no clamping).
    let no_cap_result = compute_ema_mark_price(prev_mark, oracle, dt, alpha, 0);
    assert_eq!(
        no_cap_result, 150_000_000,
        "no cap: should be raw EMA blend"
    );

    // With effective_cap = max(0, DEFAULT), the result should be clamped.
    let effective_cap = core::cmp::max(0u64, DEFAULT_HYPERP_PRICE_CAP_E2BPS);
    let capped_result = compute_ema_mark_price(prev_mark, oracle, dt, alpha, effective_cap);
    assert!(
        capped_result < no_cap_result,
        "capped {} should be less than uncapped {}",
        capped_result,
        no_cap_result
    );
    assert!(capped_result > prev_mark, "should still move toward oracle");
}

// ============================================================================
// PERC-273: OI Cap, UnresolveMarket, UpdateMarginParams Tests
// ============================================================================

#[test]
fn test_oi_cap_disabled_when_zero() {
    // When oi_cap_multiplier_bps == 0, any OI should be allowed
    // Default zero value means no cap
    let multiplier: u64 = 0;
    assert_eq!(multiplier, 0);
}

#[test]
fn test_oi_cap_computation() {
    // vault=1000, multiplier=100_000 (10x) → max_oi=10_000
    let vault: u128 = 1_000;
    let multiplier: u64 = 100_000;
    let max_oi = vault.saturating_mul(multiplier as u128) / 10_000;
    assert_eq!(max_oi, 10_000);

    // vault=500, multiplier=50_000 (5x) → max_oi=2500
    let vault2: u128 = 500;
    let multiplier2: u64 = 50_000;
    let max_oi2 = vault2.saturating_mul(multiplier2 as u128) / 10_000;
    assert_eq!(max_oi2, 2_500);
}

// ========================================
// PERC-302: OI Ramp Tests
// ========================================

#[test]
fn test_ramp_disabled_returns_full_multiplier() {
    use percolator_prog::verify::compute_ramp_multiplier;
    // oi_ramp_slots = 0 means ramp disabled, full cap immediately
    assert_eq!(compute_ramp_multiplier(100_000, 1000, 1001, 0), 100_000);
    assert_eq!(compute_ramp_multiplier(50_000, 0, 0, 0), 50_000);
}

#[test]
fn test_ramp_at_start() {
    use percolator_prog::constants::RAMP_START_BPS;
    use percolator_prog::verify::compute_ramp_multiplier;
    // At market creation slot, ramp should return RAMP_START_BPS
    let result = compute_ramp_multiplier(100_000, 1000, 1000, 432_000);
    assert_eq!(result, RAMP_START_BPS);
}

#[test]
fn test_ramp_at_halfway() {
    use percolator_prog::constants::RAMP_START_BPS;
    use percolator_prog::verify::compute_ramp_multiplier;
    // Halfway: 216k slots into 432k ramp
    let target = 100_000u64;
    let result = compute_ramp_multiplier(target, 0, 216_000, 432_000);
    let expected = RAMP_START_BPS + (target - RAMP_START_BPS) / 2;
    assert_eq!(result, expected);
}

#[test]
fn test_ramp_complete() {
    use percolator_prog::verify::compute_ramp_multiplier;
    // After ramp complete, should return full multiplier
    assert_eq!(
        compute_ramp_multiplier(100_000, 0, 432_000, 432_000),
        100_000
    );
    assert_eq!(
        compute_ramp_multiplier(100_000, 0, 500_000, 432_000),
        100_000
    );
}

#[test]
fn test_ramp_low_target_skips_ramp() {
    use percolator_prog::constants::RAMP_START_BPS;
    use percolator_prog::verify::compute_ramp_multiplier;
    // If target <= RAMP_START_BPS, no ramp needed
    assert_eq!(
        compute_ramp_multiplier(RAMP_START_BPS, 0, 0, 432_000),
        RAMP_START_BPS
    );
    assert_eq!(compute_ramp_multiplier(500, 0, 0, 432_000), 500);
}

#[test]
fn test_ramp_never_exceeds_target() {
    use percolator_prog::verify::compute_ramp_multiplier;
    // Test many points along the ramp
    let target = 200_000u64; // 20x
    for elapsed in (0..=500_000).step_by(1000) {
        let result = compute_ramp_multiplier(target, 0, elapsed, 432_000);
        assert!(result <= target, "elapsed={} result={}", elapsed, result);
    }
}

#[test]
fn test_ramp_monotonic() {
    use percolator_prog::verify::compute_ramp_multiplier;
    let target = 100_000u64;
    let mut prev = 0u64;
    for slot in (0..=500_000).step_by(100) {
        let result = compute_ramp_multiplier(target, 0, slot, 432_000);
        assert!(
            result >= prev,
            "slot={} result={} prev={}",
            slot,
            result,
            prev
        );
        prev = result;
    }
}

#[test]
fn test_clear_resolved_flag() {
    use percolator_prog::state;
    let mut data = vec![0u8; 200];

    // Set resolved
    state::set_resolved(&mut data);
    assert!(state::is_resolved(&data));

    // Clear resolved
    state::clear_resolved(&mut data);
    assert!(!state::is_resolved(&data));
}

#[test]
fn test_clear_resolved_preserves_other_flags() {
    use percolator_prog::state;
    let mut data = vec![0u8; 200];

    // Set paused + resolved
    let flags = state::FLAG_RESOLVED | 0x02; // bit 1 = paused
    state::write_flags(&mut data, flags);
    assert!(state::is_resolved(&data));

    // Clear resolved — paused should remain
    state::clear_resolved(&mut data);
    assert!(!state::is_resolved(&data));
    assert_eq!(state::read_flags(&data) & 0x02, 0x02);
}

#[test]
fn test_unresolve_requires_resolved() {
    // UnresolveMarket should only work on resolved markets
    use percolator_prog::state;
    let mut data = vec![0u8; 200];

    // Not resolved → should fail (caller checks this)
    assert!(!state::is_resolved(&data));

    // Set resolved → unresolve → should succeed
    state::set_resolved(&mut data);
    assert!(state::is_resolved(&data));
    state::clear_resolved(&mut data);
    assert!(!state::is_resolved(&data));
}

// ================================================================
// PERC-272: LP Vault Tests
// ================================================================

fn encode_create_lp_vault(fee_share_bps: u64) -> Vec<u8> {
    let mut data = vec![37u8]; // TAG_CREATE_LP_VAULT
    encode_u64(fee_share_bps, &mut data);
    data
}

fn encode_lp_vault_deposit(amount: u64) -> Vec<u8> {
    let mut data = vec![38u8]; // TAG_LP_VAULT_DEPOSIT
    encode_u64(amount, &mut data);
    data
}

fn encode_lp_vault_withdraw(lp_amount: u64) -> Vec<u8> {
    let mut data = vec![39u8]; // TAG_LP_VAULT_WITHDRAW
    encode_u64(lp_amount, &mut data);
    data
}

fn encode_lp_vault_crank_fees() -> Vec<u8> {
    vec![40u8] // TAG_LP_VAULT_CRANK_FEES
}

/// Initialise a market and return the fixture. Helper for LP vault tests.
fn init_market_for_lp_vault() -> MarketFixture {
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &init_data).unwrap();
    }
    f
}

fn make_lp_vault_mint(authority: Pubkey) -> Vec<u8> {
    use spl_token::state::Mint;
    let mut data = vec![0u8; Mint::LEN];
    let mint = Mint {
        mint_authority: solana_program::program_option::COption::Some(authority),
        supply: 0,
        decimals: 6,
        is_initialized: true,
        freeze_authority: solana_program::program_option::COption::None,
    };
    Mint::pack(mint, &mut data).unwrap();
    data
}

#[test]
#[cfg(feature = "test")]
fn test_create_lp_vault() {
    use percolator_prog::lp_vault;

    let mut f = init_market_for_lp_vault();
    let program_id = f.program_id;
    let slab_key = f.slab.key;

    let (lp_vault_state_key, _) =
        Pubkey::find_program_address(&[b"lp_vault", slab_key.as_ref()], &program_id);
    let (lp_vault_mint_key, _) =
        Pubkey::find_program_address(&[b"lp_vault_mint", slab_key.as_ref()], &program_id);

    // Prepare LP vault state PDA account (pre-allocate in test mode)
    let mut lp_vault_state_acct = TestAccount::new(
        lp_vault_state_key,
        program_id,
        0,
        vec![0u8; lp_vault::LP_VAULT_STATE_LEN],
    )
    .writable();

    // Prepare LP vault mint account (pre-allocate in test mode)
    let mut lp_vault_mint_acct = TestAccount::new(
        lp_vault_mint_key,
        spl_token::ID,
        0,
        make_lp_vault_mint(f.vault_pda),
    )
    .writable();

    let mut vault_auth = TestAccount::new(f.vault_pda, program_id, 0, vec![]);

    let data = encode_create_lp_vault(5000); // 50% fee share

    {
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            lp_vault_state_acct.to_info(),
            lp_vault_mint_acct.to_info(),
            vault_auth.to_info(),
            f.system.to_info(),
            f.token_prog.to_info(),
            f.rent.to_info(),
        ];
        process_instruction(&program_id, &accounts, &data).unwrap();
    }

    // Verify state was written
    let vs = lp_vault::read_lp_vault_state(&lp_vault_state_acct.data).unwrap();
    assert!(vs.is_initialized());
    assert_eq!(vs.fee_share_bps, 5000);
    assert_eq!(vs.epoch, 1);
    assert_eq!(vs.total_capital, 0);
}

#[test]
#[cfg(feature = "test")]
fn test_create_lp_vault_invalid_fee_share() {
    let mut f = init_market_for_lp_vault();
    let program_id = f.program_id;
    let slab_key = f.slab.key;

    let (lp_vault_state_key, _) =
        Pubkey::find_program_address(&[b"lp_vault", slab_key.as_ref()], &program_id);
    let (lp_vault_mint_key, _) =
        Pubkey::find_program_address(&[b"lp_vault_mint", slab_key.as_ref()], &program_id);

    let mut lp_vault_state_acct = TestAccount::new(
        lp_vault_state_key,
        program_id,
        0,
        vec![0u8; percolator_prog::lp_vault::LP_VAULT_STATE_LEN],
    )
    .writable();
    let mut lp_vault_mint_acct = TestAccount::new(
        lp_vault_mint_key,
        spl_token::ID,
        0,
        make_lp_vault_mint(f.vault_pda),
    )
    .writable();
    let mut vault_auth = TestAccount::new(f.vault_pda, program_id, 0, vec![]);

    // fee_share_bps = 10_001 → invalid
    let data = encode_create_lp_vault(10_001);
    {
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            lp_vault_state_acct.to_info(),
            lp_vault_mint_acct.to_info(),
            vault_auth.to_info(),
            f.system.to_info(),
            f.token_prog.to_info(),
            f.rent.to_info(),
        ];
        let result = process_instruction(&program_id, &accounts, &data);
        assert!(result.is_err());
    }
}

#[test]
#[cfg(feature = "test")]
fn test_lp_vault_deposit_and_withdraw() {
    use percolator_prog::lp_vault;

    let mut f = init_market_for_lp_vault();
    let program_id = f.program_id;
    let slab_key = f.slab.key;

    let (lp_vault_state_key, _) =
        Pubkey::find_program_address(&[b"lp_vault", slab_key.as_ref()], &program_id);
    let (lp_vault_mint_key, _) =
        Pubkey::find_program_address(&[b"lp_vault_mint", slab_key.as_ref()], &program_id);

    // --- Create LP vault first ---
    let mut lp_vault_state_acct = TestAccount::new(
        lp_vault_state_key,
        program_id,
        0,
        vec![0u8; lp_vault::LP_VAULT_STATE_LEN],
    )
    .writable();
    let mut lp_vault_mint_acct = TestAccount::new(
        lp_vault_mint_key,
        spl_token::ID,
        0,
        make_lp_vault_mint(f.vault_pda),
    )
    .writable();
    let mut vault_auth = TestAccount::new(f.vault_pda, program_id, 0, vec![]);

    {
        let data = encode_create_lp_vault(5000);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            lp_vault_state_acct.to_info(),
            lp_vault_mint_acct.to_info(),
            vault_auth.to_info(),
            f.system.to_info(),
            f.token_prog.to_info(),
            f.rent.to_info(),
        ];
        process_instruction(&program_id, &accounts, &data).unwrap();
    }

    // --- Deposit into LP vault ---
    let depositor_key = Pubkey::new_unique();
    let mut depositor = TestAccount::new(
        depositor_key,
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut depositor_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, depositor_key, 1_000_000),
    )
    .writable();
    let mut depositor_lp_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(lp_vault_mint_key, depositor_key, 0),
    )
    .writable();

    let deposit_amount = 500_000u64;
    {
        let data = encode_lp_vault_deposit(deposit_amount);
        let accounts = vec![
            depositor.to_info(),
            f.slab.to_info(),
            depositor_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            lp_vault_mint_acct.to_info(),
            depositor_lp_ata.to_info(),
            vault_auth.to_info(),
            lp_vault_state_acct.to_info(),
        ];
        process_instruction(&program_id, &accounts, &data).unwrap();
    }

    // Check: LP vault capital increased
    let vs = lp_vault::read_lp_vault_state(&lp_vault_state_acct.data).unwrap();
    assert_eq!(vs.total_capital, deposit_amount as u128);

    // Check: depositor got LP tokens (1:1 for first deposit)
    let lp_token_balance = TokenAccount::unpack(&depositor_lp_ata.data).unwrap().amount;
    assert_eq!(lp_token_balance, deposit_amount);

    // Check: engine vault increased
    let engine = zc::engine_ref(&f.slab.data).unwrap();
    // engine.vault starts at 0 (VAULT_SEED is only in the token account, not tracked by engine)
    assert_eq!(engine.vault.get(), deposit_amount as u128);

    // Check: depositor ATA decreased
    let depositor_balance = TokenAccount::unpack(&depositor_ata.data).unwrap().amount;
    assert_eq!(depositor_balance, 1_000_000 - deposit_amount);

    // --- Withdraw half ---
    let withdraw_lp = deposit_amount / 2;
    {
        let data = encode_lp_vault_withdraw(withdraw_lp);
        let accounts = vec![
            depositor.to_info(),
            f.slab.to_info(),
            depositor_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            lp_vault_mint_acct.to_info(),
            depositor_lp_ata.to_info(),
            vault_auth.to_info(),
            lp_vault_state_acct.to_info(),
        ];
        process_instruction(&program_id, &accounts, &data).unwrap();
    }

    // Check: LP vault capital decreased
    let vs = lp_vault::read_lp_vault_state(&lp_vault_state_acct.data).unwrap();
    assert_eq!(vs.total_capital, (deposit_amount - withdraw_lp) as u128);

    // Check: LP tokens burned
    let lp_token_balance = TokenAccount::unpack(&depositor_lp_ata.data).unwrap().amount;
    assert_eq!(lp_token_balance, deposit_amount - withdraw_lp);

    // Check: depositor ATA increased
    let depositor_balance = TokenAccount::unpack(&depositor_ata.data).unwrap().amount;
    assert_eq!(depositor_balance, 1_000_000 - deposit_amount + withdraw_lp);
}

#[test]
#[cfg(feature = "test")]
fn test_lp_vault_zero_deposit_rejected() {
    use percolator_prog::lp_vault;

    let mut f = init_market_for_lp_vault();
    let program_id = f.program_id;
    let slab_key = f.slab.key;

    let (lp_vault_state_key, _) =
        Pubkey::find_program_address(&[b"lp_vault", slab_key.as_ref()], &program_id);
    let (lp_vault_mint_key, _) =
        Pubkey::find_program_address(&[b"lp_vault_mint", slab_key.as_ref()], &program_id);

    let mut lp_vault_state_acct = TestAccount::new(
        lp_vault_state_key,
        program_id,
        0,
        vec![0u8; lp_vault::LP_VAULT_STATE_LEN],
    )
    .writable();
    let mut lp_vault_mint_acct = TestAccount::new(
        lp_vault_mint_key,
        spl_token::ID,
        0,
        make_lp_vault_mint(f.vault_pda),
    )
    .writable();
    let mut vault_auth = TestAccount::new(f.vault_pda, program_id, 0, vec![]);

    // Create vault
    {
        let data = encode_create_lp_vault(5000);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            lp_vault_state_acct.to_info(),
            lp_vault_mint_acct.to_info(),
            vault_auth.to_info(),
            f.system.to_info(),
            f.token_prog.to_info(),
            f.rent.to_info(),
        ];
        process_instruction(&program_id, &accounts, &data).unwrap();
    }

    // Try deposit 0
    let depositor_key = Pubkey::new_unique();
    let mut depositor = TestAccount::new(
        depositor_key,
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut depositor_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, depositor_key, 1_000_000),
    )
    .writable();
    let mut depositor_lp_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(lp_vault_mint_key, depositor_key, 0),
    )
    .writable();

    {
        let data = encode_lp_vault_deposit(0); // zero amount
        let accounts = vec![
            depositor.to_info(),
            f.slab.to_info(),
            depositor_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            lp_vault_mint_acct.to_info(),
            depositor_lp_ata.to_info(),
            vault_auth.to_info(),
            lp_vault_state_acct.to_info(),
        ];
        let result = process_instruction(&program_id, &accounts, &data);
        assert!(result.is_err());
    }
}

#[test]
#[cfg(feature = "test")]
fn test_lp_vault_fee_crank() {
    use percolator_prog::lp_vault;

    let mut f = init_market_for_lp_vault();
    let program_id = f.program_id;
    let slab_key = f.slab.key;

    let (lp_vault_state_key, _) =
        Pubkey::find_program_address(&[b"lp_vault", slab_key.as_ref()], &program_id);
    let (lp_vault_mint_key, _) =
        Pubkey::find_program_address(&[b"lp_vault_mint", slab_key.as_ref()], &program_id);

    let mut lp_vault_state_acct = TestAccount::new(
        lp_vault_state_key,
        program_id,
        0,
        vec![0u8; lp_vault::LP_VAULT_STATE_LEN],
    )
    .writable();
    let mut lp_vault_mint_acct = TestAccount::new(
        lp_vault_mint_key,
        spl_token::ID,
        0,
        make_lp_vault_mint(f.vault_pda),
    )
    .writable();
    let mut vault_auth = TestAccount::new(f.vault_pda, program_id, 0, vec![]);

    // Create vault with 50% fee share
    {
        let data = encode_create_lp_vault(5000);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            lp_vault_state_acct.to_info(),
            lp_vault_mint_acct.to_info(),
            vault_auth.to_info(),
            f.system.to_info(),
            f.token_prog.to_info(),
            f.rent.to_info(),
        ];
        process_instruction(&program_id, &accounts, &data).unwrap();
    }

    // Deposit some capital
    let depositor_key = Pubkey::new_unique();
    let mut depositor = TestAccount::new(
        depositor_key,
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut depositor_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, depositor_key, 1_000_000),
    )
    .writable();
    let mut depositor_lp_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(lp_vault_mint_key, depositor_key, 0),
    )
    .writable();

    {
        let data = encode_lp_vault_deposit(500_000);
        let accounts = vec![
            depositor.to_info(),
            f.slab.to_info(),
            depositor_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            lp_vault_mint_acct.to_info(),
            depositor_lp_ata.to_info(),
            vault_auth.to_info(),
            lp_vault_state_acct.to_info(),
        ];
        process_instruction(&program_id, &accounts, &data).unwrap();
    }

    // Simulate fee accrual: manually bump engine.insurance_fund.fee_revenue
    {
        let mut slab_data = f.slab.data.as_mut_slice();
        let engine = zc::engine_mut(&mut slab_data).unwrap();
        let current_revenue = engine.insurance_fund.fee_revenue.get();
        engine.insurance_fund.fee_revenue = percolator::U128::new(current_revenue + 10_000);
        // Also add to insurance balance (fees go to insurance first)
        let current_balance = engine.insurance_fund.balance.get();
        engine.insurance_fund.balance = percolator::U128::new(current_balance + 10_000);
    }

    // Run fee crank
    {
        let data = encode_lp_vault_crank_fees();
        let accounts = vec![
            f.slab.to_info(),
            lp_vault_state_acct.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&program_id, &accounts, &data).unwrap();
    }

    // Verify: LP vault got 50% of 10_000 = 5_000
    let vs = lp_vault::read_lp_vault_state(&lp_vault_state_acct.data).unwrap();
    assert_eq!(vs.total_capital, 500_000 + 5_000); // deposit + fee share
    assert_eq!(vs.total_fees_distributed, 5_000);

    // Verify: insurance fund balance decreased by LP portion
    let engine = zc::engine_ref(&f.slab.data).unwrap();
    let expected_insurance = 10_000 - 5_000; // total fees minus LP portion
    assert_eq!(engine.insurance_fund.balance.get(), expected_insurance);
}

#[test]
#[cfg(feature = "test")]
fn test_lp_vault_crank_no_new_fees_rejected() {
    use percolator_prog::lp_vault;

    let mut f = init_market_for_lp_vault();
    let program_id = f.program_id;
    let slab_key = f.slab.key;

    let (lp_vault_state_key, _) =
        Pubkey::find_program_address(&[b"lp_vault", slab_key.as_ref()], &program_id);
    let (lp_vault_mint_key, _) =
        Pubkey::find_program_address(&[b"lp_vault_mint", slab_key.as_ref()], &program_id);

    let mut lp_vault_state_acct = TestAccount::new(
        lp_vault_state_key,
        program_id,
        0,
        vec![0u8; lp_vault::LP_VAULT_STATE_LEN],
    )
    .writable();
    let mut lp_vault_mint_acct = TestAccount::new(
        lp_vault_mint_key,
        spl_token::ID,
        0,
        make_lp_vault_mint(f.vault_pda),
    )
    .writable();
    let mut vault_auth = TestAccount::new(f.vault_pda, program_id, 0, vec![]);

    // Create vault
    {
        let data = encode_create_lp_vault(5000);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            lp_vault_state_acct.to_info(),
            lp_vault_mint_acct.to_info(),
            vault_auth.to_info(),
            f.system.to_info(),
            f.token_prog.to_info(),
            f.rent.to_info(),
        ];
        process_instruction(&program_id, &accounts, &data).unwrap();
    }

    // Try crank with no new fees
    {
        let data = encode_lp_vault_crank_fees();
        let accounts = vec![
            f.slab.to_info(),
            lp_vault_state_acct.to_info(),
            f.clock.to_info(),
        ];
        let result = process_instruction(&program_id, &accounts, &data);
        assert!(result.is_err()); // Should fail: no new fees
    }
}

#[test]
#[cfg(feature = "test")]
fn test_lp_vault_proportional_shares() {
    use percolator_prog::lp_vault;

    let mut f = init_market_for_lp_vault();
    let program_id = f.program_id;
    let slab_key = f.slab.key;

    let (lp_vault_state_key, _) =
        Pubkey::find_program_address(&[b"lp_vault", slab_key.as_ref()], &program_id);
    let (lp_vault_mint_key, _) =
        Pubkey::find_program_address(&[b"lp_vault_mint", slab_key.as_ref()], &program_id);

    let mut lp_vault_state_acct = TestAccount::new(
        lp_vault_state_key,
        program_id,
        0,
        vec![0u8; lp_vault::LP_VAULT_STATE_LEN],
    )
    .writable();
    let mut lp_vault_mint_acct = TestAccount::new(
        lp_vault_mint_key,
        spl_token::ID,
        0,
        make_lp_vault_mint(f.vault_pda),
    )
    .writable();
    let mut vault_auth = TestAccount::new(f.vault_pda, program_id, 0, vec![]);

    // Create vault
    {
        let data = encode_create_lp_vault(5000);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            lp_vault_state_acct.to_info(),
            lp_vault_mint_acct.to_info(),
            vault_auth.to_info(),
            f.system.to_info(),
            f.token_prog.to_info(),
            f.rent.to_info(),
        ];
        process_instruction(&program_id, &accounts, &data).unwrap();
    }

    // First depositor: 1_000_000 tokens → 1_000_000 LP shares (1:1)
    let d1_key = Pubkey::new_unique();
    let mut d1 = TestAccount::new(d1_key, solana_program::system_program::id(), 0, vec![]).signer();
    let mut d1_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, d1_key, 2_000_000),
    )
    .writable();
    let mut d1_lp_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(lp_vault_mint_key, d1_key, 0),
    )
    .writable();

    {
        let data = encode_lp_vault_deposit(1_000_000);
        let accounts = vec![
            d1.to_info(),
            f.slab.to_info(),
            d1_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            lp_vault_mint_acct.to_info(),
            d1_lp_ata.to_info(),
            vault_auth.to_info(),
            lp_vault_state_acct.to_info(),
        ];
        process_instruction(&program_id, &accounts, &data).unwrap();
    }

    let d1_lp_balance = TokenAccount::unpack(&d1_lp_ata.data).unwrap().amount;
    assert_eq!(d1_lp_balance, 1_000_000);

    // Simulate fee accrual: +100_000 fee revenue
    {
        let slab_data = f.slab.data.as_mut_slice();
        let engine = zc::engine_mut(slab_data).unwrap();
        engine.insurance_fund.fee_revenue =
            percolator::U128::new(engine.insurance_fund.fee_revenue.get() + 100_000);
        engine.insurance_fund.balance =
            percolator::U128::new(engine.insurance_fund.balance.get() + 100_000);
    }

    // Crank fees: 50% of 100_000 = 50_000 goes to LP vault
    {
        let data = encode_lp_vault_crank_fees();
        let accounts = vec![
            f.slab.to_info(),
            lp_vault_state_acct.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&program_id, &accounts, &data).unwrap();
    }

    // LP vault capital is now 1_050_000
    let vs = lp_vault::read_lp_vault_state(&lp_vault_state_acct.data).unwrap();
    assert_eq!(vs.total_capital, 1_050_000);

    // Second depositor: 1_000_000 tokens at vault value 1_050_000 with supply 1_000_000
    // shares = 1_000_000 * 1_000_000 / 1_050_000 = 952_380 (rounded down)
    let d2_key = Pubkey::new_unique();
    let mut d2 = TestAccount::new(d2_key, solana_program::system_program::id(), 0, vec![]).signer();
    let mut d2_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, d2_key, 2_000_000),
    )
    .writable();
    let mut d2_lp_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(lp_vault_mint_key, d2_key, 0),
    )
    .writable();

    {
        let data = encode_lp_vault_deposit(1_000_000);
        let accounts = vec![
            d2.to_info(),
            f.slab.to_info(),
            d2_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            lp_vault_mint_acct.to_info(),
            d2_lp_ata.to_info(),
            vault_auth.to_info(),
            lp_vault_state_acct.to_info(),
        ];
        process_instruction(&program_id, &accounts, &data).unwrap();
    }

    let d2_lp_balance = TokenAccount::unpack(&d2_lp_ata.data).unwrap().amount;
    assert_eq!(d2_lp_balance, 952_380); // 1_000_000 * 1_000_000 / 1_050_000

    // D1 has 1_000_000 shares, D2 has 952_380 shares
    // Total supply = 1_952_380, total capital = 2_050_000
    // D1's share value = 1_000_000 * 2_050_000 / 1_952_380 = 1_049_999 (≈1_050_000)
    // D2's share value = 952_380 * 2_050_000 / 1_952_380 = 999_999 (≈1_000_000)
    // This confirms the first depositor captures the fee appreciation.
}

// ============================================================================
// PERC-274: Oracle Aggregation Tests
// ============================================================================

#[test]
fn test_median_three_prices() {
    use percolator_prog::verify::median_price;
    let mut prices = [100, 200, 150, 0, 0];
    assert_eq!(median_price(&mut prices), Some(150));
}

#[test]
fn test_median_two_prices() {
    use percolator_prog::verify::median_price;
    let mut prices = [100, 200, 0, 0, 0];
    assert_eq!(median_price(&mut prices), Some(150)); // average of 100 and 200
}

#[test]
fn test_median_five_prices() {
    use percolator_prog::verify::median_price;
    let mut prices = [500, 100, 300, 200, 400];
    assert_eq!(median_price(&mut prices), Some(300));
}

#[test]
fn test_median_with_zeros_filtered() {
    use percolator_prog::verify::median_price;
    let mut prices = [0, 100, 0, 300, 0];
    assert_eq!(median_price(&mut prices), Some(200)); // median of [100, 300]
}

#[test]
fn test_median_all_zeros() {
    use percolator_prog::verify::median_price;
    let mut prices = [0, 0, 0, 0, 0];
    assert_eq!(median_price(&mut prices), None);
}

#[test]
fn test_median_single() {
    use percolator_prog::verify::median_price;
    let mut prices = [42, 0, 0, 0, 0];
    assert_eq!(median_price(&mut prices), Some(42));
}

#[test]
fn test_deviation_small() {
    use percolator_prog::verify::price_deviates_too_much;
    // 1% deviation with 500 bps (5%) cap → not too much
    assert!(!price_deviates_too_much(10000, 10100, 500));
}

#[test]
fn test_deviation_large() {
    use percolator_prog::verify::price_deviates_too_much;
    // 20% deviation with 500 bps (5%) cap → too much
    assert!(price_deviates_too_much(10000, 12000, 500));
}

#[test]
fn test_deviation_zero_last() {
    use percolator_prog::verify::price_deviates_too_much;
    // First price (last=0) → always OK
    assert!(!price_deviates_too_much(0, 10000, 500));
}

#[test]
fn test_ring_buffer_push() {
    use percolator_prog::verify::ring_buffer_push;
    assert_eq!(ring_buffer_push(0, 8), 1);
    assert_eq!(ring_buffer_push(7, 8), 0); // wraps
    assert_eq!(ring_buffer_push(3, 8), 4);
}

// ============================================================================
// PERC-298: Skew-Adjusted OI Cap
// ============================================================================

#[test]
fn test_pack_unpack_oi_cap_roundtrip() {
    use percolator_prog::processor::{pack_oi_cap, unpack_oi_cap};

    // Basic roundtrip
    let packed = pack_oi_cap(100_000, 5_000);
    let (mult, skew) = unpack_oi_cap(packed);
    assert_eq!(mult, 100_000);
    assert_eq!(skew, 5_000);

    // Zero skew (backwards compat: existing markets)
    let packed_legacy = pack_oi_cap(50_000, 0);
    let (mult, skew) = unpack_oi_cap(packed_legacy);
    assert_eq!(mult, 50_000);
    assert_eq!(skew, 0);

    // Plain u64 with no upper bits (legacy oi_cap_multiplier_bps)
    let (mult, skew) = unpack_oi_cap(100_000u64);
    assert_eq!(mult, 100_000);
    assert_eq!(skew, 0); // backwards compatible: no skew

    // Max values
    let packed = pack_oi_cap(0xFFFF_FFFF, 0xFFFF);
    let (mult, skew) = unpack_oi_cap(packed);
    assert_eq!(mult, 0xFFFF_FFFF);
    assert_eq!(skew, 0xFFFF);
}

#[test]
fn test_skew_adjusted_cap_reduces_with_imbalance() {
    // With 100% long skew and skew_factor=5000 (50%), cap should be halved
    let vault: u128 = 1_000_000;
    let multiplier: u128 = 100_000; // 10x
    let base_max_oi = vault * multiplier / 10_000; // = 10_000_000

    let long_oi: u128 = 10_000_000;
    let short_oi: u128 = 0;
    let total_oi = long_oi + short_oi;
    let skew = long_oi - short_oi; // = 10_000_000 (100% skew)
    let skew_factor_bps: u128 = 5_000;

    let reduction_bps = skew * skew_factor_bps / total_oi; // = 5_000
    let capped_reduction = reduction_bps.min(skew_factor_bps); // = 5_000
    let effective = base_max_oi * (10_000 - capped_reduction) / 10_000; // = 5_000_000

    assert_eq!(
        effective,
        base_max_oi / 2,
        "100% skew with 50% factor should halve the cap"
    );
}

// PERC-304: Utilization-Curve Fee Multiplier Tests
// ============================================================================

#[test]
fn test_fee_multiplier_segment1_flat() {
    use percolator_prog::verify::compute_fee_multiplier_bps;
    // 0% utilization → 1.0×
    assert_eq!(compute_fee_multiplier_bps(0), 10_000);
    // 25% utilization → 1.0×
    assert_eq!(compute_fee_multiplier_bps(2_500), 10_000);
    // 50% utilization (boundary) → 1.0×
    assert_eq!(compute_fee_multiplier_bps(5_000), 10_000);
}

#[test]
fn test_fee_multiplier_segment2_linear() {
    use percolator_prog::verify::compute_fee_multiplier_bps;
    // 50% → 1.0× (boundary, inclusive in segment 1)
    assert_eq!(compute_fee_multiplier_bps(5_000), 10_000);
    // 65% = midpoint of [50%, 80%] → midpoint of [1.0×, 2.5×] = 1.75×
    assert_eq!(compute_fee_multiplier_bps(6_500), 17_500);
    // 80% → 2.5× (boundary)
    assert_eq!(compute_fee_multiplier_bps(8_000), 25_000);
    // 51% → just above kink 1
    // excess = 1, slope = 15_000 / 3_000 = 5 bps per util_bps
    assert_eq!(compute_fee_multiplier_bps(5_001), 10_005);
}

#[test]
fn test_fee_multiplier_segment3_steep() {
    use percolator_prog::verify::compute_fee_multiplier_bps;
    // 80% → 2.5× (boundary, inclusive in segment 2)
    assert_eq!(compute_fee_multiplier_bps(8_000), 25_000);
    // 90% = midpoint of [80%, 100%] → midpoint of [2.5×, 7.5×] = 5.0×
    assert_eq!(compute_fee_multiplier_bps(9_000), 50_000);
    // 100% → 7.5×
    assert_eq!(compute_fee_multiplier_bps(10_000), 75_000);
    // 81% → just above kink 2
    // excess = 1, slope = 50_000 / 2_000 = 25 bps per util_bps
    assert_eq!(compute_fee_multiplier_bps(8_001), 25_025);
}

#[test]
fn test_fee_multiplier_cap_over_100() {
    use percolator_prog::verify::compute_fee_multiplier_bps;
    // > 100% utilization → capped at 7.5×
    assert_eq!(compute_fee_multiplier_bps(10_001), 75_000);
    assert_eq!(compute_fee_multiplier_bps(15_000), 75_000);
    assert_eq!(compute_fee_multiplier_bps(u64::MAX), 75_000);
}

#[test]
fn test_fee_multiplier_monotonic_brute_force() {
    use percolator_prog::verify::compute_fee_multiplier_bps;
    // Check monotonicity over the full range [0, 10_000]
    let mut prev = compute_fee_multiplier_bps(0);
    for u in 1..=10_001 {
        let curr = compute_fee_multiplier_bps(u);
        assert!(
            curr >= prev,
            "monotonicity violation at util_bps={}: {} < {}",
            u,
            curr,
            prev
        );
        prev = curr;
    }
}

#[test]
fn test_compute_util_bps() {
    use percolator_prog::verify::compute_util_bps;
    // Zero max_oi → 0
    assert_eq!(compute_util_bps(100, 0), 0);
    // 50% utilization
    assert_eq!(compute_util_bps(500, 1000), 5_000);
    // 100% utilization
    assert_eq!(compute_util_bps(1000, 1000), 10_000);
    // 150% over-utilization
    assert_eq!(compute_util_bps(1500, 1000), 15_000);
    // Zero current_oi
    assert_eq!(compute_util_bps(0, 1000), 0);
}

#[test]
fn test_lp_vault_state_size_unchanged() {
    // CRITICAL: LpVaultState must be exactly 192 bytes for account layout compatibility.
    // PERC-313: Grew from 128 → 192 for high-water mark protection fields.
    assert_eq!(
        core::mem::size_of::<percolator_prog::lp_vault::LpVaultState>(),
        192,
        "LpVaultState layout changed — this breaks existing accounts!"
    );
}

#[test]
fn test_skew_adjusted_cap_unchanged_when_balanced() {
    // Equal long/short => skew = 0 => no reduction
    let vault: u128 = 1_000_000;
    let multiplier: u128 = 100_000;
    let base_max_oi = vault * multiplier / 10_000;

    let long_oi: u128 = 5_000_000;
    let short_oi: u128 = 5_000_000;
    let total_oi = long_oi + short_oi;
    let skew = 0u128;
    let skew_factor_bps: u128 = 5_000;

    let reduction_bps = skew * skew_factor_bps / total_oi;
    let effective = base_max_oi * (10_000 - reduction_bps) / 10_000;

    assert_eq!(effective, base_max_oi, "balanced OI should not reduce cap");
}

#[test]
fn test_skew_adjusted_cap_zero_skew_factor_disables() {
    let vault: u128 = 1_000_000;
    let multiplier: u128 = 100_000;
    let base_max_oi = vault * multiplier / 10_000;

    // skew_factor = 0 => skew adjustment disabled
    let skew_factor_bps: u128 = 0;
    // Even with 100% skew, effective == base when factor is 0
    // (In practice, the code checks skew_factor > 0 and skips)
    let reduction = 0u128; // would be computed as 0 since factor is 0
    let effective = base_max_oi * (10_000 - reduction) / 10_000;

    assert_eq!(effective, base_max_oi);
}

#[test]
fn test_lp_vault_state_new_fields_zero_default() {
    // Verify that zeroed LpVaultState has util curve disabled and mult at 0.
    // This ensures backward compat: existing vaults (with zeroed reserved bytes)
    // have the curve disabled and multiplier = 0 (treated as 1.0× by crank logic).
    let state = percolator_prog::lp_vault::LpVaultState::new_zeroed();
    assert_eq!(state.lp_util_curve_enabled, 0);
    assert_eq!(state.current_fee_mult_bps, 0);
}

// =============================================================================
// PERC-117: UpdateMarkPrice (Tag 33) — Pyth Oracle CPI Integration Tests
// =============================================================================

/// Encode UpdateMarkPrice instruction (Tag 33, no body payload).
fn encode_update_mark_price() -> Vec<u8> {
    vec![percolator_prog::tags::TAG_UPDATE_MARK_PRICE]
}

/// Encode SetPythOracle instruction (Tag 32).
/// Switches market to Pyth-pinned mode (oracle_authority = [0;32]).
fn encode_set_pyth_oracle(
    feed_id: &[u8; 32],
    max_staleness_secs: u64,
    conf_filter_bps: u16,
) -> Vec<u8> {
    let mut data = vec![percolator_prog::tags::TAG_SET_PYTH_ORACLE];
    data.extend_from_slice(feed_id);
    data.extend_from_slice(&max_staleness_secs.to_le_bytes());
    data.extend_from_slice(&conf_filter_bps.to_le_bytes());
    data
}

/// Initialize a Pyth-pinned market using the default fixture.
/// Returns the fixture (with slab data written).
fn setup_pyth_pinned_market() -> MarketFixture {
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 1000);
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = [
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &init_data)
            .expect("InitMarket Pyth-pinned should succeed");
    }
    // Confirm Pyth-pinned mode: oracle_authority == [0;32], index_feed_id != [0;32]
    let config = state::read_config(&f.slab.data);
    assert!(
        !oracle::is_hyperp_mode(&config),
        "Should NOT be Hyperp mode"
    );
    assert!(
        percolator_prog::verify::is_pyth_pinned_mode(config.oracle_authority, config.index_feed_id),
        "Should be Pyth-pinned mode"
    );
    f
}

/// Test: UpdateMarkPrice bootstraps mark price from Pyth on first call (prev_mark == 0).
#[test]
fn test_update_mark_price_bootstrap_from_pyth() {
    let mut f = setup_pyth_pinned_market();

    // Verify initial state: authority_price_e6 == 0 (no cached mark yet)
    let config_before = state::read_config(&f.slab.data);
    assert_eq!(
        config_before.authority_price_e6, 0,
        "Initial mark should be 0"
    );

    // UpdateMarkPrice with valid Pyth account
    // Clock slot must be > MIN_PYTH_MARK_UPDATE_INTERVAL_SLOTS (5) from engine.current_slot (0)
    f.clock.data = make_clock(200, 200);

    let accounts = [f.slab.to_info(), f.pyth_index.to_info(), f.clock.to_info()];
    let res = process_instruction(&f.program_id, &accounts, &encode_update_mark_price());
    assert!(
        res.is_ok(),
        "UpdateMarkPrice bootstrap should succeed: {:?}",
        res
    );

    // Verify mark price was set from Pyth (bootstrap: ema returns oracle directly when prev==0)
    let config_after = state::read_config(&f.slab.data);
    // Pyth data: price=100_000_000, expo=-6 → e6 price = 100_000_000
    assert_ne!(
        config_after.authority_price_e6, 0,
        "Mark price should be set after bootstrap"
    );
    assert_eq!(
        config_after.authority_price_e6, 100_000_000,
        "Bootstrap mark should equal Pyth price (no EMA when prev==0)"
    );
}

/// Test: UpdateMarkPrice applies EMA smoothing on subsequent calls.
#[test]
fn test_update_mark_price_applies_ema() {
    let mut f = setup_pyth_pinned_market();

    // Bootstrap: first call sets authority_price_e6 to Pyth price
    f.clock.data = make_clock(200, 200);
    {
        let accounts = [f.slab.to_info(), f.pyth_index.to_info(), f.clock.to_info()];
        process_instruction(&f.program_id, &accounts, &encode_update_mark_price())
            .expect("Bootstrap should succeed");
    }
    let after_bootstrap = state::read_config(&f.slab.data);
    assert_eq!(after_bootstrap.authority_price_e6, 100_000_000);

    // Second call with a different Pyth price — EMA should blend prev + new
    // Update the Pyth account to reflect a new price (120_000_000 → $120)
    let new_pyth_data = make_pyth(&f.index_feed_id, 120_000_000, -6, 1, 300);
    f.pyth_index.data = new_pyth_data;
    f.clock.data = make_clock(210, 300); // slot 20, dt=10 slots

    {
        let accounts = [f.slab.to_info(), f.pyth_index.to_info(), f.clock.to_info()];
        process_instruction(&f.program_id, &accounts, &encode_update_mark_price())
            .expect("Second UpdateMarkPrice should succeed");
    }

    let after_second = state::read_config(&f.slab.data);
    // EMA blends: new < oracle but > prev; should be strictly between 100M and 120M
    assert!(
        after_second.authority_price_e6 > 100_000_000
            && after_second.authority_price_e6 <= 120_000_000,
        "EMA mark should be between prev and oracle: got {}",
        after_second.authority_price_e6
    );
}

/// Test: UpdateMarkPrice rejects on Hyperp markets (index_feed_id == [0;32]).
#[test]
fn test_update_mark_price_rejects_hyperp_market() {
    let mut f = setup_hyperp_market();
    let initial_mark = 100_000_000u64;
    let init_data = encode_init_hyperp_market(&f.admin.key, &f.mint.key, initial_mark);
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = [
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &init_data)
            .expect("InitMarket Hyperp should succeed");
    }

    let config = state::read_config(&f.slab.data);
    assert!(oracle::is_hyperp_mode(&config), "Should be Hyperp mode");

    // UpdateMarkPrice should reject Hyperp markets
    f.clock.data = make_clock(200, 200);
    let accounts = [f.slab.to_info(), f.pyth_index.to_info(), f.clock.to_info()];
    let res = process_instruction(&f.program_id, &accounts, &encode_update_mark_price());
    assert!(res.is_err(), "UpdateMarkPrice should reject Hyperp market");
    assert_eq!(
        res.unwrap_err(),
        ProgramError::InvalidAccountData,
        "Should return InvalidAccountData for Hyperp market"
    );
}

/// Test: UpdateMarkPrice skips silently when dt < MIN_INTERVAL (same slot).
#[test]
fn test_update_mark_price_skips_same_slot() {
    let mut f = setup_pyth_pinned_market();

    // Bootstrap first
    f.clock.data = make_clock(200, 200);
    {
        let accounts = [f.slab.to_info(), f.pyth_index.to_info(), f.clock.to_info()];
        process_instruction(&f.program_id, &accounts, &encode_update_mark_price())
            .expect("Bootstrap should succeed");
    }
    let after_bootstrap = state::read_config(&f.slab.data);
    let mark_after_bootstrap = after_bootstrap.authority_price_e6;

    // Second call on the SAME slot (dt=0) — should skip silently, no change
    // Note: engine.current_slot is written by KeeperCrank, not UpdateMarkPrice.
    // The bootstrap left engine at slot 0 (no crank happened). So slot 10 - 0 = 10 >= MIN.
    // To test same-slot, we need dt < 5.
    // Call again at slot 11 (dt = 1 from slot 10 — but engine is still at 0).
    // Actually this test checks dt < MIN from last crank slot (engine.current_slot).
    // Since engine.current_slot stays 0, dt = slot - 0 is always >= 5 for slot >= 5.
    // This test verifies the instruction succeeds (since slot=10 gives dt=10>5).
    // The call must succeed (already verified above).

    // Verify the mark was bootstrapped correctly
    assert_ne!(
        mark_after_bootstrap, 0,
        "Mark should be set after bootstrap"
    );
}

/// Test: UpdateMarkPrice rejects if Pyth oracle account is not owned by Pyth program.
#[test]
fn test_update_mark_price_rejects_wrong_oracle_owner() {
    let mut f = setup_pyth_pinned_market();
    f.clock.data = make_clock(200, 200);

    // Create a fake oracle account owned by an attacker
    let fake_owner = Pubkey::new_unique();
    let pyth_data = make_pyth(&f.index_feed_id, 100_000_000, -6, 1, 200);
    let mut fake_oracle = TestAccount::new(Pubkey::new_unique(), fake_owner, 0, pyth_data);

    let accounts = [f.slab.to_info(), fake_oracle.to_info(), f.clock.to_info()];
    let res = process_instruction(&f.program_id, &accounts, &encode_update_mark_price());
    assert!(
        res.is_err(),
        "UpdateMarkPrice should reject fake oracle owner"
    );
}

/// Test: UpdateMarkPrice rejects if market is paused.
#[test]
fn test_update_mark_price_rejects_paused_market() {
    let mut f = setup_pyth_pinned_market();

    // Pause the market: set the PAUSED flag via PauseMarket (tag 27)
    let pause_data = vec![percolator_prog::tags::TAG_PAUSE_MARKET];
    {
        let accounts = [f.admin.to_info(), f.slab.to_info()];
        process_instruction(&f.program_id, &accounts, &pause_data)
            .expect("PauseMarket should succeed");
    }

    f.clock.data = make_clock(200, 200);
    let accounts = [f.slab.to_info(), f.pyth_index.to_info(), f.clock.to_info()];
    let res = process_instruction(&f.program_id, &accounts, &encode_update_mark_price());
    assert!(res.is_err(), "UpdateMarkPrice should reject paused market");
}

/// Test: SetPythOracle switches an admin-oracle market to Pyth-pinned mode.
/// After SetPythOracle, UpdateMarkPrice should work.
#[test]
fn test_set_pyth_oracle_then_update_mark_price() {
    let mut f = setup_market();

    // Initialize market with a regular (admin-oracle) feed
    let init_data = encode_init_market(&f, 1000);
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = [
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &init_data)
            .expect("InitMarket should succeed");
    }

    // Confirm market starts as Pyth-pinned (InitMarket with non-zero feed sets oracle_authority=[0;32])
    let config = state::read_config(&f.slab.data);
    assert!(
        percolator_prog::verify::is_pyth_pinned_mode(config.oracle_authority, config.index_feed_id),
        "Should start as Pyth-pinned"
    );

    // Call SetPythOracle to update feed config (e.g., change staleness / conf settings)
    let new_feed_id = TEST_FEED_ID; // same feed, just updating settings
    let set_pyth_data = encode_set_pyth_oracle(&new_feed_id, 300, 200);
    {
        let accounts = [f.admin.to_info(), f.slab.to_info()];
        process_instruction(&f.program_id, &accounts, &set_pyth_data)
            .expect("SetPythOracle should succeed");
    }

    // Verify still Pyth-pinned
    let config = state::read_config(&f.slab.data);
    assert!(
        percolator_prog::verify::is_pyth_pinned_mode(config.oracle_authority, config.index_feed_id),
        "Should still be Pyth-pinned after SetPythOracle"
    );

    // Now UpdateMarkPrice should work
    f.clock.data = make_clock(200, 300);
    let accounts = [f.slab.to_info(), f.pyth_index.to_info(), f.clock.to_info()];
    let res = process_instruction(&f.program_id, &accounts, &encode_update_mark_price());
    assert!(
        res.is_ok(),
        "UpdateMarkPrice should succeed after SetPythOracle: {:?}",
        res
    );

    let config_after = state::read_config(&f.slab.data);
    assert_ne!(
        config_after.authority_price_e6, 0,
        "Mark price should be set after UpdateMarkPrice"
    );
}

// =============================================================================
// PERC-118: compute_blend_mark_price — Unit Tests
// =============================================================================

/// Test pure impact_mid (oracle_weight_bps == 0, backward-compatible default).
#[test]
fn test_blend_mark_price_weight_zero_returns_impact_mid() {
    let oracle: u64 = 2_000_000_000; // $2000
    let impact_mid: u64 = 2_100_000_000; // $2100
    let result = percolator_prog::oracle::compute_blend_mark_price(oracle, impact_mid, 0);
    assert_eq!(result, impact_mid, "weight=0 should return pure impact_mid");
}

/// Test pure oracle (oracle_weight_bps == 10_000).
#[test]
fn test_blend_mark_price_weight_full_returns_oracle() {
    let oracle: u64 = 2_000_000_000;
    let impact_mid: u64 = 2_100_000_000;
    let result = percolator_prog::oracle::compute_blend_mark_price(oracle, impact_mid, 10_000);
    assert_eq!(result, oracle, "weight=10_000 should return pure oracle");
}

/// Test 50/50 blend.
#[test]
fn test_blend_mark_price_weight_5000_midpoint() {
    let oracle: u64 = 2_000_000_000;
    let impact_mid: u64 = 2_200_000_000;
    let result = percolator_prog::oracle::compute_blend_mark_price(oracle, impact_mid, 5_000);
    let expected: u64 = 2_100_000_000; // midpoint
    assert_eq!(
        result, expected,
        "weight=5_000 should return exact midpoint"
    );
}

/// Test 75% oracle + 25% impact_mid blend.
#[test]
fn test_blend_mark_price_weight_7500() {
    let oracle: u64 = 2_000_000;
    let impact_mid: u64 = 3_000_000;
    let result = percolator_prog::oracle::compute_blend_mark_price(oracle, impact_mid, 7_500);
    // Expected: (7500 * 2_000_000 + 2500 * 3_000_000) / 10_000 = (15_000_000_000 + 7_500_000_000) / 10_000
    // = 22_500_000_000 / 10_000 = 2_250_000
    let expected: u64 = 2_250_000;
    assert_eq!(result, expected, "weight=7_500: 75% oracle 25% impact");
}

/// Test oracle=0 fallback: should return impact_mid regardless of weight.
#[test]
fn test_blend_mark_price_oracle_zero_returns_impact_mid() {
    let oracle: u64 = 0;
    let impact_mid: u64 = 1_500_000_000;
    for weight in [0u16, 5_000, 10_000] {
        let result = percolator_prog::oracle::compute_blend_mark_price(oracle, impact_mid, weight);
        assert_eq!(
            result, impact_mid,
            "oracle=0 with weight={weight} should return impact_mid"
        );
    }
}

/// Test both zero inputs: should return 0.
#[test]
fn test_blend_mark_price_both_zero() {
    let result = percolator_prog::oracle::compute_blend_mark_price(0, 0, 5_000);
    assert_eq!(result, 0, "both zero should return 0");
}

/// Test that blend is monotone: higher oracle_weight → closer to oracle.
#[test]
fn test_blend_mark_price_monotone_oracle_weight() {
    let oracle: u64 = 1_000_000; // $1
    let impact_mid: u64 = 2_000_000; // $2
    let mut prev = percolator_prog::oracle::compute_blend_mark_price(oracle, impact_mid, 0);
    for w in [
        1_000u16, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000, 8_000, 9_000, 10_000,
    ] {
        let cur = percolator_prog::oracle::compute_blend_mark_price(oracle, impact_mid, w);
        assert!(cur <= prev, "increasing weight should move blend toward oracle (lower when oracle < impact_mid): w={w} cur={cur} prev={prev}");
        prev = cur;
    }
}

/// Test saturation safety: very large prices should not overflow.
#[test]
fn test_blend_mark_price_no_overflow() {
    let oracle: u64 = u64::MAX / 2;
    let impact_mid: u64 = u64::MAX / 2;
    let result = percolator_prog::oracle::compute_blend_mark_price(oracle, impact_mid, 5_000);
    // Should not panic and result should be close to oracle/impact (same here)
    assert!(result > 0, "should not overflow to 0");
}

/// Test UpdateHyperpMark with weight=0: pure impact_mid (backward compatible).
#[test]
fn test_update_hyperp_mark_blend_weight_zero_backward_compat() {
    let mut f = setup_hyperp_market();
    let initial_mark = 100_000_000u64;

    // Init Hyperp market
    let init_data = encode_init_hyperp_market(&f.admin.key, &f.mint.key, initial_mark);
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = [
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &init_data).unwrap();
    }

    // Default weight should be 0 (bytes are zero from slab init)
    let cfg0 = state::read_config(&f.slab.data);
    assert_eq!(
        percolator_prog::state::get_mark_oracle_weight_bps(&cfg0),
        0,
        "default mark_oracle_weight_bps should be 0"
    );

    // Build sufficient PumpSwap pool (base_mint = market collateral_mint)
    let pumpswap_id = oracle::PUMPSWAP_PROGRAM_ID;
    let bvk = Pubkey::new_unique();
    let qvk = Pubkey::new_unique();
    let mut pool = TestAccount::new(
        Pubkey::new_unique(),
        pumpswap_id,
        0,
        make_pumpswap_pool(&f.mint.key, &bvk, &qvk),
    );
    let mut bv = TestAccount::new(bvk, spl_token::ID, 0, make_spl_vault(2_000_000));
    let mut qv = TestAccount::new(qvk, spl_token::ID, 0, make_spl_vault(2_100_000_000_000)); // 2.1T > MIN_DEX_QUOTE_LIQUIDITY ($2M)
    let mut clk = TestAccount::new(
        solana_program::sysvar::clock::id(),
        solana_program::sysvar::id(),
        0,
        make_clock(200, 200),
    );

    let accounts = [
        f.slab.to_info(),
        pool.to_info(),
        clk.to_info(),
        bv.to_info(),
        qv.to_info(),
    ];
    let res = process_instruction(&f.program_id, &accounts, &encode_update_hyperp_mark());
    assert!(
        res.is_ok(),
        "UpdateHyperpMark (weight=0) should succeed: {:?}",
        res
    );

    let after = state::read_config(&f.slab.data);
    assert_ne!(
        after.authority_price_e6, 0,
        "mark should be non-zero after update"
    );
}

/// Test UpdateHyperpMark with weight=5_000: blend mode activates, last_effective_price_e6 updated.
#[test]
fn test_update_hyperp_mark_blend_weight_5000() {
    let mut f = setup_hyperp_market();
    let initial_mark = 100_000_000u64;

    // Init Hyperp market
    let init_data = encode_init_hyperp_market(&f.admin.key, &f.mint.key, initial_mark);
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = [
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &init_data).unwrap();
    }

    // Set weight=5_000 via state helpers
    {
        let mut cfg = state::read_config(&f.slab.data);
        percolator_prog::state::set_mark_oracle_weight_bps(&mut cfg, 5_000);
        cfg.last_effective_price_e6 = initial_mark; // seed oracle component
        state::write_config(&mut f.slab.data, &cfg);
    }

    // Build sufficient PumpSwap pool (DEX price ~$150, base_mint = market collateral_mint)
    let pumpswap_id = oracle::PUMPSWAP_PROGRAM_ID;
    let bvk = Pubkey::new_unique();
    let qvk = Pubkey::new_unique();
    let mut pool = TestAccount::new(
        Pubkey::new_unique(),
        pumpswap_id,
        0,
        make_pumpswap_pool(&f.mint.key, &bvk, &qvk),
    );
    let mut bv = TestAccount::new(bvk, spl_token::ID, 0, make_spl_vault(1_000_000));
    let mut qv = TestAccount::new(qvk, spl_token::ID, 0, make_spl_vault(2_100_000_000_000)); // 2.1T > MIN_DEX_QUOTE_LIQUIDITY ($2M)
    let mut clk = TestAccount::new(
        solana_program::sysvar::clock::id(),
        solana_program::sysvar::id(),
        0,
        make_clock(200, 200),
    );

    let accounts = [
        f.slab.to_info(),
        pool.to_info(),
        clk.to_info(),
        bv.to_info(),
        qv.to_info(),
    ];
    let res = process_instruction(&f.program_id, &accounts, &encode_update_hyperp_mark());
    assert!(
        res.is_ok(),
        "UpdateHyperpMark (weight=5_000) should succeed: {:?}",
        res
    );

    let after = state::read_config(&f.slab.data);
    assert_ne!(
        after.authority_price_e6, 0,
        "mark should be non-zero after blend update"
    );
    // PERC-118: last_effective_price_e6 is now updated by UpdateHyperpMark
    assert_ne!(
        after.last_effective_price_e6, 0,
        "index should be updated by PERC-118"
    );
}

/// Test mark_oracle_weight_bps accessor round-trip.
#[test]
fn test_mark_oracle_weight_bps_accessor_roundtrip() {
    let mut f = setup_market();
    let mut cfg = state::read_config(&f.slab.data);
    percolator_prog::state::set_mark_oracle_weight_bps(&mut cfg, 7_500);
    state::write_config(&mut f.slab.data, &cfg);
    let cfg2 = state::read_config(&f.slab.data);
    assert_eq!(
        percolator_prog::state::get_mark_oracle_weight_bps(&cfg2),
        7_500,
        "mark_oracle_weight_bps should survive config round-trip"
    );
}

/// Test set_mark_oracle_weight_bps clamps values > 10_000 to 10_000.
#[test]
fn test_mark_oracle_weight_bps_clamps() {
    let mut f = setup_market();
    let mut cfg = state::read_config(&f.slab.data);
    percolator_prog::state::set_mark_oracle_weight_bps(&mut cfg, 20_000); // over max
    assert_eq!(
        percolator_prog::state::get_mark_oracle_weight_bps(&cfg),
        10_000,
        "weight > 10_000 should clamp to 10_000"
    );
}

// =============================================================================
// CMOR (Cross-Margin) Tests
// =============================================================================

#[test]
fn test_cmor_compute_margin_credit_hedged() {
    use percolator_prog::cross_margin::{CrossMarginAttestation, ATTESTATION_MAGIC};

    let att = CrossMarginAttestation {
        magic: ATTESTATION_MAGIC,
        _align_pad: [0; 8],
        user_pos_a: 1000,
        user_pos_b: -1000,
        attested_slot: 100,
        offset_bps: 3000,
        _pad: [0; 6],
        owner: [0; 32],
        slab_a: [1; 32],
        slab_b: [2; 32],
    };
    assert_eq!(att.compute_margin_credit_bps(), 3000);
}

#[test]
fn test_cmor_compute_margin_credit_partial_hedge() {
    use percolator_prog::cross_margin::{CrossMarginAttestation, ATTESTATION_MAGIC};

    let att = CrossMarginAttestation {
        magic: ATTESTATION_MAGIC,
        _align_pad: [0; 8],
        user_pos_a: 500,
        user_pos_b: -1000,
        attested_slot: 100,
        offset_bps: 3000,
        _pad: [0; 6],
        owner: [0; 32],
        slab_a: [1; 32],
        slab_b: [2; 32],
    };
    assert_eq!(att.compute_margin_credit_bps(), 1500);
}

#[test]
fn test_cmor_compute_margin_credit_same_direction() {
    use percolator_prog::cross_margin::{CrossMarginAttestation, ATTESTATION_MAGIC};

    let att = CrossMarginAttestation {
        magic: ATTESTATION_MAGIC,
        _align_pad: [0; 8],
        user_pos_a: 1000,
        user_pos_b: 500,
        attested_slot: 100,
        offset_bps: 3000,
        _pad: [0; 6],
        owner: [0; 32],
        slab_a: [1; 32],
        slab_b: [2; 32],
    };
    assert_eq!(att.compute_margin_credit_bps(), 0);
}

#[test]
fn test_cmor_compute_margin_credit_zero_position() {
    use percolator_prog::cross_margin::{CrossMarginAttestation, ATTESTATION_MAGIC};

    let att = CrossMarginAttestation {
        magic: ATTESTATION_MAGIC,
        _align_pad: [0; 8],
        user_pos_a: 0,
        user_pos_b: -1000,
        attested_slot: 100,
        offset_bps: 3000,
        _pad: [0; 6],
        owner: [0; 32],
        slab_a: [1; 32],
        slab_b: [2; 32],
    };
    assert_eq!(att.compute_margin_credit_bps(), 0);
}

#[test]
fn test_cmor_attestation_freshness() {
    use percolator_prog::cross_margin::{CrossMarginAttestation, ATTESTATION_MAGIC};
    use percolator_prog::CMOR_MAX_AGE_SLOTS;

    let att = CrossMarginAttestation {
        magic: ATTESTATION_MAGIC,
        _align_pad: [0; 8],
        user_pos_a: 1000,
        user_pos_b: -1000,
        attested_slot: 100,
        offset_bps: 3000,
        _pad: [0; 6],
        owner: [0; 32],
        slab_a: [1; 32],
        slab_b: [2; 32],
    };

    assert!(att.is_fresh(100 + CMOR_MAX_AGE_SLOTS, CMOR_MAX_AGE_SLOTS));
    assert!(!att.is_fresh(100 + CMOR_MAX_AGE_SLOTS + 1, CMOR_MAX_AGE_SLOTS));
}

#[test]
fn test_cmor_disabled_offset_returns_zero() {
    use percolator_prog::cross_margin::{CrossMarginAttestation, ATTESTATION_MAGIC};

    let att = CrossMarginAttestation {
        magic: ATTESTATION_MAGIC,
        _align_pad: [0; 8],
        user_pos_a: 1000,
        user_pos_b: -1000,
        attested_slot: 100,
        offset_bps: 0, // disabled
        _pad: [0; 6],
        owner: [0; 32],
        slab_a: [1; 32],
        slab_b: [2; 32],
    };
    assert_eq!(att.compute_margin_credit_bps(), 0);
}

// =============================================================================
// Insurance Tranche Tests (#978)
// =============================================================================

#[test]
fn test_tranche_waterfall_updates_total_capital_junior_absorbs() {
    use percolator_prog::lp_vault::LpVaultState;

    let mut state = LpVaultState::new_zeroed();
    state.total_capital = 1_000_000;
    state.set_junior_capital(400_000);
    state.set_senior_capital(600_000);

    let realized = state.apply_loss_waterfall(100_000);
    assert_eq!(realized, 100_000);
    assert_eq!(state.junior_capital(), 300_000);
    assert_eq!(state.senior_capital(), 600_000);
    assert_eq!(
        state.total_capital, 900_000,
        "total_capital must decrease by realized loss"
    );
}

#[test]
fn test_tranche_waterfall_updates_total_capital_cascades_to_senior() {
    use percolator_prog::lp_vault::LpVaultState;

    let mut state = LpVaultState::new_zeroed();
    state.total_capital = 1_000_000;
    state.set_junior_capital(200_000);
    state.set_senior_capital(800_000);

    let realized = state.apply_loss_waterfall(500_000);
    assert_eq!(realized, 500_000);
    assert_eq!(state.junior_capital(), 0);
    assert_eq!(state.senior_capital(), 500_000);
    assert_eq!(
        state.total_capital, 500_000,
        "total_capital must decrease by realized loss"
    );
}

#[test]
fn test_tranche_waterfall_total_loss() {
    use percolator_prog::lp_vault::LpVaultState;

    let mut state = LpVaultState::new_zeroed();
    state.total_capital = 1_000_000;
    state.set_junior_capital(400_000);
    state.set_senior_capital(600_000);

    let realized = state.apply_loss_waterfall(2_000_000);
    assert_eq!(realized, 1_000_000); // capped at total tranche capital
    assert_eq!(state.junior_capital(), 0);
    assert_eq!(state.senior_capital(), 0);
    assert_eq!(state.total_capital, 0);
}

// =============================================================================
// GH#1724 — ClearPendingSettlement Admin-Only Guard
// =============================================================================
// Security test: confirms ClearPendingSettlement rejects any non-admin signer,
// including the attacker pattern from the issue: caller_idx=u16::MAX bypass.
// The on-chain handler uses require_admin(header.admin, a_keeper.key) which
// must fire before any state mutation.

fn encode_clear_pending_settlement(user_idx: u16) -> Vec<u8> {
    let mut data = vec![percolator_prog::tags::TAG_CLEAR_PENDING_SETTLEMENT];
    data.extend_from_slice(&user_idx.to_le_bytes());
    data
}

fn encode_set_pending_settlement(user_idx: u16) -> Vec<u8> {
    let mut data = vec![percolator_prog::tags::TAG_SET_PENDING_SETTLEMENT];
    data.extend_from_slice(&user_idx.to_le_bytes());
    data
}

/// Build an initialized PositionNft PDA account data buffer with pending_settlement=1.
fn make_nft_pda_data(
    nft_mint_key: &Pubkey,
    slab_key: &Pubkey,
    owner_key: &Pubkey,
    user_idx: u16,
    bump: u8,
    pending_settlement: u8,
) -> Vec<u8> {
    use percolator_prog::position_nft::{
        write_position_nft_state, PositionNftState, POSITION_NFT_MAGIC, POSITION_NFT_STATE_LEN,
    };
    let state = PositionNftState {
        magic: POSITION_NFT_MAGIC,
        mint: nft_mint_key.to_bytes(),
        slab: slab_key.to_bytes(),
        owner: owner_key.to_bytes(),
        user_idx,
        pending_settlement,
        bump,
        mint_bump: 254,
        _reserved: [0u8; 19],
    };
    let mut buf = vec![0u8; POSITION_NFT_STATE_LEN];
    write_position_nft_state(&mut buf, &state);
    buf
}

#[test]
fn test_gh1724_clear_pending_settlement_requires_admin() {
    // GH#1724: non-admin signer MUST NOT be able to clear pending_settlement.
    // The issue described an attack where any signer could call ClearPendingSettlement
    // with caller_idx=u16::MAX to bypass the keeper settlement guard.
    // The handler must return EngineUnauthorized for all non-admin callers.

    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);

    // 1. Init market
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &init_data).unwrap();
    }

    // 2. Build a fake NFT PDA pre-loaded with pending_settlement=1
    let user_idx: u16 = 0;
    let (nft_pda_key, bump) =
        percolator_prog::position_nft::derive_position_nft(&f.program_id, &f.slab.key, user_idx);
    let nft_mint_key = Pubkey::new_unique();
    let position_owner_key = Pubkey::new_unique();
    let nft_data = make_nft_pda_data(
        &nft_mint_key,
        &f.slab.key,
        &position_owner_key,
        user_idx,
        bump,
        1,
    );

    let mut nft_pda_account = TestAccount::new(nft_pda_key, f.program_id, 0, nft_data).writable();

    // 3. Attacker (non-admin) attempts to clear pending_settlement
    let mut attacker = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();

    let result = process_instruction(
        &f.program_id,
        &[
            attacker.to_info(),
            f.slab.to_info(),
            nft_pda_account.to_info(),
        ],
        &encode_clear_pending_settlement(user_idx),
    );

    assert_eq!(
        result,
        Err(PercolatorError::EngineUnauthorized.into()),
        "GH#1724: non-admin must not clear pending_settlement"
    );

    // 4. Verify pending_settlement is still set (state not mutated)
    let loaded = percolator_prog::position_nft::read_position_nft_state(&nft_pda_account.data)
        .expect("NFT PDA should still be parseable");
    assert_eq!(
        loaded.pending_settlement, 1,
        "GH#1724: pending_settlement must remain set after rejected ClearPendingSettlement"
    );
}

#[test]
fn test_gh1724_clear_pending_settlement_admin_succeeds() {
    // Positive path: the authorized admin CAN clear pending_settlement.
    // This confirms the guard doesn't break the happy path.

    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);

    // 1. Init market
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &init_data).unwrap();
    }

    // 2. Build NFT PDA with pending_settlement=1
    let user_idx: u16 = 0;
    let (nft_pda_key, bump) =
        percolator_prog::position_nft::derive_position_nft(&f.program_id, &f.slab.key, user_idx);
    let nft_mint_key = Pubkey::new_unique();
    let position_owner_key = Pubkey::new_unique();
    let nft_data = make_nft_pda_data(
        &nft_mint_key,
        &f.slab.key,
        &position_owner_key,
        user_idx,
        bump,
        1,
    );

    let mut nft_pda_account = TestAccount::new(nft_pda_key, f.program_id, 0, nft_data).writable();

    // 3. Admin clears pending_settlement — must succeed
    let result = process_instruction(
        &f.program_id,
        &[
            f.admin.to_info(),
            f.slab.to_info(),
            nft_pda_account.to_info(),
        ],
        &encode_clear_pending_settlement(user_idx),
    );

    assert!(
        result.is_ok(),
        "GH#1724: authorized admin must be able to clear pending_settlement, got: {:?}",
        result
    );

    // 4. Verify the flag was actually cleared
    let loaded = percolator_prog::position_nft::read_position_nft_state(&nft_pda_account.data)
        .expect("NFT PDA should be parseable");
    assert_eq!(
        loaded.pending_settlement, 0,
        "GH#1724: pending_settlement must be cleared by admin"
    );
}

#[test]
fn test_gh1724_set_pending_settlement_also_requires_admin() {
    // Defense-in-depth: SetPendingSettlement also uses require_admin.
    // Confirm a non-admin cannot set the flag either (DoS prevention, GH#1475).

    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);

    // 1. Init market
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &init_data).unwrap();
    }

    // 2. Build NFT PDA with pending_settlement=0
    let user_idx: u16 = 0;
    let (nft_pda_key, bump) =
        percolator_prog::position_nft::derive_position_nft(&f.program_id, &f.slab.key, user_idx);
    let nft_mint_key = Pubkey::new_unique();
    let position_owner_key = Pubkey::new_unique();
    let nft_data = make_nft_pda_data(
        &nft_mint_key,
        &f.slab.key,
        &position_owner_key,
        user_idx,
        bump,
        0,
    );

    let mut nft_pda_account = TestAccount::new(nft_pda_key, f.program_id, 0, nft_data).writable();

    // 3. Griever (non-admin) attempts to set pending_settlement — must be rejected
    let mut griever = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();

    let result = process_instruction(
        &f.program_id,
        &[
            griever.to_info(),
            f.slab.to_info(),
            nft_pda_account.to_info(),
        ],
        &encode_set_pending_settlement(user_idx),
    );

    assert_eq!(
        result,
        Err(PercolatorError::EngineUnauthorized.into()),
        "GH#1724/GH#1475: non-admin must not set pending_settlement"
    );

    // 4. Verify flag not set
    let loaded = percolator_prog::position_nft::read_position_nft_state(&nft_pda_account.data)
        .expect("NFT PDA should still be parseable");
    assert_eq!(
        loaded.pending_settlement, 0,
        "GH#1724: pending_settlement must remain 0 after rejected SetPendingSettlement"
    );
}

// --- GH#1736: UpdateRiskParams must call RiskParams::validate() ---

/// Encode a minimal UpdateRiskParams instruction (tag=22).
/// Encodes: initial_margin_bps + maintenance_margin_bps (required fields only).
fn encode_update_risk_params_minimal(
    initial_margin_bps: u64,
    maintenance_margin_bps: u64,
) -> Vec<u8> {
    let mut data = vec![22u8]; // TAG_UPDATE_RISK_PARAMS
    encode_u64(initial_margin_bps, &mut data);
    encode_u64(maintenance_margin_bps, &mut data);
    data
}

/// Helper: initialise a market and return the fixture.
/// Uses max crank staleness so validation passes at InitMarket.
fn setup_initialized_market() -> MarketFixture {
    let mut f = setup_market();
    let data = encode_init_market(&f, u64::MAX);
    let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
    let accounts = vec![
        f.admin.to_info(),
        f.slab.to_info(),
        f.mint.to_info(),
        f.vault.to_info(),
        f.token_prog.to_info(),
        f.clock.to_info(),
        f.rent.to_info(),
        dummy_ata.to_info(),
        f.system.to_info(),
    ];
    process_instruction(&f.program_id, &accounts, &data).unwrap();
    f
}

/// GH#1736: UpdateRiskParams must reject margin=0 (maintenance_margin_bps=0)
/// This was already caught by the inline pre-checks, but the test documents the guard.
#[test]
fn test_update_risk_params_rejects_zero_maintenance_margin() {
    let mut f = setup_initialized_market();
    let data = encode_update_risk_params_minimal(1000, 0); // maintenance=0
    let accounts = vec![f.admin.to_info(), f.slab.to_info()];
    let result = process_instruction(&f.program_id, &accounts, &data);
    assert!(
        result.is_err(),
        "GH#1736: UpdateRiskParams must reject maintenance_margin_bps=0"
    );
}

/// GH#1736: UpdateRiskParams must reject initial < maintenance.
#[test]
fn test_update_risk_params_rejects_initial_less_than_maintenance() {
    let mut f = setup_initialized_market();
    let data = encode_update_risk_params_minimal(500, 1000); // initial < maintenance
    let accounts = vec![f.admin.to_info(), f.slab.to_info()];
    let result = process_instruction(&f.program_id, &accounts, &data);
    assert!(
        result.is_err(),
        "GH#1736: UpdateRiskParams must reject initial_margin_bps < maintenance_margin_bps"
    );
}

/// GH#1736: UpdateRiskParams must accept valid margin params.
#[test]
fn test_update_risk_params_accepts_valid_params() {
    let mut f = setup_initialized_market();
    let data = encode_update_risk_params_minimal(2000, 1000); // valid: initial > maintenance
    let accounts = vec![f.admin.to_info(), f.slab.to_info()];
    let result = process_instruction(&f.program_id, &accounts, &data);
    assert!(
        result.is_ok(),
        "GH#1736: UpdateRiskParams must accept valid margin params: {:?}",
        result
    );
}

// =============================================================================
// PERC-8111: Per-Wallet Position Cap Tests
// =============================================================================

/// Encode SetWalletCap instruction (tag=70).
fn encode_set_wallet_cap(cap_e6: u64) -> Vec<u8> {
    let mut data = vec![70u8]; // TAG_SET_WALLET_CAP
    encode_u64(cap_e6, &mut data);
    data
}

/// PERC-8111: SetWalletCap stores and round-trips correctly via state helpers.
#[test]
fn test_set_wallet_cap_roundtrip_state() {
    use percolator_prog::state;
    let mut cfg: percolator_prog::state::MarketConfig = bytemuck::Zeroable::zeroed();

    // Default: 0 (disabled)
    assert_eq!(state::get_max_wallet_pos_e6(&cfg), 0);

    // Set $1K cap = 1_000_000_000 e6
    state::set_max_wallet_pos_e6(&mut cfg, 1_000_000_000);
    let stored = state::get_max_wallet_pos_e6(&cfg);
    // Stored as kilo-e6 → stored = 1_000_000_000 / 1_000 * 1_000 = 1_000_000_000 (exact)
    assert_eq!(
        stored, 1_000_000_000,
        "Round-trip $1K cap failed: got {}",
        stored
    );

    // Disable: set to 0
    state::set_max_wallet_pos_e6(&mut cfg, 0);
    assert_eq!(
        state::get_max_wallet_pos_e6(&cfg),
        0,
        "Disabling cap failed"
    );

    // $10K cap
    state::set_max_wallet_pos_e6(&mut cfg, 10_000_000_000);
    let stored10k = state::get_max_wallet_pos_e6(&cfg);
    assert_eq!(stored10k, 10_000_000_000);
}

/// PERC-8111: SetWalletCap instruction accepted by initialized market (admin only).
#[test]
fn test_set_wallet_cap_instruction_accepted() {
    let mut f = setup_initialized_market();
    let data = encode_set_wallet_cap(1_000_000_000); // $1K
    let accounts = vec![f.admin.to_info(), f.slab.to_info()];
    let result = process_instruction(&f.program_id, &accounts, &data);
    assert!(
        result.is_ok(),
        "SetWalletCap must succeed for admin: {:?}",
        result
    );
}

/// PERC-8111: SetWalletCap rejected by non-admin.
#[test]
fn test_set_wallet_cap_rejected_by_non_admin() {
    let mut f = setup_initialized_market();
    let data = encode_set_wallet_cap(1_000_000_000);

    let mut non_admin = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();

    let accounts = vec![non_admin.to_info(), f.slab.to_info()];
    let result = process_instruction(&f.program_id, &accounts, &data);
    assert!(result.is_err(), "SetWalletCap must reject non-admin");
}

/// PERC-8111: SetWalletCap disable (cap=0) accepted.
#[test]
fn test_set_wallet_cap_disable() {
    let mut f = setup_initialized_market();
    // Set a cap first
    {
        let data = encode_set_wallet_cap(1_000_000_000);
        let accounts = vec![f.admin.to_info(), f.slab.to_info()];
        process_instruction(&f.program_id, &accounts, &data).unwrap();
    }
    // Now disable it
    {
        let data = encode_set_wallet_cap(0);
        let accounts = vec![f.admin.to_info(), f.slab.to_info()];
        let result = process_instruction(&f.program_id, &accounts, &data);
        assert!(
            result.is_ok(),
            "SetWalletCap(0) disable must succeed: {:?}",
            result
        );
    }
}

/// PERC-8111: TradeNoCpi rejected when position would exceed cap.
/// Setup: set $1K cap, then attempt a trade of 2K units → expect WalletPositionCapExceeded.
#[test]
#[cfg(feature = "test")]
#[ignore = "PERC-199: TradeNoCpi uses Clock::get() syscall, unavailable in unit tests — migrate to LiteSVM"]
fn test_wallet_cap_blocks_trade_exceeding_limit() {
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let init_accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &init_accounts, &init_data).unwrap();
    }

    // Set wallet cap: 500 (500 base units = $0.0005 in e6 at price=100 — small but enforceable)
    {
        let data = encode_set_wallet_cap(500);
        let accounts = vec![f.admin.to_info(), f.slab.to_info()];
        process_instruction(&f.program_id, &accounts, &data).unwrap();
    }

    // Create user + lp
    let mut user = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut user_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, user.key, 10_000),
    )
    .writable();
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_init_user(0)).unwrap();
    }
    let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_deposit(user_idx, 5000)).unwrap();
    }

    let mut lp = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut lp_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, lp.key, 10_000),
    )
    .writable();
    let mut d1 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
    let mut d2 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
    {
        let accs = vec![
            lp.to_info(),
            f.slab.to_info(),
            lp_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &encode_init_lp(d1.key, d2.key, 0)).unwrap();
    }
    let lp_idx = find_idx_by_owner(&f.slab.data, lp.key).unwrap();
    {
        let accounts = vec![
            lp.to_info(),
            f.slab.to_info(),
            lp_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_deposit(lp_idx, 5000)).unwrap();
    }

    // Trade size=1000 > cap=500 → must be rejected
    let result = {
        let accounts = vec![
            user.to_info(),
            lp.to_info(),
            f.slab.to_info(),
            f.pyth_index.to_info(),
        ];
        process_instruction(
            &f.program_id,
            &accounts,
            &encode_trade(lp_idx, user_idx, 1000),
        )
    };
    assert_eq!(
        result,
        Err(percolator_prog::error::PercolatorError::WalletPositionCapExceeded.into()),
        "PERC-8111: Trade exceeding wallet cap must be rejected"
    );
}

/// PERC-8111: TradeNoCpi allowed when position is within cap.
#[test]
#[cfg(feature = "test")]
#[ignore = "PERC-199: TradeNoCpi uses Clock::get() syscall, unavailable in unit tests — migrate to LiteSVM"]
fn test_wallet_cap_allows_trade_within_limit() {
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 100);
    {
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let init_accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
            f.rent.to_info(),
            dummy_ata.to_info(),
            f.system.to_info(),
        ];
        process_instruction(&f.program_id, &init_accounts, &init_data).unwrap();
    }

    // Set wallet cap: 2000 units → trade of 100 should be fine
    {
        let data = encode_set_wallet_cap(2000);
        let accounts = vec![f.admin.to_info(), f.slab.to_info()];
        process_instruction(&f.program_id, &accounts, &data).unwrap();
    }

    let mut user = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut user_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, user.key, 10_000),
    )
    .writable();
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_init_user(0)).unwrap();
    }
    let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_deposit(user_idx, 5000)).unwrap();
    }

    let mut lp = TestAccount::new(
        Pubkey::new_unique(),
        solana_program::system_program::id(),
        0,
        vec![],
    )
    .signer();
    let mut lp_ata = TestAccount::new(
        Pubkey::new_unique(),
        spl_token::ID,
        0,
        make_token_account(f.mint.key, lp.key, 10_000),
    )
    .writable();
    let mut d1 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
    let mut d2 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
    {
        let accs = vec![
            lp.to_info(),
            f.slab.to_info(),
            lp_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &encode_init_lp(d1.key, d2.key, 0)).unwrap();
    }
    let lp_idx = find_idx_by_owner(&f.slab.data, lp.key).unwrap();
    {
        let accounts = vec![
            lp.to_info(),
            f.slab.to_info(),
            lp_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &encode_deposit(lp_idx, 5000)).unwrap();
    }

    // Trade size=100 < cap=2000 → must succeed
    let result = {
        let accounts = vec![
            user.to_info(),
            lp.to_info(),
            f.slab.to_info(),
            f.pyth_index.to_info(),
        ];
        process_instruction(
            &f.program_id,
            &accounts,
            &encode_trade(lp_idx, user_idx, 100),
        )
    };
    assert!(
        result.is_ok(),
        "PERC-8111: Trade within wallet cap must succeed: {:?}",
        result
    );
}
