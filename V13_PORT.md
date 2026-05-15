# v13 Wrapper Port Notes

Engine branch tested: `aeyakovenko/percolator@v13`

Pinned engine SHA: `816cc22cb49df3cb8d8d063fcd06e1bd5d3eef9e`

## Current Retest Result

The branch now builds natively against the v13 engine with a dedicated v13
wrapper entrypoint at `src/v13_program.rs`.

Passing:

- `cargo check --release --lib`
- `cargo test --release --test v13_wrapper -- --nocapture`
- `cargo test --release -- --nocapture`

The old v12 integration tests are compiled out on this branch with
`#![cfg(any())]`; they target the removed global slab ABI and cannot be
mechanically reused. The replacement suite is `tests/v13_wrapper.rs`.

`cargo build-sbf --no-default-features` exits successfully, but the SBF stack
analyzer reports oversized frames:

- `percolator::v13::MarketGroupV13::new`: estimated 13,632 bytes
- `percolator::v13::MarketGroupV13::execute_trade_with_fee_not_atomic`:
  estimated 13,120 bytes
- wrapper handlers that read/write full v13 market or portfolio values by
  value, including `handle_init_market`, `handle_init_portfolio`,
  `handle_trade_nocpi`, `handle_top_up_insurance`, and `handle_resolve_market`

The wrapper frames need heap/in-place account decoding instead of large
by-value locals. The two engine frames require SBF-safe engine entrypoints or
removing large by-value staging inside the engine.

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
