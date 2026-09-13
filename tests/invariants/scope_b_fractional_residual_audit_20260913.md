# Scope B: Fractional Residual Resolution

## Provenance And Scope

Base: `codex/astra-open-holdout-ledger-20260912` at
`4f65d830e0cb9054b1a620c62110eabcb87ef710`.
Worktree: `/tmp/percolator-pr135-scope-b-fractional-residual-20260913`.
Branch: `codex/pr135-scope-b-fractional-residual-20260913`.

Only supplied-base tests, the local public wrapper, and the pinned engine source
were inspected. User-supplied PR #435 metadata was treated solely as a row-419
family/shape cue. No PR branch, diff, tests, or GitHub content was fetched.
Neither the protected `/home/anatoly/percolator-prog` checkout nor production
code was edited. Row 419 is a coverage label, not a reproduction specification.

The retained [CU probe](cu/inv_039_fractional_residual_resolution.rs) is mounted
through the existing INV-039 owner. README and classification tables are unchanged.

## Overlap Audit

| Existing owner | Boundary already covered; distinction here |
| --- | --- |
| `inv_039_pending_loss_insured_resolution` | Two bankrupt domains, exact input insurance/B categories and terminal owner payouts. It deliberately chooses exact credit ratios and excludes fractional conversion. This probe has unequal fractional weights in one common cohort and independent booking, account, source-conversion and receipt remainders. |
| `inv_038_rounding_and_ratio_conservation` | Social-loss quotient/remainder identities and aggregate versus one-atom booking/settlement through live obligation cleanup. Its schedule oracle starts from observed close/index deltas; it does not carry this multi-owner numerator ledger through resolved payouts. |
| `inv_067_receipt_fractional_source` | Fractional source conversion, expiry and later receipt payouts with independent floors. No pending B residual/cohort attribution precedes those receipts in that owner. |
| `inv_038_mixed_residue` | Expired/Fresh source allocation and receipt cash residue. It does not independently derive a nonzero close residual and fractional B allocation from opening quantities and debtor principal. |
| `inv_039_pending_loss_resolved_histories` | All cohort close orders and a generated owner-debt oracle, but solvent integral positions without bankruptcy remainders. Another fixed two-domain permutation would be redundant. |
| `inv_039_pending_loss_owner_partition` | One owner split across portfolios sharing an ATA, with empty close ledgers. The new probe keeps owners distinct and derives a nonzero close partition plus multiple rounding stages. |

## Public Histories

Two holder/debtor pairs open the same asset. Public authenticated marks move
200,000 price units over five slots. A matched reduction exhausts the first
debtor's principal and creates residual `R`; both holders retain zero-basis,
nonzero-weight obligations. The solvent peer exits either by matched reduction
or by public shutdown/Recovery forfeit after holder accrual. Those routes have
different source/common-pool allocations despite the same price debts.

The finite product has 64 histories: two unequal fractional weight pairs, two
positive residuals for each pair, mirrored sides, the two peer-exit routes,
residual booking before/after `ResolveMarket`, and both claimant orders. The
weight/residual inputs are `[450003,600004]` with `[7,11]`, and
`[300003,700007]` with `[10,13]`. All require nonzero market booking remainder,
nonzero carry for both holders, and positive whole-atom loss for both holders.

Keeper settlement interleaves creditors, the bankrupt debtor, the solvent debtor
and an unrelated principal owner. The bankrupt portfolio is deleted before the
cohort's remaining obligations and payouts finish. Public `CloseResolved`
continues the remaining accounts, and fully paid receipts are retried with
`ClaimResolvedPayoutTopup`. Economic state is built only through System, SPL,
ATA and wrapper instructions; Clock advancement is a harness input. No engine
or oracle account bytes are injected.

## Independent Ledger

For position scale `Q`, social-loss denominator `D`, movement `m`, and weights
`q[i]`, the book computes from inputs:

```text
gain[i] = floor(q[i] * m / Q)
debt[i] = ceil(q[i] * m / Q)
R = debt[0] - bankrupt_deposit
B = floor(R * D / sum(q)); booking_rem = R * D % sum(q)
loss[i] = floor(q[i] * B / D); carry[i] = q[i] * B % D
```

The close ledger independently requires gross loss `R`, then either remaining
residual `R` or B booking `R`. Support, insurance, direct explicit assignment,
drift and junior-face-burn categories stay zero. Side-local explicit loss and
dust are **a partition within booked B**, not an additional close-ledger debit.
At every checked prefix, `R*D` equals booking remainder plus detached dust and
explicit-loss numerators plus each still-live or already-debited allocation.
Original weight, exact B snapshot/carry, stored/pending counts and zero OI are
checked separately. Deleting the debtor cannot delete this accounting history.

The second ledger models each source realization from remaining input-derived
claim face `F` and backing `A`: `rate=min(C,floor(A*C/F))`, then
`converted[i]=floor(face[i]*rate/C)`. It never uses production credit-rate or
capital deltas as expected conversion amounts. A disappeared source attribution
identifies the executed work; the predicted conversion is checked against the
source's exact remaining claim/backing stock and the owner's eventual receipt.

After source conversion, receipt face is `gain[i]-loss[i]-converted[i]`.
The common pool is zero for matched peer exit and the peer's input debt for
Recovery forfeit. Each junior payout is independently floored against this pool
and the sum of receipt faces. Every holder's exact payout is principal plus its
predicted conversion and junior allocation. The bankrupt debtor receives zero;
the solvent debtor receives its deposit minus its own price debt; the unrelated
owner receives its original principal.

At nonterminal prefixes, capital, signed PnL, unpaid face and prior SPL payout
must match that owner's attributed budget. Terminal receipt dilution is charged
only the independently calculated ratio/floor difference. An additional atom of
underpayment cannot pass by moving it to another owner or merely conserving supply.
Receipt face/prior bound and the common pool's rate numerator/denominator are
also checked exactly.

Raw owner payout vectors and terminal custody agree across side and close orders
**within each exit route**. Across the two routes, comparison adds back only the
independently predicted receipt deduction. The raw route difference is not hidden:
source-rounding backing and common-pool payout residue are checked separately,
and their sum must equal actual remaining SPL/booked custody. The terminal cash
stocks are zero, one or two atoms, according to the input/route ledger. Source
backing residue remains reserved in its source ledger; it is not mislabeled as
owner capital, insurance, or freely distributable common-pool cash.

All portfolios become terminal and are deleted. Capital, positive PnL, insurance
and materialized count reach zero. Complete stock/reservation censuses and fixed
mint supply are checked throughout; sibling asset states and foreign Accounts
are preserved. This probe stops before source expiry or slab/surplus extraction.

## Evidence And Limits

This adds sampled INV-037/038/039/066/076 composition: nonzero residual through
resolution, fractional booking/account carry, source/receipt floors, debt-owner
deletion, and exact owner payouts. INV-076 evidence is residual durability and
finalization, not adverse drift or new atomic rollback coverage. Explicit claim
calls are paid-receipt retries, not a new positive-value top-up rail comparison.

Creditors and debtors are interleaved as distinct portfolios/owners. A single
portfolio simultaneously carrying creditor and debtor roles across domains is
not covered. Neither is arbitrary history, ADL, insurance consumption, fees,
funding, changing source backing after receipt retirement, or a general F proof.
No claim is made about reproducing or closing PR #435 or any holdout.

INV-039 remains `REFUTED_CURRENT`; INV-037/038/066/076 remain `OPEN_EVIDENCE`.
Row 419 remains open/missing. No classification promotion is warranted.

## Validation

SBF was built once at the start from this worktree using the prior private PR135
cache. Subsequent changes were test-only and reused that matching artifact; no
broad rebuild or suite run was performed. SHA-256:
`8a010d8283186721866a95a71d252a3265015fbf563d6ce566cd44f263659349`.
Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

Development checks exposed two oracle assumptions, not established attribution
failures: matched-only exits produced no receipts, and the Recovery route cannot
treat nominal face as fully payable after source and receipt floors. A diagnostic
rerun confirmed the rate below one; the ledger was extended with input-derived
source and receipt arithmetic, not an observed fixed adjustment. A further driver
check was corrected because a fully source-realized owner may exit before the
other source conversion. The 64-history selector then passed in 38.51s, with 32
receipt retries and peak observed setup/continuation of 315,103 CU.

Final retained test plus four nearest controls: **5/5 passed in 76.03s**.
The retained test reports 64 worlds, 16 input-predicted receipt-floor worlds,
32 receipt retries and peak 315,103 CU. Charter/index and authoritative-status
guards: **2/2 passed**. Formatting and working-tree whitespace checks pass.
Four exploratory single-selector runs failed on the oracle/driver assertions
described above, including the diagnostic repeat. No failing probe is retained.
Existing unused-support and Solana future-compatibility warnings remain.

Exact commands from the isolated worktree:

```sh
export CARGO_TARGET_DIR=/dev/shm/pr135-scope-b-catchup-20260913-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export TMPDIR=/dev/shm CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
sha256sum "$PERCOLATOR_FUZZ_SBF"
# Used for the exploratory revisions and the first passing 64-history run.
cargo test --locked --offline --test v16_cu inv_039_pending_loss_obligation_durability::fractional_residual_resolution::v16_program_fractional_cohort_residual_preserves_owner_floors_through_resolution_orders -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_039_pending_loss_obligation_durability::fractional_residual_resolution::v16_program_fractional_cohort_residual_preserves_owner_floors_through_resolution_orders \
  inv_038_rounding_and_ratio_conservation::v16_program_social_loss_aggregate_and_chunked_routes_converge_exactly \
  inv_039_pending_loss_obligation_durability::close_reopen::two_domain_resolution::insured_resolution::v16_program_insured_pending_domains_preserve_exact_debt_through_resolution_orders \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_fractional_source::v16_program_fractional_source_conversion_preserves_receipts_through_same_bucket_expiry \
  inv_039_pending_loss_obligation_durability::resolved_histories::v16_program_pending_resolved_history_generator_preserves_owner_debt
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --quiet --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
```
