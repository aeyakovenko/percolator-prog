# Row 416: Funded Oracle Return

Base: fetched `origin/main`, `6e2d043e97397026a2f76214c695192a2a5e2bb0`.
Worktree: `/dev/shm/row416-live-oracle-incarnation-20260917`.
Branch: `codex/row416-live-oracle-incarnation-20260917`; local commit only.
Owner: [cu/inv_005_retained_oracle_exposure.rs](cu/inv_005_retained_oracle_exposure.rs).

## Duplicate Analysis

Read the current row416 notes, README, funded-role source composition, Scope T,
role roster, retained exposure tests, oracle ABA, and zero-role histories before
editing. Required oracle roles cannot be disabled through the current public API;
zero-role rejection is already covered. No disabled/restored history is claimed.

`v16_program_oracle_authority_aba_is_asset_scoped_and_rolls_back_retained_prefix`
has empty assets and no owner value. The recent cold-admin return changes the
admin role with same-price exposure; last-resolved-exposure checks when cold
management becomes admissible. This single increment instead retains a
price-changing oracle instruction across the oracle holder's A -> B -> A while
both assets have independently funded positions, then checks its renewed
economic effect. Terminal closes are the payout suffix, not another management
or final-exposure experiment. Existing tests and shared helpers are unchanged.

## Public Evidence

One new selector runs four worlds: selected asset 0/1 crossed with long/short
exposure and a corresponding +10%/-10% mark. Selected owners hold ten matched
units; two other owners hold six opposite-direction units on the sibling asset.
Oracle A and B are the sibling owners, distinct from the cold admin. All actions
after ordinary V16Svm setup use public wrapper instructions; only Clock advances
use harness scaffolding. No initialized program-owned bytes are changed.

- A signed seven-atom SPL withdrawal, sibling observation and selected mark
  simulate successfully before rotation. Incumbent-signed A -> B -> A changes
  only the selected oracle role/epoch, preserving both assets' economics and
  every owner portfolio. Observation sequences and market IDs do not change.
- The retained bundle and standalone selected mark reject exactly `EngineStale`.
  Logs pin two executed wrapper prefixes and the SPL transfer in the bundle.
  Current-epoch B rejects exactly `Unauthorized`. Complete compiled Accounts and
  market/mint/vault/owner sentinels restore, including metadata, with only the
  independently calculated payer signature and priority fees deducted.
- The original standalone withdrawal and sibling observation commit. A's mark
  then commits with only its instruction authority epoch renewed; signer,
  generation, observation sequence, time hint and price remain fixed. Full
  profiles and control sequences match expected observation/checkpoint changes.
  These are separate signed envelopes, not replay of an already-processed failure.
- Public cranks realize exactly 1,000,000 profit for owner 0 and the same loss
  from owner 1's principal, with sibling asset/portfolio state unchanged. Public
  resolution and bounded closes clear both assets' positions before the winner
  exits. Each accepted close must make progress. Owner payouts are exactly
  `[deposit0 + profit, deposit1 - profit, deposit2, deposit3, deposit4]`, including
  the seven-atom prefix. Internal/physical custody and observed supply reconcile
  after every owner exit; final capital, positive PnL, OI and custody are zero.
  The captured public trace validates and successful calls obey the CU ceiling.

Primary INV-005 owns incarnation containment. Observation scope, exact value
attribution and atomic rollback provide bounded INV-020/024/027/080/081 joins.
No production issue, fix, row closure or TSV status change is claimed.

## Verification

New selector: **1 passed, 0 failed**, four worlds, eight stale rejections, four
revoked-key rejections, four SPL rollbacks, four retained sibling retries and
twenty exact owner payouts. Peak successful CU: **213,880**; successful pre-ABA
preview: **50,810**. Harness ceiling: 1,400,000 CU.

Adjacent cold-admin-return control: **1 passed, 0 failed**, peak **165,785 CU**.
Only these two exact selectors ran. Targeted rustfmt, working/staged whitespace
and protected-path checks pass; only the three named coverage/documentation
files differ from the base.

Private host/SBF caches copied from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target`; wrapper and auth
matcher rebuilt locked/offline using platform-tools v1.52 and the pinned engine
`4db11a8cb0053815e23a35d3a7d3edc265d8d866`. The ignored matcher target links to
private build output; no fixture source is edited.

- Wrapper SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
- Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
- Final new-selector log: `/dev/shm/row416-live-oracle-incarnation-20260917-new-5.log`.
- Adjacent control log: `/dev/shm/row416-live-oracle-incarnation-20260917-control.log`.

Exact commands from the worktree:

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/row416-live-oracle-incarnation-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/row416-live-oracle-incarnation-20260917-sbf-target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu inv_005_authority_incarnation_binding::retained_oracle_exposure::v16_program_funded_oracle_return_rejects_retained_mark_and_preserves_value -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_005_authority_incarnation_binding::retained_oracle_exposure::v16_program_retained_oracle_handoff_after_cold_admin_return_preserves_drain_exit -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_005_retained_oracle_exposure.rs
git diff --check
git diff --cached --check
git diff --exit-code 6e2d043e -- src Cargo.toml Cargo.lock tests/fixtures tests/support tests/v16_cu.rs scripts 'tests/invariants/*.tsv'
git show --format= --check HEAD
git status --porcelain
```

Development corrections were confined to this test: LiteSVM preview metadata,
the changed mark's pending funding checkpoint, and terminal payout scheduling
behind the sibling's stored positions. No broader selector family was started.

## Remaining Gaps

Row 416 remains OPEN. This is one fixed AuthMark/TradeNoCpi, zero-fee/funding
history with a solvent profit/loss pair and winner-last settlement. Recovery,
ResetPending/Retired, arbitrary policy/mode/oracle schedules, lossy receipts,
impaired/earned stock, other transports and general role-coalescence histories
remain outside it. No broad suite, Kani, mutation campaign, failed-call CU bound
or generic authority-containment proof ran.
