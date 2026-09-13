# PR135 Scope H conformance, 2026-09-13

Branch: `codex/pr135-scope-h-carry-entitlement-20260913`.
Clean worktree: `/home/anatoly/worktrees/pr135-scope-h-carry-entitlement-20260913`.
Fetched base: `origin/codex/astra-open-holdout-ledger-20260912` at
`bba6bab44a92ef647a49dcd9fad2cb29febfd333`.
Only this base's invariant docs/status ledgers and current code were consulted.
No open PR diffs or test sources were inspected or copied. The original
checkout's existing changes were left intact.

## Probe And Oracle

[The new probe](cu/inv_045_target_arrival_entitlement.rs) reuses the current
public market constructor and resolved payout helper. System/SPL/ATA/wrapper
instructions create all economic accounts and positions. Public slot-zero
trades align each owner's two price exposures, and SPL revokes mint authority
at a fixed 1,000,066-atom supply. Only program loading, signer SOL, Clock and
blockhashes are harness inputs; no economic account bytes are installed.

Four histories cross both AuthMark directions with two equivalent schedules:
grouped accrual (up to four slots) plus whole two-asset bilateral reductions,
and one-slot accrual plus reversed single-asset, half-sized reductions. Active
owners start at signed lots `[13,-17]` / `[-13,17]`; passive owners hold
`[7,-11]` / `[-7,11]`. Anchors are 100/125 and the cap is 24 bps per slot.

Reductions execute at slots 5/8/11/14/17. Each removes two lots per asset,
except asset 1 removes four at slots 8/14. At slot 5, after both prices have
moved, targets change to one further atom. Same-target reports at slot 8
preserve carries 7,200/9,000. Arrival clears excess cap capacity; both assets
plateau through slot 11 with zero carry and anchors equal to their reached
prices. Another same-target report and a new farther target then precede
renewed accrual. Reports at slots 14/17 preserve the newly accrued carry.

The independent `BigUint` oracle computes a whole episode's linear capacity,
quotient and remainder from input anchors, publication slots, cap and target
distance. It does not call the deployed one-slot arithmetic or seed expected
values from decoded prices, indices, carries or payouts. A target change resets
the episode's carry; arrival changes the next episode's cap anchor. The owner
book accumulates signed input lots times each price change before later fills
reduce exposure. Decoded checkpoints reconstruct observed latent value, which
must reconcile with each independently predicted owner amount.

Every committed history action checks effective/raw prices, cap anchor/carry,
K, zero F/B, unit ADL, matched OI, zero pending obligations, individual value,
capital/positive-PnL totals and fixed supply/custody. Reports frame every owner
and the other oracle profile; trades frame both complete oracle profiles and
passive portfolios. One passive owner's complete Account is unchanged for the
entire Live history, retaining nonzero latent entitlement until resolution.

Fifty-four failures execute a real publication or reduction before an invalid
wrapper suffix. Exact error position and success logs establish execution of
the prefix. All tracked and compiled Accounts, including metadata and absence,
restore exactly except the independently calculated payer signature fee. The
identical economic instruction then retries successfully with a fresh blockhash.

Final active lots are `[3,-3]` / `[-3,3]`. Carry is `[4112,8288]` for direction
-1 and `[4688,7712]` for +1, distinguishing resumed anchors from the initial
100/125 anchors. Each active owner's cumulative PnL is +/-60; passive PnL is
+/-54. These differ from applying final positions to the endpoint price change.
Resolution and bounded owner-signed closes pay exactly:

| Direction | Owner 0 | Owner 1 | Owner 2 | Owner 3 |
| --- | ---: | ---: | ---: | ---: |
| -1 | 99943 | 200069 | 299963 | 400091 |
| +1 | 100063 | 199949 | 300071 | 399983 |

Both schedules agree per owner and return the complete fixed supply, leaving
zero vault, capital, positive PnL, insurance, provider earnings and OI.

## Classification And Limits

This is net-new bounded target-arrival/plateau/resumed-anchor evidence for
INV-045/038/052/085/086 and row 425. Existing generated fractional routes hold
targets fixed. Existing target-reversal evidence changes a target after a
price move but does not reach the target, plateau, resume from its anchor and
complete per-owner payouts. The overlapping-checkpoint probe replaces targets
before the first price atom. Their implementations are unchanged.

Row **425 remains OPEN**. INV-045 remains `REFUTED_CURRENT`; INV-038/052/085/086
remain `OPEN_EVIDENCE`. No production change, independent discovery, generic
closure, wide-operand arithmetic proof or Kani/host/SBF equivalence theorem is
claimed. This finite two-asset, classic-SPL, solvent AuthMark family uses
integral aligned exposures, zero fees/funding and unit ADL. Fractional K/F,
losses consuming prior positive source claims, arbitrary target queues,
pending funding checkpoints, CPI, trade-driven discovery, inline accrual,
other oracle modes, bankruptcy, Recovery and physical account deletion are
outside this increment.

Initial local iterations corrected an oracle assumption that cap anchors
persist after arrival. A mixed-sign prototype also produced a one-atom owner
difference with source-credit consumption outside this carry-only model; that
composition remains unclassified here. The retained fixture's aligned exposures
exclude consumption of earlier positive claims. Neither prototype result
establishes a production conformance violation.

## Validation

New exact selector: PASS, 1 test; four histories, 148 ledger checks, 54 complete
rollbacks, 16 exact SPL payouts, peak 412,814 CU below the 600,000-CU bound.
Related target-reversal selector: PASS, 1 test, eight histories, eight complete
suffix rollbacks, peak 221,400 CU. Exact selector inventory: two tests, zero
benchmarks. Both INV-079 metadata gates: PASS, 1 test each. Formatter and Git
whitespace checks pass. Existing dead-code and Solana future-compatibility
warnings remain; no broad suite or engine proof was run.

Default-feature SBF was freshly rebuilt from this worktree, locked/offline with
platform-tools v1.52 and engine `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
A private copy of an existing Cargo dependency/artifact cache seeded the target;
the wrapper and test binary were rebuilt from current source. SBF SHA-256:
`71833878f8d52373e501e70ed21d88495051695f2533b4c55362ae90e737aa9d`.

Exact commands, from the isolated worktree:

```sh
export CARGO_TARGET_DIR=/dev/shm/pr135-scope-h-20260913-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu -- --exact --list inv_045_no_free_mark_movement::public_carry_order::target_arrival_entitlement::v16_program_target_arrival_plateau_and_restart_preserve_interleaved_owner_entitlement inv_045_no_free_mark_movement::v16_program_fractional_target_reversal_commutes_with_neutral_reduction
cargo test --locked --offline --test v16_cu inv_045_no_free_mark_movement::public_carry_order::target_arrival_entitlement::v16_program_target_arrival_plateau_and_restart_preserve_interleaved_owner_entitlement -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_045_no_free_mark_movement::v16_program_fractional_target_reversal_commutes_with_neutral_reduction -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --quiet --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming -- --exact --quiet --test-threads=1
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
```
