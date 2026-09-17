# Row 417 last-claimant regression, 2026-09-17

Program base: `c7b5ae07`; branch: `fix/row417-last-claimant-20260917`.
The existing stateful INV-067 selector is correct: production erased the last
haircut receipt before unrelated backing could expire into the payout snapshot.
Its unchanged assertions are the red/green regression evidence.

The pinned engine's `clear_fully_diluted_resolved_receipt_if_terminal` treated
zero unreceipted bound plus payout readiness as final. Fresh backing was still
excluded from residual, and its later expiry increased the snapshot after the
only claimant had lost its receipt. The baseline paid 1,250 instead of 1,750 and
burned the remaining 500 tokens during slab closure.

Engine commit `4db11a8cb0053815e23a35d3a7d3edc265d8d866`, directly based on
`94979ede7db934545e53a8f210dd063a9ea3ea63`, changes only `src/v16.rs`:

- Retain a haircut receipt while fresh backing can still increase its rate.
  Keep the existing payout-readiness and claimable-value checks.
- Let resolved auto-crank expire one committed Fresh bucket from the supplied
  asset's two domains. Missing, premature or unrelated discovery with no payout
  due returns NonProgress; the wrapper's transaction boundary rolls back.

This repo updates both engine pins and the lockfile. No selector, expectation,
wrapper logic, invariant status or unrelated dependency changed.

## Results

| Exact execution | Result |
| --- | --- |
| Requested stateful selector, original SBF | FAIL: receipt absent, payout 1,250, mint supply down 500; peak 208,227 CU |
| Same selector, rebuilt SBF | PASS 1/1: receipt retained, payout 1,750, mint supply unchanged, slab tombstone; peak 208,228 CU |
| INV-067 late-expiry identity control | PASS: 4 worlds, 8 paid-prefix rollbacks, 8 retained top-ups; peak 315,353 CU |
| INV-067 crank/top-up order control | PASS: 4 worlds, 16 fresh-blockhash retries, 20 portfolio closures; peak 422,134 CU |
| Engine full-rate top-up control | PASS 1/1: partial payment, completion and zero replay payout |

The first attempt stopped because the worktree lacked the authenticated matcher;
building that fixture enabled the economic reproduction. Matcher and program SBF
builds passed with platform-tools v1.52; existing host warnings remain.

## Reproduction

Commands ran in `/dev/shm/percolator-row417-20260917`, except the engine test.
The dedicated host target already existed; the SBF target was a private copy of
`/dev/shm/percolator-public-gap-20260916-c91e-sbf-target`.

```bash
export CARGO_TARGET_DIR=/dev/shm/percolator-row417-20260917-host-target
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-public-gap-20260916-c91e-sbf-target/deploy/percolator_prog.so
family=inv_067_terminal_payout_completeness_and_exact_once_settlement
selector="$family::v16_program_late_unrelated_backing_cannot_outlive_and_erase_resolved_receipt"
CARGO_TARGET_DIR=/dev/shm/percolator-row417-20260917-matcher-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked
cargo test --locked --offline --test v16_program_stateful_fuzz "$selector" -- --exact --nocapture
# After the local engine commit and Cargo.toml pin edit:
git --git-dir=/home/anatoly/.cargo/git/db/percolator-e81a111d21da0965 fetch /dev/shm/percolator-row417-engine-20260917 fix/row417-last-claimant-20260917
cargo update --offline -p percolator
CARGO_TARGET_DIR=/dev/shm/percolator-row417-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row417-20260917-sbf-target/deploy -- --locked
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row417-20260917-sbf-target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_program_stateful_fuzz "$selector" -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  "$family::late_expiry::v16_program_retained_claim_identity_survives_late_expiry_recipient_rotation_and_atomic_retry" \
  "$family::v16_program_resolved_crank_topup_batch_order_retries_pay_exactly_once"
cd /dev/shm/percolator-row417-engine-20260917
CARGO_TARGET_DIR=/dev/shm/percolator-row417-engine-20260917-target cargo test --locked --offline --features fuzz --test v16_spec_tests v16_resolved_payout_topup_finishes_receipt_without_overpaying -- --exact --nocapture
```

Original SBF SHA-256: `87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.
Fixed SBF SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
Logs: `/dev/shm/percolator-row417-20260917-{red,build,green,controls}.log`
and `/dev/shm/percolator-row417-engine-20260917-control.log`.

## Limits

Both commits are local and unpushed. The engine worktree is
`/dev/shm/percolator-row417-engine-20260917`, on the same named branch in the
`/home/anatoly/percolator` repository. A fresh checkout needs that engine commit
available to Cargo; the GitHub dependency URL does not yet publish it.
Only the named exact tests ran. No broad suite, Kani proof, arbitrary multi-wave
backing history or whole-invariant closure is claimed.
