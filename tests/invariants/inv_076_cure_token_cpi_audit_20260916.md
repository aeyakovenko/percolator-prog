# INV-076: cure token failure preserves the close and its retry epoch

Exactly one new public LiteSVM selector, owned by INV-076 with INV-018/080
boundary evidence. It is mounted beneath the existing INV-039 cure/resolution
fixture to reuse public account construction and independent entitlement checks.

Worktree: `/dev/shm/astra-ultra-liveness-20260916`.
Branch: `astra-ultra/inv076-cure-cpi-retry-20260916`.
Initial remote base: `ecd51b8c11ec2dbbf6cce9e4495730cef6d57839`.
Final base: `48f43292` after fast-forwarding the newly landed, disjoint INV-061
test and INV-079 guard. The Git store and build target are private siblings in `/dev/shm`.
The original checkout was not modified; only its origin URL was read.

## Gap evidence

Read `INVARIANTS.md`, the coverage README's account/lifecycle and canceled-close
sections, and `scripts/loop.md`. Source review establishes the exact boundary:
`handle_cure_and_cancel_close` calls `cure_and_cancel_close_not_atomic`, bumps
the portfolio position epoch, and only then calls `transfer_tokens(...)?`.

The searches used to check adjacent coverage were:

```bash
rg -n 'cure|CureAndCancelClose|transfer_tokens' \
  tests/invariants/cu/inv_076_close_drift_residual_durability_and_finalization_atomicity.rs \
  tests/invariants/cu/inv_039_pending_loss_cure_resolution.rs \
  tests/invariants/cu/inv_012_cure_revocation.rs
rg -n 'self.delegate|insufficient allowance|TokenError::InsufficientFunds' \
  tests/invariants/cu/inv_075* \
  tests/invariants/cu/inv_076_close_drift_residual_durability_and_finalization_atomicity.rs \
  tests/invariants/cu/inv_039_pending_loss_cure_resolution.rs \
  tests/invariants/cu/inv_012_cure_revocation.rs
rg -n 'self.delegate|self_delegate|insufficient allowance|InsufficientFunds|SPL.*(cure|cancel)|cure.*SPL' \
  tests --glob '*.rs'
rg -n 'CureAndCancelClose|cure' tests/invariants/README.md
```

The second search returns no matches (exit 1). The broader searches and witness
bodies distinguish the existing coverage:

- INV-076 zero-deposit and resolve-matured cures fail before the token CPI.
- INV-012's failed cure uses a zero deposit and tests capability revocation.
- INV-039's rejected suffix runs after both cure and SPL transfer succeed.
- INV-018's actual SPL multisig error is on Deposit, without an active close.
- INV-089 uses an insufficient self-delegate allowance on activation, protecting
  generation/free-slot frontiers. It does not test cure error propagation,
  cancellation, position-epoch retry, or the pending claimant's terminal payout.

Recent remote branch tips and queued write sets were inspected. The new file,
its three-line mount in `inv_039_pending_loss_cure_resolution.rs`, and this audit
are disjoint from the listed INV-006/020/028/036/045/051/061/070/079/089 work.
No open PR implementation was imported. The shared README is unchanged.

## Executed obligation

Four worlds cross mirrored loss sides with SPL allowances of zero and 99,999
against a 100,000-atom cure. The existing public System/SPL/ATA/wrapper fixture
fixes supply at 930,777, creates a 200,000 claimant gain and 20,000 debtor
residual, and retains the claimant's zero-basis obligation. No economic account
bytes are injected. A consenting donor funds the cure from existing principal.

The captured cure first simulates successfully. Public SPL Approve makes the
source its owner's own delegate with insufficient allowance while preserving
its full balance. The actual transaction must fail at instruction 2 with SPL
`InsufficientFunds`; logs must identify the nested token invocation and Transfer.
Every compiled and fixture Account rolls back exactly, including the full close
ledger, pending obligation, position epoch, custody, account metadata and rent.
The independent payer loses exactly the signature fee.

Public SPL Revoke repairs the source. The identical captured instruction, without
rebinding its epoch, must cancel the original close, credit exactly 100,000
capital/custody, retain -20,000 debtor PnL, and advance the epoch exactly once.
Replaying its old epoch must reject with `EngineProvenanceMismatch` and exact
rollback. Resolution and five permissionless payouts then return exactly
`[400000, 80000, 200000, 250000, 777]`, leaving zero internal and SPL vault
balance. Each economic prefix checks the independent owner entitlement equation
and the existing aggregate custody/obligation census. Unrelated Accounts remain
framed on successful cure and payout calls.

The test enforces a 300,000-CU ceiling for cure/failure/retry and terminal phases.
The first passing run measured 273,754 cure CU and 191,931 terminal CU. Shared
setup retains its existing bounds; this is not a maximum-shape CU claim.

## Verification

Default-feature SBF was rebuilt in the private worktree with unchanged engine
pin `94979ede`. SHA-256:
`87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.
Existing build artifacts seeded the private target from another `/dev/shm`
target; SBF and the host test executable were compiled from this worktree.

```bash
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-liveness-20260916-target
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
new=inv_039_pending_loss_obligation_durability::close_reopen::cure_resolution::token_cpi_retry::v16_program_cure_token_cpi_failure_preserves_close_epoch_and_terminal_payouts
cargo test --locked --offline --test v16_cu "$new" -- --exact --nocapture
```

Initial run: 1 passed, 0 failed. One temporary test-only negative control changes
the second allowance from `CURE - 1` to `CURE`. The same exact selector fails
(0 passed, 1 expected failure) because SPL Transfer and the wrapper both succeed
at the supposed failure. This rejects a vacuous rollback witness. The allowance
was restored before final validation; production was never mutated.

Final exact selectors and scoped checks:

```bash
cargo test --locked --offline --test v16_cu -- --exact --nocapture \
  "$new" \
  inv_039_pending_loss_obligation_durability::close_reopen::cure_resolution::v16_program_canceled_close_keeps_debt_through_pending_release_and_resolution \
  inv_076_close_drift_residual_durability_and_finalization_atomicity::v16_program_public_close_zero_cure_rejects_atomically_and_terminal_progress_remains
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_lifecycle_metadata_evidence_is_mounted_in_public_harnesses \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete
rustfmt --edition 2021 --check --config skip_children=true \
  tests/invariants/cu/inv_039_pending_loss_cure_resolution.rs \
  tests/invariants/cu/inv_076_cure_token_cpi_retry.rs
git diff --check
git diff --cached --check
git diff --exit-code origin/main -- src Cargo.toml Cargo.lock \
  tests/invariants/open_findings.tsv tests/invariants/independent_discoveries.tsv \
  tests/invariants/coverage_reopenings.tsv tests/invariants/invariant_status.tsv
```

Final results: **3 CU-harness tests passed, 0 failed, 0 ignored**, and **2 host
metadata tests passed, 0 failed, 0 ignored**. The CU selection ran on `8b221355`;
the only subsequent main change is the disjoint INV-079 metadata guard, whose
two exact host checks were rerun successfully on `48f43292`. No listing-only,
zero-test, full-suite or broad-format run is counted as evidence.

The new selector's final run covered **4 late CPI rollbacks, 4 captured-request
retries, 4 stale replays and 20 exact unsigned payouts**. Peak CU was setup
146,443, cure/retry 264,749 and terminal 183,030; maximum observed cure CU across
passing runs was 273,754. Scoped rustfmt, whitespace, ASCII, and unchanged
production/pin/status checks pass.

## Limits

This is one finite custody/liveness increment, not a discovered production bug
or a whole-invariant certification. It assumes the owner participates in cure
and SPL revocation and the administrator resolves; the resulting payouts are
unsigned. It does not prove absent-owner cure, arbitrary close histories,
irreversible close progress, native/secondary quote equivalence, adverse drift,
nonzero funding/fees, maximum shapes, retirement or CloseSlab. Production,
dependency pins, invariant statuses and finding ledgers remain unchanged.
