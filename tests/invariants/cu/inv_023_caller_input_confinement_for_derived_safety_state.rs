//! INV-023 - caller-input confinement for derived safety state.
//!
//! `CrankObservationHint` is discovery input: it tells the wrapper which oracle
//! accounts the caller supplied. It must not become an authority to partially
//! mutate market time, oracle checkpoints, funding, or account state when a later
//! caller-controlled hint proves malformed. These tests intentionally place a valid
//! observation before a bad one so the only correct outcome is full instruction
//! failure and exact SVM rollback. A production-source-bound roster also assigns
//! every field in all 50 public instructions and their nested batch/hint structs to
//! one semantic trust class and one executable invariant witness.

use super::*;

fn assert_late_bad_crank_hint_rolls_back(label: &str, bad_tail: CrankObservationHint) {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.svm.warp_to_slot(5);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let slot_before = env.market_state().1.assets[0].slot_last;

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 5,
            observations: vec![
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                },
                bad_tail,
            ],
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "{label}: malformed late hint must reject"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "{label}: failed crank rolls back market bytes and lamports",
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "{label}: failed crank rolls back portfolio bytes and lamports",
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "{label}: failed crank cannot move custody",
    );

    env.svm.expire_blockhash();
    let valid_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 5,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[],
        )
        .expect("same public route succeeds once the hostile hint is removed");
    assert_cu_within(label, valid_cu, CRANK_CU_LIMIT);
    assert!(
        env.market_state().1.assets[0].slot_last > slot_before,
        "{label}: non-vacuous control proves the first hint would have advanced state",
    );
}

#[test]
fn v16_program_duplicate_crank_hint_after_valid_hint_rolls_back_partial_state() {
    assert_late_bad_crank_hint_rolls_back(
        "INV-023 duplicate crank hint",
        CrankObservationHint {
            asset_index: 0,
            oracle_accounts: 0,
        },
    );
}

#[test]
fn v16_program_out_of_range_crank_hint_after_valid_hint_rolls_back_partial_state() {
    assert_late_bad_crank_hint_rolls_back(
        "INV-023 out-of-range crank hint",
        CrankObservationHint {
            asset_index: 1,
            oracle_accounts: 0,
        },
    );
}

fn inv023_is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn inv023_instruction_fields(source: &str) -> std::collections::BTreeSet<(String, String)> {
    let marker = "pub enum Instruction {";
    let start = source
        .find(marker)
        .expect("production public Instruction enum")
        + marker.len();
    let tail = &source[start..];
    let end = tail
        .find("\n    impl Instruction {")
        .expect("Instruction implementation after enum");
    let mut fields = std::collections::BTreeSet::new();
    let mut current_variant: Option<String> = None;

    for line in tail[..end].lines() {
        let line = line.trim();
        if let Some(variant) = current_variant.as_ref() {
            if line == "}," {
                current_variant = None;
                continue;
            }
            if let Some((field, _)) = line.split_once(':') {
                if inv023_is_identifier(field) {
                    assert!(
                        fields.insert((variant.clone(), field.to_owned())),
                        "duplicate production instruction field {variant}.{field}",
                    );
                }
            }
            continue;
        }

        if let Some(variant) = line.strip_suffix(" {") {
            if inv023_is_identifier(variant) {
                current_variant = Some(variant.to_owned());
            }
        } else if let Some(variant) = line.strip_suffix(',') {
            if inv023_is_identifier(variant)
                && variant
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_uppercase)
            {
                assert!(
                    fields.insert((variant.to_owned(), "-".to_owned())),
                    "duplicate unit instruction variant {variant}",
                );
            }
        }
    }
    fields
}

fn inv023_struct_fields(
    source: &str,
    type_name: &str,
) -> std::collections::BTreeSet<(String, String)> {
    let marker = format!("pub struct {type_name} {{");
    let start = source
        .find(&marker)
        .unwrap_or_else(|| panic!("production public input struct {type_name}"))
        + marker.len();
    let tail = &source[start..];
    let end = tail
        .find("\n    }")
        .unwrap_or_else(|| panic!("closing brace for public input struct {type_name}"));
    let mut fields = std::collections::BTreeSet::new();
    for line in tail[..end].lines() {
        let line = line.trim();
        let Some(line) = line.strip_prefix("pub ") else {
            continue;
        };
        let Some((field, _)) = line.split_once(':') else {
            continue;
        };
        if inv023_is_identifier(field) {
            assert!(
                fields.insert((type_name.to_owned(), field.to_owned())),
                "duplicate production nested input field {type_name}.{field}",
            );
        }
    }
    fields
}

#[test]
fn v16_program_caller_input_roster_owns_every_production_field() {
    const PRODUCTION: &str = include_str!("../../../src/v16_program.rs");
    const ROSTER: &str = include_str!("../inv_023_caller_input_roster.tsv");
    const HEADER: &str = "type\tfields\tclassification\tevidence";
    const ALLOWED_CLASSES: [&str; 9] = [
        "SIGNED_CONFIG",
        "SIGNED_ECONOMIC",
        "IDENTITY_BINDING",
        "SCOPE_SELECTOR",
        "AUTHENTICATED_TIME",
        "DISCOVERY_HINT",
        "REPLAY_GUARD",
        "BOUNDED_WORK",
        "IGNORED_LEGACY",
    ];

    let mut production_fields = inv023_instruction_fields(PRODUCTION);
    for type_name in ["BatchTradeLeg", "BatchTradeCpiLeg", "CrankObservationHint"] {
        production_fields.extend(inv023_struct_fields(PRODUCTION, type_name));
    }

    let mut roster_fields = std::collections::BTreeSet::new();
    let mut covered_classes = std::collections::BTreeSet::new();
    let mut evidence_sources = std::collections::BTreeMap::<String, String>::new();
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut saw_header = false;
    for (line_number, line) in ROSTER.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !saw_header {
            assert_eq!(line, HEADER, "INV-023 roster header changed");
            saw_header = true;
            continue;
        }
        let columns: Vec<_> = line.split('\t').collect();
        assert_eq!(
            columns.len(),
            4,
            "malformed INV-023 roster row {}: {line}",
            line_number + 1,
        );
        let type_name = columns[0];
        let classification = columns[2];
        assert!(
            ALLOWED_CLASSES.contains(&classification) || classification == "NO_CALLER_DATA",
            "unknown INV-023 classification {classification}",
        );
        covered_classes.insert(classification);

        let (evidence_path, evidence_test) = columns[3]
            .split_once('#')
            .unwrap_or_else(|| panic!("evidence must be path#test on row {}", line_number + 1));
        let evidence_source = evidence_sources
            .entry(evidence_path.to_owned())
            .or_insert_with(|| {
                std::fs::read_to_string(manifest.join(evidence_path)).unwrap_or_else(|error| {
                    panic!("read INV-023 evidence {evidence_path}: {error}")
                })
            });
        assert!(
            evidence_source.contains(&format!("fn {evidence_test}(")),
            "INV-023 evidence function {evidence_path}#{evidence_test} is missing",
        );

        for field in columns[1].split(',') {
            assert!(inv023_is_identifier(field) || field == "-");
            let key = (type_name.to_owned(), field.to_owned());
            assert!(
                roster_fields.insert(key.clone()),
                "duplicate INV-023 roster field {}.{}",
                key.0,
                key.1,
            );

            if field == "-" {
                assert_eq!(classification, "NO_CALLER_DATA");
            }
            if matches!(field, "now_slot" | "now_unix_ts") {
                assert_eq!(classification, "AUTHENTICATED_TIME");
            }
            if field.ends_with("sequence") {
                assert_eq!(classification, "REPLAY_GUARD");
            }
            if matches!(field, "close_q" | "b_delta_budget" | "reduce_q") {
                assert_eq!(classification, "BOUNDED_WORK");
            }
            if field == "fee_rate_per_slot" {
                assert_eq!(classification, "IGNORED_LEGACY");
            }
            if type_name == "CrankObservationHint"
                || (type_name == "PermissionlessCrank" && field == "observations")
            {
                assert_eq!(classification, "DISCOVERY_HINT");
            }
        }
    }
    assert!(saw_header, "INV-023 roster header is missing");
    assert_eq!(
        roster_fields, production_fields,
        "INV-023 roster must classify every production caller-input field exactly once; missing={:?}; stale={:?}",
        production_fields
            .difference(&roster_fields)
            .collect::<Vec<_>>(),
        roster_fields
            .difference(&production_fields)
            .collect::<Vec<_>>(),
    );
    for classification in ALLOWED_CLASSES {
        assert!(
            covered_classes.contains(classification),
            "INV-023 trust class {classification} has no production field",
        );
    }
    assert!(covered_classes.contains("NO_CALLER_DATA"));

    let instruction_types = production_fields
        .iter()
        .map(|(type_name, _)| type_name)
        .filter(|type_name| {
            !matches!(
                type_name.as_str(),
                "BatchTradeLeg" | "BatchTradeCpiLeg" | "CrankObservationHint"
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        instruction_types.len(),
        50,
        "roster must remain bound to every public instruction variant",
    );
}
