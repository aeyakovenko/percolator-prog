# Pending Loss Across Backing Expiry

Base: `775a043e182108c953717e831d86aa90b2008baa`, the locally available
`origin/codex/astra-open-holdout-ledger-20260912` tip when the worktree was created.
Worktree: `/dev/shm/percolator-terminal-claim-conformance-20260912-a91f`.
Branch: `codex/terminal-claim-conformance-20260912-a91f`.
Only local repository files and the pinned dependency source in a private copy of
the local Cargo cache were used. No fetch, remote PR/issue/branch inspection, push,
or protected-worktree file changes were performed.

## Executable Increment

```text
inv_039_pending_loss_obligation_durability::backing_expiry::v16_program_pending_losses_survive_late_backing_expiry_and_claimant_close_order
```

The selector in [cu/inv_039_pending_loss_backing_expiry.rs](cu/inv_039_pending_loss_backing_expiry.rs)
reuses `AttributionWorld`'s public System/SPL/ATA/wrapper construction and typed
pending-obligation census. LiteSVM supplies program binaries, authenticated Clock,
and signer SOL; no economic Account bytes are installed or patched.

Five deposits total 930,777 atoms. The bystander withdraws and transfers 97 atoms
to the backing provider, who deposits them in the first debtor's source domain.
Mint authority is then revoked. Integral positions of one and two lots, and price
moves of one and 19,999, independently fix debts of 1 and 39,998 atoms. Both holders
forfeit their exposure while retaining zero-basis, nonzero-loss-weight obligations.
Resolution at slot 20 preserves those obligations and both unbooked debtors.

Sixteen worlds cross mirrored sides, delivery at expiry slot 25 or late slot 31,
holder- versus debtor-triggered normalization, and both holder close/payout orders.
The first composed transaction stages backing expiry, a debtor SPL payout, and
the other holder's detach. That holder's waiting retry rejects at instruction 5
with `EngineNonProgress`. Exactly three wrapper successes and one SPL success
must precede rejection. Every compiled or tracked Account rolls back, including
presence, rent, token metadata, source stock, debt and obligation summaries; only
the separate payer's exact signature fee changes.

An identical normalization instruction then commits alone. Both pending obligations
and the original claims remain intact. The first debtor settles and its portfolio
is deleted before either claimant can receive a payout. Both holders detach in
the selected order, but another waiting retry must still reject atomically while
the second debtor remains unbooked. Settling that debtor enables exact owner-local
payouts in the selected claimant order.

The model distinguishes original backing expiry from newly collected debt:
settling principal creates 1 or 39,998 new fresh backing atoms, with the configured
6,480,000-slot horizon. Consuming those atoms for the respective claimants raises
only the corresponding spent/receivable counters. It cannot recreate the expired
97-atom reserve or forgive the other debtor. Every prefix checks capital, PnL,
remaining receipt value, SPL balances, obligation side/count/weight, source stocks,
raw aggregate headers, reservation census, fixed supply and unaffected Accounts.

| Role | Final SPL Atoms |
| --- | ---: |
| First holder | 200,001 |
| First debtor | 179,999 |
| Second holder | 339,998 |
| Second debtor | 210,002 |
| Bystander after funding backing | 680 |
| Backing provider / market authority | 0 |

All five portfolios close with exact rent accounting. One final slab call burns
exactly 97 atoms, closes the vault, and leaves a rent-minimum tombstone; the market
authority receives only the prescribed rent refund. Final mint supply equals the
930,680 atoms held by the five users. There are 80 terminal owner payouts, 80
portfolio deletions, 32 exact rollbacks and 16 single-call slab retirements.

## Overlap And Limits

| Existing Selector Under `v16_cu` | Why This Is Additional |
| --- | --- |
| `inv_039_pending_loss_obligation_durability::shared_holder::v16_program_shared_holder_pending_domains_survive_partial_detach_and_debtor_close_orders` | Shared-holder obligations without a funded late-expiry bucket. The new history keeps distinct holders and adds expiry before debt settlement. |
| `inv_039_pending_loss_obligation_durability::resolved_histories::v16_program_resolved_debtor_deletion_preserves_unsettled_cohort_attribution` | Debtor deletion and claim persistence without backing-expiry normalization. |
| `inv_039_pending_loss_obligation_durability::terminal_fees::v16_program_pending_cohort_terminal_fees_stop_at_resolution_and_reach_insurance_exit` | Nonzero maintenance and insurance exit without late backing expiry. |
| `inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_source_realization::v16_program_retained_receipts_preserve_identity_across_fresh_realization_or_expiry` | Existing nonzero receipts across source realization/expiry, without retained pending obligations. |
| `inv_071_crank_progress::terminal_cursor_time::v16_program_persisted_scan_reclassifies_time_without_skipping_live_siblings` | Persisted terminal-scan/time classification, without the two pending debtor domains. |

The selected uncovered dimension is pending loss combined with late backing expiry
and claimant order. Row 419 gains sampled evidence; row 417's retained receipt
identity remains owned by its existing selectors, and row 424 gains no new scan
invalidation claim. No shared-holder, receipt-spend/replay, retained-receipt
fresh/expiry, reserve-backfill, scanned-insurance-withdrawal or cursor-time selector
is duplicated. All invariant verdicts and OPEN row dispositions are unchanged.

These solvent, integral-lot, zero-fee/funding histories do not establish bankruptcy,
ADL, fractional rounding, multiple expiry waves, successful cursor invalidation,
earlier-slot insurance recredit, or absent-owner portfolio deletion. The terminal
claims here are fully source-realized after normalization; this is not a nonzero
receipt surviving a successful cursor mutation.

## Validation

Default-feature wrapper SBF built locked/offline in the isolated worktree using
platform-tools v1.52 and engine pin `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
Wrapper SHA-256:
`5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
The exact selector passes all sixteen worlds, with final-run peak cost 315,178 CU
below its 400,000-CU limit. The first development run caught the configured
five-slot signer delay; expiry now coincides with its permissionless boundary.
The oracle also explicitly accounts for fresh backing created by debtor settlement.
No production fix was needed.

The exact selector plus the five nearby controls above pass (6/6). All four
invariant index/status/summary/root checks pass (4/4), as do `cargo check --tests`,
repository-wide formatting and diff whitespace checks. Existing unused-support
warnings and the Solana 1.18 client future-compatibility notice remain. Production
sources, dependency pins, the CU root, invariant status and reopening TSVs match
the base. Only the new CU sibling, its parent mount, this audit and the invariant
README change.

Commands from the isolated worktree, using its private target/cache:

```sh
export CARGO_HOME="$PWD/target/cargo-home" CARGO_TARGET_DIR="$PWD/target/build"
export TMPDIR="$PWD/target" CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_039_pending_loss_obligation_durability::backing_expiry::v16_program_pending_losses_survive_late_backing_expiry_and_claimant_close_order \
  inv_039_pending_loss_obligation_durability::shared_holder::v16_program_shared_holder_pending_domains_survive_partial_detach_and_debtor_close_orders \
  inv_039_pending_loss_obligation_durability::resolved_histories::v16_program_resolved_debtor_deletion_preserves_unsettled_cohort_attribution \
  inv_039_pending_loss_obligation_durability::terminal_fees::v16_program_pending_cohort_terminal_fees_stop_at_resolution_and_reach_insurance_exit \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_source_realization::v16_program_retained_receipts_preserve_identity_across_fresh_realization_or_expiry \
  inv_071_crank_progress::terminal_cursor_time::v16_program_persisted_scan_reclassifies_time_without_skipping_live_siblings
cargo check --locked --offline --tests
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_program_invariant_harnesses_are_test_free_roots
cargo fmt --all -- --check
git diff --check
git diff --cached --check
```
