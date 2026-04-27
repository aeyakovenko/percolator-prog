//! On-chain account types + raw byte-window accessors for the slab body.
//!
//! Ported from the legacy `mod state`, with two adjustments for Anchor v2:
//!
//! 1. `SlabHeader` is declared via `#[account]` so the macro emits the
//!    8-byte account discriminator and `Pod`/`Zeroable` glue. The legacy
//!    `magic`/`version`/`bump` fields stay inside the body and continue
//!    to be validated by the wrapper code.
//!
//! 2. Every `read_*` / `write_*` helper takes the FULL account-data slice
//!    (including the 8-byte disc). All offsets are sourced from
//!    `crate::constants::*_OFF`, which already account for `BODY_OFF = 8`.
//!
//! `MarketConfig`, `RiskBuffer`, the embedded `RiskEngine`, and the
//! generation table remain plain `#[repr(C)]` Pod regions reached by raw
//! byte-window access (matches the legacy native-program layout exactly).

use crate::constants::{
    CONFIG_LEN, CONFIG_OFF, GEN_TABLE_OFF, HEADER_LEN, HEADER_OFF, RISK_BUF_LEN, RISK_BUF_OFF,
};
use anchor_lang_v2::prelude::*;
use bytemuck::{Pod, Zeroable};
use core::mem::offset_of;

#[account]
pub struct SlabHeader {
    pub magic: u64,
    pub version: u32,
    pub bump: u8,
    pub _padding: [u8; 3],
    pub admin: [u8; 32],
    pub _reserved: [u8; 24], // [0..8]=nonce, [8..24]=mat_counter (u64) + 8 bytes unused
    pub insurance_authority: [u8; 32],
    pub insurance_operator: [u8; 32],
}

/// Body-relative offset of `_reserved`, derived from `offset_of!`.
pub const RESERVED_OFF_BODY: usize = offset_of!(SlabHeader, _reserved);
/// Account-data-relative offset of `_reserved` (= disc + body offset).
pub const RESERVED_OFF: usize = HEADER_OFF + RESERVED_OFF_BODY;

// Compile-time guard that the field layout matches expectations
// (insurance_authority + insurance_operator sit immediately after _reserved).
const _: [(); 48] = [(); RESERVED_OFF_BODY];

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct MarketConfig {
    pub collateral_mint: [u8; 32],
    pub vault_pubkey: [u8; 32],
    pub index_feed_id: [u8; 32],
    pub max_staleness_secs: u64,
    pub conf_filter_bps: u16,
    pub vault_authority_bump: u8,
    pub invert: u8,
    pub unit_scale: u32,

    pub funding_horizon_slots: u64,
    pub funding_k_bps: u64,
    pub funding_max_premium_bps: i64,
    pub funding_max_e9_per_slot: i64,

    pub hyperp_authority: [u8; 32],
    pub hyperp_mark_e6: u64,
    pub last_oracle_publish_time: i64,
    pub last_effective_price_e6: u64,

    pub insurance_withdraw_max_bps: u16,
    pub tvl_insurance_cap_mult: u16,
    pub insurance_withdraw_deposits_only: u8,
    pub _iw_padding: [u8; 3],
    pub insurance_withdraw_cooldown_slots: u64,
    pub oracle_target_price_e6: u64,
    pub oracle_target_publish_time: i64,
    pub last_hyperp_index_slot: u64,
    pub last_mark_push_slot: u128,
    pub last_insurance_withdraw_slot: u64,
    pub insurance_withdraw_deposit_remaining: u64,

    pub mark_ewma_e6: u64,
    pub mark_ewma_last_slot: u64,
    pub mark_ewma_halflife_slots: u64,
    pub init_restart_slot: u64,

    pub permissionless_resolve_stale_slots: u64,
    pub last_good_oracle_slot: u64,

    pub maintenance_fee_per_slot: u128,
    pub fee_sweep_cursor_word: u64,
    pub fee_sweep_cursor_bit: u64,
    pub mark_min_fee: u64,
    pub force_close_delay_slots: u64,
    pub new_account_fee: u128,
}

// ── Header / config bytewise accessors ──────────────────────────────────────

pub fn read_header(data: &[u8]) -> SlabHeader {
    let mut h = SlabHeader::zeroed();
    let src = &data[HEADER_OFF..HEADER_OFF + HEADER_LEN];
    bytemuck::bytes_of_mut(&mut h).copy_from_slice(src);
    h
}

pub fn write_header(data: &mut [u8], h: &SlabHeader) {
    let src = bytemuck::bytes_of(h);
    data[HEADER_OFF..HEADER_OFF + HEADER_LEN].copy_from_slice(src);
}

pub fn read_config(data: &[u8]) -> MarketConfig {
    let mut c = MarketConfig::zeroed();
    let src = &data[CONFIG_OFF..CONFIG_OFF + CONFIG_LEN];
    bytemuck::bytes_of_mut(&mut c).copy_from_slice(src);
    c
}

pub fn write_config(data: &mut [u8], c: &MarketConfig) {
    let src = bytemuck::bytes_of(c);
    data[CONFIG_OFF..CONFIG_OFF + CONFIG_LEN].copy_from_slice(src);
}

// ── Reserved-window helpers (request nonce + mat counter) ───────────────────

pub fn read_req_nonce(data: &[u8]) -> u64 {
    u64::from_le_bytes(data[RESERVED_OFF..RESERVED_OFF + 8].try_into().unwrap())
}

pub fn write_req_nonce(data: &mut [u8], nonce: u64) {
    data[RESERVED_OFF..RESERVED_OFF + 8].copy_from_slice(&nonce.to_le_bytes());
}

pub fn read_mat_counter(data: &[u8]) -> u64 {
    u64::from_le_bytes(
        data[RESERVED_OFF + 8..RESERVED_OFF + 16]
            .try_into()
            .unwrap(),
    )
}

pub fn write_mat_counter(data: &mut [u8], counter: u64) {
    data[RESERVED_OFF + 8..RESERVED_OFF + 16].copy_from_slice(&counter.to_le_bytes());
}

/// Increment the materialization counter and return the NEW value.
/// Returns None if the counter would overflow (0 reserved as "never materialized").
pub fn next_mat_counter(data: &mut [u8]) -> Option<u64> {
    let old = read_mat_counter(data);
    let c = old.checked_add(1)?;
    write_mat_counter(data, c);
    Some(c)
}

// ── Market flags (live in `_padding[0]`) ────────────────────────────────────

/// Body-relative offset of `_padding[0]` inside `SlabHeader`.
const FLAGS_OFF_BODY: usize = 13;
/// Account-data-relative offset of the flags byte.
pub const FLAGS_OFF: usize = HEADER_OFF + FLAGS_OFF_BODY;

/// CPI-in-progress reentrancy guard (TradeCpi).
pub const FLAG_CPI_IN_PROGRESS: u8 = 1 << 2;
/// Engine has received a real oracle price (not the init sentinel).
pub const FLAG_ORACLE_INITIALIZED: u8 = 1 << 3;

pub fn read_flags(data: &[u8]) -> u8 {
    data[FLAGS_OFF]
}

pub fn write_flags(data: &mut [u8], flags: u8) {
    data[FLAGS_OFF] = flags;
}

pub fn is_cpi_in_progress(data: &[u8]) -> bool {
    read_flags(data) & FLAG_CPI_IN_PROGRESS != 0
}

pub fn set_cpi_in_progress(data: &mut [u8]) {
    write_flags(data, read_flags(data) | FLAG_CPI_IN_PROGRESS);
}

pub fn clear_cpi_in_progress(data: &mut [u8]) {
    write_flags(data, read_flags(data) & !FLAG_CPI_IN_PROGRESS);
}

pub fn is_oracle_initialized(data: &[u8]) -> bool {
    read_flags(data) & FLAG_ORACLE_INITIALIZED != 0
}

pub fn set_oracle_initialized(data: &mut [u8]) {
    write_flags(data, read_flags(data) | FLAG_ORACLE_INITIALIZED);
}

// ── Risk buffer + generation table ──────────────────────────────────────────

pub fn read_risk_buffer(data: &[u8]) -> crate::risk_buffer::RiskBuffer {
    let mut buf = crate::risk_buffer::RiskBuffer::zeroed();
    let src = &data[RISK_BUF_OFF..RISK_BUF_OFF + RISK_BUF_LEN];
    bytemuck::bytes_of_mut(&mut buf).copy_from_slice(src);
    // Sanitize against corrupted slab data:
    if buf.count as usize > crate::constants::RISK_BUF_CAP {
        buf.count = crate::constants::RISK_BUF_CAP as u8;
    }
    for i in buf.count as usize..crate::constants::RISK_BUF_CAP {
        buf.entries[i] = crate::risk_buffer::RiskEntry::zeroed();
    }
    for i in (0..buf.count as usize).rev() {
        if buf.entries[i].idx as usize >= percolator::MAX_ACCOUNTS {
            buf.remove(buf.entries[i].idx);
        }
    }
    buf.recompute_min();
    if buf.scan_cursor as usize >= percolator::MAX_ACCOUNTS {
        buf.scan_cursor = 0;
    }
    buf
}

pub fn write_risk_buffer(data: &mut [u8], buf: &crate::risk_buffer::RiskBuffer) {
    let src = bytemuck::bytes_of(buf);
    data[RISK_BUF_OFF..RISK_BUF_OFF + RISK_BUF_LEN].copy_from_slice(src);
}

/// Read per-account materialization generation (u64).
/// Returns 0 for never-materialized slots (zero-initialized slab).
pub fn read_account_generation(data: &[u8], idx: u16) -> u64 {
    let off = GEN_TABLE_OFF + (idx as usize) * 8;
    u64::from_le_bytes(data[off..off + 8].try_into().unwrap())
}

/// Write per-account materialization generation.
pub fn write_account_generation(data: &mut [u8], idx: u16, generation: u64) {
    let off = GEN_TABLE_OFF + (idx as usize) * 8;
    data[off..off + 8].copy_from_slice(&generation.to_le_bytes());
}

// ── Compile-time layout invariants ──────────────────────────────────────────
//
// Guards the migration's R1 contract: the slab BODY is byte-identical to
// the legacy native-program layout, with the 8-byte Anchor v2 account
// discriminator prepended. Numeric sizes are derived (not asserted) so a
// future field addition doesn't have to update two places — only the
// disc-prefix relationship and the reserved-window position are
// load-bearing for migration correctness.

const _: () = assert!(HEADER_OFF == 8, "HEADER_OFF must equal Anchor v2 disc length");
const _: () = assert!(
    CONFIG_OFF == HEADER_OFF + HEADER_LEN,
    "CONFIG_OFF must immediately follow SlabHeader",
);
const _: () = assert!(RESERVED_OFF_BODY == 48, "_reserved must sit at body offset 48");
