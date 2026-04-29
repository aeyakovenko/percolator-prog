//! Inventory of integration test files still gated behind
//! `feature = "legacy-tests"` after the v2 migration. Both remaining
//! entries are blocked by infrastructure that is not part of this
//! repository, not by the encoder/wire-format work that Phase 6
//! addressed.
//!
//! Re-enable with:
//!
//! ```text
//! cargo test --features legacy-tests
//! ```

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
    "needs the percolator-match BPF binary, which is built from a separate \
     crate not present in this repo. See ../percolator-match (out of scope). \
     The matcher-CPI handler itself (trade_cpi.rs, tag 10) is fully ported \
     and exercised via test_phase2_dispatch + the audit's confirmed-faithful \
     review of the per-instruction port."
);
disabled!(
    unit,
    "calls `percolator_prog::processor::process_instruction` directly. v2 \
     replaces native dispatch with Anchor's macro-generated entrypoint, so \
     this entry point no longer exists. The 9 pure-helper tests from this \
     file have been lifted to test_unit_v2.rs; the remaining 13 \
     process_instruction-based tests are duplicative of coverage already in \
     test_basic / test_economic_attack_vectors / test_security and would \
     need wholesale rewriting as litesvm flows."
);
