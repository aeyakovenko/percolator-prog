//! INV-065 - Reset, recovery, and retired-state isolation.
//!
//! Normative obligation: lifecycle transitions cannot admit new risk into an
//! inconsistent episode or orphan existing user legs.
//!
//! Evidence in this file (F over public I routes): the shared stateful runner
//! configures permissionless recovery, shuts down a publicly active asset, and
//! then runs independent permissionless-progress and owner-exit campaigns. The
//! shutdown must be a real successful wrapper transition; every later success
//! is checked against the complete position/OI/source-credit/custody model, and
//! every rejection must roll back all tracked program bytes, SPL data, and
//! economic-account lamports. Generated scenarios also place ordinary public
//! actions before and after these lifecycle actions.
//!
//! This is bounded generated coverage, not exhaustive reset/recovery/retirement
//! reachability.

use super::*;

#[test]
fn v16_program_generated_shutdown_reaches_recovery_then_all_positions_exit() {
    let scenario = Scenario {
        seed: [0x65; 32],
        config: SmallMarketConfig::default(),
        actions: vec![
            Action::ConfigurePermissionlessResolve {
                stale_slots: 1_000,
                force_close_delay_slots: 100,
            },
            Action::ShutdownAsset { asset: 0, dt: 0 },
        ],
    };

    let coverage = run_scenario(&scenario)
        .expect("public recovery transition must preserve bounded progress and owner exits");
    assert_ne!(
        coverage.resolve_policy_updates, 0,
        "the recovery policy must be installed through the public wrapper"
    );
    assert_ne!(
        coverage.lifecycle_updates, 0,
        "the active asset must enter Recovery through the public wrapper"
    );
    assert_ne!(
        coverage.user_positions_closed, 0,
        "the post-shutdown owner-exit campaign must clear live positions"
    );
    assert!(
        coverage
            .known_blocker_hits
            .iter()
            .chain(coverage.known_blocker_exit_locks.iter())
            .all(|hits| *hits == 0),
        "the lifecycle witness must not rely on blocker quarantine: {coverage:?}"
    );
}
