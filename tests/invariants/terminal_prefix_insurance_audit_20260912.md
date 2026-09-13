# Insurance withdrawals behind a persisted terminal prefix

## Provenance and scope

Base: `8fb23b4d194b0953c4a01a95c4b14c431e787978`, fetched from
`origin/codex/astra-open-holdout-ledger-20260912` before worktree creation.
Worktree: `/tmp/percolator-astra-persistence-20260912`.
Branch: `codex/astra-persistence-holdout-20260912`.
The primary checkout and `/tmp/percolator-astra-watch.Cb2E7d` were not edited.
Only the requested base and its pinned dependency source informed the work;
no open PR branches, diffs or tests were consulted or copied.

One new selector is mounted under INV-071:
`inv_071_crank_progress::terminal_prefix_insurance::v16_program_scanned_insurance_withdrawals_preserve_peer_entitlements_across_late_expiry`.
Implementation: [cu/inv_071_terminal_prefix_insurance.rs](cu/inv_071_terminal_prefix_insurance.rs).
Production code, dependencies, shared fixtures and machine invariant statuses are unchanged.

## Public history

System, SPL, ATA and wrapper instructions construct a four-asset market. Two
distinct insurance beneficiaries accept public authority handoffs before funding.
They fund 37 atoms on asset 1's long domain and 53 on asset 2's short domain.
The administrator funds 61 backing atoms on asset 3, expiring at slot 20.
Mint authority is revoked at a fixed 151-atom supply. No portfolios, positions,
claims or receipts are fabricated. LiteSVM supplies programs, signer SOL,
authenticated Clock and blockhash changes only.

At slot 10 the market resolves. `CloseSlab` commits cursor `0 -> 3`, passing both
funded insurance assets and stopping at later Fresh backing. Eight worlds cross
backing side, first beneficiary and exact/late expiry at slot 20/21.

1. At slot 19, `[first withdrawal, CloseSlab]` rejects on the parked cursor.
   The first withdrawal and its SPL transfer both execute before exact rollback.
2. The identical withdrawal commits independently. Its earlier asset changes,
   the peer's complete engine-slot bytes remain exact, and the cursor stays 3.
   A same-request replay rejects even though the peer's insurance remains funded.
3. At slot 20/21, `[CloseSlab, first withdrawal]` rejects after successful expiry.
   Neither tentative reserve release nor the still-funded peer replenishes the
   withdrawn owner's allowance. Stock, engine time and cursor roll back exactly.
4. `[CloseSlab, second withdrawal, invalid System suffix]` executes expiry and a
   real peer payout before exact rollback. The identical two-instruction prefix
   then commits. Both owner allowances are exhausted, the earlier paid asset
   remains byte-exact, and cursor 3 remains valid.
5. Both retained withdrawals reject while the vault still holds 61 atoms.
   One final `CloseSlab` burns precisely those atoms, closes the vault and leaves
   the typed market tombstone. The administrator receives only the exact vault
   rent and slab excess; its quote destination stays zero.

Each world has six rejected transactions, two committed owner payouts and three
committed slab calls. There are 48 exact rollbacks, 16 payouts and eight terminal
retirements overall. Transactions verify signatures and the 1,232-byte packet
bound. Every measured continuation or rejection is bounded by 300,000 CU.

## Independent checks

The oracle uses funding inputs and which withdrawals have committed:

```text
owner[i].paid      = withdrawn[i] ? insurance_input[i] : 0
owner[i].remaining = insurance_input[i] - owner[i].paid
engine insurance  = sum(owner.remaining)
engine/SPL vault  = engine insurance + 61
fresh backing     = normalized ? 0 : 61
151 minted        = 37 beneficiary A + 53 beneficiary B + 61 terminal burn
```

Asset-local budgets are checked separately from the global budget summary and
vault liquidity. Local backing, global Fresh summary, insurance spend and source
receivables reconcile with the public inputs. Independent stock and encumbrance
censuses run at economic checkpoints. Full Account framing includes compiled
transaction accounts, both beneficiaries and destinations, admin, mint, market,
vault and Clock. Exact rejection permits only the independently calculated payer
signature fee. Success checks frame unrelated Accounts; terminal checks compare
the complete mint and administrator Accounts and exact tombstone rent.

## Distinction and explored boundaries

| Existing coverage or inspected boundary | Disposition |
| --- | --- |
| Row 419 shared-holder pending loss | Existing two-domain partial detach, debtor deletion and resolution-placement control rerun; no new pending-loss matrix. |
| Row 417 receipt identity | Existing six expiry/claim orders with atomic/split catch-up and retained receipts across fresh realization/expiry rerun; no receipt-spend duplicate. |
| Row 424 terminal reserve backfill and retired-slot reuse | Those histories reject attempted new obligations. This test commits economic mutations to assets strictly behind the cursor. |
| Terminal provider/insurance retries | Existing independent allowance checks do not establish an already-persisted earlier-asset scan. Their selector is a nearby control here. |
| Retained terminal backing withdrawal | Existing withdrawal targets the Fresh bucket at the cursor. New insurance withdrawals target two earlier scanned assets. |
| Earlier-slot time classification | The scanner's Fresh-bucket wait rule prevents the ordinary Fresh stock in these histories from being skipped. The new test releases later backing and mutates earlier insurance budgets; it does not create earlier recredit work. |
| Successful cursor-invalidating mutation | The tested withdrawals debit insurance and vault equally, preserving residual. They require no restart. No generic invalidation/restart theorem or successful newly introduced earlier obligation is claimed. |
| Close/preemption order | Existing public pending-close/B and expired-close/Recovery/Resolved controls rerun. The new matrix adds terminal withdrawal/scan suffix ordering only. |
| Shared-lien expiry/refill | Existing audit establishes the separate live reservation/refill boundary; it was not duplicated or rerun in this increment. |

New evidence supports INV-024 owner attribution; INV-063 exact/late stock
normalization; INV-070 terminal disposition; INV-071 bounded scan continuation;
INV-080 exact rollback; INV-081 selected success-state predicates; INV-086 the
finite input-derived oracle; and INV-088 local budgets versus global summaries.
INV-039/066/067/068/076 remain owned by their existing pending-loss, receipt and
close controls, with no new evidence claim from this receipt-free test.

Rows 417/419/424 and all invariant verdicts retain their existing dispositions.
Nonzero insurance spend and provider receivables, earlier-slot recredit after a
residual change, arbitrary successful cursor invalidation, bankruptcy residuals,
fees/funding, repeated expiry/refill waves and maximum-capacity shapes remain
outside this addition. No production invariant failure was observed in the new
histories; no production fix or TDD failure is claimed.

## Validation

Fresh locked/offline default-feature SBF build with platform-tools v1.52 and
engine pin `394fd0bf2cb7d73df425eb3754dc3be1a0c44336` passed. SBF SHA-256:
`5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
All artifacts use the new private `/dev/shm/astra-persistence-20260912-target`;
no other worktree's build output was copied.

Final exact selector: **1/1 passed**, eight worlds and 48 exact rollbacks,
3.26 seconds, peak **79,619 CU**. The initial version also passed; the final
version adds explicit SPL-prefix counts and frames the already-paid asset through
the peer's withdrawal. Nearby controls: **10/10 passed**, 54.67 seconds.
Invariant charter/index and authoritative status projection: **2/2 passed**.
`cargo fmt --all -- --check` and working/staged whitespace checks passed.
Existing unused-support warnings in the index target and the Solana 1.18
future-compatibility warning remain.

Commands use the following environment on each Cargo invocation:

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-persistence-20260912-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export TMPDIR=/dev/shm CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu inv_071_crank_progress::terminal_prefix_insurance::v16_program_scanned_insurance_withdrawals_preserve_peer_entitlements_across_late_expiry -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_071_crank_progress::terminal_cursor_time::v16_program_persisted_scan_reclassifies_time_without_skipping_live_siblings \
  inv_071_crank_progress::terminal_reserve_backfill::v16_program_terminal_prefix_blocks_reserve_backfill_across_expiry_and_retries_cleanup \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::v16_program_retained_terminal_withdrawal_revalidates_expiry_after_scan_and_partial_payout \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::provider_insurance_retries::v16_program_terminal_provider_and_insurance_retries_preserve_separate_entitlements \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_expiry_interleavings::v16_program_retained_claims_commute_with_late_expiry_normalization_after_catchup \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_source_realization::v16_program_retained_receipts_preserve_identity_across_fresh_realization_or_expiry \
  inv_039_pending_loss_obligation_durability::shared_holder::v16_program_shared_holder_pending_domains_survive_partial_detach_and_debtor_close_orders \
  inv_071_crank_progress::v16_program_public_pending_close_preempts_b_stale_then_exposes_b_progress \
  inv_071_crank_progress::v16_program_public_expired_close_preempts_b_stale_and_preserves_terminal_progress \
  inv_088_global_summaries_are_not_account_local_proofs::v16_program_insurance_budget_global_summary_is_exact_in_every_four_domain_touch_order
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming
cargo fmt --all -- --check
git diff --check
git diff --cached --check
```
