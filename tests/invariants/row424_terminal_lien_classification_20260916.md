# Row 424: Expired Lien Terminal Classification

## Scope and Provenance

One new LiteSVM invariant product lives under
`inv_025_exact_stock_reconciliation::lien_recovery_attribution::terminal_expiry_classification`.
It composes a publicly created counterparty lien, separately funded insurance,
authenticated backing expiry, asset Recovery, resolved settlement, portfolio
deletion and final slab closure. It does not promote row 424 or any invariant.
No current production bug was found.

- Base: `e4dc038f84845da1542521cd1913ce1b7e4abaa7`, fetched current
  `origin/codex/astra-invariant-cycle-20260915` when the task began.
- Clone: `/dev/shm/row424-terminal-classification-20260916`, cloned with
  `--no-hardlinks` from `/tmp/percolator-astra-invariant-cycle-20260915-run`.
- Engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Reused SBF: `/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so`.
- SBF SHA-256: `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
- Private host dependency cache copied with `cp -a` from
  `/dev/shm/lane11-20260915-host`; test code is compiled in this clone.

`git diff --exit-code d64f049005847848b095b3b8b2d21318d0504296 HEAD -- src Cargo.toml Cargo.lock`
passes: production/build inputs match the documented lane24 artifact base.

The coordinator checkout is not edited. All tracked edits are under
`tests/invariants`; production source, Cargo files, fixtures and every TSV remain
unchanged. No fixture artifact installation or matcher build is needed.

## Prior Coverage Audit

| Existing owner | Existing evidence and distinction |
| --- | --- |
| INV-025 `lien_recovery_attribution` | Eight worlds reconcile live liens, unrelated reserve/owner operations, late rollback and one Recovery asset. They stop after senior withdrawals with 446 atoms in custody, without expiry, resolved claim completion or slab closure. The child reuses only its public construction and instruction helpers. |
| INV-025 `fee_bearing_recovery` | Exact stock census through terminal closure, but explicitly no mark/funding PnL, liens or rounding. This product has 150 atoms of source claims and real impaired liens. |
| INV-033 | Insurance-only admission rejects; mixed-reserve payout preserves a live counterparty lien and releases/converts it in Live mode. The wrapper has no insurance-credit reservation callsite, so an insurance-backed lien cannot be publicly constructed. This product checks that funded insurance remains disjoint through impairment and retirement. |
| INV-041 | Four-party Recovery exit permutations and equal-claim Live expiry/refill permutations establish their own allocation relations. This product independently crosses forfeit order and resolved payout order with a still-attributed impaired lien and exact final burn. |
| INV-063 | Retained Recovery expiry guards capitalization; spent-backing expiry/refill remains Live. Stateful resolved-expiry and post-snapshot products cover unencumbered claims and receipt recredit. Existing INV-028 impaired-lien terminal exit stops at portfolio deletion and carries no funded insurance. This product closes its own mixed classification history through actual vault/slab disposal. |
| INV-070 / row 424 | Native booked residue, native denomination/sync, shared custody, normal closure, mixed maturity, generated source actionability, earlier-insurance scan rediscovery and prefix custody already have owners. This is classic SPL with two assets, no external surplus and no persisted-prefix discovery claim. |
| INV-086 | Deterministic replay and generated/seeded frontiers already cover reference transitions and bounded owner exit. This adds a finite input-derived terminal entitlement comparison, not general reference-model equivalence or a new M/R verdict. |

The new relation is the complete `valid lien -> impaired lien -> Recovery
obligation -> resolved receipt/payout -> insurance exit -> burn` composition.
It is not a new name for the native booked-residue or normal-close products.

## Product and Economic Book

Sixteen independently constructed worlds cross two source sides, shutdown
before/after expiry, two owner-forfeit orders, and two resolved-settlement
orders. All program accounts originate in public System instructions; mint,
token accounts and custody use SPL/ATA instructions. The shared `World::new`
revokes mint authority after issuing exactly 2,024 atoms. Only public wrapper
instructions change program-owned state. There is no account-byte injection,
snapshot restoration, fabricated token balance or engine-only operation.

The public inputs are deposits 313/1,000, prices 100 then 105/95 (mirrored on
the other source side), positions 20/10, a two-unit risk increase, backing
150/17 and insurance 83. A third empty portfolio retains its untouched 211-atom
wallet and participates in resolved cleanup. Provider/insurance funding leaves
250 of the admin's original 500 atoms outside the vault.

| Boundary | Independent classification |
| --- | --- |
| Lien creation | Capital 263 + 900; claim faces 100 + 50; fresh backing 250 + 67; insurance 83. Total booked custody is 1,563. Initial-margin shortfall reserves exactly 53 or 61 backing atoms. Claim/lien labels add no custody. |
| Winning source expires at slot 3 | Winning backing becomes impaired 53/61 plus released residual 197/189; sibling fresh backing remains 67; capital and insurance are unchanged. Portfolio and market insurance-lien fields remain zero. |
| Recovery and resolution | Both assets enter Recovery; owner forfeits clear real exposure while zero-basis obligations and impaired attribution may remain. The bounded resolved suffix removes those obligations, expires the sibling bucket and releases the lien. |
| All owners paid and deleted | Payouts are 363/950/0, exact receipt face is 150 and snapshot residual is 317. Custody is exactly 250 = 83 insurance + 167 expired backing. No claims, liens, spent backing or provider receivables remain. |
| Insurance exit | A 40-atom payout followed by premature slab close fails at instruction 3 after one wrapper success and one SPL success. Full rollback preserves the exact request for retry. Payments 40 + 43 leave 167 atoms; an additional one-atom insurance request fails even though custody can fund it. |
| Slab closure | Exactly 167 SPL atoms burn. Token wallets remain `[363, 950, 211, 333]`, summing to final supply 1,857. The vault disappears, the market retains canonical tombstone rent, and all excess market/vault/portfolio rent goes to the admin. |

The complete raw/decoded stock and reservation censuses run from the liened
frontier through expiry, each forfeit, each resolved step, and the reserve
withdrawal boundaries. Exact input-derived partitions supplement those censuses.
Portfolio deletion adds all portfolio rent to the slab without changing owner
accounts. Final mint bytes differ only in the prescribed supply decrease.

All rejected transactions compare complete tracked and compiled Account images,
including lamports, owner, data and absence, allowing only the independently
calculated payer signature fee. Signature verification and the 1,232-byte
transaction limit are checked. Eight crossed-order histories include a precise
`EngineNonProgress` waiting rollback; every committed resolved step changes state,
every round progresses, and the cohort terminates within 32 rounds. Closure is
bounded to eight calls. The three explicit rejections per world are
`EngineLockActive` at instruction 2, instruction 3 after payout, and instruction 2
after insurance is exhausted, respectively.

Owner signatures are supplied during the configured resolution delay; admin
participation is required for shutdown, resolution and slab close. Insurance
withdrawals after portfolio deletion use the public unsigned beneficiary route.
The provider and insurance beneficiary share the admin wallet in this fixture;
distinct-beneficiary attribution remains owned by existing INV-024/070 tests.
Zero fees/funding, one expiry boundary, fixed amounts, classic SPL and two assets
are deliberate limits. No native, dual-rail, CPI, receipt-conflict, maximum-shape,
arbitrary environmental invalidation or public insurance-backed lien claim is made.

## Validation

The exact selector listing contains **1 test**. The nine-selector command below
returns **7 passed, 2 failed, 0 ignored**, exit 101: the new product and six
adjacent controls pass. The two failures are independently reproduced without
these changes at `e4dc038f`, using the same SBF in the detached baseline worktree
`/dev/shm/row424-terminal-classification-20260916-baseline`:

- `v16_program_fee_bearing_recovery_reconciles_raw_stocks_through_terminal_close`
  submits `CloseSlab { authority_epoch: 0 }` after reserve withdrawals advance
  authority epochs. Both checkouts fail at the unchanged line 552 with
  `InstructionError(2, Custom(19))` (`EngineStale`), measured 2,489 CU.
- `v16_program_mixed_reserve_payout_bundle_preserves_live_lien_classification`
  retains the pre-insurance-debit epoch in its second instruction. Both checkouts
  fail at unchanged line 451 with `InstructionError(3, Custom(19))`, where the old
  test expects `Custom(21)`. The insurance debit advances the epoch before its
  bundled backing request. These are pre-existing test expectation failures,
  not a new production defect or a passing-control claim. Neither file is edited.

The new product passes **16 worlds, 144 checked resolved/insurance commits,
56 complete Account rollbacks, 8 waiting rollbacks, 16 executed SPL-prefix
rollbacks, and 16 actual slab closures**. Peak measured CU is **405,865**,
rollback peak **155,100**, slab-close peak **32,890**. The new test asserts a
500,000 CU ceiling; its checked terminal transactions also request that budget.
Fixture/setup and final-close helpers retain their existing transaction budget
but their measured CU must meet the same ceiling. These are sampled CU bounds,
not maximum-shape certification; address-dependent CU variation is possible.

The touched parent control passes eight worlds, 272 transactions and eight late
rollbacks (833,040 CU under its existing 1,400,000 limit). The existing row424
scan control passes 16 histories, 88 commits, 120 exact rollbacks and eight
rediscoveries (221,388 CU). The other passing controls are INV-033 public API
absence, INV-041 four-party Recovery order, INV-063 spent-backing expiry, and
INV-070 source completeness. Full command output is retained in
`/dev/shm/row424-terminal-classification-20260916-logs/{selectors,baseline-controls,metadata}.log`.

The six exact INV-079 metadata guards below pass **6/6**, exit 0. Scoped
`rustfmt --check`, `git diff --check`, the staged diff check, post-commit
`git show --format= --check HEAD`, and the protected-path comparison all pass.
The source-build input comparison against the lane24 artifact base also passes.
The new selector is listed and executed rather than inferred from source text.
No TSV or machine status changes are included; row424 remains `OPEN`.

```bash
cd /dev/shm/row424-terminal-classification-20260916
export CARGO_TARGET_DIR=/dev/shm/row424-terminal-classification-20260916-target
export TMPDIR=/dev/shm/row424-terminal-classification-20260916-tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu inv_025_exact_stock_reconciliation::lien_recovery_attribution::terminal_expiry_classification::v16_program_expired_lien_recovery_classifies_terminal_atoms_once_across_orders -- --exact --list
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_025_exact_stock_reconciliation::lien_recovery_attribution::terminal_expiry_classification::v16_program_expired_lien_recovery_classifies_terminal_atoms_once_across_orders \
  inv_025_exact_stock_reconciliation::lien_recovery_attribution::v16_program_lien_recovery_preserves_attributed_stocks_and_late_rollback \
  inv_025_exact_stock_reconciliation::v16_program_fee_bearing_recovery_reconciles_raw_stocks_through_terminal_close \
  inv_033_insurance_backed_lien_single_classification::v16_program_mixed_reserve_payout_bundle_preserves_live_lien_classification \
  inv_033_insurance_backed_lien_single_classification::v16_program_public_source_lien_classification_never_double_counts_insurance \
  inv_041_deterministic_allocation_and_caller_order_independence::v16_program_four_party_recovery_exit_orders_are_economically_identical \
  inv_063_backing_expiry_normalization::spent_backing_expiry::v16_program_spent_backing_expiry_preserves_unpaid_claim_and_refill_exit \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_scan_recredit::v16_program_terminal_scan_rediscovers_earlier_insurance_after_later_expiry \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::v16_program_terminal_stock_and_close_slab_composition_is_source_complete

# Reproduces the two unchanged failures: 0 passed, 2 failed, exit 101.
cd /dev/shm/row424-terminal-classification-20260916-baseline
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_025_exact_stock_reconciliation::v16_program_fee_bearing_recovery_reconciles_raw_stocks_through_terminal_close \
  inv_033_insurance_backed_lien_single_classification::v16_program_mixed_reserve_payout_bundle_preserves_live_lien_classification
cd /dev/shm/row424-terminal-classification-20260916

cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant \
  inv_079_public_reachability_evidence::v16_dated_open_security_finding_benchmark_is_non_overclaiming \
  inv_079_public_reachability_evidence::v16_program_invariant_harnesses_are_test_free_roots

rustfmt --check --edition 2021 --config skip_children=true \
  tests/invariants/cu/inv_025_lien_recovery_attribution.rs \
  tests/invariants/cu/inv_025_terminal_expiry_classification.rs
git diff --check
git diff --cached --check
git show --format= --check HEAD
git diff --exit-code e4dc038f84845da1542521cd1913ce1b7e4abaa7 HEAD -- \
  src Cargo.toml Cargo.lock tests/fixtures \
  ':(glob)tests/invariants/*.tsv' ':(glob)tests/invariants/**/*.tsv'
```
