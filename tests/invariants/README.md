# Invariant-owned test coverage

This directory owns the security tests introduced by PR135. The normative statements and required
verification methods are in [`../../INVARIANTS.md`](../../INVARIANTS.md).

## Current checkpoint

Updated 2026-08-18. The current PR135 production checkpoint pins engine commit
`0a23b5f5fc85ddb0223089c66c29cbf1600be62b`. It binds `ClosePortfolio` to the exact portfolio ID,
shared retained-owner-state sequence, and position epoch observed by the signer. Every successful
deposit advances that sequence, so an empty-state close retained before a later funded/trading
episode cannot erase that episode's funding telemetry after the account returns to empty. This uses
the existing persisted sequence lane and does not expand the account layout. The shared generated
public-transition model also
includes `ResolveMarket`, resolved-mode `PermissionlessCrank`, `CloseResolved`, and
`ClaimResolvedPayoutTopup`. Progress and owner-exit campaigns switch to terminal settlement after
resolution instead of treating live-route rejection as progress. Every successful terminal call
gets strict leg decoding, independent position/effective-OI reconciliation, receipt monotonicity,
exact destination/SPL-vault/engine-vault accounting, and account-frame checks; every rejection gets
exact program-byte, SPL, and lamport rollback checks. A bounded terminal sweep must drain every
modeled portfolio or report a nonterminal fixed point.

Verification at this checkpoint:

| Command/scope | Result | Freshness |
| --- | ---: | --- |
| Focused INV-013 delayed-close red/green and rollback scenarios | pass | rerun on the 2026-08-18 PR135 production head |
| `cargo check --tests` | pass | rerun on the 2026-08-18 PR135 test head |
| `cargo test --test v16_program_stateful_fuzz` | 134/134 | rerun on the 2026-08-18 PR135 production head |
| Registry/manifest checks in the INV-079 module | 8/8 | rerun at `e75b8a9e` |
| `cargo test --test v16_program_fuzz_regressions` | 87/87 | rerun on the 2026-08-18 PR135 production head |
| `cargo test --test v16_cu` | 720/720 | rerun on the 2026-08-18 PR135 production head |
| `cargo kani --tests -j 8 --output-format terse` | 82/82 | rerun on the 2026-08-18 PR135 production head |

This tranche changes the `ClosePortfolio` wire contract and deposit/close wrapper state transitions,
with matching test support. The locally rebuilt production SBF used by the 2026-08-18 LiteSVM run
has SHA-256
`d0d52ea43f32883794ca9317e4db1af61daef5f56aa31331a0836412be048f90`.

This is strong public-route evidence, not an exhaustive proof that the program is LoF/DoS-free.
The dated known-finding benchmark is fully classified, while the `AUDIT-*` rows below remain the
source of truth for incomplete state dimensions, route cross-products, public counterexamples,
and formal-composition gaps.

### Immediate next work

1. Complete the live open-issue coverage roster. The retained-operation matrix covers eleven
   operation families: it independently reproduces same-incarnation `ConvertReleasedPnl` retry
   value redirection (issue 387), while `RebalanceReduce` rejects the same-epoch retry with exact
   rollback on the current pin (issue 389). INV-013 now closes delayed `ClosePortfolio` consent
   (issue 402) with public red/green, generated ABA, rollback, fresh-close liveness, and Kani binding
   evidence.
2. Add public-SBF owners for the currently unmodeled rent persistence, selected-oracle-result
   timestamp, matcher-inventory reconciliation, maintenance-debt seniority, and crank-cadence
   findings (issues 404 through 409). Each owner must distinguish a live counterexample from a
   current-pin-safe control and record exact value, rollback, liveness, and CU outcomes.
3. Extend INV-086's bounded reference node and action alphabet with terminal resolution,
   payout-ledger, receipt, and close-progress state. Prove each normalized edge against the same
   deployed SBF transition while keeping the finite graph explicitly non-universal.
4. Cross the terminal reference graph with recovery, backing expiry, claimant order, prior
   insurance, authority epochs, retirement/reactivation, and the new retained-operation classes.

Wrapper proofs should remain wrapper-specific: decoding, account-role/authentication boundaries,
signed scope and ordering, engine-result propagation, custody deltas, and wrapper arithmetic. They
must not duplicate engine kernel proofs. A qualifying LoF/DoS finding still requires a public SBF
trace with valid account construction and no out-of-band mutation; a rejected transaction is
state-preserving because the wrapper returns the error and SVM rollback applies.

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
| `public_sbf/` | 87 | Deterministic public SBF/LiteSVM counterexamples, regressions, decoder corpora, trace-schema checks, and manifest checks, including paired-world conversion-retry extraction, fixed-pin rebalance-retry rejection, and issue-402 delayed-close red/green plus failed-deposit rollback |
| `stateful/` | 134 | Proptest-generated public routes, now including generated resolution and all three resolved payout rails plus an eleven-family retained-operation retry matrix, including same-incarnation PnL conversion and position-epoch-bound rebalance reduction; a generated close-consent ABA crosses arbitrary nonzero deposits and proves stale rejection plus fresh-close liveness; bounded lifecycle models cover scarce-backing pair/chunk allocation orders, a 16-cell positive-claim boundary partition, all 3! matcher-control/trade landing orders, all 32 open-route/close-route/winner-side realized-PnL attribution worlds, an independent raw-header/portfolio/domain stock census and account/bucket/reservation encumbrance census after every generated action, all eight trade-family/source-side counterparty-lien lifecycles, eight reciprocal cross-asset credit-cycle worlds, 20 user-operation/admission cells, 12 caller-priced boundary-exit cells, the four-state retirement-obligation lattice, a four-state Recovery resource-failure lattice, stale-refresh later-leg observation boundaries in Live and mixed Recovery/Live portfolios, a ten-prefix/two-configuration public crank-rank graph, all 133 public action words through depth two over an eleven-action deployed/reference alphabet, a Recovery crank/owner-exit classifier boundary, and all 5! claimant orders, including generalized active-leg/currentness, source-claim attribution, source-credit-rate, authenticated-expiry, state-indexed liveness witnesses, and reference-model/deployed-transition equivalence |
| `cu/` | 720 | Full `v16_cu` public-route, metamorphic, rollback, liveness, arithmetic-differential, and max-shape CU inventory, with no standalone top-level tests |
| `kani/` | 82 | Symbolic wrapper arithmetic, retained-close tuple binding, deposit-sequence invalidation, matcher binding, ordering, strict-decoder, and proof-assumption nonvacuity harnesses; full `cargo kani --tests` remains the required verification command |

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
| INV-008 | Independent + Direct | `public_sbf/inv_008_intent_uniqueness_and_bounded_replay.rs`, `stateful/inv_008_intent_uniqueness_and_bounded_replay.rs` (ten live duplicate-execution classes, including same-incarnation conversion retry; rebalance retry is position-epoch protected on the current pin) |
| INV-009 | SVM/CU | `cu/inv_009_partial_fill_and_retry_accounting.rs` |
| INV-010 | Independent + P + SVM/CU + Partial R | `stateful/inv_010_out_of_order_safety.rs`, `kani/inv_010_out_of_order_safety.rs`, `cu/inv_010_out_of_order_safety.rs` (all 3! landing orders of conflicting same-sequence matcher controls and a retained CPI trade, plus fresh-consent exit witnesses; other retained request domains remain) |
| INV-011 | SVM/CU + Spec gap | `cu/inv_011_signed_aggregate_economic_bounds.rs` (per-leg CPI signed price bounds and atomic batch rejection are covered; a single aggregate budget field remains absent) |
| INV-012 | SVM/CU | `cu/inv_012_capability_and_delegate_scope.rs` |
| INV-013 | P + F + SVM/CU + Cross-owner references | `public_sbf/inv_013_destructive_consent_scope.rs`, `stateful/inv_013_destructive_consent_scope.rs`, `kani/inv_013_destructive_consent_scope.rs`, and `cu/inv_013_destructive_consent_scope.rs` cover delayed close across a later funded/funding episode, arbitrary deposit/withdraw empty-state ABA, failed-deposit rollback, fresh-close liveness, exact close-binding and sequence contracts, and stale reduction rollback; related market/portfolio/position generation matrices live in INV-001, INV-003, and INV-004 |
| INV-014 | Independent + Direct + P + SVM/CU | `public_sbf/inv_014_delayed_policy_and_policy_epoch_safety.rs`, `stateful/inv_014_delayed_policy_and_policy_epoch_safety.rs`, `cu/inv_014_delayed_policy_and_policy_epoch_safety.rs`, `kani/inv_014_delayed_policy_and_policy_epoch_safety.rs` |
| INV-015 | SVM/CU | `public_sbf/inv_015_account_ownership_layout_discriminator_and_length_validity.rs` |
| INV-016 | SVM/CU | `cu/inv_016_canonical_pda_and_seed_binding.rs` (wrong-bump, cross-role, and cross-market substitutions over all 11 public custody routes) |
| INV-017 | SVM/CU + Partial M | `cu/inv_017_signer_writable_role_and_account_alias_safety.rs` exhausts all ten direct and all 21 CPI semantic account-pair aliases for single/batch trade, plus all 15 deposit and 21 withdraw account pairs and every required signer/writable downgrade, from nonvacuous public fixtures with exact matcher/market/portfolio/SPL rollback; ledger, helper, reward, close, optional-tail, and remaining aliases remain route-specific rather than pairwise-complete |
| INV-018 | SVM/CU | `cu/inv_018_quote_mint_vault_token_program_and_authority_integrity.rs` |
| INV-019 | P + SVM/CU | `kani/inv_019_cpi_invocation_and_return_data_binding.rs`, `cu/inv_019_cpi_invocation_and_return_data_binding.rs` |
| INV-020 | Independent + Direct + SVM/CU | `public_sbf/inv_020_authenticated_clock_slot_and_oracle_provenance.rs`, `stateful/inv_020_authenticated_clock_slot_and_oracle_provenance.rs`, `cu/inv_020_authenticated_clock_slot_and_oracle_provenance.rs` |
| INV-021 | SVM/CU | `cu/inv_021_account_creation_reallocation_close_rent_and_lamport_safety.rs` |
| INV-022 | P + SVM/CU + Prover gap | `kani/inv_022_instruction_decoding_and_schema_upgrade_safety.rs`, `public_sbf/inv_022_instruction_decoding_and_schema_upgrade_safety.rs`, and `cu/inv_022_instruction_decoding_and_schema_upgrade_safety.rs` cover symbolic field preservation, Kani trailing/truncation witnesses, raw public decoder rollback, a deterministic arbitrary-byte corpus, canonical round trips for all 50 tags, curated prior schemas, vector-length edges, exhaustive one-byte unknown/truncated tag rejection, and at least 1,200 deployed-SBF single-bit mutations spanning every tag plus each encoding's first, midpoint, and final payload positions with exact state rollback; the fully symbolic unknown-tag Kani query, generationless hybrid legacy Kani query, asset-lifecycle/base-unit all-fields Kani queries, tag-60 base-unit trailing-byte Kani query, and monolithic all-payload trailing-byte Kani shape remain solver cliffs and are backstopped by exhaustive host/SVM rosters |
| INV-023 | SVM/CU + Source-bound roster | `cu/inv_023_caller_input_confinement_for_derived_safety_state.rs` and `inv_023_caller_input_roster.tsv` classify every field in all 50 production instruction variants and the three nested public input structs as signed configuration/economics, identity/scope, authenticated time, replay/bounded-work control, discovery-only input, no caller data, or an explicitly ignored legacy field, and bind every row to an executable witness; late malformed crank hints also prove exact rollback and nonvacuous progress. Per-field dynamic boundary mutation, a complete account-input roster, and alternate-entrypoint substitution remain. |
| INV-024 | F + SVM/CU + Partial | `cu/inv_024_attributed_quote_value_conservation.rs`, `stateful/inv_024_attributed_quote_value_conservation.rs`, and `stateful/inv_081_success_state_validity_over_complete_public_routes.rs` cover external SPL frames, exact custody flows, aggregate conservation, and all 32 combinations of four public open routes, four public close routes, and both account-A sides with exact winner/loser PnL, conversion, payout, claim cleanup, token supply, and unrelated-account frames. A general per-transition owner/domain `TokenValueFlow` ledger and formal whole-route composition remain open. |
| INV-025 | F + SVM/CU + Partial | `stateful/inv_025_exact_stock_reconciliation.rs`, `cu/inv_025_exact_stock_reconciliation.rs`, and the shared post-transition census independently sum every materialized portfolio's capital/positive-PnL/escrow/status counts and every source domain's claims/backing/reservations/budgets/earnings/blockers, compare those sums with decoded state and the raw zero-copy header, reconcile engine custody exactly with SPL custody, and require explicit senior stocks plus a nonnegative derived junior residual after every generated action. The public owner lifecycle crosses insurance, backing, trade settlement, route-switched close, PnL conversion, backing withdrawal, and user withdrawals. Rounding residue and protocol surplus remain a derived residual because the deployed layout has no independent persisted stock-class ledgers for them. |
| INV-026 | F + SVM/CU + Partial | `stateful/inv_026_reservation_and_encumbrance_conservation.rs` and `cu/inv_026_reservation_and_encumbrance_conservation_is_separate_from_token_value.rs` run a shared independent account/bucket/reservation census after every generated public action and exhaust all four trade families times both source sides through nonzero counterparty-lien creation, out-of-order resolved close, terminal release, and exact backing consumption/provider receivable accounting. The census treats expired counterparty backing as account-local backing matched by market valid-plus-impaired state, rather than assuming it remains valid. Direct insurance-backed lien creation has no wrapper route; public impairment/recovery cross-products, pending obligations, and close reserves remain. |
| INV-027 | Independent + F + SVM/CU + Counterexample | `stateful/inv_027_protected_principal_seniority.rs` and `cu/inv_027_protected_principal_seniority.rs`. The stateful owner runs the historical-winner/fresh-entrant terminal payout trace through all four trade families and independently demonstrates that aggregate token conservation can coexist with fresh-principal subordination on the pinned engine. |
| INV-028 | Independent + SVM/CU | `stateful/inv_028_source_domain_realizability_cap.rs`, `cu/inv_028_source_domain_realizability_cap.rs` cover source reversal, expiry, rounding, the 28-domain admission-order boundary, and eight reciprocal cross-asset worlds proving full recertification cannot turn mutually offsetting claims or unattached backing into usable credit. The shared-expiry matrix independently exposed PR302's prospective-loss lock and, after that prerequisite was fixed, PR300's later provider-lien provenance underflow. On the fixed pin it alternates both public terminal routes, requires exact rollback on every error, and drives all four funded portfolios to terminal disposition. Retained public counterexamples still expose other cross-domain conversion-attribution and funded-exit gaps on the pinned engine. |
| INV-029 | F + SVM/CU + Partial R | `stateful/inv_029_positive_claim_bounds_never_understate.rs`, `cu/inv_029_positive_claim_bounds_never_understate.rs` (whole-route source-claim lifecycle census plus a 16-cell min/max and odd/even boundary partition) |
| INV-030 | Independent + SVM/CU | `stateful/inv_030_credit_rate_determinism_and_fail_closed_behavior.rs`, `cu/inv_030_credit_rate_determinism_and_fail_closed_behavior.rs`, `cu/inv_028_source_domain_realizability_cap.rs`, `cu/inv_063_backing_expiry_normalization.rs` cover the deterministic credit-rate lifecycle plus secondary expiry/progress ownership. The shared-expiry matrix independently reached PR302's impaired-domain prospective-loss rollback fixed point; the fixed pin now reconciles that loss and reaches terminal disposition. |
| INV-031 | Independent + Direct + SVM/CU | `public_sbf/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs`, `stateful/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs`, `cu/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs` (shared user credit, live domain insurance, and terminal insurance across primary/secondary collateral rails) |
| INV-032 | SVM/CU | `cu/inv_032_exact_counterparty_lien_lifecycle.rs` plus the shared-expiry lifecycle in `cu/inv_028_source_domain_realizability_cap.rs`, which requires exact provider-label retirement, sibling-label preservation, zero residual aggregate provider impairment, and no insurance-provenance substitution. |
| INV-033 | SVM/CU + API gap | `cu/inv_033_insurance_backed_lien_single_classification.rs` proves the deployed public route creates counterparty-backed source liens without double-classifying them as insurance-backed, and that unreserved domain insurance cannot be silently consumed as source-credit backing; direct insurance-backed lien creation remains engine fuzz/Kani-only until a wrapper reservation route exists |
| INV-034 | Independent + Direct + SVM/CU | `public_sbf/inv_034_domain_and_instance_isolation.rs`, `stateful/inv_034_domain_and_instance_isolation.rs`, `cu/inv_034_domain_and_instance_isolation.rs` |
| INV-035 | Independent + Direct + SVM/CU + M | `public_sbf/inv_035_no_global_b_pool_residuals_remain_local.rs`, `stateful/inv_035_no_global_b_pool_residuals_remain_local.rs`, and `cu/inv_074_scope_locality.rs` cover exact two-asset B attribution plus a 32-cell ambiguous-domain matrix spanning all four trade routes, both loss-asset identities, both close orders, and both position directions. A final reduction with a uniquely attributable residual preserves an asset-local close ledger until a permissionless crank books the loss; an ambiguous account deficit cannot charge the last touched asset or force unrelated live markets into Recovery, and instead reaches terminal settlement through the configured permissionless stale-market policy. |
| INV-036 | Independent + Direct + SVM/CU | `public_sbf/inv_036_fee_destination_and_policy_version_integrity.rs`, `stateful/inv_036_fee_destination_and_policy_version_integrity.rs`, `cu/inv_036_fee_destination_and_policy_version_integrity.rs` |
| INV-037 | SVM/CU | `cu/inv_037_exact_residual_partition.rs` |
| INV-038 | Independent + Direct + SVM/CU | `public_sbf/inv_038_rounding_and_ratio_conservation.rs`, `stateful/inv_038_rounding_and_ratio_conservation.rs`, `cu/inv_038_rounding_and_ratio_conservation.rs` |
| INV-039 | Independent + Direct | `public_sbf/inv_039_pending_loss_obligation_durability.rs`, `stateful/inv_039_pending_loss_obligation_durability.rs`; INV-027 owns the terminal owner-attribution assertion for the cross-cutting stale-cohort novation counterexample. |
| INV-040 | SVM/CU | `cu/inv_040_no_fee_seniority.rs` (no-CPI, batch no-CPI, single-CPI, and batch-CPI underfunded exits drop uncollectible fees instead of senioritizing them; maintenance fee spam remains bounded) |
| INV-041 | SVM/CU + Partial R | `stateful/inv_041_deterministic_allocation_and_caller_order_independence.rs`, `cu/inv_041_deterministic_allocation_and_caller_order_independence.rs` (both equal-priority pair orders crossed with one-shot/dust force-close schedules under scarce backing; broader allocation orders remain) |
| INV-042 | SVM/CU + Spec gap | `cu/inv_042_recovery_fallback_envelope.rs` (public force-close admission, timing, pairing, and size bounds; full recovery price/value-transfer envelope remains engine/spec proof work) |
| INV-043 | Spec/API gap | No hedge/correlation-credit feature is exposed by the current wrapper route set; treat as N/A until the spec/API enables it |
| INV-044 | SVM/CU + Cross-owner references | `cu/inv_044_no_phantom_value_from_indices_certificates_or_labels.rs`; supporting stock/label/terminal coverage in INV-025, INV-026, INV-069, and INV-070 |
| INV-045 | Independent + Direct + P + SVM/CU | `public_sbf/inv_045_no_free_mark_movement.rs`, `stateful/inv_045_no_free_mark_movement.rs`, `kani/inv_045_no_free_mark_movement.rs`, `cu/inv_045_no_free_mark_movement.rs` |
| INV-046 | SVM/CU + Partial R | `stateful/inv_046_trade_availability_without_unsafe_mark_admission.rs`, `cu/inv_046_trade_availability_without_unsafe_mark_admission.rs` (all 12 caller-priced single/batch boundary cases cover zero rejection followed by price-one progress and maximum-price progress across publicly reached Active, DrainOnly, and Recovery states; CPI matcher prices, cross-zero, stale, reset, and resolved-close state spaces remain) |
| INV-047 | SVM/CU + M | `cu/inv_047_equivalent_route_semantics.rs` covers empty-target oracle-crank equivalence, one-leg batch/single no-CPI fee equivalence, batch margin protection, zero-fill, capacity, and duplicate-asset route checks; `stateful/inv_024_attributed_quote_value_conservation.rs` independently exhausts all 32 combinations of four public open routes, four public close routes, and both account-A sides with exact owner-level realized PnL, conversion, and payout. |
| INV-048 | Independent + F + SVM/CU | `cu/inv_048_matched_trade_and_open_interest_coherence.rs`, `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, and `stateful/inv_071_crank_progress.rs`. Fresh-state scans cover all four trade routes; the stateful model separately derives exact pooled-OI deltas across matched/retained trades, crank liquidation, owner rebalance, reset cleanup, and recovery forfeit. The four-route bankruptcy matrix additionally proves the final matched reduction clears effective OI while preserving exactly one zero-basis stored leg and one pending obligation, and that terminal payout clears both without resurrecting effective OI. |
| INV-049 | F + SVM/CU | `cu/inv_049_canonical_single_net_leg_per_asset_generation.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs` |
| INV-050 | SVM/CU + M + Counterexample | `cu/inv_050_cross_zero_decomposition.rs` covers lifecycle exact-close admission, initial-margin flips, and all four public trade routes after a real partial liquidation. The latter matrix holds the signed Flip request fixed and varies only unrelated auxiliary OI: the control rejects atomically, while auxiliary OI admits the unfixed PR250/engine-134 basis-reissue path and makes fresh current-`A` legs exceed pooled effective OI by exactly the prior ADL haircut. |
| INV-051 | Independent + F + SVM/CU | `cu/inv_051_canonical_adl_effective_quantity.rs`, `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, and `stateful/inv_071_crank_progress.rs`. Directed ADL exit matrices are joined by an exact transition-derived pooled-OI ledger that deliberately separates retained raw basis from effective OI; the bankruptcy matrix separately pins the zero-effective-OI pending-obligation boundary through terminal payout. |
| INV-052 | SVM/CU | `cu/inv_052_split_merge_invariance.rs` (no-fee exact split/aggregate equivalence, fee-bearing split cannot reduce collected fees, and split withdraw custody equivalence) |
| INV-053 | Independent + Direct + SVM/CU | `public_sbf/inv_053_full_health_recertification_equivalence.rs`, `stateful/inv_053_full_health_recertification_equivalence.rs`, `cu/inv_053_full_health_recertification_equivalence.rs` (all trade-route/leg-order liquidation cells plus stale-refresh regressions requiring pending later-leg marks across ordinary Live and first-Recovery-leg portfolios; maximum 14-leg refresh remains 159,748 CU) |
| INV-054 | SVM/CU | `cu/inv_054_certificate_epoch_completeness.rs` creates a source-backed released-PnL claim entirely through public trade/mark/crank/close routes, then separately demonstrates stale favorable-action rollback and public refresh after oracle-target, isolated funding, isolated source-credit/risk, and asset-set mutations. Every deployed certificate key (`oracle`, `funding`, `risk`, `asset_set`, and account bitmap) is asserted by one shared currentness oracle. |
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
| INV-067 | Independent + Direct + SVM/CU + Partial R | `public_sbf/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs`, `stateful/inv_066_resolved_payout_fairness_and_order_independence.rs`, `stateful/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs`, `stateful/inv_071_crank_progress.rs`, and `cu/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs`. Both terminal payout routes are retried at the fixed point after each claimant in all 5! basic orders. A separate 24-world matrix reaches resolution through a publicly booked bankruptcy obligation and varies all four trade routes, three claimant orders, and both payout-route priorities with exact per-owner payout equivalence. |
| INV-068 | SVM/CU | `cu/inv_068_receipt_uniqueness_and_monotonic_topups.rs` |
| INV-069 | SVM/CU + Partial R | `stateful/inv_069_terminal_normalization_and_retirement.rs`, `cu/inv_069_terminal_normalization_and_retirement.rs` (all four funded-insurance/funded-backing blocker states and both public drain orders are covered; other terminal obligation classes remain) |
| INV-070 | SVM/CU + Partial R | `stateful/inv_066_resolved_payout_fairness_and_order_independence.rs`, `cu/inv_070_zero_unattributed_terminal_residue_and_close_slab.rs` (a public two-asset lifecycle drains every portfolio and reaches `CloseSlab` in all 5! claimant orders) |
| INV-071 | Independent + SVM/CU + Partial R + Cross-owner counterexample | `stateful/inv_071_crank_progress.rs`, `cu/inv_071_crank_progress.rs`, and `stateful/inv_082_state_indexed_liveness_theorem.rs`. Ten public prefixes across two configurations record only strict lexicographic rank-decreasing crank edges and require every observed actionable rank class to reach zero. The four-route flat-negative regression proves the final owner reduction preserves the close ledger, one permissionless `AdvanceClose` strictly decreases the residual and creates a real pending obligation, and the account cannot force market-wide Recovery. Its 24 terminal worlds then require every complete `CloseResolved`/top-up sweep to mutate toward a byte-and-value fixed point or expose a funded lock; every actor dematerializes, every retry is quiescent, and `CloseSlab` empties the SPL vault with order-invariant payouts. The rank also counts the complete ResetPending episode through finalization. A separate Recovery-only stale certificate remains a cross-owner counterexample: neither empty nor apparently complete hints can dispatch a successful crank, while owner reduction remains live. |
| INV-072 | SVM/CU + M + Partial R | `cu/inv_072_order_robust_crankability.rs` (exhaustive 40-word three-asset hint alphabet through length three, malformed tails, valid-hint normalization, selected-mark observation requirements, and public expired-close recovery after adversarial hints) |
| INV-073 | Independent + F + SVM/CU + Partial R | `stateful/inv_065_reset_recovery_and_retired_state_isolation.rs`, `stateful/inv_066_resolved_payout_fairness_and_order_independence.rs`, `stateful/inv_071_crank_progress.rs`, `cu/inv_073_no_permanent_user_lock.rs`, and the shared-expiry lifecycle in `cu/inv_028_source_domain_realizability_cap.rs` cover generated Recovery exits, all basic resolved claimant orders, a publicly booked bankruptcy obligation across 24 terminal schedules, and the corrected provider-expiry/prospective-loss composition through terminal disposition. |
| INV-074 | Independent + SVM/CU | `cu/inv_074_scope_locality.rs` (asset-local stale/bankruptcy isolation plus public asset-close locality for unrelated withdrawals and existing-position exits; new unrelated risk admission may still be conservatively blocked during active close) |
| INV-075 | SVM/CU + Partial R + Spec/implementation divergence | `cu/inv_075_close_priority_ownership_and_episode_integrity.rs` (owner/episode/replay checks plus both landing orders of two public equal-domain close starts through exact rejection, permissionless expiry/finalization, and rejected-contender exit; the engine implements first-landed exclusive domain ownership rather than the charter's strict preemption total order) |
| INV-076 | SVM/CU + Model gap | `cu/inv_076_close_drift_residual_durability_and_finalization_atomicity.rs` (public stale-cure and public-created zero-cure atomic rollback with terminal-progress checks; exhaustive close-phase fault injection remains) |
| INV-077 | Independent + SVM/CU | `cu/inv_077_bounded_work_and_maximum_shape_compute.rs` (supported 14-leg/28-source routes remain bounded; a production-derived registry maps all 50 tags to measured CU evidence; unreserved over-budget source-domain risk rejects atomically before CU exhaustion) |
| INV-078 | SVM/CU + Partial R | `stateful/inv_078_permissionless_recovery_coverage.rs`, `stateful/inv_071_crank_progress.rs`, and `cu/inv_078_permissionless_recovery_coverage.rs` cover all four absent/expired-backing by absent/tiny-insurance cells plus a distinct live-market bankruptcy whose permissionless residual booking produces a real pending obligation and whose stale-market continuation reaches terminal payout in every tested route/order schedule. Lien impairment, payout conflict, and the complete lifecycle failure set remain. |
| INV-079 | Direct + Static rosters + Partial R | `public_sbf/inv_079_public_reachability_evidence.rs`, `public_sbf/inv_007_no_aba_reuse.rs` enforce the finding manifest and production/method rosters, mutation-test the public trace recorder, and replay all 11 whole-market ABA request classes with actual transaction signers, compiled account metas, exact token/lamport deltas, rejected-call rollback with the network fee classified separately, and zero out-of-band economic mutation; the remaining qualifying PoCs are not yet trace-normalized |
| INV-080 | P + SVM/CU | `kani/inv_080_error_propagation_and_exact_rollback.rs`, `cu/inv_080_error_propagation_and_exact_rollback.rs` prove every current engine error variant maps to a nonzero instruction `ProgramError` and cover partial oracle, legacy realloc, terminal top-up, token CPI, and over-withdraw engine-error rollback paths |
| INV-081 | F + Direct + SVM/CU | `public_sbf/inv_081_success_state_validity_over_complete_public_routes.rs`, `stateful/inv_066_resolved_payout_fairness_and_order_independence.rs`, `stateful/inv_071_crank_progress.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, and `cu/inv_081_success_state_validity_over_complete_public_routes.rs`. The shared generated alphabet now includes the original fifteen direct classes plus market resolution, resolved-mode permissionless crank, resolved close, and resolved payout claim. Terminal calls use strict decoded-leg, position/effective-OI, receipt, exact payout, account-frame, and rollback oracles and a bounded nonterminal-fixed-point detector. Separate bounded owners compose the basic terminal lifecycle across all 5! claimant orders and the bankruptcy/pending-obligation lifecycle across 24 trade/claimant/payout-route schedules. |
| INV-082 | F route witness + SVM/CU + Partial R + Model/proof gap | `stateful/inv_082_state_indexed_liveness_theorem.rs`, `cu/inv_082_state_indexed_liveness_theorem.rs` require fixed public sequences to expose rank-decreasing permissionless progress, normal exits, liquidation progress, retained no-CPI execution under the state oracle, exact rollback of account substitutions, bad-hint noise, and no known-blocker quarantine. The bounded public graph requires every observed account-actionable rank class to reach zero through strict lexicographic edges, including ResetPending final-leg-clear -> finalizable -> finalized while excluding another user's old leg from an unrelated empty portfolio's rank. A public Recovery-only regression separates the engine's non-dispatchable crank state from the still-live owner exit; unobserved lifecycle classes and the complete reachable state space remain proof/model work. |
| INV-083 | SVM/CU + Machine roster | `cu/inv_083_boundary_completeness.rs` enforces named owners for zero, one, max-1, max, expiry edges, cross-zero, empty/full, and near-overflow classes; field-complete mapping remains open |
| INV-084 | P + Assumption inventory + Partial R + Proof-harness gap | `kani/inv_084_proof_assumptions_are_reachable_and_nonvacuous.rs` statically binds all eight current `kani::assume` sites to their exact source predicates and owning proofs, then exhausts each finite full-width admitted/excluded partition with boundary mutation killers; public-route establishment and implicit non-`assume` preconditions remain open |
| INV-085 | P + SVM/CU arithmetic differential + Proof gap | `kani/inv_085_proven_arithmetic_equals_deployed_arithmetic.rs`, `cu/inv_085_proven_arithmetic_equals_deployed_arithmetic.rs` cover deployed price-move and dt-clamp helpers against widened independent references with bounded symbolic Kani proofs plus deployed movement, funding, fee-supported mark clamping, and dynamic externality-fee boundary oracles; full deployed wide arithmetic versus bigint/Kani/BPF equivalence remains |
| INV-086 | Direct + F + Partial M + Partial R | `public_sbf/inv_086_reference_model_and_deployed_transition_equivalence.rs`, `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs` (the shared runner exercises nineteen direct public classes, including terminal resolution and payout routes, and independently checks terminal position/OI/receipt/custody edges. Its smaller bounded reference graph still exhausts all 133 words through depth two over eleven value, trade, crank, policy, authority, backing, and lifecycle actions; every graph edge runs exact custody/account frames and independent position/effective-OI/source-credit/stock oracles against one production SBF hash. Identity, lien, payout-ledger/receipt graph state, authority epochs, deeper sequences, and complete lifecycle models remain) |
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
   generates nineteen direct operation classes: trade, EWMA configuration, mark push, crank,
   deposit, withdraw, maintenance sync, matcher configuration, insurance top-up, backing top-up,
   released-PnL conversion, rebalance reduction, permissionless-resolve policy, asset shutdown,
   oracle-authority rotation, market resolution, resolved crank, resolved close, and resolved
   claim, plus replay/substitution meta-actions. Shared success/rollback, token/account frame,
   ghost-position, and global-state oracles apply to those routes; terminal routes additionally get
   exact receipt/payout/OI reconciliation. INV-081 still does not cover the complete 50-variant
   public transition system.
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
| AUDIT-008 | OPEN-D | The public retained-operation matrix now covers eleven families. Ten still reproduce duplicate economic execution; `RebalanceReduce` consumes a position epoch and rejects the retained same-epoch variant exactly. The new paired-world `ConvertReleasedPnl` owner proves issue 387 redirects later-epoch withdrawable value to a permissionless maintenance cranker. INV-013 separately closes delayed empty-state portfolio consent. A general program-enforced owner-intent ledger/expiry is still absent, and same-transaction, cross-entrypoint, and partial-failure exact-once tests remain. |
| AUDIT-009 | OPEN-T | One CPI short-fill rejection/retry is covered. Successful partial fills, random partitions, cumulative quantity/fee/slippage/expiry budgets, route switching, and one-minimum-fee-per-intent accounting are absent. |
| AUDIT-010 | OPEN-T | All 3! landing orders of two same-sequence matcher controls and one retained CPI trade are now exhausted with state-derived consent, exact rollback, and a matcher-independent exit. Add bounded permutations of deposit, withdraw, reduction, authority rotation, policy update, resolve, and claim with signed postcondition checks. |
| AUDIT-011 | OPEN-D | Per-leg prices and atomic batch rejection exist, but the message has no aggregate fee, quantity, slippage, deadline, final-position, or collateral/PnL-credit budget. Add those fields before split-intent proofs can close. |
| AUDIT-012 | OPEN-T | Matcher tuple and delegate checks are strong, but capability domain, authority epoch, expiry, allowed assets/operations, limits, and complete generation binding are not one-field-substitution tested or formally composed. |
| AUDIT-013 | OPEN-T | Close consent now binds the exact portfolio ID, shared owner-state sequence, and position epoch; deterministic public funding telemetry, generated deposit/withdraw ABA, failed-deposit rollback, fresh-close liveness, and exact Kani tuple/sequence contracts cover issue 402. Shutdown, resolve, liquidation delegation, recovery conversion, claim, receipt, and every later lifecycle episode still need the same retained-consent matrix. |
| AUDIT-014 | OPEN-D | Same-incarnation sequence supersession is covered, but market recreation, authority ABA, and backing-provider fee consent remain live gaps. Bind policy consent to all relevant incarnations/epochs, then compare stricter and looser delayed policies metamorphically. |
| AUDIT-015 | OPEN-T | Owner, short length, magic, kind, and type-confusion cases exist. Version, padding, invalid enum, alignment, maximum length, every account class, and a proof that validation precedes every zero-copy view remain. |
| AUDIT-016 | OPEN-T | Tests reject wrong vault and matcher PDAs, but do not systematically test noncanonical bumps, reordered/omitted seeds, omitted generation fields, role-crossing, or valid PDAs from another market. |
| AUDIT-017 | OPEN-T | All four trade routes exhaust ten direct or 21 CPI/matcher core-account pairs, while deposit and withdraw exhaust all 15 and 21 custody pairs; every required signer/writable downgrade starts from a successful public control and hostile cases reject with exact economic, SPL, and matcher rollback. Ledger, helper, reward, close, optional-tail, and remaining instruction schemas still need the same generated all-pairs matrix, with explicit successful controls for intentionally safe aliases. |
| AUDIT-018 | OPEN-T | SPL custody substitutions are extensive. Token-2022, fee-on-transfer/transfer-hook behavior, primary quote-decimal validation, and one independent actual-SPL-delta versus internal-accounting oracle across every value route remain. |
| AUDIT-019 | OPEN-T | Matcher return fields, stale data, req_id, tails, and local validation are covered. Add benign unrelated CPI before/after matcher return data, replace the injected matcher-context ABA setup with public close/recreate, and document that oracle paths are account reads rather than CPI. |
| AUDIT-020 | OPEN-T | Oracle provenance and authenticated clock tests are broad, but stored-slot rewind and expiry `-1/0/+1` are not crossed with every oracle mode and public consumer. No wrapper proof or complete clock/observation matrix exists. |
| AUDIT-021 | OPEN-T | Init growth, close rent, funded-close rejection, and reuse are covered. Residual claim/lien/recovery classes need a close/recreate matrix; impossible shrink and caller-selected close-destination cases should be proven N/A from the API. |
| AUDIT-022 | FRONTIER | Split Kani and exhaustive host/SVM decoder rosters backstop several solver cliffs. A deterministic 4,096-payload host corpus checks totality/canonicality, a canonical corpus locks all 50 tags, curated prior schemas plus vector-length boundaries reject, and a deployed-SBF matrix flips every bit at the tag and three boundary-sensitive payload positions for all schemas while requiring canonical decode-or-reject behavior and exact rollback. Duplicate-field N/A documentation, deeper interior-byte mutation, and per-tag proof decomposition for the remaining solver-cliff payloads remain. |
| AUDIT-023 | PARTIAL | A production-source-bound roster now owns every scalar/container field in all 50 instruction variants and three nested public input structs, enforces semantic classes, and requires a live evidence function for every row; late malformed crank hints prove exact rollback. Dynamic one-field boundary mutation, a complete account-role roster, and systematic alternate-entrypoint substitution remain. |
| AUDIT-024 | PARTIAL | Aggregate conservation is now supplemented by a 32-world public route-pair matrix that proves exact realized-PnL ownership through settlement, route-switched close, conversion, and SPL withdrawal for both sides. The stateful runner still needs a general per-transition `TokenValueFlow` owner/domain ledger for every value-bearing action; exact rejected-route snapshots already exist. |
| AUDIT-025 | PARTIAL | Every generated public step now runs an independent portfolio/domain census against both decoded state and the raw zero-copy header, exact SPL custody, and a nonnegative explicit-stock partition; a dedicated public lifecycle crosses insurance, backing, realized PnL, route-switched close, conversion, backing withdrawal, and terminal user withdrawals. Recovery and resolved-payout campaigns still need to call the same census directly, and rounding residue/protocol surplus cannot be independently recomputed until the deployed state exposes persisted stock-class ledgers instead of only a derived junior residual. |
| AUDIT-026 | PARTIAL | A common independent census now checks account-local face/backing classification and every market bucket/reservation equality after each generated public step. An eight-world public matrix requires nonzero counterparty-lien creation and exact terminal release/consumption for every trade family and source side, including repeated out-of-order close rounds. Add the public expiry/impairment/recovery cross-product, retry/double-use transitions, pending obligations, and close reserves. Insurance-backed lien lifecycle remains an explicit wrapper-API gap. |
| AUDIT-027 | OPEN-T | The four-route public stale-cohort trace now supplies a nonvacuous owner-level counterexample: a historical winner's profit equals fresh-entrant principal loss plus original-loser loss even though all positions terminate and SPL supply is conserved. Selected CU paths remain, but current fully backed, half-backed, certificate-stale, loss-stale, pending-close, resolved-payout, and insurance-withdrawal positive/control rows still need a normalized route-by-state seniority matrix. |
| AUDIT-028 | OPEN-T | Source reversal, expiry, rounding, sparse capacity, omitted backing, and the reciprocal `A-backs-B-backs-A` control now have public-route owners. The cyclic matrix proves recertification nets the adverse leg before credit use and backing without a claim remains unusable across all trade families and both close orders. Insurance impairment and a generalized bounded multi-domain transition proof against an independent per-domain consume/burn model remain; PR267 is the retained counterexample showing why post-state aggregate balance alone is insufficient. |
| AUDIT-029 | OPEN-T | The exact public claim census now exhausts 16 lifecycle cells over min/max positions, odd/even partial-burn edges, and both claimant orders. Interior price moves, favorable funding bounds, rebucketing, stale uncertainty, exact-receipt replacement, and the complete production state graph remain. |
| AUDIT-030 | OPEN-T | The independent rate oracle covers claim/add/expiry/reduce/refill. Add impairment, omitted/malformed state, every source-credit mutation route, and a proof that only fresh backing or a valid claim-bound decrease can improve rate. |
| AUDIT-031 | OPEN-T | Shared credit and insurance rails are tested, while vulnerable-pin double-spend traces remain. Duplicate lien creation, cross-domain reservation, partial retry, and concurrent route use need one atom-ownership lifecycle oracle. |
| AUDIT-032 | OPEN-T | One force-close route checks lien sums. Differentially recompute bucket and domain aggregates across create, consume, release, impair, recover, and every injected failure point. |
| AUDIT-033 | OPEN-D | The wrapper exposes counterparty-backed liens but no direct insurance-backed lien creation/consume route. Add the API or rely on a named engine contract, then prove consume/release/impair/recovery classification is disjoint. |
| AUDIT-034 | OPEN-T | Cross-market/domain substitutions are broad but manually selected and often malformed through account injection. Generate every public instruction/account-domain substitution and require public controls plus normalized rollback. |
| AUDIT-035 | FRONTIER | Domain-local B settlement has fixed and generated evidence. A public 32-cell matrix now exhausts four trade routes, both loss-asset identities, both close orders, and both position directions for the bounded two-asset ambiguous-deficit topology, with exact terminal payout and SPL conservation. A pure whole-transition proof that residuals cannot touch unrelated `(asset, side)` domains and larger multi-asset topologies remain. |
| AUDIT-036 | OPEN-T | Major fee routes are covered, but the parasitic zero-activity asset, every policy epoch, and all single/batch/CPI/no-CPI fee-destination pairs are not one complete matrix; no whole-route fee-flow proof exists. |
| AUDIT-037 | OPEN-D | Current state does not expose every term in the normative residual partition, and tests cover selected liquidation counters. Add explicit drift/obligation/lien categories, then recompute the disjoint equality after continuation, preemption, cancel, recovery, and finalize. |
| AUDIT-038 | OPEN-D | Dust, funding, backing splits, and composite rounding are tested, but a fractional-cap violation remains quarantined. Add exact rational/residue accounting for resolved claims, B booking, and social-loss clearing and fix the live counterexample. |
| AUDIT-039 | OPEN-D | Many accrual-before-weight-removal routes are covered, while stale-cohort novation remains an expected cross-cutting violation whose terminal principal-attribution owner is INV-027. Transfer, reset, account close, and partial liquidation also need the common obligation-before-removal state machine. |
| AUDIT-040 | OPEN-T | Four underfunded trade routes and maintenance spam are covered. Matcher, liquidation, protocol, and maintenance fee variants with remaining senior obligations need the same protected-pool delta oracle. |
| AUDIT-041 | OPEN-T | A public scarce-backing topology now exhausts both equal-priority pair orders crossed with one-shot/dust force-close schedules and compares per-user claims plus domain classifications; observation order is also covered. Extend the model to liquidation, insurance, lien, residual, payout, claim, and close-preemption ordering. |
| AUDIT-042 | OPEN-D | Force-close admission/timing/size is tested, but no normative fallback price/value-transfer envelope exists. Define it, then test stale/unavailable reference, max positions/accounts, and just-inside/outside bounds. |
| AUDIT-043 | N/A | The wrapper exposes no hedge/correlation-credit feature. Keep a static absence check; if introduced, require exhaustive small portfolios, sign flips, missing legs, bucket edges, and scenario extremes before activation. |
| AUDIT-044 | OPEN-T | Selected B and parked-PnL cases plus cross-owner stock tests exist. Exercise every A/K/F/B index, certificate, claim bound, reservation, lien, tag, and soft-credit durable-use path through public transitions with token/encumbrance balance checks. |
| AUDIT-045 | OPEN-D | Clamp helpers and many route/boundary regressions are strong, but multiple public/stateful mark-movement exploit adapters still reproduce. Fix those violations, then use one accepted-mark-update generator over all modes/routes and raw `0/1/MAX`. |
| AUDIT-046 | OPEN-T | A public 12-cell model covers no-CPI single/batch exits at raw `0/1/MAX` across Active, DrainOnly, and Recovery, including exact rollback after zero and authenticated-mark/value preservation on successful boundaries; active off-mark CPI single/batch reductions have separate owners. Cross zero/stale/out-of-band matcher prices with reset and resolved-close modes, then establish the canonical reducing route for each remaining state. |
| AUDIT-047 | OPEN-T | INV-024 already exhausts all 32 four-route open/close/winner-side combinations with exact owner-level outcomes, and INV-047 separately covers identical-snapshot no-CPI single/batch fee equivalence. Direct/composite equivalence, route-specific fee normalization across CPI/no-CPI, and wrapper/engine equivalence still need explicit metamorphic owners. |
| AUDIT-048 | OPEN-T | All four fresh trade routes scan raw OI, and the stateful model keeps an exact independent effective-OI transition ledger across matched/retained trades, crank liquidation, owner rebalance, prior-reset cleanup, and recovery forfeit. A retained public ADL/rebalance trace prevents regression to the invalid assumption that raw basis equals pooled OI. A separate 24-world public bankruptcy matrix now pins the zero-OI pending-obligation boundary: the final matched reduction clears effective OI while retaining exactly one zero-basis stored leg and one obligation, and terminal payout clears both without resurrecting OI. Directed nonzero-ADL resolved-close/recovery schedules and larger multi-account ADL topologies remain. |
| AUDIT-049 | OPEN-T | All trade routes preserve one net leg, but transfer, reset, recovery, reactivation, and deserialization attachment attempts are absent. Add public transition matrices and malformed-deserialization negatives only where deserialization is an ingress. |
| AUDIT-050 | OPEN-D | A public all-four-route matrix now creates partial liquidation/ADL, crosses zero, and proves unrelated auxiliary OI alone changes admission and erases the exact haircut from fresh-leg effective-OI attribution. This independently reaches the known PR250/engine-134 basis-reissue root through the Flip branch. Land the engine fix, invert the vulnerable cells to exact rollback, then add cross-zero boundaries around zero/effective/raw/max quantity, pending-obligation epochs, and lifecycle modes. |
| AUDIT-051 | FRONTIER | Zero-effective-OI directed matrices and the stateful transition ledger cover resize, matched trade, rebalance, liquidation, reset clear, and recovery forfeit without collapsing raw basis into effective OI. The bankruptcy matrix now carries zero effective OI through a nonzero pending-obligation epoch and terminal close. Transfer, nonzero-ADL resolved close, retirement, and a pure whole-transition equivalence proof remain. |
| AUDIT-052 | OPEN-T | Deterministic trade and withdrawal partitions exist. Add arbitrary partition/permutation fuzz for liquidation, reduction, lien consumption, insurance withdrawal, claims, cooldowns, rates, and policy limits. |
| AUDIT-053 | FRONTIER | Omitted-leg liquidation findings and route/order fuzz are now joined by public stale-refresh regressions for a pending later Live mark behind either a current Live leg or a Recovery leg. These found and fixed a wrapper branch that checked only the first selected leg before whole-account certification; stale refresh and liquidation now scan every active leg, with a measured 159,748-CU 14-leg refresh. No full-certificate oracle runs after every transition, and pending obligations, impaired liens, ADL, and all penalty lanes are not composed. Prove or differentially establish fast <= full. |
| AUDIT-054 | OPEN-T | Public favorable-action tests now isolate all four deployed global certificate keys: target/effective oracle movement, nonzero `F` movement with fixed `oracle_epoch`, source-credit backing with only `risk_epoch`, and asset append with `asset_set_epoch` plus risk. Every stale conversion rejects with exact account/market/vault rollback, and public crank restores all keys before exact conversion. Account bitmap is checked after every fixture transition, but a deliberately stale bitmap cannot be produced by a successful public route because leg mutations recertify atomically. Remaining work is to classify every health-relevant writer into these four epochs and add directed lien, pending-obligation, lifecycle/reset, and close-state cases; policy changes that do not affect health should not be invented as certificate keys. |
| AUDIT-055 | OPEN-T | A public declarative matrix now covers all 20 combinations of open, bilateral reduce, deposit, withdraw, and resolved payout with Active, DrainOnly, Recovery, and Resolved. Every allowed cell must produce its exact economic delta and every forbidden cell must roll back all tracked bytes, SPL data, and lamports. Reset-side, close-ledger, retirement/reactivation, and the remaining public instruction classes still prevent a complete 50-instruction state cross-product. |
| AUDIT-056 | OPEN-T | Batch stale-related-leg and crank-hint cases exist. Omit/reorder/duplicate the worst and benign legs across withdraw, liquidation, conversion, claim, and all trade routes, comparing each result with canonical full discovery. |
| AUDIT-057 | FRONTIER | The generator now reaches a real funded Recovery state by public policy configuration and asset shutdown and requires all modeled positions to exit. It still does not establish an exit from every reachable lifecycle state; add a bounded public-only state search whose oracle finds a reducing action or terminal receipt. |
| AUDIT-058 | OPEN-T | TVL, large amount, over-reduce, top-up, and batch cap boundaries are covered. Generate every hard OI/notional/rate bound with zero/one/max/near-max, splitting, batching, cross-zero, route, transfer, and recreate variants. |
| AUDIT-059 | OPEN-T | One minimum-liquidation-fee case is covered; stale pre-CPI tests do not prove fragmentation. Compare aggregate close with one-atom closes, retries, mixed routes, and public partial failures under an episode-level fee oracle. |
| AUDIT-060 | FRONTIER | Public IM/MM and lag gates exist, but there is no independent decomposition of pending obligations, impaired liens, reserves, oracle lag, and penalties. Build a lane model and prove each component appears exactly once. |
| AUDIT-061 | OPEN-D | Liquidation safety, fees, progress, and selected generated schedules are covered, but public/stateful ADL close-order tests still assert violations. Fix those states, then add equal-risk permutations, arbitrary close splitting, normalized loss attribution, and max-shape liquidation coverage. |
| AUDIT-062 | OPEN-T | Selected self-trades show no identity privilege. Repeat common-control counterparties over every route/oracle mode and prove no unbacked value, mark-cost bypass, fee reclaim, or attribution change with a shared reference ledger. |
| AUDIT-063 | OPEN-T | Trade consumption has a nonvacuous 4-route x `expiry-1`/`expiry`/`expiry+1` public matrix: every fresh control grows a real lien, fee-capable routes charge real fees, and both expired boundaries roll back while preserving reduction. Released-PnL conversion has the same three-slot matrix with exact backing consumption and real SPL withdrawal. Retained top-up now has all three boundaries with exact provider/custody/accounting deltas; its fresh cell reproduces PR291 on the preceding pin and proves bounded terminal settlement on the fixed pin, replacing the narrower one-off regression. Release, claim, payout, and retire still lack one complete consumer-by-boundary matrix; no proof establishes normalization before every consumer. |
| AUDIT-064 | OPEN-D | Live and terminal insurance routes are tested, but the normative shared enable flag, cap, cooldown, policy epoch, and last-withdraw ledger are partly absent/dead controls. Specify or remove them, then interleave every route against one allowance ledger. |
| AUDIT-065 | OPEN-T | A generated public policy-to-shutdown route now reaches Recovery and retains all-portfolio exits under shared invariants, while selected reset/recovery gates remain. Several fixtures still use `mutate_market`; add public begin/finalize interleavings, every trade route, side isolation, retirement/reactivation, stale-generation attempts, and a bounded admission model using public setup only. |
| AUDIT-066 | OPEN-T | A public two-asset lifecycle now exhausts all 5! basic claimant orders with exact payout/vault reconciliation and identical outcomes. Extend that bounded model with authority refinement, partial top-ups, exact-bound replacement, recovery transitions, and a rational residue oracle. |
| AUDIT-067 | OPEN-D | Both payout routes are retried at a byte- and token-stable fixed point after every claimant across all 5! basic orders. A second matrix reaches terminal settlement from a publicly booked bankruptcy obligation and proves exact per-owner payouts across all four trade routes, three claimant orders, and both payout-route priorities. Public/stateful terminal-dust tests still assert a reachable payout-erasure violation; fix it, then model a genuinely partial top-up, close/recreate, forfeit, and recovery conversion over every claim episode. |
| AUDIT-068 | OPEN-T | Replay and payout-rail tests exist. Add one-field receipt substitution for market/domain, portfolio incarnation, claim episode, face, snapshot, receipt ID, cross-portfolio, and asset-slot reuse, plus monotonic split top-ups. |
| AUDIT-069 | OPEN-T | A public bounded model now exhausts all four funded-insurance/funded-backing blocker states and both drain orders with exact rollback before terminal retirement. Spent/provider-receivable setups are still injected; recreate them publicly, then add reset history, price-only indices, expired labels, old epochs, pending loss/receipt controls, and their cross-product. |
| AUDIT-070 | OPEN-T | A complete public two-asset lifecycle now resolves, pays and dematerializes all five funded portfolios, proves zero accounting, and reaches `CloseSlab` across all 5! claimant orders while a foreign market remains byte-identical. Extend it with rounding, recovery, prior insurance, independent stock classification, and surplus sweep. |
| AUDIT-071 | OPEN-D | A ten-prefix/two-configuration public graph records only strict lexicographic rank-decreasing crank edges, covers multiple rank components, and requires every observed actionable class to reach zero. A generated public sequence exposed and fixed a model bug where clearing the final prior-epoch ResetPending leg appeared to increase rank; the rank now counts every exact reset prerequisite through finalization. The directed bankruptcy matrix adds one real `AdvanceClose` residual decrease and then requires each complete terminal-route sweep either to mutate toward a byte/value fixed point or expose a funded lock, across 24 public schedules. Other owned tests still assert a Recovery classifier/dispatch mismatch while preserving owner reduction. Fix that mismatch, then extend the graph to every crank class, lifecycle mode, close/recovery state, and maximum shape. |
| AUDIT-072 | OPEN-T | A public three-asset matrix now exhausts all 40 hint words through length three, including every bounded subset, ordering, and duplicate placement, plus selected out-of-range, malformed/absent oracle, and unclaimed account tails. Every case rejects atomically or lowers rank before an honest completion to rank zero. Extend that equivalence over every account-actionable crank class and the complete stale external-oracle tail space. |
| AUDIT-073 | OPEN-D | The stateful campaigns exit the designated liquidity provider after unilateral reduction, every modeled portfolio after public asset shutdown, every basic resolved claimant across all 5! orders, and every actor after a publicly booked bankruptcy obligation across 24 terminal schedules. Multiple owned tests still assert other publicly reachable funded locks. Fix those locks, then build a small public state graph plus long sequences requiring every funded nonterminal node to reach principal return, a receipt, or authorized junior forfeit. |
| AUDIT-074 | OPEN-D | Unrelated base trading/withdrawal cases exist, but an owned public test still asserts an unrelated backed claim is blocked by asset-local bankruptcy. Fix or normatively justify that lock, then complete side, portfolio, domain, close, and receipt locality. |
| AUDIT-075 | FRONTIER | Both landing orders of two public equal-domain close starts now prove first-landed exclusion, exact rejected-contender rollback, immutable accepted identity, permissionless expiry/finalization after configured delays, and terminal exit of the rejected contender without the first owner's signature. This also demonstrates a normative mismatch: the public API and engine expose no strict `ClosePriority` tuple or preemption order. Decide whether exclusion is the specification; otherwise add priority/preemption semantics, then model restart, stale continuation, cure/cancel, owner deposit, and no-double-booking. |
| AUDIT-076 | OPEN-T | Only stale-cure and zero-cure rollback are owned. Add table-driven public fault injection at every close phase, price/funding drift, preemption/restart, durable residual booking, complete snapshots, and atomic OI/basis-clear checks. |
| AUDIT-077 | OPEN-T | The production-derived registry now maps all 50 instruction tags to named public-route and measured CU evidence with zero omissions; this tranche added explicit `InitMarket`, enabled/disabled `SetMatcherConfig`, and 5,834-slot `UpdateAssetLifecycle` measurements and indexed nine existing bounds. Complete the remaining maximum-dimension cross-product and activation-time rejection of unsupported shapes. |
| AUDIT-078 | OPEN-T | A four-state public model crosses absent/expired backing with absent/tiny insurance after creating the same bankrupt exposure. Every cell reaches owner-callable terminal exits with zero expired-backing support, exact insurance spend, and exact residual B booking. A separate public live-market bankruptcy matrix proves one permissionless residual booking creates a real pending obligation that the stale-market continuation later drains to terminal fixed point. INV-075 covers domain-close exclusion and eventual release. Add lien impairment, true B-exhaustion/booking failure, payout conflict, oracle-unavailable terminal fallback, and the remaining lifecycle failure classes, then compose them into bounded recovery reachability. |
| AUDIT-079 | OPEN-T | An opt-in LiteSVM trace schema now records actual transaction signers, compiled account metas, exact tracked token/lamport deltas, rejected writable-account rollback with the fee-payer network charge separated from program effects, and between-transaction economic mutation. Its detector is mutation-tested, and all 11 whole-market ABA cells require zero out-of-band mutation. Attach the schema to every remaining qualifying PoC and add a normalized terminal classification for exact loss, unauthorized withdrawable gain, bounded exit, or persistent funded lock. |
| AUDIT-080 | OPEN-T | Engine-error mapping and many SPL/realloc rollback paths are covered; the shared stateful rejection snapshot now includes every modeled economic account's lamports as well as program bytes and SPL data. Fault-inject every wrapper fallible stage outside that generated model and test a later instruction in the same transaction cannot consume success-only output. |
| AUDIT-081 | FRONTIER | The shared stateful model now covers nineteen direct operation classes, including `ResolveMarket`, resolved-mode `PermissionlessCrank`, `CloseResolved`, and `ClaimResolvedPayoutTopup`. It switches progress/exit campaigns into bounded terminal sweeps, applies strict leg and independent position/effective-OI/receipt/payout/account-frame oracles on success, and exact program-byte/SPL/lamport snapshots on rejection. Separate bounded owners still supply all 5! basic claimant orders and 24 bankruptcy/pending-obligation terminal schedules. Authority epochs/ABA, retirement/reactivation, genuinely partial receipts in the shared generator, complex payout state, and the other public variants remain; the shared runner still does not assert every one of the 89 charter invariants after every success. |
| AUDIT-082 | FRONTIER | The first bounded public transition graph now composes ten public prefixes across two configurations with the deployed mode-aware rank, records only strict lexicographic crank reductions, and proves every observed actionable rank class has a path to zero. The reference model distinguishes Active/DrainOnly auto-crank work from Recovery owner-exit work and preserves the classifier/dispatch contradiction as a deterministic public regression. Expand the graph alphabet and state dimensions to all lifecycle, close, B, receipt, oracle-failure, and recovery classes; then connect each abstract node to a public-route reachability witness or a proven unreachability argument. |
| AUDIT-083 | OPEN-T | A machine-readable roster now requires actual invariant-owned tests for zero, one, max-1, max, expiry-1/equal/+1, cross-zero, empty/full, and near-overflow classes. It is class-level rather than field-complete; map every arithmetic/lifecycle field to the roster and add full-width and excluded-state reachability proofs. |
| AUDIT-084 | FRONTIER | A compile-time inventory classifies all eight current `kani::assume` sites across nine mounted Kani modules and binds each row to the exact source predicate and owning proof. A full-width symbolic partition proves admitted and excluded models and pins off-by-one, widening, and dropped-mark-clause mutation killers. Public-route establishment or named unreachability remains for each admitted domain, and implicit branch/fixture proof preconditions are not yet inventoried. |
| AUDIT-085 | FRONTIER | Selected price/funding/fee helpers match widened references on bounded domains. Full carry/borrow/multiply/divide/scale equivalence among Kani, host, BPF, and bigint remains; split by primitive and use differential full-boundary corpora where CBMC cliffs. |
| AUDIT-086 | OPEN-T | The shared runner now checks nineteen generated public classes and includes deployed terminal resolution and payout edges. The separate bounded reference graph still exhausts only 133 words through depth two over eleven value, trade, crank, matcher, backing, authority, resolve-policy, and lifecycle actions. It binds one production SBF hash and runs exact custody/account frames plus independent position/effective-OI/source-credit/stock oracles after every edge, but its normalized node omits payout-ledger, receipt, close-progress, and terminal-resolution state. Add those components and terminal actions first, then identity, all-balance, lien, authority-epoch, recovery/expiry/order dimensions, and deeper sequences without treating the finite graph as universal equivalence. |
| AUDIT-087 | OPEN-T | The static roster inventories `WrapperConfigV16` and selected policies only. Inventory every persisted security field across all account types and require one writer, enforcement read, public mutation witness, or explicit removal/N/A classification. |
| AUDIT-088 | OPEN-T | Per-asset OI/count scans cover selected trade/liquidation orderings. Recompute every market/global accumulator from all bounded portfolios/domains after every relevant public transition and compare adversarial asset/account touch orders. |
| AUDIT-089 | OPEN-T | Fresh/reuse authority and price checks are broad, but full raw-state equivalence, support weight, source ledgers, certificate invalidation, residual state, stale epochs, generation increment, and unsupported-shape cases are not one differential matrix. |

## Known-finding benchmark

`open_findings.tsv` is the unified 2026-08-03 snapshot of 143 open PRs whose titles identify a
public-route LoF or DoS class. It maps every row to a primary invariant. PR135 currently has 0
**Direct regression** rows, 0 **Missing** rows, 126 **Independent discovery** rows, and seventeen
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

On engine pin `0a23b5f5fc85ddb0223089c66c29cbf1600be62b`, the full `v16_cu` inventory is
invariant-owned and passes as an unfiltered suite. The former red PR220/PR366, PR367, live
source-backing expiry, source-domain capacity admission, and flat-negative final-leg progress
probes are fixed-pin regressions under INV-028, INV-030, INV-035, INV-053, INV-063, INV-071,
INV-074, and INV-077; the unfiltered command is the required verification command.

Use `PERCOLATOR_FUZZ_CASES`, `PERCOLATOR_FUZZ_ACTIONS`, and
`PERCOLATOR_FUZZ_SHRINK_ITERS` to raise the generated stateful budget. Kani harness names now include
their `inv_NNN_*` module path; suffix filters can still target the original proof function names.
