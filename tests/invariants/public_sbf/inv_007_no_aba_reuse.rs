//! INV-007 - No ABA reuse.
//!
//! The matrix in this file runs public close/recreate attempts for the whole
//! market and then replays retained requests from the prior incarnation. `CloseSlab`
//! leaves a rent-exempt typed tombstone, so the old address is permanently retired:
//! same-address `InitMarket` and every retained old request must reject atomically.
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

    for discovery in &discoveries {
        assert!(
            discovery.certifies_no_reuse(),
            "{:?}: market address was reusable: {discovery:?}",
            discovery.kind
        );
        assert!(discovery.tombstone_lamports > 0);
        assert!(discovery.recreation_rejected);
        assert!(discovery.recreation_exact_rollback);
        assert!(discovery.retained_intent_rejected);
        assert!(discovery.retained_intent_exact_rollback);

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
        let rejected_steps: Vec<_> = trace.steps.iter().filter(|step| !step.succeeded).collect();
        assert!(
            rejected_steps.len() >= 2,
            "{:?}: trace must include rejected reinitialization and retained replay",
            discovery.kind
        );
        let replay = trace.steps.last().expect("trace checked nonempty");
        assert_eq!(
            replay.program_id,
            percolator_prog::id(),
            "{:?}: final replay did not target the deployed wrapper",
            discovery.kind
        );
        assert!(
            !replay.succeeded,
            "{:?}: stale replay landed",
            discovery.kind
        );
        assert_eq!(replay.compute_units, None);
    }

    let holdout_routes = [
        (293, MarketIntentKind::RebalanceReduce),
        (294, MarketIntentKind::MatcherEnable),
        (295, MarketIntentKind::ForfeitRecoveryLeg),
        (296, MarketIntentKind::TradeFeePolicy),
        (307, MarketIntentKind::Deposit),
        (317, MarketIntentKind::FeeRedirectPolicy),
        (325, MarketIntentKind::MaintenanceFeePolicy),
        (326, MarketIntentKind::LiquidationFeePolicy),
    ];
    for (pr, kind) in holdout_routes {
        assert!(
            discoveries
                .iter()
                .any(|discovery| discovery.kind == kind && discovery.certifies_no_reuse()),
            "PR {pr}: missing fixed-pin no-reuse evidence for {kind:?}"
        );
    }
}
