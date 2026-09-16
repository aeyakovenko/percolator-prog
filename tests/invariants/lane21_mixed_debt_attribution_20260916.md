# Lane 21: mixed-role debt through expired-close preemption

## Provenance and disposition

- Private clone: `/tmp/percolator-lane21-mixed-debt-attribution-20260916`.
- Local branch: `codex/lane21-mixed-debt-attribution-20260916`.
- Base: `ef1647f22b196b01d55af0ab88f89c51b0bef61a`, cloned from
  `/tmp/percolator-astra-invariant-cycle-20260915-run`, branch
  `codex/astra-invariant-cycle-20260915`.
- Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Neither the base checkout nor `/home/anatoly/percolator-prog` was edited.
  All writes, builds, tests and commits use this clone or lane-owned `/dev/shm`.
- No public-route LoF, persistent DoS or required-progress CU bug was established
  in this product. No production fix or vulnerable/fixed comparison is claimed.
- Rows **419 and 435 remain OPEN/missing**; INV-039 remains `REFUTED_CURRENT`.
  Production, manifests, lockfiles and machine TSVs are unchanged. No push.

## Gap and non-overlap

INV-039 requires each pending obligation to stay attributed until settlement,
including when lifecycle transitions remove the ordinary close route. This
increment expires an active bankrupt close while its creditor is also an
unsettled debtor on another asset. Two permissionless cranks preempt the close
through global Recovery and Resolved before either role settles.

The survey used the row 419/435 entries and adjacent commentary in
`coverage_reopenings.tsv`, the invariant charter, `scripts/loop.md`, existing
test owners, and the lane 1-18 reports present on the supplied base.

| Existing coverage | Lane 21 distinction |
| --- | --- |
| `inv_039_pending_loss_funded_resolution.rs` | Existing funded, separate-portfolio cohorts; here one owner has pending creditor weight and cross-asset debt during close-expiry preemption. Funding is zero. |
| `inv_039_pending_loss_close_preemption.rs` | Existing single bankruptcy pair has no cross-asset debtor role or source-support discount. Lane 21 checks both with the existing mixed-role book. |
| `inv_039_pending_loss_cure_resolution.rs` | Existing public cure cancels the close. Here the close remains active with an exact unpaid residual through expiry and both mode transitions. |
| `inv_039_pending_loss_restart.rs` | Existing sibling restart preserves separate solvent obligations. Here no asset restarts; an expired bankrupt close drives global mode changes. |
| `inv_039_mixed_role_resolution.rs` | Existing direct resolution/live-B booking does not expire the close. Its independent book is reused without weakening any entitlement assertion. |
| `inv_039_mixed_role_fractional_retirement.rs` | Existing provider/source expiry and final retirement follow mixed-role resolution. Here close expiry precedes settlement while source support is still fresh. |
| Lane 9 | Participating-asset shutdown is not repeated. No `UpdateAssetLifecycle` appears in this new history. |
| Lane 18 | Native redemption, donations, ATA recreation and slab retirement are not repeated. This product uses classic SPL custody and stops after portfolio deletion. |
| Lane 3 | Funding and domain insurance allocation remain its axes; neither is introduced here. |
| Lanes 1, 2, 4-8, 10-17 | Retained controls, terminal payouts/reserves, reward provenance, oracle authority/freshness, admission limits and maximum-shape progress remain their owners. None adds opposing-close expiry to the same pending creditor/unsettled debtor portfolio. |

The new test is an INV-039 descendant under `mixed_role_resolution` and
`fractional_retirement`. This placement reuses the latter's signed transaction,
rollback, payout and deletion helpers without exporting or copying them.
The only fixture extension takes the close lifetime and `h_max` independently;
all existing callers retain exactly their old values, 1000 and 10 respectively.

## Public witness and oracle

Selector:

```text
inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::close_preemption::v16_program_expired_close_preserves_mixed_debt_and_fractional_source_attribution
```

System, SPL Token, ATA and wrapper instructions initialize all economic state.
LiteSVM supplies initial wallet funding, program loading and Clock advancement.
There is no program-owned account-byte injection or private engine transition.
Authenticated, bounded mark moves and signed trades create deposits
`[400000, 180000, 300000, 250000, 777]`, a 200000-atom creditor face for owner 0,
a 20000-atom unpaid bankruptcy residual for owner 1, and owner 0's separate
36000/216000-atom debt to owner 2. Owners 3 and 4 remain unrelated.

The init-time close lifetime is 12 slots and `h_max` is 1000. The test explicitly
requires `max_close_slot = 17` and creditor backing expiry at 1005. At slot 18,
the keeper uses the same observation-free `PermissionlessCrank` bytes for both
mode transitions. Each first succeeds before an unsigned deletion suffix
rejects at instruction 3, then identical crank bytes commit. All economic
accounts, asset state, source-credit records, source buckets and the active
close ledger remain unchanged except the market's mode/control state. The
book requires unbooked B, uncharged retained weight and unsettled K after each
rollback and each successful transition. Neither owner nor admin signs these
two cranks; `now_slot = 0` cannot substitute for the authenticated Clock.

The route control resolves directly at the same Clock through the public
administrator route and its normal bounded public accrual helper. Both routes
reject unsigned close at slot 18 with `ExpectedSigner`, preserving complete
Accounts. At the configured five-slot permissionless boundary, slot 23, the
same close bytes run. Residual booking first executes before a rejected suffix;
the identical instruction then commits, with both mixed roles still unsettled.

The 32 worlds cross two debts, two side orientations, two asset assignments,
two subsequent close orders and direct/preempted resolution. The orders put
the mixed debtor before the peer creditor or vice versa. Every terminal
continuation checks the original owner book; every nonterminal round must
change economic state and all owners must finish within 16 rounds. Waiting
errors restore the tracked Account frame. Every fully paid peer receipt is
retried as an exact no-op. All five portfolios are deleted with exact rent
transferred to the market and unrelated Accounts preserved.

Expected entitlements are input-derived. With `S = min(debt, 180000)`, retired
creditor face is `S * 200000 / 180000`, and the source discount is that face
minus `S`. Owner 0 must pay both the bankruptcy debit and its separate debt,
including this discount. The peer conversion preserves rate quantization and
the subsequent atom floor. These calculations are not inferred from the
control's successful balances.

| Debt | Owner SPL payouts `[0,1,2,3,4]` | Vault residue | Peer receipt face | Peer fresh reserved atoms |
| --- | --- | --- | --- | --- |
| 36000 | `[540000,0,336000,250000,777]` | 4000 | 36000 | 0 |
| 216000 | `[344000,0,516000,250000,777]` | 20000 | 180001 | 1 |

`Book::check` also checks source-domain ownership, exact close-residual
partition, OI, pending weights/counts, stock/reservation censuses and conserved
SPL supply of 1130777. Its negative control rejects a conserved one-atom
transfer between owners. The transaction helper validates signatures, packet
size, exact error instruction, successful prefix logs, complete Account
rollback and total lamports including the separate network payer's fee.
This is an assertion-level negative control, not a production mutation run.

## Results

The new exact selector passes: **32 worlds, 32 permissionless transitions,
64 successful-prefix rollbacks, 32 grace-period rejections, 16 waiting
rollbacks, 32 receipt retries and 160 portfolio deletions**. Maximum measured
continuation/rollback CU is **206213**. Existing helpers enforce 300000 CU for
resolved progress, 325000 for preemption, and 600000 per composed transaction.

All ten exact nearby selectors pass together (10 passed, 0 failed, 131.83s):

| Control | Worlds | Peak reported CU |
| --- | --- | --- |
| Single-pair expired close | 8 | 213130 |
| Cured/canceled close | 8 | 270386 cure/rollback; 188349 terminal/rollback |
| Classic fractional retirement | 32 | 199974 |
| Lane 18 native retirement | 24 | 203974 |
| Mixed funding/insurance | 12 | 158949 |
| Mixed funding | 4 | 204545 |
| Direct mixed-role resolution | 32 | 207559 |
| Lane 9 shutdown | 64 | 210559 |
| Funded pending resolution | 16 | 177388 |
| Sibling restart | 8 | 138120 |

The selected INV-079 command passes **16/16** (2.89s), including all thirteen
metadata/source guards and three public trace/classifier checks. The unrelated
`v16_program_fixed_blockers_remain_progressing` campaign is explicitly skipped.
The new selector passes 1/1 in 20.99s. Scoped rustfmt, `git diff --check`, and
the unchanged-production/manifests/machine-TSV check all exit 0. Build commands
also exit 0. Host tools are Cargo 1.90.0 and rustfmt 1.8.0-stable. The Solana
client emits its existing future-compatibility warning.

Two development runs failed on fixture assumptions. The first attempted an
unsigned payout before the configured five-slot grace elapsed; the final test
now explicitly asserts that rejection and retries at the boundary. The second
shortened source freshness along with close lifetime: the engine derives source
freshness from the maximum of accrual interval, `h_max`, and close lifetime.
Using independent horizons preserves fresh-source preconditions and explicitly
checks both deadlines. No expected owner payout, residual or fractional atom
was relaxed. Neither failure establishes a production bug.

## Artifacts and exact commands

Both SBFs were freshly built from this clone with platform-tools v1.52,
default wrapper features and the pinned lockfiles. SHA-256:

| Artifact | Hash |
| --- | --- |
| `/dev/shm/percolator-lane21-20260916-target/deploy/percolator_prog.so` | `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f` |
| `/dev/shm/percolator-lane21-20260916-matcher-target/deploy/auth_matcher.so` | `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93` |
| LiteSVM `spl_token-3.5.0.so` | `18264f491c7e0ad056dd36f42f8de6d1fedf9f044d1f521e714b4dc6b61594b6` |
| LiteSVM `spl_associated_token_account-1.1.1.so` | `e5e7aed11ad3969eea2aa76c8b4d2e73ea25be7e6b5cce989b7710cf5452496e` |

The unchanged installed LiteSVM fixtures supply SPL Token/ATA. The matcher is
exposed only through this clone's ignored `tests/fixtures/auth_matcher/target`
symlink to its private `/dev/shm` output. No other lane's artifacts are loaded.
Logs are retained in `/dev/shm/percolator-lane21-20260916-logs`.

Provisioning (clone from `/tmp`, subsequent commands from the private clone):

```bash
git clone --no-hardlinks --branch codex/astra-invariant-cycle-20260915 /tmp/percolator-astra-invariant-cycle-20260915-run /tmp/percolator-lane21-mixed-debt-attribution-20260916
cd /tmp/percolator-lane21-mixed-debt-attribution-20260916
git switch -c codex/lane21-mixed-debt-attribution-20260916
mkdir -p /dev/shm/percolator-lane21-20260916-logs
env CARGO_TARGET_DIR=/dev/shm/percolator-lane21-20260916-target CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-lane21-20260916-target/deploy -- --locked > /dev/shm/percolator-lane21-20260916-logs/build-wrapper.log 2>&1
env CARGO_TARGET_DIR=/dev/shm/percolator-lane21-20260916-matcher-target CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-lane21-20260916-matcher-target/deploy -- --locked > /dev/shm/percolator-lane21-20260916-logs/build-matcher.log 2>&1
ln -s /dev/shm/percolator-lane21-20260916-matcher-target tests/fixtures/auth_matcher/target
sha256sum /dev/shm/percolator-lane21-20260916-target/deploy/percolator_prog.so /dev/shm/percolator-lane21-20260916-matcher-target/deploy/auth_matcher.so
sha256sum /home/anatoly/.cargo/registry/src/*/litesvm-0.1.0/src/spl/programs/spl_token-3.5.0.so /home/anatoly/.cargo/registry/src/*/litesvm-0.1.0/src/spl/programs/spl_associated_token_account-1.1.1.so
```

The following exports express the identical environment passed with `env` to
each test invocation. Only explicitly scoped selectors are run:

```bash
export CARGO_TARGET_DIR=/dev/shm/percolator-lane21-20260916-target
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-lane21-20260916-target/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::close_preemption::v16_program_expired_close_preserves_mixed_debt_and_fractional_source_attribution -- --exact --nocapture > /dev/shm/percolator-lane21-20260916-logs/new-test.log 2>&1

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::v16_program_mixed_creditor_debtor_roles_preserve_owner_entitlement_through_resolution \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::v16_program_mixed_role_shutdown_preserves_pending_debt_and_terminal_entitlement \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::funding_resolution::v16_program_mixed_roles_preserve_funding_attribution_through_resolution \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::funding_resolution::funding_insurance::v16_program_mixed_funding_debt_charges_only_its_insurance_domain \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::v16_program_mixed_role_fractional_source_preserves_attribution_through_expiry_and_retirement \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::v16_program_mixed_role_native_retirement_separates_owner_debt_residue_and_surplus \
  inv_039_pending_loss_obligation_durability::resolved_histories::funded_resolution::v16_program_funded_pending_debt_survives_resolution_and_delayed_close_orders \
  inv_039_pending_loss_obligation_durability::close_reopen::close_preemption::v16_program_expired_bankrupt_close_preserves_pending_cohort_entitlement_across_routes \
  inv_039_pending_loss_obligation_durability::close_reopen::cure_resolution::v16_program_canceled_close_keeps_debt_through_pending_release_and_resolution \
  inv_039_pending_loss_obligation_durability::restart::v16_program_pending_debt_survives_sibling_restart_and_delayed_resolution \
  > /dev/shm/percolator-lane21-20260916-logs/controls.log 2>&1

cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence:: -- --skip v16_program_fixed_blockers_remain_progressing --nocapture --test-threads=1 > /dev/shm/percolator-lane21-20260916-logs/inv079.log 2>&1

rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_039_mixed_role_resolution.rs tests/invariants/cu/inv_039_mixed_role_fractional_retirement.rs tests/invariants/cu/inv_039_mixed_role_close_preemption.rs
git diff --check
git diff --exit-code ef1647f22b196b01d55af0ab88f89c51b0bef61a -- src Cargo.toml Cargo.lock 'tests/invariants/*.tsv'
```

No unfiltered repository suite, Kani campaign or repository-wide reformat is run.

Local commit and final scope checks:

```bash
git add tests/invariants/cu/inv_039_mixed_role_close_preemption.rs tests/invariants/cu/inv_039_mixed_role_fractional_retirement.rs tests/invariants/cu/inv_039_mixed_role_resolution.rs tests/invariants/README.md tests/invariants/lane21_mixed_debt_attribution_20260916.md
git diff --cached --check
git -c user.name=Codex -c user.email=codex@openai.com commit -m 'test(inv-039): preserve mixed debt across close-expiry preemption'
git show --format= --check HEAD
git diff --check ef1647f22b196b01d55af0ab88f89c51b0bef61a HEAD
git diff --exit-code ef1647f22b196b01d55af0ab88f89c51b0bef61a HEAD -- src Cargo.toml Cargo.lock 'tests/invariants/*.tsv'
git status --short --branch
git log -1 --format='%H %s'
```

## Limits and changed files

This finite product has three configured assets, five portfolios, zero funding,
fees, ADL and external backing/insurance. It does not cover fractional B cohort
allocation, underfunded receipts, source expiry concurrent with unsettled mixed
roles, cured closes, asset restart, native custody, slab retirement, malicious
oracles, arbitrary schedules or maximum shape. Portfolio deletion still needs
each owner signature. The source discount remains precisely accounted in the
vault; no claim of full market retirement or generic INV-086 equivalence is made.

Changed files:

- `tests/invariants/cu/inv_039_mixed_role_close_preemption.rs`: new invariant test.
- `tests/invariants/cu/inv_039_mixed_role_fractional_retirement.rs`: child mount only.
- `tests/invariants/cu/inv_039_mixed_role_resolution.rs`: fixture horizon parameters.
- `tests/invariants/README.md`: bounded coverage note.
- `tests/invariants/lane21_mixed_debt_attribution_20260916.md`: this report.
