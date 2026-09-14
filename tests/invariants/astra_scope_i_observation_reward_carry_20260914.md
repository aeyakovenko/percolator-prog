# Scope I: Observation, Reward Provenance And Fractional Carry

Base: `d01245dd993bfe60c2448360733c3bb63c41aa19`, the fetched tip of
`origin/codex/astra-open-holdout-ledger-20260912`.
Branch: `codex/scope-i-observation-provenance-carry-20260914`.
Isolated shared-object clone: `/tmp/percolator-scope-i-20260914`.
The protected checkouts were only consulted read-only; all edits, index changes,
commits and host build outputs belong to this clone.

No production, dependency, fixture-program or machine-status change. No public
instruction conformance mismatch was established in these bounded histories.
At the time of this scope rows **422, 425 and 426 remained OPEN**; later
top-level regressions close rows 425 and 426. This file remains a bounded
history audit, not a generic closure or production red/green claim for row 422.

## Ownership And Route/History Matrix

Reviewed `scripts/loop.md`, the relevant charter statements, README coverage,
the reopening ledger and current INV-020/024/036/038/041/045/052/053/054/056/061/
071/072/081/085/086/088 owners, including Scopes H/O/V/C.

| Existing owner | Existing boundary | Scope I increment |
| --- | --- | --- |
| H target arrival; C moving resets | Integral owner quantities, zero funding; C adds repeated resets after anchor movement | Fractional K/F owners and different account settlement cadences during three nonzero resets per asset |
| Generated fractional routes and owner cadence | Fixed targets; separate K/F floors and residue-adjusted entitlement | Changed targets after K movement, all four transports, reduction/publication order, and complete owner payouts |
| Overlapping checkpoints | Integral lots; replacements before K movement | Fractional settlement after movement; no pending funding queue is claimed |
| Retained penalty handoff | One bilateral paid-discovery history, flat recipient, stale then fresh liquidation and full catchup | Four paid-discovery transports, an exposed AuthMark recipient, observation order and missing-tail interruptions through payout |
| Exposed AuthMark keeper | Bilateral discovery; own PnL and authenticated rewards | Old retained penalty plus CPI-origin discovery, receipt-only credit and missing-evidence tails |
| O current Hybrid; V dual Hybrid; C composite recipient | Recipient external feeds, multi-provider completion or recipient-as-target | No new Hybrid-recipient cell; the recipient's separate AuthMark leg has fractional cap carry |
| INV-053/054/056 and INV-061 | Complete current health, certificate epochs, discovery hints and independent sizing | Existing independent certificate/stock/source checks are reused; exact sizing remains an adjacent control |

The carry matrix is 2 price directions x 2 owner cadences x 2 CPI choices x
2 single/batch choices x 2 publication/reduction orders = **32 histories**.
Publication order is coupled to asset order and to grouped/one-slot market
accrual; eager owner cadence uses one-slot market accrual in both orders.
Those scheduling axes are not independently exhausted.

The provenance matrix is 2 CPI choices x 2 single/one-leg-batch choices x
2 observation orders = **8 histories**. Each history includes paid discovery,
stale-report liquidation, a different fresh report while effective price still
lags, complete target catchup, recipient reduction and actual SPL payout.
The report-first schedule also settles the recipient's AuthMark observation
before target liquidation; the other schedule settles it afterward.

## Row 425 Guarantee

Owner: `cu/inv_045_fractional_reset_histories.rs`, mounted below
`public_carry_order::generated_fractional_routes`.
The existing signed-numerator ledger now records each target episode's start
price and slot. Its fixed-target controls use the same original inputs.

Two assets start at 100/125 with a 24-bps cap and opposite price directions.
Four owners deposit [100003, 200009, 300017, 400037] atoms through public
System/SPL/wrapper instructions. Mint authority is revoked. Initial fractional
reductions remove 1/4 and 1/2 lot; later reductions remove 1/8 and 3/8 lot at
slots 5/10/15/20. Slots 5/10/15 replace each target by the initial anchor plus
or minus 27/34/41. Each replacement consumes nonzero carry after actual K
movement; the input-derived pre-reset carries are 2000/5000. Targets are never
reached, so cap anchors remain 100/125. The active owners optionally settle
again at slots 3/8/13/18. One passive owner's complete Account remains
unchanged until final closing trades.

The oracle computes whole-episode cap capacity from public inputs and keeps K
and F numerators, individual floor residues, gross funding counters, latent
owner value, quantities and settlement checkpoints distinct. It uses bounded
u64/i128 arithmetic, not a full-width arithmetic theorem. Funding is nonzero
with a saturated 10000-e9 rate cap. No expected owner value is seeded from an
observed effective price, fee or payout.

Every checked prefix reconciles price/carry/K/F, unit ADL, matched OI, zero B
and pending obligations, owner capital/PnL plus latent value, fixed supply and
custody. All transports and publication orders agree exactly at a given owner
cadence. Across cadences, each payout difference equals that owner's additional
K/F floor residue; passive payouts are unchanged. The bound is 16 atoms per
active owner for four extra two-asset/two-lane settlement frontiers.

Results: **32 histories, 192 nonzero resets, 320 complete Account rollbacks,
128 exact resolved SPL payouts**, 144 separate-floor witnesses and three
nonzero cadence comparisons. Final custody is exactly the independently
calculated **12 or 15 residue atoms**, with zero insurance/provider earnings
and no remaining fresh or liened source claim. Peak measured CU: **474375**,
ceiling 600000. Rollbacks cover 192 publications and 128 reduction prefixes;
terminal rollback is inherited coverage and is not counted here.

Limits: two solvent AuthMark assets, four owners, monotone target extensions,
fixed anchors, unit ADL, zero trading/maintenance fees and explicit market
accrual. Arbitrary target reversal/arrival, pending funding replacements,
source-claim consumption, inline accrual, other oracle modes, maximum shapes
and generic arithmetic or route closure remain open. Finite integer widths and
these settlement schedules do not close INV-085/086 or row 425.

## Rows 422 And 426 Guarantee

Owner: `cu/inv_045_paid_origin_routes.rs`, below
`trade_origin_catchup::authenticated_reward_handoff::retained_penalty_handoff`.
The original retained-penalty selector remains available with its original
one-asset configuration. Its helper now also accepts the new route histories.

All economic accounts are constructed publicly and the mint supply is fixed.
CPI discovery uses a system-created authenticated matcher and a publicly
configured 1000-bps bid spread. Its maker explicitly grants the discovery-fee
cap. Bilateral and CPI requests use the same negative taker size, producing the
same signed economic positions and accepted 990400 quote from a 900000 raw
print. Paid discovery stages target 992320 and charges **1540072 atoms**.

The target price path is independently specified as **997600, 995206, 980000**
at slots 6/7/14. Stale provenance first charges **5987 atoms with no reward**.
A fresh 980000 report at slot 7 then supports an **8287-atom** effective-price
penalty, a **2762-atom** keeper reward and domain allocations **2762/2763**.
The final catchup adds no penalty or reward. Penalties use executed close
quantity and independent two-stage notional/fee rounding; this is not an
independent minimal-liquidation-quantity oracle.

The recipient holds three long AuthMark lots from 100 toward 120. Its price
moves to 101 and then 103, retaining fractional cap carry. Its **9-atom own
PnL claim** remains distinct from the reward. Target-only refresh/liquidation
preserves the recipient's legs and PnL; reward changes only its value by the
earned amount, with current-certificate checks supplied by the shared oracle.
The peer carries the opposite independently priced AuthMark value.

The recipient publicly closes its AuthMark position and withdraws exactly
**3762 SPL atoms** (1000 original principal plus 2762 reward); its 9-atom
positive PnL claim remains internal. Final owner values, fee classes, domain
budgets and payout agree across all eight histories. Other owners' remaining
Hybrid positions and the recipient's positive claim are not terminally redeemed.

Each history rejects missing declared provider tails before phases, after
completed refresh/liquidation prefixes, and after an actual SPL-paying prefix,
then executes the same economic instructions successfully. Complete
`Option<Account>` rollback includes tracked and compiled keys, matcher
context, portfolios, tokens, metadata and absence, apart from the exact payer
signature fee. The inherited equivocal-report and healthy retry checks remain.
Results: **8 histories, 16 liquidations, 8 exact payouts**, **84 missing-tail
rollbacks**, including eight restored SPL payouts; additionally 24 inherited
equivocal suffix and 24 healthy nonprogress rollbacks. Peak measured CU:
**382424**. The new two-asset per-instruction bound is 500000; the old
one-asset control retains 325000. Checked observation/payout packets are at
most 1232 bytes; carry and discovery setup packet sizes are not claimed.

Missing *declared account tails* are structural input controls. All favorable
successes use completed current market evidence. This does not establish
discovery of an omitted unknown Hybrid observation, generic current-evidence
completeness, or a general rule for reclassifying prior effective-price
provenance. Provider replacement/composites, unavailable feeds, recipient
funding/maintenance, policy succession, arbitrary sizes, shared owners,
underwater recipients and terminal cohort closure remain outside this family.
Rows 422/426 therefore remain OPEN.

## Artifacts And Validation

Production inputs compare identically to Scope W:
`git diff d01245dd -- src Cargo.toml Cargo.lock tests/fixtures/auth_matcher`
from `/run/percolator-pr135-scope-w-20260913` produced no differences.
Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
The wrapper was not rebuilt; the fixed SBF was used directly.

| Artifact | SHA-256 |
| --- | --- |
| `/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so` | `79b298706ca7ffab09a017e5f37c4c0fc43d4e6292c092d126f4fbe8dff1b673` |
| `/tmp/percolator-scope-i-20260914/tests/fixtures/auth_matcher/target/deploy/auth_matcher.so` | `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93` |

The matcher was copied from Scope W. A private 5-GiB executable tmpfs at
`target/host` holds a copied host dependency cache; test binaries compile from
this clone. The root filesystem lacked room and the first runtime-directory
copy exhausted its mount, so that partial generated copy was removed before
using the private mount. No shared build cache was written.

Development corrected test type/visibility declarations, the recipient fixture's
one-asset allocation, and its one-asset CU guardrail. These were fixture changes,
not production conformance corrections. The fixed-target carry oracle was not
weakened to accommodate the new histories.

Exact selector listing: **2 tests, 0 benchmarks**. The final exact run passes
**2/2 in 37.59s**, with 40 histories total. Carry CU was 469877 in that run;
474375 is the highest observed across successful runs. The provenance peak
remained 382424. Adjacent controls pass **13/13 in 126.35s**; their highest
reported CU is 734316 in the inherited Scope O selector. There are **no inherited
control failures**. The four metadata gates pass **4/4 in 0.01s**. Formatting,
working/staged whitespace checks and both local commit show checks pass.
Existing Solana 1.18 future-compatibility and metadata-target dead-code warnings
remain. No full repository suite, fuzz campaign or Kani proof is claimed.

The first commit attempt needed a local Git identity because clones do not inherit
repository-local configuration. Only this clone was configured, using the source
repository's existing author name/email; the retry succeeded.

Commands run from the isolated clone:

```sh
export CARGO_TARGET_DIR="$PWD/target/host" TMPDIR="$PWD/target/host"
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so
carry=inv_045_no_free_mark_movement::public_carry_order::generated_fractional_routes::fractional_reset_histories::v16_program_fractional_reset_histories_preserve_route_and_residue_adjusted_owner_value
provenance=inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::retained_penalty_handoff::paid_origin_routes::v16_program_paid_origin_routes_preserve_old_penalty_and_fresh_reward_through_missing_tails
cargo test --locked --offline --test v16_cu -- --exact --list "$carry" "$provenance"
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 "$carry" "$provenance"
cargo test --locked --offline --test v16_cu "$provenance" -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_045_no_free_mark_movement::public_carry_order::generated_fractional_routes::v16_program_generated_fractional_kf_routes_preserve_carry_owner_value_and_residue \
  inv_045_no_free_mark_movement::public_carry_order::generated_fractional_routes::v16_program_fractional_owner_crank_cadence_preserves_carry_and_residue_adjusted_entitlement \
  inv_045_no_free_mark_movement::public_carry_order::target_arrival_entitlement::v16_program_target_arrival_plateau_and_restart_preserve_interleaved_owner_entitlement \
  inv_045_no_free_mark_movement::public_carry_order::target_arrival_entitlement::moving_reset_routes::v16_program_repeated_moving_target_resets_preserve_carry_across_matcher_handoffs \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::retained_penalty_handoff::v16_program_retained_stale_penalty_survives_fresh_liquidation_and_catchup \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::v16_program_paid_discovery_fresh_handoff_authenticates_liquidation_and_keeper_exit \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::exposed_keeper_provenance::v16_program_exposed_auth_keeper_reward_commutes_with_settlement_through_hybrid_catchup \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::hybrid_recipient_provenance::v16_program_dual_hybrid_reward_lineage_survives_recipient_routes_and_payout \
  inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations::active_keeper_observations::cpi_keeper_observations::generated_current_hybrid::v16_program_generated_current_hybrid_recipient_routes_match_full_health_and_rollback \
  inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations::active_keeper_observations::reward_recipient_liquidation::v16_program_composite_recipient_target_routes_restore_missing_evidence_prefixes_and_payout \
  inv_053_full_health_recertification_equivalence::v16_bpf_trade_refreshes_stale_related_portfolio_leg_on_demand \
  inv_056_hints_are_discovery_only_favorable_actions_fully_refresh::v16_bpf_inv056_mixed_observations_preserve_full_refresh_trade_boundary \
  inv_061_deterministic_bounded_liquidation::enumerated_public_sizing::caught_up_portfolio_sizing::v16_program_caught_up_multi_asset_sizing_preserves_health_and_beneficiary_value
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --check --oneline HEAD
git show --check --oneline HEAD^
sha256sum "$PERCOLATOR_FUZZ_SBF" tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
```

## Local Changes

Test commit: `ef151b1cafd686a2e67b6d5c824bc3791be2bdc7`,
`test(invariants): cover fractional resets and paid-origin reward histories`.
The following documentation commit records coverage and leaves the rows OPEN.

- `tests/invariants/cu/inv_045_fractional_reset_histories.rs`: new carry/cadence matrix.
- `tests/invariants/cu/inv_045_generated_fractional_routes.rs`: target-episode ledger and child registration; original controls retained.
- `tests/invariants/cu/inv_045_paid_origin_routes.rs`: paid-origin transport and observation-order matrix.
- `tests/invariants/cu/inv_045_retained_penalty_handoff.rs`: shared exposed-recipient history, missing-tail checks and original control.
- `tests/invariants/README.md`: coverage summary and limits.
- `tests/invariants/coverage_reopenings.tsv`: comments only for rows 422/425/426.
- `tests/invariants/astra_scope_i_observation_reward_carry_20260914.md`: this audit.

Successful final logs are `target/scope-i-final-exact.log`,
`target/scope-i-controls.log` and `target/scope-i-metadata.log` in this clone.
