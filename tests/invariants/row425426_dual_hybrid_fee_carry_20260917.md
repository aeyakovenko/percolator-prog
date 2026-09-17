# Rows 425/426: Dual Hybrid Carry With Trading Fees

Base: `origin/main`, `58f32546e52de398d90d741ffc0706e6fb5d181f`.
Worktree: `/dev/shm/astra-ultra-row425426-observation-conformance-20260917`.
Test/docs-only defensive coverage for INV-020/045/053/056. No production issue
or invariant-status promotion is claimed.

The new selector is mounted under `chunked_observation_admission` to reuse its
public System/SPL portfolio funding and complete Account snapshot helpers.
Unlike the existing maximum-prefix stale-report test, this uses two Hybrid
assets with different anchors, independently timed whole-price crossings, and
nonzero fees on two-asset batch reductions. It also differs from the existing
fixed-price dual-Hybrid maintenance test and single-Hybrid reward/carry test.

Both owners publicly deposit 1,000,000 atoms; SPL mint authority is then revoked.
The first owner opens +400/-700 lots at 100/125, with opposite peer positions.
Reports stage 120/100 at slot 0. Each checked slot reduces both exposures by
100 lots at the independently calculated effective prices and charges 37 bps.

| Slot | Effective Prices | Carry Numerators | Gross Owner Profit | Fees Per Owner |
| --- | --- | --- | --- | --- |
| 4 | 100 / 124 | 9,600 / 2,000 | 700 | 83 |
| 5 | 101 / 124 | 2,000 / 5,000 | 1,000 | 167 |
| 7 | 101 / 123 | 6,800 / 1,000 | 1,500 | 251 |

Forward/reverse complete observation orders agree on each owner's capital,
PnL and full certificate. Final values are 1,001,249 / 998,249, with 502 atoms
of insurance fees and unchanged 2,000,000-atom custody. Funding stays zero.
The raw-state independent certificate model checks all health lanes and epochs;
input-derived arithmetic separately checks price, carry, value, fees and OI.

At each slot, a bundle completes current refresh and both paid reductions,
then an over-budget admission rejects with `EngineLockActive` at instruction 4.
Its signed epochs account for the preceding batch. A second probe observes the
asset crossing a whole atom but omits the other active Hybrid's current evidence;
it rejects with `EngineNonProgress`. Every rejection compares complete tracked
and compiled-key `Option<Account>` values, including account absence, with the
fee payer adjusted by exactly the compiled signature fee. Rejection packets
are at most 1232 bytes. The same refresh/reduction prefix subsequently succeeds.

Result: **1 passed, 0 failed, 1,473 filtered**, 1.19s; two histories, six admission
suffix rollbacks, six incomplete-evidence rollbacks, 18 independent certificates.
Peak successful CU: **refresh 178,819; paid batch reductions 343,572**.
Guardrails: 500,000 / 600,000 CU. No other selector or broad suite ran.

Host/SBF caches were privately copied from the `percolator-public-gap-20260916-c91e`
targets; current SBF was rebuilt offline with platform-tools v1.52 and `--locked`.
Engine pin: `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
Wrapper SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
No matcher is used. Logs are the worktree path plus `-sbf.log` / `-test.log`.

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row425426-observation-conformance-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-ultra-row425426-observation-conformance-20260917-sbf-target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu inv_020_authenticated_clock_slot_and_oracle_provenance::chunked_observation_admission::dual_hybrid_fee_carry::v16_program_dual_hybrid_carry_frontiers_preserve_fee_debits_and_current_health -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_020_chunked_observation_admission.rs tests/invariants/cu/inv_020_dual_hybrid_fee_carry.rs
git diff --check
git diff --cached --check
```

Scope remains sampled: fixed targets, zero funding/maintenance, distinct owners,
live solvent portfolios, and bilateral batch reductions. No liquidation/reward,
common-control, terminal payout or reserve claim is made. Clock and external
provider reports are fixtures; all protocol and custody construction and actions
use public instructions. Production, dependencies, tracked fixtures, TSVs,
Row422, Row427 and terminal/reserve paths are unchanged.
