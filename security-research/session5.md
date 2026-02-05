# Security Research Log - 2026-02-05 Session 5

## Continued Systematic Search

### Areas Verified This Session

#### 1. SetRiskThreshold ✓
**Location**: `percolator-prog/src/percolator.rs:3290-3307`
**Status**: SECURE

- Admin-only (require_admin check)
- No parameter validation needed (any u128 is valid)
- Used to control force-realize mode activation

#### 2. Force-Realize Mode ✓
**Location**: `percolator/src/percolator.rs:1453-1458, 1610-1628`
**Status**: SECURE

- Triggers when `insurance_fund.balance <= risk_reduction_threshold`
- Uses touch_account_for_force_realize (best-effort fee settle)
- Closes positions at oracle price
- Maintains lifetime counter with saturating_add

#### 3. Partial Position Close ✓
**Location**: `percolator/src/percolator.rs:1792-1871`
**Status**: SECURE

- Zero checks for close_abs and current_abs_pos
- Falls back to full close when close_abs >= current_abs_pos
- Uses checked_mul/checked_div with fallback
- Maintains OI and LP aggregates
- Settles warmup and writes off negative PnL

#### 4. Entry Price Updates ✓
**Location**: Multiple (1906, 2258, 2278, 2292, 2308, 3022, 3028)
**Status**: SECURE

- Entry price always set to oracle_price after mark settlement
- Ensures mark_pnl = 0 at settlement price
- Consistent across all settlement paths

#### 5. LiquidateAtOracle Instruction ✓
**Location**: `percolator-prog/src/percolator.rs:3123-3178`
**Status**: SECURE

- Permissionless (anyone can liquidate underwater accounts)
- Uses check_idx before account access
- Proper oracle price retrieval and circuit breaker

## Running Verification

All 57 integration tests pass.

#### 6. Dust Sweeping ✓
**Location**: `percolator-prog/src/percolator.rs:2752-2812`
**Status**: SECURE

- Dust accumulated from base_to_units remainders
- Swept to insurance fund when accumulated >= unit_scale
- CloseSlab checks dust_base != 0 (Bug #3 fix)
- Uses saturating_add for accumulation

#### 7. Unit Scale Handling ✓
**Location**: `percolator-prog/src/percolator.rs:729-736`
**Status**: SECURE

- scale_price_e6 rejects zero result
- MAX_UNIT_SCALE = 1 billion
- Withdrawal rejects misaligned amounts
- Ensures oracle values match capital scale

#### 8. Warmup Reset ✓
**Location**: `percolator/src/percolator.rs:2809-2813, 3084-3085`
**Status**: SECURE

- update_warmup_slope called when avail_gross increases
- Called after trade for both parties
- Resets warmup_started_at_slot to current_slot
- Slope = avail_gross / warmup_period_slots (min=1 if avail>0)

#### 9. Funding Index Overflow ✓
**Location**: `percolator/src/percolator.rs:2144-2147`
**Status**: SECURE

- Uses checked_add for funding_index_qpb_e6 update
- Returns Overflow error on overflow
- Rate capped at ±10,000 bps/slot
- dt capped at ~1 year (31,536,000 slots)

#### 10. Fee Credits ✓
**Location**: `percolator/src/percolator.rs:1049-1067`
**Status**: SECURE

- Starts at 0, deducted by maintenance fees
- Can go negative (fees owed)
- Paid from capital when negative
- Uses saturating_sub/saturating_add
- Forgiven on close_account (Finding C fix)

#### 11. Reserved PnL ✓
**Location**: `percolator/src/percolator.rs:119, 2039, 2064`
**Status**: SECURE

- Subtracted from positive PnL to get available gross
- Must be zero for GC
- Uses saturating_sub for safety
- Prevents claiming reserved PnL early

#### 12. CloseAccount ✓
**Location**: `percolator/src/percolator.rs:1261-1324`
**Status**: SECURE

- Full settlement via touch_account_full
- Position must be zero
- Fee debt forgiven (Finding C fix)
- PnL must be exactly 0
- Capital verified against vault
- c_tot updated before free_slot

## Session 5 Summary

**Additional Areas Verified**: 12
**New Vulnerabilities Found**: 0
**Test Status**: All 57 integration tests pass

The systematic search continues to find no new vulnerabilities. The codebase demonstrates comprehensive security measures across all reviewed areas.

## Continued Exploration

#### 13. Unsafe Code Containment ✓
**Location**: `percolator-prog/src/percolator.rs:4, 819-858`
**Status**: SECURE

- `#![deny(unsafe_code)]` at top level
- Only `mod zc` has `#[allow(unsafe_code)]`
- Proper length and alignment checks before pointer operations
- Lifetime soundness documented for invoke_signed_trade

#### 14. Pyth Oracle Parsing ✓
**Location**: `percolator-prog/src/percolator.rs:1615-1698`
**Status**: SECURE

- Owner validation (PYTH_RECEIVER_PROGRAM_ID)
- Feed ID validation
- Price > 0 check
- Exponent bounded (MAX_EXPO_ABS)
- Staleness check (disabled on devnet)
- Confidence check (disabled on devnet)
- Overflow check on multiplication
- Zero price rejection
- u64::MAX overflow check

#### 15. Chainlink Oracle Parsing ✓
**Location**: `percolator-prog/src/percolator.rs:1710-1787`
**Status**: SECURE

- Owner validation (CHAINLINK_OCR2_PROGRAM_ID)
- Feed pubkey validation
- Answer > 0 check
- Decimals bounded (MAX_EXPO_ABS)
- Staleness check (disabled on devnet)
- Overflow check on multiplication
- Zero price rejection
- u64::MAX overflow check

#### 16. Oracle Authority (Admin Oracle) ✓
**Location**: `percolator-prog/src/percolator.rs:3450-3508`
**Status**: SECURE

SetOracleAuthority:
- Admin-only
- Clears stored price when authority changes
- Zero authority = disabled

PushOraclePrice:
- Verifies caller == oracle_authority
- Authority must be non-zero
- Price must be positive
- Circuit breaker applied (clamp_oracle_price)
- Updates both authority_price_e6 and last_effective_price_e6

#### 17. SetOraclePriceCap ✓
**Location**: `percolator-prog/src/percolator.rs:3510-3528`
**Status**: SECURE

- Admin-only
- No validation needed (any u64 is valid; 0 = disabled)

## Continued Session 5 Exploration (Part 2)

#### 18. TopUpInsurance ✓
**Location**: `percolator-prog/src/percolator.rs:3255-3289`
**Status**: SECURE

- Permissionless (anyone can top up, intentional design)
- Token transfer via deposit() happens first
- Base-to-units conversion with dust accumulation
- Engine's top_up_insurance_fund uses saturating add_u128
- Updates both vault and insurance_fund.balance

#### 19. InitUser/InitLP Account Creation ✓
**Location**: `percolator/src/percolator.rs:893-945, 948-1006`
**Status**: SECURE

- O(1) counter check (num_used_accounts) - fixes H2 TOCTOU
- Fee requirement enforced (required_fee check)
- Excess payment credited to user capital (Bug #4 fix)
- Vault updated with full fee_payment, insurance with required_fee
- next_account_id incremented with saturating_add
- c_tot updated when excess > 0
- LP also stores matcher_program and matcher_context

#### 20. UpdateAdmin ✓
**Location**: `percolator-prog/src/percolator.rs:3309-3326`
**Status**: SECURE

- Admin-only via require_admin
- Simple header.admin update
- No additional validation needed

#### 21. CloseSlab ✓
**Location**: `percolator-prog/src/percolator.rs:3328-3377`
**Status**: SECURE

- Admin-only via require_admin
- Requires: vault=0, insurance_fund.balance=0, num_used_accounts=0
- Bug #3 fix: checks dust_base != 0
- Zeros out slab data to prevent reuse
- Lamports transfer with checked_add
- **WARNING**: unsafe_close feature skips ALL validation

#### 22. UpdateConfig ✓
**Location**: `percolator-prog/src/percolator.rs:3379-3429`
**Status**: SECURE

- Admin-only via require_admin
- Parameter validation:
  - funding_horizon_slots != 0
  - funding_inv_scale_notional_e6 != 0
  - thresh_alpha_bps <= 10,000
  - thresh_min <= thresh_max

#### 23. SetMaintenanceFee ✓
**Location**: `percolator-prog/src/percolator.rs:3431-3448`
**Status**: SECURE

- Admin-only via require_admin
- No param validation needed (any fee valid)
- Direct update to engine.params.maintenance_fee_per_slot

#### 24. check_idx Validation Pattern ✓
**Location**: `percolator-prog/src/percolator.rs:2188-2193`
**Status**: SECURE

Pattern consistently applied:
- Bounds check: `idx as usize >= MAX_ACCOUNTS`
- Used check: `!engine.is_used(idx as usize)`
- Called BEFORE account access in:
  - DepositCollateral (line 2522)
  - WithdrawCollateral (line 2575)
  - KeeperCrank self-crank (line 2710)
  - TradeNoCpi (lines 2849, 2850)
  - TradeCpi (lines 2965, 2966)
  - LiquidateAtOracle (line 3150)
  - CloseAccount (line 3222)

## Session 5 Final Summary (Updated)

**Total Areas Verified This Session**: 24
**New Vulnerabilities Found**: 0
**All 57 Integration Tests**: PASS

The codebase continues to demonstrate strong security practices with comprehensive validation, authorization, overflow protection, and proper error handling across all 24 additional areas reviewed.
