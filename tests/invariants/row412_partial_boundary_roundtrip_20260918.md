# Row 412: partial-fill flat and cross-zero round trips

Base: fetched `origin/main`, `dd24f45ab299511d7f46b0b0b6da1a03d12e3740`.
Worktree and branch: `/dev/shm/astra-ultra-row412-coverage-20260918` and
`astra-ultra-row412-coverage-20260918`. Tests/docs only; local commit, no push.

## Non-duplicate boundary

One selector is added to
[`cu/inv_012_partial_flip_return_rollback.rs`](cu/inv_012_partial_flip_return_rollback.rs),
reusing its existing `World`, public setup, matcher controls and account oracles.
No helper or fixture source changes are needed.

- [The recent partial/flip note](row412_partial_flip_returns_20260917.md)
  owns `4q -> 2q -> -2q -> 0`: the partial reduces without clearing or flipping;
  a later full fill crosses zero. The new partial itself clears or flips.
- `retained_same_asset_episode` owns full-fill LP close/reopen through a third
  portfolio, with the original taker unchanged. This increment moves both
  counterparties through a matcher-selected half fill and restores their vector.
- [The retained-grant note](row412_regression_health_20260917.md) owns bilateral
  revocation and retained grant writers. Here the synchronized LP grant remains
  enabled, with the same tuple, sequence, expiry and fee cap throughout.
- `inv_009_retained_partial_words` owns same-direction partial entry/residual
  budgets from flat; it does not clear or cross an existing position with a partial.

## History and checks

Eight public LiteSVM histories cross both position signs, single/one-leg batch
exit routes, and two partial boundaries. With signed unit `q`, a single CPI
requests `-8q` or `-12q` from `4q`; existing fixture mode 15 returns `-4q` or
`-6q`. The independently expected taker paths are:

- `4q -> 0 -> 4q -> 0`, with 16 fee atoms per owner.
- `4q -> -2q -> 4q -> 0`, with 20 fee atoms per owner.

Before the partial, two separately signed envelopes of the same original exit
successfully simulate. Their compute limits differ to avoid transaction-cache
rejection substituting for episode validation. The partial and a future-episode
restoration are also signed before either lands, without LP-owner signatures.
The first retained exit rejects `EngineStale` before CPI at the flat/flipped
boundary. The unchanged restoration then commits through the opposite route
from the exit. Even with the original position vector restored, the second
retained exit rejects before CPI. Fresh episodes permit the same-sized exit
under the unchanged grant, flattening both portfolios.

Every economic boundary checks actual signed positions, active legs, both
episodes, request count, grant fields, OI, fee/capital/insurance attribution,
zero PnL, SPL custody and fixed mint supply. Each accepted response binds the
actual fill, request ID, delegate-derived LP identity and partial flag. The
16 refusals restore complete transaction/protected Accounts, including absence,
with only the separate payer's exact signature fee deducted. An unrelated
funded portfolio remains byte-identical. There are 32 committed fills and 16
successful non-mutating exit simulations; no payouts are added by this selector.

## Validation

Both exact selectors pass, each running one test with 1,486 filtered out:

| Selector | Accepted/simulated peak CU | Rejection peak CU |
| --- | ---: | ---: |
| New partial-boundary round trip | 195,502 | 2,889 |
| Existing partial/hostile-flip control | 364,593 | 200,245 |

These are bounded transaction measurements excluding bootstrap, not worst-case
CU claims. The wrapper and both unchanged matcher fixtures were rebuilt
locked/offline with platform-tools v1.52 using private copied caches. Engine pin:
`4db11a8cb0053815e23a35d3a7d3edc265d8d866`.

Artifact SHA-256:

- Wrapper: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
- Auth matcher: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
- Hostile matcher: `e0c20fad34a7822cc6ce42a3c77ff08a8591977102f0c497a339d66a9dd6240a`.

```bash
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row412-coverage-20260918-host-target
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 TMPDIR=/dev/shm
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-ultra-row412-coverage-20260918-sbf-target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu inv_012_capability_and_delegate_scope::partial_flip_return_rollback::v16_program_retained_exit_stays_stale_after_partial_flat_or_cross_zero_roundtrip -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_012_capability_and_delegate_scope::partial_flip_return_rollback::v16_program_retained_partial_flip_binds_episode_grant_and_hostile_return -- --exact --nocapture --test-threads=1
rustfmt --check --edition 2021 --config skip_children=true tests/invariants/cu/inv_012_partial_flip_return_rollback.rs
git diff --check
git show --check HEAD
git diff --exit-code dd24f45a HEAD -- src Cargo.toml Cargo.lock tests/fixtures tests/support tests/v16_cu.rs 'tests/invariants/*.tsv'
```

Test logs: `/dev/shm/astra-ultra-row412-coverage-20260918-{new,control}-test.log`.
Build logs use the same prefix and `{sbf,auth,hostile}-build.log`.
Scoped rustfmt, working/staged/committed whitespace and protected-path checks pass.
Only the existing Rust file, this note and the short README entry change.

This covers Live, one active asset, fixed price, integral half fills and zero
funding/PnL. Arbitrary revocation histories, additional hostile-return classes,
multi-leg partials, moving prices, authority/context lifecycle and non-Live
boundaries remain outside this increment. No whole-row closure or status claim.
