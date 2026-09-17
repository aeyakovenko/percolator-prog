# Source-accounting conformance evidence audit, 2026-09-17

Base: `origin/main` at `cdbe46fa9f33f2845c61f2727059162b8708c4d0`.
Worktree: `/dev/shm/percolator-astra-source-accounting-20260917`.
Branch: `astra-ultra/source-accounting-conformance-20260917`.

## Decision and scope

All thirteen requested benchmark rows already have mounted behavioral witnesses
for their named families. No distinct missing public-route assertion was established
in this audit, so this change adds an evidence map rather than another test or
source roster. This is a source review and mount check, not a new behavioral run,
historical red/green demonstration, production finding, or invariant-status promotion.

The audit used current `INVARIANTS.md`, `open_findings.tsv`,
`independent_discoveries.tsv`, the invariant README/traceability notes, executable
test bodies, their discovery helpers, and harness mounts. Benchmark identifiers
and titles were used only to identify families. No withheld branch, PR patch, or
external reproduction was used.

The initial INV-028/031/032/033/063/064 review found existing consumed-backing,
shared-lien, mixed-reserve, expiry/refill, and insurance-withdrawal evidence. The
requested narrowing then prioritized INV-028/031/063. INV-032's lifecycle
composition and INV-033's engine-only insurance-reservation boundary are already
documented; the latter is not a reachable insurance-lien fixture to manufacture.
The zero-spend insurance schedule and liquidation-spend tests have different
boundaries, but this audit does not claim their arbitrary composition is covered.

## Benchmark-to-witness map

The selector IDs below resolve to full harness selectors in the next section.
These are existing witnesses, not newly executed economic evidence.

| Row(s) | Selector ID | Public setup and assertion that prevents vacuity | Limit |
| --- | --- | --- | --- |
| 213 | S1 | Public winning mark and risk increase must create a nonzero source lien before reversal. Six independent route worlds, with generated admitted increase sizes, require bounded progress, exact failed-call rollback, and conserved custody. | The reversal witness requires a reducing continuation; it is not a full payout theorem for arbitrary histories. |
| 214 | C1 | A 5,000-atom claim and nonzero counterparty lien precede exact/late expiry. Public normalization and reduction preserve senior withdrawal; the retained claim subsequently reaches exact terminal payout and both portfolios are deleted. | Two expiry/hint cases on a fixed asset shape. |
| 270, 379 | C2; C3 supplements compute/exit | Opposite profitable episodes fill all 28 source slots in forward/reverse asset order. An already-reserved asset still opens/closes; an unrelated asset rejects before admission with `Custom(9)` and exact tracked rollback, excluding CU exhaustion. C3 separately reduces every retained leg at the public 14-leg/28-source shape. | C2 is the current discovery-ledger owner for both rows. C3 ends flat with retained claims, not full claim payout. Neither substitutes for latent-generation admission. |
| 300 | C4 | Four publicly funded portfolios include a live local lien and an unsettled adverse delta. A different, lien-free winner expires the shared bucket. Both direct resolved close and crank must progress; every portfolio reaches terminal disposition and impaired aggregates reach zero. | Fixed shared-domain, four-portfolio world; not arbitrary scheduler fairness. |
| 306 | S2 | Public losses generate two genuinely fractional source rates before reversal. Both asset orders probe public continuations, require mutating progress, frame rejected calls, and reconcile custody. A required-route mask prevents an incomplete blocked-route search from claiming a persistent lock. | This proves a bounded continuation in the sampled rounding setup, not every eventual payout. |
| 364 | S3 | Provider withdrawal, a real risk lien, and public flattening leave positive PnL with a retained lien. Each of four transport worlds requires bounded mutating lien release, conversion, capital withdrawal, and portfolio close with no remaining funded value/lien. | Fixed two-asset/source shape and provider withdrawal; maximum-lien work is separately owned by INV-077. |
| 423 | C5 | Twenty-four histories include twelve reused-generation worlds. Historical claims plus admitted live legs' future domains exactly fill the 28-domain budget. Exact rejection/prefix rollback and retry precede full materialization, input-derived attribution, ranked terminal payout, and deletion. | The full exit-resource obligation remains OPEN. A source-slot witness does not prove every future backing/lien/insurance resource across arbitrary histories. |
| 267 | S4; R1 fixed control | Two equal claims live in different domains; only the higher domain has backing. Conversion must consume that domain's claim and backing together. A second conversion cannot use its residual backing for the unfunded claim; exact rollback and a 1,100-atom owner payout are checked. | Fixed economic amounts and domain pair; generated seeds do not constitute generated arbitrary domain/amount coverage. |
| 291 | S5; S6 complementary | S5 starts with real fresh backing and a positive claim, then resolves at expiry-1/expiry/expiry+1. Both claimant orders and both payout-route preferences complete, reconcile custody, and produce identical exact/late outcomes. Fresh close consumes backing nontrivially; expired close cannot. | `independent_discoveries.tsv` currently associates row 291 with S6, the retained-top-up boundary. S5 is the more direct already-funded/lapsed settlement witness; S6 also checks terminal progress after admission/rejection. |
| 361 | S6 | Identical retained top-up intent is exercised before, at, and after signed maturity. Before-expiry funding succeeds; expired intent rejects without principal movement and terminal users still settle. | One retained maturity family at bounded offsets. |
| 363 | S7 | Nonzero released claim and fresh backing make the expiry-1 conversion succeed. Exact/late expiry rejects with rollback and preserves senior exit, so a blanket rejection cannot pass. | Live conversion boundary; terminal normalization is S5. |
| 367 | S8 | All four trade transports cross expiry-1/expiry/expiry+1. Fresh control must use backing; expired risk increase must reject without fees or new liens, while risk reduction remains available. | Finite transport/boundary matrix, not arbitrary oracle/funding history. |

## Exact existing selectors

For each entry below, the invocation is
`cargo test --locked --offline --test <harness> <selector> -- --exact --nocapture`.
These invocations are a reproduction index; they were not rerun for this
documentation-only change.

`v16_program_stateful_fuzz`:

```text
S1 inv_028_source_domain_realizability_cap::v16_program_source_lien_reversal_exit_matrix_preserves_bounded_exit
S2 inv_028_source_domain_realizability_cap::v16_program_cross_domain_rounding_exit_matrix_preserves_bounded_exit
S3 inv_028_source_domain_realizability_cap::v16_program_flat_source_lien_route_matrix_preserves_bounded_claim_exit
S4 inv_031_no_double_use_of_claim_backing_or_insurance_atoms::v16_program_two_source_claims_preserve_source_backing_single_use
S5 inv_063_backing_expiry_normalization::v16_program_resolved_close_normalizes_backing_at_expiry
S6 inv_063_backing_expiry_normalization::v16_program_retained_backing_topup_boundary_matrix
S7 inv_063_backing_expiry_normalization::v16_program_backing_expiry_conversion_boundary_matrix
S8 inv_063_backing_expiry_normalization::v16_program_backing_expiry_trade_route_boundary_matrix
```

`v16_cu`:

```text
C1 inv_028_source_domain_realizability_cap::v16_program_expired_source_lien_route_matrix_preserves_bounded_owner_exit
C2 inv_028_source_domain_realizability_cap::v16_program_source_capacity_admission_order_matrix_rejects_unreserved_risk
C3 inv_077_bounded_work_and_maximum_shape_compute::v16_attack_public_14_leg_28_source_domain_exit_stays_bounded
C4 inv_028_source_domain_realizability_cap::v16_program_shared_expiry_progress_matrix_preserves_terminal_progress
C5 inv_028_source_domain_realizability_cap::historical_latent_capacity::generation_capacity_admission::v16_program_used_generation_admission_reserves_latent_capacity_through_exact_exit
```

`v16_program_fuzz_regressions`:

```text
R1 inv_031_no_double_use_of_claim_backing_or_insurance_atoms::v16_program_cross_domain_backing_is_consumed_once
```

Source owners:

- [INV-028 CU](cu/inv_028_source_domain_realizability_cap.rs),
  [used-generation admission](cu/inv_028_generation_capacity_admission.rs), and
  [historical setup/mount](cu/inv_028_historical_latent_capacity.rs).
- [INV-028 stateful](stateful/inv_028_source_domain_realizability_cap.rs),
  [INV-031 stateful](stateful/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs),
  and [INV-031 fixed control](public_sbf/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs).
- [INV-063 stateful](stateful/inv_063_backing_expiry_normalization.rs) and
  [INV-077 public maximum-shape supplement](cu/inv_077_bounded_work_and_maximum_shape_compute.rs).
- [Discovery implementations](../support/invariant_discovery.rs) and
  [public-route fixture](../support/v16_svm.rs).

The inspected economic histories use public instructions for claims, reservations,
and backing transitions. This audit adds no account-byte mutation or restored
snapshot as evidence. Existing fixtures bootstrap runtime/accounts; source review
of a public history is not a claim that every helper or test elsewhere in a large
fixture module avoids synthetic state.

## Composition and remaining gaps

The existing INV-063 source-composition gate classifies backing consumers and
binds them to executable witnesses. INV-028's composition gate binds independent
source-credit accounting, capacity, lifecycle, and transition evidence; the
row-423 metadata guard specifically retains used-generation admission and the
broader exit-resource obligation. INV-031/032 own their single-use/lifecycle
composition gates. Their presence is useful drift coverage, not evidence that a
function-name roster proves implementation semantics.

No withheld-finding closure is inferred from a row's mapping. In particular,
row 423 remains OPEN, insurance-credit reservation is wrapper-unreachable under
the current pin, and arbitrary histories, every parameter permutation, and new
engine pins require separate evidence. This change does not expand receipt
redemption or custody-CPI coverage.

## Validation

The supplied reusable default-feature SBF has SHA-256
`87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.
It was hashed, not rebuilt or executed: no production, dependency, or test source
changed. A private copy of the existing host target avoids modifying another
worker's build products.

Setup commands (the first was run from the original repository; subsequent
repository-relative commands were run from the isolated worktree):

```bash
git worktree add -b astra-ultra/source-accounting-conformance-20260917 /dev/shm/percolator-astra-source-accounting-20260917 origin/main
sha256sum /dev/shm/percolator-public-gap-20260916-c91e-sbf-target/deploy/percolator_prog.so
cp -a --reflink=auto /dev/shm/percolator-public-gap-20260916-c91e-host-target /dev/shm/percolator-astra-source-accounting-20260917-host-target
```

Exact checks:

```bash
env CARGO_TARGET_DIR=/dev/shm/percolator-astra-source-accounting-20260917-host-target CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_every_public_invariant_source_and_declared_test_is_mounted -- --exact --nocapture
awk -F '\t' '!/^#/ && NF != 8 { print FNR ": expected 8 TSV fields, got " NF; bad=1 } END { exit bad }' tests/invariants/traceability_gaps.tsv
git diff --check
git diff --exit-code cdbe46fa9f33f2845c61f2727059162b8708c4d0 -- src Cargo.toml Cargo.lock
git diff --cached --check
git diff --cached --exit-code -- src Cargo.toml Cargo.lock
```

Results: mount census **1 passed, 0 failed, 146 filtered**, discovering **508
source files and 1,914 available tests**. Compilation emitted existing unused-code
and Solana future-compatibility warnings. TSV formatting, full staged-patch
whitespace, and production/dependency comparisons pass. No Rust file changed, so
Rust formatting and economic selector reruns are not applicable. The only new
files/content are documentation; no SBF or matcher build was needed.
