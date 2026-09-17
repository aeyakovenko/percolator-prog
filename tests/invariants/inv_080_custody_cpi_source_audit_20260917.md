# Custody CPI Account Binding And Error Propagation

Owner: `public_sbf/inv_080_custody_cpi_source_contract.rs`, mounted by the
existing public-SBF INV-080 module. Primary obligation: INV-080; supporting
custody/account-binding evidence: INV-018 and INV-021. This is source-composition
conformance over public custody handlers, not a new economic finding or a new
runtime rollback proof. No invariant status changes.

Base: `origin/main` at `b71a3e81a960cfd7c7166851f48dcb00a1c9d773`.
Worktree: `/dev/shm/percolator-astra-account-custody-20260917`.
Branch: `astra-ultra/account-custody-conformance-20260917`.
All edits are local to that worktree. No push, production changes, dependency
changes, shared-fixture changes, or changes to the original checkout were made.

## Gap And Non-Duplication

Inspected the coverage README, `traceability_gaps.tsv`, the INV-015/016/017/018/
021/080 owners and recent alias/native-close/deposit/backing rollback increments,
and the production custody helpers and consumers. The traceability index has no
dedicated row for these six IDs and explicitly says it is not an exhaustive gap
census. Repo-owned source and tests supplied the comparison; no open PR diffs
or matched-book/terminal-entitlement work supplied implementation material.

The initial malformed-account/PDA/alias candidates duplicated existing matrices
and were discarded before implementation. The retained gap is the composition
between an SPL instruction constructor, its account/authority/seed arguments,
the runtime CPI result, and every current public custody result consumer.

| Existing Evidence | Why This Relation Was Missing |
| --- | --- |
| INV-015 layout and ownership matrices | Reject malformed inputs; do not enforce the disposition of a CPI result after validation succeeds. |
| INV-016 PDA/token-moving-handler roster | Finds custody validators, derivations and handler membership; does not check the result of `transfer_tokens` or its signed/burn counterparts. |
| INV-017 role/privilege matrices and valid aliases | Vary public account roles and privileges; do not constrain remapping or discarding the returned token-program error. |
| INV-018 single SPL parser gateway | Binds parser callsites and validated facts; does not inspect CPI construction, invocation account vectors or result forwarding. |
| INV-021 close/realloc and native-refund histories | Check concrete lifecycle and rent outcomes; do not cover every SPL close/transfer/burn error result syntactically. |
| INV-080 engine/dispatch source guards | Enforce engine mappings and dispatcher returns, leaving the intervening SPL helper and direct-close result paths unchecked. |

In particular, remapping the error returned by a shared signed-transfer or burn
helper is outside the predicates of the existing source rosters. The new guard
rejects that mutation independently for each helper and for both handlers that
return a signed transfer directly. Existing Deposit/native-Deposit/backing
witnesses execute real failures of the unsigned helper; they do not own this
complete signed/burn/close composition. The older failed-withdraw witness sets
the vault's program owner to a foreign key, which current `unpack_token_account`
rejects before the signed CPI. It therefore does not establish that CPI's error
identity despite its name.

The four existing source guards were run unchanged and pass. This audit does not
claim they were executed against mutated production. All negative controls live
in memory inside the new test; production was never edited, even temporarily.

## Checked Relation And Limits

The parsed-source census owns 22 result edges: 17 helper uses across 13 custody
handlers and five SPL constructors. Every reference must use a direct question
mark outside a locally caught closure/async/try block, or be the function's direct
tail result. References hidden by helper aliases or relevant macros reject.

The three complete, small helper bodies bind token program, source, destination
or mint, authority, amount, AccountInfo vector and signer seeds. Only zero amount
may return before CPI; nonzero execution directly returns `invoke` or
`invoke_signed`. An independent invocation census permits exactly those three
calls and the two direct vault-close calls.

Both close constructor/invocation pairs must match their actual vault/refund
roles. The secondary branch consumes the validated optional custody tuple;
the two close blocks directly precede canonical slab realloc. This checks wrapper
custody and error disposition, not terminal payout eligibility or entitlement.

The mutation selector parses and rejects 22 source variants, including ignored,
optional, remapped and locally caught results; indirect/macro uses; duplicate
calls; wrong CPI account/instruction/seed bindings; skipped secondary closure;
and changed helper or handler tail results. These are parsed-source controls,
not compiled or deployed production mutants. Original source, comment-only input
and an unrelated added function are three accepted controls.

This deliberately reviews the current Rust shape; equivalent future refactors
may require updating the contract. It is not Rust name-resolution/control-flow
verification, a proof about arbitrary macro expansion, a new public history,
or a proof of Solana/SPL rollback. Admission predicates, dispatch, signer/PDA
validation, matched-book accounting and terminal entitlement retain their owners.

## Artifacts And Exact Validation

The private host target was copied from the existing
`/dev/shm/percolator-public-gap-20260916-c91e-host-target` cache. Only the selected
host test binaries and the required cached `syn` feature variant were compiled;
no SBF rebuild or broad test suite was run.

The supplied SBF was reused with `src`, `Cargo.toml` and `Cargo.lock` unchanged
from this branch's `origin/main` base. The artifact source worktree
`/dev/shm/percolator-public-gap-20260916-c91e` has identical `src` and `Cargo.lock`.
Its only manifest difference is main's subsequently added, host-only `syn`
dev-dependency features `extra-traits` and `visit`; normal/on-chain dependencies
and feature definitions are identical. This branch changes none of those files.
Wrapper SHA-256:
`87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.

The existing public rollback fixture also loads an authenticated matcher even
though this selected workflow does not trade. Its source and Cargo files equal
the cached fixture at `/dev/shm/astra-ultra-inv001-023-cycle3-20260916`.
An ignored worktree-local symlink exposes that fixture's deployed artifact:
`tests/fixtures/auth_matcher/target/deploy/auth_matcher.so`.
Matcher SHA-256:
`50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

All Cargo commands used:

```bash
export CARGO_TARGET_DIR=/dev/shm/percolator-astra-account-custody-20260917/target
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-public-gap-20260916-c91e-sbf-target/deploy/percolator_prog.so

cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture \
  inv_080_error_propagation_and_exact_rollback::custody_cpi_source_contract::v16_custody_cpi_results_and_account_bindings_are_source_complete \
  inv_080_error_propagation_and_exact_rollback::custody_cpi_source_contract::v16_custody_cpi_contract_rejects_result_and_account_binding_mutations \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_every_public_invariant_source_and_declared_test_is_mounted

cargo test --locked --offline --test v16_program_fuzz_regressions \
  inv_080_error_propagation_and_exact_rollback::v16_program_engine_error_rolls_back_successful_close_and_deposit_prefix \
  -- --exact --nocapture

cargo test --locked --offline --test v16_cu -- --exact --nocapture \
  inv_016_canonical_pda_and_seed_binding::v16_program_pda_and_token_move_callsite_roster_is_source_complete \
  inv_018_quote_mint_vault_token_program_and_authority_integrity::v16_program_spl_account_parser_is_single_gateway_and_reuses_validated_state \
  inv_080_error_propagation_and_exact_rollback::v16_program_explicit_engine_error_dispositions_are_source_complete \
  inv_080_error_propagation_and_exact_rollback::v16_program_dispatch_and_entrypoints_preserve_every_handler_error

rustfmt --edition 2021 --check --config skip_children=true \
  tests/invariants/public_sbf/inv_080_custody_cpi_source_contract.rs \
  tests/invariants/public_sbf/inv_080_error_propagation_and_exact_rollback.rs
git diff --check
git diff --exit-code b71a3e81a960cfd7c7166851f48dcb00a1c9d773 -- src Cargo.toml Cargo.lock tests/support tests/fixtures
```

Final results: **3 passed / 0 failed**, **1 passed / 0 failed**, and
**4 passed / 0 failed**, respectively; none ignored. The census finds 506 source
files and 1,912 available tests. Both public rollback orders preserve exact
`EngineLockActive`, restored closed-account rent and deposit effects, followed
by live unchanged-prefix retries; observed CU is 85,047 and 133,047.
Scoped formatting, whitespace and unchanged-production checks pass.

Development runs corrected the scanner's conditional-entrypoint collection,
legitimate direct tail-result handling and syntax-template punctuation. The
optional public control initially stopped on a missing matcher fixture, then
on an unstripped build artifact; using the existing deployed artifact resolved
both setup failures. No production conformance failure was observed.

Exact run logs remain under the private `target/`: `custody-final-source.log`,
`custody-public-rollback.log`, and `custody-adjacent-source.log`.
