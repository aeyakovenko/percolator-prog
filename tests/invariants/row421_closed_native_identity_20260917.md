# Row 421: closed native beneficiary identity

Base: freshly fetched `origin/main`, `9d15e72387121786925c204e1aaf4e93c4884b19`.
Worktree: `/dev/shm/row421-public-continuity-20260917-6f72oz`.
Branch: `codex/row421-public-continuity-6f72oz`. Local commit only.
Primary invariant: INV-073. Related obligations:
INV-017/018/021/027/064/067/069/070/071/078/082.

## Distinct coverage

The new selector in [the existing native insurance module](cu/inv_073_native_insurance_ledger_progress.rs)
adds exactly one history. Prior row421 notes and tests were inspected first:

- Native ledger/redemption witnesses keep beneficiary identity separate from
  custody and return redeemed SOL to a still-present beneficiary wallet.
- Missing-wallet/recredit and native-custody witnesses remove distinct wallets
  and custody before loss settlement; the latter recreates the beneficiary wallet
  by redeeming an old donated native account to it.
- Successor custody/stale-ledger and rows410/429 recreated-beneficiary histories
  change the beneficiary role; frozen-remainder coverage replaces unusable custody.
- INV-017's owner/destination alias witness exercises classic-SPL user receipts;
  INV-021's native alias witness concerns administrative slab refunds.
- Current-main row424 retired-custody coverage composes scan rediscovery and SPL
  ATA recreation with expiry, rather than removal of the beneficiary Account itself.

Here one publicly created SPL native account is also its own token owner and the
configured insurance beneficiary. The compiled unsigned payout aliases its readonly
identity and writable destination to one writable, nonsigner Account. Native
redemption then removes that same Account while a nonzero insurance claim and its
initialized ledger survive. The stored beneficiary identity remains sufficient for
keeper-only payout to a new ATA. This joins alias admission, identity disappearance,
custody replacement and ledger continuity on the insurance route.

## History and assertions

System/SPL instructions create rent-exact native custody; consensual public role
updates assign distinct beneficiary/operator keys. Native funding supplies the
37/61-atom domain budgets and public ledger sync observes all 98 atoms. The
operator key is dropped before public resolution.

1. A keeper-only payment delivers 41 atoms to the self-owned beneficiary account.
   Its compiled account is writable but unsigned; the remaining claim is 57.
2. Beneficiary-signed SPL CloseAccount redeems precisely 41 atoms plus custody rent
   to the administrator. This signature authorizes disposition of the paid prefix.
   The beneficiary key is then dropped and its Account is empty/absent.
3. The unchanged payment retained before redemption rejects the now-missing old
   destination with `InvalidTokenAccount`, preserving all Accounts except the fee.
4. The keeper prefunds the replacement ATA address with rent plus 19 lamports.
   A bundle initializes that System account as native custody and pays the remaining
   57 atoms, then rejects a one-atom overclaim against the emptied vault. Logs prove
   ATA creation and payout completed before rejection. Complete compiled/tracked
   Account snapshots restore prefunding, ledger, debit epoch, vault and claim,
   preserving the previously redeemed SOL prefix and absent beneficiary Account.
5. The identical creation/payment prefix commits with only the keeper signature.
   The ATA contains 76 atoms: 57 insurance plus 19 external prefunding atoms.
   Ledger withdrawn value is exactly 98, observed stock is zero, and no donation
   becomes principal, deposits, profit or loss. Both role identities stay unchanged.
6. Administrator-signed CloseSlab leaves the typed tombstone with exact rent,
   closes the empty vault, refunds rent exactly and frames paid custody and ledger.

The successful continuation after identity removal takes two transactions:
prefund, then create/pay. Payments decrease the insurance rank `98 -> 57 -> 0`;
custody operations frame that rank. Whole market-state comparisons, exact native
Account images, control/profile checks, stock/encumbrance censuses and shape
validation accompany the history. All measured transactions enforce 150,000 CU
and the 1,232-byte packet bound. Setup CU is excluded.

No program-owned bytes are injected. The reused native fixture only supplies
LiteSVM's omitted native-mint genesis account. Packing modified Account clones is
used solely for expected-value comparisons, never for SVM state writes.

## Verification

All selectors use the exact prefix
`inv_073_no_permanent_user_lock::native_insurance_ledger_progress::`:

| Exact selector suffix | Result | Peak CU |
| --- | --- | ---: |
| `v16_program_closed_native_insurance_identity_preserves_unsigned_ata_continuation` | PASS, one history, two rollbacks | 59,153 |
| `v16_program_native_insurance_paid_prefix_survives_operator_free_redemption_retry` | PASS, two existing histories | 42,594 |
| `v16_program_unsigned_native_insurance_ledger_excludes_donations_through_close` | PASS, four existing histories | 34,577 |

New history: successful-operation/rejection/slab-close peaks are
**50,538 / 59,153 / 18,878 CU**. The new exact run passes 1/1 in 0.39s;
the two exact controls pass 2/2 in 2.24s. No broad suite ran.

Development failures were test-only: an initially nonexistent error enum was
corrected to the actual token-balance preflight error; the shared creation helper's
one-SOL funding was replaced with explicit rent-sized System creation so native
custody starts empty. The resulting public history passes without production edits.

Private host/SBF caches were copied from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target`, then SBF was
rebuilt locked/offline from this worktree using platform-tools v1.52 and engine
`4db11a8cb0053815e23a35d3a7d3edc265d8d866`. Build PASS (7.85s).
SBF SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
Logs are outside the worktree in
`/dev/shm/row421-public-continuity-20260917-6f72oz-logs/`:
`build-sbf.log`, `candidate.log` (compile failure), `candidate-runtime.log`
(fixture failure), `candidate-rent-exact.log` (PASS), `controls.log` (PASS),
`checks.log`, `commit-checks.log`.

Exact commands, run from the worktree:

```bash
export CARGO_TARGET_DIR=/dev/shm/row421-public-continuity-20260917-6f72oz-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/row421-public-continuity-20260917-6f72oz-sbf-target/deploy/percolator_prog.so
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
CARGO_TARGET_DIR=/dev/shm/row421-public-continuity-20260917-6f72oz-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/row421-public-continuity-20260917-6f72oz-sbf-target/deploy -- --locked
cargo test --locked --offline --test v16_cu inv_073_no_permanent_user_lock::native_insurance_ledger_progress::v16_program_closed_native_insurance_identity_preserves_unsigned_ata_continuation -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_073_no_permanent_user_lock::native_insurance_ledger_progress::v16_program_unsigned_native_insurance_ledger_excludes_donations_through_close -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_073_no_permanent_user_lock::native_insurance_ledger_progress::v16_program_native_insurance_paid_prefix_survives_operator_free_redemption_retry -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true tests/invariants/cu/inv_073_native_insurance_ledger_progress.rs
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_073_native_insurance_ledger_progress.rs
git diff --check
git diff --cached --check
git diff --exit-code 9d15e723 -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/*.tsv'
git diff --exit-code 9d15e723 -- . ':!tests/invariants/cu/inv_073_native_insurance_ledger_progress.rs' ':!tests/invariants/README.md' ':!tests/invariants/row421_closed_native_identity_20260917.md'
git show --format= --check HEAD
git status --porcelain
```

Targeted rustfmt, both whitespace checks, protected-path and exact changed-file
guards, and committed-change check pass. The final worktree is clean.

## Remaining gaps

Row 421 remains OPEN. This history has one asset, two unspent insurance domains,
native-primary custody, a funded keeper, a fixed beneficiary and no active user
liabilities. It does not add loss/recredit, expiry, pending receipts, funded
secondary rails, authority rotation, maximum-shape or arbitrary-history coverage.
INV-064 live policy equivalence and INV-027 active principal priority receive no
new direct coverage. Beneficiary-authorized redemption precedes key loss; reserve
redemption itself is not claimed permissionless. Administrative retirement still
requires its signer, and the separate ledger is preserved, not closed.
Production, status ledgers, existing tests and shared helpers are unchanged.
