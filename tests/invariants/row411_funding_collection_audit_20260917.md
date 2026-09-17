# Row 411 retained funding collection, 2026-09-17

Base: freshly fetched `origin/main`, `6ea24e2e48340ac561d27800b5d8caf8b7e33d5d`.
Worktree: `/dev/shm/percolator-row411-health-20260917`.
Branch: `codex/row411-health-20260917`; local commit only.
Primary INV-014; related INV-005/010/011/024/036/047/081 evidence.
No production bug or parent/head fix claim; row 411 remains OPEN.

## Coverage increment

Read `scripts/loop.md`, targeted invariant statements, README sections, status,
reopening and traceability TSV entries, and their mounted test owners. No withheld
finding, patch or external issue was used. Existing partial-authority and generated
partial-policy histories have zero funding/maintenance. The maintenance-reward
history has no pending funding; aggregate-cap and live entitlement histories also
hold funding fixed at zero. This adds collection of an existing position's funding
inside a retained fee-bearing close, including actual owner withdrawals.

New child: `cu/inv_014_retained_funding_collection.rs`, mounted under
`inv_014_retained_partial_authority_routes.rs`. Existing helpers are unchanged.
Eight worlds cross both position directions with single/batch CPI/no-CPI closes:

- A public matcher executes 95 of 255 position units at price 100, charging each
  owner 36 atoms at 37 bps. Closing alternatives are signed before A -> B -> A,
  the temporary 71-bps policy, and oracle/clock movement. An initial simulation
  confirms the retained close is admissible without mutating state.
- Public EWMA inputs 98 then 101 produce marks 99 then 100. The one-bps circuit
  breaker leaves the effective/execution price at 100. The negative funding rate
  is capped at 1,000 e9 units for one slot; signed floor rounding transfers exactly
  95 quote atoms from short to long. This is a fixed rounding-sensitive funding
  scenario, not a claim about positive-rate symmetry or oracle manipulation.
- The slot-1 crank collects 307 maintenance atoms from the taker. The close must
  collect the remaining 307/614, settle funding, and charge only 36 trade atoms per
  owner. Funding gains exceed even the 71-bps trade fee; maintenance exceeds the
  signed trade cap. Capital, PnL and gross fee ledgers therefore stay separate.
- At 71 bps, retained 37-bps consent (batch CPI: 36-atom aggregate cap with a
  permissive 137-bps LP grant) rejects. After restoration to 37 bps, a successful
  close precedes an obsolete-epoch policy suffix and rolls back exactly. Its policy
  sequence is still future. A separately pre-signed identical close commits;
  consumed-episode retry rejects. Successful program logs identify executed prefixes.
- Complete tracked and compiled Accounts roll back, including matcher context,
  funding indices, fee cursors, epochs, request sequence and SPL custody; only the
  separate payer's exact network fee remains. Retained serialized transactions and
  signatures stay unchanged. The input ledger checks per-call insurance rounding,
  owner-local capital/PnL, OI, mint supply, complete token Accounts, and stock and
  encumbrance censuses after every post-opening transition.
- At slot 3, public fee sync and an observed crank recertify the released claim;
  conversion pays all 95 funding atoms. Both owners withdraw fully. Short taker:
  payouts `[99028,199109]`; long taker: `[99218,198919]`. All routes leave exactly
  1,986 insurance atoms in custody. No program-owned state is injected.

## Verification

Both exact selectors below PASS. New selector: 8 worlds, 8 admissible simulations,
24 delivery rollbacks, 16 fills, 16 payouts; peak **456,043 CU**, below 500,000.
Only the adjacent parent selector is rerun; no broad suite or status promotion.
Development failures exposed fixture/oracle assumptions: crank maintenance,
pending-mark sync rejection, changed matcher quote, recertification requiring
an asset observation, and single/context versus batch/return-data matcher output.
The final public continuation resolves each; no production change was needed.

Wrapper and hostile matcher rebuilt locked/offline from this worktree using
platform-tools v1.52 and private `/dev/shm` targets. Engine pin:
`4db11a8cb0053815e23a35d3a7d3edc265d8d866`. Private caches were initially copied
from `percolator-public-gap-20260916-c91e-{host,sbf}-target` (no shared writes).
SHA-256:

- Wrapper: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
- Matcher: `e0c20fad34a7822cc6ce42a3c77ff08a8591977102f0c497a339d66a9dd6240a`.

```bash
cd /dev/shm/percolator-row411-health-20260917
env CARGO_TARGET_DIR=/dev/shm/percolator-row411-health-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_BUILD_JOBS=2 cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row411-health-20260917-sbf-target/deploy -- --locked
env CARGO_TARGET_DIR=/dev/shm/percolator-row411-health-20260917-matcher-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_BUILD_JOBS=2 cargo build-sbf --manifest-path tests/fixtures/hostile_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/hostile_matcher/target/deploy -- --locked
export CARGO_TARGET_DIR=/dev/shm/percolator-row411-health-20260917-host-target
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row411-health-20260917-sbf-target/deploy/percolator_prog.so
P=inv_014_delayed_policy_and_policy_epoch_safety::retained_partial_fee_routes::generated_partial_policy_words::retained_partial_authority_routes
cargo test --locked --offline --test v16_cu "$P::retained_funding_collection::v16_retained_partial_close_separates_funding_and_maintenance_from_fee_consent" -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu "$P::v16_retained_partial_fill_authority_return_preserves_four_route_fee_consent" -- --exact --nocapture --test-threads=1
rustfmt --check --edition 2021 --config skip_children=true tests/invariants/cu/inv_014_retained_partial_authority_routes.rs tests/invariants/cu/inv_014_retained_funding_collection.rs
git diff --check
git diff --cached --check
git diff --exit-code 6ea24e2e -- src Cargo.toml Cargo.lock tests/fixtures 'tests/invariants/*.tsv'
git show --format= --check HEAD
```

Logs: `/dev/shm/percolator-row411-health-20260917-{sbf,matcher,final,control}.log`.
Formatting and diff checks pass. Changed files are this audit, README, the new test
and its parent mount. Production, Cargo, fixtures, TSVs and row423 files are unchanged.

Remaining: positive funding rates, arbitrary marks/history lengths, multiple partial
fills, multi-leg aggregate caps, backing/dynamic/redirect fees, collection clipping,
underfunding, reward recipients, grant expiry, Recovery/Resolved and maximum shapes.
This bounded success/rollback evidence does not establish general INV-081 validity.
