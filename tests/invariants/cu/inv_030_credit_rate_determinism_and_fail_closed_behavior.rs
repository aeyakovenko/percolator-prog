//! INV-030 - Credit-rate determinism and fail-closed behavior.
//!
//! Normative obligation: source-credit rates are a deterministic function of
//! current claim bounds and independently available backing. Expiry, omission,
//! or impairment cannot make the persisted rate more favorable, and must not
//! delete the underlying claim or move custody.
//!
//! Evidence in this file (I/F): this deterministic LiteSVM wrapper test runs
//! the shared public-route lifecycle oracle with fixed parameters. It creates a
//! discounted positive claim, adds fresh backing, crosses the exact expiry slot,
//! checks a public owner risk-reduction path with zero credit, and refills the
//! bucket. After each route, an independent u128 oracle recomputes the rate.
//! The generated stateful runner also applies a transition-cause oracle to every public action and
//! successful crank: formula-input changes advance the source epoch, unchanged inputs preserve the
//! rate, and a live claim's rate cannot rise without more independently available backing or a
//! smaller claim bound.
//! A separate malformed-state matrix retains an otherwise-valid backing mutation, corrupts each
//! persisted source-credit relation on both source sides, and requires an instruction error with
//! exact market, backing-ledger, token, and lamport rollback. This deliberately malformed fixture
//! is validation evidence, not public exploit reachability.
//! Finally, a production-source lock permits only the four host codec assignments that copy the
//! embedded source records to and from their typed mirrors. On-chain rate and epoch mutation stays
//! exclusively inside the pinned engine; INV-088 separately inventories every wrapper-to-engine
//! transition callsite, so this wrapper suite does not duplicate the engine's arithmetic proof.
//!
//! Guarantee boundary: this is one non-random whole-route witness for the same
//! invariant enforced by the stateful generator. Full-width arithmetic remains
//! covered by engine/Kani and INV-085 arithmetic-differential tests.

use crate::support::v16_svm::{MarketConfig, V16Svm};
use percolator::{SourceCreditStateV16, BOUND_SCALE, CREDIT_RATE_SCALE};
use percolator_prog::{constants, state};
use solana_sdk::{account::Account, pubkey::Pubkey};

#[derive(Clone, Debug, PartialEq, Eq)]
struct MalformedSourceSnapshot {
    market: Vec<u8>,
    backing_ledger: Vec<u8>,
    token_accounts: Vec<(Pubkey, Vec<u8>)>,
    economic_lamports: Vec<(Pubkey, u64)>,
}

fn malformed_source_snapshot(env: &V16Svm) -> MalformedSourceSnapshot {
    MalformedSourceSnapshot {
        market: env.market_data(false),
        backing_ledger: env.backing_domain_ledger_data(),
        token_accounts: env.all_token_account_data(),
        economic_lamports: env.all_economic_account_lamports(),
    }
}

fn source_credit_offset(domain: usize, field_offset: usize) -> usize {
    let source_offset = if domain % 2 == 0 {
        core::mem::offset_of!(percolator::EngineAssetSlotV16Account, source_credit_long)
    } else {
        core::mem::offset_of!(percolator::EngineAssetSlotV16Account, source_credit_short)
    };
    constants::MARKET_GROUP_OFF
        + percolator::MarketGroupV16HeaderAccount::dynamic_asset_slot_offset::<
            state::AssetOracleStorageV16,
        >(domain / 2)
        .expect("source asset slot offset")
        + core::mem::offset_of!(percolator::Market<state::AssetOracleStorageV16>, engine)
        + source_offset
        + field_offset
}

fn assert_malformed_source_rejects(
    env: &mut V16Svm,
    canonical_market: &Account,
    domain: usize,
    label: &str,
    mutate: impl FnOnce(&mut Account),
) {
    env.svm
        .set_account(env.market, canonical_market.clone())
        .expect("restore canonical market");
    let expiry_slot = env.current_slot() + 10;
    let retained = env.build_retained_backing_bucket_top_up(domain as u16, 1, expiry_slot);
    let mut market = canonical_market.clone();
    mutate(&mut market);
    env.svm
        .set_account(env.market, market)
        .expect("install malformed source-credit fixture");
    let before = malformed_source_snapshot(env);
    let error = match env.land_retained(retained) {
        Ok(_) => panic!("domain {domain} malformed {label} must reject"),
        Err(error) => error,
    };
    assert!(
        !error.is_empty(),
        "domain {domain} malformed {label} must expose an instruction error"
    );
    assert_eq!(
        malformed_source_snapshot(env),
        before,
        "domain {domain} malformed {label} must roll back exactly"
    );
}

#[derive(Clone, Copy)]
struct MalformedSourceCase {
    name: &'static str,
    field_offset: usize,
    value: u128,
}

fn malformed_source_cases() -> [MalformedSourceCase; 10] {
    macro_rules! source_case {
        ($name:literal, $field:ident, $value:expr) => {
            MalformedSourceCase {
                name: $name,
                field_offset: core::mem::offset_of!(
                    percolator::SourceCreditStateV16Account,
                    $field
                ),
                value: $value,
            }
        };
    }
    [
        source_case!("unrated positive claim", positive_claim_bound_num, 1),
        source_case!("exact claim above bound", exact_positive_claim_num, 1),
        source_case!(
            "source/bucket fresh backing mismatch",
            fresh_reserved_backing_num,
            BOUND_SCALE
        ),
        source_case!("receivable above spent backing", provider_receivable_num, 1),
        source_case!(
            "counterparty lien above backing",
            valid_liened_backing_num,
            1
        ),
        source_case!(
            "impaired counterparty ledger mismatch",
            impaired_liened_backing_num,
            1
        ),
        source_case!(
            "unaligned insurance reservation",
            insurance_credit_reserved_num,
            1
        ),
        source_case!(
            "valid insurance lien above reservation",
            valid_liened_insurance_num,
            BOUND_SCALE
        ),
        source_case!(
            "impaired insurance lien above reservation",
            impaired_liened_insurance_num,
            BOUND_SCALE
        ),
        source_case!("persisted rate/formula mismatch", credit_rate_num, 0),
    ]
}

#[test]
fn v16_program_source_credit_rate_lifecycle_matches_independent_oracle_fixed_case() {
    crate::support::fuzz_model::verify_source_credit_rate_lifecycle([0x30; 32], 17, 29, 11)
        .expect("source-credit rate public lifecycle oracle");
}

#[test]
fn v16_wrapper_has_no_independent_source_credit_rate_mutation_path() {
    let source = include_str!("../../../src/v16_program.rs");
    let production = source
        .split("    #[cfg(test)]\n    mod tests")
        .next()
        .expect("production source prefix");
    let assignments: Vec<_> = production
        .lines()
        .filter(|line| line.contains("source_credit") && line.contains(" = "))
        .map(str::trim)
        .collect();
    assert_eq!(
        assignments,
        [
            "group.source_credit[long_domain] = slot",
            "group.source_credit[short_domain] = slot",
            "slot.source_credit_long = percolator::SourceCreditStateV16Account::from_runtime(",
            "slot.source_credit_short = percolator::SourceCreditStateV16Account::from_runtime(",
        ],
        "only the non-BPF typed host codec may copy complete source-credit records; a new wrapper \
         mutation path requires INV-030 route coverage",
    );
    for forbidden in [".credit_rate_num =", ".credit_epoch ="] {
        assert!(
            !production.contains(forbidden),
            "wrapper must not independently mutate engine-owned {forbidden} state",
        );
    }

    crate::assert_certified_engine_pin("INV-030 engine-owned source-credit mutation evidence");
}

#[test]
fn v16_program_malformed_embedded_source_credit_fails_closed_on_both_sides() {
    for domain in 0..2u16 {
        let mut control = V16Svm::new([0x31 + domain as u8; 32], MarketConfig::default());
        let before = malformed_source_snapshot(&control);
        let expiry_slot = control.current_slot() + 10;
        control
            .top_up_backing_bucket(domain, 1, expiry_slot)
            .unwrap_or_else(|error| panic!("domain {domain} control must land: {error}"));
        assert_ne!(
            malformed_source_snapshot(&control),
            before,
            "domain {domain} control must mutate persistent state"
        );
    }

    let mut env = V16Svm::new([0x32; 32], MarketConfig::default());
    let (_, canonical_group) = env.primary_market_state();
    for domain in 0..2 {
        assert_eq!(
            canonical_group.source_credit[domain],
            SourceCreditStateV16 {
                credit_rate_num: CREDIT_RATE_SCALE,
                ..SourceCreditStateV16::EMPTY
            }
        );
    }
    let canonical_market: Account = env.svm.get_account(&env.market).expect("canonical market");

    for domain in 0..2usize {
        for case in malformed_source_cases() {
            assert_malformed_source_rejects(
                &mut env,
                &canonical_market,
                domain,
                case.name,
                |market| {
                    let offset = source_credit_offset(domain, case.field_offset);
                    market.data[offset..offset + 16].copy_from_slice(&case.value.to_le_bytes());
                },
            );
        }

        assert_malformed_source_rejects(
            &mut env,
            &canonical_market,
            domain,
            "omitted source-credit state",
            |market| market.data.truncate(source_credit_offset(domain, 0)),
        );
    }
}
