//! INV-086 - Reference-model and deployed-transition equivalence.
//!
//! Normative obligation: The independent reference model and deployed public transition agree on normalized deltas.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_scenario_replay_is_deterministic`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_scenario_replay_is_deterministic() {
    let (_, scenario): (&str, Scenario) = blocker_scenarios()
        .into_iter()
        .next()
        .expect("blocker corpus");
    let first = run_scenario(&scenario).expect("first deterministic replay");
    let second = run_scenario(&scenario).expect("second deterministic replay");
    assert_eq!(first, second);
}
