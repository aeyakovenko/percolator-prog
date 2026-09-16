# INV-070/088 terminal-scan evidence binding

Base: `e649508587d4cb49307c73a9660ce598314f0882` (`origin/main`).
Worktree: `/dev/shm/percolator-astra-ultra-20260916-uUhd6F/worktree`.

## Gap and scope

The INV-070 composition roster preserves the terminal scan's public expiry
witness. INV-079 checks that benchmark discoveries name executable,
invariant-owned tests, and its lifecycle guard checks module mounts. Those
checks do not bind a discovery's selector to the obligation it is cited for.
An executable CloseSlab rejection test could replace row 424's expiry/prefix
witness in the discovery ledger while all three checks stayed green.

The reopening gate also accepts narrowing row 424's general environmental
actionability obligation to backing expiry alone. Keeping a three-dimensional
product, a unique property string and OPEN status satisfies that gate even
though the stated obligation has changed.

[The new guard](cu/inv_070_terminal_scan_evidence.rs) retains the reviewed
fingerprint, selector and oracle; the cursor/time/earlier-slot product; the
general actionability-invalidation property; and the affected in-scope lifecycle
and summary invariants. The bounded discovery remains OPEN for arbitrary
environmental histories. Additional evidence can coexist with the retained
witness; replacing it or closing the general obligation requires review.

This is a metadata-conformance gap, not a new economic counterexample or an
independent rediscovery of the holdout. The benchmark comparison used checked-in
titles and metadata only; no open PR implementation or fix was imported.
Production, dependencies, engine pin, benchmark classification and invariant
statuses are unchanged. The INV-079 mount guard is reused without modification.

## Negative controls

Each temporary mutation was applied in this isolated worktree and restored.
All runs below used exact selectors with one executed test, not listings.

1. In `independent_discoveries.tsv`, replace only row 424's selector
   `v16_program_terminal_scan_rediscovers_earlier_insurance_after_later_expiry`
   with `v16_program_close_slab_rejects_until_market_has_zero_terminal_residue`.
   Existing benchmark, reopening, lifecycle-mount and INV-070 composition
   selectors: **4 passed**. New binding selector: **1 expected failure**, with
   `row 424 lost its scan-restart fingerprint, selector, or oracle`.
2. Restore the discovery, then replace only row 424's property in
   `coverage_reopenings.tsv` with `backing-expiry-restarts-the-terminal-scan`.
   Existing benchmark, reopening, charter/index, machine-status and audit-summary
   selectors: **5 passed**. New binding selector: **1 expected failure**, with
   `row 424 must retain general actionability invalidation`.

The committed mutation test additionally rejects six substitutions in memory:
selector, fingerprint, oracle, cross-product, general property and removal of
INV-088 from the affected set. It first requires the real ledgers to pass and
requires exactly one occurrence of each mutation target.

## Verification commands

The host target is a private copy of cached build artifacts. Cargo rebuilt the
changed integration binaries in this worktree. No shared target was written.
No SBF was built or executed and no CU measurement is claimed.

```bash
cd /dev/shm/percolator-astra-ultra-20260916-uUhd6F/worktree
export CARGO_TARGET_DIR=/dev/shm/percolator-astra-ultra-20260916-uUhd6F/target
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm

cargo test --locked --offline --test v16_cu inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_scan_evidence::v16_row424_metadata_retains_terminal_scan_invalidation_evidence -- --exact --nocapture
cargo test --locked --offline --test v16_cu inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_scan_evidence::v16_row424_metadata_guard_rejects_unrelated_or_narrowed_evidence -- --exact --nocapture
cargo test --locked --offline --test v16_cu inv_070_zero_unattributed_terminal_residue_and_close_slab::v16_program_terminal_stock_and_close_slab_composition_is_source_complete -- --exact --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_dated_open_security_finding_benchmark_is_non_overclaiming -- --exact --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant -- --exact --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_lifecycle_metadata_evidence_is_mounted_in_public_harnesses -- --exact --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming -- --exact --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row -- --exact --nocapture

rustfmt --edition 2021 --check tests/invariants/cu/inv_070_terminal_scan_evidence.rs
git diff --check
```

Final restored-tree result: **9 passed, 0 failed**, one test per command. The
second selector checks six nonvacuous metadata mutations. The two external
negative controls each execute one failing test; they are not passing coverage.
Formatting is checked on the new Rust file; the existing parent changes only
by a module declaration. Logs are under the worktree's parent directory.

## Remaining gaps

Row 424's arbitrary environmental-writer, custody, classification and persisted
prefix histories remain OPEN, including their larger-shape products. This guard
does not establish liveness, rollback, public-route correctness, maximum compute,
proof validity or economic assertions inside the referenced witness. Other
benchmark mappings may still permit unrelated executable evidence substitutions.
Future witness replacement requires updating this binding with reviewed evidence.
