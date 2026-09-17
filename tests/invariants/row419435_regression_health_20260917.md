# Rows 419/435 regression health, 2026-09-17

Base: freshly fetched `origin/main`, `26c11a568c6fd72f16a20119f209994b7f24bee0`.
Worktree: `/dev/shm/percolator-row419435-health-20260917`.
Branch: `worker/row419435-health-20260917`.
Engine pin: `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.

## Scope and decision

Reviewed the README, [funding/loss audit](funding_loss_audit_20260917.md),
[terminal entitlement audit](terminal_payout_entitlement_audit_20260917.md),
[shared-holder audit](pending_loss_shared_holder_audit_20260912.md),
[mixed-role book](pr135_scope_j_mixed_role_conformance_20260913.md),
[fractional expiry](astra_scope_l_mixed_fractional_expiry_20260914.md), and
[fractional cohort](astra_scope_y_mixed_fractional_cohort_20260914.md), together
with their mounted CU sources. Rows 419/435 were withheld comparison labels;
no external issue, patch, or reproduction informed the new scenario.

Row 419 already checks Recovery-to-Resolved retained weight and the exact owner
vector `[12,12,0,12,0]` in two landing orders. Row 435's price-debt witness already
has an independent owner ledger, while its nonzero-funding witness checks only
equality between observed close-order outcomes and aggregate conservation.
Equal misattribution in both outcomes would pass that funding oracle. The new
selector supplies an independent funding entitlement oracle for a solvent
Recovery-forfeit origin. Existing row-419 and mixed price-debt selectors were
source-reviewed, not rerun or changed. The existing funding selector did fail on
the current pin: it expected immediate deletion of the last underfunded receipt,
before its remaining backing expired. The repaired test follows the current
public continuation described below. No production issue has been established.

Historical OPEN labels in older notes are dated evidence, not current status.
The funding module's obsolete row-435 OPEN comment is replaced with its actual
proof boundary. No machine ledger, invariant status, or other row is changed.

## New public evidence

Owner: [mixed funding CU](cu/inv_039_mixed_role_funding_resolution.rs).
New selector suffix:
`v16_program_solvent_mixed_funding_preserves_input_derived_owner_entitlements`.
Both selectors in this file remain mounted under
`inv_039_pending_loss_obligation_durability::mixed_role_resolution::funding_resolution`.

System/SPL/ATA/wrapper instructions construct all economic state. Deposits are
`[500000,500000,500000,250000,777]`; owner 0 holds four creditor lots in asset 1
and owes two lots to owner 2 in asset 2. Both side orientations cross two close
orders. Asset 1 moves first; a public refresh books its credit before shutdown
and owner-signed Recovery forfeit. Asset 2 then moves, preserving owner 0's
zero-basis creditor weight alongside an explicitly unbooked debtor leg at
resolution. Mint authority is revoked before the price histories.

Each domain has six capped one-slot moves of 10,000 from price 1,000,000.
The reported premium exceeds the configured rate cap of 1,000 E9 units:

```text
funding = sum(floor(sign * 1000 * (1000000 + sign * slot * 10000) / 1e9))
          for slot = 1..6
        = sign * 6
credit per lot = 60000 - sign * funding = 59994
exact owner payouts = [619988,260024,619988,250000,777]
```

Expected amounts use inputs, never observed PnL, receipt values, payouts, or a
second engine execution. Snapshots only distinguish booked from unbooked fixed
debts. At every resolution/close/waiting-rejection/receipt-retry/deletion checkpoint:

```text
capital + signed PnL + unpaid receipt + prior SPL payout + unbooked net credit
  = original deposit + original credit - original debt, separately per owner
```

The oracle also checks domain ownership, side, basis, retained weights, exact
OI/count aggregates, frozen price/funding indices, zero B, empty close ledgers,
stock census, custody, fixed mint supply, and payout bounds. Pending weight is
not added to the value equation. Waiting errors retain the existing exact
economic-account rollback check. Paid receipt retries remain quiescent; bounded
continuation pays every owner and deletes all five portfolios, leaving no vault
residue.

The original four insolvent funding worlds retain their differential oracle.
Their old cleanup loop stalls with only owner 0 remaining, a two-atom receipt
paid zero, and one reserved source-backing atom. Current engine pin `4db11a8c`
correctly retains that receipt until the payout rate is terminal. The repaired
loop requires this exact state, rejects a future-slot expiry hint before the
authenticated clock reaches the committed deadline with exact economic-frame
rollback, then executes the same public hinted crank at expiry. Expiry preserves
every tracked non-market account and releases exactly the reserved atom; an
explicit receipt top-up pays that one atom to owner 0 and debits the vault once.
Subsequent close/deletion completes within the existing bounded loop. Exactly
one expiry is required in each insolvent world and zero in each solvent world,
so premature receipt deletion cannot silently bypass the new suffix. This adds
a positive top-up and backing-expiry join to the mixed funding history.

This adds bounded INV-039/024/027/067/073 evidence, with INV-037's empty-close
separation and selected INV-081 state predicates. It does not add INV-070 slab
retirement evidence. No production fix or production red/green claim is made.

## Verification

Default-feature SBF was rebuilt from this worktree using a private copied cache.
SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
No matcher artifact is needed by these two selectors. Host cache and logs are
private; the original checkout and row416/row427 files were not edited.

```bash
git fetch origin main
git worktree add /dev/shm/percolator-row419435-health-20260917 -b worker/row419435-health-20260917 origin/main
cp -a --reflink=auto /dev/shm/percolator-public-gap-20260916-c91e-host-target /dev/shm/percolator-row419435-health-20260917-host-target
cp -a --reflink=auto /dev/shm/percolator-public-gap-20260916-c91e-sbf-target /dev/shm/percolator-row419435-health-20260917-sbf-target
```

From the new worktree:

```bash
env CARGO_TARGET_DIR=/dev/shm/percolator-row419435-health-20260917-sbf-target CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row419435-health-20260917-sbf-target/deploy -- --locked
export CARGO_TARGET_DIR=/dev/shm/percolator-row419435-health-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row419435-health-20260917-sbf-target/deploy/percolator_prog.so
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::funding_resolution::v16_program_solvent_mixed_funding_preserves_input_derived_owner_entitlements \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::funding_resolution::v16_program_mixed_roles_preserve_funding_attribution_through_resolution
rustfmt --edition 2021 --config skip_children=true tests/invariants/cu/inv_039_mixed_role_funding_resolution.rs
rustfmt --check --edition 2021 --config skip_children=true tests/invariants/cu/inv_039_mixed_role_funding_resolution.rs
git diff --check
git diff --cached --check
git diff --exit-code 26c11a568c6fd72f16a20119f209994b7f24bee0 -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)**/*.tsv' ':(glob)**/*row416*' ':(glob)**/*row427*'
git show --format= --check HEAD
```

Development first rejected an oracle that omitted the first funding interval.
Source review also required booking positive PnL before Recovery forfeit; the
final staged setup explicitly verifies that the other debt remains unbooked.
The first expiry probe supplied the generic live-crank payer role; Resolved
cranks require the portfolio owner's public key (without its signature after
the delay). The corrected instruction now reaches the intended NonProgress
rejection. These are test-model/setup corrections, not production regressions.

Final exact run: **2 passed, 0 failed, 1,439 filtered**, in **4.72 seconds**.

| Selector | Result |
| --- | --- |
| New solvent owner ledger | Four worlds, 66 independent checkpoints, 20 exact payouts/deletions; peak 201,926 CU |
| Repaired insolvent funding orders | Four worlds, four pre-expiry rollbacks, four hinted expiries at slot 6,480,006, four one-atom top-ups, 20 deletions; peak 201,595 CU |
| SBF build, targeted rustfmt/check, working/staged/committed whitespace checks | PASS |
| Production/Cargo/fixture/all-TSV/row416/row427 diff guard | PASS, no diff |

Logs are `/dev/shm/percolator-row419435-health-20260917-{sbf-build,exact}.log`.
The existing Solana client future-compatibility warning remains. No broad suite,
metadata census, external push, or unrelated selector was run. The commit is
local only and contains the CU owner, this note, and the short README entry.

## Open gaps

Arbitrary histories, fractional funding/cohort rounding, ADL, nonzero close
residuals with independently derived funding entitlements, competing partial
receipts beyond the fixed last-claimant top-up, insurance recredit/reserve-role
overlap, alternate quote rails, and slab retirement remain outside this increment.
Existing tests cover some of these dimensions separately; this selector does not establish
their composition. Neither row nor invariant status is promoted.
