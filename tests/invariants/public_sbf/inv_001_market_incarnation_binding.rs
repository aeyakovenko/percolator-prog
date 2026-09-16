//! INV-001 - Market incarnation binding.
//!
//! The deployed wrapper enforces a stricter policy than reusable generation IDs: once
//! `CloseSlab` succeeds, that market pubkey is permanently retired behind a typed,
//! rent-exempt tombstone. This representative public route proves same-address
//! `InitMarket` and a retained terminal control both reject with exact rollback.
//! INV-007 owns the complete retained-operation matrix and fresh-address liveness.

use crate::support::invariant_discovery::{discover_market_incarnation_replay, MarketIntentKind};

fn inv001_source_defines_test(source: &str, function: &str) -> bool {
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

#[test]
fn v16_program_closed_market_incarnation_cannot_be_recreated() {
    let protection =
        discover_market_incarnation_replay([0x01; 32], MarketIntentKind::ResolveMarket)
            .unwrap_or_else(|error| panic!("INV-001 market retirement failed: {error}"));
    assert!(protection.certifies_no_reuse(), "{protection:?}");
    assert!(protection.recreation_rejected);
    assert!(protection.recreation_exact_rollback);
    assert!(protection.retained_intent_rejected);
    assert!(protection.retained_intent_exact_rollback);
}

#[test]
fn v16_program_market_incarnation_and_transaction_domain_composition_is_source_complete() {
    let own_source = include_str!("inv_001_market_incarnation_binding.rs");
    let generated_matrix = include_str!("../stateful/inv_001_market_incarnation_binding.rs");
    let account_census = include_str!("inv_007_no_aba_reuse.rs");
    let transaction_domain =
        include_str!("inv_006_program_chain_message_type_and_version_binding.rs");
    let ordering = include_str!("../cu/inv_010_out_of_order_safety.rs");
    let mut composition_witnesses = std::collections::BTreeSet::new();
    for (path, source, witness) in [
        (
            "tests/invariants/public_sbf/inv_001_market_incarnation_binding.rs",
            own_source,
            "v16_program_closed_market_incarnation_cannot_be_recreated",
        ),
        (
            "tests/invariants/stateful/inv_001_market_incarnation_binding.rs",
            generated_matrix,
            "v16_program_market_incarnation_operation_matrix_rejects_address_reuse",
        ),
        (
            "tests/invariants/public_sbf/inv_007_no_aba_reuse.rs",
            account_census,
            "v16_wrapper_account_incarnation_census_is_source_complete",
        ),
        (
            "tests/invariants/public_sbf/inv_006_program_chain_message_type_and_version_binding.rs",
            transaction_domain,
            "retained_transaction_binds_program_market_kind_schema_and_blockhash",
        ),
        (
            "tests/invariants/public_sbf/inv_006_program_chain_message_type_and_version_binding.rs",
            transaction_domain,
            "deployed_wrapper_has_no_detached_signature_interpreter",
        ),
        (
            "tests/invariants/cu/inv_010_out_of_order_safety.rs",
            ordering,
            "v16_program_out_of_order_induction_composition_is_source_complete",
        ),
    ] {
        assert!(
            path.starts_with("tests/invariants/") && path.ends_with(".rs") && !path.contains(".."),
            "INV-001 market-incarnation witness must resolve to an invariant source file: {path}"
        );
        assert!(
            witness.starts_with("v16_")
                || witness == "retained_transaction_binds_program_market_kind_schema_and_blockhash"
                || witness == "deployed_wrapper_has_no_detached_signature_interpreter",
            "INV-001 market-incarnation witness must be a reviewed regression: {path}#{witness}"
        );
        assert!(
            composition_witnesses.insert((path, witness)),
            "duplicate INV-001 market-incarnation witness {path}#{witness}",
        );
        assert!(
            inv001_source_defines_test(source, witness),
            "INV-001 lost market-incarnation composition witness {path}#{witness}",
        );
    }
    assert_eq!(
        composition_witnesses.len(),
        6,
        "INV-001 market-incarnation witness roster drift"
    );
}
