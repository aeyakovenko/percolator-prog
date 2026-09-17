# Resolved Source Residual Entitlement

Branch: `codex/invariant-source-residual-20260916-c8d4`.
Worktree: `/dev/shm/percolator-source-residual-c8d4`.
Initial remote main: `635007c7` (the INV-017 resolved owner/destination alias).
Final integration base: `cc9d747d90c3fbc5056dcb96f2cb1269fd856bd3`.
Pinned engine: `94979ede7db934545e53a8f210dd063a9ea3ea63`.

Only supplied holdout labels, current main, and its pinned dependency were used.
No open PR branch, diff, implementation or reproduction was read or copied. No
other agent's worktree was read or modified. Builds and the mutation copy are
private to this worktree. Production source, manifests, lockfile, finding labels
and invariant statuses are unchanged.

## Coverage Gap

The new public witness is
`inv_067_terminal_payout_completeness_and_exact_once_settlement::resolved_source_residual::v16_program_late_debtor_source_cap_preserves_receipt_residual_entitlement`.

| Existing Main Owner | Boundary Already Covered | New Distinction |
| --- | --- | --- |
| INV-067 `late_expiry::World`, source realization, fractional source, committed conversion, repeated stock and overdue histories | Receipt creation/conversion, late expiry, source cleanup, retry and claimant ordering | Their shared construction settles the debtors during the live mark sequence. No resolved capital loss crosses the remaining source support gap. |
| INV-066 `v16_attack_resolved_close_order_preserves_scarce_source_backing` | Unsettled resolved debtors with scarce shared backing | Its 50 backing plus 300 debtor capital cannot exceed 400 claim face. It never has excess late capital to classify as junior residual. |
| INV-024 live-to-resolved entitlement histories | Continuous owner ledger and final solvent payout | Starts resolution with funded, settled claims; no late source-support cap or receipt/expiry boundary. |
| INV-063 expiry/refill and spent-backing owners | Live expiry, spent stock, refill and owner exit | No excess resolved debtor capital feeding another domain's terminal receipt. |
| INV-028 shared-source late exit; INV-039 pending-debt/restart owners | Source capacity, pending obligations and bounded debt settlement | No independent source-cap threshold feeding receipt floors before backing expiry. |
| INV-070/073 terminal residue and reserve owners | Terminal custody, reserve entitlement, burn/sweep and progress | Do not supply this debt-origin oracle for a surviving junior receipt. |
| Newly landed INV-067 custody boundary and INV-017 destination alias | Conditional custody account requirements and valid owner/token aliases | This test uses complete, distinct, canonical custody accounts throughout. |

The test is an invariant-owned public composition, not an engine helper replay.
No engine settlement, rate or payout helper computes expected outcomes.

## Public Construction And Oracle

System instructions allocate every market, portfolio and mint. SPL/ATA instructions
initialize and fund custody. Wrapper instructions initialize, deposit, trade, accrue,
resolve, settle and pay. LiteSVM supplies only executable programs, signer SOL and
authenticated Clock movement; no economic account bytes are injected.

Six owners deposit `[1000, 200, 1000, 300, 1000, 50]`. Positions of 2, 3 and 4
units open at 100 and accrue through ten authenticated five-unit mark increases
to 150, creating 100/150/200 positive faces. The first two winners share a source.
Only winners refresh on that asset, leaving both solvent debtors' principal and
K settlement untouched until resolution. The other asset's debtor settles its
50 principal live, and its flat remaining debt is cleared after resolution.

For first-debtor loss `L`, initial shared backing is `250 - L + {-1,0,1}`.
The 100-atom-loss order uses 149/150/151; the 150-atom-loss order uses 99/100/101.
After each resolved debtor settlement, fresh backing must be
`min(250, initial_backing + settled_losses)`. The excess belongs to residual.
Debt settlement returns precisely 100/150 senior atoms to the two debtors.
It changes no other owner's portfolio or wallet.

After both debtors settle, exactly 250 atoms remain source backing, while the
original backing amount is now junior residual. One winner detaches while another
still blocks payout; the latter then detaches, converts its entire 100/150 face,
and receives its principal plus that source entitlement. The first winner retains
its source claim. The unrelated source's 51 atoms expire; its winner creates a
200-face receipt against residual `R = initial_backing + 51` and denominator
`D = 200 + waiting_face`. Its exact initial payout is `1000 + floor(200*R/D)`.

At slot 20 or 21, expiry releases only `waiting_face`, preserving the spent
conversion and provider receivable. Both final junior entitlements are independently
computed as `floor(face * min(R + waiting_face, D) / D)`. The original receipt's
face and snapshot slot remain fixed. Cases cover both full payment and final
haircuts. Terminal vault value is exactly:

```text
surplus = max(final_residual - D, 0)
rounding = min(final_residual, D) - sum(owner-local junior floors)
vault = surplus + rounding
```

Surplus is 0..=2 and rounding is 0..=1. Mint supply, provider balance, each owner's
capital/PnL/SPL, both source faces/rates, fresh/spent/receivable stock, receipt fields
and resolved ledger are checked after every suffix attempt. Existing raw stock and
encumbrance censuses run before the stronger independent allocation assertions.

Every committed economic continuation strictly lowers the decoded lexicographic
rank of debt, exposure, fresh backing, source face, unpaid final SPL, and nonfinal
receipts. Each owner-signed portfolio deletion separately decreases the count and
transfers exact rent to the market. Zero-due retries and terminal replays preserve
the complete tracked frame. Two failed bundles per world prove rollback after
successful debtor SPL payout, and after expiry plus two successful SPL payouts.
Failure logs bind the successful prefixes and exact error location. Only the
separate payer loses the runtime signature fee.

## Discriminating Mutation

A private copy of the **pinned main dependency**, under `target-mutant-engine`,
changes only the resolved allocation in
`reserve_new_capital_backed_loss_for_source_domain_not_atomic`:

```diff
- let source_backing_num = backing_num.min(support_gap_num);
+ let _ = support_gap_num;
+ let source_backing_num = backing_num;
```

A separate manifest in `target-mutant` copies the main manifest/lock, uses the
unchanged wrapper source through a local symlink, and adds a path patch for that
private engine. Neither the main manifest nor the Cargo dependency cache is edited.
The production baseline SBF remains separate and byte-identical.

On the mutant, the exact new selector fails at stage 2 with **399 fresh backing
atoms versus the independently permitted 250**. The existing stock and encumbrance
censuses have already passed: total value is balanced but 149 atoms are assigned
to the wrong stock class. Both nearest existing public controls still pass.
This establishes a net-new executable detector for the resolved source residual
attribution class. It does not claim to reproduce or close any specific withheld PR.

## Exact Verification

Commands run from the worktree root. Default Anchor-v2 production SBF was freshly
built offline, with no shared build target:

```bash
env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  CARGO_TARGET_DIR=/dev/shm/percolator-source-residual-c8d4/target-sbf \
  CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 TMPDIR=/dev/shm/percolator-source-residual-c8d4 \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --sbf-out-dir /dev/shm/percolator-source-residual-c8d4/target-sbf/deploy -- --locked

# Separate fault-control build; its private lockfile is allowed to record the path patch.
env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  CARGO_TARGET_DIR=/dev/shm/percolator-source-residual-c8d4/target-sbf \
  CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 TMPDIR=/dev/shm/percolator-source-residual-c8d4 \
  cargo build-sbf --manifest-path target-mutant/Cargo.toml --tools-version v1.52 \
  --no-rustup-override --offline \
  --sbf-out-dir /dev/shm/percolator-source-residual-c8d4/target-mutant/deploy

export CARGO_TARGET_DIR=/dev/shm/percolator-source-residual-c8d4/target-host
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export TMPDIR=/dev/shm/percolator-source-residual-c8d4
selectors=(
  inv_067_terminal_payout_completeness_and_exact_once_settlement::resolved_source_residual::v16_program_late_debtor_source_cap_preserves_receipt_residual_entitlement
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_source_realization::v16_program_retained_receipts_preserve_identity_across_fresh_realization_or_expiry
  inv_066_resolved_payout_fairness_and_order_independence::v16_attack_resolved_close_order_preserves_scarce_source_backing
)
PERCOLATOR_FUZZ_SBF="$PWD/target-mutant/deploy/percolator_prog.so" \
  cargo test --locked --offline --test v16_cu -- --exact --nocapture "${selectors[@]}"
# Expected fault-control result: 2 passed, 1 failed (new selector only).

PERCOLATOR_FUZZ_SBF="$PWD/target-sbf/deploy/percolator_prog.so" \
  cargo test --locked --offline --test v16_cu -- --exact --nocapture "${selectors[@]}"
# 3 passed, 0 failed, 0 ignored; 1430 filtered out; 24.55 seconds.

cargo test --locked --offline --test v16_program_fuzz_regressions \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_every_public_invariant_source_and_declared_test_is_mounted \
  -- --exact --nocapture
# 1 passed, 0 failed; 502 source files / 1906 available tests; 3.74 seconds.

rustfmt --edition 2021 --check --config skip_children=true \
  tests/invariants/cu/inv_067_resolved_source_residual.rs \
  tests/invariants/cu/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs
git diff --check
git diff --cached --check
git diff --exit-code 635007c7 -- src Cargo.toml Cargo.lock
sha256sum target-sbf/deploy/percolator_prog.so target-mutant/deploy/percolator_prog.so
```

Baseline SBF SHA-256:
`87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.
Mutant SBF SHA-256:
`9684cf38c793a55c727c4040d7d3ae6627ca42cb29287cee4b9938fe938f7eaa`.

The final three baseline selectors pass on the integration base above, and the
new mount passes the exact invariant source census. The final mutation run gives
two passes and one expected failure in 4.46 seconds, after both stock censuses pass.
The new baseline selector passes 24 worlds, 252 ranked commits, 48 exact rollbacks
and 144 portfolio closes. Maximum checked suffix CU is 323,721 (a rejected bundle;
the earlier standalone run measured 317,721 with different generated account keys).
Single successful steps/deletions use the existing 300,000 bound; rejected bundles
use a 600,000 bound. Scoped rustfmt, whitespace and production-diff checks pass.
No full suite or unrelated selectors are run.

Early exploratory shapes were discarded: a receipt cannot be materialized while
stale debtors still block payout, and insolvent active debtors introduce B haircuts.
The final fixture deliberately uses solvent deferred debtors to isolate source
residual allocation. One earlier configuration was rejected during market init
because its margin did not satisfy the solvency envelope. These were fixture
development failures, not production findings.

## Limits

This is a finite primary-SPL, no-CPI, zero-fee/funding, two-asset product with six
distinct owners, one source conversion and one post-snapshot expiry. It proves
economic exit plus owner-signed deletion, leaving explicitly classified residue.
It does not add a CloseSlab/burn proof, arbitrary-history generator, maximal-shape
coverage, Recovery/insurance/live-lien product, native/dual-quote coverage or a
whole-invariant status claim. No real LoF/DoS finding is asserted.
