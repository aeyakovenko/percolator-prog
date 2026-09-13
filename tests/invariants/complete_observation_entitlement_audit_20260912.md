# Complete-observation carry and liquidation entitlement

## Scope and isolation

Base: `adeac5fcacfb431db78c5646c9bdc9f70cf4f0ed`, the current HEAD of
`codex/astra-open-holdout-ledger-20260912` when this contribution started.
Worktree: `/tmp/percolator-provenance-conformance.UhlUqG`.
Contributor branch: `codex/provenance-complete-observation-20260912`.
Only this worktree was edited. No open PR branches, diffs or tests were inspected
or copied. The test uses the base checkout's general SPL/LiteSVM fixture APIs and
the production source at its pinned engine revision.

Executable owner:
[`inv_045_complete_observation_entitlement.rs`](cu/inv_045_complete_observation_entitlement.rs),
mounted by [`inv_045_no_free_mark_movement.rs`](cu/inv_045_no_free_mark_movement.rs).
Selector:
`inv_045_no_free_mark_movement::complete_observation_entitlement::v16_program_complete_observation_partitions_preserve_fractional_liquidation_entitlement`.

The new relation is **complete observation grouping and accrual cadence through
liquidation while two fresh external marks still retain fractional carry**.
This adds a moving-price, fee-paying continuation beyond fee-refresh admission;
it adds account-local refresh, liquidation and actual owner payout beyond a
pending-fraction checkpoint. Both providers remain Pyth throughout, so selected
provider assignment and mixed-provider liquidation are not repeated. Discovery
uses permissionless external observations; trade-driven mark route orders are
not the variable. The only trade suffix closes existing exposure at the already
accepted prices.

## Public construction and comparison

Eight independently initialized worlds cross three binary choices:

- Complete observation order `[0, 1]` versus `[1, 0]`, with corresponding feed tails.
- Both observations in one discovery crank versus one crank per observation on
  the same flat keeper portfolio. Every target refresh/liquidation supplies both.
- After common publication at slot 2, catch up directly to slot 4 versus through
  slots 3 and 4. The report values and publish timestamps stay fixed.

System creates accounts, SPL creates the mint and associated token accounts,
SPL mints the fixed supply, and public Percolator instructions initialize,
configure, deposit and trade. There are no engine/account/capital/position writes
from the host. Clock sysvars and externally owned Pyth report accounts are the
explicit harness inputs. The caller always supplies `now_slot = u64::MAX`; the
observed engine slots and economics must follow authenticated Clock instead.

The initial prices are `[1003, 2007]`, quantities are `[10000, 1000] * POS_SCALE`,
and new targets are `[980, 1950]`. The price cap is 24 bps per slot and funding is
disabled. Deposits are `[610000, 20000000, 1001]` atoms for target, peer and keeper.
Each new report has publish time 101 and is first accepted at slot 2.

At each complete discovery checkpoint the independently calculated price and
carry are:

```text
numerator[i] = initial_price[i] * 24 * elapsed_slots
accepted[i]  = initial_price[i] - floor(numerator[i] / 10000)
carry[i]     = numerator[i] mod 10000
```

Both carry values must be nonzero. At slot 4, accepted prices are `[996, 1993]`
and carries are `[2216, 4504]`. The test also checks the fixed cap anchor, report
target, publish time, first-good slot, mirrored Hybrid mark, complementary K
indices and zero F indices. Discovery leaves both exposed user Accounts exactly
unchanged and earns no reward, even though market state advances.

Twenty late-duplicate attempts stage both observations before rejecting the
third. Every rejection restores complete tracked Accounts, including market,
portfolios, user signers, token accounts, vault, mint, admin and oracle inputs.
The transaction fee payer is deliberately excluded from exact equality because
SVM charges transaction fees on failure.

## Entitlement and progress oracles

The first complete target refresh settles an independently calculated 84000-atom
loss. The full health certificate must include a further 203000-atom adverse
raw-target lag penalty: maintenance and initial requirements are 800650, equity
is 526000, and deficit is 274650. Worst-case loss includes the same penalty on
top of gross accepted-price notional. Certificate fields also equal a full
refresh on copied account bytes. That snapshot comparison shares the pinned
engine and supplements, rather than replaces, the independent input arithmetic.

A real partial liquidation must occur within six complete target cranks, after
the refresh. It selects asset 0 in either observation order and closes exactly
the same quantity in all eight worlds: `4205863454` position quanta. The test
requires a positive reduction smaller than the starting exposure, zero asset-1
reduction, coherent long/short OI and a zero post-liquidation deficit. The peer's
complete Account remains unchanged during target work.

The independent fee calculation uses two ceilings: accepted-price notional,
then the configured 5-bps liquidation fee. Raw target price and the other asset's
accepted price must each produce a different fee. The actual fee is 2095 atoms;
the keeper receives `floor(2095 * 3333 / 10000) = 698`, with numerator residue
2635. Insurance retains 1397, assigned only to asset 0's domains as `[698, 699]`.
The target pays the whole fee; publication and refresh pay nothing. All mark,
carry and provenance assertions remain live through liquidation and retries.

After the peer's own complete refresh, the owner ledger is exactly
`[523905, 20084000, 1699]`. Three further target attempts preserve prices, carry,
OI, fees, domain attribution and each entitlement; at least one must reach
`EngineNonProgress` with exact rollback. Every successful crank must change
tracked public state. Quiescence is accepted only after mandatory successful
discovery, full refresh and fee-paying liquidation.

Public same-slot trades close the remaining exposure in the chosen asset order.
The target withdraws all 523905 atoms and the keeper withdraws all 1699 atoms,
leaving their internal values at zero. The peer retains 20084000 internal atoms;
insurance retains 1397 and the vault contains exactly 20085397 SPL atoms.
Mint supply, actual vault balance, all owner payouts and the independent history
ledger reconcile. Market and all portfolio shape validations run after every
successful crank, each closing trade and each payout.

## Invariant mapping and limits

| Invariants | Added bounded evidence |
| --- | --- |
| INV-020/045 | Authenticated time/report provenance; fixed-anchor accepted-price cap. |
| INV-024/038 | Each owner's PnL/fee/reward/payout, unequal price carries and reward residue. |
| INV-041/052 | Identical selection, close quantity and attributed outcome across eight partitions. |
| INV-056/088 | Complete two-leg local recertification after market-only discovery; independent lag penalty. |
| INV-061/071 | Positive bounded liquidation, coherent OI, healthy result, then bounded quiescence. |
| INV-081/086 | Deployed SBF state validation, independent input arithmetic and snapshot refresh comparison. |

This is sampled conformance evidence, with no invariant/status/holdout promotion.
It does not prove arbitrary liquidation sizing: the selected quantity is compared
across schedules, while its general arithmetic remains owned by INV-061. It does
not cover provider reassignment, stale/after-hours trade-driven provenance,
nonzero funding, omitted economically pending observations, bankruptcy, social
loss, maximum portfolio shape, or complete peer-claim realization. The peer's
84000-atom positive PnL is attributed and retained, not converted and paid out.
No production failure or production change was found or introduced.

## Reproduction

Fresh default-feature SBF built from this checkout using platform-tools v1.52.
Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
SBF SHA-256: `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
No build output or test artifacts from other worktrees were copied.

```bash
cd /tmp/percolator-provenance-conformance.UhlUqG
export CARGO_TARGET_DIR=/dev/shm/provenance-complete-observation-UhlUqG-target
export TMPDIR="$CARGO_TARGET_DIR/tmp"
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
mkdir -p "$TMPDIR" "$CARGO_TARGET_DIR/deploy"
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu inv_045_no_free_mark_movement::complete_observation_entitlement::v16_program_complete_observation_partitions_preserve_fractional_liquidation_entitlement -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --exact \
  inv_045_no_free_mark_movement::v16_program_caught_up_hybrid_reward_uses_selected_asset_provenance_in_both_hint_orders \
  inv_056_hints_are_discovery_only_favorable_actions_fully_refresh::v16_program_external_oracle_hint_and_account_order_is_normalized_or_atomic \
  inv_038_rounding_and_ratio_conservation::v16_program_social_loss_aggregate_and_chunked_routes_converge_exactly \
  inv_020_authenticated_clock_slot_and_oracle_provenance::v16_bpf_permissionless_crank_uses_authenticated_clock_slot_not_caller_slot --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
git diff --quiet adeac5fcacfb431db78c5646c9bdc9f70cf4f0ed -- src Cargo.toml Cargo.lock
```

Final selector: **1 passed**, eight histories, 4.12s. Maximum measured crank CU:
**426970**, below the asserted 650000 bound. Nearby controls: **4 passed**, 1.10s.
Invariant charter/index: **1 passed**. Repository-wide formatting, whitespace
checks and production/dependency equality against the base all passed. The build
reported existing support-module dead-code warnings and the Solana client's
future-compatibility warning. No full-suite, matcher build or Kani run was needed
for this observation-only discovery increment.
