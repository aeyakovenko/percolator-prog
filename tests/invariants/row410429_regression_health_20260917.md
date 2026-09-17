# Rows 410/429 regression health, 2026-09-17

Base: freshly fetched `origin/main`, `bc9908c8dd377a872b66c1f82dfe462e15bf4b93`.
Worktree: `/dev/shm/percolator-row410429-health-20260917`.
Branch: `worker/row410429-health-20260917`; local commit only, no push.
Scope: rows 410/429 and bounded INV-024/036/041/070/073/080/081 evidence.

Inputs: [README](README.md),
[payout/entitlement audit](terminal_payout_entitlement_audit_20260917.md),
[Scope S attribution](pr135_scope_s_reserve_beneficiary_attribution_20260913.md),
[role coalescence](terminal_role_coalescence_audit_20260912.md),
[role partition](terminal_role_partition_audit_20260912.md), and the mounted
reserve custody/keeper tests. No external PR branch or patch supplied the oracle.

## Added coverage

The new [public-route selector](cu/inv_024_recreated_beneficiary_handoff.rs)
composes a **spent payout and closed destination with a funded beneficiary merge
and return**. Existing generated role/expiry tests keep destination custody intact;
`recreated_reserve_close` keeps beneficiary ownership fixed; destination recovery
starts with empty destinations; provider keeper/ledger handoff changes the keeper,
not the beneficiary. Their separate successes do not exercise this composition.

Four histories cross the moved role (backing-fee beneficiary or insurance
beneficiary) with ATA recreation before/after the return handoff. The existing
public System/SPL/ATA/wrapper fixture constructs real earnings, resolves and pays
both users, and deletes their portfolios. No economic Account bytes are injected.
Inputs remain 100,000 backing, 875 earned fees, 31 insurance, user payouts
56,627/1,995,000, and fixed supply 2,152,533 atoms.

Each history pays 17 fee atoms and 11 insurance atoms to the original holders.
The moving holder spends its paid prefix to separate custody and closes its ATA,
then consensually transfers its still-funded role to the other reserve holder.
The test checks:

- Recreating the former holder's ATA cannot restore the transferred claim. The
  backing route rejects `Unauthorized`; insurance rejects `InvalidTokenAccount`,
  reflecting their distinct preflight order. Both roll back the ATA and rent.
- A bundle recreates custody, pays five atoms to the intermediate holder, returns
  the role, pays three atoms to the original holder, then rejects a wrong
  destination. Full compiled/tracked Account snapshots restore both payouts,
  custody/rent, the handoff, debit epochs, and both old/new holder ledgers, apart
  from exact transaction signature fees. Successful wrapper/ATA log prefixes
  establish that the failed suffix follows the intended completed instructions.
- The same serialized valid prefix retries unchanged and commits. A retained
  pre-handoff payment then rejects `EngineStale` despite the beneficiary identity
  and ATA address both having returned.
- Remaining fees, insurance and fresh principal pay without beneficiary signer
  metas. Administrator-signed CloseSlab leaves a typed tombstone, zero token
  residue, and exact slab/vault rent refund. All recipient and ledger Accounts
  remain unchanged by final retirement.

The input-maintained oracle tracks remaining claims, owner/class paid amounts,
spent custody, ledger-local paid/observation counters, role identities and the
complete control tuple. It checks whole SPL Account images, unchanged mint,
engine/raw vault agreement, reservation/stock censuses and market shape. Insurance
payments consume one authority epoch each; backing payments do not. The two
recreation orders reach the same independently required allocation:

| Moved role | Provider fees / insurance | Administrator fees / insurance | Spent prefix |
| --- | --- | --- | --- |
| Backing beneficiary | 870 / 0 | 5 / 31 | 17 fee atoms |
| Insurance beneficiary | 875 / 5 | 0 / 26 | 11 insurance atoms |

Gross payments in this table include the spent prefix; it never becomes a new
claim after recreation. Provider principal remains 100,000 in both cases.

INV-024 owns attribution; INV-036 receives fixed-policy fee destination evidence;
INV-041 receives only the two recreation-order comparisons; INV-070 receives
zero-residue retirement; INV-073 receives unsigned delivery after consensual
handoff; INV-080 receives exact failed-bundle rollback; INV-081 receives stock,
shape and control-state checks. No invariant or benchmark status is promoted.

## Validation

One exact selector only; no adjacent economic selectors or broad suite ran.
**PASS: 1 passed, 0 failed, 1,444 filtered; 2.32s.** Four histories execute 12
full-Account rollbacks, 28 committed payouts, eight committed handoffs and four
slab closures. The four failed mixed bundles restore eight completed payout
prefixes. Peak CU is **661,628**, below the 1,200,000 limit; every measured
transaction also fits the 1,232-byte packet bound. Setup CU is excluded.

Development found two test expectation errors, not production violations: the
displaced backing holder fails authority validation before token validation;
and the mixed recreation bundle exceeded the simpler role fixture's 600,000-CU
assertion after all economic checks passed. The final bound matches the reused
custody-repair helper's existing 1,200,000-CU transaction limit. An initial host
compile also required an explicit closure parameter type. No runtime history or
economic assertion was removed to obtain a pass.

Private host/SBF caches were copied, then the wrapper was rebuilt locked/offline
against current source and engine `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
Fresh SBF SHA-256:
`a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
No matcher SBF is needed by this bilateral-trade fixture. The host build passes
in 32.52s; the existing Solana future-compatibility warning remains.
Logs: `/dev/shm/percolator-row410429-health-20260917-logs/`:
`build-sbf.log`, `selector.log`, `development-error-order.log`,
`development-cu-bound.log`.

Exact commands from the worktree; exports spell out the environment passed via
`env` during execution:

```bash
cp -a --reflink=auto /dev/shm/percolator-public-gap-20260916-c91e-host-target /dev/shm/percolator-row410429-health-20260917-host-target
cp -a --reflink=auto /dev/shm/percolator-public-gap-20260916-c91e-sbf-target /dev/shm/percolator-row410429-health-20260917-sbf-target
export CARGO_TARGET_DIR=/dev/shm/percolator-row410429-health-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row410429-health-20260917-sbf-target/deploy/percolator_prog.so
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 TMPDIR=/dev/shm
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
CARGO_TARGET_DIR=/dev/shm/percolator-row410429-health-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row410429-health-20260917-sbf-target/deploy -- --locked
sha256sum "$PERCOLATOR_FUZZ_SBF"
cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::recreated_beneficiary_handoff::v16_program_recreated_reserve_beneficiary_return_preserves_spent_prefix_and_rollback -- --exact --nocapture
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_024_terminal_role_coalescence.rs tests/invariants/cu/inv_024_recreated_beneficiary_handoff.rs
git diff --check
git diff --exit-code bc9908c8 -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/*.tsv'
git diff --exit-code bc9908c8 -- . ':!tests/invariants/README.md' ':!tests/invariants/row410429_regression_health_20260917.md' ':!tests/invariants/cu/inv_024_terminal_role_coalescence.rs' ':!tests/invariants/cu/inv_024_recreated_beneficiary_handoff.rs'
git diff --cached --check
git show --format= --check HEAD
```

Targeted formatting, whitespace, committed-change and both scope guards pass.
The only existing Rust-file edit mounts the new module; existing test bodies and
shared helpers are unchanged.

## Kept-open gaps

No production issue was confirmed. This is one classic-SPL asset/domain, fixed
fee economics, fresh backing, an independent funded keeper and two consensual
handoffs. Beneficiary wallets remain present; reserve instructions omit their
signatures, while role transfers and final administrative retirement retain
their required signers. No Live shutdown-drain fallback, expiry, pending losses,
receipt competition, recredit, native/secondary rail, malicious deadline change,
unavailable market authority or arbitrary role-history closure is claimed.

The payout audit's older earned-fee succession failure was source-reviewed only;
its selector and other role/expiry products were not rerun or repaired here.
Rows 410/429 retain their existing COVERED ledger entries; invariant statuses are
unchanged. No row418/row428 file, `src`, Cargo file, fixture or status TSV changes
are included.
