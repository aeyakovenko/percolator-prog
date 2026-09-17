# Row 427 shared-maker cap composition, 2026-09-17

Base: fetched `origin/main`, `9c27cbec22d93db3a2aceef228588bade707c520`.
Worktree: `/dev/shm/percolator-row427-health-20260917`.
Branch: `codex/row427-health-20260917`; local commit only.

## Increment

[New witness](cu/inv_058_multi_asset_oi_fee_handoff.rs):
`v16_program_shared_maker_two_asset_cap_handoff_preserves_fees_and_retry`.
Finding-blind inputs were `scripts/loop.md`, INV-058, the row427 README/TSV
sections, frontier/health audits, public trade tests and wrapper interfaces.
No withheld implementation or external finding was consulted.

Existing handoffs and competing cross-zero use disjoint pairs. This witness
starts three unequal disjoint pairs at both side caps, then releases on edge
(0,3) and refills on (2,3), sharing maker 3; pair (4,5) remains untouched.
Sixteen histories cross both signs, both asset orders, real two-leg bilateral/CPI
batches and packed/split transactions. Public System/SPL/wrapper construction
and public matcher grants are reused without injecting economic state.

The second batch binds the common maker's next position epoch and unchanged
matcher configuration sequence. A last-asset max-plus-one proposal rejects with
`EngineInvalidLeg` after a successful release and both expected matcher calls.
Every tracked/compiled Account, matcher state, custody, epoch and fee rolls back,
except the payer's exact network fee. Account caps and collateral remain slack.
Split histories repeat rejection after the release commits. Prebuilt exact
requests then succeed, reaching both caps with three stored positions per side.

Input-derived positions, epochs, raw OI/count/notional, capital, mint supply and
custody reconcile through the existing oracle. Nonintegral quantities charge
2/1 atoms per asset and owner; final fees are `[3,0,3,6,0,0]`, domain budgets
`[4,4,2,2]`. The graph transfer reverses publicly, all pairs close, and six exact
owner withdrawals leave zero capital/OI and exactly 12 insurance atoms in custody.
Successful calls also frame every unmentioned Account.

## Validation

Only the new exact selector ran: **1 passed, 0 failed, 1451 filtered**, 9.63s.
Completed: **16 histories, 24 exact rollbacks, 96 payouts**. Peak CU
reject/trade/custody: **572650 / 589145 / 53882**; largest bundle **968 bytes**.
Trade/rejection budgets are 345000 CU per wrapper instruction, below the
1400000 transaction ceiling. The first draft incorrectly advanced the matcher
configuration sequence on fill and hit `EngineStale`; correcting only the test
binding reached the intended cap rejection. No public LoF/DoS bug was found.

Fresh locked/offline default-feature SBF, platform-tools v1.52, engine `4db11a8c`:
`a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
Hash-verified unchanged auth matcher:
`50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
Logs: `/dev/shm/percolator-row427-health-20260917-logs/`.

Commands from the worktree (host dependency cache privately copied):

```bash
export CARGO_TARGET_DIR=/dev/shm/percolator-row427-health-20260917-host-target
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row427-health-20260917-sbf-target/deploy/percolator_prog.so
CARGO_TARGET_DIR=/dev/shm/percolator-row427-health-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row427-health-20260917-sbf-target/deploy -- --locked
sha256sum "$PERCOLATOR_FUZZ_SBF" tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
cargo test --locked --offline --test v16_cu inv_058_cumulative_position_oi_notional_and_rate_limit_integrity::atomic_oi_fee_handoff::multi_asset::v16_program_shared_maker_two_asset_cap_handoff_preserves_fees_and_retry -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_058_multi_asset_oi_fee_handoff.rs
git diff --check
git diff --exit-code 9c27cbec -- . ':!tests/invariants/README.md' ':!tests/invariants/row427_shared_maker_audit_20260917.md' ':!tests/invariants/cu/inv_058_multi_asset_oi_fee_handoff.rs'
git diff --cached --check
git show --format= --check HEAD
```

Formatting, whitespace and scope guards pass. Shared helpers, production,
Cargo, fixtures, status TSVs and row424 files are unchanged. Existing Solana
future-compatibility warning only; no broad suite or engine proof ran.

## Remaining gaps

Row427 remains **OPEN / Conformance / LIMIT**, INV-058 **REFUTED_CURRENT**.
This bounded graph uses fixed marks, unit ADL, zero PnL/funding and full fills.
Nonunit/effective rounding, combined elapsed liabilities, partial matcher fills,
graph cross-zero, mixed lifecycle/Recovery, larger graphs and maximum simultaneous
shapes remain open. No status promotion or generic composition proof is claimed.
