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
| `public_sbf/` | 83 | Deterministic public SBF/LiteSVM counterexamples, regressions, decoder corpora, trace-schema checks, and manifest checks |
| `stateful/` | 121 | Proptest-generated public routes plus bounded lifecycle models for scarce-backing pair/chunk allocation orders, a 16-cell positive-claim boundary partition, all 3! matcher-control/trade landing orders, 20 user-operation/admission cells, 12 caller-priced boundary-exit cells, the four-state retirement-obligation lattice, a four-state Recovery resource-failure lattice, stale-refresh later-leg observation boundaries in Live and mixed Recovery/Live portfolios, a ten-prefix/two-configuration public crank-rank graph, all 133 public action words through depth two over an eleven-action deployed/reference alphabet, a Recovery crank/owner-exit classifier boundary, and all 5! claimant orders, including generalized active-leg/currentness, source-claim attribution, source-credit-rate, authenticated-expiry, state-indexed liveness witnesses, and reference-model/deployed-transition equivalence |
| `cu/` | 715 | Full `v16_cu` public-route, metamorphic, rollback, liveness, arithmetic-differential, and max-shape CU inventory, with no standalone top-level tests |
| `kani/` | 80 | Symbolic wrapper arithmetic, matcher-binding, ordering, strict-decoder, and proof-assumption nonvacuity harnesses; full `cargo kani --tests` remains the required verification command |

Most deterministic and stateful LoF adapters still reproduce quarantined vulnerable behavior;
fixed-pin regressions explicitly require safe rejection or preservation instead. A vulnerable-pin
counterexample proves public reachability but does not certify the invariant until the fixed pin
rejects the attack or preserves the required safe outcome.

The current fixed pin enforces matcher consent for CPI backing fees (PR223), ignores unsigned CPI
caller fees (PR224), requires bilateral no-CPI consent to the live base fee (PR310), and caps an
unsigned CPI LP's live base fee by its signed matcher policy. Matcher mutations now bind the
portfolio incarnation and a monotonic portfolio-local sequence, closing same-market portfolio
recreation and revoke-order replay. Whole-market recreation remains vulnerable when replacement
portfolio IDs and sequences are publicly realigned; INV-001 keeps that counterexample explicit.
All 14 retained matcher, oracle, fee, and resolve controls now use scope-local monotonic sequences,
closing same-market delayed overwrites including PR335/336/337/338/340/347/349. Market-generation
replay (including PR296/325/326), authority A -> B -> A revival, and PR339 backing-provider fee
consent remain explicit INV-001/INV-005/INV-014 gaps. All four signed trade routes, all six oracle
configuration/mark-push/restart routes, both insurance top-up routes, backing-bucket top-up,
asset-insurance withdrawal, and backing-fee policy updates now bind the asset's monotonic
`market_id`. This closes PR231/PR277/PR279/PR318/PR321/PR322/PR328 slot-reuse replay, including an
asset-0 shutdown/restart with the same insurance authority and oracle requests retained with
`u64::MAX` sequence. Whole-market resolve and permissionless-resolve policy bind the persisted
`next_market_id` asset-generation frontier, closing PR311/PR312 without incorrectly depending on
asset 0 alone. The INV-002 public-route matrix now reports zero generation-replay violations across
all 15 retained control families. Same-pubkey whole-market recreation remains an INV-001 concern
because a newly initialized market can begin with the same frontier value.

The wrapper-supported sparse source-domain liveness shape is `2 * WRAPPER_MAX_PORTFOLIO_ASSETS`
(28 domains). Public historical episodes can fill that shape; already-reserved domains and
risk-reducing exits remain live there. A risk-increasing trade on an unreserved asset must reject
before admitting a funded leg when the wrapper-supported source-domain budget is full. INV-028 owns
the admission-order matrix; INV-077 owns the CU/max-shape liveness regressions.

## Coverage status

Status meanings:

- **Direct** - finding-specific deterministic plus generated public-route evidence.
- **Independent** - a finding-agnostic public-action generator reached a normative invariant
  failure; finding-specific tests separately confirm concrete economic impact.
- **F** - a finding-agnostic stateful public-action generator enforces the invariant after each
  transition, without claiming that it independently rediscovered a benchmark finding.
- **SVM/CU** - positive whole-route enforcement, liveness, rollback, metamorphic, or CU evidence.
- **P** - an invariant-owned Kani proof over deployed wrapper code; whole-route composition may
  still be outstanding.
- **P harness** - an invariant-owned Kani harness is present, but its new result has not yet been
  executed in the current verification run.
- **Partial** - relevant legacy evidence exists outside the PR135 invariant modules or not all
  charter-required methods are present.
- **Gap** - no invariant-owned executable evidence yet.

No status in this table means “fully proven.” Full completion is governed by section 10 of the
charter.

| Invariant | Status | Primary PR135 owner |
| --- | --- | --- |
| INV-001 | Independent + Direct | `public_sbf/inv_001_market_incarnation_binding.rs`, `stateful/inv_001_market_incarnation_binding.rs` |
| INV-002 | Independent + Direct + SVM/CU | `public_sbf/inv_002_asset_generation_binding.rs`, `stateful/inv_002_asset_generation_binding.rs`, `cu/inv_002_asset_generation_binding.rs` |
| INV-003 | Independent + Direct + SVM/CU | `public_sbf/inv_003_portfolio_incarnation_binding.rs`, `stateful/inv_003_portfolio_incarnation_binding.rs`, `cu/inv_003_portfolio_incarnation_binding.rs` |
| INV-004 | Independent + P + SVM/CU | `stateful/inv_004_position_episode_binding.rs`, `kani/inv_004_position_episode_binding.rs`, `cu/inv_004_position_episode_binding.rs` (deterministic retained reduction/recovery-forfeit episode replay witness) |
| INV-005 | Independent + Direct + SVM/CU | `public_sbf/inv_005_authority_incarnation_binding.rs`, `stateful/inv_005_authority_incarnation_binding.rs`, `cu/inv_005_authority_incarnation_binding.rs` |
| INV-006 | SVM/CU | `public_sbf/inv_006_program_chain_message_type_and_version_binding.rs` (signed program, market, instruction bytes, and recent-blockhash mutation with exact rollback; explicit genesis-domain field remains absent) |
| INV-007 | Direct + Partial R | `public_sbf/inv_007_no_aba_reuse.rs` (a bounded public close/recreate/replay model exhausts all 11 retained market-scope route classes with compiled signer/meta traces and exact external deltas; every stale route still lands until persistent market generation binding is added, and other closable account classes remain) |
| INV-008 | Independent + Direct | `public_sbf/inv_008_intent_uniqueness_and_bounded_replay.rs`, `stateful/inv_008_intent_uniqueness_and_bounded_replay.rs` |
| INV-009 | SVM/CU | `cu/inv_009_partial_fill_and_retry_accounting.rs` |
| INV-010 | Independent + P + SVM/CU + Partial R | `stateful/inv_010_out_of_order_safety.rs`, `kani/inv_010_out_of_order_safety.rs`, `cu/inv_010_out_of_order_safety.rs` (all 3! landing orders of conflicting same-sequence matcher controls and a retained CPI trade, plus fresh-consent exit witnesses; other retained request domains remain) |
| INV-011 | SVM/CU + Spec gap | `cu/inv_011_signed_aggregate_economic_bounds.rs` (per-leg CPI signed price bounds and atomic batch rejection are covered; a single aggregate budget field remains absent) |
| INV-012 | SVM/CU | `cu/inv_012_capability_and_delegate_scope.rs` |
| INV-013 | SVM/CU + Cross-owner references | `cu/inv_013_destructive_consent_scope.rs` (public stale reduction episode rollback); related market/portfolio/position generation matrices live in INV-001, INV-003, and INV-004 |
| INV-014 | Independent + Direct + P + SVM/CU | `public_sbf/inv_014_delayed_policy_and_policy_epoch_safety.rs`, `stateful/inv_014_delayed_policy_and_policy_epoch_safety.rs`, `cu/inv_014_delayed_policy_and_policy_epoch_safety.rs`, `kani/inv_014_delayed_policy_and_policy_epoch_safety.rs` |
| INV-015 | SVM/CU | `public_sbf/inv_015_account_ownership_layout_discriminator_and_length_validity.rs` |
| INV-016 | SVM/CU | `cu/inv_016_canonical_pda_and_seed_binding.rs` (wrong-bump, cross-role, and cross-market substitutions over all 11 public custody routes) |
| INV-017 | SVM/CU + Partial M | `cu/inv_017_signer_writable_role_and_account_alias_safety.rs` exhausts all ten direct and all 21 CPI semantic account-pair aliases plus every required signer/writable downgrade for single and batch trade from nonvacuous public fixtures, with exact matcher/market/portfolio/vault rollback; targeted custody, ledger, helper, reward, and close aliases remain route-specific rather than pairwise-complete |
| INV-018 | SVM/CU | `cu/inv_018_quote_mint_vault_token_program_and_authority_integrity.rs` |
| INV-019 | P + SVM/CU | `kani/inv_019_cpi_invocation_and_return_data_binding.rs`, `cu/inv_019_cpi_invocation_and_return_data_binding.rs` |
| INV-020 | Independent + Direct + SVM/CU | `public_sbf/inv_020_authenticated_clock_slot_and_oracle_provenance.rs`, `stateful/inv_020_authenticated_clock_slot_and_oracle_provenance.rs`, `cu/inv_020_authenticated_clock_slot_and_oracle_provenance.rs` |
| INV-021 | SVM/CU | `cu/inv_021_account_creation_reallocation_close_rent_and_lamport_safety.rs` |
| INV-022 | P + SVM/CU + Prover gap | `kani/inv_022_instruction_decoding_and_schema_upgrade_safety.rs`, `public_sbf/inv_022_instruction_decoding_and_schema_upgrade_safety.rs`, and `cu/inv_022_instruction_decoding_and_schema_upgrade_safety.rs` cover symbolic field preservation, Kani trailing/truncation witnesses, raw public decoder rollback, a deterministic arbitrary-byte corpus, canonical round trips for all 50 tags, curated prior schemas, vector-length edges, exhaustive one-byte unknown/truncated tag rejection, and at least 1,200 deployed-SBF single-bit mutations spanning every tag plus each encoding's first, midpoint, and final payload positions with exact state rollback; the fully symbolic unknown-tag Kani query, generationless hybrid legacy Kani query, asset-lifecycle/base-unit all-fields Kani queries, tag-60 base-unit trailing-byte Kani query, and monolithic all-payload trailing-byte Kani shape remain solver cliffs and are backstopped by exhaustive host/SVM rosters |
| INV-023 | SVM/CU | `cu/inv_023_caller_input_confinement_for_derived_safety_state.rs` |
| INV-024 | F + SVM/CU + Partial | `cu/inv_024_attributed_quote_value_conservation.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs` (external SPL frames, exact deposit/withdraw flow, and public conservation regressions; full internal attribution proof remains open) |
| INV-025 | SVM/CU | `cu/inv_025_exact_stock_reconciliation.rs` |
| INV-026 | SVM/CU | `cu/inv_026_reservation_and_encumbrance_conservation_is_separate_from_token_value.rs` |
| INV-027 | SVM/CU | `cu/inv_027_protected_principal_seniority.rs` |
| INV-028 | Independent + SVM/CU | `stateful/inv_028_source_domain_realizability_cap.rs`, `cu/inv_028_source_domain_realizability_cap.rs` (28-domain admission-order matrix rejects unreserved risk before funded-leg admission while already-reserved domains remain closeable) |
| INV-029 | F + SVM/CU + Partial R | `stateful/inv_029_positive_claim_bounds_never_understate.rs`, `cu/inv_029_positive_claim_bounds_never_understate.rs` (whole-route source-claim lifecycle census plus a 16-cell min/max and odd/even boundary partition) |
| INV-030 | Independent + SVM/CU | `stateful/inv_030_credit_rate_determinism_and_fail_closed_behavior.rs`, `cu/inv_030_credit_rate_determinism_and_fail_closed_behavior.rs`, `cu/inv_063_backing_expiry_normalization.rs` (deterministic credit-rate lifecycle plus secondary expiry/progress owner) |
| INV-031 | Independent + Direct + SVM/CU | `public_sbf/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs`, `stateful/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs`, `cu/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs` (shared user credit, live domain insurance, and terminal insurance across primary/secondary collateral rails) |
| INV-032 | SVM/CU | `cu/inv_032_exact_counterparty_lien_lifecycle.rs` |
| INV-033 | SVM/CU + API gap | `cu/inv_033_insurance_backed_lien_single_classification.rs` proves the deployed public route creates counterparty-backed source liens without double-classifying them as insurance-backed, and that unreserved domain insurance cannot be silently consumed as source-credit backing; direct insurance-backed lien creation remains engine fuzz/Kani-only until a wrapper reservation route exists |
| INV-034 | Independent + Direct + SVM/CU | `public_sbf/inv_034_domain_and_instance_isolation.rs`, `stateful/inv_034_domain_and_instance_isolation.rs`, `cu/inv_034_domain_and_instance_isolation.rs` |
| INV-035 | Independent + Direct | `public_sbf/inv_035_no_global_b_pool_residuals_remain_local.rs`, `stateful/inv_035_no_global_b_pool_residuals_remain_local.rs` |
| INV-036 | Independent + Direct + SVM/CU | `public_sbf/inv_036_fee_destination_and_policy_version_integrity.rs`, `stateful/inv_036_fee_destination_and_policy_version_integrity.rs`, `cu/inv_036_fee_destination_and_policy_version_integrity.rs` |
| INV-037 | SVM/CU | `cu/inv_037_exact_residual_partition.rs` |
| INV-038 | Independent + Direct + SVM/CU | `public_sbf/inv_038_rounding_and_ratio_conservation.rs`, `stateful/inv_038_rounding_and_ratio_conservation.rs`, `cu/inv_038_rounding_and_ratio_conservation.rs` |
| INV-039 | Independent + Direct | `public_sbf/inv_039_pending_loss_obligation_durability.rs`, `stateful/inv_039_pending_loss_obligation_durability.rs` |
| INV-040 | SVM/CU | `cu/inv_040_no_fee_seniority.rs` (no-CPI, batch no-CPI, single-CPI, and batch-CPI underfunded exits drop uncollectible fees instead of senioritizing them; maintenance fee spam remains bounded) |
| INV-041 | SVM/CU + Partial R | `stateful/inv_041_deterministic_allocation_and_caller_order_independence.rs`, `cu/inv_041_deterministic_allocation_and_caller_order_independence.rs` (both equal-priority pair orders crossed with one-shot/dust force-close schedules under scarce backing; broader allocation orders remain) |
| INV-042 | SVM/CU + Spec gap | `cu/inv_042_recovery_fallback_envelope.rs` (public force-close admission, timing, pairing, and size bounds; full recovery price/value-transfer envelope remains engine/spec proof work) |
| INV-043 | Spec/API gap | No hedge/correlation-credit feature is exposed by the current wrapper route set; treat as N/A until the spec/API enables it |
| INV-044 | SVM/CU + Cross-owner references | `cu/inv_044_no_phantom_value_from_indices_certificates_or_labels.rs`; supporting stock/label/terminal coverage in INV-025, INV-026, INV-069, and INV-070 |
| INV-045 | Independent + Direct + P + SVM/CU | `public_sbf/inv_045_no_free_mark_movement.rs`, `stateful/inv_045_no_free_mark_movement.rs`, `kani/inv_045_no_free_mark_movement.rs`, `cu/inv_045_no_free_mark_movement.rs` |
| INV-046 | SVM/CU + Partial R | `stateful/inv_046_trade_availability_without_unsafe_mark_admission.rs`, `cu/inv_046_trade_availability_without_unsafe_mark_admission.rs` (all 12 caller-priced single/batch boundary cases cover zero rejection followed by price-one progress and maximum-price progress across publicly reached Active, DrainOnly, and Recovery states; CPI matcher prices, cross-zero, stale, reset, and resolved-close state spaces remain) |
| INV-047 | SVM/CU | `cu/inv_047_equivalent_route_semantics.rs` (empty-target oracle-crank equivalence, one-leg batch/single no-CPI fee equivalence, batch margin protection, zero-fill, capacity, and duplicate-asset route checks) |
| INV-048 | F + SVM/CU | `cu/inv_048_matched_trade_and_open_interest_coherence.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs` |
| INV-049 | F + SVM/CU | `cu/inv_049_canonical_single_net_leg_per_asset_generation.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs` |
| INV-050 | SVM/CU | `cu/inv_050_cross_zero_decomposition.rs` |
| INV-051 | F + SVM/CU | `cu/inv_051_canonical_adl_effective_quantity.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs` (shared post-transition pooled-OI/reset oracle) |
| INV-052 | SVM/CU | `cu/inv_052_split_merge_invariance.rs` (no-fee exact split/aggregate equivalence, fee-bearing split cannot reduce collected fees, and split withdraw custody equivalence) |
| INV-053 | Independent + Direct + SVM/CU | `public_sbf/inv_053_full_health_recertification_equivalence.rs`, `stateful/inv_053_full_health_recertification_equivalence.rs`, `cu/inv_053_full_health_recertification_equivalence.rs` (all trade-route/leg-order liquidation cells plus stale-refresh regressions requiring pending later-leg marks across ordinary Live and first-Recovery-leg portfolios; maximum 14-leg refresh remains 159,748 CU) |
| INV-054 | SVM/CU | `cu/inv_054_certificate_epoch_completeness.rs` |
| INV-055 | SVM/CU + Partial R | `stateful/inv_055_state_indexed_admission.rs`, `cu/inv_055_state_indexed_admission.rs` (public setup and exact economic/rollback oracles cover open, bilateral reduce, deposit, withdraw, and resolved payout across Active, DrainOnly, Recovery, and Resolved; reset-side and remaining instruction classes remain) |
| INV-056 | SVM/CU | `cu/inv_056_hints_are_discovery_only_favorable_actions_fully_refresh.rs` (primary withdraw/full-refresh rollback), with related crank-hint evidence in `cu/inv_023_caller_input_confinement_for_derived_safety_state.rs` and `cu/inv_072_order_robust_crankability.rs` |
| INV-057 | F + SVM/CU + Partial R | `stateful/inv_065_reset_recovery_and_retired_state_isolation.rs`, `cu/inv_057_risk_reduction_availability.rs` (generated public Recovery state retains all-portfolio exits; exhaustive lifecycle reachability remains) |
| INV-058 | SVM/CU | `cu/inv_058_cumulative_position_oi_notional_and_rate_limit_integrity.rs` |
| INV-059 | SVM/CU | `cu/inv_059_fee_fragmentation_bound.rs` |
| INV-060 | SVM/CU + Proof gap | `cu/inv_060_single_sided_margin_and_penalty_accounting.rs` (public margin-gap and lag-withdrawal gates; full certificate-lane decomposition remains proof/model work) |
| INV-061 | Independent + SVM/CU | `stateful/inv_061_deterministic_bounded_liquidation.rs`, `cu/inv_061_deterministic_bounded_liquidation.rs` |
| INV-062 | SVM/CU | `cu/inv_062_no_identity_assumptions_self_trade_containment.rs` |
| INV-063 | Independent + Direct + SVM/CU | `public_sbf/inv_063_backing_expiry_normalization.rs`, `stateful/inv_063_backing_expiry_normalization.rs`, `cu/inv_063_backing_expiry_normalization.rs` |
| INV-064 | SVM/CU | `cu/inv_064_insurance_withdrawal_policy_equivalence.rs` (live asset-domain route versus terminal market-wide route; configurable cooldown/cap fields remain spec-frontier/dead-control candidates) |
| INV-065 | F + SVM/CU + Partial R | `stateful/inv_065_reset_recovery_and_retired_state_isolation.rs`, `cu/inv_065_reset_recovery_and_retired_state_isolation.rs` (generated public policy-to-shutdown transition plus permissionless-progress and all-portfolio exit campaigns; exhaustive reset/recovery/retirement graph remains) |
| INV-066 | SVM/CU + M + Partial R | `stateful/inv_066_resolved_payout_fairness_and_order_independence.rs`, `cu/inv_066_resolved_payout_fairness_and_order_independence.rs` (all 5! basic claimant orders complete the same two-asset lifecycle with identical payouts; top-up, recovery, residue, and authority-refinement state spaces remain) |
| INV-067 | Independent + Direct + SVM/CU + Partial R | `public_sbf/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs`, `stateful/inv_066_resolved_payout_fairness_and_order_independence.rs`, `stateful/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs`, `cu/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs` (both terminal payout routes are retried at the fixed point after each claimant in all 5! basic orders) |
| INV-068 | SVM/CU | `cu/inv_068_receipt_uniqueness_and_monotonic_topups.rs` |
| INV-069 | SVM/CU + Partial R | `stateful/inv_069_terminal_normalization_and_retirement.rs`, `cu/inv_069_terminal_normalization_and_retirement.rs` (all four funded-insurance/funded-backing blocker states and both public drain orders are covered; other terminal obligation classes remain) |
| INV-070 | SVM/CU + Partial R | `stateful/inv_066_resolved_payout_fairness_and_order_independence.rs`, `cu/inv_070_zero_unattributed_terminal_residue_and_close_slab.rs` (a public two-asset lifecycle drains every portfolio and reaches `CloseSlab` in all 5! claimant orders) |
| INV-071 | Independent + SVM/CU + Partial R + Cross-owner counterexample | `cu/inv_071_crank_progress.rs`, `stateful/inv_082_state_indexed_liveness_theorem.rs` (ten public prefixes across two configurations record only strict lexicographic rank-decreasing crank edges and require every observed actionable rank class to reach zero; a Recovery-only stale certificate is classified as stale but neither empty nor apparently complete hints can dispatch a successful crank, while owner reduction remains live) |
| INV-072 | SVM/CU + M + Partial R | `cu/inv_072_order_robust_crankability.rs` (exhaustive 40-word three-asset hint alphabet through length three, malformed tails, valid-hint normalization, selected-mark observation requirements, and public expired-close recovery after adversarial hints) |
| INV-073 | Independent + F + SVM/CU + Partial R | `stateful/inv_065_reset_recovery_and_retired_state_isolation.rs`, `stateful/inv_066_resolved_payout_fairness_and_order_independence.rs`, `cu/inv_073_no_permanent_user_lock.rs` (generated Recovery exits and an exhaustive basic resolved-claim order model reach terminal disposition) |
| INV-074 | Independent + SVM/CU | `cu/inv_074_scope_locality.rs` (asset-local stale/bankruptcy isolation plus public asset-close locality for unrelated withdrawals and existing-position exits; new unrelated risk admission may still be conservatively blocked during active close) |
| INV-075 | SVM/CU + Partial R + Spec/implementation divergence | `cu/inv_075_close_priority_ownership_and_episode_integrity.rs` (owner/episode/replay checks plus both landing orders of two public equal-domain close starts through exact rejection, permissionless expiry/finalization, and rejected-contender exit; the engine implements first-landed exclusive domain ownership rather than the charter's strict preemption total order) |
| INV-076 | SVM/CU + Model gap | `cu/inv_076_close_drift_residual_durability_and_finalization_atomicity.rs` (public stale-cure and public-created zero-cure atomic rollback with terminal-progress checks; exhaustive close-phase fault injection remains) |
| INV-077 | Independent + SVM/CU | `cu/inv_077_bounded_work_and_maximum_shape_compute.rs` (supported 14-leg/28-source routes remain bounded; a production-derived registry maps all 50 tags to measured CU evidence; unreserved over-budget source-domain risk rejects atomically before CU exhaustion) |
| INV-078 | SVM/CU + Partial R | `stateful/inv_078_permissionless_recovery_coverage.rs`, `cu/inv_078_permissionless_recovery_coverage.rs` (all four absent/expired-backing by absent/tiny-insurance cells publicly create the same bankrupt exposure, then prove exact insurance spend, residual B booking, zero expired-backing support, and owner-callable terminal exits; lien impairment, payout conflict, and the complete lifecycle failure set remain) |
| INV-079 | Direct + Static rosters + Partial R | `public_sbf/inv_079_public_reachability_evidence.rs`, `public_sbf/inv_007_no_aba_reuse.rs` enforce the finding manifest and production/method rosters, mutation-test the public trace recorder, and replay all 11 whole-market ABA request classes with actual transaction signers, compiled account metas, exact token/lamport deltas, rejected-call rollback with the network fee classified separately, and zero out-of-band economic mutation; the remaining qualifying PoCs are not yet trace-normalized |
| INV-080 | P + SVM/CU | `kani/inv_080_error_propagation_and_exact_rollback.rs`, `cu/inv_080_error_propagation_and_exact_rollback.rs` prove every current engine error variant maps to a nonzero instruction `ProgramError` and cover partial oracle, legacy realloc, terminal top-up, token CPI, and over-withdraw engine-error rollback paths |
| INV-081 | F + Direct + SVM/CU | `public_sbf/inv_081_success_state_validity_over_complete_public_routes.rs`, `stateful/inv_066_resolved_payout_fairness_and_order_independence.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, `cu/inv_081_success_state_validity_over_complete_public_routes.rs` (fifteen generated operation classes share success/rollback/global-state oracles; a separate bounded owner composes trade, resolve, exact-once claim, portfolio close, and market close across all 5! claimant orders) |
| INV-082 | F route witness + SVM/CU + Partial R + Model/proof gap | `stateful/inv_082_state_indexed_liveness_theorem.rs`, `cu/inv_082_state_indexed_liveness_theorem.rs` require fixed public sequences to expose rank-decreasing permissionless progress, normal exits, liquidation progress, retained no-CPI execution under the state oracle, exact rollback of account substitutions, bad-hint noise, and no known-blocker quarantine. The bounded public graph requires every observed actionable rank class to reach zero through strict lexicographic edges. A public Recovery-only regression separates the engine's non-dispatchable crank state from the still-live owner exit; unobserved lifecycle classes and the complete reachable state space remain proof/model work. |
| INV-083 | SVM/CU + Machine roster | `cu/inv_083_boundary_completeness.rs` enforces named owners for zero, one, max-1, max, expiry edges, cross-zero, empty/full, and near-overflow classes; field-complete mapping remains open |
| INV-084 | P + Assumption inventory + Partial R + Proof-harness gap | `kani/inv_084_proof_assumptions_are_reachable_and_nonvacuous.rs` statically binds all eight current `kani::assume` sites to their exact source predicates and owning proofs, then exhausts each finite full-width admitted/excluded partition with boundary mutation killers; public-route establishment and implicit non-`assume` preconditions remain open |
| INV-085 | P + SVM/CU arithmetic differential + Proof gap | `kani/inv_085_proven_arithmetic_equals_deployed_arithmetic.rs`, `cu/inv_085_proven_arithmetic_equals_deployed_arithmetic.rs` cover deployed price-move and dt-clamp helpers against widened independent references with bounded symbolic Kani proofs plus deployed movement, funding, fee-supported mark clamping, and dynamic externality-fee boundary oracles; full deployed wide arithmetic versus bigint/Kani/BPF equivalence remains |
| INV-086 | Direct + F + Partial M + Partial R | `public_sbf/inv_086_reference_model_and_deployed_transition_equivalence.rs`, `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs` (fifteen generated public operation classes plus all 133 words through depth two over eleven value, trade, crank, policy, authority, backing, and lifecycle actions; every edge runs exact custody/account frames and independent position/OI/source-credit/stock oracles against one production SBF hash. Identity, lien, payout, authority-epoch, terminal-resolution, deeper sequences, and complete lifecycle models remain) |
| INV-087 | SVM/CU + Static roster + WrapperConfig inventory | `cu/inv_087_no_phantom_controls_or_dead_security_fields.rs` covers persisted policy writes plus public enforcement witnesses for permissionless resolve timing, activation cooldown, base-unit swaps, authority rotation, trade-fee admission, and exact liquidation cranker-share enforcement; it statically ties the wrapper-owned covered controls to source writer/enforcement edges and named witnesses, inventories every persisted `WrapperConfigV16` field, and classifies disabled insurance-withdraw policy fields as dead-control candidates rather than active protection; broader persisted-control inventory outside `WrapperConfigV16` remains |
| INV-088 | SVM/CU + Model gap | `cu/inv_088_global_summaries_are_not_account_local_proofs.rs` (public per-asset locality, multi-asset batch-no-CPI summary updates, same-asset multi-portfolio preservation, liquidation locality, and independent stored-count/ADL-effective OI scans); broader global-summary recomputation remains stateful/model work |
| INV-089 | SVM/CU | `cu/inv_089_activation_reactivation_and_initialization_equivalence.rs` |

## Exhaustiveness audit

Audit date: 2026-08-08. The answer to "is every invariant exhaustively proven or tested as much as
computationally feasible?" is **no**. The table above records evidence ownership, not completion.
This audit read the normative `Required tests` clause and the bodies of every owned and
cross-referenced test/proof for each invariant. Passing tests, file presence, a vulnerable-pin
counterexample, and a finding-specific regression do not by themselves close an invariant.

Verdicts used below:

- **OPEN-T** - a non-marginal, computationally tractable test, fuzz, metamorphic, Kani, static
  roster, or bounded-model increment is missing.
- **OPEN-D** - the invariant cannot close until a named implementation, API, persisted identity,
  ledger, or specification requirement exists or a currently reproduced violation is fixed.
- **FRONTIER** - direct whole-route proof or exhaustive reachability is currently solver/state-space
  limited, but the row names the strongest feasible decomposition or differential backstop still
  missing.
- **N/A** - the feature is not exposed by this wrapper. It must remain absent or be re-opened when
  the API is introduced.

There are no **CLOSED** rows in this audit. That does not mean the existing evidence is weak; it
means every non-N/A invariant still has at least one material charter clause or stronger feasible
method outstanding. The current ledger is 58 **OPEN-T**, 20 **OPEN-D**, 10 **FRONTIER**, and one
**N/A**.

### Cross-cutting coverage bugs

1. The charter requests `P` for 76 invariants, `F` for 85, `I` for 66, `M` for 32, `R` for 22,
   and `C` for 2. Invariant-owned directories currently exist for only 9 `P`, 27 `F`, and 87 `I`
   owners. File presence is only a lower bound; many owners cover one scenario rather than the
   required matrix. `special_method_coverage.tsv` now machine-indexes all `M`, `R`, and `C`
   obligations: all 32 `M` and both `C` rows have partial named evidence; 17 `R` rows now have
   bounded generated or exhaustive-topology evidence and the other 5 remain explicitly omitted.
2. The deployed decoder has 50 public instruction variants. The stateful public-interface model
   generates fifteen direct operation classes (trade, EWMA configuration, mark push, crank, deposit,
   withdraw, maintenance sync, matcher configuration, insurance top-up, backing top-up,
   released-PnL conversion, rebalance reduction, permissionless-resolve policy, asset shutdown, and
   oracle-authority rotation) plus replay/substitution meta-actions. The eight added classes now
   share exact rollback, token/account frame, ghost-position, and global-state
   oracles, but INV-081 still does not cover the complete public transition system.
3. Stateful suites default to 4 or 8 cases, generally 12 to 16 actions. Those are CI smoke budgets,
   not saturation evidence. There is no time-budgeted campaign, transition/branch coverage target,
   mutation score, or corpus-stability criterion for declaring a generator exhausted.
4. No general bounded BFS/model checker enumerates the reachable lifecycle graph required by
   INV-043, INV-057, INV-065, INV-073, or INV-084. INV-071/INV-082 have a narrow public
   crank-rank graph, and INV-086 exhausts all depth-two words over eleven public action classes;
   neither is a complete lifecycle graph. INV-066,
   INV-067, and INV-070 now have a narrower public two-asset model that exhausts all 5! basic
   claimant orders through exact-once retries and `CloseSlab`; INV-069 separately exhausts the
   four-state insurance/backing retirement-blocker lattice and both public drain orders; INV-010
   exhausts all 3! orders in its conflicting-control/trade topology; INV-029 exhausts a 16-cell
   public claim-attribution boundary partition; INV-041 covers a public scarce-backing pair/chunk
   ordering cross-product; INV-075 exhausts both landing orders for two equal-domain public close
   starts and demonstrates first-landed exclusion rather than priority preemption; INV-007
   exhausts all 11 retained request kinds across one public whole-market close/recreate boundary,
   and INV-079 records their compiled transaction and economic-delta traces; INV-078 crosses
   absent/expired backing with absent/tiny insurance and proves exact terminal residual
   classification in all four public Recovery cells; INV-055 has a separate public 20-cell core
   user-operation admission model; INV-046 has a separate 12-cell caller-priced
   boundary-exit model across three publicly reached lifecycle states; INV-072 exhausts all 40
   three-asset hint words through length three in one actionable topology.
5. Several liveness/admission tests create the interesting state with `set_account`,
   `mutate_market`, or benchmark seeding. That is valid for malformed-input and rollback testing,
   but it is not public-reachability evidence unless a separate public trace establishes the same
   pre-state.
6. Kani proofs cover local wrapper helpers. There is no complete wrapper-validation-to-engine-
   contract composition roster, and INV-084 now checks every explicit `kani::assume` but not every
   implicit branch/fixture precondition or its public-route establishment.
7. The known-finding benchmark is a dated snapshot. Independent rediscovery of its rows is useful
   regression evidence, but it cannot establish completeness against unknown attack classes or
   findings opened after the snapshot.

### Per-invariant coverage bugs

The last clause in each row is the strongest currently feasible closure. `AUDIT-NNN` identifiers
are machine-checked below so a future README edit cannot silently omit an invariant.

| Audit | Verdict | Known coverage bugs and strongest feasible closure |
| --- | --- | --- |
| AUDIT-001 | OPEN-D | The 11-route public matrix is a live whole-market same-pubkey ABA counterexample, not a safety proof. Add a persistent program-assigned market generation, reject every stale route before mutation, and add explicit cross-market and cross-program controls. |
| AUDIT-002 | OPEN-T | Fifteen retained asset-control families and all trade routes are covered, but claim/capability families, a deliberately weakened `(market_id, asset_index)` negative encoding, wrapper Kani composition, and full metamorphic replay coverage remain. |
| AUDIT-003 | OPEN-T | The 11 retained portfolio routes and ID lifecycle are covered; explicit same-owner-return, distinct claim/receipt consent, failed-init ID preservation, and a proof that every portfolio handler consumes `portfolio_id` remain. |
| AUDIT-004 | OPEN-T | Trade open/close, reduction, and recovery-forfeit episodes are covered. Cross-zero, side reset, conversion, close, claim, recovery conversion, terminal receipt, and resolved-claim episode transitions are missing from the replay matrix. |
| AUDIT-005 | OPEN-D | Current tests still discover authority `A -> B -> A` revival. Add a monotonic epoch for each authority scope, prove atomic epoch rotation, and flip both `A -> B -> A` and disable/re-enable matrices to rejection. |
| AUDIT-006 | OPEN-D | Transaction tampering covers program, market, kind, bytes, and blockhash, but no retained-message genesis/chain domain or explicit message-version field exists. Prefix-compatible, downgrade, and cross-cluster replay need a typed intent header and decoder fuzz. |
| AUDIT-007 | OPEN-D | A bounded public model now exhausts all 11 retained market-request kinds across one same-pubkey close/recreate boundary with zero state injection and exact trace frames, but every stale request still lands. Add a persistent market generation and flip this matrix to exact rejection, then extend it across multiple recreation depths and receipt, delegate, capability, and auxiliary-account classes. |
| AUDIT-008 | OPEN-D | Existing public tests intentionally reproduce duplicate execution across retry variants. A program-enforced intent ledger/expiry is missing, as are same-transaction, cross-entrypoint, and partial-failure exact-once tests. |
| AUDIT-009 | OPEN-T | One CPI short-fill rejection/retry is covered. Successful partial fills, random partitions, cumulative quantity/fee/slippage/expiry budgets, route switching, and one-minimum-fee-per-intent accounting are absent. |
| AUDIT-010 | OPEN-T | All 3! landing orders of two same-sequence matcher controls and one retained CPI trade are now exhausted with state-derived consent, exact rollback, and a matcher-independent exit. Add bounded permutations of deposit, withdraw, reduction, authority rotation, policy update, resolve, and claim with signed postcondition checks. |
| AUDIT-011 | OPEN-D | Per-leg prices and atomic batch rejection exist, but the message has no aggregate fee, quantity, slippage, deadline, final-position, or collateral/PnL-credit budget. Add those fields before split-intent proofs can close. |
| AUDIT-012 | OPEN-T | Matcher tuple and delegate checks are strong, but capability domain, authority epoch, expiry, allowed assets/operations, limits, and complete generation binding are not one-field-substitution tested or formally composed. |
| AUDIT-013 | OPEN-T | Coverage is limited to stale rebalance reduction and recovery forfeit. Add shutdown, resolve, close, liquidation delegation, recovery, claim, and receipt consent across every later lifecycle episode. |
| AUDIT-014 | OPEN-D | Same-incarnation sequence supersession is covered, but market recreation, authority ABA, and backing-provider fee consent remain live gaps. Bind policy consent to all relevant incarnations/epochs, then compare stricter and looser delayed policies metamorphically. |
| AUDIT-015 | OPEN-T | Owner, short length, magic, kind, and type-confusion cases exist. Version, padding, invalid enum, alignment, maximum length, every account class, and a proof that validation precedes every zero-copy view remain. |
| AUDIT-016 | OPEN-T | Tests reject wrong vault and matcher PDAs, but do not systematically test noncanonical bumps, reordered/omitted seeds, omitted generation fields, role-crossing, or valid PDAs from another market. |
| AUDIT-017 | OPEN-T | All four trade routes now exhaust their complete core account-pair spaces: ten direct pairs or 21 CPI/matcher pairs, plus every required signer/writable downgrade, from successful public controls; hostile cases reject with exact economic and matcher rollback. Custody, ledger, helper, reward, close, optional-tail, and remaining instruction schemas still need the same generated all-pairs matrix, with explicit successful controls for intentionally safe aliases. |
| AUDIT-018 | OPEN-T | SPL custody substitutions are extensive. Token-2022, fee-on-transfer/transfer-hook behavior, primary quote-decimal validation, and one independent actual-SPL-delta versus internal-accounting oracle across every value route remain. |
| AUDIT-019 | OPEN-T | Matcher return fields, stale data, req_id, tails, and local validation are covered. Add benign unrelated CPI before/after matcher return data, replace the injected matcher-context ABA setup with public close/recreate, and document that oracle paths are account reads rather than CPI. |
| AUDIT-020 | OPEN-T | Oracle provenance and authenticated clock tests are broad, but stored-slot rewind and expiry `-1/0/+1` are not crossed with every oracle mode and public consumer. No wrapper proof or complete clock/observation matrix exists. |
| AUDIT-021 | OPEN-T | Init growth, close rent, funded-close rejection, and reuse are covered. Residual claim/lien/recovery classes need a close/recreate matrix; impossible shrink and caller-selected close-destination cases should be proven N/A from the API. |
| AUDIT-022 | FRONTIER | Split Kani and exhaustive host/SVM decoder rosters backstop several solver cliffs. A deterministic 4,096-payload host corpus checks totality/canonicality, a canonical corpus locks all 50 tags, curated prior schemas plus vector-length boundaries reject, and a deployed-SBF matrix flips every bit at the tag and three boundary-sensitive payload positions for all schemas while requiring canonical decode-or-reject behavior and exact rollback. Duplicate-field N/A documentation, deeper interior-byte mutation, and per-tag proof decomposition for the remaining solver-cliff payloads remain. |
| AUDIT-023 | OPEN-T | The owner tests only late malformed crank hints. Build an instruction-field taint roster and fuzz every caller scalar/account to show it is signed intent, authenticated observation, or discovery-only rather than an economic safety input. |
| AUDIT-024 | OPEN-T | External SPL conservation is broad, but aggregate vault equality is not per-account/domain attribution. Add a stateful `TokenValueFlow` reference ledger after every successful transition and exact internal/external snapshots after every failure. |
| AUDIT-025 | OPEN-T | Three stock tests cover selected deposits, withdrawals, insurance/backing, and serialization. Recompute every stock class from raw state after every generated step through recovery, resolution, terminal close, rounding residue, and surplus. |
| AUDIT-026 | OPEN-T | One source-credit reservation path is covered. Creation, consumption, release, impairment, recovery, insurance reservations, pending obligations, close reserves, and retry/double-use need a common encumbrance lifecycle model. |
| AUDIT-027 | OPEN-T | Selected protected-principal paths are covered, but not every favorable operation/route from underbacked, loss-stale, and stale-certificate states. Add a generated route-by-state seniority matrix and normalized metamorphic outcomes. |
| AUDIT-028 | OPEN-T | Source reversal, expiry, rounding, sparse-capacity, and formula checks exist. Insurance impairment, cyclic `A-backs-B-backs-A`, omitted backing, and a bounded multi-domain proof against an independent cap model remain. |
| AUDIT-029 | OPEN-T | The exact public claim census now exhausts 16 lifecycle cells over min/max positions, odd/even partial-burn edges, and both claimant orders. Interior price moves, favorable funding bounds, rebucketing, stale uncertainty, exact-receipt replacement, and the complete production state graph remain. |
| AUDIT-030 | OPEN-T | The independent rate oracle covers claim/add/expiry/reduce/refill. Add impairment, omitted/malformed state, every source-credit mutation route, and a proof that only fresh backing or a valid claim-bound decrease can improve rate. |
| AUDIT-031 | OPEN-T | Shared credit and insurance rails are tested, while vulnerable-pin double-spend traces remain. Duplicate lien creation, cross-domain reservation, partial retry, and concurrent route use need one atom-ownership lifecycle oracle. |
| AUDIT-032 | OPEN-T | One force-close route checks lien sums. Differentially recompute bucket and domain aggregates across create, consume, release, impair, recover, and every injected failure point. |
| AUDIT-033 | OPEN-D | The wrapper exposes counterparty-backed liens but no direct insurance-backed lien creation/consume route. Add the API or rely on a named engine contract, then prove consume/release/impair/recovery classification is disjoint. |
| AUDIT-034 | OPEN-T | Cross-market/domain substitutions are broad but manually selected and often malformed through account injection. Generate every public instruction/account-domain substitution and require public controls plus normalized rollback. |
| AUDIT-035 | OPEN-T | Domain-local B settlement has fixed and generated evidence. Multi-asset bankruptcy order permutations and a pure proof that residuals cannot touch unrelated `(asset, side)` domains remain. |
| AUDIT-036 | OPEN-T | Major fee routes are covered, but the parasitic zero-activity asset, every policy epoch, and all single/batch/CPI/no-CPI fee-destination pairs are not one complete matrix; no whole-route fee-flow proof exists. |
| AUDIT-037 | OPEN-D | Current state does not expose every term in the normative residual partition, and tests cover selected liquidation counters. Add explicit drift/obligation/lien categories, then recompute the disjoint equality after continuation, preemption, cancel, recovery, and finalize. |
| AUDIT-038 | OPEN-D | Dust, funding, backing splits, and composite rounding are tested, but a fractional-cap violation remains quarantined. Add exact rational/residue accounting for resolved claims, B booking, and social-loss clearing and fix the live counterexample. |
| AUDIT-039 | OPEN-D | Many accrual-before-weight-removal routes are covered, but stale-cohort novation remains an expected violation. Transfer, reset, account close, and partial liquidation also need the common obligation-before-removal state machine. |
| AUDIT-040 | OPEN-T | Four underfunded trade routes and maintenance spam are covered. Matcher, liquidation, protocol, and maintenance fee variants with remaining senior obligations need the same protected-pool delta oracle. |
| AUDIT-041 | OPEN-T | A public scarce-backing topology now exhausts both equal-priority pair orders crossed with one-shot/dust force-close schedules and compares per-user claims plus domain classifications; observation order is also covered. Extend the model to liquidation, insurance, lien, residual, payout, claim, and close-preemption ordering. |
| AUDIT-042 | OPEN-D | Force-close admission/timing/size is tested, but no normative fallback price/value-transfer envelope exists. Define it, then test stale/unavailable reference, max positions/accounts, and just-inside/outside bounds. |
| AUDIT-043 | N/A | The wrapper exposes no hedge/correlation-credit feature. Keep a static absence check; if introduced, require exhaustive small portfolios, sign flips, missing legs, bucket edges, and scenario extremes before activation. |
| AUDIT-044 | OPEN-T | Selected B and parked-PnL cases plus cross-owner stock tests exist. Exercise every A/K/F/B index, certificate, claim bound, reservation, lien, tag, and soft-credit durable-use path through public transitions with token/encumbrance balance checks. |
| AUDIT-045 | OPEN-D | Clamp helpers and many route/boundary regressions are strong, but multiple public/stateful mark-movement exploit adapters still reproduce. Fix those violations, then use one accepted-mark-update generator over all modes/routes and raw `0/1/MAX`. |
| AUDIT-046 | OPEN-T | A public 12-cell model covers no-CPI single/batch exits at raw `0/1/MAX` across Active, DrainOnly, and Recovery, including exact rollback after zero and authenticated-mark/value preservation on successful boundaries; active off-mark CPI single/batch reductions have separate owners. Cross zero/stale/out-of-band matcher prices with reset and resolved-close modes, then establish the canonical reducing route for each remaining state. |
| AUDIT-047 | OPEN-T | Only one single-versus-batch no-CPI test compares identical snapshots; other tests are route-local checks. Add full pairwise CPI/no-CPI, single/batch, direct/composite, wrapper/engine metamorphic execution with normalized fees. |
| AUDIT-048 | OPEN-T | All four fresh trade routes scan OI, but liquidation, rebalance, reset, resolved close, and recovery are not directed owners. The stateful oracle skips equality in stale/obligation/unilateral states; model those classes explicitly. |
| AUDIT-049 | OPEN-T | All trade routes preserve one net leg, but transfer, reset, recovery, reactivation, and deserialization attachment attempts are absent. Add public transition matrices and malformed-deserialization negatives only where deserialization is an ingress. |
| AUDIT-050 | OPEN-T | Current tests cover normal no-CPI and batch lifecycle rejection, not partial liquidation, unrelated auxiliary OI, real ADL-effective decomposition, or all four routes. Add that public matrix with and without auxiliary OI. |
| AUDIT-051 | FRONTIER | Zero-effective-OI and selected ADL paths are covered, but no single pure effective-quantity oracle is compared across transfer, resize, rebalance, liquidation, clear, resolved close, recovery, retirement, and side reset. Extract and compose that oracle. |
| AUDIT-052 | OPEN-T | Deterministic trade and withdrawal partitions exist. Add arbitrary partition/permutation fuzz for liquidation, reduction, lien consumption, insurance withdrawal, claims, cooldowns, rates, and policy limits. |
| AUDIT-053 | FRONTIER | Omitted-leg liquidation findings and route/order fuzz are now joined by public stale-refresh regressions for a pending later Live mark behind either a current Live leg or a Recovery leg. These found and fixed a wrapper branch that checked only the first selected leg before whole-account certification; stale refresh and liquidation now scan every active leg, with a measured 159,748-CU 14-leg refresh. No full-certificate oracle runs after every transition, and pending obligations, impaired liens, ADL, and all penalty lanes are not composed. Prove or differentially establish fast <= full. |
| AUDIT-054 | OPEN-T | Two stale-certificate inputs are covered. Mutate active bitmap, generations, target/effective price, oracle epochs, A/K/F/B, source-credit epochs, liens, obligations, lifecycle/close modes, and policy epochs one at a time. |
| AUDIT-055 | OPEN-T | A public declarative matrix now covers all 20 combinations of open, bilateral reduce, deposit, withdraw, and resolved payout with Active, DrainOnly, Recovery, and Resolved. Every allowed cell must produce its exact economic delta and every forbidden cell must roll back all tracked bytes, SPL data, and lamports. Reset-side, close-ledger, retirement/reactivation, and the remaining public instruction classes still prevent a complete 50-instruction state cross-product. |
| AUDIT-056 | OPEN-T | Batch stale-related-leg and crank-hint cases exist. Omit/reorder/duplicate the worst and benign legs across withdraw, liquidation, conversion, claim, and all trade routes, comparing each result with canonical full discovery. |
| AUDIT-057 | FRONTIER | The generator now reaches a real funded Recovery state by public policy configuration and asset shutdown and requires all modeled positions to exit. It still does not establish an exit from every reachable lifecycle state; add a bounded public-only state search whose oracle finds a reducing action or terminal receipt. |
| AUDIT-058 | OPEN-T | TVL, large amount, over-reduce, top-up, and batch cap boundaries are covered. Generate every hard OI/notional/rate bound with zero/one/max/near-max, splitting, batching, cross-zero, route, transfer, and recreate variants. |
| AUDIT-059 | OPEN-T | One minimum-liquidation-fee case is covered; stale pre-CPI tests do not prove fragmentation. Compare aggregate close with one-atom closes, retries, mixed routes, and public partial failures under an episode-level fee oracle. |
| AUDIT-060 | FRONTIER | Public IM/MM and lag gates exist, but there is no independent decomposition of pending obligations, impaired liens, reserves, oracle lag, and penalties. Build a lane model and prove each component appears exactly once. |
| AUDIT-061 | OPEN-D | Liquidation safety, fees, progress, and selected generated schedules are covered, but public/stateful ADL close-order tests still assert violations. Fix those states, then add equal-risk permutations, arbitrary close splitting, normalized loss attribution, and max-shape liquidation coverage. |
| AUDIT-062 | OPEN-T | Selected self-trades show no identity privilege. Repeat common-control counterparties over every route/oracle mode and prove no unbacked value, mark-cost bypass, fee reclaim, or attribution change with a shared reference ledger. |
| AUDIT-063 | OPEN-T | Expiry regressions are broad, but add/consume/release/claim/close/payout/retire are not one complete consumer-by-`expiry-1/expiry/expiry+1` matrix, and no proof establishes normalization before every consumer. |
| AUDIT-064 | OPEN-D | Live and terminal insurance routes are tested, but the normative shared enable flag, cap, cooldown, policy epoch, and last-withdraw ledger are partly absent/dead controls. Specify or remove them, then interleave every route against one allowance ledger. |
| AUDIT-065 | OPEN-T | A generated public policy-to-shutdown route now reaches Recovery and retains all-portfolio exits under shared invariants, while selected reset/recovery gates remain. Several fixtures still use `mutate_market`; add public begin/finalize interleavings, every trade route, side isolation, retirement/reactivation, stale-generation attempts, and a bounded admission model using public setup only. |
| AUDIT-066 | OPEN-T | A public two-asset lifecycle now exhausts all 5! basic claimant orders with exact payout/vault reconciliation and identical outcomes. Extend that bounded model with authority refinement, partial top-ups, exact-bound replacement, recovery transitions, and a rational residue oracle. |
| AUDIT-067 | OPEN-D | Both payout routes are retried at a byte- and token-stable fixed point after every claimant across all 5! basic orders, but public/stateful terminal-dust tests still assert a reachable payout-erasure violation. Fix it, then model partial top-up, close/recreate, forfeit, and recovery conversion over every claim episode. |
| AUDIT-068 | OPEN-T | Replay and payout-rail tests exist. Add one-field receipt substitution for market/domain, portfolio incarnation, claim episode, face, snapshot, receipt ID, cross-portfolio, and asset-slot reuse, plus monotonic split top-ups. |
| AUDIT-069 | OPEN-T | A public bounded model now exhausts all four funded-insurance/funded-backing blocker states and both drain orders with exact rollback before terminal retirement. Spent/provider-receivable setups are still injected; recreate them publicly, then add reset history, price-only indices, expired labels, old epochs, pending loss/receipt controls, and their cross-product. |
| AUDIT-070 | OPEN-T | A complete public two-asset lifecycle now resolves, pays and dematerializes all five funded portfolios, proves zero accounting, and reaches `CloseSlab` across all 5! claimant orders while a foreign market remains byte-identical. Extend it with rounding, recovery, prior insurance, independent stock classification, and surplus sweep. |
| AUDIT-071 | OPEN-D | A ten-prefix/two-configuration public graph now records only strict lexicographic rank-decreasing crank edges, covers multiple rank components, and requires every observed actionable class to reach zero. Other owned tests still assert public lock/no-op discoveries: shutting down the sole-leg asset invalidates the certificate, the engine classifies the account stale, empty hints reject `NonProgress`, and a hint for the Recovery asset rejects `LockActive`; the owner can still reduce to zero. Fix that classifier/dispatch mismatch, then extend the graph to every crank class, lifecycle mode, close/recovery state, and maximum shape. |
| AUDIT-072 | OPEN-T | A public three-asset matrix now exhausts all 40 hint words through length three, including every bounded subset, ordering, and duplicate placement, plus selected out-of-range, malformed/absent oracle, and unclaimed account tails. Every case rejects atomically or lowers rank before an honest completion to rank zero. Extend that equivalence over every account-actionable crank class and the complete stale external-oracle tail space. |
| AUDIT-073 | OPEN-D | The stateful campaigns exit the designated liquidity provider after unilateral reduction, every modeled portfolio after public asset shutdown, and every basic resolved claimant across all 5! orders, but multiple owned tests still assert publicly reachable funded locks. Fix those locks, then build a small public state graph plus long sequences requiring every funded nonterminal node to reach principal return, a receipt, or authorized junior forfeit. |
| AUDIT-074 | OPEN-D | Unrelated base trading/withdrawal cases exist, but an owned public test still asserts an unrelated backed claim is blocked by asset-local bankruptcy. Fix or normatively justify that lock, then complete side, portfolio, domain, close, and receipt locality. |
| AUDIT-075 | FRONTIER | Both landing orders of two public equal-domain close starts now prove first-landed exclusion, exact rejected-contender rollback, immutable accepted identity, permissionless expiry/finalization after configured delays, and terminal exit of the rejected contender without the first owner's signature. This also demonstrates a normative mismatch: the public API and engine expose no strict `ClosePriority` tuple or preemption order. Decide whether exclusion is the specification; otherwise add priority/preemption semantics, then model restart, stale continuation, cure/cancel, owner deposit, and no-double-booking. |
| AUDIT-076 | OPEN-T | Only stale-cure and zero-cure rollback are owned. Add table-driven public fault injection at every close phase, price/funding drift, preemption/restart, durable residual booking, complete snapshots, and atomic OI/basis-clear checks. |
| AUDIT-077 | OPEN-T | The production-derived registry now maps all 50 instruction tags to named public-route and measured CU evidence with zero omissions; this tranche added explicit `InitMarket`, enabled/disabled `SetMatcherConfig`, and 5,834-slot `UpdateAssetLifecycle` measurements and indexed nine existing bounds. Complete the remaining maximum-dimension cross-product and activation-time rejection of unsupported shapes. |
| AUDIT-078 | OPEN-T | A four-state public model now crosses absent/expired backing with absent/tiny insurance after creating the same bankrupt exposure. Every cell reaches owner-callable terminal exits with zero expired-backing support, exact insurance spend, and exact residual B booking; INV-075 separately covers domain-close exclusion and eventual permissionless release. Add lien impairment, true B-exhaustion/booking failure, payout conflict, oracle-unavailable terminal fallback, and the remaining lifecycle failure classes, then compose them into bounded recovery reachability. |
| AUDIT-079 | OPEN-T | An opt-in LiteSVM trace schema now records actual transaction signers, compiled account metas, exact tracked token/lamport deltas, rejected writable-account rollback with the fee-payer network charge separated from program effects, and between-transaction economic mutation. Its detector is mutation-tested, and all 11 whole-market ABA cells require zero out-of-band mutation. Attach the schema to every remaining qualifying PoC and add a normalized terminal classification for exact loss, unauthorized withdrawable gain, bounded exit, or persistent funded lock. |
| AUDIT-080 | OPEN-T | Engine-error mapping and many SPL/realloc rollback paths are covered; the shared stateful rejection snapshot now includes every modeled economic account's lamports as well as program bytes and SPL data. Fault-inject every wrapper fallible stage outside that generated model and test a later instruction in the same transaction cannot consume success-only output. |
| AUDIT-081 | FRONTIER | The shared stateful model covers fifteen direct operation classes, and a separate bounded owner now composes public trade, resolution, all five claims, exact-once retries, portfolio closes, and market close across all 5! claimant orders. Authority epochs/ABA, retirement/reactivation, complex payout states, and those terminal operations inside the generated alphabet remain open; the runner also does not assert the full invariant suite after every success. Expand those classes and the executable invariant registry, then use typed-transition composition for routes whose whole-body proof remains intractable. |
| AUDIT-082 | FRONTIER | The first bounded public transition graph now composes ten public prefixes across two configurations with the deployed mode-aware rank, records only strict lexicographic crank reductions, and proves every observed actionable rank class has a path to zero. The reference model distinguishes Active/DrainOnly auto-crank work from Recovery owner-exit work and preserves the classifier/dispatch contradiction as a deterministic public regression. Expand the graph alphabet and state dimensions to all lifecycle, close, B, receipt, oracle-failure, and recovery classes; then connect each abstract node to a public-route reachability witness or a proven unreachability argument. |
| AUDIT-083 | OPEN-T | A machine-readable roster now requires actual invariant-owned tests for zero, one, max-1, max, expiry-1/equal/+1, cross-zero, empty/full, and near-overflow classes. It is class-level rather than field-complete; map every arithmetic/lifecycle field to the roster and add full-width and excluded-state reachability proofs. |
| AUDIT-084 | FRONTIER | A compile-time inventory classifies all eight current `kani::assume` sites across nine mounted Kani modules and binds each row to the exact source predicate and owning proof. A full-width symbolic partition proves admitted and excluded models and pins off-by-one, widening, and dropped-mark-clause mutation killers. Public-route establishment or named unreachability remains for each admitted domain, and implicit branch/fixture proof preconditions are not yet inventoried. |
| AUDIT-085 | FRONTIER | Selected price/funding/fee helpers match widened references on bounded domains. Full carry/borrow/multiply/divide/scale equivalence among Kani, host, BPF, and bigint remains; split by primitive and use differential full-boundary corpora where CBMC cliffs. |
| AUDIT-086 | OPEN-T | The reference runner checks fifteen generated public classes and now exhausts all 133 words through depth two over eleven value, trade, crank, matcher, backing, authority, resolve-policy, and lifecycle actions. The graph binds one production SBF hash, distinguishes 50+ normalized states and 100+ edges, and runs exact custody/account frames plus independent position/OI/source-credit/stock oracles after every edge. It still lacks independent identity, all-balance, lien, payout, authority-epoch, terminal-resolution, deeper sequences, and complete lifecycle state components; add those projections and extend the bounded alphabet/depth without treating this finite graph as universal equivalence. |
| AUDIT-087 | OPEN-T | The static roster inventories `WrapperConfigV16` and selected policies only. Inventory every persisted security field across all account types and require one writer, enforcement read, public mutation witness, or explicit removal/N/A classification. |
| AUDIT-088 | OPEN-T | Per-asset OI/count scans cover selected trade/liquidation orderings. Recompute every market/global accumulator from all bounded portfolios/domains after every relevant public transition and compare adversarial asset/account touch orders. |
| AUDIT-089 | OPEN-T | Fresh/reuse authority and price checks are broad, but full raw-state equivalence, support weight, source ledgers, certificate invalidation, residual state, stale epochs, generation increment, and unsupported-shape cases are not one differential matrix. |

## Known-finding benchmark

`open_findings.tsv` is the unified 2026-08-03 snapshot of 143 open PRs whose titles identify a
public-route LoF or DoS class. It maps every row to a primary invariant. PR135 currently has 0
**Direct regression** rows, 0 **Missing** rows, 124 **Independent discovery** rows, and nineteen
**Nonqualifying** rows. The independent
rows are backed by finding-agnostic fingerprints in `independent_discoveries.tsv`; that mapping is
evidence metadata and is never consumed by a generator or oracle. The older
`tests/support/open_lof_manifest.rs` retains the executable adapter mapping for its 99-LoF snapshot.
Its `Quarantined` entries also mean **Direct regression**, not **Independent discovery**. The
known-finding completion criterion is therefore **met for this dated snapshot and pinned engine**.

Every benchmark increment must:

1. snapshot every currently open public-route LoF and persistent-DoS finding;
2. map each finding to one or more normative invariants;
3. record vulnerable and fixed commits;
4. distinguish direct adapters from finding-agnostic discovery;
5. require a minimized public instruction trace with no out-of-band state mutation;
6. require exact SPL/lamport loss or a persistent funded-state exit lock;
7. reject “CU abort” as DoS unless every required user-progress route is unexecutable;
8. remain green while honestly reporting incomplete discovery coverage.

Every undiscovered qualifying trace is a test-suite gap. It must be classified as either a missing
normative oracle or missing public-sequence coverage (route, lifecycle mode, ordering, boundary,
account shape, or environmental variant). An `independent-discovery` row is accepted only when its
primary invariant matches the benchmark, its generator is an actual `#[test]` in that invariant's
module or an explicitly documented secondary owner, and the coverage index reports the same
invariant as Independent. Metadata alone cannot promote a finding.

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

On engine pin `9ffc4749a4b7e486f814090c7b43fb01a6df5dcf`, the full `v16_cu` inventory is
invariant-owned and has 707 passing tests. The former red PR220/PR366, PR367, live
source-backing expiry, and source-domain capacity admission probes are fixed-pin regressions under
INV-028, INV-030, INV-053, INV-063, and INV-077; the unfiltered command is the required
verification command.

Use `PERCOLATOR_FUZZ_CASES`, `PERCOLATOR_FUZZ_ACTIONS`, and
`PERCOLATOR_FUZZ_SHRINK_ITERS` to raise the generated stateful budget. Kani harness names now include
their `inv_NNN_*` module path; suffix filters can still target the original proof function names.
