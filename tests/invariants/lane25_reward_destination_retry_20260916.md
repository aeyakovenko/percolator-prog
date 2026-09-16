# Lane 25: optional Hybrid reward destinations and payout retries

## Provenance and disposition

- Clone: `/tmp/percolator-inv045-provenance-20260916`.
- Branch: `codex/inv045-destination-rollback-20260916`.
- Base: `63cc6f284674f31187430f52d43a201b21b95d50`, from
  `/tmp/percolator-astra-invariant-cycle-20260915-run`, branch
  `codex/astra-invariant-cycle-20260915`.
- Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- The first full `--no-hardlinks` clone ran out of filesystem space. Git removed
  the failed destination; a shallow `file://` clone of the requested branch
  succeeded. All source reads after cloning, edits and builds use this clone.
  `/home/anatoly/percolator-prog` was not used. No open fix was fetched or merged.
- Changes are confined to `tests/invariants/**`: a new INV-045 child, its parent
  module registration, the README entry and this report. No push.
- **No public-route LoF, persistent DoS or required-progress CU bug was found.**
  This is bounded conformance, not an independent-discovery or generic-closure
  claim. **Row 422 remains OPEN/missing; INV-045 remains `REFUTED_CURRENT`.**
  All production, dependency, fixture and machine-status files remain unchanged.

## Boundary and non-overlap

Primary owner:
[cu/inv_045_reward_destination_retry.rs](cu/inv_045_reward_destination_retry.rs),
mounted below `inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff`.
Adjacent checks concern INV-017/020/024/036/041/061/080/081; no status is promoted.

The optional reward destination is not required for a valid liquidation. Its
omission commits an unrewarded liquidation and the entire reclaimable penalty
enters the source domains. A malformed program-owned destination rejects. The
new product composes these two outcomes with repeated liquidation, subsequent
destination presence, full price catchup and actual keeper payout. It would
detect paying a skipped share later, reclaiming paid discovery fees as a reward,
assigning an omitted share to the wrong budget, charging at the fresh raw target
instead of the accepted price, or retaining a receipt after a failed transaction.

| Existing coverage read | New relation |
| --- | --- |
| INV-080 `v16_attack_hybrid_liquidation_bad_reward_tail_rolls_back_oracle_update`, INV-017 wrong-owner control | These existing account-validation controls do not compare committed absent/present destination schedules or follow omitted shares into domain stock and public payout. Rejection alone is not the contribution here. |
| `inv_045_authenticated_reward_handoff.rs` and `inv_045_retained_penalty_handoff.rs` | Existing histories always provide a valid destination for the measured fresh liquidations. Their missing tails are missing oracle evidence. This product keeps complete current evidence while independently omitting the optional destination. |
| Lane 12 / `inv_045_hybrid_recipient_provenance.rs` | No same-slot target replacement, recipient exposure reduction or dual-Hybrid price history is added. |
| Lane 15 / same owner | No competing recipients, matcher authorization, CPI or route switching is added. The keeper identity stays fixed. |
| Lane 23 / `inv_045_clipped_reward_maintenance.rs` | No maintenance collection, clipping, self-rebate or shared ownership is added. The new dimension is destination omission and its irreversible fee attribution. |
| `inv_045_reward_terminal_redemption.rs` | The source cohort remains Live. This is exact keeper SPL payout, not terminal cohort redemption. |
| `inv_045_reward_policy_catchup.rs` | The 3,333-bps policy is fixed throughout. No policy succession or early/late withdrawal product is repeated. |
| `inv_045_exposed_keeper_provenance.rs`, paid-origin recipient/routes, selected-asset and reward-catchup controls | No recipient position, settlement-order, hint-order, discovery-route or elapsed-catchup matrix is added. The price trajectory supplies two distinct liquidation episodes for the destination boundary. |

The survey included the row 422 README sections, current `open_findings.tsv`,
`invariant_status.tsv`, `coverage_reopenings.tsv`, `scripts/loop.md`, lane 12/15/23
reports, and the existing INV-045 owners above. The requested exclusions are
controls and non-overlap evidence, not claims of closure.

## Public history and independent oracle

System, SPL Token, ATA and wrapper instructions construct every economic account,
including the uninitialized program-owned destination used for malformed-input
checks. The test does not inject or restore program-account bytes. LiteSVM provides
program loading, wallet SOL, Clock and external Pyth reports. Mint authority is
revoked after the fixed 125,101,000-atom endowment is deposited into custody.

At slot 1 a 100-lot target/peer pair opens at 1,000,000. At slot 5 a separate
one-lot pair pays 1,540,072 discovery atoms to stage the Hybrid mark at 992,320.
Fresh reports at slots 6/7/14 produce accepted prices 997,600 / 995,206 / 980,000.
Two liquidations occur before full catchup. The independent oracle calculates:

```text
penalty = ceil(ceil(closed_quantity * accepted_price / POS_SCALE) * 5 / 10000)
eligible_share = floor(penalty * 3333 / 10000)
receipt = eligible_share if this liquidation has a destination, otherwise 0
source_domains += [floor((penalty - receipt)/2), ceil((penalty - receipt)/2)]
insurance = paid_discovery + sum(penalty) - sum(receipt)
keeper_payout = original_keeper_principal + sum(receipt)
```

Closed quantity is observed from deployed OI, not an independent liquidation-sizing
proof. The test independently distinguishes each penalty from prices based on the
entry, raw report, old paid target, accepted discovery print and submitted print.
Episodes `(quantity, penalty, eligible_share)` are `(12001223, 5987, 1995)` and
`(16653247, 8287, 2762)`. All four schedules must have identical episodes and
target/peer entitlements, with only the receipt allocation changing.

| Destination at first/second liquidation | Receipts | Skipped shares | Source domains | SPL payout |
| --- | ---: | ---: | --- | ---: |
| absent / absent | 0 | 4757 | 7136 / 7138 | 1000 |
| absent / present | 2762 | 1995 | 5755 / 5757 | 3762 |
| present / absent | 1995 | 2762 | 6139 / 6140 | 2995 |
| present / present | 4757 | 0 | 4758 / 4759 | 5757 |

The full skipped share becomes source insurance, not a deferred keeper claim.
Every phase retries with both absent and present destinations after completion;
both reject as `EngineNonProgress`. At slot 14 a valid destination is present in
every history, but actual catchup cannot create another fee or reward. Discovery
insurance remains completely outside domain entitlement. Per-event integer domain
rounding is checked independently, including the odd first penalty and reward.

Across each target crank, the three unrelated exposed portfolios and the complete
SPL mint/vault Accounts are unchanged. Reward credit preserves keeper PnL and legs;
unrewarded or non-liquidating cranks preserve the entire keeper state. Independent
stock, reservation, source-credit and current-certificate censuses run throughout.
Peer settlement completes before each next slot. All unused domains and provider
earnings remain zero. Final account claims plus insurance leave the same four-atom
live rounding residual in all worlds; custody plus actual payout equals fixed supply.

There are **72 complete Account rollback checks**:

- 20 successful crank prefixes followed by an uninitialized reward destination,
  including eight liquidation prefixes (four rewarded and four omitted).
- 24 standalone wrong-owner, target-alias and read-only destination failures at
  certified liquidation boundaries, before the identical valid call commits.
- 24 post-completion retries, with and without a destination.
- Four successful SPL withdrawal prefixes followed by malformed-destination failure.

The shared submit helper independently simulates each successful prefix, verifies
transaction signatures and the 1,232-byte packet bound, and pins the rejecting
instruction index/error. It compares all tracked and compiled complete Accounts
including absence, allowing only the separate payer's exact signature fee.
The malformed destination remains unchanged, and a valid destination permits
bounded continuation after every rejection. Final successful SPL withdrawals use
the identical bytes that were rolled back; all owner endpoints reconcile.

## Build and validation commands

Commands run from the isolated clone unless stated otherwise. Wrapper SBF was
freshly built with default features and locked/offline dependencies. No other
lane's SBF or writable build cache was reused. The new selector and eight controls
do not need the matcher, but three INV-079 trace/classifier checks do. Its unchanged
fixture source was freshly compiled; the clone's ignored artifact path links to
that build. Wrapper SHA-256:
`e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
Matcher SHA-256:
`50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
Logs: `/dev/shm/inv045-destination-20260916-logs`.

```bash
# From /tmp, after the failed full clone:
git clone --depth 1 --single-branch --branch codex/astra-invariant-cycle-20260915 file:///tmp/percolator-astra-invariant-cycle-20260915-run /tmp/percolator-inv045-provenance-20260916
cd /tmp/percolator-inv045-provenance-20260916
git switch -c codex/inv045-destination-rollback-20260916
mkdir -p /dev/shm/inv045-destination-20260916-logs /dev/shm/inv045-destination-20260916-tmp
env CARGO_TARGET_DIR=/dev/shm/inv045-destination-20260916-sbf CARGO_BUILD_JOBS=4 TMPDIR=/dev/shm/inv045-destination-20260916-tmp RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/inv045-destination-20260916-sbf/deploy -- --locked
# Run after the initial INV-079 attempt exposed the missing artifact:
env CARGO_TARGET_DIR=/dev/shm/inv045-destination-20260916-auth-sbf CARGO_BUILD_JOBS=4 TMPDIR=/dev/shm/inv045-destination-20260916-tmp RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --manifest-path tests/fixtures/auth_matcher/Cargo.toml --sbf-out-dir /dev/shm/inv045-destination-20260916-auth-sbf/deploy -- --locked
mkdir -p tests/fixtures/auth_matcher/target/deploy
ln -s /dev/shm/inv045-destination-20260916-auth-sbf/deploy/auth_matcher.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
```

The following exports express the environment supplied with `env` on every host
invocation. Tests are scoped by exact selector; no unfiltered suite is run.

```bash
export CARGO_TARGET_DIR=/dev/shm/inv045-destination-20260916-host
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export TMPDIR=/dev/shm/inv045-destination-20260916-tmp
export PERCOLATOR_FUZZ_SBF=/dev/shm/inv045-destination-20260916-sbf/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
cargo test --locked --offline --test v16_cu inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::reward_destination_retry::v16_program_hybrid_reward_destination_retries_preserve_only_committed_receipts -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::v16_program_paid_discovery_fresh_handoff_authenticates_liquidation_and_keeper_exit \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::retained_penalty_handoff::v16_program_retained_stale_penalty_survives_fresh_liquidation_and_catchup \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::reward_policy_catchup::v16_program_reward_policy_succession_preserves_receipts_and_effective_price_catchup \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::reward_terminal_redemption::v16_program_hybrid_catchup_rewards_survive_resolved_cohort_redemption_order \
  inv_045_no_free_mark_movement::trade_origin_catchup::v16_program_trade_origin_liquidation_prices_and_entitlements_survive_catchup_order \
  inv_080_error_propagation_and_exact_rollback::v16_attack_hybrid_liquidation_bad_reward_tail_rolls_back_oracle_update \
  inv_045_no_free_mark_movement::v16_program_mark_writer_and_trade_exit_composition_is_source_complete \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::reward_maintenance_catchup::clipped_reward_maintenance::v16_program_clipped_self_maintenance_keeps_hybrid_rewards_and_shared_owner_sources_exact
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_public_trace_schema_detects_out_of_band_economic_mutation \
  inv_079_public_reachability_evidence::v16_public_trace_terminal_classifier_requires_complete_economic_evidence \
  inv_079_public_reachability_evidence::v16_public_terminal_classifier_exhausts_normalized_outcome_space \
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
# Retry only the three artifact-preflight failures after the matcher build:
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_public_trace_schema_detects_out_of_band_economic_mutation \
  inv_079_public_reachability_evidence::v16_public_trace_terminal_classifier_requires_complete_economic_evidence \
  inv_079_public_reachability_evidence::v16_public_terminal_classifier_exhausts_normalized_outcome_space
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_045_authenticated_reward_handoff.rs tests/invariants/cu/inv_045_reward_destination_retry.rs
git diff --check
git diff --cached --check
git diff --exit-code 63cc6f284674f31187430f52d43a201b21b95d50 -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/**/*.tsv' ':(glob)tests/invariants/*.tsv'
git diff --exit-code 63cc6f284674f31187430f52d43a201b21b95d50 -- . ':(exclude)tests/invariants/**'
```

## Results and limits

| Check | Result |
| --- | --- |
| Fresh wrapper SBF build | PASS, exit 0, 26.56s |
| Fresh matcher SBF build | PASS, exit 0, 10.92s |
| Both host test targets, compile only | PASS, exit 0, 1m07s |
| New exact selector, initial and final | PASS, 1/1 each, 3.04s each |
| Eight exact adjacent controls | PASS, 8/8, 28.27s |
| Initial INV-079 invocation | 13 registry/source guards PASS; three trace/classifier tests FAIL at missing-matcher artifact preflight, exit 101 |
| Three INV-079 trace/classifier checks after fixture build | PASS, 3/3, 2.19s; all 16 selected INV-079 checks now have passing results |
| Scoped rustfmt check | PASS |
| Unstaged/staged Git whitespace checks | PASS |
| Protected-file and all-outside-scope diffs against base | PASS, empty |

Both exact new-selector executions cover four histories, eight liquidations,
72 exact rollbacks and four payouts. The final execution additionally asserts
the exact rollback count and all owners' payout-stage entitlements. No failing
public protocol trace was found. A source read before the first execution
corrected the expected malformed-account error to `NotInitialized`; this was
not a runtime failure. The three INV-079 setup failures were resolved by building
the unchanged matcher source, without altering fixtures or test expectations.
All 25 distinct selected tests have passing results; no full-suite claim is made.

Measured new-selector peaks: crank **319,613 CU**, rejected transaction
**345,500 CU**, SPL payout **52,088 CU**. Their limits are respectively
325,000 / 650,000 (two-instruction bundle) / 300,000 CU. Existing per-operation
guards remain enabled. The host build emits existing dead-code and Solana future-compatibility
warnings.

This finite product has one Hybrid source, one flat keeper, distinct owners,
zero funding and maintenance, fixed reward policy, two downward fresh-report
liquidations and three observation slots. It does not prove generic Hybrid
provenance persistence, retained stale-penalty composition, CPI/no-CPI switching,
recipient succession, multiple providers/assets, native custody, maximum shape,
terminal cohort exit, or independent liquidation sizing. Source portfolios
remain live with a measured four-atom rounding residual. No vulnerable/fixed-pin
experiment or open-fix comparison was performed. Row 422 and all invariant
classifications remain unchanged.
