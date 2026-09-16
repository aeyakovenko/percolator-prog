# INV-080 deposit CPI test-body fidelity

Base: `c86065d50768dd703577d829a73ea1a29b984e4b` (`origin/main`, refreshed before
final validation). Worktree: `/dev/shm/codex-inv-fidelity-20260916/worktree`.
Primary owner: `cu/inv_080_error_propagation_and_exact_rollback.rs`.

## Gap and duplicate audit

The existing selector
`v16_bpf_failed_deposit_spl_transfer_rolls_back_engine_credit` injected a source
with a random account owner. `handle_deposit` rejects that source in
`verify_user_token_account`, before engine credit and before the transfer CPI.
Its `is_err()` and unchanged zero balances therefore did not test the failure
boundary claimed by its name. The selector remains mounted and keeps its name;
this patch replaces that body rather than adding another early-rejection test.

Required guidance read: `tests/invariants/README.md` (including ownership rules),
`INVARIANTS.md` (especially INV-018/079/080), and `scripts/loop.md`.
`scripts/search.md` and repository `AGENTS.md` files were absent.
Open PR file lists were checked; this Rust file is disjoint from those write sets.

The following searches identified the existing evidence and the gap:

```bash
rg -n 'self.delegate|self_delegate|delegated_amount|TokenError::InsufficientFunds' tests/invariants --glob '*.rs'
rg -n 'self.delegate|self-delegate|Instruction: Transfer|CPI.*deposit|deposit.*CPI' tests/invariants --glob '*.rs' --glob '*audit*'
rg -n '^fn ' tests/invariants/cu/inv_080_error_propagation_and_exact_rollback.rs
rg -n 'fn handle_deposit|fn verify_user_token_account' src/v16_program.rs
```

Comparison against the final base:

| Existing owner | Distinction |
| --- | --- |
| INV-018 retained source account lifecycle | Checks `InvalidTokenAccount` preflight after public owner/account changes; does not require a failing SPL transfer after credit. |
| INV-080 public close/deposit prefix and insurance funding prefix | A successful prefix rolls back after a later engine error; the deposit's own transfer succeeds. |
| INV-089 activation fee CPI rollback | Supplies the same public allowance technique, but covers asset installation, rent and generation frontiers. It does not check portfolio deposit credit or sequence rollback. |
| Newly landed INV-076 cure CPI retry (`c86065d5`) | Covers cure and active-close retry. The deposit handler and this existing inaccurate witness are unchanged. |

No production, dependency, shared harness, invariant status or benchmark ledger
changes. This is one bounded evidence repair, not a new LoF/DoS finding or a
whole-invariant closure. Other deposit shapes and arbitrary histories remain
outside the test.

## Executed evidence

The replacement creates the market, mint, vault, portfolio and source through
System, ATA, SPL and wrapper instructions, with only ordinary SVM SOL funding.
SPL mints 107 atoms, then the owner approves itself for 99. A 100-atom deposit
passes source-owner and balance preflight, credits engine capital and advances
the portfolio sequence, then reaches SPL Transfer and receives InsufficientFunds.

Assertions require `InstructionError(2, Custom(1))`, nested SPL invocation and
Transfer logs, complete Account equality for transaction keys and the mint,
admin and vault authority, and exactly the network signature fee on the payer.
Market vault accounting, total capital and portfolio capital remain zero; the
portfolio sequence is unchanged. Public SPL revocation repairs the allowance.
The same encoded deposit request succeeds with a fresh blockhash, advances the
sequence once, moves exactly 100 atoms and credits exactly 100 capital. A normal
Live withdrawal then returns all principal, leaving zero vault/portfolio capital
and the original 107 atoms in the source with unchanged mint supply.

Validation results:

- Original selector at `48f43292`: 1 passed, 0 failed.
- Temporary old-body negative control: add an assertion that the rejected
  transaction logs contain `Program <SPL Token ID> invoke [2]`. Result: 0 passed,
  1 expected failure. The original fixture returned `Custom(11)`
  (`InvalidTokenAccount`) and no SPL invocation; 9,098 CU. Control removed.
- Replacement selector alone: 1 passed, 0 failed, 0 ignored.
- Final five exact selectors at `c86065d5` plus this patch: 5 passed, 0 failed,
  0 ignored. New witness: 1 late rollback, 1 retry, 1 payout. Final measured CU:
  rejection 40,386; retry 40,877; withdrawal 41,736, all below 300,000.
- Scoped rustfmt and git diff checks passed. All added text is ASCII.

The first SBF invocation without an explicit Solana `RUSTC` failed toolchain
selection. The first host run also lacked the explicit SBF path and failed before
fixture setup. Both configuration issues were corrected below; neither is
economic evidence. Host compilation reports the existing solana-client 1.18.26
future-compatibility warning.

## Exact commands and artifact

Run from the worktree above. Logs and both build directories are private to it.
No other checkout or agent artifact was used.

```bash
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export TMPDIR="$PWD/.verification/tmp"
env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  CARGO_TARGET_DIR="$PWD/target" \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline -- --locked
export CARGO_TARGET_DIR="$PWD/target-host"
export PERCOLATOR_FUZZ_SBF="$PWD/target/deploy/percolator_prog.so"
selector=inv_080_error_propagation_and_exact_rollback::v16_bpf_failed_deposit_spl_transfer_rolls_back_engine_credit
# Run unchanged baseline, temporary old-body CPI guard, and repaired body separately.
cargo test --locked --offline --test v16_cu "$selector" -- --exact --nocapture
# Final selection:
cargo test --locked --offline --test v16_cu -- --exact --nocapture \
  "$selector" \
  inv_080_error_propagation_and_exact_rollback::v16_program_explicit_engine_error_dispositions_are_source_complete \
  inv_080_error_propagation_and_exact_rollback::v16_program_dispatch_and_entrypoints_preserve_every_handler_error \
  inv_080_error_propagation_and_exact_rollback::v16_program_insurance_funding_prefix_rolls_back_created_ledger_on_engine_error \
  inv_018_quote_mint_vault_token_program_and_authority_integrity::v16_deposit_revalidates_retained_source_across_spl_account_lifecycle
rustfmt --edition 2021 --check --config skip_children=true tests/invariants/cu/inv_080_error_propagation_and_exact_rollback.rs
git diff --check
git diff --exit-code origin/main -- src Cargo.toml Cargo.lock
sha256sum target/deploy/percolator_prog.so
```

Default-feature (`anchor-v2`) SBF, built from `48f43292`; production and dependency
inputs are byte-identical at `c86065d5`. Engine pin:
`94979ede7db934545e53a8f210dd063a9ea3ea63`. Build driver:
`solana-cargo-build-sbf 2.3.13`, platform tools `v1.52`.
SBF SHA-256:
`87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.
