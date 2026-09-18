# Row 416: Last Live Quantity Tick

Base: `origin/main` at `dc65edbbdf4d4015579df87488f91887b31bcebb`.
Worktree: `/dev/shm/astra-ultra-row416-coverage-20260918`.
Branch: `astra-ultra/row416-coverage-20260918`; local commit only, no push.
Owner: [cu/inv_005_retained_oracle_exposure.rs](cu/inv_005_retained_oracle_exposure.rs).

## Distinct Boundary

The [last-resolved-exposure case](row416_last_resolved_exposure_20260917.md)
leaves ten whole position units on one side during terminal settlement. The
[cold-admin/DrainOnly case](row416_cold_admin_drain_exit_20260917.md) reduces
the entire matched position after incumbent-consented succession and explicitly
leaves partial reductions outside scope. Neither tests cold-admin admission at
the smallest live nonzero quantity. The retained-empty and funded-oracle-return
notes cover exposure arrival and authority epochs. Older backing-refunding,
consumed/earned-reserve and generated role histories cover different stock or
consent dimensions; their one-atom tails are not oracle-dependent position ticks.

One new selector crosses assets 0/1 with both position signs. Public TradeNoCpi
reduces a ten-unit matched position to exactly `q = 1` on each OI side: one
fixed-point quantity tick, not one whole position unit or quote atom. Both stored
position counts remain one, the asset is Active, the market is Live, and the
oracle profile and authority epoch are unchanged.

A seven-atom SPL withdrawal executes before retained cold-admin management
rejects exactly `EngineLockActive`. The existing helper checks complete compiled
and sentinel Accounts, exact payer fees and the successful wrapper/SPL prefix.
Public reduction of the final tick clears both OI sides and stored counts.
The original standalone signed handoff then commits without epoch renewal,
changing only the oracle role and one authority epoch. The rolled-back withdrawal
also commits. The failed bundle and standalone requests have distinct envelopes;
this does not replay an already-processed transaction signature.

All five owners recover their exact deposited principal through live withdrawals,
including the seven-atom prefix. Input-derived remaining capital, positive PnL,
internal/SPL custody, mint supply, sibling profile and sibling control sequences
are checked. All four public traces validate. Existing V16Svm setup and helpers
are reused unchanged; no initialized economic state is edited.

## Validation

Only the new exact selector and nearest existing terminal-boundary control ran,
once each: **2 passed, 0 failed**. The new selector covers four exact SPL-prefix
rollbacks, four empty-role handoffs and twenty owner exits.

| Exact selector suffix under `inv_005_authority_incarnation_binding::retained_oracle_exposure::` | Peak successful CU | Simulation CU |
| --- | ---: | ---: |
| `v16_program_cold_oracle_handoff_waits_for_last_live_quantity_tick` | 135,067 | none |
| `v16_program_cold_oracle_handoff_waits_for_last_resolved_exposure` | 126,361 | 126,572 |

The existing 1,400,000-CU limit is unchanged. Failed-transaction CU is not measured.
The wrapper was rebuilt locked/offline from this worktree with platform-tools
v1.52 and pinned engine `4db11a8cb0053815e23a35d3a7d3edc265d8d866`. Private host/SBF
caches were copied from `/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target`.
The ignored matcher target links to the existing main-worktree fixture artifact.

- Wrapper SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
- Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
- Logs: `/dev/shm/astra-ultra-row416-coverage-20260918-{build,new,control}.log`.

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row416-coverage-20260918-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-ultra-row416-coverage-20260918-sbf-target/deploy/percolator_prog.so
CARGO_TARGET_DIR=/dev/shm/astra-ultra-row416-coverage-20260918-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/astra-ultra-row416-coverage-20260918-sbf-target/deploy -- --locked
cargo test --locked --offline --test v16_cu inv_005_authority_incarnation_binding::retained_oracle_exposure::v16_program_cold_oracle_handoff_waits_for_last_live_quantity_tick -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_005_authority_incarnation_binding::retained_oracle_exposure::v16_program_cold_oracle_handoff_waits_for_last_resolved_exposure -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_005_retained_oracle_exposure.rs
git diff --check
git diff --cached --check
git show --check --oneline HEAD
```

Row 416 remains OPEN. This is a same-price, zero-fee/funding, solvent SPL boundary
case. OI and stored-position counts coexist; individual classifier terms,
loss-only states, claims, Recovery/ResetPending/Retired, other transports and
arbitrary histories remain outside scope. No production, Cargo, tracked fixture,
TSV or support-helper changes; no broad suite or source-composition selectors ran.
