# INV-089: late activation fee failure preserves retryability

Base: `origin/main` at `056cd6ba`. Scope: INV-089, with INV-079 public construction
and INV-080 error-propagation/rollback evidence. No open PR/issue content was read.

The existing underfunded-append and invalid-authority/price tests reject in wrapper
preflight or activation validation. They do not reach the fee CPI after append or
reuse has installed a generation and credited insurance. The new invariant-owned
test covers that composition in two public worlds, without editing account bytes:
System market allocation, SPL mint initialization/minting, ATA creation, wrapper
initialization, and (for reuse) public activation and retirement. The creator grants
itself an SPL allowance of 39 with balance 47; the activation fee is 40. Wrapper
balance preflight passes, but SPL selects the matching delegate and rejects its
insufficient allowance. The exact SPL error and nested Transfer invocation prevent
an earlier wrapper failure from masquerading as this evidence.

Both worlds require every transaction account (plus mint, admin, and vault
authority) to roll back exactly, including market length, lamports, generations,
free-slot state, and insurance. The fee payer is checked separately for the exact
signature fee. A public SPL revoke repairs the source; the identical captured
activation instruction succeeds without rebinding its generation. Exactly 40
tokens move and are credited, the frontier advances once, and the now-stale
activation rejects without effects beyond its signature fee. Full persisted asset
slots match after normalizing only the three existing market-ID fields.

This adds one executable public-route comparison, not a production fix or a
new economic counterexample. It would catch lost CPI-error propagation, consumed
generation/free-slot state on rejection, duplicate fee credit, and divergent
replacement state. INV-089 remains `OPEN_EVIDENCE`. No production, dependency,
benchmark ledger, status, or row424 terminal-scan guard changed.

## Exact Verification

All commands ran in `/dev/shm/astra-ultra-inv063-089-cycle-20260916220031`.
Existing dependency artifacts seeded the private target; default-feature SBF
was rebuilt from this worktree with the unchanged engine pin `94979ede`.
SBF SHA-256: `87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.

```bash
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-inv063-089-cycle-20260916220031-target
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
guard=inv_089_activation_reactivation_and_initialization_equivalence::v16_program_activation_fee_cpi_failure_preserves_append_and_reuse_frontiers
cargo test --locked --offline --test v16_cu "$guard" -- --exact --nocapture
```

Initial execution: **1 passed, 0 failed**. Two separate, temporary test-only
negative controls each ran that same exact selector (**0 passed, 1 failed** each):

1. Change only the new test's `max_init_fee: FEE.into()` to
   `max_init_fee: (FEE - 1).into()`. The provenance assertion rejects wrapper
   `Custom(8)` instead of SPL `Custom(1)`, despite the transaction still failing.
2. Restore the fee cap; change only its `approve` allowance from `FEE - 1` to
   `if reuse { FEE } else { FEE - 1 }`. Append completes its full test path;
   reuse succeeds at the purported failure and trips `fee CPI must fail`.

Both substitutions are restored. No production mutation was used. Final commands:

```bash
cargo test --locked --offline --test v16_cu -- --exact --nocapture \
  "$guard" \
  inv_089_activation_reactivation_and_initialization_equivalence::v16_attack_permissionless_create_underfunded_fee_does_not_activate_or_credit \
  inv_089_activation_reactivation_and_initialization_equivalence::v16_program_reuse_matches_fresh_activation_envelope_and_drops_old_authority \
  inv_089_activation_reactivation_and_initialization_equivalence::v16_program_reuse_cooldowns_independently_preserve_generation_frontier
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_lifecycle_metadata_evidence_is_mounted_in_public_harnesses \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete
rustfmt --edition 2021 --check --config skip_children=true tests/invariants/cu/inv_089_activation_reactivation_and_initialization_equivalence.rs
git diff --check
git diff --exit-code 056cd6ba -- src Cargo.toml Cargo.lock tests/invariants/open_findings.tsv tests/invariants/independent_discoveries.tsv tests/invariants/coverage_reopenings.tsv tests/invariants/invariant_status.tsv
```

Final results: **4 passed / 0 failed** in `v16_cu`; **2 passed / 0 failed** in
`v16_program_fuzz_regressions`. No listing-only or zero-test result counts as
evidence. New-test internal counts: **2 worlds, 2 late CPI rollbacks, 2 successful
retries, 2 stale replays**; final-run peak CU **32,816**, maximum observed across
successful runs **33,541**, bounded by 300,000.
Formatting and diff checks pass.

Remaining gaps: this uses one appended/reused slot, one fee, and a standard SPL
quote mint. It does not establish maximum-shape CU, arbitrary concurrent
activation histories, all replay-lane exhaustion cases, native-quote equivalence,
terminal payout liveness, or whole-invariant closure. Frozen and underfunded
sources were rejected as late-CPI candidates because preflight catches them;
existing hint-order/preemption and row424 coverage was not duplicated.
