# PR135 Scope P: first-risk liability-admission conformance

Date: 2026-09-13. Primary owner: INV-027. Related invariants: INV-010, INV-024,
INV-044, INV-053, INV-060, INV-062 and INV-081.

Worktree: `/tmp/percolator-pr135-scope-p-20260913`.
Branch: `codex/pr135-scope-p-first-risk-liability-conformance-20260913`.
Base: `5a922618a70734e4a6db25095aadb2b94b6d9702`, fetched from
`origin/codex/astra-open-holdout-ledger-20260912` before creating the worktree.
Engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

Only the requested base checkout's invariant tests, documentation and shared
helpers informed the probe. Row 413 was a coverage label, not an oracle or test
input. No external PR branch, diff or test was inspected or copied. Neither
protected checkout was edited; Git's shared worktree/ref metadata was updated
to fetch the base and register this worktree.

## Existing coverage

All files below are in `tests/invariants/cu/` on the requested base.

| Owner file | Existing evidence and boundary |
| --- | --- |
| `inv_027_joint_admission_liabilities.rs` | Fixed joint uncollected maintenance/adverse nontraded lag, either thin party, four transports, explicit versus implicit active settlement, exact margin rejection/admission; also a fixed flat reopen history. |
| `inv_027_standalone_first_admission.rs` | Four sufficiently funded standalone/batch worlds with deferred flat maintenance, later fee collection and exact owner payouts. Does not establish an uncollected flat-fee margin boundary. |
| `inv_027_first_risk_preexisting_lag.rs` | Fixed first-risk lag gate, elapsed fees, common/distinct owner addresses, settlement order and retained request contents on bilateral single/batch routes. |
| `inv_027_flat_reopen_routes.rs` | Fixed prior fee episode, explicit flat settlement, four route switches and senior exits. |
| `inv_027_first_batch_fee_boundary.rs` | Two-asset first batch with per-leg rounded trading fees and explicit maintenance prefixes. |
| `inv_027_generated_flat_fee_entitlement.rs` | Scope F: 64 seeded worlds with nonzero trading/maintenance fees, flat/prior episodes, sync/withdrawal collection, partial/full payouts and exact first-admission margin. Fixed price, one asset, no funding or lag. |
| `inv_027_funding_admission.rs` | Rounded funding debtor/creditor, maintenance, cap carry, first exposure on another asset, opposite-route retry and senior exit before the peer's unconverted junior claim. |
| `inv_027_refilled_first_admission.rs` | Clipped maintenance, self reward and refill followed by another fee interval and explicitly refreshed first admission. |

## New guarantee

One generated public LiteSVM selector is mounted under
`inv_027_protected_principal_seniority::joint_admission_liabilities::generated_first_risk_liabilities`:

`v16_program_generated_first_risk_recertifies_reaged_liabilities_and_senior_exit`.

Four reproducible XorShift input cases (`[0x70; 16]`) sample positive fee rates,
two independently sampled elapsed intervals, fractional quantities and nonzero
target lag. Thin-party identity and trade sign cover their four combinations.
Each case crosses two prior histories, four transports and two certificate
schedules, yielding 64 worlds. This is a sample of numeric inputs, not exhaustive
exploration of the numeric ranges.

1. System/SPL/wrapper instructions create and fund two traders, an unrelated
   senior portfolio and an empty keeper. Mint authority is revoked after funding.
   In half the worlds, a small position opens, ages one slot and closes before
   the first generated fee interval ends. Both histories then have flat traders.
2. Explicit public maintenance sync and refresh precede first admission on asset
   1 through standalone/batch bilateral/CPI transports. Both current certificates
   must match the input ledger and full refresh.
3. A second elapsed interval advances only the market. Trader Account images stay
   exact. An authenticated target adds adverse lag on asset 1 without moving its
   effective price. Fees are again uncollected and both certificates are stale.
4. In 32 worlds, public refresh collects the second interval and establishes
   current certificates. In the other 32, admission itself must perform that
   work. The subsequent first open on asset 0 uses the opposite batching and CPI
   family. Asset 1 remains active, nontraded and lagging in both schedules.
5. A transaction first withdraws the unrelated senior's entire post-fee principal,
   then requests one quantity atom above the modeled admission boundary. The
   admission must reject at instruction 3 with `EngineInvalidConfig`. Complete
   Accounts restore exactly, including the successful withdrawal prefix, token
   custody, market, all portfolios, matcher/context/delegate, mint, wallet
   lamports, account metadata/absence and all compiled transaction accounts. Only
   the payer's known signature fee is deducted.
6. The senior withdrawal commits separately. Admission at the exact boundary
   succeeds, both legs close through public bilateral instructions, and both
   traders withdraw their entire post-fee principal. Every tracked transition
   retains exact owner and insurance attribution.

The net-new composition is a continuing generated owner ledger across
flat/reopened first admission, a fresh active liability interval, nontraded lag,
cross-route recertification and an economic withdrawal prefix that must roll
back with admission. Scope F ends its generated liability setup at flat fee
collection; the fixed joint-liability and funding matrices start with an already
active leg. Those individual cells are reused as controls, not counted as new.

## Oracle and invariant boundaries

The input book advances fee cursors only at committed collection transitions.
For each owner, capital plus paid tokens equals deposited principal minus that
owner's collected maintenance. Each collection splits its fee into the canonical
two insurance domains using the disclosed floor/remainder rule. This matters
because a prior episode can change the split through per-event rounding even
when total fees and final owner capital agree. Route/history equivalence compares
the exact economic certificate lanes, not unrelated identity epochs or a falsely
equal insurance split.

At second admission the thin owner has exactly
`notional(asset 0) + notional(asset 1) + adverse_lag` after all maintenance.
One more quantity atom increases required notional by one. Omitting either the
new maintenance interval or adverse lag would make the excessive request fit;
both preconditions are asserted from the generated inputs. Fees reduce equity
exactly once and lag adds to requirements exactly once.

Every current certificate checks exact equity, initial requirement, maintenance
requirement, worst-case loss and zero liquidation deficit against this book.
The existing independent raw-state checker also verifies currentness keys and
all health lanes. The snapshot full-refresh checker compares engine recomputation
with the independent result and rejects hidden work. Both traders are required
to be current after first admission and after the subsequent risk increase;
checks cannot pass merely because a certificate is stale. Raw positions, OI,
fee credits, mint supply, vault/capital/insurance totals, reservation census,
owner identities and exact SPL payouts are checked alongside certificates.

INV-027 owns preservation of post-liability principal and its exact payout.
INV-024 owns the owner/domain attribution checks; INV-044 receives no-phantom-
credit evidence; INV-053 receives nontraded-leg full-health equivalence; INV-060
receives separate equity-deduction/requirement-addition evidence. INV-010 receives
bounded route and settlement-schedule equivalence, not arbitrary retained-intent
ordering. Both traders are controlled by the harness without an identity oracle,
providing limited INV-062 context; same-address aliasing is covered by the mounted
preexisting-lag selector. Valid public successes and exact rollback support
INV-081.

## Limits and row impact

No implementation conformance mismatch was observed and no production code was
changed. This is bounded conformance, not independent holdout discovery or a
whole-invariant proof. Row 413 remains `OPEN` and its inventory evidence remains
`missing`. Row 434 remains `COVERED`. `coverage_reopenings.tsv`,
`open_findings.tsv` and `invariant_status.tsv` are unchanged. INV-027 and INV-024
remain `REFUTED_CURRENT`; the other six related invariants remain `OPEN_EVIDENCE`.
All eight retain their existing sampled public-route classifications.

The new selector has zero funding, zero trading fees, no positive PnL, no junior
claims, no fee clipping/rewards, no Recovery, no token-rail alternatives and at
most two active legs per trader. Every batch has one leg. Prices do not move;
only the second-stage nontraded target is displaced. Flat first admission always
uses prior explicit fee settlement. Bare standalone admission at an uncollected
flat-fee boundary remains outside this increment. Junior-claim priority remains
the funding selector's existing evidence. Arbitrary histories, retained signed
request permutations, common-address aliasing, multi-leg batches and maximum
account/CU shapes are not newly established.

## Validation

Fresh default-feature program and auth-matcher SBF artifacts were built from
this worktree with platform-tools v1.52 and locked offline dependencies.
SHA-256:

- Program: `71833878f8d52373e501e70ed21d88495051695f2533b4c55362ae90e737aa9d`.
- Matcher: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

The new exact selector passes 1/1 in 38.23s: 64 worlds, 1,200 submitted attempts
checked against the book, 64 complete rollback checks, 2,064 current-certificate
checks and 192 exact principal payouts. Peak submitted-transaction cost is
395,884 CU against a 900,000-CU assertion. Setup/matcher-grant helper calls are
not included in that measured maximum or attempt count. The exact listing
selects one test. The seven related exact selectors pass 7/7 in 74.66s, covering
140 existing worlds. The funding selector includes its nonzero junior claim.
Four metadata selectors pass 4/4, including the reopening projection. Formatter,
diff whitespace and final-commit whitespace checks pass. All four changed files
are under `tests/invariants`; production and machine ledgers are unchanged.

The first matcher build exhausted the shared filesystem's temporary space.
Retry with `TMPDIR` inside this worktree's dedicated tmpfs succeeded. The first
host compile found a local helper argument type mismatch, corrected before the
successful new-selector run. These were build issues, not conformance failures.
Existing metadata-harness dead-code and Solana future-compatibility warnings
remain. No full suite or Kani run is claimed.

Exact commands, from the worktree:

```sh
# Isolated build storage, needed because the shared filesystems were nearly full.
mkdir -p target
sudo -n mount -t tmpfs -o size=8G,uid=1001,gid=1004 tmpfs "$PWD/target"
mkdir -p target/tmp
export TMPDIR="$PWD/target/tmp" CARGO_TARGET_DIR="$PWD/target"
export CARGO_BUILD_JOBS=4 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir target/deploy -- --locked
env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
cargo test --locked --offline --test v16_cu inv_027_protected_principal_seniority::joint_admission_liabilities::generated_first_risk_liabilities::v16_program_generated_first_risk_recertifies_reaged_liabilities_and_senior_exit -- --exact --list
cargo test --locked --offline --test v16_cu inv_027_protected_principal_seniority::joint_admission_liabilities::generated_first_risk_liabilities::v16_program_generated_first_risk_recertifies_reaged_liabilities_and_senior_exit -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_027_protected_principal_seniority::joint_admission_liabilities::v16_program_joint_accrued_liabilities_precede_risk_admission \
  inv_027_protected_principal_seniority::joint_admission_liabilities::standalone_first_admission::v16_program_standalone_first_admission_preserves_deferred_fee_owner_entitlement \
  inv_027_protected_principal_seniority::joint_admission_liabilities::first_risk_preexisting_lag::v16_program_first_risk_after_elapsed_fees_and_preexisting_lag \
  inv_027_protected_principal_seniority::joint_admission_liabilities::flat_reopen_routes::v16_program_flat_reopen_route_switch_preserves_fee_history_and_senior_exit \
  inv_027_protected_principal_seniority::joint_admission_liabilities::funding_admission::v16_program_funding_and_maintenance_precede_new_asset_after_route_rollback \
  inv_027_protected_principal_seniority::joint_admission_liabilities::generated_flat_fee_entitlement::v16_program_generated_flat_fee_collection_preserves_first_admission_entitlement \
  inv_027_protected_principal_seniority::joint_admission_liabilities::first_batch_fee_boundary::v16_program_first_batch_admission_accounts_each_rounded_fee_after_maintenance
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
cargo fmt --all
cargo fmt --all -- --check
git diff --check
sha256sum "$PERCOLATOR_FUZZ_SBF" tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
# After the scoped commit:
git show --format= --check HEAD
```
