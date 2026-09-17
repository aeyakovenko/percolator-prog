# Row 412: partial matcher reduction and hostile flip suffix

Base: freshly fetched `origin/main`, `5b1da7aa`.
Private worktree: `/dev/shm/astra-ultra-row412-20260917`.
Primary INV-012 / INV-004, with bounded return-binding, economic-bound and
rollback evidence. Tests/docs only; no production mismatch or ledger change.

## Distinct increment

The new selector lives in
[`cu/inv_012_partial_flip_return_rollback.rs`](cu/inv_012_partial_flip_return_rollback.rs),
mounted by three lines in `inv_012_capability_and_delegate_scope.rs`.

- [The recent Row 412 note](row412_regression_health_20260917.md) tests retained
  grant writers around owner-driven reduction/flip and automatic revocation.
  This witness instead preserves the grant through a matcher-selected partial
  reduction and a direct cross-zero CPI, then rejects a hostile returned fill.
- `inv_009_retained_partial_words.rs` tests partial entry/residual one-shot
  budgets. This history starts exposed, partially reduces, crosses zero and
  separates live-grant episode binding from later same-tuple grant renewal.
- INV-019's `retained_return_freshness` composes separate pairs across expiry
  and silent returns. Here both requests consume successive episodes of the
  same pair; four malformed suffix responses must restore a successful partial
  reduction, its fees, request counter and synchronized capability.

## Public history and oracle

Four worlds cross suffix route (single / one-leg batch) with both position signs.
System-created market/portfolios/context, SPL minting/ATA creation, wrapper
deposits, owner matcher grant and fixture control instructions construct all
state. There are no `set_account` calls or program-owned byte writes in the
new test. Wrapper and both existing matcher fixtures are rebuilt from this
checkout; fixture source is unchanged.

Three owners deposit 1,000,000 atoms each; the third portfolio is a complete
Account frame throughout. Price stays 100, fee is 100 bps, Clock slot 1 and
grant expiry 100. With signed unit `q`, the taker position is independently
expected to follow `4q -> 2q -> -2q -> 0`, with the opposite LP position.
Fees per owner are exactly `4 + 2 + 4 + 2 = 12` atoms. Each final owner payout
is 999,988 atoms; the vault retains 1,000,024, comprising the untouched owner's
1,000,000 principal and 24 insurance atoms. Mint supply stays 3,000,000 with
mint authority revoked.

The partial request signs `-4q`; fixture mode 15 returns authorized `-2q`.
Only single CPI supports this partial; the batch suffix is a full fill.
Before committing the partial, the test signs both stale-episode and next-episode
flip requests, plus standalone retained deliveries. A valid partial/control/flip
bundle succeeds in simulation with exactly two wrapper successes. These repaired
bundles are simulations, not committed bundles.

Five actual bundles then reject at the final instruction: one old-episode
suffix (`EngineStale`, before its matcher call), and four current-episode
suffixes with reversed direction, wrong request ID, wrong delegate-derived LP
ID or an unflagged partial (`InvalidAccountData`, after both matcher calls).
Every rejection restores complete transaction/protected Accounts, including
context bytes, capability fields, portfolio epochs, request sequence, token
accounts and absent delegate; the independent payer loses exactly its signature
fee. Logs require the successful wrapper prefix.

The unchanged pre-signed partial subsequently commits. Its LP capability tuple,
sequence, expiry and fee cap remain live while both episodes advance. A separately
retained old-episode request rejects before CPI; its distinct compute envelope
prevents transaction-cache rejection from substituting for application binding.
The unchanged next-episode request crosses zero without an LP-owner signature.
Same-tuple renewal then consumes only the grant sequence: a retained current-episode
exit rejects, and a fresh-sequence exit flattens both positions. Every accepted
matcher response is checked against input-derived size/sign, price, flags,
request counter and delegate-derived LP identity. Exact capital, PnL, OI,
insurance/domain budgets and SPL balances are checked at each economic boundary;
accepted transactions also frame all protected accounts outside their write set.

## Validation

New selector: PASS, four worlds, 16 committed fills, eight SPL payouts and 28
complete-Account rollbacks. Peak accepted/simulated CU **358,593**; peak rejection
CU **195,159**, below the test's 1,200,000 ceiling.
Both exact controls pass: the recent retained-grant selector peaks at **468,379 CU**;
the flagged-partial control does not emit CU. Each run executes exactly one test,
with 1,468 filtered out. Scoped rustfmt, whitespace checks and the protected-path
guard pass. The commit is local only.

Initial development failures were a test accessor spelling and an incorrect
test expectation that the matcher ABI LP identity was the portfolio incarnation.
The ABI identity is the delegate's first eight bytes; the oracle now derives it
independently. Neither failure established a production mismatch.

Private host/SBF caches were copied from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target`; wrapper and
auth/hostile fixtures were built locked/offline with platform-tools v1.52.
Engine pin: `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
Logs: `/dev/shm/astra-ultra-row412-{sbf-build,auth-build,hostile-build,new-test,grant-control,partial-control}.log`.

Artifact SHA-256:
- Wrapper: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
- Auth matcher: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
- Hostile matcher: `e0c20fad34a7822cc6ce42a3c77ff08a8591977102f0c497a339d66a9dd6240a`.

```bash
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row412-host-target
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-ultra-row412-sbf-target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu inv_012_capability_and_delegate_scope::partial_flip_return_rollback::v16_program_retained_partial_flip_binds_episode_grant_and_hostile_return -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_012_capability_and_delegate_scope::joint_incarnation_binding::revocation_atomicity::retained_grant_episode_retry::v16_program_retained_grant_admission_tracks_partial_and_cross_zero_commit_or_rollback -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_009_partial_fill_and_retry_accounting::v16_program_tradecpi_flagged_partial_accounts_actual_fill_and_requires_fresh_retry -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true tests/invariants/cu/inv_012_partial_flip_return_rollback.rs tests/invariants/cu/inv_012_capability_and_delegate_scope.rs
rustfmt --check --edition 2021 --config skip_children=true tests/invariants/cu/inv_012_partial_flip_return_rollback.rs tests/invariants/cu/inv_012_capability_and_delegate_scope.rs
git diff --check
git diff --cached --check
git show --check HEAD
git diff --exit-code 5b1da7aa HEAD -- src Cargo.toml Cargo.lock tests/fixtures tests/support tests/v16_cu.rs 'tests/invariants/*.tsv'
```

## Remaining gaps

Bounded Live, one active asset, fixed price, integral half-fill and no funding/PnL.
Arbitrary partial ratios, multi-leg batches, moving prices, authority/expiry or
context lifecycle, asset reuse, Recovery/Resolved episodes, other hostile response
classes, native custody and validator blockhash aging remain outside this increment.
No row status or general closure claim; Rows 411/417/421/424/428/433 are untouched.
