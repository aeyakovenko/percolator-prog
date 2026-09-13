# First-risk liabilities and authenticated liquidation handoff, 2026-09-12

Base: `origin/codex/astra-open-holdout-ledger-20260912` at
`39022191e695d6702fcb3fcdb5b7cc270dc927d2`.
Branch: `codex/astra-oracle-liquidation-gap-20260912-c7e4`.
Worktree: `/tmp/astra-oracle-liquidation-gap-20260912-c7e4`.
The shared checkout was not edited. Only the requested base's sources and tests
informed these tests; no open PR branches were fetched or used as source.
Production, dependencies, holdout verdicts and row 434 are unchanged.

## Overlap and New Relations

`rg` searched existing selectors before implementation. INV-027's first-admission
fee prefix has equal account ages and no incoming reward; INV-088's unequal-age
reward recipient ends in withdrawal, without first risk. The covered flat-reopen
history is separate and was not duplicated. The new
[`inv_027_reward_recipient_first_risk.rs`](cu/inv_027_reward_recipient_first_risk.rs)
joins incoming reward, unequal account ages and the recipient's own unpaid fees
before its first exposure. Eight worlds cross fee-time partition, participant role
and single/batch admission. Public fee/refresh instructions precede admission.

The independent ledger derives 42/28 maintenance atoms, a 9-atom reward and
1,958/981 remaining owner atoms. A 982-atom margin request rejects after the
successful fee/refresh prefix, restoring all tracked and compiled Accounts except
the exact signature fee. The 981-atom request succeeds with both current
certificates equal to independent recomputation. Both owners reduce, withdraw
their exact entitlements and delete their portfolios; only 61 insurance atoms
remain in custody. Fee-time partition and route/role changes preserve this result.

INV-045's trade-origin test ends with stale reports; its accepted-price reward
test begins with fresh reports. Neither owns the intervening authenticated handoff.
The new [`inv_045_authenticated_reward_handoff.rs`](cu/inv_045_authenticated_reward_handoff.rs)
crosses two equivalent paid prints, two fresh report targets, two keeper shares
and separate/combined report publication in sixteen worlds. Paid discovery first
establishes a 992,320 target while settlement remains at 1,000,000. A fresh report
then establishes a 997,600 accepted-price frontier. The fee oracle independently
rounds the actual closed quantity at that accepted price, distinguishing the raw
print, accepted trade print, paid target and fresh report target. It independently
allocates the target debit, keeper credit and insurance remainder.

At a current certificate with nonzero liquidation deficit, missing advertised
oracle accounts, stale reports and same-time/different-price reports reject with
exact errors and rollback. Before each accepted refresh/liquidation, a valid
independently simulated prefix followed by equivocal evidence also rejects and
rolls back the prefix. An identical fresh retry after health restoration rejects
`EngineNonProgress`, preserving the paid prefix. Bounded public settlement obtains
current independently recomputed certificates for all four exposed owners, checks
their input-derived PnL and completes the keeper's exact principal-plus-reward SPL
payout. Stock, source-rate and encumbrance censuses run after accepted measured
cranks; unrelated owners and custody are framed across liquidation.

System/SPL/ATA/wrapper instructions create all protocol and custody state. There
is no protocol-state injection or snapshot restoration. Signer SOL, Clock and
external Pyth report accounts use the existing harness fixtures. Actual Pyth
publication/signature verification remains a fixture assumption.

## Holdout Disposition

| Row | Increment | Remaining gap |
| --- | --- | --- |
| #413 | Never-exposed reward recipient's own liabilities before first risk; unequal ages, partition/role/route equivalence and full owner exit | Standalone admission without explicit fee/refresh prefix; arbitrary liability histories |
| #422 | Paid discovery through fresh report handoff, actual liquidation fee/reward attribution and keeper payout | General paid-mark histories, CPI, upward moves, multi-asset selection and complete exposed-owner exits |
| #423 | Existing integrated Hybrid historical-capacity selector rerun as a control | No new capacity-reservation theorem; general future-resource contention remains open |
| #425 | Integrated capacity/carry and interleaved carry/reward selectors rerun as controls | No new fractional-carry relation; trade before uncommitted accrual and terminal carry remain open |
| #426 | Missing advertised accounts, stale/equivocal evidence at a liquidatable current certificate; exact late rollback and retry | Omitted active-asset discovery hints, mixed providers, multiple active assets and all favorable routes |

New partial coverage joins INV-020/024/027/045/053/054/060/061/071/073/080/081.
INV-028/046/056/057/077 remain adjacent or residual obligations, not new claims of
complete coverage. All five holdouts remain **OPEN**. No vulnerable-pin replay or
independent red/green discovery evidence was produced; these are current-code
conformance tests. Closed quantity is a deployed input to the reward oracle,
not an independent proof of liquidation sizing.

## Validation

Both new selectors pass: **24 worlds and 104 exact rollbacks**, including 16
late rejections whose independently successful prefix pays a liquidation reward.
First-risk bundles peak at **316,092 CU** under 600,000; handoff transactions peak
at **346,435 CU**, with 325,000 per crank and 650,000 per two-crank bundle.
All seven adjacent exact selectors and the invariant index pass. Formatting and
whitespace checks pass. Two controls initially lacked the local matcher artifact;
both pass after its fresh build. Test development corrected oracle inversion and
certificate lifecycle assumptions. No production conformance failure was observed.
Existing unused-support and `solana-client v1.18.26` compatibility warnings remain.

Fresh default-feature wrapper SBF and authenticated matcher SBF were built from
this worktree with locked/offline platform tools v1.52. Host outputs use a private
copy of the base audit's dependency cache. Engine pin:
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
Wrapper SHA-256: `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

Exact commands (the matcher output is linked at the fixture's ignored `target/deploy`):

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-oracle-liquidation-c7e4-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 TMPDIR=/dev/shm
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_027_protected_principal_seniority::joint_admission_liabilities::reward_recipient_first_risk::v16_program_never_exposed_reward_recipient_settles_own_fees_before_first_risk \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::v16_program_paid_discovery_fresh_handoff_authenticates_liquidation_and_keeper_exit \
  inv_027_protected_principal_seniority::v16_program_flat_first_admission_fee_prefix_is_atomic_and_entitled \
  inv_027_protected_principal_seniority::joint_admission_liabilities::v16_program_joint_accrued_liabilities_precede_risk_admission \
  inv_045_no_free_mark_movement::trade_origin_catchup::v16_program_trade_origin_liquidation_prices_and_entitlements_survive_catchup_order \
  inv_045_no_free_mark_movement::accepted_price_reward::reward_catchup_order::v16_program_reward_price_tracks_actual_catchup_across_report_and_crank_orders \
  inv_020_authenticated_clock_slot_and_oracle_provenance::active_claim_evidence::v16_program_active_claim_conversion_distinguishes_current_cert_from_complete_evidence \
  inv_028_source_domain_realizability_cap::historical_latent_capacity::hybrid_capacity_carry::v16_program_historical_capacity_preserves_hybrid_carry_health_and_owner_entitlement \
  inv_045_no_free_mark_movement::interleaved_cap_carry::v16_program_interleaved_trade_routes_preserve_oracle_cap_carry_and_reward_provenance
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
```
