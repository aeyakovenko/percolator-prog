# Lane 20: Deferred Receipt and Fee Across Late Expiry

## Scope and isolation

- Base source: `/tmp/percolator-astra-invariant-cycle-20260915-run`, clean branch
  `codex/astra-invariant-cycle-20260915` at
  `ef1647f22b196b01d55af0ab88f89c51b0bef61a`.
- Private clone: `/tmp/percolator-lane20-resolved-receipt-expiry-20260916`.
- Local branch: `codex/lane20-resolved-receipt-expiry-20260916`.
- Engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Read `scripts/loop.md`, the invariant README, row 417's reopening and status,
  existing receipt/expiry owners, and lane reports before choosing the product.
  No withheld patch, alternate finding branch or private counterexample was used.
- No writes to either protected checkout. Fresh wrapper and authenticated matcher
  builds use private `/dev/shm` targets, without copying another lane's cache.
  Deployed artifacts and logs reside in this clone. No push is made.
- A final read-only source check found it clean at
  `6ca3802d775417c3a9e43095e176fad363ddceba`, advanced independently during this
  run. This lane remains based on the requested `ef1647f2`; that later commit
  was not imported.

## Missing product and non-overlap

The missing product is **a deferred junior receipt with an unpaid final
maintenance fee while an older receipt crosses backing expiry**. The existing
late-fee owner creates both junior receipts before expiry, leaving only the
backed claimant unreceipted. Here either the 700-face or the 1,300-face junior
stays unreceipted until after expiry. The backed claimant can therefore acquire
an exact receipt while the deferred junior still contributes a bound.

The new child uses the existing public `late_expiry::World` and late-fee
constants, rational payout formula, SPL payment checker and insurance request.
Only the child module is mounted in the existing invariant owner; the shared
fixture and root harness are unchanged.

| Earlier coverage | Difference from this increment |
| --- | --- |
| Lane 1 retained policy/grants | No grant, policy update or retained admission is added here. |
| Lane 2 overdue-source histories | Zero maintenance fees, two source expiries and generated grouping; no deferred fee-bearing junior replacement. |
| Lane 3 funding/insurance accounting | Unsettled funding and pending-loss obligations; here funding is zero and maintenance moves capital into insurance. |
| Lanes 4/5 current observations | Observation freshness, reward and maximum-shape conformance are setup controls, not this receipt product. |
| Lane 6 cumulative limits | Position/OI transition limits; no limit product is added. |
| Lanes 7/16 maximum shape | Hybrid/backlog/market occupancy progress; this is a fixed five-portfolio terminal shape. |
| Lane 8 native receipt liquidity | Both junior receipts already exist, with zero maintenance fees and a secondary liquidity shortage. This test uses one adequately funded classic SPL rail. |
| Lanes 9/18 pending debt/retirement | Mixed-role debt, shutdown and native retirement; those obligations and rails are absent here. |
| Lane 10 paid insurance succession | Insurance-only worlds, no pending portfolios or receipt replacement. The insurance holder here never changes. |
| Lanes 11/14 funded oracle containment | Role/oracle replacement and funded coholders, not deferred terminal receipts. |
| Lanes 12/15 Hybrid rewards | Reward attribution and competing recipients, with no corresponding action here. |
| Lane 13 terminal provider expiry | Earned provider fees, unavailable reserve signatures and custody repair with pending users. Here provider earnings are zero and the fee is the deferred junior's maintenance debit. |
| Lane 17 pending receipts/insurance succession | Both junior receipts are created before expiry; only the source claimant's seven-atom fee remains. Here only one junior receipt exists, two seven-atom fees remain, and there is no beneficiary handoff. |
| INV-066 late receipt materialization | Closest timing control, but fees/insurance are zero and it does not cross all six three-claimant orders with the fee collection routes. |
| INV-029 positive claim bounds | Supplies the census contract. This test consumes exact source bounds into receipts while a second claimant's fee changes insurance. |
| INV-024 terminal earnings expiry | Preserves earned provider fees after user deletion; it cannot exercise a deferred user's receipt and maintenance fee. |
| Receipt episode, fractional conversion and expiry-interleaving owners | Deferred/coowned episodes, source realization and repeated stock changes have zero maintenance fees; no duplicate conversion or arbitrary-history claim is made here. |

## Public history and oracle

The 72 histories cross two early junior identities, expiry delivery at slot 13
or 17, all six orders of the three positive claimants, and three timings for the
deferred junior's final fee: inside `CloseResolved`, explicit synchronization at
slot 12 before clock expiry, or explicit synchronization after normalization.
The backed claimant's final fee is collected by its paying close.

All market, portfolio, token and backing construction uses System/SPL/ATA and
public wrapper instructions. LiteSVM supplies deployed programs, signer funding
and Clock. There is no `set_account`, installed simulation result, program-owned
byte injection or private engine transition in the new child. Mint authority is
revoked through SPL before the measured histories. Economic operations use only
the independent payer's signature; mechanical portfolio deletion remains owner
signed. Comparison copies of Account data are never installed into LiteSVM.

Inputs and independently computed expectations:

```text
positive claim faces             [700, 1000, 1300], total 3000
winner principal after fees      1000 - 12*7 = 916 each
debtor fee debit                 3*7 = 21 each
initial residual                 500 - 21 + 1 = 480
late released backing            100 + 250 - 21 = 329
final residual                   480 + 329 = 809
final junior payments            floor(face*809/3000) = [188, 269, 350]
complete actor wallet vector     [1104, 0, 1185, 0, 1266]
insurance                        (3*12 + 2*3)*7 = 294
insurance domain budgets         [126, 168, 0, 0]
provider's never-deposited token  1
explicit rounding residue        809 - 188 - 269 - 350 = 2
fixed mint supply                3852
```

The initial older receipt is paid at residual 480. Its exact face is 700 or
1,300; the unreceipted bound is respectively 2,300 or 1,700. The new `Book`
derives every expected capital, fee slot, receipt, source-domain bound, market
bound and exact receipt total from the scheduled actions and public inputs.
It never uses an observed payout rate to compute entitlement.

Clock advancement preserves complete economic accounts. Retained top-up
requests for all three owners then advance only the market clock: an absent
receipt does not consume its bound, collect its fee or normalize backing.
Normalization releases exactly 329 atoms and preserves the slot-12 snapshot.
Explicit fee synchronization changes only capital/insurance accounting, leaving
the entire resolved payout ledger unchanged. Per-call integer fee distribution
is checked, including its 3/4 split between the two market budgets.

Each world first submits normalization, optional explicit fee collection, both
exact replacements and the older receipt's top-up in one transaction. An invalid
System suffix rejects only after all wrapper calls and three SPL transfers
succeed. Complete tracked accounts, including receipts, owner wallets, mint,
vault, fee cursors, capital, source stock and market lamports, must match their
pre-transaction images; the independent payer loses exactly one signature fee.
The same retained instructions then commit separately with the oracle checked
after each call. The largest rejected transaction is also checked against the
1,232-byte signed wire limit.

Receipt provenance, portfolio incarnation, position epoch, face, prior bound,
released face and cumulative payment are checked at every measured prefix.
Thirty-six schedules settle the backed claimant before the deferred junior,
exercising its otherwise missing intermediate receipt. Positive top-ups retain
their paid receipt until zero-due cleanup, while a close can clear its terminal
receipt during payment. Cleanup and a combined top-up/fee replay must preserve
complete accounts, with no fee after the slot-12 cap.

All 360 portfolio deletions refund exact rent to the market. The unchanged
insurance holder then receives all 294 atoms permissionlessly. Its wallet ends
at 295, the vault at the two rounding atoms, all user capital/PnL/source bounds
at zero, and the payout ledger unchanged by reserve withdrawal.

## Builds and exact commands

Commands run from the private clone. Initial log redirections used the clone
root; the completed logs are retained under `target/lane20-logs/`.

```sh
git clone --no-hardlinks --single-branch --branch codex/astra-invariant-cycle-20260915 \
  /tmp/percolator-astra-invariant-cycle-20260915-run \
  /tmp/percolator-lane20-resolved-receipt-expiry-20260916
git switch -c codex/lane20-resolved-receipt-expiry-20260916

env CARGO_TARGET_DIR=/dev/shm/lane20-20260916-sbf CARGO_BUILD_JOBS=4 \
  RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --sbf-out-dir target/deploy -- --locked
env CARGO_TARGET_DIR=/dev/shm/lane20-20260916-auth-sbf CARGO_BUILD_JOBS=4 \
  RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --manifest-path tests/fixtures/auth_matcher/Cargo.toml \
  --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked
sha256sum target/deploy/percolator_prog.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so

export CARGO_TARGET_DIR=/dev/shm/lane20-20260916-host
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/tmp/percolator-lane20-resolved-receipt-expiry-20260916/target/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_late_fee_reclassification::deferred_fee::v16_program_deferred_receipt_and_fee_preserve_late_expiry_claimant_fairness -- --exact --nocapture

cargo test --locked --offline --test v16_cu inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_late_fee_reclassification::v16_program_late_fee_reclassification_preserves_receipt_faces_and_claimant_order -- --exact --nocapture

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=2 \
  inv_066_resolved_payout_fairness_and_order_independence::v16_program_late_receipt_materialization_preserves_snapshot_entitlements \
  inv_029_positive_claim_bounds_never_understate::v16_program_positive_claim_bounds_match_public_lifecycle_census \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_earnings_expiry::v16_program_terminal_expiry_preserves_earned_fees_and_bounded_signed_disposal \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_rail_liquidity::v16_program_late_expiry_claimant_orders_share_secondary_liquidity_without_losing_receipts \
  inv_073_no_permanent_user_lock::successor_custody_retry::v16_program_paid_insurance_prefix_survives_beneficiary_succession_on_both_rails \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_public_reserves::v16_program_pending_users_cross_late_expiry_before_unsigned_reserve_payouts \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_late_fee_reclassification::v16_program_pending_receipts_preserve_late_fee_insurance_across_beneficiary_succession

cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=2 \
  inv_079_public_reachability_evidence::v16_public_trace_schema_detects_out_of_band_economic_mutation \
  inv_079_public_reachability_evidence::v16_public_trace_terminal_classifier_requires_complete_economic_evidence \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant \
  inv_079_public_reachability_evidence::v16_program_invariant_harnesses_are_test_free_roots \
  inv_079_public_reachability_evidence::v16_program_fixed_blockers_remain_progressing

rustfmt --edition 2021 --config skip_children=true --check \
  tests/invariants/cu/inv_067_receipt_late_fee_reclassification.rs \
  tests/invariants/cu/inv_067_receipt_deferred_fee.rs
git diff --check
git diff --exit-code ef1647f22b196b01d55af0ab88f89c51b0bef61a -- \
  src Cargo.toml Cargo.lock tests/fixtures tests/support tests/v16_cu.rs \
  tests/v16_program_fuzz_regressions.rs tests/invariants/open_findings.tsv \
  tests/invariants/invariant_status.tsv tests/invariants/coverage_reopenings.tsv
git diff --cached --check
git show --format= --check HEAD
```

The failed INV-024 control was also run in a separate, untouched baseline
worktree owned by this clone. It uses the same freshly built wrapper because
production is byte-identical to the base, and a copy of this lane's freshly
built matcher. From the main clone:

```sh
git worktree add --detach /tmp/percolator-lane20-resolved-receipt-expiry-20260916-baseline ef1647f22b196b01d55af0ab88f89c51b0bef61a
mkdir -p /tmp/percolator-lane20-resolved-receipt-expiry-20260916-baseline/tests/fixtures/auth_matcher/target/deploy
cp tests/fixtures/auth_matcher/target/deploy/auth_matcher.so /tmp/percolator-lane20-resolved-receipt-expiry-20260916-baseline/tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
```

From that detached baseline, using the host environment above:

```sh
env RUST_BACKTRACE=1 cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_earnings_expiry::v16_program_terminal_expiry_preserves_earned_fees_and_bounded_signed_disposal -- --exact --nocapture
```

Fresh default-feature wrapper SHA-256:
`e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.

Fresh authenticated matcher SHA-256:
`50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

## Validation results

| Check | Result |
| --- | --- |
| Fresh wrapper / matcher builds | PASS, default features, locked/offline, platform-tools v1.52; 26.51 / 10.57 seconds |
| New exact selector | PASS, 1 test / 72 worlds, 105.18 seconds; peak 734,172 CU under 900,000; largest rejected transaction 771 bytes under 1,232 |
| Original late-fee exact selector | PASS, 1 test / 24 worlds, 34.30 seconds; unchanged 700,000-CU bound, measured peak 646,849 |
| Seven adjacent exact controls | 6 PASS, 1 pre-existing failure, 137.64 seconds; INV-066, INV-029 and lane 8/10/13/17 selectors pass |
| INV-024 signed-expiry control on exact base | Same failure, 0 passed / 1 failed, 0.57 seconds |
| Eight selected INV-079 guards | PASS, 8/8, final run 3.37 seconds, including public trace/classifier, fixed-blocker progress, charter, status, reopening, audit summary and root ownership |
| Scoped rustfmt, whitespace and unchanged-file checks | PASS; production/dependencies/fixture sources/shared harnesses/status files match the base |

The unchanged INV-024 control fails at
`inv_024_terminal_reserve_destination_recovery.rs:84`: actual
`InstructionError(2, Custom(11))` (`InvalidTokenAccount`) versus expected
`InstructionError(2, Custom(8))` (`Unauthorized`). Canonical destination
validation rejects its wrong-recipient request before the later authority
check. The exact selector reproduces on the untouched `ef1647f2` worktree, and
the same baseline failure is documented in the Lane 13 report. That selector
and its production path are unchanged. No passing full-suite claim is made.

Control peaks: INV-066 144,260 CU; Lane 8 classic/native-split/native-grouped
276,528 / 276,925 / 279,925 CU; Lane 10 98,558 CU; Lane 13 428,497 CU;
Lane 17 763,149 CU. These are measured costs, not exact-cost assertions.

Development corrected two test expectations, without changing production:

1. The initial reused 700,000-CU guard failed at 734,172 CU after a history
   completed its economic assertions. Two receipt replacements cost more than
   the original one-replacement fee case. The new test has its own 900,000-CU
   bound; all older bounds remain unchanged. The rollback reaches the intended
   invalid System suffix, and every required public continuation succeeds.
2. A positive `ClaimResolvedPayoutTopup` retains its paid receipt until a zero-due
   retry, even with zero unreceipted bound. The oracle initially expected eager
   cleanup like `CloseResolved`. The final oracle checks the two public routes
   separately and requires the later cleanup to complete without another payout.

The retained logs are `lane20-sbf.log`, `lane20-auth-sbf.log`,
`lane20-baseline.log`, `lane20-new.log` (initial CU expectation),
`lane20-new-receipt-expectation.log`, `lane20-new-final.log`,
`lane20-controls.log`, `lane20-base-inv024.log`, `lane20-inv079.log` and
`lane20-inv079-final.log`, all under this clone's `target/lane20-logs/`.

**No new public-route LoF, persistent DoS or required-progress CU bug was found.**
No production fix or implementation red/green claim is made.

## Disposition and limits

This is bounded public-route conformance, not rediscovery or generic closure of
the withheld finding. **Row 417 remains OPEN with evidence `missing`; INV-067
remains `REFUTED_CURRENT` and INV-029 remains `OPEN_EVIDENCE`.** No machine row,
production source, manifest, lock, fixture or shared harness is changed.

The matrix has five portfolios, two assets, one classic SPL quote rail, a fixed
seven-atom maintenance rate, one late source expiry, fixed honest AuthMark inputs,
no provider earnings and no funding. Normalization precedes all three final
payouts. Arbitrary histories, fee exhaustion/debt/credits, live source conversion,
repeated expiries, Recovery, ADL, native/secondary liquidity, reserve succession,
maximum shapes and complete slab retirement remain outside this increment.
No unfiltered suite or Kani campaign is run.

Changed files: the new invariant child, its three-line parent registration,
the invariant README note and this report.
