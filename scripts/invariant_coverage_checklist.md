# Invariant Coverage Checklist

This is the coordinator's generic checklist for `INV-001` through `INV-089`.
It contains no holdout issue or PR data. A worker may mark an item only from
source and public-route evidence.

## Required evidence for every invariant

For each invariant, record all of the following before marking it covered:

- [ ] The invariant statement and its exact source-of-truth section are named.
- [ ] At least one public instruction sequence reaches the tested state.
- [ ] The test uses real account metas, signers, writable flags, SPL/lamport effects,
      and authenticated sysvars where relevant; no program-owned state injection.
- [ ] The oracle recomputes the property from raw pre/post state rather than trusting
      the field under test.
- [ ] Zero, one, maximum, boundary-minus-one, expiry, and cross-zero cases are covered
      wherever the invariant has numeric or lifecycle boundaries.
- [ ] The failing suffix or invalid input proves exact rollback and error propagation.
- [ ] At least one independent method is present: differential, metamorphic, stateful,
      bounded reachability, formal proof, or compute measurement.
- [ ] Existing selectors were searched and the new test is documented as non-redundant.
- [ ] The exact selector, SBF/program commit, peak CU, assumptions, and remaining scope
      are recorded next to the test.

## Per-invariant status

Each row must be updated by the coordinator after reviewing worker evidence. `P` is
formal proof, `F` stateful/property fuzzing, `I` SVM integration, `M` metamorphic,
`R` bounded reachability, and `C` compute evidence. `PARTIAL` is not `DONE`.

| ID | Status | Evidence tags | Public route/test selector | Remaining boundary or assumption |
| --- | --- | --- | --- | --- |
| INV-001 | TODO | | | |
| INV-002 | TODO | | | |
| INV-003 | TODO | | | |
| INV-004 | TODO | | | |
| INV-005 | TODO | | | |
| INV-006 | TODO | | | |
| INV-007 | TODO | | | |
| INV-008 | PARTIAL | P,F,I,M | `v16_backing_replay_across_sides_preserves_independent_insurance_retry` | Durable expiry and arbitrary retained-intent histories remain |
| INV-009 | TODO | | | |
| INV-010 | TODO | | | |
| INV-011 | TODO | | | |
| INV-012 | TODO | | | |
| INV-013 | TODO | | | |
| INV-014 | TODO | | | |
| INV-015 | TODO | | | |
| INV-016 | TODO | | | |
| INV-017 | TODO | | | |
| INV-018 | TODO | | | |
| INV-019 | TODO | | | |
| INV-020 | TODO | | | |
| INV-021 | TODO | | | |
| INV-022 | TODO | | | |
| INV-023 | TODO | | | |
| INV-024 | TODO | | | |
| INV-025 | TODO | | | |
| INV-026 | TODO | | | |
| INV-027 | TODO | | | |
| INV-028 | TODO | | | |
| INV-029 | TODO | | | |
| INV-030 | TODO | | | |
| INV-031 | TODO | | | |
| INV-032 | TODO | | | |
| INV-033 | TODO | | | |
| INV-034 | TODO | | | |
| INV-035 | PARTIAL | P,F,I,M | `v16_program_domain_local_b_composition_is_source_complete` | Full public transition recertification remains |
| INV-036 | TODO | | | |
| INV-037 | TODO | | | |
| INV-038 | TODO | | | |
| INV-039 | TODO | | | |
| INV-040 | TODO | | | |
| INV-041 | TODO | | | |
| INV-042 | TODO | | | |
| INV-043 | TODO | | | |
| INV-044 | TODO | | | |
| INV-045 | TODO | | | |
| INV-046 | TODO | | | |
| INV-047 | PARTIAL | F,I,M | `v16_program_fractional_close_partitions_preserve_double_ceil_fees_and_owner_payouts` | Seven/eight-leg four-route matrix remains |
| INV-048 | TODO | | | |
| INV-049 | TODO | | | |
| INV-050 | TODO | | | |
| INV-051 | TODO | | | |
| INV-052 | PARTIAL | F,I,M | `v16_program_fractional_close_partitions_preserve_double_ceil_fees_and_owner_payouts` | Non-integral split/merge families remain |
| INV-053 | TODO | | | |
| INV-054 | TODO | | | |
| INV-055 | TODO | | | |
| INV-056 | TODO | | | |
| INV-057 | TODO | | | |
| INV-058 | TODO | | | |
| INV-059 | TODO | | | |
| INV-060 | TODO | | | |
| INV-061 | TODO | | | |
| INV-062 | TODO | | | |
| INV-063 | TODO | | | |
| INV-064 | TODO | | | |
| INV-065 | TODO | | | |
| INV-066 | TODO | | | |
| INV-067 | TODO | | | |
| INV-068 | TODO | | | |
| INV-069 | TODO | | | |
| INV-070 | TODO | | | |
| INV-071 | TODO | | | |
| INV-072 | TODO | | | |
| INV-073 | TODO | | | |
| INV-074 | TODO | | | |
| INV-075 | TODO | | | |
| INV-076 | TODO | | | |
| INV-077 | TODO | | | |
| INV-078 | TODO | | | |
| INV-079 | TODO | | | |
| INV-080 | TODO | | | |
| INV-081 | PARTIAL | P,F,I | `v16_program_success_state_validity_composition_is_source_complete` | Whole-route transition execution remains |
| INV-082 | TODO | | | |
| INV-083 | PARTIAL | P,F,I,C | `v16_program_maintenance_u128_product_boundary_preserves_bounded_owner_exit` | Maximum-shape product families remain |
| INV-084 | TODO | | | |
| INV-085 | TODO | | | |
| INV-086 | PARTIAL | P,F,I,M,R | `v16_program_reference_model_dimension_composition_is_source_complete` | Current-pin public sequence equivalence remains |
| INV-087 | TODO | | | |
| INV-088 | TODO | | | |
| INV-089 | TODO | | | |

## Promotion gate

Do not promote a worker commit unless its row has a passing exact selector,
non-redundancy rationale, and no unreviewed production change. Do not declare the
holdout independently covered until every applicable row has at least one generic
test/proof and the coordinator's blind comparison is complete.
