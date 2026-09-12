# Terminal alternate custody with absent reserve holders

Base: `7374ab2f42bd2b9a6562afe8a8d5db1d43dad43b` from
`codex/astra-open-holdout-ledger-20260912`. Worktree:
`/tmp/percolator-astra-terminal-stock-progress-20260912`, branch
`codex/astra-terminal-stock-progress-20260912`.

Only the base checkout's production code, normative invariants and mounted tests
informed this increment. No open PR branch, diff or test was inspected, and the
sealed `coverage_reopenings.tsv` labels were not read or changed. No invariant
verdict or holdout disposition changes.

## New coverage

`cu/inv_082_terminal_custody_alternate.rs` adds one test:
`v16_program_absent_reserve_holders_receive_principal_through_alternate_custody`,
mounted through
`inv_082_state_indexed_liveness_theorem::terminal_destination_recovery::terminal_custody_alternate`.

Eight fresh public LiteSVM histories cross two SPL restrictions (a one-atom
delegate allowance or a distinct close authority), both claimant orders and
bundled/split progress. The backing provider owns 101 atoms of portfolio capital
and 401 atoms of backing principal; the insurance beneficiary owns 307 atoms of
portfolio capital and 509 atoms of insurance. A separate insurance operator
controls the two restricted ATAs. Mint supply is fixed at 1,318 atoms.

All economic state and collateral are created with System, SPL, ATA and wrapper
instructions. After funding and token authority configuration, the two holders
and operator keypairs are dropped. Every measured transaction has exactly the
unrelated keeper's signature, including resolution, new custody construction,
errors, payouts and retries. The administrator also supplies no signature.

At authenticated slot 10, permissionless stale resolution preserves every stock.
At slot 12, creating and initializing replacement custody before an unsigned
payout rejects `ExpectedSigner` at the owner-window boundary. At slot 13, both
terminal payout aliases reject the original ATAs with `InvalidTokenAccount`.
The alternate route uses `CreateAccountWithSeed` with the keeper as payer/base,
followed by SPL `InitializeAccount3` naming the original claimant as token owner.
These unencumbered destinations are deliberately not ATAs. They need neither a
claimant signature nor an additional mint, liquidity source or authority repair.

A bundle creates replacement custody, pays the first claimant and then rejects
the second claimant's restricted ATA. Every tracked and compiled Account rolls
back exactly, including new account existence, data, lamports, owner and rent;
the keeper loses only the network fee. Successful continuations allocate each
replacement account exactly its rent and pay exactly 101/307 atoms. Outstanding
portfolio count falls from two to zero in at most two successful payout calls
(one or two transactions, six System/SPL/wrapper instructions). Both original
payout requests reject `EngineNonProgress` on retry.

The independent oracle checks per-owner capital and token amounts, fixed mint
supply, exact vault conservation, empty PnL/fee/escrow/source/receipt state,
terminal portfolio classification, reserve buckets and budgets, authority
profiles/sequences, and unchanged original token Accounts after every measured
step. The final 910 vault atoms remain exactly classified as 401 backing plus
509 insurance. The keeper receives no quote tokens. All measured transactions
must fit the 1,232-byte wire bound and the existing 300,000-CU ceiling.

This adds bounded evidence for INV-018/021/024/027/067/071/073/078/080/081/082. It
extends the existing INV-018 delegation/revocation control, which requires the
owner to revoke delegation, and INV-082 missing/shared-destination recovery,
which reconstructs canonical custody. It does not repeat signed reserve
destination repair, native-token handling or pending terminal-fee accounting.

## Limits

The witness establishes terminal disposition of portfolio principal owned by
absent reserve holders. It does not establish unsigned withdrawal or forfeiture
of the remaining backing/insurance claims, earned-fee exit, portfolio deletion,
asset retirement or `CloseSlab`; INV-069/070 receive no new closure claim.
Positions, PnL, pending cohorts, fee credits, cancellation escrow, receipts,
reserve expiry/impairment, hostile mint freeze authority, Token-2022 and native
or secondary quote rails are outside this increment. Rent availability, classic
SPL execution and advancing authenticated time are explicit prerequisites.

## Validation

The new selector passed all eight worlds: 48 exact rollbacks and 16 committed
principal payouts, with final-run peak transaction consumption of 201,045 CU. Five nearby
controls passed: destination delegation/revocation, terminal reserve destination
repair, shared-destination recovery, native terminal surplus, and pending cohort
terminal fees. The invariant charter/index check, scoped rustfmt and whitespace
checks also passed.

The wrapper was rebuilt offline inside this worktree using platform-tools v1.52,
with the pinned engine `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`. Its SHA-256 is
`5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
The test uses no matcher program. The build and all test targets use this
worktree's own `target` directory. Existing Solana dependency warnings remain;
no production source or dependency change was needed.

Exact commands, executed from this worktree except for initial creation:

```bash
git -C /tmp/percolator-astra-watch.Cb2E7d worktree add \
  -b codex/astra-terminal-stock-progress-20260912 \
  /tmp/percolator-astra-terminal-stock-progress-20260912 \
  7374ab2f42bd2b9a6562afe8a8d5db1d43dad43b

env CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo build-sbf --tools-version v1.52 --offline -- --locked \
  > /tmp/astra-terminal-stock-sbf-20260912.log 2>&1

env CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo test --locked --offline --test v16_cu \
  inv_082_state_indexed_liveness_theorem::terminal_destination_recovery::terminal_custody_alternate::v16_program_absent_reserve_holders_receive_principal_through_alternate_custody \
  -- --exact --nocapture > /tmp/astra-terminal-stock-selector-20260912.log 2>&1

env CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo test --locked --offline --test v16_cu -- --exact \
  inv_018_quote_mint_vault_token_program_and_authority_integrity::v16_public_destination_delegation_is_route_scoped_and_revocation_restores_payout \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_reserve_destination_recovery::v16_program_terminal_reserve_destination_repair_preserves_signer_gates_and_value \
  inv_082_state_indexed_liveness_theorem::terminal_destination_recovery::shared_destination_recovery::v16_program_shared_destination_recovery_preserves_principal_and_single_rent_charge \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::v16_program_native_quote_terminal_surplus_sync_has_exact_token_and_lamport_disposition \
  inv_039_pending_loss_obligation_durability::terminal_fees::v16_program_pending_cohort_terminal_fees_stop_at_resolution_and_reach_insurance_exit \
  --nocapture > /tmp/astra-terminal-stock-controls-20260912.log 2>&1

env CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo test --locked --offline --test v16_program_fuzz_regressions \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  -- --exact --nocapture > /tmp/astra-terminal-stock-index-20260912.log 2>&1

sha256sum target/deploy/percolator_prog.so
rustfmt --edition 2021 --check --config skip_children=true \
  tests/invariants/cu/inv_082_terminal_destination_recovery.rs \
  tests/invariants/cu/inv_082_terminal_custody_alternate.rs
git diff --check
```

The invariant-index command checks the normative/index rows; it does not execute
the tests that read sealed reopening labels. No new public trace failed.
