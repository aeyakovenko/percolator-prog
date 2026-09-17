# Row 424: two expiry waves behind a scanned insurance prefix

Base: fetched `origin/main`, `5d9413fc04a20816b220b4181d16c4467e5bc70e`.
Worktree: `/dev/shm/percolator-row424-multiwave-20260917`.
Branch: `codex/row424-multiwave-prefix-20260917`; local commit only.

## Duplicate analysis

- [Recent row424 coverage](row424_health_20260917.md) repairs missing beneficiary
  custody after one expiry. This increment retains available custody throughout.
- [Scope R](pr135_scope_r_terminal_scan_conformance_20260913.md) and the existing
  scan-recredit selector discover earlier insurance after one later expiry.
- `inv_071_generated_terminal_actionability.rs` already generates two expiry-funded
  recredits, but `run` selects `world.insurance(pay)` immediately after expiry.
  Its oracle therefore checks the withdrawal consumer, not scanner rediscovery
  after a previously committed recredit/payment and a newly persisted prefix.
- Generated prefix actionability covers staggered backing deadlines without this
  spent-insurance history. Native reclassification and prefix-custody actionability
  cover external denomination/surplus, not repeated earlier-insurance rediscovery.

## Increment and oracle

The new `terminal_scan_recredit::multiwave` child constructs two assets publicly.
Both sides are sampled. Asset zero spends 100 insurance atoms against a 200-atom
gain; user payouts are exactly 1200/0/137, followed by portfolio deletion. The
later asset has opposite-side backing buckets of 37 and 41/107 atoms expiring at
slots 44 and 48. All deposits occur while Live and mint authority is revoked.
No fixture file or shared helper changes; construction uses System/SPL/ATA/wrapper
instructions, with LiteSVM signer funding and Clock advances. No program-owned
Account bytes are injected or rewritten.

At each deadline the history is `cursor 1 -> expiry -> cursor 0 -> scanner
recredit -> unsigned payment`. The first scan restores 37 atoms, which are paid
before the next saved prefix. The second scan must restore 41/63 more, bounded by
remaining historical spend and newly released residual. Normalization leaves the
earlier asset's stored bytes exact in both waves. A Fresh sibling allows scan
progress before its wait rejects; the first unpaid-close rollback includes both
calls. Final unpaid insurance blocks retirement directly.

Seven rollback transactions per history cover both expiry/rediscovery prefixes,
both unpaid-close attempts, both SPL-paying prefixes, and final burn/close before
an invalid System suffix. The shared transaction oracle checks complete compiled
and tracked Accounts, including absence, allowing only exact payer signature fees.
Successful program/SPL log counts prove the prefixes executed. Second-wave rollback
must preserve the already paid 37 atoms and remaining spend of 63.

The input-owned oracle checks every domain's Fresh backing, reserve/spend,
receivable and zero liens/obligations; decoded global summaries and stock/reservation
censuses; actual user/reserve/vault balances; fixed mint supply; authority epochs;
and the resolved payout ledger with only cumulative expired backing added to its
residual snapshot. Recredit is predicted from released backing and remaining spend,
not the engine selector. Final closure pays the administrator no quote tokens,
burns exactly 0/44 atoms, deletes the vault, and reconciles the complete mint and
administrator Accounts plus exact tombstone rent.

## Exact validation

Both exact selectors pass separately, **1 passed, 0 failed** each on private SBF:

- New: `inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_scan_recredit::multiwave::v16_program_terminal_scan_rediscovers_remaining_insurance_across_two_expiry_waves`
  Four histories, 36 commits, 28 exact rollbacks, eight scanner rediscoveries;
  peak **222,937 CU**.
- Control: `inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_scan_recredit::v16_program_terminal_scan_rediscovers_earlier_insurance_after_later_expiry`
  Sixteen histories, 88 commits, 168 exact rollbacks, eight scanner rediscoveries;
  peak **227,593 CU**.

Both use the 400,000-CU transaction bound. Peaks include user payout cleanup and
campaign transactions, including failures; earlier funding/configuration is excluded.
No broad suite or engine proof ran. Targeted rustfmt, whitespace and protected-path
checks pass; the existing Solana future-compatibility warning remains.

Private host/SBF caches were copied from `percolator-public-gap-20260916-c91e`.
SBF was rebuilt locked/offline in this worktree with platform-tools v1.52 and
engine `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
Wrapper SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
The host test compiled in the private host target. No matcher is needed.

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/percolator-row424-multiwave-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row424-multiwave-20260917-sbf-target/deploy/percolator_prog.so
CARGO_TARGET_DIR=/dev/shm/percolator-row424-multiwave-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row424-multiwave-20260917-sbf-target/deploy -- --locked
cargo test --locked --offline --test v16_cu inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_scan_recredit::multiwave::v16_program_terminal_scan_rediscovers_remaining_insurance_across_two_expiry_waves -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_scan_recredit::v16_program_terminal_scan_rediscovers_earlier_insurance_after_later_expiry -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_070_terminal_scan_recredit.rs tests/invariants/cu/inv_070_terminal_scan_multiwave.rs
git diff --check
git diff --exit-code 5d9413fc -- . ':!tests/invariants/cu/inv_070_terminal_scan_recredit.rs' ':!tests/invariants/cu/inv_070_terminal_scan_multiwave.rs' ':!tests/invariants/README.md' ':!tests/invariants/row424_multiwave_scan_20260917.md'
```

Only these two CU files, README and this note change. INV-070 owns the repeated
scanner invalidation; stock, entitlement and progress checks are finite adjacent
evidence. No production defect or status promotion is claimed. Row 424 remains
OPEN for pending receipts/obligations during scanning, native/dual custody,
unavailable keeper rent, absent retirement authority, Recovery, maximum shapes,
more than two waves and arbitrary environmental schedules.
