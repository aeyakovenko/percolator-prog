# Row 433: redeemed native earnings with an unavailable provider

Base: freshly fetched `origin/main`, `6763f553a84cfc0b8408f15802f671ec44552598`.
Branch: `codex/row433-progress-20260917-QL9FhS`.
Worktree: `/dev/shm/percolator-row433-20260917-QL9FhS/worktree`.
Primary INV-073; related INV-018/021/024/027/067/069/070/071/078/081/082.

## Coverage increment

Reviewed the row433 README/ledger entries, terminal-reserve evidence audit,
row420433 health note and existing reserve histories before editing. Absent
recipients, frozen/recreated SPL custody, paid-prefix/full-close rollback,
Recovery across last-portfolio deletion and ordinary dual-rail payout orders
already have witnesses. Those routes were not added again.

The existing dual-quote earnings history closes an empty native ATA before
payment; the native provider redemption history has no earnings or ledger.
The row421 closed native insurance identity history has an insurance ledger,
not provider earnings shared across quote rails. This increment crosses the
documented earned-fee/native-redemption gap with exactly one new history and
one new selector in the already mounted earnings file. The existing selector
retains all four original worlds through a shared verifier.

The public earnings fixture trades, resolves, pays users 56,627/1,995,000 atoms
and deletes their empty portfolios. An unsigned 17-atom native earnings payment
initializes the provider ledger. The provider publicly closes that funded ATA,
redeeming exactly rent plus 17 lamports, then transfers its whole System
wallet to the existing successor wallet and drops its key. The provider wallet
is absent or an empty, zero-lamport System account thereafter. No program-owned
bytes are mutated; the reused native-mint genesis fixture emulates SPL state.

With only the keeper signing successful economic transactions, the continuation:

1. Pays the remaining 858 earnings atoms on the configured SPL rail. A premature
   close suffix first aborts the completed payout, preserving the already-paid
   17-atom ledger, redeemed SOL and missing native custody exactly.
2. Recreates the provider's native ATA and pays 100,000 principal atoms. Another
   premature close first rolls back both ATA creation/rent and the payment,
   while preserving the now fully paid 875-atom earnings ledger. The identical
   repair/payment prefix then succeeds without any provider signature.
3. Pays 31 insurance atoms unsigned. Administrative CloseSlab sweeps cross-rail
   surplus, closes both vaults and produces the exact rent-funded tombstone.
   A repeated CloseSlab first aborts that completed closure; the same close
   instruction then commits separately with the market authority.

Full Account rollback includes all compiled and tracked accounts, allowing only
the exact signature fee. Expected failure indices and completed top-level
wrapper/ATA log counts establish the successful prefixes. Stock/encumbrance
censuses, fixed mint frames, prior user payments, beneficiary identities,
native token amounts/lamports and cumulative ledger withdrawals are checked.
Final native provider custody contains principal only: the redeemed 17 is not
paid again. The SPL provider receives 858; primary/secondary surplus 858/17 goes
to insurance custody separately from its 31-atom claim. Ledger bytes and rent
survive principal payment and final closure; the drained provider stays drained.

No production correction was required. Row 433 remains OPEN. Resolution, prior
portfolio deletion and final slab closure retain their authority assumptions;
the insurance beneficiary is the available administrator. This finite history
does not cover absent-administrator retirement, arbitrary custody/role/asset
histories, native-secondary earnings, Recovery/recredit, pending-loss/receipt
composition, expiry races, unavailable insurance beneficiaries or maximum shapes.

## Verification

Private host/SBF caches were copied with `cp -a --reflink=auto` from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target` into the task root.
The wrapper and pinned engine were rebuilt from this worktree, locked/offline,
with default features and platform-tools v1.52; build completed in 7.87 seconds.
Engine: `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
SBF SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
Logs and targets remain outside tracked files under the worktree's parent.

All three behavioral selectors below use `--test v16_cu` with `--exact`:

| ID | Result | Peak CU |
| --- | --- | ---: |
| N (new) | PASS: 1 world, 4 unsigned payments, 3 rollbacks, 1 closure | 258,278 |
| E (existing earnings) | PASS: 4 worlds, 16 payments, 5 rollbacks, 4 closures | 264,323 |
| P (existing redemption) | PASS: 4 worlds, 101 redeemed / 300 remaining each | 48,868 |

N CU [payment/setup, rejection, closure]: `[242971, 258278, 43217]`.
E CU [payment/setup, rejection, closure]: `[242977, 264323, 49217]`.
The reused transaction ceiling is 1,200,000 CU. The worker's combined behavioral
run passed 3/3. Merged-main revalidation reran all three selectors individually,
with 1 passed, 0 failed and 1,456 filtered per selector. The first probe failed
only because it expected `None` after wallet draining; LiteSVM retained a
zero-lamport System account. The corrected assertion accepts only those two
drained representations. That failed probe is retained as
`logs/probe-wallet-frame.log`; the worker passing run is `logs/final-exact.log`.
Existing Solana client compatibility warnings remain.

The exact mount census passes: 517 source files, 1,933 available tests; 1 passed,
0 failed, 146 filtered. Touched-Rust rustfmt, diff/staged/show whitespace checks,
protected paths and the exact three-file scope guard pass. No broad suite or
Kani run was performed. The local commit leaves the worktree clean.

```bash
cd /dev/shm/percolator-row433-20260917-QL9FhS/worktree
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_TARGET_DIR="$PWD/../sbf-target"
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked > ../logs/build-sbf.log 2>&1
export PERCOLATOR_FUZZ_SBF="$PWD/../sbf-target/deploy/percolator_prog.so"
export CARGO_TARGET_DIR="$PWD/../host-target" CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
N=inv_024_attributed_quote_value_conservation::terminal_earnings_succession::dual_quote_earnings_progress::v16_program_redeemed_native_fee_prefix_survives_absent_provider_and_dual_quote_close
E=inv_073_no_permanent_user_lock::v16_program_absent_provider_dual_quote_earnings_share_one_ledger_and_close
P=inv_073_no_permanent_user_lock::v16_program_absent_native_provider_redeemed_prefix_preserves_public_remainder_and_close
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 "$N" "$E" "$P" > ../logs/final-exact.log 2>&1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_every_public_invariant_source_and_declared_test_is_mounted -- --exact --nocapture --test-threads=1 > ../logs/mount-exact.log 2>&1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_073_dual_quote_earnings_progress.rs
git diff --check
git diff --exit-code 6763f553 -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/v16*' ':(glob)**/*.tsv'
git diff --exit-code 6763f553 -- . ':!tests/invariants/cu/inv_073_dual_quote_earnings_progress.rs' ':!tests/invariants/README.md' ':!tests/invariants/row433_redeemed_native_earnings_20260917.md'
git diff --cached --check
git show --format= --check HEAD
```
