# INV-051 nonunit-ADL owner reduction partitions

Base: `056cd6ba65dcea8a1c7309aa94f73011bc4020f2` (`origin/main`). Engine:
`94979ede7db934545e53a8f210dd063a9ea3ea63`. Discovery used landed source only;
no open issue/PR tests or fixes were inspected or imported.

## Gap and oracle

INV-052's `run_rebalance_partition` repeatedly reduces actor 0, whose ADL index
remains unit while actor 1 is haircutted. INV-086's ADL clamp matrix checks one
nonunit-index request followed by a full close and resolution. Neither checks
aggregate/split/reversed reductions on the already-haircutted owner, repeated
effective-to-raw conversion, and ordinary Live withdrawal after both side resets.

The new INV-051 test opens `11` or `3 * POS_SCALE + 7` quantity atoms in both
orientations. One public unilateral reduction removes `floor(open / 3)` and
leaves the other owner's raw basis untouched with a nonunit ADL index. From that
state it compares `[effective - 1]`, `[1, effective - 2]` and
`[effective - 2, 1]`, then closes the last atom. Independent BigUint-backed
helpers require `ceil(raw * A / a_basis)` exposure and
`floor(remaining_effective * a_basis / A)` retained raw basis at each prefix.
Both OI lanes must equal the input-derived remainder. Owner capital, PnL, fees,
insurance and actual SPL custody remain exact. The passive portfolio is framed
through each reduction; stale-epoch retries and zero quantities frame ten
economic accounts, excluding only the separate transaction fee payer.

Every world cleans up the passive zero-effective leg permissionlessly, finalizes
both reset sides, restores unit indices, and withdraws each owner's full input
principal. All owner reductions use only that owner, the market and its own
portfolio; the passive owner/account is absent from the instruction. The price
is fixed at `POS_SCALE`, making one quantity atom one notional atom. At price
100, a dust partial reduction correctly rejects `EngineNonProgress` because
rounded risk notional does not change; that rejected fixture was not a DoS.

## Results and negative controls

- Final selection: **6 passed, 0 failed, 0 ignored** (one new, five existing).
- New test: **12 worlds**, **20 partition reductions**, **12 zero-quantity
  InvalidInstruction rejects**, **20 stale-epoch EngineProvenanceMismatch
  rejects**, **12 one-atom exits**, **12 passive cranks**, **24 side
  finalizations**, **24 full-principal withdrawals**; zero terminal vault/capital.
- Forward-floor arithmetic differs from required exposure in **12 setups**.
  Raw subtraction differs from required inverse conversion in **16 prefixes**;
  inverse-ceil differs in **20 prefixes**. These counts are asserted.
- Temporary test-only substitution of `expected_raw` with `old_raw - reduce_q`:
  exact new selector **0 passed, 1 failed** at its raw-basis assertion.
- Temporary substitution with `ceil(remaining * a_basis / A)`:
  exact new selector **0 passed, 1 failed** at the same assertion. Both restored.
  These are oracle-sensitivity checks, not a production mutation campaign.
- Maximum observed setup/reduction/rejection/cleanup/withdrawal CU: **110,377**;
  all these calls enforce the existing **300,000** custody limit.
- Fresh default-feature SBF built in this worktree with platform-tools v1.52;
  SHA-256 `87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.

## Exact commands

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-inv045-062-cycle-20260916220023-target
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
CARGO_TARGET_DIR=/dev/shm/astra-ultra-inv045-062-cycle-20260916220023-sbf-target cargo build-sbf --tools-version v1.52 --sbf-out-dir /dev/shm/astra-ultra-inv045-062-cycle-20260916220023-target/deploy -- --locked
# Each negative control used this exact selector independently.
cargo test --locked --test v16_cu inv_051_canonical_adl_effective_quantity::v16_program_nonunit_adl_reduction_partitions_preserve_raw_basis_and_funded_exit -- --exact --nocapture
# Restored final tree: six exact selectors.
cargo test --locked --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_051_canonical_adl_effective_quantity::v16_program_nonunit_adl_reduction_partitions_preserve_raw_basis_and_funded_exit \
  inv_051_canonical_adl_effective_quantity::v16_program_crossed_adl_effective_exit_matrix_preserves_bounded_cleanup \
  inv_051_canonical_adl_effective_quantity::v16_program_unilateral_adl_effective_exit_matrix_preserves_bounded_cleanup \
  inv_051_canonical_adl_effective_quantity::v16_program_liquidation_adl_effective_exit_matrix_preserves_bounded_cleanup \
  inv_051_canonical_adl_effective_quantity::v16_attack_adl_deleverage_conserves_and_shrinks_winner_claim \
  inv_051_canonical_adl_effective_quantity::v16_attack_adl_then_settlement_winner_cannot_escape_deleverage
rustfmt --edition 2021 --check tests/invariants/cu/inv_051_canonical_adl_effective_quantity.rs
git diff --check
```

## Limits

Finite two-owner, one-asset, fixed-price, zero-fee histories through no-CPI
opening and owner-only RebalanceReduce. No changing-price/funding, liens,
multi-owner rounding aggregation, arbitrary partitions, maximum-shape CU,
complete-state equivalence, or new proof claim. No confirmed public LoF/DoS/CU
bug, production edit, status promotion, row425 carry duplication or INV-047
witness-composition change. Existing route, clamp and shape evidence remains
separately owned; these tests add the nonunit-index partition composition only.
