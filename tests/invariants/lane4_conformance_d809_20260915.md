# Lane 4 Conformance Verification

## Provenance

- Base: `d809e9a563d9b8bf38f32648b32a15d75f526ec8`.
- Branch: `codex/lane4-public-conformance-d809`.
- Workspace: `/home/anatoly/percolator-prog-lane4-d809`.
- Baseline comparison: `/home/anatoly/percolator-prog-lane4-baseline-d809`, detached at the same base.
- Engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`; default Anchor-v2 features, platform-tools v1.52.
- Fresh wrapper SBF SHA-256: `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
- Fresh auth matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
- Fresh hostile matcher SHA-256: `e0c20fad34a7822cc6ce42a3c77ff08a8591977102f0c497a339d66a9dd6240a`.

The shared checkout was not at the requested commit and contained an unresolved
`tests/v16_cu.rs`. Its branch, HEAD and tracked changes were left untouched.
Only external oracle/Clock fixtures supply observations; the regression conditions
and successful continuations use public wrapper instructions. No production code,
engine pin, dependency lockfile or machine verdict is changed.

## Changed Files

| File | Change |
| --- | --- |
| `cu/inv_045_paid_origin_hybrid_recipient.rs` | Extend four no-CPI cases to eight cases across all four discovery routes and both publication orders. |
| `cu/inv_045_no_free_mark_movement.rs` | Repair the caught-up reward history: prior-slot evidence must roll back, followed by authenticated renewal, selected-asset liquidation, owner exit and exact keeper payout in both hint orders. |
| `cu/inv_053_full_health_recertification_equivalence.rs` | Share the existing AuthMark maximum-leg fixture with a new Hybrid matrix using fourteen distinct feeds; check fourteen omissions and fourteen structurally complete prior-slot replays, complete account frames and successful full refresh. |
| `cu/inv_077_bounded_work_and_maximum_shape_compute.rs` | Extend the existing two-chunk backlog witness through decreasing slot backlog, risk recertification, fourteen bilateral reductions, exact withdrawal and portfolio close. |
| `cu/inv_077_short_side_b_budget.rs` | Remove equality to a historical CU count; retain every per-call ceiling, exact B-rank reduction, source attribution and custody assertion. |
| `public_sbf/inv_045_no_free_mark_movement.rs` | Extend the existing four target-writer cases to single and batch CPI continuation. |
| `../support/fuzz_model.rs` | Parameterize only the target-staging reproduction's continuation route; preserve the original single-CPI entry point for existing callers. |
| `README.md`, this report | Record scope, commands, measured evidence and unresolved baseline failures. |

## Row Verdicts

| Row / Invariant | Verdict for this lane |
| --- | --- |
| 422 / INV-045, INV-020, INV-061 | **OPEN.** Eight dual-Hybrid reward histories agree: discovery stock 1,540,072; retained old penalty 5,987; fresh penalty 8,287; fresh reward 2,762; keeper SPL payout 3,762 atoms. Old discovery/penalty stock never becomes new domain entitlement. The separately repaired caught-up selector pays 1,966 reward atoms from a 5,899-atom selected-asset fee in both hint orders. Arbitrary reward/funding/maintenance/policy/terminal histories remain outside this evidence. |
| 426 / INV-020, INV-053 | **COVERED**, unchanged. The existing rescue regression passes. The new maximum-leg case rejects every missing/prior-slot Hybrid report before health refresh, frames both portfolios, market, custody and external accounts, and completes a current refresh with unchanged positions and exactly 70 loss atoms. This is bounded shape evidence, not universal observation-history closure. |
| 427 / INV-058 | **OPEN.** Existing disjoint-pair, generated existing-leg, fee competition, PnL/terminal and liquidation/reset capacity witnesses pass. The recreated-counterparty CPI transfer test fails on the untouched base with `EngineInvalidLeg`; its expected-success transfer is not covered by this lane. No new OI test duplicates those witnesses. |
| 332/333 / INV-045 | **Expanded bounded coverage.** Both CPI continuations see already-staged wrapper/engine targets and an advanced oracle epoch, reject stale risk increase with rollback, retain lagging reduction where a position exists, and complete the post-catch-up round trip without owner value transfer. |
| 366 / INV-053, INV-061 | **Reconfirmed bounded coverage.** The later-leg rounded funding rescue and no-observation funding/liquidation controls pass, as do all nine INV-053 and all selected INV-061 CU tests. |
| 212/357/359 / INV-077 | **Reconfirmed at the executed shapes.** Public 28-source conversion/lien release, 14-leg/28-source direct and permissionless resolved close, liquidation, and owner exits remain bounded. This does not prove all maximum-shape products. |
| 269 / INV-077 | **Historical nonqualifying label unchanged.** The public 128- and 5,782-asset zero-delta resolution worlds complete bounded terminal withdrawal of the funded owner's 1,000,000 atoms. |
| 423 / INV-077 | **OPEN.** Source-capacity reclamation restores admission and exact owner payouts, but two Hybrid 14-leg/28-source fixture constructions still fail before their measured continuation. Combined feed/source/backlog/occupancy closure is not claimed. |

## Required Progress and CU

| Successful public continuation | Observed CU |
| --- | ---: |
| New fourteen-distinct-feed current Hybrid health refresh | 901,162 |
| Existing fourteen-leg AuthMark health control | 879,369 |
| Eight-world dual-Hybrid reward history, largest measured transaction | 389,039 |
| Renewed caught-up Hybrid reward crank | 323,330 |
| Backlog schedule, largest of fifteen cranks | 747,789 |
| Counterparty settlement / owner risk recertification after backlog | 738,726 / 502,182 |
| Largest of fourteen post-backlog reductions | 723,792 |
| Post-backlog withdrawal / portfolio close, observed maxima | 43,428 / 26,516 |
| 28-source full PnL conversion | 712,072 |
| Direct resolved close at 14 legs / 28 sources | 1,156,436 |
| Maximum-source liquidation, tested asset choices | 1,253,058 |
| Four-world equal-risk liquidation/exit matrix | 1,155,321 |
| Full active/source no-reward maintenance charge | 1,253,143 |
| Four-atom short-side B settlement | 565,955 |

The backlog sum falls from 896 to zero in fifteen public cranks. Settling the
counterparty's gain advances the risk epoch, so a second public account refresh
precedes the owner exit. In total, 33 protocol calls complete catch-up, both
refreshes, fourteen reductions, withdrawal of exactly 9,999,930 atoms, and close.
The fixture uses three shared composite feeds, at most two hints per catch-up
call, and fourteen active legs. It does not also saturate the source table or
market capacity. All measured required steps fit the 1,400,000 transaction limit;
the applicable test ceilings remain stricter.

The short-side B witness already checks every call against 1,375,000 CU and
checks that 28 calls discharge exactly 84 atoms at fourteen legs/twenty-eight
sources. Its old `max_cu == 565_957` assertion rejected a successful 565,955-CU
execution. Removing that equality corrects test evidence, not program behavior.

## Commands and Results

All builds ran offline with locked dependencies in private target directories.
The wrapper and both matchers were built with:

```sh
env CARGO_TARGET_DIR=/dev/shm/lane4-d809-sbf CARGO_BUILD_JOBS=4 cargo build-sbf --tools-version v1.52 --sbf-out-dir /home/anatoly/percolator-prog-lane4-d809/target/deploy -- --locked --offline
env CARGO_TARGET_DIR=/dev/shm/lane4-d809-sbf CARGO_BUILD_JOBS=4 cargo build-sbf --tools-version v1.52 --manifest-path tests/fixtures/auth_matcher/Cargo.toml --sbf-out-dir /home/anatoly/percolator-prog-lane4-d809/tests/fixtures/auth_matcher/target/deploy -- --locked --offline
env CARGO_TARGET_DIR=/dev/shm/lane4-d809-sbf CARGO_BUILD_JOBS=4 cargo build-sbf --tools-version v1.52 --manifest-path tests/fixtures/hostile_matcher/Cargo.toml --sbf-out-dir /home/anatoly/percolator-prog-lane4-d809/tests/fixtures/hostile_matcher/target/deploy -- --locked --offline
env CARGO_TARGET_DIR=/dev/shm/lane4-d809-host CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
```

Direct harness executions set
`PERCOLATOR_FUZZ_SBF=/home/anatoly/percolator-prog-lane4-d809/target/deploy/percolator_prog.so`.
The binaries were `/dev/shm/lane4-d809-host/debug/deps/v16_cu-8c7475d0975fcbd3`
and `v16_program_fuzz_regressions-81682abeb647f5d7` in the same directory.
Each filter below was passed as a separate positional argument, with `--nocapture`.

| Run / log under `target/` | Selection and result |
| --- | --- |
| `lane4-oracle-health.log` | CU filters `inv_020_ inv_053_ inv_058_ inv_061_ v16_attack_liquidation_cannot_omit_fresh_external_rescue_observation retained_penalty_handoff v16_program_caught_up_hybrid_reward_uses_selected_asset_provenance_in_both_hint_orders`, four threads: initial **115 passed, 17 failed**. One was a missing hostile matcher, subsequently built and rerun; sixteen reproduced on the untouched base. |
| `lane4-baseline.log` | Fresh baseline CU compilation, `inv_020_` plus the caught-up reward, recreated-counterparty and hostile-matcher cap selectors: **68 passed, 16 failed**. |
| `lane4-staging-metadata.log` | Public-SBF filters `inv_045_ inv_020_ inv_053_ inv_079_`, four threads: **26 passed**. Includes all eight target-writer/continuation cases, public-route provenance, charter/index, benchmark and authoritative-status gates. |
| `lane4-updated.log` | New maximum-Hybrid refresh, repaired caught-up reward and extended backlog selectors: **3 passed**. |
| `lane4-controls.log` | CU filters `inv_053_ inv_061_ retained_penalty_handoff` plus the caught-up reward, mark-writer source gate and hostile-matcher cap selector, four threads: **34 passed**. |
| `lane4-max-shape.log` | Twenty selected INV-077 witnesses, three threads: **17 passed, 3 failed** before the short-B metric correction. The three failures are identified below; the other seventeen complete successful required progress. |
| `lane4-baseline-max-shape.log` | Forced fresh baseline compilation after `cargo clean -p percolator-prog`: both Hybrid max-shape selectors and short-B selector reproduce all **3 failures** on untouched `d809e9a5`. |
| `lane4-final.log` | Forced fresh changed-head compilation: **6 CU selectors passed**, then **18 public-SBF target-staging/metadata selectors passed**. Includes the repaired short-B assertion and the final README. |

Final command (with the same host environment above and `PERCOLATOR_FUZZ_SBF` set):

```sh
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions -- v16_program_max_shape_hybrid_refresh_requires_each_current_report v16_program_max_shape_refresh_rejects_each_single_omitted_pending_leg v16_program_paid_origin_penalty_survives_dual_hybrid_recipient_catchup v16_program_caught_up_hybrid_reward_uses_selected_asset_provenance_in_both_hint_orders v16_bpf_public_full_14_leg_three_feed_max_backlog_has_bounded_refresh_schedule v16_program_max_shape_short_b_budget_has_exact_public_progress v16_program_pr264_pr265_pr332_pr333_targets_stage_before_stale_cpi inv_079_ --test-threads=3 --nocapture
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_045_no_free_mark_movement.rs tests/invariants/cu/inv_045_paid_origin_hybrid_recipient.rs tests/invariants/cu/inv_053_full_health_recertification_equivalence.rs tests/invariants/cu/inv_077_bounded_work_and_maximum_shape_compute.rs tests/invariants/cu/inv_077_short_side_b_budget.rs tests/invariants/public_sbf/inv_045_no_free_mark_movement.rs tests/support/fuzz_model.rs
git diff --check
```

Formatting and whitespace checks pass. `git diff --name-only d809e9a5 -- src
Cargo.toml Cargo.lock tests/invariants/invariant_status.tsv
tests/invariants/open_findings.tsv tests/invariants/coverage_reopenings.tsv`
is empty. The two isolated checkouts were created with `git worktree add` at
`d809e9a5`; all build artifacts and logs remain outside the shared checkout.

The twenty max-shape filters were:

```text
v16_program_max_source_conversion_and_owner_exit_are_bounded
v16_program_max_source_flat_lien_release_and_owner_exit_are_bounded
v16_program_max_source_capacity_reclamation_restores_funded_exit
v16_program_max_shape_resolved_close_order_matrix_is_bounded_and_fair
v16_program_direct_close_resolved_at_14_leg_28_source_shape_is_bounded
v16_program_max_shape_owner_window_signature_has_bounded_public_progress
v16_program_max_source_liquidation_asset_matrix_has_bounded_public_exits
v16_attack_public_14_leg_28_source_equal_risk_liquidation_stays_bounded
v16_attack_public_14_leg_28_source_42_feed_refresh_stays_bounded
v16_program_dense_zero_delta_resolution_shape_matrix_keeps_terminal_exit_bounded
v16_bpf_public_full_14_leg_composite_oracle_liquidation_progress_is_bounded
v16_bpf_public_full_14_leg_three_feed_oracle_refresh_is_bounded
v16_bpf_public_full_14_leg_three_feed_max_backlog_has_bounded_refresh_schedule
v16_bpf_full_14_leg_16_hint_three_feed_refresh_is_bounded
v16_attack_public_14_leg_28_source_recovery_forfeit_stays_bounded
v16_program_recovery_kf_refresh_at_14_leg_28_source_shape_is_bounded
v16_attack_public_recovery_kf_progress_survives_stale_42_feed_tail_at_max_shape
v16_program_public_full_shape_maintenance_has_bounded_continuation
v16_program_public_full_shape_b_backlog_has_bounded_settlement
short_side_b_budget
```

## Remaining Baseline Failures

The caught-up reward fixture and exact-CU assertion are corrected in this lane.
The seventeen remaining failures below are retained and reported, not skipped or relabeled
as passing. They do not establish a public loss or the absence of every bounded
continuation, so this report assigns no new security severity.

Fourteen INV-020 selectors fail on the clean baseline around partial observation
or report renewal. Most return `EngineNonProgress`; the chunked selector instead
asserts that the rejected portfolio must be its peer. Exact leaf selectors:

```text
v16_program_chunked_mixed_observations_gate_full_refresh_and_preserve_claims
v16_program_active_claim_conversion_distinguishes_current_cert_from_complete_evidence
v16_program_generated_current_hybrid_recipient_routes_match_full_health_and_rollback
v16_program_cpi_active_keeper_observations_preserve_admission_and_payout
v16_program_repeated_active_rewards_preserve_renewed_observations_and_exact_exit
v16_program_composite_recipient_target_routes_restore_missing_evidence_prefixes_and_payout
v16_program_reward_recipient_becomes_liquidation_target_after_composite_refresh
v16_program_active_keeper_reward_recertifies_unrelated_loss_across_partial_refresh
v16_program_interrupted_refresh_preserves_fee_and_liquidation_entitlements
v16_program_selected_provider_assignment_preserves_fee_domains_and_owner_exit
v16_program_mixed_provider_liquidation_omissions_preserve_exact_entitlements
v16_program_partial_observation_three_leg_reductions_match_single_and_batch
v16_program_partial_sibling_observations_cannot_expand_single_or_batch_risk_capacity
v16_program_observation_abort_restores_liquidation_reward_payout_and_intent
```

`v16_program_recreated_counterparty_preserves_post_transition_cumulative_limits`
fails its expected-success CPI transfer (`taker=1`, `maker=2`, asset 0,
quantity `-99,999,999,999,999`) with `EngineInvalidLeg`, before the recreation
portion can establish the claimed conformance. Other independently owned row-427
tests pass; their evidence does not cure this failing selector.

`v16_attack_public_14_leg_28_source_42_feed_refresh_stays_bounded` and
`v16_attack_public_recovery_kf_progress_survives_stale_42_feed_tail_at_max_shape`
fail the shared public Hybrid construction with `EngineNonProgress` at 38,056 CU.
Their intended max-shape continuation is never reached. This is a coverage gap,
not a compute-exhaustion result. The passing AuthMark source-shape tests and
Hybrid feed/backlog tests cannot be multiplied into that missing product.

Production correction: none. The work changes regression coverage and two test
expectations; it does not demonstrate a new qualifying LoF or no-escape DoS.
