# Lane 22: partial insurance recredit across unfinished liabilities

## Scope and isolation

- Base source: `/tmp/percolator-astra-invariant-cycle-20260915-run`, branch
  `codex/astra-invariant-cycle-20260915`, clean HEAD
  `d64f049005847848b095b3b8b2d21318d0504296`.
- Private non-hardlinked clone:
  `/tmp/percolator-lane22-terminal-insurance-progress-20260916`.
- Local branch: `codex/lane22-terminal-insurance-progress-20260916`.
- All edits are in that clone. Builds, host outputs and logs use lane-owned
  `/dev/shm/percolator-lane22-20260916-*` directories. Neither the base source nor
  `/home/anatoly/percolator-prog` is edited. No push is performed.
- Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`; wrapper/default features
  and authenticated matcher rebuilt offline and locked with platform-tools v1.52.

## Coverage selection and non-overlap

Surveyed `coverage_reopenings.tsv`'s Row 421 comments (lines 1159-1275 at the
base), README Row421 and lane summaries, and the insurance/recredit/native/quote/
custody owners. Those comments explicitly leave partial recredit and active peer
claims at repair open. This increment combines those dimensions with expiry
discovered by a public active-leg continuation, removing the admin-signed expiry
prerequisite in the older spent-insurance fixtures.

| Existing owner or lane | Distinction in Lane 22 |
| --- | --- |
| `inv_073_absent_insurer_spent_retirement.rs` | Reuses its publicly created trade/loss state. Existing 307-atom backing restores all 100 spent atoms after signed slab expiry; new backing restores only 37/73 after keeper expiry with unfinished peer claims. |
| Missing-wallet, quote-rail and native-recredit children | Existing worlds recover 100/101 after user deletion and administrative normalization. New worlds cross active debt, custody timing and partial recovery on one classic rail. Wallet absence alone is reused setup. |
| Native insurance ledger/redemption and frozen-insurance owners | No new native, ledger, redemption or freeze claim. Those dimensions are controls, not the new axis. |
| `inv_073_terminal_progress_product.rs` (Scope H) | Already varies partial recredit and missing custody after settlement, but explicitly signs insurance payments and administrative expiry. Lane 22 proves signature absence through expiry, recredit and payment with previously unfinished liabilities. |
| INV-039 reserve-role recredit (Scope D) | Owns pending claimant/provider coholding and partial/full recredit. Lane 22 uses distinct absent reserve roles, active-leg keeper expiry and custody timing, without admin-signed terminal normalization. |
| Lane 2 | Recovery custody/earnings and retained provider epoch retries; it does not consume and partially recover insurance in this active-liability product. |
| Lane 10 | Paid-prefix beneficiary succession, classic/native, no portfolios. Lane 22 has no succession; its unpaid insurance crosses bankrupt and profitable portfolios. |
| Lane 13 | Late provider expiry with unfinished user claims and missing insurance custody, but unspent insurance. Lane 22 adds bankruptcy spend, partial recredit and admin-free expiry. |
| Lanes 8, 17, 20 | Native receipt liquidity, funded beneficiary succession or deferred maintenance fees at expiry. Their receipt/fee/succession axes are not added here. |
| Lanes 3, 9, 18, 21 | Funding/insurance attribution, mixed-role debt shutdown, native retirement and close-expiry preemption. The fixed single-bankruptcy-pair setup is not claimed as new debt coverage. |
| Lanes 1, 4-7, 11-12, 14-16, 19 | Retained-policy, observation, limit, Hybrid/reward or maximum-shape/source products. Lane 22 does not reproduce or extend those axes. |

The new selector is mounted under the existing absent-insurer owner. No existing
Row421 selector is copied or replaced. The fixture gains one variant and a child
dispatch; existing variants retain their inputs and continuation.

## Public product and oracle

The Cartesian product is asset 0/1, surviving insurance 0/1, backing 37/73,
expiry before/after debtor settlement, and custody repair early/with first payout:
**32 worlds**. Before-loss expiry lands exactly at slot 44; after-loss expiry
lands at slot 45. These two landing slots are tied to the loss-order axis, not
claimed as an independent timing matrix. Resolution is keeper-only at slot 40;
slot 42 rejects without the owner and the grace period ends at slot 43.

Public deposits are `[1000, 100, 137]`. Ten lots move from 100 to 120, creating
200 profit and a 100-atom debtor shortfall after its capital is consumed.
Insurance funding is `100 + remainder`. The input-derived owner entitlement is
`[1200, 0, 137]`, independent of backing, custody timing and expiry order. Backing
is placed in the insurance-side domain, separate from the winner's source.
Mint authority is revoked after public funding. Reserve custody is closed and
beneficiary/operator wallets drained through SPL/System instructions before the
trade; their keys and the provider key are dropped by the shared fixture.

The continuation takes no reserve/admin keypair. Its transaction helper signs
only with the payer plus explicit portfolio owners for deletion; it checks the
message's signature count, payer identity, admin exclusion, signature validity
and 1232-byte packet bound. Every signer meta is checked against those explicit
keys, excluding all three reserve roles. The harness still stores its admin
keypair, but never uses it in this continuation. Admin and reserve Accounts remain
exact throughout.

Each history performs the following checks:

1. ATA creation succeeds before a premature insurance withdrawal rejects with
   `EngineLockActive`; full rollback restores missing custody and its rent.
   Half the worlds then commit custody repair with debt and peer claims active.
2. `CloseResolved` discovers exactly one elapsed backing bucket through an active
   leg. The profitable peer's source claim is still positive, and insurance spend
   is exactly zero or 100 according to whether expiry precedes debtor settlement.
3. Each user continuation first completes before an insurance suffix rejects.
   In late-repair worlds ATA creation also completes in that aborted transaction.
   The same user request then commits and strictly reduces a lexicographic rank:
   fresh target bucket, negative PnL, active legs, occupied sources, capital,
   positive PnL, nonterminal flag. Each owner is bounded by eight calls; the actual
   total is four per world. Exact payouts reach `[1200, 0, 137]`.
4. Insurance stays locked with three, two and one materialized empty portfolios.
   Owners delete all three; each deletion transfers exact portfolio rent to the
   slab. A final-deletion/repair/recredit/real-payment prefix rolls back when an
   unsigned `CloseSlab` suffix rejects with `ExpectedSigner`.
5. The unchanged insurance envelope retained before settlement now pays 17 atoms.
   Before it commits, a duplicate stale-epoch suffix rolls back the whole repair,
   partial recredit and SPL payment. With liquid custody remaining, replay of the
   retained envelope also rejects after the first payment has committed.
6. A current-epoch second payment returns the rest, first as a rolled-back prefix
   before unsigned slab closure, then committed. Keeper insurance payments need
   no operator, beneficiary, provider or admin signatures. Unsigned slab close
   still rejects even after all economic stock has left.

For backing `B`, remainder `R` and cumulative insurance payment `P`, the exact
post-recredit state is:

```text
user payouts             = [1200, 0, 137]
recoverable insurance    = min(100 spent, 100 receivable, B residual) = B
vault = insurance        = B + R - P
target budget           = 100 + R - P
target historical spend = 100 - B
remaining-budget total  = B + R - P
beneficiary SPL amount  = P
```

The first and second payments exhaust `B + R`, leaving 63/27 historical spent
atoms with equal exhausted budgets, not a fictional unpaid reserve claim. The
entire decoded market/config is compared after each payment; source state,
receipt aggregates and peer domains must remain unchanged. Complete expected
vault bytes, beneficiary token fields, role profiles and all control sequences
are checked, including exactly one authority-epoch increment per payment.

After every attempted transaction, stock and reservation/encumbrance censuses
use all surviving portfolios. Physical SPL custody plus recipient balances equals
the fixed minted supply. All touched/tracked Accounts roll back exactly on
rejection, including the fee payer adjusted only for actual network fees.
Successful frames enforce exact ATA rent and portfolio rent disposition.
`TokenAccount::pack` builds detached expected images only; no Account is injected.
The factory uses public System/SPL/wrapper creation. LiteSVM loading, airdrops and
clock advancement are the only runtime scaffolding.

## Results and limitations

The new selector passes **1/1**, all **32 worlds**, in **20.25s**. It checks:

- 128 ranked user continuations and 32 keeper-discovered backing expiries;
- 448 exact rejected-transaction rollbacks (14 per world);
- 96 owner-authorized deletions with exact rent;
- 32 custody creations, half with active liabilities and half with first payment;
- 64 positive unsigned insurance payments, returning 37/38/73/74 atoms per world;
- peak measured continuation/rollback CU **359,876**, below **600,000**.

The initial host compile found an inaccessible sibling transaction helper import.
The child now owns a small explicit-signature helper. No production change was
needed; no runtime property violation or security bug was found.

| Verification | Result |
| --- | --- |
| Fresh wrapper SBF | PASS, 26.61s, locked/offline |
| Fresh authenticated matcher SBF | PASS, locked/offline |
| New selector | PASS 1/1, 32 worlds, 20.25s; 1427 filtered out |
| Nine exact nearby controls listed below | PASS 9/9, 34.25s; 1419 filtered out |
| Selected INV-079 guards (all except fixed-blocker campaign) | PASS 16/16, 2.87s; 107 filtered out |
| Edited-file rustfmt | PASS |
| Worktree, staged and committed whitespace checks | PASS |
| Production/dependencies/all invariant TSVs against base | Empty diff, PASS |

This is bounded classic-SPL, two-asset, three-portfolio coverage with fixed
price/loss inputs, one backing expiry, no fees/funding/ADL, no insurance ledger,
no role succession, no native/secondary quote, and no maximum shape or arbitrary
schedule. Setup uses administrator and reserve signatures, and empty-portfolio
deletion uses owners. Absence of those owners is not covered. Admin-free economic
progress does not establish permissionless mechanical market retirement.

**Row 421 stays OPEN / missing. INV-073 stays REFUTED_CURRENT.** Production,
`Cargo.toml`, `Cargo.lock` and every invariant TSV remain unchanged. No generic
row is closed and no machine status is promoted.

## Artifacts

| Artifact | SHA-256 |
| --- | --- |
| `/dev/shm/percolator-lane22-20260916-target/deploy/percolator_prog.so` | `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f` |
| `/dev/shm/percolator-lane22-20260916-matcher-target/deploy/auth_matcher.so` | `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93` |
| LiteSVM `spl_token-3.5.0.so` | `18264f491c7e0ad056dd36f42f8de6d1fedf9f044d1f521e714b4dc6b61594b6` |
| LiteSVM `spl_associated_token_account-1.1.1.so` | `e5e7aed11ad3969eea2aa76c8b4d2e73ea25be7e6b5cce989b7710cf5452496e` |

The wrapper and matcher target directories were fresh, without copying another
lane's artifacts. The matcher's private fixture `target` is an ignored symlink
to its lane-owned `/dev/shm` target. Logs reside in
`/dev/shm/percolator-lane22-20260916-logs`.

## Exact commands

```bash
git clone --no-hardlinks --single-branch --branch codex/astra-invariant-cycle-20260915 /tmp/percolator-astra-invariant-cycle-20260915-run /tmp/percolator-lane22-terminal-insurance-progress-20260916
cd /tmp/percolator-lane22-terminal-insurance-progress-20260916
git switch -c codex/lane22-terminal-insurance-progress-20260916
mkdir -p /dev/shm/percolator-lane22-20260916-logs /dev/shm/percolator-lane22-20260916-tmp
env CARGO_TARGET_DIR=/dev/shm/percolator-lane22-20260916-target CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm/percolator-lane22-20260916-tmp RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-lane22-20260916-target/deploy -- --locked > /dev/shm/percolator-lane22-20260916-logs/build-wrapper.log 2>&1
env CARGO_TARGET_DIR=/dev/shm/percolator-lane22-20260916-matcher-target CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm/percolator-lane22-20260916-tmp RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-lane22-20260916-matcher-target/deploy -- --locked > /dev/shm/percolator-lane22-20260916-logs/build-matcher.log 2>&1
ln -s /dev/shm/percolator-lane22-20260916-matcher-target tests/fixtures/auth_matcher/target
sha256sum /dev/shm/percolator-lane22-20260916-target/deploy/percolator_prog.so /dev/shm/percolator-lane22-20260916-matcher-target/deploy/auth_matcher.so
sha256sum /home/anatoly/.cargo/registry/src/*/litesvm-0.1.0/src/spl/programs/spl_token-3.5.0.so /home/anatoly/.cargo/registry/src/*/litesvm-0.1.0/src/spl/programs/spl_associated_token_account-1.1.1.so
```

These exports express the identical per-command `env` used for scoped tests:

```bash
export CARGO_TARGET_DIR=/dev/shm/percolator-lane22-20260916-target
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export TMPDIR=/dev/shm/percolator-lane22-20260916-tmp
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-lane22-20260916-target/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu inv_073_no_permanent_user_lock::absent_insurer_spent_retirement::partial_recredit_liability_progress::v16_program_partial_insurance_recredit_crosses_active_liabilities_and_custody_repair -- --exact --nocapture --test-threads=1 > /dev/shm/percolator-lane22-20260916-logs/new-test.log 2>&1

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_073_no_permanent_user_lock::absent_insurer_spent_retirement::v16_program_absent_reserve_roles_preserve_recredited_insurance_after_backing_expiry \
  inv_073_no_permanent_user_lock::absent_insurer_spent_retirement::missing_insurance_wallet_recredit::v16_program_recredited_insurance_reaches_terminal_exit_without_wallets_or_signatures \
  inv_073_no_permanent_user_lock::absent_insurer_spent_retirement::recredited_insurance_quote_rails::v16_program_recredited_insurance_switches_quote_rails_without_operator_signatures \
  inv_073_no_permanent_user_lock::absent_insurer_spent_retirement::native_recredit_custody::v16_program_recredited_insurance_recreates_native_custody_without_role_signatures \
  inv_073_no_permanent_user_lock::frozen_insurance_remainder::v16_program_frozen_paid_insurance_preserves_unsigned_remainder_and_retirement \
  inv_073_no_permanent_user_lock::native_insurance_ledger_progress::v16_program_unsigned_native_insurance_ledger_excludes_donations_through_close \
  inv_073_no_permanent_user_lock::successor_custody_retry::v16_program_paid_insurance_prefix_survives_beneficiary_succession_on_both_rails \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_public_reserves::v16_program_pending_users_cross_late_expiry_before_unsigned_reserve_payouts \
  inv_073_no_permanent_user_lock::v16_program_terminal_disposition_and_administrative_retirement_are_source_complete \
  > /dev/shm/percolator-lane22-20260916-logs/controls.log 2>&1

cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence:: -- --skip v16_program_fixed_blockers_remain_progressing --nocapture --test-threads=1 > /dev/shm/percolator-lane22-20260916-logs/inv079.log 2>&1

rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_073_absent_insurer_spent_retirement.rs tests/invariants/cu/inv_073_partial_recredit_liability_progress.rs
git diff --check
git diff --exit-code d64f049005847848b095b3b8b2d21318d0504296 -- src Cargo.toml Cargo.lock 'tests/invariants/*.tsv'
```

Only these selectors and guards are run, not an unfiltered test suite or a
repository-wide reformat.

Local commit and final scope checks:

```bash
git add tests/invariants/cu/inv_073_absent_insurer_spent_retirement.rs tests/invariants/cu/inv_073_partial_recredit_liability_progress.rs tests/invariants/README.md tests/invariants/lane22_terminal_insurance_progress_20260916.md
git diff --cached --check
git -c user.name=Codex -c user.email=codex@openai.com commit -m 'test(inv-073): cover partial insurance recredit across active liabilities'
git show --format= --check HEAD
git diff --exit-code d64f049005847848b095b3b8b2d21318d0504296 HEAD -- src Cargo.toml Cargo.lock 'tests/invariants/*.tsv'
git status --short --branch
git rev-parse HEAD
```

Changed files are only the existing absent-insurer fixture, the new partial-
recredit child, the invariant README, and this report. No push was requested or
performed.
