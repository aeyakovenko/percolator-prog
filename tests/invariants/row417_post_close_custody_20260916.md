# Row 417: Funded Custody Recreation After Receipt Retirement

## Isolation and provenance

- Independent clone: `/dev/shm/astra-row417-terminal-20260916`.
- Local branch: `codex/astra-invariant-cycle-20260915`.
- Clone source: `/tmp/percolator-astra-invariant-cycle-20260915-run`, branch
  `codex/astra-invariant-cycle-20260915`, at
  `8839eb1aec50e9f90a06e123e3752ad2314ad9e9`.
- Additional protected comparison: prior `origin/main` commit
  `d809e9a563d9b8bf38f32648b32a15d75f526ec8`, already in the clone's object store.
  The clone's `origin` points to the local source, not GitHub.
- Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Reused SBF: `/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so`, SHA-256
  `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
  This is the same artifact documented by both prior row417 workers. No SBF
  rebuild is claimed. The fresh private host build took 1 minute 6 seconds.
- Build and logs use private `/dev/shm` target/TMPDIR paths below. The user's
  `/home/anatoly/percolator-prog` checkout and the clone source were not edited.
  No shared worktree registration, remote fetch, push, PR or external message.

## Guarantee and overlap audit

Replacing token-account lifetimes cannot replace terminal receipt entitlement.
Once the receipts are fully paid and cleared, portfolios deleted and market
tombstoned, newly initialized canonical vaults and payout ATAs do not authorize
old payout instructions, even when both vaults contain sufficient liquidity.
If recreation and funding precede an old request in one transaction, rejection
must undo the complete prefix, including rent, mint supply and native backing.

| Existing coverage inspected | Why this selector adds coverage |
| --- | --- |
| `row417_rail_receipt_20260916`, `receipt_rail_history` | Alternating quote rails, delayed peers and repeated expiries end with allocated portfolios and original vaults. |
| `row417_receipt_close_slab_rail_20260916`, `receipt_terminal_disposition` | Retained requests cross receipt clearing, portfolio deletion and slab closure; post-close vaults remain absent. The existing selector body is unchanged here. |
| `receipt_destination_recreation`, INV-082 `receipt_destination_recovery` | Destination recreation preserves a still-live embedded receipt; no replacement vault funded after a committed tombstone. |
| `receipt_rail_liquidity`, Lane 8 native receipt liquidity | Shortfalls, cross-rail retry and native synchronization already exist. Here both recreated vaults are sufficiently funded, so liquidity cannot explain rejection. |
| INV-007 `closed_market_address_is_permanently_tombstoned`; historical market-recreation helpers in `tests/support/fuzz_model.rs` | Same-address market initialization is already blocked. This selector leaves the tombstone in place and independently creates valid token custody before retrying the original receipt bytes. |

The only modification to an existing Rust file is the three-line child mount
under `receipt_rail_liquidity::rail_history::close_slab`. The new test reuses its
public setup, account frames, signer/packet/fee checker and tombstone error oracle.
There is exactly one new test, with four histories: classic/native secondary
custody x absent/prefunded addresses. No production or fixture changes.

## Public trace and oracle

1. The inherited six-portfolio, three-asset public builder creates two receipts
   with faces 700/1,300 and a source claimant with face 1,000. At slot 12 the
   residual is 501 and payments are 116/217 plus 1,000 principal each. Retain
   both handlers for both owners on both rails and the slab instruction before
   either expiry. Fund the secondary vault with 233 atoms through public custody.
2. At slots 14/17, public source normalization releases 161/189 atoms. Both older
   receipts take their top-ups on the secondary rail. Expected floors come from
   `face * residual / 3000`, with residuals 501/662/851. Whole-receipt identity,
   cumulative paid counters, rate/snapshot fields and source stocks are checked
   at every stage. The source claimant receives 1,283 on the primary rail.
3. Clear paid receipts, publicly delete all six terminal portfolios, and commit
   `CloseSlab`. Total user payments are `[1198, 0, 1283, 0, 1368, 0]`. Primary
   custody is 235 and secondary custody zero before closure. Logs require four
   successful SPL calls: the two-atom burn, 233-atom primary surplus transfer,
   and both vault closes. The provider holds 234, including its untouched atom.
4. After the market is already tombstoned, each claimant signs public SPL
   transfers of its full primary/secondary balances to actor 1, then closes
   both payout ATAs. Actor 1 holds 2,333 primary and 233 secondary atoms, all
   recorded as spent user payouts. Each owner's exact ATA rent is refunded;
   native transfers move backing lamports with the tokens.
5. For each rail, former claimant and retained handler, build an independent
   sponsor's prefix that recreates the original canonical vault and claimant
   ATA, then funds the vault. In prefunded histories each address first receives
   17 lamports through System. Primary funding transfers the sponsor's 2,333
   spent payout atoms; classic secondary funding mints 4,096; native secondary
   funding transfers 4,096 lamports and calls `SyncNative`.
6. Append the unchanged retained handler. All 32 such transactions must fail at
   that exact instruction, after two ATA successes and seven SPL successes.
   Complete economic Account images are unchanged, including sponsor, source,
   vaults, destinations, mints, owners, deleted portfolios and tombstone. The
   separate payer's whole Account differs only by its actual signature fees.
7. Commit the unchanged prefixes for the first claimant, then recreate the peer's
   payout ATA on each rail. There are six committed ATA recreations per history,
   24 total. Sponsor SOL decreases by exactly six ATA rents, plus 4,096 only for
   native backing; prefunding is counted within the rents. Neither receipt owner
   signs recreation or any later replay. Classic secondary minting requires the
   admin's mint signature, while native funding requires only the sponsor.
8. Both vaults can now cover either former claimant's full original payment.
   Repeat all eight retained owner/rail/handler requests and `CloseSlab` at
   slots 17 and 100. These 72 additional rejections execute zero SPL calls and
   preserve the whole economic frame. Every recreated claimant ATA stays empty,
   portfolios stay deleted, and the tombstone is byte-for-byte unchanged.

Post-close primary supply is exactly 3,850, and custody sums to that value.
Classic secondary supply/custody is 4,329; native secondary mint supply stays
zero, while custody totals 4,329 backed atoms. Every native account satisfies
`lamports = native rent reserve + token amount`. The sponsor retains its 233
secondary spent atoms; the 1,283-atom source claimant and 234-atom provider are
unchanged. Post-close funding is external donation, not new engine backing or
receipt entitlement. Recoverability of these deliberate donations is not tested.

Source review confirms both payout handlers validate the containing market
before payout computation and SPL transfers. Claim uses the typed header and
rejects `InvalidAccountKind`; CloseResolved/CloseSlab require the larger mutable
market layout and reject `InvalidAccountLen`. The test pins the public LiteSVM
behavior; it never invokes an engine transition or injects program-owned state.
Only the inherited external native-mint genesis fixture uses `set_account`.
Program loading, signer airdrops and Clock warps are simulator facilities.

## Reproduction

```sh
cd /dev/shm/astra-row417-terminal-20260916
export CARGO_TARGET_DIR=/dev/shm/astra-row417-terminal-20260916-target
export TMPDIR=/dev/shm/astra-row417-terminal-20260916-tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_rail_liquidity::rail_history::close_slab::post_close_custody::v16_program_funded_post_close_custody_cannot_revive_retained_receipts -- --exact --nocapture --test-threads=1

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_rail_liquidity::rail_history::close_slab::v16_program_partial_receipts_cannot_repay_across_owner_deletion_and_close_slab_rails \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_rail_liquidity::rail_history::v16_program_two_expiry_receipts_share_exact_once_entitlement_across_quote_rails \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_destination_recreation::v16_program_recreated_destination_preserves_receipt_identity_across_second_expiry_retry \
  inv_007_no_aba_reuse::v16_program_closed_market_address_is_permanently_tombstoned

rustfmt --edition 2021 --config skip_children=true --check \
  tests/invariants/cu/inv_067_receipt_post_close_custody.rs \
  tests/invariants/cu/inv_067_receipt_close_slab_rail.rs
git diff --check
git diff --cached --check
git show --format= --check HEAD
git diff --exit-code 8839eb1aec50e9f90a06e123e3752ad2314ad9e9 -- \
  src Cargo.toml Cargo.lock tests/fixtures \
  ':(glob)tests/invariants/*.tsv' ':(glob)tests/invariants/**/*.tsv'
git diff --exit-code d809e9a563d9b8bf38f32648b32a15d75f526ec8 -- \
  src Cargo.toml Cargo.lock tests/fixtures \
  ':(glob)tests/invariants/*.tsv' ':(glob)tests/invariants/**/*.tsv'
```

## Results and limits

The new exact selector passes on its first run: one test selected, four histories,
24 recreated ATAs and 104 exact rollbacks, with 32 after successful custody
creation/funding. Execution takes 5.65 seconds. Peak continuation CU is 59,810
classic and 58,310 native, below the inherited 700,000 bound. Peak serialized
transaction size is 854 bytes, below 1,232. The CU peaks cover the new post-close
recreation/replay continuations, not the inherited economic setup or market maxima.

All four adjacent exact selectors pass in one invocation (4 selected, 67.01
seconds). The table uses selector suffixes; full selectors are listed above.

| Selector/check | Result |
| --- | --- |
| `funded_post_close_custody_cannot_revive_retained_receipts` | PASS; 4 histories, 104 exact rollbacks; classic/native 59,810/58,310 CU |
| `partial_receipts_cannot_repay_across_owner_deletion_and_close_slab_rails` | PASS; 8 histories, 312 exact rollbacks; classic/native 247,476/238,476 CU; slab 67,379 CU |
| `two_expiry_receipts_share_exact_once_entitlement_across_quote_rails` | PASS; 32 histories, 192 exact rollbacks; classic/native 789,551/780,745 CU |
| `recreated_destination_preserves_receipt_identity_across_second_expiry_retry` | PASS; 4 worlds, 4 paying-prefix rollbacks; 256,333 CU |
| `closed_market_address_is_permanently_tombstoned` | PASS; unchanged existing CU assertions; no numeric peak printed |
| Touched-file rustfmt; working/staged whitespace; committed `git show --format= --check HEAD` | PASS |
| Protected diff against clone base and prior main commit | Empty for source, Cargo files, fixtures and all invariant TSVs, including nested TSVs |

Logs are `new-test-1.log` and `adjacent-controls.log` in the private TMPDIR.
The inherited Solana client future-compatibility warning remains.

**Row 417 remains OPEN with benchmark evidence `missing`; INV-067 remains
`REFUTED_CURRENT`.** This finite product adds evidence, not closure or a TSV
promotion. No real current public-route behavior violation was found.

Remaining limits include native primary residue routing, alternate quote
configuration, new liquidity shortages, mixed funding debt/ADL receipt products,
authority succession, maximum shapes and arbitrary histories. This setup has
zero fees/funding, fixed faces/stocks and two late expiries. It does not restore
deleted portfolio state or reinitialize the tombstoned market. Deleted program
accounts retain LiteSVM records with zero lamports and empty data; no bank
garbage-collection equivalence is asserted. Full-suite, Kani, fresh SBF and
donated-liquidity recovery claims are outside this result.

Changed files: this report, `README.md`, the new
`cu/inv_067_receipt_post_close_custody.rs`, and the child mount in
`cu/inv_067_receipt_close_slab_rail.rs`.
