//! Inventory of integration tests gated out during the v2 migration.
//!
//! Each `#[ignore]` here corresponds to an existing `tests/<name>.rs` file
//! whose body is wrapped in `#![cfg(feature = "legacy-tests")]`. The original
//! test sources are preserved untouched on disk; they assemble instructions
//! by hand-packing the legacy single-byte tag + raw-byte arg layout, which
//! Phase 6 will replace with `#[discrim] + Borsh args` encoders.
//!
//! Re-enable everything with:
//!
//! ```text
//! cargo test --features legacy-tests
//! ```
//!
//! Reason format: `"v2-migration: <what's blocking>"`.

macro_rules! disabled {
    ($name:ident, $reason:expr) => {
        #[test]
        #[ignore = $reason]
        fn $name() {
            // Body intentionally empty: the original test source lives at
            // tests/$name.rs and is gated via #![cfg(feature = "legacy-tests")].
        }
    };
}

disabled!(
    test_tradecpi,
    "v2-migration: matcher CPI + ctx.remaining_accounts() ix shape pending Phase 2 (trade_cpi.rs) + Phase 6"
);
disabled!(
    cu_benchmark,
    "v2-migration: CU baselines must be re-recorded after Phase 6 (Anchor v2 macro-dispatch shifts CU envelopes)"
);
disabled!(
    i128_alignment,
    "v2-migration: BPF u128 alignment probes pending Phase 6 (slab disc prefix shifts engine offsets)"
);
disabled!(
    kani,
    "v2-migration: kani harness imports percolator_prog::matcher_abi (not yet ported); pending Phase 2"
);
disabled!(
    unit,
    "v2-migration: native-tier unit tests (solana_program / spl_token) pending Phase 6"
);
