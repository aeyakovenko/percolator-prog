# Scope B: sizing after multi-asset current-source catchup

Base: `codex/astra-open-holdout-ledger-20260912`,
`e99d666fc167ed10b7a8be52ea076285319b9a6b`.
Branch: `codex/pr135-scope-b-catchup-sizing-20260913`.
Worktree: `/tmp/percolator-pr135-scope-b-catchup-20260913`, created from
`/tmp/percolator-astra-watch.Cb2E7d` before editing.
Engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
No open PR was fetched or used as a source. The protected checkout
`/home/anatoly/percolator-prog` was not accessed.

## Net-new relation

The new [public probe](cu/inv_061_caught_up_portfolio_sizing.rs) joins **INV-061
with INV-045/053**. It derives a liquidation quantity and every beneficiary's value
from opening quantities, collateral and published prices, then executes the public
crank only after both exposed assets finish price catchup. The nonselected active
leg affects the minimum quantity; a third asset's funded reserves remain local.

The overlap review found three distinct existing owners:

| Existing owner | Existing evidence | Increment retained here |
| --- | --- | --- |
| `cu/inv_061_enumerated_public_sizing.rs` | One active target leg, fixed effective price, adverse target lag, split/aggregate opening and an input-derived linear quantity oracle. | Two active legs with unequal prices after actual capped catchup and nonzero settled losses; selection follows stored leg order; single versus multi-leg batch opening. Public account/mint helpers are reused unchanged. |
| `cu/inv_045_accepted_price_reward.rs`, `cu/inv_045_reward_catchup_order.rs`, `cu/inv_045_authenticated_reward_handoff.rs`, and the caught-up Hybrid test in `cu/inv_045_no_free_mark_movement.rs` | Price provenance, fee/reward attribution and catchup histories, usually deriving the fee from the observed close quantity. | Quantity is fixed by the input oracle before liquidation, with the untouched active leg included in full maintenance. No additional Hybrid provenance claim. |
| `stateful/inv_053_full_health_recertification_equivalence.rs` and `stateful/inv_061_deterministic_bounded_liquidation.rs` | Full-refresh equivalence across trade deltas and source/ADL states; multi-asset ADL selection and follow-up fee episodes use a separate binary-search model. | Linear sizing over the whole caught-up portfolio, with actual keeper payout in the same conformance history. Existing larger/ADL coverage is complementary. |

No duplicate fixed probe was retained or deleted. The prior test only gains a
child-module declaration; no shared helper or production behavior changes.

## Oracle and public histories

Forty-eight worlds cross three quantity pairs, both stored leg orders, separate
`TradeNoCpi` versus one two-leg `BatchTradeNoCpi`, joined versus partitioned market
observations, and both final observation orders. Each target shorts assets 1/2
against one funded peer. Asset 0 holds 31 backing atoms and 42 insurance atoms;
a separately owned idle portfolio holds 53 atoms. The keeper initially owns seven.

Public AuthMark instructions publish targets of 1,250,000 and 1,375,000 from an
entry price of 1,000,000. Three authenticated slots advance the 1,250-bps-per-slot
price cap. Every intermediate effective price is checked against the input
schedule. Market observation grouping cannot mutate the target or peer Accounts
or custody. Each owner settles once after both markets finish, so settlement
rounding is independent of observation grouping. Final profiles must report the
current observation slot, and raw target/effective prices must equal the inputs.

The oracle computes per-leg initial collateral, integral price loss, post-loss
equity, ceil notional, ceil maintenance, fee and keeper/insurance shares. It scans
`1..=q` and selects the first affordable health-restoring quantity, including the
entire other leg's maintenance. It neither calls the engine selector nor takes
an executed quantity, fee, certificate lane or capital delta as an expected-value
input. The selected residual is above the minimum-margin floor.

| Opening quantities | Selected asset | Post-loss equity | Close quantity | Fee | Keeper reward | Post-close MM |
| --- | --- | --- | --- | --- | --- | --- |
| 64 / 80 | 1 | 84 | 44 | 3 | 1 | 81 |
| 64 / 80 | 2 | 84 | 40 | 3 | 1 | 81 |
| 96 / 64 | 1 | 97 | 42 | 3 | 1 | 94 |
| 96 / 64 | 2 | 97 | 38 | 3 | 1 | 94 |
| 128 / 96 | 1 | 135 | 60 | 4 | 2 | 131 |
| 128 / 96 | 2 | 135 | 54 | 4 | 2 | 131 |

All 48 worlds distinguish the correct quantity from entry-price sizing and from
sizing that drops the second leg. Thirty-two also distinguish the current-price
fee from pricing that same close at entry. These checks establish that the price
and multi-leg assertions are not vacuous.

The public close must match the independently chosen quantity. Only that asset's
two OI lanes decrease; the other active target leg, unrelated assets, peer Account,
idle owner, source records and complete custody Accounts remain framed. Asset-0
backing, source credit, insurance budgets and spent records are anchored before
catchup and checked through observation, settlement and liquidation. Backing,
source credit and budget records also remain framed through keeper payout.
The target pays exactly the derived fee, the peer retains its derived claim, and
the keeper receives its own seven atoms plus its reward through a real SPL
withdrawal. The retained fee is split between both sides of the selected asset.

Current certificates are checked against independent raw-state arithmetic and a
detached engine full refresh, including the target immediately before and after
liquidation. Stock and encumbrance censuses run after liquidation and keeper payout.
Both transport/catchup variants agree on complete target certificates and economic
endpoints within each leg order. Different stored leg orders legitimately select
different assets and quantities, so they are checked against their own inputs.

All economic state comes from System/SPL/ATA/wrapper instructions. Mint authority
is revoked before opening. Only program loading, signer SOL and Clock advancement
are harness setup. No token, market, portfolio or oracle-report Account is injected;
detached refresh copies are never installed in LiteSVM.

## Classification boundary

Labels **422/426 remain OPEN**. `invariant_status.tsv`, `open_findings.tsv`, the
reopening table and README remain unchanged. The new relation is bounded support
for INV-061/045/053 and attribution/locality joins INV-024/025/026/031/036/038.
It does not change `REFUTED_CURRENT` INV-045 or `OPEN_EVIDENCE` INV-053/061.

This is current AuthMark evidence, not Hybrid/Pyth freshness, paid-print provenance
or missing-observation rejection. Both positions enter with unit ADL; funding,
maintenance debt and preexisting source liens are zero. The close is partial and
solvent, the keeper is flat, and fee policy is fixed. Existing nonunit ADL,
minimum-fee/full-close, terminal, CPI and maximum-shape tests remain necessary.

## Exact validation

The previous task's private build cache was copied without hard links into a new
private target. Default-feature wrapper SBF was rebuilt from this worktree using
locked/offline platform-tools v1.52. SBF SHA-256:
`8a010d8283186721866a95a71d252a3265015fbf563d6ce566cd44f263659349`.

Commands from the isolated worktree:

```sh
export CARGO_TARGET_DIR=/dev/shm/pr135-scope-b-catchup-20260913-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export TMPDIR=/dev/shm CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
cp -a --reflink=auto /dev/shm/pr135-scope-b-20260913-target "$CARGO_TARGET_DIR"
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
sha256sum "$PERCOLATOR_FUZZ_SBF"
cargo test --locked --offline --test v16_cu inv_061_deterministic_bounded_liquidation::enumerated_public_sizing::caught_up_portfolio_sizing::v16_program_caught_up_multi_asset_sizing_preserves_health_and_beneficiary_value -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_061_deterministic_bounded_liquidation::enumerated_public_sizing::v16_program_small_public_liquidations_match_exhaustive_quantity_oracle -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --quiet --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming -- --exact --quiet --test-threads=1
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
```

The initial development run failed on a fixture timestamp assumption:
`mark_ewma_last_slot` tracks the last price change and remains 1 for repeated
same-price publications. The wrapper updates `last_good_oracle_slot` for each
accepted publication, so current-source evidence checks that field against slot 3
and separately checks the price-change slot. This correction follows the public
managed-mark handler; no production change or conformance violation is claimed.

Results: default-feature SBF build PASS; new selector **1/1**, **48 worlds**,
**32 fee-price discriminators**, **23.14s**, peak measured crank **334,897 CU**
under the 450,000-CU two-asset bound. The existing enumerated control passed
**1/1**, **72 worlds**, **31.14s**, peak liquidation **258,603 CU**. Both INV-079
metadata gates passed **1/1** (0.01s / 0.00s). Formatting and all Git whitespace
checks passed. No broad suite, open-PR replay or alternate-pin comparison ran.
Existing unused-support and `solana-client v1.18.26` compatibility warnings remain.

The new selector was rerun after extending asset-0 reserve frames back to the
pre-catchup snapshot. Its earlier passing result was 48 worlds in 23.27s with the
same peak; the final result above includes those stronger locality assertions.
