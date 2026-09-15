# Lane 10: terminal insurance after a paid-prefix beneficiary handoff

## Scope and provenance

- Isolated clone: `/tmp/percolator-lane10-terminal-insurance-20260915`.
- Local branch: `codex/lane10-terminal-insurance-20260915`.
- Freshly fetched base: `origin/codex/astra-invariant-cycle-20260915`,
  `2fca9fdfc2a31353f2b000e4f97fb956305982cc`.
- Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Default-feature wrapper rebuilt from this clone with platform-tools v1.52.
  SHA-256: `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
- Authenticated matcher rebuilt from this clone for the INV-079 runtime guards.
  SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
- Local evidence only: `scripts/loop.md`, the invariant README, finding/status
  TSVs, and existing invariant owners. No GitHub PR bodies or diffs were used.
  The original checkout was not modified; build directories are private.
  Read-only error/handler inspection diagnosed test rejection precedence.

The existing owner is [cu/inv_073_successor_custody_retry.rs](cu/inv_073_successor_custody_retry.rs),
mounted under `inv_073_no_permanent_user_lock::successor_custody_retry`.
Its original selector remains as a zero-paid-prefix classic-SPL control. The new
selector is
`v16_program_paid_insurance_prefix_survives_beneficiary_succession_on_both_rails`.
No production, dependency, harness-root, or machine-status file changes are made.

## Why this is new

| Existing owner | Prior coverage | Added relation |
| --- | --- | --- |
| `inv_073_successor_custody_retry.rs` | Succession before any withdrawal, missing successor ATA, stale former ledger | Succession after a committed payment, including native custody |
| `inv_073_no_permanent_user_lock.rs`, former-beneficiary-ledger selector | Unsigned successor payout and slab close; handoff precedes all payments | Former and successor retain different, exact shares of one original budget |
| `inv_073_native_insurance_ledger_progress.rs` | Native donations, lazy ledgers, redemption; one beneficiary throughout | A paid ledger remains bound to the former beneficiary across handoff |
| `inv_073_terminal_public_reserves.rs` | Reserve payout permutations and expiry | No duplicate claim for its principal/earnings/expiry matrix |
| Lane 2 Recovery cleanup and Lane 8 receipt liquidity owners | Provider epoch invalidation and native receipt expiry | Neither owns paid insurance beneficiary succession; those files are untouched |

## Public histories and oracle

Six worlds cross classic SPL versus native SOL custody with payments of 1, 19,
or 20 atoms to the former beneficiary. Insurance starts at 19/28 atoms in its
two domains. These prefixes leave the first domain positive, exactly empty, or
empty with one atom already debited from the second domain. All setup uses
System, ATA, SPL and public wrapper instructions. The existing native fixture
supplies the native mint genesis account; no economic state is injected.

Each world resolves the funded market, commits the first unsigned payment with
the former beneficiary's ledger, then transfers the insurance beneficiary role
with both holders' consent. All three role keys (former beneficiary, successor,
and operator) are dropped before the remaining measured actions. The fee payer
is explicitly distinct from every role and the administrator.

The finite continuation includes:

1. Replay of the original former-beneficiary bytes and a current-epoch version
   both reject `InvalidTokenAccount`: custody ownership is checked first.
2. Keeper ATA creation followed by the former authority with the correct new
   destination rejects `Unauthorized`. The current authority with an old epoch
   rejects `EngineStale`. Both bundles roll back the ATA and its rent debit.
3. ATA creation and a 17-atom unsigned payment execute before a suffix carrying
   the former beneficiary's ledger rejects. Logs establish successful ATA and
   wrapper prefixes; all tracked account bytes/lamports and payer rent roll back.
4. The same creation/payment bytes, with only the stale optional ledger omitted
   from the final payment, commit the successor's exact 46, 28, or 27 atoms.
   Both payments are positive, use only the fee payer's signature, and reduce
   the unpaid insurance rank to zero.
5. Administrator-signed `CloseSlab` executes before an unfundable System suffix
   rejects. Full transaction-account rollback restores market, vault and rent.
   The identical close then succeeds with exact tombstone and rent accounting.

The input-derived oracle checks each recipient's separate entitlement, both
remaining domain budgets, total insurance and vault stock, the complete decoded
market/configuration, authority epoch, beneficiary/operator profile, and the
complete former ledger record. Stock and encumbrance censuses run on every
checked terminal prefix. Token Account images and native lamports distinguish
rent from paid insurance. The former ledger, both paid destinations and mint
survive closure unchanged; the administrator receives rent only. Every measured
transaction is bounded by the owner's 300,000-CU limit.

## Obligations and limits

| Obligation | Bounded evidence |
| --- | --- |
| INV-067 | Exact recipient split and rejection of replay; no double payout after handoff or aborted cleanup |
| INV-070 | Zero insurance residue and actual vault/slab retirement after all entitlements are paid |
| INV-071 | Finite positive-value unsigned continuation, then a bounded mechanical close |
| INV-073 | Successor payout needs no beneficiary or operator signature after consensual handoff |
| INV-078 | Keeper repairs absent successor custody and completes payout despite a stale optional ledger |
| INV-082 | Concrete action per tested state: current beneficiary/epoch, keeper ATA creation, ledger omission, close |

These are insurance-only Resolved markets with no user portfolios or pending
claims. Provider principal/earnings, insurance consumption and recredit, late
backing expiry, multiple assets/quote rails, adversarial repeated handoffs,
maximum shapes, native redemption and ledger disposal remain outside this
increment. Native payout delivers wrapped native custody; it does not authorize
an absent beneficiary's later SPL redemption. Slab closure still requires the
administrator. This is finite conformance evidence, not a general liveness proof.

**Row 421 remains OPEN/missing.** INV-067/070/073 remain `REFUTED_CURRENT`;
INV-071/078/082 remain `OPEN_EVIDENCE`. No status promotion is justified.

## Validation

All commands run in the isolated clone. Build outputs use dedicated directories
under `/dev/shm` because the disk filesystem has little free space.

```sh
env CARGO_TARGET_DIR=/dev/shm/lane10-20260915-sbf CARGO_BUILD_JOBS=4 \
  RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --sbf-out-dir target/deploy -- --locked
sha256sum target/deploy/percolator_prog.so

env CARGO_TARGET_DIR=/dev/shm/lane10-20260915-auth-sbf CARGO_BUILD_JOBS=4 \
  RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --manifest-path tests/fixtures/auth_matcher/Cargo.toml \
  --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked
sha256sum target/deploy/percolator_prog.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so

export CARGO_TARGET_DIR=/dev/shm/lane10-20260915-host
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/tmp/percolator-lane10-terminal-insurance-20260915/target/deploy/percolator_prog.so
```

With that environment, the exact final test commands were:

```sh
cargo test --locked --offline --test v16_cu successor_custody_retry -- --nocapture
cargo test --locked --offline --test v16_cu -- --nocapture --test-threads=2 \
  native_insurance_ledger_progress \
  v16_program_terminal_insurance_exit_does_not_require_former_beneficiary_ledger \
  v16_program_terminal_disposition_and_administrative_retirement_are_source_complete \
  v16_program_terminal_public_reserve_disposition_preserves_value_across_orders \
  v16_program_public_reserve_payments_wait_for_resolved_senior_disposition
cargo test --locked --offline --test v16_program_fuzz_regressions \
  inv_079_public_reachability_evidence -- --nocapture \
  --skip v16_program_fixed_blockers_remain_progressing

rustfmt --edition 2021 --check tests/invariants/cu/inv_073_successor_custody_retry.rs
cargo fmt --all -- --check
git diff --check
git diff --exit-code -- src Cargo.toml Cargo.lock \
  tests/invariants/open_findings.tsv tests/invariants/invariant_status.tsv tests/fixtures
git diff --cached --check
git show --format= --check HEAD
```

Results:

- Touched module: **2 passed**, 0 failed, 1,412 filtered out, 2.42 seconds.
  The new selector completes six worlds, 18 committed insurance withdrawals,
  36 exact rejected transactions, and six slab/vault closures. Its original
  control adds two withdrawals, two rollbacks, and one closure.
- Adjacent selection: **6 passed**, 0 failed, 1,408 filtered out, 8.79 seconds.
  The native module contributes both its ledger/donation and redemption tests;
  the other four selectors cover former-ledger omission, source composition,
  twelve reserve-order/expiry worlds, and senior-claim precedence.
- INV-079: **16 passed**, 0 failed, 107 filtered out, 1.29 seconds. This includes
  thirteen metadata/source guards and three runtime trace/classifier guards.
  The broader fixed-blocker scenario runner was deliberately excluded.
- Peak observed new successor payment bundle: **139,364 CU**; stale-ledger
  rollback: **127,069 CU**; closure rollback: **34,288 CU**. All measured calls
  enforce the existing 300,000-CU bound; these are sampled, address-dependent
  observations, not worst-case CU certification.
- Scoped rustfmt and diff/production/status checks pass.
- `cargo fmt --all -- --check` reports pre-existing formatting in six unchanged
  files: `inv_024_terminal_reserve_destination_recovery.rs`,
  `inv_070_native_residue_disposition.rs`, `inv_073_dual_quote_reserve_progress.rs`,
  `inv_073_frozen_reserve_replacement.rs`, `inv_073_native_recredit_custody.rs`,
  and `inv_073_terminal_public_reserves.rs`, all under `tests/invariants/cu/`.
  Their contents match the fetched base; they are outside this edit.

Development first exposed an incorrect test expectation about rejection order:
an old destination fails `InvalidTokenAccount` before authority/epoch checks.
The final test isolates all three gates with valid prerequisite accounts and
keeps exact errors, rollback and payout assertions. A temporary Rust borrow
error was fixed by constructing retained instructions before mutating the VM.
The first INV-079 run passed thirteen guards and failed three because the clone
had no matcher SBF; rebuilding that fixture made all sixteen pass. No production
behavior was changed to address any of these test/environment issues.

No current implementation defect was found in these bounded histories. No
unfiltered suite, new stateful generator, engine proof or Kani run is claimed.
