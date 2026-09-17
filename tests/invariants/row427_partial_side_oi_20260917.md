# Row427 / INV-058 partial-fill side-OI competition

Base: `origin/main` at `88421800`.
Worktree: `/dev/shm/astra-ultra-row427-side-oi-20260917`.

[New selector](cu/inv_058_partial_side_oi_competition.rs):
`v16_program_partial_fills_compete_for_side_oi_and_match_aggregate_exit`.
The earlier INV-058 shared-maker/generated handoffs use full fills; INV-009's
maximum-quantity partial witness uses one pair. This adds actual partial execution
against capacity shared by three unequal disjoint pairs. No Row423 admission,
claim table, terminal reserve, or production change is involved.

Let `M = MAX_OI_SIDE_Q`, `h = 72 * POS_SCALE / 100 + 1`, and `r = 3h + 2`.
Initial pair quantities are `[M/4, M/3, M-M/4-M/3-2h]`. Both signs and all four
residual routes run with partial and aggregate schedules, for sixteen worlds.
System/SPL/wrapper calls construct and fund every portfolio. The existing external
matcher fixture is initialized and configured by owner-signed public instructions;
no market, portfolio, token, or matcher context bytes are injected.

- The signed request `r` exceeds initial headroom `2h`, but its authenticated
  85/255 partial fill is exactly `h`. A preliminary bundle first grows a disjoint
  pair by `h+1`; the actual partial then exceeds remaining headroom by one atom.
  Logs require the competing wrapper prefix and matcher call to succeed before
  `EngineInvalidLeg`. Full framed Accounts roll back, except the exact network fee.
- The same partial instruction then succeeds, leaving `h` headroom. The test reads
  its matcher return and checks actual size, price and partial flag, plus exact
  positions, side OI/counts, certificate notional, epochs, fees and token stock.
- A disjoint pair consumes the remaining `h`. A retained full residual `2h+2`
  rejects on each of the four routes. Another pair releases `2h+2`, after which
  the unchanged residual bytes succeed at the shared cap. Individual account
  position limits remain slack throughout.
- The aggregate schedule executes the same net changes with a single `r` fill
  after release. At 137 bps, the partial and residual each charge two atoms per
  owner, versus three for the aggregate. Exact projections agree after accounting
  for that one extra fee atom and one extra position epoch on each target owner.
- Three public closes and six exact withdrawals per world leave zero capital,
  positions and OI; custody retains precisely 16 split or 14 aggregate insurance
  atoms. All explicit trade/bundle/withdrawal packets are at most 1,232 bytes.

Result: **1 passed, 0 failed, 1,472 filtered**, 9.27s. Sixteen worlds include
eight committed partials, sixteen exact rollbacks and 96 owner payouts.
CU peaks `[rejection, accepted trade, withdrawal]`: **[343425, 191624, 46382]**.
Budgets: 345,000 CU per trade instruction, 300,000 per withdrawal, with the
1,400,000 transaction ceiling. Largest packet: **937 bytes**. Setup and matcher
control calls are excluded from these CU peaks. The first integration attempt used
a missing copied SBF artifact path; rerunning with the built `target/deploy`
artifact resolved it. No behavioral test correction or production bug was required.

Row427 remains **OPEN / Conformance / LIMIT**, INV-058 **REFUTED_CURRENT**.
This is one active asset, unit ADL, fixed mark, zero PnL/funding/elapsed liabilities,
one partial ratio and disjoint owners. Shared makers with partials, nonunit ADL,
rate limits and larger simultaneous shapes remain open. No real LoF/DoS/CU bug
was confirmed; no status TSVs change.

## Reproduction

Private copy of source-matching default-feature wrapper SBF, SHA-256
`a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`;
entire `src/`, `Cargo.toml` and `Cargo.lock` matched the source build.
Auth matcher SHA-256:
`50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
Hostile matcher rebuilt locally, locked/offline, platform-tools v1.52, SHA-256:
`e0c20fad34a7822cc6ce42a3c77ff08a8591977102f0c497a339d66a9dd6240a`.
Logs: `/dev/shm/astra-ultra-row427-side-oi-20260917-logs/`.

```bash
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row427-side-oi-20260917-host-target
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-ultra-row427-side-oi-20260917/target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu inv_058_cumulative_position_oi_notional_and_rate_limit_integrity::atomic_oi_fee_handoff::multi_asset::partial_side_oi_competition::v16_program_partial_fills_compete_for_side_oi_and_match_aggregate_exit -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_058_partial_side_oi_competition.rs tests/invariants/cu/inv_058_multi_asset_oi_fee_handoff.rs
git diff --check
git diff --cached --check
git show --format= --check HEAD
```

Only the new exact selector ran; no broad suite or engine proof. Formatting,
whitespace and path scope are checked before the local commit. Existing Solana
future-compatibility and fixture deprecation warnings remain.
