# Retained Taker Aggregate Cap

Base: local `origin/codex/astra-open-holdout-ledger-20260912` at
`e660a52e7efef3c54041d26d78480107f7cc76f9`.
Worktree: `/tmp/percolator-astra-retained-taker-bounds-20260912-c91f`.
Branch: `codex/astra-retained-taker-bounds-20260912-c91f`.
Inputs: current repository source, tests and docs only. No fetch, PR diffs,
copied build artifacts, production edits or dependency changes.

## Overlap Search

Local `rg` searches covered `retained`, `fee`, `policy`, `411`, `432`, the
INV-011/014 mounts, public CPI handlers and authenticated matcher fixture.

| Existing Coverage | Difference in This Increment |
| --- | --- |
| `inv_014_retained_permitted_policy_history.rs` | Existing detours stay within both owners' consent; this rejects at the taker's aggregate cap after the matcher runs. |
| `inv_014_retained_fee_stock.rs` | Existing fee/insurance replenishment and replay; this composes a third owner's SPL deposit with above-cap policy rollback. |
| `inv_014_retained_fee_bundle.rs` | Existing independent/shared-taker instruction envelopes; this accumulates two legs inside one signed envelope and rolls back a successful policy writer. |
| `inv_014_retained_delegated_fee_exit.rs` | Existing LP grant enforcement; the LP and per-leg terms here allow both policy values. |
| `v16_program_retained_batch_route_switch_preserves_fee_caps_and_funded_provider` | Already isolates the batch taker aggregate cap across authority succession. New coverage is the atomic deposit/policy prefix, exact compiled-account frame and exact-cap retained continuation, without authority succession. |
| `inv_005_retained_debit_matrix.rs` | Existing debit identity/authority histories; no independent reserve-debit replay is added. |

## Relation and Boundary

One selector, `v16_program_retained_taker_aggregate_cap_rolls_back_deposit_and_policy_prefix`,
is mounted under `inv_014_delayed_policy_and_policy_epoch_safety::retained_taker_aggregate_cap`.
Four worlds cross direction with committed versus transaction-local fee tightening.
Both delivery transactions are signed before the landing policy changes. Two
fractional quantities trade at a constant authenticated price; zero slippage,
funding, maintenance and backing fees isolate the signed quantity and fee budget.
The LP grant and signed per-leg fee terms remain permissive. Each proposed leg
fee is individually below the aggregate atom cap, but their sum exceeds it.

A real SPL deposit precedes each rejected batch. In the atomic-policy worlds,
the policy update also succeeds before rejection. Logs require the SPL transfer,
wrapper prefixes and matcher invocation to succeed, followed by
`InvalidInstruction` at the batch instruction. Every tracked/compiled Account,
including absent accounts, signer accounts,
SPL accounts and matcher context, is restored byte-for-byte. The independent
payer is checked separately for its exact network signature fee. Policy and
matcher request sequences and position epochs are checked after rollback.

The unchanged positive transaction initially simulates successfully and later
lands at the exact signed aggregate cap. Input-derived accounting checks each
owner's capital, PnL, exact quantities, OI, domain fee allocation, token ownership,
mint supply, custody, stock and encumbrance. LP grant identity, expiry and sequence
remain intact; position epochs and the matcher request sequence advance once.
Fresh zero-fee closes, owner withdrawals and insurance payouts empty custody.
Complete SPL account bytes agree across the four final endpoints.

## Verification

Wrapper and authenticated matcher built locally, locked and offline, with
platform-tools v1.52 and separate private targets.
Wrapper SHA-256: `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

New selector: **1/1 passed**, four worlds, four initial simulations, four exact
rollbacks and 22 measured successes. Peak success/rejection CU:
**267,355 / 253,554**. Setup and the 28 public final withdrawals are outside those
CU counters. No runtime/compiler correction or marginal probe was needed.
Adjacent retained controls: **4/4 passed**. Peak CU: shared-taker bundle
`296980`, fee stock `175119`, permitted-policy history `150082` success / `149208`
rejection, batch route switch `231509` success. Invariant index: **1/1 passed**.
Existing unused-support and Solana future-compatibility warnings remain.

Commands from this worktree (only the new selector, adjacent retained controls,
index, formatting and whitespace verification):

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-taker-bounds-c91f-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=6 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
cargo test --locked --offline --test v16_program_stateful_fuzz inv_014_delayed_policy_and_policy_epoch_safety::retained_taker_aggregate_cap::v16_program_retained_taker_aggregate_cap_rolls_back_deposit_and_policy_prefix -- --exact --nocapture
cargo test --locked --offline --test v16_program_stateful_fuzz -- --exact --nocapture --test-threads=1 \
  inv_014_delayed_policy_and_policy_epoch_safety::retained_fee_bundle::v16_program_retained_shared_taker_fee_bundle_preserves_each_instruction_bound \
  inv_014_delayed_policy_and_policy_epoch_safety::retained_fee_stock::v16_program_retained_fee_stock_bundles_preserve_owner_budgets_across_policy_and_replenishment \
  inv_014_delayed_policy_and_policy_epoch_safety::retained_permitted_policy_history::v16_program_retained_cpi_owner_budgets_ignore_permitted_policy_detours \
  inv_014_delayed_policy_and_policy_epoch_safety::v16_program_retained_batch_route_switch_preserves_fee_caps_and_funded_provider
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
git diff --cached --check
```

## Remaining Gaps

Labels **411 and 432 remain OPEN**. This test covers the explicit batch CPI
aggregate atom cap. Single-CPI fee-bps enforcement, partial fills, maximum-size
batches, adverse price changes, dynamic mark fees, backing fees, arbitrary policy
histories and generic error/success composition remain outside this increment.
No reserve-debit replay or holdout closure is claimed. No marginal probes retained.
