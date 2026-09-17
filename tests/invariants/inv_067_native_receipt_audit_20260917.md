# Native Receipt Redemption Conformance

Base: `b71a3e81a960cfd7c7166851f48dcb00a1c9d773` (`origin/main` when the
fresh worktree was created).
Branch: `astra-ultra/owner-terminal-conformance-20260917`.
Worktree: `/dev/shm/percolator-astra-owner-terminal-20260917`.
Engine: `94979ede7db934545e53a8f210dd063a9ea3ea63`.

Production source, Cargo.toml, Cargo.lock and invariant statuses are unchanged.
The original checkout was not edited; no push was performed. The review used
repo-owned invariant docs and source on the base, with no open PR diff as evidence.

## Coverage And Non-Duplication

The new INV-067 public selector covers a native SPL semantic boundary: closing a
funded native token account returns its token value, unsynced SOL and rent directly
to its owner even while that owner's portfolio retains an underfunded receipt.
The receipt must remember earlier payments across the destination's destruction
and recreation. Native rounding also requires a beneficiary transfer rather than
the ordinary-SPL burn path.

| Existing Evidence | Distinction |
| --- | --- |
| INV-067 `receipt_destination_recreation` | Ordinary SPL cannot close funded accounts; that history first spends the tokens. No native redemption, lamport oracle or native rounding beneficiary. |
| INV-067 `receipt_terminal_disposition` | Carries the same independently derived receipt floors through ordinary-SPL burn and raw-token sweep. It has no native mint, wallet redemption or insurance receipt for rounding. |
| INV-070 `native_pnl_sync_retry` | Fully funded PnL and sync/close rollback, without underfunded retained receipts. |
| INV-070 `native_booked_residue_cleanup` | Booked reserve residue from fee/loss/recredit, not rounding derived from completed receipt floors. |
| INV-068 shared-owner receipts; INV-066 late materialization | Receipt identity/order without funded native destination closure. |
| INV-024 live entitlement; INV-027 seniority; INV-073 reserves | Retain ownership/seniority/exit evidence, without this native receipt custody transition. |

Duplicate claimant-order-only and ordinary destination-recreation candidates were
not implemented. The accepted test reuses the established public trade prefix and
adds only native quote construction and a terminal suffix; there is no new active
matched-book coverage.

## Independent Oracle

The public seed deposits 3,750 atoms and funds 101 backing atoms, leaving one of the
provider's 102 wrapped atoms outside the vault. Trading quantities and mark movement
give faces `[700, 0, 1000, 0, 1300]` and senior capital `[1000, 0, 1000, 0, 1000]`.
The first two receipts pay `floor(face * 501 / 3000)` beside their senior capital.
The expiry releases 350 atoms; final junior entitlement is
`floor(face * 851 / 3000)`. Thus the three owners receive 1,198 / 1,283 / 1,368,
with precisely two atoms of rounding. No observed engine payout/rate supplies
expected entitlements.

Each world redeems either the 1,116- or 1,217-atom initial payout from its funded
native ATA, plus seven unsynced lamports and the exact native rent. The portfolio,
receipt and peer accounts remain unchanged. A retained request fails while custody
is absent and succeeds after public ATA recreation, paying only 82 or 151 new atoms.
The peer uses resolved PermissionlessCrank for its top-up. Both claims replay
without value movement. The third claimant receives its entire 1,283 atoms.

The independent book tracks cumulative payouts separately from redeemed SOL and
current native balances. Complete native Account images bind token state, lamports,
rent and authorities. Snapshot/rate/face/incarnation checks compose with stock and
encumbrance censuses. The stock census receives SPL custody less the known synced
donation, since SyncNative creates transferable surplus but no protocol deposit;
the full native vault image independently checks both booked and raw amounts.

Four worlds cross either redeemed claimant/top-up order with a 19-lamport vault
donation left unsynced or synced before expiry. Neither form changes receipt floors.
Bounded unsigned economic cleanup precedes five owner-signed deletions, exact rent
transfers, and bounded administrator-signed CloseSlab. A distinct insurance ATA gets
exactly two native atoms. The administrator receives the raw donation as tokens or
refunded SOL, plus exact vault/slab rent. Native mint bytes never change.

## Exact Verification

The supplied artifact's build worktree (`cb236b52`) differs from the base in the
`syn` dev-dependency feature list. To comply with the artifact-reuse condition,
private host/SBF cache copies were made and the default-feature SBF was rebuilt
against this worktree. Only the wrapper rebuilt, in 5.72 seconds. The result is
byte-identical to the supplied artifact, SHA-256:
`87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.

Commands from the isolated worktree:

```bash
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
export CARGO_TARGET_DIR=/dev/shm/percolator-astra-owner-terminal-20260917-sbf-target
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked

export CARGO_TARGET_DIR=/dev/shm/percolator-astra-owner-terminal-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-astra-owner-terminal-20260917-sbf-target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu -- --exact --nocapture \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::native_receipt_redemption::v16_program_native_receipt_redemption_preserves_topups_and_rounding_beneficiary \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_terminal_disposition::v16_program_receipt_terminal_suffix_partitions_rounding_burn_surplus_and_rent \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_destination_recreation::v16_program_recreated_destination_preserves_receipt_identity_across_second_expiry_retry

cargo test --locked --offline --test v16_program_fuzz_regressions \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_every_public_invariant_source_and_declared_test_is_mounted \
  -- --exact --nocapture

rustfmt --edition 2021 --check --config skip_children=true tests/invariants/cu/inv_067_native_receipt_redemption.rs tests/invariants/cu/inv_067_terminal_claim_late_expiry.rs tests/invariants/cu/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs
git diff --check
git diff --exit-code b71a3e81a960cfd7c7166851f48dcb00a1c9d773 -- src Cargo.toml Cargo.lock
```

Initial selector-only development runs corrected three oracle assumptions: the
snapshot is initially uncaptured, the last claimant may be paid and cleared in one
call, and a synced raw donation is outside booked stock. These were test-oracle
corrections; no production conformance failure was observed.

The final exact LiteSVM invocation passes **3 tests, 0 failures, 0 ignored,
1,435 filtered** in 8.62 seconds. The new selector covers **4 worlds, 8 positive
top-ups, 4 exact custody rollbacks, 20 portfolio deletions and 4 slab closures**;
peak measured CU is **177,100**, below the existing 300,000 custody guard.
The adjacent ordinary-SPL destination-recreation and terminal-disposition controls
pass at 265,299 and 161,042 peak CU respectively. Scoped rustfmt, whitespace and
the production/manifest/lock comparison pass. No broad suite was run.

The exact mount/census selector passes **1 test, 0 failures, 0 ignored,
144 filtered** in 3.77 seconds, finding **506 source files and 1,911 available
tests**, including the new source and selector.

## Limits

Fixed primary-native, two-asset, zero-fee/funding, no-CPI history with one backing
release and one redeemed destination per world. This is bounded public workflow
evidence, not arbitrary-history, Recovery, insurance-consumption, maximum-shape or
whole-invariant proof. It makes no absent-administrator retirement claim and does
not promote any invariant status.
