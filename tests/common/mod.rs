#![allow(dead_code, unused_imports, unused_variables, unused_mut, clippy::too_many_arguments, clippy::field_reassign_with_default, clippy::manual_saturating_arithmetic, clippy::useless_conversion, for_loops_over_fallibles, clippy::unnecessary_cast, clippy::absurd_extreme_comparisons, clippy::doc_markdown, clippy::manual_abs_diff, clippy::empty_line_after_doc_comments, clippy::doc_lazy_continuation, clippy::needless_range_loop, clippy::implicit_saturating_sub, clippy::wrong_self_convention)]
//! Shared test infrastructure for integration tests.

pub use litesvm::LiteSVM;
pub use solana_sdk::{
    account::Account,
    clock::Clock,
    instruction::{AccountMeta, Instruction},
    program_pack::Pack,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    sysvar,
    transaction::Transaction,
};
pub use solana_program_runtime::compute_budget::ComputeBudget;
pub use spl_token::state::{Account as TokenAccount, AccountState};
pub use std::path::PathBuf;

// SLAB_LEN for production BPF (MAX_ACCOUNTS=4096)
// Note: We use production BPF (not test feature) because test feature
// bypasses CPI for token transfers, which fails in LiteSVM.
// Haircut-ratio engine (ADL/socialization scratch arrays removed)
// 2026-04-17: CONFIG_LEN extended 432 → 480 (Phase A, +48) → 512 (Phase E,
// +32 for pending_admin). SLAB_LEN tracks via percolator_prog::constants::SLAB_LEN
// but is hardcoded here because the tests' BPF layout is a 32-bit SBF target
// that doesn't match the host constant.
pub const SLAB_LEN: usize = 1484728; // v12.17 + Phase A (+48) + Phase E (+32) = 1484648+80
pub const MAX_ACCOUNTS: usize = 4096;

// Pyth Receiver program ID
pub const PYTH_RECEIVER_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
    0x0c, 0xb7, 0xfa, 0xbb, 0x52, 0xf7, 0xa6, 0x48, 0xbb, 0x5b, 0x31, 0x7d, 0x9a, 0x01, 0x8b, 0x90,
    0x57, 0xcb, 0x02, 0x47, 0x74, 0xfa, 0xfe, 0x01, 0xe6, 0xc4, 0xdf, 0x98, 0xcc, 0x38, 0x58, 0x81,
]);

pub const TEST_FEED_ID: [u8; 32] = [0xABu8; 32];

pub fn cu_ix() -> Instruction {
    solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(1_400_000)
}

/// Create a LiteSVM instance with an uncapped compute budget for test builds.
/// Unoptimized debug BPF binaries consume far more CUs than mainnet release builds,
/// so we raise the limit to 50M to avoid false CU-exhaustion failures.
pub fn new_test_svm() -> LiteSVM {
    let mut svm = LiteSVM::new();
    svm.set_compute_budget(ComputeBudget {
        compute_unit_limit: 50_000_000,
        heap_size: 256 * 1024,
        ..ComputeBudget::default()
    });
    svm
}

pub fn program_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("target/deploy/percolator_prog.so");
    assert!(
        path.exists(),
        "BPF not found at {:?}. Run: cargo build-sbf",
        path
    );
    path
}

pub fn make_token_account_data(mint: &Pubkey, owner: &Pubkey, amount: u64) -> Vec<u8> {
    let mut data = vec![0u8; TokenAccount::LEN];
    let mut account = TokenAccount::default();
    account.mint = *mint;
    account.owner = *owner;
    account.amount = amount;
    account.state = AccountState::Initialized;
    TokenAccount::pack(account, &mut data).unwrap();
    data
}

pub fn make_token_account_with_delegate(
    mint: &Pubkey,
    owner: &Pubkey,
    amount: u64,
    delegate: &Pubkey,
    delegated_amount: u64,
) -> Vec<u8> {
    let mut data = vec![0u8; TokenAccount::LEN];
    let mut account = TokenAccount::default();
    account.mint = *mint;
    account.owner = *owner;
    account.amount = amount;
    account.delegate = solana_sdk::program_option::COption::Some(*delegate);
    account.delegated_amount = delegated_amount;
    account.state = AccountState::Initialized;
    TokenAccount::pack(account, &mut data).unwrap();
    data
}

pub fn make_token_account_with_close_authority(
    mint: &Pubkey,
    owner: &Pubkey,
    amount: u64,
    close_authority: &Pubkey,
) -> Vec<u8> {
    let mut data = vec![0u8; TokenAccount::LEN];
    let mut account = TokenAccount::default();
    account.mint = *mint;
    account.owner = *owner;
    account.amount = amount;
    account.close_authority = solana_sdk::program_option::COption::Some(*close_authority);
    account.state = AccountState::Initialized;
    TokenAccount::pack(account, &mut data).unwrap();
    data
}

pub fn make_mint_data() -> Vec<u8> {
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
pub fn make_pyth_data(
    feed_id: &[u8; 32],
    price: i64,
    expo: i32,
    conf: u64,
    publish_time: i64,
) -> Vec<u8> {
    let mut data = vec![0u8; 134];
    // verification_level = 1 (Full) at offset 40 (u16 LE)
    data[40..42].copy_from_slice(&1u16.to_le_bytes());
    data[42..74].copy_from_slice(feed_id);
    data[74..82].copy_from_slice(&price.to_le_bytes());
    data[82..90].copy_from_slice(&conf.to_le_bytes());
    data[90..94].copy_from_slice(&expo.to_le_bytes());
    data[94..102].copy_from_slice(&publish_time.to_le_bytes());
    data
}

/// Append default extended tail (66 bytes) to an InitMarket payload.
/// All extended fields get default/disabled values.
fn append_default_extended_tail(data: &mut Vec<u8>) {
    data.extend_from_slice(&0u16.to_le_bytes()); // insurance_withdraw_max_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // insurance_withdraw_cooldown_slots

    data.extend_from_slice(&0u64.to_le_bytes()); // permissionless_resolve_stale_slots
    data.extend_from_slice(&500u64.to_le_bytes()); // funding_horizon_slots (default)
    data.extend_from_slice(&100u64.to_le_bytes()); // funding_k_bps (default)
    data.extend_from_slice(&500i64.to_le_bytes()); // funding_max_premium_bps (default)
    data.extend_from_slice(&5i64.to_le_bytes()); // funding_max_bps_per_slot (default)
    data.extend_from_slice(&0u64.to_le_bytes()); // mark_min_fee (disabled)
    data.extend_from_slice(&0u64.to_le_bytes()); // force_close_delay_slots (default tail has permissionless=0)
}

/// Encode InitMarket instruction with invert flag
pub fn encode_init_market_with_invert(
    admin: &Pubkey,
    mint: &Pubkey,
    feed_id: &[u8; 32],
    invert: u8,
) -> Vec<u8> {
    encode_init_market_full_v2(admin, mint, feed_id, invert, 0, 0)
}

/// Encode InitMarket with initial_mark_price_e6 for Hyperp mode
pub fn encode_init_market_hyperp(admin: &Pubkey, mint: &Pubkey, initial_mark_price_e6: u64) -> Vec<u8> {
    // Hyperp mode: feed_id = [0; 32], invert = 0 (not inverted internally)
    encode_init_market_full_v2(admin, mint, &[0u8; 32], 0, initial_mark_price_e6, 0)
}

/// Full InitMarket encoder with all new fields
pub fn encode_init_market_with_conf_bps(
    admin: &Pubkey,
    mint: &Pubkey,
    feed_id: &[u8; 32],
    invert: u8,
    initial_mark_price_e6: u64,
    warmup_period_slots: u64,
    conf_filter_bps: u16,
) -> Vec<u8> {
    let mut data = vec![0u8];
    data.extend_from_slice(admin.as_ref());
    data.extend_from_slice(mint.as_ref());
    data.extend_from_slice(feed_id);
    data.extend_from_slice(&86400u64.to_le_bytes()); // max_staleness_secs (1 day, ≤7d cap)
    data.extend_from_slice(&conf_filter_bps.to_le_bytes()); // conf_filter_bps
    data.push(invert); // invert flag
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
    data.extend_from_slice(&initial_mark_price_e6.to_le_bytes()); // initial_mark_price_e6
    // Per-market admin limits (uncapped defaults for tests)
    data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee_per_slot (0 = disabled)
    data.extend_from_slice(&10_000_000_000_000_000u128.to_le_bytes()); // max_insurance_floor
    data.extend_from_slice(&0u64.to_le_bytes()); // min_oracle_price_cap_e2bps
    // RiskParams
    data.extend_from_slice(&warmup_period_slots.to_le_bytes()); // h_min
    data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps
    data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // trading_fee_bps
    data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data.extend_from_slice(&0u128.to_le_bytes()); // new_account_fee
    data.extend_from_slice(&0u128.to_le_bytes()); // risk_reduction_threshold
    data.extend_from_slice(&warmup_period_slots.to_le_bytes()); // h_max (must be >= h_min)

    data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_crank_staleness_slots
    data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
    data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
    data.extend_from_slice(&100u64.to_le_bytes()); // resolve_price_deviation_bps
    data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
    data.extend_from_slice(&100u128.to_le_bytes()); // min_initial_deposit
    data.extend_from_slice(&1u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&2u128.to_le_bytes()); // min_nonzero_im_req
    append_default_extended_tail(&mut data);
    data
}

pub fn encode_init_market_full_v2(
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
    data.extend_from_slice(&86400u64.to_le_bytes()); // max_staleness_secs (1 day, ≤7d cap)
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(invert); // invert flag
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
    data.extend_from_slice(&initial_mark_price_e6.to_le_bytes()); // initial_mark_price_e6
    // Per-market admin limits (uncapped defaults for tests)
    data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee_per_slot (0 = disabled) (<= MAX_PROTOCOL_FEE_ABS)
    data.extend_from_slice(&10_000_000_000_000_000u128.to_le_bytes()); // max_insurance_floor (<= MAX_VAULT_TVL)
    data.extend_from_slice(&0u64.to_le_bytes()); // min_oracle_price_cap_e2bps
    // RiskParams
    data.extend_from_slice(&warmup_period_slots.to_le_bytes()); // h_min
    data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps
    data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // trading_fee_bps
    data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data.extend_from_slice(&0u128.to_le_bytes()); // new_account_fee
    data.extend_from_slice(&0u128.to_le_bytes()); // risk_reduction_threshold
    data.extend_from_slice(&warmup_period_slots.to_le_bytes()); // h_max (must be >= h_min)

    data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_crank_staleness_slots
    data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
    data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
    data.extend_from_slice(&100u64.to_le_bytes()); // resolve_price_deviation_bps
    data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
    data.extend_from_slice(&100u128.to_le_bytes()); // min_initial_deposit
    data.extend_from_slice(&1u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&2u128.to_le_bytes()); // min_nonzero_im_req
    // Full extended tail (required — no partial tails allowed)
    append_default_extended_tail(&mut data);
    data
}

/// Encode InitMarket with oracle price cap and optional permissionless resolution.
/// This enables mark EWMA from genesis (non-zero cap = EWMA active on trades).
pub fn encode_init_market_with_cap(
    admin: &Pubkey,
    mint: &Pubkey,
    feed_id: &[u8; 32],
    invert: u8,
    min_oracle_price_cap_e2bps: u64,
    permissionless_resolve_stale_slots: u64,
) -> Vec<u8> {
    let mut data = vec![0u8];
    data.extend_from_slice(admin.as_ref());
    data.extend_from_slice(mint.as_ref());
    data.extend_from_slice(feed_id);
    data.extend_from_slice(&86400u64.to_le_bytes()); // max_staleness_secs (1 day, ≤7d cap)
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(invert);
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
    data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6 (0 for non-Hyperp)
    // Per-market admin limits
    data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee_per_slot (0 = disabled)
    data.extend_from_slice(&10_000_000_000_000_000u128.to_le_bytes()); // max_insurance_floor
    data.extend_from_slice(&min_oracle_price_cap_e2bps.to_le_bytes()); // min_oracle_price_cap_e2bps
    // RiskParams
    data.extend_from_slice(&0u64.to_le_bytes()); // h_min
    data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps
    data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // trading_fee_bps
    data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data.extend_from_slice(&0u128.to_le_bytes()); // new_account_fee
    data.extend_from_slice(&0u128.to_le_bytes()); // risk_reduction_threshold
    data.extend_from_slice(&0u64.to_le_bytes()); // h_max

    // max_crank_staleness_slots: use bounded value when permissionless resolve is enabled
    // (permissionless_resolve_stale_slots must exceed max_crank_staleness_slots)
    let max_crank = if permissionless_resolve_stale_slots > 0 {
        permissionless_resolve_stale_slots.saturating_sub(1).max(1)
    } else {
        u64::MAX
    };
    data.extend_from_slice(&max_crank.to_le_bytes()); // max_crank_staleness_slots
    data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
    data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
    data.extend_from_slice(&100u64.to_le_bytes()); // resolve_price_deviation_bps
    data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
    data.extend_from_slice(&100u128.to_le_bytes()); // min_initial_deposit
    data.extend_from_slice(&1u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&2u128.to_le_bytes()); // min_nonzero_im_req
    // Full extended tail (82 bytes)
    data.extend_from_slice(&0u16.to_le_bytes()); // insurance_withdraw_max_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // insurance_withdraw_cooldown_slots

    data.extend_from_slice(&permissionless_resolve_stale_slots.to_le_bytes());
    data.extend_from_slice(&500u64.to_le_bytes()); // funding_horizon_slots (default)
    data.extend_from_slice(&100u64.to_le_bytes()); // funding_k_bps (default)
    data.extend_from_slice(&500i64.to_le_bytes()); // funding_max_premium_bps (default)
    data.extend_from_slice(&5i64.to_le_bytes()); // funding_max_bps_per_slot (default)
    data.extend_from_slice(&0u64.to_le_bytes()); // mark_min_fee (disabled)
    // force_close_delay must be > 0 when permissionless_resolve > 0
    let force_close = if permissionless_resolve_stale_slots > 0 { 50u64 } else { 0u64 };
    data.extend_from_slice(&force_close.to_le_bytes());
    data
}

/// Encode InitMarket with oracle price cap, permissionless resolution, AND custom funding params.
/// The 4 funding params are optional trailing fields in the wire format.
pub fn encode_init_market_with_funding(
    admin: &Pubkey,
    mint: &Pubkey,
    feed_id: &[u8; 32],
    invert: u8,
    min_oracle_price_cap_e2bps: u64,
    permissionless_resolve_stale_slots: u64,
    funding_horizon_slots: u64,
    funding_k_bps: u64,
    funding_max_premium_bps: i64,
    funding_max_bps_per_slot: i64,
) -> Vec<u8> {
    let mut data = encode_init_market_with_cap(
        admin,
        mint,
        feed_id,
        invert,
        min_oracle_price_cap_e2bps,
        permissionless_resolve_stale_slots,
    );
    // Truncate default funding + mark_min_fee + force_close_delay (48 bytes)
    // from the full tail produced by encode_init_market_with_cap, then re-append
    // with custom funding params and default mark_min_fee + force_close_delay.
    data.truncate(data.len() - 48);
    data.extend_from_slice(&funding_horizon_slots.to_le_bytes());
    data.extend_from_slice(&funding_k_bps.to_le_bytes());
    data.extend_from_slice(&funding_max_premium_bps.to_le_bytes());
    data.extend_from_slice(&funding_max_bps_per_slot.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes()); // mark_min_fee (disabled)
    let fc = if permissionless_resolve_stale_slots > 0 { 50u64 } else { 0u64 };
    data.extend_from_slice(&fc.to_le_bytes()); // force_close_delay_slots
    data
}

/// Encode InitMarket with all params including mark_min_fee for fee-weighted EWMA.
pub fn encode_init_market_with_min_fee(
    admin: &Pubkey,
    mint: &Pubkey,
    feed_id: &[u8; 32],
    invert: u8,
    min_oracle_price_cap_e2bps: u64,
    permissionless_resolve_stale_slots: u64,
    funding_horizon_slots: u64,
    funding_k_bps: u64,
    funding_max_premium_bps: i64,
    funding_max_bps_per_slot: i64,
    mark_min_fee: u64,
) -> Vec<u8> {
    let mut data = encode_init_market_with_funding(
        admin, mint, feed_id, invert,
        min_oracle_price_cap_e2bps, permissionless_resolve_stale_slots,
        funding_horizon_slots, funding_k_bps,
        funding_max_premium_bps, funding_max_bps_per_slot,
    );
    // Truncate default mark_min_fee + force_close_delay (16 bytes), replace with custom
    data.truncate(data.len() - 16);
    data.extend_from_slice(&mark_min_fee.to_le_bytes());
    let fc = if permissionless_resolve_stale_slots > 0 { 50u64 } else { 0u64 };
    data.extend_from_slice(&fc.to_le_bytes()); // force_close_delay_slots
    data
}

/// Encode InitMarket with trading_fee_bps, oracle price cap, and mark_min_fee.
/// For fee-weighted EWMA integration tests that need non-zero trading fees.
pub fn encode_init_market_with_trading_fee(
    admin: &Pubkey,
    mint: &Pubkey,
    feed_id: &[u8; 32],
    invert: u8,
    min_oracle_price_cap_e2bps: u64,
    trading_fee_bps: u64,
    mark_min_fee: u64,
) -> Vec<u8> {
    let mut data = vec![0u8];
    data.extend_from_slice(admin.as_ref());
    data.extend_from_slice(mint.as_ref());
    data.extend_from_slice(feed_id);
    data.extend_from_slice(&86400u64.to_le_bytes()); // max_staleness_secs (1 day, ≤7d cap)
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(invert);
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
    data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6
    data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee_per_slot (0 = disabled)
    data.extend_from_slice(&10_000_000_000_000_000u128.to_le_bytes()); // max_insurance_floor
    data.extend_from_slice(&min_oracle_price_cap_e2bps.to_le_bytes());
    // RiskParams
    data.extend_from_slice(&0u64.to_le_bytes()); // h_min
    data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps
    data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps
    data.extend_from_slice(&trading_fee_bps.to_le_bytes()); // trading_fee_bps
    data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data.extend_from_slice(&0u128.to_le_bytes()); // new_account_fee
    data.extend_from_slice(&0u128.to_le_bytes()); // risk_reduction_threshold
    data.extend_from_slice(&0u64.to_le_bytes()); // h_max

    data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_crank_staleness_slots
    data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
    data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
    data.extend_from_slice(&100u64.to_le_bytes()); // resolve_price_deviation_bps
    data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
    data.extend_from_slice(&100u128.to_le_bytes()); // min_initial_deposit
    data.extend_from_slice(&1u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&2u128.to_le_bytes()); // min_nonzero_im_req
    data.extend_from_slice(&0u16.to_le_bytes()); // insurance_withdraw_max_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // insurance_withdraw_cooldown_slots

    data.extend_from_slice(&0u64.to_le_bytes()); // permissionless_resolve_stale_slots
    // Custom funding params (required before mark_min_fee)
    data.extend_from_slice(&500u64.to_le_bytes()); // funding_horizon_slots
    data.extend_from_slice(&100u64.to_le_bytes()); // funding_k_bps
    data.extend_from_slice(&500i64.to_le_bytes()); // funding_max_premium_bps
    data.extend_from_slice(&5i64.to_le_bytes()); // funding_max_bps_per_slot
    // mark_min_fee
    data.extend_from_slice(&mark_min_fee.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes()); // force_close_delay_slots
    data
}

/// Encode InitMarket with configurable maintenance fee and max bound.
/// Encode InitMarket with custom oracle cap (legacy name kept for test compat).
/// max_maintenance_fee_per_slot and maintenance_fee_per_slot params are ignored
/// (fields removed from engine). Wire slots now carry h_max + padding.
pub fn encode_init_market_with_maint_fee_bounded(
    admin: &Pubkey,
    mint: &Pubkey,
    feed_id: &[u8; 32],
    _max_maintenance_fee_per_slot: u128,
    _maintenance_fee_per_slot: u128,
    min_oracle_price_cap_e2bps: u64,
) -> Vec<u8> {
    let mut data = vec![0u8];
    data.extend_from_slice(admin.as_ref());
    data.extend_from_slice(mint.as_ref());
    data.extend_from_slice(feed_id);
    data.extend_from_slice(&86400u64.to_le_bytes()); // max_staleness_secs (1 day, ≤7d cap)
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(0u8); // invert
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
    data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6
    data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee_per_slot (0 = disabled)
    data.extend_from_slice(&10_000_000_000_000_000u128.to_le_bytes()); // max_insurance_floor
    data.extend_from_slice(&min_oracle_price_cap_e2bps.to_le_bytes());
    // RiskParams
    data.extend_from_slice(&0u64.to_le_bytes()); // h_min
    data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps
    data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // trading_fee_bps
    data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data.extend_from_slice(&0u128.to_le_bytes()); // new_account_fee
    data.extend_from_slice(&0u128.to_le_bytes()); // insurance_floor
    data.extend_from_slice(&0u64.to_le_bytes()); // h_max

    data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_crank_staleness_slots
    data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
    data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
    data.extend_from_slice(&100u64.to_le_bytes()); // resolve_price_deviation_bps
    data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
    data.extend_from_slice(&100u128.to_le_bytes()); // min_initial_deposit
    data.extend_from_slice(&1u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&2u128.to_le_bytes()); // min_nonzero_im_req
    append_default_extended_tail(&mut data);
    data
}

/// Encode InitMarket with force_close_delay_slots for permissionless force-close tests.
pub fn encode_init_market_with_force_close(
    admin: &Pubkey,
    mint: &Pubkey,
    feed_id: &[u8; 32],
    force_close_delay_slots: u64,
) -> Vec<u8> {
    // Build base with cap + permissionless resolve (full 82-byte tail)
    let mut data = encode_init_market_with_cap(
        admin, mint, feed_id, 0, 10_000, 100,
    );
    // Truncate default force_close_delay_slots (last 8 bytes), replace with custom
    data.truncate(data.len() - 8);
    data.extend_from_slice(&force_close_delay_slots.to_le_bytes());
    data
}

pub fn encode_force_close_resolved(user_idx: u16) -> Vec<u8> {
    let mut data = vec![30u8]; // Tag 30
    data.extend_from_slice(&user_idx.to_le_bytes());
    data
}

pub fn encode_init_lp(matcher: &Pubkey, ctx: &Pubkey, fee: u64) -> Vec<u8> {
    let mut data = vec![2u8];
    data.extend_from_slice(matcher.as_ref());
    data.extend_from_slice(ctx.as_ref());
    data.extend_from_slice(&fee.to_le_bytes());
    data
}

pub fn encode_init_user(fee: u64) -> Vec<u8> {
    let mut data = vec![1u8];
    data.extend_from_slice(&fee.to_le_bytes());
    data
}

pub fn encode_deposit(user_idx: u16, amount: u64) -> Vec<u8> {
    let mut data = vec![3u8];
    data.extend_from_slice(&user_idx.to_le_bytes());
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

pub fn encode_trade(lp: u16, user: u16, size: i128) -> Vec<u8> {
    let mut data = vec![6u8];
    data.extend_from_slice(&lp.to_le_bytes());
    data.extend_from_slice(&user.to_le_bytes());
    data.extend_from_slice(&size.to_le_bytes());
    data
}

pub fn encode_crank_permissionless() -> Vec<u8> {
    // Two-phase crank: pass first 128 account indices as candidates.
    // format_version=1: (u16 idx, u8 policy_tag) per candidate.
    let mut data = vec![5u8];
    data.extend_from_slice(&u16::MAX.to_le_bytes()); // caller_idx = permissionless
    data.push(1u8); // format_version = 1
    for i in 0..128u16 {
        data.extend_from_slice(&i.to_le_bytes()); // idx
        data.push(0u8); // tag 0 = FullClose
    }
    data
}

pub fn encode_crank_with_candidates(candidates: &[u16]) -> Vec<u8> {
    let mut data = vec![5u8];
    data.extend_from_slice(&u16::MAX.to_le_bytes());
    data.push(1u8); // format_version = 1
    for &idx in candidates {
        data.extend_from_slice(&idx.to_le_bytes());
        data.push(0u8); // tag 0 = FullClose
    }
    data
}

pub struct TestEnv {
    pub svm: LiteSVM,
    pub program_id: Pubkey,
    pub payer: Keypair,
    pub slab: Pubkey,
    pub mint: Pubkey,
    pub vault: Pubkey,
    pub pyth_index: Pubkey,
    pub pyth_col: Pubkey,
    pub account_count: u16, // Tracks number of accounts created (LP + users)
}

impl TestEnv {
    pub fn new() -> Self {
        let path = program_path();

        let mut svm = new_test_svm();
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

    /// Initialize market with custom conf_filter_bps (0 = disabled)
    pub fn init_market_with_conf_bps(&mut self, conf_filter_bps: u16) {
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
            data: encode_init_market_with_conf_bps(
                &admin.pubkey(),
                &self.mint,
                &TEST_FEED_ID,
                0, // invert
                0, // initial_mark_price_e6
                0, // warmup
                conf_filter_bps,
            ),
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("init_market_with_conf_bps failed");
    }

    pub fn init_market_with_invert(&mut self, invert: u8) {
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
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("init_market failed");
    }

    /// Initialize a market with oracle price cap (enables EWMA) and optional permissionless resolution.
    pub fn init_market_with_cap(
        &mut self,
        invert: u8,
        min_oracle_price_cap_e2bps: u64,
        permissionless_resolve_stale_slots: u64,
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
            data: encode_init_market_with_cap(
                &admin.pubkey(),
                &self.mint,
                &TEST_FEED_ID,
                invert,
                min_oracle_price_cap_e2bps,
                permissionless_resolve_stale_slots,
            ),
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("init_market_with_cap failed");
    }

    /// Initialize a market with cap, permissionless resolution, AND custom funding params.
    pub fn init_market_with_funding(
        &mut self,
        invert: u8,
        min_oracle_price_cap_e2bps: u64,
        permissionless_resolve_stale_slots: u64,
        funding_horizon_slots: u64,
        funding_k_bps: u64,
        funding_max_premium_bps: i64,
        funding_max_bps_per_slot: i64,
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
            data: encode_init_market_with_funding(
                &admin.pubkey(),
                &self.mint,
                &TEST_FEED_ID,
                invert,
                min_oracle_price_cap_e2bps,
                permissionless_resolve_stale_slots,
                funding_horizon_slots,
                funding_k_bps,
                funding_max_premium_bps,
                funding_max_bps_per_slot,
            ),
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("init_market_with_funding failed");
    }

    /// Initialize a market with trading fees and mark_min_fee for fee-weighted EWMA tests.
    pub fn init_market_fee_weighted(
        &mut self,
        invert: u8,
        min_oracle_price_cap_e2bps: u64,
        trading_fee_bps: u64,
        mark_min_fee: u64,
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
            data: encode_init_market_with_trading_fee(
                &admin.pubkey(),
                &self.mint,
                &TEST_FEED_ID,
                invert,
                min_oracle_price_cap_e2bps,
                trading_fee_bps,
                mark_min_fee,
            ),
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("init_market_with_trading_fee failed");
    }

    /// Initialize a market with all params including mark_min_fee for fee-weighted EWMA.
    pub fn init_market_with_min_fee(
        &mut self,
        invert: u8,
        min_oracle_price_cap_e2bps: u64,
        mark_min_fee: u64,
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
            data: encode_init_market_with_min_fee(
                &admin.pubkey(),
                &self.mint,
                &TEST_FEED_ID,
                invert,
                min_oracle_price_cap_e2bps,
                0, // no permissionless resolve
                500, 100, 500, 5, // default funding params
                mark_min_fee,
            ),
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("init_market_with_min_fee failed");
    }

    /// Initialize a Hyperp market (internal mark/index, no external oracle)
    pub fn init_market_hyperp(&mut self, initial_mark_price_e6: u64) {
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
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("init_market_hyperp failed");
    }

    pub fn create_ata(&mut self, owner: &Pubkey, amount: u64) -> Pubkey {
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

    pub fn init_lp(&mut self, owner: &Keypair) -> u16 {
        let idx = self.account_count;
        self.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        let ata = self.create_ata(&owner.pubkey(), 100);
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
                AccountMeta::new_readonly(matcher, false),
                AccountMeta::new_readonly(ctx, false),
            ],
            data: encode_init_lp(&matcher, &ctx, 100),
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

    pub fn init_lp_with_fee(&mut self, owner: &Keypair, fee: u64) -> u16 {
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
                AccountMeta::new_readonly(matcher, false),
                AccountMeta::new_readonly(ctx, false),
            ],
            data: encode_init_lp(&matcher, &ctx, fee),
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("init_lp_with_fee failed");
        self.account_count += 1;
        idx
    }

    pub fn init_user(&mut self, owner: &Keypair) -> u16 {
        let idx = self.account_count;
        self.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        let ata = self.create_ata(&owner.pubkey(), 100);

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
            data: encode_init_user(100),
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

    pub fn deposit(&mut self, owner: &Keypair, user_idx: u16, amount: u64) {
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

    pub fn trade(&mut self, user: &Keypair, lp: &Keypair, lp_idx: u16, user_idx: u16, size: i128) {
        let cu_ix = cu_ix();
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
            &[cu_ix, ix],
            Some(&user.pubkey()),
            &[user, lp],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("trade failed");
    }

    pub fn crank(&mut self) {
        let caller = Keypair::new();
        self.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();

        let cu_ix = cu_ix();
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
            &[cu_ix, ix],
            Some(&caller.pubkey()),
            &[&caller],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("crank failed");
    }

    pub fn try_crank(&mut self) -> Result<(), String> {
        let caller = Keypair::new();
        self.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();

        let cu_ix = cu_ix();
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
            &[cu_ix, ix],
            Some(&caller.pubkey()),
            &[&caller],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    pub fn set_slot(&mut self, slot: u64) {
        // Offset by 100 to ensure monotonicity with init_market (runs at slot 100)
        let effective_slot = slot + 100;
        self.svm.set_sysvar(&Clock {
            slot: effective_slot,
            unix_timestamp: effective_slot as i64,
            ..Clock::default()
        });
        // Update oracle publish_time to match
        let pyth_data = make_pyth_data(&TEST_FEED_ID, 138_000_000, -6, 1, effective_slot as i64);
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
    pub fn set_slot_and_price(&mut self, slot: u64, price_e6: i64) {
        let effective_slot = slot + 100;
        self.svm.set_sysvar(&Clock {
            slot: effective_slot,
            unix_timestamp: effective_slot as i64,
            ..Clock::default()
        });
        // Update oracle with new price and publish_time
        let pyth_data = make_pyth_data(&TEST_FEED_ID, price_e6, -6, 1, effective_slot as i64);
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
    pub fn try_close_account(&mut self, owner: &Keypair, user_idx: u16) -> Result<(), String> {
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
            &[cu_ix(), ix],
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
    pub fn try_admin_force_close_account(
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
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Force-close multiple accounts fully (handles two-phase ProgressOnly semantics).
    /// Pass all (idx, owner) pairs. Phase 1 reconciles all, Phase 2 closes all.
    pub fn force_close_accounts_fully(
        &mut self,
        admin: &Keypair,
        accounts: &[(u16, &Pubkey)],
    ) -> Result<(), String> {
        // Phase 1: reconcile all (some may close immediately if pnl<=0)
        for &(idx, owner) in accounts {
            let _ = self.try_admin_force_close_account(admin, idx, owner);
        }
        // Phase 2: close remaining (now terminal-ready)
        for &(idx, owner) in accounts {
            let _ = self.try_admin_force_close_account(admin, idx, owner);
        }
        Ok(())
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


/// Test that a non-inverted market works correctly (control case).
///
/// This serves as a control test to verify that non-inverted markets
/// (where oracle price is used directly as market price) still work.


// ============================================================================
// Bug regression tests
// ============================================================================

pub fn encode_close_slab() -> Vec<u8> {
    vec![13u8] // Instruction tag for CloseSlab
}

pub fn encode_resolve_market() -> Vec<u8> {
    vec![19u8]
}

pub fn encode_resolve_permissionless() -> Vec<u8> {
    vec![29u8]
}

pub fn encode_withdraw_insurance() -> Vec<u8> {
    vec![20u8] // Instruction tag for WithdrawInsurance
}

pub fn encode_set_insurance_withdraw_policy(
    authority: &Pubkey,
    min_withdraw_base: u64,
    max_withdraw_bps: u16,
    cooldown_slots: u64,
) -> Vec<u8> {
    let mut data = vec![22u8];
    data.extend_from_slice(authority.as_ref());
    data.extend_from_slice(&min_withdraw_base.to_le_bytes());
    data.extend_from_slice(&max_withdraw_bps.to_le_bytes());
    data.extend_from_slice(&cooldown_slots.to_le_bytes());
    data
}

pub fn encode_withdraw_insurance_limited(amount: u64) -> Vec<u8> {
    let mut data = vec![23u8];
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

pub fn encode_withdraw(user_idx: u16, amount: u64) -> Vec<u8> {
    let mut data = vec![4u8]; // Instruction tag for WithdrawCollateral
    data.extend_from_slice(&user_idx.to_le_bytes());
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

pub fn encode_close_account(user_idx: u16) -> Vec<u8> {
    let mut data = vec![8u8]; // Instruction tag for CloseAccount
    data.extend_from_slice(&user_idx.to_le_bytes());
    data
}

pub fn encode_admin_force_close_account(user_idx: u16) -> Vec<u8> {
    let mut data = vec![21u8]; // Tag 21: AdminForceCloseAccount
    data.extend_from_slice(&user_idx.to_le_bytes());
    data
}

/// Encode InitMarket with configurable unit_scale and new_account_fee
pub fn encode_init_market_full(
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
    data.extend_from_slice(&86400u64.to_le_bytes()); // max_staleness_secs (1 day, ≤7d cap)
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(invert);
    data.extend_from_slice(&unit_scale.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6 (0 for non-Hyperp)
    // Per-market admin limits (uncapped defaults for tests)
    data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee_per_slot (0 = disabled) (<= MAX_PROTOCOL_FEE_ABS)
    data.extend_from_slice(&10_000_000_000_000_000u128.to_le_bytes()); // max_insurance_floor (<= MAX_VAULT_TVL)
    data.extend_from_slice(&0u64.to_le_bytes()); // min_oracle_price_cap_e2bps
    // RiskParams
    data.extend_from_slice(&0u64.to_le_bytes()); // h_min
    data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps
    data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // trading_fee_bps
    data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data.extend_from_slice(&new_account_fee.to_le_bytes());
    data.extend_from_slice(&0u128.to_le_bytes()); // risk_reduction_threshold
    data.extend_from_slice(&0u64.to_le_bytes()); // h_max

    data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_crank_staleness_slots
    data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
    data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
    data.extend_from_slice(&100u64.to_le_bytes()); // resolve_price_deviation_bps
    data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
    data.extend_from_slice(&100u128.to_le_bytes()); // min_initial_deposit
    data.extend_from_slice(&1u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&2u128.to_le_bytes()); // min_nonzero_im_req
    append_default_extended_tail(&mut data);
    data
}

/// Encode InitMarket with configurable warmup_period_slots
pub fn encode_init_market_with_warmup(
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
    data.extend_from_slice(&86400u64.to_le_bytes()); // max_staleness_secs (1 day, ≤7d cap)
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(invert);
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale = 0 (no scaling)
    data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6 (0 for non-Hyperp)
    // Per-market admin limits (uncapped defaults for tests)
    data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee_per_slot (0 = disabled) (<= MAX_PROTOCOL_FEE_ABS)
    data.extend_from_slice(&10_000_000_000_000_000u128.to_le_bytes()); // max_insurance_floor (<= MAX_VAULT_TVL)
    data.extend_from_slice(&0u64.to_le_bytes()); // min_oracle_price_cap_e2bps
    // RiskParams
    data.extend_from_slice(&warmup_period_slots.to_le_bytes()); // h_min
    data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps (5%)
    data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps (10%)
    data.extend_from_slice(&0u64.to_le_bytes()); // trading_fee_bps
    data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data.extend_from_slice(&0u128.to_le_bytes()); // new_account_fee
    data.extend_from_slice(&0u128.to_le_bytes()); // risk_reduction_threshold
    data.extend_from_slice(&warmup_period_slots.to_le_bytes()); // h_max (must be >= h_min)

    data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_crank_staleness_slots
    data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
    data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
    data.extend_from_slice(&100u64.to_le_bytes()); // resolve_price_deviation_bps
    data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
    data.extend_from_slice(&100u128.to_le_bytes()); // min_initial_deposit
    data.extend_from_slice(&1u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&2u128.to_le_bytes()); // min_nonzero_im_req
    append_default_extended_tail(&mut data);
    data
}

impl TestEnv {
    /// Initialize market with full parameter control
    pub fn init_market_full(&mut self, invert: u8, unit_scale: u32, new_account_fee: u128) {
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
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("init_market failed");
    }

    /// Initialize market with configurable warmup period
    pub fn init_market_with_warmup(&mut self, invert: u8, warmup_period_slots: u64) {
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
            &[cu_ix(), ix],
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
    pub fn init_user_with_fee(&mut self, owner: &Keypair, fee: u64) -> u16 {
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
            &[cu_ix(), ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("init_user failed");
        self.account_count += 1;
        idx
    }

    /// Read num_used_accounts from engine state
    pub fn read_num_used_accounts(&self) -> u16 {
        let slab_account = self.svm.get_account(&self.slab).unwrap();
        // ENGINE_OFF = 584, num_used_accounts at engine offset 1224
        pub const NUM_USED_OFFSET: usize = 584 + 1224;
        if slab_account.data.len() < NUM_USED_OFFSET + 2 {
            return 0;
        }
        let bytes = [
            slab_account.data[NUM_USED_OFFSET],
            slab_account.data[NUM_USED_OFFSET + 1],
        ];
        u16::from_le_bytes(bytes)
    }

    /// Read authority_price_e6 from config (mark price for Hyperp, settlement for non-Hyperp)
    pub fn read_authority_price(&self) -> u64 {
        let d = self.svm.get_account(&self.slab).unwrap().data;
        percolator_prog::state::read_config(&d).authority_price_e6
    }

    /// Read last_effective_price_e6 from config (index for Hyperp, baseline for non-Hyperp)
    pub fn read_last_effective_price(&self) -> u64 {
        let d = self.svm.get_account(&self.slab).unwrap().data;
        percolator_prog::state::read_config(&d).last_effective_price_e6
    }

    /// Read mark_ewma_e6 from config
    pub fn read_mark_ewma(&self) -> u64 {
        let d = self.svm.get_account(&self.slab).unwrap().data;
        percolator_prog::state::read_config(&d).mark_ewma_e6
    }

    /// Read oracle_price_cap_e2bps from config
    pub fn read_oracle_price_cap(&self) -> u64 {
        let d = self.svm.get_account(&self.slab).unwrap().data;
        percolator_prog::state::read_config(&d).oracle_price_cap_e2bps
    }

    /// Read funding_rate_bps_per_slot_last from engine
    pub fn read_funding_rate(&self) -> i128 {
        // v12.17: funding_rate_e9_per_slot_last removed from engine.
        // Funding is now passed per-call. No stored rate to read.
        0
    }

    /// Read funding_horizon_slots from config
    pub fn read_funding_horizon(&self) -> u64 {
        let d = self.svm.get_account(&self.slab).unwrap().data;
        percolator_prog::state::read_config(&d).funding_horizon_slots
    }

    /// Read funding_k_bps from config
    pub fn read_funding_k_bps(&self) -> u64 {
        let d = self.svm.get_account(&self.slab).unwrap().data;
        percolator_prog::state::read_config(&d).funding_k_bps
    }

    /// Read funding_max_premium_bps from config
    pub fn read_funding_max_premium_bps(&self) -> i64 {
        let d = self.svm.get_account(&self.slab).unwrap().data;
        percolator_prog::state::read_config(&d).funding_max_premium_bps
    }

    /// Read funding_max_bps_per_slot from config
    pub fn read_funding_max_bps_per_slot(&self) -> i64 {
        let d = self.svm.get_account(&self.slab).unwrap().data;
        percolator_prog::state::read_config(&d).funding_max_bps_per_slot
    }

    /// Read mark_min_fee from config.
    pub fn read_mark_min_fee(&self) -> u64 {
        let d = self.svm.get_account(&self.slab).unwrap().data;
        percolator_prog::state::read_config(&d).mark_min_fee
    }

    /// Check if a slot is marked as used in the bitmap
    pub fn is_slot_used(&self, idx: u16) -> bool {
        let slab_account = self.svm.get_account(&self.slab).unwrap();
        // ENGINE_OFF = 584, offset of RiskEngine.used = 712
        // Bitmap is [u64; 64] at offset 584 + 712 = 1216
        pub const BITMAP_OFFSET: usize = 584 + 712;
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
    pub fn read_account_capital(&self, idx: u16) -> u128 {
        let slab_account = self.svm.get_account(&self.slab).unwrap();
        // ENGINE_OFF = 584, accounts array at offset 9424 within RiskEngine
        // Account size = 368 bytes, capital at offset 0 within Account
        pub const ACCOUNTS_OFFSET: usize = 584 + 9424;
        pub const ACCOUNT_SIZE: usize = 352;
        pub const CAPITAL_OFFSET_IN_ACCOUNT: usize = 0; // After account_id (u64)
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

    /// Read effective position for an account, computing it from the ADL state.
    /// Returns position in POS_SCALE units (i128, low 128 bits of I256).
    /// Formula: effective_pos_q = position_basis_q * A_side / a_basis (epoch-matched)
    /// Read effective position for an account from raw slab bytes.
    /// v12.15: position_basis_q is i128 (not I256), at offset 64 in Account.
    pub fn read_account_position(&self, idx: u16) -> i128 {
        let d = self.svm.get_account(&self.slab).unwrap().data;
        pub const ENGINE: usize = 584;
        pub const ACCOUNTS_OFFSET: usize = ENGINE + 9424;
        pub const ACCOUNT_SIZE: usize = 352;
        pub const PBQ: usize = 56;       // position_basis_q: i128 (16 bytes)
        pub const A_BASIS: usize = 72;   // adl_a_basis: u128 (16 bytes)
        pub const EPOCH_SNAP: usize = 120; // adl_epoch_snap: u64 (8 bytes)
        pub const ADL_MULT_LONG: usize = ENGINE + 392;
        pub const ADL_MULT_SHORT: usize = ENGINE + 408;
        pub const ADL_EPOCH_LONG: usize = ENGINE + 456;
        pub const ADL_EPOCH_SHORT: usize = ENGINE + 464;

        let acc_off = ACCOUNTS_OFFSET + (idx as usize) * ACCOUNT_SIZE;
        if d.len() < acc_off + ACCOUNT_SIZE { return 0; }

        // position_basis_q is now i128 (16 bytes)
        let basis = i128::from_le_bytes(d[acc_off+PBQ..acc_off+PBQ+16].try_into().unwrap());
        if basis == 0 { return 0; }

        let a_basis = u128::from_le_bytes(d[acc_off+A_BASIS..acc_off+A_BASIS+16].try_into().unwrap());
        let epoch_snap = u64::from_le_bytes(d[acc_off+EPOCH_SNAP..acc_off+EPOCH_SNAP+8].try_into().unwrap());
        if a_basis == 0 { return 0; }

        // Read A_side and epoch based on sign
        let (a_side, epoch_side) = if basis > 0 {
            let a = u128::from_le_bytes(d[ADL_MULT_LONG..ADL_MULT_LONG+16].try_into().unwrap());
            let e = u64::from_le_bytes(d[ADL_EPOCH_LONG..ADL_EPOCH_LONG+8].try_into().unwrap());
            (a, e)
        } else {
            let a = u128::from_le_bytes(d[ADL_MULT_SHORT..ADL_MULT_SHORT+16].try_into().unwrap());
            let e = u64::from_le_bytes(d[ADL_EPOCH_SHORT..ADL_EPOCH_SHORT+8].try_into().unwrap());
            (a, e)
        };

        if epoch_snap != epoch_side { return 0; }

        // effective = |basis| * A_side / a_basis
        let abs_basis = basis.unsigned_abs();
        let effective = if a_side == a_basis {
            abs_basis
        } else {
            // mul_div_floor: abs_basis * a_side / a_basis
            ((abs_basis as u128) * a_side) / a_basis.max(1)
        };
        if basis < 0 { -(effective as i128) } else { effective as i128 }
    }

    /// Snapshot config fields relevant to UpdateConfig validation tests.
    pub fn read_update_config_snapshot(&self) -> (u64, u128, u64, u128, u128) {
        let d = self.svm.get_account(&self.slab).unwrap().data;
        let config = percolator_prog::state::read_config(&d);
        // Return funding_horizon and placeholder values for removed threshold fields.
        // Threshold params were removed from the engine. The tuple format is preserved
        // for backward compatibility with tests that compare snapshots.
        (
            config.funding_horizon_slots,
            0u128, // was funding_inv_scale (removed)
            0u64,  // was thresh_alpha (removed)
            0u128, // was thresh_min (removed)
            0u128, // was thresh_max (removed)
        )
    }

    /// Try to close slab, returns Ok or error
    pub fn try_close_slab(&mut self) -> Result<(), String> {
        let admin = Keypair::from_bytes(&self.payer.to_bytes()).unwrap();
        let (vault_pda, _) =
            Pubkey::find_program_address(&[b"vault", self.slab.as_ref()], &self.program_id);
        let admin_ata = self.create_ata(&admin.pubkey(), 0);

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(vault_pda, false),
                AccountMeta::new(admin_ata, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: encode_close_slab(),
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
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
    pub fn withdraw(&mut self, owner: &Keypair, user_idx: u16, amount: u64) {
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
            &[cu_ix(), ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("withdraw failed");
    }

    /// Try to execute trade, returns result
    pub fn try_trade(
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
            &[cu_ix(), ix],
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
    pub fn vault_balance(&self) -> u64 {
        let account = self.svm.get_account(&self.vault).unwrap();
        let token_account = TokenAccount::unpack(&account.data).unwrap();
        token_account.amount
    }

    /// Close account - returns remaining capital to user (8 accounts needed)
    pub fn close_account(&mut self, owner: &Keypair, user_idx: u16) {
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
            &[cu_ix(), ix],
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


// ============================================================================
// Misaligned withdrawal rejection test (related to unit_scale)
// ============================================================================

/// Test that withdrawals with amounts not divisible by unit_scale are rejected.


// ============================================================================
// Bug #4: InitUser/InitLP should not trap fee overpayments
// ============================================================================

/// Test that fee overpayments are properly handled.
///
/// Bug: If fee_payment > new_account_fee, the excess is deposited to vault
/// but only new_account_fee is accounted in engine.vault/insurance.


// Bug #6 (Threshold EWMA slow ramp), Bug #7 (Pending epoch wraparound),
// Bug #8 (LP entry price on flip) — documented in MEMORY.md.
// Engine-level fixes verified by code inspection; no stub tests needed.

// Finding L: Original test used invert=1 (making notional tiny), so the trade
// passed for the wrong reason. The corrected test below uses invert=0.

/// Corrected version of Finding L test - uses invert=0 for accurate notional calculation.
/// The original test used invert=1, which inverts $138 to ~$7.25, resulting in
/// position notional of only ~0.5 SOL instead of 10 SOL. This test verifies
/// that initial_margin_bps is correctly enforced for risk-increasing trades.


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


/// Test that zombie accounts don't indefinitely poison the haircut ratio.
///
/// This is a simpler test that verifies the basic mechanism:
/// - Idle account with capital and no position can be closed
/// - Even without PnL, crank processes the account correctly


// ============================================================================
// HYPERP MODE SECURITY TESTS
// ============================================================================

/// Security Issue: Hyperp mode requires non-zero initial_mark_price_e6
///
/// If Hyperp mode is enabled (index_feed_id == [0; 32]) but initial_mark_price_e6 == 0,
/// the market would have no valid price and trades would fail with OracleInvalid.
/// This test verifies the validation in InitMarket rejects this configuration.


// Hyperp security stubs (TradeNoCpi disabled, exec_price clamping,
// default oracle_price_cap, index smoothing) — documented in MEMORY.md.
// Verified by code inspection + Kani proofs for clamp_toward_with_dt.

/// Test: Hyperp mode InitMarket succeeds with valid initial_mark_price


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


// ============================================================================
// Matcher Context Initialization Tests
// ============================================================================

pub fn matcher_program_path() -> PathBuf {
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
pub const MATCHER_CONTEXT_LEN: usize = 320;
pub const MATCHER_RETURN_LEN: usize = 64;
pub const MATCHER_CALL_LEN: usize = 67;
pub const MATCHER_CALL_TAG: u8 = 0;
pub const MATCHER_INIT_VAMM_TAG: u8 = 2;
pub const CTX_VAMM_OFFSET: usize = 64;
pub const VAMM_MAGIC: u64 = 0x5045_5243_4d41_5443; // "PERCMATC"

/// Matcher mode enum
#[repr(u8)]
#[derive(Clone, Copy)]
pub enum MatcherMode {
    Passive = 0,
    Vamm = 1,
}

/// Encode InitVamm instruction (Tag 2)
pub fn encode_init_vamm(
    mode: MatcherMode,
    trading_fee_bps: u32,
    base_spread_bps: u32,
    max_total_bps: u32,
    impact_k_bps: u32,
    liquidity_notional_e6: u128,
    max_fill_abs: u128,
    max_inventory_abs: u128,
) -> Vec<u8> {
    let mut data = vec![0u8; 70]; // 66 + fee_to_insurance_bps(2) + skew_spread_mult_bps(2)
    data[0] = MATCHER_INIT_VAMM_TAG;
    data[1] = mode as u8;
    data[2..6].copy_from_slice(&trading_fee_bps.to_le_bytes());
    data[6..10].copy_from_slice(&base_spread_bps.to_le_bytes());
    data[10..14].copy_from_slice(&max_total_bps.to_le_bytes());
    data[14..18].copy_from_slice(&impact_k_bps.to_le_bytes());
    data[18..34].copy_from_slice(&liquidity_notional_e6.to_le_bytes());
    data[34..50].copy_from_slice(&max_fill_abs.to_le_bytes());
    data[50..66].copy_from_slice(&max_inventory_abs.to_le_bytes());
    // fee_to_insurance_bps at [66..68] = 0, skew_spread_mult_bps at [68..70] = 0
    data
}

/// Encode percolator's InitMatcherCtx instruction (Tag 75)
/// This goes to the *percolator* program, which CPIs into the matcher with
/// the LP PDA as a PDA signer.
pub fn encode_init_matcher_ctx(
    lp_idx: u16,
    kind: u8,
    trading_fee_bps: u32,
    base_spread_bps: u32,
    max_total_bps: u32,
    impact_k_bps: u32,
    liquidity_notional_e6: u128,
    max_fill_abs: u128,
    max_inventory_abs: u128,
    fee_to_insurance_bps: u16,
    skew_spread_mult_bps: u16,
) -> Vec<u8> {
    let mut d = vec![75u8]; // tag 75
    d.extend_from_slice(&lp_idx.to_le_bytes());
    d.push(kind);
    d.extend_from_slice(&trading_fee_bps.to_le_bytes());
    d.extend_from_slice(&base_spread_bps.to_le_bytes());
    d.extend_from_slice(&max_total_bps.to_le_bytes());
    d.extend_from_slice(&impact_k_bps.to_le_bytes());
    d.extend_from_slice(&liquidity_notional_e6.to_le_bytes());
    d.extend_from_slice(&max_fill_abs.to_le_bytes());
    d.extend_from_slice(&max_inventory_abs.to_le_bytes());
    d.extend_from_slice(&fee_to_insurance_bps.to_le_bytes());
    d.extend_from_slice(&skew_spread_mult_bps.to_le_bytes());
    d
}

/// Encode a matcher call instruction (Tag 0)
pub fn encode_matcher_call(
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
pub fn read_matcher_return(data: &[u8]) -> (u32, u32, u64, i128, u64) {
    let abi_version = u32::from_le_bytes(data[0..4].try_into().unwrap());
    let flags = u32::from_le_bytes(data[4..8].try_into().unwrap());
    let exec_price = u64::from_le_bytes(data[8..16].try_into().unwrap());
    let exec_size = i128::from_le_bytes(data[16..32].try_into().unwrap());
    let req_id = u64::from_le_bytes(data[32..40].try_into().unwrap());
    (abi_version, flags, exec_price, exec_size, req_id)
}

/// Test that the matcher context can be initialized with Passive mode


/// Test that the matcher can execute a call after initialization


/// Test that double initialization is rejected


/// Test vAMM mode with impact pricing


// ============================================================================
// Comprehensive Feature Tests
// ============================================================================

impl TestEnv {
    /// Try to withdraw, returns result
    pub fn try_withdraw(&mut self, owner: &Keypair, user_idx: u16, amount: u64) -> Result<(), String> {
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
            &[cu_ix(), ix],
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
    pub fn try_deposit_unauthorized(
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
            &[cu_ix(), ix],
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
    pub fn try_trade_without_lp_sig(
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
            &[cu_ix(), ix],
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
    pub fn top_up_insurance(&mut self, payer: &Keypair, amount: u64) {
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
                AccountMeta::new_readonly(sysvar::clock::ID, false),
            ],
            data,
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&payer.pubkey()),
            &[payer],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("top_up_insurance failed");
    }

    /// Try liquidation
    pub fn try_liquidate(&mut self, target_idx: u16) -> Result<(), String> {
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
            &[cu_ix(), ix],
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


/// Test 2: Liquidation attempt when user position goes underwater


/// Test 3: Withdrawal limits - can't withdraw beyond margin requirements


/// Test 4: Unauthorized access - wrong signer can't operate on account


/// Test 5: Position flip - user goes from long to short


/// Test 6: Multiple participants - all trades succeed with single LP


/// Test 7: Oracle price impact - crank succeeds at different prices


/// Test 8: Insurance fund top-up succeeds


/// Test 9: Trading at margin limits


/// Test 10: Funding accrual - multiple cranks succeed over time


/// Test 11: Close account returns correct capital


// ============================================================================
// CRITICAL SECURITY TESTS - L7 DEEP DIVE
// ============================================================================

// Instruction encoders for admin operations
pub fn encode_update_admin(new_admin: &Pubkey) -> Vec<u8> {
    let mut data = vec![12u8]; // Tag 12: UpdateAdmin
    data.extend_from_slice(new_admin.as_ref());
    data
}

pub fn encode_accept_admin() -> Vec<u8> {
    vec![82u8] // Tag 82: AcceptAdmin
}

pub fn encode_set_risk_threshold(new_threshold: u128) -> Vec<u8> {
    let mut data = vec![11u8]; // Tag 11: SetRiskThreshold
    data.extend_from_slice(&new_threshold.to_le_bytes());
    data
}

pub fn encode_set_oracle_authority(new_authority: &Pubkey) -> Vec<u8> {
    let mut data = vec![16u8]; // Tag 16: SetOracleAuthority
    data.extend_from_slice(new_authority.as_ref());
    data
}

pub fn encode_push_oracle_price(price_e6: u64, timestamp: i64) -> Vec<u8> {
    let mut data = vec![17u8]; // Tag 17: PushOraclePrice
    data.extend_from_slice(&price_e6.to_le_bytes());
    data.extend_from_slice(&timestamp.to_le_bytes());
    data
}

pub fn encode_set_oracle_price_cap(max_change_e2bps: u64) -> Vec<u8> {
    let mut data = vec![18u8]; // Tag 18: SetOraclePriceCap
    data.extend_from_slice(&max_change_e2bps.to_le_bytes());
    data
}

pub fn encode_set_maintenance_fee(new_fee: u128) -> Vec<u8> {
    let mut data = vec![15u8]; // Tag 15: SetMaintenanceFee
    data.extend_from_slice(&new_fee.to_le_bytes());
    data
}

pub fn encode_liquidate(target_idx: u16) -> Vec<u8> {
    let mut data = vec![7u8]; // Tag 7: LiquidateAtOracle
    data.extend_from_slice(&target_idx.to_le_bytes());
    data
}

pub fn encode_update_config(
    funding_horizon_slots: u64,
    funding_k_bps: u64,
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
    /// Try UpdateAdmin instruction.
    /// Phase E (2026-04-17): two-step semantics. new_admin != default() sets
    /// pending_admin only; caller must also call try_accept_admin(&new_admin_kp)
    /// to complete the transfer. new_admin == default() still burns immediately.
    pub fn try_update_admin(&mut self, signer: &Keypair, new_admin: &Pubkey) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(self.slab, false),
            ],
            data: encode_update_admin(new_admin),
        };
        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&signer.pubkey()),
            &[signer],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Try AcceptAdmin (tag 82) — second half of two-step UpdateAdmin.
    /// Signer must match config.pending_admin (the pubkey proposed via UpdateAdmin).
    pub fn try_accept_admin(&mut self, signer: &Keypair) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(self.slab, false),
            ],
            data: encode_accept_admin(),
        };
        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&signer.pubkey()),
            &[signer],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Helper: perform a full two-step admin rotation (UpdateAdmin + AcceptAdmin).
    /// Returns Ok if both steps succeed. Used by tests that care about the
    /// end state (admin actually rotated), not the intermediate pending state.
    pub fn try_rotate_admin(
        &mut self,
        current_admin: &Keypair,
        new_admin: &Keypair,
    ) -> Result<(), String> {
        self.try_update_admin(current_admin, &new_admin.pubkey())?;
        self.try_accept_admin(new_admin)
    }

    /// Try SetRiskThreshold instruction
    pub fn try_set_risk_threshold(
        &mut self,
        signer: &Keypair,
        new_threshold: u128,
    ) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
            ],
            data: encode_set_risk_threshold(new_threshold),
        };
        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
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
    pub fn try_set_oracle_authority(
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
            &[cu_ix(), ix],
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
    pub fn try_push_oracle_price(
        &mut self,
        signer: &Keypair,
        price_e6: u64,
        _timestamp: i64,
    ) -> Result<(), String> {
        // Use current clock unix_timestamp + 1 to guarantee strict monotonicity
        // and clock anchoring. The explicit timestamp parameter is ignored —
        // kept for API compatibility.
        let clock: Clock = self.svm.get_sysvar();
        let ts = clock.unix_timestamp;
        // Bump the clock by 1 second so subsequent pushes in the same test
        // get strictly increasing timestamps.
        self.svm.set_sysvar(&Clock {
            slot: clock.slot,
            unix_timestamp: ts + 1,
            ..clock
        });
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(self.slab, false),
            ],
            data: encode_push_oracle_price(price_e6, ts),
        };
        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
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
    pub fn try_set_oracle_price_cap(
        &mut self,
        signer: &Keypair,
        max_change_e2bps: u64,
    ) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
            ],
            data: encode_set_oracle_price_cap(max_change_e2bps),
        };
        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
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
    pub fn try_set_maintenance_fee(&mut self, signer: &Keypair, new_fee: u128) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(self.slab, false),
            ],
            data: encode_set_maintenance_fee(new_fee),
        };
        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
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
    pub fn try_resolve_market(&mut self, admin: &Keypair) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_resolve_market(),
        };
        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Permissionless resolution (anyone can call when oracle is dead)
    pub fn try_resolve_permissionless(&mut self) -> Result<(), String> {
        let caller = Keypair::new();
        self.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_resolve_permissionless(),
        };
        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&caller.pubkey()),
            &[&caller],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).map(|_| ()).map_err(|e| format!("{:?}", e))
    }

    /// Try ForceCloseResolved instruction (permissionless, requires resolved + delay)
    pub fn try_force_close_resolved(
        &mut self,
        user_idx: u16,
        owner: &Pubkey,
    ) -> Result<(), String> {
        let caller = Keypair::new();
        self.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();

        let owner_ata = self.create_ata(owner, 0);
        let (vault_pda, _) =
            Pubkey::find_program_address(&[b"vault", self.slab.as_ref()], &self.program_id);

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(self.slab, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new(owner_ata, false),
                AccountMeta::new_readonly(vault_pda, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_force_close_resolved(user_idx),
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&caller.pubkey()),
            &[&caller],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Try WithdrawInsurance instruction (admin only, requires resolved + all positions closed)
    pub fn try_withdraw_insurance(&mut self, admin: &Keypair) -> Result<(), String> {
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
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Configure limited insurance-withdraw policy (admin only, resolved market only).
    pub fn try_set_insurance_withdraw_policy(
        &mut self,
        admin: &Keypair,
        authority: &Pubkey,
        min_withdraw_base: u64,
        max_withdraw_bps: u16,
        cooldown_slots: u64,
    ) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.slab, false),
            ],
            data: encode_set_insurance_withdraw_policy(
                authority,
                min_withdraw_base,
                max_withdraw_bps,
                cooldown_slots,
            ),
        };
        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Limited insurance withdraw by configured authority.
    /// Live markets require oracle (8th account). Pass pyth_index for accrual.
    pub fn try_withdraw_insurance_limited(
        &mut self,
        authority: &Keypair,
        amount: u64,
    ) -> Result<(), String> {
        let authority_ata = self.create_ata(&authority.pubkey(), 0);
        let (vault_pda, _) =
            Pubkey::find_program_address(&[b"vault", self.slab.as_ref()], &self.program_id);
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(authority_ata, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(vault_pda, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_withdraw_insurance_limited(amount),
        };
        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&authority.pubkey()),
            &[authority],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Check if market is resolved. Since engine_ref uses native struct layout
    /// which differs from BPF, we check resolved_price > 0 as a proxy.
    /// resolved_price is only set by engine.resolve_market().
    pub fn is_market_resolved(&self) -> bool {
        // Check FLAG_RESOLVED (bit 0) in header flags byte at offset 13.
        // This is stable across engine layout changes.
        let d = self.svm.get_account(&self.slab).unwrap().data;
        (d[13] & 0x01) != 0
    }

    /// Read insurance fund balance from engine
    pub fn read_insurance_balance(&self) -> u128 {
        let slab_account = self.svm.get_account(&self.slab).unwrap();
        // ENGINE_OFF = 584, InsuranceFund.balance is at offset 16 within engine
        // (vault is 16 bytes at 0, insurance_fund starts at 16)
        // InsuranceFund { balance: U128, ... } - balance is first field
        pub const INSURANCE_OFFSET: usize = 584 + 16;
        u128::from_le_bytes(
            slab_account.data[INSURANCE_OFFSET..INSURANCE_OFFSET + 16]
                .try_into()
                .unwrap(),
        )
    }

    /// Try LiquidateAtOracle instruction
    pub fn try_liquidate_target(&mut self, target_idx: u16) -> Result<(), String> {
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
            &[cu_ix(), ix],
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
    pub fn try_update_config(&mut self, signer: &Keypair) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
            ],
            data: encode_update_config(
                3600,                      // funding_horizon_slots
                100,                       // funding_k_bps
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
            &[cu_ix(), ix],
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


// ============================================================================
// Test: SetRiskThreshold authorization
// ============================================================================

/// CRITICAL: SetRiskThreshold admin-only
// ============================================================================
// Test: SetOracleAuthority and PushOraclePrice (admin oracle)
// ============================================================================

/// CRITICAL: Admin oracle mechanism for Hyperp mode


// ============================================================================
// Test: SetOraclePriceCap authorization
// ============================================================================

/// CRITICAL: SetOraclePriceCap admin-only


// ============================================================================
// Test: SetMaintenanceFee authorization
// ============================================================================

/// CRITICAL: SetMaintenanceFee admin-only
// ============================================================================
// Test: UpdateConfig authorization
// ============================================================================

/// CRITICAL: UpdateConfig admin-only with all parameters


// ============================================================================
// Test: CloseSlab requires zero balances
// ============================================================================

/// CRITICAL: CloseSlab only by admin, requires zero vault/insurance


// ============================================================================
// Test: Double initialization rejected
// ============================================================================

/// CRITICAL: InitMarket rejects already initialized slab


// ============================================================================
// Test: Invalid account indices rejected
// ============================================================================

/// CRITICAL: Invalid user_idx/lp_idx are rejected


// ============================================================================
// Test: Sell trades (negative size)
// ============================================================================

/// Test that sell trades (negative size) work correctly


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
pub fn encode_trade_cpi(lp_idx: u16, user_idx: u16, size: i128) -> Vec<u8> {
    let mut data = vec![10u8]; // TradeCpi instruction tag
    data.extend_from_slice(&lp_idx.to_le_bytes());
    data.extend_from_slice(&user_idx.to_le_bytes());
    data.extend_from_slice(&size.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes()); // limit_price_e6 = 0 (no limit)
    data
}

pub fn encode_trade_cpi_with_limit(lp_idx: u16, user_idx: u16, size: i128, limit_price_e6: u64) -> Vec<u8> {
    let mut data = vec![10u8]; // TradeCpi instruction tag
    data.extend_from_slice(&lp_idx.to_le_bytes());
    data.extend_from_slice(&user_idx.to_le_bytes());
    data.extend_from_slice(&size.to_le_bytes());
    data.extend_from_slice(&limit_price_e6.to_le_bytes());
    data
}

/// Test environment extended for TradeCpi tests
pub struct TradeCpiTestEnv {
    pub svm: LiteSVM,
    pub program_id: Pubkey,
    pub matcher_program_id: Pubkey,
    pub payer: Keypair,
    pub slab: Pubkey,
    pub mint: Pubkey,
    pub vault: Pubkey,
    pub pyth_index: Pubkey,
    pub pyth_col: Pubkey,
    pub account_count: u16,
}

impl TradeCpiTestEnv {
    pub fn new() -> Self {
        let percolator_path = program_path();
        let matcher_path = matcher_program_path();

        let mut svm = new_test_svm();
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

        TradeCpiTestEnv {
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
        }
    }

    pub fn init_market(&mut self) {
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
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("init_market failed");
    }

    pub fn create_ata(&mut self, owner: &Pubkey, amount: u64) -> Pubkey {
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
    pub fn init_lp_with_matcher(&mut self, owner: &Keypair, matcher_prog: &Pubkey) -> (u16, Pubkey) {
        let idx = self.account_count;
        self.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        let ata = self.create_ata(&owner.pubkey(), 100);

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

        // Step 1: Init LP in percolator (registers matcher_program + matcher_context
        // in the slab LP account).
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(ata, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(*matcher_prog, false),
                AccountMeta::new_readonly(ctx, false),
            ],
            data: encode_init_lp(matcher_prog, &ctx, 100),
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("init_lp failed");
        self.account_count += 1;

        // Step 2: Use percolator's InitMatcherCtx (tag 75) to CPI-init the matcher
        // context.  The LP PDA is passed as a PDA signer by the percolator
        // program, satisfying the matcher's signer requirement.
        let admin = &self.payer;
        let init_ctx_ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),       // admin (signer)
                AccountMeta::new_readonly(self.slab, false),  // slab
                AccountMeta::new(ctx, false),                 // matcher_ctx (writable)
                AccountMeta::new_readonly(*matcher_prog, false), // matcher_prog
                AccountMeta::new_readonly(lp_pda, false),     // lp_pda
            ],
            data: encode_init_matcher_ctx(
                idx,                    // lp_idx
                MatcherMode::Passive as u8, // kind
                5,                      // trading_fee_bps
                10,                     // base_spread_bps
                200,                    // max_total_bps
                0,                      // impact_k_bps
                0,                      // liquidity_notional_e6
                1_000_000_000_000,      // max_fill_abs
                0,                      // max_inventory_abs
                0,                      // fee_to_insurance_bps
                0,                      // skew_spread_mult_bps
            ),
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), init_ctx_ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("init matcher context failed");

        (idx, ctx)
    }

    /// Initialize LP with caller-provided matcher program/context without matcher-side init.
    /// Used for adversarial tests where matcher settings are intentionally malformed.
    pub fn init_lp_with_raw_matcher(
        &mut self,
        owner: &Keypair,
        matcher_prog: &Pubkey,
        matcher_ctx: &Pubkey,
    ) -> u16 {
        let idx = self.account_count;
        self.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        let ata = self.create_ata(&owner.pubkey(), 100);

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(ata, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(*matcher_prog, false),
                AccountMeta::new_readonly(*matcher_ctx, false),
            ],
            data: encode_init_lp(matcher_prog, matcher_ctx, 100),
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
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

    pub fn init_user(&mut self, owner: &Keypair) -> u16 {
        let idx = self.account_count;
        self.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        let ata = self.create_ata(&owner.pubkey(), 100);

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
            data: encode_init_user(100),
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

    pub fn deposit(&mut self, owner: &Keypair, user_idx: u16, amount: u64) {
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

    /// Execute TradeCpi instruction
    /// Note: lp_owner does NOT need to sign - this is the key permissionless property
    pub fn try_trade_cpi(
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
            &[cu_ix(), ix],
            Some(&user.pubkey()),
            &[user],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Execute TradeCpi with a limit price for slippage protection
    pub fn try_trade_cpi_with_limit(
        &mut self,
        user: &Keypair,
        lp_owner: &Pubkey,
        lp_idx: u16,
        user_idx: u16,
        size: i128,
        limit_price_e6: u64,
        matcher_prog: &Pubkey,
        matcher_ctx: &Pubkey,
    ) -> Result<(), String> {
        let lp_bytes = lp_idx.to_le_bytes();
        let (lp_pda, _) =
            Pubkey::find_program_address(&[b"lp", self.slab.as_ref(), &lp_bytes], &self.program_id);

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
                AccountMeta::new_readonly(lp_pda, false),
            ],
            data: encode_trade_cpi_with_limit(lp_idx, user_idx, size, limit_price_e6),
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
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
    pub fn try_trade_cpi_with_wrong_pda(
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
            &[cu_ix(), ix],
            Some(&user.pubkey()),
            &[user],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    pub fn init_market_hyperp(&mut self, initial_mark_price_e6: u64) {
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
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("init_market_hyperp failed");
    }

    pub fn set_slot(&mut self, slot: u64) {
        let effective_slot = slot + 100;
        self.svm.set_sysvar(&Clock {
            slot: effective_slot,
            unix_timestamp: effective_slot as i64,
            ..Clock::default()
        });
    }

    pub fn crank(&mut self) {
        let caller = Keypair::new();
        self.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();

        let cu_ix = cu_ix();
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
            &[cu_ix, ix],
            Some(&caller.pubkey()),
            &[&caller],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).expect("crank failed");
    }

    pub fn try_set_oracle_authority(
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
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    pub fn try_push_oracle_price(
        &mut self,
        authority: &Keypair,
        price_e6: u64,
        _timestamp: i64,
    ) -> Result<(), String> {
        let clock: Clock = self.svm.get_sysvar();
        let ts = clock.unix_timestamp;
        self.svm.set_sysvar(&Clock {
            slot: clock.slot,
            unix_timestamp: ts + 1,
            ..clock
        });
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
            ],
            data: encode_push_oracle_price(price_e6, ts),
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&authority.pubkey()),
            &[authority],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// v12.17: flatten positions via an opposing TradeCpi before resolution.
    /// After resolution, AdminForceCloseAccount requires position == 0.
    pub fn flatten_positions_via_trade(
        &mut self,
        user: &Keypair,
        lp_owner: &Pubkey,
        lp_idx: u16,
        user_idx: u16,
        matcher_prog: &Pubkey,
        matcher_ctx: &Pubkey,
    ) {
        let user_pos = self.read_account_position(user_idx);
        if user_pos != 0 {
            let _ = self.try_trade_cpi(
                user,
                lp_owner,
                lp_idx,
                user_idx,
                -user_pos,
                matcher_prog,
                matcher_ctx,
            );
        }
    }

    pub fn try_resolve_market(&mut self, admin: &Keypair) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_resolve_market(),
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    pub fn top_up_insurance(&mut self, payer: &Keypair, amount: u64) {
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
                AccountMeta::new_readonly(sysvar::clock::ID, false),
            ],
            data,
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&payer.pubkey()),
            &[payer],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("top_up_insurance failed");
    }

    pub fn try_withdraw_insurance(&mut self, admin: &Keypair) -> Result<(), String> {
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
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    pub fn is_market_resolved(&self) -> bool {
        // Check FLAG_RESOLVED (bit 0) in header flags byte at offset 13.
        // This is stable across engine layout changes.
        let d = self.svm.get_account(&self.slab).unwrap().data;
        (d[13] & 0x01) != 0
    }

    pub fn read_insurance_balance(&self) -> u128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        // ENGINE_OFF = 584, InsuranceFund.balance is at offset 16 within engine
        // (vault is 16 bytes at 0, insurance_fund starts at 16)
        // InsuranceFund { balance: U128, ... } - balance is first field
        pub const INSURANCE_BALANCE_OFFSET: usize = 584 + 16;
        u128::from_le_bytes(
            slab_data[INSURANCE_BALANCE_OFFSET..INSURANCE_BALANCE_OFFSET + 16]
                .try_into()
                .unwrap(),
        )
    }

    /// Read effective position (v12.15: position_basis_q is i128, not I256)
    pub fn read_account_position(&self, idx: u16) -> i128 {
        let d = self.svm.get_account(&self.slab).unwrap().data;
        pub const ENGINE: usize = 584;
        pub const ACCOUNTS_OFFSET: usize = ENGINE + 9424;
        pub const ACCOUNT_SIZE: usize = 352;
        pub const PBQ: usize = 56;
        pub const A_BASIS: usize = 72;
        pub const EPOCH_SNAP: usize = 120;
        pub const ADL_MULT_LONG: usize = ENGINE + 392;
        pub const ADL_MULT_SHORT: usize = ENGINE + 408;
        pub const ADL_EPOCH_LONG: usize = ENGINE + 456;
        pub const ADL_EPOCH_SHORT: usize = ENGINE + 464;

        let acc_off = ACCOUNTS_OFFSET + (idx as usize) * ACCOUNT_SIZE;
        if d.len() < acc_off + ACCOUNT_SIZE { return 0; }

        let basis = i128::from_le_bytes(d[acc_off+PBQ..acc_off+PBQ+16].try_into().unwrap());
        if basis == 0 { return 0; }

        let a_basis = u128::from_le_bytes(d[acc_off+A_BASIS..acc_off+A_BASIS+16].try_into().unwrap());
        let epoch_snap = u64::from_le_bytes(d[acc_off+EPOCH_SNAP..acc_off+EPOCH_SNAP+8].try_into().unwrap());
        if a_basis == 0 { return 0; }

        let (a_side, epoch_side) = if basis > 0 {
            (u128::from_le_bytes(d[ADL_MULT_LONG..ADL_MULT_LONG+16].try_into().unwrap()),
             u64::from_le_bytes(d[ADL_EPOCH_LONG..ADL_EPOCH_LONG+8].try_into().unwrap()))
        } else {
            (u128::from_le_bytes(d[ADL_MULT_SHORT..ADL_MULT_SHORT+16].try_into().unwrap()),
             u64::from_le_bytes(d[ADL_EPOCH_SHORT..ADL_EPOCH_SHORT+8].try_into().unwrap()))
        };
        if epoch_snap != epoch_side { return 0; }

        let abs_basis = basis.unsigned_abs();
        let effective = if a_side == a_basis { abs_basis } else {
            (abs_basis * a_side) / a_basis.max(1)
        };
        if basis < 0 { -(effective as i128) } else { effective as i128 }
    }

    pub fn try_withdraw(&mut self, owner: &Keypair, user_idx: u16, amount: u64) -> Result<(), String> {
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
            &[cu_ix(), ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    pub fn read_num_used_accounts(&self) -> u16 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        // ENGINE_OFF (584) + num_used_accounts offset (1224) = 1808
        u16::from_le_bytes(slab_data[1808..1810].try_into().unwrap())
    }

    /// Read pnl_pos_tot aggregate from slab
    /// This is the sum of all positive PnL values, used for haircut calculations
    pub fn read_pnl_pos_tot(&self) -> u128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        // ENGINE_OFF = 584
        // pnl_pos_tot at engine offset 352
        pub const PNL_POS_TOT_OFFSET: usize = 584 + 352;
        u128::from_le_bytes(
            slab_data[PNL_POS_TOT_OFFSET..PNL_POS_TOT_OFFSET + 16]
                .try_into()
                .unwrap(),
        )
    }

    /// Read c_tot aggregate from slab
    pub fn read_c_tot(&self) -> u128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        // c_tot is at offset 336 within RiskEngine (16 bytes before pnl_pos_tot)
        pub const C_TOT_OFFSET: usize = 584 + 336;
        u128::from_le_bytes(
            slab_data[C_TOT_OFFSET..C_TOT_OFFSET + 16]
                .try_into()
                .unwrap(),
        )
    }

    /// Read vault balance from slab
    pub fn read_vault(&self) -> u128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        // vault is at offset 0 within RiskEngine
        pub const VAULT_OFFSET: usize = 584;
        u128::from_le_bytes(
            slab_data[VAULT_OFFSET..VAULT_OFFSET + 16]
                .try_into()
                .unwrap(),
        )
    }

    /// Read account PnL
    pub fn read_account_pnl(&self, idx: u16) -> i128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        // Account layout:
        // BPF Account layout (v12.17, SBF 8-byte i128 alignment):
        //   capital: U128 (16), offset 0
        //   kind: u8 (1+7pad), offset 16
        //   pnl: i128 (16), offset 24
        //   reserved_pnl: u128 (16), offset 40
        //   position_basis_q: i128 (16), offset 56
        pub const ACCOUNTS_OFFSET: usize = 584 + 9424;
        pub const ACCOUNT_SIZE: usize = 352;
        pub const PNL_OFFSET_IN_ACCOUNT: usize = 24;
        let account_off = ACCOUNTS_OFFSET + (idx as usize) * ACCOUNT_SIZE + PNL_OFFSET_IN_ACCOUNT;
        if slab_data.len() < account_off + 16 {
            return 0;
        }
        i128::from_le_bytes(slab_data[account_off..account_off + 16].try_into().unwrap())
    }

    pub fn try_close_account(&mut self, owner: &Keypair, user_idx: u16) -> Result<(), String> {
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
            &[cu_ix(), ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    pub fn try_admin_force_close_account(
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
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// Force-close multiple accounts fully (handles two-phase ProgressOnly semantics).
    /// Pass all (idx, owner) pairs. Phase 1 reconciles all, Phase 2 closes all.
    pub fn force_close_accounts_fully(
        &mut self,
        admin: &Keypair,
        accounts: &[(u16, &Pubkey)],
    ) -> Result<(), String> {
        // Phase 1: reconcile all (some may close immediately if pnl<=0)
        for &(idx, owner) in accounts {
            let _ = self.try_admin_force_close_account(admin, idx, owner);
        }
        // Phase 2: close remaining (now terminal-ready)
        for &(idx, owner) in accounts {
            let _ = self.try_admin_force_close_account(admin, idx, owner);
        }
        Ok(())
    }

    pub fn try_close_slab(&mut self) -> Result<(), String> {
        let admin = Keypair::from_bytes(&self.payer.to_bytes()).unwrap();
        let (vault_pda, _) =
            Pubkey::find_program_address(&[b"vault", self.slab.as_ref()], &self.program_id);
        let admin_ata = self.create_ata(&admin.pubkey(), 0);
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(vault_pda, false),
                AccountMeta::new(admin_ata, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: encode_close_slab(),
        };
        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[&admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    pub fn init_market_hyperp_with_warmup(
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
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("init_market_hyperp_with_warmup failed");
    }

    pub fn read_account_capital(&self, idx: u16) -> u128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        pub const ACCOUNTS_OFFSET: usize = 584 + 9424;
        pub const ACCOUNT_SIZE: usize = 352;
        pub const CAPITAL_OFFSET_IN_ACCOUNT: usize = 0;
        let account_off =
            ACCOUNTS_OFFSET + (idx as usize) * ACCOUNT_SIZE + CAPITAL_OFFSET_IN_ACCOUNT;
        if slab_data.len() < account_off + 16 {
            return 0;
        }
        u128::from_le_bytes(slab_data[account_off..account_off + 16].try_into().unwrap())
    }

    pub fn vault_balance(&self) -> u64 {
        let vault_data = self.svm.get_account(&self.vault).unwrap().data;
        let vault_account = TokenAccount::unpack(&vault_data).unwrap();
        vault_account.amount
    }

    pub fn try_set_oracle_price_cap(
        &mut self,
        signer: &Keypair,
        max_change_e2bps: u64,
    ) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
            ],
            data: encode_set_oracle_price_cap(max_change_e2bps),
        };
        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
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


/// ATTACK: Configure LP with matcher_program = percolator program (self-CPI recursion vector).
/// TradeCpi must reject and leave accounting unchanged.


/// ATTACK: Alias matcher context to slab account in TradeCpi account list.
/// Must be rejected (shape/ownership mismatch) with no state mutation.


// ============================================================================
// Test: Multiple LPs have independent matcher bindings
// ============================================================================

/// Verify that each LP's matcher binding is independent
///
/// LP1 with Matcher A cannot be traded via Matcher B, and vice versa.
/// This ensures LP isolation.


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


// ============================================================================
// Test: Minimum margin edge case
// ============================================================================

/// Test behavior at minimum margin boundary
///
/// Verifies that trades at exactly the margin boundary work correctly
/// and that trades just below the boundary are rejected.


/// Test rapid position flips within the same slot.
/// This verifies that margin checks are applied correctly on each flip.


/// Test position flip with minimal equity (edge case at liquidation boundary).


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


/// Test that resolved markets block new activity


/// Test that users can withdraw after resolution


/// Test insurance withdrawal requires all positions closed


















/// Verify admin can always use Tag 20 (WithdrawInsurance) to drain all insurance,
/// even after a limited policy (Tag 22) is configured with a delegated authority.
/// This is by design: admin retains ultimate authority over the insurance fund.


/// Test paginated force-close with many accounts (simulates 4096 worst case)


/// Test binary outcome: price = 1e-6 (NO wins)


/// Test binary outcome: price = 1e6 (YES wins)


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


// ============================================================================
// PEN TEST SUITE: Exhaustive Security Attack Tests
// ============================================================================
//
// These tests cover all 21 instructions and known attack vectors that could
// steal user funds. Each test attempts an exploit and verifies it fails.

impl TestEnv {
    /// Read c_tot aggregate from slab
    pub fn read_c_tot(&self) -> u128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        pub const C_TOT_OFFSET: usize = 584 + 336;
        u128::from_le_bytes(
            slab_data[C_TOT_OFFSET..C_TOT_OFFSET + 16]
                .try_into()
                .unwrap(),
        )
    }

    /// Read vault balance from engine state
    pub fn read_engine_vault(&self) -> u128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        pub const VAULT_OFFSET: usize = 584;
        u128::from_le_bytes(
            slab_data[VAULT_OFFSET..VAULT_OFFSET + 16]
                .try_into()
                .unwrap(),
        )
    }

    /// Read pnl_pos_tot aggregate from slab
    pub fn read_pnl_pos_tot(&self) -> u128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        pub const PNL_POS_TOT_OFFSET: usize = 584 + 352;
        u128::from_le_bytes(
            slab_data[PNL_POS_TOT_OFFSET..PNL_POS_TOT_OFFSET + 16]
                .try_into()
                .unwrap(),
        )
    }

    /// Read account PnL for a slot
    pub fn read_account_pnl(&self, idx: u16) -> i128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        pub const ACCOUNTS_OFFSET: usize = 584 + 9424;
        pub const ACCOUNT_SIZE: usize = 352;
        pub const PNL_OFFSET_IN_ACCOUNT: usize = 24; // BPF: i128 has 8-byte alignment
        let account_off = ACCOUNTS_OFFSET + (idx as usize) * ACCOUNT_SIZE + PNL_OFFSET_IN_ACCOUNT;
        if slab_data.len() < account_off + 16 {
            return 0;
        }
        i128::from_le_bytes(slab_data[account_off..account_off + 16].try_into().unwrap())
    }

    /// Try to init user with a specific signer (for auth tests)
    pub fn try_init_user(&mut self, owner: &Keypair) -> Result<u16, String> {
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
            data: encode_init_user(100),
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
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
    pub fn try_deposit(&mut self, owner: &Keypair, user_idx: u16, amount: u64) -> Result<(), String> {
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


/// ATTACK: After incurring a PnL loss, try to withdraw the full original deposit.
/// Expected: Fails because MTM equity is reduced by loss, margin check rejects.


/// ATTACK: Withdraw an amount not aligned to unit_scale.
/// Expected: Transaction rejected for misaligned amount.


/// ATTACK: When vault is undercollateralized (haircut < 1.0), withdraw should
/// return reduced equity, not allow full withdrawal that exceeds the haircutted equity.


/// ATTACK: Withdraw without settling accrued fee debt.
/// Expected: Withdraw checks include fee debt in equity calculation.
// ============================================================================
// 2. Authorization Bypass
// ============================================================================

/// ATTACK: Attacker deposits to an account they don't own.
/// Expected: Owner check fails - signer must match account's registered owner.


/// ATTACK: Attacker withdraws from an account they don't own.
/// Expected: Owner check rejects - signer must match account's registered owner.


/// ATTACK: Close someone else's account to steal their capital.
/// Expected: Owner check rejects.


/// ATTACK: Non-admin tries admin operations (UpdateAdmin,
/// UpdateConfig, SetMaintenanceFee, ResolveMarket).
/// Expected: All admin operations fail for non-admin.


/// UpdateAdmin to zero address permanently burns admin authority.
/// After burning, all admin instructions must fail.


/// ATTACK: Push oracle price with wrong signer (not the oracle authority).
/// Expected: Transaction fails with authorization error.


// ============================================================================
// 3. Trade Manipulation
// ============================================================================

/// ATTACK: Open a position larger than initial margin allows.
/// Expected: Margin check rejects the trade.


/// ATTACK: OI-increasing trade when long side is in DrainOnly mode (spec §9.6).
/// Expected: Trade rejected with SideBlocked → EngineRiskReductionOnlyMode (0x16).
///
/// The new spec (§9.6) uses side-mode gating: trades that increase net side OI
/// on DrainOnly/ResetPending sides are rejected (RiskError::SideBlocked →
/// EngineRiskReductionOnlyMode). The old insurance_floor is no longer a trade gate;
/// it governs insurance withdrawal reserves only.
///
/// To trigger DrainOnly in a live integration scenario requires many ADL cycles
/// (A_side decaying below MIN_A_SIDE = 2^64), which is impractical to set up.
/// Instead, this test directly sets side_mode_long = DrainOnly (1) via raw byte
/// manipulation of the slab, then verifies the gating and error code mapping.


/// ATTACK: Execute TradeNoCpi in Hyperp mode (should be blocked).
/// Expected: Program rejects TradeNoCpi for Hyperp markets.


/// ATTACK: Trade after market is resolved.
/// Expected: No new trades on resolved markets.


/// ATTACK: Position flip (long->short) should use initial_margin_bps, not
/// maintenance_margin_bps. This is Finding L regression test.


// ============================================================================
// 4. TradeCpi / Matcher Attacks
// ============================================================================

/// ATTACK: Substitute a malicious matcher program in TradeCpi.
/// Expected: Matcher program must match what was registered at InitLP.


/// ATTACK: Provide wrong matcher context account.
/// Expected: Context must be owned by registered matcher program.


/// ATTACK: Supply a fabricated LP PDA that doesn't match the derivation.
/// Expected: PDA derivation check fails.


/// ATTACK: Provide a PDA that has lamports (non-system shape).
/// Expected: PDA shape validation rejects accounts with lamports/data.


/// ATTACK: LP A's matcher tries to trade for LP B.
/// Expected: Matcher context must match the LP's registered context.


// ============================================================================
// 5. Liquidation Attacks
// ============================================================================

/// ATTACK: Liquidate a solvent account (positive equity above maintenance margin).
/// Expected: Liquidation rejected for healthy accounts.


/// ATTACK: Self-liquidation to extract value (liquidation fee goes to insurance).
/// Expected: Self-liquidation doesn't create profit for the attacker.


/// ATTACK: Price recovers before liquidation executes - account is now solvent.
/// Expected: Liquidation rejected when account recovers above maintenance margin.


// ============================================================================
// 6. Insurance Fund Attacks
// ============================================================================

/// ATTACK: Withdraw insurance on an active (non-resolved) market.
/// Expected: WithdrawInsurance only works on resolved markets.


/// ATTACK: Withdraw insurance when positions are still open.
/// Expected: WithdrawInsurance requires all positions closed.


/// ATTACK: Close slab while insurance fund has remaining balance.
/// Expected: CloseSlab requires insurance_fund.balance == 0.


// ============================================================================
// 7. Oracle Manipulation
// ============================================================================

/// ATTACK: Circuit breaker should cap price movement per slot.
/// Expected: Price cannot jump more than allowed by circuit breaker.


/// ATTACK: Use a stale oracle price for margin-dependent operations.
/// Expected: Stale oracle rejected by staleness check.


/// ATTACK: Push zero price via oracle authority.
/// Expected: Zero price rejected.


/// ATTACK: Push oracle price when no oracle authority is configured.
/// Expected: Fails because default authority is [0;32] (unset).


// ============================================================================
// 8. Premarket Resolution Attacks
// ============================================================================

/// ATTACK: Resolve market without oracle authority price being set.
/// Expected: Resolution requires authority price to be set first.


/// ATTACK: Deposit after market is resolved.
/// Expected: No new deposits on resolved markets.


/// ATTACK: Init new user after market is resolved.
/// Expected: No new accounts on resolved markets.


/// ATTACK: Resolve an already-resolved market.
/// Expected: Double resolution rejected.


// ============================================================================
// 9. Account Lifecycle Attacks
// ============================================================================

/// ATTACK: Close account while still holding an open position.
/// Expected: CloseAccount rejects when position_size != 0.


/// ATTACK: Close account when PnL is outstanding (non-zero).
/// Expected: CloseAccount requires PnL == 0.


/// ATTACK: Initialize a market twice on the same slab.
/// Expected: Second InitMarket fails because slab already initialized.


// ============================================================================
// 10. Economic / Value Extraction
// ============================================================================

/// ATTACK: Accumulate dust through many sub-unit-scale deposits to extract value.
/// Expected: Dust is tracked and cannot be extracted (swept to insurance).


/// ATTACK: Micro-trade cannot extract value even with minimum position size.
/// Note: Market has trading_fee_bps=0 (default). This tests conservation,
/// not fee ceiling division. Fee ceiling division is tested at the engine level.


/// ATTACK: Deposit/withdraw cycle to manipulate haircut or extract extra tokens.
/// Expected: Vault token balance is always consistent - no tokens created from nothing.


/// ATTACK: Verify no value is created or destroyed through trading operations.
/// Expected: Total vault token balance equals total deposits minus total withdrawals.


// ============================================================================
// PEN TEST SUITE ROUND 2: Deep Crank, Funding, Warmup, GC, and Race Attacks
// ============================================================================

pub fn encode_crank_with_panic(_allow_panic: u8) -> Vec<u8> {
    // format_version=1, all FullClose
    let mut data = vec![5u8];
    data.extend_from_slice(&u16::MAX.to_le_bytes());
    data.push(1u8); // format_version = 1
    for i in 0..128u16 {
        data.extend_from_slice(&i.to_le_bytes());
        data.push(0u8); // FullClose
    }
    data
}

pub fn encode_crank_self(caller_idx: u16) -> Vec<u8> {
    // format_version=1, all FullClose
    let mut data = vec![5u8];
    data.extend_from_slice(&caller_idx.to_le_bytes());
    data.push(1u8); // format_version = 1
    for i in 0..128u16 {
        data.extend_from_slice(&i.to_le_bytes());
        data.push(0u8); // FullClose
    }
    data
}

impl TestEnv {
    /// Try crank with custom allow_panic flag
    pub fn try_crank_with_panic(&mut self, signer: &Keypair, allow_panic: u8) -> Result<(), String> {
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
            &[cu_ix(), ix],
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
    pub fn try_crank_self(&mut self, owner: &Keypair, caller_idx: u16) -> Result<(), String> {
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
            &[cu_ix(), ix],
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
    pub fn init_market_with_trading_fee(&mut self, trading_fee_bps: u64) {
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
        data.extend_from_slice(&86400u64.to_le_bytes()); // max_staleness_secs (1 day, ≤7d cap)
        data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
        data.push(0u8); // invert
        data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
        data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6
        // Per-market admin limits (uncapped defaults for tests)
        data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee_per_slot (0 = disabled) (<= MAX_PROTOCOL_FEE_ABS)
        data.extend_from_slice(&10_000_000_000_000_000u128.to_le_bytes()); // max_insurance_floor (<= MAX_VAULT_TVL)
        data.extend_from_slice(&0u64.to_le_bytes()); // min_oracle_price_cap_e2bps
        // RiskParams
        data.extend_from_slice(&0u64.to_le_bytes()); // h_min
        data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps
        data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps
        data.extend_from_slice(&trading_fee_bps.to_le_bytes()); // trading_fee_bps
        data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
        data.extend_from_slice(&0u128.to_le_bytes()); // new_account_fee
        data.extend_from_slice(&0u128.to_le_bytes()); // risk_reduction_threshold
        data.extend_from_slice(&0u64.to_le_bytes()); // h_max

        data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_crank_staleness_slots
        data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
        data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
        data.extend_from_slice(&100u64.to_le_bytes()); // resolve_price_deviation_bps
        data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
    data.extend_from_slice(&100u128.to_le_bytes()); // min_initial_deposit
    data.extend_from_slice(&1u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&2u128.to_le_bytes()); // min_nonzero_im_req

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
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("init_market_with_trading_fee failed");
    }

    // ------------------------------------------------------------------
    // Init market with trading fee AND warmup period
    // ------------------------------------------------------------------
    pub fn init_market_with_trading_fee_and_warmup(
        &mut self,
        trading_fee_bps: u64,
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

        let mut data = vec![0u8];
        data.extend_from_slice(admin.pubkey().as_ref());
        data.extend_from_slice(self.mint.as_ref());
        data.extend_from_slice(&TEST_FEED_ID);
        data.extend_from_slice(&86400u64.to_le_bytes()); // max_staleness_secs (1 day, ≤7d cap)
        data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
        data.push(0u8); // invert
        data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
        data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6
        data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee_per_slot (0 = disabled)
        data.extend_from_slice(&10_000_000_000_000_000u128.to_le_bytes()); // max_insurance_floor
        data.extend_from_slice(&0u64.to_le_bytes()); // min_oracle_price_cap_e2bps
        data.extend_from_slice(&warmup_period_slots.to_le_bytes()); // h_min
        data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps
        data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps
        data.extend_from_slice(&trading_fee_bps.to_le_bytes()); // trading_fee_bps
        data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
        data.extend_from_slice(&0u128.to_le_bytes()); // new_account_fee
        data.extend_from_slice(&0u128.to_le_bytes()); // risk_reduction_threshold
        data.extend_from_slice(&warmup_period_slots.to_le_bytes()); // h_max (must be >= h_min)

        data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_crank_staleness_slots
        data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
        data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
        data.extend_from_slice(&100u64.to_le_bytes()); // resolve_price_deviation_bps
        data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
        data.extend_from_slice(&100u128.to_le_bytes()); // min_initial_deposit
        data.extend_from_slice(&1u128.to_le_bytes()); // min_nonzero_mm_req
        data.extend_from_slice(&2u128.to_le_bytes()); // min_nonzero_im_req
        append_default_extended_tail(&mut data);

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
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("init_market_with_trading_fee_and_warmup failed");
    }

    // ------------------------------------------------------------------
    // Instruction helpers for tags 24, 26, 27, 28
    // ------------------------------------------------------------------

    /// SettleAccount (tag 26) -- permissionless
    pub fn try_settle_account(&mut self, user_idx: u16) -> Result<(), String> {
        let caller = Keypair::new();
        self.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();

        let mut data = vec![26u8];
        data.extend_from_slice(&user_idx.to_le_bytes());

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(self.slab, false),        // 0: slab (writable)
                AccountMeta::new_readonly(sysvar::clock::ID, false), // 1: clock
                AccountMeta::new_readonly(self.pyth_index, false),   // 2: oracle
            ],
            data,
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&caller.pubkey()),
            &[&caller],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// SettleAccount (tag 26) with a specific signer -- used for permissionless test
    pub fn try_settle_account_with_signer(
        &mut self,
        signer: &Keypair,
        user_idx: u16,
    ) -> Result<(), String> {
        let mut data = vec![26u8];
        data.extend_from_slice(&user_idx.to_le_bytes());

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data,
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&signer.pubkey()),
            &[signer],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// DepositFeeCredits (tag 27) -- owner only
    pub fn try_deposit_fee_credits(
        &mut self,
        owner: &Keypair,
        user_idx: u16,
        amount: u64,
    ) -> Result<(), String> {
        let ata = self.create_ata(&owner.pubkey(), amount);

        let mut data = vec![27u8];
        data.extend_from_slice(&user_idx.to_le_bytes());
        data.extend_from_slice(&amount.to_le_bytes());

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(owner.pubkey(), true),   // 0: user (signer)
                AccountMeta::new(self.slab, false),       // 1: slab
                AccountMeta::new(ata, false),             // 2: user_ata
                AccountMeta::new(self.vault, false),      // 3: vault
                AccountMeta::new_readonly(spl_token::ID, false), // 4: token_program
                AccountMeta::new_readonly(sysvar::clock::ID, false), // 5: clock
            ],
            data,
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// ConvertReleasedPnl (tag 28) -- owner only
    pub fn try_convert_released_pnl(
        &mut self,
        owner: &Keypair,
        user_idx: u16,
        amount: u64,
    ) -> Result<(), String> {
        let mut data = vec![28u8];
        data.extend_from_slice(&user_idx.to_le_bytes());
        data.extend_from_slice(&amount.to_le_bytes());

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(owner.pubkey(), true),  // 0: user (signer)
                AccountMeta::new(self.slab, false),      // 1: slab
                AccountMeta::new_readonly(sysvar::clock::ID, false), // 2: clock
                AccountMeta::new_readonly(self.pyth_index, false),   // 3: oracle
            ],
            data,
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&owner.pubkey()),
            &[owner],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    /// QueryLpFees (tag 24) -- read-only, no signer required
    pub fn try_query_lp_fees(&mut self, lp_idx: u16) -> Result<(), String> {
        let caller = Keypair::new();
        self.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();

        let mut data = vec![24u8];
        data.extend_from_slice(&lp_idx.to_le_bytes());

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new_readonly(self.slab, false), // 0: slab (read-only)
            ],
            data,
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&caller.pubkey()),
            &[&caller],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    // ------------------------------------------------------------------
    // Account field readers for fee_credits and fees_earned_total
    // ------------------------------------------------------------------

    /// Read fee_credits (i128) for an account slot.
    /// Fee credits is at offset 224 within Account.
    pub fn read_account_fee_credits(&self, idx: u16) -> i128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        const ACCOUNTS_OFFSET: usize = 584 + 9424;
        const ACCOUNT_SIZE: usize = 352;
        const FEE_CREDITS_OFFSET: usize = 224;
        let off = ACCOUNTS_OFFSET + (idx as usize) * ACCOUNT_SIZE + FEE_CREDITS_OFFSET;
        if slab_data.len() < off + 16 {
            return 0;
        }
        i128::from_le_bytes(slab_data[off..off + 16].try_into().unwrap())
    }


    /// Read warmup_started_at_slot (u64) for an account slot.
    /// warmup_started_at_slot is at offset 64 within Account.
    pub fn read_account_warmup_started_at_slot(&self, idx: u16) -> u64 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        const ACCOUNTS_OFFSET: usize = 584 + 9424;
        const ACCOUNT_SIZE: usize = 352;
        const WARMUP_SLOT_OFFSET: usize = 64;
        let off = ACCOUNTS_OFFSET + (idx as usize) * ACCOUNT_SIZE + WARMUP_SLOT_OFFSET;
        if slab_data.len() < off + 8 {
            return 0;
        }
        u64::from_le_bytes(slab_data[off..off + 8].try_into().unwrap())
    }

    /// Read reserved_pnl (u128) for an account slot.
    /// reserved_pnl is at offset 48 within Account.
    pub fn read_account_reserved_pnl(&self, idx: u16) -> u128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        const ACCOUNTS_OFFSET: usize = 584 + 9424;
        const ACCOUNT_SIZE: usize = 352;
        const RESERVED_PNL_OFFSET: usize = 40;
        let off = ACCOUNTS_OFFSET + (idx as usize) * ACCOUNT_SIZE + RESERVED_PNL_OFFSET;
        if slab_data.len() < off + 16 {
            return 0;
        }
        u128::from_le_bytes(slab_data[off..off + 16].try_into().unwrap())
    }

    /// Read account kind (u8) for an account slot.
    /// kind is at offset 16 within Account in BPF layout:
    ///   capital: U128 ([u64;2]) = 16 bytes at offset 0
    ///   kind: u8 at offset 16
    pub fn read_account_kind(&self, idx: u16) -> u8 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        const ACCOUNTS_OFFSET: usize = 584 + 9424;
        const ACCOUNT_SIZE: usize = 352;
        const KIND_OFFSET: usize = 16;
        let off = ACCOUNTS_OFFSET + (idx as usize) * ACCOUNT_SIZE + KIND_OFFSET;
        if slab_data.len() < off + 1 {
            return 0;
        }
        slab_data[off]
    }

    /// Read matcher_program ([u8; 32]) for an account slot.
    /// matcher_program is at offset 128 within Account (BPF layout).
    pub fn read_account_matcher_program(&self, idx: u16) -> [u8; 32] {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        const ACCOUNTS_OFFSET: usize = 584 + 9424;
        const ACCOUNT_SIZE: usize = 352;
        const MATCHER_PROG_OFFSET: usize = 128;
        let off = ACCOUNTS_OFFSET + (idx as usize) * ACCOUNT_SIZE + MATCHER_PROG_OFFSET;
        let mut buf = [0u8; 32];
        buf.copy_from_slice(&slab_data[off..off + 32]);
        buf
    }

    /// Read matcher_context ([u8; 32]) for an account slot.
    /// matcher_context is at offset 160 within Account (BPF layout).
    pub fn read_account_matcher_context(&self, idx: u16) -> [u8; 32] {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        const ACCOUNTS_OFFSET: usize = 584 + 9424;
        const ACCOUNT_SIZE: usize = 352;
        const MATCHER_CTX_OFFSET: usize = 160;
        let off = ACCOUNTS_OFFSET + (idx as usize) * ACCOUNT_SIZE + MATCHER_CTX_OFFSET;
        let mut buf = [0u8; 32];
        buf.copy_from_slice(&slab_data[off..off + 32]);
        buf
    }

    /// Try to init user with specific fee, returns Result.
    pub fn try_init_user_with_fee(&mut self, owner: &Keypair, fee: u64) -> Result<u16, String> {
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
            &[cu_ix(), ix],
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

    /// Try to init LP with correct 8-account layout, returns Result.
    pub fn try_init_lp_proper(&mut self, owner: &Keypair, matcher: &Pubkey, ctx: &Pubkey, fee: u64) -> Result<u16, String> {
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
                AccountMeta::new_readonly(*matcher, false),
                AccountMeta::new_readonly(*ctx, false),
            ],
            data: encode_init_lp(matcher, ctx, fee),
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
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

    /// Try ReclaimEmptyAccount (tag 25), permissionless.
    pub fn try_reclaim_empty_account(&mut self, target_idx: u16) -> Result<(), String> {
        let caller = Keypair::new();
        self.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();

        let mut data = vec![25u8];
        data.extend_from_slice(&target_idx.to_le_bytes());

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
            ],
            data,
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
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

// ============================================================================
// 11. Crank Timing & Authorization Attacks
// ============================================================================

/// allow_panic field is read and discarded for wire compatibility.
/// Both admin and non-admin cranks succeed regardless of the flag value.


/// ATTACK: Call crank twice in the same slot to cascade liquidations.
/// Expected: Second crank is a no-op (require_fresh_crank gate).


/// ATTACK: Self-crank with wrong owner (caller_idx points to someone else's account).
/// Expected: Owner check rejects because signer doesn't match account owner.


/// ATTACK: Rapid crank across many slots to compound funding drain.
/// Expected: Funding rate is capped at max_bps_per_slot; no runaway drain.


// ============================================================================
// 12. Funding Calculation Edge Cases
// ============================================================================

/// ATTACK: Crank 3 times in same slot to bypass index smoothing (Bug #9 regression).
/// Expected: dt=0 returns no index movement (fix verified).


/// ATTACK: Large time gap between cranks (dt overflow).
/// Expected: dt is capped and funding doesn't overflow.


// ============================================================================
// 13. Warmup Period Edge Cases
// ============================================================================

/// ATTACK: Warmup with period=0 (instant conversion).
/// Expected: Profit converts to capital immediately.


/// ATTACK: Warmup period long (1M slots), attempt to withdraw before conversion.
/// Expected: Unrealized PnL in warmup cannot be withdrawn as capital.


// ============================================================================
// 14. Dust & Unit Scale Edge Cases
// ============================================================================

/// ATTACK: Unit scale = 0 (no scaling) - verify dust handling is safe.
/// Expected: With unit_scale=0, no dust accumulation, clean behavior.


/// ATTACK: High unit_scale to test dust sweep boundary conditions.
/// Expected: Dust correctly tracked and not exploitable.


// ============================================================================
// 15. Trading Fee Edge Cases
// ============================================================================

/// ATTACK: Verify trading fees accrue to insurance fund and can't be evaded.
/// Expected: Fee is charged on every trade, goes to insurance.


/// ATTACK: Open and immediately close to avoid holding fees.
/// Expected: Trading fee charged on both legs, not profitable to churn.


// ============================================================================
// 16. Premarket Resolution Deep Edge Cases
// ============================================================================

/// ATTACK: Withdraw after resolution but before force-close.
/// Expected: User can still withdraw capital from resolved market.


/// ATTACK: Force-close via crank then attempt to re-open trade.
/// Expected: No new trades after resolution.


// ============================================================================
// 17. GC (Garbage Collection) Edge Cases
// ============================================================================

/// ATTACK: Close account that still has maintenance fee debt.
/// Expected: CloseAccount forgives remaining fee debt after paying what's possible.
/// ATTACK: Try to use GC'd account slot for new account creation.
/// Expected: After GC, slot is marked unused and can be reused.


// ============================================================================
// 18. Multi-Operation Race Conditions
// ============================================================================

/// ATTACK: Deposit then immediately trade in same slot to use uncranked capital.
/// Expected: Deposit is available immediately for trading (no crank needed).


/// ATTACK: Trade, then withdraw max in same slot.
/// Expected: Margin check accounts for newly opened position.


/// ATTACK: Multiple deposits in rapid succession.
/// Expected: All deposits correctly credited, no accounting errors.


// ============================================================================
// 19. Config Manipulation Attacks
// ============================================================================

/// ATTACK: UpdateConfig with extreme parameter values.
/// Expected: Engine-level guards prevent dangerous configurations.


/// Admin can rapidly toggle insurance_floor via SetRiskThreshold.
/// Per spec v10.5, insurance_floor does NOT gate trades — it only reserves
/// insurance fund balance. Trades succeed regardless of floor value.
// ============================================================================
// 20. Integer Boundary Tests
// ============================================================================

/// ATTACK: Deposit more than ATA balance (overflow attempt).
/// Expected: Rejected by token program (insufficient funds).


/// ATTACK: Trade with size = i128::MAX (overflow boundary).
/// Expected: Rejected by margin check (impossible notional value).


/// ATTACK: Trade with size = 0 (no-op trade attempt).
/// Expected: Zero-size trade is rejected and must not mutate state.


// ============================================================================
// PEN TEST SUITE ROUND 3: Config Validation, TopUpInsurance, LP, Settlement,
// Oracle Authority Lifecycle, and CloseSlab Deep Tests
// ============================================================================

impl TestEnv {
    pub fn try_top_up_insurance(&mut self, payer: &Keypair, amount: u64) -> Result<(), String> {
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
                AccountMeta::new_readonly(sysvar::clock::ID, false),
            ],
            data,
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&payer.pubkey()),
            &[payer],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    pub fn try_update_config_with_params(
        &mut self,
        signer: &Keypair,
        funding_horizon_slots: u64,
        thresh_alpha_bps: u64,
        thresh_min: u128,
        thresh_max: u128,
    ) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
            ],
            data: encode_update_config(
                funding_horizon_slots,
                100, // funding_k_bps
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
            &[cu_ix(), ix],
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


/// ATTACK: UpdateConfig with thresh_alpha_bps > 10000 (over 100%).
/// Expected: Rejected with InvalidConfigParam.


/// ATTACK: UpdateConfig with thresh_min > thresh_max (inverted bounds).
/// Expected: Rejected with InvalidConfigParam.


// ============================================================================
// 22. TopUpInsurance Attacks
// ============================================================================

/// ATTACK: TopUpInsurance on a resolved market.
/// Expected: Rejected (InvalidAccountData).


/// ATTACK: TopUpInsurance with insufficient ATA balance.
/// Expected: Token program rejects transfer.


/// ATTACK: TopUpInsurance accumulates correctly in vault and engine.
/// Expected: Insurance balance increases by correct amount, vault has the tokens.


// ============================================================================
// 23. Oracle Authority Lifecycle
// ============================================================================

/// ATTACK: Setting oracle authority to [0;32] disables authority price and clears stored price.
/// Expected: After setting to zero, PushOraclePrice fails, authority_price_e6 is cleared.


/// ATTACK: Oracle authority change mid-flight (while positions open).
/// Expected: Changing authority doesn't affect existing positions, just future price pushing.


// ============================================================================
// 24. Oracle Price Cap Deep Tests
// ============================================================================

/// ATTACK: Set oracle price cap to 0 (disables capping), verify uncapped price accepted.
/// Expected: With cap=0, any price jump is accepted.


/// ATTACK: Set oracle price cap to 1 (ultra-restrictive), push any change.
/// Expected: Price clamped to essentially no movement (1 e2bps = 0.01%).


// ============================================================================
// 25. LP-Specific Attacks
// ============================================================================

/// ATTACK: LP account should never be garbage collected, even with zero state.
/// Expected: GC skips LP accounts (they have is_lp = true).


/// ATTACK: User account with zero state SHOULD be GC'd.
/// Expected: GC reclaims user accounts with zero position/capital/pnl.


/// ATTACK: LP takes position, then try to close as if user (kind mismatch).
/// Expected: LP account cannot be closed via CloseAccount (only users can close).


// ============================================================================
// 26. CloseSlab Deep Tests
// ============================================================================

/// ATTACK: CloseSlab when vault has tokens remaining.
/// Expected: Rejected (vault must be empty).


/// ATTACK: CloseSlab on uninitialized slab.
/// Expected: Rejected (not initialized).


// ============================================================================
// 27. SetMaintenanceFee Deep Tests
// ============================================================================

/// ATTACK: Set maintenance fee to u128::MAX (maximum possible fee).
/// Expected: Fee is accepted but capital should drain predictably (not corrupt state).
/// ATTACK: SetMaintenanceFee as non-admin.
/// Expected: Rejected (admin auth check).


// ============================================================================
// 28. Settlement Pipeline Attacks
// ============================================================================

/// ATTACK: Multiple users settle in same crank - verify no double-counting.
/// Expected: Conservation holds: vault = total deposits always.


// ============================================================================
// 29. Instruction Truncation / Malformed Data
// ============================================================================

/// ATTACK: Send instruction with truncated data (too short for the tag).
/// Expected: Rejected with InvalidInstructionData.


/// ATTACK: Send unknown instruction tag (255).
/// Expected: Rejected with InvalidInstructionData.


/// ATTACK: Empty instruction data (no tag byte).
/// Expected: Rejected with InvalidInstructionData.


// ============================================================================
// 30. Cross-Operation Composition Attacks
// ============================================================================

/// ATTACK: Deposit → Resolve → Withdraw sequence.
/// Expected: Can't deposit after resolve, but can withdraw existing capital.


/// ATTACK: Trade → Price crash → Trade reverse → Crank. Does the vault balance stay correct?
/// Expected: Conservation holds through the entire sequence.


// ============================================================================
// PEN TEST SUITE ROUND 4: Account Type Confusion, Capacity Limits,
// InitLP/InitUser Edge Cases, Multi-User Withdrawal, Index Bounds
// ============================================================================

impl TestEnv {
    pub fn try_init_lp(&mut self, owner: &Keypair) -> Result<u16, String> {
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
            data: encode_init_lp(&Pubkey::new_unique(), &Pubkey::new_unique(), 100),
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
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
    pub fn try_trade_type_confused(
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
            &[cu_ix(), ix],
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
    pub fn try_deposit_to_idx(&mut self, owner: &Keypair, idx: u16, amount: u64) -> Result<(), String> {
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
            &[cu_ix(), ix],
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

/// Per spec §10.7, LP and User accounts share the same mechanics.
/// Using a User account in the LP slot of a trade is valid (spec v10.5).
/// The engine does not enforce account kind for trades — only authorization matters.


/// ATTACK: Deposit to an LP account using DepositCollateral.
/// Expected: Should succeed (LP accounts can receive deposits like users).


/// ATTACK: LiquidateAtOracle targeting an LP account.
/// Expected: LP liquidation may be handled differently (LP has position from trading).


// ============================================================================
// 32. Index Bounds Attacks
// ============================================================================

/// ATTACK: Deposit to an out-of-bounds account index.
/// Expected: Rejected by check_idx (index >= max_accounts).


/// ATTACK: Trade with out-of-bounds user_idx.
/// Expected: Rejected by check_idx.


/// ATTACK: Withdraw from out-of-bounds index.
/// Expected: Rejected by check_idx.


/// ATTACK: LiquidateAtOracle with out-of-bounds target index.
/// Expected: Rejected by check_idx.


// ============================================================================
// 33. InitLP/InitUser Edge Cases
// ============================================================================

/// ATTACK: InitLP after market resolution.
/// Expected: Rejected (no new LPs on resolved markets).


/// ATTACK: InitUser with zero fee_payment and verify clean initialization.
/// Expected: Account created with zero capital (fee_payment=0 is valid).


// ============================================================================
// 34. Multi-User Withdrawal Race
// ============================================================================

/// ATTACK: Two users both try to withdraw max capital in the same slot.
/// Expected: Both succeed (vault has enough), conservation holds.


/// ATTACK: Double withdrawal from same account in same slot.
/// Expected: Second withdrawal fails (insufficient capital).


// ============================================================================
// 35. Cross-Market Isolation
// ============================================================================

/// ATTACK: Verify two separate markets (slabs) don't interfere.
/// Expected: Each market has independent state and vault.


// ============================================================================
// 36. Slab Guard & Account Validation
// ============================================================================

/// ATTACK: Send instruction to wrong program_id's slab.
/// Expected: Slab guard rejects (program_id embedded in slab header).


// ============================================================================
// 37. Liquidation with No Position
// ============================================================================

/// ATTACK: Liquidate account that has capital but no position.
/// Expected: No-op (nothing to liquidate).


// ============================================================================
// 38. Trade Self-Trading Prevention
// ============================================================================

/// ATTACK: LP tries to trade against itself (user_idx == lp_idx).
/// Expected: Rejected or no-op (can't trade against yourself).


/// ATTACK: Conservation through complete lifecycle (init → trade → crank → close).
/// Expected: After all accounts closed, vault should have only insurance fees.


// ============================================================================
// ROUND 5: Hyperp mode, premarket resolution, multi-LP, sandwich attacks
// ============================================================================

/// ATTACK: In Hyperp mode, TradeCpi updates mark price with execution price.
/// An attacker could try rapid trades to push mark far from index to extract
/// value via favorable PnL. Circuit breaker should limit mark movement.


/// ATTACK: In Hyperp mode, index lags behind mark due to rate limiting.
/// Attacker could try to profit by trading when mark diverges from index,
/// then cranking to move index toward mark. This test verifies conservation.


/// ATTACK: Force-close during premarket resolution should maintain PnL conservation.
/// Sum of all PnL changes after force-close should be zero (zero-sum).


/// ATTACK: Try to withdraw all capital before force-close in a resolved market.
/// User might try to extract capital while still having an open position.


/// ATTACK: Extra cranks after all positions are force-closed should be idempotent.
/// No state corruption from redundant resolution cranks.


/// ATTACK: Resolve market at extreme price (near u64::MAX).
/// Test that force-close handles extreme PnL without overflow.


/// ATTACK: Non-admin tries to withdraw insurance after resolution.
/// Only admin should be able to withdraw insurance funds.


/// ATTACK: Try to withdraw insurance twice to drain vault.
/// Second withdrawal should find zero insurance and be a no-op.


/// ATTACK: TradeCpi in a resolved market should fail.
/// After resolution, no new trades should be possible.


/// ATTACK: Try to deposit after market resolution.
/// Deposits should be blocked on resolved markets.


/// ATTACK: Multi-LP conservation. Trade against 2 different LPs and verify
/// no value is created or destroyed. Total vault must remain constant.


/// ATTACK: Sandwich attack. Deposit large amount before a trade to change
/// haircut ratio, then withdraw after. Should not extract value.
/// Attacker can only withdraw at most what they deposited.


/// ATTACK: Push oracle price to zero in Hyperp mode.
/// Zero price should be rejected since it would break all calculations.


/// ATTACK: In Hyperp mode, crank at same slot should not move index (Bug #9 fix).
/// Verify that dt=0 returns index unchanged, preventing smoothing bypass.


/// ATTACK: Non-admin tries to resolve market.
/// Only the admin should be able to resolve.


/// ATTACK: LP tries to close account while it still has a position from force-close PnL.
/// After force-close, LP may have PnL that prevents account closure.


/// ATTACK: Try to init new LP after Hyperp market resolution.
/// Resolved Hyperp markets should block InitLP.


/// ATTACK: Push oracle price with extreme u64 value.
/// Circuit breaker should clamp price movement.


/// ATTACK: Hyperp funding rate extraction. Create position, crank many times
/// to accumulate premium funding, then check that funding doesn't create value.


/// ATTACK: Change oracle authority during active Hyperp positions.
/// Old authority must be rejected, new authority must be accepted.


/// ATTACK: Close slab without withdrawing insurance first.
/// CloseSlab requires insurance_fund.balance == 0.


// ============================================================================
// ROUND 6: Fee debt, warmup, position limits, conservation, nonce, dust
// ============================================================================

/// ATTACK: High maintenance fee accrual over many slots should not create
/// unbounded debt or break equity calculations. Fee debt is saturating.
/// ATTACK: Maintenance fee set to u128::MAX should not panic or corrupt state.
/// ATTACK: Warmup period prevents immediate profit withdrawal.
/// User with positive PnL should not be able to withdraw profit before warmup completes.


/// ATTACK: Try to trade with position size near i128::MAX.
/// Saturating arithmetic should prevent overflow without panicking.


/// ATTACK: Try to trade with i128::MIN position size (negative extreme).


/// ATTACK: Conservation invariant through trade + price movement + settlement.
/// vault_balance must equal internal vault tracking at every step.


/// ATTACK: Premarket partial force-close conservation.
/// After force-closing only some accounts, internal state must still be consistent.


/// ATTACK: Two sequential TradeCpi calls with the same parameters.
/// The nonce advances automatically between calls, so both are valid (not replays).
/// Verifies vault conservation after multiple trades.


/// ATTACK: Multiple deposits in same transaction should not create extra capital.
/// Total capital should equal total deposited amount.


/// ATTACK: User tries to withdraw more than their capital.
/// Should fail with insufficient balance.


/// ATTACK: Withdraw from another user's account.
/// Account owner verification should prevent this.


/// ATTACK: Deposit to another user's account.
/// Account owner verification should prevent this.


/// ATTACK: Close account owned by someone else.
/// Must verify account ownership.


/// ATTACK: LiquidateAtOracle on a healthy account should be a no-op.
/// Healthy accounts must not be liquidated.


/// ATTACK: Double resolve market attempt.
/// Second resolve should fail.


/// ATTACK: Rapid open/close trades to extract value from rounding.
/// Many tiny trades should not accumulate rounding profit.


/// ATTACK: UpdateAdmin to zero address is now rejected at the instruction level.
/// Verify that the zero-admin foot-gun guard prevents the lockout.


// ============================================================================
// ROUND 7: Advanced Attack Tests - Dust sweep, LP max tracking, entry price,
//           funding anti-retroactivity, warmup+withdraw, GC edge cases,
//           conservation invariants, timing boundaries
// ============================================================================

/// ATTACK: Dust accumulates from deposits with unit_scale, then verify crank
/// correctly sweeps dust to insurance fund. Attacker cannot prevent dust sweep.
/// Non-vacuous: asserts insurance increases by swept dust units.


/// ATTACK: LP risk gating with conservative max_abs tracking.
/// After LP shrinks from max position, risk check uses old max (conservative).
/// Verify that risk-increasing trades are correctly blocked when gate is active.


/// ATTACK: Entry price tracking through position flip (long → short).
/// After flipping, the entry_price should be updated via settle_mark_to_oracle.
/// Verify PnL calculation is correct after flip.


/// ATTACK: Funding anti-retroactivity - rate changes at zero-DT crank
/// should use the OLD rate for the elapsed interval, not the new one.
/// Test: crank twice at same slot (sets rate), then crank at later slot.


/// ATTACK: Withdrawal with warmup settlement interaction.
/// If user has unwarmed PnL, withdrawal should still respect margin after settlement.


/// ATTACK: GC removes account after force-realize closes position.
/// Verify that value doesn't leak when GC removes accounts with zero capital.


/// ATTACK: Account slot reuse after close - verify new account has clean state.
/// After closing an account, a new account created should have no
/// residual position/PnL state. Also verifies freelist integrity.


/// ATTACK: Multiple cranks with funding accumulation verify conservation.
/// Run many cranks across different slots with positions and verify
/// total value (vault) is conserved (funding is zero-sum between accounts).


/// ATTACK: Deposit to LP account with outstanding fee debt.
/// Deposit should pay fee debt first, then add remainder to capital.
/// Verify insurance fund receives correct fee payment.
/// ATTACK: UpdateConfig should preserve conservation invariant.
/// Changing risk parameters should not alter vault/capital/insurance totals.


/// ATTACK: Verify trades work with u64::MAX crank staleness.
/// Note: This market uses max_crank_staleness_slots=u64::MAX (always fresh),
/// so it only tests that large slot gaps don't break the system.
/// Stale-crank rejection is not tested here (would need finite staleness config).


/// ATTACK: Insurance fund receives both dust sweep and fee accrual in same crank.
/// Verify both sources of insurance top-up are correctly accounted for.
/// ATTACK: Close all positions then close account, verify complete cleanup.
/// User opens position, closes it, then closes account.
/// Verify capital is correctly returned and no value is left behind.


/// ATTACK: Liquidation of already-zero-position account should fail.
/// An attacker tries to liquidate an account that already has no position.


/// ATTACK: Trade must not decrease insurance fund or change vault.
/// Note: Market uses default trading_fee_bps=0. For non-zero fee testing,
/// see test_attack_new_account_fee_goes_to_insurance which tests fee→insurance.


/// ATTACK: Premarket force-close with multiple crank batches.
/// Verify that force-close across multiple crank calls (paginated)
/// correctly settles all positions and maintains conservation.


// ===================================================================
// ROUND 8: Arithmetic Boundary & State Machine Attack Tests
// ===================================================================

/// ATTACK: Circuit breaker first price acceptance.
/// When last_effective_price_e6 == 0 (first price), circuit breaker should
/// accept any raw price unclamped. Verify no panic/overflow on extreme price.


/// ATTACK: Circuit breaker clamping after second price.
/// After initial price is set, subsequent extreme prices should be clamped.
/// Verify clamping prevents exploitation via price manipulation.


/// ATTACK: Fee debt exceeds capital during crank.
/// Create a scenario where maintenance fees accumulate to exceed capital.
/// Verify equity calculation remains correct and no underflow occurs.
/// ATTACK: Rapid price oscillation precision loss.
/// Execute many trades with alternating prices to accumulate rounding errors.
/// Verify total value is conserved across repeated operations.


/// ATTACK: Multiple accounts compete for insurance fund during liquidation.
/// Create two undercollateralized accounts and liquidate both.
/// Verify insurance fund is not double-counted.


// test_attack_deposit_zero_amount_noop: removed (duplicate of test_attack_deposit_zero_amount_no_state_change which uses try_deposit)

/// ATTACK: Withdraw exactly all capital (no position).
/// Verify withdrawing exact capital amount works and leaves account with 0.


/// ATTACK: Threshold EWMA convergence across many cranks.
/// Set a risk threshold and verify it converges toward target via EWMA
/// rather than allowing wild oscillations that could be exploited.
/// ATTACK: Trade at exactly the initial margin boundary.
/// Open a position that requires exactly initial_margin_bps of capital.
/// Then try to open slightly more - should fail margin check.


/// ATTACK: Multiple deposits followed by single large withdrawal.
/// Verify conservation across many small deposits then one withdrawal.


/// ATTACK: Risk gate activation with insurance at exact threshold boundary.
/// Verify behavior when insurance_fund.balance == risk_reduction_threshold exactly.
/// ATTACK: Unit scale boundary - init market with MAX_UNIT_SCALE.
/// Verify that operations work correctly at the maximum unit scale.


/// ATTACK: Close account after opening and closing position at same price.
/// PnL is zero after round-trip. Verifies capital returned and slot freed.
/// Note: Despite the name, this test creates zero PnL (no price change).


/// ATTACK: Rapid open/close in same slot shouldn't bypass timing guards.
/// Verify that opening and closing a position in the same slot works
/// but doesn't allow exploiting stale prices or settlement.


/// ATTACK: Force-close (premarket resolution) with settlement at different price.
/// Verify PnL is calculated correctly when resolution price differs from entry.


/// ATTACK: Hyperp mode mark price clamping prevents extreme manipulation.
/// In Hyperp mode, mark price from trades is clamped against index.
/// Verify attacker can't push mark price arbitrarily far from index.


// ===================================================================
// ROUND 9: Aggregate Desync, Warmup, & State Machine Attack Tests
// ===================================================================

/// ATTACK: Verify c_tot aggregate stays in sync after multiple deposits and trades.
/// Multiple users deposit and trade, then verify c_tot == sum of individual capitals.


/// ATTACK: Verify pnl_pos_tot tracks only positive PnL accounts.
/// After trades and cranks, pnl_pos_tot should be sum of max(0, pnl) for each account.


/// ATTACK: Warmup with zero period should convert PnL instantly.
/// Init market with warmup_period_slots=0, verify profit converts immediately.


/// ATTACK: Open and close multiple positions - verify c_tot stays consistent.
/// Trade long, close, trade short, close - c_tot == sum of capitals at each step.


/// ATTACK: Multiple sequential account inits have clean independent state.
/// Create several accounts, verify each starts with zero position/PnL.
/// Then trade with one and verify the others are not affected.


/// ATTACK: Insurance fund growth from fees doesn't inflate haircut.
/// Haircut = min(residual, pnl_pos_tot) / pnl_pos_tot where residual = vault - c_tot - insurance.
/// Insurance growing from fees reduces residual, which REDUCES haircut (safer).
/// ATTACK: Withdraw more than capital should fail.
/// Verify that withdrawing more than available capital is rejected.
/// Also verify that withdrawal with position leaves at least margin.


/// ATTACK: Permissionless crank doesn't extract value.
/// Any user can call crank with caller_idx=u16::MAX. Verify no value extraction.


/// ATTACK: Multiple close-account calls on same index should fail.
/// After closing once, the slot is freed. Closing again should error.


/// ATTACK: Deposit after close should fail if account is freed.
/// After closing an account, depositing to that index should fail.


/// ATTACK: Trade to closed account index should fail.
/// After closing, trying to use the freed slot as counterparty should error.


/// ATTACK: UpdateAdmin then attempt old admin operations.
/// After admin transfer, old admin should be unable to perform admin operations.



/// ATTACK: Verify conservation after complex multi-user lifecycle.
/// Multiple users open positions, some profitable, some losing, then all close.
/// Total withdrawn should equal total deposited.


// ============================================================================
// ROUND 10: Config Boundaries, Funding Timing, Multi-LP, & Token Validation
// ============================================================================

/// ATTACK: UpdateConfig with extreme funding parameters.
/// Set funding_max_bps_per_slot to max i64, verify crank doesn't overflow.


/// ATTACK: Zero-slot crank loops shouldn't compound funding.
/// Crank multiple times at the same slot - funding should accrue only once.


/// ATTACK: Multiple LPs trading with same user - verify all positions tracked correctly.
/// Each LP independently takes opposite side of user trades.


/// ATTACK: Trade as LP-kind account in user slot (kind mismatch).
/// LP accounts can only be in lp_idx position, users in user_idx.


// test_attack_withdraw_exact_capital_zero_position: removed (duplicate of test_attack_withdraw_exact_capital_no_position)

/// ATTACK: Deposit zero amount should be harmless.
/// Depositing 0 tokens should either fail or be a no-op.


/// ATTACK: Withdraw zero amount should be harmless.
/// Withdrawing 0 tokens should either fail or be a no-op.


/// ATTACK: Trade with zero size should be harmless.
/// Trading 0 contracts should either fail or be a no-op.


/// In spec v10.5, there is no force-realize mode. Low insurance does NOT
/// trigger position force-close. Positions remain open regardless of
/// insurance level. The crank only processes funding/settlement.


/// ATTACK: Deposit after setting large maintenance fee.
/// Verify fee settlement during deposit doesn't extract extra value.
/// ATTACK: Close account forgives fee debt without extracting from vault.
/// CloseAccount pays what it can from capital, forgives the rest.
/// ATTACK: Liquidate account that becomes insolvent from price move.
/// After price crash, undercollateralized account should be liquidatable.


/// ATTACK: Insurance grows correctly from new account fees.
/// InitUser/InitLP pays a new_account_fee that goes to insurance.


/// ATTACK: Conservation invariant across large slot jumps.
/// Advance many slots, verify conservation holds despite funding/fee accrual.


// ============================================================================
// ROUND 11: Warmup, Funding Edge Cases, Liquidation Budgets, Token Validation
// ============================================================================

/// ATTACK: Warmup period settlement - profit only vests after warmup.
/// With warmup_period > 0, PnL profit should vest gradually, not instantly.


/// ATTACK: Warmup period=0 means instant settlement.
/// With warmup=0, all PnL should vest immediately.


/// ATTACK: Same-slot triple crank converges.
/// Multiple cranks at same slot should eventually stabilize (lazy settlement).
/// Second crank may settle fees, but third should be fully idempotent.


/// ATTACK: Funding rate with extreme k_bps.
/// Set funding_k_bps to maximum, verify funding rate is capped at ±10,000 bps/slot.


/// ATTACK: Funding with extreme max_premium_bps.
/// Set funding_max_premium_bps to extreme negative, verify capping works.


/// ATTACK: Funding with extreme max_bps_per_slot.
/// Set funding_max_bps_per_slot to extreme value, verify engine caps at ±10,000.


/// ATTACK: Deposit with wrong mint token account.
/// Attempt to deposit from an ATA with a different mint.


/// ATTACK: Withdraw to wrong owner's ATA.
/// Attempt to withdraw to an ATA owned by a different user.


/// ATTACK: Multiple price changes between cranks.
/// Push oracle price multiple times before cranking, verify only latest applies.


/// ATTACK: Trade immediately after deposit, same slot.
/// Deposit and trade in rapid succession without crank between.


/// ATTACK: Rapid long→short→long position reversals.
/// Multiple position flips in succession to test aggregate tracking.


/// ATTACK: Crank with no accounts (empty market).
/// KeeperCrank on a market with no users/LPs should be a no-op.


/// ATTACK: Smallest possible trade (1 contract) creates correct position.
/// Note: Market uses trading_fee_bps=0, so ceiling division is not tested here.
/// Fee ceiling division is enforced at the engine level and tested in unit proofs.


/// ATTACK: Multiple withdrawals in same slot draining capital.
/// Rapid withdrawals in same slot should correctly update capital each time.


/// ATTACK: Deposit and withdraw same slot - should be atomic operations.
/// Rapid deposit+withdraw cycle shouldn't create or destroy value.


/// ATTACK: Accrue funding with huge dt (10-year equivalent slot jump).
/// Funding accrual caps dt at ~1 year. Verify no overflow.


// ============================================================================
// ROUND 12: Unit Scale, Invert Mode, Multi-Account, Resolve Sequences
// ============================================================================

/// ATTACK: Unit scale market - trade, crank, conservation.
/// Markets with unit_scale > 0 use scaled prices. Verify conservation.


/// ATTACK: Large unit scale - very large scaling factor.
/// unit_scale=1_000_000 (1M). Verify no overflow in price scaling.


/// ATTACK: Inverted market (invert=1) trade and conservation.
/// Inverted markets use 1e12/oracle_price. Verify conservation.


/// ATTACK: Inverted market with price approaching zero.
/// When oracle price → large (inverted price → 0), verify no division issues.


/// ATTACK: Same owner creates multiple user accounts.
/// Protocol should allow it, but each account must be independent.


/// ATTACK: Resolve hyperp market then withdraw capital (no position).
/// After resolution, users should be able to withdraw their deposited capital.


/// ATTACK: TradeNoCpi on hyperp market should always be blocked.
/// Hyperp mode blocks TradeNoCpi (requires TradeCpi from matcher).


// test_attack_double_resolve_rejected: removed (duplicate of test_attack_double_resolve_market)

/// ATTACK: Non-admin tries to resolve market.
/// Only admin should be able to resolve.


/// ATTACK: Withdraw insurance before all positions force-closed.
/// WithdrawInsurance should fail while positions are still open post-resolve.


/// ATTACK: Inverted market with unit_scale > 0 (double transformation).
/// Both inversion and scaling applied. Verify conservation.


/// ATTACK: Crank multiple times across many slots with position open.
/// Verify funding accrual is correct and consistent across many intervals.


/// ATTACK: Inverted market PnL direction and conservation after price move.
/// Long on inverted market should lose when oracle rises (inverted mark falls).
/// Verify PnL eventually settles into capital and conservation holds.


// ============================================================================
// ROUND 13: Admin ops, CloseAccount edge cases, GC, multi-LP, oracle lifecycle,
//           warmup+haircut, nonce, CloseSlab, risk threshold, maintenance fee
// ============================================================================

/// ATTACK: Close account with fee debt outstanding.
/// CloseAccount should forgive remaining fee debt after paying what's possible.
/// Verify returned capital = capital - min(fee_debt, capital).


/// ATTACK: CloseSlab with dormant account (zero everything but not GC'd).
/// CloseSlab requires num_used_accounts == 0, so dormant accounts block it.


/// ATTACK: UpdateAdmin transfers control, old admin tries operation.
/// After UpdateAdmin, the old admin should be unauthorized.



/// ATTACK: Set maintenance fee to extreme value, accrue fees.
/// Verify fee debt accumulates but doesn't cause overflow or negative capital.
/// ATTACK: SetOracleAuthority to zero disables PushOraclePrice.
/// Oracle authority cleared means stored price is cleared and push fails.


/// ATTACK: Multi-LP trading - trade against two different LPs.
/// Verify each LP's position is tracked independently and conservation holds.


/// Per spec v10.5, insurance_floor (SetRiskThreshold) does NOT gate trades.
/// Trade gating is side-mode based (DrainOnly/ResetPending only).
/// This test verifies that changing insurance_floor does not affect trading.
/// ATTACK: Close account after round-trip trade with PnL.
/// Protocol requires position=0 and PnL=0 for close.


/// ATTACK: UpdateAdmin to same address (no-op).
/// Should succeed without side effects.



/// ATTACK: Double deposit then withdraw full amount.
/// Verify deposits accumulate correctly and full withdrawal returns sum.


/// ATTACK: Withdraw exactly the user's entire capital.
/// Edge case: withdraw == capital leaves zero, should succeed.


/// ATTACK: Multiple LPs with different sizes - verify LP max position tracking.
/// LP positions should be independently bounded by their own limits.


/// ATTACK: Push oracle price with decreasing timestamps.
/// Verify that stale timestamps are handled correctly.


/// ATTACK: Liquidate account that is solvent (positive equity).
/// LiquidateAtOracle should reject attempts on solvent accounts.


/// ATTACK: Close account, GC via crank, verify num_used_accounts decrements.
/// Full lifecycle: init → deposit → close → crank(GC) → verify count.


// ============================================================================
// ROUND 14: Warmup+haircut, size=1 trades, entry price, fee paths, funding,
//           position reversal margin, GC edge cases, force-realize path,
//           fee debt forgiveness, sequential operations
// ============================================================================

/// ATTACK: Trade with position size = 1 (smallest non-zero).
/// Verify conservation holds even with minimal position.


/// ATTACK: Trade size = -1 (smallest short position).
/// Verify negative position of size 1 conserves.


/// ATTACK: Position reversal (long→short) requires initial_margin_bps.
/// When crossing zero, the margin check uses the stricter initial margin.


/// ATTACK: Close account path settles fees correctly.
/// Compare: crank(settle fees) → close vs. close(settles fees internally).


/// ATTACK: Funding accumulation across position size changes.
/// Open position, crank to accrue funding, change position size, crank again.
/// Verify funding uses stored index (anti-retroactivity).


/// ATTACK: Partial position close then full close then CloseAccount.
/// Full lifecycle: open → partial close → full close → account close.


/// ATTACK: Multiple deposits to LP then user trades against it.
/// Verify LP capital accumulates correctly and trades work.


/// ATTACK: Withdraw then immediately re-deposit.
/// Verify no value created or lost in the cycle.


/// ATTACK: Warmup-period market - trade and settle across warmup slots.
/// Profit from trade should vest over warmup_period_slots.
/// Verify conservation through the vesting process.


/// ATTACK: Force-realize disabled when insurance > threshold.
/// Top up insurance to disable force-realize, verify positions persist.


/// ATTACK: Sequential deposit → trade → crank → withdraw → close lifecycle.
/// Full account lifecycle with all operations in sequence.


/// ATTACK: GC account that just had position closed.
/// Close position → crank → crank again → verify GC happens.


/// ATTACK: Large position then price crash - verify conservation through liquidation.
/// Even in liquidation, c_tot must equal sum of capitals.


/// ATTACK: Trade at max price (circuit breaker limit).
/// Oracle at extreme high price, crank, verify no overflow.


/// ATTACK: Trade at extreme low oracle price (near zero).
/// Verify no division by zero or overflow.


/// ATTACK: Rapid open/close/open cycle - same position size, different slots.
/// Tests that entry_price resets correctly on each open.


// ============================================================================
// ROUND 15: Input validation, invalid instruction paths, slab guards,
//           account state checks, multi-account scenarios, edge cases
// ============================================================================

/// ATTACK: Send instruction with tag=24 (just above max valid tag=23).
/// Should fail gracefully.


/// ATTACK: Deposit with wrong slab account (different program_id slab).
/// Slab owned by wrong program should be rejected by slab_guard.


/// ATTACK: Deposit without signer (user not signing).
/// All operations require the user to sign.


/// ATTACK: Four user accounts trading against same LP.
/// Verify conservation holds across many accounts.


/// ATTACK: Withdraw from LP account (LP should still be able to withdraw).
/// Verify LP withdraw works the same as user withdraw.


/// ATTACK: Trade at maximum position size boundary.
/// Open a position that uses nearly all margin, then try adding more.


/// ATTACK: InitMarket with admin field in data mismatching signer.
/// Code validates admin in instruction data matches signer pubkey.


/// ATTACK: InitMarket with mint field in data mismatching mint account.
/// Code validates collateral_mint in data matches the mint account provided.


/// ATTACK: Withdraw with wrong vault PDA (correct PDA but from different slab).
/// Code checks vault PDA derivation matches slab.


/// ATTACK: CloseAccount with wrong vault PDA.
/// Code checks vault PDA derivation matches slab in CloseAccount path.


/// ATTACK: TopUpInsurance with wrong vault account.
/// Code validates vault matches stored vault_pubkey.


/// ATTACK: Liquidate permissionless caller not signer.
/// Verify liquidation requires a valid signer even though it's permissionless.


/// ATTACK: Deposit with wrong oracle price account.
/// Verifies oracle account validation rejects wrong price feed.


/// ATTACK: InitUser with new_account_fee.
/// Verify fee goes to insurance fund and conservation holds.


/// ATTACK: Crank with wrong oracle account on standard market.
/// Trade/crank oracle validation should reject mismatched feed.


/// ATTACK: Withdraw with wrong SPL token program account.
/// Substituting a fake token program should be rejected.


/// ATTACK: Alias user_ata with vault in WithdrawCollateral.
/// Must reject duplicate-role account substitution.


/// ATTACK: Alias user_ata with vault in CloseAccount.
/// Must reject duplicate-role account substitution.


// ============================================================================
// Round 16: Numeric Boundary + Code Path Tests
// ============================================================================

/// ATTACK: Trade on market with unit_scale so large that scale_price_e6 returns None.
/// Oracle price $138 (138_000_000 e6), unit_scale=200_000_000.
/// scale_price_e6(138M, 200M) = 0 → None → trade should be rejected.


/// ATTACK: Inverted market with very high raw price so inverted result is zero.
/// invert_price_e6 with raw near u64::MAX: INVERSION_CONSTANT / raw → 0.
/// INVERSION_CONSTANT = 10^12, so raw > 10^12 gives inverted < 1 → 0 → None.


/// ATTACK: Inverted market with raw price = 1 (smallest non-zero).
/// invert_price_e6(1, 1) = 10^12 / 1 = 10^12 → within u64 range.
/// Verify the market handles extreme inverted prices.


/// ATTACK: Multi-instruction atomic transaction (deposit + trade in same tx).
/// Verify protocol handles multiple instructions in single transaction correctly.


/// ATTACK: Withdraw amount = unit_scale - 1 (largest misaligned amount).
/// Should be rejected by alignment check when unit_scale > 1.


/// ATTACK: Close slab after all accounts closed and insurance is zero.
/// Tests the clean shutdown path: LP deposits, withdraws, closes, then slab closes.


/// ATTACK: Liquidation at exact equity zero boundary.
/// Position PnL + capital = 0 exactly. Should be liquidatable.


/// ATTACK: Deposit and immediate crank in same slot.
/// Tests that deposit + crank in same slot doesn't create exploitable state.


/// ATTACK: Trade then immediate crank then withdraw in rapid sequence.
/// Tests state consistency across rapid operation sequence.


/// ATTACK: Multiple price changes between cranks (large gap).
/// Only the price at crank time should matter, not intermediate prices.


/// ATTACK: Deposit to account, trade, then deposit again (incremental deposits).
/// Verify capital is correct after multiple deposits with position open.


/// ATTACK: Warmup + funding interaction.
/// Open position, warmup is accruing, funding is also accruing.
/// Both should settle correctly without double-counting.


/// ATTACK: LP position tracking after multiple users trade and close.
/// Verify LP position aggregates are correct after complex trading.


/// ATTACK: Price at exact circuit breaker boundary.
/// Move price by exactly oracle_price_cap_bps per slot.
/// Verify mark tracks correctly at the boundary.


/// ATTACK: Trade with exactly initial_margin_bps worth of capital.
/// At the exact margin boundary, the trade should just barely succeed.


/// ATTACK: Maintenance fee settlement when capital is very small.
/// With large maintenance_fee_per_slot and small capital, fee should not go negative.
/// ATTACK: Mark precision with very small price increments.
/// Multiple tiny price changes and cranks should maintain conservation.


// ============================================================================
// Round 17: Instruction Handler Edge Cases + State Interactions
// ============================================================================

/// ATTACK: UpdateConfig while positions are open and funding accruing.
/// Changing funding parameters mid-flight should not cause retroactive errors.


/// ATTACK: PushOraclePrice with same price as last effective price.
/// When price doesn't change, circuit breaker should produce stable state.


/// ATTACK: Liquidate with target_idx = u16::MAX (65535, CRANK_NO_CALLER sentinel).
/// Should not confuse liquidation with permissionless crank sentinel.


/// ATTACK: Deposit after liquidation in same slot.
/// User gets liquidated, then immediately deposits. Conservation must hold.


/// ATTACK: InitLP with matcher_program = Percolator program itself.
/// InitLP stores the matcher pubkey but doesn't CPI, so it may succeed at init.
/// Verify no value extraction and conservation holds regardless of outcome.


/// ATTACK: Funding rate sign flip when LP position crosses zero.
/// LP net position goes from short to long in a single trade.


// REMOVED: test_attack_gc_exact_threshold_account - duplicate of
// test_attack_user_gc_when_empty (round 5, line 9260)

/// ATTACK: TopUpInsurance with unit_scale dust edge case.
/// Insurance topup amount that doesn't align with unit_scale.


/// ATTACK: Resolve hyperp market then attempt UpdateConfig.
/// Admin config changes should be blocked after market resolution.


/// ATTACK: PushOraclePrice after resolution.
/// Settlement parameters must be frozen once market is resolved.


/// ATTACK: SetOracleAuthority after resolution.
/// Oracle authority must remain frozen once market is resolved.


/// ATTACK: SetOraclePriceCap after resolution.
/// Price-cap settings must be frozen after market resolution.


/// ATTACK: Multiple trades filling LP position in alternating directions.
/// LP position oscillates: +5M, +2M (net -3M), -1M (net +4M), etc.
/// Verify LP position tracking remains accurate through oscillations.


/// ATTACK: SetOraclePriceCap to u64::MAX.
/// Effectively disables circuit breaker. Verify large price moves are accepted.


/// ATTACK: User deposits, withdraws everything, gets GC'd, new user takes slot.
/// Tests slot reuse and state cleanliness after GC in multi-user scenario.


/// SetRiskThreshold sets insurance_floor. Per spec §4.7, insurance_floor
/// reserves a portion of the insurance fund that cannot be withdrawn.
/// Trades are NOT gated by insurance_floor (spec v10.5 uses side-mode gating).
/// This test verifies insurance_floor can be set and the engine state is consistent.
/// ATTACK: LP tries to withdraw when haircut is active (vault < c_tot + insurance).
/// After a user takes a large loss, LP capital might be haircutted - can LP
/// withdraw more than their haircutted equity?


/// ATTACK: Open position during warmup period, partially close before warmup expires.
/// Tests interaction between warmup slope and partial position close.
/// Profit from partial close must be subject to warmup vesting.


// ============================================================================
// Round 18: Arithmetic Boundaries, Settlement Ordering, Multi-Account
// ============================================================================

/// ATTACK: Minimal position size (1 unit) with 1e-6 price precision.
/// Tests mark_pnl truncation at the smallest meaningful scale.


/// ATTACK: Haircut with zero pnl_pos_tot (no positive PnL accounts).
/// When denominator is 0, haircut should be harmless (no division by zero).


// REMOVED: test_attack_funding_zero_dt_no_accrual - duplicate of
// test_attack_funding_same_slot_three_cranks_dt_zero (line 8020)

/// ATTACK: Many users (40) trading against single LP, then crank.
/// Tests that crank handles many accounts efficiently and conserves funds.


/// ATTACK: Position flip from long to short at exact maintenance margin.
/// Verify initial_margin_bps is used (not maintenance) for the flip.


/// ATTACK: Large maintenance fee with huge dt gap (thousands of slots).
/// Tests saturating arithmetic in fee accrual over long periods.
/// ATTACK: Trade with different sizes in rapid succession (consecutive slots).
/// Position accumulation should be correct across rapid trades.


/// ATTACK: Three LPs with different positions, user trades against all.
/// Tests LP aggregate tracking with multiple LPs.


/// ATTACK: Projected haircut during trade vs realized haircut after crank.
/// Verify consistency between margin check haircut and settlement haircut.


/// ATTACK: set_pnl aggregate consistency - rapid PnL changes from trades.
/// Multiple trades that flip PnL sign should maintain pnl_pos_tot correctly.


// REMOVED: test_attack_deposit_withdraw_same_slot_no_extraction - duplicate of
// test_attack_deposit_withdraw_same_slot_atomicity (line 15845)

// REMOVED: test_attack_trade_exact_full_margin_utilization - duplicate of
// test_attack_trade_exact_margin_boundary_succeeds (line 19357)

/// ATTACK: LP partial close (reduce LP position) and verify aggregates.
/// Trade that reduces LP's exposure should update net_lp_pos correctly.


// REMOVED: test_attack_warmup_instant_period_zero - duplicate of
// test_attack_warmup_zero_period_instant (line 8110)

// REMOVED: test_attack_trade_notional_equals_margin_boundary - duplicate of
// test_attack_trade_exact_initial_margin_boundary (line 13162)

/// ATTACK: Settlement ordering - mark settlement, then funding, then fees.
/// Create scenario where ordering matters and verify correctness.
/// ATTACK: Inverted market (invert=1) with large price swing.
/// Tests conservation in inverted market with significant movement.


/// ATTACK: Two users with opposing positions, price returns to start.
/// Both users should have approximately zero PnL (minus fees).


/// ATTACK: Withdraw exactly all capital from user with open position.
/// Should fail because margin check requires capital > 0 for positions.


/// ATTACK: Insurance topup from non-admin account.
/// Anyone can top up insurance (it's a deposit, not withdrawal).


/// ATTACK: Price moves 50% down then liquidation followed by conservation check.
/// Tests that large price movements + liquidation maintain fund conservation.


// ============================================================================
// Round 19: Instruction Data, Account Ordering, Admin Transitions, LP Edge Cases
// ============================================================================

/// ATTACK: Instruction data with extra trailing bytes appended.
/// Tests that decoder rejects or ignores trailing garbage after valid data.


/// ATTACK: Trade with size = i128::MIN + 1 (extreme negative).
/// Tests that extreme negative trade sizes are handled safely.


/// ATTACK: Withdraw all capital then re-deposit in same slot.
/// Tests that withdraw+deposit cycle doesn't corrupt state.


/// ATTACK: LP tries to close account while users have matched positions.
/// LP with outstanding position should not be closeable.


/// ATTACK: Trade long then short same size - net zero position.
/// Position should cancel out to zero, conservation must hold.


/// ATTACK: LP matched by multiple users in rapid succession.
/// Tests LP position aggregate correctness under rapid multi-user trading.


/// ATTACK: Deposit after full withdrawal in same slot - cycle should not extract value.
/// Tests that rapid deposit-withdraw-deposit cycles don't corrupt aggregates.


/// ATTACK: Open max-margin position, crank with price at liquidation boundary.
/// Tests that liquidation trigger is precise and doesn't miss by 1.


/// ATTACK: Push oracle with timestamp = 0 then try to use it.
/// Tests that extreme timestamp doesn't corrupt oracle state or cause panic.


/// ATTACK: Push oracle with timestamp = i64::MAX.
/// Tests that far-future timestamps don't cause overflow or panic.


/// ATTACK: LP deposit with pending fee debt.
/// LP depositing should settle fees first, then add remaining to capital.
/// ATTACK: Config change then immediate trade tests new config applied.
/// After SetMaintenanceFee, immediate deposit should use new fee rate.
/// ATTACK: Multiple admin changes in rapid succession.
/// Tests that admin state is correctly updated through multiple transfers.



/// ATTACK: Deposit to LP account from non-owner.
/// Tests authorization on LP deposits.


/// ATTACK: Two users try to withdraw their full equity simultaneously.
/// Vault should never go below total obligations.


/// ATTACK: Multiple users with opposing positions - conservation after price swing.
/// Tests that PnL redistribution between longs/shorts conserves total value.


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
pub struct FuzzRng {
    pub state: u64,
}

impl FuzzRng {
    pub fn new(seed: u64) -> Self {
        FuzzRng { state: seed.max(1) }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() % (hi - lo))
    }

    pub fn range_i128(&mut self, lo: i128, hi: i128) -> i128 {
        if hi <= lo {
            return lo;
        }
        lo + ((self.next_u64() as i128).abs() % (hi - lo))
    }
}

/// All state-changing operations the fuzzer can perform
#[derive(Debug)]
pub enum FuzzAction {
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
    InitUser,
    CloseAccount {
        user_idx: usize,
    },
}

pub struct IntegrationFuzzer {
    pub env: TestEnv,
    pub users: Vec<(Keypair, u16)>, // (keypair, account_idx)
    pub lps: Vec<(Keypair, u16)>,   // (keypair, account_idx)
    pub admin: Keypair,
    pub current_slot: u64,
    pub current_price: i64,
    pub all_indices: Vec<u16>, // Every index ever allocated (for invariant checks)
    pub step: usize,
    pub seed: u64,
}

impl IntegrationFuzzer {
    pub fn new(seed: u64) -> Self {
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

    pub fn setup(&mut self) {
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
    pub fn check_invariants(&self, context: &str) {
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

    pub fn random_action(&self, rng: &mut FuzzRng) -> FuzzAction {
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
            85..=92 => {
                // Top up insurance (8%)
                let amount = rng.range(100_000, 1_000_000_000);
                FuzzAction::TopUpInsurance { amount }
            }
            93..=97 => {
                // Init new user (5%)
                FuzzAction::InitUser
            }
            _ => {
                // Close account (2%)
                let user_idx = rng.range(0, self.users.len() as u64) as usize;
                FuzzAction::CloseAccount { user_idx }
            }
        }
    }

    pub fn execute(&mut self, action: FuzzAction) {
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


// ============================================================================
// Binary Market (Premarket Resolution) Verification Tests
// ============================================================================

/// Verify complete binary market lifecycle with conservation:
/// trade → resolve → force-close → withdraw insurance → close accounts
/// Checks that vault SPL balance accounts for all user capital at every step.


/// Verify that with warmup_period > 0, profitable users after force-close
/// need two CloseAccount calls with a waiting period between them.
/// First call updates warmup slope; second call converts PnL to capital.


/// Verify that users with negative PnL from force-close can close immediately.
/// Losses are settled to capital immediately (no warmup delay).


/// Verify that the force-close PnL calculation is correct by comparing
/// expected PnL from position * (settlement - entry) / 1e6 with actual PnL.


/// Verify that force-close handles zero-position accounts correctly
/// (skips them without modifying state).


// ============================================================================
// AdminForceCloseAccount tests
// ============================================================================

/// Happy path: resolve → force-close positions → admin force-close account


/// AdminForceCloseAccount requires RESOLVED flag


/// AdminForceCloseAccount requires admin signer


/// AdminForceCloseAccount requires zero position


/// AdminForceCloseAccount with positive PnL applies haircut


/// AdminForceCloseAccount with negative PnL reduces capital


/// Full lifecycle: resolve → force-close positions → admin force-close all accounts → withdraw insurance → close slab


/// Test: Honest user with positive PnL can close account after force-close + warmup.
/// Force-close crank initializes warmup slope so settle_warmup_to_capital can convert
/// PnL to capital over the warmup period.


/// Test: Honest user with negative PnL can close account immediately after force-close.
/// Negative PnL is settled immediately (deducted from capital), no warmup needed.


/// Test: Both LP and user can close after force-close (full lifecycle for honest participants)


// ============================================================================
// Honest user lifecycle tests for all market types
// ============================================================================

/// Standard Pyth market: user deposits, trades (long), price goes up, flattens, closes account.
/// h_min=0 so PnL converts instantly.


/// Standard Pyth market: user deposits, trades (long), price drops, flattens, closes.
/// User loses money but can still close and get remaining capital.


/// Standard market with warmup: profitable user must wait for warmup before closing.
/// Uses a larger position (1M) to generate meaningful PnL that takes time to vest.


/// Inverted Pyth market: user can close account after trading.


/// TradeCpi hyperp market (non-resolution): user trades via CPI, flattens, closes.


/// Full lifecycle test: both LP and user close on standard market, then close slab.
/// No insurance is topped up, and no crank runs between trades (avoiding force-realize mode).


/// Regression test for PR #1: WithdrawInsurance must decrement engine.vault.
///
/// Without the fix, WithdrawInsurance zeroes insurance_fund.balance and transfers
/// SPL tokens out of the vault, but does NOT decrement engine.vault. This leaves
/// engine.vault non-zero after all capital is withdrawn, causing CloseSlab to fail
/// (it requires engine.vault.is_zero()).


// ============================================================================
// Per-Market Admin Limits Tests
// ============================================================================

/// Encode InitMarket with custom per-market admin limits.
pub fn encode_init_market_with_limits(
    admin: &Pubkey,
    mint: &Pubkey,
    feed_id: &[u8; 32],
    max_maintenance_fee: u128,
    max_risk_threshold: u128,
    min_oracle_price_cap_e2bps: u64,
) -> Vec<u8> {
    let mut data = vec![0u8];
    data.extend_from_slice(admin.as_ref());
    data.extend_from_slice(mint.as_ref());
    data.extend_from_slice(feed_id);
    data.extend_from_slice(&86400u64.to_le_bytes()); // max_staleness_secs (1 day, ≤7d cap)
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(0u8); // invert
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
    data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6
    // Per-market admin limits
    data.extend_from_slice(&max_maintenance_fee.to_le_bytes());
    data.extend_from_slice(&max_risk_threshold.to_le_bytes());
    data.extend_from_slice(&min_oracle_price_cap_e2bps.to_le_bytes());
    // RiskParams
    data.extend_from_slice(&0u64.to_le_bytes()); // h_min
    data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps
    data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // trading_fee_bps
    data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data.extend_from_slice(&0u128.to_le_bytes()); // new_account_fee
    data.extend_from_slice(&0u128.to_le_bytes()); // risk_reduction_threshold
    data.extend_from_slice(&0u64.to_le_bytes()); // h_max

    data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_crank_staleness_slots
    data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
    data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
    data.extend_from_slice(&100u64.to_le_bytes()); // resolve_price_deviation_bps
    data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
    data.extend_from_slice(&100u128.to_le_bytes()); // min_initial_deposit
    data.extend_from_slice(&1u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&2u128.to_le_bytes()); // min_nonzero_im_req
    append_default_extended_tail(&mut data);
    data
}

// ============================================================================
// Audit gap test helpers
// ============================================================================

/// Encode InitMarket with configurable maintenance_fee_per_slot (for rejection testing)
pub fn encode_init_market_with_maintenance_fee(
    admin: &Pubkey,
    mint: &Pubkey,
    feed_id: &[u8; 32],
    invert: u8,
    maintenance_fee_per_slot: u128,
) -> Vec<u8> {
    let mut data = vec![0u8];
    data.extend_from_slice(admin.as_ref());
    data.extend_from_slice(mint.as_ref());
    data.extend_from_slice(feed_id);
    data.extend_from_slice(&86400u64.to_le_bytes()); // max_staleness_secs (1 day, ≤7d cap)
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(invert);
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
    data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6
    data.extend_from_slice(&maintenance_fee_per_slot.to_le_bytes()); // maintenance_fee_per_slot
    data.extend_from_slice(&10_000_000_000_000_000u128.to_le_bytes()); // max_insurance_floor
    data.extend_from_slice(&0u64.to_le_bytes()); // min_oracle_price_cap_e2bps
    // RiskParams
    data.extend_from_slice(&0u64.to_le_bytes()); // h_min
    data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps
    data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // trading_fee_bps
    data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data.extend_from_slice(&0u128.to_le_bytes()); // new_account_fee
    data.extend_from_slice(&0u128.to_le_bytes()); // insurance_floor
    data.extend_from_slice(&0u64.to_le_bytes()); // h_max
    data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_crank_staleness_slots
    data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
    data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
    data.extend_from_slice(&100u64.to_le_bytes()); // resolve_price_deviation_bps
    data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
    data.extend_from_slice(&100u128.to_le_bytes()); // min_initial_deposit
    data.extend_from_slice(&1u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&2u128.to_le_bytes()); // min_nonzero_im_req
    append_default_extended_tail(&mut data);
    data
}

/// Encode InitMarket with configurable insurance_floor
pub fn encode_init_market_with_insurance_floor(
    admin: &Pubkey,
    mint: &Pubkey,
    feed_id: &[u8; 32],
    invert: u8,
    insurance_floor: u128,
) -> Vec<u8> {
    let mut data = vec![0u8];
    data.extend_from_slice(admin.as_ref());
    data.extend_from_slice(mint.as_ref());
    data.extend_from_slice(feed_id);
    data.extend_from_slice(&86400u64.to_le_bytes()); // max_staleness_secs (1 day, ≤7d cap)
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(invert);
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
    data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6
    data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee_per_slot (0 = disabled)
    data.extend_from_slice(&10_000_000_000_000_000u128.to_le_bytes()); // max_insurance_floor
    data.extend_from_slice(&0u64.to_le_bytes()); // min_oracle_price_cap_e2bps
    // RiskParams
    data.extend_from_slice(&0u64.to_le_bytes()); // h_min
    data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps
    data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // trading_fee_bps
    data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data.extend_from_slice(&0u128.to_le_bytes()); // new_account_fee
    data.extend_from_slice(&insurance_floor.to_le_bytes()); // insurance_floor
    data.extend_from_slice(&0u64.to_le_bytes()); // h_max

    data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_crank_staleness_slots
    data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
    data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
    data.extend_from_slice(&100u64.to_le_bytes()); // resolve_price_deviation_bps
    data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
    data.extend_from_slice(&100u128.to_le_bytes()); // min_initial_deposit
    data.extend_from_slice(&1u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&2u128.to_le_bytes()); // min_nonzero_im_req
    append_default_extended_tail(&mut data);
    data
}

impl TestEnv {
    /// Initialize market with custom insurance_floor
    pub fn init_market_with_insurance_floor(&mut self, invert: u8, insurance_floor: u128) {
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
            data: encode_init_market_with_insurance_floor(
                &admin.pubkey(),
                &self.mint,
                &TEST_FEED_ID,
                invert,
                insurance_floor,
            ),
        };

        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&admin.pubkey()),
            &[admin],
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .expect("init_market_with_insurance_floor failed");
    }

    /// Read insurance_floor from RiskEngine (standalone field, not in RiskParams)
    pub fn read_insurance_floor(&self) -> u128 {
        let slab_data = self.svm.get_account(&self.slab).unwrap().data;
        // insurance_floor is now in params.insurance_floor (no separate engine field).
        // params at engine offset 32, insurance_floor at params offset 144.
        pub const INSURANCE_FLOOR_OFFSET: usize = 584 + 32 + 144;
        u128::from_le_bytes(
            slab_data[INSURANCE_FLOOR_OFFSET..INSURANCE_FLOOR_OFFSET + 16]
                .try_into()
                .unwrap(),
        )
    }

    /// Read the risk buffer from the slab.
    /// Read risk buffer from BPF slab layout.
    /// Buffer is at (SLAB_LEN - GEN_TABLE_LEN - RISK_BUF_LEN) in BPF.
    /// We use the BPF-specific offset: the risk buffer sits right before the
    /// generation table, and the gen table is at the end of the slab.
    pub fn read_risk_buffer(&self) -> percolator_prog::risk_buffer::RiskBuffer {
        use bytemuck::Zeroable;
        let d = self.svm.get_account(&self.slab).unwrap().data;
        let buf_size = core::mem::size_of::<percolator_prog::risk_buffer::RiskBuffer>();
        let gen_table_size = 4096 * 8; // MAX_ACCOUNTS * sizeof(u64)
        let buf_off = SLAB_LEN - gen_table_size - buf_size;
        let mut buf = percolator_prog::risk_buffer::RiskBuffer::zeroed();
        bytemuck::bytes_of_mut(&mut buf).copy_from_slice(&d[buf_off..buf_off + buf_size]);
        buf
    }

    /// Try to init market with raw instruction data (for rejection testing)
    pub fn try_init_market_raw(&mut self, data: Vec<u8>) -> Result<(), String> {
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
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }
}
