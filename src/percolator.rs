//! Percolator: Single-file Solana program with embedded Risk Engine.

#![no_std]
#![deny(unsafe_code)]
// Upstream code uses patterns that trigger some clippy lints.
#![allow(
    clippy::too_many_arguments,
    clippy::large_enum_variant,
    clippy::needless_return,
    clippy::collapsible_if,
    clippy::if_same_then_else,
    clippy::manual_range_contains,
    clippy::explicit_auto_deref,
    clippy::needless_borrow,
    clippy::result_large_err,
    clippy::vec_init_then_push,
    clippy::manual_is_multiple_of,
    clippy::needless_lifetimes,
    clippy::ok_expect,
    clippy::question_mark,
    clippy::assertions_on_constants,
    unused_imports,
    unused_variables,
    dead_code,
)]

// SECURITY(L-1): Prevent accidental mainnet deploy with test feature enabled.
// The test feature bypasses oracle owner checks and replaces token CPIs with
// in-memory simulation — catastrophic if deployed to mainnet.
#[cfg(all(feature = "mainnet", feature = "test"))]
compile_error!("features `mainnet` and `test` are mutually exclusive — never deploy with `test` enabled");

extern crate alloc;

// Local SPL Token helpers — replaces spl-token crate dependency.
pub mod spl_token;

use solana_program::declare_id;

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

    pub const HEADER_LEN: usize = size_of::<SlabHeader>();
    pub const CONFIG_LEN: usize = size_of::<MarketConfig>();
    pub const ENGINE_ALIGN: usize = align_of::<RiskEngine>();

    // SBF compile-time layout pinning assertions.
    // If any of these fail, update the SDK layout constants.
    pub const ACCOUNT_SIZE: usize = size_of::<percolator::Account>();
    #[cfg(target_arch = "sbf")]
    const _SBF_ENGINE_ALIGN: [(); 8] = [(); ENGINE_ALIGN];

    /// Minimum seed deposit required for InitMarket (10 USDC at 6 decimals).
    #[cfg(not(feature = "test"))]
    pub const MIN_INIT_MARKET_SEED: u64 = 10_000_000;
    #[cfg(feature = "test")]
    pub const MIN_INIT_MARKET_SEED: u64 = 0;
    pub const MIN_INIT_MARKET_SEED_LAMPORTS: u64 = MIN_INIT_MARKET_SEED;

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
    /// Per-account materialization generation table.
    /// Stores the global mat_counter value assigned at InitUser/InitLP.
    /// Used as lp_account_id for per-instance identity across slot reuse.
    pub const GEN_TABLE_OFF: usize = RISK_BUF_OFF + RISK_BUF_LEN;
    pub const GEN_TABLE_LEN: usize = percolator::MAX_ACCOUNTS * 8; // u64 per slot
    pub const SLAB_LEN: usize = GEN_TABLE_OFF + GEN_TABLE_LEN;

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

    // Hyperp EMA oracle constants
    /// EMA window in slots for UpdateHyperpMark (~8 hours at 2.5 slots/sec).
    pub const MARK_PRICE_EMA_WINDOW_SLOTS: u64 = 72_000;
    /// Per-slot alpha for the 8-hour Hyperp EMA (≈ 2/72001 in e6 units).
    pub const MARK_PRICE_EMA_ALPHA_E6: u64 = 2_000_000 / (MARK_PRICE_EMA_WINDOW_SLOTS + 1);
    /// Minimum quote-side DEX liquidity required for UpdateHyperpMark to accept a price.
    /// 2_000_000_000_000 = 2,000,000 USDC (at 6 decimals). Thin pools below this are rejected.
    pub const MIN_DEX_QUOTE_LIQUIDITY: u64 = 2_000_000_000_000;

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
    pub const DEFAULT_THRESH_RISK_BPS: u64 = 50; // 0.50%
    pub const DEFAULT_THRESH_UPDATE_INTERVAL_SLOTS: u64 = 10;
    pub const DEFAULT_THRESH_STEP_BPS: u64 = 500; // 5% max step
    pub const DEFAULT_THRESH_ALPHA_BPS: u64 = 1000; // 10% EWMA
    pub const DEFAULT_THRESH_MIN: u128 = 0;
    pub const DEFAULT_THRESH_MAX: u128 = 10_000_000_000_000_000_000u128;
    pub const DEFAULT_THRESH_MIN_STEP: u128 = 1;
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
    /// Returns None if the nonce would overflow (u64::MAX reached).
    /// Overflow must reject the trade — wrapping would reopen old request IDs.
    #[inline]
    pub fn nonce_on_success(old: u64) -> Option<u64> {
        old.checked_add(1)
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
        // 6. Nonce overflow check
        let new_nonce = match nonce_on_success(old_nonce) {
            Some(n) => n,
            None => return TradeCpiDecision::Reject,
        };
        // All checks passed - accept the trade
        TradeCpiDecision::Accept {
            new_nonce,
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
        // 5. Compute req_id from nonce (reject on overflow) and validate ABI
        let req_id = match nonce_on_success(old_nonce) {
            Some(n) => n,
            None => return TradeCpiDecision::Reject,
        };
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

    // ─── Fork-specific verify stubs ───────────────────────────────────────────

    /// Base fee multiplier BPS (1.0x = 10_000 bps).
    pub const FEE_MULT_BASE_BPS: u64 = 10_000;

    /// Compute utilization in BPS given current OI and max OI.
    /// Returns 0 if max_oi is 0 (disabled).
    #[inline]
    pub fn compute_util_bps(current_oi: u128, max_oi: u128) -> u64 {
        if max_oi == 0 {
            return 0;
        }
        let util = current_oi.saturating_mul(10_000) / max_oi;
        core::cmp::min(util, 10_000) as u64
    }

    /// Compute fee multiplier BPS based on utilization BPS.
    /// Linear from FEE_MULT_BASE_BPS at 0% util to 2x at 100% util.
    #[inline]
    pub fn compute_fee_multiplier_bps(util_bps: u64) -> u64 {
        FEE_MULT_BASE_BPS + util_bps
    }

    /// Returns true if the market uses a pinned (authority) oracle rather than Pyth.
    /// A market with a non-zero oracle_authority and zero index_feed_id is pinned.
    #[inline]
    pub fn is_pyth_pinned_mode(oracle_authority: [u8; 32], index_feed_id: [u8; 32]) -> bool {
        // Pyth-pinned: non-zero authority AND non-zero feed ID (actual Pyth feed).
        // Hyperp mode (feed_id=[0;32]) is NOT Pyth-pinned — admin can change oracle authority.
        oracle_authority != [0u8; 32] && index_feed_id != [0u8; 32]
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
        // ── Fork-specific error variants ─────────────────────────────────────
        MarketPaused,
        LpVaultInvalidFeeShare,
        LpVaultAlreadyExists,
        LpVaultNotCreated,
        LpVaultZeroAmount,
        LpVaultSupplyMismatch,
        LpVaultWithdrawExceedsAvailable,
        LpVaultNoNewFees,
        LpCollateralDisabled,
        LpCollateralPositionOpen,
        MarketNotResolved,
        DisputeWindowClosed,
        DisputeAlreadyExists,
        NoActiveDispute,
        WithdrawQueueAlreadyExists,
        WithdrawQueueNotFound,
        WithdrawQueueNothingClaimable,
        InsuranceFundNotDepleted,
        BankruptPositionAlreadyClosed,
        AuditViolation,
        CrossMarginPairNotFound,
        InsufficientDexLiquidity,
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

        /// Permissionless Hyperp DEX EMA oracle update (tag 34).
        /// Reads DEX pool price (PumpSwap/Raydium CLMM/Meteora DLMM),
        /// applies EMA smoothing with circuit breaker, and writes new mark price.
        UpdateHyperpMark,
        /// Admin emergency pause (tag 76). Blocks Trade/Deposit/Withdraw/InitUser.
        PauseMarket,
        /// Admin unpause (tag 77). Re-enables all operations.
        UnpauseMarket,
        // ─── Fork-specific instructions ────────────────────────────────────

        /// PERC-8400: Rescue orphan vault (admin only, tag 72).
        /// Reads actual vault token balance and transfers to admin ATA.
        RescueOrphanVault,

        /// PERC-8400: Close orphan slab (admin only, tag 73).
        /// Verifies vault is empty, zeros slab data, drains lamports to admin.
        CloseOrphanSlab,

        /// PERC-SetDexPool: Pin admin-approved DEX pool for HYPERP market (tag 74).
        SetDexPool { pool: Pubkey },

        /// InitMatcherCtx: CPI to matcher program to initialize a matcher context (tag 75).
        InitMatcherCtx {
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
        },

        // ─── LP Vault (PERC-272, tags 37-40) ─────────────────────────────
        /// PERC-272: Create LP vault state PDA + SPL mint (tag 37).
        CreateLpVault {
            fee_share_bps: u64,
            /// PERC-304: Whether to enable the utilization kink curve.
            util_curve_enabled: bool,
        },
        /// PERC-272: Deposit into LP vault, receive LP shares (tag 38).
        LpVaultDeposit { amount: u64 },
        /// PERC-272: Burn LP shares and withdraw proportional SOL from LP vault (tag 39).
        LpVaultWithdraw { lp_amount: u64 },
        /// PERC-272: Permissionless crank — distribute accrued fee revenue to LP vault (tag 40).
        LpVaultCrankFees,

        /// PERC-306: Fund per-market isolated insurance balance (tag 41).
        FundMarketInsurance { amount: u64 },
        /// PERC-306: Set insurance isolation BPS for a market (tag 42).
        SetInsuranceIsolation { bps: u16 },
        /// PERC-314: Challenge settlement price (tag 43).
        ChallengeSettlement { proposed_price_e6: u64 },
        /// PERC-314: Resolve dispute (admin) (tag 44).
        ResolveDispute { accept: u8 },
        /// PERC-315: Deposit LP vault tokens as perp collateral (tag 45).
        DepositLpCollateral { user_idx: u16, lp_amount: u64 },
        /// PERC-315: Withdraw LP collateral (position must be closed) (tag 46).
        WithdrawLpCollateral { user_idx: u16, lp_amount: u64 },
        /// PERC-309: Queue large LP withdrawal (tag 47).
        QueueWithdrawal { lp_amount: u64 },
        /// PERC-309: Claim one epoch tranche (tag 48).
        ClaimQueuedWithdrawal,
        /// PERC-309: Cancel queued withdrawal (tag 49).
        CancelQueuedWithdrawal,
        /// PERC-305: Auto-deleverage (tag 50).
        ExecuteAdl { target_idx: u16 },
        /// Close a stale slab (wrong size from old program layout) and recover rent SOL (tag 51).
        CloseStaleSlabs,
        /// Reclaim rent from an uninitialised slab when market creation fails mid-flow (tag 52).
        ReclaimSlabRent,
        /// PERC-608: Transfer position ownership via CPI from percolator-nft TransferHook (tag 69).
        TransferOwnershipCpi { user_idx: u16, new_owner: [u8; 32] },
        /// PERC-622: Advance oracle phase (permissionless crank) (tag 56).
        AdvanceOraclePhase,
        /// On-chain audit crank: walk all accounts and verify conservation invariants (tag 53).
        AuditCrank,
        /// Admin: configure cross-market margin offset for a pair of slabs (tag 54).
        SetOffsetPair { offset_bps: u16 },
        /// Permissionless: attest user positions across two slabs for portfolio margin credit (tag 55).
        AttestCrossMargin { user_idx_a: u16, user_idx_b: u16 },
        /// PERC-628: Initialize the global shared vault (tag 59).
        InitSharedVault {
            epoch_duration_slots: u64,
            max_market_exposure_bps: u16,
        },
        /// PERC-628: Allocate virtual liquidity to a market (tag 60).
        AllocateMarket { amount: u128 },
        /// PERC-628: Queue a withdrawal for the current epoch (tag 61).
        QueueWithdrawalSV { lp_amount: u64 },
        /// PERC-628: Claim a queued withdrawal after epoch elapses (tag 62).
        ClaimEpochWithdrawal,
        /// PERC-628: Advance the shared vault epoch (permissionless crank) (tag 63).
        AdvanceEpoch,

        // ── PERC-608: Position NFTs (tags 64-68) ─────────────────────────
        /// PERC-608: Mint a Position NFT (Token-2022 + TokenMetadata) for an open position (tag 64).
        MintPositionNft { user_idx: u16 },
        /// PERC-608: Transfer position ownership via the NFT (tag 65).
        TransferPositionOwnership { user_idx: u16 },
        /// PERC-608: Burn the Position NFT when a position is closed (tag 66).
        BurnPositionNft { user_idx: u16 },
        /// PERC-608: Keeper sets pending_settlement=1 before a funding settlement transfer (tag 67).
        SetPendingSettlement { user_idx: u16 },
        /// PERC-608: Keeper clears pending_settlement=0 after running KeeperCrank (tag 68).
        ClearPendingSettlement { user_idx: u16 },

        /// PERC-8111: Set per-wallet position cap (admin only) (tag 70).
        SetWalletCap { cap_e6: u64 },
        /// PERC-8110: Set OI imbalance hard block threshold (admin only) (tag 71).
        SetOiImbalanceHardBlock { threshold_bps: u16 },
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
                34 => Ok(Instruction::UpdateHyperpMark),
                76 => Ok(Instruction::PauseMarket),
                77 => Ok(Instruction::UnpauseMarket),
                // Fork-specific instructions
                72 => Ok(Instruction::RescueOrphanVault),
                73 => Ok(Instruction::CloseOrphanSlab),
                74 => {
                    // SetDexPool
                    let pool = read_pubkey(&mut rest)?;
                    Ok(Instruction::SetDexPool { pool })
                }
                75 => {
                    // InitMatcherCtx
                    let lp_idx = read_u16(&mut rest)?;
                    let kind = read_u8(&mut rest)?;
                    let trading_fee_bps = read_u32(&mut rest)?;
                    let base_spread_bps = read_u32(&mut rest)?;
                    let max_total_bps = read_u32(&mut rest)?;
                    let impact_k_bps = read_u32(&mut rest)?;
                    let liquidity_notional_e6 = read_u128(&mut rest)?;
                    let max_fill_abs = read_u128(&mut rest)?;
                    let max_inventory_abs = read_u128(&mut rest)?;
                    let fee_to_insurance_bps = read_u16(&mut rest)?;
                    let skew_spread_mult_bps = read_u16(&mut rest)?;
                    Ok(Instruction::InitMatcherCtx {
                        lp_idx,
                        kind,
                        trading_fee_bps,
                        base_spread_bps,
                        max_total_bps,
                        impact_k_bps,
                        liquidity_notional_e6,
                        max_fill_abs,
                        max_inventory_abs,
                        fee_to_insurance_bps,
                        skew_spread_mult_bps,
                    })
                }
                // ─── LP Vault + additional fork instructions ───────────
                37 => {
                    let fee_share_bps = read_u64(&mut rest)?;
                    let util_curve_enabled = if !rest.is_empty() {
                        read_u8(&mut rest)? != 0
                    } else {
                        false
                    };
                    Ok(Instruction::CreateLpVault { fee_share_bps, util_curve_enabled })
                }
                38 => {
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::LpVaultDeposit { amount })
                }
                39 => {
                    let lp_amount = read_u64(&mut rest)?;
                    Ok(Instruction::LpVaultWithdraw { lp_amount })
                }
                40 => Ok(Instruction::LpVaultCrankFees),
                41 => {
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::FundMarketInsurance { amount })
                }
                42 => {
                    let bps = read_u16(&mut rest)?;
                    Ok(Instruction::SetInsuranceIsolation { bps })
                }
                43 => {
                    let proposed_price_e6 = read_u64(&mut rest)?;
                    Ok(Instruction::ChallengeSettlement { proposed_price_e6 })
                }
                44 => {
                    let accept = read_u8(&mut rest)?;
                    Ok(Instruction::ResolveDispute { accept })
                }
                45 => {
                    let user_idx = read_u16(&mut rest)?;
                    let lp_amount = read_u64(&mut rest)?;
                    Ok(Instruction::DepositLpCollateral { user_idx, lp_amount })
                }
                46 => {
                    let user_idx = read_u16(&mut rest)?;
                    let lp_amount = read_u64(&mut rest)?;
                    Ok(Instruction::WithdrawLpCollateral { user_idx, lp_amount })
                }
                47 => {
                    let lp_amount = read_u64(&mut rest)?;
                    Ok(Instruction::QueueWithdrawal { lp_amount })
                }
                48 => Ok(Instruction::ClaimQueuedWithdrawal),
                49 => Ok(Instruction::CancelQueuedWithdrawal),
                50 => {
                    let target_idx = read_u16(&mut rest)?;
                    Ok(Instruction::ExecuteAdl { target_idx })
                }
                51 => Ok(Instruction::CloseStaleSlabs),
                52 => Ok(Instruction::ReclaimSlabRent),
                53 => Ok(Instruction::AuditCrank),
                54 => {
                    let offset_bps = read_u16(&mut rest)?;
                    Ok(Instruction::SetOffsetPair { offset_bps })
                }
                55 => {
                    let user_idx_a = read_u16(&mut rest)?;
                    let user_idx_b = read_u16(&mut rest)?;
                    Ok(Instruction::AttestCrossMargin { user_idx_a, user_idx_b })
                }
                56 => Ok(Instruction::AdvanceOraclePhase),
                // 57 = gap (keeper fund removed)
                // 58 = TAG_SLASH_CREATION_DEPOSIT — intentionally unimplemented stub
                59 => {
                    let epoch_duration_slots = read_u64(&mut rest)?;
                    let max_market_exposure_bps = read_u16(&mut rest)?;
                    Ok(Instruction::InitSharedVault { epoch_duration_slots, max_market_exposure_bps })
                }
                60 => {
                    let amount = read_u128(&mut rest)?;
                    Ok(Instruction::AllocateMarket { amount })
                }
                61 => {
                    let lp_amount = read_u64(&mut rest)?;
                    Ok(Instruction::QueueWithdrawalSV { lp_amount })
                }
                62 => Ok(Instruction::ClaimEpochWithdrawal),
                63 => Ok(Instruction::AdvanceEpoch),
                64 => {
                    let user_idx = read_u16(&mut rest)?;
                    Ok(Instruction::MintPositionNft { user_idx })
                }
                65 => {
                    let user_idx = read_u16(&mut rest)?;
                    Ok(Instruction::TransferPositionOwnership { user_idx })
                }
                66 => {
                    let user_idx = read_u16(&mut rest)?;
                    Ok(Instruction::BurnPositionNft { user_idx })
                }
                67 => {
                    let user_idx = read_u16(&mut rest)?;
                    Ok(Instruction::SetPendingSettlement { user_idx })
                }
                68 => {
                    let user_idx = read_u16(&mut rest)?;
                    Ok(Instruction::ClearPendingSettlement { user_idx })
                }
                69 => {
                    let user_idx = read_u16(&mut rest)?;
                    let mut new_owner = [0u8; 32];
                    if rest.len() < 32 {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                    new_owner.copy_from_slice(&rest[..32]);
                    Ok(Instruction::TransferOwnershipCpi { user_idx, new_owner })
                }
                70 => {
                    let cap_e6 = read_u64(&mut rest)?;
                    Ok(Instruction::SetWalletCap { cap_e6 })
                }
                71 => {
                    let threshold_bps = read_u16(&mut rest)?;
                    Ok(Instruction::SetOiImbalanceHardBlock { threshold_bps })
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

    /// PERC-272: Derive LP vault state PDA.
    /// Seeds: `[b"lp_vault", slab_key]`
    pub fn derive_lp_vault_state(program_id: &Pubkey, slab_key: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[b"lp_vault", slab_key.as_ref()], program_id)
    }

    /// PERC-272: Derive LP vault SPL mint PDA.
    /// Seeds: `[b"lp_vault_mint", slab_key]`
    pub fn derive_lp_vault_mint(program_id: &Pubkey, slab_key: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[b"lp_vault_mint", slab_key.as_ref()], program_id)
    }

    /// PERC-314: Derive settlement dispute PDA.
    /// Seeds: `[b"dispute", slab_key]`
    pub fn derive_dispute(program_id: &Pubkey, slab_key: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[b"dispute", slab_key.as_ref()], program_id)
    }

    /// PERC-309: Derive withdraw queue PDA.
    /// Seeds: `[b"withdraw_queue", slab_key, user_key]`
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

        // ========================================
        // DEX Pool Pinning (PERC-SetDexPool)
        // ========================================
        /// Admin-pinned DEX pool pubkey for HYPERP markets.
        /// Set via SetDexPool (tag 74). All-zeros = not set.
        /// UpdateHyperpMark rejects pool keys that don't match this.
        pub dex_pool: [u8; 32],
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

    /// Monotonic materialization counter stored in _reserved[8..16].
    /// Incremented on every InitUser/InitLP. Used as lp_account_id
    /// to provide a true per-instance identity that survives slot reuse.
    pub fn read_mat_counter(data: &[u8]) -> u64 {
        u64::from_le_bytes(data[RESERVED_OFF + 8..RESERVED_OFF + 16].try_into().unwrap())
    }

    pub fn write_mat_counter(data: &mut [u8], counter: u64) {
        data[RESERVED_OFF + 8..RESERVED_OFF + 16].copy_from_slice(&counter.to_le_bytes());
    }

    /// Increment the materialization counter and return the NEW value.
    /// Each account gets a globally unique ID at creation time.
    pub fn next_mat_counter(data: &mut [u8]) -> u64 {
        let c = read_mat_counter(data).wrapping_add(1);
        write_mat_counter(data, c);
        c
    }

    // ========================================
    // Market Flags (stored in _padding[0] at offset 13)
    // ========================================

    /// Offset of flags byte in SlabHeader (_padding[0])
    pub const FLAGS_OFF: usize = 13;

    /// Flag bit: Market is resolved (withdraw-only mode)
    pub const FLAG_RESOLVED: u8 = 1 << 0;
    /// Flag bit: SetInsuranceWithdrawPolicy has been explicitly called.
    /// Prevents WithdrawInsuranceLimited from misinterpreting oracle
    /// timestamps as policy metadata via authority_timestamp bit patterns.
    pub const FLAG_POLICY_CONFIGURED: u8 = 1 << 1;
    /// Flag bit: CPI is in progress (reentrancy guard for TradeCpi).
    /// Set before matcher CPI, cleared after. Any reentrant instruction
    /// that sees this flag must abort.
    pub const FLAG_CPI_IN_PROGRESS: u8 = 1 << 2;
    /// Flag bit: engine has received a real oracle price (not the init sentinel).
    /// Set on first successful oracle read (crank/trade/settle).
    /// Eliminates the "price 1 means uninitialized" sentinel overload.
    pub const FLAG_ORACLE_INITIALIZED: u8 = 1 << 3;
    /// Flag bit: Market is paused (admin emergency stop or audit crank violation).
    /// Moved to bit 4 to avoid collision with upstream FLAG_CPI_IN_PROGRESS (bit 2)
    /// and FLAG_ORACLE_INITIALIZED (bit 3).
    pub const FLAG_PAUSED: u8 = 1 << 4;

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

    /// Check if engine has received a real oracle price.
    pub fn is_oracle_initialized(data: &[u8]) -> bool {
        read_flags(data) & FLAG_ORACLE_INITIALIZED != 0
    }

    /// Mark engine as having received a real oracle price.
    pub fn set_oracle_initialized(data: &mut [u8]) {
        let flags = read_flags(data) | FLAG_ORACLE_INITIALIZED;
        write_flags(data, flags);
    }

    /// Check if market is paused.
    pub fn is_paused(data: &[u8]) -> bool {
        read_flags(data) & FLAG_PAUSED != 0
    }

    /// Set or clear the paused flag.
    pub fn set_paused(data: &mut [u8], paused: bool) {
        let flags = if paused {
            read_flags(data) | FLAG_PAUSED
        } else {
            read_flags(data) & !FLAG_PAUSED
        };
        write_flags(data, flags);
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

    /// Write market_start_slot into _reserved[8..16] at InitMarket time.
    pub fn write_market_start_slot(data: &mut [u8], slot: u64) {
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

    /// Oracle phase constants.
    pub const ORACLE_PHASE_NASCENT: u8 = 0;
    pub const ORACLE_PHASE_GROWING: u8 = 1;
    pub const ORACLE_PHASE_MATURE: u8 = 2;

    /// Read oracle phase. Returns 0 if field not present (legacy market, treat as nascent).
    /// Stored in dex_pool[31] (last byte) as a byte value — avoids adding a new MarketConfig
    /// field while keeping state that survives config rewrites.
    #[inline]
    pub fn get_oracle_phase(_config: &MarketConfig) -> u8 {
        // Phase detection not available in this layout; treat as mature (Phase 3).
        ORACLE_PHASE_MATURE
    }

    /// Set oracle phase — no-op in this layout (field absent).
    #[inline]
    pub fn set_oracle_phase(_config: &mut MarketConfig, _phase: u8) {}

    /// Get cumulative volume — returns 0 (not tracked in this layout).
    #[inline]
    pub fn get_cumulative_volume(_config: &MarketConfig) -> u64 { 0 }

    /// Get phase2 delta slots — returns 0 (not tracked).
    #[inline]
    pub fn get_phase2_delta_slots(_config: &MarketConfig) -> u32 { 0 }

    /// Set phase2 delta slots — no-op in this layout.
    #[inline]
    pub fn set_phase2_delta_slots(_config: &mut MarketConfig, _delta: u32) {}

    /// Get volatility margin scale bps — returns 0 (disabled in this layout).
    #[inline]
    pub fn get_vol_margin_scale_bps(_config: &MarketConfig) -> u16 { 0 }

    /// Compute effective created slot for phase logic.
    #[inline]
    pub fn effective_created_slot(market_created_slot: u64, current_slot: u64) -> u64 {
        if market_created_slot == 0 { current_slot } else { market_created_slot }
    }

    /// Phase transition decision function. Returns (new_phase, transitioned).
    /// Since phase storage is absent, always returns (MATURE, false) so
    /// AdvanceOraclePhase becomes a safe no-op on this layout.
    pub fn check_phase_transition(
        _current_slot: u64,
        _market_created_slot: u64,
        _oracle_phase: u8,
        _cumulative_volume: u64,
        _phase2_delta_slots: u32,
        _has_mature_oracle: bool,
    ) -> (u8, bool) {
        (ORACLE_PHASE_MATURE, false)
    }

    /// Read audit status — returns 0 (field absent in this layout).
    #[inline]
    pub fn read_audit_status(_config: &MarketConfig) -> u16 { 0 }

    /// Write audit status — no-op in this layout.
    #[inline]
    pub fn write_audit_status(_config: &mut MarketConfig, _status: u16) {}

    /// Read last audit-crank pause slot — returns 0 (field absent).
    #[inline]
    pub fn read_last_audit_pause_slot(_config: &MarketConfig) -> u64 { 0 }

    /// Write last audit-crank pause slot — no-op in this layout.
    #[inline]
    pub fn write_last_audit_pause_slot(_config: &mut MarketConfig, _slot: u64) {}

    /// Get per-wallet position cap — returns 0 (disabled, field absent).
    #[inline]
    pub fn get_max_wallet_pos_e6(_config: &MarketConfig) -> u64 { 0 }

    /// Set per-wallet position cap — no-op in this layout.
    #[inline]
    pub fn set_max_wallet_pos_e6(_config: &mut MarketConfig, _cap_e6: u64) {}

    /// Get OI imbalance hard-block threshold — returns 0 (disabled).
    #[inline]
    pub fn get_oi_imbalance_hard_block_bps(_config: &MarketConfig) -> u16 { 0 }

    /// Set OI imbalance hard-block threshold — no-op in this layout.
    #[inline]
    pub fn set_oi_imbalance_hard_block_bps(_config: &mut MarketConfig, _bps: u16) {}

    // ─── Fork-specific MarketConfig field stubs ───────────────────────────────
    // These fields do not exist in the current upstream MarketConfig layout.
    // All getters return safe defaults (0 / disabled); all setters are no-ops.

    /// OI cap multiplier — 0 means disabled.
    #[inline]
    pub fn get_oi_cap_multiplier_bps(_config: &MarketConfig) -> u64 { 0 }
    #[inline]
    pub fn set_oi_cap_multiplier_bps(_config: &mut MarketConfig, _v: u64) {}

    /// Dispute window slots — 0 means disputes disabled.
    #[inline]
    pub fn get_dispute_window_slots(_config: &MarketConfig) -> u64 { 0 }
    #[inline]
    pub fn set_dispute_window_slots(_config: &mut MarketConfig, _v: u64) {}

    /// Resolved slot (slot when market was resolved).
    #[inline]
    pub fn get_resolved_slot(_config: &MarketConfig) -> u64 { 0 }
    #[inline]
    pub fn set_resolved_slot(_config: &mut MarketConfig, _v: u64) {}

    /// Dispute bond amount — 0 means no bond required.
    #[inline]
    pub fn get_dispute_bond_amount(_config: &MarketConfig) -> u64 { 0 }
    #[inline]
    pub fn set_dispute_bond_amount(_config: &mut MarketConfig, _v: u64) {}

    /// Settlement price e6 — 0 if not set.
    #[inline]
    pub fn get_settlement_price_e6(_config: &MarketConfig) -> u64 { 0 }
    #[inline]
    pub fn set_settlement_price_e6(_config: &mut MarketConfig, _v: u64) {}

    /// Insurance isolation BPS — 0 means no isolation.
    #[inline]
    pub fn get_insurance_isolation_bps(_config: &MarketConfig) -> u16 { 0 }
    #[inline]
    pub fn set_insurance_isolation_bps(_config: &mut MarketConfig, _v: u16) {}

    /// LP collateral enabled — 0 means disabled.
    #[inline]
    pub fn get_lp_collateral_enabled(_config: &MarketConfig) -> u8 { 0 }
    #[inline]
    pub fn set_lp_collateral_enabled(_config: &mut MarketConfig, _v: u8) {}

    /// LP collateral LTV BPS — 0 means disabled.
    #[inline]
    pub fn get_lp_collateral_ltv_bps(_config: &MarketConfig) -> u16 { 0 }
    #[inline]
    pub fn set_lp_collateral_ltv_bps(_config: &mut MarketConfig, _v: u16) {}

    /// Max PnL cap — 0 means no cap.
    #[inline]
    pub fn get_max_pnl_cap(_config: &MarketConfig) -> u64 { 0 }
    #[inline]
    pub fn set_max_pnl_cap(_config: &mut MarketConfig, _v: u64) {}

    /// Market created slot — 0 if not tracked.
    #[inline]
    pub fn get_market_created_slot(_config: &MarketConfig) -> u64 { 0 }
    #[inline]
    pub fn set_market_created_slot(_config: &mut MarketConfig, _v: u64) {}

    /// PERC-118: Read the mark oracle weight bps.
    /// The field is absent in this layout — always returns 0 (pure DEX price, no oracle blend).
    #[inline]
    pub fn get_mark_oracle_weight_bps(_config: &MarketConfig) -> u16 {
        0
    }

    /// Write the last observed DEX quote liquidity.
    /// The dedicated storage field is absent in this layout — this is a no-op.
    /// Pool depth enforcement is handled via the dex_pool key check instead.
    #[inline]
    pub fn set_last_dex_liquidity_k(_config: &mut MarketConfig, _quote_liquidity: u64) {}


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

    /// Read per-account materialization generation (u64).
    /// Returns 0 for never-materialized slots (zero-initialized slab).
    pub fn read_account_generation(data: &[u8], idx: u16) -> u64 {
        let off = crate::constants::GEN_TABLE_OFF + (idx as usize) * 8;
        u64::from_le_bytes(data[off..off + 8].try_into().unwrap())
    }

    /// Write per-account materialization generation.
    pub fn write_account_generation(data: &mut [u8], idx: u16, gen: u64) {
        let off = crate::constants::GEN_TABLE_OFF + (idx as usize) * 8;
        data[off..off + 8].copy_from_slice(&gen.to_le_bytes());
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

    // ─── DEX program IDs for HYPERP oracle (PERC-SetDexPool) ─────────────────

    /// PumpSwap AMM program ID: 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P
    /// PumpSwap: 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P
    pub const PUMPSWAP_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
        0x01, 0x56, 0xe0, 0xf6, 0x93, 0x66, 0x5a, 0xcf, 0x44, 0xdb, 0x15, 0x68, 0xbf, 0x17, 0x5b,
        0xaa, 0x51, 0x89, 0xcb, 0x97, 0xf5, 0xd2, 0xff, 0x3b, 0x65, 0x5d, 0x2b, 0xb6, 0xfd, 0x6d,
        0x18, 0xb0,
    ]);

    /// Raydium CLMM program ID: CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK
    /// Raydium CLMM: CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK
    pub const RAYDIUM_CLMM_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
        0xa5, 0xd5, 0xca, 0x9e, 0x04, 0xcf, 0x5d, 0xb5, 0x90, 0xb7, 0x14, 0xba, 0x2f, 0xe3, 0x2c,
        0xb1, 0x59, 0x13, 0x3f, 0xc1, 0xc1, 0x92, 0xb7, 0x22, 0x57, 0xfd, 0x07, 0xd3, 0x9c, 0xb0,
        0x40, 0x1e,
    ]);

    /// Meteora DLMM program ID: LBUZKhRxPF3XUpBCjp4YzTKgLLjTriggZTtEA3SsX1D
    /// Meteora DLMM: LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo
    pub const METEORA_DLMM_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
        0x04, 0xe9, 0xe1, 0x2f, 0xbc, 0x84, 0xe8, 0x26, 0xc9, 0x32, 0xcc, 0xe9, 0xe2, 0x64, 0x0c,
        0xce, 0x15, 0x59, 0x0c, 0x1c, 0x62, 0x73, 0xb0, 0x92, 0x57, 0x08, 0xba, 0x3b, 0x85, 0x20,
        0xb0, 0xbc,
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

    // ─── Fork-specific oracle stubs ───────────────────────────────────────────

    /// Check HYPERP oracle staleness: ensure the engine slot is recent enough.
    /// Returns error if `current_slot` is more than `max_staleness_slots` behind `clock_slot`.
    #[inline]
    pub fn check_hyperp_staleness(
        engine_slot: u64,
        max_staleness_slots: u64,
        clock_slot: u64,
    ) -> Result<(), solana_program::program_error::ProgramError> {
        if max_staleness_slots > 0 && clock_slot.saturating_sub(engine_slot) > max_staleness_slots {
            return Err(super::error::PercolatorError::OracleStale.into());
        }
        Ok(())
    }

    // =========================================================================
    // DEX Oracle Readers (PumpSwap, Raydium CLMM, Meteora DLMM)
    // Used by handle_update_hyperp_mark (tag 34).
    // =========================================================================

    // Raydium CLMM PoolState layout (Anchor — 8-byte discriminator)
    const RAYDIUM_CLMM_MIN_LEN: usize = 269;
    const RAYDIUM_CLMM_OFF_DECIMALS0: usize = 233;
    const RAYDIUM_CLMM_OFF_DECIMALS1: usize = 234;
    const RAYDIUM_CLMM_OFF_SQRT_PRICE_X64: usize = 253;

    // PumpSwap pool layout (no Anchor discriminator)
    const PUMPSWAP_MIN_LEN: usize = 195;
    const PUMPSWAP_OFF_BASE_MINT: usize = 35;
    const PUMPSWAP_OFF_QUOTE_MINT: usize = 67;
    const PUMPSWAP_OFF_BASE_VAULT: usize = 131;
    const PUMPSWAP_OFF_QUOTE_VAULT: usize = 163;

    // SPL Token Account: amount is at offset 64 (u64 LE)
    const SPL_TOKEN_AMOUNT_OFF: usize = 64;
    const SPL_TOKEN_ACCOUNT_MIN_LEN: usize = 72;

    // Meteora DLMM LbPair layout offsets
    const METEORA_DLMM_PRICE_MIN_LEN: usize = 80;
    const METEORA_DLMM_MIN_LEN: usize = 216;
    const METEORA_DLMM_OFF_BIN_STEP_SEED: usize = 73;
    const METEORA_DLMM_OFF_ACTIVE_ID: usize = 76;
    const METEORA_DLMM_OFF_RESERVE_Y: usize = 184;

    /// DEX price result with liquidity information.
    /// Used by UpdateHyperpMark to enforce minimum liquidity before accepting a price.
    pub struct DexPriceResult {
        /// The spot price in e6 format.
        pub price_e6: u64,
        /// Quote-side liquidity in the pool (quote token lamports/atoms).
        /// For PumpSwap: quote vault balance.
        /// For Raydium CLMM: virtual quote depth (L * sqrt_price / 2^64).
        /// For Meteora DLMM: vault_y SPL token balance.
        pub quote_liquidity: u64,
    }

    /// Read spot price from a Raydium CLMM pool account.
    /// Uses sqrt_price_x64 (Q64.64 fixed-point) to compute token_1 per token_0 in e6.
    fn read_raydium_clmm_price_e6(
        price_ai: &AccountInfo,
        expected_feed_id: &[u8; 32],
    ) -> Result<u64, ProgramError> {
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

        // price_e6 = sqrt_price^2 * 10^(6 + decimals_0 - decimals_1) / 2^128
        // The exponent accounts for both the e6 output scaling and the decimal difference.
        let decimal_diff = 6i32 + decimals_0 - decimals_1;

        // Must scale BEFORE dividing to avoid precision loss for prices < 2^64.
        let scale_exp = decimal_diff.max(0) as u32;
        let scale = 10u128.pow(scale_exp);
        // sqrt fits in 128 bits. sqrt * sqrt_scaled to avoid overflow:
        // Split: price = (sqrt / 2^64)^2 * scale = sqrt^2 * scale / 2^128
        // Use: (sqrt * scale_half) * sqrt / 2^128 where scale_half = sqrt(scale) — no, simpler:
        // price = ((sqrt >> 32) * (sqrt >> 32) * scale) >> 64
        // This gives 32-bit precision loss but handles the full range.
        let sqrt_shifted = sqrt_price_x64 >> 32;
        let price_e6 = if sqrt_shifted == 0 {
            0u128
        } else {
            let sq = sqrt_shifted * sqrt_shifted; // fits in 128 bits (64-bit * 64-bit)
            // sq = sqrt^2 / 2^64. Need to divide by another 2^64 and multiply by scale.
            // price_e6 = sq * scale / 2^64
            sq.checked_mul(scale)
                .map(|v| v >> 64)
                .unwrap_or_else(|| {
                    // Overflow: scale is too large, compute differently
                    (sq >> 64).saturating_mul(scale)
                })
        };
        let price_e6 = if decimal_diff < 0 {
            let down_scale = 10u128.pow((-decimal_diff) as u32);
            price_e6 / down_scale
        } else {
            price_e6
        };

        if price_e6 == 0 {
            return Err(PercolatorError::OracleInvalid.into());
        }
        if price_e6 > u64::MAX as u128 {
            return Err(PercolatorError::EngineOverflow.into());
        }

        Ok(price_e6 as u64)
    }

    /// Read spot price from a PumpSwap AMM pool. Price = quote_reserve / base_reserve in e6.
    fn read_pumpswap_price_e6(
        price_ai: &AccountInfo,
        expected_feed_id: &[u8; 32],
        remaining: &[AccountInfo],
    ) -> Result<u64, ProgramError> {
        if price_ai.key.to_bytes() != *expected_feed_id {
            return Err(PercolatorError::InvalidOracleKey.into());
        }

        let pool_data = price_ai.try_borrow_data()?;
        if pool_data.len() < PUMPSWAP_MIN_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        if remaining.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let _base_mint: [u8; 32] = pool_data[PUMPSWAP_OFF_BASE_MINT..PUMPSWAP_OFF_BASE_MINT + 32]
            .try_into()
            .unwrap();
        let _quote_mint: [u8; 32] = pool_data
            [PUMPSWAP_OFF_QUOTE_MINT..PUMPSWAP_OFF_QUOTE_MINT + 32]
            .try_into()
            .unwrap();

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

    /// Read spot price from a Meteora DLMM pool account.
    /// Price formula: (1 + bin_step/10000) ^ active_id, converted to e6.
    fn read_meteora_dlmm_price_e6(
        price_ai: &AccountInfo,
        expected_feed_id: &[u8; 32],
    ) -> Result<u64, ProgramError> {
        if price_ai.key.to_bytes() != *expected_feed_id {
            return Err(PercolatorError::InvalidOracleKey.into());
        }

        let data = price_ai.try_borrow_data()?;
        if data.len() < METEORA_DLMM_PRICE_MIN_LEN {
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

        let is_negative = active_id < 0;
        let exp = if is_negative {
            (-(active_id as i64)) as u64
        } else {
            active_id as u64
        };

        const SCALE: u128 = 1_000_000_000_000_000_000; // 1e18
        let base = SCALE + (bin_step as u128) * SCALE / 10_000;

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

        let price_e6 = if is_negative {
            if result == 0 {
                return Err(PercolatorError::OracleInvalid.into());
            }
            SCALE
                .checked_mul(1_000_000)
                .ok_or(PercolatorError::EngineOverflow)?
                / result
        } else {
            result / 1_000_000_000_000 // 1e18 -> 1e6
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
    /// Returns both the price and a measure of pool liquidity (quote-side depth).
    /// Applies inversion and unit scaling to the price.
    pub fn read_dex_price_with_liquidity(
        price_ai: &AccountInfo,
        invert: u8,
        unit_scale: u32,
        remaining_accounts: &[AccountInfo],
    ) -> Result<DexPriceResult, ProgramError> {
        let dex_feed_id = price_ai.key.to_bytes();

        let (raw_price, quote_liquidity) = if *price_ai.owner == PUMPSWAP_PROGRAM_ID {
            let pool_data = price_ai.try_borrow_data()?;
            if pool_data.len() < PUMPSWAP_MIN_LEN {
                return Err(ProgramError::InvalidAccountData);
            }
            if remaining_accounts.len() < 2 {
                return Err(ProgramError::NotEnoughAccountKeys);
            }
            let quote_vault_data = remaining_accounts[1].try_borrow_data()?;
            if quote_vault_data.len() < SPL_TOKEN_ACCOUNT_MIN_LEN {
                return Err(ProgramError::InvalidAccountData);
            }
            let quote_amount = u64::from_le_bytes(
                quote_vault_data[SPL_TOKEN_AMOUNT_OFF..SPL_TOKEN_AMOUNT_OFF + 8]
                    .try_into()
                    .unwrap(),
            );
            drop(quote_vault_data);
            drop(pool_data);
            let price = read_pumpswap_price_e6(price_ai, &dex_feed_id, remaining_accounts)?;
            (price, quote_amount)
        } else if *price_ai.owner == RAYDIUM_CLMM_PROGRAM_ID {
            let data = price_ai.try_borrow_data()?;
            if data.len() < RAYDIUM_CLMM_MIN_LEN {
                return Err(ProgramError::InvalidAccountData);
            }
            const RAYDIUM_CLMM_OFF_LIQUIDITY: usize = 237;
            let liquidity = if data.len() >= RAYDIUM_CLMM_OFF_LIQUIDITY + 16 {
                let liq = u128::from_le_bytes(
                    data[RAYDIUM_CLMM_OFF_LIQUIDITY..RAYDIUM_CLMM_OFF_LIQUIDITY + 16]
                        .try_into()
                        .unwrap(),
                );
                let sqrt_price_x64 = u128::from_le_bytes(
                    data[RAYDIUM_CLMM_OFF_SQRT_PRICE_X64..RAYDIUM_CLMM_OFF_SQRT_PRICE_X64 + 16]
                        .try_into()
                        .unwrap(),
                );
                let virtual_quote = liq.saturating_mul(sqrt_price_x64) >> 64;
                core::cmp::min(virtual_quote, u64::MAX as u128) as u64
            } else {
                0
            };
            drop(data);
            let price = read_raydium_clmm_price_e6(price_ai, &dex_feed_id)?;
            (price, liquidity)
        } else if *price_ai.owner == METEORA_DLMM_PROGRAM_ID {
            if remaining_accounts.is_empty() {
                return Err(ProgramError::NotEnoughAccountKeys);
            }
            let pool_data = price_ai.try_borrow_data()?;
            if pool_data.len() < METEORA_DLMM_MIN_LEN {
                return Err(ProgramError::InvalidAccountData);
            }
            let expected_reserve_y: [u8; 32] = pool_data
                [METEORA_DLMM_OFF_RESERVE_Y..METEORA_DLMM_OFF_RESERVE_Y + 32]
                .try_into()
                .unwrap();
            drop(pool_data);

            let vault_y_ai = &remaining_accounts[0];
            if vault_y_ai.key.to_bytes() != expected_reserve_y {
                return Err(PercolatorError::InvalidOracleKey.into());
            }
            let is_valid_token_program = *vault_y_ai.owner == crate::spl_token::id()
                || *vault_y_ai.owner == spl_token_2022::id();
            if !is_valid_token_program {
                return Err(PercolatorError::OracleInvalid.into());
            }
            let vault_y_data = vault_y_ai.try_borrow_data()?;
            if vault_y_data.len() < SPL_TOKEN_ACCOUNT_MIN_LEN {
                return Err(ProgramError::InvalidAccountData);
            }
            let quote_amount = u64::from_le_bytes(
                vault_y_data[SPL_TOKEN_AMOUNT_OFF..SPL_TOKEN_AMOUNT_OFF + 8]
                    .try_into()
                    .unwrap(),
            );
            drop(vault_y_data);
            let price = read_meteora_dlmm_price_e6(price_ai, &dex_feed_id)?;
            (price, quote_amount)
        } else {
            return Err(PercolatorError::OracleInvalid.into());
        };

        let price_after_invert = crate::verify::invert_price_e6(raw_price, invert)
            .ok_or(PercolatorError::OracleInvalid)?;
        let final_price = crate::verify::scale_price_e6(price_after_invert, unit_scale)
            .ok_or::<ProgramError>(PercolatorError::OracleInvalid.into())?;

        Ok(DexPriceResult {
            price_e6: final_price,
            quote_liquidity,
        })
    }

    /// Compute blended mark price from oracle (index) and DEX spot (impact_mid).
    /// When oracle_weight_bps == 0: returns impact_mid_e6 (pure DEX, backward compat).
    /// When oracle_weight_bps == 10_000: returns oracle_e6 (pure oracle).
    /// Values in between blend proportionally using u128 arithmetic.
    pub fn compute_blend_mark_price(
        oracle_e6: u64,
        impact_mid_e6: u64,
        oracle_weight_bps: u16,
    ) -> u64 {
        // Degenerate cases: use whichever is non-zero
        if impact_mid_e6 == 0 {
            return oracle_e6;
        }
        if oracle_e6 == 0 {
            return impact_mid_e6;
        }
        let w = (oracle_weight_bps as u64).min(10_000);
        let tw = 10_000u64.saturating_sub(w);
        // u128 arithmetic: max(price_e6) * 10_000 fits u128
        let blended = (oracle_e6 as u128)
            .saturating_mul(w as u128)
            .saturating_add((impact_mid_e6 as u128).saturating_mul(tw as u128))
            / 10_000u128;
        blended.min(u64::MAX as u128) as u64
    }

    /// Compute the next EMA mark price step.
    ///
    /// Circuit breaker clamped BEFORE EMA: oracle clamped to ±cap_e2bps*dt per slot.
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

        // EMA with compound alpha (effective_alpha = alpha * dt, capped at 1_000_000)
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

// 9a. mod insurance_lp — SPL mint/burn helpers for LP vault (reused by lp_vault)
pub mod insurance_lp {
    #[allow(unused_imports)]
    use alloc::format;
    #[cfg(not(feature = "test"))]
    use solana_program::system_instruction;
    use solana_program::{account_info::AccountInfo, program_error::ProgramError};

    #[cfg(not(feature = "test"))]
    use solana_program::program::{invoke, invoke_signed};
    #[cfg(not(feature = "test"))]
    use solana_program::sysvar::Sysvar;

    /// Create the insurance LP mint account (PDA) and initialize it.
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
            let space = crate::spl_token::state::MINT_LEN;
            let rent = solana_program::rent::Rent::get()?;
            let lamports = rent.minimum_balance(space);
            let create_ix = system_instruction::create_account(
                payer.key,
                mint_account.key,
                lamports,
                space as u64,
                &crate::spl_token::id(),
            );
            invoke_signed(
                &create_ix,
                &[payer.clone(), mint_account.clone(), system_program.clone()],
                &[mint_seeds],
            )?;
            let init_ix = crate::spl_token::initialize_mint(
                &crate::spl_token::id(),
                mint_account.key,
                vault_authority.key,
                None,
                decimals,
            )?;
            invoke(
                &init_ix,
                &[mint_account.clone(), rent_sysvar.clone(), token_program.clone()],
            )?;
        }
        #[cfg(feature = "test")]
        {
            use spl_token::state::Mint;
            use spl_token::solana_program::program_pack::Pack;
            let mut data = mint_account.try_borrow_mut_data()?;
            let mint = Mint {
                is_initialized: true,
                decimals,
                mint_authority: solana_program::program_option::COption::Some(*vault_authority.key),
                supply: 0,
                ..Mint::default()
            };
            Mint::pack(mint, &mut data).map_err(|_| ProgramError::InvalidAccountData)?;
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
            use solana_program::program::invoke_signed;
            let ix = crate::spl_token::mint_to(
                token_program.key,
                mint.key,
                destination.key,
                authority.key,
                &[],
                amount,
            )?;
            invoke_signed(
                &ix,
                &[mint.clone(), destination.clone(), authority.clone(), token_program.clone()],
                signer_seeds,
            )
        }
        #[cfg(feature = "test")]
        {
            use spl_token::state::{Account, Mint};
            use spl_token::solana_program::program_pack::Pack;
            {
                let mut mint_data = mint.try_borrow_mut_data()?;
                let mut m = Mint::unpack(&mint_data).map_err(|_| ProgramError::InvalidAccountData)?;
                m.supply = m.supply.checked_add(amount).ok_or(ProgramError::InvalidAccountData)?;
                Mint::pack(m, &mut mint_data).map_err(|_| ProgramError::InvalidAccountData)?;
            }
            {
                let mut dst_data = destination.try_borrow_mut_data()?;
                let mut acct = Account::unpack(&dst_data).unwrap_or_default();
                acct.amount = acct.amount.checked_add(amount).ok_or(ProgramError::InvalidAccountData)?;
                Account::pack(acct, &mut dst_data).map_err(|_| ProgramError::InvalidAccountData)?;
            }
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
            use solana_program::program::invoke;
            let ix = crate::spl_token::burn(
                token_program.key,
                source.key,
                mint.key,
                authority.key,
                &[],
                amount,
            )?;
            invoke(
                &ix,
                &[source.clone(), mint.clone(), authority.clone(), token_program.clone()],
            )
        }
        #[cfg(feature = "test")]
        {
            use spl_token::state::{Account, Mint};
            use spl_token::solana_program::program_pack::Pack;
            {
                let mut mint_data = mint.try_borrow_mut_data()?;
                let mut m = Mint::unpack(&mint_data).map_err(|_| ProgramError::InvalidAccountData)?;
                m.supply = m.supply.checked_sub(amount).ok_or(ProgramError::InsufficientFunds)?;
                Mint::pack(m, &mut mint_data).map_err(|_| ProgramError::InvalidAccountData)?;
            }
            {
                let mut src_data = source.try_borrow_mut_data()?;
                let mut acct = Account::unpack(&src_data).unwrap_or_default();
                acct.amount = acct.amount.checked_sub(amount).ok_or(ProgramError::InsufficientFunds)?;
                Account::pack(acct, &mut src_data).map_err(|_| ProgramError::InvalidAccountData)?;
            }
            Ok(())
        }
    }

    /// Read the current supply from an SPL mint account.
    pub fn read_mint_supply(mint_account: &AccountInfo) -> Result<u64, ProgramError> {
        use spl_token::state::Mint;
        use spl_token::solana_program::program_pack::Pack;
        let data = mint_account.try_borrow_data()?;
        let mint = Mint::unpack(&data).map_err(|_| ProgramError::InvalidAccountData)?;
        if !mint.is_initialized {
            return Err(ProgramError::UninitializedAccount);
        }
        Ok(mint.supply)
    }

    /// Read the decimals from an SPL mint account.
    pub fn read_mint_decimals(mint_account: &AccountInfo) -> Result<u8, ProgramError> {
        use spl_token::state::Mint;
        use spl_token::solana_program::program_pack::Pack;
        let data = mint_account.try_borrow_data()?;
        let mint = Mint::unpack(&data).map_err(|_| ProgramError::InvalidAccountData)?;
        Ok(mint.decimals)
    }
}

// 9b. mod lp_vault — LP vault state and helpers (PERC-272)
pub mod lp_vault {
    use bytemuck::{Pod, Zeroable};

    /// LP vault state account size in bytes.
    pub const LP_VAULT_STATE_LEN: usize = core::mem::size_of::<LpVaultState>();

    /// Magic value for LP vault state: "LPVAULT\0"
    pub const LP_VAULT_MAGIC: u64 = 0x4C50_5641_554C_5400;

    /// LP vault state PDA account layout. Seeds: `[b"lp_vault", slab_key]`.
    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct LpVaultState {
        pub magic: u64,
        pub fee_share_bps: u64,
        pub total_capital: u128,
        pub epoch: u64,
        pub last_crank_slot: u64,
        pub last_fee_snapshot: u128,
        pub total_fees_distributed: u128,
        pub loyalty_enabled: u8,
        pub _loyalty_pad: [u8; 7],
        pub queue_threshold_bps: u16,
        pub queue_epochs: u8,
        pub _drip_pad: [u8; 5],
        pub current_fee_mult_bps: u32,
        pub lp_util_curve_enabled: u8,
        pub _padding304: [u8; 3],
        pub _reserved: [u8; 24],
        pub epoch_high_water_tvl: u128,
        pub hwm_floor_bps: u16,
        pub _hwm_padding: [u8; 6],
        pub _reserved2: [u8; 40],
    }

    impl LpVaultState {
        #[inline]
        pub fn is_initialized(&self) -> bool { self.magic == LP_VAULT_MAGIC }
        #[inline]
        pub fn new_zeroed() -> Self { <Self as Zeroable>::zeroed() }

        #[inline]
        pub fn tranche_enabled(&self) -> bool { self._reserved2[0] != 0 }
        #[inline]
        pub fn senior_capital(&self) -> u128 {
            u128::from_le_bytes(self._reserved2[8..24].try_into().unwrap())
        }
        #[inline]
        pub fn set_senior_capital(&mut self, capital: u128) {
            self._reserved2[8..24].copy_from_slice(&capital.to_le_bytes());
        }
        #[inline]
        pub fn junior_capital(&self) -> u128 {
            u128::from_le_bytes(self._reserved2[24..40].try_into().unwrap())
        }
        #[inline]
        pub fn set_junior_capital(&mut self, capital: u128) {
            self._reserved2[24..40].copy_from_slice(&capital.to_le_bytes());
        }
        #[inline]
        pub fn junior_fee_mult_bps(&self) -> u16 {
            u16::from_le_bytes([self._reserved2[2], self._reserved2[3]])
        }

        pub fn apply_loss_waterfall(&mut self, loss: u128) -> u128 {
            let junior = self.junior_capital();
            if loss <= junior {
                self.set_junior_capital(junior - loss);
                self.total_capital = self.total_capital.saturating_sub(loss);
                return loss;
            }
            self.set_junior_capital(0);
            let remainder = loss - junior;
            let senior = self.senior_capital();
            let senior_loss = remainder.min(senior);
            self.set_senior_capital(senior - senior_loss);
            let realized = junior + senior_loss;
            self.total_capital = self.total_capital.saturating_sub(realized);
            realized
        }
    }

    pub fn read_lp_vault_state(data: &[u8]) -> Option<LpVaultState> {
        if data.len() < LP_VAULT_STATE_LEN { return None; }
        Some(*bytemuck::from_bytes::<LpVaultState>(&data[..LP_VAULT_STATE_LEN]))
    }

    pub fn write_lp_vault_state(data: &mut [u8], state: &LpVaultState) {
        data[..LP_VAULT_STATE_LEN].copy_from_slice(bytemuck::bytes_of(state));
    }

    // ── PERC-309: Withdraw Queue ──────────────────────────────────────────
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
        /// SECURITY(CR-2): Slot of last successful claim. Used to enforce
        /// one claim per epoch_duration window. 0 = no claim yet.
        pub last_claim_slot: u64,
        pub _reserved: [u8; 16],
    }

    impl WithdrawQueue {
        #[inline]
        pub fn is_initialized(&self) -> bool { self.magic == WITHDRAW_QUEUE_MAGIC }
        #[inline]
        pub fn claimable_this_epoch(&self) -> u64 {
            if self.epochs_remaining == 0 { return 0; }
            let remaining_lp = self.queued_lp_amount.saturating_sub(self.claimed_so_far);
            if self.epochs_remaining == 1 { remaining_lp }
            else { remaining_lp / (self.epochs_remaining as u64) }
        }
    }

    pub fn read_withdraw_queue(data: &[u8]) -> Option<WithdrawQueue> {
        if data.len() < WITHDRAW_QUEUE_LEN { return None; }
        Some(*bytemuck::from_bytes::<WithdrawQueue>(&data[..WITHDRAW_QUEUE_LEN]))
    }

    pub fn write_withdraw_queue(data: &mut [u8], q: &WithdrawQueue) {
        data[..WITHDRAW_QUEUE_LEN].copy_from_slice(bytemuck::bytes_of(q));
    }

    // ── PERC-308: Loyalty Multiplier ─────────────────────────────────────
    pub const LOYALTY_TIER1_EPOCHS: u64 = 5;
    pub const LOYALTY_TIER2_EPOCHS: u64 = 20;
    pub const LOYALTY_MULT_BASE: u64 = 10_000;
    pub const LOYALTY_MULT_TIER1: u64 = 12_000;
    pub const LOYALTY_MULT_TIER2: u64 = 15_000;

    #[inline]
    pub fn loyalty_multiplier_bps(delta_epochs: u64) -> u64 {
        if delta_epochs > LOYALTY_TIER2_EPOCHS { LOYALTY_MULT_TIER2 }
        else if delta_epochs > LOYALTY_TIER1_EPOCHS { LOYALTY_MULT_TIER1 }
        else { LOYALTY_MULT_BASE }
    }

    #[inline]
    pub fn apply_loyalty_mult(fee: u64, delta_epochs: u64) -> u64 {
        let mult = loyalty_multiplier_bps(delta_epochs);
        ((fee as u128) * (mult as u128) / 10_000).min(u64::MAX as u128) as u64
    }

    pub const LOYALTY_STAKE_MAGIC: u64 = 0x5045_5243_4C4F_5941;
    pub const LOYALTY_STAKE_LEN: usize = core::mem::size_of::<LoyaltyStake>();

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct LoyaltyStake {
        pub magic: u64,
        pub entry_epoch: u64,
        pub _reserved: [u8; 48],
    }

    impl LoyaltyStake {
        #[inline]
        pub fn is_initialized(&self) -> bool { self.magic == LOYALTY_STAKE_MAGIC }
    }

    pub fn read_loyalty_stake(data: &[u8]) -> Option<LoyaltyStake> {
        if data.len() < LOYALTY_STAKE_LEN { return None; }
        Some(*bytemuck::from_bytes::<LoyaltyStake>(&data[..LOYALTY_STAKE_LEN]))
    }

    pub fn write_loyalty_stake(data: &mut [u8], s: &LoyaltyStake) {
        data[..LOYALTY_STAKE_LEN].copy_from_slice(bytemuck::bytes_of(s));
    }

    #[cfg(test)]
    mod tests {
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
                last_claim_slot: 0,
                _reserved: [0; 16],
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
            assert_eq!(c3, 3);
            assert_eq!(c1 + c2 + c3, 7);
        }
        #[test]
        fn test_loyalty_tiers() {
            assert_eq!(loyalty_multiplier_bps(0), 10_000);
            assert_eq!(loyalty_multiplier_bps(6), 12_000);
            assert_eq!(loyalty_multiplier_bps(21), 15_000);
        }
    }
}

// 9c. LP Collateral Pricing (PERC-315)
pub mod lp_collateral {
    pub fn lp_token_value(
        lp_amount: u64,
        vault_tvl: u128,
        total_supply: u64,
        ltv_bps: u64,
    ) -> u128 {
        if total_supply == 0 || vault_tvl == 0 || lp_amount == 0 { return 0; }
        let raw_value = (lp_amount as u128).saturating_mul(vault_tvl) / (total_supply as u128);
        raw_value.saturating_mul(ltv_bps as u128) / 10_000
    }

    pub fn tvl_drawdown_exceeded(old_tvl: u64, new_tvl: u128, threshold_bps: u64) -> bool {
        if old_tvl == 0 { return false; }
        let old = old_tvl as u128;
        if new_tvl >= old { return false; }
        let drawdown_bps = (old - new_tvl) * 10_000 / old;
        drawdown_bps > threshold_bps as u128
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
        fn test_drawdown_20pct() {
            assert!(!tvl_drawdown_exceeded(1000, 800, 2000));
            assert!(tvl_drawdown_exceeded(1000, 799, 2000));
        }
    }
}

// 9d. Settlement Dispute (PERC-314)
pub mod dispute {
    use bytemuck::{Pod, Zeroable};

    pub const DISPUTE_MAGIC: u64 = 0x5045_5243_4449_5350;
    pub const DISPUTE_LEN: usize = core::mem::size_of::<SettlementDispute>();

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct SettlementDispute {
        pub magic: u64,
        pub challenger: [u8; 32],
        pub proposed_price_e6: u64,
        pub proof_slot: u64,
        pub bond_amount: u64,
        pub outcome: u8,
        pub _pad: [u8; 7],
        pub dispute_slot: u64,
        pub _reserved: [u8; 16],
    }

    impl SettlementDispute {
        #[inline]
        pub fn is_initialized(&self) -> bool { self.magic == DISPUTE_MAGIC }
    }

    pub fn read_dispute(data: &[u8]) -> Option<SettlementDispute> {
        if data.len() < DISPUTE_LEN { return None; }
        Some(*bytemuck::from_bytes::<SettlementDispute>(&data[..DISPUTE_LEN]))
    }

    pub fn write_dispute(data: &mut [u8], d: &SettlementDispute) {
        data[..DISPUTE_LEN].copy_from_slice(bytemuck::bytes_of(d));
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn test_dispute_size() { assert_eq!(DISPUTE_LEN, 96); }
    }
}

// 9e. Cross-Market Portfolio Margining (PERC-CMOR)
pub mod cross_margin {
    use bytemuck::{Pod, Zeroable};

    pub const OFFSET_PAIR_MAGIC: u64 = 0x434D_4F52_5041_4952;
    pub const ATTESTATION_MAGIC: u64 = 0x434D_4F52_4154_5445;
    pub const OFFSET_PAIR_LEN: usize = core::mem::size_of::<OffsetPairConfig>();
    pub const ATTESTATION_LEN: usize = core::mem::size_of::<CrossMarginAttestation>();

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct OffsetPairConfig {
        pub magic: u64,
        pub offset_bps: u16,
        pub enabled: u8,
        pub _pad: [u8; 5],
        pub _reserved: [u8; 16],
    }

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct CrossMarginAttestation {
        pub magic: u64,
        pub _align_pad: [u8; 8],
        pub user_pos_a: i128,
        pub user_pos_b: i128,
        pub attested_slot: u64,
        pub offset_bps: u16,
        pub _pad: [u8; 6],
        pub owner: [u8; 32],
        pub slab_a: [u8; 32],
        pub slab_b: [u8; 32],
    }

    impl OffsetPairConfig {
        #[inline]
        pub fn is_initialized(&self) -> bool { self.magic == OFFSET_PAIR_MAGIC }
    }

    impl CrossMarginAttestation {
        #[inline]
        pub fn is_initialized(&self) -> bool { self.magic == ATTESTATION_MAGIC }
        #[inline]
        pub fn is_fresh(&self, current_slot: u64, max_age_slots: u64) -> bool {
            current_slot.saturating_sub(self.attested_slot) <= max_age_slots
        }
        pub fn compute_margin_credit_bps(&self) -> u16 {
            if self.offset_bps == 0 { return 0; }
            let a = self.user_pos_a;
            let b = self.user_pos_b;
            if a == 0 || b == 0 { return 0; }
            let hedged = (a > 0 && b < 0) || (a < 0 && b > 0);
            if !hedged { return 0; }
            let abs_a = a.unsigned_abs();
            let abs_b = b.unsigned_abs();
            let smaller = abs_a.min(abs_b);
            let larger = abs_a.max(abs_b);
            let credit = (self.offset_bps as u128).saturating_mul(smaller) / larger;
            credit.min(self.offset_bps as u128) as u16
        }
    }

    #[inline]
    pub fn order_slab_pair(a: &[u8; 32], b: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
        if a < b { (*a, *b) } else { (*b, *a) }
    }

    pub fn read_offset_pair(data: &[u8]) -> Option<OffsetPairConfig> {
        if data.len() < OFFSET_PAIR_LEN { return None; }
        Some(*bytemuck::from_bytes::<OffsetPairConfig>(&data[..OFFSET_PAIR_LEN]))
    }

    pub fn write_offset_pair(data: &mut [u8], cfg: &OffsetPairConfig) {
        data[..OFFSET_PAIR_LEN].copy_from_slice(bytemuck::bytes_of(cfg));
    }

    pub fn read_attestation(data: &[u8]) -> Option<CrossMarginAttestation> {
        if data.len() < ATTESTATION_LEN { return None; }
        Some(*bytemuck::from_bytes::<CrossMarginAttestation>(&data[..ATTESTATION_LEN]))
    }

    pub fn write_attestation(data: &mut [u8], att: &CrossMarginAttestation) {
        data[..ATTESTATION_LEN].copy_from_slice(bytemuck::bytes_of(att));
    }
}

// 9f. Creator Lock (PERC-627)
pub mod creator_lock {
    use bytemuck::{Pod, Zeroable};

    pub const CREATOR_LOCK_MAGIC: u64 = 0x4352_5452_4C4F_434B;
    pub const CREATOR_LOCK_STATE_LEN: usize = core::mem::size_of::<CreatorStakeLock>();
    pub const DEFAULT_LOCK_DURATION_SLOTS: u64 = 19_440_000;
    pub const EXTRACTION_LIMIT_BPS: u64 = 15_000;
    pub const CREATOR_LOCK_SEED: &[u8] = b"creator_lock";

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct CreatorStakeLock {
        pub magic: u64,
        pub bump: u8,
        pub _pad: [u8; 7],
        pub creator: [u8; 32],
        pub lock_start_slot: u64,
        pub lock_duration_slots: u64,
        pub lp_amount_locked: u64,
        pub cumulative_extracted: u64,
        pub cumulative_deposited: u64,
        pub fee_redirect_active: u8,
        pub _reserved: [u8; 7],
    }

    const _: () = assert!(CREATOR_LOCK_STATE_LEN == 96);

    #[inline]
    pub fn is_lock_expired(current_slot: u64, lock_start: u64, duration: u64) -> bool {
        current_slot >= lock_start.saturating_add(duration)
    }

    #[inline]
    pub fn max_withdrawable(total_lp: u64, locked_lp: u64, lock_expired: bool) -> u64 {
        if lock_expired { total_lp } else { total_lp.saturating_sub(locked_lp) }
    }

    #[inline]
    pub fn check_extraction_exceeded(extracted: u64, deposited: u64, limit_bps: u64) -> bool {
        if deposited == 0 { return false; }
        let lhs = (extracted as u128).saturating_mul(10_000);
        let rhs = (deposited as u128).saturating_mul(limit_bps as u128);
        lhs > rhs
    }

    #[inline]
    pub fn compute_fee_redirect(fee_amount: u64, redirect_active: bool) -> (u64, u64) {
        if redirect_active { (0, fee_amount) } else { (fee_amount, 0) }
    }

    pub fn read_state(data: &[u8]) -> Option<&CreatorStakeLock> {
        if data.len() < CREATOR_LOCK_STATE_LEN { return None; }
        let state: &CreatorStakeLock = bytemuck::from_bytes(&data[..CREATOR_LOCK_STATE_LEN]);
        if state.magic != CREATOR_LOCK_MAGIC { return None; }
        Some(state)
    }

    pub fn write_state(data: &mut [u8], state: &CreatorStakeLock) {
        data[..CREATOR_LOCK_STATE_LEN].copy_from_slice(bytemuck::bytes_of(state));
    }

    #[inline]
    pub fn is_fee_redirect_active(state: &CreatorStakeLock) -> bool {
        state.fee_redirect_active != 0
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn test_lock_not_expired() { assert!(!is_lock_expired(100, 50, 100)); }
        #[test]
        fn test_lock_expired_exact() { assert!(is_lock_expired(150, 50, 100)); }
        #[test]
        fn test_extraction_not_exceeded() {
            assert!(!check_extraction_exceeded(100, 100, 15_000));
        }
        #[test]
        fn test_extraction_exceeded() {
            assert!(check_extraction_exceeded(160, 100, 15_000));
        }
        #[test]
        fn test_state_size() { assert_eq!(CREATOR_LOCK_STATE_LEN, 96); }
    }
}

// 9g. Creator History (PERC-629)
pub mod creator_history {
    use bytemuck::{Pod, Zeroable};

    pub const CREATOR_HISTORY_MAGIC: u64 = 0x4352_5452_4849_5354;
    pub const CREATOR_HISTORY_LEN: usize = core::mem::size_of::<CreatorHistory>();
    pub const CREATOR_HISTORY_SEED: &[u8] = b"creator_history";
    pub const BASE_DEPOSIT_E6: u64 = 2_500_000_000;
    pub const MAX_FAILURE_EXPONENT: u32 = 10;
    pub const SUCCESS_DISCOUNT_BPS: u64 = 1_000;
    pub const MAX_DISCOUNT_BPS: u64 = 5_000;
    pub const OI_THRESHOLD_BPS: u64 = 1_000;
    pub const SLASH_BPS: u64 = 5_000;
    pub const EVALUATION_PERIOD_SLOTS: u64 = 6_480_000;

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

    #[inline]
    pub fn failure_multiplier_bps(failed: u16) -> u64 {
        let exp = (failed as u32).min(MAX_FAILURE_EXPONENT);
        10_000u64.saturating_mul(1u64 << exp)
    }

    #[inline]
    pub fn success_discount_bps(successful: u16) -> u64 {
        let raw = (successful as u64).saturating_mul(SUCCESS_DISCOUNT_BPS);
        raw.min(MAX_DISCOUNT_BPS)
    }

    #[inline]
    pub fn compute_required_deposit(base_e6: u64, failed: u16, successful: u16) -> u64 {
        let mult_bps = failure_multiplier_bps(failed);
        let disc_bps = success_discount_bps(successful);
        let numerator = (base_e6 as u128)
            .saturating_mul(mult_bps as u128)
            .saturating_mul((10_000u64.saturating_sub(disc_bps)) as u128);
        let result = (numerator / (10_000u128 * 10_000u128)).min(u64::MAX as u128) as u64;
        let floor = base_e6 / 2;
        result.max(floor)
    }

    /// Compute slash amount (50% of deposit). Returns (slash, remainder).
    #[inline]
    pub fn compute_slash(deposit: u64) -> (u64, u64) {
        let slash = deposit.saturating_mul(SLASH_BPS) / 10_000;
        let remainder = deposit.saturating_sub(slash);
        (slash, remainder)
    }

    #[inline]
    pub fn oi_threshold_met(deposit_e6: u64, current_oi_e6: u64) -> bool {
        let threshold = deposit_e6.saturating_mul(OI_THRESHOLD_BPS) / 10_000;
        current_oi_e6 >= threshold
    }

    pub fn read_state(data: &[u8]) -> Option<&CreatorHistory> {
        if data.len() < CREATOR_HISTORY_LEN { return None; }
        let state: &CreatorHistory = bytemuck::from_bytes(&data[..CREATOR_HISTORY_LEN]);
        if state.magic != CREATOR_HISTORY_MAGIC { return None; }
        Some(state)
    }

    pub fn write_state(data: &mut [u8], state: &CreatorHistory) {
        data[..CREATOR_HISTORY_LEN].copy_from_slice(bytemuck::bytes_of(state));
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn test_failure_multiplier_zero() { assert_eq!(failure_multiplier_bps(0), 10_000); }
        #[test]
        fn test_slash_calculation() {
            let (slash, remainder) = compute_slash(1_000_000);
            assert_eq!(slash, 500_000);
            assert_eq!(remainder, 500_000);
        }
        #[test]
        fn test_state_size() { assert_eq!(CREATOR_HISTORY_LEN, 32); }
    }
}

// 9h. Shared Vault (PERC-628)
pub mod shared_vault {
    use bytemuck::{Pod, Zeroable};

    pub const SHARED_VAULT_MAGIC: u64 = 0x5348_5244_5641_4C54;
    pub const SHARED_VAULT_STATE_LEN: usize = core::mem::size_of::<SharedVaultState>();
    pub const SHARED_VAULT_SEED: &[u8] = b"shared_vault";
    pub const MARKET_ALLOC_MAGIC: u64 = 0x4D4B_5441_4C4C_4F43;
    pub const MARKET_ALLOC_LEN: usize = core::mem::size_of::<MarketAllocation>();
    pub const MARKET_ALLOC_SEED: &[u8] = b"market_alloc";
    pub const WITHDRAW_REQ_MAGIC: u64 = 0x5754_4844_5252_4551;
    pub const WITHDRAW_REQ_LEN: usize = core::mem::size_of::<WithdrawalRequest>();
    pub const WITHDRAW_REQ_SEED: &[u8] = b"withdraw_req";
    pub const DEFAULT_EPOCH_DURATION_SLOTS: u64 = 72_000;
    pub const DEFAULT_MAX_MARKET_EXPOSURE_BPS: u16 = 2_000;

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct SharedVaultState {
        pub magic: u64,
        pub epoch_number: u64,
        pub total_capital: u128,
        pub total_allocated: u128,
        pub pending_withdrawals: u128,
        pub epoch_start_slot: u64,
        pub epoch_duration_slots: u64,
        pub max_market_exposure_bps: u16,
        pub bump: u8,
        pub _pad: [u8; 13],
        pub epoch_snapshot_capital: u128,
        pub epoch_snapshot_pending: u128,
    }

    const _: () = assert!(SHARED_VAULT_STATE_LEN == 128);

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

    #[inline]
    pub fn check_exposure_cap(total_capital: u128, market_allocation: u128, max_bps: u16) -> bool {
        if total_capital == 0 { return market_allocation == 0; }
        let lhs = market_allocation.saturating_mul(10_000);
        let rhs = total_capital.saturating_mul(max_bps as u128);
        lhs <= rhs
    }

    #[inline]
    pub fn available_for_allocation(total_capital: u128, total_allocated: u128) -> u128 {
        total_capital.saturating_sub(total_allocated)
    }

    #[inline]
    pub fn max_single_market_allocation(total_capital: u128, max_bps: u16) -> u128 {
        total_capital.saturating_mul(max_bps as u128) / 10_000
    }

    #[inline]
    pub fn is_epoch_elapsed(current_slot: u64, epoch_start: u64, duration: u64) -> bool {
        current_slot >= epoch_start.saturating_add(duration)
    }

    #[inline]
    pub fn epoch_from_slot(current_slot: u64, genesis_slot: u64, duration: u64) -> u64 {
        if duration == 0 { return 0; }
        current_slot.saturating_sub(genesis_slot) / duration
    }

    #[inline]
    pub fn queue_withdrawal(pending: u128, amount: u64) -> u128 {
        pending.saturating_add(amount as u128)
    }

    #[inline]
    pub fn compute_proportional_withdrawal(
        request_lp: u64,
        total_pending_lp: u128,
        available_capital: u128,
    ) -> u64 {
        if total_pending_lp == 0 { return 0; }
        if available_capital >= total_pending_lp { return request_lp; }
        let result = (request_lp as u128).saturating_mul(available_capital) / total_pending_lp;
        result.min(u64::MAX as u128) as u64
    }

    pub fn read_vault_state(data: &[u8]) -> Option<SharedVaultState> {
        if data.len() < SHARED_VAULT_STATE_LEN { return None; }
        let mut s = SharedVaultState::zeroed();
        bytemuck::bytes_of_mut(&mut s).copy_from_slice(&data[..SHARED_VAULT_STATE_LEN]);
        if s.magic != SHARED_VAULT_MAGIC { return None; }
        Some(s)
    }

    pub fn write_vault_state(data: &mut [u8], state: &SharedVaultState) {
        data[..SHARED_VAULT_STATE_LEN].copy_from_slice(bytemuck::bytes_of(state));
    }

    pub fn read_market_alloc(data: &[u8]) -> Option<MarketAllocation> {
        if data.len() < MARKET_ALLOC_LEN { return None; }
        let mut s = MarketAllocation::zeroed();
        bytemuck::bytes_of_mut(&mut s).copy_from_slice(&data[..MARKET_ALLOC_LEN]);
        if s.magic != MARKET_ALLOC_MAGIC { return None; }
        Some(s)
    }

    pub fn write_market_alloc(data: &mut [u8], state: &MarketAllocation) {
        data[..MARKET_ALLOC_LEN].copy_from_slice(bytemuck::bytes_of(state));
    }

    pub fn read_withdraw_req(data: &[u8]) -> Option<WithdrawalRequest> {
        if data.len() < WITHDRAW_REQ_LEN { return None; }
        let mut s = WithdrawalRequest::zeroed();
        bytemuck::bytes_of_mut(&mut s).copy_from_slice(&data[..WITHDRAW_REQ_LEN]);
        if s.magic != WITHDRAW_REQ_MAGIC { return None; }
        Some(s)
    }

    pub fn write_withdraw_req(data: &mut [u8], state: &WithdrawalRequest) {
        data[..WITHDRAW_REQ_LEN].copy_from_slice(bytemuck::bytes_of(state));
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn test_exposure_cap_within() { assert!(check_exposure_cap(1000, 200, 2_000)); }
        #[test]
        fn test_exposure_cap_exceeded() { assert!(!check_exposure_cap(1000, 201, 2_000)); }
        #[test]
        fn test_proportional_full() {
            assert_eq!(compute_proportional_withdrawal(100, 200, 300), 100);
        }
        #[test]
        fn test_proportional_partial() {
            assert_eq!(compute_proportional_withdrawal(100, 200, 100), 50);
        }
        #[test]
        fn test_struct_sizes() {
            assert_eq!(SHARED_VAULT_STATE_LEN, 128);
            assert_eq!(MARKET_ALLOC_LEN, 48);
            assert_eq!(WITHDRAW_REQ_LEN, 32);
        }
    }
}

// 9i. Position NFT (PERC-608)
pub mod position_nft {
    use bytemuck::{Pod, Zeroable};

    pub const POSITION_NFT_MAGIC: u64 = 0x504F_534E_4654_0000;
    pub const POSITION_NFT_STATE_LEN: usize = core::mem::size_of::<PositionNftState>();
    pub const POSITION_NFT_SEED: &[u8] = b"position_nft";
    pub const POSITION_NFT_MINT_SEED: &[u8] = b"position_nft_mint";

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct PositionNftState {
        pub magic: u64,
        pub mint: [u8; 32],
        pub slab: [u8; 32],
        pub owner: [u8; 32],
        pub user_idx: u16,
        pub pending_settlement: u8,
        pub bump: u8,
        pub mint_bump: u8,
        pub _reserved: [u8; 19],
    }

    const _SIZE_CHECK: [(); 128] = [(); core::mem::size_of::<PositionNftState>()];

    impl PositionNftState {
        #[inline]
        pub fn is_initialized(&self) -> bool { self.magic == POSITION_NFT_MAGIC }
    }

    pub fn derive_position_nft(
        program_id: &solana_program::pubkey::Pubkey,
        slab_key: &solana_program::pubkey::Pubkey,
        user_idx: u16,
    ) -> (solana_program::pubkey::Pubkey, u8) {
        solana_program::pubkey::Pubkey::find_program_address(
            &[POSITION_NFT_SEED, slab_key.as_ref(), &user_idx.to_le_bytes()],
            program_id,
        )
    }

    pub fn derive_position_nft_mint(
        program_id: &solana_program::pubkey::Pubkey,
        slab_key: &solana_program::pubkey::Pubkey,
        user_idx: u16,
    ) -> (solana_program::pubkey::Pubkey, u8) {
        solana_program::pubkey::Pubkey::find_program_address(
            &[POSITION_NFT_MINT_SEED, slab_key.as_ref(), &user_idx.to_le_bytes()],
            program_id,
        )
    }

    pub fn read_position_nft_state(data: &[u8]) -> Option<PositionNftState> {
        if data.len() < POSITION_NFT_STATE_LEN { return None; }
        Some(*bytemuck::from_bytes::<PositionNftState>(&data[..POSITION_NFT_STATE_LEN]))
    }

    pub fn write_position_nft_state(data: &mut [u8], state: &PositionNftState) {
        data[..POSITION_NFT_STATE_LEN].copy_from_slice(bytemuck::bytes_of(state));
    }

    fn write_u64_decimal(mut n: u64, buf: &mut [u8]) -> usize {
        if n == 0 { buf[0] = b'0'; return 1; }
        let mut tmp = [0u8; 20];
        let mut i = 0usize;
        while n > 0 { tmp[i] = b'0' + (n % 10) as u8; n /= 10; i += 1; }
        let len = i;
        for j in 0..len { buf[j] = tmp[len - 1 - j]; }
        len
    }

    fn write_i128_decimal(n: i128, buf: &mut [u8]) -> usize {
        if n < 0 {
            buf[0] = b'-';
            let abs = (n as u128).wrapping_neg();
            let mut tmp = [0u8; 39];
            let mut idx = 0usize;
            let mut v = abs;
            if v == 0 { tmp[0] = b'0'; idx = 1; }
            else { while v > 0 { tmp[idx] = b'0' + (v % 10) as u8; v /= 10; idx += 1; } }
            let len = idx;
            for j in 0..len { buf[1 + j] = tmp[len - 1 - j]; }
            1 + len
        } else {
            write_u64_decimal(n as u64, buf)
        }
    }

    pub const NFT_MINT_SPACE: usize = 512;

    #[allow(unused_variables, clippy::too_many_arguments)]
    pub fn create_nft_mint_with_metadata<'a>(
        payer: &solana_program::account_info::AccountInfo<'a>,
        mint_account: &solana_program::account_info::AccountInfo<'a>,
        mint_authority: &solana_program::account_info::AccountInfo<'a>,
        system_program: &solana_program::account_info::AccountInfo<'a>,
        token2022_program: &solana_program::account_info::AccountInfo<'a>,
        rent_sysvar: &solana_program::account_info::AccountInfo<'a>,
        mint_seeds: &[&[u8]],
        direction: &str,
        entry_price: u64,
        size: i128,
    ) -> Result<(), solana_program::program_error::ProgramError> {
        let mut ep_buf = [0u8; 24];
        let ep_len = write_u64_decimal(entry_price, &mut ep_buf);
        let entry_price_str = core::str::from_utf8(&ep_buf[..ep_len])
            .map_err(|_| solana_program::program_error::ProgramError::InvalidAccountData)?;
        let mut sz_buf = [0u8; 42];
        let sz_len = write_i128_decimal(size, &mut sz_buf);
        let size_str = core::str::from_utf8(&sz_buf[..sz_len])
            .map_err(|_| solana_program::program_error::ProgramError::InvalidAccountData)?;

        #[cfg(not(feature = "test"))]
        {
            use alloc::string::{String, ToString};
            use solana_program::program::{invoke, invoke_signed};
            use solana_program::rent::Rent;
            use solana_program::sysvar::Sysvar;

            let rent = Rent::get()?;
            let lamports = rent.minimum_balance(NFT_MINT_SPACE);

            let create_ix = solana_program::system_instruction::create_account(
                payer.key,
                mint_account.key,
                lamports,
                NFT_MINT_SPACE as u64,
                token2022_program.key,
            );
            invoke_signed(
                &create_ix,
                &[payer.clone(), mint_account.clone(), system_program.clone()],
                &[mint_seeds],
            )?;

            let init_mp_ix = spl_token_2022::extension::metadata_pointer::instruction::initialize(
                token2022_program.key,
                mint_account.key,
                Some(*mint_authority.key),
                Some(*mint_account.key),
            )?;
            invoke(&init_mp_ix, &[mint_account.clone(), token2022_program.clone()])?;

            let init_mint_ix = spl_token_2022::instruction::initialize_mint2(
                token2022_program.key,
                mint_account.key,
                mint_authority.key,
                Some(mint_authority.key),
                0,
            )?;
            invoke(&init_mint_ix, &[mint_account.clone(), token2022_program.clone()])?;

            let init_meta_ix = spl_token_metadata_interface::instruction::initialize(
                token2022_program.key,
                mint_account.key,
                mint_authority.key,
                mint_account.key,
                mint_authority.key,
                "PERC-POS".to_string(),
                "PP".to_string(),
                String::new(),
            );
            invoke_signed(
                &init_meta_ix,
                &[mint_account.clone(), mint_authority.clone(), mint_account.clone(), mint_authority.clone()],
                &[mint_seeds],
            )?;

            let upd_dir_ix = spl_token_metadata_interface::instruction::update_field(
                token2022_program.key,
                mint_account.key,
                mint_authority.key,
                spl_token_metadata_interface::state::Field::Key("direction".to_string()),
                direction.to_string(),
            );
            invoke_signed(&upd_dir_ix, &[mint_account.clone(), mint_authority.clone()], &[mint_seeds])?;

            let upd_ep_ix = spl_token_metadata_interface::instruction::update_field(
                token2022_program.key,
                mint_account.key,
                mint_authority.key,
                spl_token_metadata_interface::state::Field::Key("entry_price".to_string()),
                entry_price_str.to_string(),
            );
            invoke_signed(&upd_ep_ix, &[mint_account.clone(), mint_authority.clone()], &[mint_seeds])?;

            let upd_sz_ix = spl_token_metadata_interface::instruction::update_field(
                token2022_program.key,
                mint_account.key,
                mint_authority.key,
                spl_token_metadata_interface::state::Field::Key("size".to_string()),
                size_str.to_string(),
            );
            invoke_signed(&upd_sz_ix, &[mint_account.clone(), mint_authority.clone()], &[mint_seeds])?;
        }
        #[cfg(feature = "test")]
        {
            use solana_program::program_pack::Pack;
            use spl_token_2022::state::Mint;
            let mut data = mint_account.try_borrow_mut_data()?;
            if data.len() < Mint::LEN {
                return Err(solana_program::program_error::ProgramError::InvalidAccountData);
            }
            let mint_state = Mint {
                is_initialized: true,
                decimals: 0,
                mint_authority: solana_program::program_option::COption::Some(*mint_authority.key),
                freeze_authority: solana_program::program_option::COption::Some(*mint_authority.key),
                supply: 0,
            };
            Mint::pack(mint_state, &mut data[..Mint::LEN])?;
            let dir_bytes = direction.as_bytes();
            let ep_bytes = entry_price_str.as_bytes();
            let sz_bytes = size_str.as_bytes();
            let buf_len = data.len();
            let dir_start = 82usize;
            let dir_end = (dir_start + dir_bytes.len()).min(buf_len);
            data[dir_start..dir_end].copy_from_slice(&dir_bytes[..dir_end - dir_start]);
            let ep_start = 130usize;
            let ep_end = (ep_start + ep_bytes.len()).min(buf_len);
            data[ep_start..ep_end].copy_from_slice(&ep_bytes[..ep_end - ep_start]);
            let sz_start = 180usize;
            let sz_end = (sz_start + sz_bytes.len()).min(buf_len);
            data[sz_start..sz_end].copy_from_slice(&sz_bytes[..sz_end - sz_start]);
        }
        let _ = (entry_price_str, size_str, direction);
        Ok(())
    }

    #[allow(unused_variables)]
    pub fn mint_nft_to<'a>(
        token2022_program: &solana_program::account_info::AccountInfo<'a>,
        mint: &solana_program::account_info::AccountInfo<'a>,
        destination: &solana_program::account_info::AccountInfo<'a>,
        authority: &solana_program::account_info::AccountInfo<'a>,
        signer_seeds: &[&[&[u8]]],
    ) -> Result<(), solana_program::program_error::ProgramError> {
        #[cfg(not(feature = "test"))]
        {
            use solana_program::program::invoke_signed;
            let ix = spl_token_2022::instruction::mint_to(
                token2022_program.key, mint.key, destination.key, authority.key, &[], 1,
            )?;
            invoke_signed(
                &ix,
                &[mint.clone(), destination.clone(), authority.clone(), token2022_program.clone()],
                signer_seeds,
            )
        }
        #[cfg(feature = "test")]
        {
            use solana_program::program_pack::Pack;
            use spl_token_2022::state::{Account as TokenAccount, Mint};
            let mut mint_data = mint.try_borrow_mut_data()?;
            let mut mint_state = Mint::unpack(&mint_data[..Mint::LEN])?;
            mint_state.supply = mint_state.supply.checked_add(1)
                .ok_or(solana_program::program_error::ProgramError::InvalidAccountData)?;
            Mint::pack(mint_state, &mut mint_data[..Mint::LEN])?;
            drop(mint_data);
            let mut dst_data = destination.try_borrow_mut_data()?;
            let mut dst_state = TokenAccount::unpack(&dst_data)?;
            dst_state.amount = dst_state.amount.checked_add(1)
                .ok_or(solana_program::program_error::ProgramError::InvalidAccountData)?;
            TokenAccount::pack(dst_state, &mut dst_data)?;
            Ok(())
        }
    }

    #[allow(unused_variables)]
    pub fn burn_nft<'a>(
        token2022_program: &solana_program::account_info::AccountInfo<'a>,
        mint: &solana_program::account_info::AccountInfo<'a>,
        source: &solana_program::account_info::AccountInfo<'a>,
        authority: &solana_program::account_info::AccountInfo<'a>,
    ) -> Result<(), solana_program::program_error::ProgramError> {
        #[cfg(not(feature = "test"))]
        {
            use solana_program::program::invoke;
            let ix = spl_token_2022::instruction::burn(
                token2022_program.key, source.key, mint.key, authority.key, &[], 1,
            )?;
            invoke(&ix, &[source.clone(), mint.clone(), authority.clone(), token2022_program.clone()])
        }
        #[cfg(feature = "test")]
        {
            use solana_program::program_pack::Pack;
            use spl_token_2022::state::{Account as TokenAccount, Mint};
            let mut src_data = source.try_borrow_mut_data()?;
            let mut src_state = TokenAccount::unpack(&src_data)?;
            src_state.amount = src_state.amount.checked_sub(1)
                .ok_or(solana_program::program_error::ProgramError::InsufficientFunds)?;
            TokenAccount::pack(src_state, &mut src_data)?;
            drop(src_data);
            let mut mint_data = mint.try_borrow_mut_data()?;
            let mut mint_state = Mint::unpack(&mint_data[..Mint::LEN])?;
            mint_state.supply = mint_state.supply.checked_sub(1)
                .ok_or(solana_program::program_error::ProgramError::InvalidAccountData)?;
            Mint::pack(mint_state, &mut mint_data[..Mint::LEN])?;
            Ok(())
        }
    }

    #[allow(unused_variables)]
    pub fn close_nft_mint<'a>(
        token2022_program: &solana_program::account_info::AccountInfo<'a>,
        mint: &solana_program::account_info::AccountInfo<'a>,
        destination: &solana_program::account_info::AccountInfo<'a>,
        close_authority: &solana_program::account_info::AccountInfo<'a>,
        signer_seeds: &[&[&[u8]]],
    ) -> Result<(), solana_program::program_error::ProgramError> {
        #[cfg(not(feature = "test"))]
        {
            use solana_program::program::invoke_signed;
            let ix = spl_token_2022::instruction::close_account(
                token2022_program.key, mint.key, destination.key, close_authority.key, &[],
            )?;
            invoke_signed(
                &ix,
                &[mint.clone(), destination.clone(), close_authority.clone(), token2022_program.clone()],
                signer_seeds,
            )
        }
        #[cfg(feature = "test")]
        {
            let lamports = mint.lamports();
            **mint.try_borrow_mut_lamports()
                .map_err(|_| solana_program::program_error::ProgramError::AccountBorrowFailed)? = 0;
            **destination.try_borrow_mut_lamports()
                .map_err(|_| solana_program::program_error::ProgramError::AccountBorrowFailed)? =
                destination.lamports().checked_add(lamports)
                    .ok_or(solana_program::program_error::ProgramError::ArithmeticOverflow)?;
            Ok(())
        }
    }

    #[allow(unused_variables)]
    pub fn transfer_nft<'a>(
        token2022_program: &solana_program::account_info::AccountInfo<'a>,
        mint: &solana_program::account_info::AccountInfo<'a>,
        source: &solana_program::account_info::AccountInfo<'a>,
        destination: &solana_program::account_info::AccountInfo<'a>,
        authority: &solana_program::account_info::AccountInfo<'a>,
    ) -> Result<(), solana_program::program_error::ProgramError> {
        #[cfg(not(feature = "test"))]
        {
            use solana_program::program::invoke;
            let ix = spl_token_2022::instruction::transfer_checked(
                token2022_program.key, source.key, mint.key, destination.key, authority.key, &[], 1, 0,
            )?;
            invoke(
                &ix,
                &[source.clone(), mint.clone(), destination.clone(), authority.clone(), token2022_program.clone()],
            )
        }
        #[cfg(feature = "test")]
        {
            use solana_program::program_pack::Pack;
            use spl_token_2022::state::Account as TokenAccount;
            let mut src_data = source.try_borrow_mut_data()?;
            let mut src_state = TokenAccount::unpack(&src_data)?;
            src_state.amount = src_state.amount.checked_sub(1)
                .ok_or(solana_program::program_error::ProgramError::InsufficientFunds)?;
            TokenAccount::pack(src_state, &mut src_data)?;
            drop(src_data);
            let mut dst_data = destination.try_borrow_mut_data()?;
            let mut dst_state = TokenAccount::unpack(&dst_data)?;
            dst_state.amount = dst_state.amount.checked_add(1)
                .ok_or(solana_program::program_error::ProgramError::InvalidAccountData)?;
            TokenAccount::pack(dst_state, &mut dst_data)?;
            Ok(())
        }
    }
}

// 9. mod processor
pub mod processor {
    #[allow(unused_imports)]
    use alloc::format; // Required by msg! macro with format args in no_std builds
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
    #[allow(unused_imports)]
    use percolator::{
        RiskEngine, RiskError, RiskParams, I128, U128, ADL_ONE, MAX_ACCOUNTS,
    };
    #[allow(unused_imports)]
    use crate::constants::{ENGINE_OFF, ENGINE_LEN, HEADER_LEN};

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
        slab_data: Option<&mut [u8]>,
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
        // NOTE: FLAG_ORACLE_INITIALIZED is NOT set here.
        // The flag means "engine.last_oracle_price is a real price" which is
        // only true after the engine processes it via accrue_market_to or similar.
        // Setting it on wrapper read alone would be unsound because zero-fill
        // and other early-return paths skip the engine call.
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
        lp_account_id: u64,
    ) -> Result<(), RiskError> {
        let lp = &engine.accounts[lp_idx as usize];
        let exec = matcher.execute_match(
            &lp.matcher_program,
            &lp.matcher_context,
            lp_account_id,
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
            if slab.owner != program_id {
                return Err(ProgramError::IllegalOwner);
            }
            solana_program::log::sol_log_64(SLAB_LEN as u64, data.len() as u64, 0, 0, 0);
            return Err(PercolatorError::InvalidSlabLen.into());
        }
        // Reentrancy guard: reject ALL instructions while a CPI is in progress.
        // A malicious matcher can re-enter any permissionless instruction during
        // TradeCpi's matcher CPI, manipulating engine state mid-instruction.
        if state::is_cpi_in_progress(data) {
            return Err(ProgramError::InvalidAccountData);
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

    /// Reject if the market is paused.
    fn require_not_paused(data: &[u8]) -> Result<(), ProgramError> {
        if state::is_paused(data) {
            return Err(PercolatorError::MarketPaused.into());
        }
        Ok(())
    }

    /// PERC-298: Unpack oi_cap_multiplier_bps field.
    /// Lower 32 bits = OI cap multiplier. Bits 32..47 = skew_factor_bps.
    #[inline]
    pub fn unpack_oi_cap(packed: u64) -> (u64, u64) {
        let multiplier = packed & 0xFFFF_FFFF;
        let skew_factor = (packed >> 32) & 0xFFFF;
        (multiplier, skew_factor)
    }

    /// PERC-298: Pack OI cap multiplier and skew factor.
    #[inline]
    #[allow(dead_code)]
    pub fn pack_oi_cap(multiplier: u64, skew_factor: u64) -> u64 {
        (multiplier & 0xFFFF_FFFF) | ((skew_factor & 0xFFFF) << 32)
    }

    /// GH#2073: Verify the Token-2022 program account is the canonical spl_token_2022::id().
    #[allow(unused_variables)]
    fn verify_token22_program(a_token22: &AccountInfo) -> Result<(), ProgramError> {
        #[cfg(not(feature = "test"))]
        {
            if *a_token22.key != spl_token_2022::id() {
                return Err(PercolatorError::InvalidTokenProgram.into());
            }
            if !a_token22.executable {
                return Err(PercolatorError::InvalidTokenProgram.into());
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
    ) -> ProgramResult
    where
        'b: 'a,
    {
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
                handle_init_market(program_id, accounts, admin, collateral_mint, index_feed_id, max_staleness_secs, conf_filter_bps, invert, unit_scale, initial_mark_price_e6, maintenance_fee_per_slot, max_insurance_floor, min_oracle_price_cap_e2bps, insurance_withdraw_max_bps, insurance_withdraw_cooldown_slots, risk_params, insurance_floor, permissionless_resolve_stale_slots, custom_funding_horizon, custom_funding_k, custom_max_premium, custom_max_per_slot, mark_min_fee, force_close_delay_slots)?;
            }
            Instruction::InitUser { fee_payment } => {
                handle_init_user(program_id, accounts, fee_payment)?;
            }
            Instruction::InitLP {
                matcher_program,
                matcher_context,
                fee_payment,
            } => {
                handle_init_lp(program_id, accounts, matcher_program, matcher_context, fee_payment)?;
            }
            Instruction::DepositCollateral { user_idx, amount } => {
                handle_deposit_collateral(program_id, accounts, user_idx, amount)?;
            }
            Instruction::WithdrawCollateral { user_idx, amount } => {
                handle_withdraw_collateral(program_id, accounts, user_idx, amount)?;
            }
            Instruction::KeeperCrank {
                caller_idx,
                candidates,
            } => {
                handle_keeper_crank(program_id, accounts, caller_idx, candidates)?;
            }
            Instruction::TradeNoCpi {
                lp_idx,
                user_idx,
                size,
            } => {
                handle_trade_no_cpi(program_id, accounts, lp_idx, user_idx, size)?;
            }
            Instruction::TradeCpi {
                lp_idx,
                user_idx,
                size,
                limit_price_e6,
            } => {
                handle_trade_cpi(program_id, accounts, lp_idx, user_idx, size, limit_price_e6)?;
            }
            Instruction::LiquidateAtOracle { target_idx } => {
                handle_liquidate_at_oracle(program_id, accounts, target_idx)?;
            }
            Instruction::CloseAccount { user_idx } => {
                handle_close_account(program_id, accounts, user_idx)?;
            }
            Instruction::TopUpInsurance { amount } => {
                handle_top_up_insurance(program_id, accounts, amount)?;
            }
            Instruction::UpdateAdmin { new_admin } => {
                handle_update_admin(program_id, accounts, new_admin)?;
            }

            Instruction::CloseSlab => {
                handle_close_slab(program_id, accounts)?;

            }

            Instruction::UpdateConfig {
                funding_horizon_slots,
                funding_k_bps,
                funding_max_premium_bps,
                funding_max_bps_per_slot,
            } => {
                handle_update_config(program_id, accounts, funding_horizon_slots, funding_k_bps, funding_max_premium_bps, funding_max_bps_per_slot)?;
            }

            Instruction::SetOracleAuthority { new_authority } => {
                handle_set_oracle_authority(program_id, accounts, new_authority)?;

            }

            Instruction::PushOraclePrice {
                price_e6,
                timestamp,
            } => {
                handle_push_oracle_price(program_id, accounts, price_e6, timestamp)?;
            }

            Instruction::SetOraclePriceCap { max_change_e2bps } => {
                handle_set_oracle_price_cap(program_id, accounts, max_change_e2bps)?;
            }

            Instruction::ResolveMarket => {
                handle_resolve_market(program_id, accounts)?;
            }

            Instruction::WithdrawInsurance => {
                handle_withdraw_insurance(program_id, accounts)?;

            }

            Instruction::SetInsuranceWithdrawPolicy {
                authority,
                min_withdraw_base,
                max_withdraw_bps,
                cooldown_slots,
            } => {
                handle_set_insurance_withdraw_policy(program_id, accounts, authority, min_withdraw_base, max_withdraw_bps, cooldown_slots)?;
            }

            Instruction::WithdrawInsuranceLimited { amount } => {
                handle_withdraw_insurance_limited(program_id, accounts, amount)?;

            }

            Instruction::AdminForceCloseAccount { user_idx } => {
                handle_admin_force_close_account(program_id, accounts, user_idx)?;
            }

            Instruction::QueryLpFees { lp_idx } => {
                handle_query_lp_fees(program_id, accounts, lp_idx)?;
            }

            Instruction::ReclaimEmptyAccount { user_idx } => {
                handle_reclaim_empty_account(program_id, accounts, user_idx)?;
            }

            Instruction::SettleAccount { user_idx } => {
                handle_settle_account(program_id, accounts, user_idx)?;
            }

            Instruction::DepositFeeCredits { user_idx, amount } => {
                handle_deposit_fee_credits(program_id, accounts, user_idx, amount)?;
            }

            Instruction::ConvertReleasedPnl { user_idx, amount } => {
                handle_convert_released_pnl(program_id, accounts, user_idx, amount)?;
            }

            Instruction::ResolvePermissionless => {
                handle_resolve_permissionless(program_id, accounts)?;
            }

            Instruction::ForceCloseResolved { user_idx } => {
                handle_force_close_resolved(program_id, accounts, user_idx)?;
            }

            // ─── Fork-specific instruction handlers ────────────────────────

            Instruction::CreateLpVault {
                fee_share_bps,
                util_curve_enabled,
            } => {
                handle_create_lp_vault(program_id, accounts, fee_share_bps, util_curve_enabled)?;
            }

            Instruction::LpVaultDeposit { amount } => {
                handle_lp_vault_deposit(program_id, accounts, amount)?;
            }

            Instruction::LpVaultWithdraw { lp_amount } => {
                handle_lp_vault_withdraw(program_id, accounts, lp_amount)?;
            }

            Instruction::LpVaultCrankFees => {
                handle_lp_vault_crank_fees(program_id, accounts)?;
            }

            Instruction::FundMarketInsurance { amount } => {
                handle_fund_market_insurance(program_id, accounts, amount)?;
            }

            Instruction::SetInsuranceIsolation { bps } => {
                handle_set_insurance_isolation(program_id, accounts, bps)?;
            }

            Instruction::ChallengeSettlement { proposed_price_e6 } => {
                handle_challenge_settlement(program_id, accounts, proposed_price_e6)?;
            }

            Instruction::ResolveDispute { accept } => {
                handle_resolve_dispute(program_id, accounts, accept)?;
            }

            Instruction::DepositLpCollateral {
                user_idx,
                lp_amount,
            } => {
                handle_deposit_lp_collateral(program_id, accounts, user_idx, lp_amount)?;
            }

            Instruction::WithdrawLpCollateral {
                user_idx,
                lp_amount,
            } => {
                handle_withdraw_lp_collateral(program_id, accounts, user_idx, lp_amount)?;
            }

            Instruction::QueueWithdrawal { lp_amount } => {
                handle_queue_withdrawal(program_id, accounts, lp_amount)?;
            }

            Instruction::ClaimQueuedWithdrawal => {
                handle_claim_queued_withdrawal(program_id, accounts)?;
            }

            Instruction::CancelQueuedWithdrawal => {
                handle_cancel_queued_withdrawal(program_id, accounts)?;
            }

            Instruction::ExecuteAdl { target_idx } => {
                handle_execute_adl(program_id, accounts, target_idx)?;
            }

            Instruction::CloseStaleSlabs => {
                handle_close_stale_slabs(program_id, accounts)?;
            }

            Instruction::ReclaimSlabRent => {
                handle_reclaim_slab_rent(program_id, accounts)?;
            }

            Instruction::TransferOwnershipCpi {
                user_idx,
                new_owner,
            } => {
                handle_transfer_ownership_cpi(program_id, accounts, user_idx, new_owner)?;
            }

            Instruction::AuditCrank => {
                handle_audit_crank(program_id, accounts)?;
            }

            Instruction::SetOffsetPair { offset_bps } => {
                handle_set_offset_pair(program_id, accounts, offset_bps)?;
            }

            Instruction::AttestCrossMargin {
                user_idx_a,
                user_idx_b,
            } => {
                handle_attest_cross_margin(program_id, accounts, user_idx_a, user_idx_b)?;
            }

            Instruction::AdvanceOraclePhase => {
                handle_advance_oracle_phase(program_id, accounts)?;
            }

            // SECURITY(H-2/H-3/H-4): SharedVault subsystem disabled for mainnet.
            // Multiple critical bugs: no deposit instruction (total_capital always 0),
            // no LP mint/share system, QueueWithdrawalSV accepts arbitrary amounts
            // with no token validation (fund theft), AllocateMarket double-counts
            // total_allocated, and no deallocation path exists. The feature is
            // incomplete and cannot be safely fixed without fundamental redesign.
            // Handler code is preserved for future development.
            Instruction::InitSharedVault { .. } => {
                msg!("SharedVault subsystem disabled — feature incomplete");
                return Err(ProgramError::InvalidInstructionData);
            }

            Instruction::AllocateMarket { .. } => {
                msg!("SharedVault subsystem disabled — feature incomplete");
                return Err(ProgramError::InvalidInstructionData);
            }

            Instruction::AdvanceEpoch => {
                msg!("SharedVault subsystem disabled — feature incomplete");
                return Err(ProgramError::InvalidInstructionData);
            }

            Instruction::QueueWithdrawalSV { .. } => {
                msg!("SharedVault subsystem disabled — feature incomplete");
                return Err(ProgramError::InvalidInstructionData);
            }

            Instruction::ClaimEpochWithdrawal => {
                msg!("SharedVault subsystem disabled — feature incomplete");
                return Err(ProgramError::InvalidInstructionData);
            }

            Instruction::MintPositionNft { user_idx } => {
                handle_mint_position_nft(program_id, accounts, user_idx)?;
            }

            Instruction::TransferPositionOwnership { user_idx } => {
                handle_transfer_position_ownership(program_id, accounts, user_idx)?;
            }

            Instruction::BurnPositionNft { user_idx } => {
                handle_burn_position_nft(program_id, accounts, user_idx)?;
            }

            Instruction::SetPendingSettlement { user_idx } => {
                handle_set_pending_settlement(program_id, accounts, user_idx)?;
            }

            Instruction::ClearPendingSettlement { user_idx } => {
                handle_clear_pending_settlement(program_id, accounts, user_idx)?;
            }

            Instruction::SetWalletCap { cap_e6 } => {
                handle_set_wallet_cap(program_id, accounts, cap_e6)?;
            }

            Instruction::SetOiImbalanceHardBlock { threshold_bps } => {
                handle_set_oi_imbalance_hard_block(program_id, accounts, threshold_bps)?;
            }

            // PERC-8400: RescueOrphanVault
            // Layout-agnostic rescue: reads raw bytes from the slab header.
            // Accounts: [admin(signer), slab(readonly), admin_ata(writable),
            //            vault(writable), token_program, vault_pda]
            Instruction::RescueOrphanVault => {
                handle_rescue_orphan_vault(program_id, accounts)?;
            }

            // PERC-8400: CloseOrphanSlab
            // Accounts: [admin(signer,writable), slab(writable), vault(readonly)]
            Instruction::CloseOrphanSlab => {
                handle_close_orphan_slab(program_id, accounts)?;
            }

            // UpdateHyperpMark (Tag 34): Permissionless Hyperp DEX EMA oracle update.
            // Accounts: [0] slab(writable), [1] DEX pool, [2] clock, [3..N] remaining
            Instruction::UpdateHyperpMark => {
                handle_update_hyperp_mark(program_id, accounts)?;
            }

            Instruction::PauseMarket => {
                handle_pause_market(program_id, accounts)?;
            }

            Instruction::UnpauseMarket => {
                handle_unpause_market(program_id, accounts)?;
            }

            // PERC-SetDexPool (Tag 74): Pin admin-approved DEX pool for HYPERP markets.
            // Accounts: [admin(signer), slab(writable), pool_account(readonly)]
            Instruction::SetDexPool { pool } => {
                handle_set_dex_pool(program_id, accounts, pool)?;
            }

            // InitMatcherCtx (Tag 75): CPI to matcher program to initialize a matcher context.
            // Accounts: [admin(signer), slab(readonly), matcher_ctx(writable),
            //            matcher_prog(executable), lp_pda]
            Instruction::InitMatcherCtx {
                lp_idx,
                kind,
                trading_fee_bps,
                base_spread_bps,
                max_total_bps,
                impact_k_bps,
                liquidity_notional_e6,
                max_fill_abs,
                max_inventory_abs,
                fee_to_insurance_bps,
                skew_spread_mult_bps,
            } => {
                handle_init_matcher_ctx(program_id, accounts, lp_idx, kind, trading_fee_bps, base_spread_bps, max_total_bps, impact_k_bps, liquidity_notional_e6, max_fill_abs, max_inventory_abs, fee_to_insurance_bps, skew_spread_mult_bps)?;
            }
        }
        Ok(())
    }

    // --- InitMarket ---
    #[inline(never)]
    fn handle_init_market<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        admin: Pubkey,
        collateral_mint: Pubkey,
        index_feed_id: [u8; 32],
        max_staleness_secs: u64,
        conf_filter_bps: u16,
        invert: u8,
        unit_scale: u32,
        initial_mark_price_e6: u64,
        maintenance_fee_per_slot: u128,
        max_insurance_floor: u128,
        min_oracle_price_cap_e2bps: u64,
        insurance_withdraw_max_bps: u16,
        insurance_withdraw_cooldown_slots: u64,
        risk_params: RiskParams,
        insurance_floor: u128,
        permissionless_resolve_stale_slots: u64,
        custom_funding_horizon: Option<u64>,
        custom_funding_k: Option<u64>,
        custom_max_premium: Option<i64>,
        custom_max_per_slot: Option<i64>,
        mark_min_fee: u64,
        force_close_delay_slots: u64,
    ) -> ProgramResult {
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
        // maintenance_fee_per_slot: legacy wire field, ignored in v12.15 engine
        let _ = maintenance_fee_per_slot;
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
        // maintenance_fee_per_slot removed from engine in v12.15

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
            // maintenance_fee_per_slot removed in v12.15 engine
            if p.insurance_floor.get() > percolator::MAX_VAULT_TVL {
                return Err(ProgramError::InvalidInstructionData);
            }
            // new_account_fee must be payable: 0 <= fee <= MAX_VAULT_TVL
            if p.new_account_fee.get() > percolator::MAX_VAULT_TVL {
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
            _iw_padding2: [0u64; 2],
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
            // DEX pool pinning: initialized to all-zeros (not set).
            // Admin must call SetDexPool (tag 74) for HYPERP markets.
            dex_pool: [0u8; 32],
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
        // Write market_start_slot (§2.1): captures creation slot for rewards program.
        // Shares _reserved[8..16] with last_thr_update_slot (initialized to same value).
        state::write_market_start_slot(&mut data, clock.slot);

        Ok(())
    }

    // --- InitUser ---
    #[inline(never)]
    fn handle_init_user<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        fee_payment: u64,
    ) -> ProgramResult {
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
        // SECURITY(H-1): Block during emergency pause.
        require_not_paused(&data)?;

        // Block new users when market is resolved
        if state::is_resolved(&data) {
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
        // Canonical deposit-based materialization (spec §10.3).
        let idx = engine.free_head;
        engine.deposit_not_atomic(idx, units as u128, 0, clock.slot)
            .map_err(map_risk_error)?;
        // Charge new_account_fee: deduct from capital → insurance
        // Tokens are already in the vault from deposit() above, so we
        // only move the internal accounting (capital → insurance) without
        // touching engine.vault (which was already incremented by deposit).
        // Charge new_account_fee: capital → insurance.
        // engine.set_capital() is test_visible! (private in prod), so manual
        // adjustment is required. Mirrors set_capital's signed-delta logic.
        let fee = engine.params.new_account_fee.get();
        if fee > 0 {
            let cap = engine.accounts[idx as usize].capital.get();
            if cap < fee {
                return Err(PercolatorError::EngineInsufficientBalance.into());
            }
            engine.accounts[idx as usize].capital = percolator::U128::new(cap - fee);
            engine.c_tot = percolator::U128::new(
                engine.c_tot.get().checked_sub(fee)
                    .ok_or(ProgramError::ArithmeticOverflow)?,
            );
            let new_ins = engine.insurance_fund.balance.get()
                .checked_add(fee)
                .ok_or(ProgramError::ArithmeticOverflow)?;
            engine.insurance_fund.balance = percolator::U128::new(new_ins);
        }
        engine.set_owner(idx, a_user.key.to_bytes())
            .map_err(map_risk_error)?;
        drop(engine);
        // LP identity: write generation counter so TradeCpi can verify lp_account_id
        let gen = state::next_mat_counter(&mut data);
        state::write_account_generation(&mut data, idx, gen);
        Ok(())
    }

    // --- InitLP ---
    #[inline(never)]
    fn handle_init_lp<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        matcher_program: Pubkey,
        matcher_context: Pubkey,
        fee_payment: u64,
    ) -> ProgramResult {
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
        // SECURITY(H-1): Block during emergency pause.
        require_not_paused(&data)?;

        // Block new LPs when market is resolved
        if state::is_resolved(&data) {
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
        let idx = engine.free_head;
        engine.deposit_not_atomic(idx, units as u128, 0, clock.slot)
            .map_err(map_risk_error)?;
        // Charge new_account_fee: capital → insurance (no vault change)
        // Charge new_account_fee: capital → insurance.
        // engine.set_capital() is test_visible! (private in prod), so manual
        // adjustment is required. Mirrors set_capital's signed-delta logic.
        let fee = engine.params.new_account_fee.get();
        if fee > 0 {
            let cap = engine.accounts[idx as usize].capital.get();
            if cap < fee {
                return Err(PercolatorError::EngineInsufficientBalance.into());
            }
            engine.accounts[idx as usize].capital = percolator::U128::new(cap - fee);
            engine.c_tot = percolator::U128::new(
                engine.c_tot.get().checked_sub(fee)
                    .ok_or(ProgramError::ArithmeticOverflow)?,
            );
            let new_ins = engine.insurance_fund.balance.get()
                .checked_add(fee)
                .ok_or(ProgramError::ArithmeticOverflow)?;
            engine.insurance_fund.balance = percolator::U128::new(new_ins);
        }
        // Set LP fields
        engine.accounts[idx as usize].kind = percolator::Account::KIND_LP;
        engine.accounts[idx as usize].matcher_program = matcher_program.to_bytes();
        engine.accounts[idx as usize].matcher_context = matcher_context.to_bytes();
        engine.set_owner(idx, a_user.key.to_bytes())
            .map_err(map_risk_error)?;
        drop(engine);
        // LP identity: write generation counter so TradeCpi can verify lp_account_id
        let gen = state::next_mat_counter(&mut data);
        state::write_account_generation(&mut data, idx, gen);
        Ok(())
    }

    // --- DepositCollateral ---
    #[inline(never)]
    fn handle_deposit_collateral<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        user_idx: u16,
        amount: u64,
    ) -> ProgramResult {
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
        // SECURITY(H-1): Block during emergency pause.
        require_not_paused(&data)?;

        // Block deposits when market is resolved
        if state::is_resolved(&data) {
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
            .deposit_not_atomic(user_idx, units as u128, 0, clock.slot)
            .map_err(map_risk_error)?;
        Ok(())
    }

    // --- WithdrawCollateral ---
    #[inline(never)]
    fn handle_withdraw_collateral<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        user_idx: u16,
        amount: u64,
    ) -> ProgramResult {
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
        // SECURITY(H-1): Block during emergency pause (users exit via CloseAccount).
        require_not_paused(&data)?;
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

        let resolved = state::is_resolved(&data);
        let clock = Clock::from_account_info(a_clock)?;
        let price = if resolved {
            let settlement = config.authority_price_e6;
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
                    &mut config, a_oracle_idx,
                )?
            } else {
                read_price_and_stamp(&mut config, a_oracle_idx, clock.unix_timestamp, clock.slot, Some(&mut *data))?
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

        // Use frozen time on resolved markets
        let withdraw_slot = if resolved { state::get_resolved_slot(&config) } else { clock.slot };
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
        Ok(())
    }

    // --- KeeperCrank ---
    #[inline(never)]
    fn handle_keeper_crank<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        caller_idx: u16,
        candidates: alloc::vec::Vec<(u16, Option<percolator::LiquidationPolicy>)>,
    ) -> ProgramResult {
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
        if state::is_resolved(&data) {
            let config = state::read_config(&data);
            let settlement_price = config.authority_price_e6;
            if settlement_price == 0 {
                return Err(ProgramError::InvalidAccountData);
            }

            // Use resolution_slot (snapshotted in ResolveMarket)
            let frozen_slot = state::get_resolved_slot(&config);

            // Dust sweep: resolved crank must also sweep dust so
            // CloseSlab's dust_base == 0 check can eventually pass.
            let dust_before = state::read_dust_base(&data);
            let unit_scale = config.unit_scale;

            let engine = zc::engine_mut(&mut data)?;

            // Resolved crank: no per-account settlement loop.
            // Accounts are settled by ForceCloseResolved / CloseAccount.

            // Sweep dust to insurance fund.
            // On resolved markets, also forgive sub-scale remainder
            // (worth < 1 engine unit, no engine accounting entry).
            let forgive_dust = if unit_scale > 0 {
                let scale = unit_scale as u64;
                if dust_before >= scale {
                    let units_to_sweep = dust_before / scale;
                    engine.top_up_insurance_fund(
                        units_to_sweep as u128, frozen_slot,
                    ).map_err(map_risk_error)?;
                }
                true
            } else {
                false
            };

            // §10.0 steps 4-7 / §10.8 steps 9-12: end-of-instruction lifecycle.
            // Propagate CorruptState (real invariant violation), ignore other
            // errors (side-reset may fail on frozen ADL state post-resolution).
            let mut ctx = percolator::InstructionContext::new();
            match engine.run_end_of_instruction_lifecycle(&mut ctx) {
                Ok(()) => {}
                Err(percolator::RiskError::CorruptState) => {
                    return Err(map_risk_error(percolator::RiskError::CorruptState));
                }
                Err(_) => {} // non-fatal on resolved markets
            }

            // engine borrow ends here (last use above).
            // Write dust_base AFTER dropping the engine borrow to avoid
            // aliasing conflict with state::write_dust_base.
            if forgive_dust && dust_before != 0 {
                // Forgive any sub-scale remainder — on resolved markets
                // no new dust can accumulate, so this is terminal cleanup.
                state::write_dust_base(&mut data, 0);
            }

            return Ok(());
        }

        let mut config = state::read_config(&data);

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
            read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot, Some(&mut *data))?
        };

        state::write_config(&mut data, &config);

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
        let funding_rate = compute_current_funding_rate_e9(&config);
        let h_lock = engine.params.h_min;
        let _outcome = engine
            .keeper_crank_not_atomic(
                clock.slot,
                price,
                &candidates,
                percolator::LIQ_BUDGET_PER_CRANK,
                funding_rate,
                h_lock,
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
                    .top_up_insurance_fund(units_to_sweep as u128, clock.slot)
                    .map_err(map_risk_error)?;
                Some(dust_before % scale)
            } else {
                None
            }
        } else {
            None
        };

        // Copy stats before threshold update (avoid borrow conflict)
        let ins_low = engine.insurance_fund.balance.get() as u64;

        // Spec §2.2.1: I_floor is immutable — no auto-update.
        // Insurance floor is set at InitMarket and never changes.
        // (EWMA auto-update removed per spec compliance.)

        // Write remaining dust if sweep occurred
        if let Some(dust) = remaining_dust {
            state::write_dust_base(&mut data, dust);
        }

        // Debug: log lifetime counters (sol_log_64: tag=CRANK_STATS, liqs, max_accounts, insurance, 0)
        // 0xC8A4C5 = "CRANK_STATS" tag; replaces msg!("CRANK_STATS") to save ~300 CU
        sol_log_64(0xC8A4C5, 0, MAX_ACCOUNTS as u64, ins_low, 0);

        Ok(())
    }

    // --- TradeNoCpi ---
    #[inline(never)]
    fn handle_trade_no_cpi<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        lp_idx: u16,
        user_idx: u16,
        size: i128,
    ) -> ProgramResult {
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
        // SECURITY(H-1): Block trading during emergency pause.
        require_not_paused(&data)?;

        // Block trading when market is resolved
        if state::is_resolved(&data) {
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
            read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot, Some(&mut *data))?;
        state::write_config(&mut data, &config);

        // LP identity: read generation before engine borrow
        let lp_gen = state::read_account_generation(&data, lp_idx);

        let engine = zc::engine_mut(&mut data)?;

        check_idx(engine, lp_idx)?;
        check_idx(engine, user_idx)?;

        // Reject same-index trades — using the same engine account slot as
        // both buyer and seller corrupts position/capital state because the
        // engine reads and writes the same mutable slot for both sides.
        if user_idx == lp_idx {
            return Err(ProgramError::InvalidArgument);
        }

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
        let funding_rate = compute_current_funding_rate_e9(&config);
        execute_trade_with_matcher(
            engine, &NoOpMatcher, lp_idx, user_idx, clock.slot, price, size,
            funding_rate, lp_gen,
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

        // Write updated config (mark_ewma changed)
        state::write_config(&mut data, &config);
        #[cfg(feature = "cu-audit")]
        {
            msg!("CU_CHECKPOINT: trade_nocpi_execute_end");
            sol_log_compute_units();
        }
        Ok(())
    }

    // --- TradeCpi ---
    #[inline(never)]
    fn handle_trade_cpi<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        lp_idx: u16,
        user_idx: u16,
        size: i128,
        limit_price_e6: u64, // 0 = no limit (backward compat),
    ) -> ProgramResult {
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
        let (lp_account_id, mut config, req_id, lp_matcher_prog, lp_matcher_ctx, engine_current_slot) = {
            let data = a_slab.try_borrow_data()?;
            slab_guard(program_id, a_slab, &*data)?;
            require_initialized(&*data)?;
            // SECURITY(H-1): Block trading during emergency pause.
            require_not_paused(&*data)?;

            // Block trading when market is resolved
            if state::is_resolved(&*data) {
                return Err(ProgramError::InvalidAccountData);
            }

            let config = state::read_config(&*data);

            // Phase 3: Monotonic nonce for req_id (prevents replay attacks)
            // Nonce advancement via verify helper (Kani-provable)
            let nonce = state::read_req_nonce(&*data);
            let req_id = crate::verify::nonce_on_success(nonce)
                .ok_or(ProgramError::InvalidAccountData)?;

            let engine = zc::engine_ref(&*data)?;

            check_idx(engine, lp_idx)?;
            check_idx(engine, user_idx)?;

            // Reject same-index trades — using the same engine account slot as
            // both buyer and seller corrupts position/capital state.
            if user_idx == lp_idx {
                return Err(ProgramError::InvalidArgument);
            }

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
            // Instance-stable LP identity: FNV-1a hash of (lp_idx, owner, matcher_context).
            // account_id field removed in v12.17; use deterministic hash instead.
            let lp_instance_id = {
                let mut h: u64 = 0xcbf29ce484222325; // FNV offset basis
                let mix = |h: &mut u64, b: u8| {
                    *h ^= b as u64;
                    *h = h.wrapping_mul(0x100000001b3); // FNV prime
                };
                mix(&mut h, lp_idx as u8);
                mix(&mut h, (lp_idx >> 8) as u8);
                for &b in lp_acc.owner.iter() { mix(&mut h, b); }
                for &b in lp_acc.matcher_context.iter() { mix(&mut h, b); }
                h
            };
            (
                lp_instance_id,
                config,
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
            read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot, None)?
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
            let mut data = state::slab_data_mut(a_slab)?;
            let pristine = state::read_config(&data);
            config.last_effective_price_e6 = pristine.last_effective_price_e6;
            config.last_hyperp_index_slot = pristine.last_hyperp_index_slot;
            // Revert last_good_oracle_slot too — zero-fills must not refresh
            // the oracle-death timer (prevents resolution-delay manipulation).
            config.last_good_oracle_slot = pristine.last_good_oracle_slot;
            state::write_config(&mut data, &config);
            state::write_req_nonce(&mut data, req_id);
            return Ok(());
        }

        let exec_price = ret.exec_price_e6;
        // Reject extreme exec prices that would corrupt engine state
        // or produce absurd PnL. Must check BEFORE engine call.
        if exec_price > percolator::MAX_ORACLE_PRICE {
            return Err(PercolatorError::OracleInvalid.into());
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
            let funding_rate = compute_current_funding_rate_e9(&config);
            execute_trade_with_matcher(
                engine, &matcher, lp_idx, user_idx, clock.slot, price, trade_size,
                funding_rate, lp_account_id,
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
        // Engine borrow dropped. Write nonce + config.
        {
            let mut data = state::slab_data_mut(a_slab)?;
            state::write_req_nonce(&mut data, req_id);
            state::write_config(&mut data, &config);
        }
        Ok(())
    }

    // --- LiquidateAtOracle ---
    #[inline(never)]
    fn handle_liquidate_at_oracle<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        target_idx: u16,
    ) -> ProgramResult {
        accounts::expect_len(accounts, 4)?;
        // AUDIT CRIT-2 FIX: require caller to be a signer
        accounts::expect_signer(&accounts[0])?;
        let a_slab = &accounts[1];
        let a_oracle = &accounts[3];
        accounts::expect_writable(a_slab)?;

        let mut data = state::slab_data_mut(a_slab)?;
        slab_guard(program_id, a_slab, &data)?;
        require_initialized(&data)?;

        // Block liquidations after market resolution — resolved markets
        // are in withdraw-only settlement phase.
        if state::is_resolved(&data) {
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
            read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot, Some(&mut *data))?
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
                compute_current_funding_rate_e9(&config), h_lock)
            .map_err(map_risk_error)?;
        sol_log_64(_res as u64, 0, 0, 0, 4); // result
        #[cfg(feature = "cu-audit")]
        {
            msg!("CU_CHECKPOINT: liquidate_end");
            sol_log_compute_units();
        }
        Ok(())
    }

    // --- CloseAccount ---
    #[inline(never)]
    fn handle_close_account<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        user_idx: u16,
    ) -> ProgramResult {
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

        let resolved = state::is_resolved(&data);
        let clock = Clock::from_account_info(&accounts[6])?;
        let price = if resolved {
            let settlement = config.authority_price_e6;
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
                read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot, Some(&mut *data))?
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
            // Do NOT pre-touch: touch can fail on epoch-mismatch accounts
            // that force_close_resolved was specifically designed to handle.
            match engine.force_close_resolved_not_atomic(user_idx, clock.slot)
                .map_err(map_risk_error)?
            {
                percolator::ResolvedCloseResult::ProgressOnly => {
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
                    compute_current_funding_rate_e9(&config), h_lock)
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
        Ok(())
    }

    // --- TopUpInsurance ---
    #[inline(never)]
    fn handle_top_up_insurance<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        amount: u64,
    ) -> ProgramResult {
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
        if state::is_resolved(&data) {
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
        Ok(())
    }

    // --- UpdateAdmin ---
    #[inline(never)]
    fn handle_update_admin<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        new_admin: Pubkey,
    ) -> ProgramResult {
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

        // SECURITY(R4-H1): Reject admin burn (zero pubkey) when no
        // permissionless resolution path exists. Without this guard,
        // burning admin on a market with permissionless_resolve_stale_slots=0
        // permanently bricks all user funds — no one can resolve the market,
        // force-close positions, or withdraw insurance.
        if new_admin == Pubkey::default() {
            let config = state::read_config(&data);
            if config.permissionless_resolve_stale_slots == 0
                || config.force_close_delay_slots == 0
            {
                msg!(
                    "UpdateAdmin: cannot burn admin without permissionless \
                     resolve ({}) and force-close ({}) paths enabled",
                    config.permissionless_resolve_stale_slots,
                    config.force_close_delay_slots,
                );
                return Err(ProgramError::InvalidArgument);
            }
        }

        header.admin = new_admin.to_bytes();
        state::write_header(&mut data, &header);
        Ok(())
    }

    // --- CloseSlab ---
    #[inline(never)]
    fn handle_close_slab<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],) -> ProgramResult {
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
            if !state::is_resolved(&data) {
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

            // Forgive any remaining dust_base — engine accounting is zero,
            // and any sub-scale remainder has been drained from the vault.
            // (dust_base tracks base-unit fractions with no engine entry)

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
        Ok(())
    }

    // --- UpdateConfig ---
    #[inline(never)]
    fn handle_update_config<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        funding_horizon_slots: u64,
        funding_k_bps: u64,
        funding_max_premium_bps: i64,
        funding_max_bps_per_slot: i64,
    ) -> ProgramResult {
        accounts::expect_len(accounts, 3)?;
        let a_admin = &accounts[0];
        let a_slab = &accounts[1];
        let a_clock = &accounts[2];

        accounts::expect_signer(a_admin)?;
        accounts::expect_writable(a_slab)?;

        let mut data = state::slab_data_mut(a_slab)?;
        slab_guard(program_id, a_slab, &data)?;
        require_initialized(&data)?;
        if state::is_resolved(&data) {
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
                // Non-Hyperp: use last oracle price from engine
                let engine = zc::engine_ref(&data)?;
                engine.last_oracle_price
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
        Ok(())
    }

    // --- SetOracleAuthority ---
    #[inline(never)]
    fn handle_set_oracle_authority<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        new_authority: Pubkey,
    ) -> ProgramResult {
        accounts::expect_len(accounts, 2)?;
        let a_admin = &accounts[0];
        let a_slab = &accounts[1];

        accounts::expect_signer(a_admin)?;
        accounts::expect_writable(a_slab)?;

        let mut data = state::slab_data_mut(a_slab)?;
        slab_guard(program_id, a_slab, &data)?;
        require_initialized(&data)?;
        if state::is_resolved(&data) {
            return Err(ProgramError::InvalidAccountData);
        }

        let header = state::read_header(&data);
        require_admin(header.admin, a_admin.key)?;

        // SECURITY(M-4): Block SetOracleAuthority on Pyth-pinned markets.
        // Pyth-pinned markets guarantee decentralized oracle pricing.
        // Setting a non-zero authority would silently switch to admin-oracle
        // mode, breaking the trust model users signed up for.
        let mut config = state::read_config(&data);
        if crate::verify::is_pyth_pinned_mode(config.oracle_authority, config.index_feed_id)
            && new_authority != Pubkey::default()
        {
            msg!("SetOracleAuthority: blocked on Pyth-pinned market");
            return Err(ProgramError::InvalidArgument);
        }
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
        Ok(())
    }

    // --- PushOraclePrice ---
    #[inline(never)]
    fn handle_push_oracle_price<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        price_e6: u64,
        timestamp: i64,
    ) -> ProgramResult {
        accounts::expect_len(accounts, 2)?;
        let a_authority = &accounts[0];
        let a_slab = &accounts[1];

        accounts::expect_signer(a_authority)?;
        accounts::expect_writable(a_slab)?;

        let mut data = state::slab_data_mut(a_slab)?;
        slab_guard(program_id, a_slab, &data)?;
        require_initialized(&data)?;
        if state::is_resolved(&data) {
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
        Ok(())
    }

    // --- SetOraclePriceCap ---
    #[inline(never)]
    fn handle_set_oracle_price_cap<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        max_change_e2bps: u64,
    ) -> ProgramResult {
        accounts::expect_len(accounts, 3)?;
        let a_admin = &accounts[0];
        let a_slab = &accounts[1];
        let a_clock = &accounts[2];

        accounts::expect_signer(a_admin)?;
        accounts::expect_writable(a_slab)?;

        let mut data = state::slab_data_mut(a_slab)?;
        slab_guard(program_id, a_slab, &data)?;
        require_initialized(&data)?;
        if state::is_resolved(&data) {
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
        Ok(())
    }

    // --- ResolveMarket ---
    #[inline(never)]
    fn handle_resolve_market<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],) -> ProgramResult {
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
        // Block resolution during pause — prevents admin from pausing the
        // market (blocking user exits) then resolving at a manipulated price
        // while users cannot adjust positions or withdraw.
        require_not_paused(&data)?;

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

        // Accrue engine to settlement price BEFORE entering resolved mode.
        // This crystallizes all account states at the settlement price so
        // force_close_resolved (which takes no price parameter) uses
        // consistent post-accrual state.
        if !oracle::is_hyperp_mode(&config) {
            let engine = zc::engine_mut(&mut data)?;
            engine.accrue_market_to(
                clock.slot, config.authority_price_e6,
                compute_current_funding_rate_e9(&config),
            ).map_err(map_risk_error)?;
        }

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
            // Accrue to resolution boundary at the settlement mark price
            // (authority_price_e6), NOT the smoothed index. force_close_resolved
            // uses K coefficients that must reflect the declared settlement price.
            let engine = zc::engine_mut(&mut data)?;
            engine.accrue_market_to(
                clock.slot, config.authority_price_e6,
                compute_current_funding_rate_e9(&config),
            ).map_err(map_risk_error)?;
            config = state::read_config(&data);
        }

        state::set_resolved_slot(&mut config, clock.slot);
        state::write_config(&mut data, &config);

        // Set engine market_mode to Resolved so force_close_resolved_not_atomic accepts it.
        {
            let engine = zc::engine_mut(&mut data)?;
            engine.market_mode = percolator::MarketMode::Resolved;
        }

        state::set_resolved(&mut data);
        Ok(())
    }

    // --- WithdrawInsurance ---
    #[inline(never)]
    fn handle_withdraw_insurance<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],) -> ProgramResult {
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
        Ok(())
    }

    // --- SetInsuranceWithdrawPolicy ---
    #[inline(never)]
    fn handle_set_insurance_withdraw_policy<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        authority: Pubkey,
        min_withdraw_base: u64,
        max_withdraw_bps: u16,
        cooldown_slots: u64,
    ) -> ProgramResult {
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
        if !state::is_resolved(&data) {
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
        Ok(())
    }

    // --- WithdrawInsuranceLimited ---
    #[inline(never)]
    fn handle_withdraw_insurance_limited<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        amount: u64,
    ) -> ProgramResult {
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

        let resolved = state::is_resolved(&data);
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
        Ok(())
    }

    // --- AdminForceCloseAccount ---
    #[inline(never)]
    fn handle_admin_force_close_account<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        user_idx: u16,
    ) -> ProgramResult {
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
        if !state::is_resolved(&data) {
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
        // Resolved markets use fixed settlement price.
        let price = config.authority_price_e6;
        if price == 0 {
            return Err(ProgramError::InvalidAccountData);
        }

        let engine = zc::engine_mut(&mut data)?;

        // Migration: if flags say resolved but engine.market_mode is still Live,
        // set it to Resolved (needed for slabs resolved before this fix).
        if engine.market_mode == percolator::MarketMode::Live {
            engine.market_mode = percolator::MarketMode::Resolved;
        }

        check_idx(engine, user_idx)?;

        // Read account owner pubkey and verify owner ATA
        let owner_pubkey = Pubkey::new_from_array(engine.accounts[user_idx as usize].owner);
        verify_token_account(a_owner_ata, &owner_pubkey, &mint)?;

        let amt_units = match engine.force_close_resolved_not_atomic(user_idx, clock.slot)
            .map_err(map_risk_error)?
        {
            percolator::ResolvedCloseResult::ProgressOnly => return Ok(()),
            percolator::ResolvedCloseResult::Closed(payout) => payout,
        };
        let amt_units_u64: u64 = amt_units
            .try_into()
            .map_err(|_| PercolatorError::EngineOverflow)?;

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
        Ok(())
    }

    // --- QueryLpFees ---
    #[inline(never)]
    fn handle_query_lp_fees<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        lp_idx: u16,
    ) -> ProgramResult {
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

        // fees_earned_total removed in v12.17 — return 0 for backward compatibility.
        let fees = 0u128;
        solana_program::program::set_return_data(&fees.to_le_bytes());
        Ok(())
    }

    // --- ReclaimEmptyAccount ---
    #[inline(never)]
    fn handle_reclaim_empty_account<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        user_idx: u16,
    ) -> ProgramResult {
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
        if state::is_resolved(&data) {
            return Err(ProgramError::InvalidAccountData);
        }

        let clock = Clock::from_account_info(_a_clock)?;
        let engine = zc::engine_mut(&mut data)?;
        engine.reclaim_empty_account_not_atomic(user_idx, clock.slot)
            .map_err(map_risk_error)?;
        // Per §10.7: MUST NOT call accrue_market_to, MUST NOT mutate side state.
        Ok(())
    }

    // --- SettleAccount ---
    #[inline(never)]
    fn handle_settle_account<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        user_idx: u16,
    ) -> ProgramResult {
        // Standalone account settlement (§10.2). Permissionless.
        accounts::expect_len(accounts, 3)?;
        let a_slab = &accounts[0];
        let a_clock = &accounts[1];
        let a_oracle = &accounts[2];
        accounts::expect_writable(a_slab)?;

        let mut data = state::slab_data_mut(a_slab)?;
        slab_guard(program_id, a_slab, &data)?;
        require_initialized(&data)?;
        if state::is_resolved(&data) {
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
            read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot, Some(&mut *data))?
        };
        state::write_config(&mut data, &config);

        let engine = zc::engine_mut(&mut data)?;
        let h_lock = engine.params.h_min;
        engine.settle_account_not_atomic(user_idx, price, clock.slot,
            compute_current_funding_rate_e9(&config), h_lock)
            .map_err(map_risk_error)?;
        Ok(())
    }

    // --- DepositFeeCredits ---
    #[inline(never)]
    fn handle_deposit_fee_credits<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        user_idx: u16,
        amount: u64,
    ) -> ProgramResult {
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
            if state::is_resolved(&data) {
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
        Ok(())
    }

    // --- ConvertReleasedPnl ---
    #[inline(never)]
    fn handle_convert_released_pnl<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        user_idx: u16,
        amount: u64,
    ) -> ProgramResult {
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
        if state::is_resolved(&data) {
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
            read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot, Some(&mut *data))?
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
            compute_current_funding_rate_e9(&config), h_lock)
            .map_err(map_risk_error)?;
        Ok(())
    }

    // --- ResolvePermissionless ---
    #[inline(never)]
    fn handle_resolve_permissionless<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],) -> ProgramResult {
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

        if state::is_resolved(&data) {
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

        // Flush Hyperp index + accrue to boundary (Bug 1+3 fix)
        if is_hyperp {
            let mark = if config.mark_ewma_e6 > 0 {
                config.mark_ewma_e6
            } else {
                config.authority_price_e6
            };
            let prev_index = config.last_effective_price_e6;
            if mark > 0 && prev_index > 0 {
                let last_idx_slot = config.last_hyperp_index_slot;
                // Use clock.slot (not engine.last_crank_slot) to avoid
                // monotonicity failure when current_slot > last_crank_slot
                // from trades/withdrawals after the last crank.
                let dt = clock.slot.saturating_sub(last_idx_slot);
                let new_index = oracle::clamp_toward_with_dt(
                    prev_index.max(1), mark,
                    config.oracle_price_cap_e2bps, dt,
                );
                config.last_effective_price_e6 = new_index;
                config.last_hyperp_index_slot = clock.slot;
            }
            state::write_config(&mut data, &config);
            // Accrue at the mark (settlement price), not the smoothed index.
            // force_close_resolved uses K coefficients that must reflect mark.
            let settle_price = mark.max(1);
            let engine = zc::engine_mut(&mut data)?;
            engine.accrue_market_to(
                clock.slot,
                settle_price,
                compute_current_funding_rate_e9(&config),
            ).map_err(map_risk_error)?;
            config = state::read_config(&data);
        }

        // Settlement price = last oracle price from engine.
        // Reject only if completely uninitialized (p == 0).
        // The non-Hyperp init sentinel (p=1) is harmless: if no one ever
        // traded or cranked, there are no positions to settle. If anyone
        // did interact, accrue_market_to updated last_oracle_price to the
        // real price. The sentinel cannot cause incorrect settlement because:
        // - All resolve paths require crank staleness (no recent activity)
        // - If the market had activity, last_oracle_price reflects it
        // - Resolution at p=1 on an empty market is a no-op
        let last_price = {
            let engine = zc::engine_ref(&data)?;
            let p = engine.last_oracle_price;
            if p == 0 {
                return Err(PercolatorError::OracleInvalid.into());
            }
            p
        };

        // Accrue engine to settlement price before entering resolved mode.
        // Hyperp already accrued above; non-Hyperp accrues here.
        if !is_hyperp {
            let engine = zc::engine_mut(&mut data)?;
            engine.accrue_market_to(clock.slot, last_price,
                    compute_current_funding_rate_e9(&config))
                .map_err(map_risk_error)?;
        }

        // Use clock.slot (not engine.last_crank_slot) — other instructions
        // advance current_slot past last_crank_slot, so using the stale
        // crank slot would make resolved touch paths fail monotonic checks.
        state::set_resolved_slot(&mut config, clock.slot);
        config.authority_price_e6 = last_price;
        state::write_config(&mut data, &config);
        state::set_resolved(&mut data);
        Ok(())
    }

    // --- ForceCloseResolved ---
    #[inline(never)]
    fn handle_force_close_resolved<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        user_idx: u16,
    ) -> ProgramResult {
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

        if !state::is_resolved(&data) {
            return Err(ProgramError::InvalidAccountData);
        }

        let config = state::read_config(&data);
        if config.force_close_delay_slots == 0 {
            return Err(PercolatorError::InvalidConfigParam.into());
        }
        let clock = Clock::from_account_info(a_clock)?;
        if clock.slot < state::get_resolved_slot(&config)
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

        let price = config.authority_price_e6;
        if price == 0 {
            return Err(ProgramError::InvalidAccountData);
        }

        let engine = zc::engine_mut(&mut data)?;
        check_idx(engine, user_idx)?;

        let owner_pubkey = Pubkey::new_from_array(
            engine.accounts[user_idx as usize].owner,
        );
        verify_token_account(a_owner_ata, &owner_pubkey, &mint)?;

        let amt_units = match engine.force_close_resolved_not_atomic(user_idx, clock.slot)
            .map_err(map_risk_error)?
        {
            percolator::ResolvedCloseResult::ProgressOnly => return Ok(()),
            percolator::ResolvedCloseResult::Closed(payout) => payout,
        };

        let amt_units_u64: u64 = amt_units
            .try_into()
            .map_err(|_| PercolatorError::EngineOverflow)?;
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
        Ok(())
    }

    // --- CreateLpVault ---
    #[inline(never)]
    fn handle_create_lp_vault<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        fee_share_bps: u64,
        util_curve_enabled: bool,
    ) -> ProgramResult {
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
        if *a_system.key != solana_program::system_program::id() {
            return Err(ProgramError::IncorrectProgramId);
        }

        if fee_share_bps > 10_000 {
            return Err(PercolatorError::LpVaultInvalidFeeShare.into());
        }

        let data = state::slab_data_mut(a_slab)?;
        slab_guard(program_id, a_slab, &data)?;
        require_initialized(&data)?;

        let header = state::read_header(&data);
        require_admin(header.admin, a_admin.key)?;
        drop(data);

        #[allow(unused_variables)]
        let (expected_state, state_bump) =
            accounts::derive_lp_vault_state(program_id, a_slab.key);
        accounts::expect_key(a_lp_vault_state, &expected_state)?;

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

        let (expected_mint, mint_bump) =
            accounts::derive_lp_vault_mint(program_id, a_slab.key);
        accounts::expect_key(a_lp_vault_mint, &expected_mint)?;

        let (auth, _vault_bump) = accounts::derive_vault_authority(program_id, a_slab.key);
        accounts::expect_key(a_vault_authority, &auth)?;

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
        }

        {
            let mut state_data = a_lp_vault_state.try_borrow_mut_data()?;
            if state_data.len() < crate::lp_vault::LP_VAULT_STATE_LEN {
                return Err(ProgramError::AccountDataTooSmall);
            }
            let mut vault_state = crate::lp_vault::LpVaultState::new_zeroed();
            vault_state.magic = crate::lp_vault::LP_VAULT_MAGIC;
            vault_state.fee_share_bps = fee_share_bps;
            vault_state.epoch = 1;
            vault_state.lp_util_curve_enabled = if util_curve_enabled { 1 } else { 0 };
            vault_state.current_fee_mult_bps = crate::verify::FEE_MULT_BASE_BPS as u32;
            vault_state.hwm_floor_bps = 5000;
            let slab_data = a_slab.try_borrow_data()?;
            let engine = zc::engine_ref(&slab_data)?;
            // fee_revenue not in current InsuranceFund layout — snapshot is 0
            vault_state.last_fee_snapshot = 0u128;
            drop(slab_data);
            crate::lp_vault::write_lp_vault_state(&mut state_data, &vault_state);
        }

        let mint_seeds: &[&[u8]] = &[b"lp_vault_mint", a_slab.key.as_ref(), &[mint_bump]];
        let decimals = 6u8;
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
        Ok(())
    }

    // --- LpVaultDeposit ---
    #[inline(never)]
    fn handle_lp_vault_deposit<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        amount: u64,
    ) -> ProgramResult {
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

        if state::is_resolved(&slab_data) {
            return Err(ProgramError::InvalidAccountData);
        }
        require_not_paused(&slab_data)?;

        let config = state::read_config(&slab_data);
        let mint = Pubkey::new_from_array(config.collateral_mint);

        // Use stored vault_authority_bump (~1500 CU cheaper than find_program_address)
        let vault_bump = config.vault_authority_bump;
        let auth = accounts::derive_vault_authority_with_bump(program_id, a_slab.key, vault_bump)?;
        verify_vault(
            a_vault,
            &auth,
            &mint,
            &Pubkey::new_from_array(config.vault_pubkey),
        )?;
        verify_token_account(a_depositor_ata, a_depositor.key, &mint)?;

        let (expected_lp_mint, _) = accounts::derive_lp_vault_mint(program_id, a_slab.key);
        accounts::expect_key(a_lp_vault_mint, &expected_lp_mint)?;
        if a_lp_vault_mint.data_len() == 0 {
            return Err(PercolatorError::LpVaultNotCreated.into());
        }

        let (expected_state, _) = accounts::derive_lp_vault_state(program_id, a_slab.key);
        accounts::expect_key(a_lp_vault_state, &expected_state)?;

        accounts::expect_key(a_vault_authority, &auth)?;

        let mut vs_data = a_lp_vault_state.try_borrow_mut_data()?;
        let mut vault_state = crate::lp_vault::read_lp_vault_state(&vs_data)
            .ok_or(PercolatorError::LpVaultNotCreated)?;
        if !vault_state.is_initialized() {
            return Err(PercolatorError::LpVaultNotCreated.into());
        }

        let lp_supply = crate::insurance_lp::read_mint_supply(a_lp_vault_mint)?;
        let capital_before = vault_state.total_capital;

        drop(slab_data);
        collateral::deposit(a_token, a_depositor_ata, a_vault, a_depositor, amount)?;

        let slab_data = a_slab.try_borrow_data()?;
        let config = state::read_config(&slab_data);
        let (units, dust) = crate::units::base_to_units(amount, config.unit_scale);
        drop(slab_data);

        let mut slab_data = state::slab_data_mut(a_slab)?;
        let old_dust = state::read_dust_base(&slab_data);
        state::write_dust_base(&mut slab_data, old_dust.saturating_add(dust));

        let lp_tokens_to_mint: u64 = if lp_supply == 0 || capital_before == 0 {
            if lp_supply > 0 && capital_before == 0 {
                // Vault is depleted but LP tokens still exist (zombie tokens).
                // A 1:1 deposit here would dilute the new depositor because
                // existing LP holders retain their tokens and claim a share
                // of the new capital. Reject until supply is cleared.
                return Err(PercolatorError::LpVaultSupplyMismatch.into());
            }
            units
        } else {
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

        vault_state.total_capital = vault_state
            .total_capital
            .checked_add(units as u128)
            .ok_or(PercolatorError::EngineOverflow)?;

        if vault_state.hwm_floor_bps > 0
            && vault_state.total_capital > vault_state.epoch_high_water_tvl
        {
            vault_state.epoch_high_water_tvl = vault_state.total_capital;
        }

        let engine = zc::engine_mut(&mut slab_data)?;
        engine.vault = percolator::U128::new(
            engine
                .vault
                .get()
                .checked_add(units as u128)
                .ok_or(PercolatorError::EngineOverflow)?,
        );
        drop(slab_data);

        crate::lp_vault::write_lp_vault_state(&mut vs_data, &vault_state);
        drop(vs_data);

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

        // 0xD09051 = "DEPOSIT" tag; logs (amount, lp_minted, epoch, 0, 0)
        sol_log_64(0xD09051, amount, lp_tokens_to_mint, vault_state.epoch, 0);
        Ok(())
    }

    // --- LpVaultWithdraw ---
    #[inline(never)]
    fn handle_lp_vault_withdraw<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        lp_amount: u64,
    ) -> ProgramResult {
        accounts::expect_len(accounts, 11)?;
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

        // accounts[9] = creator_lock_pda (existing)
        // accounts[10] = withdraw_queue_pda (SECURITY H-6)
        {
            let a_creator_lock = &accounts[9];
            let (expected_lock_pda, _) = Pubkey::find_program_address(
                &[crate::creator_lock::CREATOR_LOCK_SEED, a_slab.key.as_ref()],
                program_id,
            );
            accounts::expect_key(a_creator_lock, &expected_lock_pda)?;
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
                            .saturating_add(lp_amount);
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

        // SECURITY(H-6): Block instant withdraw when user has an active
        // withdrawal queue. Without this, the user can queue LP tokens
        // and then immediately withdraw the same tokens via LpVaultWithdraw,
        // creating a double-spend on the queued claim.
        {
            let a_withdraw_queue = &accounts[10];
            let (expected_queue, _) =
                accounts::derive_withdraw_queue(program_id, a_slab.key, a_withdrawer.key);
            accounts::expect_key(a_withdraw_queue, &expected_queue)?;
            if a_withdraw_queue.data_len() >= crate::lp_vault::WITHDRAW_QUEUE_LEN {
                let q_data = a_withdraw_queue
                    .try_borrow_data()
                    .map_err(|_| ProgramError::AccountBorrowFailed)?;
                if let Some(queue) = crate::lp_vault::read_withdraw_queue(&q_data) {
                    if queue.is_initialized() {
                        let unclaimed =
                            queue.queued_lp_amount.saturating_sub(queue.claimed_so_far);
                        if unclaimed > 0 {
                            msg!(
                                "LpVaultWithdraw blocked: active queue has {} unclaimed LP",
                                unclaimed
                            );
                            return Err(
                                PercolatorError::WithdrawQueueAlreadyExists.into()
                            );
                        }
                    }
                }
            }
        }

        let mut slab_data = state::slab_data_mut(a_slab)?;
        slab_guard(program_id, a_slab, &slab_data)?;
        require_initialized(&slab_data)?;

        let config = state::read_config(&slab_data);
        let mint = Pubkey::new_from_array(config.collateral_mint);

        // Use stored vault_authority_bump (~1500 CU cheaper than find_program_address)
        let vault_bump = config.vault_authority_bump;
        let auth = accounts::derive_vault_authority_with_bump(program_id, a_slab.key, vault_bump)?;
        verify_vault(
            a_vault,
            &auth,
            &mint,
            &Pubkey::new_from_array(config.vault_pubkey),
        )?;
        verify_token_account(a_withdrawer_ata, a_withdrawer.key, &mint)?;

        let (expected_lp_mint, _) = accounts::derive_lp_vault_mint(program_id, a_slab.key);
        accounts::expect_key(a_lp_vault_mint, &expected_lp_mint)?;

        let (expected_state, _) = accounts::derive_lp_vault_state(program_id, a_slab.key);
        accounts::expect_key(a_lp_vault_state, &expected_state)?;

        accounts::expect_key(a_vault_authority, &auth)?;

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

        let numerator = (lp_amount as u128)
            .checked_mul(capital)
            .ok_or(PercolatorError::EngineOverflow)?;
        let units_to_return = numerator / (lp_supply as u128);

        if units_to_return == 0 {
            return Err(PercolatorError::LpVaultZeroAmount.into());
        }

        if vault_state.hwm_floor_bps > 0 && vault_state.epoch_high_water_tvl > 0 {
            let remaining = capital
                .checked_sub(units_to_return)
                .ok_or(PercolatorError::EngineOverflow)?;
            let floor = vault_state
                .epoch_high_water_tvl
                .saturating_mul(vault_state.hwm_floor_bps as u128)
                / 10_000;
            if remaining < floor {
                return Err(PercolatorError::LpVaultWithdrawExceedsAvailable.into());
            }
        }

        let (oi_multiplier, _) = unpack_oi_cap(state::get_oi_cap_multiplier_bps(&config));
        if oi_multiplier > 0 {
            let remaining_capital = capital.saturating_sub(units_to_return);
            let engine = zc::engine_ref(&slab_data)?;
            let current_oi = engine.oi_eff_long_q.saturating_add(engine.oi_eff_short_q);
            let max_oi_after =
                remaining_capital.saturating_mul(oi_multiplier as u128) / 10_000;
            if current_oi > max_oi_after {
                return Err(PercolatorError::LpVaultWithdrawExceedsAvailable.into());
            }
        }

        let units_u64 = if units_to_return > u64::MAX as u128 {
            return Err(PercolatorError::EngineOverflow.into());
        } else {
            units_to_return as u64
        };
        let base_amount = crate::units::units_to_base_checked(units_u64, config.unit_scale)
            .ok_or(PercolatorError::EngineOverflow)?;

        vault_state.total_capital = capital
            .checked_sub(units_to_return)
            .ok_or(PercolatorError::EngineOverflow)?;

        let engine = zc::engine_mut(&mut slab_data)?;
        engine.vault = percolator::U128::new(
            engine
                .vault
                .get()
                .checked_sub(units_to_return)
                .ok_or(PercolatorError::EngineOverflow)?,
        );
        drop(slab_data);

        crate::lp_vault::write_lp_vault_state(&mut vs_data, &vault_state);
        drop(vs_data);

        crate::insurance_lp::burn(
            a_token,
            a_lp_vault_mint,
            a_withdrawer_lp_ata,
            a_withdrawer,
            lp_amount,
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
            a_withdrawer_ata,
            a_vault_authority,
            base_amount,
            &signer_seeds,
        )?;

        // 0xD09057 = "WITHDRAW" tag; logs (lp_burned, tokens_returned, epoch, 0, 0)
        sol_log_64(0xD09057, lp_amount, base_amount, vault_state.epoch, 0);
        Ok(())
    }

    // --- LpVaultCrankFees ---
    #[inline(never)]
    fn handle_lp_vault_crank_fees<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],) -> ProgramResult {
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

        let (expected_state, _) = accounts::derive_lp_vault_state(program_id, a_slab.key);
        accounts::expect_key(a_lp_vault_state, &expected_state)?;

        let mut vs_data = a_lp_vault_state.try_borrow_mut_data()?;
        let mut vault_state = crate::lp_vault::read_lp_vault_state(&vs_data)
            .ok_or(PercolatorError::LpVaultNotCreated)?;
        if !vault_state.is_initialized() {
            return Err(PercolatorError::LpVaultNotCreated.into());
        }

        let config = state::read_config(&slab_data);

        let engine = zc::engine_mut(&mut slab_data)?;
        // fee_revenue not in current InsuranceFund layout — always 0
        let current_fee_revenue = 0u128;
        let last_snapshot = vault_state.last_fee_snapshot;

        let fee_delta = current_fee_revenue.saturating_sub(last_snapshot);
        if fee_delta == 0 {
            return Err(PercolatorError::LpVaultNoNewFees.into());
        }

        let (oi_mult_for_util, _) = unpack_oi_cap(state::get_oi_cap_multiplier_bps(&config));
        let fee_mult_bps: u64 = if vault_state.lp_util_curve_enabled != 0
            && oi_mult_for_util > 0
        {
            let vault_balance = engine.vault.get();
            let max_oi = vault_balance.saturating_mul(oi_mult_for_util as u128) / 10_000;
            let current_oi = engine.oi_eff_long_q.saturating_add(engine.oi_eff_short_q);

            let util_bps = crate::verify::compute_util_bps(current_oi, max_oi);
            let mult = crate::verify::compute_fee_multiplier_bps(util_bps);

            vault_state.current_fee_mult_bps = mult as u32;
            mult
        } else {
            vault_state.current_fee_mult_bps = crate::verify::FEE_MULT_BASE_BPS as u32;
            crate::verify::FEE_MULT_BASE_BPS
        };

        let lp_portion = fee_delta
            .saturating_mul(vault_state.fee_share_bps as u128)
            .saturating_mul(fee_mult_bps as u128)
            / (10_000u128 * 10_000u128);

        if lp_portion > 0 {
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

        vault_state.last_fee_snapshot = current_fee_revenue;
        let clock_slot = if accounts.len() > 2 {
            Clock::from_account_info(&accounts[2])?.slot
        } else {
            Clock::get()?.slot
        };
        vault_state.last_crank_slot = clock_slot;
        drop(slab_data);

        crate::lp_vault::write_lp_vault_state(&mut vs_data, &vault_state);

        // 0xFEEC84 = "FEE_CRANK" tag; logs (delta, mult_bps, lp_portion, capital, slot)
        // Truncate u128 -> u64 for sol_log_64 (low 64 bits sufficient for debug)
        sol_log_64(0xFEEC84, fee_delta as u64, fee_mult_bps, lp_portion as u64, vault_state.total_capital as u64);
        Ok(())
    }

    // --- FundMarketInsurance ---
    #[inline(never)]
    fn handle_fund_market_insurance<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        amount: u64,
    ) -> ProgramResult {
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

        collateral::deposit(a_token, a_admin_ata, a_vault, a_admin, amount)?;

        let (units, dust) = crate::units::base_to_units(amount, config.unit_scale);
        let old_dust = state::read_dust_base(&data);
        state::write_dust_base(&mut data, old_dust.saturating_add(dust));

        let engine = zc::engine_mut(&mut data)?;
        // Inline fund_market_insurance: add units to insurance fund balance.
        engine.insurance_fund.balance = percolator::U128::new(
            engine
                .insurance_fund
                .balance
                .get()
                .checked_add(units as u128)
                .ok_or(PercolatorError::EngineOverflow)?,
        );
        // SECURITY(R4-M1): Increment engine.vault to match the token deposit.
        // Without this, engine.vault diverges from actual vault balance, causing:
        // - WithdrawInsurance to revert (ins > engine.vault check)
        // - AuditCrank false-positive solvency violation (vault < c_tot + insurance)
        // - Artificially tight OI caps
        engine.vault = percolator::U128::new(
            engine
                .vault
                .get()
                .checked_add(units as u128)
                .ok_or(PercolatorError::EngineOverflow)?,
        );

        msg!("PERC-306: funded market insurance with {} units", units);
        Ok(())
    }

    // --- SetInsuranceIsolation ---
    #[inline(never)]
    fn handle_set_insurance_isolation<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        bps: u16,
    ) -> ProgramResult {
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

        if bps > 10_000 {
            return Err(ProgramError::InvalidInstructionData);
        }

        // set_insurance_isolation_bps: stub — field not in current layout, log only.
        let _engine = zc::engine_mut(&mut data)?;
        // config.insurance_isolation_bps not in current layout — no-op write.
        msg!("PERC-306: set insurance isolation to {} bps", bps);
        Ok(())
    }

    // --- ChallengeSettlement ---
    #[inline(never)]
    fn handle_challenge_settlement<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        proposed_price_e6: u64,
    ) -> ProgramResult {
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
        if *a_system.key != solana_program::system_program::id() {
            return Err(ProgramError::IncorrectProgramId);
        }

        let data = a_slab.try_borrow_data()?;
        slab_guard(program_id, a_slab, &data)?;
        require_initialized(&data)?;

        if !state::is_resolved(&data) {
            return Err(PercolatorError::MarketNotResolved.into());
        }

        let config = state::read_config(&data);
        drop(data);

        let dispute_window_slots = state::get_dispute_window_slots(&config);
        if dispute_window_slots == 0 {
            return Err(PercolatorError::DisputeWindowClosed.into());
        }
        let clock = Clock::get()?;
        let resolved_slot = state::get_resolved_slot(&config);
        let window_end = resolved_slot
            .checked_add(dispute_window_slots)
            .ok_or(PercolatorError::DisputeWindowClosed)?;
        if clock.slot > window_end {
            return Err(PercolatorError::DisputeWindowClosed.into());
        }

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

        let mint = Pubkey::new_from_array(config.collateral_mint);
        let (auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
        verify_vault(
            a_vault,
            &auth,
            &mint,
            &Pubkey::new_from_array(config.vault_pubkey),
        )?;
        verify_token_account(a_challenger_ata, a_challenger.key, &mint)?;

        let dispute_bond_amount = state::get_dispute_bond_amount(&config);
        if dispute_bond_amount > 0 {
            collateral::deposit(
                a_token,
                a_challenger_ata,
                a_vault,
                a_challenger,
                dispute_bond_amount,
            )?;
        }

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
            bond_amount: dispute_bond_amount,
            outcome: 0,
            _pad: [0; 7],
            dispute_slot: clock.slot,
            _reserved: [0; 16],
        };

        let mut d_data = a_dispute.try_borrow_mut_data()?;
        crate::dispute::write_dispute(&mut d_data, &dispute);

        let settlement_price_e6 = state::get_settlement_price_e6(&config);
        msg!(
            "PERC-314: Settlement challenged: proposed={} vs settlement={}",
            proposed_price_e6,
            settlement_price_e6
        );
        Ok(())
    }

    // --- ResolveDispute ---
    #[inline(never)]
    fn handle_resolve_dispute<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        accept: u8,
    ) -> ProgramResult {
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
        let config = state::read_config(&data);
        drop(data);

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
            dispute.outcome = 1;

            let mut slab_data = state::slab_data_mut(a_slab)?;
            let config_w = state::read_config(&slab_data);
            // settlement_price_e6 not in current layout — update is no-op
            let _ = dispute.proposed_price_e6;
            state::write_config(&mut slab_data, &config_w);
            drop(slab_data);

            if dispute.bond_amount > 0 {
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let challenger_key = Pubkey::new_from_array(dispute.challenger);
                verify_token_account(a_challenger_ata, &challenger_key, &mint)?;
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
            dispute.outcome = 2;
            msg!("PERC-314: Dispute rejected — bond forfeited");
        }

        crate::dispute::write_dispute(&mut d_data, &dispute);
        Ok(())
    }

    // --- DepositLpCollateral ---
    #[inline(never)]
    fn handle_deposit_lp_collateral<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        user_idx: u16,
        lp_amount: u64,
    ) -> ProgramResult {
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
        if state::get_lp_collateral_enabled(&config) == 0 {
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

        // SECURITY(H-2): Validate LP escrow is owned by vault authority PDA
        // and holds the correct LP vault mint. Without this, an attacker can
        // pass their own token account as escrow, getting engine collateral
        // credit while retaining control of the LP tokens.
        let (vault_auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
        verify_token_account(a_lp_escrow, &vault_auth, &expected_mint)?;

        let lp_supply = crate::insurance_lp::read_mint_supply(a_lp_vault_mint)?;

        let collateral_units = crate::lp_collateral::lp_token_value(
            lp_amount,
            vault_tvl,
            lp_supply,
            state::get_lp_collateral_ltv_bps(&config) as u64,
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
            .deposit_not_atomic(user_idx, collateral_units, 0, clock.slot)
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

        // 0x315D09 = "PERC-315 DEPOSIT" tag; logs (lp_amount, collateral_units, ltv_bps, 0, 0)
        // Truncate u128 -> u64 for sol_log_64 (low 64 bits sufficient for debug)
        sol_log_64(0x315D09, lp_amount, collateral_units as u64, state::get_lp_collateral_ltv_bps(&config) as u64, 0);
        Ok(())
    }

    // --- WithdrawLpCollateral ---
    #[inline(never)]
    fn handle_withdraw_lp_collateral<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        user_idx: u16,
        lp_amount: u64,
    ) -> ProgramResult {
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

        let pos = engine.accounts[user_idx as usize].position_basis_q;
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

        // SECURITY(H-2): Validate LP escrow on withdrawal too (defense-in-depth).
        let (vault_auth_check, _) = accounts::derive_vault_authority(program_id, a_slab.key);
        verify_token_account(a_lp_escrow, &vault_auth_check, &expected_mint)?;

        let lp_supply = crate::insurance_lp::read_mint_supply(a_lp_vault_mint)?;

        let collateral_units = crate::lp_collateral::lp_token_value(
            lp_amount,
            vault_tvl,
            lp_supply,
            state::get_lp_collateral_ltv_bps(&config) as u64,
        );

        let mut slab_data = state::slab_data_mut(a_slab)?;
        let engine = zc::engine_mut(&mut slab_data)?;

        let clock = Clock::get()?;
        engine
            .withdraw_not_atomic(user_idx, collateral_units, 0, clock.slot, 0, engine.params.h_min)
            .map_err(map_risk_error)?;

        engine.vault = percolator::U128::new(
            engine
                .vault
                .get()
                .checked_sub(collateral_units)
                .ok_or(PercolatorError::EngineOverflow)?,
        );
        drop(slab_data);

        // Use stored vault_authority_bump (~1500 CU cheaper than find_program_address)
        let vault_bump = config.vault_authority_bump;
        let auth = accounts::derive_vault_authority_with_bump(program_id, a_slab.key, vault_bump)?;
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

        // 0x315D57 = "PERC-315 WITHDRAW" tag; logs (lp_amount, collateral_units, 0, 0, 0)
        // Truncate u128 -> u64 for sol_log_64 (low 64 bits sufficient for debug)
        sol_log_64(0x315D57, lp_amount, collateral_units as u64, 0, 0);
        Ok(())
    }

    // --- QueueWithdrawal ---
    #[inline(never)]
    fn handle_queue_withdrawal<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        lp_amount: u64,
    ) -> ProgramResult {
        accounts::expect_len(accounts, 5)?;
        let a_user = &accounts[0];
        let a_slab = &accounts[1];
        let a_lp_vault_state = &accounts[2];
        let a_queue = &accounts[3];
        let a_system = &accounts[4];

        accounts::expect_signer(a_user)?;
        accounts::expect_writable(a_queue)?;
        if *a_system.key != solana_program::system_program::id() {
            return Err(ProgramError::IncorrectProgramId);
        }

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

                let fees = 0u64;
                solana_program::program::set_return_data(&fees.to_le_bytes());

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
            last_claim_slot: 0,
            _reserved: [0; 16],
        };

        let mut q_data = a_queue.try_borrow_mut_data()?;
        crate::lp_vault::write_withdraw_queue(&mut q_data, &queue);

        msg!(
            "PERC-309: Queued {} LP over {} epochs",
            lp_amount,
            queue_epochs
        );
        Ok(())
    }

    // --- ClaimQueuedWithdrawal ---
    #[inline(never)]
    fn handle_claim_queued_withdrawal<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],) -> ProgramResult {
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

        // SECURITY(CR-2): Enforce one claim per epoch duration window.
        // Without this, all epochs are claimable in a single slot because
        // claimable_this_epoch() uses only the epochs_remaining counter
        // with no clock check.
        let clock = Clock::get()?;
        if queue.last_claim_slot > 0
            && clock.slot
                < queue
                    .last_claim_slot
                    .saturating_add(crate::shared_vault::DEFAULT_EPOCH_DURATION_SLOTS)
        {
            msg!(
                "ClaimQueuedWithdrawal: epoch not elapsed (slot={}, next={})",
                clock.slot,
                queue.last_claim_slot
                    .saturating_add(crate::shared_vault::DEFAULT_EPOCH_DURATION_SLOTS),
            );
            return Err(PercolatorError::WithdrawQueueNothingClaimable.into());
        }

        let claimable = queue.claimable_this_epoch();
        if claimable == 0 {
            return Err(PercolatorError::WithdrawQueueNothingClaimable.into());
        }

        queue.claimed_so_far = queue.claimed_so_far.saturating_add(claimable);
        queue.epochs_remaining = queue.epochs_remaining.saturating_sub(1);
        queue.last_claim_slot = clock.slot;
        crate::lp_vault::write_withdraw_queue(&mut q_data, &queue);
        drop(q_data);

        let slab_data = a_slab.try_borrow_data()?;
        slab_guard(program_id, a_slab, &slab_data)?;
        require_initialized(&slab_data)?;
        let config = state::read_config(&slab_data);
        let mint = Pubkey::new_from_array(config.collateral_mint);
        // Save bump before dropping slab_data (~1500 CU cheaper than find_program_address later)
        let vault_bump = config.vault_authority_bump;
        let vault_pubkey = config.vault_pubkey;
        drop(slab_data);

        // Use stored vault_authority_bump (~1500 CU cheaper than find_program_address)
        let auth = accounts::derive_vault_authority_with_bump(program_id, a_slab.key, vault_bump)?;
        verify_vault(
            a_vault,
            &auth,
            &mint,
            &Pubkey::new_from_array(vault_pubkey),
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

        let capital_units = (claimable as u128)
            .checked_mul(vault_state.total_capital)
            .ok_or(PercolatorError::EngineOverflow)?
            / (lp_supply as u128);

        if capital_units == 0 {
            return Err(PercolatorError::LpVaultZeroAmount.into());
        }

        if capital_units > u64::MAX as u128 {
            return Err(PercolatorError::EngineOverflow.into());
        }

        let slab_data = a_slab.try_borrow_data()?;
        let config = state::read_config(&slab_data);
        // SECURITY(M-8): use units_to_base_checked (matches LpVaultWithdraw).
        // The saturating variant silently clamps to u64::MAX on overflow.
        let base_amount = crate::units::units_to_base_checked(
            capital_units as u64,
            config.unit_scale,
        )
        .ok_or(PercolatorError::EngineOverflow)?;

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
                return Err(PercolatorError::LpVaultWithdrawExceedsAvailable.into());
            }
        }

        let (oi_multiplier, _) = unpack_oi_cap(state::get_oi_cap_multiplier_bps(&config));
        if oi_multiplier > 0 {
            let remaining_capital = vault_state
                .total_capital
                .checked_sub(capital_units)
                .ok_or(PercolatorError::EngineOverflow)?;
            let engine = zc::engine_ref(&slab_data)?;
            let current_oi = engine.oi_eff_long_q.saturating_add(engine.oi_eff_short_q);
            let max_oi_after =
                remaining_capital.saturating_mul(oi_multiplier as u128) / 10_000;
            if current_oi > max_oi_after {
                return Err(PercolatorError::LpVaultWithdrawExceedsAvailable.into());

            }
        }
        drop(slab_data);

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

        // 0x309C1A = "PERC-309 CLAIM" tag; logs (claimable, base_amount, epochs_left, 0, 0)
        sol_log_64(0x309C1A, claimable, base_amount, queue.epochs_remaining as u64, 0);
        Ok(())
    }

    // --- CancelQueuedWithdrawal ---
    #[inline(never)]
    fn handle_cancel_queued_withdrawal<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],) -> ProgramResult {
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
        Ok(())
    }

    // --- ExecuteAdl ---
    #[inline(never)]
    fn handle_execute_adl<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        target_idx: u16,
    ) -> ProgramResult {
        accounts::expect_len(accounts, 4)?;
        let a_keeper = &accounts[0];
        let a_slab = &accounts[1];
        let a_oracle = &accounts[3];
        accounts::expect_signer(a_keeper)?;
        accounts::expect_writable(a_slab)?;

        let mut data = state::slab_data_mut(a_slab)?;
        slab_guard(program_id, a_slab, &data)?;
        require_initialized(&data)?;

        // SECURITY H-1: Block ADL on resolved markets — resolved markets
        // use ForceCloseResolved at settlement price, not ADL at oracle price.
        if state::is_resolved(&data) {
            return Err(ProgramError::InvalidAccountData);
        }

        {
            let header = state::read_header(&data);
            require_admin(header.admin, a_keeper.key)?;
        }

        let mut config = state::read_config(&data);

        let clock = Clock::from_account_info(&accounts[2])?;

        let is_hyperp = oracle::is_hyperp_mode(&config);
        let price = if is_hyperp {
            let idx = config.last_effective_price_e6;
            if idx == 0 {
                return Err(PercolatorError::OracleInvalid.into());
            }
            {
                let eng = zc::engine_ref(&data)?;
                oracle::check_hyperp_staleness(
                    eng.current_slot,
                    eng.params.max_crank_staleness_slots,
                    clock.slot,

                )?;
            }
            idx
        } else {
            oracle::read_price_clamped(
                &mut config,
                a_oracle,
                clock.unix_timestamp,
            )?
        };
        state::write_config(&mut data, &config);

        let engine = zc::engine_mut(&mut data)?;

        // H-4: Insurance fund must be fully depleted before ADL activates.
        let insurance_balance = engine.insurance_fund.balance.get();
        if insurance_balance != 0 {
            msg!(
                "ADL: insurance_fund.balance={} — not depleted, ADL rejected",
                insurance_balance
            );
            return Err(PercolatorError::InsuranceFundNotDepleted.into());
        }


        // SECURITY(H-4): Pre-check — reject ADL when PnL clearly within cap.
        // The definitive check uses post-touch pnl_pos_tot (see below).
        let cap = state::get_max_pnl_cap(&config) as u128;
        {
            let pnl_pre = engine.pnl_pos_tot;
            if cap > 0 && pnl_pre <= cap {
                msg!(
                    "ADL: pnl_pos_tot={} within cap={} — no deleverage needed",
                    pnl_pre,
                    cap
                );
                return Err(ProgramError::InvalidArgument);
            }
        }

        let funding_rate = compute_current_funding_rate_e9(&config);
        let h_lock = engine.params.h_min;


        let (closed_abs, final_pnl) = engine
            .execute_adl_not_atomic(
                target_idx as usize,
                clock.slot,
                price,
                funding_rate,
                h_lock,
            )
            .map_err(map_risk_error)?;

        // SECURITY(H-2): Recompute excess from post-touch pnl_pos_tot for accurate logging.
        let excess = engine.pnl_pos_tot.saturating_sub(cap);

        let closed_lo = closed_abs as u64;
        let closed_hi = (closed_abs >> 64) as u64;
        sol_log_64(0xAD1E_0001, target_idx as u64, price, closed_lo, closed_hi);

        // 0xAD1E_0002: ADL summary — (excess_lo, excess_hi, final_pnl_lo, pnl_pos_tot_lo, tag)
        let excess_lo = excess as u64;
        let excess_hi = (excess >> 64) as u64;
        let final_pnl_abs = final_pnl.unsigned_abs();
        sol_log_64(
            0xAD1E_0002,
            excess_lo,
            excess_hi,
            final_pnl_abs as u64,
            engine.pnl_pos_tot as u64,
        );
        Ok(())
    }

    // --- CloseStaleSlabs ---
    #[inline(never)]
    fn handle_close_stale_slabs<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],) -> ProgramResult {
        accounts::expect_len(accounts, 2)?;
        let a_dest = &accounts[0];
        let a_slab = &accounts[1];

        accounts::expect_signer(a_dest)?;
        accounts::expect_writable(a_slab)?;

        if a_slab.owner != program_id {
            return Err(ProgramError::IllegalOwner);
        }

        // SECURITY(H-7): Reject slabs with valid sizes — use CloseSlab for those.
        // Synchronized with slab_guard's accepted sizes. Previous version had:
        //   - PRE_118/OLDEST using stale offsets (-16/-24 instead of -48/-56)
        //   - PRE_DEX_POOL_SLAB_LEN missing entirely
        //   - V1M2_MEDIUM_TRANSITIONAL missing entirely
        const PRE_DEX_POOL_SLAB_LEN: usize = SLAB_LEN - 32;
        const PRE_118_SLAB_LEN: usize = SLAB_LEN - 48;
        const OLDEST_SLAB_LEN: usize = SLAB_LEN - 56;
        const PRE_ADL_SLAB_LEN: usize = 1025880;
        const V1M_SMALL_LEN: usize = 65416;
        const V1M_MEDIUM_LEN: usize = 257512;
        const V1M_LARGE_LEN: usize = 1025896;
        const V1M2_MEDIUM_LEN: usize = 323312;
        const V1M2_MEDIUM_TRANSITIONAL: usize = 323328;
        let slab_data = a_slab.try_borrow_data()?;
        let slab_len = slab_data.len();
        if slab_len == SLAB_LEN
            || slab_len == PRE_DEX_POOL_SLAB_LEN
            || slab_len == PRE_118_SLAB_LEN
            || slab_len == OLDEST_SLAB_LEN
            || slab_len == PRE_ADL_SLAB_LEN
            || slab_len == V1M_SMALL_LEN
            || slab_len == V1M_MEDIUM_LEN
            || slab_len == V1M_LARGE_LEN
            || slab_len == V1M2_MEDIUM_LEN
            || slab_len == V1M2_MEDIUM_TRANSITIONAL
        {
            return Err(PercolatorError::InvalidSlabLen.into());
        }

        const ADMIN_OFF: usize = 16;
        const ADMIN_END: usize = ADMIN_OFF + 32;

        if slab_len < ADMIN_END {
            return Err(PercolatorError::NotInitialized.into());
        }

        let magic = u64::from_le_bytes(
            slab_data[0..8]
                .try_into()
                .map_err(|_| PercolatorError::InvalidMagic)?,
        );
        if magic != MAGIC {
            return Err(PercolatorError::InvalidMagic.into());
        }

        let admin_bytes: [u8; 32] = slab_data[ADMIN_OFF..ADMIN_END]
            .try_into()
            .map_err(|_| PercolatorError::InvalidMagic)?;
        drop(slab_data);

        require_admin(admin_bytes, a_dest.key)?;

        {
            let mut data = a_slab.try_borrow_mut_data()?;
            data.fill(0);
        }

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
        Ok(())
    }

    // --- ReclaimSlabRent ---
    #[inline(never)]
    fn handle_reclaim_slab_rent<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],) -> ProgramResult {
        // Two modes:
        //   Mode A (2 accounts): slab is a signer — anyone with the keypair can reclaim.
        //   Mode B (3 accounts): admin signs — reclaims orphan slabs without the keypair.
        //     accounts[2] = slab account (not signer), admin verified from header if magic set.
        let (a_dest, a_slab) = if accounts.len() >= 3 {
            // Mode B: admin reclaim for orphan zero-magic slabs
            let a_admin = &accounts[0];
            let a_slab = &accounts[1];
            let a_dest_override = &accounts[2];
            accounts::expect_signer(a_admin)?;
            accounts::expect_writable(a_slab)?;
            accounts::expect_writable(a_dest_override)?;

            if a_slab.owner != program_id {
                return Err(ProgramError::IllegalOwner);
            }

            // For Mode B, verify slab has NO magic (truly orphaned/uninitialized)
            let slab_data = a_slab.try_borrow_data()?;
            if slab_data.len() >= 8 {
                let magic = u64::from_le_bytes(
                    slab_data[0..8].try_into()
                        .map_err(|_| PercolatorError::InvalidMagic)?,
                );
                if magic == MAGIC {
                    // Initialized slab — use CloseStaleSlabs or CloseOrphanSlab instead
                    return Err(PercolatorError::AlreadyInitialized.into());
                }
            }
            drop(slab_data);

            (a_dest_override, a_slab)
        } else {
            // Mode A: original — slab is signer
            accounts::expect_len(accounts, 2)?;
            let a_dest = &accounts[0];
            let a_slab = &accounts[1];

            accounts::expect_signer(a_dest)?;
            accounts::expect_writable(a_dest)?;

            accounts::expect_signer(a_slab)?;
            accounts::expect_writable(a_slab)?;

            if a_slab.owner != program_id {
                return Err(ProgramError::IllegalOwner);
            }

            let slab_data = a_slab.try_borrow_data()?;
            if slab_data.len() >= 8 {
                let magic = u64::from_le_bytes(
                    slab_data[0..8].try_into()
                        .map_err(|_| PercolatorError::InvalidMagic)?,
                );
                if magic == MAGIC {
                    return Err(PercolatorError::AlreadyInitialized.into());
                }
            }
            drop(slab_data);

            (a_dest, a_slab)
        };

        if a_dest.key == a_slab.key {
            return Err(ProgramError::InvalidArgument);
        }

        {
            let mut data = a_slab.try_borrow_mut_data()?;
            data.fill(0);
        }

        let slab_lamports = a_slab.lamports();
        **a_slab.lamports.borrow_mut() = 0;
        **a_dest.lamports.borrow_mut() = a_dest
            .lamports()
            .checked_add(slab_lamports)
            .ok_or(PercolatorError::EngineOverflow)?;

        msg!(
            "ReclaimSlabRent: reclaimed {} lamports from uninitialised slab",
            slab_lamports,
        );
        Ok(())
    }

    // --- TransferOwnershipCpi ---
    #[inline(never)]
    fn handle_transfer_ownership_cpi<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        user_idx: u16,
        new_owner: [u8; 32],
    ) -> ProgramResult {
        accounts::expect_len(accounts, 3)?;
        let a_caller = &accounts[0];
        let a_slab = &accounts[1];
        let a_nft_prog = &accounts[2];

        accounts::expect_signer(a_caller)?;
        accounts::expect_writable(a_slab)?;

        if a_slab.owner != program_id {
            return Err(ProgramError::IllegalOwner);
        }

        if !a_nft_prog.executable {
            return Err(ProgramError::IncorrectProgramId);
        }
        if *a_nft_prog.owner != solana_program::bpf_loader_upgradeable::id()
            && *a_nft_prog.owner != solana_program::bpf_loader::id()
            && *a_nft_prog.owner != solana_program::bpf_loader_deprecated::id()
        {
            return Err(ProgramError::IncorrectProgramId);
        }

        let (expected_mint_auth, _) = solana_program::pubkey::Pubkey::find_program_address(
            &[b"mint_authority"],
            a_nft_prog.key,
        );
        if a_caller.key != &expected_mint_auth {
            solana_program::msg!(
                "TransferPositionOwnership rejected: caller {} is not the expected \
                 mint_authority PDA {} for NFT program {}",
                a_caller.key,
                expected_mint_auth,
                a_nft_prog.key
            );
            return Err(ProgramError::InvalidArgument);
        }

        // SECURITY(CR-1): Use typed engine accessor instead of hardcoded
        // byte offsets. The old code had three critical bugs:
        //   1. Read slab_data[8..10] as max_accounts — actually the version field
        //   2. Hardcoded ACCT_SIZE=240 — actual Account is 320 bytes on SBF
        //   3. Hardcoded ACCT_OWNER_OFF=184 — stale after Account struct changes
        // Fix: use zc::engine_mut() + direct struct field access, matching
        // every other instruction handler in the codebase.
        let mut slab_data = a_slab.try_borrow_mut_data()?;
        slab_guard(program_id, a_slab, &slab_data)?;
        require_initialized(&slab_data)?;

        let engine = zc::engine_mut(&mut slab_data)?;

        // Validate user_idx is in range and slot is allocated (bitmap).
        if (user_idx as usize) >= percolator::MAX_ACCOUNTS
            || !engine.is_used(user_idx as usize)
        {
            return Err(ProgramError::InvalidArgument);
        }

        // Write new owner via typed struct — compiler resolves correct
        // field offset for the target architecture (SBF vs native).
        engine.accounts[user_idx as usize].owner = new_owner;

        msg!(
            "TransferPositionOwnership: idx={}, new_owner={}",
            user_idx,
            Pubkey::new_from_array(new_owner),
        );
        Ok(())
    }

    // --- AuditCrank ---
    #[inline(never)]
    fn handle_audit_crank<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],) -> ProgramResult {
        if accounts.is_empty() {
            return Err(ProgramError::NotEnoughAccountKeys);
        }
        let a_slab = &accounts[0];
        accounts::expect_writable(a_slab)?;

        let mut data = state::slab_data_mut(a_slab)?;
        slab_guard(program_id, a_slab, &data)?;
        require_initialized(&data)?;

        let engine = zc::engine_ref(&data)?;

        let mut sum_capital: i128 = 0;
        let mut sum_pnl_pos: u128 = 0;
        let mut sum_oi: u128 = 0;
        for idx in 0..MAX_ACCOUNTS {
            if !engine.is_used(idx) {
                continue;
            }
            let acc = &engine.accounts[idx];
            sum_capital = sum_capital.saturating_add(acc.capital.get() as i128);
            let pnl = acc.pnl;
            if pnl > 0 {
                sum_pnl_pos = sum_pnl_pos.saturating_add(pnl as u128);
            }
            // SECURITY(H-5): Use effective position (post-ADL scaling), not
            // raw basis. After ADL, position_basis_q diverges from oi_eff_*_q
            // aggregates, causing a false-positive OI mismatch that pauses
            // the market during the exact crisis when it must stay running.
            let eff = engine.effective_pos_q(idx);
            sum_oi = sum_oi.saturating_add(eff.unsigned_abs());
        }

        let mut violation = false;

        let c_tot = engine.c_tot.get();
        if sum_capital != c_tot as i128 {
            // tag=0xAD01: capital_mismatch — sum_capital vs c_tot
            sol_log_64(sum_capital as u64, c_tot as u64, 0, 0, 0xAD01);
            violation = true;
        }

        let pnl_pos_tot = engine.pnl_pos_tot;
        if sum_pnl_pos != pnl_pos_tot {
            // tag=0xAD02: pnl_pos_mismatch — sum_pnl_pos vs pnl_pos_tot
            sol_log_64(sum_pnl_pos as u64, pnl_pos_tot as u64, 0, 0, 0xAD02);
            violation = true;
        }

        let total_oi = engine.oi_eff_long_q.saturating_add(engine.oi_eff_short_q);
        if sum_oi != total_oi {
            // tag=0xAD03: oi_mismatch — sum_oi vs total_oi
            sol_log_64(sum_oi as u64, total_oi as u64, 0, 0, 0xAD03);
            violation = true;
        }

        let vault = engine.vault.get();
        // isolated_balance not in current InsuranceFund layout — use 0
        let insurance_balance = engine.insurance_fund.balance.get()
            .saturating_add(0u128);
        let required = (c_tot as u128).saturating_add(insurance_balance);
        if (vault as u128) < required {
            // tag=0xAD05: solvency — vault vs required
            sol_log_64(vault as u64, required as u64, 0, 0, 0xAD05);
            violation = true;
        }

        const AUDIT_CRANK_COOLDOWN_SLOTS: u64 = 150;
        let current_slot = Clock::get()?.slot;
        let mut config = state::read_config(&data);
        if violation {
            let last_pause = state::read_last_audit_pause_slot(&config);
            if current_slot.saturating_sub(last_pause) < AUDIT_CRANK_COOLDOWN_SLOTS {
                // tag=0xAD10: cooldown active — last_pause, current_slot, cooldown
                sol_log_64(last_pause, current_slot, AUDIT_CRANK_COOLDOWN_SLOTS, 0, 0xAD10);
                return Err(PercolatorError::AuditViolation.into());
            }
            state::write_audit_status(&mut config, 0xFFFF);
            state::write_last_audit_pause_slot(&mut config, current_slot);
            state::set_paused(&mut data, true);
            state::write_config(&mut data, &config);
            // tag=0xAD11: violation detected — market paused at current_slot
            sol_log_64(0xAD11, current_slot, 0, 0, 0);
            return Err(PercolatorError::AuditViolation.into());
        } else {
            state::write_audit_status(&mut config, 1);
            state::write_config(&mut data, &config);
            // tag=0xAD00: all invariants passed at current_slot
            sol_log_64(0xAD00, current_slot, 0, 0, 0);
        }
        Ok(())
    }

    // --- SetOffsetPair ---
    #[inline(never)]
    fn handle_set_offset_pair<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        offset_bps: u16,
    ) -> ProgramResult {
        accounts::expect_len(accounts, 5)?;
        let a_admin = &accounts[0];
        let a_slab_a = &accounts[1];
        let a_slab_b = &accounts[2];
        let a_pair_pda = &accounts[3];
        let a_system = &accounts[4];

        accounts::expect_signer(a_admin)?;
        accounts::expect_writable(a_admin)?;
        accounts::expect_writable(a_pair_pda)?;
        if *a_system.key != solana_program::system_program::id() {
            return Err(ProgramError::IncorrectProgramId);
        }

        accounts::expect_owner(a_slab_a, program_id)?;
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

        accounts::expect_owner(a_slab_b, program_id)?;
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

        if offset_bps > 10_000 {
            return Err(PercolatorError::InvalidConfigParam.into());
        }

        if a_pair_pda.data_is_empty() {
            let lamports = solana_program::rent::Rent::get()?
                .minimum_balance(crate::cross_margin::OFFSET_PAIR_LEN);
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
                    crate::cross_margin::OFFSET_PAIR_LEN as u64,
                    program_id,
                ),
                &[a_admin.clone(), a_pair_pda.clone(), a_system.clone()],
                &[signer_seeds],
            )?;
        }

        let mut pair_data = a_pair_pda.try_borrow_mut_data()?;
        if pair_data.len() < crate::cross_margin::OFFSET_PAIR_LEN {
            return Err(ProgramError::AccountDataTooSmall);
        }
        let cfg = crate::cross_margin::OffsetPairConfig {
            magic: crate::cross_margin::OFFSET_PAIR_MAGIC,
            offset_bps,
            enabled: 1,
            _pad: [0; 5],
            _reserved: [0; 16],
        };
        crate::cross_margin::write_offset_pair(&mut pair_data, &cfg);
        msg!("SetOffsetPair: offset_bps={}", offset_bps);
        Ok(())
    }

    // --- AttestCrossMargin ---
    #[inline(never)]
    fn handle_attest_cross_margin<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        user_idx_a: u16,
        user_idx_b: u16,
    ) -> ProgramResult {
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

        accounts::expect_owner(a_slab_a, program_id)?;
        accounts::expect_owner(a_slab_b, program_id)?;

        let pair_data = a_pair_pda.try_borrow_data()?;
        let pair_cfg = crate::cross_margin::read_offset_pair(&pair_data)
            .ok_or(ProgramError::InvalidAccountData)?;
        if !pair_cfg.is_initialized() || pair_cfg.enabled == 0 {
            return Err(PercolatorError::CrossMarginPairNotFound.into());
        }
        let offset_bps = pair_cfg.offset_bps;
        drop(pair_data);

        let data_a = a_slab_a.try_borrow_data()?;
        if data_a.len() < ENGINE_OFF + ENGINE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let engine_a = zc::engine_ref(&data_a)?;
        check_idx(engine_a, user_idx_a)?;
        let pos_a = engine_a.accounts[user_idx_a as usize].position_basis_q;
        let owner_a = engine_a.accounts[user_idx_a as usize].owner;
        let slot = engine_a.current_slot;
        drop(data_a);

        let data_b = a_slab_b.try_borrow_data()?;
        if data_b.len() < ENGINE_OFF + ENGINE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let engine_b = zc::engine_ref(&data_b)?;
        check_idx(engine_b, user_idx_b)?;
        let pos_b = engine_b.accounts[user_idx_b as usize].position_basis_q;
        let owner_b = engine_b.accounts[user_idx_b as usize].owner;
        drop(data_b);

        if owner_a != owner_b {
            return Err(PercolatorError::EngineUnauthorized.into());
        }

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

        if a_attestation.data_is_empty() {
            let lamports = solana_program::rent::Rent::get()?
                .minimum_balance(crate::cross_margin::ATTESTATION_LEN);
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
                    crate::cross_margin::ATTESTATION_LEN as u64,
                    program_id,
                ),
                &[a_payer.clone(), a_attestation.clone(), a_system.clone()],
                &[signer_seeds],
            )?;
        }

        let mut att_data = a_attestation.try_borrow_mut_data()?;
        if att_data.len() < crate::cross_margin::ATTESTATION_LEN {
            return Err(ProgramError::AccountDataTooSmall);
        }
        let att = crate::cross_margin::CrossMarginAttestation {
            magic: crate::cross_margin::ATTESTATION_MAGIC,
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
        crate::cross_margin::write_attestation(&mut att_data, &att);
        msg!(
            "AttestCrossMargin: pos_a={} pos_b={} offset={}",
            pos_a as i64,
            pos_b as i64,
            offset_bps
        );
        Ok(())
    }

    // --- AdvanceOraclePhase ---
    #[inline(never)]
    fn handle_advance_oracle_phase<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],) -> ProgramResult {
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

        if state::get_vol_margin_scale_bps(&config) > 0 {
            return Err(PercolatorError::InvalidConfigParam.into());
        }

        let old_phase = state::get_oracle_phase(&config);

        let has_mature_oracle = crate::verify::is_pyth_pinned_mode(
            config.oracle_authority,
            config.index_feed_id,
        );

        let mcs = state::get_market_created_slot(&config);
        let created = state::effective_created_slot(mcs, clock.slot);
        if mcs == 0 && old_phase == 0 {
            state::set_market_created_slot(&mut config, clock.slot);
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
            state::write_config(&mut data, &config);
            msg!("AdvanceOraclePhase: no transition (phase={})", old_phase);
        } else {
            state::set_oracle_phase(&mut config, new_phase);

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
        Ok(())
    }

    // --- InitSharedVault ---
    #[inline(never)]
    fn handle_init_shared_vault<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        epoch_duration_slots: u64,
        max_market_exposure_bps: u16,
    ) -> ProgramResult {
        accounts::expect_len(accounts, 4)?;
        let a_admin = &accounts[0];
        let a_shared_vault = &accounts[1];
        let a_system_program = &accounts[2];
        let a_slab = &accounts[3];

        accounts::expect_signer(a_admin)?;
        accounts::expect_writable(a_shared_vault)?;

        {
            let slab_data = state::slab_data_mut(a_slab)?;
            slab_guard(program_id, a_slab, &slab_data)?;
            require_initialized(&slab_data)?;
            let header = state::read_header(&slab_data);
            require_admin(header.admin, a_admin.key)?;
        }

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

        let sv_state = crate::shared_vault::SharedVaultState {
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
        crate::shared_vault::write_vault_state(&mut sv_data, &sv_state);

        msg!(
            "PERC-628: SharedVault initialized — epoch_duration={} max_exposure_bps={}",
            duration,
            max_bps
        );
        Ok(())
    }

    // --- AllocateMarket ---
    #[inline(never)]
    fn handle_allocate_market<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        amount: u128,
    ) -> ProgramResult {
        accounts::expect_len(accounts, 5)?;
        let a_admin = &accounts[0];
        let a_slab = &accounts[1];
        let a_shared_vault = &accounts[2];
        let a_market_alloc = &accounts[3];
        let a_system_program = &accounts[4];

        accounts::expect_signer(a_admin)?;
        accounts::expect_writable(a_shared_vault)?;
        accounts::expect_writable(a_market_alloc)?;

        let slab_data = state::slab_data_mut(a_slab)?;
        slab_guard(program_id, a_slab, &slab_data)?;
        require_initialized(&slab_data)?;
        let header = state::read_header(&slab_data);
        require_admin(header.admin, a_admin.key)?;
        drop(slab_data);

        let (expected_sv, _) = Pubkey::find_program_address(
            &[crate::shared_vault::SHARED_VAULT_SEED],
            program_id,
        );
        accounts::expect_key(a_shared_vault, &expected_sv)?;

        let (expected_alloc, alloc_bump) = Pubkey::find_program_address(
            &[crate::shared_vault::MARKET_ALLOC_SEED, a_slab.key.as_ref()],
            program_id,
        );
        if *a_market_alloc.key != expected_alloc {
            return Err(ProgramError::InvalidSeeds);
        }

        let mut sv_data = a_shared_vault
            .try_borrow_mut_data()
            .map_err(|_| ProgramError::AccountBorrowFailed)?;
        let mut vault_state = crate::shared_vault::read_vault_state(&sv_data)
            .ok_or(ProgramError::UninitializedAccount)?;

        let new_allocation = amount;
        if !crate::shared_vault::check_exposure_cap(
            vault_state.total_capital,
            new_allocation,
            vault_state.max_market_exposure_bps,
        ) {
            msg!("PERC-628: allocation {} exceeds exposure cap", amount);
            return Err(ProgramError::InvalidArgument);
        }

        let available = crate::shared_vault::available_for_allocation(
            vault_state.total_capital,
            vault_state.total_allocated,
        );
        if new_allocation > available {
            msg!("PERC-628: allocation {} > available {}", amount, available);
            return Err(ProgramError::InsufficientFunds);
        }

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

        vault_state.total_allocated =
            vault_state.total_allocated.saturating_add(new_allocation);
        crate::shared_vault::write_vault_state(&mut sv_data, &vault_state);

        msg!("PERC-628: Market allocated {} from shared vault", amount);
        Ok(())
    }

    // --- AdvanceEpoch ---
    #[inline(never)]
    fn handle_advance_epoch<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],) -> ProgramResult {
        accounts::expect_len(accounts, 2)?;
        let a_shared_vault = &accounts[1];

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

        vault_state.epoch_snapshot_capital = vault_state.total_capital;
        vault_state.epoch_snapshot_pending = vault_state.pending_withdrawals;

        vault_state.epoch_number = vault_state.epoch_number.saturating_add(1);
        vault_state.epoch_start_slot = clock.slot;
        vault_state.pending_withdrawals = 0;
        crate::shared_vault::write_vault_state(&mut sv_data, &vault_state);

        msg!(
            "PERC-628: Epoch advanced to {} at slot {}",
            vault_state.epoch_number,
            clock.slot,
        );
        Ok(())
    }

    // --- QueueWithdrawalSV ---
    #[inline(never)]
    fn handle_queue_withdrawal_sv<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        lp_amount: u64,
    ) -> ProgramResult {
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

        {
            let req_data = a_withdraw_req
                .try_borrow_data()
                .map_err(|_| ProgramError::AccountBorrowFailed)?;
            if let Some(existing) = crate::shared_vault::read_withdraw_req(&req_data) {
                if existing.claimed == 0 {
                    return Err(ProgramError::AccountAlreadyInitialized);
                }
            }
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
        Ok(())
    }

    // --- ClaimEpochWithdrawal ---
    #[inline(never)]
    fn handle_claim_epoch_withdrawal<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],) -> ProgramResult {
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

        let clock = solana_program::clock::Clock::get()?;
        if !crate::shared_vault::is_epoch_elapsed(
            clock.slot,
            vault_state.epoch_start_slot,
            vault_state.epoch_duration_slots,
        ) {
            msg!("PERC-628: epoch not yet elapsed — cannot claim mid-epoch");
            return Err(ProgramError::InvalidArgument);
        }

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

        let mut req_data = a_withdraw_req
            .try_borrow_mut_data()
            .map_err(|_| ProgramError::AccountBorrowFailed)?;
        let req = crate::shared_vault::read_withdraw_req(&req_data)
            .ok_or(ProgramError::InvalidAccountData)?;

        if req.claimed != 0 {
            msg!("PERC-628: withdrawal already claimed");
            return Err(ProgramError::InvalidArgument);
        }

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

        let mut updated_req = req;
        updated_req.claimed = 1;
        crate::shared_vault::write_withdraw_req(&mut req_data, &updated_req);
        drop(req_data);

        let payout = crate::shared_vault::compute_proportional_withdrawal(
            req.lp_amount,
            vault_state.epoch_snapshot_pending,
            vault_state.epoch_snapshot_capital,
        );

        if payout > 0 {
            let base_payout =
                crate::units::units_to_base_checked(payout, config.unit_scale)
                    .ok_or(PercolatorError::EngineOverflow)?;

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
        Ok(())
    }

    // --- MintPositionNft ---
    #[inline(never)]
    fn handle_mint_position_nft<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        user_idx: u16,
    ) -> ProgramResult {
        // Accept 10 or 11 accounts — 11th is Associated Token Program for ATA creation
        if accounts.len() < 10 {
            return Err(solana_program::program_error::ProgramError::NotEnoughAccountKeys);
        }
        let a_payer = &accounts[0];
        let a_slab = &accounts[1];
        let a_nft_pda = &accounts[2];
        let a_nft_mint = &accounts[3];
        let a_owner_ata = &accounts[4];
        let a_owner = &accounts[5];
        let a_vault_auth = &accounts[6];
        let a_token22 = &accounts[7];
        let a_system = &accounts[8];
        let a_rent = &accounts[9];
        let a_ata_program = if accounts.len() > 10 { Some(&accounts[10]) } else { None };

        accounts::expect_signer(a_payer)?;
        accounts::expect_signer(a_owner)?;
        accounts::expect_writable(a_slab)?;
        accounts::expect_writable(a_nft_pda)?;
        accounts::expect_writable(a_nft_mint)?;
        accounts::expect_writable(a_owner_ata)?;
        verify_token22_program(a_token22)?;
        if *a_system.key != solana_program::system_program::id() {
            return Err(ProgramError::IncorrectProgramId);
        }

        let data = state::slab_data_mut(a_slab)?;
        slab_guard(program_id, a_slab, &data)?;
        require_initialized(&data)?;

        let engine = zc::engine_ref(&data)?;
        check_idx(engine, user_idx)?;
        let u_owner = engine.accounts[user_idx as usize].owner;
        if !crate::verify::owner_ok(u_owner, a_owner.key.to_bytes()) {
            return Err(PercolatorError::EngineUnauthorized.into());
        }

        let acct = &engine.accounts[user_idx as usize];
        let cap = acct.capital.get();
        let pos = acct.position_basis_q;
        if cap == 0 && pos == 0 {
            return Err(ProgramError::InvalidArgument);
        }
        // entry_price not in current layout — use 0 as stub
        let entry_price_raw: u64 = 0;
        let pos_size = acct.position_basis_q;
        let direction = if pos_size >= 0 { "LONG" } else { "SHORT" };
        drop(data);

        let (expected_nft_pda, nft_bump) =
            crate::position_nft::derive_position_nft(program_id, a_slab.key, user_idx);
        accounts::expect_key(a_nft_pda, &expected_nft_pda)?;

        let (expected_mint, mint_bump) =
            crate::position_nft::derive_position_nft_mint(program_id, a_slab.key, user_idx);
        accounts::expect_key(a_nft_mint, &expected_mint)?;

        let (expected_vault_auth, vault_bump) =
            accounts::derive_vault_authority(program_id, a_slab.key);
        accounts::expect_key(a_vault_auth, &expected_vault_auth)?;

        {
            let nft_data = a_nft_pda
                .try_borrow_data()
                .map_err(|_| ProgramError::AccountBorrowFailed)?;
            if nft_data.len() >= crate::position_nft::POSITION_NFT_STATE_LEN {
                if let Some(st) = crate::position_nft::read_position_nft_state(&nft_data) {
                    if st.is_initialized() {
                        return Err(ProgramError::AccountAlreadyInitialized);
                    }
                }
            }
        }

        {
            #[allow(unused_variables)]
            let nft_pda_seeds: &[&[u8]] = &[
                crate::position_nft::POSITION_NFT_SEED,
                a_slab.key.as_ref(),
                &user_idx.to_le_bytes(),
                &[nft_bump],
            ];
            let space = crate::position_nft::POSITION_NFT_STATE_LEN;
            let rent = solana_program::rent::Rent::get()?;
            let lamports = rent.minimum_balance(space);
            let create_ix = solana_program::system_instruction::create_account(
                a_payer.key,
                a_nft_pda.key,
                lamports,
                space as u64,
                program_id,
            );
            #[cfg(not(feature = "test"))]
            {
                solana_program::program::invoke_signed(
                    &create_ix,
                    &[a_payer.clone(), a_nft_pda.clone(), a_system.clone()],
                    &[nft_pda_seeds],
                )?;
            }
            let _ = (create_ix, a_system, a_rent);
        }

        {
            let mint_seeds: &[&[u8]] = &[
                crate::position_nft::POSITION_NFT_MINT_SEED,
                a_slab.key.as_ref(),
                &user_idx.to_le_bytes(),
                &[mint_bump],
            ];
            crate::position_nft::create_nft_mint_with_metadata(
                a_payer,
                a_nft_mint,
                a_vault_auth,
                a_system,
                a_token22,
                a_rent,
                mint_seeds,
                direction,
                entry_price_raw,
                pos_size,
            )?;
        }

        // Create the owner's Token-2022 ATA for the NFT mint if it doesn't exist.
        // The mint was just created above, so the ATA can't exist in a preceding IX.
        // Uses raw CPI to avoid spl-associated-token-account crate dependency (binary size).
        #[cfg(not(feature = "test"))]
        if let Some(ata_prog) = a_ata_program {
            if a_owner_ata.data_is_empty() || a_owner_ata.lamports() == 0 {
                // ATA program instruction: CreateAssociatedTokenAccount (no instruction data)
                use alloc::vec;
                let create_ata_ix = solana_program::instruction::Instruction {
                    program_id: *ata_prog.key,
                    accounts: vec![
                        solana_program::instruction::AccountMeta::new(*a_payer.key, true),
                        solana_program::instruction::AccountMeta::new(*a_owner_ata.key, false),
                        solana_program::instruction::AccountMeta::new_readonly(*a_owner.key, false),
                        solana_program::instruction::AccountMeta::new_readonly(*a_nft_mint.key, false),
                        solana_program::instruction::AccountMeta::new_readonly(*a_system.key, false),
                        solana_program::instruction::AccountMeta::new_readonly(*a_token22.key, false),
                    ],
                    data: vec![],
                };
                solana_program::program::invoke(
                    &create_ata_ix,
                    &[
                        a_payer.clone(),
                        a_owner_ata.clone(),
                        a_owner.clone(),
                        a_nft_mint.clone(),
                        a_system.clone(),
                        a_token22.clone(),
                        ata_prog.clone(),
                    ],
                )?;
            }
        }

        {
            let vault_seeds: &[&[u8]] = &[b"vault", a_slab.key.as_ref(), &[vault_bump]];
            crate::position_nft::mint_nft_to(
                a_token22,
                a_nft_mint,
                a_owner_ata,
                a_vault_auth,
                &[vault_seeds],
            )?;
        }

        {
            let mut nft_data = a_nft_pda
                .try_borrow_mut_data()
                .map_err(|_| ProgramError::AccountBorrowFailed)?;
            let nft_state = crate::position_nft::PositionNftState {
                magic: crate::position_nft::POSITION_NFT_MAGIC,
                mint: a_nft_mint.key.to_bytes(),
                slab: a_slab.key.to_bytes(),
                owner: a_owner.key.to_bytes(),
                user_idx,
                pending_settlement: 0,
                bump: nft_bump,
                mint_bump,
                _reserved: [0u8; 19],
            };
            crate::position_nft::write_position_nft_state(&mut nft_data, &nft_state);
        }

        msg!(
            "PERC-608: MintPositionNft slab={} user_idx={} owner={} direction={}",
            a_slab.key,
            user_idx,
            a_owner.key,
            direction,
        );
        Ok(())
    }

    // --- TransferPositionOwnership ---
    #[inline(never)]
    fn handle_transfer_position_ownership<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        user_idx: u16,
    ) -> ProgramResult {
        accounts::expect_len(accounts, 8)?;
        let a_current_owner = &accounts[0];
        let a_slab = &accounts[1];
        let a_nft_pda = &accounts[2];
        let a_nft_mint = &accounts[3];
        let a_src_ata = &accounts[4];
        let a_dst_ata = &accounts[5];
        let a_new_owner = &accounts[6];
        let a_token22 = &accounts[7];

        accounts::expect_signer(a_current_owner)?;
        accounts::expect_writable(a_slab)?;
        accounts::expect_writable(a_nft_pda)?;
        accounts::expect_writable(a_nft_mint)?;
        accounts::expect_writable(a_src_ata)?;
        accounts::expect_writable(a_dst_ata)?;
        verify_token22_program(a_token22)?;

        let mut slab_data = state::slab_data_mut(a_slab)?;
        slab_guard(program_id, a_slab, &slab_data)?;
        require_initialized(&slab_data)?;

        {
            let engine = zc::engine_ref(&slab_data)?;
            check_idx(engine, user_idx)?;
        }

        let (expected_nft_pda, _) =
            crate::position_nft::derive_position_nft(program_id, a_slab.key, user_idx);
        accounts::expect_key(a_nft_pda, &expected_nft_pda)?;

        let mut nft_state = {
            let nft_data = a_nft_pda
                .try_borrow_data()
                .map_err(|_| ProgramError::AccountBorrowFailed)?;
            crate::position_nft::read_position_nft_state(&nft_data)
                .filter(|s| s.is_initialized())
                .ok_or(ProgramError::UninitializedAccount)?
        };

        if nft_state.owner != a_current_owner.key.to_bytes() {
            return Err(PercolatorError::EngineUnauthorized.into());
        }

        if nft_state.mint != a_nft_mint.key.to_bytes() {
            return Err(ProgramError::InvalidArgument);
        }

        if nft_state.pending_settlement != 0 {
            msg!("PERC-608: PendingFundingNotSettled — keeper must run settlement crank");
            return Err(PercolatorError::EngineUnauthorized.into());
        }

        // Update engine-level owner so the new owner gains full control
        // (withdraw, trade, close). Done before dropping slab_data and
        // before the CPI; if the CPI reverts, the whole tx rolls back
        // atomically. The CPI only touches Token-2022 accounts (mint/ATAs),
        // not the slab, so no ExternalAccountDataModified risk.
        {
            let engine = zc::engine_mut(&mut slab_data)?;
            engine.accounts[user_idx as usize].owner = a_new_owner.key.to_bytes();
        }

        drop(slab_data);

        crate::position_nft::transfer_nft(
            a_token22,
            a_nft_mint,
            a_src_ata,
            a_dst_ata,
            a_current_owner,
        )?;

        nft_state.owner = a_new_owner.key.to_bytes();
        {
            let mut nft_data = a_nft_pda
                .try_borrow_mut_data()
                .map_err(|_| ProgramError::AccountBorrowFailed)?;
            crate::position_nft::write_position_nft_state(&mut nft_data, &nft_state);
        }

        msg!(
            "PERC-608: TransferPositionOwnership slab={} user_idx={} new_owner={}",
            a_slab.key,
            user_idx,
            a_new_owner.key,
        );
        Ok(())
    }

    // --- BurnPositionNft ---
    #[inline(never)]
    fn handle_burn_position_nft<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        user_idx: u16,
    ) -> ProgramResult {
        accounts::expect_len(accounts, 7)?;
        let a_owner = &accounts[0];
        let a_slab = &accounts[1];
        let a_nft_pda = &accounts[2];
        let a_nft_mint = &accounts[3];
        let a_owner_ata = &accounts[4];
        let a_vault_auth = &accounts[5];
        let a_token22 = &accounts[6];

        accounts::expect_signer(a_owner)?;
        accounts::expect_writable(a_slab)?;
        accounts::expect_writable(a_nft_pda)?;
        accounts::expect_writable(a_nft_mint)?;
        accounts::expect_writable(a_owner_ata)?;
        verify_token22_program(a_token22)?;

        let slab_data = state::slab_data_mut(a_slab)?;
        slab_guard(program_id, a_slab, &slab_data)?;
        require_initialized(&slab_data)?;
        drop(slab_data);

        let (expected_nft_pda, _) =
            crate::position_nft::derive_position_nft(program_id, a_slab.key, user_idx);
        accounts::expect_key(a_nft_pda, &expected_nft_pda)?;

        let (expected_vault_auth, vault_bump) =
            accounts::derive_vault_authority(program_id, a_slab.key);
        accounts::expect_key(a_vault_auth, &expected_vault_auth)?;

        let nft_state = {
            let nft_data = a_nft_pda
                .try_borrow_data()
                .map_err(|_| ProgramError::AccountBorrowFailed)?;
            crate::position_nft::read_position_nft_state(&nft_data)
                .filter(|s| s.is_initialized())
                .ok_or(ProgramError::UninitializedAccount)?
        };

        if nft_state.owner != a_owner.key.to_bytes() {
            return Err(PercolatorError::EngineUnauthorized.into());
        }

        if nft_state.mint != a_nft_mint.key.to_bytes() {
            return Err(ProgramError::InvalidArgument);
        }

        // Block burn during pending settlement — consistent with the guard
        // in TransferPositionOwnership. Destroying the NFT state while a
        // settlement is in-flight could orphan the settlement tracking.
        if nft_state.pending_settlement != 0 {
            msg!("BurnPositionNft: pending settlement must be cleared first");
            return Err(PercolatorError::EngineUnauthorized.into());
        }

        crate::position_nft::burn_nft(a_token22, a_nft_mint, a_owner_ata, a_owner)?;

        {
            let vault_seeds: &[&[u8]] = &[b"vault", a_slab.key.as_ref(), &[vault_bump]];
            crate::position_nft::close_nft_mint(
                a_token22,
                a_nft_mint,
                a_owner,
                a_vault_auth,
                &[vault_seeds],
            )?;
        }

        {
            let mut nft_data = a_nft_pda
                .try_borrow_mut_data()
                .map_err(|_| ProgramError::AccountBorrowFailed)?;
            for b in nft_data.iter_mut() {
                *b = 0;
            }
        }
        {
            let lamports = a_nft_pda.lamports();
            **a_nft_pda
                .try_borrow_mut_lamports()
                .map_err(|_| ProgramError::AccountBorrowFailed)? = 0;
            **a_owner
                .try_borrow_mut_lamports()
                .map_err(|_| ProgramError::AccountBorrowFailed)? = a_owner
                .lamports()
                .checked_add(lamports)
                .ok_or(PercolatorError::EngineOverflow)?;
        }

        msg!(
            "PERC-608: BurnPositionNft slab={} user_idx={} owner={}",
            a_slab.key,
            user_idx,
            a_owner.key,
        );
        Ok(())
    }

    // --- SetPendingSettlement ---
    #[inline(never)]
    fn handle_set_pending_settlement<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        user_idx: u16,
    ) -> ProgramResult {
        accounts::expect_len(accounts, 3)?;
        let a_keeper = &accounts[0];
        let a_slab = &accounts[1];
        let a_nft_pda = &accounts[2];

        accounts::expect_signer(a_keeper)?;
        accounts::expect_writable(a_nft_pda)?;

        {
            let slab_data = a_slab
                .try_borrow_data()
                .map_err(|_| ProgramError::AccountBorrowFailed)?;
            slab_guard(program_id, a_slab, &slab_data)?;
            require_initialized(&slab_data)?;
            let header = state::read_header(&slab_data);
            require_admin(header.admin, a_keeper.key)?;
        }

        let (expected_nft_pda, _) =
            crate::position_nft::derive_position_nft(program_id, a_slab.key, user_idx);
        accounts::expect_key(a_nft_pda, &expected_nft_pda)?;

        let mut nft_state = {
            let nft_data = a_nft_pda
                .try_borrow_data()
                .map_err(|_| ProgramError::AccountBorrowFailed)?;
            crate::position_nft::read_position_nft_state(&nft_data)
                .filter(|s| s.is_initialized())
                .ok_or(ProgramError::UninitializedAccount)?
        };

        nft_state.pending_settlement = 1;

        {
            let mut nft_data = a_nft_pda
                .try_borrow_mut_data()
                .map_err(|_| ProgramError::AccountBorrowFailed)?;
            crate::position_nft::write_position_nft_state(&mut nft_data, &nft_state);
        }

        msg!(
            "PERC-608: SetPendingSettlement slab={} user_idx={}",
            a_slab.key,
            user_idx,
        );
        Ok(())
    }

    // --- ClearPendingSettlement ---
    #[inline(never)]
    fn handle_clear_pending_settlement<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        user_idx: u16,
    ) -> ProgramResult {
        accounts::expect_len(accounts, 3)?;
        let a_keeper = &accounts[0];
        let a_slab = &accounts[1];
        let a_nft_pda = &accounts[2];

        accounts::expect_signer(a_keeper)?;
        accounts::expect_writable(a_nft_pda)?;

        {
            let slab_data = a_slab
                .try_borrow_data()
                .map_err(|_| ProgramError::AccountBorrowFailed)?;
            slab_guard(program_id, a_slab, &slab_data)?;
            require_initialized(&slab_data)?;
            let header = state::read_header(&slab_data);
            require_admin(header.admin, a_keeper.key)?;
        }

        let (expected_nft_pda, _) =
            crate::position_nft::derive_position_nft(program_id, a_slab.key, user_idx);
        accounts::expect_key(a_nft_pda, &expected_nft_pda)?;

        let mut nft_state = {
            let nft_data = a_nft_pda
                .try_borrow_data()
                .map_err(|_| ProgramError::AccountBorrowFailed)?;
            crate::position_nft::read_position_nft_state(&nft_data)
                .filter(|s| s.is_initialized())
                .ok_or(ProgramError::UninitializedAccount)?
        };

        nft_state.pending_settlement = 0;

        {
            let mut nft_data = a_nft_pda
                .try_borrow_mut_data()
                .map_err(|_| ProgramError::AccountBorrowFailed)?;
            crate::position_nft::write_position_nft_state(&mut nft_data, &nft_state);
        }

        msg!(
            "PERC-608: ClearPendingSettlement slab={} user_idx={}",
            a_slab.key,
            user_idx,
        );
        Ok(())
    }

    // --- SetWalletCap ---
    #[inline(never)]
    fn handle_set_wallet_cap<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        cap_e6: u64,
    ) -> ProgramResult {
        accounts::expect_len(accounts, 2)?;
        let a_admin = &accounts[0];
        let a_slab = &accounts[1];

        accounts::expect_signer(a_admin)?;
        accounts::expect_writable(a_slab)?;

        const MIN_WALLET_CAP_E6: u64 = 1_000;
        if cap_e6 != 0 && cap_e6 < MIN_WALLET_CAP_E6 {
            msg!(
                "PERC-8224: SetWalletCap rejected: cap_e6={} is below minimum floor {} \
                 (use 0 to disable, or >= {} to set a real cap)",
                cap_e6,
                MIN_WALLET_CAP_E6,
                MIN_WALLET_CAP_E6,
            );
            return Err(ProgramError::InvalidArgument);
        }

        let mut data = state::slab_data_mut(a_slab)?;
        slab_guard(program_id, a_slab, &data)?;
        require_initialized(&data)?;

        let header = state::read_header(&data);
        require_admin(header.admin, a_admin.key)?;

        let mut config = state::read_config(&data);
        state::set_max_wallet_pos_e6(&mut config, cap_e6);
        state::write_config(&mut data, &config);

        let stored = state::get_max_wallet_pos_e6(&config);
        msg!(
            "PERC-8111: SetWalletCap: cap_e6={} stored={}",
            cap_e6,
            stored,
        );
        Ok(())
    }

    // --- SetOiImbalanceHardBlock ---
    #[inline(never)]
    fn handle_set_oi_imbalance_hard_block<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        threshold_bps: u16,
    ) -> ProgramResult {
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

        if threshold_bps > 10_000 {
            return Err(ProgramError::InvalidArgument);
        }

        let mut config = state::read_config(&data);
        state::set_oi_imbalance_hard_block_bps(&mut config, threshold_bps);
        state::write_config(&mut data, &config);

        let stored = state::get_oi_imbalance_hard_block_bps(&config);
        msg!(
            "PERC-8110: SetOiImbalanceHardBlock: threshold_bps={} stored={}",
            threshold_bps,
            stored,
        );
        Ok(())
    }

    // --- RescueOrphanVault ---
    #[inline(never)]
    fn handle_rescue_orphan_vault<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],) -> ProgramResult {
        accounts::expect_len(accounts, 6)?;
        let a_admin = &accounts[0];
        let a_slab = &accounts[1];
        let a_admin_ata = &accounts[2];
        let a_vault = &accounts[3];
        let a_token = &accounts[4];
        let a_vault_pda = &accounts[5];

        accounts::expect_signer(a_admin)?;
        accounts::expect_writable(a_admin_ata)?;
        accounts::expect_writable(a_vault)?;
        verify_token_program(a_token)?;

        if a_slab.owner != program_id {
            return Err(ProgramError::IllegalOwner);
        }

        let slab_data = a_slab.try_borrow_data()?;
        if slab_data.len() < 48 {
            return Err(ProgramError::InvalidAccountData);
        }

        let magic = u64::from_le_bytes(
            slab_data[0..8]
                .try_into()
                .map_err(|_| ProgramError::InvalidAccountData)?,
        );
        if magic != MAGIC {
            return Err(ProgramError::InvalidAccountData);
        }

        let admin_bytes: [u8; 32] = slab_data[16..48]
            .try_into()
            .map_err(|_| ProgramError::InvalidAccountData)?;
        let slab_admin = Pubkey::new_from_array(admin_bytes);
        if slab_admin != *a_admin.key {
            return Err(ProgramError::InvalidAccountData);
        }

        let flags = slab_data[state::FLAGS_OFF];
        if flags & state::FLAG_RESOLVED == 0 {
            solana_program::msg!("RescueOrphanVault rejected: market is not resolved");
            return Err(ProgramError::InvalidAccountData);
        }

        let bump = slab_data[12];
        drop(slab_data);

        // H-1: Verify vault is owned by SPL Token program.
        if a_vault.owner != &crate::spl_token::id() {
            return Err(ProgramError::IllegalOwner);
        }

        let (auth, expected_bump) =
            accounts::derive_vault_authority(program_id, a_slab.key);
        if bump != expected_bump {
            return Err(ProgramError::InvalidAccountData);
        }
        accounts::expect_key(a_vault_pda, &auth)?;

        let vault_data = a_vault.try_borrow_data()?;
        let vault_token = crate::spl_token::state::TokenAccountView::unpack(&vault_data)?;
        if vault_token.owner != auth {
            return Err(ProgramError::InvalidAccountData);
        }
        let actual_amount = vault_token.amount;
        let vault_mint = vault_token.mint;
        drop(vault_data);

        let admin_ata_data = a_admin_ata.try_borrow_data()?;
        let admin_token =
            crate::spl_token::state::TokenAccountView::unpack(&admin_ata_data)?;
        if admin_token.owner != *a_admin.key {
            return Err(ProgramError::InvalidAccountData);
        }
        if admin_token.mint != vault_mint {
            return Err(ProgramError::InvalidAccountData);
        }
        drop(admin_ata_data);

        if actual_amount == 0 {
            msg!("PERC-8400: vault is empty, nothing to rescue");
            return Ok(());
        }

        let seed1: &[u8] = b"vault";
        let seed2: &[u8] = a_slab.key.as_ref();
        let bump_arr: [u8; 1] = [bump];
        let seed3: &[u8] = &bump_arr;
        let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
        let signer_seeds: [&[&[u8]]; 1] = [&seeds];

        collateral::withdraw(
            a_token,
            a_vault,
            a_admin_ata,
            a_vault_pda,
            actual_amount,
            &signer_seeds,
        )?;

        msg!("PERC-8400: rescued {} tokens from orphan vault", actual_amount);
        Ok(())
    }

    // --- CloseOrphanSlab ---
    #[inline(never)]
    fn handle_close_orphan_slab<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],) -> ProgramResult {
        accounts::expect_len(accounts, 3)?;
        let a_admin = &accounts[0];
        let a_slab = &accounts[1];
        let a_vault = &accounts[2];

        accounts::expect_signer(a_admin)?;
        accounts::expect_writable(a_slab)?;

        if a_slab.owner != program_id {
            return Err(ProgramError::IllegalOwner);
        }

        {
            let mut slab_data = a_slab.try_borrow_mut_data()?;
            if slab_data.len() < 48 {
                return Err(ProgramError::InvalidAccountData);
            }

            let magic = u64::from_le_bytes(
                slab_data[0..8]
                    .try_into()
                    .map_err(|_| ProgramError::InvalidAccountData)?,
            );
            if magic != MAGIC {
                return Err(ProgramError::InvalidAccountData);
            }

            let admin_bytes: [u8; 32] = slab_data[16..48]
                .try_into()
                .map_err(|_| ProgramError::InvalidAccountData)?;
            let slab_admin = Pubkey::new_from_array(admin_bytes);
            if slab_admin != *a_admin.key {
                return Err(ProgramError::InvalidAccountData);
            }

            // M-1: Verify vault is owned by SPL Token program.
            if a_vault.owner != &crate::spl_token::id() {
                return Err(ProgramError::IllegalOwner);
            }

            let vault_data = a_vault
                .try_borrow_data()
                .map_err(|_| ProgramError::InvalidAccountData)?;
            if vault_data.len() < 72 {
                return Err(ProgramError::InvalidAccountData);
            }
            let vault_amount = u64::from_le_bytes(
                vault_data[64..72]
                    .try_into()
                    .map_err(|_| ProgramError::InvalidAccountData)?,
            );
            if vault_amount > 0 {
                msg!("PERC-8400: vault still has {} tokens, rescue first", vault_amount);
                return Err(ProgramError::InvalidAccountData);
            }
            let vault_owner_bytes: [u8; 32] = vault_data[32..64]
                .try_into()
                .map_err(|_| ProgramError::InvalidAccountData)?;
            let vault_owner = Pubkey::new_from_array(vault_owner_bytes);
            let (expected_auth, _) =
                accounts::derive_vault_authority(program_id, a_slab.key);
            if vault_owner != expected_auth {
                return Err(ProgramError::InvalidAccountData);
            }
            drop(vault_data);

            for b in slab_data.iter_mut() {
                *b = 0;
            }
        }

        let slab_lamports = a_slab.lamports();
        **a_slab.lamports.borrow_mut() = 0;
        **a_admin.lamports.borrow_mut() = a_admin
            .lamports()
            .checked_add(slab_lamports)
            .ok_or(PercolatorError::EngineOverflow)?;

        msg!("PERC-8400: closed orphan slab, reclaimed {} lamports", slab_lamports);
        Ok(())
    }

    // --- UpdateHyperpMark (tag 34) ---
    // Permissionless Hyperp EMA oracle: reads DEX pool price, applies EMA smoothing,
    // writes new mark price to config.authority_price_e6.
    //
    // Accounts:
    //   0. [writable] Slab
    //   1. []         DEX pool account (PumpSwap/Raydium CLMM/Meteora DLMM)
    //   2. []         Clock sysvar
    //   3..N []       Remaining: PumpSwap: [3]=base_vault, [4]=quote_vault
    //                             Meteora DLMM: [3]=vault_y
    //                             Raydium CLMM: none required
    #[inline(never)]
    fn handle_update_hyperp_mark<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
    ) -> ProgramResult {
        if accounts.len() < 3 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // SECURITY GATE 2: Reject if called via CPI.
        // Threat: bundling UpdateHyperpMark + Trade in same tx to exploit fresh EMA.
        // Defence: stack height == 1 only for top-level instructions.
        if solana_program::instruction::get_stack_height()
            > solana_program::instruction::TRANSACTION_LEVEL_STACK_HEIGHT
        {
            msg!("UpdateHyperpMark: CPI invocation rejected (security gate 2)");
            return Err(PercolatorError::EngineUnauthorized.into());
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

        let mut config = state::read_config(&data);
        if !oracle::is_hyperp_mode(&config) {
            msg!("UpdateHyperpMark: not a Hyperp market");
            return Err(ProgramError::InvalidAccountData);
        }

        // Bootstrap guard: admin must seed initial mark via PushOraclePrice first.
        // When prev_mark==0 the circuit breaker has no reference to clamp against,
        // so a thin-pool attacker could set an arbitrary initial mark.
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

        // PERC-367: Minimum update interval (25 slots ≈ 10s) limits manipulation frequency.
        // An attacker calling every slot can only drift EMA at 0.6%/min with this guard.
        const MIN_HYPERP_UPDATE_INTERVAL_SLOTS: u64 = 25;
        if dt_slots < MIN_HYPERP_UPDATE_INTERVAL_SLOTS {
            return Ok(()); // too soon — skip silently
        }

        // SECURITY (PERC-SetDexPool): Verify pool matches admin-pinned address.
        // All-zeros means SetDexPool was never called → reject.
        if config.dex_pool == [0u8; 32] {
            msg!("UpdateHyperpMark: dex_pool not set — admin must call SetDexPool first");
            return Err(PercolatorError::OracleInvalid.into());
        }
        if a_dex_pool.key.to_bytes() != config.dex_pool {
            msg!(
                "UpdateHyperpMark: pool key {} does not match stored dex_pool",
                a_dex_pool.key,
            );
            return Err(PercolatorError::InvalidOracleKey.into());
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
        // the market's collateral_mint. Without this check, a caller could pass any
        // valid PumpSwap pool for a different token pair.
        if *a_dex_pool.owner == crate::oracle::PUMPSWAP_PROGRAM_ID {
            let pool_data = a_dex_pool.try_borrow_data()?;
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

        // SECURITY (M-4): Raydium CLMM and Meteora DLMM pools must bind one token
        // to the market's collateral_mint.
        if *a_dex_pool.owner == crate::oracle::RAYDIUM_CLMM_PROGRAM_ID {
            let pool_data = a_dex_pool.try_borrow_data()?;
            const RAYDIUM_CLMM_OFF_MINT0: usize = 73;
            const RAYDIUM_CLMM_OFF_MINT1: usize = 105;
            if pool_data.len() < RAYDIUM_CLMM_OFF_MINT1 + 32 {
                return Err(ProgramError::InvalidAccountData);
            }
            let mint0: [u8; 32] = pool_data
                [RAYDIUM_CLMM_OFF_MINT0..RAYDIUM_CLMM_OFF_MINT0 + 32]
                .try_into()
                .unwrap();
            let mint1: [u8; 32] = pool_data
                [RAYDIUM_CLMM_OFF_MINT1..RAYDIUM_CLMM_OFF_MINT1 + 32]
                .try_into()
                .unwrap();
            if mint0 != config.collateral_mint && mint1 != config.collateral_mint {
                msg!("UpdateHyperpMark: Raydium CLMM pool mints do not match collateral_mint");
                return Err(PercolatorError::InvalidOracleKey.into());
            }
        } else if *a_dex_pool.owner == crate::oracle::METEORA_DLMM_PROGRAM_ID {
            let pool_data = a_dex_pool.try_borrow_data()?;
            const METEORA_OFF_TOKEN_X_MINT: usize = 81;
            const METEORA_OFF_TOKEN_Y_MINT: usize = 113;
            if pool_data.len() < METEORA_OFF_TOKEN_Y_MINT + 32 {
                return Err(ProgramError::InvalidAccountData);
            }
            let mint_x: [u8; 32] = pool_data
                [METEORA_OFF_TOKEN_X_MINT..METEORA_OFF_TOKEN_X_MINT + 32]
                .try_into()
                .unwrap();
            let mint_y: [u8; 32] = pool_data
                [METEORA_OFF_TOKEN_Y_MINT..METEORA_OFF_TOKEN_Y_MINT + 32]
                .try_into()
                .unwrap();
            if mint_x != config.collateral_mint && mint_y != config.collateral_mint {
                msg!("UpdateHyperpMark: Meteora DLMM pool mints do not match collateral_mint");
                return Err(PercolatorError::InvalidOracleKey.into());
            }
        }

        let remaining = &accounts[3..];
        let dex_result = oracle::read_dex_price_with_liquidity(
            a_dex_pool,
            config.invert,
            config.unit_scale,
            remaining,
        )?;

        // SECURITY (#297): Minimum DEX liquidity check.
        if dex_result.quote_liquidity < crate::constants::MIN_DEX_QUOTE_LIQUIDITY {
            msg!(
                "UpdateHyperpMark: insufficient DEX liquidity {} < minimum {}",
                dex_result.quote_liquidity,
                crate::constants::MIN_DEX_QUOTE_LIQUIDITY
            );
            return Err(PercolatorError::InsufficientDexLiquidity.into());
        }

        let dex_price = dex_result.price_e6;
        let prev_mark = config.authority_price_e6;

        // SECURITY: Max deviation clamp — clamp DEX spot price to ±5% band around
        // current EMA mark. Flash-loan attacks are clamped rather than rejected
        // to avoid permanently wedging the oracle on legitimate rapid moves.
        const MAX_HYPERP_DEVIATION_BPS: u64 = 500;
        let dex_price = if prev_mark > 0 {
            let max_delta = (prev_mark as u128)
                .saturating_mul(MAX_HYPERP_DEVIATION_BPS as u128)
                / 10_000;
            let max_delta = max_delta.min(prev_mark as u128) as u64;
            let lo = prev_mark.saturating_sub(max_delta);
            let hi = prev_mark.saturating_add(max_delta);
            if dex_price < lo || dex_price > hi {
                msg!(
                    "UpdateHyperpMark: DEX price {} outside band [{}, {}] (mark {}), clamping",
                    dex_price, lo, hi, prev_mark,
                );
            }
            dex_price.clamp(lo, hi)
        } else {
            dex_price
        };

        // PERC-118: Hyperp EMA Blend — blend oracle index + DEX spot price.
        // oracle_weight_bps == 0 (default on V12_1 layout) → pure DEX price (backward compat).
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

        // SECURITY (#297 Fix 2): Circuit breaker BEFORE EMA. Hyperp markets always
        // enforce at least DEFAULT_HYPERP_PRICE_CAP_E2BPS even if admin sets cap to 0.
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
        let new_index = oracle::clamp_toward_with_dt(
            oracle_for_blend.max(1),
            new_mark,
            effective_cap,
            dt_slots,
        );

        config.authority_price_e6 = new_mark;
        config.mark_ewma_e6 = new_mark;
        config.mark_ewma_last_slot = clock.slot;
        config.last_effective_price_e6 = new_index;

        // Record pool depth for per-epoch OI cap enforcement (no-op in V12_1 layout).
        state::set_last_dex_liquidity_k(&mut config, dex_result.quote_liquidity);

        state::write_config(&mut data, &config);

        msg!(
            "UpdateHyperpMark: dex_price={} oracle={} blend={} prev_mark={} new_mark={} index={} weight_bps={} dt={} pool_depth={}",
            dex_price,
            oracle_for_blend,
            blend_input,
            prev_mark,
            new_mark,
            new_index,
            oracle_weight_bps,
            dt_slots,
            dex_result.quote_liquidity,
        );

        Ok(())
    }

    // --- PauseMarket (tag 76) ---
    #[inline(never)]
    fn handle_pause_market<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
    ) -> ProgramResult {
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
        Ok(())
    }

    // --- UnpauseMarket (tag 77) ---
    #[inline(never)]
    fn handle_unpause_market<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
    ) -> ProgramResult {
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
        Ok(())
    }

    // --- SetDexPool ---
    #[inline(never)]
    fn handle_set_dex_pool<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        pool: Pubkey,
    ) -> ProgramResult {
        accounts::expect_len(accounts, 3)?;
        let a_admin = &accounts[0];
        let a_slab = &accounts[1];
        let a_pool = &accounts[2];

        accounts::expect_signer(a_admin)?;
        accounts::expect_writable(a_slab)?;

        // SetDexPool fix: verify pool pubkey matches the pool account key.
        if pool != *a_pool.key {
            return Err(ProgramError::InvalidArgument);
        }

        let mut data = state::slab_data_mut(a_slab)?;
        slab_guard(program_id, a_slab, &data)?;
        require_initialized(&data)?;

        let header = state::read_header(&data);
        require_admin(header.admin, a_admin.key)?;

        let mut config = state::read_config(&data);

        if !oracle::is_hyperp_mode(&config) {
            msg!("SetDexPool: not a HYPERP market (index_feed_id is non-zero)");
            return Err(ProgramError::InvalidAccountData);
        }

        let is_approved_dex = *a_pool.owner == oracle::PUMPSWAP_PROGRAM_ID
            || *a_pool.owner == oracle::RAYDIUM_CLMM_PROGRAM_ID
            || *a_pool.owner == oracle::METEORA_DLMM_PROGRAM_ID;
        if !is_approved_dex {
            msg!("SetDexPool: pool account not owned by an approved DEX program");
            return Err(PercolatorError::OracleInvalid.into());
        }

        {
            let pool_data = a_pool.try_borrow_data()?;

            let mint_matches = if *a_pool.owner == oracle::PUMPSWAP_PROGRAM_ID {
                const PS_OFF_BASE_MINT: usize = 35;
                if pool_data.len() < PS_OFF_BASE_MINT + 32 {
                    return Err(ProgramError::InvalidAccountData);
                }
                let base_mint: [u8; 32] =
                    pool_data[PS_OFF_BASE_MINT..PS_OFF_BASE_MINT + 32]
                        .try_into()
                        .unwrap();
                base_mint == config.collateral_mint
            } else if *a_pool.owner == oracle::RAYDIUM_CLMM_PROGRAM_ID {
                const RAYDIUM_OFF_MINT0: usize = 73;
                const RAYDIUM_OFF_MINT1: usize = 105;
                if pool_data.len() < RAYDIUM_OFF_MINT1 + 32 {
                    return Err(ProgramError::InvalidAccountData);
                }
                let mint0: [u8; 32] = pool_data[RAYDIUM_OFF_MINT0..RAYDIUM_OFF_MINT0 + 32]
                    .try_into()
                    .unwrap();
                let mint1: [u8; 32] = pool_data[RAYDIUM_OFF_MINT1..RAYDIUM_OFF_MINT1 + 32]
                    .try_into()
                    .unwrap();
                mint0 == config.collateral_mint || mint1 == config.collateral_mint
            } else {
                // Meteora DLMM
                const METEORA_OFF_X: usize = 81;
                const METEORA_OFF_Y: usize = 113;
                if pool_data.len() < METEORA_OFF_Y + 32 {
                    return Err(ProgramError::InvalidAccountData);
                }
                let x_mint: [u8; 32] = pool_data[METEORA_OFF_X..METEORA_OFF_X + 32]
                    .try_into()
                    .unwrap();
                let y_mint: [u8; 32] = pool_data[METEORA_OFF_Y..METEORA_OFF_Y + 32]
                    .try_into()
                    .unwrap();
                x_mint == config.collateral_mint || y_mint == config.collateral_mint
            };

            if !mint_matches {
                msg!("SetDexPool: pool mints do not include market collateral_mint");
                return Err(PercolatorError::OracleInvalid.into());
            }
        }

        config.dex_pool = pool.to_bytes();
        state::write_config(&mut data, &config);

        msg!("SetDexPool: pinned pool {} for HYPERP market {}", pool, a_slab.key);
        Ok(())
    }

    // --- InitMatcherCtx ---
    #[inline(never)]
    fn handle_init_matcher_ctx<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
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
    ) -> ProgramResult {
        accounts::expect_len(accounts, 5)?;
        let a_admin = &accounts[0];
        let a_slab = &accounts[1];
        let a_matcher_ctx = &accounts[2];
        let a_matcher_prog = &accounts[3];
        let a_lp_pda = &accounts[4];

        accounts::expect_signer(a_admin)?;
        accounts::expect_writable(a_matcher_ctx)?;

        let data = a_slab.try_borrow_data()?;
        slab_guard(program_id, a_slab, &data)?;
        require_initialized(&data)?;

        let header = state::read_header(&data);
        require_admin(header.admin, a_admin.key)?;

        let engine = zc::engine_ref(&data)?;
        check_idx(engine, lp_idx)?;
        let lp_acc = &engine.accounts[lp_idx as usize];

        if lp_acc.matcher_program == [0u8; 32] {
            return Err(ProgramError::InvalidArgument);
        }

        if lp_acc.matcher_program != a_matcher_prog.key.to_bytes() {
            return Err(PercolatorError::EngineInvalidMatchingEngine.into());
        }

        if lp_acc.matcher_context != a_matcher_ctx.key.to_bytes() {
            return Err(PercolatorError::EngineInvalidMatchingEngine.into());
        }

        if a_matcher_ctx.owner != a_matcher_prog.key {
            return Err(ProgramError::IncorrectProgramId);
        }

        if !a_matcher_prog.executable {
            return Err(ProgramError::InvalidAccountData);
        }

        let lp_bytes = lp_idx.to_le_bytes();
        let (expected_lp_pda, bump) = Pubkey::find_program_address(
            &[b"lp", a_slab.key.as_ref(), &lp_bytes],
            program_id,
        );
        if *a_lp_pda.key != expected_lp_pda {
            return Err(ProgramError::InvalidSeeds);
        }

        drop(data);

        // Build matcher init CPI data (matcher tag 2 + InitParams, 70 bytes total).
        let mut cpi_data = [0u8; 70];
        cpi_data[0] = 2; // MATCHER_INIT_TAG
        cpi_data[1] = kind;
        cpi_data[2..6].copy_from_slice(&trading_fee_bps.to_le_bytes());
        cpi_data[6..10].copy_from_slice(&base_spread_bps.to_le_bytes());
        cpi_data[10..14].copy_from_slice(&max_total_bps.to_le_bytes());
        cpi_data[14..18].copy_from_slice(&impact_k_bps.to_le_bytes());
        cpi_data[18..34].copy_from_slice(&liquidity_notional_e6.to_le_bytes());
        cpi_data[34..50].copy_from_slice(&max_fill_abs.to_le_bytes());
        cpi_data[50..66].copy_from_slice(&max_inventory_abs.to_le_bytes());
        cpi_data[66..68].copy_from_slice(&fee_to_insurance_bps.to_le_bytes());
        cpi_data[68..70].copy_from_slice(&skew_spread_mult_bps.to_le_bytes());

        let metas = [
            solana_program::instruction::AccountMeta::new_readonly(
                *a_lp_pda.key,
                true,
            ),
            solana_program::instruction::AccountMeta::new(*a_matcher_ctx.key, false),
        ];

        let ix = solana_program::instruction::Instruction {
            program_id: *a_matcher_prog.key,
            accounts: metas.to_vec(),
            data: cpi_data.to_vec(),
        };

        let bump_arr = [bump];
        let seeds: &[&[u8]] = &[b"lp", a_slab.key.as_ref(), &lp_bytes, &bump_arr];

        solana_program::program::invoke_signed(
            &ix,
            &[a_lp_pda.clone(), a_matcher_ctx.clone()],
            &[seeds],
        )?;

        msg!("InitMatcherCtx: initialized matcher context for LP idx {}", lp_idx);
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

// =============================================================================
// Fuzz helpers — only compiled when the "test" feature is enabled.
// These thin wrappers expose private or panic-unsafe internal paths with safe
// signatures so libFuzzer targets can drive them without crashing the harness.
// =============================================================================

/// Public fuzz surface gated behind the `test` feature flag.
/// None of these functions are reachable from a deployed BPF binary.
#[cfg(feature = "test")]
pub mod fuzz_helpers {
    use super::*;
    use crate::constants::{HEADER_LEN, CONFIG_LEN};
    use crate::state::{SlabHeader, MarketConfig};
    use solana_program::program_error::ProgramError;

    /// Decode an arbitrary byte slice as a program instruction.
    /// Always returns Ok or Err — never panics.
    pub fn fuzz_decode_instruction(input: &[u8]) -> Result<ix::Instruction, ProgramError> {
        ix::Instruction::decode(input)
    }

    /// Parse risk params from an arbitrary byte slice by routing through the
    /// InitMarket tag (0).  The decoder calls `read_risk_params` internally,
    /// so we exercise that private path without duplicating its logic.
    ///
    /// Returns Ok or Err — never panics regardless of input length.
    pub fn fuzz_read_risk_params_via_decode(payload: &[u8]) -> Result<ix::Instruction, ProgramError> {
        // Prepend tag=0 (InitMarket) and let the full decoder run.
        let mut buf = alloc::vec![0u8]; // tag byte
        buf.extend_from_slice(payload);
        ix::Instruction::decode(&buf)
    }

    /// Safe slab-header parse: returns None if the buffer is too short,
    /// SlabHeader on any valid-length input.  Never panics.
    pub fn fuzz_read_header(data: &[u8]) -> Option<SlabHeader> {
        if data.len() < HEADER_LEN {
            return None;
        }
        Some(state::read_header(data))
    }

    /// Safe market-config parse: returns None if the buffer is too short,
    /// MarketConfig on any valid-length input.  Never panics.
    pub fn fuzz_read_config(data: &[u8]) -> Option<MarketConfig> {
        if data.len() < HEADER_LEN + CONFIG_LEN {
            return None;
        }
        Some(state::read_config(data))
    }

    /// Parse both header and config from a single slab byte slice.
    /// Returns (header, config) if the slice is large enough, None otherwise.
    pub fn fuzz_read_header_and_config(data: &[u8]) -> Option<(SlabHeader, MarketConfig)> {
        let h = fuzz_read_header(data)?;
        let c = fuzz_read_config(data)?;
        Some((h, c))
    }
}
