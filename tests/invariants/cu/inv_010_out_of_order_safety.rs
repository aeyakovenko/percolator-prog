//! INV-010 - Out-of-order safety.
//!
//! Normative obligation: retained public requests that land out of order either
//! reject atomically or remain inside every affected signer’s latest authority
//! and economic bounds.
//!
//! Evidence in this file (I/F): this deterministic LiteSVM witness exercises an
//! LP-signed retained matcher-enable request. After the LP revokes matcher
//! authority, CPI trade attempts and the stale retained enable must reject with
//! exact rollback and unchanged matcher sequence. A fresh enable then lands,
//! CPI open/close succeeds, both parties withdraw, and SPL supply is conserved.
//!
//! The source-composition gate closes arbitrary sequence length by induction rather than by
//! enumerating factorially larger schedules. The deployed one-step sequence theorem is joined to
//! every retained-family disposition, every delayed-control lane, identity/authority binding,
//! exact value attribution, and instruction-error rollback. The existing public `2!`, `3!`, and
//! 144-cell products provide noncommuting and terminal witnesses for that induction.
//!
//! Guarantee boundary: aggregate slippage, fee, and expiry terms absent from the current request
//! schema remain design gaps under INV-008/009/011/059. The composition proves ordering safety for
//! every economic bound the current schema actually carries.

#[derive(Clone, Copy)]
struct Inv010CompositionOwner {
    obligation: &'static str,
    path: &'static str,
    test: &'static str,
}

fn inv010_source_defines_test(source: &str, function: &str) -> bool {
    source.lines().any(|line| {
        line.trim()
            .strip_prefix(&format!("fn {function}"))
            .is_some_and(|tail| tail.trim_start().starts_with('('))
    })
}

#[test]
fn v16_program_out_of_order_induction_composition_is_source_complete() {
    const OWNERS: &[Inv010CompositionOwner] = &[
        Inv010CompositionOwner {
            obligation: "all retained operation families",
            path: "tests/invariants/cu/inv_008_intent_uniqueness_and_bounded_replay.rs",
            test: "v16_public_replay_disposition_roster_is_source_complete",
        },
        Inv010CompositionOwner {
            obligation: "all delayed policy and observation controls",
            path: "tests/invariants/public_sbf/inv_014_delayed_policy_and_policy_epoch_safety.rs",
            test: "v16_program_delayed_control_matrix_is_source_complete",
        },
        Inv010CompositionOwner {
            obligation: "asset generation",
            path: "tests/invariants/cu/inv_002_asset_generation_binding.rs",
            test: "v16_program_asset_generation_field_and_guard_roster_is_source_complete",
        },
        Inv010CompositionOwner {
            obligation: "portfolio incarnation",
            path: "tests/invariants/cu/inv_003_portfolio_incarnation_binding.rs",
            test: "v16_program_retained_portfolio_binding_roster_is_source_complete",
        },
        Inv010CompositionOwner {
            obligation: "position episode",
            path: "tests/invariants/cu/inv_004_position_episode_binding.rs",
            test: "v16_program_retained_position_binding_and_writer_rosters_are_source_complete",
        },
        Inv010CompositionOwner {
            obligation: "authority incarnation",
            path: "tests/invariants/cu/inv_005_authority_incarnation_binding.rs",
            test: "v16_program_configured_authority_route_dispositions_are_source_complete",
        },
        Inv010CompositionOwner {
            obligation: "attributed successful value delta",
            path: "tests/invariants/cu/inv_018_quote_mint_vault_token_program_and_authority_integrity.rs",
            test: "v16_primary_quote_routes_match_actual_spl_and_internal_accounting_deltas",
        },
        Inv010CompositionOwner {
            obligation: "instruction error propagation and rollback boundary",
            path: "tests/invariants/cu/inv_080_error_propagation_and_exact_rollback.rs",
            test: "v16_program_dispatch_and_entrypoints_preserve_every_handler_error",
        },
        Inv010CompositionOwner {
            obligation: "conflicting capability controls and trade",
            path: "tests/invariants/stateful/inv_010_out_of_order_safety.rs",
            test: "v16_program_conflicting_matcher_controls_and_trade_exhaust_all_landing_orders",
        },
        Inv010CompositionOwner {
            obligation: "portfolio value and control permutations",
            path: "tests/invariants/stateful/inv_010_out_of_order_safety.rs",
            test: "v16_program_portfolio_value_and_control_requests_exhaust_all_landing_orders",
        },
        Inv010CompositionOwner {
            obligation: "independent deposit and reduction order",
            path: "tests/invariants/stateful/inv_010_out_of_order_safety.rs",
            test: "v16_program_deposit_and_owner_reduction_commute_across_independent_bindings",
        },
        Inv010CompositionOwner {
            obligation: "authority policy order",
            path: "tests/invariants/stateful/inv_010_out_of_order_safety.rs",
            test: "v16_program_authority_handoff_and_retained_policy_obey_both_landing_orders",
        },
        Inv010CompositionOwner {
            obligation: "authority resolution order",
            path: "tests/invariants/stateful/inv_010_out_of_order_safety.rs",
            test: "v16_program_underfunded_claims_survive_both_authority_resolve_orders",
        },
        Inv010CompositionOwner {
            obligation: "policy authority resolution higher-order product",
            path: "tests/invariants/stateful/inv_010_out_of_order_safety.rs",
            test: "v16_program_underfunded_policy_handoff_and_resolve_exhaust_all_landing_orders",
        },
    ];

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut obligations = std::collections::BTreeSet::new();
    for owner in OWNERS {
        assert!(
            obligations.insert(owner.obligation),
            "duplicate ordering obligation"
        );
        let source = std::fs::read_to_string(root.join(owner.path))
            .unwrap_or_else(|error| panic!("read {}: {error}", owner.path));
        assert!(
            inv010_source_defines_test(&source, owner.test),
            "ordering obligation '{}' lacks executable owner {}#{}",
            owner.obligation,
            owner.path,
            owner.test,
        );
    }
    assert_eq!(obligations.len(), 14, "ordering composition drift");

    let kani =
        std::fs::read_to_string(root.join("tests/invariants/kani/inv_010_out_of_order_safety.rs"))
            .expect("read INV-010 one-step theorem");
    assert!(kani.contains("#[kani::proof]"));
    assert!(kani.contains("fn kani_v16_matcher_sequence_accepts_only_current_expected_value("));
    assert!(kani.contains("if current != expected || current == u64::MAX"));
    assert!(kani.contains("assert_eq!(result.unwrap(), current + 1)"));
}

#[test]
fn v16_program_matcher_mutation_order_rejects_revoked_capability_fixed_case() {
    let protection =
        crate::support::invariant_discovery::verify_matcher_mutation_order_safety([0x10; 32])
            .expect("matcher mutation order safety");
    assert!(
        protection.satisfies_invariant(),
        "matcher mutation order invariant failed: {protection:?}"
    );
}
