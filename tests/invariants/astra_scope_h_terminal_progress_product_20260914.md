# Astra Scope H: terminal progress product

Base: remote `origin/codex/astra-open-holdout-ledger-20260912` verified at
`d01245dd993bfe60c2448360733c3bb63c41aa19`. Isolated shared-object clone:
`/tmp/percolator-scope-h-terminal-20260914`, branch
`codex/astra-scope-h-terminal-matrix-20260914`. The two protected checkouts were
not edited. All changes and host intermediates belong to this fork; the supplied
SBF is read only. No external branch, PR patch or finding-specific baseline was
used. Rows 418/420/421/433 are coverage labels, not an implementation specification.

## Coverage comparison

The review used `scripts/loop.md`, `INVARIANTS.md`, the invariant README,
`coverage_reopenings.tsv`, terminal owner inventories under all requested INV
IDs, and the reusable public fixture, transaction and stock-oracle bodies.

| Prior owner family | Existing boundary and relation to this increment |
| --- | --- |
| INV-018/021 quote integrity and account lifecycle | Token/native Account images, supported classic SPL program, ATA construction, lamport/rent framing; reused in the new reserve product |
| INV-024 terminal earnings, role succession, reserve partition and recredit | The fee-loss fixture already combines paid users, 657 earned-fee atoms, 73 spent insurance atoms and provider principal on classic SPL; its original controls keep reserve wallets present and use fixed payout orders |
| INV-025 stock reconciliation and INV-027 seniority | Independent stock and encumbrance censuses are reused. The settled user principal/PnL frame is retained; active seniority is not new coverage |
| INV-063 expiry, spent backing and retained reserve stock | Exact/late expiry and source/earnings distinctions are composed here with unavailable wallets and insurance recovery; no new universal consumer or full-width expiry claim |
| INV-067 receipts, terminal claims and payout retries | Paid user episodes are reused fixture history; receipt materialization, haircut schedules and active claimant interleavings stay with their existing owners |
| INV-069/070 normalization, quote custody, native disposition and terminal scan | Exact retirement is checked for the bounded zero-native-residue/classic-residue cases. Dense scans, Recovery, arbitrary custody transformations and native booked-stock retirement remain outside this increment |
| INV-071/078/081/082 progress, recovery and complete routes | This generator constructs a finite suffix and checks its rank and success states. It does not enumerate all reachable market states or replace the existing lifecycle/receipt/scan generators |
| INV-073 provider/insurer/public-reserve owners | Existing controls own provider replenishment and custody handoff, frozen/recreated reserves, insurance ledgers, absent spent-insurance roles, dual rails and Recovery cleanup. This increment crosses fees plus partial/full recredit with every selected wallet mask and payment/expiry word |
| [Scope N](pr135_scope_n_terminal_permissionless_payouts_20260913.md) | 128 sampled earned-fee/payment words and wallet states on classic/native primary quotes; no insurance consumption/recredit |
| [Scope B](astra_scope_b_native_recredit_custody_20260913.md) | 16 spent-insurance histories, native custody redemption/recreation and dual-rail payments; no earned reserves or partial recredit |
| [Scope X](pr135_scope_x_native_terminal_residue_disposition_20260913.md) | 36 dual-quote histories with unspent reserves, redeemed custody and native donations; no fee/loss/recredit composition |
| [Scope L](pr135_scope_l_native_multisig_20260913.md) | 12 unspent-native-insurance/multisig histories; no provider earnings or expiry |

No additional rejection-only probe, production correction, independent-discovery
claim, proof classification or row promotion is included.

## Generator and oracle

The existing `terminal_fee_loss_world` remains the classic wrapper around a new
`terminal_fee_loss_world_with_quote(native)` constructor. Its original public
funding, trading, authenticated mark history, resolved payouts and empty portfolio
deletion are unchanged. The new owner is a child of the existing fee-partition
module so it can reuse that fixture's input constants and public transaction
helpers without exporting a second framework.

Each of the two exact selectors executes the complete declared Cartesian product:

| Axis | Values |
| --- | --- |
| Primary quote | Classic SPL; native wSOL, one selector each |
| Missing wallets/custody | All eight independent provider/operator/beneficiary masks |
| Payment/expiry word | Every permutation of principal, provider earnings, available insurance and expiry with principal preceding expiry: twelve words |
| Authenticated expiry | Slot 100 or 101, compared with the original expiry slot 100 |
| Retained principal at expiry | 17, 73, 101 atoms |
| Recredit | `min(retained principal, 73)`: 17, 73, 73 atoms |
| Remaining booked residue | 0, 0, 28 atoms |

There are 576 histories per quote, 1,152 total. A recursive generator enumerates
the admissible words and checks their cardinality and uniqueness; these are not
randomly sampled words. This is exhaustive only for the declared finite product.
Principal-before-expiry is a deliberate precondition for retaining the chosen
residue, not a claim that other histories are unreachable.

Public setup deposits user capital 52,502 and 2,000,000 atoms, provider backing
100,000 and insurance 31. The existing funded admission earns 875 utilization
fee atoms, split into 657 provider earnings and 218 insurance atoms. Subsequent
public authenticated marks and loss settlement consume 73 insurance atoms,
leaving 176 available and one source-principal atom. Exact user payouts are
0 and 2,051,699; all 100,834 remaining custody atoms are independently accounted
for by source principal, provider principal, fees and available insurance.

Before the new suffix, selected reserve holders publicly close their empty ATAs
and drain their wallets. All three reserve keypairs are dropped in every world,
including present-wallet controls. The operator never receives an economic
payment or signs a continuation. The keeper recreates required custody in the
same transaction as the first payment, without restoring a holder wallet or key.
An initially empty program-owned provider ledger is initialized lazily by the
earnings payout and records the exact paid class without a provider signature.

The suffix pays the one-atom source principal, executes its generated word, and
makes two more insurance payments totaling the recovered entitlement. Insurance
withdrawal can be the first or a later event after expiry, and earned fees can
remain unpaid across both normalization and insurance recredit. Six public
payments and one administrative expiry call suffice for economic completion.
Where covered, one more administrative call closes the slab.

`Book` consumes only fixture inputs and successful action amounts. Its expected
observation separates both principal domains, earned reserves, current insurance,
historical spend, five recipients and physical custody. An insurance payment
after expiry reclassifies at most the eligible residual once; it cannot spend
earned fees or count source receivables as new principal. Every checked prefix
also verifies raw stock/encumbrance censuses, market shape, authority profile and
control sequences, full SPL/native Account images, native lamports, mint supply,
lazy-ledger totals and unrelated accounts. User payouts and the operator/admin's
zero reserve entitlement remain framed throughout.

The same observation oracle rejects two aggregate-neutral observations in every
world: moving one atom from provider principal to earnings, and moving one paid
user atom to the provider. These mutations are only local oracle observations;
no modified account image is installed into LiteSVM.

The finite economic rank is `(expiry pending, unpaid claims plus eligible pending
recredit)`. Every payment lowers the second component by exactly its amount;
expiry lowers the first. Pending recredit is already counted after expiry, so
realizing insurance does not create a rank increase or duplicate entitlement.
All words, wallet masks and expiry slots have identical expected final payments
for each residual. Both quote selectors use that same input-derived oracle.

Before every economic/expiry continuation, the identical instruction prefix
followed by an unsigned administrative call returns `ExpectedSigner`. The helper
counts completed wrapper/ATA instructions, compares every compiled/tracked
Account, and charges only exact signature fees on rollback. Unchanged instruction
content then commits. This also covers actual final vault/slab closure and rent
refunds before the rejected suffix, followed by a successful close retry.
The reused helper enforces a 1,232-byte packet ceiling and a 1,200,000-CU runtime
budget; the new selectors additionally assert a 300,000-CU measured suffix ceiling.
Fixture initialization, mark/trade and earlier user-exit CU are not included in
the new suffix peak.

## Open rows and assumptions

| Row | Added evidence | Why OPEN remains appropriate |
| --- | --- | --- |
| 418 | Native and classic earned/spent-stock completion, custody repair and exact supported retirement endpoints | The 192 native worlds with 28 booked atoms stop after economic completion. Native booked-residue retirement and generic token/lifecycle products remain unverified |
| 420 | Unavailable provider keys and every wallet mask across both principal domains, earnings, expiry and insurance recovery | One provider/asset with fixed exposure history; no arbitrary provider succession, repeated disruption or full reachable-state generator |
| 421 | Partial/full spent-insurance recredit, missing operator/beneficiary wallets, earnings interference and two recovery payments | Fixed insurance spend and fee share, one expiry and one asset; no general insurance history or absent-admin normalization theorem |
| 433 | Mixed terminal reserve stock and missing role wallets through ranked payouts, lazy ledger, ATA repair and actual close retry | No general reserve history, active senior claim/receipt product, dual-rail switching or maximum-shape closure |

The economic rank reaches zero in all declared worlds. Exact administrative
retirement is covered in 960; the remaining 192 have exactly 28 booked native
atoms, zero beneficiary claims and unchanged native custody. This boundary is
inherited from N/X/B and is explicitly not certified or treated as a new observed
implementation mismatch.

The keeper needs rent funding and correct System/SPL/ATA execution. Authenticated
time advances to the selected boundary. The retained administrator normalizes
expiry and performs final `CloseSlab`; these calls are not keeper-only or
administrator-independent liveness evidence. Earlier empty portfolio deletion
uses the existing user-signed setup. Native claim payout is to wSOL custody;
beneficiary-free SOL redemption is not claimed. The fixture's native genesis
mint and signer airdrops are the existing LiteSVM environment construction; all
economic transitions are public instructions.

Other exclusions include active user/receipt/pending-loss states during repair,
native donations and synchronization, concurrent quote rails, freeze/delegation/
multisig transformations, Recovery, dense scans, maximum magnitudes, arbitrary
fee shares and loss schedules. INV-027/063/067 receive reused setup or bounded
composition evidence, not new full seniority, expiry-consumer or receipt proofs.
`open_findings.tsv`, `invariant_status.tsv`, and every machine reopening row are
unchanged. The added reopening text consists only of comments.

## Artifact and validation

Production and dependencies are unchanged. The original SBF is used directly:

```text
/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so
79b298706ca7ffab09a017e5f37c4c0fc43d4e6292c092d126f4fbe8dff1b673
```

The empty `git diff 0dbac7d2 HEAD -- src Cargo.toml Cargo.lock` binds the supplied
corrected Scope W artifact's production sources to this base. Scope W records
default features, platform-tools v1.52 and locked engine
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`. No matcher is needed. The private host
target is a 6-GiB executable tmpfs inside this fork because the root filesystem
is nearly full and `/run` is mounted `noexec`. No shared cache is modified.

Both new exact selectors passed on their first execution, 2/2 in 659.42 seconds
with two test threads, no ignored tests and no zero-match passes.

| Quote | Histories | Payments | ATA repairs | Exact rollbacks | Slab retirements | Unverified native residue endpoints | Peak CU |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Classic | 576 | 3,456 | 576 | 4,608 | 576 | 0 | 262,492 |
| Native | 576 | 3,456 | 576 | 4,416 | 384 | 192 | 264,551 |
| Total | 1,152 | 6,912 | 1,152 | 9,024 | 960 | 192 | 264,551 |

Each quote checks 1,344 insurance payments after expiry and 48 earnings payments
after insurance recredit. The 2,304 local observation mutations are separate from
the actual transaction rollback count. The new measured peak leaves 35,449 CU
below its 300,000-CU assertion.

All eight adjacent exact selectors passed, 8/8 in 85.80 seconds. This includes
Scopes X/N/L/B, the original fee-loss constructor's twelve histories, public
reserve disposition and seniority controls, and native quote roundtrip. The
largest separately reported adjacent peak was 384,722 CU in the original
fee-partition selector. No inherited control failure was observed. All four
`v16_program_fuzz_regressions` metadata gates passed, 4/4 in 0.01 seconds.
`cargo fmt --all -- --check`, `git diff --check`, staged whitespace and committed
`git show --check` passed. Existing unused-support and Solana future-compatibility
warnings remain. No unfiltered suite or engine/Kani proof was run.

Exact validation commands, from this fork (environment exports are equivalent
to the per-command `env` assignments used for execution):

```sh
mkdir -p target/host
sudo -n mount -t tmpfs -o size=6G tmpfs /tmp/percolator-scope-h-terminal-20260914/target/host
export CARGO_TARGET_DIR=/tmp/percolator-scope-h-terminal-20260914/target/host
export TMPDIR=/tmp/percolator-scope-h-terminal-20260914/target/host
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so

sha256sum "$PERCOLATOR_FUZZ_SBF"
git diff 0dbac7d2 HEAD -- src Cargo.toml Cargo.lock
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run > target/host/build.log 2>&1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=2 \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::terminal_progress_product::v16_program_classic_terminal_progress_product_preserves_stock_and_recredit \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::terminal_progress_product::v16_program_native_terminal_progress_product_preserves_stock_and_recredit \
  > target/host/product.log 2>&1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=2 \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::v16_program_terminal_recredit_preserves_earned_fee_partition_across_payout_orders \
  inv_073_no_permanent_user_lock::v16_program_generated_reserve_wallet_absence_preserves_fee_claims_across_expiry \
  inv_073_no_permanent_user_lock::absent_insurer_spent_retirement::native_recredit_custody::v16_program_recredited_insurance_recreates_native_custody_without_role_signatures \
  inv_073_no_permanent_user_lock::dual_quote_reserve_progress::native_residue_disposition::v16_program_native_residue_and_recreated_reserves_reconcile_quote_variant_retirement \
  inv_077_bounded_work_and_maximum_shape_compute::native_insurance_exit::v16_program_native_multisig_insurance_recreation_preserves_unsigned_retirement \
  inv_073_no_permanent_user_lock::v16_program_terminal_public_reserve_disposition_preserves_value_across_orders \
  inv_073_no_permanent_user_lock::v16_program_public_reserve_payments_wait_for_resolved_senior_disposition \
  inv_081_success_state_validity_over_complete_public_routes::v16_program_native_quote_roundtrip_preserves_lamports_rent_and_unsynced_value \
  > target/host/adjacent.log 2>&1
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant \
  > target/host/metadata.log 2>&1
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
```

The build and execution logs are retained under `target/validation/` after
copying them out of the private host mount. The mount is then unmounted to
release host intermediates. The requested Scope W artifact remains at its
original path. Only the two test files, README entry, reopening comments and
this audit note are committed.
