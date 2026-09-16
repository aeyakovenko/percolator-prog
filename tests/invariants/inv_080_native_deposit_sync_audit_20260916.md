# INV-080 native deposit synchronization rollback

Primary owner: `cu/inv_080_error_propagation_and_exact_rollback.rs`, with one
test in its `native_deposit_sync_retry` child module. Secondary evidence supports
INV-018/024/081. This is test-only bounded public-route coverage, not a finding,
production fix, invariant-status promotion, or independent-discovery claim.

The worktree started at `origin/main` `8b221355` in a private bare clone and
worktree under `/dev/shm/codex-invariant-public-20260916-2300`. The later main
`48f43292` changes only INV-079 lifecycle evidence files. Final main `c86065d5`
adds the separately excluded cure CPI witness under INV-039/076. An additional
`rg -n -i 'SyncNative|sync_native|native.*deposit|deposit.*native'` over its two
Rust files finds no overlap. Production, dependency pins, and all fixtures used
here are identical across these commits.

## Missing Boundary

Before editing, these searches identified the adjacent evidence:

```bash
rg -n -i 'sync_native|self.delegate|InsufficientFunds' \
  tests/invariants/cu/inv_080_error_propagation_and_exact_rollback.rs \
  tests/invariants/cu/inv_081_success_state_validity_over_complete_public_routes.rs \
  tests/invariants/cu/inv_018_quote_mint_vault_token_program_and_authority_integrity.rs
rg -n -i 'unsync|sync.?native|native.*deposit|deposit.*native' \
  tests/invariants --glob '*.rs'
rg -n -i '(rollback|reject|failure|error|abort)' \
  tests/invariants/cu/inv_008_insurance_native_recreation.rs \
  tests/invariants/cu/inv_073_native_insurance_ledger_progress.rs \
  tests/invariants/cu/inv_073_native_provider_redemption.rs \
  tests/invariants/cu/inv_070_shared_custody_terminal_history.rs \
  tests/invariants/cu/inv_073_native_recredit_custody.rs
```

- INV-080's existing failed-deposit test injects a foreign SPL account owner. It
  does not construct a public native source, execute SyncNative, or retry.
- INV-081's native roundtrip checks successful deposit/withdrawal/redemption
  with unsynchronized SOL remaining outside capital. It has no rejection path.
- INV-018's retained-source lifecycle and multisig tests use ordinary SPL quote
  accounts, without native synchronization or backing-lamport checks.
- INV-008/073's native reserve histories cover insurance epochs, payouts,
  redemptions, and recipient recreation, not deposit credit and owner sequence.
- INV-089 supplies the public self-delegate allowance technique, but its subject
  is activation generations and fees. No activation path is exercised here.
- Process inspection found an active INV-039 cure/close CPI test; that area and
  its files were excluded. Open PR file lists and recent branch subjects were
  checked without importing their implementations. None owns these changed files.

## Test Contract

The single test runs deposits of 1 and 137 atoms. Public System transfers fund a
native ATA with `2 * amount + 19` SOL atoms while its SPL amount remains zero.
Public SPL approval sets a self-delegate allowance one atom below the deposit.

1. Without SyncNative, the captured deposit fails wrapper balance validation.
   Raw lamports and rent cannot become withdrawable capital.
2. SyncNative succeeds, then the deposit passes balance validation, credits
   capital and advances the owner sequence before its SPL transfer rejects the
   insufficient allowance. The test requires the exact SPL error and nested CPI
   log, one successful token prefix, and no successful wrapper invocation.
3. Every rejection restores all compiled transaction accounts plus mint, admin,
   and vault authority, including absence, data, ownership, rent and lamports.
   The payer loses exactly the signature fee. The source is unsynchronized again;
   vault, capital and owner sequence retain their pre-deposit values.
4. Public revocation repairs the allowance without synchronizing the source.
   The unchanged SyncNative/deposit instruction bytes and metas then commit.
   Only the signed deposit amount becomes capital. Source/vault SPL amounts and
   backing lamports match independently constructed account images exactly.
5. The consumed deposit rejects as stale even though enough source tokens remain
   to fund it. A fresh full withdrawal and SPL native redemption return all the
   owner's SOL and source rent, leaving zero capital and an empty rent-only vault.

Retry transactions are freshly signed against a current blockhash; only their
instruction payloads and metas are retained. This is not signature-cache replay
or durable-nonce evidence. The existing fixture supplies native-mint genesis
because LiteSVM omits it; every economic state transition uses public programs.
The test does not cover terminal modes, maximum shapes, arbitrary histories, or
all SPL failure classes. SVM rollback remains a named platform assumption.

## Exact Validation

Commands run in the isolated worktree, with a fresh private build directory:

```bash
export CARGO_TARGET_DIR=/dev/shm/codex-invariant-public-20260916-2300-target
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
selector=inv_080_error_propagation_and_exact_rollback::native_deposit_sync_retry::v16_program_native_deposit_cpi_failure_restores_sync_and_signed_retry
cargo test --locked --offline --test v16_cu "$selector" -- --exact --nocapture
```

Fresh default-feature SBF SHA-256:
`87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.
Engine pin: `94979ede7db934545e53a8f210dd063a9ea3ea63`.

The first development execution reached the intended SPL failure and passed its
rollback assertions, then exposed an overly strict success image: SPL revocation
clears the delegate tag but preserves inactive key bytes. The expected source
image now starts from the public revoke result, checks decoded equality with the
empty token state, and derives token amount and lamports independently. The
corrected exact selector passed **1/1**, with peak **47,392 CU**.

One temporary test-only negative control changed the approval allowance and its
setup assertion from `amount - 1` to `amount`. Running the same exact selector
failed **0 passed / 1 failed** at `deposit must reject`: SyncNative and the nested
SPL transfer both succeeded, as did the wrapper. Thus a successful deposit cannot
masquerade as late-failure evidence. The mutation was restored; production was
never changed.

After rebasing onto `48f43292` and again onto final main `c86065d5`, each final
execution used the same four exact selectors:

```bash
cargo test --locked --offline --test v16_cu -- --exact --nocapture \
  "$selector" \
  inv_081_success_state_validity_over_complete_public_routes::v16_program_native_quote_roundtrip_preserves_lamports_rent_and_unsynced_value \
  inv_080_error_propagation_and_exact_rollback::v16_program_explicit_engine_error_dispositions_are_source_complete \
  inv_080_error_propagation_and_exact_rollback::v16_program_dispatch_and_entrypoints_preserve_every_handler_error
rustfmt --edition 2021 --check --config skip_children=true \
  tests/invariants/cu/inv_080_error_propagation_and_exact_rollback.rs \
  tests/invariants/cu/inv_080_native_deposit_sync_retry.rs
git diff --check
git diff --cached --check
git diff --exit-code 8b221355 -- src Cargo.toml Cargo.lock
LC_ALL=C rg -n '[^\x00-\x7F]' \
  tests/invariants/cu/inv_080_native_deposit_sync_retry.rs \
  tests/invariants/inv_080_native_deposit_sync_audit_20260916.md
```

Final result: **4 passed, 0 failed, 0 ignored** (two SBF tests and two source
guards). The single new test runs **2 worlds, 6 exact rollbacks, 2 unchanged
deposit retries, and 2 full withdrawals/native redemptions**. The final run peaks
at **42,892 CU**; the maximum observed across passing runs is **50,392 CU**, below
the existing **300,000 CU** custody bound. Scoped rustfmt and both diff checks
pass; the ASCII search returns no
matches. No broad suite, listing-only, or zero-test run is used as evidence.

The final patch has exactly one new `#[test]`, its invariant-owned module mount,
and this audit. No shared README, status ledger, production source, fixture, or
dependency pin changes. The native SyncNative/deposit failure composition remains
net-new against final main `c86065d5`.
