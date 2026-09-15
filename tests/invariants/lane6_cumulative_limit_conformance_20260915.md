# Lane 6 Cumulative-Limit Conformance

## Scope and Provenance

- Base: `origin/codex/astra-invariant-cycle-20260915`, resolved to
  `e1394241e46ba1d66e21b43f6c1c99d83761dcca` before isolation.
- Private clone: `/tmp/percolator-lane6-inv058-20260915`.
- Branch: `codex/lane6-inv058-cumulative-20260915`.
- Engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Default Anchor-v2 features; SBF platform-tools v1.52; locked, offline builds.
- Private host/SBF targets: `/dev/shm/lane6-inv058-host` and
  `/dev/shm/lane6-inv058-sbf`, initially copied from Lane 4 dependency caches.
  Cargo rebuilt the wrapper and both test harnesses from this private source tree.

The user's dirty checkout was read only. No production, dependency, or machine
status file changed. Changed files are this report, `README.md`, and
`cu/inv_058_cumulative_position_oi_notional_and_rate_limit_integrity.rs`.

Fresh artifact SHA-256 values:

| Artifact | SHA-256 |
| --- | --- |
| Wrapper | `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f` |
| Auth matcher | `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93` |
| Hostile matcher | `e0c20fad34a7822cc6ce42a3c77ff08a8591977102f0c497a339d66a9dd6240a` |

## Diagnosis

On untouched `e1394241`, the exact selector
`v16_program_recreated_counterparty_preserves_post_transition_cumulative_limits`
fails at its expected-success transfer:

```text
Cpi taker=1 maker=2 legs=[(0, -99999999999999)]
InstructionError(3, Custom(18))
custom program error: 0x12
compute_units_consumed: 103437
test result: FAILED. 0 passed; 1 failed; 0 ignored
```

Here `Custom(18)` is `EngineInvalidLeg`. The matcher succeeds first. The failing
direction is `-1`: actor 0 is short `M - 1`, actor 1 is long `M - 1`, and actor 2
is flat, where `M = MAX_OI_SIDE_Q = MAX_POSITION_ABS_Q = 100000000000000`.
The transfer would flatten actor 1 and give actor 2 the same long quantity.
Its scalar size, both final account positions, final side OI, and notional are
within their bounds. This is not a malformed or state-injected position fixture.

The engine's `src/v16.rs::apply_trade_after_refresh_not_atomic` applies the
positive-delta account first. For this transfer that is actor 2. Its Attach route
calls `attach_leg_at_slot`, `kernel_attach_leg`, then
`add_open_interest_for_new_position`. That last function checks the new long OI
before actor 1's long reduction. It observes
`2 * (M - 1) = 199999999999998 > M` and returns `InvalidLeg`.
The wrapper maps the error and SVM rejects the transaction. Its final
`ensure_trade_side_oi_cap_view` postcondition is never reached.

Thus the old expected-success setup is incompatible with the current engine's
intermediate attach guard, despite having an admissible intended final state.
This is an over-restrictive transfer ordering, not evidence of an accepted
cumulative-limit overflow. This patch does not establish acceptance equivalence
for all in-bound transfer schedules and does not repair the large-first route.

## Smallest Public Continuation

Reverse only the existing two setup fragments from `[M - 1, 1]` to `[1, M - 1]`.
The one-atom transfer attaches actor 2's leg with transient OI at most two.
The later transfer resizes that existing leg, then reduces the donor; the wrapper
checks the final side-OI bound. The committed state is the same: actor 0 at one
ceiling, actor 1 flat, actor 2 at the opposite ceiling. Both signs and every
transport complete without adding transactions or weakening any assertion.

The preserved input-derived ledger checks positions, side OI, ceil notionals,
health certificates, capital, zero PnL, custody, and stock/encumbrance censuses
after every trade. The survivor remains capped across withdrawal, close, public
System funding, initialization at the same address with a new ID, and redeposit.
Fresh cap-plus-one attempts still reject in both account roles on all four
transports. Two-asset batch rollback, two released atoms consumed exactly once,
cross-zero, opposite-cap refill, and final flatness all execute.

There are no added program-owned byte writes. The existing `V16Svm` bootstrap
supplies zeroed program storage and SPL fixtures, then initializes economic state
through public instructions. Every changed setup step is a public trade, and
the trace validates public execution and rollback. Same-address recreation still
uses LiteSVM's retained zero-lamport account storage plus public System funding;
runtime account purge and recreation of purged storage are not exercised.

## Commands and Evidence

Isolation used `git clone --shared --no-checkout /home/anatoly/percolator-prog
/tmp/percolator-lane6-inv058-20260915`, then `git switch -c
codex/lane6-inv058-cumulative-20260915 e1394241e46ba1d66e21b43f6c1c99d83761dcca`.
All remaining commands run in that clone. Build commands:

```sh
export CARGO_TARGET_DIR=/dev/shm/lane6-inv058-sbf CARGO_BUILD_JOBS=4
cargo build-sbf --tools-version v1.52 --sbf-out-dir "$PWD/target/deploy" -- --locked --offline
cargo build-sbf --tools-version v1.52 --manifest-path tests/fixtures/auth_matcher/Cargo.toml --sbf-out-dir "$PWD/tests/fixtures/auth_matcher/target/deploy" -- --locked --offline
cargo build-sbf --tools-version v1.52 --manifest-path tests/fixtures/hostile_matcher/Cargo.toml --sbf-out-dir "$PWD/tests/fixtures/hostile_matcher/target/deploy" -- --locked --offline
export CARGO_TARGET_DIR=/dev/shm/lane6-inv058-host CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF="$PWD/target/deploy/percolator_prog.so"
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
module=inv_058_cumulative_position_oi_notional_and_rate_limit_integrity
selector=${module}::v16_program_recreated_counterparty_preserves_post_transition_cumulative_limits
cu=/dev/shm/lane6-inv058-host/debug/deps/v16_cu-8c7475d0975fcbd3
"$cu" "$selector" --exact --nocapture
```

The last command ran before edits, exited 101, and was captured with `tee` and
`pipefail` in `target/lane6-baseline.log`. After the fragment-order repair:

```sh
cargo test --locked --offline --test v16_cu "$selector" -- --exact --nocapture
"$cu" inv_058_ --test-threads=3 --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_ -- --test-threads=3 --nocapture
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_058_cumulative_position_oi_notional_and_rate_limit_integrity.rs
git diff --check
```

Exact repaired selector: **1 passed, 0 failed, 0 ignored**. Eight worlds complete
**250 public transactions and 80 exact rejections**. Peak successful CU per
history transport, taking the maximum across signs: NoCpi **177,749**, Cpi
**175,272**, BatchNoCpi **212,877**, BatchCpi **232,878**. These are history maxima,
not isolated route benchmarks; each is below the 1,400,000-CU transaction ceiling.
The exact run is recorded in `target/lane6-repaired.log`.

| Verification | Exact result | Evidence |
| --- | --- | --- |
| Untouched baseline exact selector | `0 passed; 1 failed; 0 ignored`, exit 101 | `target/lane6-baseline.log` |
| Repaired exact selector | `1 passed; 0 failed; 0 ignored`, exit 0 | `target/lane6-repaired.log` |
| Entire INV-058 family | `18 passed; 0 failed; 0 ignored`, exit 0 | `target/lane6-inv058.log` |
| INV-079 evidence/status gates | `17 passed; 0 failed; 0 ignored`, exit 0 | `target/lane6-metadata.log` |
| Scoped rustfmt and `git diff --check` | Both exit 0 | Commands above |

The family run includes disjoint-owner side-OI admission, generated existing-leg
composition, fee competition, PnL/terminal handoff, liquidation/reset, source
capacity, split fills, and reduction/cross-zero controls. The metadata run
includes the authoritative status, charter/index, reopening, benchmark, and
public-trace checks. The full repository suite was not run. Builds retain the
existing Solana 1.18 future-compatibility and unused/deprecated fixture warnings.

## Row Verdict

**Row 427 remains OPEN.** `coverage_reopenings.tsv` and
`invariant_status.tsv` retain `OPEN` and `INV-058 REFUTED_CURRENT`, respectively.
The row is a `Conformance / LIMIT` finding, absent from the security-only
`open_findings.tsv`. No new security severity or production correction is claimed.

This restores bounded recreation coverage. Its overflow attempts also exceed an
account cap, so they do not independently isolate shared side-OI enforcement.
The disjoint-pair owners supply that separate evidence. Arbitrary histories,
nonunit ADL, rate/maintenance boundaries, maximum-price notional boundaries, and
combined maximum shapes remain outside this selector. The old README's supplied-
artifact pass is superseded for current-pin reproducibility by this fresh-build
baseline failure and repaired public history.
