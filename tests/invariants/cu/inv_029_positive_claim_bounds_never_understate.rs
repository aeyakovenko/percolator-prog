//! INV-029 - Positive claim bounds never understate.
//!
//! Normative obligation: market-level source-credit claim bounds must equal
//! the complete set of live portfolio-attributed positive claims. A conversion
//! or settlement route may burn a bound only with the matching account-local
//! claim delta.
//!
//! Evidence in this file (I/F): this deterministic LiteSVM wrapper test runs
//! the shared public-route lifecycle oracle with fixed non-boundary parameters:
//! two winners create claims in the same source domain, authenticated marks
//! partially burn those claims, the positions close, backing is added, and both
//! conversion orders are checked against an independent portfolio census after
//! every public transition.
//! The deployed profile has no approximate claim-bound buckets: every production source claim is
//! exact and atom-scaled. The source lock below keeps non-exact bound injection and rebucketing out
//! of the wrapper API, while the stateful complete-account census requires the persisted exact and
//! bound totals to remain equal after every generated public transition. Introducing an
//! approximate bucket is therefore a deliberate profile change that must replace this absence
//! proof with the charter's range-edge and rebucketing proofs.
//!
//! Guarantee boundary: this is one non-random whole-route witness for the same
//! invariant enforced by the stateful generator. It does not replace exhaustive
//! production account enumeration.

#[test]
fn v16_program_positive_claim_bounds_match_public_lifecycle_census() {
    crate::support::fuzz_model::verify_positive_claim_bound_attribution_lifecycle(
        [0x29; 32], 3, 13, true,
    )
    .expect("positive-claim bound public lifecycle census");
}

#[test]
fn v16_program_non_exact_claim_bound_routes_remain_absent_from_deployed_profile() {
    let source = include_str!("../../../src/v16_program.rs");
    let production = source
        .split("    #[cfg(test)]\n    mod tests")
        .next()
        .expect("production source prefix");

    for forbidden in [
        "add_source_positive_claim_bound_not_atomic",
        "claim_bound_bucket",
        "rebucket_claim",
    ] {
        assert!(
            !production.contains(forbidden),
            "non-exact claim-bound mechanism {forbidden} entered the public wrapper; INV-029 \
             requires range and rebucketing coverage before deployment",
        );
    }

    crate::assert_certified_engine_pin("INV-029 exact-claim profile evidence");
}

#[test]
fn v16_program_exact_claim_bound_composition_is_source_complete() {
    crate::assert_certified_engine_pin("INV-029 exact-claim composition");

    let model = include_str!("../../support/fuzz_model.rs");
    for required in [
        "fn assert_source_claim_bound_attribution(",
        "source.exact_positive_claim_num != source.positive_claim_bound_num",
        "attributed[domain] != source.positive_claim_bound_num",
        "domain_total != group.source_claim_bound_total_num",
        "assert_source_claim_bound_attribution(\"primary\"",
        "assert_source_claim_bound_attribution(\"foreign\"",
        "fn bounded_claim_state_changed(",
        "claim_changing_edge_count",
        "receipt_replacement_count",
    ] {
        assert!(
            model.contains(required),
            "shared public-transition model lost INV-029 relation {required}",
        );
    }

    let bounded =
        include_str!("../stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs");
    for required in [
        "fn v16_program_bounded_reference_graph_exhausts_public_action_words",
        "evidence.claim_changing_edge_count != 0",
        "evidence.receipt_replacement_count >= evidence.partial_receipt_seed_count as u64",
    ] {
        assert!(
            bounded.contains(required),
            "bounded deployed graph lost INV-029 witness {required}",
        );
    }

    let stateful = include_str!("../stateful/inv_029_positive_claim_bounds_never_understate.rs");
    for required in [
        "fn v16_program_claim_bound_boundary_partition_exhausts_public_lifecycle_grid",
        "fn v16_program_favorable_funding_claim_bounds_are_exact_across_routes_and_sides",
        "fn v16_program_stale_positive_claim_blocks_snapshot_until_exactly_materialized",
        "fn v16_program_partial_receipt_exactly_replaces_its_prior_claim_bound",
    ] {
        assert!(
            stateful.contains(required),
            "INV-029 public route matrix lost witness {required}",
        );
    }

    let transitions = include_str!("inv_088_global_summaries_are_not_account_local_proofs.rs");
    assert!(transitions.contains(
        "fn v16_program_every_wrapper_engine_transition_callsite_has_summary_disposition_and_witness"
    ));
}
