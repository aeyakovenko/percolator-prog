//! INV-008 - Intent uniqueness and bounded replay.
//!
//! Normative obligation: each retained top-up carries a per-asset monotonic intent ID. The two
//! insurance entrypoints share one lane, backing uses a separate lane, and a successful mutation
//! consumes its lane only after all wrapper and engine validation. Public-SBF and stateful owners
//! prove stale-retry rollback and fresh-intent liveness; this file pins source composition and the
//! no-growth zero-copy layout used by those routes. The replay-disposition roster also classifies
//! every public instruction and requires all retry/supersession generator kinds to retain a
//! production route, so adding a public variant cannot silently bypass an explicit replay owner.

use super::*;
use crate::support::invariant_discovery::{RetryIntentKind, SupersededIntentKind};
use std::collections::{BTreeMap, BTreeSet};

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

fn instruction_variants(source: &str) -> BTreeSet<String> {
    braced_block_after(source, "pub enum Instruction")
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let first = line.as_bytes().first().copied()?;
            if !first.is_ascii_uppercase() {
                return None;
            }
            line.split(|character| character == '{' || character == ',')
                .next()
                .map(str::trim)
                .filter(|variant| !variant.is_empty())
                .map(str::to_owned)
        })
        .collect()
}

fn debug_names<T: core::fmt::Debug>(values: &[T]) -> BTreeSet<String> {
    values.iter().map(|value| format!("{value:?}")).collect()
}

#[test]
fn v16_public_replay_disposition_roster_is_source_complete() {
    let source = include_str!("../../../src/v16_program.rs");
    let source_variants = instruction_variants(source);
    assert_eq!(
        source_variants.len(),
        50,
        "production instruction roster changed"
    );

    let mut public_registry = BTreeMap::new();
    for line in include_str!("../public_instruction_coverage.tsv").lines() {
        if line.starts_with('#') || line.is_empty() || line.starts_with("tag\t") {
            continue;
        }
        let fields: Vec<&str> = line.splitn(5, '\t').collect();
        assert_eq!(fields.len(), 5, "malformed public instruction row: {line}");
        let tag: u8 = fields[0].parse().expect("numeric instruction tag");
        assert!(
            public_registry.insert(tag, fields[1]).is_none(),
            "duplicate public instruction tag {tag}"
        );
    }

    let mut dispositions = BTreeMap::new();
    let mut retry_variants = BTreeSet::new();
    let mut retry_kinds = BTreeSet::new();
    let mut supersession_variants = BTreeSet::new();
    let mut supersession_kinds = BTreeSet::new();
    for line in include_str!("../inv_008_replay_disposition.tsv").lines() {
        if line.starts_with('#') || line.is_empty() || line.starts_with("tag\t") {
            continue;
        }
        let fields: Vec<&str> = line.splitn(4, '\t').collect();
        assert_eq!(fields.len(), 4, "malformed replay disposition row: {line}");
        let tag: u8 = fields[0].parse().expect("numeric replay disposition tag");
        let variant = fields[1];
        let disposition = fields[2];
        assert!(!fields[3].trim().is_empty(), "empty boundary for {variant}");
        assert_eq!(
            public_registry.get(&tag),
            Some(&variant),
            "replay disposition must bind the production tag and variant"
        );
        assert!(
            dispositions.insert(variant, disposition).is_none(),
            "duplicate replay disposition for {variant}"
        );

        if let Some(kinds) = disposition.strip_prefix("retry:") {
            retry_variants.insert(variant);
            retry_kinds.extend(kinds.split(',').map(str::to_owned));
        } else if let Some(kinds) = disposition.strip_prefix("supersession:") {
            supersession_variants.insert(variant);
            supersession_kinds.extend(kinds.split(',').map(str::to_owned));
        } else {
            assert!(
                matches!(
                    disposition,
                    "init-once"
                        | "incarnation-episode"
                        | "state-derived"
                        | "live-authority"
                        | "balance-bounded"
                ),
                "unknown replay disposition {disposition} for {variant}"
            );
        }
    }

    assert_eq!(
        dispositions.keys().copied().collect::<BTreeSet<_>>(),
        source_variants.iter().map(String::as_str).collect(),
        "every production instruction needs an explicit replay disposition"
    );
    assert_eq!(
        retry_kinds,
        debug_names(&RetryIntentKind::ALL),
        "every executable retry generator kind needs a production route and vice versa"
    );
    assert_eq!(
        supersession_kinds,
        debug_names(&SupersededIntentKind::ALL),
        "every executable supersession generator kind needs a production route and vice versa"
    );
    assert_eq!(
        retry_variants,
        [
            "BatchTradeCpi",
            "BatchTradeNoCpi",
            "ConvertReleasedPnl",
            "Deposit",
            "RebalanceReduce",
            "TopUpBackingBucket",
            "TopUpInsurance",
            "TopUpInsuranceDomain",
            "TradeCpi",
            "TradeNoCpi",
            "UpdateAssetLifecycle",
            "Withdraw",
        ]
        .into_iter()
        .collect(),
        "new retryable economic routes must enter the executable INV-008 matrix"
    );
    assert_eq!(
        supersession_variants,
        [
            "ConfigureAuthMark",
            "ConfigureEwmaMark",
            "ConfigureHybridOracle",
            "ConfigurePermissionlessResolve",
            "PushAuthMark",
            "PushEwmaMark",
            "SetMatcherConfig",
            "UpdateBackingFeePolicy",
            "UpdateFeeRedirectPolicy",
            "UpdateLiquidationFeePolicy",
            "UpdateMaintenanceFeePolicy",
            "UpdateMarketInitFeePolicy",
            "UpdateTradeFeePolicy",
        ]
        .into_iter()
        .collect(),
        "new delayed controls must enter the executable supersession matrix"
    );
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
