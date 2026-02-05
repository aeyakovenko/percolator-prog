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

## Continued Session 5 Exploration (Part 3)

#### 25. verify Module Pure Helpers ✓
**Location**: `percolator-prog/src/percolator.rs:228-328`
**Status**: SECURE

All Kani-provable helpers:
- `owner_ok`: Simple equality, stored == signer
- `admin_ok`: Non-zero check + equality (prevents zero-address bypass)
- `matcher_identity_ok`: Both program and context must match
- `matcher_shape_ok`: Program executable, context not, context owned by program
- `gate_active`: threshold > 0 AND balance <= threshold
- `nonce_on_success`: wrapping_add(1) for replay protection
- `cpi_trade_size`: ALWAYS uses exec_size, never requested_size

#### 26. keeper_crank Logic ✓
**Location**: `percolator/src/percolator.rs:1483-1679`
**Status**: SECURE

- Funding accrual uses STORED rate (anti-retroactivity)
- Caller settlement with 50% discount (best-effort)
- Crank cursor iteration: processes up to ACCOUNTS_PER_CRANK
- Per-account operations:
  - Maintenance fee settle (best-effort)
  - Touch account + warmup settle
  - Liquidation (if not in force-realize mode, budget limited)
  - Force-close for zero equity or dust positions
  - Force-realize (when insurance <= threshold)
  - LP max tracking
- Sweep completion detection: wraps around to sweep_start_idx
- Garbage collection after crank

#### 27. check_conservation ✓
**Location**: `percolator/src/percolator.rs:3238-3308`
**Status**: SECURE

- Computes total_capital via for_each_used
- Computes net_pnl with funding settlement simulation
- Computes net_mark for mark-to-market PnL
- Primary invariant: vault >= C_tot + I
- Extended invariant: vault >= capital + (settled_pnl + mark_pnl) + insurance
- Bounded slack (MAX_ROUNDING_SLACK) allowed for rounding

#### 28. Liquidation Logic ✓
**Location**: `percolator/src/percolator.rs:1689-1777`
**Status**: SECURE

mark_pnl_for_position:
- Zero position returns 0
- Longs profit when oracle > entry
- Shorts profit when entry > oracle
- Uses checked_mul/checked_div for overflow

compute_liquidation_close_amount:
- Deterministic closed-form calculation (no iteration)
- Target margin = maintenance + buffer
- Conservative rounding guard (subtracts 1 unit)
- Dust kill-switch: full close if remaining < min_liquidation_abs

#### 29. oracle_close_position_core ✓
**Location**: `percolator/src/percolator.rs:1879-1927`
**Status**: SECURE

- Zero position returns early
- mark_pnl overflow → wipes capital (conservative/safe)
- Uses set_pnl to maintain pnl_pos_tot aggregate
- Closes position, sets entry_price = oracle_price
- Updates OI and LP aggregates
- Settles warmup and writes off negative PnL (spec §6.1)

#### 30. oracle_close_position_slice_core ✓
**Location**: `percolator/src/percolator.rs:1792-1871`
**Status**: SECURE

- Falls back to full close if close_abs >= current_abs_pos
- Computes proportional mark_pnl for closed slice
- Uses checked_mul/checked_div with fallback
- Updates position while maintaining sign
- Updates OI and LP aggregates correctly
- Entry price unchanged (correct for partial reduction)

## Continued Session 5 Exploration (Part 4)

#### 31. execute_trade ✓
**Location**: `percolator/src/percolator.rs:2686-3088`
**Status**: SECURE

Comprehensive security:
- Timing guards: require_fresh_crank, require_recent_full_sweep
- Input validation: indices, oracle bounds, size bounds
- Account kind validation (LP vs User)
- Matcher output validation: price/size bounds, direction, partial fill
- Touch accounts BEFORE position changes
- Mark settlement (settle_mark_to_oracle) before trade
- Maintenance fee settlement
- Fee calculation with ceiling division (prevents micro-trade evasion)
- Position calculation with checked_add/checked_sub
- Trade PnL with checked_mul/checked_div
- **Projected haircut** for margin checks (post-trade pnl_pos_tot)
- Margin checks: initial for risk-increasing (incl. flips), maintenance otherwise
- State commit: fee to insurance, aggregates (c_tot, pnl_pos_tot, OI, LP aggregates)
- **Two-pass settlement** (Finding G fix): losses first, then profits

#### 32. settle_warmup_to_capital ✓
**Location**: `percolator/src/percolator.rs:3125-3200`
**Status**: SECURE

§6.1 Loss settlement:
- Negative PnL pays from capital (min of need, capital)
- Uses set_capital/set_pnl to maintain aggregates
- Remaining negative PnL written off (spec §6.1 step 4)

§6.2 Profit conversion:
- Computes avail_gross = positive_pnl - reserved
- Warmable cap = slope × elapsed
- x = min(avail_gross, cap)
- **Haircut computed BEFORE modifying state**
- y = floor(x × h_num / h_den)
- Reduce PnL by x, increase capital by y
- Advance warmup time base, recompute slope (min=1 when avail>0)

#### 33. set_pnl / set_capital ✓
**Location**: `percolator/src/percolator.rs:772-795`
**Status**: SECURE

O(1) aggregate maintenance:
- set_pnl: Updates pnl_pos_tot (only positive PnL counts)
- set_capital: Updates c_tot (delta-based update)
- All code paths modifying PnL/capital MUST use these helpers

## Continued Session 5 Exploration (Part 5)

#### 34. touch_account ✓
**Location**: `percolator/src/percolator.rs:2222-2241`
**Status**: SECURE

- Validates account exists
- Calls settle_account_funding
- Updates pnl_pos_tot aggregate for funding settlement

#### 35. settle_account_funding ✓
**Location**: `percolator/src/percolator.rs:2180-2219`
**Status**: SECURE

- delta_f = global_fi - account.funding_index (checked_sub)
- payment = position × delta_f / 1e6 (checked_mul/checked_div)
- Rounds UP for positive payments (account pays)
- Truncates for negative (account receives)
- Updates pnl and funding_index

#### 36. settle_mark_to_oracle ✓
**Location**: `percolator/src/percolator.rs:2251-2280`
**Status**: SECURE

- Zero position: sets entry = oracle for determinism
- Computes mark_pnl via mark_pnl_for_position (checked math)
- Realizes mark PnL via set_pnl (maintains aggregate)
- Sets entry_price = oracle_price
- Best-effort variant uses saturating_add (for liquidation paths)

#### 37. accrue_funding ✓
**Location**: `percolator/src/percolator.rs:2106-2151`
**Status**: SECURE

- dt = 0 returns early (no double-accrual)
- Oracle price validation (0 < price <= MAX_ORACLE_PRICE)
- Uses STORED rate (anti-retroactivity guarantee)
- Rate capped at ±10,000 bps/slot
- dt capped at ~1 year (31,536,000 slots)
- ΔF = price × rate × dt / 10,000 with checked_mul/checked_div
- funding_index_qpb_e6 updated with checked_add (returns Overflow on fail)

#### 38. touch_account_full ✓
**Location**: `percolator/src/percolator.rs:2316-2356`
**Status**: SECURE

Lazy settlement pipeline:
1. Settle funding (touch_account)
2. Settle mark-to-oracle (with warmup reset if AvailGross increases)
3. Settle maintenance fees
4. Settle warmup (convert PnL to capital)
5. Sweep fee debt from capital

## Continued Session 5 Exploration (Part 6)

#### 39. WithdrawCollateral (wrapper) ✓
**Location**: `percolator-prog/src/percolator.rs:2534-2612`
**Status**: SECURE

- Signer/writable/slab_guard checks
- PDA derivation and validation
- Oracle price read (Hyperp or circuit-breaker clamped)
- check_idx + owner_ok authorization
- Rejects misaligned withdrawals (unit_scale)
- Calls engine.withdraw() for margin check
- Token transfer via collateral::withdraw

#### 40. engine.withdraw ✓
**Location**: `percolator/src/percolator.rs:2456-2574`
**Status**: SECURE

Timing guards:
- require_fresh_crank
- require_recent_full_sweep

Validations:
- Oracle price bounds (0 < price <= MAX)
- Account exists
- touch_account_full for settlement
- Sufficient capital check

MTM equity with haircut:
- Uses effective_pos_pnl (haircut-adjusted)
- Adds mark_pnl from mark_pnl_for_position
- Overflow defaults to 0 equity (fail-safe)
- Subtracts fee debt

Margin checks:
- **Pre-withdrawal**: Initial margin check
- **Post-withdrawal**: Maintenance margin check
- If post-check fails, REVERTS via set_capital

#### 41. engine.deposit ✓
**Location**: `percolator/src/percolator.rs:2396-2452`
**Status**: SECURE

- Updates current_slot
- Validates account exists
- Calculates and settles accrued fees
- Pays owed fees from deposit first
- Vault gets full deposit
- Capital gets remainder via set_capital
- Calls settle_warmup_to_capital
- Calls pay_fee_debt_from_capital

## Continued Session 5 Exploration (Part 7)

#### 42. haircut_ratio ✓
**Location**: `percolator/src/percolator.rs:816-828`
**Status**: SECURE

- Returns (1, 1) when pnl_pos_tot == 0 (no haircut)
- residual = vault - c_tot - insurance (saturating_sub)
- h_num = min(residual, pnl_pos_tot)
- Returns (h_num, pnl_pos_tot) for ratio calculation

#### 43. effective_pos_pnl ✓
**Location**: `percolator/src/percolator.rs:833-844`
**Status**: SECURE

- Returns 0 for negative or zero PnL
- Calls haircut_ratio
- Returns full pos_pnl if h_den == 0
- Otherwise: floor(pos_pnl × h_num / h_den)

#### 44. effective_equity ✓
**Location**: `percolator/src/percolator.rs:849-861`
**Status**: SECURE

Implements: Eq_real_i = max(0, C_i + min(PNL_i, 0) + PNL_eff_pos_i)
- cap_i = capital (as signed for math)
- neg_pnl = min(pnl, 0) - only negative PnL counts against equity
- eff_pos = effective_pos_pnl(pnl) - haircutted positive PnL
- Returns max(0, cap_i + neg_pnl + eff_pos)

## Session 5 Final Summary (Updated)

**Total Areas Verified This Session**: 48
**New Vulnerabilities Found**: 0
**All 57 Integration Tests**: PASS

The codebase continues to demonstrate strong security practices with comprehensive validation, authorization, overflow protection, and proper error handling across all 44 additional areas reviewed.

## Session 5 Complete Inventory

### Wrapper Program Areas (percolator-prog/src/percolator.rs)
1. SetRiskThreshold ✓
2. LiquidateAtOracle ✓
3. TopUpInsurance ✓
4. UpdateAdmin ✓
5. CloseSlab ✓
6. UpdateConfig ✓
7. SetMaintenanceFee ✓
8. check_idx validation ✓
9. verify module helpers ✓
10. WithdrawCollateral ✓
11. Unsafe code containment ✓
12. Pyth/Chainlink/Admin oracle ✓

### Engine Areas (percolator/src/percolator.rs)
1. Force-realize mode ✓
2. Partial position close ✓
3. Entry price updates ✓
4. Dust sweeping ✓
5. Unit scale handling ✓
6. Warmup reset ✓
7. Funding index overflow ✓
8. Fee credits ✓
9. Reserved PnL ✓
10. CloseAccount ✓
11. keeper_crank ✓
12. check_conservation ✓
13. Liquidation logic ✓
14. oracle_close_position_core ✓
15. execute_trade ✓
16. settle_warmup_to_capital ✓
17. set_pnl/set_capital ✓
18. touch_account ✓
19. settle_account_funding ✓
20. settle_mark_to_oracle ✓
21. accrue_funding ✓
22. touch_account_full ✓
23. engine.withdraw ✓
24. engine.deposit ✓
25. haircut_ratio ✓
26. effective_pos_pnl ✓
27. effective_equity ✓
28. add_user/add_lp ✓
29. InitUser/InitLP ✓
30. Position flip margin selection ✓
31. Risk reduction mode ✓
32. Two-pass settlement (Finding G fix) ✓

## Additional Deep Dive: TradeCpi Security ✓

#### 45. TradeCpi Full Path ✓
**Location**: `percolator-prog/src/percolator.rs:2899-3120`
**Status**: SECURE

Pre-CPI validation:
- Account layout (8 accounts)
- User signer, slab/matcher_ctx writable
- Matcher shape (prog executable, ctx not, ctx owned by prog)
- LP PDA derivation + shape (system-owned, empty, 0 lamports)
- check_idx for LP and user
- Owner authorization for both
- Matcher identity binding (LP's registered matcher)

CPI execution:
- Slab NOT passed to CPI (prevents reentrancy)
- LP PDA becomes signer via invoke_signed
- Only passes lp_pda + matcher_ctx

Post-CPI validation:
- ABI validation: req_id, lp_account_id, oracle_price_e6 must match
- exec_size used (not requested size) - Kani-proven
- Risk reduction gate uses actual exec_size
- State modified AFTER CPI returns
- Nonce written AFTER execute_trade

#### 46. free_slot ✓
**Location**: `percolator/src/percolator.rs:1329-1335`
**Status**: SECURE

- Clears account (empty_account())
- Clears bitmap (clear_used)
- Returns slot to free list (free_head)
- Decrements num_used_accounts with saturating_sub

#### 47. garbage_collect_dust ✓
**Location**: `percolator/src/percolator.rs:1351-1425`
**Status**: SECURE

- **NEVER GCs LPs** (critical for market operation)
- Dust predicate: position=0, capital=0, reserved=0, pnl<=0
- Best-effort fee settle before dust check
- Funding snap for flat positions
- Negative PnL write-off via set_pnl
- Cursor advancement with masking
- Budget-limited (GC_CLOSE_BUDGET)

#### 48. I128/U128 BPF-Safe Types ✓
**Location**: `percolator/src/i128.rs`
**Status**: SECURE

- BPF alignment safety: [u64; 2] for consistent 8-byte alignment
- Saturating arithmetic: All operators use saturating_* (no panic/wrap)
- Checked operations: checked_add/sub/mul/div return Option
- Kani optimization: Transparent newtypes for verification
- Sign handling: is_negative checks high bit of hi word

## Known Open Issue

**Bug #9**: Hyperp index smoothing bypass (clamp_toward_with_dt returns mark when dt=0)
- **Status**: DOCUMENTED, NOT FIXED
- **Test**: `test_hyperp_index_smoothing_multiple_cranks_same_slot`
- **Severity**: Medium (requires multiple TXs in same slot, each costs fees)
- **Recommendation**: Return `index` instead of `mark` when dt=0
- See MEMORY.md and test for full analysis
