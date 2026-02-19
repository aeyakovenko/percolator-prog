# Kani Proof Strength Audit Results

Generated: 2026-02-18 (deep 6-point audit per `scripts/audit-proof-strength.md`)

147 proof harnesses across `/home/anatoly/percolator-prog/tests/kani.rs`.

Methodology: Each proof analyzed for (1) input classification, (2) branch coverage against source,
(3) invariant strength, (4) vacuity risk, (5) symbolic collapse, (6) coupling completeness.

---

## Classification Summary

| Classification | Count | Description |
|---|---|---|
| STRONG | 91 | Symbolic inputs exercise key branches; correct rejection/acceptance/property assertions; non-vacuous |
| WEAK | 38 | Misses branches, symbolic collapse, weak assertions, or redundant with universal proofs |
| UNIT TEST | 18 | All function inputs concrete; documents boundary cases and regression witnesses |
| VACUOUS | 0 | No proofs found where assertions are unreachable |

---

## WEAK Proofs by Category

### Category A: Branch Coverage Gaps

| Proof | Line | Issue | Recommendation |
|---|---|---|---|
| `kani_tradecpi_rejects_non_executable_prog` | 717 | All inputs except shape are concrete; gate 1 always fails, gates 2-6 never explored | Superseded by `kani_universal_shape_fail_rejects`; keep as documentation |
| `kani_tradecpi_rejects_executable_ctx` | 740 | Same: all inputs except shape concrete, gate 1 always fails | Same as above |
| `kani_tradecpi_rejects_pda_mismatch` | 763 | `gate_active=false`, `risk_increase=false` concrete; only pda gate tested | Superseded by `kani_universal_pda_fail_rejects` |
| `kani_tradecpi_rejects_user_auth_failure` | 790 | Symbolic `old_nonce`/`exec_size` have no effect since outcome is always Reject | Superseded by universal proofs |
| `kani_tradecpi_rejects_lp_auth_failure` | 816 | Same issue | Same |
| `kani_tradecpi_rejects_identity_mismatch` | 842 | Same issue | Same |
| `kani_tradecpi_rejects_abi_failure` | 868 | Same issue | Same |
| `kani_tradecpi_rejects_gate_risk_increase` | 893 | All boolean args concrete; superseded by `kani_universal_gate_risk_increase_rejects` | Same |
| `kani_tradecpi_allows_gate_risk_decrease` | 919 | All boolean args concrete; only tests gate-active + risk-decrease → Accept | Make booleans symbolic; only constrain gate_active=true, risk_increase=false |
| `kani_decide_crank_self_accepts` | 1474 | `permissionless=false`, `idx_exists=true` concrete; only 1 of 3 branches hit | Add universal proof over all combinations |
| `kani_decide_crank_rejects_no_idx` | 1487 | `permissionless=false`, `idx_exists=false` concrete | Same |
| `kani_decide_crank_rejects_wrong_owner` | 1501 | `permissionless=false`, `idx_exists=true`, signer != owner forced | Same |
| `kani_invert_result_zero_returns_none` | 2139 | `raw > 1e12` so `inverted == 0` always; overflow branch (`inverted > u64::MAX`) never exercised | Document that overflow branch is dead code (1e12 < u64::MAX) |
| `kani_sweep_dust_below_threshold` | 2411 | Only tests `dust < scale`; `dust >= scale` covered by conservation proof | Acceptable partial coverage |
| `kani_scale_zero_policy_end_to_end` | 2475 | `base_to_units(base, 0)` always returns `dust=0`, so `sweep_dust(0, 0)` is trivial | Add proof with `old_dust: kani::any()` via `accumulate_dust` before sweep |
| `kani_universal_gate_risk_increase_rejects` | 2914 | All concrete values for the `decide_trade_cpi` call | Use symbolic inputs; rely on from_ret version (3019) which is stronger |

### Category B: Weak Assertions

| Proof | Line | Issue | Recommendation |
|---|---|---|---|
| `kani_cpi_uses_exec_size` | 589 | `cpi_trade_size` is `{ exec_size }` — tautological | Accept as regression guard if function body could change |
| `kani_nonce_wraps_at_max` | 576 | Concrete `u64::MAX`; subsumed by `kani_nonce_advances_on_success` | Document as regression witness |
| `kani_tradecpi_reject_nonce_unchanged` | 944 | Only covers shape failures; superseded by `kani_tradecpi_any_reject_nonce_unchanged` | Keep as named documentation |
| `kani_tradecpi_accept_increments_nonce` | 977 | Good for all-pass path but superseded by `kani_tradecpi_any_accept_increments_nonce` | Same |
| `kani_tradecpi_accept_uses_exec_size` | 1023 | Duplicate of `kani_tradecpi_accept_increments_nonce`; both assert same Accept equality | Remove; fully covered |
| `kani_accumulate_dust_saturates` | 2437 | Asserts `result == old.saturating_add(added)` — code-equals-spec tautology | Accept as coupling guard |
| `kani_scale_price_and_base_to_units_use_same_divisor` | 3342 | Comment claims ratio preservation but NO formal assertion of the ratio invariant | Add assertion: margin accept/reject decision consistent across scaling |
| `kani_scale_price_e6_concrete_example` | 3380 | Asserts `scaled_valuation * unit_scale <= unscaled_valuation` (one-sided bound only) | Add bidirectional pass/fail consistency assertion |

### Category C: Symbolic Collapse

| Proof | Line | Issue | Recommendation |
|---|---|---|---|
| `kani_invert_nonzero_computes_correctly` | 2111 | `raw <= 4096` so `inverted >= ~244M`, always fits u64; overflow branch dead | Document dead branch |
| `kani_invert_monotonic` | 2155 | Both `raw1`, `raw2 <= 4096`; only tests monotonicity for small values | Acceptable for SAT tractability; note domain gap |
| `kani_base_to_units_conservation` | 2181 | `base <= 262K` (scale * KANI_MAX_QUOTIENT); integer division identity universal | Acceptable; bound for SAT tractability |
| `kani_base_to_units_dust_bound` | 2202 | Same 262K bound; `dust < scale` universal for modular arithmetic | Same |
| `kani_units_roundtrip` | 2229 | `units <= 4096`; roundtrip property universal | Same |
| `kani_base_to_units_monotonic` | 2256 | Both bases bounded at 262K; floor division monotonicity universal | Same |
| `kani_units_to_base_monotonic_bounded` | 2279 | Both units <= 4096, scale <= 64; max product 262K, well below u64::MAX saturation | Acceptable; name documents limitation |
| `kani_withdraw_misaligned_rejects` | 2324 | `q <= 4096`; misalignment property universal for any `r > 0` | Acceptable |
| `kani_clamp_toward_movement_bounded_concrete` | 3491 | `index <= 1e9`, `cap <= 200K`, `dt <= 16`; bounded domain means `lo` saturates to 0 assertion is trivial | Add proof for saturation boundary |
| `kani_clamp_toward_formula_concrete` | 3552 | `index ∈ [100,200]`, `cap ∈ [1,5]`, `dt ∈ [1,20]`, `mark ∈ [0,400]`; ~4M combinations, borderline concrete | Acceptable for SAT tractability; 3 companion proofs cover all 3 branches |
| `kani_clamp_toward_formula_within_bounds` | 3578 | Same constrained domain | Same |
| `kani_clamp_toward_formula_above_hi` | 3606 | Same constrained domain | Same |

### Category D: Trivially True or Structural

| Proof | Line | Issue | Recommendation |
|---|---|---|---|
| `kani_oracle_feed_id_match` | 1388 | `oracle_feed_id_ok(x, x)` is `x == x` — tautology | Accept as coupling guard |
| `kani_scale_zero_policy_no_dust` | 2453 | `base_to_units(base, 0)` returns `(base, 0)` by definition | Accept as scale=0 policy documentation |
| `kani_scale_zero_policy_sweep_complete` | 2464 | `sweep_dust(dust, 0)` returns `(0, 0)` by definition | Same |
| `kani_withdraw_insurance_vault_reaches_zero` | 3669 | `checked_sub(x, x) = Some(0)` — tautology | Subsumed by `result_characterization` proof |

---

## UNIT TEST Proofs (18)

Intentional: boundary cases, concrete shape tests, regression witnesses, and tests on trivial functions.

| Proof | Line | Reason |
|---|---|---|
| `kani_matcher_rejects_rejected_flag` | 162 | Concrete fields; exercises FLAG_REJECTED path only |
| `kani_matcher_rejects_zero_exec_price` | 261 | `exec_price_e6 = 0` hardcoded; single branch |
| `kani_matcher_zero_size_requires_partial_ok` | 279 | `exec_size = 0`, `flags = FLAG_VALID` concrete; single path |
| `kani_matcher_shape_rejects_non_executable_prog` | 443 | Concrete struct with one false field |
| `kani_matcher_shape_rejects_executable_ctx` | 459 | Concrete struct |
| `kani_matcher_shape_rejects_wrong_ctx_owner` | 475 | Concrete struct |
| `kani_matcher_shape_rejects_short_ctx` | 491 | Concrete struct |
| `kani_matcher_shape_valid_accepted` | 507 | Concrete all-true struct |
| `kani_tradenocpi_rejects_user_auth_failure` | 1063 | All four args concrete |
| `kani_tradenocpi_rejects_lp_auth_failure` | 1074 | All four args concrete |
| `kani_tradenocpi_rejects_gate_risk_increase` | 1085 | All four args concrete |
| `kani_tradenocpi_accepts_valid` | 1096 | All four args concrete |
| `kani_decide_single_owner_accepts` | 1437 | Single concrete `true` |
| `kani_decide_single_owner_rejects` | 1448 | Single concrete `false` |
| `kani_lp_pda_shape_valid` | 1334 | Concrete all-true struct |
| `kani_slab_shape_valid` | 1410 | Concrete all-true struct |
| `kani_min_abs_boundary_rejected` | 1834 | Concrete i128::MIN; regression test for `.abs()` panic |
| `kani_init_market_scale_zero_ok` | 3137 | Concrete `scale=0` |

---

## STRONG Proofs (91)

All remaining proofs are STRONG: symbolic inputs exercise key branches of the function-under-test,
appropriate property (rejection/acceptance/conservation/monotonicity) is asserted, non-vacuous via
explicit reachability assertions or `panic!` on wrong-branch.

Notable strongest proofs:

- **`kani_tradecpi_any_reject_nonce_unchanged`** (1191): Fully symbolic over ALL `decide_trade_cpi` inputs. Proves nonce unchanged on Reject, incremented on Accept. Non-vacuity via explicit branch witnesses. Strongest single proof in section L.
- **`kani_tradecpi_from_ret_accept_uses_exec_size`** (1743): Forces Accept path with valid ABI constraints, asserts `chosen_size == ret.exec_size`. Most security-critical proof — prevents exec_size substitution attacks.
- **`kani_abi_ok_equals_validate`** (1560): Proves `abi_ok` equivalent to `validate_matcher_return` for ALL symbolic inputs. Critical mechanically-tied coupling proof.
- **`kani_tradecpi_variants_consistent_valid_shape`** (2713): Proves `decide_trade_cpi` and `decide_trade_cpi_from_ret` agree on all outcomes for all symbolic inputs under valid shape.
- **`kani_universal_shape_fail_rejects`** (2496): Symbolic shape with `!matcher_shape_ok(shape)`, all other inputs fully symbolic. Gate 1 kill-switch proof.
- **`kani_tradecpi_from_ret_forced_acceptance`** (3067): Forces Accept via valid ABI + all-pass gates. Uses `panic!` on Reject for non-vacuity (gold standard).
- **`kani_clamp_toward_no_movement_when_dt_zero`** (3433): Proves Bug #9 fix: `dt=0 → returns index`. Symbolic index, mark, cap.
- **`kani_clamp_toward_bootstrap_when_index_zero`** (3474): Proves bootstrap case: `index=0 → returns mark`. Symbolic mark, cap, dt.
- **`kani_withdraw_insurance_vault_result_characterization`** (3683): Full symbolic; bidirectional assertions on Some/None outcomes.
- **`kani_init_market_scale_valid_range`** (3163): Symbolic `scale <= MAX_UNIT_SCALE`; proves acceptance.
- **`kani_matcher_rejects_wrong_abi_version`** (131): Full symbolic MatcherReturn; only `abi_version != MATCHER_ABI_VERSION` constrained.
- **`kani_matcher_identity_mismatch_rejected`** (410): Symbolic 256-bit keys; proves disjunctive mismatch property.

---

## Cross-Cutting Observations

1. **No vacuous proofs**: Every proof with Ok/Accept-path assertions includes explicit reachability
   checks (`panic!` on wrong branch, or assertions on both branches). The `#[kani::should_panic]`
   pattern is not used in this file.

2. **Systematic redundancy in sections L vs AE**: The 12 specific `decide_trade_cpi` proofs in
   section L cover individual gate failures with mostly concrete booleans. Sections AE (6 proofs)
   and P (2 proofs) reprove the same properties universally with fully symbolic inputs. The section L
   proofs add no verification value beyond documentation once the universal proofs exist.

3. **TradeNoCpi has zero symbolic coverage**: All 4 proofs in section M use concrete Boolean tuples.
   Replace with 2 universal proofs: (1) `user_auth=false OR lp_auth=false → Reject` for all
   symbolic Booleans, (2) `gate_active && risk_increase → Reject` universally.

4. **Dead code: `invert_price_e6` overflow branch**: `INVERSION_CONSTANT = 1e12 < u64::MAX ≈ 1.8e19`,
   so `1e12 / raw` can never exceed `u64::MAX` for any positive `raw`. The branch
   `if inverted > u64::MAX as u128` is unreachable. No proof documents this. Add:
   `kani::assert(INVERSION_CONSTANT <= u64::MAX as u128)`.

5. **Missing from_ret proof for gate-active + risk-neutral → Accept**: `kani_tradecpi_allows_gate_risk_decrease`
   (919) proves this for `decide_trade_cpi` but NO corresponding proof exists for
   `decide_trade_cpi_from_ret` with `gate_active=true, risk_increase=false → Accept`.

6. **Missing keeper crank case**: `allow_panic + admin passes + self-crank with idx_exists=false → Reject`
   is not covered. The admin-fails path is covered universally but admin-passes + self-crank-fails is not.

7. **KANI_MAX_SCALE=64 and KANI_MAX_QUOTIENT=4096 are conservative but sound**: The integer
   arithmetic properties (division identity, monotonicity, roundtrip) are mathematically universal.
   The bounds exist purely for SAT tractability, not because larger values would exercise different branches.

8. **Strongest chain**: `kani_abi_ok_equals_validate` (coupling) → `kani_tradecpi_from_ret_accept_uses_exec_size`
   (exec_size binding) → `kani_cpi_uses_exec_size` (function returns exec_size). This 3-proof chain
   establishes that accepted trades use the matcher's exec_size, not the user's requested size.
   The only gap: no proof that the instruction handler passes `chosen_size` to `engine.execute_trade()`.

9. **Concrete shape/PDA/LP tests are acceptable as documentation**: Sections E, R, and S contain
   concrete struct tests (all-true acceptance, individual field failures). While these are unit tests
   by classification, they serve as readable documentation of each validation requirement and catch
   regressions if struct fields are reordered or renamed.

10. **`scale_price_e6` proofs miss the ratio preservation property**: The 5 proofs (lines 3265-3420)
    prove zero-rejection, valid-result bounds, identity for scale<=1, and same-divisor consistency.
    But none proves the critical margin invariant: that the pass/fail decision for margin checks is
    the same with and without unit scaling. The comment at line 3342 claims ratio preservation
    without a formal assertion.
