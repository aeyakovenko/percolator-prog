# Liveness and source-capacity coverage audit, 2026-09-17

Base: `origin/main` at `cd751b44347f7d3ce4c5ece349571530535b875a`.
Branch: `audit/liveness-source-capacity-20260917`.
Worktree: `/dev/shm/percolator-liveness-source-audit-20260917`.

Scope: INV-028/071/073/077, the 26 requested rows below, and all 17 rows in
`nonqualifying_findings.tsv` as controls. Following [loop.md](../../scripts/loop.md),
finding identifiers/titles are withheld comparison data, not exploit implementations
or proof of current behavior. Only repository witnesses, helpers, mounts and wrapper
code were inspected; no external PR patch or reproduction was imported.

Every named family already has a mounted witness: **32 exact selectors pass and
two existing witnesses fail**. No uncovered real public-interface
DoS was established, so this adds documentation, not a duplicate probe or engine proof.
The two failing existing expiry witnesses below do not search valid continuations.
Production, tests, dependencies and all status/coverage ledgers remain unchanged.

## Row map

C1-C30, S1-S3 and F1 identify selectors in their respective command order below.
Delegated INV-039/051/070 owners count once; successful exact execution establishes
mounting as well as behavior. Statements describe finite assertion boundaries;
C12/C25 fail before their terminal assertions and are not passing exit evidence.

| Row(s) | Witness | Nonvacuous assertion and limit |
| --- | --- | --- |
| 190 | C1 | Two funded bankruptcy worlds commit Recovery with exact accounting/OI frame, then permissionlessly resolve. Stops at Resolved, not user payout. |
| 203 | C2 | Public partial ADL leaves positive effective OI below raw basis; unsigned pair close clears both legs, advances epochs, and bounded side finalization preserves retained value. No full payout assertion. |
| 212 | C3 | Public 28-source/14-leg liquidation at first/last asset: mutating refresh, strictly reduced exposure, exact additional owner reduction and unchanged custody under 1,375,000 CU per step. Partial progress, not complete withdrawal. |
| 213 | S1 | Nonzero funded lien precedes reversal; six independent route worlds require reduced exposure, bounded successful calls and rejected-call rollback. External payout is explicitly zero. |
| 214 | C4 | A 5,000-atom lien/claim crosses exact/late expiry; normalization and reduction preserve senior exit, then retained claims pay exactly 5,000/995,000 and both portfolios delete. |
| 217 | C5 | Fractional social-loss carry precedes unilateral/bilateral effective exit; OI decreases exactly, dust clears, and positive remaining capital withdraws exactly. |
| 228 | C6 | A funded position matched against ten fragments closes through ten permissionless pairs; all legs/OI clear with custody reconciliation. No payout theorem. |
| 246 | C7 | Five eager versus one delayed micro-price crank produce the same price advance; four zero-price-delta calls advance authenticated time/carry, with unchanged vault. |
| 252/368 | C8 | Owner-authorized partial Recovery forfeit and bounded unsigned continuation reach Resolved; unrelated funded owners receive exactly 100/101 atoms and delete. Does not certify every participant's exit. |
| 266 | C9 | Crossed exact-effective ADL exit clears retained basis and leaves bounded side cleanup; owned by INV-051, sharing INV-073's public fixture. |
| 268 | C10 | Unilateral effective exit clamps retained raw work to real exposure, clears the leg and permits bounded side finalization. Also control 202. |
| 270/379 | C11 | Forward/reverse histories fill all 28 source slots. Reserved-domain risk opens/closes; an unrelated asset rejects with `Custom(9)` and exact rollback before funded admission. No full claim payout. |
| 298 | C12, failing | Public setup reaches expired backing plus prospective adverse K delta, then malformed Resolved hints prevent the intended multi-step exit check. |
| 300 | C13 | A different winner expires shared backing with a retained lien/prospective loss; valid CloseResolved/crank routes mutate state, all four portfolios terminate and impaired aggregates clear. |
| 306 | S2 | Two fractional source domains and both asset orders require a mutating bounded continuation, rollback and custody conservation. Route masks prevent an incomplete search from claiming persistent lock; no exact full payout. |
| 357 | C14 | Public 28-source funded claim rejects too-small conversion caps atomically, converts the full amount, withdraws exact capital and deletes the portfolio. |
| 359 | C15 | Fourteen legs/28 positive source claims: unsigned detach lowers rank, both claimant orders terminate with equal nonzero owner payouts under bounded CU. |
| 364 | S3 | Provider withdrawal plus real lien survives flattening; four transports require mutating release, full conversion, withdrawal and close with zero remaining value/lien. |
| 371 | C16 | Both forfeit orders retain a zero-basis loss obligation; premature deletion rolls back, peer exit and bounded keeper work release it, exact payouts and deletion follow. INV-039 owns the shared witness. |
| 376 | C17 | Nonzero asset-0 recovered principal withdraws, remaining insurance exits, admin restarts once, fresh insurance has zero inherited spend, and new traders recover capital. Named owner/provider/admin signers remain required. |
| 420/433 | C18/C30 | Twelve fresh/expired worlds cover all principal/earnings/insurance payment orders, unsigned partial payouts, lazy ledger and payout-prefix rollback. Seniority control rejects Live, outstanding capital and undeleted empty portfolios; mechanical deletion precedes reserve payments. |
| 421 | C19 | Nonzero asset-1 insurance pays its configured beneficiary unsigned; substituted authority/custody and funded takeover reject. Live unsigned rejection and exact retirement are separate checks. INV-070 owns this witness. |
| 423 | C20 | 24 histories (12 reused-generation) join 22/24/26 historical sources to six/four/two latent domains, single/batch rollback/retry, full materialization and ranked exact payouts/deletion. Source capacity is not every future exit resource. |

Primary sources: [INV-028 CU](cu/inv_028_source_domain_realizability_cap.rs),
[INV-028 stateful](stateful/inv_028_source_domain_realizability_cap.rs),
[discovery assertions](../support/invariant_discovery.rs),
[generation admission](cu/inv_028_generation_capacity_admission.rs),
[INV-071](cu/inv_071_crank_progress.rs), [INV-073](cu/inv_073_no_permanent_user_lock.rs),
[reserve helper](cu/inv_073_terminal_public_reserves.rs), and
[INV-077](cu/inv_077_bounded_work_and_maximum_shape_compute.rs).

## Nonqualifying controls

| Row(s) | Witness | Current assertion boundary |
| --- | --- | --- |
| 202 | C10 | Correct effective reduction and cleanup; shared with 268. |
| 204 | F1 | Fixed lapsed-Live-backing public trace converges and closes an owner position. |
| 219 | C21 | Public reset/carry prefix, effective reduction and exact rejection of the second bankruptcy; does not construct the alleged zero-OI prerequisite. |
| 237/257/258/287/288 | C22 | Two unauthorized withdrawal/resolve-policy calls reject with market/custody rollback. This is an authentication control, not five distinct policy-abuse histories or compromised-authority safety. |
| 269 | C23 | Publicly activates/funds 128 and 5,782 assets; stale ordinary exit/hint rejects, explicit resolution succeeds, and the victim receives 1,000,000 within 15 terminal calls. Fixed zero-delta marks. |
| 286 | C24 | Post-snapshot expired trade rejects/rolls back and owner progress remains; only the tested prerequisite. |
| 297 | C25, failing | Both prospective-only source accounts repeatedly submit invalid terminal hints; 32 rejections, so exact 99,900,000/100,100,000 payouts are not reached. |
| 308 | C26 | Post-ADL basis reissue rejects with `Custom(21)` and exact rollback; owner reduction still lowers OI to three units. |
| 370 | C8 | Same bounded Recovery/idle-owner exit as 252/368; no duplicate run. |
| 372 | C27 | Two-episode forfeit preserves prior 48,000 claim and pays 1,048,000 through owner-signed close. |
| 373 | C28 | Retained-before-haircut schedule preserves the prior claim floor and pays 1,099,000. |
| 374 | C29 | Retained-after-expiry execution leaves 5,000 capital/68,000 PnL and consumes no provider backing; not a generic expiry result. |
| 377 | C17 | Same provider withdrawal/restart/fresh-budget witness as 376. |

Legacy `privileged-self-action` classifications are preserved as ledger data.
Under loop.md, unauthorized-call rejection cannot dismiss authorized non-oracle
abuse that strands an independent user. No such new abuse was established here.

## Failing witnesses and open gaps

- C12 / row 298 fails at [line 1551](cu/inv_071_crank_progress.rs#L1551):
  `the matrix did not exercise bounded multi-step progress`. Its later zero-rejection,
  four-account terminal-state and 400,000,000-atom total-payout assertions are unexecuted.
- C25 / control 297 fails at [line 1746](cu/inv_071_crank_progress.rs#L1746):
  `the pinned predecessor unexpectedly locked`, observed 32 rejections versus zero.
  Its two terminal-state/exact-payout assertions are unexecuted.

Source diagnosis: both send `{asset_index: u16::MAX, oracle_accounts: u8::MAX}`
in Resolved mode. The [current handler](../../src/v16_program.rs#L14342) accepts
at most one in-range asset hint with zero oracle accounts, and rejects this input
with `InvalidInstruction` before close/settlement. These tests discard each error,
so the exact runtime rejection code is not logged; the explanation is source-derived.
C13's valid public expiry continuations pass, but are not a rerun of these two
exact economic histories. Their corrected-input suffixes remain unverified here.
Repeated malformed-input rejection does not satisfy loop.md's no-bounded-path DoS gate.

Rows **420/421/423/433 remain OPEN**. INV-028/073 remain `REFUTED_CURRENT`;
INV-071/077 remain `OPEN_EVIDENCE`. General future-resource reservation, arbitrary
terminal claim/liability/custody histories, and every maximum-shape product remain
open. Reserve payouts have empty-portfolio deletion prerequisites; administrative
retirement has named signers and is not itself proof of stranded user capital.
Existing native/recredit/dual-quote evidence is indexed in the
[terminal-reserve audit](terminal_reserve_evidence_audit_20260917.md); those extra
selectors were not rerun. No broader ledger obligation, including out-of-scope
row 424 contributing to INV-071, is closed by this selection.

## Exact verification

Private host cache; existing default-feature wrapper SBF reused, not rebuilt.
`git diff --exit-code cb236b5248 HEAD -- src Cargo.lock tests/fixtures/auth_matcher tests/fixtures/hostile_matcher`
passes; the only manifest delta is host `syn` parser features. Engine pin:
`94979ede7db934545e53a8f210dd063a9ea3ea63`.
SHA-256: wrapper `87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`;
copied auth matcher `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`;
external `/dev/shm/percolator-match/target/deploy/percolator_match.so`
`51f361c6fd00bdb91c685e98f081dea5a54665ef533a7f7b619916594aae6755`.
The external matcher's source/build provenance was not re-established.
No hostile matcher is required by this selection. Logs stay outside the repository.

Commands from this worktree (setup paths are literal; C/S/F numbering follows argument order):

```bash
cp -a --reflink=auto /dev/shm/percolator-public-gap-20260916-c91e-host-target /dev/shm/percolator-liveness-source-audit-20260917-host-target
mkdir -p /dev/shm/percolator-liveness-source-audit-20260917-logs tests/fixtures/auth_matcher/target/deploy
cp /dev/shm/percolator-main-merge-20260916/tests/fixtures/auth_matcher/target/deploy/auth_matcher.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
export CARGO_TARGET_DIR=/dev/shm/percolator-liveness-source-audit-20260917-host-target
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-public-gap-20260916-c91e-sbf-target/deploy/percolator_prog.so
export AUDIT_LOG_DIR=/dev/shm/percolator-liveness-source-audit-20260917-logs
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_071_crank_progress::v16_program_bankruptcy_escalation_matrix_commits_recovery_and_resolves \
  inv_073_no_permanent_user_lock::v16_program_recovery_residue_matrix_clears_abandoned_owner_residue \
  inv_077_bounded_work_and_maximum_shape_compute::v16_program_max_source_liquidation_asset_matrix_has_bounded_public_exits \
  inv_028_source_domain_realizability_cap::v16_program_expired_source_lien_route_matrix_preserves_bounded_owner_exit \
  inv_073_no_permanent_user_lock::v16_program_fractional_social_loss_exit_matrix_preserves_funded_owner_exit \
  inv_073_no_permanent_user_lock::v16_program_fragmented_recovery_pair_matrix_clears_every_fragment \
  inv_071_crank_progress::v16_program_micro_price_schedule_is_partition_invariant_and_eventually_progresses \
  inv_073_no_permanent_user_lock::v16_program_expired_partial_close_matrix_resolves_and_preserves_idle_exit \
  inv_051_canonical_adl_effective_quantity::v16_program_crossed_adl_effective_exit_matrix_preserves_bounded_cleanup \
  inv_051_canonical_adl_effective_quantity::v16_program_unilateral_adl_effective_exit_matrix_preserves_bounded_cleanup \
  inv_028_source_domain_realizability_cap::v16_program_source_capacity_admission_order_matrix_rejects_unreserved_risk \
  inv_071_crank_progress::v16_program_prospective_loss_expiry_matrix_keeps_resolved_exit_live \
  inv_028_source_domain_realizability_cap::v16_program_shared_expiry_progress_matrix_preserves_terminal_progress \
  inv_077_bounded_work_and_maximum_shape_compute::v16_program_max_source_conversion_and_owner_exit_are_bounded \
  inv_077_bounded_work_and_maximum_shape_compute::v16_program_max_shape_resolved_close_order_matrix_is_bounded_and_fair \
  inv_039_pending_loss_obligation_durability::v16_program_pending_obligation_blocks_close_then_releases \
  inv_073_no_permanent_user_lock::v16_program_asset0_recovery_matrix_preserves_provider_withdraw_and_restart_progress \
  inv_073_no_permanent_user_lock::v16_program_terminal_public_reserve_disposition_preserves_value_across_orders \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::v16_attack_permissionless_asset_insurance_authority_cannot_withhold_terminal_close \
  inv_028_source_domain_realizability_cap::historical_latent_capacity::generation_capacity_admission::v16_program_used_generation_admission_reserves_latent_capacity_through_exact_exit \
  inv_061_deterministic_bounded_liquidation::v16_program_reset_carry_liquidation_matrix_preserves_progress \
  inv_005_authority_incarnation_binding::v16_program_privileged_policy_boundary_matrix_rejects_untrusted_callers \
  inv_077_bounded_work_and_maximum_shape_compute::v16_program_dense_zero_delta_resolution_shape_matrix_keeps_terminal_exit_bounded \
  inv_063_backing_expiry_normalization::v16_program_post_snapshot_expiry_rejects_stale_trade_then_owner_progresses \
  inv_071_crank_progress::v16_program_prospective_source_expiry_prerequisite_matrix_keeps_exit_live \
  inv_071_crank_progress::v16_program_b_budget_lock_prerequisite_rejects_post_adl_basis_reissue \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::v16_program_prior_claim_forfeit_prerequisite_matrix_preserves_withdrawable_value \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::v16_program_retained_recovery_haircut_prerequisite_matrix_keeps_prior_claim_floor \
  inv_063_backing_expiry_normalization::v16_program_retained_recovery_expiry_prerequisite_matrix_avoids_provider_capitalization \
  inv_073_no_permanent_user_lock::v16_program_public_reserve_payments_wait_for_resolved_senior_disposition > "$AUDIT_LOG_DIR/cu.log" 2>&1
cargo test --locked --offline --test v16_program_stateful_fuzz -- --exact --nocapture --test-threads=1 \
  inv_028_source_domain_realizability_cap::v16_program_source_lien_reversal_exit_matrix_preserves_bounded_exit \
  inv_028_source_domain_realizability_cap::v16_program_cross_domain_rounding_exit_matrix_preserves_bounded_exit \
  inv_028_source_domain_realizability_cap::v16_program_flat_source_lien_route_matrix_preserves_bounded_claim_exit > "$AUDIT_LOG_DIR/stateful.log" 2>&1
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_program_fixed_blockers_remain_progressing > "$AUDIT_LOG_DIR/fixed.log" 2>&1
```

| Log | Exact result |
| --- | --- |
| `cu.log` | 28 passed, 2 failed (C12/C25), 1,409 filtered; 270.23s; exit 101. |
| `stateful.log` | 3 passed, 325 filtered; 65.76s; exit 0; default eight generated cases per property plus persisted regressions. |
| `fixed.log` | 1 passed, 146 filtered; 3.40s; exit 0. |

C20: 24 worlds, 192 exact rollbacks, 132 restored prefixes, 696 terminal calls,
1,152,598 peak CU and 842-byte maximum packet. C18: twelve reserve worlds,
231,485 peak CU. C14: conversion/withdrawal/deletion 712,072/41,954/26,540 CU.
C15: forward/reverse peaks 1,154,614/1,156,581 CU over 55/56 terminal calls.
C3: first/last-asset liquidation 1,253,438/1,162,267 CU, below 1,375,000 guards.
The public 5,782-asset control passes; these measures do not imply all maxima
compose. Existing compilation/future-compatibility warnings were nonfatal.

Final checks:

```bash
git diff --check cd751b44347f7d3ce4c5ece349571530535b875a
git diff --exit-code cd751b44347f7d3ce4c5ece349571530535b875a -- . ':!tests/invariants/README.md' ':!tests/invariants/liveness_source_capacity_audit_20260917.md'
git diff --cached --check
git show --format= --check HEAD
```

Whitespace and the documentation-only scope comparison pass. No broad suite,
mount census, Kani/engine proof, production build or Rust-format pass was run;
only this document and the invariant README entry change.
