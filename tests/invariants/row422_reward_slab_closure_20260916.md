# Row 422: Hybrid reward provenance through final slab closure

Date: 2026-09-16. Branch: `codex/astra-invariant-cycle-20260915`.
Isolated clone: `/dev/shm/astra-row422-composition-20260916`, copied with
`git clone --local --no-hardlinks --branch codex/astra-invariant-cycle-20260915`
from `/tmp/percolator-astra-invariant-cycle-20260915-run` at
`8839eb1aec50e9f90a06e123e3752ad2314ad9e9`.

**No real current behavior violation was found. Row 422 remains OPEN/missing;
INV-045 remains `REFUTED_CURRENT`.** Only a new invariant test, its module
registration and documentation are added. No production or TSV changes, no
publication, and no edits to `/home/anatoly/percolator-prog`.

## Guarantee and non-overlap

One `#[test]` in [the new child](cu/inv_045_reward_slab_closure.rs) executes four
histories: optional liquidation recipient present/omitted, crossed with keeper
payment/deletion first/last. In this product, final disposition preserves each
class of value: earned reward goes only to the keeper, retained liquidation
fees go only to the distinct insurance beneficiary, unbudgeted discovery fees
are burned, and only external surplus reaches the market admin. An omitted
reward cannot be reclaimed through a retained keeper request during reserve
withdrawal, portfolio deletion or final market destruction.

The reviewed adjacent products have different endpoints:

- `lane25_reward_destination_retry_20260916`: live classic SPL keeper payout;
  it does not delete the five portfolios or retire the market.
- `row422_cpi_reward_provenance_20260916`: CPI route switching, retained penalties
  and clipped maintenance with live residual positions; no final slab closure.
- `row422_native_terminal_reward_20260916`: native sync/unwrap and complete
  economic redemption, but allocated portfolios, no insurance payout and no
  `CloseSlab`. Its top-up rejection has no captured payout snapshot.
- Existing INV-045 exposed-keeper and dual-Hybrid histories cover recipient
  exposure and CPI changes. The maximum-shape paid-Hybrid products do not
  combine an earned/omitted reward with final fee burn and surplus sweep.
- INV-070 and row417 already cover final rent/burn/replay mechanics. This test
  supplies the missing real Hybrid discovery/liquidation provenance prefix and
  independently reconciles its fee classes through those mechanics.

## Reachable history and independent book

Economic accounts are built using public System, ATA, SPL and wrapper routes.
Only program loading, signer SOL, Clock and external Pyth reports are harness
inputs. No Percolator-owned byte writes, economic snapshot restores or fabricated
SPL balances are used. An admin-consented handoff assigns a distinct insurance
beneficiary; its key is dropped after creating its destination ATA.

1. At slot 1, configure one Hybrid source with price 1,000,000, production risk
   parameters, zero funding and reward share 3333 bps. Deposit 5,100,000 /
   100,000,000 / 10,000,000 / 10,000,000 / 1,000 atoms into five portfolios.
   Open the target's 100-lot position against its peer.
2. At slot 5, stale observation advances time, then a one-lot discovery trade
   at raw 900,000 has accepted print 990,400 and staged target 992,320. The
   effective mark stays 1,000,000. Independent bilateral ceiling arithmetic
   charges 770,036 atoms per discovery trader, or 1,540,072 in total. These
   fees remain outside insurance domain budgets.
3. At slot 6, current Pyth evidence advances the effective mark to 997,600.
   A bounded crank sequence closes 12,001,223 position units. Independently
   calculate `ceil(ceil(q * price / POS_SCALE) * 5 / 10000) = 5987`; entry,
   raw trade, accepted print and oracle target prices give different penalties.
   Eligible reward is `floor(5987 * 3333 / 10000) = 1995`, paid only when the
   recipient is present on that liquidation call. Retained fees split floor/
   ceiling between the two source domains. The paid discovery stock is excluded.
4. Refresh the exposed cohort and reject a same-slot rewarded replay. Resolve
   at slot 6, freezing the mark at 997,600 and its oracle profile. The price
   delta of 2,400 gives a 240,000 target/peer transfer and a 2,400 discovery-
   trader transfer. Input-derived terminal payouts, before reading the payout
   result, are 4,854,013 / 100,240,000 / 9,227,564 / 9,232,364 / (1,000 + reward).
   There is exactly zero settlement dust in this history.
5. Redeem and owner-delete all five portfolios. In the keeper-last worlds, the
   keeper alone blocks insurance withdrawal and `CloseSlab`; even a successful
   keeper payment in the same transaction does not permit either operation
   before deletion. Check that deletion clears exactly the portfolio account,
   decrements the materialized count and moves all its lamports to the market,
   leaving its owner's SOL unchanged.
6. With no users or OI left, unpaid insurance still blocks closure. A one-atom
   overclaim fails with a funded vault. Pay precisely the retained liquidation
   budget to the unsigned, distinct beneficiary; another one-atom withdrawal
   fails although discovery fees remain in custody. Keeper close/top-up replays
   roll back the successful reserve-payment prefix exactly.
7. Publicly mint and transfer 19 external surplus atoms into the vault, without
   crediting the engine. A keeper-owned final surplus destination rejects.
   Retained keeper close/top-up requests following `CloseSlab` roll back the
   actual burn, surplus sweep, SPL vault close and typed market tombstone.
   Commit closure, then replay both keeper handlers and closure at slots 6/100.

| Recipient | Keeper payout | Insurance beneficiary | Discovery burned | Admin SPL surplus |
| --- | ---: | ---: | ---: | ---: |
| Omitted | 1,000 | 5,987 | 1,540,072 | 19 |
| Present | 2,995 | 3,992 | 1,540,072 | 19 |

Both deletion orders agree exactly. Across recipient schedules, adding back only
the committed reward normalizes the keeper principal and beneficiary budget.
Five user payouts plus beneficiary payment plus burn equal the original
125,101,000 deposit atoms. Final mint supply decreases by precisely the burn;
the 19 new surplus atoms are separately returned to the admin. Token/portfolio/
owner/provider frames remain intact across successful closure. The tombstone
retains canonical header rent; every vault/market/previously deleted portfolio
lamport above that rent reaches the admin, with the separate payer paying fees.

The shared submission helper verifies signatures and 1232-byte transaction size,
simulates each two-instruction rejection's successful prefix, pins the exact
failure index/error, and compares complete tracked and compiled `Account`
images including data, ownership, lamports and absence. Only the exact payer
signature fee is deducted from rollback expectations.

## Validation

Reused wrapper: `/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so`.
SHA-256: `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
SPL/ATA artifacts use the existing LiteSVM registry helpers. No matcher artifact
is required. The private host cache was copied using `cp -a` from
`/dev/shm/row422-native-terminal-reward-20260916-target`; all subsequent build
and temporary writes use the private paths below.

The final command passed **7 tests, 0 failed, 0 ignored, 1439 filtered out**.
The new selector pins **4 liquidations, 28 progressing terminal closes, 20
portfolio deletions, 4 beneficiary payouts, 4 slab closures and 136 exact
rollbacks**. Peak measured transaction and rollback CU were **318,538**;
committed slab closure peaked at **35,777 CU**. The helper budgets 500,000 CU
per instruction. Measured CU can vary with fresh account addresses.

Adjacent controls passed: destination retries (72 exact rollbacks; peak 345,500
CU), native terminal reward (392 rollbacks; peak 322,832 CU), parent terminal
reward (32 healthy/suffix plus 4 waiting rollbacks; peak 319,613 CU), normal slab
rent refund (26,381 CU), and both source-completeness sentinels. The four economic
selectors together account for 636 exact rollbacks. The first development run
failed only because the new test expected portfolio rent at the owner; source
review and the existing deletion control confirmed rent belongs in the slab,
and the assertion was corrected. No production change resulted.

Exact final selectors and environment:

```bash
export CARGO_TARGET_DIR=/dev/shm/astra-row422-composition-20260916-target
export TMPDIR=/dev/shm/astra-row422-composition-20260916-tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::reward_terminal_redemption::reward_slab_closure::v16_program_hybrid_reward_provenance_survives_portfolio_deletion_and_slab_closure \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::reward_terminal_redemption::v16_program_hybrid_catchup_rewards_survive_resolved_cohort_redemption_order \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::reward_terminal_redemption::native_reward_terminal::v16_program_native_hybrid_omitted_rewards_stay_unclaimed_through_sync_unwrap_and_terminal_replay \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::reward_destination_retry::v16_program_hybrid_reward_destination_retries_preserve_only_committed_receipts \
  inv_045_no_free_mark_movement::v16_program_mark_writer_and_trade_exit_composition_is_source_complete \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::v16_program_close_slab_refunds_exact_vault_and_market_excess_rent_after_normal_exit \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::v16_program_terminal_stock_and_close_slab_composition_is_source_complete
rustfmt --check --edition 2021 \
  tests/invariants/cu/inv_045_reward_terminal_redemption.rs \
  tests/invariants/cu/inv_045_reward_slab_closure.rs
git diff --check
git diff --cached --check
git show --format=fuller --check HEAD
git diff --exit-code 8839eb1aec50e9f90a06e123e3752ad2314ad9e9 HEAD -- \
  src Cargo.toml Cargo.lock tests/fixtures \
  ':(glob)tests/invariants/*.tsv' ':(glob)tests/invariants/**/*.tsv'
```

Rustfmt, working/staged/committed whitespace checks and protected diff pass.
The committed diff contains only the new test, three module-registration lines,
this report and the invariant README entry. The isolated branch is committed
locally only; its worktree is clean.

## Limits

This is bounded conformance for one classic SPL quote, one Pyth provider, one
downward Hybrid episode, fixed reward policy, zero funding and a flat keeper.
Resolution freezes a lagged mark after one liquidation; full catchup is owned
by the adjacent terminal selectors. No maintenance clipping, exposed keeper,
CPI switching, native custody, multiple providers, positive settlement dust,
source-backing expiry or provider-earnings history is claimed. No payout snapshot
is captured: `ClaimResolvedPayoutTopup` is a retained negative replay here, not
coverage of successful snapshot-backed top-ups. The new guarantee is final
disposition of real discovery/reward/retained-fee stock, not arbitrary-history
closure of INV-045.

Setup and resolution require the admin; role assignment needs both parties'
consent; this test deletes portfolios with their owner signatures. Final slab
closure requires the admin. The resolved user and insurance payouts themselves
are unsigned; the insurance beneficiary key is no longer retained. Missing
required cleanup authority and unavailable custody are outside this product.
