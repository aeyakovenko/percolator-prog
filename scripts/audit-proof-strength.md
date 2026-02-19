# Kani Proof Strength Audit Prompt (percolator-prog)

Use this prompt to analyze Kani proof harnesses in `tests/kani.rs` for weakness, vacuity, or collapse into unit tests.

This program's proofs cover **wrapper-level security properties** — authorization, ABI validation,
identity binding, nonce monotonicity, unit conversion math, oracle math, and dust accounting.
The functions-under-test are extracted into `pub mod verify` in `src/percolator.rs` and
`pub mod matcher_abi`. These are pure functions (no state, no I/O) proven against symbolic inputs.

---

Analyze each of the 147 Kani proof harnesses. For every proof, determine:

1. **Input classification**: Is each input to the function-under-test concrete (hardcoded),
   symbolic (`kani::any()` with `kani::assume` bounds), or derived (computed from other inputs)?
   A proof where ALL function inputs are concrete is a unit test, not a proof.

2. **Branch coverage**: Read the function-under-test in `src/percolator.rs` (the `verify` module,
   `matcher_abi` module, or `oracle` module) and list every conditional branch (if/else, match arms,
   `&&`/`||` short-circuit, `.min()`, `.max()`, `.clamp()`, `saturating_*`, `checked_*`, `Option`
   returns). For each branch, determine whether the proof's input constraints ALLOW the solver to
   reach both sides. Flag any branch locked to one side by concrete values or tight assumes.

   Key functions to cross-reference:
   - `verify::decide_trade_cpi` (~80 lines, 7 sequential gates)
   - `verify::decide_trade_cpi_from_ret` (~50 lines, builds on abi_ok + decide_trade_cpi)
   - `verify::decide_keeper_crank_with_panic` (~25 lines, 3 gates)
   - `matcher_abi::validate_matcher_return` (~60 lines, 8 sequential checks)
   - `verify::scale_price_e6` (division + zero-result check)
   - `verify::base_to_units` / `units_to_base` (division + remainder)
   - `verify::sweep_dust` / `accumulate_dust` (threshold comparison + saturation)
   - `oracle::clamp_toward_with_dt` (~30 lines, 5 branches: dt=0, cap=0, index=0, above/below)
   - `verify::withdraw_insurance_vault` (checked subtraction + overflow)

3. **Invariant strength**: What does the proof actually assert?
   - Does it test the REJECTION path (proof that bad input is rejected)?
   - Does it test the ACCEPTANCE path (proof that good input is accepted)?
   - Does it test a PROPERTY (e.g., nonce monotonicity, conservation, monotonicity)?
   - Are there assertions gated behind `if result == Accept` without a non-vacuity check
     (i.e., the solver might always choose Reject, making the assertion vacuously true)?

4. **Vacuity risk**: Can the solver satisfy all `kani::assume` constraints AND reach the
   assertions? Watch for:
   - Contradictory assumes that make the proof trivially true
   - `kani::assume(condition)` that restricts inputs to a single concrete value
   - Assertions on Ok/Accept paths without proof that the Ok/Accept path is reachable

5. **Symbolic collapse**: Even with `kani::any()`, check if derived values collapse the
   symbolic range. Examples:
   - `KANI_MAX_SCALE = 64` bounds unit_scale but is this sufficient to exercise all branches
     of `scale_price_e6` and `base_to_units`? Does the `price * 1_000_000 / scale` division
     always produce non-zero for small scales?
   - `KANI_MAX_QUOTIENT = 4096` — does this bound meaningfully restrict coverage?
   - In `decide_trade_cpi` proofs, if gate_active is always false (threshold=0 or balance huge),
     the risk-increase gate is never exercised.
   - In the "universal" proofs (sections AE-AI), one gate is forced to fail — verify the OTHER
     gates are still symbolically explored, not collapsed.

6. **Coupling completeness** (specific to this codebase): The `verify` module extracts logic
   from the instruction handlers in `mod processor`. For critical decisions (trade authorization,
   crank authorization, admin ops), verify that:
   - The proof exercises the SAME logic path as the production handler
   - No production handler makes a decision that ISN'T captured in the `verify` function
   - The `verify` function's parameters map 1:1 to the actual account/instruction data

For each proof, output one classification:
- **STRONG**: Symbolic inputs exercise key branches of the function-under-test, appropriate
  property is asserted, non-vacuous
- **WEAK**: Symbolic inputs but misses branches, uses weaker assertion, or symbolic collapse
  reduces coverage (list which specific issue)
- **UNIT TEST**: Concrete inputs or single execution path — intentional and acceptable if
  documented as such (base cases, regressions, meta-tests)
- **VACUOUS**: Assertions may never be reached or are trivially true

Include specific recommendations to strengthen any non-STRONG proof.

---

## Output Format

```markdown
## Classification Summary

| Classification | Count | Description |
|---|---|---|
| STRONG | N | ... |
| WEAK | N | ... |
| UNIT TEST | N | ... |
| VACUOUS | N | ... |

## WEAK Proofs by Category

### Category A: Branch Coverage Gaps
| Proof | Line | Issue | Recommendation |
|---|---|---|---|

### Category B: Weak Assertions
| Proof | Line | Issue | Recommendation |
|---|---|---|---|

### Category C: Symbolic Collapse
| Proof | Line | Issue | Recommendation |
|---|---|---|---|

### Category D: Trivially True
| Proof | Line | Issue | Recommendation |
|---|---|---|---|

## UNIT TEST Proofs
| Proof | Line | Reason |
|---|---|---|

## STRONG Proofs (N)
Brief summary of notable strongest proofs.

## Cross-Cutting Observations
Systemic patterns, common weaknesses, coverage gaps.
```
