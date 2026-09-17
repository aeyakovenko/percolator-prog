# Row 411 retained two-asset funding consent, 2026-09-17

Base: `origin/main`, `5470d70762337f11dc94210434b6a6e036d297f0`.
Worktree: `/dev/shm/astra-ultra-row411-route-funding-conformance-20260917`.
Branch: `astra-ultra/row411-route-funding-conformance-20260917`.
Local commit only; leave the worktree for parent integration. Tests/docs only.
Primary INV-014; bounded INV-011/024/047/081 evidence. Row 411 remains OPEN.

## Distinct coverage

Read the funding-renewal, split-funding, funding-collection, followup-health and
fee-policy-route audit notes, relevant README entries and `scripts/loop.md` first.
Compared the mounted funding, partial-authority, mixed-route and policy-budget
histories, plus INV-011 aggregate-cap and INV-047 two-asset fee witnesses. Existing
retained funding histories have one asset; the existing multi-asset fee witnesses
have no moving funding. This adds two funding domains to retained route consent,
including a per-leg rounding boundary that a pooled notional would miss.

One child module reuses the unchanged public `World`, exact rollback frame and
input fee/funding arithmetic. The parent changes only by mounting this child.
Two worlds compare batch CPI and bilateral batch closes:

1. A public batch-CPI opening creates short taker positions of 95 and 57 units
   at price 100 and 37 bps. Each owner pays `36 + 22 = 58` atoms. Both closing
   routes, retries, late-failure alternatives and the CPI 57-atom cap are signed
   before any fee-policy or funding movement. The LP grant permits 137 bps.
2. The public policy rises to 71 bps. Public per-asset EWMA reports at slots 1
   and 2 and observed cranking create two negative-rate funding intervals, one
   per asset. Circuit breakers keep execution at 100. Signed floor rounding
   transfers 95 and 57 atoms from short to long when the close collects funding.
3. Retained consent rejects the high policy exactly on each route. CPI reaches
   its matcher before aggregate-cap rejection; bilateral consent rejects before
   execution. Funding gains exceed the repriced gross fees but cannot fund consent.
4. Restoring 37 bps still rejects a retained CPI cap of 57, the ceiling computed
   from the combined notional. The correct sum of per-leg ceilings is 58. That
   exact alternative successfully closes both assets before a superseded policy
   suffix rejects `EngineStale`. Complete Accounts roll back, including indices,
   portfolios, grant state, matcher context and SPL custody; the independent
   payer loses only the exact network signature fee.
5. A separately pre-signed envelope commits the original close. Its consumed
   retry rejects exactly. Both funding indices advance once, the funding epoch
   advances twice, and each position epoch advances once. CPI advances the
   matcher request and preserves its grant; bilateral execution revokes the
   grant without advancing the request. Grant sequences stay unchanged.
6. Slot-3 observed recertification and conversion release all 152 funding atoms.
   Both routes pay owners exactly `[99848,200043]`, charge 116 total trade-fee
   atoms per owner, and leave custody 232 with domains `[72,72,44,44]`. Capital,
   PnL, positions and OI are zero after full withdrawal. Fixed mint supply,
   complete token Accounts, stock and encumbrance censuses are checked throughout.

All serialized retained transactions and signatures remain unchanged. Distinct
pre-signed compute-budget envelopes distinguish retries recorded by LiteSVM.
System/SPL/ATA/wrapper instructions construct and fund accounts; matcher setup
uses its public instructions. No program-owned bytes are injected or modified.

## Verification

PASS: one exact selector, two worlds, seven exact rollbacks, eight committed leg
fills and four full payouts. Observed scenario peak: **470,497 CU**, below the
unchanged 500,000-CU ceiling. No broad suite or extra control was needed: existing
helper bodies are unchanged and both transports execute in the new selector.

Wrapper and hostile matcher rebuilt locked/offline with platform-tools v1.52.
Private caches were copied without hard links from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target`.
Engine pin: `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
SHA-256:

- Wrapper: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
- Matcher: `e0c20fad34a7822cc6ce42a3c77ff08a8591977102f0c497a339d66a9dd6240a`.

```bash
R=/dev/shm/astra-ultra-row411-route-funding-conformance-20260917
cd "$R"
env CARGO_TARGET_DIR="$R-sbf-target" RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_BUILD_JOBS=2 cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$R-sbf-target/deploy" -- --locked
env CARGO_TARGET_DIR="$R-matcher-target" RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_BUILD_JOBS=2 cargo build-sbf --manifest-path tests/fixtures/hostile_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/hostile_matcher/target/deploy -- --locked
export CARGO_TARGET_DIR="$R-host-target"
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
export PERCOLATOR_FUZZ_SBF="$R-sbf-target/deploy/percolator_prog.so"
P=inv_014_delayed_policy_and_policy_epoch_safety::retained_partial_fee_routes::generated_partial_policy_words::retained_partial_authority_routes::retained_funding_collection
cargo test --locked --offline --test v16_cu "$P::retained_multi_asset_funding::v16_retained_two_asset_funding_preserves_cpi_and_bilateral_fee_consent" -- --exact --nocapture --test-threads=1
rustfmt --check --edition 2021 --config skip_children=true tests/invariants/cu/inv_014_retained_funding_collection.rs tests/invariants/cu/inv_014_retained_multi_asset_funding.rs
git diff --check
git diff --cached --check
git diff --exit-code 5470d70762337f11dc94210434b6a6e036d297f0 -- . ':(exclude)tests/invariants/cu/inv_014_retained_funding_collection.rs' ':(exclude)tests/invariants/cu/inv_014_retained_multi_asset_funding.rs' ':(exclude)tests/invariants/README.md' ':(exclude)tests/invariants/row411_multi_asset_funding_conformance_20260917.md'
git show --format= --check HEAD
```

Build/test logs are untracked siblings of the worktree; the passing log ends
`-new6.log`. Development corrected redundant asset activation, index types and
grant-sequence expectations. A two-single-CPI bundle failed before its late
suffix under this harness; the final comparison uses two batch transports.
No production mismatch or general single-route liveness claim is established.

Remaining gaps: positive funding, mixed signs, clipping/underfunding, other fee
sources, maintenance, grant expiry/renewal, arbitrary histories, Recovery/Resolved,
alternate quote rails and maximum shapes. This is bounded Live/SPL evidence;
it does not establish general INV-081 validity or change any invariant status.
Production, Cargo inputs, fixtures, shared helpers, the top-level test harness,
TSV ledgers and excluded row owners remain unchanged.
