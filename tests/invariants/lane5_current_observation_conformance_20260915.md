# Lane 5 Current Observation Conformance

## Provenance and Verdict

- Base: `origin/codex/astra-invariant-cycle-20260915`, fetched as
  `e1394241e46ba1d66e21b43f6c1c99d83761dcca`.
- Branch: `codex/lane5-current-observation-20260915`.
- Isolated clone: `/tmp/percolator-lane5-observation-20260915`.
- Engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Default Anchor-v2 features, platform-tools v1.52, locked/offline builds.
- Wrapper SBF SHA-256:
  `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
- Auth matcher SHA-256:
  `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
- Hostile matcher SHA-256:
  `e0c20fad34a7822cc6ce42a3c77ff08a8591977102f0c497a339d66a9dd6240a`.

All fourteen Lane 4 baseline failures classify as **(a) stale test expectations**.
They expect health refresh using Hybrid reports previously authenticated in an
earlier slot. The unchanged production build accepts all corrected continuations.
There is also a **(b) missing coverage dimension** in the generated-current-Hybrid
selector: its old missing-account cases only exercised declared-but-absent tails,
and its supposed current control explicitly asserted slot-zero provenance. This
lane adds well-formed observation omissions and current-slot controls. No selector
establishes **(c) a genuine implementation mismatch**, and no production correction
is retained.

The shared checkout was only read. The clone has its own Git metadata; neither
its branch operations nor its build outputs modify `/home/anatoly/percolator-prog`.
The source documents read first were `scripts/loop.md`, the invariant README,
`open_findings.tsv`, `invariant_status.tsv`, and the Lane 4 report.

## Contract

`PermissionlessCrank` may commit bounded market catch-up without changing the
target portfolio. Before subsequent stale or liquidatable account-health work,
`reject_incomplete_account_health_observations_view` checks every active leg.
For Hybrid legs whose soft-stale fallback has not matured,
`reject_incomplete_asset_health_observation_view` requires
`last_good_oracle_slot == authenticated_now_slot`. Replaying a previously accepted
publication does not renew that provenance. A complete hint list alone is
insufficient. AuthMark siblings also retain their own pending price/funding work.

The updated histories keep the old market-only prefixes, reject old reports where
health would be consumed, and supply new external reports with the same prices
and later publication times. Only legitimate Clock/external-provider fixtures are
updated. System/SPL/ATA/wrapper calls still construct all economic state. Original
exact balances, certificate comparisons, intent epochs, source/stock censuses,
prefix rollback and public exits remain asserted. No assertion is weakened to
accept either success or an arbitrary error.

## Selector Verdicts

All names below are exact leaf selectors in `v16_cu`; all are mounted under
`inv_020_authenticated_clock_slot_and_oracle_provenance`.

| Selector | Verdict and retained evidence |
| --- | --- |
| `v16_program_chunked_mixed_observations_gate_full_refresh_and_preserve_claims` | **(a), PASS.** The first owner's slot-64 refresh fails; the old assertion incorrectly restricts rejection to the already-current peer. Assert complete-old-report rollback, renew at slots 64-67, and separately reject fresh Hybrid with the pending AuthMark omitted. Sixteen direction/order/route worlds retain exact fractional carry, 17,300-atom intermediate and 20,100-atom final signed claims, and both SPL debits. |
| `v16_program_active_claim_conversion_distinguishes_current_cert_from_complete_evidence` | **(a), PASS.** The slot-4 catch-up reused the report authenticated at slot 2. Renew that report at publication 103. Four worlds retain the payable control, ten exact rejected conversions, current certificates and all four successful active-leg claim conversions. |
| `v16_program_generated_current_hybrid_recipient_routes_match_full_health_and_rollback` | **(a), PASS; additional (b) gap closed.** The old expected-success refresh asserted `last_good_oracle_slot == 0` at Clock 64. Preserve that fact at the rejected prefix, then require both profiles at slot 64/publication 102. Thirty-two worlds add 160 exact freshness rollbacks: complete old evidence plus each role's properly omitted or prior-slot Hybrid report while supplying its sibling. Preserve 384 existing malformed/stale-input rejections, 40 exit rollbacks, 224 independent/full-refresh certificate comparisons and exact 252,925-atom keeper payout across four trade routes. |
| `v16_program_cpi_active_keeper_observations_preserve_admission_and_payout` | **(a), PASS.** Slot-64 target refresh reused publication 101. Reject it, renew publication 102, and retain eight single/batch CPI worlds, 36 total exact rollbacks, active recipient lag, admission, reductions and 252,925-atom payout. |
| `v16_program_repeated_active_rewards_preserve_renewed_observations_and_exact_exit` | **(a), PASS.** Each episode reused its opening-slot report at its ending slot. Give each episode distinct staging/renewal publications; reject the staged report before renewal. Four histories retain twelve rewards, both observation orders, interruption/retry, 24 exact rollbacks and exact final exit. |
| `v16_program_composite_recipient_target_routes_restore_missing_evidence_prefixes_and_payout` | **(a), PASS.** The first target refresh reused slot-zero provenance. Renew only the target's report, retaining old recipient component reports for the composite-epoch rejection. Sixteen bilateral/CPI worlds retain 88 exact rollbacks, eight restored SPL payout prefixes and exact target/recipient entitlement. |
| `v16_program_reward_recipient_becomes_liquidation_target_after_composite_refresh` | **(a), PASS.** Same shared target-refresh precondition as the preceding selector. Four worlds retain recipient component coherence, twelve exact rollbacks, first/second fees of 8,778/3,564, rewards of 2,925/1,187 and 69,361-atom recipient exit. |
| `v16_program_active_keeper_reward_recertifies_unrelated_loss_across_partial_refresh` | **(a), PASS.** Slot-64 target refresh reused publication 101. Reject then renew; preserve eight single/batch bilateral worlds, 24 exact rollbacks, unrelated loss and adverse lag, 8,778-atom penalty, 2,925-atom reward and 252,925-atom keeper exit. |
| `v16_program_interrupted_refresh_preserves_fee_and_liquidation_entitlements` | **(a), PASS.** End-slot 64/65 health refresh reused the staged report. Supply publication 103 at the final Clock boundary. Sixteen explicit/implicit-fee, order and omission worlds retain sixty exact rollbacks, strict catch-up rank, exact maintenance/liquidation attribution and SPL rewards. |
| `v16_program_selected_provider_assignment_preserves_fee_domains_and_owner_exit` | **(a), PASS.** The supposedly successful prefix before a wrong-provider suffix reused slot-1 reports at slot 2. Reject old evidence, renew both provider fixtures at slot 2, and preserve immutable-account frames before and after renewal. Eight selected-provider/order worlds retain 48 exact rollbacks, domain attribution, independent successful-prefix simulation, exact owner exit and 52 terminal calls. |
| `v16_program_mixed_provider_liquidation_omissions_preserve_exact_entitlements` | **(a), PASS.** Same slot-1/slot-2 replay in the mixed-provider successful prefix. Reject then renew Pyth and Switchboard/Chainlink reports. Eight worlds retain 48 exact rollbacks, omission after refresh, 8,778-atom fee, 2,925-atom reward, 5,853 retained insurance atoms and exact payouts. |
| `v16_program_partial_observation_three_leg_reductions_match_single_and_batch` | **(a), PASS.** Market-only prefixes legitimately run, but later explicit owner recertification still uses slot-zero Hybrid provenance. Renew at slot 65 while retaining the partial AuthMark schedule. Sixteen worlds preserve decreasing backlog, both owner orientations, independent current values and exact single/batch reductions through all three legs. |
| `v16_program_partial_sibling_observations_cannot_expand_single_or_batch_risk_capacity` | **(a), PASS.** Same stale Hybrid precondition in the full-current comparison. Require publication 103 and provenance slot 65. Four worlds retain the pending sibling's conservative capacity bound, four successful-trade/failed-tail atomic rollbacks and exact convergence to full-current admission. |
| `v16_program_observation_abort_restores_liquidation_reward_payout_and_intent` | **(a), PASS.** The refresh at the start of the payout word reused publication 101 from slot zero. Reject it, renew at slot 64/publication 102, then run the unchanged payout/abort word. Four worlds retain all four aborts after real SPL transfers, twelve total exact rollbacks, retained intent consumption/retry and exact 5,389-atom payout. |

## Changed Files

All Rust changes are in existing owners under `tests/invariants/cu/`:

- `inv_020_active_claim_evidence.rs`
- `inv_020_active_keeper_observations.rs`
- `inv_020_chunked_observation_admission.rs`
- `inv_020_cpi_keeper_observations.rs`
- `inv_020_generated_current_hybrid.rs`
- `inv_020_interrupted_refresh_fees.rs`
- `inv_020_mixed_provider_liquidation.rs`
- `inv_020_partial_observation_routes.rs`
- `inv_020_repeated_active_rewards.rs`
- `inv_020_reward_payout_rollback.rs`
- `inv_020_reward_recipient_liquidation.rs`
- `inv_020_selected_provider_assignment.rs`

Documentation: `tests/invariants/README.md` and this report. Production source,
engine pin, dependency manifests/locks and machine verdict tables are unchanged.

## Commands and Results

From the isolated clone, fresh builds used:

```sh
env CARGO_TARGET_DIR=/dev/shm/lane5-observation-sbf CARGO_BUILD_JOBS=4 cargo build-sbf --tools-version v1.52 --sbf-out-dir /tmp/percolator-lane5-observation-20260915/target/deploy -- --locked --offline
env CARGO_TARGET_DIR=/dev/shm/lane5-observation-sbf CARGO_BUILD_JOBS=4 cargo build-sbf --tools-version v1.52 --manifest-path tests/fixtures/auth_matcher/Cargo.toml --sbf-out-dir /tmp/percolator-lane5-observation-20260915/tests/fixtures/auth_matcher/target/deploy -- --locked --offline
env CARGO_TARGET_DIR=/dev/shm/lane5-observation-sbf CARGO_BUILD_JOBS=4 cargo build-sbf --tools-version v1.52 --manifest-path tests/fixtures/hostile_matcher/Cargo.toml --sbf-out-dir /tmp/percolator-lane5-observation-20260915/tests/fixtures/hostile_matcher/target/deploy -- --locked --offline
env CARGO_TARGET_DIR=/dev/shm/lane5-observation-host CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
```

Host binaries are under `/dev/shm/lane5-observation-host/debug/deps/`:
`v16_cu-8c7475d0975fcbd3` and
`v16_program_fuzz_regressions-81682abeb647f5d7`. Direct executions set
`PERCOLATOR_FUZZ_SBF` to the wrapper artifact above. The baseline command added
`RUST_BACKTRACE=1` and ran the CU binary with
`inv_020_ --test-threads=3 --nocapture`: **67 passed, exactly 14 failed**.
After the fixture updates the same filter reports **81 passed, zero failed**.
The largest observed transaction among the repaired selectors is 740,825 CU;
all original per-call ceilings are retained.

Final validation command, with the same host environment and wrapper path:

```sh
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions -- inv_020_ inv_053_ inv_061_ inv_079_ v16_attack_liquidation_cannot_omit_fresh_external_rescue_observation v16_program_mark_writer_and_trade_exit_composition_is_source_complete --test-threads=3 --nocapture
git diff --name-only -- '*.rs' | xargs rustfmt --edition 2021 --config skip_children=true --check
git diff --check
git diff --name-only e1394241 -- src Cargo.toml Cargo.lock tests/invariants/invariant_status.tsv tests/invariants/open_findings.tsv tests/invariants/coverage_reopenings.tsv
```

Logs are archived under the isolated clone's `target/lane5/` (not committed).
`lane5-baseline.log` records the fourteen failures, `lane5-updated.log` the first
81/81 run, `lane5-final.log` the final **111 CU and 19 public-SBF passes**, and
`lane5-mutation.log` the three expected sensitivity failures. Build logs and the
exact mutation patch are retained alongside them. The final run includes the
adjacent source, provenance, health, rescue and metadata checks. Formatting and
whitespace checks pass; the production/machine-verdict diff command is empty.

## Sensitivity and Limits

A separate disposable worktree at the same base,
`/dev/shm/lane5-observation-mutation`, disables only the Hybrid
`last_good_oracle_slot != now_slot` rejection. Its separately built wrapper makes
the following three updated selectors fail because old-report calls succeed:
the chunked observation selector, generated-current-Hybrid selector and
post-payout rollback selector. The normal wrapper passes all three. This is a
guard-sensitivity experiment, not a historical exploit or production fix.
The disposable source change was then restored. Its retained artifact SHA-256 is
`380a63e66565a84b3e25c425542ae06aca4385a86aca4d4dbb69654291a93267`.
The experiment used:

```sh
git worktree add --detach /dev/shm/lane5-observation-mutation e1394241e46ba1d66e21b43f6c1c99d83761dcca
# In the disposable worktree, replace only the current-slot predicate with false.
env CARGO_TARGET_DIR=/dev/shm/lane5-observation-sbf CARGO_BUILD_JOBS=4 cargo build-sbf --tools-version v1.52 --sbf-out-dir /dev/shm/lane5-observation-mutation/target/deploy -- --locked --offline
env PERCOLATOR_FUZZ_SBF=/dev/shm/lane5-observation-mutation/target/deploy/percolator_prog.so /dev/shm/lane5-observation-host/debug/deps/v16_cu-8c7475d0975fcbd3 v16_program_generated_current_hybrid_recipient_routes_match_full_health_and_rollback v16_program_chunked_mixed_observations_gate_full_refresh_and_preserve_claims v16_program_observation_abort_restores_liquidation_reward_payout_and_intent --test-threads=3 --nocapture
```

The fourteen baseline failures do not demonstrate fresh-evidence rejection,
extractable loss or absence of a bounded continuation. No new security severity
is assigned. Row **426 remains COVERED**; rows **416/422 remain OPEN**, and INV-020
retains **OPEN_EVIDENCE / SAMPLED**. Restoring these finite histories does not
prove arbitrary provider, funding, policy, recipient or terminal-history products.
The recreated-counterparty INV-058 failure and two INV-077 Hybrid maximum-shape
construction failures in Lane 4 are outside this lane and are not claimed fixed.
