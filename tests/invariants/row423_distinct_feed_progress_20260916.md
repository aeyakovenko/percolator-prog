# Row 423: Distinct-Feed Resource and Progress Coverage

One new LiteSVM product passes with **14 active Hybrid legs, 28 value-bearing
source records, 42 distinct feed identities/accounts, and a 64-slot backlog**.
Its full observation transaction uses a publicly constructed address lookup
table and fits in 456 bytes. All 36 successful required continuations complete
below their CU guards, paying both owners and closing both portfolios.

No new current production bug was found. This is positive conformance for a
specific additional product, not row closure. Authoritative statuses are unchanged:
**row 423 OPEN, INV-028 REFUTED_CURRENT, INV-077 OPEN_EVIDENCE**. No TSV is edited.

## Provenance and Scope

- Source clone: `/tmp/percolator-astra-invariant-cycle-20260915-run`.
- Fetched origin branch: `codex/astra-invariant-cycle-20260915`.
- Base: `e4dc038f84845da1542521cd1913ce1b7e4abaa7`.
- Isolated clone: `/dev/shm/row423-inv028-inv077-20260916`.
- Local branch: `codex/row423-resource-progress-20260916`; no push.
- Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Reused default-feature SBF: `/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so`.
- SBF SHA-256: `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
- Private host target: `/dev/shm/row423-resource-progress-20260916-target`, copied
  from `/dev/shm/row421-succession-expiry-20260916-target` and cleaned with
  `cargo clean -p percolator-prog` before compilation. No SBF rebuild is claimed.

The `/home/anatoly/percolator-prog` workspace is untouched. Only four files under
`tests/invariants` change: this report, `README.md`,
`cu/inv_077_hybrid_source_backlog.rs`, and the new focused child
`cu/inv_077_distinct_feed_progress.rs`.

## Audit and Overlap

| Existing owner | Coverage already present | Boundary retained here |
| --- | --- | --- |
| Lane 7 `inv_077_hybrid_source_backlog.rs` | 14 legs, 28 sources, 64-slot backlog, 42 references to three shared feeds, complete cooperative owner exit | Economic assertions and schedule are shared, not copied as a separate discovery |
| Lane 16 extension of the same owner | 5,782 configured slots, 16 hints, 48 references to three shared feeds, exact exits | Already covers all sixteen hints; another shared-feed sixteen-hint probe would duplicate it |
| INV-053 `v16_program_max_shape_hybrid_refresh_requires_each_current_report` | 14 distinct single-feed assets, omitted/replayed report rejection and full refresh | Does not compose three distinct feeds per asset, 28 sources, two-chunk backlog and complete exit |
| INV-028 `inv_028_latent_reset_exit.rs` | 16 route/sign/lifecycle worlds, 27 claims plus a latent domain, peer reduction, nonunit ADL/reset, prior-epoch cleanup and exact payouts | One active asset in the measured suffix; no distinct composite-feed maximum |
| INV-028 `inv_028_exit_resource_reservation.rs` | 16 historical liens plus two future domains, owner-only reduction, provider reservation/payout and exact rollback | 18/28 domains; its earlier 26-history admission probes exhausted CU before success |
| INV-028 `inv_028_recovery_latent_capacity.rs` | Four sign/full-or-split worlds, 27 claims plus a latent source, Recovery force-close, 28-source terminal payout | One active asset; not a large-active-shape Hybrid Recovery product |
| INV-077 `v16_program_recovery_kf_refresh_at_14_leg_28_source_shape_is_bounded` | AuthMark full-active/source committed-state Recovery K/F refresh | Does not establish the distinct-feed Live schedule or a combined Hybrid Recovery exit |

The two older Hybrid liquidation/Recovery construction failures remain documented
in [Lane 7](lane7_combined_shape_progress_20260915.md). They were audited in source
and in that report, not rerun or repaired in this task. The INV-028 owners were
audited without edits; their historical run results are not new verification here.

The added boundary is externally distinct account loading at maximum active/source
shape, including a transaction that requires address compression and a late
feed-binding rejection. Compared with the shared-feed Lane 7 continuation, the
successful full call costs 7,294 additional CU. This is not another sixteen-hint
or maximum-market claim. Forty-eight distinct feeds, sixteen hints together with
distinct feeds, full market occupancy together with distinct feeds, liens, latent
capacity, Recovery, liquidation, fees/funding and unilateral exits remain outside
this product.

## Public Construction and Assertions

The new variant uses the existing INV-018 public System/SPL/ATA market constructor.
System instructions allocate portfolios; public `InitPortfolio`, ATA creation,
SPL `MintTo` and wrapper deposits fund both owners with 2,000,000 atoms each.
Withdrawals return to those same public ATAs. Supply is exactly 4,000,000 before
trading, unchanged at the end, and equal to final owner balances. No program
economic state, source records, positions, certificates, token balances, or lookup
table bytes are injected. Lamport airdrops, authenticated clock control and valid
external Pyth report fixtures are the existing environmental model; this is not
a test of Pyth publisher execution.

There are 42 distinct feed IDs and 168 distinct report accounts across four dated
report sets. Reports are prepared as external inputs before use. The payer creates
one lookup table at slot zero, then extends it in nine packet-valid transactions
of at most twenty addresses. The test decodes the actual table, checks its owner,
rent exemption, complete address list and extension slot, and uses it only after
the slot advances. Each complete v0 crank loads 42 readonly addresses from that
table and has one payer signature. Its serialized size is 456 bytes; the same
instruction in a legacy transaction is asserted to exceed 1,232 bytes. Every
submitted crank, including the negative case, fits the packet ceiling.

Public Hybrid configuration precedes all exposure. The shared Lane 7 economic
history observes 100 -> 101 -> 100 while closing/reversing fourteen 1,000-lot
positions. The LP retains all 28 occupied, value-bearing, unliened source records
and 28,000 atoms of PnL. Both portfolios have fourteen active legs. At slot 67,
reports target 95 after a 64-slot interval. The schedule is
`[0], [0,1], ..., [12,13], [0..13]`.

Each of fifteen permissionless calls strictly lowers the pending-slot sum:
896 -> 864, thirteen 64-slot decreases, then 32 -> 0. The first fourteen calls
preserve both portfolios byte-for-byte. Source occupancy, positive claims,
positions, OI, counterparty state and all report/table/custody frames are checked.
Two account refreshes then change state and establish complete current certificates.

Before the last catch-up call, a rotated hint order processes unfinished asset 13
first and presents a valid report belonging to asset 13 in asset 12's final feed
position. That separate dated report retains 42 distinct loaded addresses; its
feed identity is rejected before freshness is considered. The result is exactly
`InstructionError(2, Custom(29))`, `InvalidOracleKey`, at 226,934 CU. All 174
snapshotted Accounts compare equal, including the market's pending accrual,
both portfolios, mint, vault, 168 reports and the table. The payer changes only
by the one-signature fee. The next valid call consumes the unchanged final
32-slot backlog. Failed-call CU is not counted as successful progress.

Fourteen matched reductions each remove a leg from both accounts while retaining
the source claims. Conversion empties all 28 source records. Input-derived gain
is 28,000 + 14 * 5 * 1,000 = 98,000 atoms, giving exact payouts of 1,902,000 and
2,098,000. Both portfolios close, their lamports return to the market, and market
capital, insurance, engine/SPL vault balances and portfolio count become zero.
All feed accounts and the lookup table retain their complete Account frames.

| Successful required continuation | Calls | Maximum observed CU |
| --- | ---: | ---: |
| Backlog catch-up | 15 | 1,230,272 |
| Complete-report account refresh | 2 | 1,048,104 |
| Matched owner reduction | 14 | 911,346 |
| Full-source conversion | 1 | 712,020 |
| SPL withdrawal to original ATA | 2 | 38,930 |
| Portfolio closure | 2 | 26,516 |

Catch-up/refresh/reduction/conversion guards are 1,375,000 CU; withdrawal/closure
guards are 300,000. The transaction budget is 1,400,000. Construction, including
lookup-table operations, is outside the 36-call successful continuation count.
CU equality is not an assertion. Cooperative owners sign reduction/conversion/
withdrawal/close; catch-up and recertification require only the payer.

## Commands and Results

From the isolated clone, these are the environment and exact test selectors used
(tool invocations supplied the same environment inline with `env`):

```sh
export CARGO_TARGET_DIR=/dev/shm/row423-resource-progress-20260916-target
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
cargo test --locked --offline --test v16_cu inv_077_bounded_work_and_maximum_shape_compute::hybrid_source_backlog::distinct_feed_progress::v16_program_42_distinct_feeds_max_source_backlog_has_bounded_public_exit -- --exact --nocapture
cargo test --locked --offline --test v16_cu inv_077_bounded_work_and_maximum_shape_compute::hybrid_source_backlog::v16_program_max_source_hybrid_backlog_has_bounded_public_exit -- --exact --nocapture
cargo test --locked --offline --test v16_cu inv_077_bounded_work_and_maximum_shape_compute::hybrid_source_backlog::v16_program_max_market_16_hint_hybrid_source_backlog_has_bounded_public_exit -- --exact --nocapture
cargo test --locked --offline --test v16_cu inv_053_full_health_recertification_equivalence::v16_program_max_shape_hybrid_refresh_requires_each_current_report -- --exact --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_ -- --test-threads=3 --nocapture --skip v16_program_fixed_blockers_remain_progressing --skip v16_public_terminal_classifier_exhausts_normalized_outcome_space --skip v16_public_trace_schema_detects_out_of_band_economic_mutation --skip v16_public_trace_terminal_classifier_requires_complete_economic_evidence
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_077_hybrid_source_backlog.rs tests/invariants/cu/inv_077_distinct_feed_progress.rs
git diff --check
git show --format= --check HEAD
git diff --exit-code e4dc038f84845da1542521cd1913ce1b7e4abaa7 HEAD -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/**/*.tsv' ':(glob)tests/invariants/*.tsv'
git ls-files --others --exclude-standard src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/**/*.tsv' ':(glob)tests/invariants/*.tsv'
```

The four exact CU selectors each pass. Lane 7 retains its 1,222,978-CU maximum;
Lane 16 retains 1,241,192; the INV-053 full-report refresh uses 901,162. All thirteen
INV-079 metadata guards pass. Formatting, whitespace/commit checks and protected
path checks pass; protected paths have no diff or untracked additions.

An initial unrestricted INV-079 command was also run:

```sh
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_ -- --test-threads=3 --nocapture
```

It passed the same thirteen metadata guards and failed four runtime tests before
execution because the authenticated matcher artifact is absent at the harness's
hardcoded `tests/fixtures/auth_matcher/target/deploy/auth_matcher.so` path. The
explicit metadata-only command above excludes exactly those four runtime tests.
The protected fixture tree was not populated or edited. This is not a claim that
all seventeen tests or the full suite pass. Build output also contains existing
dead-code and Solana future-compatibility warnings.

The first exploratory negative fixture reused a current report and accidentally
reduced loaded-address cardinality to 41; its own assertion failed before the
negative transaction. The retained fixture uses a distinct dated account and
checks all 42 addresses. This development failure was not a product bug.

Logs are `/dev/shm/row423-final-build.log`,
`/dev/shm/row423-final-distinct-public.log`, `/dev/shm/row423-final-lane7.log`,
`/dev/shm/row423-final-lane16.log`, `/dev/shm/row423-final-inv053.log`,
`/dev/shm/row423-final-metadata.log`, and `/dev/shm/row423-final-metadata-only.log`.
