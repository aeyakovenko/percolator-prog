# Row 424: native scanner rediscovery excludes synchronized surplus

Base: fetched `origin/main`, `c87f481093a2f47587826630a8075cc3b10a9b7a`.
Private worktree: `/dev/shm/astra-ultra-row424-terminal-recredit-20260917`.
Branch: `astra-ultra/row424-terminal-recredit-20260917`.

## Distinct coverage

[Multiwave rediscovery](row424_multiwave_scan_20260917.md) covers two backing
expiries on SPL. [Retired custody](row424_health_20260917.md) covers missing SPL
beneficiary custody. Neither combines scanner rediscovery with native custody.
The existing native-prefix reclassification selector has no spent insurance or
expiry-funded rediscovery. Scope B native recredit uses the withdrawal consumer
after normalization, not the scanner behind an already persisted prefix.

This selector crosses both source sides with SyncNative before expiry or after
scanner rediscovery. It creates a two-asset native market publicly, deposits
1237 user atoms, spends 100 insurance atoms against a 200-atom gain, and pays
users exactly 1200/0/137 before deleting their portfolios. A later asset retains
61 fresh backing atoms until slot 44. After cursor 1 persists, a public System
transfer donates 53 unsynchronized lamports to the native vault. Together those
114 physical atoms exceed historical spend; only 61 are booked residual.

The scanner must rewind from 1 to 0 at expiry, preserve the earlier asset's
stored bytes during normalization, and itself restore exactly 61 insurance
atoms on its next call. SyncNative timing cannot increase recovery or satisfy
the pre-expiry claim. An absent beneficiary receives exactly 61 through unsigned
native SPL payment. Final closure transfers the 53 external surplus atoms to
the administrator's native token account and refunds only exact market/vault
rent to its wallet. No native value is burned.

## Oracles and boundaries

The input-owned oracle checks all four source domains, fresh backing,
receivables, insurance budgets/spend, zero liens/positive bounds/open interest,
decoded aggregate totals, stock and reservation censuses. The resolved payout
ledger changes only by the 61-atom expired-backing addition to its residual
snapshot. Cursor, authority epochs and sibling sequences are checked separately.

Complete native token Accounts are predicted from empty rent-bearing Accounts,
input amounts and synchronization state. Token amounts, lamports and total
tracked lamports reconcile independently; mint and prior user payouts remain
exact. The transaction frame covers all compiled and tracked Accounts, including
closed portfolios, and permits only exact payer signature fees on failure.

Seven rejected transactions per history cover pre-expiry scan/payment locks;
expiry, rediscovery and successful native payment before an invalid System
suffix; unpaid retirement; payment before an invalid suffix; exhausted payment
replay (53-token custody fails before the epoch check); and successful surplus
transfer/vault closure before an invalid suffix.
Wrapper/SPL success-log counts prove execution of each rollback prefix. Identical
instructions retry successfully where applicable. Four histories require seven
committed campaign transactions each and finish with exact tombstone rent.

Construction uses existing public native-market initialization, System, ATA, SPL
and wrapper instructions, signer funding and Clock advances. The unchanged native
helper supplies LiteSVM's omitted canonical native-mint genesis account. There
are no injected market/portfolio bytes or engine transition calls. Packing in
`native_frame` builds expected Account values only; it never writes to LiteSVM.
The harness accepts absent or zero-lamport, empty closed portfolio Accounts.

## Validation

All three exact selectors below pass separately: **1 passed, 0 failed** each.
The new selector checks **4 histories, 28 commits, 28 exact rollbacks and 4 scanner
rediscoveries**. Peaks are **218,682 CU** overall, **23,535** for scanner calls,
**168,418** for rejected transactions and **127,356** for custody/closure.
The overall peak includes user cleanup; initial funding/configuration is excluded.
The existing SPL scan-recredit and native-prefix controls peak at **224,593** and
**33,758 CU**, respectively. No broad suite or engine proof ran. Scoped rustfmt,
working/staged whitespace checks, the protected-path guard and post-commit
`git show --check` pass. The existing Solana future-compatibility warning remains.

Initial development runs corrected two test assumptions: LiteSVM may retain a
closed zero-lamport Account, and an exhausted replay fails token-balance preflight
before checking its old authority epoch. Neither required a production change.

Private host/SBF targets were copied from the existing public-gap caches, then
SBF rebuilt locked/offline in this worktree using platform-tools v1.52 and engine
`4db11a8cb0053815e23a35d3a7d3edc265d8d866`. Wrapper SHA-256:
`a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row424-terminal-recredit-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-ultra-row424-terminal-recredit-20260917-sbf-target/deploy/percolator_prog.so
CARGO_TARGET_DIR=/dev/shm/astra-ultra-row424-terminal-recredit-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/astra-ultra-row424-terminal-recredit-20260917-sbf-target/deploy -- --locked
cargo test --locked --offline --test v16_cu inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_scan_recredit::native_surplus::v16_program_native_terminal_scan_excludes_synced_surplus_from_rediscovered_insurance -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_scan_recredit::v16_program_terminal_scan_rediscovers_earlier_insurance_after_later_expiry -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_native_reclassification::v16_program_native_sync_after_terminal_prefix_reclassifies_only_external_surplus -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_070_terminal_scan_recredit.rs tests/invariants/cu/inv_070_terminal_scan_native_surplus.rs
git diff --check
git diff --cached --check
git diff --exit-code c87f481093a2f47587826630a8075cc3b10a9b7a -- . ':!tests/invariants/cu/inv_070_terminal_scan_recredit.rs' ':!tests/invariants/cu/inv_070_terminal_scan_native_surplus.rs' ':!tests/invariants/README.md' ':!tests/invariants/row424_native_scan_surplus_20260917.md'
git show --check --oneline HEAD
```

## Remaining gaps

Row 424 remains OPEN for pending receipts/obligations during scanning, Recovery,
dual custody, repeated native recredit, maximum shapes, unavailable rent or
retirement authority and arbitrary environmental schedules. This finite witness
adds INV-070/071 scanner/custody composition, not a general invalidation theorem.
Optional insurance ledgers and earned-fee custody are outside scope. No production
mismatch or status promotion is claimed. Only the new CU child, its mount, this
note and README change; production, fixtures, shared helpers, Cargo and TSVs do not.
