# Pending terminal maintenance coverage, 2026-09-12

Base: `798c30d815892ac5283a62add9bfcc7f15b3a435`,
`origin/codex/astra-open-holdout-ledger-20260912`. A branch-specific `ls-remote`
confirmed that the tracking ref was current, so no fetch was needed. Worktree:
`/tmp/percolator-terminal-receipt-coverage-20260912`; branch:
`codex/astra-terminal-receipt-coverage-20260912`. The original checkout's files,
including its existing conflict, were not edited. No open PR diffs, tests or
implementations were read. The requested PR numbers were only coverage labels.
No push was performed.

## Distinct relation

The new selector is
`inv_039_pending_loss_obligation_durability::terminal_fees::v16_program_pending_cohort_terminal_fees_stop_at_resolution_and_reach_insurance_exit`,
mounted from [the new CU module](cu/inv_039_pending_loss_terminal_fees.rs).

The base's terminal receipt, liveness and reserve audits were read first, then
`rg` surveyed the pending-loss, receipt, custody, quote and cursor modules.
The closest existing evidence differs as follows:

| Existing coverage | Difference from this increment |
| --- | --- |
| `resolved_histories` and `pending_destination_recovery` | Solvent pending cohorts, terminal close and destination repair explicitly exclude nonzero maintenance/funding. |
| `terminal_earnings_succession`, `terminal_insurance_lifecycle`, `terminal_reserve_destination_recovery` | Fee or reserve ownership, repair and payout, without retained zero-basis pending cohorts carrying unsettled debtor fees. |
| `maintenance_terminal_seniority` | Maintenance across resolution on flat owners, without original loss weights, unsettled opposing debt or holder detachment. |
| Receipt/late-expiry and terminal cursor siblings | Cover separate receipt and scan relations, not maintenance settlement on pending cohorts. |

Eight independent public worlds cross mirrored position sides, both debtor
orders and settlement at the exit-window boundary or 30 slots later. The shared
public `AttributionWorld::new_with_params` initializes five owners with deposits
200,000/180,000/300,000/250,000/777, two traded cohorts on assets 1 and 2, and a
7-atom maintenance rate. Integral quantities 3 and 2 and price moves 7 and 13,999
define original debts of **21 and 27,998** without engine arithmetic helpers.
Funding and trade fees are zero.

Public mark accrual, holder refresh, asset shutdown and holder forfeit leave two
zero-basis, nonzero-loss-weight obligations. At resolution slot 23, holders have
paid maintenance through slot 20; both untouched debtors and the idle bystander
retain fee cursor zero. No debit is inferred merely from a market crank.

Keeper-only holder detachment charges the remaining three slots and retains each
original claim without creating a payable receipt while opposing debt is
unbooked. A transaction then settles the first debtor, charges its 161-atom fee
and transfers its net SPL payout before the other cohort's waiting holder returns
`EngineNonProgress`. The exact instruction index, wrapper success count and SPL
success log establish that the prefix executed. All compiled/tracked Accounts,
including fee cursors, budgets, debt weights and custody, roll back; only the
separate keeper's actual signature fee is charged.

The same debtor instruction subsequently succeeds. Settlement of the other
debtor happens eleven slots later, followed by holder and bystander payouts.
Each successful prefix checks owner-local capital, PnL, receipt remainder, SPL
payout, fee cursor, OI, loss weights and pending/stored counts. Fees stop at slot
23 regardless of landing time. Final owner SPL amounts are exactly:

| Owner | SPL payout |
| --- | ---: |
| Holder 0 | 199,860 |
| Debtor 1 | 179,818 |
| Holder 2 | 327,837 |
| Debtor 3 | 221,841 |
| Bystander 4 | 616 |

Each owner pays **161** maintenance atoms. Total fees are **805**, split exactly
**400/405** between the base asset's two insurance domains by the actual charge
partitions; other domains receive zero. All five completed payout retries at
slot 100 or 130 reject without another fee, payout or state change.

Owner-signed portfolio deletion returns its exact rent to the slab. After every
user is disposed, the signed insurance beneficiary withdraws all 805 fee atoms.
Bounded `CloseSlab` continuation closes the empty SPL vault and leaves only the
market tombstone rent. Exact slab/vault refunds go to the market authority; user
tokens, beneficiary tokens and fixed mint supply are preserved. All 930,777
initial atoms are independently reconciled to final recipients.

All account creation and economic transitions use real wrapper, System, SPL and
ATA instructions. LiteSVM supplies installed programs, initial signer SOL,
Clock and fresh blockhashes. No economic or account-shape bytes are injected,
restored or edited. Read-only Account snapshots serve solely as assertions.
Measured transactions verify signatures, packet size, a 400,000-CU ceiling and
complete error frames including payer fees. Production, support and Cargo files,
invariant verdicts and holdout ledgers are unchanged.

## Remaining gaps

All five requested labels remain OPEN; no historical vulnerable pin was tested.

| Label | Limit after this increment |
| --- | --- |
| 417 | No new retained positive-receipt/late-backing-expiry relation. Existing receipt controls remain separate. |
| 418 | No native wSOL, native insurance retirement or Token-2022 evidence. The new fixture uses ordinary SPL accounts only. |
| 419 | Adds nonzero maintenance, deferred debtor fees, resolved detachment, exact rollback and complete terminal disposition. Nonzero funding/trade fees, clipped fees, insolvency/bankruptcy, ADL, fractional positions and arbitrary histories remain outside it. |
| 424 | No new obligation or environmental reclassification behind a persisted scan prefix; final empty-slab continuation is not general cursor invalidation evidence. |
| 433 | Economic user settlement is keeper-only; portfolio deletion and insurance extraction use their required owners/beneficiary. Lost reserve keys and completion without reserve signatures remain unproved. |

## Validation

The default-feature wrapper rebuilt from this worktree with locked/offline
platform-tools v1.52, using a private copy of the existing terminal-reserve build
cache. Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
Wrapper SHA-256:
`5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.

The new selector passes **1/1**: eight worlds, 48 exact rollbacks, 40 exact owner
payouts, eight insurance extractions and eight completed slab closes. Observed
peak in the final focused run: **181,462 CU**, below the 400,000-CU ceiling.
The new selector plus nine adjacent controls pass **10/10**. The invariant
charter/index passes **1/1**; `cargo fmt --all -- --check` and `git diff --check`
pass. Existing unused-support warnings and the Solana future-compatibility
warning remain.

Development corrected the fixture's initial live-fee cursor assumptions: a
nonflat account's fee sync waits for its settled market state, and a flat
observation crank need not charge its own fee. The final oracle explicitly
expects holder cursors at 20 and untouched debtor/bystander cursors at zero.
No production failure was found or suppressed.

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-terminal-receipt-coverage-20260912-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export TMPDIR=/dev/shm CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_039_pending_loss_obligation_durability::terminal_fees::v16_program_pending_cohort_terminal_fees_stop_at_resolution_and_reach_insurance_exit \
  inv_039_pending_loss_obligation_durability::resolved_histories::v16_program_settled_pending_cohorts_reach_exact_terminal_slab_close \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_reserve_destination_recovery::v16_program_terminal_reserve_destination_repair_preserves_signer_gates_and_value \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::v16_program_terminal_earned_fee_succession_preserves_paid_prefix_and_insurance \
  inv_024_attributed_quote_value_conservation::terminal_insurance_lifecycle::v16_program_terminal_insurance_lifecycle_preserves_fee_and_paid_prefix_attribution \
  inv_027_protected_principal_seniority::maintenance_terminal_seniority::v16_program_maintenance_terminal_orders_pay_senior_principal_before_protocol_extraction \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::late_expiry::v16_program_retained_claim_identity_survives_late_expiry_recipient_rotation_and_atomic_retry \
  inv_071_crank_progress::terminal_cursor_time::v16_program_persisted_scan_reclassifies_time_without_skipping_live_siblings \
  inv_082_state_indexed_liveness_theorem::terminal_destination_recovery::receipt_destination_recovery::v16_program_recreated_destinations_preserve_paid_receipts_across_expiry_without_owners \
  inv_082_state_indexed_liveness_theorem::terminal_destination_recovery::shared_destination_recovery::v16_program_shared_destination_recovery_preserves_principal_and_single_rent_charge
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
```
