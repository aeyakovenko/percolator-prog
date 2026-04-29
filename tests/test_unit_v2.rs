//! Pure unit tests lifted from `tests/unit.rs`. The legacy unit suite
//! depends on `percolator_prog::processor::process_instruction` (a
//! native entry point that v2 replaces with Anchor's macro-generated
//! dispatch); the tests below are the subset that exercise only
//! `oracle`, `matcher_abi`, and engine-level type contracts and so
//! lift cleanly into the v2 crate.

use bytemuck::Zeroable;
use percolator_prog::matcher_abi::{validate_matcher_return, MatcherReturn, FLAG_PARTIAL_OK, FLAG_VALID};
use percolator_prog::{constants, oracle, state};
use solana_program_error::ProgramError;

/// Matcher returning a partial fill (`exec_size < requested`) without
/// the explicit `FLAG_PARTIAL_OK` flag must be rejected.
#[test]
fn test_matcher_nonzero_partial_requires_partial_ok() {
    let ret = MatcherReturn {
        abi_version: constants::MATCHER_ABI_VERSION,
        flags: FLAG_VALID,
        exec_price_e6: 100_000_000,
        exec_size: 50,
        req_id: 7,
        lp_account_id: 11,
        oracle_price_e6: 100_000_000,
        reserved: 0,
    };

    // Partial without FLAG_PARTIAL_OK → reject.
    assert_eq!(
        validate_matcher_return(
            &ret,
            ret.lp_account_id,
            ret.oracle_price_e6,
            100, // requested_size > exec_size
            ret.req_id,
        ),
        Err(ProgramError::InvalidAccountData)
    );

    let ret_with_partial = MatcherReturn {
        flags: FLAG_VALID | FLAG_PARTIAL_OK,
        ..ret
    };
    assert!(validate_matcher_return(
        &ret_with_partial,
        ret_with_partial.lp_account_id,
        ret_with_partial.oracle_price_e6,
        100,
        ret_with_partial.req_id,
    )
    .is_ok());
}

/// On a flat market (no open interest), an external oracle price is
/// admitted without clamping — the engine adopts the raw target as
/// both `last_effective_price_e6` and `oracle_target_price_e6`.
#[test]
fn test_external_oracle_flat_market_uses_raw_target() {
    let mut config = state::MarketConfig::zeroed();

    let price = oracle::clamp_external_price(
        &mut config,
        Ok((120_000_000, 1)),
        100_000_000,
        1,
        0,
        false,
    )
    .unwrap();

    assert_eq!(price, 120_000_000);
    assert_eq!(config.last_effective_price_e6, 120_000_000);
    assert_eq!(config.oracle_target_price_e6, 120_000_000);
}

/// With open interest, the same external read is clamped to the prior
/// `last_effective_price_e6` (zero-dt clamp). The raw target is still
/// stamped for later catch-up.
#[test]
fn test_external_oracle_with_open_interest_respects_zero_dt_clamp() {
    let mut config = state::MarketConfig::zeroed();

    let price = oracle::clamp_external_price(
        &mut config,
        Ok((120_000_000, 1)),
        100_000_000,
        1,
        0,
        true,
    )
    .unwrap();

    assert_eq!(price, 100_000_000);
    assert_eq!(config.last_effective_price_e6, 100_000_000);
    assert_eq!(config.oracle_target_price_e6, 120_000_000);
}

/// Sanity that the engine struct sizes are stable. The legacy test
/// printed offsets; we just assert non-trivial sizes so a refactor that
/// accidentally collapses Account to zero-size triggers CI.
#[test]
fn test_struct_sizes_nonzero() {
    use core::mem::size_of;
    use percolator::{Account, RiskEngine};

    assert!(size_of::<Account>() > 0);
    assert!(size_of::<RiskEngine>() > size_of::<Account>());
}

/// Zero-bytes-to-Account transmute safety: every field of the engine's
/// `Account` must have zero as a valid bit pattern (no `bool`, no
/// `NonZero*`, no references). Surfaces the invariant in CI so a future
/// field type that breaks it cannot land silently.
#[test]
fn test_zc_cast_safety_invariant() {
    use core::mem::size_of;
    use percolator::Account;

    let zero = vec![0u8; size_of::<Account>()];
    // SAFETY: percolator::Account is `#[repr(C)]` Pod-shaped (every
    // field is U128 / I128 / u8 / u64 / u128 / i128 / [u8;N]); all-zero
    // bytes are a valid bit pattern for every field. If a future field
    // type (bool, NonZero*, refs) breaks this, this transmute reads UB
    // and miri/sanitizers flag it.
    let acc: &Account = unsafe { &*(zero.as_ptr() as *const Account) };
    // Touch one field to force the load.
    let _ = acc.kind;
}
