# INV-006 retained requests through address lookup tables

Baseline: `origin/main` at `2d2af7cc3d846f9d9fc386ad6d2c603d7be92a1a`.
Branch: `astra-ultra/inv001-023-cycle3-20260916222918`.
Worktree: `/dev/shm/astra-ultra-inv001-023-cycle3-20260916`.
Engine pin: `94979ede7db934545e53a8f210dd063a9ea3ea63`.

## Net-new evidence

The existing INV-006 legacy/v0 signature test explicitly uses empty lookups.
A repository search for `address_lookup_table`, `AddressLookupTable`,
`try_compile(` and `VersionedTransaction` found no populated-lookup transaction
test on this baseline. Existing INV-003 lifecycle and INV-017 account-role
matrices use directly encoded account keys. This increment exercises the runtime
resolution boundary that those tests do not reach.

The new INV-006 child has two public-SBF tests, with adjacent INV-003 and INV-017
ownership. It uses the existing V16Svm fixture, including its ordinary account
and SPL scaffolding, then public lookup-table instructions and public wrapper
instructions for every table and economic lifecycle transition. It does not
inject table bytes or edit portfolio state. The charter now distinguishes
directly signed static keys from signed lookup addresses, indices and privileges.
Authenticated runtime lookup resolution is an explicit dependency.

`retained_v0_lookup_entries_bind_signed_domains_roles_and_retry`:

- Publicly creates two tables containing the same keys, with distinct authorities.
- Asserts that the retained v0 deposit actually loads all four mutable economic
  accounts and the SPL program through a table; successfully simulates it.
- Publicly extends and freezes the original table after signing, checking its
  original entries and appended entries independently.
- Mutates table identity, writable index, readonly index and writable-index order
  after signing. All four fail signature verification with full account rollback,
  including unchanged network-payer lamports.
- Properly signs a market/portfolio alias and a readonly portfolio behind a
  successful unrelated deposit prefix. Requires the precise wrapper error at
  instruction 3, logged prefix/SPL success, and exact Account rollback. Network
  fees are checked separately and exactly.
- Lands the unchanged retained transaction, checks its amount and consumed
  sequence, then re-encodes its old consent using the second table. The new
  signature reaches the wrapper and fails `EngineStale`, rolling back its prefix.
- Pays out the deposited amount and reconciles capital, group totals, vault,
  source/destination tokens and total token supply.

`retained_v0_lookup_portfolio_aba_rolls_back_prefix_and_allows_fresh_consent`:

- Retains and successfully simulates a two-deposit v0 bundle before the second
  portfolio is withdrawn, closed and publicly recreated at the same address.
- Crosses same-owner A-A and owner A-B-A histories. Monotonic portfolio IDs are
  asserted; the lookup table remains byte-for-byte unchanged throughout.
- Requires an obsolete-ID/current-sequence probe and the original signed bundle
  to fail `EngineProvenanceMismatch` at instruction 3, each after a successful
  unrelated SPL deposit prefix. The isolated ID probe prevents stale sequence or
  signature-cache checks from masking a missing incarnation check.
- Fresh incarnation consent repairs the same bundle. Both participants withdraw
  their exact added amounts; sequences, group totals, external balances, unchanged
  table state and token supply are checked.

Across the new tests: **3 worlds, 3 live simulations, 4 signature rejections,
7 exact prefix rollbacks, 5 committed deposits, 5 final payouts**. Setup deposits,
table controls and pre-recreation withdrawals are excluded from these counts.

## Artifact and negative-control provenance

Wrapper and authenticated matcher were freshly built from this worktree with
default features, offline locked dependencies and platform-tools v1.52. Host
dependencies were privately copied from an existing `/dev/shm` target; the test
harness was compiled in this worktree.

- Wrapper SHA-256: `87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.
- Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

For a temporary negative control, only `expect_portfolio_id`'s rejection condition
was disabled (`if false && ...`). The SBF output went to a separate mutant
directory, and the production source was restored before executing the test.
The exact ABA selector failed **0 passed, 1 failed**: its obsolete-ID/current-
sequence bundle unexpectedly succeeded, with both wrapper and SPL transfers
logged. This demonstrates an economic acceptance failure, rather than a changed
error code or a sequence rejection. Mutant SHA-256:
`39c7d9bb4cdceca9c17a99629729f752abcb1544fa1185c996c7968b23ed13f7`.
No mutant source or artifact is committed.

## Reproduction

Final exact selection: **8 passed, 0 failed, 0 ignored** (2 new behavioral tests,
3 existing transaction controls, 1 existing detached-signature source census,
2 existing metadata integration checks). New-test peak CU: **44,619** for loaded
domains/roles/replay and **78,886** for the two-deposit ABA bundles, both below the
fixture's 1,400,000 limit. Scoped rustfmt and `git diff --check` pass. Existing
unused-support and `solana-client v1.18.26` compatibility warnings remain. Broad
suites and Kani were not run. The user's normal checkout retains its original
branch and pre-existing changes.

```bash
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-inv001-023-cycle3-20260916-target
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"

CARGO_TARGET_DIR=/dev/shm/astra-ultra-inv001-023-cycle3-20260916-sbf-target \
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --sbf-out-dir /dev/shm/astra-ultra-inv001-023-cycle3-20260916-target/deploy -- --locked
CARGO_TARGET_DIR=/dev/shm/astra-ultra-inv001-023-cycle3-20260916-matcher-target \
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --manifest-path tests/fixtures/auth_matcher/Cargo.toml \
  --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked

owner=inv_006_program_chain_message_type_and_version_binding
metadata=inv_079_public_reachability_evidence
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact \
  "$owner::lookup_table_retained_identity::retained_v0_lookup_entries_bind_signed_domains_roles_and_retry" \
  "$owner::lookup_table_retained_identity::retained_v0_lookup_portfolio_aba_rolls_back_prefix_and_allows_fresh_consent" \
  "$owner::retained_deposit_signatures_cannot_cross_legacy_and_v0_message_versions" \
  "$owner::retained_transaction_binds_program_market_kind_schema_and_blockhash" \
  "$owner::unmodified_retained_transaction_still_executes" \
  "$owner::deployed_wrapper_has_no_detached_signature_interpreter" \
  "$metadata::v16_invariant_charter_and_index_are_complete" \
  "$metadata::v16_public_instruction_coverage_registry_points_to_executable_evidence" \
  --nocapture

rustfmt --check --edition 2021 --config skip_children=true \
  tests/invariants/public_sbf/inv_006_program_chain_message_type_and_version_binding.rs \
  tests/invariants/public_sbf/inv_006_lookup_table_retained_identity.rs
git diff --check
```

The negative-control run substitutes
`PERCOLATOR_FUZZ_SBF=/dev/shm/astra-ultra-inv001-023-cycle3-20260916-mutant/percolator_prog.so`
and selects only the exact ABA test above. Its failure is expected.

## Limits and rejected duplicates

This is bounded executable coverage, with no new public-interface LoF/DoS/CU
finding, production fix, dependency change, evidence-fidelity guard or invariant
status promotion. It is safe for main as tests and clarification of the documented
transaction boundary.

Multiple simultaneous lookup tables, deactivation/cooldown/close/recreation of
tables, maximum-size v0 messages, loaded CPI matcher routes, arbitrary lifecycle
words, durable nonces and cross-cluster validator admission remain outside this
increment. The fixture uses LiteSVM 0.1.0's authenticated SlotHashes entry for table
creation; `warp_to_slot` alone does not synthesize validator slot history.

Rejected duplicate ideas: another empty-lookup legacy/v0 mutation, isolated
legacy portfolio ABA, existing matcher context/generation/expiry rollback products,
and another pairwise legacy alias matrix. Recent INV-008 retained insurance,
INV-014 runnable fee witnesses and INV-020 row426 evidence guards were also
excluded. The named INV-028/036/045/051/070/089 increments are outside this scope.
Only open PR/issue titles and branch names were inspected to check overlap; no
open PR implementation or test body was read or copied. All existing holdouts
retain their dispositions.
