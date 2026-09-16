//! INV-002 - Asset generation binding.
//!
//! Normative obligation: a retained asset-specific trade binds the program-assigned asset
//! generation, so retirement and reuse of the same slot cannot redirect old consent to the
//! replacement asset.
//!
//! Evidence in this file (I/C):
//! `v16_attack_signed_trade_cannot_replay_across_asset_slot_reuse` constructs and signs each of
//! the four public trade routes against generation A, retires and permissionlessly reactivates the
//! same slot as generation B, and lands the retained transaction. Every route must return the
//! dedicated generation error with byte-exact rollback, including matcher context on CPI routes.
//! A freshly encoded generation-B trade must then land and create only generation-B legs, proving
//! the guard is not a blanket trading DoS. No program-owned state is injected.
//!
//! Guarantee boundary: this covers signed trade consent. Other retained asset-scoped controls are
//! tracked independently by the public-SBF and stateful INV-002 operation matrix.
//! The source roster also composes the three market-wide generation-frontier controls with
//! their existing public witnesses; INV-012 owns the retained matcher-grant regression.

use super::*;

fn inv002_source_defines_kani_proof(source: &str, function: &str) -> bool {
    let expected = format!("fn {function}");
    let mut proof_attribute = false;

    for line in source.lines() {
        let line = line.trim();
        if line == "#[kani::proof]" {
            proof_attribute = true;
        } else if line.starts_with("fn ") {
            if proof_attribute
                && line
                    .strip_prefix(&expected)
                    .is_some_and(|tail| tail.trim_start().starts_with('('))
            {
                return true;
            }
            proof_attribute = false;
        } else if proof_attribute && !line.is_empty() && !line.starts_with("#") {
            proof_attribute = false;
        }
    }

    false
}

fn inv002_source_defines_test(source: &str, function: &str) -> bool {
    let expected = format!("fn {function}");
    let mut test_attribute = false;

    for line in source.lines() {
        let line = line.trim();
        if line == "#[test]" {
            test_attribute = true;
        } else if line.starts_with("fn ") {
            if test_attribute
                && line
                    .strip_prefix(&expected)
                    .is_some_and(|tail| tail.trim_start().starts_with('('))
            {
                return true;
            }
            test_attribute = false;
        } else if test_attribute && !line.is_empty() && !line.starts_with("#") {
            test_attribute = false;
        }
    }

    false
}

fn source_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start = source
        .find(start)
        .unwrap_or_else(|| panic!("missing source boundary {start:?}"));
    let tail = &source[start..];
    let end = tail
        .find(end)
        .unwrap_or_else(|| panic!("missing source boundary {end:?}"));
    &tail[..end]
}

fn variant_body<'a>(instruction_enum: &'a str, variant: &str) -> &'a str {
    let marker = format!("{variant} {{");
    let start = instruction_enum
        .find(&marker)
        .unwrap_or_else(|| panic!("missing instruction variant {variant}"));
    let tail = &instruction_enum[start..];
    let end = tail.find("},").expect("variant terminator") + 2;
    &tail[..end]
}

#[test]
fn v16_program_asset_generation_field_and_guard_roster_is_source_complete() {
    crate::assert_certified_engine_pin("INV-002 asset-generation composition");

    let source = include_str!("../../../src/v16_program.rs");
    let public_generation_evidence =
        include_str!("../public_sbf/inv_002_asset_generation_binding.rs");
    let transaction_domain_evidence =
        include_str!("../public_sbf/inv_006_program_chain_message_type_and_version_binding.rs");
    let matcher_scope_evidence = include_str!("inv_012_capability_and_delegate_scope.rs");
    let matcher_frontier_evidence = include_str!("inv_012_reused_asset_return_binding.rs");
    let generated_generation_evidence =
        include_str!("../stateful/inv_002_asset_generation_binding.rs");
    let lifecycle_composition =
        include_str!("inv_065_reset_recovery_and_retired_state_isolation.rs");
    let ordering_composition = include_str!("inv_010_out_of_order_safety.rs");
    let predicate_proofs = include_str!("../kani/inv_002_asset_generation_binding.rs");
    let instruction_enum =
        source_between(source, "pub enum Instruction {", "\n    impl Instruction {");

    // Per-asset IDs and market-wide frontiers are separate retained generation bindings.
    assert_eq!(instruction_enum.matches("market_id: u64").count(), 17);
    for variant in [
        "TradeNoCpi",
        "TradeCpi",
        "TopUpInsurance",
        "TopUpInsuranceDomain",
        "TopUpBackingBucket",
        "WithdrawBackingBucket",
        "UpdateBackingFeePolicy",
        "WithdrawBackingBucketEarnings",
        "ConfigureHybridOracle",
        "ConfigureEwmaMark",
        "PushEwmaMark",
        "ConfigureAuthMark",
        "PushAuthMark",
        "RestartAssetOracle",
        "WithdrawInsuranceAsset",
        "UpdateAssetAuthority",
        "UpdateAssetLifecycle",
    ] {
        assert!(
            variant_body(instruction_enum, variant).contains("market_id: u64"),
            "{variant} lost its asset-generation binding"
        );
    }
    for leg in ["pub struct BatchTradeLeg", "pub struct BatchTradeCpiLeg"] {
        let body = source_between(source, leg, "\n    }");
        assert!(body.contains("market_id: u64"), "{leg} lost market_id");
    }
    assert_eq!(
        instruction_enum
            .matches("asset_generation_frontier: u64")
            .count(),
        3
    );
    for variant in [
        "ResolveMarket",
        "ConfigurePermissionlessResolve",
        "SetMatcherConfig",
    ] {
        assert!(
            variant_body(instruction_enum, variant).contains("asset_generation_frontier: u64"),
            "{variant} lost its retained generation-frontier binding"
        );
    }

    let mut test_witnesses = std::collections::BTreeSet::new();
    for (path, source, witness) in [
        (
            "tests/invariants/public_sbf/inv_002_asset_generation_binding.rs",
            public_generation_evidence,
            "v16_program_stale_backing_earnings_withdrawal_rejects_across_asset_generation",
        ),
        (
            "tests/invariants/public_sbf/inv_006_program_chain_message_type_and_version_binding.rs",
            transaction_domain_evidence,
            "retained_transaction_binds_program_market_kind_schema_and_blockhash",
        ),
        (
            "tests/invariants/stateful/inv_002_asset_generation_binding.rs",
            generated_generation_evidence,
            "v16_program_asset_generation_operation_matrix_discovers_stale_intents",
        ),
        (
            "tests/invariants/public_sbf/inv_002_asset_generation_binding.rs",
            public_generation_evidence,
            "v16_program_retained_activation_binds_exact_next_generation_frontier",
        ),
        (
            "tests/invariants/public_sbf/inv_002_asset_generation_binding.rs",
            public_generation_evidence,
            "v16_program_pr311_pr312_marketwide_controls_reject_after_asset_slot_reuse",
        ),
        (
            "tests/invariants/cu/inv_012_reused_asset_return_binding.rs",
            matcher_frontier_evidence,
            "v16_program_retained_matcher_grant_rejects_after_asset_generation_frontier_moves",
        ),
    ] {
        assert!(
            path.starts_with("tests/invariants/") && path.ends_with(".rs"),
            "INV-002 asset-generation test witness must resolve to an invariant source file: {path}"
        );
        assert!(
            witness.starts_with("v16_")
                || witness == "retained_transaction_binds_program_market_kind_schema_and_blockhash",
            "INV-002 asset-generation witness must be a reviewed regression: {path}#{witness}"
        );
        assert!(
            test_witnesses.insert((path, witness)),
            "duplicate INV-002 asset-generation test witness {path}#{witness}"
        );
        assert!(
            inv002_source_defines_test(source, witness),
            "INV-002 lost asset-generation test witness {path}#{witness}"
        );
    }
    assert_eq!(
        test_witnesses.len(),
        6,
        "INV-002 asset-generation test witness roster drift"
    );
    let mut proof_witnesses = std::collections::BTreeSet::new();
    for (path, source, witness) in [
        (
            "tests/invariants/kani/inv_002_asset_generation_binding.rs",
            predicate_proofs,
            "kani_v16_asset_generation_binding_is_exact",
        ),
        (
            "tests/invariants/kani/inv_002_asset_generation_binding.rs",
            predicate_proofs,
            "kani_v16_asset_lifecycle_binding_selects_current_or_frontier_exactly",
        ),
    ] {
        assert!(
            path.starts_with("tests/invariants/kani/") && path.ends_with(".rs"),
            "INV-002 asset-generation proof witness must resolve to a Kani invariant source file: {path}"
        );
        assert!(
            witness.starts_with("kani_v16_"),
            "INV-002 asset-generation proof witness must be a reviewed Kani proof: {path}#{witness}"
        );
        assert!(
            proof_witnesses.insert((path, witness)),
            "duplicate INV-002 asset-generation proof witness {path}#{witness}"
        );
        assert!(
            inv002_source_defines_kani_proof(source, witness),
            "INV-002 lost asset-generation proof witness {path}#{witness}"
        );
    }
    assert_eq!(
        proof_witnesses.len(),
        2,
        "INV-002 asset-generation proof witness roster drift"
    );
    assert!(lifecycle_composition
        .contains("proof_v16_restart_empty_asset_core_preserves_budgets_and_assigns_fresh_market"));
    assert!(lifecycle_composition.contains("contract_check_asset_restart_next_counters"));
    let ordering_witness = "v16_program_out_of_order_induction_composition_is_source_complete";
    assert!(ordering_witness.starts_with("v16_"));
    assert!(
        inv002_source_defines_test(ordering_composition, ordering_witness),
        "INV-002 lost ordering composition witness {ordering_witness}"
    );

    let matcher_config = variant_body(instruction_enum, "SetMatcherConfig");
    assert!(!matcher_config.contains("asset_index"));
    assert!(!matcher_config.contains("market_id"));
    let matcher_scope_witness =
        "v16_program_matcher_capability_route_roster_binds_every_current_scope";
    assert!(matcher_scope_witness.starts_with("v16_"));
    assert!(
        inv002_source_defines_test(matcher_scope_evidence, matcher_scope_witness),
        "INV-002 lost matcher-scope composition witness {matcher_scope_witness}"
    );
    for (parent, mount) in [
        (
            include_str!("../../v16_cu.rs"),
            "#[path = \"invariants/cu/inv_012_capability_and_delegate_scope.rs\"]\nmod inv_012_capability_and_delegate_scope;",
        ),
        (
            matcher_scope_evidence,
            "#[path = \"inv_012_joint_incarnation_binding.rs\"]\nmod joint_incarnation_binding;",
        ),
        (
            include_str!("inv_012_joint_incarnation_binding.rs"),
            "#[path = \"inv_012_reused_asset_return_binding.rs\"]\nmod reused_asset_return_binding;",
        ),
    ] {
        assert!(parent.contains(mount), "retained matcher-frontier witness is not mounted: {mount}");
    }

    let matcher = source_between(
        source,
        "fn handle_set_matcher_config<'a>(",
        "fn invoke_matcher_batch<'a>(",
    );
    let matcher_preflight = source_between(
        matcher,
        "let data = market_ai.try_borrow_data()?;",
        "ensure_portfolio_storage_for_market_slots(",
    );
    assert!(matcher_preflight
        .contains("state::read_asset_lifecycle_generation_preflight(&data, 0, true)?"));
    assert!(matcher_preflight.contains("if next_market_id != asset_generation_frontier {\n                return Err(PercolatorError::EngineStale.into());\n            }"));

    let frontier_guard = source_between(
        source,
        "fn require_asset_generation_frontier_view(",
        "fn reset_empty_asset_oracle_anchor_view(",
    );
    assert!(frontier_guard.contains("if group.header.next_market_id.get() != expected_frontier {\n            return Err(PercolatorError::AssetGenerationMismatch.into());\n        }"));
    for (handler, mutation) in [
        ("fn handle_resolve_market<'a>(", "resolve_market_view("),
        (
            "fn handle_configure_permissionless_resolve<'a>(",
            "advance_control_sequence_view(",
        ),
    ] {
        let preflight = source_between(source, handler, mutation);
        assert!(
            preflight.contains("require_asset_generation_frontier_view(&group, expected_asset_generation_frontier)?;"),
            "{handler} must reject a stale frontier before mutation"
        );
    }
    assert!(
        instruction_enum.contains("ClaimResolvedPayoutTopup,"),
        "resolved claims remain permissionless current-state transitions without retained asset consent"
    );

    for (handler, next) in [
        (
            "fn handle_withdraw_backing_bucket",
            "fn handle_withdraw_backing_bucket_earnings",
        ),
        (
            "fn handle_withdraw_backing_bucket_earnings",
            "fn handle_sync_backing_domain_ledger",
        ),
    ] {
        let body = source_between(source, handler, next);
        assert!(body.contains("expected_market_id: u64"));
        assert!(body.contains("verify_domain_withdrawal_preflight("));
        assert!(body.contains("require_asset_generation_view("));
    }

    let authority = source_between(
        source,
        "fn handle_update_asset_authority",
        "fn handle_update_base_unit_mints",
    );
    assert!(authority.contains("expected_market_id: u64"));
    assert!(authority.contains("require_asset_generation_view("));

    let lifecycle = source_between(
        source,
        "fn handle_update_asset_lifecycle",
        "fn handle_finalize_reset_side",
    );
    assert!(lifecycle.contains("expected_market_id: u64"));
    assert!(lifecycle.contains("read_asset_lifecycle_generation_preflight("));
    assert!(
        lifecycle
            .matches("require_asset_lifecycle_generation_view(")
            .count()
            >= 3
    );
}

#[test]
fn host_asset_generation_wire_migrations_roundtrip_and_reject_legacy_payloads() {
    let authority = ProgInstruction::UpdateAssetAuthority {
        asset_index: 1,
        market_id: 0x1122_3344_5566_7788,
        authority_epoch: 0x99aa_bbcc_ddee_ff00,
        kind: processor::ASSET_AUTH_ORACLE,
        new_pubkey: [0xab; 32],
    };
    let encoded_authority = authority.encode();
    assert_eq!(encoded_authority.len(), 52);
    assert_eq!(
        ProgInstruction::decode(&encoded_authority).unwrap(),
        authority
    );
    let mut legacy_authority = [0u8; 36];
    legacy_authority[0] = 65;
    assert!(ProgInstruction::decode(&legacy_authority).is_err());

    let lifecycle = ProgInstruction::UpdateAssetLifecycle {
        action: processor::ASSET_ACTION_ACTIVATE,
        asset_index: 1,
        market_id: 0x8877_6655_4433_2211,
        authority_epoch: 0,
        now_slot: 7,
        initial_price: 100,
        max_init_fee: 13,
        insurance_authority: [0x11; 32],
        insurance_operator: [0x22; 32],
        backing_bucket_authority: [0x33; 32],
        oracle_authority: [0x44; 32],
    };
    let encoded_lifecycle = lifecycle.encode();
    assert_eq!(encoded_lifecycle.len(), 180);
    assert_eq!(
        ProgInstruction::decode(&encoded_lifecycle).unwrap(),
        lifecycle
    );
    let mut epochless_lifecycle = [0u8; 172];
    epochless_lifecycle[0] = 40;
    assert!(ProgInstruction::decode(&epochless_lifecycle).is_err());
    let mut legacy_lifecycle = [0u8; 164];
    legacy_lifecycle[0] = 40;
    assert!(ProgInstruction::decode(&legacy_lifecycle).is_err());
}

#[test]
fn v16_attack_signed_trade_cannot_replay_across_asset_slot_reuse() {
    assert_signed_trade_cannot_replay_across_asset_slot_reuse();
}
