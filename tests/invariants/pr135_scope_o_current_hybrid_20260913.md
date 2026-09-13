# Scope O: Current Hybrid Recipient Observations

Base: `origin/codex/astra-open-holdout-ledger-20260912` fetched at
`5a922618a70734e4a6db25095aadb2b94b6d9702`.
Branch: `codex/pr135-scope-o-current-hybrid-observations-20260913`.
Worktree: `/run/user/1001/percolator-scope-o-current-hybrid-20260913`.
Only this new worktree contains source edits and build artifacts. No open PR
branch, diff or test was consulted. Row 426 is a coverage label only.

## Ownership And Existing Evidence

INV-020 owns the new test. INV-053/054/056 receive complete-current health and
admission comparisons; INV-024 receives recipient value, penalty/reward/domain
and custody checks. INV-061/071/072 receive bounded keeper liquidation progress
and observation-order evidence. INV-081/086 receive public state validation,
full Account rollback and finite history equivalence. No universal proof is added.

The requested base already contains these distinct bounded owners:

| Existing owner | Covered boundary; distinction from this increment |
| --- | --- |
| INV-020 `partial_observation_routes` | Three-asset bilateral reductions and sibling risk admission with pending AuthMark loss. Recipient Hybrid and CPI are absent. |
| INV-020 `active_keeper_observations` / `cpi_keeper_observations` | One active AuthMark recipient's partial/full refresh and four-route economics. The recipient has no external Hybrid feed. |
| INV-020 `reward_recipient_liquidation` | A composite-Hybrid recipient becomes a second underwater target; it has no CPI recipient admission/exit product. |
| INV-020 `renewed_liquidation`, `repeated_active_rewards`, `reward_payout_rollback` | Renewed target/recipient episodes or rollback after reward payout; these do not supply this independent target/recipient Hybrid plus four-route matrix. |
| INV-053 full-health recertification | Stale related-leg refresh and full-health equivalence; this increment composes it with an externally observed paid CPI recipient. |
| INV-056 mixed observations / observation membership | Hint/admission boundaries and roster normalization; no paid Hybrid recipient transport product. |
| INV-061 caught-up portfolio sizing | Independent multi-asset liquidation quantity enumeration and beneficiary value; retained as an adjacent sizing control, not duplicated here. |

## Generated Domain And Oracle

The Cartesian history domain is four trade transports times two observation
orders times explicit/on-demand recipient refresh times clean/interrupted
execution: 32 public worlds. Asset 0 and recipient asset 2 each use a separate
single-feed Hybrid/Pyth source. Assets 1 and 3 use AuthMark. Four distinct owners
deposit `[10000000, 220000, 300000, 10000000]` atoms through System/SPL/wrapper
routes; mint authority is revoked. Oracle fixtures, Clock, program loading and
signer SOL are environmental inputs. No initialized economic Account is injected
or restored, and detached oracle copies are never installed in LiteSVM.

Current reports are first accepted at slot 0/time 101. Bounded catchup advances
market prices to `[1040000, 1050000, 1050000, 1000000]` at slot 64/time 102.
The first catchup step stops at 32 without changing any portfolio. Input-derived
checks require exact report timestamps/prices and retain original provenance
slot 0 separately from market slot 64. Reusing the report does not renew it.
The target's fully refreshed equity/margin/deficit are 130000/209000/79000.

For each interrupted world, the generator crosses three refresh target roles
(active peer, liquidation target, active reward recipient), missing declared
tail versus regressed report, either Hybrid source and transaction prefix versus
suffix. Removing the first external account shifts the other source into its
lane and fails `InvalidOracleKey`; removing the last fails
`NotEnoughAccountKeys`. Regressed supplied reports fail `OracleStale`.
A suffix rejection must occur at instruction 4, after liquidation and
recipient risk admission have both completed. Prefix rejection is instruction 2.
Full `Option<Account>` frames include all tracked and compiled accounts: market,
portfolios, intents, owners, admin, mint/vault/destinations, sources, Clock,
matcher program/context/delegate and absent addresses. The exact runtime
signature fee is the only payer adjustment. Every attempt runs stock,
reservation, current-certificate, shape and fixed-supply checks.

Complete evidence commits the same liquidation and admission instructions. The
keeper's credited reward invalidates its certificate while its Hybrid leg remains
unsettled. Explicit recipient refresh and on-demand trade refresh must settle the
same 50000-atom loss. Both owners' current certificates are checked against the
independent raw-state model AND a detached full refresh, with nonzero counters;
input-derived equity, all economic health lanes, position signs and OI further
constrain the outcome. Matcher-context changes distinguish single CPI from the
batch return-data route. Single/batch exits and actual recipient SPL withdrawal
also run as successful prefixes followed by a stale recipient observation.

The penalty formula uses observed executed close quantity, rounded notional,
100 bps and the 10000-atom cap. Reward is floor(penalty * 3333 / 10000).
The final recipient entitlement is 300000 + reward - 50000. Other owner values,
OI, insurance split and all token destinations must agree across all 32 worlds.
This checks fee attribution and route equivalence, not an independent minimal
liquidation-quantity oracle; INV-061 owns the latter.

## Limits And Status

Row 426 remains OPEN; `open_findings.tsv` and `invariant_status.tsv` are unchanged.
Missing **declared account tails** and regressed supplied reports are rejected
input controls. They do not test omission of a Hybrid observation hint while
active account certification proceeds. Every favorable success has complete
current market evidence first. This test therefore does not close the remaining
generic observation-completeness family or claim an independently found issue.

Other limits: two single-feed Pyth sources, one liquidation/reward, solvent active
recipient, integral lots, unit ADL, zero funding and trading/maintenance fees,
fixed liquidation policy, four assets/owners, Live mode and classic SPL. Mixed
provider/composite epochs, unavailable sources and fallback, provider replacement,
mixed gain/loss recipient legs, repeated rewards, larger generated histories,
maximum shapes, Recovery and terminal claims remain outside this increment.
No production fix or dependency change is included.

## Validation

Program and matcher were freshly built here with default features, engine
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`, platform-tools v1.52 and locked offline
dependencies. SHA-256:

- Program: `71833878f8d52373e501e70ed21d88495051695f2533b4c55362ae90e737aa9d`.
- Matcher: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

The focused exact selector passes 1/1 in 25.46s: 32 worlds, 384 generated
refresh failures (128/role, 192/missing-or-stale class and 192/prefix-or-suffix),
24 owner-close suffix rollbacks, 16 post-SPL suffix rollbacks, 224 nonvacuous
independent/full-refresh comparison pairs and 32 exact recipient payouts.
Peak cost is 731334 CU, bounded at 900000; all packets are at most 1232 bytes.
Executed remaining quantity is 155980, penalty 8778, reward 2925 and recipient
payout 252925 atoms in every world. No implementation issue is established in
this bounded conformance family. During development, test expectations were
corrected for first-acceptance provenance, recipient-owner signing and ordered
missing-account parser behavior; these were fixture/oracle corrections only.

The final rerun with the 900000-CU bound plus all seven adjacent selectors below
passes **8/8** in 64.38s; the new selector's rerun peak is 726834 CU (731334 is
the highest observed across runs). The exact listing selects one test. All four
final INV-079 metadata gates pass **4/4**, including the reopening-set gate.
Formatter and whitespace checks pass. The full suite and engine proofs were not
run; there are no changes to production, fixtures, dependency pins or statuses.
The command block lists separate focused and adjacent invocations; the final run
passed the same new selector as an additional exact filter to the adjacent command.

```sh
export CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0
export TMPDIR="$PWD/target" PERCOLATOR_FUZZ_SBF="$PWD/target/deploy/percolator_prog.so"
env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir target/deploy -- --locked
env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
scope_o=inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations::active_keeper_observations::cpi_keeper_observations::generated_current_hybrid::v16_program_generated_current_hybrid_recipient_routes_match_full_health_and_rollback
cargo test --locked --offline --test v16_cu "$scope_o" -- --exact --list
cargo test --locked --offline --test v16_cu "$scope_o" -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations::active_keeper_observations::cpi_keeper_observations::v16_program_cpi_active_keeper_observations_preserve_admission_and_payout \
  inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations::active_keeper_observations::reward_recipient_liquidation::v16_program_reward_recipient_becomes_liquidation_target_after_composite_refresh \
  inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations::partial_observation_routes::v16_program_partial_sibling_observations_cannot_expand_single_or_batch_risk_capacity \
  inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations::renewed_liquidation::v16_program_renewed_observations_preserve_repeated_liquidation_certificates \
  inv_053_full_health_recertification_equivalence::v16_bpf_trade_refreshes_stale_related_portfolio_leg_on_demand \
  inv_056_hints_are_discovery_only_favorable_actions_fully_refresh::v16_bpf_inv056_mixed_observations_preserve_full_refresh_trade_boundary \
  inv_061_deterministic_bounded_liquidation::enumerated_public_sizing::caught_up_portfolio_sizing::v16_program_caught_up_multi_asset_sizing_preserves_health_and_beneficiary_value
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
```
