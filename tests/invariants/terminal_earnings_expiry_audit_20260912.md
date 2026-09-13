# Terminal Earned Fees Across Backing Expiry

Base: `8fb23b4d194b0953c4a01a95c4b14c431e787978`, the locally available
`origin/codex/astra-open-holdout-ledger-20260912` tip. Isolated worktree:
`/dev/shm/percolator-terminal-reserve-attribution-20260912-b7d3`; branch:
`codex/terminal-reserve-attribution-20260912-b7d3`. Only local base files and the
locally cached pinned dependency source were inspected. No fetch, remote PR or
issue diff inspection, push, or protected-checkout edits were used.

## Executable Increment

One selector is mounted under the existing public earned-fee fixture:

```text
inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_earnings_expiry::v16_program_terminal_expiry_preserves_earned_fees_and_bounded_signed_disposal
```

The new file is `cu/inv_024_terminal_earnings_expiry.rs`. It reuses normal
System/SPL/ATA/wrapper construction, real fee-generating trades and completed
user payouts from `terminal_earnings_world`. The destination-recovery transaction
oracle is exposed only to its parent module for reuse. No production sources,
dependency pins, invariant verdicts or reopening classifications change.

Four independent worlds cross delivery at expiry slot 100 or slot 101 with
provider-earnings-first or insurance-first payout. Inputs independently fix
100,000 backing atoms, 875 earned utilization-fee atoms, 31 insurance atoms and
user payouts of 56,627 and 1,995,000. Provider, live insurance operator and terminal
beneficiary are distinct; the beneficiary also holds market authority. The fee
payer is separate. Mint authority is revoked at supply 2,152,533.

After users are paid and their portfolios deleted, the provider withdraws only
101 principal atoms. CloseSlab rejects at slot 99. At maturity, one admin-signed
call normalizes the remaining 99,899 principal atoms without the provider's
signature or any token movement. Earned fees remain booked to the provider;
neither the beneficiary/admin nor the live operator can receive them as insurance.
The trade history's 5,000-atom provider receivable and spent-backing counters are
checked against the input profit, including after expiry.

Each world checks ten exact rejections:

- Premature slab close at slot 99.
- Expiry plus insurance payout followed by unsigned provider earnings.
- Expiry plus provider earnings and lazy-ledger initialization followed by close
  while insurance remains owed.
- Provider earnings and lazy-ledger initialization followed by unsigned insurance.
- Unsigned earnings, admin-signed earnings, operator-signed insurance, insurance
  overdraw into residual, expired principal withdrawal, and close with unpaid fees.

The shared oracle checks the precise error and instruction index, completed wrapper
prefixes, every tracked and compiled Account, and the exact transaction signature
fee. Account presence, rent, SPL metadata and ledger bytes roll back together.
Positive steps frame other roles and compare full token Accounts, fixed supply,
custody, reserves, source counters, roles, epochs and market shape. In-memory
Account copies are assertion frames only and are never installed as state.

Both payout orders require the respective reserve signatures and reach the same
endpoint: provider 976, insurance beneficiary 31, live operator zero, and unchanged
user payouts. Final admin-signed close burns exactly 99,899 atoms, preserves all
holder Accounts and the paid provider ledger, closes the vault and refunds exact
market/vault rent above the tombstone minimum. There are exactly two committed
slab calls per world, one normalization and one final disposal. Provider signatures
are absent from both slab calls; permanent key loss is not a completion claim.

## Overlap And Limits

| Inspected base history | Distinct relation here |
| --- | --- |
| Terminal earned-fee succession and reserve destination repair | Both finish while backing is fresh; this test expires partially paid principal while earned fees remain. No succession or custody reconstruction is added. |
| INV-073 provider earnings and lazy-ledger final close | Withdraws all principal before close; this test preserves earnings through expiry and subsequent burning of the remaining principal. |
| Absent-provider staggered expiry and separate-beneficiary mixed maturity | Explicitly use zero earned fees; this test has 875 independently computed earned atoms and a late fee-signature boundary. |
| Funded-insurer stale-resolution, reassigned custody, shutdown operator departure and terminal role handoff | Existing coverage retained. The new history adds no role transfer, shutdown or destination-authority mutation. |

Rows 410/420/421/433 gain sampled attribution and signer-boundary evidence, not
closure. Rows 416/429 were surveyed; this selector adds no funded management or
shutdown product. All six remain OPEN. User progress is inherited from the public
fixture, with available owners for portfolio deletion; the increment measures
the subsequent terminal reserve history. Insurance beneficiary/market-authority
separation, absent keys forever, insurance recredit, multiple assets, retained
signatures, shutdown fallback, alternate quote rails and general histories remain
outside this increment.

## Validation

The default-feature wrapper SBF was rebuilt locked/offline in the isolated
worktree using platform-tools v1.52 and a private copy of the local dependency
cache. Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`. Wrapper SHA-256:
`5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.

The new exact selector and five exact nearby controls pass (6/6). The increment
covers four worlds, 40 exact rollbacks and eight committed slab calls, with peak
measured cost 274,885 CU below the shared 400,000-CU limit. All four invariant
index/status/root checks pass (4/4), as do repository-wide formatting and diff
whitespace checks. Production sources, dependency pins, the CU root and both
status TSVs match the base. Existing support dead-code warnings and the Solana
1.18 client future-compatibility notice remain.

Development corrected use of the account-view API and an initially incorrect
zero-spent-backing assertion; the expected 5,000 atoms now derive from the trade
profit. Neither correction required a production change.

Commands run from this worktree with its private `target` directory:

```sh
export CARGO_TARGET_DIR="$PWD/target" PERCOLATOR_FUZZ_SBF="$PWD/target/deploy/percolator_prog.so"
export TMPDIR="$PWD/target" CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_earnings_expiry::v16_program_terminal_expiry_preserves_earned_fees_and_bounded_signed_disposal -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::v16_program_terminal_earned_fee_succession_preserves_paid_prefix_and_insurance \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_reserve_destination_recovery::v16_program_terminal_reserve_destination_repair_preserves_signer_gates_and_value \
  inv_073_no_permanent_user_lock::v16_program_terminal_provider_earnings_and_lazy_ledger_reach_exact_slab_close \
  inv_073_no_permanent_user_lock::absent_provider_expiry_retirement::v16_program_absent_provider_staggered_expiry_reaches_funded_terminal_retirement \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::mixed_maturity::v16_program_terminal_expiry_preserves_separate_reserve_beneficiaries
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_program_invariant_harnesses_are_test_free_roots
cargo fmt --all -- --check
git diff --check
```
