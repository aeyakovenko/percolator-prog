# Scope P: Fractional Cohort Debt Through Funded Recreation

## Provenance

Base: `origin/codex/astra-open-holdout-ledger-20260912` at
`e527392517542b10a64d0ef303f346b0898509ae`.
Fresh isolated worktree: `/tmp/percolator-pr135-scope-p-20260914`.
Local branch: `codex/pr135-scope-p-invariant-20260914`.
Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
All edits and host build output are in this worktree. Protected checkouts were
not edited. Production, dependencies and machine invariant statuses are unchanged.

Read the Scope D pending-reserve and Scope L mixed-fractional audits first, then
the INV-039 charter, current owner modules, README and reopening ledger. Only
current local source and its pinned dependency informed the accounting model.
There is one new selector and no exploratory or duplicate selector retained.

## Coverage Boundary

The [new product](cu/inv_039_fractional_cohort_recreation.rs) is mounted under
`inv_039_pending_loss_obligation_durability::fractional_residual_resolution::cohort_recreation`.
It adds Scope L's explicit remaining dimension of fractional cohort allocation
with close/reopen ordering. System/SPL/ATA/wrapper instructions construct every
economic Account. Only program loading, signer SOL and Clock advancement are
harness inputs; no economic Account image is installed or mutated.

| Current owner | Distinct addition |
| --- | --- |
| Scope B `fractional_residual_resolution` | Reuses its unequal-weight fixture and debt/source arithmetic. Adds live B settlement around same-address debtor recreation and separately funded fresh capital before resolution. The original 64-world matrix is a control. |
| INV-039 `close_reopen` | Its single integral holder has no fractional allocation or carry. Here two unequal fractional holders retain separate charges and carry across recreation. |
| INV-039 `cohort_reduction` | Its equal-weight integral holders transfer and recreate a paid claimant. Here the bankrupt debtor is recreated while both original claimant legs remain, one optionally B-settled. |
| Scope J `mixed_role_resolution` | Simultaneous creditor/debtor legs remain its ownership cell. This product separates an old debtor incarnation from fresh principal at the same address. |
| Scope L `mixed_role_fractional_retirement` | L has integral positions/B with fractional source conversion and expiry. This product uses fractional position/B allocation and funded recreation; it does not repeat expiry or retirement. |
| Scope D `reserve_role_recredit` and `backing_expiry` | Remain controls. No insurance beneficiary/provider overlap, reserve funding or expiry is added. |

INV-039 owns obligation persistence; INV-024 owns the exact owner/class vector.
The inherited close partition, source face/backing, pending/stored counts, OI,
loss-weight, stock/reservation census and receipt checks provide bounded
INV-037/038/041/048/066/067/073/076/081 composition evidence. No new general
INV-086 transition-equivalence claim is made.

## Input Book And Histories

The two weight/residual inputs are `([450003, 600004], 7)` and
`([300003, 700007], 13)`. Sixteen histories cross these inputs, both position
signs, either first-settled holder, and recreation before/after its live B step.
Terminal claimant priority follows that selected holder; it is not an independent
permutation axis. Both pairs are flattened through public matched reductions.

For weights q, position scale P, social-loss denominator N and residual R:

```text
gain[i] = floor(q[i] * 200000 / P)
debt[i] = ceil(q[i] * 200000 / P)
B = floor(R * N / sum(q))
booking_remainder = R * N mod sum(q)
loss[i] = floor(q[i] * B / N)
carry[i] = q[i] * B mod N
```

The first debtor deposits `debt[0] - R`; the remaining initial capitals are
200000, 300000, 250000 and 777. Matched settlement leaves R unpaid and both
creditors with zero basis but their original nonzero loss weights. Booking R
must preserve both creditor Accounts. One live holder continuation pays its
whole-atom B debit but retains its fractional `b_rem` and leg; the other holder
remains B-stale. Resolution later detaches both carries into the market's exact
dust/explicit-loss partition. The sum of whole debits, outstanding numerator
obligations, retained carries, detached carry and booking remainder equals R*N.

After booking, a signed transaction withdraws the bystander's 777 capital atoms,
deletes the settled debtor, replenishes its exact rent with System transfer,
initializes the same address with the next portfolio ID, transfers 113 SPL atoms
from the bystander and deposits them as the new incarnation's capital. The
donor retains 664 atoms. Expected transfer and capital amounts come from these
inputs, never observed payout or stock deltas. The old debtor's ledger is empty
in the new incarnation, but the old cohort B and source ledgers remain exact.

The shared book adds only this input-owned capital transfer to its prior owner
entitlements; the original selector initializes that transfer to zero. The new
incarnation has zero PnL, no source claim and no receipt. Its 113 senior capital
atoms pay out in Resolved before either old claimant's resolved payout, while
the old debt still remains attributed to those claimants.

| Weights | R | Holder B debits | Exact five-owner payouts | Vault residue |
| --- | --- | --- | --- | --- |
| 450003, 600004 | 7 | 2, 3 | `[289998, 113, 419997, 129999, 664]` | 0 |
| 300003, 700007 | 13 | 3, 9 | `[259997, 113, 439992, 109998, 664]` | 1 |

The remaining zero/one atom follows price floors minus uncharged fractional B;
the source/reservation book also checks it. Both observed owner payout vectors
and vault residue agree across paired schedules and signs. All five portfolios
reach terminal payout and deletion within six bounded passes, with zero capital,
positive PnL, OI, retained weight and materialized portfolios. Each of the 80
final deletions returns exact rent to the market; recreation independently checks
the old rent return, new rent and debtor SOL debit.

After recreation, an input-derived five-owner vector of capital, PnL and paid
SPL is compared with actual Accounts. Two observation-only controls preserve
aggregate quote: one moves a capital atom between owners, the other reclassifies
one fresh-capital atom as PnL. Both fail the same comparison used on real data.
Every checked prefix also verifies raw SPL program ownership, token owner/mint,
the fixed mint supply and the inherited stock/reservation census.

## Atomicity And Limits

Three rejected transactions per history restore every fixture and compiled
transaction Account, including absence, data, metadata and lamports. Only the
exact signature fee is charged to the separate payer. Successful wrapper-prefix
counts, the exact suffix error/index and an actual successful SPL invocation
are asserted; wire transactions fit the 1232-byte packet bound.

1. The bystander SPL withdrawal precedes rejection of unbooked debtor deletion.
2. The entire funded recreation prefix precedes an unsigned deletion rejection.
3. The fresh incarnation's resolved SPL payout precedes that unsigned rejection.

The latter two unchanged prefixes retry with fresh blockhashes and pay exactly
the input-derived amounts. The original unbooked route is rebuilt after booking,
because booking changes the debtor's authenticated close state.

Rows **419 and 435 remain OPEN**. This is adjacent 435-style obligation/identity
attribution coverage, not a same-portfolio mixed creditor/debtor proof. The fresh
incarnation receives principal but opens no new position. The product is finite,
one traded asset, classic SPL, zero fees/funding/insurance, fully funded matched
source settlement. ADL, partial receipts/top-ups, insurance spend/recredit,
new exposure in the recreated portfolio, repeated recreation, adverse close
drift, other quote rails and maximum shapes remain untested. The terminal goal
is exact user payout/deletion; the one source-rounding atom is not driven through
expiry or slab retirement.

Development corrected one fixture expectation: the first live holder crank
settles B while retaining its leg and carry, rather than immediately detaching
it into market dust. The final book checks both stages. No implementation
conformance mismatch was established and no production correction is made.

## Artifact And Validation

Fixed default-feature SBF:
`/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so`.
SHA-256: `79b298706ca7ffab09a017e5f37c4c0fc43d4e6292c092d126f4fbe8dff1b673`.
Production source has no diff against its build checkout; Cargo.toml and
Cargo.lock hashes match. No SBF build or matcher is needed. Host dependencies
were copied into this worktree's private 6-GiB executable tmpfs at `target/host`;
shared host output was not modified. All Cargo invocations are locked/offline.

| Check | Result |
| --- | --- |
| New exact selector, initial passing run | PASS, 16 histories, 48 complete SPL-prefix rollbacks, 80 terminal deletions in 10.72 seconds; peak 556092 CU |
| Final new selector plus seven exact controls | PASS, 8/8 in 122.20 seconds; final new-selector peak 547092 CU |
| Original fractional residual | PASS, 64 worlds, 16 predicted receipt-floor worlds, 32 retries; peak 315102 CU |
| Integral debtor recreation | PASS, 2 worlds; peak composed route 384169 CU |
| Equal-weight cohort reduction/recreation | PASS, 2 worlds, 6 rollbacks, 10 payouts; peak 540781 CU |
| Scope J mixed roles | PASS, 32 worlds, 24 waits, 32 retries; peak 206217 CU |
| Scope L fractional retirement | PASS, 32 worlds, 160 rollbacks, 32 receipt retries; peak 190140 CU |
| Scope D reserve recredit | PASS, 24 worlds, 168 rollbacks, 120 user payouts/deletions; peak 480601 CU |
| Pending backing expiry | PASS, 16 worlds, 32 rollbacks, 80 payouts/deletions; peak 321267 CU |
| Four metadata gates | PASS, 4/4 in 0.01 seconds |
| Format, working/staged diff and committed show checks | PASS |

The final run includes the exact signature-count and per-deletion rent checks.
All new composed transactions are bounded by 600000 CU and 1232 wire bytes.
CU includes the new transaction helper and inherited fixture/continuation peak;
it is not an exhaustive measurement of every initialization instruction. Random
fixture keys can vary address-derivation cost. No unfiltered suite was run.
Metadata compilation emitted 346 existing unused-support warnings and Cargo
reported the existing Solana client future-compatibility warning.

Logs: `target/host/scope-p-new.log`, `target/host/scope-p-controls.log` and
`target/host/scope-p-metadata.log`, all in the isolated worktree.

Exact environment and commands from that worktree:

```sh
export CARGO_TARGET_DIR=/tmp/percolator-pr135-scope-p-20260914/target/host
export TMPDIR=/tmp/percolator-pr135-scope-p-20260914/target/host
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so

sha256sum "$PERCOLATOR_FUZZ_SBF"
git diff --no-index /run/percolator-pr135-scope-w-20260913/src src
sha256sum Cargo.toml Cargo.lock /run/percolator-pr135-scope-w-20260913/Cargo.toml /run/percolator-pr135-scope-w-20260913/Cargo.lock
cargo test --locked --offline --test v16_cu inv_039_pending_loss_obligation_durability::fractional_residual_resolution::cohort_recreation::v16_program_fractional_cohort_debt_survives_funded_debtor_recreation_and_resolution -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_039_pending_loss_obligation_durability::fractional_residual_resolution::cohort_recreation::v16_program_fractional_cohort_debt_survives_funded_debtor_recreation_and_resolution \
  inv_039_pending_loss_obligation_durability::fractional_residual_resolution::v16_program_fractional_cohort_residual_preserves_owner_floors_through_resolution_orders \
  inv_039_pending_loss_obligation_durability::close_reopen::v16_program_pending_loss_survives_debtor_recreation_and_bystander_payout \
  inv_039_pending_loss_obligation_durability::cohort_reduction::v16_program_reduced_cohort_keeps_exact_loss_after_paid_holder_transfer_and_recreation \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::v16_program_mixed_creditor_debtor_roles_preserve_owner_entitlement_through_resolution \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::v16_program_mixed_role_fractional_source_preserves_attribution_through_expiry_and_retirement \
  inv_039_pending_loss_obligation_durability::close_reopen::two_domain_resolution::insured_resolution::reserve_role_recredit::v16_program_pending_claimant_reserve_roles_preserve_attribution_through_close_and_recredit \
  inv_039_pending_loss_obligation_durability::backing_expiry::v16_program_pending_losses_survive_late_backing_expiry_and_claimant_close_order
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --check --oneline HEAD
```
