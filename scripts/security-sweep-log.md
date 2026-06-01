# security.md LiteSVM sweep log

Autonomous adversarial sweep per `scripts/security.md` (the loop + 49-category failure
checklist + attack-pattern library). For each candidate: one LiteSVM test asserting the
attacker success criterion against the production BPF binary.

**Disposition rule (per the user):** a test that reveals a REAL coverage gap / bug is KEPT
(RED `#[ignore]` if unfixed, or GREEN once fixed). A PASS_SAFE candidate that doesn't reveal
a bug is DELETED and recorded below as a checked-safe / false-positive candidate, then the
scan continues. All writes stay inside `percolator-prog`.

Engine bugs found earlier (A–G) are all fixed. Wrapper proof-strength audit done separately.

## Status: RUNNING (complex-path frontier)
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
| 13 | conservation under price move (#9/#33) — MTM settlement | move auth-mark, crank; probe whether winner gain is lost / value created or destroyed | PASS_SAFE (KEPT as regression) | initial probe fired (narrow PnL-zero-sum invariant + incomplete settlement: winner under-credited before loser funds residual). After winner->loser->winner cranking with the WIDENED invariant (total equity conserved, vault>=c_tot+insurance, +pnl<=residual) it converges exactly — §6.1/§6.2 warmup is order-robust. Kept as `v16_regression_mark_to_market_settles_conservation_under_price_move`. |
| 14 | profit realization round-trip (#33) — open/mark-up/close | value created/destroyed on profit close | PASS_SAFE (KEPT regression) | total equity conserved, flat, senior conservation, +pnl backed. `v16_regression_profit_realization_roundtrip_conserves` |
| 15 | numerical boundary (#37/#38) — i128::MIN/MAX trade size | overflow/panic on extreme size | PASS_SAFE (KEPT) | all rejected cleanly, no panic, no OI, no capital moved. `v16_attack_extreme_size_trade_rejected_no_panic` |
| 16 | value extraction (#33/#35) — profit withdraw through real token vault | winner withdraws unbacked profit / prints tokens (out > deposited) | PASS_SAFE (KEPT regression) | after profit/settle/close, each leg withdraws full capital; total tokens out <= 2_000_000 (no value printed), vault >= c_tot+insurance. Unconverted profit stays in pnl (warmup), not withdrawable. `v16_regression_profit_withdraw_no_value_printed` |
| 17 | injection (#19/#39) — caller-supplied funding_rate_e9 / recovery_reason on permissionless crank | attacker injects arbitrary funding to drain counterparty | PASS_SAFE (KEPT regression) | gate at v16_program.rs:10092 rejects ANY nonzero caller funding_rate_e9 or recovery_reason (tested i128::MAX/MIN/±1, rr=1). Real rate derived internally via premium_funding_rate_e9, clamped to max_abs_funding_e9_per_slot (:11284). rate-0 crank conserves. `v16_attack_crank_caller_funding_rate_injection_rejected` |
| 18 | cross-margin (#22/#32) — one portfolio, two assets | shared-capital cross-margin breaks aggregate conservation / OI | PASS_SAFE (KEPT regression) | open positions on asset0 + asset1 from same portfolio; c_tot==4M (no value created), vault>=c_tot+insurance, both assets' OI balanced. `v16_attack_cross_margin_two_asset_conservation` |
| 19 | cross-margin settlement (#9/#33) — asset0 up, asset1 down on one portfolio | divergent cross-asset moves create/destroy value | PASS_SAFE (KEPT regression) | gain on asset0 + loss on asset1 net to total equity == 4M after two crank passes; vault>=c_tot+insurance, +pnl backed. `v16_attack_cross_margin_divergent_moves_conserve` |
| 20 | account confusion (#44/#45) — wrong-type accounts as portfolio | market/vault/uninitialized account substituted for portfolio drains state | PASS_SAFE (KEPT regression) | withdraw w/ market-as-portfolio, trade w/ vault-as-portfolio, crank w/ system-account-as-portfolio all rejected; c_tot + vault unchanged. `v16_attack_account_type_confusion_rejected` |
| 21 | loss-of-funds / DoS (#22/#30) — withdraw after long fee accrual | maintenance-fee debt locks user out of remaining capital | PASS_SAFE (KEPT regression) | after fees accrue over 500 slots and sync, user withdraws full post-fee capital to a token account; capital->0, senior conservation holds. No LoF. `v16_attack_fee_accrual_does_not_lock_user_funds` |
| 22 | insolvency / bad-debt (#9/#33/#19) — loser driven underwater past capital | winner's profit paid from vault past backing (value printed to cover bad debt) | PASS_SAFE (KEPT regression) | small-capital short driven insolvent (capital->0) over 2 slots to effective_price>=300, then liquidated; vault >= c_tot+insurance throughout, winner positive pnl capped by residual (bad debt socialized via haircut, not printed), no capital conjured. `v16_attack_insolvency_bad_debt_is_socialized_not_printed` |
| 23 | insurance backstop (#33/#9) — bad debt vs pre-funded insurance | insurance underflow/wrap or vault over-credit while absorbing bad debt | PASS_SAFE (KEPT regression) | 1M insurance seeded, short driven insolvent; insurance only spent (<= before, no wrap), vault not over-credited (<= before), senior conservation holds with insurance accounted. `v16_attack_insurance_backstop_absorbs_bad_debt_no_underflow` |
| 24 | debtor escape / winner LoF (#22/#48) — insolvent loser withdraws | underwater loser extracts value before liquidation, stranding winner | PASS_SAFE (KEPT regression) | short driven insolvent then attempts withdraw(1/100/250) -> all rejected, zero tokens leaked, vault untouched. Winner claim preserved. `v16_attack_insolvent_loser_cannot_withdraw_to_escape` |
| 25 | premium-funding zero-sum (#33/#9) — funding accrual value-conservation | funding creates/destroys net value or leaves winner unbacked | PASS_SAFE (KEPT regression; **probe fired, investigated**) | Initial probe fired on a too-narrow `Σ(capital+pnl)==deposits` invariant (got 19,764,100 vs 20,000,000). Diagnostic showed value fully conserved: vault==20M (no tokens minted/burned), funding fees accrue to insurance (200k), §6.2 warmup holds an in-vault residual (35,900), winner pnl (464,097) <= residual (499,997). Mark premium also drives effective_price so funding/price PnL are entangled (not isolable). Widened invariant: vault==deposited, senior conservation, winner backed, no over-distribution, longs-pay-shorts direction — all hold. Same false-positive class as candidate 13. `v16_regression_premium_funding_settlement_conserves_vault` |
| 26 | liquidation reward extraction (#19/#33) — cranker-reward drains insurance/prints value | liquidator extracts reward breaking senior conservation | PASS_SAFE (DELETED — redundant) | Area already covered by `v16_bpf_permissionless_liquidation_is_bounded`, `..._cranker_reward_liquidation_rejects_invalid_shape_without_paying_reward` (rollback on bad shape, no reward paid), `..._no_cranker_liquidation_rejects_invalid_final_market_shape`, and `..._oracle_liquidation_uses_only_its_own_domain_insurance` (per-domain insurance bound). A valid nonzero-fee state can't be built via mutate_market (insurance_domain_budget is a 0-or-MAX_VAULT_TVL flag per engine v16.rs:3910; arbitrary values fail validate_shape) and there's no BPF setter for liquidation_fee_bps. New test would only re-cover existing ground — deleted. |
| 27 | §6.2 profit conversion (#33/#35) — ConvertReleasedPnl caller `amount` cap | caller converts more parked pnl to withdrawable capital than the engine released (prints capital) | PASS_SAFE (KEPT regression) | source-backed released pnl (40) via backing bucket: huge cap (1e9) converts EXACTLY 40 (engine-bounded, not more); under-cap (39) rejects entirely (wrapper rejects when engine-released > cap — no partial); zero-amount rejects; senior conservation holds, no vault tokens move. Initial counterparty-residual attempt mis-set (anti-retroactivity → pnl unbacked → engine correctly rejected convert); reworked onto the source-credit path. `v16_attack_convert_released_pnl_respects_caller_cap` |
