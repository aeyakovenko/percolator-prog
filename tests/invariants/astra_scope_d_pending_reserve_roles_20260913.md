# Astra Scope D: Pending Claimant Reserve Roles

## Provenance

Base: `origin/codex/astra-open-holdout-ledger-20260912` at
`d346cc90cc4a473b9b55664dc15ab8af7661ceef`, fetched before checkout.
Private shared-object clone: `/tmp/percolator-astra-scope-d-20260913`.
Branch: `codex/astra-scope-d-obligation-attribution-20260913`.
Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
The protected source checkouts were only read. All edits and host compilation
occurred in this private clone. Production, shared fixture APIs, engine pin and
machine invariant statuses are unchanged.

## New Coverage

The [new selector](cu/inv_039_pending_reserve_role_recredit.rs) belongs to INV-039
and reuses its insured two-domain public construction and independent debt book.
System, SPL, ATA and wrapper instructions construct all economic state. Only
program loading, signer SOL funding and the Clock are harness inputs. There is
no installation or mutation of program-owned economic account images.

| Existing coverage | Distinct addition here |
| --- | --- |
| Scope J `mixed_role_resolution` | J combines creditor and debtor legs in one portfolio, excludes insurance/provider contributions, and stops before expiry and recredit. This probe combines claimant and reserve roles through that terminal suffix. |
| `insured_resolution` | Adds a real provider bucket and claimant reserve holders, followed by expiry, recredit, separate reserve payouts and slab retirement. |
| `backing_expiry` | Adds insurance consumption and recredit to an insolvent pending cohort, with reserve beneficiaries sharing claimant wallets. |
| INV-073 absent-insurer recredit | Adds two explicitly pending cohorts, retained owner debt, shared economic roles and paired close schedules. Absence and wallet repair are not repeated. |
| INV-024 generated reserve entitlement | Starts with pending bankruptcy and spends insurance before the reserve phase. Role handoff generation and fee succession are not repeated. |

The 24 worlds cross two backing amounts, both position signs, three assignments
of provider/beneficiary roles to claimants, and two paired settlement/deletion/
expiry schedules. The roles are `(claimant 0, claimant 0)`, `(claimant 2,
claimant 0)` and `(claimant 0, claimant 2)`. All reserve destinations are existing
claimant ATAs. The insurance role transfer is signed by its funded incumbent
and incoming holder. The donor signs the backing donation to the provider.

The original gains are 200,000 and 280,000 atoms. Matched reductions consume
180,000 and 250,000 debtor principal and leave pending close residuals of 20,000
and 30,000, with zero-basis creditor legs retaining their original loss weight.
Insurance contributions of 7,500 and 6,000 are made after these residuals exist.
Booking spends those amounts exactly once and books B losses of 12,500 and
24,000. Reserve authority changes and funding do not settle either portfolio.

The retained debt book checks the entire close ledger and input-derived owner
entitlement after every prescribed resolved continuation. It also checks the
complete OI, weight, pending/stored count, capital/PnL and fixed-supply census.
The first released cohort cannot forgive the other cohort's unbooked debt.
User payouts are exactly `[387500, 0, 556000, 0, 6501]` in both schedules.

Backing is placed on asset 1's claimant side, opposite its debt-funded source,
and remains unused during user settlement. Its inputs are 338 or 7,531 atoms.
After all five portfolios are deleted, the provider receives 31 principal atoms.
The remaining 307 or 7,500 principal atoms expire at slot 25 or 26. Claim-free
residue can then restore the selected insurance domain's historical spend:

```text
recredit = min(backing - 31, 7500 historical spend, 180000 paired receivable)
         = 307 or 7500
```

The beneficiary receives this amount in two unsigned withdrawals, 37 atoms then
the remainder. Expired provider principal is never paid as provider principal;
the restored insurance belongs to the selected beneficiary even when that
beneficiary also owns a user claim or the provider role. The other asset's
insurance budget/spend remains unchanged through the checked reserve phase.

The reserve book is input-derived: expected user payouts, provider payment,
insurance recredit and insurance payment never come from observed token deltas.
Every checked prefix validates each raw SPL wallet and its owner/mint, exact
remaining principal/insurance classes, source receivable, stock/reservation
censuses and constant mint supply. A one-atom wrong-owner observation preserves
aggregate quote but fails the wallet oracle. A wrong-class observation preserves
reserve total but fails the class oracle, including coheld ATA cases.

The paired schedules must produce identical five-owner wallet vectors. Fixed
mint supply is `950001 + backing`; all of it reaches these owner wallets. The
vault reaches zero, all five portfolios are deleted, and bounded slab cleanup
reaches a tombstone without changing the final owner vectors or paying admin
quote. Portfolio deletion separately checks exact rent return to the market.

## Atomicity And Limits

Successful debtor booking and user payouts precede a rejected unsigned deletion
suffix. The final portfolio deletion and actual provider SPL payout precede an
insurance withdrawal that is still blocked by unexpired backing. Later expiry,
implicit recredit and a 37-atom insurance SPL payout precede another rejected
suffix. Every rejection restores complete fixture and compiled transaction
Accounts, including absence, data and lamports; only the exact signature fee
is charged to the independent payer. Successful wrapper-prefix counts are
asserted and the same instruction prefixes retry with fresh blockhashes.

INV-039 owns obligation persistence; INV-024 owns exact owner/class attribution.
Related INV-037/041/048/066/067/073/076/081 assertions are bounded composition
evidence. This is not a new INV-086 reference-model equivalence result.
Rows **419 and 435 remain OPEN**. No generic invariant or family is closed.

This is a finite two-domain, classic-SPL, integral, zero-fee/funding fixture.
The smaller backing case underfunds terminal insurance restoration. The chosen
source rates remain the existing exactly representable insured inputs; arbitrary
fractional conversion, ADL, adverse close drift, provider earnings, more sources,
role returns, alternate quote rails and maximum shapes are not generated.
The original creditor/debtor parties remain distinct portfolios: Scope J's
same-portfolio cell is a control, not duplicated evidence. Combining that cell
with these reserve roles and arbitrary recredit histories remains open.

Development corrected Rust ownership errors, an attempted reserve payout before
the final materialized portfolio was deleted, duplicate admin ATA creation, and
an assumption that an expiry-slot slab call would only scan an empty prefix.
The final probe scans that prefix before expiry and asserts expiry separately.
No implementation conformance mismatch was established and no production fix
or red/green production claim is made.

## Artifact And Commands

The existing corrected Scope W default-feature artifact is reused:
`/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so`.
SHA-256: `79b298706ca7ffab09a017e5f37c4c0fc43d4e6292c092d126f4fbe8dff1b673`.
`git diff --no-index` found no production-source differences against its build
checkout, and Cargo.toml/Cargo.lock hashes match. There is no SBF rebuild.
The host cache was copied into a private executable 6-GiB tmpfs mounted at this
clone's `target/host`; no shared host output was modified. No matcher is needed.

The new selector passes: 24 worlds, 168 exact rollback checks, 120 user
entitlements/deletions and 24 terminal slab closes. Highest measured CU is
484,686, including checked multi-instruction transactions, within the 600,000
assertion bound. Setup, inherited debt steps and the new transaction helper
contribute to this maximum; it is not an exhaustive measurement of every fixture
instruction. The first passing matrix took 17.66 seconds after host compilation;
the final formatted selector took 17.36 seconds (first/final peaks 474,186 and
484,686). Fresh random fixture keys can change derivation cost.

| Required check | Result |
| --- | --- |
| New exact selector | PASS, 1/1, 24 worlds, 168 exact rollbacks |
| Adjacent selectors | PASS, 3/3 together in 51.77 seconds |
| Insured pending-domain control | 32 worlds, 624 rollback checks, peak 326,782 CU |
| Backing-expiry control | 16 worlds, 32 rollback checks, peak 318,267 CU |
| Scope J control | 32 worlds, 24 waiting rejections, 32 receipt retries, peak 205,951 CU |
| Four metadata gates | PASS, 4/4 in 0.02 seconds |
| `cargo fmt --all -- --check` | PASS |
| `git diff --check` | PASS |

The metadata target emits existing unused-support warnings; Cargo reports the
existing Solana client future-compatibility warning. No unfiltered test suite
was run. Staged and committed whitespace commands are included below and are
run after staging/committing this bounded coverage increment.

Exact validation environment and commands, from the private clone:

```sh
export CARGO_TARGET_DIR=/tmp/percolator-astra-scope-d-20260913/target/host
export TMPDIR=/tmp/percolator-astra-scope-d-20260913/target/host
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so

sha256sum /run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so
git diff --no-index /run/percolator-pr135-scope-w-20260913/src src
sha256sum Cargo.toml Cargo.lock /run/percolator-pr135-scope-w-20260913/Cargo.toml /run/percolator-pr135-scope-w-20260913/Cargo.lock
cargo test --locked --offline --test v16_cu inv_039_pending_loss_obligation_durability::close_reopen::two_domain_resolution::insured_resolution::reserve_role_recredit::v16_program_pending_claimant_reserve_roles_preserve_attribution_through_close_and_recredit -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_039_pending_loss_obligation_durability::close_reopen::two_domain_resolution::insured_resolution::v16_program_insured_pending_domains_preserve_exact_debt_through_resolution_orders \
  inv_039_pending_loss_obligation_durability::backing_expiry::v16_program_pending_losses_survive_late_backing_expiry_and_claimant_close_order \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::v16_program_mixed_creditor_debtor_roles_preserve_owner_entitlement_through_resolution
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
