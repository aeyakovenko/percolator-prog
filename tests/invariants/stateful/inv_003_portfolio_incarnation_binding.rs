//! INV-003 - Portfolio incarnation binding.
//!
//! Normative obligation: portfolio-scoped consent cannot cross close and
//! same-pubkey recreation. The fuzzer generates retained requests for the full
//! portfolio-intent registry, performs a public A -> B -> A owner cycle at the
//! same pubkey, and then replays A's original request after A owns the account
//! again. Every route must reject before mutation and preserve exact rollback.

use super::*;

#[test]
fn v16_program_cure_consent_binds_same_pubkey_portfolio_incarnation() {
    let evidence =
        crate::support::fuzz_model::run_cure_portfolio_incarnation_replay_probe([0x3c; 32])
            .expect("public cure-incarnation replay route");
    assert!(
        evidence.intermediate_portfolio_id > evidence.old_portfolio_id
            && evidence.new_portfolio_id > evidence.intermediate_portfolio_id,
        "{evidence:?}"
    );
    assert!(evidence.stale_replay_rejected, "{evidence:?}");
    assert!(evidence.rejected_exact_rollback, "{evidence:?}");
    assert_eq!(evidence.stale_source_debit, 0, "{evidence:?}");
    assert_eq!(evidence.stale_capital_credit, 0, "{evidence:?}");
    assert!(!evidence.stale_close_canceled, "{evidence:?}");
    assert!(evidence.fresh_cure_landed, "{evidence:?}");
    assert!(evidence.fresh_close_canceled, "{evidence:?}");
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_003_portfolio_incarnation_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_portfolio_incarnation_operation_matrix_rejects_stale_intents(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_portfolio_incarnation_replays(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), PortfolioIntentKind::ALL.len());
        for (expected, discovery) in PortfolioIntentKind::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.kind, expected);
            prop_assert!(
                discovery.intermediate_portfolio_id > discovery.old_portfolio_id
                    && discovery.new_portfolio_id > discovery.intermediate_portfolio_id,
                "{:?}: portfolio id did not advance across A-B-A recreation: {} -> {} -> {}",
                expected,
                discovery.old_portfolio_id,
                discovery.intermediate_portfolio_id,
                discovery.new_portfolio_id,
            );
            prop_assert!(
                !discovery.accepted_stale_intent,
                "{:?}: stale retained portfolio intent landed on a replacement account",
                expected
            );
            prop_assert!(
                !discovery.mutated_economic_state,
                "{:?}: rejected stale intent failed exact rollback",
                expected
            );
            prop_assert_eq!(
                discovery.compute_units,
                None,
                "{:?}: stale replay should have no successful CU result",
                expected
            );
            prop_assert_eq!(
                discovery.public_trace.out_of_band_economic_mutations,
                0,
                "{:?}: replay evidence must use public transitions only",
                expected,
            );
            let replay = discovery.public_trace.steps.last()
                .ok_or_else(|| TestCaseError::fail(format!("{expected:?}: missing replay trace")))?;
            prop_assert!(!replay.succeeded, "{:?}: stale replay trace", expected);
            prop_assert_eq!(
                replay.rejected_exact_writable_rollback,
                Some(true),
                "{:?}: stale replay must roll back every writable account",
                expected,
            );
            prop_assert!(
                replay.token_deltas.iter().all(|(_, delta)| *delta == 0),
                "{:?}: stale replay must move no SPL value",
                expected,
            );
        }
    }
}
