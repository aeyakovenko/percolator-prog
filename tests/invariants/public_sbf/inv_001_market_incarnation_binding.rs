//! INV-001 - Market incarnation binding.
//!
//! The deployed wrapper enforces a stricter policy than reusable generation IDs: once
//! `CloseSlab` succeeds, that market pubkey is permanently retired behind a typed,
//! rent-exempt tombstone. This representative public route proves same-address
//! `InitMarket` and a retained terminal control both reject with exact rollback.
//! INV-007 owns the complete retained-operation matrix and fresh-address liveness.

use crate::support::invariant_discovery::{discover_market_incarnation_replay, MarketIntentKind};

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
    assert!(own_source.contains("fn v16_program_closed_market_incarnation_cannot_be_recreated("));

    let generated_matrix = include_str!("../stateful/inv_001_market_incarnation_binding.rs");
    assert!(generated_matrix
        .contains("fn v16_program_market_incarnation_operation_matrix_rejects_address_reuse("));

    let account_census = include_str!("inv_007_no_aba_reuse.rs");
    assert!(
        account_census.contains("fn v16_wrapper_account_incarnation_census_is_source_complete(")
    );

    let transaction_domain =
        include_str!("inv_006_program_chain_message_type_and_version_binding.rs");
    for witness in [
        "fn retained_transaction_binds_program_market_kind_schema_and_blockhash(",
        "fn deployed_wrapper_has_no_detached_signature_interpreter(",
    ] {
        assert!(
            transaction_domain.contains(witness),
            "INV-001 lost transaction-domain composition witness {witness}",
        );
    }

    let ordering = include_str!("../cu/inv_010_out_of_order_safety.rs");
    assert!(
        ordering.contains("fn v16_program_out_of_order_induction_composition_is_source_complete(")
    );
}
