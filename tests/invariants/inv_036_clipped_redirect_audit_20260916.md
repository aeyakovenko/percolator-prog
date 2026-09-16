# INV-036 clipped redirect partition audit

Base: `origin/main` at `2d2af7cc`, rechecked after implementation. Worktree:
`/dev/shm/astra-ultra-inv024-044-cycle3-20260916222908`. Branch:
`astra-ultra/inv024-044-cycle3-20260916222908`.

## Coverage gap

Main's mixed-direction fee matrix clips a payer's capital but disables redirects.
Its retained redirect matrix has fully funded, equal payers. Neither tests the
composition in which a finite payer budget crosses a leg boundary, then each
actual charge is rounded into local and redirected insurance for different
recipients. Stock conservation alone permits both incorrect fee ownership and
moving an odd redirected atom between the base domains.

The new stateful owner is mounted under INV-036. Its expected ledger starts from
deposits and signed closing quantities at the fixed authenticated price 101.
It independently computes requested fees 101 and 202, clips against each owner's
remaining capital, applies the 3,333-bps redirect, and splits each redirect between
base long and short. No production fee helper or observed fee delta determines
the expected result. The unfunded fraction is forgiven; it never becomes a fee
destination balance. Fractional redirect shares stay local, and each odd
redirected atom goes to base short under the existing policy.

Two opposite closing signs, both leg orders, and low capital one atom below or
above the first fee produce these independently checked domain budgets:

| First leg | Low capital | Base long/short | Asset 1 long/short | Asset 2 long/short |
| --- | ---: | --- | --- | --- |
| Asset 1 | 100 | 65 / 68 | 68 / 67 | 0 / 135 |
| Asset 1 | 102 | 65 / 68 | 68 / 68 | 1 / 135 |
| Asset 2 | 201 | 82 / 84 | 68 / 0 | 135 / 135 |
| Asset 2 | 203 | 82 / 85 | 68 / 1 | 135 / 135 |

Each row runs single/batch and CPI/no-CPI, with both physical participant orders:
32 worlds. CPI grants bind the LP's current episode before each close. For each
signed leg order, all transports and participant orders produce identical owner
payouts. Changing the signed leg order legitimately changes request-order fee
attribution; the test derives that change rather than asserting false equivalence.

After fees are collected, the redirect policy changes to 100%. Existing balances
must remain attributed under the earlier policy. Three independently configured
insurance operators then receive exactly their own fee budgets while the funded
trader's 697-atom principal remains protected. Each operator's overdraw, foreign
destination and post-payment reuse attempt rejects with the exact expected error
and full writable-account/SPL rollback. Finally the trader withdraws 697 atoms.
All 1,000 plus low-capital deposit atoms reach their specified recipients, the
quote vault is zero, recipient portfolios and the foreign market remain byte-exact,
and token supply is unchanged. Every captured public transaction passes the
reachability validator with zero out-of-band economic mutations.

## Negative controls

For each world, three cloned observations preserve global stock conservation:

1. Pool all redirected fees before splitting them between base sides. This
   reallocates one atom from base short to long in every row above.
2. Move one insurance atom from asset 2 short to asset 1 long, changing the recipient.
3. Move one capital atom from the funded trader to the depleted trader.

All 96 substitutions pass `assert_market_stock_census` and fail equality with the
same independent `Partition` used by the live observation oracle. Live SVM state
is never mutated by these controls. These are economic attribution controls, not
metadata substitutions or new production counterexamples.

## Validation

The new exact selector passes **1 test**, with **32 worlds, 712 transactions,
288 exact-rollback rejections, 96 balanced controls**, and maximum observed
**466,501 CU**, below the existing 1,400,000 transaction limit. The trace-consumer
inventory increases from 113 to 114 for this validated consumer.

The two adjacent exact CU selectors below pass **2/2**, testing the pre-existing
clipped/no-redirect and funded/redirect boundaries. Five host metadata selectors
pass **5/5**, checking reachability, charter ownership, authoritative statuses,
audit summaries and test-free roots. Total: **8 tests passed, 0 failed, 0 ignored**.
Scoped rustfmt and `git diff --check` pass. No listing-only selection is counted
as execution.

Host dependency artifacts were copied into a private target directory. Wrapper
and authenticated-matcher SBF were built afresh from this worktree with default
features. Commands:

```bash
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-inv024-044-cycle3-20260916222908-target

CARGO_TARGET_DIR=/dev/shm/astra-ultra-inv024-044-cycle3-20260916222908-sbf-target \
  cargo build-sbf --tools-version v1.52 --offline \
  --sbf-out-dir /dev/shm/astra-ultra-inv024-044-cycle3-20260916222908-target/deploy -- --locked
CARGO_TARGET_DIR=/dev/shm/astra-ultra-inv024-044-cycle3-20260916222908-matcher-target \
  cargo build-sbf --tools-version v1.52 --offline \
  --manifest-path tests/fixtures/auth_matcher/Cargo.toml \
  --sbf-out-dir /dev/shm/astra-ultra-inv024-044-cycle3-20260916222908/tests/fixtures/auth_matcher/target/deploy -- --locked

cargo test --locked --offline --test v16_program_stateful_fuzz \
  inv_036_fee_destination_and_policy_version_integrity::clipped_redirect_partition::v16_program_clipped_redirect_fees_preserve_rounding_and_recipient_payouts \
  -- --exact --nocapture

cargo test --locked --offline --test v16_cu -- --exact --nocapture \
  inv_036_fee_destination_and_policy_version_integrity::v16_program_mixed_direction_fee_allocation_matches_independent_side_ledger \
  inv_036_fee_destination_and_policy_version_integrity::v16_program_retained_redirect_bundle_preserves_fee_rounding_and_policy_order

cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture \
  inv_079_public_reachability_evidence::v16_every_public_trace_consumer_validates_reachability_evidence \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_program_invariant_harnesses_are_test_free_roots

rustfmt --edition 2021 --check --config skip_children=true \
  tests/invariants/stateful/inv_036_clipped_redirect_partition.rs \
  tests/invariants/stateful/inv_036_fee_destination_and_policy_version_integrity.rs \
  tests/invariants/public_sbf/inv_079_public_reachability_evidence.rs
git diff --check
git diff --exit-code 2d2af7cc -- src Cargo.toml Cargo.lock
```

SHA-256: wrapper `87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`;
matcher `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
Logs use `/dev/shm/astra-ultra-inv024-044-cycle3-20260916222908-` as their prefix.

## Scope and rejected duplicates

- INV-024 executable entitlement guards, INV-028 row423 metadata, source-realizability
  and stock evidence are already on main and were not repeated.
- The recent INV-036 multi-source witness concerns backing-utilization fees on a
  risk increase with a funded payer. This witness concerns clipped base trading
  fees during mixed-direction risk reduction, followed by insurance payouts.
- Basic clipped mixed-direction routes and funded redirect rounding already have
  witnesses. They are validation neighbors here, not claims of new discovery.
- Insurance-backed source liens remain engine-only; their absent public ingress
  was not repackaged as executable lifecycle evidence.

Open PR and issue titles were inspected only after selecting and implementing the
new matrix, as withheld comparison. No bodies, patches or tests were imported.
The issue about insurance withdrawal loss barriers is outside this flat,
zero-PnL payout history.

Remaining gaps include arbitrary price/funding histories, clipping combined with
source liens or mark-externality fees, fractional position sizes, hostile partial
fills, concurrent losses during insurance payout, native-token rails, terminal
account retirement and policy changes between fee-bearing legs. This increment
does not certify delayed fee authorization or close any open finding.

Safe for main as tests/docs only: no production, engine pin, dependency, finding
classification or invariant-status change; no new LoF, DoS or CU bug confirmed.
