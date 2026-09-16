# Row 422: native Hybrid rewards through terminal replay

Date: 2026-09-16. Local branch:
`codex/row422-native-terminal-reward-20260916`.
Worktree: `/dev/shm/row422-native-terminal-reward-20260916`, created from
`origin/codex/astra-invariant-cycle-20260915` at
`4d17c967bdc09ad02f4ea0d68693e6998a9d14b9` using
`/tmp/percolator-astra-invariant-cycle-20260915-run`.
Protected-file comparison also uses `origin/main` at
`d809e9a563d9b8bf38f32648b32a15d75f526ec8`.
No publication or edits to `/home/anatoly/percolator-prog`.

**No current behavior violation was found in this bounded product. Row 422 stays
OPEN/missing and INV-045 stays `REFUTED_CURRENT`.** No TSV is changed.

## Coverage and non-overlap

The new child of `inv_045_reward_terminal_redemption.rs` reuses the existing
observation, submission, stock/certificate census, fee and treasury helpers.
Sixteen histories cross both liquidation recipients independently present or
omitted, keeper withdrawal/unwrap after the first liquidation or at resolution,
and forward/reverse cohort redemption order. All five portfolios reach economic
terminal state; four owners retain wrapped native payouts and the keeper unwraps
to SOL. No new CPI trade product is claimed.

Lane 26's optional recipients and the recent CPI/maintenance product end with
classic SPL live custody. The parent terminal selector always supplies the reward
recipient and uses classic SPL. This increment joins omitted entitlement with
native sync/unwrap, recreated destinations and full cohort terminal payout.

Economic accounts are constructed with public System, ATA, SPL and wrapper
instructions. The existing native-market helper installs the native mint genesis
account missing from LiteSVM. Other harness inputs are program loading, signer
SOL, Clock and external Pyth reports. There are no Percolator-owned byte writes,
snapshot restores, or fabricated quote balances. A private host target cache was
copied with `cp -a`; no shared target directory is written.

## Public trace and independent checks

1. At slot 1, configure one Hybrid asset at 1,000,000 with production risk
   parameters, zero funding and a 3333-bps liquidation reward share. Publicly wrap
   and deposit five endowments: 5,100,000 / 100,000,000 / 10,000,000 / 10,000,000 /
   1,000. Open the target's 100-lot position against its peer.
2. At slot 5, advance time with stale evidence, then trade one lot at raw 900,000.
   The accepted print is 990,400 and staged target 992,320; effective price stays
   1,000,000. Independent bilateral rounding charges 1,540,072 discovery atoms,
   all outside source budgets. Separately transfer 37 admin lamports to the
   keeper's native ATA without syncing it. No engine or vault Account changes.
3. At slots 6 and 7, accept fresh reports for 992,320 and 980,000. Effective prices
   are 997,600 and 995,206. The observed liquidation quantities are 12,001,223 and
   16,653,247. Compute each penalty independently as
   `ceil(ceil(q * effective_price / POS_SCALE) * 5 / 10000)`, giving 5,987 and
   8,287. Each differs from pricing at entry, raw print, accepted print or report
   target. Reward is `floor(penalty * 3333 / 10000)` only with the recipient on
   that call. Retained amounts split with floor/ceil between source domains 0/1.
4. Before each target progress call, execute native sync and that call with an
   invalid System suffix. Independently simulate the successful prefix and pin
   the actual failing instruction index. Full tracked and compiled Accounts,
   including SPL data, lamports and absence, roll back except the exact payer
   signature fee. Commit the crank alone and require unchanged custody and peers.
   In early histories, sync the 37 lamports, withdraw all current keeper capital,
   unwrap, and recreate the same ATA. That entire payout/unwrap prefix also has
   an exact failed-suffix rollback before its committed retry.
5. At slot 14, catch up fully to 980,000 and refresh the exposed cohort. No new
   reward is created; same-slot target retries with and without a recipient
   reject as `EngineNonProgress`. Then resolve, checking an aborted sync/resolve
   prefix first. Resolution follows the same accounting in every payout schedule.
6. At slot 30, install an unrelated 1,200,000 report and drain all five portfolios
   in the selected order. Prices and the frozen oracle profile remain unchanged.
   Every accepted terminal step has an aborted-prefix twin. Keeper closure is
   bundled with sync/unwrap; the other four payouts remain wrapped. Waiting
   positive-PnL portfolios reject exactly until the other stored legs detach.
7. Recreate the keeper's native ATA again and replay at slots 30 and 100.
   `CloseResolved` rejects as `EngineNonProgress`. `SyncNative` followed by
   `ClaimResolvedPayoutTopup` rejects as `EngineLockActive`: this fully backed
   cohort has no captured payout snapshot. Both restore exact frames. The vault
   still exceeds every omitted reward, so these are not empty-vault checks.

All success submissions reconcile tracked and explicit instruction-account
lamports less the actual signature fee. Native accounts independently satisfy
`lamports = rent + SPL amount + unsynced donation`. Keeper SOL increases by exactly
paid principal/rewards plus 37 donated lamports plus returned ATA rent. Early
histories return two ATA rents; late histories return one. ATA recreation is paid
by the separate harness payer. Vault token amount equals engine vault throughout;
vault plus all five protocol payouts equals the original 125,101,000 atoms.
The native mint and provider Accounts remain unchanged.

| Recipients at slots 6 / 7 | Rewards | Omitted | Source budgets 0 / 1 | Keeper protocol payout | Final engine vault |
| --- | ---: | ---: | --- | ---: | ---: |
| Neither | 0 | 4757 | 7136 / 7138 | 1000 | 1554350 |
| First only | 1995 | 2762 | 6139 / 6140 | 2995 | 1552355 |
| Second only | 2762 | 1995 | 5755 / 5757 | 3762 | 1551588 |
| Both | 4757 | 0 | 4758 / 4759 | 5757 | 1549593 |

Other payouts are always 3,550,175 / 101,540,147 / 9,209,964 / 9,245,364.
Final vault equals insurance plus four residual rounding atoms. All principal,
positive PnL, OI and source-claim bounds are zero. Within each recipient schedule,
episode quantities/penalties/rewards, payouts, treasury, domain budgets and
residual agree across both timing/order axes. Across all schedules, adding back
only committed rewards gives the same principal payouts, insurance, vault and
total source budget. Earned plus omitted shares remain exactly 4,757.

## Artifact and commands

Reused unchanged wrapper:
`/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so`, SHA-256
`e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
SPL/ATA binaries come from the existing LiteSVM registry helpers. No matcher is
needed and no fixture files are created or changed.

Commands run from the isolated worktree. Exports below express the environment
passed with `env` on each invocation. Logs are under
`/dev/shm/row422-native-terminal-reward-20260916-logs`.

```bash
export CARGO_TARGET_DIR=/dev/shm/row422-native-terminal-reward-20260916-target
export TMPDIR=/dev/shm/row422-native-terminal-reward-20260916-tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::reward_terminal_redemption::native_reward_terminal::v16_program_native_hybrid_omitted_rewards_stay_unclaimed_through_sync_unwrap_and_terminal_replay \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::reward_terminal_redemption::v16_program_hybrid_catchup_rewards_survive_resolved_cohort_redemption_order \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::reward_destination_retry::v16_program_hybrid_reward_destination_retries_preserve_only_committed_receipts \
  inv_081_success_state_validity_over_complete_public_routes::v16_program_native_quote_roundtrip_preserves_lamports_rent_and_unsynced_value \
  inv_045_no_free_mark_movement::v16_program_mark_writer_and_trade_exit_composition_is_source_complete
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_program_open_lof_manifest_snapshot_is_structurally_honest \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_program_invariant_harnesses_are_test_free_roots
rustfmt --edition 2021 --config skip_children=true --check \
  tests/invariants/cu/inv_045_reward_terminal_redemption.rs \
  tests/invariants/cu/inv_045_native_reward_terminal.rs
git diff --check
git diff --cached --check
git diff --exit-code origin/main -- src Cargo.toml Cargo.lock tests/fixtures \
  ':(glob)tests/invariants/**/*.tsv' ':(glob)tests/invariants/*.tsv'
git diff --exit-code 4d17c967bdc09ad02f4ea0d68693e6998a9d14b9 -- src Cargo.toml Cargo.lock tests/fixtures \
  ':(glob)tests/invariants/**/*.tsv' ':(glob)tests/invariants/*.tsv'
git show --format= --check HEAD
```

## Results and limits

The new selector passes: sixteen histories, 32 liquidations, 112 terminal progress
calls and 392 exact rollbacks. Measured CU peaks: paid discovery 152,454; all
submissions 322,832; rejected bundles 322,832. Trade guard is 345,000; the shared
submission guard is 500,000 per instruction and the final bundle guard is
900,000. These are finite-shape measurements, not maximum-shape evidence.

Final targeted CU invocation: **5/5 PASS** (21.21s; `final-cu.log`), including
the new selector, classic SPL terminal and optional-recipient controls, native
roundtrip and the mark-writer composition gate. INV-079 guards: **4/4 PASS**
(0.01s; `guards.log`). Scoped rustfmt, whitespace checks, protected diffs against
both `origin/main` and the starting commit, and the final commit's
`git show --format= --check HEAD` pass. Host compilation retains the existing
Solana future-compatibility warning. No unfiltered suite or SBF rebuild was run.

Development failures corrected test assumptions: SPL close can leave a cleared
zero-lamport Account in LiteSVM, and the top-up route rejects before a payout
snapshot exists. Neither produced unpaid movement or a custody discrepancy.
There were no production edits or weakened economic/rollback assertions.

Coverage is bounded to one asset/source, five distinct owners, a flat keeper,
fixed risk/reward policy, zero funding, two downward liquidation episodes and
one discovery trade transport. Quantities come from deployed OI; fee/reward
rounding is independent but liquidation sizing is not independently proved.
The comparison covers stated economic outcomes, not full cross-world byte
equivalence. This does not establish arbitrary histories, reward-bearing exposed
keepers, multiple providers, CPI switching, snapshot-backed top-up payouts,
insurance withdrawal, portfolio deletion or slab retirement. Four rounding atoms
and insurance remain in native custody. Row 422 remains OPEN/missing.
