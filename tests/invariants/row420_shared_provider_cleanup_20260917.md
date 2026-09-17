# Row 420: absent shared provider through terminal cleanup

Base: freshly fetched `origin/main`, `c2f4d1067e8047880ce0ffa502606a600b524666`.
Branch: `codex/row420-terminal-jIpTY8`.
Worktree: `/dev/shm/percolator-row420-jIpTY8/worktree`.
Local commit only; no push. Row 420 stays OPEN and no TSV is changed.

## Bounded Continuation

The existing selector in [the shared-holder test](cu/inv_073_shared_holder_paid_reserves.rs)
already creates exposure, pays live reserve prefixes, spends the provider's
36 atoms into a separate operator's custody, drops both holder keys and the
operator key, and completes permissionless user payouts. Its four worlds retain
both claimant orders and both `CloseResolved` / `PermissionlessCrank` aliases.
The unchanged senior entitlements are 56,627 and 1,995,000 atoms. The prior
endpoint leaves two empty portfolios and 100,863 reserve atoms behind a public
withdrawal gate. The baseline passes on the private current-main SBF.

This change adds one continuation to that existing selector, without adding
another test or setup matrix:

1. The market administrator deletes the first empty portfolio. Its exact rent
   moves into the slab; the holder keys remain unavailable.
2. A transaction deletes the last portfolio, pays all 856 remaining provider
   earnings, then attempts one more earnings atom. Exactly two wrapper successes
   and one SPL transfer precede `EngineLockActive` at instruction index 4.
   Complete compiled/tracked Accounts roll back, including the deleted portfolio,
   rent, shared user/provider custody and the already-populated earnings ledger.
   Only the exact two-signature transaction fee remains charged.
3. The same deletion succeeds separately. Three keeper-only payments return
   99,983 provider principal, 856 provider earnings and 24 beneficiary insurance
   atoms. The rejected transaction's fee instruction is reused unchanged. Both
   provider payment orders run within the existing claimant-order worlds.
4. The administrator closes the empty slab at slot 11, before backing expiry 100.
   The vault disappears, the typed tombstone retains exact rent, and the exact
   3,001,037,040-lamport refund reaches the administrator in every world.

The provider destination ends at 157,466 atoms: its senior user payout plus only
its unpaid reserve tail. The insurer/user destination ends at 1,995,024; the
operator's previously paid-and-spent 43 atoms remain exact. Fixed mint supply is
2,152,533 with no burn. The provider ledger records exactly 875 lifetime earnings
withdrawn (19 live plus 856 terminal), never a second claim on the spent prefix.
Each reserve step checks input-derived stock, complete token images, wallet frames,
market stock census and reservation/encumbrance census. The final close frames
paid custody, ledger and mint. All economic state comes from public System/SPL/
wrapper instructions; account-image edits only construct assertion expectations.

Primary coverage is INV-073, with bounded custody, encumbrance, seniority,
exact-once, normalization/close and public-progress evidence related to
INV-018/021/026/027/032/063/067/069/070/071/078/082. This is not a new theorem for
those invariants or a broad audit. Insurance payout is cleanup, not a row 421
promotion. Administrative deletion and slab close still require the market
authority; economic reserve payments require only the keeper. Missing custody,
expiry/recredit, Recovery, receipts, other quote rails, multiple assets/providers,
maximum shapes and arbitrary histories are outside this continuation.

## Duplicate Analysis

Reviewed row 420's ledger notes, README and these existing owners before editing:

- [Shared-holder note](shared_holder_paid_reserves_audit_20260912.md): the exact
  paid/spent/shared-user prefix exists but deliberately stops before deletion and
  reserve payout. It is extended in place, preserving all 66 original rollbacks.
- [Public reserves](cu/inv_073_terminal_public_reserves.rs) and
  [Recovery cleanup](cu/inv_073_recovery_reserve_cleanup.rs): already own payout
  order and the last-portfolio gate with distinct users and reserve recipients.
  They do not carry this provider's spent live payments, populated ledger and
  independently paid senior user claim in the same destination through closure.
- [Keeper handoff](cu/inv_073_provider_keeper_ledger_handoff.rs),
  [generated missing wallets](cu/inv_073_generated_reserve_wallets.rs), and
  [native dual-quote earnings](row420433_health_20260917.md): custody recreation,
  missing identity, ledger replacement and rail variants already have owners.
  Those candidate routes were discarded as duplicates; no repair matrix is added.
- [Replenished provider](cu/inv_073_replenished_provider_progress.rs) and
  [terminal product health](row420421423433_regression_health_20260917.md): loss,
  recredit and expiry composition are separate histories, not needed for this
  bounded final-cleanup continuation.

No production correction was required. Only one existing invariant test file,
the README and this note change.

## Validation

All logs and private build targets are outside the worktree at
`/dev/shm/percolator-row420-jIpTY8/`. Private host and SBF caches were copied with
`cp -a --reflink=auto` from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target`.
The default-feature SBF was rebuilt from this worktree using locked/offline
platform-tools v1.52, engine `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
Build: PASS, 7.88s. No matcher fixture is needed. SBF SHA-256:
`a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.

Exact commands, run from the worktree:

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/percolator-row420-jIpTY8/host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row420-jIpTY8/sbf-target/deploy/percolator_prog.so
export ROW420_LOG=/dev/shm/percolator-row420-jIpTY8/logs
CARGO_TARGET_DIR=/dev/shm/percolator-row420-jIpTY8/sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row420-jIpTY8/sbf-target/deploy -- --locked > "$ROW420_LOG/build-sbf.log" 2>&1
S=inv_073_no_permanent_user_lock::shared_holder_paid_reserves::v16_program_absent_shared_holders_keep_paid_reserves_separate_from_public_user_exit
# Before editing: same command, redirected to baseline.log.
cargo test --locked --offline --test v16_cu "$S" -- --exact --nocapture --test-threads=1 > "$ROW420_LOG/continuation.log" 2>&1
rustfmt --edition 2021 --config skip_children=true tests/invariants/cu/inv_073_shared_holder_paid_reserves.rs
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_073_shared_holder_paid_reserves.rs
git diff --check
git diff --exit-code c2f4d106 -- src Cargo.toml Cargo.lock tests/fixtures tests/support tests/v16_cu.rs kani tests/invariants/kani ':(glob)**/*.tsv'
git diff --exit-code c2f4d106 -- . ':!tests/invariants/cu/inv_073_shared_holder_paid_reserves.rs' ':!tests/invariants/README.md' ':!tests/invariants/row420_shared_provider_cleanup_20260917.md'
git diff --cached --check
git show --format= --check HEAD
git status --porcelain
```

| Run | Result | Measured Peak CU |
| --- | --- | --- |
| `baseline.log`, unchanged selector | PASS, 1/1, 4 worlds, 2.61s | 549,857 |
| `continuation.log`, extended selector | PASS, 1/1, 4 worlds, 2.84s | 549,045 |
| New administrative deletion | 8 successes | 121,853 |
| New deletion/payment/overclaim rollback | 4 exact rollbacks | 549,045 |
| New keeper-only reserve payout | 12 successes | 226,375 |
| New slab close | 4 successes | 23,774 |

The existing prefix peaks at 546,857 CU in the extended run. All measured
transactions retain the existing 600,000-CU and 1,232-byte packet limits; initial
funding/trades are outside these peaks. Total exact rollbacks are now 70, with
24 original user-transfer rollback prefixes plus four new fee-transfer prefixes.
No test failures, broad suite, metadata promotion or unrelated health repair.
Touched-Rust formatting, whitespace, protected paths, three-file allowlist and
committed whitespace checks pass; the committed worktree is clean.
