# Row 424: scanner rediscovery through a custody repair rent shortfall

Base: `origin/main`, `dc65edbbdf4d4015579df87488f91887b31bcebb`.
Worktree: `/dev/shm/astra-ultra-row424-coverage-20260918`.
Branch: `astra-ultra/row424-coverage-20260918`; local commit only.

## Non-duplicate boundary

[Retired beneficiary custody](row424_health_20260917.md) tests missing and
prefunded SPL custody with a keeper funded with 1,000,000,000 lamports. It never
fails ATA creation for insufficient rent. The generated reserve-wallet selector
in `cu/inv_073_generated_reserve_wallets.rs` covers successful repair and hostile
suffix rollback, explicitly excluding recredit. The existing
[multiwave](row424_multiwave_scan_20260917.md) and
[native surplus](row424_native_scan_surplus_20260917.md) scanner histories keep
custody available. Those Row 424 notes list unavailable repair rent as a gap.

This increment adds one fixed history to `cu/inv_070_terminal_scan_recredit.rs`,
using the unchanged public `fixture(0, 61)`, `land` and `stocks_at_cursor` helpers.
The new boundary is a keeper with exactly ATA rent minus one lamport while an
independent transaction payer covers all signature fees. No new fixture, helper,
module mount or production change is needed.

## History and oracle

After cursor 1 persists, the beneficiary closes its empty ATA and its signing key
is dropped. At backing expiry, two successful CloseSlab prefixes normalize the
later backing and rediscover 61 earlier insurance atoms. ATA creation then fails
with System `ResultWithNegativeLamports` at transaction instruction 4. Complete
compiled/tracked Accounts roll back, restoring cursor 1, Fresh backing, insurance,
custody and keeper rent; only the exact transaction payer signature fee changes.

Standalone scans then commit expiry, cursor reset and the scanner's own 61-atom
recredit. Normalization preserves the earlier asset bytes. The same rent failure
at instruction 2 preserves the committed claim, and unpaid insurance still blocks
retirement. A public System transfer of exactly one lamport enables the unchanged
ATA-repair/unsigned-payout instruction pair. Its rent consumes the keeper's exact
balance, the recreated token Account matches the original with 61 tokens, and
the insurance authority epoch advances exactly once. The existing stock census
checks every domain, user balances, mint supply and reservations; the payout
ledger changes only by the expired backing addition. Final CloseSlab preserves
the paid insurance and mint, pays no admin tokens, closes the vault and returns
exact market/vault excess rent above the typed tombstone.

## Validation

Only the following two exact selectors ran, each once and each passing 1/1:

- New: one history, seven commits, three exact rollbacks; peak **216,937 CU**.
- Nearest control: four histories, 28 commits, 20 exact rollbacks; peak **221,593 CU**.

Both use the existing 400,000-CU bound. Peaks include fixture user-payout cleanup
and campaign transactions, including failures; earlier construction is excluded.
Private host/SBF caches were copied from `percolator-public-gap-20260916-c91e`.
SBF was rebuilt here locked/offline using platform-tools v1.52 and the unchanged
engine `4db11a8cb0053815e23a35d3a7d3edc265d8d866`. Wrapper SHA-256:
`a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
Logs: `/dev/shm/astra-ultra-row424-coverage-20260918-logs/{build-sbf,new,control}.log`.

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row424-coverage-20260918-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-ultra-row424-coverage-20260918-sbf-target/deploy/percolator_prog.so
CARGO_TARGET_DIR=/dev/shm/astra-ultra-row424-coverage-20260918-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/astra-ultra-row424-coverage-20260918-sbf-target/deploy -- --locked
cargo test --locked --offline --test v16_cu inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_scan_recredit::v16_program_rediscovered_insurance_retries_one_lamport_short_custody_repair -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_scan_recredit::v16_program_rediscovered_insurance_recreates_prefunded_beneficiary_before_retirement -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_070_terminal_scan_recredit.rs
git diff --check
git diff --cached --check
git show --check --oneline HEAD
```

Targeted formatting, working/staged whitespace and post-commit whitespace checks
pass. Only the existing CU file, this note and README change. No broad suite or
engine proof ran; the existing Solana future-compatibility warning remains.
Row 424 stays OPEN. Permanent absence of a rent sponsor, pending receipts,
Recovery, native/dual custody, repeated expiry, maximum shapes and unavailable
retirement authority remain outside this single-history increment. No production
defect, generic liveness guarantee or invariant/TSV status promotion is claimed.
