# User DoS / LoF audit — autonomous find-or-disprove log

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

---

## PENDING (market-DoS, lower priority than user-facing)
- Zero-authority strandings: permissionless retired-slot reuse (`v16_program.rs:8651`)
  accepts zero domain authorities (append path rejects at `:1475`); `UpdateAuthority`
  burns insurance/backing/mark authorities to zero. Strands a domain's insurance ->
  CloseSlab bricked. Reachable; verify via LiteSVM.

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

Other disproven (prior passes):
- Insurance/domain-budget arithmetic mismatch — REFUTED: strict lockstep; aggregate
  insurance == Σ domain budgets.
- Dead-code reservations: `insurance_credit_reserved_num`, `impaired/consumed_liened_backing`,
  `expire_source_backing_bucket`, standalone `add_source_positive_claim_bound` — no caller.
- Funding zero-sum / health-cert epoch staleness — verified safe.
- Secondary-mint outbound-either-mint — by design; amount never shorted, only mint quality.
- (Process) earlier "bounty report is a misdiagnosis with escapes" — FALSE NEGATIVE;
  the symptom is the real Finding D.
