# Lane 25: source expiry before mixed-role settlement

## Provenance and disposition

- Clone: `/tmp/percolator-lane25-pending-recovery-order-20260916`.
- Branch: `codex/lane25-pending-recovery-order-20260916`.
- Source checkout: `/tmp/percolator-astra-invariant-cycle-20260915-run`.
- Source branch: `codex/astra-invariant-cycle-20260915`.
- Base: `63cc6f284674f31187430f52d43a201b21b95d50`.
- Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- No use of `/home/anatoly/percolator-prog`; no open PR fix was read, copied or
  merged. Source checkout is unchanged. No push.
- No public-route LoF, persistent DoS or required-progress CU bug was found.
  There is no production fix or vulnerable/fixed comparison.
- Rows **419/435 remain OPEN/missing**. INV-039 remains `REFUTED_CURRENT`.
  This is bounded public conformance coverage, not generic closure.
- Changes are confined to four files under `tests/invariants/`. Production,
  manifests, lockfiles, fixture sources and machine TSVs are unchanged.

## Boundary and non-overlap

The pending creditor's own source backing expires while the same portfolio
still owes cross-asset K debt and retains uncharged zero-basis B loss weight.
The prior bankruptcy residual is also unbooked. The history resolves at slot
10 and compares terminal continuation at slots 1004, 1005 and 1011 around
the source bucket's exact expiry of 1005.

| Existing owner | Distinct coverage here |
| --- | --- |
| Lane 21 mixed-role close preemption | Its close expires at 17, and roles settle at 23 while source backing remains fresh until 1005. Here resolution is already complete when source expiry occurs; neither mixed role has settled. No close-preemption route is added. |
| Lane 18 native slab retirement | Its mixed roles settle before source expiry and native retirement. Here expired backing changes the available support before debt is charged, with classic SPL custody and no slab-retirement product. |
| Lane 11 / older accrued pending-loss routes | Elapsed funding, trade/reduction routes, Recovery forfeiture and oracle containment retain their owners. Here funding is zero and the new boundary is source freshness at mixed-role settlement. |
| Older fractional retirement | Provider and peer-source buckets expire after owner payouts. Here the original creditor's source expires before K/B settlement or any receipt. |
| `inv_039_pending_loss_backing_expiry.rs` | Its provider bucket expires with separate creditors and debtors, and opposing payment later replenishes backing. Here the same account owns both economic roles and the expiring backing originated from the already-closed opposing position. |
| Direct mixed-role and Lane 9 shutdown | These retain fresh support at mixed-role settlement. The fresh boundary here reuses that oracle as a control; the new oracle owns the expired-source outcome. |
| Funding/insurance, cure, shared-holder, restart and cohort-recreation owners | No such route or axis is added. No insurance, fees, funding, native custody, provider deposit, restart or account recreation is used. |

The README row 419/435 notes, existing INV-039 owners, Lane 9/18/21 reports,
`open_findings.tsv`, `coverage_reopenings.tsv`, and `scripts/loop.md` informed
selection. The existing setup and signed-transaction helpers are reused through
a child mount under `mixed_role_resolution::fractional_retirement`; their
behavior and assertions are unchanged.

## Public trace and oracle

Exact new selector:

```text
inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::unsettled_expiry::v16_program_source_expiry_before_mixed_role_settlement_preserves_debt_and_weight
```

1. System, SPL Token, ATA and wrapper instructions initialize three assets and
   five portfolios with deposits `[400000,180000,300000,250000,777]`.
2. Signed matched trades put owner 0 against owner 1 on one asset and owner 2
   against owner 0 on the other. Authenticated bounded mark updates create a
   200000-atom creditor gain and a separate debt of 36000 or 180000 atoms.
3. A matched reduction closes the first position, leaving owner 0 with a
   zero-basis leg retaining `POS_SCALE` loss weight. Owner 1's active close has
   a 20000-atom residual. Owner 0 retains a second leg with `2 * POS_SCALE`
   effective OI and an unsettled K snapshot. Owner 2 has the matching claim.
4. Public admin resolution at slot 10 preserves both unsettled roles and the
   residual. Clock advances to 1004, 1005 or 1011 without mutating economic
   Accounts. At 1005/1011, unsigned-owner `CloseResolved` first runs successfully
   before an unsigned deletion suffix rejects. Complete Accounts roll back,
   apart from the precisely accounted separate transaction payer fee.
5. The identical close bytes commit a normalization-only step. Backing of
   180000 atoms expires. Owner 0's complete leg array, source-domain array,
   capital and PnL, and every asset state remain unchanged. Both roles remain
   unsettled and all pending weights, counts and OI retain their prior values.
6. Two prescribed owner orders drive bounded resolved progress. Each owner's
   first close also executes before a rejected suffix, then commits unchanged.
   Waiting errors preserve complete tracked Accounts. Every nonterminal round
   changes state, and all portfolios finish within sixteen rounds.
7. Every paid receipt is retried as a no-op. All five portfolios are deleted
   with exact rent transferred to the market and unrelated Accounts preserved.

LiteSVM only loads programs, funds initial wallets and advances Clock. No
program-owned account injection, private engine transition or adaptive
simulation selects an economic outcome.

The expired-source oracle derives entitlements from the deposits and price
movements. Expired support is zero, so uncovered K debt consumes positive face
one-for-one; B charges the original 20000 residual separately. For debt `D`,
owner 0's terminal value is `400000 + 200000 - 20000 - D`, and owner 2's is
`300000 + D`. The second debt reaches the exact zero-positive-face boundary.
The surviving receipt faces are `180000 - D` for owner 0, when positive, and
`D` for owner 2. Both are paid exactly, with no fresh or spent source backing.

The fresh control uses the existing input-derived support-discount book:
`S = min(D, 180000)`, retired face `S * 200000 / 180000`, and extra discount
`retired face - S`. The difference is asserted explicitly; expiry is not
mistaken for debt forgiveness or an unauthorized payment.

| Debt | Boundary | Payouts `[0,1,2,3,4]` | Vault residue |
| --- | --- | --- | --- |
| 36000 | Fresh, slot 1004 | `[540000,0,336000,250000,777]` | 4000 |
| 36000 | Expired, slot 1005/1011 | `[544000,0,336000,250000,777]` | 0 |
| 180000 | Fresh, slot 1004 | `[380000,0,480000,250000,777]` | 20000 |
| 180000 | Expired, slot 1005/1011 | `[400000,0,480000,250000,777]` | 0 |

At every successful prefix the oracle checks capital + signed PnL + unpaid
receipt + paid SPL value for each original owner, monotonic K/B settlement,
residual partition, domain-local source membership and claim aggregates,
independent OI/weight/stored-leg/pending-count censuses, full stock/reservation
censuses, and token custody conservation of 1130777 atoms. A conserved
one-atom wrong-owner observation is rejected by the owner oracle. This is an
assertion-level negative control; no production mutation was run.

## Validation

New selector: **1 passed, 0 failed**, 48 worlds in 32.42 seconds. It covers
32 normalization-only steps, 272 successful-prefix rollbacks, 48 waiting
rollbacks, 64 receipt retries and 240 portfolio deletions. Peak measured
continuation/rollback/deletion CU: **205111**. Single normalization and payout
calls enforce the existing 300000 custody limit; composed transactions enforce
600000. Setup CU and maximum shape are not claimed.

All seven adjacent controls pass together: **7 passed, 0 failed** in 95.78
seconds. The selected INV-079 group passes **16/16** in 2.85 seconds, including
all thirteen registry/source guards and three trace/classifier checks. The
unrelated fixed-blocker campaign is explicitly excluded. Scoped rustfmt,
`git diff --check`, the protected-file diff and the full outside-scope diff
all exit 0. Both SBF builds and the host build exit 0.

| Adjacent control | Worlds | Peak reported CU |
| --- | --- | --- |
| Pending provider-backing expiry | 16 | 316767 |
| Lane 21 mixed close preemption | 32 | 205951 |
| Classic fractional retirement | 32 | 190140 |
| Lane 18 native retirement | 24 | 210806 |
| Mixed funding/insurance | 12 | 164798 |
| Direct mixed-role resolution | 32 | 205951 |
| Pending obligation at resolution | 2 | 162160 |

The first INV-079 run passed all thirteen registry/source guards; three
additional public trace/classifier checks failed only because the fresh clone
had no authenticated matcher SBF. The matcher was then freshly built from the
unchanged fixture source and the complete selected group passed on rerun. The
new economic selector passed on its first run; no expectation was relaxed.

Host tools: Cargo 1.90.0, rustfmt 1.8.0-stable. Builds use platform-tools v1.52,
default wrapper features and locked/offline dependencies. Existing Solana client
future-compatibility warnings are unchanged. No unfiltered suite or Kani run.

Fresh wrapper SHA-256:
`e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
Fresh authenticated matcher SHA-256:
`50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
Installed LiteSVM SPL Token/ATA SHA-256:
`18264f491c7e0ad056dd36f42f8de6d1fedf9f044d1f521e714b4dc6b61594b6` /
`e5e7aed11ad3969eea2aa76c8b4d2e73ea25be7e6b5cce989b7710cf5452496e`.
Logs and artifacts are under `/dev/shm/percolator-lane25-20260916-*`.

## Exact commands

Provisioning from `/tmp`:

```bash
git clone --no-hardlinks --single-branch --branch codex/astra-invariant-cycle-20260915 /tmp/percolator-astra-invariant-cycle-20260915-run /tmp/percolator-lane25-pending-recovery-order-20260916
cd /tmp/percolator-lane25-pending-recovery-order-20260916
git switch -c codex/lane25-pending-recovery-order-20260916
mkdir -p /dev/shm/percolator-lane25-20260916-logs
env TMPDIR=/dev/shm CARGO_TARGET_DIR=/dev/shm/percolator-lane25-20260916-target CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-lane25-20260916-target/deploy -- --locked > /dev/shm/percolator-lane25-20260916-logs/build-wrapper.log 2>&1
env TMPDIR=/dev/shm CARGO_TARGET_DIR=/dev/shm/percolator-lane25-20260916-matcher-target CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-lane25-20260916-matcher-target/deploy -- --locked > /dev/shm/percolator-lane25-20260916-logs/build-matcher.log 2>&1
ln -s /dev/shm/percolator-lane25-20260916-matcher-target tests/fixtures/auth_matcher/target
```

The matcher symlink is ignored build output required by the existing harness's
fixed artifact path, not a fixture source change. All commands below run from
the isolated clone. These exports express the same environment passed with
`env` to each actual invocation:

```bash
export TMPDIR=/dev/shm
export CARGO_TARGET_DIR=/dev/shm/percolator-lane25-20260916-target
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-lane25-20260916-target/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu --no-run > /dev/shm/percolator-lane25-20260916-logs/build-host.log 2>&1
cargo test --locked --offline --test v16_cu inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::unsettled_expiry::v16_program_source_expiry_before_mixed_role_settlement_preserves_debt_and_weight -- --exact --nocapture > /dev/shm/percolator-lane25-20260916-logs/new-test.log 2>&1

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::v16_program_mixed_creditor_debtor_roles_preserve_owner_entitlement_through_resolution \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::close_preemption::v16_program_expired_close_preserves_mixed_debt_and_fractional_source_attribution \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::v16_program_mixed_role_fractional_source_preserves_attribution_through_expiry_and_retirement \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::v16_program_mixed_role_native_retirement_separates_owner_debt_residue_and_surplus \
  inv_039_pending_loss_obligation_durability::backing_expiry::v16_program_pending_losses_survive_late_backing_expiry_and_claimant_close_order \
  inv_039_pending_loss_obligation_durability::v16_program_resolve_with_pending_obligation_defers_claim_until_debtor_settles \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::funding_resolution::funding_insurance::v16_program_mixed_funding_debt_charges_only_its_insurance_domain \
  > /dev/shm/percolator-lane25-20260916-logs/controls.log 2>&1

cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence:: -- --skip v16_program_fixed_blockers_remain_progressing --nocapture --test-threads=1 > /dev/shm/percolator-lane25-20260916-logs/inv079-final.log 2>&1

rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_039_mixed_role_fractional_retirement.rs tests/invariants/cu/inv_039_mixed_role_unsettled_expiry.rs
git diff --check
git diff --exit-code 63cc6f284674f31187430f52d43a201b21b95d50 -- src Cargo.toml Cargo.lock tests/fixtures 'tests/invariants/*.tsv'
git diff --exit-code 63cc6f284674f31187430f52d43a201b21b95d50 -- . ':!tests/invariants/**'
```

Local commit and final scope checks:

```bash
git add tests/invariants/cu/inv_039_mixed_role_unsettled_expiry.rs tests/invariants/cu/inv_039_mixed_role_fractional_retirement.rs tests/invariants/README.md tests/invariants/lane25_mixed_role_unsettled_expiry_20260916.md
git diff --cached --check
git -c user.name=Codex -c user.email=codex@openai.com commit -m 'test(inv-039): cover backing expiry before mixed-role settlement'
git show --format= --check HEAD
git diff --exit-code 63cc6f284674f31187430f52d43a201b21b95d50 HEAD -- src Cargo.toml Cargo.lock tests/fixtures 'tests/invariants/*.tsv'
git diff --exit-code 63cc6f284674f31187430f52d43a201b21b95d50 HEAD -- . ':!tests/invariants/**'
git status --short --branch
git log -1 --format='%H %s'
```

## Limits and files

This finite product has three assets, five portfolios, integral quantities and
price moves, zero fees/funding/insurance/ADL, no liens, no external backing
deposit, no partial receipt funding, and no arbitrary ordering or maximum-shape
claim. It does not test debt larger than the expired creditor's net face,
partial source replenishment after expiry, another Recovery transition, native
redemption, asset restart or slab retirement. Generic INV-086 equivalence and
rows 419/435 remain open.

- `cu/inv_039_mixed_role_unsettled_expiry.rs`: new public invariant and independent expired-source book.
- `cu/inv_039_mixed_role_fractional_retirement.rs`: child mount only.
- `README.md`: bounded row 419/435 coverage note.
- `lane25_mixed_role_unsettled_expiry_20260916.md`: this report.
