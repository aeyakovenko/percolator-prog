# Row 411 funding accrual between retained reductions, 2026-09-17

Base: freshly fetched `origin/main`, `f5e2640453a9c388f855ff827170c49925774613`.
Worktree: `/dev/shm/row411-inter-reduction-funding-20260917`.
Branch: `codex/row411-inter-reduction-funding-20260917`; local commit only.
Primary INV-014; row 411 remains OPEN. No production correction or TSV change.

## Duplicate analysis and increment

Read the row411 funding-collection, split-funding, partial-authority and regression
notes, their mounted tests, generated partial-policy and mixed-route fee histories.
The full-close funding selector collects one interval. The split-close selector
also collects one interval, with both reductions at slot 2; its remaining-gaps
section explicitly names funding accrual between reductions. The generated-policy,
partial-authority and mixed-route histories do not accrue funding. Repeating those
histories alone would add no evidence.

One new selector reuses the funding runner and input-derived ledger. Eight worlds
cross both position signs with single/batch CPI/no-CPI residual closes:

- The existing partial opening and first reduction leave 38 of 95 units at slot 2,
  with 95 funding atoms settled and trade fees of 36 + 22 atoms per owner.
- Public EWMA reports and an observed slot-3 crank checkpoint the zero-rate
  interval. The slot-4 report leaves another negative-rate interval pending against
  the residual. Price remains 100. No program-owned bytes are injected or mutated.
- The residual was signed before authority/policy changes and either reduction.
  At 71 bps it rejects; the batch-CPI leg/LP grant permit 137 bps, so its retained
  15-atom aggregate cap is the binding limit. Funding cannot enlarge gross consent.
- Restoring 37 bps lets the close execute before an obsolete-authority policy
  suffix fails. Complete tracked/compiled Accounts roll back, including the second
  funding settlement, maintenance, matcher effects, epochs and request sequence.
  Only the separate payer's exact signature fee is excluded.
- A separately pre-signed close commits: funding totals 95 + 38 = 133, never 190
  or 95; per-owner trade fees total 73. Consumed retry rejects. Funding indices
  and epoch advance twice; retained wire images and signatures stay unchanged.
- Slot-5 public synchronization, recertification, conversion and full withdrawals
  pay short taker `[98375,198532]` or long taker `[98641,198266]`. Custody retains
  3,216 insurance atoms in domains `[1605,1611]`. Capital, PnL, maintenance cursors,
  OI, fixed mint supply, complete token Accounts and stock/encumbrance censuses
  match the independent ledger at every checkpoint. All four endpoints agree.

## Exact private-SBF verification

All three selectors PASS; no development failure or broad suite run.
Define `P` as the exact module prefix below; each table selector is `${P}::suffix`.

```bash
R=/dev/shm/row411-inter-reduction-funding-20260917
export CARGO_TARGET_DIR="$R-host-target"
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
export PERCOLATOR_FUZZ_SBF="$R-sbf-target/deploy/percolator_prog.so"
P=inv_014_delayed_policy_and_policy_epoch_safety::retained_partial_fee_routes::generated_partial_policy_words::retained_partial_authority_routes::retained_funding_collection
# Run from R, substituting each exact suffix below:
# cargo test --locked --offline --test v16_cu "$P::$suffix" -- --exact --nocapture --test-threads=1
```

| Exact suffix | Peak CU | Worlds / rollbacks / fills / payouts |
| --- | ---: | --- |
| `v16_retained_residual_bounds_fees_after_inter_reduction_funding_accrual` | 471,788 | 8 / 40 / 24 / 16 |
| `v16_retained_split_close_bounds_fees_and_collects_funding_once_across_routes` | 464,054 | 8 / 40 / 24 / 16 |
| `v16_retained_partial_close_separates_funding_and_maintenance_from_fee_consent` | 455,883 | 8 / 24 / 16 / 16 |

Each selector also checks eight successful nonmutating simulations. All measured
transactions remain under 500,000 CU; this is not a maximum-shape claim.

Wrapper and hostile matcher rebuilt locked/offline from this worktree using
platform-tools v1.52, engine `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
Private host/SBF targets were copied without hard links from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target`; matcher build
target is `$R-matcher-target`. Artifact SHA-256:

- Wrapper: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
- Matcher: `e0c20fad34a7822cc6ce42a3c77ff08a8591977102f0c497a339d66a9dd6240a`.

Rustfmt, whitespace and exact three-path scope checks pass. Changed paths:
`tests/invariants/cu/inv_014_retained_funding_collection.rs`,
`tests/invariants/README.md`, and this note. Production, Cargo inputs, tracked
fixtures, TSV ledgers and unrelated invariants are unchanged.

## Remaining gaps

Bounded Live/SPL, one asset, base fees, two negative-rate intervals only. Positive
funding rates, arbitrary interval lengths/partial ratios, multi-asset aggregate
caps, dynamic/backing/redirect fee composition, collection clipping/underfunding,
grant expiry, Recovery/Resolved, alternate quote rails and maximum shapes remain
open. This does not close row 411 or establish general INV-081 validity.
