# First risk after elapsed fees and preexisting target lag

## Isolation

Base: `adeac5fcacfb431db78c5646c9bdc9f70cf4f0ed`, the current HEAD of
`codex/astra-open-holdout-ledger-20260912` in
`/tmp/percolator-astra-watch.Cb2E7d` when this contribution began.
Worktree: `/tmp/percolator-first-risk-elapsed-20260912`.
Branch: `codex/first-risk-elapsed-conformance-20260912`.
Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

Only the supplied HEAD's implementation, integrated tests, public helpers and
pinned engine sources were inspected. No open PR branches, diffs or tests were
inspected or copied. Neither the main workspace nor integration worktree was
edited. This change adds one test child, its mount, this audit and a README entry.
Production, shared harness, dependencies and invariant/reopening verdicts are unchanged.

The child is [`cu/inv_027_first_risk_preexisting_lag.rs`](cu/inv_027_first_risk_preexisting_lag.rs).
Its exact selector is
`inv_027_protected_principal_seniority::joint_admission_liabilities::first_risk_preexisting_lag::v16_program_first_risk_after_elapsed_fees_and_preexisting_lag`.

## Distinct relation

| Integrated evidence | New relation in this test |
| --- | --- |
| `standalone_first_admission` | Tests explicit fee settlement and a discriminating margin boundary after a pre-first-risk target excursion, including common ownership. The standalone witness has ample collateral and defers fees until reduction. |
| `reward_recipient_first_risk` | No reward transfer. Lag exists on the traded asset before either portfolio has ever held a position; common-owned portfolios retain separate fee liabilities and withdrawal limits. |
| `joint_admission_liabilities` | Both accounts are never-risked, with zero OI and no nontraded leg. A traded-asset preflight gate rejects even the numerical boundary, then target repair restores exact admission. The existing joint matrix has a live nontraded lagged leg; its flat-reopen history has no target excursion. |
| `fee_refresh_admission` | No external report renewal or stale timestamp. The changed authenticated target is economically distinct from the effective price before first exposure; identity and per-portfolio entitlement are explicit axes. |

Sixteen worlds cross the constrained party, shared/separate owners, single/batch
no-CPI trade, and the order in which both accounts synchronize and refresh. The
same order also determines payout order; these are coupled, not independent axes.
The constrained long sees target 99; the constrained short sees target 101.

## Public history and arithmetic

System, SPL Token, ATA and wrapper instructions create every economic account.
Signer SOL, Clock and transaction blockhashes are the only harness controls.
The fixed mint supply is 335 atoms and its mint authority is revoked after funding.
Shared owners have two distinct portfolios and one public ATA. No program-owned
economic bytes are injected, restored or patched. Full-refresh comparisons operate
on disposable copies and never write back to LiteSVM.

The owners are born at slots 1 and 2. Before aging, the test constructs all four
first-risk instruction contents. An empty keeper advances the market to slot 6
at price 100, then a same-slot authorized mark update creates target 99 or 101.
Both funded portfolio Accounts remain exactly equal to their pre-aging snapshots;
there is no prior position, realized PnL or fee collection. At seven atoms per slot,
maintenance is independently `7 * (6 - birth) = [35, 28]`.

Post-fee capital is 112 for the constrained portfolio and 160 for its counterparty.
Deposits are `[147, 188]` when the first portfolio is constrained and `[195, 140]`
when the second is constrained. IM/MM are 100%/50%, funding and trade fees are zero.

Each admission bundle synchronizes and refreshes both accounts, then attempts its
retained trade content. At 1.1 lots, notional is 110 and adverse one-unit lag rounds
up to two atoms, giving hypothetical IM 112. One position quantum more gives IM
113. Both reject `EngineLockActive` at transaction instruction 6: the pinned
engine's `trade_preflight_risk_gate` rejects risk increases on a lagging traded
asset before numerical margin. These denials certify the gate, not successful
lag-penalty certificate arithmetic. Both prefixes roll back the 63 maintenance
atoms and all refresh state exactly.

An authorized same-slot target return to 100 and keeper crank remove that gate.
Both owners remain flat with unchanged principal and original fee cursors. The
previously constructed 1.12-lot request requires exactly 112 IM atoms. One position
quantum more requires 113 and rejects `EngineInvalidConfig` at instruction 6,
again rolling back the entire fee/refresh prefix. The exact request succeeds.
Omitting maintenance would admit 113; collecting twice would reject 112.

After admission, both certificates have IM 112, MM 56, worst-case loss 112 and
liquidation deficit zero. Equity is independently 112/160 (or 160/112), so the
constrained owner has zero IM headroom. Every current certificate equals independent
recomputation and snapshot engine full refresh. Complete admission certificates
also match across ownership, transport and prefix order for each constrained party.
Signed long/short exposure and both OI lanes are exactly 1.12 lots.

Another same-slot sync of both portfolios is a byte-exact economic no-op. A full
close followed by withdrawal of 113 from the 112-atom portfolio rejects
`EngineLockActive` at instruction 3 and restores the successful close. This holds
with a common owner despite the other portfolio's available 160 atoms. The same
close instruction content then succeeds. Each portfolio pays its complete 112 or
160 atoms through its public ATA. Every intermediate payout checks the originating
portfolio's individual remaining capital; a shared ATA contains the sum paid so far.

Final capital and OI are zero, owner payouts total 272, and booked/raw vault stock
is exactly 63 insurance atoms. Per-account floor/remainder allocation gives asset-0
domain budgets 31/32; all other budgets are zero. No claims or keeper reward explain
any owner entitlement. After every measured success and rejection, checks reconcile
capital, fees, PnL, SPL balances, target/effective prices, K/F indices, OI, independent
stock/reservation censuses and source credit rates. Mint, signer and authority
Accounts stay exact. Rollback frames include all tracked and compiled Accounts,
including data and economic lamports, with only the payer's calculated signature
fee deducted. Transactions are verified and fit the 1,232-byte limit.

## Evidence limits

INV-010 gains bounded request-content retention across the mark excursion, prefix
permutation and retry; transactions receive fresh signatures and blockhashes. This
is not retained-signature or durable-nonce coverage. INV-024/027/062 gain individual
fee/payout attribution under shared ownership, including rejection of portfolio
overdraw. No junior PnL, source claim or underbacked obligation is present.
INV-044 covers absence of phantom value during certificate/target changes and fee
sync; soft maintenance credit and durable liens are absent. INV-053 covers the
successful zero-lag certificates. INV-060 covers maintenance once in equity and
the fail-closed traded-lag gate, not successful nonzero-lag lane decomposition.
INV-081 evidence is this bounded public composition, not a complete-route proof.

Standalone first risk with uncollected flat fees remains open. Also outside this
increment: CPI, multi-leg batches, insufficient/noncollectible fee debt, nonzero
funding or PnL, rewards, soft credit, liens, pending losses, effective-price movement,
catchup to an adverse target, arbitrary oracle histories, shutdown/resolution,
insurance payout, account deletion and maximum shapes. All invariant verdicts and
reopening labels remain unchanged; no closure or proof claim is added.

## Validation

The selector passes 16 worlds, 64 exact rollbacks, 16 exact-boundary admissions,
16 paired same-slot sync no-ops and 32 owner payouts. Maximum measured CU is 355,035.
Admission bundles retain a 750,000 ceiling; standalone closes use 345,000, custody
uses 300,000 and close/withdrawal bundles use 645,000. Setup is outside those counts.

The eight nearby selectors below pass together (8/8); the invariant charter/index
passes (1/1). Full-repository `cargo fmt --all -- --check`, staged/unstaged whitespace
checks and production/harness/dependency/verdict equality checks pass. The full test
suite and engine proofs were not run. The index target retains 346 unused-support
warnings and Cargo reports the existing `solana-client v1.18.26` future-compatibility
warning.

During construction, an expected numerical-margin error was corrected to the
documented traded-lag preflight gate. The common-owner fixture also stopped issuing
the same LiteSVM 0.1 airdrop twice: that helper builds a deterministic transaction
independent of the latest blockhash. Public System/InitPortfolio creation now reuses
the funded owner. Neither was a production conformance failure; no invariant
assertion was relaxed to permit an unsafe success.

Wrapper and auth-matcher SBF artifacts were freshly built, locked and offline with
platform-tools v1.52. The new selector needs no matcher; the joint-admission control
uses the fresh fixture. Host outputs are private under
`/dev/shm/percolator-first-risk-elapsed-target` and matcher build outputs under
`/dev/shm/percolator-first-risk-elapsed-matcher-target`. Fixture deploy output is in
the isolated worktree's ignored `tests/fixtures/auth_matcher/target/deploy`.

- Wrapper SHA-256: `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
- Auth matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

Commands, from the isolated worktree unless stated otherwise:

```sh
export CARGO_TARGET_DIR=/dev/shm/percolator-first-risk-elapsed-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export TMPDIR=/dev/shm CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
# Run from tests/fixtures/auth_matcher:
CARGO_TARGET_DIR=/dev/shm/percolator-first-risk-elapsed-matcher-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /tmp/percolator-first-risk-elapsed-20260912/tests/fixtures/auth_matcher/target/deploy -- --locked

cargo test --locked --offline --test v16_cu inv_027_protected_principal_seniority::joint_admission_liabilities::first_risk_preexisting_lag::v16_program_first_risk_after_elapsed_fees_and_preexisting_lag -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_027_protected_principal_seniority::joint_admission_liabilities::standalone_first_admission::v16_program_standalone_first_admission_preserves_deferred_fee_owner_entitlement \
  inv_027_protected_principal_seniority::joint_admission_liabilities::reward_recipient_first_risk::v16_program_never_exposed_reward_recipient_settles_own_fees_before_first_risk \
  inv_027_protected_principal_seniority::joint_admission_liabilities::v16_program_joint_accrued_liabilities_precede_risk_admission \
  inv_027_protected_principal_seniority::joint_admission_liabilities::v16_program_flat_reopen_fee_history_precedes_new_exposure \
  inv_020_authenticated_clock_slot_and_oracle_provenance::fee_refresh_admission::v16_program_timestamp_renewal_fee_refresh_and_funded_admission_retry \
  inv_060_single_sided_margin_and_penalty_accounting::v16_program_fee_and_target_lag_compose_exactly_once_in_health_lanes \
  inv_062_no_identity_assumptions_self_trade_containment::v16_program_same_owner_zero_fee_self_trade_round_trip_creates_no_value \
  inv_062_no_identity_assumptions_self_trade_containment::v16_program_same_owner_fee_self_trade_is_negative_sum_not_profitable
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git diff --exit-code adeac5fcacfb431db78c5646c9bdc9f70cf4f0ed -- src Cargo.toml Cargo.lock tests/v16_cu.rs tests/support tests/invariants/invariant_status.tsv tests/invariants/coverage_reopenings.tsv
git show --format= --check HEAD
```
