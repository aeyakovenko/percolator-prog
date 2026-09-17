# Mark/oracle movement and reward provenance audit, 2026-09-17

Base: fetched `origin/main`, `a4b5fad85592d764214a696dc706944884b3d33e`.
Branch: `astra-ultra/mark-movement-audit-20260917`.
Worktree: `/dev/shm/astra-ultra-mark-movement-audit-20260917`.
Scope: INV-020/045/053/056/062, rows
**225/260/264/265/280/282/331/332/333/356/365/369/422/425/426 only**.

## Outcome

All 15 named families have mounted public-route witnesses. No missing public-route
case was established, so no duplicate test was added. Execution nevertheless
finds **two existing reward witnesses failing at this base**. Mounted availability
and historical passing results do not establish current behavioral coverage.
This audit and its README entry record that distinction; TSV statuses stay intact.

Review follows `INVARIANTS.md`, discovery/reopening/status ledgers, current test
bodies and helpers, harness mounts, and the merged provenance fix. No production
or dependency change is justified by the observed failures.

## Row map

The `prNNN` names below abbreviate exact selectors in the command block.
[INV-045 fixed regressions](public_sbf/inv_045_no_free_mark_movement.rs) and their
[shared helpers](../support/fuzz_model.rs) own the first seven entries.

| Rows | Existing witness and assertion boundary |
| --- | --- |
| 225 | `pr225`: all four trade transports pay nonzero movement fees; pending and committed reserve withdrawals reject with exact rollback; terminal close burns exactly the paid fee. Attacker gain is zero and attacker loss is positive. |
| 260 | `pr260`: a paid pending EWMA move rejects a retained stale-price increase on all four transports. After commitment, fresh trade and exit succeed, victim loss is zero, and attacker principal payout is exactly 100,000,000. |
| 264/265/332/333 | Combined `pr264_pr265_pr332_pr333`: AuthMark push, EWMA push, single-trade and batch-trade publication stage the engine target/epoch before stale CPI admission. Rejection rolls back; lagging reduction and post-catch-up trade/exit remain live. These four publication cases are not every possible route/order product. |
| 280 | `pr280`: EWMA and stale Hybrid, single/batch no-CPI, produce a real liquidation and positive victim penalty/loss. Reward and budgeted penalty stay zero, retained penalty and attacker cost are positive. |
| 282 | `pr282`: all four transports reject an attempted pending-target override without target or payout drift. Attacker/victim payouts remain 24,000,000,000 / 20,000,000,000. |
| 356 | `pr356`: fee synchronization before adverse-mark commitment rejects with exact rollback. Reordered and canonical histories agree on rewards and both terminal owner payouts; this family expects zero reward. |
| 369 | `pr369`: single/batch CPI across EWMA/stale Hybrid cannot subsidize movement with one side's fees. Coalition extraction is bounded by prior equity, while fee-payer loss and insurance gain are positive. |
| 331 | [INV-020 fixed composite witness](public_sbf/inv_020_authenticated_clock_slot_and_oracle_provenance.rs): three skewed timestamp words reject atomically; coherent observations complete partial refresh and exact input-priced health, then pay the owner. Current run: three partial refreshes, 14,000 capital loss, 526,000 owner payout. |
| 365 | Delegated to [INV-038](public_sbf/inv_038_rounding_and_ratio_conservation.rs), `pr365`: a unit position moves from 100 toward target 1 under a 24-bps/20-slot cap. Repeated public cranks reach 1; no long overpayment/short underpayment, total terminal payout 2,000,000. |
| 422 | INV-045 fixed stale-trade/fresh-report regression preserves trade-origin exclusion during lag, then checks authenticated provenance after catch-up. Its independent fresh-origin control earns 500 atoms. The mounted [paid-origin Hybrid-recipient child](cu/inv_045_paid_origin_hybrid_recipient.rs) crosses all four discovery transports and both publication/liquidation orders: eight worlds, old penalties retained, zero rewards/domain budgets, exact 1,000 principal payout and separate nine-atom recipient PnL. See failing historical witnesses below. |
| 425 | Original discovery is delegated to [INV-071](cu/inv_071_crank_progress.rs), `v16_attack_risk_reducing_trade_cannot_erase_canonical_price_movement_remainder`. Canonical/interleaved histories both end at price 101, carry 2,000, owner payouts 1,000,100 / 999,900, wash-pair payout 2,000,000 and empty vault. |
| 426 | Original discovery is delegated to [INV-056](cu/inv_056_hints_are_discovery_only_favorable_actions_fully_refresh.rs), `v16_attack_liquidation_cannot_omit_fresh_external_rescue_observation`. Omitted/prior-slot rescue evidence rejects with exact market/portfolio/vault rollback, preserving 50 units of exposure. Fresh evidence restores health and the same 2,600,000 owner payout with zero insurance. |

The fixed files are direct children of `v16_program_fuzz_regressions`; CU owners
are direct children of `v16_cu`. The row-422 child mounts through
`trade_origin_catchup::authenticated_reward_handoff::retained_penalty_handoff`.
The discovery ledger also points at mounted stateful matrices for the original
rows; those matrices were reviewed, not rerun.

Additional executed joins stay within this audit's subject:
[INV-020 equal composites](cu/inv_020_equal_composite_provenance.rs) checks six
worlds, 36 provenance rollbacks, six current-health no-ops and 18 certificates
while preserving fractional carry. [INV-053 rounded nontraded lag](cu/inv_053_full_health_recertification_equivalence.rs)
checks three exact admission rejections and six full/independent certificate
comparisons. INV-056 mixed observations compares full, empty-hint and on-demand
refresh for both no-CPI transports at equity/margin 210, rejecting one extra
margin atom. [INV-062](cu/inv_062_no_identity_assumptions_self_trade_containment.rs)
compares 96 common-owner route/side/mode cases with 96 independent-owner controls,
plus four partial-liquidation worlds. These identity comparisons do not replace
INV-045's paid-movement extraction assertions.

## Current failures and limits

1. `trade_origin_catchup::authenticated_reward_handoff::v16_program_paid_discovery_fresh_handoff_authenticates_liquidation_and_keeper_exit`
   fails at [the keeper-value assertion](cu/inv_045_authenticated_reward_handoff.rs#L421):
   keeper value is **1,000**, expected **2,995** (an extra 1,995 reward atoms).
   The other four portfolio values match. The exact single-thread rerun fails
   identically. The helper requires a positive reward while the effective price
   still lags the fresh target. Merged fix `01ec6161e4174437ee306f329eea87bc28f41f41`
   introduced effective-price provenance, gates reclaimability on authenticated
   provenance, and restores it only at fresh-target catch-up. The observed zero
   reward agrees with the passing fixed and newer paid-origin witnesses; this is
   a stale test expectation, not evidence for removing the production guard.
   Downstream assertions in this failing selector were not reached.
2. `v16_program_caught_up_hybrid_reward_uses_selected_asset_provenance_in_both_hint_orders`
   fails at [the target-crank unwrap](cu/inv_045_no_free_mark_movement.rs#L302)
   with `InstructionError(2, Custom(22))` (`EngineNonProgress`), 52,873 transaction
   CU. It does not reach the claimed reward/payout result. No production cause or
   replacement coverage for that exact two-asset/hint-order suffix is established.

The INV-045 source-composition guard passes despite these behavioral failures:
it checks source structure and witness availability, not their runtime results.
The mount census likewise proves availability only. Earlier README/audit passing
claims describe earlier bases and cannot be carried forward for these selectors.

At this base, INV-020/062 are `OPEN_EVIDENCE`, INV-045 is `REFUTED_CURRENT`,
and INV-053/056 are `SUPPORTED`; all retain `SAMPLED / GLOBAL_CONDITIONAL_TCB`.
Row 422 remains `OPEN`; rows 425/426 remain `COVERED`. No status promotion is
made. Finite route families do not establish arbitrary histories, the complete
funding/maintenance/policy/recipient product, maximum-shape composition, or a
whole-invariant theorem. Tests execute deployed wrapper routes under LiteSVM;
Clock, provider-owned reports and matcher programs remain fixtures, not executed
provider publication protocols. Engine pin:
`94979ede7db934545e53a8f210dd063a9ea3ea63`.

## Exact verification

A private host-target copy isolates build products. Reused wrapper SBF SHA-256:
`87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.
Artifact source base `cb236b5248c941ffcb33e1e6afe62801e8a19712` has identical
`src/`, `Cargo.lock` and auth-matcher sources; the manifest delta only enables
host `syn` parser features. Auth-matcher SHA-256:
`50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
INV-062 also uses the pre-existing external matcher at
`/dev/shm/percolator-match/target/deploy/percolator_match.so`, SHA-256
`51f361c6fd00bdb91c685e98f081dea5a54665ef533a7f7b619916594aae6755`;
its source/build provenance was not re-established. No SBF rebuild is claimed.

Commands from the isolated worktree (logs are in
`/dev/shm/astra-ultra-mark-movement-audit-20260917-logs/`):

```bash
cp -a --reflink=auto /dev/shm/percolator-public-gap-20260916-c91e-host-target /dev/shm/astra-ultra-mark-movement-audit-20260917-host-target
mkdir -p tests/fixtures/auth_matcher/target/deploy
cp /dev/shm/percolator-main-merge-20260916/tests/fixtures/auth_matcher/target/deploy/auth_matcher.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-mark-movement-audit-20260917-host-target
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-public-gap-20260916-c91e-sbf-target/deploy/percolator_prog.so

cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture \
  inv_045_no_free_mark_movement::v16_program_pr260_pending_ewma_inheritance_rejects_then_trades_on_every_route \
  inv_045_no_free_mark_movement::v16_program_pr282_pending_ewma_target_override_rejects_without_value_drift \
  inv_045_no_free_mark_movement::v16_program_pr264_pr265_pr332_pr333_targets_stage_before_stale_cpi \
  inv_045_no_free_mark_movement::v16_program_pr356_pending_mark_fee_sync_rejects_then_preserves_terminal_value \
  inv_045_no_free_mark_movement::v16_program_pr369_one_sided_cpi_fee_cannot_subsidize_mark_gain \
  inv_045_no_free_mark_movement::v16_program_pr225_mark_movement_fee_is_nonwithdrawable_and_terminally_burned \
  inv_045_no_free_mark_movement::v16_program_pr280_trade_driven_liquidation_penalty_is_not_reclaimable \
  inv_045_no_free_mark_movement::v16_program_fresh_hybrid_oracle_liquidation_reward_remains_enabled \
  inv_045_no_free_mark_movement::v16_program_fresh_hybrid_report_does_not_reenable_stale_trade_liquidation_reward \
  inv_045_no_free_mark_movement::v16_row425_metadata_retains_fractional_carry_evidence_and_entitlement \
  inv_045_no_free_mark_movement::v16_row422_metadata_retains_effective_price_lineage_obligation \
  inv_020_authenticated_clock_slot_and_oracle_provenance::v16_program_temporally_skewed_composite_rejects_atomically_and_exit_stays_live \
  inv_020_authenticated_clock_slot_and_oracle_provenance::v16_row426_metadata_retains_current_observation_omission_evidence \
  inv_038_rounding_and_ratio_conservation::v16_program_pr365_fractional_cap_reaches_target_and_preserves_terminal_payouts
cargo test --locked --offline --test v16_cu -- --exact --nocapture \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::retained_penalty_handoff::paid_origin_hybrid_recipient::v16_program_paid_origin_penalty_survives_dual_hybrid_recipient_catchup \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::v16_program_paid_discovery_fresh_handoff_authenticates_liquidation_and_keeper_exit \
  inv_045_no_free_mark_movement::v16_program_mark_writer_and_trade_exit_composition_is_source_complete \
  inv_071_crank_progress::v16_attack_risk_reducing_trade_cannot_erase_canonical_price_movement_remainder \
  inv_020_authenticated_clock_slot_and_oracle_provenance::equal_composite_provenance::v16_program_equal_composite_refresh_preserves_component_provenance_and_mark_carry \
  inv_053_full_health_recertification_equivalence::v16_program_rounded_nontraded_lag_full_refresh_preserves_exact_trade_boundary \
  inv_056_hints_are_discovery_only_favorable_actions_fully_refresh::v16_attack_liquidation_cannot_omit_fresh_external_rescue_observation \
  inv_056_hints_are_discovery_only_favorable_actions_fully_refresh::v16_bpf_inv056_mixed_observations_preserve_full_refresh_trade_boundary \
  inv_062_no_identity_assumptions_self_trade_containment::v16_program_common_control_round_trip_is_conserved_across_routes_and_mark_modes \
  inv_062_no_identity_assumptions_self_trade_containment::v16_program_common_control_partial_liquidation_matches_independent_owners_and_routes
cargo test --locked --offline --test v16_cu inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::v16_program_paid_discovery_fresh_handoff_authenticates_liquidation_and_keeper_exit -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_045_no_free_mark_movement::v16_program_caught_up_hybrid_reward_uses_selected_asset_provenance_in_both_hint_orders -- --exact --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_every_public_invariant_source_and_declared_test_is_mounted -- --exact --nocapture
git diff --check a4b5fad85592d764214a696dc706944884b3d33e
git diff --exit-code a4b5fad85592d764214a696dc706944884b3d33e -- src Cargo.toml Cargo.lock
git diff --cached --check
```

Results: fixed regressions/metadata **14 passed, 0 failed, 133 filtered** (7.56s);
CU group **9 passed, 1 failed, 1,429 filtered** (74.35s, exit 101);
handoff rerun **0 passed, 1 failed, 1,438 filtered** (0.51s, exit 101);
selected-asset control **0 passed, 1 failed, 1,438 filtered** (0.43s, exit 101).
Mount census: **1 passed, 0 failed, 146 filtered**, **508 source files / 1,914
available tests** (3.78s). Thus 24 distinct checks pass and two distinct witnesses
fail; the repeated failure is not a third witness.

Whitespace and production/dependency diff checks pass. No Rust or TSV files
changed, so changed-test selectors and Rust/TSV formatting are not applicable.
Existing Solana future-compatibility/unused-support warnings remain. No broad
suite, Kani run, new behavioral coverage, or production fix is claimed.
