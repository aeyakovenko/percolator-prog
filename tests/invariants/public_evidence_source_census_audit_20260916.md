# INV-079 public evidence source census

Base: latest `origin/main` at task start, `4a1d375e19a5237e9cddfdfae7d06c3e8d171b51`.
Worktree: `/dev/shm/percolator-invariant-traceability-freshness-20260916-b73e`.
Primary owner: INV-079. Secondary evidence-availability relevance: INV-081/084/087/089.
Production, dependency pins, proof bodies and invariant statuses are unchanged.

## Net-new gap

The existing lifecycle guard starts from three ledgers and validates their named
tests. A newly added source absent from both ledgers and mounts is invisible to
that guard. Likewise, an unlisted child such as INV-084's deposit assumption
contract can lose its mount while the ledger census remains green. INV-081's
composition and INV-087's field rosters name selected witnesses; INV-084 inventories
mounted Kani sources; INV-089 exercises activation/reuse. None discovers every
public invariant source independently of those declarations.

The new guard recursively discovers all Rust sources under `tests/invariants`,
excluding the separate `kani` configuration, and requires each to occur in the
existing AST-derived graph of the three public harnesses. Helper/aggregator files
also require a mount. The same parser now retains declared and available tests
separately, including inline modules and the existing `proptest!` syntax, so an
unlisted ignored or disabled declaration cannot silently disappear. Comments and
strings do not create declarations. Unsupported property syntax fails for review.

The census derives its file/test totals from the tree, not a maintained roster.
It currently finds 500 files and 1,904 available tests. Its selectors are linked
to an INV-079 traceability row; the ledger schema guard deliberately grows from
25 to 26 rows. This change does not duplicate the dynamic-account trace test in
base commit `4a1d375e` or alter its recorder.

## Negative Controls

Before implementation, an actual temporary file
`tests/invariants/public_sbf/inv_079_unmounted_freshness_probe.rs` containing one
ordinary test was added without a mount or ledger entry. The four exact existing
gates below passed (4 passed, 0 failed). With that same file present, the new census
failed (0 passed, 1 failed), naming its unmounted path. The probe was then deleted.

The retained mutation selector rejects 35 cases: four unlisted paths spanning all
three suites plus a nested audit directory; seven disabled-declaration forms at
each path; and three mutations of the actual INV-084 deposit-contract mount
(deleted, commented, or `cfg(any())`). Each mounted-source substitution and each
real mount mutation explicitly leaves the old ledger gate green. Sixteen enabled
direct, `cfg(test)`, inline and property declarations are accepted. These controls
operate in memory, so parallel tests do not race on mutated source files.

## Commands And Results

All commands run from the private worktree, using its own `target` directory.
The initial default-debug build was interrupted and restarted with the following
bounded build settings; no other worktree or build directory was used.

Final result: **12 passed, 0 failed, 0 ignored**, 133 filtered out, 36.51 seconds.
The ledger census now checks 91 references across 503 mounted sources; the reverse
census checks 500 public invariant sources and 1,904 available declarations.
Both scoped rustfmt checks and `git diff --check` pass. A final fetch still resolves
`origin/main` to the base above. Existing dead-code/future-compatibility warnings
do not affect these results.

```sh
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0

# Before implementation, with the temporary unmounted file: 4 passed, 0 failed.
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_lifecycle_metadata_evidence_is_mounted_in_public_harnesses \
  inv_079_public_reachability_evidence::v16_dated_open_security_finding_benchmark_is_non_overclaiming \
  inv_079_public_reachability_evidence::v16_special_verification_method_registry_matches_charter \
  inv_079_public_reachability_evidence::v16_traceability_gap_ledger_points_to_executable_evidence --nocapture

# After implementation, same temporary file: 1 expected failure; restored: passes.
cargo test --locked --offline --test v16_program_fuzz_regressions \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_every_public_invariant_source_and_declared_test_is_mounted -- --exact --nocapture

# Final affected parser and metadata selectors.
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_every_public_invariant_source_and_declared_test_is_mounted \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_public_source_census_rejects_unlisted_and_disabled_evidence \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_lifecycle_metadata_evidence_is_mounted_in_public_harnesses \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_lifecycle_evidence_mount_guard_rejects_detached_and_disabled_owners \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_lifecycle_evidence_guard_rejects_disabled_and_decoy_test_declarations \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_lifecycle_property_evidence_requires_enabled_macro_and_test_items \
  inv_079_public_reachability_evidence::v16_dated_open_security_finding_benchmark_is_non_overclaiming \
  inv_079_public_reachability_evidence::v16_special_verification_method_registry_matches_charter \
  inv_079_public_reachability_evidence::v16_special_method_cross_invariant_evidence_is_reviewed \
  inv_079_public_reachability_evidence::v16_traceability_gap_ledger_points_to_executable_evidence \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete --nocapture

rustfmt --edition 2021 --config skip_children=true --check \
  tests/invariants/public_sbf/inv_079_lifecycle_evidence_mounts.rs \
  tests/invariants/public_sbf/inv_079_public_reachability_evidence.rs
git diff --check
```

## Limits

This is a host source-availability guard, not execution of all 1,904 behavioral
tests. The existing explicit-path mount forms and reviewed test macros are its
syntax boundary. Arbitrary macro expansion, other compiler configurations,
conditional test attributes that generate tests, Kani mount completeness,
semantic nonvacuity, source-witness assertion fidelity and writer/read/effect
equivalence remain separate obligations. No SBF execution, Kani solver result,
CU measurement, new public LoF/DoS defect or whole-invariant closure is claimed.
