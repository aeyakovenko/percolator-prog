# Row 417: Independent INV-067 Coverage Audit

**Decision: true gap.** Keep row 417 `missing` in `open_findings.tsv`, OPEN in
`coverage_reopenings.tsv`, and INV-067 `REFUTED_CURRENT`. Existing invariant-owned
tests provide useful bounded conformance, but do not establish independent
detection of late backing reclassification erasing a resolved claim.
No benchmark or discovery mapping is promoted.

Audit base: `da83e0e6e550235258efba39ce83b9d6da0594ac` on
`origin/codex/astra-invariant-cycle-20260915`; engine pin:
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`. Work is isolated on
`codex/pr135-inv067-row417-audit-20260916`. The held-out row supplies only the
coverage question. Neither replacement PR's implementation, diff, tests nor
artifact was used. Production code and dependency pins are unchanged.

## Evidence Boundary

- [INV-067 discovery owners](stateful/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs)
  and `independent_discoveries.tsv` cover terminal dust and bankruptcy residuals
  (rows 283/330). They supply no discovery fingerprint for this class.
- [Generated overdue histories](cu/inv_067_receipt_overdue_history.rs) have an
  input-derived per-portfolio payout oracle, generated faces, shared ownership,
  claim cadence and aborted groups. However, both receipts exist before the
  clock advances past both expiries; the two source amounts and source order
  are fixed. `Oracle::apply` allows clearing only at `source_steps == 4`, after
  both releases and the source claimant's payout. This does not generate the
  independent product of claim completion/clearing and remaining future stock.
- [Claim materialization](cu/inv_067_terminal_claim_episode_materialization.rs),
  [fractional conversion](cu/inv_067_receipt_fractional_source.rs),
  [first payable atom](cu/inv_067_receipt_first_atom.rs), and the receipt
  partition, rail, fee and terminal-disposition siblings cover valuable
  additional finite products. Their reports retain OPEN/missing; passing them
  supplies no held-out detection evidence. The [partition oracle](cu/inv_067_receipt_partition_confluence.rs)
  itself has only replacement, top-up and one release actions.
- The shared [`ScenarioRunner::assert_receipt_transition`](../support/fuzz_model.rs)
  checks immediate face/paid monotonicity, bound replacement and SPL deltas.
  On receipt removal it computes due from the **post-transition engine rate**
  and accepts an unfinalized receipt clearing when the unreceipted bound is zero
  and that due was paid. It does not independently establish that no future
  stock release can increase entitlement, or retain a removed episode's claim
  for comparison after that release. Stock conservation alone cannot detect
  wrong allocation between former claimants and residual beneficiaries.
- The [INV-066/067 induction](kani/inv_066_resolved_payout_fairness_and_exact_once.rs)
  assumes a funded cohort under `RESOLVED_RATE_SUM_AXIOM`; it has no expiry or
  stock-reclassification transition. The [composition gate](cu/inv_066_resolved_payout_fairness_and_order_independence.rs)
  checks the engine pin, witness names and wrapper call ordering. The pinned
  engine's `proof_v16_terminal_resolved_receipt_core_restores_dematerialization`
  and `proof_v16_insolvent_resolved_receipt_clears_at_terminal_rate` construct a
  zero-unreceipted-bound haircut receipt without a later backing release.
  Their terminal-rate premise does not prove stability under that release.

The missing evidence is this transition/oracle composition, not a requirement
that independent discovery exhaust all histories. Finding-blind finite tests
can qualify when they demonstrably detect the class; these passing controls
and source/proof registrations do not establish that result.

### Receipt Reference-Model Follow-Up (2026-09-16)

Review at `b95c8155c3230c383b966628c8a622c47a367cca` also checked
`run_bounded_receipt_conflict_reference_frontier` in
[`tests/support/fuzz_model.rs`](../support/fuzz_model.rs). Its
`assert_bounded_receipt_conflict_edge` computes removal entitlement from the
observed payout-ledger rate. When `before.present` is false, it rejects receipt
resurrection but returns without recomputing the removed episode's entitlement.
Graph exploration and terminal-outcome equality therefore do not supply the
missing independent receipt-accounting oracle.

The required model must keep episode face and cumulative payments after removal,
derive later entitlement from public stock events, and detect a positive unpaid
delta even when no receipt remains. A useful sensitivity control would redirect
that delta to a residual beneficiary while preserving aggregate custody; the
episode oracle must reject it. This control is not implemented or claimed here.
Existing benchmark, reopening and machine-status guards already reconcile the
metadata. No non-duplicative public-sequence test or metadata check was identified
in this review; row 417 remains **OPEN/missing**, INV-067 **`REFUTED_CURRENT`**.

Follow-up validation: all six metadata selectors listed below pass (6/6). Only
the generated overdue-history CU selector was rerun: PASS (1/1), 66 worlds,
1,461 commits, 181 rollbacks, 396 portfolio closes, 66 slab calls, peak 541,178 CU.
The cached SBF hash and production/build inputs match the baseline recorded below.
Logs: `/tmp/row417-receipt-oracle-gap-20260916-{metadata,conformance}.log`;
private target: `/dev/shm/row417-receipt-oracle-gap-20260916-target`.

## Next Generic Coverage

1. Extend the INV-067 public history generator with an independent claim-episode
   ledger keyed by market/portfolio incarnation and episode. Retain face,
   authorized conversion/forfeit and cumulative paid value after receipt
   clearing or portfolio deletion. Derive entitlement and stock attribution
   from public economic inputs/events, not the engine's reported payout rate.
2. Generate receipt materialization, every payout route, zero-due retries,
   clearing/close attempts, source conversion and independently ordered backing
   expiries. Explicitly reach zero unreceipted bound while positive future
   releasable stock remains, including stock outside the claimant's remaining
   positions. Continue after each candidate terminal prefix and later release;
   do not make source exhaustion a generator prerequisite for close attempts.
3. At every commit require each unpaid episode to remain represented and each
   payout to match the independent entitlement delta. Require full rollback on
   rejected bundles, per-episode order/partition equivalence, bounded eventual
   payout, and exact final user/provider/insurance/residue attribution. Bind
   these checks to the stock-changing public routes; aggregate custody equality
   and a successful close are insufficient.
4. Establish sensitivity to premature receipt removal and incorrect later-rate
   allocation, then demonstrate detection with the unchanged generic oracle
   against a vulnerable implementation. Record the public trace and terminal
   economic loss before adding a discovery fingerprint. Do not import the
   held-out replacement test or fix as coverage ownership.

## Original Audit Verification

All six metadata checks below pass after the README edit. All five focused
controls pass: one composition gate and four public suites totaling 90 worlds.
The generated overdue suite reports 66 worlds, 1,461 commits, 181 rollbacks,
396 portfolio closes and 66 slab calls (538,178 peak CU). These are conformance
results, not independent reproduction of the held-out failure.

Host tests were rebuilt in this worktree's private `target`, seeded by copying
the existing dependency cache without hardlinks. The cached default-feature SBF
is `/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so`, SHA-256
`e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
`src`, `Cargo.toml` and `Cargo.lock` exactly match its recorded build baseline
`d809e9a563d9b8bf38f32648b32a15d75f526ec8`. No SBF rebuild, new Rust test,
Kani proof run, replacement-PR red/green result or full-suite result is claimed.

Run from `/home/anatoly/percolator-pr135-inv067-row417-audit-20260916`:

```sh
export CARGO_TARGET_DIR="$PWD/target"
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so
inv067_prefix=inv_067_terminal_payout_completeness_and_exact_once_settlement
inv079_prefix=inv_079_public_reachability_evidence

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_066_resolved_payout_fairness_and_order_independence::v16_program_resolved_payout_induction_composition_is_source_complete \
  "${inv067_prefix}::receipt_overdue_history::v16_program_generated_overdue_source_histories_preserve_receipt_identity_and_attribution" \
  "${inv067_prefix}::receipt_fractional_source::v16_program_fractional_source_conversion_preserves_receipts_through_same_bucket_expiry" \
  "${inv067_prefix}::claim_episode_materialization::v16_program_coowned_claim_episodes_survive_deferred_receipts_and_two_expiry_rollbacks" \
  "${inv067_prefix}::receipt_first_atom::v16_program_zero_paid_receipt_survives_plateau_and_first_atom_tail_revalidation"

cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --test-threads=1 \
  "${inv079_prefix}::v16_invariant_charter_and_index_are_complete" \
  "${inv079_prefix}::v16_invariant_audit_summary_matches_every_verdict_row" \
  "${inv079_prefix}::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming" \
  "${inv079_prefix}::v16_post_pr135_counterexamples_reopen_every_affected_invariant" \
  "${inv079_prefix}::v16_dated_open_security_finding_benchmark_is_non_overclaiming" \
  "${inv079_prefix}::v16_program_invariant_harnesses_are_test_free_roots"
```

Logs: `/tmp/pr135-inv067-row417-audit-20260916-logs/`. The private build cache
was removed after validation to reclaim disk space. That original audit changed
only this report and its README index entry; it left TSVs and executable sources
unchanged.
