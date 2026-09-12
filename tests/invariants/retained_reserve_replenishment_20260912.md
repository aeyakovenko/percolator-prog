# Retained Reserve Replenishment, 2026-09-12

Base: `origin/codex/astra-open-holdout-ledger-20260912` at
`e660a52e7efef3c54041d26d78480107f7cc76f9`.
Worktree: `/tmp/percolator-astra-retained-reserve-20260912-c7f4`.
Branch: `codex/astra-retained-reserve-once-20260912-c7f4`.
Only current local source, tests and docs informed this increment. No fetch, PR
diff copying, shared-checkout edits, production changes or push.

## Overlap Search

Local `rg` searches covered `retained`, `replenish`, `InsuranceWithdrawal`,
`WithdrawInsuranceAsset`, `WithdrawBackingBucket`, `authority_epoch`, and
`stock_sequence` in the wrapper, fixture/discovery helpers and INV-008/014/024/
064/080 tests and audits. Relevant existing coverage was read before implementation.

| Existing Coverage | New Relation |
| --- | --- |
| INV-008 retry registry | Already includes insurance replenishment. This increment adds exact domain/recipient accounting, both replenishment/ABA orders, a late top-up rollback and an unaffected retained peer. |
| INV-005 retained debit matrix | Invalidates one family before its first payout. Here the selected retained debit has already paid before replenishment and authority reuse. |
| INV-014 retained fee stock | Rolls insurance payout back with fee-producing trades; does not replay a successful reserve payload. |
| INV-014 fee bundle / permitted policy history | Independently bounded trade fees and consumed trade retries; no paid reserve/authority/replenishment composition. |
| INV-064 payout schedules / optional ledger history | Finite live/resolved budgets and telemetry equivalence; no paid retained request crossing operator ABA with a top-up prefix. |

## New Selector

`inv_008_intent_uniqueness_and_bounded_replay::retained_reserve_replenishment::v16_program_paid_retained_insurance_stays_bound_after_replenishment_and_operator_aba`

The narrow [mounted test](stateful/inv_008_retained_reserve_replenishment.rs)
crosses asset slots 1/2, a 137-atom partial payout with long-domain refill versus
a 312-atom full payout with short-domain refill, and refill before/after operator
A-to-B-to-A. Initial long/short budgets are 101/211; a separate asset/recipient
owns 43 atoms. Funding authority, payout operator, temporary successor, peer
operator and network payer are distinct actors.

Three original envelopes are signed and simulated without mutation: the paid
withdrawal, a signature-distinct copy of its payload and the peer payout. The
first withdraws exactly its signed amount, taking long stock first. Replenishment
restores the selected reserve to 312 atoms. Public handoffs restore the original
operator while advancing only its asset's authority epoch twice. After both
changes, the original stale envelope rejects with `EngineStale` at instruction 2.

A fresh 17-atom top-up followed by the same stale withdrawal payload rejects at
instruction 3. Wrapper/SPL success logs establish that the prefix executed; full
tracked and compiled Accounts prove rollback of custody, source balance, budget
and top-up sequence. The separately retained, simulated top-up then succeeds
byte-for-byte. Fresh withdrawal consent pays the resulting 329 atoms; the peer's
original signed envelope pays its 43 atoms. Primary custody ends at zero. The
original withdrawal payload's cumulative committed debit is exactly 137 or 312.

Every checkpoint checks full SPL endpoint Accounts, owner/mint, fixed supply,
vault/insurance and every domain budget, asset market IDs, all authority/control
sequences and role profiles, zero portfolio capital, independent stock and
encumbrance censuses. Initial funding is checked against fixture endowments.
Each transaction preserves unrelated Accounts and all non-payer lamports. The
payer's complete Account equals its post-setup baseline less exact signature
fees, totaling 105,000 lamports per world. Setup fees use the fixture's ordinary
payer and are outside that measured history. Optional insurance telemetry is
absent. V16Svm supplies empty allocations and SPL fixtures; all initialized
economic transitions use public instructions, with no snapshot restoration.

## Boundary and Remaining Gaps

The current withdrawal wire binds amount, asset generation and authority epoch;
the withdrawal handler does not advance the authority epoch. This test asserts
that fact and uses explicit public authority updates for stale-request rejection.
It does not establish an intrinsic withdrawal stock sequence or successful-debit
epoch consumption. Replenishment alone, without authority change, is not replayed.

Labels **#415/#428 remain OPEN**. Standalone reserve consumption, backing principal/
earnings, expiry/impairment, asset reuse, shutdown/resolved transitions, nonzero
encumbrance, alternative token rails and arbitrary histories remain outside this
increment. No holdout or invariant verdict is promoted. No marginal probe remains.

## Verification

Fresh locked/offline default-feature wrapper and matcher SBF builds used
platform-tools v1.52. Host dependencies came from a private copy of the existing
retained-value audit cache; host test sources were rebuilt from this worktree.
Wrapper SHA-256: `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

The new selector passes: 8 worlds, 32 simulations, 56 successes, 16 exact
rollbacks and 8 rolled-back SPL top-ups. Peak success CU is **49,713**; peak
rejection CU is **64,207**, both below the asserted 300,000 ceiling. The two
adjacent controls pass with peaks of **128,338** (mixed debit) and **175,119**
(fee stock). The final new-selector rerun after strengthening initial funding
and total network-fee checks also passes. The invariant index passes 1/1;
`cargo fmt --all -- --check` and `git diff --check` pass. Existing unused-support
and Solana future-compatibility warnings remain. Logs use the
`/dev/shm/astra-retained-reserve-once-20260912-c7f4-` prefix with `selectors.log`,
`final-new.log`, and `index.log` suffixes.

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-retained-reserve-once-20260912-c7f4-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 TMPDIR=/dev/shm
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
cargo test --locked --offline --test v16_program_stateful_fuzz -- --exact --nocapture --test-threads=1 \
  inv_008_intent_uniqueness_and_bounded_replay::retained_reserve_replenishment::v16_program_paid_retained_insurance_stays_bound_after_replenishment_and_operator_aba \
  inv_005_authority_incarnation_binding::retained_debit_matrix::v16_program_retained_debit_permutations_preserve_independent_budgets_after_binding_changes \
  inv_014_delayed_policy_and_policy_epoch_safety::retained_fee_stock::v16_program_retained_fee_stock_bundles_preserve_owner_budgets_across_policy_and_replenishment
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
```
