# Row 416: Last Resolved Exposure

Base: freshly fetched `origin/main`, `d011ff0318ea763375c0d06fd126d4488056e746`.
Branch: `codex/row416-authority-return-20260917-r7k3`.
Worktree: `/dev/shm/percolator-row416-20260917-r7k3`; local commit only.
Owner: [cu/inv_005_retained_oracle_exposure.rs](cu/inv_005_retained_oracle_exposure.rs).

## Distinct Coverage

Inspected row 416's ledger and README entries, both September 17 row416 notes,
the funded-role source composition and Scope T notes, and the existing oracle,
insurance, backing, zero-role, refunding and shutdown/return histories before
adding this test. No external finding or patch supplied the history.

Existing exposed-oracle histories reject while both sides have OI, then either
resolve and pay owners or reduce both sides together in DrainOnly. Existing
shutdown/role-return histories use flat portfolios and funded reserves. This
increment checks the asymmetric terminal state after only one exposed owner has
settled, then the transition to an empty oracle role. It does not duplicate
cold-admin ABA, funded-holder ABA or empty-to-funded retained consent.

One new exact selector runs four worlds: assets 0/1 crossed with each exposed
owner settling first. All economic transitions use public wrapper instructions
and normal accounts; V16Svm's initial zero-account/SPL and Clock scaffolding is
unchanged. No initialized program-owned bytes are edited.

- Two independent owners open ten units of same-price matched exposure. A
  separate cold admin and incoming oracle sign management without the incumbent.
- Public ResolveMarket preserves the oracle profile and authority epoch. A
  signed CloseResolved prefix pays the first owner before cold management rejects
  EngineLockActive. Complete compiled Accounts and market, mint, vault and all
  five owners' portfolio/source/destination sentinels restore exactly, excluding
  only independently calculated payer signature and priority fees. Logs require
  both the wrapper and SPL prefix to have succeeded.
- The original unprocessed close transaction commits. The selected asset now has
  exactly `(0, 10 * POS_SCALE)` or `(10 * POS_SCALE, 0)` OI and `(0, 1)` or `(1, 0)`
  stored-position counts. A flat bystander's actual terminal payout followed by
  the same cold management instruction also rejects and rolls back. Its original
  unprocessed close transaction remains usable.
- The final exposed owner's close plus retained management simulates successfully
  in one packet. They then commit separately: both exposure sides reach zero,
  the retained signed management transaction lands for the first time, and only
  the selected oracle field and authority epoch change. Two flat owners still
  have capital in custody, demonstrating that unrelated capital does not fund the
  oracle role. Transaction envelopes differ between bundles; this makes no claim
  that an already-processed failed Solana transaction can be replayed unchanged.
- All five owners receive exactly their input principal. Remaining capital,
  positive PnL and SPL custody reach zero; mint supply and observed token supply
  reconcile; sibling profile/control sequences stay exact. Public trace validation
  passes and successful transaction CU stays within the harness limit.

Primary INV-005 owns the cold-admin/incumbent boundary. Related INV-020 is bounded
to preserving the existing same-price observation profile; INV-024/027 cover
principal attribution and solvent exit; INV-055 covers Resolved admission and
cleanup. Exact rollback also provides bounded INV-080/081 evidence. The existing
rollback helper moved to module scope for reuse; existing test behavior is unchanged.
No production correction was required, and no invariant or TSV status is promoted.

## Verification

Final exact runs: **3 passed, 0 failed** across two invocations. The new selector
passes 1/1 in 1.99s (1,455 filtered), covering four worlds, eight exact rejections,
eight rolled-back SPL payouts, four committed empty-role controls and twenty
exact owner payouts. Peak successful CU: **126,361**; successful paired-release
simulation peak: **126,572**. The two existing selectors pass 2/2 in 4.05s
(1,454 filtered), with respective successful peaks **142,495** and **165,785**.
The harness ceiling is 1,400,000 CU. Targeted rustfmt, working/staged whitespace,
protected paths and post-commit whitespace checks pass; the worktree is clean.

Development-only failures were a private fixture payer access at compilation and
AlreadyProcessed when attempting to resubmit a failed transaction's signature.
The final test uses the real owner's signer for standalone close transactions
and distinct bundle envelopes for retained instruction consent. Both failures
were test construction issues; no program failure or production fix is claimed.
The existing Solana future-compatibility warning remains.

Private host/SBF caches copied from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target`.
Wrapper and auth matcher rebuilt from this worktree, locked/offline with
platform-tools v1.52 and engine `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
Wrapper build: 7.90s; matcher build: 13.97s.

- Wrapper SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
- Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
- Logs: `/dev/shm/percolator-row416-20260917-r7k3-logs/`.
  Builds: `build-sbf.log`, `build-matcher.log`; final tests:
  `last-exposure-3.log`, `adjacent.log`; earlier attempts:
  `last-exposure.log`, `last-exposure-2.log`.

Exact commands from the worktree:

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/percolator-row416-20260917-r7k3-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row416-20260917-r7k3-sbf-target/deploy/percolator_prog.so
CARGO_TARGET_DIR=/dev/shm/percolator-row416-20260917-r7k3-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row416-20260917-r7k3-sbf-target/deploy -- --locked
(cd tests/fixtures/auth_matcher && CARGO_TARGET_DIR=/dev/shm/percolator-row416-20260917-r7k3-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row416-20260917-r7k3-sbf-target/matcher-deploy -- --locked)
cp /dev/shm/percolator-row416-20260917-r7k3-sbf-target/matcher-deploy/auth_matcher.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
cargo test --locked --offline --test v16_cu inv_005_authority_incarnation_binding::retained_oracle_exposure::v16_program_cold_oracle_handoff_waits_for_last_resolved_exposure -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_005_authority_incarnation_binding::retained_oracle_exposure::v16_program_retained_empty_oracle_handoff_rechecks_exposure_before_payout \
  inv_005_authority_incarnation_binding::retained_oracle_exposure::v16_program_retained_oracle_handoff_after_cold_admin_return_preserves_drain_exit
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_005_retained_oracle_exposure.rs
git diff --check
git diff --cached --check
git diff --exit-code d011ff0318ea763375c0d06fd126d4488056e746 -- src Cargo.toml Cargo.lock scripts tests/fixtures tests/support tests/v16_cu.rs 'tests/invariants/*.tsv' '*row411*' '*row427*'
git show --format= --check HEAD
git status --porcelain
```

## Remaining Gaps

Row 416 remains OPEN. OI and stored positions coexist in the asymmetric state;
this does not isolate each individual funded predicate term. Loss-only states,
claim receipts, impaired/earned stock, nonzero PnL, Recovery/ResetPending/Retired,
arbitrary oracle schedules, role coalescence and other trade/quote transports are
outside this history. The paired last-close/management control is a simulation;
its two constituent operations commit separately. No broad suite, Kani, mutation
campaign, failed-transaction CU bound or generic authority-containment proof ran.
