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
    /// Per-account materialization generation table.
    /// Stores the global mat_counter value assigned at InitUser/InitLP.
    /// Used as lp_account_id for per-instance identity across slot reuse.
    pub const GEN_TABLE_OFF: usize = RISK_BUF_OFF + RISK_BUF_LEN;
    pub const GEN_TABLE_LEN: usize = percolator::MAX_ACCOUNTS * 8; // u64 per slot
    pub const SLAB_LEN: usize = GEN_TABLE_OFF + GEN_TABLE_LEN;

    // CRANK_REWARD_MIN_DT removed — crank discount logic removed in v12.15
    /// Progressive scan window per crank.
    pub const RISK_SCAN_WINDOW: usize = 32;
    /// Crank reward: fraction of the maintenance-fee sweep that is paid to
    /// a non-permissionless caller. 5_000 bps = 50 %. The remaining 50 %
    /// stays in the insurance fund. Only bounded by insurance balance post-
    /// crank; the sweep itself is the natural cap (≤ FEE_SWEEP_BUDGET
    /// accounts × per-account dt × fee_rate per call).
    pub const CRANK_REWARD_BPS: u128 = 5_000;

    /// Max accounts whose fees get realized in a single KeeperCrank call.
    /// Keeps CU bounded regardless of `max_accounts` / total live-account
    /// count: at ~5K CU per `sync_account_fee_to_slot_not_atomic` call,
    /// 128 syncs ≈ 640K CU — room for liquidation/lifecycle in the same
    /// transaction. Per-account `Account::last_fee_slot` keeps the sweep
    /// correct across multiple cranks — when the cursor reaches an account,
    /// it pays for its full elapsed interval in one charge.
    pub const FEE_SWEEP_BUDGET: usize = 128;

    // Compile-time invariant: the crank's total fee-sync budget
    // (FEE_SWEEP_BUDGET) must accommodate the engine's per-crank
    // liquidation/candidate-sync allowance. The candidate-sync path
    // caps itself at min(LIQ_BUDGET_PER_CRANK, FEE_SWEEP_BUDGET); if
    // the engine constant were raised above the wrapper constant the
    // belt-and-braces min() would silently under-apply the engine
    // budget. Assert the relation so a mismatch is a build error.
    const _: () = assert!(
        (percolator::LIQ_BUDGET_PER_CRANK as usize) <= FEE_SWEEP_BUDGET,
        "LIQ_BUDGET_PER_CRANK must not exceed FEE_SWEEP_BUDGET"
    );

    // ── Engine envelope constants (wrapper-owned, immutable per deployment) ──
    //
    // These values populate the engine's per-market RiskParams envelope at
    // InitMarket. They are NOT decoded from instruction data and NOT admin-
    // configurable — every deployment uses these exact values. The envelope
    // invariant
    //   ADL_ONE * MAX_ORACLE_PRICE * MAX_ABS_FUNDING_E9_PER_SLOT *
    //     MAX_ACCRUAL_DT_SLOTS <= i128::MAX
    // must hold: 1e15 * 1e12 * 1e4 * 1e7 = 1e38 < i128::MAX (≈1.7e38). ✓
    //
    // Tightened from the prior stress-test values (1e6 rate / 1e5 dt) to the
    // production-aligned trade-off (low rate cap, long accrual window) in
    // concert with engine-crate commit 95665cb which dropped the GLOBAL
    // `MAX_ABS_FUNDING_E9_PER_SLOT` to 10_000.
    //
    // Surface them here as named constants so operators and auditors can see
    // exactly what values ship, rather than having them buried inside the
    // RiskParams literal in read_risk_params.
    /// Max dt allowed in a single `accrue_market_to` call (spec §1.4).
    /// 10_000_000 slots at 400 ms/slot ≈ 46 days — the permissionless-
    /// resolve / outage-tolerance window. Must be ≤ min_funding_lifetime_slots.
    pub const MAX_ACCRUAL_DT_SLOTS: u64 = 10_000_000;
    /// Max |funding_rate_e9_per_slot| the engine will accrue (spec §1.4).
    /// Matches the engine-crate GLOBAL ceiling. Realistic perp funding is
    /// 3-5 orders of magnitude below this (see compute_current_funding_rate_e9
    /// clamp math), so this cap exists to bound the integer-overflow envelope,
    /// not to shape market behavior.
    pub const MAX_ABS_FUNDING_E9_PER_SLOT: u64 = 10_000;
    /// Cumulative-funding lifetime (engine §1.4 v12.18.x). Distinct from
    /// the per-call `MAX_ACCRUAL_DT_SLOTS` envelope: this bounds the
    /// lifetime sum of funding contributions, not any single call.
    ///
    /// Engine init asserts the safety envelope:
    ///
    /// ```text
    /// ADL_ONE · MAX_ORACLE_PRICE · max_abs_funding_e9_per_slot ·
    ///   min_funding_lifetime_slots  ≤  i128::MAX
    /// ```
    ///
    /// With the engine-crate constants
    ///     ADL_ONE            = 10^15
    ///     MAX_ORACLE_PRICE   = 10^12
    /// and this crate's (tightened) ceiling
    ///     MAX_ABS_FUNDING_E9_PER_SLOT = 10^4
    /// the lifetime ceiling is
    ///     i128::MAX / (10^15 · 10^12 · 10^4)  ≈ 1.7 × 10^7 slots
    ///
    /// ═════════════════════════════════════════════════════════════════
    /// OPERATIONAL ASSUMPTION — accepted finite market lifetime
    /// ═════════════════════════════════════════════════════════════════
    /// The engine does not expose an F-index rebase, so every deployed
    /// market has a finite cumulative-funding lifetime bounded by the
    /// envelope above. At 400 ms/slot (~7.89 × 10^7 slots/year), the
    /// worst-case lifetimes at sustained max-rate funding are:
    ///
    /// ```text
    /// rate <= 10_000 (global max)  ⇒ ~1.7e7 slots  ≈ 2.6 months
    /// rate <=  1_000               ⇒ ~1.7e8 slots  ≈ 2.15 years
    /// rate <=    100               ⇒ ~1.7e9 slots  ≈ 21.5 years
    /// rate <=     10               ⇒ ~1.7e10 slots ≈ 215  years
    /// ```
    ///
    /// The EFFECTIVE horizon at realistic rates is vastly longer.
    /// Real perp funding averages 1 bps/day ≈ 4.6 × 10⁻¹⁰ per slot at
    /// 2.5 slots/sec. At 10^1 e9 units/slot (2.2 × 10^6 × realistic)
    /// the effective lifetime is measured in centuries.
    ///
    /// Tuning options a deployer has for extending the floor:
    ///   (a) Lower `MAX_ABS_FUNDING_E9_PER_SLOT` — the envelope scales
    ///       linearly, so halving the funding cap doubles the lifetime.
    ///   (b) Reduce the per-market `funding_max_e9_per_slot` (but note
    ///       the integer-bps granularity trap: 1 bps = 100_000 e9, which
    ///       is 10× the current engine ceiling — only `0` fits the
    ///       envelope, which disables the cap).
    ///   (c) Engine-side F-index rebase (out of wrapper scope).
    ///
    /// Admin-free deployments that intend to run indefinitely should
    /// treat the theoretical floor as a LIVENESS BUDGET: once the
    /// cumulative funding envelope is exhausted, future accrue_market
    /// _to calls saturate and the market effectively freezes. At that
    /// point `permissionless_resolve_stale_slots` is the fallback exit
    /// path for users. Operators MUST set that field > 0 on admin-
    /// free markets (a zero value combined with envelope exhaustion
    /// would trap capital).
    pub const MIN_FUNDING_LIFETIME_SLOTS: u64 = 10_000_000;
    pub const MATCHER_ABI_VERSION: u32 = 2;
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
    /// Default per-market cap on wrapper-computed funding rate, in engine-native
    /// e9 (parts-per-billion) per slot. 1_000 e9/slot ≈ 2.16e-4/slot ≈ 21.6 %/day
    /// at 2.5 slots/sec — loose enough to be non-binding on realistic markets
    /// (1 bps/day ≈ 4.6e-10/slot) and comfortably under the engine global
    /// ceiling MAX_ABS_FUNDING_E9_PER_SLOT = 10_000. Clients compute this from
    /// operator-friendly units (e.g. bps/day) at market-setup time.
    pub const DEFAULT_FUNDING_MAX_E9_PER_SLOT: i64 = 1_000;
    pub const DEFAULT_HYPERP_PRICE_CAP_E2BPS: u64 = 10_000; // 1% per slot max price change for Hyperp
    pub const MAX_ORACLE_PRICE_CAP_E2BPS: u64 = 1_000_000; // 100% — hard ceiling for circuit breaker
    pub const DEFAULT_INSURANCE_WITHDRAW_MIN_BASE: u64 = 1;
    pub const DEFAULT_INSURANCE_WITHDRAW_MAX_BPS: u16 = 100; // 1%
    pub const DEFAULT_INSURANCE_WITHDRAW_COOLDOWN_SLOTS: u64 = 400_000;
    pub const DEFAULT_MARK_EWMA_HALFLIFE_SLOTS: u64 = 100; // ~40 sec @ 2.5 slots/sec
    /// Upper bound on `force_close_delay_slots` (Finding 6). Without a bound, an
    /// init-time config of `u64::MAX` passes the "nonzero" liveness guard but
    /// makes ForceCloseResolved unreachable — `resolved_slot + delay` saturates
    /// to `u64::MAX`, stranding any accounts left on a resolved market whose
    /// admin was burned. 10_000_000 slots is ~50 days at 2 slots/s, far beyond
    /// any reasonable grace period but well short of the saturation regime.
    pub const MAX_FORCE_CLOSE_DELAY_SLOTS: u64 = 10_000_000;

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
    /// Strict equality check for instruction account-count ABIs.
    /// Each handler has a fixed account count; accepting extra trailing
    /// accounts is a footgun (caller pads with unrelated accounts →
    /// still accepted). TradeCpi is the one documented exception and
    /// uses `len_at_least`.
    pub fn len_ok(actual: usize, need: usize) -> bool {
        actual == need
    }

    /// Loose "at least N" check for instructions with a variadic tail
    /// (TradeCpi forwards the tail to the matcher CPI).
    pub fn len_at_least(actual: usize, need: usize) -> bool {
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

    // Runtime tripwire: a unit test in tests/unit.rs
    // (`test_zc_cast_safety_invariant`) asserts that no slab-persisted
    // field has an invalid bit pattern beyond the enums validated
    // above. A compile-time size assert was considered but rejected:
    // sizeof<RiskEngine> differs between x86_64 and sbf targets (u128
    // alignment), so a const-eval tripwire cannot cover both builds.
    // The unit test runs on x86_64 but is a structural check — it
    // inspects type identities, not sizes — so it is target-
    // independent and still catches the "someone silently added a
    // bool field" class.

    /// Validate ALL fields with invalid bit patterns from raw bytes
    /// BEFORE casting the slab to &RiskEngine / &mut RiskEngine.
    /// Required because the cast is `unsafe`: a Rust reference to a
    /// struct containing an invalid bit pattern is UB on first field
    /// access, irrespective of whether we read the field.
    ///
    /// The only field types in the RiskEngine slab with invalid bit
    /// patterns today are the two `#[repr(u8)]` enums:
    ///   - SideMode (2 instances at side_mode_long / side_mode_short):
    ///     valid tag bytes 0 (Normal), 1 (DrainOnly), 2 (ResetPending).
    ///   - MarketMode (at market_mode): valid tag bytes 0 (Live),
    ///     1 (Resolved).
    /// No other field type in either RiskEngine or Account has invalid
    /// bit patterns: every other field is u64/u128/i64/i128/[u8; N]/
    /// wrapper-Pod (U128/I128) or fixed u8 — all-bits-valid types.
    /// The two bool fields in the engine crate (InstructionContext,
    /// CrankOutcome) are transient runtime structs, not slab-persisted,
    /// so they are never materialized through this cast.
    ///
    /// If a future revision adds any new enum or bool field to the
    /// slab, the validation below must be extended before the cast
    /// can be considered sound. A compile-time invariant check
    /// (`assert!(size_of::<RiskEngine>() == EXPECTED)`) elsewhere in
    /// this module forces deliberate attention on layout changes.
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

    /// Invoke the matcher program via CPI. The AccountInfo clones
    /// satisfy solana_program::program::invoke_signed's ownership
    /// requirement without relying on lifetime transmutes.
    ///
    /// `tail` is the caller-supplied variadic account list that
    /// TradeCpi forwards verbatim to the matcher. The wrapper does
    /// NOT validate tail contents — the matcher owns that
    /// responsibility. Tail length is unbounded at the wire level;
    /// Solana's CPI transaction-size and account-count limits are
    /// the effective cap.
    #[inline]
    pub fn invoke_signed_trade<'a>(
        ix: &SolInstruction,
        a_lp_pda: &AccountInfo<'a>,
        a_matcher_ctx: &AccountInfo<'a>,
        a_matcher_prog: &AccountInfo<'a>,
        tail: &[AccountInfo<'a>],
        seeds: &[&[u8]],
    ) -> Result<(), ProgramError> {
        // Infos: lp_pda + matcher_ctx + matcher_prog + tail. The
        // matcher_prog is always included because invoke_signed needs
        // it to resolve the destination program; the CPI metas do not
        // list it (Solana convention).
        let mut infos: alloc::vec::Vec<AccountInfo<'a>> = alloc::vec::Vec::with_capacity(3 + tail.len());
        infos.push(a_lp_pda.clone());
        infos.push(a_matcher_ctx.clone());
        infos.push(a_matcher_prog.clone());
        for ai in tail.iter() {
            infos.push(ai.clone());
        }
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
        // Reject any flag bits outside the known set. Prevents a future
        // matcher that uses a currently-undefined flag (e.g. a new partial
        // fill semantics) from being silently accepted by this wrapper —
        // upgraders must bump the ABI version to signal new flag meaning.
        const KNOWN_FLAGS: u32 = FLAG_VALID | FLAG_PARTIAL_OK | FLAG_REJECTED;
        if (ret.flags & !KNOWN_FLAGS) != 0 {
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
        /// Wrapper-level: the gap between `engine.current_slot` and
        /// `clock.slot` exceeds `CATCHUP_CHUNKS_MAX × max_accrual_dt_slots`.
        /// Caller must run the dedicated `CatchupAccrue` instruction one
        /// or more times to commit incremental progress, then retry.
        CatchupRequired,
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
            funding_max_e9_per_slot: Option<i64>,
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
        /// Close the market slab and recover SOL to close_authority.
        /// Requires: no active accounts, no vault funds, no insurance funds.
        CloseSlab,
        /// Update configurable funding parameters. Admin only.
        UpdateConfig {
            funding_horizon_slots: u64,
            funding_k_bps: u64,
            funding_max_premium_bps: i64,
            funding_max_e9_per_slot: i64,
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
        /// Withdraw insurance fund balance. Gated by the scoped
        /// `header.insurance_authority`. Insurance withdrawal is
        /// purely BINARY: insurance_authority either has full
        /// withdrawal power or it's burned to zero (no paper-policy
        /// in between). The former SetInsuranceWithdrawPolicy /
        /// WithdrawInsuranceLimited instructions were removed —
        /// they were non-binding (same signer could always bypass by
        /// calling the unbounded path) and added complexity without
        /// a real security property.
        WithdrawInsurance,
        /// Admin force-close an abandoned account after market resolution.
        /// Requires RESOLVED flag, zero position, admin signer.
        AdminForceCloseAccount {
            user_idx: u16,
        },
        // Tag 24 QueryLpFees removed. The instruction exposed
        // `Account.fee_credits` as an "earned fees" query, but
        // `fee_credits` is a debt-tracker (engine invariant: stays in
        // [-i128::MAX, 0]; positive values are unreachable) — all
        // trading fees go straight to insurance, LPs don't accumulate
        // earnings through this field. The query returned 0 for every
        // real input; the ABI was misleading. Deleted outright rather
        // than reshaped because there is no "cumulative earned fees"
        // counter elsewhere in the engine — LPs earn via their
        // matcher's spread, not via wrapper-visible accounting.
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
        /// Permissionless market-clock catchup (tag 31). When a live market
        /// has been idle for longer than
        /// `CATCHUP_CHUNKS_MAX × max_accrual_dt_slots`, every accrue-bearing
        /// instruction would fail with `CatchupRequired` — no progress can
        /// be committed in a single failing transaction. This instruction
        /// does pure catchup (up to CATCHUP_CHUNKS_MAX chunks) and commits
        /// unconditionally, letting callers incrementally close the gap.
        ///
        /// Anyone can call. Takes slab + clock + oracle (3 accounts).
        /// The oracle is REQUIRED — a successful read proves the market
        /// is live before any accrual is applied, routing dead-oracle
        /// markets exclusively through ResolvePermissionless (whose
        /// Degenerate arm settles at rate = 0). Calls that CAN reach
        /// clock.slot persist the fresh observation; calls that can
        /// only partially advance use the read as a liveness proof
        /// but discard the observed price/index to avoid applying
        /// post-observation state to earlier engine slots.
        CatchupAccrue,
        /// Scoped-authority update (tag 32).
        ///
        /// kind:
        ///   0 = AUTHORITY_ADMIN          (header.admin)
        ///   1 = AUTHORITY_ORACLE         (config.oracle_authority)
        ///   2 = AUTHORITY_INSURANCE      (config.insurance_authority)
        ///   3 = AUTHORITY_CLOSE          (config.close_authority)
        ///
        /// Authorization: the CURRENT authority of the specified kind
        /// must sign. When `new_pubkey != Pubkey::default()` the NEW
        /// pubkey must ALSO sign, proving the receiver consents to
        /// take the role and the key isn't a typo. When `new_pubkey
        /// == Pubkey::default()` (burn), only the current authority
        /// signs.
        ///
        /// Each kind is independent — set them all to the same pubkey
        /// for a "super admin" or delegate/burn them individually for
        /// capability isolation. Burning `admin` applies the existing
        /// admin-burn liveness guards (permissionless resolve + force
        /// close configured). Burning any other kind has no guards —
        /// it just makes that capability permanently unavailable,
        /// which is a legitimate rug-proofing configuration.
        UpdateAuthority { kind: u8, new_pubkey: Pubkey },
    }

    impl Instruction {
        pub fn decode(input: &[u8]) -> Result<Self, ProgramError> {
            let (&tag, mut rest) = input
                .split_first()
                .ok_or(ProgramError::InvalidInstructionData)?;

            let result = match tag {
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
                        funding_max_e9_per_slot,
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
                        funding_max_e9_per_slot,
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
                    // Cap candidate count to prevent CU exhaustion via
                    // padding: the engine's keeper_crank_not_atomic scans
                    // every candidate in the slice, but only counts
                    // VALID existing entries against its per-crank
                    // budget. A keeper could otherwise submit thousands
                    // of invalid indices to burn CU before any useful
                    // work. We accept up to 2 × LIQ_BUDGET_PER_CRANK
                    // candidates — enough room for over-specification
                    // of deduplication / expired entries while keeping
                    // the total scan bounded.
                    const MAX_CANDIDATES: usize =
                        (percolator::LIQ_BUDGET_PER_CRANK as usize) * 2;
                    if format_version == 1 {
                        // Extended: u16 idx + u8 policy tag per candidate
                        while rest.len() >= 3 {
                            if candidates.len() >= MAX_CANDIDATES {
                                return Err(ProgramError::InvalidInstructionData);
                            }
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
                // Tag 12 (UpdateAdmin) deleted — use UpdateAuthority
                // { kind: AUTHORITY_ADMIN } (tag 32).
                13 => {
                    // CloseSlab
                    Ok(Instruction::CloseSlab)
                }
                14 => {
                    // UpdateConfig — funding params only
                    let funding_horizon_slots = read_u64(&mut rest)?;
                    let funding_k_bps = read_u64(&mut rest)?;
                    let funding_max_premium_bps = read_i64(&mut rest)?;
                    let funding_max_e9_per_slot = read_i64(&mut rest)?;
                    Ok(Instruction::UpdateConfig {
                        funding_horizon_slots,
                        funding_k_bps,
                        funding_max_premium_bps,
                        funding_max_e9_per_slot,
                    })
                }
                // Tag 16 (SetOracleAuthority) deleted — use
                // UpdateAuthority { kind: AUTHORITY_ORACLE } (tag 32).
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
                // Tags 22 (SetInsuranceWithdrawPolicy) and 23
                // (WithdrawInsuranceLimited) deleted — the bounded
                // policy was non-binding (same insurance_authority
                // could bypass via tag 20 WithdrawInsurance) and
                // added complexity without a real security property.
                // Tag 24 (QueryLpFees) removed — fell out of the ABI
                // because fee_credits is a debt counter, not an LP
                // earnings counter. See the enum comment.
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
                31 => Ok(Instruction::CatchupAccrue),
                32 => {
                    // UpdateAuthority { kind: u8, new_pubkey: [u8; 32] }
                    let kind = read_u8(&mut rest)?;
                    let new_pubkey = read_pubkey(&mut rest)?;
                    Ok(Instruction::UpdateAuthority { kind, new_pubkey })
                }
                _ => Err(ProgramError::InvalidInstructionData),
            };
            // Trailing-byte guard: every tag above fully consumes its expected
            // payload. Anything left over is either a malformed client payload
            // or a future-version wire format the current program cannot safely
            // interpret. Reject rather than silently ignore — accepting stray
            // bytes is an ABI footgun that turns into a semantic drift bug as
            // soon as any instruction grows an optional tail field.
            // (Tag 0 / InitMarket has its own extended-tail check before
            //  returning; this final check is a belt-and-braces second line.)
            if result.is_ok() && !rest.is_empty() {
                return Err(ProgramError::InvalidInstructionData);
            }
            result
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
        // Wire-format compat: engine v12.18.1 removed new_account_fee (spec §10.2
        // made deposit the canonical materialization path with no engine-native
        // opening fee). We still consume 16 bytes to keep the wire format stable
        // for existing clients, but the value is discarded.
        let _compat_new_account_fee = read_u128(input)?;
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
            // Envelope is fixed by wrapper deployment, not market-specified.
            // See `crate::constants::{MAX_ACCRUAL_DT_SLOTS,
            // MAX_ABS_FUNDING_E9_PER_SLOT}` for the invariant proof.
            max_accrual_dt_slots: crate::constants::MAX_ACCRUAL_DT_SLOTS,
            max_abs_funding_e9_per_slot: crate::constants::MAX_ABS_FUNDING_E9_PER_SLOT,
            // Active-positions cap per side (§1.4). Mirrors max_accounts —
            // no tighter per-side limit is enforced by the wrapper today.
            max_active_positions_per_side: max_accounts,
            // Cumulative-funding lifetime (§1.4 v12.18.x). Envelope matches
            // per-call `max_accrual_dt_slots` at minimum; wrapper uses the
            // same value because wrapper-deployed markets don't configure
            // a separate cumulative horizon. The engine asserts this at
            // init_engine_state and at every accrue.
            min_funding_lifetime_slots: crate::constants::MIN_FUNDING_LIFETIME_SLOTS,
        };
        Ok((params, insurance_floor))
    }
}

// 5. mod accounts (Pinocchio validation)
pub mod accounts {
    use crate::error::PercolatorError;
    use solana_program::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};

    /// Strict account-count check. Rejects if the caller passes more
    /// or fewer accounts than the handler expects.
    pub fn expect_len(accounts: &[AccountInfo], n: usize) -> Result<(), ProgramError> {
        if !crate::verify::len_ok(accounts.len(), n) {
            return Err(ProgramError::NotEnoughAccountKeys);
        }
        Ok(())
    }

    /// Variadic-tail check — used only by instructions with a
    /// documented tail forwarding convention (TradeCpi).
    pub fn expect_len_min(accounts: &[AccountInfo], n: usize) -> Result<(), ProgramError> {
        if !crate::verify::len_at_least(accounts.len(), n) {
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
        /// Scoped authority: may execute WithdrawInsurance (and the
        /// admin-only bounded WithdrawInsuranceLimited policy-setter
        /// path, once refactored). Independent of `admin`; can be
        /// delegated or burned via UpdateAuthority { kind=INSURANCE }.
        /// Initialized to the creator's pubkey at InitMarket, which
        /// yields a functional "super admin" by default.
        pub insurance_authority: [u8; 32],
        /// Scoped authority: may execute CloseSlab to sweep rent +
        /// residual insurance after the market resolves and all
        /// accounts close. Independent of `admin`; can be delegated
        /// or burned via UpdateAuthority { kind=CLOSE }. Burning
        /// traps rent forever (the traders-are-rug-proof setting).
        pub close_authority: [u8; 32],
    }

    /// Offset of _reserved field in SlabHeader, derived from offset_of! for correctness.
    pub const RESERVED_OFF: usize = offset_of!(SlabHeader, _reserved);

    // Portable compile-time assertion that RESERVED_OFF is 48 (expected layout).
    // The new insurance_authority / close_authority fields go AFTER _reserved,
    // so this offset is unchanged at 48.
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
        pub funding_max_e9_per_slot: i64,

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
        /// LEGACY TELEMETRY (not load-bearing): slot on which a prior
        /// two-phase ResolvePermissionless design first observed the
        /// external oracle as stale. Under the current STRICT HARD-
        /// TIMEOUT model the gate is `clock.slot - last_good_oracle
        /// _slot >= permissionless_resolve_stale_slots`, so this field
        /// no longer affects eligibility. Still maintained by
        /// read_price_clamped_with_external's external-Ok branch (clears
        /// to 0) for operators inspecting the slab via external tools.
        /// Layout-preserved for on-chain state compatibility.
        pub first_observed_stale_slot: u64,

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
        /// Under the STRICT HARD-TIMEOUT model, this is the authoritative
        /// liveness signal: `clock.slot - last_good_oracle_slot >=
        /// permissionless_resolve_stale_slots` makes the market stale
        /// and eligible for ResolvePermissionless, and also causes
        /// read_price_and_stamp to reject further price-taking ops.
        /// Stamped in read_price_and_stamp ONLY when the external
        /// oracle read returned Ok — authority fallback does NOT
        /// advance this field. Seeded to clock.slot at InitMarket.
        pub last_good_oracle_slot: u64,

        // ========================================
        // Fee-Weighted EWMA
        // ========================================
        /// Periodic maintenance fee per slot per account (engine units).
        /// Wrapper charges via engine.sync_account_fee_to_slot_not_atomic.
        /// 0 = disabled. Set at InitMarket, immutable.
        pub maintenance_fee_per_slot: u128,
        /// Incremental fee-sweep cursor: next bitmap word to scan on the
        /// next KeeperCrank. Per-account `last_fee_slot` on the engine side
        /// keeps the sweep correct across cranks — each account pays for
        /// its full elapsed interval the first time the cursor reaches it.
        /// Scanning is O(FEE_SWEEP_BUDGET) per crank regardless of
        /// max_accounts, so a 4096-account market doesn't blow the CU
        /// budget on a single crank.
        ///
        /// Repurposed from the former `last_fee_charge_slot` (now dead —
        /// replaced by per-account `Account::last_fee_slot`). Same 8-byte
        /// slot, same wire offset; only u16 is meaningful.
        pub fee_sweep_cursor_word: u64,
        /// Bit position within `fee_sweep_cursor_word` at which the next sweep
        /// resumes. Stored so the sweep can stop EXACTLY at FEE_SWEEP_BUDGET
        /// mid-word without losing remaining set bits to budget truncation.
        /// Only values 0..=63 are meaningful; wider values are normalized.
        /// Repurposed from the former `_fee_padding`.
        pub fee_sweep_cursor_bit: u64,
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
    /// Returns None if the counter would overflow (0 is reserved as "never materialized").
    pub fn next_mat_counter(data: &mut [u8]) -> Option<u64> {
        let old = read_mat_counter(data);
        let c = old.checked_add(1)?;
        write_mat_counter(data, c);
        Some(c)
    }

    // ========================================
    // Market Flags (stored in _padding[0] at offset 13)
    // ========================================

    /// Offset of flags byte in SlabHeader (_padding[0])
    pub const FLAGS_OFF: usize = 13;

    // Bit 1 (formerly FLAG_POLICY_CONFIGURED) is unused — the
    // policy-configured flag was removed along with the
    // SetInsuranceWithdrawPolicy / WithdrawInsuranceLimited
    // instructions. Do NOT reuse this bit without a state migration
    // (it may be set on any pre-delete slab; fresh deploy only).
    /// Flag bit: CPI is in progress (reentrancy guard for TradeCpi).
    /// Set before matcher CPI, cleared after. Any reentrant instruction
    /// that sees this flag must abort.
    pub const FLAG_CPI_IN_PROGRESS: u8 = 1 << 2;
    /// Flag bit: engine has received a real oracle price (not the init sentinel).
    /// Set on first successful oracle read (crank/trade/settle).
    /// Eliminates the "price 1 means uninitialized" sentinel overload.
    pub const FLAG_ORACLE_INITIALIZED: u8 = 1 << 3;

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

    /// Check if engine has received a real oracle price.
    pub fn is_oracle_initialized(data: &[u8]) -> bool {
        read_flags(data) & FLAG_ORACLE_INITIALIZED != 0
    }

    /// Mark engine as having received a real oracle price.
    pub fn set_oracle_initialized(data: &mut [u8]) {
        let flags = read_flags(data) | FLAG_ORACLE_INITIALIZED;
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
        // Full sanitization against corrupted slab data:
        // 1. Clamp count
        if buf.count as usize > crate::constants::RISK_BUF_CAP {
            buf.count = crate::constants::RISK_BUF_CAP as u8;
        }
        // 2. Zero entries past count
        for i in buf.count as usize..crate::constants::RISK_BUF_CAP {
            buf.entries[i] = crate::risk_buffer::RiskEntry::zeroed();
        }
        // 3. Filter invalid idx values
        for i in (0..buf.count as usize).rev() {
            if buf.entries[i].idx as usize >= percolator::MAX_ACCOUNTS {
                buf.remove(buf.entries[i].idx);
            }
        }
        // 4. Recompute min_notional from sanitized entries
        buf.recompute_min();
        // 5. Clamp scan_cursor
        if buf.scan_cursor as usize >= percolator::MAX_ACCOUNTS {
            buf.scan_cursor = 0;
        }
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

    // PriceUpdateV2 account layout (134 bytes minimum):
    //   discriminator(8) + write_authority(32) + verification_level(2)
    //     + PriceFeedMessage(76) + posted_slot(8) = 134
    //
    // The price-message block is parsed as the canonical pythnet_sdk
    // struct `pythnet_sdk::messages::PriceFeedMessage` via its
    // BorshDeserialize impl. Any breaking layout change Pyth ships
    // (field insertion, reordering, type change) surfaces as a
    // deserialize error at runtime or a compile error here — no more
    // silent garbage reads from stale fixed offsets. The enclosing
    // PriceUpdateV2 wrapper (discriminator + write_authority +
    // verification_level) remains at fixed offsets because that part
    // of the account is defined outside pythnet-sdk; a layout-audit
    // fixture test is the recommended additional guard for that
    // wrapper (see read_pyth_price_e6 comments).
    const PRICE_UPDATE_V2_MIN_LEN: usize = 134;
    const OFF_VERIFICATION_LEVEL: usize = 40; // enum discriminant (u8) + optional num_sigs (u8)
    const OFF_PRICE_FEED_MESSAGE: usize = 42; // PriceFeedMessage (72 bytes of fields)
    /// Pyth VerificationLevel::Full — enum tag value the Anchor
    /// serializer emits for the Full variant. Anchor writes the
    /// variant discriminant as one u8 followed by the variant payload
    /// (empty for Full, 1 byte num_signatures for Partial). Full is
    /// the second variant → tag byte = 1.
    const PYTH_VERIFICATION_FULL_TAG: u8 = 1;

    /// Compile-time assertion: fixed offsets in this module must match
    /// the on-chain PriceUpdateV2 layout. Any change in the wrapper or
    /// its LEN constant triggers a build error. PriceUpdateV2 is not
    /// exposed by pythnet-sdk (it lives in the Anchor-heavy
    /// pyth-solana-receiver-sdk), so we pin LEN explicitly to the
    /// documented value on the Pyth repo:
    ///   https://github.com/pyth-network/pyth-crosschain/blob/main/
    ///     target_chains/solana/pyth_solana_receiver_sdk/src/
    ///     price_update.rs (look for `pub const LEN`).
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
    /// Returns the price in e6 format (e.g., 150_000_000 = 150.00 in base units).
    pub fn read_pyth_price_e6(
        price_ai: &AccountInfo,
        expected_feed_id: &[u8; 32],
        now_unix_ts: i64,
        max_staleness_secs: u64,
        conf_bps: u16,
    ) -> Result<u64, ProgramError> {
        use pythnet_sdk::messages::PriceFeedMessage;

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
            if data[OFF_VERIFICATION_LEVEL] != PYTH_VERIFICATION_FULL_TAG {
                return Err(PercolatorError::OracleInvalid.into());
            }
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
        let msg = <PriceFeedMessage as borsh::BorshDeserialize>::deserialize(
            &mut &msg_slice[..],
        ).map_err(|_| PercolatorError::OracleInvalid)?;

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
    /// Authority fallback is gated on content-based external errors only
    /// (OracleStale / OracleConfTooWide). Caller-shaped errors
    /// (OracleInvalid for wrong owner / wrong feed / malformed data)
    /// propagate unchanged — otherwise a caller could force authority
    /// pricing by supplying a garbage oracle account. See
    /// read_price_clamped_with_external for the full policy table.
    pub fn read_price_clamped(
        config: &mut super::state::MarketConfig,
        price_ai: &AccountInfo,
        now_unix_ts: i64,
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
        read_price_clamped_with_external(config, external, now_unix_ts)
    }

    /// Same clamp/authority logic as `read_price_clamped`, but takes the
    /// already-parsed external oracle result. Lets callers that need both the
    /// external Ok/Err signal and the clamped price share a single Pyth parse
    /// (saves ~2K CU on hot paths like TradeNoCpi / WithdrawCollateral /
    /// KeeperCrank that previously parsed the oracle twice).
    /// Authority fallback only kicks in when the external oracle is
    /// genuinely UNUSABLE — i.e., OracleStale (feed past max_staleness_secs)
    /// or OracleConfTooWide (confidence band exceeds conf_filter_bps).
    /// Both signals come from the oracle's own content and cannot be
    /// manufactured by the caller.
    ///
    /// Other errors (OracleInvalid for wrong owner / wrong feed id /
    /// malformed data / bad account) DO NOT prove oracle unavailability:
    /// they just mean the caller supplied a bad account. Under the
    /// previous (buggy) policy, those errors would silently fall back
    /// to authority pricing, letting the caller pick between
    /// "authority clamped against a freshly advanced baseline" and
    /// "authority clamped against the stale baseline" by choosing
    /// whether to pass a valid oracle account. That gave the caller a
    /// full cap-step of control over the effective price on every
    /// instruction that reads the oracle. The predicate below closes
    /// that hole.
    #[inline]
    fn external_err_allows_authority_fallback(e: &ProgramError) -> bool {
        let stale_err: ProgramError = PercolatorError::OracleStale.into();
        let conf_err: ProgramError = PercolatorError::OracleConfTooWide.into();
        *e == stale_err || *e == conf_err
    }

    pub fn read_price_clamped_with_external(
        config: &mut super::state::MarketConfig,
        external: Result<u64, ProgramError>,
        now_unix_ts: i64,
    ) -> Result<u64, ProgramError> {
        // Oracle authority is a SEPARATE, WEAKER role than admin and may
        // outlive admin burn (Model 1). It acts as a bounded FALLBACK
        // price source — used only when the external oracle is genuinely
        // unusable. External primary, authority fallback:
        //
        //   external Ok                                -> return external
        //   external Err(OracleStale | ConfTooWide):
        //       authority fresh                        -> return clamped
        //                                                 authority price
        //       else                                   -> propagate external err
        //   external Err(other)                        -> propagate (caller-
        //                                                 chosen errors do
        //                                                 not justify fallback)
        //
        // Authority is always clamped against last_effective_price_e6
        // (the last FRESHLY observed external price). That baseline
        // advances only on successful external reads, so during an
        // external outage the authority price stays pinned within one
        // cap-width of the last known external price.
        //
        // Under the STRICT HARD-TIMEOUT policy, authority fallback is a
        // short-outage convenience during the window between external
        // going dark and ResolvePermissionless maturing. It does NOT
        // extend the market's oracle life:
        //   - Authority fallback does NOT advance last_good_oracle_slot
        //     (only external Ok does, in read_price_and_stamp).
        //   - Authority fallback does NOT clear first_observed_stale
        //     _slot (that field is legacy telemetry only under the
        //     hard-timeout model).
        //   - Once clock.slot - last_good_oracle_slot >=
        //     permissionless_resolve_stale_slots, read_price_and_stamp
        //     rejects even if authority is fresh — live price-taking
        //     ops can no longer continue through authority fallback
        //     once the hard timeout has matured.
        //
        // SetOracleAuthority / SetOraclePriceCap / UpdateAdmin (burn) /
        // PushOraclePrice enforce the invariant that non-Hyperp markets
        // with a configured authority also have a non-zero cap.
        if let Ok(ext_price) = external.as_ref() {
            let clamped_ext = clamp_oracle_price(
                config.last_effective_price_e6,
                *ext_price,
                config.oracle_price_cap_e2bps,
            );
            config.last_effective_price_e6 = clamped_ext;
            config.first_observed_stale_slot = 0;
            return Ok(clamped_ext);
        }

        // External failed. Only fall back to authority on content-based
        // unusability (stale / conf-too-wide); propagate caller-shaped
        // errors unchanged.
        let ext_err = match external {
            Ok(_) => unreachable!("external Ok handled above"),
            Err(e) => e,
        };
        if !external_err_allows_authority_fallback(&ext_err) {
            return Err(ext_err);
        }

        if let Some(auth_price) =
            read_authority_price(config, now_unix_ts, config.max_staleness_secs)
        {
            let clamped_auth = clamp_oracle_price(
                config.last_effective_price_e6,
                auth_price,
                config.oracle_price_cap_e2bps,
            );
            // Deliberately DO NOT clear first_observed_stale_slot here.
            // The stamp tracks continuous unusability of the defined
            // external oracle, not the authority fallback. Authority
            // fallback serves individual price reads during the delay
            // window but doesn't extend the external oracle's life —
            // clearing the stamp on fallback would block
            // ResolvePermissionless from ever maturing while authority
            // remains live, which is the "pinned theater" state the
            // unified policy fixes.
            return Ok(clamped_auth);
        }

        Err(ext_err)
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

    /// Hard-timeout predicate: has the market's configured oracle been
    /// stale for >= permissionless_resolve_stale_slots?
    ///
    /// Returns false when permissionless_resolve_stale_slots == 0
    /// (feature disabled — admin-only resolution).
    ///
    /// "Liveness slot" is:
    ///   non-Hyperp → config.last_good_oracle_slot (advances on successful
    ///                external reads only; NOT on authority fallback)
    ///   Hyperp     → config.last_mark_push_slot (advances ONLY on
    ///                full-weight mark observations: PushOraclePrice,
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
        config: &super::state::MarketConfig,
        clock_slot: u64,
    ) -> bool {
        if config.permissionless_resolve_stale_slots == 0 {
            return false;
        }
        let last_live_slot = if is_hyperp_mode(config) {
            config.last_mark_push_slot as u64
        } else {
            config.last_good_oracle_slot
        };
        clock_slot.saturating_sub(last_live_slot)
            >= config.permissionless_resolve_stale_slots
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
        // Strict hard-timeout gate (applies to both Hyperp and non-Hyperp):
        // once the oracle has been stale for >=
        // permissionless_resolve_stale_slots, no price read succeeds.
        // The market must be resolved before any further price-taking op.
        if permissionless_stale_matured(config, now_slot) {
            return Err(super::error::PercolatorError::OracleStale.into());
        }
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
            DEFAULT_FUNDING_K_BPS, DEFAULT_FUNDING_MAX_E9_PER_SLOT,
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
        RiskEngine, RiskError, U128, MAX_ACCOUNTS,
    };

    // settle_and_close_resolved removed — replaced by engine.force_close_resolved_not_atomic()
    // which handles K-pair PnL, checked arithmetic, and all settlement internally.

    /// Read oracle price for non-Hyperp markets and stamp last_good_oracle_slot
    /// ONLY when the external oracle read succeeds. Authority-fallback success
    /// does NOT stamp the field — it measures external-oracle liveness only.
    ///
    /// STRICT HARD-TIMEOUT GATE: if the hard stale window has matured
    /// (clock.slot - last_good_oracle_slot >= permissionless_resolve
    /// _stale_slots), this function rejects with OracleStale regardless
    /// of authority-fallback freshness. That prevents price-taking
    /// instructions (Trade, Withdraw, Crank, Settle, Convert, Catchup)
    /// from continuing to drift the engine price through authority
    /// fallback after the market is terminally stale — they must route
    /// to ResolvePermissionless instead.
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
        slab_data: &mut [u8],
    ) -> Result<u64, ProgramError> {
        // Hard-timeout gate: once `clock.slot - last_good_oracle_slot
        // >= permissionless_resolve_stale_slots`, the market is
        // TERMINALLY dead. No price read succeeds, even if the caller
        // now submits a fresh external oracle update. The only
        // allowed path forward is ResolvePermissionless. This
        // enforces "after N slots, dead means dead" uniformly — no
        // late keeper revival, no authority-fallback drift.
        if oracle::permissionless_stale_matured(config, clock_slot) {
            return Err(PercolatorError::OracleStale.into());
        }

        // Single Pyth/Chainlink parse shared between the "did the external read
        // succeed?" signal (used to stamp last_good_oracle_slot) and the clamped
        // price computation.
        let external = oracle::read_engine_price_e6(
            a_oracle,
            &config.index_feed_id,
            clock_unix_ts,
            config.max_staleness_secs,
            config.conf_filter_bps,
            config.invert,
            config.unit_scale,
        );
        let external_ok = external.is_ok();

        let price = oracle::read_price_clamped_with_external(
            config, external, clock_unix_ts,
        )?;

        if external_ok {
            config.last_good_oracle_slot = clock_slot;
        }
        // NOTE: FLAG_ORACLE_INITIALIZED is NOT set here.
        // The flag means "engine.last_oracle_price is a real price" which is
        // only true after the engine processes it via accrue_market_to or similar.
        // Setting it on wrapper read alone would be unsound because zero-fill
        // and other early-return paths skip the engine call.
        let _ = slab_data; // reserved for future per-read slab stamping
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
    /// Realize due maintenance fees for a single account up to `now_slot`.
    /// Idempotent: the engine's per-account `last_fee_slot` cursor prevents
    /// double-charging over the same interval, and a call at the same anchor
    /// as the cursor is a no-op (engine v12.18.4 §4.6.1).
    ///
    /// Wrappers MUST call this before any health-sensitive engine operation
    /// on the acting account when `maintenance_fee_per_slot > 0`, so that
    /// the margin / withdrawal / close check sees post-fee capital. Between
    /// cranks, each acting account self-realizes its share via this call;
    /// KeeperCrank sweeps the rest.
    ///
    /// No-op when `maintenance_fee_per_slot == 0`.
    ///
    /// Invariant: capital-sensitive operations MUST fully accrue the
    /// market (advance `last_market_slot` to `now_slot`) before syncing
    /// per-account fees. Oracle-backed paths satisfy this via
    /// `ensure_market_accrued_to_now` upstream. No-oracle paths (Deposit,
    /// DepositFeeCredits, InitUser, InitLP, TopUpInsurance,
    /// ReclaimEmptyAccount) cannot advance `last_market_slot` (no price /
    /// rate available), so they MUST pass an anchor that is already
    /// accrued — use `sync_account_fee_bounded_to_market` below rather
    /// than calling this helper with a wall-clock slot.
    ///
    /// Calling this with `now_slot > engine.last_market_slot` creates a
    /// `current_slot > last_market_slot` split that later breaks the
    /// accrual envelope: the next oracle-backed instruction will see an
    /// inflated `clock.slot - last_market_slot` dt and fail Overflow.
    fn sync_account_fee(
        engine: &mut RiskEngine,
        config: &MarketConfig,
        idx: u16,
        now_slot: u64,
    ) -> Result<(), ProgramError> {
        if config.maintenance_fee_per_slot == 0 {
            return Ok(());
        }
        engine
            .sync_account_fee_to_slot_not_atomic(
                idx,
                now_slot,
                config.maintenance_fee_per_slot,
            )
            .map_err(map_risk_error)
    }

    /// Fee-sync variant for no-oracle instructions. Caps the fee anchor
    /// at `engine.last_market_slot`, leaving full realization of fees
    /// accrued over `[last_market_slot, clock.slot]` to the next
    /// oracle-backed instruction. Prevents the `current_slot >
    /// last_market_slot` split that would otherwise brick later
    /// accrual.
    ///
    /// Acceptable trade-off: fees from the unaccrued tail are realized
    /// slightly later (on the next trade/crank/withdraw) instead of now.
    /// Correctness is preserved because the engine's per-account
    /// `last_fee_slot` still advances monotonically to the
    /// already-accrued boundary; subsequent sync calls cover the rest.
    fn sync_account_fee_bounded_to_market(
        engine: &mut RiskEngine,
        config: &MarketConfig,
        idx: u16,
        wallclock_slot: u64,
    ) -> Result<(), ProgramError> {
        if config.maintenance_fee_per_slot == 0 {
            return Ok(());
        }
        // Anchor: upper-bound by last_market_slot (no accrue in this
        // path) but floor at current_slot so sync_account_fee_to_slot_
        // not_atomic's monotonicity guard (now_slot >= current_slot)
        // holds even in the transient state where a no-oracle path
        // (InitUser/deposit) has advanced current_slot past last_market
        // _slot. In that state the account's last_fee_slot was seeded at
        // current_slot, so the anchor == current_slot case is a harmless
        // dt=0 no-op; the real fee realization happens on the next
        // oracle-backed op via ensure_market_accrued_to_now.
        let anchor = core::cmp::max(
            core::cmp::min(wallclock_slot, engine.last_market_slot),
            engine.current_slot,
        );
        engine
            .sync_account_fee_to_slot_not_atomic(
                idx,
                anchor,
                config.maintenance_fee_per_slot,
            )
            .map_err(map_risk_error)
    }

    /// Maximum number of max_dt chunks the in-line catchup can advance per
    /// instruction. Bounded by CU budget — each `accrue_market_to` is cheap
    /// but not free. For gaps beyond this, callers must use the dedicated
    /// `CatchupAccrue` instruction which commits progress atomically
    /// without attempting a main operation afterwards.
    ///
    /// 20 × max_dt ≈ 2M slots ≈ 11 days at 100k-slot envelope — well above
    /// the auditor's pathological 1.3M-slot example.
    const CATCHUP_CHUNKS_MAX: u32 = 20;

    /// Pre-chunk market-clock advancement when the gap since the last
    /// engine *accrue* exceeds `params.max_accrual_dt_slots`. The engine
    /// rejects any single `accrue_market_to` whose funding-active dt
    /// exceeds the envelope (spec §1.4 / §5.5 clause 6), so every
    /// accrue-bearing instruction (KeeperCrank, TradeCpi, TradeNoCpi,
    /// Withdraw, Liquidate, Close, Settle, Convert, live Insurance
    /// withdraw, Ordinary ResolveMarket, UpdateConfig) must close that
    /// gap before its own accrue.
    ///
    /// Cursor: loops on `engine.last_market_slot`, NOT `current_slot`.
    /// `last_market_slot` is the only cursor `accrue_market_to` uses to
    /// compute `total_dt = now_slot - last_market_slot`; `current_slot`
    /// can be advanced by non-accruing public endpoints (fee sync on Live,
    /// deposit/top-up without oracle) so it does not track market accrual.
    /// Earlier versions chunked from `current_slot`, which after any
    /// no-oracle self-advance would under-report the real gap and let the
    /// caller's own `accrue_market_to` hit Overflow on the residual.
    ///
    /// Caller supplies the catchup price and funding rate. Typical usage:
    /// the pre-oracle-read funding rate (`funding_rate_e9_pre`) and the
    /// fresh (or about-to-be-set) `oracle_price`. Using the caller-supplied
    /// rate (not 0) preserves anti-retroactivity — the rate reflects the
    /// mark/index state as it was before this instruction, not what the
    /// idle interval "should have" been (which is unknowable).
    ///
    /// If the gap exceeds `CATCHUP_CHUNKS_MAX × max_dt`, returns `Err`
    /// with `CatchupRequired` so the caller can surface "call CatchupAccrue
    /// first" instead of silently returning Ok and letting the subsequent
    /// main engine call Overflow-and-rollback (which would discard the
    /// catchup progress too, making the market unrecoverable in-line).
    ///
    /// No-op when the gap is already within the envelope, or when
    /// `max_dt == 0` (misconfiguration guard), or when the engine has never
    /// seen a real oracle observation (`last_oracle_price == 0`; the
    /// caller's own `_not_atomic` call will seed it).
    fn catchup_accrue(
        engine: &mut RiskEngine,
        now_slot: u64,
        price: u64,
        funding_rate_e9: i128,
    ) -> Result<(), ProgramError> {
        let max_dt = engine.params.max_accrual_dt_slots;
        if max_dt == 0 {
            return Ok(());
        }
        if now_slot <= engine.last_market_slot {
            return Ok(());
        }
        // Market never had a real oracle observation — nothing to catch up.
        // The caller's own _not_atomic call will seed last_oracle_price.
        if engine.last_oracle_price == 0 {
            return Ok(());
        }
        // Mirror the engine's own envelope predicate (§5.5 clause 6, v12.19):
        // accrue_market_to only enforces `total_dt > max_dt` Overflow when
        // funding would actually accumulate — i.e., rate != 0 AND both
        // sides have OI AND fund_px_last > 0. When funding is INACTIVE,
        // the engine will happily jump any dt in one call. If the wrapper
        // chunks anyway, it can raise CatchupRequired on paths where the
        // engine would legally advance directly — e.g.:
        //   - Dead-oracle UpdateConfig degenerate arm (rate = 0)
        //   - InitUser / InitLP / no-OI markets
        //   - ResolveMarket Degenerate (rate = 0)
        // Skip the chunking loop entirely when funding is inactive; the
        // caller's final accrue_market_to handles any dt in one shot.
        let funding_active = funding_rate_e9 != 0
            && engine.oi_eff_long_q != 0
            && engine.oi_eff_short_q != 0
            && engine.fund_px_last > 0;
        if !funding_active {
            return Ok(());
        }
        // Catchup chunks use the engine's STORED last_oracle_price, not the
        // caller's fresh `price`. Rationale: accrue_market_to's funding math
        // (engine §5.5) uses `fund_px_last` (the price at call entry) for
        // the fund_num_total = fund_px * rate * dt transfer, then sets
        // fund_px_last to the caller's `oracle_price`. If we used the fresh
        // price for every chunk:
        //   chunk 1: fund_px_0 = stored_P, total = stored_P * rate * max_dt,
        //            fund_px_last := fresh
        //   chunk 2: fund_px_0 = fresh,    total = fresh     * rate * max_dt
        //   ...
        // yielding a mathematically different transfer than a single-call
        // accrue (which would use stored_P throughout). Using stored_P for
        // every chunk keeps fund_px_last pinned at stored_P across the
        // chunked path, so the chunked sum equals the single-call transfer:
        //   stored_P * rate * total_dt (up to the caller's final boundary).
        // The caller's final accrue_market_to(now_slot, fresh, rate) then
        // applies fresh only to the residual, as a single-call accrue would.
        let chunk_price = engine.last_oracle_price;
        let _ = price; // caller's price used only by their final accrue_market_to
        let mut chunks: u32 = 0;
        while now_slot.saturating_sub(engine.last_market_slot) > max_dt {
            if chunks >= CATCHUP_CHUNKS_MAX {
                // Refuse to proceed — silently returning Ok here would let
                // the caller's main accrue hit Overflow on the residual
                // gap, rolling back ALL catchup work (no net progress).
                // Surfacing CatchupRequired instead tells callers/keepers
                // to use the dedicated CatchupAccrue instruction which
                // commits progress without attempting the main op.
                return Err(PercolatorError::CatchupRequired.into());
            }
            let step = engine.last_market_slot.saturating_add(max_dt);
            engine
                .accrue_market_to(step, chunk_price, funding_rate_e9)
                .map_err(map_risk_error)?;
            chunks = chunks.saturating_add(1);
        }
        Ok(())
    }

    /// Fully advance the engine's market clock to `now_slot` before any
    /// per-account fee sync. This is an explicit-ordering helper:
    /// `catchup_accrue` brings the gap within the envelope, then a final
    /// `accrue_market_to(now_slot)` closes the residual so subsequent
    /// `sync_account_fee_to_slot_not_atomic(..., now_slot, ...)` runs
    /// against a fully-accrued market.
    ///
    /// Why explicit, when the engine already self-handles it via the main
    /// op's internal accrue? Because even though the engine uses
    /// `last_market_slot` (not `current_slot`) for funding dt — so the
    /// interval is never erased (see
    /// `test_fee_sync_does_not_erase_market_accrual_interval`) — making
    /// the ordering explicit in the wrapper removes all ambiguity and
    /// aligns with the auditor-requested pattern:
    /// `ensure_market_accrued_to_now; sync_account_fee; engine.<op>_not_atomic`.
    ///
    /// The main op's internal `accrue_market_to(now_slot, price, rate)`
    /// then hits the same-slot + same-price no-op branch (engine §5.4
    /// early return) — about 150 CU of redundancy, bought for ordering
    /// clarity.
    ///
    /// No-op when the engine has no oracle observation yet (price=0
    /// catchup is unsafe) or when the gap is already zero.
    fn ensure_market_accrued_to_now(
        engine: &mut RiskEngine,
        now_slot: u64,
        price: u64,
        funding_rate_e9: i128,
    ) -> Result<(), ProgramError> {
        catchup_accrue(engine, now_slot, price, funding_rate_e9)?;
        if price > 0 && now_slot > engine.last_market_slot {
            engine
                .accrue_market_to(now_slot, price, funding_rate_e9)
                .map_err(map_risk_error)?;
        }
        Ok(())
    }

    /// Incrementally sweep maintenance fees from the current cursor position.
    /// Scans bitmap words starting at `(fee_sweep_cursor_word,
    /// fee_sweep_cursor_bit)`, calling `sync_account_fee_to_slot_not_atomic`
    /// on every set bit. Stops EXACTLY at `FEE_SWEEP_BUDGET` syncs — the bit
    /// cursor lets us pause mid-word without losing remaining set bits to
    /// budget truncation.
    ///
    /// Correctness: the engine's per-account `last_fee_slot` is the source of
    /// truth. When the cursor reaches an account, that account's sync call
    /// realizes fees for the *entire* elapsed interval
    /// `[account.last_fee_slot, now_slot]` in one charge — no fees are lost
    /// between cursor visits. Self-acting accounts realize their own fees
    /// inline on every capital-sensitive instruction (see `sync_account_fee`);
    /// the sweep handles everything that hasn't self-acted.
    ///
    /// CU bound: at most `FEE_SWEEP_BUDGET` sync calls per crank (strictly,
    /// thanks to the bit cursor), plus O(BITMAP_WORDS) word reads. Constant
    /// in `max_accounts`, so a 4096-slot market is handled the same as a
    /// 64-slot market.
    fn sweep_maintenance_fees(
        engine: &mut RiskEngine,
        config: &mut MarketConfig,
        now_slot: u64,
        max_syncs: usize,
    ) -> Result<(), ProgramError> {
        if config.maintenance_fee_per_slot == 0 {
            return Ok(());
        }
        // Early-out when the caller has already exhausted the per-
        // instruction sync budget on pre-sweep candidate syncs.
        if max_syncs == 0 {
            return Ok(());
        }
        const BITMAP_WORDS: usize = (percolator::MAX_ACCOUNTS + 63) / 64;
        // Normalize cursor in case of stale/corrupt values.
        let mut word_cursor = (config.fee_sweep_cursor_word as usize) % BITMAP_WORDS;
        let mut bit_cursor = (config.fee_sweep_cursor_bit as usize) & 63;
        let mut syncs_done: usize = 0;
        let mut words_scanned: usize = 0;
        // Budget check is inside the inner loop so we can stop exactly at
        // max_syncs, not after completing the current word.
        'outer: while words_scanned < BITMAP_WORDS {
            // Skip bits below bit_cursor on the resume word.
            let resume_mask = if bit_cursor == 0 {
                u64::MAX
            } else {
                // Clear bits 0..bit_cursor (they were already processed last call).
                !((1u64 << bit_cursor).wrapping_sub(1))
            };
            let mut bits = engine.used[word_cursor] & resume_mask;
            while bits != 0 {
                if syncs_done >= max_syncs {
                    // Stop EXACTLY at budget. Save the next unprocessed bit
                    // as the resume point for the following crank.
                    let next_bit = bits.trailing_zeros() as usize;
                    config.fee_sweep_cursor_word = word_cursor as u64;
                    config.fee_sweep_cursor_bit = next_bit as u64;
                    return Ok(());
                }
                let bit = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                let idx = word_cursor * 64 + bit;
                if idx >= percolator::MAX_ACCOUNTS {
                    continue;
                }
                engine
                    .sync_account_fee_to_slot_not_atomic(
                        idx as u16,
                        now_slot,
                        config.maintenance_fee_per_slot,
                    )
                    .map_err(map_risk_error)?;
                syncs_done += 1;
            }
            // Word fully drained — advance to next word, reset bit cursor.
            word_cursor = (word_cursor + 1) % BITMAP_WORDS;
            bit_cursor = 0;
            words_scanned += 1;
            // Budget may have hit right at the end of the word — avoid one
            // wasted iteration on the next (empty in the caller's view) word.
            if syncs_done >= max_syncs {
                break 'outer;
            }
        }
        config.fee_sweep_cursor_word = word_cursor as u64;
        config.fee_sweep_cursor_bit = 0;
        Ok(())
    }

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

        // Clamp: funding_max_e9_per_slot is already in engine-native e9 units.
        let max_rate_e9 = config.funding_max_e9_per_slot as i128;
        per_slot.clamp(-max_rate_e9, max_rate_e9)
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
        maintenance_fee_per_slot: u128,
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
        let admit_h_min = engine.params.h_min;
        let admit_h_max = engine.params.h_max;
        // Realize due maintenance fees on both counterparties BEFORE the trade
        // so margin checks see post-fee capital. No-op when fee rate is 0.
        if maintenance_fee_per_slot > 0 {
            engine.sync_account_fee_to_slot_not_atomic(
                a, now_slot, maintenance_fee_per_slot,
            )?;
            engine.sync_account_fee_to_slot_not_atomic(
                b, now_slot, maintenance_fee_per_slot,
            )?;
        }
        engine.execute_trade_not_atomic(
            a,
            b,
            oracle_price,
            now_slot,
            abs_size,
            exec.price,
            funding_rate_e9,
            admit_h_min,
            admit_h_max,
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

    // UpdateAuthority kind constants. Keep in a single place so the
    // decoder, handler, and any future on-chain schema references agree.
    pub const AUTHORITY_ADMIN: u8 = 0;
    pub const AUTHORITY_ORACLE: u8 = 1;
    pub const AUTHORITY_INSURANCE: u8 = 2;
    pub const AUTHORITY_CLOSE: u8 = 3;

    /// Standalone handler for UpdateAuthority. Extracted from
    /// process_instruction to keep its stack frame independent —
    /// inlining adds a full MarketConfig + SlabHeader to the giant
    /// process_instruction frame, which overflows the Solana BPF
    /// stack.
    #[inline(never)]
    fn handle_update_authority<'a>(
        program_id: &Pubkey,
        accounts: &[AccountInfo<'a>],
        kind: u8,
        new_pubkey: Pubkey,
    ) -> Result<(), ProgramError> {
        accounts::expect_len(accounts, 3)?;
        let a_current = &accounts[0];
        let a_new = &accounts[1];
        let a_slab = &accounts[2];

        accounts::expect_signer(a_current)?;
        accounts::expect_writable(a_slab)?;

        let mut data = state::slab_data_mut(a_slab)?;
        slab_guard(program_id, a_slab, &data)?;
        require_initialized(&data)?;

        let new_bytes = new_pubkey.to_bytes();
        let is_burn = new_bytes == [0u8; 32];

        // Hard-timeout gate for NON-BURN updates only. Burns strictly
        // REMOVE power and are the mechanism operators use to reach
        // the fully admin-free terminal state — blocking them past
        // maturity would permanently trap a market in a partially-
        // burned state. Transfers (non-burn) past maturity are still
        // rejected, consistent with "matured markets are terminal."
        if !is_burn {
            let clock_gate = Clock::get()
                .map_err(|_| ProgramError::UnsupportedSysvar)?;
            let cfg_gate = state::read_config(&data);
            if oracle::permissionless_stale_matured(&cfg_gate, clock_gate.slot) {
                return Err(PercolatorError::OracleStale.into());
            }
        }

        // New pubkey must consent unless this is a burn.
        if !is_burn {
            accounts::expect_signer(a_new)?;
            accounts::expect_key(a_new, &new_pubkey)?;
        }

        // Read current authority pubkey only (not the whole header/
        // config), to keep the frame small.
        let mut header = state::read_header(&data);
        let mut config = state::read_config(&data);

        let current_bytes = match kind {
            AUTHORITY_ADMIN => header.admin,
            AUTHORITY_ORACLE => config.oracle_authority,
            AUTHORITY_INSURANCE => header.insurance_authority,
            AUTHORITY_CLOSE => header.close_authority,
            _ => return Err(ProgramError::InvalidInstructionData),
        };
        require_admin(current_bytes, a_current.key)?;

        // Kind-specific invariants at assignment time.
        match kind {
            AUTHORITY_ADMIN => {
                if is_burn {
                    // Burning admin requires permissionless paths so the
                    // market lifecycle can complete without admin. Non-
                    // admin kinds have no such guards (burning them
                    // simply removes that capability, which is a
                    // legitimate rug-proofing configuration).
                    let (resolved, has_accounts) = {
                        let engine = zc::engine_ref(&data)?;
                        (engine.is_resolved(), engine.num_used_accounts > 0)
                    };
                    if !resolved {
                        if config.permissionless_resolve_stale_slots == 0
                            || config.force_close_delay_slots == 0
                        {
                            return Err(PercolatorError::InvalidConfigParam.into());
                        }
                    } else if has_accounts && config.force_close_delay_slots == 0 {
                        return Err(PercolatorError::InvalidConfigParam.into());
                    }
                    // Note: no is_policy_configured check. Under the
                    // 4-way split, admin and insurance_authority are
                    // independent; burning admin doesn't retain a back-
                    // channel — the insurance_authority's withdrawal
                    // policy is bounded by what admin configured BEFORE
                    // burn. Operators who want full rug-proofing also
                    // burn insurance_authority.
                }
            }
            AUTHORITY_ORACLE => {
                // Hyperp: rejecting zero-address oracle authority is
                // only needed when the EWMA isn't bootstrapped (no
                // trades → no mark source).
                if oracle::is_hyperp_mode(&config)
                    && is_burn
                    && config.mark_ewma_e6 == 0
                {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                // Weaker-authority invariant: non-Hyperp markets with
                // a configured authority must have a non-zero cap.
                if !oracle::is_hyperp_mode(&config)
                    && !is_burn
                    && config.oracle_price_cap_e2bps == 0
                {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                // Non-Hyperp resolvability: burning oracle_authority
                // removes the only path that can populate authority_
                // price_e6 (PushOraclePrice). Admin ResolveMarket then
                // only works if perm_resolve is also configured — the
                // perm-stale terminal branch uses engine.last_oracle_
                // price instead of authority_price_e6. Refuse the
                // burn if perm_resolve == 0, otherwise the market
                // loses every resolve path the moment this tx lands.
                if !oracle::is_hyperp_mode(&config)
                    && is_burn
                    && config.permissionless_resolve_stale_slots == 0
                {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
            }
            AUTHORITY_INSURANCE | AUTHORITY_CLOSE => {
                // No per-kind invariants. Burning is a legitimate
                // no-rug configuration; setting to any pubkey is a
                // normal delegation.
            }
            _ => unreachable!(),
        }

        // Commit the assignment.
        match kind {
            AUTHORITY_ADMIN => {
                header.admin = new_bytes;
                state::write_header(&mut data, &header);
            }
            AUTHORITY_ORACLE => {
                config.oracle_authority = new_bytes;
                // Clear stored price on authority change for non-Hyperp
                // markets — prevents carrying over a prior authority's
                // last push. (The prior is_policy_configured
                // exemption is gone along with the removed
                // SetInsuranceWithdrawPolicy; oracle fields are no
                // longer repurposed post-resolve.)
                if !oracle::is_hyperp_mode(&config) {
                    config.authority_price_e6 = 0;
                    config.authority_timestamp = 0;
                }
                state::write_config(&mut data, &config);
            }
            AUTHORITY_INSURANCE => {
                header.insurance_authority = new_bytes;
                state::write_header(&mut data, &header);
            }
            AUTHORITY_CLOSE => {
                header.close_authority = new_bytes;
                state::write_header(&mut data, &header);
            }
            _ => unreachable!(),
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
                funding_max_e9_per_slot: custom_max_per_slot,
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
                // max_staleness_secs: reject 0 and unreasonable values.
                // 0 would brick oracle reads. >7 days is clearly misconfigured.
                if max_staleness_secs == 0 || max_staleness_secs > 7 * 86400 {
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
                // Liveness (Finding 6): cap force_close_delay_slots so the
                // admin-burn guard's "force close is enabled" check actually
                // implies force close is REACHABLE. Without this, an admin
                // could init with delay=u64::MAX, pass the guard, then burn.
                // After resolution, resolved_slot.saturating_add(delay) would
                // saturate to u64::MAX and ForceCloseResolved would never pass
                // the time check, permanently stranding any remaining accounts.
                if force_close_delay_slots > crate::constants::MAX_FORCE_CLOSE_DELAY_SLOTS {
                    return Err(ProgramError::InvalidInstructionData);
                }
                // Maintenance-fee feature uses engine v12.18.4's per-account
                // last_fee_slot cursor via sync_account_fee_to_slot_not_atomic.
                // Every capital-sensitive instruction below realizes due fees
                // on the acting account before the engine's health check, so
                // no account can act on stale capital. New accounts are never
                // back-charged (engine Goal 47: last_fee_slot seeded at
                // materialization).
                // §14.1: H_max must not exceed permissionless resolve delay.
                // Otherwise warmup cohorts could mature after the market is already
                // permissionlessly resolved, creating inconsistent terminal state.
                if permissionless_resolve_stale_slots > 0
                    && risk_params.h_max > permissionless_resolve_stale_slots
                {
                    return Err(ProgramError::InvalidInstructionData);
                }
                // Envelope compatibility — conservative product bound kept
                // even though ResolvePermissionless itself bypasses
                // accrue_market_to via the engine's Degenerate arm (which
                // runs at rate = 0 and jumps last_market_slot to now_slot
                // without an envelope check). Reasoning for keeping it:
                //  (a) Admin ResolveMarket's Ordinary arm DOES accrue,
                //      and the engine's envelope check can reject if dt
                //      exceeds max_accrual_dt_slots. Keeping this check
                //      at init time keeps the two configs aligned so any
                //      resolve path — admin or permissionless — can be
                //      reached without hitting an envelope-rejection.
                //  (b) It's a sanity ceiling on how long a healthy market
                //      can stay unresolved; a stale threshold larger than
                //      the envelope would imply "wait N days before
                //      resolving" without ever re-accruing the market in
                //      that window, which usually indicates misconfiguration.
                // Not required for safety of the permissionless path alone.
                if permissionless_resolve_stale_slots > 0
                    && permissionless_resolve_stale_slots > risk_params.max_accrual_dt_slots
                {
                    return Err(ProgramError::InvalidInstructionData);
                }

                // Hyperp catchup-budget invariant. The real safety
                // property under the "resolve-only after maturity"
                // design is: before the market matures into the
                // permissionless-resolve window, a single live
                // recovery call (PushOraclePrice + catchup_accrue)
                // must be able to close the accrued market gap.
                // catchup_accrue loops at most CATCHUP_CHUNKS_MAX
                // accrue_market_to calls per instruction, each
                // advancing by up to max_accrual_dt_slots, so the
                // single-call recovery budget is
                //   CATCHUP_CHUNKS_MAX × MAX_ACCRUAL_DT_SLOTS.
                // If perm_resolve exceeds that budget, a market that
                // has gone stale near maturity may not be recoverable
                // in one shot — users would have to invoke the
                // dedicated CatchupAccrue instruction multiple times.
                // Refuse that configuration at init so the bound is
                // mechanically enforced.
                //
                // Note: the general-purpose check above (perm_resolve
                // <= risk_params.max_accrual_dt_slots) is strictly
                // tighter, so this assertion is currently redundant —
                // kept as a first-class, self-contained statement of
                // the Hyperp safety property in case either constant
                // moves.
                if is_hyperp && permissionless_resolve_stale_slots > 0 {
                    let max_recoverable_gap =
                        (CATCHUP_CHUNKS_MAX as u64)
                            .saturating_mul(crate::constants::MAX_ACCRUAL_DT_SLOTS);
                    if permissionless_resolve_stale_slots > max_recoverable_gap {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                }

                // Non-Hyperp resolvability invariant. A non-Hyperp market
                // resolves by one of two paths:
                //   (a) admin ResolveMarket, which requires a non-zero
                //       authority_price_e6. The only way to populate that
                //       field is PushOraclePrice, which in turn requires a
                //       non-zero oracle_authority AND oracle_price_cap_
                //       e2bps > 0. The init-time default of oracle_
                //       authority is zero when min_oracle_price_cap_e2bps
                //       == 0, so a zero cap removes path (a) forever.
                //   (b) ResolvePermissionless, gated by permissionless_
                //       stale_matured which returns false when perm_
                //       resolve == 0.
                // Refuse init configurations that start without BOTH
                // paths. This preserves the "dead means resolvable"
                // invariant for non-Hyperp markets regardless of later
                // authority burns. Hyperp is exempt — mark is trade-
                // sourced and the Hyperp dead-zone guard above handles
                // its own resolvability.
                if !is_hyperp
                    && permissionless_resolve_stale_slots == 0
                    && min_oracle_price_cap_e2bps == 0
                {
                    return Err(PercolatorError::InvalidConfigParam.into());
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
                    // Wire value is already in engine-native e9 units; compare
                    // directly against the RiskParams envelope stored by the
                    // wrapper (read_risk_params). Compare in i128 so i64::MAX
                    // can't wrap before the bound check.
                    if ms < 0 || (ms as i128) > risk_params.max_abs_funding_e9_per_slot as i128 {
                        return Err(PercolatorError::InvalidConfigParam.into());
                    }
                }
                // mark_min_fee upper bound: prevent setting so high that
                // EWMA never updates. Compare in u128 — MAX_PROTOCOL_FEE
                // _ABS is 10^36, casting to u64 wraps modulo 2^64 and
                // yields a meaningless threshold. mark_min_fee is u64
                // so under the 10^36 ceiling this check is effectively
                // "always passes", which matches the spec (no tighter
                // per-market cap today). If a tighter cap is desired in
                // the future (e.g., MAX_VAULT_TVL-scaled), replace the
                // ceiling here.
                if (mark_min_fee as u128) > percolator::MAX_PROTOCOL_FEE_ABS {
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
                // Engine requires init_oracle_price > 0 (asserted in new_with_market).
                // Hyperp: use the admin-chosen initial mark price.
                // Non-Hyperp: REQUIRE a successful oracle read at init. The engine's
                //   last_oracle_price must be a real economic value, not a positive
                //   sentinel overloaded to mean "uninitialized" — per spec goal 38
                //   (no valid positive price may encode "no price yet"). If the
                //   oracle is unavailable at init time the admin must retry when
                //   the feed is live; there is no sentinel path.
                let init_price = if is_hyperp {
                    initial_mark_price_e6
                } else {
                    // Read the external oracle NOW; propagate any error (stale,
                    // wrong feed, malformed). Success seeds engine.last_oracle_price
                    // with a real price and lets us mark the oracle-initialized
                    // flag unconditionally — no FLAG_ORACLE_INITIALIZED gating
                    // needed for engine reads after this point.
                    let fresh = oracle::read_engine_price_e6(
                        a_oracle,
                        &index_feed_id,
                        clock.unix_timestamp,
                        max_staleness_secs,
                        conf_filter_bps,
                        invert,
                        unit_scale,
                    )?;
                    if fresh == 0 || fresh > percolator::MAX_ORACLE_PRICE {
                        return Err(PercolatorError::OracleInvalid.into());
                    }
                    fresh
                };

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
                    // new_account_fee removed in engine v12.18.1 (§10.2).
                    // Warmup horizon: 0 < h_min <= h_max. h_max == 0 would
                    // zero-width the warmup cohort and break reserve logic
                    // at admission; the engine asserts this but we pre
                    // -validate for clearer errors at init time.
                    if p.h_max == 0 || p.h_min > p.h_max {
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
                    funding_max_e9_per_slot: custom_max_per_slot.unwrap_or(DEFAULT_FUNDING_MAX_E9_PER_SLOT),
                    // Oracle authority defaults to the creator's pubkey
                    // (super-admin by default, consistent with
                    // insurance_authority and close_authority). The
                    // admin can transfer it to a separate key or burn
                    // it via UpdateAuthority { kind: AUTHORITY_ORACLE }.
                    // Under the weaker-authority model, non-Hyperp
                    // markets with a nonzero oracle_authority must
                    // have oracle_price_cap_e2bps > 0 — this is
                    // checked at PushOraclePrice time (live defense)
                    // and at UpdateAuthority(ORACLE) time (set-time
                    // invariant). At init, enforce the SAME invariant:
                    // non-Hyperp with min_cap == 0 defaults
                    // oracle_authority to zero. This prevents the
                    // half-configured "visible authority, unusable
                    // push" state the auditor flagged. Once zero,
                    // authority is terminal (UpdateAuthority can't
                    // transfer from a zero current slot) — operators
                    // who want oracle push in non-Hyperp markets must
                    // init with min_cap > 0.
                    // Hyperp markets always get cap > 0 from
                    // DEFAULT_HYPERP_PRICE_CAP_E2BPS, so authority
                    // defaults to admin there.
                    oracle_authority: if is_hyperp || min_oracle_price_cap_e2bps > 0 {
                        a_admin.key.to_bytes()
                    } else {
                        [0u8; 32]
                    },
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
                    // Seed last_effective_price_e6 with the genesis reading so the
                    // circuit-breaker baseline is real from genesis too (not 0, which
                    // disables the breaker on first oracle read). For non-Hyperp we
                    // just read init_price from the feed above, so reuse it.
                    last_effective_price_e6: if is_hyperp { initial_mark_price_e6 } else { init_price },
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
                    first_observed_stale_slot: 0,
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
                    fee_sweep_cursor_word: 0,
                    fee_sweep_cursor_bit: 0,
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
                    // Default the scoped authorities to the creator's
                    // pubkey — yields a functional super-admin out of
                    // the box. Operators who want capability isolation
                    // call UpdateAuthority with the specific kind.
                    insurance_authority: a_admin.key.to_bytes(),
                    close_authority: a_admin.key.to_bytes(),
                };
                state::write_header(&mut data, &new_header);
                // Step 4: Explicitly initialize nonce to 0 for determinism
                state::write_req_nonce(&mut data, 0);
                // Oracle is now initialized from genesis in both modes:
                //   Hyperp    — mark IS the oracle, seeded from initial_mark_price_e6.
                //   Non-Hyperp — we performed a real oracle read above and used the
                //                result as init_price, so last_oracle_price is a real
                //                economic value (spec goal 38 — no sentinel).
                state::set_oracle_initialized(&mut data);
            }
            Instruction::InitUser { fee_payment } => {
                // Spec §10.2: deposit is the canonical materialization path
                // — pure capital transfer, MUST NOT accrue_market_to, MUST
                // NOT mutate side state. Therefore InitUser does not read
                // the oracle and does not require a fresh oracle. The 6-
                // account layout drops the oracle account from the earlier
                // revision; the engine's check_live_accrual_envelope inside
                // deposit_not_atomic is the only staleness gate (bounded
                // by max_accrual_dt_slots, not conf_filter/max_staleness_
                // secs), so account onboarding stays live through oracle
                // outages up to the market-accrual envelope. Callers that
                // need to widen the envelope invoke KeeperCrank first.
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

                // Hard-timeout gate: pure-deposit is still a live mutation.
                // Once the market has matured into the permissionless
                // resolve window, no further mutations are permitted.
                if oracle::permissionless_stale_matured(&config, clock.slot) {
                    return Err(PercolatorError::OracleStale.into());
                }

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
                if idx as usize >= percolator::MAX_ACCOUNTS {
                    return Err(PercolatorError::EngineOverflow.into());
                }
                // deposit_not_atomic materializes, seeds last_fee_slot at
                // clock.slot (Goal 47: no back-charge), advances current
                // _slot to clock.slot, and enforces the live-accrual
                // envelope. No wrapper accrue, no oracle read.
                engine
                    .deposit_not_atomic(idx, units as u128, 0, clock.slot)
                    .map_err(map_risk_error)?;
                engine.set_owner(idx, a_user.key.to_bytes())
                    .map_err(map_risk_error)?;
                drop(engine);
                let gen = state::next_mat_counter(&mut data)
                    .ok_or(PercolatorError::EngineOverflow)?;
                state::write_account_generation(&mut data, idx, gen);
            }
            Instruction::InitLP {
                matcher_program,
                matcher_context,
                fee_payment,
            } => {
                // Same 6-account layout and pure-deposit semantics as
                // InitUser: spec §10.2 makes account creation a pure
                // capital path that must not read the oracle or call
                // accrue_market_to.
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

                if oracle::permissionless_stale_matured(&config, clock.slot) {
                    return Err(PercolatorError::OracleStale.into());
                }

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
                if idx as usize >= percolator::MAX_ACCOUNTS {
                    return Err(PercolatorError::EngineOverflow.into());
                }
                engine
                    .deposit_not_atomic(idx, units as u128, 0, clock.slot)
                    .map_err(map_risk_error)?;
                engine.set_owner(idx, a_user.key.to_bytes())
                    .map_err(map_risk_error)?;
                engine.accounts[idx as usize].kind = percolator::Account::KIND_LP;
                engine.accounts[idx as usize].matcher_program = matcher_program.to_bytes();
                engine.accounts[idx as usize].matcher_context = matcher_context.to_bytes();
                drop(engine);
                let gen = state::next_mat_counter(&mut data)
                    .ok_or(PercolatorError::EngineOverflow)?;
                state::write_account_generation(&mut data, idx, gen);
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

                // Hard-timeout gate: once the market has been oracle-stale
                // for >= permissionless_resolve_stale_slots, it is
                // terminally dead. No live mutations — including
                // no-oracle deposits — should proceed. Users must exit
                // via ResolvePermissionless + resolved-market close
                // paths. Rejecting BEFORE the SPL transfer so funds
                // are not moved into a dead market.
                if oracle::permissionless_stale_matured(&config, clock.slot) {
                    return Err(PercolatorError::OracleStale.into());
                }

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

                // No-oracle path: pass clock.slot to deposit_not_atomic.
                // The engine's check_live_accrual_envelope gates on dt =
                // clock.slot - last_market_slot <= max_accrual_dt_slots —
                // the same safety bound that ensures the next oracle-
                // backed instruction's accrue won't exceed its envelope.
                // Fee anchoring is capped at last_market_slot by
                // sync_account_fee_bounded_to_market (per spec §10.7: no
                // accrue in this no-oracle path); the residual tail
                // (last_market_slot, clock.slot] is realized by the
                // next oracle-backed op via ensure_market_accrued_to_now.
                sync_account_fee_bounded_to_market(
                    engine, &config, user_idx, clock.slot,
                )?;

                engine
                    .deposit_not_atomic(user_idx, units as u128, 0, clock.slot)
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
                // Anti-retroactivity: capture funding rate before oracle read (§5.5)
                let funding_rate_e9 = compute_current_funding_rate_e9(&config);
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
                        read_price_and_stamp(&mut config, a_oracle_idx, clock.unix_timestamp, clock.slot, &mut data)?
                    };
                    state::write_config(&mut data, &config);
                    px
                };

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, user_idx)?;

                let owner = engine.accounts[user_idx as usize].owner;
                if !crate::verify::owner_ok(owner, a_user.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                if config.unit_scale != 0 && amount % config.unit_scale as u64 != 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }

                let (units_requested, _) = crate::units::base_to_units(amount, config.unit_scale);

                let withdraw_slot = clock.slot;
                let admit_h_min = engine.params.h_min;
                let admit_h_max = engine.params.h_max;
                // Fully accrue the market to clock.slot BEFORE syncing fees.
                // Explicit ordering — the engine already handles this via
                // withdraw_not_atomic's internal accrue (which uses
                // last_market_slot, not current_slot, for dt), but making
                // the accrue→sync→op order explicit in the wrapper removes
                // all ambiguity and aligns with the auditor-requested
                // pattern. The main op's internal accrue then no-ops
                // (same slot + same price, engine §5.4 early return).
                ensure_market_accrued_to_now(engine, clock.slot, price, funding_rate_e9)?;
                // Realize due maintenance fees BEFORE the withdrawal margin
                // check, so the account can't withdraw against pre-fee capital.
                sync_account_fee(engine, &config, user_idx, clock.slot)?;
                engine
                    .withdraw_not_atomic(user_idx, units_requested as u128, price, withdraw_slot,
                        funding_rate_e9, admit_h_min, admit_h_max)
                    .map_err(map_risk_error)?;
                drop(engine);
                if !state::is_oracle_initialized(&data) {
                    state::set_oracle_initialized(&mut data);
                }

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

                    // Resolved markets are frozen: no K/F mutation, no per-account
                    // settlement. The end-of-instruction lifecycle is a no-op here,
                    // and engine v12.18.1 no longer exposes it publicly (it is folded
                    // into the _not_atomic methods that actually change state).
                    let _ = engine; // silence "unused" when no crank work runs.

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

                // Capture pre-oracle-read funding rate for anti-retroactivity (§5.5).
                // The rate for interval [last_market_slot, now_slot] must reflect
                // mark vs index DURING that interval, not the post-read state.
                let funding_rate_e9_pre = compute_current_funding_rate_e9(&config);

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
                    read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot, &mut data)?
                };

                state::write_config(&mut data, &config);
                // FLAG_ORACLE_INITIALIZED now set inside read_price_and_stamp/get_engine_oracle_price_e6

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
                // Defense-in-depth cap: the decode-time check already
                // bounds candidates, but truncating `combined` here as
                // well ensures the engine's scan is bounded even if the
                // decode cap is loosened in the future or an insider
                // path ever constructs `combined` differently. Cap:
                // 4 (risk buffer max) + 2 × LIQ_BUDGET_PER_CRANK.
                const COMBINED_CAP: usize =
                    4 + (percolator::LIQ_BUDGET_PER_CRANK as usize) * 2;
                if combined.len() > COMBINED_CAP {
                    combined.truncate(COMBINED_CAP);
                }

                // ── Periodic maintenance fees (wrapper-owned, §8.3) ──
                //
                // Engine v12.18.4 provides a per-account fee cursor
                // (Account::last_fee_slot) and an idempotent public API
                // (sync_account_fee_to_slot_not_atomic). Per-account tracking
                // lets us correctly handle:
                //   - New accounts joining mid-interval — seeded at the
                //     materialization slot, so no back-charge (Goal 47).
                //   - Self-acting accounts — realize fees in their own
                //     instruction (via sync_account_fee below); KeeperCrank
                //     re-visiting them is a no-op at the same anchor.
                //   - Shortfalls — routed through charge_fee_to_insurance as
                //     fee-credits debt; never fails with InsufficientBalance.
                //
                // Ordering: sweep BEFORE keeper_crank_not_atomic so the crank's
                // lifecycle (side-mode drain detection / resets / health
                // reconciliation) observes fee-induced state.
                //
                // Crank reward: pay CRANK_REWARD_BPS (50 %) of the maintenance-
                // fee sweep delta back to the non-permissionless caller as
                // capital. The remaining 50% stays in insurance. No additional
                // account-count cap — the sweep itself is the bound (at most
                // FEE_SWEEP_BUDGET accounts per crank, and each contribution
                // is `fee_per_slot × dt_that_account_owed`). Trading,
                // liquidation, and resolution fees are NOT shared with the
                // cranker — sweep_delta is captured BEFORE keeper_crank_not_atomic
                // so those fees don't inflate the reward.
                //
                // Reward is paid AFTER keeper_crank_not_atomic. Paying it
                // before would let a borderline caller self-rescue: the capital
                // bump would change the account's maintenance-margin health
                // before the crank's liquidation pass evaluated it. Paying it
                // after leaves the crank's risk decisions on pre-reward state,
                // and a caller who got liquidated inside the crank (slot no
                // longer used) simply doesn't collect the reward.
                // Fully accrue market to clock.slot BEFORE sweeping fees.
                // Explicit ordering so sweep_maintenance_fees + keeper_crank
                // run on a fully-accrued market. keeper_crank_not_atomic's
                // internal accrue then no-ops on dt=0+same-price.
                ensure_market_accrued_to_now(engine, clock.slot, price, funding_rate_e9_pre)?;

                // Shared per-instruction fee-sync budget (audit #2).
                // Candidate syncs run FIRST so the engine's crank sees
                // post-fee equity for health checks. The sweep then
                // runs with the REMAINING budget, so the worst-case
                // total syncs per instruction is capped at
                // FEE_SWEEP_BUDGET (not FEE_SWEEP_BUDGET +
                // LIQ_BUDGET_PER_CRANK as it was previously). This
                // keeps the crank's CU usage bounded by the stated
                // envelope even when a full candidate list is paired
                // with a full sweep.
                //
                // Ordering: candidates first, then sweep with
                // remaining budget, then crank. Sweep-delta (keeper
                // reward) is computed from balance BEFORE the sweep
                // only, so candidate syncs don't inflate the reward.
                //
                // Budget accounting MUST MIRROR keeper_crank_not_atomic's:
                // invalid / out-of-range / unused entries are SKIPPED
                // without consuming the attempts budget (engine §10.6).
                // If we instead capped by array position (min(cap)), an
                // attacker could pad the front of `combined` with 64
                // invalid indices so the real target at position 65 is
                // never fee-synced — but the engine's crank would still
                // skip the invalid entries and reach the target, running
                // its health check on stale fee debt. The loop below
                // counts `attempts` exactly like the engine: valid
                // existing candidates consume budget; others don't.
                //
                // Dedup is also mirrored: the engine treats duplicates as
                // separate attempts (it re-processes the same idx twice
                // on consecutive entries, which is idempotent at the same
                // anchor). The wrapper skips duplicate SYNC calls purely
                // to save CU — dedup is within the loop, not the budget.
                let mut candidate_syncs = 0usize;
                if config.maintenance_fee_per_slot > 0 {
                    // Candidate syncs share the FEE_SWEEP_BUDGET with
                    // the maintenance sweep below. Hard-cap at
                    // min(LIQ_BUDGET_PER_CRANK, FEE_SWEEP_BUDGET) so
                    // that (a) the engine's per-crank liquidation
                    // attempts cap is respected, and (b) candidate
                    // syncs ALONE can never exceed the total fee-sync
                    // budget — no matter how the two engine constants
                    // relate, the total syncs per instruction is
                    // bounded by FEE_SWEEP_BUDGET.
                    let cap = core::cmp::min(
                        percolator::LIQ_BUDGET_PER_CRANK as usize,
                        crate::constants::FEE_SWEEP_BUDGET,
                    );
                    let mut synced: [u16; percolator::LIQ_BUDGET_PER_CRANK as usize]
                        = [u16::MAX; percolator::LIQ_BUDGET_PER_CRANK as usize];
                    let mut synced_count = 0usize;
                    let mut attempts = 0usize;
                    for &(idx, _policy) in combined.iter() {
                        if attempts >= cap { break; }
                        // Defense-in-depth: also bail if we're already
                        // at the shared budget. The attempts cap above
                        // subsumes this under today's constants, but
                        // keeps the bound mechanical if either
                        // constant changes.
                        if candidate_syncs >= crate::constants::FEE_SWEEP_BUDGET {
                            break;
                        }
                        let i = idx as usize;
                        if i >= percolator::MAX_ACCOUNTS { continue; }
                        if !engine.is_used(i) { continue; }
                        attempts += 1;
                        let mut already = false;
                        for j in 0..synced_count {
                            if synced[j] == idx { already = true; break; }
                        }
                        if already { continue; }
                        engine
                            .sync_account_fee_to_slot_not_atomic(
                                idx,
                                clock.slot,
                                config.maintenance_fee_per_slot,
                            )
                            .map_err(map_risk_error)?;
                        synced[synced_count] = idx;
                        synced_count += 1;
                        candidate_syncs += 1;
                    }
                }

                // Sweep with the REMAINING fee-sync budget. Keeper
                // reward (sweep_delta) is computed from balance
                // BEFORE the sweep, so candidate-sync deltas are
                // excluded from the reward.
                let ins_before = engine.insurance_fund.balance.get();
                let remaining_budget = crate::constants::FEE_SWEEP_BUDGET
                    .saturating_sub(candidate_syncs);
                sweep_maintenance_fees(engine, &mut config, clock.slot, remaining_budget)?;
                let sweep_delta = engine
                    .insurance_fund
                    .balance
                    .get()
                    .saturating_sub(ins_before);

                let admit_h_min = engine.params.h_min;
                let admit_h_max = engine.params.h_max;
                let outcome = engine
                    .keeper_crank_not_atomic(
                        clock.slot,
                        price,
                        &combined,
                        percolator::LIQ_BUDGET_PER_CRANK,
                        funding_rate_e9_pre,
                        admit_h_min,
                        admit_h_max,
                    )
                    .map_err(map_risk_error)?;
                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: keeper_crank_end");
                    sol_log_compute_units();
                }

                // Pay the crank reward AFTER keeper_crank_not_atomic has run
                // all liquidation / lifecycle logic. The sweep_delta was
                // captured pre-crank, so the reward is bounded only by the
                // maintenance-fee collection on this call, not by incidental
                // insurance growth from liquidation fees.
                //
                // Skip conditions:
                //   - permissionless caller (no account to credit)
                //   - fee feature disabled (nothing swept)
                //   - zero sweep delta (first crank of a fresh market)
                //   - caller_idx was liquidated during the crank and is no
                //     longer a used slot (cannot pay capital to a freed slot)
                if !permissionless
                    && config.maintenance_fee_per_slot > 0
                    && sweep_delta > 0
                    && engine.is_used(caller_idx as usize)
                {
                    // 50 / 50 split: half to caller, half stays in insurance.
                    let mut reward = sweep_delta
                        .saturating_mul(crate::constants::CRANK_REWARD_BPS)
                        / 10_000u128;
                    // Cap reward by post-crank EXCESS OVER THE INSURANCE
                    // FLOOR, not raw balance. Matches the engine's own
                    // insurance-loss path semantics (available =
                    // balance - insurance_floor). Without this, a crank
                    // where the sweep briefly raised insurance above the
                    // floor but a liquidation drained the excess back to
                    // (or below) the floor could still pay the reward
                    // out of floor-protected reserves.
                    let ins_now = engine.insurance_fund.balance.get();
                    let floor = engine.params.insurance_floor.get();
                    let available_reward = ins_now.saturating_sub(floor);
                    if reward > available_reward {
                        reward = available_reward;
                    }
                    if reward > 0 {
                        // Conservation: insurance − r, caller.capital + r,
                        // c_tot + r keeps vault ≥ c_tot + insurance + net_pnl
                        // intact. Use checked_add on the two growing fields
                        // (silent saturation would mask an invariant break).
                        // reward ≤ ins_now ≤ MAX_VAULT_TVL, so both additions
                        // are economically bounded, but we still fail loudly
                        // on any unexpected overflow rather than silently
                        // clipping at u128::MAX.
                        let ci = caller_idx as usize;
                        let cap_prev = engine.accounts[ci].capital.get();
                        let cap_next = cap_prev
                            .checked_add(reward)
                            .ok_or(PercolatorError::EngineOverflow)?;
                        let c_tot_prev = engine.c_tot.get();
                        let c_tot_next = c_tot_prev
                            .checked_add(reward)
                            .ok_or(PercolatorError::EngineOverflow)?;
                        engine.insurance_fund.balance = U128::new(ins_now - reward);
                        engine.accounts[ci].capital = U128::new(cap_next);
                        engine.c_tot = U128::new(c_tot_next);
                    }

                    // Belt-and-suspenders: the reward may never cause a
                    // floor breach. A market can legitimately sit below
                    // floor (e.g. after an earlier insurance-loss event),
                    // so we can't assert `balance >= floor` universally.
                    // Instead, enforce the MINIMUM-MONOTONIC property:
                    //   post_balance >= min(ins_now, floor)
                    // i.e., reward never moved us closer to zero than the
                    // floor (when we were above floor) and never moved us
                    // at all (when we were already at/below floor — cap
                    // zeroed reward). Violation = cap math regression.
                    let post_balance = engine.insurance_fund.balance.get();
                    let lower_bound = core::cmp::min(ins_now, floor);
                    debug_assert!(post_balance >= lower_bound);
                    if post_balance < lower_bound {
                        return Err(PercolatorError::EngineCorruptState.into());
                    }
                }

                // Copy stats and drop engine mutable borrow.
                // Use the actual crank outcome so observability/telemetry
                // reflects real liquidations, not a hard-coded zero.
                let liqs = outcome.num_liquidations as u64;
                let ins_low = engine.insurance_fund.balance.get() as u64;
                drop(engine);

                // Engine has now processed a real oracle price via accrue_market_to.
                // engine.last_oracle_price is no longer the init sentinel.
                if !state::is_oracle_initialized(&data) {
                    state::set_oracle_initialized(&mut data);
                }

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

                    // Phase C: progressive discovery scan.
                    // Wrap on the MARKET's configured capacity (params.max_accounts),
                    // not the compile-time MAX_ACCOUNTS. Otherwise a small market
                    // (e.g. max_accounts=64) wastes cranks walking slots 64..4095
                    // that by construction can never be in use — the risk buffer
                    // would take thousands of cranks to rediscover a newly-risky
                    // low-indexed account after the cursor passed it.
                    let scan_mod = engine.params.max_accounts as usize;
                    let scan_mod = if scan_mod == 0 || scan_mod > percolator::MAX_ACCOUNTS {
                        percolator::MAX_ACCOUNTS
                    } else {
                        scan_mod
                    };
                    let scan_start = (buf.scan_cursor as usize) % scan_mod;
                    for offset in 0..crate::constants::RISK_SCAN_WINDOW {
                        let idx = (scan_start + offset) % scan_mod;
                        if !engine.is_used(idx) { continue; }
                        let eff = engine.effective_pos_q(idx);
                        if eff == 0 { continue; }
                        let notional = percolator::wide_math::mul_div_floor_u128(
                            eff.unsigned_abs(), price as u128, percolator::POS_SCALE,
                        );
                        buf.upsert(idx as u16, notional);
                    }
                    buf.scan_cursor = ((scan_start + crate::constants::RISK_SCAN_WINDOW)
                        % scan_mod) as u16;

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

                // Capture pre-read funding rate for anti-retroactivity (§5.5)
                let funding_rate_e9 = compute_current_funding_rate_e9(&config);

                // Read oracle price with circuit-breaker clamping
                let price =
                    read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot, &mut data)?;
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

                // Fully accrue market to clock.slot BEFORE execute_trade_with
                // _matcher, which internally syncs both counterparties' fees
                // before the trade. Explicit accrue→sync→trade ordering.
                ensure_market_accrued_to_now(engine, clock.slot, price, funding_rate_e9)?;

                // Pre-sync maintenance fees for both counterparties BEFORE
                // capturing ins_before for the mark-EWMA fee-weight
                // snapshot. Without this, the `delta = ins_after -
                // ins_before` used as `fee_paid` would include
                // maintenance fees collected by execute_trade_with
                // _matcher's own internal sync — inflating EWMA weight
                // on a small trade after large maintenance accrual, a
                // mark-manipulation vector. The internal sync at the
                // same anchor becomes a no-op on `last_fee_slot`.
                if config.maintenance_fee_per_slot > 0 {
                    engine.sync_account_fee_to_slot_not_atomic(
                        user_idx, clock.slot, config.maintenance_fee_per_slot,
                    ).map_err(map_risk_error)?;
                    engine.sync_account_fee_to_slot_not_atomic(
                        lp_idx, clock.slot, config.maintenance_fee_per_slot,
                    ).map_err(map_risk_error)?;
                }

                // Snapshot insurance fund balance for fee-weighted EWMA.
                // The delta after execute_trade = trading_fees -
                // losses_absorbed (maintenance fees already synced above).
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
                // Pass maintenance_fee_per_slot = 0 so the helper's
                // internal sync is a no-op — we already pre-synced both
                // sides just above. Halves fee-sync CU on the hot path.
                execute_trade_with_matcher(
                    engine, &NoOpMatcher, lp_idx, user_idx, clock.slot, price, size,
                    funding_rate_e9, 0, // NoOpMatcher ignores lp_account_id
                    0,
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
                    // N4 fix: seed EWMA at oracle price on first trade (not exec price).
                    // Prevents attacker from imprinting a biased mark on the first fill.
                    let ewma_price = if old_ewma == 0 && config.last_effective_price_e6 > 0 {
                        config.last_effective_price_e6
                    } else {
                        clamped_price
                    };
                    config.mark_ewma_e6 = crate::verify::ewma_update(
                        old_ewma, ewma_price,
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
                if !state::is_oracle_initialized(&data) {
                    state::set_oracle_initialized(&mut data);
                }

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
                // Account layout:
                //   [0]  user (signer)
                //   [1]  lp_owner (non-signer; matcher delegates auth)
                //   [2]  slab (writable)
                //   [3]  clock sysvar
                //   [4]  oracle
                //   [5]  matcher_program
                //   [6]  matcher_context (writable)
                //   [7]  lp_pda (PDA: ["lp", slab, lp_idx])
                //   [8..] VARIADIC TAIL forwarded to matcher CPI verbatim
                //
                // The variadic tail is the one deliberate exception to
                // the wrapper's exact-account-count ABI. Callers can
                // append any number of extra accounts (other programs,
                // on-chain state the matcher wants to inspect, etc.)
                // after the fixed 8. The wrapper does NOT interpret or
                // validate the tail — it is the MATCHER'S
                // responsibility to check keys, ownership, and signer
                // flags on anything it uses. This is what makes it safe:
                //   1. The matcher is the party the LP has authorized,
                //      so extending the matcher's account set is an
                //      LP-scoped authorization.
                //   2. The wrapper's own state (slab, oracle, vault,
                //      pyth, etc.) is never in the tail — those are
                //      always at the fixed indices above. A tail
                //      account whose key collides with one of the fixed
                //      slots does not get reinterpreted by the wrapper.
                //   3. The tail accounts are passed as-is to the CPI
                //      (preserving signer/writable flags from the outer
                //      transaction), so the matcher cannot use them to
                //      gain privileges the caller didn't already grant.
                // Typical uses: pyth/chainlink feeds for matcher-side
                // pricing, on-chain whitelist PDAs, cross-program
                // inventory state, etc.
                accounts::expect_len_min(accounts, 8)?;
                let a_user = &accounts[0];
                let a_lp_owner = &accounts[1];
                let a_slab = &accounts[2];
                let a_clock = &accounts[3];
                let a_oracle = &accounts[4];
                let a_matcher_prog = &accounts[5];
                let a_matcher_ctx = &accounts[6];
                let a_lp_pda = &accounts[7];
                let a_tail = &accounts[8..];

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
                    // Reject if nonce would overflow — wrapping reopens old request IDs.
                    let nonce = state::read_req_nonce(&*data);
                    let req_id = crate::verify::nonce_on_success(nonce)
                        .ok_or(PercolatorError::EngineOverflow)?;

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
                    // Per-materialization instance ID from generation table.
                    // Assigned at InitLP, immutable for the lifetime of this LP instance.
                    // Different for every materialization even at the same slot.
                    let lp_instance_id = state::read_account_generation(&*data, lp_idx);
                    // Reject generation 0 — slot was never materialized via InitLP
                    if lp_instance_id == 0 {
                        return Err(PercolatorError::EngineAccountNotFound.into());
                    }
                    (
                        lp_instance_id,
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
                // Capture pre-read funding rate for anti-retroactivity (§5.5)
                let funding_rate_e9_pre = compute_current_funding_rate_e9(&config);

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
                    let mut slab_data = state::slab_data_mut(a_slab)?;
                    let price = read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot, &mut slab_data)?;
                    drop(slab_data);
                    price
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

                // Build CPI accounts: [lp_pda (signer), matcher_ctx
                // (writable), ...tail]. Tail metas mirror the outer
                // transaction's signer/writable flags so the matcher
                // sees exactly the same privileges the caller sent.
                // The matcher is responsible for validating keys,
                // owners, and data on every tail account it uses; the
                // wrapper does NO interpretation here.
                let mut metas: alloc::vec::Vec<AccountMeta> = alloc::vec::Vec::with_capacity(2 + a_tail.len());
                metas.push(AccountMeta::new_readonly(*a_lp_pda.key, true));
                metas.push(AccountMeta::new(*a_matcher_ctx.key, false));
                for tail_ai in a_tail.iter() {
                    metas.push(AccountMeta {
                        pubkey: *tail_ai.key,
                        is_signer: tail_ai.is_signer,
                        is_writable: tail_ai.is_writable,
                    });
                }

                let ix = SolInstruction {
                    program_id: *a_matcher_prog.key,
                    accounts: metas,
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
                zc::invoke_signed_trade(&ix, a_lp_pda, a_matcher_ctx, a_matcher_prog, a_tail, seeds)?;

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
                // with FLAG_PARTIAL_OK. The engine's trade path is skipped
                // (size_q == 0 would be rejected), but we DO advance the
                // engine's market clock via accrue_market_to so the wrapper's
                // newly-advanced index is not retroactively applied.
                //
                // Why accrue on a zero-fill: we cannot safely revert the index
                // (last_effective_price_e6, last_hyperp_index_slot) because
                // doing so re-opens the dt-accumulation attack where repeated
                // zero-fills roll back the index clock, then a real trade
                // snaps the index with a huge accumulated dt. So the index
                // legitimately advances. But advancing the index without
                // advancing engine time means the next trade/crank computes
                // funding against the new index and applies it retroactively
                // over [old engine.last_market_slot, now] — breaking the
                // anti-retroactivity rule enforced elsewhere.
                //
                // Fix: call engine.accrue_market_to(now_slot, price,
                //   funding_rate_e9_pre). This uses the PRE-oracle-read rate
                // (computed against the old index/mark) so funding accrued
                // over [engine.last_market_slot, clock.slot] reflects the
                // rate that was actually in effect. The engine then advances
                // to clock.slot with last_oracle_price = price, matching the
                // config's advanced index state.
                //
                // mark_ewma_e6 is NOT updated on zero-fill (the EWMA update
                // below is inside the exec_size != 0 branch). That's correct:
                // no trade executed, so there's no exec price to feed the EWMA.
                if ret.exec_size == 0 {
                    let mut data = state::slab_data_mut(a_slab)?;
                    let engine = zc::engine_mut(&mut data)?;
                    // Pre-chunk catch-up so the single accrue_market_to below
                    // sees dt ≤ max_accrual_dt_slots (Finding 3). Use the
                    // pre-read funding rate for catchup chunks (Finding 2).
                    catchup_accrue(engine, clock.slot, price, funding_rate_e9_pre)?;
                    engine
                        .accrue_market_to(clock.slot, price, funding_rate_e9_pre)
                        .map_err(map_risk_error)?;
                    // Restore pre-oracle config, but preserve oracle/index state
                    // that legitimately advanced during the instruction:
                    // - last_good_oracle_slot: liveness proof from successful read
                    // - last_effective_price_e6: index legitimately moved toward mark
                    // - last_hyperp_index_slot: prevents dt-accumulation attack
                    // - first_observed_stale_slot: cleared by successful read,
                    //   must stay cleared so permissionless-resolve sees the
                    //   live-oracle observation
                    let mut restored = config_pre_oracle;
                    restored.last_good_oracle_slot = config.last_good_oracle_slot;
                    restored.last_effective_price_e6 = config.last_effective_price_e6;
                    restored.last_hyperp_index_slot = config.last_hyperp_index_slot;
                    restored.first_observed_stale_slot = config.first_observed_stale_slot;
                    state::write_config(&mut data, &restored);
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

                    // Fully accrue market to clock.slot BEFORE execute_trade
                    // _with_matcher, which internally syncs both sides' fees
                    // before the trade. Explicit accrue→sync→trade ordering.
                    ensure_market_accrued_to_now(engine, clock.slot, price, funding_rate_e9_pre)?;

                    // Pre-sync fees BEFORE the ins_before snapshot to
                    // prevent maintenance-fee-inflated EWMA weight on
                    // small trades (see TradeNoCpi for rationale).
                    if config.maintenance_fee_per_slot > 0 {
                        engine.sync_account_fee_to_slot_not_atomic(
                            user_idx, clock.slot, config.maintenance_fee_per_slot,
                        ).map_err(map_risk_error)?;
                        engine.sync_account_fee_to_slot_not_atomic(
                            lp_idx, clock.slot, config.maintenance_fee_per_slot,
                        ).map_err(map_risk_error)?;
                    }

                    let trade_size = crate::verify::cpi_trade_size(ret.exec_size, size);

                    // Snapshot insurance for fee-weighted EWMA (delta approach).
                    // delta now captures ONLY trading_fees - losses_absorbed
                    // (maintenance fees already synced above).
                    // NOTE: Conservative undercount during volatile
                    // loss-absorption events (see TradeNoCpi comment).
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
                    // Use pre-oracle-read funding rate (anti-retroactivity §5.5).
                    // Pass maintenance_fee_per_slot = 0 so the helper's
                    // internal sync is a no-op — we already pre-synced
                    // both sides just above (pre-ins_before-snapshot).
                    // Halves fee-sync CU on the hot path.
                    execute_trade_with_matcher(
                        engine, &matcher, lp_idx, user_idx, clock.slot, price, trade_size,
                        funding_rate_e9_pre, lp_account_id,
                        0,
                    ).map_err(map_risk_error)?;
                    #[cfg(feature = "cu-audit")]
                    {
                        msg!("CU_CHECKPOINT: trade_cpi_execute_end");
                        sol_log_compute_units();
                    }
                    // Capture pre-trade EWMA so both the EWMA-clock refresh
                    // (inside the cap-active branch) and the Hyperp
                    // liveness refresh (after the block) can check
                    // whether the mark actually moved.
                    let old_ewma_cpi = config.mark_ewma_e6;
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
                        // N4 fix: seed at oracle on first trade
                        let ewma_price_cpi = if old_ewma_cpi == 0 && config.last_effective_price_e6 > 0 {
                            config.last_effective_price_e6
                        } else {
                            clamped_exec
                        };
                        config.mark_ewma_e6 = crate::verify::ewma_update(
                            old_ewma_cpi,
                            ewma_price_cpi,
                            config.mark_ewma_halflife_slots,
                            config.mark_ewma_last_slot,
                            clock.slot,
                            fee_paid_cpi,
                            config.mark_min_fee,
                        );
                        // EWMA math clock refreshes when:
                        //   (a) the EWMA VALUE changed (any partial-
                        //       fee sub-threshold trade that moves
                        //       mark_ewma_e6 must advance the clock,
                        //       otherwise the next ewma_update sees
                        //       a stale dt and over-weights), OR
                        //   (b) the trade was a full-weight
                        //       observation (same-price trades don't
                        //       move the value but do reset the dt
                        //       anchor to "we just saw this price").
                        // This is distinct from Hyperp liveness
                        // (below), which only counts full-weight
                        // observations.
                        let full_weight_observation = config.mark_min_fee == 0
                            || fee_paid_cpi >= config.mark_min_fee;
                        let ewma_moved = config.mark_ewma_e6 != old_ewma_cpi;
                        if ewma_moved || full_weight_observation {
                            config.mark_ewma_last_slot = clock.slot;
                        }
                        // NOTE: do NOT stamp funding rate here — execute_trade_not_atomic
                        // handles it via the funding_rate parameter (§5.5 anti-retroactivity).
                    }

                    // Hyperp: also update authority_price (legacy mark field).
                    // Hyperp-liveness clock (last_mark_push_slot) refreshes
                    // ONLY on full-weight observations — sub-threshold
                    // dust-wash trades must not keep a dead market
                    // artificially alive. This is the ONLY Hyperp
                    // liveness signal: permissionless_stale_matured for
                    // Hyperp uses last_mark_push_slot, not
                    // max(mark_ewma_last_slot, last_mark_push_slot),
                    // so partial-fee EWMA-math clock advances don't
                    // accidentally extend market life.
                    if is_hyperp {
                        config.authority_price_e6 = oracle::clamp_oracle_price(
                            config.last_effective_price_e6,
                            ret.exec_price_e6,
                            config.oracle_price_cap_e2bps,
                        );
                        // Full-weight observation check: recompute here
                        // because the earlier `full_weight_observation`
                        // binding is in the cap>0 branch's scope.
                        let fee_paid_hyperp = if config.mark_min_fee > 0 {
                            let ins_after_cpi = engine.insurance_fund.balance.get();
                            let delta = ins_after_cpi.saturating_sub(ins_before_cpi);
                            core::cmp::min(delta, u64::MAX as u128) as u64
                        } else { 0u64 };
                        let full_weight = config.mark_min_fee == 0
                            || fee_paid_hyperp >= config.mark_min_fee;
                        if full_weight {
                            config.last_mark_push_slot = clock.slot as u128;
                        }
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
                // Write nonce + config + risk buffer + oracle flag.
                {
                    let mut data = state::slab_data_mut(a_slab)?;
                    state::write_req_nonce(&mut data, req_id);
                    state::write_config(&mut data, &config);
                    if !state::is_oracle_initialized(&data) {
                        state::set_oracle_initialized(&mut data);
                    }
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
                // Anti-retroactivity: capture funding rate before oracle read (§5.5)
                let funding_rate_e9 = compute_current_funding_rate_e9(&config);
                let price = if is_hyperp {
                    let eng = zc::engine_ref(&data)?;
                    let last_slot = eng.current_slot;
                    oracle::get_engine_oracle_price_e6(
                        last_slot, clock.slot, clock.unix_timestamp,
                        &mut config, a_oracle,
                    )?
                } else {
                    read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot, &mut data)?
                };
                state::write_config(&mut data, &config);

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, target_idx)?;

                sol_log_64(target_idx as u64, price, 0, 0, 0);
                {
                    let acc = &engine.accounts[target_idx as usize];
                    sol_log_64(acc.capital.get() as u64, 0, 0, 0, 1);
                    let eff = engine.effective_pos_q(target_idx as usize);
                    let notional = engine.notional(target_idx as usize, price);
                    sol_log_64(notional as u64, (eff == 0) as u64, 0, 0, 2);
                }

                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: liquidate_start");
                    sol_log_compute_units();
                }
                let admit_h_min = engine.params.h_min;
                let admit_h_max = engine.params.h_max;
                // Fully accrue market to clock.slot BEFORE fee sync +
                // liquidate_at_oracle_not_atomic. Explicit accrue→sync→op.
                ensure_market_accrued_to_now(engine, clock.slot, price, funding_rate_e9)?;
                // Realize due maintenance fees on the target BEFORE liquidation
                // so the maintenance-margin check sees post-fee equity.
                sync_account_fee(engine, &config, target_idx, clock.slot)?;
                let _res = engine
                    .liquidate_at_oracle_not_atomic(target_idx, clock.slot, price,
                        percolator::LiquidationPolicy::FullClose,
                        funding_rate_e9,
                        admit_h_min,
                        admit_h_max)
                    .map_err(map_risk_error)?;
                sol_log_64(_res as u64, 0, 0, 0, 4); // result

                // Collect post-liquidation position for risk buffer
                let liq_eff = engine.effective_pos_q(target_idx as usize);
                drop(engine);
                if !state::is_oracle_initialized(&data) {
                    state::set_oracle_initialized(&mut data);
                }

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
                // Anti-retroactivity: capture funding rate before oracle read (§5.5)
                let funding_rate_e9 = compute_current_funding_rate_e9(&config);
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
                        read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot, &mut data)?
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
                    // Realize recurring maintenance fees to the resolved
                    // anchor BEFORE force_close_resolved. Engine's
                    // force_close_resolved_not_atomic does NOT itself sync
                    // the fee cursor; without this call an account could
                    // reach terminal close with unpaid fees accrued over
                    // [last_fee_slot, resolved_slot]. On Resolved mode
                    // sync_account_fee_to_slot_not_atomic anchors at
                    // resolved_slot automatically, but we pass resolved
                    // _slot explicitly for consistency with the other
                    // resolved paths (AdminForceCloseAccount,
                    // ForceCloseResolved). No-op when
                    // maintenance_fee_per_slot == 0.
                    let (_settle_px, resolved_slot_anchor) = engine.resolved_context();
                    sync_account_fee(engine, &config, user_idx, resolved_slot_anchor)?;
                    // force_close_resolved handles K-pair PnL,
                    // loss settlement, and account close internally. Engine
                    // v12.18.6+: signature is (idx,) — the engine pulls the
                    // resolved_slot from its own state (§9.9).
                    match engine.force_close_resolved_not_atomic(user_idx)
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
                    let admit_h_min = engine.params.h_min;
                    let admit_h_max = engine.params.h_max;
                    // Fully accrue market to clock.slot BEFORE fee sync +
                    // close_account_not_atomic. Explicit accrue→sync→op.
                    ensure_market_accrued_to_now(engine, clock.slot, price, funding_rate_e9)?;
                    // Realize due maintenance fees BEFORE close so the account
                    // cannot escape the unpaid interval by closing between cranks.
                    sync_account_fee(engine, &config, user_idx, clock.slot)?;
                    engine
                        .close_account_not_atomic(user_idx, clock.slot, price,
                            funding_rate_e9,
                            admit_h_min,
                            admit_h_max)
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
                // Live close processes a real oracle price through the engine
                if !resolved && !state::is_oracle_initialized(&data) {
                    state::set_oracle_initialized(&mut data);
                }
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

                let clock = Clock::from_account_info(a_clock)?;
                // Hard-timeout gate: reject before moving tokens into a
                // terminally-stale market.
                if oracle::permissionless_stale_matured(&config, clock.slot) {
                    return Err(PercolatorError::OracleStale.into());
                }

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
                // No-oracle path. bounded_now:
                //   - upper-bounded by last_market_slot so we don't
                //     advance current_slot past the accrue envelope,
                //   - floored at current_slot so the engine's
                //     monotonicity guard (`now_slot >= current_slot`)
                //     still holds after a prior no-oracle op (InitUser,
                //     DepositCollateral, ReclaimEmptyAccount) has
                //     advanced current_slot past last_market_slot.
                // Full accrue for the tail `(last_market_slot,
                // clock.slot]` happens on the next oracle-backed op.
                let bounded_now = core::cmp::max(
                    core::cmp::min(clock.slot, engine.last_market_slot),
                    engine.current_slot,
                );
                engine
                    .top_up_insurance_fund(units as u128, bounded_now)
                    .map_err(map_risk_error)?;
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

                    // CloseSlab is gated by the scoped close_authority,
                    // not general admin. Operators can burn admin while
                    // retaining close_authority for rent cleanup, or
                    // burn close_authority to trap rent forever (the
                    // traders-are-rug-proof configuration).
                    let header = state::read_header(&data);
                    require_admin(header.close_authority, a_dest.key)?;
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
                funding_max_e9_per_slot,
            } => {
                // Accounts: (admin, slab, clock, oracle).
                // For non-Hyperp markets the oracle is REQUIRED. Allowing the
                // caller to omit the oracle account used to be a degenerate
                // escape hatch: with no oracle, the accrual rate was forced to
                // zero regardless of the actual live mark-vs-index premium,
                // which meant admin could retroactively erase elapsed funding
                // just by leaving the oracle off the instruction. Following
                // the ResolveMarket pattern, the degenerate (rate=0) branch is
                // reserved for the *engine-confirmed-dead* oracle case only
                // (read_price_and_stamp returns OracleStale); admin may not
                // choose degenerate by omission.
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
                if zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }
                let header = state::read_header(&data);
                require_admin(header.admin, a_admin.key)?;

                // Validate parameters
                if funding_horizon_slots == 0 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                // Reject negative funding bounds — reversed clamp bounds panic.
                // Also gate against the engine's stored per-market envelope
                // (params.max_abs_funding_e9_per_slot), NOT the crate-global
                // MAX_ABS_FUNDING_E9_PER_SLOT, so UpdateConfig cannot accept a cap
                // that the engine would later reject at accrue time.
                // Compare in i128 space: `as u64` would wrap modulo 2^64 for
                // huge positive inputs (e.g., i64::MAX * 1e5 ≈ 9e23) and
                // silently pass the envelope.
                if funding_max_premium_bps < 0 || funding_max_e9_per_slot < 0 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                let engine_envelope = zc::engine_ref(&data)?.params.max_abs_funding_e9_per_slot;
                if (funding_max_e9_per_slot as i128) > engine_envelope as i128 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }

                // Read existing config
                let mut config = state::read_config(&data);

                if funding_k_bps > 100_000 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }

                // Anti-retroactivity: capture funding rate before any config mutation (§5.5)
                let funding_rate_e9 = compute_current_funding_rate_e9(&config);

                let clock = Clock::from_account_info(a_clock)?;
                // Hard-timeout gate: UpdateConfig must not mutate a
                // terminally-stale market. Admin has no "emergency
                // reconfigure" path past the hard timeout — the market
                // is dead, users exit via ResolvePermissionless.
                if oracle::permissionless_stale_matured(&config, clock.slot) {
                    return Err(PercolatorError::OracleStale.into());
                }
                // Flush Hyperp index WITHOUT external staleness check
                // (admin recovery path; the hard-timeout gate above
                // handles the terminal case).
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
                // Accrue to boundary. The ordinary/degenerate split mirrors
                // Resolve{Market,Permissionless}:
                //   ORDINARY  — fresh non-Hyperp oracle reading (or Hyperp flushed index):
                //       accrual_price = fresh reading (or index), rate = captured.
                //   DEGENERATE — non-Hyperp oracle *engine-confirmed dead*:
                //       accrual_price = P_last = engine.last_oracle_price, rate = 0.
                // The degenerate arm is entered only when read_price_and_stamp
                // returns OracleStale, i.e. the passed oracle is genuinely dead.
                // Admin cannot select the degenerate arm by omitting the oracle
                // account — the caller-supplied oracle is a required input now
                // (enforced at expect_len(4) above).
                {
                    let (accrual_price, rate_for_accrual): (u64, i128) =
                        if oracle::is_hyperp_mode(&config) {
                            (config.last_effective_price_e6, funding_rate_e9)
                        } else {
                            match read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot, &mut data) {
                                Ok(price) => {
                                    state::write_config(&mut data, &config);
                                    (price, funding_rate_e9)
                                }
                                Err(e) => {
                                    // Only OracleStale / OracleConfTooWide
                                    // prove the feed is unusable; other
                                    // errors (wrong account, bad data,
                                    // wrong feed) propagate as-is.
                                    // OracleConfTooWide added to match
                                    // ResolvePermissionless (Finding 7 of
                                    // an earlier round) — without it,
                                    // non-Hyperp markets with a live-but-
                                    // wide feed would be unable to update
                                    // config under the degenerate arm.
                                    let stale_err: ProgramError =
                                        PercolatorError::OracleStale.into();
                                    let conf_err: ProgramError =
                                        PercolatorError::OracleConfTooWide.into();
                                    if e != stale_err && e != conf_err {
                                        return Err(e);
                                    }
                                    // Oracle is confirmed unusable → degenerate arm.
                                    let engine = zc::engine_ref(&data)?;
                                    (engine.last_oracle_price, 0i128)
                                }
                            }
                        };
                    if accrual_price > 0 {
                        {
                            let engine = zc::engine_mut(&mut data)?;
                            // Pre-chunk catch-up so accrue_market_to below sees
                            // dt ≤ max_accrual_dt_slots (Finding 4). Use the
                            // same (price, rate) the final accrue uses so the
                            // catchup chunks are consistent with the boundary.
                            catchup_accrue(
                                engine, clock.slot, accrual_price, rate_for_accrual,
                            )?;
                            engine.accrue_market_to(clock.slot, accrual_price,
                                rate_for_accrual)
                                .map_err(map_risk_error)?;
                        }
                        // Engine processed real price — last_oracle_price is no longer sentinel
                        if !state::is_oracle_initialized(&data) {
                            state::set_oracle_initialized(&mut data);
                        }
                    }
                }

                config.funding_horizon_slots = funding_horizon_slots;
                config.funding_k_bps = funding_k_bps;
                config.funding_max_premium_bps = funding_max_premium_bps;
                config.funding_max_e9_per_slot = funding_max_e9_per_slot;
                // Engine v12.18.1: accrue_market_to only updates market-global state
                // (K/F/slot_last). No per-account touches means no resets to
                // schedule or finalize, so the end-of-instruction lifecycle — which
                // the engine no longer exposes publicly — is structurally a no-op
                // on this path.
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
                // Anti-retroactivity: capture funding rate before any config mutation (§5.5)
                let funding_rate_e9 = compute_current_funding_rate_e9(&config);
                // Hard-timeout gate (applies to both Hyperp and non-Hyperp):
                // once clock.slot - last_live_slot >=
                // permissionless_resolve_stale_slots, the market is
                // TERMINALLY dead. PushOraclePrice would otherwise
                // write mark_ewma_last_slot / last_mark_push_slot
                // (Hyperp) or authority_timestamp (non-Hyperp),
                // either of which can reset the timer and revive a
                // market that users are already entitled to exit
                // from. Reject before any mutation. Run it early,
                // before the Hyperp pre-push index flush, since even
                // that flush is a live-market price mutation.
                let push_clock = Clock::get()
                    .map_err(|_| ProgramError::UnsupportedSysvar)?;
                if oracle::permissionless_stale_matured(&config, push_clock.slot) {
                    return Err(PercolatorError::OracleStale.into());
                }
                // Hyperp: flush index WITHOUT external staleness check
                // (the hard-timeout gate above covers mark staleness).
                if is_hyperp {
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
                // Defense-in-depth: weaker-authority invariant checked
                // locally at the dangerous op. Even if a legacy slab or
                // migration produced a non-Hyperp state with authority
                // set but oracle_price_cap_e2bps == 0 (which
                // SetOracleAuthority / SetOraclePriceCap / UpdateAdmin
                // all reject), PushOraclePrice itself also refuses to
                // operate in that configuration — authority is ONLY
                // weaker than admin when its price effect is
                // cap-bounded.
                if !is_hyperp && config.oracle_price_cap_e2bps == 0 {
                    return Err(PercolatorError::InvalidConfigParam.into());
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
                //
                // Also reject timestamps that are already stale relative to
                // max_staleness_secs. Without this check, a push with an old
                // (monotonic, non-future) timestamp would be accepted, clear
                // first_observed_stale_slot below, and then fail
                // read_authority_price on the next priceable operation — the
                // exact freeze state the auditor flagged. Rejecting stale
                // pushes keeps the "successful push ⇒ market priceable"
                // invariant that the stale-stamp clear below relies on.
                if !is_hyperp {
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
                    // Freshness: reject pushes that read_authority_price would
                    // ignore as stale (age > max_staleness_secs). Equivalent
                    // to the read-side staleness predicate, so every accepted
                    // push is immediately usable by downstream price reads.
                    let push_age = push_clock.unix_timestamp
                        .saturating_sub(timestamp);
                    if push_age < 0
                        || (push_age as u64) > config.max_staleness_secs
                    {
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
                //
                // Hyperp stale-recovery policy (deliberate):
                //   If mark liveness has been lost beyond the catchup
                //   envelope while funding is active, the catchup_accrue
                //   call below can return CatchupRequired — the engine
                //   refuses to jump dt past max_accrual_dt_slots when
                //   funding is non-zero with OI on both sides, and the
                //   dedicated CatchupAccrue path cannot close the gap
                //   either because get_engine_oracle_price_e6 rejects
                //   the stored stale mark. Such a market is
                //   RESOLVE-ONLY: recovery is via ResolvePermissionless
                //   (the mark-staleness branch below, triggered by
                //   clock.slot - max(mark_ewma_last_slot,
                //   last_mark_push_slot) exceeding
                //   permissionless_resolve_stale_slots) or pre-burn
                //   admin ResolveMarket. Operators who want revivable
                //   Hyperp markets must ensure authority pushes happen
                //   frequently enough to stay within the catchup
                //   envelope while funding is active.
                if is_hyperp {
                    let engine = zc::engine_mut(&mut data)?;
                    // Pre-chunk catch-up so the accrue below sees
                    // dt ≤ max_accrual_dt_slots (Finding 4).
                    catchup_accrue(
                        engine, push_clock.slot, config.last_effective_price_e6,
                        funding_rate_e9,
                    )?;
                    engine.accrue_market_to(
                        push_clock.slot, config.last_effective_price_e6,
                        funding_rate_e9,
                    ).map_err(map_risk_error)?;
                }

                // Under the unified stale-oracle policy,
                // first_observed_stale_slot tracks continuous
                // unusability of the market's DEFINED oracle (external
                // Pyth/Chainlink or Hyperp internal mark), not the
                // optional authority fallback. An authority push does
                // not prove the defined oracle is live; only a
                // successful external read (non-Hyperp) or a non-stale
                // mark (Hyperp, tracked via last_mark_push_slot/
                // mark_ewma_last_slot) does. Therefore PushOraclePrice
                // DOES NOT clear first_observed_stale_slot — otherwise
                // a stale external feed + a live authority would block
                // permissionless resolve forever, re-introducing the
                // "pinned theater" state the unified policy fixes.
                //
                // Natural clearing still happens: any keeper/trade/
                // withdraw that submits a FRESH external oracle goes
                // through read_price_clamped_with_external's Ok branch,
                // which clears the stamp.

                let clamp_base = config.last_effective_price_e6;
                let clamped = oracle::clamp_oracle_price(
                    clamp_base,
                    normalized_price,
                    config.oracle_price_cap_e2bps,
                );
                config.authority_price_e6 = clamped;
                if is_hyperp {
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
                // Engine v12.18.1: accrue_market_to touches only market-global state.
                // End-of-instruction lifecycle is a no-op on this path and no longer
                // exposed publicly; next state-changing _not_atomic will finalize any
                // pending transitions.
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
                // Anti-retroactivity: capture funding rate before any config mutation (§5.5)
                let funding_rate_e9 = compute_current_funding_rate_e9(&config);

                // Hard-timeout gate: SetOraclePriceCap's Hyperp branch
                // flushes the index and calls accrue_market_to, both
                // of which mutate engine.last_oracle_price. After
                // maturity, admin must not be able to shift the
                // terminal settlement anchor — resolve through
                // Degenerate at the engine's already-accrued P_last
                // instead.
                let clock = Clock::from_account_info(a_clock)?;
                if oracle::permissionless_stale_matured(&config, clock.slot) {
                    return Err(PercolatorError::OracleStale.into());
                }

                // Flush Hyperp index (admin path; no external staleness
                // check — the hard-timeout gate above handles the
                // terminal case).
                if is_hyperp {
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
                    // Pre-chunk catch-up so the accrue below sees
                    // dt ≤ max_accrual_dt_slots (Finding 4).
                    catchup_accrue(
                        engine, clock.slot, config.last_effective_price_e6,
                        funding_rate_e9,
                    )?;
                    engine.accrue_market_to(
                        clock.slot, config.last_effective_price_e6,
                        funding_rate_e9,
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
                // Weaker-authority invariant (Model 1): once an oracle
                // authority is configured on a non-Hyperp market, the cap
                // may not be disabled. Disabling the cap would upgrade
                // authority from a bounded fallback to an unbounded price
                // setter — equivalent to admin — which breaks the
                // separation that lets admin burn leave authority alive.
                // Admins that want to remove the cap must first zero out
                // the authority via SetOracleAuthority(Pubkey::default()).
                if !is_hyperp
                    && max_change_e2bps == 0
                    && config.oracle_authority != [0u8; 32]
                {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                // Hard ceiling: cap above 100% makes the circuit breaker vacuous
                if max_change_e2bps > MAX_ORACLE_PRICE_CAP_E2BPS {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }

                config.oracle_price_cap_e2bps = max_change_e2bps;
                // Engine v12.18.1: accrue_market_to is market-global only; the
                // end-of-instruction lifecycle is not exposed and not needed here.
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
                let mut config = state::read_config(&data);
                // Anti-retroactivity: capture funding rate before any config mutation (§5.5)
                let funding_rate_e9 = compute_current_funding_rate_e9(&config);

                // Hard-timeout terminal path: once the market has been
                // oracle-stale for >= permissionless_resolve_stale_slots,
                // admin no longer settles at authority_price_e6 / mark
                // EWMA — the market resolves at engine.last_oracle_price
                // via the same Degenerate arm used by
                // ResolvePermissionless. This unifies the settlement
                // rule across both paths so admin can't revive the
                // market by pushing a fresh authority price after
                // maturity and then resolving at that pushed value.
                let clock_gate = Clock::from_account_info(a_clock)?;
                if oracle::permissionless_stale_matured(&config, clock_gate.slot) {
                    let engine = zc::engine_mut(&mut data)?;
                    let p_last = engine.last_oracle_price;
                    if p_last == 0 {
                        return Err(PercolatorError::OracleInvalid.into());
                    }
                    engine.resolve_market_not_atomic(
                        percolator::ResolveMode::Degenerate,
                        p_last,
                        p_last,
                        clock_gate.slot,
                        0,
                    ).map_err(map_risk_error)?;
                    config.authority_price_e6 = p_last;
                    config.first_observed_stale_slot = 0;
                    state::write_config(&mut data, &config);
                    return Ok(());
                }

                if config.authority_price_e6 == 0 {
                    return Err(ProgramError::InvalidAccountData);
                }
                // Non-Hyperp: require the settlement push to be fresh.
                // Prevents parking an old price in state and resolving later.
                if !oracle::is_hyperp_mode(&config) {
                    let push_age = clock_gate.unix_timestamp
                        .saturating_sub(config.authority_timestamp);
                    if push_age < 0 || push_age as u64 > config.max_staleness_secs {
                        return Err(PercolatorError::OracleStale.into());
                    }
                }
                // Read fresh external oracle for two purposes:
                // 1. Settlement circuit-breaker guard (when cap > 0)
                // 2. Pass as live_oracle_price to engine for self-synchronizing
                //    final accrual and deviation band check
                // Hyperp: admin IS the price source; live_oracle = index.
                // Non-Hyperp: always try to read fresh oracle; fall back to
                // engine.last_oracle_price only if oracle is genuinely dead.
                let mut fresh_live_oracle: Option<u64> = None;
                if !oracle::is_hyperp_mode(&config) {
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
                            fresh_live_oracle = Some(fresh_oracle);
                            // Update the circuit-breaker baseline from this fresh read
                            // so compute_current_funding_rate_e9 uses the freshest index.
                            config.last_effective_price_e6 = oracle::clamp_oracle_price(
                                config.last_effective_price_e6,
                                fresh_oracle,
                                config.oracle_price_cap_e2bps,
                            );
                            // NOTE on design: pass the RAW fresh oracle (not the
                            // clamped value) as the engine's live_oracle_price.
                            // The resolve deviation band
                            // (`resolve_price_deviation_bps`, spec §9.8 step 7)
                            // is intended to reject settlement when the
                            // admin-chosen price has drifted too far from the
                            // *actual* live market. Feeding the clamped value
                            // instead would let admin settle against a
                            // circuit-breaker-suppressed reference after a real
                            // oracle jump, locking in a stale price. The
                            // circuit breaker protects ongoing live operation;
                            // resolution is a one-shot terminal event where
                            // the raw oracle is the right signal.
                            // B3 fix: the spec's settlement deviation band is
                            // `resolve_price_deviation_bps` (plain bps, max
                            // MAX_RESOLVE_PRICE_DEVIATION_BPS=10_000), not the
                            // oracle-update circuit breaker `oracle_price_cap_e2bps`
                            // (e2bps, max 1_000_000). Applying the latter here would
                            // enforce the wrong band by two orders of magnitude and
                            // also duplicate the engine's own §9.8 step 7 check.
                            // Drop the wrapper pre-check and let resolve_market_not_atomic
                            // apply the canonical band with the canonical parameter.
                        }
                        Err(e) => {
                            // Only skip guard if oracle is genuinely
                            // unusable — OracleStale OR OracleConfTooWide.
                            // Other errors (wrong account, bad data,
                            // wrong feed) must propagate — otherwise
                            // admin can bypass guard by passing a broken
                            // oracle account. OracleConfTooWide matches
                            // ResolvePermissionless's handling; admin
                            // should be able to resolve when conf_filter
                            // _bps rejects a live-but-wide feed.
                            let stale_err: ProgramError =
                                PercolatorError::OracleStale.into();
                            let conf_err: ProgramError =
                                PercolatorError::OracleConfTooWide.into();
                            if e != stale_err && e != conf_err {
                                return Err(e);
                            }
                            // Oracle is unusable, fall back to engine state
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

                // Resolution uses two DISJOINT branches:
                //
                //   ORDINARY  — live-synchronizing inputs (fresh oracle or Hyperp index):
                //       live_oracle_price = fresh external reading (or flushed index)
                //       funding_rate_e9   = captured pre-oracle-mutation (§5.5)
                //
                //   DEGENERATE — oracle is confirmed dead (non-Hyperp only):
                //       live_oracle_price = P_last = engine.last_oracle_price
                //       funding_rate_e9   = 0   (no live signal over the dead interval)
                //
                // Conflating the two would apply a stale mark-vs-index rate over
                // a dead interval, moving funds with no economic signal backing
                // the transfer. Branch selection is explicit and logged via the
                // distinct arms below.
                let oracle_initialized = state::is_oracle_initialized(&data);
                let engine = zc::engine_mut(&mut data)?;
                let is_hyperp_local = oracle::is_hyperp_mode(&config);

                // Detect stale Hyperp mark. If the Hyperp mark signal
                // (max(mark_ewma_last_slot, last_mark_push_slot)) has
                // been silent for longer than 3 × max_staleness_secs,
                // the interval is signal-free and the admin-Ordinary
                // path would fail (catchup + final accrue at
                // last_effective_price_e6 exceeds
                // CATCHUP_CHUNKS_MAX × max_dt). Route to Degenerate
                // like ResolvePermissionless does — the market is
                // effectively dead and should resolve at P_last with
                // rate = 0 rather than get stuck.
                let hyperp_stale = if is_hyperp_local {
                    let last_update = core::cmp::max(
                        config.mark_ewma_last_slot,
                        config.last_mark_push_slot as u64,
                    );
                    let max_stale_slots = config.max_staleness_secs
                        .saturating_mul(3);
                    clock.slot.saturating_sub(last_update) > max_stale_slots
                        && oracle_initialized
                } else {
                    false
                };

                let (live_oracle, rate_for_final_accrual, in_ordinary_arm): (u64, i128, bool) =
                    if let Some(fresh) = fresh_live_oracle {
                        // ORDINARY: fresh non-Hyperp oracle reading
                        (fresh, funding_rate_e9, true)
                    } else if is_hyperp_local && !hyperp_stale {
                        // ORDINARY: Hyperp uses the just-flushed index as its live
                        // oracle; funding accrual uses the captured rate.
                        (config.last_effective_price_e6, funding_rate_e9, true)
                    } else if (is_hyperp_local && hyperp_stale) || oracle_initialized {
                        // DEGENERATE: non-Hyperp oracle is dead, OR Hyperp
                        // mark has been stale long enough that the live
                        // signal is gone. Resolve at P_last with zero
                        // funding over the signal-free interval. Engine's
                        // Degenerate arm bypasses accrue_market_to so no
                        // envelope/catchup issue.
                        (engine.last_oracle_price, 0i128, false)
                    } else {
                        // Oracle never initialized and no fresh read — cannot settle
                        // a live market from the init sentinel. Empty markets resolve
                        // via ResolvePermissionless (OI=0 safety branch).
                        return Err(PercolatorError::OracleInvalid.into());
                    };
                // Engine v12.18.5+: resolve_market_not_atomic takes an explicit
                // ResolveMode selector (Goal 51). The wrapper tells the engine
                // which arm to run; the engine enforces that Degenerate carries
                // `live == P_last && rate == 0`. A separate wrapper-side band
                // check is no longer required because the engine's band check
                // now runs unconditionally on Ordinary.
                let resolve_mode = if in_ordinary_arm {
                    percolator::ResolveMode::Ordinary
                } else {
                    percolator::ResolveMode::Degenerate
                };
                // Pre-chunk catch-up (Ordinary only). Degenerate arm already
                // bypasses accrue_market_to inside the engine, so no envelope
                // check applies. Ordinary accrues through the final step and
                // would otherwise hit Overflow when the gap exceeds max_dt.
                // Catchup uses the same (price, rate) the final accrue will
                // use, preserving anti-retroactivity (Finding 2).
                if in_ordinary_arm {
                    catchup_accrue(
                        engine, clock.slot, live_oracle, rate_for_final_accrual,
                    )?;
                }
                engine.resolve_market_not_atomic(
                    resolve_mode,
                    settlement_price,
                    live_oracle,
                    clock.slot,
                    rate_for_final_accrual,
                ).map_err(map_risk_error)?;

                state::write_config(&mut data, &config);
            }

            Instruction::WithdrawInsurance => {
                // Withdraw insurance fund. Gated by insurance_authority
                // (scoped), not general admin — operators who have
                // burned admin can still withdraw here if they kept a
                // separate insurance_authority, or lock withdrawal
                // forever by burning insurance_authority.
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
                require_admin(header.insurance_authority, a_admin.key)?;

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

                // Terminal-surplus sweep (audit P1): once all accounts
                // close and all settlement aggregates are zero, any
                // remaining `engine.vault > 0` is pure rounding residue
                // with no outstanding claims. Fold it into the insurance
                // payout so CloseSlab can zero the vault. Without this,
                // a market could arrive at num_used=0, insurance=0,
                // vault>0 from conservative quote rounding and become
                // un-closeable.
                //
                // Safety: all the aggregate zero-checks below MUST hold.
                // If they don't, there are still settlement obligations
                // and the surplus is NOT sweepable. In that case the
                // normal insurance-only drain runs.
                let c_tot_zero = engine.c_tot.get() == 0;
                let pnl_zero = engine.pnl_pos_tot == 0
                    && engine.pnl_matured_pos_tot == 0;
                let oi_zero = engine.oi_eff_long_q == 0
                    && engine.oi_eff_short_q == 0;
                // Fail-closed: also require every position/stale/neg-PnL
                // counter to be zero. If any counter is nonzero in the
                // face of num_used_accounts == 0, we have corrupt state
                // and the surplus is NOT safely sweepable — the normal
                // insurance-only drain runs instead, and the surplus
                // remains (CloseSlab stays blocked until the invariant
                // break is investigated).
                let position_counters_zero =
                    engine.stored_pos_count_long == 0
                    && engine.stored_pos_count_short == 0
                    && engine.stale_account_count_long == 0
                    && engine.stale_account_count_short == 0
                    && engine.neg_pnl_account_count == 0;
                let terminal_surplus_ok =
                    c_tot_zero && pnl_zero && oi_zero && position_counters_zero;

                // Payout = insurance balance + terminal surplus (if any).
                let insurance_units = engine.insurance_fund.balance.get();
                let vault_units = engine.vault.get();
                let payout_units = if terminal_surplus_ok {
                    vault_units
                } else {
                    insurance_units
                };
                if payout_units == 0 {
                    return Ok(()); // Nothing to withdraw
                }

                // Reject if payout exceeds u64 — silent truncation would
                // zero the engine balance but only pay out a capped amount.
                let units_u64: u64 = payout_units
                    .try_into()
                    .map_err(|_| PercolatorError::EngineOverflow)?;
                let base_amount = crate::units::units_to_base_checked(units_u64, config.unit_scale)
                    .ok_or(PercolatorError::EngineOverflow)?;

                // Zero out insurance fund and decrement engine.vault by the
                // full payout (insurance + any terminal surplus).
                engine.insurance_fund.balance = percolator::U128::ZERO;
                let payout = percolator::U128::new(payout_units);
                if payout > engine.vault {
                    return Err(PercolatorError::EngineInsufficientBalance.into());
                }
                engine.vault = engine.vault - payout;

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

                let _clock = Clock::from_account_info(&accounts[6])?;
                let engine = zc::engine_mut(&mut data)?;
                let (price, resolved_slot) = engine.resolved_context();
                if price == 0 {
                    return Err(ProgramError::InvalidAccountData);
                }

                check_idx(engine, user_idx)?;

                let owner_pubkey = Pubkey::new_from_array(engine.accounts[user_idx as usize].owner);

                // Realize recurring maintenance fees to the resolved anchor
                // BEFORE force_close_resolved. Engine's
                // force_close_resolved_not_atomic does not itself sync the
                // fee cursor. On Resolved mode sync anchors at resolved_slot
                // automatically. No-op when maintenance_fee_per_slot == 0.
                sync_account_fee(engine, &config, user_idx, resolved_slot)?;

                // Engine v12.18.6+: slot argument removed — the engine pulls
                // resolved_slot from its own state (§9.9).
                let _ = resolved_slot;
                let amt_units = match engine.force_close_resolved_not_atomic(user_idx)
                    .map_err(map_risk_error)?
                {
                    percolator::ResolvedCloseResult::ProgressOnly => return Ok(()),
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
                // Hard-timeout gate: "dead means dead" — no live
                // mutations past the stale horizon. Users exit via
                // ResolvePermissionless + resolved-market close paths.
                let config = state::read_config(&data);
                if oracle::permissionless_stale_matured(&config, clock.slot) {
                    return Err(PercolatorError::OracleStale.into());
                }

                let engine = zc::engine_mut(&mut data)?;
                // Sync recurring fees before deciding reclaim eligibility.
                // Spec obligation (§10.7 wrapper rule): when wrapper-owned
                // recurring fees are enabled, the wrapper MUST sync fees to
                // the market anchor before consulting reclaim-sensitive
                // account state. Without this sync, latent maintenance
                // fees since last_fee_slot_i go unrealized — insurance
                // under-collects and the "flat" predicate can pass against
                // stale capital. Bounded-to-market (no accrue): we are in
                // the no-oracle branch and MUST NOT accrue per §10.7, so
                // anchor at min(clock.slot, engine.last_market_slot) and
                // let a subsequent accrue-bearing op close any residual
                // gap.
                sync_account_fee_bounded_to_market(
                    engine, &config, user_idx, clock.slot,
                )?;
                // No-oracle path: cap anchor at last_market_slot but
                // floor at current_slot so the engine's monotonicity
                // guard (now_slot >= current_slot) holds even in the
                // transient state where a previous no-oracle op (e.g.
                // InitUser / DepositCollateral) has advanced current
                // _slot past last_market_slot. Full accrue still
                // happens on the next oracle-backed op.
                let bounded_now = core::cmp::max(
                    core::cmp::min(clock.slot, engine.last_market_slot),
                    engine.current_slot,
                );
                engine.reclaim_empty_account_not_atomic(user_idx, bounded_now)
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
                // Anti-retroactivity: capture funding rate before oracle read (§5.5)
                let funding_rate_e9 = compute_current_funding_rate_e9(&config);
                let price = if is_hyperp {
                    let eng = zc::engine_ref(&data)?;
                    let last_slot = eng.current_slot;
                    oracle::get_engine_oracle_price_e6(
                        last_slot, clock.slot, clock.unix_timestamp,
                        &mut config, a_oracle,
                    )?
                } else {
                    read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot, &mut data)?
                };
                state::write_config(&mut data, &config);

                let engine = zc::engine_mut(&mut data)?;
                let admit_h_min = engine.params.h_min;
                let admit_h_max = engine.params.h_max;
                // Fully accrue market to clock.slot BEFORE fee sync +
                // settle_account_not_atomic. Explicit accrue→sync→op.
                ensure_market_accrued_to_now(engine, clock.slot, price, funding_rate_e9)?;
                // Realize due maintenance fees BEFORE settle so the settle's
                // equity computation reflects post-fee capital.
                sync_account_fee(engine, &config, user_idx, clock.slot)?;
                engine.settle_account_not_atomic(user_idx, price, clock.slot,
                    funding_rate_e9,
                    admit_h_min,
                    admit_h_max)
                    .map_err(map_risk_error)?;
                drop(engine);
                if !state::is_oracle_initialized(&data) {
                    state::set_oracle_initialized(&mut data);
                }
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

                // Phase 1: sync latent maintenance fees and read post-sync
                // debt. Done under a mutable borrow so sync_account_fee can
                // realize fees accrued since last_fee_slot BEFORE we compare
                // `amount` against the outstanding-debt cap. Otherwise a user
                // with zero realized debt but nonzero latent fees would get
                // their legitimate repayment rejected as overpayment.
                let (unit_scale, debt_units) = {
                    let mut data = state::slab_data_mut(a_slab)?;
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
                    let clock = Clock::from_account_info(a_clock)?;
                    // Hard-timeout gate: no fee-credit deposits into a
                    // terminally-stale market. Users exit via resolve.
                    if oracle::permissionless_stale_matured(&cfg, clock.slot) {
                        return Err(PercolatorError::OracleStale.into());
                    }

                    let engine = zc::engine_mut(&mut data)?;
                    check_idx(engine, user_idx)?;
                    let owner = engine.accounts[user_idx as usize].owner;
                    if !crate::verify::owner_ok(owner, a_user.key.to_bytes()) {
                        return Err(PercolatorError::EngineUnauthorized.into());
                    }
                    // No-oracle path: sync fees at an anchor bounded by
                    // engine.last_market_slot (see sync_account_fee doc).
                    sync_account_fee_bounded_to_market(
                        engine, &cfg, user_idx, clock.slot,
                    )?;
                    let fc = engine.accounts[user_idx as usize].fee_credits.get();
                    let debt = if fc < 0 { fc.unsigned_abs() } else { 0u128 };
                    (cfg.unit_scale, debt)
                };
                // slab_data_mut released; OK to do SPL transfer next.

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

                // Phase 4: book the repayment in the engine. The engine's
                // deposit_fee_credits self-advances current_slot; bound its
                // now_slot at last_market_slot BUT floor at current_slot
                // so the engine monotonicity guard (`now_slot >= current
                // _slot`) holds after any prior no-oracle self-advance
                // (InitUser, DepositCollateral, ReclaimEmptyAccount,
                // TopUpInsurance). Full accrue for the residual tail
                // happens on the next oracle-backed op.
                let mut data = state::slab_data_mut(a_slab)?;
                let config = state::read_config(&data);
                let clock = Clock::from_account_info(a_clock)?;
                let (units2, _dust) = crate::units::base_to_units(amount, config.unit_scale);
                let engine = zc::engine_mut(&mut data)?;
                let _ = &config; // Phase 1 synced; no second sync needed.
                let bounded_now = core::cmp::max(
                    core::cmp::min(clock.slot, engine.last_market_slot),
                    engine.current_slot,
                );
                engine.deposit_fee_credits(user_idx, units2 as u128, bounded_now)
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
                // Anti-retroactivity: capture funding rate before oracle read (§5.5)
                let funding_rate_e9 = compute_current_funding_rate_e9(&config);
                let price = if is_hyperp {
                    let eng = zc::engine_ref(&data)?;
                    let last_slot = eng.current_slot;
                    oracle::get_engine_oracle_price_e6(
                        last_slot, clock.slot, clock.unix_timestamp,
                        &mut config, a_oracle,
                    )?
                } else {
                    read_price_and_stamp(&mut config, a_oracle, clock.unix_timestamp, clock.slot, &mut data)?
                };
                state::write_config(&mut data, &config);

                let engine = zc::engine_mut(&mut data)?;
                check_idx(engine, user_idx)?;
                let owner = engine.accounts[user_idx as usize].owner;
                if !crate::verify::owner_ok(owner, a_user.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                let (units, dust) = crate::units::base_to_units(amount, config.unit_scale);
                if dust != 0 {
                    return Err(ProgramError::InvalidArgument);
                }
                let admit_h_min = engine.params.h_min;
                let admit_h_max = engine.params.h_max;
                // Fully accrue market to clock.slot BEFORE fee sync +
                // convert_released_pnl_not_atomic. Explicit accrue→sync→op.
                ensure_market_accrued_to_now(engine, clock.slot, price, funding_rate_e9)?;
                // Realize due maintenance fees BEFORE conversion so the
                // convertible-PnL bound reflects post-fee equity.
                sync_account_fee(engine, &config, user_idx, clock.slot)?;
                engine.convert_released_pnl_not_atomic(user_idx, units as u128, price, clock.slot,
                    funding_rate_e9,
                    admit_h_min,
                    admit_h_max)
                    .map_err(map_risk_error)?;
                drop(engine);
                if !state::is_oracle_initialized(&data) {
                    state::set_oracle_initialized(&mut data);
                }
            }

            Instruction::ResolvePermissionless => {
                // STRICT HARD-TIMEOUT POLICY:
                //
                //   clock.slot - last_live_slot >= permissionless_resolve_stale_slots
                //     → market is stale, anyone resolves at engine.last_oracle_price
                //
                //   last_live_slot is:
                //     non-Hyperp → config.last_good_oracle_slot
                //                  (advances ONLY on successful external oracle
                //                  reads; authority fallback does NOT advance it)
                //     Hyperp     → max(mark_ewma_last_slot, last_mark_push_slot)
                //                  (advances on trades and mark pushes)
                //
                // No challenge window, no first-observation stamp, no oracle
                // account submitted at resolve time. The timer is purely
                // "slots since last accepted fresh oracle update for this
                // market". If no one has fed the market a fresh oracle for
                // N slots, it's dead. Period.
                //
                // Settlement is at engine.last_oracle_price (the last price
                // the engine actually accrued against), for both Hyperp and
                // non-Hyperp. Authority-fallback drift during the stale
                // window IS reflected here — trades and cranks that ran on
                // authority fallback fed into the engine — but once the
                // window matures, all price-taking live instructions also
                // reject (see permissionless_stale_matured() check in
                // read_price_and_stamp and the Hyperp price path), so there
                // is no further drift from the point of maturity onward.
                accounts::expect_len(accounts, 2)?;
                let a_slab = &accounts[0];
                let a_clock = &accounts[1];

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

                if !oracle::permissionless_stale_matured(&config, clock.slot) {
                    return Err(PercolatorError::OracleStale.into());
                }

                // Degenerate resolve at engine.last_oracle_price. Both
                // settlement_price and live_oracle_price use P_last (the
                // engine's stored last-accrued price) — the engine's
                // Degenerate arm requires equality between the two.
                let engine = zc::engine_mut(&mut data)?;
                let p_last = engine.last_oracle_price;
                if p_last == 0 {
                    return Err(PercolatorError::OracleInvalid.into());
                }
                engine.resolve_market_not_atomic(
                    percolator::ResolveMode::Degenerate,
                    p_last,
                    p_last,
                    clock.slot,
                    0,
                ).map_err(map_risk_error)?;

                config.authority_price_e6 = p_last;
                // first_observed_stale_slot is no longer load-bearing in
                // the hard-timeout model. Clear any legacy stamp on
                // resolve so the field reads 0 post-resolution (keeps
                // telemetry clean; safe because the market is terminal).
                config.first_observed_stale_slot = 0;
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
                let (price, resolved_slot) = engine.resolved_context();
                if price == 0 {
                    return Err(ProgramError::InvalidAccountData);
                }
                check_idx(engine, user_idx)?;

                let owner_pubkey = Pubkey::new_from_array(
                    engine.accounts[user_idx as usize].owner,
                );

                // Realize recurring maintenance fees to resolved_slot
                // BEFORE force_close_resolved (Finding 4). Engine does not
                // sync the fee cursor inside force_close. No-op when
                // maintenance_fee_per_slot == 0.
                sync_account_fee(engine, &config, user_idx, resolved_slot)?;

                // Engine v12.18.6+ (§9.9): slot arg removed; engine uses resolved_slot.
                let _ = resolved_slot;
                let amt_units = match engine.force_close_resolved_not_atomic(user_idx)
                    .map_err(map_risk_error)?
                {
                    percolator::ResolvedCloseResult::ProgressOnly => return Ok(()),
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

            Instruction::CatchupAccrue => {
                // Permissionless market-clock catchup. REQUIRES a live
                // oracle — proves the market is live before advancing
                // funding/accrual. Dead oracles must use
                // ResolvePermissionless (Degenerate arm, rate = 0);
                // they cannot reach CatchupAccrue. This removes the
                // prior race where CatchupAccrue (nonzero rate) could
                // settle the same dead interval that
                // ResolvePermissionless would settle at rate 0 —
                // non-deterministic settlement.
                //
                // Two modes:
                //
                //   COMPLETE — the instruction can advance the engine all
                //   the way to clock.slot in this single call
                //   (gap ≤ CATCHUP_CHUNKS_MAX × max_dt). Behaves like any
                //   ordinary inline accrue-bearing op: oracle read mutates
                //   config, pre-read rate chunks the historical interval
                //   at stored P_last, final accrue to clock.slot installs
                //   the fresh observation. Persist mutated config.
                //
                //   PARTIAL  — the gap is too large to finish in one
                //   call. Oracle is STILL read (liveness proof), but the
                //   observation is NOT persisted: no fresh price enters
                //   the engine, no config.last_effective_price_e6 /
                //   last_hyperp_index_slot / last_good_oracle_slot update
                //   leaks into later calls whose catchup hasn't reached
                //   the observation slot yet. The engine mechanically
                //   advances through `target` using stored P_last +
                //   pre-read rate only. Subsequent CatchupAccrue calls
                //   observe freshly, and the LAST call (which reaches
                //   clock.slot) persists the final observation.
                //
                // This splits prevents the time-travel that would occur
                // if a partial catchup wrote the current observation
                // immediately: later catchups would compute funding
                // using the already-advanced config index, retroactively
                // applying post-observation state to pre-observation
                // engine slots.
                //
                // Takes slab + clock + oracle.
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

                let clock = Clock::from_account_info(a_clock)?;

                // Snapshot pre-read config (MarketConfig is Pod+Copy, so
                // this is a byte-copy — cheap). Used to restore in PARTIAL
                // mode so the oracle observation is not persisted until
                // the FINAL catchup call that actually reaches clock.slot.
                let config_pre = state::read_config(&data);
                let mut config = config_pre;

                // Pre-read funding rate (anti-retroactivity §5.5).
                let funding_rate_e9_pre = compute_current_funding_rate_e9(&config);

                // Oracle read — proves market is live. Mutates `config`
                // locally (clamp/stamp for non-Hyperp, clamp-toward for
                // Hyperp). Failure routes caller to ResolvePermissionless.
                let is_hyperp = oracle::is_hyperp_mode(&config);
                let fresh_price = if is_hyperp {
                    let eng = zc::engine_ref(&data)?;
                    let last_slot = eng.current_slot;
                    oracle::get_engine_oracle_price_e6(
                        last_slot, clock.slot, clock.unix_timestamp,
                        &mut config, a_oracle,
                    )?
                } else {
                    read_price_and_stamp(
                        &mut config, a_oracle, clock.unix_timestamp, clock.slot, &mut data,
                    )?
                };

                let engine = zc::engine_mut(&mut data)?;

                // Engine never seeded — nothing to catch up past. The
                // caller's next ordinary op will seed last_oracle_price.
                // Persist the fresh observation since there's nothing to
                // "leak into" historically.
                if engine.last_oracle_price == 0 {
                    state::write_config(&mut data, &config);
                    if !state::is_oracle_initialized(&data) {
                        state::set_oracle_initialized(&mut data);
                    }
                    return Ok(());
                }

                // Decide COMPLETE vs PARTIAL based on whether one call can
                // close the full gap. If funding is inactive (rate == 0
                // OR one side has no OI OR fund_px_last == 0), the engine
                // doesn't enforce the dt envelope — accrue_market_to can
                // jump the full gap in one call. Treat as can_finish
                // regardless of gap size. This matches the engine's own
                // §5.5 clause-6 predicate and prevents CatchupAccrue from
                // gratuitously stepping partial targets on empty/inactive
                // markets.
                let max_dt = engine.params.max_accrual_dt_slots;
                let max_step_per_call = (CATCHUP_CHUNKS_MAX as u64)
                    .saturating_mul(max_dt);
                let gap = clock.slot.saturating_sub(engine.last_market_slot);
                let funding_active = funding_rate_e9_pre != 0
                    && engine.oi_eff_long_q != 0
                    && engine.oi_eff_short_q != 0
                    && engine.fund_px_last > 0;
                let can_finish = !funding_active || gap <= max_step_per_call;

                if can_finish {
                    // COMPLETE: chunk to clock.slot using stored P_last
                    // (per catchup_accrue's invariant — keeps fund_px
                    // _last pinned across chunks). Final residual
                    // accrue installs fresh_price. Persist the mutated
                    // config so the observation is recorded.
                    catchup_accrue(engine, clock.slot, fresh_price, funding_rate_e9_pre)?;
                    if clock.slot > engine.last_market_slot {
                        engine
                            .accrue_market_to(clock.slot, fresh_price, funding_rate_e9_pre)
                            .map_err(map_risk_error)?;
                    }
                    state::write_config(&mut data, &config);
                } else {
                    // PARTIAL: use stored P_last throughout (NOT the
                    // fresh price) and DO NOT persist the time-travel
                    // -sensitive oracle/index fields. Subsequent
                    // CatchupAccrue calls will observe freshly and the
                    // last one (which CAN finish) installs the final
                    // observation.
                    let stored_p_last = engine.last_oracle_price;
                    let target = engine.last_market_slot.saturating_add(max_step_per_call);
                    catchup_accrue(engine, target, stored_p_last, funding_rate_e9_pre)?;
                    if target > engine.last_market_slot {
                        engine
                            .accrue_market_to(target, stored_p_last, funding_rate_e9_pre)
                            .map_err(map_risk_error)?;
                    }
                    // Rollback selectively: roll back price/index state
                    // that would retroactively apply a post-observation
                    // index to pre-observation engine slots, but PRESERVE
                    // liveness-recovery evidence from the successful
                    // read. Otherwise a partial catchup during an idle
                    // market's oracle recovery would resurrect an old
                    // `first_observed_stale_slot`, potentially letting
                    // ResolvePermissionless span two disjoint stale
                    // windows as if they were continuous.
                    //
                    // Fields rolled back (price/index — time-travel risk):
                    //   - last_effective_price_e6     (baseline)
                    //   - last_hyperp_index_slot      (Hyperp index clock)
                    //
                    // Fields preserved from the fresh read (liveness):
                    //   - first_observed_stale_slot   (cleared to 0 by
                    //       a successful external read; must stay 0
                    //       since the market IS live right now)
                    //   - last_good_oracle_slot       (stamp proving
                    //       external oracle was observed live this call)
                    //
                    // All other fields come from config_pre (no changes
                    // expected from the oracle read anyway).
                    let mut restored = config_pre;
                    restored.first_observed_stale_slot = config.first_observed_stale_slot;
                    restored.last_good_oracle_slot = config.last_good_oracle_slot;
                    state::write_config(&mut data, &restored);
                }

                if !state::is_oracle_initialized(&data) {
                    state::set_oracle_initialized(&mut data);
                }
            }

            Instruction::UpdateAuthority { kind, new_pubkey } => {
                handle_update_authority(program_id, accounts, kind, new_pubkey)?;
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
