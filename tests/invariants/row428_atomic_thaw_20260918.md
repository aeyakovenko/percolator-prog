# Row 428 atomic destination thaw, 2026-09-18

Base: `origin/main`, `dd24f45ab299511d7f46b0b0b6da1a03d12e3740`.
Worktree: `/dev/shm/astra-ultra-row428-coverage-20260918`.
Branch: `astra-ultra-row428-coverage-20260918`; local commit only, no push.

One selector in [the existing retry file](cu/inv_008_resolved_debit_retry.rs)
reuses the public SPL market fixture with a separate mint freeze authority.
It funds 60 insurance atoms, revokes minting, resolves and freezes the destination.
All five transaction envelopes are signed and serialized before delivery:

1. The retained 29-atom debit rejects the frozen destination with
   `InvalidTokenAccount`, preserving the epoch and uninitialized ledger.
2. A thaw and 29-atom payout complete before a duplicate debit rejects with
   `EngineStale`. Rollback restores the frozen Account, custody, budget, epoch
   and uninitialized ledger together.
3. The pre-signed thaw/payout continuation commits and advances the epoch once.
4. The retained old debit rejects stale while the remaining 31 atoms can fully
   fund its 29-atom request.
5. The pre-signed successor epoch drains the remaining 31 atoms. Total receipts
   are 60, insurance and vault are zero, and the epoch advances exactly twice.

The beneficiary signs none of these deliveries. Repair transactions require the
keeper and separate mint freezer; standalone withdrawals require only the keeper.
Every rejection compares all compiled and watched Accounts, with only the exact
signature fee deducted from the payer. Success logs prove thaw and SPL transfer
completed before the stale suffix. Checkpoints compare decoded market state,
control sequences, complete token and ledger Accounts, unchanged accounts and
stock/encumbrance censuses. No economic account bytes are injected or restored.

## Non-duplicate evidence

- [The recent Row 428 retry control](row428_regression_health_20260917.md)
  covers successive debit epochs and late failure with unchanged custody state.
- [Frozen insurance remainder](cu/inv_073_frozen_insurance_remainder.rs)
  keeps the paid destination frozen and pays replacement custody; it never thaws.
- [Destination epoch repair](cu/inv_008_insurance_destination_epoch_retry.rs)
  changes token ownership and an oracle role. This increment repairs a freeze and
  exercises epoch consumption by the debit itself, without a role handoff.
- Existing terminal quote thaw cases concern `CloseSlab`, not retained insurance
  debit consent. The added dimension is atomic thaw/payout/consumed-epoch rollback.

Row 428 remains COVERED. This is one Resolved, single-asset, classic SPL history,
not an independent withdrawal sequence or arbitrary-history proof. It does not
add native redemption, new funding, liabilities, recredit or retirement coverage.

## Verification

New selector: **1 passed, 0 failed**, 1,486 filtered; 0.38s, peak **49,692 CU**.
It checks five transactions, three exact rollbacks, one restored thaw/payout and
two committed payouts. Nearest existing control: **1 passed, 0 failed**, 1,486
filtered; 3.26s, eight histories, peak **72,101 CU**. No other selectors ran.
The new history has a 200,000 CU ceiling; address-dependent CU can vary.

Private dependency-cache copies were used; SBF and host code were rebuilt from
this checkout, locked/offline. No matcher is needed. Engine pin:
`4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
SBF SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
Logs: `/dev/shm/astra-ultra-row428-coverage-20260918-{sbf,new,control}.log`.

```bash
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row428-coverage-20260918-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-ultra-row428-coverage-20260918-sbf-target/deploy/percolator_prog.so
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
env CARGO_TARGET_DIR=/dev/shm/astra-ultra-row428-coverage-20260918-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/astra-ultra-row428-coverage-20260918-sbf-target/deploy -- --locked
cargo test --locked --offline --test v16_cu inv_014_delayed_policy_and_policy_epoch_safety::reserve_debit_epoch::resolved_debit_retry::v16_resolved_retained_debit_restores_atomic_thaw_before_consuming_epoch -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_014_delayed_policy_and_policy_epoch_safety::reserve_debit_epoch::resolved_debit_retry::v16_resolved_retained_debit_retries_restore_epochs_across_signed_and_unsigned_delivery -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_008_resolved_debit_retry.rs
git diff --check
git diff --cached --check
git diff --exit-code dd24f45ab299511d7f46b0b0b6da1a03d12e3740 -- . ':!tests/invariants/cu/inv_008_resolved_debit_retry.rs' ':!tests/invariants/row428_atomic_thaw_20260918.md' ':!tests/invariants/README.md'
git show --check HEAD
```

Scoped formatting, whitespace and the protected-path guard pass. The existing
Solana-client future-compatibility warning remains. Only the test file, this note
and README change; production, dependencies, fixtures, helpers and TSVs are untouched.
