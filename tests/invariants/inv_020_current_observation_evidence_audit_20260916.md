# INV-020 current-observation evidence fidelity

Base: `origin/main` at `056cd6ba`. Worktree:
`/dev/shm/astra-ultra-inv001-023-cycle-20260916220000`.
Scope reviewed: INV-001 through INV-023. Open PR/issue bodies, fixes, and tests
were not consulted. Row426 is the existing local benchmark mapping.

The generic INV-079 gate checks recognized oracles and executable invariant
ownership, but does not bind the row426 witness to missing current evidence.
Its reviewed witness executes omitted and prior-slot rescue observations before
liquidation, requires exact rollback, and compares final SPL entitlement with a
fresh-observation control. The substitutable INV-020 composite witness tests
skewed supplied timestamps and complete coherent refresh; it does not exercise
omission or prior-slot replay of the rescue report.

The added INV-020 test requires exactly one row426 discovery with its reviewed
fingerprint, selector, and oracle, and one reopening with its missing-evidence /
multi-step-refresh / liquidation product and complete-current-observation
obligation. It does not change benchmark classifications or invariant status.
This is distinct from INV-008 insurance stock/debit evidence, INV-014 runnable
fee witnesses, INV-045 carry fidelity, INV-028 admission, and INV-070 terminal scans.

## Negative Controls

1. In `independent_discoveries.tsv`, replace only row426's selector
   `v16_attack_liquidation_cannot_omit_fresh_external_rescue_observation` with
   `v16_program_composite_timestamp_coherence_rejects_cross_epoch_liquidation`.
   Keep the fingerprint, oracle, owner, and benchmark ID unchanged.
   The five existing gates pass **5/5**. The new guard fails **0 passed / 1 failed**
   at the discovery assertion.
2. Restore the selector. In row426 of `coverage_reopenings.tsv`, replace only
   `favorable-account-actions-require-complete-current-authenticated-observations`
   with `favorable-account-actions-require-coherent-supplied-observation-timestamps`.
   Combined execution gives **5 passed / 1 failed**; only the new guard fails.

Both mutations were restored. Final metadata execution passes **6/6**.
The existing rescue witness passes **1/1** across its three worlds: both attacks
roll back exactly, preserve the 50-unit position, add zero insurance, and permit
the same 2,600,000-atom owner payout as the control; peak CU is 172,154.
The substitute coherence selector also passes **1/1**, with eight generated cases
and three skew/rollback words per case. These are corroborating existing
public-route tests, not new economic coverage.

## Exact Commands

All test invocations execute exact selectors with nonzero counts.

```bash
cd /dev/shm/astra-ultra-inv001-023-cycle-20260916220000
export CARGO_TARGET_DIR="$PWD/target" CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
guard=inv_020_authenticated_clock_slot_and_oracle_provenance::v16_row426_metadata_retains_current_observation_omission_evidence
metadata=(
  inv_079_public_reachability_evidence::v16_dated_open_security_finding_benchmark_is_non_overclaiming
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row
)
# Control 1, with substituted selector: 5 passed, then 1 expected failure.
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture "${metadata[@]}"
cargo test --locked --offline --test v16_program_fuzz_regressions "$guard" -- --exact --nocapture
# Control 2, with restored selector and weakened obligation: 5 passed, 1 expected failure.
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture "$guard" "${metadata[@]}"
# Restore both ledgers. Final metadata: 6 passed.
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture "$guard" "${metadata[@]}"

RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_TARGET_DIR="$PWD/sbf-target" cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$PWD/target/deploy" -- --locked
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_TARGET_DIR="$PWD/matcher-target" cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked
# 1 passed per selector; the latter executes 8 property cases.
cargo test --locked --offline --test v16_cu inv_056_hints_are_discovery_only_favorable_actions_fully_refresh::v16_attack_liquidation_cannot_omit_fresh_external_rescue_observation -- --exact --nocapture
PERCOLATOR_FUZZ_CASES=8 cargo test --locked --offline --test v16_program_stateful_fuzz inv_020_authenticated_clock_slot_and_oracle_provenance::v16_program_composite_timestamp_coherence_rejects_cross_epoch_liquidation -- --exact --nocapture
rustfmt --edition 2021 --check --config skip_children=true tests/invariants/public_sbf/inv_020_authenticated_clock_slot_and_oracle_provenance.rs
git diff --check
git diff --exit-code 056cd6ba -- src Cargo.toml Cargo.lock tests/invariants/independent_discoveries.tsv tests/invariants/coverage_reopenings.tsv tests/invariants/open_findings.tsv tests/invariants/invariant_status.tsv
```

Default-feature wrapper SHA-256:
`87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.
Auth matcher SHA-256:
`50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
Engine pin: `94979ede7db934545e53a8f210dd063a9ea3ea63`.

## Remaining Gaps

INV-020 remains `OPEN_EVIDENCE` with rows416/422 outstanding. This guard does not
validate test-body semantics, replace module-mount checks, or prove arbitrary
provider, partial-refresh, policy, reward, or retained-request histories.
The public executions remain bounded; there is no new LoF/DoS finding or fix.

Rejected candidates: same-address grant replay and cure-incarnation coverage
already have public lifecycle witnesses on main. A proposed expired-blockhash
test cannot establish validator expiry in pinned LiteSVM 0.1.0:
`expire_blockhash()` changes the advertised hash without transaction-age
validation. Runtime blockhash age and durable-nonce retained intents remain
outside this increment; no dependency or production changes were made.
