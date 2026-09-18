# Row 427 shared-maker partial-fill cap handoff, 2026-09-18

Base: `origin/main` at `703b0f8dff77b36b324d5642280f71e618956ecf`.
Worktree: `/dev/shm/astra-ultra-row427-coverage-20260918`.
Branch: `qa/row427-coverage-20260918`; local commit only, no push.

One selector is added to
[the existing partial-fill owner](cu/inv_058_partial_side_oi_competition.rs):
`v16_program_shared_maker_partial_handoff_retries_at_exact_side_oi_cap`.
The [earlier partial-fill witness](row427_partial_side_oi_20260917.md) uses
disjoint pairs and explicitly leaves shared makers open. The
[shared-maker witness](row427_shared_maker_audit_20260917.md) uses full fills.
Neither covers actual partial quantities composing through one maker's position
epochs and matcher response context at the aggregate cap.

The existing public System/SPL/wrapper setup opens three unequal pairs at both
side caps. Both position signs run. Let `h = RELEASE_Q`; the existing public
matcher control selects an 85/255 fill ratio. Edge (2,1) requests a reduction of
`3(h-1)`, releasing `h-1`; edge (0,1) requests `3h+2`, actually filling `h`.
The latter is signed for maker 1's next position epoch. One successful wrapper
prefix and two successful matcher calls precede `EngineInvalidLeg`. Every framed
Account, including matcher context, fees, epochs and custody, rolls back except
the exact network fee.

Changing only the prefix quantity to `3h` allows the unchanged second instruction
to succeed. Both actual fills are `h`, restoring the cap. The common maker's net
position is unchanged, its epoch advances twice, and it pays four fee atoms;
the two takers pay two each. Exact raw positions, OI/counts, certificate notional,
capital, fee domains, mint supply and custody reconcile through the unchanged
oracle. Public reversals and three pair closes leave zero positions and OI.
No economic account bytes are injected and no support helpers are modified.

## Validation

Only the new selector and nearest existing disjoint partial-fill control ran,
each once with `--exact --nocapture --test-threads=1`. Both passed 1/1 with 1,482
filtered out, on a private default-feature SBF rebuilt from this worktree.

| Selector | Completed coverage | Peak rejection / trade / withdrawal CU | Max packet |
| --- | --- | --- | --- |
| New shared-maker partial handoff | 2 worlds, 2 exact rollbacks, 4 committed partials; 1.14s | 356,496 / 370,634 / not exercised | 820 bytes |
| Existing disjoint partial-fill control | 16 worlds, 16 exact rollbacks, 96 payouts; 9.13s | 344,925 / 193,124 / 44,882 | 937 bytes |

Trade budgets remain 345,000 CU per instruction, hence 690,000 for the new
two-instruction bundles, within the 1,400,000 transaction limit. Setup and matcher
control calls are excluded from these peaks. Logs are in
`/dev/shm/astra-ultra-row427-coverage-20260918-logs/`.

Private host/SBF caches were copied from existing targets. The wrapper rebuild
used locked/offline platform-tools v1.52 and engine `4db11a8c`. Existing fixture
binaries were copied into ignored artifact paths without changing fixture sources.
SHA-256 values:

- Wrapper: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
- Auth matcher: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
- Partial-capable hostile matcher: `e0c20fad34a7822cc6ce42a3c77ff08a8591977102f0c497a339d66a9dd6240a`.

```bash
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row427-coverage-20260918-host-target
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-ultra-row427-coverage-20260918-sbf-target/deploy/percolator_prog.so
CARGO_TARGET_DIR=/dev/shm/astra-ultra-row427-coverage-20260918-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/astra-ultra-row427-coverage-20260918-sbf-target/deploy -- --locked
cargo test --locked --offline --test v16_cu inv_058_cumulative_position_oi_notional_and_rate_limit_integrity::atomic_oi_fee_handoff::multi_asset::partial_side_oi_competition::v16_program_shared_maker_partial_handoff_retries_at_exact_side_oi_cap -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_058_cumulative_position_oi_notional_and_rate_limit_integrity::atomic_oi_fee_handoff::multi_asset::partial_side_oi_competition::v16_program_partial_fills_compete_for_side_oi_and_match_aggregate_exit -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_058_partial_side_oi_competition.rs
git diff --check
git diff --cached --check
git show --check HEAD
```

Formatting and whitespace checks pass. The diff contains only this note, the
README entry and the new selector. Existing Solana future-compatibility warning
only; no broad suite or engine proof ran.

Row 427 remains **OPEN / Conformance / LIMIT**; INV-058 stays **REFUTED_CURRENT**.
This increment uses one exposed asset, unit ADL, fixed price and zero PnL/funding
or elapsed liabilities. It does not cover payouts, nonunit ADL, multiple active
assets exposed with shared partials, larger graphs or general route/history composition.
No production issue or status promotion is claimed.
