# Lane 17: Pending Receipts and Funded Insurance Succession

## Scope and isolation

- Branch: `codex/lane17-resolved-receipt-late-expiry-20260915`.
- Base: `origin/codex/astra-invariant-cycle-20260915`, fetched at
  `89cd5088a9917d53cbbf8d4247be78adde89fd95`.
- Independent clone: `/tmp/percolator-lane17-resolved-receipt-20260915`.
- Engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Charter: `scripts/loop.md` and `tests/invariants/README.md`.
- No writes to the coordinator checkout or any other agent checkout. Private
  SBF, matcher and host build directories are under `/dev/shm`; deployed files
  and logs are under this clone's `/tmp` location. No build caches were copied
  from another agent.

## Selected product and overlap review

The selected product is **funded reserve beneficiary succession while resolved
receipts, late backing expiry and the final maintenance-fee credit are pending**.
The existing `cu/inv_067_receipt_late_fee_reclassification.rs` owner is extended;
there is no new fixture, harness mount or copy of the payout oracle.

| Existing owner | Boundary not covered there |
| --- | --- |
| Lane 8 / `inv_067_receipt_rail_liquidity.rs` | Its classic/native liquidity worlds have zero maintenance fees, no funded insurance handoff, and no reserve-beneficiary exit. Native synchronization cannot witness a beneficiary change around a later fee credit. |
| Original late-fee owner | The administrator receives all insurance. It never transfers that funded role while user receipts remain pending, retains an old beneficiary request, or checks successor-payment rollback. Its original 24 worlds remain as a control. |
| Lane 10 / `inv_073_successor_custody_retry.rs` | Paid-prefix succession uses insurance-only markets with no user portfolios, pending receipts or late backing. It cannot check whether later fee credit or receipt cleanup changes the successor's allowance or the users' entitlement. |
| INV-067 provider/insurance retry owner | Live backing and separate reserve recipients are tested after fully funded user payout, without this partial-receipt/late-fee/beneficiary-handoff product. |
| Receipt partition, expiry interleaving and episode owners | They vary receipt creation, normalization and transaction boundaries but do not transfer a funded reserve beneficiary while an additional fee remains collectible. |

No private finding or withheld patch is used. This is a conformance increment,
not a claim to reproduce or close the withheld row-417 finding.

## Matrix and public evidence

The new selector executes 96 histories:

- Two directions between the original junior and backed debtor owners as former
  and successor insurance beneficiaries; their existing SPL destinations are
  distinct from the three positive claimants and the administrator.
- Handoff before or after the final seven-atom fee credit.
- Explicit `SyncMaintenanceFee` or fee collection inside `CloseResolved`.
- All six orders of the three positive claimants.
- Exact expiry at slot 13 or late expiry at slot 17.

Each history creates and funds the market using the existing System/SPL/ATA and
public wrapper fixture. It initially transfers the already-funded 287-atom
insurance role from the administrator to the former beneficiary. After backing
normalization, that beneficiary transfers it to the successor either before or
after the final fee credit. Both transfers require actual signatures from the
incumbent and incoming holder. Complete economic state and all framed accounts
other than the market remain unchanged by each handoff; only the wrapper's
authority profile/epoch changes.

Both positive receipts remain present across the second handoff. Their face,
prior bound, released face, identity, position epoch and owner are checked by the
existing oracle. The original claim instruction bytes continue across handoff,
expiry and fee collection. The successor insurance request is retained at the
second handoff and must still be byte-identical after the remaining user exits.

Every new world also checks:

1. Expiry, fee collection, last bound replacement and three actual SPL payouts
   execute before the former beneficiary's otherwise valid insurance withdrawal
   rejects with `EngineLockActive`: materialized portfolios still protect user
   completion. The entire transaction, including receipts and the fee cursor,
   rolls back. The public user continuations then complete.
2. Old-beneficiary requests, both retained and with a refreshed epoch, reject
   after all portfolios close. An epoch refresh cannot change the beneficiary.
3. The successor's complete insurance payment executes before an old-beneficiary
   suffix rejects. The payout, control epoch, custody and economic lamports roll
   back exactly. The same retained successor instruction then succeeds without
   either beneficiary or the administrator signing.
4. Replaying that paid request cannot withdraw again. Custody validation rejects
   its 294-atom demand against the remaining two-atom vault before checking the
   consumed authority epoch.

The input-derived oracle requires residual stock `480 -> 809`, exact face 3,000,
user payouts `[1104, 0, 1185, 0, 1266]`, insurance 294, provider wallet 1 and
rounding residue 2. The successor receives all 294 insurance atoms; the former
beneficiary receives zero. All five portfolios are deleted with exact rent
transfer to the market. Fixed mint supply 3,852 and custody reconcile at each
checked prefix. There is no program-owned byte mutation, private engine call,
economic account injection or installed simulation result. The shared rollback
frame excludes the separate network-fee payer and runtime accounts; the large
rollback additionally checks the payer's exact signature fee.

The new matrix contains 192 funded handoffs, 480 exact rollbacks, 384 rolled-back
SPL payouts, 480 portfolio closes and 96 successful successor insurance payments.
The old control retains its original 700,000-CU bound. The new rollback bundle
adds a sixth wrapper instruction and uses an 800,000-CU bound.

## Artifacts and commands

Wrapper SBF SHA-256:
`e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.

Authenticated matcher SHA-256:
`50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

Both artifacts were built from this branch with default features and
platform-tools v1.52. All commands run in the isolated clone:

```sh
env CARGO_TARGET_DIR=/dev/shm/lane17-20260915-sbf CARGO_BUILD_JOBS=4 \
  RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --sbf-out-dir target/deploy -- --locked

env CARGO_TARGET_DIR=/dev/shm/lane17-20260915-auth-sbf CARGO_BUILD_JOBS=4 \
  RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --manifest-path tests/fixtures/auth_matcher/Cargo.toml \
  --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked

sha256sum target/deploy/percolator_prog.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so

export CARGO_TARGET_DIR=/dev/shm/lane17-20260915-host
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/tmp/percolator-lane17-resolved-receipt-20260915/target/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_late_fee_reclassification::v16_program_pending_receipts_preserve_late_fee_insurance_across_beneficiary_succession -- --exact --nocapture

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=2 \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_late_fee_reclassification::v16_program_late_fee_reclassification_preserves_receipt_faces_and_claimant_order \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_rail_liquidity::v16_program_late_expiry_claimant_orders_share_secondary_liquidity_without_losing_receipts \
  inv_073_no_permanent_user_lock::successor_custody_retry::v16_program_paid_insurance_prefix_survives_beneficiary_succession_on_both_rails

cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence -- --nocapture

rustfmt --edition 2021 --check tests/invariants/cu/inv_067_receipt_late_fee_reclassification.rs
git diff --check
git diff --exit-code -- src Cargo.toml Cargo.lock tests/fixtures \
  tests/invariants/invariant_status.tsv tests/invariants/coverage_reopenings.tsv
git diff --cached --check
git show --format= --check HEAD
```

## Results

| Check | Result |
| --- | --- |
| New exact selector | PASS: 1 test, 96 histories, 0 failures, 127.83 seconds; peak 766,149 CU under 800,000 |
| Three exact controls | PASS: 3 tests, 0 failures, 46.23 seconds; original fee 24 worlds, Lane 8 40 worlds, Lane 10 6 worlds |
| Complete INV-079 module | PASS: 17 tests, 0 failures, 3.41 seconds, including public trace/classifier and fixed-blocker progress guards |
| Scoped rustfmt and Git whitespace checks | PASS |
| Production, dependencies, fixture sources and machine metadata diff | Empty |

The control peaks are 646,849 CU for the original fee owner (under its unchanged
700,000 bound), 285,925 for the native grouped-sync receipt control, and 108,512
for the paid-prefix succession control. The four 24-world new submatrices peak
at 766,149 / 748,149 / 749,649 / 746,649 CU respectively. All new maxima are from
the rollback bundle. CU varies with generated public keys; these are measured
values from the final runs, not exact-cost assertions.

Logs: `/tmp/lane17-20260915-new-final.log`,
`/tmp/lane17-20260915-controls.log`,
`/tmp/lane17-20260915-inv079-final.log`,
`/tmp/lane17-20260915-sbf.log` and `/tmp/lane17-20260915-auth-sbf.log`.
No unrelated or unfiltered suite and no Kani proof campaign was run.

No current implementation LoF/DoS/CU violation was found in this matrix.

Development checks exposed two test-expectation issues: the paid request first
fails custody validation, not the later epoch guard, and a six-wrapper rollback
bundle costs more than the existing five-wrapper bound (743,649 versus 700,000
CU). The new bound is 800,000; the original bound is unchanged. These are not
implementation LoF/DoS/CU findings: public positive-value continuations complete,
and the larger bundle reaches its intended instruction error below the runtime
ceiling. No production fix or red/green implementation claim is made. An initial
baseline command used an unqualified name with `--exact` and selected zero tests;
it is excluded from validation evidence.

## Disposition and limits

**Row 417 remains OPEN/missing.** This adds finite public-route evidence for
INV-024/063/067/068/073 without promoting any machine status. Rows 416/421 also
receive no closure claim.

The product is one classic SPL rail, five portfolios, fixed fees and marks,
one late backing release, two funded handoffs and full insurance withdrawal.
It does not cover arbitrary histories, mixed funding debts, repeated expiries,
ADL, maximum shapes, provider-principal/earnings beneficiary permutations,
partial insurance spend/recredit, native unwrap authority, or full slab deletion.
Two rounding atoms remain in the vault after this bounded owner-exit witness.
