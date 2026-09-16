# Rows 419/435: mixed debt ADL, rebalance and residual preemption

## Disposition and provenance

Exactly one new substantive LiteSVM product is retained, mounted under
`inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::adl_rebalance`.
It exercises a mixed owner's live debt-side ADL, owner reduction and side
finalization while a separate pending creditor residual survives. It does not
claim another underfunded-receipt product. No current production bug was found.
Rows **419/435 remain OPEN/missing**, INV-039 remains `REFUTED_CURRENT`, and all
machine status files are unchanged.

- Isolated clone: `/dev/shm/astra-rows419-435-mixed-adl-20260916`.
- Source clone: `/tmp/percolator-astra-invariant-cycle-20260915-run`.
- Local branch: `codex/astra-invariant-cycle-20260915`; local commit only, no push.
- Freshly fetched origin/base: `d34b98a8f64b10e03c4f48264ca9eb5a45589fc7`.
- Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Reused SBF: `/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so`.
- SBF SHA-256: `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
- Cargo/rustc 1.90.0; rustfmt 1.8.0-stable. Host outputs and logs are under
  `/dev/shm/astra-rows419-435-mixed-adl-20260916-{target,tmp,logs}`.
- No edits to `/home/anatoly/percolator-prog`, production, Cargo files, fixtures
  or any root/nested invariant TSV. Only `tests/invariants` files are changed.

## Audit and non-overlap

| Existing owner | Existing boundary | Distinct boundary in this product |
| --- | --- | --- |
| [Mixed underfunded receipt](cu/inv_039_mixed_underfunded_receipt.rs) | Liquidation produces nonunit ADL before a genuine partial receipt; later source realization/expiry, senior exit and terminal recredit use a checkpoint-derived junior book. | Live peer ADL on the mixed owner's **debtor** leg, then owner rebalance and side finalization while its different creditor domain still has an unbooked residual. Entitlement arithmetic is input-derived. |
| [Lane 25 source expiry](cu/inv_039_mixed_role_unsettled_expiry.rs) | Source backing expires before either mixed role settles; unit ADL. | Fresh source backing is maintained. Effective exposure is removed before preemption without erasing accrued debt or creditor loss weight. |
| [Lane 21 debt attribution](cu/inv_039_mixed_role_close_preemption.rs) | Expired opposing close preempts into Recovery/Resolved with both mixed roles still unsettled; unit ADL. | ADL and rebalance settle/remove the debtor leg first, followed by side finalization while the creditor residual remains pending. |
| [Funding order](cu/inv_039_mixed_role_funding_resolution.rs), [funding insurance](cu/inv_039_mixed_role_funding_insurance.rs) | Nonzero funding, two terminal orders, paid receipt retry; the insurance child adds still-unsettled K/F and input-derived debt/insurance-domain attribution. | No funding claim is added. Those tests do not reduce ADL-effective mixed debt through the live owner route. |
| [INV-051 effective quantity](cu/inv_051_canonical_adl_effective_quantity.rs), [INV-073 cleanup](cu/inv_073_no_permanent_user_lock.rs) | Single-pair ADL exit, raw/effective distinction, side reset and owner exit. | The owner also holds another asset's zero-basis creditor obligation; clearing/resetting its debt side cannot remove that obligation or pay its B loss. |
| [INV-039 cohort reduction](cu/inv_039_pending_loss_cohort_reduction.rs) | Same-domain holder reduction retains unpaid cohort weight, at unit ADL. | A separate debt asset has nonunit ADL and is completely removed; the creditor domain is retained through preemption. |
| [INV-037 partition](cu/inv_037_exact_residual_partition.rs), [INV-076 drift](stateful/inv_076_close_drift_residual_durability_and_finalization_atomicity.rs), [INV-048 OI](cu/inv_048_matched_trade_and_open_interest_coherence.rs) | Insurance/support residual partition, same-asset drift, and canonical position/OI census and route composition. | The exact residual partition is composed with mixed debt-side ADL, owner reduction, side finalization, and opposing-close expiry in one public history. |

The new dimension is checked independently of terminal equality: after ADL,
raw basis remains two lots and effective debt is one lot; the full original
36,000/216,000 debt still has to settle. After owner reduction and side reset,
the creditor leg must exactly equal its earlier runtime record, including
zero basis, one-lot loss weight and its unsettled B snapshot. The active close
must likewise exactly equal its original record until residual booking.
An implementation that halves accrued debt with exposure, clears both owner
legs, counts previous-reset raw residue as OI, or credits ADL quantity as
residual payment fails before any receipt retry.

## Public history and oracle

The 32 worlds cross two debts (36,000/216,000), both side orientations, both
asset assignments `[1,2]`/`[2,1]`, exact-effective/raw-basis rebalance requests,
and direct resolution/permissionless expired-close preemption. Each world is
constructed independently through the existing public `AttributionWorld` and
mixed-role setup; no private engine transition or program-owned byte mutation
is used. LiteSVM only loads programs, funds initial wallets and advances Clock.
Economic state is created by System, SPL Token, ATA and wrapper instructions.

1. Deposit `[400000,180000,300000,250000,777]`. Owner 0 is a one-lot creditor
   against owner 1 and a two-lot debtor against owner 2 on another asset.
   Authenticated marks produce a 200,000 creditor gain. Matched creditor exit
   leaves owner 1 with a 20,000 active-close residual and owner 0 with a
   zero-basis, one-lot pending creditor weight. The second mark path creates
   36,000/216,000 cross-asset debt, still unsettled on owner 0.
2. Owner 2 publicly rebalances away one of its two lots. This applies ADL to
   owner 0's debt side: `A = ADL_ONE / 2`, two-lot raw basis remains untouched,
   and both sides' effective OI becomes one lot. Owner 0's K snapshot remains
   zero, so the accrued debt is still due in full.
3. Owner 0 requests either one effective lot or two raw lots through
   `RebalanceReduce`. Both remove exactly one effective lot, clear the debtor
   leg and settle the full original debt. The creditor leg and the bankrupt
   close remain unchanged. Both effective OI totals are zero. Public
   `FinalizeResetSide` restores the mixed owner's debt side to Normal/unit A
   without touching the still-pending creditor leg or close residual.
4. Warp past the opposing close deadline. One branch directly resolves; the
   other uses two public cranks to enter Recovery then Resolved with reason
   `ActiveBankruptCloseCannotProgress`. Preemption preserves asset state,
   source credit, backing buckets and the original close record.
5. After unsigned-owner grace, the first debtor continuation finalizes exactly
   `20000 = 20000 B + 0 residual`. Later owner settlement charges that B loss
   only to owner 0. Up to 16 sweeps finish all portfolios, with strict progress
   in each nonterminal sweep. The already-paid owner-2 receipt accepts one
   exact no-op `ClaimResolvedPayoutTopup`. All five portfolios then delete with
   exact rent transfers to the market.

The parent input book supplies only fixed debt/support/face arithmetic. The
child independently checks each owner's capital + PnL + unpaid receipt + SPL
payout at every checkpoint and continuation, including the pre-settlement
debt stage. It checks exact residual categories, domain-local source ownership,
effective OI via independent ceiling arithmetic, current-epoch loss weight,
previous-reset residue, stored and pending counts, stock/reservation censuses,
mint supply and vault-plus-owner custody conservation. ADL quantity and retired
face metadata are never added as residual payments.

| Debt | Realizable creditor support | Retired support face | Final owner token vector | Final vault |
| --- | --- | --- | --- | --- |
| 36,000 | 36,000 | 40,000 | `[540000,0,336000,250000,777]` | 4,000 |
| 216,000 | 180,000 | 200,000 | `[344000,0,516000,250000,777]` | 20,000 |

All requests, sides, assignments and resolution branches must match those
input-derived values. The larger debt also retains the parent's two-stage
fractional peer-conversion floor: 35,999 converted atoms and a 180,001 receipt
face. No terminal expected value is inferred from observed payout balances.

## Rollback and CU evidence

Each peer rebalance, mixed-owner rebalance, side finalizer and first residual
booking is first executed as a successful prefix before an unsigned portfolio
deletion fails with exact `ExpectedSigner` at transaction instruction 3. Both
preemption transitions receive the same check in the 16 preempted worlds.
The existing transaction helper proves prefix success from program logs and
compares every Account in the complete transaction/world frame, with only the
actual signature fee deducted from the payer. It also checks the total lamport
sum. The same prefix instruction bytes then commit successfully, preserving
all non-target Accounts.

The product passes 32 worlds, **160 successful-prefix rollbacks, 32 side
finalizations, 32 paid-receipt retries and 160 portfolio deletions**. There are
zero waiting rejections: rebalance has already settled the mixed debt before
resolution. Peak measured CU is **254,319**, below the enforced 600,000 bound.
This measures the new suffix, resolution and its composed rollback transactions,
not every inherited setup instruction or maximum shape.

Development corrections were a Rust borrow conflict and an overly broad replay
assumption: `CloseResolved` on an already-terminal receipt returns
`EngineNonProgress`, while the dedicated claim handler is the no-op replay.
The final test explicitly requires the peer receipt to be present and finalized
before retrying. No economic expectation was weakened or derived from output.

## Exact validation commands

Clone setup (all working-tree writes are in the isolated clone):

```bash
git clone --no-hardlinks --single-branch --branch codex/astra-invariant-cycle-20260915 /tmp/percolator-astra-invariant-cycle-20260915-run /dev/shm/astra-rows419-435-mixed-adl-20260916
cd /dev/shm/astra-rows419-435-mixed-adl-20260916
git remote set-url origin git@github.com:aeyakovenko/percolator-prog.git
git fetch origin codex/astra-invariant-cycle-20260915
git merge --ff-only origin/codex/astra-invariant-cycle-20260915
```

These exports express the exact environment supplied using `env` on each test
invocation. The target directory was copied from the existing Lane 25 host
cache into a separate `/dev/shm` target; the SBF was not rebuilt.

```bash
export CARGO_TARGET_DIR=/dev/shm/astra-rows419-435-mixed-adl-20260916-target
export TMPDIR=/dev/shm/astra-rows419-435-mixed-adl-20260916-tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::adl_rebalance::v16_program_mixed_debt_adl_rebalance_preserves_residual_through_preemption -- --exact --nocapture

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::adl_rebalance::v16_program_mixed_debt_adl_rebalance_preserves_residual_through_preemption \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::underfunded_receipt::v16_program_mixed_adl_underfunded_receipt_preserves_senior_exit_and_expiry_progress \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::close_preemption::v16_program_expired_close_preserves_mixed_debt_and_fractional_source_attribution \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::unsettled_expiry::v16_program_source_expiry_before_mixed_role_settlement_preserves_debt_and_weight \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::v16_program_mixed_role_fractional_source_preserves_attribution_through_expiry_and_retirement \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::v16_program_mixed_role_native_retirement_separates_owner_debt_residue_and_surplus \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::funding_resolution::v16_program_mixed_roles_preserve_funding_attribution_through_resolution \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::funding_resolution::funding_insurance::v16_program_mixed_funding_debt_charges_only_its_insurance_domain \
  inv_051_canonical_adl_effective_quantity::v16_program_unilateral_adl_effective_exit_matrix_preserves_bounded_cleanup \
  inv_037_exact_residual_partition::v16_program_insurance_covered_liquidation_close_ledger_partitions_exactly \
  > /dev/shm/astra-rows419-435-mixed-adl-20260916-logs/final-targeted.log 2>&1

cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant \
  inv_079_public_reachability_evidence::v16_dated_open_security_finding_benchmark_is_non_overclaiming \
  inv_079_public_reachability_evidence::v16_program_invariant_harnesses_are_test_free_roots \
  inv_079_public_reachability_evidence::v16_special_verification_method_registry_matches_charter \
  > /dev/shm/astra-rows419-435-mixed-adl-20260916-logs/metadata.log 2>&1

rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_039_mixed_role_adl_rebalance.rs tests/invariants/cu/inv_039_mixed_role_fractional_retirement.rs
git diff --check
git diff --exit-code d34b98a8f64b10e03c4f48264ca9eb5a45589fc7 -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/**/*.tsv'
git diff --exit-code d34b98a8f64b10e03c4f48264ca9eb5a45589fc7 -- . ':!tests/invariants'
git show --format= --check HEAD
sha256sum /dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so
```

## Validation results

- Standalone product: **1/1 passed**, 32 worlds, 21.93 seconds. The final
  strengthened reset/supply assertions were then validated in the combined run.
- Final product plus nine adjacent controls: **10/10 passed**, zero failures,
  152.26 seconds. Both tests in the parent module were included.
- INV-079 metadata guards: **7/7 passed**, zero failures, 0.59 seconds.
- Scoped rustfmt check, `git diff --check`, protected-path diff and
  outside-scope diff: **exit 0**, no output. The protected glob includes root
  and nested TSVs. Post-commit `git show --format= --check HEAD` is also part
  of the final local-commit checks.
- Only pre-existing host dead-code warnings and the `solana-client` future
  compatibility warning appeared; no SBF or fixture build was performed.

| Exact selector family | Worlds | Reported peak CU |
| --- | --- | --- |
| New mixed debt ADL/rebalance | 32 | 254,319 |
| Existing underfunded receipt | 24 | 348,183 |
| Lane 21 close preemption | 32 | 207,555 |
| Lane 25 unsettled source expiry | 48 | 205,111 |
| Parent classic fractional retirement | 32 | 190,974 |
| Parent native retirement | 24 | 203,974 |
| Mixed funding order | 4 | 209,045 |
| Mixed funding/insurance | 12 | 158,949 |
| INV-051 unilateral ADL cleanup | 2 | Individual calls bounded; no printed campaign peak |
| INV-037 insured residual partition | 1 | Individual close bounded; no printed campaign peak |

Full logs: `final-targeted.log` and `metadata.log` in
`/dev/shm/astra-rows419-435-mixed-adl-20260916-logs`.

## Limits and changed paths

This is a finite, three-asset, five-owner, classic SPL product with zero fees
and funding, fresh source backing, a fixed half-ADL ratio and two exact debts.
It does not cover arbitrary histories, cross-zero risk reopening, nonintegral
ADL ratios, simultaneous source expiry, underfunded receipts, insurance,
authority changes, other quote rails, maximum shape or terminal slab retirement.
The final 4,000/20,000 custody residue is measured and conserved, not redeemed
or burned by this product. The adjacent tests retain those other boundaries.

- `tests/invariants/cu/inv_039_mixed_role_adl_rebalance.rs`: one new test product
  and its local oracle/instruction helpers.
- `tests/invariants/cu/inv_039_mixed_role_fractional_retirement.rs`: three-line
  child module mount only; existing controls unchanged.
- `tests/invariants/README.md`: bounded coverage note retaining OPEN statuses.
- `tests/invariants/row419_435_adl_rebalance_20260916.md`: this focused report.
