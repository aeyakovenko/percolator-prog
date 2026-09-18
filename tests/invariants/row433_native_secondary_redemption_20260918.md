# Row 433: paid native-secondary earnings redemption

Base: `origin/main` at worktree creation,
`6c267d28d7c46262829c3fffaf535c8d8a8c689c`.
Worktree: `/dev/shm/astra-ultra-row433-coverage-20260918`.
Branch: `qa/row433-coverage-20260918`. Local commit only; no push.
Owners: INV-024 / INV-073. Row 433 remains OPEN.

## Non-duplicate increment

The [native-secondary note](row433_native_secondary_earnings_20260917.md)
explicitly leaves paid native-secondary redemption open: its provider closes an
empty ATA before receiving fees. The [redeemed-earnings note](row433_redeemed_native_earnings_20260917.md)
and `cu/inv_073_dual_quote_earnings_progress.rs` cover a paid native-primary
prefix. `cu/inv_077_terminal_quote_variants.rs` redeems native-secondary custody
after closure without a provider earnings ledger. The Row 433 README entries,
nearby [health note](row420433_health_20260917.md) and
[reserve audit](terminal_reserve_evidence_audit_20260917.md) supply no witness
for paid secondary redemption followed by ledger-preserving custody repair.

One new exact selector reuses the existing native-secondary test body through
a private verifier in that same file. The original selector retains its history.
Fixtures, support helpers and harness mounts are unchanged.

After the existing public trade/resolve/user-payout prefix earns 875 atoms, a
keeper pays 17 earnings atoms from the native-secondary vault. The provider
closes that funded ATA, receives exactly rent plus 17 lamports, drains its System
wallet to the existing operator and drops its key. The continuation checks the
provider is absent or an empty zero-lamport System account. It pays another 17
fee atoms on SPL, leaving 841 owed and 34 recorded as withdrawn in the same ledger.

A keeper-only bundle recreates the ATA and pays those 841 native atoms, then
overclaims one SPL atom. `EngineLockActive` at instruction 4 rolls back every
compiled/tracked Account except the exact signature fee; completed ATA/wrapper
logs prove creation and payment ran. The 34-atom ledger, redeemed value, absent
custody and keeper rent survive. The identical repair/payment prefix then succeeds.

Unsigned principal/insurance payments remain 100,000/31 atoms. Administrative
two-vault closure preserves the 875-atom paid ledger and prior user payouts.
Final provider custody is 100,017 SPL plus 841 native atoms; another 17 native
atoms were already redeemed. Admin custody receives 31 insurance plus 858 SPL
surplus, and 17 native surplus. The separate unsynced 19-lamport donation and
exact vault/slab rent refund reach the admin wallet. Token/lamport images, fixed
mint frames, ledger identity, stock and encumbrance censuses bind these amounts.

This is one solvent-domain history. Prior portfolio deletion and native redemption
retain owner signatures; resolution and slab closure retain the administrator.
Unavailable-administrator cleanup, Recovery/recredit, receipts, expiry races,
multiple providers/assets, maximum shapes and arbitrary histories remain open.

## Verification

Private host/SBF caches were copied with `cp -a --reflink=auto` from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target` to this worktree's
`-host-target` / `-sbf-target` sibling paths. Default-feature SBF rebuilt
locked/offline with platform-tools v1.52 in 7.95 seconds.
Engine: `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
SBF SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.

Only N (new) and C (nearest existing control) ran, once each with `--exact`.
Each passed: 1 passed, 0 failed, 1,481 filtered. Each executes one world, one
complete rollback and one closure; N has five unsigned reserve payments, C four.

| Selector | Peak CU | Setup / payment / rejection / close CU |
| --- | ---: | --- |
| N | 450,839 | 450,839 / 241,485 / 444,091 / 46,217 |
| C | 450,839 | 450,839 / 238,485 / 444,091 / 43,217 |

Both satisfy the existing 1,200,000-CU ceiling. Build/test logs are the worktree's
sibling `-build.log`, `-new.log` and `-control.log` files. No broad suite, mount
census or additional selector ran. The existing Solana client future-compatibility
warning remains. Reproduction from the worktree:

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row433-coverage-20260918-sbf-target
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row433-coverage-20260918-host-target
N=inv_024_attributed_quote_value_conservation::terminal_earnings_succession::dual_quote_earnings_progress::native_secondary_earnings::v16_program_redeemed_native_secondary_earnings_preserve_ledger_through_repair_and_close
C=inv_024_attributed_quote_value_conservation::terminal_earnings_succession::dual_quote_earnings_progress::native_secondary_earnings::v16_program_native_secondary_earnings_repair_excludes_donations_and_closes_both_rails
cargo test --locked --offline --test v16_cu "$N" -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu "$C" -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_073_native_secondary_earnings.rs
git diff --check
git diff --cached --check
git show --format= --check HEAD
git diff --exit-code 6c267d28 -- . ':!tests/invariants/cu/inv_073_native_secondary_earnings.rs' ':!tests/invariants/row433_native_secondary_redemption_20260918.md' ':!tests/invariants/README.md'
```

Touched-Rust formatting, diff/staged/show whitespace and the exact three-file
scope checks pass. No production, Cargo, fixture, support-helper or TSV changes.
