# Scope F first-admission fee and flat-account entitlement conformance

Base: `origin/codex/astra-open-holdout-ledger-20260912` at
`cf0f2832668a3401ab189fc2b657fb5c3eb630f9`. Isolated worktree:
`/home/anatoly/worktrees/pr135-scope-f-first-admission-20260913`; branch:
`codex/pr135-scope-f-first-admission-20260913`. Engine pin:
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

The new child is mounted by `cu/inv_027_joint_admission_liabilities.rs`.
Its scope follows INVARIANTS.md INV-027/044/053/060/081 and the supplied branch's
row 413/434 status ledger. No open PR diff or test was inspected or copied.
Production sources, engine pin, shared harness and invariant status rows are unchanged.

## Public histories and independent oracle

Three explicit boundaries plus five XorShift cases with seed `[0x27; 16]` vary
maintenance rate, elapsed interval, quantity around whole-lot boundaries, trading
bps, partial payout, constrained party and prior exposure. Every input crosses
TradeNoCpi, TradeCpi, BatchTradeNoCpi and BatchTradeCpi with two collection schedules:
SyncMaintenanceFee then Withdraw, or Withdraw with implicit maintenance collection.
The base fee is initialized to the input bps, giving all transports identical
nonzero fee terms. One-leg batches use an independently computed fee cap.

System/SPL/ATA/wrapper instructions construct all economic accounts. Mint authority
is revoked at the sum of the three deposits. Only program loading, signer SOL,
Clock and blockhashes come from the harness. The market advances one slot at a time
under its configured accrual bound; every funded flat portfolio stays byte-identical
during that aging. Cases with a prior episode first open and close half a contract,
paying both trading fees and the intervening maintenance interval.

An unrelated senior owner receives all principal remaining after its own elapsed
fee before either trader collects. The traders then collect, withdraw a generated
partial amount and refresh. Same-slot fee retries preserve all tracked economic
Accounts exactly. An over-boundary open rejects with exact EngineInvalidConfig at
instruction 2, preserving those Accounts; the exact post-fee open succeeds. Closing
uses the opposite transport, followed by complete owner payouts in reverse order.
Matcher grants are renewed publicly where needed and checked against the same book.

The continuing book derives maintenance as `rate * (slot - prior_cursor)` and each
trade charge as `ceil(ceil(abs(quantity) * price / POS_SCALE) * bps / 10000)`.
It never seeds fees, payouts or capital from observed post-transition deltas.
After each submitted history attempt it checks each owner's capital, fee cursor,
zero fee debt/PnL, identity and SPL receipts, plus aggregate capital, vault custody,
mint supply, both insurance budgets, OI, stock and reservation censuses. Maintenance
splits are rounded per collection; equal per-owner trade fees fund both domains.
Immutable mint, owner wallets, authority and matcher/delegate accounts remain framed.
Rollback comparisons exclude the network-fee payer and cover the complete tracked
economic Accounts, including matcher context. This is not a general compiled-account
or multi-instruction rollback theorem.

Every current certificate equals the independent raw-state model. Additional
input-derived assertions require equity to equal remaining owner capital and IM/MM
to equal 100%/50% of ceiling-rounded notional, with no extra fee penalty and zero
liquidation deficit. Worst-case loss equals notional. Both refreshed flat and newly
admitted certificates must be current. Full admission certificates agree across
all eight route/collection combinations for each input. The constrained owner lands
exactly at IM after maintenance, partial payout and the rounded opening charge.

## Classification and limits

This is a net-new bounded generated public-route probe. Existing fixed first-open
and reopen selectors separately cover explicit fee prefixes; this increment joins
generated fee policies/amounts, implicit withdrawal collection, prior fee episodes,
rounded first-risk trading charges and per-owner final entitlement across all four
transports. Row 413 remains OPEN and row 434 remains COVERED. INV-027 stays
REFUTED_CURRENT; INV-044/053/060/081 stay OPEN_EVIDENCE. There is no status promotion,
whole-invariant closure or production change.

Prices are fixed AuthMark, funding and junior claims are zero, owners are distinct,
and batches have one asset/leg. The seniority evidence is owner-local fee/principal
ordering, not a new underbacked-junior-claim product. Standalone first admission
with deferred maintenance, fee exhaustion/debt, shared owners, policy transitions,
multi-asset batches, arbitrary histories and terminal resolution remain outside
scope. Scope A observation and Scope B residual products are not extended.

## Validation

The new exact selector passes 1/1: 64 worlds, 1120 submitted history attempts,
64 exact margin rollbacks, 128 same-slot fee no-ops and 192 complete owner payouts.
The count excludes public initialization and separately checked matcher grants.
Peak measured transaction cost is 310208 CU against a 400000 bound.
The two exact compatibility selectors pass 2/2: flat first admission (two worlds,
peak 355050 CU) and flat reopen route switching (16 worlds, peak 392645 CU).
The charter/index and authoritative-status metadata selectors pass 2/2. Formatter,
working/staged/commit whitespace checks and the production/harness/pin/status
equality check pass. Existing unused-support and Solana future-compatibility
warnings remain.
Initial authoring runs corrected the portfolio-ID accessor, the fixture's base-fee
configuration and the one-slot market-accrual schedule. No production correction
was needed for the final conformance histories.

Fresh default-feature wrapper and matcher SBF artifacts were built from this
worktree, then reused for test-only edits and focused checks. No build cache or test
was copied from another PR. Wrapper SHA-256:
`8a010d8283186721866a95a71d252a3265015fbf563d6ce566cd44f263659349`.
Matcher SHA-256:
`50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
The private matcher compiler cache was cleaned after its SBF output was built.
The complete suite and engine proofs are not run.

Exact commands from the isolated worktree (the SBF commands precede host checks):

```sh
export CARGO_TARGET_DIR=/tmp/pr135-scope-f-20260913-target
export PERCOLATOR_FUZZ_SBF=/tmp/pr135-scope-f-20260913-target/deploy/percolator_prog.so
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /tmp/pr135-scope-f-20260913-target/deploy -- --locked
(cd tests/fixtures/auth_matcher && CARGO_TARGET_DIR=/tmp/pr135-scope-f-20260913-matcher-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /home/anatoly/worktrees/pr135-scope-f-first-admission-20260913/tests/fixtures/auth_matcher/target/deploy)
(cd tests/fixtures/auth_matcher && cargo clean --target-dir /tmp/pr135-scope-f-20260913-matcher-target)
cargo test --locked --offline --test v16_cu inv_027_protected_principal_seniority::joint_admission_liabilities::generated_flat_fee_entitlement::v16_program_generated_flat_fee_collection_preserves_first_admission_entitlement -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 inv_027_protected_principal_seniority::v16_program_flat_first_admission_fee_prefix_is_atomic_and_entitled inv_027_protected_principal_seniority::joint_admission_liabilities::flat_reopen_routes::v16_program_flat_reopen_route_switch_preserves_fee_history_and_senior_exit
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
git diff --exit-code cf0f2832668a3401ab189fc2b657fb5c3eb630f9 -- src Cargo.toml Cargo.lock tests/v16_cu.rs tests/support tests/invariants/invariant_status.tsv tests/invariants/open_findings.tsv
```
