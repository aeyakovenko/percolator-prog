# Row 417: First Payable Receipt Atom

## Scope and provenance

Exactly one new LiteSVM test is mounted as INV-067's `receipt_first_atom` child.
The isolated clone is `/dev/shm/percolator-row417-identity-20260916`, cloned from
`/tmp/percolator-astra-invariant-cycle-20260915-run` on
`codex/astra-invariant-cycle-20260915`. Origin was set to the source's GitHub
remote and that branch fetched. The fast-forward check confirmed the starting
HEAD equals current origin: `d34b98a8f64b10e03c4f48264ca9eb5a45589fc7`.
Only a local commit is intended; no push, PR or external message.

The user's `/home/anatoly/percolator-prog` and the clone source are not edited.
All changes are below `tests/invariants`; production, Cargo files, fixtures and
all invariant TSVs (including nested TSVs) remain unchanged.
Row 417 remains **OPEN**, benchmark evidence **missing**, and INV-067 remains
**REFUTED_CURRENT**. This product does not close or promote any status.

The required SBF is reused through `PERCOLATOR_FUZZ_SBF`:
`/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so`, SHA-256
`e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
No fresh SBF build is claimed. Host dependencies were copied from
`/dev/shm/lane11-20260915-host` into a private target, then Cargo rebuilt the
checkout's test binary. Engine pin remains
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

## Distinct boundary

An existing receipt with positive face but zero paid junior value is still a
future claim. Neither a zero-due replay nor an intermediate stock release that
leaves its floor at zero may retire it. When a later source release crosses the
first-atom threshold, a previously valid request without transfer accounts must
be revalidated and reject atomically. The original complete request must still
pay exactly one atom, independently of a previously paid peer and the source
claimant's settlement order.

| Existing product audited | Boundary added here |
| --- | --- |
| `receipt_rounding_threshold` | Existing faces 700/1000/1300 already have positive initial payments; its 839/840/841 residual boundary empties the vault. Here a positive receipt starts unpaid, stays unpaid through one release, then first requires the transfer tail. |
| `receipt_repeated_stock`, `receipt_overdue_history` | Existing whole-lot faces are at least 50; the smallest already pays at initial residual 501. These products do not exercise zero paid junior value or a previously admissible shortened request becoming insufficient. |
| `claim_episode_materialization`, `receipt_deferred_fee` | These defer receipt creation; this receipt is already materialized with capital paid, nonzero face, and exactly zero junior payment. |
| `receipt_partition_confluence`, mixed crank/top-up batches, `receipt_expiry_interleavings` | Transaction order and rollback are reused techniques. The new invariant boundary is first payment after a zero-value plateau plus conditional transfer-account validation. |
| `receipt_source_realization`, `receipt_conversion_then_expiry`, `receipt_coowned_conversion`, `receipt_fractional_source`, late-fee products | No new conversion/denominator-refinement, co-ownership, fractional source allocation, maintenance fee or authority product is claimed. The fractional position constructs a four-atom ordinary junior face; both late sources expire. |
| `receipt_rail_history`, `receipt_close_slab_rail`, `receipt_post_close_custody`, destination recreation | No alternate rail, ATA/vault recreation, slab close or tombstone replay is added. |
| INV-068 identity substitutions, delegated destination/vault and co-owned stale-tail tests | Their failures concern substituted/delegated accounts or a deleted sibling. This unchanged three-account request is valid at zero due, invalid at positive due, then valid again after payment. |

The product therefore remains an integration candidate. It is not another
enumeration of the existing positive-payment histories.

## Public trace and independent oracle

The shared builder now expresses trade sizes as claim faces for the fixed
100-to-150 mark move. Existing constructors multiply their original whole-lot
inputs by 50, preserving their exact instructions and cadence. Only new
sub-lot claimants defer intermediate settlement until the final mark. System,
SPL and wrapper instructions create every economic account and transition.
LiteSVM supplies program loading, signer SOL and Clock warps; there is no
program-owned byte mutation, engine transition call or simulated poststate install.

1. Deposits, trades, debtor settlement and public resolution produce three
   positive faces `[4, 1000, 1996]`, total 3000. Each claimant starts with 1000
   capital. Two pending source domains hold 161 and 189 atoms, expiring at slots
   13 and 15. The initial junior residual is 501.
2. The small and large junior portfolios create receipts at slot 12, receiving
   1000 and 1333 respectively. Their receipt junior payments are 0 and 333.
   Retain the full claim requests, the source close, and the small claim with
   its account list shortened to owner/market/portfolio only.
3. The shortened claim commits as an exact no-op. At the first expiry, submit
   shortened claim, normalization, large claim, shortened claim, large replay.
   Only the large peer transfers, reaching 440 junior atoms. Roll back this
   whole prefix with an invalid System suffix, then commit it unchanged.
   The small receipt remains present, nonfinal and unpaid at residual 662.
4. At the second expiry, the same shortened claim still succeeds before stock
   normalization. Normalize and optionally complete source settlement, then
   pay the large peer. A shortened small-claim suffix now rejects with
   `NotEnoughAccountKeys`. Repeat the identical failed bundle twice with fresh
   blockhashes. All economic Accounts, message accounts and Clock are restored;
   the fee payer's complete Account differs only by one signature fee.
5. Retry unchanged normalization bytes with the retained complete claims in
   either order. The small owner receives exactly one junior atom and the large
   owner reaches 566. The source owner receives 1283 before or after these
   payments. Source expiry is tested at the deadline and one slot late.
6. While the source bound remains, the now-paid shortened claim is an exact
   no-op. Once that bound is replaced, it clears the receipt without a transfer.
   Roll back both receipt clears, commit them, then replay all retained claims
   with zero transfers and exact Account equality. Verify unchanged portfolio
   provenance, incarnation and position epochs before owner-signed deletion.
7. All six portfolios close, with exact rent returned to the market. Final user
   payments are `[1001, 0, 1283, 0, 1566, 0]`. The vault holds exactly one booked
   rounding atom: `851 - (1 + 283 + 566)`. The provider retains its never-deposited
   atom. Fixed SPL supply is 3852; capital, positive PnL, source bounds, insurance
   and provider earnings are zero after all exits.

Expected amounts come from public inputs and integer floors, never from the
observed payout rate or an engine payout helper. Every checkpoint compares
entire expected receipts, rate/snapshot fields, bound replacement, stock expiry,
per-owner token balances and aggregate custody. Successful transfer counts are
required in logs even for transactions that subsequently roll back.

During development, the first run stopped at the builder's four-atom face
assertion: cranking this tiny position at every intermediate mark truncated each
gain to zero. The final seed settles the affected claimants once at the final
mark; their exact faces are asserted before resolution. A Rust borrow-order
compile error was also corrected. Neither was a terminal-receipt behavior bug.

## Exact validation commands

```sh
cd /dev/shm/percolator-row417-identity-20260916
export CARGO_TARGET_DIR=/dev/shm/percolator-row417-identity-20260916-target
export TMPDIR=/dev/shm/percolator-row417-identity-20260916-tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_first_atom::v16_program_zero_paid_receipt_survives_plateau_and_first_atom_tail_revalidation -- --exact --nocapture --test-threads=1

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::late_expiry::v16_program_retained_claim_identity_survives_late_expiry_recipient_rotation_and_atomic_retry \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_fractional_source::v16_program_fractional_source_conversion_preserves_receipts_through_same_bucket_expiry \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_repeated_stock::v16_program_receipts_preserve_identity_through_two_stock_releases_and_reversed_priority \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::claim_episode_materialization::v16_program_coowned_claim_episodes_survive_deferred_receipts_and_two_expiry_rollbacks \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_overdue_history::v16_program_generated_overdue_source_histories_preserve_receipt_identity_and_attribution \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_rail_liquidity::rail_history::v16_program_two_expiry_receipts_share_exact_once_entitlement_across_quote_rails \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_late_fee_reclassification::v16_program_late_fee_reclassification_preserves_receipt_faces_and_claimant_order \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_rounding_threshold::v16_program_late_receipt_rounding_threshold_preserves_zero_vault_settlement

cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence:: -- --nocapture --test-threads=1 \
  --skip v16_program_fixed_blockers_remain_progressing \
  --skip v16_public_terminal_classifier_exhausts_normalized_outcome_space \
  --skip v16_public_trace_schema_detects_out_of_band_economic_mutation \
  --skip v16_public_trace_terminal_classifier_requires_complete_economic_evidence

rustfmt --edition 2021 --config skip_children=true --check \
  tests/invariants/cu/inv_067_receipt_first_atom.rs \
  tests/invariants/cu/inv_067_terminal_claim_late_expiry.rs \
  tests/invariants/cu/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs
git diff --check
git diff --cached --check
git show --format= --check HEAD
git diff --exit-code d34b98a8f64b10e03c4f48264ca9eb5a45589fc7 -- \
  src Cargo.toml Cargo.lock tests/fixtures \
  ':(glob)tests/invariants/*.tsv' ':(glob)tests/invariants/**/*.tsv'
git diff --name-only d34b98a8f64b10e03c4f48264ca9eb5a45589fc7 HEAD
```

## Results and limits

New exact selector: PASS, 1/1 test, eight public histories, 32 exact rollbacks,
32 rolled-back SPL transfers, 48 portfolio closes. Peak measured transaction
CU is 722,050; rejected-bundle peak is 703,004, both under 900,000. Maximum
serialized transaction is 740 bytes under 1232. These continuation measurements
exclude the inherited setup and do not claim maximum-shape coverage.

The eight adjacent exact selectors pass together (8/8, 260.06 seconds).
The new exact selector takes 9.96 seconds after host compilation. The table
uses module names; the complete exact selector commands are above.

| Adjacent control | Result and measured peak CU |
| --- | --- |
| `claim_episode_materialization` | PASS; 12 worlds, 60 rollbacks; 377,373 CU |
| `late_expiry` | PASS; 4 worlds, 8 paying-prefix rollbacks; 321,267 CU |
| `receipt_fractional_source` | PASS; 4 worlds, 4 rollbacks; 617,544 CU |
| `receipt_late_fee_reclassification` | PASS; 24 worlds; 624,349 CU |
| `receipt_overdue_history` | PASS; 66 worlds, 181 rollbacks; 553,178 CU |
| `receipt_rail_liquidity::rail_history` | PASS; 32 worlds, 192 rollbacks; classic/native 769,266/778,460 CU |
| `receipt_repeated_stock` | PASS; 12 worlds, 12 rollbacks; 365,364 CU |
| `receipt_rounding_threshold` | PASS; 36 worlds, 72 rollbacks; 435,373 CU |

Scoped INV-079 metadata run: PASS, 13/13 in 0.73 seconds, including charter/index,
authoritative machine status, dated benchmark, coverage/retry registries,
reopenings and harness ownership. An initial run used the same module selector
without the four `--skip` arguments. It selected 17 tests: all 13 metadata tests
passed, while four runtime tests failed at the common missing authenticated-
matcher artifact prerequisite, before exercising their scenarios. That run is
not reported as green. The artifact is hardcoded below protected
`tests/fixtures/auth_matcher/target/deploy`; no artifact, symlink or code change
was installed there. These unrelated runtime scenarios are outside this scoped
metadata validation.

Touched-file rustfmt, working/staged whitespace checks and the protected-path
diff pass. The protected comparison is empty for `src`, Cargo files, fixtures,
and all direct/nested invariant TSVs. Final commit whitespace and changed-file
checks are recorded with the local commit close-out.

Logs are `new-test.log`, `adjacent-controls.log`, `metadata.log` (the broad
attempt) and `metadata-scoped.log` in the private TMPDIR. Existing harness
dead-code and Solana client future-compatibility warnings remain.

No real current public-route behavior bug was found. The history has fixed
faces/stocks, zero fees/funding/rewards, one classic SPL rail, distinct owners,
and two source expiries. It does not test fresh source conversion, authority
succession, native custody, arbitrary histories, CloseSlab or a full-suite/Kani
proof. Final booked rounding is explicitly accounted for, not burned here.

Exact changed files:

- `tests/invariants/cu/inv_067_receipt_first_atom.rs` (one new test).
- `tests/invariants/cu/inv_067_terminal_claim_late_expiry.rs` (public builder).
- `tests/invariants/cu/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs` (child mount).
- `tests/invariants/README.md` (bounded evidence, unchanged status).
- `tests/invariants/row417_receipt_first_atom_20260916.md` (this report).
