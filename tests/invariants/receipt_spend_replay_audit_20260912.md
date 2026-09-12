# Spent receipt conformance audit (2026-09-12)

## Scope and provenance

One new test lives in [`cu/inv_067_receipt_spend_replay.rs`](cu/inv_067_receipt_spend_replay.rs),
mounted by `cu/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs` as
`inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_spend_replay`.
Its selector is
`v16_program_spent_payouts_do_not_replenish_receipts_across_order_and_atomic_retry`.

The fresh worktree is `/tmp/percolator-receipt-conformance.MovHNV`, on branch
`codex/receipt-conformance-20260912-MovHNV`, based on
`a157bc1dc9cf2adaf176acb140f40326d058346d`, the observed HEAD of
`/tmp/percolator-astra-watch.Cb2E7d` branch
`codex/astra-open-holdout-ledger-20260912`. Only that base's code, tests and docs
informed this increment. No open PR branches, diffs or tests were inspected or copied.
The primary checkout and integration worktree were not edited. Build artifacts use
a new private target directory; no other contributor's build cache was copied.

## New relation

Existing base coverage checks claimant orders, transaction partitions, receipt
replays, source realization, destination repair and final slab disposition. This
test composes those public endpoints with **owner spending after receipt payment**:
an empty payout destination must not replenish its receipt, and an enriched
zero-claim peer's SPL account must not acquire a protocol entitlement.

The existing `late_expiry::World::new()` fixture creates all economic state using
System, SPL, ATA and wrapper instructions: five funded portfolios, two assets,
real trades and authenticated price movement, flat claims, resolution, debtor
settlement and two partially paid receipts. Harness controls are program loading,
signer SOL, authenticated Clock and blockhash advancement. There are no economic
account writes or installed simulated poststates.

Four fresh worlds cross claimant orders `[0, 4]` / `[4, 0]` with direct execution /
rejected suffix followed by retry. The initial receipt creation order stays fixed;
the spending, top-up and subsequent claimant cleanup orders vary.

1. Owners 0 and 4 spend their entire initial capital-plus-claim payments into
   settled debtor 1's ATA. Replaying both retained top-ups against the now-empty
   original destinations preserves every tracked Account.
2. Public expiry normalization raises residual from 501 to 851. The receipts have
   positive due, 82 and 151 atoms, while 1,000 face atoms remain unreceipted and
   2,000 face atoms are exact receipts. The payout denominator remains 3,000.
3. Each owner submits `[top-up, spend exact due, same top-up]`. Both wrapper calls
   succeed, with exactly two SPL successes: one protocol payout and one owner
   transfer. The second wrapper call cannot pay again after the destination is
   emptied within the same transaction.
4. In the rollback worlds an invalid System suffix rejects after all three
   prefix instructions execute. Exact economic Accounts, including owner lamports,
   receipts and both destinations, are restored. The separate transaction payer's
   full Account differs only by the calculated two-signature network fee. The
   unchanged three-instruction bundle then succeeds on a fresh blockhash.
5. Fresh-blockhash standalone retries are inert. The remaining positive source
   claim settles, every portfolio becomes terminal within eight calls, and owner
   closes transfer portfolio rent exactly into the market. `CloseSlab` must make
   progress within eight calls, burn the two rounding atoms, close the vault and
   leave the typed market tombstone with exact rent and authority refund.

## Independent oracle

The oracle uses input integers, not an engine entitlement routine:

```text
face       = [700, 0, 1000, 0, 1300]
capital    = [1000, 0, 1000, 0, 1000]
claim(i,r) = floor(face[i] * r / 3000)
final paid = capital + claim(i,851) = [1198, 0, 1283, 0, 1368]
spent      = [1198, 0, 0, 0, 1368]
wallets    = [0, 2566, 1283, 0, 0]
3852 minted = 3849 owner entitlements + 2 rounding burn + 1 provider token
```

At each measured economic step, wallets equal attributed payout minus owner
spending plus incoming transfers. Payouts are capped by the final oracle; booked
and SPL custody reconcile with fixed supply before the terminal burn. Initial and
top-up receipt equality checks preserve all receipt fields except the exact
increase of `paid_effective`. Portfolio identity and position episode remain
fixed; unrelated Accounts remain exact. The enriched debtor stays terminal with
no receipt. A terminal reservation/encumbrance census is required before slab
closure. All four final economic outcomes must compare equal.

## Evidence boundaries

| Invariant | Evidence added |
| --- | --- |
| INV-066 | Both measured claimant orders have identical input-priced entitlements and final custody. |
| INV-067 | Owner spending, in-transaction duplicate payout and later replay cannot create a second settlement; every claimant exits. |
| INV-068 | Nonfinal paid receipts retain their identity and exact cumulative payment despite empty destination balances. |
| INV-070 | The completed nonzero-face history classifies and burns exactly two residue atoms, then reaches the typed tombstone and exact rent refund. |
| INV-080 | A returned suffix error restores the successful payout/spend/replay prefix under SVM rollback semantics. This does not prove all engine-error mappings. |
| INV-081 | Intermediate cashflows, account locality, custody and terminal stock are checked along this complete public history. This is not the full global invariant suite. |

**The unresolved zero-bound cleanup/static edge is outside this test.** The
measured payout bundles have positive exact receipts and a positive unreceipted
bound. Later exhaustion occurs only through settlement of the known positive
claim population; that continuation does not certify arbitrary zero-bound,
zero-denominator, stale or synthetic cleanup states. No invariant verdict or
holdout status is promoted, including the current INV-067/070 refutations.

Other residual gaps include arbitrary claim amounts and histories, initial receipt
creation permutations, other public payout handlers, quote rails, destination
authority changes, transfer fees, shared owners, insolvency variants, reserve
succession and owner absence during signed portfolio/slab retirement. The test
does not show owner-independent whole-market retirement.

## Validation

The new exact selector passed on its first run: **1/1 test, four worlds, four
exact rollbacks, twenty portfolio closes and four terminal tombstones**. Peak
measured CU is **270,370**, within the 500,000 bundle ceiling; terminal calls also
meet their individual 300,000 ceiling. All **five nearby controls** passed, as did
the invariant charter/index (**1/1**), `cargo fmt --all -- --check` and
`git diff --check`. No unexpected failing public trace was observed. Existing
unused-support and Solana future-compatibility warnings remain. Production code,
support fixtures, dependency pins and invariant/holdout ledgers are unchanged.

SBF was rebuilt from this isolated worktree, locked/offline, with default
features and platform-tools v1.52. Engine pin:
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`. SBF SHA-256:
`5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.

Commands run from the isolated worktree:

```sh
export CARGO_TARGET_DIR=/dev/shm/receipt-conformance-MovHNV-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export TMPDIR=/dev/shm CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_spend_replay::v16_program_spent_payouts_do_not_replenish_receipts_across_order_and_atomic_retry -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_066_resolved_payout_fairness_and_order_independence::v16_program_late_receipt_materialization_preserves_snapshot_entitlements \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_partition_confluence::v16_program_rejected_receipt_partition_suffixes_preserve_terminal_entitlements \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::v16_program_resolved_crank_topup_batch_order_retries_pay_exactly_once \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_terminal_disposition::v16_program_receipt_terminal_suffix_partitions_rounding_burn_surplus_and_rent \
  inv_068_receipt_uniqueness_and_monotonic_topups::v16_program_same_owner_receipts_keep_independent_topups_and_terminal_replays
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
git diff --cached --check
```
