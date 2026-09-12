# Historical missing coverage audit, 2026-09-12

Base: `19c9ac03` on `origin/codex/astra-open-holdout-ledger-20260912`, rebased from
`61162213` via `44f1367e` after fetching the integration updates. Branch:
`codex/astra-legacy-missing-gap-20260912-8c3f`. Worktree:
`/home/anatoly/percolator-astra-legacy-8c3f`. The original checkout's dirty files
were left untouched. Holdout numbers supplied only coverage labels; no holdout
PR code/tests were inspected or cherry-picked. Evidence comes from the base's
invariant owners, public wrapper, and pinned engine contracts.

## Audit and disposition

Before editing, `rg` audited the six ledger entries, retained fee and withdrawal
owners, first-admission owners, reserve attribution, absent-role terminal
owners, and their module mounts. These are post-hoc conformance increments, not
independent discoveries or invariant closure certificates.

| Label | Existing coverage inspected | Remaining obligation |
| --- | --- | --- |
| 410, INV-024/036/081 | [Terminal role handoff](cu/inv_024_terminal_role_handoff.rs), [earned-fee succession](cu/inv_024_terminal_earnings_succession.rs), and terminal quote/insurance lifecycle siblings | Role-attributed value across the shutdown authority fallback still needs its own generic coverage. New insurance recredit attribution is partial evidence only; it does not exercise that fallback. |
| 411, INV-014/010/011/024/036/047/081 | [Retained fee bundles](stateful/inv_014_retained_fee_bundle.rs), [retained backing caps](stateful/inv_014_retained_backing_fee_cap.rs), and delegated-fee exits | Equal participant caps in the bundle product do not isolate single-CPI taker consent. Backing-fee consent is a different lane. No new fee selector was added. |
| 413, INV-027/010/024/044/053/060/062/081 | [Flat first-admission fee and withdrawal prefixes](cu/inv_027_protected_principal_seniority.rs), [joint admission liabilities](cu/inv_027_joint_admission_liabilities.rs) | Standalone first risk on never-exposed accounts with uncollected fees remains outside those prefixed routes. The existing row-434 close/age/reopen history is distinct and was not duplicated. |
| 415, INV-008/010/011/024/031/064 | [Generated withdrawal stock histories](cu/inv_008_withdrawal_stock_history.rs), [passive reward stock](cu/inv_008_passive_reward_stock.rs) | These own portfolio withdrawals, not retained `WithdrawInsuranceAsset` stock binding. Restoring insurance in the new terminal test does not establish withdrawal replay safety. |
| 420, INV-073 and terminal progress | [Absent-provider expiry](cu/inv_073_absent_provider_expiry_retirement.rs), cooperative provider-earnings cleanup | New coverage composes unused provider backing with actual insurance consumption and all reserve keys absent. Nonexpired principal, provider earnings and generic beneficiary-independent payment remain missing. |
| 421, INV-073 and terminal progress | [Absent insurance roles after exhaustion](cu/inv_073_absent_insurer_spent_retirement.rs), cooperative terminal beneficiary succession | New coverage preserves insurance restored after backing expiry. Public user payout and reserve normalization do not establish payment of that restored beneficiary claim without its signature. |

All six entries remain `missing` in `open_findings.tsv` and `OPEN` in
`coverage_reopenings.tsv`; the historical missing count remains 21. No invariant
status or independent-discovery fingerprint is changed. These remaining gaps
are coverage limits, not newly demonstrated production failures.

## New public conformance

Selector:
`inv_073_no_permanent_user_lock::absent_insurer_spent_retirement::v16_program_absent_reserve_roles_preserve_recredited_insurance_after_backing_expiry`
in [the existing invariant-owned module](cu/inv_073_absent_insurer_spent_retirement.rs).
The existing insurance-only selector shares the public fixture; it retains its
original retirement and one-atom-blocker expectations.

Four worlds cross asset 0/1 and initial insurance 100/101. Three users deposit
1,000, 100 and 137 atoms. A 200-atom trading gain exhausts the debtor's 100 capital
and later consumes 100 insurance atoms. Separately, a provider deposits 307 unused
backing atoms on the insurance-funded side, with expiry at slot 44. The provider,
insurance beneficiary and insurance operator are distinct from the market
authority, users and payer. All three reserve signing keys are dropped after
funding; every measured transaction verifies their absence from its signer set.

Permissionless resolution and timed user exits pay exactly `[1200, 0, 137]`.
Each portfolio reaches terminal economic state in one call, under an eight-call
bound. The user receipts, zero capital/OI, custody and unused provider stock are
checked independently. Owner-signed deletion returns exact portfolio rent to
the slab. At expiry-minus-one, bounded scanning can reach the funded asset but
cannot dispose its fresh provider claim. At the exact expiry, a public slab call
normalizes that backing without changing custody, insurance or unrelated domains;
the receipt ledger's residual increases by exactly 307 while receipt identity and
payout rate remain fixed.

The next slab call restores `min(100 debtor capital, 100 insurance spent, 307
residual) = 100` atoms to insurance. It changes neither SPL balances nor the
provider's empty wallet. A recredit-plus-close bundle rejects on the close suffix:
the test requires the successful wrapper prefix in the logs and complete Account
rollback, including the recredit, cursor and payer's exact network fee. Retrying
recredit alone succeeds. The restored insurance budget is exactly 100/101, vault
custody is 307/308, and a final close preserves the beneficiary claim with exact
rollback. No burn, submitter payout or reserve-beneficiary signature is used.

This adds 70 measured transactions: 38 successes and 32 exact rejections, including
four successful recredit prefixes rolled back by a rejected close suffix. Every
envelope verifies and fits the packet bound. Complete compiled/tracked Account
frames include roles, destinations, mint, custody, portfolios and programs. The
fixed mint supply is 1,644/1,645 throughout; after payout it equals 1,337 paid
plus 307/308 remaining. System/SPL/ATA/wrapper instructions construct all economic
state. Only program installation, signer SOL, Clock and blockhashes use harness
controls; no program-owned bytes are installed or edited out of band.

This is partial INV-024/063/067/073/080/081 conformance. The separate expiry test
has no insurance-spend history; the separate exhaustion test has no funded
provider. The cooperative recredit lifecycle in INV-086 can pay the beneficiary.
The new product keeps every reserve role absent and checks restoration and
atomicity of the resulting claim. It does not overlap the retained-grant
atomicity selector, row-434 reopen history, or INV-082 shared-destination recovery.
The [integrated retained-debit matrix](retained_debit_matrix_20260912.md) composes
withdrawals across incarnation/authority changes; this test has no retained debit
envelopes or post-funding role changes and does not duplicate that coverage.

## Validation

Fresh default-feature wrapper SBF built with platform-tools v1.52 in the private
worktree. A private copy of an existing build cache seeded the target directory.
Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`. Wrapper SHA-256:
`5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.

The new selector passes 1/1 across four worlds; peak measured transaction cost
is **218,409 CU**, below **300,000 CU**. Both exact adjacent controls pass 2/2:
insurance-only exhaustion (peak 225,902 CU) and absent-provider staggered expiry
(peak 94,357 CU). Development corrected assumptions about historical source
receivables, bounded scan continuation, and the receipt ledger's expiry residual;
no production failure was suppressed or recorded as a passing violation.
The exact charter/index, benchmark-disposition and reopening checks pass 3/3;
`cargo fmt --all -- --check` and `git diff --check` pass. The existing unused
support-code and `solana-client v1.18.26` future-compatibility warnings remain.

Reproducible commands from the worktree:

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-legacy-8c3f-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export TMPDIR=/dev/shm CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 inv_073_no_permanent_user_lock::absent_insurer_spent_retirement::v16_program_absent_reserve_roles_preserve_recredited_insurance_after_backing_expiry inv_073_no_permanent_user_lock::absent_insurer_spent_retirement::v16_program_absent_insurance_roles_reach_retirement_only_after_exact_exhaustion inv_073_no_permanent_user_lock::absent_provider_expiry_retirement::v16_program_absent_provider_staggered_expiry_reaches_funded_terminal_retirement
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete inv_079_public_reachability_evidence::v16_dated_open_security_finding_benchmark_is_non_overclaiming inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
cargo fmt --all -- --check
git diff --check
```
