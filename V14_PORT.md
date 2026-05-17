# v14 Wrapper Port Notes

Engine branch tested: `aeyakovenko/percolator@v14`

Pinned engine SHA: `1ed807e387738674471970d9dcc3d95da8f68659`

## Current Retest Result

The branch now builds natively against the v14 engine with a dedicated v14
wrapper entrypoint at `src/v14_program.rs`.

Passing:

- `cargo check --release --lib`
- `cargo test --release -- --nocapture`
- `cargo kani --tests --output-format terse`

The old v12 integration tests are compiled out on this branch with
`#![cfg(any())]`; they target the removed global slab ABI and cannot be
mechanically reused. `V14_TEST_PORT_COVERAGE.md` tracks the retired v12 test
classes and the active v14 wrapper/engine coverage that replaces each class.
The replacement suite is:

- `tests/v14_wrapper.rs`: 86 native account-local wrapper tests
- `tests/v14_cu.rs`: 12 LiteSVM BPF wrapper/CU tests
- `tests/v14_kani.rs`: 16 wrapper ABI Kani proofs

`tests/v14_cu.rs` currently measures:

- init portfolio: 14,814 CU
- deposit: 28,268 CU
- withdraw: 35,742 CU
- top-up insurance: 21,449 CU
- withdraw insurance: 22,220 CU
- resolve: 11,617 CU
- configure permissionless stale resolve: 11,196 CU
- permissionless stale resolve: 11,448 CU
- close resolved: 33,769 CU
- liquidation crank: 71,730 CU
- TradeNoCpi: 80,469 CU
- TradeCpi: 101,013 CU
- refresh crank: 24,529 CU
- refresh crank before 64 extra portfolios: 24,527 CU
- refresh crank after 64 extra portfolios: 24,529 CU

It also verifies that BPF `Deposit`, `Withdraw`, `TopUpInsurance`,
`WithdrawInsuranceLimited`, `CloseResolved`, `TradeNoCpi`, `TradeCpi`,
permissionless liquidation, permissionless stale resolution, authenticated
permissionless crank time, and authenticated hybrid-oracle configuration time
move, bound, or transition real SPL Token and market state in lockstep with
`group.vault`, user capital, resolved payout, insurance, and resolved-market
mode. The v14 wrapper
ABI now binds markets to a collateral mint at `InitMarket`, validates
user/vault token accounts, and wraps public ledger mutations with SPL Token
CPIs.

The v12 `UpdateAuthority` tag is restored for the v14-backed authority fields:
admin, insurance authority, and insurance operator. Nonzero handoffs require
both the current authority and destination key to sign; scoped zero-key burns
are allowed for insurance roles. Live admin burn is rejected until a
permissionless resolved-exit profile exists. The old Hyperp mark authority kind
is rejected until Hyperp wrapper configuration is ported.

The v12 `CloseSlab` terminal lifecycle path is restored as an account-local
market close. It requires the admin destination to sign, the market to be
resolved, engine vault/insurance/capital accounting to be zero, and
`materialized_portfolio_count == 0`; it then closes the supplied SPL vault and
zeros/rent-drains the market account.

The v12 bounded insurance withdrawal surface is restored as
`WithdrawInsuranceLimited`. It is gated by the configured insurance operator,
allows live withdrawal only while the market is healthy and not in h-lock,
stress, loss-stale, active-bankrupt, or recovery state, and allows terminal
resolved withdrawal only after all portfolio claims/capital are closed. The
handler enforces the wrapper insurance policy (`max_bps`, optional
deposit-only budget, and cooldown), debits group insurance/vault accounting,
asserts public senior invariants, then moves SPL tokens from the vault PDA.
`UpdateInsurancePolicy` is admin-gated; deposit-only budgets increase only from
later `TopUpInsurance` calls and are decremented by live withdrawals so fee
growth stays behind once explicit top-up principal is withdrawn.

The v14 wrapper also exposes `UpdateLiquidationFeePolicy`, an admin-gated
market policy for splitting retained liquidation penalties between insurance
and the crank submitter. The engine remains the source of truth for the total
liquidation fee charged by a liquidation. The wrapper computes the cranker
reward only from the fee amount that actually increased insurance after the
liquidation, so bankrupt/liquidation-loss paths cannot pay rewards out of
protective insurance needed for losses. A `cranker_share_bps` of `4_000` means
40% of the retained liquidation penalty exits custody to the submitter and 60%
stays in insurance.

The v12 hybrid after-hours/Toto oracle surface is restored as
`ConfigureHybridOracle` plus hybrid-aware `PermissionlessCrank` and trade
handling. The wrapper supports one to three Pyth Pull `PriceUpdateV2` legs, for
example `TOTO/SOL = TOTO/JPY / USD/JPY / SOL/USD`. Fresh external composites
own the index and pin the fallback EWMA mark to the accepted effective price.
After the market's soft-stale slot window elapses, the same trade surfaces
remain live at any consenting execution price; trades charge the static base
fee plus the dynamic mark-movement externality floor and update the fallback
EWMA. Caller-selected crank recovery is not exposed by the wrapper because a
recovery reason alone is not a proof. Hard-stale markets exit through the native
`ResolveStalePermissionless` path described below.

The v14 wrapper now has a native stale-oracle public exit instead of the old
v12 resolve ABI. `ConfigurePermissionlessResolve` is admin-gated and records
the hard-stale slot window plus the resolved-close policy knob required before
admin burn. `ResolveStalePermissionless` is permissionless and takes only the
market account plus an authenticated slot argument used as the host-test
fallback for `Clock`; it never consumes caller-supplied oracle accounts. The
stale proof is therefore the market's stamped `last_good_oracle_slot` plus the
configured hard-stale window, not a chosen stale or confidence-wide Pyth
account. Once resolved, the existing v14 `CloseResolved` path remains
permissionless but pays only to the portfolio owner's token account.

The v12 released-PnL conversion surface is restored as `ConvertReleasedPnl`.
The v14 engine conversion primitive converts the released residual-bounded
amount in one call, so the wrapper treats the instruction amount as a maximum:
if the released amount would exceed the caller's cap, the instruction fails
without persisting the staged engine mutation. Resolved markets still use the
terminal `CloseResolved` payout path instead of live conversion.

`InitMarket` now exposes the full v14 public-fund engine envelope instead of
hardcoding the non-default fields in the wrapper. The engine remains the source
of truth for config validation; the wrapper decodes the exact wire fields,
builds `V14Config`, and only persists the market if `validate_public_user_fund`
accepts the shape.

`TradeNoCpi` restores the v12 static-fee floor as a wrapper-owned base fee:
the executed fee is `max(caller_fee_bps, trade_fee_base_bps)`, and init rejects
a base fee above the engine's `max_trading_fee_bps`. Dynamic-fee callers can
still submit a higher fee, while zero-fee callers cannot bypass a configured
base fee.

This confirms the wrapper crank path is account-local and does not scale with
materialized portfolio count. The v14 engine no longer has a global slab scan,
so the old dense 4096-account crank benchmark is not the relevant worst case.

`cargo build-sbf --no-default-features` exits successfully with no SBF stack
analyzer errors. The engine pin includes an SVM-safe in-place trade entrypoint
for Solana builds and gates the large native staging helpers out of SBF. The
wrapper uses the staged engine API in native tests, but switches to the
in-place API under `target_os = "solana"` where SVM rollback supplies
instruction atomicity.

LiteSVM executes `TradeNoCpi`, `TradeCpi`, liquidation, custody, resolution,
and stale-oracle permissionless resolution through the BPF program. The
account-local crank CU test confirms refresh cost does not scale with unrelated
materialized portfolio count.

Engine proof sweep status for the same SHA:

- Wrapper Kani proofs: PASS, 16/16.
- Engine `scripts/run_kani_full_audit.sh`: started against
  `/home/anatoly/percolator` at the same SHA. It produced PASS results through
  the early v14 harnesses but hit 10-minute timeouts on:
  - `proof_v14_bankrupt_liquidation_excludes_fee_from_residual_and_spends_insurance_once`
  - `proof_v14_funding_accrual_refresh_matches_sign_and_floor`

Those are proof-time blockers in the engine checkout under the requested
10-minute cap, not wrapper test failures.

## Original API Break

The initial `cargo check --release --lib` after moving the pin to v14 failed
architecturally, not as a small rename set:

- v12 exported a global slab engine: `RiskEngine`, `RiskParams`, indexed
  accounts, risk-buffer candidates, round-robin cursor progress, and global
  keeper request types.
- v14 exports an account-local model: `MarketGroupV14`,
  `PortfolioAccountV14`, `V14Config`, and per-account crank/trade/liquidation
  requests.
- v14 intentionally removes the old finite `MAX_ACCOUNTS` slab surface and the
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

The v14 program wrapper uses the account-local shape:

1. Replaced the single global slab account with a market-group account plus
   independently supplied portfolio accounts.
2. Replaced `InitUser` / `InitLP` indexed slots with portfolio account creation
   using `ProvenanceHeaderV14`.
3. Replaced global indexed user operations with account metas carrying the
   relevant `PortfolioAccountV14` objects.
4. Replaced keeper global scans/risk-buffer cursors with the v14
   `permissionless_crank_not_atomic` account-local API.
5. Replaced trade execution with `execute_trade_with_fee_not_atomic` over two
   explicit portfolio accounts and an effective-price array.
6. Replaced liquidation and resolved close handlers with the v14
   account-local liquidation and close APIs.
7. Rebuilt tests around account-local crank progress rather than global
   `MAX_ACCOUNTS` sweeps.

Do not implement a compatibility shim that recreates the v12 slab inside the
wrapper. That would defeat the v14 engine boundary and would be harder to audit
than an explicit v14 wrapper ABI.
