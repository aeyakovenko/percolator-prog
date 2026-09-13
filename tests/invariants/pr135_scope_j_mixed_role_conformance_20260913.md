# PR135 Scope J: Mixed Creditor/Debtor Attribution

## Provenance

Base: `origin/codex/astra-open-holdout-ledger-20260912` at
`f0c2bb729ed1476eca21dd81b53a7a7245bd2fcd`.
Branch: `codex/pr135-scope-j-mixed-role-conformance-20260913`.
Worktree: `/tmp/percolator-pr135-scope-j-20260913`.
Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

Only invariant documentation/status ledgers, supplied-base code and tests, and
the pinned engine implementation informed this work. No open PR branch, diff,
or test was inspected or copied. Rows 419/435 are conformance scope labels.
The original checkout was not edited. Production and shared fixtures are unchanged.

The new [public LiteSVM probe](cu/inv_039_mixed_role_resolution.rs) is mounted
through the existing INV-039 CU module. The distinct-portfolio fractional
residual probe remains a control: it explicitly excludes simultaneous creditor
and debtor roles on one portfolio. The shared-holder probe has two creditor
roles; the two-bankrupt-domain probe assigns creditor/debtor roles to different
portfolios. Neither supplies this owner-local mixed-role composition.

## Public History And Oracle

Deposits are `[400000, 180000, 300000, 250000, 777]`. Portfolio 0 holds one lot
as creditor in domain A and two lots as debtor in domain B. Its A counterparty
is portfolio 1; its B counterparty is portfolio 2. Authenticated mark movement
creates 200,000 gain in A. A matched reduction consumes portfolio 1's 180,000
principal and leaves a 20,000 close residual and portfolio 0's zero-basis,
nonzero-weight creditor obligation. Subsequent B marks create debt D on that
same portfolio, with the opposite gain already accrued to portfolio 2.

The 32 histories cross D in `[36000, 240000]`, mirrored sides, asset placement
`[1,2]`/`[2,1]`, residual booking before/after resolution, and two continuation
orders. At resolution portfolio 0 still carries both its pending creditor leg
and its unsettled debtor leg. Construction uses System/SPL/ATA and wrapper
instructions; only clock advancement is a harness input. Economic account bytes
are not injected. Setup verifies mark/index origins and the complete initial
role state; every checked resolved continuation, rejection, retry and deletion
then reconciles the book.

The ledger distinguishes quote support from retired claim face. The source has
180,000 backing for 200,000 face before mixed-role settlement. Current local
settlement processes K before B. From inputs alone:

```text
S = min(D, 180000)             source value used for the B-domain debt
F = S * 200000 / 180000        source face retired for that support
H = F - S                     face discount, not another quote payment
R = 200000 - 180000 = 20000    separate A-domain pending residual
```

All divisions are exact for these inputs; B booking and account remainders are
zero. The peer's source conversion is zero or the exact 1/4 rate, leaving receipt
face S. Source support, its face retirement, and B loss are separate checks. The
oracle never treats observed capital, PnL, receipt face or payout as an expected
economic amount. Observed snapshots/leg presence identify completed work;
disappearance requires the corresponding input-derived debit. At each prefix,
portfolio 0 must satisfy:

```text
capital + signed PnL + unpaid receipt face + prior SPL payout
  = 400000 + 200000 - (B charged ? R : 0) - (K settled ? D + H : 0)
```

Portfolio 1 retains exactly `-R` before its booking and zero afterward.
Portfolio 2 owns `300000 + D`; unrelated owners retain their deposits. The
oracle also checks exact source face/backing, owner/domain membership, original
loss weight, side-local B, OI, stored/pending counts, close-ledger categories,
stock/reservation censuses and fixed mint supply. A conserved one-atom owner
reassignment fails the same entitlement predicate. Foreign accounts are framed
on each close and deletion; waiting rejections restore the entire economic
frame. The separate transaction fee payer is excluded from that frame.

The input-derived terminal payouts are:

| D | Owner payout vector | Remaining vault |
| --- | --- | --- |
| 36,000 | `[540000, 0, 336000, 250000, 777]` | 4,000 source-reserved atoms |
| 240,000 | `[320000, 0, 540000, 250000, 777]` | 20,000 unreserved atoms |

Each owner receives exactly its vector component across both sides, asset
placements and continuation orders. No remainder is reassigned to another
owner or counted as senior capital. All five portfolios are deleted; capital,
positive PnL, insurance, OI, retained weights and portfolio counts reach zero.
The untouched third asset remains byte-equivalent. Paid receipts are retried
without any tracked economic change. The probe stops before source expiry or
terminal surplus extraction.

## Classification And Limits

This adds sampled INV-024/037/039/066 composition evidence for a single portfolio
holding both roles, including a nonzero pending close residual and exact owner
payouts. It does not close rows 419/435 or promote any invariant. INV-024/039
remain `REFUTED_CURRENT`; INV-037/066 remain `OPEN_EVIDENCE`. Both finding rows
remain `missing`, and row 419's coverage reopening remains `OPEN`. README and
machine classification tables are unchanged.

The finite histories use integral quantities, zero fees/funding, one creditor
per source, no ADL, and no insurance/provider contribution. They do not establish
alternate K/B economic schedules, arbitrary histories, fractional cohort
allocation, partial positive-value receipt top-ups, source expiry, or slab close.
The source-face accounting is the current deterministic support rule, not a
claim that nominal face is fully payable or that these are zero-residue exits.

One development compile caught a private sibling helper reference; assertions
were kept local. The first executable draft treated nominal face as support
value and failed its own entitlement equation. The retained oracle separately
derives S/F/H and uses exactly divisible inputs. No production failure was
established by that discarded assumption, and no production change was made.
An added endpoint census exposed a 1/6 peer-source conversion on a trial net-debtor
input of 216,000. The retained 240,000 input instead has an exact 1/4 conversion,
preserving the intended exclusion of fractional source carry. The peer's exact
receipt face S and total remaining source backing are independently asserted.

## Validation

Default-feature SBF was freshly built from this worktree, using an existing
build cache and a private output directory. SHA-256:
`71833878f8d52373e501e70ed21d88495051695f2533b4c55362ae90e737aa9d`.

The final exact selector passes **32 histories**, **24 exact waiting rejections**
and **32 paid receipt retries** in **20.87 seconds**, with peak resolved
continuation **206,217 CU**. The three nearest controls pass: two-domain
bankruptcy, shared-holder obligations and distinct-portfolio fractional residuals.
They ran in the 63.82-second four-selector check that also exposed the trial
1/6 endpoint assumption; the corrected new selector was then rerun separately.
No broad suite run was performed. Charter/index and authoritative-status guards
pass **2/2**. `cargo fmt --all -- --check` and working-tree/staged whitespace
checks pass. Existing unused-support and Solana future-compatibility warnings
remain. The committed change is also checked with `git show --format= --check HEAD`.

Exact commands, run from the isolated worktree (the exports reproduce the
environment supplied to each command):

```sh
export CARGO_TARGET_DIR=/dev/shm/pr135-scope-b-catchup-20260913-target
export PERCOLATOR_FUZZ_SBF=/tmp/percolator-pr135-scope-j-20260913/target/deploy/percolator_prog.so
export TMPDIR=/tmp CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /tmp/percolator-pr135-scope-j-20260913/target/deploy -- --locked
sha256sum target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu inv_039_pending_loss_obligation_durability::mixed_role_resolution::v16_program_mixed_creditor_debtor_roles_preserve_owner_entitlement_through_resolution -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::v16_program_mixed_creditor_debtor_roles_preserve_owner_entitlement_through_resolution \
  inv_039_pending_loss_obligation_durability::fractional_residual_resolution::v16_program_fractional_cohort_residual_preserves_owner_floors_through_resolution_orders \
  inv_039_pending_loss_obligation_durability::shared_holder::v16_program_shared_holder_pending_domains_survive_partial_detach_and_debtor_close_orders \
  inv_039_pending_loss_obligation_durability::close_reopen::two_domain_resolution::v16_program_two_bankrupt_domains_preserve_pending_debt_across_resolution_order
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --quiet --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
```
