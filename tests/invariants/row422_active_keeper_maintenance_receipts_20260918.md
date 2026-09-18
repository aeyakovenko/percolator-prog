# Row 422 active keeper maintenance receipts, 2026-09-18

Base: `origin/main`, `cb05fc229f75bc2eca6b747d3e9c4faec432c4d8`.
Worktree: `/dev/shm/astra-ultra-row422-coverage-20260918`.
Branch: `qa/row422-coverage-20260918`; local commit only, no push.

## Non-duplicate increment

The [mixed maintenance-policy witness](row422_mixed_selected_maintenance_policy_20260917.md)
has a flat keeper and explicitly excludes active keeper exposure with its receipt
history. The [mixed exposed-keeper witness](row422_mixed_selected_exposed_keeper_20260917.md)
has zero maintenance and excludes maintenance/policy composition. The earlier
`reward_maintenance_catchup` family also uses a flat keeper and a single exposed
target leg. Repeating their route, hint or catchup matrices would be duplicate.

One selector in `cu/inv_045_mixed_selected_maintenance_policy.rs` adds just the
active-recipient/maintenance join during lag. It reuses `Maintenance`, `fund`,
`sync`, `submit_sync`, `observation` and `submit` without modifying any helper.
Two worlds choose between the target's 100-lot Hybrid and AuthMark legs. An
independent keeper holds a one-lot Hybrid long against the target's peer. Before
paid discovery, a flat trader pays 28 maintenance atoms and credits the keeper
exactly nine atoms. The keeper's own fee cursor remains at slot 1.

Paid discovery and a fresh slot-6 report leave both effective prices at 997,600,
above the 992,320 target; Hybrid provenance remains trade-driven. Each target
crank preserves the complete keeper account except for eligible liquidation
reward capital and certificate invalidation. Its old receipt, unpaid maintenance
and unsettled K survive both refresh and liquidation. Target maintenance is
collected before the liquidation budget; it cannot create liquidation rewards.

| Selected target leg | Penalty | Liquidation reward | Old receipt | Later keeper fee | Keeper value after K/fee settlement |
| --- | ---: | ---: | ---: | ---: | ---: |
| Hybrid | 11,973 | 0 | 9 | 35 | 97,574 |
| AuthMark | 11,973 | 3,990 | 9 | 35 | 101,564 |

The final values independently equal 100,000 principal + 9 old receipt + reward
- 35 maintenance - 2,400 price loss. The keeper remains active. The existing
maintenance ledger checks exact insurance and domain attribution after each
target crank and keeper settlement. Both OI lanes balance, the nonselected
target OI is unchanged, and target health is restored. Penalty arithmetic uses
the executed reduction and independently predicted price; liquidation quantity
itself is engine-selected, not independently solved.

Four conflicting-report suffix transactions cover successful target refresh and
liquidation prefixes. The reused submit helper separately simulates each prefix
and requires exact tracked/transaction-account rollback, allowing only the
separate payer's signature fee. Mint authority is revoked before receipts and
discovery; mint/vault bytes, zero owner SPL payouts and total custody are checked.
System/SPL/wrapper instructions construct all economic state. Clock, external
Pyth reports, signer SOL and program loading are the only fixture inputs.

## Exact validation

Only the new selector and the nearest existing control ran, once each:

| Selector suffix | Result | Worlds | Exact rollbacks | Peak transaction CU |
| --- | --- | ---: | ---: | ---: |
| `v16_program_mixed_selected_rewards_preserve_active_keeper_maintenance_receipts` | 1 passed, 0 failed; 1.16s | 2 | 4 | 450,294 |
| `v16_program_mixed_selected_liquidation_preserves_maintenance_policy_receipts_and_payout` | 1 passed, 0 failed; 5.16s | 8 | 24 | 434,066 |

Both runs filtered out 1,484 tests. New peak includes opening/discovery trades,
maintenance sync, mark push, cranks and failed suffix transactions. Initial
funding/configuration is outside the peak; the transaction bound is 650,000 CU.
Control category peaks (progress, maintenance, policy, withdrawal) are
434,066 / 153,532 / 1,871 / 80,625 CU. No broad suite or other selector ran.

Host dependencies were copied from `/dev/shm/percolator-main-host-target` and
the SBF cache from `/dev/shm/percolator-public-gap-20260916-c91e-sbf-target`
using `cp -a --reflink=auto`, each into private targets below. The wrapper was
rebuilt locked/offline from this worktree with platform-tools v1.52 and engine
`4db11a8cb0053815e23a35d3a7d3edc265d8d866`. SBF SHA-256:
`a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`, matching
the recent Row 422 notes. Neither selector needs a matcher fixture.

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row422-coverage-20260918-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-ultra-row422-coverage-20260918-artifacts/percolator_prog.so
CARGO_TARGET_DIR=/dev/shm/astra-ultra-row422-coverage-20260918-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/astra-ultra-row422-coverage-20260918-artifacts -- --locked
cargo test --locked --offline --test v16_cu inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::retained_penalty_handoff::mixed_selected_provenance::maintenance_policy::v16_program_mixed_selected_rewards_preserve_active_keeper_maintenance_receipts -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::retained_penalty_handoff::mixed_selected_provenance::maintenance_policy::v16_program_mixed_selected_liquidation_preserves_maintenance_policy_receipts_and_payout -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_045_mixed_selected_maintenance_policy.rs
git diff --check
git diff --cached --check
git show --format= --check HEAD
```

Formatting, whitespace and the three-file changed-path allowlist pass. The
existing Solana future-compatibility warning remains. Production, Cargo,
fixtures, TSVs and support helpers are unchanged. Row 422 remains OPEN.
This finite increment does not cover nonzero funding, policy succession with an
active keeper, catchup, other transports/providers, larger shapes, arbitrary
histories, or terminal payout composition. No production issue is claimed.
