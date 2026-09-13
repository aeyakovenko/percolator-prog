# PR135 Scope U: terminal claim identity across late backing expiry

Branch: `codex/pr135-scope-u-terminal-claim-identity-late-expiry-20260913`.
Worktree: `/tmp/percolator-pr135-scope-u-20260913`.
Fetched base: `d134c64d788e49264ecc0187a75209053925f43a`, the latest requested
`origin/codex/astra-open-holdout-ledger-20260912` at worktree creation.
Engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

## Prior coverage and open boundary

Read the root README, invariant README, charter, `open_findings.tsv`, row 417
of `coverage_reopenings.tsv`, and the base's INV-066/067/068 claim families.
Row 417 is only a coverage label. No external PR branches, diffs or tests were
inspected. All working files and build output belong to this new worktree;
neither excluded checkout was edited.

Existing INV-066 late materialization varies unequal claimant order and exact
replacement around one backing expiry. INV-068 shared-owner and retired-sibling
tests keep two unequal receipts independent in one ATA, but materialize both
before that single expiry. INV-067 late-expiry recipient rotation rolls back
release plus paid prefixes; repeated-stock and destination-recreation histories
cross two expiries with both receipts already present and distinct owners.
Committed-conversion, fractional-source, overdue-history and fee-reclassification
tests separately cover denominator refinement and other stock dispositions.

The new composition is delayed creation of a second co-owned receipt while its
sibling already has a paid history, through two stock reclassifications and
repeated transaction aborts. The shared owner and destination make aggregate
custody and even owner-level totals insufficient to authenticate either episode.

Primary owner:
`inv_067_terminal_payout_completeness_and_exact_once_settlement::claim_episode_materialization`.
The new module reuses the existing public late-expiry fixture. Its only fixture
change permits selecting the owners in the existing staggered-source constructor.

## Guarantee and oracle

Twelve histories cross two co-owned claimant orders, second receipt creation
before/between/after the two expiries, and exact/one-slot-late landings. The
claimants have distinct portfolios and incarnations but one owner and one ATA.
Five owners hold six portfolios; three winning faces are 700, 1,000 and 1,300.
System/SPL/ATA/wrapper instructions create and fund all economic state, open and
close trades, publish marks, settle debtors and resolve at slot 12. Harness
inputs are program installation, initial signer SOL, Clock and blockhashes.
No program-owned economic bytes are injected or altered.

The independently maintained book uses the public deposits, sizes and mark
movement. Winning capital is 1,000 per portfolio. Snapshot residual starts at
`500 + 1 = 501`; domains 3 and 5 release `61 + 100 = 161` and `39 + 150 = 189`
at slots 13 and 15. Cumulative junior entitlement is
`floor(face * residual / 3000)` for each episode separately. The two co-owned
claims receive junior amounts `[116, 154, 198]` and `[217, 286, 368]`. Their
final joint junior payout is 566; merging faces before rounding would yield 567.
The independent source claimant receives 283 junior atoms.

At each checked prefix the book compares decoded market/portfolio/owner
provenance, incarnation, position epoch, entire embedded receipt, face, prior
bound, zero live-released face and cumulative paid value. Receipt expectations
are constructed from inputs, not copied from successful receipts. Snapshot slot
stays 12; the numerator increases by only the newly expired stock, and the
3,000-face denominator is preserved. Exact receipt mass and remaining bound are
separately checked as creation moves only the selected face between them.
Fresh/Expired bucket status, remaining source reserve, zero consumed backing
and provider receivable, senior capital, provider balance, engine/SPL vault and
fixed mint supply are checked alongside per-episode and per-destination payments.

Retained top-ups on the still-unreceipted portfolio preserve its sibling. The
second wave reverses payment priority. Each wave's release/payment prefix is
aborted twice by an ordinary invalid System suffix; in delayed-creation worlds
that prefix also materializes the new receipt. Logs require every prefix wrapper
success and the exact SPL transfer count. Complete tracked and compiled Accounts,
including absence, owners, mint, source state, Clock and receipt bytes, roll back;
the sole payer loses exactly one signature fee. The first wave retries the
unchanged instructions separately; the second retries the identical prefix in
one transaction. Zero-due retained claims are full economic-frame no-ops.

After both waves, another abort covers the source claimant's final bound
replacement/payout and both receipt-clearing calls. All claims then finish,
terminal replays cannot revive them, and all six portfolios are mechanically
closed with exact rent transfer to the market. Final per-episode SPL attribution
is `[1198, 0, 1283, 0, 1368, 0]`; the shared ATA contains 2,566. Source and user
obligations are zero. The provider retains its one never-deposited atom and the
vault retains exactly two independently computed rounding atoms.

Oracle sensitivity controls modify decoded observations only. They shift one
paid atom between the co-owned episodes, exchange whole receipts, or substitute
an incarnation. The episode equality oracle rejects each; the first two preserve
aggregate paid value and all actual SPL custody. These are oracle controls, not
public instruction attacks or production mutations.

## Limits and row impact

This is a fixed public conformance matrix, not arbitrary history induction or
independent-discovery evidence. It uses three assets, six portfolios, one shared
owner/ATA pair, two staggered sources, one standard SPL mint and no-CPI trades.
Fees, funding, positive live liens, Recovery, authority succession, simultaneous
expiry, source conversion and denominator shrinkage are outside this increment.
Only the two co-owned claimant orders vary; the independent source claimant's
terminal payout follows them. Arbitrary faces, claimant counts, quote rails,
destination replacement and portfolio recreation are delegated to other tests.
ClosePortfolio remains owner-signed. The endpoint classifies rounding residue;
it does not sweep/burn it or exercise asset retirement and CloseSlab.

INV-067 gains this episode-preserving terminal continuation. INV-010 gains
retained ordering and repeated rollback; INV-024 gains episode-level attribution;
INV-029/066 gain exact bound replacement and order/timing equivalence; INV-063
gains two-wave expiry composition; INV-068 gains shared-owner receipt uniqueness;
INV-070 gains zero user/source obligations and exact residual classification only.

Row 417 remains **OPEN**, with its eight data fields unchanged. Only a coverage
comment is added. `open_findings.tsv` and `invariant_status.tsv` are unchanged.
INV-024/067/070 remain REFUTED_CURRENT; INV-010/029/063/066/068 remain
OPEN_EVIDENCE. No production implementation change or status promotion is claimed.

## Validation and commands

Fresh default-feature SBF SHA-256:
`49f49d1a8a7a3c20e4e3e09c635c4d5ce08925056030668b6838ccf7224800b0`.
Build artifacts use a private 6 GiB tmpfs within the worktree. The first host
build exhausted shared `/tmp`; restarting with private `TMPDIR` resolved that
environment failure. Initial probe iterations corrected fixture expectations
around the zero-payment source normalization before snapshot/receipt creation
and used terminal claim top-ups for zero-due replay: repeating CloseResolved on
the already-completed source portfolio returns EngineNonProgress atomically.
No historical vulnerable artifact, external matcher or external test was used.

- New exact selector: PASS, 1/1, 17.02s; 12 worlds, 12 second-receipt creations
  (four early controls and eight delayed), 60 complete rollbacks and 72 portfolio
  closes. Peak campaign CU is 377,373, below the explicit 900,000 bound. Sixteen
  rejected wave prefixes include creation of the delayed receipt. The sensitivity
  checks exercise three observation mutations at 20 paid-receipt checkpoints.
- Related controls: PASS, 8/8, 56.98s, including existing single/staggered expiry,
  shared-owner retirement, delayed materialization, committed conversion,
  fractional-source and destination-recreation selectors.
- Metadata gates: PASS, 4/4, 0.01s. The exact focused selector lists one test.
- Fresh SBF and host compilation, formatting, working/staged diff checks and
  post-commit show-check pass. Findings and machine verdict files have no diff.
- No production mismatch, implementation commit, full suite or Kani run is
  claimed. Existing unused-support and Solana future-compatibility warnings remain.

Changed files, all under `tests/invariants`:

- `cu/inv_067_terminal_claim_episode_materialization.rs`: new probe and episode book.
- `cu/inv_067_terminal_claim_late_expiry.rs`: shared-owner staggered fixture option.
- `cu/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs`: module mount.
- `README.md`: coverage summary, limits and audit link.
- `coverage_reopenings.tsv`: comment only; row data and statuses are unchanged.
- `pr135_scope_u_terminal_claim_identity_late_expiry_20260913.md`: this audit.

```sh
git fetch origin codex/astra-open-holdout-ledger-20260912
git worktree add -b codex/pr135-scope-u-terminal-claim-identity-late-expiry-20260913 /tmp/percolator-pr135-scope-u-20260913 origin/codex/astra-open-holdout-ledger-20260912
cd /tmp/percolator-pr135-scope-u-20260913
mkdir -p target
sudo -n mount -t tmpfs -o size=6G,uid=1001,gid=1004 tmpfs /tmp/percolator-pr135-scope-u-20260913/target
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir target/deploy -- --locked
mkdir -p target/tmp
export TMPDIR=/tmp/percolator-pr135-scope-u-20260913/target/tmp
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
cargo test --locked --offline --test v16_cu inv_067_terminal_payout_completeness_and_exact_once_settlement::claim_episode_materialization::v16_program_coowned_claim_episodes_survive_deferred_receipts_and_two_expiry_rollbacks -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_067_terminal_payout_completeness_and_exact_once_settlement::claim_episode_materialization::v16_program_coowned_claim_episodes_survive_deferred_receipts_and_two_expiry_rollbacks -- --exact --list
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_066_resolved_payout_fairness_and_order_independence::v16_program_late_receipt_materialization_preserves_snapshot_entitlements \
  inv_068_receipt_uniqueness_and_monotonic_topups::v16_program_same_owner_receipts_keep_independent_topups_and_terminal_replays \
  inv_068_receipt_uniqueness_and_monotonic_topups::v16_program_retired_coowned_receipt_rolls_back_live_sibling_topup_bundle \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::late_expiry::v16_program_retained_claim_identity_survives_late_expiry_recipient_rotation_and_atomic_retry \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_repeated_stock::v16_program_receipts_preserve_identity_through_two_stock_releases_and_reversed_priority \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_conversion_then_expiry::v16_program_committed_conversion_then_late_expiry_preserves_receipt_attribution \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_fractional_source::v16_program_fractional_source_conversion_preserves_receipts_through_same_bucket_expiry \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_destination_recreation::v16_program_recreated_destination_preserves_receipt_identity_across_second_expiry_retry
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
cargo fmt --all
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
git diff --exit-code d134c64d -- tests/invariants/open_findings.tsv tests/invariants/invariant_status.tsv
sha256sum target/deploy/percolator_prog.so
```

The equivalent per-command `env` assignments were used during execution.
Build/probe/control/metadata logs are local generated files in `target/scope-u-*.log`.
