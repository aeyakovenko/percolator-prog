//! INV-079: module reachability of evidence claimed for INV-063 through INV-089.
//!
//! Existing ledgers check test declarations. Also require the declaring file to
//! belong to a public harness's module tree; include_str! alone is not a mount.
//! This checks source mounts, not execution, test bodies, or macro expansion.

use std::{collections::BTreeMap, path::PathBuf};

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

fn mounted_sources(replacement: Option<(&str, &str)>) -> BTreeMap<PathBuf, String> {
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
        mounted.insert(path, source);
    }
    mounted
}

fn evidence_gaps(mounted: &BTreeMap<PathBuf, String>) -> (usize, Vec<String>) {
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
                    .is_some_and(|source| super::source_defines_test(source, function))
                {
                    gaps.push(format!("{}: unmounted evidence {evidence}", fields[0]));
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
        // named witness must also survive in a mounted source file.
        if !mounted
            .values()
            .any(|source| super::source_defines_test(source, fields[2]))
        {
            gaps.push(format!("{}: unmounted discovery {}", fields[0], fields[2]));
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
        "INV-079 lifecycle evidence: {checked} references across {} mounted sources",
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
        let source = &baseline[&PathBuf::from(parent)];
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
