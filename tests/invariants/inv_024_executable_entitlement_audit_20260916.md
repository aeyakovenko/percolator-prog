# INV-024 executable entitlement evidence audit

Base: remote main `cc3b1a502583253a84bdead6a19717680b3e76fd`.
Scope: INV-024 through INV-044; this increment changes only INV-024 evidence
validation. The worktree and its private build output are under `/dev/shm`.

## Gap and Change

The entitlement-effect roster names 18 witnesses for 49 public instructions.
Its old guard accepted a witness's `#[test]` text without checking that its file
was mounted in a test target or that the test was enabled. Disabling INV-024's
entire stateful owner, or ignoring its multi-episode entitlement witness, left
the guard green while removing the roster's per-owner trade-history evidence.

The existing selector
`inv_024_attributed_quote_value_conservation::v16_program_entitlement_effect_roster_is_source_complete`
now parses Rust syntax, requires each owner to have an unconditional external
`#[path]` mount in one of the three current integration targets, and requires an
unconditional, non-ignored top-level `#[test]`. It also compares the roster
directly with the production instruction enum, retaining the existing public
registry comparison for tags. The `syn` dev dependency uses the already locked
2.0.117 package; no dependency versions change.

This gap was identified from the invariant and evidence sources before comparing
the open-finding benchmark. Existing attribution findings already have discovery
mappings, so this adds no discovery claim or benchmark/status reclassification.
It protects the general entitlement roster rather than duplicating a finding's
public trace or importing its production fix.

## Negative Controls

Both controls modify only the isolated worktree and were restored exactly.

| Temporary mutation | Original guard | Strengthened guard |
| --- | --- | --- |
| Add `#[cfg(any())]` to the INV-024 mount in `tests/v16_program_stateful_fuzz.rs` | 1 passed | 1 failed: owner not unconditionally mounted |
| Add `#[ignore]` to `v16_program_multi_episode_history_enforces_each_owners_exact_entitlement` | 1 passed | 1 failed: missing unconditional, non-ignored test |

The guards read witness and target source from disk. Each mutation was checked
with an actual compiled CU test binary and the same exact selector, without
running or counting the disabled witness:

```bash
/dev/shm/astra-ultra-value-20260916-logs/v16_cu-baseline inv_024_attributed_quote_value_conservation::v16_program_entitlement_effect_roster_is_source_complete --exact --nocapture
/dev/shm/astra-ultra-value-20260916-target/debug/deps/v16_cu-2aa61b67cb770766 inv_024_attributed_quote_value_conservation::v16_program_entitlement_effect_roster_is_source_complete --exact --nocapture
```

## Final Verification

Run from `/dev/shm/astra-ultra-value-20260916`:

```bash
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-value-20260916-target
export TMPDIR=/dev/shm/astra-ultra-value-20260916-tmp
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0

cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::v16_program_entitlement_effect_roster_is_source_complete -- --exact --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_public_instruction_coverage_registry_matches_production_roster -- --exact --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_public_instruction_coverage_registry_points_to_executable_evidence -- --exact --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_dated_open_security_finding_benchmark_is_non_overclaiming -- --exact --nocapture
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_024_attributed_quote_value_conservation.rs
git diff --check
```

Each Cargo selector runs **1 test, 1 passed, 0 failed, 0 ignored**: four passing
metadata tests total. The two intentional mutation failures are separate
negative controls, not passing coverage. Logs are in
`/dev/shm/astra-ultra-value-20260916-logs`. No zero-test run is counted.

## Limits and Integration

Safe for main integration as a coverage-only change: production code, engine
pin, economic test bodies, discovery mappings, and invariant statuses are
unchanged. No new public-route behavior mismatch or LiteSVM execution is claimed.

The guard accepts the current direct, unconditional module layout. Nested or
conditional evidence needs an explicit guard update; this is not a general Rust
configuration evaluator. It establishes evidence presence, not the semantic
adequacy of a named oracle. Arbitrary retained-policy, insurance/backing,
reservation, rounding, domain-isolation, and terminal histories remain outside
this increment, as do executable-mount checks for other invariant rosters and
the benchmark's individual discovery mappings. No whole-invariant closure is
claimed for INV-024 through INV-044.
