# PR135 Scope C: generated terminal actionability

Repository: `/tmp/percolator-astra-watch.Cb2E7d`. Base branch:
`codex/astra-open-holdout-ledger-20260912`, commit
`d977bd6a68bd9e39c550fe11915ffd30647e6f9b`. Fresh isolated worktree:
`/tmp/percolator-scope-c-terminal-actionability-20260913`; branch:
`codex/pr135-scope-c-terminal-actionability-20260913`.
Only base source/tests and the locked engine dependency were consulted, not open
PR diffs/tests. `/home/anatoly/percolator-prog` was not accessed or modified.
Row numbers are coverage labels, not reproducer specifications.

## Retained cell

[The new generator](cu/inv_071_generated_terminal_actionability.rs), mounted under
[INV-071](cu/inv_071_crank_progress.rs), composes public user settlement and retained
receipts with absent reserve beneficiaries, repeated insurance recredit behind a
persisted scan prefix, exact rollback, and final retirement in one history.

It adds a real dimension beyond `inv_070_generated_prefix_actionability.rs`
(generated fresh backing/expiry/surplus without receipts/recredit) and
`inv_071_terminal_prefix_recredit.rs` (directed earlier-asset recovery without this
partial receipt pool and generated joint scheduler). Neither existing test was
copied or modified. No redundant standalone probe or failure corpus is retained.

There are four directed shapes and twelve deterministic ChaCha-generated histories
(seed byte `0xc7` repeated 32 times, with shrinking). Inputs vary the placement of
two later backing buckets, their amounts/deadlines, provider principal paid before
expiry, insurance payment chunks, claimant order, and `CloseResolved` versus
permissionless crank. Directed placements straddle asset indexes 255/256/257;
one history uses all 5,782 configured/activated market slots. Generated placements
also include a second scan boundary. Both expiries must recredit the earlier
insurer after the cursor passes it, including when the later-index deadline has
already elapsed. Both recoverable-insurance shortfall and excess-residue burn are
exercised.

All economic accounts and funding use public System, SPL, ATA, and wrapper
instructions. Harness exceptions only load programs, fund SOL signers, and set
Clock. No mint/vault/market/portfolio economic image is injected. Mint authority
is revoked after funding. Beneficiary/owner keypairs are unavailable after setup;
all user and reserve payouts are payer-only signed. Administrative portfolio
deletion and slab retirement still require the market authority.

## Oracle and partition

The fixed trade prefix has one insured source and two uninsured sources. The
oracle explicitly checks the loss-adjusted junior faces, not the pre-bankruptcy
positive PnL. The insured claimant realizes 100 atoms of source backing and
retains a 100-atom receipt; the other receipt faces are 30, 70, and 20. The public
pre-realization credit rate is checked against the independently expected
100-atom conversion. This is not a general independent pricing/ADL engine.

The initial receipt pool is 201 atoms (100 spent insurance, 100 released junior
counterparty principal, one donated backing atom) against 220 atoms of claims.
Three receipts receive `floor(face * 201 / 220)` while the final 20-atom claim
remains unreceipted. Its scheduled backing expiry releases 37 atoms (20 recovered
principal plus 17 provider atoms). All four receipts then pay their complete
loss-adjusted faces before portfolio deletion. Final claimant wallets must be
`[1200, 0, 1030, 1070, 0, 1020, 0, 137]`. Duplicate zero-due claims are explicitly
terminal replays, not counted as progress-rank descent.

The suffix starts with 200 atoms of remaining insurance, 100 spent insurance,
100 atoms of provider receivable, and 18 free atoms. These are reconciled against
every source bucket, budget, spent counter and SPL wallet. The standalone suffix
model derives its next action from generated obligations, cumulative payments
and Clock, never the engine's progress selector, aggregate actionability flags,
or the observed successful outcome. Recredit is bounded by free residual,
remaining insurance spend, and the paired provider receivable. Earlier insurance
is reconsidered after each later expiry even though the scan cursor has passed
asset zero. Cached totals are checked against a raw domain census, not trusted
as the obligation oracle.

Settlement uses a lexicographic rank over backing normalization, active legs,
liens, source slots, debt, capital/PnL, unpaid wallet entitlement, and fee/receipt
completion. Each selected nonterminal public call must strictly decrease it.
Deletion separately decreases materialized portfolio count and accounts for exact
rent. The suffix rank is `(future deadlines, fresh buckets, unpaid recoverable
insurance, unpaid scheduled provider principal, remaining scan distance, live
slab)`. Every selected action must reduce it and match the modeled post-state;
retirement sets it to zero. The suffix call bound depends on scan capacity and
insurance payment chunk size.

Clock advancement is explicit and leaves the market Account unchanged. A parked
scan before expiry must return `EngineLockActive` with exact Account rollback;
the deadline then enables bounded continuation. These are waits for the chosen
expiry schedule, not claims that every public route is blocked: remaining fresh
provider principal could instead be withdrawn. Likewise, scheduling the receipt
source's expiry does not assert that earlier source realization is unavailable.

Every terminal transaction checks SPL supply against all claimant/reserve/admin
wallets plus vault, and checks engine vault equality while the market is live.
All nonwritable peer Accounts are framed. Both behind-prefix insurance recoveries
and final retirement are followed by a correctly encoded, signed System transfer
with insufficient funds. The exact failing instruction index, successful wrapper
prefix log, all compiled/tracked Accounts (including metadata and absence), and
exact signature fee are checked before the identical wrapper call commits.
Insufficient suffix funding is caller input, not a liveness failure. Final mint
supply, burned residual, zero admin token proceeds, closed vault, tombstone and
rent refund reconcile with all prior payments.

## Coverage limits

New bounded composition: INV-063/067 claimant partition, INV-070 final disposition,
INV-071/073/078/082 constructive continuation with absent beneficiaries,
INV-077/088 per-transaction compute and scan boundaries, and INV-086 a separate
generated suffix model. Exact transaction rollback also contributes to INV-080.
Associated row labels: **417/420/421/424/433**.

Classifications remain unchanged. `README.md`, `invariant_status.tsv` and
`open_findings.tsv` are not edited; all five row labels remain `missing`, and the
existing `OPEN_EVIDENCE`/`REFUTED_CURRENT`, sampled, conditional-TCB classifications
stand. This finite positive cell does not overturn other refutations.

The maximum-capacity case has sparse economic exposure: eight portfolios, three
traded assets and two later reserve buckets, not maximum simultaneous active
legs/source slots. Only SPL-primary quote custody is covered. Fees/provider
earnings are zero; oracle/authority generations and destinations stay fixed.
The scanner itself is not required to rediscover earlier insurance: the model
constructs an asset-local public withdrawal. Live receipts cannot coexist with
slab scanning because the public wrapper requires zero materialized portfolios;
the history preserves their payout accounting across that phase boundary.

Next most valuable cell: generated dense multi-leg/source-domain claimant
settlement at supported market capacity, retaining an independent rank and the
all-claimant/reserve retirement reconciliation. The current sparse capacity
witness does not establish that product (INV-071/077/082/086).

## Exact validation

All commands ran in the isolated worktree. Host compilation reused the private
PR135 target; no broad test suite was run.

```sh
cd /tmp/percolator-scope-c-terminal-actionability-20260913
export CARGO_TARGET_DIR=/dev/shm/pr135-scope-c-20260913-target
export TMPDIR=/dev/shm/pr135-scope-c-20260913-tmp
export PERCOLATOR_FUZZ_SBF=/tmp/percolator-scope-c-terminal-actionability-20260913-sbf/percolator_prog.so
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0

# T1: new composition
cargo test --locked --offline --test v16_cu inv_071_crank_progress::generated_terminal_actionability::v16_program_generated_receipts_and_reserves_have_constructible_terminal_progress -- --exact --nocapture --test-threads=1
# T2: directed recredit control
cargo test --locked --offline --test v16_cu inv_071_crank_progress::terminal_prefix_recredit::v16_program_later_expiry_recomputes_scanned_asset_insurance_entitlement -- --exact --nocapture --test-threads=1
# T3: retained-receipt control
cargo test --locked --offline --test v16_cu inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_expiry_interleavings::v16_program_retained_claims_commute_with_late_expiry_normalization_after_catchup -- --exact --nocapture --test-threads=1

cargo fmt --all -- --check
git diff --check
```

| Command | Result |
| --- | --- |
| T1 | PASS, 1/1, 169.70s: 16 histories, 767 successful terminal transactions, 75 exact rollbacks; peak tracked 314,698 CU, terminal limit 900,000. |
| T1 capacity | Included above: 5,782 slots, 72 successful terminal transactions, four rollbacks, peak 311,018 CU. |
| T2 | PASS, 1/1, 8.89s: 16 histories, peak 227,388 CU. |
| T3 | PASS, 1/1, 24.95s: 24 histories, six expiry/claim orders, peak 491,464 CU. |
| Formatting/whitespace | PASS. |

Before the resource checkpoint, the default-feature SBF was rebuilt once from
this base with the locked engine pin
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`:

```sh
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /tmp/percolator-scope-c-terminal-actionability-20260913-sbf -- --locked
sha256sum /tmp/percolator-scope-c-terminal-actionability-20260913-sbf/percolator_prog.so
```

Build PASS, 26.03s. SHA-256:
`8a010d8283186721866a95a71d252a3265015fbf563d6ce566cd44f263659349`, identical to the
prior PR135 artifact. All subsequent test-only iterations reused it. No disk
failure occurred. T1 development iterations exposed a host index type mismatch
and incorrect fixture assumptions about peer-detachment prerequisites,
source realization, and loss-adjusted receipt faces. Those assumptions were
corrected before retaining the generated test; they are not protocol findings.
No production code, dependency, README or status-table change was needed.
