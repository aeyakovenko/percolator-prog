# Percolator - Solana Perpetual Futures Protocol

## Overview

Percolator is a perpetual futures protocol deployed on Solana. It consists of two programs:

- **Percolator Program**: Core risk engine and market management
- **vAMM Matcher**: Configurable LP pricing with spread, fees, and impact

## Deployed Programs (Devnet)

| Program | Address |
|---------|---------|
| Percolator | `46iB4ET4WpqfTXAqGSmyBczLBgVhd1sHre93KtU3sTg9` |
| vAMM Matcher | `4HcGCsyjAqnFua5ccuXyt8KRRQzKFbGTJkVChpS7Lfzy` |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Percolator Program                        │
├─────────────────────────────────────────────────────────────┤
│  Slab Account (Market State)                                │
│  ├── Header (magic, version, admin, nonce)                  │
│  ├── MarketConfig (mint, vault, oracle, params)             │
│  └── RiskEngine (accounts, positions, funding)              │
├─────────────────────────────────────────────────────────────┤
│  Vault (SPL Token Account)                                  │
│  └── Owned by vault PDA: ["vault", slab_pubkey]             │
└─────────────────────────────────────────────────────────────┘
         │
         │ TradeCpi
         ▼
┌─────────────────────────────────────────────────────────────┐
│                    vAMM Matcher Program                      │
├─────────────────────────────────────────────────────────────┤
│  Matcher Context (LP-owned)                                 │
│  ├── Mode: Passive (0) or Vamm (1)                          │
│  ├── Pricing: spread_bps + trading_fee_bps + impact         │
│  └── Limits: max_fill_abs, max_inventory_abs                │
└─────────────────────────────────────────────────────────────┘
```

## Key Features

### Market Modes

1. **Standard Mode**: Uses Pyth/Chainlink oracle for price
2. **Hyperp Mode**: Internal mark/index pricing (no external oracle)
   - `index_feed_id == [0u8; 32]` triggers Hyperp mode
   - Mark price from matcher exec_price (clamped)
   - Index price smoothed toward mark (rate-limited)
   - Premium-based funding: `(mark - index) / index`

### Oracle Options

1. **Pyth Oracle**: Pull-based price feeds with confidence filter
2. **Chainlink Oracle**: OCR2 price feeds
3. **Admin Oracle**: `SetOracleAuthority` + `PushOraclePrice` for testing/emergencies

### Matcher Modes

1. **Passive Mode (0)**: Fixed spread around oracle price
2. **vAMM Mode (1)**: Dynamic spread with inventory impact
   - `exec_price = oracle * (1 + spread_bps + fee_bps + impact_bps) / 10000`
   - Impact scales with trade size relative to liquidity

### Security Features

- **TradeNoCpi disabled for Hyperp**: Prevents mark price manipulation
- **Mark price clamping**: Circuit breaker limits price change per slot (default 1%)
- **Index smoothing**: Rate-limited movement toward mark
- **Nonce discipline**: Replay protection for matcher calls
- **LP PDA validation**: System-owned, zero data, zero lamports
- **Margin enforcement**: Initial (10%) and maintenance (5%) margins

## Instructions

| Tag | Instruction | Description |
|-----|-------------|-------------|
| 0 | InitMarket | Create market with slab + vault |
| 1 | InitUser | Register user account |
| 2 | InitLP | Register LP with matcher |
| 3 | DepositCollateral | Add collateral to account |
| 4 | WithdrawCollateral | Remove collateral (margin checked) |
| 5 | KeeperCrank | Maintenance: funding, fees, liquidations |
| 6 | TradeNoCpi | Direct trade (disabled for Hyperp) |
| 7 | TradeCpi | Trade via LP matcher |
| 8 | CloseAccount | Close and withdraw remaining capital |
| 9 | TopUpInsurance | Add to insurance fund |
| 10 | LiquidateAtOracle | Liquidate underwater account |
| 11 | UpdateAdmin | Rotate admin key |
| 12 | SetRiskThreshold | Set risk reduction threshold |
| 13 | CloseSlab | Close market (admin only) |
| 14 | SetFundingParams | Configure funding rate |
| 15 | SetMaintenanceFee | Set per-slot maintenance fee |
| 16 | SetOracleAuthority | Set admin oracle authority |
| 17 | PushOraclePrice | Push price (authority only) |
| 18 | SetOraclePriceCap | Set price circuit breaker cap |

## Testing

### Local Integration Tests (32 tests)
```bash
cargo test --test integration
```

### Devnet Test (requires funded wallet)
```bash
cargo test --test devnet_test -- --ignored --nocapture
```

### Build BPF
```bash
cargo build-sbf
```

## Risk Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| maintenance_margin_bps | 500 | 5% maintenance margin |
| initial_margin_bps | 1000 | 10% initial margin |
| oracle_price_cap_e2bps | 10,000 | 1% max price change per slot (Hyperp) |
| warmup_period_slots | 0 | Slots before PnL converts to capital |
| liquidation_fee_bps | 50 | 0.5% liquidation fee |
| liquidation_buffer_bps | 100 | 1% liquidation buffer |

## File Structure

```
percolator-prog/
├── src/
│   └── percolator.rs      # Main program (3500+ lines)
├── tests/
│   ├── integration.rs     # 32 LiteSVM tests
│   └── devnet_test.rs     # Live devnet test
├── README.md              # High-level documentation
├── verification.md        # Test matrix
├── audit.md               # Security audit notes
└── CLAUDE.md              # This file
```

## Common Tasks

### Initialize a Market
1. Create slab account (owner: program, size: SLAB_LEN)
2. Create vault token account (owner: vault PDA)
3. Call InitMarket with admin, mint, oracle config, risk params

### Set Up Trading
1. InitLP with matcher program and context
2. InitUser
3. Deposit collateral to both
4. For admin oracle: SetOracleAuthority + PushOraclePrice
5. Execute trades via TradeNoCpi or TradeCpi

### Run Maintenance
1. Call KeeperCrank periodically
2. Accrues funding, charges fees, processes liquidations
3. Updates risk threshold (if auto-threshold enabled)

## Security Checklist

- [ ] Never deploy with `--features devnet` on mainnet (disables oracle checks)
- [ ] Set appropriate oracle_price_cap_e2bps for Hyperp markets
- [ ] Validate matcher program is trusted before LP registration
- [ ] Monitor insurance fund balance vs risk threshold
- [ ] Run KeeperCrank frequently during high volatility

## Dependencies

- `solana-program = "1.18"`
- `spl-token = "4.0"`
- `pyth-sdk-solana = "0.10"`
- `percolator` (risk engine crate)
