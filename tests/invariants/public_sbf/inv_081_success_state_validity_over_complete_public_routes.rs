//! INV-081 - Success-state validity over complete public routes.
//!
//! Normative obligation: Every successful wrapper-plus-engine route preserves global invariants and authorized deltas.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_blocker_corpus_is_public_sbf_and_exit_live`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

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
        assert!(
            coverage
                .known_blocker_exit_locks
                .iter()
                .all(|hits| *hits == 0),
            "safe corpus scenario {name} reached a quarantined user-exit lock"
        );
    }
}
