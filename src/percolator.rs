//! Percolator: Single-file Solana program with embedded Risk Engine.

#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

use solana_program::pubkey::Pubkey;
use solana_program::declare_id;

declare_id!("Perco1ator111111111111111111111111111111111");

// 1. mod constants
pub mod constants {
    use core::mem::{size_of, align_of};
    use crate::state::{SlabHeader, MarketConfig};
    use percolator::RiskEngine;

    pub const MAGIC: u64 = 0x504552434f4c4154; // "PERCOLAT"
    pub const VERSION: u32 = 1;

    pub const HEADER_LEN: usize = size_of::<SlabHeader>();
    pub const CONFIG_LEN: usize = size_of::<MarketConfig>();
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

    /// Maximum allowed unit_scale for InitMarket.
    /// unit_scale=0 disables scaling (1:1 base tokens to units, dust=0 always).
    /// unit_scale=1..=1_000_000_000 enables scaling with dust tracking.
    pub const MAX_UNIT_SCALE: u32 = 1_000_000_000;

    // Inventory-mark funding constants (Option 2)
    // Funding is computed on-chain from LP inventory + oracle price
    pub const FUNDING_HORIZON_SLOTS: u64 = 500;            // ~4 min @ ~2 slots/sec
    pub const FUNDING_K_BPS: u64 = 100;                    // 1.00x multiplier
    pub const FUNDING_INV_SCALE_NOTIONAL_E6: u128 = 1_000_000_000_000; // 1M USDC notional in e6
    pub const FUNDING_MAX_PREMIUM_BPS: i64 = 500;          // cap premium at 5.00%
    pub const FUNDING_MAX_BPS_PER_SLOT: i64 = 5;           // cap per-slot funding

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

    // Auto-threshold policy constants
    /// Base floor for risk_reduction_threshold (can be 0 for demo)
    pub const THRESH_FLOOR: u128 = 0;
    /// BPS of risk metric for threshold calculation (50 = 0.50%)
    pub const THRESH_RISK_BPS: u64 = 50;
    /// Minimum slots between threshold updates (prevents churn)
    pub const THRESH_UPDATE_INTERVAL_SLOTS: u64 = 10;
    /// Maximum BPS change in threshold per update (step clamp)
    pub const THRESH_STEP_BPS: u64 = 500; // 5%
    /// EWMA alpha in BPS (higher = faster response, 1000 = 10% new value)
    pub const THRESH_ALPHA_BPS: u64 = 1000;
    /// Minimum threshold value
    pub const THRESH_MIN: u128 = 0;
    /// Maximum threshold value (cap to prevent overflow)
    pub const THRESH_MAX: u128 = 10_000_000_000_000_000_000u128;
    /// Minimum step size to avoid churn on tiny changes
    pub const THRESH_MIN_STEP: u128 = 1;
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
            sum_abs: engine.get_lp_sum_abs(),
            max_abs: engine.get_lp_max_abs(),
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
        debug_assert!(self.sum_abs >= old_lp_abs, "old_lp_abs not in sum_abs - wrong engine snapshot?");

        // Update sum_abs in O(1)
        let new_sum_abs = self.sum_abs
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
    engine.compute_lp_risk_units()
}

/// Compute net LP position for inventory-based funding. O(1).
/// Uses engine's maintained net_lp_pos instead of scanning.
#[inline]
fn compute_net_lp_pos(engine: &percolator::RiskEngine) -> i128 {
    engine.get_net_lp_pos()
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
pub fn compute_inventory_funding_bps_per_slot(net_lp_pos: i128, price_e6: u64) -> i64 {
    use crate::constants::{
        FUNDING_HORIZON_SLOTS, FUNDING_K_BPS, FUNDING_INV_SCALE_NOTIONAL_E6,
        FUNDING_MAX_PREMIUM_BPS, FUNDING_MAX_BPS_PER_SLOT
    };

    if net_lp_pos == 0 || price_e6 == 0 {
        return 0;
    }

    let abs_pos: u128 = net_lp_pos.unsigned_abs();
    let notional_e6: u128 = abs_pos.saturating_mul(price_e6 as u128) / 1_000_000u128;

    // premium_bps = (notional / scale) * k_bps, capped
    let mut premium_bps_u: u128 = notional_e6
        .saturating_mul(FUNDING_K_BPS as u128)
        / FUNDING_INV_SCALE_NOTIONAL_E6;

    if premium_bps_u > (FUNDING_MAX_PREMIUM_BPS.unsigned_abs() as u128) {
        premium_bps_u = FUNDING_MAX_PREMIUM_BPS.unsigned_abs() as u128;
    }

    // Apply sign: if LP net long (net_lp_pos > 0), funding is positive
    let signed_premium_bps: i64 = if net_lp_pos > 0 {
        premium_bps_u as i64
    } else {
        -(premium_bps_u as i64)
    };

    // Convert to per-slot by dividing by horizon
    let mut per_slot: i64 = signed_premium_bps / (FUNDING_HORIZON_SLOTS as i64);

    // Sanity clamp: absolute max ±10000 bps/slot (100% per slot) to catch overflow bugs
    per_slot = per_slot.clamp(-10_000, 10_000);

    // Policy clamp: tighter bound per constants
    if per_slot > FUNDING_MAX_BPS_PER_SLOT { per_slot = FUNDING_MAX_BPS_PER_SLOT; }
    if per_slot < -FUNDING_MAX_BPS_PER_SLOT { per_slot = -FUNDING_MAX_BPS_PER_SLOT; }
    per_slot
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
        ).is_ok()
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
        if permissionless {
            SimpleDecision::Accept
        } else if idx_exists && owner_ok(stored_owner, signer) {
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
        if allow_panic != 0 {
            if !admin_ok(admin, signer) {
                return SimpleDecision::Reject;
            }
        }
        // Normal crank logic
        decide_crank(permissionless, idx_exists, stored_owner, signer)
    }

    // =========================================================================
    // Oracle inversion math (pure logic)
    // =========================================================================

    /// Inversion constant: 1e12 for price_e6 * inverted_e6 = 1e12
    pub const INVERSION_CONSTANT: u128 = 1_000_000_000_000;

    /// Invert oracle price: SOL_per_USD_e6 = 1e12 / USD_per_SOL_e6
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
    pub fn withdraw_amount_aligned(amount: u64, scale: u32) -> bool {
        if scale == 0 {
            return true;
        }
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
}

// 2. mod zc (Zero-Copy unsafe island)
#[allow(unsafe_code)]
pub mod zc {
    use solana_program::program_error::ProgramError;
    use percolator::RiskEngine;
    use crate::constants::{ENGINE_OFF, ENGINE_LEN, ENGINE_ALIGN};
    use core::mem::offset_of;

    // Use const to export the actual offset for debugging
    pub const ACCOUNTS_OFFSET: usize = offset_of!(RiskEngine, accounts);

    #[inline]
    pub fn engine_ref<'a>(data: &'a [u8]) -> Result<&'a RiskEngine, ProgramError> {
        if data.len() < ENGINE_OFF + ENGINE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let ptr = unsafe { data.as_ptr().add(ENGINE_OFF) };
        if (ptr as usize) % ENGINE_ALIGN != 0 {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(unsafe { &*(ptr as *const RiskEngine) })
    }

    #[inline]
    pub fn engine_mut<'a>(data: &'a mut [u8]) -> Result<&'a mut RiskEngine, ProgramError> {
        if data.len() < ENGINE_OFF + ENGINE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let ptr = unsafe { data.as_mut_ptr().add(ENGINE_OFF) };
        if (ptr as usize) % ENGINE_ALIGN != 0 {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(unsafe { &mut *(ptr as *mut RiskEngine) })
    }

    // NOTE: engine_write was removed because it requires passing RiskEngine by value,
    // which stack-allocates the ~6MB struct and causes stack overflow in BPF.
    // Use engine_mut() + init_in_place() instead for initialization.

    use solana_program::{
        account_info::AccountInfo,
        instruction::Instruction as SolInstruction,
        program::invoke_signed,
    };

    /// Invoke the matcher program via CPI with proper lifetime coercion.
    ///
    /// This is the ONLY place where unsafe lifetime transmute is allowed.
    /// The transmute is sound because:
    /// - We are shortening lifetime from 'a (caller) to local scope
    /// - The AccountInfo is only used for the duration of invoke_signed
    /// - We don't hold references past the function call
    #[inline]
    #[allow(unsafe_code)]
    pub fn invoke_signed_trade<'a>(
        ix: &SolInstruction,
        a_lp_pda: &AccountInfo<'a>,
        a_matcher_ctx: &AccountInfo<'a>,
        seeds: &[&[u8]],
    ) -> Result<(), ProgramError> {
        // SAFETY: AccountInfos have lifetime 'a from the caller.
        // We clone them to get owned values (still with 'a lifetime internally).
        // The invoke_signed call consumes them by reference and returns.
        // No lifetime extension occurs.
        let infos = [a_lp_pda.clone(), a_matcher_ctx.clone()];
        invoke_signed(ix, &infos, &[seeds])
    }
}

pub mod matcher_abi {
    use solana_program::program_error::ProgramError;
    use crate::constants::MATCHER_ABI_VERSION;

    /// Matcher return flags
    pub const FLAG_VALID: u32 = 1;       // bit0: response is valid
    pub const FLAG_PARTIAL_OK: u32 = 2;  // bit1: partial fill including zero allowed
    pub const FLAG_REJECTED: u32 = 4;    // bit2: trade rejected by matcher

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
        if ctx.len() < 64 { return Err(ProgramError::InvalidAccountData); }
        let abi_version = u32::from_le_bytes(ctx[0..4].try_into().unwrap());
        let flags = u32::from_le_bytes(ctx[4..8].try_into().unwrap());
        let exec_price_e6 = u64::from_le_bytes(ctx[8..16].try_into().unwrap());
        let exec_size = i128::from_le_bytes(ctx[16..32].try_into().unwrap());
        let req_id = u64::from_le_bytes(ctx[32..40].try_into().unwrap());
        let lp_account_id = u64::from_le_bytes(ctx[40..48].try_into().unwrap());
        let oracle_price_e6 = u64::from_le_bytes(ctx[48..56].try_into().unwrap());
        let reserved = u64::from_le_bytes(ctx[56..64].try_into().unwrap());

        Ok(MatcherReturn {
            abi_version, flags, exec_price_e6, exec_size, req_id, lp_account_id, oracle_price_e6, reserved
        })
    }

    pub fn validate_matcher_return(ret: &MatcherReturn, lp_account_id: u64, oracle_price_e6: u64, req_size: i128, req_id: u64) -> Result<(), ProgramError> {
        // Check ABI version
        if ret.abi_version != MATCHER_ABI_VERSION { return Err(ProgramError::InvalidAccountData); }
        // Must have VALID flag set
        if (ret.flags & FLAG_VALID) == 0 { return Err(ProgramError::InvalidAccountData); }
        // Must not have REJECTED flag set
        if (ret.flags & FLAG_REJECTED) != 0 { return Err(ProgramError::InvalidAccountData); }

        // Validate echoed fields match request
        if ret.lp_account_id != lp_account_id { return Err(ProgramError::InvalidAccountData); }
        if ret.oracle_price_e6 != oracle_price_e6 { return Err(ProgramError::InvalidAccountData); }
        if ret.reserved != 0 { return Err(ProgramError::InvalidAccountData); }
        if ret.req_id != req_id { return Err(ProgramError::InvalidAccountData); }

        // Require exec_price_e6 != 0 always - avoids "all zeros but valid flag" ambiguity
        if ret.exec_price_e6 == 0 { return Err(ProgramError::InvalidAccountData); }

        // Zero exec_size requires PARTIAL_OK flag
        if ret.exec_size == 0 {
            if (ret.flags & FLAG_PARTIAL_OK) == 0 {
                return Err(ProgramError::InvalidAccountData);
            }
            // Zero fill with PARTIAL_OK is allowed - return early
            return Ok(());
        }

        // Size constraints (use unsigned_abs to avoid i128::MIN overflow)
        if ret.exec_size.unsigned_abs() > req_size.unsigned_abs() { return Err(ProgramError::InvalidAccountData); }
        if req_size != 0 {
            if ret.exec_size.signum() != req_size.signum() { return Err(ProgramError::InvalidAccountData); }
        }
        Ok(())
    }
}

// 3. mod error
pub mod error {
    use solana_program::program_error::ProgramError;
    use percolator::RiskError;

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
            RiskError::RiskReductionOnlyMode => PercolatorError::EngineRiskReductionOnlyMode,
            RiskError::AccountKindMismatch => PercolatorError::EngineAccountKindMismatch,
        };
        ProgramError::Custom(err as u32)
    }
}

// 4. mod ix
pub mod ix {
    use solana_program::{pubkey::Pubkey, program_error::ProgramError};
    use percolator::RiskParams;

    #[derive(Debug)]
    pub enum Instruction {
        InitMarket {
            admin: Pubkey,
            collateral_mint: Pubkey,
            /// Pyth feed ID for the index price (32 bytes)
            index_feed_id: [u8; 32],
            /// Maximum staleness in seconds
            max_staleness_secs: u64,
            conf_filter_bps: u16,
            /// If non-zero, invert oracle price (USD/SOL -> SOL/USD)
            invert: u8,
            /// Lamports per Unit for boundary conversion (0 = no scaling)
            unit_scale: u32,
            risk_params: RiskParams,
        },
        InitUser { fee_payment: u64 },
        InitLP { matcher_program: Pubkey, matcher_context: Pubkey, fee_payment: u64 },
        DepositCollateral { user_idx: u16, amount: u64 },
        WithdrawCollateral { user_idx: u16, amount: u64 },
        KeeperCrank { caller_idx: u16, allow_panic: u8 },
        TradeNoCpi { lp_idx: u16, user_idx: u16, size: i128 },
        LiquidateAtOracle { target_idx: u16 },
        CloseAccount { user_idx: u16 },
        TopUpInsurance { amount: u64 },
        TradeCpi { lp_idx: u16, user_idx: u16, size: i128 },
        SetRiskThreshold { new_threshold: u128 },
        UpdateAdmin { new_admin: Pubkey },
        /// Close the market slab and recover SOL to admin.
        /// Requires: no active accounts, no vault funds, no insurance funds.
        CloseSlab,
    }

    impl Instruction {
        pub fn decode(input: &[u8]) -> Result<Self, ProgramError> {
            let (&tag, mut rest) = input.split_first().ok_or(ProgramError::InvalidInstructionData)?;
            
            match tag {
                0 => { // InitMarket
                    let admin = read_pubkey(&mut rest)?;
                    let collateral_mint = read_pubkey(&mut rest)?;
                    let index_feed_id = read_bytes32(&mut rest)?;
                    let max_staleness_secs = read_u64(&mut rest)?;
                    let conf_filter_bps = read_u16(&mut rest)?;
                    let invert = read_u8(&mut rest)?;
                    let unit_scale = read_u32(&mut rest)?;
                    let risk_params = read_risk_params(&mut rest)?;
                    Ok(Instruction::InitMarket {
                        admin, collateral_mint, index_feed_id,
                        max_staleness_secs, conf_filter_bps, invert, unit_scale, risk_params
                    })
                },
                1 => { // InitUser
                    let fee_payment = read_u64(&mut rest)?;
                    Ok(Instruction::InitUser { fee_payment })
                },
                2 => { // InitLP
                    let matcher_program = read_pubkey(&mut rest)?;
                    let matcher_context = read_pubkey(&mut rest)?;
                    let fee_payment = read_u64(&mut rest)?;
                    Ok(Instruction::InitLP { matcher_program, matcher_context, fee_payment })
                },
                3 => { // Deposit
                    let user_idx = read_u16(&mut rest)?;
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::DepositCollateral { user_idx, amount })
                },
                4 => { // Withdraw
                    let user_idx = read_u16(&mut rest)?;
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::WithdrawCollateral { user_idx, amount })
                },
                5 => { // KeeperCrank
                    let caller_idx = read_u16(&mut rest)?;
                    let allow_panic = read_u8(&mut rest)?;
                    Ok(Instruction::KeeperCrank { caller_idx, allow_panic })
                },
                6 => { // TradeNoCpi
                    let lp_idx = read_u16(&mut rest)?;
                    let user_idx = read_u16(&mut rest)?;
                    let size = read_i128(&mut rest)?;
                    Ok(Instruction::TradeNoCpi { lp_idx, user_idx, size })
                },
                7 => { // LiquidateAtOracle
                    let target_idx = read_u16(&mut rest)?;
                    Ok(Instruction::LiquidateAtOracle { target_idx })
                },
                8 => { // CloseAccount
                    let user_idx = read_u16(&mut rest)?;
                    Ok(Instruction::CloseAccount { user_idx })
                },
                9 => { // TopUpInsurance
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::TopUpInsurance { amount })
                },
                10 => { // TradeCpi
                    let lp_idx = read_u16(&mut rest)?;
                    let user_idx = read_u16(&mut rest)?;
                    let size = read_i128(&mut rest)?;
                    Ok(Instruction::TradeCpi { lp_idx, user_idx, size })
                },
                11 => { // SetRiskThreshold
                    let new_threshold = read_u128(&mut rest)?;
                    Ok(Instruction::SetRiskThreshold { new_threshold })
                },
                12 => { // UpdateAdmin
                    let new_admin = read_pubkey(&mut rest)?;
                    Ok(Instruction::UpdateAdmin { new_admin })
                },
                13 => { // CloseSlab
                    Ok(Instruction::CloseSlab)
                },
                _ => Err(ProgramError::InvalidInstructionData),
            }
        }
    }

    fn read_u8(input: &mut &[u8]) -> Result<u8, ProgramError> {
        let (&val, rest) = input.split_first().ok_or(ProgramError::InvalidInstructionData)?;
        *input = rest;
        Ok(val)
    }

    fn read_u16(input: &mut &[u8]) -> Result<u16, ProgramError> {
        if input.len() < 2 { return Err(ProgramError::InvalidInstructionData); }
        let (bytes, rest) = input.split_at(2);
        *input = rest;
        Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_u32(input: &mut &[u8]) -> Result<u32, ProgramError> {
        if input.len() < 4 { return Err(ProgramError::InvalidInstructionData); }
        let (bytes, rest) = input.split_at(4);
        *input = rest;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_u64(input: &mut &[u8]) -> Result<u64, ProgramError> {
        if input.len() < 8 { return Err(ProgramError::InvalidInstructionData); }
        let (bytes, rest) = input.split_at(8);
        *input = rest;
        Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_i128(input: &mut &[u8]) -> Result<i128, ProgramError> {
        if input.len() < 16 { return Err(ProgramError::InvalidInstructionData); }
        let (bytes, rest) = input.split_at(16);
        *input = rest;
        Ok(i128::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_u128(input: &mut &[u8]) -> Result<u128, ProgramError> {
        if input.len() < 16 { return Err(ProgramError::InvalidInstructionData); }
        let (bytes, rest) = input.split_at(16);
        *input = rest;
        Ok(u128::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_pubkey(input: &mut &[u8]) -> Result<Pubkey, ProgramError> {
        if input.len() < 32 { return Err(ProgramError::InvalidInstructionData); }
        let (bytes, rest) = input.split_at(32);
        *input = rest;
        Ok(Pubkey::new_from_array(bytes.try_into().unwrap()))
    }

    fn read_bytes32(input: &mut &[u8]) -> Result<[u8; 32], ProgramError> {
        if input.len() < 32 { return Err(ProgramError::InvalidInstructionData); }
        let (bytes, rest) = input.split_at(32);
        *input = rest;
        Ok(bytes.try_into().unwrap())
    }

    fn read_risk_params(input: &mut &[u8]) -> Result<RiskParams, ProgramError> {
        Ok(RiskParams {
            warmup_period_slots: read_u64(input)?,
            maintenance_margin_bps: read_u64(input)?,
            initial_margin_bps: read_u64(input)?,
            trading_fee_bps: read_u64(input)?,
            max_accounts: read_u64(input)?,
            new_account_fee: read_u128(input)?,
            risk_reduction_threshold: read_u128(input)?,
            maintenance_fee_per_slot: read_u128(input)?,
            max_crank_staleness_slots: read_u64(input)?,
            liquidation_fee_bps: read_u64(input)?,
            liquidation_fee_cap: read_u128(input)?,
            liquidation_buffer_bps: read_u64(input)?,
            min_liquidation_abs: read_u128(input)?,
        })
    }
}

// 5. mod accounts (Pinocchio validation)
pub mod accounts {
    use solana_program::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};
    use crate::error::PercolatorError;

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
}

// 6. mod state
pub mod state {
    use bytemuck::{Pod, Zeroable};
    use core::cell::RefMut;
    use core::mem::offset_of;
    use solana_program::account_info::AccountInfo;
    use solana_program::program_error::ProgramError;
    use crate::constants::{HEADER_LEN, CONFIG_LEN};

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct SlabHeader {
        pub magic: u64,
        pub version: u32,
        pub bump: u8,
        pub _padding: [u8; 3],
        pub admin: [u8; 32],
        pub _reserved: [u8; 24], // [0..8]=nonce, [8..16]=last_thr_slot, [16..24]=dust_base
    }

    /// Offset of _reserved field in SlabHeader, derived from offset_of! for correctness.
    pub const RESERVED_OFF: usize = offset_of!(SlabHeader, _reserved);

    // Portable compile-time assertion that RESERVED_OFF is 48 (expected layout)
    const _: [(); 48] = [(); RESERVED_OFF];

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct MarketConfig {
        pub collateral_mint: [u8; 32],
        pub vault_pubkey: [u8; 32],
        /// Reserved for future use (was collateral_oracle, unused)
        pub _reserved: [u8; 32],
        /// Pyth feed ID for the index price feed
        pub index_feed_id: [u8; 32],
        /// Maximum staleness in seconds (Pyth Pull uses unix timestamps)
        pub max_staleness_secs: u64,
        pub conf_filter_bps: u16,
        pub vault_authority_bump: u8,
        /// If non-zero, invert the oracle price (USD_per_SOL -> SOL_per_USD)
        pub invert: u8,
        /// Lamports per Unit for conversion (e.g., 1000 means 1 SOL = 1,000,000 Units)
        /// If 0, no scaling is applied (1:1 lamports to units)
        pub unit_scale: u32,
    }

    pub fn slab_data_mut<'a, 'b>(ai: &'b AccountInfo<'a>) -> Result<RefMut<'b, &'a mut [u8]>, ProgramError> {
        Ok(ai.try_borrow_mut_data()?)
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

    /// Read the request nonce from the reserved field in slab header.
    /// The nonce is stored at RESERVED_OFF..RESERVED_OFF+8 as little-endian u64.
    pub fn read_req_nonce(data: &[u8]) -> u64 {
        u64::from_le_bytes(data[RESERVED_OFF..RESERVED_OFF + 8].try_into().unwrap())
    }

    /// Write the request nonce to the reserved field in slab header.
    /// The nonce is stored in _reserved[0..8] as little-endian u64.
    /// Uses offset_of! for correctness even if SlabHeader layout changes.
    pub fn write_req_nonce(data: &mut [u8], nonce: u64) {
        #[cfg(debug_assertions)]
        debug_assert!(HEADER_LEN >= RESERVED_OFF + 16);
        data[RESERVED_OFF..RESERVED_OFF + 8].copy_from_slice(&nonce.to_le_bytes());
    }

    /// Read the last threshold update slot from _reserved[8..16].
    pub fn read_last_thr_update_slot(data: &[u8]) -> u64 {
        u64::from_le_bytes(data[RESERVED_OFF + 8..RESERVED_OFF + 16].try_into().unwrap())
    }

    /// Write the last threshold update slot to _reserved[8..16].
    pub fn write_last_thr_update_slot(data: &mut [u8], slot: u64) {
        data[RESERVED_OFF + 8..RESERVED_OFF + 16].copy_from_slice(&slot.to_le_bytes());
    }

    /// Read accumulated dust (base token remainder) from _reserved[16..24].
    pub fn read_dust_base(data: &[u8]) -> u64 {
        u64::from_le_bytes(data[RESERVED_OFF + 16..RESERVED_OFF + 24].try_into().unwrap())
    }

    /// Write accumulated dust (base token remainder) to _reserved[16..24].
    pub fn write_dust_base(data: &mut [u8], dust: u64) {
        data[RESERVED_OFF + 16..RESERVED_OFF + 24].copy_from_slice(&dust.to_le_bytes());
    }

    pub fn read_config(data: &[u8]) -> MarketConfig {
        let mut c = MarketConfig::zeroed();
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
    use solana_program::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};
    use crate::error::PercolatorError;

    // SECURITY (H5): The "devnet" feature disables critical oracle safety checks:
    // - Staleness validation (stale prices accepted)
    // - Confidence interval validation (wide confidence accepted)
    //
    // WARNING: NEVER deploy to mainnet with the "devnet" feature enabled!
    // Build for mainnet with: cargo build-sbf (without --features devnet)

    /// Pyth Solana Receiver program ID (same for mainnet and devnet)
    /// rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ
    pub const PYTH_RECEIVER_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
        0x0c, 0xb7, 0xfa, 0xbb, 0x52, 0xf7, 0xa6, 0x48,
        0xbb, 0x5b, 0x31, 0x7d, 0x9a, 0x01, 0x8b, 0x90,
        0x57, 0xcb, 0x02, 0x47, 0x74, 0xfa, 0xfe, 0x01,
        0xe6, 0xc4, 0xdf, 0x98, 0xcc, 0x38, 0x58, 0x81,
    ]);

    // PriceUpdateV2 account layout offsets (134 bytes minimum)
    // See: https://github.com/pyth-network/pyth-crosschain/blob/main/target_chains/solana/pyth_solana_receiver_sdk/src/price_update.rs
    const PRICE_UPDATE_V2_MIN_LEN: usize = 134;
    const OFF_FEED_ID: usize = 42;      // 32 bytes
    const OFF_PRICE: usize = 74;        // i64
    const OFF_CONF: usize = 82;         // u64
    const OFF_EXPO: usize = 90;         // i32
    const OFF_PUBLISH_TIME: usize = 94; // i64

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
    /// Returns the price in e6 format (e.g., 150_000_000 = $150.00).
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

        // Validate feed_id matches expected
        let feed_id: [u8; 32] = data[OFF_FEED_ID..OFF_FEED_ID + 32].try_into().unwrap();
        if &feed_id != expected_feed_id {
            return Err(PercolatorError::InvalidOracleKey.into());
        }

        // Read price fields
        let price = i64::from_le_bytes(data[OFF_PRICE..OFF_PRICE + 8].try_into().unwrap());
        let conf = u64::from_le_bytes(data[OFF_CONF..OFF_CONF + 8].try_into().unwrap());
        let expo = i32::from_le_bytes(data[OFF_EXPO..OFF_EXPO + 4].try_into().unwrap());
        let publish_time = i64::from_le_bytes(data[OFF_PUBLISH_TIME..OFF_PUBLISH_TIME + 8].try_into().unwrap());

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
            price_u.checked_mul(mul).ok_or(PercolatorError::EngineOverflow)?
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

    /// Read oracle price for engine use, applying inversion if configured.
    ///
    /// If invert == 0: returns USD_per_SOL_e6 (normal)
    /// If invert != 0: returns SOL_per_USD_e6 = 1e12 / USD_per_SOL_e6 (inverted)
    ///
    /// The raw oracle is validated (staleness, confidence) BEFORE inversion.
    pub fn read_engine_price_e6(
        price_ai: &AccountInfo,
        expected_feed_id: &[u8; 32],
        now_unix_ts: i64,
        max_staleness_secs: u64,
        conf_bps: u16,
        invert: u8,
    ) -> Result<u64, ProgramError> {
        let raw_price = read_pyth_price_e6(price_ai, expected_feed_id, now_unix_ts, max_staleness_secs, conf_bps)?;

        if invert == 0 {
            return Ok(raw_price);
        }

        // Invert: SOL_per_USD_e6 = 1e12 / USD_per_SOL_e6
        let inverted = (1_000_000_000_000u128)
            .checked_div(raw_price as u128)
            .ok_or(PercolatorError::OracleInvalid)?;

        if inverted == 0 {
            return Err(PercolatorError::OracleInvalid.into());
        }
        if inverted > u64::MAX as u128 {
            return Err(PercolatorError::EngineOverflow.into());
        }

        Ok(inverted as u64)
    }
}

// 9. mod collateral
pub mod collateral {
    use solana_program::{
        account_info::AccountInfo, program_error::ProgramError,
    };

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
        amount: u64
    ) -> Result<(), ProgramError> {
        if amount == 0 { return Ok(()); }
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
            invoke(&ix, &[source.clone(), dest.clone(), _authority.clone(), _token_program.clone()])
        }
        #[cfg(feature = "test")]
        {
            let mut src_data = source.try_borrow_mut_data()?;
            let mut src_state = TokenAccount::unpack(&src_data)?;
            src_state.amount = src_state.amount.checked_sub(amount).ok_or(ProgramError::InsufficientFunds)?;
            TokenAccount::pack(src_state, &mut src_data)?;

            let mut dst_data = dest.try_borrow_mut_data()?;
            let mut dst_state = TokenAccount::unpack(&dst_data)?;
            dst_state.amount = dst_state.amount.checked_add(amount).ok_or(ProgramError::InvalidAccountData)?;
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
        if amount == 0 { return Ok(()); }
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
            invoke_signed(&ix, &[source.clone(), dest.clone(), _authority.clone(), _token_program.clone()], _signer_seeds)
        }
        #[cfg(feature = "test")]
        {
            let mut src_data = source.try_borrow_mut_data()?;
            let mut src_state = TokenAccount::unpack(&src_data)?;
            src_state.amount = src_state.amount.checked_sub(amount).ok_or(ProgramError::InsufficientFunds)?;
            TokenAccount::pack(src_state, &mut src_data)?;

            let mut dst_data = dest.try_borrow_mut_data()?;
            let mut dst_state = TokenAccount::unpack(&dst_data)?;
            dst_state.amount = dst_state.amount.checked_add(amount).ok_or(ProgramError::InvalidAccountData)?;
            TokenAccount::pack(dst_state, &mut dst_data)?;
            Ok(())
        }
    }
}

// 9. mod processor
pub mod processor {
    use solana_program::{
        account_info::AccountInfo, entrypoint::ProgramResult, pubkey::Pubkey,
        sysvar::{clock::Clock, Sysvar},
        program_error::ProgramError,
        program_pack::Pack,
        msg,
        log::{sol_log_compute_units, sol_log_64},
    };
    use crate::{
        ix::Instruction,
        state::{self, SlabHeader, MarketConfig},
        accounts,
        constants::{MAGIC, VERSION, SLAB_LEN, CONFIG_LEN, MATCHER_CONTEXT_LEN, MATCHER_CALL_TAG, MATCHER_CALL_LEN, MATCHER_CONTEXT_PREFIX_LEN,
            THRESH_FLOOR, THRESH_RISK_BPS, THRESH_UPDATE_INTERVAL_SLOTS, THRESH_STEP_BPS, THRESH_ALPHA_BPS, THRESH_MIN, THRESH_MAX, THRESH_MIN_STEP},
        error::{PercolatorError, map_risk_error},
        oracle,
        collateral,
        zc,
    };
    use percolator::{RiskEngine, NoOpMatcher, MAX_ACCOUNTS, MatchingEngine, TradeExecution, RiskError};
    use solana_program::instruction::{Instruction as SolInstruction, AccountMeta};

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

    fn slab_guard(program_id: &Pubkey, slab: &AccountInfo, data: &[u8]) -> Result<(), ProgramError> {
        // Slab shape validation via verify helper (Kani-provable)
        let shape = crate::verify::SlabShape {
            owned_by_program: slab.owner == program_id,
            correct_len: data.len() == SLAB_LEN,
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
        if h.magic != MAGIC { return Err(PercolatorError::NotInitialized.into()); }
        if h.version != VERSION { return Err(PercolatorError::InvalidVersion.into()); }
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

    fn check_idx(engine: &RiskEngine, idx: u16) -> Result<(), ProgramError> {
        if (idx as usize) >= MAX_ACCOUNTS || !engine.is_used(idx as usize) {
            return Err(PercolatorError::EngineAccountNotFound.into());
        }
        Ok(())
    }

    fn verify_vault(a_vault: &AccountInfo, expected_owner: &Pubkey, expected_mint: &Pubkey, expected_pubkey: &Pubkey) -> Result<(), ProgramError> {
        if a_vault.key != expected_pubkey { return Err(PercolatorError::InvalidVaultAta.into()); }
        if a_vault.owner != &spl_token::ID { return Err(PercolatorError::InvalidVaultAta.into()); }
        if a_vault.data_len() != spl_token::state::Account::LEN { return Err(PercolatorError::InvalidVaultAta.into()); }

        let data = a_vault.try_borrow_data()?;
        let tok = spl_token::state::Account::unpack(&data)?;
        if tok.mint != *expected_mint { return Err(PercolatorError::InvalidMint.into()); }
        if tok.owner != *expected_owner { return Err(PercolatorError::InvalidVaultAta.into()); }
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
    fn verify_token_account(a_token_account: &AccountInfo, expected_owner: &Pubkey, expected_mint: &Pubkey) -> Result<(), ProgramError> {
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

    pub fn process_instruction<'a, 'b>(
        program_id: &Pubkey,
        accounts: &'b [AccountInfo<'a>],
        instruction_data: &[u8],
    ) -> ProgramResult {
        let instruction = Instruction::decode(instruction_data)?;

        match instruction {
            Instruction::InitMarket {
                admin, collateral_mint, index_feed_id,
                max_staleness_secs, conf_filter_bps, invert, unit_scale, risk_params
            } => {
                // Reduced from 11 to 9: removed pyth_index and pyth_collateral accounts
                // (feed_id is now passed in instruction data, not as account)
                accounts::expect_len(accounts, 9)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];
                let a_mint = &accounts[2];
                let a_vault = &accounts[3];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;

                // Ensure instruction data matches the signer
                if admin != *a_admin.key {
                    return Err(ProgramError::InvalidInstructionData);
                }

                // SECURITY (H1): Enforce collateral_mint matches the account
                // This prevents signers from being confused by mismatched instruction data
                if collateral_mint != *a_mint.key {
                    return Err(ProgramError::InvalidInstructionData);
                }

                // SECURITY (H2): Validate mint is a real SPL Token mint
                // Check owner == spl_token::ID and data length == Mint::LEN (82 bytes)
                #[cfg(not(feature = "test"))]
                {
                    use spl_token::state::Mint;
                    use solana_program::program_pack::Pack;
                    if *a_mint.owner != spl_token::ID {
                        return Err(ProgramError::IllegalOwner);
                    }
                    if a_mint.data_len() != Mint::LEN {
                        return Err(ProgramError::InvalidAccountData);
                    }
                    // Verify mint is initialized by unpacking
                    let mint_data = a_mint.try_borrow_data()?;
                    let _ = Mint::unpack(&mint_data)?;
                }

                // Validate unit_scale: reject huge values that make most deposits credit 0 units
                if !crate::verify::init_market_scale_ok(unit_scale) {
                    return Err(ProgramError::InvalidInstructionData);
                }

                #[cfg(debug_assertions)]
                {
                    if core::mem::size_of::<MarketConfig>() != CONFIG_LEN {
                        return Err(ProgramError::InvalidAccountData);
                    }
                }

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;

                let _ = zc::engine_mut(&mut data)?;

                let header = state::read_header(&data);
                if header.magic == MAGIC { return Err(PercolatorError::AlreadyInitialized.into()); }

                let (auth, bump) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(a_vault, &auth, a_mint.key, a_vault.key)?;

                for b in data.iter_mut() { *b = 0; }

                // Initialize engine in-place (zero-copy) to avoid stack overflow.
                // The data is already zeroed above, so init_in_place only sets non-zero fields.
                let engine = zc::engine_mut(&mut data)?;
                engine.init_in_place(risk_params);

                let config = MarketConfig {
                    collateral_mint: a_mint.key.to_bytes(),
                    vault_pubkey: a_vault.key.to_bytes(),
                    _reserved: [0; 32],
                    index_feed_id,
                    max_staleness_secs,
                    conf_filter_bps,
                    vault_authority_bump: bump,
                    invert,
                    unit_scale,
                };
                state::write_config(&mut data, &config);

                let new_header = SlabHeader {
                    magic: MAGIC,
                    version: VERSION,
                    bump,
                    _padding: [0; 3],
                    admin: a_admin.key.to_bytes(),
                    _reserved: [0; 24],
                };
                state::write_header(&mut data, &new_header);
                // Step 4: Explicitly initialize nonce to 0 for determinism
                state::write_req_nonce(&mut data, 0);
                // Initialize threshold update slot to 0
                state::write_last_thr_update_slot(&mut data, 0);
            },
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
                let config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let (auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(a_vault, &auth, &mint, &Pubkey::new_from_array(config.vault_pubkey))?;
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
                engine.set_owner(idx, a_user.key.to_bytes()).map_err(map_risk_error)?;
            },
            Instruction::InitLP { matcher_program, matcher_context, fee_payment } => {
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
                let config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let (auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(a_vault, &auth, &mint, &Pubkey::new_from_array(config.vault_pubkey))?;
                verify_token_account(a_user_ata, a_user.key, &mint)?;

                // Transfer base tokens to vault
                collateral::deposit(a_token, a_user_ata, a_vault, a_user, fee_payment)?;

                // Convert base tokens to units for engine
                let (units, dust) = crate::units::base_to_units(fee_payment, config.unit_scale);

                // Accumulate dust
                let old_dust = state::read_dust_base(&data);
                state::write_dust_base(&mut data, old_dust.saturating_add(dust));

                let engine = zc::engine_mut(&mut data)?;
                let idx = engine.add_lp(matcher_program.to_bytes(), matcher_context.to_bytes(), units as u128).map_err(map_risk_error)?;
                engine.set_owner(idx, a_user.key.to_bytes()).map_err(map_risk_error)?;
            },
            Instruction::DepositCollateral { user_idx, amount } => {
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
                let config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let (auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(a_vault, &auth, &mint, &Pubkey::new_from_array(config.vault_pubkey))?;
                verify_token_account(a_user_ata, a_user.key, &mint)?;

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

                engine.deposit(user_idx, units as u128).map_err(map_risk_error)?;
            },
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
                let config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, user_idx)?;

                // Owner authorization via verify helper (Kani-provable)
                let owner = engine.accounts[user_idx as usize].owner;
                if !crate::verify::owner_ok(owner, a_user.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                let (derived_pda, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                accounts::expect_key(a_vault_pda, &derived_pda)?;

                verify_vault(a_vault, &derived_pda, &mint, &Pubkey::new_from_array(config.vault_pubkey))?;
                verify_token_account(a_user_ata, a_user.key, &mint)?;

                let clock = Clock::from_account_info(a_clock)?;
                // Read oracle price (feed_id validation done inside)
                let price = oracle::read_engine_price_e6(
                    a_oracle_idx,
                    &config.index_feed_id,
                    clock.unix_timestamp,
                    config.max_staleness_secs,
                    config.conf_filter_bps,
                    config.invert,
                )?;

                // Reject misaligned withdrawal amounts (cleaner UX than silent floor)
                if config.unit_scale != 0 && amount % config.unit_scale as u64 != 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }

                // Convert requested base tokens to units
                let (units_requested, _) = crate::units::base_to_units(amount, config.unit_scale);

                engine
                    .withdraw(user_idx, units_requested as u128, clock.slot, price)
                    .map_err(map_risk_error)?;

                // Convert units back to base tokens for payout
                let base_to_pay = crate::units::units_to_base(units_requested, config.unit_scale);

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
            },
            Instruction::KeeperCrank { caller_idx, allow_panic } => {
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
                let config = state::read_config(&data);
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

                let engine = zc::engine_mut(&mut data)?;

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

                let clock = Clock::from_account_info(a_clock)?;
                // Read oracle price (feed_id validation done inside)
                let price = oracle::read_engine_price_e6(
                    a_oracle,
                    &config.index_feed_id,
                    clock.unix_timestamp,
                    config.max_staleness_secs,
                    config.conf_filter_bps,
                    config.invert,
                )?;
                // Execute crank with effective_caller_idx for clarity
                // In permissionless mode, pass CRANK_NO_CALLER to engine (out-of-range = no caller settle)
                let effective_caller_idx = if permissionless { CRANK_NO_CALLER } else { caller_idx };

                // Compute inventory-based funding rate from LP net position.
                // Engine internally gates same-slot compounding via dt = now_slot - last_funding_slot,
                // so passing the same rate multiple times in the same slot is harmless (dt=0 => no change).
                let net_lp_pos = crate::compute_net_lp_pos(engine);
                let effective_funding_rate = crate::compute_inventory_funding_bps_per_slot(net_lp_pos, price);
                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: keeper_crank_start");
                    sol_log_compute_units();
                }
                let _outcome = engine.keeper_crank(effective_caller_idx, clock.slot, price, effective_funding_rate, allow_panic != 0).map_err(map_risk_error)?;
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
                        engine.top_up_insurance_fund(units_to_sweep as u128).map_err(map_risk_error)?;
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
                let ins_low = engine.insurance_fund.balance as u64;

                // --- Threshold auto-update (rate-limited + EWMA smoothed + step-clamped)
                if clock.slot >= last_thr_slot.saturating_add(THRESH_UPDATE_INTERVAL_SLOTS) {
                    let risk_units = crate::compute_system_risk_units(engine);
                    // Convert risk_units (contracts) to notional using price
                    let risk_notional = risk_units
                        .saturating_mul(price as u128)
                        / 1_000_000;
                    // raw target: floor + risk_notional * THRESH_RISK_BPS / 10000
                    let raw_target = THRESH_FLOOR
                        .saturating_add(
                            risk_notional
                                .saturating_mul(THRESH_RISK_BPS as u128)
                                / 10_000
                        );
                    let clamped_target = raw_target.clamp(THRESH_MIN, THRESH_MAX);
                    let current = engine.risk_reduction_threshold();
                    // EWMA: new = alpha * target + (1 - alpha) * current
                    let alpha = THRESH_ALPHA_BPS as u128;
                    let smoothed = (alpha * clamped_target + (10_000 - alpha) * current) / 10_000;
                    // Step clamp: max step = THRESH_STEP_BPS / 10000 of current (but at least THRESH_MIN_STEP)
                    let max_step = (current * THRESH_STEP_BPS as u128 / 10_000)
                        .max(THRESH_MIN_STEP);
                    let final_thresh = if smoothed > current {
                        current.saturating_add(max_step.min(smoothed - current))
                    } else {
                        current.saturating_sub(max_step.min(current - smoothed))
                    };
                    engine.set_risk_reduction_threshold(final_thresh.clamp(THRESH_MIN, THRESH_MAX));
                    drop(engine);
                    state::write_last_thr_update_slot(&mut data, clock.slot);
                }

                // Write remaining dust if sweep occurred
                if let Some(dust) = remaining_dust {
                    state::write_dust_base(&mut data, dust);
                }

                // Debug: log lifetime counters (sol_log_64: tag, liqs, force, max_accounts, insurance)
                msg!("CRANK_STATS");
                sol_log_64(0xC8A4C, liqs, force, MAX_ACCOUNTS as u64, ins_low);
            },
            Instruction::TradeNoCpi { lp_idx, user_idx, size } => {
                accounts::expect_len(accounts, 5)?;
                let a_user = &accounts[0];
                let a_lp = &accounts[1];
                let a_slab = &accounts[2];
                
                accounts::expect_signer(a_user)?;
                accounts::expect_signer(a_lp)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                let config = state::read_config(&data);

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, lp_idx)?;
                check_idx(engine, user_idx)?;

                let u_owner = engine.accounts[user_idx as usize].owner;

                // Owner authorization via verify helper (Kani-provable)
                if !crate::verify::owner_ok(u_owner, a_user.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }
                let l_owner = engine.accounts[lp_idx as usize].owner;
                if !crate::verify::owner_ok(l_owner, a_lp.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                let clock = Clock::from_account_info(&accounts[3])?;
                let a_oracle = &accounts[4];

                // Read oracle price (feed_id validation done inside)
                // SECURITY: Prevents oracle substitution attacks via feed_id check
                let price = oracle::read_engine_price_e6(
                    a_oracle,
                    &config.index_feed_id,
                    clock.unix_timestamp,
                    config.max_staleness_secs,
                    config.conf_filter_bps,
                    config.invert,
                )?;

                // Gate: if insurance_fund <= threshold, only allow risk-reducing trades
                // LP delta is -size (LP takes opposite side of user's trade)
                // O(1) check after single O(n) scan
                // Gate activation via verify helper (Kani-provable)
                let bal = engine.insurance_fund.balance;
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
                    let old_lp_pos = engine.accounts[lp_idx as usize].position_size;
                    if risk_state.would_increase_risk(old_lp_pos, -size) {
                        return Err(PercolatorError::EngineRiskReductionOnlyMode.into());
                    }
                }

                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: trade_nocpi_execute_start");
                    sol_log_compute_units();
                }
                engine.execute_trade(&NoOpMatcher, lp_idx, user_idx, clock.slot, price, size).map_err(map_risk_error)?;
                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: trade_nocpi_execute_end");
                    sol_log_compute_units();
                }
            },
            Instruction::TradeCpi { lp_idx, user_idx, size } => {
                // Phase 1: Updated account layout - lp_pda must be in accounts
                accounts::expect_len(accounts, 8)?;
                let a_user = &accounts[0];
                let a_lp_owner = &accounts[1];
                let a_slab = &accounts[2];
                let a_clock = &accounts[3];
                let a_oracle = &accounts[4];
                let a_matcher_prog = &accounts[5];
                let a_matcher_ctx = &accounts[6];
                let a_lp_pda = &accounts[7];

                accounts::expect_signer(a_user)?;
                accounts::expect_signer(a_lp_owner)?;
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
                let (expected_lp_pda, bump) = Pubkey::find_program_address(
                    &[b"lp", a_slab.key.as_ref(), &lp_bytes],
                    program_id
                );
                // PDA key validation via verify helper (Kani-provable)
                if !crate::verify::pda_key_matches(expected_lp_pda.to_bytes(), a_lp_pda.key.to_bytes()) {
                    return Err(ProgramError::InvalidSeeds);
                }
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
                let (lp_account_id, config, req_id, lp_matcher_prog, lp_matcher_ctx) = {
                    let data = a_slab.try_borrow_data()?;
                    slab_guard(program_id, a_slab, &*data)?;
                    require_initialized(&*data)?;
                    let config = state::read_config(&*data);

                    // Phase 3: Monotonic nonce for req_id (prevents replay attacks)
                    // Nonce advancement via verify helper (Kani-provable)
                    let nonce = state::read_req_nonce(&*data);
                    let req_id = crate::verify::nonce_on_success(nonce);

                    let engine = zc::engine_ref(&*data)?;

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
                    (lp_acc.account_id, config, req_id, lp_acc.matcher_program, lp_acc.matcher_context)
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

                let clock = Clock::from_account_info(a_clock)?;
                // Use engine price (with inversion if configured)
                let price = oracle::read_engine_price_e6(
                    a_oracle,
                    &config.index_feed_id,
                    clock.unix_timestamp,
                    config.max_staleness_secs,
                    config.conf_filter_bps,
                    config.invert,
                )?;

                // Note: We don't zero the matcher_ctx before CPI because we don't own it.
                // Security is maintained by ABI validation which checks req_id (nonce),
                // lp_account_id, and oracle_price_e6 all match the request parameters.

                let mut cpi_data = alloc::vec::Vec::with_capacity(MATCHER_CALL_LEN);
                cpi_data.push(MATCHER_CALL_TAG);
                cpi_data.extend_from_slice(&req_id.to_le_bytes());
                cpi_data.extend_from_slice(&lp_idx.to_le_bytes());
                cpi_data.extend_from_slice(&lp_account_id.to_le_bytes());
                cpi_data.extend_from_slice(&price.to_le_bytes());
                cpi_data.extend_from_slice(&size.to_le_bytes());
                cpi_data.extend_from_slice(&[0u8; 24]); // padding to MATCHER_CALL_LEN

                #[cfg(debug_assertions)]
                {
                    if cpi_data.len() != MATCHER_CALL_LEN {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                }

                let metas = alloc::vec![
                    AccountMeta::new_readonly(*a_lp_pda.key, true), // Will become signer via invoke_signed
                    AccountMeta::new(*a_matcher_ctx.key, false),
                ];

                let ix = SolInstruction {
                    program_id: *a_matcher_prog.key,
                    accounts: metas,
                    data: cpi_data,
                };

                let bump_arr = [bump];
                let seeds: &[&[u8]] = &[b"lp", a_slab.key.as_ref(), &lp_bytes, &bump_arr];

                // Phase 2: Use zc helper for CPI - slab not passed to avoid ExternalAccountDataModified
                zc::invoke_signed_trade(&ix, a_lp_pda, a_matcher_ctx, seeds)?;

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

                let matcher = CpiMatcher { exec_price: ret.exec_price_e6, exec_size: ret.exec_size };
                {
                    let mut data = state::slab_data_mut(a_slab)?;
                    let engine = zc::engine_mut(&mut data)?;

                    // Gate: if insurance_fund <= threshold, only allow risk-reducing trades
                    // Use actual exec_size from matcher (LP delta is -exec_size)
                    // O(1) check after single O(n) scan
                    // Gate activation via verify helper (Kani-provable)
                    let bal = engine.insurance_fund.balance;
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
                        let old_lp_pos = engine.accounts[lp_idx as usize].position_size;
                        if risk_state.would_increase_risk(old_lp_pos, -ret.exec_size) {
                            return Err(PercolatorError::EngineRiskReductionOnlyMode.into());
                        }
                    }

                    // Trade size selection via verify helper (Kani-provable: uses exec_size, not requested_size)
                    let trade_size = crate::verify::cpi_trade_size(ret.exec_size, size);
                    #[cfg(feature = "cu-audit")]
                    {
                        msg!("CU_CHECKPOINT: trade_cpi_execute_start");
                        sol_log_compute_units();
                    }
                    engine.execute_trade(&matcher, lp_idx, user_idx, clock.slot, price, trade_size).map_err(map_risk_error)?;
                    #[cfg(feature = "cu-audit")]
                    {
                        msg!("CU_CHECKPOINT: trade_cpi_execute_end");
                        sol_log_compute_units();
                    }
                    // Write nonce AFTER CPI and execute_trade to avoid ExternalAccountDataModified
                    state::write_req_nonce(&mut data, req_id);
                }
            },
            Instruction::LiquidateAtOracle { target_idx } => {
                accounts::expect_len(accounts, 4)?;
                let a_slab = &accounts[1];
                let a_oracle = &accounts[3];
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                let config = state::read_config(&data);

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, target_idx)?;

                let clock = Clock::from_account_info(&accounts[2])?;
                // Use engine price (with inversion if configured)
                let price = oracle::read_engine_price_e6(
                    a_oracle,
                    &config.index_feed_id,
                    clock.unix_timestamp,
                    config.max_staleness_secs,
                    config.conf_filter_bps,
                    config.invert,
                )?;

                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: liquidate_start");
                    sol_log_compute_units();
                }
                let _res = engine.liquidate_at_oracle(target_idx, clock.slot, price).map_err(map_risk_error)?;
                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: liquidate_end");
                    sol_log_compute_units();
                }
            },
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
                let config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let (auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(a_vault, &auth, &mint, &Pubkey::new_from_array(config.vault_pubkey))?;
                verify_token_account(a_user_ata, a_user.key, &mint)?;
                accounts::expect_key(a_pda, &auth)?;

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, user_idx)?;

                // Owner authorization via verify helper (Kani-provable)
                let u_owner = engine.accounts[user_idx as usize].owner;
                if !crate::verify::owner_ok(u_owner, a_user.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                let clock = Clock::from_account_info(&accounts[6])?;
                // Use engine price (with inversion if configured)
                let price = oracle::read_engine_price_e6(
                    a_oracle,
                    &config.index_feed_id,
                    clock.unix_timestamp,
                    config.max_staleness_secs,
                    config.conf_filter_bps,
                    config.invert,
                )?;

                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: close_account_start");
                    sol_log_compute_units();
                }
                let amt_units = engine.close_account(user_idx, clock.slot, price).map_err(map_risk_error)?;
                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: close_account_end");
                    sol_log_compute_units();
                }
                let amt_units_u64: u64 = amt_units.try_into().map_err(|_| PercolatorError::EngineOverflow)?;

                // Convert units to base tokens for payout
                let base_to_pay = crate::units::units_to_base(amt_units_u64, config.unit_scale);

                let seed1: &[u8] = b"vault";
                let seed2: &[u8] = a_slab.key.as_ref();
                let bump_arr: [u8; 1] = [config.vault_authority_bump];
                let seed3: &[u8] = &bump_arr;
                let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
                let signer_seeds: [&[&[u8]]; 1] = [&seeds];

                collateral::withdraw(a_token, a_vault, a_user_ata, a_pda, base_to_pay, &signer_seeds)?;
            },
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
                let config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let (auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(a_vault, &auth, &mint, &Pubkey::new_from_array(config.vault_pubkey))?;
                verify_token_account(a_user_ata, a_user.key, &mint)?;

                // Transfer base tokens to vault
                collateral::deposit(a_token, a_user_ata, a_vault, a_user, amount)?;

                // Convert base tokens to units for engine
                let (units, dust) = crate::units::base_to_units(amount, config.unit_scale);

                // Accumulate dust
                let old_dust = state::read_dust_base(&data);
                state::write_dust_base(&mut data, old_dust.saturating_add(dust));

                let engine = zc::engine_mut(&mut data)?;
                engine.top_up_insurance_fund(units as u128).map_err(map_risk_error)?;
            },
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

                let engine = zc::engine_mut(&mut data)?;
                engine.set_risk_reduction_threshold(new_threshold);
            }

            Instruction::UpdateAdmin { new_admin } => {
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

                header.admin = new_admin.to_bytes();
                state::write_header(&mut data, &header);
            }

            Instruction::CloseSlab => {
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

                // Safety checks: ensure market is empty before closing (skip in test mode)
                #[cfg(not(feature = "test"))]
                {
                    let engine = zc::engine_ref(&data)?;
                    if engine.vault != 0 {
                        return Err(PercolatorError::EngineInsufficientBalance.into());
                    }
                    if engine.insurance_fund.balance != 0 {
                        return Err(PercolatorError::EngineInsufficientBalance.into());
                    }
                    if engine.num_used_accounts != 0 {
                        return Err(PercolatorError::EngineAccountNotFound.into());
                    }
                }

                // Zero out the slab data to prevent reuse
                for b in data.iter_mut() {
                    *b = 0;
                }
                drop(data);

                // Transfer all lamports from slab to admin
                let slab_lamports = a_slab.lamports();
                **a_slab.lamports.borrow_mut() = 0;
                **a_admin.lamports.borrow_mut() = a_admin
                    .lamports()
                    .checked_add(slab_lamports)
                    .ok_or(PercolatorError::EngineOverflow)?;
            }
        }
        Ok(())
    }
}

// 10. mod entrypoint
pub mod entrypoint {
    #[allow(unused_imports)]
    use alloc::format; // Required by entrypoint! macro in SBF builds
    use solana_program::{
        account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, pubkey::Pubkey,
    };
    use crate::processor;

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
    pub use percolator::{RiskEngine, RiskParams, RiskError, NoOpMatcher, MatchingEngine, TradeExecution};
}

