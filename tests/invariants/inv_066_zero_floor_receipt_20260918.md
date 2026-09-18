# INV-066 zero-floor receipt (2026-09-18)

Classification: nonduplicate public-route coverage for INV-066/029/038, with
INV-025/067/068 checks. No production finding or invariant status promotion.
Base: fetched `origin/main`, `617e5d155c00df22c90ca3889790479c1117f52f`.
Worktree: `/dev/shm/percolator-astra-claims-20260918`. No open PR data inspected.

The new CU selector is
`inv_066_resolved_payout_fairness_and_order_independence::v16_program_zero_floor_receipt_preserves_other_claimant_entitlement`.
Public trades at 100, an authenticated final mark of 200, and quantities
`POS_SCALE/100` and `3*POS_SCALE/100` create exact faces `[1, 3]`.
Two debtor atoms and one expired backing atom fund junior floors `[0, 2]`.
Both claimant orders must pay `[100, 102, 0]` including principal, preserve
the four-atom denominator and exact bound replacement, and leave one rounding
atom after all three portfolios are deleted. A first claimant retains its
receipt, including the zero-paid positive face; the last claimant immediately
clears its terminal receipt. Zero-due retries cannot recreate paid entitlement.

Input-derived owner floors and total custody are independent of observed payout
rates. The shared raw-header/complete-portfolio stock census runs through the
trade, settlement and payout history; receipt creation frames the peer portfolio.
The existing many-small-claims test injects PnL and checks aggregate bounds; the
late-receipt matrices pay every positive face at least one atom. This witness
specifically exercises positive-face/zero-junior-payment receipt replacement.
It does not duplicate returned receipt surplus or owner-local withdrawals.

Limits: standard CU account/token endowments, public economic transitions, one
classic SPL rail, two fixed faces and two orders. No mint-supply, arbitrary
history, unexpired source progress, or CloseSlab claim. Production is unchanged.

Validation: new selector **1 passed, 0 failed**, two worlds, peak **141,082 CU**
(500,000 ceiling); unchanged adjacent selector **1 passed, 0 failed**. Scoped
rustfmt and diff whitespace checks pass. Private host target copied from the
main-recorded cache. Reused SBF SHA-256:
`a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
Production sources and Cargo files match recorded build base `2e88c39b`.
No SBF rebuild. Exact commands, run from the worktree:

```sh
export TMPDIR=/dev/shm CARGO_TARGET_DIR=/dev/shm/percolator-astra-claims-20260918-target
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-inv077/target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu inv_066_resolved_payout_fairness_and_order_independence::v16_program_zero_floor_receipt_preserves_other_claimant_entitlement -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_066_resolved_payout_fairness_and_order_independence::v16_program_late_receipt_materialization_preserves_snapshot_entitlements -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --check --config skip_children=true tests/invariants/cu/inv_066_resolved_payout_fairness_and_order_independence.rs
git diff --check
git diff 2e88c39b HEAD -- src Cargo.toml Cargo.lock
sha256sum /dev/shm/percolator-inv077/target/deploy/percolator_prog.so
```
