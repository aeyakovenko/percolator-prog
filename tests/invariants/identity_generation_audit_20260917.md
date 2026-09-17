# Identity and generation retained-intent audit, 2026-09-17

Base: fetched `origin/main` at `7c4e6291c186a24b092dc65125045d000b5f9552`.
Branch: `audit/identity-retained-intent-20260917`.
Worktree: `/dev/shm/percolator-identity-audit-20260917`.
Scope: INV-001..005/007/012/013/016/089 and the 26 rows below.

All named rows have mounted public-route witnesses. The **30 exact selectors run
here pass; no current failing witness was observed in that selection**. No missing
public-route LoF/DoS case was established, so this patch adds only this audit and
its README entry. Existing tests and discovery/open/reopening ledgers were held
fixed as audit evidence. This is a metadata-informed coverage audit, not independent
rediscovery or a status promotion.

## Named rows

Short test names below resolve to the full selectors in the verification block.
The [shared discovery helpers](../support/invariant_discovery.rs) define the
operation registries, retained transactions and economic rollback frames.

| Rows / owner | Mounted witness and assertion boundary |
| --- | --- |
| 293/294/295/296/307/315/317/323/324, INV-001/007 | [Whole-market ABA matrix](public_sbf/inv_007_no_aba_reuse.rs), `whole_market_recreate_aba_matrix`: 11 operation kinds include rebalance, matcher enable, recovery forfeit, trade fees, deposit, shutdown, fee redirect, resolve and resolve policy. Public close and System refund preserve a typed program-owned tombstone; reinitialization and every retained request reject with tracked economic rollback and fixed SPL supply. This proves strict non-reuse, not successful same-address generation replacement. The CU `closed_market_address_is_permanently_tombstoned` control also initializes a different address. |
| 231, INV-002 | [Asset-generation regressions](public_sbf/inv_002_asset_generation_binding.rs), `pr231`: single/batch, CPI/no-CPI stale trades reject specifically for generation mismatch after reuse; fresh requests land and change economic state. |
| 275, INV-002 | `pr275`: retained AuthMark/EWMA pushes use maximum sequences; rejection cannot be explained by an older observation watermark. Fresh generation controls mutate state. |
| 277/322, INV-002 | `pr277_pr322`: AuthMark/EWMA/Hybrid configuration; `pr277_restart`: same authorized admin and future sequence across oracle restart. Both require generation rejection, rollback and fresh liveness. |
| 279/320, INV-002 | `pr279_stale_insurance_top_up` plus `pr279_asset_zero_top_up`: reused nonzero slot and base-asset Recovery/restart reject retained top-ups. The base-asset fresh control debits exactly 1,000 tokens and credits the vault equally. Both benchmark labels map to insurance funding; they do not add another portfolio-deposit test. |
| 321, INV-002 | `pr321`: stale backing top-up rejects after replacement; fresh funding succeeds with an observable economic delta. |
| 318, INV-002 | `pr318`: retained backing-fee policy cannot cross asset generation; fresh policy remains usable. This is policy-state protection, not an independent fee-entitlement proof. |
| 311/312, INV-002 | Both `pr311_pr312` selectors retain resolve and resolve-policy controls over slot reuse or base-asset restart, require generation mismatch and unchanged state, then admit current controls. The separate funded replacement-PnL stateful witness is noted below, not claimed as executed here. |
| 251/345/346, INV-005 | [Authority matrix](public_sbf/inv_005_authority_incarnation_binding.rs), `authority_incarnation_matrix`: 34 A->B->A operation kinds, including market handoff and all five asset-role handoffs, reject old consent with tracked rollback and fresh mutation controls. `stale_backing_handoff` additionally protects a 500-atom deposit: replacement gain is zero and the incumbent recovers all 500. |
| 353, INV-005 | `stale_resolve`: old-authority resolve rejects before fresh terminal completion; victim loss and winner gain are exactly 100,000 under the fresh resolution. The test distinguishes unauthorized stale commitment from the current holder's authorized action. |
| 354, INV-004/013 | [Position-episode fixed matrix](cu/inv_004_position_episode_binding.rs): recovery forfeit, rebalance reduction and released-PnL conversion preserve the portfolio ID while advancing its episode. Old consent rejects with market/subject/vault/supply rollback; fresh consent changes exposure or converts positive PnL. Three episode kinds, not every possible claim/recovery history. |
| 375, INV-005 | `funded_role_matrix`: backing provider, insurance operator and terminal insurance authority resist cold-admin seizure. Unauthorized replacement payout rejects, replacement gain is zero, and each incumbent recovers its nonzero contribution. This registry does not cover oracle-dependent exposure. |
| 416, INV-005 | [Funded-oracle takeover witness](cu/inv_005_authority_incarnation_binding.rs), `funded_asset_admin_cannot_seize_oracle_and_redistribute_user_value`: matched independent exposure prevents cold-admin oracle takeover; rejected market Account is unchanged and incumbent self-rotation remains live. On this pin the rejection branch returns before terminal payout assertions. The separately executed [cold-oracle coholder witness](cu/inv_005_cold_oracle_funded_containment.rs) covers four flat-user worlds: oracle replacement may succeed, but a backing-role seizure suffix rolls back its oracle/observation/SPL prefix; incumbent backing and user capital are paid exactly. That flat-stock evidence cannot replace the exposure witness. |

The discovery ledger's row-312 selector is
`v16_program_stateful_fuzz::inv_002_asset_generation_binding::v16_program_asset_generation_terminal_policy_rejects_before_replacement_value_transfer`
(harness followed by test path). [Its body](stateful/inv_002_asset_generation_binding.rs)
requires generation mismatch, rollback, fresh admission and `Progressing` after
replacement users accrue opposite PnL. Source-reviewed only; no new result for it
or the randomized market/asset/authority matrices is asserted.

## Other scoped identity joins

| Owner | Executed evidence and limits |
| --- | --- |
| INV-003 | [Portfolio matrix](public_sbf/inv_003_portfolio_incarnation_binding.rs): 16 retained operation kinds across same-pubkey owner A->B->A, increasing IDs, rejection and fresh economic mutation. Deposit/withdraw/close are simulated before reuse; aligned custody sequences prevent an unrelated sequence guard from masking identity. The separate unchanged-portfolio deposit keeps its original signature through neighboring recreation and transfers exactly 1,000. |
| INV-012 | [Generated grant bindings](cu/inv_012_generated_grant_bindings.rs): 108 histories derive episode/frontier pairs from input events; each stale dimension rejects independently with complete Account frames and exact payer fee. Current grants admit 864 CPI fills. [Same-asset exits](cu/inv_012_retained_same_asset_episode.rs) isolate LP episode ABA under unchanged IDs/grant and restore both CPI exit routes by updating only the LP episode, ending in exact principal payouts. [Portfolio grant rollback](cu/inv_012_portfolio_grant_rollback.rs) runs four public reincarnation bundles: failed old grant restores rent/SPL/ID frontier; the original grant then lands and both owners exit. Finite histories do not establish every standing-capability scope or revocation writer. |
| INV-013 | [Committed funding round trips](cu/inv_013_destructive_consent_scope.rs): four histories restore balances but invalidate old close consent only when funding commits. Failed deposit/withdraw/close bundles restore both SPL transfers and sequence advances, allowing the original signed close; committed funding requires a fresh close. |
| INV-016/007 | [PDA tests](cu/inv_016_canonical_pda_and_seed_binding.rs): wrong-bump, cross-role and cross-market custody substitutions reject with exact instruction-account rollback. Same-address LP recreation intentionally repeats a stateless delegate PDA, but clears the capability; unauthorized CPI rejects and fresh grant restores entry/inverse exit. Repeating PDA seeds alone is not an incarnation failure. |
| INV-089/002 | [Activation differentials](cu/inv_089_activation_reactivation_and_initialization_equivalence.rs): privileged and permissionless reuse compare the complete persisted asset slot with fresh activation, normalizing only three assigned generation IDs. Public position/source history and maximum oracle watermark are erased; old authority rejects and replacement trading/payout stays live. The public `cross_slot_activation` regression also proves only one of two retained activations can consume a shared frontier, in either order, with exact fees and untouched sibling state. These checks do not cover every replay lane at its maximum. |

## Limits and status

Production review confirms exact asset/frontier guards, grant-time
portfolio/episode/sequence/frontier checks, and funded oracle classification via
asset-local exposure/loss state in [the wrapper](../../src/v16_program.rs).
Public-route execution is the behavioral evidence above; source rosters alone
are not behavioral certification. The fresh-market control preallocates its
alternate zeroed account through the fixture; PDA fault accounts and some CU
fixture setup are likewise injected. Lifecycle/replay actions execute deployed
instructions. Clock/provider/matcher scaffolding is not a validator or provider
protocol proof. Rejection frames differ by witness as described above.

[Statuses](invariant_status.tsv) remain unchanged: INV-005 is
`REFUTED_CURRENT` (411/416), INV-089 is `OPEN_EVIDENCE` (423), and the other
scoped invariants are `SUPPORTED`; all retain `SAMPLED / GLOBAL_CONDITIONAL_TCB`.
[Reopening rows](coverage_reopenings.tsv) 412/414 are `COVERED`, while 416 remains
`OPEN`. These broader metadata verdicts are not contradicted or discharged by
passing this finite selection. No arbitrary-history, maximum-shape, complete
disabled-authority product, cross-program replay or whole-invariant proof is claimed.

## Exact verification

Host artifacts were privately copied; the existing default-feature wrapper SBF was
reused, not rebuilt. Artifact source base `cb236b5248c941ffcb33e1e6afe62801e8a19712`
matches this base's `src/`, `Cargo.lock` and auth-matcher sources; the only manifest
delta enables host `syn` parser features. Engine: `94979ede7db934545e53a8f210dd063a9ea3ea63`.
Wrapper SHA-256: `87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.
Auth-matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

Commands from the isolated worktree:

```bash
cp -a --reflink=auto /dev/shm/percolator-public-gap-20260916-c91e-host-target /dev/shm/percolator-identity-audit-20260917-host-target
mkdir -p tests/fixtures/auth_matcher/target/deploy /dev/shm/percolator-identity-audit-20260917-logs
cp /dev/shm/percolator-main-merge-20260916/tests/fixtures/auth_matcher/target/deploy/auth_matcher.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
git diff cb236b5248c941ffcb33e1e6afe62801e8a19712 -- src Cargo.toml Cargo.lock tests/fixtures/auth_matcher
sha256sum /dev/shm/percolator-public-gap-20260916-c91e-sbf-target/deploy/percolator_prog.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
export CARGO_TARGET_DIR=/dev/shm/percolator-identity-audit-20260917-host-target
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-public-gap-20260916-c91e-sbf-target/deploy/percolator_prog.so
export AUDIT_LOGS=/dev/shm/percolator-identity-audit-20260917-logs
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture \
  inv_007_no_aba_reuse::v16_program_whole_market_recreate_aba_matrix_is_public_and_nonvacuous \
  inv_002_asset_generation_binding::v16_program_pr231_asset_generation_replay_rejects_on_every_route \
  inv_002_asset_generation_binding::v16_program_pr279_stale_insurance_top_up_rejects_across_asset_generation \
  inv_002_asset_generation_binding::v16_program_pr279_asset_zero_top_up_rejects_after_restart \
  inv_002_asset_generation_binding::v16_program_pr321_stale_backing_top_up_rejects_across_asset_generation \
  inv_002_asset_generation_binding::v16_program_pr318_stale_backing_fee_policy_rejects_across_asset_generation \
  inv_002_asset_generation_binding::v16_program_pr311_pr312_marketwide_controls_reject_after_asset_slot_reuse \
  inv_002_asset_generation_binding::v16_program_pr311_pr312_marketwide_controls_reject_after_asset_zero_restart \
  inv_002_asset_generation_binding::v16_program_pr275_stale_mark_pushes_reject_across_asset_generation \
  inv_002_asset_generation_binding::v16_program_pr277_pr322_stale_oracle_controls_reject_across_asset_generation \
  inv_002_asset_generation_binding::v16_program_pr277_restart_rejects_old_generation_even_with_future_sequence \
  inv_002_asset_generation_binding::v16_program_cross_slot_activation_consumes_one_retained_generation_frontier \
  inv_003_portfolio_incarnation_binding::v16_program_all_retained_portfolio_intents_reject_after_same_pubkey_recreate \
  inv_003_portfolio_incarnation_binding::v16_program_retained_deposit_survives_unrelated_portfolio_recreation \
  inv_005_authority_incarnation_binding::v16_program_authority_incarnation_matrix_rejects_stale_consent \
  inv_005_authority_incarnation_binding::v16_program_stale_resolve_rejects_before_fresh_terminal_exit \
  inv_005_authority_incarnation_binding::v16_program_stale_backing_handoff_rejects_before_incumbent_exit \
  inv_005_authority_incarnation_binding::v16_program_funded_role_matrix_preserves_incumbent_principal > "$AUDIT_LOGS/fixed.log" 2>&1
cargo test --locked --offline --test v16_cu -- --exact --nocapture \
  inv_004_position_episode_binding::v16_program_position_episode_matrix_rejects_stale_consent_fixed_case \
  inv_005_authority_incarnation_binding::v16_attack_funded_asset_admin_cannot_seize_oracle_and_redistribute_user_value \
  inv_005_authority_incarnation_binding::funded_oracle_succession::cold_oracle_funded_containment::v16_program_cold_oracle_replacement_preserves_funded_coholder_and_atomic_prefix \
  inv_012_capability_and_delegate_scope::joint_incarnation_binding::generated_grant_bindings::v16_program_generated_retained_grants_bind_each_episode_and_generation_prefix \
  inv_012_capability_and_delegate_scope::retained_same_asset_episode::v16_program_retained_exit_cannot_follow_lp_through_same_asset_flat_reopen \
  inv_012_capability_and_delegate_scope::joint_incarnation_binding::portfolio_grant_rollback::v16_program_failed_portfolio_reincarnation_preserves_retained_owner_grant \
  inv_013_destructive_consent_scope::v16_program_close_consent_tracks_only_committed_funding_round_trips \
  inv_016_canonical_pda_and_seed_binding::v16_program_public_pda_substitution_matrix_rejects_without_mutation \
  inv_016_canonical_pda_and_seed_binding::v16_program_reused_matcher_delegate_cannot_revive_closed_portfolio_capability \
  inv_089_activation_reactivation_and_initialization_equivalence::v16_program_privileged_reuse_matches_fresh_after_public_position_and_sequence_history \
  inv_089_activation_reactivation_and_initialization_equivalence::v16_program_reused_slot_matches_fresh_persisted_state_after_public_history > "$AUDIT_LOGS/cu.log" 2>&1
cargo test --locked --offline --test v16_cu inv_007_no_aba_reuse::v16_program_closed_market_address_is_permanently_tombstoned -- --exact --nocapture > "$AUDIT_LOGS/fresh-market.log" 2>&1
git diff --check 7c4e6291c186a24b092dc65125045d000b5f9552
git diff --exit-code 7c4e6291c186a24b092dc65125045d000b5f9552 -- src Cargo.toml Cargo.lock tests/support tests/fixtures tests/v16_cu.rs tests/v16_program_fuzz_regressions.rs tests/invariants/cu tests/invariants/public_sbf tests/invariants/stateful 'tests/invariants/*.tsv'
git diff --cached --check
```

Results: fixed selectors **18 passed / 0 failed / 129 filtered**, 15.78s;
CU group **11 passed / 0 failed / 1,428 filtered**, 71.29s; fresh-market control
**1 passed / 0 failed / 1,438 filtered**. All three commands exit 0.
Whitespace/staged checks and the production/test/ledger comparison pass.
Only this Markdown audit and its README entry change. No broad suite, randomized
suite, Kani run or new test is claimed. Existing unused-support and Solana
future-compatibility warnings remain.
