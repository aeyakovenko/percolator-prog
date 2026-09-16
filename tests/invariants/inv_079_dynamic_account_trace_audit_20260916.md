# INV-079 dynamic program-account witness fidelity

Initial base: `926b95442051a0818f34d95038a0e3e232b5437f` from freshly fetched
`origin/main`. Final base: `635007c7e52ff49f32e628b4b6e8028f41384d49`
(unrelated INV-067 receipt and INV-017 destination-alias coverage).
Branch: `codex/invariant-witness-quality-20260916-f3a8`.

## Gap and Scope

`V16Svm::trace_state_keys` covered only the fixture's fixed account catalog.
A portfolio created, initialized and funded through public transactions after
`begin_public_trace` was absent unless separately added to that catalog. Its
capital or matcher control could then be changed through `set_account` and
`validate_public_execution` would still accept the trace. Main's existing
mutation sentinel changes a fixed-catalog foreign market's lamports, so it does
not exercise this hole. The lifecycle mount/declaration guards check source
availability and likewise cannot detect it.

The fix is test-support only: retain wrapper/matcher-owned transaction keys
observed before or after each transaction until trace completion, even when
their owner changes or the account disappears. Examine all transaction keys,
including accounts used only by an earlier instruction in a bundle. This does
not change the existing per-step transaction summary or token-account catalog.

The new mounted LiteSVM selector constructs an additional portfolio through
System CreateAccount, InitPortfolio and Deposit, without registering it in
`actors`. Each world establishes nonzero portfolio identity and seven atoms of
capital with exact source/vault deltas. Two clean controls withdraw all seven
atoms through the public API. Ten negative controls cross five injected changes
(capital, matcher fee cap, lamports, owner and deletion) with detection at the
next unrelated public call or directly at trace completion. Each negative must
report exactly one mutation and fail public-evidence validation. Injection is
only a falsification test for the evidence validator, never economic evidence.

This advances INV-079 and the witness-integrity obligations of INV-084/087. It
does not add a proof, establish a production LoF/DoS finding, promote invariant
status, or certify control enforcement. Unseen accounts before first transaction
contact, external oracle/Clock provenance, arbitrary instruction bundles and
arbitrary state histories remain outside this focused regression.

Required repository guidance and the INV-079/084/087 owners were read. Active
worktree/branch metadata and open PR titles were checked; no holdout branch code
was read. No production, Cargo, engine or tracked build artifact changes.

## Verification

The exact new selector failed with unchanged main test support: both clean
controls completed, then `capital, before_next_call=false` reported zero
mutations instead of one. With the capture fix, all 12 worlds passed. The eight
selectors below passed again on that final base (8 passed, 0 failed, 0 ignored;
13.82 seconds). Rustfmt and diff checks
passed. An initial compile attempt used a private payer field; the test now uses
the funded actor as transaction payer and does not expose another harness API.

Run from `/dev/shm/percolator-invariant-witness-quality-20260916-f3a8`:

```bash
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-public-gap-20260916-c91e-sbf-target/deploy/percolator_prog.so
# TDD: this selector alone failed before the support fix, then passed after it.
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::dynamic_account_trace::v16_public_trace_retains_dynamic_portfolio_controls_through_finish -- --exact --nocapture
# Final scoped regression selection:
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::dynamic_account_trace::v16_public_trace_retains_dynamic_portfolio_controls_through_finish \
  inv_079_public_reachability_evidence::v16_public_trace_schema_detects_out_of_band_economic_mutation \
  inv_079_public_reachability_evidence::v16_public_trace_terminal_classifier_requires_complete_economic_evidence \
  inv_079_public_reachability_evidence::v16_public_terminal_classifier_exhausts_normalized_outcome_space \
  inv_079_public_reachability_evidence::v16_every_public_trace_consumer_validates_reachability_evidence \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_lifecycle_metadata_evidence_is_mounted_in_public_harnesses \
  inv_080_error_propagation_and_exact_rollback::v16_program_engine_error_rolls_back_successful_close_and_deposit_prefix \
  inv_014_delayed_policy_and_policy_epoch_safety::v16_program_maintenance_share_supersession_changes_attribution_not_payer_value
rustfmt --edition 2021 --check --config skip_children=true tests/support/v16_svm.rs tests/invariants/public_sbf/inv_079_dynamic_account_trace.rs tests/invariants/public_sbf/inv_079_public_reachability_evidence.rs
git diff --check
git diff --check origin/main...HEAD
git diff --exit-code origin/main -- src Cargo.toml Cargo.lock
```

Existing host dependency outputs were copied into this worktree's private
ignored `target` and rebuilt there. The unchanged default-feature SBF was reused
read-only; `src`, `Cargo.toml` and `Cargo.lock` are byte-identical to its source
commit `cb236b52`. The matcher was privately copied from the existing current
fixture. No SBF rebuild or broad suite was run. Existing dead-code and
solana-client future-compatibility warnings remain.

Artifact SHA-256:

- Wrapper: `87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`
- Matcher: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`

The final scoped output is in the private `target/inv079-scoped.log`.
