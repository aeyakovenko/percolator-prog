# Rows 425/426 regression health, 2026-09-17

Base: freshly fetched `origin/main`, `254d6da40d6d03f20bac44ead141ab44caf05660`.
Worktree: `/dev/shm/percolator-row425426-health-20260917`.
Branch: `worker/row425426-health-20260917`; local commit only, no push.
Inputs: [README](README.md), [mark-movement audit](mark_movement_audit_20260917.md),
[INV-020 evidence audit](inv_020_current_observation_evidence_audit_20260916.md),
and [row-422 health follow-up](row422_regression_health_20260917.md).

## Diagnosis and added coverage

**No production issue was confirmed.** The existing
[chunked-observation selector](cu/inv_020_chunked_observation_admission.rs)
failed on rebuilt current-main SBF: after committing 32 logical slots of a
64-slot catch-up, its slot-0 Hybrid report cannot finish stale account health.
The original failure surfaces at the assertion allowing rejection only for the
second portfolio (old line 296). The extension explicitly verifies that the
first portfolio rejects with `EngineNonProgress` and exact Account rollback.
This matches `reject_incomplete_asset_health_observation_view` and the row-422
diagnosis: an otherwise fresh report from a prior slot cannot certify current
Hybrid health. No production guard was relaxed.

The existing selector already combines two price directions, both hint orders,
and four rotations of single/batch CPI/bilateral reductions. It now distinguishes:

- Slot 64: after a committed 32-step prefix, complete hints with the retained
  slot-0 report reject without losing that prefix or its fractional carry.
  A current same-price Hybrid report with the AuthMark hint omitted also rejects,
  undoing the second Hybrid prefix and newly supplied provenance. Complete
  current evidence then finishes both portfolios' health.
- Slots 65/66: prior-slot reports successfully advance sub-atom market carry;
  both portfolio Accounts stay byte-identical and `last_good_oracle_slot` stays
  in the prior slot. Same-price current reports subsequently renew provenance.
- Slot 67: after three committed reductions through rotated routes, the next
  whole price atom would stale health. Prior-slot evidence now rejects, restoring
  market, portfolios, custody, owner/admin and provider Accounts exactly. The
  retained slot-66 carry is 8,400; current evidence advances it to price movement
  16 and carry 800, preserving the surviving owners' extra 800-atom claim.

All eight per-world health checks now require exact agreement with the independent
raw-state certificate model immediately after refresh. Original input-derived
price/carry, per-owner value, OI, zero funding/insurance, mint supply, final flattening
and two 1,000-atom SPL withdrawals remain. Final per-owner values before withdrawal
are `1,000,000 +/- 20,100`; this selector does not withdraw all remaining value.

This adds a missing assertion dimension to an existing mounted witness. The
original row-425 AuthMark carry case lacks Hybrid currentness; row-426's rescue
case lacks interleaved reductions and fractional carry; equal-composite evidence
tests component replacement rather than committed chunked refresh plus trades.
The prior chunked selector had no explicit stale-report rollback at either the
32-step boundary or the post-trade whole-atom boundary, and its stale continuation
prevented reaching those downstream claims on this base.

## Exact execution

Only the touched selector ran. No broad suite, metadata census or Kani run.

| Execution | Result |
| --- | --- |
| Unmodified baseline | FAIL: 0 passed, 1 failed, 1,441 filtered; 0.43s |
| Extended selector on merged main | PASS: 1 passed, 0 failed, 1,442 filtered; 9.40s |
| Bounded histories | 16 worlds; 32 stale rollbacks, 16 omission rollbacks, 32 carry-only steps, 128 independent certificates |
| Peak successful CU | Crank 297,139; trade 300,395 |

Private host/SBF target directories were copied from the existing
`percolator-public-gap-20260916-c91e` targets. SBF was rebuilt locked/offline
against current source and engine `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
Wrapper SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
The ignored auth-matcher deployment was copied from main; SHA-256:
`50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
No external matcher is needed. Logs: `/dev/shm/percolator-row425426-health-20260917-logs/`
(`build-sbf.log`, `baseline.log`, `extended.log`). The pre-existing Solana
future-compatibility warning remains.

Commands from this worktree:

```bash
cp -a --reflink=auto /dev/shm/percolator-public-gap-20260916-c91e-host-target /dev/shm/percolator-row425426-health-20260917-host-target
cp -a --reflink=auto /dev/shm/percolator-public-gap-20260916-c91e-sbf-target /dev/shm/percolator-row425426-health-20260917-sbf-target
mkdir -p /dev/shm/percolator-row425426-health-20260917-logs tests/fixtures/auth_matcher/target/deploy
cp /dev/shm/percolator-main-merge-20260916/tests/fixtures/auth_matcher/target/deploy/auth_matcher.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/percolator-row425426-health-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row425426-health-20260917-sbf-target/deploy/percolator_prog.so
CARGO_TARGET_DIR=/dev/shm/percolator-row425426-health-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row425426-health-20260917-sbf-target/deploy -- --locked
# Same selector before and after the test extension; logs baseline.log / extended.log.
cargo test --locked --offline --test v16_cu inv_020_authenticated_clock_slot_and_oracle_provenance::chunked_observation_admission::v16_program_chunked_mixed_observations_gate_full_refresh_and_preserve_claims -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true tests/invariants/cu/inv_020_chunked_observation_admission.rs
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_020_chunked_observation_admission.rs
git diff --check
git diff --exit-code 254d6da40d6d03f20bac44ead141ab44caf05660 -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)**/*.tsv' ':(glob)**/*419*' ':(glob)**/*435*'
git diff --cached --check
git show --check HEAD
```

Formatting, whitespace, commit and protected-path checks pass. Only this Rust
test, this note and the short README entry change. Production, dependencies,
tracked fixtures, every TSV and row419/435 files are unchanged.

## Open gaps

Rows 425/426 remain `COVERED`; no status promotion is made. Current machine
statuses remain INV-020/062 `OPEN_EVIDENCE`, INV-045 `REFUTED_CURRENT`, and
INV-053/056 `SUPPORTED`, with `SAMPLED / GLOBAL_CONDITIONAL_TCB` scope.
The new checks support current observation evidence, carry preservation and
health equivalence; they do not close INV-045 reward provenance or INV-062
common-control extraction. Nonzero funding/fees, liquidation/rewards, common-owner
comparisons, arbitrary histories, multiple Hybrid assets, provider protocols,
maximum-shape composition and full terminal payouts remain outside this run.
Clock/provider reports remain external fixtures; protocol and custody state is
constructed and advanced through public System/SPL/wrapper routes.
