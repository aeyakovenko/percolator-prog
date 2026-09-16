# INV-079 lifecycle evidence mount audit

Base: remote `aeyakovenko/percolator-prog` main at
`cc3b1a502583253a84bdead6a19717680b3e76fd`, fetched on 2026-09-16.
Scope: evidence claimed for INV-063 through INV-089. Production and engine pins
are unchanged.

## Gap and guard

The generic benchmark, special-method and traceability guards accept a named
test declaration in a source file even when that file is disconnected from its
test harness. `include_str!` keeps that declaration visible to the guards without
registering the test. The existing INV-067 row417 guard protects one stateful
mount; it does not establish the other evidence paths.

The new [INV-079 child](public_sbf/inv_079_lifecycle_evidence_mounts.rs) parses the
three public harness roots and their transitive explicit module paths with `syn`.
It checks all in-scope references in `special_method_coverage.tsv`,
`traceability_gaps.tsv`, and `independent_discoveries.tsv` against that source
tree. Current census: **89 evidence references, 492 mounted source files**.
Discovery cross-invariant allowances remain owned by the existing benchmark
guard. The parser is a dev dependency at the version already present in the
lockfile; no dependency version changes.

Comments and string literals cannot create module edges. Conditional modules or
file-level attributes other than `cfg(test)` cannot establish an unconditional
mount. Alternative module forms/configurations require a reviewed extension.

The gap was selected from the generic source checks before comparing the withheld
benchmark mappings. The mapped max-source discoveries (212/357/359), terminal
scan discovery (424), and receipt discovery (417) demonstrate why losing a mount
would remove relevant coverage. No PR implementation or public-route fix was
copied, and no new behavioral rediscovery is claimed.

## Negative controls

Temporarily commenting out only the INV-077 module declaration and its path
attribute in `tests/v16_cu.rs`, leaving its source and metadata intact:

- Existing benchmark, special-method and traceability gates: **3 passed**.
- New mount census: **1 failed as expected**, identifying 12 unmounted references.
- Restoring the mount: final census and adjacent metadata controls **10 passed**.

The mutation test also checks three real mounts (CU INV-077, stateful INV-067,
and nested CU INV-070 terminal scan) against deletion, a block-comment decoy,
and `cfg(any())`. All **9 mutations** are rejected by the same census checker;
the unmodified tree is a required positive control. This selector is **1 passed**
(63.93 seconds), separate from the ten-test final command below. Every reported
test run selected nonzero tests. Temporary source mutations were restored.

## Reproduction

Run from the isolated worktree. The build cache is a private copy under
`/dev/shm`; compilation does not use the workspace's target directory.

```sh
export CARGO_TARGET_DIR=/dev/shm/percolator-astra-ultra-inv063-089-20260916-VEYxlT/target
export TMPDIR=/dev/shm
export CARGO_BUILD_JOBS=4
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0

# 1 passed, 9 rejected in-memory mutations.
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_lifecycle_evidence_mount_guard_rejects_detached_and_disabled_owners -- --exact --nocapture

# 10 passed after restoring the temporary INV-077 mount mutation.
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_lifecycle_metadata_evidence_is_mounted_in_public_harnesses \
  inv_079_public_reachability_evidence::v16_dated_open_security_finding_benchmark_is_non_overclaiming \
  inv_079_public_reachability_evidence::v16_special_verification_method_registry_matches_charter \
  inv_079_public_reachability_evidence::v16_special_method_cross_invariant_evidence_is_reviewed \
  inv_079_public_reachability_evidence::v16_traceability_gap_ledger_points_to_executable_evidence \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::v16_row417_metadata_retains_late_expiry_receipt_evidence \
  --nocapture

# With only the temporary commented INV-077 mount: 3 passed.
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact \
  inv_079_public_reachability_evidence::v16_dated_open_security_finding_benchmark_is_non_overclaiming \
  inv_079_public_reachability_evidence::v16_special_verification_method_registry_matches_charter \
  inv_079_public_reachability_evidence::v16_traceability_gap_ledger_points_to_executable_evidence \
  --nocapture

# With the same mutation: 1 expected failure. Restored source: 1 passed.
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_lifecycle_metadata_evidence_is_mounted_in_public_harnesses -- --exact --nocapture

# Build checks only; not counted as executed tests.
cargo test --locked --offline --test v16_cu --test v16_program_stateful_fuzz --no-run
rustfmt --check --edition 2021 --config skip_children=true tests/invariants/public_sbf/inv_079_public_reachability_evidence.rs tests/invariants/public_sbf/inv_079_lifecycle_evidence_mounts.rs
git diff --check
```

## Deliberately open

This establishes module reachability for the three named ledgers, not execution
of their behavioral witnesses. It reuses the existing named-test recognizer;
function-level `cfg`/`ignore`, macro expansion, test-body fidelity and exact
compiled-selector registration remain separate obligations. Other metadata
ledgers and INV-001 through INV-062 are outside this increment.

No SBF execution, CU measurement, Kani proof, liveness theorem or whole-invariant
closure is claimed. Public-route behavior and the broader open benchmark
obligations remain unchanged. Integration is suitable as a host-test/doc change;
normal integration CI should rerun the exact selectors on the merged tree.
