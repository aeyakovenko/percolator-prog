# Receipt and pending-destination coverage, 2026-09-12

Base: `39022191e695d6702fcb3fcdb5b7cc270dc927d2`, the supplied
`origin/codex/astra-open-holdout-ledger-20260912` ref. Branch:
`codex/astra-terminal-receipt-gap-20260912-7c9e`. Worktree:
`/tmp/astra-terminal-receipt-7c9e`, with independent Git metadata in
`/tmp/astra-terminal-receipt-7c9e.git`. The original checkout and its Git metadata
were not modified. Only the base's tests and documentation informed the changes;
no open PR implementation or tests were inspected or used. Row 434 was ignored.

## Coverage added

`rg` first surveyed the requested invariant owners, then searched for combinations
of receipt/pending state with repair, missing destinations and keeper continuation.
The existing destination-recovery tests have capital-only portfolios. The existing
receipt-expiry/recipient-rotation and resolved-cohort tests have intact SPL custody.
Two new tests compose these boundaries using the existing public fixtures:

| Selector suffix | New relation and independent oracle |
| --- | --- |
| `receipt_destination_recovery::v16_program_recreated_destinations_preserve_paid_receipts_across_expiry_without_owners` | Sixteen worlds cross slots 13/17, claimant order, split/combined repairs, and zero/above-rent pre-funding. Already-paid tokens move to separate accounts owned by the same claimants before their ATAs close. All portfolio-owner SOL is drained and those keypairs are dropped. Retained claims recover through keeper-funded ATA reconstruction after late expiry. Original faces 700/1,000/1,300 and residuals 501/851 determine every payout; the final owner totals are 1,198/0/1,283/0/1,368, with exactly two booked rounding atoms and one never-deposited provider atom remaining. |
| `pending_destination_recovery::v16_program_pending_cohort_repair_is_atomic_and_keeper_settlement_preserves_attribution` | Eight worlds cross mirrored sides, which cohort loses custody, and debtor settlement order. A public ATA closure leaves a resolved holder's zero-basis obligation intact. Repair plus detachment plus a waiting retry must roll back the new ATA, rent and original loss weight. Repair and detachment then commit together; original debtors settle 21 and 27,998 atoms. The five exact final owner totals are 200,021/179,979/327,998/222,002/777. |

The receipt oracle preserves the entire receipt identity except its paid/finalized
fields, and independently checks portfolio ID, position epoch and provenance.
Receipt consumption is allowed only at terminal completion with its full expected
payout. Earlier withdrawals remain attributed to the original owner's separate
SPL account throughout repair; repeated idempotent repairs and retained top-ups
cannot charge rent or pay again. A failed expiry/repair/payment/deletion bundle
must restore the pre-expiry ledger, receipt, account presence and staged rent.

The pending test reuses the original input-derived cohort model after successful
repair. Each prefix checks leg basis, original loss weight, pending counts, OI,
capital, PnL, source state and owner payouts. The failed repair/detachment prefix
compares the original market and obligation Accounts byte-for-byte. All owner
system accounts are empty throughout keeper settlement; no owner signs it.

Both tests use real wrapper/System/SPL/ATA instructions. Harness inputs are program
installation, initial signer SOL, Clock and blockhashes. No economic state bytes
are injected or edited. Every measured transaction checks its sole keeper signer,
signature, packet size, CU, full compiled/tracked Account frame on errors, and
exact payer fee plus successful rent charge. Mint authority is revoked; supply,
custody, every owner's tokens, and retained portfolio/vault/account rent reconcile.
The shared transaction helper keeps existing tests' 300,000-CU limit; the new
four-instruction receipt bundles have an explicit 600,000-CU limit.

## Remaining gaps

All requested holdouts remain OPEN. These are bounded current-code conformance
tests; historical vulnerable pins were not tested and no invariant verdict changes.

| Holdout | Limit after this increment |
| --- | --- |
| 410 | Abandoned administrative reserve disposition is not proved by user custody repair. |
| 417 | Adds receipt identity, late-expiry repair and claim-order composition; arbitrary faces, multiple expiry/reclassification waves and quote variants remain outside it. |
| 418 | Neither new fixture uses native wSOL or Token-2022; native terminal insurance retirement remains separate. |
| 419 | Adds custody failure and repair while resolved obligations persist; bankruptcy residuals, ADL, fractional positions and nonzero fees/funding remain outside the solvent cohort oracle. |
| 420/421/433 | The unsigned recovery paths pay users. Backing principal, provider earnings and insurance beneficiaries still need their own missing-signer conformance evidence. |
| 424 | Destination reconstruction does not establish invalidation of a previously scanned asset prefix. New obligations or environmental reclassification behind that prefix remain separate. |

The new suffixes end at economic completion. They account for retained rent rather
than claiming unsigned portfolio deletion or administrative slab retirement.
The receipt suffix retains exactly two rounding atoms for the existing final burn;
the adjacent receipt/cohort retirement tests exercise that later close boundary.

## Validation

Wrapper rebuilt from this worktree with locked/offline platform-tools v1.52 and
default features, using a private copy of the base audit's build cache. Engine pin:
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`. SBF SHA-256:
`5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.

Both new exact selectors pass: **24 worlds, 40 reconstructed ATAs and 32 exact
rollbacks**. Final measured peaks are **330,582 CU** for receipt transactions and
**167,969 CU** for pending-cohort transactions. All **12 adjacent selectors** below
pass, as do the invariant charter/index (**1/1**), `cargo fmt --all -- --check` and
`git diff --check`. Existing unused-support and Solana future-compatibility warnings
remain. Production code, support fixtures, Cargo pins and holdout ledgers are unchanged.

Exact new selectors and validation commands:

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-terminal-receipt-7c9e-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export TMPDIR=/dev/shm CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_039_pending_loss_obligation_durability::resolved_histories::pending_destination_recovery::v16_program_pending_cohort_repair_is_atomic_and_keeper_settlement_preserves_attribution \
  inv_082_state_indexed_liveness_theorem::terminal_destination_recovery::receipt_destination_recovery::v16_program_recreated_destinations_preserve_paid_receipts_across_expiry_without_owners
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::v16_program_terminal_earned_fee_succession_preserves_paid_prefix_and_insurance \
  inv_039_pending_loss_obligation_durability::resolved_histories::v16_program_settled_pending_cohorts_reach_exact_terminal_slab_close \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::late_expiry::v16_program_retained_claim_identity_survives_late_expiry_recipient_rotation_and_atomic_retry \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_expiry_interleavings::v16_program_retained_claims_commute_with_late_expiry_normalization_after_catchup \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_terminal_disposition::v16_program_receipt_terminal_suffix_partitions_rounding_burn_surplus_and_rent \
  inv_068_receipt_uniqueness_and_monotonic_topups::v16_program_same_owner_receipts_keep_independent_topups_and_terminal_replays \
  inv_071_crank_progress::terminal_cursor_time::v16_program_persisted_scan_reclassifies_time_without_skipping_live_siblings \
  inv_073_no_permanent_user_lock::absent_insurer_spent_retirement::v16_program_absent_reserve_roles_preserve_recredited_insurance_after_backing_expiry \
  inv_073_no_permanent_user_lock::absent_provider_expiry_retirement::v16_program_absent_provider_staggered_expiry_reaches_funded_terminal_retirement \
  inv_077_bounded_work_and_maximum_shape_compute::terminal_quote_variants::v16_program_freezable_quote_terminal_retry_preserves_retirement_and_rent \
  inv_082_state_indexed_liveness_theorem::terminal_destination_recovery::shared_destination_recovery::v16_program_shared_destination_recovery_preserves_principal_and_single_rent_charge \
  inv_082_state_indexed_liveness_theorem::terminal_destination_recovery::v16_program_missing_destination_has_keeper_only_terminal_recovery
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
```
