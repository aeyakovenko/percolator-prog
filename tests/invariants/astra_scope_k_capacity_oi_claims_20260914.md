# Astra Scope K: Source Capacity, Shared OI And Claim Identity

Base: `d01245dd993bfe60c2448360733c3bb63c41aa19`, the requested local
`origin/codex/astra-open-holdout-ledger-20260912` tip at checkout creation.
Branch: `codex/astra-scope-k-composition-20260914`.
Independent clone: `/tmp/percolator-scope-k-20260914`.
The clone shares existing Git objects read only; all edits, commits, host builds
and logs belong to this clone. Neither excluded checkout was edited.

## Selection And Prior Owners

Read `scripts/loop.md`, `INVARIANTS.md`, the invariant README and reopening ledger,
the requested INV-010/024/028/029/031/041/057/058/063/066/067/068/070/071/073/077/078/
082/086/088/089 owners and the U/W/R/F audits. The rows are coverage labels.
No external branch or historical implementation was used to construct this test.

| Existing Owner | Existing Product | Boundary Relevant Here |
| --- | --- | --- |
| U, INV-066/067/068 | Co-owned delayed receipts, repeated stock releases, fractional conversion and exact episode attribution | No shared OI cap or admission pressure |
| W, INV-028 | 22/24/26 historical sources, latent reservations, used generation and exact payout | Two owners; no disjoint-pair side-OI composition |
| INV-028 shared-source and resource siblings | Full claimant tables sharing a debtor, liens, Recovery and terminal latent materialization | Do not combine those tables with capped disjoint-pair handoffs |
| R, INV-063/070/071/088 | Expiry invalidates a saved scan prefix and enables earlier insurance recredit | No historical claim table or side-OI pressure at the scan boundary |
| F, INV-058 | Three unequal PnL claims through capped headroom transfer and exposed resolution | One source domain; no source-slot pressure or second post-handoff mark |
| INV-058 generated/fee siblings | Repeated directed handoffs and existing-pair fee competition | Fixed marks or zero PnL; no full source-table claimant |

The selected product joins W and F. Combining all four rows would additionally
need a generator for provider/insurance stock, haircut receipt identity and a
terminal scan after user claims finish. Those dimensions are not represented
by the solvent source-book fixture and are not inferred from its results.

## Public Matrix And Oracle

One new selector lives under INV-058:

`inv_058_cumulative_position_oi_notional_and_rate_limit_integrity::atomic_oi_fee_handoff::multi_asset::capacity_claim_composition::v16_program_full_source_claims_compose_with_side_oi_handoff_and_terminal_exit`.

Sixteen worlds independently cross initial position/mark direction, single versus
one-leg batch transport, the capacity-constrained pair as headroom donor/recipient,
and two coupled settlement/payout schedules. Six distinct owners each deposit
20000000000 atoms in a fixed-supply classic SPL market with fourteen asset slots.
System/SPL/ATA/wrapper instructions create all economic state. Program loading,
initial signer SOL, authenticated Clock warps and blockhashes are harness inputs.

Pair zero performs thirteen two-direction historical round trips. A one-atom
mark change on `1 + asset_index % 3` whole position units earns that many atoms
in each of the asset's two domains. Its claimant retains 26 detached sources
and 50 atoms of PnL; its counterparty has paid exactly 50 capital atoms into
source backing. All other owners retain their complete deposits.

The remaining asset admits three disjoint pairs contributing
`[cap/4, cap/4 + POS_SCALE, cap/2 - POS_SCALE]` to each side. Every account-local
position cap stays slack, while the aggregate reaches the side-OI cap. The
constrained portfolio's historical source set union both possible settlement
domains is exactly 28, although both new domains initially remain unoccupied.
After the first opening, a permissionless current-slot observation catches up
the previously empty asset before its peers enter. It advances the asset's
accrual slot and preserves the exact owner/domain book.

A one-atom AuthMark change settles three unequal claims. The constrained pair
gives or receives two whole position units of headroom, with the receiver using
the opposite single/batch route. Earlier claims remain unchanged as quantities
change. All three pairs then close and reopen in the opposite direction at the
current price, with each pair using both single and batch routes. A reverse
mark movement earns the input-derived claim for each resized position in the
other domain. The constrained claimant now has all 28 source records occupied.
All six positions remain open and aggregate OI remains exactly at the cap.

The independent book derives claims and losses from submitted quantities and
mark changes. It checks portfolio provenance, owner, incarnation, live position
epochs, exact source generation/face, source bounds and backing, capital, PnL,
live quantities, side counts, OI, fixed mint supply and SPL custody. Independent
stock, reservation and source-rate censuses supplement the book. Crank prefixes
receive the stock/reservation census; after each completed observation every
owner must match its input-derived claim and loss. A one-atom transfer between
two decoded owners in the same source domain preserves both domain aggregate
and custody but fails the same claim predicate used by the live oracle.

After authority resolution and the configured five-slot owner window, payer-only
CloseResolved calls detach the cohort, retire each owner's sources and pay its
exact entitlement. A flat claimant waits while peers still have exposure. Every
accepted terminal call strictly decreases the selected account's lexicographic
rank `(active legs, occupied sources, unpaid entitlement)`. At most 31 cohort
sweeps are allowed; every live observation has a four-call settlement bound.
Positive sources can disappear only with the owner's matching PnL/capital/token
accounting, and each domain's remaining backing equals its outstanding claims.
Consumed backing and the matching source spent/receivable counters must equal
the input-derived original stock less those remaining claims. Source-to-capital
conversion retains those records; no provider deposit or earnings payout exists
in this fixture.

Final winner gains are `[50000048, 50000004, 99999998]` when the constrained pair
donates headroom and `[50000052, 50000000, 99999998]` when it receives headroom.
Each loser pays the corresponding gain. Every owner receives its deposit plus
or minus that amount; direction, route and schedule do not change the endpoint.
Subsequent empty-portfolio deletion is owner-signed. Vault, capital, claims,
OI and portfolio count finish at zero. The test ends before slab retirement.

Each admission, the release/refill group and every terminal call is first paired
with an ordinary failing System suffix. Complete tracked and compiled Accounts
roll back, including custody, portfolio/source state, mint and bystanders;
only the exact payer signature fee is deducted. Logs require every intended
wrapper prefix to have succeeded. Paying terminal prefixes include real SPL
transfers. Unchanged instruction bytes and metas retry with fresh blockhashes.
This does not test retained serialized signatures or duplicate-signature replay.

## Remaining Dimensions

| Row | Added Evidence | Exact Remaining Product |
| --- | --- | --- |
| 417, OPEN | Owner/domain claims survive headroom reassignment, another mark and terminal source-to-capital/token reclassification | Shared-owner embedded receipts, fractional haircuts, delayed receipt creation, later expiry/insurance stock changes and their full ordering product |
| 423, OPEN | One full historical claimant table composes with disjoint-pair capped admission, both latent domains, shared-source realization and exact exit | Multiple constrained tables, asymmetric/shared debtors, arbitrary occupancy and asset reuse, provider liens, reserve availability, bankruptcy/Recovery and other future resources |
| 424, OPEN | No new persisted-scan evidence | Combine receipt completion and source-resource discharge with time/custody/insurance actionability changes, earlier-prefix invalidation, repeated rescans and final administrative retirement |
| 427, OPEN | Unequal PnL and full source-table pressure through capped single/batch handoff, close/reopen reversal and terminal payout | Direct cross-zero at cap, CPI/multi-leg routes, arbitrary pair graphs, nonunit ADL, fractional marks/carry, fees/funding/rates, mixed lifecycles, multiple capped assets and maximum composite shapes |

INV-058 owns the composition; INV-028 receives bounded admission/resource evidence.
Related checks cover INV-010/024/029/031/041/057/067/071/073/077/078/082. No new
INV-063/066/068/070/086/088/089 closure is claimed. All statuses remain sampled
and unchanged. Fourteen market slots and one full 28-record source table do not
establish maximum-shape compute across fourteen simultaneous legs, oracle tails,
multiple tables or the 10 MiB market configuration.

The fixture uses six distinct owners/ATAs, one constrained claimant, integral
AuthMark PnL, unit ADL, zero fees/funding and fully solvent debtors. Source backing
remains Fresh through every tested prefix. No provider/insurance claim, haircut
receipt, missing destination, authority succession, native/dual quote, Recovery,
forfeit, source generation replacement or new collateral after setup is present.
Historical opening uses the bilateral single route. Only two claimant schedules
are tested; live and terminal schedules are coupled. Economic payout needs a
funded payer; resolution and mechanical deletion retain their stated signers.

## Artifact And Validation

Production is unchanged. Reused fixed artifact:
`/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so`.
SHA-256: `79b298706ca7ffab09a017e5f37c4c0fc43d4e6292c092d126f4fbe8dff1b673`.
The artifact source checkout has no diff against this base for `src/`,
`Cargo.toml` or `Cargo.lock`. Engine pin:
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
The existing auth matcher is copied into this clone's standard fixture path;
SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
The fixture constructs matcher accounts but the new selector makes no matcher CPI.

A private 6 GiB tmpfs at `target/` contains a copy of the Scope W host cache and
all local build output. The initial host build succeeded. Draft iterations
corrected two fixture assumptions: the inherited one-slot accrual cap required
more than four catch-up calls, and newly exposed old-slot asset state needed a
public clock-only refresh before another pair could increase risk. The final
fixture uses W's 64-slot accrual setting and that public refresh. A draft direct
one-leg-batch reversal at the saturated side cap returned EngineInvalidLeg;
the final test uses the bounded close/reopen continuation. It does not establish
direct at-cap cross-zero route equivalence. These test construction changes do
not supply a public fund-loss or no-continuation result, and no production
correction or pre-fix/fixed-head experiment is claimed. A terminal oracle draft
also incorrectly expected consumed backing to vanish; the final oracle binds
that stock to exactly the original claims less their still-outstanding face.

- New exact selector: PASS, 1/1, 55.20s. All 16 worlds reach the full 28-source
  table. There are 2064 checked submissions, 664 exact rollbacks, 600 accepted
  ranked terminal calls and 96 rolled-back SPL-paying prefixes. The 1400
  committed submissions include 96 final economic payouts. Peak measured CU is
  1180147, below the explicit 1375000 bound and transaction ceiling of 1400000.
  Maximum measured packet size is 976 bytes, below 1232.
- Counters and packet measurements cover the explicit campaign sender. CU also
  includes mark publication, clock refresh, settlement, resolution and portfolio
  deletion helpers. Public fixture construction and configuration are excluded.
  This is a maximum-source-table measurement, not maximum composite shape.
- Metadata gates: PASS, 4/4, 0.01s. The exact selector listing returns one test.
  Formatting and working diff checks pass; source/dependency/verdict diffs are
  empty, and ignoring comments leaves no reopening-ledger diff.
- Adjacent controls: PASS, 8/8, 140.35s. No inherited control failures. Peaks:
  W 1152561 CU; shared-source late claimant 930572; Recovery bound 292554;
  existing-pair fees 344399; generated OI 811195; F 453650; U 378873; R 222888.
  These are separate control measurements, not new Scope K coverage counts.
- Staged diff and post-commit show checks are required for the local commit.
  No full unfiltered suite or Kani run is claimed. Existing host support
  dead-code and Solana future-compatibility warnings remain.

## Exact Commands

All behavioral commands run from `/tmp/percolator-scope-k-20260914` with:

```sh
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export TMPDIR=/tmp/percolator-scope-k-20260914/target
export PERCOLATOR_FUZZ_SBF=/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
cargo test --locked --offline --test v16_cu inv_058_cumulative_position_oi_notional_and_rate_limit_integrity::atomic_oi_fee_handoff::multi_asset::capacity_claim_composition::v16_program_full_source_claims_compose_with_side_oi_handoff_and_terminal_exit -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_058_cumulative_position_oi_notional_and_rate_limit_integrity::atomic_oi_fee_handoff::multi_asset::capacity_claim_composition::v16_program_full_source_claims_compose_with_side_oi_handoff_and_terminal_exit -- --exact --list
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_058_cumulative_position_oi_notional_and_rate_limit_integrity::atomic_oi_fee_handoff::multi_asset::pnl_terminal_handoff::v16_program_capped_pair_pnl_survives_headroom_handoff_and_ranked_terminal_payout \
  inv_058_cumulative_position_oi_notional_and_rate_limit_integrity::atomic_oi_fee_handoff::multi_asset::generated_side_oi_composition::v16_program_generated_existing_pair_side_oi_caps_compose_across_split_merge_routes \
  inv_058_cumulative_position_oi_notional_and_rate_limit_integrity::atomic_oi_fee_handoff::multi_asset::existing_leg_fee_competition::v16_existing_pairs_compete_for_fee_bearing_side_headroom_across_pair_order \
  inv_028_source_domain_realizability_cap::historical_latent_capacity::generation_capacity_admission::v16_program_used_generation_admission_reserves_latent_capacity_through_exact_exit \
  inv_028_source_domain_realizability_cap::historical_latent_capacity::shared_source_late_exit::v16_program_shared_history_preserves_late_claimant_capacity_and_exact_exit \
  inv_029_positive_claim_bounds_never_understate::v16_program_recovery_half_close_preserves_one_atom_claim_bound_through_resolved_payout \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::claim_episode_materialization::v16_program_coowned_claim_episodes_survive_deferred_receipts_and_two_expiry_rollbacks \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_scan_recredit::v16_program_terminal_scan_rediscovers_earlier_insurance_after_later_expiry
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --check --oneline HEAD
git diff --exit-code d01245dd -- src Cargo.toml Cargo.lock tests/invariants/open_findings.tsv tests/invariants/invariant_status.tsv
git diff --exit-code --ignore-matching-lines='^#' d01245dd -- tests/invariants/coverage_reopenings.tsv
git -C /run/percolator-pr135-scope-w-20260913 diff --exit-code d01245dd -- src Cargo.toml Cargo.lock
sha256sum "$PERCOLATOR_FUZZ_SBF" tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
```

Local logs are `target/scope-k-build.log`, `target/scope-k-selector.log`,
`target/scope-k-controls.log`, `target/scope-k-metadata.log`, and
`target/scope-k-list.log`. Setup used `git clone --shared --no-checkout`,
`git switch -c ... d01245dd993bfe60c2448360733c3bb63c41aa19`, a private tmpfs
mount, and copies of the existing host cache/matcher. No SBF was rebuilt.

Changed paths, all beneath `tests/invariants/`: this audit, `README.md`,
comment-only `coverage_reopenings.tsv`, `cu/inv_058_capacity_claim_composition.rs`,
its mount and configurable public fixture constructor in
`cu/inv_058_multi_asset_oi_fee_handoff.rs`, and visibility of the shared public
payout instruction builder in `cu/inv_058_pnl_terminal_handoff.rs`.
