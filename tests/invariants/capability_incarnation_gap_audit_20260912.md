# Capability/incarnation coverage increment, 2026-09-12

Base: `origin/codex/astra-open-holdout-ledger-20260912` at
`39022191e695d6702fcb3fcdb5b7cc270dc927d2`.
Branch: `codex/astra-capability-incarnation-gap-20260912-6d2a`.
Worktree: `/home/anatoly/percolator-astra-capability-6d2a`.
Only the supplied base informed implementation; no open PR code/tests were consulted.
Rows **412/414/416/429 remain OPEN**. Row 434 is outside scope.

## Selector audit

The initial `rg` audit covered INV-002/005/007/012/019/024/027/055/080/081/089,
including their public, CU and stateful selectors and existing documentation.

| Existing evidence | Missing composition added here |
| --- | --- |
| `joint_incarnation_binding`, `matcher_program_generation`, `used_generation_lifecycle`, `revocation_atomicity` | Matured asset activation, SPL withdrawal and successful sibling matcher CPI roll back together after retained oracle management rejects the new asset generation. |
| `retained_scope_product`, stateful `retained_grant_atomicity`, INV-021 `funded_lifecycle_atomicity` | Same-address close/refund/reinit rolls back when retained owner grant control reaches the replacement portfolio. Original signed grant remains deliverable. |
| `funded_oracle_succession`, `funded_backing_succession`, `terminal_role_handoff`, stateful `retained_debit_matrix` | Funded consensual oracle A->B->A, reserve ledger debits and shutdown ordering preserve two distinct beneficiaries and protected user capital. |

## New evidence

- [Activation rollback](cu/inv_012_generation_bundle_rollback.rs): eight worlds cross
  two replacement slots, single/batch CPI and position sign. Retirement precedes its
  required cooldown. The valid prefix withdraws SPL, activates a matured slot and
  invokes the matcher on a live sibling. An old-generation management suffix rejects;
  repairing only its generation makes the full simulated bundle admissible. Actual
  rejection restores all accounts, matcher writes and the generation frontier. The
  unchanged signed sibling trade lands, flattens, and the same activation payload then
  commits exactly once. Explicit fresh LP consent admits the replacement generation;
  both owners recover all collateral. INV-002/005/007/012/019/080/081/089 adjacency.
- [Portfolio grant rollback](cu/inv_012_portfolio_grant_rollback.rs): four worlds cross
  both CPI routes and position sign. Withdrawal, close, owner-funded System refund and
  same-address initialization execute before the old portfolio grant rejects. A fresh
  portfolio-ID/sequence grant validates the complete simulated prefix. Actual rejection
  restores rent, SPL, portfolio bytes and the ID frontier. The original signed grant
  subsequently lands and authorizes fresh CPI entry/exit and full owner withdrawals.
  INV-003/007/012/021/080/081 adjacency.
- [Shutdown reserve ABA](cu/inv_005_shutdown_reserve_aba.rs): eight worlds cross base/nonbase
  assets, backing domain side and shutdown-before/after rotation. A real insurance
  payout plus both oracle handoffs roll back under a stale backing suffix. Committed
  ABA invalidates both old reserve payloads; epoch-only replacements pay their separate
  owners. Full backing/insurance ledgers retain market, authority, domain, principal and
  paid-counter attribution; mint supply and owner-indexed SPL balances reconcile with
  the vault. Peer insurance and user capital remain protected. Opposite shutdown/payout
  orders produce equal final owner balances. INV-005/007/024/027/055/080/081 adjacency.

All accounts use public System/SPL/ATA and wrapper initialization; no protocol state
injection is added. Actual rejected bundles compare complete `Account` values for
transaction and protected accounts, including economic lamports and optional ledgers;
the independent payer differs only by the exact signature fee. Transactions verify
signatures and fit the 1,232-byte packet limit. Expected values derive from supplied
principal, generation frontiers and committed events.

## Validation

Three new selectors and five adjacent controls passed (8/8, 37.91s). New histories:
20 worlds, 28 rejected bundles, eight additional retained-insurance rejections,
32 reserve payouts and 32 user withdrawals. Peak rejection CU in this run:
activation 674,306; portfolio 387,340; funded ABA 57,319.
Invariant index passed (1/1); `cargo fmt --all --check` and `git diff --check` passed.

Fresh locked/offline default-feature SBF builds used platform-tools v1.52 and a private
target. Wrapper SHA-256: `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-capability-6d2a-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=8 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_012_capability_and_delegate_scope::joint_incarnation_binding::generation_bundle_rollback::v16_program_failed_asset_activation_restores_retained_sibling_cpi_and_frontier \
  inv_012_capability_and_delegate_scope::joint_incarnation_binding::portfolio_grant_rollback::v16_program_failed_portfolio_reincarnation_preserves_retained_owner_grant \
  inv_005_authority_incarnation_binding::funded_oracle_succession::shutdown_reserve_aba::v16_program_shutdown_and_funded_oracle_aba_preserve_separate_reserve_owners \
  inv_012_capability_and_delegate_scope::joint_incarnation_binding::matcher_program_generation::v16_program_matcher_program_roundtrips_compose_with_asset_reuse \
  inv_012_capability_and_delegate_scope::joint_incarnation_binding::used_generation_lifecycle::v16_program_used_asset_reuse_with_live_sibling_preserves_authorized_exit \
  inv_005_authority_incarnation_binding::funded_oracle_succession::v16_program_funded_oracle_succession_after_admin_burn_preserves_backing_exit \
  inv_021_account_creation_reallocation_close_rent_and_lamport_safety::funded_lifecycle_atomicity::v16_program_funded_lifecycle_spl_suffix_rollback_preserves_claim_and_rent \
  inv_024_attributed_quote_value_conservation::terminal_role_handoff::v16_program_terminal_role_handoff_preserves_reserve_beneficiaries_with_aliased_payer
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all --check
git diff --check
```

## Remaining gaps

This is bounded generic coverage, with no vulnerable-pin experiment or independent
holdout discovery. No status or method verdict changes. Whole-market close/reinit,
arbitrary retained grant histories across used assets and position episodes, alternate
delegate/context programs, nonconsensual funded-role management and market-authority
shutdown fallback beneficiary attribution remain outside the new oracles. Existing
selectors retain their own narrower evidence. No production change or newly confirmed
public LoF/DoS is claimed.
