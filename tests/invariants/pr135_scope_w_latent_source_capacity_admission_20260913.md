# PR135 Scope W: Latent Source Capacity Admission

Base: latest fetched `origin/codex/astra-open-holdout-ledger-20260912`,
`d134c64d788e49264ecc0187a75209053925f43a`.
Branch: `codex/pr135-scope-w-latent-source-capacity-admission-20260913`.
Worktree: `/run/percolator-pr135-scope-w-20260913`.

The worktree was initially created at `/tmp/percolator-pr135-scope-w-20260913`
and relocated with its changes and private artifacts after storage failures.
Only this new worktree was edited. Neither prohibited worktree's tracked files
were changed. Only the requested base's code, README, invariant charter, finding
inventory and coverage ledger were consulted; no external PR branches, diffs or
tests were inspected. Git's shared worktree bookkeeping records this checkout.

## Remaining Gap

`open_findings.tsv` lists 423 as historical `missing` inventory. The live
`coverage_reopenings.tsv` entry is a coverage label with status OPEN, not a
counterexample specification. Its broad guarantee requires admission to reserve
every future settlement resource needed for exit.

Existing INV-028 tests cover historical/latent domain boundaries, one free slot,
joint batch reservations, active-leg increases, terminal and Recovery
materialization, replacement of latent positions, CPI renewal, and lien-backed
admission. Scope G adds two independently full claimant tables sharing a debtor,
staggered terminal materialization and exact attribution across both payout orders.
It explicitly excludes asset reuse and risk-admission/resource closure.

The sibling-generation control changes an unused, unrelated slot while all
fourteen positions remain active. The latent-pair-reuse control replaces exposure
with another asset, without replacing that asset's generation. Neither composes
actual prior exposure on the reused slot, capacity admission in its new generation,
different historical occupancy, rollback/retry and complete domain payout.

Scope W adds that finite composition. It does not claim generic closure or promote
the finding classification. In particular the existing resource-reservation control uses 16 historical
lien-backed domains and two future domains; this new family has no provider liens
and cannot extend its funding-resource guarantee to maximum shape.

## Public Construction And Oracle

The new exact selector is:

`inv_028_source_domain_realizability_cap::historical_latent_capacity::generation_capacity_admission::v16_program_used_generation_admission_reserves_latent_capacity_through_exact_exit`.

The matrix is 24 worlds: 11/12/13 historical assets, both signs,
original/reused generation and single/batch bilateral transport. The shared
`History` fixture uses System/SPL/ATA/wrapper/matcher setup instructions; protocol
state is never installed directly. SOL funding, program loading and Clock warps
are harness inputs. Every owner starts with 1000000 collateral atoms.

The final asset first undergoes a zero-PnL position round trip. In reuse worlds,
public retirement and reactivation replace that used, empty generation. An input
journal consumes the observed pre-activation `next_market_id` exactly; portfolio
incarnations and complete portfolio/custody Accounts survive the replacement.
An admission instruction built for the old generation must reject specifically
as AssetGenerationMismatch. The reused slot and spare receive AuthMark setup
before historical PnL exists, as required by the public reconfiguration gate.
Complete historical round trips then create unequal claims on both sides of each
historical asset, leaving 22/24/26 detached sources before capacity admission.

New admission occupies one to three asset legs. Historical sources union both
possible settlement domains of every active leg must equal exactly 28, although
the new pairs initially occupy no source records. A separately activated spare
asset would require 30 domains and must reject specifically as InvalidInstruction,
both while domains are latent and after all 28 materialize. Its full-table request
is freshly bound to the current position epochs so stale intent cannot mask the
capacity guard.

Input quantities and signed one-atom mark moves independently determine each
domain's exact gain. Public cranks settle the first sides; cross-zero risk creates
twice the opposite exposure and the reverse mark materializes the remaining sides.
Historical records retain their exact values. Source generation, capital, PnL,
OI, custody, backing reservation, credit rate, and absence of insurance/liens
compose with independent stock and reservation censuses. A read-only swap of
unequal domain claims preserves aggregate value but fails the exact vector oracle.

Every admission chunk and every final live reduction is executed with an invalid
withdrawal suffix, checked for the exact program error and complete Account
rollback, then committed with the unchanged, previously signed prefix bytes.
Terminal source retirement and the debtor's actual SPL payout receive the same
rollback/retry check. Rollback compares every compiled transaction account and
the complete owner/portfolio/token/matcher/market frame. The payer differs by
exactly the transaction's signature fee. Successful prefix logs must match the
expected instruction index.

Live settlement inherits the fixture's bound of four cranks per owner/observation,
each decreasing pending authenticated accrual plus unpaid input-derived debt.
Each live final reduction decreases active-leg count. Once fully flat, authority
resolution and authenticated owner-window expiry permit payer-only CloseResolved
transactions. The terminal rank is occupied source count plus an unpaid entitlement
bit. Each accepted call strictly decreases it; each owner is bounded by 29 calls.
Peer Accounts, exact source retirement, remaining domain backing, mint, custody,
and complete owner entitlement are checked through payout. Only subsequent empty
portfolio deletion uses owner signatures.

## Scope And Row Impact

Bounded INV-028 evidence covers sparse slot capacity and realizability; INV-031
covers single-use domain backing through payout; INV-057 covers bilateral live
reductions; INV-073/078 cover unsigned economic disposition after resolution;
INV-077 covers measured transactions; INV-082 covers the stated finite ranks;
INV-089 covers used empty-slot replacement and subsequent generation-bound claims.
The optional reuse control is not full activation/reactivation equivalence.
Generation replacement precedes historical claims; this test does not establish
same-slot replacement or AuthMark reconfiguration while those claims exist.

Row 423 stays OPEN. INV-028 and INV-073 stay REFUTED_CURRENT;
INV-031/057/077/078/082/089 stay OPEN_EVIDENCE. All remain sampled. The finding
inventory, machine status rows and all other coverage labels are unchanged.

Limits: two solvent owners, classic SPL, one unused spare asset, integral gains,
zero trading/maintenance/funding fees, AuthMark, bilateral routes, at most three
simultaneous legs and one replacement per world. There is no provider principal,
insurance reservation, lien impairment, expiry, bankruptcy, side reset, Recovery
materialization, missing-oracle schedule, native quote, CPI fill, arbitrary action
generator or administrative slab retirement. Settlement/payout order is coupled
to position sign, not independently crossed. Live reductions require both owners;
resolution requires authority. Permissionlessness begins at terminal payout after
the owner window. All latent domains materialize while Live. Other future resource
classes and the broader liveness theorem remain open.

## Implementation Correction

The base accepted the additional admission while other legs still reserved latent
domains, contradicting the independent union-of-future-domains oracle. The base
run stopped at that admission assertion; no unsupported exit sequence was pursued.
The existing snapshot counted occupied source records plus the requested asset,
without the latent domains of unrelated active legs.

Implementation commit `0dbac7d2826238c5ac9c17598e13ef317170820c` extends that bounded snapshot to include both
domains of every surviving active leg. It projects each requested account's signed
delta, so a leg fully closed by the same transaction releases its latent pair.
Its occupied claims remain reserved. Single and batch paths share this helper,
including their CPI callers, and pure reductions retain their existing admission
behavior. No persisted layout, wire ABI, dependency or engine pin changes.

The used-slot setup was moved before historical PnL after the existing global
oracle reconfiguration gate rejected the initial fixture ordering. The generic
test transport already binds current asset generations; no shared fixture helper
change is necessary. The final full-table negative request uses current position
epochs. The invalid withdrawal suffix reaches Unauthorized while Live and the
documented EngineLockActive mode guard after resolution. These fixture corrections
are separate from the observed capacity-admission mismatch.

## Validation

The base SBF fails the capacity-admission assertion. The corrected default-feature
SBF passes the new selector 1/1 in 61.47s: 24 worlds, 12 reused generations,
2868 shared History calls, 972 additional checked transactions, 192 exact
rollbacks, 132 restored successful prefixes and 696 terminal calls (29/world).
The 132 prefixes comprise 36 admissions, 48 live reductions, 24 claimant source
retirements and 24 debtor SPL payouts. Rejections also include 48 capacity checks
and 12 old-generation checks. The exact selector listing selects one test.

For 11/12/13 historical assets, exact owner payouts are respectively
1000096/999904, 1000078/999922 and 1000062/999938 atoms, independently of route,
sign and reuse. All worlds finish with zero vault, capital, insurance, source
backing, OI and materialized portfolios. No post-admission funding, historical
claim conversion before admission, or forfeit is required.

Peak reported CU is 1152561, leaving 222439 below the 1375000 test ceiling.
Measured packets are at most 842 bytes. The transaction/packet counters cover
the explicit submission helper. History counts separately cover its trade/mark/
crank calls. Lifecycle/configuration/setup, resolution and deletion are not in
those call totals; resolution and deletion also contribute to the reported CU
maximum. Setup and lifecycle helper calls are not comprehensively packet-measured.
The owner-window wait lands at its exact five-slot boundary.

All six exact controls pass on the corrected artifact:

| Control | Worlds | Result / Peak CU |
| --- | ---: | --- |
| Scope G shared source / late claimant | 4 | PASS, 930572 |
| Historical lien / future resource reservation | 8 | PASS, 1193886; 32 rollback checks |
| Historical / latent boundary | 32 | PASS, 1071585 |
| Close/open latent-pair reuse | 8 | PASS, 933012 |
| Active-leg admission, all four transports | 16 | PASS, 981081 |
| Current 14-leg TradeNoCpi | 1 | PASS, 541529 |

The charter/index, audit-summary, authoritative-status and reopening-consistency
metadata gates pass. Formatter, unstaged/staged/HEAD diff checks and both commit
show checks pass. No unfiltered suite was run. The host metadata target emits
existing dead-code warnings, and Cargo reports Solana dependency future-compatibility
warnings. The SBF build has no stack-overflow diagnostic.

Artifact SHA-256, engine pin `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`:

- Base wrapper: `49f49d1a8a7a3c20e4e3e09c635c4d5ce08925056030668b6838ccf7224800b0`.
- Corrected wrapper: `79b298706ca7ffab09a017e5f37c4c0fc43d4e6292c092d126f4fbe8dff1b673`.
- Auth matcher: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

Host Cargo/rustc are 1.90.0; SBF uses platform-tools v1.52. The initial disk build
and runtime-directory host build hit separate full filesystems; `/run` also has
noexec semantics. A private executable 6 GiB tmpfs at this worktree's `target/host`
resolved compilation. Only this task's build cache was cleaned. Both deployable
artifacts were built from this worktree, with no borrowed artifacts or external
test sources. The initial wrapper and matcher commands used the worktree's then
current absolute path; equivalent commands at its final location are:

```sh
cd /run/percolator-pr135-scope-w-20260913
export CARGO_TARGET_DIR="$PWD/target/host" TMPDIR="$PWD/target/host"
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir target/deploy -- --locked
env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked
sha256sum target/deploy/percolator_prog.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF="$PWD/target/deploy/percolator_prog.so"
cargo test --locked --offline --test v16_cu inv_028_source_domain_realizability_cap::historical_latent_capacity::generation_capacity_admission::v16_program_used_generation_admission_reserves_latent_capacity_through_exact_exit -- --exact --list
cargo test --locked --offline --test v16_cu inv_028_source_domain_realizability_cap::historical_latent_capacity::generation_capacity_admission::v16_program_used_generation_admission_reserves_latent_capacity_through_exact_exit -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_028_source_domain_realizability_cap::historical_latent_capacity::shared_source_late_exit::v16_program_shared_history_preserves_late_claimant_capacity_and_exact_exit -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_028_source_domain_realizability_cap::historical_latent_capacity::exit_resource_reservation::v16_program_historical_liens_preserve_future_domains_and_owner_exit -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_028_source_domain_realizability_cap::historical_latent_capacity::v16_program_historical_and_latent_domains_share_bounded_settlement_capacity -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_028_source_domain_realizability_cap::single_slot_admission::latent_capacity_reuse::v16_program_latent_pair_reuse_preserves_historical_claims_and_replacement_exit -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_028_source_domain_realizability_cap::historical_latent_capacity::active_leg_admission::v16_program_active_leg_increases_preserve_latent_and_full_domain_owner_exit -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_077_bounded_work_and_maximum_shape_compute::v16_bpf_current_full_14_leg_tradenocpi_is_under_tx_limit -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_ -- --nocapture --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant -- --exact --nocapture --test-threads=1
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git diff HEAD --check
git show --check --oneline HEAD
git show --check --oneline HEAD^
```

Changed files: `src/v16_program.rs` in the implementation commit; and
`cu/inv_028_generation_capacity_admission.rs`, its module registration in
`cu/inv_028_historical_latent_capacity.rs`, `README.md`, comment-only
`coverage_reopenings.tsv` additions, and this audit in the separate coverage commit.
