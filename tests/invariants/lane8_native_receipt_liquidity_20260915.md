# Lane 8: Native Receipt Liquidity Across Late Expiry

## Scope and isolation

- Base: `origin/codex/astra-invariant-cycle-20260915`, fetched at
  `e1394241e46ba1d66e21b43f6c1c99d83761dcca`.
- Independent clone: `/tmp/percolator-lane8-20260915`.
- Branch: `codex/lane8-arbitrary-history-20260915`.
- Engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Read `scripts/loop.md`, invariant README/status/open findings, normalized
  reopenings, and all four Lane 1-4 reports dated 2026-09-15 before selecting
  the coverage gap. No withheld patches or other branches' tests were consulted.
- The supplied dirty checkout was read only. Clone, builds, edits and commit
  use the separate workspace. Private copies of Lane 4 build caches seeded
  fresh compilation from this checkout.
- Fresh default-feature wrapper SBF SHA-256:
  `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.

## Coverage and non-duplication

The selected gap is row 417's retained receipt identity across stock changes,
composed with native secondary custody and shared liquidity. The existing
selector `v16_program_late_expiry_claimant_orders_share_secondary_liquidity_without_losing_receipts`
is extended; no independent copy of its public fixture or payout oracle is added.

| Existing coverage | Missing intersection added here |
| --- | --- |
| INV-067 shared secondary liquidity: sixteen classic-SPL histories | Native secondary custody, raw lamports, synchronization and owner unwrap in the same late-expiry receipt history |
| Lane 2 overdue-source generator: claimant sizes, shared owners, two expiries, split/grouped requests | Its single classic rail does not exercise native lamports or `SyncNative`; that generator is rerun unchanged as a control |
| INV-070 native terminal PnL sync/close: two owners, one pending gain and a donation | No two retained unequal receipts competing for insufficient secondary liquidity while expiry changes the payout rate |
| INV-068 receipt replay/secondary rail witnesses | Existing individual-claim/replay coverage does not establish the two-claimant native expiry-and-sync product |
| Lane 1 retained policy, Lane 3 unsettled funding/insurance, Lane 4 observation/maximum shape | These are different products and are not duplicated or counted as covered by this increment |

The matrix retains all sixteen classic worlds: exact/late expiry (slots 13/14),
both claimant orders, both assignments of `CloseResolved` and
`ClaimResolvedPayoutTopup`, and replenishment versus mixed-rail exit. Sixteen
native worlds use separate synchronization. Eight native replenishment worlds
bundle synchronization with the same retained payout bytes. The no-replenishment
worlds are not repeated for a synchronization mode they never exercise.
Total: **40 worlds, 24 newly added**.

The shared fixture now accepts primary-mint decimals so native secondary custody
can satisfy the wrapper's equal-decimals contract. Existing callers retain zero
decimals; native worlds use nine. This does not rescale the atom-denominated
deposits, prices, trades or independent entitlement formula.

## Public trace and oracle

1. Public trades and deposits create claim faces 700/1,300, initially paid
   116/217. Late backing normalization releases 350 atoms; per-owner floor
   entitlements rise by 82/151. Total new due is exactly 233.
2. A 232-atom secondary vault can pay either owner, but the batch containing
   expiry and both payouts fails at instruction 4 after one SPL payout. Both
   receipts, expiry state, mints, vaults, recipients and economic signers match
   their complete pre-transaction account images.
3. In native replenishment cases, a public System transfer adds one raw lamport.
   Retrying the unchanged requests still fails and preserves the unsynchronized
   account image. Raw lamports are not booked token liquidity or engine claims.
4. `SyncNative`, expiry and both payouts execute before an intentionally invalid
   System suffix fails at instruction 6. Logs require three successful wrapper
   calls and three successful SPL calls (sync and two transfers); the complete
   tracked frame returns to its unsynchronized state.
5. Separate or bundled synchronization then admits the unchanged payout requests.
   Non-replenishment worlds reverse claimant priority and use the primary rail
   for the other claimant. Both modes retain exact receipt identity and payout
   floors; already-paid cross-rail retries cannot extract additional value.
6. All five portfolios settle and close with exact rent transfer. Native
   destinations close through SPL to their actual owners, whose SOL balances
   rise by exactly token amount plus native rent reserve. There are 120 native
   custody closes across the 24 new worlds.

The oracle uses fixed public input amounts and rational floors, not a second
engine transition. It checks each owner's combined payout, separate mint/custody
conservation, native `lamports = rent + token amount` at synchronized checkpoints,
unchanged receipt fields other than cumulative payment, terminal source/claim
stocks and the two-atom rounding residue. Native mint supply remains zero;
the explicitly donated extra lamport is separately counted. Classic/native
and split/grouped runs compare complete final payout ledgers, owner payouts
and both vault amounts, not just aggregate conservation.

Economic state is constructed by System/SPL/ATA/public wrapper instructions.
The only new `set_account` installs the immutable external SPL native-mint
genesis account omitted by LiteSVM, following the existing native fixture.
No market, portfolio, receipt or custody state is injected. The inherited
rollback frame excludes only the separate network-fee payer and runtime
accounts; it includes all tracked economic signers and custody rent.

## Commands and results

All commands run in the isolated clone. The final selected checks are below;
logs are `/tmp/lane8-20260915-{sbf,final-cu,metadata}.log`.

```sh
env CARGO_TARGET_DIR=/dev/shm/lane8-20260915-sbf CARGO_BUILD_JOBS=4 \
  RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --sbf-out-dir target/deploy -- --locked
sha256sum target/deploy/percolator_prog.so

export CARGO_TARGET_DIR=/dev/shm/lane8-20260915-host
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/tmp/percolator-lane8-20260915/target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu -- --nocapture --test-threads=2 \
  v16_program_late_expiry_claimant_orders_share_secondary_liquidity_without_losing_receipts \
  v16_program_generated_overdue_source_histories_preserve_receipt_identity_and_attribution \
  v16_program_retained_claim_identity_survives_late_expiry_recipient_rotation_and_atomic_retry \
  v16_program_coowned_receipts_preserve_attribution_across_conversion_and_late_expiry \
  v16_program_fractional_source_conversion_preserves_receipts_through_same_bucket_expiry \
  v16_program_late_fee_reclassification_preserves_receipt_faces_and_claimant_order \
  v16_program_native_pnl_terminal_sync_and_close_retry_preserves_unsynced_donation

cargo test --locked --offline --test v16_program_fuzz_regressions -- --nocapture --test-threads=2 \
  v16_invariant_charter_and_index_are_complete \
  v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  v16_invariant_audit_summary_matches_every_verdict_row \
  v16_post_pr135_counterexamples_reopen_every_affected_invariant

rustfmt --edition 2021 --config skip_children=true --check \
  tests/invariants/cu/inv_067_receipt_rail_liquidity.rs \
  tests/invariants/cu/inv_067_terminal_claim_late_expiry.rs
git diff --check
git diff --exit-code e1394241 -- src Cargo.toml Cargo.lock \
  tests/invariants/open_findings.tsv tests/invariants/invariant_status.tsv \
  tests/invariants/coverage_reopenings.tsv
```

| Check | Result |
| --- | --- |
| Fresh wrapper SBF build | Passed, offline and locked; hash above |
| Seven selected public-route tests | **7/7 passed**, 109.12 seconds |
| Extended shared-liquidity selector | **40 worlds**; 40 initial underfunded rollbacks, 16 unsynchronized-native rollbacks, 16 sync/expiry/two-payout suffix rollbacks, 24 replenishment exits, 16 mixed-rail exits, 200 portfolio deletions and 120 native custody closes |
| Measured matrix peak CU | Classic 275,899; native separate sync 273,925; native bundled sync 285,925; all below the existing 500,000 bound |
| Lane 2 overdue-source control | 66 worlds, 1,461 commits, 181 rollbacks, 396 portfolio closes and 66 slab closes; peak 544,178 CU within its separate 900,000 bound |
| Other receipt controls | Retained recipient identity: four worlds; coowned conversion: twelve; fractional source conversion: four; late fee reclassification: twenty-four; all passed |
| Native terminal PnL control | Passed, exact owner payouts 1,010/1,290 and 37-atom synchronized surplus; peak 308,898 CU |
| Metadata/index/status guards | **4/4 passed**, 0.01 seconds |
| Edited-file rustfmt and whitespace | Passed |
| Production/dependency/status/reopening diff against base | Empty |

All eleven selected checks passed. Existing unused-support and Solana
future-compatibility warnings remain. The supplied checkout's branch and dirty
status were unchanged at the final read-only check.

Development: an initial leaf filter combined with `--exact` selected zero tests
and is not credited. The fully qualified selector then passed the intermediate
48-world matrix in 54.73 seconds. The final matrix removes eight redundant
no-sync worlds and adds native unwrapping; its selected run above supersedes
that intermediate result.

## Verdicts and limits

| Row | Disposition |
| --- | --- |
| 417 | **OPEN**, improved bounded public-route evidence for native secondary liquidity, late expiry, unequal retained receipts and synchronization ordering. Benchmark evidence remains `missing`. |
| 411/416/419/420/421/433/435 | **OPEN**, unchanged. This increment adds no row-specific policy, funded-role, funding-debt or reserve-beneficiary coverage. |

INV-024/063/066/067/068/070 receive the composed custody, receipt identity,
payout and expiry assertions; INV-018/080 have the native custody and rollback
boundary. No invariant aggregate, benchmark label or normalized reopening is
promoted. No production correction or qualifying security finding is claimed.

This remains a finite five-portfolio, two-asset, single-expiry topology with
fixed faces and source stocks, honest AuthMark inputs, zero funding/fees, and
classic/native SPL custody. It does not establish arbitrary histories, mixed
funding debts, repeated expiries, ADL, maximum shape, reserve beneficiary exit
or complete market retirement. Economic payouts need only the payer; portfolio
deletion and native unwrapping retain existing owner authority. The full suite,
repository-wide formatting and Kani proofs are outside this focused run.

Changed files: `cu/inv_067_receipt_rail_liquidity.rs`,
`cu/inv_067_terminal_claim_late_expiry.rs`, `README.md`, and this report.
