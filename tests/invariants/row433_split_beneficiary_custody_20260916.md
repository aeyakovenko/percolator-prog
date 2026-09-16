# Row 433: split insurance beneficiaries with shared custody

## Isolation and scope

- Independent clone: `/dev/shm/percolator-row420-433-custody-20260916`.
- Remote: `git@github.com:aeyakovenko/percolator-prog.git`.
- Requested base: `origin/codex/astra-invariant-cycle-20260915`,
  `5594c529f52c7fdd70afc266be229d58d4677e88`.
- Local branch: `codex/row433-insurance-custody-20260916`.
- Protected baseline: `origin/main`,
  `d809e9a563d9b8bf38f32648b32a15d75f526ec8`.
- Changes: this report, the new
  [split-custody module](cu/inv_073_split_beneficiary_custody.rs), and three lines
  registering it in [the succession owner](cu/inv_073_successor_custody_retry.rs).
- No edits to `/home/anatoly/percolator-prog`, production, dependency manifests,
  lockfiles, fixture files, any TSV, row417 receipt ordering, or row419/435
  ADL/residual modules. Local commit only; no push.

The default-feature wrapper was freshly built with platform-tools v1.52 in the
clone's ignored `target`; no other lane's artifacts were copied. The SBF at
`target/deploy/percolator_prog.so` has SHA-256
`e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
No matcher artifact is needed. Logs and scratch files are private to this clone.

## New invariant product

The new selector is:

```text
inv_073_no_permanent_user_lock::successor_custody_retry::split_beneficiary_custody::v16_program_split_insurance_succession_replaces_shared_custody_without_redirecting_peer
```

Two assets initially share insurance beneficiary A and its token account. Only
one asset transfers to B. A separately transfers ownership of the shared token
account to B, including already-paid tokens. That account becomes valid custody
for B's asset and invalid custody for A's remaining asset. The peer request's
market identity, authority epoch, amount, beneficiary and ledger remain current.
Its original bytes must reject the wrong destination owner; changing only its
destination to keeper-created A-owned custody must permit payment.

Four worlds cross the transferred asset (0/1) and which remaining claim is paid
first. This tests a coupled custody failure across otherwise independent asset
epochs, not merely another missing-wallet or beneficiary-rotation example.

| Existing evidence | Distinct obligation here |
| --- | --- |
| `inv_073_successor_custody_retry.rs` | One asset transfers to missing successor custody. Here one shared address becomes valid for the successor and invalid for the still-funded peer, whose epoch must remain usable. |
| `inv_073_disabled_beneficiary_restoration.rs` | One asset crosses A/B/A restoration and delegated custody. Here the roles stay split across two assets and one voluntary SPL ownership transfer must not redirect the peer claim. |
| `inv_082_terminal_reassigned_custody.rs` | Reassigned custody with fixed reserve roles and signed insurance withdrawal. Here consented per-asset succession composes with keeper-only reserve payments and separate asset ledgers. |
| `inv_073_frozen_reserve_replacement.rs` | Fixed beneficiaries and frozen destinations. Here the original destination actively receives the successor's payment while replacement custody receives the incumbent's peer claim. |
| `inv_073_distinct_provider_disposition.rs` | Independent provider claims across two assets. Here the initial insurance beneficiary and custody are shared, and only one asset's beneficiary changes. No new provider-earnings claim is made. |

## Public trace and accounting

All economic state is created through public System, SPL, ATA or wrapper
instructions. Program loading, signer airdrops and blockhash advancement are
runtime scaffolding. Expected Account images are detached comparisons and are
never installed into LiteSVM. The mint authority is revoked after minting 118
atoms. There are no portfolios, receipts, positions, funding, ADL, losses,
provider backing, expiry, donations, or surplus in these histories.

1. Configure A as both assets' beneficiary, with a separate insurance operator.
   Fund domain budgets `[19, 28]` and `[31, 40]`, then resolve publicly with the
   administrator. Create separate zeroed insurance ledgers for A's two asset
   claims and B's eventual claim using System account creation.
2. Keeper-only payouts return prefixes 17 and 23 into the shared A-owned ATA.
   Each initializes only its matching ledger and advances only its asset epoch.
   Asset remainders are now 30 and 48; retain the peer's full-remainder request.
3. Bundle consented target succession, SPL ownership transfer, a complete B
   payout, and the unchanged peer request. The peer rejects
   `InvalidTokenAccount` after both wrapper prefixes complete. Exact rollback
   restores consent, custody ownership, B's ledger initialization and all funds.
4. Commit succession and the SPL transfer, then drop A, B and operator keys.
   The unchanged peer request still rejects, despite its current epoch. The
   existing canonical ATA remains occupied under B and retains the 40 paid atoms.
5. A keeper uses `CreateAccountWithSeed` and `InitializeAccount3` to create
   alternate A-owned custody. Creation plus the peer payment rolls back when a
   target suffix supplies A's former-target ledger (`Unauthorized`). The same
   prefix also rolls back against a target suffix with the pre-succession epoch
   (`EngineStale`). All accounts, rent, peer epoch and ledger writes restore.
6. Commit replacement creation and the first tail payment, in either order of
   beneficiaries. With positive peer funds still in the vault, a current-epoch
   one-atom overclaim on the exhausted asset rejects `EngineLockActive`.
   Administrator-signed slab closure also rejects while the other claim remains.
7. The final keeper payment and valid signed closure both succeed before an
   impossible System transfer fails. Exact rollback restores both ledgers,
   custody, slab, vault and refunded rent. Commit the identical final payment
   with only the keeper signing. Unsigned closure still rejects `ExpectedSigner`;
   administrator-signed closure then finishes with exact tombstone and rent.

Input-derived expected attribution is:

| Transferred asset | Shared custody, now owned by B | Alternate A custody | Vault at completion |
| --- | ---: | ---: | ---: |
| 0 | 40 paid atoms voluntarily transferred by A + 30 reserve atoms = 70 | 48 | 0 |
| 1 | 40 paid atoms voluntarily transferred by A + 48 reserve atoms = 88 | 30 | 0 |

The SPL ownership transfer conveys the already-paid 40 atoms; it does not itself
transfer any remaining wrapper entitlement. A's former-target ledger stays at
its original paid prefix. A's peer ledger reaches 47/71 total withdrawn, and B's
ledger records only 30/48. All profit, loss, principal and deposit fields remain
zero. Complete token, mint and ledger images are checked, including frame checks
through closure. The market oracle checks full decoded config/group state,
per-domain long-before-short budget depletion, all per-asset control sequences,
role profiles, physical custody, stock census and reservation census.

The transaction runner checks signatures, the 1232-byte bound, exact failure
indices and codes, and successful wrapper-prefix logs. Failed transactions
compare every tracked and compiled Account, allowing only exact signature fees.
Successful transactions frame all unrelated Accounts and account for keeper
fees and replacement rent. Each final tail lowers unpaid insurance by its full
input-derived remainder; two committed tail calls suffice in either order.

## Validation and limits

| Command group | Result |
| --- | --- |
| Fresh locked/offline SBF build | PASS, 26.52 seconds |
| New exact selector | PASS 1/1, four worlds, 32 rollbacks, 16 committed unsigned payments, four closures; 1.73 seconds; peak 60,005 CU |
| Entire succession owner, including the new selector and four adjacent controls | PASS 5/5, 10.04 seconds; 1448 filtered out; new-selector peak 78,005 CU |
| Touched-file `rustfmt --check` | PASS |
| Worktree, staged and committed whitespace checks | PASS |
| Protected diff against requested base and `origin/main` | PASS, empty |

The new selector enforces a 300,000-CU ceiling on instrumented continuation
transactions. Setup is outside this measurement; this is not maximum-shape or
worst-case CU evidence. The first compile used a nonexistent test-side enum name
`EngineStaleAuthority`; correcting it to the branch's `EngineStale` resolved the
compile error. No runtime property violation, LoF, DoS or CU bug was found.
The existing Solana dependency future-compatibility warning remains.

This is bounded classic-SPL coverage with two assets, cooperative succession
and custody-transfer signatures before keys are dropped, a funded independent
keeper, and an available administrator for resolution and mechanical closure.
It does not cover arbitrary schedules, native/secondary quotes, pending user
claims, unavailable succession consent, subsequent beneficiary spending,
provider claims, or permissionless slab closure.

**Rows 420, 421 and 433 remain OPEN. INV-073 remains REFUTED_CURRENT.**
No TSV or machine status is changed. Row433 gains the finite custody/succession
product above; no new row420 coverage or generic liveness closure is claimed.

## Reproduction

The commands below use the same environment as the executed per-command `env`
invocations. Paths resolve entirely inside the isolated clone.

```bash
cd /dev/shm/percolator-row420-433-custody-20260916
export CARGO_TARGET_DIR=/dev/shm/percolator-row420-433-custody-20260916/target
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export TMPDIR=/dev/shm/percolator-row420-433-custody-20260916/target/row433-tmp
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row420-433-custody-20260916/target/deploy/percolator_prog.so

env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row420-433-custody-20260916/target/deploy -- --locked > target/row433-logs/build-sbf.log 2>&1
sha256sum target/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu inv_073_no_permanent_user_lock::successor_custody_retry::split_beneficiary_custody::v16_program_split_insurance_succession_replaces_shared_custody_without_redirecting_peer -- --exact --nocapture --test-threads=1 > target/row433-logs/new-test.log 2>&1
cargo test --locked --offline --test v16_cu inv_073_no_permanent_user_lock::successor_custody_retry:: -- --nocapture --test-threads=1 > target/row433-logs/succession-controls.log 2>&1

rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_073_successor_custody_retry.rs tests/invariants/cu/inv_073_split_beneficiary_custody.rs
git diff --check
git diff --exit-code origin/main -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/*.tsv' ':(glob)tests/invariants/**/*.tsv'
git diff --exit-code 5594c529f52c7fdd70afc266be229d58d4677e88 -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/*.tsv' ':(glob)tests/invariants/**/*.tsv'

git add tests/invariants/cu/inv_073_successor_custody_retry.rs tests/invariants/cu/inv_073_split_beneficiary_custody.rs tests/invariants/row433_split_beneficiary_custody_20260916.md
git diff --cached --check
git -c user.name=Codex -c user.email=codex@openai.com commit -m 'test(invariants): cover split insurance beneficiaries sharing custody'
git show --format= --check HEAD
git diff --exit-code origin/main HEAD -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/*.tsv' ':(glob)tests/invariants/**/*.tsv'
git status --short --branch
git rev-parse HEAD
```
