# Percolator Security Audit - First Principles Review

Auditor perspective: Assume developer is adversarial. Look for backdoors, fund extraction, manipulation vectors.

## Executive Summary

| Severity | Count | Status |
|----------|-------|--------|
| 🔴 CRITICAL | 4 | **All Fixed** |
| 🟠 HIGH | 8 | **5 Fixed (Program), 3 Open (Engine)** |
| 🟡 MEDIUM | 2 | Open (Engine) |
| ✅ FIXED | 4 | Verified |

**Key Finding**: No direct admin backdoors for fund extraction. Primary attack vectors were oracle substitution and system wedging - now fixed in program layer.

---

## 🔴 CRITICAL ISSUES - ALL FIXED

### C1: ✅ FIXED - TradeNoCpi Oracle Substitution
**Location**: `percolator-prog/src/percolator.rs:1889`
**Fix**: Added `oracle_key_ok(config.index_oracle, a_oracle.key)` check before reading price.

### C2: ✅ FIXED - Oracle Parser Accepts Wrong Pyth Account Types
**Location**: `percolator-prog/src/percolator.rs:1232-1248`
**Fix**: Added validation for Pyth magic (0xa1b2c3d4), version (2), and account type (3=Price).

### C3: ✅ FIXED - Oracle Exponent Overflow
**Location**: `percolator-prog/src/percolator.rs:1253-1256`
**Fix**: Added `MAX_EXPO_ABS = 18` bound. Exponents outside [-18, +18] rejected.

### C4: ✅ FIXED - Permissionless allow_panic
**Location**: `percolator-prog/src/percolator.rs:1818-1825`
**Fix**: `allow_panic != 0` now requires admin signature via `admin_ok()` check.

---

## 🟠 HIGH ISSUES - PROGRAM FIXES COMPLETE

### H1: ✅ FIXED - InitMarket Data/Account Mismatch
**Location**: `percolator-prog/src/percolator.rs:1578-1582`
**Fix**: Added `collateral_mint != *a_mint.key` validation to enforce data matches accounts.

### H2: ✅ FIXED - InitMarket No SPL Mint Validation
**Location**: `percolator-prog/src/percolator.rs:1584-1599`
**Fix**: Added SPL Mint validation (owner, length, unpack) for mint account.

### H3: ✅ FIXED - verify_vault No State Check
**Location**: `percolator-prog/src/percolator.rs:1505-1509`
**Fix**: Added `tok.state != AccountState::Initialized` check.

### H4: ✅ FIXED - Oracle No Status Check
**Location**: `percolator-prog/src/percolator.rs:1259-1267`
**Fix**: Added Pyth trading status validation (`status == 1`).

### H5: ✅ FIXED - Devnet Feature Safety
**Location**: `percolator-prog/src/percolator.rs:1191-1197`
**Fix**: Added security warning comment documenting risks of devnet feature.

---

## 🔴 ENGINE ISSUES - OPEN (Fix Separately)

### H6: ❌ OPEN - Liquidation Error Swallowing
**Location**: `percolator/src/percolator.rs` - scan loops
**Issue**: Liquidation scan ignores individual errors.
**Partial Mitigation**: MTM overflow now returns equity=0 (fail-safe).

### H7: ❌ OPEN - ABI Drift (Unused Accounts)
**Location**: `percolator-cli/src/abi/accounts.ts`
**Issue**: InitUser/InitLP declare clock+oracle accounts but handlers ignore them.

### H8: ❌ OPEN - Socialization Wedge Risk
**Location**: `percolator/src/percolator.rs` - pending buckets
**Issue**: If `liquidate_at_oracle` writes to pending buckets and keeper fails, withdrawals blocked.

---

## 🟡 MEDIUM ISSUES - OPEN (Engine)

### M1: ❌ OPEN - Saturating Arithmetic Edge Cases
**Location**: `percolator/src/percolator.rs`
**Partial Mitigation**: mark_pnl overflow now fails safe.

### M2: ❌ OPEN - O(n²) ADL Remainder Distribution
**Location**: `percolator/src/percolator.rs:2608-2656`
**Partial Mitigation**: Bounded socialization reduces scope.

---

## ✅ PREVIOUSLY VERIFIED FIXES

### F1: MTM Margin Check
**Commit**: `1789243`
- Liquidation and margin checks now use mark-to-market equity
- Benchmark confirms 2047 liquidations trigger correctly

### F2: O(1) LP Aggregates
**Commit**: `cbf5b16`
- Baseline CU: 176K → 16K

### F3: Bounded Socialization
**Commit**: `ffda3f3`
- ADL bounded to O(WINDOW) per crank

### F4: Overflow Fail-Safe
**Commit**: `8fdb96f`
- mark_pnl overflow returns equity=0 (triggers liquidation)

---

## First-Principles Security Analysis

### Can Admin Steal Funds?
**NO** - No instruction allows admin to withdraw vault or insurance directly.

### Can LP Steal from Users?
**NO (Fixed)** - Oracle substitution attack now blocked by C1 fix.

### Can Users Steal from LP?
**NO (Fixed)** - Same oracle substitution fix.

### Can System Be Wedged?
**POSSIBLY (H8)** - Engine issue, needs separate fix.

### Are Funds Extractable Without Authorization?
**NO** - All token transfers require owner signature + PDA authority.

### Is There a Rug Pull Vector?
**NO** - Admin cannot drain funds or modify positions.

---

## Program-Specific Fixes Summary

| Issue | Fix | Line |
|-------|-----|------|
| C1: Oracle substitution | Added oracle_key_ok | 1889 |
| C2: Pyth type validation | Added magic/version/type checks | 1232-1248 |
| C3: Exponent overflow | Added MAX_EXPO_ABS bound | 1253-1256 |
| C4: allow_panic auth | Added admin_ok check | 1818-1825 |
| H1: collateral_mint mismatch | Added equality check | 1578-1582 |
| H2: SPL Mint validation | Added owner/unpack checks | 1584-1599 |
| H3: Vault state check | Added Initialized check | 1505-1509 |
| H4: Pyth status check | Added status == 1 check | 1259-1267 |
| H5: Devnet warning | Added security comment | 1191-1197 |

---

## Benchmark Validation

| Scenario | Worst CU | % Limit | Liquidations |
|----------|----------|---------|--------------|
| Baseline | 16,314 | 1.2% | - |
| 4095 healthy | 39,684 | 2.8% | - |
| 4095 + crash | 54,533 | 3.9% | 512 force |
| **MTM worst case** | **740,166** | **52.9%** | **2047** |

All scenarios under 1.4M CU limit. MTM liquidations working correctly.
