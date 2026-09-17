# INV-038/039 funding and loss-attribution audit, 2026-09-17

Base: fetched `origin/main`, `a4b5fad85592d764214a696dc706944884b3d33e`.
Worktree: `/dev/shm/percolator-funding-loss-audit-20260917`.
Branch: `astra-ultra/funding-loss-audit-20260917`.
Scope: rows **253/254/255/271/272/273/360/365/380/419/435**, with adjacent
INV-024/025/037 accounting only where it constrains these witnesses.

## Decision and evidence

All eleven named families already have mounted public-route witnesses. Review of
the current invariant charter, discovery/reopening/status ledgers, executable
assertions, shared helpers and harness mounts found no distinct missing witness
for these rows. This audit adds an assertion-boundary map and verification record;
no duplicate test, production fix, dependency change or status promotion is needed.
No withheld patch or external reproduction was used.

The exact selectors are listed in the commands below. The fixed `prNNN` tests
supplement the finding-agnostic stateful owners in `independent_discoveries.tsv`.

| Row(s) | Existing witness and nonzero assertion | Boundary |
| --- | --- | --- |
| 253 | INV-038 `pr253`: a stationary rounded mark retains nonzero premium funding. Omitting its selected observation rejects with `NonProgress` and exact tracked rollback; observed recovery restores both funding indices and the same owner payouts. | Fixed mark/size/cap; not arbitrary omitted-observation schedules. |
| 254 | INV-039 `pr254` and shutdown catch-up: shutdown-first and explicit-settlement-first book identical nonzero funding and terminal payouts. A separate stale shutdown rejects exactly, progresses within 16 public catch-up calls, then succeeds and pays 2,000,000 atoms. | Fixed unchanged effective price isolates committed funding; no arbitrary lifecycle history. |
| 255 | INV-039 `pr255` and pending-zero-move terminal catch-up: early resolution rejects exactly, stored-state public catch-up permits resolution, and terminal value matches the committed control. The zero-move case requires nonzero funding and at least two catch-up steps. | Two terminal boundaries; at most 16 catch-up calls per witness. |
| 271/272/273 | INV-039 `pr271`/`pr272`/`pr273`: CPI/batch-CPI close, unilateral reduction and Recovery forfeit preserve the nonzero balanced funding transfer. The multi-segment selector adds rejection, exact rollback, public catch-up and successful retry across all four operation kinds. | The shared stateful oracle permits Recovery terminal floor residue only with no destination gain and at most one lost atom per positive claimant, two atoms total. This is not erased funding. |
| 360 | Stateful INV-027 stale-cohort matrix: all four trade transports reject novation while one historical loser is stale, before negative PnL is booked. Owner reduction remains available; finite settlement leaves the entrant's portfolio and tokens untouched. A settled, sufficiently funded control admits the transfer. | Discovery metadata owns this row under INV-039; its executable mount is INV-027. The successful control uses different deposits, not an identical-request retry of the rejected world. This audit does not expand INV-027 scope. |
| 365 | INV-038 `pr365`: repeated max-dt cranks accumulate fractional movement from price 100 to target 1. Resolved payouts are exactly 999,901/1,000,099 atoms, conserving supply and the pair's 2,000,000 atoms. | One 24-bps cap / 20-slot max-dt case, bounded to 200 crank attempts; not all fractional operands. |
| 380 | INV-039 `pr380`: all four trade transports preserve elapsed funding indices and the unrelated victim's payout across prospective EWMA updates; paid stamp fees are nonzero. | Equal coalition/total payouts are required for fixed-price no-CPI routes. CPI matcher quotes can differ between schedules, so blanket endpoint equality would be incorrect. |
| 419 | CU Recovery-to-Resolved ordering: a zero-basis winner retains loss weight until its opposing debtor settles. Winner-first and debtor-first must both pay the exact five-owner vector `[12, 12, 0, 12, 0]`, clear all target weights/counts/OI and empty the vault. | Distinct owners, fixed two-asset setup; not the same-portfolio mixed-role case. |
| 435 | CU `shared_holder::mixed_roles`: one portfolio is both creditor and another domain's debtor. An input-derived owner ledger preserves original debt, pending weight, receipts and paid value through both close orders, late transaction rollback, ten exact payouts and deletions. | Fixed price-debt case. The separate `funding_resolution` selector adds four nonzero-funding worlds with exact per-owner endpoint equality across orders, custody conservation, waiting rollback and bounded deletion; that differential oracle is not an independent arbitrary funding-entitlement proof. |

The adjacent pending-holder deposit selector uses both sides of a public active
close. A signed seven-atom SPL deposit raises only the holder's capital and custody;
its zero-basis/nonzero-weight leg, PnL, market obligations and debtor close partition
remain exact. This enforces INV-037's distinction between retained pending state
and the disjoint close ledger, with INV-024/025 value/stock checks already present
in the scoped witness helpers. Pending weight is not an additional residual credit.

Source owners: [INV-038 regressions](public_sbf/inv_038_rounding_and_ratio_conservation.rs),
[INV-039 regressions and metadata guard](public_sbf/inv_039_pending_loss_obligation_durability.rs),
[INV-039 stateful](stateful/inv_039_pending_loss_obligation_durability.rs),
[row-360 matrix](stateful/inv_027_protected_principal_seniority.rs),
[INV-039 CU](cu/inv_039_pending_loss_obligation_durability.rs),
[mixed-role owner ledger](cu/inv_039_pending_loss_mixed_roles.rs),
[mixed funding](cu/inv_039_mixed_role_funding_resolution.rs),
[shared discovery implementations](../support/invariant_discovery.rs), and
[fixed regression helpers](../support/fuzz_model.rs).
The three harnesses mount these directly or through INV-039's `shared_holder`
and `mixed_role_resolution` children. The existing metadata guard prevents
row 419's distinct-owner scenario from replacing row 435's mixed-role evidence.

At this base, INV-038/039 are `SUPPORTED` with `PUBLIC_ROUTE / SAMPLED` evidence;
rows 419/435 are `COVERED` in `coverage_reopenings.tsv`. Older comments and dated
audits still say `OPEN`; they do not override the current machine rows. Bounded
coverage does not establish arbitrary funding/loss/ADL/receipt histories or new
engine pins. The mount census establishes availability, not assertion strength.

## Exact verification

Host harnesses are compiled in a private copy of the existing dependency target.
Reused default-feature SBF SHA-256:
`87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.
Its source base `cb236b5248c941ffcb33e1e6afe62801e8a19712` has identical `src/`,
`Cargo.lock` and matcher source; the manifest difference is host-only `syn`
features. Engine pin: `94979ede7db934545e53a8f210dd063a9ea3ea63`.
Reused matcher SHA-256:
`50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
Neither SBF artifact was rebuilt.

Commands run from the isolated worktree, after `git fetch origin main` and
`git worktree add -b astra-ultra/funding-loss-audit-20260917 /dev/shm/percolator-funding-loss-audit-20260917 origin/main`
in the original repository:

```bash
cp -a --reflink=auto /dev/shm/percolator-public-gap-20260916-c91e-host-target /dev/shm/percolator-funding-loss-audit-20260917-host-target
mkdir -p tests/fixtures/auth_matcher/target/deploy /dev/shm/percolator-funding-loss-audit-20260917-host-target/deploy
cp /dev/shm/percolator-public-gap-20260916-c91e-sbf-target/deploy/percolator_prog.so /dev/shm/percolator-funding-loss-audit-20260917-host-target/deploy/percolator_prog.so
cp /dev/shm/percolator-main-merge-20260916/tests/fixtures/auth_matcher/target/deploy/auth_matcher.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
export CARGO_TARGET_DIR=/dev/shm/percolator-funding-loss-audit-20260917-host-target
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture \
  inv_038_rounding_and_ratio_conservation::v16_program_pr253_omitted_rounded_funding_rejects_and_recovers \
  inv_038_rounding_and_ratio_conservation::v16_program_pr365_fractional_cap_reaches_target_and_preserves_terminal_payouts \
  inv_039_pending_loss_obligation_durability::v16_program_pr254_shutdown_preserves_committed_funding \
  inv_039_pending_loss_obligation_durability::v16_program_shutdown_stale_rejection_has_bounded_public_catchup \
  inv_039_pending_loss_obligation_durability::v16_program_pr255_stale_resolve_requires_public_catchup \
  inv_039_pending_loss_obligation_durability::v16_program_pending_zero_move_mark_requires_terminal_funding_catchup \
  inv_039_pending_loss_obligation_durability::v16_program_pr271_cpi_close_preserves_elapsed_funding \
  inv_039_pending_loss_obligation_durability::v16_program_pr272_unilateral_reduce_preserves_elapsed_funding \
  inv_039_pending_loss_obligation_durability::v16_program_pr273_recovery_forfeit_preserves_elapsed_funding \
  inv_039_pending_loss_obligation_durability::v16_program_multi_segment_funding_requires_catchup_before_reduction \
  inv_039_pending_loss_obligation_durability::v16_program_pr380_trade_order_preserves_elapsed_funding \
  inv_039_pending_loss_obligation_durability::v16_pending_loss_discovery_metadata_preserves_distinct_resolution_evidence
cargo test --locked --offline --test v16_program_stateful_fuzz -- --exact --nocapture \
  inv_027_protected_principal_seniority::v16_program_stale_cohort_route_matrix_preserves_historical_principal \
  inv_039_pending_loss_obligation_durability::v16_program_deposit_preserves_flat_pending_obligation_and_close_partition
cargo test --locked --offline --test v16_cu -- --exact --nocapture \
  inv_039_pending_loss_obligation_durability::v16_attack_recovery_resolved_cannot_clear_unreleased_loss_weight_before_debtor \
  inv_039_pending_loss_obligation_durability::shared_holder::mixed_roles::v16_program_mixed_creditor_debtor_preserves_pending_attribution_through_resolved_close_order \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::funding_resolution::v16_program_mixed_roles_preserve_funding_attribution_through_resolution
cargo test --locked --offline --test v16_program_fuzz_regressions \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_every_public_invariant_source_and_declared_test_is_mounted -- --exact --nocapture
git diff --check a4b5fad85592d764214a696dc706944884b3d33e
git diff --exit-code a4b5fad85592d764214a696dc706944884b3d33e -- src Cargo.toml Cargo.lock
git diff --cached --check
```

Results: all commands exit 0.

| Check | Result |
| --- | --- |
| Exact fixed regressions and resolution metadata | 12 passed, 0 failed, 135 filtered; 5.59s |
| Exact stale-cohort matrix and pending-holder deposit | 2 passed, 0 failed, 326 filtered; 16.05s; default four generated stale-cohort cases |
| Exact CU resolution witnesses | 3 passed, 0 failed, 1,436 filtered; 2.21s |
| Mount census | 1 passed, 0 failed, 146 filtered; 508 source files and 1,914 available tests |
| Full staged whitespace and production/dependency diff | Clean; no `src/`, Cargo or Rust test changes |

The mixed price-debt witness reports ten exact owner payouts/deletions and peak
suffix cost 285,960 CU; the four mixed funding worlds peak at 206,022 CU. Existing
unused-code and Solana future-compatibility warnings remain. No test selector was
changed; the listed existing selectors were rerun. Other stateful variants were
source-reviewed, not rerun. No Rust/TSV formatting check applies to these two
Markdown changes, and the current coverage ledgers remain unchanged.
