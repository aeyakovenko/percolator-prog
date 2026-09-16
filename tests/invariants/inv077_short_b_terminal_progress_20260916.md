# INV-073/077/082: Partial B Settlement Through Terminal Progress

One new public-route LiteSVM conformance product passes four histories at
14 active legs and 28 positive, unliened source records. A short-side B chunk
leaves fractional carry and 80 unsettled loss atoms; permissionless resolution
and bounded terminal continuation still return exactly the same owner payouts
as completing B settlement before resolution. Both claimant orders pass.

No current behavior mismatch was found. **Row 423 remains OPEN; INV-073 remains
REFUTED_CURRENT; INV-077 and INV-082 remain OPEN_EVIDENCE.** Rows 420, 421 and 433
also remain OPEN. This adds sampled progress coverage, without a status promotion
or a general liveness claim. No TSV was edited.

## Isolation and Changes

- Clone: `/dev/shm/percolator-inv073-077-082-progress-20260916`.
- Base: `ab8c179e21abed9b8c0a84bac21061f6fcf90343`, the fetched tip of
  `origin/codex/astra-invariant-cycle-20260915`.
- Local branch: `codex/inv077-b-terminal-progress-20260916`; no push.
- New selector: `inv_077_bounded_work_and_maximum_shape_compute::short_side_b_budget::terminal_progress::v16_program_partial_short_b_at_capacity_has_bounded_terminal_progress`.
- Changed files: `cu/inv_077_short_side_b_budget.rs`, new
  `cu/inv_077_short_b_terminal_progress.rs`, and this report.

The existing short-side B setup is extracted into a shared builder. Its market,
mint, vault, portfolios, ATAs and deposits now use public System/SPL/ATA/wrapper
instructions. Its original trade, mark, shutdown, forfeit and settlement
assertions are retained and pass. The original B selector keeps its 1,375,000-CU
guard. The new terminal selector uses the actual 1,400,000 transaction limit,
with a strict below-limit assertion on every measured transaction.

`/home/anatoly/percolator-prog`, production files, manifests, lockfiles, fixtures,
TSVs, and the newest row417, row419/435, row424 and row433 files were not edited.

## Coverage Boundary

| Existing coverage | Additional boundary in this product |
| --- | --- |
| Row423 distinct-feed progress | That product covers Hybrid observations, backlog, owner reductions and live conversion. This product covers frozen Recovery assets, partial B carry and unsigned terminal settlement. |
| Row433 split beneficiary custody | That product covers shared reserve custody after beneficiary succession. This product contains no beneficiary succession or reserve payout. |
| Original short-side B budget selector | Ends with all B work settled and exposure retained. This product resolves after one chunk or all 28 chunks, then pays every funded owner. |
| Direct maximum-shape resolved close | Uses the historical source-profit fixture, without fourteen short-side B liabilities or an interrupted fractional chunk. |
| INV-071 expired-close/B terminal continuation | Two-asset close-priority and expiry history. This product measures the combined active-leg/source caps with funded terminal payouts. |

Only one new `#[test]` is added. The original selector is reused as a control.
There is no new engine transition, model implementation, proof, production edit,
maximum-market claim, distinct-feed claim, native-quote claim, or reserve-custody
claim. Funding, fees, liens, external backing, arbitrary interruption points,
other trade transports and other claimant schedules remain outside this product.

## Reachable History and Checks

Public funding mints exactly 20,028 atoms: two 10,000-atom deposits and fourteen
two-atom debtor deposits. An empty checkpoint portfolio makes 17 portfolios.
The shared history creates fourteen historical one-atom long profits, then
fourteen short legs that each gain eight atoms against two funded debtor atoms.
Source-local shutdown, debtor forfeit and close continuation commit six B loss
atoms per short leg. At slot 81, the target has 126 PnL atoms, 10,000 capital,
fourteen active legs and twenty-eight positive source claims.

The product crosses one versus 28 public B calls with forward/reverse claimant
order. One call leaves 122 PnL atoms, 80 unsettled B atoms and one nonzero B
remainder. All 28 calls leave 42 PnL atoms and no B work. Both checkpoints retain
the full active/source shape. Portfolio signing keys are dropped before these
measured prefixes. Compiled transactions have exactly one signature, the keeper
fee payer, and fit within 1,232 bytes.

At authenticated slot 181, `ResolveStalePermissionless` receives caller slot
zero and resolves without changing the target portfolio. At slot 186, the
configured five-slot owner window has elapsed. Repeated `CloseResolved` calls
lower the lexicographic tuple of lapsed Fresh source records, remaining B atoms,
retained legs, occupied sources, unfinished portfolios and unpaid SPL atoms.
Every successful call and nonterminal sweep strictly lowers that decoded rank.
The initial work components bound total calls independently of observed CU.

The four worlds finish with payouts `[10042, 9986, 0, ..., 0]`, empty engine/SPL
vaults, zero aggregate capital, and unchanged SPL supply. All 17 portfolios are
economically terminal but remain materialized; mechanical deletion and slab
retirement are outside this test. The owner-local payout ceilings, raw/decoded
market stock census and reservation census run after every accepted step.
Untouched accounts, owner wallets and mint Accounts retain their full frames.

Every successful B, resolution or terminal instruction is first submitted with
a deliberately incomplete later `CloseResolved`. Failure is exactly
`InstructionError(3, NotEnoughAccountKeys)`, proving that the preceding call
completed. All 57 tracked Accounts, including Clock and custody, roll back
exactly. The separate payer changes only by the 5,000-lamport signature fee.
The same valid instruction then commits and must satisfy the progress/custody
checks. Failed transactions are not counted as progress.

| Histories | Terminal calls per history / initial bound | Exact payouts |
| --- | ---: | --- |
| One B call, either order | 98 / 181 | 10,042 and 9,986 |
| All 28 B calls, either order | 85 / 101 | 10,042 and 9,986 |

There are 366 terminal calls, 58 earlier B calls, four resolutions and 428
paired complete rollback checks. Maximum CU is 565,955 for B prefixes,
1,378,419 for successful terminal/resolution calls, and 1,378,804 for rollback
bundles. Successful progress therefore has 21,581 CU of observed headroom;
the rollback bundle has 21,196. These are measurements of this artifact and
history, not guaranteed headroom for other shapes or future changes.

## Artifact and Validation

Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`. Reused default-feature
wrapper SBF: `/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so`, SHA-256
`e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
Production sources and Cargo files match its recorded baseline
`d64f049005847848b095b3b8b2d21318d0504296`. No SBF rebuild is claimed. A private
host target was copied from `/dev/shm/row423-resource-progress-20260916-target`
and cleaned with `cargo clean -p percolator-prog` before compilation.

The commands below give the exact environment and selectors used; execution
supplied the environment inline with `env`.

```sh
cd /dev/shm/percolator-inv073-077-082-progress-20260916
export CARGO_TARGET_DIR=/dev/shm/percolator-inv073-077-082-progress-20260916-target
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu inv_077_bounded_work_and_maximum_shape_compute::short_side_b_budget::terminal_progress::v16_program_partial_short_b_at_capacity_has_bounded_terminal_progress -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --exact --nocapture inv_077_bounded_work_and_maximum_shape_compute::short_side_b_budget::v16_program_max_shape_short_b_budget_has_exact_public_progress inv_077_bounded_work_and_maximum_shape_compute::v16_program_public_full_shape_b_backlog_has_bounded_settlement inv_071_crank_progress::v16_program_public_expired_close_preempts_b_stale_and_preserves_terminal_progress
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_ -- --test-threads=3 --nocapture --skip v16_program_fixed_blockers_remain_progressing --skip v16_public_terminal_classifier_exhausts_normalized_outcome_space --skip v16_public_trace_schema_detects_out_of_band_economic_mutation --skip v16_public_trace_terminal_classifier_requires_complete_economic_evidence
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_077_short_side_b_budget.rs tests/invariants/cu/inv_077_short_b_terminal_progress.rs
git diff --check
git diff --cached --check
git show --format= --check HEAD
git diff --exit-code ab8c179e21abed9b8c0a84bac21061f6fcf90343 HEAD -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/**/*.tsv' ':(glob)tests/invariants/*.tsv'
git status --short --untracked-files=all --ignored tests/fixtures
git status --short
```

Results: the new selector passes (one test, four histories); three adjacent
controls pass; all 13 metadata guards pass; formatting, whitespace, commit,
protected-path and clean-tree checks pass. The metadata command deliberately
excludes four matcher-dependent runtime tests, so it is not a full-suite result.
Existing dead-code and Solana future-compatibility warnings remain.

The earlier control command was also executed:

```sh
cargo test --locked --offline --test v16_cu -- --exact --nocapture inv_077_bounded_work_and_maximum_shape_compute::short_side_b_budget::v16_program_max_shape_short_b_budget_has_exact_public_progress inv_077_bounded_work_and_maximum_shape_compute::v16_program_direct_close_resolved_at_14_leg_28_source_shape_is_bounded inv_071_crank_progress::v16_program_public_expired_close_preempts_b_stale_and_preserves_terminal_progress
```

It exited 101: two controls passed, but the direct-close control could not
construct its setup because `tests/fixtures/auth_matcher/target/deploy/auth_matcher.so`
is absent at the harness's hardcoded path. The protected fixture tree was not
populated. No direct-close control result is claimed for this clone.

Exploratory runs corrected a test fee oracle (deprecated Fees sysvar reported
zero), an incomplete rank that omitted source normalization/B work, and the
inherited 1,375,000-CU margin. The 1,378,804-CU transaction had already executed
within the actual transaction limit when that margin assertion failed. These
were test-assumption corrections, not current behavior mismatches.

Logs use prefix `/dev/shm/percolator-inv073-077-082-progress-20260916-`:
`fifth.log` is the final new-selector run, `controls-final.log` records the three
passing controls, `metadata.log` records the 13 guards, `controls.log` records
the unavailable direct-close control, and `first.log` through `fourth.log`
retain the exploratory outcomes.
