# Crank CU Benchmark Results

LiteSVM benchmark for MAX_ACCOUNTS=4096, Solana max CU per tx: 1,400,000

## Summary: Post O(1) LP Aggregates Optimization

After optimizing LP aggregates (O(1) instead of O(N) scans), all scenarios fit well within CU limits:

| Scenario | Worst Crank CU | % of Limit | Status |
|----------|----------------|------------|--------|
| Baseline (LP only) | 16,324 | 1.2% | ✓ |
| 4095 dust accounts | 22,699 | 1.6% | ✓ |
| 4095 healthy w/positions | 39,726 | 2.8% | ✓ |
| 4095 w/50% crash (force_realize) | 54,663 | 3.9% | ✓ |
| 2048 force_realize closures | 48,622 | 3.5% | ✓ |

**All scenarios easily fit within the 1.4M CU limit.**

## Detailed Results

### Scenario 1: Empty Slots (LP only)
- **16,324 CU** baseline overhead for 4096 slots
- ~4 CU/slot

### Scenario 2: All Dust Accounts (no positions)
- 4095 users: **22,699 CU** total
- ~5 CU/account

### Scenario 3: Dust Account Scaling

| Users | CU Total | CU/User |
|-------|----------|---------|
| 100 | 18,824 | 188 |
| 500 | 22,699 | 45 |
| 1,000 | 22,699 | 22 |
| 2,000 | 22,699 | 11 |
| 4,000 | 22,699 | 5 |

**All 4000+ dust accounts fit in single tx at ~23K CU**

### Scenario 4: Healthy Accounts with Positions

| Users | CU per Crank | CU/User |
|-------|-------------|---------|
| 50 | 37,007 | 740 |
| 100 | 37,557 | 375 |
| 200 | 38,657 | 193 |
| 500 | 39,262 | 78 |
| 1,000 | 39,262 | 39 |

### Scenario 5: Force-Realize Closures (50% price crash)

| Closures | CU per Crank | CU/User |
|----------|-------------|---------|
| 100 | 46,082 | 460 |
| 200 | 47,182 | 235 |
| 500 | 47,787 | 95 |
| 1,000 | 47,787 | 47 |

### Scenario 8a: Full 4096 Sweep - Healthy Accounts

16 cranks for full sweep, testing worst single crank CU:

| Users | Worst Crank CU | % of Limit |
|-------|----------------|------------|
| 256 | 39,726 | 2.8% |
| 512 | 39,726 | 2.8% |
| 1,024 | 39,726 | 2.8% |
| 2,048 | 39,726 | 2.8% |
| 4,095 | 39,726 | 2.8% |

### Scenario 8b: Full Sweep with 50% Crash

16 cranks, triggers force_realize path (insurance=0):

| Users | Worst CU | % of Limit | Force-Realize |
|-------|----------|------------|---------------|
| 256 | 47,498 | 3.4% | 33 |
| 512 | 54,427 | 3.9% | 65 |
| 1,024 | 54,654 | 3.9% | 129 |
| 2,048 | 54,654 | 3.9% | 257 |
| 4,095 | 54,663 | 3.9% | 512 |

### Scenario 9: Worst-Case Force-Realize (2048 underwater users)

Designed to test maximum emergency unwind: 1 LP + 4095 users, half underwater.

| Crank | CU | Cumulative Closures |
|-------|-----|---------------------|
| 1 | 47,787 | 32 |
| 16 | 45,713 | 512 |
| 32 | 46,569 | 1,024 |
| 48 | 47,561 | 1,536 |
| 64 | 48,553 | 2,048 |

**Result: 48,622 CU worst crank (3.5%), 3.0M CU total across 64 cranks**

## Key Findings

1. **Baseline overhead**: ~16K CU (down from ~176K before O(1) optimization)
2. **Dust accounts**: All 4095 fit in single tx at ~23K CU
3. **Healthy accounts with positions**: ~40K CU per crank
4. **Force-realize emergency path**: ~48-55K CU per crank
5. **Full 4096 account unwind**: 64 cranks at ~3M CU total

## Architecture Notes

### Force-Realize Budget
- `FORCE_REALIZE_BUDGET_PER_CRANK = 32`
- 2048 underwater users require 64 cranks to fully unwind
- Each crank stays well under limit (~48K CU, 3.5%)

### Liquidation vs Force-Realize
- **Liquidation**: Surgical, only targets underwater accounts (below maintenance margin)
- **Force-Realize**: Emergency path when insurance depleted, closes all positions
- Note: Engine's margin check uses stored PnL (not mark-to-market), so unrealized losses from price crashes trigger force_realize path rather than per-account liquidation

### O(1) LP Aggregates Optimization
Replaced O(MAX_ACCOUNTS) loops in `compute_net_lp_pos` and `LpRiskState::compute` with O(num_used) bitmap iteration via `for_each_used_lp` helper.

**Before optimization**: Baseline scan was ~176K CU
**After optimization**: Baseline scan is ~16K CU (90% reduction)
