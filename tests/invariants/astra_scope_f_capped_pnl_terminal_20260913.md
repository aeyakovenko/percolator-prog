# Astra Scope F: Capped PnL Through Terminal Payout

Base: `d346cc90cc4a473b9b55664dc15ab8af7661ceef`, the current fetched tip of
`origin/codex/astra-open-holdout-ledger-20260912` at checkout creation.
Branch: `codex/astra-scope-f-terminal-composition-20260913`.
Independent shared-object clone: `/tmp/percolator-astra-scope-f.LbYHjL`.
Neither excluded checkout was edited. All edits, host builds and logs are in
this clone; the existing matching SBF is read only. No production change.

## Prior Coverage And Selection

Read `scripts/loop.md`, the coverage ledger and invariant README, the requested
INV-010/024/028/029/031/041/057/058/063/066/067/068/070/071/073/077/078/082/086/088/089
obligations and existing test families, including Scope U/W/R and the existing
side-OI selectors. The four rows are coverage labels, not test specifications.

- Scope U combines co-owned delayed receipt creation with two backing expiries.
  Existing INV-066/067/068 controls also cover fractional conversion, late
  materialization, recipient replacement, and repeated stock reclassification.
- Scope R requires the scanner itself to rediscover earlier insurance after
  later expiry resets the persisted cursor. Existing INV-070/071/088 controls
  cover source deadlines, external custody, local recredit and cohort readiness.
- Scope W combines used-generation admission with 22/24/26 historical domains,
  remaining latent capacity and exact exit. Source/lien, Recovery, active-leg,
  shared-debtor and resource-reservation siblings cover other bounded products.
- INV-058's generated three-pair history covers repeated existing-leg headroom
  transfers at fixed marks. Its fee competition and multi-asset/CPI siblings
  cover nonzero trading fees with zero PnL. The liquidation lifecycle sibling
  covers reset/recreation with fixed-price OI accounting.

The selected increment is three unequal, already-earned claims through capped
existing-leg handoff and resolution while exposure remains open. Current
position quantity changes independently of the earlier owner's claim. This is
one new INV-058 selector, mounted in the existing public six-owner fixture.
Existing negative cap tests remain adjacent controls; this selector supplies
successful nonzero-PnL continuation rather than another cap-plus-one matrix.

## Guarantee And Oracle

Eight worlds cross both position/mark directions, single versus one-leg batch
bilateral opening/release (the refill uses the opposite transport), and two
coupled live-settlement/terminal schedules. Three pairs initially contribute
`[cap/4, cap/4 + POS_SCALE, cap/2 - POS_SCALE]` to each side. Every account is
strictly below its own position cap; aggregate OI equals the side cap.

Each of six distinct owners deposits 20000000000 atoms. Public AuthMark moves
100 to 99 or 101 at slot 1, with the position direction mirrored to keep the
even-index owners as winners. The input-derived gains are
`[25000000, 25000001, 49999999]`, with equal opposite losses. Settlement either
starts with winners or debtors; each owner has at most four public crank attempts.
The first pair releases two whole position units and the second existing pair
uses that headroom, returning aggregate OI to the cap. Their earlier claims
stay unchanged despite the new quantities and position epochs.

The oracle reads portfolio provenance/incarnation, exact source domain and
generation, source face, capital, PnL, tokens, raw quantities, stored side counts,
OI, fixed mint supply, vault and insurance. It separately checks the input-owned
claim vector, independent market stock/reservation censuses and source credit
rates. A read-only swap of two observed owner totals conserves their aggregate
but fails the same vector equality. No economic state is injected. These fully
backed claims realize before payout and leave no embedded haircut receipt at
checked prefixes; this is not new receipt-materialization evidence.

Resolution preserves every portfolio Account and leaves all six legs open.
After the configured five-slot owner window, payer-only `CloseResolved` calls
detach the cohort and return each exact entitlement. A waiting flat positive
claimant is revisited after its peers detach. Every successful call strictly
decreases `(active leg count, occupied source count, unpaid entitlement)` for
the selected portfolio; four full cohort sweeps bound economic disposition.
Unselected complete Accounts are framed through these calls. Subsequent empty
portfolio deletion is owner-signed and is not called permissionless.

All worlds end with the same owner payouts:
`[20025000000, 19975000000, 20025000001, 19974999999, 20049999999, 19950000001]`.
Vault, capital, source bounds, OI and materialized portfolios are zero. The
mint supply is 120000000000 with no mint authority. No forfeit or later deposit
is needed. The test ends after portfolio deletion, before slab retirement.

Each handoff and every terminal call is first followed by a failing ordinary
System transfer. Logs require every wrapper prefix to have succeeded, including
real SPL-paying prefixes. Complete tracked and compiled Accounts roll back,
with only the exact payer signature fee deducted. Unchanged instruction bytes
and metas then retry in fresh transactions. Retained serialized transaction
signatures and duplicate-signature replay are outside this test.

## Limits And Row Impact

INV-058 owns this successful cap/PnL/terminal composition. Adjacent bounded
evidence concerns INV-010/024/028/029/031/041/057/067/071/073/077/078/082: attribution,
source consumption, order, atomic retry and finite economic progress. None is
promoted. INV-058 remains `REFUTED_CURRENT / TRANSITION_SYSTEM / PUBLIC_ROUTE /
SAMPLED / GLOBAL_CONDITIONAL_TCB`. All data rows and machine statuses are unchanged.

Row 427 remains OPEN. Rows 417/423/424 also remain OPEN: this test adds no later
stock reclassification, persisted scan invalidation or generic admission-resource
closure. It has one exposed asset, one fully backed source domain, three pairs,
classic SPL, integral one-step AuthMark PnL, unit ADL and zero funding/fees.
It does not combine PnL with CPI, multi-asset growth, arbitrary handoff graphs,
fractional carry, new mark movement after handoff, bankruptcy, Recovery,
provider/insurance backing, expired stock, receipt haircuts, alternate quote
rails, unavailable owners/destinations, elapsed liabilities/rates or maximum
storage shapes. Mark direction and economic winner are coupled; settlement and
payout orders are coupled. Resolution needs the market authority; economic
payout after the owner window needs only a funded transaction payer.

## Artifact And Validation

Reused fixed default-feature wrapper:
`/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so`.
SHA-256: `79b298706ca7ffab09a017e5f37c4c0fc43d4e6292c092d126f4fbe8dff1b673`.
The entire `src/` tree compares equal to that artifact's source checkout;
`Cargo.toml` and `Cargo.lock` hashes also match. Engine pin:
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
The existing auth matcher is copied into this clone's standard fixture path,
SHA-256 `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
The fixture constructs matcher accounts but the new selector uses no matcher CPI.

A private executable 6 GiB tmpfs at `target/` holds a copy of the existing host
dependency cache and all new compilation output. There is no SBF rebuild because
production is unchanged. The first host draft had a u16/u32 assertion mismatch;
the first terminal draft tried to finish a waiting winner before detaching its
peers and received the documented EngineNonProgress. Correcting the test's
continuation schedule resolves that fixture assumption. No production conformance
mismatch or pre-fix/fixed-head implementation experiment is claimed.

- New exact selector: PASS, 1/1, 5.44s; 8 worlds, 160 checked campaign submissions,
  76 exact rollbacks, 68 successful ranked terminal calls, 48 committed payouts
  and 48 rolled-back SPL-paying prefixes. Peak measured CU: 453650, below 900000.
  Packet size is checked <=1232 bytes for the explicit campaign sender.
  Counters exclude fixture construction, opening/mark/settlement/resolve helpers
  and owner deletion; peak CU includes those later helpers but excludes setup.
- Adjacent controls: PASS, 6/6, 118.05s. Scope W's 24 worlds peak at 1152561 CU;
  the INV-029 Recovery-claim control peaks at 292554 CU; the 16-world fee
  competition peaks at 348859 CU; the 16-world generated OI control peaks at
  802195 CU; Scope U's 12 worlds peak at 372873 CU; Scope R's 16 worlds peak at
  221544 CU. These are control measurements, separate from the new selector.
- Metadata gates: PASS, 4/4, 0.01s. The exact selector listing returns one test.
  Host compilation, formatter and whitespace checks pass. Production and
  machine-verdict diffs are empty; ignoring comments leaves no ledger diff.
  Staged diff and post-commit `git show --check` are required for the local commit.
- No full unfiltered suite or Kani run. Existing host dead-code warnings and
  Solana future-compatibility warnings remain.

## Exact Commands

Commands run from `/tmp/percolator-astra-scope-f.LbYHjL`. Each Cargo execution
used these equivalent environment assignments:

```sh
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export TMPDIR=/tmp/percolator-astra-scope-f.LbYHjL/target
export PERCOLATOR_FUZZ_SBF=/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
cargo test --locked --offline --test v16_cu inv_058_cumulative_position_oi_notional_and_rate_limit_integrity::atomic_oi_fee_handoff::multi_asset::pnl_terminal_handoff::v16_program_capped_pair_pnl_survives_headroom_handoff_and_ranked_terminal_payout -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_058_cumulative_position_oi_notional_and_rate_limit_integrity::atomic_oi_fee_handoff::multi_asset::pnl_terminal_handoff::v16_program_capped_pair_pnl_survives_headroom_handoff_and_ranked_terminal_payout -- --exact --list
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_058_cumulative_position_oi_notional_and_rate_limit_integrity::atomic_oi_fee_handoff::multi_asset::generated_side_oi_composition::v16_program_generated_existing_pair_side_oi_caps_compose_across_split_merge_routes \
  inv_058_cumulative_position_oi_notional_and_rate_limit_integrity::atomic_oi_fee_handoff::multi_asset::existing_leg_fee_competition::v16_existing_pairs_compete_for_fee_bearing_side_headroom_across_pair_order \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::claim_episode_materialization::v16_program_coowned_claim_episodes_survive_deferred_receipts_and_two_expiry_rollbacks \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_scan_recredit::v16_program_terminal_scan_rediscovers_earlier_insurance_after_later_expiry \
  inv_028_source_domain_realizability_cap::historical_latent_capacity::generation_capacity_admission::v16_program_used_generation_admission_reserves_latent_capacity_through_exact_exit \
  inv_029_positive_claim_bounds_never_understate::v16_program_recovery_half_close_preserves_one_atom_claim_bound_through_resolved_payout
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --check --oneline HEAD
git diff --exit-code d346cc90 -- src Cargo.toml Cargo.lock tests/invariants/open_findings.tsv tests/invariants/invariant_status.tsv
git diff --exit-code --ignore-matching-lines='^#' d346cc90 -- tests/invariants/coverage_reopenings.tsv
sha256sum "$PERCOLATOR_FUZZ_SBF" tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
```

Logs: `target/scope-f-build.log`, `target/scope-f-selector.log`,
`target/scope-f-controls.log`, `target/scope-f-metadata.log`, and
`target/scope-f-list.log`. These are local generated artifacts.

Changed paths: this audit, `README.md`, comment-only `coverage_reopenings.tsv`,
`cu/inv_058_pnl_terminal_handoff.rs`, and its mount in
`cu/inv_058_multi_asset_oi_fee_handoff.rs`, all beneath `tests/invariants/`.
