//! INV-008 - Intent uniqueness and bounded replay.
//!
//! Normative obligation: each retained top-up carries a per-asset monotonic intent ID. The two
//! insurance entrypoints share one lane, backing uses a separate lane, and a successful mutation
//! consumes its lane only after all wrapper and engine validation. Public-SBF and stateful owners
//! prove stale-retry rollback and fresh-intent liveness; this file pins source composition and the
//! no-growth zero-copy layout used by those routes.

use super::*;

fn braced_block_after<'a>(source: &'a str, marker: &str) -> &'a str {
    let start = source
        .find(marker)
        .unwrap_or_else(|| panic!("missing source marker {marker}"));
    let open = start
        + source[start..]
            .find('{')
            .unwrap_or_else(|| panic!("missing opening brace after {marker}"));
    let mut depth = 0i32;
    for (offset, ch) in source[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[(open + 1)..(open + offset)];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated source block after {marker}");
}

fn assert_ordered(body: &str, markers: &[&str]) {
    let mut cursor = 0usize;
    for marker in markers {
        let offset = body[cursor..]
            .find(marker)
            .unwrap_or_else(|| panic!("missing ordered marker {marker}"));
        cursor += offset + marker.len();
    }
}

#[test]
fn v16_top_up_intent_wire_and_dispatch_roster_is_complete() {
    let source = include_str!("../../../src/v16_program.rs");
    let instruction = braced_block_after(source, "pub enum Instruction");
    for variant in [
        "TopUpInsurance",
        "TopUpInsuranceDomain",
        "TopUpBackingBucket",
    ] {
        let body = braced_block_after(instruction, variant);
        assert!(
            body.contains("intent_id: u64"),
            "{variant} must carry the monotonic top-up intent"
        );
    }

    let decode = braced_block_after(source, "pub fn decode(input: &[u8])");
    let encode = braced_block_after(source, "pub fn encode(&self)");
    let process = braced_block_after(source, "pub fn process_instruction");
    assert_eq!(decode.matches("intent_id: read_u64(&mut rest)?").count(), 3);
    assert_eq!(encode.matches("push_u64(&mut out, intent_id)").count(), 3);
    assert_eq!(process.matches("intent_id,").count(), 6);
}

#[test]
fn v16_top_up_intent_guards_precede_mutation_and_consumption_is_last() {
    let source = include_str!("../../../src/v16_program.rs");
    let market = braced_block_after(source, "fn handle_top_up_insurance<'a>");
    let domain = braced_block_after(source, "fn handle_top_up_insurance_domain<'a>");
    let backing = braced_block_after(source, "fn handle_top_up_backing_bucket<'a>");

    assert_ordered(
        market,
        &[
            "require_newer_control_sequence(sequences.insurance_top_up, intent_id)",
            "deposit_market_zero_insurance_view",
            "group.validate_shape()",
            "ControlSequenceLane::InsuranceTopUp",
            "transfer_tokens",
        ],
    );
    assert_ordered(
        domain,
        &[
            "require_newer_control_sequence(sequences.insurance_top_up, intent_id)",
            "deposit_domain_insurance_not_atomic",
            "group.validate_shape()",
            "ControlSequenceLane::InsuranceTopUp",
            "transfer_tokens",
        ],
    );
    assert_ordered(
        backing,
        &[
            "require_newer_control_sequence(sequences.backing_top_up, intent_id)",
            "deposit_fresh_counterparty_backing_not_atomic",
            "group.validate_shape()",
            "ControlSequenceLane::BackingTopUp",
            "transfer_tokens",
        ],
    );
    assert_eq!(
        market
            .matches("ControlSequenceLane::InsuranceTopUp")
            .count(),
        1
    );
    assert_eq!(
        domain
            .matches("ControlSequenceLane::InsuranceTopUp")
            .count(),
        1
    );
    assert_eq!(
        backing.matches("ControlSequenceLane::BackingTopUp").count(),
        2
    );
}

#[test]
fn v16_top_up_sequences_reuse_the_existing_zero_copy_tail_without_growth() {
    assert_eq!(
        core::mem::size_of::<state::AssetControlSequencesV16>(),
        percolator_prog::constants::ASSET_CONTROL_SEQUENCES_LEN
    );
    assert_eq!(percolator_prog::constants::ASSET_CONTROL_SEQUENCES_LEN, 88);

    let sequences = state::AssetControlSequencesV16 {
        permissionless_resolve: 9,
        insurance_top_up: 10,
        backing_top_up: 11,
        ..state::AssetControlSequencesV16::default()
    };
    assert_eq!(sequences.permissionless_resolve, 9);
    assert_eq!(sequences.insurance_top_up, 10);
    assert_eq!(sequences.backing_top_up, 11);
    state::validate_asset_control_sequences(&sequences).expect("all u64 watermarks are canonical");
}
