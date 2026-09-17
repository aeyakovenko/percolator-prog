# Retained matcher admission source contract

Baseline: latest fetched `origin/main`, `cc9d747d`. Worktree:
`/dev/shm/invariant-retained-authority-20260917-ZOpU2H/worktree`.
Branch: `codex/invariant-retained-authority-20260917-ZOpU2H`.

## Gap and scope

The INV-004 episode writer roster, INV-012 matcher roster, INV-002 generation
roster, and INV-014 fee witness roster establish source-string presence and
behavioral-witness ownership. They do not establish that retained admission
remains executable in the reviewed context, that every caller forwards its
signed operands, or that the checked fee reaches admission before CPI effects.

The new guard parses production Rust and owns the eight references connecting
the public dispatcher, `SetMatcherConfig`, both CPI handlers, the persisted
capability checker, config storage, and the two matcher invokers. It detects
additional references including function values, imports/aliases and macro
references. The three dispatch arms must pass their signed fields unchanged and
in the handler's reviewed parameter order.

The complete grant preflight must precede its storage helper. It includes owner
and account provenance, authenticated expiry, market generation frontier,
portfolio incarnation, matcher sequence and position episode. The successful
commit must persist the selected cap, expiry and current episode and propagate
the sequence increment's result. The capability helper must check its persisted
sequence, exact enabled tuple and authenticated expiry, returning the actual LP
cap. Both CPI handlers must consume their two signed episodes and the capability
as unconditional statements before mutable account borrowing, portfolio refresh,
request-sequence consumption or matcher invocation. Their LP fee checks and the
single-CPI taker's independent fee check must also precede those effects.

This does not duplicate a lifecycle history: INV-012's same/cross-asset episodes,
joint incarnation products, retained generation grant tests and owner/keeper
histories already exercise those public outcomes. INV-005 covers authority ABA
and funded containment; INV-010/013 compose the existing rosters; INV-014 already
has retained single-CPI economic histories; INV-081 covers public success-state
properties. This addition checks executable production composition. It is
unrelated to the recent INV-017/021 aliases or INV-079 trace/evidence census.
Open PRs were inspected only with `gh pr list --json number,title`; no open PR
branch, diff or reproduction was read or copied.

## Comparison controls

Two temporary, compiling production mutations were applied separately in this
worktree and then restored with `apply_patch`:

1. Change the grant's generation-preflight block from `{ ... }` to
   `if false { ... }`, preserving the original frontier read and rejection text.
2. Change the single-CPI taker check to
   `if false && cfg_pre.trade_fee_base_bps > fee_bps { ... }`.

For each mutation the exact four existing rosters below passed, while the new
source contract failed: **4 passed, 1 expected failure, 0 ignored** (exit 101).
The first reported `grant: executable identity/consent preflight changed`; the
second reported the missing unconditional `handle_trade_cpi` fee admission.
These are source-guard discrimination controls, not public exploit evidence.

The permanent negative-control test rejects 26 parsed-source mutations: skipped
generation/episode/sequence/expiry checks, widened fees, broken sequence error
propagation, conditional handlers, late LP checks, early success, swapped handler
parameters, substituted signed dispatch fields, and additional direct, aliased,
function-value or macro references. Original source and source with an extra
comment/unrelated function pass. In-memory controls are syntax/contract tests;
they are not all compiled as replacement programs.

## Exact verification

All commands run in the worktree above. Final source selection:

```sh
env CARGO_TARGET_DIR=/dev/shm/invariant-retained-authority-20260917-ZOpU2H/target CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo test --locked --offline --test v16_cu -- --exact \
  inv_002_asset_generation_binding::v16_program_asset_generation_field_and_guard_roster_is_source_complete \
  inv_004_position_episode_binding::v16_program_retained_position_binding_and_writer_rosters_are_source_complete \
  inv_012_capability_and_delegate_scope::v16_program_matcher_capability_route_roster_binds_every_current_scope \
  inv_014_delayed_policy_and_policy_epoch_safety::v16_program_retained_fee_consent_witness_roster_is_source_complete \
  inv_012_capability_and_delegate_scope::retained_admission_source::v16_retained_matcher_admission_is_source_complete_before_effects \
  inv_012_capability_and_delegate_scope::retained_admission_source::v16_retained_matcher_admission_guard_rejects_bypasses \
  --nocapture
```

The comparison command is exactly this command with the final
`v16_retained_matcher_admission_guard_rejects_bypasses` selector omitted. The
mutated production source is restored before the final six-selector run.
Final result: **6 passed, 0 failed, 0 ignored, 1,428 filtered out**, exit 0;
the mutation test reports **26 rejected mutations, 2 accepted controls**.

```sh
rustfmt --edition 2021 --config skip_children=true tests/invariants/cu/inv_012_retained_admission_source.rs
rustfmt --edition 2021 --check --config skip_children=true tests/invariants/cu/inv_012_retained_admission_source.rs tests/invariants/cu/inv_012_capability_and_delegate_scope.rs
git diff --check
git diff --exit-code origin/main -- src Cargo.lock
cmp Cargo.toml <(git show origin/main:Cargo.toml | sed 's/^syn = { version = "2", features = \["full"\] }$/syn = { version = "2", features = ["full", "extra-traits", "visit"] }/')
```

The manifest comparison permits only the stated `syn` dev-feature change.
No production source, runtime dependency, engine pin or lockfile changes.
Formatting, whitespace, production comparison and manifest comparison all exit 0.
A final `git fetch origin main` still resolves to
`cc9d747d90c3fbc5056dcb96f2cb1269fd856bd3`.

## Limits

This is a reviewed source contract, not a general control-flow, alias-analysis or
interprocedural proof. It enumerates the current matcher sinks and known effect
boundaries; new raw-byte writers or effects hidden in other helpers require
review. It does not replace the lifecycle LiteSVM tests, prove engine arithmetic,
certify arbitrary authority histories, or check every instruction decoder field.
Equivalent refactors of the locked statements require updating the contract.
No SBF build, LiteSVM execution or Kani proof is claimed for this change: the
affected tests are host source guards. No invariant status or finding changes.
