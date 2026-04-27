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
    test_basic,
    "v2-migration: encoders use legacy u8-tag + raw-byte arg packing; rewrite for #[discrim] + Borsh in Phase 6"
);
disabled!(
    test_admin,
    "v2-migration: instruction encoders + slab fixture (needs 8-byte disc prefix) pending Phase 6"
);
disabled!(
    test_conservation,
    "v2-migration: SPL token CPI + slab encoders pending Phase 6"
);
disabled!(
    test_economic_attack_vectors,
    "v2-migration: slab fixture + tag encoders pending Phase 6"
);
disabled!(
    test_envelope_gate,
    "v2-migration: InitMarket extended-tail encoder needs Borsh Option<T> rewrite (R2) in Phase 6"
);
disabled!(
    test_insurance,
    "v2-migration: insurance-flow ix encoders pending Phase 6"
);
disabled!(
    test_oracle,
    "v2-migration: oracle ix encoders + slab fixture pending Phase 4 (oracle.rs port) + Phase 6"
);
disabled!(
    test_resolution,
    "v2-migration: resolution ix encoders pending Phase 6"
);
disabled!(
    test_risk_buffer,
    "v2-migration: KeeperCrank Vec<u32> length-prefix change requires fixture rewrite in Phase 6"
);
disabled!(
    test_security,
    "v2-migration: cross-cutting ix encoders pending Phase 6"
);
disabled!(
    test_tradecpi,
    "v2-migration: matcher CPI + ctx.remaining_accounts() ix shape pending Phase 2 (trade_cpi.rs) + Phase 6"
);
disabled!(
    test_a1_siphon_regression,
    "v2-migration: insurance-siphon regression encoders pending Phase 6"
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
