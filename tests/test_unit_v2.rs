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

/// `nonce_on_success` advances by 1 on every success, returns None at
/// u64::MAX so the wrapper rejects rather than wrapping (which would
/// reopen request_id 0 to replay).
#[test]
fn test_nonce_on_success_normal() {
    use percolator_prog::policy::nonce_on_success;
    assert_eq!(nonce_on_success(0), Some(1));
    assert_eq!(nonce_on_success(42), Some(43));
    assert_eq!(nonce_on_success(u64::MAX - 1), Some(u64::MAX));
}

#[test]
fn test_nonce_on_success_rejects_overflow() {
    use percolator_prog::policy::nonce_on_success;
    assert_eq!(
        nonce_on_success(u64::MAX),
        None,
        "nonce_on_success(u64::MAX) must return None, not wrap to 0"
    );
}

#[test]
fn test_nonce_overflow_does_not_reopen_request_id_space() {
    use percolator_prog::policy::nonce_on_success;
    let at_max = nonce_on_success(u64::MAX);
    assert!(at_max.is_none(), "Must reject at u64::MAX");

    let before_max = nonce_on_success(u64::MAX - 1);
    assert_eq!(
        before_max,
        Some(u64::MAX),
        "u64::MAX-1 should advance to u64::MAX",
    );
}

/// `base_to_units` / `units_to_base_checked` must round-trip when
/// scale=0 (no conversion) and split base into (units, dust) when
/// scale>0.
#[test]
fn test_unit_scale_conversion() {
    use percolator_prog::units::{base_to_units, units_to_base_checked};

    // scale=0: identity.
    assert_eq!(base_to_units(12345, 0), (12345, 0));
    assert_eq!(units_to_base_checked(12345, 0), Some(12345));

    // scale=1000: 5500 base = 5 units + 500 dust.
    assert_eq!(base_to_units(5500, 1000), (5, 500));
    assert_eq!(base_to_units(5000, 1000), (5, 0));
    assert_eq!(units_to_base_checked(5, 1000), Some(5000));

    // scale=100: 201 base = 2 units + 1 dust.
    assert_eq!(base_to_units(201, 100), (2, 1));
    assert_eq!(units_to_base_checked(2, 100), Some(200));
}
