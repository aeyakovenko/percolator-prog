# Row 417: Receipt Clear, Owner Deletion and Dual-Rail CloseSlab

## Isolation and provenance

- Worktree: `/dev/shm/row417-receipt-close-slab-rail-20260916`.
- Local branch: `codex/row417-receipt-close-slab-rail-20260916`.
- Base: `origin/codex/astra-invariant-cycle-20260915` at
  `4d17c967bdc09ad02f4ea0d68693e6998a9d14b9`, using
  `/tmp/percolator-astra-invariant-cycle-20260915-run`.
- Protected comparison: `origin/main` at
  `d809e9a563d9b8bf38f32648b32a15d75f526ec8`.
- Engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Reused SBF: `/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so`;
  SHA-256 `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
  No SBF rebuild is claimed. Host tests compiled in a fresh private target.
- No working files in `/home/anatoly/percolator-prog` were edited. Git worktree
  registration and local branch/commit metadata use the existing shared Git
  repository. No fetch, push, PR or external message was made.

## Net-new property

The existing alternating-rail selector already covers two expiries, partial
payments and clearing with portfolios still allocated. The older receipt
terminal-disposition selector covers a single classic rail through `CloseSlab`.
Existing late-expiry owner-exit products cover portfolio deletion or native
redemption. This increment composes these boundaries: one partially topped-up
receipt clears and its owner exits while the deferred peer still has money due;
retained requests then cross dual-rail vault closure and the typed tombstone.

The finite matrix is 2 secondary rail types x 2 eager claimant identities x
2 final payment rails = **8 histories**. It reuses `World`, `Rail`, the parent
receipt/stock checker, exact payment checker and public setup. The only old Rust
change mounts this child. There are no new setup helpers or production changes.

All economic construction and transitions use System/SPL/ATA and public wrapper
instructions. The inherited native setup installs only the external SPL native
mint genesis fixture that LiteSVM lacks. No program-owned bytes are injected,
simulation result installed or private engine transition invoked. LiteSVM
provides deployed programs, signer airdrops and Clock warps.

## Public trace and assertions

1. At slot 12 the six-portfolio, three-asset public prefix produces receipt
   faces 700 and 1,300, initially paid 116 and 217, with 1,000 principal each.
   The source claimant has face 1,000. Two Fresh sources retain 161/189 atoms
   until slots 13/15. Public funding places 233 tokens in the secondary vault.
   Freeze both receipt handlers on both rails, and the `CloseSlab` instruction.
2. At late slot 14, normalize the first source and pay only the eager receipt
   on the secondary rail: 38 or 69 additional atoms. The peer's whole original
   receipt is unchanged. At late slot 17, normalize the second source and finish
   the source claimant's public close, paying it 1,283 on the primary rail.
   Both older receipts still have unpaid entitlement; the unreceipted bound is 0.
3. Pay the eager receipt's remaining 44 or 82 on the selected final rail.
   A paying `ClaimResolvedPayoutTopup` preserves the receipt with its new paid
   counter; the next zero-due claim, on the alternate rail, clears it. Abort and
   retry both steps. A premature slab suffix must reject after real payment.
   After clear, retained claims are exact no-ops and retained `CloseResolved`
   calls reject with `EngineNonProgress` on both rails.
4. Delete the eager portfolio with its owner signature. A retained claim after
   deletion in that transaction rejects and rolls back deletion and rent; the
   unchanged deletion then commits. With the owner absent, pay the deferred
   peer's 82 or 151 and append either old eager handler on either rail. Each
   suffix rejects with `NotInitialized` and restores the peer's complete receipt,
   token balances and all tracked accounts. A premature slab suffix does the
   same. Commit the peer payment, clear it on the alternate rail, and delete the
   other five terminal portfolios, each with its own deletion rollback check.
5. Payouts are exactly `[1198, 0, 1283, 0, 1368, 0]`; engine rounding is 2.
   Secondary-paid value is 38, 69 or 233. Primary custody is `2 + secondary_paid`;
   secondary custody is `233 - secondary_paid`. For native, publicly donate seven
   unsynchronized lamports to the secondary vault, without changing token amount.
6. Abort `CloseSlab` with each retained rail/handler pair as a suffix. Logs prove
   the successful wrapper close and four/five successful SPL CPIs: rounding burn,
   primary surplus transfer, both vault closes, and secondary transfer if nonzero.
   All four failures restore the whole pre-close frame, including mint supply,
   lamports, token data, account owners, rent and the original market allocation.
   Retry the unchanged slab instruction to commit its tombstone within eight
   calls; these eight worlds each need one call.
7. Exact terminal accounting burns only the two primary rounding atoms. The
   primary provider receives `1 + secondary_paid`; the secondary source receives
   `233 - secondary_paid`. Native tokens move their backing lamports to that
   source; the seven raw lamports join the admin's exact vault/market rent refund.
   The tombstone retains exactly its minimum rent. User payout accounts are
   unchanged. Replaying all eight old claimant/rail/handler combinations and the
   slab instruction at slots 17 and 100 cannot revive entitlement or custody.

The input-driven floors are `face * residual / 3000`, for residuals 501/662/851.
The inherited oracle compares whole receipts, snapshot slot and rate denominator,
and Fresh/Expired source stock. Final assertions check each user's combined
payment, per-rail token movements, mint supply, rent and native lamports.

All failed suffixes compare complete `Account` images for the market, portfolios,
owners, admin, both mints/vaults, provider, secondary source, destinations and
vault authority. Runtime sysvars/programs are outside this economic frame. The
separate payer's entire Account must differ only by the default fee times the
actual signature count. Suffix transactions explicitly check signer privileges
and serialized packet size. Suffixes after committed owner deletion and final
slab calls contain no receipt owner signatures. Retained requests reuse instruction bytes with fresh blockhashes;
this does not claim replay of identical already-confirmed transaction signatures.

## Commands

```sh
cd /dev/shm/row417-receipt-close-slab-rail-20260916
export CARGO_TARGET_DIR=/dev/shm/row417-receipt-close-slab-rail-20260916-target
export TMPDIR=/dev/shm/row417-receipt-close-slab-rail-20260916-tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_rail_liquidity::rail_history::close_slab::v16_program_partial_receipts_cannot_repay_across_owner_deletion_and_close_slab_rails -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_rail_liquidity::rail_history::v16_program_two_expiry_receipts_share_exact_once_entitlement_across_quote_rails -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_terminal_disposition::v16_program_receipt_terminal_suffix_partitions_rounding_burn_surplus_and_rent -- --exact --nocapture --test-threads=1

rustfmt --edition 2021 --config skip_children=true --check \
  tests/invariants/cu/inv_067_receipt_close_slab_rail.rs \
  tests/invariants/cu/inv_067_receipt_rail_history.rs
git diff --check
git diff --cached --check
git show --format= --check HEAD
git diff --exit-code origin/main -- src Cargo.toml Cargo.lock tests/fixtures 'tests/invariants/*.tsv'
git diff --exit-code 4d17c967bdc09ad02f4ea0d68693e6998a9d14b9 -- src Cargo.toml Cargo.lock tests/fixtures 'tests/invariants/*.tsv'
```

## Results and limits

The new exact selector passes: **8 histories, 312 exact rollbacks**, including
56 after actual receipt payments and 32 after vault closure/tombstone writes.
Peak transaction size is 710 bytes. Test execution takes 11.71 seconds.

| Check | Result |
| --- | --- |
| New classic/native continuation CU | 236,984 / 237,073; below the asserted 700,000 bound |
| New slab plus retained-request suffix peak CU | 52,379 |
| Existing alternating-rail exact selector | PASS, 32 histories and 192 exact rollbacks; peak classic/native 781,266 / 769,460 CU; 48.79 seconds |
| Existing receipt terminal-disposition exact selector | PASS, 8 worlds; peak 161,043 CU, slab 36,714 CU; 8.55 seconds |
| Touched-file rustfmt, working/staged whitespace, committed `git show --format= --check HEAD` | PASS |
| Protected diff against `origin/main` and the branch base | Empty: source, Cargo files, fixtures and every invariant TSV unchanged |

No current-behavior property violation was found in the new matrix. Three initial
development runs stopped on test-oracle assumptions: payment and clear require
separate claim calls; LiteSVM retains deleted program ownership and reports
`NotInitialized`; and tombstones give `InvalidAccountKind` for claims but
`InvalidAccountLen` for `CloseResolved`/`CloseSlab`. The final test pins these
actual public boundaries, with the payment and rollback property unchanged.
No production patch was attempted. Logs, including those initial runs, are in
the private TMPDIR; final new evidence is `new-test-final.log`, with controls in
`rail-control.log` and `slab-control.log`. The Solana client future-compatibility
warning is inherited. The initial locked offline host build took 1m 10s.

**Row 417 remains OPEN with benchmark evidence `missing`; INV-067 remains
`REFUTED_CURRENT`.** No TSV status is changed. This fixed public product has zero
fees/funding, late deliveries at 14/17, a classic primary mint and classic/native
secondary mint. It excludes arbitrary histories, native primary residue routing,
post-close ATA recreation, native redemption, alternate quote configuration,
new liquidity shortages, maximum market shapes, mixed debt, ADL and authority
succession. CU peaks cover measured continuations and failures after the public
prefix, not setup or a maximum-shape audit. No full-suite or Kani result is claimed.

Changed files:

- `tests/invariants/cu/inv_067_receipt_close_slab_rail.rs`
- `tests/invariants/cu/inv_067_receipt_rail_history.rs` (child mount only)
- `tests/invariants/README.md`
- `tests/invariants/row417_receipt_close_slab_rail_20260916.md`
