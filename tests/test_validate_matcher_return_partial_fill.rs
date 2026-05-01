//! Regression tests for `validate_matcher_return` partial-fill `FLAG_PARTIAL_OK` requirement.
//!
//! Context: see issue #73 — earlier deployed builds (commit `06f86fb`) only required
//! `FLAG_PARTIAL_OK` for zero-fills (`exec_size == 0`). Genuine partial fills
//! (`0 < exec_size.unsigned_abs() < req_size.unsigned_abs()`) bypassed the flag check.
//! Commit `482b648` added the missing check at `src/percolator.rs:1511-1515`.
//!
//! These tests pin the post-fix behavior so any regression of `482b648` is caught.
//! All four matcher-fill states are exercised:
//!
//!   * partial fill, no `FLAG_PARTIAL_OK`  → MUST be rejected (the C-16 fix)
//!   * partial fill, with `FLAG_PARTIAL_OK` → MUST be accepted
//!   * full fill,    no `FLAG_PARTIAL_OK`  → MUST be accepted (flag not required)
//!   * zero fill,    no `FLAG_PARTIAL_OK`  → MUST be rejected (pre-existing check)

use percolator_prog::constants::MATCHER_ABI_VERSION;
use percolator_prog::matcher_abi::{
    validate_matcher_return, MatcherReturn, FLAG_PARTIAL_OK, FLAG_VALID,
};

const LP_ACCOUNT_ID: u64 = 42;
const ORACLE_PRICE_E6: u64 = 138_000_000;
const REQ_SIZE: i128 = 1_000_000;
const REQ_ID: u64 = 99;

fn ret(exec_size: i128, flags: u32) -> MatcherReturn {
    MatcherReturn {
        abi_version: MATCHER_ABI_VERSION,
        flags,
        exec_price_e6: ORACLE_PRICE_E6,
        exec_size,
        req_id: REQ_ID,
        lp_account_id: LP_ACCOUNT_ID,
        oracle_price_e6: ORACLE_PRICE_E6,
        reserved: 0,
    }
}

fn check(r: &MatcherReturn) -> Result<(), solana_program::program_error::ProgramError> {
    validate_matcher_return(r, LP_ACCOUNT_ID, ORACLE_PRICE_E6, REQ_SIZE, REQ_ID)
}

#[test]
fn partial_fill_without_flag_partial_ok_is_rejected() {
    // Pre-`482b648` deployed builds accepted this; post-fix builds must reject it.
    let r = ret(1, FLAG_VALID);
    assert!(
        check(&r).is_err(),
        "partial fill (exec={} < req={}) without FLAG_PARTIAL_OK must be rejected; \
         regression of fix at src/percolator.rs:1511-1515",
        r.exec_size,
        REQ_SIZE
    );
}

#[test]
fn partial_fill_with_flag_partial_ok_is_accepted() {
    let r = ret(1, FLAG_VALID | FLAG_PARTIAL_OK);
    assert!(check(&r).is_ok(), "partial fill with FLAG_PARTIAL_OK must be accepted");
}

#[test]
fn full_fill_without_flag_partial_ok_is_accepted() {
    // exec_size == req_size is not a partial fill, so the flag is not required.
    let r = ret(REQ_SIZE, FLAG_VALID);
    assert!(
        check(&r).is_ok(),
        "full fill (exec_size == req_size) without FLAG_PARTIAL_OK must be accepted"
    );
}

#[test]
fn zero_fill_without_flag_partial_ok_is_rejected() {
    // Sanity: pre-existing check at the top of validate_matcher_return.
    let r = ret(0, FLAG_VALID);
    assert!(
        check(&r).is_err(),
        "zero fill without FLAG_PARTIAL_OK must be rejected (pre-existing check)"
    );
}
