# Lane 12: same-slot Hybrid reward provenance (2026-09-15)

## Scope and verdict

Fresh base: `origin/codex/astra-invariant-cycle-20260915` at
`4e76552eeaf7d137398bfd87520f2d0ca220460a`; pinned engine
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`. Local branch:
`codex/lane12-hybrid-reward-provenance-20260915`, isolated clone:
`/tmp/percolator-lane12-20260915-yfrNT4`.

**Row 422 remains OPEN (`missing`); INV-045 remains `REFUTED_CURRENT`.**
This is bounded public-route conformance, not an independent discovery of the
missing finding or an invariant closure. Production sources, manifests, locks,
and all machine status files are unchanged. The original working directory was
not used for builds or edits. No GitHub PR body or diff was inspected.

Owner: [cu/inv_045_hybrid_recipient_provenance.rs](cu/inv_045_hybrid_recipient_provenance.rs),
under `inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff`.
Only this existing Rust owner, this report and a short README section change.

## Local coverage review

The classification and reachability rules come from `scripts/loop.md`; the row
and invariant dispositions come from `open_findings.tsv`, `invariant_status.tsv`
and `tests/invariants/README.md`. Relevant existing owners were compared before
choosing the increment:

| Existing owner | Covered history | Increment here |
| --- | --- | --- |
| `inv_045_authenticated_reward_handoff.rs` | Paid discovery followed by fresh evidence while effective price lags; conflicting reports; funding | Independent source and recipient targets after an earned reward |
| `inv_045_retained_penalty_handoff.rs`, `inv_045_paid_origin_hybrid_recipient.rs` | Retained fallback penalty, later authenticated liquidation, four discovery routes and dual-Hybrid recipient | Different same-slot target values interleaved with a recipient reduction |
| `inv_045_corroborated_mark_fees.rs` | Caught-up paid mark, new liquidation fee, common trader/keeper ownership | Two assets with distinct target and recipient price provenance |
| `inv_045_interleaved_cap_carry.rs` | Repeated same-target reports, four paid trade routes, fractional carry, later liquidation | Actual target replacements following liquidation, then SPL reward withdrawal |
| `inv_045_hybrid_recipient_provenance.rs` | Two Hybrid trajectories, source-domain attribution, recipient PnL and payout | Separate newer reports replace each target and restore it around a reduction |
| Stateful/public-SBF INV-045 owners | EWMA/AuthMark/Hybrid route and pending-target matrices, paid fee retention and terminal coalition controls | Reuse the existing dual-Hybrid LiteSVM fixture instead of duplicating those matrices |

The addition exercises INV-045 with adjacent INV-020 report identity/freshness,
INV-024/036 domain and reward attribution, INV-041 settlement ordering and
INV-061 bounded liquidation. It does not add a new INV-062 ownership product.

## Executable evidence

The existing selector
`v16_program_dual_hybrid_reward_lineage_survives_recipient_routes_and_payout`
failed on the untouched base with `InstructionError(2, Custom(22))`
(`EngineNonProgress`) in `World::current`: the second-slot recipient health
refresh reused a prior-slot report. The corrected continuation first requires
that old report to roll back exactly, then supplies renewed reports. Subsequent
market-only catchup may still use the retained report. Oracle age tolerance is
not treated as current-slot health evidence. This is fixture maintenance, not a
production defect.

The new selector
`v16_program_same_slot_hybrid_target_replacements_preserve_earned_reward_and_exit`
reuses the same construction and independent accounting. Its 32 worlds cross
both source asset IDs, both price directions, publication before/with liquidation,
recipient settlement before/after reward, and both observation/reduction orders.
After a nonzero partial liquidation, each asset receives a newer, different
target in the same slot. A recipient reduction separates those replacements
from another pair of reports restoring the original targets. Both single and
batch no-CPI reductions execute in each world.

Assertions include:

- Each same-slot replacement preserves effective price, EWMA mark, fractional
  capacity and open interest. Only its own oracle profile changes; target price,
  provider price, publication time and observation slot match the supplied input.
- Every exposed portfolio, including the liquidated owner and credited keeper,
  is byte-identical across market-only replacement. Mint, vault and all SPL
  destinations are also byte-identical. The independent source/reservation and
  current-health censuses run after every measured transaction.
- Liquidation fees use closed quantity and the committed source price, with
  explicit inequalities against the raw source target and recipient prices.
  Only the configured share becomes keeper capital. The retained remainder
  belongs to the source asset's two insurance domains.
- Price expectations after replacement are calculated from input anchors and
  elapsed slots, not copied from observed output. A replacement checkpoints the
  first accepted price: subsequent movement uses that anchor, while its own
  slot contributes zero movement. The two-lot/one-lot recipient PnL schedule
  independently determines its remaining capital and final SPL payout.
- Four successful replacement prefixes per new world are followed by a
  conflicting same-feed/same-time report. Exact error index/code, successful
  prefix logs, all tracked and transaction accounts, account absence and exact
  payer signature fees are checked on rollback. Existing liquidation-prefix
  and actual SPL-withdrawal-prefix rollback checks remain active.
- Final keeper capital and PnL are zero, the SPL payout is exact, insurance and
  budgets retain their attribution, and vault plus payout equals initial supply.
  The unsupplied liquidated account stays byte-identical after its paid episode.

## Validation

Builds use private `/dev/shm/lane12-20260915-{host,sbf}` targets. Dependency caches
were copied from existing local build directories; the wrapper was recompiled
from this clone with platform-tools v1.52 and default features.

- Wrapper SBF SHA-256:
  `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
- Auth matcher SBF SHA-256:
  `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

Results:

| Selection | Result |
| --- | --- |
| Original dual-Hybrid selector | PASS: 32 worlds, 112 exact rollbacks, 32 SPL-prefix rollbacks |
| Same-slot replacement selector | PASS: 32 worlds, 128 committed target replacements, 240 exact rollbacks, 32 SPL-prefix rollbacks |
| Both touched selectors | 2/2 PASS, 37.19 seconds; 64 rewarded liquidations; peak transaction CU 370,185 |
| Paid-origin dual-Hybrid control | PASS: eight discovery-route/publication-order worlds |
| Corroborated paid-mark control | PASS: four ownership/publication-order worlds |
| Interleaved cap/reward control | PASS: both route orders |
| Three adjacent selectors | 3/3 PASS, 10.69 seconds |
| INV-079 metadata/source guards listed below | 13/13 PASS, 0.72 seconds |
| Changed-file rustfmt and git diff checks | PASS |
| Repository-wide cargo fmt check | FAIL on six unchanged baseline files, listed below |

The branch SBF was built with:

```sh
env CARGO_TARGET_DIR=/dev/shm/lane12-20260915-sbf CARGO_BUILD_JOBS=4 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/lane12-20260915-sbf/deploy -- --locked
env CARGO_TARGET_DIR=/dev/shm/lane12-20260915-sbf CARGO_BUILD_JOBS=4 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --manifest-path tests/fixtures/auth_matcher/Cargo.toml --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked --offline
sha256sum /dev/shm/lane12-20260915-sbf/deploy/percolator_prog.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
```

Host commands below share this environment (each actual test command set these
variables explicitly with `env`):

```sh
export CARGO_TARGET_DIR=/dev/shm/lane12-20260915-host
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane12-20260915-sbf/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
cargo test --locked --offline --test v16_cu hybrid_recipient_provenance:: -- --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::retained_penalty_handoff::paid_origin_hybrid_recipient::v16_program_paid_origin_penalty_survives_dual_hybrid_recipient_catchup \
  inv_045_no_free_mark_movement::trade_origin_catchup::corroborated_mark_fees::v16_program_corroborated_paid_mark_only_distributes_new_liquidation_fees \
  inv_045_no_free_mark_movement::interleaved_cap_carry::v16_program_interleaved_trade_routes_preserve_oracle_cap_carry_and_reward_provenance \
  inv_079_public_reachability_evidence::v16_every_public_trace_consumer_validates_reachability_evidence \
  inv_079_public_reachability_evidence::v16_finding_blind_violation_oracle_evidence_roster_is_source_complete \
  inv_079_public_reachability_evidence::v16_retained_retry_terminal_dispositions_are_source_complete \
  inv_079_public_reachability_evidence::v16_program_open_lof_manifest_snapshot_is_structurally_honest \
  inv_079_public_reachability_evidence::v16_superseded_control_terminal_dispositions_are_source_complete \
  inv_079_public_reachability_evidence::v16_dated_open_security_finding_benchmark_is_non_overclaiming \
  inv_079_public_reachability_evidence::v16_public_instruction_coverage_registry_matches_production_roster \
  inv_079_public_reachability_evidence::v16_special_verification_method_registry_matches_charter \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant \
  inv_079_public_reachability_evidence::v16_program_invariant_harnesses_are_test_free_roots
rustfmt --edition 2021 --check tests/invariants/cu/inv_045_hybrid_recipient_provenance.rs
cargo fmt --all -- --check
git diff --check
git diff --exit-code HEAD -- src Cargo.toml Cargo.lock tests/invariants/open_findings.tsv tests/invariants/invariant_status.tsv
```

The unchanged original selector was first run at the base with:

```sh
cargo test --locked --offline --test v16_cu v16_program_dual_hybrid_reward_lineage_survives_recipient_routes_and_payout -- --nocapture --test-threads=1
```

Local logs: `/dev/shm/lane12-baseline.log`,
`/dev/shm/lane12-dual-hybrid-final.log`,
`/dev/shm/lane12-controls-guards.log` and
`/dev/shm/lane12-format-all.log`. The host build emits existing dead-code and
Solana future-compatibility warnings.

The six whole-repository formatting failures are:

- `cu/inv_024_terminal_reserve_destination_recovery.rs`
- `cu/inv_070_native_residue_disposition.rs`
- `cu/inv_073_dual_quote_reserve_progress.rs`
- `cu/inv_073_frozen_reserve_replacement.rs`
- `cu/inv_073_native_recredit_custody.rs`
- `cu/inv_073_terminal_public_reserves.rs`

A `git diff --exit-code HEAD --` check of those six files passed. Formatting
churn from the initial whole-repository formatter was removed; none is included
in this commit.

## Remaining gaps

This increment is a finite two-asset, five-portfolio, four-slot history with
zero funding and maintenance, integral lots, classic SPL custody and one
rewarded liquidation. Reports change after that liquidation; it does not
certify arbitrary target reversals before health refresh, multiple competing
reward recipients, renewed liquidation episodes, arbitrary report interleavings,
fee-policy succession, co-owned counterparties, CPI recipient reductions,
maximum shape, native custody, or whole-cohort terminal redemption. The target
and counterparties do not all exit. Earlier paid-discovery, funding and terminal
owners retain their distinct scopes. No status is promoted by these passing
samples, and no source guard is presented as an execution proof.
