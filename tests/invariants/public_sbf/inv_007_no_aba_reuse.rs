//! INV-007 - No ABA reuse.
//!
//! The matrix in this file runs public close/recreate sequences for the whole
//! market and then replays retained requests from the prior incarnation. It is
//! intentionally a discovery owner, not a green certification: fixed routes must
//! reject with exact rollback, while any still-accepted stale route is recorded as
//! a bounded public ABA counterexample. This prevents market/asset/portfolio
//! generation regressions from being hidden in leaf tests.
//!
//! Secondary coverage: INV-079. Every cell enables the shared public-trace
//! recorder before the old-generation request is retained. The trace records
//! transaction signers, actual compiled account metas, exact tracked token and
//! lamport deltas, classifies the transaction fee on rejected calls, verifies
//! program rollback, and detects any between-transaction mutation of
//! program-owned or economic fixture accounts.

use crate::support::invariant_discovery::{discover_market_incarnation_replays, MarketIntentKind};

#[test]
fn v16_program_whole_market_recreate_aba_matrix_is_public_and_nonvacuous() {
    let discoveries = discover_market_incarnation_replays([0x07; 32])
        .unwrap_or_else(|error| panic!("INV-007 whole-market ABA matrix failed: {error}"));
    assert_eq!(discoveries.len(), MarketIntentKind::ALL.len());

    let mut accepted = Vec::new();
    let mut rejected = Vec::new();
    for discovery in &discoveries {
        let trace = &discovery.public_trace;
        assert_eq!(
            trace.out_of_band_economic_mutations, 0,
            "{:?}: ABA witness used out-of-band economic state mutation",
            discovery.kind
        );
        assert!(
            !trace.steps.is_empty(),
            "{:?}: ABA witness emitted no public transaction trace",
            discovery.kind
        );
        for (step_index, step) in trace.steps.iter().enumerate() {
            assert!(
                !step.transaction_signers.is_empty(),
                "{:?} step {step_index}: transaction signer roster is empty",
                discovery.kind
            );
            assert!(
                !step.accounts.is_empty(),
                "{:?} step {step_index}: compiled account-meta roster is empty",
                discovery.kind
            );
            assert!(
                !step.token_deltas.is_empty() && !step.lamport_deltas.is_empty(),
                "{:?} step {step_index}: exact external delta frame is absent",
                discovery.kind
            );
            if !step.succeeded {
                assert_eq!(
                    step.rejected_exact_writable_rollback,
                    Some(true),
                    "{:?} step {step_index}: rejected public call did not roll back writable accounts",
                    discovery.kind
                );
                assert_eq!(
                    step.rejected_no_program_lamport_delta,
                    Some(true),
                    "{:?} step {step_index}: rejected public call changed non-fee lamports",
                    discovery.kind
                );
                assert!(
                    step.token_deltas.iter().all(|(_, delta)| *delta == 0),
                    "{:?} step {step_index}: rejected public call changed SPL value",
                    discovery.kind
                );
            }
        }
        let replay = trace.steps.last().expect("trace checked nonempty");
        assert_eq!(
            replay.program_id,
            percolator_prog::id(),
            "{:?}: final replay did not target the deployed wrapper",
            discovery.kind
        );
        assert_eq!(
            replay.succeeded, discovery.accepted_stale_intent,
            "{:?}: trace/replay disposition mismatch",
            discovery.kind
        );
        assert_eq!(
            replay.compute_units, discovery.compute_units,
            "{:?}: trace/replay compute evidence mismatch",
            discovery.kind
        );
        assert!(
            discovery.new_market_id >= discovery.old_market_id,
            "{:?}: replacement market id regressed: {} -> {}",
            discovery.kind,
            discovery.old_market_id,
            discovery.new_market_id,
        );
        if discovery.accepted_stale_intent {
            assert!(
                discovery.mutated_economic_state,
                "{:?}: stale retained market request landed without an observable delta",
                discovery.kind,
            );
            assert!(
                discovery.compute_units.is_some_and(|cu| cu < 1_400_000),
                "{:?}: accepted stale retained request must have bounded CU evidence",
                discovery.kind,
            );
            accepted.push(discovery.kind);
        } else {
            assert!(
                !discovery.mutated_economic_state,
                "{:?}: rejected stale retained request failed exact rollback",
                discovery.kind,
            );
            assert_eq!(
                discovery.compute_units, None,
                "{:?}: rejected stale retained request should not report success CU",
                discovery.kind,
            );
            rejected.push(discovery.kind);
        }
    }

    assert!(
        !accepted.is_empty(),
        "matrix should remain non-vacuous until whole-market incarnation binding is fixed",
    );
    eprintln!(
        "INV-007 whole-market ABA accepted stale routes: {accepted:?}; rejected stale routes: {rejected:?}",
    );
}
