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

---

## PENDING (to verify/disprove)
- Maintenance-fee drain on a position the user can't exit (stale oracle / drain-only).
- Haircut under-paying a winner who can't realize (ConvertReleasedPnl / resolved).
- Deposit trapped behind a reservation / mode the user can't exit.
- Trade/liquidation counterparty griefing extracting user value.
- Third-party making a user's portfolio un-closable.
- Position un-exitable due to stale/bad oracle (no liquidation/resolve escape).
- Zero-authority strandings: permissionless retired-slot reuse (`v16_program.rs:8651`)
  + UpdateAuthority burn — market DoS, not strictly user; verify.

## DISPROVEN / FALSE POSITIVES (do not re-report as live bugs)
- Insurance/domain-budget arithmetic mismatch — REFUTED: strict lockstep on every
  credit/spend; aggregate insurance == Σ domain budgets.
- Dead-code reservations: `insurance_credit_reserved_num`, `impaired/consumed_liened_backing`,
  `expire_source_backing_bucket`, standalone `add_source_positive_claim_bound`,
  `cancel_deposit_escrow` funding — no production caller; latent only.
- Funding zero-sum / health-cert epoch staleness — verified safe in prior passes.
- Secondary-mint outbound-either-mint — by design (documented base-unit path); amount
  never shorted, only mint quality.
- (Process) earlier "bounty report is a misdiagnosis with escapes" — FALSE NEGATIVE;
  the symptom is the real Finding D.
