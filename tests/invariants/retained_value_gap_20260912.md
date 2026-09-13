# Retained Fee and Insurance Stock Composition

Base: `origin/codex/astra-open-holdout-ledger-20260912` at `39022191`.
Branch: `codex/astra-retained-value-gap-20260912-k7m`.
Worktree: `/tmp/astra-retained-value-gap-20260912-k7m`.
Inputs were model reasoning and this base's local source/tests, searched with `rg`
before implementation. No open PR branch, patch or test was read or copied.
The shared checkout was not modified. Row 413 remains OPEN and separate;
434 is already covered and was excluded from this work.

## New Coverage

[`stateful/inv_014_retained_fee_stock.rs`](stateful/inv_014_retained_fee_stock.rs)
composes behaviors previously separated between INV-008 retained trade/deposit
retries, INV-014 retained fee bundles and INV-005 mixed retained debit budgets.
The new relation is rollback of an insurance payout together with fee-producing
trades, followed by an unchanged signed bundle under a permitted fee decrease.

The finite product crosses four routes, both trade directions, both payout/trade
orders and zero/19-bps successor policies. The original signed cap is 37 bps;
the unsigned LP has an independent 503-bps grant. Fractional quantity and a
100,003 price exercise two-stage fee rounding. Bilateral fills retain their
explicit fee; CPI fills charge the current base within the signed envelope.

Each world independently funds long/short insurance with 101/211 atoms, signs
nine initially executable alternatives and proves simulation changes no Account.
A duplicate-trade suffix rejects after both the 137-atom SPL payout and first
trade have executed. Complete tracked/compiled Accounts, matcher context,
position epochs and payer network fees verify exact transaction rollback.
The original clean bundle then succeeds byte-for-byte after policy relaxation.

An input-derived ledger checks each trader's capital, PnL, position and SPL
source/destination, token owners/mint, insurer funding and payouts, both domain
budgets, custody and fixed supply. It accounts for the different domain allocation
when the long-first payout occurs before versus after fee credit. All four
originally signed trade transports reject after consumption, both before and
after a fresh closing fee plus a 43-atom short-domain top-up. Fresh insurance
prefixes before those consumed payloads transfer one atom and roll back exactly.
No successful insurance request is replayed against replenished stock.

Fresh owner withdrawals and the final live insurance payout empty primary custody.
Complete SPL account bytes agree across direction, payout ordering and the
single/batch pair within each fee regime. Existing independent stock and
encumbrance censuses run at every economic checkpoint. V16Svm supplies its usual
empty account/SPL fixtures; all initialized economic transitions use public
instructions. No program-owned bytes, shared helpers, production code or pins
were changed.

## Holdout Boundary

| OPEN Rows | Partial Evidence Added | Still Unproved |
| --- | --- | --- |
| 411, 432 | Separate owner fee budgets survive permitted decreases, zero-fee execution, late rollback and cross-route consumed-intent retries | Retained single-CPI taker protection when policy exceeds signed terms; arbitrary policy histories |
| 415, 428 | Insurance payouts and fee-created stock roll back together; later fee/top-up stock cannot fund consumed trade retries; exact insurer attribution | Stock-sequence binding and successful insurance-debit epoch consumption, including replay against replenishment and lifecycle changes |

This adds partial INV-008/011/014/024/031/036/047/064/080/081 evidence.
No holdout or invariant verdict is promoted. No vulnerable pin was executed.
Resolved/shutdown insurance, partial fills, backing fees and arbitrary histories
remain outside this increment. No new public LoF/DoS was observed.

## Verification

Wrapper and authenticated matcher SBF were rebuilt from this checkout with locked,
offline dependencies and platform-tools v1.52; the host dependency cache was
privately copied from the base's retained-authority audit target.
Wrapper SHA-256: `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
The new selector covers 32 worlds, 288 simulations, 544 exact rollbacks,
288 rolled-back SPL transfers, 16 rolled-back matcher calls and 192 measured
successful transactions; peak CU is 175,119. Final exact selectors pass 3/3,
the invariant index passes 1/1, and formatting/whitespace checks pass.
Development corrected the fixture's fee-policy signer after authority transfer.

Exact commands from the isolated worktree:

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-retained-value-gap-20260912-k7m-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
cargo test --locked --offline --test v16_program_stateful_fuzz -- --exact --nocapture --test-threads=1 inv_014_delayed_policy_and_policy_epoch_safety::retained_fee_stock::v16_program_retained_fee_stock_bundles_preserve_owner_budgets_across_policy_and_replenishment inv_014_delayed_policy_and_policy_epoch_safety::retained_fee_bundle::v16_program_retained_shared_taker_fee_bundle_preserves_each_instruction_bound inv_005_authority_incarnation_binding::retained_debit_matrix::v16_program_retained_debit_permutations_preserve_independent_budgets_after_binding_changes
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
```
