//! Compile-time constants. Ported from the legacy `mod constants`.
//!
//! **R1 resolution:** the slab is wrapped as `Account<PercolatorSlab>`, so
//! Anchor v2 prepends an 8-byte account discriminator. The body
//! follows verbatim from legacy starting at `BODY_OFF = DISC_LEN = 8`.
//! All `*_OFF` constants and the body byte-window helpers in
//! `state.rs` operate on the FULL account-data slice (disc + body).
//! This is a deliberate ABI change versus the legacy native program;
//! test fixtures must prepend the SHA256("account:SlabHeader")[..8]
//! disc bytes when pre-allocating a slab via `set_account`.

use crate::risk_buffer::RiskBuffer;
use crate::state::{MarketConfig, SlabHeader};
use core::mem::{align_of, size_of};
use percolator::RiskEngine;

pub const MAGIC: u64 = 0x504552434f4c4154; // "PERCOLAT"

/// Anchor v2 account discriminator length (8-byte SHA256 prefix).
pub const DISC_LEN: usize = 8;
/// First byte of the slab body (post-disc).
pub const BODY_OFF: usize = DISC_LEN;

/// Body-internal sizes — unchanged from legacy. Body layout is
/// `[SlabHeader][MarketConfig][padding-to-engine-align][RiskEngine][RiskBuffer][GenTable]`.
pub const HEADER_LEN: usize = size_of::<SlabHeader>();
pub const CONFIG_LEN: usize = size_of::<MarketConfig>();
pub const ENGINE_ALIGN: usize = align_of::<RiskEngine>();

pub const fn align_up(x: usize, a: usize) -> usize {
    (x + (a - 1)) & !(a - 1)
}

/// Slab account-data offsets (relative to the FULL account-data buffer,
/// i.e. *after* Anchor v2's 8-byte disc prefix).
pub const HEADER_OFF: usize = BODY_OFF;
pub const CONFIG_OFF: usize = HEADER_OFF + HEADER_LEN;
pub const ENGINE_OFF: usize = align_up(CONFIG_OFF + CONFIG_LEN, ENGINE_ALIGN);
pub const ENGINE_LEN: usize = size_of::<RiskEngine>();

// RiskBuffer: 4-entry persistent cache of highest-notional accounts
pub const RISK_BUF_CAP: usize = 4;
pub const RISK_BUF_OFF: usize = ENGINE_OFF + ENGINE_LEN;
pub const RISK_BUF_LEN: usize = size_of::<RiskBuffer>();
/// Per-account materialization generation table (u64 per slot).
pub const GEN_TABLE_OFF: usize = RISK_BUF_OFF + RISK_BUF_LEN;
pub const GEN_TABLE_LEN: usize = percolator::MAX_ACCOUNTS * 8;
/// Total slab account size — INCLUDES the 8-byte Anchor v2 disc prefix.
pub const SLAB_LEN: usize = GEN_TABLE_OFF + GEN_TABLE_LEN;

// ── Wrapper budgets (unchanged from legacy) ─────────────────────────────────

pub const RISK_SCAN_WINDOW: usize = 32;
pub const CRANK_REWARD_BPS: u128 = 5_000;
pub const FEE_SWEEP_BUDGET: usize = 128;
pub const LIQ_BUDGET_PER_CRANK: u16 = 64;
pub const RR_WINDOW_PER_CRANK: u64 = 64;

const _: () = assert!(
    (LIQ_BUDGET_PER_CRANK as usize) <= FEE_SWEEP_BUDGET,
    "LIQ_BUDGET_PER_CRANK must not exceed FEE_SWEEP_BUDGET"
);
const _: () = assert!(
    (LIQ_BUDGET_PER_CRANK as u64) + RR_WINDOW_PER_CRANK
        <= percolator::MAX_TOUCHED_PER_INSTRUCTION as u64,
    "KeeperCrank Phase 1 + Phase 2 must fit engine touched-account capacity"
);

// ── Engine envelope constants (immutable per deployment) ────────────────────

pub const MAX_ACCRUAL_DT_SLOTS: u64 = 100;
pub const MAX_ABS_FUNDING_E9_PER_SLOT: u64 = 10_000;
pub const MIN_FUNDING_LIFETIME_SLOTS: u64 = 10_000_000;
pub const MATCHER_ABI_VERSION: u32 = 2;
pub const MATCHER_CONTEXT_LEN: usize = 320;
pub const MAX_MATCHER_TAIL_ACCOUNTS: usize = 32;
pub const MATCHER_CALL_TAG: u8 = 0;
pub const MATCHER_CALL_LEN: usize = 67;

pub const CRANK_NO_CALLER: u16 = u16::MAX;

pub const MAX_UNIT_SCALE: u32 = 1_000_000_000;
pub const MIN_CONF_FILTER_BPS: u16 = 50;
pub const MAX_CONF_FILTER_BPS: u16 = 1_000;
pub const MAX_ORACLE_STALENESS_SECS: u64 = 600;

pub const DEFAULT_FUNDING_HORIZON_SLOTS: u64 = 500;
pub const DEFAULT_FUNDING_K_BPS: u64 = 100;
pub const DEFAULT_FUNDING_MAX_PREMIUM_BPS: i64 = 500;
pub const DEFAULT_FUNDING_MAX_E9_PER_SLOT: i64 = 1_000;
pub const DEFAULT_MARK_EWMA_HALFLIFE_SLOTS: u64 = 100;
pub const DEFAULT_PERMISSIONLESS_RESOLVE_STALE_SLOTS: u64 = 0;
pub const MAX_FORCE_CLOSE_DELAY_SLOTS: u64 = 10_000_000;
pub const INSURANCE_WITHDRAW_DEPOSITS_ONLY_FLAG: u16 = 0x8000;
pub const INSURANCE_WITHDRAW_MAX_BPS_MASK: u16 = 0x7FFF;
