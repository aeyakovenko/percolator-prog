# User DoS / LoF audit — autonomous find-or-disprove log

## FINAL SUMMARY (D + E FIXED & green; F + G open program bugs)

The user-DoS/LoF sweep converged. **4 confirmed bugs**, each with a LiteSVM proof.
**D + E are FIXED** in the engine (`b6e23b3`, `f9af174`) and their proofs are now GREEN
regressions in the default suite. **F + G are open PROGRAM (wrapper) bugs** — their RED
proofs are `#[ignore]`'d so the default suite stays green (run `cargo test --test v16_cu
-- --ignored`); both are fixable directly in `percolator-prog` (no engine round-trip).
Everything else disproven or verified sound with line-refs (see ledger below).

| # | Severity | Summary | RED test | One-line fix |
|---|---|---|---|---|
| **D** | HIGH | Insolvent resolved market un-drainable: a haircut winner's payout receipt never finalizes (`finalized` needs `paid==full face`, but paid caps at `floor(face·rate)`), so the portfolio can't dematerialize → `WithdrawInsurance`/`CloseSlab` blocked forever. Validates the bounty report. | `v16_audit_insolvent_resolved_winner_can_dematerialize` (GREEN) | **FIXED** engine `b6e23b3` (clear fully-diluted receipt at terminal rate) |
| **E** | HIGH | `CureAndCancelClose` leaves `close_progress=canceled` (never reset to EMPTY); withdraw requires EMPTY → flat solvent user frozen in Live mode. `Deposit` isn't gated on it → permanent capital sink. | `v16_audit_withdraw_after_cure_and_cancel_close` (GREEN) | **FIXED** engine `f9af174` (withdraw allows an inert canceled ledger) |
| **F** | market DoS (program) | Permissionless retired-slot reuse (`v16_program.rs:8651`) accepts zero domain authorities (append path rejects at `:1475`) → that domain's insurance is un-withdrawable → `CloseSlab` bricked. | `v16_audit_permissionless_reuse_rejects_zero_insurance_authority` | add the `== [0u8;32] → InvalidInstruction` check at `:8651` |
| **G** | market DoS (program) | `close_resolved` charges a maintenance fee into `group.insurance` with NO per-domain budget credit; `WithdrawInsurance` caps at Σ domain budgets, so the fee is un-withdrawable by anyone → `CloseSlab` bricked. Corrects the earlier "lockstep" false-negative. Mainnet-confirmed (AWCZ2pK). | `v16_audit_resolved_maintenance_fee_insurance_stays_recoverable` | domain-credit the close-resolved fee (mirror SyncMaintenanceFee) and/or let admin sweep aggregate insurance in Resolved mode |

**Engine vs program:** D, E are ENGINE bugs (`../percolator`); F, G are PROGRAM bugs (`percolator-prog/src/v16_program.rs`, fixable directly without an engine round-trip).

Wrapper pinned at engine `b6e23b3`. Default suite: 69 pass, 2 ignored (F, G — open PROGRAM bugs).
D + E fixed and now GREEN regressions; F + G are wrapper-side and still open.

---

Running log of the user-facing denial-of-service / loss-of-funds sweep. Each
candidate is verified or disproven with a LiteSVM test in `tests/v16_cu.rs`
(prefix `v16_audit_`).

**Convention**
- CONFIRMED bug → keep a RED test asserting the CORRECT behavior, marked
  `#[ignore = "RED until <fix> (Finding X)"]` so the default suite stays green.
  Run all with `cargo test --test v16_cu -- --ignored`. Un-ignore when fixed.
- DISPROVEN candidate → keep a GREEN regression test asserting the safe behavior.
- Engine-source fixes are NOT pushed here (they need the engine author's Kani
  verification); this log records the finding + proposed fix.

All findings below are ENGINE bugs (`../percolator/src/v16.rs`); the wrapper only
dispatches.

---

## CONFIRMED

### Finding D — insolvent resolved market permanently un-drainable (HIGH)
Test: `v16_audit_insolvent_resolved_winner_can_dematerialize` (RED, ignored).
A haircut winner (`residual < positive-PnL face` → payout rate < 1) is paid
`floor(face·rate) < face`. `receipt.finalized` is set only when
`paid_effective == terminal_positive_claim_face` (full face, `v16.rs:2667`), so it
never finalizes. `portfolio_view_is_closable` stays false → `materialized_portfolio_count`
stuck ≥ 1 → `WithdrawInsurance` and `CloseSlab` blocked forever. Insurance + backing
earnings + residual vault + ~0.8 SOL rent stranded. Unprivileged-reachable (bad-debt
counterparty, permissionless resolve). **Validates the `/tmp/bug.md` bounty report's
symptom** (they mis-named the cause as RefineResolvedUnreceiptedBound).
Fix: finalize at the haircut entitlement (`paid_effective == floor(face·final_rate)`
when the rate is terminal), or add a permissionless "abandon fully-diluted receipt".

### Finding E — CureAndCancelClose permanently freezes user withdraw (HIGH, user DoS)
Test: `v16_audit_withdraw_after_cure_and_cancel_close` (RED, ignored).
After a user cures+cancels a forced close, `close_progress` is left `canceled`,
never reset to `EMPTY` (`v16.rs:11220`). `withdraw_not_atomic` requires
`close_progress == EMPTY` (`v16.rs:11315`), so a flat, solvent user can never
withdraw their capital again in Live mode (recoverable only if the market resolves).
Confirmed: withdraw reverts `0x15` (EngineLockActive) post-cure. Reachable: a domain
close barrier opens on counterparty bankruptcy → user CureAndCancelClose → closes
position (clear_leg leaves close_progress untouched) → flat but frozen.
Fix: reset `close_progress` to EMPTY once the cancel barrier is consumed and no leg
references it, or have withdraw treat a `canceled`/inert ledger as withdrawable.

**E is worse than just the cured capital — it's a permanent capital SINK.** `Deposit`
does NOT gate on `close_progress` (`deposit_not_atomic`, `v16.rs:11613`, only
validate_with_market which permits a non-EMPTY ledger), so the user can keep
depositing into a canceled-close account, and every later deposit is also frozen.
`ClosePortfolio` is blocked too because it requires `capital == 0` but cure restored
positive capital. Root cause: `close_progress` has FOUR engine writers
(`begin`/`advance`/`advance_quantity_adl`/`cure`, none writes EMPTY post-init), so once
non-EMPTY it stays non-EMPTY for the account's life.

### Finding G — resolved-mode fee strands insurance (program bug, market DoS) [CONFIRMED + LiteSVM repro]
Test: `v16_audit_resolved_maintenance_fee_insurance_stays_recoverable` (RED, ignored).
`handle_close_resolved` charges an accrued maintenance fee into `group.insurance`
(`close_resolved_account_not_atomic(.., cfg.maintenance_fee_per_slot)`, v16_program.rs:9899)
but the wrapper does NOT credit any per-domain budget for it (no `credit_*_view` call in the
handler). `WithdrawInsurance` caps each authority's claim at Σ(domain budget remaining)
(`terminal_insurance_remaining_for_authority_view`, v16_program.rs:4879), not `group.insurance`,
so this fee is withdrawable by NOBODY (even admin) and permanently blocks `CloseSlab`
(requires `insurance==0`). Repro: market with `maintenance_fee_per_slot=5`, deposit, warp 100
slots, resolve, close → `insurance=500`, `Σ domain budgets=0` → 500 stranded. **PROGRAM bug**
(wrapper `v16_program.rs`); engine not involved. **CORRECTS the earlier "insurance/domain
lockstep — REFUTED" false-negative below**: lockstep holds for TRADE fees and `SyncMaintenanceFee`
(which domain-credit) but NOT for `close_resolved`'s maintenance fee. Confirmed by mainnet
evidence (market AWCZ2pK: 4060 lamports stranded, every authority = admin) in /tmp/bug.md.
**Fix:** domain-credit the close-resolved maintenance fee (mirror SyncMaintenanceFee's
`credit_maintenance_fee_to_active_market_budgets_view`), and/or let admin sweep aggregate
`group.insurance` in Resolved mode when `Σ domain budgets < insurance`. Both wrapper-side.

### Finding F — permissionless retired-slot reuse accepts zero domain authorities → stranded insurance (market DoS) [CONFIRMED + LiteSVM repro]
Test: `v16_audit_permissionless_reuse_rejects_zero_insurance_authority` (RED, ignored) —
reuses a retired slot with `insurance_authority = 0` and asserts rejection; RED because
the instruction ACCEPTS it.
The append path validates all four domain authorities are non-zero
(`activate_dynamic_asset_slot`, `v16_program.rs:1475-1481`), but the permissionless
retired-slot REUSE branch of `handle_update_asset_lifecycle` (`v16_program.rs:8651-8656`,
reached when `!still_asset_authority && asset_index < configured_slots &&
free_market_slot_count != 0` on a RETIRED slot) writes them straight from caller args
with NO zero-check; `write_oracle_profile_to_view_if_separate`→`validate_asset_oracle_profile`
never inspects the authority fields. With `insurance_authority == 0`, fees accrued to that
asset's domain are withdrawable by nobody (`terminal_insurance_remaining_for_authority_view`
rejects a zero authority, `v16_program.rs:4885`), and `CloseSlab` (requires `insurance==0`)
is permanently bricked. Attacker-reachable in a permissionless-init market (pay the init
fee, reuse a retired slot with zero authorities). Direct verification: line 8651 is missing
the 1475 guard. **Fix:** add the same `== [0u8;32] → InvalidInstruction` check at 8651.
(A secondary admin self-footgun variant: `UpdateAuthority` burns insurance/backing/mark
authorities to zero with no recovery — MEDIUM.)
LiteSVM repro deferred (needs append→retire→reuse→accrue→resolve chain); code gap is
unambiguous.

---

## PENDING (next batches)
- Full LiteSVM repro for Finding F (retired-slot reuse with zero authority).
- Deeper combination / novel-vector hunt: chunked B-settlement edge cases, ADL
  interactions, multi-asset resolved wind-down with mixed lifecycles.

## DISPROVEN / FALSE POSITIVES (traced read-only with resetter line-refs)
User value-extraction gating fields — all but close_progress/receipt have a reachable
resetter or are unreachable-positive in this revision:
- `close_progress.finalized` — implies bankrupt account => `capital == 0` (negative pnl
  consumed first, `v16.rs:10087`); `ClosePortfolio` gates on `has_pending_residual()`
  which is false when finalized -> closable. Not a freeze.
- `reserved_pnl` — only written down/zero (`v16.rs:7016`, `11544`); never set positive
  from zero this revision. Not a freeze.
- `cancel_deposit_escrow` — only ever written to 0 (`v16.rs:11217`, `12306`); dead.
- `stale_state` / `b_stale_state` — permissionless resetters `clear_account_stale`
  (`v16.rs:7397`) / `clear_account_b_stale` (`v16.rs:7698`) via crank-refresh.
- `source_claim_bound_num` — Resolved burn releases liens (`v16.rs:5777`) -> zeroed in
  close_resolved.
- `ConvertReleasedPnl` — Live with no source claim returns 0 -> `Err(LockActive)`, PnL
  NOT lost (stays claimable at resolution); haircut math symmetric with the receipt path.
  No loss/freeze.
- Maintenance/trade/liq fees — `charge_account_fee_current_not_atomic` caps at
  `min(fee, capital)` and skips when `pnl<0` (`v16.rs:10665`); never drives capital
  negative; user always has a permissionless exit (ResolveStalePermissionless,
  risk-reducing trade, CloseResolved).

Verified SOUND (deep read-only trace, no bug):
- TradeCpi external matcher — taker (account_a) fill bound by user limit price
  (`v16_program.rs:6392-6401`); both parties sign+own (`:6270`, `:6347`); matcher return
  identity-bound (req_id/lp/oracle/asset echoed, `:3613-3660`); fee user-signed + capped
  (`:6338`); no reentrancy (slab not passed, mutate after CPI). Can't extract beyond
  two-party consent.
- Multi-asset resolved wind-down, mixed lifecycles — `close_resolved` settle path is
  lifecycle-agnostic (`v16.rs:7914-7942`); a side resets at most once (no mode→Normal
  reset; `:9869`/`:9894`) so the epoch-mismatch strand is closed; forfeit covers every
  reachable dead-leg combo (`:11721`); Retired assets can't carry legs (retire requires
  empty). Asset-lifecycle `Recovery` is never assigned (dead but harmless). The
  pending_domain_loss_barrier co-leg freeze is the SAME mechanism as Finding D, not new.

- Dust/rounding LoF on deposit→withdraw — DISPROVEN: deposit/withdraw are exact 1:1
  (`amount_to_u64` pure cast, no scale); existing `v16_bpf_deposit_and_withdraw_move_spl_tokens_with_ledger`
  proves exact round-trip (deposit 1000 → withdraw 400 → dest=400, vault/c_tot/capital exact).
  Default new_account_fee/maintenance_fee = 0; backing BOUND_SCALE top-up/withdraw are exact
  inverses. No sub-unit dust path.

Other disproven (prior passes):
- Insurance/domain-budget lockstep — PARTIALLY REFUTED, see Finding G (FALSE NEGATIVE
  corrected): lockstep holds for trade fees + SyncMaintenanceFee (domain-credited), but
  `close_resolved`'s maintenance fee credits aggregate `group.insurance` WITHOUT a domain
  budget -> stranded. Aggregate insurance can exceed Σ domain budgets after a resolved close.
- Dead-code reservations: `insurance_credit_reserved_num`, `impaired/consumed_liened_backing`,
  `expire_source_backing_bucket`, standalone `add_source_positive_claim_bound` — no caller.
- Funding zero-sum / health-cert epoch staleness — verified safe.
- Secondary-mint outbound-either-mint — by design; amount never shorted, only mint quality.
- (Process) earlier "bounty report is a misdiagnosis with escapes" — FALSE NEGATIVE;
  the symptom is the real Finding D.
