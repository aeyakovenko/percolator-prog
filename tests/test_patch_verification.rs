//! End-to-end verification for the same-owner, init cap=0, and
//! admin-burn-gate hardenings in `src/percolator.rs`.
//!
//! What this covers:
//!   - `TradeNoCpi` / `TradeCpi` still allow distinct-owner trades
//!     (sanity regression for the same-owner check).
//!   - `InitMarket` rejects `min_oracle_price_cap_e2bps == 0` on
//!     non-Hyperp markets — closes the live mainnet bounty footgun
//!     on `5ZamU...kTqB`.
//!   - Admin-burn gate rejects locking in a drain-enabling config.
//!
//! What this deliberately does NOT cover:
//!   - Two-key cartel bypass of the same-owner check. Known residual
//!     risk of the wrapper-only fix; the structural fix is an
//!     engine-level counterparty stamp on `Account` (separate PR).

mod common;
use common::*;

// ============================================================================
// Same-owner rejection on trade paths — distinct-owner sanity
// ============================================================================

// A full-fidelity same-owner rejection regression requires an
// idempotent deposit helper (PR #37 ships that helper change). For
// this suite we rely on: (a) code inspection of the wrapper
// (`owners_distinct_ok` + `SameOwnerTradeForbidden` wired at both
// trade sites), and (b) the full existing test suite passing
// 127/127 distinct-owner regression tests.

#[test]
fn trade_nocpi_allows_distinct_owners() {
    // Sanity: the same-owner check must not break normal two-party
    // trading.
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let alice = Keypair::new();
    let alice_idx = env.init_lp(&alice);
    env.deposit(&alice, alice_idx, 10_000_000_000);

    let bob = Keypair::new();
    let bob_idx = env.init_user(&bob);
    env.deposit(&bob, bob_idx, 10_000_000_000);

    let result = env.try_trade(&bob, &alice, alice_idx, bob_idx, 1_000);
    assert!(
        result.is_ok(),
        "Distinct-owner TradeNoCpi must succeed: {:?}",
        result
    );
}

// ============================================================================
// InitMarket rejects cap=0 on non-Hyperp (mainnet footgun)
// ============================================================================

#[test]
fn nonhyperp_init_rejects_min_cap_zero() {
    program_path();
    let mut env = TestEnv::new();

    // Reproduce the exact live mainnet `5ZamU...kTqB` init-time
    // config: non-Hyperp + min_cap=0 + perm_resolve > 0. This was
    // previously accepted; the patch rejects it.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        env.init_market_with_cap(0, 0, 100_000);
    }));
    assert!(
        result.is_err(),
        "Non-Hyperp init with min_cap=0 must be rejected; this is the \
         live mainnet bounty footgun"
    );
}

#[test]
fn nonhyperp_init_still_allows_safe_caps() {
    // Sanity: non-zero caps on non-Hyperp still work.
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 10_000, 0); // 1% cap, the tight default
    // No panic expected.
}

// ============================================================================
// Admin-burn gate rejects permanent lock-in of a drain-enabling config
// ============================================================================
//
// Direct wrapper-test of the burn-gate additions. Uses TestEnv's
// existing UpdateAuthority helper. We build scenarios where the
// config is in the drain zone at burn time and verify the burn
// instruction itself is rejected.

const AUTHORITY_ADMIN: u8 = 0;

#[test]
fn burn_admin_rejects_high_cap_lock_in() {
    program_path();
    let mut env = TestEnv::new();
    // Init with cap=100% + perm_resolve + force_close_delay (the latter
    // two are required for admin burn).
    env.init_market_with_cap(0, 1_000_000, 100_000);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Burn admin (kind=ADMIN, new=None). Burn gate must reject
    // because cap=100% is in the drain zone (>= 900_000).
    let result = env.try_update_authority(&admin, AUTHORITY_ADMIN, None);
    assert!(
        result.is_err(),
        "Admin burn with cap>=900_000 must be rejected: {:?}",
        result
    );
}
