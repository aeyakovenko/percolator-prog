//! Pure policy helpers for program-level authorization and CPI binding.
//! Verbatim port of the legacy `mod policy` body (dedented).

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
    let effective_alpha_bps = if mark_min_fee == 0 || fee_paid >= mark_min_fee {
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
