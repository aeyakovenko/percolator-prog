# Retained Permitted-Policy Histories

Base: local `origin/codex/astra-open-holdout-ledger-20260912` at `adf21c47`.
Worktree: `/tmp/astra-retained-consent-coverage-20260912-01`.
Branch: `codex/astra-retained-consent-coverage-20260912-01`.
Inputs: this base's tests, invariant documents and source. No fetch, open-PR diff,
other branch source, or copied build artifact was used. The shared checkout was
left untouched. Production code, dependencies and holdout verdicts are unchanged.

## Overlap Search

Local `rg` searches covered retained fee/debit/consent/withdrawal histories and
their invariant mounts. The relevant existing relations are:

| Existing File | Already Owned Relation |
| --- | --- |
| `stateful/inv_014_retained_fee_stock.rs` | Fee decrease, insurance payout/trade ordering, replenishment and consumed-trade rollback |
| `stateful/inv_014_retained_fee_bundle.rs` | Atomic route products and a shared taker's separate signed fee envelopes |
| `stateful/inv_014_retained_delegated_fee_exit.rs` | LP consent, policy relaxation ordering and exit rollback |
| `stateful/inv_005_retained_debit_matrix.rs` | Independent capital/insurance/backing debit families after identity or authority changes |
| `cu/inv_064_insurance_withdrawal_policy_equivalence.rs` | Live/resolved budgets, payout schedules and optional ledger histories |
| `cu/inv_028_historical_latent_capacity.rs` | Historical and latent domains sharing bounded backing capacity |
| `cu/inv_012_portfolio_grant_rollback.rs` | Portfolio reincarnation and retained grant rollback |

Fresh reserve withdrawal/lifecycle controls would overlap existing coverage.
No such probe was added. Historical capacity and capability rollback are outside
the new selector.

## New Relation

`stateful/inv_014_retained_permitted_policy_history.rs`, mounted under INV-014,
compares direct histories with nonmonotone policy detours. Two disjoint owner
pairs sign at 19 bps before either delivery. The first executes at 7 bps, the
second at 31 bps. The detour inserts 37 then zero before each delivery. Policy
always stays within both participants' authorization. Taker/LP cap ordering
reverses between `[37,503]/[83,211]` and `[503,37]/[211,83]`.

Each pair independently uses single or batch CPI, with both trade directions
and fractional quantities at a 100,003 price: 32 worlds total. The initial
64 signed simulations preserve all tracked/compiled Accounts. Before each
retained delivery, a valid 23-bps policy prefix and the original fill execute;
a duplicate policy suffix rejects. Logs prove both wrapper successes and the
matcher call. All Accounts, including SPL and matcher context, roll back exactly;
only the independent payer's exact signature fee remains charged.

The original retained transaction then succeeds byte-for-byte. An input-derived
ledger checks each owner's capital, zero PnL, position, fee budget, domain fee
allocation, token ownership/mint, custody and fixed supply. Successful fills
advance only the expected position episodes while preserving LP fee terms and
matcher sequences. Fresh 19-bps closes, owner withdrawals and insurer payouts
empty custody. Complete SPL account bytes agree across all 32 worlds. Every
measured success also preserves complete unrelated Accounts and all lamports
except the payer fee. Independent stock and encumbrance censuses run throughout.
The standard fixture provides empty allocations and SPL setup; economic state
uses public instructions only, with no initialized program-byte injection.

## Remaining Gaps

| Holdout Label | Exact Boundary Still Unproved Here |
| --- | --- |
| 411 | Single-CPI taker rejection when current policy exceeds signed terms while LP consent remains permissive |
| 432 | Participant-local protection above consent, partial/multi-leg fills, and arbitrary policy histories; this increment covers only the stated permitted finite histories |
| 415 | Successful insurance-withdrawal stock-epoch consumption and replay after stock replenishment |
| 428 | Consumed insurance/backing debit binding across replenishment, authority/asset reuse, expiry/impairment and live/resolved/shutdown transitions |

These are partial INV-011/014/024/036/047/080 observations. No holdout is closed.
No above-consent delivery or consumed reserve-debit replay was exercised.

## Verification

Wrapper and authenticated matcher were built here, offline and locked, using
platform-tools v1.52. Host dependencies were built in a new private target.
Wrapper SHA-256: `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

New selector output: `worlds=32, simulations=64, exact_rollbacks=64,
rolled_back_matcher_calls=64, measured_successes=544, success_cu=150082,
failure_cu=149208`. This excludes fixture/authority/grant setup CU. Development
corrected private fixture-key access and the expected LP position-epoch advance.
The two adjacent selectors pass (2/2): shared-taker bundle peak successful CU
`296980`; retained fee stock peak CU `175119`. The invariant index passes (1/1),
as do `cargo fmt --all -- --check` and both unstaged/staged whitespace checks.
Existing unused-support and Solana future-compatibility warnings remain.

Exact commands (from the worktree):

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-retained-consent-coverage-20260912-01-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=6 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_BUILD_JOBS=4 cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
CARGO_TARGET_DIR=/dev/shm/astra-retained-consent-coverage-20260912-01-matcher-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_BUILD_JOBS=4 cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$PWD/tests/fixtures/auth_matcher/target/deploy" -- --locked
cargo test --locked --offline --test v16_program_stateful_fuzz inv_014_delayed_policy_and_policy_epoch_safety::retained_permitted_policy_history::v16_program_retained_cpi_owner_budgets_ignore_permitted_policy_detours -- --exact --nocapture
cargo test --locked --offline --test v16_program_stateful_fuzz -- --exact --nocapture --test-threads=1 inv_014_delayed_policy_and_policy_epoch_safety::retained_fee_bundle::v16_program_retained_shared_taker_fee_bundle_preserves_each_instruction_bound inv_014_delayed_policy_and_policy_epoch_safety::retained_fee_stock::v16_program_retained_fee_stock_bundles_preserve_owner_budgets_across_policy_and_replenishment
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
```
