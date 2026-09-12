# Retained Capability, Identity and Replay Coverage Audit

Baseline: `origin/main` at `2b1d025c004f92d3f89bac00113be90a0cbbcf63`, verified against
GitHub's main ref on 2026-09-12. Engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
Inputs were local main source, tests, invariant records and GitHub PR metadata.
No holdout patches or test bodies were fetched or copied. This is a metadata-informed
coverage audit, not an independent rediscovery of those findings.

All five PRs were OPEN with base `main` when inspected:

| PR | Head | Reopening Row | Invariant Owner |
| --- | --- | --- | --- |
| [412](https://github.com/aeyakovenko/percolator-prog/pull/412) | `44c6e4af09d9` | 412 | INV-012, with INV-004/005/010 |
| [414](https://github.com/aeyakovenko/percolator-prog/pull/414) | `d15592f3ef52` | 414 | INV-012, with INV-002/007/019/089 |
| [416](https://github.com/aeyakovenko/percolator-prog/pull/416) | `4a011fd9a779` | 416 | INV-005, with INV-020/024/027/055 |
| [428](https://github.com/aeyakovenko/percolator-prog/pull/428) | `a05ef7d397be` | 415 | INV-008, with INV-010/011/024/031/064 |
| [432](https://github.com/aeyakovenko/percolator-prog/pull/432) | `630a14b7ba63` | 411 | INV-014, with INV-005/010/011/024/036/047/081 |

## Coverage Findings

### 412: Retained Grant Admission Is Not Modeled Across Every Revocation

The relevant owners in [stateful INV-012](stateful/inv_012_capability_and_delegate_scope.rs)
are `v16_program_retained_capability_histories_preserve_authorization_scope`,
`v16_program_ordered_grant_histories_bind_retained_cpi_disposition`, and the
[owner-episode child](stateful/inv_012_owner_episode_revocation.rs)
`v16_program_retained_capability_cannot_cross_owner_episode_revocation`.
Recovery-forfeit, liquidation, cure and keeper children add important writer cases.
They separate stale consumer requests from current-episode capability admission and
provide rollback and fresh-consent controls.

The ordered product's `write_grant` obtains a current `GrantOracle` immediately
before signing. `AuthorizationHistory::replay` applies accepted `Grant` events;
it has no independent admission relation between a retained grant's original
episode and later revocation events. The owner-episode product retains consumers,
not the grant-writing authorization. The INV-014 supersession generator covers
explicit competing matcher controls, while INV-004's writer roster proves source
membership, not that every writer is composed with retained grant admission.
The retained-grant-expiry child covers a different admission dimension.

Missing: one input-derived grant-issuance/admission model shared by retained writers
and consumers, with committed and rolled-back episode writers, grant replacements,
scope-local independence and public continuation. The new test below adds only
the atomic, explicit-control portion. Automatic-revocation holdout 412 stays OPEN.

### 414: Request Generation Checks Do Not Establish Standing Grant Scope

Relevant owners are INV-002's
`v16_program_generated_mixed_generation_batches_preserve_retained_scope` in
[stateful INV-002](stateful/inv_002_asset_generation_binding.rs), and INV-012's
`v16_program_joint_replacements_require_every_bound_incarnation` plus its
[scope, matcher-program and used-generation children](cu/inv_012_joint_incarnation_binding.rs).
These cover stale request identity, replacement ordering, ABA, exact rollback and
current-binding controls. They do not independently model the set of economic
asset incarnations admitted by the standing grant at authorization time.

There is also a positive-control assumption to revisit: `run_mixed_generation_history`
changes only the batch's asset-generation fields and expects restored CPI liveness.
That is sufficient for bilateral request identity but is not sufficient evidence
of the unsigned LP's consent to the replacement economic object. The joint
replacement matrix similarly varies grant replacement order without tracking an
independent grant-time asset scope. These controls must be reviewed against the
eventual consent policy, not mechanically preserved as liveness requirements.

Missing: separate request identity and grant scope in the oracle; compose asset
replacement, append and Recovery/restart with retained/fresh consumers and
unchanged-asset continuation. Keep both CPI transports and mixed-leg atomicity.
The shared stateful `Action::RetainTrade` currently constructs only no-CPI trades
([fuzz_model.rs](../support/fuzz_model.rs)); it cannot fill this gap. Row 414 stays OPEN.

### 416: The Funded-Role Registry Omits Oracle-Dependent Exposure

The appropriate generator is `v16_program_funded_role_matrix_preserves_incumbent_principal`
in [stateful INV-005](stateful/inv_005_authority_incarnation_binding.rs), backed by
`FundedRoleKind` in [invariant_discovery.rs](../support/invariant_discovery.rs).
Its registry contains backing provider, insurance operator and terminal insurance
authority. Oracle authority is absent. The authority-incarnation matrix and role
source roster cover authenticated ownership and epochs, which do not establish
economic containment of a correctly signed management action.

[Funded oracle succession](cu/inv_005_funded_oracle_succession.rs) adds an
incumbent-consented handoff and backing exit after admin renunciation. It does not
exercise the incumbent-consent requirement when live user exposure depends on the
oracle. The current production funded-role classifier also does not classify
oracle-dependent positions as funded role value.

Missing: role-independent attribution of which principal, live exposure, claims
and terminal obligations depend on each authority; generate management actions at
those boundaries and require consent from the affected incumbent. Preserve empty
setup and incumbent-approved succession as positive controls. Row 416 stays OPEN.

### 428 / Row 415: Generic Detection Already Exists on Main

`v16_program_retry_operation_matrix_rejects_every_stale_retry` in
[stateful INV-008](stateful/inv_008_intent_uniqueness_and_bounded_replay.rs) iterates
`RetryIntentKind::ALL`, including `InsuranceWithdrawal`. Its generic helper already
includes replenished insurance stock and requires consumed authorization to reject.
The test asserts an empty violation set. This is source-level detection coverage
for the holdout's core relation, not merely a missing roster entry. It was not run
as part of this audit, so this report does not claim a fresh failing or passing result.

[Withdrawal stock histories](cu/inv_008_withdrawal_stock_history.rs) extend portfolio
withdrawals with partial amounts, custody-only deposits, passive rewards and late
failure, but explicitly exclude insurance stock binding. The remaining work is to
apply a shared first-execution budget oracle to all retained debit families,
including insurance lifecycle/role/asset variations and exact owner attribution.
`inv_079_retry_kind_dispositions.tsv` also correctly separates replay evidence from
independent terminal value attribution. Row 415 stays OPEN; another fixed insurance
case would duplicate existing generic detection.

### 432 / Row 411: Generic Taker Fee Detection Already Exists on Main

`v16_program_fee_consent_operation_matrix_discovers_unsigned_debits` in
[stateful INV-014](stateful/inv_014_delayed_policy_and_policy_epoch_safety.rs) iterates
`FeeConsentKind::ALL`. That registry includes `RetainedCpiTakerBaseFee`, distinct
from the LP cap cases, and its oracle requires no violation of signed fee terms.
It is already the invariant-owned detection point for this holdout. It was inspected,
not executed; a green inventory or adjacent LP-cap selector cannot certify it.

Main also owns retained fee bundles, delegated exits, backing caps, activation fees
and partial/exact-fill policy cases. Remaining work is a shared participant-local
fee budget across generated route, policy, partial-fill and landing-order histories,
with independent fee attribution and rollback. The default retained queue's no-CPI
restriction remains relevant here too. Row 411 stays OPEN; no second isolated
single-CPI taker test is needed for detection.

## New Invariant Test

[inv_012_retained_grant_atomicity.rs](stateful/inv_012_retained_grant_atomicity.rs),
mounted in stateful INV-012, implements:

`inv_012_capability_and_delegate_scope::retained_grant_atomicity::v16_program_retained_joint_grants_consume_only_committed_owner_scopes`

The test derives its expectation from INV-008/010/012/080: rejected joint updates
consume no owner authorization; successful updates consume only their own scope.
It uses the existing public fixture and event-history oracle and has no finding
registry, holdout identifiers, production edits or imported PR test inputs.

Sixteen finite histories cross both owner scopes, both instruction orders,
enable/disable payloads and both CPI transports. All joint and standalone grant
messages are signed and successfully simulated before a competing owner's commit.
The joint update must reject at the selected instruction with exact error, full
account rollback excluding the separate network-fee payer, and a logged successful
prefix when rejection is at the second instruction. The unaffected owner's
unchanged, separately retained message must still succeed exactly once. Independent
transport retries require application rejection, not signature-cache rejection.
Every control step checks event-derived grant fields and unchanged economic
portfolio bytes. Public CPI fills establish that both final authorized scopes work.

This is distinct from the existing single-owner explicit-supersession matrix,
delivery-time expiry test, and position-revocation transaction test: it composes
independently retained grant writers for two owners with partial-prefix rollback
and reuse of untouched pre-signed consent. It provides bounded public-route
conformance coverage, not a complete randomized history generator. No automatic
episode revocation, generation replacement, nonzero fee debit, custody payout or
maximum-shape result is claimed. All five holdouts and existing verdicts stay open.

## Validation

Worktree: `/tmp/codex-agent-worktrees/identity-replay-audit-20260912-7f29b3`.
Branch: `codex/identity-replay-audit-20260912-7f29b3`.
Host build cache was privately copied from `/dev/shm/pr135-main-merge-20260912-target`.
Wrapper and authenticated matcher SBF were rebuilt offline in this worktree with
platform-tools v1.52, locked dependencies and default features.

- Wrapper SHA-256: `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
- Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

Exact verification commands, from the worktree:

```sh
export CARGO_TARGET_DIR=/dev/shm/identity-replay-audit-20260912-7f29b3-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
cargo test --locked --offline --test v16_program_stateful_fuzz inv_012_capability_and_delegate_scope::retained_grant_atomicity::v16_program_retained_joint_grants_consume_only_committed_owner_scopes -- --exact --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
```

Results: new selector **1/1**, invariant index **1/1**, formatting and whitespace
checks pass. The final new-selector run has 16 worlds, 64 live simulations,
48 exact rejections, eight rolled-back grant prefixes and 32 CPI fills; peak
measured CU is **150,995**. There are 120 post-setup public transactions including
eight explicit reauthorizations in the disabled-control worlds. The new selector
ran twice: once initially and once after strengthening successful grant checks to
compare complete economic portfolio bytes. Both passed; no other behavioral
selector ran. Existing unused-support and `solana-client v1.18.26` compatibility
warnings remain.

The holdout selectors, broad suites and Kani were not run. Main was rechecked at
the same SHA. Production, dependencies, shared helpers, fixture sources, reopening
rows and invariant verdicts have no diff from the baseline. The original worktree
still has its pre-existing lock-file deletion and `tests/v16_cu.rs` conflict.
No new public-interface LoF, DoS or CU bug was found; existing holdouts are not
new findings.
