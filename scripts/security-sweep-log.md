# security.md LiteSVM sweep log

Autonomous adversarial sweep per `scripts/security.md` (the loop + 49-category failure
checklist + attack-pattern library). For each candidate: one LiteSVM test asserting the
attacker success criterion against the production BPF binary.

**Disposition rule (per the user):** a test that reveals a REAL coverage gap / bug is KEPT
(RED `#[ignore]` if unfixed, or GREEN once fixed). A PASS_SAFE candidate that doesn't reveal
a bug is DELETED and recorded below as a checked-safe / false-positive candidate, then the
scan continues. All writes stay inside `percolator-prog`.

Engine bugs found earlier (A–G) are all fixed. Wrapper proof-strength audit done separately.

## Status: CONVERGED (standard-pattern pass)
13 adversarial LiteSVM candidates across the 49-category checklist + attack-pattern library —
**all PASS_SAFE, 0 findings**. The protocol robustly rejects the standard attacks (double-init,
over-leverage, self-trade, non-flat/over-draw withdraw, frozen-time writes, zero-price fills) and
maintains conservation (c_tot==Σcapitals, vault==c_tot+insurance, OI balanced) through trade
open/close. The real bugs in these categories (Findings A–G) were already found and FIXED by the
earlier agent-driven sweeps. Per the security.md stop rule (6 consecutive PASS_SAFE batches), the
brute-force LiteSVM loop has converged. Deeper complex-path probing (liquidation-under-insolvency,
ADL/haircut precision, funding-accrual zero-sum, TradeCpi matcher) needs elaborate multi-step
setups and was covered analytically by the prior sweeps; those are the remaining frontier.

## Confirmed findings (kept tests)
_(none this sweep — all candidates PASS_SAFE; real bugs A–G were found+fixed earlier.)_

## Checked-safe / false-positive candidates (deleted tests)

| # | Candidate (failure-mode / attack pattern) | Attacker model | Result | Why safe |
|---|---|---|---|---|
| 1 | idempotency gap — double SyncMaintenanceFee at same slot | permissionless caller syncs maintenance fee twice at the same slot to double-charge | PASS_SAFE | 2nd same-slot sync is a no-op (last_fee_slot already advanced; delta=0). Capital/insurance/vault unchanged. (Note: identical txs hit LiteSVM `AlreadyProcessed` dedup — must `expire_blockhash()` to test distinctly.) |
| 2 | double-init (#45) — re-init a funded portfolio | attacker re-sends InitPortfolio on an initialized, funded portfolio to reset/wipe state | PASS_SAFE | re-init rejected; capital (1000) unchanged. |
| 3 | reclaim-non-flat (#48) — ClosePortfolio with capital | attacker closes a funded portfolio to reclaim rent / strand capital | PASS_SAFE | rejected; capital (1000) unchanged |
| 4 | overdraw (#35) — withdraw > capital | withdraw 200 from a 100-capital account | PASS_SAFE | rejected; capital=100, dest token=0 |
| 5 | frozen-time write (#30) — Deposit on resolved market | deposit into a Resolved market | PASS_SAFE | rejected; vault unchanged |
| 6 | accounting drift (#32/#35) — conservation after trade w/ fee | check c_tot vs Σcapitals + vault vs c_tot+insurance after a fee'd trade | PASS_SAFE | c_tot==Σcapitals, vault==c_tot+insurance, net pnl 0, OI balanced |
| 7 | open/close conservation + OI drift | open then close (opposite trade) | PASS_SAFE | both flat, c_tot==Σcapitals, vault==c_tot+insurance, OI 0 |
| 8 | input validation (#39) — zero exec_price trade | TradeNoCpi at exec_price 0 | PASS_SAFE | rejected; no OI created |
| 9 | LP/identity (#49) — self-trade same account | TradeNoCpi with account_a==account_b (wash) | PASS_SAFE | rejected; no OI |
| 10 | IM bypass (#19/#46) — over-leverage open | 100-capital account opens 100k notional at 100% IM | PASS_SAFE | rejected; no OI |
| 11 | frozen-time (#30) — trade on resolved market | TradeNoCpi after ResolveMarket | PASS_SAFE | rejected; no OI |
| 12 | non-flat withdraw (#22/#48) — withdraw with open position | withdraw 1 while holding a position | PASS_SAFE | rejected; capital + dest unchanged |
