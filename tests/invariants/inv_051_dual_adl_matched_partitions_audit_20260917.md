# INV-051 dual-ADL matched reduction conformance

Base: `origin/main` at `4eb82c5ad8d43063d9f2852552d7fcafdce5ce26`.
Local branch: `astra-ultra/matched-book-conformance-20260917`.
Worktree: `/dev/shm/percolator-astra-ultra-matched-book-20260917`.
Engine: `94979ede7db934545e53a8f210dd063a9ea3ea63`.

## Gap selection

Discovery used the repository's charter, README, `traceability_gaps.tsv`, owning
invariant sources and their shared local helpers. No open PR diffs were used.
The traceability table is explicitly nonexhaustive and has no dedicated row for
this matched dual-index product; its existing INV-052 references concern claim
conversion and aggregate signed budgets.

The relevant existing evidence was compared before implementation:

| Owner / exact witness | Existing guarantee and distinction |
| --- | --- |
| INV-047 `v16_program_nonzero_fee_trade_routes_are_byte_exact_after_transport_normalization` and the inventory/cashflow partition owner | Unit-index route economics and transaction partitions; no simultaneous inverse conversion of two nonunit bases. |
| INV-048 `v16_program_all_trade_routes_keep_oi_equal_to_active_leg_scan` | Fresh-state four-route raw-basis census, extended by separate stateful lifecycle witnesses. |
| INV-050 `v16_program_adl_maker_reduction_bounds_taker_cross_zero_and_matched_book` | A scaled maker reduces against a unit-index taker; the unrelated three-owner cross-zero witness explicitly has unit indices. |
| INV-051 `v16_program_nonunit_adl_reduction_partitions_preserve_raw_basis_and_funded_exit` | Repeated unilateral reductions convert one scaled owner while changing the passive side's A. |
| INV-052 `run_rebalance_partition` / `v16_program_owner_rebalance_reduction_is_split_merge_invariant` | Repeatedly reduces actor zero, whose own A remains unit. The fee-trade partition owner also begins at unit A. |
| INV-058 `v16_program_post_transition_caps_match_across_reduction_and_cross_zero_histories` and the side-OI handoff owners | Position/side-cap and fee composition at unit ADL indices. |
| INV-061 nonunit fee-boundary sizing and INV-086 dual-ADL helpers | Engine-selected liquidation, Recovery and force-close quantities; no mixed-route matched partition comparison. |

Reusing unilateral reduction partitions or another nonunit liquidation boundary
would duplicate these owners. The implemented gap is two different nonunit raw
bases converted simultaneously by matched fills, with both side indices fixed
through the partial reductions. It adds INV-047/048/051/052 evidence without a
status promotion or a new INV-050/058/061 closure claim.

## Independent relation

Two owners open `Q = 31` or `3 * POS_SCALE + 7` atoms, in both orientations.
Owner zero reduces `floor(Q/3)`; owner one then reduces `floor(first/4)` of
the surviving effective quantity. Inputs alone determine the two distinct A
indices and both retained raw bases. Both bases must exceed effective exposure.

From this public state, the matched histories use `[effective - 1]`,
`[1, effective - 2]`, or `[effective - 2, 1]`, then a final one-atom close.
Four rotations through single no-CPI, single CPI, batch no-CPI and batch CPI
exercise mixed transports. Identities and the economic setup are identical
across compared worlds, rebuilt through public instructions without restoration.

At every setup position transition and matched fill, all five portfolios are
decoded. Independent BigUint-backed arithmetic checks effective exposure,
signed sides, generation/epoch, stored counts, loss weights and both OI lanes.
Every partial fill requires each raw basis to equal
`floor(remaining * original_a_basis / that_owner_current_A)`; both A indices
stay fixed. Complete leg endpoints agree across partitions and transports.

The mark is fixed at `POS_SCALE`, so notional equals quantity and PnL is zero.
A 137-bps base fee applies only to the matched suffix. Each owner's capital
debit is exactly the sum of per-fill ceilings; splitting adds at most one atom
per additional prefix fill. Insurance and custody reconcile independently.
Every fill frames SPL data and complete unrelated market, portfolio, mint and
backing-ledger Accounts. The final close clears both raw legs immediately,
both sides finalize, and each owner withdraws exactly principal minus its fees.
The trace validates the public suffix including setup trades, rebalances, fee
policy, implicit matcher authorizations, fills, resets and withdrawals.

## Validation

- New selector: **1 passed, 0 failed**, 48 worlds, 128 fills, 96 resets and
  96 exact payouts; 23.85 seconds.
- Nonvacuity: raw subtraction differs in 112 owner-prefix cells, sharing the
  other owner's index in 32, and inverse ceiling in 160. These are arithmetic
  distinctions asserted by the test, not production mutation results.
- Peak measured fill/reset/payout CU: **164,792**, within the existing trade
  and custody bounds. Setup and matcher authorization are outside this peak.
- Adjacent exact controls: **2 passed, 0 failed**, 6.74 seconds.
- Source/mount census: **1 passed, 0 failed**, 505 source files and 1,910
  available tests; 3.76 seconds. No broad suite was run.
- Scoped rustfmt, patch whitespace, and unchanged production/manifest/lock
  comparison pass. Existing host dead-code and Solana future-compatibility
  warnings remain.

The supplied SBF build's checkpoint has different dev-only `syn` features in
`Cargo.toml`. To honor the unchanged-manifest reuse condition, its build cache
was copied to a private target and SBF rebuilt from this worktree (5.79 seconds).
Result SHA-256: `87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.
Host dependencies were also copied into a private target. The authenticated
matcher is read through a worktree-local ignored symlink to the existing fixture;
SHA-256: `397cdded3ba64b5e03ea54498a160878dcc81dc844222f3ffbdc4e6210dd2936`.
No file in the original checkout was edited.

Exact commands from the isolated worktree:

```bash
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
export CARGO_TARGET_DIR=/dev/shm/percolator-astra-ultra-matched-book-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-astra-ultra-matched-book-20260917-sbf-target/deploy/percolator_prog.so

CARGO_TARGET_DIR=/dev/shm/percolator-astra-ultra-matched-book-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-astra-ultra-matched-book-20260917-sbf-target/deploy -- --locked

cargo test --locked --offline --test v16_cu inv_051_canonical_adl_effective_quantity::dual_adl_matched_partitions::v16_program_dual_adl_matched_partitions_preserve_both_bases_and_fee_adjusted_exits -- --exact --nocapture

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_051_canonical_adl_effective_quantity::v16_program_nonunit_adl_reduction_partitions_preserve_raw_basis_and_funded_exit \
  inv_050_cross_zero_decomposition::v16_program_adl_maker_reduction_bounds_taker_cross_zero_and_matched_book

cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_every_public_invariant_source_and_declared_test_is_mounted -- --exact --nocapture

rustfmt --edition 2021 --check --config skip_children=true tests/invariants/cu/inv_051_canonical_adl_effective_quantity.rs tests/invariants/cu/inv_051_dual_adl_matched_partitions.rs
git diff --check
git diff --exit-code origin/main -- src Cargo.toml Cargo.lock
```

## Limits

Finite two-owner, one-asset, fixed-mark histories with one fee policy and four
cyclic transport orders. This does not assert byte equality of entire worlds,
arbitrary partitions, maximum quantity/shape, changing prices/funding, source
liens, liquidation sizing, Recovery or terminal receipts. Observation/freshness
behavior is outside this increment. No production code, dependency pin or
invariant status changes were needed.
