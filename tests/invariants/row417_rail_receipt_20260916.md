# Row 417: Receipt Entitlement Across Alternating Quote Rails

## Isolation and provenance

- Worktree: `/dev/shm/row417-rail-receipt-20260916`.
- Local branch: `codex/row417-rail-receipt-20260916`.
- Base: `origin/codex/astra-invariant-cycle-20260915` at
  `b4091247021656d6877e2454ed04e4b0f8cbfefa`, from
  `/tmp/percolator-astra-invariant-cycle-20260915-run`.
- Protected comparison: `origin/main` at
  `d809e9a563d9b8bf38f32648b32a15d75f526ec8`.
- Engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Reused SBF: `/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so`;
  SHA-256 `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
  This matches the artifact recorded in the Lane 24 report; no SBF rebuild is
  claimed. A fresh private host target compiled this worktree.
- The user's `/home/anatoly/percolator-prog` checkout was not edited. No remote
  fetch, push, PR or external message is part of this worker.

## Property and net-new product

The invariant is cumulative: each receipt's payment is at most its current
immutable-face floor, and an already-paid increment cannot be paid again on a
different rail. A later backing release may legitimately raise that floor;
retained requests may collect only its unpaid difference. Clearing a terminal
receipt cannot recreate its entitlement.

The selector lives under INV-067's existing `receipt_rail_liquidity` module and
reuses its private `Rail` custody/frame helpers. A small new `World` constructor
combines the existing staggered-source public builder with its existing quote
mint setup hook. No program-owned state is injected. The inherited native setup
installs only the external SPL native-mint genesis account missing from LiteSVM;
System, SPL, ATA and wrapper instructions construct all custody and economic
state. Clock warps and signer airdrops are the existing simulator facilities.

| Existing evidence | New intersection |
| --- | --- |
| `receipt_rail_liquidity`: one expiry, shared liquidity, native sync, owner exits | Two committed source expiries with a rail switch between payment waves and one deliberately delayed peer |
| `receipt_repeated_stock`: two releases, reversed priority, primary payouts | Classic/native secondary equivalence at each observed prefix, mixed-handler cross-rail duplicates inside one transaction |
| Receipt expiry/owner-exit windows | Retained payout bytes, cumulative paid entitlement and receipt clearing with owners absent from every suffix transaction |

The finite matrix is 2 rail types x 2 expiry timings x 2 first-payment rails x
2 eager claimants x 2 peer schedules = **32 histories**. Both adjacent controls
are rerun with exact selectors. Coverage and machine statuses are not promoted.

## Public trace and oracle

1. The existing six-portfolio, three-asset public trading prefix creates receipt
   faces 700 and 1,300 and a separate source claimant with face 1,000. At slot 12,
   residual 501 pays the two receipts 116/217 plus their 1,000 principal each.
   Two Fresh source domains retain 161/189 atoms until slots 13/15. Public SPL
   minting or native wrapping funds the canonical secondary vault with 233 atoms.
2. Freeze both payout handlers for both receipts on both rails. Retry the fully
   paid claimant before each expiry (slots 12/14) and at its exact/late landing
   (13/14 and 15/16). These claims advance the clock but preserve the original
   rate, paid counters and pending source stocks.
3. Normalize one expired source and pay on the selected rail. For each paid
   receipt, duplicate claims on the alternate and original rail execute in the
   same transaction and must cause no second SPL transfer. Append an invalid
   System instruction, require all preceding wrapper calls and real transfers
   to have succeeded, and compare complete rollback frames. Retry the unchanged
   prefix under a fresh blockhash to commit it.
4. After the first release the residual is 662, with floors 154/286. In half the
   histories only the eager claimant takes this increment; the peer's original
   receipt, including its initial paid counter, remains intact. After the second
   release residual is 851 and floors are 198/368. Reverse claimant priority and
   switch the payment rail. A deferred peer receives 82 or 151 total new atoms;
   an eager claimant receives only its remaining 44 or 82 atoms.
5. Two public closes consume the remaining source attributions and pay its owner
   exactly 1,283 atoms. Abort/retry the paying close, then abort/retry clearing
   both older receipts. All six portfolios are economically terminal, receipt
   bound is zero, payouts are `[1198, 0, 1283, 0, 1368, 0]`, and engine dust is 2.
6. Advance to slot 16/17 and then 100. Abort/retry both rails' retained claims and
   replay again at the same clock. Every receipt stays absent; neither remaining
   primary liquidity nor unspent secondary liquidity restores entitlement.

Expected amounts come from the public faces and input stock releases, using
`face * residual / 3000`. Assertions bind the entire receipt, fixed snapshot and
rate denominator, source Fresh/Expired stock, portfolio identity/episode, each
owner's combined payout and exact per-rail token deltas. Native custody checks
`lamports = native rent reserve + token amount`; classic/native mint supplies
and complete custody sums are independently reconciled. A native trace's eight
economic checkpoints must equal its classic counterpart, including ledgers,
receipts, per-rail destinations and vault amounts.

Each history has six rejected suffixes: two expiry/payment batches, source
payment, receipt clearing, and two terminal replay batches. All **192 failures**
restore every tracked economic Account field. The separate fee payer's complete
Account must differ only by one default signature fee. Runtime sysvars and
program accounts are excluded from the inherited economic frame.

Model-side review of `handle_close_resolved` and
`handle_claim_resolved_payout_topup` in `src/v16_program.rs` found that both
routes update the receipt through the engine before validating and transferring
the selected quote tokens. Engine `resolved_receipt_claimable_against_ledger`
subtracts `paid_effective` from the current floor. `advance_resolved_slot_not_atomic`
only advances time; terminal clearing requires zero unreceipted bound and zero
remaining claimable amount. The tests check public transaction behavior without
directly invoking those engine transitions.

## Reproduction

```sh
cd /dev/shm/row417-rail-receipt-20260916
export CARGO_TARGET_DIR=/dev/shm/row417-rail-receipt-20260916-target
export TMPDIR=/dev/shm/row417-rail-receipt-20260916-tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_rail_liquidity::rail_history::v16_program_two_expiry_receipts_share_exact_once_entitlement_across_quote_rails -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_rail_liquidity::v16_program_late_expiry_claimant_orders_share_secondary_liquidity_without_losing_receipts -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_repeated_stock::v16_program_receipts_preserve_identity_through_two_stock_releases_and_reversed_priority -- --exact --nocapture --test-threads=1

rustfmt --edition 2021 --config skip_children=true --check \
  tests/invariants/cu/inv_067_receipt_rail_history.rs \
  tests/invariants/cu/inv_067_receipt_rail_liquidity.rs \
  tests/invariants/cu/inv_067_terminal_claim_late_expiry.rs
git diff --check
git show --format= --check HEAD
git diff --exit-code origin/main -- src Cargo.toml Cargo.lock tests/fixtures 'tests/invariants/*.tsv'
git diff --exit-code b4091247021656d6877e2454ed04e4b0f8cbfefa -- src Cargo.toml Cargo.lock tests/fixtures 'tests/invariants/*.tsv'
```

## Results and limits

All three exact selectors passed on the first run, each selecting one test.
No current-behavior property violation was found in this bounded product.

| Check | Result |
| --- | --- |
| New two-expiry alternating-rail selector | PASS, 32 histories and 192 exact suffix rollbacks, 50.21 seconds |
| New selector peak transaction CU | Classic 775,266; native 783,551; both below 900,000 |
| Existing shared-rail selector | PASS, 40 histories, 45.72 seconds; peak CU classic 270,528, native split sync 270,925, native grouped sync 285,925 |
| Existing repeated-stock selector | PASS, 12 histories, 16.30 seconds; peak CU 365,364 |
| Touched-file rustfmt | PASS |
| Working/staged whitespace and committed `git show --format= --check HEAD` | PASS |
| Protected diff against `origin/main` and the branch base | Empty: source, Cargo files, fixtures and every invariant TSV unchanged |

The private log directory is `/dev/shm/row417-rail-receipt-20260916-tmp`, with
`new-test.log`, `rail-control.log` and `stock-control.log`. The new selector's
host build took 1 minute 8 seconds using the locked offline dependency cache.
The existing Solana client future-compatibility warning remains. CU maxima cover
the measured public continuations and failed transactions, not a maximum-market
shape or an exhaustive setup-CU audit.

**Row 417 remains OPEN with benchmark evidence `missing`; INV-067 remains
`REFUTED_CURRENT`.** This is bounded coverage, not closure of arbitrary receipt
histories. The fixed faces, source amounts and AuthMark prefix have zero fees and
funding. Primary custody is classic SPL; the secondary rail is classic or wrapped
native with the existing decimal compatibility. There are no arbitrary mint
changes, new liquidity shortages, mixed funding debts, ADL, maximum shapes or
native unsynchronized donations in this increment. Portfolios remain allocated
after economic settlement; owner exits, native unwrap and slab retirement are
covered elsewhere. No full-suite, Kani or independently rebuilt SBF claim is made.

Changed files: the new `cu/inv_067_receipt_rail_history.rs`, its module mount in
`cu/inv_067_receipt_rail_liquidity.rs`, the setup hook in
`cu/inv_067_terminal_claim_late_expiry.rs`, `README.md`, and this report.
