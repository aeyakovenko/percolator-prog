# Lane 15: competing Hybrid reward recipients and CPI exits

## Scope and verdict

Base: `origin/codex/astra-invariant-cycle-20260915` at `89cd5088`.
Branch: `codex/lane15-hybrid-reward-competing-recipients-20260915`.
Independent clone: `/tmp/percolator-lane15-20260915`. The coordinator checkout
was only read to obtain its origin URL; all edits and builds used this clone
and private `/dev/shm/lane15-20260915-{host,sbf}` targets.
Pinned engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

The charter is `scripts/loop.md` and the ownership/reachability rules in
`tests/invariants/README.md`. Primary owner: INV-045, in
[cu/inv_045_hybrid_recipient_provenance.rs](cu/inv_045_hybrid_recipient_provenance.rs).
Adjacent assertions concern INV-020/024/036/041/061 and the matcher authorization
boundary. Only that Rust owner, this report and the README change.

**No current implementation LoF, DoS or CU violation was found. Row 422 remains
OPEN/missing and INV-045 remains `REFUTED_CURRENT`.** This is finite public-route
conformance evidence. Production, dependencies and machine dispositions are
unchanged; no red/green production fix or invariant closure is claimed.

## Non-overlap

| Existing evidence | Existing boundary | Lane 15 increment |
| --- | --- | --- |
| Lane 12, same owner | One reward recipient; single/batch no-CPI reductions; same-slot target replacements | Either recipient-trade participant wins; another claimant follows; CPI/non-CPI route switches |
| Paid-origin Hybrid recipient owner | Paid-discovery penalty retention, fresh-report handoff and recipient catchup | Post-award recipient identity competition and unsigned-maker CPI reductions |
| Interleaved cap/carry owner | Four paid trade routes before later liquidation | Transport changes on the already credited recipient pair, then exact reward withdrawal |
| Corroborated-mark fee owner | Fresh liquidation fee versus previously paid fees; common ownership | Independent recipient owners, alternative successful/reverted claims and matcher context rollback |

Lane 15 uses no target replacements. It preserves both Lane 12 selectors as
controls and reuses their public construction, independent stock/certificate
censuses, source-price arithmetic and recipient loss oracle.

## Exact economic invariant

For closed source quantity `q`, committed source price `p`, position scale `S`,
and five-basis-point liquidation fee:

```text
P = ceil(ceil(q * p / S) * 5 / 10_000)
R = floor(P * 3_333 / 10_000)
source value after liquidation = source endowment - 240_000 - P
source insurance = P - R
source domain budgets = [floor((P-R)/2), ceil((P-R)/2)]
```

All other insurance domains and provider earnings remain zero. The fee oracle
explicitly distinguishes `p` from the source raw target and every candidate
recipient price. The two-asset Hybrid trajectories are computed from input
prices and elapsed slots, not inferred from output balances.

The recipient pair starts with two matched lots and reduces one lot at each of
two successive accepted prices. Its input-derived loss is `L = 14_400` atoms.
The losing taker (actor 4) and winning maker (actor 2) have entitlements:

```text
taker value = taker endowment - L + R * [recipient == taker]
maker value = maker endowment + L + R * [recipient == maker]
```

The selected recipient withdraws exactly `R` SPL atoms after both reductions.
The taker subsequently withdraws its remaining `endowment - L`, ending with
zero capital and PnL. The maker retains exactly `endowment + L`, excluding the
already withdrawn reward. Token destinations, total vault stock, source value
and insurance reconcile. Rival claims, transport changes and failed prefixes
cannot redirect or duplicate `R`, or exchange the source penalty for the
recipient's own trading PnL.

## Public histories

The new selector is
`v16_program_competing_hybrid_recipients_preserve_rewards_across_cpi_route_switches`.
Its 128 worlds cross both source asset IDs, both price directions, publication
before/with liquidation, selected-recipient refresh before/after reward,
both hint/single-batch orders, CPI before/after non-CPI, and either reward owner.

- The other claimant's valid crank prefix executes before a stale-report suffix
  fails. Every tracked and transaction Account rolls back, including account
  absence and exact signer fees. The winner then commits the liquidation.
- The rival's immediate attempt at the completed liquidation rejects with
  `EngineNonProgress`. After the first recipient reduction, a rival crank
  succeeds by renewing a stale target certificate. Only that certificate may
  change in the target portfolio; both recipient Accounts and all five owners'
  economic values remain exact. A subsequent claim rejects as non-progress.
- Every world uses one CPI and one non-CPI reduction, spanning all four public
  transports. The maker authorizes the system-created matcher context publicly;
  a non-CPI-first history renews its authorization before the CPI fill.
  The maker does not sign the CPI transaction. Logs prove actual matcher
  invocation/success. Single CPI writes its context return record; batch CPI
  uses Solana return data and preserves the context, as does non-CPI.
- Each reduction also executes before a failing stale-report suffix. The same
  instruction bytes then commit. Rollback includes the matcher context, both
  portfolios, market, custody, program Accounts and wallets.
- An actual SPL reward transfer also completes before a rejected suffix and
  rolls back. The identical withdrawal then commits to the selected owner.
  Both complete oracle profiles survive recipient trades; source price/OI,
  recipient PnL, insurance attribution and custody remain independently checked.

Construction uses public System/SPL/wrapper instructions. Only external Pyth
reports, Clock, signer SOL and program loading are environmental inputs. There
are no program-owned byte writes, private engine transitions or restored state
snapshots. The matcher is built from this branch and its context is created
and initialized through public instructions. Every measured transaction is
signature-verified, fits the 1,232-byte limit and stays below 900,000 CU.

During development, an expectation that the rival's post-reduction crank would
reject was corrected: the current implementation legitimately refreshes the
target certificate. The final assertion requires that successful refresh,
unchanged economic attribution, and subsequent non-progress. This was a test
expectation issue, not a discovered implementation defect. A further fixture
assertion was scoped to single CPI after confirming that the batch matcher
returns quotes via Solana return data without writing its context.

## Artifacts and commands

Both SBF artifacts were built from the isolated branch with platform-tools
v1.52 and default features, without reusing another checkout's binaries:

- Wrapper SHA-256: `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
- Auth matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

```sh
env CARGO_TARGET_DIR=/dev/shm/lane15-20260915-sbf CARGO_BUILD_JOBS=4 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/lane15-20260915-sbf/deploy -- --locked
env CARGO_TARGET_DIR=/dev/shm/lane15-20260915-sbf CARGO_BUILD_JOBS=4 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --manifest-path tests/fixtures/auth_matcher/Cargo.toml --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked --offline
sha256sum /dev/shm/lane15-20260915-sbf/deploy/percolator_prog.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
```

Each Cargo command below uses the following environment (set explicitly with
`env` in the executed commands):

```sh
export CARGO_TARGET_DIR=/dev/shm/lane15-20260915-host
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane15-20260915-sbf/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
cargo test --locked --offline --test v16_cu hybrid_recipient_provenance:: -- --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::retained_penalty_handoff::paid_origin_hybrid_recipient::v16_program_paid_origin_penalty_survives_dual_hybrid_recipient_catchup \
  inv_045_no_free_mark_movement::trade_origin_catchup::corroborated_mark_fees::v16_program_corroborated_paid_mark_only_distributes_new_liquidation_fees \
  inv_045_no_free_mark_movement::interleaved_cap_carry::v16_program_interleaved_trade_routes_preserve_oracle_cap_carry_and_reward_provenance \
  inv_045_no_free_mark_movement::v16_program_mark_writer_and_trade_exit_composition_is_source_complete \
  inv_079_public_reachability_evidence::v16_every_public_trace_consumer_validates_reachability_evidence \
  inv_079_public_reachability_evidence::v16_finding_blind_violation_oracle_evidence_roster_is_source_complete \
  inv_079_public_reachability_evidence::v16_retained_retry_terminal_dispositions_are_source_complete \
  inv_079_public_reachability_evidence::v16_program_open_lof_manifest_snapshot_is_structurally_honest \
  inv_079_public_reachability_evidence::v16_superseded_control_terminal_dispositions_are_source_complete \
  inv_079_public_reachability_evidence::v16_dated_open_security_finding_benchmark_is_non_overclaiming \
  inv_079_public_reachability_evidence::v16_public_instruction_coverage_registry_matches_production_roster \
  inv_079_public_reachability_evidence::v16_special_verification_method_registry_matches_charter \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant \
  inv_079_public_reachability_evidence::v16_program_invariant_harnesses_are_test_free_roots
rustfmt --edition 2021 --check tests/invariants/cu/inv_045_hybrid_recipient_provenance.rs
git diff --check
git diff --exit-code HEAD -- src Cargo.toml Cargo.lock tests/invariants/open_findings.tsv tests/invariants/invariant_status.tsv
```

## Verification results

| Selection | Result |
| --- | --- |
| New competing-recipient selector | PASS: 128 worlds, 128 rewarded liquidations, 960 exact rollbacks |
| CPI execution within new selector | PASS: 128 commits and 128 failed-prefix executions; 64 single and 64 batch fills, maker unsigned |
| Rival post-reduction refresh | PASS: 128 successful certificate refreshes with no second reward |
| Reward SPL payout within new selector | PASS: 128 exact payouts and 128 completed-transfer rollbacks |
| Original dual-Hybrid selector | PASS: 32 worlds, 112 exact rollbacks |
| Lane 12 target replacement selector | PASS: 32 worlds, 240 exact rollbacks |
| Complete touched-owner selection | 3/3 PASS, 192 worlds, 1,312 exact rollbacks, 114.35 seconds |
| Paid-origin, corroborated-fee and interleaved-cap controls plus reward source guard | 4/4 PASS, 10.57 seconds |
| Listed INV-079 reachability/metadata guards | 13/13 PASS, 0.72 seconds |
| Scoped rustfmt, whitespace and production/status-file framing | PASS |

Peak measured CU across the new and existing owner selectors is **370,185**,
below the 900,000 assertion. Each new world has two committed reductions and
two failed reduction prefixes. The 64 single-CPI worlds require a changed
matcher context on commit and exact context restoration on rollback; the 64
batch-CPI worlds require an unchanged context and actual matcher invocation.

Final logs are `/dev/shm/lane15-hybrid-final.log` and
`/dev/shm/lane15-controls-guards.log`. The host build emits existing dead-code
and Solana future-compatibility warnings. No unfiltered suite was run.

## Remaining boundary

Row 422 remains open. This product covers two assets, five portfolios, one
rewarded liquidation, four slots, zero funding/maintenance/trade fees, integral
lots and classic SPL custody. It does not establish maximum-shape behavior,
native custody, arbitrary report histories, policy changes, multiple rewarded
liquidation episodes, unrestricted paid-origin histories or whole-cohort terminal
redemption. The source and its counterparty remain exposed; the recipient maker
retains its non-reward capital/PnL. Existing paid-origin and terminal owners keep
their separate scope. A passing fixed matrix is not an independent rediscovery
of the missing row-422 finding or a universal economic proof.
