# Row 416 regression health, 2026-09-17

Base: freshly fetched `origin/main`, `7b9c84bc47cf99a7bc072c3d34645b8282e7f956`.
Worktree: `/dev/shm/percolator-row416-health-20260917`.
Branch: `worker/row416-health-20260917`; local commit only, no push.
Scope: funded oracle authority containment, INV-005/020/024/027/055/081.

## Coverage gap

Inputs: the [invariant README](README.md),
[identity audit](identity_generation_audit_20260917.md),
[terminal payout audit](terminal_payout_entitlement_audit_20260917.md),
[retained identity audit](retained_identity_replay_audit_20260912.md),
[Scope T](pr135_scope_t_funded_role_authority_containment_20260913.md), and
[funded-role source composition](inv_005_funded_role_source_composition_20260914.md).
Row 416 was a withheld coverage comparison, not an imported external PR patch.
No external PR implementation/test or row-427 side-OI file was used or changed.

The existing exposure witness returns after rejection and incumbent self-rotation;
its successful rejection branch never reaches terminal payouts. Flat backing and
insurance coholder tests and generated role epochs cover separate stock histories.
Retained insurance management already crosses empty-to-funded stock, but does not
cover oracle-dependent exposure appearing after valid cold-admin consent.
The terminal audit also cautions against counting unexecuted payout suffixes.

Production already routes oracle fundedness through
`asset_local_has_exposure_or_loss_state_view`; the cold-only funded handoff returns
`EngineLockActive` before consuming its authority epoch. The new test checks that
boundary through the deployed program. No production issue or stale expectation
in an existing selector was established; existing selectors were not rerun.

## New public-route evidence

Owner: [cu/inv_005_retained_oracle_exposure.rs](cu/inv_005_retained_oracle_exposure.rs),
mounted under `inv_005_authority_incarnation_binding::retained_oracle_exposure`.

Four worlds cross assets 0/1 with both signs of ten-unit matched exposure:

- Simulate a signed seven-atom withdrawal plus cold-admin oracle handoff while
  the asset is empty. Then publicly open independently funded exposure without
  changing the handoff's market ID or authority epoch.
- Land the identical bundle and standalone handoff. Require `EngineLockActive`
  at the handoff instruction, preserving every compiled Account exactly except
  the payer's independently calculated signature and priority fees. The rejected
  bundle has executed the SPL withdrawal prefix before the failing suffix.
- Retry the identical signed withdrawal successfully for exactly seven atoms.
  Incumbent oracle succession succeeds and advances only the selected authority
  epoch; the sibling profile and control sequences remain unchanged.
- The unconsented replacement cannot publish a higher mark. The consensual
  successor publishes the original price at authenticated slot 2. Permissionless
  resolution at slot 4 pays all five owners their exact deposited principal,
  including prior withdrawal, in both exposed-owner settlement orders.
- Require zero remaining capital, positive PnL and vault balance, unchanged mint
  supply, complete observed SPL conservation, and a validated public execution
  trace with exactly three rejected transactions per world and bounded success CU.

INV-005 owns the authority/consent boundary; INV-020 the scoped observation
provenance; INV-024 exact owner attribution; INV-027 preservation of senior
principal in this solvent, zero-PnL case; INV-055 current-state admission across
exposure creation and resolution; INV-081 public composition/rollback evidence.
These are bounded joins, not independent proofs of all six invariants.

## Results and artifacts

Final selector: **1 passed, 0 failed, 1,439 filtered**, 2.17s. All four worlds
complete: eight funded-handoff rejections, four unauthorized mark rejections,
four identical SPL withdrawal retries and twenty exact owner payouts. Peak
successful transaction cost: **142,495 CU** (limit 1,400,000). Current-engine SBF
build passes (7.84s); final host build passes (32.59s). Targeted rustfmt,
`git diff --check`, staged whitespace, protected-path diff and `git show --check`
checks pass. The existing Solana future-compatibility warning remains.

Only the new exact selector ran. Development iterations corrected new-test
expectations for `EngineLockActive`, the fixture's third compute-budget instruction,
and its priority fees. An unnecessary unchanged-price crank returned
`EngineNonProgress` and was removed. Failed LiteSVM 0.1 simulations debit payer
fees outside the trace; those redundant previews were removed while retaining
actual rejection execution and exact Account frames. None is a production finding.

Private host/SBF caches were copied with `cp -a --reflink=auto` from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target` to
`/dev/shm/percolator-row416-health-20260917-{host,sbf}-target`. SBF was rebuilt
locked/offline against current engine `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
The auth matcher was copied from main's ignored fixture deployment. No tracked
fixture, dependency, production or status file changed.

- Wrapper SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
- Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
- Logs: `/dev/shm/percolator-row416-health-20260917-logs/`, `build-sbf.log`,
  `retained-oracle.log`, `retained-oracle-{2,3,4}.log`, and `green.log`.

Exact verification commands from this worktree:

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/percolator-row416-health-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row416-health-20260917-sbf-target/deploy/percolator_prog.so
CARGO_TARGET_DIR=/dev/shm/percolator-row416-health-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row416-health-20260917-sbf-target/deploy -- --locked
cargo test --locked --offline --test v16_cu inv_005_authority_incarnation_binding::retained_oracle_exposure::v16_program_retained_empty_oracle_handoff_rechecks_exposure_before_payout -- --exact --nocapture
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_005_retained_oracle_exposure.rs tests/invariants/cu/inv_005_authority_incarnation_binding.rs
git diff --check
git diff --cached --check
git diff --exit-code 7b9c84bc47cf99a7bc072c3d34645b8282e7f956 -- src Cargo.toml Cargo.lock tests/fixtures 'tests/invariants/*.tsv'
git show --check --oneline HEAD
```

## Open gaps

Row 416 remains OPEN; all status TSVs are unchanged. This does not exhaust
loss-only/stored-position/claim/terminal-obligation classifier terms, zero-role
or coalesced-role histories, lifecycle products, other trade transports, arbitrary
oracle/clock schedules or maximum shapes. Successful empty-state handoff is a
simulation control, not a committed cold takeover. The original early-return
witness remains unchanged. No broad suite, Kani, mutation campaign or complete
success-state proof ran. V16Svm injects initial accounts and Clock scaffolding;
the traced economic transitions use public instructions. No failed-transaction
CU bound or validator/provider publication proof is claimed.
