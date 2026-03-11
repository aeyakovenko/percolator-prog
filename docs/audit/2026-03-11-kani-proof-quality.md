# Kani Proof Quality Audit — 2026-03-11

## Summary

| Classification | Count |
|----------------|-------|
| INDUCTIVE      | 0     |
| STRONG         | 0     |
| WEAK           | 35    |
| UNIT TEST      | 1     |
| VACUOUS        | 0     |

**Overall assessment:** The Kani proof suite is broad (36 proofs across 7 modules) but exclusively validates pure arithmetic helper functions. No proof operates on program-level state, checks `canonical_inv()` or `valid_state()`, follows the assume(INV) → transition → assert(INV) inductive pattern, or exercises multi-account topology. The suite provides good coverage of arithmetic safety properties (bounds, monotonicity, conservation) but zero coverage of system-level state invariants. This is a critical gap — an arithmetic bug in a helper would be caught, but a state-machine violation (e.g., funds draining via malformed account interaction) would not.

**Key gaps across all proofs:**
- **No `canonical_inv()` or `valid_state()` usage** — zero proofs verify the system-level invariant
- **No inductive pattern** — no proof assumes the invariant, applies a transition, then re-checks the invariant
- **No multi-account topology** — every proof operates on scalar values or a single zeroed struct
- **No reachability witnesses** — zero `kani::cover!()` calls; vacuity risk is unmitigated
- **No loops over account arrays** — but also no delta-based loop-free alternatives; the proofs simply never touch accounts

## Proofs

| Proof | Line | Classification | Criteria Gaps | Recommendation |
|-------|------|----------------|---------------|----------------|
| `proof_queued_withdrawal_total_never_exceeds_original_amount` | 6060 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. Has a loop (epoch iteration) with unwind(6). | Add `kani::cover!()` to confirm non-vacuity. Upgrade to operate on a `WithdrawQueue` embedded in full vault state with `canonical_inv()` pre/post. |
| `proof_loyalty_mult_never_exceeds_max_tier` | 6219 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. Range-bounded input (delta ≤ 1M). | Add `kani::cover!()`. Low priority — pure function, bounding proof is appropriate. |
| `nightly_lp_collateral_value_never_exceeds_raw_share` | 6291 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. All inputs range-bounded. | Add `kani::cover!()`. Consider embedding in vault state to verify LTV interaction with actual collateral tracking. |
| `nightly_drawdown_monotone` | 6309 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. | Add `kani::cover!()`. Monotonicity proof is appropriate for this helper. |
| `proof_split_deposit_conservation` | 6786 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. | **Priority upgrade** — conservation proof should be lifted to full deposit flow with `canonical_inv()` pre/post to ensure on-chain fund conservation. Add `kani::cover!()`. |
| `proof_reward_bounded` | 6798 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. Zero assumptions (full u64 space). | Good symbolic coverage. Add `kani::cover!()`. Upgrade to verify reward disbursement preserves `canonical_inv()`. |
| `proof_reward_monotone_decrease` | 6809 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. Zero assumptions. | Add `kani::cover!()`. Low priority. |
| `proof_topup_monotone_increase` | 6818 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. | Add `kani::cover!()`. Low priority. |
| `nightly_proof_lock_never_expires_early` | 7085 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. | Add `kani::cover!()`. Low priority — pure time-comparison function. |
| `proof_max_withdrawable_bounded` | 7099 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. Zero assumptions. | Add `kani::cover!()`. Low priority. |
| `proof_fully_locked_zero_withdraw` | 7110 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. | Add `kani::cover!()`. Low priority. |
| `nightly_proof_extraction_monotone` | 7121 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. | Add `kani::cover!()`. Consider upgrading to verify extraction limits against actual vault state. |
| `proof_fee_redirect_conservation` | 7136 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. | **Priority upgrade** — fee conservation should be verified end-to-end against vault lamport balances with `canonical_inv()`. Add `kani::cover!()`. |
| `proof_multiplier_monotone` | 7369 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. | Add `kani::cover!()`. Low priority. |
| `proof_discount_bounded` | 7379 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. Zero assumptions. | Add `kani::cover!()`. Low priority. |
| `nightly_proof_deposit_floor` | 7387 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. | Add `kani::cover!()`. Low priority. |
| `nightly_proof_slash_conservation` | 7399 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. | **Priority upgrade** — slash conservation is security-critical. Should verify slash + remainder == deposit within full program state and `canonical_inv()`. Add `kani::cover!()`. |
| `nightly_proof_oi_threshold_monotone` | 7408 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. | Add `kani::cover!()`. Low priority. |
| `nightly_sv_exposure_cap_bounded` | 7887 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. | Add `kani::cover!()`. Consider verifying cap enforcement against actual vault allocation state. |
| `nightly_sv_available_bounded` | 7901 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. Zero assumptions. | Add `kani::cover!()`. Low priority. |
| `nightly_sv_proportional_bounded` | 7910 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. | Add `kani::cover!()`. |
| `nightly_sv_epoch_monotone` | 7922 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. | Add `kani::cover!()`. Low priority. |
| `nightly_sv_queue_monotone` | 7939 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. Zero assumptions. | Add `kani::cover!()`. Low priority. |
| `nightly_sv_max_alloc_bounded` | 7948 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. | Add `kani::cover!()`. |
| `proof_sv_ordering_invariant` | 7959 | WEAK | No canonical_inv, no inductive pattern, no reachability witness. Conceptually 2-user but no actual accounts. | Add `kani::cover!()`. **Priority upgrade** — fairness property should be verified with actual multi-account vault state. |
| `nightly_sv_total_payout_bounded` | 7983 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. | **Priority upgrade** — payout boundedness is security-critical. Should verify against full vault state with `canonical_inv()` to ensure vault cannot be drained. Add `kani::cover!()`. |
| `proof_orphan_penalty_only_applies_when_oracle_stale_and_not_resolved` | 8546 | WEAK | No canonical_inv, no inductive pattern, no reachability witness. Uses zeroed MarketConfig — no real account state. | Add `kani::cover!()`. Consider testing with non-zeroed MarketConfig fields to cover interaction effects. |
| `nightly_sv_exits_after_duration` | 15693 | WEAK | No canonical_inv, no inductive pattern, no reachability witness. Uses zeroed MarketConfig. | Add `kani::cover!()`. |
| `proof_oracle_phase_monotone` | 16046 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. Closest to inductive (old_phase → new_phase monotonicity) but lacks state invariant. | Add `kani::cover!()`. Upgrade to embed in market state with `canonical_inv()`. |
| `proof_phase1_oi_cap_bounded` | 16064 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. | Add `kani::cover!()`. Low priority. |
| `proof_phase2_leverage_bounded` | 16072 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. | Add `kani::cover!()`. Low priority. |
| `proof_phase3_terminal` | 16080 | UNIT TEST | Concrete `old_phase = 2` (not symbolic). No canonical_inv, no inductive pattern, no accounts, no reachability witness. | Make `old_phase` symbolic with `kani::assume(old_phase == 2)` for consistency. Add `kani::cover!()`. Low priority. |
| `proof_cumulative_volume_monotone` | 16096 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. Zero assumptions. | Add `kani::cover!()`. Low priority. |
| `proof_phase1_requires_min_time` | 16105 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. | Add `kani::cover!()`. |
| `proof_phase_caps_leq_base` | 16119 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. | Add `kani::cover!()`. Low priority. |
| `proof_legacy_market_no_auto_promote` | 16131 | WEAK | No canonical_inv, no inductive pattern, no accounts, no reachability witness. | Add `kani::cover!()`. |

## Critical Upgrades Needed

### 1. Add system-level inductive proofs (NEW — highest priority)
**No proof in the suite verifies `canonical_inv()`.** The entire Kani harness validates arithmetic helpers in isolation but never tests that program state transitions preserve the system invariant. This means a bug in how helpers are *composed* — or in account deserialization, authority checks, or cross-account balance reconciliation — would go undetected by Kani.

**Recommendation:** Create at least 3 new INDUCTIVE proofs covering:
- **Deposit flow:** assume(`canonical_inv(state)`) → `process_deposit(symbolic_args)` → assert(`canonical_inv(state')`)
- **Withdrawal flow:** assume(`canonical_inv(state)`) → `process_withdrawal(symbolic_args)` → assert(`canonical_inv(state')`)
- **Liquidation/slash flow:** assume(`canonical_inv(state)`) → `process_slash(symbolic_args)` → assert(`canonical_inv(state')`)

Each should use fully symbolic state (not zeroed structs), multi-account topology, and include `kani::cover!()` reachability witnesses.

### 2. Upgrade `nightly_sv_total_payout_bounded` (line 7983) to STRONG
This proof verifies that a single user's payout never exceeds their LP position — a security-critical property for preventing vault drainage. Currently it operates on raw scalars. It should be upgraded to operate on a full `SharedVault` state with `canonical_inv()` checked pre/post, and extended to verify that the *sum* of all payouts ≤ vault capital (multi-account conservation).

### 3. Upgrade `proof_split_deposit_conservation` (line 6786) and `proof_fee_redirect_conservation` (line 7136)
Both prove fund conservation (`a + b == total`) for arithmetic helpers. These should be lifted to full instruction-level proofs verifying that on-chain lamport/token balances are conserved across the deposit-split and fee-redirect instructions. This is where real fund-loss bugs would hide.

### 4. Upgrade `nightly_proof_slash_conservation` (line 7399) to STRONG
Slash conservation (`slash + remainder == deposit`) is security-critical for creator stake. The pure-function proof should be embedded in full program state to verify that the slash instruction preserves `canonical_inv()` and that slashed funds are correctly routed (not destroyed or duplicated).

### 5. Add `kani::cover!()` to all 36 proofs (quick win)
Zero proofs have reachability witnesses. Without `kani::cover!()`, overly restrictive `kani::assume()` calls could render a proof vacuously true (the property holds because no inputs satisfy the assumptions). Adding `kani::cover!(true, "reachable")` after the assume block in each proof is a one-line change that eliminates this risk.

## Appendix: Classification Criteria Reference

| Criterion | INDUCTIVE | STRONG | WEAK | UNIT TEST | VACUOUS |
|-----------|-----------|--------|------|-----------|---------|
| Symbolic state | Fully symbolic | Symbolic inputs | Range-bounded | Concrete | N/A |
| Invariant | canonical_inv() | canonical_inv() | None or valid_state() | None | N/A |
| Loop handling | Delta-based loop-free | Any | Any | Any | N/A |
| Non-vacuity | kani::cover!() witness | kani::cover!() | None | None | Unreachable |
| Topology | Multi-account | Any | Single or none | Single | N/A |
| Inductive strength | assume(INV)+tx+assert(INV) | Constructed state | Property check | Assert on concrete | Trivially true |
