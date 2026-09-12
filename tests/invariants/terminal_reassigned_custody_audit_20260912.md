# Terminal progress after canonical custody ownership changes

## Scope and isolation

One public LiteSVM test is mounted as
`inv_082_state_indexed_liveness_theorem::terminal_reassigned_custody::v16_program_reassigned_canonical_custody_keeps_absent_owner_claims_through_retirement`
in [`cu/inv_082_terminal_reassigned_custody.rs`](cu/inv_082_terminal_reassigned_custody.rs).
It checks supported successful behavior, exact rejected preconditions and complete
transaction rollback. Production source, dependencies and invariant verdicts are unchanged.

The isolated branch `codex/inv-terminal-keeper-authority-20260912` in
`/tmp/percolator-inv-terminal-keeper-20260912` starts at
`adeac5fcacfb431db78c5646c9bdc9f70cf4f0ed`, the current HEAD of
`codex/astra-open-holdout-ledger-20260912` in
`/tmp/percolator-astra-watch.Cb2E7d` when work began. Neither the original workspace
nor the integration checkout was edited. No open PR branches, diffs or tests were
inspected or copied. Novelty was assessed using the inherited coverage summaries
and production code; existing generic public System/SPL fixture helpers are reused.
No sealed reopening ledger was read or changed. No push is performed.

## Public sequence and attribution

Eight fresh worlds cross both claimant orders, `CloseResolved` versus
`PermissionlessCrank`, and bundled versus split replacement custody construction.
System, SPL, ATA and wrapper instructions construct all economic accounts and
state. Only signer airdrops and authenticated Clock advancement are environment
fixtures. The mint has six decimals, a fixed supply of 456 atoms, and no remaining
mint or freeze authority. No matcher program is used.

At slot 1 two owners deposit 101 and 307 atoms. Their original ATAs retain 5 and 7
tokens. The administrator funds 19 backing atoms expiring at slot 20, transfers 11
external surplus tokens to the canonical vault, and retains 6 tokens in its ATA.
All three SPL accounts then undergo a signed `SetAuthority(AccountOwner)` to one
different custodian. The two portfolio-owner keypairs and custodian keypair are
dropped. The transferred token accounts remain occupied, with unchanged mint,
address and nonzero balances. Their new owner cannot participate in recovery.

This explicit transfer assigns only the 18 tokens already in those accounts to
the custodian. It does not transfer the two portfolio claims or the administrator's
insurance-beneficiary entitlement. The test preserves the complete original token
Accounts and absent signer Accounts through every measured stage.

An unrelated keeper resolves stale state at slot 10. Each portfolio owes exactly
`(10 - 1) * 3 = 27` maintenance atoms, regardless of the later payout slot or the
compatibility alias's zero fee-rate argument. At slot 12, replacement creation
followed by unsigned payout rejects `ExpectedSigner` within the owner window.
At slot 13 the original ATAs reject `InvalidTokenAccount` because they now belong
to the custodian. A bundle creating replacement custody and paying the first
owner before encountering the second owner's unusable destination also rolls back.

The keeper creates each alternate account with `CreateAccountWithSeed` using its
own payer/base signature, followed by `InitializeAccount3` binding the original
portfolio owner. No owner signature, authority repair, ATA reconstruction or new
quote liquidity is needed. The two exact payouts are **74 and 280 atoms**. Only
the keeper signs resolution, replacement creation, principal payout and retry
transactions. Both payout aliases reject repeat settlement with `EngineNonProgress`.
The independent oracle checks capital, fee cursor, zero PnL/escrow/receipt/source
state, SPL owners and amounts, mint supply, budgets, backing and engine shape.

The configured administrator subsequently performs mechanical deletion of the
empty portfolios. An unrelated keeper cannot authorize that deletion. A bundle
deleting both portfolios before attempting insurance withdrawal to the reassigned
admin ATA rejects and restores both portfolios and all rent. Successful deletion
moves both portfolios' exact lamports into the slab. Keeper-created admin-owned
custody then receives exactly **54 maintenance atoms** through an admin-signed
insurance withdrawal; insurance and backing authority identities remain unchanged.

At slot 19 the unexpired backing prevents slab closure with exact rollback. At
slot 20 one slab call expires the backing label without moving tokens; the next
burns exactly **19 atoms**, sweeps exactly **11 surplus atoms**, closes the vault
and writes the exact rent-exempt market tombstone. A late failing SPL transfer
after that complete close restores mint supply, token balances, vault existence,
slab bytes and length, and all refunded lamports. The unchanged valid slab call
then succeeds.

Final SPL attribution is `74 + 280 + 65 + 5 + 7 + 6 = 437`, with `456 - 437 = 19`
burned. The admin's replacement holds 54 earned maintenance atoms plus 11 swept
surplus atoms. The custodian still holds exactly its separately transferred 18
tokens. No keeper receives quote tokens. Replacement rent is paid exactly once
by the keeper. Final admin lamports increase by the initial slab lamports plus
both portfolio refunds plus vault rent, less the retained tombstone rent.
Every rejected transaction restores all tracked and compiled Accounts exactly,
except for the independently calculated signature fee. Successful transactions
also check the keeper's complete Account against signature fees and newly created
custody rent.

## Bounds and novelty

After resolution, the economic rank is the number of portfolios not yet terminal,
followed by the number of missing replacement accounts for those portfolios.
Split creation lowers the second component; each payout checks a state-derived
decrease of exactly one in the first component. There are exactly two successful
payout calls and at most four keeper transactions for creation and payout.
Administrative completion separately lowers the number of materialized portfolios,
the outstanding fee budget, the fresh backing count and finally the open slab.
There are exactly two successful slab calls. All measured transactions must fit
1,232 wire bytes and the existing 300,000-CU custody ceiling.

| Existing coverage | New distinction |
| --- | --- |
| `terminal_custody_alternate` | Previously delegated or separately closable owner-owned custody and principal-only disposition with remaining reserve claims. This history changes the SPL owner, preserves the transferee's nonzero token entitlement, charges maintenance and reaches full administrative retirement. |
| `shared_destination_recovery` | Reconstructs shared canonical custody. Here each original canonical address remains occupied under an unavailable different owner; only alternate non-ATA destinations are constructed. |
| `terminal_reserve_destination_recovery` | Repairs custody for signed reserve payouts. Here both portfolio signers are unavailable and public principal payout precedes a separately signed fee withdrawal and cleanup. |
| `terminal_destination_variants` | Preserves delegate and close-authority capabilities of an admin-owned destination through disposal. Here original token ownership has changed and that destination is unusable; absent user claims and replacement beneficiary custody are included. |
| `pending_destination_recovery` | Pending-cohort settlement and destination reconstruction are separate existing coverage. This history is flat and solvent, with no pending cohort, and tests owner transfer, fee attribution and complete slab disposal. |

| Invariant | Bounded evidence |
| --- | --- |
| INV-018 | Canonical vault, classic-SPL mint, distinct token owner versus portfolio owner, unencumbered replacement custody and exact token movement. |
| INV-021 | Public creation, exact replacement rent, signed portfolio deletion into the slab, vault refund and tombstone rent; late failures restore creation and deletion. |
| INV-024 | Independent input ledger distinguishes absent owners' net principal, maintenance beneficiary, original token transferee, backing retirement and surplus. |
| INV-027 | Disclosed fees leave exact net principal and preserve reserved backing until expiry. This is a solvent control, not an underbacked junior-priority proof. |
| INV-067 | Two distinct claims reach terminal state exactly once; replay rejects without new payout. No nonzero recovery receipt is claimed. |
| INV-069/070 | Backing expires in one bounded step; classified stock burns, external surplus sweeps, custody closes and exact tombstone rent remains. |
| INV-071/082 | Both public payout aliases lower the observed economic rank; administrative completion has a separate finite rank and named authority. |
| INV-073/078 | Absent portfolio signers and unavailable original destination authority do not prevent permissionless net-principal disposition. |
| INV-081 | Full public transitions compose with independent economic assertions, engine shape validation and exact transaction rollback, also supporting INV-080. |

This is not a universal liveness proof. Advancing authenticated time, a funded fair
keeper, supported classic SPL execution, configured stale resolution and a
cooperative administrator/insurance beneficiary are required. The owners remain
absent after payment: custody is correctly attributed, but subsequent owner-signed
spending is not demonstrated. Final fee withdrawal, deletion and slab closure are
administrative, not permissionless. Distinct administrator and beneficiary roles,
absent beneficiaries with outstanding reserve claims, nonzero PnL/receipts,
pending losses, fee credits, cancellation escrow, insolvency, provider earnings,
insurance impairment, active positions, maximum-capacity scans, native/secondary
quote rails and Token-2022 remain outside this increment.

## Validation

The selector passes all eight worlds: 16 exact owner payouts, 80 complete rollback
checks and 16 successful slab calls. Final-run peak measured CU is 198,142. All
six nearby controls below pass (6/6 in 18.02 seconds). The first host
compile exposed test API naming mismatches, corrected before public execution.
No public conformance failure occurred and no expected failure was weakened.
The invariant charter/index selector passes 1/1; repository formatting and
working-tree whitespace checks pass. Staged and committed whitespace are also
checked by the commands below.

The locked/offline default-feature SBF build uses platform-tools v1.52 and the
unchanged engine pin `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`. Its SHA-256 is
`5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
Build outputs are isolated in `/dev/shm/percolator-inv-terminal-keeper-target`
because the disk filesystem had only 2 GB free. Existing unused-test-support
warnings and the Solana dependency future-compatibility warning remain.

Commands from the isolated worktree, except initial creation:

```sh
git -C /tmp/percolator-astra-watch.Cb2E7d worktree add \
  -b codex/inv-terminal-keeper-authority-20260912 \
  /tmp/percolator-inv-terminal-keeper-20260912 \
  adeac5fcacfb431db78c5646c9bdc9f70cf4f0ed

export CARGO_TARGET_DIR=/dev/shm/percolator-inv-terminal-keeper-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=6 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
cargo build-sbf --tools-version v1.52 --sbf-out-dir "$CARGO_TARGET_DIR/deploy" --offline -- --locked
sha256sum "$PERCOLATOR_FUZZ_SBF"
cargo test --locked --offline --test v16_cu inv_082_state_indexed_liveness_theorem::terminal_reassigned_custody::v16_program_reassigned_canonical_custody_keeps_absent_owner_claims_through_retirement -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_reserve_destination_recovery::v16_program_terminal_reserve_destination_repair_preserves_signer_gates_and_value \
  inv_027_protected_principal_seniority::maintenance_terminal_seniority::v16_program_maintenance_terminal_orders_pay_senior_principal_before_protocol_extraction \
  inv_039_pending_loss_obligation_durability::resolved_histories::pending_destination_recovery::v16_program_pending_cohort_repair_is_atomic_and_keeper_settlement_preserves_attribution \
  inv_077_bounded_work_and_maximum_shape_compute::terminal_destination_variants::v16_program_terminal_sweep_preserves_destination_authorities_and_exact_disposal \
  inv_082_state_indexed_liveness_theorem::terminal_destination_recovery::shared_destination_recovery::v16_program_shared_destination_recovery_preserves_principal_and_single_rent_charge \
  inv_082_state_indexed_liveness_theorem::terminal_destination_recovery::terminal_custody_alternate::v16_program_absent_reserve_holders_receive_principal_through_alternate_custody
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
rustfmt --edition 2021 --config skip_children=true tests/invariants/cu/inv_082_terminal_reassigned_custody.rs
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
```

Logs use `/tmp/percolator-inv-terminal-keeper-{sbf,selector,controls,index}.log`.
The index selector validates charter and coverage-index completeness; it does not
execute the tests that read sealed reopening labels. No invariant verdict changes.
