# Row 427 / INV-058 regression health, 2026-09-17

Base: freshly fetched `origin/main`, `36c7231792c9d8928fcb1f9fd9b9b8b67a91c4bc`.
Worktree: `/dev/shm/percolator-row427-health-20260917`.
Branch: `row427-health-20260917`; local commit only, no push.
Inputs: [frontier audit](inv_058_side_oi_frontier_audit_20260917.md) and
[README](README.md). Only the new exact selector ran; existing selectors were
not rerun. Production, dependencies, fixtures and status TSVs are unchanged.

## Added composition

[The new public LiteSVM/CU witness](cu/inv_058_competing_cross_zero.rs) combines
direct cross-zero with three unequal disjoint pairs sharing one side-OI cap.
It reuses the existing public System/SPL/wrapper/matcher construction and raw
position, side/count, certificate-notional, capital, supply and custody census.
There is no injected portfolio/market state or private engine execution.

Let `M = MAX_OI_SIDE_Q` and `q = M / 4`. Initial pair magnitudes are
`q`, `M / 3`, and `M - q - M / 3`. Both signs, both competing-pair orders and
all four flip transports run, with rotated release/competitor transports:

- At the cap, both the equal-magnitude flip and the `q -> -(q+1)` flip reject
  with `EngineInvalidLeg`, preserving every framed account except transaction
  fees. All requested sizes, individual positions and notionals remain admissible.
- A different pair releases `q+1`. A two-instruction bundle refills one atom
  into the third pair, then attempts the retained flip. The first wrapper call
  and all expected matcher calls succeed before the flip rejects. Full rollback
  restores the competing prefix, matcher state, positions, epochs and custody.
- The exact retained flip instruction then succeeds with `q+1` headroom,
  leaving `q` headroom. A second direct flip shrinks exposure by two atoms;
  the competing pair consumes all `q+2` headroom. Another atom rejects exactly.
- At the shared cap, a two-call close/reopen reverses the first pair while
  preserving its competitors. All three pairs subsequently close through their
  assigned transports, leaving zero OI and zero stored positions with exact stock.

Every submitted trade/bundle is checked against the 1,232-byte packet limit.
Accepted instructions are bounded by 345,000 CU each; rejection bundles use
the same per-instruction allowance and the harness's 1,400,000 transaction limit.
This adds the audit's missing competing-pair cross-zero dimension: earlier
cross-zero coverage used one pair, while generated disjoint-pair handoffs retained
their signs. It is not a new ADL, matcher-partial-fill or terminal-payout witness.

## Diagnosis and results

The first hypothesis, that a net-neutral direct flip succeeds at a fully shared
cap, failed (`cross-zero.log`, repeated with diagnostics in
`cross-zero-diagnostic.log`; each 0 passed / 1 failed, 0.48s). Source inspection
of pinned engine `4db11a8c` explains the boundary: the first account's new leg
is attached before the other account's old leg is removed, and
`add_open_interest_for_new_position` checks the transient side total.
This is a current admission boundary, not evidence that every bounded continuation
fails. The final witness executes both direct flips with sufficient transient
headroom and bounded close/reopen at the cap. No production issue was confirmed;
the initial test assumption was corrected without changing production.

An intermediate run failed before SBF execution when a borrowed artifact was
removed (`cross-zero-missing-artifact.log`, 0.12s). The final run uses a private,
locked/offline rebuild from this worktree, not the removed artifact.

Final result on the merged main worktree: **1 passed, 0 failed, 1,439 filtered**,
9.42s, exit 0.
Completed: **16 histories, 32 direct flips, 16 bounded split reversals,
64 exact rollbacks and 48 pair closes**. Peak rejection CU **309,383**;
peak accepted-trade CU **193,767**; maximum competition bundle **979 bytes**.
No custody payout instruction is exercised. Existing Solana future-compatibility
warnings remain. No broad suite, engine proof or metadata selector ran.

## Reproduction

Logs: `/dev/shm/percolator-row427-health-20260917-logs/`.
Private SBF SHA-256:
`a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
Unchanged auth-matcher SHA-256:
`50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
The host cache was copied before the source artifact disappeared. Commands below
are from this worktree, except worktree creation from the supplied main checkout:

```bash
git fetch origin main
git worktree add -b row427-health-20260917 /dev/shm/percolator-row427-health-20260917 origin/main
cp -a --reflink=auto /dev/shm/percolator-row420421423433-health-20260917-host-target /dev/shm/percolator-row427-health-20260917-host-target
cp -a --reflink=auto /dev/shm/percolator-public-gap-20260916-c91e-sbf-target /dev/shm/percolator-row427-health-20260917-sbf-target
mkdir -p /dev/shm/percolator-row427-health-20260917-logs tests/fixtures/auth_matcher/target/deploy
cp /dev/shm/percolator-main-merge-20260916/tests/fixtures/auth_matcher/target/deploy/auth_matcher.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/percolator-row427-health-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row427-health-20260917-sbf-target/deploy/percolator_prog.so
CARGO_TARGET_DIR=/dev/shm/percolator-row427-health-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row427-health-20260917-sbf-target/deploy -- --locked
sha256sum "$PERCOLATOR_FUZZ_SBF" tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
cargo test --locked --offline --test v16_cu inv_058_cumulative_position_oi_notional_and_rate_limit_integrity::atomic_oi_fee_handoff::multi_asset::competing_cross_zero::v16_program_direct_cross_zero_competes_for_shared_side_oi_headroom -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_058_competing_cross_zero.rs tests/invariants/cu/inv_058_multi_asset_oi_fee_handoff.rs
git diff --check
git diff --exit-code 36c72317 -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/*.tsv'
git diff --exit-code 36c72317 -- . ':!tests/invariants/README.md' ':!tests/invariants/row427_regression_health_20260917.md' ':!tests/invariants/cu/inv_058_competing_cross_zero.rs' ':!tests/invariants/cu/inv_058_multi_asset_oi_fee_handoff.rs'
git diff --cached --check
git show --format= --check HEAD
```

SBF build, targeted formatting, whitespace, committed-change and scope guards
pass. The parent Rust file changes only to mount the new module.

## Open gaps

Row 427 remains **OPEN**, `Conformance / LIMIT`; INV-058 remains `REFUTED_CURRENT`.
These finite, unit-ADL, fixed-mark, zero-fee/funding/PnL histories do not establish
nonunit/partial ADL rounding, partial matcher fills, elapsed liabilities, shared
owner graphs, Recovery composition, multiple capped assets or maximum shapes.
The batch routes carry one leg. No invariant status or historical witness claim
is promoted, and no extractable loss or persistent DoS is claimed.
