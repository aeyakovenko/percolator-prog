# Lane 2: terminal progress and payout conformance

## Scope and provenance

- Base: `d809e9a5`, as requested; engine pin `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Worktree: `/home/anatoly/percolator-lane2-terminal-20260915`.
- Branch: `codex/lane2-terminal-conformance-20260915`.
- The user's `/home/anatoly/percolator-prog` checkout was read only. Its existing
  conflicted branch was not used as the implementation base.
- Read `scripts/loop.md`, the invariant README, open findings and invariant status
  at the requested base before development. No holdout branches, patches or tests
  were inspected or copied. The benchmark labels remain unchanged; this report
  claims additional bounded conformance, not new independent discovery.
- Production source, dependency manifest and lockfile are unchanged. No production
  defect was found in the exercised paths, so no implementation fix or defect
  red/green claim is made.
- Built the default-feature wrapper SBF from this worktree with platform-tools
  v1.52. SHA-256:
  `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.

## Changes

| File | Change |
| --- | --- |
| [cu/inv_067_terminal_claim_late_expiry.rs](cu/inv_067_terminal_claim_late_expiry.rs) | Parameterizes the existing staggered public fixture's two unbacked claimant trade sizes while preserving its total face, deposits and source stocks. Existing callers keep the 14/26-lot split. |
| [cu/inv_067_receipt_overdue_history.rs](cu/inv_067_receipt_overdue_history.rs) | Extends the existing generator with claimant sizes and shared custody, a rank, exact per-portfolio accounting and final portfolio/slab cleanup. |
| [cu/inv_073_recovery_reserve_cleanup.rs](cu/inv_073_recovery_reserve_cleanup.rs) | Adds stale provider-request rollback and successful retry across terminal insurance epoch consumption to the existing two Recovery worlds. |
| [cu/inv_072_order_robust_crankability.rs](cu/inv_072_order_robust_crankability.rs) | Corrects a stale source-gate helper name and checks complete health-observation coverage before Live selection. |
| [README.md](README.md) | Adds a scoped evidence summary without changing invariant dispositions or benchmark labels. |
| This report | Records commands, results, row verdicts and limits. |

No duplicate selectors or independent fixture copies were added.

The initial composition run exposed an inherited INV-072 assertion naming
`reject_missing_pending_liquidation_observations_view`, which no longer exists.
Production and this source gate were unchanged from `d809e9a5` at that failure.
The corrected gate requires the current
`reject_incomplete_account_health_observations_view` call, its stale/liquidatable
condition, and parser-before-coverage-before-Live-selection ordering. The existing
Recovery/expired-close and Resolved bypass requirements remain. This is a
red/green repair of test/source drift, not a production vulnerability fix.

## Evidence

### Generated receipts

The existing selector
`inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_overdue_history::v16_program_generated_overdue_source_histories_preserve_receipt_identity_and_attribution`
now exercises nine deterministic histories and 24 seeded generated histories,
each as split instructions and grouped transactions: **66 public worlds**.
Generated inputs include 1..39 first-claimant lots, separate/shared ownership,
landing slots 15..63, claimant cadence, transaction group widths and aborted groups.
Deterministic cases retain the original three histories and add 1/39, 20/20 and
39/1-lot splits with both ownership shapes. The equal-face shared-owner case
distinguishes the sum of two receipt floors from rounding their combined face.

All collateral, portfolios, trades and backing are created through public
System/SPL/wrapper instructions. The oracle derives each face from trade lots and
the 50-atom price change. After both source expiries, claimant `i` receives
`1000 + floor(face_i * 851 / 3000)` atoms. The provider retains its one token that
was never deposited. Receipt identity, prior bound, paid amount, source stock and
denominator remain separately checked when two portfolios share one token account.

The progress rank is lexicographic:
`(remaining source steps, unpaid claimant atoms, present receipt count)`.
Each needed modeled action lowers the rank, and public state must match that
model after every transaction; the paired split execution also checks each
individual action. Already-current retries are explicitly allowed to be inert.
There are four source steps and at most forty scheduled claim calls, followed by
six cleanup claims and one replay transaction. Each generated group aborts at
most once before its successful retry: at most 95 economic transactions in this
post-snapshot phase. This bound excludes the shared public setup.

Observed result: **1,461 committed economic transactions, 181 exact rollbacks
(76 paying, 37 releasing source stock), 396 portfolio deletions, 66 slab calls**.
Every world reaches zero economic rank and a market tombstone. SPL mint supply
falls by exactly the generated rounding residue; the vault closes; complete
recipient frames and the designated rent refund remain exact. Every world used
one slab call, against an explicit eight-call bound. Peak measured CU:
**538,178**, below the existing 900,000 campaign ceiling.

### Recovery reserves

The existing selector
`inv_073_no_permanent_user_lock::v16_program_recovery_reserve_repair_crosses_last_portfolio_cleanup_without_beneficiary_signatures`
still creates funded Recovery positions and earned fees, force-closes, resolves,
pays users, and crosses the last-materialized-portfolio gate. Reserve destinations
are absent and reserve keys are dropped before terminal progress.

The added sequence starts after one provider stock class has already been paid.
A keeper creates insurance custody and pays insurance, then a retained request
for the other provider class fails with `EngineStale` at instruction index 4.
Full account rollback restores custody absence, insurance stock, the authority
epoch, ledger state and the already-committed provider prefix. The keeper then
commits insurance payment; the same retained request rejects at index 2; a fresh
unsigned provider request pays the remainder. Both principal-first and
earnings-first orders are exercised. Exact reserve amounts are 100,000 principal,
875 earnings and 31 insurance atoms. Each accepted payment reduces unpaid stock
by its exact amount; only insurance consumes an authority epoch.

Observed result: **2 worlds, 12 exact rollbacks**, all economic payments and slab
retirement completed. Peak CU: **481,785**, below the new 800,000 witness ceiling.
The shared transaction helper checks required signatures, packet size, completed
prefix instructions and complete account images, including fee-payer rent/fees.

## Row verdicts

| Row(s) | Verdict for this increment |
| --- | --- |
| 417 | Additional bounded coverage: generated claimant faces and shared destinations compose with two overdue sources, retained requests, exact rollback and final residue disposition. Remains OPEN / `missing`; this does not establish the full withheld finding or arbitrary-history entitlement. |
| 420 | Existing `independent-discovery` evidence strengthened: absent-provider principal and earnings complete despite an intervening insurance debit making a retained request stale. Broader row remains OPEN. |
| 421 | Additional bounded coverage: unsigned insurance custody reconstruction/payment rolls back and then commits while preserving separate provider claims. Remains OPEN / `missing`; full permissionless retirement is not established. |
| 433 | Existing `independent-discovery` evidence strengthened: Recovery user exit, last-portfolio cleanup, all three reserve classes, stale cross-route retry and final close compose in both provider orders. Broader row remains OPEN. |
| 286 | Generated post-snapshot expiry/payout evidence also exercises this boundary. Remains `nonqualifying`; no exact-parent qualifying loss demonstration is asserted. |
| 204, 269, 297, 308, 372, 373, 374 | Existing expiry, maximum-shape, B-budget and retained-Recovery prerequisite controls rerun. Remain `nonqualifying`; a passing progress path or unreachable prerequisite does not establish the claimed historical impact. |
| 202, 219, 257, 287, 288, 370 | No new row-specific coverage or reclassification. Existing broad terminal/Recovery controls are not treated as discovery of these withheld cases. Remain `nonqualifying`. |
| 418, 424 | Adjacent terminal-disposition invariants benefit from the classic-token finalization checks. No new native-token or scan-invalidation benchmark claim. Existing dispositions unchanged. |

The added evidence directly concerns INV-063/064/066/067/068/069/070/071/073/077/
078/082. INV-065/072/074/075/076 have no new route witnesses in this increment;
their existing composition gates are validation controls, not new proof.

## Commands and results

Commands ran from the isolated worktree. Build outputs use an isolated 8 GiB tmpfs
because the shared filesystem had about 1.5 GiB free. Initial attempts to read the
four requested files in the user's checkout found them absent; they were read
from `d809e9a5` and then from this worktree.

```sh
git worktree add -b codex/lane2-terminal-conformance-20260915 /home/anatoly/percolator-lane2-terminal-20260915 d809e9a5
mkdir -p target
sudo -n mount -t tmpfs -o size=8G,uid=1001,gid=1001 tmpfs /home/anatoly/percolator-lane2-terminal-20260915/target
env CARGO_TARGET_DIR="$PWD/target/sbf" CARGO_BUILD_JOBS=4 cargo build-sbf --tools-version v1.52 --sbf-out-dir target/deploy -- --locked --offline
env CARGO_TARGET_DIR="$PWD/target/auth-sbf" CARGO_BUILD_JOBS=4 cargo build-sbf --tools-version v1.52 --manifest-path tests/fixtures/auth_matcher/Cargo.toml --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked --offline
sha256sum target/deploy/percolator_prog.so
```

The following exports express the equivalent per-command `env` assignments used
for the host build and tests:

```sh
export CARGO_TARGET_DIR="$PWD/target/host"
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF="$PWD/target/deploy/percolator_prog.so"
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=2 \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_overdue_history::v16_program_generated_overdue_source_histories_preserve_receipt_identity_and_attribution \
  inv_073_no_permanent_user_lock::v16_program_recovery_reserve_repair_crosses_last_portfolio_cleanup_without_beneficiary_signatures

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=2 \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_conversion_then_expiry::v16_program_committed_conversion_then_late_expiry_preserves_receipt_attribution \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_coowned_conversion::v16_program_coowned_receipts_preserve_attribution_across_conversion_and_late_expiry \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_fractional_source::v16_program_fractional_source_conversion_preserves_receipts_through_same_bucket_expiry \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_late_fee_reclassification::v16_program_late_fee_reclassification_preserves_receipt_faces_and_claimant_order \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::v16_program_receipt_payout_and_portfolio_close_retry_is_exact_once \
  inv_073_no_permanent_user_lock::v16_program_terminal_public_reserve_disposition_preserves_value_across_orders \
  inv_073_no_permanent_user_lock::v16_program_public_reserve_payments_wait_for_resolved_senior_disposition \
  inv_073_no_permanent_user_lock::v16_program_terminal_insurance_exit_does_not_require_former_beneficiary_ledger \
  inv_077_bounded_work_and_maximum_shape_compute::v16_program_direct_close_resolved_at_14_leg_28_source_shape_is_bounded

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=2 \
  inv_063_backing_expiry_normalization::v16_program_post_snapshot_expiry_rejects_stale_trade_then_owner_progresses \
  inv_063_backing_expiry_normalization::v16_program_retained_recovery_expiry_prerequisite_matrix_avoids_provider_capitalization \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::v16_program_retained_recovery_haircut_prerequisite_matrix_keeps_prior_claim_floor \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::v16_program_prior_claim_forfeit_prerequisite_matrix_preserves_withdrawable_value \
  inv_071_crank_progress::v16_program_prospective_loss_expiry_matrix_keeps_resolved_exit_live \
  inv_071_crank_progress::v16_program_prospective_source_expiry_prerequisite_matrix_keeps_exit_live \
  inv_071_crank_progress::v16_program_b_budget_lock_prerequisite_rejects_post_adl_basis_reissue \
  inv_073_no_permanent_user_lock::v16_program_fractional_social_loss_exit_matrix_preserves_funded_owner_exit \
  inv_073_no_permanent_user_lock::v16_program_expired_partial_close_matrix_resolves_and_preserves_idle_exit \
  inv_077_bounded_work_and_maximum_shape_compute::v16_program_dense_zero_delta_resolution_shape_matrix_keeps_terminal_exit_bounded \
  inv_082_state_indexed_liveness_theorem::v16_program_mixed_recovery_active_terminal_accrual_has_bounded_public_exit
```

The initial combined `v16_cu`/`v16_program_fuzz_regressions` gate command selected
the ten composition gates below and the four metadata gates. It stopped after
9/10 CU composition gates passed and the inherited INV-072 assertion failed;
the metadata executable did not run in that invocation. After the test-only
correction, the final gate commands were:

```sh
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=2 \
  inv_063_backing_expiry_normalization::v16_program_backing_expiry_consumer_composition_is_source_complete \
  inv_065_reset_recovery_and_retired_state_isolation::v16_program_lifecycle_isolation_composition_is_source_complete \
  inv_066_resolved_payout_fairness_and_order_independence::v16_program_resolved_payout_induction_composition_is_source_complete \
  inv_069_terminal_normalization_and_retirement::v16_program_terminal_blocker_census_composes_engine_retirement_before_wrapper_cleanup \
  inv_071_crank_progress::v16_program_crank_progress_and_recovery_composition_is_source_complete \
  inv_072_order_robust_crankability::v16_program_every_auto_crank_plan_and_hint_parser_stratum_has_public_evidence \
  inv_072_order_robust_crankability::v16_program_crank_hint_matrix_preserves_or_discovers_canonical_progress \
  inv_073_no_permanent_user_lock::v16_program_terminal_disposition_and_administrative_retirement_are_source_complete \
  inv_074_scope_locality::v16_program_scope_locality_composition_is_source_complete \
  inv_075_close_priority_ownership_and_episode_integrity::v16_program_exclusive_close_ownership_composition_is_source_complete \
  inv_076_close_drift_residual_durability_and_finalization_atomicity::v16_program_close_finalization_composition_is_source_complete

cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=2 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant

rustfmt --edition 2021 --config skip_children=true --check \
  tests/invariants/cu/inv_067_terminal_claim_late_expiry.rs \
  tests/invariants/cu/inv_067_receipt_overdue_history.rs \
  tests/invariants/cu/inv_072_order_robust_crankability.rs \
  tests/invariants/cu/inv_073_recovery_reserve_cleanup.rs
cargo fmt --all -- --check
git diff --check
git diff d809e9a5 -- src Cargo.toml Cargo.lock
```

| Validation | Result |
| --- | --- |
| Fresh wrapper and authenticated matcher SBF builds; host test build | Passed, offline and locked. |
| Strengthened selectors | 2/2 passed in 93.51 s. |
| Adjacent controls | 9/9 passed in 47.60 s. Includes separate/shared conversion, split-source and maintenance-fee fixtures, unsigned reserve seniority/payment, and direct 14-leg/28-source resolved close. The maximum-shape close peaked at 1,156,436 CU. |
| Prerequisite/progress controls | 11/11 passed in 169.90 s. Includes the 128/5,782-slot zero-delta resolution matrix and mixed Recovery/Active terminal accrual (197,910 CU peak). |
| Final composition checks and hint-matrix control | 11/11 passed in 15.69 s after correcting the inherited INV-072 source assertion. |
| Metadata gates | 4/4 passed in 0.01 s; open findings and authoritative invariant statuses remain unchanged. |
| Edited-file rustfmt; `git diff --check` | Passed. |
| Production/dependency diff against `d809e9a5` | Empty. |
| Repository-wide `cargo fmt --all -- --check` | Failed on six unchanged files listed below; no unrelated formatting edits applied. |

The final selection comprises **37 distinct passing checks**: 23 public-route
regressions, ten source-composition checks and four metadata checks. The initial
source-gate failure and its correction are recorded above rather than omitted.
Existing unused-support and Solana future-compatibility warnings remain.

Repository-wide formatting differences are confined to these unchanged files in
`tests/invariants/cu/`: `inv_024_terminal_reserve_destination_recovery.rs`,
`inv_070_native_residue_disposition.rs`, `inv_073_dual_quote_reserve_progress.rs`,
`inv_073_frozen_reserve_replacement.rs`, `inv_073_native_recredit_custody.rs`, and
`inv_073_terminal_public_reserves.rs`. The four edited Rust files pass the focused
rustfmt check.

Local execution logs are in `target/sbf-build.log`, `target/auth-sbf-build.log`,
`target/host-build.log`, `target/lane2-focused.log`, `target/lane2-adjacent.log`,
`target/lane2-prerequisites.log`, `target/lane2-gates.log` (initial red source
assertion), `target/lane2-gates-green.log`, `target/lane2-metadata.log`, and
`target/fmt-all.log`. These build/log files are untracked, and the target mount
is temporary; the result record in this report is retained in Git.

## Remaining gaps

This is sampled public-route evidence, not a universal liveness or entitlement
proof. The new receipt generator has six portfolios, three assets, fixed total
face/deposits, fixed source magnitudes and expiry order, fee-free no-CPI trades,
and one classic SPL rail. Its generated face split covers a finite range; it does
not vary source counts, reversed source expiry, funding, ADL, insurance recredit,
alternate quote rails, arbitrary claim populations or maximum shapes in the same
history. The separate maximum-shape control is not that composition.

The reserve increment retains two Recovery worlds with fixed reserve amounts and
payment orders. It does not generate arbitrary reserve histories or replace
existing native/alternate-custody coverage. Public economic payout requires fair
submission and authenticated inputs. Mechanical portfolio deletion and slab
closure still use their existing owner/admin authorization. No claim of wholly
permissionless market retirement is made. No unfiltered suite or new Kani proof
was run.
