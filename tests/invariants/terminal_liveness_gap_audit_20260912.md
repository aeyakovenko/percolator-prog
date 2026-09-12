# Terminal and liveness coverage audit, 2026-09-12

Base: `61162213a70939de8e1a9ce0de8eda6d851a87c0` on
`origin/codex/astra-open-holdout-ledger-20260912`. Branch:
`codex/astra-terminal-liveness-gap-20260912-c7a91`. Worktree:
`/tmp/percolator-astra-terminal-c7a91`. The original checkout was dirty and its
files were left untouched. Only the supplied holdout labels and local base
tests/documentation informed this audit; no holdout PR code or tests were read.

## Distinct coverage

The new selector is
`inv_082_state_indexed_liveness_theorem::terminal_destination_recovery::shared_destination_recovery::v16_program_shared_destination_recovery_preserves_principal_and_single_rent_charge`
in [the new CU module](cu/inv_082_shared_destination_recovery.rs). It reuses
the existing public SPL fixture and the
[destination recovery transaction oracle](cu/inv_082_terminal_destination_recovery.rs).

Before editing, `rg` audited terminal, receipt, quote, pending-obligation and
destination-recovery owners, then searched for shared/missing destinations,
idempotent creation and pre-funding. Closest existing coverage:

| Existing owner | Difference from this increment |
| --- | --- |
| `v16_program_missing_destination_has_keeper_only_terminal_recovery` | Distinct owners/destinations and ordinary ATA creation; no pre-funded shared destination or repeated idempotent creation. |
| `v16_program_same_owner_receipts_keep_independent_topups_and_terminal_replays` | Shared custody already exists; focuses on receipt-local faces and floors, not custody reconstruction or keeper rent. |
| `v16_program_quote_variants_have_bounded_empty_terminal_close_at_capacity` | Empty terminal vault retirement across quote variants, not shared user destination recovery. |

Sixteen fresh public worlds cross four starting destination lamport balances
(`0`, `rent - 1`, `rent`, `rent + 1`), both payout orders, and split/combined
transactions. Two portfolios have one owner and share an ATA; their distinct
101- and 37-atom deposits are the independent entitlement oracle. Owner-authorized
SPL closure removes the empty ATA after deposit. Public System transfers create
the pre-funded destination cases. The owner transfers away its remaining SOL,
and the test drops its signing key before the measured recovery history.

At the configured resolution/exit boundaries, a separate keeper invokes public
stale resolution, ATA `CreateIdempotent`, `CloseResolved`, and
`PermissionlessCrank`. Every measured transaction has exactly one signature,
belonging to the keeper. The first repair charges only the missing rent;
idempotent creation for the second co-owned payout charges none. Pre-funded
lamports above rent survive without becoming quote-token principal.

The oracle reconciles each portfolio's remaining principal to its own deposit,
the shared destination to the sum paid, booked and SPL vault amounts to unpaid
principal, fixed mint supply, and terminal economic state. It checks peer frames
between split payouts. Rejections compare complete tracked and compiled Accounts,
including metadata, lamports and account presence; payer differences equal only
the network fee. Coverage includes refusal before the owner window expires,
rollback of successful ATA creation and SPL payout when an unsigned portfolio
deletion follows, and exact `EngineNonProgress` rollback of completed-bundle retries.

All market, portfolio, mint, ATA and economic transitions use the existing real
wrapper/System/SPL/ATA SBF paths. Harness controls are program installation,
initial signer SOL, Clock and fresh blockhashes. No program-owned account bytes
are installed or mutated out of band; no production code or support helper changes
are needed. This is sampled INV-067/080/081/082 evidence. Economic payout is proved;
owner-signed portfolio deletion and administrative slab retirement are not claimed.

## Holdout disposition

These are coverage limits, not newly demonstrated production failures. No holdout
status or invariant verdict is promoted by this increment.

| Holdout | Independent base coverage inspected | Still outside this increment |
| --- | --- | --- |
| 417: resolved receipts / late expiry | [Receipt/expiry interleavings](cu/inv_067_receipt_expiry_interleavings.rs), plus shared-owner and terminal-disposition siblings | Receipts and late backing reclassification are absent from the new capital-only history. Row remains OPEN. |
| 418: native wSOL terminal insurance | [Terminal quote variants](cu/inv_077_terminal_quote_variants.rs) | Native insurance retirement is not inferred from SPL destination recovery or empty/native quote controls. Row remains OPEN. |
| 419: pending loss through resolution | [Resolved pending cohorts](cu/inv_039_pending_loss_resolved_histories.rs) | Existing cohort evidence is scoped to solvent integral positions without fees/funding; this test adds no pending obligation coverage. Row remains OPEN. |
| 424: terminal scan / backing expiry | [Persisted cursor/time composition](cu/inv_071_terminal_cursor_time.rs) and [prefix reuse](cu/inv_070_terminal_prefix_reuse.rs) | No terminal scan or expiry reclassification occurs here. General cursor invalidation coverage remains separate. Row remains OPEN. |
| 433: unsigned terminal reserve payouts | [User terminal exits](cu/inv_073_no_permanent_user_lock.rs) and [missing destination recovery](cu/inv_082_terminal_destination_recovery.rs) | New unsigned recovery covers shared user principal only. Backing principal, provider earnings and insurance beneficiary payouts still require their own conformance evidence. Row remains OPEN. |

## Validation

Fresh locked/offline default-feature wrapper SBF build succeeded in the isolated
worktree using platform-tools v1.52. A private copy of the existing build cache
seeded the target directory; the wrapper rebuilt from this worktree.
Engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
Wrapper SHA-256: `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.

The new exact selector passes **1/1**, covering 16 worlds, 32 portfolio payouts,
48 exact rollbacks (including 16 completed-bundle retries), and 16 repairs with a
single rent charge. Peak measured transaction cost: **221,509 CU**, below the
shared oracle's 300,000-CU ceiling. The adjacent existing destination-recovery
selector passes **1/1**, peak 122,527 CU. Development corrected the new test's
completed-payout retry expectation from success to the existing public
`EngineNonProgress` contract; no production failure was found or suppressed.
The invariant charter/index selector passes **1/1**; `cargo fmt --all -- --check`
and `git diff --check` pass. Existing unused-support warnings and the
`solana-client v1.18.26` future-compatibility warning remain.

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-terminal-liveness-c7a91-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export TMPDIR=/dev/shm CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu inv_082_state_indexed_liveness_theorem::terminal_destination_recovery::shared_destination_recovery::v16_program_shared_destination_recovery_preserves_principal_and_single_rent_charge -- --exact --nocapture
cargo test --locked --offline --test v16_cu inv_082_state_indexed_liveness_theorem::terminal_destination_recovery::v16_program_missing_destination_has_keeper_only_terminal_recovery -- --exact --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
```
