# Terminal destination-authority variant audit, 2026-09-12

## Scope and isolation

One generic public LiteSVM conformance selector is mounted as
`inv_077_bounded_work_and_maximum_shape_compute::terminal_destination_variants::v16_program_terminal_sweep_preserves_destination_authorities_and_exact_disposal`
in [`cu/inv_077_terminal_destination_variants.rs`](cu/inv_077_terminal_destination_variants.rs).
The test accepts ordinary supported classic-SPL account variants and checks their
terminal disposition using input amounts and complete Account frames. It has no
expected-production-failure switch, quarantine, pin exception or production edit.

The isolated branch `codex/quote-variant-terminal-20260912` was created in
`/tmp/percolator-quote-variant.vp4jFF` from
`c809f2b1c2ea7256e1848ce9fc369b6b3128b0c9`, the current HEAD of
`codex/astra-open-holdout-ledger-20260912` at
`/tmp/percolator-astra-watch.Cb2E7d` when work began. All source inspection and edits
after worktree creation used that isolated checkout and its inherited baseline.
No open PR branches, diffs or tests were inspected or copied. No push is performed.

## Public histories and independent checks

Six fresh worlds cross three destination variants with bundled/split disposal:
delegate only, separate close authority only, and both. The SPL owner of the
non-ATA sweep destination is always the signing market authority. System and SPL
instructions create and initialize the mint and token accounts; ATA instructions
create canonical vault/funding custody. The public wrapper initializes, funds
backing, resolves, normalizes and closes the market. Wallet airdrops and Clock
advancement are the only environment fixtures. No program or token bytes are
injected into LiteSVM; packing token data only constructs expected host snapshots.

Each world mints 335 atoms and revokes mint authority, with no freeze authority.
Public funding leaves 307 booked backing atoms plus 17 external surplus atoms in
the canonical vault, and 11 pre-existing atoms in the sweep destination. The mint
has six decimals; all accounting is in raw atoms. The destination's optional
delegate has exactly 11 atoms of allowance. Its optional close authority is a
different funded signer. The nonnative vault receives 43 extra lamports and the
destination is created with 59 lamports above token-account rent.

At authenticated `expiry - 1`, a transaction transfers 13 additional lamports to
the vault before `CloseSlab` rejects with `EngineLockActive`. Every compiled
transaction Account and all tracked market, mint, custody and authority Accounts
are restored, except for the independently calculated signature fee.

At expiry, the first successful `CloseSlab` expires the backing label without
moving SPL amounts, mint supply or account lamports. The test independently checks
307 booked atoms, zero capital/insurance/portfolios, fresh-versus-expired backing
and source-credit labels, the stock census and the reservation/encumbrance census.
The market's data changes, while its complete non-data metadata stays fixed.

The next successful slab call burns exactly 307 atoms, sweeps 17 to the existing
destination, closes the canonical vault and writes the exact 16-byte market
tombstone at rent. Receiving that sweep preserves the destination's owner,
delegate, 11-atom allowance, close authority, rent and nonnative status. The
delegate, when present, then spends only its original 11 atoms; otherwise the
owner spends them. The owner spends the remaining 17 atoms. The named close
authority closes the empty destination and receives its lamports. The sink ends
with exactly 28 tokens, matching the remaining fixed mint supply.

A rejected version executes the entire retirement, both SPL transfers and both
custody closes before a final SPL transfer requests 29 atoms from the 28-atom
sink. The exact final-instruction `InsufficientFunds` error and full rollback frame
prove restoration of the mint burn, stock, delegation consumption, all token
transfers, both rent refunds and the tombstone. The original valid continuation
then succeeds either as one transaction or as four separate instructions. Split
execution checks complete expected Accounts at every intermediate boundary;
bundled execution checks the same final input-derived disposition.

Lamports are accounted separately from SPL stock. With token rent `R` and
tombstone rent `T`, slab closure pays the market authority exactly
`market_lamports - T + R + 43`. Destination closure pays its named close authority
exactly `R + 59`; in the delegate-only variant that authority is the owner. The
mint, funding account, sink rent, unused signer wallets and vault-authority Account
remain framed. Successful terminal transactions also reconcile the complete fee
payer Account against the number of required signatures. Closed custody has no
lamports or uncleared token bytes.

## Novelty and coverage limits

The inherited native surplus/sync and native roundtrip controls distinguish token
amount from native backing lamports. This witness uses a nonnative fixed-supply
mint and tests existing destination capabilities across a real burn and sweep.
The terminal alternate-custody and shared-destination controls pay user principal
into unencumbered custody; the reserve-destination control repairs custody before
signed reserve payouts. This witness preserves the existing destination's delegate
and close authority through the administrative sweep, then exercises their distinct
SPL spending and rent-disposal rights. It creates no replacement destination.
The freezable-quote control tests freeze/thaw and retirement-mint validation, not
these existing destination capabilities or the subsequent destination close.

| Invariant | Bounded evidence and limit |
| --- | --- |
| INV-018 | Classic-SPL mint, canonical vault, owner, nonnative status, fixed supply and destination capabilities remain exact across terminal token movement. |
| INV-021 | Exact slab tombstone/rent, vault refund, independently authorized destination refund and complete rollback after both custody closes. |
| INV-025 | Separate stock and encumbrance censuses before/after expiry; 335 initial tokens become 307 burned plus 28 held, with raw lamports separate. |
| INV-069/070 | Expired backing normalizes in one call, then exactly retires with separately classified surplus and no stranded custody value. |
| INV-073/078 | Administrative-lifecycle boundary only: no user claims exist. Authenticated expiry and a cooperative market authority permit finite slab progress. This is not a permissionless user-fund liveness proof. |
| INV-077 | Exactly two successful slab calls; the successful-state rank is fresh backing/open slab, expired backing/open slab, then closed slab. Measured transactions stay below 150,000 CU and within 1,232 bytes. This is the default market shape, not maximum capacity. |
| INV-080/081 | Exact errors, complete transaction rollback and complete successful terminal Account oracles compose with external SPL account disposal. |

Row 418 remains OPEN and no invariant verdict changes. Residual gaps include user
principal/receipt recovery, backing liens or earnings, insurance spend/recredit,
absent or uncooperative authorities, native booked-residue retirement, freeze/thaw,
secondary rails, account recreation, maximum-capacity scans and arbitrary histories.
Token-2022 extensions remain unsupported and are not exercised here. This is one
bounded conformance matrix, not a general stateful generator or an invariant closure.

## Validation

The locked/offline default-feature SBF build used platform-tools v1.52 and has
SHA-256 `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
The exact selector passes 1/1 in 2.22s: six worlds, twelve exact rollback bundles,
and two successful slab calls per world. Peak measured CU is 53,002 against the
150,000 limit. The seven exact nearby controls pass 7/7 in 13.87s, and the invariant
index passes 1/1. Repository formatting and working/staged/committed whitespace
checks are required below. Existing host dead-code and Solana future-compatibility
warnings remain.

The first selector run stopped at an incorrect test expectation that the fresh
source-credit backing reservation was zero. The existing expiry control confirms
that both fresh backing labels equal `307 * BOUND_SCALE` before expiry and become
zero after expiry. Correcting that oracle produced the passing trace above. No
production conformance failure was observed or converted into an expected success.

Commands from the isolated worktree:

```sh
export CARGO_TARGET_DIR=/dev/shm/percolator-quote-variant-vp4jFF-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=6 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
cargo build-sbf --tools-version v1.52 --sbf-out-dir "$CARGO_TARGET_DIR/deploy" --offline -- --locked
sha256sum "$PERCOLATOR_FUZZ_SBF"
cargo test --locked --offline --test v16_cu inv_077_bounded_work_and_maximum_shape_compute::terminal_destination_variants::v16_program_terminal_sweep_preserves_destination_authorities_and_exact_disposal -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_077_bounded_work_and_maximum_shape_compute::terminal_quote_variants::v16_program_freezable_quote_terminal_retry_preserves_retirement_and_rent \
  inv_077_bounded_work_and_maximum_shape_compute::terminal_quote_variants::v16_program_dual_quote_provider_expiry_has_bounded_terminal_disposition \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::v16_program_native_quote_terminal_surplus_sync_has_exact_token_and_lamport_disposition \
  inv_081_success_state_validity_over_complete_public_routes::v16_program_native_quote_roundtrip_preserves_lamports_rent_and_unsynced_value \
  inv_082_state_indexed_liveness_theorem::terminal_destination_recovery::terminal_custody_alternate::v16_program_absent_reserve_holders_receive_principal_through_alternate_custody \
  inv_082_state_indexed_liveness_theorem::terminal_destination_recovery::shared_destination_recovery::v16_program_shared_destination_recovery_preserves_principal_and_single_rent_charge \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_reserve_destination_recovery::v16_program_terminal_reserve_destination_repair_preserves_signer_gates_and_value
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
rustfmt --edition 2021 tests/invariants/cu/inv_077_terminal_destination_variants.rs
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
```
