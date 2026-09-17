# Row422 mixed selection with maintenance policy receipts

Base: `origin/main` at `5470d70762337f11dc94210434b6a6e036d297f0`.
Worktree: `/dev/shm/astra-ultra-row422-reward-maintenance-conformance-20260917`.
Scope: defensive public conformance for INV-020/024/036/041/045/061/062.

## Non-duplicative increment

The regression-health, mixed-selected-provenance, exposed-keeper and mark-movement
notes leave maintenance/policy composition with mixed target selection open.
The existing mixed selectors have zero maintenance. The separate
`inv_045_reward_maintenance_catchup.rs` and `inv_045_reward_policy_catchup.rs`
families use a single exposed target leg; INV-024's maintenance-policy entitlement
selector uses flat portfolios. None joins historical maintenance receipts and
canonical maintenance budgets with the choice between differently sourced target
legs and the recipient's later maintenance-net SPL withdrawal.

One new child selector adds that join. Eight worlds cross selected Hybrid/AuthMark
leg, liquidation one/four slots after paid discovery, and keeper maintenance
collection before liquidation/during withdrawal. Target and peer each hold two
100-lot legs. Two initially flat traders each pay 28 maintenance atoms through
retained public sync instructions straddling a share update from 3,333 to 6,667
bps, crediting the independent keeper exactly 9 and 18 atoms. They subsequently
pay the independently computed discovery fee. Liquidation share stays 3,333 bps.

| Selected leg | Boundary | Penalty | Liquidation reward | Maintenance receipts | Keeper fees | SPL payout |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Hybrid | Lag, 997,600 | 11,973 | 0 | 27 | 35 | 992 |
| AuthMark | Lag, 997,600 | 11,973 | 3,990 | 27 | 35 | 4,982 |
| Either | Catchup, 992,320 | 12,720 | 4,239 | 27 | 56 | 5,210 |

Nonzero asset-0 maintenance budgets and earned receipts cannot authenticate a
lagged selected Hybrid leg. AuthMark selection still earns its reward while the
nonselected Hybrid leg has trade-driven provenance. At catchup both selections
are eligible. An input-derived maintenance ledger checks each charge, each
receipt's floor rounding and each retained fee's separate floor/ceil domain
split. The penalty oracle uses executed reduction quantity and independently
predicted accepted price; it is not an independent liquidation-quantity model.

Every owner has exact settled PnL, fee and receipt attribution. Policy succession
preserves portfolio bytes, engine state and Hybrid profile. Each target crank
permits only the computed reward and certificate invalidation in the recipient;
its fee cursor and old receipts survive. The final accounts, legs, payout,
insurance and all domain budgets agree across collection orders. Independent
health, stock/source census, balanced OI, fixed mint and exact SPL custody checks
remain in force. No terminal reserve or market resolution path is exercised.

Sixteen conflicting-report suffix transactions cover successful refresh and
liquidation prefixes, and eight invalid System suffix transactions cover the
successful SPL withdrawal. Each prefix is simulated independently. All tracked
and compiled transaction accounts roll back exactly, allowing only the separate
network payer's exact signature fee. System/SPL/wrapper instructions construct
and change all economic state. Only Clock, signer funding, program loading and
external Pyth report accounts are fixtures; mint authority is revoked before
receipt collection and discovery. No program-owned bytes are injected.

## Exact validation

Only the new selector ran: **1 passed, 0 failed, 1,475 filtered out; 5.14s**.
Eight worlds and 24 exact rollbacks. Measured transaction CU peaks:

| Operation | Peak CU |
| --- | ---: |
| Progress, trades and conflicting-report rollbacks | 434,066 |
| Explicit maintenance sync | 153,532 |
| Maintenance policy succession | 1,871 |
| Keeper withdrawal and its rollback | 79,123 |

Setup, funding and initial configuration are outside these peaks. The inherited
progress/withdrawal submit helper bounds each transaction by 500,000 CU times
its instruction count. All four category peaks additionally have a 650,000 CU
transaction bound.

Private host dependencies were copied from
`/dev/shm/percolator-main-host-target`. The SBF cache was copied from
`/dev/shm/percolator-public-gap-20260916-c91e-sbf-target`, then the wrapper was
rebuilt locked/offline from this worktree with tools v1.52 and engine
`4db11a8cb0053815e23a35d3a7d3edc265d8d866`. Its SHA-256 is
`a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`, matching
the current Row422 notes. This no-CPI selector needs no matcher fixture.

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row422-reward-maintenance-conformance-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-ultra-row422-reward-maintenance-conformance-20260917-artifacts/percolator_prog.so
CARGO_TARGET_DIR=/dev/shm/astra-ultra-row422-reward-maintenance-conformance-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/astra-ultra-row422-reward-maintenance-conformance-20260917-artifacts -- --locked
cargo test --locked --offline --test v16_cu inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::retained_penalty_handoff::mixed_selected_provenance::maintenance_policy::v16_program_mixed_selected_liquidation_preserves_maintenance_policy_receipts_and_payout -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_045_mixed_selected_provenance.rs tests/invariants/cu/inv_045_mixed_selected_maintenance_policy.rs
git diff --check
git diff --cached --check
```

Logs are beside the SBF in the private `-artifacts` directory: `build-sbf.log`
and `probe4.log` (passing run). Earlier probes stopped on test-construction
assumptions: an extra signer on permissionless sync, exposed fee anchoring before
market advancement, and a flat observation's unchanged fee cursor. The first
probe used the copied pre-rebuild SBF and stopped before the new economic path;
only the rebuilt artifact supports the reported result.

Targeted rustfmt, whitespace and an exact changed-path allowlist pass. No broad
suite or additional selector ran. The existing Solana future-compatibility
warning remains. Production, Cargo, fixtures, status TSVs and other rows are
unchanged. Row422 remains OPEN; this finite composition does not cover nonzero
funding, repeated mixed-selected liquidation episodes, active keeper exposure
with this receipt history, alternate transports/providers, larger shapes or
full owner redemption. No production issue or invariant closure is claimed.
