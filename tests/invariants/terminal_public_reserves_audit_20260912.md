# Public terminal reserve disposition, 2026-09-12

Base: `af97beeac08872b28103feb20b83cf615557f00a` from
`origin/codex/astra-open-holdout-ledger-20260912`. Branch:
`codex/astra-terminal-public-disposition-20260912`. Worktree:
`/home/anatoly/percolator-prog-astra-terminal-public-disposition`.
No GitHub PR/issue branches or diffs were consulted. The original checkout's files
were left untouched. The clean detached `-base` worktree was used only to rebuild
the supplied base for the final red/green comparison.

## Change and Scope

The three public reserve withdrawal handlers required the reserve holder's
signature even after full resolved wind-down. The wrapper now permits an unsigned
payment to the recorded backing provider or terminal insurance beneficiary in
Resolved mode. Such a payment requires a beneficiary-owned, initialized SPL
destination with no delegate or close authority. Signed live management, canonical
vault and mint validation, role and epoch binding, principal expiry, ledger binding,
stock limits, and the existing `materialized_portfolio_count == 0 && c_tot == 0`
resolved gate remain in force. There is no new instruction or engine change.

The INV-073-owned selectors call a narrow child of the existing earned-fee fixture
owner, avoiding another trading setup. A fixture option exposes the state before
resolution for the consent and seniority control. All economic state is created
with real System, SPL, ATA and wrapper instructions; test controls are initial
signer SOL, Clock, program installation and blockhash renewal. Account copies are
read-only assertion images, never injected into LiteSVM.

## Distinct Coverage

The new family crosses all six principal/earnings/insurance payout orders with two
terminal principal histories: full withdrawal while fresh, or expiry of the unpaid
tail after a 101-atom public payment. Both histories first pay partial earned fees
(17 atoms) and insurance (7 atoms), then reverse the payout order for remaining
claims. The independent trading inputs yield owner entitlements 56,627/1,995,000,
provider principal 100,000, earned fees 875, and insurance 31.

Provider, insurance operator, insurance beneficiary, market authority and keeper
are distinct. The first three signing keys are dropped before measured payments.
All 66 reserve payments have exactly one transaction signature, the keeper's.
The 138 exact rejections cover successful payment/lazy-ledger prefixes followed by
unsigned mechanical close (36), publicly constructed delegated or externally
closable destinations (72), and premature slab closure while reserve claims remain
(30). Every rejection compares all compiled and tracked Accounts, including
presence, metadata and lamports, allowing only the actual transaction fee.

After each payment, the oracle reconciles the input-derived claim balances,
provider earnings ledger, source principal/reservations and consumed-backing
history, insurance totals, full SPL account images, unchanged authority profile,
fixed supply and owner payouts. Six expiry histories explicitly reclassify and
burn 99,899 unpaid principal atoms each. All twelve histories finish slab closure
with exact rent refund and tombstone rent; ledger rent stays in the ledger.
Expiry normalization and mechanical slab closure are separately admin-signed.

The additional control tries all three routes unsigned while Live, after resolution
with senior capital outstanding, and after exact keeper-only user payouts with
empty portfolios still materialized. Live attempts require consent; both resolved
stages reject at the existing full-wind-down gate with exact rollback. The control
returns both users' full entitlements in at most sixteen public close attempts.

Closest prior coverage was absent-provider expiry, absent-insurer exhaustion and
recredit, signed destination repair, signed earned-fee expiry, and provider-role
roundtrip. Those did not pay surviving reserve claims without reserve signatures.
This patch adds no native-insurance, pending-loss or provider-roundtrip family.
Existing signer-denial suffixes in four neighboring tests now use a different
role's destination; their completed prefixes, signed payouts and exact retries
remain checked. The reserve account-role matrix uses unsigned resolved success
controls and retains live signer-downgrade rejections.

## Validation

Both SBF artifacts were built locally with locked/offline dependencies, default
features and platform-tools v1.52. Engine pin:
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

- Base SBF SHA-256: `d5f2d3d2c35842aab0979ab24fed415fe36b2b93ed6cc80998fee839ae76343f`.
- Fixed SBF SHA-256: `c8b584ed01570396e1031a1d4566e2ebc3b48781056f1297e7bde40905f4694d`.
- The final unchanged disposition oracle fails on the base at the first unsigned
  earnings payment: `InstructionError(2, Custom(6))`, before the expected invalid
  close suffix at index 3. It passes on the fixed wrapper with all twelve worlds;
  observed peak transaction cost is 261,807 CU. The adjacent destination-repair
  rollback probes exercise deeper multi-instruction prefixes and peak at 414,635
  CU under their 1,200,000-CU transaction ceiling.
- All eleven exact CU selectors below pass. The charter/index selector
  `inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete`
  in `v16_program_fuzz_regressions` also passes. `cargo check --locked --offline --lib`,
  `cargo fmt --all -- --check` and `git diff --check` pass. Existing unused-support
  warnings and the Solana client future-compatibility warning remain.

Exact `v16_cu` selectors (no unfiltered suite):

```text
inv_073_no_permanent_user_lock::v16_program_terminal_public_reserve_disposition_preserves_value_across_orders
inv_073_no_permanent_user_lock::v16_program_public_reserve_payments_wait_for_resolved_senior_disposition
inv_017_signer_writable_role_and_account_alias_safety::v16_program_reserve_custody_account_pairs_and_required_privileges_are_exhaustive
inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_reserve_destination_recovery::v16_program_terminal_reserve_destination_repair_preserves_beneficiaries_and_value
inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_earnings_expiry::v16_program_terminal_expiry_preserves_earned_fees_and_bounded_signed_disposal
inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_earnings_roundtrip::v16_program_terminal_provider_roundtrip_preserves_intervening_fee_payouts
inv_024_attributed_quote_value_conservation::terminal_earnings_succession::v16_program_terminal_earned_fee_succession_preserves_paid_prefix_and_insurance
inv_067_terminal_payout_completeness_and_exact_once_settlement::provider_insurance_retries::v16_program_absent_provider_preserves_user_order_and_operator_free_insurance_exit
inv_067_terminal_payout_completeness_and_exact_once_settlement::provider_insurance_retries::v16_program_terminal_provider_and_insurance_retries_preserve_separate_entitlements
inv_064_insurance_withdrawal_policy_equivalence::v16_program_live_and_resolved_insurance_withdrawals_share_one_finite_budget
inv_077_bounded_work_and_maximum_shape_compute::native_insurance_exit::v16_program_native_insurance_partial_redemption_reaches_bounded_terminal_exit
```

Commands use `CARGO_TARGET_DIR=/dev/shm/astra-terminal-public-disposition-target`,
`PERCOLATOR_FUZZ_SBF=$CARGO_TARGET_DIR/deploy/percolator_prog.so`, `TMPDIR=/dev/shm`,
four build jobs, incremental compilation off, and dev/test debug info off. Invoke
`cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1`
with the selectors above. For the base comparison only, change the SBF path to
`$CARGO_TARGET_DIR/base-deploy/percolator_prog.so`.

## Files Changed

```text
src/v16_program.rs
tests/invariants/README.md
tests/invariants/coverage_reopenings.tsv
tests/invariants/cu/inv_017_signer_writable_role_and_account_alias_safety.rs
tests/invariants/cu/inv_024_attributed_quote_value_conservation.rs
tests/invariants/cu/inv_024_terminal_earnings_expiry.rs
tests/invariants/cu/inv_024_terminal_earnings_roundtrip.rs
tests/invariants/cu/inv_024_terminal_earnings_succession.rs
tests/invariants/cu/inv_024_terminal_reserve_destination_recovery.rs
tests/invariants/cu/inv_067_terminal_provider_insurance_retries.rs
tests/invariants/cu/inv_073_no_permanent_user_lock.rs
tests/invariants/cu/inv_073_terminal_public_reserves.rs
tests/invariants/terminal_public_reserves_audit_20260912.md
```

## Remaining Obligations

Rows **420, 421 and 433 remain OPEN**. This is a finite SPL/asset-0 family with
matched integral PnL and a specific earned-fee history, not generic reachability
over all terminal stock, quote, asset, receipt, pending-loss and recredit states.
The existing dependency on owner-signed deletion of economically empty portfolios
is explicitly preserved and tested. Fully absent market authorities, mechanical
permissionless slab retirement and absent-beneficiary native redemption are not
proved. Neither historical holdout coverage nor whole-invariant status is promoted.
