//! INV-084 - Proof assumptions are reachable and nonvacuous.
//!
//! Normative obligation: every explicit proof assumption is inventoried from
//! the actual mounted Kani modules, has a machine-checked two-sided witness, and
//! is established by a public route or a named solver-bound discharge.
//!
//! Evidence in this file (I/P roster): the source audit discovers mounted Kani
//! modules from `kani/v16_kani.rs`, extracts every `kani::assume` tuple, and
//! compares it exactly with the manifest. It also derives all 157 direct and 36
//! generated harnesses, classifies symbolic-total, branch-witnessed, explicitly
//! constrained, and concrete-exact proofs, and rejects missing claims, covers,
//! proof attributes, or public counterparts. A public LiteSVM composition reaches
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

#[derive(Clone, Debug)]
struct Inv084Function {
    file: String,
    name: String,
    body: String,
    proof: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct Inv084FunctionFacts {
    symbolic: bool,
    assumption: bool,
    claim: bool,
    cover: bool,
    branch_limited_claim: bool,
}

impl Inv084FunctionFacts {
    fn merge(&mut self, other: Self) {
        self.symbolic |= other.symbolic;
        self.assumption |= other.assumption;
        self.claim |= other.claim;
        self.cover |= other.cover;
        self.branch_limited_claim |= other.branch_limited_claim;
    }
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

fn inv_084_matching_brace(source: &str, open: usize) -> usize {
    let mut depth = 0usize;
    for (relative, byte) in source.as_bytes()[open..].iter().copied().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1).expect("unmatched closing brace");
                if depth == 0 {
                    return open + relative;
                }
            }
            _ => {}
        }
    }
    panic!("unterminated function body")
}

fn inv_084_top_level_functions(
    file: &str,
    source: &str,
) -> std::collections::BTreeMap<String, Inv084Function> {
    let mut functions = std::collections::BTreeMap::new();
    let mut offset = 0usize;

    for line in source.split_inclusive('\n') {
        if let Some(rest) = line.strip_prefix("fn ") {
            let name_end = rest
                .find(|byte: char| !(byte == '_' || byte.is_ascii_alphanumeric()))
                .expect("top-level function must have a signature");
            let name = &rest[..name_end];
            let open = offset
                + source[offset..]
                    .find('{')
                    .unwrap_or_else(|| panic!("{file}::{name} lacks a body"));
            let close = inv_084_matching_brace(source, open);
            let prefix = &source[..offset];
            let proof_attribute = prefix.rfind("#[kani::proof]");
            let previous_function = prefix.rfind("\nfn ");
            let proof = proof_attribute.is_some_and(|attribute| {
                previous_function.is_none_or(|previous| attribute > previous)
            });
            assert!(
                functions
                    .insert(
                        name.to_owned(),
                        Inv084Function {
                            file: file.to_owned(),
                            name: name.to_owned(),
                            body: source[open..=close].to_owned(),
                            proof,
                        },
                    )
                    .is_none(),
                "duplicate function {file}::{name}"
            );
        }
        offset += line.len();
    }
    functions
}

fn inv_084_is_word_at(source: &[u8], index: usize, word: &[u8]) -> bool {
    let Some(candidate) = source.get(index..index + word.len()) else {
        return false;
    };
    if candidate != word {
        return false;
    }
    let is_identifier = |byte: u8| byte == b'_' || byte.is_ascii_alphanumeric();
    source
        .get(index.wrapping_sub(1))
        .is_none_or(|byte| !is_identifier(*byte))
        && source
            .get(index + word.len())
            .is_none_or(|byte| !is_identifier(*byte))
}

fn inv_084_block_contains_claim(source: &str, keyword: &[u8]) -> bool {
    let bytes = source.as_bytes();
    for index in 0..bytes.len() {
        if !inv_084_is_word_at(bytes, index, keyword) {
            continue;
        }
        let tail = &source[index + keyword.len()..];
        let Some(open_relative) = tail.find('{') else {
            continue;
        };
        if tail
            .find(';')
            .is_some_and(|semicolon| semicolon < open_relative)
        {
            continue;
        }
        let open = index + keyword.len() + open_relative;
        let close = inv_084_matching_brace(source, open);
        let block = &source[open + 1..close];
        if block.contains("assert") || block.contains("panic!") || block.contains("return") {
            return true;
        }
    }
    false
}

fn inv_084_direct_function_facts(body: &str) -> Inv084FunctionFacts {
    Inv084FunctionFacts {
        symbolic: body.contains("kani::any"),
        assumption: body.contains("kani::assume("),
        claim: body.contains("assert")
            || body.contains("unreachable!")
            || body.contains("panic!")
            || body.contains(".unwrap()")
            || body.contains(".expect("),
        cover: body.contains("kani::cover!"),
        branch_limited_claim: body.contains("return;")
            || body.contains("=> return")
            || inv_084_block_contains_claim(body, b"if")
            || inv_084_block_contains_claim(body, b"else"),
    }
}

fn inv_084_transitive_function_facts(
    function: &str,
    functions: &std::collections::BTreeMap<String, Inv084Function>,
    visiting: &mut std::collections::BTreeSet<String>,
    cache: &mut std::collections::BTreeMap<String, Inv084FunctionFacts>,
) -> Inv084FunctionFacts {
    if let Some(facts) = cache.get(function) {
        return *facts;
    }
    assert!(
        visiting.insert(function.to_owned()),
        "recursive proof helper {function} requires an explicit disposition"
    );
    let body = &functions
        .get(function)
        .unwrap_or_else(|| panic!("missing function {function}"))
        .body;
    let mut facts = inv_084_direct_function_facts(body);
    for helper in functions.keys() {
        if helper == function {
            continue;
        }
        if body.contains(&format!("{helper}(")) || body.contains(&format!("{helper}::<")) {
            facts.merge(inv_084_transitive_function_facts(
                helper, functions, visiting, cache,
            ));
        }
    }
    visiting.remove(function);
    cache.insert(function.to_owned(), facts);
    facts
}

fn inv_084_generated_trade_decoder_harnesses(source: &str) -> std::collections::BTreeSet<String> {
    let mut generated = std::collections::BTreeSet::new();
    for macro_name in ["prove_nocpi_trade_field!(", "prove_cpi_trade_field!("] {
        let mut tail = source;
        while let Some(start) = tail.find(macro_name) {
            tail = &tail[start + macro_name.len()..];
            let end = tail
                .find(");")
                .unwrap_or_else(|| panic!("unterminated {macro_name} invocation"));
            let arguments = tail[..end].split(',').map(str::trim).collect::<Vec<_>>();
            assert_eq!(arguments.len(), 4, "{macro_name} must keep four arguments");
            for name in &arguments[..2] {
                assert!(
                    name.starts_with("kani_v16_") && generated.insert((*name).to_owned()),
                    "invalid or duplicate generated harness {name}"
                );
            }
            tail = &tail[end + 2..];
        }
    }
    generated
}

#[test]
fn v16_program_every_mounted_explicit_kani_assumption_is_exactly_inventoried() {
    use std::collections::{BTreeMap, BTreeSet};

    const EXPECTED_MOUNTED_MODULES: usize = 24;
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
fn v16_program_every_mounted_kani_harness_has_a_nonvacuity_disposition() {
    use std::collections::{BTreeMap, BTreeSet};

    const EXPECTED_DIRECT_HARNESSES: usize = 157;
    const EXPECTED_GENERATED_HARNESSES: usize = 36;
    const EXPECTED_TOTAL_HARNESSES: usize = 193;
    const CONCRETE_PUBLIC_EVIDENCE: [(&str, &str); 3] = [
        (
            "tests/invariants/kani/inv_020_authenticated_clock_slot_and_oracle_provenance.rs",
            "tests/invariants/cu/inv_020_authenticated_clock_slot_and_oracle_provenance.rs#host_oracle_valid_layout_boundaries_match_independent_typed_reference",
        ),
        (
            "tests/invariants/kani/inv_022_instruction_decoding_and_schema_upgrade_safety.rs",
            "tests/invariants/cu/inv_022_instruction_decoding_and_schema_upgrade_safety.rs#v16_program_deployed_decoder_bit_mutation_matrix_is_total_canonical_and_atomic",
        ),
        (
            "tests/invariants/kani/inv_084_proof_assumptions_are_reachable_and_nonvacuous.rs",
            "tests/invariants/cu/inv_084_proof_assumptions_are_reachable_and_nonvacuous.rs#v16_program_explicit_kani_guard_domains_are_publicly_reachable_and_fail_closed",
        ),
    ];

    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mounted_files = inv_084_mounted_kani_files();
    let root_functions = inv_084_top_level_functions("kani/v16_kani.rs", INV_084_KANI_ROOT);
    let concrete_evidence = CONCRETE_PUBLIC_EVIDENCE
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    let mut direct_harnesses = BTreeSet::new();
    let mut generated_harnesses = BTreeSet::new();
    let mut category_counts = BTreeMap::<&str, usize>::new();

    for file in &mounted_files {
        let source = std::fs::read_to_string(manifest.join(file))
            .unwrap_or_else(|error| panic!("read mounted Kani module {file}: {error}"));
        let local_functions = inv_084_top_level_functions(file, &source);
        let mut all_functions = root_functions.clone();
        for (name, function) in &local_functions {
            assert!(
                all_functions
                    .insert(name.clone(), function.clone())
                    .is_none(),
                "root/local helper collision for {file}::{name}"
            );
        }

        let assumptions = inv_084_source_assumptions(file, &source)
            .into_iter()
            .map(|assumption| assumption.owning_proof)
            .collect::<BTreeSet<_>>();
        let mut cache = BTreeMap::new();
        for function in local_functions
            .values()
            .filter(|function| function.name.starts_with("kani"))
        {
            assert!(
                function.proof,
                "{}#{} is named as a harness but lacks its own #[kani::proof] attribute",
                function.file, function.name
            );
            assert!(
                direct_harnesses.insert(format!("{}#{}", function.file, function.name)),
                "duplicate direct Kani harness {}#{}",
                function.file,
                function.name
            );
            let facts = inv_084_transitive_function_facts(
                &function.name,
                &all_functions,
                &mut BTreeSet::new(),
                &mut cache,
            );
            assert!(
                facts.claim,
                "{}#{} has no direct or helper assertion/panic claim",
                function.file, function.name
            );
            assert_eq!(
                facts.assumption,
                assumptions.contains(&function.name),
                "{}#{} assumption ownership differs from the exact source inventory",
                function.file,
                function.name
            );
            if facts.branch_limited_claim {
                assert!(
                    facts.cover,
                    "{}#{} makes a branch-limited claim without a constructive Kani cover",
                    function.file, function.name
                );
            }

            let category = if facts.assumption {
                "EXPLICIT_ASSUMPTION"
            } else if !facts.symbolic {
                let evidence = concrete_evidence.get(file.as_str()).unwrap_or_else(|| {
                    panic!(
                        "{}#{} is a concrete fixture without module-level public evidence",
                        function.file, function.name
                    )
                });
                let (evidence_file, evidence_function) = evidence
                    .split_once('#')
                    .expect("concrete evidence must be path#function");
                let evidence_source = std::fs::read_to_string(manifest.join(evidence_file))
                    .unwrap_or_else(|error| panic!("read {evidence_file}: {error}"));
                assert!(
                    evidence_source.contains(&format!("fn {evidence_function}(")),
                    "concrete fixture evidence {evidence} is absent"
                );
                "CONCRETE_EXACT"
            } else if facts.branch_limited_claim {
                "BRANCH_WITNESSED"
            } else {
                "SYMBOLIC_TOTAL"
            };
            *category_counts.entry(category).or_default() += 1;
        }

        generated_harnesses.extend(inv_084_generated_trade_decoder_harnesses(&source));
    }

    assert_eq!(direct_harnesses.len(), EXPECTED_DIRECT_HARNESSES);
    assert_eq!(generated_harnesses.len(), EXPECTED_GENERATED_HARNESSES);
    assert!(
        generated_harnesses.iter().all(|name| direct_harnesses
            .iter()
            .all(|direct| !direct.ends_with(&format!("#{name}")))),
        "generated and direct Kani harness names must be disjoint"
    );
    assert_eq!(
        direct_harnesses.len() + generated_harnesses.len(),
        EXPECTED_TOTAL_HARNESSES,
        "the source-derived harness roster must equal `cargo kani list`"
    );
    let generated_source =
        std::fs::read_to_string(manifest.join(
            "tests/invariants/kani/inv_022_instruction_decoding_and_schema_upgrade_safety.rs",
        ))
        .expect("read generated trade-decoder proof source");
    assert!(generated_source.contains("fields.$field = kani::any::<$ty>()"));
    assert!(generated_source.contains("assert_single_nocpi_trade_decoder_preserves(fields)"));
    assert!(generated_source.contains("assert_batch_nocpi_trade_decoder_preserves(fields)"));
    assert!(generated_source.contains("assert_single_cpi_trade_decoder_preserves(fields)"));
    assert!(generated_source.contains("assert_batch_cpi_trade_decoder_preserves(fields)"));
    category_counts.insert("GENERATED_SYMBOLIC_TOTAL", generated_harnesses.len());

    let expected_categories = BTreeMap::from([
        ("BRANCH_WITNESSED", 27usize),
        ("CONCRETE_EXACT", 29),
        ("EXPLICIT_ASSUMPTION", 10),
        ("GENERATED_SYMBOLIC_TOTAL", 36),
        ("SYMBOLIC_TOTAL", 91),
    ]);
    assert_eq!(
        category_counts, expected_categories,
        "every harness category change requires a deliberate nonvacuity review"
    );
    assert_eq!(
        category_counts.values().sum::<usize>(),
        EXPECTED_TOTAL_HARNESSES
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
