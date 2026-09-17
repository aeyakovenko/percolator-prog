# Row 416: Cold-Admin Return and Live Drain Exit

Base: latest fetched `origin/main`, `d70373311b32040a4a39f5c82b6e52bf99dd04e1`.
Worktree: `/dev/shm/percolator-row416-health-20260917`.
Branch: `codex/row416-health-20260917`; local commit only.
Owner: [cu/inv_005_retained_oracle_exposure.rs](cu/inv_005_retained_oracle_exposure.rs).

## Distinct Coverage

Finding-blind inputs: `scripts/loop.md`, invariant charter/README, row 416's
coverage/status TSV entries, authority-containment roster, Scope T, funded-role
source composition and adjacent public tests. No external finding or patch used.
Existing cold-admin ABA/generated role histories have flat economic portfolios;
the September 17 retained-exposure test ends in resolution. This increment joins
retained incumbent oracle consent, admin return, live exposure, lifecycle admission
and complete live principal exit. No shared helper or production change.

Four worlds cross asset 0/1 with both signs of ten-unit matched exposure:

- Prevalidate a signed seven-atom SPL withdrawal plus incumbent oracle handoff
  while exposure is live. Cold-admin A -> B -> A preserves decoded economics and
  returns the same profile, advancing the authority epoch twice. The original
  bundle rejects `EngineStale` after the SPL prefix executes.
- Renew incumbent consent. Its handoff plus additional trade simulates successfully
  in Active. Public DrainOnly changes lifecycle without changing the authority
  epoch. Current cold-admin oracle seizure still rejects `EngineLockActive` after
  the same SPL prefix. The retained handoff/trade bundle executes the handoff,
  then rejects the risk-increasing suffix with `EngineLockActive`.
- All rejections restore complete compiled Accounts plus market, mint, custody
  and all five owners' portfolio/source/destination sentinels, excluding only
  independently calculated payer signature and priority fees. Logs pin completed
  wrapper/SPL prefixes; signatures and packet bounds are checked.
- Retry the identical signed incumbent handoff and withdrawal successfully.
  Only the oracle field and one authority epoch change. The former oracle cannot
  publish at the current epoch. The successor publishes the unchanged honest price
  at authenticated slot 2 despite a `u64::MAX` time hint.
- Public matched reduction closes both positions in DrainOnly; all five owners
  withdraw their exact input principal, including the earlier seven atoms, with
  both exposed-owner payout orders. Input-derived remaining capital/custody reaches
  zero, positive PnL stays zero, SPL supply reconciles, sibling profiles/sequences
  stay exact, and the public trace validates without economic state injection.

INV-005 owns consent/scope; INV-020 authenticated observations; INV-024/027 exact
principal attribution/exit; INV-055 Active/DrainOnly admission; rollback joins
INV-080/081. Initial V16Svm accounts and Clock are harness scaffolding; economic
history after setup uses public instructions. No LoF/DoS bug or fix is claimed.

## Validation

Final exact run: **2 passed, 0 failed, 1,448 filtered**, 4.06s. New selector:
four worlds, 16 exact rejections, four rolled-back handoffs, eight rolled-back
SPL payouts, four live reductions and 20 owner exits. Peak successful CU:
**165,785** (existing selector: 142,495); harness limit 1,400,000.
Targeted rustfmt, working/staged diff checks and post-commit whitespace pass.
Production, Cargo, fixtures and TSVs remain unchanged; no row411 file is edited.

Private host/SBF caches copied from `/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target`.
Wrapper and matcher rebuilt locked/offline with platform-tools v1.52 in this
worktree's private SBF target; engine `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
Wrapper build 7.88s; matcher 13.93s; initial host compile 34.71s. No test failed.
The initial green used a copied matcher; final green uses the rebuilt matcher.
Existing Solana future-compatibility warning remains.

- Wrapper SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
- Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
- Logs: `/dev/shm/percolator-row416-health-20260917-logs/{build-sbf,build-matcher,drain-exit,green}.log`.

Exact commands, from the worktree:

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/percolator-row416-health-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row416-health-20260917-sbf-target/deploy/percolator_prog.so
CARGO_TARGET_DIR=/dev/shm/percolator-row416-health-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row416-health-20260917-sbf-target/deploy -- --locked
(cd tests/fixtures/auth_matcher && CARGO_TARGET_DIR=/dev/shm/percolator-row416-health-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row416-health-20260917-sbf-target/matcher-deploy -- --locked)
cp /dev/shm/percolator-row416-health-20260917-sbf-target/matcher-deploy/auth_matcher.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
cargo test --locked --offline --test v16_cu inv_005_authority_incarnation_binding::retained_oracle_exposure::v16_program_retained_oracle_handoff_after_cold_admin_return_preserves_drain_exit -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_005_authority_incarnation_binding::retained_oracle_exposure::v16_program_retained_empty_oracle_handoff_rechecks_exposure_before_payout -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_005_retained_oracle_exposure.rs
git diff --check
git diff --cached --check
git diff --exit-code d7037331 -- src Cargo.toml Cargo.lock tests/fixtures 'tests/invariants/*.tsv' '*row411*'
git show --check --oneline HEAD
```

## Remaining Gaps

Row 416 remains OPEN. Same-price, zero-fee, matched TradeNoCpi histories do not
cover loss-only/stored-position/claim classifier terms, earned reserves, partial
reductions, other trade transports, Recovery/ResetPending/Retired, quote rails,
arbitrary oracle schedules, role coalescence or maximum shapes. INV-027 here is
solvent principal preservation, not an underbacked seniority proof. No broad
suite, Kani, mutation campaign or global success-state certification ran.
