# Row 417: Recovery forfeit through partial receipt completion

Base: `82a7aed7313c6705227337af5d579f8613d90a6e`, fetched `origin/main`.
Worktree: `/dev/shm/percolator-row417-partial-receipt-cleanup-20260917`.
Primary INV-067; related INV-024/063/066/068. One new public LiteSVM selector.

## Duplicate analysis

- `row417_health_20260917.md` covers fully receipted claims across two unrelated
  expiry waves; `row417_full_payment_late_cleanup_20260917.md` covers full-face
  payment and portfolio deletion before later stock cleanup. Neither includes a
  destructive Recovery forfeit in the claimant's history.
- Scope U, repeated-stock and late-expiry tests already cover deferred receipt
  creation and claimant order. Rail-liquidity and native-redemption tests cover
  custody competition and recreation. Those are not new increments here.
- INV-067's prior-claim/retained-haircut forfeit prerequisites protect older claims
  through Recovery and solvent terminal payout. INV-027's Recovery forfeit witness
  checks booked claims and principal withdrawal. Neither carries the surviving
  claim into an underfunded receipt and subsequent positive expiry top-up.

## Kept increment

Eight worlds cross two forfeit orders, two receipt orders, and expiry at slot 40
or 47. Public asset-0 trades book faces `14 * 50 = 700` and `26 * 50 = 1,300`.
One claimant subsequently opens an asset-1 position of 10 at 100. Its debtor
settles `10 * (105 - 100) = 50`, but the claimant leaves that gain unrefreshed
and explicitly forfeits it in Recovery. The older 700-atom claim survives;
the 50 atoms replenish asset-1 backing, with no corresponding new claim.

At slot 15, the first debtor's 500 and one expired source-backing atom form a
501-atom residual against the original 2,000-face denominator. Receipts pay
175/325 and retain their exact identity, face and paid counters. The Recovery
asset's 350 backing plus 50 settled loss remain Fresh until slot 40. Its expiry
raises residual to 901, so retained permissionless top-ups pay exactly 140/260.
The whole expiry plus both SPL transfers is also aborted by an invalid System
suffix: all tracked and compiled Accounts roll back, except the payer's exact
signature fee. Zero-due and terminal retries cannot restore paid or forfeited value.

Final owner payouts are `[1315, 0, 1585, 50]`; all four portfolios delete with
exact rent transfer. Vault and fixed mint supply reconcile with the single
remaining rounding atom. Receipt incarnation/position epoch/owner, full receipt
fields, denominator, source expiry and stock, and custody are independently checked.
The new suffix stops at portfolio deletion, without claiming a new slab-close witness.
System/SPL/ATA/wrapper instructions construct all accounts and economic transitions;
LiteSVM supplies programs, signer SOL, time and blockhashes. No state-byte injection.

## Exact validation

The wrapper and pinned engine were rebuilt locked/offline with default features,
platform-tools v1.52, and private copies of the c91e host/SBF caches.
SBF SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.

Final exact run on merged `main`: **3 passed, 0 failed**, 17.23 seconds. Peak measured CU:

| Selector below | Peak CU |
| --- | ---: |
| New Recovery receipt increment | 429,551 |
| Existing INV-027 Recovery control | 198,394 |
| Existing INV-067 terminal-disposition control | 161,166 |

```sh
export CARGO_TARGET_DIR=/dev/shm/row417-recovery-receipt-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/row417-recovery-receipt-sbf-target/deploy/percolator_prog.so
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_recovery_forfeit::v16_program_recovery_forfeit_preserves_older_partial_receipts_through_late_expiry \
  inv_027_protected_principal_seniority::recovery_forfeit_seniority::v16_program_recovery_forfeit_preserves_booked_claim_and_order_independent_principal \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_terminal_disposition::v16_program_receipt_terminal_suffix_partitions_rounding_burn_surplus_and_rent
```

Only these exact selectors, scoped rustfmt, whitespace and protected-path checks
are used. No broad suite or metadata census. Production, Cargo, fixtures, shared
helpers, TSV ledgers and unrelated invariant files are unchanged.
The new selector completes eight worlds, eight paying-prefix rollbacks, sixteen
positive top-ups and 32 portfolio deletions. Development corrected unsigned-close
timing, bounded clock catch-up and source-side selection. A same-asset probe
reached full payment before the required partial-receipt suffix and was not kept.

## Remaining gaps

Fixed two-asset, two-claimant, primary-SPL, zero-fee/funding history. Pending debt
is settled before receipt creation. Same-asset bankruptcy/forfeit dilution,
native or dual rails, insurance recredit, arbitrary faces/order products and
maximum shapes remain outside this increment. Row 417 remains OPEN.
