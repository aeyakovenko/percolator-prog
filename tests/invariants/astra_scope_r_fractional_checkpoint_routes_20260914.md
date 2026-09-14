# Scope R: Fractional Funding Checkpoint Route Conformance

Base: `e527392517542b10a64d0ef303f346b0898509ae`, the local
`origin/codex/astra-open-holdout-ledger-20260912` tip at worktree creation.
Branch: `codex/pr135-scope-r-defensive-carry-20260914`.
Fresh isolated worktree: `/tmp/percolator-pr135-scope-r-20260914`.
All source edits, build outputs and index operations belong to this worktree.
Protected checkouts were read only. No production, fixture source, dependency,
wire or machine-status change is included.

At the time of this scope, rows **425 and 426 remained OPEN** and no implementation
/ invariant mismatch was established. The later row-425 canonical-accrual
regression in [README.md](README.md#row-425-canonical-accrual-carry-closure-2026-09-14)
and row-426 current-Hybrid rescue regression in
[README.md](README.md#row-426-current-hybrid-rescue-closure-2026-09-14)
now cover both rows. This scope remains one bounded public LiteSVM product for
INV-045/038/085/086, not a whole-invariant closure.

## Ownership Review

Read the Scope I/C/H/O/V audits, invariant charter, README, reopening comments,
`fractional_reset_histories`, `generated_fractional_routes`,
`paid_origin_routes`, and retained/overlapping funding-checkpoint owners before
implementation. The existing selectors are retained.

| Existing owner | Already covered | Increment here |
| --- | --- | --- |
| Scope I fractional resets | Fractional K/F, moving targets, four transports, owner cadence; no pending funding checkpoint | Fractional owners retain a signed reduction while the old funding mark remains owed |
| Generated fractional routes | Fixed targets, separate K/F residues and settlement frontiers | Target replacement creates a future funding activation, with interrupted activation and retained CPI |
| Retained funding retry | Integral lots, one boundary, bilateral two-leg batch | Fractional lots, boundaries 5/7, all four transports, successful activation rollback |
| Overlapping checkpoints | Integral lots, repeated replacement/cancellation | Fractional settlement and owner residue attribution; no additional replacement queue |
| H/C arrival and moving resets | Integral exposure, bigint price envelopes, no funding | No duplicate arrival or bigint boundary selector |
| I paid origin, O current Hybrid, V Hybrid rewards | Discovery/reward provenance and external observation completeness | Adjacent controls only; no new row 426 observation coverage is claimed |

The new file is a child of `generated_fractional_routes`. Its ledger gains a
separate input-owned active funding mark and optional activation boundary.
Caught-up publications keep their original behavior. The existing retained
executor is exposed within the carry family and reused without behavioral
changes; there is no duplicate rollback harness.

## Product And Oracle

The full Cartesian domain is two premium directions x two activation boundaries
(5/7) x CPI/bilateral x single/one-leg-batch x clean/interrupted = **32 histories**.
Asset publication, crank observation, flattening-leg and payout order reverse
with the batch choice; these ordering axes are not independently exhausted.

The existing public System/SPL/wrapper constructor creates two AuthMark assets
at 100/125, four owners with principal [100003,200009,300017,400037], and matched
lots [13,17]/[-13,-17]/[7,11]/[-7,-11]. SPL revokes mint authority at the fixed
1000066 supply. Public reductions at slot 0 remove 1/4 and 1/2 lot from the first
pair. Funding uses a saturated 10000-e9 rate cap, price movement is 24 bps/slot,
and each public market step accrues at most one slot.

At slot 2, carries are [4800,6000] and funding is nonzero. Owners sign a
3/8-lot asset-0 reduction, using the input-predicted eventual execution price.
CPI uses a system-created authenticated matcher and a publicly configured,
position-bound zero-fee grant. All delivery alternatives are signed before
publication. Their economic instruction bytes and account metas match; distinct
CU limits distinguish transaction signatures. Signature and 1232-byte packet
checks run on the retained envelopes and custom crank transactions.

Clock advances to 5 or 7 while the market frontier remains 2. Public mark
replacement reverses both targets and resets the price carry at frontier 2,
but preserves the old funding mark through the selected activation slot.
The ledger derives the whole-episode cap quotient/remainder from input anchors
and elapsed slots, then accumulates signed funding using the correct old/new
mark. Activation occurs after the boundary slot's funding charge. Four further
slots accrue under the replacement funding mark before final public flattening.

Each checked prefix compares deployed effective/raw prices, carry, K/F indices,
funding marks and pending checkpoint, unit ADL, zero B/pending obligations,
matched OI, and owner snapshot quantities. Each owner's input-derived ideal
numerator equals settled value plus latent K/F numerators plus that owner's
separate floor residues. Gross funding paid/received counters remain separate
by owner and side. Capital, positive PnL, fixed mint supply and SPL custody also
reconcile. No expected value, price, funding index, residue or payout is seeded
from an observed economic result.

The interrupted half of the domain contains **64 complete Account rollbacks**:

- 32 exact EngineStale rejections, before and after one partial public step.
- 16 successful checkpoint-activation prefixes followed by an invalid suffix.
- 16 successful retained reductions followed by an invalid suffix.

Error index and wrapper-success logs establish each successful prefix. Frames
retain complete Option<Account> values for tracked and compiled keys, including
owner/token accounts, matcher keys on CPI reductions, metadata and absence.
Only the exact runtime signature fee changes. Serialized retained transactions
are checked unchanged before delivery; the final signed alternative commits
with the original owner consent and matcher grant. These are distinct retained
signed envelopes, not replay of an already-recorded Solana signature.

The signed owners remain byte-identical during publication/catchup. A passive
owner remains byte-identical throughout the Live history until flattening.
Each reduction preserves the complete oracle profiles. The four route and two
retry variants agree exactly on price, carry, F, ideal owner numerators, each
owner's K/F residues and funding counters, and final payouts for each
direction/boundary pair. Boundary variants are different economic histories and
are not required to have equal payouts.

Bounded resolved close pays **128 exact owner entitlements**. Remaining vault
stock must equal the input-derived settlement residue, with zero insurance,
provider earnings, currently valid source backing and valid/impaired liens.
The product produces **2/3/5/6 residue atoms** and **48 separate-floor witnesses**.
It does not add terminal payout-suffix rollback coverage.

## Validation

Final exact run: **1/1 passes in 20.76s**, with 32 histories, 64 exact rollbacks,
128 payouts and peak **407529 CU** under the 600000-CU guardrail. The combined
inventory lists 12 selectors; the final new-selector inventory lists exactly
one test and zero benchmarks. Adjacent controls pass **11/11 in 140.75s**,
including all changed shared-ledger owners and the existing retained executor.
Their highest reported CU is 735816 in Scope O. There are no inherited failures.
All four metadata gates pass **4/4 in 0.01s**. Formatting and working/staged
whitespace checks pass; the local commit is also checked with git show.
Existing metadata-target dead-code and Solana future-compatibility warnings
remain. No full repository suite or engine/Kani proof is claimed.

The initial carry/owner product passed. An added assertion that every raw Fresh
bucket must be empty was too strong: the direction -1, slot-7 history retains
one atom tagged Fresh with expiry 107 at payout slot 112. Expiry is lazy; the
pinned engine's terminal slab step classifies that bucket for expiration.
The final check requires any such nonzero raw bucket to be expired at the
input payout slot and still requires zero valid/impaired liens and earnings.
Administrative normalization and physical slab retirement are not exercised.
This was an oracle scope correction, not a production mismatch or a relaxation
of the input-derived owner payouts and total custody residue.

The supplied default-feature Scope W SBF is used without rebuilding.
Scope W's production, Cargo files and matcher source compare identically to this
base. Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

| Artifact | SHA-256 |
| --- | --- |
| `/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so` | `79b298706ca7ffab09a017e5f37c4c0fc43d4e6292c092d126f4fbe8dff1b673` |
| Local copied `tests/fixtures/auth_matcher/target/deploy/auth_matcher.so` | `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93` |

A private executable 5-GiB tmpfs mounted at `target/host` provides the requested
/tmp CARGO_TARGET_DIR despite root filesystem pressure. A private copy of the
Scope I host dependency cache seeds it; test binaries compile from this worktree.
No shared target is written. Successful logs are
`target/scope-r-{list,final-list,exact,controls,metadata}.log`;
`target/scope-r-reserve-check.log` records the overstrict diagnostic assertion.

Commands from this worktree:

```sh
export CARGO_TARGET_DIR="$PWD/target/host" TMPDIR="$PWD/target/host"
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so
exact=inv_045_no_free_mark_movement::public_carry_order::generated_fractional_routes::fractional_checkpoint_routes::v16_program_fractional_checkpoint_retries_preserve_four_route_owner_residues
cargo test --locked --offline --test v16_cu -- --exact --list "$exact"
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 "$exact"
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_045_no_free_mark_movement::public_carry_order::generated_fractional_routes::v16_program_generated_fractional_kf_routes_preserve_carry_owner_value_and_residue \
  inv_045_no_free_mark_movement::public_carry_order::generated_fractional_routes::v16_program_fractional_owner_crank_cadence_preserves_carry_and_residue_adjusted_entitlement \
  inv_045_no_free_mark_movement::public_carry_order::generated_fractional_routes::fractional_reset_histories::v16_program_fractional_reset_histories_preserve_route_and_residue_adjusted_owner_value \
  inv_045_no_free_mark_movement::public_carry_order::funding_carry_entitlement::retained_funding_retry::v16_program_retained_reduction_preserves_carry_across_pending_funding_checkpoint \
  inv_045_no_free_mark_movement::public_carry_order::funding_carry_entitlement::checkpoint_replacement::v16_program_overlapping_target_replacements_preserve_carry_funding_and_resolved_payouts \
  inv_045_no_free_mark_movement::public_carry_order::funding_carry_entitlement::v16_program_funding_reversal_preserves_carry_and_unsettled_owner_entitlement \
  inv_045_no_free_mark_movement::public_carry_order::target_arrival_entitlement::v16_program_target_arrival_plateau_and_restart_preserve_interleaved_owner_entitlement \
  inv_045_no_free_mark_movement::public_carry_order::target_arrival_entitlement::moving_reset_routes::v16_program_repeated_moving_target_resets_preserve_carry_across_matcher_handoffs \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::retained_penalty_handoff::paid_origin_routes::v16_program_paid_origin_routes_preserve_old_penalty_and_fresh_reward_through_missing_tails \
  inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations::active_keeper_observations::cpi_keeper_observations::generated_current_hybrid::v16_program_generated_current_hybrid_recipient_routes_match_full_health_and_rollback \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::hybrid_recipient_provenance::v16_program_dual_hybrid_reward_lineage_survives_recipient_routes_and_payout
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --check --oneline HEAD
sha256sum "$PERCOLATOR_FUZZ_SBF" tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
```

The listing also includes every adjacent selector before execution.

## Remaining Limits

Two solvent AuthMark assets, four owners, unit ADL, zero trading/maintenance fees,
one pending replacement per asset, two checkpoint deadlines and fixed owner
settlement frontiers bound this product. Retained requests and batches select
one asset; final flattening is a bilateral two-asset batch. Partial inline
attempts reject; successful risk reduction follows complete public catchup.
No private engine transition or initialized economic Account injection is used.

The host oracle uses bounded u64/i128 arithmetic and compares its predictions
with the deployed SBF. This is narrow empirical INV-085/086 evidence, not a
full-width U256/bigint boundary corpus, Kani equivalence proof, shrinking fuzz
campaign or source-complete route theorem. Arbitrary funding queues, moving
anchors/arrival, ADL, retained multi-leg batch CPI, fees, independent source-credit
consumption/haircut attribution, other oracle/quote modes, maximum shapes, liquidation
reward cohorts and generic terminal lifecycle remain outside this increment.

All favorable trades here have completed AuthMark evidence. Missing unknown
Hybrid observations and recipient/liquidation evidence completeness remain
row 426 obligations; adjacent O/V/I successes do not close them. OPEN rows and
machine classifications remain unchanged. No marginal standalone rejection or
duplicate selector is retained.

## Files

- `cu/inv_045_fractional_checkpoint_routes.rs`: the single new public product.
- `cu/inv_045_generated_fractional_routes.rs`: input-owned funding checkpoints and registration.
- `cu/inv_045_funding_carry_entitlement.rs`: family-local retained executor visibility.
- `cu/inv_045_retained_funding_retry.rs`: family-local sign/reject visibility only.
- `README.md`: coverage summary and bounded interpretation.
- `coverage_reopenings.tsv`: comments only for rows 425/426.
- `astra_scope_r_fractional_checkpoint_routes_20260914.md`: this audit and validation commands.
