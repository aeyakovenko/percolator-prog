# PR135 Scope K: dense terminal claimant settlement

Base: `origin/codex/astra-open-holdout-ledger-20260912` at
`f0c2bb729ed1476eca21dd81b53a7a7245bd2fcd`. Isolated worktree:
`/home/anatoly/percolator-pr135-scope-k-20260913`; branch:
`codex/pr135-scope-k-dense-terminal-20260913`.
Only invariant documentation/status ledgers, this base's code/tests, and its
locked engine source were consulted. No open PR branches, diffs or tests were
inspected or copied. The original worktree was not modified.

## Added cell

The [new public LiteSVM probe](cu/inv_071_dense_terminal_claimants.rs), mounted
under [INV-071](cu/inv_071_crank_progress.rs), follows the remaining dense cell in
the [Scope C ledger](pr135_scope_c_terminal_actionability_20260913.md).

Both deterministic histories activate all 5,782 supported market slots. Four
funded portfolios each hold 14 active legs, for 56 simultaneous legs across 28
traded assets. Each of the two profitable portfolios holds 14 value-bearing,
unliened source records, for 28 occupied source domains in the cohort. The
placement includes 255/256/257, 511/512/513 and the last supported asset. All four
claimants are settled and deleted before reserve payment and market retirement.
This combines dense economic exposure with complete claimant/reserve disposition;
the existing sparse generated terminal-actionability probe is unchanged.

Seeds `0x4b00` and `0x4b01` drive `StdRng` trade quantities (1 through 5), backing
principal (31 through 71) and insurance funding (11 through 29). The histories
reverse profitable source sides, claimant order, reserve-domain order and
provider/insurer priority; they use `CloseResolved` and hint-free resolved
`PermissionlessCrank`, respectively. This is a two-history stateful probe, not
exhaustive generation or a shrinking reference engine.

System, SPL, ATA and wrapper instructions construct all economic accounts and
funding. The only harness state operations load programs, airdrop signer SOL and
advance Clock. Both reserve beneficiaries and all portfolio owner keypairs are
discarded after setup. Terminal economic payments have only the keeper/payer
signature. Administrative portfolio deletion and `CloseSlab` use the retained
market authority. Mint authority is revoked after the initial public minting.

## Independent checks

Each owner deposits 100,000 atoms. Authenticated marks move exactly one price
unit in the selected profitable direction; no fees, funding or bankruptcy enter
the schedule. Claimant entitlement is independently `deposit +/- sum(units)`.
Before resolution, each source domain and exact positive claim face is checked
against those inputs. The test never calls the engine progress selector or uses
its actionability flags to choose a successful terminal action.

All peers first detach one leg per call. Solvent counterparties then finish
before positive source realization. Every nonterminal claimant call strictly
reduces the lexicographic rank `(active legs, liened face, occupied sources,
negative PnL, capital plus positive PnL, unpaid wallet entitlement, fee-slot lag)`.
The bound is 14 detach calls plus at most 18 further calls per portfolio.
An independent census reconstructs OI from individual legs and original signed
trade quantities across all 5,782 assets after each successful claimant call;
the sum of individual portfolio capital must equal the market capital total.
Peer Accounts are framed in full, including metadata and absence.

After all claimant payments and portfolio deletions, every raw source bucket,
source credit record, insurance budget and spent counter is reconciled against
the initial funding and cumulative payments. Cached fresh-backing and insurance
totals must equal that census. Each reserve payment strictly reduces an
input-derived unpaid-reserve rank. The provider receives exactly its original
principal; the insurer receives exactly its original insurance funding.

Solvent loss settlement replenishes fresh source backing before positive claim
conversion consumes it. Conversion retains an audit receivable, spent-backing
and consumed-backing counter equal to the original per-domain gain. These are
checked explicitly throughout reserve withdrawal; they are not additional
wallet entitlements. With every payable stock at zero, the independent model
expects single-call slab retirement without an asset scan. The test checks the
exact typed tombstone, retained rent, closed vault and administrative rent refund.

Every terminal transaction reconciles fixed SPL supply against all four claimant
wallets, both reserve wallets, the admin destination and vault. Engine vault and
SPL vault must agree before retirement. No atoms are burned or paid to
the admin token destination. Portfolio deletion separately reduces materialized
portfolio count and refunds exact portfolio rent into the market.

Every post-detachment settlement call and the final retirement are first followed
by a correctly encoded, payer-signed System transfer exceeding the payer's funds.
The exact instruction-3 failure, successful wrapper prefix, full tracked and
compiled Account rollback and exact signature fee are checked. The identical
wrapper instruction then commits. Both positive claimants must have a real SPL
payout in these rolled-back prefixes. Caller funding failure is not classified
as a protocol continuation failure.

## Classification and limits

The new bounded cell contributes dense claimant completeness and constructive
continuation to INV-067/070/071/073/077/078/082, the limited independent arithmetic
and rank checks to INV-086, full-domain census to INV-088, and exact transaction
rollback to INV-080. Reserve continuation relates to row labels 420/421/433.

Classifications are unchanged. `README.md`, `invariant_status.tsv` and
`open_findings.tsv` are unchanged; existing `OPEN_EVIDENCE`, `REFUTED_CURRENT`,
`SAMPLED`, conditional-TCB and `missing` labels remain authoritative. A positive
finite cell does not close an invariant or supersede another current refutation.

The probe reaches the active-leg maximum per portfolio, with 14 occupied sources
per profitable portfolio, not the 28-source wrapper bound per portfolio. It
does not expose every market asset simultaneously. It covers SPL-primary custody,
solvent integral claims, zero fees/funding, fresh provider principal and unspent
insurance, fixed authority/oracle generations and fixed destinations. It does
not combine this shape with partial receipts, source expiry, insurance recredit,
provider earnings, native custody, active liens or persisted terminal scanning.
The Scope C sparse receipt/expiry/recredit product and these dense states remain
distinct cells. No production or dependency change is retained.

## Exact validation

Commands run from the isolated worktree. The private target was copied without
hard links from `/dev/shm/pr135-scope-c-20260913-target`; no other worktree's test
source or binary was used as evidence. Default-feature SBF was rebuilt from this
base and the locked engine `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
SBF SHA-256:
`71833878f8d52373e501e70ed21d88495051695f2533b4c55362ae90e737aa9d`.

```sh
cd /home/anatoly/percolator-pr135-scope-k-20260913
export CARGO_TARGET_DIR=/home/anatoly/pr135-scope-k-20260913-target
export TMPDIR=/home/anatoly/pr135-scope-k-20260913-tmp
export PERCOLATOR_FUZZ_SBF=/home/anatoly/pr135-scope-k-20260913-sbf/percolator_prog.so
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_NET_OFFLINE=true cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /home/anatoly/pr135-scope-k-20260913-sbf -- --locked
sha256sum "$PERCOLATOR_FUZZ_SBF"
cargo test --locked --offline --test v16_cu inv_071_crank_progress::dense_terminal_claimants::v16_program_dense_claimants_and_reserves_reconcile_at_supported_capacity -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_071_crank_progress::generated_terminal_actionability::v16_program_generated_receipts_and_reserves_have_constructible_terminal_progress -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --quiet --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming -- --exact --quiet --test-threads=1
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
```

The new selector passes **1/1 in 105.92s**: two capacity histories, 290 committed
terminal transactions, 58 exact rollbacks, and four rolled-back SPL claimant
payouts. Each history has 145 committed transactions and 29 rollbacks. The
tracked resolution/terminal peak is **896,912 CU**, below the per-transaction
1,375,000-CU limit. Claimant wallets are `[100041, 99959, 100039, 99961]` and
`[100041, 99959, 100050, 99950]`; each history returns all 400,000 deposited
claimant atoms plus the independently funded reserves to their respective
beneficiaries. The successful SBF rebuild took 5.76s.

The unchanged sparse control passes **1/1 in 180.26s**: 16 histories, 767
committed terminal transactions, 75 exact rollbacks and peak 314,698 CU. Both
INV-079 metadata selectors pass **1/1**. Formatting and staged/unstaged whitespace
checks pass. Existing dead-code and Solana future-compatibility warnings remain.
No broad suite or engine proof was run.

Development iterations corrected host field/index references, a rank closure's
borrow, the expected historical conversion counters, the empty-stock retirement
schedule, and the placement of the rollback check around combined source removal
and payout. These were test-oracle or harness corrections. No production failure
or refutation is claimed by this probe.
