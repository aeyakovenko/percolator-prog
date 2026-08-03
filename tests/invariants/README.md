# Invariant-owned test coverage

This directory owns the security tests introduced by PR135. The normative statements and required
verification methods are in [`../../INVARIANTS.md`](../../INVARIANTS.md).

## Ownership rules

1. Every new security test has one primary `INV-NNN` owner and lives in that invariant's file.
2. A test may support secondary invariants; its module documentation names the primary guarantee
   boundary and the assertions it actually makes.
3. `public_sbf/` contains deterministic public-route regressions and exact economic assertions.
4. `stateful/` contains generated parameter/sequence variants of those public routes.
5. `cu/` contains real LiteSVM/SBF route, rollback, liveness, metamorphic, and compute tests.
6. The top-level Rust files are thin harnesses. Shared account builders and reference models remain
   in `tests/support/`; they are not tests and have no independent evidentiary status.
7. A finding-specific adapter is **Direct regression** evidence. It is not **Independent discovery**
   and cannot complete the known-finding benchmark by itself.
8. A module header must not claim a universal guarantee from one route, bounded input domain, or
   vulnerable-pin counterexample. Missing proof/fuzz/reachability methods remain explicit gaps.

## Current PR135 inventory

| Suite | Tests | Evidence |
| --- | ---: | --- |
| `public_sbf/` | 74 | Deterministic public SBF/LiteSVM counterexamples, regressions, and manifest checks |
| `stateful/` | 111 | Proptest-generated public routes, including forty-three finding-agnostic discovery properties |
| `cu/` | 74 | Positive public-route, metamorphic, rollback, liveness, and max-shape CU tests |
| `kani/` | 40 | Symbolic wrapper arithmetic, matcher-binding, and strict-decoder proofs |

The deterministic and stateful LoF adapters currently reproduce quarantined vulnerable behavior.
Their presence proves that a finding is reachable through the public wrapper; it does not certify
the invariant until a fixed pin rejects the attack or preserves the required safe outcome.

## Coverage status

Status meanings:

- **Direct** - finding-specific deterministic plus generated public-route evidence.
- **Independent** - a finding-agnostic public-action generator reached a normative invariant
  failure; finding-specific tests separately confirm concrete economic impact.
- **SVM/CU** - positive whole-route enforcement, liveness, rollback, metamorphic, or CU evidence.
- **P** - an invariant-owned Kani proof over deployed wrapper code; whole-route composition may
  still be outstanding.
- **Partial** - relevant legacy evidence exists outside the PR135 invariant modules or not all
  charter-required methods are present.
- **Gap** - no invariant-owned executable evidence yet.

No status in this table means “fully proven.” Full completion is governed by section 10 of the
charter.

| Invariant | Status | Primary PR135 owner |
| --- | --- | --- |
| INV-001 | Independent + Direct | `public_sbf/inv_001_market_incarnation_binding.rs`, `stateful/inv_001_market_incarnation_binding.rs` |
| INV-002 | Independent + Direct | `public_sbf/inv_002_asset_generation_binding.rs`, `stateful/inv_002_asset_generation_binding.rs` |
| INV-003 | Independent + Direct | `public_sbf/inv_003_portfolio_incarnation_binding.rs`, `stateful/inv_003_portfolio_incarnation_binding.rs` |
| INV-004 | Independent | `stateful/inv_004_position_episode_binding.rs` |
| INV-005 | Independent + Direct | `public_sbf/inv_005_authority_incarnation_binding.rs`, `stateful/inv_005_authority_incarnation_binding.rs` |
| INV-006 | Gap | - |
| INV-007 | Gap | - |
| INV-008 | Independent + Direct | `public_sbf/inv_008_intent_uniqueness_and_bounded_replay.rs`, `stateful/inv_008_intent_uniqueness_and_bounded_replay.rs` |
| INV-009 | Gap | - |
| INV-010 | Independent | `stateful/inv_010_out_of_order_safety.rs` |
| INV-011 | Gap | - |
| INV-012 | Gap | - |
| INV-013 | Gap | - |
| INV-014 | Independent + Direct | `public_sbf/inv_014_delayed_policy_and_policy_epoch_safety.rs`, `stateful/inv_014_delayed_policy_and_policy_epoch_safety.rs` |
| INV-015 | Gap | - |
| INV-016 | Gap | - |
| INV-017 | Gap | - |
| INV-018 | SVM/CU | `cu/inv_018_quote_mint_vault_token_program_and_authority_integrity.rs` |
| INV-019 | P + SVM/CU | `kani/inv_019_cpi_invocation_and_return_data_binding.rs`, `cu/inv_019_cpi_invocation_and_return_data_binding.rs` |
| INV-020 | Independent + Direct + SVM/CU | `public_sbf/inv_020_authenticated_clock_slot_and_oracle_provenance.rs`, `stateful/inv_020_authenticated_clock_slot_and_oracle_provenance.rs`, `cu/inv_020_authenticated_clock_slot_and_oracle_provenance.rs` |
| INV-021 | Gap | - |
| INV-022 | P | `kani/inv_022_instruction_decoding_and_schema_upgrade_safety.rs` |
| INV-023 | Gap | - |
| INV-024 | Gap | - |
| INV-025 | Gap | - |
| INV-026 | Gap | - |
| INV-027 | SVM/CU | `cu/inv_027_protected_principal_seniority.rs` |
| INV-028 | Independent + SVM/CU | `stateful/inv_028_source_domain_realizability_cap.rs`, `cu/inv_028_source_domain_realizability_cap.rs` |
| INV-029 | Gap | - |
| INV-030 | Gap | - |
| INV-031 | Independent + Direct | `public_sbf/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs`, `stateful/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs` |
| INV-032 | SVM/CU | `cu/inv_032_exact_counterparty_lien_lifecycle.rs` |
| INV-033 | Gap | - |
| INV-034 | Independent + Direct + SVM/CU | `public_sbf/inv_034_domain_and_instance_isolation.rs`, `stateful/inv_034_domain_and_instance_isolation.rs`, `cu/inv_034_domain_and_instance_isolation.rs` |
| INV-035 | Independent + Direct | `public_sbf/inv_035_no_global_b_pool_residuals_remain_local.rs`, `stateful/inv_035_no_global_b_pool_residuals_remain_local.rs` |
| INV-036 | Direct + SVM/CU | `public_sbf/inv_036_fee_destination_and_policy_version_integrity.rs`, `stateful/inv_036_fee_destination_and_policy_version_integrity.rs`, `cu/inv_036_fee_destination_and_policy_version_integrity.rs` |
| INV-037 | Gap | - |
| INV-038 | Independent + Direct | `public_sbf/inv_038_rounding_and_ratio_conservation.rs`, `stateful/inv_038_rounding_and_ratio_conservation.rs` |
| INV-039 | Independent + Direct | `public_sbf/inv_039_pending_loss_obligation_durability.rs`, `stateful/inv_039_pending_loss_obligation_durability.rs` |
| INV-040 | Gap | - |
| INV-041 | SVM/CU | `cu/inv_041_deterministic_allocation_and_caller_order_independence.rs` |
| INV-042 | Gap | - |
| INV-043 | Gap | - |
| INV-044 | Gap | - |
| INV-045 | Independent + Direct + P + SVM/CU | `public_sbf/inv_045_no_free_mark_movement.rs`, `stateful/inv_045_no_free_mark_movement.rs`, `kani/inv_045_no_free_mark_movement.rs`, `cu/inv_045_no_free_mark_movement.rs` |
| INV-046 | SVM/CU | `cu/inv_046_trade_availability_without_unsafe_mark_admission.rs` |
| INV-047 | SVM/CU | `cu/inv_047_equivalent_route_semantics.rs` |
| INV-048 | Gap | - |
| INV-049 | Gap | - |
| INV-050 | SVM/CU | `cu/inv_050_cross_zero_decomposition.rs` |
| INV-051 | Gap | - |
| INV-052 | Gap | - |
| INV-053 | Independent + Direct + SVM/CU | `public_sbf/inv_053_full_health_recertification_equivalence.rs`, `stateful/inv_053_full_health_recertification_equivalence.rs`, `cu/inv_053_full_health_recertification_equivalence.rs` |
| INV-054 | SVM/CU | `cu/inv_054_certificate_epoch_completeness.rs` |
| INV-055 | SVM/CU | `cu/inv_055_state_indexed_admission.rs` |
| INV-056 | Gap | - |
| INV-057 | SVM/CU | `cu/inv_057_risk_reduction_availability.rs` |
| INV-058 | Gap | - |
| INV-059 | SVM/CU | `cu/inv_059_fee_fragmentation_bound.rs` |
| INV-060 | Gap | - |
| INV-061 | Independent + SVM/CU | `stateful/inv_061_deterministic_bounded_liquidation.rs`, `cu/inv_061_deterministic_bounded_liquidation.rs` |
| INV-062 | Gap | - |
| INV-063 | Independent + Direct + SVM/CU | `public_sbf/inv_063_backing_expiry_normalization.rs`, `stateful/inv_063_backing_expiry_normalization.rs`, `cu/inv_063_backing_expiry_normalization.rs` |
| INV-064 | Gap | - |
| INV-065 | Gap | - |
| INV-066 | SVM/CU | `cu/inv_066_resolved_payout_fairness_and_order_independence.rs` |
| INV-067 | Independent + Direct | `public_sbf/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs`, `stateful/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs` |
| INV-068 | Gap | - |
| INV-069 | Gap | - |
| INV-070 | Gap | - |
| INV-071 | SVM/CU | `cu/inv_071_crank_progress.rs` |
| INV-072 | Gap | - |
| INV-073 | SVM/CU | `cu/inv_073_no_permanent_user_lock.rs` |
| INV-074 | SVM/CU | `cu/inv_074_scope_locality.rs` |
| INV-075 | Gap | - |
| INV-076 | Gap | - |
| INV-077 | Independent + SVM/CU | `cu/inv_077_bounded_work_and_maximum_shape_compute.rs` |
| INV-078 | SVM/CU | `cu/inv_078_permissionless_recovery_coverage.rs` |
| INV-079 | Direct | `public_sbf/inv_079_public_reachability_evidence.rs` |
| INV-080 | SVM/CU | `cu/inv_080_error_propagation_and_exact_rollback.rs` |
| INV-081 | Direct | `public_sbf/inv_081_success_state_validity_over_complete_public_routes.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs` |
| INV-082 | Gap | - |
| INV-083 | Gap | - |
| INV-084 | Gap | - |
| INV-085 | Gap | - |
| INV-086 | Direct | `public_sbf/inv_086_reference_model_and_deployed_transition_equivalence.rs` |
| INV-087 | Gap | - |
| INV-088 | Gap | - |
| INV-089 | Gap | - |

## Known-finding benchmark

`open_findings.tsv` is the unified 2026-08-03 snapshot of 143 open PRs whose titles identify a
public-route LoF or DoS class. It maps every row to a primary invariant. PR135 currently has 0
**Direct regression** rows, 38 **Missing** rows, 104 **Independent discovery** rows, and one
**Nonqualifying** row. The independent
rows are backed by finding-agnostic fingerprints in `independent_discoveries.tsv`; that mapping is
evidence metadata and is never consumed by a generator or oracle. The older
`tests/support/open_lof_manifest.rs` retains the executable adapter mapping for its 99-LoF snapshot.
Its `Quarantined` entries also mean **Direct regression**, not **Independent discovery**. The
known-finding completion criterion is therefore **not met**.

Every benchmark increment must:

1. snapshot every currently open public-route LoF and persistent-DoS finding;
2. map each finding to one or more normative invariants;
3. record vulnerable and fixed commits;
4. distinguish direct adapters from finding-agnostic discovery;
5. require a minimized public instruction trace with no out-of-band state mutation;
6. require exact SPL/lamport loss or a persistent funded-state exit lock;
7. reject “CU abort” as DoS unless every required user-progress route is unexecutable;
8. remain green while honestly reporting incomplete discovery coverage.

`nonqualifying_findings.tsv` is the equally strict negative roster. It may remove an open claim
from the gap count only when an invariant-owned public SBF test proves the pinned program is safe,
the alleged value is nonextractable, an honest bounded exit remains, or the claim is otherwise
outside the accepted public LoF/DoS definition. PR titles and fix-branch tests are not evidence.

Verification is complete only when that roster has zero `Missing` and zero `Direct regression`
entries.

## Commands

```bash
cargo check --tests
cargo test --test v16_program_fuzz_regressions
cargo test --test v16_program_stateful_fuzz
cargo test --test v16_cu
cargo kani --tests
```

At PR135 commit `1082c060a5461380560c28c162433143ed238714`, the full `v16_cu` command has
631 passing tests and these two intentionally red TDD probes:

- `v16_attack_pending_later_rounded_rescue_funding_requires_observation` (INV-053)
- `v16_probe_post_expiry_trade_cannot_charge_backing_fee` (INV-063)

They fail identically before and after the file-only reorganization. Until their fixes land, use
the following command to verify that all non-red CU tests pass without concealing the open probes:

```bash
cargo test --test v16_cu -- \
  --skip v16_attack_pending_later_rounded_rescue_funding_requires_observation \
  --skip v16_probe_post_expiry_trade_cannot_charge_backing_fee
```

Use `PERCOLATOR_FUZZ_CASES`, `PERCOLATOR_FUZZ_ACTIONS`, and
`PERCOLATOR_FUZZ_SHRINK_ITERS` to raise the generated stateful budget. Kani harness names now include
their `inv_NNN_*` module path; suffix filters can still target the original proof function names.
