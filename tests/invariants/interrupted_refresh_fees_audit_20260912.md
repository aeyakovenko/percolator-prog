# Interrupted Refresh, Maintenance and Liquidation Entitlement

Base: `origin/codex/astra-open-holdout-ledger-20260912` at
`3dabe7e4aa096988d9a09f16f2b580b63cf10d71`.
Worktree: `/home/anatoly/percolator-row426-public-observations-20260912`.
Branch: `codex/astra-row426-public-observations-20260912`.
Only this worktree was edited. No GitHub PR/issue branches or diffs were inspected
or copied. Production, shared support, dependency pins and invariant verdicts are
unchanged. This is current-code conformance, with no vulnerable-pin replay.

Owner: [cu/inv_020_interrupted_refresh_fees.rs](cu/inv_020_interrupted_refresh_fees.rs),
mounted below `inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations`.
Selector: `interrupted_refresh_fees::v16_program_interrupted_refresh_preserves_fee_and_liquidation_entitlements`.

## Distinct Relation

The existing staged-action selector already compares bounded market-only catchup,
missing AuthMark evidence, full refresh and paid liquidation or owner reduction.
Recreating that product was discarded during overlap review. Its maintenance rate
is zero, however. The fee-refresh admission selector has seven-atom maintenance
but constant prices, one-slot progress, healthy admission and no liquidation.
The aged-maintenance liquidation control exhausts target capital with senior
fees, without interrupted external observation or a positive reward continuation.

This increment joins **interrupted observation completeness, real senior fee
collection through two public refresh routes, and a positive liquidation reward**.
The finite product crosses:

- Completion at authenticated slot 64 versus a Clock advance to slot 65.
- Target crank with automatic maintenance versus keeper market discovery followed
  by explicit `SyncMaintenanceFee` and target recertification.
- Both complete observation orders, with matching oracle tails.
- Complete continuation versus an intervening omitted-pending-AuthMark attempt.

All sixteen worlds start from independent public construction. System, SPL, ATA
and wrapper instructions create and fund all economic state; mint authority is
revoked. The inherited `funded_owner` helper constructs portfolios through System
and `InitPortfolio`. No initialized program-owned bytes are edited or restored.
Signer SOL, authenticated Clock and simulated provider-owned Pyth reports are
harness inputs; real Pyth publication/signature verification remains a fixture
assumption. Caller slot hints are always `u64::MAX`.

## Observations and Economic Oracle

The two initial prices are 1,000,000, with one unit short per asset on the target.
Deposits are 10,000,000 / 220,000 / 1,000 for peer, target and keeper. Current
targets are 1,040,000 / 1,050,000, published at slot zero. Price movement is an
integer 1,000 per logical slot, with zero funding. Fractional price carry and
trade-origin provenance are not varied; this does not duplicate rows 422/425.

The first 32-step prefix at Clock 64 advances both assets to slot 32 / 1,032,000
without changing any portfolio or charging maintenance. At completion time 64,
omitting AuthMark rejects exactly and restores the successful Hybrid prefix plus
any attempted account fee work. At time 65 the same omission instead commits
Hybrid progress to slot 64, leaving AuthMark at 32; every portfolio and insurance
remain unchanged. Thus rejection alone is not the invariant oracle: successful
incomplete discovery is also required to preserve account economics.

Complete continuations strictly decrease the input-defined sum of remaining
asset slots, reaching zero in at most two more cranks. They pay no keeper reward.
Explicit fee collection rejects `EngineLockActive` before market completion;
the same instruction bytes succeed after complete keeper discovery. This route
settles the target's price loss and fee before recertification. Automatic
collection reaches the same current target certificate inside the crank.

Independent inputs specify loss 90,000, gross IM/MM 209,000, maintenance
`7 * completion_slot` (448 or 455), equity `220000 - 90000 - maintenance`, and
deficit `209000 - equity`. Current certificates must also pass the independent
raw-state health model; the target check requires that model comparison to execute,
not silently skip a stale certificate. Shape, stock and encumbrance checks run
through completed refresh, liquidation and payout.

Liquidation must reduce asset 0, preserve asset 1's quantity, restore health and
retain coherent OI. The reward oracle independently rounds the actual closed
quantity's notional at 1,040,000, then applies the 100-bps fee capped at 10,000.
Keeper credit is `floor(penalty * 3333 / 10000)`, excluding all maintenance.
Insurance retains `maintenance + penalty - reward`. Its two asset-0 domain
budgets must match the separate floor/remainder allocations; asset 1 receives zero.
Liquidation sizing is a deployed input to this oracle, not independently proved.

The keeper's public SPL withdrawal additionally collects its own 448/455-atom
maintenance exactly once. Payout is `1000 + reward - maintenance`, keeper capital
ends at zero, and insurance becomes `2 * maintenance + penalty - reward`. The
domain oracle preserves each separate odd-atom split. Fixed mint supply, custody,
target value and the untouched peer Account remain accounted for. The eight
worlds at each completion time have identical quantitative liquidation and payout
outcomes despite differing fee routes and observation schedules.

| Completion Slot | Maintenance Per Owner | Liquidation Penalty | Keeper Reward | Keeper SPL Payout | Target Capital | Final Insurance |
| --- | --- | --- | --- | --- | --- | --- |
| 64 | 448 | 8,828 | 2,942 | 3,494 | 120,724 | 6,782 |
| 65 | 455 | 8,829 | 2,942 | 3,487 | 120,716 | 6,797 |

Sixty rejections compare complete tracked and compiled Accounts, including payer
metadata less the independently calculated signature fee: sixteen old-but-still-
wall-clock-fresh report replays, thirty-two late duplicate observations, eight
premature fee collections and four omitted pending-leg attempts. All valid retry
paths continue to independently checked positive economics. Fixture development
corrected helper imports, an error-enum label and the explicit fee route's
side-effect settlement assumption; no production conformance defect was found.

## Limits and Ledger

Added bounded evidence belongs to INV-020/024/053/054/056/061/071/072/081/086,
with maintenance seniority adjacent to INV-027. **Row 426 remains OPEN.** The ledger
requires an invariant-owned oracle that fails on a vulnerable pin and passes
unchanged after a production fix; this increment does not meet that closure rule.

The test omits pending AuthMark, not economically pending Hybrid evidence. It does
not cover arbitrary provider modes/assignments, maximum account shapes, nonzero
funding, bankruptcy, claims, all favorable wrapper routes, or general liquidation
sizing/liveness. The exposed peer remains unrefreshed and unpaid; only the keeper
completes an SPL exit. No whole-invariant or holdout promotion is claimed.

## Validation

The final exact run passes **4/4**: the new selector and three nearby controls.
The new selector passes sixteen worlds and sixty exact rollbacks with peak
**394,545 CU**, below its 500,000-CU two-asset bound. All four selectors complete
in 13.90 seconds. Repository formatting, whitespace and the production/support/
pin/verdict equality check pass. No broader tests were run.

Fresh locked/offline default-feature SBF was built in the private target directory.
Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
Wrapper SHA-256: `d5f2d3d2c35842aab0979ab24fed415fe36b2b93ed6cc80998fee839ae76343f`.
No build output was copied from other worktrees. Exact commands:

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-row426-public-observations-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 TMPDIR=/dev/shm
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations::interrupted_refresh_fees::v16_program_interrupted_refresh_preserves_fee_and_liquidation_entitlements \
  inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations::v16_program_staged_observations_match_current_liquidation_and_reduction \
  inv_020_authenticated_clock_slot_and_oracle_provenance::fee_refresh_admission::v16_program_timestamp_renewal_fee_refresh_and_funded_admission_retry \
  inv_027_protected_principal_seniority::v16_program_issue408_liquidation_reward_cannot_preempt_aged_maintenance_collateral
cargo fmt --all -- --check
git diff --check
git diff --exit-code 3dabe7e4aa096988d9a09f16f2b580b63cf10d71 -- src tests/support Cargo.toml Cargo.lock tests/invariants/invariant_status.tsv
```

No broad suite, matcher build, Kani run or cargo check is needed for this test-only
increment. The existing `solana-client v1.18.26` future-compatibility warning remains.
