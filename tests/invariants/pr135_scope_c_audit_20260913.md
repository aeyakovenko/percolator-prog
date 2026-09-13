# PR135 Scope C coverage audit, 2026-09-13

Repository: `/tmp/percolator-astra-watch.Cb2E7d`. Isolated worktree:
`/tmp/percolator-pr135-scope-c-20260913`; branch:
`codex/pr135-scope-c-20260913`. Base branch:
`codex/astra-open-holdout-ledger-20260912`, commit
`8f62a5c59a096c8ce678fa8684efe22fcbc1625e`.
Only this base's source and tests were consulted; no open-PR code or tests were
read or copied. Row numbers below are coverage labels.
`/home/anatoly/percolator-prog` was not accessed or modified.

## Retained change

The existing [generated terminal scan test](cu/inv_070_generated_prefix_actionability.rs)
already varies source placement around the 256-asset scan boundary, four backing
amounts/deadlines, clock cadence, transaction grouping and external surplus. A
new boundary probe would duplicate that generator. No new standalone probe was
created or retained.

Its rollback suffix previously contained malformed System instruction bytes.
That suffix was removed and replaced with a correctly encoded, owner-signed SPL
transfer between publicly created token accounts. Its amount exceeds the donor's
entire original supply, so it must fail with SPL `InsufficientFunds`, including
before donation and after the terminal prefix closes the vault. The suffix uses
the surviving destination account. This exercises a real account-balance failure
after accepted scanner, expiry, burn, sweep and rent operations.

The existing checker requires the exact failed instruction index, successful
wrapper/SPL prefix log counts and complete Account rollback, including absence,
metadata and the exact signature fee. It then commits the identical successful
prefix and checks the input-derived source/cursor rank, mint supply, destination
balance and tombstone/rent disposition. A funded source before its deadline is
an explicit wait with `EngineLockActive`; after deadlines pass, bounded public
continuation must reach closure. Insufficient donor funds are caller input, not
evidence of a protocol progress failure.

New evidence cell: **INV-080 exact rollback after valid SPL balance failure,
composed with INV-063/070/071/077/088 generated terminal expiry/scan continuation
(row 424)**. This strengthens the fault and continuation evidence; it does not
add a new source-state dimension or establish general INV-086 equivalence.
All economic accounts are constructed by System, SPL, ATA and wrapper
instructions. Harness-only operations load programs, fund SOL signers and advance
Clock. No economic Account image is injected by this test.

## Scope C ledger

These are bounded coverage cells, not closures of the listed rows. Test IDs refer
to the exact commands below. Existing rows 417/418/420/421/424/433 remain `missing`
in `open_findings.tsv`; the conservative `OPEN_EVIDENCE` and `REFUTED_CURRENT`
classifications in `invariant_status.tsv` are unchanged. Passing selected routes
does not overturn a refutation elsewhere. README and status tables were not edited.

| Row labels | Existing general evidence verified here | Remaining coverage boundary |
| --- | --- | --- |
| 417 | T2: receipt-local identity/floors, six claim/expiry orders, split/atomic catch-up, duplicate payout frames; INV-063/066/067/068/070. | The fixed receipt/source population is not a generator of every receipt, fee, conversion and reserve partition. |
| 418 | T4: two signed native-insurance payouts, public destination recreation, native token/lamport accounting, final slab closure. T8: supported-capacity expired-stock retirement on eligible quote rails. | Signed native insurance redemption and SPL-primary expired-stock burn are separate cells. T8 explicitly omits native-primary expired backing; the product is not established by either test. |
| 420 | T3: unsigned fresh provider principal and earnings, partial payments, all reserve payout orders, unpaid-principal expiry, final closure. T5: paid-prefix preservation across failed final closure. | Public beneficiary payout follows senior settlement and portfolio deletion. Arbitrary mixed receipt/pending-obligation/provider histories still need a joint oracle. |
| 421 | T3/T5: unsigned insurance-beneficiary payout alongside distinct provider stocks; payment order, lazy/paid ledger and final-close retry. T4 separately measures signed native redemption. | General beneficiary/operator histories and their composition with native stock, recredit and pending claims are not certified. |
| 424 | T1: generated deadline/prefix actionability and valid SPL failure rollback, then successful bounded retry. T6: clock advancement, public user payout, sibling expiry, exact prefix/rent rollback and terminal scan. | Insurance recredit plus receipts and other obligations across the persisted prefix remain separate finite witnesses, not a combined generated oracle. |
| 433 | T3/T5: beneficiary nonsigners receive exact principal, earnings and insurance; prior payments survive close rollback/retry. T7: keeper-created custody and unsigned user payout. | User payout, reserve payout and administrative retirement are distinct obligations. Neither unsigned payouts nor keeper custody repair proves administrator-free slab retirement. |

Public wrapper contracts were checked at
`verify_domain_withdrawal_preflight`, `handle_withdraw_backing_bucket`,
`handle_withdraw_backing_bucket_earnings`, `handle_withdraw_insurance_asset`,
`handle_close_resolved`, `handle_permissionless_crank_zero_copy` and
`handle_close_slab` in [v16_program.rs](../../src/v16_program.rs).
Resolved reserve routes permit beneficiary nonsigners while retaining identity,
generation, epoch, destination and senior-disposition checks. `CloseSlab` requires
the market authority, persists bounded scan outcomes and may retire stock before
SPL burn/transfer/close and rent finalization. T1 specifically checks rollback of
that composition without changing production behavior.

## Nonqualifying liveness and CU labels

The mapped selectors below were inspected, not executed in this audit. Most use
the older `V16CuEnv` constructor, which injects initialized mint/vault or blank
program accounts; its deposit helper also supplies token state. They remain
useful transition tests, but this audit does not count them as a history built
entirely by public account-creation/funding instructions. No historical finding
adapter or whole test suite was run.

| Labels | Existing selector / assertion | Disposition and missing cell |
| --- | --- | --- |
| 202 | `v16_program_unilateral_adl_effective_exit_matrix_preserves_bounded_cleanup`: effective OI reduction, reset finalization and owner capital exit. | A successful bounded continuation exists. This selector passes the exact effective quantity; the nonqualifying explanation's raw-budget clamp needs the separate INV-086 reduction-clamp matrix. Do not infer rejected overshoot or arbitrary-budget behavior from this selector alone. |
| 204 | `v16_program_fixed_blockers_remain_progressing`; dedicated `v16_attack_lapsed_live_source_backing_expires_bounded_and_owner_can_reduce`. | Retain the general expiry/owner-exit claim. The adapter only demands a nonzero closed-position count; a strict per-step rank and public account construction are stronger independent cells. |
| 219 | `v16_program_reset_carry_liquidation_matrix_preserves_progress`. | Its downstream prerequisite rejects while effective OI remains nonzero. That disposes of the proposed prerequisite; it does not certify liveness from a bilateral-zero-OI state. |
| 257, 287, 288 | `v16_program_privileged_policy_boundary_matrix_rejects_untrusted_callers`. | Wrong-authority withdrawal/policy requests reject and selected accounts remain exact. This is authorization coverage, not a proof of funded-deadline monotonicity, authorized-policy progress or complete compiled-account rollback. |
| 269 | `v16_program_dense_zero_delta_resolution_shape_matrix_keeps_terminal_exit_bounded`. | Distinguishes stale ordinary trading and a nonprogressing zero-delta hint from successful permissionless resolution and bounded full payout to one claimant. Other funded portfolios are not all drained. T8 independently verifies public terminal retirement at maximum capacity, but has no dense exposed portfolios. The combined dense-exposure/all-claimant-retirement cell remains missing here. |
| 297 | `v16_program_prospective_source_expiry_prerequisite_matrix_keeps_exit_live`. | Existing trace closes both portfolios and returns their 200,000,000 deposited atoms. It is a bounded successful route, not evidence about an unreachable downstream state or every source-expiry history. |
| 308 | `v16_program_b_budget_lock_prerequisite_rejects_post_adl_basis_reissue`. | Rejected post-ADL basis reissue is followed by successful owner reduction with two-sided OI descent. It does not exercise terminal B-budget progress from the rejected state. |

For the rest of INV-063 through INV-089: T2 checks bounded fairness and receipt
monotonicity (066/068); T3/T5/T8 check terminal normalization (069) and T7 supplies
an explicit environmental continuation (078/082). Account frames and stock
checks contribute local success-state evidence (081). No additional claims are
made for 064/065/072/074/075/076/083/084/085/087/089. The INV-086 stateful suite
already distinguishes generated histories, seeded frontiers and arithmetic
reference models; T1's input-derived expiry model is only one bounded cell of
that obligation. INV-079 metadata checks passed without changing classifications.

## Exact validation

Environment for the successful host commands:

```sh
cd /tmp/percolator-pr135-scope-c-20260913
export CARGO_TARGET_DIR=/dev/shm/pr135-scope-c-20260913-target
export PERCOLATOR_FUZZ_SBF=/tmp/percolator-pr135-scope-c-20260913-sbf/percolator_prog.so
export TMPDIR=/dev/shm/pr135-scope-c-20260913-tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0

# T1
cargo test --locked --offline --test v16_cu inv_070_zero_unattributed_terminal_residue_and_close_slab::generated_prefix_actionability::v16_program_generated_scans_recompute_actionability_across_source_deadlines_and_prefixes -- --exact --nocapture --test-threads=1
# T2
cargo test --locked --offline --test v16_cu inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_expiry_interleavings::v16_program_retained_claims_commute_with_late_expiry_normalization_after_catchup -- --exact --nocapture --test-threads=1
# T3
cargo test --locked --offline --test v16_cu inv_073_no_permanent_user_lock::v16_program_terminal_public_reserve_disposition_preserves_value_across_orders -- --exact --nocapture --test-threads=1
# T4
cargo test --locked --offline --test v16_cu inv_077_bounded_work_and_maximum_shape_compute::native_insurance_exit::v16_program_native_insurance_partial_redemption_reaches_bounded_terminal_exit -- --exact --nocapture --test-threads=1
# T5
cargo test --locked --offline --test v16_cu inv_073_no_permanent_user_lock::v16_program_absent_reserve_recipients_preserve_paid_prefix_through_final_close_rollback_and_retry -- --exact --nocapture --test-threads=1
# T6
cargo test --locked --offline --test v16_cu inv_071_crank_progress::terminal_cursor_time::v16_program_persisted_scan_reclassifies_time_without_skipping_live_siblings -- --exact --nocapture --test-threads=1
# T7
cargo test --locked --offline --test v16_cu inv_082_state_indexed_liveness_theorem::terminal_destination_recovery::shared_destination_recovery::v16_program_shared_destination_recovery_preserves_principal_and_single_rent_charge -- --exact --nocapture --test-threads=1
# T8
cargo test --locked --offline --test v16_cu inv_077_bounded_work_and_maximum_shape_compute::terminal_quote_variants::v16_program_quote_variants_retire_public_last_domain_backing_at_capacity -- --exact --nocapture --test-threads=1

cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming -- --exact --quiet --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --quiet --test-threads=1
cargo fmt --all -- --check
git diff --check
```

| Test | Result | Bounded evidence / peak CU |
| --- | --- | --- |
| T1 | PASS, 1/1, 59.39s | 54 worlds, 356 commits, 558 exact rollbacks; 302 scanner prefixes and 108 custody prefixes (includes donation rollback); 189,492 CU against 900,000. |
| T2 | PASS, 1/1, 24.82s | 24 worlds; six expiry/claim orders; 491,464 CU against 500,000. |
| T3 | PASS, 1/1, 7.15s | 12 reserve-order/expiry worlds; 237,495 CU. |
| T4 | PASS, 1/1, 1.52s | Four worlds; two payouts and one slab close each; 40,085 CU against 150,000. |
| T5 | PASS, 1/1, 1.12s | Two worlds, eight exact rollbacks; payout prefix 556,463, rejection 576,116, final close 573,911 CU against 1,200,000. |
| T6 | PASS, 1/1, 3.55s | Four worlds, 32 committed suffix calls, 52 exact rejections; success 137,225, rejection 148,351 CU. |
| T7 | PASS, 1/1, 6.37s | 16 worlds, 48 exact rollbacks, 32 portfolio payouts, 16 rent-once repairs; 215,507 CU. |
| T8 | PASS, 1/1, 186.97s | 16 worlds; public last-domain expiry at 255/256/257/5,782 assets on four eligible quote configurations; 2/2/3/24 close calls; close peak 53,582, lifecycle peak 214,735 CU against 300,000. |
| Metadata | PASS, 1/1 each | Both exact INV-079 checks; no invariant promotion. |
| Formatting / whitespace | PASS | `cargo fmt --all -- --check`, `git diff --check`. |

The default-feature SBF was rebuilt from this worktree with the locked engine
pin `394fd0bf2cb7d73df425eb3754dc3be1a0c44336` and platform-tools v1.52:

```sh
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/pr135-scope-c-20260913-target/deploy -- --locked
sha256sum /dev/shm/pr135-scope-c-20260913-target/deploy/percolator_prog.so
```

Build: PASS, 25.69s. SBF SHA-256:
`8a010d8283186721866a95a71d252a3265015fbf563d6ce566cd44f263659349`.
Initially only compiled cache directories were copied from the base repository's
target to the private target. The first T1 invocation, with
`PERCOLATOR_FUZZ_SBF=/dev/shm/pr135-scope-c-20260913-target/deploy/percolator_prog.so`,
exited 101 during host dependency compilation because `/dev/shm` filled; no test
executed. Recovery preserved the newly built SBF and cleaned only this task's
target:

```sh
cp -a /dev/shm/pr135-scope-c-20260913-target/deploy /tmp/percolator-pr135-scope-c-20260913-sbf
cargo clean --target-dir /dev/shm/pr135-scope-c-20260913-target
```

The successful T1 retry rebuilt host dependencies from scratch in 1m03s. Existing
dead-code and Solana future-compatibility warnings remain. No production fix,
vulnerable-pin reproduction, broad suite or external PR comparison was performed.

## Next most valuable cell

An input-derived terminal generator composing **receipts, insurance recredit and
backing expiry across a persisted scan prefix**, with absent beneficiaries and
publicly constructed accounts. It should identify the next required public
action or explicit timed wait at each state, check a decreasing progress rank,
and reconcile each claimant/reserve through final retirement. Exercise scan
boundaries and supported capacity without treating a wrong hint, insufficient
caller balance or authorized deadline change as a liveness failure. Existing
single-purpose witnesses cover parts of this product; they do not establish the
joint INV-063/067/070/071/073/077/078/082/086/088 cell for
417/420/421/424/433.

In particular, [earlier-asset recredit](cu/inv_071_terminal_prefix_recredit.rs)
already checks an explicit asset-local withdrawal after later expiry, but
explicitly excludes scanner rediscovery. The next increment must add the joint
generated actionability oracle, rather than duplicate that directed withdrawal.
