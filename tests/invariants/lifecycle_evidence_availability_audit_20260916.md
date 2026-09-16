# INV-079 lifecycle evidence availability

Base: `origin/main` at `2d2af7cc`, fetched on 2026-09-16. Worktree:
`/dev/shm/astra-ultra-inv063-089-cycle3-20260916222942`.
Scope: evidence for INV-063 through INV-089 in the three ledgers already checked
by the lifecycle mount guard. Production, dependencies, engine pin, metadata
classifications and invariant statuses are unchanged.

## Confirmed Gap

The existing guard establishes that a source file is mounted, then recognizes
test declarations by line text. A function can retain its name and `#[test]`
while `#[ignore]` prevents execution or `#[cfg(any())]` removes it from the
compiled test suite. Comments, strings and functions nested inside a helper can
also satisfy that recognizer.

The on-disk negative control added only
`#[ignore = "temporary INV-079 availability negative control"]` to
`v16_attack_public_10m_market_max_source_owner_exit_stays_bounded`:

- The existing benchmark, special-method registry, traceability and lifecycle
  mount gates passed **4/4**.
- Its exact CU selector collected **1 ignored test, 0 executed tests**. This is
  negative-control evidence, not a passing behavioral verification.
- After the guard change, the same on-disk mutation caused the exact lifecycle
  census to fail **1/1 as expected**, naming both affected INV-077 ledger entries.
- The mutation was removed. The restored exact public CU test passed **1/1**.

The new declaration mutation test was also run against the original recognizer:
**1 expected failure**, because an ignored witness produced no evidence gap.
This establishes a bad substitution accepted by current-main gates; this is
not a newly discovered runtime loss-of-funds or denial-of-service bug.

## Change and Bounds

The existing [guard](public_sbf/inv_079_lifecycle_evidence_mounts.rs) now indexes
parsed test items once per mounted source. Direct tests require `#[test]`,
no `#[ignore]`, and no conditional attributes except plain `#[cfg(test)]`.
The same condition policy already applied to file and module mounts.

Four current references use `proptest!`. A small `syn` parser recognizes that
existing declaration form, checks the macro invocation and each generated test's
attributes, and consumes strategy arguments and bodies without searching them for
test names. Unsupported macro forms provide no evidence. No dependency was added.
Bare names in these ledgers refer to direct items in the named file; inline
modules and functions nested inside helpers do not supply those declarations.

The census passes **89 references across 494 mounted source files**.
The two new exact mutation selectors reject **36 declaration substitutions**
across CU, stateful, classifier and property-test witnesses, plus **2 disabled
property macro invocations**. All 38 substitutions remain acceptable to the old
line recognizer. **12 positive controls** retain ordinary attributes,
`#[cfg(test)]`, and documentation containing the text `#[ignore]`.
The existing **9 module-detachment mutations** also pass.

This is source-availability evidence. It does not prove that every test executes
in CI, that a test body asserts the right property, that property-test cases are
nonzero, or that all macro expansions and feature configurations are equivalent.
Other evidence ledgers, composition rosters and INV-001 through INV-062 remain
outside this increment. No invariant is promoted to closed.

## Exact Verification

Run from the isolated worktree:

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-inv063-089-target-20260916222942
export TMPDIR=/dev/shm
export CARGO_BUILD_JOBS=2
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0

# Final guard/control run: 8 passed, 0 failed, 0 ignored.
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_lifecycle_metadata_evidence_is_mounted_in_public_harnesses \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_lifecycle_evidence_mount_guard_rejects_detached_and_disabled_owners \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_lifecycle_evidence_guard_rejects_disabled_and_decoy_test_declarations \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_lifecycle_property_evidence_requires_enabled_macro_and_test_items \
  inv_079_public_reachability_evidence::v16_dated_open_security_finding_benchmark_is_non_overclaiming \
  inv_079_public_reachability_evidence::v16_special_verification_method_registry_matches_charter \
  inv_079_public_reachability_evidence::v16_traceability_gap_ledger_points_to_executable_evidence \
  inv_079_public_reachability_evidence::v16_special_method_cross_invariant_evidence_is_reviewed \
  --nocapture

# New declaration test on the original guard: 1 expected failure.
cargo test --locked --offline --test v16_program_fuzz_regressions \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_lifecycle_evidence_guard_rejects_disabled_and_decoy_test_declarations \
  -- --exact --nocapture

# With the temporary ignored CU witness: original guard passes, new guard fails.
cargo test --locked --offline --test v16_program_fuzz_regressions \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_lifecycle_metadata_evidence_is_mounted_in_public_harnesses \
  -- --exact --nocapture

# Fresh default-feature program and fixture SBF builds from this worktree.
cargo build-sbf --tools-version v1.52 --sbf-out-dir "$PWD/target/deploy" -- --locked
CARGO_TARGET_DIR=/dev/shm/astra-ultra-inv063-089-auth-target-20260916222942 \
  cargo build-sbf --tools-version v1.52 \
  --manifest-path tests/fixtures/auth_matcher/Cargo.toml \
  --sbf-out-dir "$PWD/tests/fixtures/auth_matcher/target/deploy" -- --locked

# Restored witness: 1 passed, 0 ignored; public 5782-asset/14-leg/28-source exit.
PERCOLATOR_FUZZ_SBF="$PWD/target/deploy/percolator_prog.so" \
  cargo test --locked --offline --test v16_cu \
  inv_077_bounded_work_and_maximum_shape_compute::v16_attack_public_10m_market_max_source_owner_exit_stays_bounded \
  -- --exact --nocapture

rustfmt --check --edition 2021 --config skip_children=true \
  tests/invariants/public_sbf/inv_079_lifecycle_evidence_mounts.rs
git diff --check
```

The restored public witness measured `RebalanceReduce` at **1,179,018 CU**;
post-growth refresh peaked at 826,264 CU over 30 calls, ResetPending cleanup at
542,493 CU, FinalizeResetSide at 3,119 CU, and subsequent exits at 682,423 CU.
This is a rerun of existing behavioral evidence, not new CU coverage.

Fresh artifact SHA-256:

- Program: `87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.
- Auth matcher: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

## Integration and Rejected Duplicates

Safe for main as tests/docs only, subject to normal integration checks. All
temporary witness edits are restored; the user checkout was never edited or
switched. No open PR or issue implementation was consulted or copied.

The recent INV-070 row424 terminal-scan binding and INV-089 activation-fee CPI
rollback were excluded. Main already covers competing close landing orders,
atomic finalization handoff, cure replay and canceled-debt resolution, so those
small-state variants were rejected as duplicate work. The prior INV-079 mount
increment explicitly left function-level availability open; this increment
addresses that separate gap without claiming a new lifecycle theorem.
