# Row 422: INV-045 Reward-Provenance Conformance Audit

Date: 2026-09-16. Base: `codex/astra-invariant-cycle-20260915` at
`5bde2f8191961921803ce3ae6aa81c6d1b350cff`.
This is a PR135 invariant-conformance coverage audit.
**Row 422 remains `missing` in `open_findings.tsv`, OPEN in
`coverage_reopenings.tsv`, and INV-045 remains `REFUTED_CURRENT`.**

## Decision and Missing Boundary

The existing tests demonstrate bounded reward accounting and public-route
conformance. They do not supply finding-blind evidence for the reopening's
property:
`liquidation-reward-provenance-follows-the-effective-price-until-catchup`.
No additional Rust is warranted merely to repeat these already covered cells.

The missing boundary is the **independent eligibility oracle composed with the
paid-origin/fresh-report public history**:

- Carry the origin of the price actually consumed by liquidation through paid
  discovery, a later authenticated target, partial movement, target replacement
  and eventual catchup. Derive whether the selected episode's penalty is
  reclaimable from that history, independently of the wrapper's current
  freshness/reclaimability predicate or a scenario's designated fresh phase.
- Compose that oracle with public trade, observation and liquidation orderings,
  including a fresh target arriving while the effective price still lags.
  Reconcile any resulting keeper receipt and source-domain credit separately
  from old discovery fees and retained penalties, through committed custody.
  Existing rollback and payout helpers can support this work.
- Supply an invariant-owned executable discovery and a minimized public trace
  if a mismatch is found. Map the invariant fingerprint to the benchmark only
  afterward; row identity must not determine generated actions or expectations.

This is not a demand for an unbounded whole-system proof before a benchmark
mapping. A finite finding-blind discovery can qualify. The present gap is that
the reviewed histories do not independently decide the disputed provenance
transition, even where their fee arithmetic and receipt bookkeeping are
independent. No evidence here establishes that every fresh-report reward during
lag is wrong, or that the historical finding is nonqualifying.

The closest existing discovery is
`stateful/inv_045_no_free_mark_movement.rs::v16_program_mark_mode_route_matrix_keeps_liquidation_penalties_nonreclaimable`,
mapped to row 280 in `independent_discoveries.tsv`.
Its helper, `tests/support/invariant_discovery.rs::discover_one_trade_driven_liquidation`,
explicitly keeps the Hybrid feed stale through liquidation. Its zero-reward,
zero-domain-credit and terminal coalition-value checks therefore do not decide
the fresh-report handoff. The INV-045 discovery roster has no row-422 mapping.

Conversely,
`cu/inv_045_authenticated_reward_handoff.rs::run_authenticated_handoff`
computes `reward = penalty * share / 10000` in its designated fresh phase while
the accepted price is still behind the target. It independently prices the fee
and checks custody, but does not derive that phase's eligibility from a separate
model of effective-price lineage. The same distinction applies to the fixed
fresh episodes in the destination, maintenance, CPI and terminal children.

The source-composition selector
`v16_program_mark_writer_and_trade_exit_composition_is_source_complete`
checks selected-asset attribution, complete observations, fee/reward ordering,
the retained-fee cap and the production reclaimability predicate. It protects
source structure; it is not an independent behavioral oracle for that predicate.

## Existing Evidence

The audit inspected `open_findings.tsv`, the README, every
`cu/inv_045*reward*.rs` owner, the three existing `row422*.md` reports,
and the lane 12/23/25 reward notes. It also traced the INV-045 discovery adapter,
the public account builders and the source-composition guard.

The following selected owners are rerun in this audit. Counts below are each
test's bounded scope, not additional independent discoveries.

| Owner | Public-route conformance already covered |
| --- | --- |
| [Lane 12 recipient provenance](cu/inv_045_hybrid_recipient_provenance.rs) | Two selectors, 32 worlds each: dual-Hybrid source/recipient trajectories, both directions and settlement orders; newer same-slot target replacement/restoration around recipient reduction; exact fee, earned reward, own-position PnL, rollback and SPL exit. |
| [Lane 23 clipped maintenance](cu/inv_045_clipped_reward_maintenance.rs) | Eight worlds: collection before/after receipt, shared/separate ownership and two maintenance shares. Independent keeper book distinguishes collected fees, forgiven debt, self-rebates, two liquidation receipts and canonical versus source budgets. |
| [Lane 25 destination retry](cu/inv_045_reward_destination_retry.rs) | Four worlds: independently present/omitted destinations at two liquidations, malformed-account rollback, catchup, replay and exact keeper payout. An omitted share is retained source insurance, not a deferred receipt. |
| [CPI provenance](cu/inv_045_cpi_reward_provenance.rs) | Two selectors, sixteen ordered trade-route pairs each: paid discovery, stale retained penalty, clipped maintenance, fresh rewarded/omitted episode, matcher-context succession, post-ADL reduction and SPL payout. |
| [Native terminal reward](cu/inv_045_native_reward_terminal.rs) | Sixteen worlds: four recipient schedules, early/resolved native payment and both redemption orders; sync, unwrap, recreated custody, complete cohort payout and negative replay. No captured payout snapshot or successful top-up claim. |
| [Slab closure](cu/inv_045_reward_slab_closure.rs) | Four worlds: reward present/omitted and keeper deletion first/last. One lagged liquidation proceeds through frozen-price redemption, all portfolio deletions, unsigned insurance payout, discovery-fee burn, surplus return and final closure. |
| [Source composition](cu/inv_045_no_free_mark_movement.rs) | Static mark-writer/trade/crank attribution and ordering guard; counted separately from the eight executable economic selectors. |

Earlier reports retain their exact scopes and historical validation results:
[lane 12](lane12_hybrid_reward_provenance_20260915.md),
[lane 23](lane23_hybrid_reward_provenance_20260916.md),
[lane 25](lane25_reward_destination_retry_20260916.md),
[CPI](row422_cpi_reward_provenance_20260916.md),
[native](row422_native_terminal_reward_20260916.md), and
[closure](row422_reward_slab_closure_20260916.md).

The remaining reviewed reward owners cover accepted-price/catchup arithmetic,
authenticated handoff and funding, policy succession, unclipped maintenance,
classic SPL cohort redemption, frozen-destination retry, and simultaneous
paid/authenticated assets. In particular, `inv_045_cross_asset_reward_isolation.rs`
already checks two nonzero liquidation penalties with different provenance,
and `inv_045_frozen_reward_payout.rs` already checks thaw/retry after partial
payment. Those dimensions are not absent from the suite. These owners were
source-reviewed, not all rerun or promoted by this audit.

Older `accepted_price_reward` and `reward_catchup_order` use
`V16CuEnv::new_with_init_params`, `create_portfolio` and `deposit`;
those fixture helpers install account/token state directly. Their arithmetic
assertions are not evidence of a wholly public economic construction. The eight
economic selectors selected above instead use public economic builders.

### Public Construction and Limits

Classic SPL construction follows `inv018_public_spl_market_with_params` and
`trade_origin_catchup::fund`: System account creation, SPL mint/ATA creation,
public minting, wrapper initialization and deposits. Matcher contexts are
System-created and publicly initialized. Native construction uses the existing
native mint genesis fixture plus public wrapping and deposits. Program loading,
signer SOL, Clock and external Pyth reports are harness inputs; these selected
histories do not inject or restore Percolator economic state.

Successful liquidation uses valid accounts. Malformed destinations and invalid
suffixes are negative controls whose prefixes are independently simulated;
complete tracked/compiled Account frames roll back apart from the payer's exact
signature fee. Selected helpers also check signatures and packet size. The
stock/reservation/certificate censuses and token/lamport endpoints validate
accounting, but do not independently determine reward eligibility. Liquidation
quantity is observed from deployed open-interest changes; fee rounding is
independent, liquidation sizing is not.

The union already includes CPI switching, native redemption, clipped fees and
classic SPL final closure. Their existence must not be confused with a single
history covering every combination. Snapshot-backed reward top-ups, broader
provider/recipient histories and maximum-shape reward composition retain their
own limits; adding another such fixed cell would not supply the missing oracle.
This slice is executable and is not blocked by an engine or artifact dependency.
There is no new behavior finding, production fix or severity reclassification.

## Validation

Worktree: `/dev/shm/percolator-pr135-inv045-row422-20260916-7f4c`.
Branch: `codex/pr135-inv045-row422-20260916-7f4c`.
Engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

The private host cache was copied without hardlinks from
`/dev/shm/percolator-row416-oracle-funded-target/debug` into the target below.
Builds and logs use private `/tmp` space because `/dev/shm` is nearly full.
The wrapper and matcher are private copies of the unchanged lane-24 artifacts;
no SBF rebuild is claimed. Production, Cargo files and fixture sources match
`d809e9a563d9b8bf38f32648b32a15d75f526ec8` exactly.

| Artifact | SHA-256 |
| --- | --- |
| `artifacts/percolator_prog.so` | `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f` |
| `artifacts/auth_matcher.so` | `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93` |

Run from the isolated worktree:

```bash
export CARGO_TARGET_DIR=/tmp/pr135-inv045-row422-20260916-7f4c/target
export TMPDIR=/tmp/pr135-inv045-row422-20260916-7f4c/tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/tmp/pr135-inv045-row422-20260916-7f4c/artifacts/percolator_prog.so
export PERCOLATOR_ROW422_MATCHER_SBF=/tmp/pr135-inv045-row422-20260916-7f4c/artifacts/auth_matcher.so
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::hybrid_recipient_provenance::v16_program_dual_hybrid_reward_lineage_survives_recipient_routes_and_payout \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::hybrid_recipient_provenance::v16_program_same_slot_hybrid_target_replacements_preserve_earned_reward_and_exit \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::reward_maintenance_catchup::clipped_reward_maintenance::v16_program_clipped_self_maintenance_keeps_hybrid_rewards_and_shared_owner_sources_exact \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::reward_destination_retry::v16_program_hybrid_reward_destination_retries_preserve_only_committed_receipts \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::cpi_reward_provenance::v16_program_cpi_switch_retained_penalty_and_clipped_reward_normalize \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::cpi_reward_provenance::v16_program_cpi_switch_omitted_rewards_cannot_be_reclaimed \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::reward_terminal_redemption::native_reward_terminal::v16_program_native_hybrid_omitted_rewards_stay_unclaimed_through_sync_unwrap_and_terminal_replay \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::reward_terminal_redemption::reward_slab_closure::v16_program_hybrid_reward_provenance_survives_portfolio_deletion_and_slab_closure \
  inv_045_no_free_mark_movement::v16_program_mark_writer_and_trade_exit_composition_is_source_complete
```

The host test binaries were compiled from this worktree. The nine exact
`v16_cu` selectors passed: **9 passed, 0 failed, 1,472 filtered out**, 97.12
seconds. The eight economic selectors cover 128 histories, 188 liquidations and
1,632 complete Account rollback checks. Peak measured transaction CU was
370,185; slab closure peaked at 38,777 CU. All existing CU assertions passed.
CU can vary with generated account addresses. Log:
`/tmp/pr135-inv045-row422-20260916-7f4c/tmp/reward-conformance.log`.

The ledger/docs checks used the same environment:

```bash
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_dated_open_security_finding_benchmark_is_non_overclaiming \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
git diff --check
git diff --exit-code 5bde2f81 -- src Cargo.toml Cargo.lock tests/fixtures \
  tests/invariants/cu tests/invariants/stateful tests/invariants/public_sbf \
  tests/invariants/invariant_status.tsv tests/invariants/independent_discoveries.tsv
diff -u <(git show 5bde2f81:tests/invariants/open_findings.tsv | sed '/^#/d') \
  <(sed '/^#/d' tests/invariants/open_findings.tsv)
diff -u <(git show 5bde2f81:tests/invariants/coverage_reopenings.tsv | sed '/^#/d') \
  <(sed '/^#/d' tests/invariants/coverage_reopenings.tsv)
```

Metadata results: **1 passed, 2 failed, 120 filtered out** (0.01 seconds;
Cargo exit 101). Machine-status consistency passed. The two failures are
pre-existing row-421 reconciliation drift at base `5bde2f81`:

- `v16_dated_open_security_finding_benchmark_is_non_overclaiming`, line 775:
  the ledger has five missing rows, while the assertion still expects six.
- `v16_post_pr135_counterexamples_reopen_every_affected_invariant`, line 2117:
  the actual independently discovered OPEN set is
  `{411, 420, 421, 423, 424, 433}`; the expected set omits 421.

The unchanged guard source and both ledgers' non-comment lines compare exactly
with the base; this audit adds comments only. That source/data identity establishes
the failures predate this slice; no separate baseline execution is claimed.
Log: `/tmp/pr135-inv045-row422-20260916-7f4c/tmp/ledger-guards.log`.
The unrelated guard reconciliation is left for its owner. It does not prevent
execution of INV-045 coverage and does not change row 422 to dependency-blocked.

Whitespace, protected-source and ledger-data identity checks pass. Existing host
dead-code and Solana future-compatibility warnings remain. No broad suite was
run. Changes are limited to this report, a README entry and commentary beside
row 422 in the two ledgers; no Rust or production code changes are included.
