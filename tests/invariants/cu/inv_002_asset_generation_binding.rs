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

use super::*;

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
    let source = include_str!("../../../src/v16_program.rs");
    let instruction_enum =
        source_between(source, "pub enum Instruction {", "\n    impl Instruction {");

    // Fifteen direct instruction fields plus the two batch-leg fields are the currently encoded
    // asset-generation surface. Any roster change requires an INV-002 classification.
    assert_eq!(instruction_enum.matches("market_id: u64").count(), 15);
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

    // These are the exact remaining signed asset-slot families without a generation field. Keep
    // this list explicit until their public replay matrices and wire migrations close the gap.
    for variant in ["UpdateAssetAuthority", "UpdateAssetLifecycle"] {
        assert!(
            !variant_body(instruction_enum, variant).contains("market_id: u64"),
            "{variant} gained a binding; update INV-002 evidence and remove it from the gap roster"
        );
    }
}

#[test]
fn v16_attack_signed_trade_cannot_replay_across_asset_slot_reuse() {
    assert_signed_trade_cannot_replay_across_asset_slot_reuse();
}
