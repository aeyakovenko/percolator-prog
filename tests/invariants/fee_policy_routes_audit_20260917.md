# Fee, policy, destination and route-equivalence audit, 2026-09-17

Base: fetched `origin/main`, `7c4e6291c186a24b092dc65125045d000b5f9552`.
Branch: `audit/fee-policy-routes-20260917`.
Worktree: `/dev/shm/percolator-fee-policy-routes-audit-20260917`.
Scope: INV-011/014/024/036/040/047/052/059/064 and the 21 named rows below.

All named families have mounted witnesses; **29 distinct selectors pass and three
existing witnesses fail**. No new public-route LoF/DoS gap was established.
This is an evidence audit, with no duplicate tests, production changes, ledger
changes or status promotion. Existing tests and open ledgers were comparison data;
no external issue/PR implementation was imported.

## Row map

F/C/S references identify selectors in command order below: F1-F13 are the first
fixed group, F14 the generation group, C1-C15 the first CU group, C16 the added
finite-budget control, and S1-S2 the stateful group. All execute deployed public
wrapper instructions in LiteSVM; successful test execution also confirms mounting.

| Rows | Selector(s), assertions and boundary |
| --- | --- |
| 223/259 | F1 tests four transports and both participant roles: no unauthorized debit/provider credit. Single routes additionally reject over-cap consent with rollback, then collect a positive fee and pay exactly that amount to the provider. Batch backing fees are unsupported/rejected, not positive execution coverage. F3's focused matcher-cap witness currently fails before provider withdrawal; see below. |
| 224 | F2: single/batch CPI ignore a caller's 10,000-bps LP fee; attacker profit, LP loss and withdrawable insurance are zero, insurance withdrawal rejects atomically, total payout is 2,000,000. |
| 256 | S1's `FreshSignedLiveBaseFee`: both traders freshly sign a 500-bps close after a live hike; exact authorized aggregate debit is 100,000. This certifies consent, not a ban on live policy changes. Retained cases are separate matrix lanes. |
| 284 | C5 compares mixed signs, both leg orders and all four transports with independent side budgets and terminal custody. C4 adds asymmetric terminal loss/support, but fails its negative-direction payout expectation. Its full route product is not currently certified. |
| 310/313 | F6/F5: both no-CPI transports reject stale base-fee consent; both CPI transports reject insufficient LP consent, including 499 versus 500 bps. Exact rollback, fresh 100,000 victim/LP fee, 200,000 insurance credit and 200,000,000 terminal payout prevent rejection-only coverage. |
| 314 | F4: retained creator cap 1 rejects installed activation fee 1,000 atomically; authorized activation charges and insures exactly one atom. |
| 325/326 | F14, owned by [INV-007](public_sbf/inv_007_no_aba_reuse.rs): whole-market recreation/reinitialization and retained maintenance/liquidation policy replay reject, with public trace and rollback checks. Current policy is permanent no-reuse; this is not successful market recreation followed by a generation-mismatch test. |
| 334/335/336/337/338/340/347/349 | F9: same-market supersession matrix requires stale rejection/no overwrite and a fresh, economically mutating control. Covers matcher config, five oracle control lanes, liquidation/maintenance/trade fees, redirect, resolve, and both backing sides. F10/F11 compare maintenance/liquidation recipient attribution with unchanged payer/victim value; F12 checks redirect terminal value; F13 checks both resolve-policy landing orders through funded exit. These are bounded histories, not the whole policy/route product. |
| 339 | F7: both top-up/policy landing orders preserve provider-approved terms with nonzero fees and exact provider/insurance attribution and payout. F8: funded terms reject economic changes, allow sequence-only refresh, and permit new terms after principal exit. |
| 411/432 | S1 includes retained single-CPI taker consent separately from permissive LP consent. C2 retains identical signed bytes across 19 -> 100 bps with taker cap 99 and LP cap 137: exact `InvalidInstruction` before matcher CPI, full non-payer rollback/exact network fee, then fresh consent charges 100 per owner. C3 executes permitted higher/lower policies with actual partial single-CPI and exact batch fills, rejects batch partial returns atomically, and pays both owners. Row 411 remains OPEN; row 432 remains COVERED. |

## Fee and route joins

| Invariants | Selector(s), nonvacuity and limits |
| --- | --- |
| 011 | C1: two-leg CPI aggregate fee/slippage exact-minus-one rejection restores market, both portfolios and matcher context; exact caps execute both legs/OI. Fee arithmetic is input-derived; slippage uses the production helper, so it is not an independent arithmetic oracle for that helper. |
| 024/036 | C6: retained redirect transaction rollback and fresh single/batch execution preserve per-payer floors; exact final domains are `[22,30,12,12,18,18,0,0]`, including the unused asset. S2: 32 clipped-fee worlds across four transports, two leg orders and participant orders, 712 public transactions, 288 exact rejections and 96 stock-balanced attribution controls. Distinct insurers receive their own fees, funded trader receives 697, vault empties. Signed leg order can legitimately change clipped attribution. |
| 040 | C7/C8: partly uncollectible full exits succeed on all four transports; no remaining OI, unchanged token custody, collected fees below quoted fees and exact aggregate capital debit. These older fixtures do not independently certify every owner's loss allocation. C9 adds clipped maintenance 7 of 30, refill 17, recipient-switch retries and exact owner/peer payouts without recharging forgiven debt. |
| 047/052 | C10: off-mark two-asset whole-leg partitions across all four transports, notionals 73/217, fees 2/3 per owner, typed limit rejects and normalized complete account frames. Matcher/grant/episode differences are explicitly checked and normalized. Quantity fragmentation is separate: C11 checks both closing signs and reordered partitions against aggregate, with at most one extra ceiling atom per extra fill, exact per-owner payouts and custody. Fixed-price/fixed-policy evidence only. |
| 052/059 | C12: maintenance total stays 580; cranker rewards 232/231/230 under one/two/three partitions, a conservative floor bound. C13: 40 liquidation histories and 160 rejected hints/retries; four trade transports lead to one engine-selected residual close, one positive minimum fee within cap, and exact owner payout. This is not caller-selected arbitrary liquidation sizing. |
| 064 | C16: four ordered/split Live-to-Resolved histories drain the same 60-atom endowment (7 Live, 53 terminal), then reject exhaustion. C15 rejects removed tags 23/33/41 without market/vault changes. C14's stronger schedule/account-frame comparison fails; see below. Current routes use finite per-asset budgets and authority epochs, not the removed limited-withdrawal/cooldown policy surface. |

Related existing evidence is already documented in the
[partial-policy audit](pr135_scope_m_single_cpi_fee_consent_20260913.md),
[multi-source fee audit](inv_036_multi_source_fee_audit_20260916.md), and
[clipped redirect audit](inv_036_clipped_redirect_audit_20260916.md).
The existing audits of the multi-source and generated partial-policy-word
selectors were reviewed; neither selector was rerun here. Their arbitrary-history, concurrent-source,
expiry, clipping and changing-policy limits remain; they do not close row 411.

## Current failing witnesses

All three reproduce with `--test-threads=1`; downstream assertions are not passes.

- **F3 / PR223:** [helper](../support/fuzz_model.rs#L22856) reports
  `landed true, capital delta -150`, against hardcoded maintenance debit 120.
  The earlier unauthorized-cap rollback and positive authorized LP/provider fee
  checks are reached successfully. Failure is at the zero-cap reduction's capital
  expectation, before provider SPL withdrawal. The fixture charges 30 per slot;
  this audit does not establish a production cause or certify the remaining exit.
- **C4 / directional fees:** [negative-direction payout](cu/inv_036_fee_destination_and_policy_version_integrity.rs#L320)
  is **1,000,001**, expected **1,000,000**. Positive-direction comparisons complete;
  the negative single-no-CPI reference fails before the other negative transports.
  C5/S2 passing fee attribution does not establish this terminal-support expectation.
- **C14 / insurance schedules:** [line 604](cu/inv_064_insurance_withdrawal_policy_equivalence.rs#L604)
  fails `schedules converge on full non-payer account frames`. Both schedules
  first pass exact budget/custody checks and leave `[0,0,12,19]`, vault 31.
  The schedules make different numbers of successful withdrawals; production
  [advances the authority epoch on every debit](../../src/v16_program.rs#L11052).
  Byte equality therefore conflicts with current replay protection. This is a
  source-based explanation, not a decoded comparison proving no other differing
  fields. The fragmented schedule's final exhaustion retry is not reached.

The initial C3 failure was only a missing hostile-matcher artifact. After a fresh
locked/offline build, C3 passes four histories, two rollbacks, four fills and eight
owner payouts (155,834 peak CU); it is not a current behavioral failure.

Status ledger remains: INV-014 `REFUTED_CURRENT`; INV-011/024/036/047/064
`OPEN_EVIDENCE`; INV-040/052/059 `SUPPORTED`. All remain sampled/conditional.
Passing bounded selectors neither repair those statuses nor prove arbitrary
retained histories, all fee sources, maximum shapes, or terminal liveness.
Clock/provider reports and matcher programs retain their fixture assumptions.

## Exact verification

Private host cache copied from
`/dev/shm/percolator-public-gap-20260916-c91e-host-target`.
Wrapper SBF reused from the matching source checkpoint `cb236b5248c941ffcb33e1e6afe62801e8a19712`:
`git diff --exit-code cb236b5248 HEAD -- src Cargo.lock tests/fixtures/auth_matcher tests/fixtures/hostile_matcher`
passes; the manifest delta only enables host `syn` parser features. No wrapper
rebuild is claimed. Engine pin: `94979ede7db934545e53a8f210dd063a9ea3ea63`.

Artifact SHA-256:
- Wrapper: `87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.
- Copied authenticated matcher: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
- Fresh hostile matcher: `e0c20fad34a7822cc6ce42a3c77ff08a8591977102f0c497a339d66a9dd6240a`.
- Existing external `/dev/shm/percolator-match/target/deploy/percolator_match.so`:
  `51f361c6fd00bdb91c685e98f081dea5a54665ef533a7f7b619916594aae6755`;
  its source/build provenance was not re-established.

Commands from the audit worktree; full logs are under
`/dev/shm/percolator-fee-policy-routes-audit-20260917-logs/`.
No broad suite, source census or Kani run was performed.

```bash
export CARGO_TARGET_DIR=/dev/shm/percolator-fee-policy-routes-audit-20260917-host-target
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-public-gap-20260916-c91e-sbf-target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_program_fuzz_regressions --test v16_cu --no-run
CARGO_TARGET_DIR=/dev/shm/percolator-fee-policy-routes-audit-20260917-matcher-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/hostile_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/hostile_matcher/target/deploy -- --locked
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture \
  inv_036_fee_destination_and_policy_version_integrity::v16_program_retained_source_fee_caps_bind_every_single_trade_role \
  inv_036_fee_destination_and_policy_version_integrity::v16_program_pr224_unsigned_lp_caller_fee_is_ignored \
  inv_036_fee_destination_and_policy_version_integrity::v16_program_pr223_unsigned_lp_backing_fee_requires_matcher_consent \
  inv_036_fee_destination_and_policy_version_integrity::v16_program_pr314_permissionless_activation_fee_requires_creator_consent \
  inv_036_fee_destination_and_policy_version_integrity::v16_program_pr313_cpi_base_fee_requires_lp_consent \
  inv_036_fee_destination_and_policy_version_integrity::v16_program_pr310_bilateral_base_fee_requires_fresh_consent \
  inv_014_delayed_policy_and_policy_epoch_safety::v16_program_backing_provider_fee_terms_survive_both_landing_orders \
  inv_014_delayed_policy_and_policy_epoch_safety::v16_program_backing_provider_exit_unfreezes_policy_change \
  inv_014_delayed_policy_and_policy_epoch_safety::v16_program_same_market_delayed_controls_reject_atomically \
  inv_014_delayed_policy_and_policy_epoch_safety::v16_program_maintenance_share_supersession_changes_attribution_not_payer_value \
  inv_014_delayed_policy_and_policy_epoch_safety::v16_program_liquidation_share_supersession_changes_attribution_not_victim_value \
  inv_014_delayed_policy_and_policy_epoch_safety::v16_program_fee_redirect_supersession_preserves_terminal_domain_value \
  inv_014_delayed_policy_and_policy_epoch_safety::v16_program_resolve_policy_supersession_preserves_a_complete_funded_exit
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture \
  inv_007_no_aba_reuse::v16_program_whole_market_recreate_aba_matrix_is_public_and_nonvacuous
cargo test --locked --offline --test v16_cu -- --exact --nocapture \
  inv_011_signed_aggregate_economic_bounds::v16_program_batch_cpi_aggregate_quote_caps_abort_matcher_and_wrapper_atomically \
  inv_014_delayed_policy_and_policy_epoch_safety::v16_retained_single_cpi_taker_fee_cap_rejects_policy_increase \
  inv_014_delayed_policy_and_policy_epoch_safety::v16_retained_fee_terms_bound_partial_and_exact_fill_routes_after_policy_change \
  inv_036_fee_destination_and_policy_version_integrity::v16_program_signed_direction_route_matrix_preserves_side_attribution_and_terminal_value \
  inv_036_fee_destination_and_policy_version_integrity::v16_program_mixed_direction_fee_allocation_matches_independent_side_ledger \
  inv_036_fee_destination_and_policy_version_integrity::v16_program_retained_redirect_bundle_preserves_fee_rounding_and_policy_order \
  inv_040_no_fee_seniority::v16_program_uncollectible_exit_fee_is_dropped_not_senioritized \
  inv_040_no_fee_seniority::v16_program_cpi_uncollectible_exit_fee_is_dropped_not_senioritized \
  inv_040_no_fee_seniority::v16_program_clipped_maintenance_refill_retries_cannot_recharge_or_redirect \
  inv_047_equivalent_route_semantics::fee_leg_partition::v16_program_off_mark_quotes_preserve_fractional_fee_route_equivalence \
  inv_052_split_merge_invariance::v16_program_split_fee_close_has_bounded_rounding_and_exact_custody \
  inv_052_split_merge_invariance::v16_program_maintenance_fee_cadence_is_conservative_and_value_exact \
  inv_059_fee_fragmentation_bound::v16_program_minimum_fee_episode_histories_match_aggregate_close \
  inv_064_insurance_withdrawal_policy_equivalence::v16_program_insurance_withdrawal_schedules_preserve_asset_allowance_and_exact_retry \
  inv_064_insurance_withdrawal_policy_equivalence::v16_attack_removed_limited_insurance_tags_reject_without_mutation
cargo test --locked --offline --test v16_program_stateful_fuzz -- --exact --nocapture \
  inv_014_delayed_policy_and_policy_epoch_safety::v16_program_fee_consent_operation_matrix_discovers_unsigned_debits \
  inv_036_fee_destination_and_policy_version_integrity::clipped_redirect_partition::v16_program_clipped_redirect_fees_preserve_rounding_and_recipient_payouts
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_014_delayed_policy_and_policy_epoch_safety::v16_retained_fee_terms_bound_partial_and_exact_fill_routes_after_policy_change \
  inv_036_fee_destination_and_policy_version_integrity::v16_program_signed_direction_route_matrix_preserves_side_attribution_and_terminal_value \
  inv_064_insurance_withdrawal_policy_equivalence::v16_program_insurance_withdrawal_schedules_preserve_asset_allowance_and_exact_retry \
  inv_064_insurance_withdrawal_policy_equivalence::v16_program_live_and_resolved_insurance_withdrawals_share_one_finite_budget
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_036_fee_destination_and_policy_version_integrity::v16_program_pr223_unsigned_lp_backing_fee_requires_matcher_consent
git diff --check 7c4e6291
git diff --exit-code 7c4e6291 -- src Cargo.toml Cargo.lock tests/fixtures tests/support tests/invariants/cu tests/invariants/stateful tests/invariants/public_sbf tests/invariants/*.tsv proptest-regressions
git diff --cached --check
git show --format= --check HEAD
```

| Log | Result |
| --- | --- |
| `build.log`, `matcher-build.log` | Compilation passes; existing warnings only. |
| `fixed.log` | 12 passed, 1 failed, 134 filtered; 6.89s; exit 101. |
| `generation.log` | 1 passed, 146 filtered; 5.49s; exit 0. |
| `cu.log` | 12 passed, 3 failed (one missing fixture), 1,424 filtered; 17.14s; exit 101. |
| `stateful.log` | 2 passed, 326 filtered; 39.35s; exit 0. Generic fee matrix uses default eight generated cases plus persisted regressions. |
| `cu-recheck.log` | 2 passed (C3/C16), 2 failed (C4/C14), 1,435 filtered; 6.03s; exit 101. |
| `pr223-recheck.log` | 0 passed, 1 failed, 146 filtered; 0.76s; exit 101. |

Unique final outcomes: **29 pass / 3 fail**, not a count of repeated executions.
Whitespace, unchanged production/test/ledger diff and committed-patch checks pass.
Only this audit and its README entry change; Rust formatting is not applicable.
