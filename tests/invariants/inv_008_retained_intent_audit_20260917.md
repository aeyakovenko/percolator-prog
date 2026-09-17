# INV-008 retained-intent witness audit, 2026-09-17

Base: fetched `origin/main`, `205ef676dd8f534cf6a179dc1ddd0ffacb13c4a1`.
Worktree: `/dev/shm/astra-ultra-retained-intent-audit-20260917`.
Branch: `astra-ultra/retained-intent-audit-20260917`.
Scope: rows **343/344/350/351/355/362/415/428 only**.

## Decision and traceability

All eight named families already have mounted public-route witnesses. No missing
public-route witness was established, so this adds only this audit and its README
link. The review follows `INVARIANTS.md`, current discovery/status/reopening ledgers,
test bodies, discovery helpers and harness mounts; no withheld patch or external
reproduction was used.

The six original rows map to the existing stateful
`inv_008_intent_uniqueness_and_bounded_replay::v16_program_retry_operation_matrix_rejects_every_stale_retry`
in `v16_program_stateful_fuzz`. Its helpers also drive the fixed regression
selectors below. Each helper constructs two retained requests before delivery,
requires initial success, rejects the stale request without economic-frame drift,
checks SPL supply, then requires a newly bound request to change economic state.
This supplies a positive control against blanket rejection.

| Row | Existing public witness and nonzero boundary |
| --- | --- |
| 343 | `pr343` below exercises all four single/batch CPI/no-CPI trade routes at quarter-unit size. The 16 ordered route pairs additionally require bundle rollback, one standalone fill, stale alternate-route rejection, exact bilateral positions and OI, and conserved supply. |
| 344 | `pr344` funds 1,000 atoms through an independent insurance authority. The direct/domain watermark test and both bundle orders bind both insurance entrypoints to the same consumed intent. |
| 350 | `pr350` deposits 1,000 atoms into a publicly initialized portfolio. The same-family bundle matrix requires rollback of the first deposit when its duplicate fails, followed by exactly one mutating standalone execution. |
| 351 | `pr351` installs an independent backing provider and funds 1,000 atoms in domain 1 with maturity 100. Stale/fresh and duplicate-bundle checks preserve the backing intent watermark. |
| 355 | `pr355` withdraws 1,000 atoms from funded capital. Stale retries cannot repeat the payout; bundle rollback leaves one standalone execution available. This is the withdrawal retry family, not a liquidation-history claim. |
| 362 | `pr362` publicly retires asset slot 1, configures a 500-atom activation fee and activates it. After retirement of that generation, the old request still rejects; a fresh activation succeeds. |
| 415 | The generated CU stock witness runs four input shapes, two target assets and six orders: 48 Live histories. Pre-serialized rail/ledger variants cross partial/full debits, three refills, sibling debits, donations and late SPL failures. Per-intent receipts check exact destinations, domain budgets, epochs, supply and rollback; a one-atom destination swap must fail attribution. |
| 428 | The INV-014 `reserve_debit_epoch` Resolved witness publicly funds four budgets (19/41/23/47), resolves, then pays 29 atoms across long/short budgets. Both assets and optional-ledger states must increment only the paid asset's authority epoch exactly once, with exact custody/accounting deltas. The separate Live selector is the control. |

Mounts: [fixed regressions](public_sbf/inv_008_intent_uniqueness_and_bounded_replay.rs)
and [stateful matrix](stateful/inv_008_intent_uniqueness_and_bounded_replay.rs)
are direct harness children. [Generated stock histories](cu/inv_008_generated_insurance_stock_epochs.rs)
are mounted by [INV-008 CU](cu/inv_008_intent_uniqueness_and_bounded_replay.rs);
[reserve debit epochs](cu/inv_014_reserve_debit_epoch.rs) are mounted by
[INV-014 CU](cu/inv_014_delayed_policy_and_policy_epoch_safety.rs).
The existing metadata guard requires rows 415 and 428 to retain these distinct
discovery owners; an isolated debit is not replacement-stock replay evidence.

## Interpretation limits

At this base, `invariant_status.tsv` records INV-008 as
`SUPPORTED / TRANSITION_SYSTEM / PUBLIC_ROUTE / SAMPLED / GLOBAL_CONDITIONAL_TCB`;
`coverage_reopenings.tsv` records 415 and 428 as `COVERED`. Older test comments,
dated audits and the replay-disposition boundary still contain `OPEN` language.
Those historical descriptions do not supersede the current machine ledgers.
No status is changed here.

These are bounded witnesses at engine pin `94979ede7db934545e53a8f210dd063a9ea3ea63`.
The current charter permits one-shot signed Solana requests whose envelope bounds
validity and whose success consumes the program episode. These witnesses do not
establish persistent residual authorization or arbitrary history coverage.
The row-428 selector submits a signed transaction even in Resolved mode: it
checks terminal epoch consumption, not an unsigned-caller or retained-refill
history. The full mount census checks availability, not assertion strength.

## Exact verification

All commands below run from the isolated worktree. A private copy of the existing
host target avoids modifying another worker's build products. Reused default-feature
SBF SHA-256: `87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.
Its source worktree at `cb236b5248c941ffcb33e1e6afe62801e8a19712` has identical
`src/` and `Cargo.lock`; the only manifest difference is host-only `syn` parser
features. The current matcher source matches the main worktree; its artifact
SHA-256 is `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
Neither artifact was rebuilt for this documentation change.

```bash
cp -a --reflink=auto /dev/shm/percolator-public-gap-20260916-c91e-host-target /dev/shm/astra-ultra-retained-intent-audit-20260917-host-target
mkdir -p tests/fixtures/auth_matcher/target/deploy /dev/shm/astra-ultra-retained-intent-audit-20260917-host-target/deploy
cp /dev/shm/percolator-public-gap-20260916-c91e-sbf-target/deploy/percolator_prog.so /dev/shm/astra-ultra-retained-intent-audit-20260917-host-target/deploy/percolator_prog.so
cp /dev/shm/percolator-main-merge-20260916/tests/fixtures/auth_matcher/target/deploy/auth_matcher.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-retained-intent-audit-20260917-host-target
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture \
  inv_008_intent_uniqueness_and_bounded_replay::v16_program_pr343_trade_retry_variants_reject_stale_and_land_fresh \
  inv_008_intent_uniqueness_and_bounded_replay::v16_program_pr344_insurance_top_up_retry_rejects_stale_and_lands_fresh \
  inv_008_intent_uniqueness_and_bounded_replay::v16_program_pr350_deposit_retry_rejects_stale_and_lands_fresh \
  inv_008_intent_uniqueness_and_bounded_replay::v16_program_pr351_backing_top_up_retry_rejects_stale_and_lands_fresh \
  inv_008_intent_uniqueness_and_bounded_replay::v16_program_pr355_withdrawal_retry_rejects_stale_and_lands_fresh \
  inv_008_intent_uniqueness_and_bounded_replay::v16_program_pr362_activation_retry_rejects_after_generation_consumed \
  inv_008_intent_uniqueness_and_bounded_replay::v16_program_insurance_top_up_routes_share_one_replay_watermark \
  inv_008_intent_uniqueness_and_bounded_replay::v16_program_same_transaction_cross_route_retry_is_atomic_and_exact_once \
  inv_008_intent_uniqueness_and_bounded_replay::v16_program_every_retained_family_is_atomic_when_duplicated_in_one_transaction \
  inv_008_intent_uniqueness_and_bounded_replay::v16_program_retained_trade_intent_is_exact_once_across_every_route_pair \
  inv_008_intent_uniqueness_and_bounded_replay::v16_retained_insurance_discovery_metadata_preserves_stock_and_resolved_debit_evidence
cargo test --locked --offline --test v16_cu -- --exact --nocapture \
  inv_008_intent_uniqueness_and_bounded_replay::generated_insurance_stock_epochs::v16_generated_live_insurance_stock_epochs_preserve_retained_rail_entitlement \
  inv_014_delayed_policy_and_policy_epoch_safety::reserve_debit_epoch::v16_live_insurance_debit_consumes_reserve_authority_epoch \
  inv_014_delayed_policy_and_policy_epoch_safety::reserve_debit_epoch::v16_resolved_insurance_debit_consumes_reserve_authority_epoch \
  inv_008_intent_uniqueness_and_bounded_replay::v16_public_replay_disposition_roster_is_source_complete
cargo test --locked --offline --test v16_program_fuzz_regressions \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_every_public_invariant_source_and_declared_test_is_mounted -- --exact --nocapture
git diff --check 205ef676dd8f534cf6a179dc1ddd0ffacb13c4a1
git diff --exit-code 205ef676dd8f534cf6a179dc1ddd0ffacb13c4a1 -- src Cargo.toml Cargo.lock
git diff --cached --check
```

Results: all commands exit 0. Fixed regressions/metadata: **11 passed, 0 failed,
136 filtered**, 7.63s. CU witnesses/source roster: **4 passed, 0 failed,
1,435 filtered**, 25.65s. Row 415 records **48 histories, 2,208 transactions,
1,776 exact rollbacks, 1,392 restored SPL transfers**, peak **224,320 CU**.
Live/Resolved epoch controls each record **four payouts, zero epoch violations**,
peaks **46,968 / 41,775 CU**. The mount census passes **1 test, 146 filtered**,
finding **508 source files and 1,914 available tests**.

Full staged-patch whitespace and production/dependency comparisons pass. The host
build reports existing unused-code and Solana future-compatibility warnings.
No Rust or TSV file changed, so changed-test selectors and Rust/TSV formatting
are not applicable. The existing fixed witnesses were rerun as listed; the
stateful proptest matrix was reviewed but not rerun. No new behavioral test,
production fix, dependency change or status promotion is claimed.
