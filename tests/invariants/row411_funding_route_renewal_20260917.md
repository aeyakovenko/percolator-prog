# Row 411 funded bilateral reduction and retained renewal, 2026-09-17

Base: freshly fetched `origin/main`, `70657375a15fba4dceb7085b3b2f389ed258f38f`.
Private worktree: `/dev/shm/astra-row411-retained-consent-20260917`.
Branch: `astra-row411-retained-consent-20260917`; local commit only.
Primary INV-014. Row 411 remains OPEN; no production correction or TSV change.

## Distinct Coverage

Reviewed `row411_inter_reduction_funding_20260917.md`,
`row411_split_funding_routes_20260917.md`,
`row411_funding_collection_audit_20260917.md`,
`row411_followup_health_20260917.md`, their mounted tests, the generated
partial-policy tests, retained policy-route budgets, mixed-route fees and retained
fee-expiry history. The funding histories keep the first closing fill on CPI and
never renew a grant after a bilateral reduction. The mixed-route history renews
separately with zero funding and maintenance. The expiry witness also holds
funding at zero. None tests an atomic retained renewal plus a funded residual
whose cap is one atom below the independently computed fee.

The new child `cu/inv_014_retained_funding_route_renewal.rs` reuses unchanged
public construction, complete-account rollback and input-derived fee/funding
ledger functions. Its parent changes only by adding the module mount. Four
worlds cross both position directions with single/batch bilateral reductions;
the final route is batch CPI with one leg and an aggregate quote-atom cap.

1. A real matcher partial opens 95 of 255 units at price 100 and 37 bps. Each
   owner pays 36 atoms. Before policy/clock changes, the owners sign the bilateral
   reduction, future-epoch residual, grant renewal and all alternative envelopes.
   Renewal binds the future LP position epoch; the residual binds the next
   matcher sequence.
2. The 71-bps policy rejects the retained 37-bps bilateral reduction. Restoring
   37 bps commits 57 units of reduction, charging 22 atoms per owner, collecting
   95 funding atoms and maintenance, and revoking the LP grant without advancing
   its matcher sequence. The remaining position is exactly 38 units.
3. Public EWMA reports and observed cranks leave another negative-rate interval
   pending at slot 4. A residual with the current sequence and future position
   epochs rejects `Unauthorized` because the bilateral fill disabled the grant.
4. At 71 bps, renewal executes and the matcher returns, but the retained 15-atom
   aggregate cap rejects. The LP grant and leg both permit 137 bps; pending
   funding of 38 atoms exceeds even the 27-atom repriced fee. Funding gains cannot
   offset gross fee consent. At restored 37 bps, a separately retained cap of 14
   rejects after renewal and matcher execution. The successful alternative differs
   only by one cap atom and its pre-signed compute-budget envelope.
5. The exact 15-atom alternative executes renewal, funding collection and close
   before an obsolete policy-sequence suffix rejects `EngineStale`. Logs require
   both wrapper prefixes and the matcher to succeed. Complete tracked and compiled
   Accounts roll back, including the disabled grant, expiry, sequence, funding
   indices, maintenance, matcher context, position epochs and SPL custody. Only
   the separate payer's exact signature fee is excluded. A nonmutating simulation
   and then a separately pre-signed envelope commit that same renewal and close.
   Retained consumed-renewal and consumed-close retries both reject exactly.
6. Funding settles as 95 + 38 = 133, with two index/epoch advances; gross trade
   fees total 36 + 22 + 15 = 73 per owner. Grant identity, 137-bps ceiling and
   expiry return; its sequence advances exactly once. The taker's grant sequence
   is unchanged. Position epochs advance twice, matcher requests once after the
   opening, policy sequence four times and oracle-observation sequence four times.
7. Slot-5 public maintenance sync, observed recertification and conversion release
   all 133 funding atoms. Full withdrawals pay `[98375,198532]` for the short
   taker or `[98641,198266]` for the long taker. Both bilateral routes leave 3,216
   atoms in custody, insurance domains `[1605,1611]`, and no owner capital/PnL/OI.

The independent ledger checks capital, PnL, per-call fee rounding, maintenance
cursors, insurance domains, custody, fixed mint supply and stock/encumbrance
censuses throughout. Complete token Accounts are compared against an initial
snapshot with only the input-derived balance changed. Every framed unrelated
account is unchanged on successful calls; rejection frames compare all Accounts.
All retained serialized transactions and signatures remain unchanged. Distinct
pre-signed CU envelopes avoid retrying a transaction signature already recorded
by LiteSVM. All accounts use normal public System/SPL/ATA/wrapper construction;
no program-owned bytes are injected or mutated.

## Verification

One new exact selector and two relevant controls PASS; no broad suite was run.
The new selector checks four worlds, eight successful nonmutating simulations,
28 exact rollbacks, 12 committed fills and eight full payouts.

| Selector below | Peak CU |
| --- | ---: |
| `$F::retained_funding_route_renewal::v16_retained_funded_bilateral_reduction_and_renewal_preserve_residual_aggregate_cap` | 461,554 |
| `$F::v16_retained_residual_bounds_fees_after_inter_reduction_funding_accrual` | 470,288 |
| `$P::retained_single_cpi_policy_history::retained_mixed_route_fees::v16_retained_mixed_route_fee_budgets_survive_bilateral_revocation_and_renewal` | 226,720 |

The new test keeps the existing 500,000-CU ceiling. These are observed peaks,
not maximum-shape evidence. Two development runs exposed incorrect test-side
final-state expectations: matcher config contains the advancing position epoch,
and four public mark reports advance the oracle-observation sequence. Correcting
those expectations preserved all economic and rollback assertions; neither was
a production mismatch.

Wrapper, hostile matcher and auth matcher were rebuilt locked/offline from this
worktree with platform-tools v1.52. Host/SBF caches were copied without hard links
from `/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target`; subsequent
builds use private targets. Engine: `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
Artifact SHA-256:

- Wrapper: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
- Hostile matcher: `e0c20fad34a7822cc6ce42a3c77ff08a8591977102f0c497a339d66a9dd6240a`.
- Auth matcher: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

```bash
R=/dev/shm/astra-row411-retained-consent-20260917
cd "$R"
env CARGO_TARGET_DIR="$R-sbf-target" RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_BUILD_JOBS=2 cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$R-sbf-target/deploy" -- --locked
env CARGO_TARGET_DIR="$R-matcher-target" RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_BUILD_JOBS=2 cargo build-sbf --manifest-path tests/fixtures/hostile_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/hostile_matcher/target/deploy -- --locked
env CARGO_TARGET_DIR="$R-auth-target" RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_BUILD_JOBS=2 cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked
export CARGO_TARGET_DIR="$R-host-target"
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
export PERCOLATOR_FUZZ_SBF="$R-sbf-target/deploy/percolator_prog.so"
P=inv_014_delayed_policy_and_policy_epoch_safety
F=$P::retained_partial_fee_routes::generated_partial_policy_words::retained_partial_authority_routes::retained_funding_collection
cargo test --locked --offline --test v16_cu "$F::retained_funding_route_renewal::v16_retained_funded_bilateral_reduction_and_renewal_preserve_residual_aggregate_cap" -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu "$F::v16_retained_residual_bounds_fees_after_inter_reduction_funding_accrual" -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu "$P::retained_single_cpi_policy_history::retained_mixed_route_fees::v16_retained_mixed_route_fee_budgets_survive_bilateral_revocation_and_renewal" -- --exact --nocapture --test-threads=1
rustfmt --check --edition 2021 --config skip_children=true tests/invariants/cu/inv_014_retained_funding_collection.rs tests/invariants/cu/inv_014_retained_funding_route_renewal.rs
git diff --check
git diff --cached --check
git diff --exit-code 70657375a15fba4dceb7085b3b2f389ed258f38f -- . ':(exclude)tests/invariants/cu/inv_014_retained_funding_collection.rs' ':(exclude)tests/invariants/cu/inv_014_retained_funding_route_renewal.rs' ':(exclude)tests/invariants/README.md' ':(exclude)tests/invariants/row411_funding_route_renewal_20260917.md'
git show --format= --check HEAD
```

Formatting, whitespace, committed-patch and exact four-path scope checks pass.
Production, Cargo inputs, tracked fixtures, shared helpers, `tests/v16_cu.rs`, TSV
ledgers and Row 417/421/424/428/433 test owners are unchanged. Build/test logs are
untracked siblings of the worktree; the passing selector log ends `-new-pass.log`.

## Remaining Gaps

Bounded Live/SPL, one asset, base fees and two negative-rate intervals only.
Positive funding, multi-asset aggregate caps, arbitrary ratios/intervals,
clipping/underfunding, dynamic/backing/redirect fees, grant expiry, repeated
renewals, Recovery/Resolved, alternate quote rails and maximum shapes remain
open. This does not close Row 411 or establish general INV-081 validity.
