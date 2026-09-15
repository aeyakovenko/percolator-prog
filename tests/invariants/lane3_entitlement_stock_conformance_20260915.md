# Lane 3: Entitlement and Accounting Stocks

## Scope and isolation

- Base: `d809e9a563d9b8bf38f32648b32a15d75f526ec8`.
- Branch: `codex/lane3-entitlement-stock-20260915`.
- Worktree: `/home/anatoly/percolator-lane3-entitlement-20260915`.
- Engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Fresh default-feature SBF SHA-256:
  `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.

The supplied checkout was on another branch with unresolved changes. Only its
Git objects were read to obtain the requested base and create this worktree.
The four requested workflow/status files were read first from the specified
commit. The base's tests, documentation, and pinned engine informed this work;
no withheld fix or other branch's tests were copied. A private copy of an
existing build cache was used, with fresh builds of this wrapper and its matcher
fixture. Benchmark rows are comparison labels, not a claim of rediscovery.

## Changes and overlap

1. Added [the insurance regression](cu/inv_039_mixed_role_funding_insurance.rs).
   It composes a pending creditor loss with an unsettled funding-bearing debt
   on the same portfolio, domain-local insurance, and resolved settlement.
2. Extended [the funding fixture](cu/inv_039_mixed_role_funding_resolution.rs)
   to support a later debtor mark and per-prefix checks. Funding indices are
   now checked against signed inputs, not merely required to be nonzero.
3. Corrected [the terminal scan regression](cu/inv_070_terminal_scan_recredit.rs)
   to bind each continuation to the authority epoch after its preceding debit.
4. Added this report and the README coverage note. Production, dependencies,
   benchmark evidence, and authoritative invariant statuses are unchanged.

The old funding fixture publishes both asset marks before closing the creditor
leg. That trade also settles the cross-asset debtor leg. Position retention alone
therefore did not establish an unsettled debt at resolution. The added variant
publishes the second mark after the first close, then verifies its debtor K/F
snapshots are zero, its creditor has zero basis and nonzero loss weight, and its
original capital is intact. This is the main new coverage distinction.

Existing uninsured mixed-role tests already calculate source-face discounts.
Existing insured pending-loss tests use separate creditor/debtor portfolios.
Existing reserve-role tests cover pending claimants, expiry, and recredit.
Existing source-capacity and terminal scan tests already cover admission rollback,
latent materialization, and scan/withdraw ordering. They were used as controls
instead of adding duplicate regressions.

The scan control failed on the unchanged base with `EngineStale` (19) where it
expected `EngineLockActive` (21): its insurance payment consumed an authority
epoch, so the next reused `CloseSlab` instruction was stale. The correction
constructs the expected next epoch for both in-transaction suffixes and later
committed payments. The stale variant remains an explicit rollback assertion;
the current-epoch variant reaches the intended unpaid-entitlement guard. Each
stock checkpoint also checks the exact epoch, including its restoration after
rollback. This is test maintenance, not a production security correction.

## Independent oracle

There are twelve worlds: two side orientations, three insurance placements
(loss side, opposite side on the same asset, or another asset), and two close
orders. All economic construction uses System/SPL/ATA and public wrapper
instructions. Clock advancement is the only harness input after construction;
no economic state injection or private engine transition drives a history.

Deposits are `[600000, 179985, 300000, 250000, 20001]`. Both positions have four
lots. Each six-slot capped mark path moves 60,000 per lot. Signed funding floors
to six atoms per lot over that path, so each gain/debt is `G = D = 239976`.
An independent donor withdraws and contributes 10,000 atoms to one insurance
domain. Only the loss domain may spend that contribution.

From inputs: source support `S = 179985`; gross bankruptcy `G-S = 59991`;
insurance consumed `I` is either 10,000 or zero; remaining B loss is `R=59991-I`.
The cross-asset K/F debt consumes support S and retires the whole creditor face
G. Insurance pays the bankruptcy residual and does not become fresh backing.
For owner 0, every checked prefix must satisfy:

```text
capital + signed PnL + unpaid receipt face + prior SPL payout
  = 600000 + G - (K/F settled ? D + G - S : 0) - (B charged ? R : 0)
```

The peer's entitlement becomes `300000+D` only when its gain settles. The
bankrupt owner retains `-59991` until booking. Unrelated owners retain their
principal, less the donor's explicit contribution. Receipt due is checked as
face minus cumulative payment, separately from capital and prior SPL payout.
Every source entry must belong to its expected owner and asset side.

| Insurance placement | Per-owner terminal SPL payouts | Remaining vault | Unspent insurance |
| --- | --- | --- | --- |
| Loss domain | `[490018, 0, 539976, 250000, 10001]` | 59,991 | 0 |
| Opposite side / other asset | `[480018, 0, 539976, 250000, 10001]` | 69,991 | 10,000 |

The 59,991-atom remainder is checked separately from insurance; total custody
alone is insufficient. Each waiting rejection restores the economic frame;
successful continuations change it within bounded work. All five portfolios
are deleted and paid receipt retries preserve the frame. Reserve withdrawal,
late source expiry, and slab retirement are outside this new history.

Development assertions caught two oracle/fixture assumptions: the targets are
committed before the first funding slot, and the original matched close already
settles cross-asset debt. Those failures were not production counterexamples.
No minimal production correction was required.

## Coverage verdicts

| Row | Verdict at the requested base | Limits |
| --- | --- | --- |
| 419 | OPEN; added twelve pending-loss/funding/insurance histories with per-prefix owner checks | No arbitrary cohort, ADL, Recovery, fee, CPI, or maximum-shape composition |
| 423 | OPEN; reran the existing 24-world used-generation admission and exact-exit matrix | Its 28-domain reservation mechanism is covered; this adds no new resource class or capacity implementation proof |
| 424 | OPEN; repaired stale-epoch expectations and passed sixteen scan/expiry histories with both withdrawal orders | Covers debit-epoch consumption composed with backing-expiry prefix restart; not every environmental writer |
| 435 | OPEN; added actually unsettled mixed-role funding debt with loss-side, opposite-side, and foreign-asset insurance | Finite five-owner topology and two close orders; no general entitlement/equivalence theorem |

INV-024/025/026/031/034/035/037/039/041 receive bounded owner, domain, stock,
reservation, loss-partition, or ordering assertions in the new history.
INV-027 has the unrelated-principal frame; INV-029 has peer-receipt accounting;
INV-038 has exact signed funding rounding. INV-028/030/032/033 are exercised by
the existing capacity and reserve/expiry controls. INV-036 has no new fee
composition. INV-086 receives a finite input-derived model comparison, not
unbounded transition equivalence. None of INV-024 through INV-039, INV-041, or
INV-086 is promoted to proven or globally closed.

## Commands

All commands below run from the isolated worktree. Environment values used:

```sh
export CARGO_TARGET_DIR=/dev/shm/percolator-lane3-20260915-target
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=$CARGO_TARGET_DIR/deploy/percolator_prog.so
env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked
cargo test --locked --offline --test v16_cu -- --nocapture --test-threads=1 \
  v16_program_mixed_funding_debt_charges_only_its_insurance_domain \
  v16_program_mixed_roles_preserve_funding_attribution_through_resolution \
  v16_program_used_generation_admission_reserves_latent_capacity_through_exact_exit \
  v16_program_terminal_scan_rediscovers_earlier_insurance_after_later_expiry \
  v16_program_pending_claimant_reserve_roles_preserve_attribution_through_close_and_recredit \
  v16_program_mixed_creditor_debtor_roles_preserve_owner_entitlement_through_resolution
cargo test --locked --offline --test v16_cu v16_program_terminal_scan_rediscovers_earlier_insurance_after_later_expiry -- --nocapture --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions -- --test-threads=1 \
  v16_invariant_charter_and_index_are_complete \
  v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  v16_post_pr135_counterexamples_reopen_every_affected_invariant
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_039_mixed_role_funding_resolution.rs tests/invariants/cu/inv_039_mixed_role_funding_insurance.rs tests/invariants/cu/inv_070_terminal_scan_recredit.rs
cargo fmt --all -- --check
git diff --check
```

The focused formatting check passes. Repository-wide `cargo fmt --all -- --check`
reports pre-existing formatting differences in six unchanged INV-024/070/073
files. A trial whole-repository format was undone for those files only; the
patch retains no unrelated formatting changes.

## Results

The initial six-selector run passed five tests and exposed the stale-epoch scan
assertion. After the test correction, the scan selector passed on the same SBF.
The five already-passing histories were not repeated for that independent edit.

| Selector family | Worlds | Result / measured peak |
| --- | --- | --- |
| New mixed funding and insurance | 12 | PASS; 160,449 CU |
| Existing mixed funding orders | 4 | PASS; 204,545 CU |
| Existing uninsured mixed roles | 32 | PASS; 24 waiting rollbacks, 32 receipt retries; 209,217 CU |
| Existing used-generation capacity | 24 | PASS; 972 checked transactions, 192 exact rollbacks, 696 terminal calls; 1,152,523 CU |
| Existing pending claimant/reserve recredit | 24 | PASS; 168 exact rollbacks, 24 slab closes; 480,365 CU |
| Corrected terminal scan recredit | 16 | PASS; 88 commits, 120 exact rollbacks, eight rediscoveries; 221,388 CU |

All three invariant-index/status/reopening guards pass. `git diff --check` and
the focused Rust formatting checks pass. These are selected public-route checks,
not a full repository test-suite run. The benchmark classifications and all
authoritative statuses remain unchanged.
