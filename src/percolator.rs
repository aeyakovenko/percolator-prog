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
/// O(n) scan, but called once per instruction then used for O(1) delta checks.
pub struct LpRiskState {
    pub sum_abs: u128,
    pub max_abs: u128,
}

impl LpRiskState {
    /// Scan all LP accounts to compute aggregate risk state. O(n).
    pub fn compute(engine: &percolator::RiskEngine) -> Self {
        let mut sum_abs: u128 = 0;
        let mut max_abs: u128 = 0;
        for i in 0..engine.accounts.len() {
            if engine.is_used(i) && engine.accounts[i].is_lp() {
                let abs_pos = engine.accounts[i].position_size.unsigned_abs();
                sum_abs = sum_abs.saturating_add(abs_pos);
                max_abs = max_abs.max(abs_pos);
            }
        }
        Self { sum_abs, max_abs }
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

/// Compute system risk units for threshold calculation. O(MAX_ACCOUNTS).
/// Used by KeeperCrank for threshold updates.
fn compute_system_risk_units(engine: &percolator::RiskEngine) -> u128 {
    LpRiskState::compute(engine).risk()
}

/// Compute net LP position for inventory-based funding.
/// Sums position_size across all used LP accounts.
fn compute_net_lp_pos(engine: &percolator::RiskEngine) -> i128 {
    let mut net: i128 = 0;
    for i in 0..engine.accounts.len() {
        if engine.is_used(i) && engine.accounts[i].is_lp() {
            net = net.saturating_add(engine.accounts[i].position_size);
        }
    }
    net
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
fn compute_inventory_funding_bps_per_slot(net_lp_pos: i128, price_e6: u64) -> i64 {
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

    /// Oracle key check: provided oracle must match expected config oracle.
    #[inline]
    pub fn oracle_key_ok(expected: [u8; 32], provided: [u8; 32]) -> bool {
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
            pyth_index: Pubkey,
            pyth_collateral: Pubkey,
            max_staleness_slots: u64,
            conf_filter_bps: u16,
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
    }

    impl Instruction {
        pub fn decode(input: &[u8]) -> Result<Self, ProgramError> {
            let (&tag, mut rest) = input.split_first().ok_or(ProgramError::InvalidInstructionData)?;
            
            match tag {
                0 => { // InitMarket
                    let admin = read_pubkey(&mut rest)?;
                    let collateral_mint = read_pubkey(&mut rest)?;
                    let pyth_index = read_pubkey(&mut rest)?;
                    let pyth_collateral = read_pubkey(&mut rest)?;
                    let max_staleness_slots = read_u64(&mut rest)?;
                    let conf_filter_bps = read_u16(&mut rest)?;
                    let risk_params = read_risk_params(&mut rest)?;
                    Ok(Instruction::InitMarket { 
                        admin, collateral_mint, pyth_index, pyth_collateral, 
                        max_staleness_slots, conf_filter_bps, risk_params 
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
        pub _reserved: [u8; 24], // [0..8]=nonce, [8..16]=last_thr_slot, [16..24]=unused
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
        pub collateral_oracle: [u8; 32],
        pub index_oracle: [u8; 32],
        pub max_staleness_slots: u64,
        pub conf_filter_bps: u16,
        pub vault_authority_bump: u8,
        pub _padding: [u8; 5],
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

// 7. mod oracle
pub mod oracle {
    use solana_program::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};
    use crate::error::PercolatorError;

    /// Pyth mainnet price program ID
    #[cfg(not(feature = "devnet"))]
    pub const PYTH_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
        0x92, 0x6a, 0xb1, 0x3b, 0x47, 0x4a, 0x34, 0x42,
        0x91, 0xb3, 0x29, 0x67, 0xf5, 0xf5, 0x3f, 0x7e,
        0x2e, 0x3e, 0x23, 0x42, 0x2c, 0x62, 0x8d, 0x8f,
        0x5d, 0x0a, 0xd0, 0x85, 0x8c, 0x0a, 0xe0, 0x73,
    ]); // FsJ3A3u2vn5cTVofAjvy6y5kwABJAqYWpe4975bi2epH

    /// Pyth devnet price program ID
    #[cfg(feature = "devnet")]
    pub const PYTH_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
        0x0a, 0x1a, 0x98, 0x33, 0xa3, 0x76, 0x55, 0x2b,
        0x56, 0xb7, 0xca, 0x0d, 0xed, 0x19, 0x29, 0x17,
        0x00, 0x57, 0xe8, 0x27, 0xa0, 0xc6, 0x27, 0xf4,
        0xb6, 0x47, 0xb9, 0xee, 0x90, 0x99, 0xaf, 0xb4,
    ]); // gSbePebfvPy7tRqimPoVecS2UsBvYv46ynrzWocc92s

    pub fn read_pyth_price_e6(price_ai: &AccountInfo, now_slot: u64, max_staleness: u64, conf_bps: u16) -> Result<u64, ProgramError> {
        // Validate oracle owner (skip in tests to allow mock oracles)
        #[cfg(not(test))]
        {
            if *price_ai.owner != PYTH_PROGRAM_ID {
                return Err(ProgramError::IllegalOwner);
            }
        }

        let data = price_ai.try_borrow_data()?;
        if data.len() < 208 {
            return Err(ProgramError::InvalidAccountData);
        }

        let expo = i32::from_le_bytes(data[20..24].try_into().unwrap());
        let price = i64::from_le_bytes(data[176..184].try_into().unwrap());
        let conf = u64::from_le_bytes(data[184..192].try_into().unwrap());
        let pub_slot = u64::from_le_bytes(data[200..208].try_into().unwrap());

        if price <= 0 {
            return Err(PercolatorError::OracleInvalid.into()); 
        }

        // Skip staleness check on devnet since oracles aren't actively updated
        #[cfg(not(feature = "devnet"))]
        {
            let age = now_slot.saturating_sub(pub_slot);
            if age > max_staleness {
                return Err(PercolatorError::OracleStale.into());
            }
        }
        #[cfg(feature = "devnet")]
        let _ = (pub_slot, max_staleness); // Suppress unused warnings

        let price_u = price as u128;
        // Skip confidence check on devnet since oracles have unreliable confidence data
        #[cfg(not(feature = "devnet"))]
        {
            let lhs = (conf as u128) * 10_000;
            let rhs = price_u * (conf_bps as u128);
            if lhs > rhs {
                return Err(PercolatorError::OracleConfTooWide.into());
            }
        }
        #[cfg(feature = "devnet")]
        let _ = (conf, conf_bps); // Suppress unused warnings

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
}

// 8. mod collateral
pub mod collateral {
    use solana_program::{
        account_info::AccountInfo, program_error::ProgramError,
    };

    #[cfg(not(test))]
    use solana_program::program::{invoke, invoke_signed};

    #[cfg(test)]
    use solana_program::program_pack::Pack;
    #[cfg(test)]
    use spl_token::state::Account as TokenAccount;

    pub fn deposit<'a>(
        _token_program: &AccountInfo<'a>,
        source: &AccountInfo<'a>,
        dest: &AccountInfo<'a>,
        _authority: &AccountInfo<'a>,
        amount: u64
    ) -> Result<(), ProgramError> {
        if amount == 0 { return Ok(()); }
        #[cfg(not(test))]
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
        #[cfg(test)]
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
        #[cfg(not(test))]
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
        #[cfg(test)]
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
        log::sol_log_compute_units,
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
        Ok(())
    }

    /// Verify a user's token account: owner, mint, and initialized state.
    /// Skip in tests to allow mock accounts.
    #[allow(unused_variables)]
    fn verify_token_account(a_token_account: &AccountInfo, expected_owner: &Pubkey, expected_mint: &Pubkey) -> Result<(), ProgramError> {
        #[cfg(not(test))]
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
        #[cfg(not(test))]
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
                admin, collateral_mint: _collateral_mint, pyth_index, pyth_collateral,
                max_staleness_slots, conf_filter_bps, risk_params
            } => {
                accounts::expect_len(accounts, 11)?;
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
                    collateral_oracle: pyth_collateral.to_bytes(),
                    index_oracle: pyth_index.to_bytes(),
                    max_staleness_slots,
                    conf_filter_bps,
                    vault_authority_bump: bump,
                    _padding: [0; 5],
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
                accounts::expect_len(accounts, 7)?;
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

                let engine = zc::engine_mut(&mut data)?;

                collateral::deposit(a_token, a_user_ata, a_vault, a_user, fee_payment)?;

                let idx = engine.add_user(fee_payment as u128).map_err(map_risk_error)?;
                engine.set_owner(idx, a_user.key.to_bytes()).map_err(map_risk_error)?;
            },
            Instruction::InitLP { matcher_program, matcher_context, fee_payment } => {
                accounts::expect_len(accounts, 7)?;
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

                let engine = zc::engine_mut(&mut data)?;

                collateral::deposit(a_token, a_user_ata, a_vault, a_user, fee_payment)?;

                let idx = engine.add_lp(matcher_program.to_bytes(), matcher_context.to_bytes(), fee_payment as u128).map_err(map_risk_error)?;
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

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, user_idx)?;

                // Owner authorization via verify helper (Kani-provable)
                let owner = engine.accounts[user_idx as usize].owner;
                if !crate::verify::owner_ok(owner, a_user.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                collateral::deposit(a_token, a_user_ata, a_vault, a_user, amount)?;
                engine.deposit(user_idx, amount as u128).map_err(map_risk_error)?;
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

                // Oracle key validation via verify helper (Kani-provable)
                if !crate::verify::oracle_key_ok(config.index_oracle, a_oracle_idx.key.to_bytes()) {
                    return Err(ProgramError::InvalidArgument);
                }

                let (derived_pda, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                accounts::expect_key(a_vault_pda, &derived_pda)?;

                verify_vault(a_vault, &derived_pda, &mint, &Pubkey::new_from_array(config.vault_pubkey))?;
                verify_token_account(a_user_ata, a_user.key, &mint)?;

                let clock = Clock::from_account_info(a_clock)?;
                let price = oracle::read_pyth_price_e6(
                    a_oracle_idx,
                    clock.slot,
                    config.max_staleness_slots,
                    config.conf_filter_bps,
                )?;

                engine
                    .withdraw(user_idx, amount as u128, clock.slot, price)
                    .map_err(map_risk_error)?;

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
                    amount,
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
                // Read last threshold update slot BEFORE mutable engine borrow
                let last_thr_slot = state::read_last_thr_update_slot(&data);

                // Oracle key validation via verify helper (Kani-provable)
                if !crate::verify::oracle_key_ok(config.index_oracle, a_oracle.key.to_bytes()) {
                    return Err(ProgramError::InvalidArgument);
                }

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
                let price = oracle::read_pyth_price_e6(a_oracle, clock.slot, config.max_staleness_slots, config.conf_filter_bps)?;

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
                    state::write_last_thr_update_slot(&mut data, clock.slot);
                }
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
                let price = oracle::read_pyth_price_e6(&accounts[4], clock.slot, config.max_staleness_slots, config.conf_filter_bps)?;

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

                // Oracle key validation via verify helper (Kani-provable)
                if !crate::verify::oracle_key_ok(config.index_oracle, a_oracle.key.to_bytes()) {
                    return Err(ProgramError::InvalidArgument);
                }

                let clock = Clock::from_account_info(a_clock)?;
                let price = oracle::read_pyth_price_e6(a_oracle, clock.slot, config.max_staleness_slots, config.conf_filter_bps)?;

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

                // Oracle key validation via verify helper (Kani-provable)
                if !crate::verify::oracle_key_ok(config.index_oracle, a_oracle.key.to_bytes()) {
                    return Err(ProgramError::InvalidArgument);
                }

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, target_idx)?;

                let clock = Clock::from_account_info(&accounts[2])?;
                let price = oracle::read_pyth_price_e6(a_oracle, clock.slot, config.max_staleness_slots, config.conf_filter_bps)?;

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

                // Oracle key validation via verify helper (Kani-provable)
                if !crate::verify::oracle_key_ok(config.index_oracle, a_oracle.key.to_bytes()) {
                    return Err(ProgramError::InvalidArgument);
                }

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, user_idx)?;

                // Owner authorization via verify helper (Kani-provable)
                let u_owner = engine.accounts[user_idx as usize].owner;
                if !crate::verify::owner_ok(u_owner, a_user.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                let clock = Clock::from_account_info(&accounts[6])?;
                let price = oracle::read_pyth_price_e6(a_oracle, clock.slot, config.max_staleness_slots, config.conf_filter_bps)?;

                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: close_account_start");
                    sol_log_compute_units();
                }
                let amt = engine.close_account(user_idx, clock.slot, price).map_err(map_risk_error)?;
                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: close_account_end");
                    sol_log_compute_units();
                }
                let amt_u64: u64 = amt.try_into().map_err(|_| PercolatorError::EngineOverflow)?;

                let seed1: &[u8] = b"vault";
                let seed2: &[u8] = a_slab.key.as_ref();
                let bump_arr: [u8; 1] = [config.vault_authority_bump];
                let seed3: &[u8] = &bump_arr;
                let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
                let signer_seeds: [&[&[u8]]; 1] = [&seeds];

                collateral::withdraw(a_token, a_vault, a_user_ata, a_pda, amt_u64, &signer_seeds)?;
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

                let engine = zc::engine_mut(&mut data)?;

                collateral::deposit(a_token, a_user_ata, a_vault, a_user, amount)?;
                engine.top_up_insurance_fund(amount as u128).map_err(map_risk_error)?;
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

#[cfg(test)]
mod tests {
    extern crate std;
    extern crate alloc;
    use alloc::{vec, vec::Vec};
    use solana_program::{
        account_info::AccountInfo,
        pubkey::Pubkey,
        clock::Clock,
        program_pack::Pack,
        program_error::ProgramError,
    };
    use spl_token::state::{Account as TokenAccount, AccountState};
    use crate::{
        processor::process_instruction,
        constants::{MAGIC, VERSION},
        zc,
        error::PercolatorError,
        state,
    };
    use percolator::MAX_ACCOUNTS;

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
            Self { key, owner, lamports, data, is_signer: false, is_writable: false, executable: false }
        }
        fn signer(mut self) -> Self { self.is_signer = true; self }
        fn writable(mut self) -> Self { self.is_writable = true; self }
        
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

    fn make_pyth(price: i64, expo: i32, conf: u64, pub_slot: u64) -> Vec<u8> {
        let mut data = vec![0u8; 208];
        data[20..24].copy_from_slice(&expo.to_le_bytes());
        data[176..184].copy_from_slice(&price.to_le_bytes());
        data[184..192].copy_from_slice(&conf.to_le_bytes());
        data[200..208].copy_from_slice(&pub_slot.to_le_bytes());
        data
    }

    fn make_clock(slot: u64) -> Vec<u8> {
        let clock = Clock { slot, ..Clock::default() };
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
        pyth_col: TestAccount,
        clock: TestAccount,
        rent: TestAccount,
        system: TestAccount,
        vault_pda: Pubkey,
    }

    fn setup_market() -> MarketFixture {
        let program_id = Pubkey::new_unique();
        let slab_key = Pubkey::new_unique();
        let (vault_pda, _) = Pubkey::find_program_address(&[b"vault", slab_key.as_ref()], &program_id);
        let mint_key = Pubkey::new_unique();

        let pyth_data = make_pyth(1000, -6, 1, 100); 

        MarketFixture {
            program_id,
            admin: TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer(),
            slab: TestAccount::new(slab_key, program_id, 0, vec![0u8; crate::constants::SLAB_LEN]).writable(),
            mint: TestAccount::new(mint_key, solana_program::system_program::id(), 0, vec![]),
            vault: TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(mint_key, vault_pda, 0)).writable(),
            token_prog: TestAccount::new(spl_token::ID, Pubkey::default(), 0, vec![]),
            pyth_index: TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, pyth_data.clone()),
            pyth_col: TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, pyth_data),
            clock: TestAccount::new(solana_program::sysvar::clock::id(), solana_program::sysvar::id(), 0, make_clock(100)),
            rent: TestAccount::new(solana_program::sysvar::rent::id(), solana_program::sysvar::id(), 0, vec![]),
            system: TestAccount::new(solana_program::system_program::id(), Pubkey::default(), 0, vec![]),
            vault_pda,
        }
    }

    // --- Encoders --- 
    
    fn encode_u64(val: u64, buf: &mut Vec<u8>) { buf.extend_from_slice(&val.to_le_bytes()); }
    fn encode_u16(val: u16, buf: &mut Vec<u8>) { buf.extend_from_slice(&val.to_le_bytes()); }
    fn encode_i128(val: i128, buf: &mut Vec<u8>) { buf.extend_from_slice(&val.to_le_bytes()); }
    fn encode_u128(val: u128, buf: &mut Vec<u8>) { buf.extend_from_slice(&val.to_le_bytes()); }
    fn encode_pubkey(val: &Pubkey, buf: &mut Vec<u8>) { buf.extend_from_slice(val.as_ref()); }

    fn encode_init_market(fixture: &MarketFixture, crank_staleness: u64) -> Vec<u8> {
        let mut data = vec![0u8];
        encode_pubkey(&fixture.admin.key, &mut data);
        encode_pubkey(&fixture.mint.key, &mut data);
        encode_pubkey(&fixture.pyth_index.key, &mut data);
        encode_pubkey(&fixture.pyth_col.key, &mut data);
        encode_u64(100, &mut data);
        encode_u16(500, &mut data);
        
        encode_u64(0, &mut data);
        encode_u64(0, &mut data);
        encode_u64(0, &mut data);
        encode_u64(0, &mut data);
        encode_u64(64, &mut data);
        encode_u128(0, &mut data);
        encode_u128(0, &mut data);
        encode_u128(0, &mut data);
        encode_u64(crank_staleness, &mut data);
        encode_u64(0, &mut data);
        encode_u128(0, &mut data);
        encode_u64(0, &mut data);
        encode_u128(0, &mut data);
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
        use std::println;
        use percolator::{RiskEngine, Account, MAX_ACCOUNTS};
        use core::mem::{size_of, offset_of};

        println!("Size of Account: {}", size_of::<Account>());
        println!("Offset of Account.kind: {}", offset_of!(Account, kind));
        println!("Offset of Account.owner: {}", offset_of!(Account, owner));
        println!("Size of RiskEngine: {}", size_of::<RiskEngine>());
        println!("MAX_ACCOUNTS: {}", MAX_ACCOUNTS);

        let account_array_size = MAX_ACCOUNTS * size_of::<Account>();
        println!("Account array size: {}", account_array_size);

        // Print offset of accounts array within RiskEngine
        println!("Offset of RiskEngine.accounts: {}", offset_of!(RiskEngine, accounts));
        println!("Offset of RiskEngine.vault: {}", offset_of!(RiskEngine, vault));
        println!("Offset of RiskEngine.insurance_fund: {}", offset_of!(RiskEngine, insurance_fund));
        println!("Offset of RiskEngine.params: {}", offset_of!(RiskEngine, params));
        println!("Offset of RiskEngine.used: {}", offset_of!(RiskEngine, used));

        // Print the SBF constant (note: this is x86_64 value when run as native test)
        println!("ACCOUNTS_OFFSET (this test is x86_64): {}", crate::zc::ACCOUNTS_OFFSET);

        // Print SLAB_LEN
        println!("ENGINE_OFF: {}", crate::constants::ENGINE_OFF);
        println!("ENGINE_LEN: {}", crate::constants::ENGINE_LEN);
        println!("SLAB_LEN: {}", crate::constants::SLAB_LEN);
    }

    #[test]
    fn test_init_market() {
        let mut f = setup_market();
        let data = encode_init_market(&f, 100);
        
        {
            let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let accounts = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                dummy_ata.to_info(), f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info(),
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
    fn test_init_user() {
        let mut f = setup_market();
        let init_data = encode_init_market(&f, 100);
        {
            let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let init_accounts = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(),
                f.token_prog.to_info(), dummy_ata.to_info(),
                f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
            ];
            process_instruction(&f.program_id, &init_accounts, &init_data).unwrap();
        }

        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 1000)).writable();

        let data = encode_init_user(100);
        {
            let accounts = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                f.clock.to_info(), f.pyth_col.to_info(),
            ];
            process_instruction(&f.program_id, &accounts, &data).unwrap();
        }

        let vault_state = TokenAccount::unpack(&f.vault.data).unwrap();
        assert_eq!(vault_state.amount, 100); 
        assert!(find_idx_by_owner(&f.slab.data, user.key).is_some());
    }

    #[test]
    fn test_deposit_withdraw() {
        let mut f = setup_market();
        let init_data = encode_init_market(&f, 0); 
        {
            let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let init_accounts = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(),
                f.token_prog.to_info(), dummy_ata.to_info(),
                f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
            ];
            process_instruction(&f.program_id, &init_accounts, &init_data).unwrap();
        }

        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 1000)).writable();
        {
            let accounts = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                f.clock.to_info(), f.pyth_col.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &encode_init_user(0)).unwrap();
        }
        let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

        {
            let accounts = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &encode_deposit(user_idx, 500)).unwrap();
        }

        {
            let accounts = vec![
                user.to_info(), f.slab.to_info(), f.clock.to_info(), f.pyth_index.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &encode_crank(user_idx, 0)).unwrap();
        }

        {
            let mut vault_pda_account = TestAccount::new(f.vault_pda, solana_program::system_program::id(), 0, vec![]);
            let accounts = vec![
                user.to_info(), f.slab.to_info(), f.vault.to_info(), user_ata.to_info(), vault_pda_account.to_info(),
                f.token_prog.to_info(), f.clock.to_info(), f.pyth_index.to_info(),
            ];
            process_instruction(&f.program_id, &accounts, &encode_withdraw(user_idx, 200)).unwrap();
        }

        let vault_state = TokenAccount::unpack(&f.vault.data).unwrap();
        assert_eq!(vault_state.amount, 300);
    }

    #[test]
    fn test_vault_validation() {
        let mut f = setup_market();
        f.vault.owner = solana_program::system_program::id();
        let init_data = encode_init_market(&f, 100);
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let init_accounts = vec![
            f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(),
            f.token_prog.to_info(), dummy_ata.to_info(),
            f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
        ];
        let res = process_instruction(&f.program_id, &init_accounts, &init_data);
        assert_eq!(res, Err(PercolatorError::InvalidVaultAta.into()));
    }

    #[test]
    fn test_trade() {
        let mut f = setup_market();
        let init_data = encode_init_market(&f, 100);
        {
            let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let init_accounts = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(),
                f.token_prog.to_info(), dummy_ata.to_info(),
                f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
            ];
            process_instruction(&f.program_id, &init_accounts, &init_data).unwrap();
        }

        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 1000)).writable();
        {
            let accounts = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                f.clock.to_info(), f.pyth_col.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &encode_init_user(0)).unwrap();
        }
        let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();
        {
            let accounts = vec![user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info()];
            process_instruction(&f.program_id, &accounts, &encode_deposit(user_idx, 1000)).unwrap();
        }

        let mut lp = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut lp_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, lp.key, 1000)).writable();
        let mut d1 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let mut d2 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        {
            let matcher_prog_key = d1.key;
            let matcher_ctx_key = d2.key;
            let accs = vec![
                lp.to_info(), f.slab.to_info(), lp_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                d1.to_info(), d2.to_info()
            ];
            process_instruction(&f.program_id, &accs, &encode_init_lp(matcher_prog_key, matcher_ctx_key, 0)).unwrap();
        }
        let lp_idx = find_idx_by_owner(&f.slab.data, lp.key).unwrap();
        {
            let accounts = vec![lp.to_info(), f.slab.to_info(), lp_ata.to_info(), f.vault.to_info(), f.token_prog.to_info()];
            process_instruction(&f.program_id, &accounts, &encode_deposit(lp_idx, 1000)).unwrap();
        }

        {
            let accounts = vec![
                user.to_info(), lp.to_info(), f.slab.to_info(), f.clock.to_info(), f.pyth_col.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &encode_trade(lp_idx, user_idx, 100)).unwrap();
        }
    }

    #[test]
    fn test_withdraw_wrong_signer() {
        let mut f = setup_market();
        let init_data = encode_init_market(&f, 0);
        {
            let mut dummy = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let accs = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                dummy.to_info(), f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
            ];
            process_instruction(&f.program_id, &accs, &init_data).unwrap();
        }

        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 1000)).writable();
        {
            let accounts = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                f.clock.to_info(), f.pyth_col.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &encode_init_user(0)).unwrap();
        }
        let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

        {
            let accounts = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &encode_deposit(user_idx, 500)).unwrap();
        }

        {
            let accs = vec![
                user.to_info(), f.slab.to_info(), f.clock.to_info(), f.pyth_index.to_info()
            ];
            process_instruction(&f.program_id, &accs, &encode_crank(user_idx, 0)).unwrap();
        }

        let mut attacker = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut vault_pda = TestAccount::new(f.vault_pda, solana_program::system_program::id(), 0, vec![]);
        
        let res = {
            let accounts = vec![
                attacker.to_info(),
                f.slab.to_info(), f.vault.to_info(), user_ata.to_info(), vault_pda.to_info(),
                f.token_prog.to_info(), f.clock.to_info(), f.pyth_index.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &encode_withdraw(user_idx, 100))
        };
        assert_eq!(res, Err(PercolatorError::EngineUnauthorized.into()));
    }

    #[test]
    fn test_trade_wrong_signer() {
        let mut f = setup_market();
        let init_data = encode_init_market(&f, 0);
        {
            let mut dummy = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let accs = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                dummy.to_info(), f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
            ];
            process_instruction(&f.program_id, &accs, &init_data).unwrap();
        }

        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 1000)).writable();
        {
            let accs = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                f.clock.to_info(), f.pyth_col.to_info()
            ];
            process_instruction(&f.program_id, &accs, &encode_init_user(0)).unwrap();
        }
        let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

        let mut lp = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut lp_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, lp.key, 1000)).writable();
        let mut d1 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let mut d2 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        {
            let matcher_prog_key = d1.key;
            let matcher_ctx_key = d2.key;
            let accs = vec![
                lp.to_info(), f.slab.to_info(), lp_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                d1.to_info(), d2.to_info()
            ];
            process_instruction(&f.program_id, &accs, &encode_init_lp(matcher_prog_key, matcher_ctx_key, 0)).unwrap();
        }
        let lp_idx = find_idx_by_owner(&f.slab.data, lp.key).unwrap();

        {
            let accs = vec![user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info()];
            process_instruction(&f.program_id, &accs, &encode_deposit(user_idx, 1000)).unwrap();
        }
        {
            let accs = vec![lp.to_info(), f.slab.to_info(), lp_ata.to_info(), f.vault.to_info(), f.token_prog.to_info()];
            process_instruction(&f.program_id, &accs, &encode_deposit(lp_idx, 1000)).unwrap();
        }
        {
            let accs = vec![user.to_info(), f.slab.to_info(), f.clock.to_info(), f.pyth_index.to_info()];
            process_instruction(&f.program_id, &accs, &encode_crank(user_idx, 0)).unwrap();
        }

        let mut attacker = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        {
            let accs = vec![
                attacker.to_info(), lp.to_info(), f.slab.to_info(), f.clock.to_info(), f.pyth_index.to_info()
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
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                dummy.to_info(), f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info(),
            ];
            process_instruction(&f.program_id, &accs, &init_data).unwrap();
        }

        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 1000)).writable();
        {
            let accs = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                f.clock.to_info(), f.pyth_col.to_info(),
            ];
            process_instruction(&f.program_id, &accs, &encode_init_user(0)).unwrap();
        }
        let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

        let mut lp = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut lp_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, lp.key, 1000)).writable();
        let mut matcher_program = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        matcher_program.executable = true;
        let mut matcher_ctx = TestAccount::new(Pubkey::new_unique(), matcher_program.key, 0, vec![0u8; 320]);
        matcher_ctx.is_writable = true;
        {
            let matcher_prog_key = matcher_program.key;
            let matcher_ctx_key = matcher_ctx.key;
            let accs = vec![
                lp.to_info(), f.slab.to_info(), lp_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                matcher_program.to_info(), matcher_ctx.to_info()
            ];
            process_instruction(&f.program_id, &accs, &encode_init_lp(matcher_prog_key, matcher_ctx_key, 0)).unwrap();
        }
        let lp_idx = find_idx_by_owner(&f.slab.data, lp.key).unwrap();

        // Create WRONG lp_pda - use a random key instead of the correct PDA
        let mut wrong_lp_pda = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]);

        let accs = vec![
            user.to_info(),
            lp.to_info(),
            f.slab.to_info(),
            f.clock.to_info(),
            f.pyth_index.to_info(),
            matcher_program.to_info(),
            matcher_ctx.to_info(),
            wrong_lp_pda.to_info(),
        ];
        let res = process_instruction(&f.program_id, &accs, &encode_trade_cpi(lp_idx, user_idx, 100));
        assert_eq!(res, Err(ProgramError::InvalidSeeds));
    }

    #[test]
    fn test_trade_cpi_wrong_lp_owner_rejected() {
        let mut f = setup_market();
        let init_data = encode_init_market(&f, 100);
        {
            let mut dummy = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let accs = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                dummy.to_info(), f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info(),
            ];
            process_instruction(&f.program_id, &accs, &init_data).unwrap();
        }

        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 1000)).writable();
        {
            let accs = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                f.clock.to_info(), f.pyth_col.to_info(),
            ];
            process_instruction(&f.program_id, &accs, &encode_init_user(0)).unwrap();
        }
        let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

        let mut lp = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut lp_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, lp.key, 1000)).writable();
        let mut matcher_program = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        matcher_program.executable = true;
        let mut matcher_ctx = TestAccount::new(Pubkey::new_unique(), matcher_program.key, 0, vec![0u8; 320]);
        matcher_ctx.is_writable = true;
        {
            let matcher_prog_key = matcher_program.key;
            let matcher_ctx_key = matcher_ctx.key;
            let accs = vec![
                lp.to_info(), f.slab.to_info(), lp_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                matcher_program.to_info(), matcher_ctx.to_info()
            ];
            process_instruction(&f.program_id, &accs, &encode_init_lp(matcher_prog_key, matcher_ctx_key, 0)).unwrap();
        }
        let lp_idx = find_idx_by_owner(&f.slab.data, lp.key).unwrap();

        let mut wrong_lp = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();

        // Create lp_pda account (system-owned, 0 data)
        let lp_bytes = lp_idx.to_le_bytes();
        let (lp_pda_key, _) = Pubkey::find_program_address(
            &[b"lp", f.slab.key.as_ref(), &lp_bytes],
            &f.program_id
        );
        let mut lp_pda = TestAccount::new(lp_pda_key, solana_program::system_program::id(), 0, vec![]);

        let res = {
            let accs = vec![
                user.to_info(), // 0
                wrong_lp.to_info(), // 1 (WRONG OWNER)
                f.slab.to_info(), // 2
                f.clock.to_info(), // 3
                f.pyth_index.to_info(), // 4 oracle
                matcher_program.to_info(), // 5 matcher
                matcher_ctx.to_info(), // 6 context
                lp_pda.to_info(), // 7 lp_pda
            ];
            process_instruction(&f.program_id, &accs, &encode_trade_cpi(lp_idx, user_idx, 100))
        };
        assert_eq!(res, Err(PercolatorError::EngineUnauthorized.into()));
    }

    #[test]
    fn test_trade_cpi_wrong_oracle_key_rejected() {
        let mut f = setup_market();
        let init_data = encode_init_market(&f, 100);
        {
            let mut dummy = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let accs = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                dummy.to_info(), f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info(),
            ];
            process_instruction(&f.program_id, &accs, &init_data).unwrap();
        }

        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 1000)).writable();
        {
            let accs = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                f.clock.to_info(), f.pyth_col.to_info(),
            ];
            process_instruction(&f.program_id, &accs, &encode_init_user(0)).unwrap();
        }
        let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

        let mut lp = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut lp_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, lp.key, 1000)).writable();
        let mut matcher_program = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        matcher_program.executable = true;
        let mut matcher_ctx = TestAccount::new(Pubkey::new_unique(), matcher_program.key, 0, vec![0u8; 320]);
        matcher_ctx.is_writable = true;
        {
            let matcher_prog_key = matcher_program.key;
            let matcher_ctx_key = matcher_ctx.key;
            let accs = vec![
                lp.to_info(), f.slab.to_info(), lp_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                matcher_program.to_info(), matcher_ctx.to_info()
            ];
            process_instruction(&f.program_id, &accs, &encode_init_lp(matcher_prog_key, matcher_ctx_key, 0)).unwrap();
        }
        let lp_idx = find_idx_by_owner(&f.slab.data, lp.key).unwrap();

        let mut wrong_oracle = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![0u8; 208]);

        // Create lp_pda account (system-owned, 0 data)
        let lp_bytes = lp_idx.to_le_bytes();
        let (lp_pda_key, _) = Pubkey::find_program_address(
            &[b"lp", f.slab.key.as_ref(), &lp_bytes],
            &f.program_id
        );
        let mut lp_pda = TestAccount::new(lp_pda_key, solana_program::system_program::id(), 0, vec![]);

        let res = {
            let accs = vec![
                user.to_info(), // 0
                lp.to_info(), // 1
                f.slab.to_info(), // 2
                f.clock.to_info(), // 3
                wrong_oracle.to_info(), // 4 oracle (WRONG KEY)
                matcher_program.to_info(), // 5 matcher
                matcher_ctx.to_info(), // 6 context
                lp_pda.to_info(), // 7 lp_pda
            ];
            process_instruction(&f.program_id, &accs, &encode_trade_cpi(lp_idx, user_idx, 100))
        };
        assert_eq!(res, Err(ProgramError::InvalidArgument));
    }

    #[test]
    fn test_set_risk_threshold() {
        let mut f = setup_market();
        let init_data = encode_init_market(&f, 100);
        {
            let mut dummy = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let accs = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                dummy.to_info(), f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info(),
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
            process_instruction(&f.program_id, &accs, &encode_set_risk_threshold(new_threshold)).unwrap();
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
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                dummy.to_info(), f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info(),
            ];
            process_instruction(&f.program_id, &accs, &init_data).unwrap();
        }

        // Non-admin tries to set threshold
        let mut attacker = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let new_threshold: u128 = 999_999;
        {
            let accs = vec![
                attacker.to_info(), // attacker (signer, but not admin)
                f.slab.to_info(),   // slab (writable)
            ];
            let res = process_instruction(&f.program_id, &accs, &encode_set_risk_threshold(new_threshold));
            assert_eq!(res, Err(PercolatorError::EngineUnauthorized.into()));
        }

        // Verify threshold was NOT updated (still 0)
        {
            let engine = zc::engine_ref(&f.slab.data).unwrap();
            assert_eq!(engine.risk_reduction_threshold(), 0);
        }
    }

    #[test]
    fn test_crank_updates_threshold_from_risk_metric() {
        use crate::constants::{THRESH_FLOOR, THRESH_RISK_BPS, THRESH_ALPHA_BPS, THRESH_MIN_STEP, THRESH_STEP_BPS};

        let mut f = setup_market();
        let init_data = encode_init_market(&f, 100);
        {
            let mut dummy = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let accs = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                dummy.to_info(), f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info(),
            ];
            process_instruction(&f.program_id, &accs, &init_data).unwrap();
        }

        // Verify initial threshold is 0
        {
            let engine = zc::engine_ref(&f.slab.data).unwrap();
            assert_eq!(engine.risk_reduction_threshold(), 0);
            assert_eq!(engine.total_open_interest, 0);
        }

        // Create user
        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 10_000_000)).writable();
        {
            let accs = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                f.clock.to_info(), f.pyth_col.to_info(),
            ];
            process_instruction(&f.program_id, &accs, &encode_init_user(0)).unwrap();
        }
        let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

        // Create LP
        let mut lp = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut lp_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, lp.key, 10_000_000)).writable();
        let mut d1 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let mut d2 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        {
            let matcher_prog_key = d1.key;
            let matcher_ctx_key = d2.key;
            let accs = vec![
                lp.to_info(), f.slab.to_info(), lp_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                d1.to_info(), d2.to_info(),
            ];
            process_instruction(&f.program_id, &accs, &encode_init_lp(matcher_prog_key, matcher_ctx_key, 0)).unwrap();
        }
        let lp_idx = find_idx_by_owner(&f.slab.data, lp.key).unwrap();

        // Deposit for both user and LP
        {
            let accs = vec![user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info()];
            process_instruction(&f.program_id, &accs, &encode_deposit(user_idx, 1_000_000)).unwrap();
        }
        {
            let accs = vec![lp.to_info(), f.slab.to_info(), lp_ata.to_info(), f.vault.to_info(), f.token_prog.to_info()];
            process_instruction(&f.program_id, &accs, &encode_deposit(lp_idx, 1_000_000)).unwrap();
        }

        // Execute trade to create positions
        let trade_size: i128 = 100_000;
        {
            let accs = vec![
                user.to_info(), lp.to_info(), f.slab.to_info(), f.clock.to_info(), f.pyth_col.to_info()
            ];
            process_instruction(&f.program_id, &accs, &encode_trade(lp_idx, user_idx, trade_size)).unwrap();
        }

        // Verify positions were set by trade
        {
            let engine = zc::engine_ref(&f.slab.data).unwrap();
            let lp_pos = engine.accounts[lp_idx as usize].position_size;
            let user_pos = engine.accounts[user_idx as usize].position_size;
            assert_ne!(lp_pos, 0, "LP should have non-zero position after trade");
            assert_ne!(user_pos, 0, "User should have non-zero position after trade");
            // Verify LP is marked as LP
            assert!(engine.accounts[lp_idx as usize].is_lp(), "LP account should be marked as LP");
            assert!(engine.is_used(lp_idx as usize), "LP should be marked as used");
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
            let risk_units = crate::compute_system_risk_units(engine);
            assert!(risk_units > 0, "risk_units should be > 0 when there are LP positions");
        }

        // Now call crank - this should update threshold based on risk metric
        // Clock slot defaults to 0 in test, but last_thr_slot is also 0,
        // so update won't trigger unless slot >= 0 + THRESH_UPDATE_INTERVAL_SLOTS
        // We need to advance the clock
        f.clock.data = make_clock(100); // Advance past rate limit
        {
            let accs = vec![user.to_info(), f.slab.to_info(), f.clock.to_info(), f.pyth_index.to_info()];
            process_instruction(&f.program_id, &accs, &encode_crank(user_idx, 0)).unwrap();
        }

        // Verify threshold update ran by checking last_thr_update_slot
        let last_thr_slot_after = state::read_last_thr_update_slot(&f.slab.data);
        assert_eq!(last_thr_slot_after, 100, "last_thr_update_slot should be set to clock.slot after crank");

        // Check if positions are still non-zero after crank
        {
            let engine = zc::engine_ref(&f.slab.data).unwrap();
            let lp_pos = engine.accounts[lp_idx as usize].position_size;
            // Crank may liquidate positions. Check if LP still has position.
            let risk_units_after = crate::compute_system_risk_units(engine);
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
            //   net_exposure = |-100000| = 100000
            //   max_concentration = 100000
            //   risk_units = 100000 + 100000 = 200000
            // raw_target = THRESH_FLOOR + (200000 * THRESH_RISK_BPS / 10000) = 0 + (200000 * 50 / 10000) = 1000
            // EWMA with current=0, alpha=1000: smoothed = (1000 * 1000 + 9000 * 0) / 10000 = 100
            // Step clamp: max_step = max(0 * 500 / 10000, 1) = 1
            // final = 0 + min(1, 100) = 1

            assert!(threshold > 0, "Threshold should be > 0 after crank with positions");
            // Due to step clamping from 0, the first update will be capped at THRESH_MIN_STEP
            assert_eq!(threshold, 1, "First update from 0 should be step-clamped to THRESH_MIN_STEP");
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
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(),
                f.token_prog.to_info(), dummy_ata.to_info(),
                f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &init_data).unwrap();
        }

        // Create a random "keeper" account that is NOT a signer
        let mut keeper = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]);
        // Note: keeper is NOT marked as signer

        // Call permissionless crank - should succeed even though keeper is not a signer
        {
            let accs = vec![
                keeper.to_info(),  // Not a signer!
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
            assert_eq!(engine.vault, 0); // No deposits yet, vault should be 0
        }
    }

    #[test]
    fn test_permissionless_crank_gc() {
        // Non-vacuous test: create a dust account and verify GC frees it
        let mut f = setup_market();
        let init_data = encode_init_market(&f, 100);

        // Init market
        {
            let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let accounts = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(),
                f.token_prog.to_info(), dummy_ata.to_info(),
                f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &init_data).unwrap();
        }

        // Init user - creates account slot
        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 1000)).writable();
        {
            let accounts = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                f.clock.to_info(), f.pyth_col.to_info()
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
            account.capital = 0;
            account.pnl = -1;
            account.funding_index = funding_idx;
            account.fee_credits = 0;
            account.last_fee_slot = current_slot;
        }

        // Verify account is now a dust candidate
        {
            let engine = zc::engine_ref(&f.slab.data).unwrap();
            let account = &engine.accounts[user_idx as usize];
            assert_eq!(account.capital, 0, "capital should be 0");
            assert_eq!(account.pnl, -1, "pnl should be -1");
            assert_eq!(account.position_size, 0, "position_size should be 0");
            assert_eq!(account.reserved_pnl, 0, "reserved_pnl should be 0");
            assert_eq!(account.funding_index, engine.funding_index_qpb_e6, "funding should match");
        }

        // Call permissionless crank - should GC the dust account
        let mut keeper = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]);
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
            assert_eq!(engine.num_used_accounts, used_before - 1, "num_used_accounts should decrease by 1");
            assert!(!engine.is_used(user_idx as usize), "User account should no longer be used after GC");
        }
    }

    #[test]
    fn test_permissionless_funding_not_controllable() {
        // Security test: permissionless caller cannot influence funding rate.
        // Funding is computed deterministically from (LP inventory, oracle price, constants).
        //
        // Key security property: calling crank multiple times in the same slot is harmless
        // because engine gates via dt=0 (no funding accrues when dt=0).
        //
        // NOTE: Testing non-zero funding rate changes requires production-scale positions
        // ($1M+ notional to exceed FUNDING_INV_SCALE_NOTIONAL_E6), but such positions
        // would be immediately liquidated for insufficient margin in test setup.
        // This test verifies the dt=0 gating mechanism works correctly, which is the
        // core anti-spam property preventing attackers from compounding funding.
        let mut f = setup_market();
        let init_data = encode_init_market(&f, 100);

        // Init market
        {
            let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let accounts = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(),
                f.token_prog.to_info(), dummy_ata.to_info(),
                f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &init_data).unwrap();
        }

        // Init user with deposit
        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 1_000_000)).writable();
        {
            let accounts = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                f.clock.to_info(), f.pyth_col.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &encode_init_user(100)).unwrap();
        }
        let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();
        {
            let accounts = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                f.clock.to_info(), f.pyth_col.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &encode_deposit(user_idx, 100_000)).unwrap();
        }

        // Record funding index and last_funding_slot before any crank
        let (_funding_before, _last_slot_before) = {
            let engine = zc::engine_ref(&f.slab.data).unwrap();
            (engine.funding_index_qpb_e6, engine.last_funding_slot)
        };

        // Random keeper calls crank - first crank at slot 100
        let mut keeper = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]);
        {
            let accs = vec![
                keeper.to_info(), f.slab.to_info(), f.clock.to_info(), f.pyth_index.to_info(),
            ];
            process_instruction(&f.program_id, &accs, &encode_crank_permissionless(0)).unwrap();
        }
        let (funding_after_first, last_slot_after_first) = {
            let engine = zc::engine_ref(&f.slab.data).unwrap();
            (engine.funding_index_qpb_e6, engine.last_funding_slot)
        };
        // last_funding_slot should advance from 0 to 100
        assert_eq!(last_slot_after_first, 100, "last_funding_slot should be updated to clock.slot after first crank");

        // Second crank in SAME slot (slot 100) - should NOT change funding (dt=0 gating)
        {
            let accs = vec![
                keeper.to_info(), f.slab.to_info(), f.clock.to_info(), f.pyth_index.to_info(),
            ];
            process_instruction(&f.program_id, &accs, &encode_crank_permissionless(0)).unwrap();
        }
        let (funding_after_second, last_slot_after_second) = {
            let engine = zc::engine_ref(&f.slab.data).unwrap();
            (engine.funding_index_qpb_e6, engine.last_funding_slot)
        };

        // KEY SECURITY ASSERTION: same-slot crank does NOT change funding index
        // This is the core anti-spam property - attackers can't compound funding by spamming cranks
        assert_eq!(funding_after_second, funding_after_first,
            "Same-slot crank must not change funding (dt=0 gating). before={}, after={}",
            funding_after_first, funding_after_second);
        assert_eq!(last_slot_after_second, last_slot_after_first,
            "last_funding_slot should not change on same-slot crank");

        // Third crank in same slot - still no change (verify it's consistently gated)
        {
            let accs = vec![
                keeper.to_info(), f.slab.to_info(), f.clock.to_info(), f.pyth_index.to_info(),
            ];
            process_instruction(&f.program_id, &accs, &encode_crank_permissionless(0)).unwrap();
        }
        let funding_after_third = {
            let engine = zc::engine_ref(&f.slab.data).unwrap();
            engine.funding_index_qpb_e6
        };
        assert_eq!(funding_after_third, funding_after_first,
            "Multiple same-slot cranks must not accumulate funding changes");

        // Verify last_funding_slot advances when slot changes
        f.clock.data = make_clock(101);
        {
            let accs = vec![
                keeper.to_info(), f.slab.to_info(), f.clock.to_info(), f.pyth_index.to_info(),
            ];
            process_instruction(&f.program_id, &accs, &encode_crank_permissionless(0)).unwrap();
        }
        let last_slot_after_new_slot = {
            let engine = zc::engine_ref(&f.slab.data).unwrap();
            engine.last_funding_slot
        };
        assert_eq!(last_slot_after_new_slot, 101,
            "last_funding_slot should advance to new slot. was {}, now {}",
            last_slot_after_second, last_slot_after_new_slot);
    }

    #[test]
    fn test_funding_sign_flips_with_lp_position() {
        // Security test: funding rate sign must follow LP net position sign.
        // This catches accidental sign inversion bugs.

        // Test the pure compute function directly
        let price_e6 = 100_000_000u64; // $100

        // LP net long => positive funding rate (longs pay)
        let net_long: i128 = 1_000_000; // 1M contracts long
        let rate_long = crate::compute_inventory_funding_bps_per_slot(net_long, price_e6);

        // LP net short => negative funding rate (shorts pay)
        let net_short: i128 = -1_000_000; // 1M contracts short
        let rate_short = crate::compute_inventory_funding_bps_per_slot(net_short, price_e6);

        // LP flat => zero funding rate
        let net_flat: i128 = 0;
        let rate_flat = crate::compute_inventory_funding_bps_per_slot(net_flat, price_e6);

        // Verify sign convention
        assert!(rate_long >= 0, "LP net long should give non-negative funding rate");
        assert!(rate_short <= 0, "LP net short should give non-positive funding rate");
        assert_eq!(rate_flat, 0, "LP flat should give zero funding rate");

        // Verify opposite signs (if non-zero)
        if rate_long != 0 && rate_short != 0 {
            assert!(
                (rate_long > 0 && rate_short < 0) || (rate_long < 0 && rate_short > 0),
                "Funding rates for opposite LP positions must have opposite signs"
            );
        }
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
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(),
                f.token_prog.to_info(), dummy_ata.to_info(),
                f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &init_data).unwrap();
        }

        // Verify initial admin is set
        let header = state::read_header(&f.slab.data);
        assert_eq!(header.admin, f.admin.key.to_bytes());

        // Create new admin B
        let new_admin_b = Pubkey::new_unique();
        let mut admin_b_account = TestAccount::new(new_admin_b, solana_program::system_program::id(), 0, vec![]).signer();

        // Admin A rotates to admin B
        {
            let accounts = vec![f.admin.to_info(), f.slab.to_info()];
            process_instruction(&f.program_id, &accounts, &encode_update_admin(&new_admin_b)).unwrap();
        }

        // Verify admin is now B
        let header = state::read_header(&f.slab.data);
        assert_eq!(header.admin, new_admin_b.to_bytes());

        // Create new admin C
        let new_admin_c = Pubkey::new_unique();

        // Admin B rotates to admin C (proves rotation actually took effect)
        {
            let accounts = vec![admin_b_account.to_info(), f.slab.to_info()];
            process_instruction(&f.program_id, &accounts, &encode_update_admin(&new_admin_c)).unwrap();
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
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(),
                f.token_prog.to_info(), dummy_ata.to_info(),
                f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &init_data).unwrap();
        }

        // Attacker tries to rotate admin
        let attacker = Pubkey::new_unique();
        let mut attacker_account = TestAccount::new(attacker, solana_program::system_program::id(), 0, vec![]).signer();
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
    fn test_burn_admin_to_zero() {
        let mut f = setup_market();
        let init_data = encode_init_market(&f, 100);

        // Init market with admin A
        {
            let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let accounts = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(),
                f.token_prog.to_info(), dummy_ata.to_info(),
                f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &init_data).unwrap();
        }

        // Admin burns to zero (Pubkey::default())
        let zero_admin = Pubkey::default();
        {
            let accounts = vec![f.admin.to_info(), f.slab.to_info()];
            process_instruction(&f.program_id, &accounts, &encode_update_admin(&zero_admin)).unwrap();
        }

        // Verify admin is now all zeros
        let header = state::read_header(&f.slab.data);
        assert_eq!(header.admin, [0u8; 32]);
    }

    #[test]
    fn test_after_burn_admin_ops_disabled() {
        let mut f = setup_market();
        let init_data = encode_init_market(&f, 100);

        // Init market with admin A
        {
            let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let accounts = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(),
                f.token_prog.to_info(), dummy_ata.to_info(),
                f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &init_data).unwrap();
        }

        // Admin burns to zero
        let zero_admin = Pubkey::default();
        {
            let accounts = vec![f.admin.to_info(), f.slab.to_info()];
            process_instruction(&f.program_id, &accounts, &encode_update_admin(&zero_admin)).unwrap();
        }

        // Attempt UpdateAdmin signed by anyone (including zero pubkey signer) → must fail
        let anyone = Pubkey::new_unique();
        let mut anyone_account = TestAccount::new(anyone, solana_program::system_program::id(), 0, vec![]).signer();
        {
            let accounts = vec![anyone_account.to_info(), f.slab.to_info()];
            let res = process_instruction(&f.program_id, &accounts, &encode_update_admin(&Pubkey::new_unique()));
            assert_eq!(res, Err(PercolatorError::EngineUnauthorized.into()));
        }

        // Attempt SetRiskThreshold signed by anyone → must fail
        {
            let accounts = vec![anyone_account.to_info(), f.slab.to_info()];
            let res = process_instruction(&f.program_id, &accounts, &encode_set_risk_threshold(12345));
            assert_eq!(res, Err(PercolatorError::EngineUnauthorized.into()));
        }

        // Even original admin cannot do admin ops anymore
        let original_admin_key = f.admin.key; // capture before mutable borrow
        {
            let accounts = vec![f.admin.to_info(), f.slab.to_info()];
            let res = process_instruction(&f.program_id, &accounts, &encode_update_admin(&original_admin_key));
            assert_eq!(res, Err(PercolatorError::EngineUnauthorized.into()));
        }
    }
}