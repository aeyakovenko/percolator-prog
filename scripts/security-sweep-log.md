# security.md LiteSVM sweep log

Autonomous adversarial sweep per `scripts/security.md` (the loop + 49-category failure
checklist + attack-pattern library). For each candidate: one LiteSVM test asserting the
attacker success criterion against the production BPF binary.

**Disposition rule (per the user):** a test that reveals a REAL coverage gap / bug is KEPT
(RED `#[ignore]` if unfixed, or GREEN once fixed). A PASS_SAFE candidate that doesn't reveal
a bug is DELETED and recorded below as a checked-safe / false-positive candidate, then the
scan continues. All writes stay inside `percolator-prog`.

Engine bugs found earlier (A–G) are all fixed. Wrapper proof-strength audit done separately.

## Confirmed findings (kept tests)
_(none yet this sweep)_

## Checked-safe / false-positive candidates (deleted tests)

| # | Candidate (failure-mode / attack pattern) | Attacker model | Result | Why safe |
|---|---|---|---|---|
| 1 | idempotency gap — double SyncMaintenanceFee at same slot | permissionless caller syncs maintenance fee twice at the same slot to double-charge | PASS_SAFE | 2nd same-slot sync is a no-op (last_fee_slot already advanced; delta=0). Capital/insurance/vault unchanged. (Note: identical txs hit LiteSVM `AlreadyProcessed` dedup — must `expire_blockhash()` to test distinctly.) |
| 2 | double-init (#45) — re-init a funded portfolio | attacker re-sends InitPortfolio on an initialized, funded portfolio to reset/wipe state | PASS_SAFE | re-init rejected; capital (1000) unchanged. |
| 3 | reclaim-non-flat (#48) — ClosePortfolio with capital | attacker closes a funded portfolio to reclaim rent / strand capital | PASS_SAFE | rejected; capital (1000) unchanged |
| 4 | overdraw (#35) — withdraw > capital | withdraw 200 from a 100-capital account | PASS_SAFE | rejected; capital=100, dest token=0 |
| 5 | frozen-time write (#30) — Deposit on resolved market | deposit into a Resolved market | PASS_SAFE | rejected; vault unchanged |
