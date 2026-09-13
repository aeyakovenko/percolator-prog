# Latent Source Capacity Across Peer Reset

Base: `af97beeac08872b28103feb20b83cf615557f00a`, from the requested
`origin/codex/astra-open-holdout-ledger-20260912`. Worktree:
`/home/anatoly/percolator-row423-exit-resources-20260912`.
Only the supplied base and pinned dependency sources were inspected; no holdout
PR/issue branch or diff was accessed. Production and shared support are unchanged.

## Distinct Obligation

| Existing evidence | New composition |
| --- | --- |
| Historical/latent capacity, including resolution | A future domain remains absent through peer nonunit ADL and prior-epoch reset |
| Retained-domain episodes with bilateral DrainOnly exit | The peer reduces using only its own signature while the winner's Account stays unchanged |
| Maximum-source owner reduction | Source storage must still grow during later cleanup, and both funded owners complete exact payout |

The new selector uses the existing System/SPL/ATA/matcher/wrapper `History`
construction. No initialized program-owned bytes are changed directly. The finite
product crosses all four trade admission transports, both initial position signs,
and Active/DrainOnly: **16 worlds**, with one active asset after the history prefix.

Thirteen two-sided historical episodes retain 26 claims totaling 50 atoms. A
six-unit new position reserves the remaining pair, earns six atoms on one side,
and reverses. The winner then skips refresh while the loser settles the next
six-atom loss, reduces by three units, settles another three-atom loss at half ADL,
and removes the remaining exposure. The winner still has exactly 27 source records
and a prior-epoch leg. One permissionless cleanup (four-call bound) creates the
last record with nine atoms and removes that leg. Every earlier claim is preserved.

The independent suffix oracle checks input-derived per-domain claim/backing
prefixes, the union of occupied and future active-leg domains, zero liens and
insurance reservations, exact owner capital/PnL, effective OI, SPL custody and mint
supply. Cranks strictly decrease authenticated accrual/economic debt or the
lexicographic retained-leg/economic-debt rank. Both owner-only reductions preserve
the winner's complete Account bytes. Conversion, both SPL withdrawals and portfolio
deletion pay exactly **1,000,065 / 999,935 atoms**. Side finalization follows these
payouts and preserves zero remaining stock and zero materialized portfolios.

## Limits

Row **423 remains OPEN**. This is passing bounded conformance, with no production
fix or vulnerable-pin red/green evidence. INV-028/031/057/073/077/078/082 gain this
specific public continuation; no new activation/reactivation result is claimed for
INV-089. The maximum source count is exercised, not maximum simultaneous legs or
market bytes. The batch transports carry one leg. Ordering of source history and
owner payout is fixed. Side finalization before payout, arbitrary ADL fractions,
expiry/liens, fees/funding, oracle outage, insolvency and general recovery remain
outside the witness. The price and optional DrainOnly prefix require the configured
oracle/market authority; subsequent reduction and cleanup require no such signature.

The discarded partial-live-reclamation candidate assumed a caller-selected partial
conversion and conversion while exposed to the claim source; that is outside the
conversion contract. Its file and fixture changes were removed. The retained test
does not alter any production guard or relax its funded-exit requirement.
The removed development selector was
`historical_latent_capacity::live_reclamation::v16_program_live_claim_reclamation_preserves_future_domains_and_owner_exit`
under the same INV-028 owner; it did not produce passing coverage.

## Validation

The new exact selector passes **1/1**, with **16 worlds / 2,176 counted history
calls** and a maximum **one** prior-epoch cleanup call. Maximum CU for trade / crank /
conversion / withdrawal / close / owner reduction / lifecycle is
**861,901 / 499,169 / 712,286 / 53,954 / 26,540 / 110,365 / 7,617**.
Construction and mark writers are outside those maxima; mark calls are separately
bounded by 1,375,000 CU. Custody, deletion and side finalization have a 300,000 ceiling.

Both exact nearby controls pass **2/2**: the existing 32-world historical/latent
matrix and the 14-leg/28-source owner-reduction continuation. Formatting, working
and staged whitespace, and the ledger's eight-column TSV checks pass. No broad
suite was run. Cargo check is unnecessary for the final test/documentation-only
diff; production and shared support are unchanged. The existing `solana-client
v1.18.26` future-compatibility warning remains.

Default-feature wrapper and matcher SBF builds used locked offline platform-tools
v1.52 in private targets. The engine remains `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
SHA-256:

```text
wrapper  d5f2d3d2c35842aab0979ab24fed415fe36b2b93ed6cc80998fee839ae76343f
matcher  50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93
```

Exact selectors and checks (from this worktree):

```sh
export TMPDIR=/dev/shm CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/percolator-row423-exit-resources-20260912-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
cargo test --locked --offline --test v16_cu inv_028_source_domain_realizability_cap::historical_latent_capacity::latent_reset_exit::v16_program_latent_source_survives_owner_reduction_and_prior_epoch_exit -- --exact --nocapture
cargo test --locked --offline --test v16_cu inv_028_source_domain_realizability_cap::historical_latent_capacity::v16_program_historical_and_latent_domains_share_bounded_settlement_capacity -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
git diff --cached --check
```

Adjacent CU note: `inv_077_bounded_work_and_maximum_shape_compute::v16_attack_max_source_owner_rebalance_reduce_stays_bounded`
was attempted as an extra control and currently fails independently at the CU ceiling
(`ProgramFailedToComplete`, 1,399,676 / 1,399,700 CU). It is not claimed as validation
for this increment; row 423 remains OPEN.
