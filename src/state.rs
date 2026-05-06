//! On-chain account types + raw byte-window accessors for the slab.
//!
//! `PercolatorSlab` is the unified `#[account]` type covering the entire
//! slab body — header, market config, risk engine, risk buffer, and the
//! per-account materialization generation table. Anchor v2 wraps it as
//! `Account<PercolatorSlab>`, validating the account discriminator +
//! program owner at handler entry, and length on load.
//!
//! ## Why byte-array storage for some regions
//!
//! `Account<H>` requires `align_of::<H>() <= 8`: Solana account-data
//! buffers are 8-aligned at the entrypoint, and Anchor's `header_ptr`
//! sits at offset 8 (after the disc) — so any field whose natural
//! alignment exceeds 8 would be UB to dereference through `&H`. On the
//! host (post-Rust-1.77) `u128` aligns to 16, which makes `MarketConfig`,
//! `RiskBuffer`, and `RiskEngine` all align-16. To keep the outer
//! `PercolatorSlab` align-≤-8, those regions use byte-storage wrappers
//! (`ConfigBytes`, `RiskBufBytes`, `EngineCell`) with align-1, plus
//! typed accessors that bytemuck-cast through unaligned reads (or, for
//! the engine, runtime-validated alignment + discriminant checks).
//!
//! ## EngineCell / `MaybeUninit` rationale
//!
//! `RiskEngine` embeds three `#[repr(u8)]` enums (`MarketMode`,
//! `SideMode × 2`) whose discriminants don't cover every byte value, so
//! `RiskEngine` cannot be `bytemuck::Pod`. The `EngineCell` storage is a
//! `[MaybeUninit<u8>; ENGINE_LEN]` array — it has no validity
//! invariants at the type level, so any byte pattern is sound to
//! *store*. The `engine()` / `engine_mut()` accessors run
//! `validate_raw_discriminants` plus an alignment check before forming
//! `&RiskEngine` / `&mut RiskEngine`. The `unsafe impl Pod` on
//! `EngineCell` is sound because the cell holds opaque bytes; the
//! `RiskEngine` validity contract is enforced exclusively at the
//! accessor boundary.

#![allow(unsafe_code)]

use crate::constants::{
    CONFIG_LEN, CONFIG_OFF, DISC_LEN, ENGINE_LEN, ENGINE_OFF, GEN_TABLE_OFF, HEADER_LEN,
    HEADER_OFF, RISK_BUF_LEN, RISK_BUF_OFF, SLAB_LEN,
};
use anchor_lang_v2::prelude::*;
use bytemuck::{Pod, Zeroable};
use core::mem::{align_of, offset_of, size_of, MaybeUninit};
use percolator::RiskEngine;
use solana_program_error::ProgramError;

// ── Inner typed regions (decoded by value via bytemuck) ────────────────────

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct SlabHeader {
    pub magic: u64,
    pub version: u32,
    pub bump: u8,
    pub _padding: [u8; 3],
    pub admin: [u8; 32],
    pub _reserved: [u8; 24], // [0..8]=nonce, [8..16]=mat_counter (u64) + 8 bytes unused
    pub insurance_authority: [u8; 32],
    pub insurance_operator: [u8; 32],
}

/// Body-relative offset of `_reserved`, derived from `offset_of!`.
pub const RESERVED_OFF_BODY: usize = offset_of!(SlabHeader, _reserved);
/// Account-data-relative offset of `_reserved` (= disc + body offset).
pub const RESERVED_OFF: usize = HEADER_OFF + RESERVED_OFF_BODY;

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

// ── Byte-storage wrappers for alignment-tricky regions ─────────────────────

/// Byte-array storage for `MarketConfig`. Forces align-1, lets the
/// outer `PercolatorSlab` keep `align <= 8` (Anchor's `Account<H>`
/// invariant). `MarketConfig` itself has align-16 on host (raw `u128`
/// fields), align-8 on BPF.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct ConfigBytes {
    pub bytes: [u8; CONFIG_LEN],
}

impl ConfigBytes {
    /// Decode the stored bytes as a `MarketConfig`. Copies 384 bytes —
    /// same cost as the legacy `read_config(slab_data)`.
    #[inline]
    pub fn get(&self) -> MarketConfig {
        bytemuck::pod_read_unaligned(&self.bytes)
    }

    /// Overwrite the stored bytes from a `MarketConfig` value. Same
    /// shape as the legacy `write_config(slab_data, &cfg)`.
    #[inline]
    pub fn set(&mut self, c: &MarketConfig) {
        self.bytes.copy_from_slice(bytemuck::bytes_of(c));
    }
}

/// Byte-array storage for `RiskBuffer` (same alignment story as
/// `ConfigBytes` — `u128` entries push align to 16 on host).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct RiskBufBytes {
    pub bytes: [u8; RISK_BUF_LEN],
}

impl RiskBufBytes {
    #[inline]
    pub fn get(&self) -> crate::risk_buffer::RiskBuffer {
        bytemuck::pod_read_unaligned(&self.bytes)
    }

    #[inline]
    pub fn set(&mut self, b: &crate::risk_buffer::RiskBuffer) {
        self.bytes.copy_from_slice(bytemuck::bytes_of(b));
    }
}

/// Storage cell for `RiskEngine`. `[MaybeUninit<u8>; ENGINE_LEN]`
/// disables Rust's validity invariants at the type level — bytes can
/// hold any pattern, including the "invalid" `#[repr(u8)]` enum
/// discriminants raw account data might carry. Reads validate enum
/// discriminants + memory alignment before forming `&RiskEngine`;
/// writes go through `engine_mut`, which performs the same checks.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EngineCell {
    raw: [MaybeUninit<u8>; ENGINE_LEN],
}

// SAFETY: any byte pattern is a valid `EngineCell`. The `RiskEngine`
// validity contract (enum discriminants in range, pointer alignment
// matching `align_of::<RiskEngine>()`) is enforced in the typed
// accessors below; raw byte access through `as_bytes` / `as_bytes_mut`
// is sound for any contents.
unsafe impl Zeroable for EngineCell {}
unsafe impl Pod for EngineCell {}

const _: () = assert!(size_of::<EngineCell>() == ENGINE_LEN);
const _: () = assert!(align_of::<EngineCell>() == 1);

impl EngineCell {
    /// Validate `RiskEngine`'s `#[repr(u8)]` enum discriminant bytes
    /// AND pointer alignment, then form a `&RiskEngine`. Both checks
    /// must pass; either failure returns `InvalidAccountData`. Forming
    /// `&RiskEngine` over invalid bytes / misaligned memory is UB on
    /// any field access, so the validation is load-bearing.
    #[inline]
    pub fn engine(&self) -> core::result::Result<&RiskEngine, ProgramError> {
        let ptr = self.raw.as_ptr().cast::<u8>();
        if (ptr as usize) % align_of::<RiskEngine>() != 0 {
            return Err(ProgramError::InvalidAccountData);
        }
        // SAFETY: `self.raw` is `ENGINE_LEN` bytes of any pattern; reading
        // them as `&[u8]` is sound for any contents.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, ENGINE_LEN) };
        validate_raw_discriminants(bytes)?;
        // SAFETY: alignment validated above, discriminants validated; the
        // remaining `RiskEngine` fields are numeric / arrays of `Pod` types
        // for which any bit pattern is valid.
        Ok(unsafe { &*ptr.cast::<RiskEngine>() })
    }

    #[inline]
    pub fn engine_mut(&mut self) -> core::result::Result<&mut RiskEngine, ProgramError> {
        let ptr = self.raw.as_mut_ptr().cast::<u8>();
        if (ptr as usize) % align_of::<RiskEngine>() != 0 {
            return Err(ProgramError::InvalidAccountData);
        }
        let bytes = unsafe { core::slice::from_raw_parts(ptr, ENGINE_LEN) };
        validate_raw_discriminants(bytes)?;
        Ok(unsafe { &mut *ptr.cast::<RiskEngine>() })
    }

    /// Raw byte view; used by the legacy byte-window helpers.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.raw.as_ptr().cast::<u8>(), ENGINE_LEN) }
    }

    #[inline]
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self.raw.as_mut_ptr().cast::<u8>(), ENGINE_LEN)
        }
    }
}

#[inline]
fn validate_raw_discriminants(data: &[u8]) -> core::result::Result<(), ProgramError> {
    // SideMode: valid 0 (Normal), 1 (DrainOnly), 2 (ResetPending)
    let sm_long = data[offset_of!(RiskEngine, side_mode_long)];
    let sm_short = data[offset_of!(RiskEngine, side_mode_short)];
    if sm_long > 2 || sm_short > 2 {
        return Err(ProgramError::InvalidAccountData);
    }
    // MarketMode: valid 0 (Live), 1 (Resolved)
    let mm = data[offset_of!(RiskEngine, market_mode)];
    if mm > 1 {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(())
}

// ── The unified slab account ───────────────────────────────────────────────

/// Compile-time check: the legacy ENGINE_OFF math (which inserts
/// padding to align the engine to 16-byte boundaries) must produce no
/// padding for the current `MarketConfig` size, so the typed-field
/// layout matches the on-wire layout without an explicit `_engine_pad`
/// field. If `MarketConfig` ever changes such that this is no longer
/// the case, this assert fires and an explicit padding field needs to
/// be added between `config` and `engine`.
const _: () = {
    let pre_engine = HEADER_OFF + HEADER_LEN + CONFIG_LEN;
    assert!(
        pre_engine == ENGINE_OFF,
        "MarketConfig size changed; insert _engine_pad field in PercolatorSlab"
    );
};

#[account]
#[repr(C)]
pub struct PercolatorSlab {
    pub header: SlabHeader,
    pub config: ConfigBytes,
    pub engine: EngineCell,
    pub risk_buf: RiskBufBytes,
    pub gen_table: [u64; percolator::MAX_ACCOUNTS],
}

const _: () = assert!(
    DISC_LEN + size_of::<PercolatorSlab>() == SLAB_LEN,
    "PercolatorSlab layout drift",
);
const _: () = assert!(align_of::<PercolatorSlab>() <= 8);

// ── Raw-byte accessors for the legacy byte-window helpers ──────────────────
//
// Both helpers derive the data slice from the typed `&PercolatorSlab`
// pointer Anchor v2's `Account<PercolatorSlab>` maintains internally:
// the slab sits at offset `DISC_LEN = 8` of the data buffer, so
// subtracting that lands at the start of the buffer. `SLAB_LEN` is the
// assumed full length — `slab_shape_guard` MUST run first.
//
// Solana's runtime guarantees the data buffer's address + capacity are
// valid for the entire instruction lifetime; this is the same pattern
// Anchor v2's internal `Slab::guard_bytes_mut` uses (see
// `anchor-v2-ref/lang-v2/src/accounts/slab.rs`).

/// Mutable view of the full slab data buffer (disc + body).
///
/// SAFETY: requires `slab_shape_guard` has confirmed `data_len ==
/// SLAB_LEN`. Reading past `SLAB_LEN` would be UB.
pub fn slab_data_mut<'a>(slab: &'a mut Account<PercolatorSlab>) -> &'a mut [u8] {
    let typed: &mut PercolatorSlab = slab;
    let ptr = typed as *mut PercolatorSlab as *mut u8;
    let data_ptr = unsafe { ptr.sub(DISC_LEN) };
    unsafe { core::slice::from_raw_parts_mut(data_ptr, SLAB_LEN) }
}

/// Read-only view of the full slab data buffer (disc + body).
pub fn slab_data<'a>(slab: &'a Account<PercolatorSlab>) -> &'a [u8] {
    let typed: &PercolatorSlab = slab;
    let ptr = typed as *const PercolatorSlab as *const u8;
    let data_ptr = unsafe { ptr.sub(DISC_LEN) };
    unsafe { core::slice::from_raw_parts(data_ptr, SLAB_LEN) }
}

// ── Header / config bytewise accessors ─────────────────────────────────────

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

// ── Reserved-window helpers (request nonce + mat counter) ──────────────────

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

// ── Market flags (live in `_padding[0]`) ───────────────────────────────────

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

// ── Risk buffer + generation table ─────────────────────────────────────────

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

/// First 8 bytes of `sha256("account:PercolatorSlab")` — Anchor v2's
/// account discriminator for `PercolatorSlab`. Exposed so test fixtures
/// (and any off-chain client that pre-allocates a slab via
/// `set_account` / `system_program::create_account`) can prefix the
/// slab buffer correctly without taking a direct dependency on
/// `anchor_lang_v2`.
pub fn slab_discriminator() -> &'static [u8] {
    <PercolatorSlab as Discriminator>::DISCRIMINATOR
}

/// Legacy alias retained for test fixtures that imported the previous
/// name. Forwards to `slab_discriminator`.
#[deprecated(note = "Use `slab_discriminator()` (slab is now PercolatorSlab, not SlabHeader).")]
pub fn slab_header_discriminator() -> &'static [u8] {
    slab_discriminator()
}

// ── Compile-time layout invariants ─────────────────────────────────────────
//
// Guards the disc-prefix relationship and `_reserved` position; numeric
// sizes are derived from the field definitions.

const _: () = assert!(
    HEADER_OFF == DISC_LEN,
    "HEADER_OFF must equal Anchor v2 disc length (8)",
);
const _: () = assert!(
    CONFIG_OFF == HEADER_OFF + HEADER_LEN,
    "CONFIG_OFF must immediately follow SlabHeader",
);
const _: () = assert!(RESERVED_OFF_BODY == 48, "_reserved must sit at body offset 48");
