# Scope V: Hybrid Reward Provenance

Requested base: `origin/codex/astra-open-holdout-ledger-20260912`, fetched at
`d134c64d788e49264ecc0187a75209053925f43a`.
Branch: `codex/pr135-scope-v-hybrid-reward-provenance-20260913`.
New worktree: `/run/user/1001/percolator-scope-v-20260913`. It was created at
`/tmp/percolator-scope-v-20260913` and moved after compilation filled that
filesystem. Only this worktree's files/artifacts were changed. Neither protected
checkout was edited; no external PR branch, diff or test was inspected.
Row 422 was read only as a coverage label.

## Existing Coverage And Increment

| Existing invariant owner | Evidence and distinction |
| --- | --- |
| INV-045 `trade_origin_catchup` | Paid Hybrid discovery, unequal stale reports, zero reward and catchup order; no independently moving Hybrid recipient. |
| INV-045 `authenticated_reward_handoff` | Paid discovery then fresh evidence, effective-price penalty and flat keeper payout, including funding. |
| INV-045 `accepted_price_reward::reward_catchup_order` | Fixed authenticated target with actual catchup/report renewal; flat recipient and one asset. |
| INV-045 `exposed_keeper_provenance` | Active AuthMark recipient, two rewards and own PnL; no recipient external report. |
| INV-045 maintenance, policy and terminal reward children | Maintenance/domain budgets, retained policy receipts and cohort redemption; no dual moving Hybrid transport product. |
| INV-020 Scope O `generated_current_hybrid` | Independent recipient Hybrid feed and four trade routes after complete price catchup. |
| INV-020 recipient-liquidation / payout rollback children | Recipient becomes a second target or renewed liquidation/payout suffix rollback; no independent lag and split recipient reductions. |
| INV-045 Scope H `target_arrival_entitlement` | AuthMark arrival, plateau and resumed cap anchors with independent owner carry accounting; no liquidation reward. |
| INV-061 `caught_up_portfolio_sizing` | Exhaustive small-quantity sizing with multiple fully caught-up assets; retained as the independent sizing control. |

New INV-045 child: `inv_045_hybrid_recipient_provenance.rs`, selector
`v16_program_dual_hybrid_reward_lineage_survives_recipient_routes_and_payout`.
The 32 worlds cross two asset assignments, two directions, two report/catchup
schedules, two recipient settlement orders and two transport orders. Report-first
also renews equal-price reports; target-first reuses first-accepted reports.
Observation order is paired with single-then-batch versus batch-then-single
recipient reductions. These paired axes are not independently exhausted.

## Guarantee And Oracle

System/SPL/ATA/wrapper routes create every economic Account. SPL mint authority
is revoked after deposits. Only program loading, signer SOL, Clock/blockhashes
and external Pyth reports are harness inputs. Both feeds are supplied to every
observation. No initialized protocol Account is injected or restored.

The target starts at 1000000 with 100 adverse lots and 5100000 collateral.
The recipient starts at 2000000 with two adverse lots and 10000000 collateral.
Prices move in opposite directions. An input-derived linear 24-bps-per-slot
envelope predicts exposed target arrival after three steps. Recipient exposure
is removed after step two, so its next observation adopts the authenticated raw
target through the empty-market path in `canonical_accrual_path_for_target_view`.
Stored target, individual report prices, publication time, first acceptance
slot and effective price are checked separately. Reusing a report cannot renew
its acceptance slot; renewed reports cannot reset the price path.

One first-step partial liquidation must restore target health within six calls.
The oracle uses executed close quantity and the independently predicted effective
price: `ceil(ceil(q * price / POS_SCALE) * 5 / 10000)`. The result must distinguish
entry, pending target and recipient entry/effective/target prices. Reward is
`floor(fee * 3333 / 10000)`. This is independent price/fee attribution, not an
independent liquidation-quantity oracle.

The target loses exactly 240000 settled price loss plus its penalty. Credit
changes only recipient capital and certificate validity; its Hybrid legs and
PnL remain unchanged. The remainder splits floor/ceil into the selected asset's
two domains; every other budget stays zero. Recipient reductions remove one lot
after each of the first two steps. Its independent own loss is 9600 then 14400,
separate from reward entitlement. Reward payout occurs while both prices lag;
remaining recipient principal is paid after both catch up. Total recipient
payout must equal `10000000 - 14400 + reward`.

Each liquidation/refresh candidate executes before a stale recipient observation
rejects. Actual SPL reward payout executes before a stale target observation
rejects. Error position and program-success logs establish the prefixes; complete
`Option<Account>` frames include tracked and compiled accounts, metadata and
absence. Only the exact runtime signature fee changes. The same economic
instruction then succeeds with a fresh blockhash. Stock, reservation, source
credit and current-certificate censuses run after every checked transaction.
Owner values, fee/reward, domains and custody agree across the order variants.
Aggregate conservation is supplementary.

## Limits And Row Impact

Row **422 remains OPEN**. `open_findings.tsv` and `invariant_status.tsv` are
unchanged. This adds bounded INV-020/024/036/041/045/061/062 conformance, with no
independent discovery or whole-invariant certification. No production or
dependency change is included.

Both histories originate in authenticated reports. This probe does not decide
whether fresh evidence may reclassify a prior trade-origin mark or retained
penalty before catchup. Paid discovery, stale fallback, provider replacement,
composite feeds, funding, maintenance/trading fees, policy succession, CPI,
underwater recipients, maximum shapes and terminal cohorts are outside scope.
Each batch contains one leg. The target remains exposed after its first
liquidation; later losses remain latent while its Account is framed. The
target/recipient comparison covers the settled liquidation episode and recipient
payout, not final target-close entitlement or all possible common-owner histories.

## Validation

Fresh default-feature program and matcher SBF builds passed with platform-tools
v1.52 and locked offline dependencies. Engine pin:
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`. SHA-256:

- Program: `49f49d1a8a7a3c20e4e3e09c635c4d5ce08925056030668b6838ccf7224800b0`.
- Matcher: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

The program was built before relocating this same worktree; production files
were unchanged by the relocation or probe. Both host test binaries build with
`--no-run`. Exact listing selects one test. The new exact selector passes 1/1
in 17.41s: 32 worlds, 32 rewarded liquidations, 32 SPL payout rollbacks and
96 complete Account rollbacks. Peak cost is 369909 CU under a 900000-CU bound;
every checked transaction is at most 1232 bytes. Both single and batch recipient
reductions execute in every world, followed by two exact recipient SPL payouts.

The seven adjacent exact selectors below pass 7/7 in 116.16s, including Scope O,
Scope H, the 96-world accepted-price catchup family and 48-world INV-061 sizing
control. All four INV-079 metadata gates pass 4/4, including the reopening-set
gate. Formatter, working/staged diff checks and committed show-check pass.
The full repository suite and Kani/engine proofs were not run.

Initial build attempts encountered filesystem exhaustion; moving this worktree
and resuming recovered them. During probe development, test expectations were
corrected for the batch ABI, refresh of the supplied observer certificate and
the explicit empty-asset price path. No production defect was established.
Final logs are in this worktree's `target/scope-v-{build,list,exact,controls,metadata}.log`.

Run from the isolated worktree:

```sh
export CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0
export TMPDIR="$PWD/target" PERCOLATOR_FUZZ_SBF="$PWD/target/deploy/percolator_prog.so"
env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir target/deploy -- --locked
env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
scope_v=inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::hybrid_recipient_provenance::v16_program_dual_hybrid_reward_lineage_survives_recipient_routes_and_payout
cargo test --locked --offline --test v16_cu "$scope_v" -- --exact --list
cargo test --locked --offline --test v16_cu "$scope_v" -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_045_no_free_mark_movement::trade_origin_catchup::v16_program_trade_origin_liquidation_prices_and_entitlements_survive_catchup_order \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::v16_program_paid_discovery_fresh_handoff_authenticates_liquidation_and_keeper_exit \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::exposed_keeper_provenance::v16_program_exposed_auth_keeper_reward_commutes_with_settlement_through_hybrid_catchup \
  inv_045_no_free_mark_movement::accepted_price_reward::reward_catchup_order::v16_program_reward_price_tracks_actual_catchup_across_report_and_crank_orders \
  inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations::active_keeper_observations::cpi_keeper_observations::generated_current_hybrid::v16_program_generated_current_hybrid_recipient_routes_match_full_health_and_rollback \
  inv_045_no_free_mark_movement::public_carry_order::target_arrival_entitlement::v16_program_target_arrival_plateau_and_restart_preserve_interleaved_owner_entitlement \
  inv_061_deterministic_bounded_liquidation::enumerated_public_sizing::caught_up_portfolio_sizing::v16_program_caught_up_multi_asset_sizing_preserves_health_and_beneficiary_value
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
cargo fmt --all -- --check
git diff --check
git diff --cached --check
sha256sum target/deploy/percolator_prog.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
git show --format= --check HEAD
```
