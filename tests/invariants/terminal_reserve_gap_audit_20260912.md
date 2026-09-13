# Terminal reserve destination coverage, 2026-09-12

Base: `adf21c4793b7c4202db8e9433c383bebb982d254`, the local
`origin/codex/astra-open-holdout-ledger-20260912` ref. Worktree:
`/tmp/percolator-astra-terminal-gap-20260912-unique`; branch:
`codex/astra-terminal-insurance-gap-20260912-unique`. The original checkout was
left untouched. No fetch, open PR diffs, copied PR tests, or push was used.

## Overlap and relation

`rg` surveyed terminal quote, receipt, pending-loss, cursor, reserve-authority and
destination owners, then paired `repair|recreat|idempotent|missing` with
`backing|provider|insurance|reserve` across the CU/stateful files. Source review
covered the three reserve withdrawal handlers, ledger initialization, and both
portfolio and slab closure. Closest current coverage:

| Existing coverage | Distinction |
| --- | --- |
| `terminal_receipt_gap_audit_20260912.md` | Repairs user receipt/pending-cohort custody; explicitly leaves reserve payouts outside its unsigned evidence. |
| `terminal_liveness_gap_audit_20260912.md` | Repairs shared user principal destinations; no reserve fee or beneficiary claims. |
| `inv_024_terminal_earnings_succession.rs` | Real earned fees and signed succession with intact custody. Its public setup is extracted unchanged, retaining the original pre-settlement mint oracle; portfolio rent attribution checks are added. |
| `inv_024_terminal_insurance_lifecycle.rs` and INV-073 absent-reserve siblings | Beneficiary succession, exhausted insurance or expiry/recredit; no keeper reconstruction followed by a reserve signature boundary. |

One new selector is mounted in the narrow INV-024 child
`cu/inv_024_terminal_reserve_destination_recovery.rs`. At the exact user exit
boundary (resolution slot 2 plus delay 5), portfolios have paid and dematerialized.
The input-derived remaining claims are 100,000 backing principal, 875 utilization
fees, and 31 insurance atoms. The provider, live insurance operator and terminal
beneficiary are distinct; the beneficiary also holds the market authority.

Public SPL closure removes the provider and beneficiary ATAs. Each nonzero reserve
route is tried after keeper-funded `CreateIdempotent` with only the keeper signing:
all three reject `ExpectedSigner`. Three further bundles first reconstruct custody,
pay a different signed reserve claim, and initialize its ledger where applicable;
they then reconstruct the other destination and reject its unsigned withdrawal.
The exact error index and completed wrapper/ATA prefix counts prevent an earlier
rejection from satisfying the test. Every compiled/tracked Account, account
presence and rent rolls back; only the actual signature fees remain charged.

Two fresh worlds finish with principal-first or earnings-first payouts, insurance
between them. The duplicated initial rejection matrix is omitted from the second
world. Both yield user SPL balances 56,627/1,995,000, provider 100,875, beneficiary
31, and live operator zero. Every prefix checks fixed supply, custody, independent
reserve amounts, stock/reservation censuses, ledger identity/counters, exact SPL
Account images, and framed wallets. Reconstructing the shared provider ATA for its
second payout charges no rent. Post-hoc ledgers observe earnings/insurance; backing
principal uses the existing route without an optional deposit ledger.

Portfolio-close lamports enter the market slab; original ATA rent goes to its
owner, reconstruction rent is paid by the keeper, and final slab/vault rent goes
to the market authority minus the exact retained tombstone rent. Ledger rent stays
in its accounts. Successful close leaves all owner/provider/beneficiary SPL value
unchanged. All transitions use real wrapper/System/SPL/ATA instructions. Account
copies are read-only assertion frames, never restored or installed as state.

## Validation

Locked/offline default-feature SBF rebuilt from this worktree with platform-tools
v1.52, using a private copy of the base audit's build cache. Engine pin:
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`. Wrapper SHA-256:
`5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
New selector: **2 worlds, 6 exact rollbacks, 6 signed payouts, 4 committed ATA
reconstructions, 2 slab closes**; observed peak **275,583 CU**, limit **400,000 CU**.
The new selector plus four adjacent controls pass (**5/5**); the invariant
charter/index passes (**1/1**), as do `cargo fmt --all -- --check` and
`git diff --check`. Existing unused-support and Solana future-compatibility
warnings remain. Production sources, dependencies and holdout ledgers are unchanged.
Development corrected test assumptions about closed-account presence, post-hoc
principal ledgers and portfolio rent routing; no production behavior was changed.

Focused commands (no unfiltered suite):

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-terminal-reserve-gap-20260912-unique-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export TMPDIR=/dev/shm CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_reserve_destination_recovery::v16_program_terminal_reserve_destination_repair_preserves_signer_gates_and_value \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::v16_program_terminal_earned_fee_succession_preserves_paid_prefix_and_insurance
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_024_attributed_quote_value_conservation::terminal_insurance_lifecycle::v16_program_terminal_insurance_lifecycle_preserves_fee_and_paid_prefix_attribution \
  inv_073_no_permanent_user_lock::absent_insurer_spent_retirement::v16_program_absent_reserve_roles_preserve_recredited_insurance_after_backing_expiry \
  inv_082_state_indexed_liveness_theorem::terminal_destination_recovery::v16_program_missing_destination_has_keeper_only_terminal_recovery
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
```

## Remaining gaps

All five holdouts remain OPEN. For #433 this establishes the current signature
barrier and exact signed recovery, not completion without reserve authorities.
Beneficiary/marketauth separation, partial reserve payments, multiple domains and
lost reserve keys remain outside this slice. #418 native insurance retirement and
Token-2022 behavior, #417 retained receipts with late expiry, #419 pending losses
with fees/funding, and #424 invalidation behind a scanned prefix are unchanged.
No historical vulnerable pin, invariant verdict or holdout ledger was changed.
