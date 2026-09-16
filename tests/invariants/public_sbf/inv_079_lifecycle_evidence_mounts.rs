//! INV-079: module reachability of evidence claimed for INV-063 through INV-089.
//!
//! Require both a mounted file and an unconditional, nonignored test item.
//! Text in comments, strings or nested functions cannot supply test evidence.
//! The existing proptest! declaration form is recognized explicitly. Other macro
//! expansion, execution and test-body fidelity remain separate obligations.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

const ROOTS: [&str; 3] = [
    "tests/v16_cu.rs",
    "tests/v16_program_fuzz_regressions.rs",
    "tests/v16_program_stateful_fuzz.rs",
];

fn in_scope(invariant: &str) -> bool {
    invariant
        .strip_prefix("INV-")
        .and_then(|id| id.parse::<u16>().ok())
        .is_some_and(|id| (63..=89).contains(&id))
}

fn unconditional_test_mount(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().all(|attr| {
        if attr.path().is_ident("cfg") {
            attr.parse_args::<syn::Path>()
                .is_ok_and(|path| path.is_ident("test"))
        } else {
            // Feature/platform conditions need a separate configuration witness.
            !attr.path().is_ident("cfg_attr")
        }
    })
}

#[derive(Clone)]
struct MountedSource {
    source: String,
    available_tests: BTreeSet<String>,
}

fn available_test(attrs: &[syn::Attribute]) -> bool {
    unconditional_test_mount(attrs)
        && attrs.iter().any(|attr| attr.path().is_ident("test"))
        && !attrs.iter().any(|attr| attr.path().is_ident("ignore"))
}

struct PropertyTests(BTreeSet<String>);

impl syn::parse::Parse for PropertyTests {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let config = input.call(syn::Attribute::parse_inner)?;
        if config
            .iter()
            .any(|attr| !attr.path().is_ident("proptest_config"))
        {
            return Err(input.error("unreviewed proptest configuration attribute"));
        }
        let mut tests = BTreeSet::new();
        while !input.is_empty() {
            let attrs = input.call(syn::Attribute::parse_outer)?;
            input.parse::<syn::Token![fn]>()?;
            let name = input.parse::<syn::Ident>()?;
            let arguments;
            syn::parenthesized!(arguments in input);
            // Proptest's strategy arguments are not Rust function parameters.
            // Consume their token trees without searching inside them for tests.
            while !arguments.is_empty() {
                arguments.step(|cursor| {
                    cursor
                        .token_tree()
                        .map(|(_, rest)| ((), rest))
                        .ok_or_else(|| cursor.error("expected property-test argument"))
                })?;
            }
            input.parse::<syn::Block>()?;
            if available_test(&attrs) {
                tests.insert(name.to_string());
            }
        }
        Ok(Self(tests))
    }
}

impl MountedSource {
    fn from_parsed(source: String, file: &syn::File) -> Self {
        let mut available_tests = BTreeSet::new();
        if unconditional_test_mount(&file.attrs) {
            for item in &file.items {
                match item {
                    syn::Item::Fn(function) if available_test(&function.attrs) => {
                        available_tests.insert(function.sig.ident.to_string());
                    }
                    syn::Item::Macro(item)
                        if item.mac.path.is_ident("proptest")
                            && unconditional_test_mount(&item.attrs) =>
                    {
                        // Unsupported macro forms supply no evidence until reviewed.
                        if let Ok(tests) = item.mac.parse_body::<PropertyTests>() {
                            available_tests.extend(tests.0);
                        }
                    }
                    _ => {}
                }
            }
        }
        Self {
            source,
            available_tests,
        }
    }
}

fn mounted_sources(replacement: Option<(&str, &str)>) -> BTreeMap<PathBuf, MountedSource> {
    let mut pending = ROOTS.map(PathBuf::from).to_vec();
    let mut mounted = BTreeMap::new();
    while let Some(path) = pending.pop() {
        if mounted.contains_key(&path) {
            continue;
        }
        let source = match replacement.filter(|(changed, _)| path == PathBuf::from(changed)) {
            Some((_, source)) => source.to_owned(),
            None => std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(&path),
            )
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display())),
        };
        let file = syn::parse_file(&source)
            .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()));
        if !unconditional_test_mount(&file.attrs) {
            continue;
        }
        let indexed = MountedSource::from_parsed(source, &file);
        for item in file.items {
            let syn::Item::Mod(module) = item else {
                continue;
            };
            if !unconditional_test_mount(&module.attrs) {
                continue;
            }
            // The invariant suites use explicit file mounts. Unsupported mount
            // forms cannot silently certify a referenced evidence file.
            for attr in module
                .attrs
                .iter()
                .filter(|attr| attr.path().is_ident("path"))
            {
                let syn::Meta::NameValue(meta) = &attr.meta else {
                    panic!("nonliteral module path in {}", path.display());
                };
                let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(value),
                    ..
                }) = &meta.value
                else {
                    panic!("nonliteral module path in {}", path.display());
                };
                assert!(module.content.is_none(), "inline path mounts need review");
                let child = path.parent().unwrap().join(value.value());
                assert!(
                    !child
                        .components()
                        .any(|part| matches!(part, std::path::Component::ParentDir)),
                    "parent-relative evidence mount needs review: {}",
                    child.display()
                );
                if child.starts_with("tests/invariants") {
                    pending.push(child);
                }
            }
        }
        mounted.insert(path, indexed);
    }
    mounted
}

fn evidence_gaps(mounted: &BTreeMap<PathBuf, MountedSource>) -> (usize, Vec<String>) {
    let mut checked = 0;
    let mut gaps = Vec::new();
    for (ledger, width, column) in [
        (include_str!("../special_method_coverage.tsv"), 5, 3),
        (include_str!("../traceability_gaps.tsv"), 8, 3),
    ] {
        for line in ledger.lines().filter(|line| !line.starts_with('#')) {
            let fields: Vec<_> = line.split('\t').collect();
            if !in_scope(fields[0]) {
                continue;
            }
            assert_eq!(fields.len(), width, "evidence ledger schema changed");
            if fields[column] == "-" {
                continue;
            }
            for evidence in fields[column].split(';').map(str::trim) {
                checked += 1;
                let (path, function) = evidence.split_once('#').expect("path#test evidence");
                if !mounted
                    .get(&PathBuf::from(path))
                    .is_some_and(|source| source.available_tests.contains(function))
                {
                    gaps.push(format!(
                        "{}: unavailable test evidence {evidence}",
                        fields[0]
                    ));
                }
            }
        }
    }
    for line in include_str!("../independent_discoveries.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        let fields: Vec<_> = line.split('\t').collect();
        if !in_scope(fields[0]) {
            continue;
        }
        assert_eq!(fields.len(), 5, "discovery ledger schema changed");
        checked += 1;
        // The benchmark gate owns reviewed cross-invariant borrowing. Here its
        // named witness must also survive as an available test in a mounted file.
        if !mounted
            .values()
            .any(|source| source.available_tests.contains(fields[2]))
        {
            gaps.push(format!(
                "{}: unavailable discovery {}",
                fields[0], fields[2]
            ));
        }
    }
    (checked, gaps)
}

#[test]
fn v16_lifecycle_metadata_evidence_is_mounted_in_public_harnesses() {
    let mounted = mounted_sources(None);
    let (checked, gaps) = evidence_gaps(&mounted);
    assert!(checked > 0, "the scoped evidence census must not be empty");
    assert!(gaps.is_empty(), "{}", gaps.join("\n"));
    eprintln!(
        "INV-079 lifecycle evidence: {checked} available test references across {} mounted sources",
        mounted.len()
    );
}

#[test]
fn v16_lifecycle_evidence_mount_guard_rejects_detached_and_disabled_owners() {
    let baseline = mounted_sources(None);
    assert!(evidence_gaps(&baseline).1.is_empty());
    let cases = [
        (
            ROOTS[0],
            "invariants/cu/inv_077_bounded_work_and_maximum_shape_compute.rs",
            "inv_077_bounded_work_and_maximum_shape_compute",
            "v16_attack_public_10m_market_max_source_owner_exit_stays_bounded",
        ),
        (
            ROOTS[2],
            "invariants/stateful/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs",
            "inv_067_terminal_payout_completeness_and_exact_once_settlement",
            "v16_program_late_unrelated_backing_cannot_outlive_and_erase_resolved_receipt",
        ),
        (
            "tests/invariants/cu/inv_070_zero_unattributed_terminal_residue_and_close_slab.rs",
            "inv_070_terminal_scan_recredit.rs",
            "terminal_scan_recredit",
            "v16_program_terminal_scan_rediscovers_earlier_insurance_after_later_expiry",
        ),
    ];
    for (parent, child, module, witness) in cases {
        let source = &baseline[&PathBuf::from(parent)].source;
        let mount = format!("#[path = \"{child}\"]\nmod {module};");
        assert_eq!(
            source.matches(&mount).count(),
            1,
            "mutation must alter one real mount"
        );
        for replacement in [
            String::new(),
            format!("/* {mount} */"),
            format!("#[cfg(any())]\n{mount}"),
        ] {
            let mutated = source.replacen(&mount, &replacement, 1);
            let (_, gaps) = evidence_gaps(&mounted_sources(Some((parent, &mutated))));
            assert!(
                gaps.iter().any(|gap| gap.contains(witness)),
                "mount mutation must invalidate {witness}, got {gaps:?}"
            );
        }
    }
    eprintln!(
        "INV-079 lifecycle mount guard rejected 9 detached/commented/disabled evidence mutations"
    );
}

#[test]
fn v16_lifecycle_evidence_guard_rejects_disabled_and_decoy_test_declarations() {
    let baseline = mounted_sources(None);
    assert!(evidence_gaps(&baseline).1.is_empty());
    let cases = [
        (
            "tests/invariants/cu/inv_077_bounded_work_and_maximum_shape_compute.rs",
            "v16_attack_public_10m_market_max_source_owner_exit_stays_bounded",
        ),
        (
            "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
            "v16_program_reference_model_dimension_composition_is_source_complete",
        ),
        (
            "tests/invariants/public_sbf/inv_079_public_reachability_evidence.rs",
            "v16_public_terminal_classifier_exhausts_normalized_outcome_space",
        ),
        (
            "tests/invariants/stateful/inv_030_credit_rate_determinism_and_fail_closed_behavior.rs",
            "v16_program_source_credit_rate_lifecycle_matches_independent_oracle",
        ),
    ];
    let mut rejected = 0;
    let mut accepted = 0;
    for (path, witness) in cases {
        let source = &baseline[&PathBuf::from(path)].source;
        let substituted_gaps = |source: String| {
            let file = syn::parse_file(&source).expect("valid substitution syntax");
            let mut mutated = baseline.clone();
            mutated.insert(
                PathBuf::from(path),
                MountedSource::from_parsed(source, &file),
            );
            evidence_gaps(&mutated).1
        };
        let declaration = format!("fn {witness}(");
        assert_eq!(source.matches(&declaration).count(), 1);
        let mut replacements = Vec::new();
        for attr in [
            "#[ignore]",
            "#[ignore = \"disabled evidence\"]",
            "#[cfg(any())]",
            "#[cfg(all(test, any()))]",
            "#[cfg_attr(test, ignore)]",
        ] {
            replacements.push(source.replacen(&declaration, &format!("{attr}\n{declaration}"), 1));
        }
        let decoy = format!("#[test]\nfn {witness}() {{}}");
        replacements.extend([
            format!("/*\n{decoy}\n*/"),
            format!("const DECOY: &str = r#\"\n{decoy}\n\"#;"),
            format!("fn helper() {{\n{decoy}\n}}"),
            format!("#[cfg(any())]\nmod disabled {{\n{decoy}\n}}"),
        ]);
        for source in replacements {
            assert!(
                super::source_defines_test(&source, witness),
                "negative control must be accepted by the old declaration recognizer"
            );
            let gaps = substituted_gaps(source);
            assert!(
                gaps.iter().any(|gap| gap.contains(witness)),
                "disabled or decoy declaration must invalidate {path}#{witness}: {gaps:?}"
            );
            rejected += 1;
        }
        for attr in [
            "#[cfg(test)]",
            "#[allow(dead_code)]",
            "#[doc = \"#[ignore] is only documentation here\"]",
        ] {
            let source = source.replacen(&declaration, &format!("{attr}\n{declaration}"), 1);
            assert!(
                substituted_gaps(source).is_empty(),
                "unconditional test evidence must survive {attr}: {path}#{witness}"
            );
            accepted += 1;
        }
    }
    assert_eq!((rejected, accepted), (36, 12));
    eprintln!("INV-079 test availability: {rejected} rejected substitutions, {accepted} positive controls");
}

#[test]
fn v16_lifecycle_property_evidence_requires_enabled_macro_and_test_items() {
    let path = PathBuf::from(
        "tests/invariants/stateful/inv_030_credit_rate_determinism_and_fail_closed_behavior.rs",
    );
    let witness = "v16_program_source_credit_rate_lifecycle_matches_independent_oracle";
    let baseline = mounted_sources(None);
    assert!(evidence_gaps(&baseline).1.is_empty());
    let source = &baseline[&path].source;
    let invocation = "proptest! {";
    assert_eq!(source.matches(invocation).count(), 1);
    for attr in ["#[cfg(any())]", "#[cfg_attr(test, cfg(any()))]"] {
        let source = source.replacen(invocation, &format!("{attr}\n{invocation}"), 1);
        assert!(super::source_defines_test(&source, witness));
        let file = syn::parse_file(&source).unwrap();
        let mut mutated = baseline.clone();
        mutated.insert(path.clone(), MountedSource::from_parsed(source, &file));
        let (_, gaps) = evidence_gaps(&mutated);
        assert!(gaps.iter().any(|gap| gap.contains(witness)), "{gaps:?}");
    }
    eprintln!("INV-079 property-test availability: 2 rejected disabled macro invocations");
}
