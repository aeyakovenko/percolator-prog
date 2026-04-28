//! v2 test infrastructure. Replaces the legacy `tests/common/mod.rs`
//! (preserved at `tests/common/legacy.rs`, gated behind
//! `feature = "legacy-tests"` for reference). Encoders here emit the
//! v2 wire format (wincode/Borsh `BORSH_CONFIG`); slab fixtures carry
//! the 8-byte Anchor v2 disc prefix; offset constants are shifted +8.
//!
//! Public surface preserved as much as possible so legacy tests can
//! lift their `#![cfg(feature = "legacy-tests")]` gate without bulk
//! edits.

#![allow(dead_code, unused_imports)]

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
pub use std::path::PathBuf;

// SPL Token program id — same canonical value as `spl_token::ID` /
// `pinocchio_token::ID`.
pub const SPL_TOKEN_ID: Pubkey =
    solana_sdk::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

/// Re-exposed under the legacy `spl_token` name so test files that
/// did `use common::spl_token` keep compiling.
pub mod spl_token {
    pub use super::SPL_TOKEN_ID as ID;
    pub mod state {
        pub struct Account;
        impl Account {
            pub const LEN: usize = super::super::TOKEN_ACCOUNT_LEN;
        }
        pub struct Mint;
        impl Mint {
            pub const LEN: usize = super::super::MINT_LEN;
        }
    }
}

// ── Program IDs ─────────────────────────────────────────────────────────────

/// Canonical declared program id from `declare_id!`. v2's entrypoint
/// validates `crate::ID` against the program id supplied by the
/// runtime, so tests MUST deploy the .so at this address.
pub const PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("Perco1ator111111111111111111111111111111111");

pub const PYTH_RECEIVER_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
    0x0c, 0xb7, 0xfa, 0xbb, 0x52, 0xf7, 0xa6, 0x48, 0xbb, 0x5b, 0x31, 0x7d, 0x9a, 0x01, 0x8b, 0x90,
    0x57, 0xcb, 0x02, 0x47, 0x74, 0xfa, 0xfe, 0x01, 0xe6, 0xc4, 0xdf, 0x98, 0xcc, 0x38, 0x58, 0x81,
]);

pub const TEST_FEED_ID: [u8; 32] = [0xAB; 32];

// ── BPF-target slab/engine layout ───────────────────────────────────────────

#[cfg(all(feature = "small", not(feature = "medium")))]
pub const SLAB_LEN: usize = 96_672;
#[cfg(all(feature = "small", not(feature = "medium")))]
pub const MAX_ACCOUNTS: usize = 256;

#[cfg(all(feature = "medium", not(feature = "small")))]
pub const SLAB_LEN: usize = 382_464;
#[cfg(all(feature = "medium", not(feature = "small")))]
pub const MAX_ACCOUNTS: usize = 1024;

#[cfg(not(any(feature = "small", feature = "medium")))]
pub const SLAB_LEN: usize = 1_525_632;
#[cfg(not(any(feature = "small", feature = "medium")))]
pub const MAX_ACCOUNTS: usize = 4096;

/// All v2 BPF offsets shift +8 vs the legacy native layout (Anchor v2 disc
/// prefix). Body-relative offsets inside SlabHeader / MarketConfig are
/// unchanged; data-relative offsets gain +8.
pub const ENGINE_OFFSET: usize = 528; // legacy 520 + DISC_LEN(8)
pub const ENGINE_BITMAP_OFFSET: usize = 720; // 528 + 192

#[cfg(all(feature = "small", not(feature = "medium")))]
pub const ENGINE_NUM_USED_OFFSET: usize = 752;
#[cfg(all(feature = "small", not(feature = "medium")))]
pub const ENGINE_ACCOUNTS_OFFSET: usize = 1784;

#[cfg(all(feature = "medium", not(feature = "small")))]
pub const ENGINE_NUM_USED_OFFSET: usize = 848;
#[cfg(all(feature = "medium", not(feature = "small")))]
pub const ENGINE_ACCOUNTS_OFFSET: usize = 4952;

#[cfg(not(any(feature = "small", feature = "medium")))]
pub const ENGINE_NUM_USED_OFFSET: usize = 1232;
#[cfg(not(any(feature = "small", feature = "medium")))]
pub const ENGINE_ACCOUNTS_OFFSET: usize = 17624;

// ── Tunables ────────────────────────────────────────────────────────────────

pub const TEST_MAX_PRICE_MOVE_BPS_PER_SLOT: u64 = 4;
pub const TEST_MAX_STALENESS_SECS: u64 =
    percolator_prog::constants::MAX_ORACLE_STALENESS_SECS;
pub const DEFAULT_NEW_ACCOUNT_FEE: u64 = 1;
pub const DEFAULT_INIT_PAYMENT: u64 = 100;
pub const DEFAULT_INIT_CAPITAL: u64 = DEFAULT_INIT_PAYMENT - DEFAULT_NEW_ACCOUNT_FEE;

// ── Compute-budget helper ───────────────────────────────────────────────────

const COMPUTE_BUDGET_PROGRAM: Pubkey =
    solana_sdk::pubkey!("ComputeBudget111111111111111111111111111111");

/// Direct compute-budget instruction encoding (avoids
/// `solana-compute-budget-interface` dev-dep).
pub fn cu_ix() -> Instruction {
    let mut data = vec![2u8];
    data.extend_from_slice(&1_400_000u32.to_le_bytes());
    Instruction { program_id: COMPUTE_BUDGET_PROGRAM, accounts: vec![], data }
}

// ── Program path ────────────────────────────────────────────────────────────

pub fn program_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("target/deploy/percolator_prog.so");
    assert!(p.exists(), "BPF binary not found at {:?} — run cargo build-sbf", p);
    p
}

// ── SPL Token v1 fixture builders ───────────────────────────────────────────

/// SPL Token v1 Account (165 bytes).
pub const TOKEN_ACCOUNT_LEN: usize = 165;
/// SPL Token v1 Mint (82 bytes).
pub const MINT_LEN: usize = 82;

pub fn make_token_account_data(mint: &Pubkey, owner: &Pubkey, amount: u64) -> Vec<u8> {
    let mut data = vec![0u8; TOKEN_ACCOUNT_LEN];
    data[0..32].copy_from_slice(&mint.to_bytes());
    data[32..64].copy_from_slice(&owner.to_bytes());
    data[64..72].copy_from_slice(&amount.to_le_bytes());
    // delegate COption (72..108): None
    data[108] = 1; // state = Initialized
    // is_native (109..121): None
    // delegated_amount (121..129): 0
    // close_authority (129..165): None
    data
}

pub fn make_token_account_with_delegate(
    mint: &Pubkey,
    owner: &Pubkey,
    amount: u64,
    delegate: &Pubkey,
    delegated_amount: u64,
) -> Vec<u8> {
    let mut data = make_token_account_data(mint, owner, amount);
    // delegate COption: tag(4 LE) at 72..76, payload at 76..108
    data[72..76].copy_from_slice(&1u32.to_le_bytes());
    data[76..108].copy_from_slice(&delegate.to_bytes());
    data[121..129].copy_from_slice(&delegated_amount.to_le_bytes());
    data
}

pub fn make_token_account_with_close_authority(
    mint: &Pubkey,
    owner: &Pubkey,
    amount: u64,
    close_authority: &Pubkey,
) -> Vec<u8> {
    let mut data = make_token_account_data(mint, owner, amount);
    // close_authority COption (129..165): tag at 129..133, payload at 133..165
    data[129..133].copy_from_slice(&1u32.to_le_bytes());
    data[133..165].copy_from_slice(&close_authority.to_bytes());
    data
}

pub fn make_mint_data() -> Vec<u8> {
    let mut data = vec![0u8; MINT_LEN];
    // mint_authority COption (0..36): None (tag=0)
    // supply (36..44): 0
    data[44] = 6; // decimals
    data[45] = 1; // is_initialized
    // freeze_authority COption (46..82): None
    data
}

/// PriceUpdateV2 mock — Pyth Solana Receiver v2 account (Full
/// verification level). 134 bytes.
pub fn make_pyth_data(
    feed_id: &[u8; 32],
    price: i64,
    expo: i32,
    conf: u64,
    publish_time: i64,
) -> Vec<u8> {
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

/// Slab fixture buffer — fully zeroed, length = `SLAB_LEN`. Anchor v2
/// stamps the disc on `#[account(zeroed)]` entry.
pub fn make_uninit_slab_data() -> Vec<u8> {
    vec![0u8; SLAB_LEN]
}

// ── v2 InitMarketArgs encoder ───────────────────────────────────────────────

/// Builder for `InitMarketArgs`. Defaults match the legacy
/// "non-Hyperp, perm_resolve=80, no funding overrides" fixture.
#[derive(Clone)]
pub struct InitOpts {
    pub admin: Pubkey,
    pub mint: Pubkey,
    pub feed_id: [u8; 32],
    pub max_staleness_secs: u64,
    pub conf_filter_bps: u16,
    pub invert: u8,
    pub unit_scale: u32,
    pub initial_mark_price_e6: u64,
    pub maintenance_fee_per_slot: u128,
    // RiskParams (14 fields)
    pub h_min: u64,
    pub maintenance_margin_bps: u64,
    pub initial_margin_bps: u64,
    pub trading_fee_bps: u64,
    pub max_accounts: u64,
    pub new_account_fee: u128,
    pub h_max: u64,
    pub liquidation_fee_bps: u64,
    pub liquidation_fee_cap: u128,
    pub resolve_price_deviation_bps: u64,
    pub min_liquidation_abs: u128,
    pub min_nonzero_mm_req: u128,
    pub min_nonzero_im_req: u128,
    pub max_price_move_bps_per_slot: u64,
    // Tail
    pub insurance_withdraw_max_bps: u16,
    pub insurance_withdraw_cooldown_slots: u64,
    pub permissionless_resolve_stale_slots: u64,
    pub funding_horizon_slots: Option<u64>,
    pub funding_k_bps: Option<u64>,
    pub funding_max_premium_bps: Option<i64>,
    pub funding_max_e9_per_slot: Option<i64>,
    pub mark_min_fee: u64,
    pub force_close_delay_slots: u64,
}

impl InitOpts {
    /// Default non-Hyperp fixture. Caller fills `admin` / `mint` /
    /// `feed_id`.
    pub fn default_for(admin: Pubkey, mint: Pubkey, feed_id: [u8; 32]) -> Self {
        let is_hyperp = feed_id == [0u8; 32];
        let perm_resolve: u64 = if is_hyperp { 0 } else { 80 };
        let force_close: u64 = if is_hyperp { 0 } else { 50 };
        InitOpts {
            admin,
            mint,
            feed_id,
            max_staleness_secs: TEST_MAX_STALENESS_SECS,
            conf_filter_bps: 500,
            invert: 0,
            unit_scale: 0,
            initial_mark_price_e6: 0,
            maintenance_fee_per_slot: 0,
            h_min: 1,
            maintenance_margin_bps: 500,
            initial_margin_bps: 1000,
            trading_fee_bps: 0,
            max_accounts: MAX_ACCOUNTS as u64,
            new_account_fee: 1, // anti-spam dust
            h_max: 1,
            liquidation_fee_bps: 50,
            liquidation_fee_cap: 1_000_000_000_000,
            resolve_price_deviation_bps: 100,
            min_liquidation_abs: 0,
            min_nonzero_mm_req: 21,
            min_nonzero_im_req: 22,
            max_price_move_bps_per_slot: TEST_MAX_PRICE_MOVE_BPS_PER_SLOT,
            insurance_withdraw_max_bps: 0,
            insurance_withdraw_cooldown_slots: 0,
            permissionless_resolve_stale_slots: perm_resolve,
            funding_horizon_slots: None,
            funding_k_bps: None,
            funding_max_premium_bps: None,
            funding_max_e9_per_slot: None,
            mark_min_fee: 0,
            force_close_delay_slots: force_close,
        }
    }
}

fn encode_option_u64(out: &mut Vec<u8>, v: Option<u64>) {
    match v {
        None => out.push(0),
        Some(x) => {
            out.push(1);
            out.extend_from_slice(&x.to_le_bytes());
        }
    }
}
fn encode_option_i64(out: &mut Vec<u8>, v: Option<i64>) {
    match v {
        None => out.push(0),
        Some(x) => {
            out.push(1);
            out.extend_from_slice(&x.to_le_bytes());
        }
    }
}

/// Hand-encode `[0u8] [InitMarketArgs ...]` — wincode/Borsh fixed-LE.
pub fn encode_init_market(opts: &InitOpts) -> Vec<u8> {
    let mut data = vec![0u8]; // tag 0
    data.extend_from_slice(&opts.admin.to_bytes());
    data.extend_from_slice(&opts.mint.to_bytes());
    data.extend_from_slice(&opts.feed_id);
    data.extend_from_slice(&opts.max_staleness_secs.to_le_bytes());
    data.extend_from_slice(&opts.conf_filter_bps.to_le_bytes());
    data.push(opts.invert);
    data.extend_from_slice(&opts.unit_scale.to_le_bytes());
    data.extend_from_slice(&opts.initial_mark_price_e6.to_le_bytes());
    data.extend_from_slice(&opts.maintenance_fee_per_slot.to_le_bytes());
    // RiskParamsArgs (14 fields, declaration order)
    data.extend_from_slice(&opts.h_min.to_le_bytes());
    data.extend_from_slice(&opts.maintenance_margin_bps.to_le_bytes());
    data.extend_from_slice(&opts.initial_margin_bps.to_le_bytes());
    data.extend_from_slice(&opts.trading_fee_bps.to_le_bytes());
    data.extend_from_slice(&opts.max_accounts.to_le_bytes());
    data.extend_from_slice(&opts.new_account_fee.to_le_bytes());
    data.extend_from_slice(&opts.h_max.to_le_bytes());
    data.extend_from_slice(&opts.liquidation_fee_bps.to_le_bytes());
    data.extend_from_slice(&opts.liquidation_fee_cap.to_le_bytes());
    data.extend_from_slice(&opts.resolve_price_deviation_bps.to_le_bytes());
    data.extend_from_slice(&opts.min_liquidation_abs.to_le_bytes());
    data.extend_from_slice(&opts.min_nonzero_mm_req.to_le_bytes());
    data.extend_from_slice(&opts.min_nonzero_im_req.to_le_bytes());
    data.extend_from_slice(&opts.max_price_move_bps_per_slot.to_le_bytes());
    // Tail
    data.extend_from_slice(&opts.insurance_withdraw_max_bps.to_le_bytes());
    data.extend_from_slice(&opts.insurance_withdraw_cooldown_slots.to_le_bytes());
    data.extend_from_slice(&opts.permissionless_resolve_stale_slots.to_le_bytes());
    encode_option_u64(&mut data, opts.funding_horizon_slots);
    encode_option_u64(&mut data, opts.funding_k_bps);
    encode_option_i64(&mut data, opts.funding_max_premium_bps);
    encode_option_i64(&mut data, opts.funding_max_e9_per_slot);
    data.extend_from_slice(&opts.mark_min_fee.to_le_bytes());
    data.extend_from_slice(&opts.force_close_delay_slots.to_le_bytes());
    data
}

// ── Per-instruction encoders (positional args via #[program]) ───────────────

pub fn encode_init_user(fee_payment: u64) -> Vec<u8> {
    let mut data = vec![1u8];
    data.extend_from_slice(&fee_payment.to_le_bytes());
    data
}

pub fn encode_init_lp(matcher: &Pubkey, ctx: &Pubkey, fee_payment: u64) -> Vec<u8> {
    let mut data = vec![2u8];
    data.extend_from_slice(&matcher.to_bytes());
    data.extend_from_slice(&ctx.to_bytes());
    data.extend_from_slice(&fee_payment.to_le_bytes());
    data
}

pub fn encode_deposit(user_idx: u16, amount: u64) -> Vec<u8> {
    let mut data = vec![3u8];
    data.extend_from_slice(&user_idx.to_le_bytes());
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

pub fn encode_withdraw(user_idx: u16, amount: u64) -> Vec<u8> {
    let mut data = vec![4u8];
    data.extend_from_slice(&user_idx.to_le_bytes());
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

/// Permissionless KeeperCrank with empty candidate vec.
/// Wire: `[5u8] [u16::MAX caller_idx] [u32 LE = 0 candidate count]`.
pub fn encode_crank_permissionless() -> Vec<u8> {
    let mut data = vec![5u8];
    data.extend_from_slice(&u16::MAX.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data
}

/// KeeperCrank with FullClose candidates, permissionless.
/// Wire: `[5u8] [u16::MAX] [u32 count] [(u16 idx, Option<{u8 tag, Option<u128>}>)…]`.
pub fn encode_crank_with_candidates(candidates: &[u16]) -> Vec<u8> {
    let mut data = vec![5u8];
    data.extend_from_slice(&u16::MAX.to_le_bytes());
    data.extend_from_slice(&(candidates.len() as u32).to_le_bytes());
    for &idx in candidates {
        data.extend_from_slice(&idx.to_le_bytes());
        // Some(WireLiquidationPolicy { tag: 0 (FullClose), partial_amount: None })
        data.push(1); // Option::Some
        data.push(0); // tag = FullClose
        data.push(0); // partial_amount = None
    }
    data
}

pub fn encode_trade(lp_idx: u16, user_idx: u16, size: i128) -> Vec<u8> {
    let mut data = vec![6u8];
    data.extend_from_slice(&lp_idx.to_le_bytes());
    data.extend_from_slice(&user_idx.to_le_bytes());
    data.extend_from_slice(&size.to_le_bytes());
    data
}

pub fn encode_liquidate(target_idx: u16) -> Vec<u8> {
    let mut data = vec![7u8];
    data.extend_from_slice(&target_idx.to_le_bytes());
    data
}

pub fn encode_close_account(user_idx: u16) -> Vec<u8> {
    let mut data = vec![8u8];
    data.extend_from_slice(&user_idx.to_le_bytes());
    data
}

pub fn encode_top_up_insurance(amount: u64) -> Vec<u8> {
    let mut data = vec![9u8];
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

pub fn encode_close_slab() -> Vec<u8> {
    vec![13u8]
}

pub fn encode_update_config(
    funding_horizon_slots: u64,
    funding_k_bps: u64,
    funding_max_premium_bps: i64,
    funding_max_e9_per_slot: i64,
    tvl_insurance_cap_mult: u16,
) -> Vec<u8> {
    let mut data = vec![14u8];
    data.extend_from_slice(&funding_horizon_slots.to_le_bytes());
    data.extend_from_slice(&funding_k_bps.to_le_bytes());
    data.extend_from_slice(&funding_max_premium_bps.to_le_bytes());
    data.extend_from_slice(&funding_max_e9_per_slot.to_le_bytes());
    data.extend_from_slice(&tvl_insurance_cap_mult.to_le_bytes());
    data
}

pub fn encode_push_hyperp_mark(price_e6: u64, timestamp: i64) -> Vec<u8> {
    let mut data = vec![17u8];
    data.extend_from_slice(&price_e6.to_le_bytes());
    data.extend_from_slice(&timestamp.to_le_bytes());
    data
}

pub fn encode_resolve_market(mode: u8) -> Vec<u8> {
    vec![19u8, mode]
}

pub fn encode_withdraw_insurance() -> Vec<u8> {
    vec![20u8]
}

pub fn encode_admin_force_close_account(user_idx: u16) -> Vec<u8> {
    let mut data = vec![21u8];
    data.extend_from_slice(&user_idx.to_le_bytes());
    data
}

pub fn encode_withdraw_insurance_limited(amount: u64) -> Vec<u8> {
    let mut data = vec![23u8];
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

pub fn encode_reclaim_empty_account(user_idx: u16) -> Vec<u8> {
    let mut data = vec![25u8];
    data.extend_from_slice(&user_idx.to_le_bytes());
    data
}

pub fn encode_settle_account(user_idx: u16) -> Vec<u8> {
    let mut data = vec![26u8];
    data.extend_from_slice(&user_idx.to_le_bytes());
    data
}

pub fn encode_deposit_fee_credits(user_idx: u16, amount: u64) -> Vec<u8> {
    let mut data = vec![27u8];
    data.extend_from_slice(&user_idx.to_le_bytes());
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

pub fn encode_convert_released_pnl(user_idx: u16, amount: u64) -> Vec<u8> {
    let mut data = vec![28u8];
    data.extend_from_slice(&user_idx.to_le_bytes());
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

pub fn encode_resolve_permissionless() -> Vec<u8> {
    vec![29u8]
}

pub fn encode_force_close_resolved(user_idx: u16) -> Vec<u8> {
    let mut data = vec![30u8];
    data.extend_from_slice(&user_idx.to_le_bytes());
    data
}

pub fn encode_catchup_accrue() -> Vec<u8> {
    vec![31u8]
}

// UpdateAuthority (tag 32): kind_byte + new_pubkey[32]
pub const AUTHORITY_ADMIN: u8 = 0;
pub const AUTHORITY_HYPERP_MARK: u8 = 1;
pub const AUTHORITY_INSURANCE: u8 = 2;
pub const AUTHORITY_OPERATOR: u8 = 3;

pub fn encode_update_authority(kind: u8, new_pubkey: &Pubkey) -> Vec<u8> {
    let mut data = vec![32u8];
    data.push(kind);
    data.extend_from_slice(&new_pubkey.to_bytes());
    data
}

/// Legacy back-compat shim. Original tag 12 (UpdateAdmin) was deleted;
/// route through UpdateAuthority{kind=AUTHORITY_ADMIN}.
pub fn encode_update_admin(new_admin: &Pubkey) -> Vec<u8> {
    encode_update_authority(AUTHORITY_ADMIN, new_admin)
}

// ── Convenience init_market_* shim wrappers ─────────────────────────────────
//
// The legacy file had a dozen `encode_init_market_*` variants, each
// emitting a slightly different positional layout. With v2's single
// Borsh args struct, every variant is a thin wrapper that overrides
// fields on `InitOpts::default_for(...)`. Only the variants used by
// existing tests are provided; tests can always build `InitOpts`
// directly for one-off shapes.

pub fn encode_init_market_with_invert(
    admin: &Pubkey,
    mint: &Pubkey,
    feed_id: &[u8; 32],
    invert: u8,
) -> Vec<u8> {
    let mut o = InitOpts::default_for(*admin, *mint, *feed_id);
    o.invert = invert;
    encode_init_market(&o)
}

pub fn encode_init_market_with_cap(
    admin: &Pubkey,
    mint: &Pubkey,
    feed_id: &[u8; 32],
    invert: u8,
    permissionless_resolve_stale_slots: u64,
) -> Vec<u8> {
    let mut o = InitOpts::default_for(*admin, *mint, *feed_id);
    o.invert = invert;
    o.permissionless_resolve_stale_slots = permissionless_resolve_stale_slots;
    if permissionless_resolve_stale_slots > 0 {
        o.force_close_delay_slots = 50;
    }
    encode_init_market(&o)
}

pub fn encode_init_market_hyperp(initial_mark_price_e6: u64) -> Vec<u8> {
    let admin = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let mut o = InitOpts::default_for(admin, mint, [0u8; 32]);
    o.initial_mark_price_e6 = initial_mark_price_e6;
    o.permissionless_resolve_stale_slots = 0;
    encode_init_market(&o)
}

// ── TestEnv ─────────────────────────────────────────────────────────────────

pub struct TestEnv {
    pub svm: LiteSVM,
    pub program_id: Pubkey,
    pub payer: Keypair,
    pub slab: Pubkey,
    pub mint: Pubkey,
    pub vault: Pubkey,
    pub pyth_index: Pubkey,
    pub pyth_col: Pubkey,
    pub account_count: u16,
}

impl TestEnv {
    pub fn new() -> Self {
        let path = program_path();

        let mut svm = LiteSVM::new();
        let bytes = std::fs::read(&path).expect("read program");
        svm.add_program(PROGRAM_ID, &bytes).expect("load program");

        let payer = Keypair::new();
        svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();

        let slab = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let pyth_index = Pubkey::new_unique();
        let pyth_col = Pubkey::new_unique();
        let (vault_authority, _) =
            Pubkey::find_program_address(&[b"vault", slab.as_ref()], &PROGRAM_ID);
        let vault = Pubkey::new_unique();

        // Slab — fully zero (`#[account(zeroed)]` requires disc bytes = 0
        // pre-init; Anchor stamps on entry).
        svm.set_account(
            slab,
            Account {
                lamports: 1_000_000_000,
                data: make_uninit_slab_data(),
                owner: PROGRAM_ID,
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
                data: make_token_account_data(&mint, &vault_authority, 0),
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

        TestEnv {
            svm,
            program_id: PROGRAM_ID,
            payer,
            slab,
            mint,
            vault,
            pyth_index,
            pyth_col,
            account_count: 0,
        }
    }

    /// Vault PDA derived from the slab key (matches the on-chain
    /// `cpi::derive_vault_authority` seeds).
    pub fn vault_pda(&self) -> Pubkey {
        let (pda, _) = Pubkey::find_program_address(&[b"vault", self.slab.as_ref()], &self.program_id);
        pda
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

    fn send_ix_signed_by(&mut self, ix: Instruction, signers: &[&Keypair], label: &str) {
        let payer = signers[0];
        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&payer.pubkey()),
            signers,
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .unwrap_or_else(|e| panic!("{label} failed: {:?}", e));
    }

    fn try_send_ix(&mut self, ix: Instruction, signers: &[&Keypair]) -> Result<(), String> {
        let payer = signers[0];
        let tx = Transaction::new_signed_with_payer(
            &[cu_ix(), ix],
            Some(&payer.pubkey()),
            signers,
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    // ── InitMarket variants ─────────────────────────────────────────────────

    fn init_market_with_opts(&mut self, opts: InitOpts) {
        let admin = self.payer.insecure_clone();
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
            data: encode_init_market(&opts),
        };
        self.send_ix_signed_by(ix, &[&admin], "init_market");
    }

    pub fn init_market_with_invert(&mut self, invert: u8) {
        self.init_market_with_cap(invert, 80);
    }

    pub fn init_market_with_cap(
        &mut self,
        invert: u8,
        permissionless_resolve_stale_slots: u64,
    ) {
        let mut o =
            InitOpts::default_for(self.payer.pubkey(), self.mint, TEST_FEED_ID);
        o.invert = invert;
        o.permissionless_resolve_stale_slots = permissionless_resolve_stale_slots;
        if permissionless_resolve_stale_slots > 0 {
            o.force_close_delay_slots = 50;
        }
        self.init_market_with_opts(o);
    }

    pub fn init_market_hyperp(&mut self, initial_mark_price_e6: u64) {
        // Hyperp = feed_id all-zero. Hyperp markets carry their own
        // mark; perm_resolve must be 0 (or paired with mark_min_fee).
        let mut o =
            InitOpts::default_for(self.payer.pubkey(), self.mint, [0u8; 32]);
        o.initial_mark_price_e6 = initial_mark_price_e6;
        self.init_market_with_opts(o);
    }

    pub fn init_market_with_funding(
        &mut self,
        funding_horizon_slots: u64,
        funding_k_bps: u64,
        funding_max_premium_bps: i64,
        funding_max_e9_per_slot: i64,
    ) {
        let mut o =
            InitOpts::default_for(self.payer.pubkey(), self.mint, TEST_FEED_ID);
        o.funding_horizon_slots = Some(funding_horizon_slots);
        o.funding_k_bps = Some(funding_k_bps);
        o.funding_max_premium_bps = Some(funding_max_premium_bps);
        o.funding_max_e9_per_slot = Some(funding_max_e9_per_slot);
        self.init_market_with_opts(o);
    }

    // ── Account materialization ─────────────────────────────────────────────

    pub fn init_lp(&mut self, owner: &Keypair) -> u16 {
        self.init_lp_with_fee(owner, DEFAULT_INIT_PAYMENT)
    }

    pub fn init_lp_with_fee(&mut self, owner: &Keypair, fee: u64) -> u16 {
        let idx = self.account_count;
        self.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        let ata = self.create_ata(&owner.pubkey(), fee);
        // Matcher program / context — placeholders for pure-LP tests.
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
        self.send_ix_signed_by(ix, &[owner], "init_lp");
        self.account_count += 1;
        idx
    }

    pub fn init_user(&mut self, owner: &Keypair) -> u16 {
        self.init_user_with_fee(owner, DEFAULT_INIT_PAYMENT)
    }

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
            ],
            data: encode_init_user(fee),
        };
        self.send_ix_signed_by(ix, &[owner], "init_user");
        self.account_count += 1;
        idx
    }

    pub fn try_init_user(&mut self, owner: &Keypair) -> Result<(), String> {
        self.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        let ata = self.create_ata(&owner.pubkey(), DEFAULT_INIT_PAYMENT);
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
            data: encode_init_user(DEFAULT_INIT_PAYMENT),
        };
        self.try_send_ix(ix, &[owner])
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
        self.send_ix_signed_by(ix, &[owner], "deposit");
    }

    pub fn withdraw(&mut self, owner: &Keypair, user_idx: u16, amount: u64) {
        let ata = self.create_ata(&owner.pubkey(), 0);
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new(ata, false),
                AccountMeta::new_readonly(self.vault_pda(), false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_withdraw(user_idx, amount),
        };
        self.send_ix_signed_by(ix, &[owner], "withdraw");
    }

    pub fn try_withdraw(
        &mut self,
        owner: &Keypair,
        user_idx: u16,
        amount: u64,
    ) -> Result<(), String> {
        let ata = self.create_ata(&owner.pubkey(), 0);
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new(ata, false),
                AccountMeta::new_readonly(self.vault_pda(), false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_withdraw(user_idx, amount),
        };
        self.try_send_ix(ix, &[owner])
    }

    pub fn trade(
        &mut self,
        user: &Keypair,
        lp: &Keypair,
        lp_idx: u16,
        user_idx: u16,
        size: i128,
    ) {
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
        self.send_ix_signed_by(ix, &[user, lp], "trade");
    }

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
        self.try_send_ix(ix, &[user, lp])
    }

    pub fn crank(&mut self) {
        self.crank_once();
    }

    pub fn crank_once(&mut self) {
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
        self.send_ix_signed_by(ix, &[&caller], "crank");
    }

    pub fn try_crank(&mut self) -> Result<(), String> {
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
        self.try_send_ix(ix, &[&caller])
    }

    pub fn top_up_insurance(&mut self, payer: &Keypair, amount: u64) {
        let ata = self.create_ata(&payer.pubkey(), amount);
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
            data: encode_top_up_insurance(amount),
        };
        self.send_ix_signed_by(ix, &[payer], "top_up_insurance");
    }

    pub fn try_top_up_insurance(
        &mut self,
        payer: &Keypair,
        amount: u64,
    ) -> Result<(), String> {
        let ata = self.create_ata(&payer.pubkey(), amount);
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
            data: encode_top_up_insurance(amount),
        };
        self.try_send_ix(ix, &[payer])
    }

    pub fn try_resolve_market(
        &mut self,
        admin: &Keypair,
        mode: u8,
    ) -> Result<(), String> {
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_resolve_market(mode),
        };
        self.try_send_ix(ix, &[admin])
    }

    pub fn try_settle_account(&mut self, user_idx: u16) -> Result<(), String> {
        let payer = self.payer.insecure_clone();
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_settle_account(user_idx),
        };
        self.try_send_ix(ix, &[&payer])
    }

    pub fn try_liquidate(&mut self, target_idx: u16) -> Result<(), String> {
        let caller = Keypair::new();
        self.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(self.slab, false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(self.pyth_index, false),
            ],
            data: encode_liquidate(target_idx),
        };
        self.try_send_ix(ix, &[&caller])
    }

    // ── Slot/oracle manipulation ────────────────────────────────────────────

    /// Set Clock + re-stamp Pyth oracle accounts at `(effective_slot,
    /// price_e6)` without walking through intermediate cranks.
    pub fn set_slot_and_price_raw_no_walk(&mut self, effective_slot: u64, price_e6: i64) {
        self.svm.set_sysvar(&Clock {
            slot: effective_slot,
            unix_timestamp: effective_slot as i64,
            ..Clock::default()
        });
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

    // ── Read helpers ────────────────────────────────────────────────────────

    pub fn read_insurance_balance(&self) -> u128 {
        let d = self.svm.get_account(&self.slab).unwrap().data;
        // RiskEngine.insurance_fund.balance is 16 bytes of u128 at a known
        // body offset. The legacy file's offset table tracks this; for now
        // expose it via a conservative `engine.balance` lookup using a
        // recorded BPF offset.
        const INSURANCE_OFF: usize = ENGINE_OFFSET + 736; // legacy offset
        u128::from_le_bytes(d[INSURANCE_OFF..INSURANCE_OFF + 16].try_into().unwrap())
    }

    pub fn is_market_resolved(&self) -> bool {
        let d = self.svm.get_account(&self.slab).unwrap().data;
        // RiskEngine.resolved flag offset (BPF) — legacy used 600.
        const RESOLVED_OFF: usize = ENGINE_OFFSET + 600;
        d[RESOLVED_OFF] != 0
    }
}
