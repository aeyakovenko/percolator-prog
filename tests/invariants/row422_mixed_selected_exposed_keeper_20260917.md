# Row 422 mixed selection with an exposed keeper

Base: `origin/main`, `9d5af93f036d6bd9b09c671e018754894598d8fa`.
Worktree: `/dev/shm/row422-liquidation-provenance-20260917`.
Branch: `codex/row422-liquidation-provenance-20260917`.

## Duplicate analysis and increment

The [mixed selected-provenance note](row422_mixed_selected_provenance_20260917.md)
explicitly leaves active keeper exposure combined with mixed target selection
open. Its 32 worlds use a flat keeper. The existing `exposed_keeper_provenance`,
`hybrid_recipient_provenance`, and `paid_origin_hybrid_recipient` families expose
the recipient on a separate asset from the target. Repeating their discovery
transport or report-order matrices would add no new product. Existing maintenance
and terminal-redemption families also do not compose mixed target selection with
an exposed beneficiary; those dimensions are not added here.

One new child gives an independently owned keeper a one-lot long or short on the
same paid-origin Hybrid market as one of the target's two 100-lot legs. The other
target leg uses AuthMark. Sixteen worlds cross selected leg, one/four elapsed
slots, keeper direction, and keeper K settlement before/after liquidation. The
last keeper settlement delta is nonzero in every world. At lag the keeper remains
exposed to trade-driven Hybrid provenance even when the selected AuthMark leg is
reward eligible. At catchup the shared Hybrid market is authenticated.

| Selected leg | Boundary | Penalty | Reward |
| --- | --- | ---: | ---: |
| Hybrid | Lag, 997,600 | 11,973 | 0 |
| AuthMark | Lag, 997,600 | 11,973 | 3,990 |
| Either | Catchup, 992,320 | 12,720 | 4,239 |

The price and discovery-cost oracles use input-derived rate/fee arithmetic.
Liquidation quantity remains engine-selected; penalty uses that quantity and the
independently predicted accepted price. Reward eligibility follows the selected
target leg, not the recipient's price lineage or PnL sign. Comparing the entire
recipient account before/after each target crank allows only reward capital and
certificate invalidation to change. Subsequent settlement checks each owner's
value, then compares capital, PnL and legs across the two settlement orders.
Target health, balanced OI, stock/source census, selected domain attribution,
fixed mint supply and exact SPL custody are checked independently.

There are 32 conflicting-report suffix rollbacks, including successful refresh
and liquidation prefixes, plus 16 active-keeper withdrawal rejections. The shared
submit helper simulates each prefix and checks complete tracked/transaction
accounts on rejection, allowing only the separate payer's exact network fee.
All economic construction and changes use System/SPL/wrapper instructions; only
Clock, signer funding, external Pyth reports and program loading are fixtures.
Mint authority is revoked before discovery. No program-owned bytes are injected.

## Exact validation

One exact selector passes: **1 passed, 0 failed; 16 worlds, 48 rollbacks**.
Peak measured transaction CU: **470,411**, including failed suffix transactions.
The shared action bound is 500,000 CU and the new transaction bound is 650,000 CU.
Setup/funding and configuration are outside the peak. No broad suite ran.

The private SBF is copied from
`/dev/shm/percolator-row423-latent-competition-20260917-sbf-target/deploy/percolator_prog.so`.
Its SHA-256 matches the rebuilt current-engine artifact in both existing row422
notes: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
Production source and Cargo files have no difference from their documented
`4ab515f3` base. Engine: `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
Host dependencies were copied to a private target and the test compiled here.
This single no-CPI route needs no matcher fixture or fixture changes.

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/row422-liquidation-provenance-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/row422-liquidation-provenance-20260917-artifacts/percolator_prog.so
cargo test --locked --offline --test v16_cu inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::retained_penalty_handoff::mixed_selected_provenance::exposed_keeper::v16_program_mixed_selected_rewards_commute_with_exposed_keeper_settlement -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_045_mixed_selected_provenance.rs tests/invariants/cu/inv_045_mixed_selected_exposed_keeper.rs
git diff --check
git diff --exit-code 9d5af93f -- src Cargo.toml Cargo.lock tests/fixtures 'tests/invariants/*.tsv'
```

Final run: `../row422-liquidation-provenance-20260917-artifacts/final.log`.
Targeted formatting, whitespace and protected-path checks pass. The existing
Solana future-compatibility warning remains.

## Limits

Row 422 remains OPEN; this is one finite composition increment. It does not prove
arbitrary histories, funding/maintenance/policy succession, provider settlement,
other discovery transports, other observation orders, larger shapes or terminal
payout composition. Live keeper K settlement and reward attribution are covered;
full redemption is not. An exploratory signed close against the post-liquidation
peer rejected with `EngineLockActive`; that terminal continuation is outside the
retained selector. No production issue or closure is claimed. Production, Cargo,
fixtures, status ledgers and unrelated invariant files are untouched.
