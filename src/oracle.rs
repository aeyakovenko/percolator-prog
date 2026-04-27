//! Pyth / Chainlink oracle decoders + price-clamp helpers.
//!
//! Verbatim port of the legacy `mod oracle`, with these Anchor v2
//! adjustments (mechanical, no semantic change):
//!   - `solana_program::account_info::AccountInfo` → `pinocchio::account::AccountView`.
//!   - `solana_program::program_error::ProgramError` → `solana_program_error::ProgramError`.
//!   - `solana_program::pubkey::Pubkey` → `pinocchio::address::Address`.
//!   - `solana_program::sysvar::last_restart_slot::LastRestartSlot` →
//!     `solana_sysvar::last_restart_slot::LastRestartSlot` (modular crate).

#![allow(unused_imports)]

use crate::errors::PercolatorError;
use crate::state::MarketConfig;
use pinocchio::account::AccountView;
use pinocchio::address::Address;
use solana_program_error::ProgramError;




/// Pyth Solana Receiver program ID
/// rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ
pub const PYTH_RECEIVER_PROGRAM_ID: Address = Address::new_from_array([
    0x0c, 0xb7, 0xfa, 0xbb, 0x52, 0xf7, 0xa6, 0x48, 0xbb, 0x5b, 0x31, 0x7d, 0x9a, 0x01, 0x8b,
    0x90, 0x57, 0xcb, 0x02, 0x47, 0x74, 0xfa, 0xfe, 0x01, 0xe6, 0xc4, 0xdf, 0x98, 0xcc, 0x38,
    0x58, 0x81,
]);

/// Chainlink OCR2 Store program ID
/// HEvSKofvBgfaexv23kMabbYqxasxU3mQ4ibBMEmJWHny
pub const CHAINLINK_OCR2_PROGRAM_ID: Address = Address::new_from_array([
    0xf1, 0x4b, 0xf6, 0x5a, 0xd5, 0x6b, 0xd2, 0xba, 0x71, 0x5e, 0x45, 0x74, 0x2c, 0x23, 0x1f,
    0x27, 0xd6, 0x36, 0x21, 0xcf, 0x5b, 0x77, 0x8f, 0x37, 0xc1, 0xa2, 0x48, 0x95, 0x1d, 0x17,
    0x56, 0x02,
]);

// PriceUpdateV2 account layout. PriceUpdateV2::LEN = 134 is the
// MAXIMUM allocation; the actual byte count of USED bytes depends on
// the VerificationLevel variant because Borsh-serialized enums are
// variable-size:
//
//   Partial { num_signatures: u8 } → 2 bytes (disc=0x00 + 1-byte u8)
//   Full                           → 1 byte  (disc=0x01, no payload)
//
// Full variant layout (133 used bytes + 1 trailing unused):
//   discriminator(8) + write_authority(32) + verification_level(1)
//     + PriceFeedMessage(84) + posted_slot(8) = 133
//
// Partial variant layout (134 used bytes):
//   discriminator(8) + write_authority(32) + verification_level(2)
//     + PriceFeedMessage(84) + posted_slot(8) = 134
//
// Since the wrapper REJECTS non-Full verification, it only ever
// deserializes messages whose price_message starts at byte 41 (not
// 42). The earlier constant OFF_PRICE_FEED_MESSAGE = 42 silently
// shifted every field by one byte: feed_id at bytes 42..74 is in
// fact `price_message[1..33]` of the real account — which always
// mismatches the expected feed_id and returns InvalidOracleKey.
//
// The price-message block is parsed as the canonical pythnet_sdk
// struct `pythnet_sdk::messages::PriceFeedMessage` via its
// BorshDeserialize impl. Any breaking layout change Pyth ships
// (field insertion, reordering, type change) surfaces as a
// deserialize error at runtime or a compile error here.
//
// PriceFeedMessage fields (84 bytes, in Borsh declaration order per
// pythnet-sdk 2.3.1 src/messages.rs):
//   feed_id: [u8; 32]          (+32 →  32)
//   price: i64                 (+ 8 →  40)
//   conf: u64                  (+ 8 →  48)
//   exponent: i32              (+ 4 →  52)
//   publish_time: i64          (+ 8 →  60)
//   prev_publish_time: i64     (+ 8 →  68)
//   ema_price: i64             (+ 8 →  76)
//   ema_conf: u64              (+ 8 →  84)
const PRICE_UPDATE_V2_MIN_LEN: usize = 134;
const OFF_VERIFICATION_LEVEL: usize = 40; // u8 variant discriminant
/// PriceFeedMessage starts immediately after the 1-byte Full
/// discriminator. The wrapper rejects Partial upstream; offset 41
/// is correct for every price-message the wrapper ever deserializes.
const OFF_PRICE_FEED_MESSAGE: usize = 41;
/// Anchor discriminator for `PriceUpdateV2`: sha256("account:PriceUpdateV2")[0..8].
const PYTH_PRICE_UPDATE_V2_DISCRIMINATOR: [u8; 8] =
    [0x22, 0xf1, 0x23, 0x63, 0x9d, 0x7e, 0xf4, 0xcd];
/// Pyth VerificationLevel::Full — enum tag value the Anchor
/// serializer emits for the Full variant. Anchor writes the
/// variant discriminant as one u8 followed by the variant payload
/// (empty for Full, 1 byte num_signatures for Partial). Full is
/// the second variant → tag byte = 1.
const PYTH_VERIFICATION_FULL_TAG: u8 = 1;

/// Compile-time assertion: LEN must match the upstream Pyth
/// constant (sum of 8 + 32 + 2 + 84 + 8 = 134, with 2-byte
/// verification_level budget). Pyth allocates max size regardless
/// of variant, so the account is always 134 bytes.
const _: () = assert!(PRICE_UPDATE_V2_MIN_LEN == 134);

// Chainlink OCR2 State/Aggregator account layout offsets
// Note: Different from the Transmissions ring buffer format in older docs
// Must cover the last byte the parser reads: CL_OFF_ANSWER (216) + 16
// bytes for the i128 answer = 232. The prior `224` let a truncated
// Chainlink-owned feed (length 224..231) panic on the answer slice.
const CL_MIN_LEN: usize = 232;
const CL_OFF_DECIMALS: usize = 138; // u8 - number of decimals
                                    // Skip unused: latest_round_id (143), live_length (148), live_cursor (152)
                                    // The actual price data is stored directly at tail:
const CL_OFF_TIMESTAMP: usize = 208; // u64 - unix timestamp (seconds)
const CL_OFF_ANSWER: usize = 216; // i128 - price answer

// Maximum supported exponent to prevent overflow (10^18 fits in u128)
const MAX_EXPO_ABS: i32 = 18;

/// Read price from a Pyth PriceUpdateV2 account.
///
/// Parameters:
/// - price_ai: The PriceUpdateV2 account
/// - expected_feed_id: The expected Pyth feed ID (must match account's feed_id)
/// - now_unix_ts: Current unix timestamp (from clock.unix_timestamp)
/// - max_staleness_secs: Maximum age in seconds
/// - conf_bps: Maximum confidence interval in basis points
///
/// Returns `(price_e6, publish_time)` where `publish_time` is the Pyth
/// off-chain network's timestamp for this observation. The caller is
/// expected to enforce monotonicity against any previously-accepted
/// `publish_time` — see `clamp_external_price`.
pub fn read_pyth_price_e6(
    price_ai: &AccountView,
    expected_feed_id: &[u8; 32],
    now_unix_ts: i64,
    max_staleness_secs: u64,
    conf_bps: u16,
) -> Result<(u64, i64), ProgramError> {
    use pythnet_sdk::messages::PriceFeedMessage;

    // Validate oracle owner.
    if *price_ai.owner() != PYTH_RECEIVER_PROGRAM_ID {
        return Err(ProgramError::IllegalOwner);
    }

    let data = price_ai.try_borrow()?;
    if data.len() < PRICE_UPDATE_V2_MIN_LEN {
        return Err(ProgramError::InvalidAccountData);
    }
    if data[..8] != PYTH_PRICE_UPDATE_V2_DISCRIMINATOR {
        return Err(PercolatorError::OracleInvalid.into());
    }

    // Reject partially verified Pyth updates (only Full is safe).
    if data[OFF_VERIFICATION_LEVEL] != PYTH_VERIFICATION_FULL_TAG {
        return Err(PercolatorError::OracleInvalid.into());
    }

    // Deserialize the PriceFeedMessage block via the canonical
    // pythnet-sdk struct. This replaces the prior hand-rolled
    // fixed-offset reads — any layout change in Pyth's struct
    // surfaces as a borsh deserialize error here, not silent
    // garbage. See read_price_clamped comments for the outer
    // wrapper (discriminator + write_authority + verification
    // _level) which is still pinned by offset since
    // PriceUpdateV2 lives in the Anchor-heavy receiver SDK that
    // we deliberately do not pull in as a dep.
    let msg_slice = &data[OFF_PRICE_FEED_MESSAGE..];
    let msg = <PriceFeedMessage as borsh::BorshDeserialize>::deserialize(&mut &msg_slice[..])
        .map_err(|_| PercolatorError::OracleInvalid)?;

    // Validate feed_id matches expected
    if &msg.feed_id != expected_feed_id {
        return Err(PercolatorError::InvalidOracleKey.into());
    }

    let price = msg.price;
    let conf = msg.conf;
    let expo = msg.exponent;
    let publish_time = msg.publish_time;

    if price <= 0 {
        return Err(PercolatorError::OracleInvalid.into());
    }

    // SECURITY (C3): Bound exponent to prevent overflow in pow()
    // Use explicit range check instead of abs() — i32::MIN.abs() overflows.
    if expo < -MAX_EXPO_ABS || expo > MAX_EXPO_ABS {
        return Err(PercolatorError::OracleInvalid.into());
    }

    // Staleness check
    {
        let age = now_unix_ts.saturating_sub(publish_time);
        if age < 0 || age as u64 > max_staleness_secs {
            return Err(PercolatorError::OracleStale.into());
        }
    }

    // Confidence check (0 = disabled)
    let price_u = price as u128;
    if conf_bps != 0 {
        let lhs = (conf as u128) * 10_000;
        let rhs = price_u * (conf_bps as u128);
        if lhs > rhs {
            return Err(PercolatorError::OracleConfTooWide.into());
        }
    }

    // Convert to e6 format
    let scale = expo + 6;
    let final_price_u128 = if scale >= 0 {
        let mul = 10u128.pow(scale as u32);
        price_u
            .checked_mul(mul)
            .ok_or(PercolatorError::EngineOverflow)?
    } else {
        let div = 10u128.pow((-scale) as u32);
        price_u / div
    };

    if final_price_u128 == 0 {
        return Err(PercolatorError::OracleInvalid.into());
    }
    if final_price_u128 > u64::MAX as u128 {
        return Err(PercolatorError::EngineOverflow.into());
    }

    Ok((final_price_u128 as u64, publish_time))
}

/// Read price from a Chainlink OCR2 State/Aggregator account.
///
/// Parameters:
/// - price_ai: The Chainlink aggregator account
/// - expected_feed_pubkey: The expected feed account pubkey (for validation)
/// - now_unix_ts: Current unix timestamp (from clock.unix_timestamp)
/// - max_staleness_secs: Maximum age in seconds
///
/// Returns `(price_e6, observation_timestamp)` where the timestamp is
/// the Chainlink off-chain reporters' unix timestamp for this round.
/// The caller is expected to enforce monotonicity against any
/// previously-accepted timestamp — see `clamp_external_price`.
/// Note: Chainlink doesn't have confidence intervals, so conf_bps is not used.
pub fn read_chainlink_price_e6(
    price_ai: &AccountView,
    expected_feed_pubkey: &[u8; 32],
    now_unix_ts: i64,
    max_staleness_secs: u64,
) -> Result<(u64, i64), ProgramError> {
    // Validate oracle owner.
    if *price_ai.owner() != CHAINLINK_OCR2_PROGRAM_ID {
        return Err(ProgramError::IllegalOwner);
    }

    // Validate feed pubkey matches expected
    if price_ai.address().to_bytes() != *expected_feed_pubkey {
        return Err(PercolatorError::InvalidOracleKey.into());
    }

    let data = price_ai.try_borrow()?;
    if data.len() < CL_MIN_LEN {
        return Err(ProgramError::InvalidAccountData);
    }

    // Read header fields
    let decimals = data[CL_OFF_DECIMALS];

    // Read price data directly from fixed offsets
    let timestamp = u64::from_le_bytes(
        data[CL_OFF_TIMESTAMP..CL_OFF_TIMESTAMP + 8]
            .try_into()
            .unwrap(),
    );
    // Read answer as i128 (16 bytes), but only bottom 8 bytes are typically used
    let answer =
        i128::from_le_bytes(data[CL_OFF_ANSWER..CL_OFF_ANSWER + 16].try_into().unwrap());

    if answer <= 0 {
        return Err(PercolatorError::OracleInvalid.into());
    }

    // SECURITY (C3): Bound decimals to prevent overflow in pow()
    if decimals > MAX_EXPO_ABS as u8 {
        return Err(PercolatorError::OracleInvalid.into());
    }

    // Staleness check
    {
        // Validate timestamp fits in i64 before cast (year 2262+ overflow)
        if timestamp > i64::MAX as u64 {
            return Err(PercolatorError::OracleStale.into());
        }
        let age = now_unix_ts.saturating_sub(timestamp as i64);
        if age < 0 || age as u64 > max_staleness_secs {
            return Err(PercolatorError::OracleStale.into());
        }
    }

    // Convert to e6 format
    // Chainlink decimals work like: price = answer / 10^decimals
    // We want e6, so: price_e6 = answer * 10^6 / 10^decimals = answer * 10^(6-decimals)
    let price_u = answer as u128;
    let scale = 6i32 - decimals as i32;
    let final_price_u128 = if scale >= 0 {
        let mul = 10u128.pow(scale as u32);
        price_u
            .checked_mul(mul)
            .ok_or(PercolatorError::EngineOverflow)?
    } else {
        let div = 10u128.pow((-scale) as u32);
        price_u / div
    };

    if final_price_u128 == 0 {
        return Err(PercolatorError::OracleInvalid.into());
    }
    if final_price_u128 > u64::MAX as u128 {
        return Err(PercolatorError::EngineOverflow.into());
    }

    Ok((final_price_u128 as u64, timestamp as i64))
}

/// Read oracle price for engine use, applying inversion and unit scaling if configured.
///
/// Automatically detects oracle type by account owner:
/// - PYTH_RECEIVER_PROGRAM_ID: reads Pyth PriceUpdateV2
/// - CHAINLINK_OCR2_PROGRAM_ID: reads Chainlink OCR2 Transmissions
///
/// Transformations applied in order:
/// 1. If invert != 0: inverted price = 1e12 / raw_e6
/// 2. If unit_scale > 1: scaled price = price / unit_scale
///
/// CRITICAL: The unit_scale transformation ensures oracle-derived values (entry_price,
/// mark_pnl, position_value) are in the same scale as capital (which is stored in units).
/// Without this scaling, margin checks would compare units to base tokens incorrectly.
///
/// The raw oracle is validated (staleness, confidence for Pyth) BEFORE transformations.
pub fn read_engine_price_e6(
    price_ai: &AccountView,
    expected_feed_id: &[u8; 32],
    now_unix_ts: i64,
    max_staleness_secs: u64,
    conf_bps: u16,
    invert: u8,
    unit_scale: u32,
) -> Result<(u64, i64), ProgramError> {
    // Detect oracle type by account owner and dispatch
    let (raw_price, publish_time) = if *price_ai.owner() == PYTH_RECEIVER_PROGRAM_ID {
        read_pyth_price_e6(
            price_ai,
            expected_feed_id,
            now_unix_ts,
            max_staleness_secs,
            conf_bps,
        )?
    } else if *price_ai.owner() == CHAINLINK_OCR2_PROGRAM_ID {
        // Chainlink safety: the feed pubkey check ensures only the
        // specific account stored in index_feed_id at InitMarket can be read.
        // A different Chainlink-owned account would fail the pubkey match.
        read_chainlink_price_e6(price_ai, expected_feed_id, now_unix_ts, max_staleness_secs)?
    } else {
        return Err(ProgramError::IllegalOwner);
    };

    // Step 1: Apply inversion if configured (uses policy::invert_price_e6)
    let price_after_invert = crate::policy::invert_price_e6(raw_price, invert)
        .ok_or(PercolatorError::OracleInvalid)?;

    // Step 2: Apply unit scaling if configured (uses policy::scale_price_e6)
    // This ensures oracle-derived values match capital scale (stored in units)
    let engine_price = crate::policy::scale_price_e6(price_after_invert, unit_scale)
        .ok_or(PercolatorError::OracleInvalid)?;

    // Enforce MAX_ORACLE_PRICE at ingress
    if engine_price > percolator::MAX_ORACLE_PRICE {
        return Err(PercolatorError::OracleInvalid.into());
    }
    Ok((engine_price, publish_time))
}

/// Clamp `raw_price` so it cannot move more than `max_change_e2bps` from `last_price`.
/// Units: 1_000_000 e2bps = 100%. 0 = disabled (no cap). last_price == 0 = first-time.
pub fn clamp_oracle_price(last_price: u64, raw_price: u64, max_change_bps: u64) -> u64 {
    if max_change_bps == 0 || last_price == 0 {
        return raw_price;
    }
    let max_delta_128 = (last_price as u128) * (max_change_bps as u128) / 10_000;
    let max_delta = core::cmp::min(max_delta_128, u64::MAX as u128) as u64;
    let lower = last_price.saturating_sub(max_delta);
    let upper = last_price.saturating_add(max_delta);
    raw_price.clamp(lower, upper)
}

/// Read the external (Pyth/Chainlink) oracle price.
///
/// Pyth/Chainlink is the only price source for non-Hyperp markets.
/// Any parse error (stale, wide confidence, wrong feed, malformed)
/// propagates to the caller — no authority fallback. If Pyth is
/// terminally dead, the market freezes until `permissionless_resolve
/// _stale_slots` matures and settles at `engine.last_oracle_price`
/// via the Degenerate arm of ResolveMarket / ResolvePermissionless.
pub fn read_price_clamped(
    config: &mut crate::state::MarketConfig,
    price_ai: &AccountView,
    now_unix_ts: i64,
    max_change_bps: u64,
    p_last: u64,
    price_move_dt_slots: u64,
    oi_any: bool,
) -> Result<u64, ProgramError> {
    let external = read_engine_price_e6(
        price_ai,
        &config.index_feed_id,
        now_unix_ts,
        config.max_staleness_secs,
        config.conf_filter_bps,
        config.invert,
        config.unit_scale,
    );
    clamp_external_price(
        config,
        external,
        p_last,
        max_change_bps,
        price_move_dt_slots,
        oi_any,
    )
}

/// Accept an already-parsed external observation into a target/effective split.
///
/// Fresh source observations update `oracle_target_*` with the raw signed
/// target. The returned/stored effective price is then capped from engine
/// `P_last` toward that target by `max_change_bps * price_move_dt_slots`.
/// Duplicate or older observations do not replace the target, but they may
/// continue the staircase toward the already-persisted target as time moves.
pub fn clamp_external_price(
    config: &mut crate::state::MarketConfig,
    external: Result<(u64, i64), ProgramError>,
    p_last: u64,
    max_change_bps: u64,
    price_move_dt_slots: u64,
    oi_any: bool,
) -> Result<u64, ProgramError> {
    let (ext_price, publish_time) = external?;
    if publish_time > config.oracle_target_publish_time {
        config.oracle_target_price_e6 = ext_price;
        config.oracle_target_publish_time = publish_time;
        config.last_oracle_publish_time = publish_time;
    } else if config.oracle_target_price_e6 == 0 {
        config.oracle_target_price_e6 = ext_price;
        config.oracle_target_publish_time = publish_time;
        config.last_oracle_publish_time = publish_time;
    }

    let target = config.oracle_target_price_e6;
    let anchor = if p_last != 0 {
        p_last
    } else if config.last_effective_price_e6 != 0 {
        config.last_effective_price_e6
    } else {
        target
    };
    let effective = if oi_any {
        clamp_toward_engine_dt(anchor, target, max_change_bps, price_move_dt_slots)
    } else {
        target
    };
    config.last_effective_price_e6 = effective;
    Ok(effective)
}

// =========================================================================
// Hyperp mode helpers (internal mark/index, no external oracle)
// =========================================================================

/// Check if Hyperp mode is active (internal mark/index pricing).
/// Hyperp mode is active when index_feed_id is all zeros.
#[inline]
pub fn is_hyperp_mode(config: &crate::state::MarketConfig) -> bool {
    config.index_feed_id == [0u8; 32]
}

/// Hard-timeout predicate: has the market's configured oracle been
/// stale for >= permissionless_resolve_stale_slots?
///
/// Returns false when permissionless_resolve_stale_slots == 0
/// (feature disabled — admin-only resolution).
///
/// "Liveness slot" is:
///   non-Hyperp → config.last_good_oracle_slot (advances on successful
///                external Pyth/Chainlink reads)
///   Hyperp     → config.last_mark_push_slot (advances ONLY on
///                full-weight mark observations: PushHyperpMark,
///                or a TradeCpi fill whose fee paid the mark_min
///                _fee threshold). mark_ewma_last_slot is the
///                EWMA-math clock, NOT a liveness signal —
///                partial-fee sub-threshold trades advance the
///                EWMA clock so dt stays correct for weighting,
///                but they must NOT extend market life.
///
/// Once this returns true, the market is DEAD: ResolvePermissionless
/// may be called, and every price-taking live instruction
/// (read_price_and_stamp for non-Hyperp, get_engine_oracle_price_e6
/// for Hyperp) rejects further price reads to prevent state drift
/// before terminal resolution.
pub fn permissionless_stale_matured(
    config: &crate::state::MarketConfig,
    clock_slot: u64,
) -> bool {
    // Cluster-restart gate (SIMD-0047 `LastRestartSlot` sysvar):
    // any hard-fork restart after `InitMarket` freezes the market
    // unconditionally, even when slot-based staleness is disabled.
    // Resolution flows through the Degenerate arm and settles at the
    // last cached pre-restart oracle price.
    if cluster_restarted_since_init(config) {
        return true;
    }
    if config.permissionless_resolve_stale_slots == 0 {
        return false;
    }
    let last_live_slot = if is_hyperp_mode(config) {
        config.last_mark_push_slot as u64
    } else {
        config.last_good_oracle_slot
    };
    clock_slot.saturating_sub(last_live_slot) >= config.permissionless_resolve_stale_slots
}

/// Pure comparison the on-chain path uses after reading the sysvar.
/// Separated so proof harnesses can check it symbolically without stubbing syscalls.
#[inline]
pub fn restart_detected(init_restart_slot: u64, current_last_restart_slot: u64) -> bool {
    current_last_restart_slot > init_restart_slot
}

/// On-chain restart check. Invokes `sol_get_last_restart_slot` and
/// compares against the slot captured at `InitMarket`. Returns false
/// under `cfg(kani)` so verification harnesses don't need to stub the
/// syscall — the pure comparison is proved separately via
/// `restart_detected`.
#[cfg(not(feature = "kani"))]
#[inline]
pub fn cluster_restarted_since_init(config: &crate::state::MarketConfig) -> bool {
    use solana_sysvar::last_restart_slot::LastRestartSlot;
    use solana_sysvar::Sysvar;
    match LastRestartSlot::get() {
        Ok(lrs) => restart_detected(config.init_restart_slot, lrs.last_restart_slot),
        Err(_) => false,
    }
}

#[cfg(feature = "kani")]
#[inline]
pub fn cluster_restarted_since_init(_config: &crate::state::MarketConfig) -> bool {
    false
}

/// External-oracle target/effective staircase. Unlike the Hyperp helper
/// below, this intentionally does not cap accumulated dt; the caller passes
/// the engine-relevant residual dt for the actual accrual step.
pub fn clamp_toward_engine_dt(p_last: u64, target: u64, cap_bps: u64, dt_slots: u64) -> u64 {
    if p_last == 0 || target == 0 {
        return target;
    }
    if cap_bps == 0 || dt_slots == 0 {
        return p_last;
    }

    let max_delta_u128 = (p_last as u128)
        .saturating_mul(cap_bps as u128)
        .saturating_mul(dt_slots as u128)
        / 10_000u128;
    let max_delta = core::cmp::min(max_delta_u128, u64::MAX as u128) as u64;
    if target > p_last {
        core::cmp::min(target, p_last.saturating_add(max_delta))
    } else {
        core::cmp::max(target, p_last.saturating_sub(max_delta))
    }
}

/// Move `index` toward `mark`, but clamp movement by cap_bps * dt_slots.
/// cap_bps units: standard bps (10_000 = 100%).
/// Returns the new index value.
///
/// Security: When dt_slots == 0 (same slot) or cap_bps == 0 (cap disabled),
/// returns index unchanged to prevent bypassing rate limits.
/// Maximum effective dt for rate-limiting. Caps accumulated movement to
/// prevent a crank pause from allowing a full-magnitude index jump.
/// ~1 hour at 2.5 slots/sec = 9000 slots.
const MAX_CLAMP_DT_SLOTS: u64 = 9_000;

pub fn clamp_toward_with_dt(index: u64, mark: u64, cap_bps: u64, dt_slots: u64) -> u64 {
    if index == 0 {
        return mark;
    }
    if cap_bps == 0 || dt_slots == 0 {
        return index;
    }

    // Cap dt to bound accumulated movement after crank pauses
    let capped_dt = dt_slots.min(MAX_CLAMP_DT_SLOTS);

    let max_delta_u128 = (index as u128)
        .saturating_mul(cap_bps as u128)
        .saturating_mul(capped_dt as u128)
        / 10_000u128;

    let max_delta = core::cmp::min(max_delta_u128, u64::MAX as u128) as u64;
    let lo = index.saturating_sub(max_delta);
    let hi = index.saturating_add(max_delta);
    mark.clamp(lo, hi)
}

/// Get engine oracle price (unified: external oracle vs Hyperp mode).
/// In Hyperp mode: updates index toward mark with rate limiting.
///   Mark staleness enforced via last_mark_push_slot.
/// In external mode: reads the signed Pyth/Chainlink observation directly.
pub fn get_engine_oracle_price_e6(
    engine_last_oracle_price: u64,
    price_move_dt_slots: u64,
    now_slot: u64,
    now_unix_ts: i64,
    config: &mut crate::state::MarketConfig,
    a_oracle: &AccountView,
    max_change_bps: u64,
    oi_any: bool,
) -> Result<u64, ProgramError> {
    // Strict hard-timeout gate (applies to both Hyperp and non-Hyperp):
    // once the oracle has been stale for >=
    // permissionless_resolve_stale_slots, no price read succeeds.
    // The market must be resolved before any further price-taking op.
    if permissionless_stale_matured(config, now_slot) {
        return Err(crate::errors::PercolatorError::OracleStale.into());
    }
    // Hyperp mode: index_feed_id == 0
    if is_hyperp_mode(config) {
        // Mark source: prefer trade-derived EWMA, fall back to authority push
        let mark = if config.mark_ewma_e6 > 0 {
            config.mark_ewma_e6
        } else {
            config.hyperp_mark_e6
        };
        if mark == 0 {
            return Err(crate::errors::PercolatorError::OracleInvalid.into());
        }
        // Staleness: keyed off last trade OR last authority push (whichever is newer)
        let last_update = core::cmp::max(
            config.mark_ewma_last_slot,
            config.last_mark_push_slot as u64,
        );
        let last_push = last_update;
        if last_push > 0 {
            let max_stale_slots = if config.max_staleness_secs > u64::MAX / 3 {
                u64::MAX
            } else {
                config.max_staleness_secs * 3
            };
            if now_slot.saturating_sub(last_push) > max_stale_slots {
                return Err(crate::errors::PercolatorError::OracleStale.into());
            }
        }

        // Hyperp uses the same target/effective split as external
        // oracles: mark/EWMA is the target, and the engine-fed index
        // moves from engine P_last over the residual dt that the next
        // accrue may legally consume. Do not key this off
        // last_hyperp_index_slot; repeated partial catchups must advance
        // from the engine's stored price and remaining accrual window.
        let anchor = if engine_last_oracle_price != 0 {
            engine_last_oracle_price
        } else if config.last_effective_price_e6 != 0 {
            config.last_effective_price_e6
        } else {
            mark
        };
        let new_index = if oi_any {
            clamp_toward_engine_dt(anchor, mark, max_change_bps, price_move_dt_slots)
        } else {
            mark
        };

        config.last_effective_price_e6 = new_index;
        if new_index != anchor || new_index == mark {
            config.last_hyperp_index_slot = now_slot;
        }
        return Ok(new_index);
    }

    // Non-Hyperp: source signed Pyth/Chainlink price; the engine enforces
    // the dt-scaled movement cap during accrual.
    read_price_clamped(
        config,
        a_oracle,
        now_unix_ts,
        max_change_bps,
        engine_last_oracle_price,
        price_move_dt_slots,
        oi_any,
    )
}
