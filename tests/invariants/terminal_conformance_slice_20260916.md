# PR135 terminal conformance evidence: rows 417/419/421/435

## Decision

All four rows have existing executable PR135 coverage for their bounded public
mechanisms and are recorded as `independent-discovery`. No additional economic
test is needed. This change adds exact discovery fingerprints and updates the
INV-079 registry checks; it does not adapt a finding-specific reproducer.
The generators and economic oracles never consume finding IDs or ledger data.

All four broader `coverage_reopenings.tsv` rows remain `OPEN`, and invariant
machine verdicts remain unchanged. The existing engine #202 / wrapper #440
dependency for rows 419/435 is not resolved by this audit. No vulnerable/fixed
comparison, current defect, severity acceptance, universal liveness or
whole-invariant completion is claimed. A green finite conformance product is
not a `nonqualifying` disposition of the historical finding.

## Public construction and scope

The audit inspected the charter, benchmark/reopening ledgers, invariant README,
INV-039/067/070/073 owners and their mounted children, row417 receipt/rail/closure
notes, row419/435 mixed-role/ADL/underfunded-receipt notes, and row421
restoration/booked-residue/partial-recredit notes.

All five selected tests create economic state through System, SPL Token, ATA
and wrapper instructions. Native histories install only the external native
mint genesis account omitted by LiteSVM. Program loading, wallet airdrops and
Clock advancement are runtime scaffolding. No Percolator-owned account bytes,
private engine transitions or installed simulation outputs construct the states.
Invalid destinations and rejected suffixes are explicit negative controls;
successful economic histories use valid accounts.

### Row 417: INV-067 receipt identity and exact-once payout

Mapped owner: [receipt CloseSlab rail product](cu/inv_067_receipt_close_slab_rail.rs),
mounted beneath INV-067's `receipt_rail_liquidity::rail_history::close_slab`.

Eight histories cross classic/native secondary rails, the eagerly paid claimant
and final payment rail. Two receipts have faces 700/1300 and an independent
source claimant has face 1000. Public late expiry changes residual stock from
501 to 662 to 851. The input oracle uses `face * residual / 3000`; full receipt
images preserve identity and only cumulative paid counters may advance.

The original requests survive late normalization, peer payment, receipt clear,
portfolio deletion and slab closure without repaying value. Final user payouts
are `[1198, 0, 1283, 0, 1368, 0]`, rounding burn is exactly two atoms, and rail
custody, mint supply, rent and native lamports reconcile. Full Account rollback
checks include successful SPL payout and tombstone prefixes. This also supplies
INV-070 terminal-disposition evidence, without asserting arbitrary future stock
reclassification or maximum-shape coverage.

### Row 419: INV-039 pending debt through resolution

Mapped owner: [funded pending resolution](cu/inv_039_pending_loss_funded_resolution.rs),
mounted beneath INV-039's `resolved_histories::funded_resolution`.

Sixteen histories cross mirrored price/funding signs, debtor order, claimant
order and delayed settlement. Public Recovery forfeits create zero-basis pending
creditor legs while debtors retain their original exposure. Resolution preserves
those obligations. Resolved detachment cannot pay a creditor before its debtor
settles; a successful unrelated debtor prefix followed by a waiting creditor
must roll back completely. Debtor deletion cannot erase the unpaid peer domain.

The independent price/funding book derives debts of 19999/39998 and checks each
owner's capital, PnL, receipt and SPL payout. Every portfolio ultimately exits.
The row435 product below separately covers bankruptcy B booking before/after
resolution with still-pending creditor weight. These finite schedules do not
close all bankruptcy/funding/maintenance/source-conversion combinations or
verify the pending engine #202 / wrapper #440 fix line.

### Row 421: INV-073 public insurance payout and INV-070 disposition

Mapped INV-073 entry point:
`v16_program_terminal_public_reserve_disposition_preserves_value_across_orders`
in [the invariant owner](cu/inv_073_no_permanent_user_lock.rs), executing
[the shared public reserve product](cu/inv_073_terminal_public_reserves.rs).
This is the same existing mechanism evidence already accepted for rows 420/433.

Twelve histories cross all six principal/earnings/insurance payment orders with
fresh/expired backing. Provider and beneficiary keys are dropped before partial
unsigned payouts. Separate stock classes and recipient Account images check
that insurance reaches its actual beneficiary, provider value stays separate,
failed suffixes roll back payment, and signed final close burns only expired
principal and refunds exact rent. Admin token custody receives no reserve value.

The [disabled-custody recredit child](cu/inv_073_partial_recredit_disabled_custody.rs)
adds four histories with classic/native quote and recovery of 37/72 from 73 spent
insurance atoms. Absent beneficiary/provider keys cannot repair delegated
custody or consent to succession. The keeper creates valid replacement custody,
pays the same beneficiary exactly the recovered stock, preserves its earlier
176 paid atoms, separately pays 657 provider-fee atoms and reaches signed slab
closure with zero custody. Replayed stale-epoch payouts reject exactly.

Economic payments need only the independent payer. Setup and role handoffs,
empty-portfolio deletion, native redemption and mechanical slab closure retain
their actual required signers. This does not establish permissionless mechanical
closure or every publicly reachable funded state's exit path.

### Row 435: INV-039 mixed creditor/debtor attribution

Mapped owner: [mixed-role resolution](cu/inv_039_mixed_role_resolution.rs),
mounted directly beneath INV-039.

Thirty-two histories cross debts 36000/240000, both sides, both asset assignments,
Live/Resolved residual booking and both close orders. One portfolio is a pending
creditor on one asset and an unsettled debtor on another. Its independent book
requires original debt, the 20000 bankruptcy debit and a separate source-face
discount to remain attributed to that owner, including while settlement waits.

With `S = min(debt, 180000)` and `H = S * 200000 / 180000 - S`, final payouts
must be `[580000 - debt - H, 0, 300000 + debt, 250000, 777]`. Residue is exactly
`H`, and all five portfolios delete. Domain-local source ownership, pending
counts, loss weights and exact paid-receipt retries rule out conserved-total
owner swaps. This product has zero fees/funding and integral source rates;
broader compositions and the focused fix dependency remain OPEN.

## Provenance and exact validation

- Worktree: `/dev/shm/percolator-pr135-terminal-20260916-9f2c`.
- Branch: `codex/pr135-terminal-conformance-20260916-9f2c`.
- Base: `5c870324` from `codex/astra-invariant-cycle-20260915`.
- Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Reused SBF: `/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so`.
- SHA-256: `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
- The lane24 build base `d64f049005847848b095b3b8b2d21318d0504296` has no diff
  against this slice's base in `src`, `Cargo.toml` or `Cargo.lock`. No new SBF
  build is claimed. The private host target was copied from the existing ADL
  lane cache; Cargo rebuilt the selected harness from this worktree.
- No checkout files in `/home/anatoly/percolator-prog` were edited. Production,
  dependency pins, fixtures and INV-005/045 files are unchanged.

```sh
cd /dev/shm/percolator-pr135-terminal-20260916-9f2c
export CARGO_TARGET_DIR=/dev/shm/pr135-terminal-9f2c-target
export TMPDIR=/dev/shm/pr135-terminal-9f2c-tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_rail_liquidity::rail_history::close_slab::v16_program_partial_receipts_cannot_repay_across_owner_deletion_and_close_slab_rails \
  inv_039_pending_loss_obligation_durability::resolved_histories::funded_resolution::v16_program_funded_pending_debt_survives_resolution_and_delayed_close_orders \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::v16_program_mixed_creditor_debtor_roles_preserve_owner_entitlement_through_resolution \
  inv_073_no_permanent_user_lock::v16_program_terminal_public_reserve_disposition_preserves_value_across_orders \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::terminal_progress_product::booked_residue_beneficiary_epochs::partial_recredit_disabled_custody::v16_program_partial_recredit_replaces_disabled_custody_without_succession_consent

cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_dated_open_security_finding_benchmark_is_non_overclaiming \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming

rustfmt --edition 2021 --config skip_children=true --check tests/invariants/public_sbf/inv_079_public_reachability_evidence.rs
git diff --check
git diff --exit-code 5c870324 -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/**/inv_005*' ':(glob)tests/invariants/**/inv_045*'
```

Runtime result: **5/5 passed**, 72 histories, zero failures/ignored tests,
53.07 seconds. Logs are in `/dev/shm/pr135-terminal-9f2c-logs/conformance.log`.
Registry validation: **4/4 passed**, zero failures/ignored tests, 0.54 seconds;
log: `/dev/shm/pr135-terminal-9f2c-logs/metadata.log`. The updated benchmark has
147 independent mappings, 17 nonqualifying rows and two missing rows. All four
audited reopenings remain OPEN, with machine verdicts unchanged. Scoped rustfmt,
whitespace and protected-path diff checks pass. Only existing host dead-code
warnings and the `solana-client` future-compatibility warning were emitted.

| Product | Histories | Measured peak CU | Additional checks reported |
| --- | ---: | ---: | --- |
| Receipt rail/deletion/slab | 8 | 241484 | 312 exact rollbacks, 56 paying-receipt rollbacks, 32 tombstone rollbacks |
| Funded pending debt | 16 | 177388 | 96 exact rollbacks, 80 payouts, 80 portfolio deletions |
| Mixed creditor/debtor | 32 | 206059 | 24 exact waiting rejections, 32 paid-receipt retries |
| Unsigned reserves | 12 | 234435 | All six payment orders with fresh/expired principal |
| Disabled custody/recredit | 4 | 250635 | 48 exact rollbacks, unchanged earlier payment, replacement-custody payout |

Only directly relevant selectors are run; this is not a full-suite result.
