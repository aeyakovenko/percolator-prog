//! INV-009 - partial-fill and retry accounting.
//!
//! Normative obligation: a matcher cannot rewrite the quantity relationship of
//! an atomic batch. Single TradeCpi may accept an explicitly flagged partial,
//! but the deployed batch validator must accept only an exact requested fill.
//!
//! Evidence in this file (P): Kani executes both deployed matcher validators over
//! independent full-width fields. This is a local admission proof; actual
//! quantity, fee, OI, retry, and rollback effects are composed through LiteSVM in
//! the invariant's CU file.

use super::*;

#[kani::proof]
fn kani_v16_atomic_batch_accepts_only_exact_bound_matcher_fill() {
    let ret = MatcherReturn {
        abi_version: kani::any(),
        flags: kani::any(),
        exec_price_e6: kani::any(),
        exec_size: kani::any(),
        req_id: kani::any(),
        lp_account_id: kani::any(),
        oracle_price_e6: kani::any(),
        asset_index: kani::any(),
    };
    let lp_account_id: u64 = kani::any();
    let asset_index: u16 = kani::any();
    let oracle_price_e6: u64 = kani::any();
    let req_size: i128 = kani::any();
    let req_id: u64 = kani::any();

    let ordinary = validate_matcher_return(
        &ret,
        lp_account_id,
        asset_index,
        oracle_price_e6,
        req_size,
        req_id,
    );
    let atomic = validate_atomic_batch_matcher_return(
        &ret,
        lp_account_id,
        asset_index,
        oracle_price_e6,
        req_size,
        req_id,
    );

    if atomic.is_ok() {
        assert!(ordinary.is_ok());
        assert_eq!(ret.exec_size, req_size);
    }
    if ordinary.is_err() || ret.exec_size != req_size {
        assert!(atomic.is_err());
    }

    kani::cover!(atomic.is_ok());
    kani::cover!(ordinary.is_ok() && ret.exec_size != req_size && atomic.is_err());
}
