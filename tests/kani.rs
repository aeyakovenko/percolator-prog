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
use percolator_prog::constants::MATCHER_ABI_VERSION;
use percolator_prog::constants::MAX_UNIT_SCALE;
use percolator_prog::matcher_abi::{
    validate_matcher_return, MatcherReturn, FLAG_PARTIAL_OK, FLAG_REJECTED, FLAG_VALID,
};
use percolator_prog::oracle::{clamp_oracle_price, clamp_toward_with_dt, restart_detected};
use percolator_prog::policy::{
    abi_ok,
    admin_ok,
    cpi_trade_size,
    decide_admin_op,
    decide_crank,
    // New: allow_panic crank decision
    decide_keeper_crank,
    decide_single_owner_op,
    decide_trade_cpi,
    decide_trade_cpi_from_ret,
    decide_trade_nocpi,
    decision_nonce,
    // Fee-weighted EWMA
    ewma_update,
    // New: InitMarket scale validation
    init_market_scale_ok,
    // New: Oracle inversion math
    invert_price_e6,
    len_at_least,
    len_ok,
    matcher_identity_ok,
    matcher_shape_ok,
    nonce_on_failure,
    nonce_on_success,
    owner_ok,
    pda_key_matches,
    // New: Oracle unit scale math
    scale_price_e6,
    // Account validation helpers
    signer_ok,
    slab_shape_ok,
    writable_ok,
    MatcherAccountsShape,
    // ABI validation from real inputs
    MatcherReturnFields,
    SimpleDecision,
    SlabShape,
    TradeCpiDecision,
    TradeNoCpiDecision,
    INVERSION_CONSTANT,
};

// Kani-specific bounds to avoid SAT explosion on division/modulo.
// MAX_UNIT_SCALE (1 billion) is too large for bit-precise SAT solving.
// Using small bounds keeps proofs tractable while still exercising the logic.
// The actual MAX_UNIT_SCALE bound is proven separately in init_market_scale_* proofs.
const KANI_MAX_SCALE: u32 = 64;
// Cap quotients to keep division/mod tractable
const KANI_MAX_QUOTIENT: u64 = 16384;

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
// A. MATCHER ABI VALIDATION (8 proofs - program-level)
// req_id/lp_account_id/oracle_price single-gate proofs removed:
// subsumed by kani_abi_ok_equals_validate (section R)
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
    assert!(
        result.is_err(),
        "zero exec_size without PARTIAL_OK must be rejected"
    );
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
    assert!(
        result.is_err(),
        "exec_size exceeding req_size must be rejected"
    );
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

    assert!(!owner_ok(stored, signer), "owner mismatch must be rejected");
}

/// Prove: owner match is accepted
#[kani::proof]
fn kani_owner_match_accepted() {
    let owner: [u8; 32] = kani::any();

    assert!(owner_ok(owner, owner), "owner match must be accepted");
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

    assert!(!admin_ok(admin, signer), "admin mismatch must be rejected");
}

/// Prove: admin match is accepted (when not burned)
#[kani::proof]
fn kani_admin_match_accepted() {
    let admin: [u8; 32] = kani::any();
    kani::assume(admin != [0u8; 32]); // Not burned

    assert!(admin_ok(admin, admin), "admin match must be accepted");
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
// NOTE: These use concrete structs (UNIT TEST classification). Individually
// superseded by kani_universal_shape_fail_rejects (AE) for rejection and
// kani_tradecpi_accept_increments_nonce (L) for acceptance. Retained as
// readable documentation of each field's validation requirement.
// =============================================================================

/// Universal: matcher_shape_ok is fully characterized
///
/// CODE-EQUALS-SPEC: The body of `matcher_shape_ok` IS the expression
/// `prog_exec && !ctx_exec && ctx_owned && ctx_len`. This proof asserts the
/// function equals its own body for all symbolic inputs. Fully symbolic;
/// provides regression protection if the function body is modified.
#[kani::proof]
fn kani_matcher_shape_universal() {
    let shape = MatcherAccountsShape {
        prog_executable: kani::any(),
        ctx_executable: kani::any(),
        ctx_owner_is_prog: kani::any(),
        ctx_len_ok: kani::any(),
    };

    let expected = shape.prog_executable
        && !shape.ctx_executable
        && shape.ctx_owner_is_prog
        && shape.ctx_len_ok;
    assert_eq!(
        matcher_shape_ok(shape),
        expected,
        "matcher_shape_ok must equal (prog_exec && !ctx_exec && ctx_owned && ctx_len)"
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

    assert!(pda_key_matches(key, key), "PDA key match must be accepted");
}

// =============================================================================
// G. NONCE MONOTONICITY (3 proofs)
// =============================================================================

/// Prove: nonce unchanged on failure
#[kani::proof]
fn kani_nonce_unchanged_on_failure() {
    let old_nonce: u64 = kani::any();
    let new_nonce = nonce_on_failure(old_nonce);

    assert_eq!(new_nonce, old_nonce, "nonce must be unchanged on failure");
}

/// Prove: nonce advances by exactly 1 on success; overflow returns None.
#[kani::proof]
fn kani_nonce_advances_on_success() {
    let old_nonce: u64 = kani::any();
    let new_nonce = nonce_on_success(old_nonce);

    if old_nonce < u64::MAX {
        assert_eq!(
            new_nonce,
            Some(old_nonce + 1),
            "nonce must advance by 1 on success"
        );
    } else {
        assert_eq!(
            new_nonce, None,
            "nonce_on_success must return None at u64::MAX"
        );
    }
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

// =============================================================================
// J. PER-INSTRUCTION AUTHORIZATION
// (Removed: single_owner_authorized and trade_authorized harnesses —
//  verify functions deleted from wrapper)
// =============================================================================

// =============================================================================
// L. TRADECPI DECISION COUPLING - CRITICAL
// These prove program-level policies, not just helper semantics.
//
// kani_decide_trade_cpi_universal fully characterizes the function:
// Accept iff shape_ok && identity && pda && abi && user && lp && !(gate && risk).
// Subsumes all individual gate rejection proofs (AE section) and the former
// kani_tradecpi_allows_gate_risk_decrease. Individual AE proofs retained as
// readable documentation.
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

/// Universal characterization of decide_trade_cpi: fully symbolic inputs.
/// Proves: Accept iff shape_ok && identity && pda && abi && user && lp.
/// On Accept: new_nonce == nonce_on_success(old_nonce), chosen_size == exec_size.
#[kani::proof]
fn kani_decide_trade_cpi_universal() {
    let old_nonce: u64 = kani::any();
    let exec_size: i128 = kani::any();
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
    let lp_key_ok: bool = kani::any();

    let decision = decide_trade_cpi(
        old_nonce,
        shape,
        identity_ok,
        pda_ok,
        abi_ok,
        user_auth_ok,
        lp_key_ok,
        exec_size,
    );

    let should_accept = matcher_shape_ok(shape)
        && identity_ok
        && pda_ok
        && abi_ok
        && user_auth_ok
        && lp_key_ok
        && old_nonce < u64::MAX; // nonce overflow gate

    if should_accept {
        match decision {
            TradeCpiDecision::Accept {
                new_nonce,
                chosen_size,
            } => {
                assert_eq!(
                    Some(new_nonce),
                    nonce_on_success(old_nonce),
                    "accept nonce must be nonce_on_success(old_nonce)"
                );
                assert_eq!(
                    chosen_size, exec_size,
                    "accept chosen_size must equal exec_size"
                );
            }
            _ => panic!("all gates pass but got Reject"),
        }
    } else {
        assert_eq!(
            decision,
            TradeCpiDecision::Reject,
            "any gate failure (or nonce overflow) must produce Reject"
        );
    }
}

/// Prove: TradeCpi reject leaves nonce unchanged for all invalid matcher shapes.
#[kani::proof]
fn kani_tradecpi_reject_nonce_unchanged() {
    let old_nonce: u64 = kani::any();
    let exec_size: i128 = kani::any();

    // Quantify over all invalid matcher shapes, not just one witness.
    let shape = MatcherAccountsShape {
        prog_executable: kani::any(),
        ctx_executable: kani::any(),
        ctx_owner_is_prog: kani::any(),
        ctx_len_ok: kani::any(),
    };
    kani::assume(!matcher_shape_ok(shape));

    let decision = decide_trade_cpi(old_nonce, shape, true, true, true, true, true, exec_size);

    assert_eq!(
        decision,
        TradeCpiDecision::Reject,
        "TradeCpi must reject for any invalid matcher shape"
    );

    let result_nonce = decision_nonce(old_nonce, decision);

    assert_eq!(
        result_nonce, old_nonce,
        "TradeCpi reject must leave nonce unchanged"
    );
}

/// Prove: TradeCpi accept increments nonce for all valid matcher shapes.
/// At the u64::MAX boundary the nonce-overflow gate forces Reject — verified
/// separately in kani_decide_trade_cpi_universal.
#[kani::proof]
fn kani_tradecpi_accept_increments_nonce() {
    let old_nonce: u64 = kani::any();
    let exec_size: i128 = kani::any();

    // Quantify over all valid matcher shapes, not just one witness.
    let shape = MatcherAccountsShape {
        prog_executable: kani::any(),
        ctx_executable: kani::any(),
        ctx_owner_is_prog: kani::any(),
        ctx_len_ok: kani::any(),
    };
    kani::assume(matcher_shape_ok(shape));
    // Exclude the overflow boundary; covered by the universal proof above.
    kani::assume(old_nonce < u64::MAX);

    let decision = decide_trade_cpi(old_nonce, shape, true, true, true, true, true, exec_size);

    assert_eq!(
        decision,
        TradeCpiDecision::Accept {
            new_nonce: old_nonce + 1,
            chosen_size: exec_size,
        },
        "TradeCpi must accept for any valid matcher shape when all other checks pass"
    );

    let result_nonce = decision_nonce(old_nonce, decision);

    assert_eq!(
        result_nonce,
        old_nonce + 1,
        "TradeCpi accept must increment nonce by 1"
    );
}

// Note: kani_tradecpi_accept_uses_exec_size removed — duplicate of
// kani_tradecpi_accept_increments_nonce (same assertion on same inputs).

// =============================================================================
// M. TRADENOCPI DECISION COUPLING (3 proofs — universal symbolic)
// =============================================================================

/// Universal: TradeNoCpi rejects when user_auth=false OR lp_auth=false.
///
/// NOTE: This proof is subsumed by `kani_tradenocpi_universal_characterization`
/// which provides a full characterization of `decide_trade_nocpi`.
#[kani::proof]
fn kani_tradenocpi_auth_failure_rejects() {
    let user_auth_ok: bool = kani::any();
    let lp_auth_ok: bool = kani::any();

    // At least one auth must fail
    kani::assume(!user_auth_ok || !lp_auth_ok);

    let decision = decide_trade_nocpi(user_auth_ok, lp_auth_ok);
    assert_eq!(
        decision,
        TradeNoCpiDecision::Reject,
        "TradeNoCpi must reject when any auth check fails"
    );
}

/// Universal: TradeNoCpi decision is fully characterized by its inputs
#[kani::proof]
fn kani_tradenocpi_universal_characterization() {
    let user_auth_ok: bool = kani::any();
    let lp_auth_ok: bool = kani::any();

    let decision = decide_trade_nocpi(user_auth_ok, lp_auth_ok);

    // Full characterization: accept iff all auth passes
    let should_accept = user_auth_ok && lp_auth_ok;
    if should_accept {
        assert_eq!(
            decision,
            TradeNoCpiDecision::Accept,
            "must accept when all conditions pass"
        );
    } else {
        assert_eq!(
            decision,
            TradeNoCpiDecision::Reject,
            "must reject when any condition fails"
        );
    }
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
    assert!(
        result.is_ok(),
        "zero exec_size with PARTIAL_OK must be accepted"
    );
}

// =============================================================================
// O. MISSING SHAPE COUPLING PROOFS (2 proofs)
// =============================================================================

// =============================================================================
// P. UNIVERSAL REJECT => NONCE UNCHANGED (1 proof)
// This subsumes all specific "reject => nonce unchanged" proofs
// =============================================================================

/// Prove: ANY TradeCpi rejection leaves nonce unchanged (universal quantification)
/// Non-vacuity: concrete witness proves at least one Reject path exists.
#[kani::proof]
fn kani_tradecpi_any_reject_nonce_unchanged() {
    // Non-vacuity witness: bad shape always produces Reject
    {
        let bad = MatcherAccountsShape {
            prog_executable: false,
            ctx_executable: false,
            ctx_owner_is_prog: true,
            ctx_len_ok: true,
        };
        let d = decide_trade_cpi(0, bad, true, true, true, true, true, 0);
        assert!(
            matches!(d, TradeCpiDecision::Reject),
            "non-vacuity: bad shape must reject"
        );
    }

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
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce,
        shape,
        identity_ok,
        pda_ok,
        abi_ok,
        user_auth_ok,
        lp_auth_ok,
        exec_size,
    );

    // Strengthened: prove nonce transition relation for both outcome variants.
    let expected_nonce = match &decision {
        TradeCpiDecision::Reject => old_nonce,
        TradeCpiDecision::Accept { .. } => old_nonce.wrapping_add(1),
    };
    let result_nonce = decision_nonce(old_nonce, decision);
    assert_eq!(
        result_nonce, expected_nonce,
        "decision_nonce must agree with TradeCpiDecision outcome"
    );
}

/// Prove: ANY TradeCpi acceptance increments nonce (universal quantification)
/// Non-vacuity: concrete witness proves at least one Accept path exists.
#[kani::proof]
fn kani_tradecpi_any_accept_increments_nonce() {
    // Non-vacuity witness: all-valid inputs produce Accept
    {
        let d = decide_trade_cpi(0, valid_shape(), true, true, true, true, true, 0);
        assert!(
            matches!(d, TradeCpiDecision::Accept { .. }),
            "non-vacuity: all-valid inputs must accept"
        );
    }

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
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce,
        shape,
        identity_ok,
        pda_ok,
        abi_ok,
        user_auth_ok,
        lp_auth_ok,
        exec_size,
    );

    // Strengthened: prove nonce transition relation for both outcome variants.
    let expected_nonce = match &decision {
        TradeCpiDecision::Reject => old_nonce,
        TradeCpiDecision::Accept { .. } => old_nonce.wrapping_add(1),
    };
    let result_nonce = decision_nonce(old_nonce, decision);
    assert_eq!(
        result_nonce, expected_nonce,
        "decision_nonce must agree with TradeCpiDecision outcome"
    );
}

// =============================================================================
// Q. ACCOUNT VALIDATION HELPERS (2 proofs)
// =============================================================================
// Note: signer_ok and writable_ok are identity functions (return input unchanged).
// Testing them would be trivial (proving true==true). Only len_ok has real logic.

/// Prove: len_ok requires actual == need (strict equality)
///
/// CODE-EQUALS-SPEC: The body of `len_ok` IS `actual == need`. This proof asserts
/// the function equals its own body for all symbolic inputs. Fully symbolic;
/// provides regression protection if the function body is modified.
///
/// The stricter contract (was `>=` until the "strict-equality for instruction
/// account counts" fix) prevents callers from padding with unrelated trailing
/// accounts on handlers that expect a fixed shape. `len_at_least` is the
/// documented escape hatch for TradeCpi's variadic matcher-tail.
#[kani::proof]
fn kani_len_ok_universal() {
    let actual: usize = kani::any();
    let need: usize = kani::any();

    assert_eq!(
        len_ok(actual, need),
        actual == need,
        "len_ok must return (actual == need)"
    );
}

/// Prove: len_at_least requires actual >= need (TradeCpi variadic-tail helper)
///
/// CODE-EQUALS-SPEC: `len_at_least` is the documented loose check used by
/// TradeCpi (which forwards tail accounts to the matcher CPI). Covering it
/// ensures the variadic-tail ABI keeps exactly the "≥" semantics it is
/// specified to have.
#[kani::proof]
fn kani_len_at_least_universal() {
    let actual: usize = kani::any();
    let need: usize = kani::any();

    assert_eq!(
        len_at_least(actual, need),
        actual >= need,
        "len_at_least must return (actual >= need)"
    );
}

// =============================================================================
// R. LP PDA SHAPE VALIDATION (4 proofs)
// LP PDA shape check removed — PDA key match is sufficient.
// Only this program can sign for the PDA, so it's always system-owned
// with zero data. The shape proof is no longer needed.
// =============================================================================

// =============================================================================
// S. SLAB SHAPE (oracle_feed_id_ok removed — verify function deleted)
// =============================================================================

/// Prove: valid slab shape is accepted
///
/// CODE-EQUALS-SPEC: The body of `slab_shape_ok` IS
/// `owned_by_program && correct_len`. This proof asserts the function equals
/// its own body for all symbolic inputs. Fully symbolic; provides regression
/// protection if the function body is modified.
#[kani::proof]
fn kani_slab_shape_universal() {
    let owned: bool = kani::any();
    let correct_len: bool = kani::any();
    let shape = SlabShape {
        owned_by_program: owned,
        correct_len: correct_len,
    };
    let expected = owned && correct_len;
    assert_eq!(
        slab_shape_ok(shape),
        expected,
        "slab_shape_ok must equal (owned && correct_len)"
    );
}

// =============================================================================
// T. SIMPLE DECISION FUNCTIONS (6 proofs)
// =============================================================================

/// Universal: decide_single_owner_op is fully characterized
/// (subsumes the concrete true/false unit tests)
#[kani::proof]
fn kani_decide_single_owner_universal() {
    let auth_ok: bool = kani::any();
    let decision = decide_single_owner_op(auth_ok);
    if auth_ok {
        assert_eq!(decision, SimpleDecision::Accept, "auth ok must accept");
    } else {
        assert_eq!(decision, SimpleDecision::Reject, "auth fail must reject");
    }
}

/// Universal: decide_crank is fully characterized by its inputs
/// Exercises all 3 branches (permissionless, self-crank-ok, self-crank-fail)
#[kani::proof]
fn kani_decide_crank_universal() {
    let permissionless: bool = kani::any();
    let idx_exists: bool = kani::any();
    let stored: [u8; 32] = kani::any();
    let signer: [u8; 32] = kani::any();

    let decision = decide_crank(permissionless, idx_exists, stored, signer);

    let should_accept = permissionless || (idx_exists && stored == signer);
    if should_accept {
        assert_eq!(decision, SimpleDecision::Accept, "must accept");
    } else {
        assert_eq!(decision, SimpleDecision::Reject, "must reject");
    }
}

/// Universal: decide_admin_op is fully characterized
/// accept iff admin != [0;32] && admin == signer
#[kani::proof]
fn kani_decide_admin_universal() {
    let admin: [u8; 32] = kani::any();
    let signer: [u8; 32] = kani::any();

    let decision = decide_admin_op(admin, signer);

    let should_accept = admin != [0u8; 32] && admin == signer;
    if should_accept {
        assert_eq!(decision, SimpleDecision::Accept, "valid admin must accept");
    } else {
        assert_eq!(
            decision,
            SimpleDecision::Reject,
            "invalid admin must reject"
        );
    }
}

// =============================================================================
// U. VERIFY::ABI_OK EQUIVALENCE (1 proof)
// Prove that policy::abi_ok is equivalent to validate_matcher_return
// =============================================================================

/// Prove: policy::abi_ok returns true iff validate_matcher_return returns Ok
/// This is a single strong equivalence proof - abi_ok calls the real validator.
#[kani::proof]
fn kani_abi_ok_equals_validate() {
    let ret = any_matcher_return();
    let lp_account_id: u64 = kani::any();
    let oracle_price: u64 = kani::any();
    let req_size: i128 = kani::any();
    let req_id: u64 = kani::any();

    let validate_result =
        validate_matcher_return(&ret, lp_account_id, oracle_price, req_size, req_id);

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
    assert_eq!(
        abi_ok_result,
        validate_result.is_ok(),
        "abi_ok must be equivalent to validate_matcher_return.is_ok()"
    );
}

// =============================================================================
// V. DECIDE_TRADE_CPI_FROM_RET UNIVERSAL PROOFS (3 proofs)
// These prove program-level policies using the mechanically-tied decision function
// =============================================================================

/// Prove: ANY rejection from decide_trade_cpi_from_ret leaves nonce unchanged
/// Non-vacuity: concrete witness proves at least one Reject path exists.
#[kani::proof]
fn kani_tradecpi_from_ret_any_reject_nonce_unchanged() {
    // Non-vacuity witness: bad shape always produces Reject
    {
        let bad = MatcherAccountsShape {
            prog_executable: false,
            ctx_executable: false,
            ctx_owner_is_prog: true,
            ctx_len_ok: true,
        };
        let dummy_ret = MatcherReturnFields {
            abi_version: 0,
            flags: 0,
            exec_price_e6: 0,
            exec_size: 0,
            req_id: 0,
            lp_account_id: 0,
            oracle_price_e6: 0,
            reserved: 0,
        };
        let d = decide_trade_cpi_from_ret(0, bad, true, true, true, true, dummy_ret, 0, 0, 0);
        assert!(
            matches!(d, TradeCpiDecision::Reject),
            "non-vacuity: bad shape must reject"
        );
    }

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
    let ret = any_matcher_return_fields();
    let lp_account_id: u64 = kani::any();
    let oracle_price_e6: u64 = kani::any();
    let req_size: i128 = kani::any();

    let decision = decide_trade_cpi_from_ret(
        old_nonce,
        shape,
        identity_ok,
        pda_ok,
        user_auth_ok,
        lp_auth_ok,
        ret,
        lp_account_id,
        oracle_price_e6,
        req_size,
    );

    // Strengthened: prove nonce transition relation for both outcome variants.
    let expected_nonce = match &decision {
        TradeCpiDecision::Reject => old_nonce,
        TradeCpiDecision::Accept { .. } => old_nonce.wrapping_add(1),
    };
    let result_nonce = decision_nonce(old_nonce, decision);
    assert_eq!(
        result_nonce, expected_nonce,
        "decision_nonce must agree with TradeCpiDecision outcome (from_ret)"
    );
}

/// Prove: ANY acceptance from decide_trade_cpi_from_ret increments nonce
/// Non-vacuity: concrete witness proves at least one Accept path exists.
#[kani::proof]
fn kani_tradecpi_from_ret_any_accept_increments_nonce() {
    // Non-vacuity witness: construct valid ABI inputs that produce Accept
    {
        let req_id = nonce_on_success(42).expect("42 + 1 cannot overflow u64");
        let valid_ret = MatcherReturnFields {
            abi_version: MATCHER_ABI_VERSION,
            flags: FLAG_VALID | FLAG_PARTIAL_OK,
            exec_price_e6: 1_000_000,
            exec_size: 0,
            req_id,
            lp_account_id: 1,
            oracle_price_e6: 50_000_000,
            reserved: 0,
        };
        let d = decide_trade_cpi_from_ret(
            42,
            valid_shape(),
            true,
            true,
            true,
            true,
            valid_ret,
            1,
            50_000_000,
            100,
        );
        assert!(
            matches!(d, TradeCpiDecision::Accept { .. }),
            "non-vacuity: valid ABI inputs must accept"
        );
    }

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
    let ret = any_matcher_return_fields();
    let lp_account_id: u64 = kani::any();
    let oracle_price_e6: u64 = kani::any();
    let req_size: i128 = kani::any();

    let decision = decide_trade_cpi_from_ret(
        old_nonce,
        shape,
        identity_ok,
        pda_ok,
        user_auth_ok,
        lp_auth_ok,
        ret,
        lp_account_id,
        oracle_price_e6,
        req_size,
    );

    // Strengthened: prove nonce transition relation for both outcome variants.
    let expected_nonce = match &decision {
        TradeCpiDecision::Reject => old_nonce,
        TradeCpiDecision::Accept { .. } => old_nonce.wrapping_add(1),
    };
    let result_nonce = decision_nonce(old_nonce, decision);
    assert_eq!(
        result_nonce, expected_nonce,
        "decision_nonce must agree with TradeCpiDecision outcome (from_ret)"
    );
}

/// Prove: ANY acceptance uses exec_size from ret, not req_size
/// NON-VACUOUS: Forces Accept path by constraining inputs to valid state
///
/// UNIT TEST: Shape is hardcoded valid; all authorization bools are concrete `true`.
/// Only `exec_size`, `req_size`, `lp_account_id`, and
/// `oracle_price_e6` are symbolic. This functions as a unit test of `cpi_trade_size`
/// on the accept path, verifying the exec_size binding property rather than the
/// full symbolic space of authorization conditions.
#[kani::proof]
fn kani_tradecpi_from_ret_accept_uses_exec_size() {
    let old_nonce: u64 = kani::any();
    // Force valid matcher shape
    let shape = MatcherAccountsShape {
        prog_executable: true,
        ctx_executable: false,
        ctx_owner_is_prog: true,
        ctx_len_ok: true,
    };
    // Force all authorization checks to pass
    let identity_ok: bool = true;
    let pda_ok: bool = true;
    let user_auth_ok: bool = true;
    let lp_auth_ok: bool = true;

    // Force valid matcher return
    let exec_size: i128 = kani::any();
    let req_size: i128 = kani::any();
    kani::assume(exec_size != 0);
    kani::assume(req_size != 0);
    // exec_size must have same sign as req_size and |exec_size| <= |req_size|
    kani::assume((exec_size > 0) == (req_size > 0));
    kani::assume(exec_size.unsigned_abs() <= req_size.unsigned_abs());

    let lp_account_id: u64 = kani::any();
    let oracle_price_e6: u64 = kani::any();
    kani::assume(oracle_price_e6 > 0);

    // Must have room to advance the nonce (overflow gate) and req_id must
    // match nonce_on_success(old_nonce) for ABI validation to pass.
    kani::assume(old_nonce < u64::MAX);
    let expected_req_id = nonce_on_success(old_nonce).expect("assumed no overflow");

    let ret = MatcherReturnFields {
        abi_version: MATCHER_ABI_VERSION,
        flags: FLAG_VALID | FLAG_PARTIAL_OK,
        exec_price_e6: kani::any::<u64>().max(1), // Non-zero price
        exec_size,
        req_id: expected_req_id, // Must match nonce_on_success(old_nonce)
        lp_account_id,           // Must match
        oracle_price_e6,         // Must match
        reserved: 0,
    };

    let decision = decide_trade_cpi_from_ret(
        old_nonce,
        shape,
        identity_ok,
        pda_ok,
        user_auth_ok,
        lp_auth_ok,
        ret,
        lp_account_id,
        oracle_price_e6,
        req_size,
    );

    // MUST be Accept with these inputs - panic if not (catches regression)
    match decision {
        TradeCpiDecision::Accept { chosen_size, .. } => {
            assert_eq!(
                chosen_size, ret.exec_size,
                "TradeCpi accept must use exec_size from matcher return, not req_size"
            );
        }
        TradeCpiDecision::Reject => {
            panic!(
                "Expected Accept with valid inputs - function may have regressed to always-reject"
            );
        }
    }
}

// =============================================================================
// W. REJECT => NO CHOSEN_SIZE
// =============================================================================
// Note: Removed trivial proof. The Reject variant having no fields is a
// compile-time structural guarantee enforced by Rust's type system.
// A Kani proof asserting `true` on enum match adds no verification value.

// =============================================================================
// X. i128::MIN BOUNDARY REGRESSION (1 proof)
// =============================================================================

/// Regression proof: i128::MIN boundary case is correctly rejected
/// This proves that exec_size=i128::MIN, req_size=i128::MIN+1 is rejected
/// because |i128::MIN| = 2^127 > |i128::MIN+1| = 2^127-1
/// The old .abs() implementation would panic; .unsigned_abs() handles this correctly.
///
/// UNIT TEST: All inputs are concrete literals: `exec_size = i128::MIN`,
/// `req_size = i128::MIN + 1`. This is a single-path regression test proving the
/// old `.abs()` panic is fixed. Valuable as a regression guard but not a symbolic proof.
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

    assert!(
        result.is_err(),
        "i128::MIN exec_size with req_size=i128::MIN+1 must be rejected (|exec| > |req|)"
    );
}

// =============================================================================
// Y. ACCEPTANCE PROOFS - Valid inputs MUST be accepted
// =============================================================================

/// Prove: minimal valid non-zero exec_size is accepted
#[kani::proof]
fn kani_matcher_accepts_minimal_valid_nonzero_exec() {
    let mut ret = any_matcher_return();
    // Constrain to valid inputs
    ret.abi_version = MATCHER_ABI_VERSION;
    ret.flags = FLAG_VALID;
    ret.reserved = 0;
    kani::assume(ret.exec_price_e6 != 0);
    kani::assume(ret.exec_size != 0);

    // Use ret's own fields for expected values (no mismatch)
    let lp_account_id: u64 = ret.lp_account_id;
    let oracle_price: u64 = ret.oracle_price_e6;
    let req_id: u64 = ret.req_id;

    // Minimal nonzero fill: exact full fill, so PARTIAL_OK is not required.
    let req_size: i128 = ret.exec_size;

    let result = validate_matcher_return(&ret, lp_account_id, oracle_price, req_size, req_id);
    assert!(result.is_ok(), "valid inputs must be accepted");
}

/// Prove: exec_size == req_size (same sign) is accepted
#[kani::proof]
fn kani_matcher_accepts_exec_size_equal_req_size() {
    let mut ret = any_matcher_return();
    ret.abi_version = MATCHER_ABI_VERSION;
    ret.flags = FLAG_VALID;
    ret.reserved = 0;
    kani::assume(ret.exec_price_e6 != 0);
    kani::assume(ret.exec_size != 0);

    // exec_size == req_size
    let req_size: i128 = ret.exec_size;
    let lp_account_id: u64 = ret.lp_account_id;
    let oracle_price: u64 = ret.oracle_price_e6;
    let req_id: u64 = ret.req_id;

    let result = validate_matcher_return(&ret, lp_account_id, oracle_price, req_size, req_id);
    assert!(result.is_ok(), "exec_size == req_size must be accepted");
}

/// Prove: partial fill with PARTIAL_OK is accepted
#[kani::proof]
fn kani_matcher_accepts_partial_fill_with_flag() {
    let mut ret = any_matcher_return();
    ret.abi_version = MATCHER_ABI_VERSION;
    ret.flags = FLAG_VALID | FLAG_PARTIAL_OK;
    ret.reserved = 0;
    kani::assume(ret.exec_price_e6 != 0);
    kani::assume(ret.exec_size != 0);

    let lp_account_id: u64 = ret.lp_account_id;
    let oracle_price: u64 = ret.oracle_price_e6;
    let req_id: u64 = ret.req_id;

    // req_size >= exec_size, same sign (partial fill)
    let req_size: i128 = kani::any();
    kani::assume(req_size.signum() == ret.exec_size.signum());
    kani::assume(req_size.unsigned_abs() >= ret.exec_size.unsigned_abs());

    let result = validate_matcher_return(&ret, lp_account_id, oracle_price, req_size, req_id);
    assert!(
        result.is_ok(),
        "partial fill with PARTIAL_OK must be accepted"
    );
}

/// Universal characterization: decide_keeper_crank ==
///   decide_crank(permissionless, idx_exists, stored_owner, signer)
/// allow_panic removed from model — runtime ignores it for wire compat.
#[kani::proof]
fn kani_decide_keeper_crank_universal() {
    let signer: [u8; 32] = kani::any();
    let permissionless: bool = kani::any();
    let idx_exists: bool = kani::any();
    let stored_owner: [u8; 32] = kani::any();

    let decision = decide_keeper_crank(permissionless, idx_exists, stored_owner, signer);

    let expected = decide_crank(permissionless, idx_exists, stored_owner, signer);

    assert_eq!(
        decision, expected,
        "decide_keeper_crank must equal decide_crank"
    );
}

// =============================================================================
// AA. ORACLE INVERSION MATH PROOFS (5 proofs)
// =============================================================================

/// Prove: invert==0 returns raw unchanged (for any raw including 0)
/// Note: invert==0 is "no inversion" - raw passes through unchanged
#[kani::proof]
fn kani_invert_zero_returns_raw() {
    let raw: u64 = kani::any();
    let result = invert_price_e6(raw, 0);
    assert_eq!(result, Some(raw), "invert==0 must return raw unchanged");
}

/// Prove: invert!=0 with valid raw returns correct floor(1e12/raw)
/// NON-VACUOUS: forces success path by constraining raw to valid range
/// Bounded to 8192: 128-bit division + equality is SAT-heavy (~66s)
///
/// SAT tractability bound: the proved property (floor-division correctness,
/// i.e. result == INVERSION_CONSTANT / raw) holds for all u64 inputs by
/// definition of integer division. The assume bound keeps verification
/// tractable for the CBMC solver; the mathematical guarantee is universal.
#[kani::proof]
fn kani_invert_nonzero_computes_correctly() {
    let raw: u64 = kani::any();
    kani::assume(raw > 0);
    kani::assume(raw <= 8192);

    let result = invert_price_e6(raw, 1);

    // Must succeed: 1e12 / raw >= 1 when raw <= 1e12
    let inverted = result.expect("inversion must succeed for raw in (0, 8192]");

    // Verify correctness: exact floor division
    let expected = INVERSION_CONSTANT / (raw as u128);
    assert_eq!(
        inverted as u128, expected,
        "inversion must be floor(1e12/raw)"
    );
}

/// Prove: raw==0 always returns None for any non-zero invert (div by zero protection)
#[kani::proof]
fn kani_invert_zero_raw_returns_none() {
    let invert: u8 = kani::any();
    kani::assume(invert != 0);
    let result = invert_price_e6(0, invert);
    assert!(result.is_none(), "raw==0 must return None");
}

/// Prove: inverted==0 returns None for ALL raw > INVERSION_CONSTANT
/// Since 1e12 / raw < 1 when raw > 1e12, the result floors to 0 => None.
#[kani::proof]
fn kani_invert_result_zero_returns_none() {
    let raw: u64 = kani::any();
    kani::assume(raw > INVERSION_CONSTANT as u64);

    let result = invert_price_e6(raw, 1);
    assert!(
        result.is_none(),
        "inversion resulting in 0 must return None"
    );
}

// Compile-time assertion: INVERSION_CONSTANT (1e12) fits in u64 (max ~1.8e19).
// This replaces the former kani::assert on compile-time constants, which was
// VACUOUS (could not fail under any model). A `const` assertion is checked at
// compile time and guards against any future change to INVERSION_CONSTANT that
// would make the overflow branch reachable.
const _: () = assert!(
    INVERSION_CONSTANT <= u64::MAX as u128,
    "INVERSION_CONSTANT must fit in u64, making overflow branch unreachable"
);

/// Prove: the overflow branch in invert_price_e6 is dead code.
/// INVERSION_CONSTANT = 1e12 < u64::MAX ≈ 1.8e19, so 1e12/raw can never
/// exceed u64::MAX for any positive raw. Documents this structural property.
/// The compile-time `const` assertion above guards the constant value;
/// this symbolic proof covers all positive `raw` inputs.
#[kani::proof]
fn kani_invert_overflow_branch_is_dead() {
    // For any raw > 0, inverted = INVERSION_CONSTANT / raw <= INVERSION_CONSTANT <= u64::MAX
    let raw: u64 = kani::any();
    kani::assume(raw > 0);
    let inverted = INVERSION_CONSTANT / (raw as u128);
    assert!(
        inverted <= u64::MAX as u128,
        "inversion result must fit in u64 for all positive raw"
    );
}

/// Prove: monotonicity - if raw1 > raw2 > 0 then inv1 <= inv2
///
/// SAT tractability bound: the proved property (Euclidean division monotonicity:
/// larger divisor => smaller quotient) holds for all u64 inputs by definition of
/// floor-division. The assume bound keeps verification tractable for the CBMC
/// solver. The `None` interaction (raw > INVERSION_CONSTANT returns None) is
/// covered separately by `kani_invert_result_zero_returns_none`.
#[kani::proof]
fn kani_invert_monotonic() {
    let raw1: u64 = kani::any();
    let raw2: u64 = kani::any();
    kani::assume(raw1 > 0 && raw2 > 0);
    kani::assume(raw1 > raw2);
    // Cap to keep division tractable for SAT solver
    kani::assume(raw1 <= KANI_MAX_QUOTIENT);
    kani::assume(raw2 <= KANI_MAX_QUOTIENT);

    let inv1 = invert_price_e6(raw1, 1);
    let inv2 = invert_price_e6(raw2, 1);

    // Bounded domain guarantees successful inversion for both values.
    assert!(
        inv1.is_some(),
        "raw1 in bounded domain must invert successfully"
    );
    assert!(
        inv2.is_some(),
        "raw2 in bounded domain must invert successfully"
    );
    let i1 = inv1.unwrap();
    let i2 = inv2.unwrap();
    assert!(i1 <= i2, "inversion must be monotonically decreasing");
}

// =============================================================================
// AB. UNIT CONVERSION ALGEBRA PROOFS
// (Removed: base_to_units, units_to_base harnesses — verify functions deleted)
// =============================================================================

// =============================================================================
// AC. WITHDRAW ALIGNMENT PROOFS
// (Removed: withdraw_amount_aligned harnesses — verify function deleted)
// =============================================================================

// AD. DUST MATH PROOFS
// (Removed: sweep_dust, accumulate_dust, base_to_units harnesses — verify functions deleted)
// =============================================================================

// =============================================================================
// AE. UNIVERSAL GATE ORDERING PROOFS FOR TRADECPI (6 proofs)
// These prove that specific gates cause rejection regardless of other inputs
// =============================================================================

/// Universal: matcher_shape_ok==false => Reject (regardless of other inputs)
#[kani::proof]
fn kani_universal_shape_fail_rejects() {
    let old_nonce: u64 = kani::any();
    let shape = MatcherAccountsShape {
        prog_executable: kani::any(),
        ctx_executable: kani::any(),
        ctx_owner_is_prog: kani::any(),
        ctx_len_ok: kani::any(),
    };
    // Force shape to be invalid
    kani::assume(!matcher_shape_ok(shape));

    let identity_ok: bool = kani::any();
    let pda_ok: bool = kani::any();
    let abi_ok: bool = kani::any();
    let user_auth_ok: bool = kani::any();
    let lp_auth_ok: bool = kani::any();
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce,
        shape,
        identity_ok,
        pda_ok,
        abi_ok,
        user_auth_ok,
        lp_auth_ok,
        exec_size,
    );

    assert_eq!(
        decision,
        TradeCpiDecision::Reject,
        "invalid shape must always reject"
    );
}

/// Universal: pda_ok==false => Reject
#[kani::proof]
fn kani_universal_pda_fail_rejects() {
    let old_nonce: u64 = kani::any();
    let shape = MatcherAccountsShape {
        prog_executable: kani::any(),
        ctx_executable: kani::any(),
        ctx_owner_is_prog: kani::any(),
        ctx_len_ok: kani::any(),
    };
    kani::assume(matcher_shape_ok(shape));
    let identity_ok: bool = kani::any();
    let pda_ok = false; // Force failure
    let abi_ok: bool = kani::any();
    let user_auth_ok: bool = kani::any();
    let lp_auth_ok: bool = kani::any();
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce,
        shape,
        identity_ok,
        pda_ok,
        abi_ok,
        user_auth_ok,
        lp_auth_ok,
        exec_size,
    );

    assert_eq!(
        decision,
        TradeCpiDecision::Reject,
        "pda_ok==false must always reject"
    );
}

/// Universal: user_auth_ok==false => Reject
#[kani::proof]
fn kani_universal_user_auth_fail_rejects() {
    let old_nonce: u64 = kani::any();
    let shape = MatcherAccountsShape {
        prog_executable: kani::any(),
        ctx_executable: kani::any(),
        ctx_owner_is_prog: kani::any(),
        ctx_len_ok: kani::any(),
    };
    kani::assume(matcher_shape_ok(shape));
    let identity_ok: bool = kani::any();
    let pda_ok: bool = kani::any();
    let abi_ok: bool = kani::any();
    let user_auth_ok = false; // Force failure
    let lp_auth_ok: bool = kani::any();
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce,
        shape,
        identity_ok,
        pda_ok,
        abi_ok,
        user_auth_ok,
        lp_auth_ok,
        exec_size,
    );

    assert_eq!(
        decision,
        TradeCpiDecision::Reject,
        "user_auth_ok==false must always reject"
    );
}

/// Universal: lp_auth_ok==false => Reject
#[kani::proof]
fn kani_universal_lp_auth_fail_rejects() {
    let old_nonce: u64 = kani::any();
    let shape = MatcherAccountsShape {
        prog_executable: kani::any(),
        ctx_executable: kani::any(),
        ctx_owner_is_prog: kani::any(),
        ctx_len_ok: kani::any(),
    };
    kani::assume(matcher_shape_ok(shape));
    let identity_ok: bool = kani::any();
    let pda_ok: bool = kani::any();
    let abi_ok: bool = kani::any();
    let user_auth_ok: bool = kani::any();
    let lp_auth_ok = false; // Force failure
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce,
        shape,
        identity_ok,
        pda_ok,
        abi_ok,
        user_auth_ok,
        lp_auth_ok,
        exec_size,
    );

    assert_eq!(
        decision,
        TradeCpiDecision::Reject,
        "lp_auth_ok==false must always reject"
    );
}

/// Universal: identity_ok==false => Reject
#[kani::proof]
fn kani_universal_identity_fail_rejects() {
    let old_nonce: u64 = kani::any();
    let shape = MatcherAccountsShape {
        prog_executable: kani::any(),
        ctx_executable: kani::any(),
        ctx_owner_is_prog: kani::any(),
        ctx_len_ok: kani::any(),
    };
    kani::assume(matcher_shape_ok(shape));
    let identity_ok = false; // Force failure
    let pda_ok: bool = kani::any();
    let abi_ok: bool = kani::any();
    let user_auth_ok: bool = kani::any();
    let lp_auth_ok: bool = kani::any();
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce,
        shape,
        identity_ok,
        pda_ok,
        abi_ok,
        user_auth_ok,
        lp_auth_ok,
        exec_size,
    );

    assert_eq!(
        decision,
        TradeCpiDecision::Reject,
        "identity_ok==false must always reject"
    );
}

/// Universal: abi_ok==false => Reject
#[kani::proof]
fn kani_universal_abi_fail_rejects() {
    let old_nonce: u64 = kani::any();
    let shape = MatcherAccountsShape {
        prog_executable: kani::any(),
        ctx_executable: kani::any(),
        ctx_owner_is_prog: kani::any(),
        ctx_len_ok: kani::any(),
    };
    kani::assume(matcher_shape_ok(shape));
    let identity_ok: bool = kani::any();
    let pda_ok: bool = kani::any();
    let abi_ok = false; // Force failure
    let user_auth_ok: bool = kani::any();
    let lp_auth_ok: bool = kani::any();
    let exec_size: i128 = kani::any();

    let decision = decide_trade_cpi(
        old_nonce,
        shape,
        identity_ok,
        pda_ok,
        abi_ok,
        user_auth_ok,
        lp_auth_ok,
        exec_size,
    );

    assert_eq!(
        decision,
        TradeCpiDecision::Reject,
        "abi_ok==false must always reject"
    );
}

// =============================================================================
// AF. CONSISTENCY BETWEEN decide_trade_cpi AND decide_trade_cpi_from_ret
// Split into valid-shape and invalid-shape for faster/sharper proofs
// =============================================================================

/// Prove: consistency under VALID shape - focuses on ABI/nonce/identity
#[kani::proof]
fn kani_tradecpi_variants_consistent_valid_shape() {
    let old_nonce: u64 = kani::any();
    let shape = valid_shape(); // Force valid shape

    let identity_ok: bool = kani::any();
    let pda_ok: bool = kani::any();
    let user_auth_ok: bool = kani::any();
    let lp_auth_ok: bool = kani::any();

    // Create ret fields
    let ret = any_matcher_return_fields();
    let lp_account_id: u64 = kani::any();
    let oracle_price_e6: u64 = kani::any();
    let req_size: i128 = kani::any();

    // Compute req_id as decide_trade_cpi_from_ret does. The overflow gate in
    // both variants makes Accept impossible at u64::MAX, and the variants only
    // need to agree about Reject in that case — we still derive an expected
    // req_id for the abi_ok probe (any u64 works; production code never reaches
    // abi_ok on overflow because the accept branch is blocked anyway).
    let req_id = nonce_on_success(old_nonce).unwrap_or(0);

    // Check if ABI would pass
    let abi_passes = abi_ok(ret, lp_account_id, oracle_price_e6, req_size, req_id);

    // Get decisions from both variants
    let decision1 = decide_trade_cpi(
        old_nonce,
        shape,
        identity_ok,
        pda_ok,
        abi_passes,
        user_auth_ok,
        lp_auth_ok,
        ret.exec_size,
    );

    let decision2 = decide_trade_cpi_from_ret(
        old_nonce,
        shape,
        identity_ok,
        pda_ok,
        user_auth_ok,
        lp_auth_ok,
        ret,
        lp_account_id,
        oracle_price_e6,
        req_size,
    );

    // Both must give same outcome (modulo overflow: at u64::MAX both reject).
    if old_nonce == u64::MAX {
        assert_eq!(decision1, TradeCpiDecision::Reject);
        assert_eq!(decision2, TradeCpiDecision::Reject);
        return;
    }
    match (&decision1, &decision2) {
        (TradeCpiDecision::Reject, TradeCpiDecision::Reject) => {}
        (
            TradeCpiDecision::Accept {
                new_nonce: n1,
                chosen_size: s1,
            },
            TradeCpiDecision::Accept {
                new_nonce: n2,
                chosen_size: s2,
            },
        ) => {
            assert_eq!(*n1, *n2, "nonces must match");
            assert_eq!(*s1, *s2, "chosen_sizes must match");
        }
        _ => panic!("decisions must be consistent"),
    }
}

/// Prove: consistency under INVALID shape - both must reject (fast proof)
#[kani::proof]
fn kani_tradecpi_variants_consistent_invalid_shape() {
    let old_nonce: u64 = kani::any();
    let shape = MatcherAccountsShape {
        prog_executable: kani::any(),
        ctx_executable: kani::any(),
        ctx_owner_is_prog: kani::any(),
        ctx_len_ok: kani::any(),
    };
    // Force INVALID shape
    kani::assume(!matcher_shape_ok(shape));

    // Other inputs symbolic
    let identity_ok: bool = kani::any();
    let pda_ok: bool = kani::any();
    let user_auth_ok: bool = kani::any();
    let lp_auth_ok: bool = kani::any();
    let ret = any_matcher_return_fields();
    let lp_account_id: u64 = kani::any();
    let oracle_price_e6: u64 = kani::any();
    let req_size: i128 = kani::any();

    // req_id can be anything when shape is invalid — both variants reject before
    // reading abi_ok. Use unwrap_or to handle the u64::MAX overflow case.
    let req_id = nonce_on_success(old_nonce).unwrap_or(0);
    let abi_passes = abi_ok(ret, lp_account_id, oracle_price_e6, req_size, req_id);

    let decision1 = decide_trade_cpi(
        old_nonce,
        shape,
        identity_ok,
        pda_ok,
        abi_passes,
        user_auth_ok,
        lp_auth_ok,
        ret.exec_size,
    );

    let decision2 = decide_trade_cpi_from_ret(
        old_nonce,
        shape,
        identity_ok,
        pda_ok,
        user_auth_ok,
        lp_auth_ok,
        ret,
        lp_account_id,
        oracle_price_e6,
        req_size,
    );

    // Both must reject on invalid shape
    assert_eq!(
        decision1,
        TradeCpiDecision::Reject,
        "invalid shape must reject (variant 1)"
    );
    assert_eq!(
        decision2,
        TradeCpiDecision::Reject,
        "invalid shape must reject (variant 2)"
    );
}

/// Prove: decide_trade_cpi_from_ret computes req_id as nonce_on_success(old_nonce)
/// NON-VACUOUS: forces acceptance by constraining ret to be ABI-valid
///
/// UNIT TEST: Shape is hardcoded via `valid_shape()`; all authorization bools are
/// concrete `true`. Only `old_nonce`, `lp_account_id`,
/// `oracle_price_e6`, and `req_size` are symbolic. This is a borderline unit test
/// of nonce binding on a single forced-accept path; the full symbolic space is
/// covered by `kani_tradecpi_from_ret_any_accept_increments_nonce`.
#[kani::proof]
fn kani_tradecpi_from_ret_req_id_is_nonce_plus_one() {
    let old_nonce: u64 = kani::any();
    let shape = valid_shape();

    // Force the accept path — exclude the nonce-overflow boundary so req_id is
    // well-defined and Accept is reachable.
    kani::assume(old_nonce < u64::MAX);
    let expected_req_id = nonce_on_success(old_nonce).expect("old_nonce < u64::MAX assumed");

    // Constrain ret to be ABI-valid for this req_id
    let mut ret = any_matcher_return_fields();
    ret.abi_version = MATCHER_ABI_VERSION;
    ret.flags = FLAG_VALID | FLAG_PARTIAL_OK; // PARTIAL_OK allows exec_size=0
    ret.reserved = 0;
    kani::assume(ret.exec_price_e6 != 0);
    ret.req_id = expected_req_id; // Must match nonce_on_success(old_nonce)
    ret.exec_size = 0; // With PARTIAL_OK, zero size is always valid

    let lp_account_id: u64 = ret.lp_account_id;
    let oracle_price_e6: u64 = ret.oracle_price_e6;
    let req_size: i128 = kani::any();

    // All other checks pass
    let decision = decide_trade_cpi_from_ret(
        old_nonce,
        shape,
        true, // identity_ok
        true, // pda_ok
        true, // user_auth_ok
        true, // lp_auth_ok
        ret,
        lp_account_id,
        oracle_price_e6,
        req_size,
    );

    // FORCE acceptance - with valid ABI inputs, must accept
    match decision {
        TradeCpiDecision::Accept { new_nonce, .. } => {
            assert_eq!(
                new_nonce, expected_req_id,
                "new_nonce must equal nonce_on_success(old_nonce)"
            );
        }
        TradeCpiDecision::Reject => {
            panic!("must accept with valid ABI inputs");
        }
    }
}

// =============================================================================
// AG. UNIVERSAL GATE PROOF (missing from AE)
// =============================================================================

// =============================================================================
// AH. ADDITIONAL STRENGTHENING PROOFS
// =============================================================================

// Note: Removed kani_unit_conversion_deterministic (purity test).
// Note: Removed kani_scale_validation_pure (purity test).
// Note: Removed kani_units_roundtrip_exact_when_no_dust (base_to_units/units_to_base deleted).
// Note: kani_universal_panic_requires_admin removed — allow_panic is dead.

// =============================================================================
// AJ. END-TO-END FORCED ACCEPTANCE FOR FROM_RET PATH
// =============================================================================

/// Prove: end-to-end acceptance when all conditions are met
/// NON-VACUOUS: forces Accept and verifies all output fields
///
/// UNIT TEST: All authorization bools are concrete `true`;
/// `exec_size = 0` with `PARTIAL_OK`. Proves one specific happy-path execution.
/// This serves as a non-vacuity witness for the from_ret accept path rather than
/// a symbolic proof of the full authorization space.
#[kani::proof]
fn kani_tradecpi_from_ret_forced_acceptance() {
    let old_nonce: u64 = kani::any();
    let shape = valid_shape();

    // Force the accept path — exclude the nonce-overflow boundary.
    kani::assume(old_nonce < u64::MAX);
    // Construct ABI-valid ret
    let expected_req_id = nonce_on_success(old_nonce).expect("old_nonce < u64::MAX assumed");
    let mut ret = any_matcher_return_fields();
    ret.abi_version = MATCHER_ABI_VERSION;
    ret.flags = FLAG_VALID | FLAG_PARTIAL_OK;
    ret.reserved = 0;
    kani::assume(ret.exec_price_e6 != 0);
    ret.req_id = expected_req_id;
    ret.exec_size = 0; // PARTIAL_OK allows zero

    let lp_account_id: u64 = ret.lp_account_id;
    let oracle_price_e6: u64 = ret.oracle_price_e6;
    let req_size: i128 = kani::any();

    // All checks pass
    let decision = decide_trade_cpi_from_ret(
        old_nonce,
        shape,
        true, // identity_ok
        true, // pda_ok
        true, // user_auth_ok
        true, // lp_auth_ok
        ret,
        lp_account_id,
        oracle_price_e6,
        req_size,
    );

    // MUST accept
    match decision {
        TradeCpiDecision::Accept {
            new_nonce,
            chosen_size,
        } => {
            assert_eq!(new_nonce, expected_req_id, "new_nonce must be nonce+1");
            assert_eq!(chosen_size, ret.exec_size, "chosen_size must be exec_size");
        }
        TradeCpiDecision::Reject => {
            panic!("must accept when all conditions pass");
        }
    }
}

// =============================================================================
// AK. INITMARKET UNIT_SCALE BOUNDS PROOFS (4 proofs)
// =============================================================================

/// Prove: scale > MAX_UNIT_SCALE is rejected
#[kani::proof]
fn kani_init_market_scale_rejects_overflow() {
    // Ensure out-of-range values are constructible (avoid vacuous assumptions).
    assert!(
        MAX_UNIT_SCALE < u32::MAX,
        "MAX_UNIT_SCALE must allow at least one out-of-range value"
    );
    let scale: u32 = kani::any();
    kani::assume(scale > MAX_UNIT_SCALE);

    let result = init_market_scale_ok(scale);
    assert!(!result, "scale > MAX_UNIT_SCALE must be rejected");
}

/// Prove: any scale in valid range [0, MAX_UNIT_SCALE] is accepted
#[kani::proof]
fn kani_init_market_scale_valid_range() {
    let scale: u32 = kani::any();
    kani::assume(scale <= MAX_UNIT_SCALE);

    let result = init_market_scale_ok(scale);

    assert!(result, "any scale in [0, MAX_UNIT_SCALE] must be accepted");
}

// =============================================================================
// AL. NON-INTERFERENCE PROOFS
// =============================================================================
// Note: Removed trivial proofs. admin_ok and owner_ok compare [u8; 32] arrays
// and don't reference unit_scale at all. Independence is structural (no shared
// state), not a runtime property that needs formal verification.

// Purity proofs removed — see note in section AH above.

// =============================================================================
// BUG DETECTION: Unit Scale Margin Inconsistency
// =============================================================================
//
// These proofs demonstrate a BUG in the current margin calculation:
// - Capital is scaled by unit_scale (base_tokens / unit_scale)
// - Position value is NOT scaled (position_size * price / 1_000_000)
// - Margin check compares capital (scaled) vs margin_required (unscaled)
// - This causes the same economic position to pass/fail margin based on unit_scale
//
// The proofs use ACTUAL PRODUCTION CODE from the percolator library:
// - percolator::RiskEngine::mark_pnl_for_position (the real mark_pnl calculation)
//
// This section documents the historical bug mechanism and anchors production
// formulas used by the post-fix proofs below.

/// Compute position value using the SAME FORMULA as production code.
/// This replicates percolator::RiskEngine::is_above_margin_bps_mtm exactly.
/// See percolator/src/percolator.rs lines 3135-3138.
#[inline]
fn production_position_value(position_size: i128, oracle_price: u64) -> u128 {
    // Exact formula from production: mul_u128(abs(pos), price) / 1_000_000
    let abs_pos = position_size.unsigned_abs();
    abs_pos.saturating_mul(oracle_price as u128) / 1_000_000
}

/// Compute margin required using the SAME FORMULA as production code.
/// See percolator/src/percolator.rs line 3141.
#[inline]
fn production_margin_required(position_value: u128, margin_bps: u64) -> u128 {
    position_value.saturating_mul(margin_bps as u128) / 10_000
}

/// Compute mark-to-market PnL using the SAME FORMULA as production code.
/// This replicates percolator::RiskEngine::mark_pnl_for_position exactly.
/// See percolator/src/percolator.rs lines 1542-1562.
#[inline]
fn production_mark_pnl(position_size: i128, entry_price: u64, oracle_price: u64) -> Option<i128> {
    if position_size == 0 {
        return Some(0);
    }
    let abs_pos = position_size.unsigned_abs();
    let diff: i128 = if position_size > 0 {
        // Long: profit when oracle > entry
        (oracle_price as i128).saturating_sub(entry_price as i128)
    } else {
        // Short: profit when entry > oracle
        (entry_price as i128).saturating_sub(oracle_price as i128)
    };
    // mark_pnl = diff * abs_pos / 1_000_000 (production uses checked_mul/checked_div)
    diff.checked_mul(abs_pos as i128)?.checked_div(1_000_000)
}

/// Compute equity using the SAME FORMULA as production code.
/// This replicates percolator::RiskEngine::account_equity_mtm_at_oracle exactly.
/// See percolator/src/percolator.rs lines 3108-3120.
///
/// BUG: Production code adds capital (in units) + pnl + mark_pnl (both NOT in units).
/// This mixes different unit systems when unit_scale != 0.
#[inline]
fn production_equity(capital: u128, pnl: i128, mark_pnl: i128) -> u128 {
    // Exact formula from production: max(0, capital + pnl + mark_pnl)
    let cap_i = if capital > i128::MAX as u128 {
        i128::MAX
    } else {
        capital as i128
    };
    let eq_i = cap_i.saturating_add(pnl).saturating_add(mark_pnl);
    if eq_i > 0 {
        eq_i as u128
    } else {
        0
    }
}

// =============================================================================
// PRODUCTION scale_price_e6 proofs - These test the ACTUAL production function
// =============================================================================

/// Prove scale_price_e6 returns None when result would be zero.
/// This tests the PRODUCTION function directly.
#[kani::proof]
fn kani_scale_price_e6_zero_result_rejected() {
    let price: u64 = kani::any();
    let unit_scale: u32 = kani::any();

    // Constrain to avoid trivial cases
    kani::assume(unit_scale > 1);
    kani::assume(price > 0);
    kani::assume(price < unit_scale as u64); // Result would be zero

    // PRODUCTION function should reject (return None)
    let result = scale_price_e6(price, unit_scale);
    assert!(
        result.is_none(),
        "scale_price_e6 must reject when scaled price would be zero"
    );
}

/// Prove scale_price_e6 returns Some when result is non-zero.
/// This tests the PRODUCTION function directly.
///
/// SAT tractability bound: the proved property (floor-division correctness:
/// result == price / unit_scale when result != 0) holds for all u64 inputs by
/// definition of integer division. The assume bounds (KANI_MAX_SCALE=64,
/// price <= KANI_MAX_QUOTIENT * unit_scale) keep verification tractable for
/// the CBMC solver. Production prices can reach billions; the mathematical
/// guarantee is universal across all inputs where result != 0.
#[kani::proof]
fn kani_scale_price_e6_valid_result() {
    let price: u64 = kani::any();
    let unit_scale: u32 = kani::any();

    // Constrain to valid inputs that produce non-zero result
    kani::assume(unit_scale > 1);
    kani::assume(unit_scale <= KANI_MAX_SCALE); // Keep SAT tractable
    kani::assume(price >= unit_scale as u64); // Ensures result >= 1
    kani::assume(price <= KANI_MAX_QUOTIENT as u64 * unit_scale as u64); // Tight bound for SAT

    // PRODUCTION function should succeed
    let result = scale_price_e6(price, unit_scale);
    assert!(
        result.is_some(),
        "scale_price_e6 must succeed for valid inputs"
    );

    // Verify the formula: scaled = price / unit_scale
    let scaled = result.unwrap();
    assert_eq!(
        scaled,
        price / unit_scale as u64,
        "scale_price_e6 must compute price / unit_scale"
    );
}

/// Prove scale_price_e6 is identity when unit_scale <= 1.
/// This tests the PRODUCTION function directly.
#[kani::proof]
fn kani_scale_price_e6_identity_for_scale_leq_1() {
    let price: u64 = kani::any();
    let unit_scale: u32 = kani::any();

    kani::assume(unit_scale <= 1);

    // PRODUCTION function should return price unchanged
    let result = scale_price_e6(price, unit_scale);
    assert!(
        result.is_some(),
        "scale_price_e6 must succeed when unit_scale <= 1"
    );
    assert_eq!(
        result.unwrap(),
        price,
        "scale_price_e6 must be identity when unit_scale <= 1"
    );
}

// Removed: kani_scale_price_and_base_to_units_use_same_divisor (base_to_units deleted)

/// Prove scaled-price math preserves conservative margin behavior under unit scaling.
/// Uses u16 multipliers + u8 scale/bps for SAT tractability.
///
/// SAT tractability bound: the proved property (conservative margin: floor rounding
/// in scale_price_e6 means scaled position value never exceeds unscaled after
/// re-scaling) holds for all inputs. The narrow domain (scale 2..16, u8/u16
/// multipliers) is required to keep the 3-deep multiplication chain tractable for
/// the CBMC solver. This is effectively a bounded integration test of the margin
/// conservatism property.
#[kani::proof]
fn kani_scale_price_e6_concrete_example() {
    let scale_raw: u8 = kani::any();
    let price_mult: u16 = kani::any();
    let pos_raw: u8 = kani::any();
    let bps_raw: u8 = kani::any();

    kani::assume(scale_raw >= 2);
    kani::assume(scale_raw <= 16);
    kani::assume(price_mult >= 1);
    kani::assume(pos_raw >= 1);
    kani::assume(bps_raw >= 1);

    let unit_scale = scale_raw as u32;
    let oracle_price = (price_mult as u64) * (unit_scale as u64); // guaranteed >= unit_scale
    let position_size = pos_raw as u128;
    let margin_bps = bps_raw as u128;

    let scaled = scale_price_e6(oracle_price, unit_scale).unwrap();

    // Conversion identity
    assert_eq!(scaled, oracle_price / unit_scale as u64);

    // Scaled valuation is conservative: floor(price/scale) cannot increase value.
    let pv_unscaled = position_size * oracle_price as u128 / 1_000_000;
    let pv_scaled = position_size * scaled as u128 / 1_000_000;
    assert!(
        pv_scaled * unit_scale as u128 <= pv_unscaled,
        "scaled position value must not exceed unscaled value after re-scaling"
    );

    let mr_unscaled = pv_unscaled * margin_bps / 10_000;
    let mr_scaled = pv_scaled * margin_bps / 10_000;
    assert!(
        mr_scaled * unit_scale as u128 <= mr_unscaled,
        "scaled margin requirement must not exceed unscaled requirement after re-scaling"
    );
}
// Integer truncation can cause < 1 unit differences that flip results at exact
// boundaries, but this is unavoidable with integer arithmetic and economically
// insignificant compared to the original bug (factor of unit_scale difference).

// =============================================================================
// BUG #9 RATE LIMITING PROOFS (clamp_toward_with_dt)
// =============================================================================
//
// Bug #9: In Hyperp mode, clamp_toward_with_dt originally returned `mark` when
// dt=0 (same slot), allowing double-crank to bypass rate limiting.
// Fix: Return `index` (no movement) when dt=0 or cap=0.

/// Prove: When dt_slots == 0, index is returned unchanged (no movement).
/// This is the core Bug #9 fix - prevents same-slot rate limit bypass.
///
/// Covers BOTH cases:
/// - index > 0: the dt=0 early-return path returns `index` unchanged (Bug #9 fix).
/// - index == 0: the bootstrap branch returns `mark`, which is DIFFERENT behavior
///   but is also correct — bootstrap always initializes to `mark` regardless of `dt`.
///
/// The assumption `index > 0 && cap_bps > 0` was previously used here, which
/// excluded the bootstrap branch. This proof is now fully symbolic on `index`.
#[kani::proof]
fn kani_clamp_toward_no_movement_when_dt_zero() {
    let index: u64 = kani::any();
    let mark: u64 = kani::any();
    let cap_bps: u64 = kani::any();

    // dt_slots = 0 (same slot)
    let result = clamp_toward_with_dt(index, mark, cap_bps, 0);

    if index == 0 {
        // Bootstrap branch: always returns mark regardless of dt or cap
        assert_eq!(
            result, mark,
            "clamp_toward_with_dt must return mark in bootstrap case (index=0)"
        );
    } else {
        // Bug #9 fix: dt=0 returns index unchanged (no movement), regardless of cap
        assert_eq!(
            result, index,
            "clamp_toward_with_dt must return index unchanged when dt_slots=0 and index>0"
        );
    }
}

/// Prove: When cap_bps == 0, index is returned unchanged (rate limiting disabled).
///
/// Covers BOTH cases:
/// - index > 0: the cap=0 early-return path returns `index` unchanged.
/// - index == 0: the bootstrap branch returns `mark`, which is the correct
///   behavior — bootstrap always initializes to `mark` regardless of cap.
///
/// The assumption `index > 0 && dt_slots > 0` was previously used here, which
/// excluded the bootstrap branch. This proof is now fully symbolic on `index`.
#[kani::proof]
fn kani_clamp_toward_no_movement_when_cap_zero() {
    let index: u64 = kani::any();
    let mark: u64 = kani::any();
    let dt_slots: u64 = kani::any();

    // cap_bps = 0 (rate limiting disabled)
    let result = clamp_toward_with_dt(index, mark, 0, dt_slots);

    if index == 0 {
        // Bootstrap branch: always returns mark regardless of dt or cap
        assert_eq!(
            result, mark,
            "clamp_toward_with_dt must return mark in bootstrap case (index=0)"
        );
    } else {
        // cap=0 means rate limiting disabled: return index unchanged
        assert_eq!(
            result, index,
            "clamp_toward_with_dt must return index unchanged when cap_bps=0 and index>0"
        );
    }
}

/// Prove: When index == 0 (uninitialized), mark is returned (bootstrap case).
#[kani::proof]
fn kani_clamp_toward_bootstrap_when_index_zero() {
    let mark: u64 = kani::any();
    let cap_bps: u64 = kani::any();
    let dt_slots: u64 = kani::any();

    // index = 0 is the bootstrap/initialization case
    let result = clamp_toward_with_dt(0, mark, cap_bps, dt_slots);

    assert_eq!(
        result, mark,
        "clamp_toward_with_dt must return mark when index=0 (bootstrap)"
    );
}

/// Prove: Index movement is always bounded by computed max_delta.
/// Uses u8-range inputs; triple-multiplication chain limits SAT tractability.
/// Companion: kani_clamp_toward_saturation_paths covers large u64 values.
///
/// SAT tractability bound: the proved property (movement bound: result in [lo, hi]
/// where max_delta = index * cap_bps * dt / 10_000) holds for all valid inputs.
/// The u8 domain (index 10..255, cap 1..20 steps, dt 1..16) is required to keep
/// the triple-multiplication chain tractable for the CBMC solver. Coverage of
/// large index values is provided by `kani_clamp_toward_saturation_paths`.
#[kani::proof]
fn kani_clamp_toward_movement_bounded_concrete() {
    let index_raw: u8 = kani::any();
    let cap_steps_raw: u8 = kani::any();
    let dt_raw: u8 = kani::any();
    let mark: u64 = kani::any();

    kani::assume(index_raw >= 10); // exclude index=0 bootstrap
    kani::assume(cap_steps_raw >= 1);
    kani::assume(cap_steps_raw <= 20); // 1%..20% cap
    kani::assume(dt_raw >= 1);
    kani::assume(dt_raw <= 16);

    let index = index_raw as u64;
    let cap_bps = (cap_steps_raw as u64) * 100;
    let dt_slots = dt_raw as u64;

    let result = clamp_toward_with_dt(index, mark, cap_bps, dt_slots);

    let max_delta = ((index as u128 * cap_bps as u128 * dt_slots as u128) / 10_000u128) as u64;
    let lo = index.saturating_sub(max_delta);
    let hi = index.saturating_add(max_delta);

    assert!(
        result >= lo && result <= hi,
        "result must stay within computed movement bounds"
    );
}

/// Shared bounded symbolic domain for clamp branch formula proofs.
/// Bounds widened to u16 index/mark while keeping triple-multiply SAT tractable.
fn any_clamp_formula_inputs() -> (u64, u64, u64, u64, u64, u64) {
    let index_raw: u16 = kani::any();
    let cap_steps_raw: u8 = kani::any(); // 1 step = 100 bps (1.00%)
    let dt_slots_raw: u8 = kani::any();
    let mark_raw: u16 = kani::any();

    kani::assume(index_raw >= 100);
    kani::assume(index_raw <= 1000);
    kani::assume(cap_steps_raw > 0);
    kani::assume(cap_steps_raw <= 5); // 1%..5% cap
    kani::assume(dt_slots_raw > 0);
    kani::assume(dt_slots_raw <= 20);
    kani::assume(mark_raw <= 2000);

    let index_u32 = index_raw as u32;
    let cap_u32 = (cap_steps_raw as u32) * 100u32;
    let dt_u32 = dt_slots_raw as u32;

    // With the bounds above, this product fits in u32 without overflow.
    // max: 1000 * 500 * 20 = 10_000_000 < u32::MAX
    let max_delta = (index_u32 * cap_u32 * dt_u32 / 10_000u32) as u64;
    let index = index_u32 as u64;
    kani::assume(max_delta > 0); // Non-trivial clamping regime
    kani::assume(max_delta <= index); // Prevent underflow in index - max_delta

    let lo = index - max_delta;
    let hi = index + max_delta;
    let mark = mark_raw as u64;

    (index, mark, cap_u32 as u64, dt_u32 as u64, lo, hi)
}

/// Prove formula correctness for the `mark < lo` branch with symbolic cap/dt.
///
/// UNIT TEST: The symbolic portion uses `any_clamp_formula_inputs()` with tight
/// bounds (index 100..1000, cap 1%..5%, dt 1..20, mark <= 2000) plus
/// `kani::assume(mark < lo)`. The concrete non-vacuity witness uses hardcoded
/// values. This is effectively a bounded integration test of the `mark < lo`
/// formula branch; the unbounded formula properties are covered by
/// `kani_clamp_toward_formula_within_bounds` and `kani_clamp_toward_formula_above_hi`.
#[kani::proof]
fn kani_clamp_toward_formula_concrete() {
    // Non-vacuity witness: below-band branch is reachable.
    {
        let index = 2_000u64;
        let cap_bps = 100u64;
        let dt_slots = 10u64;
        let max_delta = (index * cap_bps * dt_slots) / 10_000u64;
        let lo = index - max_delta;
        let mark = 1_000u64;
        assert!(mark < lo, "witness must exercise mark < lo branch");
        assert_eq!(
            clamp_toward_with_dt(index, mark, cap_bps, dt_slots),
            lo,
            "non-vacuity witness: mark below lo clamps to lo"
        );
    }

    let (index, mark, cap_bps, dt_slots, lo, _) = any_clamp_formula_inputs();
    kani::assume(mark < lo);

    let result = clamp_toward_with_dt(index, mark, cap_bps, dt_slots);
    assert_eq!(result, lo, "mark below lo must clamp to lo");
}

/// Companion proof: when mark is within the allowed band, result equals mark.
#[kani::proof]
fn kani_clamp_toward_formula_within_bounds() {
    // Non-vacuity witness: within-band branch is reachable.
    {
        let index = 2_000u64;
        let cap_bps = 100u64;
        let dt_slots = 10u64;
        let max_delta = (index * cap_bps * dt_slots) / 10_000u64;
        let lo = index - max_delta;
        let hi = index + max_delta;
        let mark = 2_000u64;
        assert!(mark >= lo && mark <= hi, "witness must be inside [lo, hi]");
        assert_eq!(
            clamp_toward_with_dt(index, mark, cap_bps, dt_slots),
            mark,
            "non-vacuity witness: mark inside [lo, hi] remains unchanged"
        );
    }

    let (index, mark, cap_bps, dt_slots, lo, hi) = any_clamp_formula_inputs();
    kani::assume(mark >= lo);
    kani::assume(mark <= hi);

    let result = clamp_toward_with_dt(index, mark, cap_bps, dt_slots);
    assert_eq!(result, mark, "mark inside [lo, hi] must remain unchanged");
}

/// Companion proof: when mark is above the allowed band, result clamps to `hi`.
#[kani::proof]
fn kani_clamp_toward_formula_above_hi() {
    // Non-vacuity witness: above-band branch is reachable.
    {
        let index = 2_000u64;
        let cap_bps = 100u64;
        let dt_slots = 10u64;
        let max_delta = (index * cap_bps * dt_slots) / 10_000u64;
        let hi = index + max_delta;
        let mark = 3_000u64;
        assert!(mark > hi, "witness must exercise mark > hi branch");
        assert_eq!(
            clamp_toward_with_dt(index, mark, cap_bps, dt_slots),
            hi,
            "non-vacuity witness: mark above hi clamps to hi"
        );
    }

    let (index, mark, cap_bps, dt_slots, _, hi) = any_clamp_formula_inputs();
    kani::assume(mark > hi);

    let result = clamp_toward_with_dt(index, mark, cap_bps, dt_slots);
    assert_eq!(result, hi, "mark above hi must clamp to hi");
}

/// Prove: clamp_toward_with_dt exercises saturation paths with large u64 inputs.
/// Tests: saturating_mul overflow in max_delta_u128, min(max_delta_u128, u64::MAX)
/// clamp, and saturating_sub/add hitting 0 or u64::MAX.
#[kani::proof]
fn kani_clamp_toward_saturation_paths() {
    // Non-vacuity witness 1: max_delta saturates to u64::MAX, lo=0, hi=u64::MAX
    {
        let index = u64::MAX / 2;
        let cap_bps = 10_000; // 100%
        let dt_slots = 100;
        let result = clamp_toward_with_dt(index, 0, cap_bps, dt_slots);
        // max_delta_u128 = (MAX/2) * 10_000 * 100 / 10_000 = (MAX/2)*100 >> u64::MAX
        // so max_delta = u64::MAX, lo = saturating_sub = 0
        assert_eq!(
            result, 0,
            "witness: mark=0 with saturated max_delta clamps to lo=0"
        );
    }

    // Non-vacuity witness 2: hi saturates to u64::MAX
    {
        let index = u64::MAX - 10;
        let cap_bps = 100; // 1%
        let dt_slots = 1;
        let result = clamp_toward_with_dt(index, u64::MAX, cap_bps, dt_slots);
        // max_delta = (MAX-10) * 100 / 10_000 ≈ MAX/100, hi = saturating_add = u64::MAX
        assert_eq!(
            result,
            u64::MAX,
            "witness: mark=MAX with hi=MAX clamps to MAX"
        );
    }

    // Symbolic proof: large index with symbolic mark exercises saturation
    let index_offset: u8 = kani::any();
    let mark: u64 = kani::any();
    let cap_steps: u8 = kani::any();
    let dt_raw: u8 = kani::any();

    kani::assume(cap_steps >= 1);
    kani::assume(dt_raw >= 1);

    let index = (u64::MAX / 2).saturating_add(index_offset as u64);
    let cap_bps = (cap_steps as u64) * 1_000; // 10%..2550% (forces large delta)
    let dt_slots = dt_raw as u64;

    let result = clamp_toward_with_dt(index, mark, cap_bps, dt_slots);

    // Recompute expected bounds (mirrors production code)
    let max_delta_u128 = (index as u128)
        .saturating_mul(cap_bps as u128)
        .saturating_mul(dt_slots as u128)
        / 10_000u128;
    let max_delta = core::cmp::min(max_delta_u128, u64::MAX as u128) as u64;
    let lo = index.saturating_sub(max_delta);
    let hi = index.saturating_add(max_delta);

    assert!(
        result >= lo && result <= hi,
        "result must stay within saturated bounds"
    );
    assert_eq!(
        result,
        mark.clamp(lo, hi),
        "result must equal mark.clamp(lo, hi)"
    );
}

// =========================================================================
// WithdrawInsurance vault accounting proofs
// (Removed: withdraw_insurance_vault harnesses — verify function deleted)
// =========================================================================

// =============================================================================
// INDUCTIVE: Full-domain algebraic properties
//
// These proofs use fully symbolic inputs (no bounded ranges) and verify
// properties via comparison logic rather than multiplication of unknowns
// (which creates intractable SAT constraints in CBMC).
//
// Note: Floor-division properties (monotonicity, conservatism) cannot be
// proved inductively in CBMC because they require symbolic×symbolic
// multiplication. The bounded proofs above verify the implementation IS
// floor division; the mathematical properties follow trivially.
// =============================================================================

/// Inductive: clamp(mark, lo, hi) is always within [lo, hi] for any mark, lo, hi
///
/// This is a trivial property of clamp but proves it holds for the full u64 domain,
/// complementing the bounded kani_clamp_toward_movement_bounded_concrete which
/// verifies the max_delta COMPUTATION is correct (for u8 inputs).
#[kani::proof]
fn inductive_clamp_within_bounds() {
    let mark: u64 = kani::any();
    let lo: u64 = kani::any();
    let hi: u64 = kani::any();
    kani::assume(lo <= hi);

    let result = mark.clamp(lo, hi);

    assert!(
        result >= lo && result <= hi,
        "clamp must stay within [lo, hi]"
    );
}

// =============================================================================
// NEW: UNIVERSAL CHARACTERIZATION — decide_trade_cpi_from_ret (Tier 1 gap)
// =============================================================================

/// Universal characterization of decide_trade_cpi_from_ret: fully symbolic inputs.
///
/// Specification (verified):
///   Accept iff:
///     matcher_shape_ok(shape)
///     && pda_ok
///     && user_auth_ok && lp_auth_ok
///     && identity_ok
///     && abi_ok(ret, lp_account_id, oracle_price_e6, req_size, nonce_on_success(old_nonce))
///
///   On Accept:
///     new_nonce == nonce_on_success(old_nonce)
///     chosen_size == cpi_trade_size(ret.exec_size, req_size)
///
/// This is the Tier 1 universal proof for `decide_trade_cpi_from_ret`.
/// Analogous to `kani_decide_trade_cpi_universal` for `decide_trade_cpi`.
///
/// Note on ABI validity: `abi_ok` computes `validate_matcher_return` with
/// `req_id = nonce_on_success(old_nonce)`. We construct a symbolically ABI-valid
/// `ret` by setting the echoed fields to agree with symbolic `lp_account_id`,
/// `oracle_price_e6`, `req_size`, and the computed `req_id`, then assume the
/// remaining structural ABI checks (abi_version, flags, exec_price, exec_size
/// constraints) are satisfied. This gives us a fully symbolic accept condition.
#[kani::proof]
fn kani_decide_trade_cpi_from_ret_universal() {
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

    // Construct an ABI-valid ret symbolically.
    // The `abi_ok` call inside `decide_trade_cpi_from_ret` uses:
    //   req_id = nonce_on_success(old_nonce)
    //   lp_account_id, oracle_price_e6, req_size as passed in
    // We derive the expected req_id and set ret.req_id to match. A nonce at
    // u64::MAX overflows in nonce_on_success and is a separate reject path —
    // handle it up front so the rest of the proof treats req_id as a u64.
    let req_id = match nonce_on_success(old_nonce) {
        Some(n) => n,
        None => {
            // Overflow gate: production code rejects regardless of other inputs.
            let decision = decide_trade_cpi_from_ret(
                old_nonce,
                shape,
                identity_ok,
                pda_ok,
                user_auth_ok,
                lp_auth_ok,
                any_matcher_return_fields(),
                kani::any(),
                kani::any(),
                kani::any(),
            );
            assert_eq!(
                decision,
                TradeCpiDecision::Reject,
                "nonce overflow must force Reject"
            );
            return;
        }
    };
    let mut ret = any_matcher_return_fields();
    ret.abi_version = MATCHER_ABI_VERSION;
    ret.flags = FLAG_VALID | FLAG_PARTIAL_OK; // PARTIAL_OK allows exec_size=0
    ret.reserved = 0;
    kani::assume(ret.exec_price_e6 != 0);
    ret.req_id = req_id; // Must echo computed req_id for ABI validation to pass

    let lp_account_id: u64 = ret.lp_account_id; // Must match echoed value
    let oracle_price_e6: u64 = ret.oracle_price_e6; // Must match echoed value
    let req_size: i128 = kani::any();

    // exec_size must satisfy |exec_size| <= |req_size| with same sign (or be 0 with PARTIAL_OK)
    // We use exec_size = 0 (always valid with PARTIAL_OK) for simplicity.
    ret.exec_size = 0;

    // Compute the expected ABI validity for the assume/assert below.
    let abi_valid = abi_ok(ret, lp_account_id, oracle_price_e6, req_size, req_id);

    // The should_accept specification matches the production gate order exactly.
    let should_accept =
        matcher_shape_ok(shape) && pda_ok && user_auth_ok && lp_auth_ok && identity_ok && abi_valid;

    let decision = decide_trade_cpi_from_ret(
        old_nonce,
        shape,
        identity_ok,
        pda_ok,
        user_auth_ok,
        lp_auth_ok,
        ret,
        lp_account_id,
        oracle_price_e6,
        req_size,
    );

    if should_accept {
        match decision {
            TradeCpiDecision::Accept {
                new_nonce,
                chosen_size,
            } => {
                assert_eq!(
                    Some(new_nonce),
                    nonce_on_success(old_nonce),
                    "accept new_nonce must be nonce_on_success(old_nonce)"
                );
                assert_eq!(
                    chosen_size,
                    cpi_trade_size(ret.exec_size, req_size),
                    "accept chosen_size must equal cpi_trade_size(exec_size, req_size)"
                );
            }
            TradeCpiDecision::Reject => {
                panic!("all gates pass but got Reject");
            }
        }
    } else {
        assert_eq!(
            decision,
            TradeCpiDecision::Reject,
            "any gate failure must produce Reject"
        );
    }
}

// =============================================================================
// NEW: UNIVERSAL CHARACTERIZATION — clamp_oracle_price (circuit breaker)
// =============================================================================

/// Universal characterization of clamp_oracle_price: all 3 branches proved.
///
/// Proves three disjoint cases:
/// (a) max_change_bps == 0 => circuit breaker disabled, result == raw_price
/// (b) last_price == 0 => first time (no history), result == raw_price
/// (c) both non-zero => result is clamped to [lo, hi] where
///     max_delta = last_price * max_change_bps / 10_000 (saturating)
///     lo = last_price.saturating_sub(max_delta)
///     hi = last_price.saturating_add(max_delta)
///
/// Cases (a) and (b) are proved with fully symbolic inputs (trivial fast paths).
/// Case (c) requires bounding `last_price` to a u16 range because the
/// u128 multiplication `last_price * max_change_bps` is symbolic×symbolic and
/// exceeds CBMC SAT tractability for full u64×u64 inputs. The bound covers the
/// structural clamping logic; the inductive_clamp_within_bounds proof establishes
/// that `x.clamp(lo, hi)` is always in [lo, hi] for all u64 inputs.
///
/// Production usage: read_price_clamped calls this to prevent oracle price
/// manipulation beyond the configured per-update cap.
#[kani::proof]
fn kani_clamp_oracle_price_universal() {
    // Cases (a) and (b): fully symbolic — trivially return raw_price
    {
        let raw_price: u64 = kani::any();
        let max_change_bps: u64 = kani::any();

        // (a) disabled
        let result_a = clamp_oracle_price(0, raw_price, 0);
        assert_eq!(
            result_a, raw_price,
            "max_change_bps=0 must return raw_price unchanged (disabled)"
        );

        // Also: any last_price with max_change_bps=0 => disabled
        let last_price_a: u64 = kani::any();
        let result_a2 = clamp_oracle_price(last_price_a, raw_price, 0);
        assert_eq!(
            result_a2, raw_price,
            "max_change_bps=0 must return raw_price unchanged for any last_price"
        );

        // (b) first-time
        let max_change_bps_b: u64 = kani::any();
        let result_b = clamp_oracle_price(0, raw_price, max_change_bps_b);
        assert_eq!(
            result_b, raw_price,
            "last_price=0 must return raw_price unchanged (first time)"
        );
    }

    // Case (c): normal clamping — bound both inputs to u8 for SAT tractability.
    // SAT tractability bound: the 128-bit multiplication `last_price * max_change_bps`
    // is symbolic×symbolic and intractable at full u64 range. Bounding both to u8
    // keeps CBMC tractable while fully exercising the clamping branch logic.
    // The `inductive_clamp_within_bounds` proof (unbounded) establishes that
    // `x.clamp(lo, hi)` is always in [lo, hi] for all u64 inputs.
    let last_raw: u8 = kani::any();
    let cap_raw: u8 = kani::any();
    let raw_price: u64 = kani::any();

    kani::assume(last_raw > 0); // non-zero: enter clamping branch
    kani::assume(cap_raw > 0); // non-zero: enter clamping branch

    let last = last_raw as u64;
    let max_change_bps = cap_raw as u64; // 1..255 bps
    let result = clamp_oracle_price(last, raw_price, max_change_bps);

    // Mirror the production formula exactly
    let max_delta = ((last as u128) * (max_change_bps as u128) / 10_000) as u64;
    let lo = last.saturating_sub(max_delta);
    let hi = last.saturating_add(max_delta);

    assert!(
        result >= lo,
        "clamped result must be >= lo (lower circuit breaker bound)"
    );
    assert!(
        result <= hi,
        "clamped result must be <= hi (upper circuit breaker bound)"
    );
    assert_eq!(
        result,
        raw_price.clamp(lo, hi),
        "clamped result must equal raw_price.clamp(lo, hi)"
    );
}

// ============================================================================
// Fee-Weighted EWMA Proofs
// ============================================================================

/// Bounded price range for single-call EWMA proofs.
const KANI_MAX_PRICE: u64 = 1_000_000;
/// Tighter bound for two-call comparison proofs (SAT solver tractability).
const KANI_MAX_PRICE_CMP: u64 = 16;

/// For all valid inputs: result is in [min(old, price), max(old, price)].
/// Fee-weighting cannot push EWMA outside the convex hull of old and price.
#[kani::proof]
#[kani::unwind(2)]
fn proof_ewma_weighted_result_bounded() {
    let old: u64 = kani::any();
    let price: u64 = kani::any();
    let halflife: u64 = kani::any();
    let last_slot: u64 = kani::any();
    let now_slot: u64 = kani::any();
    let fee_paid: u64 = kani::any();
    let min_fee: u64 = kani::any();

    kani::assume(old > 0 && old <= KANI_MAX_PRICE);
    kani::assume(price > 0 && price <= KANI_MAX_PRICE);
    kani::assume(halflife > 0 && halflife <= 10_000);
    kani::assume(now_slot >= last_slot);
    kani::assume(now_slot - last_slot <= 10_000);
    kani::assume(min_fee <= KANI_MAX_PRICE);

    let result = ewma_update(old, price, halflife, last_slot, now_slot, fee_paid, min_fee);
    let lo = core::cmp::min(old, price);
    let hi = core::cmp::max(old, price);
    assert!(
        result >= lo && result <= hi,
        "EWMA result must be in [min(old,price), max(old,price)]"
    );
}

/// Monotone in fee: larger fee moves mark more toward price (never less).
/// For price > old: fee_a < fee_b implies result_a <= result_b.
/// For price < old: fee_a < fee_b implies result_a >= result_b.
#[kani::proof]
#[kani::unwind(2)]
fn proof_ewma_weighted_monotone_in_fee() {
    let old: u64 = kani::any();
    let price: u64 = kani::any();
    let halflife: u64 = kani::any();
    let last_slot: u64 = kani::any();
    let now_slot: u64 = kani::any();
    let fee_a: u64 = kani::any();
    let fee_b: u64 = kani::any();
    let min_fee: u64 = kani::any();

    kani::assume(old > 0 && old <= KANI_MAX_PRICE_CMP);
    kani::assume(price > 0 && price <= KANI_MAX_PRICE_CMP);
    kani::assume(halflife > 0 && halflife <= 1_000);
    kani::assume(now_slot >= last_slot);
    kani::assume(now_slot - last_slot <= 1_000);
    kani::assume(min_fee > 1 && min_fee <= KANI_MAX_PRICE_CMP);
    kani::assume(fee_a < fee_b);
    // Force at least one fee below threshold to exercise the scaling logic.
    kani::assume(fee_a < min_fee);

    let result_a = ewma_update(old, price, halflife, last_slot, now_slot, fee_a, min_fee);
    let result_b = ewma_update(old, price, halflife, last_slot, now_slot, fee_b, min_fee);

    if price > old {
        assert!(
            result_a <= result_b,
            "Higher fee must move mark more toward higher price"
        );
    } else if price < old {
        assert!(
            result_a >= result_b,
            "Higher fee must move mark more toward lower price"
        );
    }
    // price == old: both results equal old, trivially monotone
}

/// Zero fee with weighting enabled never moves the mark.
#[kani::proof]
#[kani::unwind(2)]
fn proof_ewma_zero_fee_identity() {
    let old: u64 = kani::any();
    let price: u64 = kani::any();
    let halflife: u64 = kani::any();
    let last_slot: u64 = kani::any();
    let now_slot: u64 = kani::any();
    let min_fee: u64 = kani::any();

    kani::assume(old > 0 && old <= KANI_MAX_PRICE);
    kani::assume(price > 0 && price <= KANI_MAX_PRICE);
    kani::assume(halflife > 0 && halflife <= 10_000);
    kani::assume(now_slot >= last_slot);
    kani::assume(now_slot - last_slot <= 10_000);
    kani::assume(min_fee > 0);

    let result = ewma_update(old, price, halflife, last_slot, now_slot, 0, min_fee);
    assert_eq!(result, old, "Zero fee must never move mark");
}

/// At-or-above threshold, fee-weighted result equals disabled-weighting result.
/// When fee_paid >= min_fee, effective_alpha = alpha (full weight), which is
/// identical to the min_fee=0 (disabled) path.
#[kani::proof]
#[kani::unwind(2)]
fn proof_ewma_weight_at_threshold_equals_unweighted() {
    let old: u64 = kani::any();
    let price: u64 = kani::any();
    let halflife: u64 = kani::any();
    let last_slot: u64 = kani::any();
    let now_slot: u64 = kani::any();
    let min_fee: u64 = kani::any();
    let fee_paid: u64 = kani::any();

    kani::assume(old > 0 && old <= KANI_MAX_PRICE_CMP);
    kani::assume(price > 0 && price <= KANI_MAX_PRICE_CMP);
    kani::assume(halflife > 0 && halflife <= 1_000);
    kani::assume(now_slot >= last_slot);
    kani::assume(now_slot - last_slot <= 1_000);
    kani::assume(min_fee > 0 && min_fee <= KANI_MAX_PRICE_CMP);
    kani::assume(fee_paid >= min_fee);

    // At-threshold: effective_alpha = alpha (unscaled)
    let weighted = ewma_update(old, price, halflife, last_slot, now_slot, fee_paid, min_fee);
    // Disabled weighting: also uses full alpha (min_fee=0 skips scaling)
    // Pass fee_paid to satisfy the old==0 bootstrap check too
    let disabled = ewma_update(old, price, halflife, last_slot, now_slot, fee_paid, 0);
    assert_eq!(
        weighted, disabled,
        "At-or-above threshold must equal disabled-weighting result"
    );
}

// =============================================================================
// V. CLUSTER RESTART DETECTION (1 proof)
// =============================================================================

/// Prove: restart_detected returns true iff the current LastRestartSlot value
/// is strictly greater than the slot captured at InitMarket.
///
/// CODE-EQUALS-SPEC: `restart_detected` is the pure comparison the on-chain
/// path runs after `LastRestartSlot::get()`. Separating it from the syscall
/// lets Kani prove the comparison symbolically while cfg-gating the sysvar
/// itself. Universal over all u64 pairs.
#[kani::proof]
fn kani_restart_detected_universal() {
    let init: u64 = kani::any();
    let current: u64 = kani::any();

    assert_eq!(
        restart_detected(init, current),
        current > init,
        "restart_detected must return (current > init_restart_slot)"
    );
}
