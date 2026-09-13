# Pending Obligations With Premium Funding At Resolution

Worktree: `/home/anatoly/percolator-prog-row419-20260912`.
Branch: `codex/astra-pending-obligation-row419-20260912`.
Base: `f70a5d4e56dbce3b96c3d6cfdb67ad5db3fda944`.

The main checkout's starting HEAD, `39f08dba`, did not contain `tests/invariants/`.
The worker's empty branch was rebased onto the locally available public coverage
baseline above. Main-checkout files and other workers' files were not modified.
Only local public repository files and pinned dependency source were read;
there was no GitHub PR/issue/branch inspection, network fetch or sealed-test access.

## Executing Evidence

```text
inv_039_pending_loss_obligation_durability::resolved_histories::funded_resolution::v16_program_funded_pending_debt_survives_resolution_and_delayed_close_orders
```

The [new CU sibling](cu/inv_039_pending_loss_funded_resolution.rs) reuses the
existing public `AttributionWorld` constructor and owner-value/obligation census.
System, SPL, ATA and wrapper instructions create all economic accounts. LiteSVM
provides program binaries, signer SOL and Clock; no economic account bytes are
installed or patched. Mint authority is revoked before accrual.

One- and two-lot positions open in separate domains at price 1,000,000. Signed
AuthMark publication at slot 1 sets a target of 900,000 or 1,100,000. With a
100-bps cap anchored to the entry price and a one-slot accrual window, two public
cranks reach 980,000 or 1,020,000. The first step activates the funding mark;
the second accrues signed premium funding at the 1,000-e9 cap. Signed floor
arithmetic gives one funding atom per lot, opposing the holder's price gain.
The original debtor obligations are therefore 19,999 and 39,998 atoms in both
directions. These values are computed from test inputs, without engine arithmetic
helpers or observed account values as the entitlement oracle.

Each holder books its credit, then shuts down and forfeits its exposure while
retaining a zero-basis, nonzero-loss-weight obligation. Neither debtor Account
changes. Resolution at slot 2 preserves every asset and non-market Account.
Sixteen histories cross two directions, both debtor orders, both claimant orders,
and settlement at the permissionless boundary or 31 slots later.

Both holders detach without payout while the debtors remain unbooked. A composed
first-debtor payout followed by the other holder's waiting retry rejects after
the debtor's successful wrapper and SPL prefixes. Every compiled or tracked
Account rolls back, including presence, data, token state and rent; only the
separate payer's exact signature fee changes. The same debtor payment then commits,
and its portfolio is deleted before the second debt settles. Another waiting
retry preserves the original unpaid claim. Further elapsed time cannot change
the resolved funding indices or owner entitlements.

After the second debtor settles, either claimant order receives these exact SPL
amounts, with no fees, insurance, social loss or mint-supply change:

| Owner | SPL Atoms |
| --- | ---: |
| First holder | 219,999 |
| First debtor | 160,001 |
| Second holder | 339,998 |
| Second debtor | 210,002 |
| Bystander | 777 |

Paid retries at slot 100 or 131 reject with exact rollback. Every portfolio is
deleted with its precise rent transferred to the market. All histories finish
with zero materialized portfolios, vault tokens, capital and positive PnL.
Totals: 96 exact rollbacks, 80 owner payouts and 80 portfolio deletions.
Peak measured terminal transaction compute, including rejected transactions, is
180,307 CU against a 400,000-CU assertion. Setup CU is not included in that peak.

## Overlap And Limits

| Existing Coverage | Additional Dimension Here |
| --- | --- |
| INV-039 `resolved_histories` matrices and bounded history generator | Those disable funding; this carries nonzero premium funding into two pending resolved cohorts. |
| INV-039 `terminal_fees` | Maintenance fees are distinct from the signed F indices and opposing premium-funding debt tested here. |
| INV-039 `shared_holder`, `backing_expiry`, `pending_destination_recovery` | Shared holders, backing normalization and custody repair remain in their existing selectors. None is reimplemented. |
| INV-039 public SBF funding/shutdown/forfeit route coverage | The added histories retain two zero-basis obligations across resolution and delete the first debtor while the second debt is still unbooked. |

This is bounded INV-039/024/041/067/073/081 evidence, with frozen-index and
unaffected-Account checks relevant to INV-086. Related INV-037/048/066/076 gain
no new close-partition, bankruptcy, source-reservation or drift oracle. Nonzero
social loss, fractional lots, shared holders, backing expiry, maintenance fees,
receipt reclassification, CPI routes, absent deletion signers and slab retirement
are outside this increment. The existing generic history selector is not extended.
**Row 419 remains OPEN.** No invariant verdict, production code or dependency pin
changes; no generic generator/oracle or vulnerable-pin red/green result is claimed.

## Discarded Attempts

- The main-checkout HEAD lacked the invariant tree; no tests or notes were added
  on that base before selecting the local public coverage baseline.
- The first compile used nonexistent `b_long`/`b_short` field names. The assertion
  was corrected to the existing `b_long_num`/`b_short_num` fields.
- A 100-bps cap with the inherited 20-slot window was rejected by `InitMarket`
  with `InvalidAccountData`, before any economic history. The retained test uses
  a valid one-slot window for its two explicit one-slot accruals.
- The first price oracle incorrectly compounded the cap and expected 980,100.
  The canonical unchanged-target cap is anchored, as specified by the existing
  public cap/cadence coverage and wrapper path. The retained input oracle uses
  980,000/1,020,000. The funding-debt and payout assertions were not weakened.

No valid-history invariant violation was observed. No exploratory selectors,
economic-state injection, production edits or discarded probes are committed.

## Validation

The default-feature SBF was rebuilt locked/offline in this worktree using
platform-tools v1.52 and engine `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
The program SHA-256 is
`d5f2d3d2c35842aab0979ab24fed415fe36b2b93ed6cc80998fee839ae76343f`.
The new exact selector passes all sixteen histories. All four exact invariant
index/status/summary/root checks and repository-wide formatting pass. Working and
staged diff whitespace checks pass. Production sources, `Cargo.toml`, `Cargo.lock`
and the CU root match the base. No full-suite claim is made.

All Cargo commands use this environment, with commands run from the worktree:

```sh
export CARGO_TARGET_DIR=/home/anatoly/percolator-prog-row419-20260912/target
export TMPDIR=/home/anatoly/percolator-prog-row419-20260912/target
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /home/anatoly/percolator-prog-row419-20260912/target/deploy -- --locked
cargo test --locked --offline --test v16_cu inv_039_pending_loss_obligation_durability::resolved_histories::funded_resolution::v16_program_funded_pending_debt_survives_resolution_and_delayed_close_orders -- --exact --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_program_invariant_harnesses_are_test_free_roots
rustfmt --edition 2021 tests/invariants/cu/inv_039_pending_loss_funded_resolution.rs tests/invariants/cu/inv_039_pending_loss_resolved_histories.rs
cargo fmt --all -- --check
git diff --check
git diff --cached --check
sha256sum target/deploy/percolator_prog.so
```

The initial target cache was copied into this worktree from the local public
verification cache using
`cp -a --reflink=auto /dev/shm/percolator-watch-verify-target target`.
The cached SBF was replaced by the fresh build above. Shared cache and main
checkout files were not edited. Existing unused-support warnings and the Solana
1.18 client future-compatibility notice remain.
