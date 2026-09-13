# Scope B: Pending Owner Partition Through Resolution

## Scope And Provenance

Base branch: `codex/astra-open-holdout-ledger-20260912` at
`485c8bde3ad62358e150f7135aedcce549f636ca`.
Isolated worktree: `/tmp/percolator-pr135-scope-b-pending-20260913`.
Branch: `codex/pr135-scope-b-pending-attribution-20260913`.
Only the supplied base's existing tests, local wrapper and pinned engine source
were inspected. No open PR branches, diffs or tests were consulted. The protected
`/home/anatoly/percolator-prog` checkout was not edited.

Row 419 is a coverage label, not a reproduction specification. The retained test
is [the owner-partition probe](cu/inv_039_pending_loss_owner_partition.rs), mounted
through the existing INV-039 CU owner. Production code and shared fixtures are
unchanged. Neither README nor machine classification tables are changed.

## Nonduplication Audit

| Existing coverage | Why another copy was rejected / what is different here |
| --- | --- |
| `resolved_histories` | Already has 144 histories spanning all 24 cohort close orders and three resolution boundaries, plus an input-derived generated history oracle. Another two-domain close permutation is redundant. Its actor-to-owner mapping is one-to-one. |
| `two_domain_resolution`, `insured_resolution` | Already preserve exact bankruptcy residual/B/insurance debt through resolution and claimant orders. Repeating their fixed domain products adds no coverage. |
| `shared_holder` | One portfolio owns two pending domains and is partially detached. The new probe instead partitions one owner's membership in one domain across multiple portfolios with the same destination ATA, alongside a competing owner. |
| `cohort_reduction` | Two distinct owners retain equal weights through live reductions, transfers and recreation. The new comparison changes portfolio cardinality for one unchanged economic owner and carries every part through resolved payout and deletion. |
| `close_preemption`, `cure_resolution`, funded/backing/restart children | These already own expiry, cancellation, funding, backing expiry and peer-generation lifecycle cells. No duplicate of those histories was retained. |

The wrapper's signed `handle_resolve_market` and stale-permissionless handler
both call `resolve_market_view`, with different authorization/staleness and
pending-mark policies. Merely swapping their tags on a frozen solvent fixture
would not establish a new pending-debt accounting mechanism. The retained test
uses public `ResolveMarket`, not a new claim about stale resolution provenance.

## Public Construction And Oracle

The baseline deposits are `[800000, 720000, 800000, 720000, 777]`. One owner holds
four lots against debtor 1; a distinct owner holds four lots against debtor 3,
in the same asset and source side. The first owner's portfolio is compared with
partitions `[1, 3]` and `[1, 1, 2]`. Each lot carries 200,000 principal atoms.
Splitting uses signed public withdrawal into the existing owner ATA, System
account creation, `InitPortfolio`, and public redeposit. The new portfolios use
that same owner and ATA. No economic Accounts or oracle bytes are injected.

Authenticated mark moves of 8 or 16,000 atoms over 20 slots, in either side
orientation, are settled on holders. Public asset shutdown and Recovery forfeit
leave each holder with zero basis and its original nonzero loss weight. One
debtor optionally settles before resolution. Resolution preserves every
non-market Account. The delayed keeper continuation varies debtor/holder order,
detaches holders, pays/deletes debtors, and closes/deletes claimant portfolios.
Each successful deletion preserves all foreign Accounts, including the shared
ATA and the other portfolios belonging to that owner.

The independent book uses only input deposits, signed quantities and price
movement to calculate each portfolio's entitlement. At each checked pending or
terminal prefix it requires:

```text
capital + signed PnL + unpaid receipt face + prior SPL payout
    + still-unsettled input debt = input deposit + input signed price gain
```

SPL balance deltas from a particular public payout are attributed to that
portfolio, then summed by owner and compared with the single actual owner ATA.
Counting a shared ATA twice, losing one part's debt, or paying one part from
another part's entitlement cannot pass merely because aggregate supply balances.
The oracle does not infer expected entitlement from production PnL, receipt face
or realized payout fields.

Basis, original weight, stored-leg counts and pending counts are independently
predicted per side. Opening OI is eight lots on each side. Raw market stock and
reservation censuses reconcile all still-materialized portfolios, booked/SPL
custody, and fixed mint supply. The close residual partition is checked, but is
explicitly required to stay empty in these solvent worlds. Untouched sibling
assets retain their full initial asset state at the endpoint.

For movement `m`, every partition, side and order yields the same owner vector:

```text
[800000 + 4*m, 720000 - 4*m, 800000 + 4*m, 720000 - 4*m, 777]
```

Every portfolio is deleted. Vault, insurance, capital, positive PnL and
materialized count all end at zero. Present, fully paid receipts also accept
idempotent `ClaimResolvedPayoutTopup` retries with an unchanged tracked frame.
These are retries, not new positive-value top-up coverage.

## Discarded Assumptions And Limits

An initial selector run failed because the driver required every close to make
progress. The wrapper selects `EngineNonProgress` for a waiting claimant; the
final driver accepts only that specific rejection, requires exact tracked
economic rollback, and leaves its book unchanged. Its bounded completion suffix
must still pay and delete every portfolio. The separate transaction fee payer
is excluded from the economic frame; no new exact fee-payer rollback claim is
made.

A second run completed all 48 entitlement and terminal-stock comparisons but
failed the proposed nonvacuity condition: no paid portfolio could be deleted
while a co-owner's loss weight remained pending in these histories. That
early-deletion cell was discarded, not relabeled as covered. The retained
nonvacuous boundary is deletion after detachment while another portfolio with
the same owner/ATA still has a strictly positive unpaid entitlement.

This adds sampled INV-039/048/066 evidence for the **pending cohort -> resolved
detachment -> separate shared-owner payout/deletion** composition. It does not
add nonzero INV-037 residual partition or INV-076 drift/finalization evidence.
There is no bankruptcy, B allocation, insurance consumption, fractional rounding,
nonzero funding/fees, positive-value claim-top-up comparison, or arbitrary-history
proof. Source fairness is fully funded, not a competing haircut oracle.

Classifications remain unchanged: INV-039 is `REFUTED_CURRENT`; INV-037/048/066/076
remain `OPEN_EVIDENCE`; row 419 remains open/missing. The next valuable extension
is a rounding-aware independent owner ledger for unequal fractional cohort
partitions with a nonzero residual through resolution, after checking overlap
with the existing insured and fractional-receipt owners.

## Validation

Default-feature SBF was freshly rebuilt from this worktree, using the previous
Scope B private build cache because `/tmp` and `/dev/shm` are nearly full. Engine
pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`. SBF SHA-256:
`8a010d8283186721866a95a71d252a3265015fbf563d6ce566cd44f263659349`.

Final exact CU run: **4/4 passed in 49.79s**. The new selector covers 48 worlds,
48 deletions with co-owner value still unpaid, and 72 receipt retries; peak
observed setup/continuation is **190,813 CU**. Nearest controls are the two-domain
bankruptcy selector, generated resolved-history selector, and shared-holder
selector. The two exploratory revisions above each failed only their stated
driver/nonvacuity assertion; they are not evidence of a lost-value failure.

Charter/index and authoritative/nonoverclaiming status guards: **2/2 passed**.
Formatting and working-tree whitespace checks pass. The metadata target's
existing unused-support warnings and Solana future-compatibility warning remain.

Exact commands, run from the isolated worktree:

```sh
export CARGO_TARGET_DIR=/dev/shm/pr135-scope-b-catchup-20260913-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export TMPDIR=/dev/shm CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
sha256sum "$PERCOLATOR_FUZZ_SBF"
# This exact single selector was used for both exploratory revisions above.
cargo test --locked --offline --test v16_cu inv_039_pending_loss_obligation_durability::owner_partition::v16_program_pending_owner_partition_preserves_shared_ata_entitlements_through_deletion -- --exact --nocapture --test-threads=1
# Final retained test and the three nearest controls, with no broad-suite run.
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_039_pending_loss_obligation_durability::owner_partition::v16_program_pending_owner_partition_preserves_shared_ata_entitlements_through_deletion \
  inv_039_pending_loss_obligation_durability::shared_holder::v16_program_shared_holder_pending_domains_survive_partial_detach_and_debtor_close_orders \
  inv_039_pending_loss_obligation_durability::resolved_histories::v16_program_pending_resolved_history_generator_preserves_owner_debt \
  inv_039_pending_loss_obligation_durability::close_reopen::two_domain_resolution::v16_program_two_bankrupt_domains_preserve_pending_debt_across_resolution_order
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --quiet --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
```
