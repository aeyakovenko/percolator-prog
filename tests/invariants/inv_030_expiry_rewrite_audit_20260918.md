# INV-030: expiry rewrites cannot renew discounted credit

Classification: nonduplicate bounded public-route coverage, passing on main.
Base: `00d17465406abbe56e263463fae7ba6269b6471b` (fresh origin/main).
Worktree: `/tmp/percolator-astra-generation-20260918`, backed by its own bare
repository at `/tmp/percolator-astra-generation-20260918.git`.
No open PR branches, diffs, commits or tests were inspected.

One new stateful LiteSVM test covers both source sides. Twenty position units
and a five-atom authenticated mark move create a 100-atom claim. The loser stays
unsettled, so exactly 37 provider atoms support it. Nonzero top-ups with earlier
and later expiries reject with EngineLockActive and exact transaction rollback;
one atom at the original expiry succeeds and raises credit from 37% to 38%.
Zero-amount top-ups with expiry 0, original-1, original, original+1 and u64::MAX
preserve every source record, bucket, portfolio, provider ledger and token account,
both before and after normalization at the original expiry. Credit becomes zero
at that expiry; claim face and vault tokens remain unchanged.

The oracle derives claims from size and mark movement and backing from actual
deposits, independently computes the rate and realizability cap, and reconciles
account/domain/market claims and SPL custody. Existing public stock/encumbrance
censuses and trace validation additionally require exact failed-transaction
rollback and no out-of-band economic mutation. Unlike the existing INV-028
zero-amount input gate, this has a nonzero discounted live claim and checks its
expiry continuation. It does not duplicate shared-lien CPI expiry/refill or a
native reserve swap. Rows 423/424 and invariant statuses remain unchanged; no
capacity, activation, insurance-lien reachability or general liveness claim.

## Validation

Dependency artifacts seeded private target directories; wrapper and authenticated
matcher SBF were rebuilt from this checkout with locked dependencies and engine
`4db11a8cb0053815e23a35d3a7d3edc265d8d866`. Exact commands, from the worktree:

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/percolator-astra-generation-20260918-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-astra-generation-20260918-sbf-target/deploy/percolator_prog.so
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_TARGET_DIR=/dev/shm/percolator-astra-generation-20260918-sbf-target cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-astra-generation-20260918-sbf-target/deploy -- --locked
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_TARGET_DIR=/dev/shm/percolator-astra-generation-20260918-sbf-target cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked
cargo test --locked --offline --test v16_program_stateful_fuzz inv_030_credit_rate_determinism_and_fail_closed_behavior::v16_program_backing_expiry_rewrite_cannot_renew_discounted_credit -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_program_stateful_fuzz inv_030_credit_rate_determinism_and_fail_closed_behavior::v16_program_source_credit_saturation_boundary_preserves_claims_and_advances_epoch -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/stateful/inv_030_credit_rate_determinism_and_fail_closed_behavior.rs
git diff --check
```

Both builds pass. Each exact selector passes 1/1. The new selector executes two
31-step histories with 20 zero-amount calls, four expiry-mismatch rejections and
two funded controls; peak successful transaction CU is 221,198. Formatting and
whitespace checks pass. Existing dead-code and Solana future-compatibility
warnings remain. An initial test assertion used the wrong numeric error code;
it was corrected to the named EngineLockActive discriminant, not a production fix.

Wrapper SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
Exact test logs: `/tmp/percolator-astra-generation-20260918-{new,control}.log`.
