# Lane 13: Pending Users Across Provider Expiry

## Provenance

- Base: fetched `origin/codex/astra-invariant-cycle-20260915`,
  `89cd5088a9917d53cbbf8d4247be78adde89fd95`.
- Branch: `codex/lane13-terminal-provider-expiry-20260915`.
- Isolated network clone: `/tmp/percolator-lane13-terminal-provider-expiry-20260915`.
- Detached baseline worktree, owned by this clone: `/tmp/percolator-lane13-baseline-20260915`.
- Engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Fresh default-feature wrapper SBF SHA-256:
  `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
- Fresh authenticated matcher SBF SHA-256:
  `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

Both artifacts were built from this branch. Build outputs are private directories
under `/dev/shm` or the clone under `/tmp`. No protected or other-agent checkout
was edited. Production, dependencies, shared fixtures and all machine TSVs match
the fetched base.

## Coverage And Non-Duplication

Primary owner: INV-073, in existing
[`cu/inv_073_terminal_public_reserves.rs`](cu/inv_073_terminal_public_reserves.rs).
The new selector reuses `terminal_earnings_world_with_user_signers(false, None)`,
`reserve_payout` and the existing complete-Account transaction checker. It adds
no parallel fixture, harness plumbing or engine transition.

| Existing owner | Difference in this increment |
| --- | --- |
| Lane 10 `successor_custody_retry` | Lane 10 splits already-paid insurance across beneficiary succession in a market without portfolios or backing. Here the beneficiary is fixed before resolution, and earned provider fees coexist with positive user claims at expiry. No paid succession is repeated. |
| This file's reserve disposition matrix | That matrix completes user settlement/deletion before reserve payment and expiry. Here expiry precedes completion, and successful user settlement/normalization prefixes are composed with forbidden reserve suffixes. |
| This file's seniority control | The old control stops with paid but materialized portfolios. The new path crosses expiry, deletes users, repairs insurance custody, pays both distinct reserves and retires the slab. |
| INV-024 signed earnings expiry | That fixture starts after user deletion and retains beneficiary signatures. Here all measured reserve payouts use only the independent payer's signature. |
| Scope H terminal progress product | Its large reserve/expiry product uses earlier user settlement as setup. Here a positive source claim remains at each generated expiry boundary. |
| Scope D pending claimant reserve roles | Scope D expires backing after all five portfolios are deleted and has zero provider fees. Here earned fees survive expiry before deletion, with a closed insurance destination and unavailable reserve keys. |

Sixteen histories cross delivery at slot 100 or 101, expiry before any user
continuation or after the first successful continuation, both user orders, and
both provider-fee/insurance payout orders. The backing contract expires at 100.
Every expiry boundary asserts a positive aggregate source claim, remaining user
capital, an unfinished portfolio and a still-fresh provider bucket. Clock
advancement alone preserves the full market Account.

The public fixture supplies 52,502 and 2,000,000 user atoms, 100,000 provider
principal atoms and 31 insurance atoms. Its honest mark change creates 5,000
profit atoms and a subsequent fee-bearing trade earns the provider 875 atoms.
The input-derived terminal wallet vector is:

```text
winner     52,502 + 5,000 - 875 = 56,627
loser   2,000,000 - 5,000       = 1,995,000
provider earnings             = 875
operator                      = 0
insurance beneficiary         = 31
expiry-authorized retirement  = 100,000
```

The beneficiary accepts its role before resolution, then closes its empty ATA
through SPL. The provider, beneficiary and insurance-operator keypairs are
dropped. Their funded wallets remain present; this is unavailable-signature
coverage, with absent token custody for the insurer, not absent System wallets.
All construction uses System/ATA/SPL/wrapper routes. No economic Account image
is installed or mutated in LiteSVM. Local expected Account copies are comparison
oracles only.

At expiry, attempted principal and earnings withdrawals reject while users
remain materialized. A keeper ATA-creation prefix followed by insurance
withdrawal also rejects and restores custody absence and payer rent. Each
successful user continuation first executes in a transaction whose forbidden
earnings suffix rejects; complete account bytes/lamports roll back before the
identical continuation commits.

The independent lexicographic rank counts fresh buckets, impaired backing,
active user legs, account source liens, occupied source entries, negative PnL
and unpaid input-derived user entitlement. Every committed user continuation
decreases it. Four winner-first schedules reach a flat winner waiting for the
peer's settlement; the exact `EngineNonProgress` rollback is checked, followed
by successful peer and winner progress within the eight-opportunity bound.

Paid users still block insurance until their owner-signed mechanical deletion.
Deletion returns exact portfolio rent to the market. Then retained principal
bytes reject `EngineStale`, while keeper-funded ATA recreation and independent
provider earnings/insurance withdrawals succeed. Each payout first rolls back
with an expired-principal suffix; its epoch is adjusted for an insurance debit
so expiry, rather than a stale epoch, is the rejecting condition. Current-epoch
one-atom reserve overclaims reject even with 100,000 atoms still in custody.

Each checkpoint checks separate wallet entitlements, complete token/vault
Account images, mint supply, domain insurance budgets, provider earnings,
provider ledger identity and paid-fee telemetry, immutable role profile and the
insurance debit epoch. Existing stock and reservation censuses include every
remaining portfolio. The admin receives no quote. Final admin-signed slab
closure burns exactly 100,000 expired principal atoms, closes the vault, leaves
the expected tombstone and refunds exact rent; paid wallets and the fee ledger
remain framed by the transaction checker.

## Results And Limits

- New exact selector: **1 passed**, 0 failed, 1,418 filtered, 10.10 seconds.
  Sixteen worlds, 68 successful user continuations, 216 exact rollbacks,
  32 unsigned reserve payouts, 32 portfolio deletions and 16 slab/vault closes.
  Peak measured CU: **437,497**, below the asserted **600,000** bound.
- Adjacent selection: **3 passed, 1 failed**, 7.71 seconds. Passing controls:
  reserve seniority, twelve-world public disposition and the terminal source
  completeness guard.
- The unchanged signed-expiry control fails at its expected `Unauthorized`
  (`Custom(8)`) versus actual `InvalidTokenAccount` (`Custom(11)`) assertion.
  Its same exact selector reproduces on detached, untouched `89cd5088`:
  **0 passed, 1 failed**, 0.56 seconds, using the same freshly built SBF.
  Canonical destination validation rejects the wrong-recipient request before
  the later authority check. This is an existing test expectation failure;
  the selector and production were not changed by this lane.
- Selected INV-079 metadata/public reachability guards: **16 passed**, 0 failed.
  The broad fixed-blocker scenario runner is excluded.
- Scoped rustfmt, Git whitespace checks and production/TSV identity checks pass.

Development corrected zero-lamport SPL tombstone handling and expanded the
progress rank for real normalization/lien steps. The first between-expiry
winner schedule exposed the bounded waiting state described above. A provisional
400,000-CU local assertion was too low for the composed settlement/rejection
bundle (413,497); the final 600,000 bound covers the measured composition and
remains below the existing helper's 1,200,000 transaction limit. No compute
abort or loss/stranding defect was observed. An initial unqualified `--exact`
filter matched zero tests and is not counted as validation.

**No current implementation LoF/DoS/CU violation was established; no production
fix or red/green implementation claim is made. Rows 420 and 421 remain OPEN;
row 421 remains `missing`.** This is bounded INV-073 evidence with secondary
INV-024/063/067/070/071/078/080/082 checks, not a generic coverage promotion.
The users' ordinary positive source claims are pending here; this does not
cover bankruptcy residual debt or arbitrary retained receipt cohorts. Native
custody/redemption, spent-insurance recredit, multiple assets, maximum shapes,
arbitrary role histories, ledger disposal and missing cleanup signers remain
open. Portfolio deletion needs user signatures and slab closure needs admin.
No unfiltered suite or Kani run is claimed.

## Exact Commands

Build commands run from the isolated branch clone:

```sh
env CARGO_TARGET_DIR=/dev/shm/lane13-20260915-sbf CARGO_BUILD_JOBS=4 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/lane13-20260915-sbf/deploy -- --locked
env CARGO_TARGET_DIR=/dev/shm/lane13-20260915-auth-sbf CARGO_BUILD_JOBS=4 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --manifest-path tests/fixtures/auth_matcher/Cargo.toml --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked
sha256sum /dev/shm/lane13-20260915-sbf/deploy/percolator_prog.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so

export CARGO_TARGET_DIR=/dev/shm/lane13-20260915-host
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane13-20260915-sbf/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_public_reserves::v16_program_pending_users_cross_late_expiry_before_unsigned_reserve_payouts -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --nocapture --test-threads=2 v16_program_public_reserve_payments_wait_for_resolved_senior_disposition v16_program_terminal_public_reserve_disposition_preserves_value_across_orders v16_program_terminal_expiry_preserves_earned_fees_and_bounded_signed_disposal v16_program_terminal_disposition_and_administrative_retirement_are_source_complete
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence -- --nocapture --skip v16_program_fixed_blockers_remain_progressing

rustfmt --edition 2021 --check tests/invariants/cu/inv_073_terminal_public_reserves.rs
git diff --check
git diff --exit-code 89cd5088 -- '*.tsv' src Cargo.toml Cargo.lock tests/fixtures
git diff --cached --check
git show --format= --check HEAD
```

The equivalent per-command environment was supplied during execution. Baseline
failure confirmation, run from `/tmp/percolator-lane13-baseline-20260915`:

```sh
env CARGO_TARGET_DIR=/dev/shm/lane13-20260915-baseline-host CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 PERCOLATOR_FUZZ_SBF=/dev/shm/lane13-20260915-sbf/deploy/percolator_prog.so cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_earnings_expiry::v16_program_terminal_expiry_preserves_earned_fees_and_bounded_signed_disposal -- --exact --nocapture
```
