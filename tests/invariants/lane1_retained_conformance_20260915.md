# Lane 1: Retained Identity and Policy Conformance

Base: `d809e9a563d9b8bf38f32648b32a15d75f526ec8`.
Workspace: `/tmp/percolator-lane1-retained-20260915`.
Branch: `codex/lane1-retained-conformance-20260915`.
The supplied checkout was left untouched, including its existing merge conflict.

Read-first inputs were `scripts/loop.md`, this directory's README,
`open_findings.tsv`, and `invariant_status.tsv` from the requested commit.
The benchmark titles and local coverage records supplied the obligations; no
benchmark PR patch or implementation was consulted. This increment adds bounded
conformance and sensitivity evidence, not a new severity classification or a
historical red/green exploit claim.

## New Coverage and Overlap

Owner: [cu/inv_014_retained_oracle_role_close.rs](cu/inv_014_retained_oracle_role_close.rs).
Selector: `v16_retained_close_preserves_fee_consent_after_funded_oracle_role_handoff`.
It reuses the existing single-CPI `World`, full-Account rollback checker, independent
fee calculation, and round-trip payout/stock census. Its parent adds only a module
mount. No production, dependency, fixture, or aggregate-status change is required.

Existing cold-oracle insurance/backing tests use flat users and separately funded
reserves. Existing retained round-trip and authority-ABA fee tests keep oracle
ownership fixed. The new intersection has live positions, insurance earned from
both traders, coalesced LP/cold-admin roles, and a retained close across oracle
management. Existing generation, partial-fill, and reserve-stock generators are
rerun rather than copied into another fixture.

Four worlds exhaust two trade directions and two attempted funded-role transfers:
insurance beneficiary and insurance operator. All economic state is created by
public System, SPL, ATA, wrapper, and authenticated-matcher instructions. The
clock is fixed at slot 1 with an honest unchanged price of 100. Oracle setup and
ordinary signer funding use the existing public fixture helpers. No portfolio,
market, or matcher account image is patched to produce the tested condition.

1. Public opening deposits the retained fixture's final 113 atoms and charges
   49 atoms to each owner at 19 bps. Both insurance domain budgets are nonzero.
2. Three distinct closing transactions are signed before management or repricing.
   Each has only payer and taker signatures, the current portfolio incarnations,
   position epochs, matcher sequence, and a 19-bps cap. All simulate successfully.
3. An LP/cold-admin-signed oracle rotation and successful fee-paying close precede
   a funded-role seizure. `EngineLockActive` at instruction 4 restores complete
   Accounts, including both positions, capital, fee stock, oracle profile, matcher
   context, and request/authority sequences. Only the normal payer fee is spent.
4. The standalone oracle rotation succeeds without the funded incumbent signing.
   Only the oracle authority and its authority epoch change. The retained close
   still simulates successfully. Observation power does not acquire insurance rights.
5. The current insurance authority raises policy to 37 bps. The unchanged close
   rejects at instruction 2 with `InvalidInstruction` before matcher invocation,
   although the LP grant permits 137 bps. The independent fee calculation makes
   the economic difference explicit: 95 atoms would exceed the 49-atom consent.
6. Restoring 19 bps admits an unchanged pre-signed alternative. Exactly one close
   commits, each position epoch advances once, and matcher sequence is preserved.
   Another pre-signed alternative rejects with `EngineStale`, independently of
   Solana's duplicate-signature cache.
7. Both owners withdraw all remaining capital through SPL. Per-owner total fees
   are 98 atoms, remaining vault insurance is 196 atoms, and each domain owns 98.
   Mint supply, per-owner payouts, PnL, OI, stock and reservation censuses reconcile.

Maximum new-test CU by phase: simulation 159,001; controls 2,929;
charged-close rollback 162,764; opening/close/withdrawal 199,219. The existing
500,000-CU transaction limit remains the bound; no max-shape claim is made.

## Row Verdicts

`coverage_reopenings.tsv` supplies normalized row status; `open_findings.tsv`
separately records benchmark discovery provenance. Neither is promoted here.

| Row | Verdict | Evidence and remaining limit |
| --- | --- | --- |
| 411 | OPEN; bounded coverage extended | New active-position oracle/policy/retained-close composition plus the existing taker-cap and mixed-route controls pass. All fee-bearing routes and arbitrary histories remain outside this increment. |
| 416 | OPEN; benchmark provenance remains `missing` | Trading-earned insurance remains with its incumbent through cold-admin oracle management; both funded-role seizure suffixes roll back an executed close. This does not forbid observation-only succession or prove containment over arbitrary backing/claim/lifecycle/oracle histories. |
| 432 | COVERED, reaffirmed | Independent taker-cap control and new retained close reject a rate beyond taker consent while LP consent remains permissive; exact restored-policy exit succeeds. This is the normalized single-CPI claim, not all of INV-014. |
| 412 | COVERED, reaffirmed | Existing scope-product and position-episode controls pass. The new test additionally restores a close's epochs and grant on failed management, then rejects a consumed close. |
| 414 | COVERED, reaffirmed | Existing public portfolio/asset/grant scope-product and asset-slot reuse controls pass; no new generation mechanism is claimed. |
| 415 | COVERED, reaffirmed | Existing generated insurance-stock histories pass: 48 worlds and 2,208 transactions across retained withdrawals, replenishment, custody, ledger and rollback variations. |
| 428 | COVERED, reaffirmed | Existing Live ledger/destination retry and explicit Live/Resolved reserve-authority epoch debit tests pass. |

Aggregate status stays unchanged: INV-001/002/003/004/006/007/008/009/012/013
are `SUPPORTED`; INV-005 and INV-014 are `REFUTED_CURRENT`; INV-010 and INV-011
are `OPEN_EVIDENCE`. All fourteen remain `PUBLIC_ROUTE` / `SAMPLED` under
`GLOBAL_CONDITIONAL_TCB`. Other retained rows keep their existing mappings; this
run does not claim to rerun every historical selector attached to them.

## Validation Commands

All commands ran in the isolated workspace. Host builds used these settings:

```sh
export CARGO_TARGET_DIR=/dev/shm/percolator-lane1-20260915-host
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4
export PERCOLATOR_FUZZ_SBF=/tmp/percolator-lane1-retained-20260915/target/deploy/percolator_prog.so
```

Fresh wrapper and matcher artifacts used `cargo build-sbf --tools-version v1.52
--no-rustup-override --offline --sbf-out-dir <output> -- --locked` with
`RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc`, four build
jobs, and separate task-owned targets. The wrapper output is `target/deploy`;
fixture builds specify `--manifest-path tests/fixtures/{auth_matcher,hostile_matcher}/Cargo.toml`
and output to the corresponding fixture's `target/deploy`. Task-owned SBF
intermediate targets were cleaned after artifact generation due to filesystem
capacity; no shared or other-lane target was cleaned.

```sh
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_014_retained_oracle_role_close.rs
git diff --check
git diff --exit-code -- src/v16_program.rs Cargo.toml Cargo.lock

cargo test --locked --offline --test v16_cu -- --nocapture --test-threads=2 \
  retained_single_cpi_policy_history \
  v16_retained_single_cpi_taker_fee_cap_rejects_policy_increase \
  v16_program_cold_oracle_replacement_preserves \
  v16_program_funded_role_guard_and_oracle_handoff_are_source_complete \
  v16_attack_signed_trade_cannot_replay_across_asset_slot_reuse \
  v16_portfolio_incarnation_id_separates_close_and_reuse \
  v16_program_position_episode_matrix_rejects_stale_consent_fixed_case \
  v16_program_closed_market_address_is_permanently_tombstoned \
  v16_generated_live_insurance_stock_epochs_preserve_retained_rail_entitlement \
  v16_live_insurance_debit_consumes_epoch_across_ledger_and_destination_retries \
  v16_program_partial_fill_invalidates_every_stale_route_and_allows_every_fresh_residual \
  v16_program_retained_single_cpi_quote_refresh_preserves_both_landing_orders \
  v16_program_batch_cpi_aggregate_quote_caps_abort_matcher_and_wrapper_atomically \
  v16_program_retained_scope_product_requires_portfolio_asset_and_grant \
  v16_program_retained_scope_product_separates_admin_epoch_tuple_and_expiry \
  v16_program_close_consent_tracks_only_committed_funding_round_trips \
  v16_live_insurance_debit_consumes_reserve_authority_epoch \
  v16_resolved_insurance_debit_consumes_reserve_authority_epoch

cargo test --locked --offline --test v16_program_fuzz_regressions -- --nocapture --test-threads=2 \
  inv_001_market_incarnation_binding \
  inv_006_program_chain_message_type_and_version_binding \
  v16_invariant_charter_and_index_are_complete \
  v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  v16_dated_open_security_finding_benchmark_is_non_overclaiming
```

Final result: 26 CU tests and 9 public-route/metadata tests passed. The first broad CU run
had one fixture-availability failure, resolved by building `hostile_matcher` and
rerunning the affected test and final selection. The initial new-test compile
also caught a private sibling helper; mounting beneath its owner fixed visibility
without expanding the helper API. The full repository suite was not run.

Local run logs: [final CU selection](/tmp/percolator-lane1-20260915-final-cu.log),
[final public/metadata selection](/tmp/percolator-lane1-20260915-final-public.log),
[fee mutation](/tmp/percolator-lane1-20260915-fee-mutant.log), and
[role mutation](/tmp/percolator-lane1-20260915-role-mutant.log).

Wrapper SHA-256: `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
Authenticated matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

## Mutation Sensitivity

Two temporary mutations were compiled separately with the same SBF command and
run against the already-built test binary, selecting only the new test. Setting
`PERCOLATOR_FUZZ_SBF` selected `target/fee-mutant/percolator_prog.so` or
`target/role-mutant/percolator_prog.so`; neither replaced the base artifact.

| Mutation | Observed failure |
| --- | --- |
| Remove the single-CPI `cfg_pre.trade_fee_base_bps > fee_bps` rejection | The retained 19-bps close succeeds after the 37-bps policy update; the expected instruction-2 rejection assertion fails. |
| Have the oracle handoff also write `insurance_authority` and `insurance_operator` | The cold-admin rotation/close/seizure bundle succeeds; the expected instruction-4 funded-role rejection assertion fails. |

Both mutant runs exit 101 at the intended assertion after public instruction
success, not during setup. The fee guard is restored before building the role
mutant. All source edits are then restored exactly and the original artifact's
tests rerun. These mutants demonstrate test sensitivity to the mismatch classes;
they are not evidence of a vulnerability at the base commit or any benchmark PR's
exact pre-fix parent.
