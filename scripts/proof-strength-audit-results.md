# Kani Proof Strength Audit Results — `tests/v16_kani.rs` (34 proofs)

Audit per `scripts/audit-proof-strength.md`. Functions-under-test: `Instruction::decode`
(`ix` module), `matcher_abi::validate_matcher_return`, `policy_v16::premium_funding_rate_e9`.

## Classification Summary

| Classification | Count | Description |
|---|---|---|
| STRONG | 5 | Symbolic inputs reach all material branches; correct property; non-vacuous |
| WEAK | 26 | Dominant `u16`→`u64`/`u128`/`i128`/`u32` widening collapse (Category C); plus the accept-only matcher proof and the concrete trailing/truncation batteries |
| UNIT TEST | 3 | Fully concrete buffers / single path (acceptable base/regression) |
| VACUOUS | 0 | none |

**Headline finding (Category C — symbolic collapse).** ~16 decode round-trip proofs draw
`u16`/`i16` symbolic seeds and widen them (`as u64`/`as u128`/`as i128`/`as u32`) into wider
wire fields. The encoded buffer therefore has the high bytes of every multi-byte field
**always zero**, so the round-trip proves byte *ordering/offset* but only proves value
preservation over the low 16 bits. Escapes uncovered: (a) a decode helper that reads a
**narrower width** than the field and zero-extends (e.g. a `read_u128` that only reads 8
bytes), and (b) an **off-by-N offset within a field whose differing bytes are all zero**.
The reviewed source helpers are correct (no live bug), but the proofs don't *guarantee* it.
`kani_v16_update_trade_fee_policy_...` already uses the correct full-width pattern.

## WEAK — Category C: symbolic collapse (fix: full-width `kani::any()`)

Uniform fix `[full-width]`: replace `let f = raw as WIDE;` with `let f: WIDE = kani::any();`
(u64/u128/i128/u32). Kani solves these fixed-size straight-line layout round-trips fast.

| Proof | Line | Widened fields |
|---|---|---|
| `kani_v16_init_market_decode_preserves_wire_fields` | 38 | 19 u64/u128 fields (margins, fees, slots, chunks, atoms) |
| `kani_v16_amount_instructions_decode_preserves_wire_fields` | 161 | `amount` (u128) |
| `kani_v16_domain_insurance_decode_preserves_wire_fields` | 207 | `amount` (u128) |
| `kani_v16_recovery_close_progress_decode_preserves_wire_fields` | 243 | b_delta_budget/reduce_q/close_q (u128), now_slot (u64) |
| `kani_v16_top_up_backing_bucket_decode_preserves_wire_fields` | 331 | amount (u128), expiry_slot (u64) |
| `kani_v16_withdraw_backing_bucket_decode_preserves_wire_fields` | 359 | amount (u128) |
| `kani_v16_asset_lifecycle_decode_preserves_wire_fields` | 378 | now_slot, initial_price (u64) |
| `kani_v16_tradenocpi_decode_preserves_wire_fields` | 427 | size_q (i128), exec_price/fee_bps (u64) |
| `kani_v16_tradecpi_decode_preserves_wire_fields` | 460 | size_q (i128), fee_bps/limit_price (u64) |
| `kani_v16_permissionless_crank_decode_preserves_wire_fields` | 546 | funding_rate_e9 (i128), close_q (u128), now_slot/fee_bps (u64) |
| `kani_v16_update_insurance_policy_decode_preserves_wire_fields` | 618 | cooldown_slots (u64) |
| `kani_v16_update_market_init_fee_policy_decode_preserves_wire_fields` | 733 | min_init_fee (u128) |
| `kani_v16_base_unit_payloads_decode_preserves_wire_fields` | 750 | amount (u128) |
| `kani_v16_permissionless_resolve_decode_preserves_wire_fields` | 781 | stale_slots, force_close_delay_slots, now_slot (u64) |
| `kani_v16_configure_hybrid_oracle_decode_preserves_wire_fields` | 816 | 6×u64/i64 + **unit_scale (u32)** |
| `kani_v16_ewma_mark_decode_preserves_wire_fields` | 900 | now_slot/initial_mark_e6/halflife/min_fee/push_mark (u64) |

Security-relevant wide fields to prioritize: `funding_rate_e9` (i128, crank), `size_q`
(i128, trades), all `u128` amounts, and `unit_scale` (u32, hybrid oracle).

## WEAK — Category A: branch-coverage gap

| Proof | Line | Issue | Fix |
|---|---|---|---|
| `kani_v16_matcher_return_accepts_only_bound_echoed_fills` | 494 | Encodes `abi_version=MATCHER_ABI_VERSION` and all echoed fields equal to expected, so the rejection branches (abi mismatch, echoed-field mismatch, `exec_price==0`, `exec_size==0 && price!=oracle`, i128::MIN guards) are never exercised; sizes are i16-widened. Proves only the accept-path forward implication. | Draw `abi_version`, the echoed fields, and full-width `i128` sizes symbolically; assert the rejection direction (any mismatch ⇒ `is_err()`) alongside the accept-path property. |

## WEAK — trailing-byte / truncation (concrete payload batteries)

Symbolically vary only the trailing byte (or nothing); instruction payloads concrete. Valuable
regression coverage of the trailing-byte/length checks, not symbolic over content/length:
`kani_v16_*_reject_trailing_byte` (1007/1039/1067/1104/1179/1250), and
`kani_v16_unknown_or_truncated_tags_reject` (1304 — tag IS symbolic; tighten its hand-maintained
exclusion list to the exact single-byte-acceptable tags to remove drift risk).

## UNIT TEST (acceptable as base/regression)

| Proof | Line | Reason |
|---|---|---|
| `kani_v16_zero_length_decode_rejects` | 1352 | concrete empty slice |
| `kani_v16_every_active_payload_rejects_one_byte_truncation` | 1358 | ~35 hardcoded (len−1) buffers, no symbolic input |
| `kani_v16_decode_rejects_trailing_bytes` | 994 | tag concrete (1), one trailing byte symbolic |

## STRONG (5)

- `kani_v16_premium_funding_rate_is_clamped_and_signed` (11) — symbolic; reaches all branches
  (cap==0, mark==index, mark≷index, **both sides of `min(premium,cap)`**); asserts `|rate|≤cap`,
  zero on degenerate branches, correct sign.
- `kani_v16_update_authority_decode_preserves_wire_fields` (591) — full 32-byte pubkey + u8 kind, no collapse.
- `kani_v16_update_liquidation_fee_policy_...` (645), `kani_v16_update_maintenance_fee_policy_...` (661),
  `kani_v16_update_fee_redirect_policy_...` (719) — single genuine `u16` field, full coverage.
- (`kani_v16_update_trade_fee_policy_...` (703) draws `trade_fee_base_bps: u64 = kani::any()` full-width —
  the model the other decode proofs should follow.)

## Cross-cutting

1. Systemic Category-C widening is the headline; fix is uniform and cheap (`as WIDE` → `: WIDE = kani::any()`).
2. The matcher proof proves only the accept direction; add the rejection direction + full-width sizes.
3. Trailing/truncation proofs are concrete batteries — useful regressions, low symbolic value.
4. **No vacuity, no soundness gaps**: every assertion is reachable and the source is correct against
   the asserted properties. The weaknesses are coverage breadth (high bytes, rejection paths), not false proofs.

## Remediation status — DONE
- Category-C decode proofs (~15): widening replaced with full-width `kani::any()` (u64/u128/i128/u32);
  dead `_raw` seeds removed. The `unit_scale` (u32) and the i128 `funding_rate_e9`/`size_q` and all
  `u128` amounts are now exercised at full width.
- Matcher proof (`kani_v16_matcher_return_accepts_only_bound_echoed_fills`): rewritten to draw the
  echoed fields and abi_version INDEPENDENTLY of the bound params (sizes full-width i128), asserting
  BOTH the rejection (binding) direction and the accept direction, plus `kani::cover!(is_ok())` for
  non-vacuity.
- Trailing-byte/truncation batteries and UNIT-TEST base cases: left as-is (acceptable regression coverage).
- **Full wrapper Kani re-run: 34/34 harnesses verified, 0 failures.** Worst-case proof time ~59s
  (full-width InitMarket); the matcher cover property is satisfied (accept path reachable).
