# INV-028 latent-admission metadata fidelity

Base: `origin/main` at `e6495085`, fetched 2026-09-16.
Worktree: `/dev/shm/percolator-astra-ultra-inv024-044-20260916`.
Scope: INV-028 source capacity and INV-031 single use within the requested
INV-024 through INV-044 range. Production, dependency pins, benchmark rows, and
machine invariant statuses are unchanged.

## Gap

Review began with the invariant contracts and their composition gates. The
INV-028 gate preserves 23 capacity witnesses, but does not bind a discovery
mapping to the particular witness that supports it. The generic INV-079 gates
validate row shape, invariant ownership, executable test names, and an allowed
oracle vocabulary. They do not preserve the relationship between those fields
or the meaning of the broader exit-resource obligation.

The subsequent withheld-benchmark metadata comparison identified row 423:
`source-capacity/used-generation-latent-reservation`. Its witness includes
surviving latent legs, used asset generations, admission, and exact terminal
exit. The older admission-order witness mapped to rows 270/379 cannot replace
that scenario merely because both tests belong to INV-028. Neither bounded
witness closes the broader requirement to reserve every future settlement
resource needed for exit. No open PR implementation or fix was read or copied.

The new test pins the finding's INV-028 ownership and classification, the
INV-028/031 affected obligations, the historical/latent/admission product, the
full exit-resource property, and the reviewed discovery fingerprint, selector,
and oracle. Additional discovery evidence remains possible. OPEN/COVERED status
remains governed by the existing generic gates; this test does not promote it.
Existing source guards retain responsibility for witness presence. No Rust
source parsing is added.

## Negative controls

Each mutation was made alone in this worktree and restored after measurement.

| Temporary mutation | Five existing INV-079 gates | Existing INV-028 roster | New guard |
| --- | --- | --- | --- |
| Row 423 property changed to `risk-admission-reserves-only-vacant-source-domain-slots` | 5 passed | 1 passed | 1 failed at full exit-resource property |
| Row 423 selector/oracle replaced with the older admission-order pair | 5 passed | 1 passed | 1 failed at used-generation witness/oracle |
| Original metadata restored | 5 passed | 1 passed | 1 passed |

The second mutation retained the row 423 fingerprint but substituted
`v16_program_source_capacity_admission_order_matrix_rejects_unreserved_risk`
and `admitted-live-leg-must-reserve-a-settlement-source-slot`. These are real,
already accepted INV-028 evidence, so failure is not due to an unknown selector
or invalid oracle. Both old-check sets passed on base behavior; the only new
assertion was the added row-specific guard. The first old-check run preceded
the addition of that guard.

## Exact validation

All commands run from the worktree. The private build directory was copied from
an existing host cache; Cargo compiled the test targets for this worktree. No
listing-only, zero-test, SBF, or Kani run is counted as executed evidence.

```sh
export CARGO_TARGET_DIR=/dev/shm/percolator-astra-ultra-inv024-044-20260916-target
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 TMPDIR=/dev/shm
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0

# 5 passed on each of the two mutations and again on restored metadata.
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact \
  inv_079_public_reachability_evidence::v16_dated_open_security_finding_benchmark_is_non_overclaiming \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  --nocapture

# First mutation: 1 passed on the old guard, then 1 expected failure on the new.
cargo test --locked --offline --test v16_cu inv_028_source_domain_realizability_cap::v16_program_source_realizability_cap_composition_is_source_complete -- --exact --nocapture
cargo test --locked --offline --test v16_cu inv_028_source_domain_realizability_cap::v16_row423_metadata_preserves_latent_admission_evidence_and_exit_resource_obligation -- --exact --nocapture

# Second mutation: 1 passed / 1 expected failure. Restored metadata: 2 passed.
cargo test --locked --offline --test v16_cu -- --exact \
  inv_028_source_domain_realizability_cap::v16_row423_metadata_preserves_latent_admission_evidence_and_exit_resource_obligation \
  inv_028_source_domain_realizability_cap::v16_program_source_realizability_cap_composition_is_source_complete \
  --nocapture

rustfmt --check --edition 2021 --config skip_children=true tests/invariants/cu/inv_028_source_domain_realizability_cap.rs
git diff --check
```

Final result: **7 passed**, with every selector executed exactly and nonzero
counts. The two expected negative-control failures are separate from this total.
Builds report the existing Solana future-incompatibility notice and dead-code
warnings in the generic regression harness.

## Remaining gaps

Row 423 remains OPEN and INV-028 retains its existing `REFUTED_CURRENT` machine
status. This metadata guard does not establish the general admission/resource/
lifecycle theorem, execute the public-route witness, measure CU, validate its
assertion bodies, or strengthen the source roster's parser and module checks.
Arbitrary histories, simultaneous resource constraints, and maximum composite
shapes remain outside this increment. No economic rediscovery or whole-invariant
closure is claimed. Integration is suitable as a test/documentation change.
