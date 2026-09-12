# Shared-holder pending-loss conformance, 2026-09-12

## Scope and provenance

Base: `abfaaf4bda787a99075e693c5c6fe908d5eacd23`, the current HEAD of
`codex/astra-open-holdout-ledger-20260912` when this contribution began.
Isolated worktree: `/tmp/percolator-pending-loss-durability-20260912`.
Local branch: `codex/pending-loss-durability-20260912`.

One test is mounted as
`inv_039_pending_loss_obligation_durability::shared_holder::v16_program_shared_holder_pending_domains_survive_partial_detach_and_debtor_close_orders`
in `tests/v16_cu.rs`, through the existing INV-039 owner. Its implementation is
[`cu/inv_039_pending_loss_shared_holder.rs`](cu/inv_039_pending_loss_shared_holder.rs).
Construction reuses only the base's public `AttributionWorld` account setup and
instruction helpers. No open PR branch, diff or test content informed the test.
Production code, dependencies, support fixtures and invariant verdicts are unchanged.

## Public history and independent oracle

Five owners deposit `[200000, 180000, 300000, 250000, 777]` SPL atoms through
System, SPL, ATA and wrapper instructions. Owner 0 opens one and two integral
lots on assets 1 and 2 against owners 1 and 3. Authenticated price movements of
7 and 13,999 produce independently computed debts of 7 and 27,998 atoms. Owners
2 and 4 remain economically independent bystanders. Mirrored short/long worlds
use the same input-derived debts; live opening OI balances on each asset.

Holder refresh, asset shutdown and owner-signed recovery forfeit leave **two
zero-basis, nonzero-weight pending legs in the same portfolio**. Both debtors'
original Accounts remain untouched during holder preparation. Sixteen histories
cross side orientation, first debtor, first-debtor settlement before/after
resolution, and partial holder detach before/after first-debtor deletion.
Resolution preserves the assets and every non-market Account exactly.

After the five-slot permissionless-close delay, public continuation removes one
canonical holder leg at a time. The intermediate state still has one pending
domain and the full 28,005-atom claim. Even after its first debtor has paid and
its portfolio has been deleted, the shared holder cannot receive early value
while the second debtor remains unbooked. Detaching both legs does not pay or
forgive either original debt. The reference model checks owner capital, PnL,
receipt remainder and prior SPL payouts against original deposits and input debt
after each economic continuation. It separately predicts basis, loss weight,
stored counts and pending counts for each asset/side, and reconciles capital,
positive PnL, materialized accounts, custody and mint supply.

Resolution placement changes the payment class, not the entitlement:

| First debtor settlement | Source-realized claim | Junior receipt face | Holder SPL endpoint |
| --- | ---: | ---: | ---: |
| Both debtors after resolution | 28,005 | 0; no snapshot | 228,005 |
| 7-atom debtor before resolution | 27,998 | 7 | 228,005 |
| 27,998-atom debtor before resolution | 7 | 27,998 | 228,005 |

The model predicts junior face from the public early-settlement input. Any
present receipt must have that exact face; final exact-receipt and unreceipted
ledger terms are checked. Fully paid junior claims accept repeated top-up calls
without mutation. The fully source-realized case has no snapshot or receipt;
a top-up request therefore rejects with `EngineLockActive`, preserving every
Account except the exact independent payer transaction fee.

Five rejected transactions per history cover live holder deletion, holder
detach followed by premature claim, debtor payout/deletion followed by premature
claim, claim before remaining detach, and a detached holder waiting for the
second debtor. Eight receipt-free histories add a terminal claim rejection:
**88 exact rollbacks** in total. Typed errors and failing instruction indices
prove the intended suffix was reached. Snapshots include every transaction
account, all economic fixture Accounts and exact payer fees. Successful debtor
deletion is separately checked for unchanged asset/source-credit/payout ledgers and
the exact portfolio-rent credit to the market.

Both debtors then settle. At most four further holder closes complete source
realization and payout, each with observable progress. Keeper-only economic
continuations are separate from owner-signed mechanical deletion. Both deletion
orders reach the same owner SPL vector:
`[228005, 179993, 300000, 222002, 777]`.
All 80 portfolios delete, all 16 empty vaults close, and all 16 markets become
rent-exact closed tombstones through public `CloseSlab`. Its destination ATA is
created publicly; the admin receives rent only and zero quote tokens.

## Net-new boundary

| Baseline control | Distinct contribution |
| --- | --- |
| `terminal_fees` | One holder carries two pending domains; no maintenance-fee dimension. |
| `resolved_histories` | Partial canonical detach is observable within one holder, with its first debtor deleted while another obligation remains. Resolution placement also changes the receipt/source partition of that same holder's claim. |
| `receipt_spend_replay` | No external spending or receipt replenishment. Pending debt precedes any snapshot or receipt. |
| `receipt_partition_confluence` | The unsettled account's two obligations and debtor deletion drive progress; the matrix is not a partition of already eligible receipt calls. |
| Terminal receipt/destination tests | Ordinary pre-existing owner ATAs; no custody repair, expiry, destination authority or rent-recovery permutation. |

## Invariant mapping and limits

| Invariant | Bounded evidence and limitation |
| --- | --- |
| INV-024 | Each owner retains its exact input-derived entitlement across source realization and junior payment. No fees, reserves or external transfers. |
| INV-037 | The deployed disjoint residual equation is checked and every close ledger remains empty. Nonzero pending weights never count as residual payment. **No nonzero bankruptcy partition coverage.** |
| INV-039 | Two pending weights survive resolution and staged rejection; partial detach and debtor deletion preserve the shared owner's entire unpaid claim. |
| INV-041 | All sixteen schedules yield identical per-owner endpoints despite different payment classes. Not arbitrary caller order or allocation fairness. |
| INV-048 | Bilateral opening OI and later input-predicted basis, OI, weights and counts remain distinct; a pending leg carries no OI. |
| INV-066 | No early snapshot; exact early-debt receipt face and bound removal after debt booking. Only one eventual junior claimant; no competing haircut allocation. |
| INV-067 | Full public payouts, receipt retries or explicit no-snapshot rejection, and mechanical/market retirement. No missing-claim forgiveness. |
| INV-073 | A bounded keeper path disposes of funded accounts after authenticated delay. Owner signatures remain necessary for mechanical deletion; admin signs slab retirement. |
| INV-076 | Staged settlement/deletion rollback preserves basis, weights and value; solvent finalization composes with pending detach. **No adverse drift, bankruptcy or nonzero residual barrier.** |
| INV-081 | Selected accounting and locality predicates across the complete public route, not the full global invariant suite. |
| INV-086 | A finite input-derived economic/obligation model is checked against SBF results. No general differential generator or shrinking claim. |

Additional INV-080 evidence covers exact transaction rollback. Arbitrary amounts,
fractional rounding, funding, fees, insolvency, B/ADL, asset reuse, claimant
competition and alternate quote rails remain outside the test. Existing
invariant refutations and holdout labels are not promoted or closed.

## Validation

SBF rebuilt locally, locked/offline, with default features and platform-tools
v1.52. Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
SBF SHA-256: `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.

During test development, two oracle assumptions were corrected before the final
validation: a fully source-realized payout has no snapshot for a successful
top-up, and a mixed payout's receipt contains only the early-settled debt, not
the full two-domain claim. Exact entitlement and rollback assertions were
retained. These were test-precondition/payment-classification errors, not an
observed lost-value or terminal-liveness failure.

Final exact selector: **1/1 passed**, sixteen worlds, 88 exact rollbacks, 80
portfolio deletions and sixteen slab retirements. Final-run peak rejected bundle:
**229,452 CU** under the 500,000 bundle limit; single continuations meet 300,000.
Nearby controls: **7/7 passed**. Invariant charter/index: **1/1 passed**.
`cargo fmt --all -- --check`, `git diff --check` and the staged diff check passed.
The existing unused-support warnings in the index target and Solana 1.18 future
compatibility warning remain. No production invariant failure was observed.

Commands, from the isolated worktree (the environment was supplied with `env`
on each build/test invocation):

```sh
export CARGO_TARGET_DIR=/dev/shm/pending-loss-durability-20260912-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export TMPDIR=/dev/shm CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
sha256sum "$PERCOLATOR_FUZZ_SBF"
cargo test --locked --offline --test v16_cu inv_039_pending_loss_obligation_durability::shared_holder::v16_program_shared_holder_pending_domains_survive_partial_detach_and_debtor_close_orders -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_039_pending_loss_obligation_durability::terminal_fees::v16_program_pending_cohort_terminal_fees_stop_at_resolution_and_reach_insurance_exit \
  inv_039_pending_loss_obligation_durability::resolved_histories::v16_program_resolved_debtor_deletion_preserves_unsettled_cohort_attribution \
  inv_039_pending_loss_obligation_durability::resolved_histories::pending_destination_recovery::v16_program_pending_cohort_repair_is_atomic_and_keeper_settlement_preserves_attribution \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_spend_replay::v16_program_spent_payouts_do_not_replenish_receipts_across_order_and_atomic_retry \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_partition_confluence::v16_program_rejected_receipt_partition_suffixes_preserve_terminal_entitlements \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_terminal_disposition::v16_program_receipt_terminal_suffix_partitions_rounding_burn_surplus_and_rent \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_source_realization::v16_program_retained_receipts_preserve_identity_across_fresh_realization_or_expiry
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
git diff --cached --check
```

No push or remote write was performed.
