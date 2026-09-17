# Row 422 mixed selected provenance, 2026-09-17

Base: latest fetched `origin/main`, `4ab515f3ef0599007c7052feec592f73fd4b8202`.
Worktree: `/dev/shm/percolator-row422-health-20260917`.
Branch: `row422-health-20260917`; local commit only.

## Increment

Finding-blind review used `scripts/loop.md`, INV-020/045/061/062, their public
witnesses, and targeted README/TSV coverage obligations. No withheld benchmark
implementation or external finding patch was inspected.

Existing selected-asset coverage has only one exposed target leg and an unexposed
EWMA sibling. Paid-origin handoffs and active Auth/Hybrid keeper compositions
separate the recipient's exposure from the target. INV-061's multi-leg sizing
uses authenticated prices throughout; INV-062's identity matrix does not join
paid Hybrid provenance to mixed-target selection.

The new child `cu/inv_045_mixed_selected_provenance.rs` gives the target two
100-lot legs: paid-origin Hybrid and AuthMark. Opening order chooses the first
persisted leg. Thirty-two worlds independently cross that order, observation
order, single/batch CPI/no-CPI discovery, and liquidation one/four slots later.
The wash pair and flat keeper share one signer, with distinct portfolios; target
and peer have independent owners. System/SPL/wrapper instructions create and
fund all economic state. Only Clock, signer funding, program loading and external
Pyth report accounts are fixtures. Mint authority is revoked before discovery.

Both effective prices follow an input-derived 24-bps-per-slot path from 1,000,000
to 992,320. Equal-price report renewals cannot prematurely authenticate Hybrid
provenance. Prior-slot evidence rejects target health work; current evidence
allows bounded refresh and a health-restoring partial liquidation. The fee
oracle uses executed quantity and independently predicted effective price, with
two ceiling operations. It is not an independent liquidation-quantity model.

| Boundary | Selected leg | Effective price | Penalty | Keeper reward |
| --- | --- | ---: | ---: | ---: |
| Lag, slot 6 | Hybrid | 997,600 | 11,973 | 0 |
| Lag, slot 6 | AuthMark | 997,600 | 11,973 | 3,990 |
| Catchup, slot 9 | Either | 992,320 | 12,720 | 4,239 |

Only an eligible selected asset receives the retained floor/ceil domain split;
the nonselected asset cannot grant or suppress reward eligibility. Every owner
gets exact settled PnL/fee attribution, both OI lanes balance, and the target's
current certificate agrees with the independent health oracle. The keeper
withdraws exactly principal plus reward through SPL; coalition reward stays
below its paid discovery cost and total custody reconciles. All eight route/hint
worlds at each selection/time boundary have identical normalized economics.

There are 96 exact rollbacks: 32 prior-slot health rejections and 64 valid
refresh/liquidation prefixes followed by conflicting same-timestamp reports.
The valid prefix is also simulated independently. Full tracked and transaction
accounts roll back, accounting for the exact separate network payer fee.

## Validation

The exact selector below passes: **1 passed, 0 failed**, 32 worlds, 96 rollbacks.
Peak measured transaction CU: **438,515**, including failed suffix transactions.
The local bounds are 500,000 per instruction and 650,000 measured transaction CU.
Setup/funding and matcher configuration are not included in the reported peak.
The initial probe stopped at the reused one-leg helper's 325,000-CU assertion
(354,614 CU); only this new two-leg child's bound was adjusted. No shared helper
or production behavior changed; no public LoF/DoS bug was reproduced.

Wrapper and auth matcher were rebuilt locked/offline with tools v1.52 in private
`/dev/shm/percolator-row422-health-20260917-{sbf,matcher}-target` directories.
Host dependencies were copied to the private `-host-target`; tests compiled here.
Engine: `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
Wrapper SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
Logs: `/dev/shm/percolator-row422-health-20260917-logs/`.

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/percolator-row422-health-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row422-health-20260917-sbf-target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::retained_penalty_handoff::mixed_selected_provenance::v16_program_mixed_exposed_legs_bind_rewards_to_selected_price_lineage -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_045_mixed_selected_provenance.rs tests/invariants/cu/inv_045_retained_penalty_handoff.rs
git diff --check
git diff --exit-code 4ab515f3 -- src Cargo.toml Cargo.lock tests/invariants/*.tsv
git diff --cached --check
```

Only that behavioral selector ran. Targeted formatting and diff checks pass.
Existing Solana future-compatibility warning remains. No broad suite or Kani ran.
Row 422 remains **OPEN**; invariant verdicts are unchanged. Arbitrary histories,
nonzero funding, maintenance/policy succession, active keeper exposure combined
with mixed target selection, opposite price directions, larger shapes, other
providers, and full trader/peer terminal redemption remain outside this increment.
Rows 421, 412 and 413/434 files, production, Cargo and status TSVs are untouched.
