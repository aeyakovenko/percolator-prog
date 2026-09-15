# Lane 9: pending loss and mixed-role debt across shutdown

## Disposition and provenance

- Isolated clone: `/tmp/percolator-lane9-pending-loss-debt-20260915`.
- Local branch: `codex/lane9-pending-loss-debt-20260915`.
- Freshly fetched base: `origin/codex/astra-invariant-cycle-20260915`,
  `2fca9fdfc2a31353f2b000e4f97fb956305982cc`.
- Tested code/README commit: `f041ba2d2533d7d826dc932f591fedb2d961fdd2`.
  This report is a documentation-only descendant of that commit.
- Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Production changed: **no**. Manifests, locks, machine TSVs, shared harnesses,
  and other lanes' test files are unchanged.
- Row **419 remains OPEN/missing**, with added pending-weight durability across
  shutdown of the obligation's own asset before resolution.
- Row **435 remains OPEN/missing**, with added mixed-role owner attribution
  across shutdown of either participating asset or both in either order.
- INV-039 remains `REFUTED_CURRENT`; no machine disposition is promoted.
- No genuine public-route LoF or persistent DoS was discovered in these bounded
  histories. There is no red public-route regression or production fix proposed.

The original `/home/anatoly/percolator-prog` was only read for its remote URL;
all fetching, building, editing and committing occurred in the isolated clone.
No GitHub PR body/diff, row-specific patch, or alternative finding branch was
inspected. Evidence selection used `scripts/loop.md`, the relevant sections of
`tests/invariants/README.md`, `open_findings.tsv`, `invariant_status.tsv`, and
the existing invariant owners and their public harness helpers.

## Gap and overlap review

The INV-039 stateful owner states the invariant: pending accrual and loss
obligations cannot be erased by route choice or lifecycle changes. The coverage
question was whether shutting down an asset that actually carries one of a
portfolio's two economic roles preserves both obligations through resolution.

| Existing owner | Existing boundary | Added boundary |
| --- | --- | --- |
| `cu/inv_039_mixed_role_resolution.rs` | Pending creditor plus unsettled debtor; direct resolution, two debt/support regimes, live/resolved booking and terminal order | Actual creditor/debtor asset shutdown before resolution |
| `cu/inv_039_pending_loss_restart.rs` and its trading child | Shutdown/restart of an unrelated sibling asset | Shutdown of an asset carrying the mixed owner's obligation |
| `stateful/inv_039_pending_loss_obligation_durability.rs` | Funding accrual versus shutdown, reduction and Recovery forfeit | Same portfolio retains a zero-basis creditor weight and a cross-asset debt across shutdown |
| `cu/inv_039_mixed_role_funding_resolution.rs` and insurance child | Nonzero funding, deferred debt and typed insurance attribution through resolution | Lifecycle composition; these active-lane owners were not edited |
| `cu/inv_039_mixed_role_fractional_retirement.rs` | Fractional peer-source conversion and backing expiry through retirement | Pre-resolution shutdown of either economic role |

The new selector lives in the existing mixed-role owner and reuses `setup`,
`Book::check`, `Book::close`, and the normal public `AttributionWorld` fixture.
It does not introduce a duplicate fixture or entitlement model.

## Witness and oracles

Selector:
`inv_039_pending_loss_obligation_durability::mixed_role_resolution::v16_program_mixed_role_shutdown_preserves_pending_debt_and_terminal_entitlement`.

The public setup deposits `[400000, 180000, 300000, 250000, 777]` atoms. Actor 0
earns a 200000-atom face against actor 1, whose capital falls short by 20000.
A signed matched close leaves actor 0 with a zero-basis, nonzero-weight creditor
leg. Actor 0 also owes actor 2 either 36000 or 240000 atoms on another asset;
that K debt remains unsettled at shutdown and resolution. These inputs exercise
both partial source-support consumption and complete source-face retirement.

The 64 worlds cross two debts, two side orientations, two asset assignments,
four shutdown schedules (creditor, debtor, creditor then debtor, debtor then
creditor), and two terminal payout orders. Each shutdown uses the public
`UpdateAssetLifecycle` route and is required to reach `Recovery`.

Each of the 96 shutdowns first executes successfully before an unsigned
`ClosePortfolio` suffix fails at instruction 3 with `ExpectedSigner`. Exactly
one successful wrapper log proves that the lifecycle prefix ran. The entire
tracked Account frame rolls back: market, mint, vault, vault authority, admin,
and all owner wallets, portfolios and destinations. The distinct network fee
payer is intentionally excluded. Reusing the identical authorized shutdown
instruction then commits, preserves all source-credit records, and changes no
tracked Account except the market.

At each shutdown/resolve boundary, the existing independent book still requires
unbooked B, uncharged pending weight, and unsettled K debt. Every terminal
prefix checks owner-local capital + PnL + unpaid receipt + SPL payout, source
membership, the exact close residual partition, OI, stored/pending counts,
loss-weight aggregates, stock/reservation censuses and SPL supply. The book
also rejects a hypothetical conserved one-atom transfer to the wrong owner.
No economic state bytes or private engine transitions construct the witness.

Terminal expectations are calculated from the inputs, not inferred from a
green control's balances:

| Cross-asset debt | Owner SPL payouts `[0,1,2,3,4]` | Residual vault |
| --- | --- | --- |
| 36000 | `[540000, 0, 336000, 250000, 777]` | 4000 |
| 240000 | `[320000, 0, 540000, 250000, 777]` | 20000 |

All worlds reach terminal payout within the explicit 16-round bound (at most
80 `CloseResolved` attempts). A nonterminal round must change the tracked
state, and each successful close must change it. The book's booked/charged/
settled flags are monotonic. Fully paid receipt retries are exact no-ops.
Every world deletes all five portfolios and preserves the unrelated asset-0
engine state. This is a finite completion witness, not a general rank theorem.

## Results

| Validation | Result |
| --- | --- |
| New mixed-role shutdown selector | PASS: 64 worlds, 96 successful-prefix rollbacks and retained shutdowns, 64 waiting rollbacks, 64 paid receipt retries; peak 212217 CU under 300000 |
| Existing mixed-role direct-resolution selector | PASS: 32 worlds, 24 waiting rollbacks, 32 receipt retries; peak 210717 CU |
| Existing mixed-role funding selector | PASS: 4 worlds; peak 204545 CU |
| Existing mixed funding/insurance selector | PASS: 12 worlds; peak 161949 CU |
| Existing mixed fractional-retirement selector | PASS: 32 worlds, 160 rollback checks, 32 receipt retries, 16 waiting rollbacks, 96 terminal calls; peak 190140 CU |
| INV-079 selection | PASS: all 16 selected tests, including all 13 metadata/source guards and three public trace/classifier checks |
| Scoped rustfmt and Git whitespace checks | PASS |
| Repository-wide `cargo fmt --all -- --check` | FAIL: pre-existing formatting in six unchanged files listed below |

The first INV-079 invocation passed all 13 metadata/source guards but three
trace checks failed because this fresh clone lacked its auth matcher artifact.
Building that fixture from this branch and rerunning the identical selection
gave 16/16 passing. The unrelated `v16_program_fixed_blockers_remain_progressing`
runtime campaign was explicitly skipped, so this is not a claim that the full
INV-079 or repository suite was run. No unfiltered suite or Kani run was made.

Repository formatting differences are confined to these files, all byte-identical
to the fetched base according to `git diff --exit-code`:

- `cu/inv_024_terminal_reserve_destination_recovery.rs`
- `cu/inv_070_native_residue_disposition.rs`
- `cu/inv_073_dual_quote_reserve_progress.rs`
- `cu/inv_073_frozen_reserve_replacement.rs`
- `cu/inv_073_native_recredit_custody.rs`
- `cu/inv_073_terminal_public_reserves.rs`

## Artifacts and exact commands

Default-feature wrapper SBF was built in this clone with platform-tools v1.52:

- `/dev/shm/percolator-lane9-20260915-target/deploy/percolator_prog.so`
- SHA-256: `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`

The auth matcher was also built from the clone's unchanged fixture sources:

- `tests/fixtures/auth_matcher/target/deploy/auth_matcher.so`
- SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`

The tests load LiteSVM's installed SPL Token/ATA fixture programs through the
existing harness. Host tools: Cargo 1.90.0 and rustfmt 1.8.0-stable. Isolated
build outputs use `/dev/shm` because the root filesystem had little free space.

Provisioning, with clone executed from `/tmp`, then other commands from the clone:

```bash
git clone --single-branch --branch codex/astra-invariant-cycle-20260915 git@github.com:aeyakovenko/percolator-prog.git /tmp/percolator-lane9-pending-loss-debt-20260915
git fetch origin codex/astra-invariant-cycle-20260915
git switch --create codex/lane9-pending-loss-debt-20260915 origin/codex/astra-invariant-cycle-20260915
git rev-parse HEAD
```

Builds:

```bash
env CARGO_TARGET_DIR=/dev/shm/percolator-lane9-20260915-target CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-lane9-20260915-target/deploy -- --locked
env CARGO_TARGET_DIR=/dev/shm/percolator-lane9-20260915-matcher-target CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked
sha256sum /dev/shm/percolator-lane9-20260915-target/deploy/percolator_prog.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
```

Focused tests, all with the branch-built wrapper explicitly selected:

```bash
env CARGO_TARGET_DIR=/dev/shm/percolator-lane9-20260915-target CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-lane9-20260915-target/deploy/percolator_prog.so cargo test --locked --offline --test v16_cu inv_039_pending_loss_obligation_durability::mixed_role_resolution::v16_program_mixed_creditor_debtor_roles_preserve_owner_entitlement_through_resolution -- --exact --nocapture
env CARGO_TARGET_DIR=/dev/shm/percolator-lane9-20260915-target CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-lane9-20260915-target/deploy/percolator_prog.so cargo test --locked --offline --test v16_cu inv_039_pending_loss_obligation_durability::mixed_role_resolution::v16_program_mixed_role_shutdown_preserves_pending_debt_and_terminal_entitlement -- --exact --nocapture
env CARGO_TARGET_DIR=/dev/shm/percolator-lane9-20260915-target CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-lane9-20260915-target/deploy/percolator_prog.so cargo test --locked --offline --test v16_cu inv_039_pending_loss_obligation_durability::mixed_role_resolution::funding_resolution:: -- --nocapture
env CARGO_TARGET_DIR=/dev/shm/percolator-lane9-20260915-target CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-lane9-20260915-target/deploy/percolator_prog.so cargo test --locked --offline --test v16_cu inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::v16_program_mixed_role_fractional_source_preserves_attribution_through_expiry_and_retirement -- --exact --nocapture
env CARGO_TARGET_DIR=/dev/shm/percolator-lane9-20260915-target CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-lane9-20260915-target/deploy/percolator_prog.so cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence:: -- --skip v16_program_fixed_blockers_remain_progressing --nocapture
```

The successful INV-079 retry used the last command inside
`bash -o pipefail -c '... 2>&1 | tail -n 35'` to limit repeated compiler warnings;
the selection, environment and exit-status checking were unchanged.

Formatting, scope and commit checks:

```bash
rustfmt --edition 2021 --config skip_children=true tests/invariants/cu/inv_039_mixed_role_resolution.rs
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_039_mixed_role_resolution.rs
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git diff --exit-code 2fca9fdfc2a31353f2b000e4f97fb956305982cc -- src Cargo.toml Cargo.lock tests/invariants/*.tsv
git diff --exit-code 2fca9fdfc2a31353f2b000e4f97fb956305982cc -- tests/invariants/cu/inv_024_terminal_reserve_destination_recovery.rs tests/invariants/cu/inv_070_native_residue_disposition.rs tests/invariants/cu/inv_073_dual_quote_reserve_progress.rs tests/invariants/cu/inv_073_frozen_reserve_replacement.rs tests/invariants/cu/inv_073_native_recredit_custody.rs tests/invariants/cu/inv_073_terminal_public_reserves.rs
git add tests/invariants/cu/inv_039_mixed_role_resolution.rs tests/invariants/README.md
git -c user.name='anatoly yakovenko' -c user.email=anatoly@solana.com commit -m 'test(inv-039): preserve mixed-role debt across asset shutdown'
git add tests/invariants/lane9_pending_loss_debt_20260915.md
git -c user.name='anatoly yakovenko' -c user.email=anatoly@solana.com commit -m 'docs: record lane 9 pending-loss verification evidence'
git show --format= --check HEAD
git diff --check 2fca9fdfc2a31353f2b000e4f97fb956305982cc HEAD
git status --short
git log -2 --format='%H %s'
```

The initial commit attempt found no Git identity in the fresh clone. The
successful commits use the existing branch author's identity per command;
no global Git configuration was written. No commits were pushed.

## Remaining limits and changed files

This increment uses fixed honest AuthMark observations, integral quantities,
zero funding and fees, classic SPL custody, no insurance/backing-provider
top-ups, and no ADL. The new test does not establish funding-bearing mixed-role
shutdown, reduction/forfeit instead of resolve, malicious-oracle behavior,
arbitrary schedules, maximum shape, slab retirement, or generic INV-086
equivalence. Admin supplies the legitimate shutdown and subsequent resolve;
the terminal payouts are permissionless public calls. It does not prove that
every Recovery state has an admin-independent escape. The exact remaining
vault residue is measured, not represented as a fully retired market.

Final changed files:

- `tests/invariants/cu/inv_039_mixed_role_resolution.rs`
- `tests/invariants/README.md`
- `tests/invariants/lane9_pending_loss_debt_20260915.md`

These observations improve bounded coverage of rows 419 and 435 without
claiming either normalized missing finding has been independently rediscovered
or closed.
