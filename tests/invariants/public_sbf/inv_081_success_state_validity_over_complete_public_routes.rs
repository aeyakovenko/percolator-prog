//! INV-081 - Success-state validity over complete public routes.
//!
//! Normative obligation: Every successful wrapper-plus-engine route preserves global invariants and authorized deltas.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_blocker_corpus_is_public_sbf_and_exit_live`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! The shared runner propagates every failed progress or exit campaign directly; there is no
//! known-finding quarantine that can convert a funded lock into successful coverage.

use super::*;

#[test]
fn v16_program_blocker_corpus_is_public_sbf_and_exit_live() {
    for (name, scenario) in blocker_scenarios() {
        let coverage = run_scenario(&scenario).unwrap_or_else(|error| {
            panic!(
                "blocker corpus scenario {name} failed\nscenario={}\n{error}",
                serde_json::to_string_pretty(&scenario).unwrap()
            )
        });
        assert_ne!(
            coverage.user_positions_closed, 0,
            "safe corpus scenario {name} closed no funded public position"
        );
    }
}
