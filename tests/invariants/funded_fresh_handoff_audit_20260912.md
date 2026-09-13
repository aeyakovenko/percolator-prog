# Nonzero funding through fresh-report handoff, 2026-09-12

Base: local `origin/codex/astra-open-holdout-ledger-20260912` at
`8fb23b4d194b0953c4a01a95c4b14c431e787978`.
Branch: `codex/oracle-admission-boundary-20260912-c84e`.
Worktree: `/dev/shm/percolator-oracle-admission-boundary-20260912-c84e`.
Only the requested base's local files and its locally cached locked engine source
informed this increment. No network fetch, remote PR/issue/diff, other worktree's
tests or copied build artifacts were used. The main checkout and
`/tmp/percolator-astra-watch.Cb2E7d` were not edited.

## Selection and Overlap

The selected dimension is row 422's nonzero funding combined with fresh-report
reward provenance handoff. The existing authenticated-handoff and reward-catchup
selectors explicitly disable funding. The trade-origin catchup selector also
disables funding and ends with stale reports. INV-052's downward-funding cadence
selector has no paid-discovery/fresh-report liquidation handoff; its stateful
net-funding selector compares target histories and backing rewards. INV-045's
pending-trade-mark replacement selector owns replacement and owner exit without
this keeper-reward transition.

INV-027's standalone first-admission selector documents sufficient headroom;
INV-020's chunked/staged observation tests and INV-053's full-health equivalence
tests own other admission and refresh relations. They are not duplicated here.

The existing [`cu/inv_045_authenticated_reward_handoff.rs`](cu/inv_045_authenticated_reward_handoff.rs)
fixture is extracted into a shared runner. Its original selector retains all
sixteen zero-funding worlds. One new exact selector adds only two worlds:

`inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::v16_program_nonzero_funding_fresh_handoff_preserves_owner_and_keeper_entitlement`

## Executable Relation

Both worlds use the same paid print, report and 3,333-bps keeper share, varying
only whether the fresh report is published separately before target refresh.
Public System/SPL/ATA/wrapper instructions create all economic accounts. The
harness supplies signer SOL, Clock and external Pyth report fixtures; it never
injects protocol state or restores a snapshot. Real provider publication and
signature verification remain assumptions of the external-report fixture.

At slot 5, the paid print leaves the accepted settlement price at 1,000,000 and
establishes the 992,320 funding mark, with both funding indices still zero. At
slot 6, a fresh report hands off provenance and the accepted price becomes
997,600. The negative premium reaches the configured 1,000-e9 rate cap. The
independent arithmetic is `floor(-1,000 * 997,600 / 1,000,000,000) = -1` atom per
lot. The test checks the exact signed funding indices and the resulting 100/1
atom transfers from the two shorts to the two longs. Each long's net mark loss
is therefore 2,399 atoms per lot.

The observed closed quantity is 11,999,399 position quanta. Independent nested
ceilings on that quantity and the accepted price produce the 5,986-atom fee;
flooring its 3,333-bps share produces the 1,995-atom keeper reward. Alternative
raw/accepted trade prints, the entry price and report target produce different
fees. Closed quantity is an observed input to this fee oracle, not an independent
liquidation-sizing proof.

After bounded public settlement, exact capital and PnL lanes are:

| Account | Capital | PnL | Capital + PnL |
| --- | ---: | ---: | ---: |
| Target long owner | 4,854,114 | 0 | 4,854,114 |
| Passive short owner | 100,000,000 | 239,900 | 100,239,900 |
| Paid-print long owner | 9,227,565 | 0 | 9,227,565 |
| Paid-print short owner | 9,229,964 | 2,399 | 9,232,363 |
| Keeper | 2,995 | 0 | 2,995 |

Each paid-print trader contributed 770,036 fee atoms. Insurance retains exactly
1,544,063 atoms. The keeper withdraws all 2,995 atoms through the public SPL
route; the other owner token accounts remain at zero. Booked and SPL vault
balances are both 125,098,005 atoms afterward, and all owner value plus insurance
reconciles to custody. Backing-provider earnings are explicitly zero throughout:
this fixture has no funded backing provider. Pyth provider Accounts and signer
Accounts, including their lamports, remain byte-for-byte unchanged.

Each world includes two valid fresh-crank prefixes followed by equivocal evidence
that rejects `OracleInvalid` at instruction 3. The prefix is independently
simulated successfully; one of these rollbacks restores a reward-bearing
liquidation, and the combined-publication world also rolls back funding accrual.
Missing advertised evidence, stale evidence and equivocal evidence reject at
instruction 2. A repeated successful request after restored health rejects
`EngineNonProgress`. Every rejection restores all tracked and compiled Accounts
except the exact payer signature fee; a fresh transaction envelope provides the
successful retry. These are twelve exact rejected transactions across two worlds.

Independent stock, source-credit and encumbrance censuses accompany current
certificate recomputation. Unrelated portfolios and SPL custody remain exact
across liquidation. This is bounded INV-020/024/045/053/054/060/061/071/080/081
conformance evidence. Row 422 remains **OPEN**; rows 413, 425 and 426 and all
machine-readable invariant/holdout dispositions are unchanged. Positive funding,
multiple funding slots, provider changes, arbitrary liquidation sizing, CPI,
multi-asset selection and complete exposed-owner exits remain outside this test.

## Validation

The new exact selector passes both worlds, four late rollbacks, two reward-bearing
rollbacks and two keeper payouts, with a peak transaction cost of 349,292 CU.
Existing limits remain 325,000 per crank, 650,000 per two-crank bundle and 300,000
per withdrawal. All four adjacent exact controls pass, including the unchanged
sixteen-world zero-funding matrix at 346,435 peak transaction CU. The three
charter/index, audit-summary and authoritative-status checks pass. Repository
formatting, working-tree/staged whitespace checks and the production/Cargo/status
no-diff check pass. Only the test, this audit and its README entry are staged.

The wrapper SBF was freshly built offline in this worktree, with platform-tools
v1.52 and engine `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`. Host compilation uses
the same private worktree target. Wrapper SHA-256:
`5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
Production and Cargo inputs are unchanged. Development corrected a test accessor
to read the per-asset funding checkpoint; no production conformance failure was
observed. Existing unused-support warnings and the `solana-client v1.18.26`
future-compatibility warning remain.

```sh
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF="$PWD/target/deploy/percolator_prog.so"
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir target/deploy -- --locked
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::v16_program_nonzero_funding_fresh_handoff_preserves_owner_and_keeper_entitlement \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::v16_program_paid_discovery_fresh_handoff_authenticates_liquidation_and_keeper_exit \
  inv_045_no_free_mark_movement::trade_origin_catchup::v16_program_trade_origin_liquidation_prices_and_entitlements_survive_catchup_order \
  inv_045_no_free_mark_movement::accepted_price_reward::reward_catchup_order::v16_program_reward_price_tracks_actual_catchup_across_report_and_crank_orders \
  inv_052_split_merge_invariance::v16_program_downward_funding_is_crank_partition_invariant
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git diff --exit-code 8fb23b4d194b0953c4a01a95c4b14c431e787978 -- src Cargo.toml Cargo.lock tests/invariants/invariant_status.tsv tests/invariants/coverage_reopenings.tsv
git status --short --branch
```
