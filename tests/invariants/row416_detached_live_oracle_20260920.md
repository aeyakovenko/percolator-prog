# Row 416: Detached Live Oracle Across Market-Authority ABA

Base: `origin/main`, `415a499ca5ed939522cc3ba15db39dee04b36125`.
Worktree: `/dev/shm/percolator-inv005-row416-20260920`.
Branch: `codex/inv005-row416-authority-containment-20260920`.
Scope: INV-005 cold-admin, funded-role and authority containment only.
PR #441 was treated solely as the supplied withheld title; no PR diffs were read.

## Existing Selector Inventory

The following selector suffixes are under `inv_005_authority_incarnation_binding::`
in `v16_cu`, except the explicitly qualified terminal control. These were inspected
before adding the test; inventory alone is not a claim of current runtime validation.

| Existing selector | Already covered |
| --- | --- |
| `cold_admin_earned_reserve::v16_program_cold_admin_rotation_preserves_earned_reserve_after_partial_principal_repayment` | Active lien, earned fees, correctly signed seizure/policy attempts and exact payout rollback. |
| `funded_backing_succession::v16_program_funded_backing_succession_preserves_paid_prefix_and_terminal_role_partition` | Incumbent-consented funded backing succession after a paid prefix; Live/terminal role partition. |
| `inv_024_attributed_quote_value_conservation::terminal_earnings_succession::resolution_submitter_reserve::v16_program_resolution_bundle_cannot_preserve_submitter_live_reserve_authority` | Terminal submitter role boundary across authority/permissionless resolution. |
| `funded_oracle_succession::cold_oracle_funded_containment::v16_program_cold_oracle_replacement_preserves_funded_coholder_and_atomic_prefix` | Flat portfolios, same/changed-price cold-admin oracle replacement and protection of the coheld backing role. |
| `retained_insurance_management::v16_program_retained_empty_insurance_management_rechecks_stock_before_ordered_succession` | Correctly signed empty-role management cannot seize a subsequently funded incumbent role. |
| `cold_admin_handoff_scope::v16_program_cold_admin_aba_and_burn_preserve_funded_handoff_scope_and_value` | Cold-admin ABA/burn with funded insurance/backing, scope-local epochs and payouts. |
| `cold_admin_handoff_scope::funded_role_zero_transition::generated_funded_role_epochs::v16_program_generated_funded_role_round_trips_preserve_entitlements_and_observation_scope` | Interleaved funded-role round trips and cold-admin ABA; flat economic portfolios. |
| `cold_admin_handoff_scope::funded_role_zero_transition::generated_funded_role_epochs::v16_program_market_authority_aba_cannot_reacquire_detached_funded_role` | Market-authority ABA with a detached backing/operator role and reserve payouts. |
| `retained_oracle_exposure::v16_program_funded_oracle_return_rejects_retained_mark_and_preserves_value` | Direct oracle ABA over two exposed assets and retained price-changing marks. |
| `retained_oracle_exposure::v16_program_retained_oracle_handoff_after_cold_admin_return_preserves_drain_exit` | Cold-admin ABA over exposure, rejected seizure, incumbent oracle succession and DrainOnly owner exit. |

## Added Relation

One new public-route test detaches the exposure-funded oracle from a coheld market
authority during A -> B -> A. B first inherits the oracle with A's consent, then
delegates only that oracle to C before returning market/cold authority to A.
Both assets have matched live exposure; the selected oracle has no reserve stock.

A's retained market handoff rejects `EngineStale` after it returns. A newly signed
current-epoch cold-admin oracle handoff rejects `EngineLockActive`. Each rejection
restores an executed seven-atom SPL withdrawal, complete tracked/compiled Accounts,
and the exact payer fee delta. Renewing the market handoff succeeds while preserving
C's oracle, proving automatic coheld-role rekeying cannot reacquire a detached role.
Current-epoch price-changing observations signed by A or B reject `Unauthorized`.
C's observation succeeds; both matched positions reduce publicly and all five
owners receive their exact original deposits. Both trade directions are tested.

This differs from the reserve-role ABA control because oracle funding comes from
live trader exposure, and differs from existing live-oracle/cold-admin ABA because
`UpdateAuthority` automatically rekeys coheld roles across the detachment. Public
fixtures, signed wrapper/System/SPL execution and existing trace validation are
used throughout. No state injection, production changes or invariant-status changes.
Row 416 remains open; this is bounded conformance evidence, not general closure.

## Validation

New exact selector: **1 passed**, two worlds, eight exact rejections, four SPL
prefix rollbacks, ten complete owner exits; peak successful transaction **125,821 CU**.
Adjacent exact selector: **1 passed**, two detached reserve roles; peak **43,242 CU**.
Both used the default-feature wrapper freshly rebuilt from the unchanged base with
platform-tools v1.52, plus a locally built authenticated matcher fixture.
Wrapper SBF SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.

```sh
export CARGO_TARGET_DIR=/dev/shm/percolator-inv005-row416-20260920-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
cargo test --locked --offline --test v16_cu inv_005_authority_incarnation_binding::retained_oracle_exposure::detached_live_oracle::v16_program_market_authority_aba_preserves_detached_live_oracle_and_owner_exits -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_005_authority_incarnation_binding::cold_admin_handoff_scope::funded_role_zero_transition::generated_funded_role_epochs::v16_program_market_authority_aba_cannot_reacquire_detached_funded_role -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_005_detached_live_oracle.rs
git diff --check
git diff --cached --check
```
