//! Kani formal verification harnesses for percolator-prog.
//!
//! Run with: `cargo kani --tests`
//!
//! These harnesses prove PROGRAM-LEVEL security properties:
//! - Matcher ABI validation rejects malformed/malicious returns
//! - Owner/signer enforcement for all account operations
//! - Admin authorization and burned admin handling
//! - CPI identity binding (matcher program/context match LP registration)
//! - Matcher account shape validation
//! - PDA key mismatch rejection
//! - Nonce monotonicity (unchanged on failure, +1 on success)
//! - CPI uses exec_size (not requested size)
//!
//! Note: CPI execution and risk engine internals are NOT modeled.
//! Only wrapper-level authorization and binding logic is proven.

#![cfg(kani)]

extern crate kani;

// Import real types and helpers from the program crate
use percolator_prog::matcher_abi::{
    MatcherReturn, validate_matcher_return, FLAG_VALID, FLAG_PARTIAL_OK, FLAG_REJECTED,
};
use percolator_prog::constants::MATCHER_ABI_VERSION;
use percolator_prog::verify::{
    owner_ok, admin_ok, matcher_identity_ok, matcher_shape_ok, MatcherAccountsShape,
    gate_active, nonce_on_success, nonce_on_failure, pda_key_matches, cpi_trade_size,
    crank_authorized,
    // Account validation helpers
    signer_ok, writable_ok, len_ok,
    LpPdaShape, lp_pda_shape_ok, oracle_key_ok,
    SlabShape, slab_shape_ok,
    // Decision helpers for program-level coupling proofs
    single_owner_authorized, trade_authorized,
    TradeCpiDecision, decide_trade_cpi, decision_nonce,
    TradeNoCpiDecision, decide_trade_nocpi,
    SimpleDecision, decide_single_owner_op, decide_crank, decide_admin_op,
    // New: ABI validation from real inputs
    MatcherReturnFields, abi_ok, decide_trade_cpi_from_ret,
};

// =============================================================================
// Test Fixtures
// =============================================================================

/// Create a MatcherReturn from individual symbolic fields
fn any_matcher_return() -> MatcherReturn {
    MatcherReturn {
        abi_version: kani::any(),
        flags: kani::any(),
        exec_price_e6: kani::any(),
        exec_size: kani::any(),
        req_id: kani::any(),
        lp_account_id: kani::any(),
        oracle_price_e6: kani::any(),
        reserved: kani::any(),
    }
}

/// Create a MatcherReturnFields from individual symbolic fields
fn any_matcher_return_fields() -> MatcherReturnFields {
    MatcherReturnFields {
        abi_version: kani::any(),
        flags: kani::any(),
        exec_price_e6: kani::any(),
        exec_size: kani::any(),
        req_id: kani::any(),
        lp_account_id: kani::any(),
        oracle_price_e6: kani::any(),
        reserved: kani::any(),
    }
}

// =============================================================================
// A. MATCHER ABI VALIDATION (11 proofs - program-level, keep these)
// =============================================================================

/// Prove: wrong ABI version is always rejected
#[kani::proof]
fn kani_matcher_rejects_wrong_abi_version() {
    let mut ret = any_matcher_return();
    kani::assume(ret.abi_version != MATCHER_ABI_VERSION);

    let lp_account_id: u64 = kani::any();
    let oracle_price: u64 = kani::any();
    let req_size: i128 = kani::any();
    let req_id: u64 = kani::any();

    let result = validate_matcher_return(&ret, lp_account_id, oracle_price, req_size, req_id);
    assert!(result.is_err(), "wrong ABI version must be rejected");
}

/// Prove: missing VALID flag is always rejected
#[kani::proof]
fn kani_matcher_rejects_missing_valid_flag() {
    let mut ret = any_matcher_return();
    ret.abi_version = MATCHER_ABI_VERSION;
    kani::assume((ret.flags & FLAG_VALID) == 0);

    let lp_account_id: u64 = kani::any();
    let oracle_price: u64 = kani::any();
    let req_size: i128 = kani::any();
    let req_id: u64 = kani::any();

    let result = validate_matcher_return(&ret, lp_account_id, oracle_price, req_size, req_id);
    assert!(result.is_err(), "missing VALID flag must be rejected");
}

/// Prove: REJECTED flag always causes rejection
#[kani::proof]
fn kani_matcher_rejects_rejected_flag() {
    let mut ret = any_matcher_return();
    ret.abi_version = MATCHER_ABI_VERSION;
    ret.flags |= FLAG_VALID;
    ret.flags |= FLAG_REJECTED;

    let lp_account_id: u64 = kani::any();
    let oracle_price: u64 = kani::any();
    let req_size: i128 = kani::any();
    let req_id: u64 = kani::any();

    let result = validate_matcher_return(&ret, lp_account_id, oracle_price, req_size, req_id);
    assert!(result.is_err(), "REJECTED flag must cause rejection");
}

/// Prove: wrong req_id is always rejected
#[kani::proof]
fn kani_matcher_rejects_wrong_req_id() {
    let mut ret = any_matcher_return();
    ret.abi_version = MATCHER_ABI_VERSION;
    ret.flags = FLAG_VALID;
    ret.reserved = 0;
    kani::assume(ret.exec_price_e6 != 0);

    let lp_account_id: u64 = ret.lp_account_id;
    let oracle_price: u64 = ret.oracle_price_e6;
    let req_size: i128 = kani::any();
    kani::assume(req_size != 0);
    kani::assume(ret.exec_size != 0);
    kani::assume(ret.exec_size.signum() == req_size.signum());
    kani::assume(ret.exec_size.unsigned_abs() <= req_size.unsigned_abs());

    let req_id: u64 = kani::any();
    kani::assume(ret.req_id != req_id);

    let result = validate_matcher_return(&ret, lp_account_id, oracle_price, req_size, req_id);
    assert!(result.is_err(), "wrong req_id must be rejected");
}

/// Prove: wrong lp_account_id is always rejected
#[kani::proof]
fn kani_matcher_rejects_wrong_lp_account_id() {
    let mut ret = any_matcher_return();
    ret.abi_version = MATCHER_ABI_VERSION;
    ret.flags = FLAG_VALID;
    ret.reserved = 0;
    kani::assume(ret.exec_price_e6 != 0);

    let lp_account_id: u64 = kani::any();
    kani::assume(ret.lp_account_id != lp_account_id);

    let oracle_price: u64 = ret.oracle_price_e6;
    let req_size: i128 = kani::any();
    let req_id: u64 = ret.req_id;

    let result = validate_matcher_return(&ret, lp_account_id, oracle_price, req_size, req_id);
    assert!(result.is_err(), "wrong lp_account_id must be rejected");
}

/// Prove: wrong oracle_price is always rejected
#[kani::proof]
fn kani_matcher_rejects_wrong_oracle_price() {
    let mut ret = any_matcher_return();
    ret.abi_version = MATCHER_ABI_VERSION;
    ret.flags = FLAG_VALID;
    ret.reserved = 0;
    kani::assume(ret.exec_price_e6 != 0);

    let lp_account_id: u64 = ret.lp_account_id;
    let oracle_price: u64 = kani::any();
    kani::assume(ret.oracle_price_e6 != oracle_price);

    let req_size: i128 = kani::any();
    let req_id: u64 = ret.req_id;

    let result = validate_matcher_return(&ret, lp_account_id, oracle_price, req_size, req_id);
    assert!(result.is_err(), "wrong oracle_price must be rejected");
}

/// Prove: non-zero reserved field is always rejected
#[kani::proof]
fn kani_matcher_rejects_nonzero_reserved() {
    let mut ret = any_matcher_return();
    ret.abi_version = MATCHER_ABI_VERSION;
    ret.flags = FLAG_VALID;
    kani::assume(ret.exec_price_e6 != 0);
    kani::assume(ret.reserved != 0);

    let lp_account_id: u64 = ret.lp_account_id;
    let oracle_price: u64 = ret.oracle_price_e6;
    let req_size: i128 = kani::any();
    let req_id: u64 = ret.req_id;

    let result = validate_matcher_return(&ret, lp_account_id, oracle_price, req_size, req_id);
    assert!(result.is_err(), "non-zero reserved must be rejected");
}

/// Prove: zero exec_price is always rejected
#[kani::proof]
fn kani_matcher_rejects_zero_exec_price() {
    let mut ret = any_matcher_return();
    ret.abi_version = MATCHER_ABI_VERSION;
    ret.flags = FLAG_VALID;
    ret.reserved = 0;
    ret.exec_price_e6 = 0;

    let lp_account_id: u64 = ret.lp_account_id;
    let oracle_price: u64 = ret.oracle_price_e6;
    let req_size: i128 = kani::any();
    let req_id: u64 = ret.req_id;

    let result = validate_matcher_return(&ret, lp_account_id, oracle_price, req_size, req_id);
    assert!(result.is_err(), "zero exec_price must be rejected");
}

/// Prove: zero exec_size without PARTIAL_OK is rejected
#[kani::proof]
fn kani_matcher_zero_size_requires_partial_ok() {
    let mut ret = any_matcher_return();
    ret.abi_version = MATCHER_ABI_VERSION;
    ret.flags = FLAG_VALID; // No PARTIAL_OK
    ret.reserved = 0;
    kani::assume(ret.exec_price_e6 != 0);
    ret.exec_size = 0;

    let lp_account_id: u64 = ret.lp_account_id;
    let oracle_price: u64 = ret.oracle_price_e6;
    let req_size: i128 = kani::any();
    let req_id: u64 = ret.req_id;

    let result = validate_matcher_return(&ret, lp_account_id, oracle_price, req_size, req_id);
    assert!(result.is_err(), "zero exec_size without PARTIAL_OK must be rejected");
}

/// Prove: exec_size exceeding req_size is rejected
#[kani::proof]
fn kani_matcher_rejects_exec_size_exceeds_req() {
    let mut ret = any_matcher_return();
    ret.abi_version = MATCHER_ABI_VERSION;
    ret.flags = FLAG_VALID;
    ret.reserved = 0;
    kani::assume(ret.exec_price_e6 != 0);
    kani::assume(ret.exec_size != 0);

    let lp_account_id: u64 = ret.lp_account_id;
    let oracle_price: u64 = ret.oracle_price_e6;
    let req_id: u64 = ret.req_id;

    let req_size: i128 = kani::any();
    kani::assume(ret.exec_size.unsigned_abs() > req_size.unsigned_abs());

    let result = validate_matcher_return(&ret, lp_account_id, oracle_price, req_size, req_id);
    assert!(result.is_err(), "exec_size exceeding req_size must be rejected");
}

/// Prove: sign mismatch between exec_size and req_size is rejected
#[kani::proof]
fn kani_matcher_rejects_sign_mismatch() {
    let mut ret = any_matcher_return();
    ret.abi_version = MATCHER_ABI_VERSION;
    ret.flags = FLAG_VALID;
    ret.reserved = 0;
    kani::assume(ret.exec_price_e6 != 0);
    kani::assume(ret.exec_size != 0);

    let lp_account_id: u64 = ret.lp_account_id;
    let oracle_price: u64 = ret.oracle_price_e6;
    let req_id: u64 = ret.req_id;

    let req_size: i128 = kani::any();
    kani::assume(req_size != 0);
    kani::assume(ret.exec_size.signum() != req_size.signum());
    kani::assume(ret.exec_size.unsigned_abs() <= req_size.unsigned_abs());

    let result = validate_matcher_return(&ret, lp_account_id, oracle_price, req_size, req_id);
    assert!(result.is_err(), "sign mismatch must be rejected");
}

// =============================================================================
// B. OWNER/SIGNER ENFORCEMENT (2 proofs)
// =============================================================================

/// Prove: owner mismatch is rejected
#[kani::proof]
fn kani_owner_mismatch_rejected() {
    let stored: [u8; 32] = kani::any();
    let signer: [u8; 32] = kani::any();
    kani::assume(stored != signer);

    assert!(
        !owner_ok(stored, signer),
        "owner mismatch must be rejected"
    );
}

/// Prove: owner match is accepted
#[kani::proof]
fn kani_owner_match_accepted() {
    let owner: [u8; 32] = kani::any();

    assert!(
        owner_ok(owner, owner),
        "owner match must be accepted"
    );
}

// =============================================================================
// C. ADMIN AUTHORIZATION (3 proofs)
// =============================================================================

/// Prove: admin mismatch is rejected
#[kani::proof]
fn kani_admin_mismatch_rejected() {
    let admin: [u8; 32] = kani::any();
    let signer: [u8; 32] = kani::any();
    kani::assume(admin != [0u8; 32]); // Not burned
    kani::assume(admin != signer);

    assert!(
        !admin_ok(admin, signer),
        "admin mismatch must be rejected"
    );
}

/// Prove: admin match is accepted (when not burned)
#[kani::proof]
fn kani_admin_match_accepted() {
    let admin: [u8; 32] = kani::any();
    kani::assume(admin != [0u8; 32]); // Not burned

    assert!(
        admin_ok(admin, admin),
        "admin match must be accepted"
    );
}

/// Prove: burned admin (all zeros) disables all admin ops
#[kani::proof]
fn kani_admin_burned_disables_ops() {
    let burned_admin = [0u8; 32];
    let signer: [u8; 32] = kani::any();

    assert!(
        !admin_ok(burned_admin, signer),
        "burned admin must disable all admin ops"
    );
}

// =============================================================================
// D. CPI IDENTITY BINDING (2 proofs) - CRITICAL
// =============================================================================

/// Prove: CPI matcher identity mismatch (program or context) is rejected
#[kani::proof]
fn kani_matcher_identity_mismatch_rejected() {
    let lp_prog: [u8; 32] = kani::any();
    let lp_ctx: [u8; 32] = kani::any();
    let provided_prog: [u8; 32] = kani::any();
    let provided_ctx: [u8; 32] = kani::any();

    // At least one must mismatch
    kani::assume(lp_prog != provided_prog || lp_ctx != provided_ctx);

    assert!(
        !matcher_identity_ok(lp_prog, lp_ctx, provided_prog, provided_ctx),
        "matcher identity mismatch must be rejected"
    );
}

/// Prove: CPI matcher identity match is accepted
#[kani::proof]
fn kani_matcher_identity_match_accepted() {
    let prog: [u8; 32] = kani::any();
    let ctx: [u8; 32] = kani::any();

    assert!(
        matcher_identity_ok(prog, ctx, prog, ctx),
        "matcher identity match must be accepted"
    );
}

// =============================================================================
// E. MATCHER ACCOUNT SHAPE VALIDATION (5 proofs)
// =============================================================================

/// Prove: non-executable matcher program is rejected
#[kani::proof]
fn kani_matcher_shape_rejects_non_executable_prog() {
    let shape = MatcherAccountsShape {
        prog_executable: false, // BAD
        ctx_executable: false,
        ctx_owner_is_prog: true,
        ctx_len_ok: true,
    };

    assert!(
        !matcher_shape_ok(shape),
        "non-executable matcher program must be rejected"
    );
}

/// Prove: executable matcher context is rejected
#[kani::proof]
fn kani_matcher_shape_rejects_executable_ctx() {
    let shape = MatcherAccountsShape {
        prog_executable: true,
        ctx_executable: true, // BAD
        ctx_owner_is_prog: true,
        ctx_len_ok: true,
    };

    assert!(
        !matcher_shape_ok(shape),
        "executable matcher context must be rejected"
    );
}

/// Prove: context not owned by program is rejected
#[kani::proof]
fn kani_matcher_shape_rejects_wrong_ctx_owner() {
    let shape = MatcherAccountsShape {
        prog_executable: true,
        ctx_executable: false,
        ctx_owner_is_prog: false, // BAD
        ctx_len_ok: true,
    };

    assert!(
        !matcher_shape_ok(shape),
        "context not owned by program must be rejected"
    );
}

/// Prove: insufficient context length is rejected
#[kani::proof]
fn kani_matcher_shape_rejects_short_ctx() {
    let shape = MatcherAccountsShape {
        prog_executable: true,
        ctx_executable: false,
        ctx_owner_is_prog: true,
        ctx_len_ok: false, // BAD
    };

    assert!(
        !matcher_shape_ok(shape),
        "insufficient context length must be rejected"
    );
}

/// Prove: valid matcher shape is accepted
#[kani::proof]
fn kani_matcher_shape_valid_accepted() {
    let shape = MatcherAccountsShape {
        prog_executable: true,
        ctx_executable: false,
        ctx_owner_is_prog: true,
        ctx_len_ok: true,
    };

    assert!(
        matcher_shape_ok(shape),
        "valid matcher shape must be accepted"
    );
}

// =============================================================================
// F. PDA KEY MATCHING (2 proofs)
// =============================================================================

/// Prove: PDA key mismatch is rejected
#[kani::proof]
fn kani_pda_mismatch_rejected() {
    let expected: [u8; 32] = kani::any();
    let provided: [u8; 32] = kani::any();
    kani::assume(expected != provided);

    assert!(
        !pda_key_matches(expected, provided),
        "PDA key mismatch must be rejected"
    );
}

/// Prove: PDA key match is accepted
#[kani::proof]
fn kani_pda_match_accepted() {
    let key: [u8; 32] = kani::any();

    assert!(
        pda_key_matches(key, key),
        "PDA key match must be accepted"
    );
}

// =============================================================================
// G. NONCE MONOTONICITY (3 proofs)
// =============================================================================

/// Prove: nonce unchanged on failure
#[kani::proof]
fn kani_nonce_unchanged_on_failure() {
    let old_nonce: u64 = kani::any();
    let new_nonce = nonce_on_failure(old_nonce);

    assert_eq!(
        new_nonce, old_nonce,
        "nonce must be unchanged on failure"
    );
}

/// Prove: nonce advances by exactly 1 on success
#[kani::proof]
fn kani_nonce_advances_on_success() {
    let old_nonce: u64 = kani::any();
    let new_nonce = nonce_on_success(old_nonce);

    assert_eq!(
        new_nonce,
        old_nonce.wrapping_add(1),
        "nonce must advance by 1 on success"
    );
}

/// Prove: nonce wraps correctly at u64::MAX
#[kani::proof]
fn kani_nonce_wraps_at_max() {
    let old_nonce = u64::MAX;
    let new_nonce = nonce_on_success(old_nonce);

    assert_eq!(
        new_nonce, 0,
        "nonce must wrap to 0 at u64::MAX"
    );
}

// =============================================================================
// H. CPI USES EXEC_SIZE (1 proof) - CRITICAL
// =============================================================================

/// Prove: CPI path uses exec_size from matcher, not requested size
#[kani::proof]
fn kani_cpi_uses_exec_size() {
    let exec_size: i128 = kani::any();
    let requested_size: i128 = kani::any();

    // Even when they differ, cpi_trade_size returns exec_size
    let chosen = cpi_trade_size(exec_size, requested_size);

    assert_eq!(
        chosen, exec_size,
        "CPI must use exec_size, not requested size"
    );
}

// =============================================================================
// I. GATE ACTIVATION LOGIC (3 proofs)
// =============================================================================

/// Prove: gate not active when threshold is zero
#[kani::proof]
fn kani_gate_inactive_when_threshold_zero() {
    let balance: u128 = kani::any();

    assert!(
        !gate_active(0, balance),
        "gate must be inactive when threshold is zero"
    );
}

/// Prove: gate not active when balance exceeds threshold
#[kani::proof]
fn kani_gate_inactive_when_balance_exceeds() {
    let threshold: u128 = kani::any();
    let balance: u128 = kani::any();
    kani::assume(balance > threshold);

    assert!(
        !gate_active(threshold, balance),
        "gate must be inactive when balance > threshold"
    );
}

/// Prove: gate active when threshold > 0 and balance <= threshold
#[kani::proof]
fn kani_gate_active_when_conditions_met() {
    let threshold: u128 = kani::any();
    kani::assume(threshold > 0);
    let balance: u128 = kani::any();
    kani::assume(balance <= threshold);

    assert!(
        gate_active(threshold, balance),
        "gate must be active when threshold > 0 and balance <= threshold"
    );
}

// =============================================================================
// J. KEEPER CRANK AUTHORIZATION (3 proofs)
// =============================================================================

/// Prove: crank authorized when account doesn't exist
#[kani::proof]
fn kani_crank_authorized_no_account() {
    let signer: [u8; 32] = kani::any();
    let stored_owner: [u8; 32] = kani::any();

    assert!(
        crank_authorized(false, stored_owner, signer),
        "crank must be authorized when account doesn't exist"
    );
}

/// Prove: crank authorized when signer matches owner
#[kani::proof]
fn kani_crank_authorized_owner_match() {
    let owner: [u8; 32] = kani::any();

    assert!(
        crank_authorized(true, owner, owner),
        "crank must be authorized when signer matches owner"
    );
}

/// Prove: crank rejected when signer doesn't match owner
#[kani::proof]
fn kani_crank_rejected_owner_mismatch() {
    let stored_owner: [u8; 32] = kani::any();
    let signer: [u8; 32] = kani::any();
    kani::assume(stored_owner != signer);

    assert!(
        !crank_authorized(true, stored_owner, signer),
        "crank must be rejected when signer doesn't match existing account owner"
    );
}

// =============================================================================
// K. PER-INSTRUCTION AUTHORIZATION (4 proofs)
// =============================================================================

/// Prove: single-owner instruction rejects on mismatch
#[kani::proof]
fn kani_single_owner_mismatch_rejected() {
    let stored: [u8; 32] = kani::any();
    let signer: [u8; 32] = kani::any();
    kani::assume(stored != signer);

    assert!(
        !single_owner_authorized(stored, signer),
        "single-owner instruction must reject on mismatch"
    );
}

/// Prove: single-owner instruction accepts on match
#[kani::proof]
fn kani_single_owner_match_accepted() {
    let owner: [u8; 32] = kani::any();

    assert!(
        single_owner_authorized(owner, owner),
        "single-owner instruction must accept on match"
    );
}

/// Prove: trade rejects when user owner mismatch
#[kani::proof]
fn kani_trade_rejects_user_mismatch() {
    let user_owner: [u8; 32] = kani::any();
    let user_signer: [u8; 32] = kani::any();
    let lp_owner: [u8; 32] = kani::any();
    kani::assume(user_owner != user_signer);

    assert!(
        !trade_authorized(user_owner, user_signer, lp_owner, lp_owner),
        "trade must reject when user owner doesn't match"
    );
}

/// Prove: trade rejects when LP owner mismatch
#[kani::proof]
fn kani_trade_rejects_lp_mismatch() {
    let user_owner: [u8; 32] = kani::any();
    let lp_owner: [u8; 32] = kani::any();
    let lp_signer: [u8; 32] = kani::any();
    kani::assume(lp_owner != lp_signer);

    assert!(
        !trade_authorized(user_owner, user_owner, lp_owner, lp_signer),
        "trade must reject when LP owner doesn't match"
    );
}

// =============================================================================
// L. TRADECPI DECISION COUPLING (12 proofs) - CRITICAL
// These prove program-level policies, not just helper semantics
// =============================================================================

/// Helper: create a valid shape for testing other conditions
fn valid_shape() -> MatcherAccountsShape {
    MatcherAccountsShape {
        prog_executable: true,
        ctx_executable: false,
        ctx_owner_is_prog: true,
        ctx_len_ok: true,
    }
}

/// Prove: TradeCpi rejects on bad matcher shape (non-executable prog)
#[kani::proof]
fn kani_tradecpi_rejects_non_executable_prog() {
    let old_nonce: u64 = kani::any();
    let shape = MatcherAccountsShape {
        prog_executable: false, // BAD
        ctx_executable: false,
        ctx_owner_is_prog: true,
        ctx_len_ok: true,
    };
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce, shape, true, true, true, true, true, false, false, exec_size
    );

    assert_eq!(decision, TradeCpiDecision::Reject,
        "TradeCpi must reject non-executable matcher program");
}

/// Prove: TradeCpi rejects on bad matcher shape (executable ctx)
#[kani::proof]
fn kani_tradecpi_rejects_executable_ctx() {
    let old_nonce: u64 = kani::any();
    let shape = MatcherAccountsShape {
        prog_executable: true,
        ctx_executable: true, // BAD
        ctx_owner_is_prog: true,
        ctx_len_ok: true,
    };
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce, shape, true, true, true, true, true, false, false, exec_size
    );

    assert_eq!(decision, TradeCpiDecision::Reject,
        "TradeCpi must reject executable matcher context");
}

/// Prove: TradeCpi rejects on PDA mismatch (even if everything else valid)
#[kani::proof]
fn kani_tradecpi_rejects_pda_mismatch() {
    let old_nonce: u64 = kani::any();
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce, valid_shape(),
        true,  // identity_ok
        false, // pda_ok - BAD
        true,  // abi_ok
        true,  // user_auth_ok
        true,  // lp_auth_ok
        false, // gate_active
        false, // risk_increase
        exec_size
    );

    assert_eq!(decision, TradeCpiDecision::Reject,
        "TradeCpi must reject PDA mismatch");
}

/// Prove: TradeCpi rejects on user auth failure
#[kani::proof]
fn kani_tradecpi_rejects_user_auth_failure() {
    let old_nonce: u64 = kani::any();
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce, valid_shape(),
        true,  // identity_ok
        true,  // pda_ok
        true,  // abi_ok
        false, // user_auth_ok - BAD
        true,  // lp_auth_ok
        false, // gate_active
        false, // risk_increase
        exec_size
    );

    assert_eq!(decision, TradeCpiDecision::Reject,
        "TradeCpi must reject user auth failure");
}

/// Prove: TradeCpi rejects on LP auth failure
#[kani::proof]
fn kani_tradecpi_rejects_lp_auth_failure() {
    let old_nonce: u64 = kani::any();
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce, valid_shape(),
        true,  // identity_ok
        true,  // pda_ok
        true,  // abi_ok
        true,  // user_auth_ok
        false, // lp_auth_ok - BAD
        false, // gate_active
        false, // risk_increase
        exec_size
    );

    assert_eq!(decision, TradeCpiDecision::Reject,
        "TradeCpi must reject LP auth failure");
}

/// Prove: TradeCpi rejects on identity mismatch (even if ABI valid)
#[kani::proof]
fn kani_tradecpi_rejects_identity_mismatch() {
    let old_nonce: u64 = kani::any();
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce, valid_shape(),
        false, // identity_ok - BAD
        true,  // pda_ok
        true,  // abi_ok (strong adversary: valid ABI but wrong identity)
        true,  // user_auth_ok
        true,  // lp_auth_ok
        false, // gate_active
        false, // risk_increase
        exec_size
    );

    assert_eq!(decision, TradeCpiDecision::Reject,
        "TradeCpi must reject identity mismatch even if ABI valid");
}

/// Prove: TradeCpi rejects on ABI validation failure
#[kani::proof]
fn kani_tradecpi_rejects_abi_failure() {
    let old_nonce: u64 = kani::any();
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce, valid_shape(),
        true,  // identity_ok
        true,  // pda_ok
        false, // abi_ok - BAD
        true,  // user_auth_ok
        true,  // lp_auth_ok
        false, // gate_active
        false, // risk_increase
        exec_size
    );

    assert_eq!(decision, TradeCpiDecision::Reject,
        "TradeCpi must reject ABI validation failure");
}

/// Prove: TradeCpi rejects on gate active + risk increase
#[kani::proof]
fn kani_tradecpi_rejects_gate_risk_increase() {
    let old_nonce: u64 = kani::any();
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce, valid_shape(),
        true,  // identity_ok
        true,  // pda_ok
        true,  // abi_ok
        true,  // user_auth_ok
        true,  // lp_auth_ok
        true,  // gate_active - ACTIVE
        true,  // risk_increase - INCREASING
        exec_size
    );

    assert_eq!(decision, TradeCpiDecision::Reject,
        "TradeCpi must reject when gate active and risk increasing");
}

/// Prove: TradeCpi allows risk-reducing trade when gate active
#[kani::proof]
fn kani_tradecpi_allows_gate_risk_decrease() {
    let old_nonce: u64 = kani::any();
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce, valid_shape(),
        true,  // identity_ok
        true,  // pda_ok
        true,  // abi_ok
        true,  // user_auth_ok
        true,  // lp_auth_ok
        true,  // gate_active
        false, // risk_increase - NOT increasing (reducing or neutral)
        exec_size
    );

    assert!(matches!(decision, TradeCpiDecision::Accept { .. }),
        "TradeCpi must allow risk-reducing trade when gate active");
}

/// Prove: TradeCpi reject leaves nonce unchanged
#[kani::proof]
fn kani_tradecpi_reject_nonce_unchanged() {
    let old_nonce: u64 = kani::any();
    let exec_size: i128 = kani::any();

    // Force a rejection (bad shape)
    let bad_shape = MatcherAccountsShape {
        prog_executable: false,
        ctx_executable: false,
        ctx_owner_is_prog: true,
        ctx_len_ok: true,
    };

    let decision = decide_trade_cpi(
        old_nonce, bad_shape, true, true, true, true, true, false, false, exec_size
    );

    let result_nonce = decision_nonce(old_nonce, decision);

    assert_eq!(result_nonce, old_nonce,
        "TradeCpi reject must leave nonce unchanged");
}

/// Prove: TradeCpi accept increments nonce
#[kani::proof]
fn kani_tradecpi_accept_increments_nonce() {
    let old_nonce: u64 = kani::any();
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce, valid_shape(),
        true, true, true, true, true, false, false, exec_size
    );

    assert!(matches!(decision, TradeCpiDecision::Accept { .. }),
        "should accept with all valid inputs");

    let result_nonce = decision_nonce(old_nonce, decision);

    assert_eq!(result_nonce, old_nonce.wrapping_add(1),
        "TradeCpi accept must increment nonce by 1");
}

/// Prove: TradeCpi accept uses exec_size
#[kani::proof]
fn kani_tradecpi_accept_uses_exec_size() {
    let old_nonce: u64 = kani::any();
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce, valid_shape(),
        true, true, true, true, true, false, false, exec_size
    );

    if let TradeCpiDecision::Accept { chosen_size, .. } = decision {
        assert_eq!(chosen_size, exec_size,
            "TradeCpi accept must use exec_size");
    } else {
        panic!("expected Accept");
    }
}

// =============================================================================
// M. TRADENOCPI DECISION COUPLING (4 proofs)
// =============================================================================

/// Prove: TradeNoCpi rejects on user auth failure
#[kani::proof]
fn kani_tradenocpi_rejects_user_auth_failure() {
    let decision = decide_trade_nocpi(false, true, false, false);
    assert_eq!(decision, TradeNoCpiDecision::Reject,
        "TradeNoCpi must reject user auth failure");
}

/// Prove: TradeNoCpi rejects on LP auth failure
#[kani::proof]
fn kani_tradenocpi_rejects_lp_auth_failure() {
    let decision = decide_trade_nocpi(true, false, false, false);
    assert_eq!(decision, TradeNoCpiDecision::Reject,
        "TradeNoCpi must reject LP auth failure");
}

/// Prove: TradeNoCpi rejects on gate active + risk increase
#[kani::proof]
fn kani_tradenocpi_rejects_gate_risk_increase() {
    let decision = decide_trade_nocpi(true, true, true, true);
    assert_eq!(decision, TradeNoCpiDecision::Reject,
        "TradeNoCpi must reject when gate active and risk increasing");
}

/// Prove: TradeNoCpi accepts when all checks pass
#[kani::proof]
fn kani_tradenocpi_accepts_valid() {
    let decision = decide_trade_nocpi(true, true, false, false);
    assert_eq!(decision, TradeNoCpiDecision::Accept,
        "TradeNoCpi must accept when all checks pass");
}

// =============================================================================
// N. ZERO SIZE WITH PARTIAL_OK (1 proof)
// =============================================================================

/// Prove: zero exec_size with PARTIAL_OK flag is accepted
#[kani::proof]
fn kani_matcher_zero_size_with_partial_ok_accepted() {
    let mut ret = any_matcher_return();
    ret.abi_version = MATCHER_ABI_VERSION;
    ret.flags = FLAG_VALID | FLAG_PARTIAL_OK;
    ret.reserved = 0;
    kani::assume(ret.exec_price_e6 != 0);
    ret.exec_size = 0;

    let lp_account_id: u64 = ret.lp_account_id;
    let oracle_price: u64 = ret.oracle_price_e6;
    // When exec_size == 0, validate_matcher_return returns early before abs() checks
    // so req_size can be any value including i128::MIN
    let req_size: i128 = kani::any();
    let req_id: u64 = ret.req_id;

    let result = validate_matcher_return(&ret, lp_account_id, oracle_price, req_size, req_id);
    assert!(result.is_ok(), "zero exec_size with PARTIAL_OK must be accepted");
}

// =============================================================================
// O. MISSING SHAPE COUPLING PROOFS (2 proofs)
// =============================================================================

/// Prove: TradeCpi rejects on bad matcher shape (ctx owner mismatch)
#[kani::proof]
fn kani_tradecpi_rejects_ctx_owner_mismatch() {
    let old_nonce: u64 = kani::any();
    let shape = MatcherAccountsShape {
        prog_executable: true,
        ctx_executable: false,
        ctx_owner_is_prog: false, // BAD - context not owned by program
        ctx_len_ok: true,
    };
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce, shape, true, true, true, true, true, false, false, exec_size
    );

    assert_eq!(decision, TradeCpiDecision::Reject,
        "TradeCpi must reject when context not owned by matcher program");
}

/// Prove: TradeCpi rejects on bad matcher shape (ctx too short)
#[kani::proof]
fn kani_tradecpi_rejects_ctx_len_short() {
    let old_nonce: u64 = kani::any();
    let shape = MatcherAccountsShape {
        prog_executable: true,
        ctx_executable: false,
        ctx_owner_is_prog: true,
        ctx_len_ok: false, // BAD - context length insufficient
    };
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce, shape, true, true, true, true, true, false, false, exec_size
    );

    assert_eq!(decision, TradeCpiDecision::Reject,
        "TradeCpi must reject when context length insufficient");
}

// =============================================================================
// P. UNIVERSAL REJECT => NONCE UNCHANGED (1 proof)
// This subsumes all specific "reject => nonce unchanged" proofs
// =============================================================================

/// Prove: ANY TradeCpi rejection leaves nonce unchanged (universal quantification)
#[kani::proof]
fn kani_tradecpi_any_reject_nonce_unchanged() {
    let old_nonce: u64 = kani::any();

    // Build shape from symbolic bools (MatcherAccountsShape doesn't impl kani::Arbitrary)
    let shape = MatcherAccountsShape {
        prog_executable: kani::any(),
        ctx_executable: kani::any(),
        ctx_owner_is_prog: kani::any(),
        ctx_len_ok: kani::any(),
    };

    let identity_ok: bool = kani::any();
    let pda_ok: bool = kani::any();
    let abi_ok: bool = kani::any();
    let user_auth_ok: bool = kani::any();
    let lp_auth_ok: bool = kani::any();
    let gate_active: bool = kani::any();
    let risk_increase: bool = kani::any();
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce, shape, identity_ok, pda_ok, abi_ok,
        user_auth_ok, lp_auth_ok, gate_active, risk_increase, exec_size
    );

    // Only consider rejection cases
    kani::assume(matches!(decision, TradeCpiDecision::Reject));

    // For ANY rejection, nonce must be unchanged
    let result_nonce = decision_nonce(old_nonce, decision);
    assert_eq!(result_nonce, old_nonce,
        "ANY TradeCpi rejection must leave nonce unchanged");
}

/// Prove: ANY TradeCpi acceptance increments nonce (universal quantification)
#[kani::proof]
fn kani_tradecpi_any_accept_increments_nonce() {
    let old_nonce: u64 = kani::any();

    // Build shape from symbolic bools
    let shape = MatcherAccountsShape {
        prog_executable: kani::any(),
        ctx_executable: kani::any(),
        ctx_owner_is_prog: kani::any(),
        ctx_len_ok: kani::any(),
    };

    let identity_ok: bool = kani::any();
    let pda_ok: bool = kani::any();
    let abi_ok: bool = kani::any();
    let user_auth_ok: bool = kani::any();
    let lp_auth_ok: bool = kani::any();
    let gate_active: bool = kani::any();
    let risk_increase: bool = kani::any();
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce, shape, identity_ok, pda_ok, abi_ok,
        user_auth_ok, lp_auth_ok, gate_active, risk_increase, exec_size
    );

    // Only consider acceptance cases
    kani::assume(matches!(decision, TradeCpiDecision::Accept { .. }));

    // For ANY acceptance, nonce must increment by 1
    let result_nonce = decision_nonce(old_nonce, decision);
    assert_eq!(result_nonce, old_nonce.wrapping_add(1),
        "ANY TradeCpi acceptance must increment nonce by 1");
}

// =============================================================================
// Q. ACCOUNT VALIDATION HELPERS (6 proofs)
// =============================================================================

/// Prove: signer_ok reflects is_signer truthfully
#[kani::proof]
fn kani_signer_ok_true() {
    assert!(signer_ok(true), "signer_ok(true) must be true");
}

#[kani::proof]
fn kani_signer_ok_false() {
    assert!(!signer_ok(false), "signer_ok(false) must be false");
}

/// Prove: writable_ok reflects is_writable truthfully
#[kani::proof]
fn kani_writable_ok_true() {
    assert!(writable_ok(true), "writable_ok(true) must be true");
}

#[kani::proof]
fn kani_writable_ok_false() {
    assert!(!writable_ok(false), "writable_ok(false) must be false");
}

/// Prove: len_ok requires actual >= need
#[kani::proof]
fn kani_len_ok_sufficient() {
    let actual: usize = kani::any();
    let need: usize = kani::any();
    kani::assume(actual >= need);
    assert!(len_ok(actual, need), "len_ok must pass when actual >= need");
}

#[kani::proof]
fn kani_len_ok_insufficient() {
    let actual: usize = kani::any();
    let need: usize = kani::any();
    kani::assume(actual < need);
    assert!(!len_ok(actual, need), "len_ok must fail when actual < need");
}

// =============================================================================
// R. LP PDA SHAPE VALIDATION (4 proofs)
// =============================================================================

/// Prove: valid LP PDA shape is accepted
#[kani::proof]
fn kani_lp_pda_shape_valid() {
    let shape = LpPdaShape {
        is_system_owned: true,
        data_len_zero: true,
        lamports_zero: true,
    };
    assert!(lp_pda_shape_ok(shape), "valid LP PDA shape must be accepted");
}

/// Prove: non-system-owned LP PDA is rejected
#[kani::proof]
fn kani_lp_pda_rejects_wrong_owner() {
    let shape = LpPdaShape {
        is_system_owned: false,
        data_len_zero: true,
        lamports_zero: true,
    };
    assert!(!lp_pda_shape_ok(shape), "non-system-owned LP PDA must be rejected");
}

/// Prove: LP PDA with data is rejected
#[kani::proof]
fn kani_lp_pda_rejects_has_data() {
    let shape = LpPdaShape {
        is_system_owned: true,
        data_len_zero: false,
        lamports_zero: true,
    };
    assert!(!lp_pda_shape_ok(shape), "LP PDA with data must be rejected");
}

/// Prove: funded LP PDA is rejected
#[kani::proof]
fn kani_lp_pda_rejects_funded() {
    let shape = LpPdaShape {
        is_system_owned: true,
        data_len_zero: true,
        lamports_zero: false,
    };
    assert!(!lp_pda_shape_ok(shape), "funded LP PDA must be rejected");
}

// =============================================================================
// S. ORACLE KEY AND SLAB SHAPE (4 proofs)
// =============================================================================

/// Prove: oracle_key_ok accepts matching keys
#[kani::proof]
fn kani_oracle_key_match() {
    let key: [u8; 32] = kani::any();
    assert!(oracle_key_ok(key, key), "matching oracle keys must be accepted");
}

/// Prove: oracle_key_ok rejects mismatched keys
#[kani::proof]
fn kani_oracle_key_mismatch() {
    let expected: [u8; 32] = kani::any();
    let provided: [u8; 32] = kani::any();
    kani::assume(expected != provided);
    assert!(!oracle_key_ok(expected, provided), "mismatched oracle keys must be rejected");
}

/// Prove: valid slab shape is accepted
#[kani::proof]
fn kani_slab_shape_valid() {
    let shape = SlabShape {
        owned_by_program: true,
        correct_len: true,
    };
    assert!(slab_shape_ok(shape), "valid slab shape must be accepted");
}

/// Prove: invalid slab shape is rejected
#[kani::proof]
fn kani_slab_shape_invalid() {
    let owned: bool = kani::any();
    let correct_len: bool = kani::any();
    kani::assume(!owned || !correct_len);
    let shape = SlabShape {
        owned_by_program: owned,
        correct_len: correct_len,
    };
    assert!(!slab_shape_ok(shape), "invalid slab shape must be rejected");
}

// =============================================================================
// T. SIMPLE DECISION FUNCTIONS (6 proofs)
// =============================================================================

/// Prove: decide_single_owner_op accepts when auth ok
#[kani::proof]
fn kani_decide_single_owner_accepts() {
    let decision = decide_single_owner_op(true);
    assert_eq!(decision, SimpleDecision::Accept,
        "decide_single_owner_op must accept when auth ok");
}

/// Prove: decide_single_owner_op rejects when auth fails
#[kani::proof]
fn kani_decide_single_owner_rejects() {
    let decision = decide_single_owner_op(false);
    assert_eq!(decision, SimpleDecision::Reject,
        "decide_single_owner_op must reject when auth fails");
}

/// Prove: decide_crank accepts when crank authorized
#[kani::proof]
fn kani_decide_crank_accepts() {
    let owner: [u8; 32] = kani::any();
    // When idx exists and signer matches owner, or when idx doesn't exist
    let decision1 = decide_crank(true, owner, owner);
    assert_eq!(decision1, SimpleDecision::Accept, "crank must accept owner match");

    let signer: [u8; 32] = kani::any();
    let stored: [u8; 32] = kani::any();
    let decision2 = decide_crank(false, stored, signer);
    assert_eq!(decision2, SimpleDecision::Accept, "crank must accept non-existent account");
}

/// Prove: decide_crank rejects unauthorized
#[kani::proof]
fn kani_decide_crank_rejects() {
    let stored: [u8; 32] = kani::any();
    let signer: [u8; 32] = kani::any();
    kani::assume(stored != signer);

    let decision = decide_crank(true, stored, signer);
    assert_eq!(decision, SimpleDecision::Reject,
        "crank must reject owner mismatch on existing account");
}

/// Prove: decide_admin_op accepts valid admin
#[kani::proof]
fn kani_decide_admin_accepts() {
    let admin: [u8; 32] = kani::any();
    kani::assume(admin != [0u8; 32]);

    let decision = decide_admin_op(admin, admin);
    assert_eq!(decision, SimpleDecision::Accept,
        "admin op must accept matching non-burned admin");
}

/// Prove: decide_admin_op rejects invalid admin
#[kani::proof]
fn kani_decide_admin_rejects() {
    // Case 1: burned admin
    let signer: [u8; 32] = kani::any();
    let decision1 = decide_admin_op([0u8; 32], signer);
    assert_eq!(decision1, SimpleDecision::Reject, "burned admin must reject");

    // Case 2: admin mismatch
    let admin: [u8; 32] = kani::any();
    kani::assume(admin != [0u8; 32]);
    kani::assume(admin != signer);
    let decision2 = decide_admin_op(admin, signer);
    assert_eq!(decision2, SimpleDecision::Reject, "admin mismatch must reject");
}

// =============================================================================
// U. VERIFY::ABI_OK EQUIVALENCE (1 proof)
// Prove that verify::abi_ok is equivalent to validate_matcher_return
// =============================================================================

/// Prove: verify::abi_ok returns true iff validate_matcher_return returns Ok
/// This is a single strong equivalence proof - abi_ok calls the real validator.
#[kani::proof]
fn kani_abi_ok_equals_validate() {
    let ret = any_matcher_return();
    let lp_account_id: u64 = kani::any();
    let oracle_price: u64 = kani::any();
    let req_size: i128 = kani::any();
    let req_id: u64 = kani::any();

    let validate_result = validate_matcher_return(&ret, lp_account_id, oracle_price, req_size, req_id);

    let ret_fields = MatcherReturnFields {
        abi_version: ret.abi_version,
        flags: ret.flags,
        exec_price_e6: ret.exec_price_e6,
        exec_size: ret.exec_size,
        req_id: ret.req_id,
        lp_account_id: ret.lp_account_id,
        oracle_price_e6: ret.oracle_price_e6,
        reserved: ret.reserved,
    };
    let abi_ok_result = abi_ok(ret_fields, lp_account_id, oracle_price, req_size, req_id);

    // Strong equivalence: abi_ok == validate.is_ok() for all inputs
    assert_eq!(abi_ok_result, validate_result.is_ok(),
        "abi_ok must be equivalent to validate_matcher_return.is_ok()");
}

// =============================================================================
// V. DECIDE_TRADE_CPI_FROM_RET UNIVERSAL PROOFS (3 proofs)
// These prove program-level policies using the mechanically-tied decision function
// =============================================================================

/// Prove: ANY rejection from decide_trade_cpi_from_ret leaves nonce unchanged
#[kani::proof]
fn kani_tradecpi_from_ret_any_reject_nonce_unchanged() {
    let old_nonce: u64 = kani::any();
    let shape = MatcherAccountsShape {
        prog_executable: kani::any(),
        ctx_executable: kani::any(),
        ctx_owner_is_prog: kani::any(),
        ctx_len_ok: kani::any(),
    };
    let identity_ok: bool = kani::any();
    let pda_ok: bool = kani::any();
    let user_auth_ok: bool = kani::any();
    let lp_auth_ok: bool = kani::any();
    let gate_is_active: bool = kani::any();
    let risk_increase: bool = kani::any();
    let ret = any_matcher_return_fields();
    let lp_account_id: u64 = kani::any();
    let oracle_price_e6: u64 = kani::any();
    let req_size: i128 = kani::any();

    let decision = decide_trade_cpi_from_ret(
        old_nonce, shape, identity_ok, pda_ok,
        user_auth_ok, lp_auth_ok, gate_is_active, risk_increase,
        ret, lp_account_id, oracle_price_e6, req_size
    );

    // Only consider rejection cases
    kani::assume(matches!(decision, TradeCpiDecision::Reject));

    // For ANY rejection, nonce must be unchanged
    let result_nonce = decision_nonce(old_nonce, decision);
    assert_eq!(result_nonce, old_nonce,
        "ANY TradeCpi rejection (from real inputs) must leave nonce unchanged");
}

/// Prove: ANY acceptance from decide_trade_cpi_from_ret increments nonce
#[kani::proof]
fn kani_tradecpi_from_ret_any_accept_increments_nonce() {
    let old_nonce: u64 = kani::any();
    let shape = MatcherAccountsShape {
        prog_executable: kani::any(),
        ctx_executable: kani::any(),
        ctx_owner_is_prog: kani::any(),
        ctx_len_ok: kani::any(),
    };
    let identity_ok: bool = kani::any();
    let pda_ok: bool = kani::any();
    let user_auth_ok: bool = kani::any();
    let lp_auth_ok: bool = kani::any();
    let gate_is_active: bool = kani::any();
    let risk_increase: bool = kani::any();
    let ret = any_matcher_return_fields();
    let lp_account_id: u64 = kani::any();
    let oracle_price_e6: u64 = kani::any();
    let req_size: i128 = kani::any();

    let decision = decide_trade_cpi_from_ret(
        old_nonce, shape, identity_ok, pda_ok,
        user_auth_ok, lp_auth_ok, gate_is_active, risk_increase,
        ret, lp_account_id, oracle_price_e6, req_size
    );

    // Only consider acceptance cases
    kani::assume(matches!(decision, TradeCpiDecision::Accept { .. }));

    // For ANY acceptance, nonce must increment by 1
    let result_nonce = decision_nonce(old_nonce, decision);
    assert_eq!(result_nonce, old_nonce.wrapping_add(1),
        "ANY TradeCpi acceptance (from real inputs) must increment nonce by 1");
}

/// Prove: ANY acceptance uses exec_size from ret, not req_size
#[kani::proof]
fn kani_tradecpi_from_ret_accept_uses_exec_size() {
    let old_nonce: u64 = kani::any();
    let shape = MatcherAccountsShape {
        prog_executable: kani::any(),
        ctx_executable: kani::any(),
        ctx_owner_is_prog: kani::any(),
        ctx_len_ok: kani::any(),
    };
    let identity_ok: bool = kani::any();
    let pda_ok: bool = kani::any();
    let user_auth_ok: bool = kani::any();
    let lp_auth_ok: bool = kani::any();
    let gate_is_active: bool = kani::any();
    let risk_increase: bool = kani::any();
    let ret = any_matcher_return_fields();
    let lp_account_id: u64 = kani::any();
    let oracle_price_e6: u64 = kani::any();
    let req_size: i128 = kani::any();

    let decision = decide_trade_cpi_from_ret(
        old_nonce, shape, identity_ok, pda_ok,
        user_auth_ok, lp_auth_ok, gate_is_active, risk_increase,
        ret, lp_account_id, oracle_price_e6, req_size
    );

    if let TradeCpiDecision::Accept { chosen_size, .. } = decision {
        assert_eq!(chosen_size, ret.exec_size,
            "TradeCpi accept must use exec_size from matcher return, not req_size");
    }
}

// =============================================================================
// W. REJECT => NO CHOSEN_SIZE (1 proof)
// =============================================================================

/// Prove: Reject decision has no chosen_size field (structural guarantee)
#[kani::proof]
fn kani_reject_has_no_chosen_size() {
    let decision = TradeCpiDecision::Reject;

    // This is a structural proof - Reject variant has no chosen_size field
    // The match below proves that Reject cannot carry a chosen_size
    match decision {
        TradeCpiDecision::Reject => {
            // Reject has no fields - chosen_size is not accessible
            assert!(true, "Reject has no chosen_size by construction");
        }
        TradeCpiDecision::Accept { chosen_size: _, new_nonce: _ } => {
            panic!("expected Reject, got Accept");
        }
    }
}

// =============================================================================
// X. i128::MIN BOUNDARY REGRESSION (1 proof)
// =============================================================================

/// Regression proof: i128::MIN boundary case is correctly rejected
/// This proves that exec_size=i128::MIN, req_size=i128::MIN+1 is rejected
/// because |i128::MIN| = 2^127 > |i128::MIN+1| = 2^127-1
/// The old .abs() implementation would panic; .unsigned_abs() handles this correctly.
#[kani::proof]
fn kani_min_abs_boundary_rejected() {
    let ret = MatcherReturn {
        abi_version: MATCHER_ABI_VERSION,
        flags: FLAG_VALID,
        exec_price_e6: 1_000_000, // non-zero price
        exec_size: i128::MIN,     // -2^127
        req_id: 42,
        lp_account_id: 100,
        oracle_price_e6: 50_000_000,
        reserved: 0,
    };

    let req_size = i128::MIN + 1; // -2^127 + 1, so |req_size| = 2^127 - 1

    // |exec_size| = 2^127, |req_size| = 2^127 - 1
    // Since |exec_size| > |req_size|, this must be rejected
    let result = validate_matcher_return(
        &ret,
        ret.lp_account_id,
        ret.oracle_price_e6,
        req_size,
        ret.req_id,
    );

    assert!(result.is_err(),
        "i128::MIN exec_size with req_size=i128::MIN+1 must be rejected (|exec| > |req|)");
}
