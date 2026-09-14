# PR135 Scope Q: prefix custody and current insurance actionability

## Scope and isolation

Worktree: `/tmp/percolator-pr135-scope-q-20260914`.
Branch: `codex/pr135-scope-q-20260914`.
Base: `e527392517542b10a64d0ef303f346b0898509ae`, the local
`origin/codex/astra-open-holdout-ledger-20260912` at worktree creation.
All source edits and validation run in this fresh worktree. Neither
`/home/anatoly/percolator-prog` nor `/tmp/percolator-astra-watch.Cb2E7d` was edited.
The source repository was used only to create the requested linked worktree.

One new public conformance selector is mounted under INV-070:

`inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_prefix_custody_actionability::v16_program_cached_prefix_custody_surplus_cannot_capitalize_spent_insurance`.

No production change was necessary for this bounded product. Row 424 remains OPEN;
its eight data fields, invariant status and findings ledgers are unchanged.

## Prior coverage and distinct increment

The requested `inv_070_terminal_prefix_reuse`, `terminal_scan_recredit` and row 424
notes were read before implementation. Related public coverage was also checked:

| Existing evidence | Boundary retained by its owner |
| --- | --- |
| `inv_070_terminal_prefix_reuse.rs` | Valid Live reuse controls and exact rejection behind a terminal prefix. |
| `inv_070_terminal_scan_recredit.rs` | Scanner rediscovery of earlier insurance after later expiry; scanner/withdrawal order comparison. |
| `inv_071_terminal_prefix_recredit.rs` | Asset-local withdrawal recredit after later expiry, without external custody. |
| INV-070 external-surplus-after-prefix selector | Exact classification of unsolicited custody without historical spent insurance. |
| `inv_071_terminal_reserve_backfill.rs` | Donation plus rejected reserve admission; no spent insurance to recredit. |
| Generated source/cursor-time controls | Deadline boundaries, current source stocks and bounded rescans. |

The new product crosses the existing spent-insurance prefix with an independent
public SPL custody writer. It distinguishes a raw custody increase from an actual
change in locally recoverable insurance. It does not add a scanner rediscovery
test or a separate probe for every existing rollback boundary. Only one selector
is retained; no marginal standalone probes were added.

## Public history and oracle

Eight independently constructed worlds cross both source sides, later backing of
61/307 atoms, and no transfer versus a transfer of 89 atoms. The existing public
recredit fixture constructs two assets, three portfolios with capital 1000/100/137,
a 200-atom gain and 100-atom insurance spend. User payouts are 1200/0/137, followed
by public portfolio deletion. No market, portfolio, mint or token bytes are injected.
LiteSVM supplies signer funding, authenticated Clock and blockhash progression.

At slot 43, CloseSlab scans past the earlier spent-insurance asset and persists
cursor 1, blocked by later Fresh backing expiring at slot 44. The first insurance
withdrawal instruction is retained unchanged throughout the history. It cannot
pay while all booked residual is still provider backing.

In the donated worlds, the first paid user publicly transfers 89 SPL atoms into
the vault. The complete market Account stays identical, including the prefix,
booked vault, insurance spend and payout ledger. The retained withdrawal still
rejects even though the physical vault has enough tokens. A bundled transfer and
rejected withdrawal proves exact rollback after an executed SPL prefix; the same
transfer then commits independently.

Authenticated Clock advances to slot 44 without changing the market Account.
CloseSlab normalization releases exactly the booked backing and resets the prefix.
The current earlier entitlement is input-derived:

`recovered = min(100 receivable, 100 spent, backing) = 61 or 100`.

The 89 external atoms are excluded. In particular, the 61-atom world has 150 raw
vault atoms after donation, yet cannot pay even 62 insurance atoms. That rejection
must also roll back tentative recredit. The identical retained first withdrawal
then recomputes and pays one third of the actual recovery; a second unsigned
beneficiary withdrawal pays the remainder. No insurer key signs these payments.
An unpaid insurance remainder continues to block CloseSlab.

The shared test-only stock oracle has one explicit extension: an amount moved from
the first user's paid token balance into raw vault custody. Existing callers pass
zero. It retains every domain's backing/reserve/receivable/budget/spend checks,
decoded source and insurance summaries, zero user/OI obligations, fixed mint
supply, independent stock/reservation censuses and overlap computed from booked
stocks. The new selector frames the full resolved payout ledger, allowing exactly
the booked backing addition to snapshot residual, never the external transfer.

The observed lexicographic rank is:

`(Fresh sources, recoverable spend remaining, insurance payout remaining, unscanned slots, open slab)`.

Every successful wrapper continuation strictly decreases this rank: initial scan,
expiry, partial payout, final payout and closure. Donation and Clock are environmental
events with unchanged economic rank. Each world has five committed wrapper calls,
plus one SPL transfer in donated worlds; waiting is an exact rejection. The terminal
zero rank is assigned only after checking the actual tombstone and closed vault.

Final CloseSlab burns 0/207 booked atoms, sweeps exactly 0/89 external atoms to the
market authority, closes custody and refunds exact market/vault rent while retaining
the tombstone minimum. All user and insurer token balances and the complete mint
Account are checked. Donation/control pairs have identical insurance recovery,
burn and final mint supply, with the independently asserted user-to-authority
transfer accounting for their custody difference.

## Rollback and measurement

The shared transaction helper verifies signatures and packet size <=1232 bytes,
frames every compiled and tracked complete Account including absence and Clock,
and adjusts only the payer's actual required-signature fee. Exact error codes and
instruction indices are asserted. Successful wrapper/SPL log counts establish
that the requested prefixes actually ran before rejection.

The 44 rollbacks include pre-expiry insurance attempts, executed donation plus
rejected insurance suffix, executed expiry plus insurance payment followed by an
invalid System suffix, excess entitlement despite sufficient raw custody, closure
with unpaid insurance, and executed final burn/sweep/closure followed by an invalid
System suffix. The corresponding successful continuations all commit.

New selector: PASS, 1/1, 4.52 seconds; eight worlds, 44 commits, 44 exact rollbacks,
four donation/control comparisons and eight closures. Peak measured CU is 227544
under the shared 400000 limit. This peak includes all campaign transactions and
the shared fixture's measured user settlement calls; it is not a comprehensive
CU benchmark of every setup instruction.

## Validation and provenance

The user-specified SBF artifact was consumed read-only without rebuilding:

`/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so`

SHA-256: `79b298706ca7ffab09a017e5f37c4c0fc43d4e6292c092d126f4fbe8dff1b673`.
Engine pin in the worktree: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
No claim of a fresh artifact/source equivalence build is made. Host dependencies
were built locked/offline in a private 7 GiB tmpfs at the `/tmp` target below,
because the root filesystem had only about 557 MiB available. No shared target
cache was reused, changed or cleaned.

| Check | Result |
| --- | --- |
| New exact selector | PASS, 1/1; peak 227544 CU. |
| New exact selector listing | Exactly one test, zero benchmarks. |
| Six adjacent selectors below | PASS, 6/6 in 86.67 seconds. |
| Four metadata gates below | PASS, 4/4 in 0.01 seconds. |
| `cargo fmt --all -- --check` and `git diff --check` | PASS. |
| Production and status/findings diff against base | Empty. |

The adjacent scanner recredit control peaks at 225888 CU; the shared withdrawal
control at 227388 CU. The larger generated source control passes 54 worlds,
546 commits and 748 rollbacks with peak 431317 CU under its own bound. That
adjacent measurement is separate from the new product's 400000-CU limit. Existing
unused-support and Solana future-compatibility warnings remain. Commit whitespace
is checked with the final command below after creating the local commit.

Environment on each test command:

```sh
export CARGO_TARGET_DIR=/tmp/percolator-pr135-scope-q-target-20260914
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
cargo test --locked --offline --test v16_cu inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_prefix_custody_actionability::v16_program_cached_prefix_custody_surplus_cannot_capitalize_spent_insurance -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_prefix_custody_actionability::v16_program_cached_prefix_custody_surplus_cannot_capitalize_spent_insurance -- --exact --list
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_prefix_reuse::v16_program_terminal_prefix_rejects_retired_slot_reuse_with_exact_rollback \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_scan_recredit::v16_program_terminal_scan_rediscovers_earlier_insurance_after_later_expiry \
  inv_071_crank_progress::terminal_prefix_recredit::v16_program_later_expiry_recomputes_scanned_asset_insurance_entitlement \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::v16_program_terminal_scan_reconciles_external_surplus_arriving_after_cached_prefix \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::generated_prefix_actionability::v16_program_generated_scans_recompute_actionability_across_source_deadlines_and_prefixes \
  inv_071_crank_progress::terminal_cursor_time::v16_program_persisted_scan_reclassifies_time_without_skipping_live_siblings
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
git diff --exit-code e5273925 -- src tests/invariants/open_findings.tsv tests/invariants/invariant_status.tsv
```

## Changed files

- `cu/inv_070_terminal_prefix_custody_actionability.rs`: the one new bounded product.
- `cu/inv_070_zero_unattributed_terminal_residue_and_close_slab.rs`: module mount only.
- `cu/inv_071_terminal_prefix_recredit.rs`: shared stock oracle with explicit custody transfer; existing callers pass zero.
- `README.md`: evidence entry and limits.
- `coverage_reopenings.tsv`: comments only, row 424 remains OPEN.
- `pr135_scope_q_prefix_custody_actionability_20260914.md`: this audit.

## Remaining limits

This is fixed, bounded public evidence for INV-070/071/088 with related stock,
attribution and rollback checks. It does not close general environmental
invalidation, pending receipt histories, Recovery, retirement as an expiry
consumer, fresh reserve refill, alternate payout routes, native/dual quote custody,
authority succession, optional reserve ledgers, multiple competing insurers,
maximum asset shapes or arbitrary histories. Expiry is sampled exactly at slot 44;
late boundaries remain owned by adjacent tests. Beneficiary custody exists and
the market authority cooperates in final closure. No full suite or Kani run is
claimed, and existing REFUTED_CURRENT/OPEN_EVIDENCE classifications are preserved.
