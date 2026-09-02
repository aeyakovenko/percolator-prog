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
    let marker = format!("fn {function}");
    let mut saw_test = false;
    for line in source.lines() {
        let line = line.trim();
        if line == "#[test]" {
            saw_test = true;
        } else if line.starts_with("fn ") {
            if saw_test
                && line
                    .strip_prefix(&marker)
                    .is_some_and(|tail| tail.trim_start().starts_with('('))
            {
                return true;
            }
            saw_test = false;
        } else if saw_test && !line.is_empty() && !line.starts_with('#') {
            saw_test = false;
        }
    }
    false
}

fn inv010_evidence_parts(evidence: &str) -> (&str, &str) {
    let evidence = evidence
        .split_once(':')
        .map_or(evidence, |(_, evidence)| evidence);
    evidence
        .split_once('#')
        .unwrap_or_else(|| panic!("history evidence must be path#test: {evidence}"))
}

#[test]
fn v16_program_every_public_route_has_an_explicit_history_relation() {
    const CLASS_RELATIONS: &[(&str, &str)] = &[
        ("initialization", "FirstLandedSerial"),
        ("linear_amount", "ExactEconomic"),
        ("state_progress", "SignedEnvelope"),
        ("trade_partition", "ConservativeBound"),
        ("terminal_administration", "FirstLandedSerial"),
        ("legacy_insurance_split", "ConservativeBound"),
        ("resolved_entitlement", "TerminalEntitlement"),
        ("claim_conversion", "ConservativeBound"),
        ("authority_serialization", "FirstLandedSerial"),
        ("control_supersession", "FirstLandedSerial"),
        ("lifecycle_serialization", "FirstLandedSerial"),
        ("close_episode", "FirstLandedSerial"),
        ("recovery_episode", "FirstLandedSerial"),
        ("reduction_partition", "ConservativeBound"),
        ("fee_cadence", "ConservativeBound"),
        ("observation_ledger", "SignedEnvelope"),
        ("insurance_withdrawal", "ExactEconomic"),
        ("authorized_configuration", "SignedEnvelope"),
    ];

    crate::assert_certified_engine_pin("INV-010 whole-route history relation census");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let class_relations = CLASS_RELATIONS
        .iter()
        .copied()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(class_relations.len(), CLASS_RELATIONS.len());

    let mut public_routes = std::collections::BTreeMap::new();
    for line in include_str!("../public_instruction_coverage.tsv").lines() {
        if line.starts_with('#') || line.is_empty() || line.starts_with("tag\t") {
            continue;
        }
        let fields = line.splitn(5, '\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 5, "malformed public-route row: {line}");
        let tag = fields[0].parse::<u8>().expect("numeric public tag");
        assert!(
            public_routes.insert(tag, (fields[1], fields[2])).is_none(),
            "duplicate public tag {tag}"
        );
    }
    assert_eq!(public_routes.len(), 49, "public instruction census drift");

    let mut dispositions = std::collections::BTreeMap::new();
    let mut used_classes = std::collections::BTreeSet::new();
    let mut used_relations = std::collections::BTreeSet::new();
    let mut source_cache = std::collections::BTreeMap::<String, String>::new();
    for line in include_str!("../inv_010_history_relation_dispositions.tsv").lines() {
        if line.starts_with('#') || line.is_empty() || line.starts_with("tag\t") {
            continue;
        }
        let fields = line.splitn(6, '\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 6, "malformed history row: {line}");
        let tag = fields[0].parse::<u8>().expect("numeric history tag");
        let variant = fields[1];
        let history_class = fields[2];
        let relation = fields[3];
        let evidence = fields[4];
        assert!(
            !fields[5].trim().is_empty(),
            "empty history boundary for {variant}"
        );
        assert_eq!(
            public_routes.get(&tag).map(|route| route.0),
            Some(variant),
            "history disposition must bind the production tag and variant"
        );
        assert_eq!(
            class_relations.get(history_class),
            Some(&relation),
            "invalid history class/relation pair for {variant}"
        );
        assert!(
            dispositions.insert(tag, variant).is_none(),
            "duplicate history disposition for tag {tag}"
        );
        used_classes.insert(history_class);
        used_relations.insert(relation);

        let (path, function) = inv010_evidence_parts(evidence);
        assert!(path.starts_with("tests/invariants/"));
        let source = source_cache.entry(path.to_owned()).or_insert_with(|| {
            std::fs::read_to_string(root.join(path))
                .unwrap_or_else(|error| panic!("read history evidence {path}: {error}"))
        });
        assert!(
            inv010_source_defines_test(source, function),
            "history row {variant} lacks executable evidence {evidence}"
        );

        let public_evidence = public_routes.get(&tag).unwrap().1;
        let (public_path, public_function) = inv010_evidence_parts(public_evidence);
        let public_source = source_cache
            .entry(public_path.to_owned())
            .or_insert_with(|| {
                std::fs::read_to_string(root.join(public_path))
                    .unwrap_or_else(|error| panic!("read public evidence {public_path}: {error}"))
            });
        assert!(
            inv010_source_defines_test(public_source, public_function),
            "history row {variant} lacks executable public-route evidence {public_evidence}"
        );
    }

    assert_eq!(dispositions.len(), 49, "history disposition census drift");
    assert_eq!(
        dispositions
            .keys()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        public_routes.keys().copied().collect(),
        "every public route needs exactly one history relation"
    );
    assert_eq!(
        used_classes,
        class_relations.keys().copied().collect(),
        "unused history classes hide stale proof-equivalence categories"
    );
    assert_eq!(
        used_relations,
        [
            "ConservativeBound",
            "ExactEconomic",
            "FirstLandedSerial",
            "SignedEnvelope",
            "TerminalEntitlement",
        ]
        .into_iter()
        .collect(),
        "history relation vocabulary drift"
    );

    for (path, theorem) in [
        (
            "tests/invariants/kani/inv_024_attributed_quote_value_conservation.rs",
            "kani_inv024_entitlement_envelope_is_inductive_over_arbitrary_history_step",
        ),
        (
            "tests/invariants/kani/inv_025_exact_stock_reconciliation.rs",
            "kani_inv025_observation_ledger_net_identity_is_history_inductive",
        ),
    ] {
        let proof = std::fs::read_to_string(root.join(path))
            .unwrap_or_else(|error| panic!("read history theorem {path}: {error}"));
        assert!(proof.contains("#[kani::proof]"));
        assert!(
            proof.contains(&format!("fn {theorem}(")),
            "missing history theorem {path}#{theorem}"
        );
    }
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
            obligation: "every public route has an explicit history relation",
            path: "tests/invariants/cu/inv_010_out_of_order_safety.rs",
            test: "v16_program_every_public_route_has_an_explicit_history_relation",
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
    assert_eq!(obligations.len(), 15, "ordering composition drift");

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
