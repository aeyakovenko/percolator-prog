# Timestamp renewal, maintenance and funded admission

## Isolation and scope

- Base: `abfaaf4bda787a99075e693c5c6fe908d5eacd23`, the requested current HEAD of
  `/tmp/percolator-astra-watch.Cb2E7d` on
  `codex/astra-open-holdout-ledger-20260912` when this contribution started.
- Worktree: `/tmp/percolator-auth-health-conformance.IGdvkJ`.
- Branch: `codex/auth-health-conformance-20260912`.
- Engine: Cargo-pinned `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Child: `cu/inv_020_fee_refresh_admission.rs`, mounted by INV-020 as
  `fee_refresh_admission`.
- Exact selector:
  `inv_020_authenticated_clock_slot_and_oracle_provenance::fee_refresh_admission::v16_program_timestamp_renewal_fee_refresh_and_funded_admission_retry`.

The contribution uses the supplied HEAD's production implementation, pinned engine,
public LiteSVM helpers, invariant charter, and existing integrated coverage. No open
PR branch, diff or test was inspected or copied. The excluded selected-provider,
mixed-provider liquidation, active-claim and unfinished fractional-reward test
bodies were not read. Their distinctions below use the task and integrated README
or module descriptions. Neither the main workspace nor integration worktree was
edited. Production sources, shared harness, dependency pins, invariant verdicts and
reopening rows are unchanged. There is no fixed-pin closure claim.

## Distinct relation

Eight worlds cross two active-leg insertion orders, two observation orders, and two
schedules: explicit successful refresh before admission, or an atomic
deposit/refresh/admission bundle after rejected inputs. Caller slot hints are zero
for one observation order and `u64::MAX` for the other; both execute at authenticated
slot 1. These are coupled axes, not an exhaustive slot-hint product.

| Prior evidence | Distinction here |
| --- | --- |
| `selected_provider_assignment` | Two Pyth assets retain the same provider assignment. The discriminating action is signed risk admission at the maintenance-adjusted IM boundary. |
| `mixed_provider_liquidation` | Both participants remain healthy; stale evidence restores a real SPL deposit prefix and a complete-observation retry admits new exposure. |
| Active-claim evidence | There are no source claims, conversion requests, liens or positive PnL. Both payouts come from each owner's deposited principal less attributed maintenance. |
| Unfinished fractional-reward attempt | Prices are constant integers and no reward is earned. The nonzero debit is an exact seven-atom maintenance charge per trader. |
| Existing maintenance-before-admission matrix | External report renewal, full-observation rollback, a real custody prefix, split/bundled equivalence and complete owner payout compose on two already-active legs. |

## Public history and oracle

System, SPL Token, ATA and wrapper instructions create and fund every economic
account. The existing public SPL bootstrap supplies the market and vault; this
test creates its portfolios through System and `InitPortfolio`, mints through SPL,
and deposits through the wrapper. Mint authority is revoked at exactly 394 atoms.
Harness changes are signer SOL, Clock, blockhashes, and simulated provider-owned
Pyth reports. No program-owned economic bytes are injected, restored or patched.
Snapshot full refresh uses disposable decoded copies and never writes into LiteSVM.

At slot 0 / Unix time 100, each trader deposits 167 atoms and an unrelated owner
deposits 23. The first trader retains a 37-atom SPL top-up. The book opens three
lots at price 100 and two at price 200, with opposite positions for the two owners.
IM/MM are 20%/10%; integer floors are 11/10, below every nonempty tested leg's
requirements. The 24-bps per-slot movement bound and nonzero 5% capped liquidation
fee form an admissible configuration. Funding and trade fees are zero.

At authenticated slot 1 / Unix time 161, each trader owes seven maintenance atoms.
Prices stay 100/200. Reports renew only their publication timestamps. In retry
worlds, the first observation is fresh while the second remains at time 100, one
second beyond the configured 60-second freshness limit. The already-constructed
deposit/refresh/admission bundle rejects at instruction 3 with `OracleStale`.
The exact tracked Accounts, including data, economic lamports, custody, owners,
providers, programs and the funded peer, equal the pre-transaction snapshot.
The network transaction-fee payer is outside this economic frame.

After correcting only the second report timestamp, another bundle successfully
executes the deposit and refresh prefix but rejects at instruction 4 with
`EngineInvalidConfig`: its risk increase is one `POS_SCALE` lot plus one position
quantum. That extra quantum increases rounded notional by one atom and total IM
from 160 to 161. The counterparty has exactly `167 - 7 = 160` atoms. Exact rollback
therefore includes the SPL transfer, oracle renewals, fee collection, certificates
and attempted position change. The original complete bundle, without rebuilding
its instruction data or account lists, succeeds with exactly one additional lot.
Transaction envelopes receive fresh blockhashes; this is instruction-content retry,
not a durable-nonce or retained-signature test.

Control worlds execute the deposit/refresh prefix separately. Their input-defined
refresh rank is `(sum(1 - asset.slot_last), 1 - last_fee_slot, certificate_stale)`.
Each successful refresh strictly decreases this finite lexicographic rank to
`(0, 0, 0)`. Only the first trader's seven-atom fee has been collected at this
checkpoint; the subsequent admission refreshes and charges the counterparty.
The final complete book and per-owner entitlements equal the bundled schedule.

After admission, positions are four/two lots. The independent health oracle is:

| Lane | First trader | Counterparty |
| --- | ---: | ---: |
| Capital and equity | 197 | 160 |
| Initial requirement | 160 | 160 |
| Maintenance requirement | 80 | 80 |
| Liquidation deficit | 0 | 0 |
| Worst-case notional | 800 | 800 |

Every certificate also binds all four current market epochs and the active bitmap,
and equals engine snapshot full refresh. The counterparty has exactly zero IM
headroom. Both oracle publication times are 161, liveness slots and engine asset
cursors are 1, and caller slot hints cannot change economic time.

Empty and complete same-slot hints then probe the current healthy account. A valid
no-op is accepted; `EngineNonProgress` requires exact rollback. Either outcome
must preserve the independently specified book, health, fees and entitlements.
This deliberately does not fail solely because the crank has no remaining work.

Both owners close their legs in reverse insertion order and withdraw through
their public SPL ATAs. After every tested fill, refresh checkpoint and withdrawal,
the oracle checks per-owner capital and wallet amounts, signed positions and exact
OI/counts, zero PnL/fee debt, absence of close/receipt state, market/account shape,
fixed mint supply and raw/booked custody. After every fee-adjusted admission and
owner reduction it checks all certificate lanes and full-refresh equivalence.

Owner payouts are exactly `167 + 37 - 7 = 197` and `167 - 7 = 160`. The untouched
portfolio remains byte-exact with 23 atoms. Vault residue is exactly 37 atoms:
23 protected peer capital plus 14 insurance. Maintenance goes only to asset 0,
split per account as 3/4 atoms, producing domain budgets 6/8; asset 1 receives zero.
No claim, liquidation penalty or unassigned residual explains an owner payout.

## Evidence limits

This supplies bounded INV-020/024/053/054/056/071/072/081/086 evidence. INV-061 gains
only healthy-account preservation with nonzero liquidation-fee configuration;
selection, sizing and positive liquidation rewards are not exercised. Rank evidence
covers the four explicit refresh checkpoints, not a general liveness theorem.
The arithmetic oracle and snapshot comparison cover these finite zero-PnL worlds,
not general reference-model/deployed equivalence or the charter's proof obligations.

Economically pending evidence omission is not constructed: with unchanged prices
and zero funding, missing observations need not change accrual. Empty hints are
tested only after full current evidence. Stale evidence supplies the exact
rollback/retry witness. Moving prices, funding, partial catchup, active claims,
liens, ADL, insolvency, arbitrary provider histories, CPI/batch routes, noncollectible
fees, shutdown/resolution, peer and insurance payout, and maximum shapes remain
outside this increment. No invariant or holdout status is promoted.

## Validation

Fresh default-feature SBF build passed and matches the integration artifact:
SHA-256 `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
All build outputs are private under `/dev/shm/percolator-auth-health-IGdvkJ-target`.
No matcher binary is needed. The new selector passes **1/1** with eight worlds,
four stale rejections, four one-quantum margin rejections, four measured refreshes
and sixteen quiescent probes. Nearby controls pass **8/8**; invariant charter/index
passes **1/1**. Scoped rustfmt, whitespace and production/harness/pin/status equality
checks pass. The complete suite and engine proofs are not run.

During construction, two inadmissible market configurations rejected at initialization:
the default 100% movement allowance exceeded the requested 10% maintenance budget,
then a two-atom MM floor could not cover separate ceiling-rounded movement and
liquidation charges at small notionals. A 24-bps movement bound and 10/11-atom
MM/IM floors satisfy the configuration contract and do not bind the tested positions.
No normal public conformance transition failed. Existing unused-support warnings
in the index target and the `solana-client` future-incompatibility warning remain.

Commands run from the isolated worktree:

```sh
export CARGO_TARGET_DIR=/dev/shm/percolator-auth-health-IGdvkJ-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export TMPDIR=/dev/shm CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu inv_020_authenticated_clock_slot_and_oracle_provenance::fee_refresh_admission::v16_program_timestamp_renewal_fee_refresh_and_funded_admission_retry -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_020_authenticated_clock_slot_and_oracle_provenance::v16_bpf_hybrid_fresh_oracle_trade_opens_and_closes \
  inv_020_authenticated_clock_slot_and_oracle_provenance::v16_program_crank_future_now_slot_does_not_overaccrue \
  inv_020_authenticated_clock_slot_and_oracle_provenance::v16_program_unchanged_oracle_report_cannot_renew_withdrawal_window \
  inv_053_full_health_recertification_equivalence::v16_bpf_trade_refreshes_stale_related_portfolio_leg_on_demand \
  inv_060_single_sided_margin_and_penalty_accounting::v16_program_fee_and_target_lag_compose_exactly_once_in_health_lanes \
  inv_061_deterministic_bounded_liquidation::v16_program_healthy_account_not_liquidatable \
  inv_071_crank_progress::v16_regression_crank_idempotent_at_settlement_fixed_point \
  inv_072_order_robust_crankability::v16_program_permissionless_crank_valid_hint_order_is_normalized
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
rustfmt --edition 2021 --check --config skip_children=true tests/invariants/cu/inv_020_authenticated_clock_slot_and_oracle_provenance.rs tests/invariants/cu/inv_020_fee_refresh_admission.rs
git diff --check
git diff --cached --check
git diff --exit-code abfaaf4bda787a99075e693c5c6fe908d5eacd23 -- src Cargo.toml Cargo.lock tests/v16_cu.rs tests/invariants/coverage_reopenings.tsv tests/invariants/invariant_status.tsv
git show --format= --check HEAD
```
