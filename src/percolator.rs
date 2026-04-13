//! Percolator: Single-file Solana program with embedded Risk Engine.

#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

use solana_program::declare_id;

declare_id!("Perco1ator111111111111111111111111111111111");

// 1. mod constants
pub mod constants {
    use crate::state::{MarketConfig, SlabHeader};
    use core::mem::{align_of, size_of};
    use percolator::RiskEngine;

    pub const MAGIC: u64 = 0x504552434f4c4154; // "PERCOLAT"

    pub const HEADER_LEN: usize = size_of::<SlabHeader>();
    pub const CONFIG_LEN: usize = size_of::<MarketConfig>();
    pub const ENGINE_ALIGN: usize = align_of::<RiskEngine>();

    pub const fn align_up(x: usize, a: usize) -> usize {
        (x + (a - 1)) & !(a - 1)
    }

    pub const ENGINE_OFF: usize = align_up(HEADER_LEN + CONFIG_LEN, ENGINE_ALIGN);
    pub const ENGINE_LEN: usize = size_of::<RiskEngine>();

    // RiskBuffer: 4-entry persistent cache of highest-notional accounts
    pub const RISK_BUF_CAP: usize = 4;
    // RISK_BUF_EMPTY removed — buffer uses zeroed entries, not sentinels
    pub const RISK_BUF_OFF: usize = ENGINE_OFF + ENGINE_LEN;
    pub const RISK_BUF_LEN: usize = size_of::<crate::risk_buffer::RiskBuffer>();
    pub const SLAB_LEN: usize = RISK_BUF_OFF + RISK_BUF_LEN;

    // CRANK_REWARD_MIN_DT removed — crank discount logic removed in v12.15
    /// Progressive scan window per crank.
    pub const RISK_SCAN_WINDOW: usize = 32;
    pub const MATCHER_ABI_VERSION: u32 = 1;
    // MATCHER_CONTEXT_PREFIX_LEN removed — validation uses MATCHER_CONTEXT_LEN directly
    pub const MATCHER_CONTEXT_LEN: usize = 320;
    pub const MATCHER_CALL_TAG: u8 = 0;
    pub const MATCHER_CALL_LEN: usize = 67;

    /// Sentinel value for permissionless crank (no caller account required)
    pub const CRANK_NO_CALLER: u16 = u16::MAX;

    /// Maximum allowed unit_scale for InitMarket.
    /// unit_scale=0 disables scaling (1:1 base tokens to units, dust=0 always).
    /// unit_scale=1..=1_000_000_000 enables scaling with dust tracking.
    pub const MAX_UNIT_SCALE: u32 = 1_000_000_000;

    // Default funding parameters (used at init_market, can be changed via update_config)
    pub const DEFAULT_FUNDING_HORIZON_SLOTS: u64 = 500; // ~4 min @ ~2 slots/sec
    pub const DEFAULT_FUNDING_K_BPS: u64 = 100; // 1.00x multiplier
    pub const DEFAULT_FUNDING_MAX_PREMIUM_BPS: i64 = 500; // cap premium at 5.00%
    pub const DEFAULT_FUNDING_MAX_BPS_PER_SLOT: i64 = 5; // cap per-slot funding
    pub const DEFAULT_HYPERP_PRICE_CAP_E2BPS: u64 = 10_000; // 1% per slot max price change for Hyperp
    pub const MAX_ORACLE_PRICE_CAP_E2BPS: u64 = 1_000_000; // 100% — hard ceiling for circuit breaker
    pub const DEFAULT_INSURANCE_WITHDRAW_MIN_BASE: u64 = 1;
    pub const DEFAULT_INSURANCE_WITHDRAW_MAX_BPS: u16 = 100; // 1%
    pub const DEFAULT_INSURANCE_WITHDRAW_COOLDOWN_SLOTS: u64 = 400_000;
    pub const DEFAULT_MARK_EWMA_HALFLIFE_SLOTS: u64 = 100; // ~40 sec @ 2.5 slots/sec

}

// 1b. Insurance withdraw helpers

// Packed insurance-withdraw metadata in config.authority_timestamp (i64/u64):
// [max_withdraw_bps:16][last_withdraw_slot:48]
pub const INS_WITHDRAW_LAST_SLOT_MASK: u64 = (1u64 << 48) - 1;
// Sentinel in the 48-bit slot field meaning "no successful limited withdraw yet".
const INS_WITHDRAW_LAST_SLOT_NONE: u64 = INS_WITHDRAW_LAST_SLOT_MASK;

#[inline]
pub fn pack_ins_withdraw_meta(max_bps: u16, last_slot: u64) -> Option<i64> {
    if max_bps == 0 || max_bps > 10_000 || last_slot > INS_WITHDRAW_LAST_SLOT_MASK {
        return None;
    }
    let packed = ((max_bps as u64) << 48) | last_slot;
    Some(packed as i64)
}

#[inline]
pub fn unpack_ins_withdraw_meta(packed: i64) -> (u16, u64) {
    let raw = packed as u64;
    let max_bps = ((raw >> 48) & 0xFFFF) as u16;
    let last_slot = raw & INS_WITHDRAW_LAST_SLOT_MASK;
    (max_bps, last_slot)
}

// =============================================================================
// Pure helpers for Kani verification (program-level invariants only)
// =============================================================================

// 1b. mod risk_buffer
pub mod risk_buffer {
    use bytemuck::{Pod, Zeroable};
    use crate::constants::RISK_BUF_CAP;

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct RiskEntry {
        pub idx: u16,
        pub _pad: [u8; 14],
        pub notional: u128,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct RiskBuffer {
        pub scan_cursor: u16,
        pub count: u8,
        pub _pad: [u8; 13],
        pub min_notional: u128,
        pub entries: [RiskEntry; RISK_BUF_CAP],
    }

    impl RiskBuffer {
        pub fn recompute_min(&mut self) {
            self.min_notional = match self.count {
                0 => 0,
                1 => self.entries[0].notional,
                2 => core::cmp::min(
                    self.entries[0].notional,
                    self.entries[1].notional,
                ),
                3 => core::cmp::min(
                    self.entries[0].notional,
                    core::cmp::min(
                        self.entries[1].notional,
                        self.entries[2].notional,
                    ),
                ),
                _ => core::cmp::min(
                    core::cmp::min(
                        self.entries[0].notional,
                        self.entries[1].notional,
                    ),
                    core::cmp::min(
                        self.entries[2].notional,
                        self.entries[3].notional,
                    ),
                ),
            };
        }

        pub fn find(&self, idx: u16) -> Option<usize> {
            if self.count > 0 && self.entries[0].idx == idx { return Some(0); }
            if self.count > 1 && self.entries[1].idx == idx { return Some(1); }
            if self.count > 2 && self.entries[2].idx == idx { return Some(2); }
            if self.count > 3 && self.entries[3].idx == idx { return Some(3); }
            None
        }

        fn min_slot(&self) -> usize {
            let mut m = 0;
            if self.count > 1 && self.entries[1].notional < self.entries[m].notional { m = 1; }
            if self.count > 2 && self.entries[2].notional < self.entries[m].notional { m = 2; }
            if self.count > 3 && self.entries[3].notional < self.entries[m].notional { m = 3; }
            m
        }

        /// Insert or update. Returns true if buffer changed.
        pub fn upsert(&mut self, idx: u16, notional: u128) -> bool {
            if let Some(slot) = self.find(idx) {
                if self.entries[slot].notional == notional {
                    return false;
                }
                self.entries[slot].notional = notional;
                self.recompute_min();
                return true;
            }
            if (self.count as usize) < RISK_BUF_CAP {
                let s = self.count as usize;
                self.entries[s].idx = idx;
                self.entries[s].notional = notional;
                self.entries[s]._pad = [0; 14];
                self.count += 1;
                self.recompute_min();
                return true;
            }
            if notional <= self.min_notional {
                return false;
            }
            let victim = self.min_slot();
            self.entries[victim].idx = idx;
            self.entries[victim].notional = notional;
            self.entries[victim]._pad = [0; 14];
            self.recompute_min();
            true
        }

        /// Remove by idx. Swap-remove with last.
        pub fn remove(&mut self, idx: u16) -> bool {
            let slot = match self.find(idx) {
                Some(s) => s,
                None => return false,
            };
            let last = self.count as usize - 1;
            if slot != last {
                self.entries[slot] = self.entries[last];
            }
            self.entries[last] = RiskEntry::zeroed();
            self.count -= 1;
            self.recompute_min();
            true
        }
    }
}

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
    /// Used by: UpdateAdmin, UpdateConfig, SetOracleAuthority
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

    // LP PDA shape check removed — PDA key match is sufficient.
    // Only this program can sign for the PDA (invoke_signed), so it's
    // always system-owned with zero data. Extra checks wasted CUs.

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
    /// * `lp_key_ok` - Whether provided LP owner key matches stored LP owner.
    ///   NOTE: Runtime TradeCpi does NOT require LP owner to be a signer.
    ///   LP authorization is delegated to the matcher program at registration
    ///   time — the CPI identity binding (matcher_identity_ok) is the actual
    ///   LP-side authorization gate. This parameter models key-equality only.
    /// * `exec_size` - The exec_size from matcher return
    #[inline]
    pub fn decide_trade_cpi(
        old_nonce: u64,
        shape: MatcherAccountsShape,
        identity_ok: bool,
        pda_ok: bool,
        abi_ok: bool,
        user_auth_ok: bool,
        lp_key_ok: bool,
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
        // 3. Owner authorization (user signer + LP key equality)
        if !user_auth_ok || !lp_key_ok {
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
    /// * `lp_key_ok` - Whether provided LP owner key matches stored LP owner
    ///   (key-equality only, not signer — see decide_trade_cpi docs)
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
        lp_key_ok: bool,
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
        // 3. Owner authorization (user signer + LP key equality)
        if !user_auth_ok || !lp_key_ok {
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
    /// * `lp_auth_ok` - Whether LP signer matches stored LP owner.
    ///   NOTE: TradeNoCpi requires LP to be a signer (unlike TradeCpi).
    #[inline]
    pub fn decide_trade_nocpi(
        user_auth_ok: bool,
        lp_auth_ok: bool,
    ) -> TradeNoCpiDecision {
        if !user_auth_ok || !lp_auth_ok {
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

    /// Decision for admin operations (UpdateAdmin, UpdateConfig, SetOracleAuthority, etc.).
    #[inline]
    pub fn decide_admin_op(admin: [u8; 32], signer: [u8; 32]) -> SimpleDecision {
        if admin_ok(admin, signer) {
            SimpleDecision::Accept
        } else {
            SimpleDecision::Reject
        }
    }

    // =========================================================================
    // KeeperCrank decision logic
    // =========================================================================

    /// Decision for KeeperCrank authorization.
    /// Permissionless: always accept.
    /// Self-crank: requires idx exists and owner match.
    #[inline]
    pub fn decide_keeper_crank(
        permissionless: bool,
        idx_exists: bool,
        stored_owner: [u8; 32],
        signer: [u8; 32],
    ) -> SimpleDecision {
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

    /// Convert a raw oracle price to engine-space: invert then scale.
    /// All Hyperp internal prices (authority_price_e6, last_effective_price_e6)
    /// must be in engine-space. Apply this at every ingress point:
    /// InitMarket, PushOraclePrice, TradeCpi mark-update.
    #[inline]
    pub fn to_engine_price(raw: u64, invert: u8, unit_scale: u32) -> Option<u64> {
        let after_invert = invert_price_e6(raw, invert)?;
        scale_price_e6(after_invert, unit_scale)
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
    // Mark EWMA (trade-derived mark price)
    // =========================================================================

    /// Choose the clamp base for mark EWMA updates.
    /// Always clamps against the index (last_effective_price_e6),
    /// never against the mark itself. This bounds mark-index
    /// divergence to one cap-width regardless of wash-trade duration.
    #[inline]
    pub fn mark_ewma_clamp_base(last_effective_price_e6: u64) -> u64 {
        last_effective_price_e6.max(1)
    }

    /// EWMA update for mark price tracking.
    ///
    /// Computes: new = old * (1 - alpha) + price * alpha
    /// where alpha ≈ dt / (dt + halflife)  (Padé approximant of 1 - 2^(-dt/hl))
    ///
    /// Returns old unchanged if dt == 0 (same-slot protection).
    /// Returns price directly if old == 0 (first update) or halflife == 0 (instant).
    #[inline]
    pub fn ewma_update(
        old: u64,
        price: u64,
        halflife_slots: u64,
        last_slot: u64,
        now_slot: u64,
        fee_paid: u64,
        mark_min_fee: u64,
    ) -> u64 {
        // First update: seed EWMA to price, but only if fee threshold is met.
        // This prevents dust trades from bootstrapping the mark on non-Hyperp markets.
        if old == 0 {
            if mark_min_fee > 0 && fee_paid < mark_min_fee { return 0; }
            return price;
        }
        let dt = now_slot.saturating_sub(last_slot);
        if dt == 0 { return old; }
        if halflife_slots == 0 { return price; }
        // Zero fee with weighting enabled: no mark movement
        if fee_paid == 0 && mark_min_fee > 0 { return old; }

        let alpha_bps = (10_000u128 * dt as u128) / (dt as u128 + halflife_slots as u128);

        // Fee weighting: scale alpha by min(fee_paid/mark_min_fee, 1).
        // Trades below the fee threshold get proportionally reduced mark influence.
        // This makes wash trading cost-proportional: to move the mark like a
        // legitimate trade, the attacker must burn the same fee into insurance.
        let effective_alpha_bps = if mark_min_fee == 0
            || fee_paid >= mark_min_fee
        {
            alpha_bps
        } else {
            alpha_bps * (fee_paid as u128) / (mark_min_fee as u128)
        };

        let old128 = old as u128;
        let price128 = price as u128;
        let result = if price >= old {
            let delta = price128 - old128;
            old128 + (delta * effective_alpha_bps / 10_000)
        } else {
            let delta = old128 - price128;
            old128 - (delta * effective_alpha_bps / 10_000)
        };
        core::cmp::min(result, u64::MAX as u128) as u64
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

    /// Offset of side_mode_long within RiskEngine (repr(u8) enum)
    const SM_LONG_OFF: usize = offset_of!(RiskEngine, side_mode_long);
    /// Offset of side_mode_short within RiskEngine (repr(u8) enum)
    const SM_SHORT_OFF: usize = offset_of!(RiskEngine, side_mode_short);
    /// Offset of market_mode within RiskEngine (repr(u8) enum)
    const MM_OFF: usize = offset_of!(RiskEngine, market_mode);

    /// Validate ALL enum discriminants from raw bytes BEFORE casting to &RiskEngine.
    ///
    /// RiskEngine contains these types with invalid bit patterns:
    ///   - SideMode (2 instances): valid 0-2
    ///   - MarketMode (1 instance): valid 0-1
    ///   - Account.overflow_older_present / overflow_newest_present (bool):
    ///     valid 0-1, but per-account validation is O(MAX_ACCOUNTS) so we rely
    ///     on the slab being program-owned (only typed Rust writes touch these).
    ///
    /// Account.kind was changed from AccountKind enum to plain u8, eliminating
    /// the UB class at the type level — u8 has no invalid representations.
    #[inline]
    fn validate_raw_discriminants(data: &[u8]) -> Result<(), ProgramError> {
        let base = ENGINE_OFF;
        // SideMode: valid 0 (Normal), 1 (DrainOnly), 2 (ResetPending)
        let sm_long = data[base + SM_LONG_OFF];
        let sm_short = data[base + SM_SHORT_OFF];
        if sm_long > 2 || sm_short > 2 {
            return Err(ProgramError::InvalidAccountData);
        }
        // MarketMode: valid 0 (Live), 1 (Resolved)
        let mm = data[base + MM_OFF];
        if mm > 1 {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(())
    }

    pub fn engine_ref<'a>(data: &'a [u8]) -> Result<&'a RiskEngine, ProgramError> {
        // Require full ENGINE_LEN to avoid UB from reference extending past buffer
        if data.len() < ENGINE_OFF + ENGINE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let ptr = unsafe { data.as_ptr().add(ENGINE_OFF) };
        if (ptr as usize) % ENGINE_ALIGN != 0 {
            return Err(ProgramError::InvalidAccountData);
        }
        // Validate enum discriminants from raw bytes before creating reference
        validate_raw_discriminants(data)?;
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
        validate_raw_discriminants(data)?;
        Ok(unsafe { &mut *(ptr as *mut RiskEngine) })
    }

    // NOTE: engine_write was removed because it requires passing RiskEngine by value,
    // which stack-allocates the ~6MB struct and causes stack overflow in BPF.
    // Use engine_mut() + init_in_place() instead for initialization.

    use solana_program::{
        account_info::AccountInfo, instruction::Instruction as SolInstruction,
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
        a_matcher_prog: &AccountInfo<'a>,
        seeds: &[&[u8]],
    ) -> Result<(), ProgramError> {
        let infos = [
            a_lp_pda.clone(),
            a_matcher_ctx.clone(),
            a_matcher_prog.clone(),
        ];
        invoke_signed(ix, &infos, &[seeds])
    }
}

pub mod matcher_abi {
    use crate::constants::MATCHER_ABI_VERSION;
    use solana_program::program_error::ProgramError;

    /// Matcher return flags
    pub const FLAG_VALID: u32 = 1; // bit0: response is valid
    pub const FLAG_PARTIAL_OK: u32 = 2; // bit1: partial fill including zero allowed
    pub const FLAG_REJECTED: u32 = 4; // bit2: trade rejected by matcher

    /// Matcher return structure (ABI v1).
    /// IMPORTANT: exec_price_e6 must be in engine-space (already inverted
    /// and scaled). The matcher receives oracle_price_e6 in engine-space
    /// and must return exec_price_e6 in the same space. The wrapper stores
    /// it directly as the Hyperp mark price without re-normalization.
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
        if req_size != 0 {
            if ret.exec_size.signum() != req_size.signum() {
                return Err(ProgramError::InvalidAccountData);
            }
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
        EngineCorruptState,
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
            RiskError::PnlNotWarmedUp => PercolatorError::EnginePnlNotWarmedUp,
            RiskError::Overflow => PercolatorError::EngineOverflow,
            RiskError::AccountNotFound => PercolatorError::EngineAccountNotFound,
            RiskError::SideBlocked => PercolatorError::EngineRiskReductionOnlyMode,
            RiskError::CorruptState => PercolatorError::EngineCorruptState,
        };
        ProgramError::Custom(err as u32)
    }
}

// 4. mod ix
pub mod ix {
    use percolator::{RiskParams, U128};
    use solana_program::{program_error::ProgramError, pubkey::Pubkey};

    #[derive(Debug)]
    pub enum Instruction {
        InitMarket {
            admin: Pubkey,
            collateral_mint: Pubkey,
            /// Pyth feed ID for the index price (32 bytes).
            /// If all zeros, enables Hyperp mode (internal mark/index, no external oracle).
            index_feed_id: [u8; 32],
            /// Maximum staleness in seconds
            max_staleness_secs: u64,
            conf_filter_bps: u16,
            /// If non-zero, invert oracle price (raw -> 1e12/raw)
            invert: u8,
            /// Lamports per Unit for boundary conversion (0 = no scaling)
            unit_scale: u32,
            /// Initial mark price in e6 format. Required (non-zero) if Hyperp mode.
            initial_mark_price_e6: u64,
            /// Periodic maintenance fee per slot per account (engine units). 0 = disabled.
            maintenance_fee_per_slot: u128,
            /// Per-market admin limit: max insurance floor
            max_insurance_floor: u128,
            /// Per-market admin limit: min oracle price cap (e2bps floor for non-zero values)
            min_oracle_price_cap_e2bps: u64,
            /// Insurance withdrawal: max bps per withdrawal (0 = no live withdrawals)
            insurance_withdraw_max_bps: u16,
            /// Insurance withdrawal: cooldown slots between withdrawals
            insurance_withdraw_cooldown_slots: u64,
            risk_params: RiskParams,
            insurance_floor: u128,
            /// Slots of oracle staleness for permissionless resolution. 0 = disabled.
            permissionless_resolve_stale_slots: u64,
            /// Optional custom funding parameters (override defaults when present)
            funding_horizon_slots: Option<u64>,
            funding_k_bps: Option<u64>,
            funding_max_premium_bps: Option<i64>,
            funding_max_bps_per_slot: Option<i64>,
            /// Fee-weighted EWMA: min fee for full mark weight. 0 = disabled.
            mark_min_fee: u64,
            /// Permissionless force-close delay after resolution. 0 = disabled.
            force_close_delay_slots: u64,
        },
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
            candidates: alloc::vec::Vec<(u16, Option<percolator::LiquidationPolicy>)>,
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
            limit_price_e6: u64, // 0 = no limit (backward compat)
        },
        UpdateAdmin {
            new_admin: Pubkey,
        },
        /// Close the market slab and recover SOL to admin.
        /// Requires: no active accounts, no vault funds, no insurance funds.
        CloseSlab,
        /// Update configurable funding parameters. Admin only.
        UpdateConfig {
            funding_horizon_slots: u64,
            funding_k_bps: u64,
            funding_max_premium_bps: i64,
            funding_max_bps_per_slot: i64,
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
        /// Set limited insurance-withdraw policy (admin only, resolved market).
        SetInsuranceWithdrawPolicy {
            authority: Pubkey,
            min_withdraw_base: u64,
            max_withdraw_bps: u16,
            cooldown_slots: u64,
        },
        /// Withdraw insurance under configured min/max/cooldown constraints.
        WithdrawInsuranceLimited {
            amount: u64,
        },
        /// Admin force-close an abandoned account after market resolution.
        /// Requires RESOLVED flag, zero position, admin signer.
        AdminForceCloseAccount {
            user_idx: u16,
        },
        /// Query cumulative fees earned by an LP position (§2.2).
        /// Returns fees_earned_total via set_return_data. No state mutation.
        QueryLpFees {
            lp_idx: u16,
        },
        /// Permissionless reclamation of empty/dust accounts (§2.6, §10.7).
        ReclaimEmptyAccount {
            user_idx: u16,
        },
        /// Standalone account settlement (§10.2). Permissionless.
        SettleAccount {
            user_idx: u16,
        },
        /// Direct fee-debt repayment (§10.3.1). Owner only.
        DepositFeeCredits {
            user_idx: u16,
            amount: u64,
        },
        /// Voluntary PnL conversion with open position (§10.4.1). Owner only.
        ConvertReleasedPnl {
            user_idx: u16,
            amount: u64,
        },
        /// Permissionless market resolution after prolonged oracle staleness.
        /// Anyone can call when the oracle has been stale for at least
        /// config.permissionless_resolve_stale_slots. Settles at the last
        /// known good oracle price from engine.last_oracle_price.
        ResolvePermissionless,
        /// Permissionless force-close for resolved markets (tag 30).
        /// Requires RESOLVED + delay. Sends capital to stored owner ATA.
        ForceCloseResolved {
            user_idx: u16,
        },
    }

    impl Instruction {
        pub fn decode(input: &[u8]) -> Result<Self, ProgramError> {
            let (&tag, mut rest) = input
                .split_first()
                .ok_or(ProgramError::InvalidInstructionData)?;

            match tag {
                0 => {
                    // InitMarket
                    let admin = read_pubkey(&mut rest)?;
                    let collateral_mint = read_pubkey(&mut rest)?;
                    let index_feed_id = read_bytes32(&mut rest)?;
                    let max_staleness_secs = read_u64(&mut rest)?;
                    let conf_filter_bps = read_u16(&mut rest)?;
                    let invert = read_u8(&mut rest)?;
                    let unit_scale = read_u32(&mut rest)?;
                    let initial_mark_price_e6 = read_u64(&mut rest)?;
                    let maintenance_fee_per_slot = read_u128(&mut rest)?; // periodic fee per slot per account
                    let max_insurance_floor = read_u128(&mut rest)?;
                    let min_oracle_price_cap_e2bps = read_u64(&mut rest)?;
                    // Insurance withdrawal limits (immutable after init)
                    let (risk_params, insurance_floor) = read_risk_params(&mut rest)?;
                    // Extended fields: either ALL present (66 bytes) or NONE.
                    // No partial tails — prevents silent misparsing of truncated payloads.
                    // Total: insurance(2+8) + permissionless(8) + funding(8+8+8+8) +
                    //        mark_min_fee(8) + force_close_delay(8) = 66 bytes
                    const EXTENDED_TAIL_LEN: usize = 2 + 8 * 8;
                    let (
                        insurance_withdraw_max_bps,
                        insurance_withdraw_cooldown_slots,
                        permissionless_resolve_stale_slots,
                        funding_horizon_slots,
                        funding_k_bps,
                        funding_max_premium_bps,
                        funding_max_bps_per_slot,
                        mark_min_fee,
                        force_close_delay_slots,
                    ) = if rest.is_empty() {
                        // Minimal payload: all extended fields use defaults
                        (0u16, 0u64, 0u64, None, None, None, None, 0u64, 0u64)
                    } else if rest.len() >= EXTENDED_TAIL_LEN {
                        // Full extended payload
                        let iwm = read_u16(&mut rest)?;
                        let iwc = read_u64(&mut rest)?;
                        let prs = read_u64(&mut rest)?;
                        let fh = read_u64(&mut rest)?;
                        let fk = read_u64(&mut rest)?;
                        let fmp = read_i64(&mut rest)?;
                        let fms = read_i64(&mut rest)?;
                        let mmf = read_u64(&mut rest)?;
                        let fcd = read_u64(&mut rest)?;
                        (iwm, iwc, prs, Some(fh), Some(fk), Some(fmp), Some(fms), mmf, fcd)
                    } else {
                        // Partial tail: reject to prevent misparsing
                        return Err(ProgramError::InvalidInstructionData);
                    };
                    // Reject trailing bytes to prevent silent misparsing.
                    // All optional fields are parsed — leftover data means the
                    // client sent a malformed or future-version payload.
                    if !rest.is_empty() {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                    Ok(Instruction::InitMarket {
                        admin,
                        collateral_mint,
                        index_feed_id,
                        max_staleness_secs,
                        conf_filter_bps,
                        invert,
                        unit_scale,
                        initial_mark_price_e6,
                        maintenance_fee_per_slot,
                        max_insurance_floor,
                        min_oracle_price_cap_e2bps,
                        insurance_withdraw_max_bps,
                        insurance_withdraw_cooldown_slots,
                        risk_params,
                        insurance_floor,
                        permissionless_resolve_stale_slots,
                        funding_horizon_slots,
                        funding_k_bps,
                        funding_max_premium_bps,
                        funding_max_bps_per_slot,
                        mark_min_fee,
                        force_close_delay_slots,
                    })
                }
                1 => {
                    // InitUser
                    let fee_payment = read_u64(&mut rest)?;
                    Ok(Instruction::InitUser { fee_payment })
                }
                2 => {
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
                3 => {
                    // Deposit
                    let user_idx = read_u16(&mut rest)?;
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::DepositCollateral { user_idx, amount })
                }
                4 => {
                    // Withdraw
                    let user_idx = read_u16(&mut rest)?;
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::WithdrawCollateral { user_idx, amount })
                }
                5 => {
                    // KeeperCrank — two-phase: candidates computed off-chain
                    let caller_idx = read_u16(&mut rest)?;
                    let format_version = read_u8(&mut rest)?;
                    // format_version 1: u16 idx + u8 policy_tag per candidate
                    //   policy tag 0 = FullClose, 1 = ExactPartial(u128), 0xFF = touch-only
                    let mut candidates = alloc::vec::Vec::new();
                    if format_version == 1 {
                        // Extended: u16 idx + u8 policy tag per candidate
                        while rest.len() >= 3 {
                            let idx = read_u16(&mut rest)?;
                            let tag = read_u8(&mut rest)?;
                            let policy = match tag {
                                0 => Some(percolator::LiquidationPolicy::FullClose),
                                1 => {
                                    let q = read_u128(&mut rest)?;
                                    Some(percolator::LiquidationPolicy::ExactPartial(q))
                                }
                                0xFF => None,
                                _ => return Err(ProgramError::InvalidInstructionData),
                            };
                            candidates.push((idx, policy));
                        }
                    } else {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                    Ok(Instruction::KeeperCrank {
                        caller_idx,
                        candidates,
                    })
                }
                6 => {
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
                7 => {
                    // LiquidateAtOracle
                    let target_idx = read_u16(&mut rest)?;
                    Ok(Instruction::LiquidateAtOracle { target_idx })
                }
                8 => {
                    // CloseAccount
                    let user_idx = read_u16(&mut rest)?;
                    Ok(Instruction::CloseAccount { user_idx })
                }
                9 => {
                    // TopUpInsurance
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::TopUpInsurance { amount })
                }
                10 => {
                    // TradeCpi
                    let lp_idx = read_u16(&mut rest)?;
                    let user_idx = read_u16(&mut rest)?;
                    let size = read_i128(&mut rest)?;
                    let limit_price_e6 = read_u64(&mut rest)?;
                    Ok(Instruction::TradeCpi {
                        lp_idx,
                        user_idx,
                        size,
                        limit_price_e6,
                    })
                }
                12 => {
                    // UpdateAdmin
                    let new_admin = read_pubkey(&mut rest)?;
                    Ok(Instruction::UpdateAdmin { new_admin })
                }
                13 => {
                    // CloseSlab
                    Ok(Instruction::CloseSlab)
                }
                14 => {
                    // UpdateConfig — funding params only
                    let funding_horizon_slots = read_u64(&mut rest)?;
                    let funding_k_bps = read_u64(&mut rest)?;
                    let funding_max_premium_bps = read_i64(&mut rest)?;
                    let funding_max_bps_per_slot = read_i64(&mut rest)?;
                    Ok(Instruction::UpdateConfig {
                        funding_horizon_slots,
                        funding_k_bps,
                        funding_max_premium_bps,
                        funding_max_bps_per_slot,
                    })
                }
                16 => {
                    // SetOracleAuthority
                    let new_authority = read_pubkey(&mut rest)?;
                    Ok(Instruction::SetOracleAuthority { new_authority })
                }
                17 => {
                    // PushOraclePrice
                    let price_e6 = read_u64(&mut rest)?;
                    let timestamp = read_i64(&mut rest)?;
                    Ok(Instruction::PushOraclePrice {
                        price_e6,
                        timestamp,
                    })
                }
                18 => {
                    // SetOraclePriceCap
                    let max_change_e2bps = read_u64(&mut rest)?;
                    Ok(Instruction::SetOraclePriceCap { max_change_e2bps })
                }
                19 => Ok(Instruction::ResolveMarket),
                20 => Ok(Instruction::WithdrawInsurance),
                21 => {
                    let user_idx = read_u16(&mut rest)?;
                    Ok(Instruction::AdminForceCloseAccount { user_idx })
                }
                22 => {
                    let authority = read_pubkey(&mut rest)?;
                    let min_withdraw_base = read_u64(&mut rest)?;
                    let max_withdraw_bps = read_u16(&mut rest)?;
                    let cooldown_slots = read_u64(&mut rest)?;
                    Ok(Instruction::SetInsuranceWithdrawPolicy {
                        authority,
                        min_withdraw_base,
                        max_withdraw_bps,
                        cooldown_slots,
                    })
                }
                23 => {
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::WithdrawInsuranceLimited { amount })
                }
                24 => {
                    let lp_idx = read_u16(&mut rest)?;
                    Ok(Instruction::QueryLpFees { lp_idx })
                }
                25 => {
                    let user_idx = read_u16(&mut rest)?;
                    Ok(Instruction::ReclaimEmptyAccount { user_idx })
                }
                26 => {
                    // SettleAccount (§10.2)
                    let user_idx = read_u16(&mut rest)?;
                    Ok(Instruction::SettleAccount { user_idx })
                }
                27 => {
                    // DepositFeeCredits (§10.3.1)
                    let user_idx = read_u16(&mut rest)?;
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::DepositFeeCredits { user_idx, amount })
                }
                28 => {
                    // ConvertReleasedPnl (§10.4.1)
                    let user_idx = read_u16(&mut rest)?;
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::ConvertReleasedPnl { user_idx, amount })
                }
                29 => Ok(Instruction::ResolvePermissionless),
                30 => {
                    let user_idx = read_u16(&mut rest)?;
                    Ok(Instruction::ForceCloseResolved { user_idx })
                }
                _ => Err(ProgramError::InvalidInstructionData),
            }
        }
    }

    fn read_u8(input: &mut &[u8]) -> Result<u8, ProgramError> {
        let (&val, rest) = input
            .split_first()
            .ok_or(ProgramError::InvalidInstructionData)?;
        *input = rest;
        Ok(val)
    }

    fn read_u16(input: &mut &[u8]) -> Result<u16, ProgramError> {
        if input.len() < 2 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let (bytes, rest) = input.split_at(2);
        *input = rest;
        Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_u32(input: &mut &[u8]) -> Result<u32, ProgramError> {
        if input.len() < 4 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let (bytes, rest) = input.split_at(4);
        *input = rest;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_u64(input: &mut &[u8]) -> Result<u64, ProgramError> {
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

    fn read_pubkey(input: &mut &[u8]) -> Result<Pubkey, ProgramError> {
        if input.len() < 32 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let (bytes, rest) = input.split_at(32);
        *input = rest;
        Ok(Pubkey::new_from_array(bytes.try_into().unwrap()))
    }

    fn read_bytes32(input: &mut &[u8]) -> Result<[u8; 32], ProgramError> {
        if input.len() < 32 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let (bytes, rest) = input.split_at(32);
        *input = rest;
        Ok(bytes.try_into().unwrap())
    }

    fn read_risk_params(input: &mut &[u8]) -> Result<(RiskParams, u128), ProgramError> {
        let h_min = read_u64(input)?;
        let maintenance_margin_bps = read_u64(input)?;
        let initial_margin_bps = read_u64(input)?;
        let trading_fee_bps = read_u64(input)?;
        let max_accounts = read_u64(input)?;
        let new_account_fee = U128::new(read_u128(input)?);
        let insurance_floor = read_u128(input)?;
        let h_max = read_u64(input)?;
        let max_crank_staleness_slots = read_u64(input)?;
        let liquidation_fee_bps = read_u64(input)?;
        let liquidation_fee_cap = U128::new(read_u128(input)?);
        let resolve_price_deviation_bps = read_u64(input)?; // was _liquidation_buffer_bps
        let min_liquidation_abs = U128::new(read_u128(input)?);
        if input.len() < 48 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let min_initial_deposit = U128::new(read_u128(input)?);
        let min_nonzero_mm_req = read_u128(input)?;
        let min_nonzero_im_req = read_u128(input)?;
        let params = RiskParams {
            maintenance_margin_bps,
            initial_margin_bps,
            trading_fee_bps,
            max_accounts,
            new_account_fee,
            max_crank_staleness_slots,
            liquidation_fee_bps,
            liquidation_fee_cap,
            min_liquidation_abs,
            min_initial_deposit,
            min_nonzero_mm_req,
            min_nonzero_im_req,
            insurance_floor: U128::new(insurance_floor),
            h_min,
            h_max,
            resolve_price_deviation_bps,
        };
        Ok((params, insurance_floor))
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

    /// Derive vault authority from stored bump (saves ~1300 CU vs find_program_address)
    pub fn derive_vault_authority_with_bump(
        program_id: &Pubkey,
        slab_key: &Pubkey,
        bump: u8,
    ) -> Result<Pubkey, ProgramError> {
        Pubkey::create_program_address(
            &[b"vault", slab_key.as_ref(), &[bump]],
            program_id,
        ).map_err(|_| ProgramError::InvalidSeeds)
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
        pub _padding: [u8; 3],
        pub admin: [u8; 32],
        pub _reserved: [u8; 24], // [0..8]=nonce, [8..24]=unused
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
        /// Max premium in basis points (500 = 5%)
        pub funding_max_premium_bps: i64,
        /// Max funding rate per slot in basis points
        pub funding_max_bps_per_slot: i64,

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
        // Per-Market Admin Limits (set at InitMarket, immutable)
        // ========================================
        /// Maximum insurance floor admin can set. Must be > 0 at init.
        pub max_insurance_floor: u128,
        /// Minimum oracle price cap (e2bps) admin can set (floor for non-zero values).
        /// 0 = no floor (admin can set any value).
        pub min_oracle_price_cap_e2bps: u64,

        // ========================================
        // Insurance Withdrawal Limits (set at InitMarket, immutable)
        // ========================================
        /// Max bps of insurance fund withdrawable per withdrawal (1-10000).
        /// 0 = disabled (no live-market withdrawals allowed).
        pub insurance_withdraw_max_bps: u16,
        /// Padding for alignment.
        pub _iw_padding: [u8; 6],
        /// Minimum slots between insurance withdrawals.
        pub insurance_withdraw_cooldown_slots: u64,
        pub _iw_padding2: [u64; 2],
        pub last_hyperp_index_slot: u64,
        pub last_mark_push_slot: u128,
        /// Last slot when insurance was withdrawn (for live-market cooldown tracking).
        /// Uses a dedicated field to avoid overwriting oracle config fields.
        pub last_insurance_withdraw_slot: u64,
        /// Padding for alignment.
        pub _liw_padding: u64,

        // ========================================
        // Mark EWMA (trade-derived mark price for funding)
        // ========================================
        /// EWMA of execution prices (e6). Updated on every TradeCpi fill.
        pub mark_ewma_e6: u64,
        /// Slot when mark_ewma_e6 was last updated.
        pub mark_ewma_last_slot: u64,
        /// EWMA decay half-life in slots. 0 = last trade price directly.
        pub mark_ewma_halflife_slots: u64,
        /// Padding for u128 alignment.
        pub _ewma_padding: u64,

        // ========================================
        // Permissionless Resolution
        // ========================================
        /// Slots of oracle staleness required before anyone can resolve.
        /// 0 = disabled (admin-only resolution). Set at InitMarket, immutable.
        pub permissionless_resolve_stale_slots: u64,
        /// Slot of last successful external oracle read (non-Hyperp only).
        /// Used by ResolvePermissionless to measure oracle-death duration.
        /// Stamped by read_price_clamped wrapper on every successful read.
        pub last_good_oracle_slot: u64,

        // ========================================
        // Fee-Weighted EWMA
        // ========================================
        /// Periodic maintenance fee per slot per account (engine units).
        /// Wrapper charges via engine.charge_account_fee_not_atomic during KeeperCrank.
        /// 0 = disabled. Set at InitMarket, immutable.
        pub maintenance_fee_per_slot: u128,
        /// Last slot when maintenance fees were globally charged.
        pub last_fee_charge_slot: u64,
        pub _fee_padding: u64,
        /// Minimum fee (in engine units, same as insurance_fund.balance) for full mark EWMA weight.
        /// Trades with fee below this get proportionally reduced alpha.
        /// 0 = disabled (all trades get full weight, backward compat).
        /// Set at InitMarket, immutable.
        pub mark_min_fee: u64,
        /// Minimum slots after resolution before permissionless force-close.
        /// 0 = disabled. Set at InitMarket, immutable.
        pub force_close_delay_slots: u64,
    }

    pub fn slab_data_mut<'a, 'b>(
        ai: &'b AccountInfo<'a>,
    ) -> Result<RefMut<'b, &'a mut [u8]>, ProgramError> {
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

    // ========================================
    // Market Flags (stored in _padding[0] at offset 13)
    // ========================================

    /// Offset of flags byte in SlabHeader (_padding[0])
    pub const FLAGS_OFF: usize = 13;

    /// Flag bit: SetInsuranceWithdrawPolicy has been explicitly called.
    /// Prevents WithdrawInsuranceLimited from misinterpreting oracle
    /// timestamps as policy metadata via authority_timestamp bit patterns.
    pub const FLAG_POLICY_CONFIGURED: u8 = 1 << 1;
    /// Flag bit: CPI is in progress (reentrancy guard for TradeCpi).
    /// Set before matcher CPI, cleared after. Any reentrant instruction
    /// that sees this flag must abort.
    pub const FLAG_CPI_IN_PROGRESS: u8 = 1 << 2;

    /// Read market flags from _padding[0].
    pub fn read_flags(data: &[u8]) -> u8 {
        data[FLAGS_OFF]
    }

    /// Write market flags to _padding[0].
    pub fn write_flags(data: &mut [u8], flags: u8) {
        data[FLAGS_OFF] = flags;
    }

    /// Check if CPI is in progress (reentrancy guard).
    pub fn is_cpi_in_progress(data: &[u8]) -> bool {
        read_flags(data) & FLAG_CPI_IN_PROGRESS != 0
    }

    /// Set CPI-in-progress flag (call before matcher CPI).
    pub fn set_cpi_in_progress(data: &mut [u8]) {
        let flags = read_flags(data) | FLAG_CPI_IN_PROGRESS;
        write_flags(data, flags);
    }

    /// Clear CPI-in-progress flag (call after matcher CPI returns).
    pub fn clear_cpi_in_progress(data: &mut [u8]) {
        let flags = read_flags(data) & !FLAG_CPI_IN_PROGRESS;
        write_flags(data, flags);
    }

    /// Check if insurance withdraw policy was explicitly configured.
    pub fn is_policy_configured(data: &[u8]) -> bool {
        read_flags(data) & FLAG_POLICY_CONFIGURED != 0
    }

    /// Set the policy-configured flag.
    pub fn set_policy_configured(data: &mut [u8]) {
        let flags = read_flags(data) | FLAG_POLICY_CONFIGURED;
        write_flags(data, flags);
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

    pub fn read_risk_buffer(data: &[u8]) -> crate::risk_buffer::RiskBuffer {
        use crate::constants::RISK_BUF_OFF;
        use crate::constants::RISK_BUF_LEN;
        let mut buf = crate::risk_buffer::RiskBuffer::zeroed();
        let src = &data[RISK_BUF_OFF..RISK_BUF_OFF + RISK_BUF_LEN];
        bytemuck::bytes_of_mut(&mut buf).copy_from_slice(src);
        buf
    }

    pub fn write_risk_buffer(data: &mut [u8], buf: &crate::risk_buffer::RiskBuffer) {
        use crate::constants::RISK_BUF_OFF;
        use crate::constants::RISK_BUF_LEN;
        let src = bytemuck::bytes_of(buf);
        data[RISK_BUF_OFF..RISK_BUF_OFF + RISK_BUF_LEN].copy_from_slice(src);
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
    use crate::error::PercolatorError;
    use solana_program::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};

    /// Pyth Solana Receiver program ID
    /// rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ
    pub const PYTH_RECEIVER_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
        0x0c, 0xb7, 0xfa, 0xbb, 0x52, 0xf7, 0xa6, 0x48, 0xbb, 0x5b, 0x31, 0x7d, 0x9a, 0x01, 0x8b,
        0x90, 0x57, 0xcb, 0x02, 0x47, 0x74, 0xfa, 0xfe, 0x01, 0xe6, 0xc4, 0xdf, 0x98, 0xcc, 0x38,
        0x58, 0x81,
    ]);

    /// Chainlink OCR2 Store program ID
    /// HEvSKofvBgfaexv23kMabbYqxasxU3mQ4ibBMEmJWHny
    pub const CHAINLINK_OCR2_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
        0xf1, 0x4b, 0xf6, 0x5a, 0xd5, 0x6b, 0xd2, 0xba, 0x71, 0x5e, 0x45, 0x74, 0x2c, 0x23, 0x1f,
        0x27, 0xd6, 0x36, 0x21, 0xcf, 0x5b, 0x77, 0x8f, 0x37, 0xc1, 0xa2, 0x48, 0x95, 0x1d, 0x17,
        0x56, 0x02,
    ]);

    // PriceUpdateV2 account layout offsets (134 bytes minimum)
    // See: https://github.com/pyth-network/pyth-crosschain/blob/main/target_chains/solana/pyth_solana_receiver_sdk/src/price_update.rs
    // Layout: discriminator(8) + write_authority(32) + verification_level(2) + feed_id(32) + ...
    const PRICE_UPDATE_V2_MIN_LEN: usize = 134;
    const OFF_VERIFICATION_LEVEL: usize = 40; // u16 enum: 0=Partial, 1=Full
    const OFF_FEED_ID: usize = 42; // 32 bytes
    const OFF_PRICE: usize = 74; // i64
    const OFF_CONF: usize = 82; // u64
    const OFF_EXPO: usize = 90; // i32
    const OFF_PUBLISH_TIME: usize = 94; // i64
    /// Pyth VerificationLevel::Full (the only safe level for production)
    const PYTH_VERIFICATION_FULL: u16 = 1;

    // Chainlink OCR2 State/Aggregator account layout offsets
    // Note: Different from the Transmissions ring buffer format in older docs
    const CL_MIN_LEN: usize = 224; // Minimum required length
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

        // Reject partially verified Pyth updates (only Full is safe)
        #[cfg(not(feature = "test"))]
        {
            let vl = u16::from_le_bytes(
                data[OFF_VERIFICATION_LEVEL..OFF_VERIFICATION_LEVEL + 2]
                    .try_into()
                    .unwrap(),
            );
            if vl != PYTH_VERIFICATION_FULL {
                return Err(PercolatorError::OracleInvalid.into());
            }
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
        let publish_time = i64::from_le_bytes(
            data[OFF_PUBLISH_TIME..OFF_PUBLISH_TIME + 8]
                .try_into()
                .unwrap(),
        );

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

        Ok(final_price_u128 as u64)
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
        price_ai: &AccountInfo,
        expected_feed_id: &[u8; 32],
        now_unix_ts: i64,
        max_staleness_secs: u64,
        conf_bps: u16,
        invert: u8,
        unit_scale: u32,
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
            // Chainlink safety: the feed pubkey check (line 2072) ensures only the
            // specific account stored in index_feed_id at InitMarket can be read.
            // A different Chainlink-owned account would fail the pubkey match.
            read_chainlink_price_e6(price_ai, expected_feed_id, now_unix_ts, max_staleness_secs)?
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
        let engine_price = crate::verify::scale_price_e6(price_after_invert, unit_scale)
            .ok_or(PercolatorError::OracleInvalid)?;

        // Enforce MAX_ORACLE_PRICE at ingress
        if engine_price > percolator::MAX_ORACLE_PRICE {
            return Err(PercolatorError::OracleInvalid.into());
        }
        Ok(engine_price)
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

    /// Clamp `raw_price` so it cannot move more than `max_change_e2bps` from `last_price`.
    /// Units: 1_000_000 e2bps = 100%. 0 = disabled (no cap). last_price == 0 = first-time.
    pub fn clamp_oracle_price(last_price: u64, raw_price: u64, max_change_e2bps: u64) -> u64 {
        if max_change_e2bps == 0 || last_price == 0 {
            return raw_price;
        }
        let max_delta_128 = (last_price as u128) * (max_change_e2bps as u128) / 1_000_000;
        let max_delta = core::cmp::min(max_delta_128, u64::MAX as u128) as u64;
        let lower = last_price.saturating_sub(max_delta);
        let upper = last_price.saturating_add(max_delta);
        raw_price.clamp(lower, upper)
    }

    /// Read oracle price with circuit-breaker clamping.
    ///
    /// The baseline (`last_effective_price_e6`) is ONLY updated from external
    /// oracle reads (Pyth/Chainlink). Authority-pushed prices are used as the
    /// returned effective price but do NOT contaminate the baseline. This
    /// prevents the admin from ratcheting the baseline via push+crank interleaving.
    ///
    /// When the circuit breaker is configured (min_oracle_price_cap_e2bps > 0),
    /// the external oracle read MUST succeed whenever authority pricing is used.
    /// This prevents callers from bypassing the fresh external anchor by
    /// supplying a bad/stale oracle account.
    pub fn read_price_clamped(
        config: &mut super::state::MarketConfig,
        price_ai: &AccountInfo,
        now_unix_ts: i64,
    ) -> Result<u64, ProgramError> {
        // Always try to read external oracle to update baseline
        let external = read_engine_price_e6(
            price_ai,
            &config.index_feed_id,
            now_unix_ts,
            config.max_staleness_secs,
            config.conf_filter_bps,
            config.invert,
            config.unit_scale,
        );

        // Update baseline from external oracle only (never from authority)
        if let Ok(ext_price) = external {
            let clamped_ext = clamp_oracle_price(
                config.last_effective_price_e6,
                ext_price,
                config.oracle_price_cap_e2bps,
            );
            config.last_effective_price_e6 = clamped_ext;
        }

        // Return the authority price if fresh, otherwise the external price
        if let Some(auth_price) = read_authority_price(config, now_unix_ts, config.max_staleness_secs) {
            // When the live circuit breaker is active, require the external
            // oracle to have succeeded. Uses the active cap (not the immutable
            // floor) so zero-floor markets with a live breaker are also protected.
            if config.oracle_price_cap_e2bps != 0 && external.is_err() {
                return external; // propagate the external oracle error
            }
            // Authority price is clamped against the (now-updated) external baseline
            let clamped_auth = clamp_oracle_price(
                config.last_effective_price_e6,
                auth_price,
                config.oracle_price_cap_e2bps,
            );
            return Ok(clamped_auth);
        }

        // No authority: use external price (already clamped above)
        match external {
            Ok(_) => Ok(config.last_effective_price_e6),
            Err(e) => Err(e),
        }
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

    /// Move `index` toward `mark`, but clamp movement by cap_e2bps * dt_slots.
    /// cap_e2bps units: 1_000_000 = 100.00%
    /// Returns the new index value.
    ///
    /// Security: When dt_slots == 0 (same slot) or cap_e2bps == 0 (cap disabled),
    /// returns index unchanged to prevent bypassing rate limits.
    /// Maximum effective dt for rate-limiting. Caps accumulated movement to
    /// prevent a crank pause from allowing a full-magnitude index jump.
    /// ~1 hour at 2.5 slots/sec = 9000 slots.
    const MAX_CLAMP_DT_SLOTS: u64 = 9_000;

    pub fn clamp_toward_with_dt(index: u64, mark: u64, cap_e2bps: u64, dt_slots: u64) -> u64 {
        if index == 0 {
            return mark;
        }
        if cap_e2bps == 0 || dt_slots == 0 {
            return index;
        }

        // Cap dt to bound accumulated movement after crank pauses
        let capped_dt = dt_slots.min(MAX_CLAMP_DT_SLOTS);

        let max_delta_u128 = (index as u128)
            .saturating_mul(cap_e2bps as u128)
            .saturating_mul(capped_dt as u128)
            / 1_000_000u128;

        let max_delta = core::cmp::min(max_delta_u128, u64::MAX as u128) as u64;
        let lo = index.saturating_sub(max_delta);
        let hi = index.saturating_add(max_delta);
        mark.clamp(lo, hi)
    }

    /// Get engine oracle price (unified: external oracle vs Hyperp mode).
    /// In Hyperp mode: updates index toward mark with rate limiting.
    ///   Mark staleness enforced via last_mark_push_slot.
    /// In external mode: reads from Pyth/Chainlink/authority with circuit breaker.
    pub fn get_engine_oracle_price_e6(
        _engine_last_slot: u64,
        now_slot: u64,
        now_unix_ts: i64,
        config: &mut super::state::MarketConfig,
        a_oracle: &AccountInfo,
    ) -> Result<u64, ProgramError> {
        // Hyperp mode: index_feed_id == 0
        if is_hyperp_mode(config) {
            // Mark source: prefer trade-derived EWMA, fall back to authority push
            let mark = if config.mark_ewma_e6 > 0 {
                config.mark_ewma_e6
            } else {
                config.authority_price_e6
            };
            if mark == 0 {
                return Err(super::error::PercolatorError::OracleInvalid.into());
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
                    return Err(super::error::PercolatorError::OracleStale.into());
                }
            }

            let prev_index = config.last_effective_price_e6;
            // Use dedicated last_hyperp_index_slot, not engine.current_slot.
            // This tracks exactly when the index was last updated, preventing
            // both under-counting dt (unrelated user activity) and over-counting
            // dt (admin flush without engine.current_slot advance).
            let last_idx_slot = config.last_hyperp_index_slot;
            let dt = now_slot.saturating_sub(last_idx_slot);
            let new_index =
                clamp_toward_with_dt(prev_index.max(1), mark, config.oracle_price_cap_e2bps, dt);

            config.last_effective_price_e6 = new_index;
            config.last_hyperp_index_slot = now_slot;
            return Ok(new_index);
        }

        // Non-Hyperp: existing behavior (authority -> Pyth/Chainlink) + circuit breaker
        read_price_clamped(config, a_oracle, now_unix_ts)
    }

    /// Compute premium-based funding rate (Hyperp funding model).
    /// Premium = (mark - index) / index, converted to bps per slot.
    /// Returns signed bps per slot (positive = longs pay shorts).
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

        // Convert to per-slot by dividing by horizon, clamp in i128 before
        // casting to i64 to avoid truncation on huge admin-configured inputs.
        let per_slot_128 = scaled / (funding_horizon_slots as i128);
        let clamped_128 = per_slot_128.clamp(
            -(max_bps_per_slot as i128),
            max_bps_per_slot as i128,
        );
        // Safe: clamped value is within i64 range (max_bps_per_slot is i64)
        clamped_128 as i64
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

// 9. mod processor
pub mod processor {
    use crate::{
        accounts, collateral,
        constants::{
            CONFIG_LEN, DEFAULT_FUNDING_HORIZON_SLOTS,
            DEFAULT_FUNDING_K_BPS, DEFAULT_FUNDING_MAX_BPS_PER_SLOT,
            DEFAULT_FUNDING_MAX_PREMIUM_BPS, DEFAULT_HYPERP_PRICE_CAP_E2BPS, MAX_ORACLE_PRICE_CAP_E2BPS,
            DEFAULT_INSURANCE_WITHDRAW_COOLDOWN_SLOTS, DEFAULT_INSURANCE_WITHDRAW_MAX_BPS,
            DEFAULT_INSURANCE_WITHDRAW_MIN_BASE, DEFAULT_MARK_EWMA_HALFLIFE_SLOTS,
            MAGIC, MATCHER_CALL_LEN, MATCHER_CALL_TAG,
            SLAB_LEN,
        },
        error::{map_risk_error, PercolatorError},
        ix::Instruction,
        oracle,
        pack_ins_withdraw_meta,
        state::{self, MarketConfig, SlabHeader},
        unpack_ins_withdraw_meta,
        zc,
    };
    use percolator::{
        RiskEngine, RiskError, U128, ADL_ONE, MAX_ACCOUNTS,
    };

    // settle_and_close_resolved removed — replaced by engine.force_close_resolved_not_atomic()
    // which handles K-pair PnL, checked arithmetic, and all settlement internally.

    /// Read oracle price for non-Hyperp markets and stamp last_good_oracle_slot
    /// ONLY when the external oracle read succeeds. Authority-fallback success
    /// does NOT stamp the field — it measures external-oracle liveness only.
    ///
    /// Probes external oracle separately to detect liveness. This doubles the
    /// oracle parse (~2K CU) but is necessary because read_price_clamped can
    /// succeed via authority fallback without a live external oracle, and
    /// change-detection on last_effective_price_e6 misses same-price reads.
    fn read_price_and_stamp(
        config: &mut state::MarketConfig,
        a_oracle: &AccountInfo,
        clock_unix_ts: i64,
        clock_slot: u64,
    ) -> Result<u64, ProgramError> {
        let external_ok = oracle::read_engine_price_e6(
            a_oracle,
            &config.index_feed_id,
            clock_unix_ts,
            config.max_staleness_secs,
            config.conf_filter_bps,
            config.invert,
            config.unit_scale,
        ).is_ok();

        let price = oracle::read_price_clamped(config, a_oracle, clock_unix_ts)?;

        if external_ok {
            config.last_good_oracle_slot = clock_slot;
        }
        Ok(price)
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct TradeExecution {
        /// Actual execution price (may differ from oracle/requested price)
        pub price: u64,
        /// Actual executed size (may be partial fill)
        pub size: i128,
    }

    /// Trait for pluggable matching engines
    pub trait MatchingEngine {
        fn execute_match(
            &self,
            lp_program: &[u8; 32],
            lp_context: &[u8; 32],
            lp_account_id: u64,
            oracle_price: u64,
            size: i128,
        ) -> Result<TradeExecution, RiskError>;
    }

    /// No-op matching engine (for testing/TradeNoCpi)
    pub struct NoOpMatcher;

    impl MatchingEngine for NoOpMatcher {
        fn execute_match(
            &self,
            _lp_program: &[u8; 32],
            _lp_context: &[u8; 32],
            _lp_account_id: u64,
            oracle_price: u64,
            size: i128,
        ) -> Result<TradeExecution, RiskError> {
            Ok(TradeExecution {
                price: oracle_price,
                size,
            })
        }
    }

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

    /// Compute funding rate from mark-index premium (all market types).
    /// Uses trade-derived EWMA mark vs oracle index.
    /// Returns 0 if no trades yet (mark_ewma == 0) or params unset.
    /// Compute funding rate in e9-per-slot (ppb) directly.
    /// Avoids bps quantization: sub-bps rates are preserved as nonzero ppb values.
    fn compute_current_funding_rate_e9(config: &MarketConfig) -> i128 {
        let mark = config.mark_ewma_e6;
        let index = config.last_effective_price_e6;
        if mark == 0 || index == 0 || config.funding_horizon_slots == 0 {
            return 0;
        }

        let diff = mark as i128 - index as i128;
        // premium in e9: diff * 1_000_000_000 / index
        let mut premium_e9 = diff.saturating_mul(1_000_000_000) / (index as i128);

        // Clamp premium: max_premium_bps * 100_000 converts bps to e9
        let max_prem_e9 = (config.funding_max_premium_bps as i128) * 100_000;
        premium_e9 = premium_e9.clamp(-max_prem_e9, max_prem_e9);

        // Apply k multiplier (100 = 1.00x)
        let scaled = premium_e9.saturating_mul(config.funding_k_bps as i128) / 100;

        // Per-slot: divide by horizon
        let per_slot = scaled / (config.funding_horizon_slots as i128);

        // Clamp: max_bps_per_slot * 100_000 converts bps to e9
        let max_rate_e9 = (config.funding_max_bps_per_slot as i128) * 100_000;
        per_slot.clamp(-max_rate_e9, max_rate_e9)
    }

    /// Convert bps to e9 for validation only (admin-configured limits).
    fn funding_bps_to_e9(bps: i64) -> i128 {
        (bps as i128) * 100_000
    }

    fn execute_trade_with_matcher<M: MatchingEngine>(
        engine: &mut RiskEngine,
        matcher: &M,
        lp_idx: u16,
        user_idx: u16,
        now_slot: u64,
        oracle_price: u64,
        size: i128,
        funding_rate_e9: i128,
    ) -> Result<(), RiskError> {
        let lp = &engine.accounts[lp_idx as usize];
        let exec = matcher.execute_match(
            &lp.matcher_program,
            &lp.matcher_context,
            lp_idx as u64,
            oracle_price,
            size,
        )?;
        // POS_SCALE = 1_000_000 in spec v11.5, same as instruction units.
        // No conversion needed.
        let size_q: i128 = exec.size;
        // Spec v12: size_q must be > 0. Account `a` buys from `b`.
        // Positive size = user buys from LP (user goes long).
        // Negative size = LP buys from user (user goes short) — swap order.
        let (a, b, abs_size) = if size_q > 0 {
            (user_idx, lp_idx, size_q)
        } else if size_q < 0 {
            // checked_neg rejects i128::MIN (which has no positive counterpart)
            let pos = size_q.checked_neg().ok_or(RiskError::Overflow)?;
            (lp_idx, user_idx, pos)
        } else {
            return Err(RiskError::Overflow);
        };
        let h_lock = engine.params.h_min;
        engine.execute_trade_not_atomic(
            a,
            b,
            oracle_price,
            now_slot,
            abs_size,
            exec.price,
            funding_rate_e9,
            h_lock,
        )
    }

    use solana_program::instruction::{AccountMeta, Instruction as SolInstruction};
    use solana_program::{
        account_info::AccountInfo,
        entrypoint::ProgramResult,
        log::{sol_log_64, sol_log_compute_units},
        msg,
        program_error::ProgramError,
        program_pack::Pack,
        pubkey::Pubkey,
        sysvar::{clock::Clock, Sysvar},
    };

    fn slab_guard(
        program_id: &Pubkey,
        slab: &AccountInfo,
        data: &[u8],
    ) -> Result<(), ProgramError> {
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
        if h.magic != MAGIC {
            return Err(PercolatorError::NotInitialized.into());
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

    fn check_idx(engine: &RiskEngine, idx: u16) -> Result<(), ProgramError> {
        if (idx as usize) >= MAX_ACCOUNTS || !engine.is_used(idx as usize) {
            return Err(PercolatorError::EngineAccountNotFound.into());
        }
        Ok(())
    }

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
        // Reject vault with pre-set delegate or close_authority — these allow
        // a third party to drain or close the vault outside program control.
        if tok.delegate.is_some() || tok.close_authority.is_some() {
            return Err(PercolatorError::InvalidVaultAta.into());
        }
        Ok(())
    }

    /// verify_vault + require zero balance (for InitMarket).
    /// Reuses the unpack from verify_vault logic (single unpack).
    fn verify_vault_empty(
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
        if tok.state != spl_token::state::AccountState::Initialized {
            return Err(PercolatorError::InvalidVaultAta.into());
        }
        if tok.delegate.is_some() || tok.close_authority.is_some() {
            return Err(PercolatorError::InvalidVaultAta.into());
        }
        if tok.amount != 0 {
            return Err(ProgramError::InvalidAccountData);
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

    pub fn process_instruction<'a, 'b>(
        program_id: &Pubkey,
        accounts: &'b [AccountInfo<'a>],
        instruction_data: &[u8],
    ) -> ProgramResult {
        // Durable nonce rejection removed — the check was opt-in (only ran if
        // caller voluntarily passed the Instructions sysvar) and therefore not
        // actually enforceable. Timing-sensitive operations should rely on
        // slot/timestamp freshness checks instead.

        let instruction = Instruction::decode(instruction_data)?;

        match instruction {
            Instruction::InitMarket {
                admin,
                collateral_mint,
                index_feed_id,
                max_staleness_secs,
                conf_filter_bps,
                invert,
                unit_scale,
                initial_mark_price_e6,
                maintenance_fee_per_slot,
                max_insurance_floor,
                min_oracle_price_cap_e2bps,
                insurance_withdraw_max_bps,
                insurance_withdraw_cooldown_slots,
                risk_params,
                insurance_floor,
                permissionless_resolve_stale_slots,
                funding_horizon_slots: custom_funding_horizon,
                funding_k_bps: custom_funding_k,
                funding_max_premium_bps: custom_max_premium,
                funding_max_bps_per_slot: custom_max_per_slot,
                mark_min_fee,
                force_close_delay_slots,
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
                    use solana_program::program_pack::Pack;
                    use spl_token::state::Mint;
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

                // invert must be 0 or 1 (boolean stored as u8)
                if invert > 1 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                // conf_filter_bps: 0..=10_000 (0 = disabled, 10_000 = 100%)
                if conf_filter_bps > 10_000 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                // Validate unit_scale: reject huge values that make most deposits credit 0 units
                if !crate::verify::init_market_scale_ok(unit_scale) {
                    return Err(ProgramError::InvalidInstructionData);
                }
                // Margin params: initial >= maintenance, both non-zero, initial <= 100%
                if risk_params.initial_margin_bps == 0
                    || risk_params.maintenance_margin_bps == 0
                {
                    return Err(ProgramError::InvalidInstructionData);
                }
                if risk_params.initial_margin_bps > 10_000 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                if risk_params.initial_margin_bps < risk_params.maintenance_margin_bps {
                    return Err(ProgramError::InvalidInstructionData);
                }
                // insurance_withdraw_max_bps is a percentage (0..=10_000)
                if insurance_withdraw_max_bps > 10_000 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                // If live withdrawals are enabled, require an explicit cooldown
                // (0 would fall through to DEFAULT which may surprise the admin).
                if insurance_withdraw_max_bps > 0 && insurance_withdraw_cooldown_slots == 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }

                // max_staleness_secs: reject 0 (would brick oracle reads —
                // any non-zero age > 0 fails the staleness check).
                if max_staleness_secs == 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }

                // Hyperp mode validation: if index_feed_id is all zeros, require initial_mark_price_e6
                let is_hyperp = index_feed_id == [0u8; 32];
                if is_hyperp && initial_mark_price_e6 == 0 {
                    // Hyperp mode requires a non-zero initial mark price
                    return Err(ProgramError::InvalidInstructionData);
                }

                // Normalize initial mark price to engine-space (invert + scale).
                // All Hyperp internal prices must be in engine-space.
                let initial_mark_price_e6 = if is_hyperp {
                    let p = crate::verify::to_engine_price(initial_mark_price_e6, invert, unit_scale)
                        .ok_or(PercolatorError::OracleInvalid)?;
                    // Enforce MAX_ORACLE_PRICE at genesis — same invariant as runtime ingress
                    if p > percolator::MAX_ORACLE_PRICE {
                        return Err(PercolatorError::OracleInvalid.into());
                    }
                    p
                } else {
                    initial_mark_price_e6
                };

                // Validate per-market admin limits (must be set at init time).
                // Bounds-check against engine-level constants to prevent admin
                // from setting values that violate engine invariants.
                if max_insurance_floor == 0
                    || max_insurance_floor > percolator::MAX_VAULT_TVL
                {
                    return Err(ProgramError::InvalidInstructionData);
                }
                // Validate initial insurance_floor against per-market limit
                if insurance_floor > max_insurance_floor {
                    return Err(ProgramError::InvalidInstructionData);
                }
                // Oracle cap floor: hard-bounded to MAX (100%)
                if min_oracle_price_cap_e2bps > MAX_ORACLE_PRICE_CAP_E2BPS {
                    return Err(ProgramError::InvalidInstructionData);
                }
                // Permissionless resolve: if enabled, must exceed max_crank_staleness
                // to prevent accidental instant-resolution from one missed crank.
                if permissionless_resolve_stale_slots > 0
                    && permissionless_resolve_stale_slots <= risk_params.max_crank_staleness_slots
                {
                    return Err(ProgramError::InvalidInstructionData);
                }
                // Liveness: if permissionless resolution is enabled, force_close must
                // also be enabled. Otherwise abandoned accounts on resolved markets
                // with burned admin have no cleanup path.
                if permissionless_resolve_stale_slots > 0 && force_close_delay_slots == 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                // §14.1: H_max must not exceed permissionless resolve delay.
                // Otherwise warmup cohorts could mature after the market is already
                // permissionlessly resolved, creating inconsistent terminal state.
                if permissionless_resolve_stale_slots > 0
                    && risk_params.h_max > permissionless_resolve_stale_slots
                {
                    return Err(ProgramError::InvalidInstructionData);
                }

                // Validate custom funding parameters (same checks as UpdateConfig).
                // These are immutable after init for governance-free deployments.
                if let Some(h) = custom_funding_horizon {
                    if h == 0 {
                        return Err(PercolatorError::InvalidConfigParam.into());
                    }
                }
                if let Some(k) = custom_funding_k {
                    if k > 100_000 {
                        return Err(PercolatorError::InvalidConfigParam.into());
                    }
                }
                if let Some(mp) = custom_max_premium {
                    if mp < 0 {
                        return Err(PercolatorError::InvalidConfigParam.into());
                    }
                }
                if let Some(ms) = custom_max_per_slot {
                    if ms < 0 || funding_bps_to_e9(ms) > percolator::MAX_ABS_FUNDING_E9_PER_SLOT {
                        return Err(PercolatorError::InvalidConfigParam.into());
                    }
                }
                // mark_min_fee upper bound: prevent setting so high that EWMA never updates
                if mark_min_fee > percolator::MAX_PROTOCOL_FEE_ABS as u64 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }

                #[cfg(debug_assertions)]
                {
                    if core::mem::size_of::<MarketConfig>() != CONFIG_LEN {
                        return Err(ProgramError::InvalidAccountData);
                    }
                }

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;

                // Check magic BEFORE any unsafe cast — raw bytes may contain
                // invalid enum discriminants that would be UB if cast to RiskEngine.
                let header = state::read_header(&data);
                if header.magic == MAGIC {
                    return Err(PercolatorError::AlreadyInitialized.into());
                }

                let (auth, bump) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault_empty(a_vault, &auth, a_mint.key, a_vault.key)?;

                for b in data.iter_mut() {
                    *b = 0;
                }

                // Initialize engine in-place (zero-copy) to avoid stack overflow.
                let a_clock = &accounts[5];
                let a_oracle = &accounts[7];
                let clock = Clock::from_account_info(a_clock)?;
                // Engine v12 requires init_oracle_price > 0.
                // Hyperp: use the normalized initial mark price.
                // Non-Hyperp: use 1 as sentinel — the real oracle price is
                // established on the first KeeperCrank via accrue_market_to.
                // This is safe because no trades can happen before a crank
                // (require_fresh_crank gate in engine), and the first
                // accrue_market_to overwrites last_oracle_price.
                let init_price = if is_hyperp { initial_mark_price_e6 } else { 1 };

                // Prevalidate all engine RiskParams invariants to return
                // ProgramError instead of panicking inside engine.init_in_place().
                {
                    let p = &risk_params;
                    if (p.max_accounts as usize) > percolator::MAX_ACCOUNTS || p.max_accounts == 0 {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                    if p.maintenance_margin_bps > p.initial_margin_bps
                        || p.initial_margin_bps > 10_000
                    {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                    if p.trading_fee_bps > 10_000 || p.liquidation_fee_bps > 10_000 {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                    if p.min_nonzero_mm_req == 0
                        || p.min_nonzero_mm_req >= p.min_nonzero_im_req
                        || p.min_nonzero_im_req > p.min_initial_deposit.get()
                    {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                    if p.min_initial_deposit.get() == 0
                        || p.min_initial_deposit.get() > percolator::MAX_VAULT_TVL
                    {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                    if p.min_liquidation_abs.get() > p.liquidation_fee_cap.get()
                        || p.liquidation_fee_cap.get() > percolator::MAX_PROTOCOL_FEE_ABS
                    {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                    if p.insurance_floor.get() > percolator::MAX_VAULT_TVL {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                    // new_account_fee must be payable: 0 <= fee <= MAX_VAULT_TVL
                    if p.new_account_fee.get() > percolator::MAX_VAULT_TVL {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                    // Warmup horizon: h_min <= h_max (engine asserts, but we pre-validate)
                    if p.h_min > p.h_max {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                    // Settlement deviation band: 0 < bps <= MAX (engine asserts)
                    if p.resolve_price_deviation_bps == 0
                        || p.resolve_price_deviation_bps > percolator::MAX_RESOLVE_PRICE_DEVIATION_BPS
                    {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                }

                let engine = zc::engine_mut(&mut data)?;
                engine.init_in_place(risk_params, clock.slot, init_price);
                // init_in_place sets last_crank_slot = 0; override to init slot
                // so first crank doesn't see a huge staleness gap.
                engine.last_crank_slot = clock.slot;

                let config = MarketConfig {
                    collateral_mint: a_mint.key.to_bytes(),
                    vault_pubkey: a_vault.key.to_bytes(),
                    index_feed_id,
                    max_staleness_secs,
                    conf_filter_bps,
                    vault_authority_bump: bump,
                    invert,
                    unit_scale,
                    // Funding parameters (custom overrides or defaults)
                    funding_horizon_slots: custom_funding_horizon.unwrap_or(DEFAULT_FUNDING_HORIZON_SLOTS),
                    funding_k_bps: custom_funding_k.unwrap_or(DEFAULT_FUNDING_K_BPS),
                    funding_max_premium_bps: custom_max_premium.unwrap_or(DEFAULT_FUNDING_MAX_PREMIUM_BPS),
                    funding_max_bps_per_slot: custom_max_per_slot.unwrap_or(DEFAULT_FUNDING_MAX_BPS_PER_SLOT),
                    // Oracle authority (disabled by default - use Pyth/Chainlink)
                    // In Hyperp mode: authority_price_e6 = mark, last_effective_price_e6 = index
                    oracle_authority: [0u8; 32],
                    authority_price_e6: if is_hyperp { initial_mark_price_e6 } else { 0 },
                    authority_timestamp: 0, // In Hyperp mode: stores funding rate (bps per slot)
                    // Oracle price circuit breaker
                    // In Hyperp mode: used for rate-limited index smoothing AND mark price clamping
                    // Default: disabled for non-Hyperp, 1% per slot for Hyperp
                    oracle_price_cap_e2bps: if is_hyperp {
                        DEFAULT_HYPERP_PRICE_CAP_E2BPS.max(min_oracle_price_cap_e2bps)
                    } else {
                        // Non-Hyperp: start at the immutable floor so the circuit
                        // breaker is active from genesis. 0 floor = no breaker.
                        min_oracle_price_cap_e2bps
                    },
                    last_effective_price_e6: if is_hyperp { initial_mark_price_e6 } else { 0 },
                    // Per-market admin limits (immutable after init)
                    max_insurance_floor,
                    min_oracle_price_cap_e2bps,
                    // Insurance withdrawal limits (immutable after init)
                    insurance_withdraw_max_bps,
                    _iw_padding: [0u8; 6],
                    insurance_withdraw_cooldown_slots,
                    _iw_padding2: [0; 2],
                    last_hyperp_index_slot: if is_hyperp { clock.slot } else { 0 },
                    // Hyperp: stamp init slot so stale check works from genesis.
                    // Non-Hyperp: 0 (no mark push concept).
                    last_mark_push_slot: if is_hyperp { clock.slot as u128 } else { 0 },
                    last_insurance_withdraw_slot: 0,
                    _liw_padding: 0,
                    // Mark EWMA: Hyperp bootstraps from initial mark, non-Hyperp from first trade
                    mark_ewma_e6: if is_hyperp { initial_mark_price_e6 } else { 0 },
                    mark_ewma_last_slot: if is_hyperp { clock.slot } else { 0 },
                    mark_ewma_halflife_slots: DEFAULT_MARK_EWMA_HALFLIFE_SLOTS,
                    _ewma_padding: 0,
                    permissionless_resolve_stale_slots,
                    // Init to clock.slot so permissionless resolution timer starts
                    // from market creation, not slot 0 (prevents immediate resolution
                    // if the oracle happens to be down during market creation).
                    last_good_oracle_slot: clock.slot,
                    maintenance_fee_per_slot,
                    last_fee_charge_slot: clock.slot,
                    _fee_padding: 0,
                    mark_min_fee,
                    force_close_delay_slots,
                };
                // Hyperp markets must have non-zero cap for index smoothing
                if is_hyperp && config.oracle_price_cap_e2bps == 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                state::write_config(&mut data, &config);

                let new_header = SlabHeader {
                    magic: MAGIC,
                    version: 0, // unused, no versioning
                    bump,
                    _padding: [0; 3],
                    admin: a_admin.key.to_bytes(),
                    _reserved: [0; 24],
                };
                state::write_header(&mut data, &new_header);
                // Step 4: Explicitly initialize nonce to 0 for determinism
                state::write_req_nonce(&mut data, 0);
            }
            Instruction::InitUser { fee_payment } => {
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

                // Block new users when market is resolved
                if zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }
                let config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let auth = accounts::derive_vault_authority_with_bump(
                    program_id, a_slab.key, config.vault_authority_bump,
                )?;
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_user_ata, a_user.key, &mint)?;

                let clock = Clock::from_account_info(a_clock)?;

                // Reject misaligned deposits — dust would be silently donated
                let (_units_check, dust_check) = crate::units::base_to_units(fee_payment, config.unit_scale);
                if dust_check != 0 {
                    return Err(ProgramError::InvalidArgument);
                }

                // Transfer base tokens to vault
                collateral::deposit(a_token, a_user_ata, a_vault, a_user, fee_payment)?;

                // Convert base tokens to units for engine
                let (units, _dust) = crate::units::base_to_units(fee_payment, config.unit_scale);

                let engine = zc::engine_mut(&mut data)?;
                // Use engine's canonical materialization API (§10.3).
                // Handles fee deduction, dust rejection, vault/insurance accounting,
                // max_accounts bound, and materialized_account_count tracking.
                let idx = engine.materialize_with_fee(
                    percolator::Account::KIND_USER,
                    units as u128,
                    [0u8; 32], // no matcher for users
                    [0u8; 32],
                ).map_err(map_risk_error)?;
                engine.set_owner(idx, a_user.key.to_bytes())
                    .map_err(map_risk_error)?;
            }
            Instruction::InitLP {
                matcher_program,
                matcher_context,
                fee_payment,
            } => {
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

                // Block new LPs when market is resolved
                if zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }

                let config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let auth = accounts::derive_vault_authority_with_bump(
                    program_id, a_slab.key, config.vault_authority_bump,
                )?;
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_user_ata, a_user.key, &mint)?;

                let clock = Clock::from_account_info(a_clock)?;

                // Reject misaligned deposits — dust would be silently donated
                let (_units_check, dust_check) = crate::units::base_to_units(fee_payment, config.unit_scale);
                if dust_check != 0 {
                    return Err(ProgramError::InvalidArgument);
                }

                // Transfer base tokens to vault
                collateral::deposit(a_token, a_user_ata, a_vault, a_user, fee_payment)?;

                // Convert base tokens to units for engine
                let (units, _dust) = crate::units::base_to_units(fee_payment, config.unit_scale);

                let engine = zc::engine_mut(&mut data)?;
                // Use engine's canonical materialization API (§10.3).
                let idx = engine.materialize_with_fee(
                    percolator::Account::KIND_LP,
                    units as u128,
                    matcher_program.to_bytes(),
                    matcher_context.to_bytes(),
                ).map_err(map_risk_error)?;
                engine.set_owner(idx, a_user.key.to_bytes())
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

                // Block deposits when market is resolved
                if zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }

                let config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let auth = accounts::derive_vault_authority_with_bump(
                    program_id, a_slab.key, config.vault_authority_bump,
                )?;
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_user_ata, a_user.key, &mint)?;

                let clock = Clock::from_account_info(a_clock)?;

                // Reject misaligned deposits — dust would be silently donated
                let (_units_check, dust_check) = crate::units::base_to_units(amount, config.unit_scale);
                if dust_check != 0 {
                    return Err(ProgramError::InvalidArgument);
                }

                // Transfer base tokens to vault
                collateral::deposit(a_token, a_user_ata, a_vault, a_user, amount)?;

                // Convert base tokens to units for engine
                let (units, _dust) = crate::units::base_to_units(amount, config.unit_scale);

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, user_idx)?;

                // Owner authorization via verify helper (Kani-provable)
                let owner = engine.accounts[user_idx as usize].owner;
                if !crate::verify::owner_ok(owner, a_user.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                engine
                    .deposit(user_idx, units as u128, 0, clock.slot)
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
                let mut config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let derived_pda = accounts::derive_vault_authority_with_bump(
                    program_id, a_slab.key, config.vault_authority_bump,
                )?;
                accounts::expect_key(a_vault_pda, &derived_pda)?;

                verify_vault(
                    a_vault,
                    &derived_pda,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_user_ata, a_user.key, &mint)?;

                // Block withdrawals on resolved markets.
                // The engine's withdraw_not_atomic requires MarketMode::Live.
                // After resolution, users exit via CloseAccount / ForceCloseResolved.
                if zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }

                let clock = Clock::from_account_info(a_clock)?;
                let price = {
                    let is_hyperp = oracle::is_hyperp_mode(&config);
                    let px = if is_hyperp {
                        let eng = zc::engine_ref(&data)?;
                        let last_slot = eng.current_slot;
                        oracle::get_engine_oracle_price_e6(
                            last_slot, clock.slot, clock.unix_timestamp,
                            &mut config, a_oracle_idx,
                        )?
                    } else {
                        read_price_and_stamp(&mut config, a_oracle_idx, clock.unix_timestamp, clock.slot)?
                    };
                    state::write_config(&mut data, &config);
                    px
                };

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, user_idx)?;

                // Owner authorization via verify helper (Kani-provable)
                let owner = engine.accounts[user_idx as usize].owner;
                if !crate::verify::owner_ok(owner, a_user.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                // withdraw_not_atomic internally calls touch_account_full.
                // No separate pre-touch needed — it would run without lifecycle
                // handling and leave stale side state.

                // Reject misaligned withdrawal amounts (cleaner UX than silent floor)
                if config.unit_scale != 0 && amount % config.unit_scale as u64 != 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }

                // Convert requested base tokens to units
                let (units_requested, _) = crate::units::base_to_units(amount, config.unit_scale);

                let withdraw_slot = clock.slot;
                let h_lock = engine.params.h_min;
                engine
                    .withdraw_not_atomic(user_idx, units_requested as u128, price, withdraw_slot,
                        compute_current_funding_rate_e9(&config), h_lock)
                    .map_err(map_risk_error)?;

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
                candidates,
            } => {
                use crate::constants::CRANK_NO_CALLER;

                accounts::expect_len(accounts, 4)?;
                let a_caller = &accounts[0];
                let a_slab = &accounts[1];
                let a_clock = &accounts[2];
                let a_oracle = &accounts[3];

                // Permissionless mode: caller_idx == u16::MAX means anyone can crank.
                // Resolved markets are always permissionless (settlement is idempotent).
                let permissionless = caller_idx == CRANK_NO_CALLER;

                if !permissionless {
                    accounts::expect_signer(a_caller)?;
                }
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                // Check if market is resolved - frozen time mode.
                // NOTE: resolved crank is effectively permissionless regardless of
                // caller_idx — the resolved path returns before owner-match checks.
                // This is intentional: settlement is idempotent and no funds move.
                // All resolved operations use engine.current_slot (frozen at
                // last pre-resolution crank) instead of clock.slot.
                if zc::engine_ref(&data)?.is_resolved() {
                    let engine = zc::engine_mut(&mut data)?;
                    let (resolved_price, _) = engine.resolved_context();
                    if resolved_price == 0 {
                        return Err(ProgramError::InvalidAccountData);
                    }

                    // Resolved crank: no per-account settlement here.
                    // Accounts are settled by ForceCloseResolved / CloseAccount
                    // which call force_close_resolved_not_atomic atomically.
                    // The resolved crank only handles lifecycle.

                    // §10.0 steps 4-7 / §10.8 steps 9-12: end-of-instruction lifecycle.
                    // Propagate CorruptState (real invariant violation), ignore other
                    // errors (side-reset may fail on frozen ADL state post-resolution).
                    // Pass zero funding rate — market is resolved, no funding accrual.
                    let mut ctx = percolator::InstructionContext::new();
                    match engine.run_end_of_instruction_lifecycle(&mut ctx) {
                        Ok(()) => {}
                        Err(percolator::RiskError::CorruptState) => {
                            return Err(map_risk_error(percolator::RiskError::CorruptState));
                        }
                        Err(_) => {} // non-fatal on resolved markets
                    }

                    return Ok(());
                }

                let mut config = state::read_config(&data);

                let clock = Clock::from_account_info(a_clock)?;

                // Hyperp mode: use get_engine_oracle_price_e6 for rate-limited index smoothing
                // Otherwise: use read_price_clamped as before
                let is_hyperp = oracle::is_hyperp_mode(&config);
                let engine_last_slot = {
                    let engine = zc::engine_ref(&data)?;
                    engine.current_slot
                };

                let price = if is_hyperp {
                    // Hyperp mode: update index toward mark with rate limiting
                    oracle::get_engine_oracle_price_e6(
                        engine_last_slot,
                        clock.slot,
                        clock.unix_timestamp,
                        &mut config,
                        a_oracle,
                    )?
                } else {
                    read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot)?
                };

                state::write_config(&mut data, &config);

                // Read risk buffer BEFORE engine borrow (disjoint regions,
                // but borrow checker can't see that).
                let buf = state::read_risk_buffer(&data);

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
                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: keeper_crank_start");
                    sol_log_compute_units();
                }
                let mut combined = alloc::vec::Vec::with_capacity(
                    buf.count as usize + candidates.len(),
                );
                for i in 0..buf.count as usize {
                    combined.push((
                        buf.entries[i].idx,
                        Some(percolator::LiquidationPolicy::FullClose),
                    ));
                }
                combined.extend_from_slice(&candidates);

                let funding_rate_e9 = compute_current_funding_rate_e9(&config);
                let h_lock = engine.params.h_min;
                let _outcome = engine
                    .keeper_crank_not_atomic(
                        clock.slot,
                        price,
                        &combined,
                        percolator::LIQ_BUDGET_PER_CRANK,
                        funding_rate_e9,
                        h_lock,
                    )
                    .map_err(map_risk_error)?;
                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: keeper_crank_end");
                    sol_log_compute_units();
                }

                // ── Periodic maintenance fees (wrapper-owned, §8.3) ──
                // Charge maintenance_fee_per_slot * dt for each candidate account.
                // Uses engine.charge_account_fee_not_atomic for safe capital→insurance transfer.
                if config.maintenance_fee_per_slot > 0 {
                    let dt = clock.slot.saturating_sub(config.last_fee_charge_slot);
                    if dt > 0 {
                        let fee_per_account = config.maintenance_fee_per_slot.saturating_mul(dt as u128);
                        // Charge each candidate that was processed by the crank.
                        // Use the combined list (buffer + external candidates) to match
                        // which accounts were touched.
                        for &(cidx, _) in combined.iter() {
                            if (cidx as usize) < percolator::MAX_ACCOUNTS
                                && engine.is_used(cidx as usize)
                            {
                                // Best-effort: ignore errors (account may have been
                                // liquidated or closed during this crank).
                                let _ = engine.charge_account_fee_not_atomic(
                                    cidx, fee_per_account, clock.slot,
                                );
                            }
                        }
                        config.last_fee_charge_slot = clock.slot;
                    }
                }

                // Copy stats and drop engine mutable borrow
                let liqs = 0u64;
                let ins_low = engine.insurance_fund.balance.get() as u64;
                drop(engine);

                // Write updated config (fee charge slot may have changed)
                state::write_config(&mut data, &config);

                // ── RiskBuffer maintenance (engine borrow dropped) ──
                {
                    let mut buf = state::read_risk_buffer(&data);
                    let engine = zc::engine_ref(&data)?;

                    // Phase A: scrub dead entries
                    for i in (0..4usize).rev() {
                        if i >= buf.count as usize { continue; }
                        let eidx = buf.entries[i].idx as usize;
                        if !engine.is_used(eidx) || engine.effective_pos_q(eidx) == 0 {
                            buf.remove(buf.entries[i].idx);
                        }
                    }

                    // Phase B: refresh surviving entries
                    for i in 0..buf.count as usize {
                        let eidx = buf.entries[i].idx as usize;
                        let eff = engine.effective_pos_q(eidx);
                        let notional = percolator::wide_math::mul_div_floor_u128(
                            eff.unsigned_abs(), price as u128, percolator::POS_SCALE,
                        );
                        buf.entries[i].notional = notional;
                    }
                    buf.recompute_min();

                    // Phase C: progressive discovery scan
                    let scan_start = buf.scan_cursor as usize;
                    for offset in 0..crate::constants::RISK_SCAN_WINDOW {
                        let idx = (scan_start + offset) % percolator::MAX_ACCOUNTS;
                        if !engine.is_used(idx) { continue; }
                        let eff = engine.effective_pos_q(idx);
                        if eff == 0 { continue; }
                        let notional = percolator::wide_math::mul_div_floor_u128(
                            eff.unsigned_abs(), price as u128, percolator::POS_SCALE,
                        );
                        buf.upsert(idx as u16, notional);
                    }
                    buf.scan_cursor = ((scan_start + crate::constants::RISK_SCAN_WINDOW)
                        % percolator::MAX_ACCOUNTS) as u16;

                    // Phase D: ingest caller-supplied candidates
                    for &(cidx, _) in candidates.iter() {
                        let ci = cidx as usize;
                        if ci >= percolator::MAX_ACCOUNTS || !engine.is_used(ci) {
                            continue;
                        }
                        let eff = engine.effective_pos_q(ci);
                        if eff == 0 {
                            buf.remove(cidx);
                        } else {
                            let notional = percolator::wide_math::mul_div_floor_u128(
                                eff.unsigned_abs(), price as u128, percolator::POS_SCALE,
                            );
                            buf.upsert(cidx, notional);
                        }
                    }

                    drop(engine); // release immutable borrow before write
                    state::write_risk_buffer(&mut data, &buf);
                }

                // Debug: log lifetime counters (sol_log_64: tag, liqs, max_accounts, insurance, 0)
                msg!("CRANK_STATS");
                sol_log_64(0xC8A4C, liqs, MAX_ACCOUNTS as u64, ins_low, 0);
            }
            Instruction::TradeNoCpi {
                lp_idx,
                user_idx,
                size,
            } => {
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

                // Block trading when market is resolved
                if zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }

                let mut config = state::read_config(&data);

                let clock = Clock::from_account_info(&accounts[3])?;
                let a_oracle = &accounts[4];

                // Hyperp mode: reject TradeNoCpi to prevent mark price manipulation
                // All trades must go through TradeCpi with a pinned matcher
                if oracle::is_hyperp_mode(&config) {
                    return Err(PercolatorError::HyperpTradeNoCpiDisabled.into());
                }

                // Read oracle price with circuit-breaker clamping
                let price =
                    read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot)?;
                state::write_config(&mut data, &config);

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, lp_idx)?;
                check_idx(engine, user_idx)?;

                // TradeNoCpi: no matcher check. Both sides are bilateral signers,
                // no CPI is invoked. Matcher config only matters for TradeCpi.

                let u_owner = engine.accounts[user_idx as usize].owner;

                // Owner authorization via verify helper (Kani-provable)
                if !crate::verify::owner_ok(u_owner, a_user.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }
                let l_owner = engine.accounts[lp_idx as usize].owner;
                if !crate::verify::owner_ok(l_owner, a_lp.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                // Side-mode gating is handled inside engine.execute_trade_not_atomic()

                // Snapshot insurance fund balance for fee-weighted EWMA.
                // The delta after execute_trade = fees_collected - losses_absorbed.
                // NOTE: If loss absorption occurs during the same trade (spec §5.4),
                // delta undercounts the actual fee. This is the conservative direction:
                // mark is stickier during volatile loss-absorption events, never
                // more manipulable. A future engine API could expose fee_paid directly.
                let ins_before = engine.insurance_fund.balance.get();

                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: trade_nocpi_execute_start");
                    sol_log_compute_units();
                }
                let funding_rate_e9 = compute_current_funding_rate_e9(&config);
                execute_trade_with_matcher(
                    engine, &NoOpMatcher, lp_idx, user_idx, clock.slot, price, size,
                    funding_rate_e9,
                ).map_err(map_risk_error)?;

                // Update mark EWMA from trade (NoOpMatcher fills at oracle price).
                // NOTE: NoOpMatcher fills at oracle price, so mark_ewma converges to oracle
                // for TradeNoCpi trades. This means TradeNoCpi-only markets have zero premium
                // and zero funding. Markets that need funding must use TradeCpi with a matcher
                // that can set exec_price != oracle (creating mark/index divergence).
                // Only when circuit breaker is active (cap > 0) — without cap,
                // exec prices are unbounded and EWMA would be manipulable.
                if config.oracle_price_cap_e2bps > 0 {
                    let clamped_price = oracle::clamp_oracle_price(
                        crate::verify::mark_ewma_clamp_base(config.last_effective_price_e6),
                        price,
                        config.oracle_price_cap_e2bps,
                    );
                    // fee_paid = actual fee collected into insurance (post - pre).
                    // This is exact: no overestimate from pre-trade capital snapshot.
                    let fee_paid_nocpi = if config.mark_min_fee > 0 {
                        let ins_after = engine.insurance_fund.balance.get();
                        let delta = ins_after.saturating_sub(ins_before);
                        core::cmp::min(delta, u64::MAX as u128) as u64
                    } else { 0u64 };
                    let old_ewma = config.mark_ewma_e6;
                    config.mark_ewma_e6 = crate::verify::ewma_update(
                        old_ewma, clamped_price,
                        config.mark_ewma_halflife_slots,
                        config.mark_ewma_last_slot, clock.slot,
                        fee_paid_nocpi,
                        config.mark_min_fee,
                    );
                    // Only update the EWMA clock when the mark actually moved.
                    // Zero-weight trades must not refresh the clock — that would
                    // shrink future dt and damp legitimate updates.
                    if config.mark_ewma_e6 != old_ewma {
                        config.mark_ewma_last_slot = clock.slot;
                    }
                    // NOTE: do NOT stamp funding rate here — execute_trade_not_atomic
                    // handles it via the funding_rate parameter (§5.5 anti-retroactivity).
                }

                // v12.17: funding rate is passed to accrue_market_to, not stored directly.
                // The next accrual (crank/trade/settle) will use the updated mark EWMA.

                // Collect post-trade positions for risk buffer
                let user_eff_nocpi = engine.effective_pos_q(user_idx as usize);
                let lp_eff_nocpi = engine.effective_pos_q(lp_idx as usize);
                drop(engine);

                // Write updated config (mark_ewma changed)
                state::write_config(&mut data, &config);

                // Update risk buffer
                {
                    let mut buf = state::read_risk_buffer(&data);
                    for &(idx, eff) in &[(user_idx, user_eff_nocpi), (lp_idx, lp_eff_nocpi)] {
                        if eff == 0 {
                            buf.remove(idx);
                        } else {
                            let notional = percolator::wide_math::mul_div_floor_u128(
                                eff.unsigned_abs(), price as u128, percolator::POS_SCALE,
                            );
                            buf.upsert(idx, notional);
                        }
                    }
                    state::write_risk_buffer(&mut data, &buf);
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
                limit_price_e6,
            } => {
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
                // Reject zero-size requests at entry — zero-fill path should only
                // be reached via matcher returning exec_size == 0 on a nonzero request.
                if size == 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }
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
                let (expected_lp_pda, bump) = Pubkey::find_program_address(
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
                // PDA key match is sufficient — only this program can sign
                // for it, so it's always system-owned with zero data.

                // Phase 3 & 4: Read engine state, generate nonce, validate matcher identity
                // Note: Use immutable borrow for reading to avoid ExternalAccountDataModified
                // Nonce write is deferred until after execute_trade
                let (lp_account_id, mut config, config_pre_oracle, req_id, lp_matcher_prog, lp_matcher_ctx, engine_current_slot) = {
                    let data = a_slab.try_borrow_data()?;
                    slab_guard(program_id, a_slab, &*data)?;
                    require_initialized(&*data)?;

                    // Block trading when market is resolved
                    if zc::engine_ref(&*data)?.is_resolved() {
                        return Err(ProgramError::InvalidAccountData);
                    }
                    // Reentrancy guard: reject if another CPI is in progress.
                    // Prevents malicious matcher from re-entering TradeCpi during
                    // its callback, which would execute two trades for one user signature.
                    if state::is_cpi_in_progress(&*data) {
                        return Err(ProgramError::InvalidAccountData);
                    }

                    let config = state::read_config(&*data);
                    // Snapshot config BEFORE oracle/index mutations for zero-fill rollback.
                    let config_pre_oracle = config;

                    // Phase 3: Monotonic nonce for req_id (prevents replay attacks)
                    // Nonce advancement via verify helper (Kani-provable)
                    let nonce = state::read_req_nonce(&*data);
                    let req_id = crate::verify::nonce_on_success(nonce);

                    let engine = zc::engine_ref(&*data)?;

                    check_idx(engine, lp_idx)?;
                    check_idx(engine, user_idx)?;

                    // TradeCpi: require lp_idx has matcher config (non-zero matcher_program).
                    // The matcher program/context are used for CPI — zero fields would
                    // cause CPI to fail or route to the wrong program.
                    // This uses matcher config, not account kind, as the LP capability check.
                    if engine.accounts[lp_idx as usize].matcher_program == [0u8; 32] {
                        return Err(PercolatorError::EngineAccountKindMismatch.into());
                    }

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
                        lp_idx as u64,
                        config,
                        config_pre_oracle,
                        req_id,
                        lp_acc.matcher_program,
                        lp_acc.matcher_context,
                        engine.current_slot,
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

                let clock = Clock::from_account_info(a_clock)?;
                // Oracle price: Hyperp mode applies rate-limited index update
                // via clamp_toward_with_dt (prevents stale-index manipulation).
                // Non-Hyperp: standard circuit-breaker clamping.
                let is_hyperp = oracle::is_hyperp_mode(&config);
                let price = if is_hyperp {
                    oracle::get_engine_oracle_price_e6(
                        engine_current_slot, clock.slot, clock.unix_timestamp,
                        &mut config, a_oracle,
                    )?
                } else {
                    read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot)?
                };

                // Note: We don't zero the matcher_ctx before CPI because we don't own it.
                // Security is maintained by ABI validation which checks req_id (nonce),
                // lp_account_id, and oracle_price_e6 all match the request parameters.

                // Stack-allocated CPI data (67 bytes) — avoids heap allocation
                let mut cpi_data = [0u8; MATCHER_CALL_LEN];
                cpi_data[0] = MATCHER_CALL_TAG;
                cpi_data[1..9].copy_from_slice(&req_id.to_le_bytes());
                cpi_data[9..11].copy_from_slice(&lp_idx.to_le_bytes());
                cpi_data[11..19].copy_from_slice(&lp_account_id.to_le_bytes());
                cpi_data[19..27].copy_from_slice(&price.to_le_bytes());
                cpi_data[27..43].copy_from_slice(&size.to_le_bytes());
                // bytes 43..67 already zero (padding)

                let metas = [
                    AccountMeta::new_readonly(*a_lp_pda.key, true),
                    AccountMeta::new(*a_matcher_ctx.key, false),
                ];

                let ix = SolInstruction {
                    program_id: *a_matcher_prog.key,
                    accounts: metas.to_vec(),
                    data: cpi_data.to_vec(),
                };

                let bump_arr = [bump];
                let seeds: &[&[u8]] = &[b"lp", a_slab.key.as_ref(), &lp_bytes, &bump_arr];

                // Set reentrancy guard BEFORE CPI. Any reentrant TradeCpi will
                // see FLAG_CPI_IN_PROGRESS and abort.
                {
                    let mut data = state::slab_data_mut(a_slab)?;
                    state::set_cpi_in_progress(&mut data);
                }

                // Phase 2: Use zc helper for CPI - slab not passed to avoid ExternalAccountDataModified
                zc::invoke_signed_trade(&ix, a_lp_pda, a_matcher_ctx, a_matcher_prog, seeds)?;

                // Clear reentrancy guard after CPI returns.
                {
                    let mut data = state::slab_data_mut(a_slab)?;
                    state::clear_cpi_in_progress(&mut data);
                }

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

                // User-side slippage protection.
                // Normalize limit to engine-space (same invert+scale as exec_price).
                // For inverted markets, inversion is order-reversing: a "better"
                // raw buy price maps to a larger engine price, so inequalities flip.
                if limit_price_e6 != 0 && ret.exec_size != 0 {
                    let limit_eng = crate::verify::to_engine_price(
                        limit_price_e6, config.invert, config.unit_scale,
                    ).ok_or(PercolatorError::OracleInvalid)?;
                    let inverted = config.invert != 0;
                    if size > 0 {
                        // Buying: raw user wants exec <= limit (pay no more)
                        // Normal:   exec_eng > limit_eng → reject
                        // Inverted: exec_eng < limit_eng → reject (order flipped)
                        let bad = if inverted {
                            ret.exec_price_e6 < limit_eng
                        } else {
                            ret.exec_price_e6 > limit_eng
                        };
                        if bad {
                            return Err(ProgramError::InvalidAccountData);
                        }
                    } else {
                        // Selling: raw user wants exec >= limit (receive no less)
                        // Normal:   exec_eng < limit_eng → reject
                        // Inverted: exec_eng > limit_eng → reject (order flipped)
                        let bad = if inverted {
                            ret.exec_price_e6 > limit_eng
                        } else {
                            ret.exec_price_e6 < limit_eng
                        };
                        if bad {
                            return Err(ProgramError::InvalidAccountData);
                        }
                    }
                }

                // Zero-fill: ABI-valid no-op when matcher returns exec_size == 0
                // with FLAG_PARTIAL_OK. Skip engine call which rejects size_q == 0.
                // Zero-fill: no trade occurred, so do not persist oracle side effects.
                // Revert last_effective_price_e6 for ALL markets — prevents repeated
                // zero-fills from walking the circuit-breaker baseline toward the raw
                // oracle price (Hyperp: index ratchet, non-Hyperp: baseline walk).
                // SAFETY: mark_ewma_e6 is NOT reverted here because the EWMA update
                // happens AFTER this early return (inside the exec_size != 0 branch below).
                // Zero-fills never touch the EWMA, so no revert is needed.
                if ret.exec_size == 0 {
                    // Restore the pre-oracle-read config snapshot.
                    // The wrapper mutated config during oracle/index processing
                    // and wrote it to the slab before the CPI. Reading "pristine"
                    // from slab here would get the mutated version — use the
                    // explicit pre-oracle snapshot instead.
                    let mut data = state::slab_data_mut(a_slab)?;
                    state::write_config(&mut data, &config_pre_oracle);
                    state::write_req_nonce(&mut data, req_id);
                    return Ok(());
                }

                let exec_price = ret.exec_price_e6;
                // Reject extreme exec prices that would corrupt engine state
                // or produce absurd PnL. Must check BEFORE engine call.
                if exec_price > percolator::MAX_ORACLE_PRICE {
                    return Err(PercolatorError::OracleInvalid.into());
                }

                // Anti-off-market execution policy (§14.3):
                // |exec_price - oracle_price| * 10_000 <= band * oracle_price
                // where band = max(2 * trading_fee_bps, 100) (at least 1% band).
                // This prevents the matcher from filling at wildly off-market prices.
                if exec_price > 0 && price > 0 {
                    let band_bps = {
                        let data_ref = a_slab.try_borrow_data()?;
                        let engine_ref = zc::engine_ref(&data_ref)?;
                        let fee_bps = engine_ref.params.trading_fee_bps;
                        core::cmp::max(fee_bps.saturating_mul(2), 100) // at least 1%
                    };
                    let diff = if exec_price > price {
                        exec_price - price
                    } else {
                        price - exec_price
                    };
                    // diff * 10_000 <= band_bps * price (both sides u128 to avoid overflow)
                    let lhs = (diff as u128).saturating_mul(10_000);
                    let rhs = (band_bps as u128).saturating_mul(price as u128);
                    if lhs > rhs {
                        return Err(PercolatorError::OracleInvalid.into());
                    }
                }
                {
                    let mut data = state::slab_data_mut(a_slab)?;
                    let engine = zc::engine_mut(&mut data)?;

                    let trade_size = crate::verify::cpi_trade_size(ret.exec_size, size);

                    // Snapshot insurance for fee-weighted EWMA (delta approach).
                    // NOTE: delta = fees - losses_absorbed. Conservative undercount
                    // during volatile loss-absorption events (see TradeNoCpi comment).
                    let ins_before_cpi = engine.insurance_fund.balance.get();

                    #[cfg(feature = "cu-audit")]
                    {
                        msg!("CU_CHECKPOINT: trade_cpi_execute_start");
                        sol_log_compute_units();
                    }
                    let matcher = CpiMatcher {
                        exec_price,
                        exec_size: trade_size,
                    };
                    // Compute funding BEFORE trade (uses pre-fill state per anti-retroactivity)
                    let funding_rate_e9 = compute_current_funding_rate_e9(&config);
                    execute_trade_with_matcher(
                        engine, &matcher, lp_idx, user_idx, clock.slot, price, trade_size,
                        funding_rate_e9,
                    ).map_err(map_risk_error)?;
                    #[cfg(feature = "cu-audit")]
                    {
                        msg!("CU_CHECKPOINT: trade_cpi_execute_end");
                        sol_log_compute_units();
                    }
                    // Update trade-derived mark EWMA (all market types).
                    // Only when circuit breaker is active — without cap, exec prices
                    // are unbounded and EWMA would be manipulable.
                    if config.oracle_price_cap_e2bps > 0 {
                        let clamped_exec = oracle::clamp_oracle_price(
                            crate::verify::mark_ewma_clamp_base(config.last_effective_price_e6),
                            ret.exec_price_e6,
                            config.oracle_price_cap_e2bps,
                        );
                        // fee_paid = actual fee collected into insurance (post - pre).
                        let fee_paid_cpi = if config.mark_min_fee > 0 {
                            let ins_after_cpi = engine.insurance_fund.balance.get();
                            let delta = ins_after_cpi.saturating_sub(ins_before_cpi);
                            core::cmp::min(delta, u64::MAX as u128) as u64
                        } else { 0u64 };
                        let old_ewma_cpi = config.mark_ewma_e6;
                        config.mark_ewma_e6 = crate::verify::ewma_update(
                            old_ewma_cpi,
                            clamped_exec,
                            config.mark_ewma_halflife_slots,
                            config.mark_ewma_last_slot,
                            clock.slot,
                            fee_paid_cpi,
                            config.mark_min_fee,
                        );
                        // Only update EWMA clock when mark actually moved
                        if config.mark_ewma_e6 != old_ewma_cpi {
                            config.mark_ewma_last_slot = clock.slot;
                        }
                        // NOTE: do NOT stamp funding rate here — execute_trade_not_atomic
                        // handles it via the funding_rate parameter (§5.5 anti-retroactivity).
                    }

                    // Hyperp: also update authority_price (legacy mark field)
                    if is_hyperp {
                        config.authority_price_e6 = oracle::clamp_oracle_price(
                            config.last_effective_price_e6,
                            ret.exec_price_e6,
                            config.oracle_price_cap_e2bps,
                        );
                        config.last_mark_push_slot = clock.slot as u128;
                    }
                }
                // Engine borrow dropped.
                // Collect post-trade positions for risk buffer (re-borrow as ref)
                let (user_eff_cpi, lp_eff_cpi) = {
                    let data = a_slab.try_borrow_data()?;
                    let engine = zc::engine_ref(&data)?;
                    (engine.effective_pos_q(user_idx as usize),
                     engine.effective_pos_q(lp_idx as usize))
                };
                // Write nonce + config + risk buffer.
                {
                    let mut data = state::slab_data_mut(a_slab)?;
                    state::write_req_nonce(&mut data, req_id);
                    state::write_config(&mut data, &config);
                    // v12.17: funding rate passed to accrue_market_to, not stored directly.
                    // Update risk buffer — use oracle price for notional ranking (H1/M9).
                    // exec_price is gameable by a colluding matcher; oracle price is not.
                    let mut buf = state::read_risk_buffer(&data);
                    for &(idx, eff) in &[(user_idx, user_eff_cpi), (lp_idx, lp_eff_cpi)] {
                        if eff == 0 {
                            buf.remove(idx);
                        } else {
                            let notional = percolator::wide_math::mul_div_floor_u128(
                                eff.unsigned_abs(), price as u128, percolator::POS_SCALE,
                            );
                            buf.upsert(idx, notional);
                        }
                    }
                    state::write_risk_buffer(&mut data, &buf);
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

                // Block liquidations after market resolution — resolved markets
                // are in withdraw-only settlement phase.
                if zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }

                let mut config = state::read_config(&data);

                let clock = Clock::from_account_info(&accounts[2])?;
                let is_hyperp = oracle::is_hyperp_mode(&config);
                let price = if is_hyperp {
                    // Read engine.current_slot before mutable borrow
                    let eng = zc::engine_ref(&data)?;
                    let last_slot = eng.current_slot;
                    oracle::get_engine_oracle_price_e6(
                        last_slot, clock.slot, clock.unix_timestamp,
                        &mut config, a_oracle,
                    )?
                } else {
                    read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot)?
                };
                state::write_config(&mut data, &config);

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, target_idx)?;

                // Debug logging for liquidation (using sol_log_64 for no_std)
                sol_log_64(target_idx as u64, price, 0, 0, 0); // idx, price
                {
                    let acc = &engine.accounts[target_idx as usize];
                    sol_log_64(acc.capital.get() as u64, 0, 0, 0, 1); // cap
                    let eff = engine.effective_pos_q(target_idx as usize);
                    let notional = engine.notional(target_idx as usize, price);
                    sol_log_64(notional as u64, (eff == 0) as u64, 0, 0, 2); // notional, has_pos
                }

                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: liquidate_start");
                    sol_log_compute_units();
                }
                let h_lock = engine.params.h_min;
                let _res = engine
                    .liquidate_at_oracle_not_atomic(target_idx, clock.slot, price,
                        percolator::LiquidationPolicy::FullClose,
                        compute_current_funding_rate_e9(&config),
                        h_lock)
                    .map_err(map_risk_error)?;
                sol_log_64(_res as u64, 0, 0, 0, 4); // result

                // Collect post-liquidation position for risk buffer
                let liq_eff = engine.effective_pos_q(target_idx as usize);
                drop(engine);

                // Update risk buffer (engine borrow dropped)
                {
                    let mut buf = state::read_risk_buffer(&data);
                    if liq_eff == 0 {
                        buf.remove(target_idx);
                    } else {
                        let notional = percolator::wide_math::mul_div_floor_u128(
                            liq_eff.unsigned_abs(), price as u128, percolator::POS_SCALE,
                        );
                        buf.upsert(target_idx, notional);
                    }
                    state::write_risk_buffer(&mut data, &buf);
                }

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

                let auth = accounts::derive_vault_authority_with_bump(
                    program_id, a_slab.key, config.vault_authority_bump,
                )?;
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_user_ata, a_user.key, &mint)?;
                accounts::expect_key(a_pda, &auth)?;

                let resolved = zc::engine_ref(&data)?.is_resolved();
                let clock = Clock::from_account_info(&accounts[6])?;
                let price = if resolved {
                    let eng = zc::engine_ref(&data)?;
                    let (settlement, _) = eng.resolved_context();
                    if settlement == 0 {
                        return Err(ProgramError::InvalidAccountData);
                    }
                    settlement
                } else {
                    let is_hyperp = oracle::is_hyperp_mode(&config);
                    let px = if is_hyperp {
                        let eng = zc::engine_ref(&data)?;
                        let last_slot = eng.current_slot;
                        oracle::get_engine_oracle_price_e6(
                            last_slot, clock.slot, clock.unix_timestamp,
                            &mut config, a_oracle,
                        )?
                    } else {
                        read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot)?
                    };
                    state::write_config(&mut data, &config);
                    px
                };

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
                let amt_units = if resolved {
                    // force_close_resolved handles K-pair PnL, maintenance fees,
                    // loss settlement, and account close internally.
                    match engine.force_close_resolved_not_atomic(user_idx, engine.resolved_context().1)
                        .map_err(map_risk_error)?
                    {
                        percolator::ResolvedCloseResult::Deferred => {
                            // Phase 1 reconciliation only — account still open.
                            // Caller must retry after all accounts reconciled.
                            return Ok(());
                        }
                        percolator::ResolvedCloseResult::Closed(payout) => payout,
                    }
                } else {
                    let h_lock = engine.params.h_min;
                    engine
                        .close_account_not_atomic(user_idx, clock.slot, price,
                            compute_current_funding_rate_e9(&config),
                            h_lock)
                        .map_err(map_risk_error)?
                };
                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: close_account_end");
                    sol_log_compute_units();
                }

                let amt_units_u64: u64 = amt_units
                    .try_into()
                    .map_err(|_| PercolatorError::EngineOverflow)?;

                // Remove from risk buffer (drop engine borrow first to release data)
                drop(engine);
                {
                    let mut buf = state::read_risk_buffer(&data);
                    buf.remove(user_idx);
                    state::write_risk_buffer(&mut data, &buf);
                }

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

                // Block insurance top-up when market is resolved
                if zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }

                let config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let auth = accounts::derive_vault_authority_with_bump(
                    program_id, a_slab.key, config.vault_authority_bump,
                )?;
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_user_ata, a_user.key, &mint)?;

                // Reject misaligned deposits — dust would be silently donated
                let (_units_check, dust_check) = crate::units::base_to_units(amount, config.unit_scale);
                if dust_check != 0 {
                    return Err(ProgramError::InvalidArgument);
                }

                // Transfer base tokens to vault
                collateral::deposit(a_token, a_user_ata, a_vault, a_user, amount)?;

                // Convert base tokens to units for engine
                let (units, _dust) = crate::units::base_to_units(amount, config.unit_scale);

                let clock = Clock::from_account_info(a_clock)?;
                let engine = zc::engine_mut(&mut data)?;
                engine
                    .top_up_insurance_fund(units as u128, clock.slot)
                    .map_err(map_risk_error)?;
            }
            Instruction::UpdateAdmin { new_admin } => {
                accounts::expect_len(accounts, 2)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;

                // Zero-address admin permanently burns admin authority (§7 step [3]).
                // require_admin rejects [0u8;32] so all admin instructions become
                // permanently inaccessible once set.

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let mut header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                // Liveness guard: block admin burn unless the market can fully
                // self-recover without admin. Burning admin is irreversible and
                // strands any funds that require admin action to recover.
                //
                // Admin burn guard: burning admin permanently disables admin ops.
                // This is intentional — it locks the insurance fund and proves
                // the admin cannot extract it. The slab rent is accepted as
                // stranded (CloseSlab requires admin).
                //
                // For live markets: require permissionless resolution + force-close
                // so the market lifecycle can complete without admin.
                // For resolved markets: allow unconditionally (admin already resolved).
                if new_admin.to_bytes() == [0u8; 32] {
                    let mut config = state::read_config(&data);

                    // Reject burn if insurance-withdraw policy is configured.
                    // Otherwise the configured authority can still extract insurance
                    // after admin is burned, defeating the "no privileged extraction" guarantee.
                    if state::is_policy_configured(&data) {
                        return Err(PercolatorError::InvalidConfigParam.into());
                    }

                    let resolved = zc::engine_ref(&data)?.is_resolved();
                    let engine = zc::engine_ref(&data)?;
                    let has_accounts = engine.num_used_accounts > 0;

                    if !resolved {
                        // Live market: require both permissionless paths
                        let has_permissionless_resolve = config.permissionless_resolve_stale_slots > 0;
                        let has_permissionless_force_close = config.force_close_delay_slots > 0;
                        if !has_permissionless_resolve || !has_permissionless_force_close {
                            return Err(PercolatorError::InvalidConfigParam.into());
                        }
                    } else if has_accounts {
                        // Resolved market with open accounts: require permissionless
                        // force-close so abandoned accounts can still be cleaned up.
                        if config.force_close_delay_slots == 0 {
                            return Err(PercolatorError::InvalidConfigParam.into());
                        }
                    }
                    // Resolved + no accounts: allow unconditionally

                    // Clear oracle authority on burn. PushOraclePrice authorizes off
                    // config.oracle_authority, not header.admin. Without this, a
                    // "burned" market still has a privileged signer that can push
                    // prices, influence liquidations/funding, and block permissionless
                    // resolution by staying fresh.
                    // NOTE: do NOT clear config.authority_price_e6 — it may be needed
                    // for legacy paths. Resolved exits now use engine.resolved_context().
                    if config.oracle_authority != [0u8; 32] {
                        config.oracle_authority = [0u8; 32];
                        config.authority_timestamp = 0;
                        state::write_config(&mut data, &config);
                    }
                }

                header.admin = new_admin.to_bytes();
                state::write_header(&mut data, &header);
            }

            Instruction::CloseSlab => {
                accounts::expect_len(accounts, 6)?;
                let a_dest = &accounts[0];
                let a_slab = &accounts[1];
                let a_vault = &accounts[2];
                let a_vault_auth = &accounts[3];
                let a_dest_ata = &accounts[4];
                let a_token = &accounts[5];

                accounts::expect_signer(a_dest)?;
                accounts::expect_writable(a_slab)?;
                verify_token_program(a_token)?;

                {
                    let mut data = state::slab_data_mut(a_slab)?;
                    slab_guard(program_id, a_slab, &data)?;
                    require_initialized(&data)?;

                    // Require resolved — enforce lifecycle ordering
                    if !zc::engine_ref(&data)?.is_resolved() {
                        return Err(ProgramError::InvalidAccountData);
                    }

                    let header = state::read_header(&data);
                    require_admin(header.admin, a_dest.key)?;

                    let config = state::read_config(&data);
                    let mint = Pubkey::new_from_array(config.collateral_mint);
                    let auth = accounts::derive_vault_authority_with_bump(
                    program_id, a_slab.key, config.vault_authority_bump,
                )?;
                    verify_vault(
                        a_vault,
                        &auth,
                        &mint,
                        &Pubkey::new_from_array(config.vault_pubkey),
                    )?;

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

                    // Drain any stranded vault tokens (unsolicited transfers or
                    // sub-scale dust) to admin's ATA. This is the terminal cleanup
                    // path — engine accounting is already zero.
                    let vault_data = a_vault.try_borrow_data()?;
                    let vault_token = spl_token::state::Account::unpack(&vault_data)?;
                    let stranded = vault_token.amount;
                    drop(vault_data);

                    if stranded > 0 {
                        // Validate admin's token account before drain
                        verify_token_account(a_dest_ata, a_dest.key, &mint)?;
                        // Verify vault authority PDA
                        let expected_auth = Pubkey::create_program_address(
                            &[b"vault", a_slab.key.as_ref(), &[config.vault_authority_bump]],
                            program_id,
                        ).map_err(|_| ProgramError::InvalidSeeds)?;
                        if a_vault_auth.key != &expected_auth {
                            return Err(ProgramError::InvalidSeeds);
                        }

                        let seed1: &[u8] = b"vault";
                        let seed2: &[u8] = a_slab.key.as_ref();
                        let bump_arr: [u8; 1] = [config.vault_authority_bump];
                        let seed3: &[u8] = &bump_arr;
                        let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
                        let signer_seeds: [&[&[u8]]; 1] = [&seeds];
                        // Drain stranded vault tokens → admin ATA
                        collateral::withdraw(
                            a_token,
                            a_vault,
                            a_dest_ata,
                            a_vault_auth,
                            stranded,
                            &signer_seeds,
                        )?;
                    }

                    // Close the vault token account to recover its rent.
                    // SPL Token CloseAccount transfers remaining rent to destination.
                    {
                        let seed1: &[u8] = b"vault";
                        let seed2: &[u8] = a_slab.key.as_ref();
                        let bump_arr: [u8; 1] = [config.vault_authority_bump];
                        let seed3: &[u8] = &bump_arr;
                        let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
                        let signer_seeds: [&[&[u8]]; 1] = [&seeds];

                        let close_ix = spl_token::instruction::close_account(
                            a_token.key,
                            a_vault.key,
                            a_dest.key,  // rent destination
                            a_vault_auth.key,
                            &[],
                        )?;
                        solana_program::program::invoke_signed(
                            &close_ix,
                            &[a_vault.clone(), a_dest.clone(), a_vault_auth.clone(), a_token.clone()],
                            &signer_seeds,
                        )?;
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
                funding_max_premium_bps,
                funding_max_bps_per_slot,
            } => {
                accounts::expect_len(accounts, 3)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];
                let a_clock = &accounts[2];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                if zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }
                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                // Validate parameters
                if funding_horizon_slots == 0 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                // Reject negative funding bounds — reversed clamp bounds panic
                if funding_max_premium_bps < 0 || funding_max_bps_per_slot < 0
                    || funding_bps_to_e9(funding_max_bps_per_slot) > percolator::MAX_ABS_FUNDING_E9_PER_SLOT
                {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }

                // Read existing config
                let mut config = state::read_config(&data);

                if funding_k_bps > 100_000 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }

                // Flush Hyperp index WITHOUT staleness check (admin recovery path).
                let clock = Clock::from_account_info(a_clock)?;
                if oracle::is_hyperp_mode(&config) {
                    let prev_index = config.last_effective_price_e6;
                    let mark = if config.mark_ewma_e6 > 0 { config.mark_ewma_e6 } else { config.authority_price_e6 };
                    if mark > 0 && prev_index > 0 {
                        let last_idx_slot = config.last_hyperp_index_slot;
                        let dt = clock.slot.saturating_sub(last_idx_slot);
                        let new_index = oracle::clamp_toward_with_dt(
                            prev_index.max(1), mark, config.oracle_price_cap_e2bps, dt,
                        );
                        config.last_effective_price_e6 = new_index;
                        config.last_hyperp_index_slot = clock.slot;
                    }
                    state::write_config(&mut data, &config);
                }
                // Accrue to boundary using engine's already-stored rate.
                // Do NOT overwrite funding_rate_bps_per_slot_last before accrual —
                // that would retroactively reprice the elapsed interval.
                // Both Hyperp and non-Hyperp must accrue before changing funding params.
                {
                    let accrual_price = if oracle::is_hyperp_mode(&config) {
                        config.last_effective_price_e6
                    } else {
                        // Non-Hyperp: use last oracle price ONLY if engine has seen
                        // a real oracle (last_oracle_price > 1). Sentinel price 1
                        // from init must never be used for accrual.
                        let engine = zc::engine_ref(&data)?;
                        if engine.last_oracle_price > 1 { engine.last_oracle_price } else { 0 }
                    };
                    if accrual_price > 0 {
                        let engine = zc::engine_mut(&mut data)?;
                        engine.accrue_market_to(clock.slot, accrual_price,
                            compute_current_funding_rate_e9(&config))
                            .map_err(map_risk_error)?;
                    }
                }

                config.funding_horizon_slots = funding_horizon_slots;
                config.funding_k_bps = funding_k_bps;
                config.funding_max_premium_bps = funding_max_premium_bps;
                config.funding_max_bps_per_slot = funding_max_bps_per_slot;
                // Run end-of-instruction lifecycle after accrue + config change.
                // Finalizes pending resets triggered by the accrual.
                {
                    let engine = zc::engine_mut(&mut data)?;
                    let mut ctx = percolator::InstructionContext::new();
                    match engine.run_end_of_instruction_lifecycle(&mut ctx) {
                        Ok(()) => {}
                        Err(percolator::RiskError::CorruptState) => {
                            return Err(map_risk_error(percolator::RiskError::CorruptState));
                        }
                        Err(_) => {} // non-fatal (side reset may fail on frozen ADL state)
                    }
                }
                state::write_config(&mut data, &config);
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
                if zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }

                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                // Update oracle authority in config
                let mut config = state::read_config(&data);
                // Hyperp: reject zero-address unless trade flow has bootstrapped
                // the EWMA (mark_ewma_e6 > 0). Without trades AND no authority,
                // there's no mark price source. With EWMA bootstrapped, the market
                // can run admin-free on trade-derived mark.
                if oracle::is_hyperp_mode(&config)
                    && new_authority == Pubkey::default()
                    && config.mark_ewma_e6 == 0
                {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                config.oracle_authority = new_authority.to_bytes();
                // Clear stored price when authority changes — except on Hyperp
                // where authority_price_e6 is the mark price.
                if !oracle::is_hyperp_mode(&config) {
                    config.authority_price_e6 = 0;
                    config.authority_timestamp = 0;
                }
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
                if zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }

                let mut config = state::read_config(&data);
                let is_hyperp = oracle::is_hyperp_mode(&config);
                // Hyperp: flush index WITHOUT staleness check.
                // PushOraclePrice is the recovery path for stale marks —
                // it must not be blocked by the very staleness it's meant to fix.
                if is_hyperp {
                    let push_clock = Clock::get()
                        .map_err(|_| ProgramError::UnsupportedSysvar)?;
                    let prev_index = config.last_effective_price_e6;
                    let mark = if config.mark_ewma_e6 > 0 { config.mark_ewma_e6 } else { config.authority_price_e6 };
                    if mark > 0 && prev_index > 0 {
                        let last_idx_slot = config.last_hyperp_index_slot;
                        let dt = push_clock.slot.saturating_sub(last_idx_slot);
                        let new_index = oracle::clamp_toward_with_dt(
                            prev_index.max(1), mark, config.oracle_price_cap_e2bps, dt,
                        );
                        config.last_effective_price_e6 = new_index;
                        config.last_hyperp_index_slot = push_clock.slot;
                    }
                    state::write_config(&mut data, &config);
                    config = state::read_config(&data);
                }
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

                // Normalize to engine-space (invert + scale) for ALL markets.
                // Authority prices must be in the same price space as
                // Pyth/Chainlink-derived prices (which go through
                // read_engine_price_e6 → invert → scale).
                let normalized_price = crate::verify::to_engine_price(
                    price_e6, config.invert, config.unit_scale,
                ).ok_or(PercolatorError::OracleInvalid)?;

                // Enforce MAX_ORACLE_PRICE at ingress (engine rejects > MAX internally)
                if normalized_price > percolator::MAX_ORACLE_PRICE {
                    return Err(PercolatorError::OracleInvalid.into());
                }

                // For non-Hyperp markets, require strictly increasing timestamps
                // anchored to the current clock. This prevents the admin from
                // walking last_effective_price_e6 in a single burst (each push
                // must use a later timestamp, and the timestamp must not exceed
                // the current unix_timestamp from the clock sysvar).
                if !is_hyperp {
                    let push_clock = Clock::get()
                        .map_err(|_| ProgramError::UnsupportedSysvar)?;
                    // Strict monotonicity: reject equal timestamps
                    if config.authority_timestamp != 0
                        && timestamp <= config.authority_timestamp
                    {
                        return Err(PercolatorError::OracleStale.into());
                    }
                    // Clock anchoring: timestamp must not be in the future
                    if timestamp > push_clock.unix_timestamp {
                        return Err(PercolatorError::OracleStale.into());
                    }
                }

                // Clamp against circuit breaker.
                // Hyperp: clamp against INDEX (last_effective_price_e6), not
                //   previous mark. This bounds the mark-index gap to one
                //   cap-width regardless of how many same-slot pushes occur.
                //   The index only moves per-slot via clamp_toward_with_dt.
                // Non-Hyperp: clamp against last_effective_price_e6 baseline.
                // Accrue to boundary using engine's already-stored rate.
                // Do NOT overwrite funding_rate_bps_per_slot_last before accrual.
                if is_hyperp {
                    let push_clock2 = Clock::get()
                        .map_err(|_| ProgramError::UnsupportedSysvar)?;
                    let engine = zc::engine_mut(&mut data)?;
                    engine.accrue_market_to(
                        push_clock2.slot, config.last_effective_price_e6,
                        compute_current_funding_rate_e9(&config),
                    ).map_err(map_risk_error)?;
                }

                let clamp_base = config.last_effective_price_e6;
                let clamped = oracle::clamp_oracle_price(
                    clamp_base,
                    normalized_price,
                    config.oracle_price_cap_e2bps,
                );
                config.authority_price_e6 = clamped;
                if is_hyperp {
                    let push_clock = Clock::get()
                        .map_err(|_| ProgramError::UnsupportedSysvar)?;
                    config.last_mark_push_slot = push_clock.slot as u128;
                    // Admin push feeds through EWMA like trades do.
                    // Direct overwrite was removed — it would let a single push
                    // reset the trade-derived EWMA, defeating smoothing.
                    // Admin push always gets full weight (pass min_fee as fee_paid)
                    config.mark_ewma_e6 = crate::verify::ewma_update(
                        config.mark_ewma_e6, clamped,
                        config.mark_ewma_halflife_slots,
                        config.mark_ewma_last_slot, push_clock.slot,
                        config.mark_min_fee, config.mark_min_fee,
                    );
                    config.mark_ewma_last_slot = push_clock.slot;
                } else {
                    config.authority_timestamp = timestamp;
                    // Do NOT write last_effective_price_e6 here.
                    // That baseline must only be set by external oracle reads
                    // (crank/trade/withdraw) so admin can't poison it to bypass
                    // the settlement circuit breaker in ResolveMarket.
                }
                // Run end-of-instruction lifecycle (§5.7-5.8) after accrue_market_to.
                // This finalizes any pending DrainOnly→ResetPending→Normal transitions
                // triggered by the accrual. Without this, sides could stay DrainOnly
                // with OI=0 until the next standard-lifecycle instruction.
                if is_hyperp {
                    let engine = zc::engine_mut(&mut data)?;
                    let mut ctx = percolator::InstructionContext::new();
                    match engine.run_end_of_instruction_lifecycle(&mut ctx) {
                        Ok(()) => {}
                        Err(percolator::RiskError::CorruptState) => {
                            return Err(map_risk_error(percolator::RiskError::CorruptState));
                        }
                        Err(_) => {} // non-fatal (side reset may fail on frozen ADL state)
                    }
                }
                state::write_config(&mut data, &config);
            }

            Instruction::SetOraclePriceCap { max_change_e2bps } => {
                accounts::expect_len(accounts, 3)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];
                let a_clock = &accounts[2];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                if zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }

                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                let mut config = state::read_config(&data);
                let is_hyperp = oracle::is_hyperp_mode(&config);

                // Flush Hyperp index WITHOUT staleness check (admin path)
                if is_hyperp {
                    let clock = Clock::from_account_info(a_clock)?;
                    let prev_index = config.last_effective_price_e6;
                    let mark = if config.mark_ewma_e6 > 0 { config.mark_ewma_e6 } else { config.authority_price_e6 };
                    if mark > 0 && prev_index > 0 {
                        let last_idx_slot = config.last_hyperp_index_slot;
                        let dt = clock.slot.saturating_sub(last_idx_slot);
                        let new_index = oracle::clamp_toward_with_dt(
                            prev_index.max(1), mark, config.oracle_price_cap_e2bps, dt,
                        );
                        config.last_effective_price_e6 = new_index;
                        config.last_hyperp_index_slot = clock.slot;
                    }
                    state::write_config(&mut data, &config);
                    config = state::read_config(&data);
                    // Accrue to boundary using engine's already-stored rate.
                    let engine = zc::engine_mut(&mut data)?;
                    engine.accrue_market_to(
                        clock.slot, config.last_effective_price_e6,
                        compute_current_funding_rate_e9(&config),
                    ).map_err(map_risk_error)?;
                }

                // Hyperp markets must not set cap to 0 — it would freeze index
                // smoothing (clamp_toward_with_dt returns mark unchanged when cap==0).
                if is_hyperp && max_change_e2bps == 0 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                // Non-zero cap must be >= per-market floor.
                if max_change_e2bps != 0
                    && max_change_e2bps < config.min_oracle_price_cap_e2bps
                {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                // Non-Hyperp: cap=0 disables clamping, but if the immutable
                // floor is set, disabling clamping would let PushOraclePrice
                // walk last_effective_price_e6 arbitrarily, poisoning the
                // baseline that ResolveMarket checks against. Reject cap=0
                // when the floor is non-zero.
                if !is_hyperp
                    && max_change_e2bps == 0
                    && config.min_oracle_price_cap_e2bps != 0
                {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                // Hard ceiling: cap above 100% makes the circuit breaker vacuous
                if max_change_e2bps > MAX_ORACLE_PRICE_CAP_E2BPS {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }

                config.oracle_price_cap_e2bps = max_change_e2bps;
                // Run end-of-instruction lifecycle after accrue + cap change.
                if is_hyperp {
                    let engine = zc::engine_mut(&mut data)?;
                    let mut ctx = percolator::InstructionContext::new();
                    match engine.run_end_of_instruction_lifecycle(&mut ctx) {
                        Ok(()) => {}
                        Err(percolator::RiskError::CorruptState) => {
                            return Err(map_risk_error(percolator::RiskError::CorruptState));
                        }
                        Err(_) => {} // non-fatal (side reset may fail on frozen ADL state)
                    }
                }
                state::write_config(&mut data, &config);
            }

            Instruction::ResolveMarket => {
                // Resolve market: snapshot resolution slot, set RESOLVED flag.
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

                // Can't re-resolve
                if zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }

                // Require admin oracle price to be set (authority_price_e6 > 0)
                let config = state::read_config(&data);
                if config.authority_price_e6 == 0 {
                    return Err(ProgramError::InvalidAccountData);
                }
                // Non-Hyperp: require the settlement push to be fresh.
                // Prevents parking an old price in state and resolving later.
                if !oracle::is_hyperp_mode(&config) {
                    let clock_fresh = Clock::from_account_info(a_clock)?;
                    let push_age = clock_fresh.unix_timestamp
                        .saturating_sub(config.authority_timestamp);
                    if push_age < 0 || push_age as u64 > config.max_staleness_secs {
                        return Err(PercolatorError::OracleStale.into());
                    }
                }
                // Non-Hyperp: settlement price must be within circuit-breaker
                // bounds of a FRESH external oracle read. Uses the live
                // oracle_price_cap_e2bps (not just the immutable floor) so markets
                // with min_cap=0 but live cap>0 still get the settlement guard.
                // Hyperp: admin IS the price source, no external baseline.
                // If the oracle is stale/dead, skip the guard — the admin must
                // be able to resolve even when the oracle has died (prevents deadlock
                // on markets with nonzero cap floor + dead oracle).
                if !oracle::is_hyperp_mode(&config)
                    && config.oracle_price_cap_e2bps != 0
                {
                    let clock_tmp = Clock::from_account_info(a_clock)?;
                    let oracle_result = oracle::read_engine_price_e6(
                        a_oracle,
                        &config.index_feed_id,
                        clock_tmp.unix_timestamp,
                        config.max_staleness_secs,
                        config.conf_filter_bps,
                        config.invert,
                        config.unit_scale,
                    );
                    match oracle_result {
                        Ok(fresh_oracle) => {
                            // Oracle is live — enforce settlement guard
                            let clamped = oracle::clamp_oracle_price(
                                fresh_oracle,
                                config.authority_price_e6,
                                config.oracle_price_cap_e2bps,
                            );
                            if clamped != config.authority_price_e6 {
                                return Err(PercolatorError::OracleInvalid.into());
                            }
                        }
                        Err(e) => {
                            // Only skip guard if oracle is genuinely stale/dead.
                            // Other errors (wrong account, bad data, wrong feed) must
                            // propagate — otherwise admin can bypass guard by passing
                            // a broken oracle account.
                            let stale_err: ProgramError = PercolatorError::OracleStale.into();
                            if e != stale_err {
                                return Err(e);
                            }
                            // OracleStale = oracle is dead, allow admin to resolve
                        }
                    }
                }

                let clock = Clock::from_account_info(a_clock)?;
                let mut config = config;

                // Flush Hyperp index to resolution slot WITHOUT staleness check.
                // Admin must be able to resolve even if mark is stale.
                if oracle::is_hyperp_mode(&config) {
                    let prev_index = config.last_effective_price_e6;
                    let mark = if config.mark_ewma_e6 > 0 { config.mark_ewma_e6 } else { config.authority_price_e6 };
                    if mark > 0 && prev_index > 0 {
                        let last_idx_slot = config.last_hyperp_index_slot;
                        let dt = clock.slot.saturating_sub(last_idx_slot);
                        let new_index = oracle::clamp_toward_with_dt(
                            prev_index.max(1), mark, config.oracle_price_cap_e2bps, dt,
                        );
                        config.last_effective_price_e6 = new_index;
                        config.last_hyperp_index_slot = clock.slot;
                    }
                    state::write_config(&mut data, &config);
                }

                // Determine canonical settlement price.
                // Hyperp: use mark EWMA (smoothed observable price) for consistency
                // between admin and permissionless resolution paths. If EWMA is
                // uninitialized, fall back to authority_price_e6.
                // Non-Hyperp: use authority_price_e6 (admin-pushed settlement price).
                let settlement_price = if oracle::is_hyperp_mode(&config) {
                    let mark = config.mark_ewma_e6;
                    if mark > 0 { mark } else { config.authority_price_e6 }
                } else {
                    config.authority_price_e6
                };

                // Call the engine's resolve_market transition.
                // This does final accrual at settlement price, sets MarketMode::Resolved,
                // matures all PnL, zeros OI, and drains/finalizes sides.
                // The engine validates the price deviation band against P_last internally.
                let engine = zc::engine_mut(&mut data)?;
                engine.resolve_market(settlement_price, clock.slot)
                    .map_err(map_risk_error)?;

                state::write_config(&mut data, &config);
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
                if !zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }

                let config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let auth = accounts::derive_vault_authority_with_bump(
                    program_id, a_slab.key, config.vault_authority_bump,
                )?;
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_admin_ata, a_admin.key, &mint)?;
                accounts::expect_key(a_vault_pda, &auth)?;

                let engine = zc::engine_mut(&mut data)?;

                // Require all accounts to be fully closed (not just effective_pos_q==0,
                // which returns 0 for epoch-mismatched stale positions).
                // Any used account means unsettled state may remain.
                if engine.num_used_accounts != 0 {
                    return Err(ProgramError::InvalidAccountData);
                }

                // Get insurance balance and convert to base tokens
                let insurance_units = engine.insurance_fund.balance.get();
                if insurance_units == 0 {
                    return Ok(()); // Nothing to withdraw
                }

                // Reject if balance exceeds u64 — silent truncation would
                // zero the engine balance but only pay out a capped amount.
                let units_u64: u64 = insurance_units
                    .try_into()
                    .map_err(|_| PercolatorError::EngineOverflow)?;
                let base_amount = crate::units::units_to_base_checked(units_u64, config.unit_scale)
                    .ok_or(PercolatorError::EngineOverflow)?;

                // Zero out insurance fund and decrement engine.vault
                engine.insurance_fund.balance = percolator::U128::ZERO;
                let ins = percolator::U128::new(insurance_units);
                if ins > engine.vault {
                    return Err(PercolatorError::EngineInsufficientBalance.into());
                }
                engine.vault = engine.vault - ins;

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

            Instruction::SetInsuranceWithdrawPolicy {
                authority,
                min_withdraw_base,
                max_withdraw_bps,
                cooldown_slots,
            } => {
                accounts::expect_len(accounts, 2)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                // Policy writes oracle/index fields. Only safe when all accounts
                // are closed — prevents corrupting Hyperp settlement math.
                if !zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }
                {
                    let engine = zc::engine_ref(&data)?;
                    if engine.num_used_accounts != 0 {
                        return Err(ProgramError::InvalidAccountData);
                    }
                }

                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                if min_withdraw_base == 0 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                if max_withdraw_bps == 0 || max_withdraw_bps > 10_000 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }

                let mut config = state::read_config(&data);
                if config.unit_scale != 0 && min_withdraw_base % (config.unit_scale as u64) != 0 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }

                let packed = pack_ins_withdraw_meta(
                    max_withdraw_bps,
                    crate::INS_WITHDRAW_LAST_SLOT_NONE,
                )
                    .ok_or(PercolatorError::InvalidConfigParam)?;

                // Reuse these fields in resolved mode for policy state.
                config.oracle_authority = authority.to_bytes();
                config.last_effective_price_e6 = min_withdraw_base;
                config.oracle_price_cap_e2bps = cooldown_slots;
                config.authority_timestamp = packed;
                state::write_config(&mut data, &config);
                // Set explicit flag so WithdrawInsuranceLimited can distinguish
                // real policy from oracle timestamp bit patterns.
                state::set_policy_configured(&mut data);
            }

            Instruction::WithdrawInsuranceLimited { amount } => {
                // Limited insurance withdraw (configured authority + min/max/cooldown checks)
                accounts::expect_len(accounts, 7)?;
                let a_authority = &accounts[0];
                let a_slab = &accounts[1];
                let a_authority_ata = &accounts[2];
                let a_vault = &accounts[3];
                let a_token = &accounts[4];
                let a_vault_pda = &accounts[5];
                let a_clock = &accounts[6];

                accounts::expect_signer(a_authority)?;
                accounts::expect_writable(a_slab)?;
                verify_token_program(a_token)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let resolved = zc::engine_ref(&data)?.is_resolved();
                let header = state::read_header(&data);
                let mut config = state::read_config(&data);

                // If immutable insurance_withdraw_max_bps == 0, live-market
                // withdrawals are disabled. Only resolved markets can withdraw.
                if config.insurance_withdraw_max_bps == 0 && !resolved {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                let clock = Clock::from_account_info(a_clock)?;

                // Use explicit flag to determine if SetInsuranceWithdrawPolicy was called.
                // Previously inferred from authority_timestamp bit patterns, which an
                // oracle authority could forge via crafted PushOraclePrice timestamps.
                let configured = state::is_policy_configured(&data);
                // Defensive: configured flag should only be set on resolved markets
                // (SetInsuranceWithdrawPolicy is gated on is_resolved). If this
                // invariant is ever broken, reject rather than use repurposed fields.
                if configured && !resolved {
                    return Err(ProgramError::InvalidAccountData);
                }
                let (stored_bps, stored_last_slot) = if configured {
                    unpack_ins_withdraw_meta(config.authority_timestamp)
                } else {
                    (0u16, crate::INS_WITHDRAW_LAST_SLOT_NONE)
                };
                let policy_authority = if configured {
                    config.oracle_authority
                } else {
                    header.admin
                };
                let policy_min_base = if configured {
                    config.last_effective_price_e6
                } else {
                    // Default floor should represent at least one withdrawable unit.
                    // On scaled markets (unit_scale > 1), base amounts must be aligned
                    // to unit_scale, so a base-min of 1 would otherwise collapse to 0 units.
                    core::cmp::max(DEFAULT_INSURANCE_WITHDRAW_MIN_BASE, config.unit_scale as u64)
                };
                let policy_max_bps = if configured {
                    stored_bps
                } else if config.insurance_withdraw_max_bps > 0 {
                    // Use immutable config value (live or resolved unconfigured)
                    config.insurance_withdraw_max_bps
                } else {
                    DEFAULT_INSURANCE_WITHDRAW_MAX_BPS
                };
                let policy_cooldown = if configured {
                    config.oracle_price_cap_e2bps
                } else {
                    DEFAULT_INSURANCE_WITHDRAW_COOLDOWN_SLOTS
                };
                let last_withdraw_slot = if configured {
                    stored_last_slot
                } else if config.last_insurance_withdraw_slot > 0 {
                    // Unconfigured: always use dedicated config field (live or resolved)
                    config.last_insurance_withdraw_slot
                } else {
                    crate::INS_WITHDRAW_LAST_SLOT_NONE
                };

                if policy_min_base == 0 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                if policy_authority != a_authority.key.to_bytes() {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }
                if config.unit_scale != 0 && amount % (config.unit_scale as u64) != 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                // On live markets, use config cooldown directly (not max with defaults).
                // On resolved markets, use stricter of policy and config.
                let effective_cooldown = if !resolved && config.insurance_withdraw_cooldown_slots > 0 {
                    config.insurance_withdraw_cooldown_slots
                } else if config.insurance_withdraw_cooldown_slots > 0 {
                    core::cmp::max(policy_cooldown, config.insurance_withdraw_cooldown_slots)
                } else {
                    policy_cooldown
                };
                if last_withdraw_slot != crate::INS_WITHDRAW_LAST_SLOT_NONE
                    && clock.slot < last_withdraw_slot.saturating_add(effective_cooldown)
                {
                    return Err(ProgramError::InvalidAccountData);
                }

                let mint = Pubkey::new_from_array(config.collateral_mint);
                let auth = accounts::derive_vault_authority_with_bump(
                    program_id, a_slab.key, config.vault_authority_bump,
                )?;
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_authority_ata, a_authority.key, &mint)?;
                accounts::expect_key(a_vault_pda, &auth)?;

                let (units_u64, _) = crate::units::base_to_units(amount, config.unit_scale);
                let units_requested = units_u64 as u128;
                if units_requested == 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                let (policy_min_units_u64, _) =
                    crate::units::base_to_units(policy_min_base, config.unit_scale);
                let policy_min_units = policy_min_units_u64 as u128;

                // `resolved` already computed above
                {
                    let engine = zc::engine_mut(&mut data)?;
                    if resolved {
                        // Require all accounts fully closed, not just effective_pos_q==0
                        // (which returns 0 for epoch-mismatched stale positions).
                        if engine.num_used_accounts != 0 {
                            return Err(ProgramError::InvalidAccountData);
                        }
                    }

                    // On live markets, require a recent crank so that latent
                    // losses are reflected in insurance_fund.balance before
                    // allowing withdrawal. Without this, unsettled losses
                    // could make the stored balance overstated.
                    if !resolved {
                        let staleness = clock.slot.saturating_sub(engine.last_crank_slot);
                        if staleness > engine.params.max_crank_staleness_slots {
                            return Err(PercolatorError::OracleStale.into());
                        }
                    }

                    let insurance_units = engine.insurance_fund.balance.get();
                    if insurance_units == 0 {
                        return Ok(());
                    }
                    if units_requested > insurance_units {
                        return Err(PercolatorError::EngineInsufficientBalance.into());
                    }

                    // On live markets, cannot withdraw below insurance_floor
                    if !resolved {
                        let floor = engine.params.insurance_floor.get();
                        let post_balance = insurance_units.saturating_sub(units_requested);
                        if post_balance < floor {
                            return Err(PercolatorError::EngineInsufficientBalance.into());
                        }
                    }

                    // On live markets, policy_max_bps already IS the config value.
                    // On resolved markets, cap to the stricter of policy and config.
                    let effective_max_bps = if resolved && config.insurance_withdraw_max_bps > 0 {
                        core::cmp::min(policy_max_bps, config.insurance_withdraw_max_bps)
                    } else {
                        policy_max_bps
                    };

                    let pct_limited_units =
                        insurance_units.saturating_mul(effective_max_bps as u128) / 10_000u128;
                    let max_allowed_units = core::cmp::max(pct_limited_units, policy_min_units);
                    if units_requested > max_allowed_units {
                        return Err(ProgramError::InvalidInstructionData);
                    }

                    // effective_cooldown already computed and enforced above

                    let req = percolator::U128::new(units_requested);
                    if req > engine.vault {
                        return Err(PercolatorError::EngineInsufficientBalance.into());
                    }
                    engine.insurance_fund.balance = engine.insurance_fund.balance - req;
                    engine.vault = engine.vault - req;
                }

                // Persist cooldown slot.
                if configured {
                    // Configured policy: pack slot into authority_timestamp
                    let packed = pack_ins_withdraw_meta(policy_max_bps, clock.slot)
                        .ok_or(PercolatorError::EngineOverflow)?;
                    config.oracle_authority = policy_authority;
                    config.last_effective_price_e6 = policy_min_base;
                    config.oracle_price_cap_e2bps = policy_cooldown;
                    config.authority_timestamp = packed;
                } else {
                    // Unconfigured (default): use dedicated field for cooldown
                    config.last_insurance_withdraw_slot = clock.slot;
                }
                state::write_config(&mut data, &config);

                let seed1: &[u8] = b"vault";
                let seed2: &[u8] = a_slab.key.as_ref();
                let bump_arr: [u8; 1] = [config.vault_authority_bump];
                let seed3: &[u8] = &bump_arr;
                let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
                let signer_seeds: [&[&[u8]]; 1] = [&seeds];

                collateral::withdraw(
                    a_token,
                    a_vault,
                    a_authority_ata,
                    a_vault_pda,
                    amount,
                    &signer_seeds,
                )?;
            }

            Instruction::AdminForceCloseAccount { user_idx } => {
                // Admin force-close an abandoned account after market resolution.
                // Settles PnL (with haircut for positive), forgives fee debt,
                // then delegates to engine.close_account_not_atomic() for the rest.
                accounts::expect_len(accounts, 8)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];
                let a_vault = &accounts[2];
                let a_owner_ata = &accounts[3];
                let a_pda = &accounts[4];
                let a_token = &accounts[5];
                let _a_oracle = &accounts[7];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;
                verify_token_program(a_token)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                // Must be resolved
                if !zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }

                let config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let auth = accounts::derive_vault_authority_with_bump(
                    program_id, a_slab.key, config.vault_authority_bump,
                )?;
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                accounts::expect_key(a_pda, &auth)?;

                let clock = Clock::from_account_info(&accounts[6])?;
                let engine = zc::engine_mut(&mut data)?;
                let (price, _) = engine.resolved_context();
                if price == 0 {
                    return Err(ProgramError::InvalidAccountData);
                }

                check_idx(engine, user_idx)?;

                let owner_pubkey = Pubkey::new_from_array(engine.accounts[user_idx as usize].owner);

                let amt_units = match engine.force_close_resolved_not_atomic(user_idx, engine.resolved_context().1)
                    .map_err(map_risk_error)?
                {
                    percolator::ResolvedCloseResult::Deferred => return Ok(()),
                    percolator::ResolvedCloseResult::Closed(payout) => payout,
                };

                let amt_units_u64: u64 = amt_units
                    .try_into()
                    .map_err(|_| PercolatorError::EngineOverflow)?;

                // Only verify owner ATA when there's a nonzero payout.
                if amt_units_u64 > 0 {
                    verify_token_account(a_owner_ata, &owner_pubkey, &mint)?;
                }

                // Remove from risk buffer (drop engine first)
                drop(engine);
                {
                    let mut buf = state::read_risk_buffer(&data);
                    buf.remove(user_idx);
                    state::write_risk_buffer(&mut data, &buf);
                }

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
                    a_owner_ata,
                    a_pda,
                    base_to_pay,
                    &signer_seeds,
                )?;
            }

            Instruction::QueryLpFees { lp_idx } => {
                // §2.2: Read-only query of LP cumulative fees. No state mutation.
                accounts::expect_len(accounts, 1)?;
                let a_slab = &accounts[0];

                let data = a_slab.try_borrow_data()?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let engine = zc::engine_ref(&data)?;
                check_idx(engine, lp_idx)?;
                if !engine.accounts[lp_idx as usize].is_lp() {
                    return Err(PercolatorError::EngineNotAnLPAccount.into());
                }

                let fees = 0u64;
                solana_program::program::set_return_data(&fees.to_le_bytes());
            }

            Instruction::ReclaimEmptyAccount { user_idx } => {
                // Permissionless account reclamation (spec §2.6, §10.7).
                // Recycles flat/dust accounts without touching side state.
                accounts::expect_len(accounts, 2)?;
                let a_slab = &accounts[0];
                let _a_clock = &accounts[1];
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                // Block on resolved markets — unsettled PnL from resolution
                // may not yet be reflected in capital. Reclaiming before
                // touch_account_full would forfeit claimable value.
                if zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }

                let clock = Clock::from_account_info(_a_clock)?;
                let engine = zc::engine_mut(&mut data)?;
                engine.reclaim_empty_account_not_atomic(user_idx, clock.slot)
                    .map_err(map_risk_error)?;
                // Per §10.7: MUST NOT call accrue_market_to, MUST NOT mutate side state.
            }

            Instruction::SettleAccount { user_idx } => {
                // Standalone account settlement (§10.2). Permissionless.
                accounts::expect_len(accounts, 3)?;
                let a_slab = &accounts[0];
                let a_clock = &accounts[1];
                let a_oracle = &accounts[2];
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                if zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }

                let mut config = state::read_config(&data);
                let clock = Clock::from_account_info(a_clock)?;

                let is_hyperp = oracle::is_hyperp_mode(&config);
                let price = if is_hyperp {
                    let eng = zc::engine_ref(&data)?;
                    let last_slot = eng.current_slot;
                    oracle::get_engine_oracle_price_e6(
                        last_slot, clock.slot, clock.unix_timestamp,
                        &mut config, a_oracle,
                    )?
                } else {
                    read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot)?
                };
                state::write_config(&mut data, &config);

                let engine = zc::engine_mut(&mut data)?;
                let h_lock = engine.params.h_min;
                engine.settle_account_not_atomic(user_idx, price, clock.slot,
                    compute_current_funding_rate_e9(&config),
                    h_lock)
                    .map_err(map_risk_error)?;
            }

            Instruction::DepositFeeCredits { user_idx, amount } => {
                // Direct fee-debt repayment (§10.3.1). Owner only.
                // SECURITY: Read fee debt BEFORE the SPL transfer to reject
                // overpayment. Without this, excess tokens become stranded
                // vault surplus with no withdrawal path for the user.
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

                // Phase 1: Read fee debt and validate (immutable borrow)
                // Also verify vault BEFORE the SPL transfer.
                let (unit_scale, debt_units) = {
                    let data = a_slab.try_borrow_data()?;
                    slab_guard(program_id, a_slab, &data)?;
                    require_initialized(&data)?;
                    if zc::engine_ref(&data)?.is_resolved() {
                        return Err(ProgramError::InvalidAccountData);
                    }
                    let cfg = state::read_config(&data);
                    let mint = Pubkey::new_from_array(cfg.collateral_mint);
                    let auth = accounts::derive_vault_authority_with_bump(
                        program_id, a_slab.key, cfg.vault_authority_bump,
                    )?;
                    verify_vault(a_vault, &auth, &mint,
                        &Pubkey::new_from_array(cfg.vault_pubkey))?;
                    verify_token_account(a_user_ata, a_user.key, &mint)?;
                    let engine = zc::engine_ref(&data)?;
                    check_idx(engine, user_idx)?;
                    let owner = engine.accounts[user_idx as usize].owner;
                    if !crate::verify::owner_ok(owner, a_user.key.to_bytes()) {
                        return Err(PercolatorError::EngineUnauthorized.into());
                    }
                    let fc = engine.accounts[user_idx as usize].fee_credits.get();
                    let debt = if fc < 0 { fc.unsigned_abs() } else { 0u128 };
                    (cfg.unit_scale, debt)
                };
                // data (Ref) dropped here — releases immutable borrow

                // Phase 2: Reject zero, misaligned, or overpayment
                let (units, dust) = crate::units::base_to_units(amount, unit_scale);
                if units == 0 || dust != 0 {
                    return Err(ProgramError::InvalidArgument);
                }
                if (units as u128) > debt_units {
                    return Err(ProgramError::InvalidArgument);
                }

                // Phase 3: SPL transfer (only after validation)
                collateral::deposit(a_token, a_user_ata, a_vault, a_user, amount)?;

                // Phase 4: Engine deposit_fee_credits (mutable borrow)
                // Vault already verified in Phase 1.
                let mut data = state::slab_data_mut(a_slab)?;
                let config = state::read_config(&data);
                let clock = Clock::from_account_info(a_clock)?;
                let (units2, _dust) = crate::units::base_to_units(amount, config.unit_scale);
                // dust is always 0 here — rejected by `dust != 0` check in Phase 2.

                let engine = zc::engine_mut(&mut data)?;
                engine.deposit_fee_credits(user_idx, units2 as u128, clock.slot)
                    .map_err(map_risk_error)?;
            }

            Instruction::ConvertReleasedPnl { user_idx, amount } => {
                // Voluntary PnL conversion (§10.4.1). Owner only.
                accounts::expect_len(accounts, 4)?;
                let a_user = &accounts[0];
                let a_slab = &accounts[1];
                let a_clock = &accounts[2];
                let a_oracle = &accounts[3];

                accounts::expect_signer(a_user)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                if zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }

                let mut config = state::read_config(&data);
                let clock = Clock::from_account_info(a_clock)?;

                let is_hyperp = oracle::is_hyperp_mode(&config);
                let price = if is_hyperp {
                    let eng = zc::engine_ref(&data)?;
                    let last_slot = eng.current_slot;
                    oracle::get_engine_oracle_price_e6(
                        last_slot, clock.slot, clock.unix_timestamp,
                        &mut config, a_oracle,
                    )?
                } else {
                    read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot)?
                };
                state::write_config(&mut data, &config);

                let engine = zc::engine_mut(&mut data)?;
                check_idx(engine, user_idx)?;
                let owner = engine.accounts[user_idx as usize].owner;
                if !crate::verify::owner_ok(owner, a_user.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                // Reject misaligned amounts — silent truncation could lose value
                let (units, dust) = crate::units::base_to_units(amount, config.unit_scale);
                if dust != 0 {
                    return Err(ProgramError::InvalidArgument);
                }
                let h_lock = engine.params.h_min;
                engine.convert_released_pnl_not_atomic(user_idx, units as u128, price, clock.slot,
                    compute_current_funding_rate_e9(&config),
                    h_lock)
                    .map_err(map_risk_error)?;
            }

            Instruction::ResolvePermissionless => {
                // Permissionless resolution when oracle is actually dead.
                // Anyone can call. Requires oracle account to prove staleness.
                accounts::expect_len(accounts, 3)?;
                let a_slab = &accounts[0];
                let a_clock = &accounts[1];
                let a_oracle = &accounts[2];

                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                if zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }

                let mut config = state::read_config(&data);

                if config.permissionless_resolve_stale_slots == 0 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }

                let clock = Clock::from_account_info(a_clock)?;

                // Verify oracle is actually stale RIGHT NOW by trying to read it.
                // Only OracleStale proves the oracle is dead. Other errors
                // (wrong account, bad data) don't prove staleness — they could
                // be an attacker passing garbage to fake oracle death.
                let is_hyperp = oracle::is_hyperp_mode(&config);
                if !is_hyperp {
                    let oracle_result = oracle::read_engine_price_e6(
                        a_oracle, &config.index_feed_id,
                        clock.unix_timestamp, config.max_staleness_secs,
                        config.conf_filter_bps, config.invert, config.unit_scale,
                    );
                    match oracle_result {
                        Ok(_) => return Err(ProgramError::InvalidAccountData), // live
                        Err(e) => {
                            let stale_err: ProgramError = PercolatorError::OracleStale.into();
                            if e != stale_err {
                                return Err(e); // wrong account / bad data — propagate
                            }
                            // OracleStale = oracle is actually dead → proceed
                        }
                    }
                } else {
                    // Hyperp: check mark staleness (last trade or push)
                    let last_update = core::cmp::max(
                        config.mark_ewma_last_slot,
                        config.last_mark_push_slot as u64,
                    );
                    let staleness = clock.slot.saturating_sub(last_update);
                    if staleness < config.permissionless_resolve_stale_slots {
                        return Err(PercolatorError::OracleStale.into());
                    }
                }

                // Block if an oracle authority is configured AND has pushed recently.
                // If the authority has never pushed (timestamp=0) or their last push
                // is stale, the authority is effectively dead and permissionless
                // resolution should proceed. This prevents the deadlock where:
                // authority set + external oracle dead = no resolution path.
                if config.oracle_authority != [0u8; 32] && config.authority_timestamp > 0 {
                    let authority_age_secs = clock.unix_timestamp
                        .saturating_sub(config.authority_timestamp);
                    // Authority is "fresh" if push happened within max_staleness_secs
                    // (the same staleness window as the external oracle feed).
                    if authority_age_secs >= 0
                        && (authority_age_secs as u64) < config.max_staleness_secs
                    {
                        return Err(ProgramError::InvalidAccountData);
                    }
                }

                // Require oracle/mark has been dead for the configured delay.
                // Non-Hyperp: use dedicated last_good_oracle_slot, stamped on every
                //   successful read_price_clamped across all instruction paths.
                // Hyperp: use max(mark_ewma_last_slot, last_mark_push_slot) — the
                //   same signal used for the mark staleness check above, so both
                //   checks use consistent liveness information.
                {
                    let reference_slot = if !is_hyperp {
                        config.last_good_oracle_slot
                    } else {
                        core::cmp::max(
                            config.mark_ewma_last_slot,
                            config.last_mark_push_slot as u64,
                        )
                    };
                    let oracle_dead_duration = clock.slot.saturating_sub(reference_slot);
                    if oracle_dead_duration < config.permissionless_resolve_stale_slots {
                        return Err(PercolatorError::OracleStale.into());
                    }
                }

                // Flush Hyperp index toward mark before resolution.
                if is_hyperp {
                    let mark = if config.mark_ewma_e6 > 0 {
                        config.mark_ewma_e6
                    } else {
                        config.authority_price_e6
                    };
                    let prev_index = config.last_effective_price_e6;
                    if mark > 0 && prev_index > 0 {
                        let last_idx_slot = config.last_hyperp_index_slot;
                        let dt = clock.slot.saturating_sub(last_idx_slot);
                        let new_index = oracle::clamp_toward_with_dt(
                            prev_index.max(1), mark,
                            config.oracle_price_cap_e2bps, dt,
                        );
                        config.last_effective_price_e6 = new_index;
                        config.last_hyperp_index_slot = clock.slot;
                    }
                    state::write_config(&mut data, &config);
                }

                // Determine canonical settlement price.
                // Hyperp: use mark EWMA (same as ResolveMarket Hyperp path).
                // Non-Hyperp: use engine.last_oracle_price — but ONLY if the engine
                // has seen a real oracle accrual. If oracle never initialized, allow
                // resolution only if market is empty (no positions to mis-settle).
                // This prevents a liveness hole where deposits arrive before first
                // crank, oracle dies, and permissionless resolve is blocked.
                let settlement_price = if is_hyperp {
                    let mark = config.mark_ewma_e6;
                    if mark > 0 { mark } else { config.authority_price_e6 }
                } else {
                    let engine = zc::engine_ref(&data)?;
                    if engine.last_oracle_price <= 1 {
                        // Oracle never initialized — only safe if no open positions.
                        // Accounts may exist (deposits-only) but if OI is zero,
                        // there is no position-dependent settlement risk and the
                        // sentinel price is harmless.
                        if engine.oi_eff_long_q > 0 || engine.oi_eff_short_q > 0 {
                            return Err(PercolatorError::OracleInvalid.into());
                        }
                    }
                    let p = engine.last_oracle_price;
                    if p == 0 {
                        return Err(PercolatorError::OracleInvalid.into());
                    }
                    p
                };

                // Call engine resolve_market with canonical settlement price.
                let engine = zc::engine_mut(&mut data)?;
                engine.resolve_market(settlement_price, clock.slot)
                    .map_err(map_risk_error)?;

                config.authority_price_e6 = settlement_price;
                state::write_config(&mut data, &config);
            }

            Instruction::ForceCloseResolved { user_idx } => {
                // Permissionless force-close for resolved markets.
                // Mirrors AdminForceCloseAccount but requires delay and no admin.
                accounts::expect_len(accounts, 7)?;
                let a_slab = &accounts[0];
                let a_vault = &accounts[1];
                let a_owner_ata = &accounts[2];
                let a_pda = &accounts[3];
                let a_token = &accounts[4];
                let a_clock = &accounts[5];
                // accounts[6] = oracle (unused but passed for compatibility)

                accounts::expect_writable(a_slab)?;
                verify_token_program(a_token)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                let resolved_slot = {
                    let eng = zc::engine_ref(&data)?;
                    if !eng.is_resolved() {
                        return Err(ProgramError::InvalidAccountData);
                    }
                    eng.resolved_context().1
                };

                let config = state::read_config(&data);
                if config.force_close_delay_slots == 0 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                let clock = Clock::from_account_info(a_clock)?;
                if clock.slot < resolved_slot
                    .saturating_add(config.force_close_delay_slots)
                {
                    return Err(ProgramError::InvalidAccountData);
                }

                let mint = Pubkey::new_from_array(config.collateral_mint);
                let auth = accounts::derive_vault_authority_with_bump(
                    program_id, a_slab.key, config.vault_authority_bump,
                )?;
                verify_vault(
                    a_vault, &auth, &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                accounts::expect_key(a_pda, &auth)?;

                let engine = zc::engine_mut(&mut data)?;
                let (price, _) = engine.resolved_context();
                if price == 0 {
                    return Err(ProgramError::InvalidAccountData);
                }
                check_idx(engine, user_idx)?;

                let owner_pubkey = Pubkey::new_from_array(
                    engine.accounts[user_idx as usize].owner,
                );

                let amt_units = match engine.force_close_resolved_not_atomic(user_idx, engine.resolved_context().1)
                    .map_err(map_risk_error)?
                {
                    percolator::ResolvedCloseResult::Deferred => return Ok(()),
                    percolator::ResolvedCloseResult::Closed(payout) => payout,
                };

                // Only verify owner ATA when there's a nonzero payout.
                if amt_units > 0 {
                    verify_token_account(a_owner_ata, &owner_pubkey, &mint)?;
                }

                let amt_units_u64: u64 = amt_units
                    .try_into()
                    .map_err(|_| PercolatorError::EngineOverflow)?;

                // Remove from risk buffer before withdraw
                drop(engine);
                {
                    let mut buf = state::read_risk_buffer(&data);
                    buf.remove(user_idx);
                    state::write_risk_buffer(&mut data, &buf);
                }

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
                    a_token, a_vault, a_owner_ata, a_pda,
                    base_to_pay, &signer_seeds,
                )?;
            }
        }
        Ok(())
    }
}

// 10. mod entrypoint
#[cfg(not(feature = "no-entrypoint"))]
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
        RiskEngine, RiskError, RiskParams,
    };
    pub use crate::processor::{
        MatchingEngine, NoOpMatcher, TradeExecution,
    };
}
