# Row 422: CPI switching, retained penalties and clipped reward provenance

Date: 2026-09-16. Local branch: `codex/row422-cpi-reward-provenance-20260916`.
Worktree: `/dev/shm/row422-cpi-reward-provenance-20260916`, created from
`origin/codex/astra-invariant-cycle-20260915` at
`b4091247021656d6877e2454ed04e4b0f8cbfefa` in
`/tmp/percolator-astra-invariant-cycle-20260915-run`.
Protected-file comparison uses `origin/main` at
`d809e9a563d9b8bf38f32648b32a15d75f526ec8` as well as the branch base.
No publication or changes to `/home/anatoly/percolator-prog`.

**No current-behavior violation was found in this bounded product. Row 422 stays
OPEN/missing and INV-045 stays `REFUTED_CURRENT`.** Neither TSV is changed.

## Coverage and non-overlap

Two selectors each run sixteen ordered pairs of single/batch CPI/no-CPI routes.
The first route pays for discovery; the second reduces positions after two
liquidations and actual catchup. Eight pairs per selector switch CPI/no-CPI;
the remaining eight provide controls within each transport family. One selector
includes the fresh liquidation recipient and the other omits it. All construction
uses System/SPL/wrapper/matcher instructions. Signer SOL, Clock and external Pyth
reports are harness inputs; there are no protocol-account writes or restores.

Lane 23 clips self-maintenance around fresh Hybrid rewards without route switching
or retained stale penalties. Lane 26 varies fresh reward destinations without
maintenance, route switching or matcher rotation. The older paid-origin route
product varies discovery transport without composing a second trade transport
with clipped maintenance. This increment joins those boundaries with an actual
public matcher-context succession and a later effective-quantity reduction.

## Public trace and assertions

1. At slot 1, configure canonical AuthMark asset 0 and Hybrid asset 1 at 1,000,000.
   Fund five distinct owners through SPL and disable minting. Open the target's
   100-lot position; fund the flat keeper with 101 atoms. Maintenance is 160 atoms
   per slot and both reward shares are 3333 bps. Initialize an authorized matcher
   through System creation and matcher instructions, with a 1000-bps bid spread.
2. At slot 5, a stale observation advances time. Discover a downward mark through
   the selected first route. The raw 900,000 print is accepted at 990,400 and
   stages 992,320 while the effective price remains 1,000,000. Independently
   calculated discovery fees are 1,540,072 atoms, all outside source budgets.
3. At slot 6, self-maintenance charges 101, rebates 33 and forgives 699. The stale
   report permits liquidation at effective price 997,600, charging 5,994 with no
   keeper reward or domain budget. Rotate to a second publicly initialized matcher
   context. Trying to close the original one-lot quantity now oversteps the
   post-ADL short position and rejects exactly with `EngineLockActive` on every
   second route, preserving both matcher contexts and every economic account.
4. At slot 7, maintenance charges the remaining 33, rebates 10 and forgives 127.
   Fresh evidence targets 980,000, but the accepted effective price is 995,206.
   The second liquidation charges 8,287; the eligible share is 2,762. Receipt is
   conditional on this call's recipient, and the entire unreceived amount stays
   in the source domains. Discovery and the old penalty remain unbudgeted.
5. At slot 14, complete catchup to 980,000 and settle every exposed portfolio.
   Reduce both discovery positions by half the smaller independently computed
   effective post-ADL quantity through the selected second route. No new fee or
   mark movement occurs; effective reductions agree within one quantity atom.
   The superseded matcher context remains unchanged. Logs distinguish actual CPI
   invocation from no-CPI execution; single matcher responses persist in context,
   whereas batch responses use return data.
6. Retry maintenance and target cranks with/without the reward recipient. Fee
   retries are exact no-ops; target cranks reject as `EngineNonProgress`. Pay the
   keeper's complete capital into its SPL account, with mint supply unchanged.
   Vault plus payout equals the fixed public endowment. Other owners receive no
   SPL payout in this bounded live-state test.

Liquidation sizes are observed from deployed OI: 12,015,859 and 16,652,956 quantity
atoms. Penalty is independently `ceil(ceil(q * effective_price / POS_SCALE) *
5 / 10000)`; reward is `floor(penalty * 3333 / 10000)` only for fresh provenance
with a recipient. Each penalty is distinguished from entry, raw, discovery and
old-target prices. This is not an independent liquidation-sizing proof.

| Fresh recipient | Charged maintenance | Rebates | Forgiven | Receipt | Skipped | Source domains 2/3 | Keeper SPL |
| --- | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| Omitted | 144 | 46 | 1936 | 0 | 2762 | 4143 / 4144 | 3 |
| Present | 1254 | 416 | 826 | 2762 | 0 | 2762 / 2763 | 2025 |

The four solvent portfolios pay 8,320 maintenance atoms altogether. Canonical
domains 0/1 finish at 4208/4210 (omitted) or 4578/4580 (present). Keeper payout is
`101 + receipts + rebates - charged`. The 2,022 payout difference exactly matches
the difference in retained insurance. All sixteen routes in each schedule agree
on the entire engine group and five engine portfolios, with only market, account
and owner identities normalized. No balances, source fields, legs, fees,
certificates or accounting cursors are normalized away. Independent stock,
reservation, source-credit and current-certificate censuses run throughout.

There are 304 exact rollback checks per selector: 176 successful crank prefixes
followed by invalid suffixes, 96 nonprogress retries, 16 oversized post-ADL trade
attempts and 16 successful payout prefixes followed by invalid destinations.
The crank suffixes include an uninitialized recipient, a same-time conflicting
report (`OracleInvalid`), and the superseded matcher tuple with the current
sequence (`Unauthorized`). They include the actual stale and fresh liquidation
prefixes. The shared helper independently simulates successful prefixes and pins
the failing instruction index/error. All tracked and compiled complete Accounts,
including absence, restore except the separate payer's exact signature fee.

## Artifacts and commands

The host cache was copied with `cp -a` to a private target directory; it is not a
shared writable cache. Reused, unchanged SBF artifacts:

| Artifact | SHA-256 |
| --- | --- |
| `/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so` | `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f` |
| `/dev/shm/lane24-20260916-matcher/deploy/auth_matcher.so` | `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93` |

The new child accepts `PERCOLATOR_ROW422_MATCHER_SBF`, defaulting to the existing
matcher helper path. Thus even ignored files under `tests/fixtures` are untouched.
Logs are in `/dev/shm/row422-cpi-reward-provenance-20260916-logs`.

Commands below run from the isolated worktree. The exports express the same
environment supplied with `env` on each invocation. No unfiltered suite is run.

```bash
export CARGO_TARGET_DIR=/dev/shm/row422-cpi-reward-provenance-20260916-target
export TMPDIR=/dev/shm/row422-cpi-reward-provenance-20260916-tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so
export PERCOLATOR_ROW422_MATCHER_SBF=/dev/shm/lane24-20260916-matcher/deploy/auth_matcher.so
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::cpi_reward_provenance::v16_program_cpi_switch_retained_penalty_and_clipped_reward_normalize \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::cpi_reward_provenance::v16_program_cpi_switch_omitted_rewards_cannot_be_reclaimed \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::reward_maintenance_catchup::clipped_reward_maintenance::v16_program_clipped_self_maintenance_keeps_hybrid_rewards_and_shared_owner_sources_exact \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::reward_destination_retry::v16_program_hybrid_reward_destination_retries_preserve_only_committed_receipts \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::retained_penalty_handoff::v16_program_retained_stale_penalty_survives_fresh_liquidation_and_catchup \
  inv_045_no_free_mark_movement::v16_program_mark_writer_and_trade_exit_composition_is_source_complete
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_program_open_lof_manifest_snapshot_is_structurally_honest \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_program_invariant_harnesses_are_test_free_roots
rustfmt --edition 2021 --config skip_children=true --check \
  tests/invariants/cu/inv_045_authenticated_reward_handoff.rs \
  tests/invariants/cu/inv_045_cpi_reward_provenance.rs
git diff --check
git diff --cached --check
git diff --exit-code origin/main -- src Cargo.toml Cargo.lock tests/fixtures \
  ':(glob)tests/invariants/**/*.tsv' ':(glob)tests/invariants/*.tsv'
git diff --exit-code b4091247021656d6877e2454ed04e4b0f8cbfefa -- src Cargo.toml Cargo.lock tests/fixtures \
  ':(glob)tests/invariants/**/*.tsv' ':(glob)tests/invariants/*.tsv'
git show --format= --check HEAD
```

## Results and limits

New matrix: 2/2 selectors, 32 histories, 64 liquidations, 608 exact rollbacks and
32 payouts. Final-run new-selector CU peaks:

| Recipient | Trade | Crank/maintenance | Rejected bundle | SPL payout |
| --- | ---: | ---: | ---: | ---: |
| Omitted | 239922 | 279734 | 308082 | 73009 |
| Present | 245922 | 322341 | 350689 | 62509 |

Guards are 345,000 for trades, 325,000 for individual cranks/maintenance,
650,000 for two-instruction rejected bundles and 300,000 for payouts.
Final `v16_cu` invocation: **6/6 PASS**, including both new selectors and four
adjacent controls (42.00s; `final-cu.log`). INV-079: **4/4 PASS** (0.01s;
`guards.log`). Scoped rustfmt, Git whitespace checks and protected diffs against
both `origin/main` and the branch base pass. The final commit also passes
`git show --format= --check HEAD`. Host compilation retains existing dead-code
and Solana future-compatibility warnings. No full-suite claim is made.

Development failures corrected test assumptions: empty clipped portfolios retire
without a self-rebate; an original one-lot reversal can overclose after ADL, even
after mark catchup; and batch matcher responses do not write context. These did
not violate the requested accounting/route property. No production changes or
assertion weakening of that property were made.

This finite product uses one Hybrid source plus canonical AuthMark asset 0,
five distinct owners, fixed policies, zero funding, classic SPL custody, two
downward liquidation episodes and three observation slots. Economic portfolios
remain live, with residual positions and claims; only the keeper withdraws.
It does not prove arbitrary histories, full terminal redemption, maximum shape,
multi-source/provider interactions, native custody, unrelated policy succession
or all matcher implementations. It does not close row 422 or change any verdict.
