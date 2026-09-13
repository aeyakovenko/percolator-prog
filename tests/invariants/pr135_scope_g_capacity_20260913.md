# PR135 Scope G: Shared Source Capacity

Base: `origin/codex/astra-open-holdout-ledger-20260912` at
`bba6bab44a92ef647a49dcd9fad2cb29febfd333`.
Branch: `codex/pr135-scope-g-latent-exit-20260913`.
Worktree: `/home/anatoly/percolator-pr135-scope-g-latent-exit-20260913`.
Only invariant documentation/status ledgers and current code were used; no open
PR diffs or tests were consulted. Build outputs were compiled privately from
this worktree, without copying other worktrees' tests or artifacts.

## Evidence

The new public LiteSVM selector is
`inv_028_source_domain_realizability_cap::historical_latent_capacity::shared_source_late_exit::v16_program_shared_history_preserves_late_claimant_capacity_and_exact_exit`.

System, SPL, ATA and wrapper instructions construct a 14-asset market and three
portfolios, each funded with 1000000 atoms. Thirteen complete position episodes
leave both claimants with 26 detached, unequal source claims. A final episode
grows each table to 27 occupied sources, with one domain still latent after a
cross-zero trade. The last mark pays the shared debtor's loss before resolution
but leaves both claimants untouched. Each historical/future source union is 28.
The four histories cross long/short orientation and which claimant proceeds first.

At owner-window expiry, public `CloseResolved` and `PermissionlessCrank` calls
independently materialize both final claims and fill both 28-source tables.
After the first materialization, the peer still has exactly its 27 historical
records; the shared domain holds the input-derived backing for both claims while
its registered bound includes only the first. The cohort's terminal readiness
then permits the debtor's senior payout and the two complete claimant payouts.
After the first claimant exits, the second still owns all its exact claims and
the domain ledger retains exactly its unpaid backing. All terminal economic
instructions use only the unrelated transaction payer's signature. Mechanical
deletion of the three empty portfolios separately uses each owner's signature.

The independent ledger computes gains from signed integral quantities and
one-atom price moves, maintaining owner/domain earned, funded and redeemed
amounts. It checks every post-funding call against the permitted settlement
prefix, exact domain claim sums, remaining backing, capital, PnL, SPL custody,
active positions and Live OI. Stock, reservation and rate censuses supplement it.
Complete untouched portfolio/token/mint frames are checked on the applicable
routes. Swapping the unequal owner claim vectors and moving an atom between
domains both fail the oracle while preserving the corresponding aggregate sum.
These are observation-only mutations and are never installed in LiteSVM.

Live settlement is bounded by four calls per owner/observation, each decreasing
input debt plus outstanding authenticated accrual. Claimant terminal rank is
`2 * latent_domains + active_legs + occupied_sources + unpaid_entitlement_bit`.
Every accepted terminal claimant call strictly decreases it. Two materialization
calls, one debtor exit and at most 31 payout-continuation calls per claimant give
a fixed bound of 65 terminal calls per history. The debtor call is checked for
complete senior payout and terminality. Observed: 59 terminal calls per history,
236 total, within 1128 checked post-funding calls. Peak measured CU is 917414,
below the 1375000 test ceiling by 457586. Claimant terminal transactions are at
most 465 bytes, below 1232. Setup funding/configuration is outside the measured
call/CU totals; packet measurements cover claimant terminal transactions.

Exact final owner tokens are `1000054`, `1000164`, and `999782`, independent of
orientation and order. Each history ends with zero vault, capital, insurance,
source claims, source backing, OI and materialized portfolios. No forfeits or
new funding are needed after admission.

## Distinct Scope

`inv_028_terminal_latent_capacity` owns one claimant with fourteen active legs,
fourteen historical sources and fourteen latent sources, plus owner-window
boundaries. `exit_resource_reservation` owns sixteen lien-backed historical
domains, provider surplus withdrawal and two future domains, within 18 sources.
Neither checks two independently full source tables sharing the same domain
backing through cohort readiness and separate complete owner payouts. The new
probe uses one active asset at its capacity frontier and detached historical
claims, so it adds that composition without extending those controls' products.

Initial fixture runs were corrected for current authenticated observations,
bounded market catch-up, and a stale-resolution threshold long enough for the
multi-asset setup. The progress oracle also distinguishes materialization from
source retirement and respects the existing cohort-readiness payout gate.
An early claimant-only schedule returned NonProgress while peer work remained;
the final test includes the bounded public peer continuation. No production
correction or invariant classification change follows from those fixture runs.

Bounded secondary evidence applies to INV-031 attribution and single-use backing,
INV-057/073/078 public economic exit, INV-077 measured work, and INV-082 strict
rank decrease for this constructed family. INV-089 activation/reactivation is
unchanged. Row 423 remains OPEN. INV-028/073 remain REFUTED_CURRENT and
INV-031/057/077/078/082/089 remain OPEN_EVIDENCE in the machine ledger.

Limits: four deterministic histories, classic SPL, integral fully backed claims,
one shared solvent debtor, one final active asset, bilateral no-CPI trades, fresh
AuthMark observations, zero fees/funding and administrative resolution followed
by authenticated owner-window expiry. Underfunding, liens, expiry, Recovery,
asset reuse, more owners, maximum active-leg products, arbitrary generators,
other future resource classes and administrative market retirement remain outside
this increment. This is sampled conformance evidence, not generic row closure.

## Validation Commands

Default-feature SBF was rebuilt offline with platform-tools v1.52; the engine
pin remains `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`. Host Cargo and rustc are
1.90.0. Artifact SHA-256:

- Wrapper: `71833878f8d52373e501e70ed21d88495051695f2533b4c55362ae90e737aa9d`.
- Auth matcher: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

Build commands, run from the worktree:

```sh
env CARGO_TARGET_DIR=/dev/shm/pr135-scope-g-20260913-target CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/pr135-scope-g-20260913-target/deploy -- --locked
env CARGO_TARGET_DIR=/dev/shm/pr135-scope-g-20260913-matcher-target CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked
sha256sum /dev/shm/pr135-scope-g-20260913-target/deploy/percolator_prog.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
```

The test commands use these environment assignments:

```sh
export CARGO_TARGET_DIR=/dev/shm/pr135-scope-g-20260913-target
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/pr135-scope-g-20260913-target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu inv_028_source_domain_realizability_cap::historical_latent_capacity::shared_source_late_exit::v16_program_shared_history_preserves_late_claimant_capacity_and_exact_exit -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_028_source_domain_realizability_cap::historical_latent_capacity::concurrent_latent_capacity::terminal_latent_capacity::v16_program_full_latent_settlement_survives_terminal_owner_window_expiry -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_028_source_domain_realizability_cap::historical_latent_capacity::exit_resource_reservation::v16_program_historical_liens_preserve_future_domains_and_owner_exit -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --quiet --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming -- --exact --quiet --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant -- --exact --quiet --test-threads=1
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git diff HEAD --check
```

Results: the final new selector passes 1/1 (four histories, 17.13s). Both named
controls pass 1/1: terminal latent capacity covers four histories (peak 1219281
CU); resource reservation covers eight (peak 1186467 CU, 32 exact rollbacks).
Charter/index and authoritative machine-status gates pass 1/1 each. Formatter
and unstaged, staged and HEAD whitespace checks pass. Existing dead-code and
Solana dependency future-compatibility warnings are emitted by the test targets.

The additional `v16_post_pr135_counterexamples_reopen_every_affected_invariant`
gate fails at its row-set equality: the expected set contains 435 and the actual
set does not. This identical failure is present on the untouched base at line
2050 of the metadata test. It is outside Scope G; row 435 and classification
data were left unchanged. The baseline confirmation used the same environment:

```sh
git worktree add --detach /dev/shm/pr135-scope-g-metadata-base-20260913 bba6bab44a92ef647a49dcd9fad2cb29febfd333
cd /dev/shm/pr135-scope-g-metadata-base-20260913
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant -- --exact --quiet --test-threads=1
```

Changed files:

- [cu/inv_028_shared_source_late_exit.rs](cu/inv_028_shared_source_late_exit.rs): one new probe and its independent ledger.
- [cu/inv_028_historical_latent_capacity.rs](cu/inv_028_historical_latent_capacity.rs): module registration.
- [README.md](README.md): coverage entry.
- [coverage_reopenings.tsv](coverage_reopenings.tsv): comment-only row 423 evidence.
- [pr135_scope_g_capacity_20260913.md](pr135_scope_g_capacity_20260913.md): scope, provenance and validation record.

Production, dependency pins, machine status rows and classification data are
unchanged. The unfiltered test suite was not run.
