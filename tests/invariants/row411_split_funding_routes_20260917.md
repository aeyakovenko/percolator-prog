# Row 411 retained split funding close, 2026-09-17

Base: freshly fetched `origin/main`, `2132f2b866ab2eca6f4d5ce7d81dfec5a85c7bcc`.
Branch: `codex/row411-fee-composition-20260917-200619-EEjA3e`.
Worktree: `/dev/shm/row411-fee-composition-20260917-200619-EEjA3e/worktree`.
Local commit only. Primary INV-014; related INV-005/010/011/024/036/047/081.
No production correction was needed. Row 411 remains OPEN; no TSV was changed.

## Why Keep This History

Inspected the existing row411 funding, partial-authority and regression-health
notes, their mounted tests, generated partial-policy histories, and the retained
mixed-route fee history and README notes before implementation.

- The existing funding history opens with a partial fill, then closes the entire
  position in one fill. It does not retain a funded position after collection or
  exercise another fee-bearing close after funding has already settled.
- The mixed-route history composes reductions, fee policies and grant renewal,
  but has zero funding and maintenance and no matcher-selected partial fill.
- Generated partial-policy and partial-authority histories also hold funding and
  maintenance at zero. The maintenance-reward history has no pending funding.

This increment combines a committed matcher partial close, settled funding,
independently rounded closing fees, and a retained residual through four routes.
It uses the existing funding history runner and ledger, adding exactly one test
selector in the same Rust file. The existing selector still exercises its original
full-close path. No module mounts or fixture changes were needed.

The active row421 checkout concerns terminal insurance/custody continuity; this
history remains Live and uses owner withdrawals. No terminal insurance witness
or row421-owned test file is touched.

## Economic And Authorization Checks

Eight worlds cross both position signs with single/batch CPI/no-CPI residuals.
Public System/SPL/ATA/wrapper/matcher instructions create and fund normal accounts.
There is no program-owned account injection or byte mutation. Clock movement and
public EWMA reports reproduce the existing one-slot negative funding scenario.

1. A matcher executes 95 of 255 units at price 100 and 37 bps, charging each owner
   36 atoms. Both closing intents and their alternative transaction envelopes are
   signed before authority A -> B -> A, policy changes and funding accrual. The
   residual explicitly signs the future position epochs after one more fill.
2. At 71 bps the retained 37-bps partial close rejects exactly. Public restoration
   to 37 bps permits the same pre-signed instruction to execute 57 of 95 units.
   It charges 22 atoms per owner, collects all 95 funding atoms and the outstanding
   maintenance, and leaves 38 position units. Both fee cursors reach slot 2.
   Consumed-episode retry rejects without another funding or fee debit.
3. Repricing to 71 bps rejects the retained 38-unit residual on every route.
   Batch CPI reaches its matcher before its 15-atom aggregate cap rejects; its
   137-bps leg term and LP grant remain permissive. The other routes retain
   37-bps consent. Funding gains cannot authorize extra gross trade fees.
4. After restoring 37 bps, the residual succeeds inside a transaction whose
   obsolete-authority policy suffix fails. Its policy sequence is still future.
   Complete tracked and compiled Accounts roll back, preserving the committed
   partial, funding settlement and maintenance cursors. A separately pre-signed
   residual then commits for 15 atoms per owner; its consumed retry rejects.
5. The input-derived ledger checks each checkpoint's capital, PnL, fee cursors,
   remaining position, OI, domain insurance, custody, fixed mint supply and stock
   and encumbrance censuses. Trade fees total `36 + 22 + 15 = 73` per owner.
   The two close ceilings total 37, one atom more than a single full close's 36.
   Funding is exactly 95 once; maintenance is independently 921 per owner by exit.
6. Public slot-3 synchronization, observed recertification and conversion release
   the winner's full 95 funding atoms. Both owners withdraw completely: short
   taker `[99027,199108]`, long taker `[99217,198918]`. Every route leaves exactly
   1,988 insurance atoms in custody, with domain budgets `[992,996]`.

Full signed wire images remain unchanged. Successful-program log counts prove
the late-failing transaction executed its close. Rollback frames include matcher
context, policies, epochs, request sequences and complete token Accounts; only
the separate payer's exact network signature fee is excluded. Distinct pre-signed
CU-limit envelopes distinguish retries; this is not a claim that LiteSVM accepts
the same already-recorded failed signature again.

## Verification

Both exact selectors PASS; no broad suite was run and no development run failed.

| Selector suffix under `P` below | Worlds | Rollbacks | Fills | Payouts | Peak CU |
| --- | ---: | ---: | ---: | ---: | ---: |
| `v16_retained_split_close_bounds_fees_and_collects_funding_once_across_routes` | 8 | 40 | 24 | 16 | 460,904 |
| `v16_retained_partial_close_separates_funding_and_maintenance_from_fee_consent` | 8 | 24 | 16 | 16 | 457,383 |

Each selector also has eight successful, nonmutating admissibility simulations.
Both remain below the 500,000-CU per-transaction ceiling. Peaks are observed run
values, not a maximum-shape claim.

Private host/SBF caches were copied from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target`; all subsequent
writes used this task's private directories. Wrapper and hostile matcher were
rebuilt locked/offline from this worktree using platform-tools v1.52. Engine pin:
`4db11a8cb0053815e23a35d3a7d3edc265d8d866`. SHA-256:

- Wrapper: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
- Hostile matcher: `e0c20fad34a7822cc6ce42a3c77ff08a8591977102f0c497a339d66a9dd6240a`.

```bash
R=/dev/shm/row411-fee-composition-20260917-200619-EEjA3e
cd "$R/worktree"
env CARGO_TARGET_DIR="$R/sbf-target" RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_BUILD_JOBS=2 cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$R/sbf-target/deploy" -- --locked >"$R/sbf-build.log" 2>&1
env CARGO_TARGET_DIR="$R/matcher-target" RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_BUILD_JOBS=2 cargo build-sbf --manifest-path tests/fixtures/hostile_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/hostile_matcher/target/deploy -- --locked >"$R/matcher-build.log" 2>&1
export CARGO_TARGET_DIR="$R/host-target"
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
export PERCOLATOR_FUZZ_SBF="$R/sbf-target/deploy/percolator_prog.so"
P=inv_014_delayed_policy_and_policy_epoch_safety::retained_partial_fee_routes::generated_partial_policy_words::retained_partial_authority_routes::retained_funding_collection
cargo test --locked --offline --test v16_cu "$P::v16_retained_split_close_bounds_fees_and_collects_funding_once_across_routes" -- --exact --nocapture --test-threads=1 >"$R/split-close-development.log" 2>&1
cargo test --locked --offline --test v16_cu "$P::v16_retained_partial_close_separates_funding_and_maintenance_from_fee_consent" -- --exact --nocapture --test-threads=1 >"$R/full-close-control.log" 2>&1
rustfmt --edition 2021 --config skip_children=true tests/invariants/cu/inv_014_retained_funding_collection.rs
rustfmt --check --edition 2021 --config skip_children=true tests/invariants/cu/inv_014_retained_funding_collection.rs
git diff --check
git diff --cached --check
git diff --exit-code 2132f2b866ab2eca6f4d5ce7d81dfec5a85c7bcc -- . ':(exclude)tests/invariants/cu/inv_014_retained_funding_collection.rs' ':(exclude)tests/invariants/README.md' ':(exclude)tests/invariants/row411_split_funding_routes_20260917.md'
git show --format= --check HEAD
git status --porcelain
```

Formatting, whitespace, committed-patch and protected-path checks pass. The
protected-path guard permits only the one Rust file, README and this note, so it
also protects production, Cargo inputs, fixtures, all TSVs and row421 tests.
Long logs and build artifacts remain outside tracked files in `R`.

## Remaining Gaps

This is bounded Live, one-asset, base-fee evidence with one pending negative-rate
funding interval and two same-slot closing fills. Positive funding rates, funding
accrual between reductions, arbitrary history lengths/partial ratios, multi-asset
aggregate caps, dynamic/backing/redirect fees, clipping and underfunding, grant
renewal/expiry, Recovery/Resolved, alternate quote rails and maximum shapes remain
open. It does not prove generic row411 closure or general INV-081 validity.
