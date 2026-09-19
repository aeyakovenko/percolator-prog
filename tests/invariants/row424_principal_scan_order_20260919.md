# INV-070: provider withdrawal around a persisted terminal prefix

Base: `origin/main` at `0ad6eaeb31ec3a4813851b38dfe91af78137d7d9`.
Isolated worktree: `/tmp/percolator-inv070-scan-prefix-20260919`.
No PR diffs inspected; the open relation is validation only. No production bug
found, status promotion, or claim that row 424 is closed.

The new `principal_order` selector crosses both source sides with provider
withdrawals of 107, 246, or all 307 backing atoms at expiry-1, before or after
CloseSlab caches the earlier spent-insurance prefix. The full-withdrawal case
before scanning is the uncached control. Partial withdrawals preserve the
cursor, then authenticated expiry requires reset and scanner rediscovery;
full withdrawal removes the backing wait without creating recoverable residue.
Principal and insurance withdrawals are permissionless wrapper instructions.
The shared fixture constructs economic state through public transactions;
Clock warp is the only out-of-band environmental change after construction.

Input arithmetic requires final `(principal, insurance, burned residue)` of
`(107, 100, 100)`, `(246, 61, 0)`, or `(307, 0, 0)` in both scan orders.
Checks cover every source domain, immutable payout-ledger fields, exact released
residual, authority sequences, custody, mint supply, earlier-slot framing,
stock/encumbrance censuses, strict finite rank descent, vault closure, typed
tombstone and exact rent. Rejected withdrawal, expiry/recredit and close bundles
frame complete Accounts and verify successful instruction prefixes before rollback.
A retained one-atom principal probe rejects at the authenticated deadline while
custody remains; the fully drained control rejects in token-balance preflight.

Net-new boundary: existing scan recredit, expiry-wave, native-reclassification,
custody-repair and regression-health coverage does not interleave a provider's
partial/full principal withdrawal with the cached earlier insurance prefix.
This tests the amount and existence of subsequent actionability, including a
full-drain control, with terminal outcomes independent of scan order. Evidence
is finite for INV-070 and related INV-024/025/033/041/063/069/071/086/088.

Validation: new exact selector passes 12 histories, 58 commits and 64 exact
rollbacks, maximum observed peak 229,449 CU (400,000 ceiling). Adjacent original scanner selector
passes. Scoped rustfmt and `git diff --check` pass. Cached default-feature SBF
SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
Production sources and Cargo files match its main-recorded build base `2e88c39b`.
Host dependencies were cached in a private target; the test binary was rebuilt.

```sh
export TMPDIR=/dev/shm CARGO_TARGET_DIR=/dev/shm/percolator-inv070-scan-prefix-20260919-host
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-inv077/target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_scan_recredit::principal_order::v16_program_terminal_scan_recomputes_residual_after_provider_withdrawal_order -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_scan_recredit::v16_program_terminal_scan_rediscovers_earlier_insurance_after_later_expiry -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --check --config skip_children=true tests/invariants/cu/inv_070_terminal_scan_principal_order.rs tests/invariants/cu/inv_070_terminal_scan_recredit.rs
git diff --check
git diff 2e88c39b HEAD -- src Cargo.toml Cargo.lock
sha256sum /dev/shm/percolator-inv077/target/deploy/percolator_prog.so
```
