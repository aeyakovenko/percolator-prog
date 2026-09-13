# Selected-provider assignment conformance, 2026-09-12

Base: `origin/codex/astra-open-holdout-ledger-20260912` at
`798c30d815892ac5283a62add9bfcc7f15b3a435`.
Worktree: `/tmp/percolator-oracle-admission-carry-20260912-b6f2`.
Branch: `codex/oracle-admission-carry-conformance-20260912-b6f2`.
The remote head matched the existing tracking ref, so no fetch was necessary.
Only this base's sources, tests, documents and the locked engine source informed
the increment. No open PR diffs or other worktree implementations were inspected
or copied. The original checkout was not edited. No push, production change,
Cargo change, invariant-status promotion or holdout closure is included.

## Overlap and New Scope

The overlap search covered the INV-020/027/028/045 CU owners and their mounted
children, associated audits, README and local wrapper/engine routing code.

| Existing coverage | Boundary of this increment |
| --- | --- |
| Standalone first admission and reward-recipient first risk | No new maintenance or admission assertion. |
| Authenticated reward handoff and interleaved carry | No paid-mark discovery, trade transport or carry increment. |
| Historical/latent, Hybrid and single-vacancy capacity | No new source-capacity history. |
| Pending fractional carry through due trade and resolution | Already on the base; not claimed as new terminal-carry coverage. |
| Chunked/staged observations and active-claim evidence | Already distinguish bounded progress from complete evidence; no general pending-omission theorem is added. |
| Mixed-provider liquidation | Always selects Pyth on asset 0 and pays only the keeper. This child selects a key-bound provider on either asset and completes all owner payouts. |
| Composite-provider liquidation | Multiple vendors form one asset's price; it does not separate physical leg slot, selected asset index and two assets' provider accounts with an exact fee-domain ledger. |

[`cu/inv_020_selected_provider_assignment.rs`](cu/inv_020_selected_provider_assignment.rs)
is mounted as a child of `mixed_provider_liquidation`, reusing its observation,
exact transaction rollback and independent census helpers. The exact selector is:

```text
inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations::mixed_provider_liquidation::selected_provider_assignment::v16_program_selected_provider_assignment_preserves_fee_domains_and_owner_exit
```

Eight fresh worlds cross selected Switchboard/Chainlink, selected asset 0/1 and
forward/reverse observation order. The selected asset is opened first, and the
test verifies its persisted physical slot is zero. Its sibling uses Pyth.
Payout order follows observation order, so those two dimensions are correlated;
this is not their independent Cartesian product. No world restores a snapshot.

Both owners open unit positions at 1,000,000. Current reports of 1,040,000 and
1,050,000 are authenticated at the same slot before next-slot accrual. Full
refresh then certifies exactly 130,000 target equity, 209,000 maintenance margin
and a 79,000 deficit. Only after that complete refresh does the liquidation call
omit the selected asset's discovery hint and supply its sibling's observation.

Before refresh and before liquidation, substituting the sibling's valid report
for the selected asset's configured feed rejects `InvalidOracleKey` at instruction
index 2. Bundling the identical independently simulated successful refresh or
liquidation before that rejection fails at index 3. The helper compares every
tracked and compiled Account, including payer metadata less the exact calculated
signature fee. After successful liquidation, a retry rejects `EngineNonProgress`
without a second reward. There are five such exact rollbacks per world, including
one rollback of a paid liquidation.

The selected asset's OI alone decreases, and the target position epoch advances
exactly once. An independent nested-ceiling notional/fee oracle consumes the
observed closed quantity and the selected asset's input price. It also requires
that using the sibling price would produce a different fee. Liquidation sizing
itself remains a deployed input, not an independent sizing proof.

| Selected asset | Closed quantity units | Target fee | Keeper reward | Insurance domains 0/1/2/3 |
| --- | ---: | ---: | ---: | --- |
| 0 | 844,020 | 8,778 | 2,925 | 2,926 / 2,927 / 0 / 0 |
| 1 | 835,981 | 8,778 | 2,925 | 0 / 0 / 2,926 / 2,927 |

The unrefreshed peer Account remains exact through liquidation. After peer
refresh, keeper withdrawal and public resolution, bounded owner-signed
`CloseResolved` calls pay the existing public ATAs exactly 10,090,000, 121,222
and 3,925 atoms for peer, target and keeper. All portfolios become terminal;
5,853 insurance atoms remain in both booked and SPL vault custody, with zero
capital and positive PnL totals. Provider/order outcomes agree for each selected
asset. Mint supply stays 10,221,000.

The new action and terminal phases use independent stock, source-credit,
reservation/encumbrance and current-certificate censuses. Terminal prefixes
require bounded progress, no owner overpayment, exact vault-plus-payout
conservation, fixed insurance/domain budgets and unchanged provider/mint/signer
Accounts. Terminal nonprogress retries compare the tracked frame excluding the
network fee payer; the 40 exact-fee rollback claims above refer to the separate
explicit rejection transactions. Fixture construction uses System/SPL/ATA/wrapper
instructions. Only signer SOL, Clock, blockhashes and vendor-owned report fixtures
use harness controls; vendor publication/authentication is a fixture assumption.

## Remaining Gaps

All five holdout labels remain **OPEN** and are used only as coverage names.

| Label | Increment and remaining scope |
| --- | --- |
| #413 | No new coverage. Net-of-uncollected-fee first-admission margin boundaries and insufficient-capital histories remain outside this test. |
| #422 | Exact non-Pyth selected-asset reward/insurance attribution and all three SPL entitlements. General paid-mark provenance, multiple reward episodes, exposed keepers and provider-policy succession remain outside this increment. |
| #423 | No new coverage. Existing bounded admission/reclamation matrices remain; broader over-capacity admission/recovery and arbitrary future-domain contention are not extended. |
| #425 | No new coverage. The base already has due-trade/terminal fractional-carry evidence; arbitrary uncommitted accrual and carry histories are not extended. |
| #426 | Two selected providers on either asset index, wrong-provider-account rejection and reversed complete observations. Selected-evidence omission is only after complete refresh; pending/uncommitted omission, multiple candidate selection episodes, more than two active assets and composite selected feeds remain outside this increment. |

Source capacity is unsaturated, funding and maintenance are zero, report inputs
are fresh and coherent, and liquidation is solvent. Reserve withdrawal, portfolio
Account rent reclamation, market retirement, all-route closure and vendor consensus
verification are not claimed. This is bounded current-code conformance evidence.

## Validation

The new exact selector passes: **8 worlds, 40 exact rollbacks, 52 successful
terminal calls**, with peak CU **379,573** action, **415,580** rollback bundle,
**52,095** keeper withdrawal and **201,391** terminal continuation. The existing
500,000-CU two-asset action budget applies, doubled for two-instruction bundles;
withdrawal retains its 300,000-CU budget. Setup is outside those counters.

A fresh locked/offline default-feature wrapper SBF was built in this worktree
with platform-tools v1.52; host compilation uses the same private fresh target,
with no copied artifacts. Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
Wrapper SHA-256: `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
Development corrected a temporary Rust slice lifetime in the new test. No
production conformance failure was observed. The existing Solana future-compatibility
warning and 346 invariant-index unused-support warnings accompanied the builds.

The new selector and three adjacent controls pass **4/4** together. The controls
cover the original mixed-provider omission relation (8 worlds / 40 exact rollbacks),
composite-provider liquidation (3 worlds), and due-trade/terminal fractional carry
(8 histories). Their peak CU are respectively 419,973, 296,367 and 214,785.
The invariant charter/index passes **1/1**. `cargo fmt --all -- --check`,
`git diff --check` and the production/Cargo unchanged check pass. The original
checkout retains its pre-existing conflicted `tests/v16_cu.rs` and deleted
`.claude/scheduled_tasks.lock`; no test or formatting command ran there.

```sh
export CARGO_TARGET_DIR=/dev/shm/oracle-admission-carry-b6f2-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=6 CARGO_INCREMENTAL=0 TMPDIR=/dev/shm
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations::mixed_provider_liquidation::selected_provider_assignment::v16_program_selected_provider_assignment_preserves_fee_domains_and_owner_exit \
  inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations::mixed_provider_liquidation::v16_program_mixed_provider_liquidation_omissions_preserve_exact_entitlements \
  inv_020_authenticated_clock_slot_and_oracle_provenance::v16_program_composite_epochs_gate_real_liquidation_across_provider_roles \
  inv_045_no_free_mark_movement::v16_program_pending_fractional_carry_survives_due_trade_and_resolution
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
git diff --exit-code -- src Cargo.toml Cargo.lock
```
