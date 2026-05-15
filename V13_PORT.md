# v13 Wrapper Port Notes

Engine branch tested: `aeyakovenko/percolator@v13`

Pinned engine SHA: `816cc22cb49df3cb8d8d063fcd06e1bd5d3eef9e`

## Retest Result

`cargo check --release --lib` does not compile after the engine pin is moved to
v13. The failure is architectural, not a small rename set:

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

## Required Wrapper Port

The v13 program wrapper needs a real account-layout and ABI port:

1. Replace the single global slab account with a market-group account plus
   independently supplied portfolio accounts.
2. Replace `InitUser` / `InitLP` indexed slots with portfolio account creation
   using `ProvenanceHeaderV13`.
3. Replace global indexed user operations with account metas carrying the
   relevant `PortfolioAccountV13` objects.
4. Replace keeper global scans/risk-buffer cursors with the v13
   `permissionless_crank_not_atomic` account-local API.
5. Replace trade execution with `execute_trade_with_fee_not_atomic` over two
   explicit portfolio accounts and an effective-price array.
6. Replace liquidation and resolved close handlers with the v13
   account-local liquidation and close APIs.
7. Rebuild tests around account-local crank progress rather than global
   `MAX_ACCOUNTS` sweeps.

Do not implement a compatibility shim that recreates the v12 slab inside the
wrapper. That would defeat the v13 engine boundary and would be harder to audit
than an explicit v13 wrapper ABI.
