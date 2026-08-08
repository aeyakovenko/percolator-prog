//! INV-030 - Credit-rate determinism and fail-closed behavior.
//!
//! Normative obligation: the persisted source-credit rate must equal an independent recomputation
//! from the current claim bound and unencumbered backing. Removing or expiring backing cannot make
//! the rate more favorable, while adding new backing may restore credit without deleting claims.
//!
//! Evidence in this file (F over public LiteSVM routes):
//! `v16_program_source_credit_rate_lifecycle_matches_independent_oracle` generates provider amounts
//! and an authenticated winning mark, creates a discounted claim through a real trade and crank,
//! then exercises backing addition, exact-expiry normalization, owner risk reduction with zero
//! source credit, and expired-bucket refill. The shared global postcondition recomputes every
//! primary and foreign source domain after every generated public action with overflow-free u128
//! long division independent of the engine's U256 routine. The persisted minimized seed is the
//! public trace that exposed the pre-fix lapsed-Fresh crank loop.
//!
//! Guarantee boundary: this covers deployed serialization and the generated lifecycle. The engine
//! owns the full-width pure arithmetic proof; broader reachability still requires the charter's
//! exhaustive model and all public source-credit mutation routes.

use super::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_source_credit_rate_lifecycle_matches_independent_oracle(
        seed in any::<[u8; 32]>(),
        initial_backing in 1u16..=100,
        added_backing in 1u16..=100,
        price_move in 5u8..=20,
    ) {
        let result =
            verify_source_credit_rate_lifecycle(seed, initial_backing, added_backing, price_move);
        prop_assert!(
            result.is_ok(),
            "public source-credit lifecycle diverged from its independent rate oracle: {}",
            result.unwrap_err()
        );
    }
}
