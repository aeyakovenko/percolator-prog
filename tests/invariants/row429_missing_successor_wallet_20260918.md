# Row 429: missing fee-successor wallet, 2026-09-18

Base: freshly fetched `origin/main`, `9c8717a27e1a9e4811d6d3b8b5848b88889f6eed`.
Worktree: `/dev/shm/astra-ultra-row429-coverage-20260918`.
Branch: `astra-ultra/row429-coverage-20260918`; local commit only, no push.

One new exact selector in the existing
[beneficiary-handoff file](cu/inv_024_recreated_beneficiary_handoff.rs) adds
missing System wallets after funded backing-fee succession and paid prefixes.
It reuses the public earnings fixture, payout/handoff builders, stock checks and
complete-Account transaction frames without changing those helpers.

## History and assertions

The fixture earns 875 fee atoms, resolves, pays both users and deletes their
portfolios. The incumbent receives 100,000 principal and 17 fee atoms, consents
to transfer the still-funded backing role, and the successor receives 19 fees
through its own ledger. Both holders publicly transfer their entire SOL balance
to the independent keeper. Their wallets are absent or empty zero-lamport System
Accounts, and both keypairs are dropped before any continuation.

A keeper-only transaction pays five fees to the successor, then rejects a
one-atom fee request using the former holder's ledger with `Unauthorized` at
instruction index 3. The successful wrapper prefix is checked; all compiled and
tracked Accounts, including the paid token prefix and both ledgers, roll back
exactly apart from the keeper's signature fee. The identical five-atom instruction
then commits. A final unsigned bundle pays the remaining 834 fees and 31 insurance
atoms. Six committed reserve payouts leave zero booked and raw vault value.

The incumbent keeps exactly 100,017 tokens and its unchanged 17-fee ledger;
the successor receives 858 fees in total, with zero newly accrued earnings
reported by succession. Insurance receives its separate 31 atoms. Complete SPL
Account images, unchanged mint and wallet Accounts, decoded successor ledger,
control sequences, role profile, stock census and reservation census are checked
through the absent-wallet suffix. The keeper's SOL receipts do not revive either
wallet or change token entitlement.

## Non-duplication and limits

- The [Rows 410/429 regression-health note](row410429_regression_health_20260917.md)
  explicitly keeps beneficiary wallets present during spent-payout custody
  recreation and merge/return. This new history removes both fee-holder wallets
  after each has a paid ledger history.
- The nearest [earned-fee succession control](cu/inv_024_terminal_earnings_succession.rs)
  retains both holders' wallets and signs successor payouts. The new continuation
  needs neither keypair nor a funded System Account at either beneficiary key.
- The [generated wallet-absence matrix](cu/inv_073_generated_reserve_wallets.rs)
  changes the insurance beneficiary during setup, but keeps the backing
  beneficiary fixed. The [provider keeper/ledger handoff](cu/inv_073_provider_keeper_ledger_handoff.rs)
  also keeps the backing beneficiary fixed while changing the keeper.

This is bounded Row 429 paid-history evidence. It adds no Row 410 shutdown
submitter or reserve-role attribution claim. Row 429 retains its existing COVERED
entry; invariant and TSV statuses are unchanged. ATA custody stays present, and
the final empty slab is not retired. Live shutdown, expiry/recredit, receipt
competition, other quote rails and arbitrary reserve histories are outside this
increment. No implementation violation was observed.

## Validation

Only the two exact selectors below ran; both passed on their first run, each
with 1 passed, 0 failed and 1,488 filtered. New/control runtime: 0.56s / 0.55s.
Measured CU peaks: **450,936 / 423,123**. The new test measures the handoff,
wallet drains, successful payments and rejected bundle, excluding fixture setup;
it asserts a 600,000-CU peak and the reused helpers check 1,232-byte packets.
The existing `solana-client v1.18.26` future-compatibility warning remains.

Private host/SBF caches were copied without hard links and the default-feature
wrapper was rebuilt locked/offline with platform-tools v1.52. No matcher is needed
by this bilateral fixture. Wrapper SHA-256:
`a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.

Exact build/test commands, with the environment used for execution:

```bash
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row429-coverage-20260918-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-ultra-row429-coverage-20260918-sbf-target/deploy/percolator_prog.so
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 TMPDIR=/dev/shm
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
CARGO_TARGET_DIR=/dev/shm/astra-ultra-row429-coverage-20260918-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/astra-ultra-row429-coverage-20260918-sbf-target/deploy -- --locked
cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::recreated_beneficiary_handoff::v16_program_missing_fee_successor_wallet_preserves_paid_history_and_unsigned_tail -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::v16_program_terminal_earned_fee_succession_preserves_paid_prefix_and_insurance -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_024_recreated_beneficiary_handoff.rs
git diff --check
git diff --cached --check
git show --check HEAD
```

Targeted formatting, diff/commit whitespace checks and the tests/docs-only scope
check pass. Production, Cargo files, fixtures, TSVs and support helpers are unchanged.
