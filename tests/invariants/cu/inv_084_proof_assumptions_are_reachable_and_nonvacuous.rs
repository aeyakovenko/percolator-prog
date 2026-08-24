//! INV-084 - Proof assumptions are reachable and nonvacuous.
//!
//! Normative obligation: every explicit proof assumption is inventoried from
//! the actual mounted Kani modules, has a machine-checked two-sided witness, and
//! is established by a public route or a named solver-bound discharge.
//!
//! Evidence in this file (I/P roster): the source audit discovers mounted Kani
//! modules from `kani/v16_kani.rs`, extracts every `kani::assume` tuple, and
//! compares it exactly with the manifest. It also validates the Kani witness and
//! public-route evidence named by every row. A public LiteSVM composition reaches
//! matcher cap/toggle, nonzero incarnation, sequence, episode, and nonzero trade
//! domains while excluded toggles, caps, and zero-size trades roll back exactly.

use super::*;

const INV_084_KANI_ROOT: &str = include_str!("../../../kani/v16_kani.rs");
const INV_084_ASSUME_INVENTORY: &str = include_str!("../kani_assumption_inventory.tsv");
const INV_084_INVENTORY_HEADER: &str = "file\tline\towner_invariant\towning_proof\tassumption_predicate\tproof_witness\tclassification\tpublic_evidence";

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Inv084Assumption {
    file: String,
    line: usize,
    owner_invariant: String,
    owning_proof: String,
    predicate: String,
}

fn inv_084_mounted_kani_files() -> std::collections::BTreeSet<String> {
    let mut files = std::collections::BTreeSet::new();
    for line in INV_084_KANI_ROOT.lines() {
        let line = line.trim();
        let Some(path) = line
            .strip_prefix("#[path = \"")
            .and_then(|rest| rest.strip_suffix("\"]"))
        else {
            continue;
        };
        let Some(path) = path.strip_prefix("../tests/invariants/kani/") else {
            continue;
        };
        assert!(
            files.insert(format!("tests/invariants/kani/{path}")),
            "duplicate mounted Kani module {path}"
        );
    }
    files
}

fn inv_084_source_assumptions(
    file: &str,
    source: &str,
) -> std::collections::BTreeSet<Inv084Assumption> {
    let owner_invariant = source
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("//! INV-")
                .and_then(|rest| rest.split_once(' '))
                .map(|(digits, _)| format!("INV-{digits}"))
        })
        .unwrap_or_else(|| panic!("{file} lacks an invariant ownership header"));
    let mut owning_proof = "<module>".to_string();
    let mut assumptions = std::collections::BTreeSet::new();

    for (line_index, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed
            .strip_prefix("fn ")
            .or_else(|| trimmed.strip_prefix("pub fn "))
        {
            let end = rest
                .find(|ch: char| !(ch == '_' || ch.is_ascii_alphanumeric()))
                .unwrap_or(rest.len());
            owning_proof = rest[..end].to_owned();
        }

        let Some(start) = line.find("kani::assume(") else {
            continue;
        };
        let predicate_start = start + "kani::assume(".len();
        let rest = &line[predicate_start..];
        let predicate_end = rest.find(");").unwrap_or_else(|| {
            panic!(
                "{file}:{} assumption must remain on one auditable line",
                line_index + 1
            )
        });
        assert!(
            !rest[predicate_end + 2..].contains("kani::assume("),
            "{file}:{} has multiple assumptions on one line",
            line_index + 1
        );
        let assumption = Inv084Assumption {
            file: file.to_owned(),
            line: line_index + 1,
            owner_invariant: owner_invariant.clone(),
            owning_proof: owning_proof.clone(),
            predicate: rest[..predicate_end].trim().to_owned(),
        };
        assert!(
            assumptions.insert(assumption),
            "duplicate assumption at {file}:{}",
            line_index + 1
        );
    }
    assumptions
}

#[test]
fn v16_program_every_mounted_explicit_kani_assumption_is_exactly_inventoried() {
    use std::collections::{BTreeMap, BTreeSet};

    const EXPECTED_MOUNTED_MODULES: usize = 19;
    const EXPECTED_ASSUMPTIONS: usize = 13;
    const ALLOWED_CLASSIFICATIONS: [&str; 3] = [
        "NONVACUITY_WITNESS",
        "ROUTE_ESTABLISHED",
        "SOLVER_BOUND_RATIONALE",
    ];

    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mounted_files = inv_084_mounted_kani_files();
    assert_eq!(
        mounted_files.len(),
        EXPECTED_MOUNTED_MODULES,
        "mounted Kani module count changed; inventory every module deliberately"
    );

    let mut actual = BTreeSet::new();
    let mut source_cache = BTreeMap::<String, String>::new();
    for file in &mounted_files {
        let source = std::fs::read_to_string(manifest.join(file))
            .unwrap_or_else(|error| panic!("read mounted Kani module {file}: {error}"));
        actual.extend(inv_084_source_assumptions(file, &source));
        source_cache.insert(file.clone(), source);
    }
    assert_eq!(
        actual.len(),
        EXPECTED_ASSUMPTIONS,
        "explicit Kani assumption count changed; inventory every predicate deliberately"
    );

    let mut expected = BTreeSet::new();
    let mut lines = INV_084_ASSUME_INVENTORY.lines();
    assert_eq!(lines.next(), Some(INV_084_INVENTORY_HEADER));
    for (row_index, row) in lines.enumerate() {
        if row.trim().is_empty() || row.starts_with('#') {
            continue;
        }
        let columns = row.split('\t').collect::<Vec<_>>();
        assert_eq!(
            columns.len(),
            8,
            "inventory row {} must have eight columns",
            row_index + 2
        );
        assert!(mounted_files.contains(columns[0]));
        assert!(ALLOWED_CLASSIFICATIONS.contains(&columns[6]));
        let assumption = Inv084Assumption {
            file: columns[0].to_owned(),
            line: columns[1]
                .parse()
                .unwrap_or_else(|_| panic!("invalid line on inventory row {}", row_index + 2)),
            owner_invariant: columns[2].to_owned(),
            owning_proof: columns[3].to_owned(),
            predicate: columns[4].to_owned(),
        };
        assert!(expected.insert(assumption), "duplicate inventory row {row}");

        let owner_source = source_cache.get(columns[0]).unwrap();
        assert!(
            owner_source.contains(&format!("fn {}(", columns[3])),
            "owning proof {} is absent from {}",
            columns[3],
            columns[0]
        );

        let witness_source = source_cache
            .get("tests/invariants/kani/inv_084_proof_assumptions_are_reachable_and_nonvacuous.rs")
            .unwrap();
        assert!(
            witness_source.contains(&format!("fn {}(", columns[5])),
            "proof witness {} is absent",
            columns[5]
        );

        let (evidence_file, evidence_function) = columns[7]
            .split_once('#')
            .expect("public evidence must be path#function");
        let evidence_source = source_cache
            .entry(evidence_file.to_owned())
            .or_insert_with(|| {
                std::fs::read_to_string(manifest.join(evidence_file))
                    .unwrap_or_else(|error| panic!("read public evidence {evidence_file}: {error}"))
            });
        assert!(
            evidence_source.contains(&format!("fn {evidence_function}(")),
            "public evidence {} is absent",
            columns[7]
        );
    }

    assert_eq!(
        actual, expected,
        "assumption inventory differs from the actual mounted Kani sources"
    );
}

#[test]
fn v16_program_explicit_kani_guard_domains_are_publicly_reachable_and_fail_closed() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF");
    env.svm.add_program(matcher_program, &matcher_bytes);

    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    assert!(env.portfolio_id(taker) != 0);
    assert!(env.portfolio_id(lp) != 0);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&lp_owner, lp, 1_000_000);
    assert!(env.portfolio_matcher_sequence(lp) < u64::MAX);

    let (context, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp_owner, lp);
    let configured = env.portfolio_matcher_config(lp);
    assert_eq!(configured.enabled(), 1);
    assert_eq!(configured.trade_fee_cap_bps(), 10_000);
    assert!(configured.position_epoch() < state::PortfolioMatcherConfigV16::position_epoch_max());

    for (label, enabled, fee_cap) in [
        ("invalid toggle", 2, 10_000),
        ("invalid fee cap", 1, 10_001),
    ] {
        let market_before = env.svm.get_account(&env.market).unwrap();
        let lp_before = env.svm.get_account(&lp).unwrap();
        let rejected = env.try_set_matcher_config_with_trade_fee_cap(
            matcher_program,
            &lp_owner,
            lp,
            context,
            delegate,
            enabled,
            fee_cap,
        );
        assert!(rejected.is_err(), "{label} must reject");
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);
    }

    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker).unwrap();
    let lp_before = env.svm.get_account(&lp).unwrap();
    let zero = env.try_trade_asset_with_cu(0, &taker_owner, taker, &lp_owner, lp, 0, 100, 0);
    assert!(zero.is_err());
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);

    let epoch_before = env.portfolio_matcher_config(lp).position_epoch();
    env.trade_asset_with_cu(
        0,
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        POS_SCALE as i128,
        100,
        0,
    );
    assert_eq!(
        env.portfolio_matcher_config(lp).position_epoch(),
        epoch_before + 1
    );
    env.trade_asset_with_cu(
        0,
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        -(POS_SCALE as i128),
        100,
        0,
    );
    assert_eq!(env.market_state().1.assets[0].oi_eff_long_q, 0);
    assert_eq!(env.market_state().1.assets[0].oi_eff_short_q, 0);
    env.withdraw(&taker_owner, taker, 1_000_000);
    env.withdraw(&lp_owner, lp, 1_000_000);
    assert_eq!(env.token_amount(env.vault), 0);
}
