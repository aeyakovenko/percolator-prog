# Lane 23: clipped maintenance and Hybrid reward provenance

## Provenance and disposition

- Private clone: `/tmp/percolator-lane23-hybrid-reward-provenance-20260916`.
- Local branch: `codex/lane23-hybrid-reward-provenance-20260916`.
- Base: `d64f049005847848b095b3b8b2d21318d0504296`, cloned with
  `--no-hardlinks` from `/tmp/percolator-astra-invariant-cycle-20260915-run`,
  branch `codex/astra-invariant-cycle-20260915`.
- Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- The source checkout and `/home/anatoly/percolator-prog` were not edited or
  used as build targets. All edits, builds and the local commit are isolated.
- **No public-route LoF, persistent DoS or required-progress CU bug was found.**
  Tests/docs only; no red/green production fix or independent discovery claimed.
- **Row 422 remains OPEN/missing; INV-045 remains `REFUTED_CURRENT`.**
  Production, manifests, locks and machine status/coverage TSVs are unchanged.
  No push.

## Survey and non-overlap

The survey read the row 422 entry and its adjacent reward/catchup/maintenance
notes in `coverage_reopenings.tsv`, `scripts/loop.md`, the current status and
finding rows, existing INV-045 owners, INV-040's clipped-fee refill control,
INV-027's later admission control, and the lane 1-21 reports/README summaries.
The reopening explicitly leaves clipping/rewarded maintenance and shared-owner
recipient compositions outside its existing maintenance sample.

| Existing evidence | Lane 23 increment |
| --- | --- |
| `inv_045_reward_maintenance_catchup.rs` | Existing keeper stays solvent at collection and receives no maintenance rebate. Here the fee clips before actual Hybrid reward ingress, with nonzero self-rebates and independently rounded canonical budgets. |
| INV-040 clipped maintenance/refill and INV-027 refilled admission | Existing replenishment is an SPL deposit. Here two earned liquidation receipts replenish the account during Hybrid lag, retaining a separate source-price penalty and insurance-domain split. No first-risk admission is added. |
| `inv_045_exposed_keeper_provenance.rs` | Existing exposed AuthMark keeper has ample capital, zero maintenance and separate owners. This product uses a flat, fee-exhaustible keeper and shared ownership with the liquidated target. It makes no underwater-position claim. |
| `inv_045_reward_policy_catchup.rs`, authenticated handoff and paid-origin owners | Existing liquidation-policy, funding, report and discovery-transport products remain controls. This product fixes liquidation policy and varies only the init-time maintenance share and collection order. |
| `inv_045_corroborated_mark_fees.rs` | Existing common trader/keeper ownership follows caught-up paid marks. Here target/keeper ownership overlaps during two lagged receipts and clipped/rebated maintenance, with one shared ATA counted once. |
| `inv_045_reward_terminal_redemption.rs` | Existing whole-cohort resolution has zero maintenance. Lane 23 completes the keeper's live withdrawal, leaving the source cohort open. |
| Lane 12 | No same-slot target replacement, recipient reduction or dual-Hybrid trajectory. |
| Lane 15 | No competing liquidation recipients, CPI authorization or route switching. |
| Lanes 1, 4, 5, 6, 11, 14 | Retained consent, current observations, cumulative admission and funded authority changes do not exercise clipped maintenance before liquidation-reward ingress. No authority or admission product is repeated. |
| Lanes 2, 8, 10, 13, 17, 20 | Terminal receipts, reserve/beneficiary succession, custody and expiry remain separate. This history remains Live. |
| Lanes 3, 9, 18, 21 | Funding/pending-debt attribution, shutdown, native retirement and close-expiry preemption remain separate. No funding, close ledger, lifecycle transition or native custody is introduced. |
| Lanes 7, 16, 19 | Maximum shape and latent-domain/backlog realization remain separate. This history has two configured assets and five portfolios. |

## Public history and accounting

Owner: [cu/inv_045_clipped_reward_maintenance.rs](cu/inv_045_clipped_reward_maintenance.rs),
under INV-045's `trade_origin_catchup::authenticated_reward_handoff::reward_maintenance_catchup`.
Primary ownership is INV-045, with adjacent INV-024/036/040/041/062/088 checks.
System, SPL Token, ATA and wrapper instructions create every economic account.
LiteSVM supplies program loading, wallet SOL, Clock and external Pyth fixtures.
There is no program-owned byte injection or private engine transition.

The eight worlds cross independent/shared target-and-keeper ownership,
maintenance before/after the first liquidation, and shares 3,333/5,000 bps.
Asset 0 is the canonical maintenance destination; asset 1 is the Hybrid
liquidation source. Both start at 1,000,000. Paid discovery at slot 5 retains
1,540,072 atoms without domain budgets. Current authenticated reports at slots
6/7/14 yield effective prices 997,600 / 995,206 / 980,000. The first two stages
liquidate partially; full catchup produces no third reward.

For each closed source quantity `q`, the independent penalty oracle is
`P = ceil(ceil(q * effective_price / POS_SCALE) * 5 / 10000)` and the liquidation
reward is `R = floor(P * 3333 / 10000)`. It explicitly rejects prices based on
the entry, raw target, paid print and submitted discovery print. The episodes
are `(q, P, R) = (12015859, 5994, 1997), (16652956, 8287, 2762)`.
Thus source budgets are always `[4760, 4762]` and total rewards are 4,759.

The keeper starts with 101 atoms, no PnL, and a 160-atom per-slot maintenance
rate. Its book starts from these inputs, advances its own fee cursor, and
computes each collection as:

```text
due = 160 * (slot - previous_fee_slot)
charged = min(due, keeper_capital)
rebate = floor(charged * maintenance_share / 10000)
forgiven += due - charged
keeper_capital -= charged - rebate
canonical_budgets += [floor((charged-rebate)/2), ceil((charged-rebate)/2)]
```

Liquidation credit changes only recipient capital and certificate validity.
It cannot refresh the fee cursor, restore forgiven debt, change its PnL or
credit a co-owned target. Self-maintenance collection preserves the source
oracle/assets and all four other portfolio Accounts. Post-reward same-slot
retries preserve every tracked Account. The existing independent stock,
reservation, source-credit and health-certificate censuses accompany measured
economic transitions; every peer fee cursor must reach the phase's Clock.

| Maintenance Share | Collect First | Charged | Rebates | Forgiven | SPL Payout | Canonical Budgets |
| --- | --- | --- | --- | --- | --- | --- |
| 3333 | no | 2080 | 692 | 0 | 3472 | [4853, 4855] |
| 3333 | yes | 1381 | 459 | 699 | 3938 | [4620, 4622] |
| 5000 | no | 2080 | 1040 | 0 | 3820 | [4680, 4680] |
| 5000 | yes | 1381 | 690 | 699 | 4169 | [4505, 4506] |

Both ownership variants match each row exactly. Payout is
`101 + liquidation_rewards + maintenance_rebates - collected_maintenance`.
Collection order intentionally need not commute: the 466/349-atom payout
differences equal the differences in retained canonical maintenance insurance.
All other portfolio values and the source-asset budgets agree across orders.
Paid discovery stays unbudgeted; provider earnings and unused domains remain
zero. The shared ATA is deduplicated in the custody sum. Vault plus actual SPL
payout equals initial supply, keeper capital/PnL finish at zero, and the live
source cohort retains the same three-atom residual in every world.

The 72 rollback checks comprise 16 reward-prefix suffix failures, 24
maintenance-prefix suffix failures, 24 healthy/nonprogress retries and eight
SPL-payout-prefix suffix failures. The same first instruction is independently
simulated successfully before each rejected suffix. Expected error indices and
codes pin completed prefixes, and all tracked/compiled Accounts (including
absence) restore except for the exact signature fee. Successful retries use
the same instruction bytes. There are 24 post-reward idempotent fee retries.

One development run failed before execution with `KeypairPubkeyMismatch`:
the reused transaction helper always supplied an owner signer, but
`SyncMaintenanceFee` is permissionless. The helper now derives the owner signer
requirement from instruction metas for both the transaction and the simulated
prefix. Existing signed callers retain their signatures and fee accounting.
The corrected product passed; this was test-harness maintenance, not a program
bug. The final run additionally moves the idempotent fee retry after reward
receipt, explicitly checking that replenished capital cannot revive forgiven
fees.

## Artifacts and commands

Both SBFs were freshly compiled from this clone with platform-tools v1.52 and
locked/offline dependencies. No other lane's build target or SBF was reused.
Wrapper default features are enabled. SHA-256:

| Artifact | Hash |
| --- | --- |
| `/dev/shm/lane23-20260916-sbf/deploy/percolator_prog.so` | `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f` |
| `/dev/shm/lane23-20260916-auth-sbf/deploy/auth_matcher.so` | `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93` |
| LiteSVM `spl_token-3.5.0.so` | `18264f491c7e0ad056dd36f42f8de6d1fedf9f044d1f521e714b4dc6b61594b6` |
| LiteSVM `spl_associated_token_account-1.1.1.so` | `e5e7aed11ad3969eea2aa76c8b4d2e73ea25be7e6b5cce989b7710cf5452496e` |

The installed LiteSVM SPL/ATA fixtures are unchanged. This clone's ignored
matcher artifact path links to its own fresh `/dev/shm` SBF. Logs live in
`/dev/shm/lane23-20260916-logs`.

Provisioning (clone command from `/tmp`; all later commands from the clone):

```bash
git clone --no-hardlinks --branch codex/astra-invariant-cycle-20260915 /tmp/percolator-astra-invariant-cycle-20260915-run /tmp/percolator-lane23-hybrid-reward-provenance-20260916
cd /tmp/percolator-lane23-hybrid-reward-provenance-20260916
git switch -c codex/lane23-hybrid-reward-provenance-20260916
mkdir -p /dev/shm/lane23-20260916-logs
env CARGO_TARGET_DIR=/dev/shm/lane23-20260916-sbf CARGO_BUILD_JOBS=4 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/lane23-20260916-sbf/deploy -- --locked > /dev/shm/lane23-20260916-logs/build-wrapper.log 2>&1
env CARGO_TARGET_DIR=/dev/shm/lane23-20260916-auth-sbf CARGO_BUILD_JOBS=4 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --manifest-path tests/fixtures/auth_matcher/Cargo.toml --sbf-out-dir /dev/shm/lane23-20260916-auth-sbf/deploy -- --locked > /dev/shm/lane23-20260916-logs/build-matcher.log 2>&1
mkdir -p tests/fixtures/auth_matcher/target/deploy
ln -s /dev/shm/lane23-20260916-auth-sbf/deploy/auth_matcher.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
sha256sum /dev/shm/lane23-20260916-sbf/deploy/percolator_prog.so /dev/shm/lane23-20260916-auth-sbf/deploy/auth_matcher.so
sha256sum /home/anatoly/.cargo/registry/src/*/litesvm-0.1.0/src/spl/programs/spl_token-3.5.0.so /home/anatoly/.cargo/registry/src/*/litesvm-0.1.0/src/spl/programs/spl_associated_token_account-1.1.1.so
```

These exports express the identical environment supplied with `env` on each
host invocation. Only the new selector, nearby controls and selected INV-079
guards execute; no unfiltered suite is run.

```bash
export CARGO_TARGET_DIR=/dev/shm/lane23-20260916-host
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane23-20260916-sbf/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run > /dev/shm/lane23-20260916-logs/build-host.log 2>&1
cargo test --locked --offline --test v16_cu inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::reward_maintenance_catchup::clipped_reward_maintenance::v16_program_clipped_self_maintenance_keeps_hybrid_rewards_and_shared_owner_sources_exact -- --exact --nocapture --test-threads=1 > /dev/shm/lane23-20260916-logs/new-final.log 2>&1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::v16_program_paid_discovery_fresh_handoff_authenticates_liquidation_and_keeper_exit \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::v16_program_nonzero_funding_fresh_handoff_preserves_owner_and_keeper_entitlement \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::reward_maintenance_catchup::v16_program_keeper_maintenance_preserves_distinct_reward_budgets_until_catchup \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::reward_policy_catchup::v16_program_reward_policy_succession_preserves_receipts_and_effective_price_catchup \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::exposed_keeper_provenance::v16_program_exposed_auth_keeper_reward_commutes_with_settlement_through_hybrid_catchup \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::reward_terminal_redemption::v16_program_hybrid_catchup_rewards_survive_resolved_cohort_redemption_order \
  inv_045_no_free_mark_movement::trade_origin_catchup::corroborated_mark_fees::v16_program_corroborated_paid_mark_only_distributes_new_liquidation_fees \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::retained_penalty_handoff::paid_origin_hybrid_recipient::v16_program_paid_origin_penalty_survives_dual_hybrid_recipient_catchup > /dev/shm/lane23-20260916-logs/controls.log 2>&1
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
  inv_079_public_reachability_evidence::v16_program_invariant_harnesses_are_test_free_roots > /dev/shm/lane23-20260916-logs/inv079.log 2>&1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_045_authenticated_reward_handoff.rs tests/invariants/cu/inv_045_reward_maintenance_catchup.rs tests/invariants/cu/inv_045_clipped_reward_maintenance.rs
git diff --check
git diff --exit-code d64f049005847848b095b3b8b2d21318d0504296 -- src Cargo.toml Cargo.lock tests/fixtures tests/invariants/open_findings.tsv tests/invariants/invariant_status.tsv tests/invariants/coverage_reopenings.tsv
```

The two development runs used the same test environment and the short selector
`v16_program_clipped_self_maintenance_keeps_hybrid_rewards_and_shared_owner_sources_exact`
without `--exact`; logs are `new-development.log` and `new-development-2.log`.

## Results and limits

| Scoped Check | Result |
| --- | --- |
| Fresh wrapper SBF | PASS, exit 0, 25.65s |
| Fresh auth matcher SBF | PASS, exit 0, 11.57s |
| Host test targets, compile only | PASS, exit 0, 1m07s |
| New exact selector | PASS, 1/1, 6.32s |
| Eight exact nearby controls listed above | PASS, 8/8, 27.66s |
| Sixteen exact INV-079 guards listed above | PASS, 16/16, 2.87s |
| Scoped rustfmt | PASS, exit 0 |
| Git whitespace checks | PASS, exit 0 |
| Production/manifests/locks/fixture sources/status TSV identity | PASS, exit 0 against `d64f0490` |

The final new selector passes 1/1 in 6.32 seconds, with eight histories,
16 liquidations, 24 idempotent retries, 72 rollbacks and eight payouts.
Peak measured transaction CU is 322,491, below the asserted 500,000 bound.
Wrapper, matcher and initial host builds all exit 0. The host build emits
existing dead-code and Solana future-compatibility warnings.

The nearby controls cover paid discovery/fresh handoff (16 worlds), nonzero
funding (2), unclipped keeper maintenance (2), liquidation-policy succession
(2), exposed AuthMark recipients (4), resolved cohort redemption (4),
corroborated paid marks with common owners (4), and paid-origin dual-Hybrid
recipients (8). All pass against the fresh SBF. The selected INV-079 checks are
thirteen source/metadata guards and three trace/classifier checks; no separate
fixed-blocker campaign is run. The source checkout remains clean at the base.

This is bounded conformance for a flat recipient, two assets, five portfolios,
two reward episodes and three observation slots. It does not establish generic
provenance reclassification, underwater exposed keepers, CPI/AuthMark reward
origin, arbitrary report histories, additional providers, nonzero funding,
mid-history policy succession, maximum shape, native custody or complete
terminal cohort exits. Peer maintenance accounting uses decoded fee cursors
with independently checked phase endpoints; it is not an arbitrary-fee-history
model. The three-atom live residual is preserved, not terminally redeemed.

Changed files: the new INV-045 child, its three-line parent module registration,
the existing authenticated-handoff submit helper, this report and the README.
No production or machine disposition changes are warranted by these samples.
