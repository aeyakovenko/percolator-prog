# Percolator-prog Formal Verification Audit

## Kani Proofs Summary

**Date:** 2026-01-04
**Kani Version:** 0.66.0
**Total Proofs:** 56
**Passed:** 56
**Failed:** 0

## Proof Categories

These proofs verify **program-level** security properties only.
Risk engine internals are NOT modeled - only wrapper authorization and binding logic.

### A. Matcher ABI Validation (12 proofs)
| Harness | Property |
|---------|----------|
| kani_matcher_rejects_wrong_abi_version | Wrong ABI version rejected |
| kani_matcher_rejects_missing_valid_flag | Missing VALID flag rejected |
| kani_matcher_rejects_rejected_flag | REJECTED flag causes rejection |
| kani_matcher_rejects_wrong_req_id | Mismatched req_id rejected |
| kani_matcher_rejects_wrong_lp_account_id | Mismatched lp_account_id rejected |
| kani_matcher_rejects_wrong_oracle_price | Mismatched oracle_price rejected |
| kani_matcher_rejects_nonzero_reserved | Non-zero reserved rejected |
| kani_matcher_rejects_zero_exec_price | Zero exec_price rejected |
| kani_matcher_zero_size_requires_partial_ok | Zero size needs PARTIAL_OK |
| kani_matcher_rejects_exec_size_exceeds_req | exec_size > req_size rejected |
| kani_matcher_rejects_sign_mismatch | Sign mismatch rejected |
| kani_matcher_zero_size_with_partial_ok_accepted | Zero size with PARTIAL_OK accepted |

### B. Owner/Signer Enforcement (2 proofs)
| Harness | Property |
|---------|----------|
| kani_owner_mismatch_rejected | Owner != signer -> rejected |
| kani_owner_match_accepted | Owner == signer -> accepted |

### C. Admin Authorization (3 proofs)
| Harness | Property |
|---------|----------|
| kani_admin_mismatch_rejected | Admin != signer -> rejected |
| kani_admin_match_accepted | Admin == signer -> accepted |
| kani_admin_burned_disables_ops | Admin == [0;32] -> all ops disabled |

### D. CPI Identity Binding (2 proofs) - CRITICAL
| Harness | Property |
|---------|----------|
| kani_matcher_identity_mismatch_rejected | LP prog/ctx != provided -> rejected |
| kani_matcher_identity_match_accepted | LP prog/ctx == provided -> accepted |

### E. Matcher Account Shape Validation (5 proofs)
| Harness | Property |
|---------|----------|
| kani_matcher_shape_rejects_non_executable_prog | Non-executable program rejected |
| kani_matcher_shape_rejects_executable_ctx | Executable context rejected |
| kani_matcher_shape_rejects_wrong_ctx_owner | Context not owned by program rejected |
| kani_matcher_shape_rejects_short_ctx | Insufficient context length rejected |
| kani_matcher_shape_valid_accepted | Valid shape accepted |

### F. PDA Key Matching (2 proofs)
| Harness | Property |
|---------|----------|
| kani_pda_mismatch_rejected | Expected != provided key -> rejected |
| kani_pda_match_accepted | Expected == provided key -> accepted |

### G. Nonce Monotonicity (3 proofs)
| Harness | Property |
|---------|----------|
| kani_nonce_unchanged_on_failure | Failure -> nonce unchanged |
| kani_nonce_advances_on_success | Success -> nonce += 1 |
| kani_nonce_wraps_at_max | u64::MAX -> wraps to 0 |

### H. CPI Uses exec_size (1 proof) - CRITICAL
| Harness | Property |
|---------|----------|
| kani_cpi_uses_exec_size | CPI uses exec_size, not requested size |

### I. Gate Activation Logic (3 proofs)
| Harness | Property |
|---------|----------|
| kani_gate_inactive_when_threshold_zero | threshold=0 -> gate inactive |
| kani_gate_inactive_when_balance_exceeds | balance > threshold -> gate inactive |
| kani_gate_active_when_conditions_met | threshold>0 && balance<=threshold -> gate active |

### J. Keeper Crank Authorization (3 proofs)
| Harness | Property |
|---------|----------|
| kani_crank_authorized_no_account | Account doesn't exist -> anyone can crank |
| kani_crank_authorized_owner_match | Owner matches -> crank allowed |
| kani_crank_rejected_owner_mismatch | Owner mismatch -> crank rejected |

### K. Per-Instruction Authorization (4 proofs)
| Harness | Property |
|---------|----------|
| kani_single_owner_mismatch_rejected | Single-owner instruction rejects on mismatch |
| kani_single_owner_match_accepted | Single-owner instruction accepts on match |
| kani_trade_rejects_user_mismatch | Trade rejects when user owner mismatch |
| kani_trade_rejects_lp_mismatch | Trade rejects when LP owner mismatch |

### L. TradeCpi Decision Coupling (12 proofs) - CRITICAL
These prove program-level policies via the `decide_trade_cpi` decision helper.

| Harness | Property |
|---------|----------|
| kani_tradecpi_rejects_non_executable_prog | Bad shape (non-exec prog) -> reject |
| kani_tradecpi_rejects_executable_ctx | Bad shape (exec ctx) -> reject |
| kani_tradecpi_rejects_pda_mismatch | PDA mismatch -> reject |
| kani_tradecpi_rejects_user_auth_failure | User auth failure -> reject |
| kani_tradecpi_rejects_lp_auth_failure | LP auth failure -> reject |
| kani_tradecpi_rejects_identity_mismatch | Identity mismatch (even if ABI valid) -> reject |
| kani_tradecpi_rejects_abi_failure | ABI failure -> reject |
| kani_tradecpi_rejects_gate_risk_increase | Gate active + risk increase -> reject |
| kani_tradecpi_allows_gate_risk_decrease | Gate active + risk decrease -> accept |
| kani_tradecpi_reject_nonce_unchanged | Reject -> nonce unchanged |
| kani_tradecpi_accept_increments_nonce | Accept -> nonce += 1 |
| kani_tradecpi_accept_uses_exec_size | Accept -> uses exec_size |

### M. TradeNoCpi Decision Coupling (4 proofs)
| Harness | Property |
|---------|----------|
| kani_tradenocpi_rejects_user_auth_failure | User auth failure -> reject |
| kani_tradenocpi_rejects_lp_auth_failure | LP auth failure -> reject |
| kani_tradenocpi_rejects_gate_risk_increase | Gate active + risk increase -> reject |
| kani_tradenocpi_accepts_valid | All checks pass -> accept |

## Key Security Properties Proven

### Authorization Surface
1. **Owner checks cannot be bypassed** - Every account operation validates owner == signer
2. **Admin checks cannot be bypassed** - Admin ops require admin == signer
3. **Burned admin is permanent** - [0;32] admin disables all admin ops forever
4. **Crank authorization is correct** - Existing accounts require owner, non-existent allow anyone
5. **Trade requires both parties** - Both user and LP owners must sign

### CPI Security (CRITICAL)
1. **Matcher identity binding** - CPI only proceeds if provided program/context match LP registration
2. **Matcher shape validation** - Program must be executable, context must not be, owner must be program
3. **exec_size is used** - CPI path uses matcher's exec_size, never the user's requested size
4. **Identity mismatch rejects even with valid ABI** - Strong adversary model

### State Consistency
1. **Nonce unchanged on failure** - Any rejection leaves nonce unchanged
2. **Nonce advances on success** - Successful trade advances nonce by exactly 1
3. **Nonce wraps correctly** - u64::MAX wraps to 0

### Risk Gate Policy
1. **Gate inactive when threshold=0** - Zero threshold disables gating
2. **Gate inactive when balance > threshold** - Sufficient funds disable gating
3. **Risk-increasing trades rejected when gate active** - Anti-DoS protection
4. **Risk-reducing trades allowed when gate active** - Deleveraging permitted

### Matcher ABI
1. **All field mismatches rejected** - ABI version, req_id, lp_account_id, oracle_price, reserved
2. **Flag semantics enforced** - VALID required, REJECTED causes rejection, PARTIAL_OK for zero size
3. **Size constraints enforced** - exec_size <= req_size, sign must match

## Implementation: pub mod verify

Pure helpers in percolator.rs, **wired into actual instruction handlers**:

```rust
pub mod verify {
    // Primitive helpers
    pub fn owner_ok(stored, signer) -> bool
    pub fn admin_ok(admin, signer) -> bool
    pub fn matcher_identity_ok(lp_prog, lp_ctx, provided_prog, provided_ctx) -> bool
    pub fn matcher_shape_ok(shape: MatcherAccountsShape) -> bool
    pub fn ctx_len_sufficient(len: usize) -> bool
    pub fn gate_active(threshold, balance) -> bool
    pub fn nonce_on_success(old) -> u64
    pub fn nonce_on_failure(old) -> u64
    pub fn pda_key_matches(expected, provided) -> bool
    pub fn cpi_trade_size(exec_size, requested_size) -> i128
    pub fn crank_authorized(idx_exists, stored_owner, signer) -> bool

    // Per-instruction authorization
    pub fn single_owner_authorized(stored_owner, signer) -> bool
    pub fn trade_authorized(user_owner, user_signer, lp_owner, lp_signer) -> bool

    // Decision helpers (model full wrapper policy)
    pub enum TradeCpiDecision { Reject, Accept { new_nonce, chosen_size } }
    pub fn decide_trade_cpi(...) -> TradeCpiDecision
    pub fn decision_nonce(old_nonce, decision) -> u64

    pub enum TradeNoCpiDecision { Reject, Accept }
    pub fn decide_trade_nocpi(...) -> TradeNoCpiDecision
}
```

### Helper Wiring (Instruction -> verify::)

| Instruction | Helpers Used |
|-------------|--------------|
| DepositCollateral | `owner_ok` |
| WithdrawCollateral | `owner_ok` |
| TradeNoCpi | `owner_ok` (x2), `gate_active` |
| TradeCpi | `matcher_shape_ok`, `ctx_len_sufficient`, `owner_ok` (x2), `matcher_identity_ok`, `nonce_on_success`, `gate_active`, `cpi_trade_size` |
| CloseAccount | `owner_ok` |
| KeeperCrank | `crank_authorized` |
| SetRiskThreshold | `admin_ok` (via require_admin) |
| UpdateAdmin | `admin_ok` (via require_admin) |

**Note:** Kani proofs verify properties of the same code paths the program actually executes.
The `decide_trade_cpi` helper models the full wrapper decision, enabling Kani to prove
program-level policies like "reject implies nonce unchanged" and "accept implies uses exec_size".

## What is NOT Proven

- Risk engine internals (LpRiskState, risk metric formula)
- CPI execution (Solana invoke mechanics)
- AccountInfo validation (done at runtime by Solana)
- Actual PDA derivation (Solana's find_program_address)
- Token transfer correctness (SPL Token program)
