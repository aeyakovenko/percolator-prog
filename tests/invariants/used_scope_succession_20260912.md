# Used-scope succession coverage, 2026-09-12

Base: local committed public-harness revision
`6c107cf0e9caeecba02c6c034be62e37a5a5aa81`.
Branch: `codex/astra-capability-rows412-414-20260912`.
Worktree: `/tmp/percolator-astra-capability-rows412-414-20260912`.
The original checkout has unresolved changes and no invariant directory; it was
left untouched. Only local public code, tests, the charter and coverage notes
informed this increment. No GitHub PR/issue/branch or sealed evidence was read.

## Distinct Coverage

The selector is mounted beneath
`inv_012_capability_and_delegate_scope::joint_incarnation_binding::used_generation_lifecycle::used_scope_succession`:
`v16_program_used_scope_succession_preserves_revocation_and_current_authorized_exit`.

| Existing selector | Additional composition here |
| --- | --- |
| `v16_program_used_asset_reuse_with_live_sibling_preserves_authorized_exit` | Its matcher tuple and oracle holder stay constant, and CPI closes preserve the LP grant. Here bilateral closes revoke the grant before matcher-context/delegate succession and live-sibling oracle succession. |
| `v16_program_matcher_program_roundtrips_compose_with_asset_reuse` | It interleaves empty-slot replacement with alternate matcher programs and retained requests. Here the matcher program stays constant, both contexts execute actual exposure, and used target generations coexist with a live sibling. |
| `v16_program_retained_capability_tracks_committed_revocation_after_bundle_rollback` | It owns rollback and retained-consumer disposition; this test adds successful used-asset and observation-authority continuation after committed revocation. |
| `v16_program_retained_scope_product_separates_admin_epoch_tuple_and_expiry` | It owns isolated tuple/epoch/expiry disposition; this test combines a live oracle handoff with actual slot use, replacement, revocation and exact exit. |
| `v16_program_oracle_authority_aba_is_asset_scoped_and_rolls_back_retained_prefix` | It owns retained oracle controls; this test makes both current oracle holders publish observations used by live-portfolio refresh and subsequent authorized matcher fills. |

Eight worlds cross single/batch bilateral-close and reentry routes, both position
orientations, and oracle handoff before/after target replacement. The final CPI
exit uses the opposite transport. Batch leg order and payout order follow the
handoff-order flag; they are not additional independent dimensions.

Each world first opens asset 1 and asset 2 with separate CPI fills. Two cycles
then close only asset 2 bilaterally, immediately assert the LP grant is disabled
with zero expiry and unchanged sequence/tuple/fee cap, install an owner-signed
context grant, and replace the used asset-2 generation. The live asset-1 oracle
holder goes A-to-B-to-A across the cycles, with both parties signing each handoff.
Fresh observations and public cranks refresh the surviving exposure before the
replacement oracle is configured and the LP explicitly renews its grant again.
Context/delegate A-to-B-to-A likewise occurs without reinitializing either context.

Event-derived assertions check generations `[1, 2, 4]` then `[1, 2, 5]`, unchanged
portfolio IDs, two position-epoch advances from bilateral closes, four explicit
grant-sequence advances, two asset-local authority-epoch advances and one request
ID per committed CPI. Each close immediately checks both positions and capital;
the parent additionally checks exact matched OI, generation-bound live legs,
grant fields, zero PnL, fixed supply and engine/SPL custody. Handoffs preserve
all framed nonmarket accounts. Reuse preserves both live portfolio accounts.

Every single CPI validates all typed return fields, including current request,
delegate-derived LP identity, asset and authenticated oracle price, and preserves
context bytes outside the single-return record. Batch CPI preserves that record.
The displaced context stays byte-exact; lifecycle, control and payout operations
preserve both contexts and never advance invocation IDs. All 16 owner payouts
return exactly the original 1,000,000 collateral atoms, leaving zero OI and custody.

System/SPL/ATA/wrapper instructions construct economic accounts, and the LP
initializes both System-created honest matcher contexts through their public API.
Harness controls are program loading, signer SOL, Clock and blockhashes. There is
no economic state injection, hostile matcher, engine transition used as an oracle,
or change to shared harness, production, dependencies or invariant verdicts.

## Limits

**Rows 412 and 414 remain OPEN.** This is one bounded positive selector, not a
generic generator/oracle. Immediate revocation-state assertions do not establish
retained-consumer rejection. Explicit owner renewal after reuse does not establish
standing-grant generation confinement. No stale capability or observation replay,
unauthorized authority action, vulnerable-pin experiment, new violation, or fix
is claimed. Constant AuthMark price and zero fees/funding exclude economically
distinct observation prices, source claims, reserve liabilities and terminal
disposal. Portfolio recreation, whole-market incarnation and arbitrary histories
remain outside this increment. Oracle evidence is successful authenticated
publication and scope-local counters, not a new oracle-substitution rejection test.

## Validation

The new selector passed its first execution: **8 worlds, 16 immediate revocations,
16 replacements, 16 consensual oracle handoffs, 32 owner grants, 32 recertifying
cranks, 52 CPI fills/exits and 16 complete payouts**. Peak measured CU was
244,787 for selected writers including bilateral close, 472,333 for CPI fills,
and 143,268 for withdrawals. Enforced limits are respectively 300,000, 750,000,
and 300,000. Setup transactions are not included in these peak measurements.
No probes were discarded and no valid-history failure was observed.

The existing parent lifecycle control passed **1/1 across 16 worlds** and the
invariant index passed **1/1**. Formatting and whitespace checks passed, as did
the production/dependency/shared-harness/status-file diff guard. No broad suite
or Kani run is claimed. Existing unused-support warnings and the
`solana-client v1.18.26` future-incompatibility warning remain.

SBF artifacts are private copies of the documented matching base builds, not new
builds. Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
Wrapper SHA-256: `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
The wrapper came from `/dev/shm/astra-capability-6d2a-target/deploy/percolator_prog.so`;
the matcher came from the base checkout's ignored fixture build. The host cache
was copied into a private target, and Cargo compiled the test from this worktree.

Exact test commands, run from the worktree (the same environment is supplied to
each Cargo test with `env`):

```sh
env CARGO_TARGET_DIR=/tmp/percolator-astra-capability-rows412-414-20260912-build CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 PERCOLATOR_FUZZ_SBF=/tmp/percolator-astra-capability-rows412-414-20260912-build/deploy/percolator_prog.so cargo test --locked --offline --test v16_cu inv_012_capability_and_delegate_scope::joint_incarnation_binding::used_generation_lifecycle::used_scope_succession::v16_program_used_scope_succession_preserves_revocation_and_current_authorized_exit -- --exact --nocapture
env CARGO_TARGET_DIR=/tmp/percolator-astra-capability-rows412-414-20260912-build CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 PERCOLATOR_FUZZ_SBF=/tmp/percolator-astra-capability-rows412-414-20260912-build/deploy/percolator_prog.so cargo test --locked --offline --test v16_cu inv_012_capability_and_delegate_scope::joint_incarnation_binding::used_generation_lifecycle::v16_program_used_asset_reuse_with_live_sibling_preserves_authorized_exit -- --exact --nocapture
env CARGO_TARGET_DIR=/tmp/percolator-astra-capability-rows412-414-20260912-build CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 PERCOLATOR_FUZZ_SBF=/tmp/percolator-astra-capability-rows412-414-20260912-build/deploy/percolator_prog.so cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
rustfmt --edition 2021 tests/invariants/cu/inv_012_used_scope_succession.rs tests/invariants/cu/inv_012_used_generation_lifecycle.rs
cargo fmt --all -- --check
git diff --check
git diff --exit-code 6c107cf0e9caeecba02c6c034be62e37a5a5aa81 -- src Cargo.toml Cargo.lock tests/support tests/fixtures tests/invariants/invariant_status.tsv
git diff --cached --check
git show --format= --check HEAD
```
