# INV-080 backing bundle CPI rollback and retry

Primary owner: `cu/inv_080_error_propagation_and_exact_rollback.rs`, through
`backing_bundle_cpi_retry`. One new public LiteSVM test covers a second backing
funding CPI failure after the first funding transfer succeeds. This is bounded
coverage evidence, not a confirmed bug or an invariant-status promotion.

Private Git store: `/dev/shm/codex-inv-route-01a0ac7d.git`.
Worktree: `/dev/shm/codex-inv-route-01a0ac7d`.
Branch: `codex/invariant-route-rollback-20260916-01a0ac7d`.
Initial latest-main base: `c86065d5`.
Final base: `85eb0db528dc2ac6902f025ccf123182ab551328`.
The repository was cloned directly from GitHub; the original checkout and other
agents' directories were not accessed. Builds use a fresh private target.

## Gap And Duplicate Audit

Read `INVARIANTS.md`, the coverage README's current-goal, ownership, rollback,
and recent coverage sections, and `scripts/loop.md`. Checked open PR file lists
and recent remote branch subjects before editing. The excluded INV-006/020/028/
036/045/051/061/070/076/079/089 work is not changed.

Relevant searches, followed by inspection of the matching witness bodies:

```bash
rg -n 'InsufficientFunds|insufficient allowance|self.delegate' tests/invariants -g '*.rs'
rg -n 'fn |CPI|cpi|top.up' tests/invariants/cu/inv_080_error_propagation_and_exact_rollback.rs
rg -n 'backing.*(bundle|prefix|allowance|CPI failure)|second.*(backing|top.up)|backing.*second' tests/invariants -g '*.rs' -g '*.md'
rg -n 'backing.*top.?up|top.?up.*backing|expired.*(top|refill)|refill.*expir' tests/invariants/cu/inv_008* tests/invariants/cu/inv_063* tests/invariants/stateful/inv_008*
rg -n 'Transfer|TopUpBackingBucket|backing|ledger|SyncNative' tests/invariants/cu/inv_080_native_deposit_sync_retry.rs
```

- Existing `v16_bpf_failed_backing_topup_transfer_preserves_same_intent_retry`
  creates token-shaped data under an injected foreign program owner and accepts
  any error. Current `unpack_token_account` rejects that owner during wrapper
  preflight, before engine mutation, ledger initialization, or token CPI. Its
  subsequent repair also uses `set_account`. The existing selector still passes.
- The insurance funding-prefix witness creates a ledger and performs two
  successful top-ups, then fails in a later engine instruction. It does not
  exercise a rejected backing transfer or two domain sidecars sharing one lane.
- INV-008's underfunded-rail retry rolls back a successful backing top-up when
  a subsequent portfolio withdrawal rejects its custody balance. Its backing
  CPI succeeds; it does not reject an inner backing transfer.
- INV-063's retained backing matrix checks authenticated expiry and maturity
  boundaries. It does not create a publicly delegated source whose second
  funding CPI fails after a successful sibling-domain transfer.
- INV-089 activation and INV-076 cure use the self-delegate technique on other
  handlers and protect generation or close/position state. They do not exercise
  the shared backing watermark or two lazy sidecar initializations.
- Main advanced during development to `934b8d89`, adding the native deposit
  SyncNative witness. That witness protects native backing lamports and a
  portfolio sequence. The branch was rebased onto it, preserving its module
  mount. The new backing test remains distinct and its file is separate.
- Final main `85eb0db5` strengthens the ordinary Deposit CPI witness. It covers
  one portfolio credit and retry; no backing bucket, domain sidecar, shared
  asset control lane, or successful preceding funding CPI is involved. The
  branch was rebased again and that exact selector joins final validation.

The handler updates the backing bucket, source credit, sidecar principal, and
shared `backing_top_up` sequence before `transfer_tokens(...)?`. Swallowing that
error could leave unfunded backing and a consumed request; partial bundle commit
could also leave funded state without the caller's intended atomic allocation.
These are potential regression consequences, not observed production defects.

## Executed Obligation

Four worlds cross domain order `[0, 1]` / `[1, 0]` with remaining SPL allowance
one atom / one atom below the second transfer. Both domains belong to asset 0.
Public System/SPL/ATA/wrapper instructions create all economic accounts. Supply
is fixed at 181 atoms: 37 and 41 provider atoms plus 103 independent user atoms.
The mint authority is revoked before the experiment; no economic bytes are
injected. The independent user deposits capital before the retained bundle.

The bundle creates two rent-funded sidecars and deposits into both domains
using consecutive values from the same asset control lane. It first simulates
successfully. Public SPL Approve then installs an insufficient self-delegate
allowance without changing source balance. A positive remainder is necessary:
SPL clears an exactly exhausted delegate and may authorize the next transfer
through the owner branch.

The actual bundle must fail at transaction instruction 5 with SPL
`InsufficientFunds`. Logs require two nested token invocations, two Transfer
instructions, and exactly one successful token invocation. Complete Accounts
for all compiled transaction keys and additional fixture accounts are restored,
including both sidecars' absence, source allowance, market/ledger bytes, and
rent. The payer loses exactly four signature fees, not the sidecar rent.

Public Revoke repairs the source. The unchanged bundle instruction bytes and
metas then succeed, charging rent once and advancing only the shared backing
lane by two. Full sidecar records equal independent input-derived expectations;
each bucket and source-credit reservation equals its own funding amount.
Reverse-order withdrawals return exactly 37 and 41 atoms, preserve the peer's
sidecar, and clear both backing reservations. Both old top-ups then reject as
stale despite the source again holding all 78 atoms. The independent portfolio
remains byte-identical until its owner withdraws all 103 atoms, leaving zero
internal and SPL custody. Every failure and continuation enforces 300,000 CU.

Transactions are freshly signed against a current blockhash; the instruction
payloads and account metas are retained. This is not transaction-cache replay,
durable-nonce coverage, or a proof of arbitrary histories. SVM rollback remains
the platform assumption named by INV-080. Utilization, expiry, multiple assets,
terminal modes, other SPL failure classes, and maximum shapes remain outside
this increment. Production, shared fixtures, pins, and status ledgers are unchanged.

## Exact Verification

```bash
export CARGO_TARGET_DIR=/dev/shm/codex-inv-route-01a0ac7d/target
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
selector=inv_080_error_propagation_and_exact_rollback::backing_bundle_cpi_retry::v16_program_second_backing_cpi_failure_restores_sidecars_and_shared_intent_retry
cargo test --locked --offline --test v16_cu "$selector" -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --exact --nocapture \
  "$selector" \
  inv_080_error_propagation_and_exact_rollback::v16_bpf_failed_deposit_spl_transfer_rolls_back_engine_credit \
  inv_080_error_propagation_and_exact_rollback::v16_bpf_failed_backing_topup_transfer_preserves_same_intent_retry \
  inv_080_error_propagation_and_exact_rollback::v16_program_insurance_funding_prefix_rolls_back_created_ledger_on_engine_error \
  inv_080_error_propagation_and_exact_rollback::v16_program_explicit_engine_error_dispositions_are_source_complete \
  inv_080_error_propagation_and_exact_rollback::v16_program_dispatch_and_entrypoints_preserve_every_handler_error \
  inv_080_error_propagation_and_exact_rollback::native_deposit_sync_retry::v16_program_native_deposit_cpi_failure_restores_sync_and_signed_retry
rustfmt --edition 2021 --check --config skip_children=true \
  tests/invariants/cu/inv_080_error_propagation_and_exact_rollback.rs \
  tests/invariants/cu/inv_080_backing_bundle_cpi_retry.rs
git diff --check
git diff --cached --check
git diff --exit-code origin/main -- src Cargo.toml Cargo.lock tests/support \
  tests/invariants/README.md tests/invariants/open_findings.tsv \
  tests/invariants/independent_discoveries.tsv tests/invariants/coverage_reopenings.tsv \
  tests/invariants/invariant_status.tsv
LC_ALL=C rg -n '[^\x00-\x7F]' \
  tests/invariants/cu/inv_080_backing_bundle_cpi_retry.rs \
  tests/invariants/inv_080_backing_bundle_cpi_audit_20260916.md
```

Fresh default-feature SBF SHA-256:
`87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.
Engine pin: `94979ede7db934545e53a8f210dd063a9ea3ea63`.
Source, dependencies, and fixtures are identical across initial and final bases.

Two temporary test-only negative controls each run the new exact selector:

1. Replace the second `remaining_allowance` value with the full second amount.
   Result: **0 passed, 1 expected failure** at the required-error assertion.
   Both nested transfers and wrapper calls succeed, rejecting vacuous failure
   evidence.
2. Restore the allowance, then expect `AMOUNTS[1 - domain]` as each sidecar's
   principal. Result: **0 passed, 1 expected failure** at full ledger equality
   after successful retry. Swapping principal preserves the total of 78 but
   violates the required per-domain allocation.

Both controls are restored. The initial exact selector passed **1/1**, with
**86,729 peak CU**. The six-selector run on `934b8d89` passed **6/6**, with
**77,729 peak CU** for the new witness. Final validation on `85eb0db5` passed
**7 tests, 0 failed, 0 ignored** (five SBF tests and two source guards), with
**83,729 peak CU** for the new witness. The maximum observed passing-run value
remains 86,729, below the 300,000 bound.

The final new selector covers **4 worlds, 4 second-CPI rollbacks, 4 captured
bundle retries, 8 stale replays, 8 exact provider payouts, and 4 full user exits**.
Scoped rustfmt, staged/unstaged whitespace checks, ASCII checks, and unchanged
production/fixture/pin/status checks pass. Build and execution logs are kept
under the private `target/audit/` directory. No broad suite, listing-only run,
or zero-test run is counted as evidence.
