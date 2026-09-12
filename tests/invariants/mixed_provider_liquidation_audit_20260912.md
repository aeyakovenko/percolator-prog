# Mixed-provider liquidation observation relation, 2026-09-12

Base: `origin/codex/astra-open-holdout-ledger-20260912`,
`adf21c4793b7c4202db8e9433c383bebb982d254`.
Worktree: `/tmp/percolator-astra-ultra-mixed-liquidation-f91d`.
Branch: `codex/astra-ultra-mixed-liquidation-20260912-f91d`.
Only base source, tests and invariant documents informed this increment. No open
PR diffs were fetched or copied; the shared checkout was not edited. No push.

## Overlap Search

`rg` searched `tests/invariants/cu/inv_{020,027,028,045}*.rs` and
`tests/invariants/oracle*md` for `first.admission`, `staged_observations_match_current`,
`composite_epochs_gate_real_liquidation`, `authenticated_reward_handoff`,
`hybrid_capacity_carry`, `interleaved_cap_carry`, `active_claim_evidence`, and
`omitted.Hybrid`. Relevant implementations and wrapper observation/fee handling
were read before writing the new mounted child.

| Existing owner | Distinction |
| --- | --- |
| INV-020 staged observations | Pyth/AuthMark; active schedules omit AuthMark, but do not omit the selected Hybrid asset. New histories use two external-provider assets without snapshot restoration. |
| INV-020 composite liquidation | Multiple providers compose one asset; no two-asset observation-membership comparison with an exact fee/domain ledger and keeper SPL payout. |
| INV-045 authenticated reward handoff | Paid-mark/fresh-report handoff on one asset; no selected-asset omission after complete mixed-provider refresh. |
| INV-020 current health and active-claim evidence | Owner exit/conversion after refresh; no rewarded liquidation in the two-provider membership relation. |
| INV-027 first-risk, INV-028 Hybrid capacity, INV-045 interleaved carry | Liability/source/carry owners are already integrated; their histories were not duplicated. |

## New Relation

[`cu/inv_020_mixed_provider_liquidation.rs`](cu/inv_020_mixed_provider_liquidation.rs)
is mounted below `inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations`.
Eight fresh worlds cross Pyth/Switchboard versus Pyth/Chainlink, full observation
order, and full versus omitted-selected-asset evidence on the liquidation step.
Both assets are publicly opened, adverse reports are authenticated, and the target
is fully refreshed before the omission comparison. No pending-omission theorem
is claimed. The selected asset remains asset 0 in every world.

Two unit shorts independently lose 40,000 + 50,000 atoms. Equity becomes 130,000,
maintenance margin 209,000, and deficit 79,000. The selected asset closes 844,020
quantity units. Independently rounding its notional, then applying the 100-bps
fee and 3,333-bps keeper share, yields an 8,778-atom target debit, 2,925-atom keeper
credit and 5,853 insurance atoms, split 2,926/2,927 across asset 0's domains.
The peer retains 10,090,000 marked atoms; the target retains 121,222 atoms. The
keeper withdraws exactly 3,925 SPL atoms (principal plus reward). Economic outputs
agree across all worlds, with current independently recomputed certificates,
stock/source/encumbrance censuses, fixed mint, custody and unrelated-owner frames.
Position epoch advances exactly once and the other asset's OI stays unchanged.

Each world has five exact transaction rollbacks: old-but-wall-clock-fresh evidence
before refresh and at liquidatable current health; two late rejections after an
independently successful refresh/liquidation prefix; and a nonprogress retry after
health restoration. All tracked and compiled Accounts, including full payer
metadata less the exact signature fee, are compared. The same successful prefix
is then committed and its economic effects checked. Eight late rollbacks contain
a paid liquidation. There is no account snapshot restoration or protocol-byte
injection; economic accounts use System/SPL/ATA/wrapper instructions. Clock,
signer SOL and external vendor reports use the existing fixture helpers. Vendor
publication/signature verification remains a fixture assumption.

Initial development removed assumed-reject omission probes after an omission
returned success. The trace had not independently established the intended
pending-action precondition; no rejection theorem or production diagnosis is
claimed from that probe. A keeper-certificate setup
assertion was corrected, and the two-asset action uses the adjacent staged-action
500,000-CU ceiling rather than the generic 325,000 crank guardrail. No production
or dependency changes, holdout closure, or invariant-status promotion is included.

## Validation

The new selector passes 8 worlds / 40 exact rollbacks. Peak CU by phase:
publication **61,646**, standalone rejection **70,116**, late rollback **419,973**,
refresh/liquidation **382,700**, keeper payout **55,095** (the earlier standalone
run peaked at **56,595**). Two-asset instructions
are bounded at 500,000 CU; two-instruction rollback bundles at 1,000,000; SPL payout
at 300,000. These results do not claim the generic 325,000-CU crank ceiling.
The new selector and four adjacent controls pass **5/5**; the invariant index
passes **1/1**. `cargo fmt --all -- --check` and `git diff --check` pass.
Control CU: observation replay peaks at **301,930**; staged observations at
**380,553**; composite provider liquidation at **296,367**; authenticated handoff
at **346,435**. Logs are `/dev/shm/astra-ultra-mixed-liquidation-f91d-selectors.log`
and `/dev/shm/astra-ultra-mixed-liquidation-f91d-index.log`. The existing
`solana-client v1.18.26` future-incompatibility warning remains.

Fresh locked/offline default-feature SBF and host builds use private outputs.
Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
Wrapper SHA-256: `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
Exact commands and selectors:

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-mixed-liquidation-f91d-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=6 CARGO_INCREMENTAL=0 TMPDIR=/dev/shm
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations::mixed_provider_liquidation::v16_program_mixed_provider_liquidation_omissions_preserve_exact_entitlements \
  inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations::v16_program_staged_observations_match_current_liquidation_and_reduction \
  inv_020_authenticated_clock_slot_and_oracle_provenance::liquidation_observation_replay::v16_program_liquidation_rejects_rewound_observations_after_authenticated_market_move \
  inv_020_authenticated_clock_slot_and_oracle_provenance::v16_program_composite_epochs_gate_real_liquidation_across_provider_roles \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::v16_program_paid_discovery_fresh_handoff_authenticates_liquidation_and_keeper_exit
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
```

## Remaining Gaps

| Holdout | Disposition |
| --- | --- |
| #413 | OPEN: standalone first admission with uncollected liabilities; no new coverage here. |
| #422 | OPEN: partial fresh-evidence reward attribution with omitted selected-asset discovery; general paid-mark histories, other selected assets and exposed-owner terminal exits remain. |
| #423 | OPEN: over-capacity source admission/recovery and general future-resource contention; no new coverage here. |
| #425 | OPEN: trade before uncommitted accrual and terminal carry; no new coverage here. |
| #426 | OPEN: partial two-provider omission after complete refresh; omitted pending/uncommitted evidence, other provider/asset assignments, composites and all favorable routes remain. |

Liquidation quantity is a deployed input to the independent fee oracle, not an
independent liquidation-sizing proof. This is bounded current-code conformance.
