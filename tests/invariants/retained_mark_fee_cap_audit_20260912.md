# Retained Computed Mark-Fee Cap

Base: `origin/codex/astra-open-holdout-ledger-20260912` at
`798c30d815892ac5283a62add9bfcc7f15b3a435`.
Worktree: `/tmp/percolator-prog-astra-retained-taker-consent-20260912`.
Branch: `codex/astra-retained-taker-consent-20260912`.

Only the requested branch was fetched. Inputs were the base's repository source,
tests, charter and audit notes. No open PR diff or remote PR body was consulted;
411/432 are holdout identifiers only. Production, Cargo files and the original
checkout's working files were not edited. Artifacts were built locally in private
targets, locked and offline. No push was performed. The session cannot select or
verify the requested `gpt-6-astra/ultra` runtime; no runtime identity claim is made.

## Overlap Review

| Existing coverage | New relation |
| --- | --- |
| `stateful/inv_014_retained_taker_aggregate_cap.rs` | That test disables mark movement. Here the signed atom cap includes independently computed movement fees after an authorized quote change. |
| `stateful/inv_014_retained_permitted_policy_history.rs` | That test varies permitted base fees at constant authenticated prices. Here the dynamic fee raises the actual debit above the base-only sum. |
| `stateful/inv_014_retained_fee_bundle.rs` | Separate instruction envelopes and shared-taker composition; this is one two-leg envelope with computed fees and both leg orders. |
| `stateful/inv_014_retained_fee_stock.rs` | Fee stock, payout order and replenishment; this tests retained dynamic-fee consent before payout. |
| `v16_program_retained_batch_route_switch_preserves_fee_caps_and_funded_provider` | Existing authority/route succession uses base fees; this tests EWMA movement fees with unchanged standing consent. |
| `cu/inv_014_delayed_policy_and_policy_epoch_safety.rs` | Retained single partial and batch exact-fill boundaries after permitted base-policy changes already exist; no new partial-fill claim is made. |
| `cu/inv_011_signed_aggregate_economic_bounds.rs` | Existing aggregate/partition controls use fixed authenticated fee prices. Here the fee has a computed movement component. |
| `stateful/inv_045_no_free_mark_movement.rs` and `inv_045_retained_mark_exit.rs` | Existing paid-mark and exit relations do not assert this retained exact-minus-one/exact batch atom boundary. |

## Test Scope

Selector:
`inv_014_delayed_policy_and_policy_epoch_safety::retained_mark_fee_cap::v16_program_retained_batch_atom_cap_includes_computed_mark_fees`.

Eight worlds cross two trade directions, two leg orders and direct versus
nonmonotone committed policy histories. Both deliveries are signed while the base
policy is 19 bps, at zero matcher spread, slot 1 and price 100,000. Both initially simulate successfully
without changing any tracked or compiled Account. The histories are `19 -> 37`
and `19 -> 0 -> 23 -> 37`. Public LP configuration then changes bid/ask spreads to
100 bps, and the authenticated clock advances one slot. LP and per-leg terms stay
at 503 bps. Both quantities are fractional and opposite-signed, with different
magnitudes. No program-owned state bytes are injected.

The oracle uses input quantities and quote prices to independently calculate
ceil-rounded notional and base fees, a half-weight EWMA movement at halflife one,
and the two-sided movement charge. It searches the bounded bps range for the
least fee covering that charge, without production fee/mark helpers or deriving
expectations from observed debits. In these funded, initially empty-OI worlds,
each computed rate is below both participants' bps terms. Each individual leg
fee and the sum of base fees fit below the rejected aggregate cap. Only the
complete computed-fee sum crosses it.

The exact-minus-one delivery requires successful matcher execution and then
`InvalidInstruction` at the wrapper. Every tracked/compiled Account, including
absent accounts, portfolios, matcher context, SPL custody and passive accounts,
matches its prior value. The independent network payer loses exactly the
signature fee. The unchanged exact-cap transaction then accepts both legs.
Positions and the market matcher-request sequence advance exactly once; the
standing LP grant sequence and identity remain unchanged.

After each policy step, rejection and fill, the test checks stock and encumbrance,
input-priced owner capital, zero PnL, exact positions and OI, both stored EWMA marks,
total insurance and per-domain base-fee budgets. The movement-fee portion stays
outside operator-withdrawable budgets. Successful delivery permits data changes
only in the market and two portfolios. Token account bytes, vault custody and
mint supply stay fixed. Equivalent direction worlds agree economically across
leg order and policy detours. The test stops with open positions at the paid
mark targets; it does not test subsequent mark catchup, settlement or withdrawal.

## Verification

Locally built wrapper SHA-256:
`5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
Locally built authenticated matcher SHA-256:
`50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

New selector: **1/1 passed**, eight worlds, 16 initial simulations, eight exact
rollbacks and eight exact-cap fills. Peak success/rejection CU:
**260,297 / 235,177**. Fixture construction and policy/spread writes are outside
those CU counters. The first run exposed a test-only expectation that incorrectly
advanced the standing matcher-grant sequence on fill. Correcting it to preserve
that sequence matches the public contract and existing adjacent coverage; the
economic assertions were unchanged. No production fix or discarded marginal
probe was needed. Existing unused-support and Solana future-compatibility warnings
remain. Nearby controls: **4/4 passed** (retained aggregate cap, permitted-policy
history, retained batch route switch and paid-mark route convergence). Invariant
index: **1/1 passed**. Formatting and whitespace checks passed. The original
checkout still has exactly its pre-existing deletion and merge-conflict status.

Commands, starting from the original checkout for the first two commands and
then from the new worktree:

```sh
git fetch --no-tags origin refs/heads/codex/astra-open-holdout-ledger-20260912:refs/remotes/origin/codex/astra-open-holdout-ledger-20260912
git worktree add -b codex/astra-retained-taker-consent-20260912 /tmp/percolator-prog-astra-retained-taker-consent-20260912 refs/remotes/origin/codex/astra-open-holdout-ledger-20260912
cd /tmp/percolator-prog-astra-retained-taker-consent-20260912
export CARGO_TARGET_DIR=/dev/shm/astra-retained-mark-cap-20260912-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=6 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
cargo build-sbf --tools-version v1.52 --offline -- --locked
CARGO_TARGET_DIR=/dev/shm/astra-retained-mark-cap-20260912-matcher-target cargo build-sbf --tools-version v1.52 --offline --manifest-path tests/fixtures/auth_matcher/Cargo.toml --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked
cargo test --locked --offline --test v16_program_stateful_fuzz inv_014_delayed_policy_and_policy_epoch_safety::retained_mark_fee_cap::v16_program_retained_batch_atom_cap_includes_computed_mark_fees -- --exact --nocapture
cargo test --locked --offline --test v16_program_stateful_fuzz -- --exact --nocapture --test-threads=1 \
  inv_014_delayed_policy_and_policy_epoch_safety::retained_taker_aggregate_cap::v16_program_retained_taker_aggregate_cap_rolls_back_deposit_and_policy_prefix \
  inv_014_delayed_policy_and_policy_epoch_safety::retained_permitted_policy_history::v16_program_retained_cpi_owner_budgets_ignore_permitted_policy_detours \
  inv_014_delayed_policy_and_policy_epoch_safety::v16_program_retained_batch_route_switch_preserves_fee_caps_and_funded_provider \
  inv_045_no_free_mark_movement::v16_program_trade_driven_mark_route_orders_converge_economically
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all
cargo fmt --all -- --check
git diff --check
git diff --cached --check
```

## Remaining Gaps

Labels **411 and 432 remain OPEN**. Single-CPI fee-bps/fee-atom conformance and
asymmetric dynamic LP consent are not proved here. This test does not cover
partial fills, split quantities across transactions, maximum-size batches,
passive OI determining the movement fee, underfunded fee collection, positive
minimum-mark-fee attenuation, backing or maintenance fees, Hybrid mode,
arbitrary policy/price histories, post-fill catchup, owner payouts or replay
after successful consumption. No holdout status or invariant verdict changes.
