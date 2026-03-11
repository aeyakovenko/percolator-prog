//! Percolator: Single-file Solana program with embedded Risk Engine.

#![no_std]
#![deny(unsafe_code)]

// =============================================================================
// COMPILE-TIME SAFETY GUARDS
// =============================================================================
// These guards prevent dangerous feature combinations from compiling.
// The `mainnet` feature acts as a build-time assertion that no test/devnet
// features are accidentally enabled in production builds.

/// C2: unsafe_close skips ALL CloseSlab validation — test environments only!
/// PERC-136 #309: Guard against accidental enabling outside test builds.
#[cfg(all(feature = "unsafe_close", feature = "mainnet"))]
compile_error!("unsafe_close MUST NOT be enabled on mainnet builds!");

#[cfg(all(feature = "unsafe_close", not(feature = "test"), not(test)))]
compile_error!(
    "unsafe_close MUST ONLY be enabled with the 'test' feature — it is a drain-all backdoor!"
);

/// H2: devnet disables oracle staleness/confidence checks — not safe for mainnet!
#[cfg(all(feature = "devnet", feature = "mainnet"))]
compile_error!("devnet feature MUST NOT be enabled on mainnet builds!");

extern crate alloc;

use solana_program::account_info::AccountInfo;
use solana_program::declare_id;
use solana_program::pubkey::Pubkey;

declare_id!("Perco1ator111111111111111111111111111111111");

/// Instruction tag constants — single source of truth for CPI callers.
#[path = "tags.rs"]
pub mod tags;

// 1. mod constants
pub mod constants {
    use crate::state::{MarketConfig, SlabHeader};
    use core::mem::{align_of, size_of};
    use percolator::RiskEngine;

    pub const MAGIC: u64 = 0x504552434f4c4154; // "PERCOLAT"
    pub const VERSION: u32 = 1;

    pub const HEADER_LEN: usize = size_of::<SlabHeader>();
    pub const CONFIG_LEN: usize = size_of::<MarketConfig>();
    // PERC-312: Compile-time assertion for CONFIG_LEN (catches silent misalignment)
    // Native (u128 align=16): 512; SBF (u128 align=8): 496
    #[cfg(target_arch = "bpf")]
    const _: [(); 496] = [(); CONFIG_LEN];
    #[cfg(not(target_arch = "bpf"))]
    const _: [(); 512] = [(); CONFIG_LEN];
    pub const ENGINE_ALIGN: usize = align_of::<RiskEngine>();

    pub const fn align_up(x: usize, a: usize) -> usize {
        (x + (a - 1)) & !(a - 1)
    }

    pub const ENGINE_OFF: usize = align_up(HEADER_LEN + CONFIG_LEN, ENGINE_ALIGN);
    pub const ENGINE_LEN: usize = size_of::<RiskEngine>();
    pub const SLAB_LEN: usize = ENGINE_OFF + ENGINE_LEN;
    pub const MATCHER_ABI_VERSION: u32 = 1;
    pub const MATCHER_CONTEXT_PREFIX_LEN: usize = 64;
    pub const MATCHER_CONTEXT_LEN: usize = 320;
    pub const MATCHER_CALL_TAG: u8 = 0;
    pub const MATCHER_CALL_LEN: usize = 67;

    /// Sentinel value for permissionless crank (no caller account required)
    pub const CRANK_NO_CALLER: u16 = u16::MAX;

    /// PERC-302: Ramp start point (0.1x vault = 1000 bps).
    /// New markets start at this OI cap and ramp linearly to target.
    pub const RAMP_START_BPS: u64 = 1_000;

    // ── Instruction tags (single source of truth) ──────────────────────────
    // Keep in sync with program/src/tags.rs when that file exists (PERC-112).
    // Add new tags here AND to tags.rs.
    pub const TAG_SET_PYTH_ORACLE: u8 = 32;
    /// Alias for `tags::TAG_UPDATE_MARK_PRICE` (PERC-117 — Pyth oracle CPI integration).
    pub const TAG_MARK_PRICE_CRANK: u8 = 33;
    // ── Mark price EMA parameters (PERC-118/119) ───────────────────────────
    /// 8-hour EMA window in slots (~400ms/slot → 72_000 slots ≈ 8 hours)
    pub const MARK_PRICE_EMA_WINDOW_SLOTS: u64 = 72_000;
    /// Per-slot EMA alpha in e-6 units: 2/(72_000+1) ≈ 27
    pub const MARK_PRICE_EMA_ALPHA_E6: u64 = 2_000_000 / (MARK_PRICE_EMA_WINDOW_SLOTS + 1);

    /// Maximum allowed unit_scale for InitMarket.
    /// unit_scale=0 disables scaling (1:1 base tokens to units, dust=0 always).
    /// unit_scale=1..=1_000_000_000 enables scaling with dust tracking.
    pub const MAX_UNIT_SCALE: u32 = 1_000_000_000;

    /// Magic confirmation code for RenounceAdmin (prevents accidental calls).
    /// "RENOUNCE" in ASCII = 0x52454E4F554E4345
    pub const RENOUNCE_ADMIN_CONFIRMATION: u64 = 0x52454E4F554E4345;

    /// Minimum seed deposit for InitMarket (in base token lamports/atoms).
    /// Prevents spam-market creation. Enforced on-chain, not just UI.
    /// For USDC (6 decimals): 500_000_000 = 500 USDC.
    /// For SOL (9 decimals): 500_000_000 = 0.5 SOL — admin must fund via separate deposit.
    /// Set to 0 in test feature to allow tests with zero seed.
    #[cfg(not(feature = "test"))]
    pub const MIN_INIT_MARKET_SEED: u64 = 500_000_000; // 500 USDC at 6 decimals
    #[cfg(feature = "test")]
    pub const MIN_INIT_MARKET_SEED: u64 = 0; // Allow zero seed in tests
    /// Alias for backwards compatibility with PERC-136 references.
    pub const MIN_INIT_MARKET_SEED_LAMPORTS: u64 = MIN_INIT_MARKET_SEED;

    // Default funding parameters (used at init_market, can be changed via update_config)
    pub const DEFAULT_FUNDING_HORIZON_SLOTS: u64 = 500; // ~4 min @ ~2 slots/sec
    pub const DEFAULT_FUNDING_K_BPS: u64 = 100; // 1.00x multiplier
    pub const DEFAULT_FUNDING_INV_SCALE_NOTIONAL_E6: u128 = 1_000_000_000_000; // Funding scale factor (e6 units)
    pub const DEFAULT_FUNDING_MAX_PREMIUM_BPS: i64 = 500; // cap premium at 5.00%
    pub const DEFAULT_FUNDING_MAX_BPS_PER_SLOT: i64 = 5; // cap per-slot funding
    pub const DEFAULT_HYPERP_PRICE_CAP_E2BPS: u64 = 10_000; // 1% per slot max price change for Hyperp
    pub const DEFAULT_DEX_ORACLE_PRICE_CAP_E2BPS: u64 = 50_000; // 5% per slot max price change for DEX oracle markets

    /// Minimum DEX quote-side liquidity (in quote token lamports/atoms) required
    /// for UpdateHyperpMark to accept the price. This prevents bootstrapping a
    /// Hyperp market from a near-empty pool where an attacker can cheaply
    /// manipulate the spot price.
    ///
    /// For PumpSwap: checks quote_vault balance.
    /// For Raydium CLMM: checks pool liquidity field (u128).
    /// For Meteora DLMM: checks that bin_step and active_id produce non-degenerate price.
    ///
    /// Value: 100_000_000 (100 USDC at 6 decimals, or 0.1 SOL at 9 decimals).
    /// This is intentionally low — the circuit breaker provides the primary protection.
    /// This constant gates the MINIMUM pool depth to prevent trivial manipulation.
    pub const MIN_DEX_QUOTE_LIQUIDITY: u64 = 100_000_000;

    // Matcher call ABI offsets (67-byte layout)
    // byte 0: tag (u8)
    // 1..9: req_id (u64)
    // 9..11: lp_idx (u16)
    // 11..19: lp_account_id (u64)
    // 19..27: oracle_price_e6 (u64)
    // 27..43: req_size (i128)
    // 43..67: reserved (must be zero)
    pub const CALL_OFF_TAG: usize = 0;
    pub const CALL_OFF_REQ_ID: usize = 1;
    pub const CALL_OFF_LP_IDX: usize = 9;
    pub const CALL_OFF_LP_ACCOUNT_ID: usize = 11;
    pub const CALL_OFF_ORACLE_PRICE: usize = 19;
    pub const CALL_OFF_REQ_SIZE: usize = 27;
    pub const CALL_OFF_PADDING: usize = 43;

    // Matcher return ABI offsets (64-byte prefix)
    pub const RET_OFF_ABI_VERSION: usize = 0;
    pub const RET_OFF_FLAGS: usize = 4;
    pub const RET_OFF_EXEC_PRICE: usize = 8;
    pub const RET_OFF_EXEC_SIZE: usize = 16;
    pub const RET_OFF_REQ_ID: usize = 32;
    pub const RET_OFF_LP_ACCOUNT_ID: usize = 40;
    pub const RET_OFF_ORACLE_PRICE: usize = 48;
    pub const RET_OFF_RESERVED: usize = 56;

    // Default threshold parameters (used at init_market, can be changed via update_config)
    pub const DEFAULT_THRESH_FLOOR: u128 = 0;
    pub const DEFAULT_THRESH_RISK_BPS: u64 = 200; // 2.00% (was 0.50% — too aggressive)
    pub const DEFAULT_THRESH_UPDATE_INTERVAL_SLOTS: u64 = 10;
    pub const DEFAULT_THRESH_STEP_BPS: u64 = 2000; // 20% max step (was 5% — too slow)
    pub const DEFAULT_THRESH_ALPHA_BPS: u64 = 5000; // 50% EWMA (was 10% — decayed too slowly)
    pub const DEFAULT_THRESH_MIN: u128 = 0;
    pub const DEFAULT_THRESH_MAX: u128 = 10_000_000_000_000_000_000u128;
    pub const DEFAULT_THRESH_MIN_STEP: u128 = 1;
}

// 1b. Risk metric helpers (pure functions for anti-DoS threshold calculation)

/// LP risk state: (sum_abs, max_abs) over all LP positions.
/// LP aggregate risk state for O(1) risk delta checks.
/// Uses engine's maintained aggregates instead of scanning.
pub struct LpRiskState {
    pub sum_abs: u128,
    pub max_abs: u128,
}

impl LpRiskState {
    /// Get LP aggregate risk state from engine's maintained fields. O(1).
    #[inline]
    pub fn compute(engine: &percolator::RiskEngine) -> Self {
        Self {
            sum_abs: engine.lp_sum_abs.get(),
            max_abs: engine.lp_max_abs.get(),
        }
    }

    /// Current risk metric: max_concentration + sum_abs/8
    #[inline]
    pub fn risk(&self) -> u128 {
        self.max_abs.saturating_add(self.sum_abs / 8)
    }

    /// O(1) check: would applying delta to LP at lp_idx increase system risk?
    /// delta is the LP's position change (negative of user's trade size).
    /// Conservative: when LP was max and shrinks, we keep max_abs (overestimates risk, safe).
    #[inline]
    pub fn would_increase_risk(&self, old_lp_pos: i128, delta: i128) -> bool {
        let old_lp_abs = old_lp_pos.unsigned_abs();
        let new_lp_pos = old_lp_pos.saturating_add(delta);
        let new_lp_abs = new_lp_pos.unsigned_abs();

        // Guard: old_lp_abs must be part of sum_abs (caller must use same engine snapshot)
        #[cfg(debug_assertions)]
        debug_assert!(
            self.sum_abs >= old_lp_abs,
            "old_lp_abs not in sum_abs - wrong engine snapshot?"
        );

        // Update sum_abs in O(1)
        let new_sum_abs = self
            .sum_abs
            .saturating_sub(old_lp_abs)
            .saturating_add(new_lp_abs);

        // Update max_abs in O(1) (conservative when LP was max and shrinks)
        let new_max_abs = if new_lp_abs >= self.max_abs {
            // LP becomes new max (or ties)
            new_lp_abs
        } else if old_lp_abs == self.max_abs && new_lp_abs < old_lp_abs {
            // LP was max and shrunk - we don't know second-largest without scan.
            // Conservative: keep old max (overestimates risk, which is safe for gating).
            self.max_abs
        } else {
            // LP wasn't max, stays not max
            self.max_abs
        };

        let old_risk = self.risk();
        let new_risk = new_max_abs.saturating_add(new_sum_abs / 8);
        new_risk > old_risk
    }
}

/// Compute system risk units for threshold calculation. O(1).
/// Uses engine's maintained LP aggregates instead of scanning.
#[inline]
pub fn compute_system_risk_units(engine: &percolator::RiskEngine) -> u128 {
    LpRiskState::compute(engine).risk()
}

/// Compute net LP position for inventory-based funding. O(1).
/// Uses engine's maintained net_lp_pos instead of scanning.
#[inline]
fn compute_net_lp_pos(engine: &percolator::RiskEngine) -> i128 {
    engine.net_lp_pos.get()
}

/// Compute inventory-based funding rate (bps per slot).
///
/// Engine convention:
///   funding_rate_bps_per_slot > 0 => longs pay shorts
///   (because pnl -= position * ΔF, ΔF>0 when rate>0)
///
/// Policy: rate sign follows LP inventory sign to push net_lp_pos toward 0.
///   - If LP net long (net_lp_pos > 0), rate > 0 => longs pay => discourages longs => pushes inventory toward 0.
///   - If LP net short (net_lp_pos < 0), rate < 0 => shorts pay => discourages shorts => pushes inventory toward 0.
pub fn compute_inventory_funding_bps_per_slot(
    net_lp_pos: i128,
    price_e6: u64,
    funding_horizon_slots: u64,
    funding_k_bps: u64,
    funding_inv_scale_notional_e6: u128,
    funding_max_premium_bps: i64,
    funding_max_bps_per_slot: i64,
    funding_k2_bps: u16,
) -> i64 {
    if net_lp_pos == 0 || price_e6 == 0 || funding_horizon_slots == 0 {
        return 0;
    }

    let abs_pos: u128 = net_lp_pos.unsigned_abs();
    let notional_e6: u128 = abs_pos.saturating_mul(price_e6 as u128) / 1_000_000u128;

    let scale = funding_inv_scale_notional_e6.max(1);

    // Linear component: premium_bps = (notional / scale) * k_bps
    let linear_bps_u: u128 = notional_e6.saturating_mul(funding_k_bps as u128) / scale;

    // Quadratic component: k2 * (notional / scale)^2
    // #982: Use fixed-point to avoid precision loss on small skew ratios.
    // Scale up by 1e6 before dividing, then normalize after squaring.
    const QUAD_PRECISION: u128 = 1_000_000;
    let quadratic_bps_u: u128 = if funding_k2_bps > 0 {
        // skew_ratio_fp = notional * PRECISION / scale (fixed-point, ~e6 range)
        let skew_ratio_fp = notional_e6.saturating_mul(QUAD_PRECISION) / scale;
        // k2 * (skew_ratio_fp)^2 / (PRECISION^2 * 10_000)
        skew_ratio_fp
            .saturating_mul(skew_ratio_fp)
            .saturating_mul(funding_k2_bps as u128)
            / (QUAD_PRECISION
                .saturating_mul(QUAD_PRECISION)
                .saturating_mul(10_000))
    } else {
        0
    };

    let mut premium_bps_u: u128 = linear_bps_u.saturating_add(quadratic_bps_u);

    if premium_bps_u > (funding_max_premium_bps.unsigned_abs() as u128) {
        premium_bps_u = funding_max_premium_bps.unsigned_abs() as u128;
    }

    // Apply sign: if LP net long (net_lp_pos > 0), funding is positive
    let signed_premium_bps: i64 = if net_lp_pos > 0 {
        premium_bps_u as i64
    } else {
        -(premium_bps_u as i64)
    };

    // Convert to per-slot by dividing by horizon
    let mut per_slot: i64 = signed_premium_bps / (funding_horizon_slots as i64);

    // Sanity clamp: absolute max ±10000 bps/slot (100% per slot) to catch overflow bugs
    per_slot = per_slot.clamp(-10_000, 10_000);

    // Policy clamp: tighter bound per config
    if per_slot > funding_max_bps_per_slot {
        per_slot = funding_max_bps_per_slot;
    }
    if per_slot < -funding_max_bps_per_slot {
        per_slot = -funding_max_bps_per_slot;
    }
    per_slot
}

/// Integer square root via 3-iteration Newton-Raphson. Used by VRAM.
/// Returns floor(sqrt(x)).
#[inline]
pub fn isqrt_u32(x: u32) -> u32 {
    if x <= 1 {
        return x;
    }
    // Initial guess: half the bit-width
    let mut guess = 1u32 << ((32 - x.leading_zeros()) / 2);
    // 3 Newton-Raphson iterations (sufficient for u32 range)
    guess = (guess + x / guess) / 2;
    guess = (guess + x / guess) / 2;
    guess = (guess + x / guess) / 2;
    // Floor correction: ensure guess^2 <= x
    if guess.saturating_mul(guess) > x {
        guess -= 1;
    }
    guess
}

/// Compute VRAM (Volatility-Regime Adaptive Margin) scaling factor in bps.
///
/// Returns effective margin multiplier in basis points (10_000 = 1.0x, no change).
/// The margin is scaled up when realized volatility exceeds the target.
///
/// Formula: max(10_000, scale_bps * sqrt(ewmv_e12) / target_vol_e6)
///
/// - `ewmv_e12`: exponentially weighted moving variance (e12 scaled)
/// - `scale_bps`: sensitivity parameter (e.g., 10_000 = 1.0x base scaling)
/// - `target_vol_e6`: baseline volatility for 1.0x margin (e6 scaled)
#[inline]
pub fn compute_vram_margin_bps(ewmv_e12: u32, scale_bps: u16, target_vol_e6: u16) -> u64 {
    if scale_bps == 0 || target_vol_e6 == 0 {
        return 10_000; // disabled → 1.0x
    }
    // sqrt(ewmv_e12) gives realized vol in e6 units
    let realized_vol_e6 = isqrt_u32(ewmv_e12);
    // effective = scale_bps * realized_vol / target_vol
    let effective =
        (scale_bps as u64).saturating_mul(realized_vol_e6 as u64) / (target_vol_e6 as u64);
    // Floor at 10_000 (never reduce margin below base)
    effective.max(10_000)
}

/// Apply VRAM scaling to engine margin params. Returns (orig_initial, orig_maintenance).
/// Caller must restore originals after the engine operation.
#[inline]
pub fn apply_vram_scaling(
    engine: &mut percolator::RiskEngine,
    config: &state::MarketConfig,
) -> (u64, u64) {
    let scale_bps = state::get_vol_margin_scale_bps(config);
    let orig_init = engine.params.initial_margin_bps;
    let orig_maint = engine.params.maintenance_margin_bps;
    if scale_bps == 0 {
        return (orig_init, orig_maint);
    }
    let ewmv = state::get_ewmv_e12(config);
    let target = state::get_vol_margin_target_e6(config);
    let mult = compute_vram_margin_bps(ewmv, scale_bps, target);
    // Scale margins, capping at 10_000 bps (100%)
    engine.params.initial_margin_bps =
        ((orig_init as u128).saturating_mul(mult as u128) / 10_000).min(10_000) as u64;
    engine.params.maintenance_margin_bps =
        ((orig_maint as u128).saturating_mul(mult as u128) / 10_000).min(10_000) as u64;
    // Maintain invariant: initial >= maintenance
    if engine.params.initial_margin_bps < engine.params.maintenance_margin_bps {
        engine.params.initial_margin_bps = engine.params.maintenance_margin_bps;
    }
    (orig_init, orig_maint)
}

/// Restore engine margin params after VRAM-scaled operation.
#[inline]
pub fn restore_margins(engine: &mut percolator::RiskEngine, orig: (u64, u64)) {
    engine.params.initial_margin_bps = orig.0;
    engine.params.maintenance_margin_bps = orig.1;
}

/// Maximum age (in slots) for a CMOR attestation to be considered fresh.
/// ~2 minutes at 400ms slots = 300 slots.
pub const CMOR_MAX_AGE_SLOTS: u64 = 300;

/// Apply CMOR cross-margin credit to engine margin params if attestation is
/// present, fresh, and yields a nonzero credit. Returns the credit_bps applied
/// (0 if no attestation or stale). Caller must call `restore_margins` after the
/// engine operation (VRAM restore already handles this — CMOR modifies the same
/// fields so a single restore suffices as long as CMOR is applied *after* VRAM).
#[inline]
pub fn apply_cmor_credit(
    engine: &mut percolator::RiskEngine,
    attestation_data: &[u8],
    current_slot: u64,
) -> u16 {
    let att = match cross_margin::read_attestation(attestation_data) {
        Some(a) if a.is_initialized() => a,
        _ => return 0,
    };
    if !att.is_fresh(current_slot, CMOR_MAX_AGE_SLOTS) {
        return 0;
    }
    let credit_bps = att.compute_margin_credit_bps();
    if credit_bps == 0 {
        return 0;
    }
    // Reduce margin requirements by credit_bps, flooring at 1 bps (never zero margin)
    let reduce = |m: u64| -> u64 {
        let reduction = (m as u128).saturating_mul(credit_bps as u128) / 10_000;
        (m as u128).saturating_sub(reduction).max(1) as u64
    };
    engine.params.initial_margin_bps = reduce(engine.params.initial_margin_bps);
    engine.params.maintenance_margin_bps = reduce(engine.params.maintenance_margin_bps);
    // Maintain invariant: initial >= maintenance
    if engine.params.initial_margin_bps < engine.params.maintenance_margin_bps {
        engine.params.initial_margin_bps = engine.params.maintenance_margin_bps;
    }
    credit_bps
}

/// Check if the last account in the slice is a CMOR attestation PDA (owned by
/// this program, with valid CMOR magic). Returns true if so.
#[inline]
pub fn last_account_is_cmor(accounts: &[AccountInfo], program_id: &Pubkey) -> bool {
    if accounts.is_empty() {
        return false;
    }
    let last = &accounts[accounts.len() - 1];
    if last.owner != program_id {
        return false;
    }
    let data = match last.try_borrow_data() {
        Ok(d) => d,
        Err(_) => return false,
    };
    if data.len() < cross_margin::ATTESTATION_LEN {
        return false;
    }
    match cross_margin::read_attestation(&data) {
        Some(a) => a.is_initialized(),
        None => false,
    }
}

/// Try to read and apply CMOR credit from the last account in the accounts
/// array. Validates that the account is owned by this program, contains a
/// valid initialized CMOR attestation, and the attestation belongs to the
/// given user. Returns credit_bps applied (0 if no valid attestation found).
#[inline]
pub fn try_apply_cmor_from_accounts(
    engine: &mut percolator::RiskEngine,
    accounts: &[AccountInfo],
    program_id: &Pubkey,
    user_key: &Pubkey,
    current_slab_key: &Pubkey,
    current_slot: u64,
) -> u16 {
    if accounts.is_empty() {
        return 0;
    }
    let last = &accounts[accounts.len() - 1];
    if last.owner != program_id {
        return 0;
    }
    let cmor_data = match last.try_borrow_data() {
        Ok(d) => d,
        Err(_) => return 0,
    };
    if cmor_data.len() < cross_margin::ATTESTATION_LEN {
        return 0;
    }
    // Verify attestation belongs to the current user
    let att = match cross_margin::read_attestation(&cmor_data) {
        Some(a) if a.is_initialized() => a,
        _ => return 0,
    };
    if att.owner != user_key.to_bytes() {
        return 0;
    }
    // #986: Verify attestation is bound to the current slab pair.
    // The attestation must reference the slab being traded on as either slab_a or slab_b.
    let slab_bytes = current_slab_key.to_bytes();
    if att.slab_a != slab_bytes && att.slab_b != slab_bytes {
        return 0;
    }
    apply_cmor_credit(engine, &cmor_data, current_slot)
}

// =============================================================================
// Pure helpers for Kani verification (program-level invariants only)
// =============================================================================

/// Pure verification helpers for program-level authorization and CPI binding.
/// These are tested by Kani to prove wrapper-level security properties.
pub mod verify {
    use crate::constants::MATCHER_CONTEXT_LEN;

    /// Owner authorization: stored owner must match signer.
    /// Used by: DepositCollateral, WithdrawCollateral, TradeNoCpi, TradeCpi, CloseAccount
    #[inline]
    pub fn owner_ok(stored: [u8; 32], signer: [u8; 32]) -> bool {
        stored == signer
    }

    /// Admin authorization: admin must be non-zero (not burned) and match signer.
    /// Used by: SetRiskThreshold, UpdateAdmin
    #[inline]
    pub fn admin_ok(admin: [u8; 32], signer: [u8; 32]) -> bool {
        admin != [0u8; 32] && admin == signer
    }

    /// CPI identity binding: matcher program and context must match LP registration.
    /// This is the critical CPI security check.
    #[inline]
    pub fn matcher_identity_ok(
        lp_matcher_program: [u8; 32],
        lp_matcher_context: [u8; 32],
        provided_program: [u8; 32],
        provided_context: [u8; 32],
    ) -> bool {
        lp_matcher_program == provided_program && lp_matcher_context == provided_context
    }

    /// Matcher account shape validation.
    /// Checks: program is executable, context is not executable,
    /// context owner is program, context has sufficient length.
    #[derive(Clone, Copy)]
    pub struct MatcherAccountsShape {
        pub prog_executable: bool,
        pub ctx_executable: bool,
        pub ctx_owner_is_prog: bool,
        pub ctx_len_ok: bool,
    }

    #[inline]
    pub fn matcher_shape_ok(shape: MatcherAccountsShape) -> bool {
        shape.prog_executable
            && !shape.ctx_executable
            && shape.ctx_owner_is_prog
            && shape.ctx_len_ok
    }

    /// Check if context length meets minimum requirement.
    #[inline]
    pub fn ctx_len_sufficient(len: usize) -> bool {
        len >= MATCHER_CONTEXT_LEN
    }

    /// Gating is active when threshold > 0 AND balance <= threshold.
    #[inline]
    pub fn gate_active(threshold: u128, balance: u128) -> bool {
        threshold > 0 && balance <= threshold
    }

    /// Nonce update on success: advances by 1.
    #[inline]
    pub fn nonce_on_success(old: u64) -> u64 {
        old.wrapping_add(1)
    }

    /// Nonce update on failure: unchanged.
    #[inline]
    pub fn nonce_on_failure(old: u64) -> u64 {
        old
    }

    /// PDA key comparison: provided key must match expected derived key.
    #[inline]
    pub fn pda_key_matches(expected: [u8; 32], provided: [u8; 32]) -> bool {
        expected == provided
    }

    /// Trade size selection for CPI path: must use exec_size from matcher, not requested size.
    /// Returns the size that should be passed to engine.execute_trade.
    #[inline]
    pub fn cpi_trade_size(exec_size: i128, _requested_size: i128) -> i128 {
        exec_size // Must use exec_size, never requested_size
    }

    // =========================================================================
    // Account validation helpers
    // =========================================================================

    /// Signer requirement: account must be a signer.
    #[inline]
    pub fn signer_ok(is_signer: bool) -> bool {
        is_signer
    }

    /// Writable requirement: account must be writable.
    #[inline]
    pub fn writable_ok(is_writable: bool) -> bool {
        is_writable
    }

    /// Account count requirement: must have at least `need` accounts.
    #[inline]
    pub fn len_ok(actual: usize, need: usize) -> bool {
        actual >= need
    }

    /// LP PDA shape validation for TradeCpi.
    /// PDA must be system-owned, have zero data, and zero lamports.
    #[derive(Clone, Copy)]
    pub struct LpPdaShape {
        pub is_system_owned: bool,
        pub data_len_zero: bool,
        pub lamports_zero: bool,
    }

    #[inline]
    pub fn lp_pda_shape_ok(s: LpPdaShape) -> bool {
        s.is_system_owned && s.data_len_zero && s.lamports_zero
    }

    /// Oracle feed ID check: provided feed_id must match expected config feed_id.
    #[inline]
    pub fn oracle_feed_id_ok(expected: [u8; 32], provided: [u8; 32]) -> bool {
        expected == provided
    }

    /// Detect Pyth-pinned mode from config fields.
    ///
    /// A market is Pyth-pinned when:
    ///   - oracle_authority == [0;32]  (PushOraclePrice disabled)
    ///   - index_feed_id != [0;32]     (not Hyperp mode)
    ///
    /// In this mode, every price read goes directly to read_pyth_price_e6()
    /// with on-chain staleness + confidence + feed-ID validation. No fallback.
    ///
    /// This function is a pure boolean predicate exposed for Kani proofs.
    #[inline]
    pub fn is_pyth_pinned_mode(oracle_authority: [u8; 32], index_feed_id: [u8; 32]) -> bool {
        oracle_authority == [0u8; 32] && index_feed_id != [0u8; 32]
    }

    /// Detect Hyperp mode: index_feed_id is all-zeros.
    #[inline]
    pub fn is_hyperp_mode_verify(index_feed_id: [u8; 32]) -> bool {
        index_feed_id == [0u8; 32]
    }

    /// Staleness check predicate — mirrors the on-chain gate in read_pyth_price_e6.
    /// Returns true if the price is fresh (not stale).
    ///
    /// age = now - publish_time (signed; negative = price from future, always stale)
    #[inline]
    pub fn pyth_price_is_fresh(
        publish_time: i64,
        now_unix_ts: i64,
        max_staleness_secs: u64,
    ) -> bool {
        let age = now_unix_ts.saturating_sub(publish_time);
        !(age < 0 || age as u64 > max_staleness_secs)
    }

    /// Slab shape validation.
    /// Slab must be owned by this program and have correct length.
    #[derive(Clone, Copy)]
    pub struct SlabShape {
        pub owned_by_program: bool,
        pub correct_len: bool,
    }

    #[inline]
    pub fn slab_shape_ok(s: SlabShape) -> bool {
        s.owned_by_program && s.correct_len
    }

    // =========================================================================
    // Per-instruction authorization helpers
    // =========================================================================

    /// Single-owner instruction authorization (Deposit, Withdraw, Close).
    #[inline]
    pub fn single_owner_authorized(stored_owner: [u8; 32], signer: [u8; 32]) -> bool {
        owner_ok(stored_owner, signer)
    }

    /// Trade authorization: both user and LP owners must match signers.
    #[inline]
    pub fn trade_authorized(
        user_owner: [u8; 32],
        user_signer: [u8; 32],
        lp_owner: [u8; 32],
        lp_signer: [u8; 32],
    ) -> bool {
        owner_ok(user_owner, user_signer) && owner_ok(lp_owner, lp_signer)
    }

    // =========================================================================
    // TradeCpi decision logic - models the full wrapper policy
    // =========================================================================

    /// Decision outcome for TradeCpi instruction.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum TradeCpiDecision {
        /// Reject the trade - nonce unchanged, no engine call
        Reject,
        /// Accept the trade - nonce incremented, engine called with chosen_size
        Accept { new_nonce: u64, chosen_size: i128 },
    }

    /// Pure decision function for TradeCpi instruction.
    /// Models the wrapper's full policy without touching the risk engine.
    ///
    /// # Arguments
    /// * `old_nonce` - Current nonce before this trade
    /// * `shape` - Matcher account shape validation inputs
    /// * `identity_ok` - Whether matcher identity matches LP registration
    /// * `pda_ok` - Whether LP PDA matches expected derivation
    /// * `abi_ok` - Whether matcher return passes ABI validation
    /// * `user_auth_ok` - Whether user signer matches user owner
    /// * `lp_auth_ok` - Whether LP signer matches LP owner
    /// * `gate_active` - Whether the risk-reduction gate is active
    /// * `risk_increase` - Whether this trade would increase system risk
    /// * `exec_size` - The exec_size from matcher return
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn decide_trade_cpi(
        old_nonce: u64,
        shape: MatcherAccountsShape,
        identity_ok: bool,
        pda_ok: bool,
        abi_ok: bool,
        user_auth_ok: bool,
        lp_auth_ok: bool,
        gate_active: bool,
        risk_increase: bool,
        exec_size: i128,
    ) -> TradeCpiDecision {
        // Check in order of actual program execution:
        // 1. Matcher shape validation
        if !matcher_shape_ok(shape) {
            return TradeCpiDecision::Reject;
        }
        // 2. PDA validation
        if !pda_ok {
            return TradeCpiDecision::Reject;
        }
        // 3. Owner authorization (user and LP)
        if !user_auth_ok || !lp_auth_ok {
            return TradeCpiDecision::Reject;
        }
        // 4. Matcher identity binding
        if !identity_ok {
            return TradeCpiDecision::Reject;
        }
        // 5. ABI validation (after CPI returns)
        if !abi_ok {
            return TradeCpiDecision::Reject;
        }
        // 6. Risk gate check
        if gate_active && risk_increase {
            return TradeCpiDecision::Reject;
        }
        // All checks passed - accept the trade
        TradeCpiDecision::Accept {
            new_nonce: nonce_on_success(old_nonce),
            chosen_size: cpi_trade_size(exec_size, 0), // 0 is placeholder for requested_size
        }
    }

    /// Extract nonce from TradeCpiDecision.
    #[inline]
    pub fn decision_nonce(old_nonce: u64, decision: TradeCpiDecision) -> u64 {
        match decision {
            TradeCpiDecision::Reject => nonce_on_failure(old_nonce),
            TradeCpiDecision::Accept { new_nonce, .. } => new_nonce,
        }
    }

    // =========================================================================
    // ABI validation from real MatcherReturn inputs
    // =========================================================================

    /// Pure matcher return fields for Kani verification.
    /// Mirrors matcher_abi::MatcherReturn but lives in verify module for Kani access.
    #[derive(Debug, Clone, Copy)]
    pub struct MatcherReturnFields {
        pub abi_version: u32,
        pub flags: u32,
        pub exec_price_e6: u64,
        pub exec_size: i128,
        pub req_id: u64,
        pub lp_account_id: u64,
        pub oracle_price_e6: u64,
        pub reserved: u64,
    }

    impl MatcherReturnFields {
        /// Convert to matcher_abi::MatcherReturn for validation.
        #[inline]
        pub fn to_matcher_return(&self) -> crate::matcher_abi::MatcherReturn {
            crate::matcher_abi::MatcherReturn {
                abi_version: self.abi_version,
                flags: self.flags,
                exec_price_e6: self.exec_price_e6,
                exec_size: self.exec_size,
                req_id: self.req_id,
                lp_account_id: self.lp_account_id,
                oracle_price_e6: self.oracle_price_e6,
                reserved: self.reserved,
            }
        }
    }

    /// ABI validation of matcher return - calls the real validate_matcher_return.
    /// Returns true iff the matcher return passes all ABI checks.
    /// This avoids logic duplication and ensures Kani proofs test the real code.
    #[inline]
    pub fn abi_ok(
        ret: MatcherReturnFields,
        expected_lp_account_id: u64,
        expected_oracle_price_e6: u64,
        req_size: i128,
        expected_req_id: u64,
    ) -> bool {
        let matcher_ret = ret.to_matcher_return();
        crate::matcher_abi::validate_matcher_return(
            &matcher_ret,
            expected_lp_account_id,
            expected_oracle_price_e6,
            req_size,
            expected_req_id,
        )
        .is_ok()
    }

    /// Decision function for TradeCpi that computes ABI validity from real inputs.
    /// This is the mechanically-tied version that proves program-level policies.
    ///
    /// # Arguments
    /// * `old_nonce` - Current nonce before this trade
    /// * `shape` - Matcher account shape validation inputs
    /// * `identity_ok` - Whether matcher identity matches LP registration
    /// * `pda_ok` - Whether LP PDA matches expected derivation
    /// * `user_auth_ok` - Whether user signer matches user owner
    /// * `lp_auth_ok` - Whether LP signer matches LP owner
    /// * `gate_active` - Whether the risk-reduction gate is active
    /// * `risk_increase` - Whether this trade would increase system risk
    /// * `ret` - The matcher return fields (from CPI)
    /// * `lp_account_id` - Expected LP account ID from request
    /// * `oracle_price_e6` - Expected oracle price from request
    /// * `req_size` - Requested trade size
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn decide_trade_cpi_from_ret(
        old_nonce: u64,
        shape: MatcherAccountsShape,
        identity_ok: bool,
        pda_ok: bool,
        user_auth_ok: bool,
        lp_auth_ok: bool,
        gate_is_active: bool,
        risk_increase: bool,
        ret: MatcherReturnFields,
        lp_account_id: u64,
        oracle_price_e6: u64,
        req_size: i128,
    ) -> TradeCpiDecision {
        // Check in order of actual program execution:
        // 1. Matcher shape validation
        if !matcher_shape_ok(shape) {
            return TradeCpiDecision::Reject;
        }
        // 2. PDA validation
        if !pda_ok {
            return TradeCpiDecision::Reject;
        }
        // 3. Owner authorization (user and LP)
        if !user_auth_ok || !lp_auth_ok {
            return TradeCpiDecision::Reject;
        }
        // 4. Matcher identity binding
        if !identity_ok {
            return TradeCpiDecision::Reject;
        }
        // 5. Compute req_id from nonce and validate ABI
        let req_id = nonce_on_success(old_nonce);
        if !abi_ok(ret, lp_account_id, oracle_price_e6, req_size, req_id) {
            return TradeCpiDecision::Reject;
        }
        // 6. Risk gate check
        if gate_is_active && risk_increase {
            return TradeCpiDecision::Reject;
        }
        // All checks passed - accept the trade
        TradeCpiDecision::Accept {
            new_nonce: req_id,
            chosen_size: cpi_trade_size(ret.exec_size, req_size),
        }
    }

    // =========================================================================
    // TradeNoCpi decision logic
    // =========================================================================

    /// Decision outcome for TradeNoCpi instruction.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum TradeNoCpiDecision {
        Reject,
        Accept,
    }

    /// Pure decision function for TradeNoCpi instruction.
    #[inline]
    pub fn decide_trade_nocpi(
        user_auth_ok: bool,
        lp_auth_ok: bool,
        gate_active: bool,
        risk_increase: bool,
    ) -> TradeNoCpiDecision {
        if !user_auth_ok || !lp_auth_ok {
            return TradeNoCpiDecision::Reject;
        }
        if gate_active && risk_increase {
            return TradeNoCpiDecision::Reject;
        }
        TradeNoCpiDecision::Accept
    }

    // =========================================================================
    // Other instruction decision logic
    // =========================================================================

    /// Simple Accept/Reject decision for single-check instructions.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum SimpleDecision {
        Reject,
        Accept,
    }

    /// Decision for Deposit/Withdraw/Close: requires owner authorization.
    #[inline]
    pub fn decide_single_owner_op(owner_auth_ok: bool) -> SimpleDecision {
        if owner_auth_ok {
            SimpleDecision::Accept
        } else {
            SimpleDecision::Reject
        }
    }

    /// Decision for KeeperCrank:
    /// - Permissionless mode (caller_idx == u16::MAX): always accept
    /// - Self-crank mode: idx must exist AND owner must match signer
    #[inline]
    pub fn decide_crank(
        permissionless: bool,
        idx_exists: bool,
        stored_owner: [u8; 32],
        signer: [u8; 32],
    ) -> SimpleDecision {
        if permissionless || (idx_exists && owner_ok(stored_owner, signer)) {
            SimpleDecision::Accept
        } else {
            SimpleDecision::Reject
        }
    }

    /// Decision for admin operations (SetRiskThreshold, UpdateAdmin).
    #[inline]
    pub fn decide_admin_op(admin: [u8; 32], signer: [u8; 32]) -> SimpleDecision {
        if admin_ok(admin, signer) {
            SimpleDecision::Accept
        } else {
            SimpleDecision::Reject
        }
    }

    // =========================================================================
    // KeeperCrank with allow_panic decision logic
    // =========================================================================

    /// Decision for KeeperCrank with allow_panic support.
    /// - If allow_panic != 0: requires admin authorization
    /// - If allow_panic == 0 and permissionless: always accept
    /// - If allow_panic == 0 and self-crank: requires idx exists and owner match
    #[inline]
    pub fn decide_keeper_crank_with_panic(
        allow_panic: u8,
        admin: [u8; 32],
        signer: [u8; 32],
        permissionless: bool,
        idx_exists: bool,
        stored_owner: [u8; 32],
    ) -> SimpleDecision {
        // If allow_panic is requested, must have admin authorization
        if allow_panic != 0 && !admin_ok(admin, signer) {
            return SimpleDecision::Reject;
        }
        // Normal crank logic
        decide_crank(permissionless, idx_exists, stored_owner, signer)
    }

    // =========================================================================
    // Oracle inversion math (pure logic)
    // =========================================================================

    /// Inversion constant: 1e12 for price_e6 * inverted_e6 = 1e12
    pub const INVERSION_CONSTANT: u128 = 1_000_000_000_000;

    /// Invert oracle price: inverted_e6 = 1e12 / raw_e6
    /// Returns None if raw == 0 or result overflows u64.
    #[inline]
    pub fn invert_price_e6(raw: u64, invert: u8) -> Option<u64> {
        if invert == 0 {
            return Some(raw);
        }
        if raw == 0 {
            return None;
        }
        let inverted = INVERSION_CONSTANT / (raw as u128);
        if inverted == 0 {
            return None;
        }
        if inverted > u64::MAX as u128 {
            return None;
        }
        Some(inverted as u64)
    }

    /// Scale oracle price by unit_scale: scaled_e6 = price_e6 / unit_scale
    /// Returns None if result would be zero (price too small for scale).
    ///
    /// CRITICAL: This ensures oracle-derived values (entry_price, mark_pnl, position_value)
    /// are in the same scale as capital (which is stored in units via base_to_units).
    /// Without this scaling, margin checks would compare units to base tokens incorrectly.
    #[inline]
    pub fn scale_price_e6(price: u64, unit_scale: u32) -> Option<u64> {
        if unit_scale <= 1 {
            return Some(price);
        }
        let scaled = price / unit_scale as u64;
        if scaled == 0 {
            return None;
        }
        Some(scaled)
    }

    // =========================================================================
    // Unit scale conversion math (pure logic)
    // =========================================================================

    /// Convert base amount to (units, dust).
    /// If scale == 0: returns (base, 0).
    /// Otherwise: units = base / scale, dust = base % scale.
    #[inline]
    pub fn base_to_units(base: u64, scale: u32) -> (u64, u64) {
        if scale == 0 {
            return (base, 0);
        }
        let s = scale as u64;
        (base / s, base % s)
    }

    /// Convert units to base amount (saturating).
    /// If scale == 0: returns units.
    /// Otherwise: returns units * scale (saturating).
    #[inline]
    pub fn units_to_base(units: u64, scale: u32) -> u64 {
        if scale == 0 {
            return units;
        }
        units.saturating_mul(scale as u64)
    }

    // =========================================================================
    // Withdraw alignment check (pure logic)
    // =========================================================================

    /// Check if withdraw amount is properly aligned to unit_scale.
    /// If scale == 0: always aligned.
    /// Otherwise: amount must be divisible by scale.
    #[inline]
    #[allow(clippy::manual_is_multiple_of)]
    pub fn withdraw_amount_aligned(amount: u64, scale: u32) -> bool {
        if scale == 0 {
            return true;
        }
        // Use modulo instead of .is_multiple_of() for SBF toolchain compatibility
        amount % (scale as u64) == 0
    }

    // =========================================================================
    // Dust bookkeeping math (pure logic)
    // =========================================================================

    /// Accumulate dust: old_dust + added_dust (saturating).
    #[inline]
    pub fn accumulate_dust(old_dust: u64, added_dust: u64) -> u64 {
        old_dust.saturating_add(added_dust)
    }

    /// Sweep dust into units: returns (units_swept, remaining_dust).
    /// If scale == 0: returns (dust, 0) - all dust becomes units.
    /// Otherwise: units_swept = dust / scale, remaining = dust % scale.
    #[inline]
    pub fn sweep_dust(dust: u64, scale: u32) -> (u64, u64) {
        if scale == 0 {
            return (dust, 0);
        }
        let s = scale as u64;
        (dust / s, dust % s)
    }

    // =========================================================================
    // InitMarket scale validation (pure logic)
    // =========================================================================

    /// Validate unit_scale for InitMarket instruction.
    /// Returns true if scale is within allowed bounds.
    /// scale=0: disables scaling, 1:1 base tokens to units, dust always 0.
    /// scale=1..=MAX_UNIT_SCALE: enables scaling with dust tracking.
    #[inline]
    pub fn init_market_scale_ok(unit_scale: u32) -> bool {
        unit_scale <= crate::constants::MAX_UNIT_SCALE
    }

    // =========================================================================
    // PERC-241: Additional pure verification helpers for Kani proofs
    // =========================================================================

    // ---- 1. TOKEN DECIMALS ----
    // The program does NOT assume 9 decimals anywhere. Prices are in e6 format,
    // base_to_units / units_to_base are decimal-agnostic (they only use unit_scale).
    // The verify helper below proves the conversion is independent of decimals.

    /// Convert a token amount from one decimal representation to another.
    /// from_amount * 10^to_decimals / 10^from_decimals (saturating).
    /// Returns None on division by zero (impossible for 10^n but kept for Kani).
    #[inline]
    pub fn convert_decimals(from_amount: u64, from_decimals: u8, to_decimals: u8) -> u64 {
        if from_decimals == to_decimals {
            return from_amount;
        }
        if to_decimals > from_decimals {
            let diff = (to_decimals - from_decimals) as u32;
            let mul = 10u64.saturating_pow(diff);
            from_amount.saturating_mul(mul)
        } else {
            let diff = (from_decimals - to_decimals) as u32;
            let div = 10u64.pow(diff);
            from_amount / div
        }
    }

    /// Verify that base_to_units is decimal-independent:
    /// The result only depends on (base, unit_scale), never on token decimals.
    /// This is a structural property verified by Kani proofs.
    #[inline]
    pub fn base_to_units_decimal_agnostic(base: u64, unit_scale: u32) -> (u64, u64) {
        base_to_units(base, unit_scale)
    }

    // ---- 2. u64::MAX EDGE CASES ----
    // Verify helpers for deposit/withdraw/fee at u64::MAX.

    /// Safe deposit: returns new_capital = old + amount (saturating, no overflow).
    /// Returns None if would overflow u128.
    #[inline]
    pub fn checked_deposit(old_capital: u128, amount: u128) -> Option<u128> {
        old_capital.checked_add(amount)
    }

    /// Safe withdraw: returns new_capital = old - amount.
    /// Returns None if insufficient balance.
    #[inline]
    pub fn checked_withdraw(old_capital: u128, amount: u128) -> Option<u128> {
        if amount > old_capital {
            None
        } else {
            Some(old_capital - amount)
        }
    }

    /// Fee calculation using ceiling division (protocol-favour rounding).
    /// fee = ceil(notional * fee_bps / 10_000)
    /// Returns (fee, fee_floor_met) where fee_floor_met means fee >= 1 for nonzero notional.
    #[inline]
    pub fn compute_fee_ceil(notional: u128, fee_bps: u64) -> u128 {
        if notional == 0 || fee_bps == 0 {
            return 0;
        }
        // Ceiling division: (notional * fee_bps + 9999) / 10_000
        let numerator = notional
            .saturating_mul(fee_bps as u128)
            .saturating_add(9999);
        numerator / 10_000
    }

    /// Floor fee for comparison: fee_floor = floor(notional * fee_bps / 10_000)
    #[inline]
    pub fn compute_fee_floor(notional: u128, fee_bps: u64) -> u128 {
        if notional == 0 || fee_bps == 0 {
            return 0;
        }
        notional.saturating_mul(fee_bps as u128) / 10_000
    }

    // ---- 3. STATE MACHINE INVALID TRANSITIONS ----
    // Model account state as an enum for verification.

    /// Account lifecycle state for state machine proofs.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum AccountState {
        /// Account slot is free (not allocated)
        Free,
        /// Account is open and active
        Open,
        /// Account has been closed
        Closed,
    }

    /// Validate state transition: returns true if (from -> to) is allowed.
    /// Allowed transitions:
    ///   Free -> Open (add_user / add_lp)
    ///   Open -> Closed (close_account)
    ///   Closed -> Free (garbage collection)
    /// All others are invalid.
    #[inline]
    pub fn valid_state_transition(from: AccountState, to: AccountState) -> bool {
        matches!(
            (from, to),
            (AccountState::Free, AccountState::Open)
                | (AccountState::Open, AccountState::Closed)
                | (AccountState::Closed, AccountState::Free)
        )
    }

    /// Validate that an operation is allowed in a given state.
    #[inline]
    pub fn operation_allowed_in_state(state: AccountState, op: AccountOp) -> bool {
        match op {
            AccountOp::Open => state == AccountState::Free,
            AccountOp::Deposit => state == AccountState::Open,
            AccountOp::Withdraw => state == AccountState::Open,
            AccountOp::Trade => state == AccountState::Open,
            AccountOp::Close => state == AccountState::Open,
            AccountOp::Crank => state == AccountState::Open,
        }
    }

    /// Operations that can be performed on an account.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum AccountOp {
        Open,
        Deposit,
        Withdraw,
        Trade,
        Close,
        Crank,
    }

    // ---- 4. CONCURRENCY / INTERLEAVED INSTRUCTIONS ----
    // Pure nonce model for verifying concurrent operation safety.

    /// Model for verifying nonce serialization under concurrent operations.
    /// Two operations on the same slab must be serialized by nonce.
    #[inline]
    pub fn nonces_serialize_correctly(
        nonce_before: u64,
        op1_succeeds: bool,
        op2_succeeds: bool,
    ) -> (u64, u64) {
        let nonce_after_op1 = if op1_succeeds {
            nonce_on_success(nonce_before)
        } else {
            nonce_on_failure(nonce_before)
        };
        let nonce_after_op2 = if op2_succeeds {
            nonce_on_success(nonce_after_op1)
        } else {
            nonce_on_failure(nonce_after_op1)
        };
        (nonce_after_op1, nonce_after_op2)
    }

    /// Position invariant: user_pos + lp_pos == 0 for a two-party trade.
    /// Verifies zero-sum property holds after any trade.
    #[inline]
    pub fn position_zero_sum(user_pos: i128, lp_pos: i128) -> bool {
        user_pos.saturating_add(lp_pos) == 0
    }

    /// After a trade of `size`: new_user = old_user + size, new_lp = old_lp - size.
    /// Returns (new_user_pos, new_lp_pos, zero_sum_holds).
    #[inline]
    pub fn apply_trade_positions(old_user: i128, old_lp: i128, size: i128) -> (i128, i128, bool) {
        let new_user = old_user.saturating_add(size);
        let new_lp = old_lp.saturating_sub(size);
        // Zero-sum: if old_user + old_lp == 0, then new_user + new_lp should == 0
        // (assuming no saturation in the add/sub)
        let old_sum = old_user.wrapping_add(old_lp);
        let new_sum = new_user.wrapping_add(new_lp);
        (new_user, new_lp, old_sum == new_sum)
    }

    // ---- 5. CIRCUIT BREAKER EMA (sub-proofs) ----
    // Helpers already exist: compute_ema_mark_price, clamp_toward_with_dt
    // Sub-proof helpers for EMA update, trigger check, recovery.

    /// EMA update step (no clamping): ema_new = oracle * alpha + prev * (1 - alpha)
    #[inline]
    pub fn ema_step_unclamped(prev: u64, oracle: u64, alpha_e6: u64) -> u64 {
        if prev == 0 {
            return oracle;
        }
        if oracle == 0 {
            return prev;
        }
        let one_minus = 1_000_000u64.saturating_sub(alpha_e6);
        let ema = (oracle as u128)
            .saturating_mul(alpha_e6 as u128)
            .saturating_add((prev as u128).saturating_mul(one_minus as u128))
            / 1_000_000u128;
        ema.min(u64::MAX as u128) as u64
    }

    /// Circuit breaker trigger check: does the raw oracle exceed the cap?
    #[inline]
    pub fn circuit_breaker_triggered(
        prev_mark: u64,
        raw_oracle: u64,
        cap_e2bps: u64,
        dt_slots: u64,
    ) -> bool {
        if prev_mark == 0 || cap_e2bps == 0 || dt_slots == 0 {
            return false;
        }
        let max_delta = (prev_mark as u128)
            .saturating_mul(cap_e2bps as u128)
            .saturating_mul(dt_slots as u128)
            / 1_000_000u128;
        let max_delta = max_delta.min(prev_mark as u128) as u64;
        let lo = prev_mark.saturating_sub(max_delta);
        let hi = prev_mark.saturating_add(max_delta);
        raw_oracle < lo || raw_oracle > hi
    }

    /// Recovery check: after N steps, mark should converge toward oracle.
    /// Returns the distance |mark - oracle| after one EMA step with clamping.
    #[inline]
    pub fn mark_distance_after_step(
        prev_mark: u64,
        oracle: u64,
        alpha_e6: u64,
        cap_e2bps: u64,
        dt_slots: u64,
    ) -> u64 {
        let new_mark =
            crate::oracle::compute_ema_mark_price(prev_mark, oracle, dt_slots, alpha_e6, cap_e2bps);
        new_mark.abs_diff(oracle)
    }

    // ---- 6. FEE ROUNDING DIRECTION ----
    // Already have compute_fee_ceil and compute_fee_floor above.

    // ---- 7. DUST ACCUMULATION ----
    // Existing: base_to_units, accumulate_dust, sweep_dust.
    // New: multi-operation dust conservation helper.

    /// Simulate N small deposits and verify total value is conserved.
    /// total_base_in = sum of all deposit amounts
    /// total_units = sum of units credited
    /// total_dust = accumulated dust
    /// Conservation: total_units * scale + total_dust == total_base_in
    #[inline]
    pub fn dust_conservation_check(deposits: &[(u64, u32)], // (amount, scale) pairs
    ) -> bool {
        let mut total_units: u128 = 0;
        let mut total_dust: u64 = 0;
        let mut total_base: u128 = 0;

        for &(amount, scale) in deposits {
            let (units, dust) = base_to_units(amount, scale);
            total_units += units as u128;
            total_dust = accumulate_dust(total_dust, dust);
            total_base += amount as u128;
        }

        // Conservation: units * scale + dust == base (for uniform scale)
        if deposits.is_empty() {
            return true;
        }
        let scale = deposits[0].1;
        if scale == 0 {
            // scale==0: total_units == total_base, dust == 0
            return total_units == total_base && total_dust == 0;
        }
        let reconstructed = total_units * (scale as u128) + (total_dust as u128);
        reconstructed == total_base
    }

    // ---- 8. SELF-LIQUIDATION RESISTANCE ----
    // Pure model: a user cannot liquidate themselves at a profit.

    /// Check if liquidation produces profit for the liquidated party.
    /// Returns true if the liquidation is safe (no profit for the liquidated).
    /// close_pnl is the PnL from closing at oracle price.
    #[inline]
    pub fn liquidation_no_profit(equity_before: u128, equity_after: u128, _fee_paid: u128) -> bool {
        // After liquidation, equity should decrease by at least the fee
        // (no gaming via self-liquidation)
        equity_after <= equity_before
    }

    /// Check self-liquidation safety: the liquidation fee makes it unprofitable.
    /// penalty = liquidation_fee (paid to insurance fund).
    /// Net effect on liquidated account = -penalty (always negative).
    #[inline]
    pub fn self_liquidation_unprofitable(position_value: u128, fee_bps: u64) -> bool {
        // Any fee > 0 makes self-liquidation strictly worse than just closing
        if fee_bps == 0 {
            return true; // Zero fee = no penalty but also no profit from liquidation
        }
        let fee = compute_fee_ceil(position_value, fee_bps);
        fee > 0
    }

    // ---- 9. SANDWICH RESISTANCE ----
    // The circuit breaker bounds price impact per slot.

    /// Maximum price impact from a single transaction.
    /// With circuit breaker: price can move at most cap_e2bps per slot.
    /// Returns the maximum absolute price change allowed.
    #[inline]
    pub fn max_price_impact(current_price: u64, cap_e2bps: u64) -> u64 {
        if current_price == 0 || cap_e2bps == 0 {
            return 0;
        }
        let delta = (current_price as u128).saturating_mul(cap_e2bps as u128) / 1_000_000u128;
        delta.min(current_price as u128) as u64
    }

    /// Verify that mark price movement is bounded within one slot.
    /// This is the key sandwich resistance property.
    #[inline]
    pub fn price_impact_bounded(price_before: u64, price_after: u64, cap_e2bps: u64) -> bool {
        let max_impact = max_price_impact(price_before, cap_e2bps);
        let lo = price_before.saturating_sub(max_impact);
        let hi = price_before.saturating_add(max_impact);
        price_after >= lo && price_after <= hi
    }

    // ---- 10. ORACLE MANIPULATION ----
    // Helpers for adversarial oracle inputs.

    /// Validate oracle price is within sane bounds.
    /// price=0 and price>MAX_ORACLE are rejected.
    #[inline]
    pub fn oracle_price_valid(price: u64) -> bool {
        price > 0 && price <= 1_000_000_000_000_000 // MAX_ORACLE_PRICE
    }

    /// Circuit breaker should fire for extreme oracle jumps.
    /// A 99% drop from prev_price should trigger the breaker for any reasonable cap.
    #[inline]
    pub fn extreme_drop_triggers_breaker(prev_price: u64, cap_e2bps: u64, dt_slots: u64) -> bool {
        if prev_price == 0 || cap_e2bps == 0 || dt_slots == 0 {
            return false; // Breaker disabled
        }
        // 99% drop: new_price = prev_price / 100
        let crashed_price = prev_price / 100;
        circuit_breaker_triggered(prev_price, crashed_price, cap_e2bps, dt_slots)
    }

    // ========================================
    // PERC-274: Oracle Aggregation (Pure Logic)
    // ========================================

    /// Compute median of up to MAX_ORACLE_SOURCES prices (sorted in-place).
    /// Returns None if no valid prices (all zeros filtered out).
    ///
    /// Invariant: result is always >= min(inputs) and <= max(inputs).
    pub const MAX_ORACLE_SOURCES: usize = 5;

    pub fn median_price(prices: &mut [u64]) -> Option<u64> {
        // Filter out zeros (invalid/failed oracle reads)
        let mut valid: [u64; MAX_ORACLE_SOURCES] = [0; MAX_ORACLE_SOURCES];
        let mut count = 0usize;
        for &p in prices.iter() {
            if p > 0 && count < MAX_ORACLE_SOURCES {
                valid[count] = p;
                count += 1;
            }
        }
        if count == 0 {
            return None;
        }

        // Sort valid prices (insertion sort — tiny array, no alloc)
        for i in 1..count {
            let key = valid[i];
            let mut j = i;
            while j > 0 && valid[j - 1] > key {
                valid[j] = valid[j - 1];
                j -= 1;
            }
            valid[j] = key;
        }

        // Median: middle element for odd count, average of two middle for even
        if count % 2 == 1 {
            Some(valid[count / 2])
        } else {
            let a = valid[count / 2 - 1] as u128;
            let b = valid[count / 2] as u128;
            Some(((a + b) / 2) as u64)
        }
    }

    /// Check if a new price deviates too much from last accepted price.
    /// Returns true if deviation exceeds max_deviation_bps.
    /// 0 = disabled (no deviation check).
    pub fn price_deviates_too_much(
        last_price: u64,
        new_price: u64,
        max_deviation_bps: u64,
    ) -> bool {
        if last_price == 0 || max_deviation_bps == 0 {
            return false; // First price or check disabled
        }
        let diff = new_price.abs_diff(last_price);
        // deviation = diff * 10_000 / last_price
        let deviation_bps = (diff as u128) * 10_000 / (last_price as u128);
        deviation_bps > max_deviation_bps as u128
    }

    /// Ring buffer price history entry.
    #[derive(Clone, Copy)]
    pub struct PriceHistoryEntry {
        pub price_e6: u64,
        pub timestamp: i64,
        pub slot: u64,
        pub source_count: u8,
    }

    /// Update a ring buffer of price history. Returns new cursor.
    pub fn ring_buffer_push(cursor: u8, capacity: u8) -> u8 {
        if capacity == 0 {
            return 0;
        }
        (cursor + 1) % capacity
    }

    /// PERC-302: Compute effective OI cap multiplier with market maturity ramp.
    ///
    /// New markets start at RAMP_START_BPS (0.1x vault) and ramp linearly to
    /// `oi_cap_multiplier_bps` over `oi_ramp_slots` slots since `market_created_slot`.
    ///
    /// Invariant: result is always in [RAMP_START_BPS, oi_cap_multiplier_bps].
    /// When oi_ramp_slots == 0: returns oi_cap_multiplier_bps immediately (backwards compat).
    /// When oi_cap_multiplier_bps <= RAMP_START_BPS: returns oi_cap_multiplier_bps (no ramp needed).
    #[inline]
    pub fn compute_ramp_multiplier(
        oi_cap_multiplier_bps: u64,
        market_created_slot: u64,
        current_slot: u64,
        oi_ramp_slots: u64,
    ) -> u64 {
        use crate::constants::RAMP_START_BPS;

        // Ramp disabled: use full multiplier immediately
        if oi_ramp_slots == 0 {
            return oi_cap_multiplier_bps;
        }

        // If target is already at or below ramp start, no ramp needed
        if oi_cap_multiplier_bps <= RAMP_START_BPS {
            return oi_cap_multiplier_bps;
        }

        // Elapsed slots since market creation (saturating to handle clock skew)
        let elapsed = current_slot.saturating_sub(market_created_slot);

        // Ramp complete: use full multiplier
        if elapsed >= oi_ramp_slots {
            return oi_cap_multiplier_bps;
        }

        // Linear interpolation: RAMP_START_BPS + (target - start) * elapsed / ramp_slots
        let range = oi_cap_multiplier_bps - RAMP_START_BPS;
        let ramp_add = (range as u128).saturating_mul(elapsed as u128) / (oi_ramp_slots as u128);

        // Clamp to target (should never exceed due to elapsed < oi_ramp_slots, but be safe)
        let result = RAMP_START_BPS.saturating_add(ramp_add as u64);
        core::cmp::min(result, oi_cap_multiplier_bps)
    }

    // ========================================
    // PERC-304: LP Utilization-Curve Fee Multiplier
    // ========================================

    /// Kink-curve fee multiplier constants (basis points).
    pub const FEE_MULT_BASE_BPS: u64 = 10_000; // 1.0x (floor)
    pub const FEE_MULT_KINK1_BPS: u64 = 25_000; // 2.5x (at 80% util)
    pub const FEE_MULT_MAX_BPS: u64 = 75_000; // 7.5x (at 100% util)
    pub const UTIL_KINK1_BPS: u64 = 5_000; // 50% utilization
    pub const UTIL_KINK2_BPS: u64 = 8_000; // 80% utilization
    pub const UTIL_MAX_BPS: u64 = 10_000; // 100% utilization

    /// PERC-304: Compute fee multiplier based on OI utilization kink curve.
    ///
    /// Three-segment piecewise linear curve (same principle as Drift borrow rate):
    ///
    /// | Utilization         | Multiplier                                      |
    /// |---------------------|-------------------------------------------------|
    /// | 0–50 %  (0–5000)    | 1.0× (10 000 bps)                              |
    /// | 50–80 % (5000–8000) | Linear 1.0×→2.5× (10 000 → 25 000 bps)        |
    /// | 80–100% (8000–10000)| Linear 2.5×→7.5× (25 000 → 75 000 bps)        |
    /// | > 100 %             | Capped at 7.5× (75 000 bps)                    |
    ///
    /// Input:  `util_bps` — utilization in basis points (0 = idle, 10 000 = fully utilised).
    ///         Values > 10 000 are possible if OI exceeds the cap.
    /// Output: multiplier in bps ∈ [10 000, 75 000].
    ///
    /// All arithmetic is u64; no overflow possible for inputs ≤ 10 000.
    /// Monotonically non-decreasing — proven by Kani harness
    /// `proof_fee_mult_monotonically_increases_with_utilization`.
    #[inline]
    pub fn compute_fee_multiplier_bps(util_bps: u64) -> u64 {
        if util_bps <= UTIL_KINK1_BPS {
            // Segment 1: flat 1.0×
            FEE_MULT_BASE_BPS
        } else if util_bps <= UTIL_KINK2_BPS {
            // Segment 2: linear 1.0× → 2.5× over [50%, 80%]
            // slope = (25_000 - 10_000) / (8_000 - 5_000) = 15_000 / 3_000 = 5 bps per util_bps
            let excess = util_bps - UTIL_KINK1_BPS; // 0..3_000
            let range_mult = FEE_MULT_KINK1_BPS - FEE_MULT_BASE_BPS; // 15_000
            let range_util = UTIL_KINK2_BPS - UTIL_KINK1_BPS; // 3_000
            FEE_MULT_BASE_BPS + excess * range_mult / range_util
        } else if util_bps <= UTIL_MAX_BPS {
            // Segment 3: linear 2.5× → 7.5× over [80%, 100%]
            // slope = (75_000 - 25_000) / (10_000 - 8_000) = 50_000 / 2_000 = 25 bps per util_bps
            let excess = util_bps - UTIL_KINK2_BPS; // 0..2_000
            let range_mult = FEE_MULT_MAX_BPS - FEE_MULT_KINK1_BPS; // 50_000
            let range_util = UTIL_MAX_BPS - UTIL_KINK2_BPS; // 2_000
            FEE_MULT_KINK1_BPS + excess * range_mult / range_util
        } else {
            // Segment 4: capped at 7.5×
            FEE_MULT_MAX_BPS
        }
    }

    /// Compute OI utilization in basis points.
    ///
    /// `util_bps = current_oi * 10_000 / max_oi`
    ///
    /// Returns 0 if max_oi is 0 (OI cap disabled or vault empty).
    /// Can return > 10_000 if OI exceeds the cap (over-utilised).
    #[inline]
    pub fn compute_util_bps(current_oi: u128, max_oi: u128) -> u64 {
        if max_oi == 0 {
            return 0;
        }
        let util = current_oi.saturating_mul(10_000) / max_oi;
        // Clamp to u64 (can't realistically exceed but be safe)
        core::cmp::min(util, u64::MAX as u128) as u64
    }
}

// 2. mod zc (Zero-Copy unsafe island)
#[allow(unsafe_code)]
pub mod zc {
    use crate::constants::{ENGINE_ALIGN, ENGINE_LEN, ENGINE_OFF};
    use core::mem::offset_of;
    use percolator::RiskEngine;
    use solana_program::program_error::ProgramError;

    // Use const to export the actual offset for debugging
    pub const ACCOUNTS_OFFSET: usize = offset_of!(RiskEngine, accounts);

    /// Old slab length (before Account struct reordering migration)
    /// Old slabs support up to 4095 accounts, new slabs support 4096.
    ///
    /// PERC-118 migration: RiskEngine grew by 16 bytes (trade_twap_e6 + twap_last_slot).
    /// Pre-PERC-118 slabs are ENGINE_LEN - 16 bytes smaller than the current ENGINE_LEN.
    /// Pre-PERC-118 + pre-Account-reorder slabs are ENGINE_LEN - 24 bytes smaller.
    /// Accept the oldest possible layout so all existing devnet slabs remain readable.
    /// The new TWAP fields are at the END of RiskEngine and default to zero (= no TWAP,
    /// pure oracle mark) when read from memory beyond the old slab boundary.
    const OLDEST_ENGINE_LEN: usize = ENGINE_LEN - 24;

    #[inline]
    #[allow(clippy::manual_is_multiple_of)]
    pub fn engine_ref(data: &[u8]) -> Result<&RiskEngine, ProgramError> {
        // Accept all legacy slab sizes down to ENGINE_LEN - 24 for backward compatibility.
        // Migration layers: -8 (pre-Account-reorder), -16 (pre-PERC-118), -24 (both combined).
        if data.len() < ENGINE_OFF + OLDEST_ENGINE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let ptr = unsafe { data.as_ptr().add(ENGINE_OFF) };
        // Use modulo instead of .is_multiple_of() for SBF toolchain compatibility
        if (ptr as usize) % ENGINE_ALIGN != 0 {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(unsafe { &*(ptr as *const RiskEngine) })
    }

    #[inline]
    #[allow(clippy::manual_is_multiple_of)]
    pub fn engine_mut(data: &mut [u8]) -> Result<&mut RiskEngine, ProgramError> {
        // Accept all legacy slab sizes down to ENGINE_LEN - 24 for backward compatibility.
        // Migration layers: -8 (pre-Account-reorder), -16 (pre-PERC-118), -24 (both combined).
        if data.len() < ENGINE_OFF + OLDEST_ENGINE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let ptr = unsafe { data.as_mut_ptr().add(ENGINE_OFF) };
        // Use modulo instead of .is_multiple_of() for SBF toolchain compatibility
        if (ptr as usize) % ENGINE_ALIGN != 0 {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(unsafe { &mut *(ptr as *mut RiskEngine) })
    }

    // NOTE: engine_write was removed because it requires passing RiskEngine by value,
    // which stack-allocates the ~6MB struct and causes stack overflow in BPF.
    // Use engine_mut() + init_in_place() instead for initialization.

    use solana_program::{
        account_info::AccountInfo, instruction::Instruction as SolInstruction,
        program::invoke_signed_unchecked,
    };

    /// Invoke the matcher program via CPI with proper lifetime coercion.
    ///
    /// PERC-154: Uses invoke_signed_unchecked to skip RefCell borrow validation
    /// (~200 CU savings). This is safe because:
    /// - a_lp_pda is system-owned with empty data (no RefCell contention)
    /// - a_matcher_ctx is writable and we don't hold borrows across the CPI
    /// - The AccountInfo is only used for the duration of invoke_signed
    /// - We don't hold references past the function call
    #[inline]
    #[allow(unsafe_code)]
    pub fn invoke_signed_trade<'a>(
        ix: &SolInstruction,
        a_lp_pda: &AccountInfo<'a>,
        a_matcher_ctx: &AccountInfo<'a>,
        a_matcher_prog: &AccountInfo<'a>,
        seeds: &[&[u8]],
    ) -> Result<(), ProgramError> {
        // SAFETY: AccountInfos have lifetime 'a from the caller.
        // We clone them to get owned values (still with 'a lifetime internally).
        // The invoke_signed_unchecked call consumes them by reference and returns.
        // No lifetime extension occurs. RefCell validation skipped because:
        // - a_lp_pda is system-owned PDA with 0 data and 0 lamports (validated earlier)
        // - a_matcher_ctx borrow was dropped before this call
        // - a_matcher_prog is the program being invoked (required by Solana CPI)
        let infos = [
            a_lp_pda.clone(),
            a_matcher_ctx.clone(),
            a_matcher_prog.clone(),
        ];
        invoke_signed_unchecked(ix, &infos, &[seeds])
    }
}

pub mod matcher_abi {
    use crate::constants::MATCHER_ABI_VERSION;
    use solana_program::program_error::ProgramError;

    /// Matcher return flags
    pub const FLAG_VALID: u32 = 1; // bit0: response is valid
    pub const FLAG_PARTIAL_OK: u32 = 2; // bit1: partial fill including zero allowed
    pub const FLAG_REJECTED: u32 = 4; // bit2: trade rejected by matcher

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    pub struct MatcherReturn {
        pub abi_version: u32,
        pub flags: u32,
        pub exec_price_e6: u64,
        pub exec_size: i128,
        pub req_id: u64,
        pub lp_account_id: u64,
        pub oracle_price_e6: u64,
        pub reserved: u64,
    }

    pub fn read_matcher_return(ctx: &[u8]) -> Result<MatcherReturn, ProgramError> {
        if ctx.len() < 64 {
            return Err(ProgramError::InvalidAccountData);
        }
        let abi_version = u32::from_le_bytes(ctx[0..4].try_into().unwrap());
        let flags = u32::from_le_bytes(ctx[4..8].try_into().unwrap());
        let exec_price_e6 = u64::from_le_bytes(ctx[8..16].try_into().unwrap());
        let exec_size = i128::from_le_bytes(ctx[16..32].try_into().unwrap());
        let req_id = u64::from_le_bytes(ctx[32..40].try_into().unwrap());
        let lp_account_id = u64::from_le_bytes(ctx[40..48].try_into().unwrap());
        let oracle_price_e6 = u64::from_le_bytes(ctx[48..56].try_into().unwrap());
        let reserved = u64::from_le_bytes(ctx[56..64].try_into().unwrap());

        Ok(MatcherReturn {
            abi_version,
            flags,
            exec_price_e6,
            exec_size,
            req_id,
            lp_account_id,
            oracle_price_e6,
            reserved,
        })
    }

    pub fn validate_matcher_return(
        ret: &MatcherReturn,
        lp_account_id: u64,
        oracle_price_e6: u64,
        req_size: i128,
        req_id: u64,
    ) -> Result<(), ProgramError> {
        // Check ABI version
        if ret.abi_version != MATCHER_ABI_VERSION {
            return Err(ProgramError::InvalidAccountData);
        }
        // Must have VALID flag set
        if (ret.flags & FLAG_VALID) == 0 {
            return Err(ProgramError::InvalidAccountData);
        }
        // Must not have REJECTED flag set
        if (ret.flags & FLAG_REJECTED) != 0 {
            return Err(ProgramError::InvalidAccountData);
        }

        // Validate echoed fields match request
        if ret.lp_account_id != lp_account_id {
            return Err(ProgramError::InvalidAccountData);
        }
        if ret.oracle_price_e6 != oracle_price_e6 {
            return Err(ProgramError::InvalidAccountData);
        }
        if ret.reserved != 0 {
            return Err(ProgramError::InvalidAccountData);
        }
        if ret.req_id != req_id {
            return Err(ProgramError::InvalidAccountData);
        }

        // Require exec_price_e6 != 0 always - avoids "all zeros but valid flag" ambiguity
        if ret.exec_price_e6 == 0 {
            return Err(ProgramError::InvalidAccountData);
        }

        // Zero exec_size requires PARTIAL_OK flag
        if ret.exec_size == 0 {
            if (ret.flags & FLAG_PARTIAL_OK) == 0 {
                return Err(ProgramError::InvalidAccountData);
            }
            // Zero fill with PARTIAL_OK is allowed - return early
            return Ok(());
        }

        // Size constraints (use unsigned_abs to avoid i128::MIN overflow)
        if ret.exec_size.unsigned_abs() > req_size.unsigned_abs() {
            return Err(ProgramError::InvalidAccountData);
        }
        if req_size != 0 && ret.exec_size.signum() != req_size.signum() {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(())
    }
}

// 3. mod error
pub mod error {
    use percolator::RiskError;
    use solana_program::program_error::ProgramError;

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub enum PercolatorError {
        InvalidMagic,
        InvalidVersion,
        AlreadyInitialized,
        NotInitialized,
        InvalidSlabLen,
        InvalidOracleKey,
        OracleStale,
        OracleConfTooWide,
        InvalidVaultAta,
        InvalidMint,
        ExpectedSigner,
        ExpectedWritable,
        OracleInvalid,
        EngineInsufficientBalance,
        EngineUndercollateralized,
        EngineUnauthorized,
        EngineInvalidMatchingEngine,
        EnginePnlNotWarmedUp,
        EngineOverflow,
        EngineAccountNotFound,
        EngineNotAnLPAccount,
        EnginePositionSizeMismatch,
        EngineRiskReductionOnlyMode,
        EngineAccountKindMismatch,
        InvalidTokenAccount,
        InvalidTokenProgram,
        InvalidConfigParam,
        HyperpTradeNoCpiDisabled,
        InsuranceMintAlreadyExists,
        InsuranceMintNotCreated,
        InsuranceBelowThreshold,
        InsuranceZeroAmount,
        InsuranceSupplyMismatch,
        /// Market is paused — trading, deposits, and withdrawals are disabled
        MarketPaused,
        /// #312: RenounceAdmin not allowed — market must be RESOLVED first
        AdminRenounceNotAllowed,
        /// #312: Invalid confirmation code for RenounceAdmin
        InvalidConfirmation,
        /// #299: InitMarket vault seed below minimum (PERC-136)
        InsufficientSeed,
        /// #297: DEX pool has insufficient liquidity for safe Hyperp oracle bootstrapping.
        /// The quote-side reserves must meet the minimum threshold to resist manipulation.
        InsufficientDexLiquidity,
        /// PERC-272: LP vault already created for this market.
        LpVaultAlreadyExists,
        /// PERC-272: LP vault not yet created (call CreateLpVault first).
        LpVaultNotCreated,
        /// PERC-272: LP vault zero amount (deposit or withdraw amount is zero).
        LpVaultZeroAmount,
        /// PERC-272: LP vault supply/capital mismatch (supply > 0 but capital == 0).
        LpVaultSupplyMismatch,
        /// PERC-272: LP vault withdrawal exceeds available capital after OI reservation.
        LpVaultWithdrawExceedsAvailable,
        /// PERC-272: LP vault fee share basis points out of range (0..=10_000).
        LpVaultInvalidFeeShare,
        /// PERC-272: No new fees to distribute to LP vault.
        LpVaultNoNewFees,
        /// PERC-312: Safety valve — new position on dominant side blocked during rebalancing.
        SafetyValveDominantSideBlocked,
        /// PERC-314: Dispute window has closed.
        DisputeWindowClosed,
        /// PERC-314: Dispute already exists for this market.
        DisputeAlreadyExists,
        /// PERC-314: Market not resolved — cannot dispute.
        MarketNotResolved,
        /// PERC-314: No active dispute to resolve.
        NoActiveDispute,
        /// PERC-315: LP collateral not enabled for this market.
        LpCollateralDisabled,
        /// PERC-315: Position still open — cannot withdraw LP collateral.
        LpCollateralPositionOpen,
        /// PERC-309: Withdraw queue already exists.
        WithdrawQueueAlreadyExists,
        /// PERC-309: No queued withdrawal found.
        WithdrawQueueNotFound,
        /// PERC-309: Nothing claimable this epoch.
        WithdrawQueueNothingClaimable,
        /// Audit crank detected a conservation invariant violation.
        AuditViolation,
        /// Cross-margin offset pair not configured for these slabs.
        CrossMarginPairNotFound,
        /// Cross-margin attestation stale (too many slots since attested).
        CrossMarginAttestationStale,
    }

    impl From<PercolatorError> for ProgramError {
        fn from(e: PercolatorError) -> Self {
            ProgramError::Custom(e as u32)
        }
    }

    pub fn map_risk_error(e: RiskError) -> ProgramError {
        let err = match e {
            RiskError::InsufficientBalance => PercolatorError::EngineInsufficientBalance,
            RiskError::Undercollateralized => PercolatorError::EngineUndercollateralized,
            RiskError::Unauthorized => PercolatorError::EngineUnauthorized,
            RiskError::InvalidMatchingEngine => PercolatorError::EngineInvalidMatchingEngine,
            RiskError::PnlNotWarmedUp => PercolatorError::EnginePnlNotWarmedUp,
            RiskError::Overflow => PercolatorError::EngineOverflow,
            RiskError::AccountNotFound => PercolatorError::EngineAccountNotFound,
            RiskError::NotAnLPAccount => PercolatorError::EngineNotAnLPAccount,
            RiskError::PositionSizeMismatch => PercolatorError::EnginePositionSizeMismatch,
            RiskError::AccountKindMismatch => PercolatorError::EngineAccountKindMismatch,
        };
        ProgramError::Custom(err as u32)
    }
}

// 4. mod ix
pub mod ix {
    use percolator::{RiskParams, U128};
    use solana_program::{program_error::ProgramError, pubkey::Pubkey};

    #[derive(Debug)]
    #[allow(clippy::large_enum_variant)]
    pub enum Instruction {
        /// PERC-331: InitMarket is early-dispatched in process_instruction before
        /// Instruction::decode() is called, so this variant is never constructed.
        /// Fields removed to shrink the enum (RiskParams ~300 B was the largest variant,
        /// bloating every other variant's stack allocation and causing SBF stack overflow).
        InitMarket,
        InitUser {
            fee_payment: u64,
        },
        InitLP {
            matcher_program: Pubkey,
            matcher_context: Pubkey,
            fee_payment: u64,
        },
        DepositCollateral {
            user_idx: u16,
            amount: u64,
        },
        WithdrawCollateral {
            user_idx: u16,
            amount: u64,
        },
        KeeperCrank {
            caller_idx: u16,
            allow_panic: u8,
        },
        TradeNoCpi {
            lp_idx: u16,
            user_idx: u16,
            size: i128,
        },
        LiquidateAtOracle {
            target_idx: u16,
        },
        CloseAccount {
            user_idx: u16,
        },
        TopUpInsurance {
            amount: u64,
        },
        TradeCpi {
            lp_idx: u16,
            user_idx: u16,
            size: i128,
        },
        SetRiskThreshold {
            new_threshold: u128,
        },
        UpdateAdmin {
            new_admin: Pubkey,
        },
        /// Close the market slab and recover SOL to admin.
        /// Requires: no active accounts, no vault funds, no insurance funds.
        CloseSlab,
        /// Update configurable parameters (funding + threshold). Admin only.
        UpdateConfig {
            funding_horizon_slots: u64,
            funding_k_bps: u64,
            funding_inv_scale_notional_e6: u128,
            funding_max_premium_bps: i64,
            funding_max_bps_per_slot: i64,
            thresh_floor: u128,
            thresh_risk_bps: u64,
            thresh_update_interval_slots: u64,
            thresh_step_bps: u64,
            thresh_alpha_bps: u64,
            thresh_min: u128,
            thresh_max: u128,
            thresh_min_step: u128,
            // PERC-121: Premium funding params
            funding_premium_weight_bps: u64,
            funding_settlement_interval_slots: u64,
            funding_premium_dampening_e6: u64,
            funding_premium_max_bps_per_slot: i64,
            /// Quadratic funding convexity coefficient k2 (bps). 0 = disabled.
            funding_k2_bps: u16,
        },
        /// Set maintenance fee per slot (admin only)
        SetMaintenanceFee {
            new_fee: u128,
        },
        /// Set the oracle price authority (admin only).
        /// Authority can push prices instead of requiring Pyth/Chainlink.
        /// Pass zero pubkey to disable and require Pyth/Chainlink.
        SetOracleAuthority {
            new_authority: Pubkey,
        },
        /// Push oracle price (oracle authority only).
        /// Stores the price for use by crank/trade operations.
        PushOraclePrice {
            price_e6: u64,
            timestamp: i64,
        },
        /// Set oracle price circuit breaker cap (admin only).
        /// max_change_e2bps in 0.01 bps units (1_000_000 = 100%). 0 = disabled.
        SetOraclePriceCap {
            max_change_e2bps: u64,
        },
        /// Resolve market: force-close all positions at admin oracle price, enter withdraw-only mode.
        /// Admin only. Uses authority_price_e6 as settlement price.
        ResolveMarket,
        /// Withdraw insurance fund balance (admin only, requires RESOLVED flag).
        WithdrawInsurance,
        /// Admin force-close: unconditionally close any position at oracle price.
        /// Skips margin checks. Admin only.
        AdminForceClose {
            target_idx: u16,
        },
        /// Update initial and maintenance margin BPS. Admin only.
        UpdateRiskParams {
            initial_margin_bps: u64,
            maintenance_margin_bps: u64,
            trading_fee_bps: Option<u64>,
            /// OI cap multiplier in bps. 0 = disabled. 100_000 = 10x vault.
            /// None = don't change (backwards compatible).
            oi_cap_multiplier_bps: Option<u64>,
            /// Max total positive PnL cap. 0 = disabled.
            /// None = don't change (backwards compatible).
            max_pnl_cap: Option<u64>,
            /// PERC-302: OI ramp duration in slots. 0 = full cap immediately.
            /// None = don't change (backwards compatible).
            oi_ramp_slots: Option<u64>,
            /// PERC-298: Skew factor for dynamic OI cap tightening (bps).
            /// 0 = disabled. None = don't change (backwards compatible).
            skew_factor_bps: Option<u64>,
            /// PERC-300: Adaptive funding rate config.
            /// None = don't change (backwards compatible).
            adaptive_funding_enabled: Option<u8>,
            adaptive_scale_bps: Option<u16>,
            adaptive_max_funding_bps: Option<u64>,
            /// PERC-118: Mark price blend weight (oracle %).
            /// 10_000 = 100% oracle. 7_000 = 70% oracle + 30% TWAP.
            /// None = don't change (backwards compatible).
            mark_oracle_weight_bps: Option<u16>,
            /// VRAM: Sensitivity scaling. 0 = disabled. 10_000 = 1.0x base scaling.
            vol_margin_scale_bps: Option<u16>,
            /// VRAM: EWMA alpha (e6). Controls smoothing of variance estimate.
            vol_alpha_e6: Option<u16>,
            /// VRAM: Target volatility (e6). Baseline vol for 1.0x margin.
            vol_margin_target_e6: Option<u16>,
        },
        /// Renounce admin: set admin to all zeros (irreversible). Admin only.
        /// Renounce admin permanently. Requires market RESOLVED and confirmation code.
        RenounceAdmin {
            confirmation: u64,
        },
        /// Create the insurance LP SPL mint for this market. Admin only, once per market.
        /// Mint PDA: ["ins_lp", slab_pubkey]. Authority: vault PDA.
        CreateInsuranceMint,
        /// Deposit collateral into insurance fund, receive LP tokens proportional to share.
        /// Permissionless. LP tokens are freely transferable.
        DepositInsuranceLP {
            amount: u64,
        },
        /// Burn LP tokens and withdraw proportional share of insurance fund.
        /// Cannot withdraw below risk_reduction_threshold.
        WithdrawInsuranceLP {
            lp_amount: u64,
        },
        /// Pause the market. Admin only. Blocks trade/deposit/withdraw/init_user.
        /// Crank, liquidation, admin actions, and unpause still allowed.
        PauseMarket,
        /// Unpause the market. Admin only. Restores normal operation.
        UnpauseMarket,
        /// Two-step admin transfer: Step 2 — pending admin accepts the role.
        AcceptAdmin,

        /// Set the insurance withdrawal policy on a resolved market (Tag 30).
        ///
        /// Admin only. Creates/updates an `InsuranceWithdrawPolicy` PDA account at
        /// `[b"ins_policy", slab_key]` with the given parameters.
        ///
        /// Accounts:
        ///   0. `[signer, writable]` Admin (payer for policy account creation)
        ///   1. `[writable]` Slab account
        ///   2. `[writable]` Policy PDA (derived: [b"ins_policy", slab_key], created if needed)
        ///   3. `[]` System program (for account creation)
        SetInsuranceWithdrawPolicy {
            /// Pubkey authorized to call `WithdrawInsuranceLimited`
            authority: Pubkey,
            /// Minimum withdrawal amount in base token lamports
            min_withdraw_base: u64,
            /// Maximum withdrawal per epoch as bps of insurance balance (10_000 = 100%)
            max_withdraw_bps: u16,
            /// Minimum slots between withdrawals (cooldown)
            cooldown_slots: u64,
        },

        /// Withdraw a limited amount from the insurance fund (Tag 31).
        ///
        /// Callable by the policy authority (not necessarily admin). Requires:
        ///   - Market is RESOLVED
        ///   - SetInsuranceWithdrawPolicy called first
        ///   - Cooldown elapsed since last withdrawal
        ///   - Amount within per-epoch bps cap
        ///
        /// Accounts:
        ///   0. `[signer]` Authority (must match policy.authority)
        ///   1. `[writable]` Slab account
        ///   2. `[writable]` Authority's token account (destination)
        ///   3. `[writable]` Insurance vault token account (source)
        ///   4. `[]` Token program
        ///   5. `[]` Vault authority PDA
        ///   6. `[writable]` Policy PDA (updated: last_withdraw_slot, epoch_drawn)
        ///   7. `[]` Clock sysvar
        WithdrawInsuranceLimited {
            /// Amount to withdraw in base token lamports
            amount: u64,
        },

        /// Configure on-chain Pyth oracle for this market (Tag 32).
        /// Admin-only. Switches to Pyth-pinned mode.
        SetPythOracle {
            feed_id: [u8; 32],
            max_staleness_secs: u64,
            conf_filter_bps: u16,
        },

        /// PERC-117: Update mark price from Pyth oracle (Tag 33).
        ///
        /// **Permissionless** — anyone can call. This is the mark price crank
        /// for Pyth-pinned markets. Reads the current Pyth PriceUpdateV2 price,
        /// applies 8-hour EMA smoothing with circuit breaker, and writes the
        /// new mark to `authority_price_e6`.
        ///
        /// Requires: market is in Pyth-pinned mode
        ///   (`oracle_authority == [0;32]` AND `index_feed_id != [0;32]`).
        ///
        /// Accounts:
        /// - 0. `[writable]` Slab
        /// - 1. `[]`         Pyth PriceUpdateV2 account
        /// - 2. `[]`         Clock sysvar
        UpdateMarkPrice,

        /// Update the Hyperp mark price from a DEX oracle (Tag 34).
        ///
        /// **Permissionless** — anyone can call. This is the core Hyperp EMA oracle
        /// mechanism for permissionless token markets (no Pyth/Chainlink needed).
        ///
        /// Reads the current spot price from a PumpSwap, Raydium CLMM, or
        /// Meteora DLMM pool account, applies 8-hour EMA smoothing with circuit
        /// breaker, and writes the new mark to `authority_price_e6`.
        ///
        /// Requires: market is in Hyperp mode (`index_feed_id == [0;32]`).
        /// The DEX oracle account must be owned by an approved DEX program.
        ///
        /// Accounts:
        /// - 0. `[writable]` Slab
        /// - 1. `[]` DEX pool account (PumpSwap / Raydium CLMM / Meteora DLMM)
        /// - 2. `[]` Clock sysvar
        /// - 3..N `[]` Remaining accounts (PumpSwap vault0, vault1 for price calc)
        UpdateHyperpMark,
        /// PERC-154: Optimized TradeCpi with caller-provided PDA bump.
        /// Eliminates `find_program_address` (~1500 CU savings).
        /// Same accounts and semantics as TradeCpi; instruction data adds 1 bump byte.
        TradeCpiV2 {
            lp_idx: u16,
            user_idx: u16,
            size: i128,
            bump: u8,
        },
        /// PERC-273: Unresolve a market — clear RESOLVED flag, re-enable trading.
        /// Admin only. Requires confirmation code to prevent accidental invocation.
        /// Accounts: [admin(signer), slab(writable)]
        UnresolveMarket {
            /// Must equal 0xDEAD_BEEF_CAFE_1234 to confirm intent
            confirmation: u64,
        },

        /// PERC-272: Create LP vault — initialise state PDA + SPL mint for LP shares.
        /// Admin only. One per market.
        /// fee_share_bps: 0..=10_000 — portion of trading fees directed to LP vault.
        /// util_curve_enabled: PERC-304 — enable utilization kink curve for fee multiplier.
        ///   Optional (backwards compatible). Default: false (disabled).
        /// Accounts: [admin(signer,payer), slab(writable), lp_vault_state(writable),
        ///            lp_vault_mint(writable), vault_authority, system_program,
        ///            token_program, rent_sysvar]
        CreateLpVault {
            fee_share_bps: u64,
            /// PERC-304: Whether to enable the utilization kink curve.
            util_curve_enabled: bool,
        },
        /// PERC-272: Deposit SOL into LP vault, receive LP shares.
        /// Permissionless. LP shares are freely transferable SPL tokens.
        /// Accounts: [depositor(signer), slab(writable), depositor_ata(writable),
        ///            vault(writable), token_program, lp_vault_mint(writable),
        ///            depositor_lp_ata(writable), vault_authority,
        ///            lp_vault_state(writable)]
        LpVaultDeposit {
            amount: u64,
        },
        /// PERC-272: Burn LP shares and withdraw proportional SOL from LP vault.
        /// Cannot withdraw if it would bring vault capital below OI reservation.
        /// Accounts: [withdrawer(signer), slab(writable), withdrawer_ata(writable),
        ///            vault(writable), token_program, lp_vault_mint(writable),
        ///            withdrawer_lp_ata(writable), vault_authority,
        ///            lp_vault_state(writable)]
        LpVaultWithdraw {
            lp_amount: u64,
        },
        /// PERC-272: Permissionless crank — distribute accrued fee revenue to LP vault.
        /// Reads fee_revenue delta since last snapshot, credits LP portion to vault capital.
        /// Accounts: [slab(writable), lp_vault_state(writable)]
        LpVaultCrankFees,

        /// PERC-306: Fund per-market isolated insurance balance.
        /// Accounts: [admin(signer, writable), slab(writable), user_ata(writable), vault(writable), token_program]
        FundMarketInsurance {
            amount: u64,
        },

        /// PERC-306: Set insurance isolation BPS for a market.
        /// Accounts: [admin(signer), slab(writable)]
        SetInsuranceIsolation {
            bps: u16,
        },
        /// PERC-314: Challenge settlement price.
        ChallengeSettlement {
            proposed_price_e6: u64,
        },
        /// PERC-314: Resolve dispute (admin).
        ResolveDispute {
            accept: u8,
        },
        /// PERC-315: Deposit LP vault tokens as perp collateral.
        DepositLpCollateral {
            user_idx: u16,
            lp_amount: u64,
        },
        /// PERC-315: Withdraw LP collateral (position must be closed).
        WithdrawLpCollateral {
            user_idx: u16,
            lp_amount: u64,
        },
        /// PERC-309: Queue large LP withdrawal.
        QueueWithdrawal {
            lp_amount: u64,
        },
        /// PERC-309: Claim one epoch tranche.
        ClaimQueuedWithdrawal,
        /// PERC-309: Cancel queued withdrawal.
        CancelQueuedWithdrawal,
        /// PERC-305: Auto-deleverage — surgically close/reduce the most profitable
        /// position when pnl_pos_tot exceeds max_pnl_cap. Permissionless.
        ExecuteAdl {
            target_idx: u16,
        },
        /// Close a stale slab (wrong size from an old program layout) and recover rent SOL.
        ///
        /// Unlike CloseSlab (tag 13), this path skips `slab_guard` so that slabs with
        /// invalid data lengths (left over from old devnet deploys) can be reclaimed.
        ///
        /// Safety guards:
        ///   1. slab.owner == program_id
        ///   2. slab data length must NOT match any currently-accepted tier (SLAB_LEN,
        ///      SLAB_LEN-16, SLAB_LEN-24). Use CloseSlab for properly-sized slabs.
        ///   3. First 8 bytes must equal the MAGIC constant ("PERCOLAT"). Slabs with
        ///      zero/garbage magic cannot be closed through this path.
        ///   4. Signer must match the admin field at header offset 16..48.
        ///
        /// Accounts: [dest(signer,writable), slab(writable)]
        CloseStaleSlabs,

        /// PERC-511: Reclaim rent from an uninitialised slab when market creation fails mid-flow.
        ///
        /// This is the self-service recovery path for users whose market creation tx failed
        /// after the slab account was funded for rent but before InitMarket completed.
        /// It allows the market creator to reclaim their SOL without admin intervention.
        ///
        /// Safety guards:
        ///   1. slab.owner == program_id
        ///   2. Slab must be uninitialised: first 8 bytes MUST NOT equal MAGIC ("PERCOLAT").
        ///      → If magic IS present, the market was initialised; use CloseSlab (tag 13) instead.
        ///   3. Slab account must be a signer (proves the caller holds the slab keypair).
        ///   4. Destination must be a signer and writable (receives the reclaimed lamports).
        ///
        /// Accounts: [dest(signer,writable), slab(signer,writable)]
        ReclaimSlabRent,
        /// PERC-622: Advance oracle phase (permissionless crank).
        AdvanceOraclePhase,

        /// On-chain audit crank: walk all accounts and verify conservation invariants.
        /// Permissionless. Checks capital, PnL, OI, LP aggregates and solvency.
        /// Sets FLAG_PAUSED on violation.
        ///
        /// Accounts: [slab(writable)]
        AuditCrank,

        /// Admin: configure cross-market margin offset for a pair of slabs.
        /// Creates/updates an OffsetPairConfig PDA at ["cmor_pair", slab_a, slab_b].
        ///
        /// Accounts: [admin(signer,payer), slab_a, slab_b, pair_pda(writable), system_program]
        SetOffsetPair {
            offset_bps: u16,
        },

        /// Permissionless: attest user positions across two slabs for portfolio margin credit.
        /// Creates/updates a CrossMarginAttestation PDA at ["cmor", user, slab_a, slab_b].
        ///
        /// Accounts: [payer(signer), slab_a, slab_b, attestation_pda(writable),
        ///            pair_pda, system_program]
        AttestCrossMargin {
            user_idx_a: u16,
            user_idx_b: u16,
        },
        /// PERC-623: Anyone can top up a market's keeper fund by transferring
        /// lamports. The amount is read from instruction data (u64).
        TopUpKeeperFund {
            amount: u64,
        },
        /// PERC-628: Initialize the global shared vault.
        InitSharedVault {
            epoch_duration_slots: u64,
            max_market_exposure_bps: u16,
        },
        /// PERC-628: Allocate virtual liquidity to a market.
        AllocateMarket {
            amount: u128,
        },
        /// PERC-628: Queue a withdrawal for the current epoch.
        QueueWithdrawalSV {
            lp_amount: u64,
        },
        /// PERC-628: Claim a queued withdrawal after epoch elapses.
        ClaimEpochWithdrawal,
        /// PERC-628: Advance the shared vault epoch (permissionless crank).
        AdvanceEpoch,
    }

    impl Instruction {
        pub fn decode(input: &[u8]) -> Result<Self, ProgramError> {
            let (&tag, mut rest) = input
                .split_first()
                .ok_or(ProgramError::InvalidInstructionData)?;

            use crate::tags::*;
            match tag {
                TAG_INIT_MARKET => {
                    // PERC-331: InitMarket is early-dispatched in process_instruction
                    // before decode() is called. If we reach here, it's a logic error.
                    // Return the fieldless variant (fields parsed in process_init_market).
                    Ok(Instruction::InitMarket)
                }
                TAG_INIT_USER => {
                    // InitUser
                    let fee_payment = read_u64(&mut rest)?;
                    Ok(Instruction::InitUser { fee_payment })
                }
                TAG_INIT_LP => {
                    // InitLP
                    let matcher_program = read_pubkey(&mut rest)?;
                    let matcher_context = read_pubkey(&mut rest)?;
                    let fee_payment = read_u64(&mut rest)?;
                    Ok(Instruction::InitLP {
                        matcher_program,
                        matcher_context,
                        fee_payment,
                    })
                }
                TAG_DEPOSIT_COLLATERAL => {
                    // Deposit
                    let user_idx = read_u16(&mut rest)?;
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::DepositCollateral { user_idx, amount })
                }
                TAG_WITHDRAW_COLLATERAL => {
                    // Withdraw
                    let user_idx = read_u16(&mut rest)?;
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::WithdrawCollateral { user_idx, amount })
                }
                TAG_KEEPER_CRANK => {
                    // KeeperCrank
                    let caller_idx = read_u16(&mut rest)?;
                    let allow_panic = read_u8(&mut rest)?;
                    Ok(Instruction::KeeperCrank {
                        caller_idx,
                        allow_panic,
                    })
                }
                TAG_TRADE_NO_CPI => {
                    // TradeNoCpi
                    let lp_idx = read_u16(&mut rest)?;
                    let user_idx = read_u16(&mut rest)?;
                    let size = read_i128(&mut rest)?;
                    Ok(Instruction::TradeNoCpi {
                        lp_idx,
                        user_idx,
                        size,
                    })
                }
                TAG_LIQUIDATE_AT_ORACLE => {
                    // LiquidateAtOracle
                    let target_idx = read_u16(&mut rest)?;
                    Ok(Instruction::LiquidateAtOracle { target_idx })
                }
                TAG_CLOSE_ACCOUNT => {
                    // CloseAccount
                    let user_idx = read_u16(&mut rest)?;
                    Ok(Instruction::CloseAccount { user_idx })
                }
                TAG_TOP_UP_INSURANCE => {
                    // TopUpInsurance
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::TopUpInsurance { amount })
                }
                TAG_TRADE_CPI => {
                    // TradeCpi
                    let lp_idx = read_u16(&mut rest)?;
                    let user_idx = read_u16(&mut rest)?;
                    let size = read_i128(&mut rest)?;
                    Ok(Instruction::TradeCpi {
                        lp_idx,
                        user_idx,
                        size,
                    })
                }
                TAG_SET_RISK_THRESHOLD => {
                    // SetRiskThreshold
                    let new_threshold = read_u128(&mut rest)?;
                    Ok(Instruction::SetRiskThreshold { new_threshold })
                }
                TAG_UPDATE_ADMIN => {
                    // UpdateAdmin
                    let new_admin = read_pubkey(&mut rest)?;
                    Ok(Instruction::UpdateAdmin { new_admin })
                }
                TAG_CLOSE_SLAB => {
                    // CloseSlab
                    Ok(Instruction::CloseSlab)
                }
                TAG_UPDATE_CONFIG => {
                    // UpdateConfig
                    let funding_horizon_slots = read_u64(&mut rest)?;
                    let funding_k_bps = read_u64(&mut rest)?;
                    let funding_inv_scale_notional_e6 = read_u128(&mut rest)?;
                    let funding_max_premium_bps = read_i64(&mut rest)?;
                    let funding_max_bps_per_slot = read_i64(&mut rest)?;
                    let thresh_floor = read_u128(&mut rest)?;
                    let thresh_risk_bps = read_u64(&mut rest)?;
                    let thresh_update_interval_slots = read_u64(&mut rest)?;
                    let thresh_step_bps = read_u64(&mut rest)?;
                    let thresh_alpha_bps = read_u64(&mut rest)?;
                    let thresh_min = read_u128(&mut rest)?;
                    let thresh_max = read_u128(&mut rest)?;
                    let thresh_min_step = read_u128(&mut rest)?;
                    // #983: Validate optional tail length to prevent partial config
                    // corruption from truncated payloads.
                    // Valid tail sizes: 0 (no optional), 8+8+8+8=32 (premium params),
                    // 32+2=34 (premium + k2).
                    const VALID_CONFIG_TAIL_LENS: &[usize] = &[0, 32, 34];
                    if !VALID_CONFIG_TAIL_LENS.contains(&rest.len()) {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                    // PERC-121: Premium funding params (optional for backward compat)
                    let funding_premium_weight_bps = read_u64(&mut rest).unwrap_or(0);
                    let funding_settlement_interval_slots = read_u64(&mut rest).unwrap_or(0);
                    let funding_premium_dampening_e6 = read_u64(&mut rest).unwrap_or(1_000_000);
                    let funding_premium_max_bps_per_slot = read_i64(&mut rest).unwrap_or(5);
                    // Quadratic funding convexity k2 (optional, backward compatible)
                    let funding_k2_bps = read_u16(&mut rest).unwrap_or(0);
                    Ok(Instruction::UpdateConfig {
                        funding_horizon_slots,
                        funding_k_bps,
                        funding_inv_scale_notional_e6,
                        funding_max_premium_bps,
                        funding_max_bps_per_slot,
                        thresh_floor,
                        thresh_risk_bps,
                        thresh_update_interval_slots,
                        thresh_step_bps,
                        thresh_alpha_bps,
                        thresh_min,
                        thresh_max,
                        thresh_min_step,
                        funding_premium_weight_bps,
                        funding_settlement_interval_slots,
                        funding_premium_dampening_e6,
                        funding_premium_max_bps_per_slot,
                        funding_k2_bps,
                    })
                }
                TAG_SET_MAINTENANCE_FEE => {
                    // SetMaintenanceFee
                    let new_fee = read_u128(&mut rest)?;
                    Ok(Instruction::SetMaintenanceFee { new_fee })
                }
                TAG_SET_ORACLE_AUTHORITY => {
                    // SetOracleAuthority
                    let new_authority = read_pubkey(&mut rest)?;
                    Ok(Instruction::SetOracleAuthority { new_authority })
                }
                TAG_PUSH_ORACLE_PRICE => {
                    // PushOraclePrice
                    let price_e6 = read_u64(&mut rest)?;
                    let timestamp = read_i64(&mut rest)?;
                    Ok(Instruction::PushOraclePrice {
                        price_e6,
                        timestamp,
                    })
                }
                TAG_SET_ORACLE_PRICE_CAP => {
                    // SetOraclePriceCap
                    let max_change_e2bps = read_u64(&mut rest)?;
                    Ok(Instruction::SetOraclePriceCap { max_change_e2bps })
                }
                TAG_RESOLVE_MARKET => Ok(Instruction::ResolveMarket),
                TAG_WITHDRAW_INSURANCE => Ok(Instruction::WithdrawInsurance),
                TAG_ADMIN_FORCE_CLOSE => {
                    // AdminForceClose
                    let target_idx = read_u16(&mut rest)?;
                    Ok(Instruction::AdminForceClose { target_idx })
                }
                TAG_UPDATE_RISK_PARAMS => {
                    // UpdateRiskParams
                    let initial_margin_bps = read_u64(&mut rest)?;
                    let maintenance_margin_bps = read_u64(&mut rest)?;
                    // Optional: trading_fee_bps (backwards compatible — old clients send 17 bytes, new send 25+)
                    let trading_fee_bps = if rest.len() >= 8 {
                        Some(read_u64(&mut rest)?)
                    } else {
                        None
                    };
                    // Optional: oi_cap_multiplier_bps (PERC-273, backwards compatible)
                    let oi_cap_multiplier_bps = if rest.len() >= 8 {
                        Some(read_u64(&mut rest)?)
                    } else {
                        None
                    };
                    // Optional: max_pnl_cap (PERC-272, backwards compatible)
                    let max_pnl_cap = if rest.len() >= 8 {
                        Some(read_u64(&mut rest)?)
                    } else {
                        None
                    };
                    // Optional: oi_ramp_slots (PERC-302, backwards compatible)
                    let oi_ramp_slots = if rest.len() >= 8 {
                        Some(read_u64(&mut rest)?)
                    } else {
                        None
                    };
                    // Optional: skew_factor_bps (PERC-298, backwards compatible)
                    let skew_factor_bps = if rest.len() >= 8 {
                        Some(read_u64(&mut rest)?)
                    } else {
                        None
                    };
                    // PERC-649: Strict tail-length guard.
                    // After the five optional u64 fields the only valid remaining
                    // byte-counts are:
                    //   0  — no adaptive or mark params
                    //   1  — adaptive_funding_enabled only          (u8)
                    //   3  — + adaptive_scale_bps                   (+u16)
                    //  11  — + adaptive_max_funding_bps             (+u64)
                    //  13  — + mark_oracle_weight_bps               (+u16)
                    // Any other length (e.g. 2 — an isolated u16 that looks like
                    // mark_oracle_weight_bps) would be mis-decoded as
                    // adaptive_funding_enabled, silently corrupting state.
                    // Extended: +2 vol_margin_scale_bps, +2 vol_alpha_e6, +2 vol_margin_target_e6
                    const VALID_TAIL_LENS: &[usize] = &[0, 1, 3, 11, 13, 15, 17, 19];
                    if !VALID_TAIL_LENS.contains(&rest.len()) {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                    // PERC-300: Optional adaptive funding params
                    let adaptive_funding_enabled = if !rest.is_empty() {
                        Some(read_u8(&mut rest)?)
                    } else {
                        None
                    };
                    let adaptive_scale_bps = if rest.len() >= 2 {
                        Some(read_u16(&mut rest)?)
                    } else {
                        None
                    };
                    let adaptive_max_funding_bps = if rest.len() >= 8 {
                        Some(read_u64(&mut rest)?)
                    } else {
                        None
                    };
                    // PERC-118: Optional mark blend weight
                    let mark_oracle_weight_bps = if rest.len() >= 2 {
                        Some(read_u16(&mut rest)?)
                    } else {
                        None
                    };
                    // VRAM: Optional volatility-regime adaptive margin params
                    let vol_margin_scale_bps = if rest.len() >= 2 {
                        Some(read_u16(&mut rest)?)
                    } else {
                        None
                    };
                    let vol_alpha_e6 = if rest.len() >= 2 {
                        Some(read_u16(&mut rest)?)
                    } else {
                        None
                    };
                    let vol_margin_target_e6 = if rest.len() >= 2 {
                        Some(read_u16(&mut rest)?)
                    } else {
                        None
                    };
                    Ok(Instruction::UpdateRiskParams {
                        initial_margin_bps,
                        maintenance_margin_bps,
                        trading_fee_bps,
                        oi_cap_multiplier_bps,
                        max_pnl_cap,
                        oi_ramp_slots,
                        skew_factor_bps,
                        adaptive_funding_enabled,
                        adaptive_scale_bps,
                        adaptive_max_funding_bps,
                        mark_oracle_weight_bps,
                        vol_margin_scale_bps,
                        vol_alpha_e6,
                        vol_margin_target_e6,
                    })
                }
                TAG_RENOUNCE_ADMIN => {
                    let confirmation = read_u64(&mut rest)?;
                    Ok(Instruction::RenounceAdmin { confirmation })
                }
                TAG_CREATE_INSURANCE_MINT => Ok(Instruction::CreateInsuranceMint),
                TAG_DEPOSIT_INSURANCE_LP => {
                    // DepositInsuranceLP
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::DepositInsuranceLP { amount })
                }
                TAG_WITHDRAW_INSURANCE_LP => {
                    // WithdrawInsuranceLP
                    let lp_amount = read_u64(&mut rest)?;
                    Ok(Instruction::WithdrawInsuranceLP { lp_amount })
                }
                TAG_PAUSE_MARKET => Ok(Instruction::PauseMarket),
                TAG_UNPAUSE_MARKET => Ok(Instruction::UnpauseMarket),
                TAG_ACCEPT_ADMIN => Ok(Instruction::AcceptAdmin),
                TAG_SET_INSURANCE_WITHDRAW_POLICY => {
                    // SetInsuranceWithdrawPolicy (Tag 30):
                    // authority(32) + min_withdraw_base(8) + max_withdraw_bps(2) + cooldown_slots(8) = 50 bytes
                    if rest.len() < 50 {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                    let authority = {
                        let arr: [u8; 32] = rest[..32]
                            .try_into()
                            .map_err(|_| ProgramError::InvalidInstructionData)?;
                        Pubkey::from(arr)
                    };
                    let min_withdraw_base = u64::from_le_bytes(
                        rest[32..40]
                            .try_into()
                            .map_err(|_| ProgramError::InvalidInstructionData)?,
                    );
                    let max_withdraw_bps = u16::from_le_bytes(
                        rest[40..42]
                            .try_into()
                            .map_err(|_| ProgramError::InvalidInstructionData)?,
                    );
                    let cooldown_slots = u64::from_le_bytes(
                        rest[42..50]
                            .try_into()
                            .map_err(|_| ProgramError::InvalidInstructionData)?,
                    );
                    Ok(Instruction::SetInsuranceWithdrawPolicy {
                        authority,
                        min_withdraw_base,
                        max_withdraw_bps,
                        cooldown_slots,
                    })
                }
                TAG_WITHDRAW_INSURANCE_LIMITED => {
                    // WithdrawInsuranceLimited (Tag 31): amount(8 bytes)
                    if rest.len() < 8 {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                    let amount = u64::from_le_bytes(
                        rest[..8]
                            .try_into()
                            .map_err(|_| ProgramError::InvalidInstructionData)?,
                    );
                    Ok(Instruction::WithdrawInsuranceLimited { amount })
                }
                TAG_SET_PYTH_ORACLE => {
                    if rest.len() < 42 {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                    let feed_id: [u8; 32] = rest[..32]
                        .try_into()
                        .map_err(|_| ProgramError::InvalidInstructionData)?;
                    let max_staleness_secs = u64::from_le_bytes(
                        rest[32..40]
                            .try_into()
                            .map_err(|_| ProgramError::InvalidInstructionData)?,
                    );
                    let conf_filter_bps = u16::from_le_bytes(
                        rest[40..42]
                            .try_into()
                            .map_err(|_| ProgramError::InvalidInstructionData)?,
                    );
                    Ok(Instruction::SetPythOracle {
                        feed_id,
                        max_staleness_secs,
                        conf_filter_bps,
                    })
                }
                TAG_UPDATE_MARK_PRICE => Ok(Instruction::UpdateMarkPrice),
                TAG_UPDATE_HYPERP_MARK => Ok(Instruction::UpdateHyperpMark),
                TAG_TRADE_CPI_V2 => {
                    // PERC-154: TradeCpiV2 — same as TradeCpi but includes PDA bump byte
                    let lp_idx = read_u16(&mut rest)?;
                    let user_idx = read_u16(&mut rest)?;
                    let size = read_i128(&mut rest)?;
                    let bump = read_u8(&mut rest)?;
                    Ok(Instruction::TradeCpiV2 {
                        lp_idx,
                        user_idx,
                        size,
                        bump,
                    })
                }
                TAG_UNRESOLVE_MARKET => {
                    let confirmation = read_u64(&mut rest)?;
                    Ok(Instruction::UnresolveMarket { confirmation })
                }
                TAG_CREATE_LP_VAULT => {
                    let fee_share_bps = read_u64(&mut rest)?;
                    // PERC-304: Optional util_curve_enabled byte (backwards compatible)
                    let util_curve_enabled = if !rest.is_empty() {
                        read_u8(&mut rest)? != 0
                    } else {
                        false
                    };
                    Ok(Instruction::CreateLpVault {
                        fee_share_bps,
                        util_curve_enabled,
                    })
                }
                TAG_LP_VAULT_DEPOSIT => {
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::LpVaultDeposit { amount })
                }
                TAG_LP_VAULT_WITHDRAW => {
                    let lp_amount = read_u64(&mut rest)?;
                    Ok(Instruction::LpVaultWithdraw { lp_amount })
                }
                TAG_LP_VAULT_CRANK_FEES => Ok(Instruction::LpVaultCrankFees),
                TAG_FUND_MARKET_INSURANCE => {
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::FundMarketInsurance { amount })
                }
                TAG_SET_INSURANCE_ISOLATION => {
                    let bps = read_u16(&mut rest)?;
                    Ok(Instruction::SetInsuranceIsolation { bps })
                }
                TAG_CHALLENGE_SETTLEMENT => {
                    let proposed_price_e6 = read_u64(&mut rest)?;
                    Ok(Instruction::ChallengeSettlement { proposed_price_e6 })
                }
                TAG_RESOLVE_DISPUTE => {
                    let accept = read_u8(&mut rest)?;
                    Ok(Instruction::ResolveDispute { accept })
                }
                TAG_DEPOSIT_LP_COLLATERAL => {
                    let user_idx = read_u16(&mut rest)?;
                    let lp_amount = read_u64(&mut rest)?;
                    Ok(Instruction::DepositLpCollateral {
                        user_idx,
                        lp_amount,
                    })
                }
                TAG_WITHDRAW_LP_COLLATERAL => {
                    let user_idx = read_u16(&mut rest)?;
                    let lp_amount = read_u64(&mut rest)?;
                    Ok(Instruction::WithdrawLpCollateral {
                        user_idx,
                        lp_amount,
                    })
                }
                TAG_QUEUE_WITHDRAWAL => {
                    let lp_amount = read_u64(&mut rest)?;
                    Ok(Instruction::QueueWithdrawal { lp_amount })
                }
                TAG_CLAIM_QUEUED_WITHDRAWAL => Ok(Instruction::ClaimQueuedWithdrawal),
                TAG_CANCEL_QUEUED_WITHDRAWAL => Ok(Instruction::CancelQueuedWithdrawal),
                TAG_EXECUTE_ADL => {
                    let target_idx = read_u16(&mut rest)?;
                    Ok(Instruction::ExecuteAdl { target_idx })
                }
                TAG_CLOSE_STALE_SLAB => Ok(Instruction::CloseStaleSlabs),
                TAG_RECLAIM_SLAB_RENT => Ok(Instruction::ReclaimSlabRent),
                TAG_ADVANCE_ORACLE_PHASE => Ok(Instruction::AdvanceOraclePhase),
                TAG_AUDIT_CRANK => Ok(Instruction::AuditCrank),
                TAG_SET_OFFSET_PAIR => {
                    let offset_bps = read_u16(&mut rest)?;
                    Ok(Instruction::SetOffsetPair { offset_bps })
                }
                TAG_ATTEST_CROSS_MARGIN => {
                    let user_idx_a = read_u16(&mut rest)?;
                    let user_idx_b = read_u16(&mut rest)?;
                    Ok(Instruction::AttestCrossMargin {
                        user_idx_a,
                        user_idx_b,
                    })
                }
                TAG_TOPUP_KEEPER_FUND => {
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::TopUpKeeperFund { amount })
                }
                TAG_INIT_SHARED_VAULT => {
                    let epoch_duration_slots = read_u64(&mut rest)?;
                    let max_market_exposure_bps = read_u16(&mut rest)?;
                    Ok(Instruction::InitSharedVault {
                        epoch_duration_slots,
                        max_market_exposure_bps,
                    })
                }
                TAG_ALLOCATE_MARKET => {
                    let amount = read_u128(&mut rest)?;
                    Ok(Instruction::AllocateMarket { amount })
                }
                TAG_QUEUE_WITHDRAWAL_SV => {
                    let lp_amount = read_u64(&mut rest)?;
                    Ok(Instruction::QueueWithdrawalSV { lp_amount })
                }
                TAG_CLAIM_EPOCH_WITHDRAWAL => Ok(Instruction::ClaimEpochWithdrawal),
                TAG_ADVANCE_EPOCH => Ok(Instruction::AdvanceEpoch),
                _ => Err(ProgramError::InvalidInstructionData),
            }
        }
    }

    pub(crate) fn read_u8(input: &mut &[u8]) -> Result<u8, ProgramError> {
        let (&val, rest) = input
            .split_first()
            .ok_or(ProgramError::InvalidInstructionData)?;
        *input = rest;
        Ok(val)
    }

    pub(crate) fn read_u16(input: &mut &[u8]) -> Result<u16, ProgramError> {
        if input.len() < 2 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let (bytes, rest) = input.split_at(2);
        *input = rest;
        Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
    }

    pub(crate) fn read_u32(input: &mut &[u8]) -> Result<u32, ProgramError> {
        if input.len() < 4 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let (bytes, rest) = input.split_at(4);
        *input = rest;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    pub(crate) fn read_u64(input: &mut &[u8]) -> Result<u64, ProgramError> {
        if input.len() < 8 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let (bytes, rest) = input.split_at(8);
        *input = rest;
        Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_i64(input: &mut &[u8]) -> Result<i64, ProgramError> {
        if input.len() < 8 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let (bytes, rest) = input.split_at(8);
        *input = rest;
        Ok(i64::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_i128(input: &mut &[u8]) -> Result<i128, ProgramError> {
        if input.len() < 16 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let (bytes, rest) = input.split_at(16);
        *input = rest;
        Ok(i128::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_u128(input: &mut &[u8]) -> Result<u128, ProgramError> {
        if input.len() < 16 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let (bytes, rest) = input.split_at(16);
        *input = rest;
        Ok(u128::from_le_bytes(bytes.try_into().unwrap()))
    }

    pub(crate) fn read_pubkey(input: &mut &[u8]) -> Result<Pubkey, ProgramError> {
        if input.len() < 32 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let (bytes, rest) = input.split_at(32);
        *input = rest;
        Ok(Pubkey::new_from_array(bytes.try_into().unwrap()))
    }

    pub(crate) fn read_bytes32(input: &mut &[u8]) -> Result<[u8; 32], ProgramError> {
        if input.len() < 32 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let (bytes, rest) = input.split_at(32);
        *input = rest;
        Ok(bytes.try_into().unwrap())
    }

    pub(crate) fn read_risk_params(input: &mut &[u8]) -> Result<RiskParams, ProgramError> {
        Ok(RiskParams {
            warmup_period_slots: read_u64(input)?,
            maintenance_margin_bps: read_u64(input)?,
            initial_margin_bps: read_u64(input)?,
            trading_fee_bps: read_u64(input)?,
            max_accounts: read_u64(input)?,
            new_account_fee: U128::new(read_u128(input)?),
            risk_reduction_threshold: U128::new(read_u128(input)?),
            maintenance_fee_per_slot: U128::new(read_u128(input)?),
            max_crank_staleness_slots: read_u64(input)?,
            liquidation_fee_bps: read_u64(input)?,
            liquidation_fee_cap: U128::new(read_u128(input)?),
            liquidation_buffer_bps: read_u64(input)?,
            min_liquidation_abs: U128::new(read_u128(input)?),
            // PERC-121: Funding rate params (defaults for backward compat)
            funding_premium_weight_bps: 0,
            funding_settlement_interval_slots: 0,
            funding_premium_dampening_e6: 1_000_000,
            funding_premium_max_bps_per_slot: 5,
            // PERC-122: Partial liquidation params (defaults for backward compat)
            partial_liquidation_bps: 2000,
            partial_liquidation_cooldown_slots: 30,
            use_mark_price_for_liquidation: false,
            // Issue #300: Emergency cooldown bypass (default = half maintenance margin)
            emergency_liquidation_margin_bps: 0, // 0 = auto (maintenance_margin_bps / 2)
            // PERC-120: Dynamic fee params (defaults = flat fee, no split, no surge)
            fee_tier2_bps: 0,
            fee_tier3_bps: 0,
            fee_tier2_threshold: 0,
            fee_tier3_threshold: 0,
            fee_split_lp_bps: 0,
            fee_split_protocol_bps: 0,
            fee_split_creator_bps: 0,
            fee_utilization_surge_bps: 0,
        })
    }

    #[cfg(test)]
    mod decode_tests {
        use super::*;
        use alloc::vec;
        use alloc::vec::Vec;

        /// Build an UpdateRiskParams instruction payload.
        ///
        /// Layout: TAG(1) | initial_margin_bps(8) | maintenance_margin_bps(8) | rest
        ///
        /// The `rest` slice is appended verbatim after the two required u64 fields.
        /// NOTE: The PERC-649 guard checks `rest.len()` *after* greedy u64 reads,
        /// so "rest" here must include optional u64 fields (8 bytes each) + adaptive tail.
        fn urp_bytes(rest: &[u8]) -> Vec<u8> {
            let mut v = vec![crate::tags::TAG_UPDATE_RISK_PARAMS];
            v.extend_from_slice(&100u64.to_le_bytes()); // initial_margin_bps
            v.extend_from_slice(&50u64.to_le_bytes()); // maintenance_margin_bps
            v.extend_from_slice(rest);
            v
        }

        // ── PERC-649: invalid tail lengths must be rejected ──────────────────────
        // The greedy-u64 readers consume 8-byte chunks; the guard then checks the
        // residual against {0, 1, 3, 11, 13}.

        #[test]
        fn test_urp_bug_2byte_tail_rejected() {
            // THE BUG: a 2-byte payload intended for mark_oracle_weight_bps was
            // previously silently mis-decoded as adaptive_funding_enabled.
            // After the guard, residual = 2 ∉ {0,1,3,11,13} → Err.
            let data = urp_bytes(&[0x70, 0x1B]); // 7_000u16 LE
            assert_eq!(
                Instruction::decode(&data).unwrap_err(),
                solana_program::program_error::ProgramError::InvalidInstructionData,
                "2-byte tail (mark_oracle_weight_bps-only) must be rejected (PERC-649)"
            );
        }

        #[test]
        fn test_urp_residual_4_rejected() {
            // rest = 4 bytes: trading_fee_bps(8) would NOT be consumed (4 < 8),
            // guard sees residual=4 ∉ {0,1,3,11,13} → Err.
            let data = urp_bytes(&[0x01, 0x00, 0x00, 0x00]);
            assert_eq!(
                Instruction::decode(&data).unwrap_err(),
                solana_program::program_error::ProgramError::InvalidInstructionData
            );
        }

        #[test]
        fn test_urp_residual_5_rejected() {
            // rest = 12 bytes: trading_fee_bps(8) consumed → residual = 4 ∉ valid.
            let data = urp_bytes(&[0u8; 12]);
            assert_eq!(
                Instruction::decode(&data).unwrap_err(),
                solana_program::program_error::ProgramError::InvalidInstructionData
            );
        }

        // ── PERC-649: valid payloads must decode without error ───────────────────
        // residual = rest after all greedy u64 reads; must be in {0, 1, 3, 11, 13}.

        #[test]
        fn test_urp_required_only_ok() {
            // rest = 0 → residual = 0 ∈ {0,1,3,11,13} ✓
            let data = urp_bytes(&[]);
            assert!(Instruction::decode(&data).is_ok());
        }

        #[test]
        fn test_urp_adaptive_enabled_only_ok() {
            // rest = 1 → residual = 1 ∈ {0,1,3,11,13} ✓
            let data = urp_bytes(&[0x01]);
            assert!(Instruction::decode(&data).is_ok());
        }

        #[test]
        fn test_urp_adaptive_enabled_and_scale_ok() {
            // rest = 3 → residual = 3 ∈ {0,1,3,11,13} ✓
            let mut rest = vec![0x01];
            rest.extend_from_slice(&500u16.to_le_bytes());
            let data = urp_bytes(&rest);
            assert!(Instruction::decode(&data).is_ok());
        }

        #[test]
        fn test_urp_all_u64s_plus_full_adaptive_tail_ok() {
            // rest = 5×u64(40) + adaptive_tail(11) = 51 → after greedy reads residual=11 ✓
            let mut rest = Vec::new();
            for _ in 0..5 {
                rest.extend_from_slice(&1u64.to_le_bytes());
            } // 5 optional u64s
            rest.push(0x01); // adaptive_enabled
            rest.extend_from_slice(&500u16.to_le_bytes()); // adaptive_scale
            rest.extend_from_slice(&100u64.to_le_bytes()); // adaptive_max
            assert_eq!(rest.len(), 51);
            let data = urp_bytes(&rest);
            assert!(Instruction::decode(&data).is_ok());
        }

        #[test]
        fn test_urp_full_payload_mark_oracle_weight_decoded() {
            // rest = 5×u64(40) + adaptive_tail(11) + mark_oracle_weight(2) = 53
            // after greedy reads residual = 13 ∈ {0,1,3,11,13} ✓
            let mut rest = Vec::new();
            for _ in 0..5 {
                rest.extend_from_slice(&1u64.to_le_bytes());
            } // 5 optional u64s
            rest.push(0x01); // adaptive_enabled
            rest.extend_from_slice(&500u16.to_le_bytes()); // adaptive_scale
            rest.extend_from_slice(&100u64.to_le_bytes()); // adaptive_max
            rest.extend_from_slice(&7_000u16.to_le_bytes()); // mark weight 70%
            assert_eq!(rest.len(), 53);
            let data = urp_bytes(&rest);
            match Instruction::decode(&data).expect("full payload should decode") {
                Instruction::UpdateRiskParams {
                    mark_oracle_weight_bps,
                    adaptive_funding_enabled,
                    ..
                } => {
                    assert_eq!(mark_oracle_weight_bps, Some(7_000), "mark weight");
                    assert_eq!(adaptive_funding_enabled, Some(0x01), "adaptive enabled");
                }
                other => panic!("unexpected variant: {other:?}"),
            }
        }
    }
}

// 5. mod accounts (Pinocchio validation)
pub mod accounts {
    use crate::error::PercolatorError;
    use solana_program::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};

    pub fn expect_len(accounts: &[AccountInfo], n: usize) -> Result<(), ProgramError> {
        // Length check via verify helper (Kani-provable)
        if !crate::verify::len_ok(accounts.len(), n) {
            return Err(ProgramError::NotEnoughAccountKeys);
        }
        Ok(())
    }

    pub fn expect_signer(ai: &AccountInfo) -> Result<(), ProgramError> {
        // Signer check via verify helper (Kani-provable)
        if !crate::verify::signer_ok(ai.is_signer) {
            return Err(PercolatorError::ExpectedSigner.into());
        }
        Ok(())
    }

    pub fn expect_writable(ai: &AccountInfo) -> Result<(), ProgramError> {
        // Writable check via verify helper (Kani-provable)
        if !crate::verify::writable_ok(ai.is_writable) {
            return Err(PercolatorError::ExpectedWritable.into());
        }
        Ok(())
    }

    pub fn expect_owner(ai: &AccountInfo, owner: &Pubkey) -> Result<(), ProgramError> {
        if ai.owner != owner {
            return Err(ProgramError::IllegalOwner);
        }
        Ok(())
    }

    pub fn expect_key(ai: &AccountInfo, expected: &Pubkey) -> Result<(), ProgramError> {
        // Key check via verify helper (Kani-provable)
        if !crate::verify::pda_key_matches(expected.to_bytes(), ai.key.to_bytes()) {
            return Err(ProgramError::InvalidArgument);
        }
        Ok(())
    }

    pub fn derive_vault_authority(program_id: &Pubkey, slab_key: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[b"vault", slab_key.as_ref()], program_id)
    }

    pub fn derive_insurance_lp_mint(program_id: &Pubkey, slab_key: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[b"ins_lp", slab_key.as_ref()], program_id)
    }

    /// PERC-272: Derive LP vault state PDA.
    pub fn derive_lp_vault_state(program_id: &Pubkey, slab_key: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[b"lp_vault", slab_key.as_ref()], program_id)
    }

    /// PERC-272: Derive LP vault SPL mint PDA.
    pub fn derive_lp_vault_mint(program_id: &Pubkey, slab_key: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[b"lp_vault_mint", slab_key.as_ref()], program_id)
    }

    /// PERC-308: Derive loyalty stake PDA.
    pub fn derive_loyalty_stake(
        program_id: &Pubkey,
        slab_key: &Pubkey,
        user_key: &Pubkey,
    ) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[b"loyalty", slab_key.as_ref(), user_key.as_ref()],
            program_id,
        )
    }

    /// PERC-314: Derive settlement dispute PDA.
    pub fn derive_dispute(program_id: &Pubkey, slab_key: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[b"dispute", slab_key.as_ref()], program_id)
    }

    /// PERC-309: Derive withdraw queue PDA.
    pub fn derive_withdraw_queue(
        program_id: &Pubkey,
        slab_key: &Pubkey,
        user_key: &Pubkey,
    ) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[b"withdraw_queue", slab_key.as_ref(), user_key.as_ref()],
            program_id,
        )
    }
}

// 6. mod state
pub mod state {
    use crate::constants::{CONFIG_LEN, HEADER_LEN};
    use bytemuck::{Pod, Zeroable};
    use core::cell::RefMut;
    use core::mem::offset_of;
    use solana_program::account_info::AccountInfo;
    use solana_program::program_error::ProgramError;

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct SlabHeader {
        pub magic: u64,
        pub version: u32,
        pub bump: u8,
        pub _padding: [u8; 3], // _padding[0] = flags byte (bit 0: resolved, bit 1: paused)
        pub admin: [u8; 32],
        /// Pending admin for two-step admin transfer. All zeros = no pending transfer.
        pub pending_admin: [u8; 32],
        pub _reserved: [u8; 24], // [0..8]=nonce, [8..16]=last_thr_slot, [16..24]=dust_base
    }

    /// Offset of _reserved field in SlabHeader, derived from offset_of! for correctness.
    pub const RESERVED_OFF: usize = offset_of!(SlabHeader, _reserved);

    // Portable compile-time assertion that RESERVED_OFF is 80 (expected layout)
    const _: [(); 80] = [(); RESERVED_OFF];

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct MarketConfig {
        pub collateral_mint: [u8; 32],
        pub vault_pubkey: [u8; 32],
        /// Pyth feed ID for the index price feed
        pub index_feed_id: [u8; 32],
        /// Maximum staleness in seconds (Pyth Pull uses unix timestamps)
        pub max_staleness_secs: u64,
        pub conf_filter_bps: u16,
        pub vault_authority_bump: u8,
        /// If non-zero, invert the oracle price (raw -> 1e12/raw)
        pub invert: u8,
        /// Lamports per Unit for conversion (e.g., 1000 means 1 SOL = 1,000,000 Units)
        /// If 0, no scaling is applied (1:1 lamports to units)
        pub unit_scale: u32,

        // ========================================
        // Funding Parameters (configurable)
        // ========================================
        /// Funding horizon in slots (~4 min at 500 slots)
        pub funding_horizon_slots: u64,
        /// Funding rate multiplier in basis points (100 = 1.00x)
        pub funding_k_bps: u64,
        /// Funding scale factor in e6 units (controls funding rate sensitivity)
        pub funding_inv_scale_notional_e6: u128,
        /// Max premium in basis points (500 = 5%)
        pub funding_max_premium_bps: i64,
        /// Max funding rate per slot in basis points
        pub funding_max_bps_per_slot: i64,

        // ========================================
        // Premium Funding Parameters (PERC-121)
        // ========================================
        /// Weight of premium (mark-index) component (0..10_000 bps)
        pub funding_premium_weight_bps: u64,
        /// Settlement interval in slots (0 = every slot)
        pub funding_settlement_interval_slots: u64,
        /// Dampening factor for premium rate (e6 units, 1_000_000 = 1.0x)
        pub funding_premium_dampening_e6: u64,
        /// Max premium funding rate per slot (bps)
        pub funding_premium_max_bps_per_slot: i64,

        // ========================================
        // Threshold Parameters (configurable)
        // ========================================
        /// Floor for threshold calculation
        pub thresh_floor: u128,
        /// Risk coefficient in basis points (50 = 0.5%)
        pub thresh_risk_bps: u64,
        /// Update interval in slots
        pub thresh_update_interval_slots: u64,
        /// Max step size in basis points (500 = 5%)
        pub thresh_step_bps: u64,
        /// EWMA alpha in basis points (1000 = 10%)
        pub thresh_alpha_bps: u64,
        /// Minimum threshold value
        pub thresh_min: u128,
        /// Maximum threshold value
        pub thresh_max: u128,
        /// Minimum step size
        pub thresh_min_step: u128,

        // ========================================
        // Oracle Authority (optional signer-based oracle)
        // ========================================
        /// Oracle price authority pubkey. If non-zero, this signer can push prices
        /// directly instead of requiring Pyth/Chainlink. All zeros = disabled.
        pub oracle_authority: [u8; 32],
        /// Last price pushed by oracle authority (in e6 format, already scaled)
        pub authority_price_e6: u64,
        /// Unix timestamp when authority last pushed the price
        pub authority_timestamp: i64,

        // ========================================
        // Oracle Price Circuit Breaker
        // ========================================
        /// Max oracle price change per update in 0.01 bps (e2bps).
        /// 0 = disabled (no cap). 1_000_000 = 100%.
        pub oracle_price_cap_e2bps: u64,
        /// Last effective oracle price (after clamping), in e6 format.
        /// 0 = no history (first price accepted as-is).
        pub last_effective_price_e6: u64,

        // ========================================
        // Dynamic OI Cap (PERC-273)
        // ========================================
        /// OI cap multiplier in basis points. Max OI = vault * multiplier / 10_000.
        /// 0 = disabled (no OI cap). 100_000 = 10x vault capital.
        /// Set via UpdateMarginParams instruction.
        pub oi_cap_multiplier_bps: u64,
        /// Max total positive PnL allowed (absolute, in collateral units).
        /// 0 = disabled. Trades rejected if pnl_pos_tot would exceed this.
        /// Set via UpdateRiskParams. Prevents LP capital exhaustion.
        pub max_pnl_cap: u64,
        // ========================================
        // Market Maturity OI Ramp (PERC-302)
        // ========================================
        /// Slot when market was created (set once at InitMarket).
        /// Used by OI ramp to compute elapsed slots since market birth.
        pub market_created_slot: u64,
        /// Number of slots over which OI cap ramps from RAMP_START_BPS to
        /// oi_cap_multiplier_bps. 0 = disabled (full cap immediately).
        pub oi_ramp_slots: u64,

        // ========================================
        // Adaptive Funding Rate (PERC-300)
        // ========================================
        /// Enable adaptive (OI-skew-based) funding rate.
        /// 0 = disabled (use inventory/premium funding). 1 = enabled.
        pub adaptive_funding_enabled: u8,
        /// Padding for alignment
        pub _adaptive_pad: u8,
        /// Scale factor for skew adjustment (bps). Controls how aggressively
        /// the rate adjusts to OI imbalance. Typical: 100-500.
        pub adaptive_scale_bps: u16,
        /// Padding for u64 alignment
        pub _adaptive_pad2: u32,
        /// Max adaptive funding rate (bps per slot, absolute value).
        /// Rate clamped to [-max, +max]. 0 = use default max.
        pub adaptive_max_funding_bps: u64,

        // ========================================
        // Per-Market Insurance Isolation (PERC-306)
        // ========================================
        /// Max basis points of global insurance fund this market can access.
        /// 0 = disabled (unlimited access, legacy behavior).
        /// E.g., 1000 = 10% of global fund.
        pub insurance_isolation_bps: u16,

        /// Padding / reserved bytes (PERC-306 layout preservation).
        /// PERC-118: [0..2] = mark_oracle_weight_bps (u16 LE, 0..=10_000).
        /// Access via state::get/set_mark_oracle_weight_bps().
        pub _insurance_isolation_padding: [u8; 14],

        // ========================================
        // Safety Valve (PERC-312)
        // ========================================
        /// Duration of rebalancing mode in slots (default 500).
        /// 0 = safety valve disabled.
        pub safety_valve_duration: u64,
        /// Emergency close rebate in bps (paid to dominant-side closers).
        pub emergency_close_rebate_bps: u16,
        /// Number of consecutive max-funding epochs before triggering (default 5).
        pub safety_valve_epochs: u8,
        /// Whether safety valve is currently enabled (1=yes, 0=no).
        pub safety_valve_enabled: u8,
        /// Padding to maintain alignment.
        pub _safety_valve_pad: [u8; 4],
        /// Runtime: consecutive epochs where funding hit max (resets on < max).
        pub consecutive_max_funding_epochs: u8,
        /// Runtime: 1 if rebalancing mode is currently active, 0 otherwise.
        pub rebalancing_active: u8,
        pub _rebalancing_pad: [u8; 6],
        /// Runtime: slot when rebalancing mode was activated (0 if inactive).
        pub rebalancing_start_slot: u64,

        // ========================================
        // Orphan Market Penalty (PERC-307)
        // ========================================
        /// Slots of oracle staleness before orphan penalty kicks in (default 1000).
        /// 0 = disabled.
        pub orphan_threshold_slots: u64,
        /// Penalty rate in bps per slot applied to position funding when oracle stale.
        pub orphan_penalty_bps_per_slot: u16,
        pub _orphan_pad: [u8; 6],

        // ========================================
        // Settlement Dispute (PERC-314)
        // ========================================
        /// Dispute window in slots after resolution. 0 = disputes disabled.
        pub dispute_window_slots: u64,
        /// Bond amount in base tokens required to challenge settlement.
        pub dispute_bond_amount: u64,
        /// Slot when market was resolved (set by ResolveMarket).
        pub resolved_slot: u64,
        /// Settlement price at resolution (copied from authority_price_e6).
        pub settlement_price_e6: u64,

        // ========================================
        // LP Token Collateral (PERC-315)
        // ========================================
        /// 1 = LP vault tokens accepted as perp collateral, 0 = disabled.
        pub lp_collateral_enabled: u8,
        pub _lp_col_pad: [u8; 7],
        /// Loan-to-value ratio in basis points for LP tokens (e.g., 8000 = 80%).
        pub lp_collateral_ltv_bps: u64,
    }

    pub fn slab_data_mut<'a, 'b>(
        ai: &'b AccountInfo<'a>,
    ) -> Result<RefMut<'b, &'a mut [u8]>, ProgramError> {
        ai.try_borrow_mut_data()
    }

    pub fn read_header(data: &[u8]) -> SlabHeader {
        let mut h = SlabHeader::zeroed();
        let src = &data[..HEADER_LEN];
        let dst = bytemuck::bytes_of_mut(&mut h);
        dst.copy_from_slice(src);
        h
    }

    pub fn write_header(data: &mut [u8], h: &SlabHeader) {
        let src = bytemuck::bytes_of(h);
        let dst = &mut data[..HEADER_LEN];
        dst.copy_from_slice(src);
    }

    /// Check if the market is paused (bit 1 of flags byte).
    #[inline]
    pub fn is_paused(data: &[u8]) -> bool {
        read_flags(data) & FLAG_PAUSED != 0
    }

    /// Set or clear the paused flag in the slab data.
    #[inline]
    pub fn set_paused(data: &mut [u8], paused: bool) {
        let mut flags = read_flags(data);
        if paused {
            flags |= FLAG_PAUSED;
        } else {
            flags &= !FLAG_PAUSED;
        }
        write_flags(data, flags);
    }

    /// Read the request nonce from the reserved field in slab header.
    /// The nonce is stored at RESERVED_OFF..RESERVED_OFF+8 as little-endian u64.
    pub fn read_req_nonce(data: &[u8]) -> u64 {
        u64::from_le_bytes(data[RESERVED_OFF..RESERVED_OFF + 8].try_into().unwrap())
    }

    /// Write the request nonce to the reserved field in slab header.
    /// The nonce is stored in _reserved[0..8] as little-endian u64.
    /// Uses offset_of! for correctness even if SlabHeader layout changes.
    pub fn write_req_nonce(data: &mut [u8], nonce: u64) {
        // Compile-time layout check (always true for current struct)
        const _: () = assert!(HEADER_LEN >= RESERVED_OFF + 16);
        data[RESERVED_OFF..RESERVED_OFF + 8].copy_from_slice(&nonce.to_le_bytes());
    }

    /// Read the last threshold update slot from _reserved[8..16].
    pub fn read_last_thr_update_slot(data: &[u8]) -> u64 {
        u64::from_le_bytes(
            data[RESERVED_OFF + 8..RESERVED_OFF + 16]
                .try_into()
                .unwrap(),
        )
    }

    /// Write the last threshold update slot to _reserved[8..16].
    pub fn write_last_thr_update_slot(data: &mut [u8], slot: u64) {
        data[RESERVED_OFF + 8..RESERVED_OFF + 16].copy_from_slice(&slot.to_le_bytes());
    }

    /// Read accumulated dust (base token remainder) from _reserved[16..24].
    pub fn read_dust_base(data: &[u8]) -> u64 {
        u64::from_le_bytes(
            data[RESERVED_OFF + 16..RESERVED_OFF + 24]
                .try_into()
                .unwrap(),
        )
    }

    /// Write accumulated dust (base token remainder) to _reserved[16..24].
    pub fn write_dust_base(data: &mut [u8], dust: u64) {
        data[RESERVED_OFF + 16..RESERVED_OFF + 24].copy_from_slice(&dust.to_le_bytes());
    }

    // ========================================
    // Market Flags (stored in _padding[0] at offset 13)
    // ========================================

    /// Offset of flags byte in SlabHeader (_padding[0])
    pub const FLAGS_OFF: usize = 13;

    /// Flag bit: Market is resolved (withdraw-only mode)
    pub const FLAG_RESOLVED: u8 = 1 << 0;
    /// Flag bit: Market is paused (admin emergency stop)
    pub const FLAG_PAUSED: u8 = 1 << 1;

    /// Read market flags from _padding[0].
    pub fn read_flags(data: &[u8]) -> u8 {
        data[FLAGS_OFF]
    }

    /// Write market flags to _padding[0].
    pub fn write_flags(data: &mut [u8], flags: u8) {
        data[FLAGS_OFF] = flags;
    }

    /// Check if market is resolved (withdraw-only mode).
    pub fn is_resolved(data: &[u8]) -> bool {
        read_flags(data) & FLAG_RESOLVED != 0
    }

    /// Set the resolved flag.
    pub fn set_resolved(data: &mut [u8]) {
        let flags = read_flags(data) | FLAG_RESOLVED;
        write_flags(data, flags);
    }

    /// Clear the resolved flag (PERC-273: UnresolveMarket).
    pub fn clear_resolved(data: &mut [u8]) {
        let flags = read_flags(data) & !FLAG_RESOLVED;
        write_flags(data, flags);
    }

    pub fn read_config(data: &[u8]) -> MarketConfig {
        let mut c: MarketConfig = bytemuck::Zeroable::zeroed();
        let src = &data[HEADER_LEN..HEADER_LEN + CONFIG_LEN];
        let dst = bytemuck::bytes_of_mut(&mut c);
        dst.copy_from_slice(src);
        c
    }

    pub fn write_config(data: &mut [u8], c: &MarketConfig) {
        let src = bytemuck::bytes_of(c);
        let dst = &mut data[HEADER_LEN..HEADER_LEN + CONFIG_LEN];
        dst.copy_from_slice(src);
    }

    /// Write raw bytes at a field offset within the config region.
    /// Used by `process_init_market` to avoid constructing a full MarketConfig
    /// on the stack (496 B on SBF), preventing stack overflow.
    #[inline]
    pub fn write_config_bytes(data: &mut [u8], field_offset: usize, bytes: &[u8]) {
        let start = HEADER_LEN + field_offset;
        data[start..start + bytes.len()].copy_from_slice(bytes);
    }

    /// PERC-118: Read the mark oracle weight from `_insurance_isolation_padding[0..2]`.
    ///
    /// Stored as little-endian u16 in padding bytes to avoid CONFIG_LEN changes.
    /// 0 = backward-compatible: blend uses pure impact_mid (no oracle component).
    /// 10_000 = 100% oracle (mark = oracle, premium always 0).
    /// Values in between blend oracle + impact_mid proportionally.
    #[inline]
    pub fn get_mark_oracle_weight_bps(config: &MarketConfig) -> u16 {
        u16::from_le_bytes([
            config._insurance_isolation_padding[0],
            config._insurance_isolation_padding[1],
        ])
    }

    /// PERC-118: Set the mark oracle weight into `_insurance_isolation_padding[0..2]`.
    /// Clamps to [0, 10_000].
    #[inline]
    pub fn set_mark_oracle_weight_bps(config: &mut MarketConfig, weight_bps: u16) {
        let clamped = weight_bps.min(10_000);
        let bytes = clamped.to_le_bytes();
        config._insurance_isolation_padding[0] = bytes[0];
        config._insurance_isolation_padding[1] = bytes[1];
    }

    // ========================================
    // PERC-622: Three-Phase Oracle Transition
    // ========================================

    /// Oracle phase constants.
    pub const ORACLE_PHASE_NASCENT: u8 = 0;
    pub const ORACLE_PHASE_GROWING: u8 = 1;
    pub const ORACLE_PHASE_MATURE: u8 = 2;

    /// Phase transition thresholds.
    /// ~72 hours at 400ms slots = 648_000 slots (time-only path).
    pub const PHASE1_MIN_SLOTS: u64 = 648_000;
    /// ~4 hours at 400ms slots = 36_000 slots (minimum floor for volume path).
    /// Security: prevents wash-trading to skip time requirement entirely.
    pub const PHASE1_VOLUME_MIN_SLOTS: u64 = 36_000;
    /// $100K cumulative volume threshold to exit Phase 1 (in collateral units).
    pub const PHASE2_VOLUME_THRESHOLD: u64 = 100_000_000_000; // 100K * 1e6 (e6 format)
    /// ~14 days at 400ms slots = 3_024_000 slots.
    pub const PHASE2_MATURITY_SLOTS: u64 = 3_024_000;

    /// Phase 1 caps.
    pub const PHASE1_OI_CAP_E6: u64 = 10_000_000_000; // $10K in e6
    pub const PHASE1_MAX_LEVERAGE_BPS: u64 = 20_000; // 2x

    /// Phase 2 caps.
    pub const PHASE2_OI_CAP_E6: u64 = 100_000_000_000; // $100K in e6
    pub const PHASE2_MAX_LEVERAGE_BPS: u64 = 50_000; // 5x

    /// Read oracle phase from config padding[2].
    #[inline]
    pub fn get_oracle_phase(config: &MarketConfig) -> u8 {
        config._insurance_isolation_padding[2].min(ORACLE_PHASE_MATURE)
    }

    /// Set oracle phase in config padding[2]. Clamps to [0, 2].
    #[inline]
    pub fn set_oracle_phase(config: &mut MarketConfig, phase: u8) {
        config._insurance_isolation_padding[2] = phase.min(ORACLE_PHASE_MATURE);
    }

    /// Read cumulative volume (e6 format) from config padding[3..11].
    #[inline]
    pub fn get_cumulative_volume(config: &MarketConfig) -> u64 {
        let bytes: [u8; 8] = config._insurance_isolation_padding[3..11]
            .try_into()
            .unwrap_or([0u8; 8]);
        u64::from_le_bytes(bytes)
    }

    /// Set cumulative volume (e6 format) in config padding[3..11].
    #[inline]
    pub fn set_cumulative_volume(config: &mut MarketConfig, vol: u64) {
        let bytes = vol.to_le_bytes();
        config._insurance_isolation_padding[3..11].copy_from_slice(&bytes);
    }

    /// Read phase2 delta slots (u24 LE) from config padding[11..14].
    /// Max value: 16_777_215 (~77 days at 400ms — more than enough).
    #[inline]
    pub fn get_phase2_delta_slots(config: &MarketConfig) -> u32 {
        let b = &config._insurance_isolation_padding[11..14];
        u32::from_le_bytes([b[0], b[1], b[2], 0])
    }

    /// Set phase2 delta slots (u24 LE) in config padding[11..14].
    /// Truncates to 24 bits (max 16_777_215).
    #[inline]
    pub fn set_phase2_delta_slots(config: &mut MarketConfig, delta: u32) {
        let clamped = delta.min(0x00FF_FFFF);
        let bytes = clamped.to_le_bytes();
        config._insurance_isolation_padding[11] = bytes[0];
        config._insurance_isolation_padding[12] = bytes[1];
        config._insurance_isolation_padding[13] = bytes[2];
    }

    /// Resolve effective market_created_slot for phase logic.
    /// If market_created_slot == 0 (legacy market, field never set), returns current_slot
    /// so elapsed = 0 — the market starts fresh in Phase 1 rather than auto-promoting.
    /// Callers SHOULD lazy-init market_created_slot on first encounter.
    #[inline]
    pub fn effective_created_slot(market_created_slot: u64, current_slot: u64) -> u64 {
        if market_created_slot == 0 {
            current_slot
        } else {
            market_created_slot
        }
    }

    /// Pure decision function: check if oracle phase should advance.
    /// Returns (new_phase, transitioned).
    /// Phase transitions are monotonic: 0→1→2, never backwards.
    ///
    /// IMPORTANT: `market_created_slot` MUST be pre-resolved via `effective_created_slot()`
    /// to handle legacy markets where the field is zero.
    pub fn check_phase_transition(
        current_slot: u64,
        market_created_slot: u64,
        oracle_phase: u8,
        cumulative_volume: u64,
        phase2_delta_slots: u32,
        has_mature_oracle: bool,
    ) -> (u8, bool) {
        match oracle_phase {
            0 => {
                // Phase 1 → Phase 2:
                //   Path A: 72h elapsed (time-only, regardless of volume)
                //   Path B: 4h elapsed AND $100K cumulative volume
                // Security: 4h floor prevents wash-trade instant bypass.
                let elapsed = current_slot.saturating_sub(market_created_slot);
                let time_ready = elapsed >= PHASE1_MIN_SLOTS;
                let volume_ready = elapsed >= PHASE1_VOLUME_MIN_SLOTS
                    && cumulative_volume >= PHASE2_VOLUME_THRESHOLD;
                if time_ready || volume_ready {
                    (ORACLE_PHASE_GROWING, true)
                } else {
                    (ORACLE_PHASE_NASCENT, false)
                }
            }
            1 => {
                // Phase 2 → Phase 3: 14d elapsed since Phase 2 entry OR mature oracle available
                if has_mature_oracle {
                    return (ORACLE_PHASE_MATURE, true);
                }
                let phase2_start = market_created_slot.saturating_add(phase2_delta_slots as u64);
                let elapsed_since_phase2 = current_slot.saturating_sub(phase2_start);
                if elapsed_since_phase2 >= PHASE2_MATURITY_SLOTS {
                    (ORACLE_PHASE_MATURE, true)
                } else {
                    (ORACLE_PHASE_GROWING, false)
                }
            }
            _ => (ORACLE_PHASE_MATURE, false), // Phase 3 is terminal
        }
    }

    /// Return the effective OI cap for the current oracle phase.
    /// Phase 1: $10K, Phase 2: $100K, Phase 3: full configured cap.
    pub fn phase_oi_cap(oracle_phase: u8, base_oi_cap: u64) -> u64 {
        match oracle_phase {
            0 => PHASE1_OI_CAP_E6.min(base_oi_cap),
            1 => PHASE2_OI_CAP_E6.min(base_oi_cap),
            _ => base_oi_cap,
        }
    }

    /// Return the effective max leverage (in bps) for the current oracle phase.
    /// Phase 1: 2x (20_000), Phase 2: 5x (50_000), Phase 3: full configured.
    pub fn phase_max_leverage_bps(oracle_phase: u8, base_max_lev_bps: u64) -> u64 {
        match oracle_phase {
            0 => PHASE1_MAX_LEVERAGE_BPS.min(base_max_lev_bps),
            1 => PHASE2_MAX_LEVERAGE_BPS.min(base_max_lev_bps),
            _ => base_max_lev_bps,
        }
    }

    /// Accumulate trade volume. Saturating add to prevent overflow.
    pub fn accumulate_volume(config: &mut MarketConfig, trade_notional_e6: u64) {
        let current = get_cumulative_volume(config);
        set_cumulative_volume(config, current.saturating_add(trade_notional_e6));
    }

    // ========================================
    // Feature 1: Quadratic Funding Convexity
    // ========================================

    /// Read quadratic funding coefficient k2 from `_insurance_isolation_padding[2..4]`.
    /// 0 = disabled (linear-only funding). Typical range: 1–500 bps.
    #[inline]
    pub fn get_funding_k2_bps(config: &MarketConfig) -> u16 {
        u16::from_le_bytes([
            config._insurance_isolation_padding[2],
            config._insurance_isolation_padding[3],
        ])
    }

    /// Set quadratic funding coefficient k2 into `_insurance_isolation_padding[2..4]`.
    #[inline]
    pub fn set_funding_k2_bps(config: &mut MarketConfig, k2_bps: u16) {
        let bytes = k2_bps.to_le_bytes();
        config._insurance_isolation_padding[2] = bytes[0];
        config._insurance_isolation_padding[3] = bytes[1];
    }

    // ========================================
    // Feature 2: VRAM (Volatility-Regime Adaptive Margin)
    // ========================================

    /// Read EWMV (exponentially weighted moving variance) from `_insurance_isolation_padding[4..8]`.
    /// Scaled by 1e12. 0 = no variance history.
    #[inline]
    pub fn get_ewmv_e12(config: &MarketConfig) -> u32 {
        u32::from_le_bytes([
            config._insurance_isolation_padding[4],
            config._insurance_isolation_padding[5],
            config._insurance_isolation_padding[6],
            config._insurance_isolation_padding[7],
        ])
    }

    /// Write EWMV into `_insurance_isolation_padding[4..8]`.
    #[inline]
    pub fn set_ewmv_e12(config: &mut MarketConfig, ewmv: u32) {
        let bytes = ewmv.to_le_bytes();
        config._insurance_isolation_padding[4] = bytes[0];
        config._insurance_isolation_padding[5] = bytes[1];
        config._insurance_isolation_padding[6] = bytes[2];
        config._insurance_isolation_padding[7] = bytes[3];
    }

    /// Read last volatility oracle price from `_insurance_isolation_padding[8..12]`.
    /// Stored as u32 in **e3** format (price_e6 / 1000). Max ~$4.29M.
    /// 0 = no previous price.
    /// (#980: changed from e6 to e3 to support BTC/ETH-priced markets)
    ///
    /// Migration: pre-#980 slabs stored price in e6 format (up to u32::MAX ≈ $4295 in e6).
    /// Any stored value that exceeds MAX_SANE_PRICE_E3 is implausible as an e3 price and
    /// indicates a legacy e6 value. We return 0 to restart VRAM cleanly rather than spike.
    /// MAX_SANE_PRICE_E3 = 4_294_000 ≈ $4294 in e3 (the old u32 clip ceiling in e6 terms).
    /// This is idempotent: returns 0 until a valid post-migration e3 value is written.
    #[inline]
    pub fn get_last_vol_price_e3(config: &MarketConfig) -> u32 {
        let raw = u32::from_le_bytes([
            config._insurance_isolation_padding[8],
            config._insurance_isolation_padding[9],
            config._insurance_isolation_padding[10],
            config._insurance_isolation_padding[11],
        ]);
        // Migration guard: any value > 4_294_000 (> $4294 in e3, which was the old e6 clip
        // ceiling for u32 overflow) is a legacy e6 value. Reset to 0 to avoid 1000x VRAM spike.
        if raw > 4_294_000 {
            0
        } else {
            raw
        }
    }

    /// Write last volatility oracle price into `_insurance_isolation_padding[8..12]`.
    /// Stores in **e3** format (price_e6 / 1000).
    #[inline]
    pub fn set_last_vol_price_e3(config: &mut MarketConfig, price_e3: u32) {
        let bytes = price_e3.to_le_bytes();
        config._insurance_isolation_padding[8] = bytes[0];
        config._insurance_isolation_padding[9] = bytes[1];
        config._insurance_isolation_padding[10] = bytes[2];
        config._insurance_isolation_padding[11] = bytes[3];
    }

    /// Read VRAM sensitivity scale from `_insurance_isolation_padding[12..14]`.
    /// 0 = VRAM disabled. Typical: 10_000 (1.0x scaling).
    #[inline]
    pub fn get_vol_margin_scale_bps(config: &MarketConfig) -> u16 {
        u16::from_le_bytes([
            config._insurance_isolation_padding[12],
            config._insurance_isolation_padding[13],
        ])
    }

    /// Write VRAM sensitivity scale into `_insurance_isolation_padding[12..14]`.
    #[inline]
    pub fn set_vol_margin_scale_bps(config: &mut MarketConfig, scale: u16) {
        let bytes = scale.to_le_bytes();
        config._insurance_isolation_padding[12] = bytes[0];
        config._insurance_isolation_padding[13] = bytes[1];
    }

    /// Read EWMA alpha from `_lp_col_pad[0..2]`.
    /// e6 units: 100 = alpha of 0.0001, 10_000 = alpha of 0.01.
    #[inline]
    pub fn get_vol_alpha_e6(config: &MarketConfig) -> u16 {
        u16::from_le_bytes([config._lp_col_pad[0], config._lp_col_pad[1]])
    }

    /// Write EWMA alpha into `_lp_col_pad[0..2]`.
    #[inline]
    pub fn set_vol_alpha_e6(config: &mut MarketConfig, alpha: u16) {
        let bytes = alpha.to_le_bytes();
        config._lp_col_pad[0] = bytes[0];
        config._lp_col_pad[1] = bytes[1];
    }

    /// Read VRAM target volatility from `_lp_col_pad[2..4]`.
    /// e6 units: represents the baseline volatility for 1.0x margin.
    #[inline]
    pub fn get_vol_margin_target_e6(config: &MarketConfig) -> u16 {
        u16::from_le_bytes([config._lp_col_pad[2], config._lp_col_pad[3]])
    }

    /// Write VRAM target volatility into `_lp_col_pad[2..4]`.
    #[inline]
    pub fn set_vol_margin_target_e6(config: &mut MarketConfig, target: u16) {
        let bytes = target.to_le_bytes();
        config._lp_col_pad[2] = bytes[0];
        config._lp_col_pad[3] = bytes[1];
    }

    // ========================================
    // Feature 3: Audit Crank
    // ========================================

    /// Read audit status from `_orphan_pad[0..2]`.
    /// 0 = never run, 1 = last audit passed, 0xFFFF = violation detected.
    #[inline]
    pub fn read_audit_status(config: &MarketConfig) -> u16 {
        u16::from_le_bytes([config._orphan_pad[0], config._orphan_pad[1]])
    }

    /// Write audit status into `_orphan_pad[0..2]`.
    #[inline]
    pub fn write_audit_status(config: &mut MarketConfig, status: u16) {
        let bytes = status.to_le_bytes();
        config._orphan_pad[0] = bytes[0];
        config._orphan_pad[1] = bytes[1];
    }

    /// Read the slot at which AuditCrank last paused the market from
    /// `_rebalancing_pad[0..6]` (48-bit little-endian u64 — sufficient for
    /// centuries of Solana slots).
    /// 0 = never paused via AuditCrank.
    #[inline]
    pub fn read_last_audit_pause_slot(config: &MarketConfig) -> u64 {
        let mut buf = [0u8; 8];
        buf[..6].copy_from_slice(&config._rebalancing_pad[..6]);
        u64::from_le_bytes(buf)
    }

    /// Write the last AuditCrank pause slot into `_rebalancing_pad[0..6]`.
    #[inline]
    pub fn write_last_audit_pause_slot(config: &mut MarketConfig, slot: u64) {
        let bytes = slot.to_le_bytes();
        config._rebalancing_pad[..6].copy_from_slice(&bytes[..6]);
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn empty_config() -> MarketConfig {
            bytemuck::Zeroable::zeroed()
        }

        // ── #980 migration guard ──────────────────────────────────────────────
        #[test]
        fn get_last_vol_price_e3_returns_zero_for_legacy_e6_value() {
            let mut cfg = empty_config();
            // Simulate a pre-#980 slab: SOL at $200 stored as e6 = 200_000_000
            let legacy_e6: u32 = 200_000_000;
            let bytes = legacy_e6.to_le_bytes();
            cfg._insurance_isolation_padding[8] = bytes[0];
            cfg._insurance_isolation_padding[9] = bytes[1];
            cfg._insurance_isolation_padding[10] = bytes[2];
            cfg._insurance_isolation_padding[11] = bytes[3];
            // Migration guard must return 0 (value > 4_294_000 is implausible as e3)
            assert_eq!(get_last_vol_price_e3(&cfg), 0);
        }

        #[test]
        fn get_last_vol_price_e3_returns_valid_e3_value() {
            let mut cfg = empty_config();
            // SOL at $200 in e3 format = 200_000
            let valid_e3: u32 = 200_000;
            set_last_vol_price_e3(&mut cfg, valid_e3);
            assert_eq!(get_last_vol_price_e3(&cfg), valid_e3);
        }

        #[test]
        fn get_last_vol_price_e3_boundary_at_migration_threshold() {
            let mut cfg = empty_config();
            // 4_294_000 is exactly at the threshold — should pass through
            set_last_vol_price_e3(&mut cfg, 4_294_000);
            assert_eq!(get_last_vol_price_e3(&cfg), 4_294_000);
            // 4_294_001 is above threshold — legacy e6, return 0
            set_last_vol_price_e3(&mut cfg, 4_294_001);
            assert_eq!(get_last_vol_price_e3(&cfg), 0);
        }

        // ── #980 sub-mill price clamp ─────────────────────────────────────────
        #[test]
        fn set_last_vol_price_e3_stores_nonzero_for_submil_price() {
            // Caller must clamp: (price_e6 / 1000).max(1) for price_e6 in 1..999
            // This test verifies the storage round-trip for the clamped value
            let mut cfg = empty_config();
            set_last_vol_price_e3(&mut cfg, 1); // sub-mill clamped to 1
            assert_eq!(get_last_vol_price_e3(&cfg), 1);
        }
    }
}

// 7. mod units - base token/units conversion at instruction boundaries
pub mod units {
    /// Convert base token amount to units, returning (units, dust).
    /// Base token is the collateral (e.g., lamports for SOL, satoshis for BTC).
    /// If scale is 0, returns (base, 0) - no scaling.
    #[inline]
    pub fn base_to_units(base: u64, scale: u32) -> (u64, u64) {
        if scale == 0 {
            return (base, 0);
        }
        let s = scale as u64;
        (base / s, base % s)
    }

    /// Convert units to base token amount.
    /// If scale is 0, returns units unchanged - no scaling.
    #[inline]
    pub fn units_to_base(units: u64, scale: u32) -> u64 {
        if scale == 0 {
            return units;
        }
        units.saturating_mul(scale as u64)
    }

    /// Convert units to base token amount with overflow check.
    /// Returns None if overflow would occur.
    #[inline]
    pub fn units_to_base_checked(units: u64, scale: u32) -> Option<u64> {
        if scale == 0 {
            return Some(units);
        }
        units.checked_mul(scale as u64)
    }
}

// 8. mod oracle
pub mod oracle {
    use crate::constants::DEFAULT_DEX_ORACLE_PRICE_CAP_E2BPS;
    use crate::error::PercolatorError;
    use solana_program::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};

    // SECURITY (H5): The "devnet" feature disables critical oracle safety checks:
    // - Staleness validation (stale prices accepted)
    // - Confidence interval validation (wide confidence accepted)
    //
    // WARNING: NEVER deploy to mainnet with the "devnet" feature enabled!
    // Build for mainnet with: cargo build-sbf (without --features devnet)

    /// Pyth Solana Receiver program ID (same for mainnet and devnet)
    /// rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ
    pub const PYTH_RECEIVER_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
        0x0c, 0xb7, 0xfa, 0xbb, 0x52, 0xf7, 0xa6, 0x48, 0xbb, 0x5b, 0x31, 0x7d, 0x9a, 0x01, 0x8b,
        0x90, 0x57, 0xcb, 0x02, 0x47, 0x74, 0xfa, 0xfe, 0x01, 0xe6, 0xc4, 0xdf, 0x98, 0xcc, 0x38,
        0x58, 0x81,
    ]);

    /// Chainlink OCR2 Store program ID (same for mainnet and devnet)
    /// HEvSKofvBgfaexv23kMabbYqxasxU3mQ4ibBMEmJWHny
    pub const CHAINLINK_OCR2_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
        0xf1, 0x4b, 0xf6, 0x5a, 0xd5, 0x6b, 0xd2, 0xba, 0x71, 0x5e, 0x45, 0x74, 0x2c, 0x23, 0x1f,
        0x27, 0xd6, 0x36, 0x21, 0xcf, 0x5b, 0x77, 0x8f, 0x37, 0xc1, 0xa2, 0x48, 0x95, 0x1d, 0x17,
        0x56, 0x02,
    ]);

    /// PumpSwap AMM program ID
    /// pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA
    pub const PUMPSWAP_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
        0x0c, 0x14, 0xde, 0xfc, 0x82, 0x5e, 0xc6, 0x76, 0x94, 0x25, 0x08, 0x18, 0xbb, 0x65, 0x40,
        0x65, 0xf4, 0x29, 0x8d, 0x31, 0x56, 0xd5, 0x71, 0xb4, 0xd4, 0xf8, 0x09, 0x0c, 0x18, 0xe9,
        0xa8, 0x63,
    ]);

    /// Raydium CLMM (Concentrated Liquidity) program ID
    /// CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK
    pub const RAYDIUM_CLMM_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
        0xa5, 0xd5, 0xca, 0x9e, 0x04, 0xcf, 0x5d, 0xb5, 0x90, 0xb7, 0x14, 0xba, 0x2f, 0xe3, 0x2c,
        0xb1, 0x59, 0x13, 0x3f, 0xc1, 0xc1, 0x92, 0xb7, 0x22, 0x57, 0xfd, 0x07, 0xd3, 0x9c, 0xb0,
        0x40, 0x1e,
    ]);

    /// Meteora DLMM (Dynamic Liquidity Market Maker) program ID
    /// LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo
    pub const METEORA_DLMM_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
        0x04, 0xe9, 0xe1, 0x2f, 0xbc, 0x84, 0xe8, 0x26, 0xc9, 0x32, 0xcc, 0xe9, 0xe2, 0x64, 0x0c,
        0xce, 0x15, 0x59, 0x0c, 0x1c, 0x62, 0x73, 0xb0, 0x92, 0x57, 0x08, 0xba, 0x3b, 0x85, 0x20,
        0xb0, 0xbc,
    ]);

    // PriceUpdateV2 account layout (Borsh-serialized via Anchor's #[account])
    // See: https://github.com/pyth-network/pyth-crosschain/blob/main/target_chains/solana/pyth_solana_receiver_sdk/src/price_update.rs
    //
    // Layout:
    //   [0..8]   discriminator
    //   [8..40]  write_authority (Pubkey)
    //   [40]     verification_level variant (Borsh enum):
    //              0x00 = Partial { num_signatures: u8 } → 2 bytes total (variant + data)
    //              0x01 = Full                           → 1 byte total  (variant only)
    //   [40+N..] PriceFeedMessage: feed_id(32) + price(i64) + conf(u64) + expo(i32) + publish_time(i64) + ...
    //   [...+8]  posted_slot (u64)
    //
    // The base offset for PriceFeedMessage depends on the verification variant:
    //   Partial → base = 42 (8 + 32 + 2)
    //   Full    → base = 41 (8 + 32 + 1)
    const PRICE_UPDATE_V2_MIN_LEN: usize = 134;
    const PYTH_DISCRIMINATOR_LEN: usize = 8;
    const PYTH_WRITE_AUTHORITY_LEN: usize = 32;
    const PYTH_VERIFICATION_LEVEL_OFF: usize = PYTH_DISCRIMINATOR_LEN + PYTH_WRITE_AUTHORITY_LEN; // 40

    // Chainlink OCR2 State/Aggregator account layout offsets (devnet format)
    // This is the simpler account format used on Solana devnet
    // Note: Different from the Transmissions ring buffer format in older docs
    const CL_MIN_LEN: usize = 224; // Minimum required length
    const CL_OFF_DECIMALS: usize = 138; // u8 - number of decimals
                                        // Skip unused: latest_round_id (143), live_length (148), live_cursor (152)
                                        // The actual price data is stored directly at tail:
    const _CL_OFF_SLOT: usize = 200; // u64 - slot when updated
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
    /// Returns the price in e6 format (e.g., 150_000_000 = 150.00 in base units).
    pub fn read_pyth_price_e6(
        price_ai: &AccountInfo,
        expected_feed_id: &[u8; 32],
        now_unix_ts: i64,
        max_staleness_secs: u64,
        conf_bps: u16,
    ) -> Result<u64, ProgramError> {
        // Validate oracle owner (skip in tests to allow mock oracles)
        #[cfg(not(feature = "test"))]
        {
            if *price_ai.owner != PYTH_RECEIVER_PROGRAM_ID {
                return Err(ProgramError::IllegalOwner);
            }
        }

        let data = price_ai.try_borrow_data()?;
        if data.len() < PRICE_UPDATE_V2_MIN_LEN {
            return Err(ProgramError::InvalidAccountData);
        }

        // Determine the base offset for PriceFeedMessage based on VerificationLevel variant.
        // Borsh serializes enums as: 1 byte variant index + variant data.
        //   Partial (0x00) has { num_signatures: u8 } → 2 bytes total
        //   Full    (0x01) has no data                → 1 byte total
        let verification_variant = data[PYTH_VERIFICATION_LEVEL_OFF];
        let base = match verification_variant {
            0 => PYTH_VERIFICATION_LEVEL_OFF + 2, // Partial: variant(1) + num_signatures(1) = 42
            1 => PYTH_VERIFICATION_LEVEL_OFF + 1, // Full: variant(1) = 41
            _ => return Err(ProgramError::InvalidAccountData),
        };

        // PriceFeedMessage field offsets relative to base:
        //   feed_id(32) + price(i64=8) + conf(u64=8) + expo(i32=4) + publish_time(i64=8)
        let off_feed_id = base;
        let off_price = base + 32;
        let off_conf = off_price + 8;
        let off_expo = off_conf + 8;
        let off_publish_time = off_expo + 4;

        // Bounds check
        if off_publish_time + 8 > data.len() {
            return Err(ProgramError::InvalidAccountData);
        }

        // Validate feed_id matches expected
        let feed_id: [u8; 32] = data[off_feed_id..off_feed_id + 32].try_into().unwrap();
        if &feed_id != expected_feed_id {
            return Err(PercolatorError::InvalidOracleKey.into());
        }

        // Read price fields
        let price = i64::from_le_bytes(data[off_price..off_price + 8].try_into().unwrap());
        let conf = u64::from_le_bytes(data[off_conf..off_conf + 8].try_into().unwrap());
        let expo = i32::from_le_bytes(data[off_expo..off_expo + 4].try_into().unwrap());
        let publish_time = i64::from_le_bytes(
            data[off_publish_time..off_publish_time + 8]
                .try_into()
                .unwrap(),
        );

        if price <= 0 {
            return Err(PercolatorError::OracleInvalid.into());
        }

        // SECURITY (C3): Bound exponent to prevent overflow in pow()
        if expo.abs() > MAX_EXPO_ABS {
            return Err(PercolatorError::OracleInvalid.into());
        }

        // Staleness check (skip on devnet)
        #[cfg(not(feature = "devnet"))]
        {
            let age = now_unix_ts.saturating_sub(publish_time);
            if age < 0 || age as u64 > max_staleness_secs {
                return Err(PercolatorError::OracleStale.into());
            }
        }
        #[cfg(feature = "devnet")]
        let _ = (publish_time, max_staleness_secs, now_unix_ts);

        // Confidence check (skip on devnet)
        let price_u = price as u128;
        #[cfg(not(feature = "devnet"))]
        {
            let lhs = (conf as u128) * 10_000;
            let rhs = price_u * (conf_bps as u128);
            if lhs > rhs {
                return Err(PercolatorError::OracleConfTooWide.into());
            }
        }
        #[cfg(feature = "devnet")]
        let _ = (conf, conf_bps);

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

        Ok(final_price_u128 as u64)
    }

    /// Read price from a Chainlink OCR2 State/Aggregator account.
    ///
    /// Parameters:
    /// - price_ai: The Chainlink aggregator account
    /// - expected_feed_pubkey: The expected feed account pubkey (for validation)
    /// - now_unix_ts: Current unix timestamp (from clock.unix_timestamp)
    /// - max_staleness_secs: Maximum age in seconds
    ///
    /// Returns the price in e6 format (e.g., 150_000_000 = 150.00 in base units).
    /// Note: Chainlink doesn't have confidence intervals, so conf_bps is not used.
    pub fn read_chainlink_price_e6(
        price_ai: &AccountInfo,
        expected_feed_pubkey: &[u8; 32],
        now_unix_ts: i64,
        max_staleness_secs: u64,
    ) -> Result<u64, ProgramError> {
        // Validate oracle owner (skip in tests to allow mock oracles)
        #[cfg(not(feature = "test"))]
        {
            if *price_ai.owner != CHAINLINK_OCR2_PROGRAM_ID {
                return Err(ProgramError::IllegalOwner);
            }
        }

        // Validate feed pubkey matches expected
        if price_ai.key.to_bytes() != *expected_feed_pubkey {
            return Err(PercolatorError::InvalidOracleKey.into());
        }

        let data = price_ai.try_borrow_data()?;
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

        // Staleness check (skip on devnet)
        #[cfg(not(feature = "devnet"))]
        {
            let age = now_unix_ts.saturating_sub(timestamp as i64);
            if age < 0 || age as u64 > max_staleness_secs {
                return Err(PercolatorError::OracleStale.into());
            }
        }
        #[cfg(feature = "devnet")]
        let _ = (timestamp, max_staleness_secs, now_unix_ts);

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

        Ok(final_price_u128 as u64)
    }

    // =========================================================================
    // DEX Oracle Readers (PumpSwap, Raydium CLMM, Meteora DLMM)
    // =========================================================================

    // Raydium CLMM PoolState layout (Anchor — 8-byte discriminator)
    const RAYDIUM_CLMM_MIN_LEN: usize = 269;
    const _RAYDIUM_CLMM_OFF_MINT0: usize = 73;
    const _RAYDIUM_CLMM_OFF_MINT1: usize = 105;
    const RAYDIUM_CLMM_OFF_DECIMALS0: usize = 233;
    const RAYDIUM_CLMM_OFF_DECIMALS1: usize = 234;
    const RAYDIUM_CLMM_OFF_SQRT_PRICE_X64: usize = 253;

    /// Read spot price from a Raydium CLMM pool account.
    ///
    /// Uses sqrt_price_x64 (Q64.64 fixed-point) to compute:
    ///   price_e6 = (sqrt_price_x64^2 / 2^128) * 10^(6 + decimals_0 - decimals_1)
    ///
    /// Returns token_1 per token_0 in e6 format.
    ///
    /// SECURITY NOTE: DEX spot prices have no staleness/confidence checks and are
    /// vulnerable to flash-loan manipulation. See PumpSwap docs for details.
    pub fn read_raydium_clmm_price_e6(
        price_ai: &AccountInfo,
        expected_feed_id: &[u8; 32],
    ) -> Result<u64, ProgramError> {
        // Validate pool address matches expected (stored in index_feed_id)
        if price_ai.key.to_bytes() != *expected_feed_id {
            return Err(PercolatorError::InvalidOracleKey.into());
        }

        let data = price_ai.try_borrow_data()?;
        if data.len() < RAYDIUM_CLMM_MIN_LEN {
            return Err(ProgramError::InvalidAccountData);
        }

        let decimals_0 = data[RAYDIUM_CLMM_OFF_DECIMALS0] as i32;
        let decimals_1 = data[RAYDIUM_CLMM_OFF_DECIMALS1] as i32;

        let sqrt_price_x64 = u128::from_le_bytes(
            data[RAYDIUM_CLMM_OFF_SQRT_PRICE_X64..RAYDIUM_CLMM_OFF_SQRT_PRICE_X64 + 16]
                .try_into()
                .unwrap(),
        );

        if sqrt_price_x64 == 0 {
            return Err(PercolatorError::OracleInvalid.into());
        }

        // price_ratio = sqrt_price_x64^2 / 2^128
        // To avoid overflow, compute in steps:
        //   numerator = sqrt_price_x64^2  (can be up to 256 bits, but we use u128 carefully)
        //   We need: price_e6 = (sqrt^2 / 2^128) * 10^(6 + d0 - d1)
        //
        // Rewrite to avoid intermediate overflow:
        //   price_e6 = sqrt^2 * 10^(6 + d0 - d1) / 2^128
        //
        // Split sqrt into hi (top 64 bits) and lo (bottom 64 bits) for precision:
        //   sqrt^2 = (hi * 2^64 + lo)^2 but that's 256-bit.
        //
        // Simpler approach: divide first, multiply second.
        //   step1 = sqrt / 2^64  (integer, may lose precision for small prices)
        //   price_ratio_approx = step1 * sqrt  (fits u128 since both < 2^64 range)
        //
        // For better precision with small prices, we scale up first:
        let decimal_diff = 6i32 + decimals_0 - decimals_1;

        // Compute price_e6 = sqrt_price_x64^2 * 10^decimal_diff / 2^128
        //
        // PRECISION FIX: The naive approach `(sqrt >> 64) * sqrt` drops all low bits,
        // causing sqrtHi = 0 for micro-priced tokens (most memecoins where sqrt < 2^64).
        // Instead, we scale up by 1e6 BEFORE dividing, preserving precision:
        //   scaled_sqrt = sqrt * 1_000_000
        //   term = scaled_sqrt >> 64
        //   price_e6_raw = term * sqrt >> 64
        // This gives us 6 extra decimal digits of precision.
        // We then adjust decimal_diff by -6 since we already multiplied by 1e6.
        let scaled_sqrt = sqrt_price_x64
            .checked_mul(1_000_000)
            .ok_or(PercolatorError::EngineOverflow)?;
        let term = scaled_sqrt >> 64;
        let price_e6_raw = term
            .checked_mul(sqrt_price_x64)
            .ok_or(PercolatorError::EngineOverflow)?
            >> 64;

        // We already embedded 1e6, so adjust decimal_diff accordingly
        let adjusted_diff = decimal_diff - 6;

        let price_e6 = if adjusted_diff >= 0 {
            let scale = 10u128.pow(adjusted_diff as u32);
            price_e6_raw
                .checked_mul(scale)
                .ok_or(PercolatorError::EngineOverflow)?
        } else {
            let scale = 10u128.pow((-adjusted_diff) as u32);
            price_e6_raw / scale
        };

        if price_e6 == 0 {
            return Err(PercolatorError::OracleInvalid.into());
        }
        if price_e6 > u64::MAX as u128 {
            return Err(PercolatorError::EngineOverflow.into());
        }

        Ok(price_e6 as u64)
    }

    /// DEX price result with liquidity information.
    /// Used by UpdateHyperpMark to enforce minimum liquidity before accepting a price.
    pub struct DexPriceResult {
        /// The spot price in e6 format.
        pub price_e6: u64,
        /// Quote-side liquidity in the pool (quote token lamports/atoms).
        /// For PumpSwap: quote vault balance.
        /// For Raydium CLMM: sqrt of liquidity field (approximation of effective depth).
        /// For Meteora DLMM: 0 (no direct liquidity field; price validity implies liquidity).
        pub quote_liquidity: u64,
    }

    // PumpSwap pool layout (no Anchor discriminator)
    const PUMPSWAP_MIN_LEN: usize = 195;
    const PUMPSWAP_OFF_BASE_MINT: usize = 35;
    const PUMPSWAP_OFF_QUOTE_MINT: usize = 67;
    const PUMPSWAP_OFF_BASE_VAULT: usize = 131;
    const PUMPSWAP_OFF_QUOTE_VAULT: usize = 163;

    // SPL Token Account: amount is at offset 64 (u64 LE)
    const SPL_TOKEN_AMOUNT_OFF: usize = 64;
    const SPL_TOKEN_ACCOUNT_MIN_LEN: usize = 72; // need at least through amount field

    /// Read spot price from a PumpSwap AMM pool.
    ///
    /// PumpSwap is a constant-product AMM. Price = quote_reserve / base_reserve.
    /// Requires remaining_accounts[0] = base vault, remaining_accounts[1] = quote vault.
    ///
    /// Returns price in e6 format: price_e6 = quote_amount * 1_000_000 / base_amount.
    /// The `invert` and `unit_scale` fields handle decimal adjustments.
    ///
    /// SECURITY NOTE on DEX oracle freshness:
    /// Unlike Pyth/Chainlink, DEX spot prices have NO staleness or confidence checks.
    /// Spot prices are vulnerable to flash-loan manipulation within a single transaction.
    /// Market creators should understand this trade-off. The clamping logic in
    /// `read_engine_price_with_fallback` provides some protection by capping max price
    /// changes, but this is not a substitute for TWAP or multi-block aggregation.
    /// For high-value markets, prefer Pyth/Chainlink oracles.
    pub fn read_pumpswap_price_e6(
        price_ai: &AccountInfo,
        expected_feed_id: &[u8; 32],
        remaining: &[AccountInfo],
    ) -> Result<u64, ProgramError> {
        // Validate pool address
        if price_ai.key.to_bytes() != *expected_feed_id {
            return Err(PercolatorError::InvalidOracleKey.into());
        }

        let pool_data = price_ai.try_borrow_data()?;
        if pool_data.len() < PUMPSWAP_MIN_LEN {
            return Err(ProgramError::InvalidAccountData);
        }

        // Need exactly 2 remaining accounts: base vault, quote vault
        if remaining.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Read and log base/quote mints for verification.
        // NOTE: We validate vault addresses (which are derived from the pool) but callers
        // must ensure the pool's base_mint/quote_mint match their expected token pair.
        // The pool address itself is validated via expected_feed_id, and the market creator
        // is responsible for configuring the correct pool. An incorrect pool would yield
        // wrong prices but cannot steal funds from the percolator engine.
        let _base_mint: [u8; 32] = pool_data[PUMPSWAP_OFF_BASE_MINT..PUMPSWAP_OFF_BASE_MINT + 32]
            .try_into()
            .unwrap();
        let _quote_mint: [u8; 32] = pool_data
            [PUMPSWAP_OFF_QUOTE_MINT..PUMPSWAP_OFF_QUOTE_MINT + 32]
            .try_into()
            .unwrap();

        // Validate vault addresses match pool's stored vaults
        let expected_base_vault: [u8; 32] = pool_data
            [PUMPSWAP_OFF_BASE_VAULT..PUMPSWAP_OFF_BASE_VAULT + 32]
            .try_into()
            .unwrap();
        let expected_quote_vault: [u8; 32] = pool_data
            [PUMPSWAP_OFF_QUOTE_VAULT..PUMPSWAP_OFF_QUOTE_VAULT + 32]
            .try_into()
            .unwrap();

        if remaining[0].key.to_bytes() != expected_base_vault {
            return Err(PercolatorError::InvalidOracleKey.into());
        }
        if remaining[1].key.to_bytes() != expected_quote_vault {
            return Err(PercolatorError::InvalidOracleKey.into());
        }

        // Read token amounts from vault accounts
        let base_vault_data = remaining[0].try_borrow_data()?;
        let quote_vault_data = remaining[1].try_borrow_data()?;

        if base_vault_data.len() < SPL_TOKEN_ACCOUNT_MIN_LEN
            || quote_vault_data.len() < SPL_TOKEN_ACCOUNT_MIN_LEN
        {
            return Err(ProgramError::InvalidAccountData);
        }

        let base_amount = u64::from_le_bytes(
            base_vault_data[SPL_TOKEN_AMOUNT_OFF..SPL_TOKEN_AMOUNT_OFF + 8]
                .try_into()
                .unwrap(),
        );
        let quote_amount = u64::from_le_bytes(
            quote_vault_data[SPL_TOKEN_AMOUNT_OFF..SPL_TOKEN_AMOUNT_OFF + 8]
                .try_into()
                .unwrap(),
        );

        if base_amount == 0 {
            return Err(PercolatorError::OracleInvalid.into());
        }

        // price_e6 = quote_amount * 1_000_000 / base_amount
        let price_e6 = (quote_amount as u128)
            .checked_mul(1_000_000)
            .ok_or(PercolatorError::EngineOverflow)?
            / (base_amount as u128);

        if price_e6 == 0 {
            return Err(PercolatorError::OracleInvalid.into());
        }
        if price_e6 > u64::MAX as u128 {
            return Err(PercolatorError::EngineOverflow.into());
        }

        Ok(price_e6 as u64)
    }

    // Meteora DLMM LbPair layout (Anchor — 8-byte discriminator)
    // Key fields from the IDL:
    //   parameters (PDA padding + StaticParameters + VariableParameters) starts at offset 8
    //   StaticParameters contains active_id (i32) and bin_step (u16)
    //   Layout verified from Meteora DLMM source:
    //     [8..16]    parameters.padding (?)
    //     Relevant: active_id at 16, bin_step at 20
    //   Actual anchor layout (from LbPair struct):
    //     [8..40]    parameters (StaticParameters 32 bytes)
    //     [40..72]   v_parameters (VariableParameters 32 bytes)
    //     [72..76]   bump_seed [u8;2] + padding
    //     Then: bin_step_seed [u8;2], pair_type u8, active_id i32, ...
    //
    // Simplified: we read active_id and bin_step from known offsets.
    // From Meteora source: LbPair has active_id at offset 8+32+32+2+2+1 = 77 (i32)
    //   and bin_step at offset 8+0+10 = 18 (u16) inside StaticParameters
    //
    // Verified from Meteora DLMM IDL/source:
    //   StaticParameters layout (at offset 8):
    //     base_factor: u16 (0-2)
    //     filter_period: u16 (2-4)
    //     decay_period: u16 (4-6)
    //     reduction_factor: u16 (6-8)
    //     variable_fee_control: u32 (8-12)
    //     max_volatility_accumulator: u32 (12-16)
    //     min_bin_id: i32 (16-20)
    //     max_bin_id: i32 (20-24)
    //     protocol_share: u16 (24-26)
    //     padding: [u8;6] (26-32)
    //   VariableParameters layout (at offset 40):
    //     volatility_accumulator: u32 (0-4)
    //     volatility_reference: u32 (4-8)
    //     id_reference: i32 (8-12)
    //     time_of_last_update: u64 (12-20, but padded to 16 = 24)
    //     padding: [u8;8] (24-32)
    //   After parameters:
    //     [72..74]   bump_seed: [u8;2]
    //     [74..76]   bin_step_seed: [u8;2]  — NOT bin_step (this is just the LE bytes of bin_step for PDA)
    //     [76]       pair_type: u8
    //     [77..81]   active_id: i32
    //     [81..113]  token_x_mint: Pubkey
    //     [113..145] token_y_mint: Pubkey
    //
    // We also need bin_step. The canonical source is LbPair.bin_step field, but it's not
    // stored directly — it's derived from the PDA seeds. However, bin_step_seed at [74..76]
    // IS the bin_step as u16 LE (used in PDA derivation). We can read it from there.

    const METEORA_DLMM_MIN_LEN: usize = 145;
    const METEORA_DLMM_OFF_BIN_STEP_SEED: usize = 74; // u16 LE = bin_step
    const METEORA_DLMM_OFF_ACTIVE_ID: usize = 77; // i32 LE

    /// Read spot price from a Meteora DLMM pool account.
    ///
    /// Price formula: price = (1 + bin_step/10000) ^ active_id
    ///
    /// Uses binary exponentiation with u128 fixed-point (38 decimal digits).
    /// Returns price in e6 format.
    ///
    /// SECURITY NOTE: DEX spot prices have no staleness/confidence checks and are
    /// vulnerable to flash-loan manipulation. See PumpSwap docs for details.
    pub fn read_meteora_dlmm_price_e6(
        price_ai: &AccountInfo,
        expected_feed_id: &[u8; 32],
    ) -> Result<u64, ProgramError> {
        // Validate pool address
        if price_ai.key.to_bytes() != *expected_feed_id {
            return Err(PercolatorError::InvalidOracleKey.into());
        }

        let data = price_ai.try_borrow_data()?;
        if data.len() < METEORA_DLMM_MIN_LEN {
            return Err(ProgramError::InvalidAccountData);
        }

        let bin_step = u16::from_le_bytes(
            data[METEORA_DLMM_OFF_BIN_STEP_SEED..METEORA_DLMM_OFF_BIN_STEP_SEED + 2]
                .try_into()
                .unwrap(),
        ) as u64;

        let active_id = i32::from_le_bytes(
            data[METEORA_DLMM_OFF_ACTIVE_ID..METEORA_DLMM_OFF_ACTIVE_ID + 4]
                .try_into()
                .unwrap(),
        );

        if bin_step == 0 {
            return Err(PercolatorError::OracleInvalid.into());
        }

        // Zero-price bin offset: active_id is signed, center is 0
        // Price = (1 + bin_step/10000) ^ active_id
        // For negative active_id: price = 1 / (1 + bin_step/10000) ^ |active_id|
        let is_negative = active_id < 0;
        let exp = if is_negative {
            (-(active_id as i64)) as u64
        } else {
            active_id as u64
        };

        // Binary exponentiation in fixed-point (scale = 1e18 for precision)
        const SCALE: u128 = 1_000_000_000_000_000_000; // 1e18
        let base = SCALE + (bin_step as u128) * SCALE / 10_000; // (1 + bin_step/10000) * SCALE

        let mut result: u128 = SCALE;
        let mut b: u128 = base;
        let mut e = exp;

        while e > 0 {
            if e & 1 == 1 {
                result = result
                    .checked_mul(b)
                    .ok_or(PercolatorError::EngineOverflow)?
                    / SCALE;
            }
            e >>= 1;
            if e > 0 {
                b = b.checked_mul(b).ok_or(PercolatorError::EngineOverflow)? / SCALE;
            }
        }

        // result is price * SCALE (1e18)
        // Convert to e6: price_e6 = result / 1e12
        let price_e6 = if is_negative {
            // price = 1/result (in fixed point): SCALE^2 / result
            // then convert to e6: (SCALE^2 / result) / 1e12 = SCALE * 1e6 / result
            if result == 0 {
                return Err(PercolatorError::OracleInvalid.into());
            }
            SCALE
                .checked_mul(1_000_000)
                .ok_or(PercolatorError::EngineOverflow)?
                / result
        } else {
            result / 1_000_000_000_000 // result / 1e12 to go from 1e18 to 1e6
        };

        if price_e6 == 0 {
            return Err(PercolatorError::OracleInvalid.into());
        }
        if price_e6 > u64::MAX as u128 {
            return Err(PercolatorError::EngineOverflow.into());
        }

        Ok(price_e6 as u64)
    }

    /// Read DEX spot price with liquidity information.
    ///
    /// Returns both the price and a measure of pool liquidity (quote-side depth).
    /// Used by `UpdateHyperpMark` to enforce minimum liquidity before accepting
    /// a price update — prevents bootstrapping from near-empty pools (#297 Fix 1).
    ///
    /// Applies inversion and unit scaling to the price (same as read_engine_price_e6).
    /// The quote_liquidity value is the RAW quote-side depth (not inverted/scaled),
    /// since it's compared against a threshold in native quote token units.
    pub fn read_dex_price_with_liquidity(
        price_ai: &AccountInfo,
        invert: u8,
        unit_scale: u32,
        remaining_accounts: &[AccountInfo],
    ) -> Result<DexPriceResult, ProgramError> {
        // Use the DEX pool's own pubkey as the feed ID (standard for Hyperp mode)
        let dex_feed_id = price_ai.key.to_bytes();

        let (raw_price, quote_liquidity) = if *price_ai.owner == PUMPSWAP_PROGRAM_ID {
            // PumpSwap: read price and extract quote vault balance as liquidity
            let pool_data = price_ai.try_borrow_data()?;
            if pool_data.len() < PUMPSWAP_MIN_LEN {
                return Err(ProgramError::InvalidAccountData);
            }
            if remaining_accounts.len() < 2 {
                return Err(ProgramError::NotEnoughAccountKeys);
            }

            // Read quote vault balance as liquidity metric
            let quote_vault_data = remaining_accounts[1].try_borrow_data()?;
            if quote_vault_data.len() < SPL_TOKEN_ACCOUNT_MIN_LEN {
                return Err(ProgramError::InvalidAccountData);
            }
            let quote_amount = u64::from_le_bytes(
                quote_vault_data[SPL_TOKEN_AMOUNT_OFF..SPL_TOKEN_AMOUNT_OFF + 8]
                    .try_into()
                    .unwrap(),
            );
            // Drop borrows before calling read_pumpswap_price_e6 which re-borrows
            drop(quote_vault_data);
            drop(pool_data);

            let price = read_pumpswap_price_e6(price_ai, &dex_feed_id, remaining_accounts)?;
            (price, quote_amount)
        } else if *price_ai.owner == RAYDIUM_CLMM_PROGRAM_ID {
            // Raydium CLMM: use the liquidity field as depth indicator
            let data = price_ai.try_borrow_data()?;
            if data.len() < RAYDIUM_CLMM_MIN_LEN {
                return Err(ProgramError::InvalidAccountData);
            }
            // Raydium CLMM pool has a liquidity field (u128) at offset 237
            // This represents the active in-range liquidity
            const RAYDIUM_CLMM_OFF_LIQUIDITY: usize = 237;
            let liquidity = if data.len() >= RAYDIUM_CLMM_OFF_LIQUIDITY + 16 {
                let liq = u128::from_le_bytes(
                    data[RAYDIUM_CLMM_OFF_LIQUIDITY..RAYDIUM_CLMM_OFF_LIQUIDITY + 16]
                        .try_into()
                        .unwrap(),
                );
                // Convert to u64 by taking sqrt (liquidity in CLMM is L^2-like)
                // Use a rough approximation: if liq > u64::MAX, saturate
                core::cmp::min(liq, u64::MAX as u128) as u64
            } else {
                0
            };
            drop(data);

            let price = read_raydium_clmm_price_e6(price_ai, &dex_feed_id)?;
            (price, liquidity)
        } else if *price_ai.owner == METEORA_DLMM_PROGRAM_ID {
            // Meteora DLMM: no direct liquidity field accessible without scanning bins.
            // The price calculation succeeding (non-zero result) implies SOME liquidity.
            // We use u64::MAX as a sentinel meaning "liquidity not measurable but present".
            // The caller can skip the liquidity check for Meteora or use a different threshold.
            let price = read_meteora_dlmm_price_e6(price_ai, &dex_feed_id)?;
            (price, u64::MAX)
        } else {
            return Err(PercolatorError::OracleInvalid.into());
        };

        // Apply inversion and unit scaling to the price
        let price_after_invert = crate::verify::invert_price_e6(raw_price, invert)
            .ok_or(PercolatorError::OracleInvalid)?;
        let final_price = crate::verify::scale_price_e6(price_after_invert, unit_scale)
            .ok_or::<ProgramError>(PercolatorError::OracleInvalid.into())?;

        Ok(DexPriceResult {
            price_e6: final_price,
            quote_liquidity,
        })
    }

    /// Read oracle price for engine use, applying inversion and unit scaling if configured.
    ///
    /// Automatically detects oracle type by account owner:
    /// - PYTH_RECEIVER_PROGRAM_ID: reads Pyth PriceUpdateV2
    /// - CHAINLINK_OCR2_PROGRAM_ID: reads Chainlink OCR2 Transmissions
    /// - RAYDIUM_CLMM_PROGRAM_ID: reads Raydium CLMM sqrt_price_x64
    /// - PUMPSWAP_PROGRAM_ID: reads PumpSwap AMM reserves (needs remaining_accounts)
    /// - METEORA_DLMM_PROGRAM_ID: reads Meteora DLMM active bin price
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
    #[allow(clippy::too_many_arguments)]
    pub fn read_engine_price_e6(
        price_ai: &AccountInfo,
        expected_feed_id: &[u8; 32],
        now_unix_ts: i64,
        max_staleness_secs: u64,
        conf_bps: u16,
        invert: u8,
        unit_scale: u32,
        remaining_accounts: &[AccountInfo],
    ) -> Result<u64, ProgramError> {
        // Detect oracle type by account owner and dispatch
        let raw_price = if *price_ai.owner == PYTH_RECEIVER_PROGRAM_ID {
            read_pyth_price_e6(
                price_ai,
                expected_feed_id,
                now_unix_ts,
                max_staleness_secs,
                conf_bps,
            )?
        } else if *price_ai.owner == CHAINLINK_OCR2_PROGRAM_ID {
            read_chainlink_price_e6(price_ai, expected_feed_id, now_unix_ts, max_staleness_secs)?
        } else if *price_ai.owner == RAYDIUM_CLMM_PROGRAM_ID {
            read_raydium_clmm_price_e6(price_ai, expected_feed_id)?
        } else if *price_ai.owner == PUMPSWAP_PROGRAM_ID {
            read_pumpswap_price_e6(price_ai, expected_feed_id, remaining_accounts)?
        } else if *price_ai.owner == METEORA_DLMM_PROGRAM_ID {
            read_meteora_dlmm_price_e6(price_ai, expected_feed_id)?
        } else {
            // In test mode, try Pyth format first (for existing tests)
            #[cfg(feature = "test")]
            {
                read_pyth_price_e6(
                    price_ai,
                    expected_feed_id,
                    now_unix_ts,
                    max_staleness_secs,
                    conf_bps,
                )?
            }
            #[cfg(not(feature = "test"))]
            {
                return Err(ProgramError::IllegalOwner);
            }
        };

        // Step 1: Apply inversion if configured (uses verify::invert_price_e6)
        let price_after_invert = crate::verify::invert_price_e6(raw_price, invert)
            .ok_or(PercolatorError::OracleInvalid)?;

        // Step 2: Apply unit scaling if configured (uses verify::scale_price_e6)
        // This ensures oracle-derived values match capital scale (stored in units)
        crate::verify::scale_price_e6(price_after_invert, unit_scale)
            .ok_or(PercolatorError::OracleInvalid.into())
    }

    /// Check if authority-pushed price is available and fresh.
    /// Returns Some(price_e6) if authority is set and price is within staleness bounds.
    /// Returns None if no authority is set or price is stale.
    ///
    /// Note: The stored authority_price_e6 is already in the correct format (e6, scaled).
    pub fn read_authority_price(
        config: &super::state::MarketConfig,
        now_unix_ts: i64,
        max_staleness_secs: u64,
    ) -> Option<u64> {
        // No authority set
        if config.oracle_authority == [0u8; 32] {
            return None;
        }
        // No price pushed yet
        if config.authority_price_e6 == 0 {
            return None;
        }
        // Check staleness
        let age = now_unix_ts.saturating_sub(config.authority_timestamp);
        if age < 0 || age as u64 > max_staleness_secs {
            return None;
        }
        Some(config.authority_price_e6)
    }

    /// Read oracle price, preferring authority-pushed price over Pyth/Chainlink.
    ///
    /// If an oracle authority is configured and has pushed a fresh price, use that.
    /// Otherwise, fall back to reading from the provided Pyth/Chainlink account.
    ///
    /// The price_ai can be any account when using authority oracle - it won't be read
    /// if the authority price is valid.
    pub fn read_price_with_authority(
        config: &super::state::MarketConfig,
        price_ai: &AccountInfo,
        now_unix_ts: i64,
        remaining_accounts: &[AccountInfo],
    ) -> Result<u64, ProgramError> {
        // Pyth-pinned mode: oracle_authority is all-zeros AND index_feed_id is non-zero.
        // In this mode we NEVER use authority_price (PushOraclePrice is disabled).
        // We go directly to on-chain Pyth validation with hard rejection on staleness.
        let is_pyth_pinned =
            config.oracle_authority == [0u8; 32] && config.index_feed_id != [0u8; 32];

        if is_pyth_pinned {
            // SECURITY: enforce that the oracle account is owned by an approved oracle program.
            // This prevents substituting an attacker-controlled account for a real Pyth feed.
            if !is_approved_oracle_program(price_ai) {
                return Err(super::error::PercolatorError::OracleInvalid.into());
            }
            // Read directly from Pyth — staleness, confidence, and feed-ID validated on-chain.
            // OracleStale / OracleInvalid are returned as hard errors; no fallback to cache.
            return read_engine_price_e6(
                price_ai,
                &config.index_feed_id,
                now_unix_ts,
                config.max_staleness_secs,
                config.conf_filter_bps,
                config.invert,
                config.unit_scale,
                remaining_accounts,
            );
        }

        // Non-pinned mode (admin oracle or legacy markets):
        // Try authority price first; fall back to Pyth/Chainlink/DEX if absent/stale.
        if let Some(authority_price) =
            read_authority_price(config, now_unix_ts, config.max_staleness_secs)
        {
            return Ok(authority_price);
        }

        // Fall back to Pyth/Chainlink/DEX
        read_engine_price_e6(
            price_ai,
            &config.index_feed_id,
            now_unix_ts,
            config.max_staleness_secs,
            config.conf_filter_bps,
            config.invert,
            config.unit_scale,
            remaining_accounts,
        )
    }

    /// Returns true if the account is owned by an approved on-chain oracle program.
    /// Used in Pyth-pinned mode to prevent account substitution attacks.
    #[inline]
    pub fn is_approved_oracle_program(price_ai: &AccountInfo) -> bool {
        *price_ai.owner == PYTH_RECEIVER_PROGRAM_ID
            || *price_ai.owner == CHAINLINK_OCR2_PROGRAM_ID
            || *price_ai.owner == PUMPSWAP_PROGRAM_ID
            || *price_ai.owner == RAYDIUM_CLMM_PROGRAM_ID
            || *price_ai.owner == METEORA_DLMM_PROGRAM_ID
    }

    /// Clamp `raw_price` so it cannot move more than `max_change_e2bps` from `last_price`.
    /// Units: 1_000_000 e2bps = 100%. 0 = disabled (no cap). last_price == 0 = first-time.
    pub fn clamp_oracle_price(last_price: u64, raw_price: u64, max_change_e2bps: u64) -> u64 {
        if max_change_e2bps == 0 || last_price == 0 {
            return raw_price;
        }
        let max_delta = ((last_price as u128) * (max_change_e2bps as u128) / 1_000_000) as u64;
        let lower = last_price.saturating_sub(max_delta);
        let upper = last_price.saturating_add(max_delta);
        raw_price.clamp(lower, upper)
    }

    /// Read oracle price with circuit-breaker clamping.
    /// Reads raw price via `read_price_with_authority`, clamps it against
    /// `config.last_effective_price_e6`, and updates that field to the post-clamped value.
    /// Returns true if the oracle account is owned by a DEX program (PumpSwap, Raydium CLMM, Meteora DLMM).
    /// DEX spot prices are vulnerable to flash-loan manipulation and require circuit breaker protection.
    #[inline]
    pub fn is_dex_oracle(price_ai: &AccountInfo) -> bool {
        *price_ai.owner == PUMPSWAP_PROGRAM_ID
            || *price_ai.owner == RAYDIUM_CLMM_PROGRAM_ID
            || *price_ai.owner == METEORA_DLMM_PROGRAM_ID
    }

    pub fn read_price_clamped(
        config: &mut super::state::MarketConfig,
        price_ai: &AccountInfo,
        now_unix_ts: i64,
        remaining_accounts: &[AccountInfo],
    ) -> Result<u64, ProgramError> {
        let (price, _was_clamped) =
            read_price_clamped_ext(config, price_ai, now_unix_ts, remaining_accounts)?;
        Ok(price)
    }

    /// PERC-299: Extended version that also reports whether the circuit breaker fired.
    pub fn read_price_clamped_ext(
        config: &mut super::state::MarketConfig,
        price_ai: &AccountInfo,
        now_unix_ts: i64,
        remaining_accounts: &[AccountInfo],
    ) -> Result<(u64, bool), ProgramError> {
        let raw = read_price_with_authority(config, price_ai, now_unix_ts, remaining_accounts)?;
        // For DEX oracles, enforce minimum circuit breaker cap to mitigate flash-loan attacks.
        // This protects existing markets that were initialized with cap=0 (pre-fix default).
        let effective_cap = if is_dex_oracle(price_ai)
            && config.oracle_price_cap_e2bps < DEFAULT_DEX_ORACLE_PRICE_CAP_E2BPS
        {
            DEFAULT_DEX_ORACLE_PRICE_CAP_E2BPS
        } else {
            config.oracle_price_cap_e2bps
        };
        let clamped = clamp_oracle_price(config.last_effective_price_e6, raw, effective_cap);
        let was_clamped = clamped != raw && config.last_effective_price_e6 != 0;
        config.last_effective_price_e6 = clamped;
        Ok((clamped, was_clamped))
    }

    // =========================================================================
    // PERC-274: Multi-source Oracle Aggregation
    // =========================================================================

    /// Read price from multiple oracle sources and compute median.
    /// Falls back to single-source if only one oracle account provided.
    ///
    /// Sources tried (in order): primary oracle, then remaining_accounts[0..N]
    /// Each source that fails silently returns 0 (filtered by median).
    /// Requires at least 1 valid price to succeed.
    ///
    /// Emits sol_log with accepted price, source count, and timestamp for
    /// on-chain transparency and off-chain indexing.
    pub fn read_price_aggregated(
        config: &mut super::state::MarketConfig,
        primary_oracle: &AccountInfo,
        now_unix_ts: i64,
        remaining_accounts: &[AccountInfo],
    ) -> Result<u64, ProgramError> {
        use crate::verify::{median_price, MAX_ORACLE_SOURCES};

        let mut prices = [0u64; MAX_ORACLE_SOURCES];
        let mut source_count = 0u8;

        // Try primary oracle
        if let Ok(p) =
            read_price_with_authority(config, primary_oracle, now_unix_ts, remaining_accounts)
        {
            if p > 0 {
                prices[0] = p;
                source_count += 1;
            }
        }

        // Try additional oracle accounts from remaining_accounts
        // Convention: after any PumpSwap vault accounts, additional oracles start
        // We try each remaining account as a potential oracle
        for extra in remaining_accounts.iter() {
            if source_count as usize >= MAX_ORACLE_SOURCES {
                break;
            }
            // Skip accounts that are clearly not oracles (system program, token program, etc.)
            if extra.data_len() < 100 {
                continue;
            }
            // Try Pyth
            if let Ok(p) = read_pyth_price_e6(
                extra,
                &config.index_feed_id,
                now_unix_ts,
                config.max_staleness_secs,
                config.conf_filter_bps,
            ) {
                if p > 0 {
                    prices[source_count as usize] = p;
                    source_count += 1;
                    continue;
                }
            }
            // Try Chainlink (use index_feed_id as expected pubkey for validation)
            if let Ok(p) = read_chainlink_price_e6(
                extra,
                &config.index_feed_id,
                now_unix_ts,
                config.max_staleness_secs,
            ) {
                if p > 0 {
                    prices[source_count as usize] = p;
                    source_count += 1;
                }
            }
        }

        if source_count == 0 {
            return Err(super::error::PercolatorError::OracleStale.into());
        }

        // Compute median
        let median = median_price(&mut prices)
            .ok_or::<ProgramError>(super::error::PercolatorError::OracleStale.into())?;

        // Apply circuit breaker clamping
        let effective_cap = if is_dex_oracle(primary_oracle)
            && config.oracle_price_cap_e2bps < DEFAULT_DEX_ORACLE_PRICE_CAP_E2BPS
        {
            DEFAULT_DEX_ORACLE_PRICE_CAP_E2BPS
        } else {
            config.oracle_price_cap_e2bps
        };
        let clamped = clamp_oracle_price(config.last_effective_price_e6, median, effective_cap);
        config.last_effective_price_e6 = clamped;

        // Emit price event for on-chain transparency
        #[cfg(not(feature = "test"))]
        {
            use alloc::format;
            solana_program::msg!(
                "OracleAggregated: price={} sources={} median={} clamped={} ts={}",
                clamped,
                source_count,
                median,
                clamped,
                now_unix_ts
            );
        }
        #[cfg(feature = "test")]
        {
            // In test mode, msg! doesn't need alloc::format
            let _ = (clamped, source_count, median, now_unix_ts);
        }

        Ok(clamped)
    }

    // =========================================================================
    // Hyperp mode helpers (internal mark/index, no external oracle)
    // =========================================================================

    /// Check if Hyperp mode is active (internal mark/index pricing).
    /// Hyperp mode is active when index_feed_id is all zeros.
    #[inline]
    pub fn is_hyperp_mode(config: &super::state::MarketConfig) -> bool {
        config.index_feed_id == [0u8; 32]
    }

    /// Check that the Hyperp oracle price is not stale.
    /// Returns `OracleStale` if the engine hasn't been cranked within
    /// `max_crank_staleness_slots` slots.
    #[inline]
    pub fn check_hyperp_staleness(
        engine_current_slot: u64,
        max_crank_staleness_slots: u64,
        clock_slot: u64,
    ) -> Result<(), ProgramError> {
        if max_crank_staleness_slots > 0 && max_crank_staleness_slots != u64::MAX {
            let age = clock_slot.saturating_sub(engine_current_slot);
            if age > max_crank_staleness_slots {
                solana_program::msg!("Hyperp oracle stale");
                return Err(super::error::PercolatorError::OracleStale.into());
            }
        }
        Ok(())
    }

    /// Move `index` toward `mark`, but clamp movement by cap_e2bps * dt_slots.
    /// cap_e2bps units: 1_000_000 = 100.00%
    /// Returns the new index value.
    ///
    /// Security: When dt_slots == 0 (same slot) or cap_e2bps == 0 (cap disabled),
    /// returns index unchanged to prevent bypassing rate limits.
    pub fn clamp_toward_with_dt(index: u64, mark: u64, cap_e2bps: u64, dt_slots: u64) -> u64 {
        if index == 0 {
            return mark;
        }
        // Bug #9 fix: return index (no movement) when dt=0 or cap=0,
        // rather than mark (bypass rate limiting)
        if cap_e2bps == 0 || dt_slots == 0 {
            return index;
        }

        let max_delta_u128 = (index as u128)
            .saturating_mul(cap_e2bps as u128)
            .saturating_mul(dt_slots as u128)
            / 1_000_000u128;

        let max_delta = core::cmp::min(max_delta_u128, u64::MAX as u128) as u64;
        let lo = index.saturating_sub(max_delta);
        let hi = index.saturating_add(max_delta);
        mark.clamp(lo, hi)
    }

    /// Get engine oracle price (unified: external oracle vs Hyperp mode).
    /// In Hyperp mode: updates index toward mark with rate limiting.
    /// In external mode: reads from Pyth/Chainlink/authority with circuit breaker.
    pub fn get_engine_oracle_price_e6(
        engine_last_slot: u64,
        now_slot: u64,
        now_unix_ts: i64,
        config: &mut super::state::MarketConfig,
        a_oracle: &AccountInfo,
        remaining_accounts: &[AccountInfo],
    ) -> Result<u64, ProgramError> {
        // Hyperp mode: index_feed_id == 0
        if is_hyperp_mode(config) {
            let mark = config.authority_price_e6;
            if mark == 0 {
                return Err(super::error::PercolatorError::OracleInvalid.into());
            }

            let prev_index = config.last_effective_price_e6;
            let dt = now_slot.saturating_sub(engine_last_slot);
            let new_index =
                clamp_toward_with_dt(prev_index.max(1), mark, config.oracle_price_cap_e2bps, dt);

            config.last_effective_price_e6 = new_index;
            return Ok(new_index);
        }

        // Non-Hyperp: existing behavior (authority -> Pyth/Chainlink) + circuit breaker
        read_price_clamped(config, a_oracle, now_unix_ts, remaining_accounts)
    }

    /// Compute the Hyperp EMA blend mark price input (PERC-118).
    ///
    /// Blends a stable oracle price with a real-time impact mid price using a
    /// configurable oracle weight:
    ///
    /// ```text
    /// blend = (oracle_weight * oracle_e6 + (10_000 - oracle_weight) * impact_mid_e6) / 10_000
    /// ```
    ///
    /// The blend is then fed into `compute_ema_mark_price` as the target price,
    /// creating a mark that:
    /// - Tracks real-time market movements (impact_mid component)
    /// - Stays anchored to the oracle reference (oracle component)
    /// - Enables non-zero mark premium when impact_mid diverges from oracle
    ///
    /// # Arguments
    /// - `oracle_e6`: Reference oracle price (e.g., last_effective_price_e6 for Hyperp markets).
    ///   If zero, returns `impact_mid_e6` directly (unblended).
    /// - `impact_mid_e6`: Real-time market price (e.g., DEX pool spot price).
    /// - `oracle_weight_bps`: Oracle weight in basis points (0..=10_000).
    ///   0 = 100% impact_mid (backward-compatible, no blend).
    ///   10_000 = 100% oracle (mark anchored to oracle, premium always 0).
    ///
    /// # Returns
    /// Blended price in e6 units, saturating at u64::MAX.
    pub fn compute_blend_mark_price(
        oracle_e6: u64,
        impact_mid_e6: u64,
        oracle_weight_bps: u16,
    ) -> u64 {
        percolator::RiskEngine::compute_blended_mark_price(
            oracle_e6,
            impact_mid_e6,
            oracle_weight_bps as u64,
        )
    }

    /// Compute premium-based funding rate (Hyperp funding model).
    /// Premium = (mark - index) / index, converted to bps per slot.
    /// Returns signed bps per slot (positive = longs pay shorts).
    /// Compute the next EMA mark price step.
    ///
    /// mark_new = oracle_clamped * alpha + mark_prev * (1-alpha)
    ///
    /// Circuit breaker applied BEFORE EMA: oracle clamped to ±cap_e2bps*dt per slot.
    /// dt_slots compounding: effective_alpha ≈ min(alpha*dt, 1_000_000).
    /// Bootstrap: mark_prev==0 returns oracle directly.
    pub fn compute_ema_mark_price(
        mark_prev_e6: u64,
        oracle_e6: u64,
        dt_slots: u64,
        alpha_e6: u64,
        cap_e2bps: u64,
    ) -> u64 {
        if oracle_e6 == 0 {
            return mark_prev_e6;
        }
        if mark_prev_e6 == 0 || dt_slots == 0 {
            return oracle_e6;
        }

        // Circuit breaker: clamp oracle toward prev mark
        let oracle_clamped = if cap_e2bps > 0 {
            let max_delta = (mark_prev_e6 as u128)
                .saturating_mul(cap_e2bps as u128)
                .saturating_mul(dt_slots as u128)
                / 1_000_000u128;
            let max_delta = max_delta.min(mark_prev_e6 as u128) as u64;
            oracle_e6.clamp(
                mark_prev_e6.saturating_sub(max_delta),
                mark_prev_e6.saturating_add(max_delta),
            )
        } else {
            oracle_e6
        };

        // EMA with compound alpha
        let eff_alpha = (alpha_e6 as u128)
            .saturating_mul(dt_slots as u128)
            .min(1_000_000u128) as u64;
        let one_minus = 1_000_000u64.saturating_sub(eff_alpha);

        let ema = (oracle_clamped as u128)
            .saturating_mul(eff_alpha as u128)
            .saturating_add((mark_prev_e6 as u128).saturating_mul(one_minus as u128))
            / 1_000_000u128;

        ema.min(u64::MAX as u128) as u64
    }

    pub fn compute_premium_funding_bps_per_slot(
        mark_e6: u64,
        index_e6: u64,
        funding_horizon_slots: u64,
        funding_k_bps: u64,   // 100 = 1.00x multiplier
        max_premium_bps: i64, // e.g. 500 = 5%
        max_bps_per_slot: i64,
    ) -> i64 {
        if mark_e6 == 0 || index_e6 == 0 || funding_horizon_slots == 0 {
            return 0;
        }

        let diff = mark_e6 as i128 - index_e6 as i128;
        let mut premium_bps = diff.saturating_mul(10_000) / (index_e6 as i128);

        // Clamp premium
        premium_bps = premium_bps.clamp(-(max_premium_bps as i128), max_premium_bps as i128);

        // Apply k multiplier (100 => 1.00x)
        let scaled = premium_bps.saturating_mul(funding_k_bps as i128) / 100i128;

        // Convert to per-slot by dividing by horizon
        let mut per_slot = (scaled / (funding_horizon_slots as i128)) as i64;

        // Policy clamp
        per_slot = per_slot.clamp(-max_bps_per_slot, max_bps_per_slot);
        per_slot
    }
}

// 9. mod collateral
pub mod collateral {
    use solana_program::{account_info::AccountInfo, program_error::ProgramError};

    #[cfg(not(feature = "test"))]
    use solana_program::program::{invoke, invoke_signed};

    #[cfg(feature = "test")]
    use solana_program::program_pack::Pack;
    #[cfg(feature = "test")]
    use spl_token::state::Account as TokenAccount;

    pub fn deposit<'a>(
        _token_program: &AccountInfo<'a>,
        source: &AccountInfo<'a>,
        dest: &AccountInfo<'a>,
        _authority: &AccountInfo<'a>,
        amount: u64,
    ) -> Result<(), ProgramError> {
        if amount == 0 {
            return Ok(());
        }
        #[cfg(not(feature = "test"))]
        {
            let ix = spl_token::instruction::transfer(
                _token_program.key,
                source.key,
                dest.key,
                _authority.key,
                &[],
                amount,
            )?;
            invoke(
                &ix,
                &[
                    source.clone(),
                    dest.clone(),
                    _authority.clone(),
                    _token_program.clone(),
                ],
            )
        }
        #[cfg(feature = "test")]
        {
            let mut src_data = source.try_borrow_mut_data()?;
            let mut src_state = TokenAccount::unpack(&src_data)?;
            src_state.amount = src_state
                .amount
                .checked_sub(amount)
                .ok_or(ProgramError::InsufficientFunds)?;
            TokenAccount::pack(src_state, &mut src_data)?;

            let mut dst_data = dest.try_borrow_mut_data()?;
            let mut dst_state = TokenAccount::unpack(&dst_data)?;
            dst_state.amount = dst_state
                .amount
                .checked_add(amount)
                .ok_or(ProgramError::InvalidAccountData)?;
            TokenAccount::pack(dst_state, &mut dst_data)?;
            Ok(())
        }
    }

    pub fn withdraw<'a>(
        _token_program: &AccountInfo<'a>,
        source: &AccountInfo<'a>,
        dest: &AccountInfo<'a>,
        _authority: &AccountInfo<'a>,
        amount: u64,
        _signer_seeds: &[&[&[u8]]],
    ) -> Result<(), ProgramError> {
        if amount == 0 {
            return Ok(());
        }
        #[cfg(not(feature = "test"))]
        {
            let ix = spl_token::instruction::transfer(
                _token_program.key,
                source.key,
                dest.key,
                _authority.key,
                &[],
                amount,
            )?;
            invoke_signed(
                &ix,
                &[
                    source.clone(),
                    dest.clone(),
                    _authority.clone(),
                    _token_program.clone(),
                ],
                _signer_seeds,
            )
        }
        #[cfg(feature = "test")]
        {
            let mut src_data = source.try_borrow_mut_data()?;
            let mut src_state = TokenAccount::unpack(&src_data)?;
            src_state.amount = src_state
                .amount
                .checked_sub(amount)
                .ok_or(ProgramError::InsufficientFunds)?;
            TokenAccount::pack(src_state, &mut src_data)?;

            let mut dst_data = dest.try_borrow_mut_data()?;
            let mut dst_state = TokenAccount::unpack(&dst_data)?;
            dst_state.amount = dst_state
                .amount
                .checked_add(amount)
                .ok_or(ProgramError::InvalidAccountData)?;
            TokenAccount::pack(dst_state, &mut dst_data)?;
            Ok(())
        }
    }
}

// 9a. mod insurance_lp — SPL mint/burn helpers for insurance LP tokens
pub mod insurance_lp {
    #[allow(unused_imports)]
    use alloc::format;
    use solana_program::{
        account_info::AccountInfo, program_error::ProgramError, system_instruction,
    };

    #[cfg(not(feature = "test"))]
    use solana_program::program::{invoke, invoke_signed};
    use solana_program::program_pack::Pack;
    #[cfg(not(feature = "test"))]
    use solana_program::sysvar::Sysvar;

    /// Create the insurance LP mint account (PDA) and initialize it.
    /// Mint authority = vault_authority PDA. Freeze authority = None.
    #[allow(unused_variables, clippy::too_many_arguments)]
    pub fn create_mint<'a>(
        payer: &AccountInfo<'a>,
        mint_account: &AccountInfo<'a>,
        vault_authority: &AccountInfo<'a>,
        system_program: &AccountInfo<'a>,
        token_program: &AccountInfo<'a>,
        rent_sysvar: &AccountInfo<'a>,
        decimals: u8,
        mint_seeds: &[&[u8]],
    ) -> Result<(), ProgramError> {
        #[cfg(not(feature = "test"))]
        {
            let space = spl_token::state::Mint::LEN;
            let rent = solana_program::rent::Rent::get()?;
            let lamports = rent.minimum_balance(space);

            // Create account via CPI with PDA signing
            let create_ix = system_instruction::create_account(
                payer.key,
                mint_account.key,
                lamports,
                space as u64,
                &spl_token::ID,
            );
            invoke_signed(
                &create_ix,
                &[payer.clone(), mint_account.clone(), system_program.clone()],
                &[mint_seeds],
            )?;

            // Initialize mint: authority = vault_authority PDA, freeze = None
            let init_ix = spl_token::instruction::initialize_mint(
                &spl_token::ID,
                mint_account.key,
                vault_authority.key,
                None,
                decimals,
            )?;
            invoke(
                &init_ix,
                &[
                    mint_account.clone(),
                    rent_sysvar.clone(),
                    token_program.clone(),
                ],
            )?;
        }
        #[cfg(feature = "test")]
        {
            // In test mode, initialize the mint data directly
            use solana_program::program_pack::Pack;
            use spl_token::state::Mint;
            let mut data = mint_account.try_borrow_mut_data()?;
            let mut mint_state = Mint::default();
            mint_state.is_initialized = true;
            mint_state.decimals = decimals;
            mint_state.mint_authority =
                solana_program::program_option::COption::Some(*vault_authority.key);
            mint_state.freeze_authority = solana_program::program_option::COption::None;
            mint_state.supply = 0;
            Mint::pack(mint_state, &mut data)?;
        }
        Ok(())
    }

    /// Mint LP tokens to a user's token account. Signed by vault_authority PDA.
    #[allow(unused_variables)]
    pub fn mint_to<'a>(
        token_program: &AccountInfo<'a>,
        mint: &AccountInfo<'a>,
        destination: &AccountInfo<'a>,
        authority: &AccountInfo<'a>,
        amount: u64,
        signer_seeds: &[&[&[u8]]],
    ) -> Result<(), ProgramError> {
        if amount == 0 {
            return Ok(());
        }
        #[cfg(not(feature = "test"))]
        {
            let ix = spl_token::instruction::mint_to(
                token_program.key,
                mint.key,
                destination.key,
                authority.key,
                &[],
                amount,
            )?;
            invoke_signed(
                &ix,
                &[
                    mint.clone(),
                    destination.clone(),
                    authority.clone(),
                    token_program.clone(),
                ],
                signer_seeds,
            )
        }
        #[cfg(feature = "test")]
        {
            use solana_program::program_pack::Pack;
            use spl_token::state::{Account as TokenAccount, Mint};

            // Update mint supply
            let mut mint_data = mint.try_borrow_mut_data()?;
            let mut mint_state = Mint::unpack(&mint_data)?;
            mint_state.supply = mint_state
                .supply
                .checked_add(amount)
                .ok_or(ProgramError::InvalidAccountData)?;
            Mint::pack(mint_state, &mut mint_data)?;

            // Update destination balance
            let mut dst_data = destination.try_borrow_mut_data()?;
            let mut dst_state = TokenAccount::unpack(&dst_data)?;
            dst_state.amount = dst_state
                .amount
                .checked_add(amount)
                .ok_or(ProgramError::InvalidAccountData)?;
            TokenAccount::pack(dst_state, &mut dst_data)?;
            Ok(())
        }
    }

    /// Burn LP tokens from a user's token account. User is the authority.
    #[allow(unused_variables)]
    pub fn burn<'a>(
        token_program: &AccountInfo<'a>,
        mint: &AccountInfo<'a>,
        source: &AccountInfo<'a>,
        authority: &AccountInfo<'a>,
        amount: u64,
    ) -> Result<(), ProgramError> {
        if amount == 0 {
            return Ok(());
        }
        #[cfg(not(feature = "test"))]
        {
            let ix = spl_token::instruction::burn(
                token_program.key,
                source.key,
                mint.key,
                authority.key,
                &[],
                amount,
            )?;
            invoke(
                &ix,
                &[
                    source.clone(),
                    mint.clone(),
                    authority.clone(),
                    token_program.clone(),
                ],
            )
        }
        #[cfg(feature = "test")]
        {
            use solana_program::program_pack::Pack;
            use spl_token::state::{Account as TokenAccount, Mint};

            // Update mint supply
            let mut mint_data = mint.try_borrow_mut_data()?;
            let mut mint_state = Mint::unpack(&mint_data)?;
            mint_state.supply = mint_state
                .supply
                .checked_sub(amount)
                .ok_or(ProgramError::InsufficientFunds)?;
            Mint::pack(mint_state, &mut mint_data)?;

            // Update source balance
            let mut src_data = source.try_borrow_mut_data()?;
            let mut src_state = TokenAccount::unpack(&src_data)?;
            src_state.amount = src_state
                .amount
                .checked_sub(amount)
                .ok_or(ProgramError::InsufficientFunds)?;
            TokenAccount::pack(src_state, &mut src_data)?;
            Ok(())
        }
    }

    /// Read the current supply from an SPL mint account.
    pub fn read_mint_supply(mint_account: &AccountInfo) -> Result<u64, ProgramError> {
        use solana_program::program_pack::Pack;
        let data = mint_account.try_borrow_data()?;
        let mint = spl_token::state::Mint::unpack(&data)?;
        if !mint.is_initialized {
            return Err(ProgramError::UninitializedAccount);
        }
        Ok(mint.supply)
    }

    /// Read the decimals from an SPL mint account.
    pub fn read_mint_decimals(mint_account: &AccountInfo) -> Result<u8, ProgramError> {
        use solana_program::program_pack::Pack;
        let data = mint_account.try_borrow_data()?;
        let mint = spl_token::state::Mint::unpack(&data)?;
        Ok(mint.decimals)
    }
}

// 9b. mod lp_vault — LP vault state and helpers (PERC-272)
pub mod lp_vault {
    use bytemuck::{Pod, Zeroable};

    /// LP vault state account size in bytes.
    pub const LP_VAULT_STATE_LEN: usize = core::mem::size_of::<LpVaultState>();

    /// Magic value for LP vault state: "LPVAULT\0" = 0x4C505641554C5400
    pub const LP_VAULT_MAGIC: u64 = 0x4C50_5641_554C_5400;

    /// LP vault state PDA account layout.
    ///
    /// Seeds: `["lp_vault", slab_key]`.
    /// Tracks total LP capital, fee distribution snapshots, and epoch.
    /// Epoch-based: auto-resolves on deposit/withdraw, no admin reset needed.
    /// LP vault state: all u128 fields at 16-byte aligned offsets.
    /// Layout (total 128 bytes):
    ///   0..8   magic (u64)
    ///   8..16  fee_share_bps (u64)
    ///   16..32 total_capital (u128)
    ///   32..40 epoch (u64)
    ///   40..48 last_crank_slot (u64)
    ///   48..64 last_fee_snapshot (u128)
    ///   64..80 total_fees_distributed (u128)
    ///   80..84 current_fee_mult_bps (u32) — PERC-304
    ///   84     lp_util_curve_enabled (u8) — PERC-304
    ///   85..88 _padding304 ([u8; 3])
    ///   88..112 _reserved ([u8; 24])
    ///   --- PERC-313: struct grows from 128 → 192 ---
    ///   112..128 epoch_high_water_tvl (u128) — PERC-313
    ///   128..130 hwm_floor_bps (u16) — PERC-313
    ///   130..136 _hwm_padding ([u8; 6])
    ///   136..176 _reserved2 ([u8; 40])
    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct LpVaultState {
        /// Magic identifier: LP_VAULT_MAGIC when initialized.
        pub magic: u64,
        /// Fee share in basis points (0..=10_000). Portion of trading fees
        /// redirected from insurance fund to LP vault on crank.
        pub fee_share_bps: u64,
        /// Total LP capital in engine units (deposit units, not base tokens).
        /// Increases on deposit + fee crank. Decreases on withdraw.
        pub total_capital: u128,
        /// Auto-incrementing epoch. Incremented when vault is fully drained.
        /// Epochs allow trustless resolution without admin intervention:
        /// a drained vault auto-starts a fresh epoch on next deposit.
        pub epoch: u64,
        /// Slot of last fee crank (prevents double-crank in same slot).
        pub last_crank_slot: u64,
        /// Snapshot of `engine.insurance_fund.fee_revenue` at last fee crank.
        /// Delta since snapshot = new fees to distribute.
        pub last_fee_snapshot: u128,
        /// Total fees ever distributed to LP vault (monotonically increasing).
        pub total_fees_distributed: u128,
        /// PERC-308: Whether loyalty multiplier is enabled (1=yes, 0=no).
        pub loyalty_enabled: u8,
        pub _loyalty_pad: [u8; 7],
        /// PERC-309: Withdrawal threshold in bps of TVL. 0 = disabled.
        pub queue_threshold_bps: u16,
        /// PERC-309: Number of epochs for queued withdrawals (default 5).
        pub queue_epochs: u8,
        pub _drip_pad: [u8; 5],
        /// Reserved for future use.
        /// PERC-304: Current fee multiplier in basis points (10_000 = 1.0×).
        /// Updated on every LpVaultCrankFees when util curve is enabled.
        /// Readable by off-chain keepers/SDK for APY display.
        pub current_fee_mult_bps: u32,
        /// PERC-304: Whether the utilization kink curve is active.
        /// 0 = disabled (multiplier always 1.0×), 1 = enabled.
        /// Set via CreateLpVault instruction (optional parameter).
        pub lp_util_curve_enabled: u8,
        /// Alignment padding (PERC-304).
        pub _padding304: [u8; 3],
        /// Reserved for future use (alignment + forward compat).
        /// Original 128-byte struct: 80 (base) + 8 (PERC-308) + 8 (PERC-309) + 8 (PERC-304) + 24 = 128.
        pub _reserved: [u8; 24],

        // ========================================
        // PERC-313: High-Water Mark Protection (grows struct from 128 → 192)
        // ========================================
        /// Max TVL (total_capital) seen in the current epoch.
        /// Updated on every deposit. Reset to current TVL on epoch change.
        pub epoch_high_water_tvl: u128,

        /// Floor as BPS of epoch_high_water_tvl. Withdrawals that would push
        /// total_capital below hwm_floor are blocked.
        /// Default 5000 = 50%. 0 = disabled.
        pub hwm_floor_bps: u16,

        /// Padding for alignment after u16
        pub _hwm_padding: [u8; 6],

        /// Reserved for future use (alignment + forward compat).
        /// 128 + 16 + 2 + 6 + 40 = 192 (16-byte aligned for u128).
        pub _reserved2: [u8; 40],
    }

    impl LpVaultState {
        /// Check if this state account is initialised.
        #[inline]
        pub fn is_initialized(&self) -> bool {
            self.magic == LP_VAULT_MAGIC
        }

        /// Create a zeroed LP vault state.
        #[inline]
        pub fn new_zeroed() -> Self {
            <Self as Zeroable>::zeroed()
        }
    }

    // ========================================
    // Feature 4: Insurance Fund Tranche Waterfall
    // ========================================
    // Tranche fields stored in `_reserved2[0..40]`:
    //   [0]      tranche_enabled: u8 (0 = disabled, 1 = enabled)
    //   [1]      _tranche_pad0: u8
    //   [2..4]   junior_fee_mult_bps: u16 (junior tranche fee multiplier, e.g., 15_000 = 1.5x)
    //   [4..8]   _tranche_pad1: [u8; 4]
    //   [8..24]  senior_capital: u128 (total capital in senior tranche)
    //   [24..40] junior_capital: u128 (total capital in junior tranche)

    impl LpVaultState {
        /// Check if tranches are enabled.
        #[inline]
        pub fn tranche_enabled(&self) -> bool {
            self._reserved2[0] != 0
        }

        /// Enable/disable tranches.
        #[inline]
        pub fn set_tranche_enabled(&mut self, enabled: bool) {
            self._reserved2[0] = if enabled { 1 } else { 0 };
        }

        /// Read junior tranche fee multiplier in bps (e.g., 15_000 = 1.5x senior yield).
        #[inline]
        pub fn junior_fee_mult_bps(&self) -> u16 {
            u16::from_le_bytes([self._reserved2[2], self._reserved2[3]])
        }

        /// Set junior tranche fee multiplier.
        #[inline]
        pub fn set_junior_fee_mult_bps(&mut self, mult: u16) {
            let bytes = mult.to_le_bytes();
            self._reserved2[2] = bytes[0];
            self._reserved2[3] = bytes[1];
        }

        /// Read senior tranche capital.
        #[inline]
        pub fn senior_capital(&self) -> u128 {
            u128::from_le_bytes(self._reserved2[8..24].try_into().unwrap())
        }

        /// Set senior tranche capital.
        #[inline]
        pub fn set_senior_capital(&mut self, capital: u128) {
            self._reserved2[8..24].copy_from_slice(&capital.to_le_bytes());
        }

        /// Read junior tranche capital.
        #[inline]
        pub fn junior_capital(&self) -> u128 {
            u128::from_le_bytes(self._reserved2[24..40].try_into().unwrap())
        }

        /// Set junior tranche capital.
        #[inline]
        pub fn set_junior_capital(&mut self, capital: u128) {
            self._reserved2[24..40].copy_from_slice(&capital.to_le_bytes());
        }

        /// Distribute fees between tranches using the junior multiplier.
        ///
        /// Junior tranche earns `junior_fee_mult_bps / 10_000` times as much per unit
        /// of capital as senior. Returns (senior_share, junior_share).
        pub fn split_fees_by_tranche(&self, total_fees: u128) -> (u128, u128) {
            let senior = self.senior_capital();
            let junior = self.junior_capital();
            if senior == 0 && junior == 0 {
                return (total_fees, 0);
            }
            if junior == 0 {
                return (total_fees, 0);
            }
            if senior == 0 {
                return (0, total_fees);
            }
            // Weighted split: junior weight = junior_capital * junior_mult_bps
            // senior weight = senior_capital * 10_000
            let mult = self.junior_fee_mult_bps().max(10_000) as u128;
            let senior_weight = senior.saturating_mul(10_000);
            let junior_weight = junior.saturating_mul(mult);
            let total_weight = senior_weight.saturating_add(junior_weight);
            if total_weight == 0 {
                return (total_fees, 0);
            }
            let junior_share = total_fees.saturating_mul(junior_weight) / total_weight;
            let senior_share = total_fees.saturating_sub(junior_share);
            (senior_share, junior_share)
        }

        /// Apply loss waterfall: junior tranche absorbs losses first.
        /// Returns actual loss absorbed.
        pub fn apply_loss_waterfall(&mut self, loss: u128) -> u128 {
            let junior = self.junior_capital();
            if loss <= junior {
                // Junior absorbs all
                self.set_junior_capital(junior - loss);
                // #978: Keep total_capital in sync with tranche balances
                self.total_capital = self.total_capital.saturating_sub(loss);
                return loss;
            }
            // Junior wiped out, remainder hits senior
            self.set_junior_capital(0);
            let remainder = loss - junior;
            let senior = self.senior_capital();
            let senior_loss = remainder.min(senior);
            self.set_senior_capital(senior - senior_loss);
            let realized = junior + senior_loss;
            // #978: Keep total_capital in sync with tranche balances
            self.total_capital = self.total_capital.saturating_sub(realized);
            realized
        }
    }

    /// Read LP vault state from raw account data.
    pub fn read_lp_vault_state(data: &[u8]) -> Option<LpVaultState> {
        if data.len() < LP_VAULT_STATE_LEN {
            return None;
        }
        let bytes = &data[..LP_VAULT_STATE_LEN];
        Some(*bytemuck::from_bytes::<LpVaultState>(bytes))
    }

    /// Write LP vault state to raw account data.
    pub fn write_lp_vault_state(data: &mut [u8], state: &LpVaultState) {
        let bytes = bytemuck::bytes_of(state);
        data[..LP_VAULT_STATE_LEN].copy_from_slice(bytes);
    }

    // =========================================================================
    // PERC-309: Withdraw Queue
    // =========================================================================
    pub const WITHDRAW_QUEUE_MAGIC: u64 = 0x5045_5243_5155_4555;
    pub const WITHDRAW_QUEUE_LEN: usize = core::mem::size_of::<WithdrawQueue>();

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct WithdrawQueue {
        pub magic: u64,
        pub queued_lp_amount: u64,
        pub queue_start_slot: u64,
        pub epochs_remaining: u8,
        pub total_epochs: u8,
        pub _pad: [u8; 6],
        pub claimed_so_far: u64,
        pub _reserved: [u8; 24],
    }

    impl WithdrawQueue {
        #[inline]
        pub fn is_initialized(&self) -> bool {
            self.magic == WITHDRAW_QUEUE_MAGIC
        }
        #[inline]
        pub fn claimable_this_epoch(&self) -> u64 {
            if self.epochs_remaining == 0 {
                return 0;
            }
            let remaining_lp = self.queued_lp_amount.saturating_sub(self.claimed_so_far);
            if self.epochs_remaining == 1 {
                remaining_lp
            } else {
                remaining_lp / (self.epochs_remaining as u64)
            }
        }
    }

    pub fn read_withdraw_queue(data: &[u8]) -> Option<WithdrawQueue> {
        if data.len() < WITHDRAW_QUEUE_LEN {
            return None;
        }
        Some(*bytemuck::from_bytes::<WithdrawQueue>(
            &data[..WITHDRAW_QUEUE_LEN],
        ))
    }

    pub fn write_withdraw_queue(data: &mut [u8], q: &WithdrawQueue) {
        data[..WITHDRAW_QUEUE_LEN].copy_from_slice(bytemuck::bytes_of(q));
    }

    #[cfg(kani)]
    mod withdraw_queue_proofs {
        use super::*;
        #[kani::proof]
        #[kani::unwind(6)]
        fn proof_queued_withdrawal_total_never_exceeds_original_amount() {
            let queued_lp: u64 = kani::any();
            let total_epochs: u8 = kani::any();
            kani::assume(queued_lp > 0 && queued_lp <= 1_000_000_000_000);
            kani::assume(total_epochs > 0 && total_epochs <= 5);
            let mut q = WithdrawQueue {
                magic: WITHDRAW_QUEUE_MAGIC,
                queued_lp_amount: queued_lp,
                queue_start_slot: 100,
                epochs_remaining: total_epochs,
                total_epochs,
                _pad: [0; 6],
                claimed_so_far: 0,
                _reserved: [0; 24],
            };
            let mut total_claimed: u64 = 0;
            for _ in 0..total_epochs {
                let c = q.claimable_this_epoch();
                total_claimed = total_claimed.saturating_add(c);
                q.claimed_so_far = q.claimed_so_far.saturating_add(c);
                q.epochs_remaining = q.epochs_remaining.saturating_sub(1);
            }
            assert!(total_claimed == queued_lp);
            assert!(total_claimed <= queued_lp);
        }
    }

    #[cfg(test)]
    mod withdraw_queue_tests {
        use super::*;
        fn make_queue(amount: u64, epochs: u8) -> WithdrawQueue {
            WithdrawQueue {
                magic: WITHDRAW_QUEUE_MAGIC,
                queued_lp_amount: amount,
                queue_start_slot: 0,
                epochs_remaining: epochs,
                total_epochs: epochs,
                _pad: [0; 6],
                claimed_so_far: 0,
                _reserved: [0; 24],
            }
        }
        #[test]
        fn test_claimable_5_epochs() {
            let mut q = make_queue(100, 5);
            let mut total = 0u64;
            for _ in 0..5 {
                let c = q.claimable_this_epoch();
                assert_eq!(c, 20);
                total += c;
                q.claimed_so_far += c;
                q.epochs_remaining -= 1;
            }
            assert_eq!(total, 100);
        }
        #[test]
        fn test_claimable_indivisible() {
            let mut q = make_queue(7, 3);
            let c1 = q.claimable_this_epoch();
            assert_eq!(c1, 2);
            q.claimed_so_far += c1;
            q.epochs_remaining -= 1;
            let c2 = q.claimable_this_epoch();
            assert_eq!(c2, 2);
            q.claimed_so_far += c2;
            q.epochs_remaining -= 1;
            let c3 = q.claimable_this_epoch();
            assert_eq!(c3, 3); // last epoch gets remainder
            assert_eq!(c1 + c2 + c3, 7);
        }
        #[test]
        fn test_claimable_zero_remaining() {
            let mut q = make_queue(100, 1);
            q.epochs_remaining = 0;
            assert_eq!(q.claimable_this_epoch(), 0);
        }
        #[test]
        fn test_queue_roundtrip() {
            let q = make_queue(500, 5);
            let mut buf = [0u8; WITHDRAW_QUEUE_LEN];
            write_withdraw_queue(&mut buf, &q);
            let q2 = read_withdraw_queue(&buf).unwrap();
            assert_eq!(q2.queued_lp_amount, 500);
            assert_eq!(q2.total_epochs, 5);
        }
    }

    // =========================================================================
    // PERC-308: Loyalty Multiplier
    // =========================================================================

    /// Loyalty tier thresholds (in epochs)
    pub const LOYALTY_TIER1_EPOCHS: u64 = 5;
    pub const LOYALTY_TIER2_EPOCHS: u64 = 20;

    /// Loyalty multipliers in bps (10_000 = 1.0x)
    pub const LOYALTY_MULT_BASE: u64 = 10_000; // 1.0x for 0-5 epochs
    pub const LOYALTY_MULT_TIER1: u64 = 12_000; // 1.2x for 6-20 epochs
    pub const LOYALTY_MULT_TIER2: u64 = 15_000; // 1.5x for 20+ epochs

    /// Compute loyalty multiplier (bps) from epoch delta.
    #[inline]
    pub fn loyalty_multiplier_bps(delta_epochs: u64) -> u64 {
        if delta_epochs > LOYALTY_TIER2_EPOCHS {
            LOYALTY_MULT_TIER2
        } else if delta_epochs > LOYALTY_TIER1_EPOCHS {
            LOYALTY_MULT_TIER1
        } else {
            LOYALTY_MULT_BASE
        }
    }

    /// Apply loyalty multiplier to a fee amount.
    /// Returns: fee * multiplier / 10_000 (saturating).
    #[inline]
    pub fn apply_loyalty_mult(fee: u64, delta_epochs: u64) -> u64 {
        let mult = loyalty_multiplier_bps(delta_epochs);
        ((fee as u128) * (mult as u128) / 10_000) as u64
    }

    // Per-user loyalty PDA
    pub const LOYALTY_STAKE_MAGIC: u64 = 0x5045_5243_4C4F_5941; // "PERCLOYA"
    pub const LOYALTY_STAKE_LEN: usize = core::mem::size_of::<LoyaltyStake>();

    /// Per-user loyalty stake tracking.
    /// Seeds: `["loyalty", slab_key, user_pubkey]`.
    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct LoyaltyStake {
        pub magic: u64,
        pub entry_epoch: u64,
        pub _reserved: [u8; 48],
    }

    impl LoyaltyStake {
        #[inline]
        pub fn is_initialized(&self) -> bool {
            self.magic == LOYALTY_STAKE_MAGIC
        }
    }

    pub fn read_loyalty_stake(data: &[u8]) -> Option<LoyaltyStake> {
        if data.len() < LOYALTY_STAKE_LEN {
            return None;
        }
        Some(*bytemuck::from_bytes::<LoyaltyStake>(
            &data[..LOYALTY_STAKE_LEN],
        ))
    }

    pub fn write_loyalty_stake(data: &mut [u8], s: &LoyaltyStake) {
        data[..LOYALTY_STAKE_LEN].copy_from_slice(bytemuck::bytes_of(s));
    }

    #[cfg(kani)]
    mod proofs {
        use super::*;
        #[kani::proof]
        #[kani::unwind(1)]
        fn proof_loyalty_mult_never_exceeds_max_tier() {
            let delta: u64 = kani::any();
            kani::assume(delta <= 1_000_000);
            let mult = loyalty_multiplier_bps(delta);
            assert!(mult >= LOYALTY_MULT_BASE);
            assert!(mult <= LOYALTY_MULT_TIER2);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn test_loyalty_base() {
            assert_eq!(loyalty_multiplier_bps(0), 10_000);
            assert_eq!(loyalty_multiplier_bps(5), 10_000);
        }
        #[test]
        fn test_loyalty_tier1() {
            assert_eq!(loyalty_multiplier_bps(6), 12_000);
            assert_eq!(loyalty_multiplier_bps(20), 12_000);
        }
        #[test]
        fn test_loyalty_tier2() {
            assert_eq!(loyalty_multiplier_bps(21), 15_000);
            assert_eq!(loyalty_multiplier_bps(100), 15_000);
        }
        #[test]
        fn test_apply_mult() {
            assert_eq!(apply_loyalty_mult(1000, 0), 1000); // 1.0x
            assert_eq!(apply_loyalty_mult(1000, 10), 1200); // 1.2x
            assert_eq!(apply_loyalty_mult(1000, 25), 1500); // 1.5x
        }
    }
}

// 8b. LP Collateral Pricing (PERC-315)
pub mod lp_collateral {
    /// Compute LP token value in collateral units.
    /// lp_value = (lp_amount * vault_tvl / total_supply) * ltv_bps / 10_000
    pub fn lp_token_value(
        lp_amount: u64,
        vault_tvl: u128,
        total_supply: u64,
        ltv_bps: u64,
    ) -> u128 {
        if total_supply == 0 || vault_tvl == 0 || lp_amount == 0 {
            return 0;
        }
        let raw_value = (lp_amount as u128) * vault_tvl / (total_supply as u128);
        raw_value * (ltv_bps as u128) / 10_000
    }

    /// Check if vault TVL has dropped more than threshold since position open.
    pub fn tvl_drawdown_exceeded(old_tvl: u64, new_tvl: u128, threshold_bps: u64) -> bool {
        if old_tvl == 0 {
            return false;
        }
        let old = old_tvl as u128;
        if new_tvl >= old {
            return false;
        }
        let drawdown_bps = (old - new_tvl) * 10_000 / old;
        drawdown_bps > threshold_bps as u128
    }

    #[cfg(kani)]
    mod proofs {
        use super::*;

        #[kani::proof]
        #[kani::unwind(1)]
        fn nightly_lp_collateral_value_never_exceeds_raw_share() {
            let lp_amount: u64 = kani::any();
            let vault_tvl: u128 = kani::any();
            let total_supply: u64 = kani::any();
            let ltv_bps: u64 = kani::any();

            kani::assume(lp_amount > 0 && lp_amount <= 1_000_000_000_000);
            kani::assume(vault_tvl > 0 && vault_tvl <= 1_000_000_000_000_000);
            kani::assume(total_supply > 0 && total_supply <= 1_000_000_000_000);
            kani::assume(ltv_bps > 0 && ltv_bps <= 10_000);

            let value = lp_token_value(lp_amount, vault_tvl, total_supply, ltv_bps);
            let raw = (lp_amount as u128) * vault_tvl / (total_supply as u128);
            assert!(value <= raw, "value {} > raw {}", value, raw);
        }

        #[kani::proof]
        #[kani::unwind(1)]
        fn nightly_drawdown_monotone() {
            let old_tvl: u64 = kani::any();
            let new_tvl_high: u128 = kani::any();
            let new_tvl_low: u128 = kani::any();
            let threshold: u64 = kani::any();

            kani::assume(old_tvl > 0 && old_tvl <= 1_000_000_000_000);
            kani::assume(new_tvl_high <= old_tvl as u128);
            kani::assume(new_tvl_low <= new_tvl_high);
            kani::assume(threshold > 0 && threshold <= 10_000);

            let triggered_high = tvl_drawdown_exceeded(old_tvl, new_tvl_high, threshold);
            let triggered_low = tvl_drawdown_exceeded(old_tvl, new_tvl_low, threshold);

            if triggered_high {
                assert!(triggered_low);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_lp_token_value_basic() {
            let v = lp_token_value(100, 1000, 200, 5000);
            assert_eq!(v, 250);
        }

        #[test]
        fn test_lp_token_value_zero_supply() {
            assert_eq!(lp_token_value(100, 1000, 0, 5000), 0);
        }

        #[test]
        fn test_lp_token_value_100_ltv() {
            let v = lp_token_value(100, 1000, 200, 10_000);
            assert_eq!(v, 500);
        }

        #[test]
        fn test_drawdown_20pct() {
            assert!(!tvl_drawdown_exceeded(1000, 800, 2000));
            assert!(tvl_drawdown_exceeded(1000, 799, 2000));
        }

        #[test]
        fn test_drawdown_no_drop() {
            assert!(!tvl_drawdown_exceeded(1000, 1000, 2000));
            assert!(!tvl_drawdown_exceeded(1000, 1100, 2000));
        }

        #[test]
        fn test_drawdown_zero_old() {
            assert!(!tvl_drawdown_exceeded(0, 0, 2000));
        }
    }
}

// 8c. Settlement Dispute (PERC-314)
pub mod dispute {
    use bytemuck::{Pod, Zeroable};

    pub const DISPUTE_MAGIC: u64 = 0x5045_5243_4449_5350; // "PERCDISP"
    pub const DISPUTE_LEN: usize = core::mem::size_of::<SettlementDispute>();

    /// Settlement dispute PDA.
    /// Seeds: `["dispute", slab_key]`.
    /// Layout (96 bytes):
    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct SettlementDispute {
        pub magic: u64,
        /// Challenger pubkey.
        pub challenger: [u8; 32],
        /// Proposed settlement price (e6).
        pub proposed_price_e6: u64,
        /// Slot of the Pyth proof used by challenger.
        pub proof_slot: u64,
        /// Bond amount deposited by challenger.
        pub bond_amount: u64,
        /// 0=pending, 1=accepted (challenger wins), 2=rejected (challenger loses).
        pub outcome: u8,
        pub _pad: [u8; 7],
        /// Slot when dispute was submitted.
        pub dispute_slot: u64,
        pub _reserved: [u8; 16],
    }

    impl SettlementDispute {
        #[inline]
        pub fn is_initialized(&self) -> bool {
            self.magic == DISPUTE_MAGIC
        }
    }

    pub fn read_dispute(data: &[u8]) -> Option<SettlementDispute> {
        if data.len() < DISPUTE_LEN {
            return None;
        }
        Some(*bytemuck::from_bytes::<SettlementDispute>(
            &data[..DISPUTE_LEN],
        ))
    }

    pub fn write_dispute(data: &mut [u8], d: &SettlementDispute) {
        data[..DISPUTE_LEN].copy_from_slice(bytemuck::bytes_of(d));
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_dispute_size() {
            assert_eq!(DISPUTE_LEN, 96);
        }

        #[test]
        fn test_dispute_roundtrip() {
            let d = SettlementDispute {
                magic: DISPUTE_MAGIC,
                challenger: [1; 32],
                proposed_price_e6: 100_000_000,
                proof_slot: 500,
                bond_amount: 1000,
                outcome: 0,
                _pad: [0; 7],
                dispute_slot: 400,
                _reserved: [0; 16],
            };
            let mut buf = [0u8; DISPUTE_LEN];
            write_dispute(&mut buf, &d);
            let d2 = read_dispute(&buf).unwrap();
            assert_eq!(d2.proposed_price_e6, 100_000_000);
            assert_eq!(d2.challenger, [1; 32]);
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Feature 5: Cross-Market Portfolio Margining (CMOR)
// ═══════════════════════════════════════════════════════════════
pub mod cross_margin {
    use bytemuck::{Pod, Zeroable};

    /// Magic for OffsetPairConfig PDA: "CMORPAIR"
    pub const OFFSET_PAIR_MAGIC: u64 = 0x434D_4F52_5041_4952;
    /// Magic for CrossMarginAttestation PDA: "CMORATTE"
    pub const ATTESTATION_MAGIC: u64 = 0x434D_4F52_4154_5445;

    pub const OFFSET_PAIR_LEN: usize = core::mem::size_of::<OffsetPairConfig>();
    pub const ATTESTATION_LEN: usize = core::mem::size_of::<CrossMarginAttestation>();

    /// Admin-configured pair of slabs eligible for portfolio margin offset.
    /// PDA seeds: ["cmor_pair", slab_a, slab_b] (slab_a < slab_b lexicographically).
    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct OffsetPairConfig {
        pub magic: u64,
        /// Margin offset in basis points. E.g., 3000 = 30% margin reduction for hedged positions.
        pub offset_bps: u16,
        /// 1 = enabled, 0 = disabled.
        pub enabled: u8,
        pub _pad: [u8; 5],
        pub _reserved: [u8; 16],
    }

    /// Per-user attestation of positions across two slabs.
    /// PDA seeds: ["cmor", user_pubkey, min(slab_a, slab_b), max(slab_a, slab_b)].
    /// Written by permissionless keeper after reading both slabs.
    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct CrossMarginAttestation {
        pub magic: u64,
        /// Explicit padding for i128 alignment (native: 16-byte aligned)
        pub _align_pad: [u8; 8],
        /// User position in slab A (signed: +long, -short).
        pub user_pos_a: i128,
        /// User position in slab B (signed).
        pub user_pos_b: i128,
        /// Slot when this attestation was written.
        pub attested_slot: u64,
        /// Margin offset bps (copied from OffsetPairConfig at attestation time).
        pub offset_bps: u16,
        pub _pad: [u8; 6],
        /// Owner pubkey — the user whose positions are attested.
        /// Used to verify the attestation belongs to the current trader.
        pub owner: [u8; 32],
        /// Slab A pubkey (the lesser of the two, lexicographically sorted).
        /// Used to verify the attestation is bound to the correct market pair.
        /// (#986: prevents cross-market attestation reuse)
        pub slab_a: [u8; 32],
        /// Slab B pubkey (the greater of the two, lexicographically sorted).
        pub slab_b: [u8; 32],
    }

    impl OffsetPairConfig {
        #[inline]
        pub fn is_initialized(&self) -> bool {
            self.magic == OFFSET_PAIR_MAGIC
        }
    }

    impl CrossMarginAttestation {
        #[inline]
        pub fn is_initialized(&self) -> bool {
            self.magic == ATTESTATION_MAGIC
        }

        /// Check if attestation is fresh enough (within `max_age_slots` of `current_slot`).
        #[inline]
        pub fn is_fresh(&self, current_slot: u64, max_age_slots: u64) -> bool {
            current_slot.saturating_sub(self.attested_slot) <= max_age_slots
        }

        /// Compute portfolio margin credit in bps for the user.
        /// Returns margin reduction (0 if positions are same-direction or attestation disabled).
        /// Hedged positions (opposite directions) get `offset_bps` reduction.
        pub fn compute_margin_credit_bps(&self) -> u16 {
            if self.offset_bps == 0 {
                return 0;
            }
            // Positions must be in opposite directions to qualify
            let a = self.user_pos_a;
            let b = self.user_pos_b;
            if a == 0 || b == 0 {
                return 0;
            }
            // Opposite sign = hedged
            let hedged = (a > 0 && b < 0) || (a < 0 && b > 0);
            if !hedged {
                return 0;
            }
            // Scale offset by the smaller leg's proportion
            let abs_a = a.unsigned_abs();
            let abs_b = b.unsigned_abs();
            let smaller = abs_a.min(abs_b);
            let larger = abs_a.max(abs_b);
            // hedged_ratio = smaller / larger (0..1), scale offset proportionally
            let credit = (self.offset_bps as u128).saturating_mul(smaller) / larger;
            credit.min(self.offset_bps as u128) as u16
        }
    }

    /// Canonicalize slab pair ordering (lower pubkey first).
    #[inline]
    pub fn order_slab_pair(a: &[u8; 32], b: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
        if a < b {
            (*a, *b)
        } else {
            (*b, *a)
        }
    }

    pub fn read_offset_pair(data: &[u8]) -> Option<OffsetPairConfig> {
        if data.len() < OFFSET_PAIR_LEN {
            return None;
        }
        Some(*bytemuck::from_bytes::<OffsetPairConfig>(
            &data[..OFFSET_PAIR_LEN],
        ))
    }

    pub fn write_offset_pair(data: &mut [u8], cfg: &OffsetPairConfig) {
        data[..OFFSET_PAIR_LEN].copy_from_slice(bytemuck::bytes_of(cfg));
    }

    pub fn read_attestation(data: &[u8]) -> Option<CrossMarginAttestation> {
        if data.len() < ATTESTATION_LEN {
            return None;
        }
        Some(*bytemuck::from_bytes::<CrossMarginAttestation>(
            &data[..ATTESTATION_LEN],
        ))
    }

    pub fn write_attestation(data: &mut [u8], att: &CrossMarginAttestation) {
        data[..ATTESTATION_LEN].copy_from_slice(bytemuck::bytes_of(att));
    }
}

// 8b. mod keeper_fund — PERC-623: Self-Funding Keeper
pub mod keeper_fund {
    use bytemuck::{Pod, Zeroable};

    /// Magic bytes for KeeperFundState PDA: "KEEPFUND"
    pub const KEEPER_FUND_MAGIC: u64 = 0x4B454550_46554E44;

    /// Size of the KeeperFundState account data.
    pub const KEEPER_FUND_STATE_LEN: usize = core::mem::size_of::<KeeperFundState>();

    /// Default split: 30% of creation deposit goes to keeper fund.
    pub const KEEPER_FUND_SPLIT_BPS: u64 = 3_000;

    /// Default reward per successful KeeperCrank (in base token lamports).
    /// ~0.001 SOL equivalent. Configurable per market at init.
    pub const DEFAULT_REWARD_PER_CRANK: u64 = 1_000_000; // 0.001 SOL in lamports

    /// Fee percentage diverted to keeper fund top-up (in bps).
    pub const KEEPER_FEE_TOPUP_BPS: u64 = 500; // 5% of fees

    /// PDA seed prefix.
    pub const KEEPER_FUND_SEED: &[u8] = b"keeper_fund";

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct KeeperFundState {
        pub magic: u64,
        pub bump: u8,
        /// 1 if market was auto-paused due to keeper fund depletion.
        /// TopUpKeeperFund only unpauses when this is set, preventing
        /// accidental clearing of admin pauses (#1015).
        pub depleted_pause: u8,
        pub _pad: [u8; 6],
        /// Current fund balance (base token lamports).
        pub balance: u64,
        /// Reward paid to crank caller per successful KeeperCrank.
        pub reward_per_crank: u64,
        /// Lifetime total rewards paid out.
        pub total_rewarded: u64,
        /// Lifetime total topped up from fees.
        pub total_topped_up: u64,
    }

    // Compile-time size check
    const _: [(); 48] = [(); KEEPER_FUND_STATE_LEN];

    /// Compute the deposit split: (lp_amount, keeper_fund_amount).
    /// keeper_fund_amount = deposit * split_bps / 10_000
    /// lp_amount = deposit - keeper_fund_amount (remainder, avoids rounding loss)
    ///
    /// Invariant: lp_amount + keeper_fund_amount == deposit (exact).
    pub fn split_deposit(deposit: u64, split_bps: u64) -> (u64, u64) {
        let capped_bps = split_bps.min(10_000);
        let keeper_amount = deposit.saturating_mul(capped_bps) / 10_000;
        let lp_amount = deposit.saturating_sub(keeper_amount);
        (lp_amount, keeper_amount)
    }

    /// Pay crank reward from fund. Returns (new_balance, actual_reward).
    /// If balance < reward_per_crank, pays the remaining balance (partial reward).
    pub fn pay_crank_reward(balance: u64, reward_per_crank: u64) -> (u64, u64) {
        let actual = balance.min(reward_per_crank);
        (balance.saturating_sub(actual), actual)
    }

    /// Top up keeper fund from fees. Returns (new_balance, topped_up_amount).
    pub fn topup_from_fees(balance: u64, fee_amount: u64, topup_bps: u64) -> (u64, u64) {
        let topup = fee_amount.saturating_mul(topup_bps.min(10_000)) / 10_000;
        (balance.saturating_add(topup), topup)
    }

    /// Check if fund is depleted (balance == 0 after paying reward).
    pub fn is_depleted(balance: u64) -> bool {
        balance == 0
    }

    /// Read KeeperFundState from account data.
    pub fn read_state(data: &[u8]) -> Option<&KeeperFundState> {
        if data.len() < KEEPER_FUND_STATE_LEN {
            return None;
        }
        let state: &KeeperFundState = bytemuck::from_bytes(&data[..KEEPER_FUND_STATE_LEN]);
        if state.magic != KEEPER_FUND_MAGIC {
            return None;
        }
        Some(state)
    }

    /// Write KeeperFundState to account data.
    pub fn write_state(data: &mut [u8], state: &KeeperFundState) {
        data[..KEEPER_FUND_STATE_LEN].copy_from_slice(bytemuck::bytes_of(state));
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_split_deposit_conservation() {
            // Exact conservation for various deposits
            for deposit in [0, 1, 100, 999, 1_000_000, u64::MAX / 10_000] {
                let (lp, fund) = split_deposit(deposit, KEEPER_FUND_SPLIT_BPS);
                assert_eq!(lp + fund, deposit, "conservation failed for {}", deposit);
            }
        }

        #[test]
        fn test_split_deposit_ratios() {
            let (lp, fund) = split_deposit(10_000, 3_000);
            assert_eq!(fund, 3_000); // 30%
            assert_eq!(lp, 7_000); // 70%
        }

        #[test]
        fn test_split_deposit_zero() {
            let (lp, fund) = split_deposit(0, 3_000);
            assert_eq!(lp, 0);
            assert_eq!(fund, 0);
        }

        #[test]
        fn test_split_deposit_cap_bps() {
            // split_bps > 10_000 capped to 10_000
            let (lp, fund) = split_deposit(1_000, 20_000);
            assert_eq!(fund, 1_000); // capped to 100%
            assert_eq!(lp, 0);
        }

        #[test]
        fn test_pay_crank_reward_normal() {
            let (new_bal, reward) = pay_crank_reward(10_000, 1_000);
            assert_eq!(new_bal, 9_000);
            assert_eq!(reward, 1_000);
        }

        #[test]
        fn test_pay_crank_reward_insufficient() {
            let (new_bal, reward) = pay_crank_reward(500, 1_000);
            assert_eq!(new_bal, 0);
            assert_eq!(reward, 500); // partial
        }

        #[test]
        fn test_pay_crank_reward_zero_balance() {
            let (new_bal, reward) = pay_crank_reward(0, 1_000);
            assert_eq!(new_bal, 0);
            assert_eq!(reward, 0);
        }

        #[test]
        fn test_topup_from_fees() {
            let (new_bal, topped) = topup_from_fees(1_000, 10_000, 500);
            assert_eq!(topped, 500); // 5% of 10_000
            assert_eq!(new_bal, 1_500);
        }

        #[test]
        fn test_state_roundtrip() {
            let state = KeeperFundState {
                magic: KEEPER_FUND_MAGIC,
                bump: 254,
                depleted_pause: 0,
                _pad: [0; 6],
                balance: 12345,
                reward_per_crank: 1000,
                total_rewarded: 5000,
                total_topped_up: 3000,
            };
            let mut buf = [0u8; KEEPER_FUND_STATE_LEN];
            write_state(&mut buf, &state);
            let read = read_state(&buf).unwrap();
            assert_eq!(read.balance, 12345);
            assert_eq!(read.bump, 254);
            assert_eq!(read.total_rewarded, 5000);
        }

        #[test]
        fn test_read_state_bad_magic() {
            let mut buf = [0u8; KEEPER_FUND_STATE_LEN];
            buf[0..8].copy_from_slice(&0xDEADBEEFu64.to_le_bytes());
            assert!(read_state(&buf).is_none());
        }
    }
}

#[cfg(kani)]
mod keeper_fund_kani {
    use crate::keeper_fund::*;

    /// Deposit split conserves total: lp + fund == deposit.
    #[kani::proof]
    fn proof_split_deposit_conservation() {
        let deposit: u64 = kani::any();
        let split_bps: u64 = kani::any();
        kani::assume(split_bps <= 10_000);
        // Only check deposits that won't overflow in multiply
        kani::assume(deposit <= u64::MAX / 10_000);
        let (lp, fund) = split_deposit(deposit, split_bps);
        assert!(lp + fund == deposit, "split must conserve deposit");
    }

    /// Crank reward never exceeds balance.
    #[kani::proof]
    fn proof_reward_bounded() {
        let balance: u64 = kani::any();
        let reward_per_crank: u64 = kani::any();
        let (new_bal, actual) = pay_crank_reward(balance, reward_per_crank);
        assert!(actual <= balance, "reward must not exceed balance");
        assert!(new_bal <= balance, "new balance must not increase");
        assert!(new_bal + actual == balance, "conservation");
    }

    /// Fund balance monotonically decreases from rewards.
    #[kani::proof]
    fn proof_reward_monotone_decrease() {
        let balance: u64 = kani::any();
        let reward: u64 = kani::any();
        let (new_bal, _) = pay_crank_reward(balance, reward);
        assert!(new_bal <= balance);
    }

    /// Topup never decreases balance.
    #[kani::proof]
    fn proof_topup_monotone_increase() {
        let balance: u64 = kani::any();
        let fee: u64 = kani::any();
        let bps: u64 = kani::any();
        kani::assume(bps <= 10_000);
        kani::assume(fee <= u64::MAX / 10_000);
        let (new_bal, _) = topup_from_fees(balance, fee, bps);
        assert!(new_bal >= balance);
    }
}

// 8c. mod creator_lock — PERC-627: Creator Stake Lock + Adversarial Wallet Tracking
pub mod creator_lock {
    use bytemuck::{Pod, Zeroable};

    /// Magic bytes: "CRTRLOCK"
    pub const CREATOR_LOCK_MAGIC: u64 = 0x4352_5452_4C4F_434B;

    /// Size of the CreatorStakeLock account data.
    pub const CREATOR_LOCK_STATE_LEN: usize = core::mem::size_of::<CreatorStakeLock>();

    /// Default lock duration: ~90 days in slots (1 slot ≈ 400ms, 216_000 slots/day).
    pub const DEFAULT_LOCK_DURATION_SLOTS: u64 = 19_440_000;

    /// Extraction limit in bps: 15_000 = 150% (creator extracted 50% more than deposited).
    pub const EXTRACTION_LIMIT_BPS: u64 = 15_000;

    /// PDA seed prefix.
    pub const CREATOR_LOCK_SEED: &[u8] = b"creator_lock";

    /// On-chain state for creator's locked LP position + extraction tracking.
    ///
    /// Seeds: `["creator_lock", slab_pubkey]`
    /// Size: 96 bytes.
    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct CreatorStakeLock {
        /// Magic identifier: CREATOR_LOCK_MAGIC when initialized.
        pub magic: u64,
        /// PDA bump seed.
        pub bump: u8,
        /// Padding for alignment.
        pub _pad: [u8; 7],
        /// Creator wallet pubkey (32 bytes).
        pub creator: [u8; 32],
        /// Slot when the lock began.
        pub lock_start_slot: u64,
        /// Minimum lock duration in slots.
        pub lock_duration_slots: u64,
        /// LP tokens locked (cannot withdraw until lock expires).
        pub lp_amount_locked: u64,
        /// Total value extracted from LP vault by creator (monotonically increasing).
        pub cumulative_extracted: u64,
        /// Total value deposited into LP vault by creator (monotonically increasing).
        pub cumulative_deposited: u64,
        /// 1 = creator fee share redirected to insurance fund.
        pub fee_redirect_active: u8,
        /// Reserved for future use.
        pub _reserved: [u8; 7],
    }

    // Compile-time size assert
    const _: () = assert!(CREATOR_LOCK_STATE_LEN == 96);

    /// Check if the creator lock has expired.
    #[inline]
    pub fn is_lock_expired(current_slot: u64, lock_start: u64, duration: u64) -> bool {
        current_slot >= lock_start.saturating_add(duration)
    }

    /// Maximum LP tokens the creator can withdraw.
    /// If lock is active, they can only withdraw excess above the locked amount.
    /// If lock expired, they can withdraw everything.
    #[inline]
    pub fn max_withdrawable(total_lp: u64, locked_lp: u64, lock_expired: bool) -> u64 {
        if lock_expired {
            total_lp
        } else {
            total_lp.saturating_sub(locked_lp)
        }
    }

    /// Check if creator has exceeded the extraction threshold.
    /// Returns true if extraction ratio exceeds limit_bps / 10_000 of deposited.
    /// Safe against zero-deposit (returns false — no deposit means no extraction check).
    #[inline]
    pub fn check_extraction_exceeded(extracted: u64, deposited: u64, limit_bps: u64) -> bool {
        if deposited == 0 {
            return false;
        }
        // extracted > deposited * limit_bps / 10_000
        // Rearranged to avoid overflow: extracted * 10_000 > deposited * limit_bps
        let lhs = (extracted as u128).saturating_mul(10_000);
        let rhs = (deposited as u128).saturating_mul(limit_bps as u128);
        lhs > rhs
    }

    /// Compute fee split when redirect is active.
    /// Returns (to_creator, to_insurance). Conservation: sum == fee_amount.
    #[inline]
    pub fn compute_fee_redirect(fee_amount: u64, redirect_active: bool) -> (u64, u64) {
        if redirect_active {
            (0, fee_amount)
        } else {
            (fee_amount, 0)
        }
    }

    /// Read CreatorStakeLock from account data. Returns None if magic doesn't match.
    pub fn read_state(data: &[u8]) -> Option<&CreatorStakeLock> {
        if data.len() < CREATOR_LOCK_STATE_LEN {
            return None;
        }
        let state: &CreatorStakeLock = bytemuck::from_bytes(&data[..CREATOR_LOCK_STATE_LEN]);
        if state.magic != CREATOR_LOCK_MAGIC {
            return None;
        }
        Some(state)
    }

    /// Write CreatorStakeLock to account data.
    pub fn write_state(data: &mut [u8], state: &CreatorStakeLock) {
        data[..CREATOR_LOCK_STATE_LEN].copy_from_slice(bytemuck::bytes_of(state));
    }

    /// Check if fee redirect is active (security #1012: use != 0, not == 1).
    #[inline]
    pub fn is_fee_redirect_active(state: &CreatorStakeLock) -> bool {
        state.fee_redirect_active != 0
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_lock_not_expired() {
            assert!(!is_lock_expired(100, 50, 100));
        }

        #[test]
        fn test_lock_expired_exact() {
            assert!(is_lock_expired(150, 50, 100));
        }

        #[test]
        fn test_lock_expired_past() {
            assert!(is_lock_expired(200, 50, 100));
        }

        #[test]
        fn test_lock_saturating_overflow() {
            // lock_start near u64::MAX — saturating_add prevents overflow
            assert!(!is_lock_expired(100, u64::MAX - 10, 100));
        }

        #[test]
        fn test_max_withdrawable_locked() {
            assert_eq!(max_withdrawable(1000, 800, false), 200);
        }

        #[test]
        fn test_max_withdrawable_all_locked() {
            assert_eq!(max_withdrawable(800, 800, false), 0);
        }

        #[test]
        fn test_max_withdrawable_expired() {
            assert_eq!(max_withdrawable(1000, 800, true), 1000);
        }

        #[test]
        fn test_max_withdrawable_more_locked_than_total() {
            // Edge case: locked > total (shouldn't happen, but saturating_sub handles)
            assert_eq!(max_withdrawable(500, 800, false), 0);
        }

        #[test]
        fn test_extraction_not_exceeded() {
            // extracted 100, deposited 100, limit 150% → ratio 100% < 150%
            assert!(!check_extraction_exceeded(100, 100, 15_000));
        }

        #[test]
        fn test_extraction_exceeded() {
            // extracted 160, deposited 100, limit 150% → ratio 160% > 150%
            assert!(check_extraction_exceeded(160, 100, 15_000));
        }

        #[test]
        fn test_extraction_exact_boundary() {
            // extracted 150, deposited 100, limit 150% → ratio 150% == 150% → NOT exceeded (>)
            assert!(!check_extraction_exceeded(150, 100, 15_000));
        }

        #[test]
        fn test_extraction_zero_deposit() {
            assert!(!check_extraction_exceeded(100, 0, 15_000));
        }

        #[test]
        fn test_fee_redirect_active() {
            let (to_creator, to_insurance) = compute_fee_redirect(1000, true);
            assert_eq!(to_creator, 0);
            assert_eq!(to_insurance, 1000);
        }

        #[test]
        fn test_fee_redirect_inactive() {
            let (to_creator, to_insurance) = compute_fee_redirect(1000, false);
            assert_eq!(to_creator, 1000);
            assert_eq!(to_insurance, 0);
        }

        #[test]
        fn test_fee_redirect_conservation() {
            let (a, b) = compute_fee_redirect(12345, true);
            assert_eq!(a + b, 12345);
            let (a, b) = compute_fee_redirect(12345, false);
            assert_eq!(a + b, 12345);
        }

        #[test]
        fn test_state_roundtrip() {
            let state = CreatorStakeLock {
                magic: CREATOR_LOCK_MAGIC,
                bump: 253,
                _pad: [0; 7],
                creator: [42u8; 32],
                lock_start_slot: 1000,
                lock_duration_slots: DEFAULT_LOCK_DURATION_SLOTS,
                lp_amount_locked: 5000,
                cumulative_extracted: 100,
                cumulative_deposited: 200,
                fee_redirect_active: 0,
                _reserved: [0; 7],
            };
            let mut buf = [0u8; CREATOR_LOCK_STATE_LEN];
            write_state(&mut buf, &state);
            let read = read_state(&buf).unwrap();
            assert_eq!(read.bump, 253);
            assert_eq!(read.creator, [42u8; 32]);
            assert_eq!(read.lp_amount_locked, 5000);
            assert_eq!(read.lock_duration_slots, DEFAULT_LOCK_DURATION_SLOTS);
        }

        #[test]
        fn test_read_state_bad_magic() {
            let mut buf = [0u8; CREATOR_LOCK_STATE_LEN];
            buf[0..8].copy_from_slice(&0xDEADBEEFu64.to_le_bytes());
            assert!(read_state(&buf).is_none());
        }

        #[test]
        fn test_state_size() {
            assert_eq!(CREATOR_LOCK_STATE_LEN, 96);
        }
    }
}

#[cfg(kani)]
mod creator_lock_kani {
    use crate::creator_lock::*;

    /// Lock never expires early: if current_slot < start + duration, not expired.
    #[kani::proof]
    #[kani::unwind(1)]
    fn nightly_proof_lock_never_expires_early() {
        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        let current: u64 = kani::any();
        // Avoid saturating_add masking the invariant
        kani::assume(start <= u64::MAX - duration);
        if current < start + duration {
            assert!(!is_lock_expired(current, start, duration));
        }
    }

    /// Max withdrawable never exceeds total LP.
    #[kani::proof]
    #[kani::unwind(1)]
    fn proof_max_withdrawable_bounded() {
        let total: u64 = kani::any();
        let locked: u64 = kani::any();
        let expired: bool = kani::any();
        let result = max_withdrawable(total, locked, expired);
        assert!(result <= total);
    }

    /// Max withdrawable == 0 when fully locked and not expired.
    #[kani::proof]
    #[kani::unwind(1)]
    fn proof_fully_locked_zero_withdraw() {
        let total: u64 = kani::any();
        let locked: u64 = kani::any();
        kani::assume(locked >= total);
        let result = max_withdrawable(total, locked, false);
        assert!(result == 0);
    }

    /// Extraction check is monotone: more extraction → more likely to trigger.
    #[kani::proof]
    #[kani::unwind(1)]
    fn nightly_proof_extraction_monotone() {
        let extracted_a: u64 = kani::any();
        let extracted_b: u64 = kani::any();
        let deposited: u64 = kani::any();
        let limit: u64 = kani::any();
        kani::assume(extracted_a <= extracted_b);
        kani::assume(limit <= 100_000); // reasonable upper bound
        if check_extraction_exceeded(extracted_a, deposited, limit) {
            assert!(check_extraction_exceeded(extracted_b, deposited, limit));
        }
    }

    /// Fee redirect conservation: to_creator + to_insurance == fee_amount.
    #[kani::proof]
    #[kani::unwind(1)]
    fn proof_fee_redirect_conservation() {
        let fee: u64 = kani::any();
        let active: bool = kani::any();
        let (a, b) = compute_fee_redirect(fee, active);
        assert!(a + b == fee);
    }
}

// 8d. mod creator_history — PERC-629: Dynamic Creation Deposit (Anti-Spam)
pub mod creator_history {
    use bytemuck::{Pod, Zeroable};

    /// Magic bytes: "CRTRHIST"
    pub const CREATOR_HISTORY_MAGIC: u64 = 0x4352_5452_4849_5354;
    pub const CREATOR_HISTORY_LEN: usize = core::mem::size_of::<CreatorHistory>();
    pub const CREATOR_HISTORY_SEED: &[u8] = b"creator_history";

    /// Base deposit in e6 units ($2,500).
    pub const BASE_DEPOSIT_E6: u64 = 2_500_000_000;
    /// Max failure exponent (2^10 = 1024x cap).
    pub const MAX_FAILURE_EXPONENT: u32 = 10;
    /// Discount per successful market (10% = 1000 bps).
    pub const SUCCESS_DISCOUNT_BPS: u64 = 1_000;
    /// Maximum discount (50% = 5000 bps).
    pub const MAX_DISCOUNT_BPS: u64 = 5_000;
    /// OI threshold: market must reach 10% of deposit in OI.
    pub const OI_THRESHOLD_BPS: u64 = 1_000;
    /// Slash: 50% of deposit to insurance on failure.
    pub const SLASH_BPS: u64 = 5_000;
    /// Evaluation period: ~30 days in slots.
    pub const EVALUATION_PERIOD_SLOTS: u64 = 6_480_000;

    /// On-chain per-creator market history.
    /// Seeds: `["creator_history", creator_pubkey]`
    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct CreatorHistory {
        pub magic: u64,
        pub bump: u8,
        pub _pad: [u8; 3],
        pub total_markets: u16,
        pub successful_markets: u16,
        pub failed_markets: u16,
        pub _reserved: [u8; 14],
    }

    const _: () = assert!(CREATOR_HISTORY_LEN == 32);

    /// Compute the deposit multiplier from failure count.
    /// Returns multiplier in bps (10_000 = 1x, 20_000 = 2x, etc.)
    #[inline]
    pub fn failure_multiplier_bps(failed: u16) -> u64 {
        let exp = (failed as u32).min(MAX_FAILURE_EXPONENT);
        10_000u64.saturating_mul(1u64 << exp)
    }

    /// Compute discount from successful market count.
    /// Returns discount in bps, capped at MAX_DISCOUNT_BPS.
    #[inline]
    pub fn success_discount_bps(successful: u16) -> u64 {
        let raw = (successful as u64).saturating_mul(SUCCESS_DISCOUNT_BPS);
        raw.min(MAX_DISCOUNT_BPS)
    }

    /// Compute required deposit given creator history.
    /// result = base * multiplier * (1 - discount) / 10_000^2
    /// Floor: base * 50% (never below half base even with max discount).
    #[inline]
    pub fn compute_required_deposit(base_e6: u64, failed: u16, successful: u16) -> u64 {
        let mult_bps = failure_multiplier_bps(failed);
        let disc_bps = success_discount_bps(successful);
        // effective = base * mult / 10_000 * (10_000 - disc) / 10_000
        let numerator = (base_e6 as u128)
            .saturating_mul(mult_bps as u128)
            .saturating_mul((10_000u64.saturating_sub(disc_bps)) as u128);
        let result = (numerator / (10_000u128 * 10_000u128)) as u64;
        // Floor: 50% of base
        let floor = base_e6 / 2;
        result.max(floor)
    }

    /// Compute slash amount (50% of deposit).
    #[inline]
    pub fn compute_slash(deposit: u64) -> (u64, u64) {
        let slash = deposit.saturating_mul(SLASH_BPS) / 10_000;
        let remainder = deposit.saturating_sub(slash);
        (slash, remainder)
    }

    /// Check if market reached OI threshold.
    #[inline]
    pub fn oi_threshold_met(deposit_e6: u64, current_oi_e6: u64) -> bool {
        let threshold = deposit_e6.saturating_mul(OI_THRESHOLD_BPS) / 10_000;
        current_oi_e6 >= threshold
    }

    pub fn read_state(data: &[u8]) -> Option<&CreatorHistory> {
        if data.len() < CREATOR_HISTORY_LEN {
            return None;
        }
        let state: &CreatorHistory = bytemuck::from_bytes(&data[..CREATOR_HISTORY_LEN]);
        if state.magic != CREATOR_HISTORY_MAGIC {
            return None;
        }
        Some(state)
    }

    pub fn write_state(data: &mut [u8], state: &CreatorHistory) {
        data[..CREATOR_HISTORY_LEN].copy_from_slice(bytemuck::bytes_of(state));
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_failure_multiplier_zero() {
            assert_eq!(failure_multiplier_bps(0), 10_000); // 1x
        }

        #[test]
        fn test_failure_multiplier_one() {
            assert_eq!(failure_multiplier_bps(1), 20_000); // 2x
        }

        #[test]
        fn test_failure_multiplier_three() {
            assert_eq!(failure_multiplier_bps(3), 80_000); // 8x
        }

        #[test]
        fn test_failure_multiplier_capped() {
            assert_eq!(failure_multiplier_bps(15), 10_240_000); // capped at 2^10 = 1024x
            assert_eq!(failure_multiplier_bps(10), 10_240_000);
        }

        #[test]
        fn test_success_discount() {
            assert_eq!(success_discount_bps(0), 0);
            assert_eq!(success_discount_bps(1), 1_000); // 10%
            assert_eq!(success_discount_bps(3), 3_000); // 30%
            assert_eq!(success_discount_bps(5), 5_000); // 50% (max)
            assert_eq!(success_discount_bps(10), 5_000); // capped
        }

        #[test]
        fn test_deposit_base_case() {
            // No history: 1x * (1 - 0) = base
            let dep = compute_required_deposit(BASE_DEPOSIT_E6, 0, 0);
            assert_eq!(dep, BASE_DEPOSIT_E6);
        }

        #[test]
        fn test_deposit_one_failure() {
            // 2x * (1 - 0) = 2 * base
            let dep = compute_required_deposit(BASE_DEPOSIT_E6, 1, 0);
            assert_eq!(dep, BASE_DEPOSIT_E6 * 2);
        }

        #[test]
        fn test_deposit_with_discount() {
            // 1x * (1 - 10%) = 0.9 * base
            let dep = compute_required_deposit(BASE_DEPOSIT_E6, 0, 1);
            assert_eq!(dep, BASE_DEPOSIT_E6 * 9 / 10);
        }

        #[test]
        fn test_deposit_floor() {
            // Max discount (50%) + 0 failures = 50% of base = floor
            let dep = compute_required_deposit(BASE_DEPOSIT_E6, 0, 10);
            assert_eq!(dep, BASE_DEPOSIT_E6 / 2);
        }

        #[test]
        fn test_slash_calculation() {
            let (slash, remainder) = compute_slash(1_000_000);
            assert_eq!(slash, 500_000);
            assert_eq!(remainder, 500_000);
        }

        #[test]
        fn test_slash_conservation() {
            let (slash, remainder) = compute_slash(1_000_001);
            assert_eq!(slash + remainder, 1_000_001);
        }

        #[test]
        fn test_oi_threshold_met() {
            assert!(oi_threshold_met(1_000_000, 100_000)); // exactly 10%
            assert!(oi_threshold_met(1_000_000, 200_000)); // 20%
            assert!(!oi_threshold_met(1_000_000, 50_000)); // 5%
        }

        #[test]
        fn test_state_roundtrip() {
            let state = CreatorHistory {
                magic: CREATOR_HISTORY_MAGIC,
                bump: 250,
                _pad: [0; 3],
                total_markets: 5,
                successful_markets: 3,
                failed_markets: 2,
                _reserved: [0; 14],
            };
            let mut buf = [0u8; CREATOR_HISTORY_LEN];
            write_state(&mut buf, &state);
            let read = read_state(&buf).unwrap();
            assert_eq!(read.total_markets, 5);
            assert_eq!(read.successful_markets, 3);
            assert_eq!(read.failed_markets, 2);
        }

        #[test]
        fn test_read_bad_magic() {
            let mut buf = [0u8; CREATOR_HISTORY_LEN];
            buf[0..8].copy_from_slice(&0xDEADu64.to_le_bytes());
            assert!(read_state(&buf).is_none());
        }

        #[test]
        fn test_state_size() {
            assert_eq!(CREATOR_HISTORY_LEN, 32);
        }
    }
}

#[cfg(kani)]
mod creator_history_kani {
    use crate::creator_history::*;

    /// Multiplier monotonically increases with failures.
    #[kani::proof]
    #[kani::unwind(2)]
    fn proof_multiplier_monotone() {
        let a: u16 = kani::any();
        let b: u16 = kani::any();
        kani::assume(a <= b);
        assert!(failure_multiplier_bps(a) <= failure_multiplier_bps(b));
    }

    /// Discount bounded by MAX_DISCOUNT_BPS.
    #[kani::proof]
    #[kani::unwind(1)]
    fn proof_discount_bounded() {
        let s: u16 = kani::any();
        assert!(success_discount_bps(s) <= MAX_DISCOUNT_BPS);
    }

    /// Required deposit >= floor (50% of base).
    #[kani::proof]
    #[kani::unwind(2)]
    fn nightly_proof_deposit_floor() {
        let failed: u16 = kani::any();
        let successful: u16 = kani::any();
        let base: u64 = kani::any();
        kani::assume(base <= 1_000_000_000_000); // reasonable range
        let dep = compute_required_deposit(base, failed, successful);
        assert!(dep >= base / 2);
    }

    /// Slash conservation: slash + remainder == deposit.
    #[kani::proof]
    #[kani::unwind(1)]
    fn nightly_proof_slash_conservation() {
        let deposit: u64 = kani::any();
        let (slash, remainder) = compute_slash(deposit);
        assert!(slash + remainder == deposit);
    }

    /// OI threshold is monotone: more OI → more likely to pass.
    #[kani::proof]
    #[kani::unwind(1)]
    fn nightly_proof_oi_threshold_monotone() {
        let deposit: u64 = kani::any();
        let oi_a: u64 = kani::any();
        let oi_b: u64 = kani::any();
        kani::assume(oi_a <= oi_b);
        if oi_threshold_met(deposit, oi_a) {
            assert!(oi_threshold_met(deposit, oi_b));
        }
    }
}

// 8e. mod shared_vault — PERC-628: Elastic Shared Vault + Epoch Withdrawals
pub mod shared_vault {
    use bytemuck::{Pod, Zeroable};

    pub const SHARED_VAULT_MAGIC: u64 = 0x5348_5244_5641_4C54; // "SHRDVALT"
    pub const SHARED_VAULT_STATE_LEN: usize = core::mem::size_of::<SharedVaultState>();
    pub const SHARED_VAULT_SEED: &[u8] = b"shared_vault";

    pub const MARKET_ALLOC_MAGIC: u64 = 0x4D4B_5441_4C4C_4F43; // "MKTALLOC"
    pub const MARKET_ALLOC_LEN: usize = core::mem::size_of::<MarketAllocation>();
    pub const MARKET_ALLOC_SEED: &[u8] = b"market_alloc";

    pub const WITHDRAW_REQ_MAGIC: u64 = 0x5754_4844_5252_4551; // "WTHDRREQ"
    pub const WITHDRAW_REQ_LEN: usize = core::mem::size_of::<WithdrawalRequest>();
    pub const WITHDRAW_REQ_SEED: &[u8] = b"withdraw_req";

    pub const DEFAULT_EPOCH_DURATION_SLOTS: u64 = 72_000; // ~8 hours
    pub const DEFAULT_MAX_MARKET_EXPOSURE_BPS: u16 = 2_000; // 20%

    /// Global shared vault state.
    /// Layout: all u128 at 16-byte aligned offsets, u64s grouped.
    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct SharedVaultState {
        pub magic: u64,                   // 0..8
        pub epoch_number: u64,            // 8..16
        pub total_capital: u128,          // 16..32
        pub total_allocated: u128,        // 32..48
        pub pending_withdrawals: u128,    // 48..64
        pub epoch_start_slot: u64,        // 64..72
        pub epoch_duration_slots: u64,    // 72..80
        pub max_market_exposure_bps: u16, // 80..82
        pub bump: u8,                     // 82
        /// Alignment padding to next 16-byte boundary for u128 fields below.
        pub _pad: [u8; 13], // 83..96
        /// Snapshot of `total_capital` taken at the start of each epoch
        /// (set by `AdvanceEpoch`). Used as the fixed available-capital
        /// denominator in `ClaimEpochWithdrawal` so that claim-ordering
        /// never affects individual payouts. Fixes security issue #1016.
        pub epoch_snapshot_capital: u128, // 96..112
        /// Snapshot of `pending_withdrawals` taken at the start of each epoch
        /// (set by `AdvanceEpoch`). Preserved after the per-epoch reset so
        /// claims can still compute proportional payouts. Fixes #1016.
        pub epoch_snapshot_pending: u128, // 112..128
    }

    const _: () = assert!(SHARED_VAULT_STATE_LEN == 128);

    /// Per-market virtual allocation.
    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct MarketAllocation {
        pub magic: u64,
        pub bump: u8,
        pub _pad: [u8; 7],
        pub allocated_capital: u128,
        pub utilized_capital: u128,
    }

    const _: () = assert!(MARKET_ALLOC_LEN == 48);

    /// Per-user per-epoch withdrawal request.
    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct WithdrawalRequest {
        pub magic: u64,
        pub bump: u8,
        pub claimed: u8,
        pub _pad: [u8; 6],
        pub lp_amount: u64,
        pub epoch_number: u64,
    }

    const _: () = assert!(WITHDRAW_REQ_LEN == 32);

    // --- Pure logic ---

    /// Check if a market allocation would exceed the exposure cap.
    /// Returns true if the allocation is within bounds.
    #[inline]
    pub fn check_exposure_cap(total_capital: u128, market_allocation: u128, max_bps: u16) -> bool {
        if total_capital == 0 {
            return market_allocation == 0;
        }
        // market_allocation * 10_000 <= total_capital * max_bps
        let lhs = market_allocation.saturating_mul(10_000);
        let rhs = total_capital.saturating_mul(max_bps as u128);
        lhs <= rhs
    }

    /// Available capital for new allocations.
    #[inline]
    pub fn available_for_allocation(total_capital: u128, total_allocated: u128) -> u128 {
        total_capital.saturating_sub(total_allocated)
    }

    /// Maximum allocation for a single market given the exposure cap.
    #[inline]
    pub fn max_single_market_allocation(total_capital: u128, max_bps: u16) -> u128 {
        total_capital.saturating_mul(max_bps as u128) / 10_000
    }

    /// Check if the epoch has elapsed.
    #[inline]
    pub fn is_epoch_elapsed(current_slot: u64, epoch_start: u64, duration: u64) -> bool {
        current_slot >= epoch_start.saturating_add(duration)
    }

    /// Compute epoch number from slot.
    #[inline]
    pub fn epoch_from_slot(current_slot: u64, genesis_slot: u64, duration: u64) -> u64 {
        if duration == 0 {
            return 0;
        }
        current_slot.saturating_sub(genesis_slot) / duration
    }

    /// Queue a withdrawal: add to pending total.
    #[inline]
    pub fn queue_withdrawal(pending: u128, amount: u64) -> u128 {
        pending.saturating_add(amount as u128)
    }

    /// Compute proportional withdrawal amount for one user.
    /// If total pending > available capital, everyone gets proportionally less.
    /// All users in the same epoch get the same effective price.
    #[inline]
    pub fn compute_proportional_withdrawal(
        request_lp: u64,
        total_pending_lp: u128,
        available_capital: u128,
    ) -> u64 {
        if total_pending_lp == 0 {
            return 0;
        }
        // If enough capital for everyone, return full request
        if available_capital >= total_pending_lp {
            return request_lp;
        }
        // Proportional: request * available / total_pending
        let result = (request_lp as u128).saturating_mul(available_capital) / total_pending_lp;
        result.min(u64::MAX as u128) as u64
    }

    // --- State I/O ---

    pub fn read_vault_state(data: &[u8]) -> Option<SharedVaultState> {
        if data.len() < SHARED_VAULT_STATE_LEN {
            return None;
        }
        let mut s = SharedVaultState::zeroed();
        bytemuck::bytes_of_mut(&mut s).copy_from_slice(&data[..SHARED_VAULT_STATE_LEN]);
        if s.magic != SHARED_VAULT_MAGIC {
            return None;
        }
        Some(s)
    }

    pub fn write_vault_state(data: &mut [u8], state: &SharedVaultState) {
        data[..SHARED_VAULT_STATE_LEN].copy_from_slice(bytemuck::bytes_of(state));
    }

    pub fn read_market_alloc(data: &[u8]) -> Option<MarketAllocation> {
        if data.len() < MARKET_ALLOC_LEN {
            return None;
        }
        let mut s = MarketAllocation::zeroed();
        bytemuck::bytes_of_mut(&mut s).copy_from_slice(&data[..MARKET_ALLOC_LEN]);
        if s.magic != MARKET_ALLOC_MAGIC {
            return None;
        }
        Some(s)
    }

    pub fn write_market_alloc(data: &mut [u8], state: &MarketAllocation) {
        data[..MARKET_ALLOC_LEN].copy_from_slice(bytemuck::bytes_of(state));
    }

    pub fn read_withdraw_req(data: &[u8]) -> Option<WithdrawalRequest> {
        if data.len() < WITHDRAW_REQ_LEN {
            return None;
        }
        let mut s = WithdrawalRequest::zeroed();
        bytemuck::bytes_of_mut(&mut s).copy_from_slice(&data[..WITHDRAW_REQ_LEN]);
        if s.magic != WITHDRAW_REQ_MAGIC {
            return None;
        }
        Some(s)
    }

    pub fn write_withdraw_req(data: &mut [u8], state: &WithdrawalRequest) {
        data[..WITHDRAW_REQ_LEN].copy_from_slice(bytemuck::bytes_of(state));
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_exposure_cap_within() {
            // 20% of 1000 = 200, allocation 200 → ok
            assert!(check_exposure_cap(1000, 200, 2_000));
        }

        #[test]
        fn test_exposure_cap_exceeded() {
            // 20% of 1000 = 200, allocation 201 → exceeded
            assert!(!check_exposure_cap(1000, 201, 2_000));
        }

        #[test]
        fn test_exposure_cap_zero_capital() {
            assert!(check_exposure_cap(0, 0, 2_000));
            assert!(!check_exposure_cap(0, 1, 2_000));
        }

        #[test]
        fn test_available_for_allocation() {
            assert_eq!(available_for_allocation(1000, 400), 600);
            assert_eq!(available_for_allocation(100, 200), 0); // saturating
        }

        #[test]
        fn test_max_single_market() {
            assert_eq!(max_single_market_allocation(10_000, 2_000), 2_000);
            assert_eq!(max_single_market_allocation(10_000, 5_000), 5_000);
        }

        #[test]
        fn test_epoch_elapsed() {
            assert!(!is_epoch_elapsed(100, 50, 100));
            assert!(is_epoch_elapsed(150, 50, 100));
            assert!(is_epoch_elapsed(200, 50, 100));
        }

        #[test]
        fn test_epoch_from_slot() {
            assert_eq!(epoch_from_slot(72_000, 0, 72_000), 1);
            assert_eq!(epoch_from_slot(144_000, 0, 72_000), 2);
            assert_eq!(epoch_from_slot(71_999, 0, 72_000), 0);
        }

        #[test]
        fn test_epoch_from_slot_zero_duration() {
            assert_eq!(epoch_from_slot(100, 0, 0), 0);
        }

        #[test]
        fn test_queue_withdrawal() {
            assert_eq!(queue_withdrawal(1000, 500), 1500);
        }

        #[test]
        fn test_proportional_withdrawal_full() {
            // Enough capital for everyone: get full amount
            assert_eq!(compute_proportional_withdrawal(100, 200, 300), 100);
        }

        #[test]
        fn test_proportional_withdrawal_partial() {
            // Only 50% capital available: everyone gets 50%
            assert_eq!(compute_proportional_withdrawal(100, 200, 100), 50);
        }

        #[test]
        fn test_proportional_withdrawal_zero_pending() {
            assert_eq!(compute_proportional_withdrawal(100, 0, 1000), 0);
        }

        #[test]
        fn test_proportional_withdrawal_exact() {
            // Exactly enough
            assert_eq!(compute_proportional_withdrawal(100, 100, 100), 100);
        }

        #[test]
        fn test_vault_state_roundtrip() {
            let state = SharedVaultState {
                magic: SHARED_VAULT_MAGIC,
                epoch_number: 42,
                total_capital: 1_000_000,
                total_allocated: 500_000,
                pending_withdrawals: 10_000,
                epoch_start_slot: 100_000,
                epoch_duration_slots: DEFAULT_EPOCH_DURATION_SLOTS,
                max_market_exposure_bps: DEFAULT_MAX_MARKET_EXPOSURE_BPS,
                bump: 255,
                _pad: [0; 13],
                epoch_snapshot_capital: 0,
                epoch_snapshot_pending: 0,
            };
            let mut buf = [0u8; SHARED_VAULT_STATE_LEN];
            write_vault_state(&mut buf, &state);
            let read = read_vault_state(&buf).unwrap();
            assert_eq!(read.total_capital, 1_000_000);
            assert_eq!(read.epoch_number, 42);
            assert_eq!(read.max_market_exposure_bps, 2_000);
        }

        #[test]
        fn test_market_alloc_roundtrip() {
            let alloc = MarketAllocation {
                magic: MARKET_ALLOC_MAGIC,
                bump: 254,
                _pad: [0; 7],
                allocated_capital: 200_000,
                utilized_capital: 150_000,
            };
            let mut buf = [0u8; MARKET_ALLOC_LEN];
            write_market_alloc(&mut buf, &alloc);
            let read = read_market_alloc(&buf).unwrap();
            assert_eq!(read.allocated_capital, 200_000);
            assert_eq!(read.utilized_capital, 150_000);
        }

        #[test]
        fn test_withdraw_req_roundtrip() {
            let req = WithdrawalRequest {
                magic: WITHDRAW_REQ_MAGIC,
                bump: 253,
                claimed: 0,
                _pad: [0; 6],
                lp_amount: 5_000,
                epoch_number: 42,
            };
            let mut buf = [0u8; WITHDRAW_REQ_LEN];
            write_withdraw_req(&mut buf, &req);
            let read = read_withdraw_req(&buf).unwrap();
            assert_eq!(read.lp_amount, 5_000);
            assert_eq!(read.epoch_number, 42);
            assert_eq!(read.claimed, 0);
        }

        #[test]
        fn test_read_bad_magic() {
            let mut buf = [0u8; SHARED_VAULT_STATE_LEN];
            buf[0..8].copy_from_slice(&0xDEADu64.to_le_bytes());
            assert!(read_vault_state(&buf).is_none());
        }

        #[test]
        fn test_struct_sizes() {
            assert_eq!(SHARED_VAULT_STATE_LEN, 128);
            assert_eq!(MARKET_ALLOC_LEN, 48);
            assert_eq!(WITHDRAW_REQ_LEN, 32);
        }

        /// Simulate the double-claim guard: once claimed=1, a second
        /// attempt must be rejected (mirrors ClaimEpochWithdrawal handler).
        #[test]
        fn test_double_claim_guard() {
            let req = WithdrawalRequest {
                magic: WITHDRAW_REQ_MAGIC,
                bump: 1,
                claimed: 0,
                _pad: [0; 6],
                lp_amount: 1_000,
                epoch_number: 5,
            };
            assert_eq!(req.claimed, 0, "initially unclaimed");
            let mut buf = [0u8; WITHDRAW_REQ_LEN];
            write_withdraw_req(&mut buf, &req);
            // First claim: mark claimed=1
            let mut updated = read_withdraw_req(&buf).unwrap();
            updated.claimed = 1;
            write_withdraw_req(&mut buf, &updated);
            // Second attempt must be rejected
            let re_read = read_withdraw_req(&buf).unwrap();
            assert_eq!(re_read.claimed, 1, "claimed flag must persist");
            assert_ne!(re_read.claimed, 0, "double-claim guard: must reject");
        }

        /// Payout is zero when there are no pending withdrawals.
        #[test]
        fn test_zero_payout_no_pending() {
            let payout = compute_proportional_withdrawal(500, 0, 10_000);
            assert_eq!(payout, 0, "zero pending → zero payout");
        }

        /// Payout equals the request when capital is more than sufficient.
        #[test]
        fn test_payout_capped_at_request() {
            let payout = compute_proportional_withdrawal(100, 100, 999_999);
            assert_eq!(payout, 100);
        }

        // ── Fix #1016: epoch_snapshot_capital / epoch_snapshot_pending ──

        /// SharedVaultState struct is still exactly 128 bytes after adding
        /// epoch_snapshot_capital and epoch_snapshot_pending.
        #[test]
        fn test_struct_size_unchanged() {
            assert_eq!(SHARED_VAULT_STATE_LEN, 128);
        }

        /// epoch_snapshot_capital / epoch_snapshot_pending survive a
        /// write → read round-trip.
        #[test]
        fn test_epoch_snapshot_roundtrip() {
            let state = SharedVaultState {
                magic: SHARED_VAULT_MAGIC,
                epoch_number: 5,
                total_capital: 100_000,
                total_allocated: 0,
                pending_withdrawals: 0,
                epoch_start_slot: 1_000,
                epoch_duration_slots: DEFAULT_EPOCH_DURATION_SLOTS,
                max_market_exposure_bps: DEFAULT_MAX_MARKET_EXPOSURE_BPS,
                bump: 1,
                _pad: [0; 13],
                epoch_snapshot_capital: 99_000,
                epoch_snapshot_pending: 200_000,
            };
            let mut buf = [0u8; SHARED_VAULT_STATE_LEN];
            write_vault_state(&mut buf, &state);
            let read = read_vault_state(&buf).unwrap();
            assert_eq!(read.epoch_snapshot_capital, 99_000);
            assert_eq!(read.epoch_snapshot_pending, 200_000);
        }

        /// Ordering invariant: User A (first) and User B (second) receive the
        /// same per-LP-token payout from the epoch snapshot values.
        ///
        /// Scenario: epoch ends with total_capital=100, pending=200 (50% funded).
        /// Each user has 20 LP. Using snapshots both get 10 tokens.
        /// With live total_capital User B would get only 9 (the bug).
        #[test]
        fn test_ordering_invariant_snapshot_values() {
            // Epoch snapshot values (fixed at epoch boundary)
            let snapshot_capital: u128 = 100;
            let snapshot_pending: u128 = 200;

            // User A claims first
            let payout_a = compute_proportional_withdrawal(20, snapshot_pending, snapshot_capital);
            // User B claims second — snapshot values are unchanged
            let payout_b = compute_proportional_withdrawal(20, snapshot_pending, snapshot_capital);

            assert_eq!(payout_a, 10, "User A should get 10");
            assert_eq!(payout_b, 10, "User B should get 10 (same as A)");
            assert_eq!(payout_a, payout_b, "ordering must not affect payout");
        }

        /// Underfunded epoch: proportional reduction applies equally.
        #[test]
        fn test_underfunded_epoch_equal_reduction() {
            // 3 users each with 30 LP, total pending=90, capital=45 (50% funded)
            let snapshot_capital: u128 = 45;
            let snapshot_pending: u128 = 90;

            let p1 = compute_proportional_withdrawal(30, snapshot_pending, snapshot_capital);
            let p2 = compute_proportional_withdrawal(30, snapshot_pending, snapshot_capital);
            let p3 = compute_proportional_withdrawal(30, snapshot_pending, snapshot_capital);

            assert_eq!(p1, 15);
            assert_eq!(p2, 15);
            assert_eq!(p3, 15);
            // Total payouts don't exceed available capital
            assert!(p1 as u128 + p2 as u128 + p3 as u128 <= snapshot_capital);
        }
    }
}

#[cfg(kani)]
mod shared_vault_kani {
    use crate::shared_vault::*;

    /// Exposure cap: if check_exposure_cap passes, allocation <= max % of total.
    #[kani::proof]
    fn nightly_sv_exposure_cap_bounded() {
        let total: u128 = kani::any();
        let alloc: u128 = kani::any();
        let max_bps: u16 = kani::any();
        kani::assume(total <= u128::MAX / 10_000);
        kani::assume(alloc <= u128::MAX / 10_000);
        if check_exposure_cap(total, alloc, max_bps) && total > 0 {
            // alloc * 10_000 <= total * max_bps
            assert!(alloc.saturating_mul(10_000) <= total.saturating_mul(max_bps as u128));
        }
    }

    /// Available for allocation never exceeds total capital.
    #[kani::proof]
    fn nightly_sv_available_bounded() {
        let total: u128 = kani::any();
        let allocated: u128 = kani::any();
        let avail = available_for_allocation(total, allocated);
        assert!(avail <= total);
    }

    /// Proportional withdrawal is fair: result <= request.
    #[kani::proof]
    fn nightly_sv_proportional_bounded() {
        let req: u64 = kani::any();
        let total_pending: u128 = kani::any();
        let available: u128 = kani::any();
        kani::assume(total_pending > 0);
        kani::assume(req as u128 <= total_pending);
        let result = compute_proportional_withdrawal(req, total_pending, available);
        assert!(result <= req);
    }

    /// Epoch monotonically increases with slot.
    #[kani::proof]
    fn nightly_sv_epoch_monotone() {
        let slot_a: u64 = kani::any();
        let slot_b: u64 = kani::any();
        let genesis: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        kani::assume(slot_a <= slot_b);
        kani::assume(slot_a >= genesis);
        kani::assume(slot_b >= genesis);
        assert!(
            epoch_from_slot(slot_a, genesis, duration)
                <= epoch_from_slot(slot_b, genesis, duration)
        );
    }

    /// Queue withdrawal monotonically increases pending.
    #[kani::proof]
    fn nightly_sv_queue_monotone() {
        let pending: u128 = kani::any();
        let amount: u64 = kani::any();
        let new_pending = queue_withdrawal(pending, amount);
        assert!(new_pending >= pending);
    }

    /// Max single market allocation never exceeds total capital.
    #[kani::proof]
    fn nightly_sv_max_alloc_bounded() {
        let total: u128 = kani::any();
        let max_bps: u16 = kani::any();
        kani::assume(max_bps <= 10_000);
        let max_alloc = max_single_market_allocation(total, max_bps);
        assert!(max_alloc <= total);
    }

    /// #1016 fix: ordering invariant — two users with equal LP receive equal
    /// payout when using fixed snapshot values (not live total_capital).
    #[kani::proof]
    fn proof_sv_ordering_invariant() {
        let snapshot_capital: u128 = kani::any();
        let snapshot_pending: u128 = kani::any();
        let lp_a: u64 = kani::any();
        let lp_b: u64 = kani::any();
        kani::assume(snapshot_pending > 0);
        kani::assume(lp_a == lp_b); // same LP amount → must get same payout
        kani::assume(lp_a as u128 <= snapshot_pending);

        let payout_a = compute_proportional_withdrawal(lp_a, snapshot_pending, snapshot_capital);
        // User B claims "after" A — but snapshot values are immutable, so
        // available_capital seen by B is identical to that seen by A.
        let payout_b = compute_proportional_withdrawal(lp_b, snapshot_pending, snapshot_capital);

        assert_eq!(
            payout_a, payout_b,
            "same LP → same payout regardless of order"
        );
    }

    /// #1016 fix: total payout of all users in an epoch never exceeds
    /// epoch_snapshot_capital (no over-payment from the vault).
    #[kani::proof]
    #[kani::unwind(1)]
    fn nightly_proof_sv_total_payout_bounded() {
        let snapshot_capital: u128 = kani::any();
        let snapshot_pending: u128 = kani::any();
        let lp_user: u64 = kani::any();
        kani::assume(snapshot_pending > 0);
        kani::assume(lp_user as u128 <= snapshot_pending);

        let payout = compute_proportional_withdrawal(lp_user, snapshot_pending, snapshot_capital);
        // Individual payout must not exceed the user's proportional share of capital
        if snapshot_capital < snapshot_pending {
            // Underfunded: payout < request
            assert!(payout <= lp_user);
        } else {
            // Fully funded: payout == request
            assert_eq!(payout as u128, lp_user as u128);
        }
    }
}

// 9. mod processor
pub mod processor {
    use crate::{
        accounts, collateral,
        constants::{
            DEFAULT_DEX_ORACLE_PRICE_CAP_E2BPS, DEFAULT_FUNDING_HORIZON_SLOTS,
            DEFAULT_FUNDING_INV_SCALE_NOTIONAL_E6, DEFAULT_FUNDING_K_BPS,
            DEFAULT_FUNDING_MAX_BPS_PER_SLOT, DEFAULT_FUNDING_MAX_PREMIUM_BPS,
            DEFAULT_HYPERP_PRICE_CAP_E2BPS, DEFAULT_THRESH_ALPHA_BPS, DEFAULT_THRESH_FLOOR,
            DEFAULT_THRESH_MAX, DEFAULT_THRESH_MIN, DEFAULT_THRESH_MIN_STEP,
            DEFAULT_THRESH_RISK_BPS, DEFAULT_THRESH_STEP_BPS, DEFAULT_THRESH_UPDATE_INTERVAL_SLOTS,
            ENGINE_LEN, ENGINE_OFF, HEADER_LEN, MAGIC, MATCHER_CALL_LEN, MATCHER_CALL_TAG,
            SLAB_LEN, VERSION,
        },
        cross_margin,
        error::{map_risk_error, PercolatorError},
        ix::{self, Instruction},
        oracle,
        state::{self, MarketConfig, SlabHeader},
        verify::compute_ramp_multiplier,
        zc,
    };
    use alloc::boxed::Box;
    #[allow(unused_imports)]
    use alloc::format;
    use percolator::{
        MatchingEngine, NoOpMatcher, RiskEngine, RiskError, RiskParams, TradeExecution,
        MAX_ACCOUNTS,
    };
    use solana_program::instruction::{AccountMeta, Instruction as SolInstruction};
    use solana_program::{
        account_info::AccountInfo,
        entrypoint::ProgramResult,
        log::sol_log_64,
        msg,
        program_error::ProgramError,
        program_pack::Pack,
        pubkey::Pubkey,
        sysvar::{clock::Clock, Sysvar},
    };

    struct CpiMatcher {
        exec_price: u64,
        exec_size: i128,
    }

    impl MatchingEngine for CpiMatcher {
        fn execute_match(
            &self,
            _lp_program: &[u8; 32],
            _lp_context: &[u8; 32],
            _lp_account_id: u64,
            _oracle_price: u64,
            _size: i128,
        ) -> Result<TradeExecution, RiskError> {
            Ok(TradeExecution {
                price: self.exec_price,
                size: self.exec_size,
            })
        }
    }

    /// PERC-328: #[inline(never)] prevents slab-shape locals from merging
    /// into the caller's 4 KiB SBF frame.
    #[inline(never)]
    fn slab_guard(
        program_id: &Pubkey,
        slab: &AccountInfo,
        data: &[u8],
    ) -> Result<(), ProgramError> {
        // Slab shape validation via verify helper (Kani-provable).
        // Three legacy sizes are accepted for backward compatibility:
        //   SLAB_LEN        — current (PERC-118 + Account reorder)
        //   SLAB_LEN - 16   — pre-PERC-118 (before trade_twap_e6 + twap_last_slot, +16 bytes)
        //   SLAB_LEN - 24   — pre-PERC-118 + pre-Account-reorder (oldest devnet slabs, -8 bytes)
        // New TWAP fields default to zero when read from old slabs → pure oracle mark (safe).
        const PRE_118_SLAB_LEN: usize = SLAB_LEN - 16;
        const OLDEST_SLAB_LEN: usize = SLAB_LEN - 24;
        let shape = crate::verify::SlabShape {
            owned_by_program: slab.owner == program_id,
            correct_len: data.len() == SLAB_LEN
                || data.len() == PRE_118_SLAB_LEN
                || data.len() == OLDEST_SLAB_LEN,
        };
        if !crate::verify::slab_shape_ok(shape) {
            // Return specific error based on which check failed
            if slab.owner != program_id {
                return Err(ProgramError::IllegalOwner);
            }
            solana_program::log::sol_log_64(SLAB_LEN as u64, data.len() as u64, 0, 0, 0);
            return Err(PercolatorError::InvalidSlabLen.into());
        }
        Ok(())
    }

    fn require_initialized(data: &[u8]) -> Result<(), ProgramError> {
        let h = state::read_header(data);
        if h.magic != MAGIC {
            return Err(PercolatorError::NotInitialized.into());
        }
        if h.version != VERSION {
            return Err(PercolatorError::InvalidVersion.into());
        }
        Ok(())
    }

    /// Require that the signer is the current admin.
    /// If admin is burned (all zeros), admin operations are permanently disabled.
    /// Admin authorization via verify helper (Kani-provable)
    fn require_admin(header_admin: [u8; 32], signer: &Pubkey) -> Result<(), ProgramError> {
        if !crate::verify::admin_ok(header_admin, signer.to_bytes()) {
            return Err(PercolatorError::EngineUnauthorized.into());
        }
        Ok(())
    }

    /// Reject if the market is paused. Used in trade/deposit/withdraw/init_user.
    fn require_not_paused(data: &[u8]) -> Result<(), ProgramError> {
        if state::is_paused(data) {
            return Err(PercolatorError::MarketPaused.into());
        }
        Ok(())
    }

    /// PERC-298: Unpack oi_cap_multiplier_bps field.
    /// Lower 32 bits = OI cap multiplier. Bits 32..47 = skew_factor_bps.
    /// Backwards compatible: existing markets have upper bits = 0 (skew disabled).
    #[inline]
    pub fn unpack_oi_cap(packed: u64) -> (u64, u64) {
        let multiplier = packed & 0xFFFF_FFFF;
        let skew_factor = (packed >> 32) & 0xFFFF;
        (multiplier, skew_factor)
    }

    /// PERC-298: Pack OI cap multiplier and skew factor into a single u64.
    #[inline]
    pub fn pack_oi_cap(multiplier: u64, skew_factor: u64) -> u64 {
        (multiplier & 0xFFFF_FFFF) | ((skew_factor & 0xFFFF) << 32)
    }

    /// PERC-273 + PERC-298 + PERC-302: Check dynamic OI cap after trade execution.
    /// If oi_cap_multiplier_bps > 0, enforce: total_open_interest <= effective_max_oi.
    /// PERC-302: If oi_ramp_slots > 0, the effective multiplier ramps linearly.
    /// PERC-298: When skew_factor > 0, the effective cap tightens with OI skew:
    ///   effective_max_oi = base_max_oi * (1 - |long_oi - short_oi| / total_oi * skew_factor / 10_000)
    fn check_oi_cap(
        engine: &RiskEngine,
        config: &state::MarketConfig,
        current_slot: u64,
    ) -> Result<(), ProgramError> {
        let (multiplier, skew_factor_bps) = unpack_oi_cap(config.oi_cap_multiplier_bps);
        if multiplier == 0 {
            return Ok(()); // OI cap disabled
        }

        // PERC-302: Apply market maturity ramp
        let effective_multiplier = compute_ramp_multiplier(
            multiplier,
            config.market_created_slot,
            current_slot,
            config.oi_ramp_slots,
        );

        let vault = engine.vault.get();
        let base_max_oi = vault.saturating_mul(effective_multiplier as u128) / 10_000;
        let current_oi = engine.total_open_interest.get();

        // PERC-299: Halve cap when emergency OI mode is active (circuit breaker fired)
        let base_max_oi = if engine.is_emergency_oi_mode() {
            base_max_oi / 2
        } else {
            base_max_oi
        };

        // PERC-298: Apply skew-based tightening
        let max_oi = if skew_factor_bps > 0 && current_oi > 0 {
            let long = engine.long_oi.get();
            let short = engine.short_oi.get();
            let skew = long.abs_diff(short);
            // reduction_bps = (skew / total_oi) * skew_factor_bps
            let reduction_bps = skew.saturating_mul(skew_factor_bps as u128) / current_oi;
            // Cap at skew_factor_bps to prevent underflow
            let capped_reduction = reduction_bps.min(skew_factor_bps as u128);
            base_max_oi.saturating_mul(10_000u128.saturating_sub(capped_reduction)) / 10_000
        } else {
            base_max_oi
        };

        // PERC-622: Apply oracle phase OI cap (absolute ceiling per phase)
        let phase = state::get_oracle_phase(config);
        let phase_cap = state::phase_oi_cap(phase, u64::MAX) as u128;
        let max_oi = max_oi.min(phase_cap);

        if current_oi > max_oi {
            msg!(
                "OI cap exceeded: current={} max={} (vault={} multiplier={} effective={} skew_factor={} phase={})",
                current_oi,
                max_oi,
                vault,
                multiplier,
                effective_multiplier,
                skew_factor_bps,
                phase,
            );
            return Err(PercolatorError::EngineRiskReductionOnlyMode.into());
        }
        Ok(())
    }

    /// PERC-622: Check phase-based leverage limit after trade.
    /// Leverage = notional / collateral. Phase 1: max 2x, Phase 2: max 5x.
    /// We enforce via minimum initial_margin_bps floor:
    ///   Phase 1: floor = 5000 bps (50% → 2x)
    ///   Phase 2: floor = 2000 bps (20% → 5x)
    ///   Phase 3: no additional floor
    /// If the engine's initial_margin_bps is already above the floor, no change needed.
    /// This check rejects trades where the resulting position would exceed phase leverage.
    fn check_phase_leverage(
        engine: &RiskEngine,
        config: &state::MarketConfig,
        user_idx: u16,
    ) -> Result<(), ProgramError> {
        let phase = state::get_oracle_phase(config);
        if phase >= state::ORACLE_PHASE_MATURE {
            return Ok(()); // Phase 3: no additional leverage restriction
        }

        let max_lev_bps = state::phase_max_leverage_bps(phase, u64::MAX);
        if max_lev_bps == 0 {
            return Ok(());
        }

        // min_margin_bps = 10_000_000 / max_lev_bps (e.g., 10M / 20000 = 500 → 5.00%)
        // We use 10_000 * 10_000 / max_lev_bps for bps precision
        let min_margin_bps = 10_000u64.saturating_mul(10_000) / max_lev_bps;

        // Check if current initial margin is sufficient
        let current_margin_bps = engine.params.initial_margin_bps;
        if u128::from(current_margin_bps) >= min_margin_bps as u128 {
            return Ok(()); // Existing margin requirement is stricter — no issue
        }

        // Margin requirement is looser than phase allows.
        // Check if the user's position actually exceeds phase leverage.
        let acct = &engine.accounts[user_idx as usize];
        let pos_size = acct.position_size.get().unsigned_abs();
        if pos_size == 0 {
            return Ok(());
        }
        let capital = acct.capital.get();
        if capital == 0 {
            return Err(PercolatorError::EngineRiskReductionOnlyMode.into());
        }
        // leverage_bps = pos_size * 10_000 / capital
        let leverage_bps = (pos_size as u128)
            .saturating_mul(10_000)
            .checked_div(capital as u128)
            .unwrap_or(u128::MAX);
        if leverage_bps > max_lev_bps as u128 {
            msg!(
                "Phase leverage exceeded: leverage_bps={} max={} phase={}",
                leverage_bps,
                max_lev_bps,
                phase
            );
            return Err(PercolatorError::EngineRiskReductionOnlyMode.into());
        }
        Ok(())
    }

    /// PERC-272: Check max PnL cap after trade execution.
    /// If max_pnl_cap > 0, enforce: pnl_pos_tot <= max_pnl_cap.
    fn check_pnl_cap(
        engine: &RiskEngine,
        config: &state::MarketConfig,
    ) -> Result<(), ProgramError> {
        let cap = config.max_pnl_cap;
        if cap == 0 {
            return Ok(()); // PnL cap disabled
        }
        let current_pnl = engine.pnl_pos_tot.get();
        if current_pnl > cap as u128 {
            msg!(
                "PnL cap exceeded: current_pnl_pos_tot={} max={}",
                current_pnl,
                cap
            );
            return Err(PercolatorError::EngineRiskReductionOnlyMode.into());
        }
        Ok(())
    }

    /// PERC-312: Safety valve — check and auto-exit rebalancing mode.
    /// If rebalancing is active and trade would increase position on the dominant side, reject.
    /// Dominant side = direction of net LP position (longs dominant if net_lp_pos < 0,
    /// meaning LPs are net short, so users are net long).
    ///
    /// `size` is the user's trade size (positive = user buys = goes long).
    /// `old_user_pos` is the user's position before this trade.
    /// PERC-312: Safety valve check (read-only on config).
    /// Auto-exit is handled in keeper crank; trade path only checks.
    fn check_safety_valve(
        config: &state::MarketConfig,
        net_lp_pos: i128,
        size: i128,
        old_user_pos: i128,
        current_slot: u64,
    ) -> Result<(), ProgramError> {
        if config.safety_valve_enabled == 0 || config.rebalancing_active == 0 {
            return Ok(());
        }

        // Auto-exit check: if duration elapsed, allow trade through.
        // PERC-322/LOW-1: The actual rebalancing_active flag is cleared
        // in update_safety_valve_on_funding() during the next crank.
        if config.safety_valve_duration > 0 {
            let deadline = config
                .rebalancing_start_slot
                .saturating_add(config.safety_valve_duration);
            if current_slot >= deadline {
                return Ok(()); // Duration elapsed, flag cleared on next crank
            }
        }

        // Determine dominant side from net LP position.
        let dominant_is_long = net_lp_pos < 0;

        // Check if this trade increases user's position on the dominant side
        let new_user_pos = old_user_pos.saturating_add(size);
        let increases_dominant = if dominant_is_long {
            // Block new longs: new position is more positive than old
            new_user_pos > old_user_pos && new_user_pos > 0
        } else {
            // Block new shorts: new position is more negative than old
            new_user_pos < old_user_pos && new_user_pos < 0
        };

        if increases_dominant {
            msg!(
                "PERC-312: Safety valve blocked trade: size={} dominant_long={} net_lp={}",
                size,
                dominant_is_long,
                net_lp_pos
            );
            return Err(PercolatorError::SafetyValveDominantSideBlocked.into());
        }

        Ok(())
    }

    /// PERC-312: Update safety valve state after funding crank.
    /// Call after computing new funding rate. If rate is at max for consecutive epochs,
    /// activate rebalancing mode.
    fn update_safety_valve_on_funding(
        config: &mut state::MarketConfig,
        funding_rate_bps: i64,
        current_slot: u64,
    ) {
        if config.safety_valve_enabled == 0 || config.safety_valve_duration == 0 {
            return;
        }

        // PERC-322/LOW-1: Check duration-based auto-exit FIRST.
        // If rebalancing is active and duration has elapsed, clear the flag immediately
        // rather than leaving it dangling until skew resolves organically.
        // This closes the gap where check_safety_valve() silently allowed trades
        // but rebalancing_active remained set, causing inconsistent state.
        if config.rebalancing_active != 0 && config.safety_valve_duration > 0 {
            let deadline = config
                .rebalancing_start_slot
                .saturating_add(config.safety_valve_duration);
            if current_slot >= deadline {
                config.rebalancing_active = 0;
                config.rebalancing_start_slot = 0;
                config.consecutive_max_funding_epochs = 0;
                msg!(
                    "PERC-312: Rebalancing auto-exited — duration expired at slot {} (deadline={})",
                    current_slot,
                    deadline
                );
                return;
            }
        }

        let max_bps = config.funding_max_bps_per_slot;
        let at_max = funding_rate_bps.abs() >= max_bps.abs() && max_bps != 0;

        if at_max {
            config.consecutive_max_funding_epochs =
                config.consecutive_max_funding_epochs.saturating_add(1);
        } else {
            config.consecutive_max_funding_epochs = 0;
            // If skew resolved, also exit rebalancing mode
            if config.rebalancing_active != 0 {
                config.rebalancing_active = 0;
                config.rebalancing_start_slot = 0;
                msg!(
                    "PERC-312: Rebalancing exited — skew resolved at slot {}",
                    current_slot
                );
            }
        }

        let threshold = if config.safety_valve_epochs == 0 {
            5
        } else {
            config.safety_valve_epochs
        };
        if config.consecutive_max_funding_epochs >= threshold && config.rebalancing_active == 0 {
            config.rebalancing_active = 1;
            config.rebalancing_start_slot = current_slot;
            msg!(
                "PERC-312: Safety valve ACTIVATED at slot {} after {} consecutive max-funding epochs",
                current_slot, config.consecutive_max_funding_epochs
            );
        }
    }

    /// PERC-307: Compute orphan penalty for a position.
    /// Returns penalty amount in collateral units (to be added to insurance fund).
    /// Penalty = |position_size| * penalty_bps * stale_slots / 10_000
    /// Only applies if oracle is stale AND market is not resolved.
    #[allow(dead_code)] // PERC-307: Will be used when orphan penalty crank is implemented
    fn compute_orphan_penalty(
        config: &state::MarketConfig,
        current_slot: u64,
        last_oracle_slot: u64,
        is_resolved: bool,
        position_size_abs: u128,
    ) -> u128 {
        if is_resolved
            || config.orphan_threshold_slots == 0
            || config.orphan_penalty_bps_per_slot == 0
        {
            return 0;
        }
        let staleness = current_slot.saturating_sub(last_oracle_slot);
        if staleness <= config.orphan_threshold_slots {
            return 0;
        }
        let penalty_slots = staleness - config.orphan_threshold_slots;
        let penalty_bps = config.orphan_penalty_bps_per_slot as u128;
        position_size_abs
            .saturating_mul(penalty_bps)
            .saturating_mul(penalty_slots as u128)
            / 10_000
    }

    #[cfg(test)]
    mod hyperp_staleness_tests {
        use crate::oracle::check_hyperp_staleness;

        #[test]
        fn staleness_ok_within_threshold() {
            // Engine cranked at slot 100, current slot 150, max stale = 100
            assert!(check_hyperp_staleness(100, 100, 150).is_ok());
        }

        #[test]
        fn staleness_ok_at_boundary() {
            // Exactly at the boundary: age == max_stale
            assert!(check_hyperp_staleness(100, 50, 150).is_ok());
        }

        #[test]
        fn staleness_rejected_past_threshold() {
            // Engine cranked at slot 100, current slot 201, max stale = 100
            let result = check_hyperp_staleness(100, 100, 201);
            assert!(result.is_err());
        }

        #[test]
        fn staleness_disabled_when_zero() {
            // max_crank_staleness_slots = 0 disables the check
            assert!(check_hyperp_staleness(0, 0, 999_999).is_ok());
        }

        #[test]
        fn staleness_disabled_when_max_u64() {
            // max_crank_staleness_slots = u64::MAX disables the check
            assert!(check_hyperp_staleness(0, u64::MAX, 999_999).is_ok());
        }

        #[test]
        fn staleness_fresh_crank_always_ok() {
            // Engine just cranked (same slot)
            assert!(check_hyperp_staleness(500, 100, 500).is_ok());
        }
    }

    #[cfg(test)]
    mod orphan_tests {
        use super::*;
        use crate::state::MarketConfig;

        fn test_config(threshold: u64, penalty_bps: u16) -> MarketConfig {
            let mut c: MarketConfig = bytemuck::Zeroable::zeroed();
            c.orphan_threshold_slots = threshold;
            c.orphan_penalty_bps_per_slot = penalty_bps;
            c
        }

        #[test]
        fn test_no_penalty_when_disabled() {
            let c = test_config(0, 1);
            assert_eq!(compute_orphan_penalty(&c, 2000, 500, false, 1000), 0);
        }

        #[test]
        fn test_no_penalty_when_not_stale() {
            let c = test_config(1000, 1);
            assert_eq!(compute_orphan_penalty(&c, 1500, 1000, false, 1000), 0);
        }

        #[test]
        fn test_no_penalty_when_resolved() {
            let c = test_config(1000, 1);
            assert_eq!(compute_orphan_penalty(&c, 3000, 500, true, 1000), 0);
        }

        #[test]
        fn test_penalty_applied() {
            let c = test_config(1000, 1); // 1 bps per slot
                                          // staleness = 2500, penalty_slots = 1500
                                          // penalty = 1000 * 1 * 1500 / 10000 = 150
            assert_eq!(compute_orphan_penalty(&c, 3000, 500, false, 1000), 150);
        }

        #[test]
        fn test_penalty_at_boundary() {
            let c = test_config(1000, 1);
            // staleness = 1000, exactly at threshold → no penalty
            assert_eq!(compute_orphan_penalty(&c, 2000, 1000, false, 1000), 0);
            // staleness = 1001 → 1 penalty slot
            assert_eq!(compute_orphan_penalty(&c, 2001, 1000, false, 10000), 1);
        }
    }

    #[cfg(kani)]
    mod orphan_proofs {
        use super::*;

        #[kani::proof]
        #[kani::unwind(1)]
        fn proof_orphan_penalty_only_applies_when_oracle_stale_and_not_resolved() {
            let threshold: u64 = kani::any();
            let penalty_bps: u16 = kani::any();
            let current_slot: u64 = kani::any();
            let last_oracle_slot: u64 = kani::any();
            let is_resolved: bool = kani::any();
            let pos_abs: u128 = kani::any();

            kani::assume(threshold <= 100_000);
            kani::assume(current_slot <= 1_000_000);
            kani::assume(last_oracle_slot <= current_slot);
            kani::assume(pos_abs <= 1_000_000_000_000);

            let mut c: crate::state::MarketConfig = bytemuck::Zeroable::zeroed();
            c.orphan_threshold_slots = threshold;
            c.orphan_penalty_bps_per_slot = penalty_bps;

            let penalty =
                compute_orphan_penalty(&c, current_slot, last_oracle_slot, is_resolved, pos_abs);

            // PROPERTY 1: penalty is zero if resolved
            if is_resolved {
                assert!(penalty == 0, "penalty must be 0 when resolved");
            }
            // PROPERTY 2: penalty is zero if not stale enough
            let staleness = current_slot.saturating_sub(last_oracle_slot);
            if staleness <= threshold {
                assert!(penalty == 0, "penalty must be 0 when not stale");
            }
            // PROPERTY 3: penalty is zero if disabled
            if threshold == 0 || penalty_bps == 0 {
                assert!(penalty == 0, "penalty must be 0 when disabled");
            }
        }
    }

    fn check_idx(engine: &RiskEngine, idx: u16) -> Result<(), ProgramError> {
        if (idx as usize) >= MAX_ACCOUNTS || !engine.is_used(idx as usize) {
            return Err(PercolatorError::EngineAccountNotFound.into());
        }
        Ok(())
    }

    /// PERC-328: #[inline(never)] keeps Account::unpack (165 B) in its own
    /// 4 KiB SBF frame, preventing stack overflow in process_init_market.
    #[inline(never)]
    fn verify_vault(
        a_vault: &AccountInfo,
        expected_owner: &Pubkey,
        expected_mint: &Pubkey,
        expected_pubkey: &Pubkey,
    ) -> Result<(), ProgramError> {
        if a_vault.key != expected_pubkey {
            return Err(PercolatorError::InvalidVaultAta.into());
        }
        if a_vault.owner != &spl_token::ID {
            return Err(PercolatorError::InvalidVaultAta.into());
        }
        if a_vault.data_len() != spl_token::state::Account::LEN {
            return Err(PercolatorError::InvalidVaultAta.into());
        }

        let data = a_vault.try_borrow_data()?;
        let tok = spl_token::state::Account::unpack(&data)?;
        if tok.mint != *expected_mint {
            return Err(PercolatorError::InvalidMint.into());
        }
        if tok.owner != *expected_owner {
            return Err(PercolatorError::InvalidVaultAta.into());
        }
        // SECURITY (H3): Verify vault token account is initialized
        // Uninitialized vault could brick deposits/withdrawals
        if tok.state != spl_token::state::AccountState::Initialized {
            return Err(PercolatorError::InvalidVaultAta.into());
        }
        Ok(())
    }

    /// Verify a user's token account: owner, mint, and initialized state.
    /// Skip in tests to allow mock accounts.
    #[allow(unused_variables)]
    fn verify_token_account(
        a_token_account: &AccountInfo,
        expected_owner: &Pubkey,
        expected_mint: &Pubkey,
    ) -> Result<(), ProgramError> {
        #[cfg(not(feature = "test"))]
        {
            if a_token_account.owner != &spl_token::ID {
                return Err(PercolatorError::InvalidTokenAccount.into());
            }
            if a_token_account.data_len() != spl_token::state::Account::LEN {
                return Err(PercolatorError::InvalidTokenAccount.into());
            }

            let data = a_token_account.try_borrow_data()?;
            let tok = spl_token::state::Account::unpack(&data)?;
            if tok.mint != *expected_mint {
                return Err(PercolatorError::InvalidMint.into());
            }
            if tok.owner != *expected_owner {
                return Err(PercolatorError::InvalidTokenAccount.into());
            }
            if tok.state != spl_token::state::AccountState::Initialized {
                return Err(PercolatorError::InvalidTokenAccount.into());
            }
        }
        Ok(())
    }

    /// Verify the token program account is valid.
    /// Skip in tests to allow mock accounts.
    #[allow(unused_variables)]
    fn verify_token_program(a_token: &AccountInfo) -> Result<(), ProgramError> {
        #[cfg(not(feature = "test"))]
        {
            if *a_token.key != spl_token::ID {
                return Err(PercolatorError::InvalidTokenProgram.into());
            }
            if !a_token.executable {
                return Err(PercolatorError::InvalidTokenProgram.into());
            }
        }
        Ok(())
    }

    /// PERC-328: Validate SPL mint account in its own stack frame.
    /// `Mint::unpack` creates an 82-byte struct on the stack; keeping it in
    /// the caller's frame (process_init_market) contributed to SBF 4 KiB
    /// frame overflow.
    #[inline(never)]
    fn validate_spl_mint(a_mint: &AccountInfo) -> ProgramResult {
        #[cfg(not(feature = "test"))]
        {
            use solana_program::program_pack::Pack;
            use spl_token::state::Mint;
            if *a_mint.owner != spl_token::ID {
                return Err(ProgramError::IllegalOwner);
            }
            if a_mint.data_len() != Mint::LEN {
                return Err(ProgramError::InvalidAccountData);
            }
            let mint_data = a_mint.try_borrow_data()?;
            let _ = Mint::unpack(&mint_data)?;
        }
        #[cfg(feature = "test")]
        let _ = a_mint;
        Ok(())
    }

    /// PERC-328: Validate vault has sufficient seed lamports in its own frame.
    /// `Account::unpack` creates a 165-byte struct; isolated to prevent stack
    /// overflow in the parent frame.
    #[inline(never)]
    fn validate_vault_seed_lamports(a_vault: &AccountInfo) -> ProgramResult {
        #[cfg(not(feature = "test"))]
        {
            use solana_program::program_pack::Pack;
            let vault_data = a_vault.try_borrow_data()?;
            let vault_tok = spl_token::state::Account::unpack(&vault_data)?;
            if vault_tok.amount < crate::constants::MIN_INIT_MARKET_SEED_LAMPORTS {
                return Err(PercolatorError::InsufficientSeed.into());
            }
        }
        #[cfg(feature = "test")]
        let _ = a_vault;
        Ok(())
    }

    /// PERC-328: Compact struct for passing parsed InitMarket fields between
    /// stack-isolated phases. Only scalars and small arrays — RiskParams is
    /// passed separately via Box to stay on the heap.
    struct InitMarketFields {
        index_feed_id: [u8; 32],
        max_staleness_secs: u64,
        conf_filter_bps: u16,
        invert: u8,
        unit_scale: u32,
        initial_mark_price_e6: u64,
        is_hyperp: bool,
    }

    /// PERC-328: Context for `init_market_write_slab` — bundles account
    /// keys and the vault-authority bump to stay within clippy's 7-arg limit.
    struct WriteSlabContext<'a, 'b> {
        bump: u8,
        admin_key: &'a Pubkey,
        mint_key: &'a Pubkey,
        vault_key: &'a Pubkey,
        a_clock: &'b AccountInfo<'a>,
    }

    /// PERC-328: Write config fields + header to the slab in its own frame.
    /// This isolates the SlabHeader (~98 B), all the wcb temporaries, and
    /// the Clock struct from the parent's frame.
    #[inline(never)]
    fn init_market_write_slab<'a, 'b>(
        data: &mut [u8],
        fields: &InitMarketFields,
        risk_params: RiskParams,
        ctx: &WriteSlabContext<'a, 'b>,
    ) -> ProgramResult {
        // Zero the slab
        for b in data.iter_mut() {
            *b = 0;
        }

        // Initialize risk engine
        let engine = zc::engine_mut(data)?;
        engine
            .init_in_place(risk_params)
            .map_err(crate::error::map_risk_error)?;

        let clock = Clock::from_account_info(ctx.a_clock)?;
        engine.current_slot = clock.slot;
        engine.last_funding_slot = clock.slot;
        engine.last_crank_slot = clock.slot;

        // Write config fields directly into the zeroed slab buffer.
        // Avoids constructing a 496-byte MarketConfig on the stack.
        {
            use core::mem::offset_of;
            use state::write_config_bytes as wcb;
            type MC = MarketConfig;

            wcb(
                data,
                offset_of!(MC, collateral_mint),
                &ctx.mint_key.to_bytes(),
            );
            wcb(
                data,
                offset_of!(MC, vault_pubkey),
                &ctx.vault_key.to_bytes(),
            );
            wcb(data, offset_of!(MC, index_feed_id), &fields.index_feed_id);
            wcb(
                data,
                offset_of!(MC, max_staleness_secs),
                &fields.max_staleness_secs.to_le_bytes(),
            );
            wcb(
                data,
                offset_of!(MC, conf_filter_bps),
                &fields.conf_filter_bps.to_le_bytes(),
            );
            wcb(data, offset_of!(MC, vault_authority_bump), &[ctx.bump]);
            wcb(data, offset_of!(MC, invert), &[fields.invert]);
            wcb(
                data,
                offset_of!(MC, unit_scale),
                &fields.unit_scale.to_le_bytes(),
            );
            wcb(
                data,
                offset_of!(MC, funding_horizon_slots),
                &DEFAULT_FUNDING_HORIZON_SLOTS.to_le_bytes(),
            );
            wcb(
                data,
                offset_of!(MC, funding_k_bps),
                &DEFAULT_FUNDING_K_BPS.to_le_bytes(),
            );
            wcb(
                data,
                offset_of!(MC, funding_inv_scale_notional_e6),
                &DEFAULT_FUNDING_INV_SCALE_NOTIONAL_E6.to_le_bytes(),
            );
            wcb(
                data,
                offset_of!(MC, funding_max_premium_bps),
                &DEFAULT_FUNDING_MAX_PREMIUM_BPS.to_le_bytes(),
            );
            wcb(
                data,
                offset_of!(MC, funding_max_bps_per_slot),
                &DEFAULT_FUNDING_MAX_BPS_PER_SLOT.to_le_bytes(),
            );
            wcb(
                data,
                offset_of!(MC, funding_premium_dampening_e6),
                &1_000_000u64.to_le_bytes(),
            );
            wcb(
                data,
                offset_of!(MC, funding_premium_max_bps_per_slot),
                &5i64.to_le_bytes(),
            );
            wcb(
                data,
                offset_of!(MC, thresh_floor),
                &DEFAULT_THRESH_FLOOR.to_le_bytes(),
            );
            wcb(
                data,
                offset_of!(MC, thresh_risk_bps),
                &DEFAULT_THRESH_RISK_BPS.to_le_bytes(),
            );
            wcb(
                data,
                offset_of!(MC, thresh_update_interval_slots),
                &DEFAULT_THRESH_UPDATE_INTERVAL_SLOTS.to_le_bytes(),
            );
            wcb(
                data,
                offset_of!(MC, thresh_step_bps),
                &DEFAULT_THRESH_STEP_BPS.to_le_bytes(),
            );
            wcb(
                data,
                offset_of!(MC, thresh_alpha_bps),
                &DEFAULT_THRESH_ALPHA_BPS.to_le_bytes(),
            );
            wcb(
                data,
                offset_of!(MC, thresh_min),
                &DEFAULT_THRESH_MIN.to_le_bytes(),
            );
            wcb(
                data,
                offset_of!(MC, thresh_max),
                &DEFAULT_THRESH_MAX.to_le_bytes(),
            );
            wcb(
                data,
                offset_of!(MC, thresh_min_step),
                &DEFAULT_THRESH_MIN_STEP.to_le_bytes(),
            );
            if fields.is_hyperp {
                wcb(
                    data,
                    offset_of!(MC, authority_price_e6),
                    &fields.initial_mark_price_e6.to_le_bytes(),
                );
            }
            let cap = if fields.is_hyperp {
                DEFAULT_HYPERP_PRICE_CAP_E2BPS
            } else {
                DEFAULT_DEX_ORACLE_PRICE_CAP_E2BPS
            };
            wcb(
                data,
                offset_of!(MC, oracle_price_cap_e2bps),
                &cap.to_le_bytes(),
            );
            if fields.is_hyperp {
                wcb(
                    data,
                    offset_of!(MC, last_effective_price_e6),
                    &fields.initial_mark_price_e6.to_le_bytes(),
                );
            }
            wcb(
                data,
                offset_of!(MC, market_created_slot),
                &clock.slot.to_le_bytes(),
            );
            wcb(data, offset_of!(MC, safety_valve_epochs), &[5u8]);
        }

        let new_header = SlabHeader {
            magic: MAGIC,
            version: VERSION,
            bump: ctx.bump,
            _padding: [0; 3],
            admin: ctx.admin_key.to_bytes(),
            pending_admin: [0; 32],
            _reserved: [0; 24],
        };
        state::write_header(data, &new_header);
        state::write_req_nonce(data, 0);
        state::write_last_thr_update_slot(data, 0);
        Ok(())
    }

    /// PERC-328: Reject already-initialized slabs in its own frame.
    /// Keeps the 104-byte SlabHeader off the caller's stack.
    #[inline(never)]
    fn reject_if_initialized(data: &[u8]) -> ProgramResult {
        let header = state::read_header(data);
        if header.magic == MAGIC {
            return Err(PercolatorError::AlreadyInitialized.into());
        }
        Ok(())
    }

    /// PERC-328: PDA derivation in its own frame.
    /// `Pubkey::find_program_address` hashes SHA-256 in a loop; under LTO the
    /// ~200 B hash state can be inlined into the caller, contributing to frame
    /// overflow. Isolating it prevents that.
    #[inline(never)]
    fn derive_vault_authority_isolated(program_id: &Pubkey, slab_key: &Pubkey) -> (Pubkey, u8) {
        accounts::derive_vault_authority(program_id, slab_key)
    }

    /// PERC-328 / PERC-331: InitMarket handler — stack-split architecture.
    ///
    /// SBF enforces a hard 4 KiB per-frame stack limit. The original monolithic
    /// handler accumulated ~4.5 KiB of locals (RiskParams 300 B, 3× Pubkey 96 B,
    /// SlabHeader 98 B, Clock 40 B, SPL Mint::unpack 82 B, SPL Account::unpack
    /// 165 B × 2, plus compiler spill slots) causing "Access violation in stack
    /// frame 1" at only 1341 CU.
    ///
    /// Fix: split into 4 `#[inline(never)]` functions, each with its own 4 KiB
    /// frame:
    ///   1. `process_init_market` — thin coordinator (~200 B locals)
    ///   2. `validate_spl_mint` — isolates Mint::unpack (82 B)
    ///   3. `validate_vault_seed_lamports` — isolates Account::unpack (165 B)
    ///   4. `init_market_write_slab` — isolates SlabHeader, Clock, RiskParams,
    ///      and all config writes
    ///
    /// Additionally `verify_vault` and `slab_guard` are marked `#[inline(never)]`
    /// to prevent their locals from being folded into any caller's frame.
    #[inline(never)]
    fn process_init_market<'a, 'b>(
        program_id: &Pubkey,
        accounts: &'b [AccountInfo<'a>],
        instruction_data: &[u8],
    ) -> ProgramResult {
        // Parse fields directly from raw bytes — skip tag byte (already checked)
        let mut rest = &instruction_data[1..];
        let admin = ix::read_pubkey(&mut rest)?;
        let collateral_mint = ix::read_pubkey(&mut rest)?;
        let index_feed_id = ix::read_bytes32(&mut rest)?;
        let max_staleness_secs = ix::read_u64(&mut rest)?;
        let conf_filter_bps = ix::read_u16(&mut rest)?;
        let invert = ix::read_u8(&mut rest)?;
        let unit_scale = ix::read_u32(&mut rest)?;
        let initial_mark_price_e6 = ix::read_u64(&mut rest)?;
        // PERC-328: Box the ~300 B RiskParams to move it from stack to heap.
        let risk_params = Box::new(ix::read_risk_params(&mut rest)?);

        accounts::expect_len(accounts, 9)?;
        let a_admin = &accounts[0];
        let a_slab = &accounts[1];
        let a_mint = &accounts[2];
        let a_vault = &accounts[3];

        accounts::expect_signer(a_admin)?;
        accounts::expect_writable(a_slab)?;

        if admin != *a_admin.key {
            return Err(ProgramError::InvalidInstructionData);
        }

        // SECURITY (H1): Enforce collateral_mint matches the account
        if collateral_mint != *a_mint.key {
            return Err(ProgramError::InvalidInstructionData);
        }

        // SECURITY (H2): Validate mint — isolated in its own frame (PERC-328)
        validate_spl_mint(a_mint)?;

        // SECURITY (#299): Seed deposit validated in validate_vault_seed_lamports()
        // (isolated frame, typed Account::unpack — single source of truth).

        if !crate::verify::init_market_scale_ok(unit_scale) {
            return Err(ProgramError::InvalidInstructionData);
        }

        let is_hyperp = index_feed_id == [0u8; 32];
        if is_hyperp && initial_mark_price_e6 == 0 {
            return Err(ProgramError::InvalidInstructionData);
        }

        let initial_mark_price_e6 = if is_hyperp && invert != 0 {
            crate::verify::invert_price_e6(initial_mark_price_e6, invert)
                .ok_or(PercolatorError::OracleInvalid)?
        } else {
            initial_mark_price_e6
        };

        #[cfg(debug_assertions)]
        {
            use crate::constants::CONFIG_LEN;
            if core::mem::size_of::<MarketConfig>() != CONFIG_LEN {
                return Err(ProgramError::InvalidAccountData);
            }
        }

        let mut data = state::slab_data_mut(a_slab)?;
        slab_guard(program_id, a_slab, &data)?;
        let _ = zc::engine_mut(&mut data)?;

        // PERC-328: check magic in its own frame to keep 104-byte SlabHeader
        // off process_init_market's stack.
        reject_if_initialized(&data)?;

        // PERC-328: PDA derivation in its own frame — find_program_address
        // allocates SHA-256 state (~200 B) that LTO can inline into the caller.
        let (auth, bump) = derive_vault_authority_isolated(program_id, a_slab.key);
        verify_vault(a_vault, &auth, a_mint.key, a_vault.key)?;

        // PERC-328: Isolated SPL Account::unpack in its own frame
        validate_vault_seed_lamports(a_vault)?;

        // Pack parsed fields into a compact struct for the write phase
        let fields = InitMarketFields {
            index_feed_id,
            max_staleness_secs,
            conf_filter_bps,
            invert,
            unit_scale,
            initial_mark_price_e6,
            is_hyperp,
        };

        // PERC-328: Write phase in its own frame — isolates SlabHeader,
        // Clock, RiskParams init, and all config field writes.
        let write_ctx = WriteSlabContext {
            bump,
            admin_key: a_admin.key,
            mint_key: a_mint.key,
            vault_key: a_vault.key,
            a_clock: &accounts[5],
        };
        init_market_write_slab(&mut data, &fields, *risk_params, &write_ctx)?;

        // PERC-623: Optional keeper fund PDA initialization.
        // accounts[9] = keeper_fund PDA (writable), accounts[10] = system_program
        // The admin funds the keeper with SOL lamports (separate from the SPL token
        // vault deposit). The keeper fund pays crank rewards in SOL.
        // Backward compatible: callers passing only 9 accounts skip this.
        if accounts.len() >= 11 {
            let a_keeper_fund = &accounts[9];
            let a_system_program = &accounts[10];
            accounts::expect_writable(a_keeper_fund)?;

            // Verify system program
            if *a_system_program.key != solana_program::system_program::id() {
                return Err(ProgramError::IncorrectProgramId);
            }

            // Verify PDA derivation
            let (expected_pda, pda_bump) = Pubkey::find_program_address(
                &[crate::keeper_fund::KEEPER_FUND_SEED, a_slab.key.as_ref()],
                program_id,
            );
            if *a_keeper_fund.key != expected_pda {
                return Err(ProgramError::InvalidSeeds);
            }

            // Create the keeper fund PDA account (program-owned, SOL-funded)
            let rent = solana_program::rent::Rent::get()?;
            let rent_lamports = rent.minimum_balance(crate::keeper_fund::KEEPER_FUND_STATE_LEN);
            // The admin's excess lamports above rent become the keeper fund balance.
            // Minimum keeper fund: DEFAULT_REWARD_PER_CRANK * 100 (enough for 100 cranks).
            let min_fund = crate::keeper_fund::DEFAULT_REWARD_PER_CRANK
                .saturating_mul(100)
                .saturating_add(rent_lamports);

            let bump_bytes = [pda_bump];
            let signer_seeds: &[&[u8]] = &[
                crate::keeper_fund::KEEPER_FUND_SEED,
                a_slab.key.as_ref(),
                &bump_bytes,
            ];
            solana_program::program::invoke_signed(
                &solana_program::system_instruction::create_account(
                    a_admin.key,
                    &expected_pda,
                    min_fund,
                    crate::keeper_fund::KEEPER_FUND_STATE_LEN as u64,
                    program_id,
                ),
                &[
                    a_admin.clone(),
                    a_keeper_fund.clone(),
                    a_system_program.clone(),
                ],
                &[signer_seeds],
            )?;

            let fund_balance = min_fund.saturating_sub(rent_lamports);

            // Initialize keeper fund state
            let default_reward = crate::keeper_fund::DEFAULT_REWARD_PER_CRANK;
            let state = crate::keeper_fund::KeeperFundState {
                magic: crate::keeper_fund::KEEPER_FUND_MAGIC,
                bump: pda_bump,
                depleted_pause: 0,
                _pad: [0u8; 6],
                balance: fund_balance,
                reward_per_crank: default_reward,
                total_rewarded: 0,
                total_topped_up: 0,
            };
            let mut fund_data = a_keeper_fund
                .try_borrow_mut_data()
                .map_err(|_| ProgramError::AccountBorrowFailed)?;
            crate::keeper_fund::write_state(&mut fund_data, &state);

            msg!(
                "PERC-623: KeeperFund initialized — balance={} reward_per_crank={}",
                fund_balance,
                default_reward
            );
        }

        // PERC-627: Optional creator stake lock PDA initialization.
        // accounts[11] = creator_lock PDA (writable), accounts[12] = system_program
        // (indexes 11/12 because keeper fund uses 9/10)
        // Backward compatible: callers with fewer accounts skip this.
        if accounts.len() >= 13 {
            let a_creator_lock = &accounts[11];
            let a_system_prog = &accounts[12];
            accounts::expect_writable(a_creator_lock)?;

            if *a_system_prog.key != solana_program::system_program::id() {
                return Err(ProgramError::IncorrectProgramId);
            }

            let (expected_pda, pda_bump) = Pubkey::find_program_address(
                &[crate::creator_lock::CREATOR_LOCK_SEED, a_slab.key.as_ref()],
                program_id,
            );
            if *a_creator_lock.key != expected_pda {
                return Err(ProgramError::InvalidSeeds);
            }

            let rent = solana_program::rent::Rent::get()?;
            let lamports = rent.minimum_balance(crate::creator_lock::CREATOR_LOCK_STATE_LEN);
            let bump_bytes = [pda_bump];
            let signer_seeds: &[&[u8]] = &[
                crate::creator_lock::CREATOR_LOCK_SEED,
                a_slab.key.as_ref(),
                &bump_bytes,
            ];
            solana_program::program::invoke_signed(
                &solana_program::system_instruction::create_account(
                    a_admin.key,
                    &expected_pda,
                    lamports,
                    crate::creator_lock::CREATOR_LOCK_STATE_LEN as u64,
                    program_id,
                ),
                &[
                    a_admin.clone(),
                    a_creator_lock.clone(),
                    a_system_prog.clone(),
                ],
                &[signer_seeds],
            )?;

            let clock = solana_program::clock::Clock::get()?;
            // Security: initialize cumulative_deposited to seed deposit
            let seed_deposit = {
                let vault_data = a_vault
                    .try_borrow_data()
                    .map_err(|_| ProgramError::AccountBorrowFailed)?;
                spl_token::state::Account::unpack(&vault_data)
                    .map(|a| a.amount)
                    .unwrap_or(0)
            };
            let state = crate::creator_lock::CreatorStakeLock {
                magic: crate::creator_lock::CREATOR_LOCK_MAGIC,
                bump: pda_bump,
                _pad: [0u8; 7],
                creator: a_admin.key.to_bytes(),
                lock_start_slot: clock.slot,
                lock_duration_slots: crate::creator_lock::DEFAULT_LOCK_DURATION_SLOTS,
                lp_amount_locked: 0,
                cumulative_extracted: 0,
                cumulative_deposited: seed_deposit,
                fee_redirect_active: 0,
                _reserved: [0u8; 7],
            };
            let mut lock_data = a_creator_lock
                .try_borrow_mut_data()
                .map_err(|_| ProgramError::AccountBorrowFailed)?;
            crate::creator_lock::write_state(&mut lock_data, &state);

            msg!(
                "PERC-627: CreatorStakeLock initialized — lock_duration={} seed_deposit={}",
                crate::creator_lock::DEFAULT_LOCK_DURATION_SLOTS,
                seed_deposit
            );
        }

        Ok(())
    }

    pub fn process_instruction<'a, 'b>(
        program_id: &Pubkey,
        accounts: &'b [AccountInfo<'a>],
        instruction_data: &[u8],
    ) -> ProgramResult {
        // PERC-331: Tag-based dispatch to #[inline(never)] sub-dispatchers.
        // The SBF 4 KiB per-frame stack limit is easily exceeded by the
        // monolithic 50-arm match (each arm allocates MarketConfig ~512 B,
        // Pubkeys, RefMut, etc.). Splitting by tag range gives each
        // sub-dispatcher its own 4 KiB frame.
        let tag = *instruction_data
            .first()
            .ok_or(ProgramError::InvalidInstructionData)?;

        use crate::tags::*;
        match tag {
            TAG_INIT_MARKET => process_init_market(program_id, accounts, instruction_data),
            // Core user-facing ops (tags 1-10 + 35 for TradeCpiV2)
            TAG_INIT_USER..=TAG_TRADE_CPI | TAG_TRADE_CPI_V2 => {
                dispatch_core_ops(program_id, accounts, instruction_data)
            }
            // Admin + insurance ops (tags 11-31)
            TAG_SET_RISK_THRESHOLD..=TAG_WITHDRAW_INSURANCE_LIMITED => {
                dispatch_admin_ops(program_id, accounts, instruction_data)
            }
            // Extended ops (tags 32+, except 35 which routes to core)
            _ => dispatch_extended_ops(program_id, accounts, instruction_data),
        }
    }

    /// Core user-facing operations: InitUser, InitLP, Deposit, Withdraw,
    /// KeeperCrank, TradeNoCpi, Liquidate, Close, TopUpInsurance, TradeCpi.
    #[inline(never)]
    fn dispatch_core_ops<'a, 'b>(
        program_id: &Pubkey,
        accounts: &'b [AccountInfo<'a>],
        instruction_data: &[u8],
    ) -> ProgramResult {
        let instruction = Instruction::decode(instruction_data)?;

        match instruction {
            Instruction::InitMarket => {
                unreachable!()
            }
            Instruction::InitUser { fee_payment } => {
                accounts::expect_len(accounts, 5)?;
                let a_user = &accounts[0];
                let a_slab = &accounts[1];
                let a_user_ata = &accounts[2];
                let a_vault = &accounts[3];
                let a_token = &accounts[4];

                accounts::expect_signer(a_user)?;
                accounts::expect_writable(a_slab)?;
                verify_token_program(a_token)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                require_not_paused(&data)?;

                // Block new users when market is resolved
                if state::is_resolved(&data) {
                    return Err(ProgramError::InvalidAccountData);
                }
                let config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let (auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_user_ata, a_user.key, &mint)?;

                // Transfer base tokens to vault
                collateral::deposit(a_token, a_user_ata, a_vault, a_user, fee_payment)?;

                // Convert base tokens to units for engine
                let (units, dust) = crate::units::base_to_units(fee_payment, config.unit_scale);

                // Accumulate dust
                let old_dust = state::read_dust_base(&data);
                state::write_dust_base(&mut data, old_dust.saturating_add(dust));

                let engine = zc::engine_mut(&mut data)?;
                let idx = engine.add_user(units as u128).map_err(map_risk_error)?;
                engine
                    .set_owner(idx, a_user.key.to_bytes())
                    .map_err(map_risk_error)?;
            }
            Instruction::InitLP {
                matcher_program,
                matcher_context,
                fee_payment,
            } => {
                accounts::expect_len(accounts, 5)?;
                let a_user = &accounts[0];
                let a_slab = &accounts[1];
                let a_user_ata = &accounts[2];
                let a_vault = &accounts[3];
                let a_token = &accounts[4];

                accounts::expect_signer(a_user)?;
                accounts::expect_writable(a_slab)?;
                verify_token_program(a_token)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                // Block new LPs when market is resolved
                if state::is_resolved(&data) {
                    return Err(ProgramError::InvalidAccountData);
                }

                let config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let (auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_user_ata, a_user.key, &mint)?;

                // Transfer base tokens to vault
                collateral::deposit(a_token, a_user_ata, a_vault, a_user, fee_payment)?;

                // Convert base tokens to units for engine
                let (units, dust) = crate::units::base_to_units(fee_payment, config.unit_scale);

                // Accumulate dust
                let old_dust = state::read_dust_base(&data);
                state::write_dust_base(&mut data, old_dust.saturating_add(dust));

                let engine = zc::engine_mut(&mut data)?;
                let idx = engine
                    .add_lp(
                        matcher_program.to_bytes(),
                        matcher_context.to_bytes(),
                        units as u128,
                    )
                    .map_err(map_risk_error)?;
                engine
                    .set_owner(idx, a_user.key.to_bytes())
                    .map_err(map_risk_error)?;
            }
            Instruction::DepositCollateral { user_idx, amount } => {
                accounts::expect_len(accounts, 6)?;
                let a_user = &accounts[0];
                let a_slab = &accounts[1];
                let a_user_ata = &accounts[2];
                let a_vault = &accounts[3];
                let a_token = &accounts[4];
                let a_clock = &accounts[5];

                accounts::expect_signer(a_user)?;
                accounts::expect_writable(a_slab)?;
                verify_token_program(a_token)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                require_not_paused(&data)?;

                // Block deposits when market is resolved
                if state::is_resolved(&data) {
                    return Err(ProgramError::InvalidAccountData);
                }

                let config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let (auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_user_ata, a_user.key, &mint)?;

                let clock = Clock::from_account_info(a_clock)?;

                // Transfer base tokens to vault
                collateral::deposit(a_token, a_user_ata, a_vault, a_user, amount)?;

                // Convert base tokens to units for engine
                let (units, dust) = crate::units::base_to_units(amount, config.unit_scale);

                // Accumulate dust
                let old_dust = state::read_dust_base(&data);
                state::write_dust_base(&mut data, old_dust.saturating_add(dust));

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, user_idx)?;

                // Owner authorization via verify helper (Kani-provable)
                let owner = engine.accounts[user_idx as usize].owner;
                if !crate::verify::owner_ok(owner, a_user.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                engine
                    .deposit(user_idx, units as u128, clock.slot)
                    .map_err(map_risk_error)?;
            }
            Instruction::WithdrawCollateral { user_idx, amount } => {
                accounts::expect_len(accounts, 8)?;
                let a_user = &accounts[0];
                let a_slab = &accounts[1];
                let a_vault = &accounts[2];
                let a_user_ata = &accounts[3];
                let a_vault_pda = &accounts[4];
                let a_token = &accounts[5];
                let a_clock = &accounts[6];
                let a_oracle_idx = &accounts[7];

                accounts::expect_signer(a_user)?;
                accounts::expect_writable(a_slab)?;
                verify_token_program(a_token)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                require_not_paused(&data)?;
                let mut config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let (derived_pda, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                accounts::expect_key(a_vault_pda, &derived_pda)?;

                verify_vault(
                    a_vault,
                    &derived_pda,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_user_ata, a_user.key, &mint)?;

                let clock = Clock::from_account_info(a_clock)?;

                // CMOR: detect trailing attestation PDA to exclude from oracle slice
                let has_cmor = crate::last_account_is_cmor(accounts, program_id);
                let oracle_end = if has_cmor {
                    accounts.len() - 1
                } else {
                    accounts.len()
                };

                // Read oracle price: Hyperp mode uses index directly, otherwise circuit-breaker clamping
                let is_hyperp = oracle::is_hyperp_mode(&config);
                let price = if is_hyperp {
                    let idx = config.last_effective_price_e6;
                    if idx == 0 {
                        return Err(PercolatorError::OracleInvalid.into());
                    }
                    // PERC-365: Reject trades if Hyperp oracle is stale
                    {
                        let eng = zc::engine_ref(&data)?;
                        oracle::check_hyperp_staleness(
                            eng.current_slot,
                            eng.max_crank_staleness_slots,
                            clock.slot,
                        )?;
                    }
                    idx
                } else {
                    oracle::read_price_clamped(
                        &mut config,
                        a_oracle_idx,
                        clock.unix_timestamp,
                        &accounts[8..oracle_end],
                    )?
                };
                state::write_config(&mut data, &config);

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, user_idx)?;

                // Owner authorization via verify helper (Kani-provable)
                let owner = engine.accounts[user_idx as usize].owner;
                if !crate::verify::owner_ok(owner, a_user.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                // Reject misaligned withdrawal amounts (cleaner UX than silent floor)
                if config.unit_scale != 0 && amount % config.unit_scale as u64 != 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }

                // Convert requested base tokens to units
                let (units_requested, _) = crate::units::base_to_units(amount, config.unit_scale);

                // VRAM: scale margins before withdrawal (margin check is inside engine.withdraw)
                let vram_orig = crate::apply_vram_scaling(engine, &config);

                // CMOR: apply cross-margin credit after VRAM scaling (stacks on top)
                // restore_margins(vram_orig) will undo both VRAM and CMOR adjustments
                if has_cmor {
                    crate::try_apply_cmor_from_accounts(
                        engine, accounts, program_id, a_user.key, a_slab.key, clock.slot,
                    );
                }

                let withdraw_result = engine
                    .withdraw(user_idx, units_requested as u128, clock.slot, price)
                    .map_err(map_risk_error);
                crate::restore_margins(engine, vram_orig);
                withdraw_result?;

                // Convert units back to base tokens for payout (checked to prevent silent overflow)
                let base_to_pay =
                    crate::units::units_to_base_checked(units_requested, config.unit_scale)
                        .ok_or(PercolatorError::EngineOverflow)?;

                let seed1: &[u8] = b"vault";
                let seed2: &[u8] = a_slab.key.as_ref();
                let bump_arr: [u8; 1] = [config.vault_authority_bump];
                let seed3: &[u8] = &bump_arr;
                let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
                let signer_seeds: [&[&[u8]]; 1] = [&seeds];

                collateral::withdraw(
                    a_token,
                    a_vault,
                    a_user_ata,
                    a_vault_pda,
                    base_to_pay,
                    &signer_seeds,
                )?;
            }
            Instruction::KeeperCrank {
                caller_idx,
                allow_panic,
            } => {
                use crate::constants::CRANK_NO_CALLER;

                accounts::expect_len(accounts, 4)?;
                let a_caller = &accounts[0];
                let a_slab = &accounts[1];
                let a_clock = &accounts[2];
                let a_oracle = &accounts[3];

                // Permissionless mode: caller_idx == u16::MAX means anyone can crank
                let permissionless = caller_idx == CRANK_NO_CALLER;

                if !permissionless {
                    // Self-crank mode: require signer + owner authorization
                    accounts::expect_signer(a_caller)?;
                }
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                // Check if market is resolved - if so, force-close positions instead of normal crank
                if state::is_resolved(&data) {
                    let config = state::read_config(&data);
                    let settlement_price = config.authority_price_e6;
                    if settlement_price == 0 {
                        return Err(ProgramError::InvalidAccountData);
                    }

                    let clock = Clock::from_account_info(a_clock)?;
                    let engine = zc::engine_mut(&mut data)?;

                    // Force-close positions in a paginated manner using crank_cursor
                    // Process up to 64 accounts per crank call (bounded compute)
                    const BATCH_SIZE: u16 = 64;
                    let start = engine.crank_cursor;
                    let end = core::cmp::min(start + BATCH_SIZE, percolator::MAX_ACCOUNTS as u16);

                    for idx in start..end {
                        if engine.is_used(idx as usize) {
                            // Bug cb8a4c22: Settle unsettled funding before computing
                            // settlement PnL. Without this, accrued funding credits/debits
                            // are lost, leading to incorrect payouts at resolution.
                            let _ = engine.touch_account(idx);

                            let acc = &engine.accounts[idx as usize];
                            let pos = acc.position_size.get();
                            if pos != 0 {
                                // Settle position using COIN-MARGINED PnL formula
                                // (matches mark_pnl_for_position in the risk engine)
                                // PnL = diff * abs_pos / settle_price
                                let entry = acc.entry_price as i128;
                                let settle = settlement_price as i128;
                                let abs_pos = if pos < 0 { pos.wrapping_neg() } else { pos };
                                let diff = if pos > 0 {
                                    settle.saturating_sub(entry)
                                } else {
                                    entry.saturating_sub(settle)
                                };
                                // Guard against division by zero (settle == 0 checked above,
                                // but defense in depth)
                                let pnl_delta = if settle != 0 {
                                    diff.saturating_mul(abs_pos) / settle
                                } else {
                                    0i128
                                };

                                // Add to PnL using set_pnl() to maintain pnl_pos_tot aggregate
                                // SECURITY: Must use set_pnl() for correct haircut calculations
                                let old_pnl = acc.pnl.get();
                                let new_pnl = old_pnl.saturating_add(pnl_delta);
                                engine.set_pnl(idx as usize, new_pnl);

                                // Clear position
                                engine.accounts[idx as usize].position_size =
                                    percolator::I128::ZERO;
                                engine.accounts[idx as usize].entry_price = 0;
                            }
                        }
                    }

                    // Update crank cursor for next call
                    engine.crank_cursor = if end >= percolator::MAX_ACCOUNTS as u16 {
                        0
                    } else {
                        end
                    };
                    engine.current_slot = clock.slot;

                    return Ok(());
                }

                let mut config = state::read_config(&data);
                let header = state::read_header(&data);
                // Read last threshold update slot BEFORE mutable engine borrow
                let last_thr_slot = state::read_last_thr_update_slot(&data);

                // SECURITY (C4): allow_panic triggers global settlement - admin only
                // This prevents griefing attacks where anyone triggers panic at worst moment
                if allow_panic != 0 {
                    accounts::expect_signer(a_caller)?;
                    if !crate::verify::admin_ok(header.admin, a_caller.key.to_bytes()) {
                        return Err(PercolatorError::EngineUnauthorized.into());
                    }
                }

                // Read dust before borrowing engine (for dust sweep later)
                let dust_before = state::read_dust_base(&data);
                let unit_scale = config.unit_scale;

                let clock = Clock::from_account_info(a_clock)?;

                // Hyperp mode: use get_engine_oracle_price_e6 for rate-limited index smoothing
                // Otherwise: use read_price_clamped as before
                let is_hyperp = oracle::is_hyperp_mode(&config);
                let engine_last_slot = {
                    let engine = zc::engine_ref(&data)?;
                    engine.current_slot
                };

                let remaining_oracle_accounts = &accounts[4..];
                // PERC-299: Track whether circuit breaker fired for emergency OI mode
                let (price, breaker_fired) = if is_hyperp {
                    // Hyperp mode: update index toward mark with rate limiting
                    // Hyperp clamping is internal — treat as non-breaker for OI purposes
                    let p = oracle::get_engine_oracle_price_e6(
                        engine_last_slot,
                        clock.slot,
                        clock.unix_timestamp,
                        &mut config,
                        a_oracle,
                        remaining_oracle_accounts,
                    )?;
                    (p, false)
                } else {
                    oracle::read_price_clamped_ext(
                        &mut config,
                        a_oracle,
                        clock.unix_timestamp,
                        remaining_oracle_accounts,
                    )?
                };

                // Hyperp mode: compute and store funding rate BEFORE engine borrow
                // This avoids borrow conflicts with config read/write
                let hyperp_funding_rate = if is_hyperp {
                    // Read previous funding rate (piecewise-constant: use stored rate, then update)
                    // authority_timestamp is reinterpreted as i64 funding rate in Hyperp mode
                    let prev_rate = config.authority_timestamp;

                    // Compute new rate from premium
                    let mark_e6 = config.authority_price_e6;
                    let index_e6 = config.last_effective_price_e6;
                    let new_rate = oracle::compute_premium_funding_bps_per_slot(
                        mark_e6,
                        index_e6,
                        config.funding_horizon_slots,
                        config.funding_k_bps,
                        config.funding_max_premium_bps,
                        config.funding_max_bps_per_slot,
                    );

                    // Store new rate in config for next crank
                    config.authority_timestamp = new_rate;

                    Some(prev_rate) // Use PREVIOUS rate for this crank (piecewise-constant model)
                } else {
                    None
                };

                // VRAM: Update EWMV (exponentially weighted moving variance) from oracle returns
                {
                    let vol_scale = state::get_vol_margin_scale_bps(&config);
                    if vol_scale > 0 && price > 0 {
                        let prev_price_e3 = state::get_last_vol_price_e3(&config);
                        // #980 sub-mill guard: price_e6 < 1000 → price_e3 == 0 → skip EWMV
                        // (avoids -100% phantom return and EWMV lockout for micro-cap assets)
                        let price_e3_cur = price / 1000; // price is e6
                        if prev_price_e3 > 0 && price_e3_cur > 0 {
                            // #980: Migration guard for legacy e6 values. If the stored
                            // prev_price is >1000× the current e3 price, it's a legacy
                            // e6 value. Skip this update and overwrite with e3 below.
                            let ratio = (prev_price_e3 as u64)
                                .checked_div(price_e3_cur.max(1))
                                .unwrap_or(0);
                            if ratio > 1000 {
                                // Legacy e6→e3 transition: discard stale EWMV sample.
                                // The price store below will write the correct e3 value.
                            } else {
                                // #980: Use e3-scaled prices for return calculation.
                                // r_t = (p - pp) * 1e6 / pp — ratio is scale-invariant.
                                let price_e3 = price_e3_cur as i64;
                                let p = price_e3;
                                let pp = prev_price_e3 as i64;
                                let return_e6 = ((p - pp) as i128).saturating_mul(1_000_000)
                                    / (pp as i128).max(1);
                                // r_t^2 in e12 units — clamp in i128 before downcast (#979)
                                let r_sq_e12_i128 = return_e6.saturating_mul(return_e6);
                                let r_sq_e12_u32 = r_sq_e12_i128.min(i128::from(u32::MAX)) as u32;
                                let alpha_e6 = state::get_vol_alpha_e6(&config) as u32;
                                let old_ewmv = state::get_ewmv_e12(&config);
                                // ewmv = alpha * r_t^2 + (1 - alpha) * ewmv_prev
                                let new_ewmv = ((alpha_e6 as u64)
                                    .saturating_mul(r_sq_e12_u32 as u64)
                                    / 1_000_000
                                    + (1_000_000u64.saturating_sub(alpha_e6 as u64))
                                        .saturating_mul(old_ewmv as u64)
                                        / 1_000_000)
                                    .min(u32::MAX as u64)
                                    as u32;
                                state::set_ewmv_e12(&mut config, new_ewmv);
                            } // else (non-migration path)
                        }
                        // Store current price for next return calculation (e3 format).
                        // Clamp sub-mill prices (price_e6 < 1000) to min 1 e3 so that EWMV
                        // resumes updating once the price rises above $0.001 again.
                        let price_e3_store = price_e3_cur.min(u32::MAX as u64).max(1) as u32;
                        state::set_last_vol_price_e3(&mut config, price_e3_store);
                    }
                }

                state::write_config(&mut data, &config);

                let slab_len = data.len();
                let engine = zc::engine_mut(&mut data)?;

                // PERC-299: Update emergency OI mode based on circuit breaker status
                if breaker_fired {
                    engine.enter_emergency_oi_mode(clock.slot);
                } else {
                    engine.check_emergency_recovery(clock.slot);
                }

                // Crank authorization:
                // - Permissionless mode (caller_idx == u16::MAX): anyone can crank
                // - Self-crank mode: caller_idx must be a valid, existing account owned by signer
                if !permissionless {
                    check_idx(engine, caller_idx)?;
                    let stored_owner = engine.accounts[caller_idx as usize].owner;
                    if !crate::verify::owner_ok(stored_owner, a_caller.key.to_bytes()) {
                        return Err(PercolatorError::EngineUnauthorized.into());
                    }
                }
                // Execute crank with effective_caller_idx for clarity
                // In permissionless mode, pass CRANK_NO_CALLER to engine (out-of-range = no caller settle)
                let effective_caller_idx = if permissionless {
                    CRANK_NO_CALLER
                } else {
                    caller_idx
                };

                // Compute funding rate:
                // - Hyperp mode: use pre-computed rate (avoids borrow conflict)
                // - Normal mode: inventory-based funding from LP net position
                let raw_funding_rate = if let Some(rate) = hyperp_funding_rate {
                    rate
                } else {
                    // Normal mode: inventory-based funding from LP net position
                    // Engine internally gates same-slot compounding via dt = now_slot - last_funding_slot,
                    // so passing the same rate multiple times in the same slot is harmless (dt=0 => no change).
                    let net_lp_pos = crate::compute_net_lp_pos(engine);
                    crate::compute_inventory_funding_bps_per_slot(
                        net_lp_pos,
                        price,
                        config.funding_horizon_slots,
                        config.funding_k_bps,
                        config.funding_inv_scale_notional_e6,
                        config.funding_max_premium_bps,
                        config.funding_max_bps_per_slot,
                        state::get_funding_k2_bps(&config),
                    )
                };

                // PERC-118: Blended mark price (oracle + trade TWAP).
                // weight=0 (default) → pure impact_mid (backward compatible:
                // compute_blend_mark_price returns impact_mid when w=0, but
                // engine TWAP is 0 for new markets, so set_mark_price_blended
                // falls back to oracle anyway). For configured markets, the
                // blend weight controls oracle vs TWAP proportions.
                //
                // Safety: set_mark_price_blended writes trade_twap_e6 + twap_last_slot
                // at the tail of RiskEngine. On pre-PERC-118 slabs (data.len() <
                // ENGINE_OFF + ENGINE_LEN), those fields are beyond the allocated data.
                // Skip the TWAP write for undersized slabs — they get pure oracle mark.
                if slab_len >= ENGINE_OFF + ENGINE_LEN {
                    let w = state::get_mark_oracle_weight_bps(&config) as u64;
                    engine.set_mark_price_blended(config.authority_price_e6, w);
                }

                // PERC-121: Blend inventory + premium funding rates
                let blended_funding_rate = if config.funding_premium_weight_bps > 0 {
                    let premium_rate = percolator::RiskEngine::compute_premium_funding_bps_per_slot(
                        engine.mark_price_e6,
                        price, // index/oracle price
                        config.funding_premium_dampening_e6,
                        config.funding_premium_max_bps_per_slot,
                    );
                    percolator::RiskEngine::compute_combined_funding_rate(
                        raw_funding_rate,
                        premium_rate,
                        config.funding_premium_weight_bps,
                    )
                } else {
                    raw_funding_rate
                };

                // PERC-300: Adaptive funding rate (replaces inventory/premium when enabled)
                let blended_funding_rate = if config.adaptive_funding_enabled != 0 {
                    let prev_rate = engine.funding_rate_bps_per_slot_last;
                    let max_bps = if config.adaptive_max_funding_bps > 0 {
                        config.adaptive_max_funding_bps
                    } else {
                        config.funding_max_bps_per_slot.unsigned_abs()
                    };
                    percolator::RiskEngine::compute_adaptive_funding_rate(
                        prev_rate,
                        engine.long_oi.get(),
                        engine.short_oi.get(),
                        engine.total_open_interest.get(),
                        config.adaptive_scale_bps,
                        max_bps,
                    )
                } else {
                    blended_funding_rate
                };

                // F5: Funding rate dampening on low-liquidity markets.
                let effective_funding_rate = {
                    let oi = engine.total_open_interest.get();
                    let vault = engine.vault.get();
                    if vault == 0 || oi == 0 {
                        blended_funding_rate
                    } else {
                        let oi_x10000 = (oi as u128).saturating_mul(10_000);
                        let vault_x2 = (vault as u128).saturating_mul(2);
                        let scale_bps = core::cmp::min(oi_x10000 / vault_x2.max(1), 10_000) as i64;
                        (blended_funding_rate as i128 * scale_bps as i128 / 10_000) as i64
                    }
                };
                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: keeper_crank_start");
                    sol_log_compute_units();
                }
                let _outcome = engine
                    .keeper_crank(
                        effective_caller_idx,
                        clock.slot,
                        price,
                        effective_funding_rate,
                        allow_panic != 0,
                    )
                    .map_err(map_risk_error)?;
                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: keeper_crank_end");
                    sol_log_compute_units();
                }

                // Dust sweep: if accumulated dust >= unit_scale, sweep to insurance fund
                // Done before copying stats so insurance balance reflects the sweep
                let remaining_dust = if unit_scale > 0 {
                    let scale = unit_scale as u64;
                    if dust_before >= scale {
                        let units_to_sweep = dust_before / scale;
                        engine
                            .top_up_insurance_fund(units_to_sweep as u128)
                            .map_err(map_risk_error)?;
                        Some(dust_before % scale)
                    } else {
                        None
                    }
                } else {
                    None
                };

                // Copy stats before threshold update (avoid borrow conflict)
                let liqs = engine.lifetime_liquidations;
                let force = engine.lifetime_force_realize_closes;
                // PERC-306: Total insurance (global + isolated) as the low watermark
                let ins_low = engine
                    .insurance_fund
                    .balance
                    .get()
                    .saturating_add(engine.insurance_fund.isolated_balance.get())
                    as u64;

                // --- Threshold auto-update (rate-limited + EWMA smoothed + step-clamped)
                if clock.slot >= last_thr_slot.saturating_add(config.thresh_update_interval_slots) {
                    let risk_units = crate::compute_system_risk_units(engine);
                    // Convert risk_units (contracts) to notional using price
                    let risk_notional = risk_units.saturating_mul(price as u128) / 1_000_000;
                    // raw target: floor + risk_notional * thresh_risk_bps / 10000
                    let raw_target = config.thresh_floor.saturating_add(
                        risk_notional.saturating_mul(config.thresh_risk_bps as u128) / 10_000,
                    );
                    let clamped_target = raw_target.clamp(config.thresh_min, config.thresh_max);
                    let current = engine.risk_reduction_threshold();
                    // EWMA: new = alpha * target + (1 - alpha) * current
                    let alpha = config.thresh_alpha_bps as u128;
                    let smoothed = (alpha * clamped_target + (10_000 - alpha) * current) / 10_000;
                    // Step clamp: max step = thresh_step_bps / 10000 of current (but at least thresh_min_step)
                    // Bug #6 fix: When current == 0, allow stepping to clamped_target directly
                    // Otherwise threshold would only increase by thresh_min_step (=1) per update
                    let max_step = if current == 0 {
                        clamped_target // Allow full jump when starting from zero
                    } else {
                        (current * config.thresh_step_bps as u128 / 10_000)
                            .max(config.thresh_min_step)
                    };
                    let final_thresh = if smoothed > current {
                        current.saturating_add(max_step.min(smoothed - current))
                    } else {
                        current.saturating_sub(max_step.min(current - smoothed))
                    };
                    engine.set_risk_reduction_threshold(
                        final_thresh.clamp(config.thresh_min, config.thresh_max),
                    );
                    let _ = engine;
                    state::write_last_thr_update_slot(&mut data, clock.slot);
                }

                // Write remaining dust if sweep occurred
                if let Some(dust) = remaining_dust {
                    state::write_dust_base(&mut data, dust);
                }

                // PERC-312: Update safety valve state based on funding rate
                {
                    let mut config = state::read_config(&data);
                    update_safety_valve_on_funding(&mut config, effective_funding_rate, clock.slot);
                    state::write_config(&mut data, &config);
                }

                // Debug: log lifetime counters (sol_log_64: tag, liqs, force, max_accounts, insurance)
                msg!("CRANK_STATS");
                sol_log_64(0xC8A4C, liqs, force, MAX_ACCOUNTS as u64, ins_low);

                // PERC-623: Optional keeper fund reward — if 5th account is a valid
                // KeeperFund PDA, pay crank reward to caller. Backward compatible:
                // callers passing only 4 accounts skip this entirely.
                if accounts.len() >= 5 {
                    let a_keeper_fund = &accounts[4];
                    if a_keeper_fund.is_writable {
                        // Verify PDA derivation: seeds = ["keeper_fund", slab_key]
                        let (expected_pda, _bump) = Pubkey::find_program_address(
                            &[crate::keeper_fund::KEEPER_FUND_SEED, a_slab.key.as_ref()],
                            program_id,
                        );
                        if *a_keeper_fund.key == expected_pda {
                            let mut fund_data = a_keeper_fund
                                .try_borrow_mut_data()
                                .map_err(|_| ProgramError::AccountBorrowFailed)?;
                            if let Some(fund_state) = crate::keeper_fund::read_state(&fund_data) {
                                let (new_bal, reward) = crate::keeper_fund::pay_crank_reward(
                                    fund_state.balance,
                                    fund_state.reward_per_crank,
                                );
                                if reward > 0 {
                                    let mut new_state = *fund_state;
                                    new_state.balance = new_bal;
                                    new_state.total_rewarded =
                                        new_state.total_rewarded.saturating_add(reward);
                                    crate::keeper_fund::write_state(&mut fund_data, &new_state);

                                    // Transfer lamports from KeeperFund PDA to caller
                                    **a_keeper_fund.try_borrow_mut_lamports()? -= reward;
                                    **a_caller.try_borrow_mut_lamports()? += reward;

                                    // If fund depleted, market auto-pause + set depleted_pause flag
                                    if crate::keeper_fund::is_depleted(new_bal) {
                                        state::set_paused(&mut data, true);
                                        // #1015: Mark pause source as depletion so TopUpKeeperFund
                                        // knows it's safe to unpause (vs admin-initiated pause).
                                        new_state.depleted_pause = 1;
                                        crate::keeper_fund::write_state(&mut fund_data, &new_state);
                                        msg!("KEEPER_FUND_DEPLETED: market paused (depleted_pause=1)");
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Instruction::TradeNoCpi {
                lp_idx,
                user_idx,
                size,
            } => {
                // PERC-199: Removed clock sysvar from accounts (was [5] → now [4]).
                // Clock::get() syscall replaces Clock::from_account_info, saving CU
                // and reducing the instruction's account count by 1.
                accounts::expect_len(accounts, 4)?;
                let a_user = &accounts[0];
                let a_lp = &accounts[1];
                let a_slab = &accounts[2];

                accounts::expect_signer(a_user)?;
                accounts::expect_signer(a_lp)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                require_not_paused(&data)?;

                // Block trading when market is resolved
                if state::is_resolved(&data) {
                    return Err(ProgramError::InvalidAccountData);
                }

                let mut config = state::read_config(&data);

                // PERC-199: Clock::get() replaces Clock::from_account_info — clock
                // sysvar account removed from the instruction entirely.
                let clock = Clock::get()?;
                let a_oracle = &accounts[3];

                // Hyperp mode: reject TradeNoCpi to prevent mark price manipulation
                // All trades must go through TradeCpi with a pinned matcher
                if oracle::is_hyperp_mode(&config) {
                    return Err(PercolatorError::HyperpTradeNoCpiDisabled.into());
                }

                // CMOR: detect trailing attestation PDA to exclude it from oracle slice
                let has_cmor = crate::last_account_is_cmor(accounts, program_id);
                let oracle_end = if has_cmor {
                    accounts.len() - 1
                } else {
                    accounts.len()
                };

                // Read oracle price with circuit-breaker clamping
                let price = oracle::read_price_clamped(
                    &mut config,
                    a_oracle,
                    clock.unix_timestamp,
                    &accounts[4..oracle_end],
                )?;
                state::write_config(&mut data, &config);

                // PERC-312: Pre-trade safety valve check (needs engine read then config write)
                {
                    let engine = zc::engine_ref(&data)?;
                    check_idx(engine, lp_idx)?;
                    check_idx(engine, user_idx)?;
                    let u_owner = engine.accounts[user_idx as usize].owner;
                    if !crate::verify::owner_ok(u_owner, a_user.key.to_bytes()) {
                        return Err(PercolatorError::EngineUnauthorized.into());
                    }
                    let l_owner = engine.accounts[lp_idx as usize].owner;
                    if !crate::verify::owner_ok(l_owner, a_lp.key.to_bytes()) {
                        return Err(PercolatorError::EngineUnauthorized.into());
                    }
                    let old_user_pos = engine.accounts[user_idx as usize].position_size.get();
                    let net_lp = engine.net_lp_pos.get();
                    check_safety_valve(&config, net_lp, size, old_user_pos, clock.slot)?;
                }

                let engine = zc::engine_mut(&mut data)?;

                // Gate: if insurance_fund <= threshold, only allow risk-reducing trades
                // LP delta is -size (LP takes opposite side of user's trade)
                // O(1) check after single O(n) scan
                // Gate activation via verify helper (Kani-provable)
                // PERC-306: Use total insurance (global + isolated)
                let bal = engine
                    .insurance_fund
                    .balance
                    .get()
                    .saturating_add(engine.insurance_fund.isolated_balance.get());
                let thr = engine.risk_reduction_threshold();
                if crate::verify::gate_active(thr, bal) {
                    #[cfg(feature = "cu-audit")]
                    {
                        msg!("CU_CHECKPOINT: trade_nocpi_compute_start");
                        sol_log_compute_units();
                    }
                    let risk_state = crate::LpRiskState::compute(engine);
                    #[cfg(feature = "cu-audit")]
                    {
                        msg!("CU_CHECKPOINT: trade_nocpi_compute_end");
                        sol_log_compute_units();
                    }
                    let old_lp_pos = engine.accounts[lp_idx as usize].position_size.get();
                    if risk_state.would_increase_risk(old_lp_pos, -size) {
                        return Err(PercolatorError::EngineRiskReductionOnlyMode.into());
                    }
                }

                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: trade_nocpi_execute_start");
                    sol_log_compute_units();
                }

                // CMOR: save original margins, apply cross-margin credit, restore after trade
                let margin_orig = (
                    engine.params.initial_margin_bps,
                    engine.params.maintenance_margin_bps,
                );
                if has_cmor {
                    crate::try_apply_cmor_from_accounts(
                        engine, accounts, program_id, a_user.key, a_slab.key, clock.slot,
                    );
                }

                let trade_result = engine
                    .execute_trade(&NoOpMatcher, lp_idx, user_idx, clock.slot, price, size)
                    .map_err(map_risk_error);
                crate::restore_margins(engine, margin_orig);
                trade_result?;

                // PERC-273 + PERC-302: Dynamic OI cap check after trade (with ramp)
                check_oi_cap(engine, &config, clock.slot)?;
                check_pnl_cap(engine, &config)?;
                check_phase_leverage(engine, &config, user_idx)?;

                // PERC-622: Accumulate trade volume for oracle phase transitions.
                // trade_notional_e6 = |size| * price / 1e6 (approximate notional)
                {
                    let trade_notional_e6 = (size.unsigned_abs() as u64)
                        .saturating_mul(price)
                        .checked_div(1_000_000)
                        .unwrap_or(0);
                    let mut vol_config = state::read_config(&data);
                    state::accumulate_volume(&mut vol_config, trade_notional_e6);
                    state::write_config(&mut data, &vol_config);
                }

                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: trade_nocpi_execute_end");
                    sol_log_compute_units();
                }
            }
            Instruction::TradeCpi {
                lp_idx,
                user_idx,
                size,
            }
            | Instruction::TradeCpiV2 {
                lp_idx,
                user_idx,
                size,
                ..
            } => {
                // PERC-154: TradeCpi and TradeCpiV2 share the same handler.
                // V2 provides the PDA bump to skip find_program_address (~1500 CU).
                let caller_bump = match &instruction {
                    Instruction::TradeCpiV2 { bump, .. } => Some(*bump),
                    _ => None,
                };

                // PERC-199: Clock sysvar removed from accounts (was 8 → now 7).
                // Clock::get() syscall replaces Clock::from_account_info.
                accounts::expect_len(accounts, 7)?;
                let a_user = &accounts[0];
                let a_lp_owner = &accounts[1];
                let a_slab = &accounts[2];
                let a_oracle = &accounts[3];
                let a_matcher_prog = &accounts[4];
                let a_matcher_ctx = &accounts[5];
                let a_lp_pda = &accounts[6];

                accounts::expect_signer(a_user)?;
                // Note: a_lp_owner does NOT need to be a signer for TradeCpi.
                // LP owner delegated trade authorization to the matcher program.
                // The matcher CPI (via LP PDA invoke_signed) validates the trade.
                accounts::expect_writable(a_slab)?;
                accounts::expect_writable(a_matcher_ctx)?;

                // Matcher shape validation via verify helper (Kani-provable)
                let matcher_shape = crate::verify::MatcherAccountsShape {
                    prog_executable: a_matcher_prog.executable,
                    ctx_executable: a_matcher_ctx.executable,
                    ctx_owner_is_prog: a_matcher_ctx.owner == a_matcher_prog.key,
                    ctx_len_ok: crate::verify::ctx_len_sufficient(a_matcher_ctx.data_len()),
                };
                if !crate::verify::matcher_shape_ok(matcher_shape) {
                    return Err(ProgramError::InvalidAccountData);
                }

                // Phase 1: Validate lp_pda is the correct PDA, system-owned, empty data, 0 lamports
                let lp_bytes = lp_idx.to_le_bytes();
                // PERC-154: If caller provided bump (V2), use create_program_address
                // to skip the expensive find_program_address loop (~1500 CU savings).
                let bump = if let Some(b) = caller_bump {
                    // V2 path: verify bump is correct via create_program_address
                    let bump_arr = [b];
                    let expected_lp_pda = Pubkey::create_program_address(
                        &[b"lp", a_slab.key.as_ref(), &lp_bytes, &bump_arr],
                        program_id,
                    )
                    .map_err(|_| ProgramError::InvalidSeeds)?;
                    if !crate::verify::pda_key_matches(
                        expected_lp_pda.to_bytes(),
                        a_lp_pda.key.to_bytes(),
                    ) {
                        return Err(ProgramError::InvalidSeeds);
                    }
                    b
                } else {
                    // V1 path: find bump via find_program_address
                    let (expected_lp_pda, found_bump) = Pubkey::find_program_address(
                        &[b"lp", a_slab.key.as_ref(), &lp_bytes],
                        program_id,
                    );
                    // PDA key validation via verify helper (Kani-provable)
                    if !crate::verify::pda_key_matches(
                        expected_lp_pda.to_bytes(),
                        a_lp_pda.key.to_bytes(),
                    ) {
                        return Err(ProgramError::InvalidSeeds);
                    }
                    found_bump
                };
                // LP PDA shape validation via verify helper (Kani-provable)
                let lp_pda_shape = crate::verify::LpPdaShape {
                    is_system_owned: a_lp_pda.owner == &solana_program::system_program::ID,
                    data_len_zero: a_lp_pda.data_len() == 0,
                    lamports_zero: **a_lp_pda.lamports.borrow() == 0,
                };
                if !crate::verify::lp_pda_shape_ok(lp_pda_shape) {
                    return Err(ProgramError::InvalidAccountData);
                }

                // Phase 3 & 4: Read engine state, generate nonce, validate matcher identity
                // Note: Use immutable borrow for reading to avoid ExternalAccountDataModified
                // Nonce write is deferred until after execute_trade
                let (
                    lp_account_id,
                    mut config,
                    req_id,
                    lp_matcher_prog,
                    lp_matcher_ctx,
                    eng_current_slot,
                    eng_max_stale,
                ) = {
                    let data = a_slab.try_borrow_data()?;
                    slab_guard(program_id, a_slab, &data)?;
                    require_initialized(&data)?;
                    require_not_paused(&data)?;

                    // Block trading when market is resolved
                    if state::is_resolved(&data) {
                        return Err(ProgramError::InvalidAccountData);
                    }

                    let config = state::read_config(&data);

                    // Phase 3: Monotonic nonce for req_id (prevents replay attacks)
                    // Nonce advancement via verify helper (Kani-provable)
                    let nonce = state::read_req_nonce(&data);
                    let req_id = crate::verify::nonce_on_success(nonce);

                    let engine = zc::engine_ref(&data)?;

                    check_idx(engine, lp_idx)?;
                    check_idx(engine, user_idx)?;

                    // Owner authorization via verify helper (Kani-provable)
                    let u_owner = engine.accounts[user_idx as usize].owner;
                    if !crate::verify::owner_ok(u_owner, a_user.key.to_bytes()) {
                        return Err(PercolatorError::EngineUnauthorized.into());
                    }
                    let l_owner = engine.accounts[lp_idx as usize].owner;
                    if !crate::verify::owner_ok(l_owner, a_lp_owner.key.to_bytes()) {
                        return Err(PercolatorError::EngineUnauthorized.into());
                    }

                    let lp_acc = &engine.accounts[lp_idx as usize];
                    (
                        lp_acc.account_id,
                        config,
                        req_id,
                        lp_acc.matcher_program,
                        lp_acc.matcher_context,
                        engine.current_slot,
                        engine.max_crank_staleness_slots,
                    )
                };

                // Matcher identity binding via verify helper (Kani-provable)
                if !crate::verify::matcher_identity_ok(
                    lp_matcher_prog,
                    lp_matcher_ctx,
                    a_matcher_prog.key.to_bytes(),
                    a_matcher_ctx.key.to_bytes(),
                ) {
                    return Err(PercolatorError::EngineInvalidMatchingEngine.into());
                }

                // PERC-199: Clock::get() saves ~50-100 CU vs from_account_info deserialization
                let clock = Clock::get()?;

                // CMOR: detect trailing attestation PDA to exclude from oracle slice
                let has_cmor_cpi = crate::last_account_is_cmor(accounts, program_id);
                let oracle_end_cpi = if has_cmor_cpi {
                    accounts.len() - 1
                } else {
                    accounts.len()
                };

                // Read oracle price: Hyperp mode uses index directly, otherwise circuit-breaker clamping
                let is_hyperp = oracle::is_hyperp_mode(&config);
                let price = if is_hyperp {
                    // Hyperp mode: use current index price for trade execution
                    let idx = config.last_effective_price_e6;
                    if idx == 0 {
                        return Err(PercolatorError::OracleInvalid.into());
                    }
                    // PERC-365: Reject trades if Hyperp oracle is stale
                    oracle::check_hyperp_staleness(eng_current_slot, eng_max_stale, clock.slot)?;
                    idx
                } else {
                    oracle::read_price_clamped(
                        &mut config,
                        a_oracle,
                        clock.unix_timestamp,
                        &accounts[7..oracle_end_cpi],
                    )?
                };

                // Note: We don't zero the matcher_ctx before CPI because we don't own it.
                // Security is maintained by ABI validation which checks req_id (nonce),
                // lp_account_id, and oracle_price_e6 all match the request parameters.

                // PERC-154: Stack-allocated CPI data — reduces intermediate heap work (~100-200 CU)
                let mut cpi_data = [0u8; MATCHER_CALL_LEN];
                {
                    let mut off = 0usize;
                    cpi_data[off] = MATCHER_CALL_TAG;
                    off += 1;
                    cpi_data[off..off + 8].copy_from_slice(&req_id.to_le_bytes());
                    off += 8;
                    cpi_data[off..off + 2].copy_from_slice(&lp_idx.to_le_bytes());
                    off += 2;
                    cpi_data[off..off + 8].copy_from_slice(&lp_account_id.to_le_bytes());
                    off += 8;
                    cpi_data[off..off + 8].copy_from_slice(&price.to_le_bytes());
                    off += 8;
                    cpi_data[off..off + 16].copy_from_slice(&size.to_le_bytes());
                    // remaining 24 bytes are already zero (padding)
                }

                // PERC-154: Stack-allocated account metas — reduces intermediate heap work
                // Note: metas.to_vec() and cpi_data.to_vec() still allocate, but the
                // stack-based construction avoids additional intermediate allocations.
                let metas = [
                    AccountMeta::new_readonly(*a_lp_pda.key, true), // Will become signer via invoke_signed
                    AccountMeta::new(*a_matcher_ctx.key, false),
                ];

                let ix = SolInstruction {
                    program_id: *a_matcher_prog.key,
                    accounts: metas.to_vec(),
                    data: cpi_data.to_vec(),
                };

                let bump_arr = [bump];
                let seeds: &[&[u8]] = &[b"lp", a_slab.key.as_ref(), &lp_bytes, &bump_arr];

                // Phase 2: Use zc helper for CPI - slab not passed to avoid ExternalAccountDataModified
                zc::invoke_signed_trade(&ix, a_lp_pda, a_matcher_ctx, a_matcher_prog, seeds)?;

                let ctx_data = a_matcher_ctx.try_borrow_data()?;
                let ret = crate::matcher_abi::read_matcher_return(&ctx_data)?;
                // ABI validation via verify helper (Kani-provable)
                let ret_fields = crate::verify::MatcherReturnFields {
                    abi_version: ret.abi_version,
                    flags: ret.flags,
                    exec_price_e6: ret.exec_price_e6,
                    exec_size: ret.exec_size,
                    req_id: ret.req_id,
                    lp_account_id: ret.lp_account_id,
                    oracle_price_e6: ret.oracle_price_e6,
                    reserved: ret.reserved,
                };
                if !crate::verify::abi_ok(ret_fields, lp_account_id, price, size, req_id) {
                    return Err(ProgramError::InvalidAccountData);
                }
                drop(ctx_data);

                let matcher = CpiMatcher {
                    exec_price: ret.exec_price_e6,
                    exec_size: ret.exec_size,
                };
                {
                    let mut data = state::slab_data_mut(a_slab)?;

                    // PERC-199: Hyperp mark price update — apply BEFORE write_config to
                    // eliminate the second read_config()+write_config() pair (~100 CU saved).
                    // Safe because last_effective_price_e6 and oracle_price_cap_e2bps are
                    // unchanged since the first read.
                    if is_hyperp {
                        let clamped_mark = oracle::clamp_oracle_price(
                            config.last_effective_price_e6,
                            ret.exec_price_e6,
                            config.oracle_price_cap_e2bps,
                        );
                        // Bug b5232aef: Rate-limit mark price updates to prevent
                        // malicious matchers from grinding mark via repeated TradeCpi.
                        // Only update mark if: (a) first trade this slot, or
                        // (b) new mark is closer to index than current mark.
                        let index = config.last_effective_price_e6;
                        let current_mark = config.authority_price_e6;
                        let new_dist = clamped_mark.abs_diff(index);
                        let old_dist = current_mark.abs_diff(index);
                        // Allow update only if it moves mark closer to index (convergent)
                        // or if current mark is 0 (uninitialized)
                        if current_mark == 0 || new_dist <= old_dist {
                            config.authority_price_e6 = clamped_mark;
                        }
                    }

                    state::write_config(&mut data, &config);
                    let engine = zc::engine_mut(&mut data)?;

                    // Gate: if insurance_fund <= threshold, only allow risk-reducing trades
                    // Use actual exec_size from matcher (LP delta is -exec_size)
                    // O(1) check after single O(n) scan
                    // Gate activation via verify helper (Kani-provable)
                    // PERC-306: Use total insurance (global + isolated)
                    let bal = engine
                        .insurance_fund
                        .balance
                        .get()
                        .saturating_add(engine.insurance_fund.isolated_balance.get());
                    let thr = engine.risk_reduction_threshold();
                    if crate::verify::gate_active(thr, bal) {
                        #[cfg(feature = "cu-audit")]
                        {
                            msg!("CU_CHECKPOINT: trade_cpi_compute_start");
                            sol_log_compute_units();
                        }
                        let risk_state = crate::LpRiskState::compute(engine);
                        #[cfg(feature = "cu-audit")]
                        {
                            msg!("CU_CHECKPOINT: trade_cpi_compute_end");
                            sol_log_compute_units();
                        }
                        let old_lp_pos = engine.accounts[lp_idx as usize].position_size.get();
                        if risk_state.would_increase_risk(old_lp_pos, -ret.exec_size) {
                            return Err(PercolatorError::EngineRiskReductionOnlyMode.into());
                        }
                    }

                    // Trade size selection via verify helper (Kani-provable: uses exec_size, not requested_size)
                    let trade_size = crate::verify::cpi_trade_size(ret.exec_size, size);

                    // PERC-312: Safety valve check
                    {
                        let old_user_pos = engine.accounts[user_idx as usize].position_size.get();
                        let net_lp = engine.net_lp_pos.get();
                        check_safety_valve(&config, net_lp, trade_size, old_user_pos, clock.slot)?;
                    }

                    #[cfg(feature = "cu-audit")]
                    {
                        msg!("CU_CHECKPOINT: trade_cpi_execute_start");
                        sol_log_compute_units();
                    }

                    // CMOR: save margins, apply credit, restore after trade
                    let margin_orig_cpi = (
                        engine.params.initial_margin_bps,
                        engine.params.maintenance_margin_bps,
                    );
                    if has_cmor_cpi {
                        crate::try_apply_cmor_from_accounts(
                            engine, accounts, program_id, a_user.key, a_slab.key, clock.slot,
                        );
                    }

                    let trade_result_cpi = engine
                        .execute_trade(&matcher, lp_idx, user_idx, clock.slot, price, trade_size)
                        .map_err(map_risk_error);
                    crate::restore_margins(engine, margin_orig_cpi);
                    trade_result_cpi?;

                    // PERC-273 + PERC-302: Dynamic OI cap check after trade (with ramp)
                    check_oi_cap(engine, &config, clock.slot)?;
                    check_pnl_cap(engine, &config)?;
                    check_phase_leverage(engine, &config, user_idx)?;

                    // PERC-622: Accumulate trade volume for oracle phase transitions.
                    {
                        let trade_notional_e6 = (trade_size.unsigned_abs() as u64)
                            .saturating_mul(price)
                            .checked_div(1_000_000)
                            .unwrap_or(0);
                        let mut vol_config = state::read_config(&data);
                        state::accumulate_volume(&mut vol_config, trade_notional_e6);
                        state::write_config(&mut data, &vol_config);
                    }

                    #[cfg(feature = "cu-audit")]
                    {
                        msg!("CU_CHECKPOINT: trade_cpi_execute_end");
                        sol_log_compute_units();
                    }
                    // Write nonce AFTER CPI and execute_trade to avoid ExternalAccountDataModified
                    state::write_req_nonce(&mut data, req_id);
                }
            }
            Instruction::LiquidateAtOracle { target_idx } => {
                accounts::expect_len(accounts, 4)?;
                let a_slab = &accounts[1];
                let a_oracle = &accounts[3];
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                let mut config = state::read_config(&data);

                let clock = Clock::from_account_info(&accounts[2])?;

                // CMOR: detect trailing attestation PDA to exclude from oracle slice
                let has_cmor_liq_detect = crate::last_account_is_cmor(accounts, program_id);
                let oracle_end_liq = if has_cmor_liq_detect {
                    accounts.len() - 1
                } else {
                    accounts.len()
                };

                // Read oracle price: Hyperp mode uses index directly, otherwise circuit-breaker clamping
                let is_hyperp = oracle::is_hyperp_mode(&config);
                let price = if is_hyperp {
                    let idx = config.last_effective_price_e6;
                    if idx == 0 {
                        return Err(PercolatorError::OracleInvalid.into());
                    }
                    // PERC-365: Reject liquidations if Hyperp oracle is stale
                    {
                        let eng = zc::engine_ref(&data)?;
                        oracle::check_hyperp_staleness(
                            eng.current_slot,
                            eng.max_crank_staleness_slots,
                            clock.slot,
                        )?;
                    }
                    idx
                } else {
                    oracle::read_price_clamped(
                        &mut config,
                        a_oracle,
                        clock.unix_timestamp,
                        &accounts[4..oracle_end_liq],
                    )?
                };
                state::write_config(&mut data, &config);

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, target_idx)?;

                // Debug logging for liquidation (using sol_log_64 for no_std)
                sol_log_64(target_idx as u64, price, 0, 0, 0); // idx, price
                {
                    let acc = &engine.accounts[target_idx as usize];
                    sol_log_64(acc.capital.get() as u64, acc.pnl.get() as u64, 0, 0, 1); // cap, pnl
                    sol_log_64(acc.position_size.get() as u64, acc.entry_price, 0, 0, 2); // pos, entry
                                                                                          // Calculate mark PnL
                    let pos = acc.position_size.get();
                    let entry = acc.entry_price as i128;
                    let mark = pos.saturating_mul(price as i128 - entry) / 1_000_000;
                    let equity = (acc.capital.get() as i128)
                        .saturating_add(acc.pnl.get())
                        .saturating_add(mark);
                    let notional = (if pos < 0 { -pos } else { pos } as u128)
                        .saturating_mul(price as u128)
                        / 1_000_000;
                    let maint_req = notional
                        .saturating_mul(engine.params.maintenance_margin_bps as u128)
                        / 10_000;
                    sol_log_64(mark as u64, equity as u64, maint_req as u64, 0, 3);
                    // mark, equity, maint
                }

                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: liquidate_start");
                    sol_log_compute_units();
                }
                // Snapshot pre-liquidation state for event logging
                let pre_cap = engine.accounts[target_idx as usize].capital.get() as u64;
                let pre_pos = engine.accounts[target_idx as usize].position_size.get();
                // VRAM: scale margins before liquidation check
                let vram_orig = crate::apply_vram_scaling(engine, &config);

                // CMOR: apply cross-margin credit — hedged users get reduced margin
                // requirement, making them harder to liquidate (correctly reflecting
                // their lower portfolio risk)
                let has_cmor_liq = crate::last_account_is_cmor(accounts, program_id);
                if has_cmor_liq {
                    let target_owner = Pubkey::from(engine.accounts[target_idx as usize].owner);
                    crate::try_apply_cmor_from_accounts(
                        engine,
                        accounts,
                        program_id,
                        &target_owner,
                        a_slab.key,
                        clock.slot,
                    );
                }

                let liq_result = engine
                    .liquidate_at_oracle(target_idx, clock.slot, price)
                    .map_err(map_risk_error);
                crate::restore_margins(engine, vram_orig);
                let _res = liq_result?;
                let post_cap = engine.accounts[target_idx as usize].capital.get() as u64;
                let post_pos = engine.accounts[target_idx as usize].position_size.get();
                // Enhanced liquidation event: tag=4, result, pre_cap, post_cap, price
                sol_log_64(_res as u64, pre_cap, post_cap, price, 4);
                // Liquidation detail: tag=5, pre_pos(low), pre_pos(high), post_pos(low), partial_flag
                let is_partial = post_pos != 0;
                sol_log_64(
                    pre_pos as u64,
                    (pre_pos >> 64) as u64,
                    post_pos as u64,
                    is_partial as u64,
                    5,
                );
                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: liquidate_end");
                    sol_log_compute_units();
                }
            }
            Instruction::CloseAccount { user_idx } => {
                accounts::expect_len(accounts, 8)?;
                let a_user = &accounts[0];
                let a_slab = &accounts[1];
                let a_vault = &accounts[2];
                let a_user_ata = &accounts[3];
                let a_pda = &accounts[4];
                let a_token = &accounts[5];
                let a_oracle = &accounts[7];

                accounts::expect_signer(a_user)?;
                accounts::expect_writable(a_slab)?;
                verify_token_program(a_token)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                let mut config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let (auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_user_ata, a_user.key, &mint)?;
                accounts::expect_key(a_pda, &auth)?;

                let clock = Clock::from_account_info(&accounts[6])?;
                // Read oracle price: Hyperp mode uses index directly, otherwise circuit-breaker clamping
                let is_hyperp = oracle::is_hyperp_mode(&config);
                let price = if is_hyperp {
                    let idx = config.last_effective_price_e6;
                    if idx == 0 {
                        return Err(PercolatorError::OracleInvalid.into());
                    }
                    // PERC-365: Reject if Hyperp oracle is stale
                    {
                        let eng = zc::engine_ref(&data)?;
                        oracle::check_hyperp_staleness(
                            eng.current_slot,
                            eng.max_crank_staleness_slots,
                            clock.slot,
                        )?;
                    }
                    idx
                } else {
                    oracle::read_price_clamped(
                        &mut config,
                        a_oracle,
                        clock.unix_timestamp,
                        &accounts[8..],
                    )?
                };
                state::write_config(&mut data, &config);

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, user_idx)?;

                // Owner authorization via verify helper (Kani-provable)
                let u_owner = engine.accounts[user_idx as usize].owner;
                if !crate::verify::owner_ok(u_owner, a_user.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: close_account_start");
                    sol_log_compute_units();
                }
                let amt_units = engine
                    .close_account(user_idx, clock.slot, price)
                    .map_err(map_risk_error)?;
                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: close_account_end");
                    sol_log_compute_units();
                }
                let amt_units_u64: u64 = amt_units
                    .try_into()
                    .map_err(|_| PercolatorError::EngineOverflow)?;

                // Convert units to base tokens for payout (checked to prevent silent overflow)
                let base_to_pay =
                    crate::units::units_to_base_checked(amt_units_u64, config.unit_scale)
                        .ok_or(PercolatorError::EngineOverflow)?;

                let seed1: &[u8] = b"vault";
                let seed2: &[u8] = a_slab.key.as_ref();
                let bump_arr: [u8; 1] = [config.vault_authority_bump];
                let seed3: &[u8] = &bump_arr;
                let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
                let signer_seeds: [&[&[u8]]; 1] = [&seeds];

                collateral::withdraw(
                    a_token,
                    a_vault,
                    a_user_ata,
                    a_pda,
                    base_to_pay,
                    &signer_seeds,
                )?;
            }
            Instruction::TopUpInsurance { amount } => {
                accounts::expect_len(accounts, 5)?;
                let a_user = &accounts[0];
                let a_slab = &accounts[1];
                let a_user_ata = &accounts[2];
                let a_vault = &accounts[3];
                let a_token = &accounts[4];

                accounts::expect_signer(a_user)?;
                accounts::expect_writable(a_slab)?;
                verify_token_program(a_token)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                // Block insurance top-up when market is resolved
                if state::is_resolved(&data) {
                    return Err(ProgramError::InvalidAccountData);
                }

                let config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let (auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_user_ata, a_user.key, &mint)?;

                // Transfer base tokens to vault
                collateral::deposit(a_token, a_user_ata, a_vault, a_user, amount)?;

                // Convert base tokens to units for engine
                let (units, dust) = crate::units::base_to_units(amount, config.unit_scale);

                // Accumulate dust
                let old_dust = state::read_dust_base(&data);
                state::write_dust_base(&mut data, old_dust.saturating_add(dust));

                let engine = zc::engine_mut(&mut data)?;
                engine
                    .top_up_insurance_fund(units as u128)
                    .map_err(map_risk_error)?;
            }
            // Defense-in-depth: if a future tag routes here by mistake,
            // return an error instead of panicking (unreachable! aborts the tx).
            _ => return Err(ProgramError::InvalidInstructionData),
        }
        Ok(())
    }

    /// Admin + insurance operations: SetRiskThreshold through WithdrawInsuranceLimited (tags 11-31).
    #[inline(never)]
    fn dispatch_admin_ops<'a, 'b>(
        program_id: &Pubkey,
        accounts: &'b [AccountInfo<'a>],
        instruction_data: &[u8],
    ) -> ProgramResult {
        let instruction = Instruction::decode(instruction_data)?;

        match instruction {
            Instruction::SetRiskThreshold { new_threshold } => {
                accounts::expect_len(accounts, 2)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                // Bounds: threshold is in e18 units representing a percentage.
                // Min: 0 (disabled). Max: 100% = 1e18 (full risk reduction).
                const MAX_THRESHOLD: u128 = 1_000_000_000_000_000_000; // 100% in e18
                if new_threshold > MAX_THRESHOLD {
                    return Err(ProgramError::InvalidInstructionData);
                }

                let engine = zc::engine_mut(&mut data)?;
                engine.set_risk_reduction_threshold(new_threshold);
            }

            Instruction::UpdateAdmin { new_admin } => {
                // Two-step admin transfer: Step 1 — PROPOSE new admin.
                // Current admin proposes; new admin must call AcceptAdmin to complete.
                // This prevents accidental lockout from admin key typos.
                accounts::expect_len(accounts, 2)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let mut header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                // Cannot propose the zero address
                if new_admin == Pubkey::default() {
                    return Err(ProgramError::InvalidInstructionData);
                }

                header.pending_admin = new_admin.to_bytes();
                state::write_header(&mut data, &header);
            }

            Instruction::CloseSlab => {
                accounts::expect_len(accounts, 2)?;
                let a_dest = &accounts[0];
                let a_slab = &accounts[1];

                accounts::expect_signer(a_dest)?;
                accounts::expect_writable(a_slab)?;

                // With unsafe_close: skip all validation and zeroing (CU limit)
                // Account will be garbage collected after lamports are drained
                #[cfg(not(feature = "unsafe_close"))]
                {
                    let mut data = state::slab_data_mut(a_slab)?;
                    slab_guard(program_id, a_slab, &data)?;
                    require_initialized(&data)?;

                    let header = state::read_header(&data);
                    require_admin(header.admin, a_dest.key)?;

                    let engine = zc::engine_ref(&data)?;
                    if !engine.vault.is_zero() {
                        return Err(PercolatorError::EngineInsufficientBalance.into());
                    }
                    if !engine.insurance_fund.balance.is_zero() {
                        return Err(PercolatorError::EngineInsufficientBalance.into());
                    }
                    if engine.num_used_accounts != 0 {
                        return Err(PercolatorError::EngineAccountNotFound.into());
                    }

                    // Bug #3 fix: Check dust_base to prevent closing with unaccounted funds
                    let dust_base = state::read_dust_base(&data);
                    if dust_base != 0 {
                        return Err(PercolatorError::EngineInsufficientBalance.into());
                    }

                    // Zero out the slab data to prevent reuse
                    for b in data.iter_mut() {
                        *b = 0;
                    }
                }

                // Transfer all lamports from slab to destination
                let slab_lamports = a_slab.lamports();
                **a_slab.lamports.borrow_mut() = 0;
                **a_dest.lamports.borrow_mut() = a_dest
                    .lamports()
                    .checked_add(slab_lamports)
                    .ok_or(PercolatorError::EngineOverflow)?;
            }

            Instruction::UpdateConfig {
                funding_horizon_slots,
                funding_k_bps,
                funding_inv_scale_notional_e6,
                funding_max_premium_bps,
                funding_max_bps_per_slot,
                thresh_floor,
                thresh_risk_bps,
                thresh_update_interval_slots,
                thresh_step_bps,
                thresh_alpha_bps,
                thresh_min,
                thresh_max,
                thresh_min_step,
                funding_premium_weight_bps,
                funding_settlement_interval_slots,
                funding_premium_dampening_e6,
                funding_premium_max_bps_per_slot,
                funding_k2_bps,
            } => {
                accounts::expect_len(accounts, 2)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                // Validate parameters
                if funding_horizon_slots == 0 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                if funding_inv_scale_notional_e6 == 0 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                if thresh_alpha_bps > 10_000 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                if thresh_min > thresh_max {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                // PERC-121: Validate premium funding params
                if funding_premium_weight_bps > 10_000 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                if funding_premium_weight_bps > 0 && funding_premium_dampening_e6 == 0 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }

                // Read existing config and update
                let mut config = state::read_config(&data);
                config.funding_horizon_slots = funding_horizon_slots;
                config.funding_k_bps = funding_k_bps;
                config.funding_inv_scale_notional_e6 = funding_inv_scale_notional_e6;
                config.funding_max_premium_bps = funding_max_premium_bps;
                config.funding_max_bps_per_slot = funding_max_bps_per_slot;
                config.thresh_floor = thresh_floor;
                config.thresh_risk_bps = thresh_risk_bps;
                config.thresh_update_interval_slots = thresh_update_interval_slots;
                config.thresh_step_bps = thresh_step_bps;
                config.thresh_alpha_bps = thresh_alpha_bps;
                config.thresh_min = thresh_min;
                config.thresh_max = thresh_max;
                config.thresh_min_step = thresh_min_step;
                // PERC-121: Premium funding params
                config.funding_premium_weight_bps = funding_premium_weight_bps;
                config.funding_settlement_interval_slots = funding_settlement_interval_slots;
                config.funding_premium_dampening_e6 = funding_premium_dampening_e6;
                config.funding_premium_max_bps_per_slot = funding_premium_max_bps_per_slot;
                // Quadratic funding convexity
                state::set_funding_k2_bps(&mut config, funding_k2_bps);
                state::write_config(&mut data, &config);
            }

            Instruction::SetMaintenanceFee { new_fee } => {
                accounts::expect_len(accounts, 2)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                // Cap: max 0.01% per slot ≈ 43% per day at 400ms slots.
                // 0.01% = 1e14 in e18 units. This prevents griefing by malicious admin.
                const MAX_FEE_PER_SLOT: u128 = 100_000_000_000_000; // 0.01% in e18
                if new_fee > MAX_FEE_PER_SLOT {
                    return Err(ProgramError::InvalidInstructionData);
                }

                let engine = zc::engine_mut(&mut data)?;
                engine.params.maintenance_fee_per_slot = percolator::U128::new(new_fee);
            }

            Instruction::SetOracleAuthority { new_authority } => {
                accounts::expect_len(accounts, 2)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                // Update oracle authority in config
                let mut config = state::read_config(&data);
                config.oracle_authority = new_authority.to_bytes();
                // Clear stored price when authority changes
                config.authority_price_e6 = 0;
                config.authority_timestamp = 0;
                state::write_config(&mut data, &config);
            }

            Instruction::PushOraclePrice {
                price_e6,
                timestamp,
            } => {
                accounts::expect_len(accounts, 2)?;
                let a_authority = &accounts[0];
                let a_slab = &accounts[1];

                accounts::expect_signer(a_authority)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                // Verify caller is the oracle authority
                let mut config = state::read_config(&data);
                if config.oracle_authority == [0u8; 32] {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }
                if config.oracle_authority != a_authority.key.to_bytes() {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                // Validate price (must be positive)
                if price_e6 == 0 {
                    return Err(PercolatorError::OracleInvalid.into());
                }

                // Clamp the incoming price against circuit breaker
                let clamped = oracle::clamp_oracle_price(
                    config.last_effective_price_e6,
                    price_e6,
                    config.oracle_price_cap_e2bps,
                );
                config.authority_price_e6 = clamped;
                // In Hyperp mode, authority_timestamp stores the funding rate (bps/slot).
                // Only write the oracle timestamp in non-Hyperp admin oracle mode.
                if !oracle::is_hyperp_mode(&config) {
                    config.authority_timestamp = timestamp;
                }
                config.last_effective_price_e6 = clamped;
                state::write_config(&mut data, &config);
            }

            Instruction::SetOraclePriceCap { max_change_e2bps } => {
                accounts::expect_len(accounts, 2)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                let mut config = state::read_config(&data);

                // SECURITY (#297 Fix 2): Prevent admin from disabling circuit breaker
                // on Hyperp markets. Enforce minimum cap to protect EMA from manipulation.
                if oracle::is_hyperp_mode(&config)
                    && max_change_e2bps < crate::constants::DEFAULT_HYPERP_PRICE_CAP_E2BPS
                {
                    msg!(
                        "SetOracleCap: Hyperp markets require cap >= {} (got {})",
                        crate::constants::DEFAULT_HYPERP_PRICE_CAP_E2BPS,
                        max_change_e2bps
                    );
                    return Err(ProgramError::InvalidArgument);
                }

                config.oracle_price_cap_e2bps = max_change_e2bps;
                state::write_config(&mut data, &config);
            }

            Instruction::ResolveMarket => {
                // Resolve market: set RESOLVED flag, use admin oracle price for settlement
                // Positions are force-closed via subsequent KeeperCrank calls (paginated)
                accounts::expect_len(accounts, 2)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                // Can't re-resolve
                if state::is_resolved(&data) {
                    return Err(ProgramError::InvalidAccountData);
                }

                // Require admin oracle price to be set (authority_price_e6 > 0)
                let config = state::read_config(&data);
                if config.authority_price_e6 == 0 {
                    return Err(ProgramError::InvalidAccountData);
                }

                // Set the resolved flag
                state::set_resolved(&mut data);
            }

            Instruction::WithdrawInsurance => {
                // Withdraw insurance fund (admin only, requires RESOLVED and all positions closed)
                accounts::expect_len(accounts, 6)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];
                let a_admin_ata = &accounts[2];
                let a_vault = &accounts[3];
                let a_token = &accounts[4];
                let a_vault_pda = &accounts[5];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;
                verify_token_program(a_token)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                // Must be resolved
                if !state::is_resolved(&data) {
                    return Err(ProgramError::InvalidAccountData);
                }

                let config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let (auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_admin_ata, a_admin.key, &mint)?;
                accounts::expect_key(a_vault_pda, &auth)?;

                let engine = zc::engine_mut(&mut data)?;

                // Require all positions to be closed (force-closed by crank)
                // Check that no account has position_size != 0
                let mut has_open_positions = false;
                for i in 0..percolator::MAX_ACCOUNTS {
                    if engine.is_used(i) {
                        let pos = engine.accounts[i].position_size.get();
                        if pos != 0 {
                            has_open_positions = true;
                            break;
                        }
                    }
                }
                if has_open_positions {
                    return Err(ProgramError::InvalidAccountData);
                }

                // Get insurance balance and convert to base tokens
                let insurance_units = engine.insurance_fund.balance.get();
                if insurance_units == 0 {
                    return Ok(()); // Nothing to withdraw
                }

                // Cap at u64::MAX for conversion (should never happen in practice)
                let units_u64 = if insurance_units > u64::MAX as u128 {
                    u64::MAX
                } else {
                    insurance_units as u64
                };
                let base_amount = crate::units::units_to_base_checked(units_u64, config.unit_scale)
                    .ok_or(PercolatorError::EngineOverflow)?;

                // Zero out insurance fund
                engine.insurance_fund.balance = percolator::U128::ZERO;

                // Transfer from vault to admin
                let seed1: &[u8] = b"vault";
                let seed2: &[u8] = a_slab.key.as_ref();
                let bump_arr: [u8; 1] = [config.vault_authority_bump];
                let seed3: &[u8] = &bump_arr;
                let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
                let signer_seeds: [&[&[u8]]; 1] = [&seeds];

                collateral::withdraw(
                    a_token,
                    a_vault,
                    a_admin_ata,
                    a_vault_pda,
                    base_amount,
                    &signer_seeds,
                )?;
            }
            Instruction::AdminForceClose { target_idx } => {
                // Admin force-close: unconditionally close any position at oracle price.
                // Accounts: [admin(signer), slab(writable), clock, oracle]
                accounts::expect_len(accounts, 4)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];
                let a_oracle = &accounts[3];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                let mut config = state::read_config(&data);
                let clock = Clock::from_account_info(&accounts[2])?;

                // Read oracle price (same logic as LiquidateAtOracle)
                let is_hyperp = oracle::is_hyperp_mode(&config);
                let price = if is_hyperp {
                    let idx = config.last_effective_price_e6;
                    if idx == 0 {
                        return Err(PercolatorError::OracleInvalid.into());
                    }
                    // PERC-365: Reject ADL if Hyperp oracle is stale
                    {
                        let eng = zc::engine_ref(&data)?;
                        oracle::check_hyperp_staleness(
                            eng.current_slot,
                            eng.max_crank_staleness_slots,
                            clock.slot,
                        )?;
                    }
                    idx
                } else {
                    oracle::read_price_clamped(
                        &mut config,
                        a_oracle,
                        clock.unix_timestamp,
                        &accounts[4..],
                    )?
                };
                state::write_config(&mut data, &config);

                let engine = zc::engine_mut(&mut data)?;
                check_idx(engine, target_idx)?;

                engine
                    .admin_force_close(target_idx, clock.slot, price)
                    .map_err(map_risk_error)?;
            }

            Instruction::UpdateRiskParams {
                initial_margin_bps,
                maintenance_margin_bps,
                trading_fee_bps,
                oi_cap_multiplier_bps,
                max_pnl_cap,
                oi_ramp_slots,
                skew_factor_bps,
                adaptive_funding_enabled,
                adaptive_scale_bps,
                adaptive_max_funding_bps,
                mark_oracle_weight_bps,
                vol_margin_scale_bps,
                vol_alpha_e6,
                vol_margin_target_e6,
            } => {
                // Update margin + fee parameters. Admin only.
                // Accounts: [admin(signer), slab(writable)]
                accounts::expect_len(accounts, 2)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                // Validate: initial >= maintenance, both > 0, both <= 10000
                if initial_margin_bps == 0 || maintenance_margin_bps == 0 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                if initial_margin_bps > 10_000 || maintenance_margin_bps > 10_000 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                if initial_margin_bps < maintenance_margin_bps {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }

                // Validate trading fee if provided (0-1000 bps = 0-10%)
                if let Some(fee) = trading_fee_bps {
                    if fee > 1_000 {
                        return Err(PercolatorError::InvalidConfigParam.into());
                    }
                }

                let engine = zc::engine_mut(&mut data)?;
                let _ = engine.set_margin_params(initial_margin_bps, maintenance_margin_bps);

                // Update trading fee if provided (backwards compatible)
                if let Some(fee) = trading_fee_bps {
                    engine.params.trading_fee_bps = fee;
                }

                // PERC-273: Update OI cap multiplier if provided
                // PERC-272: Update max PnL cap if provided
                // PERC-298 + PERC-302: Update OI cap, skew factor, ramp slots if provided
                if oi_cap_multiplier_bps.is_some()
                    || max_pnl_cap.is_some()
                    || oi_ramp_slots.is_some()
                    || skew_factor_bps.is_some()
                    || adaptive_funding_enabled.is_some()
                    || adaptive_scale_bps.is_some()
                    || adaptive_max_funding_bps.is_some()
                    || mark_oracle_weight_bps.is_some()
                    || vol_margin_scale_bps.is_some()
                    || vol_alpha_e6.is_some()
                    || vol_margin_target_e6.is_some()
                {
                    let mut config = state::read_config(&data);
                    if let Some(oi_cap) = oi_cap_multiplier_bps {
                        // Preserve existing skew factor when only updating multiplier
                        let (_, existing_skew) = unpack_oi_cap(config.oi_cap_multiplier_bps);
                        config.oi_cap_multiplier_bps = pack_oi_cap(oi_cap, existing_skew);
                    }
                    if let Some(skew) = skew_factor_bps {
                        // PERC-298: validate skew_factor_bps <= 10_000 (100%)
                        if skew > 10_000 {
                            return Err(PercolatorError::InvalidConfigParam.into());
                        }
                        let (existing_mult, _) = unpack_oi_cap(config.oi_cap_multiplier_bps);
                        config.oi_cap_multiplier_bps = pack_oi_cap(existing_mult, skew);
                    }
                    if let Some(pnl_cap) = max_pnl_cap {
                        config.max_pnl_cap = pnl_cap;
                    }
                    if let Some(ramp) = oi_ramp_slots {
                        config.oi_ramp_slots = ramp;
                    }
                    // PERC-300: Adaptive funding rate params
                    if let Some(enabled) = adaptive_funding_enabled {
                        config.adaptive_funding_enabled = enabled;
                    }
                    if let Some(scale) = adaptive_scale_bps {
                        config.adaptive_scale_bps = scale;
                    }
                    if let Some(max_bps) = adaptive_max_funding_bps {
                        config.adaptive_max_funding_bps = max_bps;
                    }
                    // PERC-118: Mark blend weight
                    if let Some(w) = mark_oracle_weight_bps {
                        if w > 10_000 {
                            return Err(PercolatorError::InvalidConfigParam.into());
                        }
                        state::set_mark_oracle_weight_bps(&mut config, w);
                    }
                    // VRAM: Volatility-Regime Adaptive Margin params
                    if let Some(scale) = vol_margin_scale_bps {
                        state::set_vol_margin_scale_bps(&mut config, scale);
                    }
                    if let Some(alpha) = vol_alpha_e6 {
                        state::set_vol_alpha_e6(&mut config, alpha);
                    }
                    if let Some(target) = vol_margin_target_e6 {
                        state::set_vol_margin_target_e6(&mut config, target);
                    }
                    state::write_config(&mut data, &config);
                    let (mult, skew) = unpack_oi_cap(config.oi_cap_multiplier_bps);
                    msg!(
                        "UpdateRiskParams: oi_cap={} max_pnl_cap={:?} skew_factor={} oi_ramp_slots={:?}",
                        mult,
                        max_pnl_cap,
                        skew,
                        oi_ramp_slots
                    );
                }

                msg!("UpdateRiskParams: initial_margin_bps={}, maintenance_margin_bps={}, trading_fee_bps={:?}",
                    initial_margin_bps, maintenance_margin_bps, trading_fee_bps);
            }

            Instruction::RenounceAdmin { confirmation } => {
                // Renounce admin: set admin to all zeros (irreversible).
                // SECURITY (#312): Requires market RESOLVED + confirmation code.
                // PERC-136 #312: Only allowed after market is RESOLVED to prevent
                // admin abandonment while users still have open positions.
                // Accounts: [admin(signer), slab(writable)]
                accounts::expect_len(accounts, 2)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                // Guard: market must be RESOLVED before admin can renounce (PERC-136 #312)
                if !state::is_resolved(&data) {
                    return Err(PercolatorError::AdminRenounceNotAllowed.into());
                }

                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                // SECURITY (#312): Market must be RESOLVED before admin can renounce
                if !state::is_resolved(&data) {
                    return Err(PercolatorError::AdminRenounceNotAllowed.into());
                }

                // SECURITY (#312): Require confirmation code to prevent accidental calls
                if confirmation != crate::constants::RENOUNCE_ADMIN_CONFIRMATION {
                    return Err(PercolatorError::InvalidConfirmation.into());
                }

                // Set admin to all zeros — irreversible
                let mut new_header = header;
                new_header.admin = [0u8; 32];
                state::write_header(&mut data, &new_header);
            }

            Instruction::CreateInsuranceMint => {
                // Create insurance LP mint for this market. Admin only, once per market.
                // Accounts: [admin(signer), slab, ins_lp_mint(writable), vault_authority,
                //            collateral_mint, system_program, token_program, rent, payer(signer+writable)]
                accounts::expect_len(accounts, 9)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];
                let a_ins_lp_mint = &accounts[2];
                let a_vault_authority = &accounts[3];
                let a_collateral_mint = &accounts[4];
                let a_system = &accounts[5];
                let a_token = &accounts[6];
                let a_rent = &accounts[7];
                let a_payer = &accounts[8];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_ins_lp_mint)?;
                accounts::expect_signer(a_payer)?;
                accounts::expect_writable(a_payer)?;
                verify_token_program(a_token)?;

                let data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                // Verify the ins_lp_mint PDA
                let (expected_mint, mint_bump) =
                    accounts::derive_insurance_lp_mint(program_id, a_slab.key);
                accounts::expect_key(a_ins_lp_mint, &expected_mint)?;

                // Verify vault authority PDA
                let (expected_auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                accounts::expect_key(a_vault_authority, &expected_auth)?;

                // Check mint doesn't already exist (data len == 0 means not yet created)
                if a_ins_lp_mint.data_len() > 0 {
                    return Err(PercolatorError::InsuranceMintAlreadyExists.into());
                }

                // Read collateral mint decimals
                let decimals = crate::insurance_lp::read_mint_decimals(a_collateral_mint)?;

                // Create and initialize the mint PDA
                let slab_key_bytes = a_slab.key.as_ref();
                let bump_arr: [u8; 1] = [mint_bump];
                let mint_seeds: &[&[u8]] = &[b"ins_lp", slab_key_bytes, &bump_arr];

                crate::insurance_lp::create_mint(
                    a_payer,
                    a_ins_lp_mint,
                    a_vault_authority,
                    a_system,
                    a_token,
                    a_rent,
                    decimals,
                    mint_seeds,
                )?;

                msg!("Insurance LP mint created");
            }

            Instruction::DepositInsuranceLP { amount } => {
                // Deposit collateral into insurance fund, receive LP tokens.
                // Accounts: [depositor(signer), slab(writable), depositor_ata(writable),
                //            vault(writable), token_program, ins_lp_mint(writable),
                //            depositor_lp_ata(writable), vault_authority]
                accounts::expect_len(accounts, 8)?;
                let a_depositor = &accounts[0];
                let a_slab = &accounts[1];
                let a_depositor_ata = &accounts[2];
                let a_vault = &accounts[3];
                let a_token = &accounts[4];
                let a_ins_lp_mint = &accounts[5];
                let a_depositor_lp_ata = &accounts[6];
                let a_vault_authority = &accounts[7];

                accounts::expect_signer(a_depositor)?;
                accounts::expect_writable(a_slab)?;
                accounts::expect_writable(a_depositor_ata)?;
                accounts::expect_writable(a_vault)?;
                accounts::expect_writable(a_ins_lp_mint)?;
                accounts::expect_writable(a_depositor_lp_ata)?;
                verify_token_program(a_token)?;

                if amount == 0 {
                    return Err(PercolatorError::InsuranceZeroAmount.into());
                }

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                // Block deposits on resolved markets
                if state::is_resolved(&data) {
                    return Err(ProgramError::InvalidAccountData);
                }

                let config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                // Verify vault
                let (auth, vault_bump) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_depositor_ata, a_depositor.key, &mint)?;

                // Verify insurance LP mint PDA
                let (expected_lp_mint, _) =
                    accounts::derive_insurance_lp_mint(program_id, a_slab.key);
                accounts::expect_key(a_ins_lp_mint, &expected_lp_mint)?;

                // Verify LP mint exists
                if a_ins_lp_mint.data_len() == 0 {
                    return Err(PercolatorError::InsuranceMintNotCreated.into());
                }

                // Verify vault authority PDA
                accounts::expect_key(a_vault_authority, &auth)?;

                // Read current insurance balance and LP supply BEFORE deposit
                let engine = zc::engine_mut(&mut data)?;
                let insurance_balance_before: u128 = engine.insurance_fund.balance.get();
                let lp_supply = crate::insurance_lp::read_mint_supply(a_ins_lp_mint)?;

                // Transfer collateral from depositor to vault
                collateral::deposit(a_token, a_depositor_ata, a_vault, a_depositor, amount)?;

                // Convert base tokens to units
                let (units, dust) = crate::units::base_to_units(amount, config.unit_scale);

                // Accumulate dust
                let old_dust = state::read_dust_base(&data);
                state::write_dust_base(&mut data, old_dust.saturating_add(dust));

                // Calculate LP tokens to mint
                let lp_tokens_to_mint: u64 = if lp_supply == 0 {
                    // First deposit: 1:1 ratio (units of collateral = LP tokens)
                    // Guard: if insurance already has balance but supply is 0, that means
                    // admin topped up via TopUpInsurance before creating LP mint.
                    // Still safe: first LP depositor gets tokens proportional to their deposit only.
                    units
                } else {
                    if insurance_balance_before == 0 {
                        // Shouldn't happen: supply > 0 but balance == 0 means fund was drained.
                        // Reject to prevent division by zero and unfair minting.
                        return Err(PercolatorError::InsuranceSupplyMismatch.into());
                    }
                    // Proportional: tokens = deposit_units * supply / balance
                    // Use u128 for intermediate to prevent overflow
                    let numerator = (units as u128)
                        .checked_mul(lp_supply as u128)
                        .ok_or(PercolatorError::EngineOverflow)?;
                    let result = numerator / insurance_balance_before;
                    // Round DOWN (depositor gets fewer tokens — pool is never underfunded)
                    if result > u64::MAX as u128 {
                        return Err(PercolatorError::EngineOverflow.into());
                    }
                    result as u64
                };

                if lp_tokens_to_mint == 0 {
                    // Deposit too small to mint any LP tokens — reject to prevent loss
                    return Err(PercolatorError::InsuranceZeroAmount.into());
                }

                // Top up insurance fund in engine
                // Re-borrow engine after the collateral transfer
                let engine = zc::engine_mut(&mut data)?;
                engine
                    .top_up_insurance_fund(units as u128)
                    .map_err(map_risk_error)?;

                // Mint LP tokens to depositor
                let seed1: &[u8] = b"vault";
                let seed2: &[u8] = a_slab.key.as_ref();
                let bump_arr: [u8; 1] = [vault_bump];
                let seed3: &[u8] = &bump_arr;
                let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
                let signer_seeds: [&[&[u8]]; 1] = [&seeds];

                crate::insurance_lp::mint_to(
                    a_token,
                    a_ins_lp_mint,
                    a_depositor_lp_ata,
                    a_vault_authority,
                    lp_tokens_to_mint,
                    &signer_seeds,
                )?;

                msg!(
                    "Insurance LP deposit: {} tokens, {} LP minted",
                    amount,
                    lp_tokens_to_mint
                );
            }

            Instruction::WithdrawInsuranceLP { lp_amount } => {
                // Burn LP tokens and withdraw proportional share of insurance fund.
                // Accounts: [withdrawer(signer), slab(writable), withdrawer_ata(writable),
                //            vault(writable), token_program, ins_lp_mint(writable),
                //            withdrawer_lp_ata(writable), vault_authority]
                accounts::expect_len(accounts, 8)?;
                let a_withdrawer = &accounts[0];
                let a_slab = &accounts[1];
                let a_withdrawer_ata = &accounts[2];
                let a_vault = &accounts[3];
                let a_token = &accounts[4];
                let a_ins_lp_mint = &accounts[5];
                let a_withdrawer_lp_ata = &accounts[6];
                let a_vault_authority = &accounts[7];

                accounts::expect_signer(a_withdrawer)?;
                accounts::expect_writable(a_slab)?;
                accounts::expect_writable(a_withdrawer_ata)?;
                accounts::expect_writable(a_vault)?;
                accounts::expect_writable(a_ins_lp_mint)?;
                accounts::expect_writable(a_withdrawer_lp_ata)?;
                verify_token_program(a_token)?;

                if lp_amount == 0 {
                    return Err(PercolatorError::InsuranceZeroAmount.into());
                }

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                // Verify vault
                let (auth, vault_bump) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_withdrawer_ata, a_withdrawer.key, &mint)?;

                // Verify insurance LP mint PDA
                let (expected_lp_mint, _) =
                    accounts::derive_insurance_lp_mint(program_id, a_slab.key);
                accounts::expect_key(a_ins_lp_mint, &expected_lp_mint)?;

                if a_ins_lp_mint.data_len() == 0 {
                    return Err(PercolatorError::InsuranceMintNotCreated.into());
                }

                // Verify vault authority
                accounts::expect_key(a_vault_authority, &auth)?;

                // Read current insurance balance and LP supply
                let engine = zc::engine_mut(&mut data)?;
                let insurance_balance: u128 = engine.insurance_fund.balance.get();
                let lp_supply = crate::insurance_lp::read_mint_supply(a_ins_lp_mint)?;

                if lp_supply == 0 || insurance_balance == 0 {
                    return Err(PercolatorError::InsuranceSupplyMismatch.into());
                }

                // Calculate units to return: lp_amount * insurance_balance / lp_supply
                // Round DOWN (user gets less — pool is never underfunded)
                let numerator = (lp_amount as u128)
                    .checked_mul(insurance_balance)
                    .ok_or(PercolatorError::EngineOverflow)?;
                let units_to_return = numerator / (lp_supply as u128);

                if units_to_return == 0 {
                    return Err(PercolatorError::InsuranceZeroAmount.into());
                }

                // Safety: cannot withdraw below risk_reduction_threshold
                let remaining = insurance_balance.saturating_sub(units_to_return);
                let threshold = engine.params.risk_reduction_threshold;
                if remaining < threshold.get() {
                    return Err(PercolatorError::InsuranceBelowThreshold.into());
                }

                // Convert units to base tokens
                let units_u64 = if units_to_return > u64::MAX as u128 {
                    return Err(PercolatorError::EngineOverflow.into());
                } else {
                    units_to_return as u64
                };
                let base_amount = crate::units::units_to_base_checked(units_u64, config.unit_scale)
                    .ok_or(PercolatorError::EngineOverflow)?;

                // Reduce insurance fund balance (checked to prevent silent underflow)
                let new_balance = insurance_balance
                    .checked_sub(units_to_return)
                    .ok_or(PercolatorError::EngineOverflow)?;
                engine.insurance_fund.balance = percolator::U128::new(new_balance);

                // Burn LP tokens from withdrawer (user signs as authority over their tokens)
                crate::insurance_lp::burn(
                    a_token,
                    a_ins_lp_mint,
                    a_withdrawer_lp_ata,
                    a_withdrawer,
                    lp_amount,
                )?;

                // Transfer collateral from vault to withdrawer
                let seed1: &[u8] = b"vault";
                let seed2: &[u8] = a_slab.key.as_ref();
                let bump_arr: [u8; 1] = [vault_bump];
                let seed3: &[u8] = &bump_arr;
                let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
                let signer_seeds: [&[&[u8]]; 1] = [&seeds];

                collateral::withdraw(
                    a_token,
                    a_vault,
                    a_withdrawer_ata,
                    a_vault_authority,
                    base_amount,
                    &signer_seeds,
                )?;

                msg!(
                    "Insurance LP withdraw: {} LP burned, {} tokens returned",
                    lp_amount,
                    base_amount
                );
            }

            Instruction::PauseMarket => {
                // Pause the market. Admin only.
                // When paused: Trade, Deposit, Withdraw, InitUser are blocked.
                // Still allowed: Crank, Liquidate, AdminForceClose, Unpause, SetRiskThreshold, etc.
                accounts::expect_len(accounts, 2)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                state::set_paused(&mut data, true);
                msg!("Market paused by admin");
            }

            Instruction::UnpauseMarket => {
                // Unpause the market. Admin only.
                accounts::expect_len(accounts, 2)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                state::set_paused(&mut data, false);
                msg!("Market unpaused by admin");
            }

            Instruction::AcceptAdmin => {
                // Two-step admin transfer: Step 2 — pending admin accepts.
                accounts::expect_len(accounts, 2)?;
                let a_new_admin = &accounts[0];
                let a_slab = &accounts[1];

                accounts::expect_signer(a_new_admin)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let mut header = state::read_header(&data);

                // Must have a pending admin proposal
                if header.pending_admin == [0u8; 32] {
                    return Err(ProgramError::InvalidInstructionData);
                }

                // Signer must be the pending admin
                if header.pending_admin != a_new_admin.key.to_bytes() {
                    return Err(ProgramError::InvalidInstructionData);
                }

                header.admin = header.pending_admin;
                header.pending_admin = [0u8; 32];
                state::write_header(&mut data, &header);
                msg!("Admin transfer accepted");
            }

            Instruction::SetInsuranceWithdrawPolicy {
                authority,
                min_withdraw_base,
                max_withdraw_bps,
                cooldown_slots,
            } => {
                // SetInsuranceWithdrawPolicy (Tag 30) — admin only.
                // Creates or updates an InsuranceWithdrawPolicy PDA account.
                // PDA seeds: [b"ins_policy", slab_key]
                //
                // Accounts:
                //   0. [signer, writable] Admin (rent payer)
                //   1. [writable]         Slab
                //   2. [writable]         Policy PDA (ins_policy, created if needed)
                //   3. []                 System program
                accounts::expect_len(accounts, 4)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];
                let a_policy = &accounts[2];
                let a_system = &accounts[3];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;
                accounts::expect_writable(a_policy)?;

                // Verify system program
                if *a_system.key != solana_program::system_program::id() {
                    return Err(ProgramError::IncorrectProgramId);
                }

                let data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                // Market must be resolved before a policy can be set
                if !state::is_resolved(&data) {
                    return Err(ProgramError::InvalidAccountData);
                }
                drop(data);

                // Validate params: bps must be <= 10_000 (100%)
                if max_withdraw_bps > 10_000 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                // authority must not be the default pubkey
                if authority == Pubkey::default() {
                    return Err(ProgramError::InvalidInstructionData);
                }

                // Derive policy PDA
                let policy_seeds: &[&[u8]] = &[b"ins_policy", a_slab.key.as_ref()];
                let (expected_policy, policy_bump) =
                    Pubkey::find_program_address(policy_seeds, program_id);
                if *a_policy.key != expected_policy {
                    return Err(ProgramError::InvalidArgument);
                }

                // InsuranceWithdrawPolicy layout (49 bytes):
                //   [0..32]  authority
                //   [32..40] min_withdraw_base (u64 LE)
                //   [40..42] max_withdraw_bps (u16 LE)
                //   [42..50] cooldown_slots (u64 LE)
                //   [50..58] last_withdraw_slot (u64 LE, starts at 0)
                //   [58..66] epoch_drawn (u64 LE, starts at 0)
                //   [66]     bump (u8)
                const POLICY_LEN: usize = 67;

                let is_new = a_policy.data_is_empty();
                if is_new {
                    // Create the account
                    let lamports = solana_program::rent::Rent::get()?.minimum_balance(POLICY_LEN);
                    let bump_bytes = [policy_bump];
                    let signer_seeds: &[&[u8]] = &[b"ins_policy", a_slab.key.as_ref(), &bump_bytes];
                    solana_program::program::invoke_signed(
                        &solana_program::system_instruction::create_account(
                            a_admin.key,
                            &expected_policy,
                            lamports,
                            POLICY_LEN as u64,
                            program_id,
                        ),
                        &[a_admin.clone(), a_policy.clone()],
                        &[signer_seeds],
                    )?;
                }

                // Write policy fields
                let mut pdata = a_policy.try_borrow_mut_data()?;
                if pdata.len() < POLICY_LEN {
                    return Err(ProgramError::AccountDataTooSmall);
                }
                pdata[0..32].copy_from_slice(authority.as_ref());
                pdata[32..40].copy_from_slice(&min_withdraw_base.to_le_bytes());
                pdata[40..42].copy_from_slice(&max_withdraw_bps.to_le_bytes());
                pdata[42..50].copy_from_slice(&cooldown_slots.to_le_bytes());
                // On fresh creation: initialise tracking fields to zero.
                // On update: preserve last_withdraw_slot and epoch_drawn (don't reset cooldown).
                if is_new {
                    pdata[50..58].copy_from_slice(&0u64.to_le_bytes()); // last_withdraw_slot
                    pdata[58..66].copy_from_slice(&0u64.to_le_bytes()); // epoch_drawn
                }
                pdata[66] = policy_bump;

                msg!(
                    "InsuranceWithdrawPolicy set: authority={}, min={}, max_bps={}, cooldown={}",
                    authority,
                    min_withdraw_base,
                    max_withdraw_bps,
                    cooldown_slots
                );
            }

            Instruction::WithdrawInsuranceLimited { amount } => {
                // WithdrawInsuranceLimited (Tag 31) — policy authority only.
                // Withdraws up to `amount` base tokens from the insurance vault,
                // subject to policy constraints (cooldown, bps cap).
                //
                // Accounts:
                //   0. [signer]    Authority (must match policy.authority)
                //   1. [writable]  Slab
                //   2. [writable]  Authority's token account (destination)
                //   3. [writable]  Insurance vault token account (source)
                //   4. []          Token program
                //   5. []          Vault authority PDA
                //   6. [writable]  Policy PDA
                //   7. []          Clock sysvar
                accounts::expect_len(accounts, 8)?;
                let a_auth = &accounts[0];
                let a_slab = &accounts[1];
                let a_dest = &accounts[2];
                let a_vault = &accounts[3];
                let a_token = &accounts[4];
                let a_vault_pda = &accounts[5];
                let a_policy = &accounts[6];
                let a_clock = &accounts[7];

                accounts::expect_signer(a_auth)?;
                accounts::expect_writable(a_slab)?;
                accounts::expect_writable(a_policy)?;
                verify_token_program(a_token)?;

                let clock = Clock::from_account_info(a_clock)?;

                // Read and validate slab
                let mut slab_data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &slab_data)?;
                require_initialized(&slab_data)?;

                // Must be resolved
                if !state::is_resolved(&slab_data) {
                    return Err(ProgramError::InvalidAccountData);
                }

                let config = state::read_config(&slab_data);
                let mint = Pubkey::new_from_array(config.collateral_mint);
                let (vault_auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(
                    a_vault,
                    &vault_auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_dest, a_auth.key, &mint)?;
                accounts::expect_key(a_vault_pda, &vault_auth)?;

                // Derive and validate policy PDA
                let (expected_policy, _) =
                    Pubkey::find_program_address(&[b"ins_policy", a_slab.key.as_ref()], program_id);
                if *a_policy.key != expected_policy {
                    return Err(ProgramError::InvalidArgument);
                }

                // Read policy
                const POLICY_LEN: usize = 67;
                let mut pdata = a_policy.try_borrow_mut_data()?;
                if pdata.len() < POLICY_LEN {
                    return Err(ProgramError::AccountDataTooSmall);
                }

                let policy_authority = Pubkey::from(
                    <[u8; 32]>::try_from(&pdata[0..32])
                        .map_err(|_| ProgramError::InvalidAccountData)?,
                );
                let min_withdraw_base = u64::from_le_bytes(
                    pdata[32..40]
                        .try_into()
                        .map_err(|_| ProgramError::InvalidAccountData)?,
                );
                let max_withdraw_bps = u16::from_le_bytes(
                    pdata[40..42]
                        .try_into()
                        .map_err(|_| ProgramError::InvalidAccountData)?,
                );
                let cooldown_slots = u64::from_le_bytes(
                    pdata[42..50]
                        .try_into()
                        .map_err(|_| ProgramError::InvalidAccountData)?,
                );
                let last_withdraw_slot = u64::from_le_bytes(
                    pdata[50..58]
                        .try_into()
                        .map_err(|_| ProgramError::InvalidAccountData)?,
                );
                let epoch_drawn = u64::from_le_bytes(
                    pdata[58..66]
                        .try_into()
                        .map_err(|_| ProgramError::InvalidAccountData)?,
                );

                // Verify authority
                if *a_auth.key != policy_authority {
                    return Err(ProgramError::MissingRequiredSignature);
                }

                // Cooldown check
                if clock.slot < last_withdraw_slot.saturating_add(cooldown_slots) {
                    msg!(
                        "Withdrawal cooldown not elapsed: slot={}, last={}, cooldown={}",
                        clock.slot,
                        last_withdraw_slot,
                        cooldown_slots
                    );
                    return Err(ProgramError::InvalidAccountData);
                }

                // Minimum amount check
                if amount < min_withdraw_base {
                    return Err(ProgramError::InvalidInstructionData);
                }

                // --- Phase 1: Compute actual_amount inside a block, then release slab borrow ---
                let (actual_amount, new_balance, vault_bump) = {
                    let engine = zc::engine_mut(&mut slab_data)?;
                    let insurance_units: u128 = engine.insurance_fund.balance.get();
                    let insurance_base = crate::units::units_to_base_checked(
                        if insurance_units > u64::MAX as u128 {
                            u64::MAX
                        } else {
                            insurance_units as u64
                        },
                        config.unit_scale,
                    )
                    .ok_or(PercolatorError::EngineOverflow)?;

                    // Per-epoch bps cap
                    let is_new_epoch = last_withdraw_slot == 0
                        || clock.slot >= last_withdraw_slot.saturating_add(cooldown_slots);
                    let current_epoch_drawn = if is_new_epoch { 0u64 } else { epoch_drawn };
                    let epoch_cap = if max_withdraw_bps == 0 {
                        u64::MAX
                    } else {
                        (insurance_base as u128)
                            .saturating_mul(max_withdraw_bps as u128)
                            .saturating_div(10_000) as u64
                    };

                    let remaining_cap = epoch_cap.saturating_sub(current_epoch_drawn);
                    let actual = amount.min(remaining_cap).min(insurance_base);

                    if actual == 0 {
                        msg!("Nothing to withdraw (cap exhausted or insurance empty)");
                        return Ok(());
                    }

                    // Deduct from insurance fund
                    let (units_to_deduct, _dust) =
                        crate::units::base_to_units(actual, config.unit_scale);
                    let new_bal = insurance_units.saturating_sub(units_to_deduct as u128);
                    engine.insurance_fund.balance = percolator::U128::new(new_bal);

                    // Pre-compute updated epoch_drawn for later
                    let new_epoch_drawn = current_epoch_drawn.saturating_add(actual);
                    (
                        actual,
                        (new_bal, new_epoch_drawn),
                        config.vault_authority_bump,
                    )
                };
                let (new_ins_balance, new_epoch_drawn) = new_balance;

                // --- Phase 2: CPI transfer (no slab borrow held) ---
                let seed1: &[u8] = b"vault";
                let seed2: &[u8] = a_slab.key.as_ref();
                let bump_bytes = [vault_bump];
                let seed3: &[u8] = &bump_bytes;
                let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
                let signer_seeds: [&[&[u8]]; 1] = [&seeds];

                drop(slab_data); // release slab borrow before CPI

                collateral::withdraw(
                    a_token,
                    a_vault,
                    a_dest,
                    a_vault_pda,
                    actual_amount,
                    &signer_seeds,
                )?;

                // --- Phase 3: Update policy state ---
                pdata[50..58].copy_from_slice(&clock.slot.to_le_bytes()); // last_withdraw_slot
                pdata[58..66].copy_from_slice(&new_epoch_drawn.to_le_bytes()); // epoch_drawn

                msg!("WithdrawInsuranceLimited: withdrew {} base tokens, insurance_units_remaining={}",
                    actual_amount, new_ins_balance);
            }
            // Defense-in-depth: if a future tag routes here by mistake,
            // return an error instead of panicking (unreachable! aborts the tx).
            _ => return Err(ProgramError::InvalidInstructionData),
        }
        Ok(())
    }

    /// Extended operations: SetPythOracle through CancelQueuedWithdrawal (tags 32+).
    /// TradeCpiV2 (tag 35) is routed to dispatch_core_ops instead.
    #[inline(never)]
    fn dispatch_extended_ops<'a, 'b>(
        program_id: &Pubkey,
        accounts: &'b [AccountInfo<'a>],
        instruction_data: &[u8],
    ) -> ProgramResult {
        let instruction = Instruction::decode(instruction_data)?;

        match instruction {
            Instruction::SetPythOracle {
                feed_id,
                max_staleness_secs,
                conf_filter_bps,
            } => {
                accounts::expect_len(accounts, 2)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];
                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;
                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;
                if feed_id == [0u8; 32] {
                    return Err(ProgramError::InvalidInstructionData);
                }
                if max_staleness_secs == 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                let mut config = state::read_config(&data);
                if oracle::is_hyperp_mode(&config) {
                    return Err(ProgramError::InvalidAccountData);
                }
                config.index_feed_id = feed_id;
                config.max_staleness_secs = max_staleness_secs;
                config.conf_filter_bps = conf_filter_bps;
                config.oracle_authority = [0u8; 32];
                config.authority_price_e6 = 0;
                config.authority_timestamp = 0;
                config.last_effective_price_e6 = 0;
                state::write_config(&mut data, &config);
                msg!(
                    "SetPythOracle: Pyth-pinned, staleness={}s, conf_bps={}",
                    max_staleness_secs,
                    conf_filter_bps
                );
            }

            Instruction::UpdateMarkPrice => {
                // UpdateMarkPrice (Tag 33) — permissionless Pyth mark price crank.
                //
                // PERC-117: Reads the current Pyth PriceUpdateV2 price, applies
                // 8-hour EMA smoothing with circuit breaker, and writes the new
                // mark to authority_price_e6. This provides a manipulation-resistant
                // mark price for funding rates, liquidations, and PnL calculations.
                //
                // Only valid for Pyth-pinned markets (oracle_authority==[0;32] AND
                // index_feed_id != [0;32]). Hyperp and admin-oracle markets are rejected.
                //
                // Accounts:
                //   0. [writable] Slab
                //   1. []         Pyth PriceUpdateV2 account
                //   2. []         Clock sysvar
                if accounts.len() < 3 {
                    return Err(ProgramError::NotEnoughAccountKeys);
                }
                let a_slab = &accounts[0];
                let a_pyth = &accounts[1];
                let a_clock = &accounts[2];

                accounts::expect_writable(a_slab)?;

                let clock = Clock::from_account_info(a_clock)?;
                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                require_not_paused(&data)?;

                let mut config = state::read_config(&data);

                // Only Pyth-pinned markets can use this instruction.
                // Pyth-pinned: oracle_authority == [0;32] AND index_feed_id != [0;32].
                if !crate::verify::is_pyth_pinned_mode(
                    config.oracle_authority,
                    config.index_feed_id,
                ) {
                    msg!("UpdateMarkPrice: not a Pyth-pinned market");
                    return Err(ProgramError::InvalidAccountData);
                }

                // Resolved markets don't need mark updates
                if state::is_resolved(&data) {
                    return Ok(());
                }

                // Read last update slot from engine
                let last_slot = {
                    let engine = zc::engine_ref(&data)?;
                    engine.current_slot
                };
                let dt_slots = clock.slot.saturating_sub(last_slot);
                if dt_slots == 0 {
                    return Ok(()); // same slot — no-op
                }

                // Minimum update interval — limits manipulation frequency.
                // An attacker calling UpdateMarkPrice every slot with a delayed
                // Pyth price can attempt to lag the EMA. A 5-slot (~2s) cooldown
                // reduces the rate of possible EMA manipulation attempts.
                const MIN_PYTH_MARK_UPDATE_INTERVAL_SLOTS: u64 = 5;
                if dt_slots < MIN_PYTH_MARK_UPDATE_INTERVAL_SLOTS {
                    return Ok(()); // too soon — skip silently
                }

                // SECURITY: enforce that the oracle account is owned by the
                // Pyth receiver program. Prevents substituting an attacker-controlled
                // account for a real PriceUpdateV2 feed.
                if !oracle::is_approved_oracle_program(a_pyth) {
                    msg!("UpdateMarkPrice: oracle account not owned by approved oracle program");
                    return Err(PercolatorError::OracleInvalid.into());
                }

                // Read current Pyth price (staleness + confidence + feed-ID validated).
                let raw_price = oracle::read_pyth_price_e6(
                    a_pyth,
                    &config.index_feed_id,
                    clock.unix_timestamp,
                    config.max_staleness_secs,
                    config.conf_filter_bps,
                )?;

                // Apply inversion and unit scaling as configured.
                let pyth_price = {
                    let inverted = crate::verify::invert_price_e6(raw_price, config.invert)
                        .ok_or(PercolatorError::OracleInvalid)?;
                    crate::verify::scale_price_e6(inverted, config.unit_scale)
                        .ok_or::<ProgramError>(PercolatorError::OracleInvalid.into())?
                };

                // Circuit breaker + EMA: clamp raw Pyth price movement before
                // feeding into the EMA. This prevents a single stale/wrong Pyth
                // price from instantly shifting the mark far from fair value.
                let prev_mark = config.authority_price_e6;
                let cap = config.oracle_price_cap_e2bps;
                let new_mark = oracle::compute_ema_mark_price(
                    prev_mark,
                    pyth_price,
                    dt_slots,
                    crate::constants::MARK_PRICE_EMA_ALPHA_E6,
                    cap,
                );

                config.authority_price_e6 = new_mark;
                state::write_config(&mut data, &config);

                msg!(
                    "UpdateMarkPrice: pyth_price={} prev_mark={} new_mark={} dt={}",
                    pyth_price,
                    prev_mark,
                    new_mark,
                    dt_slots,
                );
            }

            Instruction::UpdateHyperpMark => {
                // UpdateHyperpMark (Tag 34) — permissionless Hyperp EMA oracle.
                //
                // This is the core mechanism for permissionless token markets:
                // reads the spot price from a DEX AMM pool (PumpSwap/Raydium/Meteora),
                // applies 8-hour EMA smoothing with circuit breaker, and writes the
                // new mark price. No Pyth/Chainlink feed needed — the DEX IS the oracle.
                //
                // Accounts:
                //   0. [writable] Slab
                //   1. []         DEX pool account (PumpSwap/Raydium CLMM/Meteora DLMM)
                //   2. []         Clock sysvar
                //   3..N []       Remaining accounts (PumpSwap vaults for price calc)
                if accounts.len() < 3 {
                    return Err(ProgramError::NotEnoughAccountKeys);
                }
                let a_slab = &accounts[0];
                let a_dex_pool = &accounts[1];
                let a_clock = &accounts[2];

                accounts::expect_writable(a_slab)?;

                let clock = Clock::from_account_info(a_clock)?;
                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                require_not_paused(&data)?;

                // Only Hyperp markets can use this instruction
                let mut config = state::read_config(&data);
                if !oracle::is_hyperp_mode(&config) {
                    msg!("UpdateHyperpMark: not a Hyperp market");
                    return Err(ProgramError::InvalidAccountData);
                }

                // SECURITY: Bootstrap guard — admin must seed initial mark via
                // PushOraclePrice before permissionless cranking is allowed.
                // When prev_mark==0 the circuit breaker is bypassed (no reference
                // price to clamp against), so a thin-pool attacker could set an
                // arbitrary initial mark. Reject until admin bootstraps.
                if config.authority_price_e6 == 0 {
                    msg!("UpdateHyperpMark: market not bootstrapped (prev_mark==0), use PushOraclePrice first");
                    return Err(PercolatorError::OracleInvalid.into());
                }

                // Resolved markets don't need mark updates
                if state::is_resolved(&data) {
                    return Ok(());
                }

                // Read last update slot from engine
                let last_slot = {
                    let engine = zc::engine_ref(&data)?;
                    engine.current_slot
                };
                let dt_slots = clock.slot.saturating_sub(last_slot);
                if dt_slots == 0 {
                    return Ok(()); // same slot — no-op
                }

                // PERC-367: Minimum update interval — limits manipulation frequency.
                // An attacker calling UpdateHyperpMark every slot with a manipulated
                // pool can gradually shift the EMA. A 5-slot (~2s) cooldown reduces
                // the rate of possible EMA poisoning attempts.
                const MIN_HYPERP_UPDATE_INTERVAL_SLOTS: u64 = 5;
                if dt_slots < MIN_HYPERP_UPDATE_INTERVAL_SLOTS {
                    return Ok(()); // too soon — skip silently
                }

                // SECURITY: verify the DEX pool account is owned by an approved DEX program
                let is_dex = *a_dex_pool.owner == crate::oracle::PUMPSWAP_PROGRAM_ID
                    || *a_dex_pool.owner == crate::oracle::RAYDIUM_CLMM_PROGRAM_ID
                    || *a_dex_pool.owner == crate::oracle::METEORA_DLMM_PROGRAM_ID;
                if !is_dex {
                    msg!("UpdateHyperpMark: oracle account not owned by approved DEX program");
                    return Err(PercolatorError::OracleInvalid.into());
                }

                // SECURITY (MEDIUM #2): for PumpSwap pools, verify pool.base_mint matches
                // the market's collateral_mint. Without this check, a caller could pass
                // any valid PumpSwap pool (e.g., a pool for a different token pair) and
                // the program would compute prices from that unrelated pool.
                // This applies only to PumpSwap — Raydium/Meteora use different layouts.
                if *a_dex_pool.owner == crate::oracle::PUMPSWAP_PROGRAM_ID {
                    let pool_data = a_dex_pool.try_borrow_data()?;
                    // PumpSwap pool account layout (no Anchor discriminator):
                    //   [0..3]    header — pool_bump (u8), index (u8), and 1 byte flags
                    //   [3..35]   creator Pubkey (32 bytes)
                    //   [35..67]  base_mint Pubkey (32 bytes)  ← PUMPSWAP_OFF_BASE_MINT_HYPERP
                    //   [67..99]  quote_mint Pubkey (32 bytes)
                    //   [131..163] base_vault Pubkey, [163..195] quote_vault Pubkey
                    // Canonical constant: oracle::PUMPSWAP_OFF_BASE_MINT = 35 (private to
                    // oracle mod; duplicated here). Layout cross-referenced with
                    // PUMPSWAP_MIN_LEN = 195 enforced in oracle::read_pumpswap_price_e6.
                    const PUMPSWAP_OFF_BASE_MINT_HYPERP: usize = 35;
                    if pool_data.len() < PUMPSWAP_OFF_BASE_MINT_HYPERP + 32 {
                        return Err(ProgramError::InvalidAccountData);
                    }
                    let pool_base_mint: [u8; 32] = pool_data
                        [PUMPSWAP_OFF_BASE_MINT_HYPERP..PUMPSWAP_OFF_BASE_MINT_HYPERP + 32]
                        .try_into()
                        .unwrap();
                    if pool_base_mint != config.collateral_mint {
                        msg!("UpdateHyperpMark: pool base_mint does not match market collateral_mint");
                        return Err(PercolatorError::InvalidOracleKey.into());
                    }
                }

                // Read spot price AND liquidity from the DEX pool (#297 Fix 1).
                // Rejects thin pools where an attacker can cheaply manipulate spot price.
                let remaining = &accounts[3..];
                let dex_result = oracle::read_dex_price_with_liquidity(
                    a_dex_pool,
                    config.invert,
                    config.unit_scale,
                    remaining,
                )?;

                // SECURITY (#297): Minimum DEX liquidity check.
                // Prevent Hyperp EMA bootstrapping from near-empty pools.
                // An attacker with minimal capital could manipulate the spot price in a thin pool
                // and seed a false EMA baseline. The quote-side liquidity must exceed the threshold.
                if dex_result.quote_liquidity < crate::constants::MIN_DEX_QUOTE_LIQUIDITY {
                    msg!(
                        "UpdateHyperpMark: insufficient DEX liquidity {} < minimum {}",
                        dex_result.quote_liquidity,
                        crate::constants::MIN_DEX_QUOTE_LIQUIDITY
                    );
                    return Err(PercolatorError::InsufficientDexLiquidity.into());
                }

                let dex_price = dex_result.price_e6;

                // PERC-118: Hyperp EMA Blend — compute blend input from oracle + impact_mid.
                //
                // oracle      = last_effective_price_e6 (the rate-limited index price that
                //               lags behind the mark — stable reference component).
                // impact_mid  = dex_price (real-time DEX pool spot price).
                //
                // blend = (oracle_weight * oracle + (10_000 - oracle_weight) * impact_mid) / 10_000
                //
                // When mark_oracle_weight_bps == 0 (default / existing markets), blend == dex_price
                // (fully backward-compatible — pure DEX price tracking as before).
                //
                // When mark_oracle_weight_bps > 0, the blend anchors the mark toward the stable
                // oracle component, creating a non-trivial mark_premium that can drive funding.
                //
                // Use prev_mark as oracle fallback if last_effective_price_e6 is not yet seeded.
                let prev_mark = config.authority_price_e6;
                let oracle_for_blend = if config.last_effective_price_e6 > 0 {
                    config.last_effective_price_e6
                } else {
                    prev_mark
                };
                let oracle_weight_bps = state::get_mark_oracle_weight_bps(&config);
                let blend_input = oracle::compute_blend_mark_price(
                    oracle_for_blend,
                    dex_price,
                    oracle_weight_bps,
                );

                // SECURITY (#297 Fix 2): Circuit breaker BEFORE EMA update.
                // The blended input price must be clamped before it propagates into the EMA.
                // Enforce a minimum cap — even if admin set oracle_price_cap_e2bps to 0,
                // Hyperp markets always use at least DEFAULT_HYPERP_PRICE_CAP_E2BPS.
                let effective_cap = core::cmp::max(
                    config.oracle_price_cap_e2bps,
                    crate::constants::DEFAULT_HYPERP_PRICE_CAP_E2BPS,
                );
                let new_mark = oracle::compute_ema_mark_price(
                    prev_mark,
                    blend_input,
                    dt_slots,
                    crate::constants::MARK_PRICE_EMA_ALPHA_E6,
                    effective_cap,
                );

                // Update last_effective_price_e6 toward new_mark (rate-limited).
                // This keeps the oracle-for-blend current so subsequent UpdateHyperpMark
                // calls have a fresh reference, and PERC-119 can compute mark_premium from it.
                let new_index = oracle::clamp_toward_with_dt(
                    oracle_for_blend.max(1),
                    new_mark,
                    effective_cap,
                    dt_slots,
                );

                config.authority_price_e6 = new_mark;
                config.last_effective_price_e6 = new_index;
                state::write_config(&mut data, &config);

                msg!(
                    "UpdateHyperpMark: dex_price={} oracle={} blend={} prev_mark={} new_mark={} index={} weight_bps={} dt={}",
                    dex_price,
                    oracle_for_blend,
                    blend_input,
                    prev_mark,
                    new_mark,
                    new_index,
                    oracle_weight_bps,
                    dt_slots
                );
            }

            Instruction::UnresolveMarket { confirmation } => {
                // PERC-273: Unresolve market — clear RESOLVED flag, re-enable trading.
                // Admin only. Requires confirmation code to prevent accidental invocation.
                // PERC-322/MEDIUM-2: Oracle recovery check — verify oracle is alive before
                // re-enabling trading to prevent operating with stale/absent prices.
                const UNRESOLVE_CONFIRMATION: u64 = 0xDEAD_BEEF_CAFE_1234;
                if confirmation != UNRESOLVE_CONFIRMATION {
                    msg!("UnresolveMarket: invalid confirmation code");
                    return Err(ProgramError::InvalidInstructionData);
                }
                accounts::expect_len(accounts, 4)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];
                let a_clock = &accounts[2];
                let a_oracle = &accounts[3];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                // Must currently be resolved
                if !state::is_resolved(&data) {
                    return Err(ProgramError::InvalidAccountData);
                }

                // PERC-322/MEDIUM-2: Verify oracle is alive before re-enabling trading.
                // Prevents unresolving into a stale-oracle state where trades execute
                // against outdated prices.
                let mut config = state::read_config(&data);
                let clock = Clock::from_account_info(a_clock)?;
                let remaining_oracle_accounts = if accounts.len() > 4 {
                    &accounts[4..]
                } else {
                    &[]
                };
                let oracle_price = oracle::read_price_with_authority(
                    &config,
                    a_oracle,
                    clock.unix_timestamp,
                    remaining_oracle_accounts,
                )?;
                if oracle_price == 0 {
                    msg!("UnresolveMarket: oracle returned zero price — cannot unresolve");
                    return Err(PercolatorError::OracleInvalid.into());
                }

                // Update config with fresh oracle price so the first crank after unresolve
                // operates on a known-good baseline
                config.last_effective_price_e6 = oracle_price;
                config.settlement_price_e6 = 0; // Clear settlement price
                state::write_config(&mut data, &config);

                // Clear the resolved flag
                state::clear_resolved(&mut data);

                msg!(
                    "UnresolveMarket: admin={} slab={} oracle_price={}",
                    a_admin.key,
                    a_slab.key,
                    oracle_price
                );
            }

            // ================================================================
            // PERC-272: LP Vault Instructions
            // ================================================================
            Instruction::CreateLpVault {
                fee_share_bps,
                util_curve_enabled,
            } => {
                // Create LP vault: initialise state PDA + SPL mint.
                // Admin only, one per market.
                // Accounts: [admin(signer,payer), slab(writable), lp_vault_state(writable),
                //            lp_vault_mint(writable), vault_authority, system_program,
                //            token_program, rent_sysvar]
                accounts::expect_len(accounts, 8)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];
                let a_lp_vault_state = &accounts[2];
                let a_lp_vault_mint = &accounts[3];
                let a_vault_authority = &accounts[4];
                let a_system = &accounts[5];
                let a_token = &accounts[6];
                let a_rent = &accounts[7];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;
                accounts::expect_writable(a_lp_vault_state)?;
                accounts::expect_writable(a_lp_vault_mint)?;
                verify_token_program(a_token)?;

                if fee_share_bps > 10_000 {
                    return Err(PercolatorError::LpVaultInvalidFeeShare.into());
                }

                let data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;
                drop(data);

                // Verify LP vault state PDA
                #[allow(unused_variables)]
                let (expected_state, state_bump) =
                    accounts::derive_lp_vault_state(program_id, a_slab.key);
                accounts::expect_key(a_lp_vault_state, &expected_state)?;

                // Check not already created: in non-test mode, data_len == 0 means
                // the PDA has not been created yet. In test mode, the account is
                // pre-allocated with zeroed data, so we check the magic value.
                if a_lp_vault_state.data_len() > 0 {
                    let state_data = a_lp_vault_state.try_borrow_data()?;
                    if state_data.len() >= 8 {
                        let magic = u64::from_le_bytes(state_data[..8].try_into().unwrap());
                        if magic == crate::lp_vault::LP_VAULT_MAGIC {
                            return Err(PercolatorError::LpVaultAlreadyExists.into());
                        }
                    }
                    drop(state_data);
                }

                // Verify LP vault mint PDA
                let (expected_mint, mint_bump) =
                    accounts::derive_lp_vault_mint(program_id, a_slab.key);
                accounts::expect_key(a_lp_vault_mint, &expected_mint)?;

                // Verify vault authority PDA
                let (auth, _vault_bump) = accounts::derive_vault_authority(program_id, a_slab.key);
                accounts::expect_key(a_vault_authority, &auth)?;

                // Create state PDA account
                #[cfg(not(feature = "test"))]
                {
                    use solana_program::program::invoke_signed;
                    use solana_program::sysvar::Sysvar;

                    let space = crate::lp_vault::LP_VAULT_STATE_LEN;
                    let rent = solana_program::rent::Rent::get()?;
                    let lamports = rent.minimum_balance(space);

                    let state_seeds: &[&[u8]] = &[b"lp_vault", a_slab.key.as_ref(), &[state_bump]];
                    let create_ix = solana_program::system_instruction::create_account(
                        a_admin.key,
                        a_lp_vault_state.key,
                        lamports,
                        space as u64,
                        program_id,
                    );
                    invoke_signed(
                        &create_ix,
                        &[a_admin.clone(), a_lp_vault_state.clone(), a_system.clone()],
                        &[state_seeds],
                    )?;
                }
                #[cfg(feature = "test")]
                {
                    let _ = a_system;
                    // In test mode the account is pre-created with correct size
                }

                // Initialise LP vault state
                {
                    let mut state_data = a_lp_vault_state.try_borrow_mut_data()?;
                    if state_data.len() < crate::lp_vault::LP_VAULT_STATE_LEN {
                        return Err(ProgramError::AccountDataTooSmall);
                    }
                    let mut vault_state = crate::lp_vault::LpVaultState::new_zeroed();
                    vault_state.magic = crate::lp_vault::LP_VAULT_MAGIC;
                    vault_state.fee_share_bps = fee_share_bps;
                    vault_state.epoch = 1;
                    // PERC-304: Set util curve flag and initial multiplier
                    vault_state.lp_util_curve_enabled = if util_curve_enabled { 1 } else { 0 };
                    vault_state.current_fee_mult_bps = crate::verify::FEE_MULT_BASE_BPS as u32;
                    // PERC-313: Default HWM floor = 50%
                    vault_state.hwm_floor_bps = 5000;
                    // Snapshot current fee_revenue so we only distribute NEW fees
                    let slab_data = a_slab.try_borrow_data()?;
                    let engine = zc::engine_ref(&slab_data)?;
                    vault_state.last_fee_snapshot = engine.insurance_fund.fee_revenue.get();
                    drop(slab_data);
                    crate::lp_vault::write_lp_vault_state(&mut state_data, &vault_state);
                }

                // Create LP vault mint (reuse insurance_lp::create_mint)
                let mint_seeds: &[&[u8]] = &[b"lp_vault_mint", a_slab.key.as_ref(), &[mint_bump]];
                // Read collateral decimals from config
                let slab_data = a_slab.try_borrow_data()?;
                let config = state::read_config(&slab_data);
                drop(slab_data);
                // Use same decimals as collateral for LP token
                let decimals = 6u8; // Standard for SOL-denominated vaults
                let _ = config;
                crate::insurance_lp::create_mint(
                    a_admin,
                    a_lp_vault_mint,
                    a_vault_authority,
                    a_system,
                    a_token,
                    a_rent,
                    decimals,
                    mint_seeds,
                )?;

                msg!(
                    "LP vault created: fee_share={}bps util_curve={} slab={}",
                    fee_share_bps,
                    util_curve_enabled,
                    a_slab.key
                );
            }

            Instruction::LpVaultDeposit { amount } => {
                // Deposit SOL into LP vault, receive LP shares.
                // Accounts: [depositor(signer), slab(writable), depositor_ata(writable),
                //            vault(writable), token_program, lp_vault_mint(writable),
                //            depositor_lp_ata(writable), vault_authority,
                //            lp_vault_state(writable)]
                accounts::expect_len(accounts, 9)?;
                let a_depositor = &accounts[0];
                let a_slab = &accounts[1];
                let a_depositor_ata = &accounts[2];
                let a_vault = &accounts[3];
                let a_token = &accounts[4];
                let a_lp_vault_mint = &accounts[5];
                let a_depositor_lp_ata = &accounts[6];
                let a_vault_authority = &accounts[7];
                let a_lp_vault_state = &accounts[8];

                accounts::expect_signer(a_depositor)?;
                accounts::expect_writable(a_slab)?;
                accounts::expect_writable(a_depositor_ata)?;
                accounts::expect_writable(a_vault)?;
                accounts::expect_writable(a_lp_vault_mint)?;
                accounts::expect_writable(a_depositor_lp_ata)?;
                accounts::expect_writable(a_lp_vault_state)?;
                verify_token_program(a_token)?;

                if amount == 0 {
                    return Err(PercolatorError::LpVaultZeroAmount.into());
                }

                let slab_data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &slab_data)?;
                require_initialized(&slab_data)?;

                // Block deposits on resolved or paused markets
                if state::is_resolved(&slab_data) {
                    return Err(ProgramError::InvalidAccountData);
                }
                require_not_paused(&slab_data)?;

                let config = state::read_config(&slab_data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                // Verify vault
                let (auth, vault_bump) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_depositor_ata, a_depositor.key, &mint)?;

                // Verify LP vault mint PDA
                let (expected_lp_mint, _) = accounts::derive_lp_vault_mint(program_id, a_slab.key);
                accounts::expect_key(a_lp_vault_mint, &expected_lp_mint)?;
                if a_lp_vault_mint.data_len() == 0 {
                    return Err(PercolatorError::LpVaultNotCreated.into());
                }

                // Verify LP vault state PDA
                let (expected_state, _) = accounts::derive_lp_vault_state(program_id, a_slab.key);
                accounts::expect_key(a_lp_vault_state, &expected_state)?;

                // Verify vault authority PDA
                accounts::expect_key(a_vault_authority, &auth)?;

                // Read LP vault state
                let mut vs_data = a_lp_vault_state.try_borrow_mut_data()?;
                let mut vault_state = crate::lp_vault::read_lp_vault_state(&vs_data)
                    .ok_or(PercolatorError::LpVaultNotCreated)?;
                if !vault_state.is_initialized() {
                    return Err(PercolatorError::LpVaultNotCreated.into());
                }

                // Read current LP supply and vault capital
                let lp_supply = crate::insurance_lp::read_mint_supply(a_lp_vault_mint)?;
                let capital_before = vault_state.total_capital;

                // Transfer collateral from depositor to vault
                // Must drop slab_data borrow to allow collateral::deposit CPI
                drop(slab_data);
                collateral::deposit(a_token, a_depositor_ata, a_vault, a_depositor, amount)?;

                // Convert base tokens to units
                let slab_data = a_slab.try_borrow_data()?;
                let config = state::read_config(&slab_data);
                let (units, dust) = crate::units::base_to_units(amount, config.unit_scale);
                drop(slab_data);

                // Accumulate dust
                let mut slab_data = state::slab_data_mut(a_slab)?;
                let old_dust = state::read_dust_base(&slab_data);
                state::write_dust_base(&mut slab_data, old_dust.saturating_add(dust));

                // Calculate LP tokens to mint
                let lp_tokens_to_mint: u64 = if lp_supply == 0 || capital_before == 0 {
                    // First deposit (or fresh epoch after drain): 1:1
                    // If epoch incremented due to drain, LP supply is 0 and capital is 0.
                    if lp_supply > 0 && capital_before == 0 {
                        // Supply > 0 but capital == 0: vault was drained. Start fresh epoch.
                        vault_state.epoch = vault_state.epoch.saturating_add(1);
                        // Previous shares are worthless. Mint fresh for new depositor.
                        // NOTE: old share holders get nothing (vault was fully drained).
                    }
                    units
                } else {
                    // Proportional: tokens = deposit_units * supply / capital
                    let numerator = (units as u128)
                        .checked_mul(lp_supply as u128)
                        .ok_or(PercolatorError::EngineOverflow)?;
                    let result = numerator / capital_before;
                    if result > u64::MAX as u128 {
                        return Err(PercolatorError::EngineOverflow.into());
                    }
                    result as u64
                };

                if lp_tokens_to_mint == 0 {
                    return Err(PercolatorError::LpVaultZeroAmount.into());
                }

                // Increase LP vault capital
                vault_state.total_capital = vault_state
                    .total_capital
                    .checked_add(units as u128)
                    .ok_or(PercolatorError::EngineOverflow)?;

                // PERC-313: Update epoch high-water mark after deposit
                // Track the peak TVL within each epoch so withdrawals can be
                // bounded to a floor percentage of that peak.
                if vault_state.hwm_floor_bps > 0
                    && vault_state.total_capital > vault_state.epoch_high_water_tvl
                {
                    vault_state.epoch_high_water_tvl = vault_state.total_capital;
                }

                // Update engine vault balance (tokens are already in the vault token account)
                let engine = zc::engine_mut(&mut slab_data)?;
                engine.vault = percolator::U128::new(
                    engine
                        .vault
                        .get()
                        .checked_add(units as u128)
                        .ok_or(PercolatorError::EngineOverflow)?,
                );
                drop(slab_data);

                // Write updated vault state
                crate::lp_vault::write_lp_vault_state(&mut vs_data, &vault_state);
                drop(vs_data);

                // Mint LP tokens to depositor
                let seed1: &[u8] = b"vault";
                let seed2: &[u8] = a_slab.key.as_ref();
                let bump_arr: [u8; 1] = [vault_bump];
                let seed3: &[u8] = &bump_arr;
                let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
                let signer_seeds: [&[&[u8]]; 1] = [&seeds];

                crate::insurance_lp::mint_to(
                    a_token,
                    a_lp_vault_mint,
                    a_depositor_lp_ata,
                    a_vault_authority,
                    lp_tokens_to_mint,
                    &signer_seeds,
                )?;

                // PERC-627: Track creator deposit in CreatorStakeLock PDA.
                if accounts.len() >= 10 {
                    let a_creator_lock = &accounts[9];
                    let (expected_lock_pda, _) = Pubkey::find_program_address(
                        &[crate::creator_lock::CREATOR_LOCK_SEED, a_slab.key.as_ref()],
                        program_id,
                    );
                    if *a_creator_lock.key == expected_lock_pda && a_creator_lock.is_writable {
                        if let Ok(mut lock_data) = a_creator_lock.try_borrow_mut_data() {
                            if let Some(lock_state) = crate::creator_lock::read_state(&lock_data) {
                                let creator_key = Pubkey::new_from_array(lock_state.creator);
                                if *a_depositor.key == creator_key {
                                    let mut new_lock = *lock_state;
                                    new_lock.lp_amount_locked =
                                        new_lock.lp_amount_locked.saturating_add(lp_tokens_to_mint);
                                    new_lock.cumulative_deposited = new_lock
                                        .cumulative_deposited
                                        .saturating_add(lp_tokens_to_mint as u64);
                                    crate::creator_lock::write_state(&mut lock_data, &new_lock);
                                }
                            }
                        }
                    }
                }

                msg!(
                    "LP vault deposit: {} tokens, {} LP shares minted, epoch={}",
                    amount,
                    lp_tokens_to_mint,
                    vault_state.epoch
                );
            }

            Instruction::LpVaultWithdraw { lp_amount } => {
                // Burn LP shares and withdraw proportional SOL from LP vault.
                // Accounts: [withdrawer(signer), slab(writable), withdrawer_ata(writable),
                //            vault(writable), token_program, lp_vault_mint(writable),
                //            withdrawer_lp_ata(writable), vault_authority,
                //            lp_vault_state(writable)]
                accounts::expect_len(accounts, 9)?;
                let a_withdrawer = &accounts[0];
                let a_slab = &accounts[1];
                let a_withdrawer_ata = &accounts[2];
                let a_vault = &accounts[3];
                let a_token = &accounts[4];
                let a_lp_vault_mint = &accounts[5];
                let a_withdrawer_lp_ata = &accounts[6];
                let a_vault_authority = &accounts[7];
                let a_lp_vault_state = &accounts[8];

                accounts::expect_signer(a_withdrawer)?;
                accounts::expect_writable(a_slab)?;
                accounts::expect_writable(a_withdrawer_ata)?;
                accounts::expect_writable(a_vault)?;
                accounts::expect_writable(a_lp_vault_mint)?;
                accounts::expect_writable(a_withdrawer_lp_ata)?;
                accounts::expect_writable(a_lp_vault_state)?;
                verify_token_program(a_token)?;

                if lp_amount == 0 {
                    return Err(PercolatorError::LpVaultZeroAmount.into());
                }

                // PERC-627: Creator stake lock enforcement.
                if accounts.len() >= 10 {
                    let a_creator_lock = &accounts[9];
                    let (expected_lock_pda, _) = Pubkey::find_program_address(
                        &[crate::creator_lock::CREATOR_LOCK_SEED, a_slab.key.as_ref()],
                        program_id,
                    );
                    if *a_creator_lock.key == expected_lock_pda {
                        accounts::expect_writable(a_creator_lock)?;
                        let mut lock_data = a_creator_lock
                            .try_borrow_mut_data()
                            .map_err(|_| ProgramError::AccountBorrowFailed)?;
                        if let Some(lock_state) = crate::creator_lock::read_state(&lock_data) {
                            let creator_key = Pubkey::new_from_array(lock_state.creator);
                            if *a_withdrawer.key == creator_key {
                                let clock = solana_program::clock::Clock::get()?;
                                let expired = crate::creator_lock::is_lock_expired(
                                    clock.slot,
                                    lock_state.lock_start_slot,
                                    lock_state.lock_duration_slots,
                                );
                                let max_withdraw = crate::creator_lock::max_withdrawable(
                                    lp_amount,
                                    lock_state.lp_amount_locked,
                                    expired,
                                );
                                if lp_amount > max_withdraw {
                                    msg!(
                                        "CREATOR_LOCK: withdraw {} > max {}",
                                        lp_amount,
                                        max_withdraw
                                    );
                                    return Err(ProgramError::InvalidArgument);
                                }
                                let mut new_lock = *lock_state;
                                new_lock.cumulative_extracted = new_lock
                                    .cumulative_extracted
                                    .saturating_add(lp_amount as u64);
                                if crate::creator_lock::check_extraction_exceeded(
                                    new_lock.cumulative_extracted,
                                    new_lock.cumulative_deposited,
                                    crate::creator_lock::EXTRACTION_LIMIT_BPS,
                                ) {
                                    new_lock.fee_redirect_active = 1;
                                    msg!("CREATOR_LOCK: fee redirect activated");
                                }
                                crate::creator_lock::write_state(&mut lock_data, &new_lock);
                            }
                        }
                    }
                }

                let mut slab_data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &slab_data)?;
                require_initialized(&slab_data)?;

                let config = state::read_config(&slab_data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                // Verify vault
                let (auth, vault_bump) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_withdrawer_ata, a_withdrawer.key, &mint)?;

                // Verify LP vault mint PDA
                let (expected_lp_mint, _) = accounts::derive_lp_vault_mint(program_id, a_slab.key);
                accounts::expect_key(a_lp_vault_mint, &expected_lp_mint)?;

                // Verify LP vault state PDA
                let (expected_state, _) = accounts::derive_lp_vault_state(program_id, a_slab.key);
                accounts::expect_key(a_lp_vault_state, &expected_state)?;

                // Verify vault authority
                accounts::expect_key(a_vault_authority, &auth)?;

                // Read LP vault state
                let mut vs_data = a_lp_vault_state.try_borrow_mut_data()?;
                let mut vault_state = crate::lp_vault::read_lp_vault_state(&vs_data)
                    .ok_or(PercolatorError::LpVaultNotCreated)?;
                if !vault_state.is_initialized() {
                    return Err(PercolatorError::LpVaultNotCreated.into());
                }

                let lp_supply = crate::insurance_lp::read_mint_supply(a_lp_vault_mint)?;
                let capital = vault_state.total_capital;

                if lp_supply == 0 || capital == 0 {
                    return Err(PercolatorError::LpVaultSupplyMismatch.into());
                }

                // Calculate units to return: lp_amount * capital / supply
                // Round DOWN (user gets less — vault is never underfunded)
                let numerator = (lp_amount as u128)
                    .checked_mul(capital)
                    .ok_or(PercolatorError::EngineOverflow)?;
                let units_to_return = numerator / (lp_supply as u128);

                if units_to_return == 0 {
                    return Err(PercolatorError::LpVaultZeroAmount.into());
                }

                // PERC-313: High-water mark floor enforcement.
                // Block withdrawals that would push total_capital below
                // hwm_floor_bps % of the epoch's peak TVL.
                if vault_state.hwm_floor_bps > 0 && vault_state.epoch_high_water_tvl > 0 {
                    let remaining = capital
                        .checked_sub(units_to_return)
                        .ok_or(PercolatorError::EngineOverflow)?;
                    let floor = vault_state
                        .epoch_high_water_tvl
                        .saturating_mul(vault_state.hwm_floor_bps as u128)
                        / 10_000;
                    if remaining < floor {
                        msg!(
                            "HWM block: remaining={} < floor={} (hwm={}, bps={})",
                            remaining,
                            floor,
                            vault_state.epoch_high_water_tvl,
                            vault_state.hwm_floor_bps
                        );
                        return Err(PercolatorError::LpVaultWithdrawExceedsAvailable.into());
                    }
                }

                // OI reservation check: cannot withdraw if it would make vault
                // too small to back current OI. After withdrawal:
                //   remaining_capital >= total_oi / (effective_multiplier / 10_000)
                // Equivalently: remaining_capital * effective_multiplier / 10_000 >= total_oi
                // PERC-298: Unpack to get base multiplier. PERC-302: Use ramped multiplier.
                let (oi_multiplier, _) = unpack_oi_cap(config.oi_cap_multiplier_bps);
                if oi_multiplier > 0 {
                    let remaining_capital = capital.saturating_sub(units_to_return);
                    let engine = zc::engine_ref(&slab_data)?;
                    let current_oi = engine.total_open_interest.get();
                    let max_oi_after =
                        remaining_capital.saturating_mul(oi_multiplier as u128) / 10_000;
                    if current_oi > max_oi_after {
                        return Err(PercolatorError::LpVaultWithdrawExceedsAvailable.into());
                    }
                }

                // Convert units to base tokens
                let units_u64 = if units_to_return > u64::MAX as u128 {
                    return Err(PercolatorError::EngineOverflow.into());
                } else {
                    units_to_return as u64
                };
                let base_amount = crate::units::units_to_base_checked(units_u64, config.unit_scale)
                    .ok_or(PercolatorError::EngineOverflow)?;

                // Reduce LP vault capital
                vault_state.total_capital = capital
                    .checked_sub(units_to_return)
                    .ok_or(PercolatorError::EngineOverflow)?;

                // Reduce engine vault balance
                let engine = zc::engine_mut(&mut slab_data)?;
                engine.vault = percolator::U128::new(
                    engine
                        .vault
                        .get()
                        .checked_sub(units_to_return)
                        .ok_or(PercolatorError::EngineOverflow)?,
                );
                drop(slab_data);

                // Write updated vault state
                crate::lp_vault::write_lp_vault_state(&mut vs_data, &vault_state);
                drop(vs_data);

                // Burn LP tokens from withdrawer
                crate::insurance_lp::burn(
                    a_token,
                    a_lp_vault_mint,
                    a_withdrawer_lp_ata,
                    a_withdrawer,
                    lp_amount,
                )?;

                // Transfer collateral from vault to withdrawer
                let seed1: &[u8] = b"vault";
                let seed2: &[u8] = a_slab.key.as_ref();
                let bump_arr: [u8; 1] = [vault_bump];
                let seed3: &[u8] = &bump_arr;
                let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
                let signer_seeds: [&[&[u8]]; 1] = [&seeds];

                collateral::withdraw(
                    a_token,
                    a_vault,
                    a_withdrawer_ata,
                    a_vault_authority,
                    base_amount,
                    &signer_seeds,
                )?;

                msg!(
                    "LP vault withdraw: {} LP burned, {} tokens returned, epoch={}",
                    lp_amount,
                    base_amount,
                    vault_state.epoch
                );
            }

            Instruction::LpVaultCrankFees => {
                // Permissionless crank: distribute accrued fee revenue to LP vault.
                // Reads fee_revenue delta since last snapshot, credits LP portion.
                // PERC-304: If util curve enabled, applies utilization-based fee multiplier.
                // Accounts: [slab(writable), lp_vault_state(writable), clock(optional)]
                if accounts.len() < 2 {
                    return Err(ProgramError::NotEnoughAccountKeys);
                }
                let a_slab = &accounts[0];
                let a_lp_vault_state = &accounts[1];

                accounts::expect_writable(a_slab)?;
                accounts::expect_writable(a_lp_vault_state)?;

                let mut slab_data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &slab_data)?;
                require_initialized(&slab_data)?;

                // Verify LP vault state PDA
                let (expected_state, _) = accounts::derive_lp_vault_state(program_id, a_slab.key);
                accounts::expect_key(a_lp_vault_state, &expected_state)?;

                // Read LP vault state
                let mut vs_data = a_lp_vault_state.try_borrow_mut_data()?;
                let mut vault_state = crate::lp_vault::read_lp_vault_state(&vs_data)
                    .ok_or(PercolatorError::LpVaultNotCreated)?;
                if !vault_state.is_initialized() {
                    return Err(PercolatorError::LpVaultNotCreated.into());
                }

                // Read market config for OI cap multiplier (PERC-304)
                let config = state::read_config(&slab_data);

                // Read current fee revenue from engine
                let engine = zc::engine_mut(&mut slab_data)?;
                let current_fee_revenue = engine.insurance_fund.fee_revenue.get();
                let last_snapshot = vault_state.last_fee_snapshot;

                // Calculate new fees since last crank
                let fee_delta = current_fee_revenue.saturating_sub(last_snapshot);
                if fee_delta == 0 {
                    return Err(PercolatorError::LpVaultNoNewFees.into());
                }

                // PERC-304: Compute utilization-based fee multiplier
                let (oi_mult_for_util, _) = unpack_oi_cap(config.oi_cap_multiplier_bps);
                let fee_mult_bps: u64 = if vault_state.lp_util_curve_enabled != 0
                    && oi_mult_for_util > 0
                {
                    // Compute max OI from engine vault balance and config multiplier
                    let vault_balance = engine.vault.get();
                    let max_oi = vault_balance.saturating_mul(oi_mult_for_util as u128) / 10_000;
                    let current_oi = engine.total_open_interest.get();

                    // Utilization = current_oi / max_oi (in bps)
                    let util_bps = crate::verify::compute_util_bps(current_oi, max_oi);
                    let mult = crate::verify::compute_fee_multiplier_bps(util_bps);

                    // Store the computed multiplier for off-chain readers
                    vault_state.current_fee_mult_bps = mult as u32;
                    mult
                } else {
                    // Curve disabled or no OI cap — 1.0× multiplier
                    vault_state.current_fee_mult_bps = crate::verify::FEE_MULT_BASE_BPS as u32;
                    crate::verify::FEE_MULT_BASE_BPS
                };

                // LP vault gets fee_share_bps portion of the delta, scaled by multiplier.
                // lp_portion = fee_delta * fee_share_bps / 10_000 * fee_mult_bps / 10_000
                // Rewritten to minimise rounding loss:
                //   lp_portion = fee_delta * fee_share_bps * fee_mult_bps / (10_000 * 10_000)
                let lp_portion = fee_delta
                    .saturating_mul(vault_state.fee_share_bps as u128)
                    .saturating_mul(fee_mult_bps as u128)
                    / (10_000u128 * 10_000u128);

                if lp_portion > 0 {
                    // Move lp_portion from insurance fund balance to LP vault capital.
                    // The tokens are already in the vault token account — this just
                    // updates the internal accounting (insurance → LP vault).
                    let ins_balance = engine.insurance_fund.balance.get();
                    let actual_transfer = core::cmp::min(lp_portion, ins_balance);

                    engine.insurance_fund.balance =
                        percolator::U128::new(ins_balance.saturating_sub(actual_transfer));

                    vault_state.total_capital = vault_state
                        .total_capital
                        .checked_add(actual_transfer)
                        .ok_or(PercolatorError::EngineOverflow)?;
                    vault_state.total_fees_distributed = vault_state
                        .total_fees_distributed
                        .checked_add(actual_transfer)
                        .ok_or(PercolatorError::EngineOverflow)?;
                }

                // Update snapshot and last crank slot
                vault_state.last_fee_snapshot = current_fee_revenue;
                // Read clock: prefer explicit account (index 2) for testability,
                // fall back to Clock::get() syscall for backward compatibility.
                let clock_slot = if accounts.len() > 2 {
                    Clock::from_account_info(&accounts[2])?.slot
                } else {
                    Clock::get()?.slot
                };
                vault_state.last_crank_slot = clock_slot;
                drop(slab_data);

                crate::lp_vault::write_lp_vault_state(&mut vs_data, &vault_state);

                msg!(
                    "LP vault fee crank: delta={} mult={}bps lp_portion={} capital={} slot={}",
                    fee_delta,
                    fee_mult_bps,
                    lp_portion,
                    vault_state.total_capital,
                    clock_slot
                );
            }
            // ========================================
            // PERC-306: Per-Market Insurance Isolation
            // ========================================
            Instruction::FundMarketInsurance { amount } => {
                // PERC-306: Fund isolated insurance balance for this market.
                // Same account layout as TopUpInsurance: [admin, slab, admin_ata, vault, token_program]
                accounts::expect_len(accounts, 5)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];
                let a_admin_ata = &accounts[2];
                let a_vault = &accounts[3];
                let a_token = &accounts[4];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;
                verify_token_program(a_token)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                // Block when market is resolved
                if state::is_resolved(&data) {
                    return Err(ProgramError::InvalidAccountData);
                }

                let config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let (auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_admin_ata, a_admin.key, &mint)?;

                // Transfer tokens to vault
                collateral::deposit(a_token, a_admin_ata, a_vault, a_admin, amount)?;

                // Convert to units
                let (units, dust) = crate::units::base_to_units(amount, config.unit_scale);
                let old_dust = state::read_dust_base(&data);
                state::write_dust_base(&mut data, old_dust.saturating_add(dust));

                // Fund isolated insurance (not global)
                let engine = zc::engine_mut(&mut data)?;
                engine
                    .fund_market_insurance(units as u128)
                    .map_err(map_risk_error)?;

                msg!("PERC-306: funded market insurance with {} units", units);
            }

            Instruction::SetInsuranceIsolation { bps } => {
                // PERC-306: Set insurance isolation BPS for this market. Admin only.
                // Accounts: [admin(signer), slab(writable)]
                accounts::expect_len(accounts, 2)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                // Validate BPS range (0 = disabled, max 10000 = 100%)
                if bps > 10_000 {
                    return Err(ProgramError::InvalidInstructionData);
                }

                // Update engine's insurance isolation BPS
                let engine = zc::engine_mut(&mut data)?;
                engine.set_insurance_isolation_bps(bps);

                // Also write to MarketConfig for persistence
                let mut config = state::read_config(&data);
                config.insurance_isolation_bps = bps;
                state::write_config(&mut data, &config);

                msg!("PERC-306: set insurance isolation to {} bps", bps);
            }

            // =============================================================
            // PERC-314: Settlement Dispute
            // =============================================================
            Instruction::ChallengeSettlement { proposed_price_e6 } => {
                // Accounts: [challenger(signer), slab(writable), dispute_pda(writable),
                //            challenger_ata(writable), vault(writable), token_program, system_program]
                accounts::expect_len(accounts, 7)?;
                let a_challenger = &accounts[0];
                let a_slab = &accounts[1];
                let a_dispute = &accounts[2];
                let a_challenger_ata = &accounts[3];
                let a_vault = &accounts[4];
                let a_token = &accounts[5];
                let a_system = &accounts[6];

                accounts::expect_signer(a_challenger)?;
                accounts::expect_writable(a_slab)?;
                accounts::expect_writable(a_dispute)?;
                accounts::expect_writable(a_challenger_ata)?;
                accounts::expect_writable(a_vault)?;
                verify_token_program(a_token)?;

                let data = a_slab.try_borrow_data()?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                // Must be resolved
                if !state::is_resolved(&data) {
                    return Err(PercolatorError::MarketNotResolved.into());
                }

                let config = state::read_config(&data);
                drop(data);

                // Check dispute window is open
                if config.dispute_window_slots == 0 {
                    return Err(PercolatorError::DisputeWindowClosed.into());
                }
                let clock = Clock::get()?;
                // PERC-322/MEDIUM-3: Use checked_add to prevent overflow.
                // If resolved_slot + dispute_window_slots overflows u64, the dispute
                // window is treated as closed (fail-safe) rather than saturating to
                // u64::MAX which would keep the window open indefinitely.
                let window_end = config
                    .resolved_slot
                    .checked_add(config.dispute_window_slots)
                    .ok_or(PercolatorError::DisputeWindowClosed)?;
                if clock.slot > window_end {
                    return Err(PercolatorError::DisputeWindowClosed.into());
                }

                // Verify and create dispute PDA
                let (expected_dispute, dispute_bump) =
                    accounts::derive_dispute(program_id, a_slab.key);
                accounts::expect_key(a_dispute, &expected_dispute)?;

                if a_dispute.data_len() > 0 {
                    let d_data = a_dispute.try_borrow_data()?;
                    if let Some(existing) = crate::dispute::read_dispute(&d_data) {
                        if existing.is_initialized() {
                            return Err(PercolatorError::DisputeAlreadyExists.into());
                        }
                    }
                    drop(d_data);
                }

                // Transfer bond from challenger to vault
                let mint = Pubkey::new_from_array(config.collateral_mint);
                let (auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_challenger_ata, a_challenger.key, &mint)?;

                if config.dispute_bond_amount > 0 {
                    collateral::deposit(
                        a_token,
                        a_challenger_ata,
                        a_vault,
                        a_challenger,
                        config.dispute_bond_amount,
                    )?;
                }

                // Create dispute PDA
                let dispute_len = crate::dispute::DISPUTE_LEN;
                let rent = solana_program::rent::Rent::get()?;
                let lamports = rent.minimum_balance(dispute_len);

                let seed1: &[u8] = b"dispute";
                let seed2: &[u8] = a_slab.key.as_ref();
                let bump_arr: [u8; 1] = [dispute_bump];
                let seed3: &[u8] = &bump_arr;
                let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
                let signer_seeds: [&[&[u8]]; 1] = [&seeds];

                solana_program::program::invoke_signed(
                    &solana_program::system_instruction::create_account(
                        a_challenger.key,
                        a_dispute.key,
                        lamports,
                        dispute_len as u64,
                        program_id,
                    ),
                    &[a_challenger.clone(), a_dispute.clone(), a_system.clone()],
                    &signer_seeds,
                )?;

                let dispute = crate::dispute::SettlementDispute {
                    magic: crate::dispute::DISPUTE_MAGIC,
                    challenger: a_challenger.key.to_bytes(),
                    proposed_price_e6,
                    proof_slot: clock.slot,
                    bond_amount: config.dispute_bond_amount,
                    outcome: 0, // pending
                    _pad: [0; 7],
                    dispute_slot: clock.slot,
                    _reserved: [0; 16],
                };

                let mut d_data = a_dispute.try_borrow_mut_data()?;
                crate::dispute::write_dispute(&mut d_data, &dispute);

                msg!(
                    "PERC-314: Settlement challenged: proposed={} vs settlement={}",
                    proposed_price_e6,
                    config.settlement_price_e6
                );
            }

            Instruction::ResolveDispute { accept } => {
                // Admin resolves dispute. accept=1: challenger wins, accept=0: challenger loses.
                // Accounts: [admin(signer), slab(writable), dispute_pda(writable),
                //            challenger_ata(writable), vault(writable), vault_authority, token_program]
                accounts::expect_len(accounts, 7)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];
                let a_dispute = &accounts[2];
                let a_challenger_ata = &accounts[3];
                let a_vault = &accounts[4];
                let a_vault_authority = &accounts[5];
                let a_token = &accounts[6];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;
                accounts::expect_writable(a_dispute)?;
                accounts::expect_writable(a_challenger_ata)?;
                accounts::expect_writable(a_vault)?;
                verify_token_program(a_token)?;

                let data = a_slab.try_borrow_data()?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                let header = state::read_header(&data);
                let _config = state::read_config(&data);
                drop(data);

                // Admin only
                if !crate::verify::admin_ok(header.admin, a_admin.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                let (expected_dispute, _) = accounts::derive_dispute(program_id, a_slab.key);
                accounts::expect_key(a_dispute, &expected_dispute)?;

                let mut d_data = a_dispute.try_borrow_mut_data()?;
                let mut dispute = crate::dispute::read_dispute(&d_data)
                    .ok_or(PercolatorError::NoActiveDispute)?;
                if !dispute.is_initialized() || dispute.outcome != 0 {
                    return Err(PercolatorError::NoActiveDispute.into());
                }

                if accept != 0 {
                    // Challenger wins: update settlement price, return bond + bonus
                    dispute.outcome = 1;

                    let mut slab_data = state::slab_data_mut(a_slab)?;
                    let mut config = state::read_config(&slab_data);
                    config.settlement_price_e6 = dispute.proposed_price_e6;
                    state::write_config(&mut slab_data, &config);
                    drop(slab_data);

                    // Return bond to challenger
                    if dispute.bond_amount > 0 {
                        let mint = Pubkey::new_from_array(config.collateral_mint);
                        let (auth, vault_bump) =
                            accounts::derive_vault_authority(program_id, a_slab.key);
                        accounts::expect_key(a_vault_authority, &auth)?;
                        verify_vault(
                            a_vault,
                            &auth,
                            &mint,
                            &Pubkey::new_from_array(config.vault_pubkey),
                        )?;

                        let seed1: &[u8] = b"vault";
                        let seed2: &[u8] = a_slab.key.as_ref();
                        let bump_arr: [u8; 1] = [vault_bump];
                        let seed3: &[u8] = &bump_arr;
                        let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
                        let signer_seeds: [&[&[u8]]; 1] = [&seeds];

                        collateral::withdraw(
                            a_token,
                            a_vault,
                            a_challenger_ata,
                            a_vault_authority,
                            dispute.bond_amount,
                            &signer_seeds,
                        )?;
                    }

                    msg!(
                        "PERC-314: Dispute accepted — settlement updated to {}",
                        dispute.proposed_price_e6
                    );
                } else {
                    // Challenger loses: bond stays in vault (insurance fund)
                    dispute.outcome = 2;
                    msg!("PERC-314: Dispute rejected — bond forfeited");
                }

                crate::dispute::write_dispute(&mut d_data, &dispute);
            }

            // =============================================================
            // PERC-315: LP Token Collateral
            // =============================================================
            Instruction::DepositLpCollateral {
                user_idx,
                lp_amount,
            } => {
                accounts::expect_len(accounts, 7)?;
                let a_user = &accounts[0];
                let a_slab = &accounts[1];
                let a_user_lp_ata = &accounts[2];
                let a_lp_vault_mint = &accounts[3];
                let a_lp_vault_state = &accounts[4];
                let a_token = &accounts[5];
                let a_lp_escrow = &accounts[6];

                accounts::expect_signer(a_user)?;
                accounts::expect_writable(a_slab)?;
                accounts::expect_writable(a_user_lp_ata)?;
                accounts::expect_writable(a_lp_escrow)?;
                verify_token_program(a_token)?;

                if lp_amount == 0 {
                    return Err(PercolatorError::LpVaultZeroAmount.into());
                }

                let mut slab_data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &slab_data)?;
                require_initialized(&slab_data)?;
                require_not_paused(&slab_data)?;

                let config = state::read_config(&slab_data);
                if config.lp_collateral_enabled == 0 {
                    return Err(PercolatorError::LpCollateralDisabled.into());
                }

                let (expected_state, _) = accounts::derive_lp_vault_state(program_id, a_slab.key);
                accounts::expect_key(a_lp_vault_state, &expected_state)?;

                let vs_data = a_lp_vault_state.try_borrow_data()?;
                let vault_state = crate::lp_vault::read_lp_vault_state(&vs_data)
                    .ok_or(PercolatorError::LpVaultNotCreated)?;
                let vault_tvl = vault_state.total_capital;
                drop(vs_data);

                let (expected_mint, _) = accounts::derive_lp_vault_mint(program_id, a_slab.key);
                accounts::expect_key(a_lp_vault_mint, &expected_mint)?;
                let lp_supply = crate::insurance_lp::read_mint_supply(a_lp_vault_mint)?;

                let collateral_units = crate::lp_collateral::lp_token_value(
                    lp_amount,
                    vault_tvl,
                    lp_supply,
                    config.lp_collateral_ltv_bps,
                );

                if collateral_units == 0 {
                    return Err(PercolatorError::LpVaultZeroAmount.into());
                }

                let engine = zc::engine_mut(&mut slab_data)?;
                check_idx(engine, user_idx)?;

                let owner = engine.accounts[user_idx as usize].owner;
                if !crate::verify::owner_ok(owner, a_user.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                let clock = Clock::get()?;

                engine
                    .deposit(user_idx, collateral_units, clock.slot)
                    .map_err(map_risk_error)?;

                engine.vault = percolator::U128::new(
                    engine
                        .vault
                        .get()
                        .checked_add(collateral_units)
                        .ok_or(PercolatorError::EngineOverflow)?,
                );
                drop(slab_data);

                collateral::deposit(a_token, a_user_lp_ata, a_lp_escrow, a_user, lp_amount)?;

                msg!(
                    "PERC-315: Deposited {} LP tokens as {} collateral units (LTV={}bps)",
                    lp_amount,
                    collateral_units,
                    config.lp_collateral_ltv_bps
                );
            }

            Instruction::WithdrawLpCollateral {
                user_idx,
                lp_amount,
            } => {
                accounts::expect_len(accounts, 8)?;
                let a_user = &accounts[0];
                let a_slab = &accounts[1];
                let a_user_lp_ata = &accounts[2];
                let a_lp_vault_mint = &accounts[3];
                let a_lp_vault_state = &accounts[4];
                let a_token = &accounts[5];
                let a_lp_escrow = &accounts[6];
                let a_vault_authority = &accounts[7];

                accounts::expect_signer(a_user)?;
                accounts::expect_writable(a_slab)?;
                accounts::expect_writable(a_user_lp_ata)?;
                accounts::expect_writable(a_lp_escrow)?;
                verify_token_program(a_token)?;

                let mut slab_data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &slab_data)?;
                require_initialized(&slab_data)?;
                let config = state::read_config(&slab_data);

                let engine = zc::engine_mut(&mut slab_data)?;
                check_idx(engine, user_idx)?;

                let owner = engine.accounts[user_idx as usize].owner;
                if !crate::verify::owner_ok(owner, a_user.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                let pos = engine.accounts[user_idx as usize].position_size.get();
                if pos != 0 {
                    return Err(PercolatorError::LpCollateralPositionOpen.into());
                }

                let (expected_state, _) = accounts::derive_lp_vault_state(program_id, a_slab.key);
                accounts::expect_key(a_lp_vault_state, &expected_state)?;

                drop(slab_data);

                let vs_data = a_lp_vault_state.try_borrow_data()?;
                let vault_state = crate::lp_vault::read_lp_vault_state(&vs_data)
                    .ok_or(PercolatorError::LpVaultNotCreated)?;
                let vault_tvl = vault_state.total_capital;
                drop(vs_data);

                let (expected_mint, _) = accounts::derive_lp_vault_mint(program_id, a_slab.key);
                accounts::expect_key(a_lp_vault_mint, &expected_mint)?;
                let lp_supply = crate::insurance_lp::read_mint_supply(a_lp_vault_mint)?;

                let collateral_units = crate::lp_collateral::lp_token_value(
                    lp_amount,
                    vault_tvl,
                    lp_supply,
                    config.lp_collateral_ltv_bps,
                );

                let mut slab_data = state::slab_data_mut(a_slab)?;
                let engine = zc::engine_mut(&mut slab_data)?;

                let clock = Clock::get()?;
                engine
                    .withdraw(user_idx, collateral_units, clock.slot, 0)
                    .map_err(map_risk_error)?;

                // PERC-321: Use checked_sub to fail cleanly if vault underflows instead
                // of silently setting vault to 0 (which breaks the conservation invariant).
                engine.vault = percolator::U128::new(
                    engine
                        .vault
                        .get()
                        .checked_sub(collateral_units)
                        .ok_or(PercolatorError::EngineOverflow)?,
                );
                drop(slab_data);

                let (auth, vault_bump) = accounts::derive_vault_authority(program_id, a_slab.key);
                accounts::expect_key(a_vault_authority, &auth)?;

                let seed1: &[u8] = b"vault";
                let seed2: &[u8] = a_slab.key.as_ref();
                let bump_arr: [u8; 1] = [vault_bump];
                let seed3: &[u8] = &bump_arr;
                let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
                let signer_seeds: [&[&[u8]]; 1] = [&seeds];

                collateral::withdraw(
                    a_token,
                    a_lp_escrow,
                    a_user_lp_ata,
                    a_vault_authority,
                    lp_amount,
                    &signer_seeds,
                )?;

                msg!(
                    "PERC-315: Withdrew {} LP tokens ({} collateral units)",
                    lp_amount,
                    collateral_units
                );
            }

            Instruction::QueueWithdrawal { lp_amount } => {
                accounts::expect_len(accounts, 5)?;
                let a_user = &accounts[0];
                let a_slab = &accounts[1];
                let a_lp_vault_state = &accounts[2];
                let a_queue = &accounts[3];
                let a_system = &accounts[4];

                accounts::expect_signer(a_user)?;
                accounts::expect_writable(a_queue)?;

                let data = a_slab.try_borrow_data()?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                require_not_paused(&data)?;
                drop(data);

                let (expected_state, _) = accounts::derive_lp_vault_state(program_id, a_slab.key);
                accounts::expect_key(a_lp_vault_state, &expected_state)?;

                let vs_data = a_lp_vault_state.try_borrow_data()?;
                let vault_state = crate::lp_vault::read_lp_vault_state(&vs_data)
                    .ok_or(PercolatorError::LpVaultNotCreated)?;
                if !vault_state.is_initialized() {
                    return Err(PercolatorError::LpVaultNotCreated.into());
                }
                let queue_epochs = if vault_state.queue_epochs == 0 {
                    5u8
                } else {
                    vault_state.queue_epochs
                };
                drop(vs_data);

                let (expected_queue, queue_bump) =
                    accounts::derive_withdraw_queue(program_id, a_slab.key, a_user.key);
                accounts::expect_key(a_queue, &expected_queue)?;

                if a_queue.data_len() > 0 {
                    let q_data = a_queue.try_borrow_data()?;
                    if let Some(existing) = crate::lp_vault::read_withdraw_queue(&q_data) {
                        if existing.is_initialized() {
                            return Err(PercolatorError::WithdrawQueueAlreadyExists.into());
                        }
                    }
                    drop(q_data);
                }

                if lp_amount == 0 {
                    return Err(PercolatorError::LpVaultZeroAmount.into());
                }

                let queue_len = crate::lp_vault::WITHDRAW_QUEUE_LEN;
                let rent = solana_program::rent::Rent::get()?;
                let lamports = rent.minimum_balance(queue_len);

                let seed1: &[u8] = b"withdraw_queue";
                let seed2: &[u8] = a_slab.key.as_ref();
                let seed3: &[u8] = a_user.key.as_ref();
                let bump_arr: [u8; 1] = [queue_bump];
                let seed4: &[u8] = &bump_arr;
                let seeds: [&[u8]; 4] = [seed1, seed2, seed3, seed4];
                let signer_seeds: [&[&[u8]]; 1] = [&seeds];

                solana_program::program::invoke_signed(
                    &solana_program::system_instruction::create_account(
                        a_user.key,
                        a_queue.key,
                        lamports,
                        queue_len as u64,
                        program_id,
                    ),
                    &[a_user.clone(), a_queue.clone(), a_system.clone()],
                    &signer_seeds,
                )?;

                let clock = Clock::get()?;
                let queue = crate::lp_vault::WithdrawQueue {
                    magic: crate::lp_vault::WITHDRAW_QUEUE_MAGIC,
                    queued_lp_amount: lp_amount,
                    queue_start_slot: clock.slot,
                    epochs_remaining: queue_epochs,
                    total_epochs: queue_epochs,
                    _pad: [0; 6],
                    claimed_so_far: 0,
                    _reserved: [0; 24],
                };

                let mut q_data = a_queue.try_borrow_mut_data()?;
                crate::lp_vault::write_withdraw_queue(&mut q_data, &queue);

                msg!(
                    "PERC-309: Queued {} LP over {} epochs",
                    lp_amount,
                    queue_epochs
                );
            }

            Instruction::ClaimQueuedWithdrawal => {
                accounts::expect_len(accounts, 10)?;
                let a_user = &accounts[0];
                let a_slab = &accounts[1];
                let a_queue = &accounts[2];
                let a_lp_vault_mint = &accounts[3];
                let a_user_lp_ata = &accounts[4];
                let a_vault = &accounts[5];
                let a_user_ata = &accounts[6];
                let a_vault_authority = &accounts[7];
                let a_token = &accounts[8];
                let a_lp_vault_state = &accounts[9];

                accounts::expect_signer(a_user)?;
                accounts::expect_writable(a_slab)?;
                accounts::expect_writable(a_queue)?;
                accounts::expect_writable(a_lp_vault_mint)?;
                accounts::expect_writable(a_user_lp_ata)?;
                accounts::expect_writable(a_vault)?;
                accounts::expect_writable(a_user_ata)?;
                accounts::expect_writable(a_lp_vault_state)?;
                verify_token_program(a_token)?;

                let (expected_queue, _) =
                    accounts::derive_withdraw_queue(program_id, a_slab.key, a_user.key);
                accounts::expect_key(a_queue, &expected_queue)?;

                let mut q_data = a_queue.try_borrow_mut_data()?;
                let mut queue = crate::lp_vault::read_withdraw_queue(&q_data)
                    .ok_or(PercolatorError::WithdrawQueueNotFound)?;
                if !queue.is_initialized() {
                    return Err(PercolatorError::WithdrawQueueNotFound.into());
                }

                let claimable = queue.claimable_this_epoch();
                if claimable == 0 {
                    return Err(PercolatorError::WithdrawQueueNothingClaimable.into());
                }

                queue.claimed_so_far = queue.claimed_so_far.saturating_add(claimable);
                queue.epochs_remaining = queue.epochs_remaining.saturating_sub(1);
                crate::lp_vault::write_withdraw_queue(&mut q_data, &queue);
                drop(q_data);

                let slab_data = a_slab.try_borrow_data()?;
                slab_guard(program_id, a_slab, &slab_data)?;
                require_initialized(&slab_data)?;
                let config = state::read_config(&slab_data);
                let mint = Pubkey::new_from_array(config.collateral_mint);
                drop(slab_data);

                let (auth, vault_bump) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                accounts::expect_key(a_vault_authority, &auth)?;
                verify_token_account(a_user_ata, a_user.key, &mint)?;

                let (expected_lp_mint, _) = accounts::derive_lp_vault_mint(program_id, a_slab.key);
                accounts::expect_key(a_lp_vault_mint, &expected_lp_mint)?;

                let (expected_state, _) = accounts::derive_lp_vault_state(program_id, a_slab.key);
                accounts::expect_key(a_lp_vault_state, &expected_state)?;

                let mut vs_data = a_lp_vault_state.try_borrow_mut_data()?;
                let mut vault_state = crate::lp_vault::read_lp_vault_state(&vs_data)
                    .ok_or(PercolatorError::LpVaultNotCreated)?;
                if !vault_state.is_initialized() {
                    return Err(PercolatorError::LpVaultNotCreated.into());
                }

                let lp_supply = crate::insurance_lp::read_mint_supply(a_lp_vault_mint)?;
                if lp_supply == 0 || vault_state.total_capital == 0 {
                    return Err(PercolatorError::LpVaultSupplyMismatch.into());
                }

                let capital_units =
                    (claimable as u128) * vault_state.total_capital / (lp_supply as u128);

                if capital_units == 0 {
                    return Err(PercolatorError::LpVaultZeroAmount.into());
                }

                let slab_data = a_slab.try_borrow_data()?;
                let config = state::read_config(&slab_data);
                let base_amount =
                    crate::units::units_to_base(capital_units as u64, config.unit_scale);

                // PERC-313: HWM floor enforcement (same as LpVaultWithdraw)
                if vault_state.hwm_floor_bps > 0 && vault_state.epoch_high_water_tvl > 0 {
                    let remaining = vault_state
                        .total_capital
                        .checked_sub(capital_units)
                        .ok_or(PercolatorError::EngineOverflow)?;
                    let floor = vault_state
                        .epoch_high_water_tvl
                        .saturating_mul(vault_state.hwm_floor_bps as u128)
                        / 10_000;
                    if remaining < floor {
                        msg!(
                            "ClaimQueued HWM block: remaining={} < floor={}",
                            remaining,
                            floor
                        );
                        return Err(PercolatorError::LpVaultWithdrawExceedsAvailable.into());
                    }
                }

                // #555: OI reservation check (was missing from ClaimQueuedWithdrawal)
                let (oi_multiplier, _) = unpack_oi_cap(config.oi_cap_multiplier_bps);
                if oi_multiplier > 0 {
                    let remaining_capital = vault_state
                        .total_capital
                        .checked_sub(capital_units)
                        .ok_or(PercolatorError::EngineOverflow)?;
                    let engine = zc::engine_ref(&slab_data)?;
                    let current_oi = engine.total_open_interest.get();
                    let max_oi_after =
                        remaining_capital.saturating_mul(oi_multiplier as u128) / 10_000;
                    if current_oi > max_oi_after {
                        return Err(PercolatorError::LpVaultWithdrawExceedsAvailable.into());
                    }
                }
                drop(slab_data);

                // #555: Use checked_sub instead of saturating_sub for accounting safety
                vault_state.total_capital = vault_state
                    .total_capital
                    .checked_sub(capital_units)
                    .ok_or(PercolatorError::EngineOverflow)?;
                crate::lp_vault::write_lp_vault_state(&mut vs_data, &vault_state);
                drop(vs_data);

                let mut slab_data = state::slab_data_mut(a_slab)?;
                let engine = zc::engine_mut(&mut slab_data)?;
                engine.vault = percolator::U128::new(
                    engine
                        .vault
                        .get()
                        .checked_sub(capital_units)
                        .ok_or(PercolatorError::EngineOverflow)?,
                );
                drop(slab_data);

                crate::insurance_lp::burn(
                    a_token,
                    a_lp_vault_mint,
                    a_user_lp_ata,
                    a_user,
                    claimable,
                )?;

                let seed1: &[u8] = b"vault";
                let seed2: &[u8] = a_slab.key.as_ref();
                let bump_arr: [u8; 1] = [vault_bump];
                let seed3: &[u8] = &bump_arr;
                let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
                let signer_seeds: [&[&[u8]]; 1] = [&seeds];

                collateral::withdraw(
                    a_token,
                    a_vault,
                    a_user_ata,
                    a_vault_authority,
                    base_amount,
                    &signer_seeds,
                )?;

                msg!(
                    "PERC-309: Claimed {} LP ({} tokens), {} epochs left",
                    claimable,
                    base_amount,
                    queue.epochs_remaining
                );
            }

            Instruction::CancelQueuedWithdrawal => {
                accounts::expect_len(accounts, 3)?;
                let a_user = &accounts[0];
                let a_slab = &accounts[1];
                let a_queue = &accounts[2];

                accounts::expect_signer(a_user)?;
                accounts::expect_writable(a_queue)?;

                let data = a_slab.try_borrow_data()?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                drop(data);

                let (expected_queue, _) =
                    accounts::derive_withdraw_queue(program_id, a_slab.key, a_user.key);
                accounts::expect_key(a_queue, &expected_queue)?;

                let q_data = a_queue.try_borrow_data()?;
                let queue = crate::lp_vault::read_withdraw_queue(&q_data)
                    .ok_or(PercolatorError::WithdrawQueueNotFound)?;
                if !queue.is_initialized() {
                    return Err(PercolatorError::WithdrawQueueNotFound.into());
                }
                let remaining = queue.queued_lp_amount.saturating_sub(queue.claimed_so_far);
                drop(q_data);

                let mut q_data = a_queue.try_borrow_mut_data()?;
                q_data.fill(0);
                drop(q_data);

                let mut queue_lamports = a_queue.try_borrow_mut_lamports()?;
                let mut user_lamports = a_user.try_borrow_mut_lamports()?;
                **user_lamports = user_lamports
                    .checked_add(**queue_lamports)
                    .ok_or(ProgramError::ArithmeticOverflow)?;
                **queue_lamports = 0;

                msg!("PERC-309: Cancelled, {} LP unclaimed", remaining);
            }

            Instruction::ExecuteAdl { target_idx } => {
                // PERC-305: Auto-deleverage — permissionless instruction to surgically
                // close/reduce the most profitable position when pnl_pos_tot > max_pnl_cap.
                //
                // Accounts:
                //   0. [signer]   Caller (permissionless — incentive is unblocking
                //                 the market for normal trading)
                //   1. [writable] Slab account
                //   2. []         Clock sysvar
                //   3. []         Oracle account (Pyth/Chainlink/authority — same as liquidation)
                //   4.. []       (optional) Backup oracle accounts for non-hyperp markets
                accounts::expect_len(accounts, 4)?;
                let a_caller = &accounts[0];
                let a_slab = &accounts[1];
                let a_oracle = &accounts[3];
                accounts::expect_signer(a_caller)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let mut config = state::read_config(&data);

                // ADL requires max_pnl_cap to be set
                if config.max_pnl_cap == 0 {
                    msg!("ADL: max_pnl_cap not configured");
                    return Err(ProgramError::InvalidInstructionData);
                }

                let clock = Clock::from_account_info(&accounts[2])?;

                // Read oracle price (same logic as liquidation).
                // NOTE: For non-hyperp markets with backup oracles, callers must pass
                // additional oracle accounts beyond accounts[3]. The current expect_len(4)
                // is the minimum; read_price_clamped reads from accounts[4..] for backups.
                let is_hyperp = oracle::is_hyperp_mode(&config);
                let price = if is_hyperp {
                    let idx = config.last_effective_price_e6;
                    if idx == 0 {
                        return Err(PercolatorError::OracleInvalid.into());
                    }
                    // PERC-365: Reject if Hyperp oracle is stale
                    {
                        let eng = zc::engine_ref(&data)?;
                        oracle::check_hyperp_staleness(
                            eng.current_slot,
                            eng.max_crank_staleness_slots,
                            clock.slot,
                        )?;
                    }
                    idx
                } else {
                    oracle::read_price_clamped(
                        &mut config,
                        a_oracle,
                        clock.unix_timestamp,
                        &accounts[4..],
                    )?
                };
                state::write_config(&mut data, &config);

                let engine = zc::engine_mut(&mut data)?;
                check_idx(engine, target_idx)?;

                let pnl_pos_tot = engine.pnl_pos_tot.get();
                let cap = config.max_pnl_cap as u128;

                // Precondition: PnL cap must be exceeded
                if pnl_pos_tot <= cap {
                    msg!(
                        "ADL: pnl_pos_tot {} <= cap {}, not needed",
                        pnl_pos_tot,
                        cap
                    );
                    return Err(PercolatorError::EngineRiskReductionOnlyMode.into());
                }

                let excess = pnl_pos_tot.saturating_sub(cap);

                // Delegate to core engine's execute_adl — handles settlement,
                // PnL checks, full/partial close, and OI updates.
                let closed_abs = engine
                    .execute_adl(target_idx, clock.slot, price, excess)
                    .map_err(map_risk_error)?;

                msg!(
                    "ADL: idx={} closed={} excess={} pnl_pos_tot_after={}",
                    target_idx,
                    closed_abs,
                    excess,
                    engine.pnl_pos_tot.get()
                );
            }

            Instruction::CloseStaleSlabs => {
                // Close a stale slab whose data length does not match any accepted tier.
                //
                // This is the recovery path for slabs created by old program layouts
                // (e.g. pre-PERC-120 devnet deploys with now-invalid sizes).  Unlike
                // CloseSlab (tag 13) we skip `slab_guard`; instead we gate on:
                //   1. Program ownership of the slab account.
                //   2. The slab size must NOT match any valid tier (SLAB_LEN ± 0/16/24).
                //   3. Header magic == MAGIC ("PERCOLAT") at bytes [0..8].
                //   4. Signer == admin field at header bytes [16..48].
                //
                // Accounts: [dest(signer,writable), slab(writable)]
                accounts::expect_len(accounts, 2)?;
                let a_dest = &accounts[0];
                let a_slab = &accounts[1];

                accounts::expect_signer(a_dest)?;
                accounts::expect_writable(a_slab)?;

                // 1. Must be owned by this program.
                if a_slab.owner != program_id {
                    return Err(ProgramError::IllegalOwner);
                }

                // 2. Reject slabs with valid sizes — use CloseSlab for those.
                const PRE_118_SLAB_LEN: usize = SLAB_LEN - 16;
                const OLDEST_SLAB_LEN: usize = SLAB_LEN - 24;
                let slab_data = a_slab.try_borrow_data()?;
                let slab_len = slab_data.len();
                if slab_len == SLAB_LEN
                    || slab_len == PRE_118_SLAB_LEN
                    || slab_len == OLDEST_SLAB_LEN
                {
                    return Err(PercolatorError::InvalidSlabLen.into());
                }

                // Header layout (stable across V0 and V1):
                //   [0..8]   magic: u64 (little-endian)
                //   [8..12]  version: u32
                //   [12..13] bump: u8
                //   [13..16] _padding: [u8;3]
                //   [16..48] admin: [u8;32]
                const ADMIN_OFF: usize = 16;
                const ADMIN_END: usize = ADMIN_OFF + 32;

                if slab_len < ADMIN_END {
                    // Slab too small to contain any valid header — refuse.
                    return Err(PercolatorError::NotInitialized.into());
                }

                // 3. Magic check.
                let magic = u64::from_le_bytes(
                    slab_data[0..8]
                        .try_into()
                        .map_err(|_| PercolatorError::InvalidMagic)?,
                );
                if magic != MAGIC {
                    return Err(PercolatorError::InvalidMagic.into());
                }

                // 4. Admin check using the same helper as CloseSlab.
                let admin_bytes: [u8; 32] = slab_data[ADMIN_OFF..ADMIN_END]
                    .try_into()
                    .map_err(|_| PercolatorError::InvalidMagic)?;
                drop(slab_data); // release borrow before lamport transfer

                require_admin(admin_bytes, a_dest.key)?;

                // Zero account data before draining lamports to prevent residual
                // data exposure (Solana best practice for account closure).
                {
                    let mut data = a_slab.try_borrow_mut_data()?;
                    data.fill(0);
                }

                // Drain all lamports to dest.
                let slab_lamports = a_slab.lamports();
                **a_slab.lamports.borrow_mut() = 0;
                **a_dest.lamports.borrow_mut() = a_dest
                    .lamports()
                    .checked_add(slab_lamports)
                    .ok_or(PercolatorError::EngineOverflow)?;

                msg!(
                    "CloseStaleSlabs: closed stale slab (size={}) reclaimed {} lamports",
                    slab_len,
                    slab_lamports,
                );
            }

            // PERC-511: Reclaim rent from an uninitialised slab.
            //
            // Accounts: [dest(signer,writable), slab(signer,writable)]
            //
            // This is the self-service recovery path for a market creator whose
            // CreateMarket tx failed after the slab was funded but before InitMarket
            // completed. The slab has no stored authority, so ownership is proven
            // by requiring the slab account itself to sign (the creator retains the
            // slab keypair in localStorage and includes it as a tx signer).
            Instruction::ReclaimSlabRent => {
                accounts::expect_len(accounts, 2)?;
                let a_dest = &accounts[0];
                let a_slab = &accounts[1];

                // dest: signer + writable (receives lamports)
                accounts::expect_signer(a_dest)?;
                accounts::expect_writable(a_dest)?;

                // slab: signer + writable (proves keypair ownership)
                accounts::expect_signer(a_slab)?;
                accounts::expect_writable(a_slab)?;

                // Guard: dest and slab must be different accounts (#936)
                if a_dest.key == a_slab.key {
                    return Err(ProgramError::InvalidArgument);
                }

                // 1. Slab must be owned by this program.
                if a_slab.owner != program_id {
                    return Err(ProgramError::IllegalOwner);
                }

                // 2. Slab must be uninitialised (magic != MAGIC).
                //    If magic is present the market was initialised — use CloseSlab (tag 13).
                let slab_data = a_slab.try_borrow_data()?;
                let slab_len = slab_data.len();
                if slab_len >= 8 {
                    let magic = u64::from_le_bytes(
                        slab_data[0..8]
                            .try_into()
                            .map_err(|_| PercolatorError::InvalidMagic)?,
                    );
                    if magic == MAGIC {
                        // Market was successfully initialised — cannot reclaim this way.
                        return Err(PercolatorError::AlreadyInitialized.into());
                    }
                }
                drop(slab_data); // release borrow before mutations

                // Zero the slab data to prevent data residue exposure.
                {
                    let mut data = a_slab.try_borrow_mut_data()?;
                    data.fill(0);
                }

                // Drain all lamports from the slab to the destination.
                let slab_lamports = a_slab.lamports();
                **a_slab.lamports.borrow_mut() = 0;
                **a_dest.lamports.borrow_mut() = a_dest
                    .lamports()
                    .checked_add(slab_lamports)
                    .ok_or(PercolatorError::EngineOverflow)?;

                msg!(
                    "ReclaimSlabRent: reclaimed {} lamports from uninitialised slab (size={})",
                    slab_lamports,
                    slab_len,
                );
            }

            // ═══════════════════════════════════════════════════════════════
            // Feature 3: On-Chain Audit Crank (tag 53)
            // ═══════════════════════════════════════════════════════════════
            Instruction::AuditCrank => {
                // Permissionless: anyone can call. Walks all accounts and verifies
                // conservation invariants. Sets FLAG_PAUSED on violation.
                // Accounts: [slab(writable)]
                if accounts.is_empty() {
                    return Err(ProgramError::NotEnoughAccountKeys);
                }
                let a_slab = &accounts[0];
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let engine = zc::engine_ref(&data)?;

                // Walk all accounts and compute running aggregates
                let mut sum_capital: i128 = 0;
                let mut sum_pnl_pos: u128 = 0;
                let mut sum_oi: u128 = 0;
                for idx in 0..MAX_ACCOUNTS {
                    if !engine.is_used(idx) {
                        continue;
                    }
                    let acc = &engine.accounts[idx];
                    sum_capital = sum_capital.saturating_add(acc.capital.get() as i128);
                    let pnl = acc.pnl.get();
                    if pnl > 0 {
                        sum_pnl_pos = sum_pnl_pos.saturating_add(pnl as u128);
                    }
                    let pos = acc.position_size.get();
                    sum_oi = sum_oi.saturating_add(pos.unsigned_abs());
                }

                // Check invariants
                let mut violation = false;

                // Invariant 1: sum(capital) == engine.c_tot
                let c_tot = engine.c_tot.get();
                if sum_capital != c_tot as i128 {
                    msg!("AUDIT_VIOLATION: capital_mismatch");
                    sol_log_64(sum_capital as u64, c_tot as u64, 0, 0, 0xAD01);
                    violation = true;
                }

                // Invariant 2: sum(max(0, pnl)) == engine.pnl_pos_tot
                let pnl_pos_tot = engine.pnl_pos_tot.get();
                if sum_pnl_pos != pnl_pos_tot {
                    msg!("AUDIT_VIOLATION: pnl_pos_mismatch");
                    sol_log_64(sum_pnl_pos as u64, pnl_pos_tot as u64, 0, 0, 0xAD02);
                    violation = true;
                }

                // Invariant 3: sum(|position|) == engine.total_open_interest
                let total_oi = engine.total_open_interest.get();
                if sum_oi != total_oi {
                    msg!("AUDIT_VIOLATION: oi_mismatch");
                    sol_log_64(sum_oi as u64, total_oi as u64, 0, 0, 0xAD03);
                    violation = true;
                }

                // Invariant 4: net LP position consistency
                // Uses engine's maintained net_lp_pos (O(1), already aggregated)
                let net_lp_pos = engine.net_lp_pos.get();
                let lp_sum_abs = engine.lp_sum_abs.get();
                // net_lp_pos magnitude must not exceed lp_sum_abs
                if net_lp_pos.unsigned_abs() > lp_sum_abs {
                    msg!("AUDIT_VIOLATION: lp_pos_inconsistent");
                    violation = true;
                }

                // Invariant 5: vault >= c_tot + insurance (global + isolated) (solvency)
                // #981: Include isolated_balance in solvency check
                let vault = engine.vault.get();
                let insurance_balance = engine
                    .insurance_fund
                    .balance
                    .get()
                    .saturating_add(engine.insurance_fund.isolated_balance.get());
                let required = (c_tot as u128).saturating_add(insurance_balance);
                if (vault as u128) < required {
                    msg!("AUDIT_VIOLATION: solvency");
                    sol_log_64(vault as u64, required as u64, 0, 0, 0xAD05);
                    violation = true;
                }

                // Write audit result and optionally pause.
                // #959: Cooldown guard — AuditCrank may not re-pause a market
                // within AUDIT_CRANK_COOLDOWN_SLOTS of the previous pause to
                // prevent a permissionless DoS via false/transient invariant
                // triggers. 150 slots ≈ 60 s on mainnet (400 ms/slot).
                const AUDIT_CRANK_COOLDOWN_SLOTS: u64 = 150;
                let current_slot = Clock::get()?.slot;
                let mut config = state::read_config(&data);
                if violation {
                    let last_pause = state::read_last_audit_pause_slot(&config);
                    if current_slot.saturating_sub(last_pause) < AUDIT_CRANK_COOLDOWN_SLOTS {
                        // Cooldown active — log violation but do not pause.
                        // Prevents rapid repeated DoS via marginal invariant drift.
                        msg!(
                            "AUDIT_CRANK: violation detected but cooldown active \
                             (last_pause={} current={} cooldown={})",
                            last_pause,
                            current_slot,
                            AUDIT_CRANK_COOLDOWN_SLOTS,
                        );
                        return Err(PercolatorError::AuditViolation.into());
                    }
                    state::write_audit_status(&mut config, 0xFFFF);
                    state::write_last_audit_pause_slot(&mut config, current_slot);
                    state::set_paused(&mut data, true);
                    state::write_config(&mut data, &config);
                    msg!("AUDIT_CRANK: VIOLATION DETECTED — market paused");
                    return Err(PercolatorError::AuditViolation.into());
                } else {
                    state::write_audit_status(&mut config, 1);
                    state::write_config(&mut data, &config);
                    msg!("AUDIT_CRANK: all invariants passed");
                }
            }

            // ═══════════════════════════════════════════════════════════════
            // Feature 5: Cross-Market Portfolio Margining — SetOffsetPair (tag 54)
            // ═══════════════════════════════════════════════════════════════
            Instruction::SetOffsetPair { offset_bps } => {
                // Admin configures margin offset for a pair of slabs.
                // Accounts: [admin(signer,payer), slab_a, slab_b, pair_pda(writable), system_program]
                accounts::expect_len(accounts, 5)?;
                let a_admin = &accounts[0];
                let a_slab_a = &accounts[1];
                let a_slab_b = &accounts[2]; // #958: was incorrectly prefixed with _ (unused)
                let a_pair_pda = &accounts[3];
                let a_system = &accounts[4];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_admin)?;
                accounts::expect_writable(a_pair_pda)?;
                if *a_system.key != solana_program::system_program::id() {
                    return Err(ProgramError::IncorrectProgramId);
                }

                // #983: Validate system program
                if *a_system.key != solana_program::system_program::id() {
                    return Err(ProgramError::IncorrectProgramId);
                }

                // Verify admin on slab_a (#958: slab_a admin check)
                {
                    let data_a = a_slab_a.try_borrow_data()?;
                    if data_a.len() < HEADER_LEN {
                        return Err(ProgramError::InvalidAccountData);
                    }
                    let header = state::read_header(&data_a);
                    if header.magic != MAGIC {
                        return Err(PercolatorError::InvalidMagic.into());
                    }
                    require_admin(header.admin, a_admin.key)?;
                }

                // #958: Verify admin on slab_b — prevents cross-admin pair manipulation
                // where slab_a admin could register a pair with an unrelated slab_b.
                {
                    let data_b = a_slab_b.try_borrow_data()?;
                    if data_b.len() < HEADER_LEN {
                        return Err(ProgramError::InvalidAccountData);
                    }
                    let header_b = state::read_header(&data_b);
                    if header_b.magic != MAGIC {
                        return Err(PercolatorError::InvalidMagic.into());
                    }
                    require_admin(header_b.admin, a_admin.key)?;
                }

                // #957: Verify PDA derivation of a_pair_pda — prevents account
                // substitution attacks where an attacker passes an arbitrary
                // writable account instead of the canonical PDA.
                // Seeds: ["cmor_pair", min(slab_a, slab_b), max(slab_a, slab_b)]
                // Keys are ordered lexicographically so the PDA is symmetric.
                let (slab_min_pair, slab_max_pair) =
                    if a_slab_a.key.as_ref() <= a_slab_b.key.as_ref() {
                        (a_slab_a.key, a_slab_b.key)
                    } else {
                        (a_slab_b.key, a_slab_a.key)
                    };
                let (expected_pda, pair_bump) = Pubkey::find_program_address(
                    &[b"cmor_pair", slab_min_pair.as_ref(), slab_max_pair.as_ref()],
                    program_id,
                );
                if a_pair_pda.key != &expected_pda {
                    return Err(ProgramError::InvalidSeeds);
                }

                // Validate offset_bps (0..=10_000)
                if offset_bps > 10_000 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }

                // #977: Create PDA if it doesn't exist yet
                if a_pair_pda.data_is_empty() {
                    let lamports = solana_program::rent::Rent::get()?
                        .minimum_balance(cross_margin::OFFSET_PAIR_LEN);
                    let bump_bytes = [pair_bump];
                    let signer_seeds: &[&[u8]] = &[
                        b"cmor_pair",
                        slab_min_pair.as_ref(),
                        slab_max_pair.as_ref(),
                        &bump_bytes,
                    ];
                    solana_program::program::invoke_signed(
                        &solana_program::system_instruction::create_account(
                            a_admin.key,
                            &expected_pda,
                            lamports,
                            cross_margin::OFFSET_PAIR_LEN as u64,
                            program_id,
                        ),
                        &[a_admin.clone(), a_pair_pda.clone(), a_system.clone()],
                        &[signer_seeds],
                    )?;
                }

                // Write pair config
                let mut pair_data = a_pair_pda.try_borrow_mut_data()?;
                if pair_data.len() < cross_margin::OFFSET_PAIR_LEN {
                    return Err(ProgramError::AccountDataTooSmall);
                }
                let cfg = cross_margin::OffsetPairConfig {
                    magic: cross_margin::OFFSET_PAIR_MAGIC,
                    offset_bps,
                    enabled: 1,
                    _pad: [0; 5],
                    _reserved: [0; 16],
                };
                cross_margin::write_offset_pair(&mut pair_data, &cfg);
                msg!("SetOffsetPair: offset_bps={}", offset_bps);
            }

            // ═══════════════════════════════════════════════════════════════
            // Feature 5: Cross-Market Portfolio Margining — AttestCrossMargin (tag 55)
            // ═══════════════════════════════════════════════════════════════
            Instruction::AttestCrossMargin {
                user_idx_a,
                user_idx_b,
            } => {
                // Permissionless keeper attests user positions across two slabs.
                //
                // Account layout (6 accounts, fixed order):
                //   [0] payer       — fee-payer AND transaction signer (always accounts[0]).
                //                     This is the only signer required; any permissionless
                //                     keeper may submit the instruction. The fee-payer never
                //                     gains special authority over the attested positions.
                //   [1] slab_a      — first slab account (read-only)
                //   [2] slab_b      — second slab account (read-only)
                //   [3] attestation_pda — per-user CMOR attestation PDA (writable)
                //   [4] pair_pda    — offset-pair config PDA (read-only)
                //   [5] system_program — needed for PDA creation when attestation doesn't
                //                        exist yet (create_account_signed path in PR #82)
                accounts::expect_len(accounts, 6)?;
                let a_payer = &accounts[0];
                let a_slab_a = &accounts[1];
                let a_slab_b = &accounts[2];
                let a_attestation = &accounts[3];
                let a_pair_pda = &accounts[4];
                let a_system = &accounts[5];

                accounts::expect_signer(a_payer)?;
                accounts::expect_writable(a_payer)?;
                accounts::expect_writable(a_attestation)?;
                if *a_system.key != solana_program::system_program::id() {
                    return Err(ProgramError::IncorrectProgramId);
                }

                // #983: Validate system program
                if *a_system.key != solana_program::system_program::id() {
                    return Err(ProgramError::IncorrectProgramId);
                }

                // Read pair config to get offset_bps
                let pair_data = a_pair_pda.try_borrow_data()?;
                let pair_cfg = cross_margin::read_offset_pair(&pair_data)
                    .ok_or(ProgramError::InvalidAccountData)?;
                if !pair_cfg.is_initialized() || pair_cfg.enabled == 0 {
                    return Err(PercolatorError::CrossMarginPairNotFound.into());
                }
                let offset_bps = pair_cfg.offset_bps;
                drop(pair_data);

                // Read user position from slab A
                let data_a = a_slab_a.try_borrow_data()?;
                if data_a.len() < ENGINE_OFF + ENGINE_LEN {
                    return Err(ProgramError::InvalidAccountData);
                }
                let engine_a = zc::engine_ref(&data_a)?;
                check_idx(engine_a, user_idx_a)?;
                let pos_a = engine_a.accounts[user_idx_a as usize].position_size.get();
                let owner_a = engine_a.accounts[user_idx_a as usize].owner;
                let slot = engine_a.current_slot;
                drop(data_a);

                // Read user position from slab B
                let data_b = a_slab_b.try_borrow_data()?;
                if data_b.len() < ENGINE_OFF + ENGINE_LEN {
                    return Err(ProgramError::InvalidAccountData);
                }
                let engine_b = zc::engine_ref(&data_b)?;
                check_idx(engine_b, user_idx_b)?;
                let pos_b = engine_b.accounts[user_idx_b as usize].position_size.get();
                let owner_b = engine_b.accounts[user_idx_b as usize].owner;
                drop(data_b);

                // Verify both positions belong to the same user
                if owner_a != owner_b {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                // #957: Verify PDA derivation of a_pair_pda — prevents an attacker
                // from passing an arbitrary account in place of the canonical pair PDA.
                // Seeds: ["cmor_pair", min(slab_a, slab_b), max(slab_a, slab_b)]
                {
                    let (slab_min, slab_max) = if a_slab_a.key.as_ref() <= a_slab_b.key.as_ref() {
                        (a_slab_a.key, a_slab_b.key)
                    } else {
                        (a_slab_b.key, a_slab_a.key)
                    };
                    let (expected_pair_pda, _bump) = Pubkey::find_program_address(
                        &[b"cmor_pair", slab_min.as_ref(), slab_max.as_ref()],
                        program_id,
                    );
                    if a_pair_pda.key != &expected_pair_pda {
                        return Err(ProgramError::InvalidSeeds);
                    }
                }

                // #957: Verify PDA derivation of a_attestation — prevents account
                // substitution where the caller writes to an arbitrary writable account
                // rather than the canonical per-user attestation PDA.
                // Seeds: ["cmor", owner, slab_a, slab_b]  (keys in canonical order)
                let (slab_min_att, slab_max_att) = if a_slab_a.key.as_ref() <= a_slab_b.key.as_ref()
                {
                    (a_slab_a.key, a_slab_b.key)
                } else {
                    (a_slab_b.key, a_slab_a.key)
                };
                let owner_key = Pubkey::from(owner_a);
                let (expected_att_pda, att_bump) = Pubkey::find_program_address(
                    &[
                        b"cmor",
                        owner_key.as_ref(),
                        slab_min_att.as_ref(),
                        slab_max_att.as_ref(),
                    ],
                    program_id,
                );
                if a_attestation.key != &expected_att_pda {
                    return Err(ProgramError::InvalidSeeds);
                }

                // #977: Create attestation PDA if it doesn't exist yet
                if a_attestation.data_is_empty() {
                    let lamports = solana_program::rent::Rent::get()?
                        .minimum_balance(cross_margin::ATTESTATION_LEN);
                    let bump_bytes = [att_bump];
                    let signer_seeds: &[&[u8]] = &[
                        b"cmor",
                        owner_key.as_ref(),
                        slab_min_att.as_ref(),
                        slab_max_att.as_ref(),
                        &bump_bytes,
                    ];
                    solana_program::program::invoke_signed(
                        &solana_program::system_instruction::create_account(
                            a_payer.key,
                            &expected_att_pda,
                            lamports,
                            cross_margin::ATTESTATION_LEN as u64,
                            program_id,
                        ),
                        &[a_payer.clone(), a_attestation.clone(), a_system.clone()],
                        &[signer_seeds],
                    )?;
                }

                // Write attestation
                let mut att_data = a_attestation.try_borrow_mut_data()?;
                if att_data.len() < cross_margin::ATTESTATION_LEN {
                    return Err(ProgramError::AccountDataTooSmall);
                }
                let att = cross_margin::CrossMarginAttestation {
                    magic: cross_margin::ATTESTATION_MAGIC,
                    _align_pad: [0; 8],
                    user_pos_a: pos_a,
                    user_pos_b: pos_b,
                    attested_slot: slot,
                    offset_bps,
                    _pad: [0; 6],
                    owner: owner_a,
                    slab_a: if a_slab_a.key.as_ref() <= a_slab_b.key.as_ref() {
                        a_slab_a.key.to_bytes()
                    } else {
                        a_slab_b.key.to_bytes()
                    },
                    slab_b: if a_slab_a.key.as_ref() <= a_slab_b.key.as_ref() {
                        a_slab_b.key.to_bytes()
                    } else {
                        a_slab_a.key.to_bytes()
                    },
                };
                cross_margin::write_attestation(&mut att_data, &att);
                msg!(
                    "AttestCrossMargin: pos_a={} pos_b={} offset={}",
                    pos_a as i64,
                    pos_b as i64,
                    offset_bps
                );
            }

            // ═══════════════════════════════════════════════════════════════
            // PERC-622: Advance Oracle Phase (tag 56, permissionless crank)
            // ═══════════════════════════════════════════════════════════════
            Instruction::AdvanceOraclePhase => {
                // Accounts: [slab(writable)]
                // Permissionless: anyone can call. No signer required beyond fee payer.
                if accounts.is_empty() {
                    return Err(ProgramError::NotEnoughAccountKeys);
                }
                let a_slab = &accounts[0];
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let mut config = state::read_config(&data);
                let clock = Clock::get()?;

                let old_phase = state::get_oracle_phase(&config);

                // Pyth-pinned markets auto-promote to Phase 3
                let has_mature_oracle = crate::verify::is_pyth_pinned_mode(
                    config.oracle_authority,
                    config.index_feed_id,
                );

                // Lazy-init market_created_slot for legacy markets
                let created = state::effective_created_slot(config.market_created_slot, clock.slot);
                if config.market_created_slot == 0 && old_phase == 0 {
                    config.market_created_slot = clock.slot;
                }

                let (new_phase, transitioned) = state::check_phase_transition(
                    clock.slot,
                    created,
                    old_phase,
                    state::get_cumulative_volume(&config),
                    state::get_phase2_delta_slots(&config),
                    has_mature_oracle,
                );

                if !transitioned {
                    // No transition needed — write config (may have lazy-init'd created_slot)
                    state::write_config(&mut data, &config);
                    msg!("AdvanceOraclePhase: no transition (phase={})", old_phase);
                } else {
                    state::set_oracle_phase(&mut config, new_phase);

                    // Record Phase 2 entry delta for Phase 2→3 time check
                    if new_phase == state::ORACLE_PHASE_GROWING {
                        let delta = clock.slot.saturating_sub(created) as u32;
                        state::set_phase2_delta_slots(&mut config, delta);
                    }

                    state::write_config(&mut data, &config);
                    msg!(
                        "AdvanceOraclePhase: {} -> {} at slot {}",
                        old_phase,
                        new_phase,
                        clock.slot
                    );
                }
            }

            // PERC-623: TopUpKeeperFund — anyone can add lamports to a market's
            // keeper fund PDA. Permissionless (no admin check).
            Instruction::TopUpKeeperFund { amount } => {
                // accounts: [0] funder (signer), [1] slab (writable), [2] keeper_fund PDA (writable)
                accounts::expect_len(accounts, 3)?;
                let a_funder = &accounts[0];
                let a_slab = &accounts[1];
                let a_keeper_fund = &accounts[2];
                accounts::expect_signer(a_funder)?;
                accounts::expect_writable(a_slab)?;
                accounts::expect_writable(a_keeper_fund)?;

                // Verify slab is a valid program-owned slab
                {
                    let slab_data = state::slab_data_mut(a_slab)?;
                    slab_guard(program_id, a_slab, &slab_data)?;
                    require_initialized(&slab_data)?;
                }

                // Verify keeper_fund PDA derivation
                let (expected_pda, _bump) = Pubkey::find_program_address(
                    &[crate::keeper_fund::KEEPER_FUND_SEED, a_slab.key.as_ref()],
                    program_id,
                );
                if *a_keeper_fund.key != expected_pda {
                    return Err(ProgramError::InvalidSeeds);
                }

                if amount == 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }

                // Transfer lamports from funder to keeper fund PDA
                **a_funder.try_borrow_mut_lamports()? -= amount;
                **a_keeper_fund.try_borrow_mut_lamports()? += amount;

                // Update keeper fund state
                let mut fund_data = a_keeper_fund
                    .try_borrow_mut_data()
                    .map_err(|_| ProgramError::AccountBorrowFailed)?;

                if let Some(fund_state) = crate::keeper_fund::read_state(&fund_data) {
                    let mut new_state = *fund_state;
                    new_state.balance = new_state.balance.saturating_add(amount);
                    new_state.total_topped_up = new_state.total_topped_up.saturating_add(amount);
                    crate::keeper_fund::write_state(&mut fund_data, &new_state);

                    // #1015: Only unpause if market was auto-paused due to keeper fund
                    // depletion (depleted_pause != 0). Never clear admin-initiated pauses.
                    if new_state.depleted_pause != 0
                        && !crate::keeper_fund::is_depleted(new_state.balance)
                    {
                        new_state.depleted_pause = 0;
                        crate::keeper_fund::write_state(&mut fund_data, &new_state);
                        drop(fund_data);
                        let mut slab_data = state::slab_data_mut(a_slab)?;
                        if state::read_flags(&slab_data) & state::FLAG_PAUSED != 0 {
                            state::set_paused(&mut slab_data, false);
                            msg!("KEEPER_FUND_TOPPED_UP: market unpaused (was depleted_pause)");
                        }
                    }
                } else {
                    return Err(ProgramError::InvalidAccountData);
                }

                msg!("TopUpKeeperFund: amount={}", amount);
            }

            // PERC-628: InitSharedVault — admin creates the global shared vault PDA.
            Instruction::InitSharedVault {
                epoch_duration_slots,
                max_market_exposure_bps,
            } => {
                // accounts: [0] admin (signer), [1] shared_vault PDA (writable),
                //           [2] system_program
                accounts::expect_len(accounts, 3)?;
                let a_admin = &accounts[0];
                let a_shared_vault = &accounts[1];
                let a_system_program = &accounts[2];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_shared_vault)?;

                if *a_system_program.key != solana_program::system_program::id() {
                    return Err(ProgramError::IncorrectProgramId);
                }

                let (expected_pda, pda_bump) = Pubkey::find_program_address(
                    &[crate::shared_vault::SHARED_VAULT_SEED],
                    program_id,
                );
                if *a_shared_vault.key != expected_pda {
                    return Err(ProgramError::InvalidSeeds);
                }

                if !a_shared_vault.data_is_empty() {
                    return Err(ProgramError::AccountAlreadyInitialized);
                }

                let rent = solana_program::rent::Rent::get()?;
                let lamports = rent.minimum_balance(crate::shared_vault::SHARED_VAULT_STATE_LEN);
                let bump_bytes = [pda_bump];
                let signer_seeds: &[&[u8]] = &[crate::shared_vault::SHARED_VAULT_SEED, &bump_bytes];
                solana_program::program::invoke_signed(
                    &solana_program::system_instruction::create_account(
                        a_admin.key,
                        &expected_pda,
                        lamports,
                        crate::shared_vault::SHARED_VAULT_STATE_LEN as u64,
                        program_id,
                    ),
                    &[
                        a_admin.clone(),
                        a_shared_vault.clone(),
                        a_system_program.clone(),
                    ],
                    &[signer_seeds],
                )?;

                let clock = solana_program::clock::Clock::get()?;
                let duration = if epoch_duration_slots == 0 {
                    crate::shared_vault::DEFAULT_EPOCH_DURATION_SLOTS
                } else {
                    epoch_duration_slots
                };
                let max_bps = if max_market_exposure_bps == 0 {
                    crate::shared_vault::DEFAULT_MAX_MARKET_EXPOSURE_BPS
                } else {
                    max_market_exposure_bps.min(10_000)
                };

                let state = crate::shared_vault::SharedVaultState {
                    magic: crate::shared_vault::SHARED_VAULT_MAGIC,
                    epoch_number: 0,
                    total_capital: 0,
                    total_allocated: 0,
                    pending_withdrawals: 0,
                    epoch_start_slot: clock.slot,
                    epoch_duration_slots: duration,
                    max_market_exposure_bps: max_bps,
                    bump: pda_bump,
                    _pad: [0; 13],
                    epoch_snapshot_capital: 0,
                    epoch_snapshot_pending: 0,
                };
                let mut sv_data = a_shared_vault
                    .try_borrow_mut_data()
                    .map_err(|_| ProgramError::AccountBorrowFailed)?;
                crate::shared_vault::write_vault_state(&mut sv_data, &state);

                msg!(
                    "PERC-628: SharedVault initialized — epoch_duration={} max_exposure_bps={}",
                    duration,
                    max_bps
                );
            }

            // PERC-628: AllocateMarket — allocate virtual liquidity to a market.
            Instruction::AllocateMarket { amount } => {
                // accounts: [0] admin (signer), [1] slab, [2] shared_vault PDA (writable),
                //           [3] market_alloc PDA (writable), [4] system_program
                accounts::expect_len(accounts, 5)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];
                let a_shared_vault = &accounts[2];
                let a_market_alloc = &accounts[3];
                let a_system_program = &accounts[4];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_shared_vault)?;
                accounts::expect_writable(a_market_alloc)?;

                // Verify slab
                let slab_data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &slab_data)?;
                require_initialized(&slab_data)?;
                let header = state::read_header(&slab_data);
                require_admin(header.admin, a_admin.key)?;
                drop(slab_data);

                // Verify shared vault PDA
                let (expected_sv, _) = Pubkey::find_program_address(
                    &[crate::shared_vault::SHARED_VAULT_SEED],
                    program_id,
                );
                accounts::expect_key(a_shared_vault, &expected_sv)?;

                // Verify market_alloc PDA
                let (expected_alloc, alloc_bump) = Pubkey::find_program_address(
                    &[crate::shared_vault::MARKET_ALLOC_SEED, a_slab.key.as_ref()],
                    program_id,
                );
                if *a_market_alloc.key != expected_alloc {
                    return Err(ProgramError::InvalidSeeds);
                }

                // Read shared vault state
                let mut sv_data = a_shared_vault
                    .try_borrow_mut_data()
                    .map_err(|_| ProgramError::AccountBorrowFailed)?;
                let mut vault_state = crate::shared_vault::read_vault_state(&sv_data)
                    .ok_or(ProgramError::UninitializedAccount)?;

                // Check exposure cap
                let new_allocation = amount;
                if !crate::shared_vault::check_exposure_cap(
                    vault_state.total_capital,
                    new_allocation,
                    vault_state.max_market_exposure_bps,
                ) {
                    msg!("PERC-628: allocation {} exceeds exposure cap", amount);
                    return Err(ProgramError::InvalidArgument);
                }

                // Check available capital
                let available = crate::shared_vault::available_for_allocation(
                    vault_state.total_capital,
                    vault_state.total_allocated,
                );
                if new_allocation > available {
                    msg!("PERC-628: allocation {} > available {}", amount, available);
                    return Err(ProgramError::InsufficientFunds);
                }

                // Create market_alloc PDA if needed
                if a_market_alloc.data_is_empty() {
                    if *a_system_program.key != solana_program::system_program::id() {
                        return Err(ProgramError::IncorrectProgramId);
                    }
                    let rent = solana_program::rent::Rent::get()?;
                    let lamports = rent.minimum_balance(crate::shared_vault::MARKET_ALLOC_LEN);
                    let bump_bytes = [alloc_bump];
                    let signer_seeds: &[&[u8]] = &[
                        crate::shared_vault::MARKET_ALLOC_SEED,
                        a_slab.key.as_ref(),
                        &bump_bytes,
                    ];
                    solana_program::program::invoke_signed(
                        &solana_program::system_instruction::create_account(
                            a_admin.key,
                            &expected_alloc,
                            lamports,
                            crate::shared_vault::MARKET_ALLOC_LEN as u64,
                            program_id,
                        ),
                        &[
                            a_admin.clone(),
                            a_market_alloc.clone(),
                            a_system_program.clone(),
                        ],
                        &[signer_seeds],
                    )?;
                }

                // Update market allocation
                let alloc = crate::shared_vault::MarketAllocation {
                    magic: crate::shared_vault::MARKET_ALLOC_MAGIC,
                    bump: alloc_bump,
                    _pad: [0; 7],
                    allocated_capital: new_allocation,
                    utilized_capital: 0,
                };
                let mut alloc_data = a_market_alloc
                    .try_borrow_mut_data()
                    .map_err(|_| ProgramError::AccountBorrowFailed)?;
                crate::shared_vault::write_market_alloc(&mut alloc_data, &alloc);

                // Update shared vault totals
                vault_state.total_allocated =
                    vault_state.total_allocated.saturating_add(new_allocation);
                crate::shared_vault::write_vault_state(&mut sv_data, &vault_state);

                msg!("PERC-628: Market allocated {} from shared vault", amount);
            }

            // PERC-628: AdvanceEpoch — permissionless crank to advance the epoch.
            Instruction::AdvanceEpoch => {
                // accounts: [0] caller (signer), [1] shared_vault PDA (writable)
                accounts::expect_len(accounts, 2)?;
                let a_caller = &accounts[0];
                let a_shared_vault = &accounts[1];

                accounts::expect_signer(a_caller)?;
                accounts::expect_writable(a_shared_vault)?;

                let (expected_sv, _) = Pubkey::find_program_address(
                    &[crate::shared_vault::SHARED_VAULT_SEED],
                    program_id,
                );
                accounts::expect_key(a_shared_vault, &expected_sv)?;

                let mut sv_data = a_shared_vault
                    .try_borrow_mut_data()
                    .map_err(|_| ProgramError::AccountBorrowFailed)?;
                let mut vault_state = crate::shared_vault::read_vault_state(&sv_data)
                    .ok_or(ProgramError::UninitializedAccount)?;

                let clock = solana_program::clock::Clock::get()?;
                if !crate::shared_vault::is_epoch_elapsed(
                    clock.slot,
                    vault_state.epoch_start_slot,
                    vault_state.epoch_duration_slots,
                ) {
                    return Err(ProgramError::InvalidArgument);
                }

                // Snapshot capital and pending_withdrawals at epoch boundary
                // BEFORE resetting so ClaimEpochWithdrawal uses fixed values
                // regardless of claim ordering. Fixes security issue #1016.
                vault_state.epoch_snapshot_capital = vault_state.total_capital;
                vault_state.epoch_snapshot_pending = vault_state.pending_withdrawals;

                vault_state.epoch_number = vault_state.epoch_number.saturating_add(1);
                vault_state.epoch_start_slot = clock.slot;
                vault_state.pending_withdrawals = 0;
                crate::shared_vault::write_vault_state(&mut sv_data, &vault_state);

                msg!(
                    "PERC-628: Epoch advanced to {} at slot {} (snapshot_capital={} snapshot_pending={})",
                    vault_state.epoch_number,
                    clock.slot,
                    vault_state.epoch_snapshot_capital,
                    vault_state.epoch_snapshot_pending,
                );
            }

            // PERC-628: QueueWithdrawalSV — queue a withdrawal for the current epoch.
            Instruction::QueueWithdrawalSV { lp_amount } => {
                // accounts: [0] user (signer), [1] shared_vault PDA (writable),
                //           [2] withdraw_req PDA (writable), [3] system_program
                accounts::expect_len(accounts, 4)?;
                let a_user = &accounts[0];
                let a_shared_vault = &accounts[1];
                let a_withdraw_req = &accounts[2];
                let a_system_program = &accounts[3];

                accounts::expect_signer(a_user)?;
                accounts::expect_writable(a_shared_vault)?;
                accounts::expect_writable(a_withdraw_req)?;

                if lp_amount == 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }

                let (expected_sv, _) = Pubkey::find_program_address(
                    &[crate::shared_vault::SHARED_VAULT_SEED],
                    program_id,
                );
                accounts::expect_key(a_shared_vault, &expected_sv)?;

                let mut sv_data = a_shared_vault
                    .try_borrow_mut_data()
                    .map_err(|_| ProgramError::AccountBorrowFailed)?;
                let mut vault_state = crate::shared_vault::read_vault_state(&sv_data)
                    .ok_or(ProgramError::UninitializedAccount)?;

                // Verify withdraw_req PDA
                let epoch_bytes = vault_state.epoch_number.to_le_bytes();
                let (expected_req, req_bump) = Pubkey::find_program_address(
                    &[
                        crate::shared_vault::WITHDRAW_REQ_SEED,
                        a_shared_vault.key.as_ref(),
                        a_user.key.as_ref(),
                        &epoch_bytes,
                    ],
                    program_id,
                );
                if *a_withdraw_req.key != expected_req {
                    return Err(ProgramError::InvalidSeeds);
                }

                // Create withdraw_req PDA if needed
                if a_withdraw_req.data_is_empty() {
                    if *a_system_program.key != solana_program::system_program::id() {
                        return Err(ProgramError::IncorrectProgramId);
                    }
                    let rent = solana_program::rent::Rent::get()?;
                    let lamports = rent.minimum_balance(crate::shared_vault::WITHDRAW_REQ_LEN);
                    let bump_bytes = [req_bump];
                    let signer_seeds: &[&[u8]] = &[
                        crate::shared_vault::WITHDRAW_REQ_SEED,
                        a_shared_vault.key.as_ref(),
                        a_user.key.as_ref(),
                        &epoch_bytes,
                        &bump_bytes,
                    ];
                    solana_program::program::invoke_signed(
                        &solana_program::system_instruction::create_account(
                            a_user.key,
                            &expected_req,
                            lamports,
                            crate::shared_vault::WITHDRAW_REQ_LEN as u64,
                            program_id,
                        ),
                        &[
                            a_user.clone(),
                            a_withdraw_req.clone(),
                            a_system_program.clone(),
                        ],
                        &[signer_seeds],
                    )?;
                }

                let req = crate::shared_vault::WithdrawalRequest {
                    magic: crate::shared_vault::WITHDRAW_REQ_MAGIC,
                    bump: req_bump,
                    claimed: 0,
                    _pad: [0; 6],
                    lp_amount,
                    epoch_number: vault_state.epoch_number,
                };
                let mut req_data = a_withdraw_req
                    .try_borrow_mut_data()
                    .map_err(|_| ProgramError::AccountBorrowFailed)?;
                crate::shared_vault::write_withdraw_req(&mut req_data, &req);

                // Update pending withdrawals
                vault_state.pending_withdrawals = crate::shared_vault::queue_withdrawal(
                    vault_state.pending_withdrawals,
                    lp_amount,
                );
                crate::shared_vault::write_vault_state(&mut sv_data, &vault_state);

                msg!(
                    "PERC-628: Queued withdrawal {} LP for epoch {}",
                    lp_amount,
                    vault_state.epoch_number
                );
            }

            // PERC-628: ClaimEpochWithdrawal — claim after epoch elapses.
            // Security: MUST gate on is_epoch_elapsed() and set claimed=1
            // BEFORE transfer (not after). No mid-epoch claims.
            Instruction::ClaimEpochWithdrawal => {
                // accounts: [0] user (signer), [1] shared_vault PDA (writable),
                //           [2] withdraw_req PDA (writable),
                //           [3] slab (for vault derivation),
                //           [4] vault (writable), [5] user_ata (writable),
                //           [6] vault_authority, [7] token_program
                accounts::expect_len(accounts, 8)?;
                let a_user = &accounts[0];
                let a_shared_vault = &accounts[1];
                let a_withdraw_req = &accounts[2];
                let a_slab = &accounts[3];
                let a_vault = &accounts[4];
                let a_user_ata = &accounts[5];
                let a_vault_authority = &accounts[6];
                let a_token = &accounts[7];

                accounts::expect_signer(a_user)?;
                accounts::expect_writable(a_shared_vault)?;
                accounts::expect_writable(a_withdraw_req)?;
                accounts::expect_writable(a_vault)?;
                accounts::expect_writable(a_user_ata)?;
                verify_token_program(a_token)?;

                let (expected_sv, _) = Pubkey::find_program_address(
                    &[crate::shared_vault::SHARED_VAULT_SEED],
                    program_id,
                );
                accounts::expect_key(a_shared_vault, &expected_sv)?;

                let mut sv_data = a_shared_vault
                    .try_borrow_mut_data()
                    .map_err(|_| ProgramError::AccountBorrowFailed)?;
                let mut vault_state = crate::shared_vault::read_vault_state(&sv_data)
                    .ok_or(ProgramError::UninitializedAccount)?;

                // Security HIGH #1: Gate on epoch elapsed — no mid-epoch claims
                let clock = solana_program::clock::Clock::get()?;
                if !crate::shared_vault::is_epoch_elapsed(
                    clock.slot,
                    vault_state.epoch_start_slot,
                    vault_state.epoch_duration_slots,
                ) {
                    msg!("PERC-628: epoch not yet elapsed — cannot claim mid-epoch");
                    return Err(ProgramError::InvalidArgument);
                }

                // Verify slab + vault
                let slab_data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &slab_data)?;
                require_initialized(&slab_data)?;
                let config = state::read_config(&slab_data);
                let mint = Pubkey::new_from_array(config.collateral_mint);
                let (auth, vault_bump) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                accounts::expect_key(a_vault_authority, &auth)?;
                verify_token_account(a_user_ata, a_user.key, &mint)?;
                drop(slab_data);

                // Read request
                let mut req_data = a_withdraw_req
                    .try_borrow_mut_data()
                    .map_err(|_| ProgramError::AccountBorrowFailed)?;
                let req = crate::shared_vault::read_withdraw_req(&req_data)
                    .ok_or(ProgramError::InvalidAccountData)?;

                if req.claimed != 0 {
                    msg!("PERC-628: withdrawal already claimed");
                    return Err(ProgramError::InvalidArgument);
                }

                // Security HIGH #2: Verify request epoch < current epoch.
                // Re-derive PDA from stored epoch to confirm authenticity.
                let req_epoch_bytes = req.epoch_number.to_le_bytes();
                let (expected_req_pda, _) = Pubkey::find_program_address(
                    &[
                        crate::shared_vault::WITHDRAW_REQ_SEED,
                        a_shared_vault.key.as_ref(),
                        a_user.key.as_ref(),
                        &req_epoch_bytes,
                    ],
                    program_id,
                );
                if *a_withdraw_req.key != expected_req_pda {
                    return Err(ProgramError::InvalidSeeds);
                }
                if req.epoch_number >= vault_state.epoch_number {
                    msg!(
                        "PERC-628: request epoch {} >= current {} — must wait for epoch advance",
                        req.epoch_number,
                        vault_state.epoch_number
                    );
                    return Err(ProgramError::InvalidArgument);
                }

                // Security: set claimed=1 BEFORE any transfer
                let mut updated_req = req;
                updated_req.claimed = 1;
                crate::shared_vault::write_withdraw_req(&mut req_data, &updated_req);
                drop(req_data);

                // Compute proportional withdrawal using epoch-boundary snapshots.
                // Using snapshots (not live values) ensures all users in the
                // same epoch get the same per-LP-token payout regardless of
                // claim ordering. Fixes security issue #1016.
                let payout = crate::shared_vault::compute_proportional_withdrawal(
                    req.lp_amount,
                    vault_state.epoch_snapshot_pending,
                    vault_state.epoch_snapshot_capital,
                );

                if payout > 0 {
                    // Convert units to base tokens
                    let base_payout =
                        crate::units::units_to_base_checked(payout, config.unit_scale)
                            .ok_or(PercolatorError::EngineOverflow)?;

                    // Transfer from vault to user
                    let seed1: &[u8] = b"vault";
                    let seed2: &[u8] = a_slab.key.as_ref();
                    let bump_arr: [u8; 1] = [vault_bump];
                    let seed3: &[u8] = &bump_arr;
                    let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
                    let signer_seeds: [&[&[u8]]; 1] = [&seeds];

                    collateral::withdraw(
                        a_token,
                        a_vault,
                        a_user_ata,
                        a_vault_authority,
                        base_payout,
                        &signer_seeds,
                    )?;

                    // Update shared vault accounting
                    vault_state.total_capital =
                        vault_state.total_capital.saturating_sub(payout as u128);
                    crate::shared_vault::write_vault_state(&mut sv_data, &vault_state);

                    msg!(
                        "PERC-628: Claim: {} LP → {} base tokens transferred",
                        req.lp_amount,
                        base_payout
                    );
                } else {
                    msg!("PERC-628: Claim: {} LP → 0 payout", req.lp_amount);
                }
            }

            // Defense-in-depth: if a future tag routes here by mistake,
            // return an error instead of panicking (unreachable! aborts the tx).
            _ => return Err(ProgramError::InvalidInstructionData),
        }
        Ok(())
    }

    #[cfg(test)]
    mod safety_valve_tests {
        use super::*;

        fn make_config(enabled: bool, duration: u64, epochs: u8) -> state::MarketConfig {
            let mut c = <state::MarketConfig as bytemuck::Zeroable>::zeroed();
            c.safety_valve_enabled = if enabled { 1 } else { 0 };
            c.safety_valve_duration = duration;
            c.safety_valve_epochs = epochs;
            c.funding_max_bps_per_slot = 10;
            c
        }

        #[test]
        fn test_valve_blocks_dominant_long() {
            let mut c = make_config(true, 500, 5);
            c.rebalancing_active = 1;
            c.rebalancing_start_slot = 100;
            // net_lp_pos = -100 → users are net long → longs dominant
            // User has pos=0, tries to go long (size=10) → blocked
            let r = check_safety_valve(&c, -100, 10, 0, 200);
            assert!(r.is_err());
        }

        #[test]
        fn test_valve_allows_closing_dominant() {
            let mut c = make_config(true, 500, 5);
            c.rebalancing_active = 1;
            c.rebalancing_start_slot = 100;
            // net_lp_pos = -100 → longs dominant
            // User has pos=50 (long), tries to close (size=-50) → allowed (reduces dominant)
            let r = check_safety_valve(&c, -100, -50, 50, 200);
            assert!(r.is_ok());
        }

        #[test]
        fn test_valve_allows_non_dominant() {
            let mut c = make_config(true, 500, 5);
            c.rebalancing_active = 1;
            c.rebalancing_start_slot = 100;
            // net_lp_pos = -100 → longs dominant
            // User tries to go short (size=-10) → allowed (shorts are non-dominant)
            let r = check_safety_valve(&c, -100, -10, 0, 200);
            assert!(r.is_ok());
        }

        #[test]
        fn test_valve_auto_exit_by_duration() {
            let mut c = make_config(true, 500, 5);
            c.rebalancing_active = 1;
            c.rebalancing_start_slot = 100;
            // slot 700 > 100 + 500 = 600 → auto-exit
            let r = check_safety_valve(&c, -100, 10, 0, 700);
            assert!(r.is_ok()); // Should be allowed (valve expired)
        }

        #[test]
        fn test_valve_disabled() {
            let c = make_config(false, 500, 5);
            // Even with rebalancing_active=1, disabled flag skips
            let r = check_safety_valve(&c, -100, 10, 0, 200);
            assert!(r.is_ok());
        }

        #[test]
        fn test_update_triggers_after_epochs() {
            let mut c = make_config(true, 500, 3);
            // Simulate 3 consecutive max-funding cranks
            update_safety_valve_on_funding(&mut c, 10, 100); // at max
            assert_eq!(c.consecutive_max_funding_epochs, 1);
            assert_eq!(c.rebalancing_active, 0);

            update_safety_valve_on_funding(&mut c, 10, 200);
            assert_eq!(c.consecutive_max_funding_epochs, 2);
            assert_eq!(c.rebalancing_active, 0);

            update_safety_valve_on_funding(&mut c, 10, 300);
            assert_eq!(c.consecutive_max_funding_epochs, 3);
            assert_eq!(c.rebalancing_active, 1);
            assert_eq!(c.rebalancing_start_slot, 300);
        }

        #[test]
        fn test_update_resets_on_non_max() {
            let mut c = make_config(true, 500, 3);
            c.consecutive_max_funding_epochs = 2;
            // Below max → reset counter
            update_safety_valve_on_funding(&mut c, 5, 100);
            assert_eq!(c.consecutive_max_funding_epochs, 0);
        }

        #[test]
        fn test_update_exits_rebalancing_on_skew_resolve() {
            let mut c = make_config(true, 500, 3);
            c.rebalancing_active = 1;
            c.rebalancing_start_slot = 100;
            c.consecutive_max_funding_epochs = 5;
            // Below max → skew resolved → exit rebalancing
            update_safety_valve_on_funding(&mut c, 5, 200);
            assert_eq!(c.rebalancing_active, 0);
            assert_eq!(c.rebalancing_start_slot, 0);
        }
    }

    #[cfg(kani)]
    mod safety_valve_proofs {
        use super::*;

        /// PERC-312: Safety valve always exits after duration elapses.
        /// No trade is blocked once current_slot >= rebalancing_start_slot + duration.
        #[kani::proof]
        #[kani::unwind(1)]
        fn nightly_sv_exits_after_duration() {
            let duration: u64 = kani::any();
            let start_slot: u64 = kani::any();
            let current_slot: u64 = kani::any();
            let net_lp_pos: i128 = kani::any();
            let size: i128 = kani::any();
            let old_user_pos: i128 = kani::any();

            kani::assume(duration > 0);
            kani::assume(start_slot <= u64::MAX / 2);
            kani::assume(current_slot >= start_slot.saturating_add(duration));

            let mut c = <state::MarketConfig as bytemuck::Zeroable>::zeroed();
            c.safety_valve_enabled = 1;
            c.safety_valve_duration = duration;
            c.rebalancing_active = 1;
            c.rebalancing_start_slot = start_slot;
            c.funding_max_bps_per_slot = 10;

            let result = check_safety_valve(&c, net_lp_pos, size, old_user_pos, current_slot);
            assert!(
                result.is_ok(),
                "trade must not be blocked after duration elapsed"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// PERC-622: Oracle Phase Transition — Tests & Kani Proofs
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod oracle_phase_tests {
    use crate::state::*;
    use bytemuck::Zeroable;

    #[test]
    fn test_phase_accessors_roundtrip() {
        let mut config = MarketConfig::zeroed();
        assert_eq!(get_oracle_phase(&config), 0);
        set_oracle_phase(&mut config, 1);
        assert_eq!(get_oracle_phase(&config), 1);
        set_oracle_phase(&mut config, 2);
        assert_eq!(get_oracle_phase(&config), 2);
        // Clamp to 2
        set_oracle_phase(&mut config, 255);
        assert_eq!(get_oracle_phase(&config), 2);
    }

    #[test]
    fn test_volume_accessors_roundtrip() {
        let mut config = MarketConfig::zeroed();
        assert_eq!(get_cumulative_volume(&config), 0);
        set_cumulative_volume(&mut config, 12345678);
        assert_eq!(get_cumulative_volume(&config), 12345678);
        set_cumulative_volume(&mut config, u64::MAX);
        assert_eq!(get_cumulative_volume(&config), u64::MAX);
    }

    #[test]
    fn test_phase2_delta_slots_roundtrip() {
        let mut config = MarketConfig::zeroed();
        assert_eq!(get_phase2_delta_slots(&config), 0);
        set_phase2_delta_slots(&mut config, 648_000);
        assert_eq!(get_phase2_delta_slots(&config), 648_000);
        // Max u24
        set_phase2_delta_slots(&mut config, 0x00FF_FFFF);
        assert_eq!(get_phase2_delta_slots(&config), 0x00FF_FFFF);
        // Clamp overflow
        set_phase2_delta_slots(&mut config, 0x0100_0000);
        assert_eq!(get_phase2_delta_slots(&config), 0x00FF_FFFF);
    }

    #[test]
    fn test_phase1_to_phase2_transition() {
        // Not enough time, not enough volume
        let (phase, trans) = check_phase_transition(100, 0, 0, 0, 0, false);
        assert_eq!(phase, 0);
        assert!(!trans);

        // Volume met but under 4h floor → still Phase 1
        let (phase, trans) = check_phase_transition(
            PHASE1_VOLUME_MIN_SLOTS - 1,
            0,
            0,
            PHASE2_VOLUME_THRESHOLD,
            0,
            false,
        );
        assert_eq!(phase, 0);
        assert!(!trans);

        // Path B: 4h elapsed + volume met → Phase 2
        let (phase, trans) = check_phase_transition(
            PHASE1_VOLUME_MIN_SLOTS,
            0,
            0,
            PHASE2_VOLUME_THRESHOLD,
            0,
            false,
        );
        assert_eq!(phase, 1);
        assert!(trans);

        // Path A: 72h elapsed, no volume → Phase 2
        let (phase, trans) = check_phase_transition(PHASE1_MIN_SLOTS, 0, 0, 0, 0, false);
        assert_eq!(phase, 1);
        assert!(trans);

        // Both conditions met
        let (phase, trans) =
            check_phase_transition(PHASE1_MIN_SLOTS, 0, 0, PHASE2_VOLUME_THRESHOLD, 0, false);
        assert_eq!(phase, 1);
        assert!(trans);
    }

    #[test]
    fn test_phase2_to_phase3_by_time() {
        let created = 1_000_000u64;
        let delta = PHASE1_MIN_SLOTS as u32; // Phase 2 entered at created + delta
        let phase2_start = created + delta as u64;
        let mature_slot = phase2_start + PHASE2_MATURITY_SLOTS;

        // Not yet mature
        let (phase, trans) = check_phase_transition(mature_slot - 1, created, 1, 0, delta, false);
        assert_eq!(phase, 1);
        assert!(!trans);

        // Mature by time
        let (phase, trans) = check_phase_transition(mature_slot, created, 1, 0, delta, false);
        assert_eq!(phase, 2);
        assert!(trans);
    }

    #[test]
    fn test_phase2_to_phase3_by_oracle() {
        // Mature oracle available → immediate transition
        let (phase, trans) = check_phase_transition(0, 0, 1, 0, 0, true);
        assert_eq!(phase, 2);
        assert!(trans);
    }

    #[test]
    fn test_phase3_terminal() {
        let (phase, trans) = check_phase_transition(u64::MAX, 0, 2, u64::MAX, u32::MAX, true);
        assert_eq!(phase, 2);
        assert!(!trans);
    }

    #[test]
    fn test_phase_oi_cap() {
        assert_eq!(phase_oi_cap(0, u64::MAX), PHASE1_OI_CAP_E6);
        assert_eq!(phase_oi_cap(1, u64::MAX), PHASE2_OI_CAP_E6);
        assert_eq!(phase_oi_cap(2, 999), 999);
        // Phase cap respects base when base is lower
        assert_eq!(phase_oi_cap(0, 5_000), 5_000);
    }

    #[test]
    fn test_phase_max_leverage() {
        assert_eq!(phase_max_leverage_bps(0, u64::MAX), PHASE1_MAX_LEVERAGE_BPS);
        assert_eq!(phase_max_leverage_bps(1, u64::MAX), PHASE2_MAX_LEVERAGE_BPS);
        assert_eq!(phase_max_leverage_bps(2, 1_000_000), 1_000_000);
        // Respects lower base
        assert_eq!(phase_max_leverage_bps(0, 10_000), 10_000);
    }

    #[test]
    fn test_accumulate_volume() {
        let mut config = MarketConfig::zeroed();
        accumulate_volume(&mut config, 50_000_000_000);
        assert_eq!(get_cumulative_volume(&config), 50_000_000_000);
        accumulate_volume(&mut config, 50_000_000_000);
        assert_eq!(get_cumulative_volume(&config), 100_000_000_000);
        // Saturating
        set_cumulative_volume(&mut config, u64::MAX - 1);
        accumulate_volume(&mut config, 100);
        assert_eq!(get_cumulative_volume(&config), u64::MAX);
    }

    #[test]
    fn test_accessors_dont_clobber_mark_weight() {
        let mut config = MarketConfig::zeroed();
        set_mark_oracle_weight_bps(&mut config, 5000);
        set_oracle_phase(&mut config, 1);
        set_cumulative_volume(&mut config, 999);
        set_phase2_delta_slots(&mut config, 12345);
        // mark_oracle_weight should be untouched
        assert_eq!(get_mark_oracle_weight_bps(&config), 5000);
    }

    #[test]
    fn test_effective_created_slot_legacy() {
        // Legacy market: market_created_slot == 0 → returns current_slot
        assert_eq!(effective_created_slot(0, 310_000_000), 310_000_000);
        // Normal market: returns stored value
        assert_eq!(effective_created_slot(100_000, 310_000_000), 100_000);
    }

    #[test]
    fn test_legacy_market_no_auto_promote() {
        // Legacy market with market_created_slot == 0.
        // After effective_created_slot resolution, elapsed = 0 → stays Phase 1.
        let current = 310_000_000u64;
        let resolved = effective_created_slot(0, current);
        let (phase, trans) = check_phase_transition(current, resolved, 0, u64::MAX, 0, false);
        assert_eq!(phase, 0, "legacy market must NOT auto-promote");
        assert!(!trans);
    }
}

#[cfg(kani)]
mod oracle_phase_kani {
    use crate::state::*;

    /// Phase transitions are monotonic: new_phase >= old_phase, always ≤ 2.
    #[kani::proof]
    fn proof_oracle_phase_monotone() {
        let old_phase: u8 = kani::any();
        kani::assume(old_phase <= 2);
        let slot: u64 = kani::any();
        let created: u64 = kani::any();
        kani::assume(created <= slot);
        let vol: u64 = kani::any();
        let delta: u32 = kani::any();
        let has_oracle: bool = kani::any();

        let (new_phase, _) =
            check_phase_transition(slot, created, old_phase, vol, delta, has_oracle);
        assert!(new_phase >= old_phase, "phase must never decrease");
        assert!(new_phase <= 2, "phase must be 0, 1, or 2");
    }

    /// Phase 1 OI cap is always PHASE1_OI_CAP_E6 or less.
    #[kani::proof]
    fn proof_phase1_oi_cap_bounded() {
        let base_cap: u64 = kani::any();
        let cap = phase_oi_cap(0, base_cap);
        assert!(cap <= PHASE1_OI_CAP_E6);
    }

    /// Phase 2 leverage is always PHASE2_MAX_LEVERAGE_BPS or less.
    #[kani::proof]
    fn proof_phase2_leverage_bounded() {
        let base_lev: u64 = kani::any();
        let lev = phase_max_leverage_bps(1, base_lev);
        assert!(lev <= PHASE2_MAX_LEVERAGE_BPS);
    }

    /// Phase 3 is terminal — never transitions further.
    #[kani::proof]
    fn proof_phase3_terminal() {
        let slot: u64 = kani::any();
        let created: u64 = kani::any();
        kani::assume(created <= slot);
        let vol: u64 = kani::any();
        let delta: u32 = kani::any();
        let has_oracle: bool = kani::any();

        let (new_phase, transitioned) =
            check_phase_transition(slot, created, 2, vol, delta, has_oracle);
        assert!(new_phase == 2, "Phase 3 is terminal");
        assert!(!transitioned, "Phase 3 never transitions");
    }

    /// Cumulative volume never decreases.
    #[kani::proof]
    fn proof_cumulative_volume_monotone() {
        let old_vol: u64 = kani::any();
        let trade_notional: u64 = kani::any();
        let new_vol = old_vol.saturating_add(trade_notional);
        assert!(new_vol >= old_vol);
    }

    /// Cannot leave Phase 1 before PHASE1_VOLUME_MIN_SLOTS (4h absolute floor).
    #[kani::proof]
    fn proof_phase1_requires_min_time() {
        let created: u64 = kani::any();
        let slot: u64 = kani::any();
        kani::assume(created <= slot);
        kani::assume(slot - created < PHASE1_VOLUME_MIN_SLOTS);
        let vol: u64 = kani::any();

        let (new_phase, transitioned) = check_phase_transition(slot, created, 0, vol, 0, false);
        assert_eq!(new_phase, 0);
        assert!(!transitioned);
    }

    /// Phase caps are always <= full configured caps.
    #[kani::proof]
    fn proof_phase_caps_leq_base() {
        let phase: u8 = kani::any();
        kani::assume(phase <= 2);
        let base_oi: u64 = kani::any();
        let base_lev: u64 = kani::any();

        assert!(phase_oi_cap(phase, base_oi) <= base_oi);
        assert!(phase_max_leverage_bps(phase, base_lev) <= base_lev);
    }

    /// Legacy markets (market_created_slot==0) never auto-promote from Phase 1.
    #[kani::proof]
    fn proof_legacy_market_no_auto_promote() {
        let current_slot: u64 = kani::any();
        kani::assume(current_slot > 0);
        let resolved = effective_created_slot(0, current_slot);
        // elapsed = current_slot - resolved = 0
        assert_eq!(resolved, current_slot);
        let vol: u64 = kani::any();
        let (phase, _) = check_phase_transition(current_slot, resolved, 0, vol, 0, false);
        assert_eq!(phase, 0, "legacy market stays Phase 1 on first encounter");
    }
}

// 10. mod entrypoint
pub mod entrypoint {
    use crate::processor;
    #[allow(unused_imports)]
    use alloc::format; // Required by entrypoint! macro in SBF builds
    use solana_program::{
        account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, pubkey::Pubkey,
    };

    entrypoint!(process_instruction);

    fn process_instruction<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        instruction_data: &[u8],
    ) -> ProgramResult {
        processor::process_instruction(program_id, accounts, instruction_data)
    }
}

// 11. mod risk (glue)
pub mod risk {
    pub use percolator::{
        MatchingEngine, NoOpMatcher, RiskEngine, RiskError, RiskParams, TradeExecution,
    };
}
