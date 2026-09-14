# Astra Scope J: retained policy, admission and earned attribution

Base: `d01245dd993bfe60c2448360733c3bb63c41aa19`, verified as the current
`origin/codex/astra-open-holdout-ledger-20260912` tip with `git ls-remote`.
Branch: `codex/astra-scope-j-retained-admission-20260914`.
Isolated fork: `/tmp/percolator-scope-j.M3IjWZ`, created with `git clone --shared`
and its own index, refs and working files. Neither protected checkout was edited.
Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

The loop rules, invariant charter, README ownership/coverage sections, reopening
ledger and current requested owners informed the selection. Existing source/tests
and Scopes E/M/P/S/T supply the comparison below. Row labels are coverage metadata,
not economic expectations. No external implementation or PR was used as an oracle.

## Distinct Coverage

| Existing family | Existing boundary | New composition |
| --- | --- | --- |
| Scope E retained policy earned reserves | Four single direct/CPI full closes, returning insurance authority, incumbent provider fixed | Both funded roles return, temporary provider receives an earned slice, lower/equal fee restoration and independent terminal payout orders |
| Scope M generated partial policy words | Partial fills and policy words, fixed authority epoch, Live principal exits | Retained policy/trade envelopes across funded authority return; no additional partial-fill samples |
| Scope P generated first-risk liabilities | Generated fee intervals and nontraded lag, no trade fees or retained policy return | Fixed numeric first admission with retained withdrawal/trade consent, nonzero fees and either party holding temporary fee authority |
| Scope F generated flat fee entitlement | Sampled flat/reopened numbers and four collection/admission routes | No new numeric samples; retained signatures cross funded role and policy transitions |
| Scope S generated expiring roles | Role returns around expiry after all users exit | Role returns while positions and liens remain live, retained closing consent and a paid temporary-provider prefix |
| Scope T generated funded-role epochs | Interleaved funded roles, flat owners, same-price observations and reserve withdrawals | Trading consent, health margin, provider earnings and terminal attribution; no additional role-word samples |
| Retained fee-authority epoch | Unfunded inherited fee authority and full fills | Funded policy role, both funded-role return ordering, maintenance collection and earned-reserve payouts |

## Retained Earned Reserves

Primary INV-014 owner: `cu/inv_014_generated_policy_reserve_routes.rs`, mounted
below `terminal_earnings_succession::terminal_role_coalescence::retained_policy_earned_reserves`.
The existing `History` fixture/signature/full-Account checker is reused unchanged.

The Cartesian product is two single transports, restored base policies 19/37,
two provider/insurance outward orders (with reverse return order), and two
independent terminal principal/insurance payout orders: **16 worlds**.
Owner settlement order is coupled to outward role order, not a further dimension.

Public setup earns 875 utilization atoms, split into 657 provider atoms and 218
insurance atoms. The original provider receives 17 atoms. Both funded roles then
transfer to the existing insurance operator, which receives 11 provider-fee atoms
and raises the base trade fee to 41 bps. Both roles consensually return. Management
preserves the complete market economy and user Accounts; control sequences and
the restored full authority profile are exact. Paid earnings do not return with
the role. The source principal and users' positions stay live during the returns.

Before the handoffs, four transactions are signed and successfully simulated:
old policy/close, close/old policy, and two otherwise identical close alternatives.
The retained policy has a sequence gap of 100, so sequence supersession cannot
mask epoch invalidation. After return, the old policy prefix rejects stale and
one close alternative rejects the 41-bps policy. At restored policy the old-policy
suffix rolls back a completed close, including the CPI when applicable. The
never-delivered alternative then succeeds with its complete original serialized
transaction unchanged. Normal signature verification and transaction history stay
enabled. A failed transaction signature is not reused as successful consent.

Single CPI charges current base policy; bilateral trades charge the signed 37
bps. The independent per-owner closing fee is
`ceil(1050 * 105 * charged_bps / 10000)`, or 210/408 atoms. Both users are paid and
deleted after resolution. A provider-fee payout followed by an insurance request
for the former beneficiary/current operator rolls back the real SPL prefix. The
valid payout instruction retries in a new envelope. Unsigned terminal reserves
pay only their current beneficiaries. Input-derived final amounts are:

| Recipient | 210-atom close fee | 408-atom close fee |
| --- | ---: | ---: |
| First owner | 56417 | 56219 |
| Second owner | 1994790 | 1994592 |
| Returning provider, principal plus earned fees | 100646 | 100646 |
| Temporary provider/current insurance operator | 11 | 11 |
| Returning insurance beneficiary | 669 | 1065 |

Each column equals the fixed 2152533-atom supply. Exact SPL Account images, owner
equity, insurance domains, source earnings, OI, position epochs, stock and
encumbrance censuses are checked. Both fee ledgers retain their own 646/11 paid
history. A conserved one-atom wrong-owner observation fails the same endpoint
predicate used on runtime token balances. Route normalization removes only the
disclosed closing fees, then compares every owner's endpoint across all worlds.
CloseSlab leaves the typed rent-exact tombstone, closes custody, preserves tokens,
mint and fee ledgers, and returns only market/vault rent to market authority.

## Retained First Admission

Primary INV-027 owner: `cu/inv_027_retained_admission_policy_return.rs`, mounted
below `joint_admission_liabilities`. The existing public portfolio/deposit and
authenticated matcher constructors are reused; no generated sampled cases are
copied or added to the existing families.

Four transports cross base policies 19/37, explicit sync versus collection by
withdrawal, and either trader as temporary insurance policy authority: **32 worlds**.
The numeric inputs are fixed: 7 atoms/slot over slots 1..4, five lots at price 100,
100% initial margin, 50% maintenance margin, and a 7-atom withdrawal prefix.
The thin owner is always owner 0; numeric inputs and trade direction are not
claimed as independently generated dimensions.

The peer's 21-atom fee is publicly collected before retention, making the policy
role funded in every world. In explicit worlds the thin owner is also synced;
in the other worlds that owner's 21-atom fee remains uncollected until the
retained withdrawal instruction. The position-free user Accounts remain unchanged
across the authenticated slot/price update and all role handoffs. Initial funds
are computed from the declared opening route, policy, fees and exact margin.

Four retained envelopes bind the same withdrawal and first-risk trade, with or
without a forward-gap policy prefix/suffix. After the funded policy A -> trader -> A
return and temporary 41-bps policy, old policy and over-budget envelopes reject.
At restored policy, a completed withdrawal and exact first fill roll back on the
stale policy suffix. A separate one-quantity-atom-above-boundary transaction also
restores the completed SPL withdrawal. The original never-delivered successful
envelope then admits exactly 500 atoms of certified equity and initial margin.

The batch-CPI high-policy envelope rejects `EngineInvalidConfig` after matcher
execution; its signed aggregate cap is 2 atoms while current fees would be 3.
That envelope also exceeds thin-account margin: this is a combined economic
boundary, not an isolated batch fee-rate theorem. The other high-policy routes
reject `InvalidInstruction` before matcher execution. Complete compiled/tracked
Accounts roll back, including implicit fee collection and the SPL prefix where
applicable; only the payer's exact signature fee differs.

The input book separately tracks each owner's maintenance, trading fees and
prior payouts. It checks exact capital, fee cursors, zero PnL/soft credits/claims,
both OI sides, ownership, whole SPL Accounts, mint, insurance domain rounding and
stock/reservation censuses. After admission and close both current certificates
must match independent raw-state health math and the input margin/equity lanes.
This is not a new snapshot-full-refresh comparison or lag/funding decomposition.

An opposite batching/CPI route closes the position. Where a bilateral opening
revokes the old matcher grant, the LP publicly renews it before that fresh close.
Both owners withdraw their exact remaining principal in either owner order.
Normalized payouts equal each deposited principal minus 21 maintenance atoms;
insurance retains exactly the separately computed maintenance and trade fees.
This generator ends Live with zero user capital, not at slab retirement.

## Limits And Rows

No public-instruction conformance mismatch was established. Production and Cargo
inputs are unchanged; this increment is bounded coverage and documentation only.

Rows **410, 411, 413, 416 and 429 remain OPEN**. All non-comment reopening rows,
`open_findings.tsv` and `invariant_status.tsv` are unchanged. INV-014 and INV-027
are primary; related assertions are described above, not whole-invariant closure.

- 410/429: finite one-asset fresh-reserve histories; no expiry, insurance consumption
  or recredit, receipts, pending loss, alternate token rails, missing beneficiaries,
  unrelated submitter permutations, or arbitrary role/history lengths.
- 411: exact full fills with fixed authenticated prices, selected base policies and
  one backing tariff. No partial/multi-leg budgets, new backing-fee consent, dynamic
  marks, redirect policy, durable nonce or arbitrary policy words.
- 413: a public fee-collecting prefix always precedes risk. Bare standalone first
  admission with deferred flat fees, funding, lag, junior support, clipping and
  maximum portfolio shapes remain outside this increment.
- 416: both returns are consensual. No new cold-admin replacement matrix, disabled
  role shape or all-role economic-containment theorem is claimed. INV-062 supplies
  common harness control only, not same-address aliasing coverage.

The reserve generator uses single transports because the existing wrapper rejects
batch trades with a nonzero backing tariff. Policy retirement is itself guarded
while that bucket is funded. Initial batch/retirement attempts were removed from
the candidate matrix; they establish neither a production mismatch nor new
rejection-only coverage. Other development corrections were test-side instruction
naming, compatible init parameters, matcher renewal and the batch-CPI error/log
expectation. No successful payout or economic expected amount was weakened.

## Artifact And Validation

`src` compares byte-identical with the fixed Scope W checkout. Cargo inputs and
authenticated matcher source hashes also match. The reused program path is
`/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so`, SHA-256
`79b298706ca7ffab09a017e5f37c4c0fc43d4e6292c092d126f4fbe8dff1b673`.
The matching auth matcher is copied into this fork's fixture output path, SHA-256
`50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
No new production build or artifact is claimed.

The first host build exhausted `/run/user/1001`; only this task's failed outputs
were moved/cleaned. `/run` is noexec, so a private executable 6-GiB tmpfs at
`/run/astra-scope-j-20260914/target` holds host builds. No other worker's cache was
modified. Logs are `/run/astra-scope-j-20260914/*.log`.

Final new exact selectors: **2/2 PASS**, 48 worlds in 27.00s, 96 successful
simulations, 192 complete-Account rollbacks, 128 funded handoffs, 32 exact first
admissions, 96 final owner payouts/exits and 16 slab closures. Peaks are **578408**
and **275823 CU** in the final combined run. An earlier successful reserve run
measured **581408 CU**; 700000 is the sampled envelope, not a maximum-shape bound.
Fixture construction and helper-based matcher renewal are not fully metered by
the new selector counters. Existing Solana compatibility/dead-code warnings remain.

Adjacent controls: **5 PASS, 1 inherited FAIL** in 283.68s. Passing controls cover
Scope T's 360 funded-role worlds (59882 CU), fee-authority return (193865 CU),
Scope S's 72 expiry worlds (458351 CU), Scope E's four earned-reserve worlds
(578408 CU), and Scope P's 64 first-risk worlds (416766 CU).

The unchanged `terminal_fee_share_succession` control fails at
`cu/inv_024_terminal_fee_share_succession.rs:57`, expecting authority epoch 2 and
observing 3 after a Live insurance debit. The exact selector also fails in 0.47s
on an untouched detached `d01245dd` clone at `/run/astra-scope-j-20260914/base`,
using the same fixed program. Its working tree is clean. The old epoch expectation
is outside this coverage increment; no inherited test was changed.

All four metadata gates pass (4/4, 0.01s). The new-selector exact listing selects
two tests. Host compilation, formatter and working/staged/committed whitespace
checks pass. Non-comment coverage rows and all machine statuses compare exactly
with the base. No unfiltered suite or Kani run is claimed.

```sh
cd /tmp/percolator-scope-j.M3IjWZ
export CARGO_TARGET_DIR=/run/astra-scope-j-20260914/target
export TMPDIR=$CARGO_TARGET_DIR/tmp CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so

diff -qr src /run/percolator-pr135-scope-w-20260913/src
sha256sum "$PERCOLATOR_FUZZ_SBF" tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run

RESERVE=inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::retained_policy_earned_reserves::generated_policy_reserve_routes::v16_generated_retained_policy_routes_preserve_paid_earnings_across_funded_returns
ADMISSION=inv_027_protected_principal_seniority::joint_admission_liabilities::retained_admission_policy_return::v16_generated_retained_first_admission_survives_funded_policy_return_and_fee_collection
cargo test --locked --offline --test v16_cu -- --exact --list "$RESERVE" "$ADMISSION"
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 "$RESERVE" "$ADMISSION"

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::retained_policy_earned_reserves::v16_retained_close_policy_return_preserves_earned_reserves_through_terminal_payout \
  inv_014_delayed_policy_and_policy_epoch_safety::retained_single_cpi_policy_history::retained_fee_authority_epoch::v16_retained_cpi_fee_terms_survive_authority_aba_and_stale_policy_bundles \
  inv_027_protected_principal_seniority::joint_admission_liabilities::generated_first_risk_liabilities::v16_program_generated_first_risk_recertifies_reaged_liabilities_and_senior_exit \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::generated_expiring_roles::v16_program_generated_expiring_role_returns_preserve_beneficiaries_through_cleanup \
  inv_005_authority_incarnation_binding::cold_admin_handoff_scope::funded_role_zero_transition::generated_funded_role_epochs::v16_program_generated_funded_role_round_trips_preserve_entitlements_and_observation_scope \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::terminal_fee_share_succession::v16_program_terminal_fee_share_succession_preserves_operator_paid_history

cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --check HEAD
```

The detached-base control was reproduced with the same exported environment:

```sh
git clone --shared --no-checkout /tmp/percolator-scope-j.M3IjWZ /run/astra-scope-j-20260914/base
cd /run/astra-scope-j-20260914/base
git checkout --detach d01245dd993bfe60c2448360733c3bb63c41aa19
cargo clean -p percolator-prog
cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::terminal_fee_share_succession::v16_program_terminal_fee_share_succession_preserves_operator_paid_history -- --exact --nocapture --test-threads=1
git status --porcelain=v1
cd /tmp/percolator-scope-j.M3IjWZ
cargo clean -p percolator-prog
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
cargo test --locked --offline --test v16_cu -- --exact --list "$RESERVE" "$ADMISSION"
```

The package-only cleans affect this task's private target and prevent the detached
base and task checkout from sharing a stale test binary. Dependency outputs are
retained. Final logs include `new-tests-final.log`, `selectors.log`, `controls.log`,
`base-control.log`, `metadata.log`, `final-build.log` and `final-selectors.log`.

Changed paths:

- `tests/invariants/cu/inv_014_generated_policy_reserve_routes.rs`
- `tests/invariants/cu/inv_027_retained_admission_policy_return.rs`
- `tests/invariants/cu/inv_014_retained_policy_earned_reserves.rs` (module mount only)
- `tests/invariants/cu/inv_027_joint_admission_liabilities.rs` (module mount only)
- `tests/invariants/README.md`
- `tests/invariants/coverage_reopenings.tsv` (comments only)
- `tests/invariants/astra_scope_j_retained_policy_admission_20260914.md`
