//! INV-050 - Cross-zero decomposition under pending-loss epochs.
//!
//! Normative obligation: a pending domain-loss barrier may block new or flipped exposure, but it
//! must not block an exact same-side reduction and must release through bounded permissionless
//! work. The finding-blind matrix creates a real bankruptcy close and its zero-basis pending-loss
//! obligation without mutating program-owned bytes. While that close owns the domain barrier, it
//! runs the same cross-zero and exact-exit requests through all four trade routes for both long and
//! short barrier domains. Each flip must reject with the shared runner's exact full-state rollback
//! oracle; each exact reduction must clear both auxiliary positions and effective OI without
//! rewriting the close. The retained cure then cancels the close, and bounded public cranks must
//! release the obligation.
//!
//! Evidence: public LiteSVM stateful composition plus route metamorphism and independent raw-leg,
//! effective-OI, custody, encumbrance, stock, rollback, and liveness oracles after every step.
//!
//! Guarantee boundary: one real pending-loss episode is reached for every route/orientation cell.
//! Multiple simultaneous barriers, cross-asset barrier ordering, and full-width cross-zero
//! quantities remain owned by INV-039, INV-052, INV-074, and the remaining INV-050 boundary
//! campaign.

use crate::support::fuzz_model::run_pending_barrier_cross_zero_probe;

#[test]
fn v16_program_pending_loss_barrier_rejects_flips_but_preserves_all_route_exits() {
    let evidence = run_pending_barrier_cross_zero_probe()
        .expect("pending-loss barrier cross-zero matrix must preserve bounded owner exits");
    assert_eq!(evidence.world_count, 8, "{evidence:?}");
    assert_eq!(evidence.route_worlds, [2; 4], "{evidence:?}");
    assert_eq!(evidence.long_barrier_worlds, 4, "{evidence:?}");
    assert_eq!(evidence.short_barrier_worlds, 4, "{evidence:?}");
    assert_eq!(
        evidence.rejected_cross_zero_worlds, evidence.world_count,
        "{evidence:?}"
    );
    assert_eq!(
        evidence.exact_exit_worlds, evidence.world_count,
        "{evidence:?}"
    );
    assert_eq!(
        evidence.released_barrier_worlds, evidence.world_count,
        "{evidence:?}"
    );
}
