# Same-program role switching across used asset reuse

Base: `7374ab2f42bd2b9a6562afe8a8d5db1d43dad43b`, the requested branch's HEAD
when this work began. Worktree: `/tmp/percolator-astra-capability-incarnation.6piTse`.
Branch: `codex/astra-capability-incarnation-20260912-6piTse`.
Only the base's production ABI, invariant statements, fixtures and mounted tests
informed this increment. No open PR branch, diff or test was consulted or imported.
The coverage ledger and README supply holdout labels, not a finding reproduction.

## Net-new relation

`cu/inv_012_role_switch_generation.rs` is mounted under
`inv_012_capability_and_delegate_scope::joint_incarnation_binding::role_switch_generation`.
Its selector is
`v16_program_same_program_role_switch_revocation_survives_used_asset_reuse`.

Two distinct owner-bound contexts and delegate PDAs share one matcher program.
An LP first trades and flattens an asset through its own context, retaining a
valid signed CPI request and real context response bytes. It then signs as taker
through the peer's context. Even though the program address is identical and a
second fill returns both portfolios to flat, its own grant must remain revoked.
Retirement and reactivation of the previously traded slot compose before or after
this role switch. Neither returning to the original balances nor the new asset
incarnation supplies the missing owner authorization.

The generated product has 16 histories: target asset 1/2, retained single/batch
consumer, reuse before/after role switching, and positive/negative position size.
Role switching uses the opposite CPI transport from the retained consumer. Each
history has six committed fills, four rejected deliveries, two successful
simulations and both owners' complete SPL withdrawals.

The event-derived expectations distinguish all of these boundaries:

- Position episodes advance on each committed fill; portfolio identities and
  both owner-grant sequences stay fixed during role switching.
- Only the participating LP retains enabled state and expiry. The signing
  taker's grant disables and clears expiry while preserving tuple and fee cap.
- Invoking the same program through the peer's context leaves the original
  context Account byte-identical, including its earlier response.
- Reusing the target consumes exactly generation 4, preserves sibling
  generations and both complete portfolio Accounts, and cannot enable a grant.
- The unchanged retained transaction rejects with `EngineStale` under its
  original still-current blockhash. Current request identities still reject
  with `Unauthorized` until the owner explicitly reauthorizes.
- Reauthorization restores the identical tuple with a new sequence. The old
  sequence then rejects with `EngineStale`, and a current request carrying only
  the old asset generation rejects with `AssetGenerationMismatch`.
- Every rejection precedes matcher invocation and preserves all compiled and
  tracked Accounts, including both contexts, delegate accounts, owner lamports,
  mint and custody. Only the independently calculated network fee is deducted.
- A successful peer simulation proves the synchronized capability remains
  usable. Fresh original-LP consent permits entry into the replacement asset,
  an exit through the other CPI transport and each owner's exact 1,000,000-atom
  payout. Capital, PnL, signed quantities, OI, generation frontier, request count,
  zero insurance and fixed mint supply are reconciled from the inputs.

Public construction uses the parent's System/SPL/ATA/wrapper fixture, including
System-created portfolios and both contexts. Environment controls are signer SOL,
program loading and Clock; no engine account or matcher result is injected. Small
distinct CU limits distinguish rejected transactions without changing economic
fields or adding priority fees. The original retained transaction is unchanged.

## Existing Boundaries

| Base selector or child | Existing relation | Added relation |
| --- | --- | --- |
| `funded_owner_roundtrip` | Funded A-to-B-to-A portfolio ownership and old grants | Owners stay fixed while LP/taker roles reverse under two contexts of one program |
| `funded_oracle_succession` | Funded oracle/backing role containment | Portfolio-local matcher authority and asset reuse, without reserve succession |
| `matcher_program_generation` | Matcher program roundtrips with generation repair | Program stays identical while context participation decides automatic revocation |
| `portfolio_grant_rollback` | Failed portfolio recreation preserves old grant | Committed role switching and used-slot reuse cannot restore the revoked grant |
| `revocation_atomicity` | Bilateral writer rollback and committed position roundtrip | CPI signing-taker revocation with an independently live peer context, composed with reuse |
| `used_generation_lifecycle` | Used-slot reuse while a sibling stays exposed | Reuse commutes with a role switch that automatically revokes one of two grants |
| `v16_program_issue406_matcher_trade_routes_preserve_only_participating_lp_capability` | Isolated taker/LP grant disposition | Retained signed request, reused traded asset, independent identity denials and full SPL exit |

## Residual Gaps

This is bounded INV-012/002/004/007/019/024/081/089 conformance evidence, with
adjacent INV-005 disable/regrant evidence. Holdout labels **412 and 414 remain
OPEN**. It does not promote an invariant verdict or establish an arbitrary-history
generator/oracle against a vulnerable pin.

Owner A-to-B-to-A and portfolio recreation remain adjacent controls, not new
evidence here. Context destruction/recreation, program upgrades, nonzero fees and
PnL, partial fills, multi-leg batches, live sibling exposure during retirement,
liquidation/cure/recovery/terminal writers, expiry boundaries, longer mixed writer
histories and the complete object/authority replacement product remain outside
this increment. The same-program fixture authenticates context/delegate binding;
it does not model a production matcher's inventory or upgrades.

## Validation

The new selector passed **1/1** (16 histories, 64 exact rejections, 96 fills,
32 successful simulations and 32 complete owner withdrawals). Adjacent controls
passed **8/8**, the invariant index passed **1/1**, and touched-file formatting
and staged whitespace checks passed. Peak CU: fills 437,233; rejections 110,557;
writers 112,339; custody 147,768. These fit the 750,000 trade/writer and 300,000
custody ceilings.

Both production and authenticated-matcher SBF artifacts were built from this
worktree with platform-tools v1.52, offline and locked. Production used default
features. Host compilation required test-helper borrow/import corrections before
execution; no public-route conformance failure occurred. Existing unused-support
and `solana-client v1.18.26` future-incompatibility warnings remain. Production
code, dependency pins and holdout status are unchanged. No push was performed.

Commands, run from the isolated worktree:

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-capability-6piTse-target
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"

cargo build-sbf --tools-version v1.52 --offline -- --locked
env CARGO_TARGET_DIR=/dev/shm/astra-capability-6piTse-matcher-target \
  cargo build-sbf --tools-version v1.52 --offline \
  --manifest-path tests/fixtures/auth_matcher/Cargo.toml \
  --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked

cargo test --locked --offline --test v16_cu \
  inv_012_capability_and_delegate_scope::joint_incarnation_binding::role_switch_generation::v16_program_same_program_role_switch_revocation_survives_used_asset_reuse \
  -- --exact --nocapture

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_012_capability_and_delegate_scope::joint_incarnation_binding::funded_owner_roundtrip::v16_program_funded_owner_roundtrip_rejects_old_grant_after_current_cpi_prefix \
  inv_005_authority_incarnation_binding::funded_oracle_succession::v16_program_funded_oracle_succession_after_admin_burn_preserves_backing_exit \
  inv_012_capability_and_delegate_scope::joint_incarnation_binding::matcher_program_generation::v16_program_matcher_program_roundtrips_compose_with_asset_reuse \
  inv_012_capability_and_delegate_scope::joint_incarnation_binding::portfolio_grant_rollback::v16_program_failed_portfolio_reincarnation_preserves_retained_owner_grant \
  inv_012_capability_and_delegate_scope::joint_incarnation_binding::revocation_atomicity::v16_program_retained_capability_tracks_committed_revocation_after_bundle_rollback \
  inv_012_capability_and_delegate_scope::joint_incarnation_binding::used_generation_lifecycle::v16_program_used_asset_reuse_with_live_sibling_preserves_authorized_exit \
  inv_012_capability_and_delegate_scope::v16_program_issue406_matcher_trade_routes_preserve_only_participating_lp_capability \
  inv_012_capability_and_delegate_scope::v16_program_matcher_capability_route_roster_binds_every_current_scope

cargo test --locked --offline --test v16_program_fuzz_regressions \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  -- --exact --nocapture
rustfmt --edition 2021 --config skip_children=true --check \
  tests/invariants/cu/inv_012_role_switch_generation.rs \
  tests/invariants/cu/inv_012_joint_incarnation_binding.rs
git diff --check
git diff --cached --check
```
