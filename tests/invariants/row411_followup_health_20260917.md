# Row 411 retained partial-fill authority return, 2026-09-17

Base: freshly fetched `origin/main`, `1e3e2a4b4bf44df56b2b54f1120a7ae4ebebfd06`.
Worktree: `/dev/shm/percolator-row411-followup-health-20260917`.
Branch: `row411-followup-health-20260917`; local commit only, no push.
Primary INV-014; related INV-005/010/011/024/036/047 evidence.

No production issue was confirmed. This is test/documentation coverage;
row 411 remains **OPEN**, with no invariant or benchmark status changes.

## Distinct composition

Inputs: the [README](README.md), [fee/policy route audit](fee_policy_routes_audit_20260917.md),
[stale-expectation repairs](row411_regression_health_20260917.md),
[generated partial-policy audit](pr135_scope_m_single_cpi_fee_consent_20260913.md),
[funded policy return audit](astra_scope_j_retained_policy_admission_20260914.md),
and retained aggregate-cap/expiry notes and their mounted witnesses.

| Existing evidence | Increment here |
| --- | --- |
| `inv_014_generated_partial_policy_words.rs`: actual partial execution, single-route continuations, fixed authority epoch | All four closing transports after authority return, including batch CPI's aggregate atom cap |
| `inv_014_retained_fee_authority_epoch.rs`: A -> B -> A, stale policy prefix/suffix, full fills | Same policy-epoch distinction after committed partial success, with consumed opening/closing retries and complete payouts |
| `inv_014_retained_mixed_route_fees.rs`: full CPI/bilateral/CPI history and grant renewal | Partial opening plus retained close after authority return; no grant renewal |
| Scope J / retained aggregate-cap / expiry notes | No prior partial-fill plus authority-return plus four-route continuation product; no new backing, expiry or reserve-withdrawal claim |

New file: `cu/inv_014_retained_partial_authority_routes.rs`, mounted as a child
of `generated_partial_policy_words` to reuse its unchanged public `World`,
independent `Book`, complete rollback frame and simulation accounting.
The parent changes only by adding this module mount.

## Assertions and result

Eight worlds cross both position signs with single/batch, CPI/no-CPI closes.
Every world commits a real 95/255 matcher partial at 37 bps, then retains five
close envelopes before A -> B -> A. Temporary authority B installs 71 bps;
the LP's separate 137-bps grant permits that policy throughout.

- Authority epochs advance exactly twice; the complete authority profile returns
  to its original value. Portfolio positions and already-earned fees are framed.
- A's retained policy has a still-future sequence (`+7`) but an obsolete epoch.
  Its prefix rejects `EngineStale`; changing only that epoch allows the policy
  plus close in simulation, isolating the stale authorization boundary.
- Retained 37-bps single/direct terms and batch CPI's equivalent atom cap reject
  the higher policy with `InvalidInstruction`. Only batch CPI reaches its matcher
  before this rejection, and the entire matcher/account frame rolls back.
- After public restoration to 37 bps, a retained close succeeds inside a bundle
  before its stale policy suffix rejects. Logs require the successful close;
  complete Accounts, custody, epochs, request sequence and fees roll back.
- A separately pre-signed close with identical trade bytes then commits. The
  original partial-opening retry and a consumed-close alternative both reject
  `EngineStale`, preserving instruction-local consent and episode binding.
- The input-derived ceiling-fee ledger checks 36 atoms per owner per fill,
  72 total per owner, insurance domains `[72,72]`, exact payouts
  `[100044,199935]`, and residual vault 144. All four routes agree economically.
  Exact SPL Account bytes, fixed mint supply, positions/OI, grant state, request
  counts, stock and reservation censuses are checked through both payouts.

PASS: **1 exact selector**, 8 worlds, 40 permitted/8 denied simulations,
40 delivery rollbacks, 16 committed fills, 16 owner payouts; **196,695 peak CU**.
Runtime 4.06s, 1,441 unrelated tests filtered out. No broad suite was run.

The first development run failed with LiteSVM `AlreadyProcessed` when attempting
to simulate an already-delivered failed transaction again. The corrected test
pre-signs distinct compute-budget envelopes before the handoffs and asserts
that only that envelope field differs; trade bytes and all retained wire images
remain unchanged. Signature verification and transaction history remain enabled.
This was a test scheduling error, not a production regression or TDD fix claim.

## Exact verification

Private host/SBF caches copied from the existing public-gap caches. Wrapper and
hostile matcher rebuilt locked/offline from this worktree with platform-tools
v1.52. Engine pin: `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
Commands below use the same environment and arguments as execution; logs are
`/dev/shm/percolator-row411-followup-health-20260917-{sbf-build,matcher-build,check,check2}.log`.
Builds pass; `check` is the scheduling failure and `check2` is the final PASS.
Existing matcher deprecation and host future-compatibility warnings only.

```bash
git fetch origin main
git worktree add -b row411-followup-health-20260917 /dev/shm/percolator-row411-followup-health-20260917 origin/main
cd /dev/shm/percolator-row411-followup-health-20260917
cp -a /dev/shm/percolator-public-gap-20260916-c91e-host-target /dev/shm/percolator-row411-followup-health-20260917-host-target
cp -a /dev/shm/percolator-public-gap-20260916-c91e-sbf-target /dev/shm/percolator-row411-followup-health-20260917-sbf-target
env CARGO_TARGET_DIR=/dev/shm/percolator-row411-followup-health-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_BUILD_JOBS=2 cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row411-followup-health-20260917-sbf-target/deploy -- --locked
env CARGO_TARGET_DIR=/dev/shm/percolator-row411-followup-health-20260917-matcher-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_BUILD_JOBS=2 cargo build-sbf --manifest-path tests/fixtures/hostile_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/hostile_matcher/target/deploy -- --locked
export CARGO_TARGET_DIR=/dev/shm/percolator-row411-followup-health-20260917-host-target
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row411-followup-health-20260917-sbf-target/deploy/percolator_prog.so
T=inv_014_delayed_policy_and_policy_epoch_safety::retained_partial_fee_routes::generated_partial_policy_words::retained_partial_authority_routes::v16_retained_partial_fill_authority_return_preserves_four_route_fee_consent
cargo test --locked --offline --test v16_cu "$T" -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true tests/invariants/cu/inv_014_generated_partial_policy_words.rs tests/invariants/cu/inv_014_retained_partial_authority_routes.rs
rustfmt --check --edition 2021 --config skip_children=true tests/invariants/cu/inv_014_generated_partial_policy_words.rs tests/invariants/cu/inv_014_retained_partial_authority_routes.rs
git diff --check
git diff --cached --check
git diff --exit-code 1e3e2a4b -- src Cargo.toml Cargo.lock tests/fixtures 'tests/invariants/*.tsv'
git show --format= --check HEAD
```

Formatting, whitespace, staged-patch and committed-patch checks pass.
The unchanged-production/manifest/lock/fixture/status guard passes. Changed paths
are exactly the new test, its parent mount, this note and the short README entry;
row419/435 files are untouched.

Artifact SHA-256:
- Wrapper: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
- Hostile matcher: `e0c20fad34a7822cc6ce42a3c77ff08a8591977102f0c497a339d66a9dd6240a`.

## Open gaps

Bounded Live, one-asset, fixed-price, funded base-fee evidence only. Arbitrary
route/history lengths, multiple persistent partial fills, multi-leg aggregation,
independent fee sources, clipping, dynamic/backing/maintenance/redirect fees,
grant replacement/expiry, moving prices/funding, durable nonce transactions,
Recovery/Resolved lifecycle and maximum shapes remain outside this increment.
Renewed policy bundles are positive simulations; committed success uses the
original retained trade after a separate policy restoration. There is no claim
of retrying the identical failed transaction signature or whole-row closure.
