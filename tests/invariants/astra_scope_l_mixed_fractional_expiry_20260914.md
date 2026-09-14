# Astra Scope L: Mixed Obligations And Fractional Source Expiry

## Provenance

Base: `origin/codex/astra-open-holdout-ledger-20260912` at
`d01245dd993bfe60c2448360733c3bb63c41aa19`.
Private shared-object clone: `/tmp/percolator-astra-scope-l-20260914`.
Branch: `codex/astra-scope-l-pending-resolution-20260914`.
Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
The two protected checkouts were not edited. All changes, host output and logs
are in this private clone. Production, dependencies and machine statuses are unchanged.

## Owner Comparison

The new selector is mounted below INV-039's existing `mixed_role_resolution`.
It reuses Scope J's public setup and debt book; the book now derives peer receipt
face through both fixed-point floors and accounts for an idle owner's signed
capital-to-provider contribution. J's original exactly representable inputs
retain the same expected values and selector.

| Current owner | Existing boundary and new composition |
| --- | --- |
| Scope J, INV-039 mixed roles | Same-portfolio creditor/debtor obligations, exact source conversion, user deletion. L adds fractional peer-source conversion and two expiry deadlines through slab retirement. |
| Scope D, INV-039 reserve roles | Two distinct claimant portfolios, insurance spend, claimant/provider/insurance overlap and recredit. L has no insurance contributor and its idle provider has neither trading role. That overlap cell is not repeated. |
| INV-039 fractional residuals, Scope B | Unequal fractional cohort weights, B remainders and distinct portfolios. L retains integral quantities/B and adds same-portfolio debt with a fractional source rate. |
| INV-024 | Its owner history and reserve families distinguish attribution from aggregate custody. L retains J's owner book through source residue and provider expiry, without another authority-succession family. |
| INV-037/076 | The close partition remains independently checked; support face is not a second value payment. L adds terminal composition and rollback, not new drift or ADL coverage. |
| INV-041/048 | Paired schedules compare observed owner vectors; the inherited full leg/OI/weight/pending census runs after checked continuations. This does not exhaust orderings or maximum shapes. |
| INV-066/067 | Fractional receipt/source-expiry owners lack this pending mixed-role origin. L checks a fully paid 180,001-face receipt and retries before later backing release; partial top-ups remain outside the increment. |
| INV-073 | Public economic payouts precede separately signed portfolio deletion and admin slab cleanup. L adds the mixed fractional origin, not a new missing-signer or custody-repair product. |
| INV-081/086 | The existing global stock/reservation checks and complete Account rollback are reused. L does not extend the general action-word runner, shrinker or full reference-model transition relation. |

Reviewed `scripts/loop.md`, `INVARIANTS.md`, the README owner sections, reopening
ledger, current owner modules and Scopes J/D before choosing this bounded product.
No finding-specific external branch or reproduction was used.

## Input Book And Histories

Original deposits are `[400000, 180000, 300000, 250000, 777]`, with fixed SPL supply
1,130,777. The original creditor gain is 200,000 and bankrupt close residual is
20,000. Portfolio 0 retains its creditor obligation in A while carrying debtor
exposure in B. Scope J's public matched reduction and authenticated marks construct
both roles. System/SPL/ATA/wrapper instructions create all economic state; program
loading, signer SOL and Clock advancement are harness inputs, with no installed or
mutated economic account images.

The product crosses two `(debt, provider principal)` inputs, both signs, both A/B
asset placements, two paired live/resolved-booking and continuation/deletion
schedules, and exact/one-slot-late handling of both expiry deadlines: 32 histories.
The first debtor continuation is rolled back and retried before the remaining
order is driven; booking timing and close order are paired, not independent axes.

For debt D and fixed-point scale C, the input book computes:

```text
S = min(D, 180000) = 180000        support value
F = S * 200000 / 180000 = 200000  retired source face
H = F - S = 20000                face discount, not another support payment
R = 200000 - 180000 = 20000       separate pending residual
peer_backing = D - S
peer_rate = floor(peer_backing * C / D)
peer_conversion = floor(D * peer_rate / C)
peer_receipt = D - peer_conversion = 180001
```

The new ideal peer rates are 1/6 and 3/13. Both quantize below the exact rate;
the converted whole atoms are 35,999 and 53,999. The additional receipt atom is
fully paid, while one counterparty backing atom remains reserved in its source.
The original owner-local invariant remains principal plus accrued PnL, minus its
own B and K charges and face discount, equal to capital, signed PnL, unpaid receipt
face and prior SPL payout. A conserved one-atom reassignment fails the same book.

| D | Provider principal | Exact five-owner payouts | Final burn |
| --- | --- | --- | --- |
| 216,000 | 101 | `[344000, 0, 516000, 250000, 676]` | 20,101 |
| 234,000 | 307 | `[326000, 0, 534000, 250000, 470]` | 20,307 |

The idle fifth owner accepts the initially empty asset-zero provider authority,
then atomically withdraws and funds its bucket from existing capital. The authority
transfer frames every portfolio; the funding leaves SPL supply and custody
unchanged while reducing only that owner's user entitlement. It is not an
insurance beneficiary and has no creditor or debtor leg.

The complete close ledger, owner/domain attribution, retained loss weight,
source face/backing, side B, OI, stored/pending counts, capital/PnL, stock and
reservation census are checked through resolution and all five deletions.
The peer receipt face and paid amount are exactly 180,001; its paid retry changes
no tracked Account. Each deletion sends its exact rent to the market.

At slot 25/26, the idle provider bucket expires. It cannot release the peer's
one source-reserved atom or pay any user again. The peer backing was crystallized
at slot 15 with the setup's 1,000-slot freshness horizon, so its independently
expected expiry is slot 1,015. That exact deadline is checked before handling it
at slot 1,015/1,016. After this separate expiry, bounded slab cleanup burns exactly
H plus expired provider principal, closes the vault, and leaves a rent-exact slab
tombstone. Admin receives only released rent, zero quote. Final observed owner
vectors match the input book and each other across the paired schedules and sides.

Five rejected transactions per world cover completed debt continuation, portfolio
deletion, first expiry, source expiry and final burn/vault-close prefixes. Each
ends with an unsigned deletion and requires the expected suffix error plus the
correct successful-prefix count. Every fixture and compiled transaction Account
is restored, including absence, metadata and lamports; only the exact signature
fee is charged to the separate payer. Unchanged prefixes then retry successfully.

## Limits And Classification

Rows **419 and 435 remain OPEN**. This is finite INV-039/024 coverage with bounded
INV-037/041/048/066/067/073/076/081 composition assertions. It does not close the
generic generator/oracle or claim a new INV-086 equivalence result.

Remaining dimensions include fractional position/B-cohort allocation, arbitrary
support-face rounding, ADL, adverse close drift, fees/funding, partially funded
terminal receipts and positive top-ups, insurance spend/recredit, overlapping
reserve roles, alternate K/B economic schedules, repeated histories, CPI/batch
routes, alternate quote rails and maximum shapes. Both receipt owners are fully
paid before either expiry; this is not a same-portfolio recredit proof.

Development corrected a bucket-field API assumption and the missing initial
provider-authority assignment. An initial attempt to finish slab cleanup at
slot 25 hit the still-fresh peer-source prerequisite. Extending authenticated
time to the independently specified 1,015-slot deadline completes the public
path. No public-instruction conformance mismatch was established, and no
production correction or production red/green claim is made.

## Artifact And Validation

Reused fixed default-feature SBF:
`/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so`.
SHA-256: `79b298706ca7ffab09a017e5f37c4c0fc43d4e6292c092d126f4fbe8dff1b673`.
`git diff --no-index` reports no production-source difference against the artifact's
build checkout; Cargo.toml/Cargo.lock hashes match. No SBF build or matcher is needed.
Host dependencies were copied to this clone's private executable 6-GiB tmpfs at
`target/host`; shared host output was not modified.

| Required check | Result |
| --- | --- |
| New exact selector | PASS, 1/1 in 21.60 seconds: 32 histories, 160 complete rollback checks, 16 waiting rollbacks, 32 paid-receipt retries, 160 deletions and 96 terminal calls |
| Scope J exact-rate mixed-role control | PASS: 32 histories, 24 waiting rollbacks, 32 receipt retries; peak 205,951 CU |
| Scope B fractional-residual control | PASS: 64 histories, 16 predicted receipt-floor cases, 32 receipt retries; peak 315,102 CU |
| Scope D insured reserve-role control | PASS: 24 histories, 168 rollback checks, 120 user payouts/deletions; peak 472,694 CU |
| Pending backing-expiry control | PASS: 16 histories, 32 rollback checks, 80 payouts/deletions; peak 310,678 CU |
| Four controls together | PASS, 4/4 in 85.44 seconds; no inherited control failures |
| Four `v16_program_fuzz_regressions` metadata gates | PASS, 4/4 in 0.01 seconds |
| `cargo fmt --all -- --check` | PASS |
| Working-tree, staged and committed whitespace checks | PASS |

New-selector peak measured CU is **193,974**, below the 600,000 transaction
ceiling. This maximum includes the new transaction helper, inherited debt-book
continuations and provider authority/funding calls. It is not an exhaustive
measurement of every setup instruction. The first passing matrix measured
190,140 CU; fresh random fixture keys can change address-derivation costs.
The final new selector was rerun after the rent and observed-wallet comparison
checks were added. No unfiltered suite was run.

The metadata target reports 346 existing unused-support warnings; Cargo also
reports the existing Solana client future-compatibility warning. Logs are retained
under the private `target/host/scope-l-{new,controls,metadata}.log` paths.

Exact environment and commands from the private clone:

```sh
export CARGO_TARGET_DIR=/tmp/percolator-astra-scope-l-20260914/target/host
export TMPDIR=/tmp/percolator-astra-scope-l-20260914/target/host
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so

sha256sum "$PERCOLATOR_FUZZ_SBF"
git diff --no-index /run/percolator-pr135-scope-w-20260913/src src
sha256sum Cargo.toml Cargo.lock /run/percolator-pr135-scope-w-20260913/Cargo.toml /run/percolator-pr135-scope-w-20260913/Cargo.lock
cargo test --locked --offline --test v16_cu inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::v16_program_mixed_role_fractional_source_preserves_attribution_through_expiry_and_retirement -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::v16_program_mixed_creditor_debtor_roles_preserve_owner_entitlement_through_resolution \
  inv_039_pending_loss_obligation_durability::fractional_residual_resolution::v16_program_fractional_cohort_residual_preserves_owner_floors_through_resolution_orders \
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
