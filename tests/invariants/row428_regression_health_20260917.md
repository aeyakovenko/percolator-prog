# Row 428 retained insurance debit regression health, 2026-09-17

Base: freshly fetched `origin/main`, `959146284ff4febc6bce89c62c2b218048f6e045`.
Worktree: `/dev/shm/percolator-row428-health-20260917`.
Branch: `astra-row428-health-20260917`; local commit only, no push.

Inputs: [README](README.md), [retained-intent audit](inv_008_retained_intent_audit_20260917.md),
[retained reserve stock audit](retained_reserve_stock_audit_20260912.md),
[Live debit notes](pr135_scope_e_retained_debit_20260913.md),
[generated stock notes](astra_scope_g_retained_stock_epochs_20260914.md), and
the existing row-428 reserve-epoch, round-trip, native-recipient and insurer-succession witnesses.

## Distinct coverage

The [new test](cu/inv_008_resolved_debit_retry.rs) is mounted under the existing
INV-014 `reserve_debit_epoch` owner. Its evidence relates to
INV-008/010/024/064/080/081; it does not change any verdict.

| Existing evidence | Added dimension |
| --- | --- |
| Row-428 Live/Resolved reserve epoch selectors | Retained terminal requests through late failure, committed partial payment and stale retry |
| Live debit and generated stock histories | Both signed and genuinely permissionless Resolved deliveries, with one versus two required signatures |
| Insurance round-trip history | Successful debit epochs, rather than the shared top-up watermark or depleted-stock rejection |
| Expired principal/earned stock and native/succession histories | Two successive terminal debit epochs roll back together, then the retained continuation commits unchanged |

Eight public System/SPL/wrapper histories cross assets 0/1, signed-first versus
unsigned-first requests, and ledger-first versus no-ledger-first requests.
The four input budgets are 19/41/23/47 atoms; mint supply is fixed at 130 and
minting authority is revoked. The market is publicly resolved with no portfolios.
All twelve transaction envelopes are signed and serialized before the first
delivery, including explicitly predicted successor epochs. They are never rebound,
resigned or restored from snapshots.

Each history executes:

1. Each initial admission variant pays 29 atoms before a late SPL failure.
   A duplicate-debit bundle also pays once before its consumed epoch rejects.
   Complete rollback restores the epoch, both domain budgets and lazy ledger.
2. The retained alternate variant commits its 29-atom partial payout. Both old
   variants reject with `EngineStale`, while 31 or 41 target atoms still cover
   the entire stale amount. This distinguishes epoch rejection from exhaustion.
3. Two successive seven-atom payouts reach a late SPL failure, restoring both
   epoch increments. A fresh-prefix/stale-suffix bundle also rolls back. The
   pre-signed two-payout continuation then commits, advancing exactly twice;
   its opposite signing/ledger variant rejects stale while funds remain.
4. Pre-signed permissionless requests pay the target remainder and peer budget
   completely. Final target receipts total 60 or 70 atoms, the peer gets the
   other budget, and vault and all four domain budgets are zero. Both assets
   use the same configured beneficiary wallet; this is not independent-owner evidence.

Every rejection compares all tracked and compiled Accounts, including absence,
bytes, metadata and lamports. Only the payer loses the exact signature fee.
The deliberately empty SPL source belongs to that payer, so inducing the late
error does not add an insurance-authority signature to permissionless requests.
Exact error indexes/codes and successful wrapper/SPL log counts prove the prefix
executed. Every checkpoint checks complete token/mint/ledger Accounts, all control
lanes, profiles and generations, input-derived budgets and payouts, market account
metadata, engine shape, and independent stock and encumbrance censuses.

## Results and limits

New selector on merged main: **1 passed, 0 failed, 1,443 filtered**, 3.29s, exit 0.
**8 histories, 96 transactions, 64 exact rollbacks, 48 restored SPL transfers,
40 payouts**; peak **94,096 CU**, maximum packet **639 bytes**.
The first host compile found a missing test-only `BTreeMap` import and executed
no tests; after correcting it and adding census checks, the selector passed.
No production issue was observed; no production, dependency, fixture or TSV edit.
The existing Solana-client future-compatibility warning remains.

Row 428 stays `COVERED` at this base; historical OPEN descriptions are not a
status override. This is a bounded regression, not a new stock-sequence design
or arbitrary-history proof. It uses one classic SPL rail and the same withdrawal
handler with two signer/admission variants. Native/secondary custody, distinct
beneficiaries, new funding, role changes, terminal recredit, outstanding claims,
Live-to-Resolved composition, blockhash expiry and durable nonces remain open
composition dimensions. No row425/426 or row419/435 file was edited.

## Exact verification

All commands below ran in the isolated worktree except fetch/worktree creation,
which ran in `/dev/shm/percolator-main-merge-20260916`. Private copies of existing
dependency caches were used; SBF and host tests were rebuilt from this checkout.
No matcher artifact is needed. Engine pin: `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
SBF SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
Logs: `/dev/shm/percolator-row428-health-20260917-{sbf,test}.log`.

```bash
git fetch origin main
git worktree add -b astra-row428-health-20260917 /dev/shm/percolator-row428-health-20260917 origin/main
cp -a --reflink=auto /dev/shm/percolator-public-gap-20260916-c91e-host-target /dev/shm/percolator-row428-health-20260917-host-target
cp -a --reflink=auto /dev/shm/percolator-public-gap-20260916-c91e-sbf-target /dev/shm/percolator-row428-health-20260917-sbf-target
env CARGO_TARGET_DIR=/dev/shm/percolator-row428-health-20260917-sbf-target TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row428-health-20260917-sbf-target/deploy -- --locked
env CARGO_TARGET_DIR=/dev/shm/percolator-row428-health-20260917-host-target PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row428-health-20260917-sbf-target/deploy/percolator_prog.so TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked --offline --test v16_cu inv_014_delayed_policy_and_policy_epoch_safety::reserve_debit_epoch::resolved_debit_retry::v16_resolved_retained_debit_retries_restore_epochs_across_signed_and_unsigned_delivery -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_008_resolved_debit_retry.rs tests/invariants/cu/inv_014_reserve_debit_epoch.rs
git diff --check
git diff --exit-code 959146284ff4febc6bce89c62c2b218048f6e045 -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/*.tsv'
git diff --exit-code 959146284ff4febc6bce89c62c2b218048f6e045 -- . ':!tests/invariants/README.md' ':!tests/invariants/row428_regression_health_20260917.md' ':!tests/invariants/cu/inv_008_resolved_debit_retry.rs' ':!tests/invariants/cu/inv_014_reserve_debit_epoch.rs'
git diff --cached --check
git show --check HEAD
```

Build, targeted formatting, whitespace and scope guards pass. Only the new exact
selector ran; no existing selector, metadata gate or broad suite was rerun.
