# Retained Reserve Stock at Authenticated Expiry

Date: 2026-09-12.
Base: `origin/codex/astra-open-holdout-ledger-20260912`,
`798c30d815892ac5283a62add9bfcc7f15b3a435`.
Worktree: `/tmp/percolator-retained-stock-20260912-NQhcHD`.
Branch: `codex/astra-retained-stock-20260912-NQhcHD`.

Only the requested branch was fetched. Inputs were this base's source, tests,
docs and pinned engine source. No open PR diff, other branch implementation,
production edit, Cargo edit, original-checkout edit or push was used.

## Distinct Coverage

| Existing test | Added relation |
| --- | --- |
| `inv_005_retained_debit_matrix` | Existing mixed families lose authority/incarnation binding before payout. Here both reserve bindings remain unchanged; principal expires while earned fees remain payable in the same domain. |
| `inv_008_retained_reserve_replenishment` | Existing paid insurance crosses refill and operator ABA with zero encumbrance. Here a real backing lien survives principal/earnings payouts and authenticated expiry admission. |
| `inv_014_retained_fee_stock` | Existing fee-producing trades compose with insurance payouts and refill. Here an expired principal suffix rolls back an earned-fee SPL payout and its auxiliary ledger. |
| `v16_program_backing_principal_release_respects_authenticated_expiry` | Existing principal-only expiry matrix has a live claim. The new bundle also owns earned fees, a nonzero valid backing lien, both payout orders and an unchanged retained fee continuation. |
| `v16_program_retained_source_fees_survive_repricing_policy_and_settlement_orders` | Existing earned fees cross repricing, trade-policy rejection and exit ordering. The new rejection is principal expiry with no authority, policy, trade or portfolio-sequence change. |

The new test is
[`stateful/inv_063_retained_reserve_stock.rs`](stateful/inv_063_retained_reserve_stock.rs),
mounted in `stateful/inv_063_backing_expiry_normalization.rs` as
`retained_reserve_stock`. Its exact selector is:

`inv_063_backing_expiry_normalization::retained_reserve_stock::v16_program_retained_principal_expiry_preserves_encumbered_backing_and_earned_fee_stock`

## Public History and Oracle

Twelve independently constructed worlds cross asset slots 0/1, both principal/
earnings instruction orders and authenticated slots 4/5/6 around expiry slot 5.
The backing provider and network payer are distinct from both traders. Public
funding supplies 100,000 provider atoms; trader deposits are 52,502 and 2,000,000.
A 1,000-lot position moves from 100 to 105. Public settlement transfers 5,000
atoms out of the loser's capital into source backing, while the winner retains a
5,000-atom source claim. A 50-lot increase requires a 2,623-atom backing lien and
charges `ceil(2,623 * 3,333 / 10,000) = 875` earned-fee atoms to winner capital.
Fresh source backing is 105,000 atoms, including 2,623 liened atoms; the provider
ledger separately records the 100,000-atom deposit. Insurance is zero.

Four envelopes are signed and successfully simulated before Clock advances:
137-atom principal, 875-atom earnings, their ordered bundle, and a signature-
distinct copy of the earnings request. Simulations preserve every tracked and
compiled Account. Deliveries compare the retained serialized bytes. There is no
blockhash refresh, payload rebuilding or state restoration between retention and
delivery; the engine's stored slot stays at 2 while authenticated Clock advances.

Before expiry, the original bundle pays exactly 1,012 atoms. At and after expiry,
the principal instruction rejects with `EngineStale`; when earnings come first,
wrapper/SPL success logs prove the 875-atom prefix executed and was rolled back.
The original standalone principal envelope also rejects, while the original
standalone earnings envelope pays its 875 atoms. Every world then submits the
other pre-signed earnings envelope: `EngineLockActive` and complete rollback prove
that exhausted earnings cannot consume still-abundant backing principal or vault
custody. That last rejection is a current-stock bound, not an intent watermark.

Every economic checkpoint reconciles input-derived vault/capital/source-backing
amounts, provider principal and earned-fee ledger fields, all backing buckets,
nonzero lien preservation, unrelated source domains, asset/control/profile state,
complete SPL endpoints and fixed token supply. Complete tracked/compiled Accounts
preserve portfolios and unrelated state on successful payouts and restore every
rejected transaction. Non-payer lamports never change; the payer loses exactly
the signature fee for each delivery. Independent public stock and encumbrance
censuses run at each checkpoint. V16Svm supplies empty account allocations and
SPL fixtures; initialized economic state changes only through public instructions.

## Remaining Gaps

Labels **#415/#428 remain OPEN**. This increment adds stock and authenticated-
expiry evidence for INV-024/031/036/063/080, not standalone exact-once reserve
authorization. It never submits a paid insurance or backing-principal payload
against replenishment, nor a paid earned-fee payload against newly earned fees.
The current withdrawal authority epoch stays unchanged throughout these payouts;
the stock rejection must not be presented as successful-debit epoch consumption.

Clock expiry is tested before public normalization: the bucket still stores
`Fresh` and its valid lien. Normalized expiry/impairment, backing refill, insurance
encumbrance, shutdown/resolution, asset reuse, alternative token rails, arbitrary
histories and terminal owner/provider exits remain outside this test. Existing
invariant verdicts and holdout ledgers are unchanged.

## Verification

Wrapper and authenticated matcher SBF were rebuilt offline from this worktree
with locked dependencies, default features and platform-tools v1.52. Private
copies of the existing retained-reserve build caches supplied dependencies;
the host test target was rebuilt from this worktree.

- Wrapper SHA-256: `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
- Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
- New selector: 12 worlds, 48 simulations, 12 successful deliveries, 28 exact
  rollbacks, four rolled-back earned-fee SPL payouts; peak CU **444,560** within
  the fixture's transaction budget.
- Final exact selector run: **6/6 passed**, including the new selector, all three
  named retained-reserve controls, the principal-expiry control and retained
  source-fee history. Invariant charter/index: **1/1 passed**.
- `cargo fmt --all -- --check` and `git diff --check` passed. Production, Cargo
  files and invariant/holdout verdicts match the base. The original checkout's
  pre-existing dirty status is unchanged. Existing unused-support and Solana
  future-compatibility warnings remain.

Development corrected the independent fixture ledger to include the loser's
5,000-atom settlement and used the fixture transaction budget for the two-payout
CU bound. No production conformance failure was observed.

Commands from this worktree:

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-retained-stock-NQhcHD-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 TMPDIR=/dev/shm
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
cargo test --locked --offline --test v16_program_stateful_fuzz -- --exact --nocapture --test-threads=1 \
  inv_063_backing_expiry_normalization::retained_reserve_stock::v16_program_retained_principal_expiry_preserves_encumbered_backing_and_earned_fee_stock \
  inv_005_authority_incarnation_binding::retained_debit_matrix::v16_program_retained_debit_permutations_preserve_independent_budgets_after_binding_changes \
  inv_008_intent_uniqueness_and_bounded_replay::retained_reserve_replenishment::v16_program_paid_retained_insurance_stays_bound_after_replenishment_and_operator_aba \
  inv_014_delayed_policy_and_policy_epoch_safety::retained_fee_stock::v16_program_retained_fee_stock_bundles_preserve_owner_budgets_across_policy_and_replenishment \
  inv_063_backing_expiry_normalization::v16_program_backing_principal_release_respects_authenticated_expiry \
  inv_036_fee_destination_and_policy_version_integrity::v16_program_retained_source_fees_survive_repricing_policy_and_settlement_orders
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
```

Logs: `/dev/shm/astra-retained-stock-NQhcHD-new.log`,
`/dev/shm/astra-retained-stock-NQhcHD-selectors.log` and
`/dev/shm/astra-retained-stock-NQhcHD-index.log`.
