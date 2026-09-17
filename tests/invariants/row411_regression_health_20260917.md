# Row 411 fee/policy regression health, 2026-09-17

Base: current `origin/main`, `8fc4787c196002874366194de4211a7edac5fd7f`.
Worktree: `/dev/shm/percolator-row411-health-20260917`.
Branch: `codex/row411-regression-health-20260917`; local only, no push.
Scope: row 411 / INV-005/010/011/014/024/036/047/081 and the three failures in
the [fee/policy audit](fee_policy_routes_audit_20260917.md).

All three failures reproduce with freshly built current-main SBF. All are stale
test expectations; no production correction or missing economic setup is needed.
The existing selectors are repaired, without new tests or status changes.

| Selector below | Baseline | Correction and verified suffix |
| --- | --- | --- |
| F / PR223 | Capital delta -150, expected -120 | The public setup initializes the fee cursor at slot 1; submission is slot 6. Assert both cursors and the exact `30 * (6 - 1)` charge in the helper, with the same independent caller expectation. Unauthorized LP debit still rejects atomically, zero-cap reduction earns no further provider fee, and provider SPL withdrawal/custody checks now complete. |
| D / signed direction | Negative payout 1,000,001, expected 1,000,000 | The bankrupt short consumes opposite-side (long) insurance: 1,000,000 atoms for positive opening direction, one atom for negative. Assert exact spent counters on every route; negative payout is 1,000,001 and residual vault 2,000,000. Both signs across all four transports now complete and agree. |
| I / insurance schedules | Full account equality fails after economic checks | Each successful debit advances its asset authority epoch. Assert exact per-asset increments after every withdrawal, then normalize only those two fields in copied checkpoint accounts. Every other non-payer account byte matches. Live/Resolved debit counts are `[1,1]`/`[2,1]` coalesced versus `[2,2]`/`[5,2]` fragmented. Both exhaustion retries now verify typed rejection, exact unnormalized rollback and network fees; budgets remain `[0,0,12,19]`, vault 31. |

The relevant production rules are `collect_maintenance_fee_before_trade_view` in
`src/v16_program.rs`, the pinned engine's `sync_account_fee_to_slot_not_atomic`
and `consume_domain_insurance_for_negative_pnl`, and the wrapper's
`handle_withdraw_insurance_asset` epoch advance. The engine pin remains
`4db11a8cb0053815e23a35d3a7d3edc265d8d866`.

## Exact verification

Private host/SBF caches were copied from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target`.
Wrapper and both in-repo matchers were rebuilt locked/offline with platform-tools
v1.52. No old wrapper artifact was used. These selectors use the authenticated or
hostile matcher; they do not depend on the external `percolator-match` artifact.

```bash
export CARGO_TARGET_DIR=/dev/shm/percolator-row411-health-20260917-host-target
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row411-health-20260917-sbf-target/deploy/percolator_prog.so
CARGO_TARGET_DIR=/dev/shm/percolator-row411-health-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row411-health-20260917-sbf-target/deploy -- --locked
for matcher in auth_matcher hostile_matcher; do
  CARGO_TARGET_DIR=/dev/shm/percolator-row411-health-20260917-matcher-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/$matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/$matcher/target/deploy -- --locked
done
cargo test --locked --offline --test v16_program_fuzz_regressions --test v16_cu --no-run
F=inv_036_fee_destination_and_policy_version_integrity::v16_program_pr223_unsigned_lp_backing_fee_requires_matcher_consent
D=inv_036_fee_destination_and_policy_version_integrity::v16_program_signed_direction_route_matrix_preserves_side_attribution_and_terminal_value
I=inv_064_insurance_withdrawal_policy_equivalence::v16_program_insurance_withdrawal_schedules_preserve_asset_allowance_and_exact_retry
R=inv_014_delayed_policy_and_policy_epoch_safety::v16_retained_single_cpi_taker_fee_cap_rejects_policy_increase
P=inv_014_delayed_policy_and_policy_epoch_safety::v16_retained_fee_terms_bound_partial_and_exact_fill_routes_after_policy_change
# Baseline, then after the scoped test changes:
cargo test --locked --offline --test v16_program_fuzz_regressions "$F" -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 "$D" "$I"
# Final CU verification also includes two existing row-411 consent controls:
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 "$D" "$I" "$R" "$P"
git diff --check
git diff --exit-code 8fc4787c -- src Cargo.toml Cargo.lock tests/fixtures tests/invariants/*.tsv
git diff --cached --check
git show --format= --check HEAD
```

Logs: `/dev/shm/percolator-row411-health-20260917-*.log`.

| Log suffix | Result |
| --- | --- |
| `sbf-build`, `matcher-build`, `hostile-build`, `host-build` | PASS; existing host warnings only |
| `pr223-red` | FAIL 0/1, exit 101, delta -150 |
| `cu-red` | FAIL 0/2, exit 101, original payout/frame assertions |
| `pr223-check` | PASS 1/1, including fee cursors and provider withdrawal |
| `cu-check` | PASS 2/2; temporary diagnostic confirmed spent `[1000000,0]` / `[1,0]` on all routes, replaced by a permanent assertion |
| `cu-green` | PASS 4/4, including the permanent spent assertion and both row-411 controls |

Artifact SHA-256:
- Wrapper: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
- Authenticated matcher: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
- Hostile matcher: `e0c20fad34a7822cc6ce42a3c77ff08a8591977102f0c497a339d66a9dd6240a`.

## Kept open

Row 411 stays OPEN; row 432 and every invariant/benchmark status are unchanged.
R checks retained 99-bps taker consent across 19 -> 100 bps with independent LP
cap 137, exact rollback and fresh 100-atom fees. P checks permitted policy changes,
single-CPI partial fills, exact batch fills, batch-partial rejection and owner
payouts. These bounded controls do not close the full fee-policy/route product:
arbitrary retained histories, concurrent fee sources, clipping, expiry, lifecycle
composition and general INV-081 success validity remain open. No broad suites,
new source census, Kani or unrelated invariant selectors were run. Clock and
matcher fixtures retain their existing assumptions. This supersedes only the
three failing-witness health results in the earlier audit.
