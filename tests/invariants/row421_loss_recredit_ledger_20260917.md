# Row 421: funded insurance ledger across loss and recredit

Base: fetched `origin/main`, `15620f87ef4dfa3564d90671705c6927f7343d14`.
Worktree: `/dev/shm/row421-expiry-insurance-20260917`.
Branch: `codex/row421-expiry-insurance-20260917`. Primary invariant: INV-073.

## Changed paths

- `cu/inv_073_insurance_loss_recredit_ledger.rs`: one new public LiteSVM history.
- `cu/inv_073_no_permanent_user_lock.rs`: child module registration only.
- `README.md` and this note: evidence and limits; no status promotion.

## Duplicate analysis

Inspected the existing row421 notes and owning tests before editing:

| Existing evidence | Distinct assertion here |
| --- | --- |
| Closed native beneficiary identity and native ledger/redemption histories | Those retain unspent insurance; this ledger records actual loss and recovery. No custody repair or redemption is added. |
| Absent-insurer expiry, missing-wallet recredit, recredited quote rails and native recredit custody | Those lack a funded insurance ledger spanning bankruptcy, an observed loss and subsequent unsigned recovered payouts. |
| Terminal progress product and generated terminal actionability | Those already cover payment/expiry schedules, partial or repeated recredit and reserve absence. Their insurance history does not retain this funded deposit/loss/profit ledger. |
| Depleted reserve beneficiary succession (INV-024) | Its ledgers start after the loss; roles participate in succession. This fixed beneficiary's ledger starts at funding and both insurance keys are dropped before trading. |
| Reserve destination repair and generated expiring roles | Unspent insurance or telemetry changes from peer payouts, rather than a realized insurance loss followed by recovery in the same funded ledger. |

The senior-liability, portfolio-deletion, expiry and administrative-signature
checks are composition controls, not standalone novelty claims.

## Public history

System/ATA/SPL/wrapper instructions create all economic state. Public funding
deposits 117 insurance atoms with the optional ledger and 307 backing atoms.
Trades and authenticated marks create a 100-atom bankrupt deficit. Neither
insurance role signs after funding. Permissionless resolution and user exits pay
`[1200, 0, 137]`; owner-signed deletion removes the three empty portfolios.

| Committed checkpoint | Withdrawn | Principal | Observed stock | Loss | Profit |
| --- | ---: | ---: | ---: | ---: | ---: |
| Funded ledger | 0 | 117 | 117 | 0 | 0 |
| Unsigned 17-atom payment before expiry | 17 | 100 | 0 | 100 | 0 |
| Expiry normalization at slot 44 | 17 | 100 | 0 | 100 | 0 |
| Unsigned recredit and 41-atom payment | 58 | 59 | 59 | 100 | 100 |
| Unsigned 59-atom tail | 117 | 0 | 0 | 100 | 100 |

Deposited value stays 117 throughout. Five exact Account rollbacks cover payout
with active senior capital, payout before empty-portfolio deletion, payout before
expiry, successful recredit/payment followed by unsigned CloseSlab, and overclaim
after full payment. The rejected bundle must log its successful wrapper/SPL prefix;
only signature fees survive. Its identical payment then commits with only the
keeper signature. Insurance budgets/spend, custody/supply, source receivables,
ledger identity/metadata, role profiles, debit epochs, stock/encumbrance censuses
and market shape are checked. Final signed closure burns the 207-atom residue,
refunds exact rent and frames the paid custody and ledger. No program-owned bytes
are injected; mutable views are only used to validate local Account copies.

## Exact validation

All three selectors PASS on merged `main` with rebuilt default-feature SBF (3/3, 4.30s).

| Exact selector | Peak CU |
| --- | ---: |
| `inv_073_no_permanent_user_lock::insurance_loss_recredit_ledger::v16_program_unsigned_insurance_ledger_preserves_loss_and_recredit_after_cleanup` | 215,443 |
| `inv_073_no_permanent_user_lock::absent_insurer_spent_retirement::v16_program_absent_reserve_roles_preserve_recredited_insurance_after_backing_expiry` | 228,958 |
| `inv_073_no_permanent_user_lock::native_insurance_ledger_progress::v16_program_unsigned_native_insurance_ledger_excludes_donations_through_close` | 37,577 |

New-history peaks (user progress / rejection / insurance payment / cleanup):
**215,443 / 129,116 / 127,004 / 127,067 CU**, below 400,000. Funding/trade/mark setup
is excluded. The reused transaction verifier enforces the 1,232-byte packet bound.

Private host/SBF caches were copied, not linked, from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target`.
Reproduction from this worktree:

```bash
export CARGO_TARGET_DIR=/dev/shm/row421-expiry-insurance-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/row421-expiry-insurance-20260917-sbf-target/deploy/percolator_prog.so
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
CARGO_TARGET_DIR=/dev/shm/row421-expiry-insurance-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/row421-expiry-insurance-20260917-sbf-target/deploy -- --locked
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 inv_073_no_permanent_user_lock::insurance_loss_recredit_ledger::v16_program_unsigned_insurance_ledger_preserves_loss_and_recredit_after_cleanup inv_073_no_permanent_user_lock::absent_insurer_spent_retirement::v16_program_absent_reserve_roles_preserve_recredited_insurance_after_backing_expiry inv_073_no_permanent_user_lock::native_insurance_ledger_progress::v16_program_unsigned_native_insurance_ledger_excludes_donations_through_close
```

SBF SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
Targeted rustfmt, whitespace and exact changed-path checks pass. Production,
Cargo inputs, fixtures, TSV ledgers and unrelated invariant files are unchanged.

## Remaining gaps

Row 421 remains OPEN. This is one fixed classic-SPL asset, one full recredit and
one fixed beneficiary, with available custody and keeper funding. It adds no
arbitrary-history, partial/repeated-loss recovery, native/dual-rail, authority
succession, absent-wallet, maximum-shape or ledger-disposal theorem. Active
liabilities block insurance payment; user/receipt permutation coverage is not
new. Owners sign deletion, and the administrator signs expiry normalization and
retirement. Neither administrative operation is claimed permissionless.
