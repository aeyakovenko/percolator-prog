# INV-058 row 427 side-OI frontier audit, 2026-09-17

Base: `origin/main` at `c7b5ae07f9a7a36f3ac6f9d1309bb2a66b45ff50`.
Worktree: `/dev/shm/percolator-inv058-row427-20260917`.
Branch: `audit/inv-058-row427-20260917`.

## Decision

Row **427 remains OPEN**, a withheld current gap. Its machine classification is
`Conformance / LIMIT`; INV-058 remains `REFUTED_CURRENT / PUBLIC_ROUTE / SAMPLED`
with counterexample row `427`. Historical `COVERED` prose does not supersede those
rows. The existing metadata guard preserves the distinct-owner obligation and
conformance classification, but does not itself assert the OPEN status.

Review of `INVARIANTS.md`, the README, `coverage_reopenings.tsv`, all eight
`cu/inv_058*.rs` files and their mounts found no new public-route issue satisfying
[scripts/loop.md](../../scripts/loop.md). No new regression or production fix is
justified. Repeating cap rejection, rollback or engine arithmetic alone would
not demonstrate extractable loss, failure of every bounded continuation, or a
required operation exceeding its compute budget. No engine proof or withheld
implementation was used to manufacture coverage.

The wrapper's `ensure_trade_side_oi_cap_view` checks both resulting side counters
after the shared single execution and every requested asset after batch execution.
The source roster locks those sites and engine pin
`94979ede7db934545e53a8f210dd063a9ea3ea63`; source presence is not a behavioral proof.

## Witness map

IDs resolve to exact selectors in the command block. All ten behavioral selectors
were selected explicitly; the other four roster entries below were source-reviewed
only. Counts describe completed assertions only when the selector passes.

| ID | Existing owner and substantive assertion | Boundary |
| --- | --- | --- |
| T1 | [Root](cu/inv_058_cumulative_position_oi_notional_and_rate_limit_integrity.rs): two disjoint pairs fill aggregate OI while individual caps remain slack; all four transports reject a further atom with exact economic rollback. | Four opening routes, fixed mark, unit ADL. Isolates admission, not terminal payout. |
| T2 | Root: nonzero authenticated funding creates two claimants and two capital payers at the cap; all transports reject another atom, then both pairs close with stock/encumbrance reconciliation. | Accepts either `EngineInvalidLeg` or `EngineLockActive`: cannot isolate the OI predicate after funding. Unit ADL; closes are bilateral. |
| T3 | [Liquidation lifecycle](cu/inv_058_liquidation_lifecycle.rs): maintenance-driven full liquidation releases OI, retains the peer's reset leg and second-asset notional, then exercises withdrawal, recreation, cleanup and admission. | Explicitly full reset with unit ADL, not partial ADL or disjoint-pair cap rejection. |
| T4/T5 | [Atomic handoff](cu/inv_058_atomic_oi_fee_handoff.rs): fee-bearing release/refill into a fresh third pair across separate/packed transactions; late cap failure restores fees, positions, matcher state and custody; exact owner payouts. | T4 bilateral, T5 six mixed CPI assignments; fixed price, one asset, fresh recipient legs. |
| T6 | [Two-asset handoff](cu/inv_058_multi_asset_oi_fee_handoff.rs): clear one leg, resize another, refill a fresh pair through mixed routes; late OI and SPL-tail failure restores both books/four fee domains before retry and payout. | Two assets, unit ADL, zero PnL/funding, no elapsed liabilities. |
| T7 | [Generated existing legs](cu/inv_058_generated_side_oi_composition.rs): four seeds, mirrored signs and merged/partitioned schedules exercise all 16 release/refill route pairs, raw OI/count/notional census, 480 rollbacks and final closes. | Three disjoint pairs/two assets, fixed marks, zero fees/funding/PnL; finite histories. |
| T8 | [Existing-leg fee competition](cu/inv_058_existing_leg_fee_competition.rs): two existing pairs compete without release, both execution orders, fees of 2/1 atoms, late rejection and unchanged-request retry; 64 rollbacks and 96 payouts. | One active asset, four route assignments, unit ADL, zero PnL/funding. |
| T9 | [PnL terminal handoff](cu/inv_058_pnl_terminal_handoff.rs): three unequal claims survive capped headroom reassignment and resolution; rank-decreasing permissionless closes pay each owner exactly. | One fully backed domain, integral mark, bilateral single/batch, zero fees/funding, unit ADL. |
| T10 | [Capacity/claim composition](cu/inv_058_capacity_claim_composition.rs): 26 historical sources plus two latent domains, three capped pairs, handoff/reversal and second mark; 28 occupied records, exact owner/domain claims, ranked terminal payout and deletion. | One constrained table, solvent debtors, bilateral single/one-leg batch. Fourteen configured slots are not fourteen simultaneous active legs or maximum composite CU coverage. |

The four other entries in the existing 14-witness roster are root-module selectors:

| Exact suffix (not rerun) | Scope, insufficient to close row 427 |
| --- | --- |
| `v16_program_split_fills_cannot_cross_position_or_side_oi_cap_on_any_route_pair` | Sixteen route pairs at max-1/max/max+1 and exact-max exit on one owner pair; position and side caps coincide. |
| `v16_program_post_transition_caps_match_across_reduction_and_cross_zero_histories` | Same-pair reduction/cross-zero/close-reopen post-state OI and notional, unit ADL. |
| `v16_program_recreated_counterparty_preserves_post_transition_cumulative_limits` | Recreated counterparty, both roles, two-asset rollback and fresh headroom; fixed price/zero fees. |
| `v16_program_batch_tradecpi_configured_leg_cap_rejects_before_hostile_matcher_cpi` | Configured portfolio leg count rejects before hostile matcher execution; external matcher fixture, not aggregate OI or economic liveness. |

The 2026-09-08 attach guard and 2026-09-12 existing-leg resize postcondition are
historical repairs recorded in the README. Their historical red/green results
were not rerun here and are not new LoF/DoS findings.

## Current results and failures

T1-T10 and M1-M2: **12 passed, 0 failed, 1,427 filtered**, 129.31s, exit 0.
The exact INV-079 machine-status selector: **1 passed, 0 failed, 146 filtered**,
exit 0. Thus no current failing witness was observed among these thirteen checks.
The older README reports of an empty INV-058 counterexample projection versus
`{427}` do not reproduce at this base; the current machine row contains `427`.
No behavioral selector was changed, and no broad suite or engine proof was run.

| Completed witness | Current measurements |
| --- | --- |
| T3 | Eight lifecycle worlds, two liquidation steps each; maximum reported CU 250,982. |
| T4/T5 | 8/24 worlds, 16/48 exact rollbacks, 48/144 payouts; trade peaks 277,667/323,251 CU. |
| T6 | 16 worlds, 32 rollbacks, 96 payouts; rejection/trade peaks 601,501/597,178 CU. |
| T7 | 16 worlds, 480 rollbacks, 104 paired checkpoints; rejection/trade peaks 805,697/295,239 CU. |
| T8 | 16 worlds, 64 rollbacks, 96 payouts; rejection/trade/custody peaks 344,425/191,235/47,882 CU. |
| T9 | Eight worlds, 76 rollbacks, 68 ranked terminal calls; peak 453,690 CU. |
| T10 | 16 worlds/full tables, 664 rollbacks (96 paying), 600 terminal calls; peak 1,180,187 CU below its 1,375,000 budget, largest packet 976 bytes. |

Historical cap breaches were substantive conformance failures: the fresh-owner
trace reached `MAX_OI_SIDE_Q + 1` on engine `495a5590`, and the later generated
resize trace accepted excess aggregate OI before the shared wrapper postcondition.
Their repaired descendants T1/T7 pass here. Neither historical cap breach alone
proves independent-victim loss or persistent DoS under `scripts/loop.md`.

## Kept-open frontier

Passing individual families cannot establish their full composition. Missing
evidence remains for nonunit/partial ADL and effective rounding at the shared cap;
direct cross-zero amid competing disjoint pairs; combined fees, funding, elapsed
maintenance/rate liabilities and PnL; partial matcher fills; mixed lifecycle or
Recovery states; larger/shared pair graphs, multiple constrained claim tables and
multiple capped assets at maximum simultaneous portfolio/oracle/account shapes.
The unit-ADL reset witness cannot substitute for the nonunit case. The funding
witness's lock alternative cannot prove cap-specific rejection. Full source-table
coverage does not establish every future backing/receipt/exit-resource obligation.
None of these untested combinations is asserted to be an exploitable bug.

## Reproduction

Reused default-feature wrapper SBF SHA-256:
`87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.
Its source checkout `/dev/shm/percolator-public-gap-20260916-c91e` has identical
`src/`, `Cargo.lock` and auth-matcher source; the manifest difference only adds
host-test `syn` features `extra-traits` and `visit`. Auth-matcher SHA-256:
`50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
Neither SBF artifact was rebuilt. The host harness was rebuilt in a private copy
of the existing target. Logs: `/dev/shm/percolator-inv058-row427-20260917-logs/`.

Setup (worktree creation from the supplied repository; other commands inside it):

```bash
git worktree add -b audit/inv-058-row427-20260917 /dev/shm/percolator-inv058-row427-20260917 origin/main
cp -a --reflink=auto /dev/shm/percolator-public-gap-20260916-c91e-host-target /dev/shm/percolator-inv058-row427-20260917-host-target
mkdir -p tests/fixtures/auth_matcher/target/deploy /dev/shm/percolator-inv058-row427-20260917-logs
cp /dev/shm/percolator-main-merge-20260916/tests/fixtures/auth_matcher/target/deploy/auth_matcher.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
export CARGO_TARGET_DIR=/dev/shm/percolator-inv058-row427-20260917-host-target
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-public-gap-20260916-c91e-sbf-target/deploy/percolator_prog.so
sha256sum "$PERCOLATOR_FUZZ_SBF" tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
```

Exact selector invocation (variables abbreviate the expanded original command):

```bash
m=inv_058_cumulative_position_oi_notional_and_rate_limit_integrity
h=$m::atomic_oi_fee_handoff
a=$h::multi_asset
T1=$m::v16_program_distinct_owner_pairs_cannot_cross_shared_side_oi_cap
T2=$m::v16_program_funding_accrual_does_not_open_shared_side_oi_headroom
T3=$m::liquidation_lifecycle::v16_program_liquidation_reset_reopen_reuses_capacity_and_preserves_live_notional
T4=$h::v16_program_disjoint_pair_oi_handoff_preserves_fees_across_transaction_partitions
T5=$h::v16_program_cpi_disjoint_pair_oi_handoff_rolls_back_matcher_and_stock_across_routes
T6=$a::v16_program_two_asset_oi_fee_handoff_is_atomic_across_clear_resize_and_route_switch
T7=$a::generated_side_oi_composition::v16_program_generated_existing_pair_side_oi_caps_compose_across_split_merge_routes
T8=$a::existing_leg_fee_competition::v16_existing_pairs_compete_for_fee_bearing_side_headroom_across_pair_order
T9=$a::pnl_terminal_handoff::v16_program_capped_pair_pnl_survives_headroom_handoff_and_ranked_terminal_payout
T10=$a::capacity_claim_composition::v16_program_full_source_claims_compose_with_side_oi_handoff_and_terminal_exit
M1=$m::v16_row427_metadata_retains_distinct_owner_side_oi_conformance
M2=$m::v16_program_side_oi_cap_witness_roster_is_source_complete
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  "$T1" "$T2" "$T3" "$T4" "$T5" "$T6" "$T7" "$T8" "$T9" "$T10" "$M1" "$M2"
cargo test --locked --offline --test v16_program_fuzz_regressions \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming -- --exact --nocapture
git diff --check
git diff --exit-code c7b5ae07f9a7a36f3ac6f9d1309bb2a66b45ff50 -- src Cargo.toml Cargo.lock tests/invariants/cu tests/invariants/coverage_reopenings.tsv tests/invariants/invariant_status.tsv
git diff --cached --check
git show --format= --check HEAD
```

Whitespace and unchanged production/dependency/test/status comparisons pass.
Only this audit and the README entry change. Rust/TSV formatting is inapplicable
to these Markdown edits. Compilation reports existing unused-support and Solana
future-compatibility warnings. Main is untouched and the commit is local only.
