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
    pub const RISK_BUF_OFF: usize = ENGINE_OFF + ENGINE_LEN;
    pub const RISK_BUF_LEN: usize = size_of::<crate::risk_buffer::RiskBuffer>();
    /// Per-account materialization generation table.
    /// Stores the global mat_counter value assigned at InitUser/InitLP.
    /// Used as lp_account_id for per-instance identity across slot reuse.
    pub const GEN_TABLE_OFF: usize = RISK_BUF_OFF + RISK_BUF_LEN;
    pub const GEN_TABLE_LEN: usize = percolator::MAX_ACCOUNTS * 8; // u64 per slot
    pub const SLAB_LEN: usize = GEN_TABLE_OFF + GEN_TABLE_LEN;

    /// Progressive risk-buffer discovery window per crank. Kept small because
    /// this runs after the engine's liquidation cascade; dense worst-case
    /// cranks must still fit under the SVM CU cap. Honest keepers can always
    /// refill urgent accounts immediately through the candidate tail.
    pub const RISK_SCAN_WINDOW: usize = 8;
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

    /// Maximum caller-supplied liquidation candidates decoded for one crank.
    /// This is intentionally larger than the per-crank execution budget so
    /// honest keepers can over-specify the next cascade segment; the wrapper
    /// uses the tail for issue-65 coverage checks while the engine executes a
    /// CU-bounded prefix.
    pub const MAX_KEEPER_CANDIDATES: usize = 128;
    /// Phase 1 revalidation/liquidation execution budget per KeeperCrank
    /// (wrapper-owned since v12.19, which dropped the engine-level
    /// `LIQ_BUDGET_PER_CRANK`). Kept at 8 so dense liquidation cascades plus
    /// wrapper coverage checks fit the SVM compute envelope.
    pub const LIQ_BUDGET_PER_CRANK: u16 = 8;
    /// Maximum candidate entries the wrapper/engine may inspect before Phase 2.
    /// This is deliberately independent from the liquidation execution budget:
    /// malformed or non-actionable candidate padding still consumes bounded
    /// inspection work, while honest keepers can over-specify a small tail.
    pub const CANDIDATE_INSPECTION_BUDGET_PER_CRANK: u16 = 32;
    /// Phase 2 mandatory engine round-robin touch window. Kept at 32 so dense
    /// no-candidate cranks remain below the SVM CU cap after the engine's
    /// liquidation pass and wrapper risk-buffer maintenance.
    pub const RR_WINDOW_PER_CRANK: u64 = 32;
    /// Candidate-bearing cranks have extra wrapper coverage checks and engine
    /// liquidation work. Use the same fixed touch budget as empty cranks so
    /// KeeperCrank CU does not scale with total live accounts.
    pub const RR_WINDOW_WITH_CANDIDATES_PER_CRANK: u64 = 32;
    /// Engine Phase 2 inspected-slot cap. This is intentionally explicit even
    /// when the deployment wants full-slab greedy progress: avoid delegating
    /// unbounded `u64::MAX` scan policy to the engine API.
    pub const RR_SCAN_LIMIT_PER_CRANK: u64 = percolator::MAX_ACCOUNTS as u64;

    // Compile-time invariant: the crank's total fee-sync budget
    // (FEE_SWEEP_BUDGET) must accommodate the wrapper's per-crank
    // liquidation/candidate-sync allowance. The candidate-sync path
    // caps itself at min(LIQ_BUDGET_PER_CRANK, FEE_SWEEP_BUDGET); if
    // this constant were raised above FEE_SWEEP_BUDGET the belt-and-
    // braces min() would silently under-apply the budget. Assert so
    // a mismatch is a build error.
    const _: () = assert!(
        (LIQ_BUDGET_PER_CRANK as usize) <= FEE_SWEEP_BUDGET,
        "LIQ_BUDGET_PER_CRANK must not exceed FEE_SWEEP_BUDGET"
    );
    const _: () = assert!(
        (LIQ_BUDGET_PER_CRANK as u64) + RR_WINDOW_PER_CRANK
            <= percolator::MAX_TOUCHED_PER_INSTRUCTION as u64,
        "KeeperCrank Phase 1 + Phase 2 must fit engine touched-account capacity"
    );
    const _: () = assert!(
        (LIQ_BUDGET_PER_CRANK as u64) + RR_WINDOW_WITH_CANDIDATES_PER_CRANK
            <= percolator::MAX_TOUCHED_PER_INSTRUCTION as u64,
        "KeeperCrank candidate Phase 1 + Phase 2 must fit engine touched-account capacity"
    );

    // ── Engine envelope constants (wrapper-owned, immutable per deployment) ──
    //
    // These values populate the engine's per-market RiskParams envelope at
    // InitMarket. They are NOT decoded from instruction data and NOT admin-
    // configurable — every deployment uses these exact values. The envelope
    // invariant
    //   ADL_ONE * MAX_ORACLE_PRICE * MAX_ABS_FUNDING_E9_PER_SLOT *
    //     MAX_ACCRUAL_DT_SLOTS <= i128::MAX
    // must hold: 1e15 * 1e12 * 1e4 * 100 = 1e33 < i128::MAX (≈1.7e38). ✓
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
    ///
    /// Tightened from the legacy 10_000_000 to satisfy the v12.19 engine
    /// solvency envelope (§1.4):
    ///
    ///   max_price_move_bps_per_slot * max_accrual_dt_slots
    ///     + floor(max_abs_funding_e9_per_slot * max_accrual_dt_slots
    ///             * 10_000 / FUNDING_DEN)
    ///     + liquidation_fee_bps
    ///     <= maintenance_margin_bps
    ///
    /// For a deployment with maintenance=500, liq=5, max_price_move=49
    /// bps/slot, max_abs_funding_e9_per_slot=10_000 the envelope
    /// admits a 10-slot max accrual window:
    ///
    ///   49 * 10 + floor(10_000 * 10 * 10_000 / FUNDING_DEN) + 5 <= 500
    ///
    /// This keeps the maximum price move over one max-dt crank window close
    /// to 1 / leverage for a 20x market. Catchup loops up to
    /// `CATCHUP_CHUNKS_MAX × 10` = 200 slots in one instruction before
    /// `CatchupRequired`.
    pub const MAX_ACCRUAL_DT_SLOTS: u64 = 10;
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
    pub const MATCHER_CONTEXT_LEN: usize = 320;
    /// Maximum variadic accounts forwarded by TradeCpi to the matcher.
    /// Transaction account limits are an outer bound; this protocol cap keeps
    /// wrapper heap/CU and matcher ABI expectations explicit.
    pub const MAX_MATCHER_TAIL_ACCOUNTS: usize = 32;
    pub const MATCHER_CALL_TAG: u8 = 0;
    pub const MATCHER_CALL_LEN: usize = 67;

    /// Sentinel value for permissionless crank (no caller account required)
    pub const CRANK_NO_CALLER: u16 = u16::MAX;

    /// Maximum allowed unit_scale for InitMarket.
    /// unit_scale=0 disables scaling (1:1 base tokens to units, dust=0 always).
    /// unit_scale=1..=1_000_000_000 enables scaling with dust tracking.
    pub const MAX_UNIT_SCALE: u32 = 1_000_000_000;
    /// InitMarket confidence-filter range for external oracle markets.
    /// Zero used to mean "disabled"; new deployments must carry an explicit
    /// nonzero confidence bound so a stale/misconfigured wide-conf feed cannot
    /// silently become the market's accepted oracle.
    pub const MIN_CONF_FILTER_BPS: u16 = 50;
    pub const MAX_CONF_FILTER_BPS: u16 = 1_000;
    /// InitMarket upper bound on oracle staleness. Kept intentionally short:
    /// a market that tolerates hours/days of stale oracle data lets the admin
    /// or deployer choose a liveness footgun that users cannot distinguish at
    /// runtime from an intentionally permissive market.
    pub const MAX_ORACLE_STALENESS_SECS: u64 = 600;

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
    pub const DEFAULT_MARK_EWMA_HALFLIFE_SLOTS: u64 = 100; // ~40 sec @ 2.5 slots/sec
    /// Default slot-based oracle staleness window before anyone may resolve.
    /// Disabled by default (0 == opt-out). Markets that need permissionless
    /// resolution MUST set this explicitly on the extended InitMarket tail.
    /// The non-Hyperp resolvability guard (see InitMarket) still requires a
    /// non-zero value OR Hyperp mode, so an admin-free non-Hyperp market can't
    /// be shipped with this at 0.
    pub const DEFAULT_PERMISSIONLESS_RESOLVE_STALE_SLOTS: u64 = 0;
    /// Upper bound on `force_close_delay_slots` (Finding 6). Without a bound, an
    /// init-time config of `u64::MAX` passes the "nonzero" liveness guard but
    /// makes ForceCloseResolved unreachable — `resolved_slot + delay` saturates
    /// to `u64::MAX`, stranding any accounts left on a resolved market whose
    /// admin was burned. 10_000_000 slots is ~50 days at 2 slots/s, far beyond
    /// any reasonable grace period but well short of the saturation regime.
    pub const MAX_FORCE_CLOSE_DELAY_SLOTS: u64 = 10_000_000;
    /// Maximum profit-maturity horizon (`RiskParams.h_max`) accepted by the
    /// wrapper. This is a product safety bound, not an arithmetic envelope:
    /// h_max is independent from `max_accrual_dt_slots` and oracle staleness.
    /// 6_480_000 slots is ~30 days at 400 ms/slot.
    pub const MAX_PROFIT_MATURITY_SLOTS: u64 = 6_480_000;
    /// Maximum hard oracle-staleness horizon for permissionless market
    /// resolution. This is a product liveness bound, not an accrual envelope:
    /// dead-oracle resolution uses the engine's Degenerate path at P_last,
    /// while live catchup/progress is routed through KeeperCrank.
    /// 6_480_000 slots is ~30 days at 400 ms/slot.
    pub const MAX_PERMISSIONLESS_RESOLVE_STALE_SLOTS: u64 = 6_480_000;
    /// InitMarket wire flag packed into insurance_withdraw_max_bps. When set,
    /// WithdrawInsuranceLimited can withdraw only explicit TopUpInsurance
    /// principal; insurance growth from fees/liquidations remains behind.
    pub const INSURANCE_WITHDRAW_DEPOSITS_ONLY_FLAG: u16 = 0x8000;
    pub const INSURANCE_WITHDRAW_MAX_BPS_MASK: u16 = 0x7FFF;
}

// =============================================================================
// Persistent risk cache and pure policy helpers.
// =============================================================================

// 1b. mod risk_buffer
pub mod risk_buffer {
    use crate::constants::RISK_BUF_CAP;
    use bytemuck::{Pod, Zeroable};

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
                2 => core::cmp::min(self.entries[0].notional, self.entries[1].notional),
                3 => core::cmp::min(
                    self.entries[0].notional,
                    core::cmp::min(self.entries[1].notional, self.entries[2].notional),
                ),
                _ => core::cmp::min(
                    core::cmp::min(self.entries[0].notional, self.entries[1].notional),
                    core::cmp::min(self.entries[2].notional, self.entries[3].notional),
                ),
            };
        }

        pub fn find(&self, idx: u16) -> Option<usize> {
            if self.count > 0 && self.entries[0].idx == idx {
                return Some(0);
            }
            if self.count > 1 && self.entries[1].idx == idx {
                return Some(1);
            }
            if self.count > 2 && self.entries[2].idx == idx {
                return Some(2);
            }
            if self.count > 3 && self.entries[3].idx == idx {
                return Some(3);
            }
            None
        }

        fn min_slot(&self) -> usize {
            let mut m = 0;
            if self.count > 1 && self.entries[1].notional < self.entries[m].notional {
                m = 1;
            }
            if self.count > 2 && self.entries[2].notional < self.entries[m].notional {
                m = 2;
            }
            if self.count > 3 && self.entries[3].notional < self.entries[m].notional {
                m = 3;
            }
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

/// Pure policy helpers for program-level authorization and CPI binding.
pub mod policy {
    use crate::constants::MATCHER_CONTEXT_LEN;

    /// Owner authorization: stored owner must match signer.
    /// Used by: DepositCollateral, WithdrawCollateral, TradeNoCpi, TradeCpi, CloseAccount
    #[inline]
    pub fn owner_ok(stored: [u8; 32], signer: [u8; 32]) -> bool {
        stored == signer
    }

    /// Admin authorization: admin must be non-zero (not burned) and match signer.
    /// Used by: UpdateAuthority, UpdateConfig, and other admin-gated ops.
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

    /// Account-free catchup cannot safely perform equity-active accrual on an
    /// exposed market. Without a candidate list or account touch set, there is
    /// no liquidation/revalidation turn between cumulative price/funding moves.
    #[inline]
    pub fn account_free_catchup_allows_accrual(
        oi_eff_long_q: u128,
        oi_eff_short_q: u128,
        last_oracle_price: u64,
        fresh_price: u64,
        funding_rate_e9: i128,
        fund_px_last: u64,
    ) -> bool {
        let exposed = oi_eff_long_q != 0 || oi_eff_short_q != 0;
        if !exposed {
            return true;
        }

        let price_move_active = last_oracle_price > 0 && fresh_price != last_oracle_price;
        let funding_active =
            funding_rate_e9 != 0 && oi_eff_long_q != 0 && oi_eff_short_q != 0 && fund_px_last > 0;

        !price_move_active && !funding_active
    }

    /// Account-limited instructions must not be a hidden market progress path
    /// when exposed OI exists. They only touch a bounded caller-supplied set;
    /// unrelated liquidatable accounts need KeeperCrank's cascade before the
    /// market clock/price/funding can advance.
    #[inline]
    pub fn account_limited_op_allows_accrual(
        oi_eff_long_q: u128,
        oi_eff_short_q: u128,
        last_oracle_price: u64,
        fresh_price: u64,
        funding_rate_e9: i128,
        fund_px_last: u64,
        dt_slots: u64,
    ) -> bool {
        let exposed = oi_eff_long_q != 0 || oi_eff_short_q != 0;
        if !exposed {
            return true;
        }

        let price_move_active = last_oracle_price > 0 && fresh_price != last_oracle_price;
        let funding_active = dt_slots != 0
            && funding_rate_e9 != 0
            && oi_eff_long_q != 0
            && oi_eff_short_q != 0
            && fund_px_last > 0;

        !price_move_active && !funding_active
    }

    /// Pure funding-rate policy from mark/index premium.
    ///
    /// Returns `None` for corrupt negative caps. Otherwise returns the e9
    /// per-slot funding rate after premium clamp, k scaling, horizon division,
    /// and final per-slot rate clamp.
    #[inline]
    pub fn funding_rate_e9_from_mark_index(
        mark_ewma_e6: u64,
        last_effective_price_e6: u64,
        funding_horizon_slots: u64,
        funding_k_bps: u64,
        funding_max_premium_bps: i64,
        funding_max_e9_per_slot: i64,
    ) -> Option<i128> {
        if funding_max_premium_bps < 0 || funding_max_e9_per_slot < 0 {
            return None;
        }

        let mark = mark_ewma_e6;
        let index = last_effective_price_e6;
        if mark == 0 || index == 0 || funding_horizon_slots == 0 {
            return Some(0);
        }

        let diff = mark as i128 - index as i128;
        let mut premium_e9 = diff.saturating_mul(1_000_000_000) / (index as i128);

        let max_prem_e9 = (funding_max_premium_bps as i128) * 100_000;
        premium_e9 = premium_e9.clamp(-max_prem_e9, max_prem_e9);

        let scaled = premium_e9.saturating_mul(funding_k_bps as i128) / 100;
        let per_slot = scaled / (funding_horizon_slots as i128);

        let max_rate_e9 = funding_max_e9_per_slot as i128;
        Some(per_slot.clamp(-max_rate_e9, max_rate_e9))
    }

    /// KeeperCrank partial-catchup decision. When the market clock is too far
    /// behind for one inline accrual, the crank may commit one bounded
    /// equity-active segment while keeping the authenticated wall-clock slot
    /// as the public time anchor.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CrankCatchupTarget {
        None,
        Target(u64),
    }

    #[inline]
    pub fn oversized_crank_catchup_target(
        max_accrual_dt_slots: u64,
        chunks_per_instruction: u64,
        last_market_slot: u64,
        now_slot: u64,
        last_oracle_price: u64,
        fresh_price: u64,
        funding_rate_e9: i128,
        oi_eff_long_q: u128,
        oi_eff_short_q: u128,
        fund_px_last: u64,
    ) -> CrankCatchupTarget {
        if max_accrual_dt_slots == 0
            || chunks_per_instruction == 0
            || now_slot <= last_market_slot
            || last_oracle_price == 0
        {
            return CrankCatchupTarget::None;
        }

        let _ = chunks_per_instruction;
        let gap = now_slot - last_market_slot;
        if gap <= max_accrual_dt_slots {
            return CrankCatchupTarget::None;
        }

        let oi_any = oi_eff_long_q != 0 || oi_eff_short_q != 0;
        let funding_active =
            funding_rate_e9 != 0 && oi_eff_long_q != 0 && oi_eff_short_q != 0 && fund_px_last > 0;
        let price_move_active = fresh_price != last_oracle_price && oi_any;
        if !funding_active && !price_move_active {
            return CrankCatchupTarget::None;
        }

        // Since gap > max_accrual_dt_slots and gap = now - last,
        // last + max_accrual_dt_slots is strictly before now and cannot
        // overflow.
        CrankCatchupTarget::Target(last_market_slot + max_accrual_dt_slots)
    }

    /// The subset of MarketConfig fields that a partial crank catchup is
    /// allowed to choose between "before read" and "after read/sweep" values.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PartialCrankConfigFields {
        pub last_effective_price_e6: u64,
        pub last_hyperp_index_slot: u64,
        pub last_good_oracle_slot: u64,
        pub last_oracle_publish_time: i64,
        pub oracle_target_price_e6: u64,
        pub oracle_target_publish_time: i64,
        pub fee_sweep_cursor_word: u64,
        pub fee_sweep_cursor_bit: u64,
    }

    /// Partial crank catchup preserves fresh target/liveness data, the
    /// bounded effective/index step actually fed to the engine, and the
    /// fee-sweep cursor. The raw target may remain ahead of the effective
    /// engine price; user value-moving paths handle that lag separately.
    #[inline]
    pub fn partial_crank_config_fields_to_write(
        _before_read: PartialCrankConfigFields,
        after_read_and_sweep: PartialCrankConfigFields,
    ) -> PartialCrankConfigFields {
        PartialCrankConfigFields {
            last_effective_price_e6: after_read_and_sweep.last_effective_price_e6,
            last_hyperp_index_slot: after_read_and_sweep.last_hyperp_index_slot,
            last_good_oracle_slot: after_read_and_sweep.last_good_oracle_slot,
            last_oracle_publish_time: after_read_and_sweep.last_oracle_publish_time,
            oracle_target_price_e6: after_read_and_sweep.oracle_target_price_e6,
            oracle_target_publish_time: after_read_and_sweep.oracle_target_publish_time,
            fee_sweep_cursor_word: after_read_and_sweep.fee_sweep_cursor_word,
            fee_sweep_cursor_bit: after_read_and_sweep.fee_sweep_cursor_bit,
        }
    }

    /// True when the wrapper knows the raw oracle/Hyperp target still differs
    /// from the engine's last effective accrued price.
    #[inline]
    pub fn target_lag_pending(
        is_hyperp: bool,
        external_oracle_target_price_e6: u64,
        hyperp_target_price_e6: u64,
        engine_last_oracle_price: u64,
    ) -> bool {
        let target = if is_hyperp {
            hyperp_target_price_e6
        } else {
            external_oracle_target_price_e6
        };
        target != 0 && target != engine_last_oracle_price
    }

    /// True when the latest oracle read produced an effective engine price that
    /// still has not caught up to the raw target. TradeCpi uses this before CPI
    /// so a matcher is not invoked for a trade the wrapper must reject anyway.
    #[inline]
    pub fn target_lag_after_read(
        is_hyperp: bool,
        external_oracle_target_price_e6: u64,
        hyperp_target_price_e6: u64,
        effective_price_e6: u64,
    ) -> bool {
        let target = if is_hyperp {
            hyperp_target_price_e6
        } else {
            external_oracle_target_price_e6
        };
        target != 0 && target != effective_price_e6
    }

    /// Public extraction-sensitive/risk-increasing user operations are allowed
    /// only after the raw target has caught up to the engine's effective price.
    #[inline]
    pub fn user_value_op_allowed_after_accrual(
        is_hyperp: bool,
        external_oracle_target_price_e6: u64,
        hyperp_target_price_e6: u64,
        engine_last_oracle_price: u64,
    ) -> bool {
        !target_lag_pending(
            is_hyperp,
            external_oracle_target_price_e6,
            hyperp_target_price_e6,
            engine_last_oracle_price,
        )
    }

    /// Pre-CPI TradeCpi admission under raw-target/effective-price lag policy.
    #[inline]
    pub fn trade_cpi_allowed_after_oracle_read(
        is_hyperp: bool,
        external_oracle_target_price_e6: u64,
        hyperp_target_price_e6: u64,
        effective_price_e6: u64,
    ) -> bool {
        !target_lag_after_read(
            is_hyperp,
            external_oracle_target_price_e6,
            hyperp_target_price_e6,
            effective_price_e6,
        )
    }

    /// Account index must be inside both the compiled hard cap and the
    /// market's configured account capacity.
    #[inline]
    pub fn market_idx_within_capacity(idx: usize, market_max_accounts: u64) -> bool {
        idx < percolator::MAX_ACCOUNTS && (idx as u64) < market_max_accounts
    }

    /// Fee sync must never advance beyond the market boundary that has already
    /// been economically accrued. Live markets use last_market_slot; resolved
    /// markets use the immutable resolved_slot.
    #[inline]
    pub fn fee_sync_anchor_within_accrued_boundary(
        is_resolved: bool,
        anchor_slot: u64,
        last_market_slot: u64,
        resolved_slot: u64,
    ) -> bool {
        if is_resolved {
            anchor_slot <= resolved_slot
        } else {
            anchor_slot <= last_market_slot
        }
    }

    /// No-oracle instructions cannot accrue market price/funding state, so they
    /// may only sync fees up to the already-accrued market boundary. Returning
    /// None means there is no safe forward fee interval for this instruction.
    #[inline]
    pub fn no_oracle_fee_sync_anchor(
        wallclock_slot: u64,
        last_market_slot: u64,
        current_slot: u64,
        account_last_fee_slot: u64,
    ) -> Option<u64> {
        let anchor = if wallclock_slot < last_market_slot {
            wallclock_slot
        } else {
            last_market_slot
        };
        if anchor < current_slot || anchor < account_last_fee_slot {
            None
        } else {
            Some(anchor)
        }
    }

    /// Permissionless resolved-market force close is live iff the configured
    /// delay is nonzero and the elapsed slot distance from resolution meets it.
    /// Uses saturating_sub rather than `resolved_slot + delay`, so large slot
    /// values cannot make the close path unreachable through addition
    /// saturation.
    #[inline]
    pub fn force_close_delay_elapsed(
        clock_slot: u64,
        resolved_slot: u64,
        force_close_delay_slots: u64,
    ) -> bool {
        force_close_delay_slots != 0
            && clock_slot.saturating_sub(resolved_slot) >= force_close_delay_slots
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

    /// Pure matcher return fields.
    /// Mirrors matcher_abi::MatcherReturn for test and proof harnesses.
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
    /// This avoids logic duplication and keeps proofs tied to the real code.
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
    pub fn decide_trade_nocpi(user_auth_ok: bool, lp_auth_ok: bool) -> TradeNoCpiDecision {
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

    /// Decision for admin operations (UpdateAuthority, UpdateConfig, etc.).
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
    /// All Hyperp internal prices (hyperp_mark_e6, last_effective_price_e6)
    /// must be in engine-space. Apply this at every ingress point:
    /// InitMarket, PushHyperpMark, TradeCpi mark-update.
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

    /// Live insurance withdrawal health gate.
    ///
    /// Bounded operator withdrawals are only allowed while the senior residual
    /// is non-negative:
    ///
    /// ```text
    /// residual = vault - c_tot - insurance >= 0
    /// ```
    ///
    /// A withdrawal subtracts the same amount from `vault` and `insurance`, so
    /// it preserves this residual. The wrapper still requires the residual to
    /// be non-negative up front so operators cannot extract from an already
    /// deficit live market.
    #[inline]
    pub fn live_insurance_withdraw_residual_ok(vault: u128, c_tot: u128, insurance: u128) -> bool {
        match c_tot.checked_add(insurance) {
            Some(senior) => vault >= senior,
            None => false,
        }
    }

    /// Live limited insurance withdrawal is allowed only in a healthy live
    /// market: no active stress/h-max reconciliation envelope and non-negative
    /// senior residual.
    #[inline]
    pub fn live_insurance_withdraw_market_healthy(
        vault: u128,
        c_tot: u128,
        insurance: u128,
        stress_envelope_active: bool,
    ) -> bool {
        !stress_envelope_active && live_insurance_withdraw_residual_ok(vault, c_tot, insurance)
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

    /// Apply fee weighting to an already-computed EWMA alpha.
    #[inline]
    pub fn ewma_effective_alpha_bps(alpha_bps: u128, fee_paid: u64, mark_min_fee: u64) -> u128 {
        if mark_min_fee == 0 || fee_paid >= mark_min_fee {
            alpha_bps
        } else {
            alpha_bps * (fee_paid as u128) / (mark_min_fee as u128)
        }
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
            if mark_min_fee > 0 && fee_paid < mark_min_fee {
                return 0;
            }
            return price;
        }
        let dt = now_slot.saturating_sub(last_slot);
        if dt == 0 {
            return old;
        }
        if halflife_slots == 0 {
            return price;
        }
        // Zero fee with weighting enabled: no mark movement
        if fee_paid == 0 && mark_min_fee > 0 {
            return old;
        }

        let alpha_bps = (10_000u128 * dt as u128) / (dt as u128 + halflife_slots as u128);

        // Fee weighting: scale alpha by min(fee_paid/mark_min_fee, 1).
        // Trades below the fee threshold get proportionally reduced mark influence.
        // This makes wash trading cost-proportional: to move the mark like a
        // legitimate trade, the attacker must burn the same fee into insurance.
        let effective_alpha_bps = ewma_effective_alpha_bps(alpha_bps, fee_paid, mark_min_fee);

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
    /// Offset of bankruptcy_hmax_lock_active within RiskEngine (bool)
    const BANKRUPTCY_HMAX_LOCK_ACTIVE_OFF: usize =
        offset_of!(RiskEngine, bankruptcy_hmax_lock_active);
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
    /// With the currently pinned engine, the field types in the RiskEngine
    /// slab with invalid bit patterns are:
    ///   - SideMode (2 instances at side_mode_long / side_mode_short):
    ///     valid tag bytes 0 (Normal), 1 (DrainOnly), 2 (ResetPending).
    ///   - MarketMode (at market_mode): valid tag bytes 0 (Live),
    ///     1 (Resolved).
    ///   - bankruptcy_hmax_lock_active: valid bool bytes 0 or 1.
    /// No other field type in either RiskEngine or Account has invalid
    /// bit patterns: every other field is u64/u128/i64/i128/[u8; N]/
    /// wrapper-Pod (U128/I128) or fixed u8 — all-bits-valid types.
    ///
    /// If a future revision adds any new enum or bool field to the slab, the
    /// validation below must be extended before the cast can be considered
    /// sound.
    #[inline]
    fn validate_raw_engine_state_shape(data: &[u8]) -> Result<(), ProgramError> {
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
        let hmax = data[base + BANKRUPTCY_HMAX_LOCK_ACTIVE_OFF];
        if hmax > 1 {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(())
    }

    #[doc(hidden)]
    pub fn validate_raw_engine_bytes_for_test(data: &[u8]) -> Result<(), ProgramError> {
        validate_raw_engine_state_shape(data)
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
        // Validate invalid-bit-pattern fields before creating a reference.
        validate_raw_engine_state_shape(data)?;
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
        validate_raw_engine_state_shape(data)?;
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
        let mut infos: alloc::vec::Vec<AccountInfo<'a>> =
            alloc::vec::Vec::with_capacity(3 + tail.len());
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
    pub const FLAG_PARTIAL_OK: u32 = 2; // bit1: partial fill, including zero, allowed
    pub const FLAG_REJECTED: u32 = 4; // bit2: trade rejected by matcher

    /// Matcher return structure.
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
        if ret.exec_size.unsigned_abs() < req_size.unsigned_abs()
            && (ret.flags & FLAG_PARTIAL_OK) == 0
        {
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
        EngineCorruptState,
        /// Wrapper-level: the requested operation cannot safely advance market
        /// time. Caller must run KeeperCrank to commit account-touching market
        /// progress, then retry the original operation.
        CatchupRequired,
        /// Deposit rejected: post-deposit `c_tot` would exceed
        /// `tvl_insurance_cap_mult * insurance_fund.balance`.
        /// Only triggered when the admin has enabled the cap via UpdateConfig.
        DepositCapExceeded,
        /// `WithdrawInsuranceLimited` called within the configured
        /// `insurance_withdraw_cooldown_slots` window.
        InsuranceWithdrawCooldown,
        /// `WithdrawInsuranceLimited` amount exceeds
        /// `insurance_withdraw_max_bps * insurance_fund.balance / 10_000`
        /// (with a minimum floor of 10 units to avoid Zeno's-paradox lockout
        /// at small bps × small insurance).
        InsuranceWithdrawCapExceeded,
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
            /// Insurance withdrawal: max bps per withdrawal (0 = no live withdrawals)
            /// High bit enables deposits-only mode for WithdrawInsuranceLimited;
            /// low 15 bits store the bps value.
            insurance_withdraw_max_bps: u16,
            /// Insurance withdrawal: cooldown slots between withdrawals
            insurance_withdraw_cooldown_slots: u64,
            risk_params: RiskParams,
            /// Wrapper-charged new-account fee (base units). Charged by the
            /// wrapper at InitUser/InitLP: `fee_payment` is split into
            /// `new_account_fee` (routed to insurance) + remainder (credited
            /// as initial capital). Engine never sees this. 0 = disabled.
            new_account_fee: u128,
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
        /// Close the market slab and recover SOL to the admin-supplied
        /// destination. Requires: no active accounts, no vault funds,
        /// no insurance funds.
        CloseSlab,
        /// Update configurable funding parameters. Admin only.
        UpdateConfig {
            funding_horizon_slots: u64,
            funding_k_bps: u64,
            funding_max_premium_bps: i64,
            funding_max_e9_per_slot: i64,
            /// Admin-opt-in deposit cap multiplier. 0 disables the check.
            /// See `MarketConfig.tvl_insurance_cap_mult`.
            tvl_insurance_cap_mult: u16,
        },
        /// Push oracle price (oracle authority only).
        /// Stores the price for use by crank/trade operations.
        PushHyperpMark {
            price_e6: u64,
            timestamp: i64,
        },
        /// Resolve market: force-close all positions at admin oracle price, enter withdraw-only mode.
        /// Admin only. `mode` selects the §9.8 settlement arm explicitly:
        ///   0 = Ordinary   (live inputs; rejected when the oracle is stale)
        ///   1 = Degenerate (settle at engine.last_oracle_price, rate = 0;
        ///                    wrapper's canonical dead-oracle terminal path).
        ResolveMarket {
            mode: u8,
        },
        /// Withdraw insurance fund balance (UNBOUNDED). Gated by
        /// `header.insurance_authority`; requires market resolved +
        /// all accounts closed. For live, bounded extraction see
        /// `WithdrawInsuranceLimited` (tag 23). The two paths have
        /// structurally DISJOINT authority gates — this is what makes
        /// the bounded path's bps + cooldown bounds un-bypassable.
        WithdrawInsurance,
        /// Admin force-close an abandoned account after market resolution.
        /// Requires RESOLVED flag, zero position, admin signer.
        AdminForceCloseAccount {
            user_idx: u16,
        },
        /// BOUNDED live insurance withdrawal. Gated by
        /// `header.insurance_operator` (distinct from `insurance_authority`
        /// — the split is what makes the bounds meaningful). Per-call
        /// amount capped at `config.insurance_withdraw_max_bps *
        /// insurance_fund.balance / 10_000` with a floor of 10 units
        /// (anti-Zeno). Calls must be at least
        /// `config.insurance_withdraw_cooldown_slots` apart. Works on
        /// LIVE markets only; resolved markets use the unbounded tag 20.
        WithdrawInsuranceLimited {
            amount: u64,
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
        /// Requires RESOLVED + delay. Admin-only. Sends capital to any
        /// valid SPL token account whose token-owner matches the stored
        /// owner and whose mint matches `collateral_mint` — the caller
        /// chooses the destination (typically but not necessarily the
        /// canonical ATA). The wrapper enforces owner + mint equality
        /// via `verify_token_account`; it does NOT derive the
        /// Associated Token Address, so a non-ATA account owned by the
        /// stored owner is also accepted.
        ForceCloseResolved {
            user_idx: u16,
        },
        /// Scoped-authority update (tag 32).
        ///
        /// kind:
        ///   0 = AUTHORITY_ADMIN              (header.admin)
        ///   1 = AUTHORITY_HYPERP_MARK        (config.hyperp_authority)
        ///   2 = AUTHORITY_INSURANCE          (header.insurance_authority)
        ///   4 = AUTHORITY_INSURANCE_OPERATOR (header.insurance_operator)
        /// (kind = 3 / AUTHORITY_CLOSE was deleted; close authority merged
        /// into admin.)
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
        UpdateAuthority {
            kind: u8,
            new_pubkey: Pubkey,
        },
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
                                                                          // Insurance withdrawal limits (immutable after init)
                    let (risk_params, new_account_fee) = read_risk_params(&mut rest)?;
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
                        // Minimal payload: all extended fields use defaults.
                        // permissionless_resolve_stale_slots seeds to
                        // DEFAULT_PERMISSIONLESS_RESOLVE_STALE_SLOTS. With
                        // the production default of 0, non-Hyperp markets
                        // must use the extended tail to opt into a
                        // permissionless exit.
                        // force_close_delay_slots seeds to 1 slot (minimum
                        // liveness) to satisfy the init-time validation that
                        // permissionless_resolve > 0 ⇒ force_close > 0.
                        (
                            0u16,
                            0u64,
                            crate::constants::DEFAULT_PERMISSIONLESS_RESOLVE_STALE_SLOTS,
                            None,
                            None,
                            None,
                            None,
                            0u64,
                            1u64,
                        )
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
                        (
                            iwm,
                            iwc,
                            prs,
                            Some(fh),
                            Some(fk),
                            Some(fmp),
                            Some(fms),
                            mmf,
                            fcd,
                        )
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
                        insurance_withdraw_max_bps,
                        insurance_withdraw_cooldown_slots,
                        risk_params,
                        new_account_fee,
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
                    // work. We accept a fixed candidate cap that is larger
                    // than the execution budget so keepers can include the
                    // next cascade tail for wrapper coverage checks without
                    // asking the engine to process it in the same call.
                    const MAX_CANDIDATES: usize = crate::constants::MAX_KEEPER_CANDIDATES;
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
                        if !rest.is_empty() {
                            return Err(ProgramError::InvalidInstructionData);
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
                // Tag 7 (LiquidateAtOracle) retired. Liquidation is routed
                // through KeeperCrank candidates so every public liquidation
                // shares the same catchup, fee-sync, risk-buffer, and
                // round-robin account-touch path.
                7 => return Err(ProgramError::InvalidInstructionData),
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
                    // UpdateConfig — funding params + TVL:insurance cap
                    let funding_horizon_slots = read_u64(&mut rest)?;
                    let funding_k_bps = read_u64(&mut rest)?;
                    let funding_max_premium_bps = read_i64(&mut rest)?;
                    let funding_max_e9_per_slot = read_i64(&mut rest)?;
                    let tvl_insurance_cap_mult = read_u16(&mut rest)?;
                    Ok(Instruction::UpdateConfig {
                        funding_horizon_slots,
                        funding_k_bps,
                        funding_max_premium_bps,
                        funding_max_e9_per_slot,
                        tvl_insurance_cap_mult,
                    })
                }
                // Tag 16 (SetOracleAuthority) deleted — use
                // UpdateAuthority { kind: AUTHORITY_HYPERP_MARK } (tag 32).
                17 => {
                    // PushHyperpMark (Hyperp-only; handler rejects non-Hyperp)
                    let price_e6 = read_u64(&mut rest)?;
                    let timestamp = read_i64(&mut rest)?;
                    Ok(Instruction::PushHyperpMark {
                        price_e6,
                        timestamp,
                    })
                }
                19 => {
                    // ResolveMarket { mode: u8 }
                    //   mode = 0 → Ordinary, mode = 1 → Degenerate
                    let mode = read_u8(&mut rest)?;
                    if mode > 1 {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                    Ok(Instruction::ResolveMarket { mode })
                }
                20 => Ok(Instruction::WithdrawInsurance),
                21 => {
                    let user_idx = read_u16(&mut rest)?;
                    Ok(Instruction::AdminForceCloseAccount { user_idx })
                }
                // Tag 22 (SetInsuranceWithdrawPolicy) deleted — policy
                // was folded into config fields set at init/via
                // UpdateConfig, no separate setter instruction needed.
                //
                // Tag 23 (WithdrawInsuranceLimited) RESTORED with a
                // separate scoped authority (`header.insurance_operator`)
                // that cannot call the unbounded tag 20. The prior
                // deletion rationale was "same signer could bypass" —
                // that no longer holds now that the auth scopes are
                // structurally disjoint.
                23 => {
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::WithdrawInsuranceLimited { amount })
                }
                // Tag 25 (ReclaimEmptyAccount) retired. Empty/dust account
                // reclamation is now a KeeperCrank candidate-GC concern.
                25 => return Err(ProgramError::InvalidInstructionData),
                // Tag 26 (SettleAccount) retired. Account settlement is now
                // performed by KeeperCrank touch-only candidates or the
                // mandatory greedy round-robin sweep.
                26 => return Err(ProgramError::InvalidInstructionData),
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
                // Tag 31 (CatchupAccrue) retired. Public market-clock progress
                // is routed through KeeperCrank so exposed markets get an
                // account-touching revalidation/liquidation turn.
                31 => return Err(ProgramError::InvalidInstructionData),
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
        // Wrapper-charged new-account fee (base units). At InitUser/InitLP
        // the wrapper splits `fee_payment` into two parts: `new_account_fee`
        // is routed to the insurance fund; the remainder is credited as
        // account capital. Engine never sees the fee. Zero disables.
        let new_account_fee = read_u128(input)?;
        let h_max = read_u64(input)?;
        let _max_crank_staleness_slots = read_u64(input)?;
        let liquidation_fee_bps = read_u64(input)?;
        let liquidation_fee_cap = U128::new(read_u128(input)?);
        let resolve_price_deviation_bps = read_u64(input)?; // was _liquidation_buffer_bps
        let min_liquidation_abs = U128::new(read_u128(input)?);
        if input.len() < 40 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let min_nonzero_mm_req = read_u128(input)?;
        let min_nonzero_im_req = read_u128(input)?;
        // v12.19: per-slot price-move cap (standard bps, 100 = 1%).
        // Init-immutable per spec §1.4 solvency invariant.
        let max_price_move_bps_per_slot = read_u64(input)?;

        // §1.4 requires `cfg_max_price_move_bps_per_slot > 0`. Reject before
        // any engine call so the error surfaces as `InvalidConfigParam`
        // rather than an engine-side panic.
        if h_max == 0
            || h_max < h_min
            || h_max > crate::constants::MAX_PROFIT_MATURITY_SLOTS
            || max_price_move_bps_per_slot == 0
        {
            return Err(crate::error::PercolatorError::InvalidConfigParam.into());
        }

        let params = RiskParams {
            maintenance_margin_bps,
            initial_margin_bps,
            trading_fee_bps,
            max_accounts,
            liquidation_fee_bps,
            liquidation_fee_cap,
            min_liquidation_abs,
            min_nonzero_mm_req,
            min_nonzero_im_req,
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
            max_price_move_bps_per_slot,
        };
        Ok((params, new_account_fee))
    }
}

// 5. mod accounts (Pinocchio validation)
pub mod accounts {
    use crate::error::PercolatorError;
    use solana_program::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};

    /// Strict account-count check. Rejects if the caller passes more
    /// or fewer accounts than the handler expects.
    pub fn expect_len(accounts: &[AccountInfo], n: usize) -> Result<(), ProgramError> {
        if !crate::policy::len_ok(accounts.len(), n) {
            return Err(ProgramError::NotEnoughAccountKeys);
        }
        Ok(())
    }

    /// Variadic-tail check — used only by instructions with a
    /// documented tail forwarding convention (TradeCpi).
    pub fn expect_len_min(accounts: &[AccountInfo], n: usize) -> Result<(), ProgramError> {
        if !crate::policy::len_at_least(accounts.len(), n) {
            return Err(ProgramError::NotEnoughAccountKeys);
        }
        Ok(())
    }

    pub fn expect_signer(ai: &AccountInfo) -> Result<(), ProgramError> {
        // Signer check via policy helper
        if !crate::policy::signer_ok(ai.is_signer) {
            return Err(PercolatorError::ExpectedSigner.into());
        }
        Ok(())
    }

    pub fn expect_writable(ai: &AccountInfo) -> Result<(), ProgramError> {
        // Writable check via policy helper
        if !crate::policy::writable_ok(ai.is_writable) {
            return Err(PercolatorError::ExpectedWritable.into());
        }
        Ok(())
    }

    pub fn expect_key(ai: &AccountInfo, expected: &Pubkey) -> Result<(), ProgramError> {
        // Key check via policy helper
        if !crate::policy::pda_key_matches(expected.to_bytes(), ai.key.to_bytes()) {
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
        Pubkey::create_program_address(&[b"vault", slab_key.as_ref(), &[bump]], program_id)
            .map_err(|_| ProgramError::InvalidSeeds)
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
        /// Scoped authority: may execute `WithdrawInsuranceLimited`
        /// (tag 23) — a bounded live fee-extraction path enforcing
        /// `config.insurance_withdraw_max_bps` per withdrawal and
        /// `config.insurance_withdraw_cooldown_slots` between them.
        /// Structurally CANNOT call tag 20 (`WithdrawInsurance`),
        /// whose unbounded drain is gated on `insurance_authority`.
        /// The auth split is load-bearing: it's what makes the bounds
        /// un-bypassable. Burn to lock fee extraction. Independent of
        /// all other authorities; rotated via
        /// UpdateAuthority { kind=INSURANCE_OPERATOR }.
        pub insurance_operator: [u8; 32],
    }

    /// Offset of _reserved field in SlabHeader, derived from offset_of! for correctness.
    pub const RESERVED_OFF: usize = offset_of!(SlabHeader, _reserved);

    // Portable compile-time assertion that RESERVED_OFF is 48 (expected layout).
    // Subsequent authority fields (insurance_authority, insurance_operator) sit
    // after _reserved, so this offset is stable at 48.
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
        pub hyperp_authority: [u8; 32],
        /// Last price pushed by the Hyperp mark authority (e6, already
        /// invert+scale normalized to engine space).
        pub hyperp_mark_e6: u64,
        /// Most recently accepted external oracle observation timestamp
        /// (Pyth `publish_time` or Chainlink `timestamp`, in seconds).
        /// Kept for ABI compatibility and liveness accounting; the raw
        /// source target is stored separately in `oracle_target_*`, and
        /// `last_effective_price_e6` is the dt-capped price fed to the engine.
        pub last_oracle_publish_time: i64,

        /// Last effective oracle/index price in e6 format. External mode stores
        /// the dt-capped staircase value, not necessarily the raw oracle target;
        /// Hyperp mode stores the rate-limited index.
        pub last_effective_price_e6: u64,

        // ========================================
        // Insurance Withdrawal Limits (set at InitMarket, immutable)
        // ========================================
        /// Max bps of insurance fund withdrawable per withdrawal (1-10000).
        /// 0 = disabled (no live-market withdrawals allowed).
        pub insurance_withdraw_max_bps: u16,
        /// Admin-opt-in deposit cap: total user capital `c_tot` after a
        /// deposit must satisfy `c_tot_new <= tvl_insurance_cap_mult *
        /// insurance_fund.balance`. 0 disables the check (default).
        /// Tuned by admin via UpdateConfig; typical production values are
        /// 10–100 (mature perp DEXs run ~20× insurance coverage).
        pub tvl_insurance_cap_mult: u16,
        /// Optional bounded-withdrawal mode. 0 = legacy bounded withdrawals
        /// may withdraw live insurance subject to bps/cooldown. 1 = tag 23
        /// may withdraw only the remaining amount deposited through
        /// TopUpInsurance; fee/trading/liquidation growth stays behind.
        pub insurance_withdraw_deposits_only: u8,
        /// Padding for alignment (was [u8; 4]; shrunk when deposit-only mode
        /// claimed 1 byte of the former slot).
        pub _iw_padding: [u8; 3],
        /// Minimum slots between insurance withdrawals.
        pub insurance_withdraw_cooldown_slots: u64,
        /// Latest raw external oracle target in engine-space e6. Non-Hyperp
        /// only; Hyperp markets keep this equal to their initialized index.
        pub oracle_target_price_e6: u64,
        /// Timestamp of `oracle_target_price_e6`.
        pub oracle_target_publish_time: i64,
        pub last_hyperp_index_slot: u64,
        pub last_mark_push_slot: u128,
        /// Last slot when insurance was withdrawn (for live-market cooldown tracking).
        /// Uses a dedicated field to avoid overwriting oracle config fields.
        pub last_insurance_withdraw_slot: u64,
        /// Remaining TopUpInsurance principal withdrawable through tag 23
        /// when `insurance_withdraw_deposits_only == 1`. This tracks only
        /// explicit top-ups: it increases on TopUpInsurance and decreases on
        /// WithdrawInsuranceLimited. Insurance growth from fees/liquidations
        /// never increments this budget.
        pub insurance_withdraw_deposit_remaining: u64,

        // ========================================
        // Mark EWMA (trade-derived mark price for funding)
        // ========================================
        /// EWMA of execution prices (e6). Updated on every TradeCpi fill.
        pub mark_ewma_e6: u64,
        /// Slot when mark_ewma_e6 was last updated.
        pub mark_ewma_last_slot: u64,
        /// EWMA decay half-life in slots. 0 = last trade price directly.
        pub mark_ewma_halflife_slots: u64,
        /// `LastRestartSlot` sysvar reading captured at InitMarket, used to
        /// detect post-init cluster restarts. Once any observer sees
        /// `LastRestartSlot::get() > init_restart_slot`, the market is
        /// considered dead regardless of oracle-staleness configuration —
        /// `permissionless_stale_matured` returns true and resolution settles
        /// at `engine.last_oracle_price` (the pre-restart cached price).
        /// Occupies the slot previously reserved as u128-alignment padding
        /// for the `maintenance_fee_per_slot` u128 that follows.
        pub init_restart_slot: u64,

        // ========================================
        // Permissionless Resolution
        // ========================================
        /// Slots of oracle staleness required before anyone can resolve.
        /// 0 = disabled (admin-only resolution). Set at InitMarket, immutable.
        pub permissionless_resolve_stale_slots: u64,
        /// Slot of last successful external oracle read (non-Hyperp only).
        /// Authoritative liveness signal under the strict hard-timeout
        /// model: `clock.slot - last_good_oracle_slot >=
        /// permissionless_resolve_stale_slots` makes the market stale
        /// and eligible for ResolvePermissionless, and causes
        /// read_price_and_stamp to reject further price-taking ops.
        /// Seeded to clock.slot at InitMarket.
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
        /// Wrapper-charged new-account fee (base units). At InitUser/InitLP
        /// the wrapper requires `fee_payment >= new_account_fee`; the fee
        /// goes to `insurance_fund.balance`, the remainder becomes capital.
        /// 0 = disabled (full `fee_payment` goes to capital).
        pub new_account_fee: u128,
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
        use crate::constants::RISK_BUF_LEN;
        use crate::constants::RISK_BUF_OFF;
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
        use crate::constants::RISK_BUF_LEN;
        use crate::constants::RISK_BUF_OFF;
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
        price_ai: &AccountInfo,
        expected_feed_id: &[u8; 32],
        now_unix_ts: i64,
        max_staleness_secs: u64,
        conf_bps: u16,
    ) -> Result<(u64, i64), ProgramError> {
        use pythnet_sdk::messages::PriceFeedMessage;

        // Validate oracle owner.
        if *price_ai.owner != PYTH_RECEIVER_PROGRAM_ID {
            return Err(ProgramError::IllegalOwner);
        }

        let data = price_ai.try_borrow_data()?;
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
        price_ai: &AccountInfo,
        expected_feed_pubkey: &[u8; 32],
        now_unix_ts: i64,
        max_staleness_secs: u64,
    ) -> Result<(u64, i64), ProgramError> {
        // Validate oracle owner.
        if *price_ai.owner != CHAINLINK_OCR2_PROGRAM_ID {
            return Err(ProgramError::IllegalOwner);
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
        price_ai: &AccountInfo,
        expected_feed_id: &[u8; 32],
        now_unix_ts: i64,
        max_staleness_secs: u64,
        conf_bps: u16,
        invert: u8,
        unit_scale: u32,
    ) -> Result<(u64, i64), ProgramError> {
        // Detect oracle type by account owner and dispatch
        let (raw_price, publish_time) = if *price_ai.owner == PYTH_RECEIVER_PROGRAM_ID {
            read_pyth_price_e6(
                price_ai,
                expected_feed_id,
                now_unix_ts,
                max_staleness_secs,
                conf_bps,
            )?
        } else if *price_ai.owner == CHAINLINK_OCR2_PROGRAM_ID {
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
        config: &mut super::state::MarketConfig,
        price_ai: &AccountInfo,
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
        config: &mut super::state::MarketConfig,
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
        let effective = effective_price_from_target(
            anchor,
            target,
            max_change_bps,
            price_move_dt_slots,
            oi_any,
        );
        config.last_effective_price_e6 = effective;
        Ok(effective)
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
        config: &super::state::MarketConfig,
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
    pub fn cluster_restarted_since_init(config: &super::state::MarketConfig) -> bool {
        use solana_program::sysvar::last_restart_slot::LastRestartSlot;
        use solana_program::sysvar::Sysvar;
        match LastRestartSlot::get() {
            Ok(lrs) => restart_detected(config.init_restart_slot, lrs.last_restart_slot),
            Err(_) => false,
        }
    }

    #[cfg(feature = "kani")]
    #[inline]
    pub fn cluster_restarted_since_init(_config: &super::state::MarketConfig) -> bool {
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

    /// Spec §3 target/effective price law used by both external-oracle and
    /// Hyperp ingress. With exposed OI, the engine receives a capped staircase
    /// price from its current effective price toward the raw target. With no
    /// OI, the wrapper may adopt the raw target directly because no live
    /// position can lose equity from the jump.
    #[inline]
    pub fn effective_price_from_target(
        anchor: u64,
        target: u64,
        max_change_bps: u64,
        price_move_dt_slots: u64,
        oi_any: bool,
    ) -> u64 {
        if oi_any {
            clamp_toward_engine_dt(anchor, target, max_change_bps, price_move_dt_slots)
        } else {
            target
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
        config: &mut super::state::MarketConfig,
        a_oracle: &AccountInfo,
        max_change_bps: u64,
        oi_any: bool,
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
                config.hyperp_mark_e6
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
            let new_index = effective_price_from_target(
                anchor,
                mark,
                max_change_bps,
                price_move_dt_slots,
                oi_any,
            );

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
}

// 9. mod collateral
pub mod collateral {
    use solana_program::{account_info::AccountInfo, program_error::ProgramError};

    use solana_program::program::{invoke, invoke_signed};

    pub fn deposit<'a>(
        token_program: &AccountInfo<'a>,
        source: &AccountInfo<'a>,
        dest: &AccountInfo<'a>,
        authority: &AccountInfo<'a>,
        amount: u64,
    ) -> Result<(), ProgramError> {
        if amount == 0 {
            return Ok(());
        }
        let ix = spl_token::instruction::transfer(
            token_program.key,
            source.key,
            dest.key,
            authority.key,
            &[],
            amount,
        )?;
        invoke(
            &ix,
            &[
                source.clone(),
                dest.clone(),
                authority.clone(),
                token_program.clone(),
            ],
        )
    }

    pub fn withdraw<'a>(
        token_program: &AccountInfo<'a>,
        source: &AccountInfo<'a>,
        dest: &AccountInfo<'a>,
        authority: &AccountInfo<'a>,
        amount: u64,
        signer_seeds: &[&[&[u8]]],
    ) -> Result<(), ProgramError> {
        if amount == 0 {
            return Ok(());
        }
        let ix = spl_token::instruction::transfer(
            token_program.key,
            source.key,
            dest.key,
            authority.key,
            &[],
            amount,
        )?;
        invoke_signed(
            &ix,
            &[
                source.clone(),
                dest.clone(),
                authority.clone(),
                token_program.clone(),
            ],
            signer_seeds,
        )
    }
}

// 9. mod processor
pub mod processor {
    use crate::{
        accounts, collateral,
        constants::{
            DEFAULT_FUNDING_HORIZON_SLOTS, DEFAULT_FUNDING_K_BPS, DEFAULT_FUNDING_MAX_E9_PER_SLOT,
            DEFAULT_FUNDING_MAX_PREMIUM_BPS, DEFAULT_MARK_EWMA_HALFLIFE_SLOTS, MAGIC,
            MATCHER_CALL_LEN, MATCHER_CALL_TAG, MAX_MATCHER_TAIL_ACCOUNTS, SLAB_LEN,
        },
        error::{map_risk_error, PercolatorError},
        ix::Instruction,
        oracle,
        state::{self, MarketConfig, SlabHeader},
        zc,
    };
    use percolator::{RiskEngine, RiskError};

    // settle_and_close_resolved removed — replaced by engine
    // force_close_resolved_with_fee_not_atomic(), which handles K-pair PnL,
    // checked arithmetic, fee-after-loss ordering, and settlement internally.

    /// Read oracle price for non-Hyperp markets and stamp
    /// `last_good_oracle_slot`. Any Pyth/Chainlink parse error propagates
    /// unchanged — there is no authority fallback.
    ///
    /// STRICT HARD-TIMEOUT GATE: if the hard stale window has matured
    /// (clock.slot - last_good_oracle_slot >=
    /// permissionless_resolve_stale_slots), this function rejects with
    /// OracleStale even when a fresh external price is supplied. That
    /// prevents price-taking instructions (Trade, Withdraw, Crank,
    /// Settle, Convert, Catchup) from reviving a terminally dead market
    /// — they must route to ResolvePermissionless instead.
    fn read_price_and_stamp(
        config: &mut state::MarketConfig,
        a_oracle: &AccountInfo,
        clock_unix_ts: i64,
        clock_slot: u64,
        slab_data: &mut [u8],
    ) -> Result<u64, ProgramError> {
        if oracle::permissionless_stale_matured(config, clock_slot) {
            return Err(PercolatorError::OracleStale.into());
        }

        // Source the per-slot price-move cap from RiskParams (init-
        // immutable per spec §1.4 solvency envelope). Standard bps.
        let (p_last, max_change_bps, price_move_dt_slots, oi_any) = {
            let engine = zc::engine_ref(slab_data)?;
            (
                engine.last_oracle_price,
                engine.params.max_price_move_bps_per_slot,
                price_move_residual_dt(engine, clock_slot)?,
                engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0,
            )
        };

        let external = oracle::read_engine_price_e6(
            a_oracle,
            &config.index_feed_id,
            clock_unix_ts,
            config.max_staleness_secs,
            config.conf_filter_bps,
            config.invert,
            config.unit_scale,
        );
        // Snapshot the source-feed clock before the call so we can
        // tell whether THIS read advanced state. Stale/duplicate
        // observations get the cached price from
        // `clamp_external_price` without advancing the timestamp; we
        // must not stamp the liveness cursor on those — otherwise an
        // attacker can replay an old Pyth account to extend market
        // life past `permissionless_resolve_stale_slots`.
        let prev_publish_time = config.oracle_target_publish_time;
        let price = oracle::clamp_external_price(
            config,
            external,
            p_last,
            max_change_bps,
            price_move_dt_slots,
            oi_any,
        )?;
        if config.oracle_target_publish_time > prev_publish_time {
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
    /// DepositFeeCredits, InitUser, InitLP, TopUpInsurance, crank candidate
    /// GC) cannot advance `last_market_slot` (no price /
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
        let is_resolved = engine.is_resolved();
        let resolved_slot = if is_resolved {
            engine.resolved_context().1
        } else {
            0
        };
        if !crate::policy::fee_sync_anchor_within_accrued_boundary(
            is_resolved,
            now_slot,
            engine.last_market_slot,
            resolved_slot,
        ) {
            return Err(PercolatorError::CatchupRequired.into());
        }
        engine
            .sync_account_fee_to_slot_not_atomic(idx, now_slot, config.maintenance_fee_per_slot)
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
        check_idx(engine, idx)?;
        let Some(anchor) = crate::policy::no_oracle_fee_sync_anchor(
            wallclock_slot,
            engine.last_market_slot,
            engine.current_slot,
            engine.accounts[idx as usize].last_fee_slot,
        ) else {
            return Ok(());
        };
        engine
            .sync_account_fee_to_slot_not_atomic(idx, anchor, config.maintenance_fee_per_slot)
            .map_err(map_risk_error)
    }

    fn check_no_oracle_live_envelope(
        engine: &RiskEngine,
        wallclock_slot: u64,
    ) -> Result<(), ProgramError> {
        let gap = wallclock_slot
            .checked_sub(engine.last_market_slot)
            .ok_or(PercolatorError::EngineOverflow)?;
        let oi_any = engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0;
        if oi_any && gap > engine.params.max_accrual_dt_slots {
            return Err(PercolatorError::CatchupRequired.into());
        }
        Ok(())
    }

    fn price_move_residual_dt(
        engine: &RiskEngine,
        wallclock_slot: u64,
    ) -> Result<u64, ProgramError> {
        price_move_residual_dt_from_parts(
            engine.last_market_slot,
            engine.params.max_accrual_dt_slots,
            wallclock_slot,
        )
    }

    fn price_move_residual_dt_from_parts(
        last_market_slot: u64,
        max_dt: u64,
        wallclock_slot: u64,
    ) -> Result<u64, ProgramError> {
        let gap = wallclock_slot
            .checked_sub(last_market_slot)
            .ok_or(PercolatorError::EngineOverflow)?;
        if max_dt == 0 || gap <= max_dt {
            return Ok(gap);
        }
        let rem = gap % max_dt;
        Ok(if rem == 0 { max_dt } else { rem })
    }

    fn hyperp_target_price(config: &MarketConfig) -> u64 {
        if config.mark_ewma_e6 > 0 {
            config.mark_ewma_e6
        } else {
            config.hyperp_mark_e6
        }
    }

    fn oracle_target_pending(config: &MarketConfig, engine: &RiskEngine) -> bool {
        !crate::policy::user_value_op_allowed_after_accrual(
            oracle::is_hyperp_mode(config),
            config.oracle_target_price_e6,
            hyperp_target_price(config),
            engine.last_oracle_price,
        )
    }

    fn reject_any_target_lag(
        config: &MarketConfig,
        engine: &RiskEngine,
    ) -> Result<(), ProgramError> {
        if oracle_target_pending(config, engine) {
            return Err(PercolatorError::CatchupRequired.into());
        }
        Ok(())
    }

    fn target_lag_after_read(config: &MarketConfig, effective_price: u64) -> bool {
        !crate::policy::trade_cpi_allowed_after_oracle_read(
            oracle::is_hyperp_mode(config),
            config.oracle_target_price_e6,
            hyperp_target_price(config),
            effective_price,
        )
    }

    fn effective_pos_q_checked(engine: &RiskEngine, idx: usize) -> Result<i128, ProgramError> {
        engine
            .try_effective_pos_q(idx)
            .map_err(|_| PercolatorError::EngineCorruptState.into())
    }

    fn risk_notional_ceil(eff: i128, price: u64) -> u128 {
        percolator::wide_math::mul_div_ceil_u128(
            eff.unsigned_abs(),
            price as u128,
            percolator::POS_SCALE,
        )
    }

    fn current_trade_fee_paid_cap(
        size: i128,
        exec_price: u64,
        trading_fee_bps: u64,
    ) -> Result<u128, ProgramError> {
        if trading_fee_bps == 0 || size == 0 {
            return Ok(0);
        }
        let notional = percolator::wide_math::mul_div_floor_u128(
            size.unsigned_abs(),
            exec_price as u128,
            percolator::POS_SCALE,
        );
        if notional == 0 {
            return Ok(0);
        }
        let one_side_fee =
            percolator::wide_math::mul_div_ceil_u128(notional, trading_fee_bps as u128, 10_000);
        one_side_fee
            .checked_mul(2)
            .ok_or_else(|| PercolatorError::EngineOverflow.into())
    }

    fn reject_stuck_target_accrual(
        config: &MarketConfig,
        engine: &RiskEngine,
        now_slot: u64,
        price: u64,
    ) -> Result<(), ProgramError> {
        let dt = now_slot
            .checked_sub(engine.last_market_slot)
            .ok_or(PercolatorError::EngineOverflow)?;
        let oi_any = engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0;
        if dt > 0
            && oi_any
            && oracle_target_pending(config, engine)
            && price == engine.last_oracle_price
        {
            return Err(PercolatorError::CatchupRequired.into());
        }
        Ok(())
    }

    fn prepare_lazy_free_head(engine: &mut RiskEngine) -> Result<u16, ProgramError> {
        let max_accounts = core::cmp::min(
            engine.params.max_accounts as usize,
            percolator::MAX_ACCOUNTS,
        );
        let idx = engine.free_head;
        if idx == u16::MAX || (idx as usize) >= max_accounts || engine.is_used(idx as usize) {
            return Err(PercolatorError::EngineOverflow.into());
        }

        let i = idx as usize;
        let valid_head = engine.prev_free[i] == u16::MAX
            && (engine.next_free[i] == u16::MAX
                || ((engine.next_free[i] as usize) < max_accounts
                    && !engine.is_used(engine.next_free[i] as usize)
                    && engine.prev_free[engine.next_free[i] as usize] == idx));
        if !valid_head {
            if idx as u64 != engine.num_used_accounts as u64 {
                return Err(PercolatorError::EngineOverflow.into());
            }
            let next = if i + 1 < max_accounts {
                (i + 1) as u16
            } else {
                u16::MAX
            };
            engine.prev_free[i] = u16::MAX;
            engine.next_free[i] = next;
            if next != u16::MAX {
                engine.prev_free[next as usize] = idx;
            }
        }

        Ok(idx)
    }

    /// Maximum number of max_dt chunks the in-line catchup can advance per
    /// instruction. Bounded by CU budget — each `accrue_market_to` is cheap
    /// but not free. For gaps beyond this, callers must use KeeperCrank to
    /// commit account-touching progress before attempting a main operation.
    ///
    /// 20 × max_dt = 20 × 10 = 200 slots per single instruction. Larger
    /// gaps require multiple KeeperCrank calls — that's the design
    /// contract, not a misconfig.
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
    /// with `CatchupRequired` for user/admin operations. KeeperCrank uses a
    /// separate partial-progress wrapper so the crank itself can commit a
    /// bounded chunk instead of rolling back.
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
        // accrue_market_to rejects `total_dt > max_dt` when EITHER funding
        // or price movement would drain equity:
        //
        //   funding_active    = rate != 0 AND both OI sides live AND fund_px_last > 0
        //   price_move_active = P_last > 0 AND oracle_price != P_last AND any OI live
        //
        // Prior versions chunked only on `funding_active`. A zero-funding
        // market with live OI and a fresh oracle price different from
        // P_last would then skip catchup, and the caller's final
        // `accrue_market_to(now, fresh, rate)` would itself trip the
        // envelope (and/or the §5.5 step-9 per-slot price-move cap) and
        // make the market unrecoverable in-line.
        //
        // Do not invent intermediate oracle prices. If the clock gap is too
        // large, catch up time using stored P_last only, then let the final
        // real observation pass or fail the engine's dt-scaled price cap.
        let oi_any = engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0;
        let funding_active = funding_rate_e9 != 0
            && engine.oi_eff_long_q != 0
            && engine.oi_eff_short_q != 0
            && engine.fund_px_last > 0;
        let price_move_active =
            engine.last_oracle_price > 0 && price != engine.last_oracle_price && oi_any;
        if !funding_active && !price_move_active {
            // Neither accrual driver is active — the engine's envelope
            // predicate will permit a single-call jump. Caller's final
            // accrue_market_to handles it in one shot.
            return Ok(());
        }
        let cap_bps = engine.params.max_price_move_bps_per_slot;
        let mut chunks: u32 = 0;
        while now_slot.saturating_sub(engine.last_market_slot) > max_dt {
            if chunks >= CATCHUP_CHUNKS_MAX {
                // Silently returning Ok here would let the caller's
                // main accrue hit Overflow on the residual, rolling
                // back ALL catchup progress. Surface CatchupRequired
                // so the caller routes to KeeperCrank, which commits
                // account-touching progress before the main op is retried.
                return Err(PercolatorError::CatchupRequired.into());
            }
            let chunk_dt = max_dt;
            let step_slot = engine.last_market_slot.saturating_add(chunk_dt);
            let prev_price = engine.last_oracle_price;
            engine
                .accrue_market_to(step_slot, prev_price, funding_rate_e9)
                .map_err(map_risk_error)?;
            chunks = chunks.saturating_add(1);
        }
        if price_move_active {
            let remaining = now_slot.saturating_sub(engine.last_market_slot);
            let prev = engine.last_oracle_price;
            let abs_delta = if price >= prev {
                price - prev
            } else {
                prev - price
            };
            if abs_delta != 0 {
                if remaining == 0 {
                    return Err(PercolatorError::OracleInvalid.into());
                }
                let lhs = (abs_delta as u128).saturating_mul(10_000u128);
                let rhs = (cap_bps as u128)
                    .saturating_mul(remaining as u128)
                    .saturating_mul(prev as u128);
                if lhs > rhs {
                    return Err(PercolatorError::OracleInvalid.into());
                }
            }
        }
        Ok(())
    }

    fn oversized_catchup_target(
        engine: &RiskEngine,
        now_slot: u64,
        price: u64,
        funding_rate_e9: i128,
    ) -> Result<Option<u64>, ProgramError> {
        match crate::policy::oversized_crank_catchup_target(
            engine.params.max_accrual_dt_slots,
            1,
            engine.last_market_slot,
            now_slot,
            engine.last_oracle_price,
            price,
            funding_rate_e9,
            engine.oi_eff_long_q,
            engine.oi_eff_short_q,
            engine.fund_px_last,
        ) {
            crate::policy::CrankCatchupTarget::None => Ok(None),
            crate::policy::CrankCatchupTarget::Target(target) => Ok(Some(target)),
        }
    }

    fn keeper_partial_segment_price(
        engine: &RiskEngine,
        config: &MarketConfig,
        target_slot: u64,
    ) -> Result<u64, ProgramError> {
        let dt = target_slot
            .checked_sub(engine.last_market_slot)
            .ok_or(PercolatorError::EngineOverflow)?;
        let target = if oracle::is_hyperp_mode(config) {
            hyperp_target_price(config)
        } else {
            config.oracle_target_price_e6
        };
        if target == 0 {
            return Ok(engine.last_oracle_price);
        }
        Ok(crate::oracle::effective_price_from_target(
            engine.last_oracle_price,
            target,
            engine.params.max_price_move_bps_per_slot,
            dt,
            engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0,
        ))
    }

    fn partial_crank_config_fields(
        config: MarketConfig,
    ) -> crate::policy::PartialCrankConfigFields {
        crate::policy::PartialCrankConfigFields {
            last_effective_price_e6: config.last_effective_price_e6,
            last_hyperp_index_slot: config.last_hyperp_index_slot,
            last_good_oracle_slot: config.last_good_oracle_slot,
            last_oracle_publish_time: config.last_oracle_publish_time,
            oracle_target_price_e6: config.oracle_target_price_e6,
            oracle_target_publish_time: config.oracle_target_publish_time,
            fee_sweep_cursor_word: config.fee_sweep_cursor_word,
            fee_sweep_cursor_bit: config.fee_sweep_cursor_bit,
        }
    }

    fn apply_partial_crank_config_fields(
        config: &mut MarketConfig,
        fields: crate::policy::PartialCrankConfigFields,
    ) {
        config.last_effective_price_e6 = fields.last_effective_price_e6;
        config.last_hyperp_index_slot = fields.last_hyperp_index_slot;
        config.last_good_oracle_slot = fields.last_good_oracle_slot;
        config.last_oracle_publish_time = fields.last_oracle_publish_time;
        config.oracle_target_price_e6 = fields.oracle_target_price_e6;
        config.oracle_target_publish_time = fields.oracle_target_publish_time;
        config.fee_sweep_cursor_word = fields.fee_sweep_cursor_word;
        config.fee_sweep_cursor_bit = fields.fee_sweep_cursor_bit;
    }

    fn partial_crank_config_to_write(
        before_read: MarketConfig,
        after_read_and_sweep: MarketConfig,
    ) -> MarketConfig {
        let fields = crate::policy::partial_crank_config_fields_to_write(
            partial_crank_config_fields(before_read),
            partial_crank_config_fields(after_read_and_sweep),
        );
        let mut restored = before_read;
        apply_partial_crank_config_fields(&mut restored, fields);
        restored
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
    /// catchup is unsafe). Same-slot price replacement is allowed only
    /// for flat markets; live OI still requires elapsed slot budget.
    fn ensure_market_accrued_to_now(
        engine: &mut RiskEngine,
        now_slot: u64,
        price: u64,
        funding_rate_e9: i128,
    ) -> Result<(), ProgramError> {
        catchup_accrue(engine, now_slot, price, funding_rate_e9)?;
        let flat_same_slot_price_update = price > 0
            && now_slot == engine.last_market_slot
            && price != engine.last_oracle_price
            && engine.oi_eff_long_q == 0
            && engine.oi_eff_short_q == 0;
        if price > 0 && (now_slot > engine.last_market_slot || flat_same_slot_price_update) {
            engine
                .accrue_market_to(now_slot, price, funding_rate_e9)
                .map_err(map_risk_error)?;
        }
        Ok(())
    }

    fn reject_account_limited_market_progress(
        engine: &RiskEngine,
        now_slot: u64,
        price: u64,
        funding_rate_e9: i128,
    ) -> Result<(), ProgramError> {
        let dt_slots = now_slot.saturating_sub(engine.last_market_slot);
        if !crate::policy::account_limited_op_allows_accrual(
            engine.oi_eff_long_q,
            engine.oi_eff_short_q,
            engine.last_oracle_price,
            price,
            funding_rate_e9,
            engine.fund_px_last,
            dt_slots,
        ) {
            return Err(PercolatorError::CatchupRequired.into());
        }
        Ok(())
    }

    fn ensure_market_accrued_to_now_for_account_limited_op(
        engine: &mut RiskEngine,
        config: &MarketConfig,
        now_slot: u64,
        price: u64,
        funding_rate_e9: i128,
    ) -> Result<(), ProgramError> {
        reject_stuck_target_accrual(config, engine, now_slot, price)?;
        reject_account_limited_market_progress(engine, now_slot, price, funding_rate_e9)?;
        ensure_market_accrued_to_now(engine, now_slot, price, funding_rate_e9)
    }

    fn keeper_policy_can_liquidate(
        _engine: &RiskEngine,
        _idx: usize,
        _eff: i128,
        _price: u64,
        policy: Option<percolator::LiquidationPolicy>,
    ) -> bool {
        match policy {
            Some(percolator::LiquidationPolicy::FullClose) => true,
            // ExactPartial remains an executable engine hint, but it is not a
            // standalone wrapper coverage proof. The engine validates partials
            // after accrue + touch; this coverage pass runs before that state
            // transition. Treating a pre-accrual partial preview as coverage
            // would be a drift-prone false-positive path. Keepers that need
            // coverage for an unhealthy account include a FullClose fallback.
            Some(percolator::LiquidationPolicy::ExactPartial(_)) => false,
            None => false,
        }
    }

    fn compute_kf_pnl_delta_preview(
        abs_basis: u128,
        k_snap: i128,
        k_now: percolator::wide_math::I256,
        f_snap: i128,
        f_now: percolator::wide_math::I256,
        den: u128,
    ) -> Option<i128> {
        use percolator::wide_math::{div_rem_u256, I256, U256};

        if abs_basis == 0 {
            return Some(0);
        }
        let k_diff = k_now.checked_sub(I256::from_i128(k_snap))?;
        let k_scaled = if k_diff.is_zero() {
            I256::ZERO
        } else {
            if k_diff == I256::MIN {
                return None;
            }
            let neg = k_diff.is_negative();
            let prod = k_diff
                .abs_u256()
                .checked_mul(U256::from_u128(percolator::FUNDING_DEN))?;
            let pos = I256::from_u256_or_overflow(prod)?;
            if neg {
                I256::ZERO.checked_sub(pos)?
            } else {
                pos
            }
        };
        let f_diff = f_now.checked_sub(I256::from_i128(f_snap))?;
        let combined = k_scaled.checked_add(f_diff)?;
        if combined.is_zero() {
            return Some(0);
        }
        if combined == I256::MIN {
            return None;
        }
        let negative = combined.is_negative();
        let den_wide =
            U256::from_u128(den).checked_mul(U256::from_u128(percolator::FUNDING_DEN))?;
        let p = U256::from_u128(abs_basis).checked_mul(combined.abs_u256())?;
        let (q, rem) = div_rem_u256(p, den_wide);
        if negative {
            let mag = if !rem.is_zero() {
                q.checked_add(U256::ONE)?
            } else {
                q
            };
            let mag_u128 = mag.try_into_u128()?;
            if mag_u128 > i128::MAX as u128 {
                return None;
            }
            Some(-(mag_u128 as i128))
        } else {
            let q_u128 = q.try_into_u128()?;
            if q_u128 > i128::MAX as u128 {
                return None;
            }
            Some(q_u128 as i128)
        }
    }

    fn predicted_side_kf_after_accrual(
        engine: &RiskEngine,
        long_side: bool,
        now_slot: u64,
        price: u64,
        funding_rate_e9: i128,
    ) -> Option<(percolator::wide_math::I256, percolator::wide_math::I256)> {
        use percolator::wide_math::I256;

        if now_slot < engine.last_market_slot {
            return None;
        }
        let dt = now_slot.saturating_sub(engine.last_market_slot);
        let mut k_long = I256::from_i128(engine.adl_coeff_long);
        let mut k_short = I256::from_i128(engine.adl_coeff_short);
        let mut f_long = I256::from_i128(engine.f_long_num);
        let mut f_short = I256::from_i128(engine.f_short_num);

        let delta_p = (price as i128).checked_sub(engine.last_oracle_price as i128)?;
        if delta_p != 0 {
            let delta_p_wide = I256::from_i128(delta_p);
            let dk_long = I256::from_u128(engine.adl_mult_long).checked_mul_i256(delta_p_wide)?;
            let dk_short = I256::from_u128(engine.adl_mult_short).checked_mul_i256(delta_p_wide)?;
            k_long = k_long.checked_add(dk_long)?;
            k_short = k_short.checked_sub(dk_short)?;
        }

        if funding_rate_e9 != 0
            && dt > 0
            && engine.oi_eff_long_q != 0
            && engine.oi_eff_short_q != 0
            && engine.fund_px_last > 0
        {
            let fund_num_total = I256::from_u128(engine.fund_px_last as u128)
                .checked_mul_i256(I256::from_i128(funding_rate_e9))?
                .checked_mul_i256(I256::from_u128(dt as u128))?;
            let df_long = I256::from_u128(engine.adl_mult_long).checked_mul_i256(fund_num_total)?;
            let df_short =
                I256::from_u128(engine.adl_mult_short).checked_mul_i256(fund_num_total)?;
            f_long = f_long.checked_sub(df_long)?;
            f_short = f_short.checked_add(df_short)?;
        }

        if long_side {
            Some((k_long, f_long))
        } else {
            Some((k_short, f_short))
        }
    }

    fn predicted_account_equity_after_accrual(
        engine: &RiskEngine,
        idx: usize,
        eff: i128,
        now_slot: u64,
        price: u64,
        funding_rate_e9: i128,
    ) -> Result<Option<percolator::wide_math::I256>, ProgramError> {
        use percolator::wide_math::I256;

        if eff == 0 {
            return Ok(Some(I256::from_i128(0)));
        }
        let account = &engine.accounts[idx];
        let basis = account.position_basis_q;
        if basis == 0 || basis == i128::MIN || account.adl_a_basis == 0 || account.pnl == i128::MIN
        {
            return Ok(None);
        }
        let fc = account.fee_credits.get();
        if fc > 0 || fc == i128::MIN {
            return Ok(None);
        }

        let long_side = basis > 0;
        let epoch_side = if long_side {
            engine.adl_epoch_long
        } else {
            engine.adl_epoch_short
        };
        if account.adl_epoch_snap != epoch_side {
            return Ok(None);
        }

        let den = match account.adl_a_basis.checked_mul(percolator::POS_SCALE) {
            Some(v) => v,
            None => return Ok(None),
        };
        let (k_now, f_now) = match predicted_side_kf_after_accrual(
            engine,
            long_side,
            now_slot,
            price,
            funding_rate_e9,
        ) {
            Some(v) => v,
            None => return Ok(None),
        };
        let pnl_delta = match compute_kf_pnl_delta_preview(
            basis.unsigned_abs(),
            account.adl_k_snap,
            k_now,
            account.f_snap,
            f_now,
            den,
        ) {
            Some(v) => v,
            None => return Ok(None),
        };
        let pnl_after = match I256::from_i128(account.pnl).checked_add(I256::from_i128(pnl_delta)) {
            Some(v) => v,
            None => return Ok(None),
        };
        let fee_debt = if fc < 0 { fc.unsigned_abs() } else { 0 };
        match I256::from_u128(account.capital.get())
            .checked_add(pnl_after)
            .and_then(|v| v.checked_sub(I256::from_u128(fee_debt)))
        {
            Some(v) => Ok(Some(v)),
            None => Ok(None),
        }
    }

    fn account_provably_nonnegative_after_accrual(
        engine: &RiskEngine,
        idx: usize,
        eff: i128,
        now_slot: u64,
        price: u64,
        funding_rate_e9: i128,
    ) -> Result<bool, ProgramError> {
        use percolator::wide_math::I256;

        // Fast path for the overwhelmingly common zero-funding crank case.
        // A raw worst-case price-loss bound is enough to prevent insurance
        // leakage and is much cheaper than re-previewing K/F math for every
        // exposed account in a dense market. This intentionally ignores
        // direction and subtracts |eff| * |delta_price| for every account, so
        // it can only be more conservative than actual price PnL.
        if funding_rate_e9 == 0 {
            let account = &engine.accounts[idx];
            let fc = account.fee_credits.get();
            if account.position_basis_q == i128::MIN
                || account.adl_a_basis == 0
                || fc > 0
                || fc == i128::MIN
                || account.pnl == i128::MIN
            {
                return Ok(false);
            }
            let raw = engine.account_equity_maint_raw_wide(account);
            let delta = if price >= engine.last_oracle_price {
                price - engine.last_oracle_price
            } else {
                engine.last_oracle_price - price
            };
            let worst_loss = risk_notional_ceil(eff, delta);
            if let Some(after_worst_loss) = raw.checked_sub(I256::from_u128(worst_loss)) {
                return Ok(after_worst_loss >= I256::from_i128(0));
            }
        }

        if let Some(eq_after) = predicted_account_equity_after_accrual(
            engine,
            idx,
            eff,
            now_slot,
            price,
            funding_rate_e9,
        )? {
            if eq_after >= I256::from_i128(0) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn account_provably_above_maintenance_after_accrual(
        engine: &RiskEngine,
        idx: usize,
        eff: i128,
        now_slot: u64,
        price: u64,
        funding_rate_e9: i128,
    ) -> Result<bool, ProgramError> {
        use percolator::wide_math::I256;

        if eff == 0 {
            return Ok(true);
        }
        let notional = risk_notional_ceil(eff, price);
        let proportional_mm = percolator::wide_math::mul_div_floor_u128(
            notional,
            engine.params.maintenance_margin_bps as u128,
            10_000,
        );
        let mm_req = core::cmp::max(proportional_mm, engine.params.min_nonzero_mm_req);

        // Empty-candidate cranks are adversarial no-op attempts. They are
        // allowed to advance only while every uncovered exposed account remains
        // provably above maintenance after the step; otherwise a caller could
        // walk an evicted account below MM and liquidate it later after the
        // market clock has already absorbed the loss.
        if funding_rate_e9 == 0 {
            let account = &engine.accounts[idx];
            let fc = account.fee_credits.get();
            if account.position_basis_q == i128::MIN
                || account.adl_a_basis == 0
                || fc > 0
                || fc == i128::MIN
                || account.pnl == i128::MIN
            {
                return Ok(false);
            }
            let raw = engine.account_equity_maint_raw_wide(account);
            let delta = if price >= engine.last_oracle_price {
                price - engine.last_oracle_price
            } else {
                engine.last_oracle_price - price
            };
            let worst_loss = risk_notional_ceil(eff, delta);
            if let Some(after_worst_loss) = raw.checked_sub(I256::from_u128(worst_loss)) {
                return Ok(after_worst_loss > I256::from_u128(mm_req));
            }
        }

        if let Some(eq_after) = predicted_account_equity_after_accrual(
            engine,
            idx,
            eff,
            now_slot,
            price,
            funding_rate_e9,
        )? {
            if eq_after > I256::from_u128(mm_req) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn set_idx_bit(words: &mut [u64; (percolator::MAX_ACCOUNTS + 63) / 64], idx: usize) {
        words[idx >> 6] |= 1u64 << (idx & 63);
    }

    fn idx_bit(words: &[u64; (percolator::MAX_ACCOUNTS + 63) / 64], idx: usize) -> bool {
        (words[idx >> 6] & (1u64 << (idx & 63))) != 0
    }

    fn build_phase2_touch_map(
        engine: &RiskEngine,
        rr_touch_limit: u64,
        rr_scan_limit: u64,
        phase1_protective: &[u64; (percolator::MAX_ACCOUNTS + 63) / 64],
    ) -> [u64; (percolator::MAX_ACCOUNTS + 63) / 64] {
        let mut phase2_touched = [0u64; (percolator::MAX_ACCOUNTS + 63) / 64];
        if rr_touch_limit == 0 || rr_scan_limit == 0 {
            return phase2_touched;
        }
        let wrap_bound = core::cmp::min(
            engine.params.max_accounts as usize,
            percolator::MAX_ACCOUNTS,
        );
        if wrap_bound == 0 || engine.rr_cursor_position >= wrap_bound as u64 {
            return phase2_touched;
        }
        let mut i = engine.rr_cursor_position;
        let mut touched = 0u64;
        let scan_cap = core::cmp::min(rr_scan_limit, wrap_bound as u64);
        let mut inspected = 0u64;
        while i < wrap_bound as u64 && inspected < scan_cap && touched < rr_touch_limit {
            let idx = i as usize;
            if idx_used_in_market(engine, idx) && !idx_bit(phase1_protective, idx) {
                set_idx_bit(&mut phase2_touched, idx);
                touched = match touched.checked_add(1) {
                    Some(v) => v,
                    None => return phase2_touched,
                };
            }
            i += 1;
            inspected = match inspected.checked_add(1) {
                Some(v) => v,
                None => return phase2_touched,
            };
        }
        phase2_touched
    }

    fn build_keeper_liquidation_coverage_maps(
        engine: &RiskEngine,
        combined: &[(u16, Option<percolator::LiquidationPolicy>)],
        now_slot: u64,
        price: u64,
        funding_rate_e9: i128,
        phase1_protective: &mut [u64; (percolator::MAX_ACCOUNTS + 63) / 64],
    ) -> Result<u16, ProgramError> {
        let mut attempts: u16 = 0;
        let max_candidate_inspections = keeper_candidate_inspection_cap();
        let mut inspected: u16 = 0;
        let mut protective_attempts: u16 = 0;
        for &(candidate_idx, policy) in combined.iter() {
            if inspected >= max_candidate_inspections {
                break;
            }
            inspected = match inspected.checked_add(1) {
                Some(v) => v,
                None => break,
            };
            let candidate_usize = candidate_idx as usize;
            if !idx_used_in_market(engine, candidate_usize) {
                continue;
            }
            let eff = effective_pos_q_checked(engine, candidate_usize)?;
            let can_liquidate =
                keeper_policy_can_liquidate(engine, candidate_usize, eff, price, policy);
            let phase1_attempt = attempts < crate::constants::LIQ_BUDGET_PER_CRANK;
            attempts = attempts.saturating_add(1);
            let needs_protection = eff != 0
                && !account_provably_above_maintenance_after_accrual(
                    engine,
                    candidate_usize,
                    eff,
                    now_slot,
                    price,
                    funding_rate_e9,
                )?;
            if can_liquidate && phase1_attempt && needs_protection {
                set_idx_bit(phase1_protective, candidate_usize);
                protective_attempts = protective_attempts.saturating_add(1);
            }
        }
        Ok(protective_attempts)
    }

    fn keeper_candidate_inspection_cap() -> u16 {
        core::cmp::min(
            percolator::MAX_TOUCHED_PER_INSTRUCTION as u16,
            crate::constants::CANDIDATE_INSPECTION_BUDGET_PER_CRANK,
        )
    }

    fn check_keeper_crank_market_progress_coverage(
        engine: &RiskEngine,
        now_slot: u64,
        price: u64,
        funding_rate_e9: i128,
        combined: &[(u16, Option<percolator::LiquidationPolicy>)],
        rr_window_size: u64,
        rr_scan_limit: u64,
    ) -> Result<bool, ProgramError> {
        let dt_slots = now_slot.saturating_sub(engine.last_market_slot);
        if dt_slots == 0 && price == engine.last_oracle_price {
            return Ok(false);
        }
        if price == engine.last_oracle_price && funding_rate_e9 == 0 {
            // No price/funding state changes for any exposed account. Slot
            // catch-up and fee sweeping are already bounded independently, so
            // this path must not run an account-count-sized health scan.
            return Ok(false);
        }

        let defer_phase2 = false;
        let mut phase1_protective = [0u64; (percolator::MAX_ACCOUNTS + 63) / 64];
        let protective_attempts = build_keeper_liquidation_coverage_maps(
            engine,
            combined,
            now_slot,
            price,
            funding_rate_e9,
            &mut phase1_protective,
        )?;
        let dense_progress_floor = core::cmp::max(
            1,
            crate::constants::LIQ_BUDGET_PER_CRANK
                .saturating_sub(crate::constants::RISK_BUF_CAP as u16),
        );
        if protective_attempts >= dense_progress_floor {
            // Phase 1 is already doing bounded liquidation/touch-protection
            // work. Commit that progress and suppress Phase 2 instead of
            // scanning the whole market or requiring impossible all-account
            // coverage in the same instruction.
            return Ok(true);
        }
        if protective_attempts > 0 {
            // A candidate-covered cascade is doing real bounded protective
            // work, but not enough to saturate the dense-progress floor. Let
            // that work commit and suppress Phase 2 so the RR pass cannot
            // realize an uncovered tail loss in the same crank.
            return Ok(true);
        }
        // Phase 1 runs before Phase 2. Conservatively simulate
        // phase-1-protective FullClose candidates as no longer consuming RR
        // touch budget, because they may be closed before Phase 2 scans. This
        // can over-defer Phase 2, but it must not under-defer and let Phase 2
        // touch a deferred unhealthy tail account without its policy.
        //
        // Build the map once. Re-scanning the RR window for every listed tail
        // candidate is O(candidates * scan_window) and can exceed the SVM CU
        // envelope on dense candidate cascades.
        let phase2_touched =
            build_phase2_touch_map(engine, rr_window_size, rr_scan_limit, &phase1_protective);

        // Fixed-size no-op protection: if the bounded RR window would touch an
        // uncovered account that cannot be proven nonnegative after this
        // global step, reject the crank instead of silently advancing market
        // state. Candidate-covered cranks returned above, so reaching this
        // branch means there is no Phase 1 protective work to commit.
        for word_cursor in 0..phase2_touched.len() {
            let mut bits = phase2_touched[word_cursor];
            while bits != 0 {
                let bit = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                let idx = word_cursor * 64 + bit;
                if !idx_used_in_market(engine, idx) {
                    continue;
                }
                let eff = effective_pos_q_checked(engine, idx)?;
                if eff == 0 {
                    continue;
                }
                if idx_bit(&phase1_protective, idx) {
                    continue;
                }
                if !account_provably_nonnegative_after_accrual(
                    engine,
                    idx,
                    eff,
                    now_slot,
                    price,
                    funding_rate_e9,
                )? {
                    return Err(PercolatorError::CatchupRequired.into());
                }
            }
        }
        Ok(defer_phase2)
    }

    fn prepare_keeper_crank_market_progress(
        engine: &RiskEngine,
        config: &MarketConfig,
        now_slot: u64,
        price: u64,
        funding_rate_e9: i128,
        combined: &[(u16, Option<percolator::LiquidationPolicy>)],
    ) -> Result<(bool, u64, u64), ProgramError> {
        reject_stuck_target_accrual(config, engine, now_slot, price)?;
        let rr_window_size = if combined.is_empty() {
            crate::constants::RR_WINDOW_PER_CRANK
        } else {
            crate::constants::RR_WINDOW_WITH_CANDIDATES_PER_CRANK
        };
        let rr_scan_limit = core::cmp::min(
            crate::constants::RR_SCAN_LIMIT_PER_CRANK,
            engine.params.max_accounts,
        );
        let defer_phase2 = check_keeper_crank_market_progress_coverage(
            engine,
            now_slot,
            price,
            funding_rate_e9,
            combined,
            rr_window_size,
            rr_scan_limit,
        )?;
        Ok((defer_phase2, rr_window_size, rr_scan_limit))
    }

    fn sync_keeper_candidate_fees_after_crank(
        engine: &mut RiskEngine,
        combined: &[(u16, Option<percolator::LiquidationPolicy>)],
        now_slot: u64,
        maintenance_fee_per_slot: u128,
    ) -> Result<usize, ProgramError> {
        if maintenance_fee_per_slot == 0 {
            return Ok(0);
        }
        let cap = core::cmp::min(
            crate::constants::LIQ_BUDGET_PER_CRANK as usize,
            crate::constants::FEE_SWEEP_BUDGET,
        );
        let mut synced: [u16; crate::constants::LIQ_BUDGET_PER_CRANK as usize] =
            [u16::MAX; crate::constants::LIQ_BUDGET_PER_CRANK as usize];
        let mut synced_count = 0usize;
        let mut attempts = 0usize;
        let mut candidate_syncs = 0usize;
        for &(idx, _policy) in combined.iter() {
            if attempts >= cap || candidate_syncs >= crate::constants::FEE_SWEEP_BUDGET {
                break;
            }
            let i = idx as usize;
            if !idx_used_in_market(engine, i) {
                continue;
            }
            attempts += 1;
            let mut already = false;
            for j in 0..synced_count {
                if synced[j] == idx {
                    already = true;
                    break;
                }
            }
            if already {
                continue;
            }
            // Candidate fees run only after the engine's authoritative
            // candidate pass. That keeps recurring fees junior to any lazy
            // mark/funding loss realized by the touch, unlike the bitmap
            // sweep below which intentionally skips nonflat accounts.
            if engine.accounts[i].last_fee_slot >= now_slot {
                continue;
            }
            engine
                .sync_account_fee_to_slot_not_atomic(idx, now_slot, maintenance_fee_per_slot)
                .map_err(map_risk_error)?;
            synced[synced_count] = idx;
            synced_count += 1;
            candidate_syncs += 1;
        }
        Ok(candidate_syncs)
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
                if !idx_within_market_capacity(engine, idx) {
                    continue;
                }
                // Keeper fee sweeping is loss-senior: do not charge recurring
                // fees from exposed accounts before their lazy K/F/mark loss
                // has been settled by an engine touch. Nonflat accounts pay
                // fees through their own value-moving operation or after a
                // future fee-aware engine touch path; the bitmap sweep is for
                // flat accounts whose capital is not standing in front of
                // unsettled position loss.
                if effective_pos_q_checked(engine, idx)? != 0 {
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

                // Permissionless dust reclaim: fee accrual just charged
                // this account; if that drained capital to zero on a
                // flat account (no position, no PnL, no reserve, no
                // pending, no positive fee_credits), free the slot now.
                // Without this, an attacker could fill `max_accounts`
                // with dust and brick onboarding even when fees drain
                // capital, because slot reclamation would otherwise
                // require a targeted crank candidate.
                //
                // All six flat-clean predicates the engine's reclaim
                // checks are mirrored here so the call CANNOT hit an
                // `Undercollateralized` / `CorruptState` early return.
                // That lets us propagate any remaining error with `?`
                // rather than silently swallowing a `_not_atomic`
                // failure — per the engine contract, a failing
                // `_not_atomic` may have already mutated state and the
                // caller must abort the transaction. Envelope /
                // market-mode guards upstream (KeeperCrank's oracle
                // read + is_resolved gate + accrue_market_to) ensure
                // the remaining engine preconditions hold, so in
                // practice the `?` is unreachable — but if a future
                // engine change introduces a new precondition, we get
                // a transaction rollback instead of silent corruption.
                reclaim_flat_zero_account_if_eligible(engine, idx as u16, now_slot)?;
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

    #[inline]
    fn reclaim_flat_zero_account_if_eligible(
        engine: &mut RiskEngine,
        idx: u16,
        now_slot: u64,
    ) -> Result<(), ProgramError> {
        if !idx_used_in_market(engine, idx as usize) {
            return Ok(());
        }
        let acc = &engine.accounts[idx as usize];
        let fee_credits = acc.fee_credits.get();
        if acc.capital.is_zero()
            && acc.position_basis_q == 0
            && acc.pnl == 0
            && acc.reserved_pnl == 0
            && acc.sched_present == 0
            && acc.pending_present == 0
            && fee_credits <= 0
        {
            if fee_credits == i128::MIN {
                return Err(PercolatorError::EngineCorruptState.into());
            }
            engine
                .reclaim_empty_account_not_atomic(idx, now_slot)
                .map_err(map_risk_error)?;
        }
        Ok(())
    }

    fn compute_current_funding_rate_e9(config: &MarketConfig) -> Result<i128, ProgramError> {
        crate::policy::funding_rate_e9_from_mark_index(
            config.mark_ewma_e6,
            config.last_effective_price_e6,
            config.funding_horizon_slots,
            config.funding_k_bps,
            config.funding_max_premium_bps,
            config.funding_max_e9_per_slot,
        )
        .ok_or(PercolatorError::InvalidConfigParam.into())
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
            engine.sync_account_fee_to_slot_not_atomic(a, now_slot, maintenance_fee_per_slot)?;
            engine.sync_account_fee_to_slot_not_atomic(b, now_slot, maintenance_fee_per_slot)?;
        }
        let admit_threshold = Some(engine.params.maintenance_margin_bps as u128);
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
            admit_threshold,
        )
    }

    use solana_program::instruction::{AccountMeta, Instruction as SolInstruction};
    #[cfg(feature = "cu-audit")]
    use solana_program::log::sol_log_compute_units;
    use solana_program::{
        account_info::AccountInfo,
        entrypoint::ProgramResult,
        program_error::ProgramError,
        program_pack::Pack,
        pubkey::Pubkey,
        sysvar::{clock::Clock, Sysvar},
    };
    #[cfg(feature = "cu-audit")]
    use solana_program::{log::sol_log_64, msg};

    fn slab_shape_guard(
        program_id: &Pubkey,
        slab: &AccountInfo,
        data: &[u8],
    ) -> Result<(), ProgramError> {
        // Slab shape validation via policy helper
        let shape = crate::policy::SlabShape {
            owned_by_program: slab.owner == program_id,
            correct_len: data.len() == SLAB_LEN,
        };
        if !crate::policy::slab_shape_ok(shape) {
            if slab.owner != program_id {
                return Err(ProgramError::IllegalOwner);
            }
            solana_program::log::sol_log_64(SLAB_LEN as u64, data.len() as u64, 0, 0, 0);
            return Err(PercolatorError::InvalidSlabLen.into());
        }
        Ok(())
    }

    fn slab_guard(
        program_id: &Pubkey,
        slab: &AccountInfo,
        data: &[u8],
    ) -> Result<(), ProgramError> {
        slab_shape_guard(program_id, slab, data)?;
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
    /// Admin authorization via policy helper
    fn require_admin(header_admin: [u8; 32], signer: &Pubkey) -> Result<(), ProgramError> {
        if !crate::policy::admin_ok(header_admin, signer.to_bytes()) {
            return Err(PercolatorError::EngineUnauthorized.into());
        }
        Ok(())
    }

    #[inline]
    fn idx_within_market_capacity(engine: &RiskEngine, idx: usize) -> bool {
        crate::policy::market_idx_within_capacity(idx, engine.params.max_accounts)
    }

    #[inline]
    fn idx_used_in_market(engine: &RiskEngine, idx: usize) -> bool {
        idx_within_market_capacity(engine, idx) && engine.is_used(idx)
    }

    fn check_idx(engine: &RiskEngine, idx: u16) -> Result<(), ProgramError> {
        if !idx_used_in_market(engine, idx as usize) {
            return Err(PercolatorError::EngineAccountNotFound.into());
        }
        Ok(())
    }

    #[inline]
    fn insurance_withdraw_deposits_only(config: &MarketConfig) -> Result<bool, ProgramError> {
        match config.insurance_withdraw_deposits_only {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(PercolatorError::InvalidConfigParam.into()),
        }
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
        Ok(())
    }

    /// Verify the token program account is valid.
    fn verify_token_program(a_token: &AccountInfo) -> Result<(), ProgramError> {
        if *a_token.key != spl_token::ID {
            return Err(PercolatorError::InvalidTokenProgram.into());
        }
        if !a_token.executable {
            return Err(PercolatorError::InvalidTokenProgram.into());
        }
        Ok(())
    }

    // UpdateAuthority kind constants. Keep in a single place so the
    // decoder, handler, and any future on-chain schema references agree.
    pub const AUTHORITY_ADMIN: u8 = 0;
    pub const AUTHORITY_HYPERP_MARK: u8 = 1;
    pub const AUTHORITY_INSURANCE: u8 = 2;
    // Tag 3 (AUTHORITY_CLOSE) deleted — close_authority merged into admin.
    /// Scoped live-withdrawal authority. Cannot call tag 20
    /// (unbounded), only tag 23 (`WithdrawInsuranceLimited`).
    pub const AUTHORITY_INSURANCE_OPERATOR: u8 = 4;

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
            let clock_gate = Clock::get().map_err(|_| ProgramError::UnsupportedSysvar)?;
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
            AUTHORITY_HYPERP_MARK => config.hyperp_authority,
            AUTHORITY_INSURANCE => header.insurance_authority,
            AUTHORITY_INSURANCE_OPERATOR => header.insurance_operator,
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
            AUTHORITY_HYPERP_MARK => {
                // AUTHORITY_HYPERP_MARK is Hyperp-only — it's the mark-push
                // signer for `PushHyperpMark`. Non-Hyperp markets have
                // no authority role.
                if !oracle::is_hyperp_mode(&config) {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                // Burning is only safe once the EWMA is bootstrapped
                // (otherwise the mark source is gone and no settlement
                // path remains).
                if is_burn && config.mark_ewma_e6 == 0 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
            }
            AUTHORITY_INSURANCE | AUTHORITY_INSURANCE_OPERATOR => {
                // No per-kind invariants. Burning is a legitimate
                // no-rug configuration; setting to any pubkey is a
                // normal delegation. The insurance_operator kind is
                // structurally prevented from calling tag 20 because
                // the `require_admin(header.insurance_authority, ...)`
                // check in WithdrawInsurance looks at a different
                // field — auth scopes are disjoint.
            }
            _ => unreachable!(),
        }

        // Commit the assignment.
        match kind {
            AUTHORITY_ADMIN => {
                header.admin = new_bytes;
                state::write_header(&mut data, &header);
            }
            AUTHORITY_HYPERP_MARK => {
                config.hyperp_authority = new_bytes;
                state::write_config(&mut data, &config);
            }
            AUTHORITY_INSURANCE => {
                header.insurance_authority = new_bytes;
                state::write_header(&mut data, &header);
            }
            AUTHORITY_INSURANCE_OPERATOR => {
                header.insurance_operator = new_bytes;
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
                insurance_withdraw_max_bps,
                insurance_withdraw_cooldown_slots,
                risk_params,
                new_account_fee,
                permissionless_resolve_stale_slots,
                funding_horizon_slots: custom_funding_horizon,
                funding_k_bps: custom_funding_k,
                funding_max_premium_bps: custom_max_premium,
                funding_max_e9_per_slot: custom_max_per_slot,
                mark_min_fee,
                force_close_delay_slots,
            } => {
                accounts::expect_len(accounts, 6)?;
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
                // Confidence filter: require a nonzero, operationally sane
                // range. Disabling confidence checks is too sharp for public
                // deployments; wide confidence bands are equivalent to
                // accepting a low-quality oracle.
                if conf_filter_bps < crate::constants::MIN_CONF_FILTER_BPS
                    || conf_filter_bps > crate::constants::MAX_CONF_FILTER_BPS
                {
                    return Err(ProgramError::InvalidInstructionData);
                }
                // Validate unit_scale: reject huge values that make most deposits credit 0 units
                if !crate::policy::init_market_scale_ok(unit_scale) {
                    return Err(ProgramError::InvalidInstructionData);
                }
                // Margin params: initial >= maintenance, both non-zero, initial <= 100%
                if risk_params.initial_margin_bps == 0 || risk_params.maintenance_margin_bps == 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                if risk_params.initial_margin_bps > 10_000 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                if risk_params.initial_margin_bps < risk_params.maintenance_margin_bps {
                    return Err(ProgramError::InvalidInstructionData);
                }
                if risk_params.h_max == 0
                    || risk_params.h_max < risk_params.h_min
                    || risk_params.h_max > crate::constants::MAX_PROFIT_MATURITY_SLOTS
                {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                let insurance_withdraw_deposits_only = (insurance_withdraw_max_bps
                    & crate::constants::INSURANCE_WITHDRAW_DEPOSITS_ONLY_FLAG)
                    != 0;
                let insurance_withdraw_max_bps =
                    insurance_withdraw_max_bps & crate::constants::INSURANCE_WITHDRAW_MAX_BPS_MASK;
                // insurance_withdraw_max_bps is a percentage (0..=10_000);
                // the top wire bit is reserved for deposit-only mode.
                if insurance_withdraw_max_bps > 10_000 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                // If live withdrawals are enabled, require an explicit cooldown
                // (0 would fall through to DEFAULT which may surprise the admin).
                if insurance_withdraw_max_bps > 0 && insurance_withdraw_cooldown_slots == 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }

                // max_staleness_secs: reject 0 (would brick oracle reads) and
                // reject long stale windows that make oracle liveness a
                // deployer-controlled footgun.
                if max_staleness_secs == 0
                    || max_staleness_secs > crate::constants::MAX_ORACLE_STALENESS_SECS
                {
                    return Err(ProgramError::InvalidInstructionData);
                }

                // Hyperp mode validation: if index_feed_id is all zeros, require initial_mark_price_e6
                let is_hyperp = index_feed_id == [0u8; 32];
                if is_hyperp && initial_mark_price_e6 == 0 {
                    // Hyperp mode requires a non-zero initial mark price
                    return Err(ProgramError::InvalidInstructionData);
                }

                // Hyperp + h_min=0 is a print-money configuration. In Hyperp
                // mode the mark EWMA is walked by trade fills (no external
                // oracle pinning it), and MTM reads the EWMA whenever it's
                // non-zero. With h_min=0 the admission lane releases fresh
                // positive PnL with horizon zero, so a self-matching attacker
                // can convert EWMA walk → realized PnL → withdrawable
                // capital inside a single slot. The warmup horizon is the
                // only thing that lets a third-party crank revalidate
                // before extraction; deleting it removes the defense.
                // h_min is init-immutable (UpdateConfig wire format doesn't
                // carry it), so this gate at init is sufficient.
                if is_hyperp && risk_params.h_min == 0 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }

                // Normalize initial mark price to engine-space (invert + scale).
                // All Hyperp internal prices must be in engine-space.
                let initial_mark_price_e6 = if is_hyperp {
                    let p =
                        crate::policy::to_engine_price(initial_mark_price_e6, invert, unit_scale)
                            .ok_or(PercolatorError::OracleInvalid)?;
                    // Enforce MAX_ORACLE_PRICE at genesis — same invariant as runtime ingress
                    if p > percolator::MAX_ORACLE_PRICE {
                        return Err(PercolatorError::OracleInvalid.into());
                    }
                    p
                } else {
                    initial_mark_price_e6
                };

                // Validate new_account_fee: 0 ≤ fee ≤ MAX_VAULT_TVL.
                if new_account_fee > percolator::MAX_VAULT_TVL {
                    return Err(ProgramError::InvalidInstructionData);
                }
                // Scale alignment: `new_account_fee` is paid in BASE units
                // and split out of `fee_payment` at InitUser/InitLP. The
                // wrapper rejects misaligned `fee_payment` (the outer
                // dust check), but the SPLIT into (fee, capital) can
                // still produce dust on each side if `new_account_fee`
                // itself isn't aligned to `unit_scale`. That dust is
                // silently discarded into the vault by the units-conversion.
                // Reject the misconfig at init so admins can't ship a
                // market that leaks per-account dust to the vault.
                if unit_scale > 0 && new_account_fee % (unit_scale as u128) != 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }

                // Cheap wrapper policy checks happen here; exact RiskParams
                // validation is left to engine.init_in_place so InitMarket
                // does not run the heavy envelope verifier twice.
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
                // Permissionless resolution has an independent product
                // horizon. It is intentionally NOT capped by
                // max_accrual_dt_slots: dead-oracle settlement uses the
                // engine's Degenerate path at P_last/rate=0, while live
                // markets use KeeperCrank for explicit bounded progress.
                if permissionless_resolve_stale_slots > 0
                    && permissionless_resolve_stale_slots
                        > crate::constants::MAX_PERMISSIONLESS_RESOLVE_STALE_SLOTS
                {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }

                // Non-Hyperp resolvability invariant: a non-Hyperp market
                // with `permissionless_resolve_stale_slots == 0` has only
                // the admin-ResolveMarket path. If the admin is later burned
                // (UpdateAuthority → Pubkey::zero()), the market is
                // permanently un-resolvable — positions and insurance would
                // be stranded. Reject this combo outright so clients can't
                // deploy a bricked-on-burn market. Hyperp markets are
                // exempt: they resolve from the stored `hyperp_mark_e6`
                // without a live oracle read.
                if !is_hyperp && permissionless_resolve_stale_slots == 0 {
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

                // F2: Hyperp liveness spoof defense. When a Hyperp
                // market enables permissionless resolution, the ONLY
                // hard-timeout liveness signal is `last_mark_push_slot`,
                // which advances on any "full-weight" trade. With
                // `mark_min_fee == 0`, every trade is full-weight —
                // a permissionless attacker with their own matcher can
                // self-trade every slot to keep the market "live"
                // indefinitely, blocking ResolvePermissionless. Require
                // a nonzero threshold so cheap self-trades can't refresh
                // liveness. Non-perm-resolve Hyperp markets (admin-only
                // resolve) don't have this bricking vector.
                let is_hyperp_init = index_feed_id == [0u8; 32];
                if is_hyperp_init && permissionless_resolve_stale_slots > 0 && mark_min_fee == 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }

                // Public account materialization must be economically costly
                // on every market. Resolution mode does not protect the live
                // account table from zero-cost slot exhaustion.
                if new_account_fee == 0 && maintenance_fee_per_slot == 0 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }

                #[cfg(debug_assertions)]
                {
                    if core::mem::size_of::<MarketConfig>() != crate::constants::CONFIG_LEN {
                        return Err(ProgramError::InvalidAccountData);
                    }
                }

                let mut data = state::slab_data_mut(a_slab)?;
                slab_shape_guard(program_id, a_slab, &data)?;

                // Check magic BEFORE any unsafe cast — raw bytes may contain
                // invalid enum discriminants that would be UB if cast to RiskEngine.
                let header = state::read_header(&data);
                if header.magic == MAGIC {
                    return Err(PercolatorError::AlreadyInitialized.into());
                }
                // Canonicalize the entire slab before the first RiskEngine
                // reference. An uninitialized System account is normally
                // zero-filled, but `magic != MAGIC` does not prove stale bytes
                // in the engine/config/cache regions are valid or canonical.
                for b in data.iter_mut() {
                    *b = 0;
                }

                let (auth, bump) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault_empty(a_vault, &auth, a_mint.key, a_vault.key)?;

                // Initialize engine in-place (zero-copy) to avoid stack overflow.
                let a_clock = &accounts[4];
                let a_oracle = &accounts[5];
                let clock = Clock::from_account_info(a_clock)?;
                // Engine requires init_oracle_price > 0 (asserted in new_with_market).
                // Hyperp: use the admin-chosen initial mark price.
                // Non-Hyperp: REQUIRE a successful oracle read at init. The engine's
                //   last_oracle_price must be a real economic value, not a positive
                //   sentinel overloaded to mean "uninitialized" — per spec goal 38
                //   (no valid positive price may encode "no price yet"). If the
                //   oracle is unavailable at init time the admin must retry when
                //   the feed is live; there is no sentinel path.
                let (init_price, init_publish_time) = if is_hyperp {
                    (initial_mark_price_e6, 0i64)
                } else {
                    // Read the external oracle NOW; propagate any error (stale,
                    // wrong feed, malformed). Success seeds engine.last_oracle_price
                    // with a real price and lets us mark the oracle-initialized
                    // flag unconditionally — no FLAG_ORACLE_INITIALIZED gating
                    // needed for engine reads after this point. Capture the
                    // observation's `publish_time` as the monotonicity baseline
                    // so subsequent reads can't rewind below the genesis point.
                    let (fresh, publish_time) = oracle::read_engine_price_e6(
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
                    (fresh, publish_time)
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
                        || p.min_nonzero_im_req > percolator::MAX_VAULT_TVL
                    {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                    if p.min_liquidation_abs.get() > p.liquidation_fee_cap.get()
                        || p.liquidation_fee_cap.get() > percolator::MAX_PROTOCOL_FEE_ABS
                    {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                    if p.h_max == 0
                        || p.h_max < p.h_min
                        || p.h_max > crate::constants::MAX_PROFIT_MATURITY_SLOTS
                    {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                    // Settlement deviation band: 0 <= bps <= MAX per spec
                    // v12.19.6. Zero means "no deviation tolerance" — the
                    // authority settlement path admits only exact P_last.
                    if p.resolve_price_deviation_bps > percolator::MAX_RESOLVE_PRICE_DEVIATION_BPS {
                        return Err(ProgramError::InvalidInstructionData);
                    }
                }

                let engine = zc::engine_mut(&mut data)?;
                engine
                    .init_in_place(risk_params, clock.slot, init_price)
                    .map_err(map_risk_error)?;

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
                    funding_horizon_slots: custom_funding_horizon
                        .unwrap_or(DEFAULT_FUNDING_HORIZON_SLOTS),
                    funding_k_bps: custom_funding_k.unwrap_or(DEFAULT_FUNDING_K_BPS),
                    funding_max_premium_bps: custom_max_premium
                        .unwrap_or(DEFAULT_FUNDING_MAX_PREMIUM_BPS),
                    funding_max_e9_per_slot: custom_max_per_slot
                        .unwrap_or(DEFAULT_FUNDING_MAX_E9_PER_SLOT),
                    // Oracle authority is Hyperp-only: it signs the
                    // admin-pushed mark (PushHyperpMark). Non-Hyperp
                    // markets price exclusively off Pyth/Chainlink with
                    // no authority fallback — any parse error freezes
                    // the market until `permissionless_resolve_stale_
                    // slots` matures and settles at the cached
                    // `engine.last_oracle_price` via the Degenerate arm.
                    // Set to zero for non-Hyperp.
                    hyperp_authority: if is_hyperp {
                        a_admin.key.to_bytes()
                    } else {
                        [0u8; 32]
                    },
                    hyperp_mark_e6: if is_hyperp { initial_mark_price_e6 } else { 0 },
                    last_oracle_publish_time: init_publish_time,
                    // Seed last_effective_price_e6 with the genesis reading so the
                    // circuit-breaker baseline is real from genesis too (not 0, which
                    // disables the breaker on first oracle read). For non-Hyperp we
                    // just read init_price from the feed above, so reuse it.
                    last_effective_price_e6: if is_hyperp {
                        initial_mark_price_e6
                    } else {
                        init_price
                    },
                    // Insurance withdrawal limits (immutable after init)
                    insurance_withdraw_max_bps,
                    tvl_insurance_cap_mult: 0, // disabled at init; admin opts in via UpdateConfig
                    insurance_withdraw_deposits_only: insurance_withdraw_deposits_only as u8,
                    _iw_padding: [0u8; 3],
                    insurance_withdraw_cooldown_slots,
                    oracle_target_price_e6: init_price,
                    oracle_target_publish_time: init_publish_time,
                    last_hyperp_index_slot: if is_hyperp { clock.slot } else { 0 },
                    // Hyperp: stamp init slot so stale check works from genesis.
                    // Non-Hyperp: 0 (no mark push concept).
                    last_mark_push_slot: if is_hyperp { clock.slot as u128 } else { 0 },
                    last_insurance_withdraw_slot: 0,
                    insurance_withdraw_deposit_remaining: 0,
                    // Mark EWMA: Hyperp bootstraps from initial mark, non-Hyperp from first trade
                    mark_ewma_e6: if is_hyperp { initial_mark_price_e6 } else { 0 },
                    mark_ewma_last_slot: if is_hyperp { clock.slot } else { 0 },
                    mark_ewma_halflife_slots: DEFAULT_MARK_EWMA_HALFLIFE_SLOTS,
                    init_restart_slot: {
                        use solana_program::sysvar::last_restart_slot::LastRestartSlot;
                        use solana_program::sysvar::Sysvar;
                        LastRestartSlot::get()
                            .map(|lrs| lrs.last_restart_slot)
                            .unwrap_or(0)
                    },
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
                    new_account_fee,
                };
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
                    insurance_operator: a_admin.key.to_bytes(),
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
                    program_id,
                    a_slab.key,
                    config.vault_authority_bump,
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
                check_no_oracle_live_envelope(zc::engine_ref(&data)?, clock.slot)?;

                // Reject misaligned deposits — dust would be silently donated
                let (_units_check, dust_check) =
                    crate::units::base_to_units(fee_payment, config.unit_scale);
                if dust_check != 0 {
                    return Err(ProgramError::InvalidArgument);
                }

                // InitUser splits `fee_payment` into:
                //   - `new_account_fee` → insurance (wrapper-charged)
                //   - remainder → capital
                //
                // Engine requires `amount > 0` on materialization (§10.2);
                // anti-spam is the wrapper's `new_account_fee` plus the
                // materialization floor `capital_units > 0`. Higher floors
                // are wrapper policy — users holding a dust capital balance
                // are cleaned up by maintenance fees (§7.3).
                let fee_base: u64 = config
                    .new_account_fee
                    .try_into()
                    .map_err(|_| PercolatorError::EngineOverflow)?;
                if fee_payment <= fee_base {
                    return Err(PercolatorError::EngineInsufficientBalance.into());
                }
                let capital_base = fee_payment - fee_base;

                // TVL:insurance cap (admin opt-in). Must apply to InitUser
                // too, not just DepositCollateral — otherwise `fee_payment`
                // here is an unbounded bypass that lets an attacker create
                // fresh accounts with arbitrary capital while the normal
                // deposit path is capped. Simulate the post-fee state: the
                // `new_account_fee` share raises the insurance ceiling, and
                // only the capital remainder counts against c_tot.
                if config.tvl_insurance_cap_mult > 0 {
                    let (capital_units_sim, _) =
                        crate::units::base_to_units(capital_base, config.unit_scale);
                    let (fee_units_sim, _) =
                        crate::units::base_to_units(fee_base, config.unit_scale);
                    let engine_r = zc::engine_ref(&data)?;
                    let ins_new = engine_r
                        .insurance_fund
                        .balance
                        .get()
                        .saturating_add(fee_units_sim as u128);
                    let c_tot_new = engine_r
                        .c_tot
                        .get()
                        .saturating_add(capital_units_sim as u128);
                    let cap = ins_new.saturating_mul(config.tvl_insurance_cap_mult as u128);
                    if c_tot_new > cap {
                        return Err(PercolatorError::DepositCapExceeded.into());
                    }
                }

                // Transfer the full fee_payment to vault; split downstream.
                collateral::deposit(a_token, a_user_ata, a_vault, a_user, fee_payment)?;

                let (capital_units, _) =
                    crate::units::base_to_units(capital_base, config.unit_scale);
                let (fee_units, _) = crate::units::base_to_units(fee_base, config.unit_scale);
                if capital_units == 0 {
                    return Err(PercolatorError::EngineInsufficientBalance.into());
                }

                let engine = zc::engine_mut(&mut data)?;
                let idx = prepare_lazy_free_head(engine)?;
                engine
                    .deposit_not_atomic(idx, capital_units as u128, clock.slot)
                    .map_err(map_risk_error)?;
                engine
                    .set_owner(idx, a_user.key.to_bytes())
                    .map_err(map_risk_error)?;
                if fee_units > 0 {
                    engine
                        .top_up_insurance_fund(fee_units as u128, clock.slot)
                        .map_err(map_risk_error)?;
                }
                let gen =
                    state::next_mat_counter(&mut data).ok_or(PercolatorError::EngineOverflow)?;
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
                if matcher_program == Pubkey::default() || matcher_context == Pubkey::default() {
                    return Err(ProgramError::InvalidInstructionData);
                }

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
                    program_id,
                    a_slab.key,
                    config.vault_authority_bump,
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
                check_no_oracle_live_envelope(zc::engine_ref(&data)?, clock.slot)?;

                // Reject misaligned deposits — dust would be silently donated
                let (_units_check, dust_check) =
                    crate::units::base_to_units(fee_payment, config.unit_scale);
                if dust_check != 0 {
                    return Err(ProgramError::InvalidArgument);
                }

                // Same split semantics as InitUser: fee → insurance, rest
                // → capital. Engine requires capital_units > 0 on
                // materialization; higher floors are wrapper policy.
                let fee_base: u64 = config
                    .new_account_fee
                    .try_into()
                    .map_err(|_| PercolatorError::EngineOverflow)?;
                if fee_payment <= fee_base {
                    return Err(PercolatorError::EngineInsufficientBalance.into());
                }
                let capital_base = fee_payment - fee_base;

                // TVL:insurance cap (admin opt-in). Same bypass concern as
                // InitUser — simulate post-fee state (fee → insurance,
                // capital → c_tot) before checking the cap.
                if config.tvl_insurance_cap_mult > 0 {
                    let (capital_units_sim, _) =
                        crate::units::base_to_units(capital_base, config.unit_scale);
                    let (fee_units_sim, _) =
                        crate::units::base_to_units(fee_base, config.unit_scale);
                    let engine_r = zc::engine_ref(&data)?;
                    let ins_new = engine_r
                        .insurance_fund
                        .balance
                        .get()
                        .saturating_add(fee_units_sim as u128);
                    let c_tot_new = engine_r
                        .c_tot
                        .get()
                        .saturating_add(capital_units_sim as u128);
                    let cap = ins_new.saturating_mul(config.tvl_insurance_cap_mult as u128);
                    if c_tot_new > cap {
                        return Err(PercolatorError::DepositCapExceeded.into());
                    }
                }

                collateral::deposit(a_token, a_user_ata, a_vault, a_user, fee_payment)?;

                let (capital_units, _) =
                    crate::units::base_to_units(capital_base, config.unit_scale);
                let (fee_units, _) = crate::units::base_to_units(fee_base, config.unit_scale);
                if capital_units == 0 {
                    return Err(PercolatorError::EngineInsufficientBalance.into());
                }

                let engine = zc::engine_mut(&mut data)?;
                let idx = prepare_lazy_free_head(engine)?;
                engine
                    .deposit_not_atomic(idx, capital_units as u128, clock.slot)
                    .map_err(map_risk_error)?;
                engine
                    .set_owner(idx, a_user.key.to_bytes())
                    .map_err(map_risk_error)?;
                engine.accounts[idx as usize].kind = percolator::Account::KIND_LP;
                engine.accounts[idx as usize].matcher_program = matcher_program.to_bytes();
                engine.accounts[idx as usize].matcher_context = matcher_context.to_bytes();
                if fee_units > 0 {
                    engine
                        .top_up_insurance_fund(fee_units as u128, clock.slot)
                        .map_err(map_risk_error)?;
                }
                let gen =
                    state::next_mat_counter(&mut data).ok_or(PercolatorError::EngineOverflow)?;
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
                if amount == 0 {
                    return Err(ProgramError::InvalidArgument);
                }

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
                    program_id,
                    a_slab.key,
                    config.vault_authority_bump,
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
                check_no_oracle_live_envelope(zc::engine_ref(&data)?, clock.slot)?;

                // Reject misaligned deposits — dust would be silently donated
                let (_units_check, dust_check) =
                    crate::units::base_to_units(amount, config.unit_scale);
                if dust_check != 0 {
                    return Err(ProgramError::InvalidArgument);
                }

                // TVL:insurance cap (admin opt-in). Enforced BEFORE the
                // SPL transfer so rejected deposits don't move funds.
                // Formula: `c_tot_new <= k * insurance_fund.balance`.
                // k=0 disables the check; nonzero k with zero insurance
                // means no deposits accepted — operator is expected to
                // seed insurance (via TopUpInsurance or fee accumulation)
                // before enabling or raising k. No fee split here: the
                // full amount credits capital against the current
                // insurance balance.
                if config.tvl_insurance_cap_mult > 0 {
                    let (capital_units_sim, _) =
                        crate::units::base_to_units(amount, config.unit_scale);
                    let engine_r = zc::engine_ref(&data)?;
                    let ins = engine_r.insurance_fund.balance.get();
                    let c_tot_new = engine_r
                        .c_tot
                        .get()
                        .saturating_add(capital_units_sim as u128);
                    let cap = ins.saturating_mul(config.tvl_insurance_cap_mult as u128);
                    if c_tot_new > cap {
                        return Err(PercolatorError::DepositCapExceeded.into());
                    }
                }

                // Transfer base tokens to vault
                collateral::deposit(a_token, a_user_ata, a_vault, a_user, amount)?;

                // Convert base tokens to units for engine
                let (units, _dust) = crate::units::base_to_units(amount, config.unit_scale);

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, user_idx)?;

                // Owner authorization via policy helper
                let owner = engine.accounts[user_idx as usize].owner;
                if !crate::policy::owner_ok(owner, a_user.key.to_bytes()) {
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
                sync_account_fee_bounded_to_market(engine, &config, user_idx, clock.slot)?;

                engine
                    .deposit_not_atomic(user_idx, units as u128, clock.slot)
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
                if amount == 0 {
                    return Err(ProgramError::InvalidArgument);
                }

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                let mut config = state::read_config(&data);
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let derived_pda = accounts::derive_vault_authority_with_bump(
                    program_id,
                    a_slab.key,
                    config.vault_authority_bump,
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
                let funding_rate_e9 = compute_current_funding_rate_e9(&config)?;
                let price = {
                    let is_hyperp = oracle::is_hyperp_mode(&config);
                    let px = if is_hyperp {
                        let eng = zc::engine_ref(&data)?;
                        let p_last = eng.last_oracle_price;
                        let price_move_dt = price_move_residual_dt(eng, clock.slot)?;
                        let cap_bps = eng.params.max_price_move_bps_per_slot;
                        let oi_any = eng.oi_eff_long_q != 0 || eng.oi_eff_short_q != 0;
                        oracle::get_engine_oracle_price_e6(
                            p_last,
                            price_move_dt,
                            clock.slot,
                            clock.unix_timestamp,
                            &mut config,
                            a_oracle_idx,
                            cap_bps,
                            oi_any,
                        )?
                    } else {
                        read_price_and_stamp(
                            &mut config,
                            a_oracle_idx,
                            clock.unix_timestamp,
                            clock.slot,
                            &mut data,
                        )?
                    };
                    state::write_config(&mut data, &config);
                    px
                };

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, user_idx)?;

                let owner = engine.accounts[user_idx as usize].owner;
                if !crate::policy::owner_ok(owner, a_user.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                if config.unit_scale != 0 && amount % config.unit_scale as u64 != 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }

                let (units_requested, _) = crate::units::base_to_units(amount, config.unit_scale);

                let withdraw_slot = clock.slot;
                let admit_h_min = engine.params.h_min;
                let admit_h_max = engine.params.h_max;
                // Withdraw is account-limited: it only touches this user, so
                // it must not become a hidden market-progress path while
                // unrelated OI is exposed. If price/funding progress is
                // equity-active, KeeperCrank must do the account-touching
                // cascade first. When the strict gate allows progress, accrue
                // before fee sync and withdraw so the engine's internal
                // accrue no-ops at the same slot/price.
                ensure_market_accrued_to_now_for_account_limited_op(
                    engine,
                    &config,
                    clock.slot,
                    price,
                    funding_rate_e9,
                )?;
                reject_any_target_lag(&config, engine)?;
                // Realize due maintenance fees BEFORE the withdrawal margin
                // check, so the account can't withdraw against pre-fee capital.
                sync_account_fee(engine, &config, user_idx, clock.slot)?;
                let admit_threshold = Some(engine.params.maintenance_margin_bps as u128);
                engine
                    .withdraw_not_atomic(
                        user_idx,
                        units_requested as u128,
                        price,
                        withdraw_slot,
                        funding_rate_e9,
                        admit_h_min,
                        admit_h_max,
                        admit_threshold,
                    )
                    .map_err(map_risk_error)?;
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
                    // through the fee-aware resolved close API.
                    // The resolved crank only handles lifecycle.

                    // Resolved markets are frozen: no K/F mutation, no per-account
                    // settlement. The end-of-instruction lifecycle is a no-op here,
                    // and engine v12.18.1 no longer exposes it publicly (it is folded
                    // into the _not_atomic methods that actually change state).
                    let _ = engine; // silence "unused" when no crank work runs.

                    return Ok(());
                }

                let config_pre_read = state::read_config(&data);
                let mut config = config_pre_read;

                let clock = Clock::from_account_info(a_clock)?;

                // Hyperp mode updates the internal index toward mark; external
                // mode reads the signed oracle and lets the engine enforce caps.
                let is_hyperp = oracle::is_hyperp_mode(&config);
                let (engine_last_oracle_price, price_move_dt, cap_bps, oi_any) = {
                    let engine = zc::engine_ref(&data)?;
                    (
                        engine.last_oracle_price,
                        price_move_residual_dt(engine, clock.slot)?,
                        engine.params.max_price_move_bps_per_slot,
                        engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0,
                    )
                };

                // Capture pre-oracle-read funding rate for anti-retroactivity (§5.5).
                // The rate for interval [last_market_slot, now_slot] must reflect
                // mark vs index DURING that interval, not the post-read state.
                let funding_rate_e9_pre = compute_current_funding_rate_e9(&config)?;

                let price = if is_hyperp {
                    // Hyperp mode: update index toward mark with rate limiting
                    oracle::get_engine_oracle_price_e6(
                        engine_last_oracle_price,
                        price_move_dt,
                        clock.slot,
                        clock.unix_timestamp,
                        &mut config,
                        a_oracle,
                        cap_bps,
                        oi_any,
                    )?
                } else {
                    read_price_and_stamp(
                        &mut config,
                        a_oracle,
                        clock.unix_timestamp,
                        clock.slot,
                        &mut data,
                    )?
                };

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
                    if !crate::policy::owner_ok(stored_owner, a_caller.key.to_bytes()) {
                        return Err(PercolatorError::EngineUnauthorized.into());
                    }
                }
                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: keeper_crank_start");
                    sol_log_compute_units();
                }
                let mut combined =
                    alloc::vec::Vec::with_capacity(buf.count as usize + candidates.len());
                for i in 0..buf.count as usize {
                    let risk_idx = buf.entries[i].idx;
                    let risk_eff_abs = if idx_used_in_market(engine, risk_idx as usize) {
                        effective_pos_q_checked(engine, risk_idx as usize)?.unsigned_abs()
                    } else {
                        0
                    };
                    for &(cidx, policy) in candidates.iter() {
                        let q_close_q = match policy {
                            Some(percolator::LiquidationPolicy::ExactPartial(q)) => q,
                            _ => continue,
                        };
                        if cidx != risk_idx || q_close_q == 0 || q_close_q >= risk_eff_abs {
                            continue;
                        }
                        // Preserve one keeper-supplied partial-liquidation
                        // opportunity for cached high-risk accounts, but keep
                        // the risk-buffer FullClose immediately behind it as
                        // a fallback. Invalid local bounds are skipped here so
                        // repeated malformed hints cannot consume the engine's
                        // phase-1 attempt budget before the fallback.
                        combined.push((cidx, policy));
                        break;
                    }
                    combined.push((risk_idx, Some(percolator::LiquidationPolicy::FullClose)));
                }
                for &(cidx, policy) in candidates.iter() {
                    let mut already_promoted = false;
                    if matches!(policy, Some(percolator::LiquidationPolicy::ExactPartial(_))) {
                        for i in 0..buf.count as usize {
                            if buf.entries[i].idx == cidx {
                                already_promoted = true;
                                break;
                            }
                        }
                    }
                    if !already_promoted {
                        combined.push((cidx, policy));
                    }
                }
                // Defense-in-depth cap: the decode-time check already
                // bounds candidates, but truncating `combined` here as
                // well ensures the engine's scan is bounded even if the
                // decode cap is loosened in the future or an insider
                // path ever constructs `combined` differently. Cap:
                // risk buffer max + the decoded candidate cap.
                const COMBINED_CAP: usize =
                    crate::constants::RISK_BUF_CAP + crate::constants::MAX_KEEPER_CANDIDATES;
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
                // Ordering: keeper_crank_not_atomic first, then candidate fee
                // syncs and the bitmap sweep. That keeps recurring fees junior
                // to lazy mark/funding losses discovered by the authoritative
                // touch while retaining a bounded post-crank fee pass.
                //
                // Crank reward: pay CRANK_REWARD_BPS (50 %) of the maintenance-
                // fee sweep delta back to the non-permissionless caller as
                // capital. The remaining 50% stays in insurance. No additional
                // account-count cap — the sweep itself is the bound (at most
                // FEE_SWEEP_BUDGET accounts per crank, and each contribution
                // is `fee_per_slot × dt_that_account_owed`). Trading,
                // liquidation, and resolution fees are NOT shared with the
                // cranker — sweep_delta is measured only around the post-crank
                // bitmap sweep, not around liquidation or candidate fees.
                //
                // Reward is paid AFTER keeper_crank_not_atomic. Paying it
                // before would let a borderline caller self-rescue: the capital
                // bump would change the account's maintenance-margin health
                // before the crank's liquidation pass evaluated it. Paying it
                // after leaves the crank's risk decisions on pre-reward state,
                // and a caller who got liquidated inside the crank (slot no
                // longer used) simply doesn't collect the reward.
                // Fully accrue market to clock.slot when the full gap fits.
                // If the market is farther behind, the latest engine advances
                // exactly one bounded segment toward the raw target while
                // retaining clock.slot as the authenticated public time.
                let raw_target = if oracle::is_hyperp_mode(&config) {
                    hyperp_target_price(&config)
                } else {
                    config.oracle_target_price_e6
                };
                let partial_target =
                    oversized_catchup_target(engine, clock.slot, raw_target, funding_rate_e9_pre)?;
                let (
                    crank_slot,
                    crank_price,
                    partial_catchup,
                    defer_phase2,
                    rr_window_size,
                    rr_scan_limit,
                ) = if let Some(target_slot) = partial_target {
                    let segment_price = keeper_partial_segment_price(engine, &config, target_slot)?;
                    if segment_price == engine.last_oracle_price
                        && raw_target != engine.last_oracle_price
                        && (engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0)
                    {
                        return Err(PercolatorError::CatchupRequired.into());
                    }
                    config.last_effective_price_e6 = segment_price;
                    if oracle::is_hyperp_mode(&config)
                        && (segment_price != engine.last_oracle_price
                            || segment_price == hyperp_target_price(&config))
                    {
                        config.last_hyperp_index_slot = clock.slot;
                    }
                    let rr_window_size = if candidates.is_empty() {
                        crate::constants::RR_WINDOW_PER_CRANK
                    } else {
                        crate::constants::RR_WINDOW_WITH_CANDIDATES_PER_CRANK
                    };
                    let rr_scan_limit = core::cmp::min(
                        crate::constants::RR_SCAN_LIMIT_PER_CRANK,
                        engine.params.max_accounts,
                    );
                    (
                        clock.slot,
                        segment_price,
                        true,
                        false,
                        rr_window_size,
                        rr_scan_limit,
                    )
                } else {
                    let (defer_phase2, rr_window_size, rr_scan_limit) =
                        prepare_keeper_crank_market_progress(
                            engine,
                            &config,
                            clock.slot,
                            price,
                            funding_rate_e9_pre,
                            &combined,
                        )?;
                    (
                        clock.slot,
                        price,
                        false,
                        defer_phase2,
                        rr_window_size,
                        rr_scan_limit,
                    )
                };

                let admit_h_min = engine.params.h_min;
                let admit_h_max = engine.params.h_max;
                let admit_threshold = Some(engine.params.maintenance_margin_bps as u128);
                let rr_window_size = if defer_phase2 { 0 } else { rr_window_size };
                let engine_candidates: &[(u16, Option<percolator::LiquidationPolicy>)] =
                    if partial_catchup { &[] } else { &combined };
                let _outcome = engine
                    .keeper_crank_with_request_not_atomic(percolator::KeeperCrankRequest {
                        now_slot: crank_slot,
                        oracle_price: crank_price,
                        ordered_candidates: engine_candidates,
                        max_revalidations: if partial_catchup {
                            0
                        } else {
                            crate::constants::LIQ_BUDGET_PER_CRANK
                        },
                        max_candidate_inspections: if partial_catchup {
                            0
                        } else {
                            keeper_candidate_inspection_cap()
                        },
                        funding_rate_e9: funding_rate_e9_pre,
                        admit_h_min,
                        admit_h_max,
                        admit_h_max_consumption_threshold_bps_opt: admit_threshold,
                        rr_touch_limit: rr_window_size,
                        rr_scan_limit,
                    })
                    .map_err(map_risk_error)?;
                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: keeper_crank_end");
                    sol_log_compute_units();
                }

                // Fee realization runs after authoritative keeper touch/loss
                // settlement so recurring fees cannot become senior to lazy
                // mark/funding losses. Candidate syncs are bounded and not
                // rewardable; the reward delta is measured only over the
                // bitmap sweep that follows them.
                let sweep_delta = if partial_catchup {
                    // The market remains loss-stale. Do not charge recurring
                    // fees until a later crank makes the market loss-current;
                    // fee sync would otherwise be senior to the remaining
                    // unapplied historical loss segment.
                    0
                } else {
                    let candidate_syncs = sync_keeper_candidate_fees_after_crank(
                        engine,
                        &combined,
                        crank_slot,
                        config.maintenance_fee_per_slot,
                    )?;
                    let ins_before_reward_sweep = engine.insurance_fund.balance.get();
                    let remaining_budget =
                        crate::constants::FEE_SWEEP_BUDGET.saturating_sub(candidate_syncs);
                    sweep_maintenance_fees(engine, &mut config, crank_slot, remaining_budget)?;
                    engine
                        .insurance_fund
                        .balance
                        .get()
                        .saturating_sub(ins_before_reward_sweep)
                };

                // Pay the crank reward AFTER keeper_crank_not_atomic has run
                // all liquidation / lifecycle logic. The sweep_delta is
                // captured around the post-crank sweep, so the reward is
                // bounded only by the maintenance-fee collection on this call,
                // not by incidental insurance growth from liquidation fees.
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
                    && idx_used_in_market(engine, caller_idx as usize)
                {
                    // 50 / 50 split: half to caller, half stays in insurance.
                    let mut reward =
                        sweep_delta.saturating_mul(crate::constants::CRANK_REWARD_BPS) / 10_000u128;
                    // Cap reward by post-crank insurance balance.
                    let ins_now = engine.insurance_fund.balance.get();
                    if reward > ins_now {
                        reward = ins_now;
                    }
                    if reward > 0 {
                        engine
                            .credit_account_from_insurance_not_atomic(
                                caller_idx, reward, crank_slot,
                            )
                            .map_err(map_risk_error)?;
                    }

                    // Belt-and-suspenders: the reward may never cause a
                    // floor breach. A market can legitimately sit below
                    // floor (e.g. after an earlier insurance-loss event),
                    // so we can't assert `balance >= floor` universally.
                    // Instead, enforce the MINIMUM-MONOTONIC property:
                    //   post_balance >= 0  (reward cap is min(reward, ins_now)
                    // so subtracting can't underflow). Violation = cap math
                    // regression.
                    let post_balance = engine.insurance_fund.balance.get();
                    debug_assert!(post_balance <= ins_now);
                    if post_balance > ins_now {
                        return Err(PercolatorError::EngineCorruptState.into());
                    }
                }

                // Public ReclaimEmptyAccount is retired. Keepers that want
                // targeted empty-account cleanup submit the account as a
                // touch-only candidate; after the engine's account-touching
                // crank has settled local state, reclaim flat zero-capital
                // candidates here. Ineligible candidates are ignored, matching
                // the crank's untrusted-shortlist semantics.
                for &(idx, _) in combined.iter() {
                    reclaim_flat_zero_account_if_eligible(engine, idx, crank_slot)?;
                }

                // Copy stats and drop engine mutable borrow.
                // Use the actual crank outcome so observability/telemetry
                // reflects real liquidations, not a hard-coded zero.
                #[cfg(feature = "cu-audit")]
                let liqs = _outcome.num_liquidations as u64;
                #[cfg(feature = "cu-audit")]
                let ins_low = engine.insurance_fund.balance.get() as u64;

                // Engine has now processed a real oracle price via accrue_market_to.
                // engine.last_oracle_price is no longer the init sentinel.
                if !state::is_oracle_initialized(&data) {
                    state::set_oracle_initialized(&mut data);
                }

                // Write updated config. On partial catchup, preserve the
                // oracle target/liveness and fee cursor, but roll back
                // effective/index fields that correspond to the wall-clock
                // observation the engine has not reached yet.
                let config_to_write = if partial_catchup {
                    partial_crank_config_to_write(config_pre_read, config)
                } else {
                    config
                };
                state::write_config(&mut data, &config_to_write);

                // ── RiskBuffer maintenance (engine borrow dropped) ──
                {
                    let mut buf = state::read_risk_buffer(&data);
                    let engine = zc::engine_ref(&data)?;

                    // Phase A: scrub dead entries
                    for i in (0..4usize).rev() {
                        if i >= buf.count as usize {
                            continue;
                        }
                        let eidx = buf.entries[i].idx as usize;
                        if !idx_used_in_market(engine, eidx)
                            || effective_pos_q_checked(engine, eidx)? == 0
                        {
                            buf.remove(buf.entries[i].idx);
                        }
                    }
                    #[cfg(feature = "cu-audit")]
                    {
                        msg!("CU_CHECKPOINT: risk_buffer_phase_a_end");
                        sol_log_compute_units();
                    }

                    // Phase B: refresh surviving entries
                    for i in 0..buf.count as usize {
                        let eidx = buf.entries[i].idx as usize;
                        let eff = effective_pos_q_checked(engine, eidx)?;
                        let notional = risk_notional_ceil(eff, crank_price);
                        buf.entries[i].notional = notional;
                    }
                    buf.recompute_min();
                    #[cfg(feature = "cu-audit")]
                    {
                        msg!("CU_CHECKPOINT: risk_buffer_phase_b_end");
                        sol_log_compute_units();
                    }

                    // Phase C: progressive discovery scan over the used-account
                    // bitmap. The budget is spent on live slots, not empty
                    // storage indices; sparse markets can therefore refill the
                    // risk buffer in one crank instead of waiting for a dense
                    // cursor window to walk across empty slots.
                    let scan_mod = engine.params.max_accounts as usize;
                    let scan_mod = if scan_mod == 0 || scan_mod > percolator::MAX_ACCOUNTS {
                        percolator::MAX_ACCOUNTS
                    } else {
                        scan_mod
                    };
                    let scan_start = (buf.scan_cursor as usize) % scan_mod;
                    let mut used_seen = 0usize;
                    let mut next_cursor = scan_start;
                    let mut completed_cycle = true;

                    'scan: for pass in 0..2usize {
                        let (range_start, range_end) = if pass == 0 {
                            (scan_start, scan_mod)
                        } else {
                            (0usize, scan_start)
                        };
                        if range_start >= range_end {
                            continue;
                        }

                        let first_word = range_start >> 6;
                        let last_word = (range_end - 1) >> 6;
                        let mut word_cursor = first_word;
                        while word_cursor <= last_word {
                            let word_base = word_cursor * 64;
                            let lower_bit = if word_cursor == first_word {
                                range_start & 63
                            } else {
                                0
                            };
                            let upper_bit_exclusive = if word_cursor == last_word {
                                ((range_end - 1) & 63) + 1
                            } else {
                                64
                            };
                            let lower_mask = if lower_bit == 0 {
                                u64::MAX
                            } else {
                                !((1u64 << lower_bit).wrapping_sub(1))
                            };
                            let upper_mask = if upper_bit_exclusive == 64 {
                                u64::MAX
                            } else {
                                (1u64 << upper_bit_exclusive) - 1
                            };
                            let mut bits = engine.used[word_cursor] & lower_mask & upper_mask;

                            while bits != 0 {
                                let bit = bits.trailing_zeros() as usize;
                                bits &= bits - 1;
                                let idx = word_base + bit;
                                if !idx_used_in_market(engine, idx) {
                                    continue;
                                }
                                used_seen += 1;
                                let eff = effective_pos_q_checked(engine, idx)?;
                                if eff == 0 {
                                    buf.remove(idx as u16);
                                } else {
                                    let notional = risk_notional_ceil(eff, crank_price);
                                    buf.upsert(idx as u16, notional);
                                }
                                next_cursor = (idx + 1) % scan_mod;
                                if used_seen >= crate::constants::RISK_SCAN_WINDOW {
                                    completed_cycle = false;
                                    break 'scan;
                                }
                            }

                            word_cursor += 1;
                        }
                    }
                    if completed_cycle {
                        // Completed a full bitmap cycle before spending the
                        // used-account budget. Restart from the same point on
                        // the next crank; every used slot has just been seen.
                        next_cursor = scan_start;
                    }
                    buf.scan_cursor = next_cursor as u16;
                    #[cfg(feature = "cu-audit")]
                    {
                        msg!("CU_CHECKPOINT: risk_buffer_phase_c_end");
                        sol_log_compute_units();
                    }

                    // Phase D: ingest caller-supplied candidates
                    let mut candidate_ingests = 0usize;
                    for &(cidx, _) in candidates.iter() {
                        if candidate_ingests >= crate::constants::RISK_SCAN_WINDOW {
                            break;
                        }
                        let ci = cidx as usize;
                        if !idx_used_in_market(engine, ci) {
                            continue;
                        }
                        let eff = effective_pos_q_checked(engine, ci)?;
                        if eff == 0 {
                            buf.remove(cidx);
                        } else {
                            let notional = risk_notional_ceil(eff, crank_price);
                            buf.upsert(cidx, notional);
                        }
                        candidate_ingests += 1;
                    }

                    state::write_risk_buffer(&mut data, &buf);
                    #[cfg(feature = "cu-audit")]
                    {
                        msg!("CU_CHECKPOINT: risk_buffer_phase_d_end");
                        sol_log_compute_units();
                    }
                }

                #[cfg(feature = "cu-audit")]
                {
                    msg!("CRANK_STATS");
                    sol_log_64(0xC8A4C, liqs, percolator::MAX_ACCOUNTS as u64, ins_low, 0);
                }
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
                if size == 0 || size == i128::MIN {
                    return Err(ProgramError::InvalidInstructionData);
                }

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
                let funding_rate_e9 = compute_current_funding_rate_e9(&config)?;

                // Read the signed oracle price and let the engine enforce caps.
                let price = read_price_and_stamp(
                    &mut config,
                    a_oracle,
                    clock.unix_timestamp,
                    clock.slot,
                    &mut data,
                )?;
                state::write_config(&mut data, &config);

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, lp_idx)?;
                check_idx(engine, user_idx)?;

                // TradeNoCpi: no matcher check. Both sides are bilateral signers,
                // no CPI is invoked. Matcher config only matters for TradeCpi.

                let u_owner = engine.accounts[user_idx as usize].owner;

                // Owner authorization via policy helper
                if !crate::policy::owner_ok(u_owner, a_user.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }
                let l_owner = engine.accounts[lp_idx as usize].owner;
                if !crate::policy::owner_ok(l_owner, a_lp.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                // Side-mode gating is handled inside engine.execute_trade_not_atomic()

                // Exposed market progress belongs to KeeperCrank. If this
                // account-limited trade is allowed to proceed, accrue to
                // clock.slot before execute_trade_with_matcher, which
                // internally syncs both counterparties' fees before the trade.
                ensure_market_accrued_to_now_for_account_limited_op(
                    engine,
                    &config,
                    clock.slot,
                    price,
                    funding_rate_e9,
                )?;
                reject_any_target_lag(&config, engine)?;

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
                    engine
                        .sync_account_fee_to_slot_not_atomic(
                            user_idx,
                            clock.slot,
                            config.maintenance_fee_per_slot,
                        )
                        .map_err(map_risk_error)?;
                    engine
                        .sync_account_fee_to_slot_not_atomic(
                            lp_idx,
                            clock.slot,
                            config.maintenance_fee_per_slot,
                        )
                        .map_err(map_risk_error)?;
                }

                // Snapshot insurance fund balance for fee-weighted EWMA.
                // The delta after execute_trade = trading_fees -
                // losses_absorbed (maintenance fees already synced above).
                // NOTE: If loss absorption occurs during the same trade (spec §5.4),
                // delta undercounts the actual fee. This is the conservative direction:
                // mark is stickier during volatile loss-absorption events, never
                // more manipulable. A future engine API could expose fee_paid directly.
                let current_fee_paid_cap =
                    current_trade_fee_paid_cap(size, price, engine.params.trading_fee_bps)?;
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
                    engine,
                    &NoOpMatcher,
                    lp_idx,
                    user_idx,
                    clock.slot,
                    price,
                    size,
                    funding_rate_e9,
                    0, // NoOpMatcher ignores lp_account_id
                    0,
                )
                .map_err(map_risk_error)?;

                // Update mark EWMA from trade (NoOpMatcher fills at oracle price).
                // NOTE: NoOpMatcher fills at oracle price, so mark_ewma converges to oracle
                // for TradeNoCpi trades. This means TradeNoCpi-only markets have zero premium
                // and zero funding. Markets that need funding must use TradeCpi with a matcher
                // that can set exec_price != oracle (creating mark/index divergence).
                // Per-slot price-move cap is init-immutable (engine RiskParams).
                let max_change_bps = engine.params.max_price_move_bps_per_slot;
                if max_change_bps > 0 {
                    let clamped_price = oracle::clamp_oracle_price(
                        crate::policy::mark_ewma_clamp_base(config.last_effective_price_e6),
                        price,
                        max_change_bps,
                    );
                    // fee_paid = actual fee collected into insurance (post - pre).
                    // This is exact: no overestimate from pre-trade capital snapshot.
                    let fee_paid_nocpi = if config.mark_min_fee > 0 {
                        let ins_after = engine.insurance_fund.balance.get();
                        let delta = ins_after
                            .saturating_sub(ins_before)
                            .min(current_fee_paid_cap);
                        core::cmp::min(delta, u64::MAX as u128) as u64
                    } else {
                        0u64
                    };
                    let old_ewma = config.mark_ewma_e6;
                    // N4 fix: seed EWMA at oracle price on first trade (not exec price).
                    // Prevents attacker from imprinting a biased mark on the first fill.
                    let ewma_price = if old_ewma == 0 && config.last_effective_price_e6 > 0 {
                        config.last_effective_price_e6
                    } else {
                        clamped_price
                    };
                    config.mark_ewma_e6 = crate::policy::ewma_update(
                        old_ewma,
                        ewma_price,
                        config.mark_ewma_halflife_slots,
                        config.mark_ewma_last_slot,
                        clock.slot,
                        fee_paid_nocpi,
                        config.mark_min_fee,
                    );
                    // Only full-weight observations advance the EWMA clock
                    // (Finding 7). Sub-threshold trades can still nudge the
                    // EWMA value via partial alpha, but their clock bump
                    // would make the clock a liveness signal attackers can
                    // cheaply refresh — and on Hyperp markets the soft-
                    // staleness check reads `max(mark_ewma_last_slot,
                    // last_mark_push_slot)`, so any clock bump on dust
                    // trades keeps an otherwise-dead Hyperp market live.
                    // Gating on full-weight collapses the two-clock
                    // dichotomy: both clocks now only refresh on
                    // observation-eligible fills.
                    let full_weight_observation_nocpi =
                        config.mark_min_fee == 0 || fee_paid_nocpi >= config.mark_min_fee;
                    if full_weight_observation_nocpi {
                        config.mark_ewma_last_slot = clock.slot;
                    }
                    // NOTE: do NOT stamp funding rate here — execute_trade_not_atomic
                    // handles it via the funding_rate parameter (§5.5 anti-retroactivity).
                }

                // v12.17: funding rate is passed to accrue_market_to, not stored directly.
                // The next accrual (crank/trade/settle) will use the updated mark EWMA.

                // Collect post-trade positions for risk buffer
                let user_eff_nocpi = effective_pos_q_checked(engine, user_idx as usize)?;
                let lp_eff_nocpi = effective_pos_q_checked(engine, lp_idx as usize)?;
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
                            let notional = risk_notional_ceil(eff, price);
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
                if a_tail.len() > MAX_MATCHER_TAIL_ACCOUNTS {
                    return Err(ProgramError::InvalidInstructionData);
                }

                accounts::expect_signer(a_user)?;
                // Reject zero-size requests at entry — zero-fill path should only
                // be reached via matcher returning exec_size == 0 on a nonzero request.
                // Also reject i128::MIN before oracle/CPI work; it has no positive
                // counterpart and the engine would reject it later.
                if size == 0 || size == i128::MIN {
                    return Err(ProgramError::InvalidInstructionData);
                }
                if lp_idx == user_idx {
                    return Err(ProgramError::InvalidInstructionData);
                }
                // Note: a_lp_owner does NOT need to be a signer for TradeCpi.
                // LP owner delegated trade authorization to the matcher program.
                // The matcher CPI (via LP PDA invoke_signed) validates the trade.
                accounts::expect_writable(a_slab)?;
                accounts::expect_writable(a_matcher_ctx)?;

                // Matcher shape validation via policy helper
                let matcher_shape = crate::policy::MatcherAccountsShape {
                    prog_executable: a_matcher_prog.executable,
                    ctx_executable: a_matcher_ctx.executable,
                    ctx_owner_is_prog: a_matcher_ctx.owner == a_matcher_prog.key,
                    ctx_len_ok: crate::policy::ctx_len_sufficient(a_matcher_ctx.data_len()),
                };
                if !crate::policy::matcher_shape_ok(matcher_shape) {
                    return Err(ProgramError::InvalidAccountData);
                }

                // Phase 1: Validate lp_pda is the correct PDA, system-owned, empty data, 0 lamports
                let lp_bytes = lp_idx.to_le_bytes();
                let (expected_lp_pda, bump) = Pubkey::find_program_address(
                    &[b"lp", a_slab.key.as_ref(), &lp_bytes],
                    program_id,
                );
                // PDA key validation via policy helper
                if !crate::policy::pda_key_matches(
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
                let (
                    lp_account_id,
                    mut config,
                    config_pre_oracle,
                    req_id,
                    lp_matcher_prog,
                    lp_matcher_ctx,
                    engine_last_oracle_price,
                    engine_last_market_slot,
                    engine_max_accrual_dt_slots,
                    engine_cap_bps,
                    engine_oi_any,
                ) = {
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
                    // Nonce advancement via policy helper
                    // Reject if nonce would overflow — wrapping reopens old request IDs.
                    let nonce = state::read_req_nonce(&*data);
                    let req_id = crate::policy::nonce_on_success(nonce)
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

                    // Owner authorization via policy helper
                    let u_owner = engine.accounts[user_idx as usize].owner;
                    if !crate::policy::owner_ok(u_owner, a_user.key.to_bytes()) {
                        return Err(PercolatorError::EngineUnauthorized.into());
                    }
                    let l_owner = engine.accounts[lp_idx as usize].owner;
                    if !crate::policy::owner_ok(l_owner, a_lp_owner.key.to_bytes()) {
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
                        engine.last_oracle_price,
                        engine.last_market_slot,
                        engine.params.max_accrual_dt_slots,
                        engine.params.max_price_move_bps_per_slot,
                        engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0,
                    )
                };

                // Matcher identity binding via policy helper
                if !crate::policy::matcher_identity_ok(
                    lp_matcher_prog,
                    lp_matcher_ctx,
                    a_matcher_prog.key.to_bytes(),
                    a_matcher_ctx.key.to_bytes(),
                ) {
                    return Err(PercolatorError::EngineInvalidMatchingEngine.into());
                }

                let clock = Clock::from_account_info(a_clock)?;
                // Capture pre-read funding rate for anti-retroactivity (§5.5)
                let funding_rate_e9_pre = compute_current_funding_rate_e9(&config)?;

                // Oracle price: Hyperp mode applies rate-limited index update
                // via clamp_toward_with_dt (prevents stale-index manipulation).
                // Non-Hyperp: signed external price; the engine enforces caps.
                let is_hyperp = oracle::is_hyperp_mode(&config);
                let price = if is_hyperp {
                    let price_move_dt = price_move_residual_dt_from_parts(
                        engine_last_market_slot,
                        engine_max_accrual_dt_slots,
                        clock.slot,
                    )?;
                    oracle::get_engine_oracle_price_e6(
                        engine_last_oracle_price,
                        price_move_dt,
                        clock.slot,
                        clock.unix_timestamp,
                        &mut config,
                        a_oracle,
                        engine_cap_bps,
                        engine_oi_any,
                    )?
                } else {
                    let mut slab_data = state::slab_data_mut(a_slab)?;
                    let price = read_price_and_stamp(
                        &mut config,
                        a_oracle,
                        clock.unix_timestamp,
                        clock.slot,
                        &mut slab_data,
                    )?;
                    drop(slab_data);
                    price
                };

                if target_lag_after_read(&config, price) {
                    return Err(PercolatorError::CatchupRequired.into());
                }
                // Reject before matcher CPI if this trade would be the hidden
                // progress path for unrelated exposed accounts.
                {
                    let data = a_slab.try_borrow_data()?;
                    reject_account_limited_market_progress(
                        zc::engine_ref(&*data)?,
                        clock.slot,
                        price,
                        funding_rate_e9_pre,
                    )?;
                }

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
                let mut metas: alloc::vec::Vec<AccountMeta> =
                    alloc::vec::Vec::with_capacity(2 + a_tail.len());
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
                zc::invoke_signed_trade(
                    &ix,
                    a_lp_pda,
                    a_matcher_ctx,
                    a_matcher_prog,
                    a_tail,
                    seeds,
                )?;

                // Clear reentrancy guard after CPI returns.
                {
                    let mut data = state::slab_data_mut(a_slab)?;
                    state::clear_cpi_in_progress(&mut data);
                }

                let ctx_data = a_matcher_ctx.try_borrow_data()?;
                let ret = crate::matcher_abi::read_matcher_return(&ctx_data)?;
                // ABI validation via policy helper
                let ret_fields = crate::policy::MatcherReturnFields {
                    abi_version: ret.abi_version,
                    flags: ret.flags,
                    exec_price_e6: ret.exec_price_e6,
                    exec_size: ret.exec_size,
                    req_id: ret.req_id,
                    lp_account_id: ret.lp_account_id,
                    oracle_price_e6: ret.oracle_price_e6,
                    reserved: ret.reserved,
                };
                if !crate::policy::abi_ok(ret_fields, lp_account_id, price, size, req_id) {
                    return Err(ProgramError::InvalidAccountData);
                }
                drop(ctx_data);

                // User-side slippage protection.
                // Normalize limit to engine-space (same invert+scale as exec_price).
                // For inverted markets, inversion is order-reversing: a "better"
                // raw buy price maps to a larger engine price, so inequalities flip.
                if limit_price_e6 != 0 && ret.exec_size != 0 {
                    let limit_eng = crate::policy::to_engine_price(
                        limit_price_e6,
                        config.invert,
                        config.unit_scale,
                    )
                    .ok_or(PercolatorError::OracleInvalid)?;
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
                    reject_stuck_target_accrual(&config, engine, clock.slot, price)?;
                    reject_account_limited_market_progress(
                        engine,
                        clock.slot,
                        price,
                        funding_rate_e9_pre,
                    )?;
                    // Pre-chunk catch-up so the single accrue_market_to below
                    // sees dt ≤ max_accrual_dt_slots (Finding 3). Use the
                    // pre-read funding rate for catchup chunks (Finding 2).
                    catchup_accrue(engine, clock.slot, price, funding_rate_e9_pre)?;
                    engine
                        .accrue_market_to(clock.slot, price, funding_rate_e9_pre)
                        .map_err(map_risk_error)?;
                    // Restore pre-oracle config, but preserve oracle/index
                    // state that legitimately advanced during the instruction:
                    // - last_good_oracle_slot:        liveness proof from
                    //                                 successful read
                    // - last_effective_price_e6:      accepted external price
                    //                                 or Hyperp index
                    // - last_oracle_publish_time:     MUST be preserved
                    //                                 atomically with the
                    //                                 baseline — otherwise a
                    //                                 zero-fill could keep
                    //                                 the new baseline while
                    //                                 rolling the timestamp
                    //                                 back, letting the same
                    //                                 Pyth update advance
                    //                                 baseline N times via
                    //                                 interleaved zero-fills.
                    // - last_hyperp_index_slot:       prevents dt-accumulation
                    //                                 attack on Hyperp index.
                    let mut restored = config_pre_oracle;
                    restored.last_good_oracle_slot = config.last_good_oracle_slot;
                    restored.last_effective_price_e6 = config.last_effective_price_e6;
                    restored.last_oracle_publish_time = config.last_oracle_publish_time;
                    restored.oracle_target_price_e6 = config.oracle_target_price_e6;
                    restored.oracle_target_publish_time = config.oracle_target_publish_time;
                    restored.last_hyperp_index_slot = config.last_hyperp_index_slot;
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

                    // Exposed market progress belongs to KeeperCrank. If this
                    // account-limited trade is allowed to proceed, accrue to
                    // clock.slot before execute_trade_with_matcher, which
                    // internally syncs both sides' fees before the trade.
                    ensure_market_accrued_to_now_for_account_limited_op(
                        engine,
                        &config,
                        clock.slot,
                        price,
                        funding_rate_e9_pre,
                    )?;

                    // Pre-sync fees BEFORE the ins_before snapshot to
                    // prevent maintenance-fee-inflated EWMA weight on
                    // small trades (see TradeNoCpi for rationale).
                    if config.maintenance_fee_per_slot > 0 {
                        engine
                            .sync_account_fee_to_slot_not_atomic(
                                user_idx,
                                clock.slot,
                                config.maintenance_fee_per_slot,
                            )
                            .map_err(map_risk_error)?;
                        engine
                            .sync_account_fee_to_slot_not_atomic(
                                lp_idx,
                                clock.slot,
                                config.maintenance_fee_per_slot,
                            )
                            .map_err(map_risk_error)?;
                    }

                    let trade_size = crate::policy::cpi_trade_size(ret.exec_size, size);
                    let current_fee_paid_cap = current_trade_fee_paid_cap(
                        trade_size,
                        exec_price,
                        engine.params.trading_fee_bps,
                    )?;

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
                        engine,
                        &matcher,
                        lp_idx,
                        user_idx,
                        clock.slot,
                        price,
                        trade_size,
                        funding_rate_e9_pre,
                        lp_account_id,
                        0,
                    )
                    .map_err(map_risk_error)?;
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
                    // Per-slot price-move cap is init-immutable (engine RiskParams).
                    let max_change_bps_cpi = engine.params.max_price_move_bps_per_slot;
                    if max_change_bps_cpi > 0 {
                        let clamped_exec = oracle::clamp_oracle_price(
                            crate::policy::mark_ewma_clamp_base(config.last_effective_price_e6),
                            ret.exec_price_e6,
                            max_change_bps_cpi,
                        );
                        // fee_paid = actual fee collected into insurance (post - pre).
                        let fee_paid_cpi = if config.mark_min_fee > 0 {
                            let ins_after_cpi = engine.insurance_fund.balance.get();
                            let delta = ins_after_cpi
                                .saturating_sub(ins_before_cpi)
                                .min(current_fee_paid_cap);
                            core::cmp::min(delta, u64::MAX as u128) as u64
                        } else {
                            0u64
                        };
                        // N4 fix: seed at oracle on first trade
                        let ewma_price_cpi =
                            if old_ewma_cpi == 0 && config.last_effective_price_e6 > 0 {
                                config.last_effective_price_e6
                            } else {
                                clamped_exec
                            };
                        config.mark_ewma_e6 = crate::policy::ewma_update(
                            old_ewma_cpi,
                            ewma_price_cpi,
                            config.mark_ewma_halflife_slots,
                            config.mark_ewma_last_slot,
                            clock.slot,
                            fee_paid_cpi,
                            config.mark_min_fee,
                        );
                        // Only full-weight observations advance the EWMA
                        // clock (Finding 7). Partial-alpha nudges from
                        // sub-threshold trades still mutate the EWMA
                        // value, but the clock is treated strictly as a
                        // liveness signal — otherwise dust trades on
                        // Hyperp markets refresh the soft-staleness check
                        // `max(mark_ewma_last_slot, last_mark_push_slot)`,
                        // keeping an otherwise-dead market alive. The
                        // minor EWMA-dt drift (next full-weight trade
                        // sees `dt = time_since_last_full_weight`, not
                        // `time_since_last_partial`) is an acceptable
                        // tradeoff.
                        let full_weight_observation =
                            config.mark_min_fee == 0 || fee_paid_cpi >= config.mark_min_fee;
                        if full_weight_observation {
                            config.mark_ewma_last_slot = clock.slot;
                        }
                        // NOTE: do NOT stamp funding rate here — execute_trade_not_atomic
                        // handles it via the funding_rate parameter (§5.5 anti-retroactivity).
                    }

                    // Hyperp: also update hyperp_mark_e6.
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
                        config.hyperp_mark_e6 = oracle::clamp_oracle_price(
                            config.last_effective_price_e6,
                            ret.exec_price_e6,
                            max_change_bps_cpi,
                        );
                        // Full-weight observation check: recompute here
                        // because the earlier `full_weight_observation`
                        // binding is in the cap>0 branch's scope.
                        let fee_paid_hyperp = if config.mark_min_fee > 0 {
                            let ins_after_cpi = engine.insurance_fund.balance.get();
                            let delta = ins_after_cpi
                                .saturating_sub(ins_before_cpi)
                                .min(current_fee_paid_cap);
                            core::cmp::min(delta, u64::MAX as u128) as u64
                        } else {
                            0u64
                        };
                        let full_weight =
                            config.mark_min_fee == 0 || fee_paid_hyperp >= config.mark_min_fee;
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
                    (
                        effective_pos_q_checked(engine, user_idx as usize)?,
                        effective_pos_q_checked(engine, lp_idx as usize)?,
                    )
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
                            let notional = risk_notional_ceil(eff, price);
                            buf.upsert(idx, notional);
                        }
                    }
                    state::write_risk_buffer(&mut data, &buf);
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
                    program_id,
                    a_slab.key,
                    config.vault_authority_bump,
                )?;
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                accounts::expect_key(a_pda, &auth)?;

                let resolved = zc::engine_ref(&data)?.is_resolved();
                let clock = Clock::from_account_info(&accounts[6])?;
                let mut funding_rate_e9 = 0i128;
                let price = if resolved {
                    let eng = zc::engine_ref(&data)?;
                    let (settlement, _) = eng.resolved_context();
                    if settlement == 0 {
                        return Err(ProgramError::InvalidAccountData);
                    }
                    settlement
                } else {
                    // Anti-retroactivity: capture funding rate before oracle read (§5.5)
                    funding_rate_e9 = compute_current_funding_rate_e9(&config)?;
                    let is_hyperp = oracle::is_hyperp_mode(&config);
                    let px = if is_hyperp {
                        let eng = zc::engine_ref(&data)?;
                        let p_last = eng.last_oracle_price;
                        let price_move_dt = price_move_residual_dt(eng, clock.slot)?;
                        let cap_bps = eng.params.max_price_move_bps_per_slot;
                        let oi_any = eng.oi_eff_long_q != 0 || eng.oi_eff_short_q != 0;
                        oracle::get_engine_oracle_price_e6(
                            p_last,
                            price_move_dt,
                            clock.slot,
                            clock.unix_timestamp,
                            &mut config,
                            a_oracle,
                            cap_bps,
                            oi_any,
                        )?
                    } else {
                        read_price_and_stamp(
                            &mut config,
                            a_oracle,
                            clock.unix_timestamp,
                            clock.slot,
                            &mut data,
                        )?
                    };
                    state::write_config(&mut data, &config);
                    px
                };

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, user_idx)?;

                // Owner authorization via policy helper
                let u_owner = engine.accounts[user_idx as usize].owner;
                if !crate::policy::owner_ok(u_owner, a_user.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                #[cfg(feature = "cu-audit")]
                {
                    msg!("CU_CHECKPOINT: close_account_start");
                    sol_log_compute_units();
                }
                let amt_units = if resolved {
                    // Engine fee-aware resolved close reconciles terminal
                    // K/F loss first, then syncs recurring fees to the
                    // resolved slot. Do not pre-sync here or fees become
                    // senior to same-account terminal losses.
                    match engine
                        .force_close_resolved_with_fee_not_atomic(
                            user_idx,
                            config.maintenance_fee_per_slot,
                        )
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
                    ensure_market_accrued_to_now_for_account_limited_op(
                        engine,
                        &config,
                        clock.slot,
                        price,
                        funding_rate_e9,
                    )?;
                    reject_any_target_lag(&config, engine)?;
                    // Realize due maintenance fees BEFORE close so the account
                    // cannot escape the unpaid interval by closing between cranks.
                    sync_account_fee(engine, &config, user_idx, clock.slot)?;
                    engine
                        .close_account_not_atomic(
                            user_idx,
                            clock.slot,
                            price,
                            funding_rate_e9,
                            admit_h_min,
                            admit_h_max,
                            Some(engine.params.maintenance_margin_bps as u128),
                        )
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

                if base_to_pay > 0 {
                    verify_token_account(a_user_ata, a_user.key, &mint)?;

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
                if amount == 0 {
                    return Err(ProgramError::InvalidArgument);
                }

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                // Block insurance top-up when market is resolved
                if zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }

                let mut config = state::read_config(&data);
                insurance_withdraw_deposits_only(&config)?;
                let mint = Pubkey::new_from_array(config.collateral_mint);

                let auth = accounts::derive_vault_authority_with_bump(
                    program_id,
                    a_slab.key,
                    config.vault_authority_bump,
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
                check_no_oracle_live_envelope(zc::engine_ref(&data)?, clock.slot)?;

                // Reject misaligned deposits — dust would be silently donated
                let (_units_check, dust_check) =
                    crate::units::base_to_units(amount, config.unit_scale);
                if dust_check != 0 {
                    return Err(ProgramError::InvalidArgument);
                }

                // Convert base tokens to units for engine
                let (units, _dust) = crate::units::base_to_units(amount, config.unit_scale);
                let new_deposit_remaining = config
                    .insurance_withdraw_deposit_remaining
                    .checked_add(units)
                    .ok_or(PercolatorError::EngineOverflow)?;

                // Transfer base tokens to vault
                collateral::deposit(a_token, a_user_ata, a_vault, a_user, amount)?;

                let engine = zc::engine_mut(&mut data)?;
                engine
                    .top_up_insurance_fund(units as u128, clock.slot)
                    .map_err(map_risk_error)?;
                config.insurance_withdraw_deposit_remaining = new_deposit_remaining;
                state::write_config(&mut data, &config);
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
                accounts::expect_writable(a_dest)?;
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

                    // CloseSlab is gated by `admin`. Operators who burn
                    // admin for rug-proofing trap the slab rent (~0.04 SOL),
                    // which is the accepted cost of the fully admin-free
                    // terminal state.
                    let header = state::read_header(&data);
                    require_admin(header.admin, a_dest.key)?;
                    let config = state::read_config(&data);

                    let mint = Pubkey::new_from_array(config.collateral_mint);
                    let auth = accounts::derive_vault_authority_with_bump(
                        program_id,
                        a_slab.key,
                        config.vault_authority_bump,
                    )?;
                    verify_vault(
                        a_vault,
                        &auth,
                        &mint,
                        &Pubkey::new_from_array(config.vault_pubkey),
                    )?;
                    accounts::expect_key(a_vault_auth, &auth)?;

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
                            a_dest.key, // rent destination
                            a_vault_auth.key,
                            &[],
                        )?;
                        solana_program::program::invoke_signed(
                            &close_ix,
                            &[
                                a_vault.clone(),
                                a_dest.clone(),
                                a_vault_auth.clone(),
                                a_token.clone(),
                            ],
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
                tvl_insurance_cap_mult,
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
                let funding_rate_e9 = compute_current_funding_rate_e9(&config)?;

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
                    let engine = zc::engine_ref(&data)?;
                    let max_change_bps = engine.params.max_price_move_bps_per_slot;
                    let mark = hyperp_target_price(&config);
                    if mark > 0 {
                        let anchor = if engine.last_oracle_price != 0 {
                            engine.last_oracle_price
                        } else if config.last_effective_price_e6 != 0 {
                            config.last_effective_price_e6
                        } else {
                            mark
                        };
                        let dt = price_move_residual_dt(engine, clock.slot)?;
                        let oi_any = engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0;
                        let new_index = if oi_any {
                            oracle::clamp_toward_engine_dt(anchor, mark, max_change_bps, dt)
                        } else {
                            mark
                        };
                        config.last_effective_price_e6 = new_index;
                        if new_index != anchor || new_index == mark {
                            config.last_hyperp_index_slot = clock.slot;
                        }
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
                            match read_price_and_stamp(
                                &mut config,
                                a_oracle,
                                clock.unix_timestamp,
                                clock.slot,
                                &mut data,
                            ) {
                                Ok(price) => {
                                    state::write_config(&mut data, &config);
                                    (price, funding_rate_e9)
                                }
                                Err(e) => {
                                    // v12.19.6: propagate the oracle error.
                                    // UpdateConfig must NOT enter a
                                    // degenerate-price + rate=0 arm on
                                    // OracleStale / OracleConfTooWide — that
                                    // would erase elapsed funding on a live
                                    // market (current-slot advances but no
                                    // accrual lands). Admin MUST route stale
                                    // markets through Resolve{Market,
                                    // Permissionless}; the hard-timeout gate
                                    // above is the "market is dead" signal.
                                    return Err(e);
                                }
                            }
                        };
                    if accrual_price > 0 {
                        {
                            let engine = zc::engine_mut(&mut data)?;
                            reject_stuck_target_accrual(
                                &config,
                                engine,
                                clock.slot,
                                accrual_price,
                            )?;
                            reject_account_limited_market_progress(
                                engine,
                                clock.slot,
                                accrual_price,
                                rate_for_accrual,
                            )?;
                            // Pre-chunk catch-up so accrue_market_to below sees
                            // dt ≤ max_accrual_dt_slots (Finding 4). Use the
                            // same (price, rate) the final accrue uses so the
                            // catchup chunks are consistent with the boundary.
                            catchup_accrue(engine, clock.slot, accrual_price, rate_for_accrual)?;
                            engine
                                .accrue_market_to(clock.slot, accrual_price, rate_for_accrual)
                                .map_err(map_risk_error)?;
                        }
                        // Engine processed real price — last_oracle_price is no longer sentinel
                        if !state::is_oracle_initialized(&data) {
                            state::set_oracle_initialized(&mut data);
                        }
                    }
                }

                reject_any_target_lag(&config, zc::engine_ref(&data)?)?;

                config.funding_horizon_slots = funding_horizon_slots;
                config.funding_k_bps = funding_k_bps;
                config.funding_max_premium_bps = funding_max_premium_bps;
                config.funding_max_e9_per_slot = funding_max_e9_per_slot;
                config.tvl_insurance_cap_mult = tvl_insurance_cap_mult;
                // Engine v12.18.1: accrue_market_to only updates market-global state
                // (K/F/slot_last). No per-account touches means no resets to
                // schedule or finalize, so the end-of-instruction lifecycle — which
                // the engine no longer exposes publicly — is structurally a no-op
                // on this path.
                state::write_config(&mut data, &config);
            }

            Instruction::PushHyperpMark {
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
                // Hyperp-only: `PushHyperpMark` is the admin mark-push
                // for internally-priced markets. Non-Hyperp markets
                // price exclusively off Pyth/Chainlink with no authority
                // path — the instruction is rejected.
                if !oracle::is_hyperp_mode(&config) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }
                if config.hyperp_authority == [0u8; 32]
                    || config.hyperp_authority != a_authority.key.to_bytes()
                {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }
                // `timestamp` is legacy wire data — non-Hyperp consumed
                // it as a staleness reference, Hyperp ignores it.
                let _ = timestamp;
                // Anti-retroactivity: capture funding rate before any config mutation (§5.5)
                let funding_rate_e9 = compute_current_funding_rate_e9(&config)?;
                // Hard-timeout gate: once clock.slot - last_live_slot >=
                // permissionless_resolve_stale_slots, the market is
                // TERMINALLY dead. Reject before any mutation.
                let push_clock = Clock::get().map_err(|_| ProgramError::UnsupportedSysvar)?;
                if oracle::permissionless_stale_matured(&config, push_clock.slot) {
                    return Err(PercolatorError::OracleStale.into());
                }
                // Flush index WITHOUT external staleness check
                // (the hard-timeout gate above covers mark staleness).
                let max_change_bps = zc::engine_ref(&data)?.params.max_price_move_bps_per_slot;
                {
                    let engine = zc::engine_ref(&data)?;
                    let mark = hyperp_target_price(&config);
                    if mark > 0 {
                        let anchor = if engine.last_oracle_price != 0 {
                            engine.last_oracle_price
                        } else if config.last_effective_price_e6 != 0 {
                            config.last_effective_price_e6
                        } else {
                            mark
                        };
                        let dt = price_move_residual_dt(engine, push_clock.slot)?;
                        let oi_any = engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0;
                        let new_index = if oi_any {
                            oracle::clamp_toward_engine_dt(anchor, mark, max_change_bps, dt)
                        } else {
                            mark
                        };
                        config.last_effective_price_e6 = new_index;
                        if new_index != anchor || new_index == mark {
                            config.last_hyperp_index_slot = push_clock.slot;
                        }
                    }
                    state::write_config(&mut data, &config);
                    config = state::read_config(&data);
                }

                if price_e6 == 0 {
                    return Err(PercolatorError::OracleInvalid.into());
                }

                let normalized_price =
                    crate::policy::to_engine_price(price_e6, config.invert, config.unit_scale)
                        .ok_or(PercolatorError::OracleInvalid)?;

                if normalized_price > percolator::MAX_ORACLE_PRICE {
                    return Err(PercolatorError::OracleInvalid.into());
                }

                // Hyperp stale-recovery policy (deliberate):
                //   If mark liveness has been lost beyond the catchup
                //   envelope while funding is active, the catchup_accrue
                //   call below can return CatchupRequired. Such a market
                //   is RESOLVE-ONLY (mark-staleness branch of
                //   ResolvePermissionless). Operators who want revivable
                //   Hyperp markets must push frequently enough to stay
                //   within the catchup envelope.
                {
                    let engine = zc::engine_mut(&mut data)?;
                    reject_stuck_target_accrual(
                        &config,
                        engine,
                        push_clock.slot,
                        config.last_effective_price_e6,
                    )?;
                    reject_account_limited_market_progress(
                        engine,
                        push_clock.slot,
                        config.last_effective_price_e6,
                        funding_rate_e9,
                    )?;
                    catchup_accrue(
                        engine,
                        push_clock.slot,
                        config.last_effective_price_e6,
                        funding_rate_e9,
                    )?;
                    engine
                        .accrue_market_to(
                            push_clock.slot,
                            config.last_effective_price_e6,
                            funding_rate_e9,
                        )
                        .map_err(map_risk_error)?;
                }

                // Clamp against index (last_effective_price_e6). This
                // bounds the mark-index gap to one cap-width regardless
                // of how many same-slot pushes occur; the index itself
                // only moves per-slot via clamp_toward_with_dt.
                let clamp_base = config.last_effective_price_e6;
                let clamped =
                    oracle::clamp_oracle_price(clamp_base, normalized_price, max_change_bps);
                config.hyperp_mark_e6 = clamped;
                config.last_mark_push_slot = push_clock.slot as u128;
                // Admin push feeds through EWMA like trades (full weight).
                config.mark_ewma_e6 = crate::policy::ewma_update(
                    config.mark_ewma_e6,
                    clamped,
                    config.mark_ewma_halflife_slots,
                    config.mark_ewma_last_slot,
                    push_clock.slot,
                    config.mark_min_fee,
                    config.mark_min_fee,
                );
                config.mark_ewma_last_slot = push_clock.slot;
                state::write_config(&mut data, &config);
            }

            Instruction::ResolveMarket { mode } => {
                // Resolve market: snapshot resolution slot, set RESOLVED flag.
                // Caller picks the §9.8 settlement arm explicitly via `mode`
                // (0 = Ordinary, 1 = Degenerate). The wrapper no longer
                // silently promotes a stale Ordinary call to Degenerate —
                // clients that want dead-oracle settlement must ask for it.
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

                // Require admin oracle price to be set (hyperp_mark_e6 > 0)
                let mut config = state::read_config(&data);
                // Per-slot price-move cap (init-immutable via RiskParams).
                let max_change_bps = zc::engine_ref(&data)?.params.max_price_move_bps_per_slot;
                // Anti-retroactivity: capture funding rate before any config mutation (§5.5)
                let funding_rate_e9 = compute_current_funding_rate_e9(&config)?;

                let clock_gate = Clock::from_account_info(a_clock)?;

                // Explicit Degenerate branch: settle at engine.last_oracle_price
                // with rate = 0. Used for dead-oracle markets (stale gate
                // matured, or admin-observed deadness). Mirrors
                // ResolvePermissionless's terminal settlement.
                if mode == 1 {
                    let stale_or_restarted =
                        oracle::permissionless_stale_matured(&config, clock_gate.slot);
                    if !stale_or_restarted {
                        if oracle::is_hyperp_mode(&config) {
                            let oracle_initialized = state::is_oracle_initialized(&data);
                            let last_update = core::cmp::max(
                                config.mark_ewma_last_slot,
                                config.last_mark_push_slot as u64,
                            );
                            let max_stale_slots = config.max_staleness_secs.saturating_mul(3);
                            let hyperp_stale = clock_gate.slot.saturating_sub(last_update)
                                > max_stale_slots
                                && oracle_initialized;
                            if !hyperp_stale {
                                return Err(PercolatorError::OracleInvalid.into());
                            }
                        } else {
                            let live = oracle::read_engine_price_e6(
                                a_oracle,
                                &config.index_feed_id,
                                clock_gate.unix_timestamp,
                                config.max_staleness_secs,
                                config.conf_filter_bps,
                                config.invert,
                                config.unit_scale,
                            );
                            match live {
                                Ok(_) => return Err(PercolatorError::OracleInvalid.into()),
                                Err(e)
                                    if e == ProgramError::from(PercolatorError::OracleStale)
                                        || e == ProgramError::from(
                                            PercolatorError::OracleConfTooWide,
                                        ) => {}
                                Err(e) => return Err(e),
                            }
                        }
                    }
                    let engine = zc::engine_mut(&mut data)?;
                    let p_last = engine.last_oracle_price;
                    if p_last == 0 {
                        return Err(PercolatorError::OracleInvalid.into());
                    }
                    engine
                        .resolve_market_not_atomic(
                            percolator::ResolveMode::Degenerate,
                            p_last,
                            p_last,
                            clock_gate.slot,
                            0,
                        )
                        .map_err(map_risk_error)?;
                    config.hyperp_mark_e6 = p_last;
                    state::write_config(&mut data, &config);
                    return Ok(());
                }

                // Ordinary branch: require live oracle. If the stale gate has
                // matured the caller must switch to Degenerate (mode = 1) —
                // we don't quietly settle against a dead oracle.
                if oracle::permissionless_stale_matured(&config, clock_gate.slot) {
                    return Err(PercolatorError::OracleStale.into());
                }

                // Hyperp markets need their mark initialized to settle.
                // Non-Hyperp markets settle at the fresh external oracle
                // (read below) — no pre-push required.
                if oracle::is_hyperp_mode(&config) && config.hyperp_mark_e6 == 0 {
                    return Err(ProgramError::InvalidAccountData);
                }
                // Read fresh external oracle for two purposes:
                // 1. Pass as live_oracle_price to the engine for
                //    self-synchronizing final accrual and the
                //    §9.8 deviation band check.
                // 2. Non-Hyperp: also serves as the settlement price
                //    (admin resolves at the current market price).
                // Hyperp: admin's pushed mark IS the price source;
                // live_oracle = index, settlement_price = EWMA/mark.
                // If the external oracle is genuinely dead (stale or
                // conf-too-wide), Ordinary rejects; callers must explicitly
                // select mode = 1 for the gated Degenerate recovery arm.
                // Other parse errors propagate (bad account, wrong feed).
                let mut fresh_live_oracle: Option<u64> = None;
                if !oracle::is_hyperp_mode(&config) {
                    let clock_tmp = Clock::from_account_info(a_clock)?;
                    let fresh = read_price_and_stamp(
                        &mut config,
                        a_oracle,
                        clock_tmp.unix_timestamp,
                        clock_tmp.slot,
                        &mut data,
                    )?;
                    fresh_live_oracle = Some(fresh);
                }

                let clock = Clock::from_account_info(a_clock)?;
                let mut config = config;

                // Flush Hyperp index to resolution slot WITHOUT staleness check.
                // Admin must be able to resolve even if mark is stale.
                if oracle::is_hyperp_mode(&config) {
                    let engine = zc::engine_ref(&data)?;
                    let mark = hyperp_target_price(&config);
                    if mark > 0 {
                        let anchor = if engine.last_oracle_price != 0 {
                            engine.last_oracle_price
                        } else if config.last_effective_price_e6 != 0 {
                            config.last_effective_price_e6
                        } else {
                            mark
                        };
                        let dt = price_move_residual_dt(engine, clock.slot)?;
                        let oi_any = engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0;
                        let new_index = if oi_any {
                            oracle::clamp_toward_engine_dt(anchor, mark, max_change_bps, dt)
                        } else {
                            mark
                        };
                        config.last_effective_price_e6 = new_index;
                        if new_index != anchor || new_index == mark {
                            config.last_hyperp_index_slot = clock.slot;
                        }
                    }
                    state::write_config(&mut data, &config);
                }

                // Determine canonical settlement price.
                //   Hyperp: mark EWMA (smoothed observable price), or
                //     hyperp_mark_e6 if EWMA is uninitialized.
                //   Non-Hyperp: the fresh external oracle reading
                //     (if available). If external is dead, the
                //     Degenerate arm below settles at
                //     engine.last_oracle_price.
                let settlement_price = if oracle::is_hyperp_mode(&config) {
                    let mark = config.mark_ewma_e6;
                    if mark > 0 {
                        mark
                    } else {
                        config.hyperp_mark_e6
                    }
                } else {
                    match fresh_live_oracle {
                        Some(fresh) => fresh,
                        None => {
                            let engine_r = zc::engine_ref(&data)?;
                            engine_r.last_oracle_price
                        }
                    }
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
                // the interval is signal-free. Ordinary rejects and the
                // caller must explicitly choose mode = 1, which is separately
                // gated above before Degenerate settlement at P_last/rate=0.
                let hyperp_stale = if is_hyperp_local {
                    let last_update = core::cmp::max(
                        config.mark_ewma_last_slot,
                        config.last_mark_push_slot as u64,
                    );
                    let max_stale_slots = config.max_staleness_secs.saturating_mul(3);
                    clock.slot.saturating_sub(last_update) > max_stale_slots && oracle_initialized
                } else {
                    false
                };

                // Mode = 0 (Ordinary) policy already stated above: require a
                // live input. If none is available (oracle never initialized,
                // Hyperp mark silent beyond 3× max_staleness, or external
                // oracle missing on non-Hyperp) the caller must switch to
                // mode = 1 (Degenerate); we no longer silently promote.
                let (live_oracle, rate_for_final_accrual): (u64, i128) = if let Some(fresh) =
                    fresh_live_oracle
                {
                    if config.oracle_target_price_e6 != 0 && fresh != config.oracle_target_price_e6
                    {
                        return Err(PercolatorError::CatchupRequired.into());
                    }
                    (fresh, funding_rate_e9)
                } else if is_hyperp_local && !hyperp_stale {
                    (config.last_effective_price_e6, funding_rate_e9)
                } else {
                    return Err(PercolatorError::OracleStale.into());
                };
                let _ = oracle_initialized;
                // Engine v12.18.5+: resolve_market_not_atomic takes an explicit
                // ResolveMode selector (Goal 51). This arm is always Ordinary;
                // Degenerate is reached only via the mode == 1 branch below.
                let resolve_mode = percolator::ResolveMode::Ordinary;
                // Pre-chunk catch-up. Ordinary accrues through the final step
                // and would otherwise hit Overflow when the gap exceeds
                // max_dt. Catchup uses the same (price, rate) the final
                // accrue will use, preserving anti-retroactivity (Finding 2).
                reject_stuck_target_accrual(&config, engine, clock.slot, live_oracle)?;
                catchup_accrue(engine, clock.slot, live_oracle, rate_for_final_accrual)?;
                engine
                    .resolve_market_not_atomic(
                        resolve_mode,
                        settlement_price,
                        live_oracle,
                        clock.slot,
                        rate_for_final_accrual,
                    )
                    .map_err(map_risk_error)?;

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
                    program_id,
                    a_slab.key,
                    config.vault_authority_bump,
                )?;
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_admin_ata, a_admin.key, &mint)?;
                accounts::expect_key(a_vault_pda, &auth)?;

                let payout_units = {
                    let engine = zc::engine_mut(&mut data)?;

                    // Require all accounts to be fully closed (not just
                    // effective_pos_q==0, which returns 0 for epoch-mismatched
                    // stale positions). Any used account means unsettled state
                    // may remain.
                    if engine.num_used_accounts != 0 {
                        return Err(ProgramError::InvalidAccountData);
                    }

                    engine
                        .withdraw_resolved_insurance_not_atomic()
                        .map_err(map_risk_error)?
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
                drop(data);

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

            Instruction::WithdrawInsuranceLimited { amount } => {
                // BOUNDED live insurance withdrawal.
                //
                // Auth: `header.insurance_operator` (distinct from
                // `insurance_authority` which gates the unbounded tag 20).
                // This auth split is structural — an operator with only
                // `insurance_operator` CANNOT bypass the bounds by
                // calling tag 20.
                //
                // Bounds:
                //   per-call amount ≤ max(10, bps_cap), clamped to insurance
                //     where bps_cap = insurance × max_bps / 10_000
                //   cooldown: clock.slot - last_withdraw_slot ≥ cooldown_slots
                //
                // The 10-unit floor (anti-Zeno) lets the fund drain to zero
                // even when max_bps × insurance < 10.
                //
                // Live markets only. For resolved markets use tag 20
                // (whose terminal-surplus sweep also folds residue into
                // the payout — not replicated here).
                const MIN_WITHDRAW_FLOOR_UNITS: u128 = 10;

                accounts::expect_len(accounts, 7)?;
                let a_operator = &accounts[0];
                let a_slab = &accounts[1];
                let a_operator_ata = &accounts[2];
                let a_vault = &accounts[3];
                let a_token = &accounts[4];
                let a_vault_pda = &accounts[5];
                let a_clock = &accounts[6];

                accounts::expect_signer(a_operator)?;
                accounts::expect_writable(a_slab)?;
                verify_token_program(a_token)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;

                // Live markets only. Resolved markets go through tag 20.
                if zc::engine_ref(&data)?.is_resolved() {
                    return Err(ProgramError::InvalidAccountData);
                }

                let header = state::read_header(&data);
                require_admin(header.insurance_operator, a_operator.key)?;

                let mut config = state::read_config(&data);
                let clock = Clock::from_account_info(a_clock)?;

                // Hard-timeout gate: don't mutate a matured market.
                if oracle::permissionless_stale_matured(&config, clock.slot) {
                    return Err(PercolatorError::OracleStale.into());
                }

                // Feature-disabled gate: max_bps == 0 means "bounded path
                // turned off." Operator must opt in at init or via
                // UpdateConfig (field is admin-set, not operator-set).
                if config.insurance_withdraw_max_bps == 0 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                let deposits_only = insurance_withdraw_deposits_only(&config)?;
                {
                    let engine = zc::engine_ref(&data)?;
                    reject_any_target_lag(&config, engine)?;
                    let oi_any = engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0;
                    if oi_any && engine.last_market_slot != clock.slot {
                        return Err(PercolatorError::CatchupRequired.into());
                    }
                    let stress_envelope_active = engine.stress_consumed_bps_e9_since_envelope != 0
                        || engine.stress_envelope_remaining_indices != 0
                        || engine.bankruptcy_hmax_lock_active;
                    if !crate::policy::live_insurance_withdraw_market_healthy(
                        engine.vault.get(),
                        engine.c_tot.get(),
                        engine.insurance_fund.balance.get(),
                        stress_envelope_active,
                    ) {
                        return Err(PercolatorError::EngineInsufficientBalance.into());
                    }
                }

                // Cooldown: first call (last_slot == 0) bypasses. Subsequent
                // calls require clock.slot - last ≥ cooldown_slots.
                let last = config.last_insurance_withdraw_slot;
                if last != 0
                    && clock.slot.saturating_sub(last) < config.insurance_withdraw_cooldown_slots
                {
                    return Err(PercolatorError::InsuranceWithdrawCooldown.into());
                }

                let (amount_units, dust) = crate::units::base_to_units(amount, config.unit_scale);
                if dust != 0 || amount_units == 0 {
                    return Err(ProgramError::InvalidArgument);
                }

                // Compute per-call cap: bps × insurance / 10_000, floor
                // lifted to MIN_WITHDRAW_FLOOR_UNITS, clamped to insurance.
                let ins = zc::engine_ref(&data)?.insurance_fund.balance.get();
                if ins == 0 {
                    return Err(PercolatorError::EngineInsufficientBalance.into());
                }
                let bps_cap =
                    ins.saturating_mul(config.insurance_withdraw_max_bps as u128) / 10_000;
                let cap = core::cmp::max(bps_cap, MIN_WITHDRAW_FLOOR_UNITS);
                let mut cap = core::cmp::min(cap, ins);
                if deposits_only {
                    cap = core::cmp::min(cap, config.insurance_withdraw_deposit_remaining as u128);
                }
                if (amount_units as u128) > cap {
                    return Err(PercolatorError::InsuranceWithdrawCapExceeded.into());
                }

                // Vault + ATA checks (reuse the pattern from tag 20).
                let mint = Pubkey::new_from_array(config.collateral_mint);
                let auth = accounts::derive_vault_authority_with_bump(
                    program_id,
                    a_slab.key,
                    config.vault_authority_bump,
                )?;
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                verify_token_account(a_operator_ata, a_operator.key, &mint)?;
                accounts::expect_key(a_vault_pda, &auth)?;

                // Commit state changes BEFORE the CPI. The CPI is SPL
                // Token, which cannot re-enter this program, so ordering
                // is safe; doing state-then-CPI means a CPI failure
                // reverts the whole tx atomically.
                {
                    let engine = zc::engine_mut(&mut data)?;
                    engine
                        .withdraw_live_insurance_not_atomic(amount_units as u128, clock.slot)
                        .map_err(map_risk_error)?;
                }
                config.last_insurance_withdraw_slot = clock.slot;
                if deposits_only {
                    config.insurance_withdraw_deposit_remaining = config
                        .insurance_withdraw_deposit_remaining
                        .checked_sub(amount_units)
                        .ok_or(PercolatorError::InsuranceWithdrawCapExceeded)?;
                } else {
                    config.insurance_withdraw_deposit_remaining = config
                        .insurance_withdraw_deposit_remaining
                        .saturating_sub(amount_units);
                }
                state::write_config(&mut data, &config);
                drop(data);

                // PDA-signed SPL Token transfer.
                let vault_tag: &[u8] = b"vault";
                let slab_bytes = a_slab.key.to_bytes();
                let seed1: &[u8] = vault_tag;
                let seed2: &[u8] = &slab_bytes;
                let bump_arr: [u8; 1] = [config.vault_authority_bump];
                let seed3: &[u8] = &bump_arr;
                let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
                let signer_seeds: [&[&[u8]]; 1] = [&seeds];

                collateral::withdraw(
                    a_token,
                    a_vault,
                    a_operator_ata,
                    a_vault_pda,
                    amount,
                    &signer_seeds,
                )?;
            }

            Instruction::AdminForceCloseAccount { user_idx } => {
                // Admin force-close an abandoned account after market resolution.
                // Settles PnL (with haircut for positive), forgives fee debt,
                // then delegates to engine.close_account_not_atomic() for the rest.
                accounts::expect_len(accounts, 7)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];
                let a_vault = &accounts[2];
                let a_owner_ata = &accounts[3];
                let a_pda = &accounts[4];
                let a_token = &accounts[5];

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
                    program_id,
                    a_slab.key,
                    config.vault_authority_bump,
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
                let (price, _resolved_slot) = engine.resolved_context();
                if price == 0 {
                    return Err(ProgramError::InvalidAccountData);
                }

                check_idx(engine, user_idx)?;

                let owner_pubkey = Pubkey::new_from_array(engine.accounts[user_idx as usize].owner);

                let amt_units = match engine
                    .force_close_resolved_with_fee_not_atomic(
                        user_idx,
                        config.maintenance_fee_per_slot,
                    )
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
                        program_id,
                        a_slab.key,
                        cfg.vault_authority_bump,
                    )?;
                    verify_vault(
                        a_vault,
                        &auth,
                        &mint,
                        &Pubkey::new_from_array(cfg.vault_pubkey),
                    )?;
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
                    if !crate::policy::owner_ok(owner, a_user.key.to_bytes()) {
                        return Err(PercolatorError::EngineUnauthorized.into());
                    }
                    check_no_oracle_live_envelope(engine, clock.slot)?;
                    // No-oracle path: sync fees at an anchor bounded by
                    // engine.last_market_slot (see sync_account_fee doc).
                    sync_account_fee_bounded_to_market(engine, &cfg, user_idx, clock.slot)?;
                    let fc = engine.accounts[user_idx as usize].fee_credits.get();
                    if fc > 0 || fc == i128::MIN {
                        return Err(PercolatorError::EngineCorruptState.into());
                    }
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

                // Phase 4: book the repayment in the engine.
                let mut data = state::slab_data_mut(a_slab)?;
                let config = state::read_config(&data);
                let clock = Clock::from_account_info(a_clock)?;
                let (units2, _dust) = crate::units::base_to_units(amount, config.unit_scale);
                let engine = zc::engine_mut(&mut data)?;
                let _ = &config; // Phase 1 synced; no second sync needed.
                engine
                    .deposit_fee_credits(user_idx, units2 as u128, clock.slot)
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
                let funding_rate_e9 = compute_current_funding_rate_e9(&config)?;
                let price = if is_hyperp {
                    let eng = zc::engine_ref(&data)?;
                    let p_last = eng.last_oracle_price;
                    let price_move_dt = price_move_residual_dt(eng, clock.slot)?;
                    let cap_bps = eng.params.max_price_move_bps_per_slot;
                    let oi_any = eng.oi_eff_long_q != 0 || eng.oi_eff_short_q != 0;
                    oracle::get_engine_oracle_price_e6(
                        p_last,
                        price_move_dt,
                        clock.slot,
                        clock.unix_timestamp,
                        &mut config,
                        a_oracle,
                        cap_bps,
                        oi_any,
                    )?
                } else {
                    read_price_and_stamp(
                        &mut config,
                        a_oracle,
                        clock.unix_timestamp,
                        clock.slot,
                        &mut data,
                    )?
                };
                state::write_config(&mut data, &config);

                let engine = zc::engine_mut(&mut data)?;
                check_idx(engine, user_idx)?;
                let owner = engine.accounts[user_idx as usize].owner;
                if !crate::policy::owner_ok(owner, a_user.key.to_bytes()) {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                let (units, dust) = crate::units::base_to_units(amount, config.unit_scale);
                if units == 0 || dust != 0 {
                    return Err(ProgramError::InvalidArgument);
                }
                let admit_h_min = engine.params.h_min;
                let admit_h_max = engine.params.h_max;
                // Fully accrue market to clock.slot BEFORE fee sync +
                // convert_released_pnl_not_atomic. Explicit accrue→sync→op.
                ensure_market_accrued_to_now_for_account_limited_op(
                    engine,
                    &config,
                    clock.slot,
                    price,
                    funding_rate_e9,
                )?;
                reject_any_target_lag(&config, engine)?;
                // Realize due maintenance fees BEFORE conversion so the
                // convertible-PnL bound reflects post-fee equity.
                sync_account_fee(engine, &config, user_idx, clock.slot)?;
                engine
                    .convert_released_pnl_not_atomic(
                        user_idx,
                        units as u128,
                        price,
                        clock.slot,
                        funding_rate_e9,
                        admit_h_min,
                        admit_h_max,
                        Some(engine.params.maintenance_margin_bps as u128),
                    )
                    .map_err(map_risk_error)?;
                if !state::is_oracle_initialized(&data) {
                    state::set_oracle_initialized(&mut data);
                }
            }

            Instruction::ResolvePermissionless => {
                // STRICT HARD-TIMEOUT POLICY:
                //
                //   clock.slot - last_live_slot >= permissionless_resolve_stale_slots
                //     → market is stale; anyone resolves at engine.last_oracle_price.
                //
                //   last_live_slot is:
                //     non-Hyperp → config.last_good_oracle_slot
                //                  (advances on successful external Pyth/Chainlink reads)
                //     Hyperp     → max(mark_ewma_last_slot, last_mark_push_slot)
                //                  (advances on full-weight trades and admin mark pushes)
                //
                // No challenge window, no oracle account submitted at resolve
                // time. If no one has fed the market a fresh price for N
                // slots, it's dead. Settlement is at engine.last_oracle_price
                // (the last price the engine actually accrued against).
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

                // A post-init cluster-restart (SIMD-0047 LastRestartSlot
                // bump) freezes the market unconditionally — bypass the
                // "feature disabled" gate so markets with no slot-based
                // staleness window can still be resolved after a restart.
                let restarted = oracle::cluster_restarted_since_init(&config);
                if !restarted && config.permissionless_resolve_stale_slots == 0 {
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
                engine
                    .resolve_market_not_atomic(
                        percolator::ResolveMode::Degenerate,
                        p_last,
                        p_last,
                        clock.slot,
                        0,
                    )
                    .map_err(map_risk_error)?;

                config.hyperp_mark_e6 = p_last;
                state::write_config(&mut data, &config);
            }

            Instruction::ForceCloseResolved { user_idx } => {
                // Permissionless force-close for resolved markets.
                // Mirrors AdminForceCloseAccount but requires delay and no admin.
                accounts::expect_len(accounts, 6)?;
                let a_slab = &accounts[0];
                let a_vault = &accounts[1];
                let a_owner_ata = &accounts[2];
                let a_pda = &accounts[3];
                let a_token = &accounts[4];
                let a_clock = &accounts[5];

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
                if !crate::policy::force_close_delay_elapsed(
                    clock.slot,
                    resolved_slot,
                    config.force_close_delay_slots,
                ) {
                    return Err(ProgramError::InvalidAccountData);
                }

                let mint = Pubkey::new_from_array(config.collateral_mint);
                let auth = accounts::derive_vault_authority_with_bump(
                    program_id,
                    a_slab.key,
                    config.vault_authority_bump,
                )?;
                verify_vault(
                    a_vault,
                    &auth,
                    &mint,
                    &Pubkey::new_from_array(config.vault_pubkey),
                )?;
                accounts::expect_key(a_pda, &auth)?;

                let engine = zc::engine_mut(&mut data)?;
                let (price, _resolved_slot) = engine.resolved_context();
                if price == 0 {
                    return Err(ProgramError::InvalidAccountData);
                }
                check_idx(engine, user_idx)?;

                let owner_pubkey = Pubkey::new_from_array(engine.accounts[user_idx as usize].owner);

                let amt_units = match engine
                    .force_close_resolved_with_fee_not_atomic(
                        user_idx,
                        config.maintenance_fee_per_slot,
                    )
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
    pub use crate::processor::{MatchingEngine, NoOpMatcher, TradeExecution};
    pub use percolator::{RiskEngine, RiskError, RiskParams};
}
