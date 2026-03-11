# PERC-629: Dynamic Creation Deposit (Anti-Spam)

## Overview

Creation deposit scales with creator wallet history. Spam becomes exponentially expensive while good creators pay less.

## Mechanics

| Metric | Effect |
|--------|--------|
| Base deposit | $2,500 equivalent in collateral token |
| Each failed/dead market | Deposit multiplier doubles |
| Each successful market | Small reduction (10% off, min 50% of base) |
| Market never reaches 10% of deposit in OI within 30 days | 50% of deposit slashed to insurance |

## On-Chain Storage

### CreatorHistory PDA
Seeds: `["creator_history", creator_pubkey]`
Size: 48 bytes

```rust
struct CreatorHistory {
    magic: u64,          // "CRTRHIST"
    bump: u8,
    _pad: [u8; 7],
    creator: [u8; 32],   // redundant but useful for verification
    // — stored inline, not as separate fields, to keep PDA small —
    // Packed stats (8 bytes):
    total_markets: u16,     // total markets created
    successful_markets: u16, // markets that reached OI threshold
    failed_markets: u16,     // markets that failed OI threshold or died
    _stats_pad: u16,
}
```

Wait — 32 bytes for creator is redundant since it's in the PDA seed. Let me optimize:

### Revised: CreatorHistory PDA (32 bytes)
Seeds: `["creator_history", creator_pubkey]`

```rust
struct CreatorHistory {
    magic: u64,              // "CRTRHIST"
    bump: u8,
    _pad: [u8; 3],
    total_markets: u16,      // total markets ever created
    successful_markets: u16, // reached OI threshold
    failed_markets: u16,     // failed OI threshold or died
    _reserved: [u8; 14],
}
```
Total: 32 bytes.

## Pure Logic

### Deposit Calculation
```
multiplier = 2^failed_markets  (capped at 2^10 = 1024x)
discount = successful_markets * 10%  (capped at 50%)
effective_mult = max(multiplier * (1 - discount), 0.5)
required_deposit = base_deposit * effective_mult
```

### Slash Check (called by keeper/crank after 30 days)
```
if market_oi < deposit * 10% after 30 days:
    slash 50% of deposit to insurance fund
    increment creator.failed_markets
else:
    increment creator.successful_markets
```

## Constants
```rust
pub const CREATOR_HISTORY_MAGIC: u64 = 0x4352_5452_4849_5354; // "CRTRHIST"
pub const CREATOR_HISTORY_LEN: usize = 32;
pub const CREATOR_HISTORY_SEED: &[u8] = b"creator_history";
pub const BASE_DEPOSIT_E6: u64 = 2_500_000_000; // $2,500 in e6
pub const MAX_FAILURE_EXPONENT: u32 = 10; // max 1024x multiplier
pub const SUCCESS_DISCOUNT_BPS: u64 = 1_000; // 10% per success
pub const MAX_DISCOUNT_BPS: u64 = 5_000; // 50% max discount
pub const OI_THRESHOLD_BPS: u64 = 1_000; // 10% of deposit
pub const SLASH_BPS: u64 = 5_000; // 50% slash
pub const EVALUATION_PERIOD_SLOTS: u64 = 6_480_000; // ~30 days
```

## Kani Proofs
1. Deposit multiplier monotonically increases with failures
2. Discount bounded by MAX_DISCOUNT_BPS
3. Effective deposit >= base * 50% (floor)
4. Slash amount <= deposit (bounded)
5. Conservation: slash_to_insurance + remainder == deposit
