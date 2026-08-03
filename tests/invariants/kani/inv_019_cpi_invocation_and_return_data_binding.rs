//! INV-019 - CPI invocation and return-data binding.
//!
//! Normative obligation: The symbolic matcher return is accepted only when flags and echoed
//! request fields bind the exact invocation.
//!
//! Evidence in this file (P): Kani executes the deployed wrapper arithmetic, decoder, or
//! matcher-validation code over symbolic inputs. These leaf/local proofs do not establish
//! wrapper-plus-engine whole-route conservation or liveness on their own.
//!
//! Guarantee boundary: this proves the wrapper's matcher-return validator, not CPI provenance,
//! account validation, engine transition safety, token movement, or SVM rollback.

use super::*;

#[kani::proof]
fn kani_v16_matcher_return_accepts_only_bound_echoed_fills() {
    // Audit fix: the ret's echoed fields and abi_version are drawn INDEPENDENTLY of the
    // expected (bound) params, and sizes are full-width i128, so both the accept path AND
    // every rejection branch (abi mismatch, echoed-field mismatch, zero exec price, flag
    // checks, size guards) are symbolically exercised — not just the accept path.
    let abi_version: u32 = kani::any();
    let flags: u32 = kani::any();
    let exec_price_e6: u64 = kani::any();
    let exec_size: i128 = kani::any();
    let req_id_ret: u64 = kani::any();
    let lp_ret: u64 = kani::any();
    let oracle_ret: u64 = kani::any();
    let asset_ret: u64 = kani::any();
    // Bound (expected) params the validator echoes against — independent symbolics.
    let lp_account_id: u64 = kani::any();
    let asset_index: u16 = kani::any();
    let oracle_price_e6: u64 = kani::any();
    let req_size: i128 = kani::any();
    let req_id: u64 = kani::any();

    let ret = MatcherReturn {
        abi_version,
        flags,
        exec_price_e6,
        exec_size,
        req_id: req_id_ret,
        lp_account_id: lp_ret,
        oracle_price_e6: oracle_ret,
        asset_index: asset_ret,
    };

    let result = validate_matcher_return(
        &ret,
        lp_account_id,
        asset_index,
        oracle_price_e6,
        req_size,
        req_id,
    );

    // Rejection direction (the binding security property): a return with the wrong ABI,
    // a non-VALID/REJECTED flag state, any echoed field not bound to the expected param,
    // or a zero exec price MUST be rejected.
    if abi_version != percolator_prog::constants::MATCHER_ABI_VERSION
        || (flags & FLAG_VALID) == 0
        || (flags & FLAG_REJECTED) != 0
        || lp_ret != lp_account_id
        || oracle_ret != oracle_price_e6
        || asset_ret != asset_index as u64
        || req_id_ret != req_id
        || exec_price_e6 == 0
    {
        assert!(result.is_err());
    }

    // Accept direction: an accepted fill is bound to every expected field and within the
    // requested size, with the partial flag set whenever the fill is short.
    if result.is_ok() {
        assert!((flags & FLAG_VALID) != 0);
        assert!((flags & FLAG_REJECTED) == 0);
        assert_eq!(lp_ret, lp_account_id);
        assert_eq!(oracle_ret, oracle_price_e6);
        assert_eq!(asset_ret, asset_index as u64);
        assert_eq!(req_id_ret, req_id);
        assert!(exec_price_e6 != 0);
        if exec_size == 0 {
            assert!((flags & FLAG_PARTIAL_OK) != 0);
            assert_eq!(exec_price_e6, oracle_price_e6);
        } else {
            assert_eq!(exec_size.signum(), req_size.signum());
            assert!(exec_size.unsigned_abs() <= req_size.unsigned_abs());
            if exec_size.unsigned_abs() < req_size.unsigned_abs() {
                assert!((flags & FLAG_PARTIAL_OK) != 0);
            }
        }
    }
    // Ensure the accept path is reachable (non-vacuity of the accept assertions).
    kani::cover!(result.is_ok());
}
