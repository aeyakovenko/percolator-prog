# Standalone first-admission disposition, 2026-09-12

Base: `origin/codex/astra-open-holdout-ledger-20260912` at
`e660a52e7efef3c54041d26d78480107f7cc76f9`.
Worktree: `/tmp/astra-standalone-admission-20260912-f29c`.
Branch: `codex/astra-standalone-admission-20260912-f29c`.
Only current repository source, the locked engine source, tests and docs informed
this increment. The original checkout was not edited; no remote fetch, open PR
diff, external test copy or push was used. Production and Cargo inputs are unchanged.

## Scope and Contract

The selected slice is standalone first admission without an explicit fee/refresh
prefix. Existing INV-027 first-admission witnesses use SyncMaintenanceFee/crank or
Withdraw prefixes; the integrated reward-recipient witness also synchronizes and
refreshes both participants before admission. The new narrow file
[`cu/inv_027_standalone_first_admission.rs`](cu/inv_027_standalone_first_admission.rs)
is mounted under `inv_027_protected_principal_seniority::joint_admission_liabilities`.
It adds no reward handoff, Hybrid capacity/carry, interleaved carry, active-claim
evidence or mixed-provider liquidation matrix.

The current wrapper's `collect_maintenance_fee_before_trade_view` explicitly
defers maintenance realization for flat accounts to subsequent value-debit routes.
Its trade currentness helper also permits flat first opens. The test therefore
uses enough capital to satisfy initial margin after all elapsed fees. It does not
assert that first admission collects those fees or enforces a net-of-arrears
margin boundary. A successful trade and a current certificate alone cannot prove
that stronger obligation.

Two independently owned portfolios are created at slots 1 and 2 and funded with
2,000 and 1,000 SPL atoms. An empty keeper portfolio advances the authenticated
AuthMark market to slot 6 while both funded portfolio Accounts remain identical.
At 7 atoms per slot, the owners owe 35 and 28 maintenance atoms. A 1.1-lot first
open at price 100 requires 110 initial-margin atoms and collects
`ceil(110 * 100 / 10,000) = 2` trade-fee atoms from each owner. Its transaction
contains only compute-budget instructions and one TradeNoCpi or BatchTradeNoCpi.

Four worlds cross single/batch transport and long/short direction. The next
owner-signed reduction closes the entire exposure at the same price and slot,
collecting the deferred maintenance without a SyncMaintenanceFee or owner refresh
instruction. Each owner withdraws its complete independently computed entitlement:
1,963 and 970 atoms. The keeper receives zero; the remaining 67 booked and raw
vault atoms belong to insurance, with exact long/short budgets 33/34 on asset 0
and zero budgets on other assets. Payout order follows direction, so this does not
claim an independent cross-product of payout order and direction.

Each admission and reduction first executes in a transaction with a wrong-owner
withdrawal suffix. The suffix must reject Unauthorized at instruction index 3.
All tracked and compiled Accounts match the pre-transaction image exactly,
including position epochs, fee cursors, certificates, mint and SPL custody;
only the payer's calculated signature fee is deducted. The same trade request
then succeeds with a fresh transaction envelope. No snapshot is restored.

After every measured success and rejection, the test checks each owner's capital,
fee cursor, SPL balance, position and OI, exact insurance/domain attribution,
independent market stock and reservation/encumbrance censuses, source credit rates
and every current owner certificate. Both admission certificates must be current
and match independent recomputation. Keeper, signer, authority and mint Accounts
are preserved exactly. All economic accounts are built through System/SPL/ATA and
wrapper instructions; harness controls are signer SOL, Clock and blockhashes.

## Remaining Gaps

| Holdout | Increment | Remaining gap |
| --- | --- | --- |
| #413 | Sufficiently funded standalone first admission, deferred collection on reduction, exact owner exit and late rollback | Net-of-arrears admission boundary, insufficient funds, arbitrary histories, CPI and flat reopening |
| #422 | No new reward evidence | General paid-mark/reward provenance and selected-asset histories |
| #423 | No new capacity evidence | Over-capacity source admission, recovery and arbitrary retained-source histories |
| #425 | No new carry evidence | Trade before uncommitted accrual and terminal fractional carry |
| #426 | No new observation coverage | Pending/uncommitted observation omission before complete refresh and broader selected-asset/provider assignment |

All five rows remain **OPEN**. No invariant status or discovery metadata is
promoted. Source claims, liens and encumbrances are zero in this slice; their
censuses establish absence of unintended attribution, not nonzero backing
realizability. Insurance remains in the vault; reserve payout, portfolio deletion
and slab retirement are outside this increment. No marginal parameter sweeps or
speculative margin-boundary assertions are retained.

## Validation

The changed selector and three adjacent controls pass **4/4** in one exact
selection. The new selector completes **4 worlds, 8 exact rollbacks, 8 owner
payouts and 16 successful measured transactions**, with **329,011 peak CU**.
Trades retain the existing 345,000-CU limit, withdrawals retain 300,000 and
trade/withdrawal bundles use their combined 645,000 ceiling. Setup is outside
the measured transaction count. The invariant charter/index passes **1/1**;
`cargo fmt --all -- --check` and `git diff --check` pass.

Development corrected the test's fee-rate type, application of the custody CU
limit to a trade, and an extraneous withdrawal signer. No production conformance
failure was observed. Existing unused-support and `solana-client v1.18.26`
future-compatibility warnings remain.

A fresh locked/offline default-feature wrapper SBF was built in this worktree
using platform-tools v1.52. Host compilation also uses a fresh private target,
with no copied artifacts. Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
Wrapper SHA-256: `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-standalone-admission-f29c-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 TMPDIR=/dev/shm
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_027_protected_principal_seniority::joint_admission_liabilities::standalone_first_admission::v16_program_standalone_first_admission_preserves_deferred_fee_owner_entitlement \
  inv_027_protected_principal_seniority::v16_program_flat_first_admission_fee_prefix_is_atomic_and_entitled \
  inv_027_protected_principal_seniority::v16_program_flat_withdrawal_fees_precede_first_admission_and_roll_back_custody \
  inv_027_protected_principal_seniority::joint_admission_liabilities::reward_recipient_first_risk::v16_program_never_exposed_reward_recipient_settles_own_fees_before_first_risk
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
```
