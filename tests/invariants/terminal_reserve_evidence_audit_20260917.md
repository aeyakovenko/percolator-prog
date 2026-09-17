# INV-070/073/078 terminal-reserve evidence audit, 2026-09-17

Base: `origin/main` at `205ef676dd8f534cf6a179dc1ddd0ffacb13c4a1`.
Branch: `astra-ultra/terminal-reserve-audit-20260917`.
Worktree: `/dev/shm/astra-ultra-terminal-reserve-audit-20260917`.

## Decision

Rows 418/420/421/433 already have mounted behavioral witnesses. Source review
established no distinct missing test in these named families. The traceability
gap is the cross-owner mapping: row 418's discovery selector lives below an
INV-024 fixture, rows 420/433 share one INV-073 entrypoint, and row 421's discovery
selector lives under INV-070. This audit connects those selectors to their
assertions and authority limits without adding duplicate tests.

Reviewed: `INVARIANTS.md`, the discovery/reopening/status/traceability TSVs,
README, harness mounts, and the test bodies/helpers linked below. Counts and
economic assertions below describe existing source, not new behavioral runs.
Row 418 remains COVERED; rows 420/421/433 remain OPEN. INV-070/073 remain
REFUTED_CURRENT and INV-078 remains OPEN_EVIDENCE.

## Existing Witnesses

| Row / invariant | Selector IDs | Nonvacuous assertion and limit |
| --- | --- | --- |
| 418 / INV-070 | N, C | Twelve native histories finish fee/loss/recredit accounting with nonzero booked residue, then preserve prior payments, send residue to the insurance beneficiary, separate donated surplus/rent, and close to an exact tombstone without changing the native mint. Six custody repairs and rejected redirect/close suffixes are checked. C is the two-world classic-SPL burn control. The insurance payout/redemption prefix uses the insurer signature; only the subsequent residue continuation drops all reserve keys. Administrator-signed expiry/CloseSlab remains required. |
| 420 / INV-073 | P, G | P crosses all six principal/earnings/insurance payment orders with fresh/expired backing: twelve worlds with partial payments, lazy earnings ledger, exact custody/claim accounting, encumbered-destination rejection and payout-prefix rollback. Provider/beneficiary keys are dropped before reserve payout. G checks Live unsigned rejection, outstanding senior-capital rejection, and rejection even after user payout while two empty portfolios remain. Mechanical deletion is a prerequisite, so this is not an arbitrary funded-state liveness proof. |
| 421 / INV-073 | I, R | I pays a nonzero asset-1 insurance claim without the configured authority's signature, rejects substituted authority/destination and funded-role takeover, and checks unsigned Live rejection. R adds four asset/remainder histories where both insurance wallets/custody disappear before loss settlement; keeper-created custody receives the recredited 100/101-atom claim, with exact rollback and 207-atom classic residue burn. Portfolio deletion and final slab closure retain their signers. |
| 433 / INV-073 | P, G, Q | P/G are shared evidence, not separate tests for row 420 and row 433. Q adds four classic/native rail and payment-order histories: unsigned partial payments consume the same domain claims across two rails, preserve exact custody/lamports and distinguish displaced primary stock from entitlement. Q has no earned fees or active user liabilities; P separately owns earned fees. Their arbitrary composition is not inferred. |
| INV-078 recovery boundary | F | Four absent/expired-backing x absent/insufficient-insurance worlds reach exact keeper-only economic completion for five funded portfolios after an owner-signed forfeit prefix. The suffix validates signer traces, owner-local capital/payouts, stock and both retry routes. It leaves materialized portfolios and residual vault stock; it does not prove reserve payout or slab retirement for the four rows. |

Existing source-composition gates in INV-070, INV-073 and INV-071 bind terminal
stock, economic/admin phases and recovery evidence. They are drift checks, not
substitutes for executing economic witnesses or proving arbitrary reachability.
The charter explicitly separates user economic disposition from mechanical
deletion and administrative retirement. A funded keeper, usable or reconstructible
custody, authenticated time/recovery availability, and the named administrative
steps remain assumptions; no status or generic recovery claim is promoted.

## Exact Selector Index

For N/C, prepend
`inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::terminal_progress_product::native_booked_residue_cleanup::`.
Source: [native residue cleanup](cu/inv_070_native_booked_residue_cleanup.rs).

```text
N v16_program_native_booked_residue_escheats_after_fee_recredit_completion
C v16_program_classic_booked_residue_burn_control_preserves_paid_claims
```

The remaining selectors are complete. N/C/P/G/I/R/Q use `--test v16_cu`;
F uses `--test v16_program_stateful_fuzz`.

```text
P inv_073_no_permanent_user_lock::v16_program_terminal_public_reserve_disposition_preserves_value_across_orders
G inv_073_no_permanent_user_lock::v16_program_public_reserve_payments_wait_for_resolved_senior_disposition
I inv_070_zero_unattributed_terminal_residue_and_close_slab::v16_attack_permissionless_asset_insurance_authority_cannot_withhold_terminal_close
R inv_073_no_permanent_user_lock::absent_insurer_spent_retirement::missing_insurance_wallet_recredit::v16_program_recredited_insurance_reaches_terminal_exit_without_wallets_or_signatures
Q inv_073_no_permanent_user_lock::dual_quote_reserve_progress::v16_program_unsigned_dual_quote_reserves_preserve_domain_claims_and_terminal_surplus
F inv_078_permissionless_recovery_coverage::v16_program_recovery_resource_failure_lattice_preserves_public_exit
```

P/G's [INV-073 entrypoints](cu/inv_073_no_permanent_user_lock.rs) call the
[reserve helper](cu/inv_073_terminal_public_reserves.rs), re-exported by
[INV-024's earnings fixture](cu/inv_024_terminal_earnings_succession.rs).
I is in [INV-070](cu/inv_070_zero_unattributed_terminal_residue_and_close_slab.rs).
R uses [absent-reserve construction](cu/inv_073_absent_insurer_spent_retirement.rs)
and [missing-wallet completion](cu/inv_073_missing_insurance_wallet_recredit.rs).
Q is [dual-quote reserve progress](cu/inv_073_dual_quote_reserve_progress.rs);
F is [INV-078 stateful recovery](stateful/inv_078_permissionless_recovery_coverage.rs).
For reproduction, use `cargo test --locked --offline --test <harness> <selector>
-- --exact --nocapture` with a matching SBF artifact. These economic selectors
were source-reviewed, not rerun in this documentation-only audit.

## Validation

A private copy of the existing host target was used; no SBF rebuild or execution
was needed. Commands from the isolated worktree:

```bash
cp -a --reflink=auto /dev/shm/percolator-public-gap-20260916-c91e-host-target /dev/shm/astra-ultra-terminal-reserve-audit-20260917-host-target
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-terminal-reserve-audit-20260917-host-target
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_every_public_invariant_source_and_declared_test_is_mounted -- --exact --nocapture
awk -F '\t' '!/^#/ && NF != 8 { print FNR ": expected 8 fields, got " NF; bad=1 } END { exit bad }' tests/invariants/traceability_gaps.tsv
git diff --check
git diff --exit-code 205ef676dd8f534cf6a179dc1ddd0ffacb13c4a1 -- . ':!tests/invariants/*.md' ':!tests/invariants/traceability_gaps.tsv'
git diff --cached --check
git diff --cached --exit-code -- . ':!tests/invariants/*.md' ':!tests/invariants/traceability_gaps.tsv'
```

Mount census: **1 passed, 0 failed, 146 filtered; 508 source files and 1,914
available tests**. Compilation emitted existing unused-code and Solana
future-compatibility warnings. TSV shape, whitespace and documentation-only
scope checks pass. No Rust, `src`, Cargo, harness or invariant-status changes.
