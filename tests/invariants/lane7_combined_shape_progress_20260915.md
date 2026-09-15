# Lane 7 Combined-Shape Progress

## Verdict

**A valid public-route combined witness exists. Row 423 remains OPEN and INV-077
remains OPEN_EVIDENCE.** The new witness combines fourteen active Hybrid legs,
twenty-eight value-bearing source records, a 64-slot accrual backlog and a final
forty-two-reference observation call, then finishes exact owner exits. This narrows
the coverage frontier; it does not close the original liquidation/Recovery cases
or the unrestricted feed/source/backlog/market-occupancy product. No new security
finding or production correction is claimed.

## Provenance

- Base: `origin/codex/astra-invariant-cycle-20260915`, locally resolved to
  `e1394241e46ba1d66e21b43f6c1c99d83761dcca`.
- Isolated clone: `/home/anatoly/percolator-lane7-20260915`.
- Branch: `codex/lane7-inv077-combined-progress-20260915`.
- Engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Default Anchor-v2 features, platform-tools v1.52, locked offline dependencies.
- Fresh wrapper SBF SHA-256:
  `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
- Fresh auth matcher SHA-256:
  `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

The user's dirty checkout was read only. The clone uses shared Git objects but
has its own index, branch refs and files. Build targets and logs are private under
`/dev/shm/lane7-*`; deploy artifacts belong to the isolated clone. Production,
engine pin, lockfiles and machine verdict tables are unchanged.

## Original Failure

Both named Lane 4 selectors reproduce `EngineNonProgress` (`Custom(22)`) on the
untouched base at 38,056 CU:

```text
v16_attack_public_14_leg_28_source_42_feed_refresh_stays_bounded
v16_attack_public_recovery_kf_progress_survives_stale_42_feed_tail_at_max_shape
```

Their shared `setup_max_source_live_pair_with_hybrid_oracles` delegates to the
sequential per-asset source builder in `tests/v16_cu.rs`. A temporary read-only
diagnostic in `drive_both_current` located the rejection at slot 4, on the taker:
asset 0 still has `slot_last = 3` and `last_good_oracle_slot = 3`, while asset 1's
new report is being processed. The builder supplies no observations to the
whole-account refresh. The wrapper's
`reject_incomplete_account_health_observations_view` requires each active Hybrid
leg's current-slot report while the soft-stale fallback has not matured.

Thus the intended fourteen-leg/twenty-eight-source measurement is never reached.
The failing call is not compute exhaustion, and it does not establish absence of
every bounded continuation. The diagnostic was removed; neither original test is
ignored, renamed to imply success, or converted to an expected-panic test. Their
historical 1.201M-CU liquidation and 1.147M-CU Recovery claims are explicitly
qualified in the owner module and README.

A construction alternative that reconfigured an already source-bearing AuthMark
market was also rejected with `EngineLockActive`. The public reconfiguration gate
forbids existing exposure/positive PnL, so the retained test configures Hybrid
before trading. No private engine call or economic-state injection is used.

## New Witness

Owner: [cu/inv_077_hybrid_source_backlog.rs](cu/inv_077_hybrid_source_backlog.rs).
Selector: `v16_program_max_source_hybrid_backlog_has_bounded_public_exit`.

The public history opens fourteen LP long positions of 1,000 lots, observes
100 to 101, closes them, opens fourteen LP shorts, then observes 101 to 100.
Every price change supplies complete current reports and refreshes both accounts
to current certificates. This creates exactly 28,000 atoms of LP PnL in twenty-eight
occupied, value-bearing, unliened domains. The LP has 2,000,000 capital atoms;
its counterparty has 1,972,000. All fourteen legs remain active at slot 3.

At slot 67, current composite reports price every asset at 95. The schedule is
`[0], [0,1], [1,2], ..., [12,13], [0,1,...,13]`. Each of its fifteen successful
permissionless cranks reduces the pending-slot sum: 896 initially, decreases of
32/64/.../64/32, then zero. The first fourteen calls leave both portfolios
byte-identical. The last call combines the outstanding 32-step asset path,
all fourteen hints, forty-two feed references and the LP's twenty-eight sources.
Every prefix retains all legs, source occupancy and positive claim records,
unchanged effective OI, counterparty bytes and SPL/feed account frames.

Two complete-report account refreshes commit state changes and current
certificates. Fourteen matched reductions each remove a leg on both accounts and
retain source claims; conversion then releases all 98,000 LP PnL atoms and empties
the source table. Withdrawals pay exactly 1,902,000 and 2,098,000 atoms. Both
portfolios close; capital, insurance, portfolio count and engine/SPL vaults are
zero, with unchanged mint and feed accounts. These are **36 successful required
continuation calls**, excluding construction. The price/lot history independently
determines entitlements: 28,000 historical gain plus 70,000 backlog gain, with
4,000,000 total atoms conserved.

This adds source saturation to the previously separate Hybrid backlog witness.
It is not another AuthMark conversion or feed-only refresh control. Parameters
and assertions bind the current supported 14-leg/28-domain and 32-step path limits.
The market has fourteen configured slots; max accrual dt is 64, price cap is 100
bps per slot, Hybrid soft-stale threshold is 64, and fees/funding are zero. There
are three distinct feed accounts per report set, not forty-two distinct feeds.
The initial one-/two-hint calls do not claim to execute all fourteen maximum paths
in one transaction. Owners cooperate on bilateral reductions, conversion,
withdrawal and close; only catch-up/refresh are permissionless.

## Successful Required CU

These maxima are from successful required continuations only. Failed setup calls
and setup trades/configuration are excluded. Every new crank/reduction/conversion
asserts at most 1,375,000 CU; withdrawals/closes use the stricter 300,000 ceiling.
The transaction ceiling is 1,400,000 CU. Exact CU equality is not a test condition.

| Required continuation | Calls | Maximum observed CU |
| --- | ---: | ---: |
| Combined catch-up | 15 | 1,222,978 |
| Complete-report account settlement/recertification | 2 | 1,040,810 |
| Matched owner reduction | 14 | 911,346 |
| Full-source conversion | 1 | 712,020 |
| Exact SPL withdrawal | 2 | 52,430 |
| Portfolio close | 2 | 26,516 |

## Commands and Validation

Isolation and builds, from the isolated clone except the initial two commands:

```sh
git clone --shared --no-checkout /home/anatoly/percolator-prog /home/anatoly/percolator-lane7-20260915
git -C /home/anatoly/percolator-lane7-20260915 switch --create codex/lane7-inv077-combined-progress-20260915 e1394241e46ba1d66e21b43f6c1c99d83761dcca
env CARGO_TARGET_DIR=/dev/shm/lane7-20260915-sbf CARGO_BUILD_JOBS=4 cargo build-sbf --tools-version v1.52 --sbf-out-dir /home/anatoly/percolator-lane7-20260915/target/deploy -- --locked --offline
env CARGO_TARGET_DIR=/dev/shm/lane7-20260915-sbf CARGO_BUILD_JOBS=4 cargo build-sbf --tools-version v1.52 --manifest-path tests/fixtures/auth_matcher/Cargo.toml --sbf-out-dir /home/anatoly/percolator-lane7-20260915/tests/fixtures/auth_matcher/target/deploy -- --locked --offline
cp -a /dev/shm/lane4-d809-host /dev/shm/lane7-20260915-host
env CARGO_TARGET_DIR=/dev/shm/lane7-20260915-host cargo clean -p percolator-prog
export CARGO_TARGET_DIR=/dev/shm/lane7-20260915-host CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/home/anatoly/percolator-lane7-20260915/target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
```

The dependency target copy accelerates compilation; `cargo clean -p` forced a
fresh wrapper/harness build at the untouched base before edits. The two failing
selectors were executed against that baseline with `RUST_BACKTRACE=1`, then
reconfirmed against the changed head. Baseline log: `/dev/shm/lane7-baseline-hybrid.log`.
The changed-head failure log is `/dev/shm/lane7-final-frontier.log`.

```sh
cargo test --locked --offline --test v16_cu -- v16_attack_public_14_leg_28_source_42_feed_refresh_stays_bounded v16_attack_public_recovery_kf_progress_survives_stale_42_feed_tail_at_max_shape --test-threads=1 --nocapture
cargo test --locked --offline --test v16_cu -- v16_program_max_source_hybrid_backlog_has_bounded_public_exit v16_bpf_public_full_14_leg_three_feed_max_backlog_has_bounded_refresh_schedule v16_program_max_source_conversion_and_owner_exit_are_bounded v16_program_max_source_capacity_reclamation_restores_funded_exit v16_program_max_shape_hybrid_refresh_requires_each_current_report v16_program_recovery_kf_refresh_at_14_leg_28_source_shape_is_bounded --test-threads=3 --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_ -- --test-threads=3 --nocapture
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_077_hybrid_source_backlog.rs tests/invariants/cu/inv_077_bounded_work_and_maximum_shape_compute.rs
git diff --check
```

The six CU selectors pass (`/dev/shm/lane7-final-cu.log`). They include the new
combined witness plus the original independent backlog, fourteen-distinct-feed
health, AuthMark conversion, capacity-reclamation and Recovery K/F controls.
All **17** INV-079 metadata/public-route gates pass, recorded in
`/dev/shm/lane7-final-metadata.log`; formatting and `git diff --check` pass.
The two original selectors still fail on the changed head. This is focused
verification, not a claim that the full suite is green.

## Changed Files and Frontier

- `cu/inv_077_hybrid_source_backlog.rs`: new public construction and bounded exit.
- `cu/inv_077_bounded_work_and_maximum_shape_compute.rs`: mount the owner and
  correct current-evidence claims for the two failing Hybrid selectors.
- `README.md`: index the witness and qualify route-to-shape evidence.
- This audit: provenance, exact failure, measured success and remaining frontier.

Maximum market occupancy (5,782 slots), sixteen hints, distinct-feed maxima,
simultaneous liens, source-capacity competition/reuse, other mark/fee/funding
histories, unilateral exits, liquidation, Recovery and resolved receipts are not
composed by this test. The original two selectors remain failing coverage
frontiers. The new successful Live schedule cannot be substituted for either
original terminal/liquidation scenario. Row 423's independent-discovery benchmark
label and authoritative OPEN disposition are unchanged.
