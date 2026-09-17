# INV-027 standalone flat reopen audit, 2026-09-17

Base: `9e39d78c2a7b5bd60d7017c5471e6c3177b70035` (`origin/main` at worktree creation).
Worktree: `/dev/shm/percolator-row413434-health-20260917`.
Engine observed in the fresh build: `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
Owner: [cu/inv_027_generated_flat_fee_entitlement.rs](cu/inv_027_generated_flat_fee_entitlement.rs),
mounted under `inv_027_protected_principal_seniority::joint_admission_liabilities::generated_flat_fee_entitlement`.

## Guarantee and novelty

The finding-blind candidate comes from the documented standalone-reopening exclusion:
the existing generated and route-switch histories settle fees before reopening;
standalone first-admission coverage starts with never-traded portfolios. No holdout
implementation, production source or injected economic state supplies this test.

Two input cases cross all four transports and standalone versus explicit fee/refresh
settlement (16 worlds). Owners open, age, close flat with rounded trading and maintenance
charges, withdraw part of their principal, then age again without portfolio mutation.
An unrelated senior owner withdraws before the traders settle their remaining fees.
Public matcher grants are renewed as needed; CPI maker signatures are absent from fills.

A reopen above post-fee IM fails with `EngineInvalidConfig`; every tracked and compiled
Account, including matcher context and economic lamports, restores exactly except the
calculated runtime signature fee. The exact boundary succeeds as one wrapper instruction
in eight standalone worlds. Both modes produce identical full current health certificates.
Same-slot fee sync is an exact no-op. Closure switches both CPI and batching families;
each owner receives exactly its remaining principal after every earlier payout and fee.

The existing input-owned `FeeBook` independently accumulates fee cursors, maintenance,
rounded trade charges, per-domain insurance and cumulative SPL payouts. Every checked
attempt also reconciles stock, encumbrances, identity, positions/OI, mint supply, custody
and immutable accounts. The two new cases end with cumulative owner payouts
`[112, 185, 59]` / `[287, 214, 59]` and fee-only vaults of `96` / `178` atoms.
Public System/SPL/ATA/wrapper construction and clock advancement are the only setup paths.

## Verification

Fresh default-feature wrapper and auth-matcher SBF, platform-tools v1.52, private target.
Wrapper SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
Exact run: **2 passed, 0 failed** (40.50 s). New selector: 16 worlds, 304 checked attempts,
16 margin rollbacks, 32 sync no-ops, 48 complete owner payouts. Existing shared-runner
selector: 64 worlds, 1,120 attempts, 64 rollbacks, 128 no-ops, 192 complete payouts.
Each peaks at **310,239 CU**, below 400,000. CU/counts cover runner submissions, excluding
construction and matcher-configuration helpers. The existing solana-client future-compatibility
warning remains. No broad suite, production fix or red/green bug claim.

From the worktree, the exact commands are:

```sh
export CARGO_TARGET_DIR="$PWD/target" CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$PWD/tests/fixtures/auth_matcher/target/deploy" -- --locked
cargo test --locked --offline --test v16_cu inv_027_protected_principal_seniority::joint_admission_liabilities::generated_flat_fee_entitlement::v16_program_standalone_flat_reopen_crystallizes_fees_and_preserves_owner_exit -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_027_protected_principal_seniority::joint_admission_liabilities::generated_flat_fee_entitlement::v16_program_generated_flat_fee_collection_preserves_first_admission_entitlement -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_027_generated_flat_fee_entitlement.rs
git diff --check
git diff --cached --check
git show --format= --check HEAD
```

## Remaining limits

This is bounded row-434 / INV-027 conformance with related fee, health and entitlement
evidence, not a new finding or status promotion. Prices are fixed, fees fully collectible,
and batches have one leg on one asset. Junior claims, funding/price lag, clipped/debt or
rewarded fees, policy changes, arbitrary histories, shared owners, multi-asset batches,
portfolio deletion, terminal exit and maximum shapes remain outside this increment.
No production/src/Cargo changes; no row412, row410/429, row418 or row425/426 file edits.
