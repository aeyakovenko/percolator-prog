# Sibling-generation liveness conformance, 2026-09-12

Base: `7374ab2f42bd2b9a6562afe8a8d5db1d43dad43b`, the current HEAD of
`codex/astra-open-holdout-ledger-20260912` when this contribution started.
Isolated branch: `codex/astra-exposure-shape-liveness-20260912`.
Worktree: `/tmp/percolator-astra-shape-liveness-20260912`.
The integration worktree and `/home/anatoly/percolator-prog` were not edited.
Only the requested base's repository sources/tests/docs and its pinned engine
dependency informed this contribution. No open PR branches, diffs or tests were
inspected or copied. All build artifacts were built locally from this worktree.

## Overlap Audit

| Existing coverage | Existing relation | New relation here |
| --- | --- | --- |
| `cu/inv_028_historical_latent_capacity.rs` | Closed historical claims share capacity with at most two live legs | All fourteen legs remain active across a change to configured market/source shape |
| `cu/inv_028_concurrent_latent_capacity.rs` | Concurrent cohorts fill all 28 source domains in a fixed market | A new sibling generation changes global epochs between admission and future-domain materialization |
| `cu/inv_028_retained_domain_episodes.rs` | New position episodes reuse already occupied same-generation domains | An unrelated market slot is retired and reactivated while existing legs still need latent domains |
| `cu/inv_077_bounded_work_and_maximum_shape_compute.rs` | Market growth at full source occupancy preserves a bounded owner reduction; source reclamation restores capacity | Append/reuse occurs before half the source domains materialize, followed by all owner payouts without claim reclamation |
| `cu/inv_089_activation_reactivation_and_initialization_equivalence.rs` | Fresh/reused persisted-slot equivalence and replacement admission at the leg cap | Sibling generation changes preserve another full portfolio's partially materialized reservation frontier and economic endpoint |

## Mounted Test

[`cu/inv_028_sibling_generation_liveness.rs`](cu/inv_028_sibling_generation_liveness.rs)
adds exactly one test:

```text
inv_028_source_domain_realizability_cap::historical_latent_capacity::sibling_generation_liveness::v16_program_sibling_generation_changes_preserve_reserved_settlement_and_owner_exit
```

The parent gains a capacity-parameterized constructor; its original constructor
continues to request fourteen market slots. The new test requests account capacity
for fifteen slots, with only fourteen configured by public InitMarket. All economic
accounts and collateral come from System/SPL/ATA/wrapper instructions. The existing
authenticated matcher fixture is created through public instructions; trades in
this increment are bilateral TradeNoCpi. Harness controls are signer SOL, Clock and
blockhashes. There are no installed or restored economic snapshots.

Eight histories cross append-only versus append/retire/reuse, mirrored alternating
long/short positions, and both account settlement/refresh/payout orders. Lot sizes
cycle through one, two and three, totaling 27 lots per portfolio. Two owners deposit
1,000,000 atoms each and open all fourteen assets. A one-atom favorable move creates
fourteen retained claims. The owners reverse their positions at those committed
prices; every leg remains active, and its opposite source domain is still latent.

The market authority appends inactive-in-the-portfolios sibling slot 14. Reuse
histories additionally retire and reactivate that empty slot at advancing Clock
slots. Each accepted configuration instruction must preserve both complete portfolio
Accounts, the original fourteen AssetStates and their source/backing records, mint,
vault and owner token Accounts. The new source credits are empty, insurance budgets
are zero, configured shape grows to fifteen slots, and asset-set epochs advance.
Activation consumes the expected next generation; reuse assigns a distinct identity.

The public refresh rank is the lexicographic pair of pending authenticated accrual
slots across active legs and the number of non-current owner certificates. Each
accepted crank strictly lowers that rank, preserves peer Account bytes and all
already settled source claims, and satisfies the input-derived accounting oracle.
The loop has a 32-attempt bound. No sibling observations are needed because neither
portfolio has exposure or claims in that slot. Once both certificates are current,
returning the original marks to 100 materializes the remaining fourteen domains.
Every domain claim must equal its input lot size times BOUND_SCALE, with no change
to earlier attribution. Both portfolios still have fourteen active legs.

Fourteen bilateral owner-authorized closing trades each lower both portfolios'
active-leg count by exactly one. Claim conversion, owner SPL withdrawals and portfolio
deletion complete through the existing public accounting helper. Each world pays
exactly 1,000,054 and 999,946 atoms, with zero final booked/raw vault custody, source
claims/backing and materialized portfolios, and unchanged mint supply. The unused
sibling remains Active with no OI, source claims or insurance entitlement.

## Coverage Limits

| Invariant | Evidence and remaining limit |
| --- | --- |
| INV-028 | Reserved future domains remain materializable after sibling shape/generation changes; exact domain credit stays bounded by attributed backing. No arbitrary-history or over-capacity result. |
| INV-057 | Each bilateral closing trade strictly reduces both owners' exposure after bounded public readiness repair. Requires both trade counterparties' signatures; unilateral reduction is not tested. |
| INV-073 | Both funded owners complete principal plus earned-claim payout and delete their portfolios. Asset/slab administrative retirement is outside this test. |
| INV-077 | Full 14-leg/28-source account shape is measured, with fifteen configured market slots. Not a maximum market-byte, hint, oracle-tail or Cartesian-product proof. |
| INV-078 | Adjacent successful public readiness continuation only. No ordinary-progress failure class or permissionless terminal fallback is triggered, so no direct new failure-class coverage is claimed. |
| INV-082 | Concrete lexicographic readiness rank and decreasing owner exposure rank on eight reachable histories. No abstract theorem or exhaustive graph claim. |
| INV-089 | Appended/reused sibling identity, empty source state and global certificate invalidation preserve existing account entitlements. No full persisted-slot differential or permissionless fee/authority envelope. |

There is no failure injection or rollback claim. Oracle outage, nonzero fees/funding,
liens, insolvency, repeated/adversarial churn, paused-refresh interleavings, changed
exposed-asset configuration and arbitrary reuse histories remain untested. The
market authority cooperates for the configuration prefix; the subsequent economic
continuation uses only the public payer and the two owners. Invariant verdicts,
holdout labels, production code and Cargo pins are unchanged.

## Validation

The final new selector passes **1/1**, covering **8 worlds and 696 accepted history
calls**. Fixture construction and initial oracle configuration are outside that
count. Post-change public repair takes at most **2 calls**, within the 32-attempt
bound. Maximum CU is **1,040,149**, below 1,375,000; lifecycle transitions peak at
16,937, trades at 807,373, conversion at 712,274, withdrawal at 44,955 and portfolio
deletion at 26,540. Custody and deletion retain their separate 300,000 CU ceiling.
The invariant charter/index selector passes **1/1**.

The combined final run passes **6/6**: the new selector plus **5/5 adjacent controls**
(historical/latent capacity, concurrent latent capacity, maximum-source owner
reduction, source-capacity reclamation and reused-slot admission). Formatting,
working/staged whitespace and committed-diff whitespace checks pass.

Development corrected a test-only reference to a field absent from AssetStateV16
and selected the dedicated append helper before public execution. The first public
conformance run passed; adding explicit exposure-rank checks also passed. No
production conformance failure was observed. Existing unused-support warnings and
the `solana-client v1.18.26` future-compatibility warning remain.

Wrapper and authenticated-matcher SBF artifacts were rebuilt with default features
using locked offline platform-tools v1.52. Their SHA-256 hashes are:

```text
wrapper  5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e
matcher  50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93
```

Build commands, from the isolated worktree (the fixture command uses its own cwd):

```sh
export TMPDIR=/dev/shm CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
env CARGO_TARGET_DIR=/dev/shm/astra-shape-liveness-20260912-target \
  RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --sbf-out-dir /dev/shm/astra-shape-liveness-20260912-target/deploy -- --locked
cd /tmp/percolator-astra-shape-liveness-20260912/tests/fixtures/auth_matcher
env CARGO_TARGET_DIR=/dev/shm/astra-shape-liveness-20260912-matcher-target \
  RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --sbf-out-dir /tmp/percolator-astra-shape-liveness-20260912/tests/fixtures/auth_matcher/target/deploy -- --locked
cd /tmp/percolator-astra-shape-liveness-20260912
export CARGO_TARGET_DIR=/dev/shm/astra-shape-liveness-20260912-host
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-shape-liveness-20260912-target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu \
  inv_028_source_domain_realizability_cap::historical_latent_capacity::sibling_generation_liveness::v16_program_sibling_generation_changes_preserve_reserved_settlement_and_owner_exit \
  -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_028_source_domain_realizability_cap::historical_latent_capacity::sibling_generation_liveness::v16_program_sibling_generation_changes_preserve_reserved_settlement_and_owner_exit \
  inv_028_source_domain_realizability_cap::historical_latent_capacity::v16_program_historical_and_latent_domains_share_bounded_settlement_capacity \
  inv_028_source_domain_realizability_cap::historical_latent_capacity::concurrent_latent_capacity::v16_program_concurrent_latent_cohorts_preserve_full_shape_settlement_and_exit \
  inv_077_bounded_work_and_maximum_shape_compute::v16_program_max_source_capacity_reclamation_restores_funded_exit \
  inv_077_bounded_work_and_maximum_shape_compute::v16_attack_max_source_owner_rebalance_reduce_stays_bounded \
  inv_089_activation_reactivation_and_initialization_equivalence::v16_program_reused_slot_rejects_fifteenth_leg_then_admits_replacement_at_cap
cargo test --locked --offline --test v16_program_fuzz_regressions \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
```

Build/test logs are `/dev/shm/astra-shape-liveness-20260912-{sbf,matcher,host,selector,controls,index}.log`.
