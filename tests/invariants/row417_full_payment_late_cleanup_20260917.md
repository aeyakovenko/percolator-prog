# Row 417: full payment before late backing cleanup

Base: `c2f4d1067e8047880ce0ffa502606a600b524666`, freshly fetched `origin/main`.
Branch: `codex/row417-continuity-20260917T201950-QNPQSUys`.
Worktree: `/dev/shm/percolator-row417-QNPQSUys/worktree`.
Primary INV-067; related INV-010/024/029/063/066/068/070.

## Coverage increment

Read the existing row417 health, last-claimant and row417424 notes, Scope U,
receipt destination/terminal/native histories, and source-residual coverage.
The existing fully-receipted two-wave test retains haircut receipts until both
buckets expire. Source-residual coverage reaches full rate at its final expiry;
terminal-disposition coverage completes receipts after expiry. Neither establishes
that full-face receipts allow portfolio deletion while another bucket stays Fresh,
then preserve the completed claim denominator through that bucket's release.

One new selector shares the existing stateful two-wave fixture. Eight worlds vary
which source side expires first, exact slots 40/60 versus both overdue at 61, and
unequal claimant order. Public trades at 100 then 150 produce gross faces 700/1300;
the debtor's 500 atoms realize 175/325 before the slot-14 snapshot. The remaining
receipt faces are exactly 525/975, with zero unreceipted bound and vacant sources.

The first discovered bucket releases 1,500 atoms, reaching full rate. Retained
permissionless top-ups pay each face exactly and finalize both receipts. Full
receipt fields, portfolio incarnation/position epoch/owner, common denominator,
stock, provider debit, vault, mint supply and claimant SPL balances are checked.
Zero-due retries preserve the complete economic frame. Both portfolios then close
with exact rent transfer while the other 189-atom bucket remains Fresh. In the
four exact-slot worlds, bounded slab progress reaches an atomic pre-expiry lock;
the mint and vault retain all 189 atoms.

At maturity, CloseSlab releases that bucket. Snapshot residual rises from 1,500
to 1,689 while the entire remaining ledger equals its prior value, including
1,500 exact face mass, zero unreceipted bound, slot 14 and rate 1. Each successful
expiry or final burn/close prefix is aborted by an invalid System suffix, then
the retained CloseSlab transaction succeeds. All compiled Accounts and the wider
economic frame roll back, including absent portfolios, custody and mint. Only
the payer loses the decoded signature and priority fees. Logs prove the aborted
final prefix executed both SPL Burn and CloseAccount. All eight worlds reach
the tombstone, with owner payouts 1,700/2,300 and exactly 189 tokens burned.

This exercises full-payment liveness alongside the existing haircut-retention
guard. The usual V16Svm empty-account allocation and SPL endowments are reused;
all economic transitions use public instructions. No program-owned economic bytes
are edited, and no production correction was needed.

## Verification

Private host/SBF caches were copied from the c91e caches; the program and pinned
engine were rebuilt from this worktree, locked/offline, with default features and
platform-tools v1.52. Unchanged authenticated-matcher bytes were copied into the
ignored fixture target and hash-verified; no matcher trade is used by these tests.

Program SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

All selectors below belong to
`inv_067_terminal_payout_completeness_and_exact_once_settlement` in
`--test v16_program_stateful_fuzz`:

| Exact selector suffix | Result | Peak CU |
| --- | --- | ---: |
| `v16_program_fully_paid_receipts_exit_before_late_excess_backing_cleanup` | PASS: 8 worlds, 16 successful-prefix rollbacks, 16 positive top-ups, 8 slab closures | 245,968 |
| `v16_program_fully_receipted_claimants_survive_two_unrelated_expiry_waves` | PASS: existing 8 worlds, 8 paid-prefix rollbacks, 32 positive top-ups, 8 slab closures | 408,787 |
| `v16_program_late_unrelated_backing_cannot_outlive_and_erase_resolved_receipt` | PASS: receipt survives, payout 1,750, unchanged supply, tombstone | 208,228 |

Final run: **3 passed, 0 failed, 327 filtered**, 15.75 seconds. Scoped rustfmt,
diff/show whitespace checks and protected-path/scope guards pass. No broad suite
or Kani run. Logs and build artifacts are outside the worktree under
`/dev/shm/percolator-row417-QNPQSUys/`; final log: `logs/final-exact.log`.

Development probes first attempted a waiting sibling across full-rate expiry:
auto-crank instead pays already-due full-rate claims and rejects finalized ones;
direct asset retirement rejects in Resolved mode. Those assumptions were removed.
The kept route uses the public slab continuation after full payment and deletion.
Other probe failures corrected priority-fee accounting and a decoded stock field;
they were test assumptions/compilation errors, not production findings.

```bash
cd /dev/shm/percolator-row417-QNPQSUys/worktree
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 TMPDIR=/dev/shm
export CARGO_TARGET_DIR="$PWD/../sbf-target"
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
export PERCOLATOR_FUZZ_SBF="$PWD/../sbf-target/deploy/percolator_prog.so"
export CARGO_TARGET_DIR="$PWD/../host-target" CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
family=inv_067_terminal_payout_completeness_and_exact_once_settlement
cargo test --locked --offline --test v16_program_stateful_fuzz -- --exact --nocapture --test-threads=1 \
  "$family::v16_program_fully_paid_receipts_exit_before_late_excess_backing_cleanup" \
  "$family::v16_program_fully_receipted_claimants_survive_two_unrelated_expiry_waves" \
  "$family::v16_program_late_unrelated_backing_cannot_outlive_and_erase_resolved_receipt"
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/stateful/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs
git diff --check
git show --format= --check HEAD
git diff --exit-code c2f4d106 -- src Cargo.toml Cargo.lock tests/fixtures tests/support tests/v16_cu.rs tests/v16_program_stateful_fuzz.rs ':(glob)tests/invariants/*.tsv'
git diff --exit-code c2f4d106 -- . ':!tests/invariants/stateful/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs' ':!tests/invariants/README.md' ':!tests/invariants/row417_full_payment_late_cleanup_20260917.md'
```

## Remaining gaps

Row 417 and all invariant statuses remain unchanged. This is finite two-claimant,
two-bucket, primary-SPL, zero-fee/funding evidence. Arbitrary stock histories,
insurance recredit, native or recreated custody, Recovery, CPI trade routes and
maximum shapes remain open. Slab cleanup assumes an available market authority.
