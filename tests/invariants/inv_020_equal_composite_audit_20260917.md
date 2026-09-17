# Equal Composite Provenance Conformance

Base: `origin/main` at `fee828705731347b76febecd682263cc8bdcea02`.
Branch: `astra-ultra/oracle-mark-evidence-20260917`.
Worktree: `/dev/shm/percolator-astra-oracle-mark-20260917`.

The owner is `cu/inv_020_equal_composite_provenance.rs`, mounted as
`inv_020_authenticated_clock_slot_and_oracle_provenance::equal_composite_provenance`.
This adds bounded public-workflow evidence for INV-020/045/053/054. INV-056 was
reviewed for overlapping observation coverage; this test makes no new claim about
hint membership, omitted observations, or whole-invariant closure.

## Evidence

All market, portfolio and custody construction uses System, SPL, ATA and wrapper
instructions. Clock, signer SOL and provider-owned reports are external fixtures;
the provider publication programs are not executed. No initialized protocol bytes
are edited, restored or fabricated. All six worlds hold one matched unit of live
exposure. Three cyclic provider orders cross base and secondary oracle profiles.

The component vectors `[7200000, 2000000, 3000000]` and
`[14400000, 4000000, 3000000]` both produce price 120 after division by components
two and three and unit scaling. The existing bigint rational model checks that
collision independently. Each slot rejects the alternate vector at the accepted
timestamp, accepts it at a newer timestamp, then rejects the former vector under
that new timestamp. This requires actual component provenance replacement even
though the output price is unchanged. All 36 rejections preserve full economic
and provider Accounts, including lamports; the network fee payer is excluded.

Movement is derived directly from `100 * 37 * elapsed / 10000`, with the remainder
checked separately. Across slots 2/3/4, effective price is 100/100/101 and carry is
3700/7400/1100. Fresh same-slot component replacement must preserve the entire
engine asset. A report replay in the next slot may advance settlement but cannot
renew `last_good_oracle_slot`. Caller slot values alternate between zero and
`u64::MAX`; authenticated Clock determines progress.

The initial target change stales the untouched trader's certificate. Public
refresh records short equity 1000/1000/999 and margin 120/120/120. Eighteen current
certificates match every lane and key of the independent raw-state model. At the
middle slot, K/F and raw/effective prices have not changed, so a redundant account
refresh must return `EngineNonProgress` with six additional exact rollbacks.
The passive counterparty stays byte-identical, SPL balances remain exact, and
matched OI and zero funding are checked throughout. There is no terminal route.

## Non-Duplication

- `v16_program_composite_epoch_coherence_crosses_all_providers_and_transforms`
  checks skew, rewind and coherent newer timestamps with fixed component prices.
  It does not exercise distinct component vectors with equal composed output.
- `v16_program_crank_oracle_same_publish_time_price_change_rejects` changes a
  single feed's output. An aggregate-only comparison would satisfy that test but
  fail the new equal-output component checks.
- `v16_program_unchanged_oracle_report_cannot_renew_withdrawal_window` distinguishes
  repeated reports from newer same-price reports, but does not replace components
  or carry fractional mark movement through those replacements.
- INV-045's `public_carry_order::target_arrival_entitlement` tracks AuthMark
  target/plateau/restart arithmetic. It does not commit external component
  provenance while preserving the same raw target and fractional carry.
- INV-053's rounded nontraded lag and INV-054's target-only/disjoint/fee-only
  invalidation tests cover admission and refresh after economic state changes.
  This test also requires exact certificate reuse when only provider provenance
  and sub-atom carry change, followed by refresh after a whole-atom loss.
- INV-056's membership and mixed-observation tests vary discovery sets and tails.
  This test always supplies all three configured components.

The requested README, traceability gaps and relevant CU/public-SBF/stateful/Kani
owners were inspected. The traceability file is explicitly nonexhaustive; its
INV-054 fee-only admission row does not describe this composition. No open PR
diffs, active worker diffs or withheld repro bodies supplied the test. The initial
candidate of ordinary freshness/fee-only coverage was discarded before editing
because existing selectors already cover it. No production failure was found.

## Exact Validation

`src`, `Cargo.toml` and `Cargo.lock` match the supplied artifact's source worktree
`/dev/shm/percolator-public-gap-20260916-c91e` byte for byte. They also have no
changes relative to this branch's base. Reused SBF SHA-256:
`87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.
A private copy of the existing host target cache avoids dependency rebuilds and
shared-target writes. No SBF rebuild or unfiltered suite was run.

```bash
cd /dev/shm/percolator-astra-oracle-mark-20260917
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
export CARGO_TARGET_DIR=/dev/shm/percolator-astra-oracle-mark-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-public-gap-20260916-c91e-sbf-target/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu inv_020_authenticated_clock_slot_and_oracle_provenance::equal_composite_provenance::v16_program_equal_composite_refresh_preserves_component_provenance_and_mark_carry -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --exact inv_020_authenticated_clock_slot_and_oracle_provenance::v16_program_composite_epoch_coherence_crosses_all_providers_and_transforms inv_020_authenticated_clock_slot_and_oracle_provenance::v16_program_crank_oracle_same_publish_time_price_change_rejects inv_053_full_health_recertification_equivalence::v16_program_rounded_nontraded_lag_full_refresh_preserves_exact_trade_boundary --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_every_public_invariant_source_and_declared_test_is_mounted -- --exact --nocapture

rustfmt --edition 2021 --check --config skip_children=true tests/invariants/cu/inv_020_authenticated_clock_slot_and_oracle_provenance.rs tests/invariants/cu/inv_020_equal_composite_provenance.rs
git diff --check
```

The new selector passes: **1 passed, 0 failed, 0 ignored**, six worlds, 36
provenance rollbacks, six current-health no-op rollbacks, 18 certificate checks,
and **104,834 peak CU** under the existing 325,000 crank limit. The initial
development run expected progress at the already-current middle slot and failed
with `EngineNonProgress`; the final test explicitly requires that result and
complete rollback. No production assertion or error code was relaxed.

The adjacent exact-selector invocation passes **3 passed, 0 failed, 0 ignored**.
The composite control covers 126 configurations, 114 skew rejections and 126
rewind rejections. The rounded-lag control reports three exact rejections and six
exact certificate comparisons. The mount census passes **1 passed, 0 failed,
0 ignored**, discovering 503 source files and 1,907 available test declarations,
including this new file and selector. Scoped rustfmt and `git diff --check` pass.
Only existing unused-test-support and Solana future-compatibility warnings appear.
