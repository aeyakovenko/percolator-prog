# Rows 417/424 regression health, 2026-09-17

Base: freshly fetched `origin/main`, `9ed44f9f9afb2d1ad61058d6c7f05550fb3de0e7`.
Worktree: `/dev/shm/percolator-row417424-health-20260917`.
Branch: `audit/row417424-health-20260917`; local commit only, no push.
Scope: rows 417/424 and related INV-024/027/067/070/073/078/080/081 evidence.

Inputs: [README](README.md), [payout audit](terminal_payout_entitlement_audit_20260917.md),
[reserve audit](terminal_reserve_evidence_audit_20260917.md), and
[last-claimant regression](row417_last_claimant_regression_20260917.md).
Existing PRs/holdout rows were comparison data; no PR branch or patch was imported.
The invariant statements, mounted test bodies and current wrapper determined the
assertions. No production issue was confirmed; production and status TSVs are unchanged.

## Diagnosis and substantive coverage

**Row 417 already has the required last-claimant witness.** The unchanged stateful
selector passes against rebuilt current-main SBF: its sole haircut receipt survives
unrelated Fresh backing, missing/unrelated discovery rejects atomically, and a
permissionless crank releases the backing at expiry. Final payout rises from 1,250
to 1,750; mint supply stays 4,500,000,000 and slab retirement leaves the typed tombstone.
Main already pins engine `4db11a8c`; the older payout audit's failed S2 is historical.
Another identical public trace would add no coverage. Portfolio deletion and final
retirement still use their named signers; this is not signer-free lifecycle completion.

**Row 424 had two stale test histories.** Current
`handle_withdraw_insurance_asset` advances the asset's authority epoch even for
unsigned terminal payment. An earlier `CloseSlab` therefore returns `EngineStale`
(`Custom(19)`), before an economic lock or final-close preview can be reached.

- [Scan recredit](cu/inv_070_terminal_scan_recredit.rs) now constructs continuations
  for the initial epoch and the two successive insurance debits. Retained stale
  suffixes explicitly reject; current-epoch suffixes still reach `EngineLockActive`
  (`Custom(21)`). Both expiry/payment/close and payment/close bundles assert complete
  account rollback, including successful SPL CPI prefixes and transaction fees.
  A committed partial-payment replay and old close reject without advancing stock,
  cursor, payout ledger or control sequences. The complete asset-zero control tuple
  advances only its authority epoch, once per committed payment. This adds **64
  rollback transactions** to the original 104-check matrix and restores the previously
  unreachable rediscovery, final payment, burn and retirement assertions.
- [Native reclassification](cu/inv_070_terminal_native_reclassification.rs) adds
  **three payment/old-close rollback transactions**, one per SyncNative timing.
  It checks the exact control tuple before and after the committed payment, then
  builds the final close with the consumed epoch accounted for. The successfully
  previewed signed transaction remains byte-identical across external SyncNative
  and slot advancement. All original token/lamport, mint, beneficiary and tombstone
  checks remain, including the raw-versus-wrapped 19-atom surplus partition.

These are additions to existing public-route LiteSVM/CU selectors, not duplicate
fixtures. INV-024 receives exact input-owned payment/residue attribution; INV-070
receives executed scan/classification/retirement suffixes; INV-080 receives returned
errors with full rollback; INV-081 receives bounded stock/custody/sequence validity.
INV-067's last-claimant control is independently green. INV-027/073/078 retain only
the existing bounded senior-disposition and public-recovery evidence: these reserve
fixtures start after user liabilities are settled and do not extend those invariants
to arbitrary funded states or every recovery failure class.

## Exact results

| Execution | Result |
| --- | --- |
| Unmodified row-424 pair | FAIL: 0 passed, 2 failed, 1,437 filtered; 1.31s. Scan bundle reports instruction 4 / `Custom(19)` instead of `Custom(21)`; native preview reports instruction 2 / `Custom(19)`, 2,489 CU. |
| Repaired and extended row-424 pair | PASS: 2 passed, 0 failed, 1,437 filtered; 12.06s. |
| Scan/recredit | 16 histories, 8 order comparisons, 88 commits, 168 exact rollbacks, 8 scanner rediscoveries; peak 225,937 CU, below 400,000. Recovery is 61/61 or 100/307 atoms; residual burn is 0/207 atoms. |
| Native classification | Never / BeforeScan / AfterPreview all pass; peaks 33,758 / 32,258 / 30,758 CU, below 300,000. Each pays 37 insurance and 23 backing atoms and partitions 19 external atoms exactly. |
| Unchanged row-417 selector | PASS: 1 passed, 0 failed, 327 filtered; 0.78s. Peak 208,228 CU; receipt, 1,750 payout, unchanged supply and tombstone assertions pass. |

Only these three exact selectors executed. Both host harnesses and current SBF
built successfully; existing unused-code/Solana future-compatibility warnings remain.
No broad suite, source census, status promotion or engine/Kani proof ran.

## Reproduction and artifacts

Private targets were copied from the existing 20260916 cache; the wrapper was rebuilt
locked/offline for the current source and engine pin using platform-tools v1.52.
The old cached wrapper hash `87011b68...` was not used for behavioral results.
Fresh wrapper SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
The unchanged authenticated matcher was copied from main's fixture deployment:
SHA-256 `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
No external matcher was needed. Logs: `/dev/shm/percolator-row417424-health-20260917-logs/`
(`build-host.log`, `build-sbf.log`, `baseline-cu.log`, `green-cu.log`, `row417.log`).

Commands from the worktree; exports spell out the environment supplied via `env`:

```bash
cp -a --reflink=auto /dev/shm/percolator-public-gap-20260916-c91e-host-target /dev/shm/percolator-row417424-health-20260917-host-target
cp -a --reflink=auto /dev/shm/percolator-public-gap-20260916-c91e-sbf-target /dev/shm/percolator-row417424-health-20260917-sbf-target
mkdir -p /dev/shm/percolator-row417424-health-20260917-logs tests/fixtures/auth_matcher/target/deploy
cp /dev/shm/percolator-main-merge-20260916/tests/fixtures/auth_matcher/target/deploy/auth_matcher.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/percolator-row417424-health-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row417424-health-20260917-sbf-target/deploy/percolator_prog.so
CARGO_TARGET_DIR=/dev/shm/percolator-row417424-health-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row417424-health-20260917-sbf-target/deploy -- --locked
cargo test --locked --offline --test v16_cu --test v16_program_stateful_fuzz --no-run
# Identical command before and after the test changes:
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_scan_recredit::v16_program_terminal_scan_rediscovers_earlier_insurance_after_later_expiry \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_native_reclassification::v16_program_native_sync_after_terminal_prefix_reclassifies_only_external_surplus
cargo test --locked --offline --test v16_program_stateful_fuzz inv_067_terminal_payout_completeness_and_exact_once_settlement::v16_program_late_unrelated_backing_cannot_outlive_and_erase_resolved_receipt -- --exact --nocapture
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_070_terminal_scan_recredit.rs tests/invariants/cu/inv_070_terminal_native_reclassification.rs
git diff --check
git diff --exit-code 9ed44f9f -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/*.tsv'
git diff --exit-code 9ed44f9f -- . ':!tests/invariants/README.md' ':!tests/invariants/row417424_regression_health_20260917.md' ':!tests/invariants/cu/inv_070_terminal_scan_recredit.rs' ':!tests/invariants/cu/inv_070_terminal_native_reclassification.rs'
git diff --cached --check
git show --format= --check HEAD
```

Formatting, whitespace, committed-change checks and both scope guards pass. No
`src`, Cargo, fixture, harness-mount or TSV diff is included.

## Kept-open gaps

Rows 417/424 remain OPEN and all invariant statuses remain unchanged. Arbitrary
multi-wave backing/receipt histories, last-claimant composition with scan recredit,
new earlier obligations combined with native reclassification, unavailable-custody
recovery, and malicious authorized policy/deadline changes remain outside this run.
The generated 54-world scan control and adjacent reserve/earnings selectors were
not rerun. Funded keepers, reconstructible custody and authenticated time remain
assumptions; expiry/retirement use administrator authority. A fresh environment
must have the pinned engine commit available to Cargo, as the row-417 note records.
