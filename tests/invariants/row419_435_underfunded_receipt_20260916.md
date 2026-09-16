# Rows 419/435: mixed ADL and underfunded receipt frontier

## Provenance and disposition

- Worktree: `/dev/shm/row419-435-underfunded-receipt-20260916-sidecar`.
- Local branch: `codex/row419-435-underfunded-receipt-20260916-sidecar`.
- Source repository: `/tmp/percolator-astra-invariant-cycle-20260915-run`.
- Base: `b4091247021656d6877e2454ed04e4b0f8cbfefa`, the requested
  `origin/codex/astra-invariant-cycle-20260915` at worktree creation.
- Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Reused SBF: `/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so`.
  SHA-256: `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
- All writes, builds and logs are local under `/dev/shm`. No push; no edits to
  `/home/anatoly/percolator-prog`. No production, Cargo, fixture or TSV edits.
- No current public behavior violation was found in this bounded product.
  No production fix or vulnerable/fixed comparison is claimed.
- Rows **419/435 remain OPEN/missing** in `coverage_reopenings.tsv` and
  `open_findings.tsv`; INV-039 remains `REFUTED_CURRENT`.

## New boundary

Lane 21 expires a bankrupt close before mixed-role settlement with fresh source
backing. Lane 25 expires the mixed owner's own source backing before its debts
settle. Neither creates a genuine underfunded receipt with nonunit ADL and
historical insurance spend, then crosses another owner's still-unrealized
backed claim with an unpaid senior exit and terminal recredit.

The new child reuses `AttributionWorld` and the fractional-retirement signed
transaction, payout and deletion helpers. The existing parent gains only a
three-line module mount. All economic construction uses System, SPL Token, ATA
and wrapper instructions. LiteSVM loads the program, funds initial wallets and
advances Clock. There is no account-byte injection or private engine transition.

## Public history

1. Create three assets and five owners with deposits
   `[1000,250,1000,250,777]`. Publicly mint and deposit backing of one atom in
   source domain 5, expiring at 12, and 750 atoms in domain 3, expiring at 25.
   Mint another 123 atoms for later insurance funding.
2. Signed trades place owner 0 long against owner 1 on asset 2, owner 2 long
   against owner 3 on asset 1, and owner 2 long against owner 0 on asset 0.
   Quantities are 20, 20 and one/two lots respectively. All open at price 100.
3. Authenticated prices rise from 105 to 150 in five-unit increments at slots
   2 through 11. At slot 2, fixed-slot permissionless continuation liquidates
   owner 1 partially. Asset 2's long ADL factor becomes `714285700000000`, below
   `ADL_ONE = 1000000000000000`; effective OI is 14285714 rather than the
   creditor's retained raw basis of 20000000. Owner 0 remains both creditor and
   cross-asset debtor, with an exact 50/100-atom mark-derived debt.
4. Top up domain-2 insurance by 123 atoms through its public instruction.
   Permissionless liquidation of owner 3 finalizes a 750-atom close as
   `750 = 123 insurance + 627 B`. Resolve at slot 11 and continue at slot 16,
   after the configured five-slot unsigned-owner grace period.
5. Finish negative owners, then interleave mixed-owner and peer close steps.
   One exact waiting rejection per world rolls back all tracked Accounts;
   peer detachment supplies the required public progress. Lower-index source
   realization converts the peer's already-paid debt support first, preserving
   its domain-3 backing until the mixed owner receives a partial receipt.
6. Retry that receipt without new liquidity: it is an exact no-op and leaves
   the peer's senior capital and the unrelated 777-atom deposit intact.
7. Advance to slot 24, 25 or 26. Cross peer-first/mixed-owner-first order with
   `CloseResolved`/`ClaimResolvedPayoutTopup` for the existing receipt. The idle
   senior owner exits between the two claimants. Every nonterminal sweep changes
   economic state, and all five owners finish within 16 sweeps.
8. Retry both paid receipts exactly, delete all five portfolios with exact
   rent transfers, then expire remaining backing at slot 26 and drive bounded
   `CloseSlab`/`WithdrawInsuranceAsset` cleanup. Every scan progresses; insurance
   withdrawal plus the final supply burn partitions the entire residual vault.

The 24 worlds are two debts x three expiry landings x two claimant orders x two
receipt payout handlers. All setup worlds are independently constructed.

## Oracle and atom partition

The setup checks mixed roles, mark-derived peer PnL, the mixed owner's exact
principal debit, nonunit ADL, the insured close partition, and public stock,
reservation, stored-leg, loss-weight, pending-count and effective-OI censuses.
The OI oracle uses independent ceiling arithmetic and explicitly distinguishes
previous-reset residue from current-epoch loss weight.

`ReceiptBook` starts at the partial-receipt checkpoint. Its initial junior faces
are read from the receipt and the peer's remaining PnL. This is intentionally
**not a fully independent input-derived entitlement model of the setup**.
Senior values are tied to initial deposits and the first five-unit debt move.
From that checkpoint onward, the oracle independently computes source rate
quantization and conversion floors, preserves senior/face ownership, and derives
the common receipt budget from custody minus disjoint senior stocks, adding
back prior junior payments. It does not use the production payout rate to
predict payments.

Each continuation must match the predicted owner-token vector, capital, PnL,
receipt face/paid value, exact/unreceipted bound partition, payout-rate fraction
and custody equation. Source realization increases spent backing and provider
receivables by exactly the converted atoms. Expiry increases the junior budget
by exactly the expired fresh backing. A conserved one-atom wrong-owner
observation is rejected by the payout-vector assertion; this is an assertion
negative control, not a production mutation test.

| Mixed debt | Initial receipt face / paid | Checkpoint-predicted final owner payouts | Residual vault |
| --- | --- | --- | --- |
| 50 | 162 / 104 | `[1157,0,1423,0,777]` | 794 |
| 100 | 77 / 53 | `[1067,0,1473,0,777]` | 834 |

All suffix orders and expiry landings match those checkpoint entitlements.
Fresh realization creates the provider receivable that permits exactly 123
insurance atoms to be recredited after all portfolios disappear. Expiry before
realization creates no such receivable and permits zero recredit. Thus the
terminal disposition is `794 = 123 + 671` or `834 = 123 + 711` in the eight
fresh worlds, versus an entire 794/834-atom burn in the sixteen expired worlds.
Insurance cannot be recredited or withdrawn again on subsequent cleanup steps.

## Validation

The new selector passes 24 worlds with **172 successful-prefix rollbacks,
24 waiting rollbacks, 72 exact receipt retries, 120 portfolio deletions,
24 slab retirements and 8 nonzero insurance recredits**. Peak measured CU is
**348183**, including selected live cranks/liquidation, resolved continuations,
composed rollbacks and cleanup. The campaign and transaction helper enforce
600000 CU. This is not maximum-shape coverage or a measurement of every setup
instruction.

Every receipt-suffix instruction first executes successfully before an unsigned
deletion suffix fails at the exact instruction with `ExpectedSigner`. The
complete transaction Account frame and payer-adjusted lamport sum roll back;
the identical prefix bytes then commit. Successful calls preserve unrelated
Accounts. Test logs are under
`/dev/shm/row419-435-underfunded-receipt-20260916-logs`.

The final targeted invocation passes **5/5 exact selectors, 0 failures**, in
123.60 seconds. Scoped rustfmt, `git diff --check`, the protected diff against
`origin/main` (`d809e9a563d9b8bf38f32648b32a15d75f526ec8`) and the outside-scope
diff against the base all pass. Host tools: Cargo 1.90.0 and rustfmt
1.8.0-stable. The existing `solana-client` future-compatibility warning remains.

| Selector owner | Worlds | Peak reported CU |
| --- | --- | --- |
| New mixed underfunded receipt | 24 | 348183 |
| Lane 21 close preemption | 32 | 212217 |
| Lane 25 unsettled source expiry | 48 | 206217 |
| Classic fractional retirement | 32 | 190140 |
| Direct mixed-role resolution | 32 | 206217 |

Development failures concerned fixture/oracle assumptions: an inadmissible
mark-move/margin configuration, trying to create a receipt before peer
detachment, and counting previous-reset loss weight as current weight. The
final test uses an admissible configuration, checks the waiting rollback and
distinguishes current/reset-epoch weight. No production behavior or payout
expectation was weakened to accommodate an observed loss.

## Exact commands

Worktree creation, from the supplied source repository:

```bash
git worktree add -b codex/row419-435-underfunded-receipt-20260916-sidecar /dev/shm/row419-435-underfunded-receipt-20260916-sidecar origin/codex/astra-invariant-cycle-20260915
```

The following exports express the environment supplied with `env` to each
test invocation. Run from the isolated worktree:

```bash
export CARGO_TARGET_DIR=/dev/shm/row419-435-underfunded-receipt-20260916-target
export TMPDIR=/dev/shm/row419-435-underfunded-receipt-20260916-tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::underfunded_receipt::v16_program_mixed_adl_underfunded_receipt_preserves_senior_exit_and_expiry_progress -- --exact --nocapture

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::underfunded_receipt::v16_program_mixed_adl_underfunded_receipt_preserves_senior_exit_and_expiry_progress \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::close_preemption::v16_program_expired_close_preserves_mixed_debt_and_fractional_source_attribution \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::unsettled_expiry::v16_program_source_expiry_before_mixed_role_settlement_preserves_debt_and_weight \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::v16_program_mixed_role_fractional_source_preserves_attribution_through_expiry_and_retirement \
  inv_039_pending_loss_obligation_durability::mixed_role_resolution::v16_program_mixed_creditor_debtor_roles_preserve_owner_entitlement_through_resolution \
  > /dev/shm/row419-435-underfunded-receipt-20260916-logs/final-targeted.log 2>&1

rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_039_mixed_underfunded_receipt.rs tests/invariants/cu/inv_039_mixed_role_fractional_retirement.rs
git diff --check
git diff --exit-code origin/main -- src Cargo.toml Cargo.lock tests/fixtures 'tests/invariants/*.tsv'
git diff --exit-code b4091247021656d6877e2454ed04e4b0f8cbfefa -- . ':!tests/invariants'
git show --format= --check HEAD
```

## Limits and changed files

This finite product has three fixed asset assignments, five owners, one side
orientation, classic SPL custody, zero funding/fees and two debts. It does not
cover arbitrary schedules, maximum shape, mirrored ADL, additional quote rails,
authority changes, restart or close-expiry preemption. Junior entitlements are
checkpoint-derived, while senior principal, source conversion, later payments
and disjoint terminal disposition have explicit independent checks. No generic
row closure or full INV-086 equivalence is claimed.

- `cu/inv_039_mixed_underfunded_receipt.rs`: new public invariant matrix.
- `cu/inv_039_mixed_role_fractional_retirement.rs`: child mount only.
- `README.md`: bounded coverage note retaining OPEN/missing status.
- `row419_435_underfunded_receipt_20260916.md`: this report.
