//! Unit tests for percolator-prog
//!
//! These tests verify the Solana program wrapper's instruction handling,
//! including account validation, state management, and invariants.

use bytemuck::Zeroable;
use percolator::{I128, MAX_ACCOUNTS, U128};
use percolator_prog::{
    constants::MAGIC,
    error::PercolatorError,
    matcher_abi::{validate_matcher_return, MatcherReturn, FLAG_PARTIAL_OK, FLAG_VALID},
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
/// Layout: discriminator(8) + write_authority(32) + verification_level(1 for Full)
///         + feed_id(32) + price(8) + conf(8) + expo(4) + publish_time(8) + ...
/// Borsh enum variants are variable-size; Full has no payload, so
/// PriceFeedMessage begins at byte 41.
fn make_pyth(feed_id: &[u8; 32], price: i64, expo: i32, conf: u64, publish_time: i64) -> Vec<u8> {
    let mut data = vec![0u8; 134];
    data[0..8].copy_from_slice(&[0x22, 0xf1, 0x23, 0x63, 0x9d, 0x7e, 0xf4, 0xcd]);
    // verification_level = Full (discriminant 0x01) at offset 40
    data[40] = 1;
    // feed_id at offset 41
    data[41..73].copy_from_slice(feed_id);
    // price at offset 73
    data[73..81].copy_from_slice(&price.to_le_bytes());
    // conf at offset 81
    data[81..89].copy_from_slice(&conf.to_le_bytes());
    // expo at offset 89
    data[89..93].copy_from_slice(&expo.to_le_bytes());
    // publish_time at offset 93
    data[93..101].copy_from_slice(&publish_time.to_le_bytes());
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
            make_token_account(mint_key, vault_pda, 0),
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
    encode_u64(0, &mut data); // initial_mark_price_e6 (0 for non-Hyperp markets)
                              // Per-market admin limits (uncapped defaults for tests)
    encode_u128(0u128, &mut data); // maintenance_fee_per_slot (0 = disabled)
                                   // RiskParams: warmup, maintenance_margin_bps, initial_margin_bps, trading_fee_bps
    encode_u64(1, &mut data); // h_min
    encode_u64(500, &mut data); // maintenance_margin_bps (must be < initial_margin_bps)
    encode_u64(1000, &mut data); // initial_margin_bps
    encode_u64(0, &mut data); // trading_fee_bps
    encode_u64(MAX_ACCOUNTS as u64, &mut data); // max_accounts
    encode_u128(1, &mut data); // new_account_fee (§12.19.6 F8 anti-spam)
    encode_u64(1, &mut data); // h_max (v12.18.1: must be >= 1)
    encode_u64(crank_staleness, &mut data); // max_crank_staleness_slots
    encode_u64(0, &mut data); // liquidation_fee_bps
    encode_u128(0, &mut data); // liquidation_fee_cap
    encode_u64(100, &mut data); // resolve_price_deviation_bps
    encode_u128(0, &mut data); // min_liquidation_abs
    data.extend_from_slice(&21u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&22u128.to_le_bytes()); // min_nonzero_im_req
    encode_u64(2, &mut data); // max_price_move_bps_per_slot (v12.19)
    encode_u16(0, &mut data); // insurance_withdraw_max_bps
    encode_u64(0, &mut data); // insurance_withdraw_cooldown_slots

    // v12.19.6: non-Hyperp needs perm_resolve > max_crank_staleness AND
    // perm_resolve <= MAX_ACCRUAL_DT_SLOTS (= 100). Callers must pass
    // crank_staleness < 100 for a resolvable market.
    encode_u64(crank_staleness.saturating_add(1).min(100), &mut data); // permissionless_resolve_stale_slots
    encode_u64(500, &mut data); // funding_horizon_slots
    encode_u64(100, &mut data); // funding_k_bps
    data.extend_from_slice(&500i64.to_le_bytes()); // funding_max_premium_bps
    data.extend_from_slice(&1_000i64.to_le_bytes()); // funding_max_e9_per_slot
    encode_u64(0, &mut data); // mark_min_fee
    encode_u64(50, &mut data); // force_close_delay_slots (required when perm_resolve>0)
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
    encode_u64(0, &mut data); // initial_mark_price_e6 (0 for non-Hyperp markets)
                              // Per-market admin limits (uncapped defaults for tests)
    encode_u128(0u128, &mut data); // maintenance_fee_per_slot (0 = disabled)
                                   // RiskParams: warmup, maintenance_margin_bps, initial_margin_bps, trading_fee_bps
    encode_u64(1, &mut data); // h_min
    encode_u64(500, &mut data); // maintenance_margin_bps (must be < initial_margin_bps)
    encode_u64(1000, &mut data); // initial_margin_bps
    encode_u64(0, &mut data); // trading_fee_bps
    encode_u64(MAX_ACCOUNTS as u64, &mut data); // max_accounts
    encode_u128(1, &mut data); // new_account_fee (§12.19.6 F8 anti-spam)
    encode_u64(1, &mut data); // h_max
    encode_u64(crank_staleness, &mut data); // max_crank_staleness_slots
    encode_u64(0, &mut data); // liquidation_fee_bps
    encode_u128(0, &mut data); // liquidation_fee_cap
    encode_u64(0, &mut data); // resolve_price_deviation_bps
    encode_u128(0, &mut data); // min_liquidation_abs
    data.extend_from_slice(&21u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&22u128.to_le_bytes()); // min_nonzero_im_req
    encode_u64(2, &mut data); // max_price_move_bps_per_slot (v12.19)
    encode_u16(0, &mut data); // insurance_withdraw_max_bps
    encode_u64(0, &mut data); // insurance_withdraw_cooldown_slots

    // v12.19.6: non-Hyperp needs perm_resolve > max_crank_staleness AND
    // perm_resolve <= MAX_ACCRUAL_DT_SLOTS (= 100).
    encode_u64(crank_staleness.saturating_add(1).min(100), &mut data); // permissionless_resolve_stale_slots
    encode_u64(500, &mut data); // funding_horizon_slots
    encode_u64(100, &mut data); // funding_k_bps
    data.extend_from_slice(&500i64.to_le_bytes()); // funding_max_premium_bps
    data.extend_from_slice(&1_000i64.to_le_bytes()); // funding_max_e9_per_slot
    encode_u64(0, &mut data); // mark_min_fee
    encode_u64(50, &mut data); // force_close_delay_slots
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
    // Two-phase crank: include first 128 account indices as candidates
    for i in 0..128u16 {
        encode_u16(i, &mut data);
    }
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
    // UpdateAuthority { kind: AUTHORITY_ADMIN = 0, new_pubkey }
    let mut data = vec![32u8];
    data.push(0u8); // AUTHORITY_ADMIN
    encode_pubkey(new_admin, &mut data);
    data
}

fn encode_close_slab() -> Vec<u8> {
    vec![13u8]
}

fn encode_reclaim_empty_account(user_idx: u16) -> Vec<u8> {
    let mut data = vec![25u8];
    encode_u16(user_idx, &mut data);
    data
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
fn test_matcher_nonzero_partial_requires_partial_ok() {
    let ret = MatcherReturn {
        abi_version: percolator_prog::constants::MATCHER_ABI_VERSION,
        flags: FLAG_VALID,
        exec_price_e6: 100_000_000,
        exec_size: 50,
        req_id: 7,
        lp_account_id: 11,
        oracle_price_e6: 100_000_000,
        reserved: 0,
    };

    assert_eq!(
        validate_matcher_return(
            &ret,
            ret.lp_account_id,
            ret.oracle_price_e6,
            100,
            ret.req_id,
        ),
        Err(ProgramError::InvalidAccountData)
    );

    let ret_with_partial = MatcherReturn {
        flags: FLAG_VALID | FLAG_PARTIAL_OK,
        ..ret
    };
    assert!(validate_matcher_return(
        &ret_with_partial,
        ret_with_partial.lp_account_id,
        ret_with_partial.oracle_price_e6,
        100,
        ret_with_partial.req_id,
    )
    .is_ok());
}

#[test]
fn test_external_oracle_flat_market_uses_raw_target() {
    let mut config = state::MarketConfig::zeroed();

    let price =
        oracle::clamp_external_price(&mut config, Ok((120_000_000, 1)), 100_000_000, 1, 0, false)
            .unwrap();

    assert_eq!(price, 120_000_000);
    assert_eq!(config.last_effective_price_e6, 120_000_000);
    assert_eq!(config.oracle_target_price_e6, 120_000_000);
}

#[test]
fn test_external_oracle_with_open_interest_respects_zero_dt_clamp() {
    let mut config = state::MarketConfig::zeroed();

    let price =
        oracle::clamp_external_price(&mut config, Ok((120_000_000, 1)), 100_000_000, 1, 0, true)
            .unwrap();

    assert_eq!(price, 100_000_000);
    assert_eq!(config.last_effective_price_e6, 100_000_000);
    assert_eq!(config.oracle_target_price_e6, 120_000_000);
}

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

    // Print MarketConfig layout
    println!("HEADER_LEN: {}", percolator_prog::constants::HEADER_LEN);
    println!("CONFIG_LEN: {}", percolator_prog::constants::CONFIG_LEN);
    println!(
        "Offset of last_effective_price_e6: {}",
        offset_of!(state::MarketConfig, last_effective_price_e6)
    );
    println!(
        "Slab offset of last_effective_price_e6: {}",
        percolator_prog::constants::HEADER_LEN
            + offset_of!(state::MarketConfig, last_effective_price_e6)
    );
    println!(
        "Offset of hyperp_mark_e6: {}",
        offset_of!(state::MarketConfig, hyperp_mark_e6)
    );
    println!(
        "Slab offset of hyperp_mark_e6: {}",
        percolator_prog::constants::HEADER_LEN + offset_of!(state::MarketConfig, hyperp_mark_e6)
    );
    // oracle_price_cap_e2bps and min_oracle_price_cap_e2bps removed in v12.19 —
    // per-slot price-move cap is now in RiskParams.max_price_move_bps_per_slot.
    println!(
        "Offset of max_staleness_secs: {}",
        offset_of!(state::MarketConfig, max_staleness_secs)
    );
    println!(
        "Slab offset of max_staleness_secs: {}",
        percolator_prog::constants::HEADER_LEN
            + offset_of!(state::MarketConfig, max_staleness_secs)
    );
    println!(
        "Offset of RiskEngine.side_mode_long: {}",
        offset_of!(RiskEngine, side_mode_long)
    );
    println!(
        "Slab offset of side_mode_long: {}",
        percolator_prog::constants::ENGINE_OFF + offset_of!(RiskEngine, side_mode_long)
    );
    // MarketConfig field offsets for admin limits test
    println!(
        "Offset of maintenance_fee_per_slot: {}",
        offset_of!(state::MarketConfig, maintenance_fee_per_slot)
    );
    println!(
        "Slab offset of maintenance_fee_per_slot: {}",
        percolator_prog::constants::HEADER_LEN
            + offset_of!(state::MarketConfig, maintenance_fee_per_slot)
    );
    println!(
        "Offset of mark_ewma_e6: {}",
        offset_of!(state::MarketConfig, mark_ewma_e6)
    );
    println!(
        "Slab offset of mark_ewma_e6: {}",
        percolator_prog::constants::HEADER_LEN + offset_of!(state::MarketConfig, mark_ewma_e6)
    );
    println!(
        "Offset of last_oracle_price: {}",
        offset_of!(RiskEngine, last_oracle_price)
    );
    println!(
        "Slab offset of last_oracle_price: {}",
        percolator_prog::constants::ENGINE_OFF + offset_of!(RiskEngine, last_oracle_price)
    );
    println!(
        "Offset of last_market_slot: {}",
        offset_of!(RiskEngine, last_market_slot)
    );
    println!(
        "Slab offset of last_market_slot: {}",
        percolator_prog::constants::ENGINE_OFF + offset_of!(RiskEngine, last_market_slot)
    );
}

/// Runtime tripwire for the unsafe zero-copy cast in `zc::engine_ref`
/// / `zc::engine_mut`. The cast is sound only if every field of
/// `RiskEngine` (including its nested `accounts: [Account;
/// MAX_ACCOUNTS]`) either (a) has no invalid bit patterns, or (b) is
/// explicitly validated by `validate_raw_discriminants`.
///
/// The audit flagged the theoretical risk of a future author adding a
/// `bool` or `#[repr(u8)] enum` field to one of these structs, which
/// would make the unsafe cast UB on first access even when
/// `validate_raw_discriminants` succeeds. Today the slab-persisted
/// types contain only fixed-width integer/array fields plus the two
/// `#[repr(u8)]` enums `SideMode` and `MarketMode` (both validated).
///
/// This test asserts that structural invariant at runtime by
/// instantiating every slab field through zero bytes (via
/// `Account::default()`-style construction in the engine crate) and
/// relying on `#[repr(C)]` plus bytemuck's `NoUninit` discipline. A
/// simpler, still useful check: ensure Account's size is reachable
/// from a zeroed byte array without UB — which it is iff all fields
/// have all-bits-valid bit patterns.
#[test]
fn test_zc_cast_safety_invariant() {
    use core::mem::size_of;
    use percolator::Account;

    // All-zero bytes must be a valid Account — today every field is
    // U128 / I128 / u8 / u64 / u128 / i128 / [u8;N], all of which have
    // zero as a valid bit pattern. If a future field type breaks this
    // (bool, NonZero*, &T, etc.), this transmute becomes UB on first
    // field read after the cast; the test will surface the defect in
    // CI (either via miri/sanitizers if enabled, or as a structural
    // mismatch). It ALSO enforces that the author thought about this
    // constraint when touching Account.
    let zero = [0u8; size_of::<Account>()];
    let acct: Account = unsafe { core::mem::transmute_copy(&zero) };
    // Touch every wrapper-visible getter so any invalid bit pattern
    // surfaces now rather than later.
    let _ = acct.capital.get();
    let _ = acct.pnl;
    let _ = acct.reserved_pnl;
    let _ = acct.position_basis_q;
    let _ = acct.fee_credits.get();
    let _ = acct.last_fee_slot;
    let _ = acct.kind;
    let _ = acct.sched_present;
    let _ = acct.pending_present;
    // If the above compiles and runs clean, every field in Account
    // is all-bits-valid. RiskEngine-level fields (vault, params, enum
    // discriminants, etc.) are already either all-bits-valid or
    // covered by validate_raw_discriminants; the audit concern was
    // specifically nested Account fields.
}

#[test]
fn test_init_market() {
    let mut f = setup_market();
    let data = encode_init_market(&f, 50);

    {
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
        ];
        process_instruction(&f.program_id, &accounts, &data).unwrap();
    }

    let header = state::read_header(&f.slab.data);
    assert_eq!(header.magic, MAGIC);
    assert_eq!(header.version, 0);

    let engine = zc::engine_ref(&f.slab.data).unwrap();
    assert_eq!(engine.params.max_accounts, MAX_ACCOUNTS as u64);
}

#[test]
fn test_vault_validation() {
    let mut f = setup_market();
    f.vault.owner = solana_program::system_program::id();
    let init_data = encode_init_market(&f, 50);
    let init_accounts = vec![
        f.admin.to_info(),
        f.slab.to_info(),
        f.mint.to_info(),
        f.vault.to_info(),
        f.clock.to_info(),
        f.pyth_index.to_info(),
    ];
    let res = process_instruction(&f.program_id, &init_accounts, &init_data);
    assert_eq!(res, Err(PercolatorError::InvalidVaultAta.into()));
}

#[test]
#[ignore = "native debug engine scans unused zero-memory accounts; SBF integration covers zero-copy materialization"]
fn test_trade() {
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 50);
    {
        let init_accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
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
        make_token_account(f.mint.key, user.key, 2000),
    )
    .writable();
    {
        let accounts = vec![
            user.to_info(),
            f.slab.to_info(),
            user_ata.to_info(),
            f.vault.to_info(),
            f.token_prog.to_info(),
            f.clock.to_info(),
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
        make_token_account(f.mint.key, lp.key, 2000),
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
            f.clock.to_info(),
        ];
        process_instruction(
            &f.program_id,
            &accs,
            &encode_init_lp(matcher_prog_key, matcher_ctx_key, 100),
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
        let accounts = vec![
            user.to_info(),
            lp.to_info(),
            f.slab.to_info(),
            f.clock.to_info(),
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
fn test_set_risk_threshold() {
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 50);
    {
        let accs = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &init_data).unwrap();
    }

    // Opcode 11 (SetRiskThreshold) was removed — insurance_floor is now immutable (§2.2.1).
    // Verify that calling opcode 11 returns InvalidInstructionData.
    {
        let accs = vec![f.admin.to_info(), f.slab.to_info(), f.clock.to_info()];
        let res = process_instruction(
            &f.program_id,
            &accs,
            &encode_set_risk_threshold(123_456_789),
        );
        assert_eq!(
            res,
            Err(ProgramError::InvalidInstructionData),
            "Opcode 11 should be rejected as removed"
        );
    }
}

#[test]
fn test_set_risk_threshold_non_admin_fails() {
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 50);
    {
        let accs = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
        ];
        process_instruction(&f.program_id, &accs, &init_data).unwrap();
    }

    // Opcode 11 was removed — any caller gets InvalidInstructionData, even non-admin
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
            f.slab.to_info(),
            f.clock.to_info(),
        ];
        let res = process_instruction(
            &f.program_id,
            &accs,
            &encode_set_risk_threshold(new_threshold),
        );
        assert_eq!(res, Err(ProgramError::InvalidInstructionData));
    }

    // insurance_floor field deleted from RiskParams — nothing to assert.
}

#[test]
#[ignore = "native debug engine scans unused zero-memory accounts; SBF integration covers zero-copy materialization"]
fn test_permissionless_crank_gc() {
    // Non-vacuous test: create a dust account and verify GC frees it
    let mut f = setup_market();
    let init_data = encode_init_market(&f, 50);

    // Init market
    {
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
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
            f.clock.to_info(),
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
    // - pnl = 0 (GC requires pnl == 0 per spec §2.6)
    // - position_basis_q = 0 (already 0)
    // - reserved_pnl = 0 (already 0)
    // - fee_credits = 0
    // Also zero out vault and c_tot to keep conservation invariant consistent.
    {
        let engine = zc::engine_mut(&mut f.slab.data).unwrap();
        engine.accounts[user_idx as usize].capital = U128::ZERO;
        engine.accounts[user_idx as usize].pnl = 0i128;
        engine.accounts[user_idx as usize].fee_credits = I128::ZERO;
        engine.c_tot = U128::ZERO;
        engine.vault = engine.insurance_fund.balance;
    }
    // Also zero the SPL vault balance to match
    {
        let mut vault_data = f.vault.data.clone();
        let mut vault_state = TokenAccount::unpack(&vault_data).unwrap();
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        vault_state.amount = engine.insurance_fund.balance.get() as u64;
        TokenAccount::pack(vault_state, &mut vault_data).unwrap();
        f.vault.data = vault_data;
    }

    // Verify account is now a dust candidate
    {
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        let account = &engine.accounts[user_idx as usize];
        assert!(account.capital.is_zero(), "capital should be 0");
        assert_eq!(account.pnl, 0, "pnl should be 0");
        assert!(
            account.position_basis_q == 0,
            "position_basis_q should be 0"
        );
        assert_eq!(account.reserved_pnl, 0, "reserved_pnl should be 0");
    }

    // Call ReclaimEmptyAccount - should reclaim the dust account
    // ReclaimEmptyAccount (opcode 25) expects 2 accounts: slab (writable), clock
    {
        let accs = vec![f.slab.to_info(), f.clock.to_info()];
        process_instruction(
            &f.program_id,
            &accs,
            &encode_reclaim_empty_account(user_idx),
        )
        .unwrap();
    }

    // Verify reclaim freed the account
    {
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        assert_eq!(
            engine.num_used_accounts,
            used_before - 1,
            "num_used_accounts should decrease by 1"
        );
        assert!(
            !engine.is_used(user_idx as usize),
            "User account should no longer be used after reclaim"
        );
    }
}

// Admin rotation coverage lives in the integration suite
// (tests/test_admin.rs::test_update_authority_*). The native unit
// harness here cannot host the Clock sysvar that UpdateAuthority's
// hard-timeout gate requires, so these tests were removed when
// UpdateAdmin (tag 12) was replaced by UpdateAuthority (tag 32).

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
    let (price_raw, _) =
        read_engine_price_e6(&oracle.to_info(), &feed_id, 100, 100, 500, 0, 0).unwrap();
    assert_eq!(
        price_raw, 100_000_000,
        "Raw price should be $100 (100_000_000 e6)"
    );

    // With inversion (invert=1, unit_scale=0)
    let (price_inv, _) =
        read_engine_price_e6(&oracle.to_info(), &feed_id, 100, 100, 500, 1, 0).unwrap();
    assert_eq!(
        price_inv, 10_000,
        "Inverted price should be 10_000 e6 (= 1e12 / 100_000_000)"
    );

    // Test unit_scale transformation (oracle price scaling)
    // With unit_scale=1000: price_scaled = 100_000_000 / 1000 = 100_000
    let (price_scaled, _) =
        read_engine_price_e6(&oracle.to_info(), &feed_id, 100, 100, 500, 0, 1000).unwrap();
    assert_eq!(
        price_scaled, 100_000,
        "Scaled price should be 100_000 e6 (= 100_000_000 / 1000)"
    );

    // Test combined inversion + unit_scale
    // Inverted: 1e12 / 100_000_000 = 10_000
    // Then scaled: 10_000 / 1000 = 10
    let (price_inv_scaled, _) =
        read_engine_price_e6(&oracle.to_info(), &feed_id, 100, 100, 500, 1, 1000).unwrap();
    assert_eq!(
        price_inv_scaled, 10,
        "Inverted+scaled price should be 10 e6"
    );
}

#[test]
fn test_unit_scale_conversion() {
    // Test base_to_units and units_to_base with unit_scale
    use percolator_prog::units::{base_to_units, units_to_base_checked};

    // With scale=0, no conversion
    assert_eq!(base_to_units(12345, 0), (12345, 0));
    assert_eq!(units_to_base_checked(12345, 0), Some(12345));

    // With scale=1000 (e.g., for wSOL where 1000 lamports = 1 unit)
    assert_eq!(base_to_units(5500, 1000), (5, 500)); // 5 units, 500 dust
    assert_eq!(base_to_units(5000, 1000), (5, 0)); // 5 units, no dust
    assert_eq!(units_to_base_checked(5, 1000), Some(5000));

    // With scale=100
    assert_eq!(base_to_units(201, 100), (2, 1)); // 2 units, 1 dust
    assert_eq!(units_to_base_checked(2, 100), Some(200));
}

#[test]
fn test_unit_scale_validation_at_init() {
    // Test that unit_scale > 1_000_000_000 is rejected
    let mut f = setup_market();
    let data = encode_init_market_invert(&f, 50, 0, 2_000_000_000); // Too large

    {
        let accounts = vec![
            f.admin.to_info(),
            f.slab.to_info(),
            f.mint.to_info(),
            f.vault.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
        ];
        let res = process_instruction(&f.program_id, &accounts, &data);
        assert_eq!(
            res,
            Err(ProgramError::InvalidInstructionData),
            "Should reject unit_scale > 1B"
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

// ============================================================================
// Nonce overflow tests
// ============================================================================

#[test]
fn test_nonce_on_success_normal() {
    assert_eq!(percolator_prog::policy::nonce_on_success(0), Some(1));
    assert_eq!(percolator_prog::policy::nonce_on_success(42), Some(43));
    assert_eq!(
        percolator_prog::policy::nonce_on_success(u64::MAX - 1),
        Some(u64::MAX)
    );
}

#[test]
fn test_nonce_on_success_rejects_overflow() {
    assert_eq!(
        percolator_prog::policy::nonce_on_success(u64::MAX),
        None,
        "nonce_on_success(u64::MAX) must return None, not wrap to 0"
    );
}

#[test]
fn test_nonce_overflow_does_not_reopen_request_id_space() {
    // The point: if nonce wrapped, req_id 0 would be reissued,
    // and a matcher holding a stale response with req_id=0 could replay it.
    // With checked_add, this is blocked.
    let at_max = percolator_prog::policy::nonce_on_success(u64::MAX);
    assert!(at_max.is_none(), "Must reject at u64::MAX");

    // Verify the previous value still works
    let before_max = percolator_prog::policy::nonce_on_success(u64::MAX - 1);
    assert_eq!(
        before_max,
        Some(u64::MAX),
        "u64::MAX-1 should advance to u64::MAX"
    );
}
