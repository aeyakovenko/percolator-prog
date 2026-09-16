# INV-061 nonunit liquidation fee boundaries

Base: `2d2af7cc3d846f9d9fc386ad6d2c603d7be92a1a` (`origin/main`). Engine:
`94979ede7db934545e53a8f210dd063a9ea3ea63`. Isolated branch/worktree:
`astra-ultra/inv045-062-cycle3-20260916222938` at
`/dev/shm/astra-ultra-inv045-062-cycle3-20260916222938`.

## Net-new coverage

The existing INV-061 exhaustive public oracle starts at a unit ADL index and has
no minimum liquidation fee. The queued opposing-ADL regression tests one
surviving large-position partial-close size and one cure, with one fee policy.
INV-086's scaled liquidation model covers a fixed dual-ADL history over four
opening transports and uses binary search. None enumerates the small-quantity
fee-boundary product after ADL, including the subminimum full-close exception,
then requires every owner's and keeper's exact ordinary Live payout.

The new selector uses 17, 31 and 64 raw quantity atoms, public opposing reductions
of 1, floor(raw/3), and floor(raw/2), both signs, four fee profiles, and empty
versus complete reverse-ordered hints. All market, mint, custody, and portfolio
state is constructed through System/SPL/ATA/wrapper instructions. No economic
state bytes are injected or restored. AuthMark target-only lag leaves effective
price fixed at POS_SCALE, making notional equal to quantity and settlement PnL
zero. Capital is ceil(70% of initial raw quantity); maintenance is independently
computed as max(ceil(60% of effective quantity), 3) plus ceil(50% adverse lag).

The oracle linearly enumerates admissible effective close quantities, including
the proportional-margin dust boundary and partial minimum-fee gate. It never
reads a deployed selector result or health certificate to choose a close.
Independent BigUint-backed conversion checks the retained raw basis and both
participants' effective exposure against both maintained OI counters. The queued
target's complete account is unchanged by opposing ADL, and its certificate
epochs still match the market. Public liquidation must recertify its exposure.

## Results

- 144 worlds: **72 partial liquidations, 24 full closes, 48 cured queues**.
- **336 exact rollback rejections**: 144 duplicate-hint calls, 48 cured first
  calls, and 144 post-action retries. The frame includes market, both owners'
  portfolios, keeper portfolio/authority, mint, vault, and three token accounts;
  the separate network-fee payer is excluded.
- All 144 raw-size alternatives predict a different close. Fee, keeper reward,
  both selected-domain insurance credits, passive-account framing, stock and
  reservation censuses, and partial-close health certificates are asserted.
- Complete hints permit **12 unpaid flat-certificate refreshes** after full
  close. Only the target certificate changes; the whole decoded market and the
  other portfolios remain unchanged. The next identical call rejects. With
  empty hints, the first post-full-close call already rejects.
- **120 owner closes, 144 passive cleanups, 288 side finalizations, 432 exact
  SPL withdrawals**. All positions and capital exit in Live; unit ADL indices
  and Normal sides are restored. Only the independently computed insurance fee
  remains in custody, and mint supply is unchanged.
- Peak measured CU: **259,038**. Liquidation/refresh/reject calls enforce the
  existing 325,000 bound; owner reduction, cleanup, reset and payout calls enforce
  the existing 300,000 bound. Setup calls are not included in this peak.
- Final exact selection: **5 CU-harness tests and 2 host metadata tests passed**.

Two temporary test-only negative controls use the new exact selector below:

1. Replace `close_oracle(effective, capital, policy)` with
   `close_oracle(raw, capital, policy)`: **0 passed, 1 failed**, observed OI
   `[10, 10]` versus wrong expected `[9, 9]` in the first world.
2. Restore effective sizing, remove `policy.raw(close) >= policy.minimum` from
   the oracle: **0 passed, 1 failed**, observed full-close OI `[0, 0]` versus
   wrong partial-close expectation `[8, 8]` at the two-atom minimum fee.

Both mutations are restored. These demonstrate oracle sensitivity, not a
production mutation campaign. During fixture development, a five-atom minimum
with a three-atom maintenance floor correctly failed public market validation;
the final two-atom minimum is admissible. The initial expectation that every
post-full-close crank rejects was refined to cover the unpaid flat refresh.
Neither development failure established a public LoF/DoS/CU bug.

## Exact validation

Host and SBF dependency caches were copied into private /dev/shm target dirs.
Default-feature SBF was then rebuilt from this worktree with platform-tools v1.52:
SHA-256 `87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.
No matcher artifact is needed for these selected public no-CPI setup routes.

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-inv045-062-cycle3-20260916222938-target
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
CARGO_TARGET_DIR=/dev/shm/astra-ultra-inv045-062-cycle3-20260916222938-sbf-target cargo build-sbf --tools-version v1.52 --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
new=inv_061_deterministic_bounded_liquidation::enumerated_public_sizing::nonunit_fee_boundary_sizing::v16_program_nonunit_liquidation_fee_boundaries_match_exhaustive_oracle_and_exit
# Each temporary negative control used this same exact execution selector.
cargo test --locked --offline --test v16_cu "$new" -- --exact --nocapture
# Restored final tree, five exact tests, zero ignored.
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  "$new" \
  inv_061_deterministic_bounded_liquidation::enumerated_public_sizing::v16_program_small_public_liquidations_match_exhaustive_quantity_oracle \
  inv_061_deterministic_bounded_liquidation::v16_program_queued_liquidation_recertifies_after_partitioned_opposing_adl \
  inv_059_fee_fragmentation_bound::v16_attack_min_liquidation_fee_falls_back_to_full_close_progress \
  inv_061_deterministic_bounded_liquidation::v16_program_liquidation_composition_is_source_complete
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming
rustfmt --edition 2021 --check --config skip_children=true tests/invariants/cu/inv_061_enumerated_public_sizing.rs tests/invariants/cu/inv_061_nonunit_fee_boundary_sizing.rs
git diff --check
```

## Limits and rejected duplicates

This is finite public-route evidence, not arbitrary ADL histories, partition
closure, maximum-shape CU, changing effective price/funding, Hybrid provenance,
multi-owner rounding aggregation, liens, Recovery, or Resolved settlement.
INV-061 remains `OPEN_EVIDENCE`; production code, engine pin and status ledgers
are unchanged. This tests/docs increment is safe for main within that scope.

Rejected as already covered: row425 carry-fidelity metadata, INV-047 route
witness composition, INV-051 nonunit owner-reduction partitions, maximum-shape
liquidation, and observation-roster permutation alone. The distinguishing product
here is atom-scale nonunit liquidation sizing plus fee admissibility and exits.

Discovery used landed code and tests. After writing the candidate, open PR/issue
search results were used only as withheld title/number validation; no open bodies,
diffs, fixes, or tests were read or copied. The listed Hybrid provenance/current
observation and policy-replay findings concern different histories from this
fixed-price AuthMark fee-boundary product. The normal checkout was not edited
or switched.
