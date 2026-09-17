# Row 428 native replenishment across resolution, 2026-09-17

Base: freshly fetched `origin/main`, `4a3e7c8d57def60d8468cb2c0d1e7863403029d1`.
Private worktree: `/dev/shm/percolator-astra-ultra-row428-20260917`.
Branch: `astra-ultra/row428-native-stock-replenishment-20260917`; local commit only.
Owner: INV-064, with bounded INV-008/010/024/031/080/081 evidence.

## Non-duplicate increment

[The new child](cu/inv_064_native_replenishment_epoch.rs) is mounted under the
existing INV-064 CU owner. It composes complete Live depletion, independent
native funding, resolution and retained signed/permissionless withdrawals
across native-primary and classic-secondary custody with two distinct owners.

| Existing evidence | Added dimension |
| --- | --- |
| [Recent Row 428 health](row428_regression_health_20260917.md) | Live-to-Resolved history, depleted then replenished stock, two quote rails and distinct beneficiaries |
| [Generated stock epochs](pr135_scope_q_retained_insurance_stock_epochs_20260913.md) | Native lamports, terminal delivery, independently owned sibling epoch and ledger |
| [Native recipient recreation](cu/inv_008_insurance_native_recreation.rs) | Successful debit itself consumes consent; no role handoff is needed |

The scenario does not extend Rows 417, 421, 424 or 433. There are no user claims,
backing expiry, insurance recredit, scanner histories or earnings withdrawals.

## Public history and oracle

Four histories cross the first withdrawal rail with signed versus keeper-only
terminal partial delivery. The established native fixture supplies the missing
native-mint genesis account; every economic account uses public System, ATA,
SPL or wrapper construction. Native source accounts receive exact rent. No
program-owned bytes are installed, altered or restored in LiteSVM.

Two different wallets are each the insurance authority and operator of their
own asset. Public native deposits fund 37 target atoms and 61 sibling atoms,
with a separate 83 atoms retained in the target's source account. A revoked
classic mint supplies exactly 200 secondary liquidity atoms. This raw secondary
custody never counts as an additional insurance entitlement. Sources and payout
ATAs are distinct, so replenishment cannot recycle the first payout.

All twelve transaction envelopes per history are signed and serialized before
the first debit, including explicitly predicted successor epochs. Unique compute
limits avoid transaction-cache rejection; delivery never rebinds or resigns.

1. A signed Live debit exhausts all 37 target atoms on the selected rail while
   preserving the sibling's 61 atoms, complete ledger and control sequences.
2. A public 83-atom top-up into the opposite domain completes before an old
   debit suffix rejects with `EngineStale`. Exact rollback restores native
   source/vault lamports, ledger, domain stock and top-up watermark. The same
   pre-signed refill then commits. Top-ups use the supported primary rail.
3. Public resolution preserves both insurance epochs. Old signed and unsigned
   envelopes reject on the opposite withdrawal rail despite sufficient target
   allowance and physical custody for the entire retained 37-atom amount.
4. A 19-atom target payout and 11-atom sibling payout complete before a stale
   target suffix restores both owners' epochs, ledgers, custody and stock. The
   retained valid prefix then commits. Its target is signed in two histories
   and unsigned in two; the independently owned sibling is always unsigned.
   The opposite signing variant rejects stale with 64 target atoms still left.
5. Keeper-only successor envelopes pay the remaining target 64 and sibling 50
   atoms. A fresh-epoch one-atom overclaim rejects with `EngineLockActive` while
   the selected physical rail remains funded. Both entitlement ledgers end at
   zero, with exact total withdrawals of 120 and 61 atoms.

Every checkpoint compares the complete decoded market/configuration against
input-derived expectations, all control lanes, profiles, ledger Account images,
token Account images, native lamports/rent, mint Accounts and unchanged wallet
Accounts. Both physical rail censuses close independently; total leftover raw
custody is exactly 200 atoms. Insurance budget/spend, booked vault, capital,
claims, source encumbrance and other market state remain independently checked.
The reserved cap, deposits-only, cooldown and last-withdraw-slot fields stay
zero; this is preservation evidence, not evidence of an active cooldown policy.
Stock/encumbrance censuses and engine shape validation run at every checkpoint.

Every rejection compares all tracked and transaction-compiled Accounts, including
absence, data, metadata and lamports. Only the payer loses the exact required
signature fee. Exact instruction indexes/errors and successful wrapper/SPL log
counts prove the prefixes executed. Permissionless terminal continuations have
one required signature; target-signed variants have two, with neither the
sibling owner nor administrator signing those continuations.

## Results and limits

New exact selector: **1 passed, 0 failed, 1,466 filtered**, 1.70 seconds.
**4 histories, 48 transactions, 24 exact rollbacks, 12 restored transfers,
20 payouts**, peak **79,739 CU**, maximum packet **765 bytes**.
Per-history CU peaks in `(first rail, signed partial)` order:
`(native,false) 70,751`, `(native,true) 79,739`,
`(secondary,false) 61,822`, `(secondary,true) 75,310`.
The fixed transaction ceiling is 200,000 CU. Address-dependent CU can vary.

Two exact controls pass, 1,465 filtered, 4.78 seconds:

| Control | Peak CU |
| --- | ---: |
| Live insurance debit consumes reserve authority epoch | 33,269 |
| Recent Row 428 signed/unsigned Resolved retries | 78,101 |

The initial compile needed a local `BTreeMap` import. The first execution caught
the generic creation helper's excess rent becoming extra wrapped SOL; exact-rent
public source creation corrected the fixture without weakening custody assertions.
No production mismatch was observed. The existing Solana-client future Rust
compatibility warning remains. No broad suite or metadata selectors were run.

Row 428 classifications and TSVs are unchanged. This is a bounded conformance
increment, not a whole-invariant closure or separate withdrawal stock sequence:
withdrawals still consume the shared per-asset authority epoch. Unexecuted consent
across funding without an intervening debit, arbitrary/repeated histories,
liabilities, terminal recredit, role changes, native redemption, missing/frozen
custody, administrator-free resolution, blockhash expiry and durable nonces are
outside this scenario. Final progress is complete insurance withdrawal; raw
secondary liquidity disposition and slab/ledger retirement are not exercised.

## Exact verification

Private copies of dependency caches were used. Host and SBF code were rebuilt
from this checkout; no matcher is needed. Engine pin:
`4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
SBF SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
Logs: `/dev/shm/astra-row428-20260917-{sbf,test,controls}.log`.

```bash
export CARGO_TARGET_DIR=/dev/shm/astra-row428-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-row428-20260917-sbf-target/deploy/percolator_prog.so
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
env CARGO_TARGET_DIR=/dev/shm/astra-row428-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/astra-row428-20260917-sbf-target/deploy -- --locked
cargo test --locked --offline --test v16_cu inv_064_insurance_withdrawal_policy_equivalence::native_replenishment_epoch::v16_native_replenishment_preserves_consumed_consent_across_resolved_quote_routes -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_014_delayed_policy_and_policy_epoch_safety::reserve_debit_epoch::v16_live_insurance_debit_consumes_reserve_authority_epoch \
  inv_014_delayed_policy_and_policy_epoch_safety::reserve_debit_epoch::resolved_debit_retry::v16_resolved_retained_debit_retries_restore_epochs_across_signed_and_unsigned_delivery
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_064_insurance_withdrawal_policy_equivalence.rs tests/invariants/cu/inv_064_native_replenishment_epoch.rs
git diff --check
git diff --cached --check
git diff --exit-code 4a3e7c8d57def60d8468cb2c0d1e7863403029d1 -- src Cargo.toml Cargo.lock tests/fixtures tests/support tests/v16_cu.rs ':(glob)tests/invariants/*.tsv'
git diff --exit-code 4a3e7c8d57def60d8468cb2c0d1e7863403029d1 -- . ':!tests/invariants/README.md' ':!tests/invariants/row428_native_replenishment_20260917.md' ':!tests/invariants/cu/inv_064_insurance_withdrawal_policy_equivalence.rs' ':!tests/invariants/cu/inv_064_native_replenishment_epoch.rs'
git show --check HEAD
```

Scoped formatting, whitespace checks and both protected-path guards pass. The
commit contains only the child, its three-line mount, this note and README summary.
