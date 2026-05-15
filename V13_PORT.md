# v13 Wrapper Port Notes

Engine branch tested: `aeyakovenko/percolator@v13`

Pinned engine SHA: `6fc2276c854329c3b1d6227d2c569eefa4dc6c48`

## Current Retest Result

The branch now builds natively against the v13 engine with a dedicated v13
wrapper entrypoint at `src/v13_program.rs`.

Passing:

- `cargo check --release --lib`
- `cargo test --release -- --nocapture`
- `cargo kani --tests --output-format terse`

The old v12 integration tests are compiled out on this branch with
`#![cfg(any())]`; they target the removed global slab ABI and cannot be
mechanically reused. `V13_TEST_PORT_COVERAGE.md` tracks the retired v12 test
classes and the active v13 wrapper/engine coverage that replaces each class.
The replacement suite is:

- `tests/v13_wrapper.rs`: 35 native account-local wrapper tests
- `tests/v13_cu.rs`: 5 LiteSVM BPF wrapper/CU tests
- `tests/v13_kani.rs`: 8 wrapper ABI Kani proofs

`tests/v13_cu.rs` currently measures:

- init portfolio: 3,366 CU
- deposit: 14,761 CU
- withdraw: 22,082 CU
- top-up insurance: 12,792 CU
- resolve: 1,455 CU
- close resolved: 20,093 CU
- refresh crank: 8,988 CU
- recovery crank: 3,239 CU
- refresh crank before 64 extra portfolios: 8,986 CU
- refresh crank after 64 extra portfolios: 8,986 CU

It also verifies that BPF `Deposit`, `Withdraw`, `TopUpInsurance`, and
`CloseResolved` move real SPL Token balances in lockstep with `group.vault`,
user capital, resolved payout, and insurance. The v13 wrapper ABI now binds
markets to a collateral mint at `InitMarket`, validates user/vault token
accounts, and wraps public ledger mutations with SPL Token CPIs.

This confirms the wrapper crank path is account-local and does not scale with
materialized portfolio count. The v13 engine no longer has a global slab scan,
so the old dense 4096-account crank benchmark is not the relevant worst case.

`cargo build-sbf --no-default-features` exits successfully and the wrapper no
longer reports oversized stack frames after moving large runtime reads and init
construction off-stack. The SBF stack analyzer still reports two engine-crate
frames:

- `percolator::v13::MarketGroupV13::new`: estimated 13,632 bytes
- `percolator::v13::MarketGroupV13::execute_trade_with_fee_not_atomic`:
  estimated 13,120 bytes

The wrapper avoids calling `MarketGroupV13::new` on-chain, but the symbol is
still compiled in the dependency. `TradeNoCpi` still calls
`execute_trade_with_fee_not_atomic`; LiteSVM confirms the BPF trade path traps
with an access violation consistent with that engine stack warning. Crank CU
tests therefore seed no-position portfolio state through BPF and measure the
BPF crank path directly. Liquidation CU cannot be measured through the wrapper
until the engine exposes an SBF-safe trade/position setup path or removes the
large staged copies in `execute_trade_with_fee_not_atomic`.

Engine proof sweep status for the same SHA:

- Wrapper Kani proofs: PASS, 8/8.
- Engine `scripts/run_kani_full_audit.sh`: started against
  `/home/anatoly/percolator` at the same SHA. It produced PASS results through
  the early v13 harnesses but hit 10-minute timeouts on:
  - `proof_v13_bankrupt_liquidation_excludes_fee_from_residual_and_spends_insurance_once`
  - `proof_v13_funding_accrual_refresh_matches_sign_and_floor`

Those are proof-time blockers in the engine checkout under the requested
10-minute cap, not wrapper test failures.

## Original API Break

The initial `cargo check --release --lib` after moving the pin to v13 failed
architecturally, not as a small rename set:

- v12 exported a global slab engine: `RiskEngine`, `RiskParams`, indexed
  accounts, risk-buffer candidates, round-robin cursor progress, and global
  keeper request types.
- v13 exports an account-local model: `MarketGroupV13`,
  `PortfolioAccountV13`, `V13Config`, and per-account crank/trade/liquidation
  requests.
- v13 intentionally removes the old finite `MAX_ACCOUNTS` slab surface and the
  old global account array APIs.

The first compile pass reported 66 unresolved old-engine symbols, including:

- `RiskEngine`
- `RiskParams`
- `RiskError`
- `MAX_ACCOUNTS`
- `MAX_TOUCHED_PER_INSTRUCTION`
- `LiquidationPolicy`
- `PermissionlessProgressRequest`
- `PermissionlessProgressOutcome`
- `ResolvedCloseResult`
- `ResolveMode`
- `MarketMode`
- `SideMode`

## Wrapper Port Shape

The v13 program wrapper uses the account-local shape:

1. Replaced the single global slab account with a market-group account plus
   independently supplied portfolio accounts.
2. Replaced `InitUser` / `InitLP` indexed slots with portfolio account creation
   using `ProvenanceHeaderV13`.
3. Replaced global indexed user operations with account metas carrying the
   relevant `PortfolioAccountV13` objects.
4. Replaced keeper global scans/risk-buffer cursors with the v13
   `permissionless_crank_not_atomic` account-local API.
5. Replaced trade execution with `execute_trade_with_fee_not_atomic` over two
   explicit portfolio accounts and an effective-price array.
6. Replaced liquidation and resolved close handlers with the v13
   account-local liquidation and close APIs.
7. Rebuilt tests around account-local crank progress rather than global
   `MAX_ACCOUNTS` sweeps.

Do not implement a compatibility shim that recreates the v12 slab inside the
wrapper. That would defeat the v13 engine boundary and would be harder to audit
than an explicit v13 wrapper ABI.
