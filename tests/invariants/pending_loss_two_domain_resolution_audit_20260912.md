# Row 419: Two Bankruptcy Domains Across Resolution

Base: `origin/codex/astra-open-holdout-ledger-20260912` at
`fe01ae243c561a2331d956897caedc71b06c36d1`.
Local branch: `codex/row419-obligation-order-20260912`.
Isolated worktree and build: `/tmp/percolator-row419-obligation-order`.
The coordinator worktree was not edited. No push was performed.

## Retained Coverage

`cu/inv_039_pending_loss_two_domain_resolution.rs` adds one LiteSVM selector under
`inv_039_pending_loss_obligation_durability::close_reopen::two_domain_resolution`.
System, SPL, ATA and wrapper instructions construct all economic state. Clock and
authenticated marks drive time and accrual; no engine account bytes are seeded.

Eight histories cross both side orientations, either domain booking first in Live,
and both claimant orders. The holders have opposite sides and unequal weights
(one and two lots). Their input-derived gains are 200,000 and 280,000 atoms; their
opposing debtors contribute 180,000 and 250,000 principal, leaving distinct 20,000
and 30,000 bankruptcy residuals.

Both flat holders retain their original obligations when only one debtor books B.
Resolution preserves both portfolio Accounts and close ledgers. The first holder
then settles its own exact debit and releases its weight while the second domain
still has an active residual. Neither that release nor the second debtor's later
booking can mutate the other participant's Account or erase the second holder's
original retained leg. The first holder's payout waits until the second holder
absorbs its own B debit.

The oracle checks owner capital, signed PnL, unpaid receipt face and external SPL
tokens against input-derived entitlements after each prescribed terminal step. It
also checks both complete close partitions, OI, weights, pending/stored counts,
aggregate capital/PnL, and fixed initial token supply. Final payouts are exactly
`[380000, 0, 550000, 0, 777]` in every history. All portfolios become terminal and
are deleted; booked and SPL custody, capital and positive PnL reach zero.

There are eight rejected transactions containing both the first holder's debit
and the second debtor's residual booking, plus sixteen rejected claimant-payment
prefixes. The shared rejection oracle requires every prefix to succeed before the
specified error and compares complete tracked/transaction Accounts, including
payer lamports minus signature fees. Four premature claims and forty terminal
retries also reject with exact rollback.

This adds sampled INV-039/024/037/041/048/067/073/081 evidence. It does not promote
INV-066/076/086 or discharge row419's broader history requirements. Row419 remains
**OPEN**. Production code, dependencies and invariant verdicts are unchanged.

## Nonduplication And Limits

- Existing dual-close locality coverage stops at live residual progress. This
  test carries both domains through mixed booking, resolution and terminal claims.
- The row419 preemption selector has one bankrupt cohort. The resolved-history,
  funded-resolution and shared-holder matrices carry solvent opposing debts.
- Cured-close debt, single-cohort expiry, funding-only, shared-holder and backing
  expiry candidates were rejected during source review as already covered. No
  additional executable test was retained for those candidates.
- The retained test has no cure, fees, funding, explicit insurance/backing top-ups,
  post-close price drift, fractional positions, expiry, reset/restart or slab
  retirement. It is not a generic generator or reference/deployed equivalence proof.

An exploratory assertion that a waiting retained holder's immediate retry must
return `EngineNonProgress` was discarded. The retry instead returned success with
an identical tracked economic frame. In engine pin
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`, the actionable summary treats a nonempty
active bitmap as pre-payout progress, while resolved detach can still be blocked
by a pending domain loss barrier. The wrapper's `handle_close_resolved` delegates
to that engine selector. This is an engine progress-classification observation,
not an established LoF/DoS: the prescribed opposing-debt continuation succeeds,
then every owner receives the exact entitlement and exits. No engine change or
wrapper workaround is included. The retained test makes no claim that every idle
retry before debt settlement rejects. Temporary diagnostic output was removed.

## Validation

Default-feature SBF rebuilt locally with platform-tools v1.52. Artifact SHA-256:
`c8b584ed01570396e1031a1d4566e2ebc3b48781056f1297e7bde40905f4694d`.
New selector: 1/1 passes, eight histories. Peak measured setup trade/crank is
326,760 CU; peak measured terminal/rollback continuation is 239,018 CU.
Nearest controls: 3/3 pass. Charter/index: 1/1 passes. Formatting and working-tree,
staged and committed whitespace checks pass. Existing dead-code and Solana future
compatibility warnings do not affect these results.

Commands use the isolated build directory:

```sh
export CARGO_TARGET_DIR=/tmp/percolator-row419-obligation-order/target
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu inv_039_pending_loss_obligation_durability::close_reopen::two_domain_resolution::v16_program_two_bankrupt_domains_preserve_pending_debt_across_resolution_order -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_039_pending_loss_obligation_durability::close_reopen::close_preemption::v16_program_expired_bankrupt_close_preserves_pending_cohort_entitlement_across_routes \
  inv_039_pending_loss_obligation_durability::close_reopen::cure_resolution::v16_program_canceled_close_keeps_debt_through_pending_release_and_resolution \
  inv_039_pending_loss_obligation_durability::resolved_histories::v16_program_resolved_debtor_deletion_preserves_unsettled_cohort_attribution
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
```
