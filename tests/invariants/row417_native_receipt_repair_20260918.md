# Row 417 atomic native receipt repair, 2026-09-18

Base: `origin/main`, `cb05fc229f75bc2eca6b747d3e9c4faec432c4d8`.
Worktree: `/dev/shm/astra-ultra-row417-coverage-20260918`.
Branch: `qa/row417-coverage-20260918`; local commit only, no push.

## Increment and duplicate evidence

One new exact LiteSVM selector in `cu/inv_067_native_receipt_redemption.rs`
reuses `World::before_native_receipts`, `Book`, and `materialize` unchanged.
Public native payouts of 1,116/1,217 leave two underfunded receipts. The first
owner closes its funded native ATA, redeeming 1,116 lamports plus rent while
preserving its receipt. At slot 13, a keeper-only transaction recreates that
ATA, releases 350 backing atoms, pays the peer and then the retained claimant.

An invalid System suffix rejects after ATA creation and both SPL transfers.
Logs confirm the successful ATA call, three wrapper calls and two transfers.
Every compiled Account and the wider economic frame must exactly restore,
including the absent ATA, native vault/peer backing lamports, receipt counters,
source stock and mint. The payer loses only its exact single-signature fee,
recovering all rent spent by the aborted repair.

The same four instructions then commit. Input-derived junior floors move from
`face * 501 / 3000` to `face * 851 / 3000`: the new ATA receives only 82 atoms,
the peer receives 151, and the payer spends exactly native ATA rent plus fee.
Full native Account images and receipt fields, owner/incarnation/position
identity, stock censuses and a zero-payment retained retry preserve prior value.
The suffix stops here; the third claimant and final retirement use existing coverage.

Existing evidence compared:

- The same file's native-redemption control commits expiry and ATA recreation
  separately. Its failed claim sees missing custody before any repair; it does
  not undo a funded ATA repair and successful native payments together.
- `cu/inv_082_receipt_destination_recovery.rs` already aborts expiry/repair/receipt
  payment, but uses ordinary SPL and saves prior payouts in token accounts. It
  does not redeem funded native custody or restore transferred backing lamports.
- `cu/inv_073_native_recredit_custody.rs` composes native repair/payment rollback
  after user liabilities are settled, without active receipt entitlement.
- The Row 417 health, full-payment, Recovery-forfeit and destination-recreation
  histories cover other receipt/expiry boundaries. This adds no new amount/order
  matrix or broad Row 417 closure claim.

## Exact validation

Private host/SBF caches were copied outside the worktree. The default-feature
wrapper and pinned engine were rebuilt locked/offline with platform-tools v1.52.
Engine: `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
Wrapper SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
Neither selector needs matcher files; fixture paths were not changed.

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row417-coverage-20260918-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-ultra-row417-coverage-20260918-sbf-target/deploy/percolator_prog.so
family=inv_067_terminal_payout_completeness_and_exact_once_settlement::native_receipt_redemption
cargo test --locked --offline --test v16_cu "$family::v16_program_native_receipt_repair_and_expiry_roll_back_with_paid_suffix" -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu "$family::v16_program_native_receipt_redemption_preserves_topups_and_rounding_beneficiary" -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_067_native_receipt_redemption.rs
git diff --check
git diff --cached --check
git show --format= --check HEAD
```

Both exact selectors pass, one test per invocation: new **339,081 CU** peak
(1.05s), control **173,615 CU** peak (4.43s). Only these selectors ran. Peaks
cover the calls recorded by the existing fixture and suffix, not all fixture
initialization. The new multi-instruction test uses the existing 600,000-CU
receipt-repair bundle limit; the control's 300,000-CU limit is unchanged.
Development corrected a raw-transaction return type and an inappropriate
single-call CU bound after every new economic assertion had passed.
Logs: `/dev/shm/astra-ultra-row417-coverage-20260918-{new,control}.log`.

Scoped rustfmt, diff/show whitespace and three-file scope checks pass. Production,
Cargo files, fixtures, TSVs, support helpers and all existing test bodies are
unchanged. Row 417 remains OPEN: insurance recredit, Recovery, unavailable owners,
dual rails, arbitrary histories and final retirement are outside this one history.
