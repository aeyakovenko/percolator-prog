# Row 422 regression health, 2026-09-17

Base: `origin/main` at `8fc4787c196002874366194de4211a7edac5fd7f`.
Worktree: `/dev/shm/percolator-row422-health-20260917`.
Branch: `codex/row422-regression-health-20260917`; local commit only.
Scope: row 422 / INV-020/024/036/041/045/061/062, the two failures in the
[mark-movement audit](mark_movement_audit_20260917.md) only.

## Diagnosis and corrections

Both unchanged selectors reproduce on current main. **No production issue was
confirmed.** Wrapper source, engine pins, lockfile and status ledgers are unchanged.

- **Paid-origin handoff: stale expectation.** Keeper value is 1,000, not 2,995.
  `liquidation_penalty_reclaimable_from_profile_view` requires authenticated
  effective-price provenance; `canonical_accrual_path_for_target_view` restores
  that provenance only on fresh-target catch-up. Here the paid effective price
  remains 997,600 and lags the fresh target. The existing helper now expects zero
  reward, asserts trade-driven provenance and zero domain budgets, and retains
  positive penalty/foregone-share, exact owner-value, rollback and SPL assertions.
  The historical selector name is retained.
- **Selected-asset catch-up: missing current-slot report.** The original target
  crank fails with `InstructionError(2, Custom(22))`, 52,873 transaction CU.
  `reject_incomplete_asset_health_observation_view` requires
  `last_good_oracle_slot == now_slot` for a fresh-mode Hybrid health refresh.
  Reusing the slot-2 report advances the price to 992,800 at slot 4 but cannot
  refresh the target's health. The test now asserts this rejection and exact
  tracked-account rollback in both hint orders, then supplies a same-price report
  published at time 103 in slot 4. All original positive reward, selected-domain,
  order-equivalence, owner-exit and keeper-withdrawal assertions remain.

## Exact results

| Execution | Result |
| --- | --- |
| Unmodified two-selector baseline | FAIL: 0 passed, 2 failed, 1,437 filtered; 0.92s |
| Corrected same two selectors | PASS: 2 passed, 0 failed, 1,437 filtered; 9.48s |
| Paid-origin handoff | 16 worlds, 32 late rollbacks, 16 liquidation-prefix rollbacks; zero reward, 1,000 keeper payout; peak transaction 298,170 CU |
| Selected-asset catch-up | 2 hint orders; each charges 5,899, rewards 1,966, retains 3,933, pays keeper 2,966; peak crank/exit/withdrawal 323,548 / 146,602 / 50,604 CU |

Only these exact selectors ran, including temporary diagnostic reruns. Diagnostics
were removed. One intermediate host compile needed a qualified constants import.
Targeted Rust formatting, whitespace and production/dependency diff checks pass.
The existing Solana future-compatibility warning remains.

## Reproduction and artifacts

The old audit SBF uses engine `94979ede` and is unsuitable for current main.
Private host/SBF targets were copied with `cp -a --reflink=auto` from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target` to
`/dev/shm/percolator-row422-health-20260917-{host,sbf}-target`. The wrapper was
rebuilt locked/offline with the existing v1.52 toolchain and current engine
`4db11a8cb0053815e23a35d3a7d3edc265d8d866` from Cargo's local cache.
Wrapper SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
The auth matcher was copied from main's fixture deployment to the same relative
path here; SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
No external matcher is needed by these selectors. Logs are in
`/dev/shm/percolator-row422-health-20260917-logs/` (`build-sbf.log`,
`baseline.log`, diagnostic logs, `green.log`).

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/percolator-row422-health-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row422-health-20260917-sbf-target/deploy/percolator_prog.so
CARGO_TARGET_DIR=/dev/shm/percolator-row422-health-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row422-health-20260917-sbf-target/deploy -- --locked
# Same command before and after the test corrections:
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::v16_program_paid_discovery_fresh_handoff_authenticates_liquidation_and_keeper_exit \
  inv_045_no_free_mark_movement::v16_program_caught_up_hybrid_reward_uses_selected_asset_provenance_in_both_hint_orders
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_045_authenticated_reward_handoff.rs tests/invariants/cu/inv_045_no_free_mark_movement.rs
git diff --check
git diff --exit-code 8fc4787c -- src Cargo.toml Cargo.lock tests/invariants/invariant_status.tsv tests/invariants/coverage_reopenings.tsv
git diff --cached --check
```

## Kept-open gaps

Row 422 remains **OPEN**; INV-045 remains `REFUTED_CURRENT`, and the six other
scoped invariants remain `OPEN_EVIDENCE`. This is bounded regression repair, not
invariant closure. The helper's separate nonzero-funding selector was not rerun.
Arbitrary paid-origin/catch-up histories, the funding/maintenance/policy/recipient
product, other transports and maximum-shape composition remain outside this run.
Clock and external reports are fixtures, not executed provider publication
protocols. No broad suite or Kani proof ran. Reproduction needs the pinned engine
commit available locally, as documented by the row-417 pin audit.
