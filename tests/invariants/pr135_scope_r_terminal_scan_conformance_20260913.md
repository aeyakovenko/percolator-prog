# PR135 Scope R: terminal scan environmental reclassification

Branch: `codex/pr135-scope-r-terminal-scan-conformance-20260913`.
Worktree: `/tmp/percolator-pr135-scope-r-20260913`.
Fetched base: `9a4391f1`, the requested
`origin/codex/astra-open-holdout-ledger-20260912` at worktree creation.
Engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

## Scope and prior coverage

Row 424 is a coverage label only. The invariant statements, local production
code and tests on the requested base determine the probe. No external PR branch,
diff or test was inspected or copied. The two excluded worktrees were not edited.

Already covered on that base:

- INV-070 retired-slot reuse: public admin and permissionless reuse rejects behind
  cursor 2, with Live success controls and expiry-prefix rollback.
- INV-070 generated source actionability and INV-071 cursor-time: Fresh sources
  remain at or ahead of a persisted cursor; generated deadlines cross 256-slot
  boundaries with stock, rank and exact closure checks.
- INV-070 external surplus, native SyncNative and retained provider withdrawal:
  changes to raw custody or principal eligibility compose with terminal scanning.
- INV-071 prefix insurance and prefix recredit: asset-local payouts preserve peer
  claims; later expiry makes spent insurance on an earlier asset payable through
  the withdrawal consumer. Its module explicitly excludes scanner rediscovery.
- INV-088 resolved actionability: later-account cleanup enables an unchanged
  earlier claimant, without a persisted slab-scan restart.
- INV-063 normalization: consumer composition, authenticated expiry boundaries,
  Live/Resolved/Recovery consumers, and single/staggered retirement expiry.
  Retirement tests preserve custody and atomically reject a still-Fresh sibling.
- INV-071 generated terminal actionability composes receipts and repeated recredit,
  but its continuation oracle chooses the insurance withdrawal before another scan.

The new primary owner is
[cu/inv_070_terminal_scan_recredit.rs](cu/inv_070_terminal_scan_recredit.rs), mounted
under `inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_scan_recredit`.
The existing INV-071 public construction, transaction framing and decoded stock
oracle are factored for reuse. This is bounded public conformance evidence, with
no independent-discovery benchmark or whole-invariant closure claim.

## Guarantee and oracle

Sixteen System/SPL/ATA/wrapper histories cross both insurance sides, backing
amounts 61/307, exact/one-slot-late expiry, and scanner-first/withdrawal-first
continuations. Three portfolios receive input-derived payouts of 1200, 0 and
137 atoms after a 200-atom gain and 100-atom insurance spend. All portfolios
are publicly dematerialized before slab scanning. The insurer is distinct from
the market authority and does not sign terminal withdrawals.

At slot 43, a successful scan parks at asset 1 while its backing is Fresh.
Asset 0's insurance is spent and has no current payable budget. Authenticated
Clock advances to slot 44 or 45 without changing the market Account. The next
scan normalizes asset 1 and must invalidate the earlier prefix. Asset 0's complete
stored slot remains unchanged by that normalization, but its current entitlement
is now `min(100 receivable, 100 spent, released backing)`: exactly 61 or 100.

In eight histories, another CloseSlab instruction must rediscover that earlier
insurance, recredit exactly the entitlement and retain custody. An unpaid or
partly paid insurance budget blocks final closure. Eight withdrawal-first
controls independently reconstruct the same histories and recredit through the
asset-local consumer. Two unsigned payments and final close have identical
normalized economic outcomes in both orders.

The shared oracle checks each domain's Fresh backing, reserves, receivable,
insurance budget/spend, source and global summaries, zero remaining user/OI
obligations, actual SPL balances and fixed mint supply. It recomputes overlap from
decoded stocks, without engine transition or residual helpers, and compares it
with the input-owned entitlement. The new probe also frames the resolved payout
ledger, allowing exactly the backing addition to its snapshot residual.
Stock/reservation censuses supplement these assertions.

Every attempted campaign transaction frames complete tracked and compiled
Accounts, including absence, mint, wallets, portfolios and Clock; only the
calculated payer signature fee is excluded. Successful wrapper/SPL log counts
prove execution of the rollback prefixes. A rejected expiry/payment/close group,
a rejected scanner-recredit/close group, unpaid-close attempts and final closure
followed by an invalid System instruction cover distinct boundaries. Final
closure burns 0 or 207 atoms, pays the market authority no quote tokens, closes
the vault and reconciles exact tombstone rent and the authority's rent refund.

Non-vacuity: 16 histories, 8 order comparisons, 88 committed campaign
transactions, 104 exact rollbacks, 8 scanner rediscoveries and 32 unsigned
insurance payments. Fixture construction and user cleanup are additional calls.

## Implementation mismatch and separate commit

The original deployed artifact failed the new assertion immediately after
successful later expiry: persisted cursor was 1, expected 0. This is an observed
invalidation failure; no lost-payout exploit or severity assessment is claimed.
Code inspection showed that the next engine scan begins at that persisted cursor,
while the final retirement path does not revisit earlier recredit candidates.

Production-only commit `94f9007145d52dde95b7c6774d556c376066f026` changes
`handle_close_slab`: a `BackingExpired` outcome saves cursor zero. It preserves
the bounded single-step engine call and custody sequencing. No engine pin,
account layout, new aggregate or protocol instruction is introduced.

Related tests now expect the reset, including a second scan to park again before
an unexpired sibling. Generated finite bounds allow rewalking the asset chunks
after each source expiry; their lexicographic fresh-source/cursor ranks still
decrease. The generated receipt control counts post-expiry recredits instead of
requiring the now-invalid nonzero cursor. Its executed payout rollback remains.
Other control changes are cursor expectations and corresponding rank/call counts.

## Limits and row impact

The new probe has two assets, one historical insured loss, no live liens at the
scan boundary, no fees/funding, one standard SPL quote rail and cooperative market
authority for CloseSlab. The fixture is public but fixed; it is not a new history
generator. Other expiry consumers, new obligations, receipts still pending
during scanning, Recovery, refill, successful retired-slot reuse, authority
succession, multiple competing recredit beneficiaries and arbitrary environmental
writers are outside this increment. The fix may require additional bounded scan
calls after each expiry; no unchanged total-call guarantee is claimed.

Primary INV-070 gains scanner rediscovery coverage. INV-024/025 gain exact
entitlement and stock disposition evidence; INV-041 gains the route-order control;
INV-063/069/071 gain expiry, residue and bounded continuation evidence.
INV-033's insurance classification and INV-086/088's model/summary relations are
checked only for this finite composition, not all consume or lifecycle branches.

Row 424 remains **OPEN**. Its eight data fields are unchanged; only a coverage
comment is added. `open_findings.tsv`, `invariant_status.tsv` and every other row
status remain unchanged. INV-070 and INV-024 remain REFUTED_CURRENT; the other
listed invariants remain OPEN_EVIDENCE. There is no classification promotion.

## Validation

Fresh default-feature SBF builds and host compilation used only this worktree.
The target is a private 6 GiB tmpfs mounted inside the worktree because the shared
disk and /dev/shm were nearly full. No other target cache was copied or cleaned.
Final SBF SHA-256:
`49f49d1a8a7a3c20e4e3e09c635c4d5ce08925056030668b6838ccf7224800b0`.

- New exact selector: PASS, 1/1; final instrumented run 8.98s,
  peak 225888 CU under the shared 400000-CU bound.
- Initial adjacent control batch: 16/17 passed in 93.25s. The generated receipt
  control failed its obsolete `cursor > 0` non-vacuity counter after the reset.
  Its focused rerun after the counter/bound update passed 1/1 in 172.73s:
  16 worlds, 793 steps, 75 rollbacks, peak 317698 CU. All 17 distinct controls
  therefore pass across the batch and focused rerun; the other 16 were not rerun.
- Generated source control includes repeated rescans around 256-slot boundaries.
  The 5782-asset claim-free closure control passed in 23 calls, peak 136122 CU.
- Four metadata gates passed before production edits and in the final rerun
  (4/4, 0.01s). The exact new selector listing contains one test.
- `cargo fmt --all`, the formatting check, working/staged whitespace checks and
  post-commit `git show --format= --check HEAD` pass. The production commit also
  passes `git show --format= --check`. Status/findings ledgers have no diff.
- No full suite, Kani run or broad environmental invalidation proof is claimed.
  Existing unused-support and Solana future-compatibility warnings remain.

## Changed files

Production-only commit:

- `src/v16_program.rs`: invalidate the saved terminal prefix after backing expiry.

Conformance/documentation commit, all under `tests/invariants`:

- `cu/inv_070_terminal_scan_recredit.rs`: new primary probe and order control.
- `cu/inv_071_terminal_prefix_recredit.rs`: shared public fixture, stock oracle and
  exact transaction framing; retain the existing withdrawal-first selector.
- `cu/inv_070_zero_unattributed_terminal_residue_and_close_slab.rs`: mount the probe
  and update the retained-withdrawal cursor expectation.
- `cu/inv_071_crank_progress.rs`: expose the shared test-only fixture module.
- `cu/inv_070_generated_prefix_actionability.rs`: generated rewind oracle/bound.
- `cu/inv_071_generated_terminal_actionability.rs`: rewind oracle, finite bound
  and non-vacuity accounting for repeated expiry-funded insurance recredit.
- `cu/inv_070_terminal_prefix_reuse.rs`: post-expiry cursor expectation.
- `cu/inv_071_terminal_cursor_time.rs`: rewind, rescan, wait and rank controls.
- `cu/inv_071_terminal_prefix_insurance.rs`: post-expiry cursor expectation.
- `cu/inv_071_terminal_reserve_backfill.rs`: post-expiry cursor/rank expectations.
- `README.md`: owner, mismatch, limits and audit link.
- `coverage_reopenings.tsv`: coverage comment only, row 424 remains OPEN.
- `pr135_scope_r_terminal_scan_conformance_20260913.md`: this audit and commands.

## Exact commands

Exact setup/build commands:

```sh
git fetch origin codex/astra-open-holdout-ledger-20260912
git worktree add -b codex/pr135-scope-r-terminal-scan-conformance-20260913 /tmp/percolator-pr135-scope-r-20260913 origin/codex/astra-open-holdout-ledger-20260912
cd /tmp/percolator-pr135-scope-r-20260913
mkdir -p target
sudo -n mount -t tmpfs -o size=6G,uid=1001,gid=1004 tmpfs /tmp/percolator-pr135-scope-r-20260913/target
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir target/deploy -- --locked
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
```

The build-sbf command was repeated after the isolated production commit. Exact
behavioral commands (the new selector ran on the original and fixed artifacts):

```sh
cargo test --locked --offline --test v16_cu inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_scan_recredit::v16_program_terminal_scan_rediscovers_earlier_insurance_after_later_expiry -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_scan_recredit::v16_program_terminal_scan_rediscovers_earlier_insurance_after_later_expiry -- --exact --list
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::generated_prefix_actionability::v16_program_generated_scans_recompute_actionability_across_source_deadlines_and_prefixes \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_prefix_reuse::v16_program_terminal_prefix_rejects_retired_slot_reuse_with_exact_rollback \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::v16_program_terminal_scan_reconciles_external_surplus_arriving_after_cached_prefix \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::v16_program_retained_terminal_withdrawal_revalidates_expiry_after_scan_and_partial_payout \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::mixed_maturity::v16_program_mixed_maturity_terminal_residue_preserves_partition_and_close_retry \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_native_reclassification::v16_program_native_sync_after_terminal_prefix_reclassifies_only_external_surplus \
  inv_071_crank_progress::terminal_cursor_time::v16_program_persisted_scan_reclassifies_time_without_skipping_live_siblings \
  inv_071_crank_progress::terminal_prefix_recredit::v16_program_later_expiry_recomputes_scanned_asset_insurance_entitlement \
  inv_071_crank_progress::terminal_prefix_insurance::v16_program_scanned_insurance_withdrawals_preserve_peer_entitlements_across_late_expiry \
  inv_071_crank_progress::terminal_reserve_backfill::v16_program_terminal_prefix_blocks_reserve_backfill_across_expiry_and_retries_cleanup \
  inv_071_crank_progress::generated_terminal_actionability::v16_program_generated_receipts_and_reserves_have_constructible_terminal_progress \
  inv_063_backing_expiry_normalization::v16_program_retire_normalizes_unreferenced_lapsed_backing \
  inv_063_backing_expiry_normalization::v16_program_retire_staggered_backing_expiry_is_atomic_across_siblings \
  inv_063_backing_expiry_normalization::v16_program_backing_expiry_consumer_composition_is_source_complete \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::v16_program_terminal_stock_and_close_slab_composition_is_source_complete \
  inv_088_global_summaries_are_not_account_local_proofs::v16_program_every_wrapper_engine_transition_callsite_has_summary_disposition_and_witness \
  inv_077_bounded_work_and_maximum_shape_compute::v16_bpf_terminal_claim_free_surplus_close_stays_bounded_on_10m_market
cargo test --locked --offline --test v16_cu inv_071_crank_progress::generated_terminal_actionability::v16_program_generated_receipts_and_reserves_have_constructible_terminal_progress -- --exact --nocapture --test-threads=1
```

The control batch output is in `target/scope-r-controls.log`; the instrumented
probe and generated-control rerun outputs are in `target/scope-r-probe.log` and
`target/scope-r-generated-control.log`. These are local generated artifacts.

Exact metadata/hygiene commands:

```sh
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
cargo fmt --all
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
git show --format= --check 94f9007145d52dde95b7c6774d556c376066f026
git diff --exit-code 9a4391f1 -- tests/invariants/open_findings.tsv tests/invariants/invariant_status.tsv
sha256sum target/deploy/percolator_prog.so
```
