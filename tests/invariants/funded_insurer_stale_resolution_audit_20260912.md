# Funded Insurer Handoff Across Stale Resolution

## Scope and Provenance

This contribution adds one executable public LiteSVM selector:

```text
inv_005_authority_incarnation_binding::funded_insurer_stale_resolution::v16_program_funded_insurer_handoff_preserves_stale_deadline_and_permissionless_user_exit
```

The test is in `cu/inv_005_funded_insurer_stale_resolution.rs`, mounted by
`cu/inv_005_authority_incarnation_binding.rs`. The fresh contributor worktree is
`/tmp/percolator-funded-authority-20260912`, branch
`codex/funded-authority-containment-20260912`, based on
`adeac5fcacfb431db78c5646c9bdc9f70cf4f0ed`, the supplied integration branch's HEAD
when work began. Source and harness inspection used this pinned checkout only;
no open PR branches, diffs or tests were inspected or copied. Program source,
invariant verdicts and holdout classifications are unchanged. No push is required.

## New Public History

Four independent worlds cross beneficiary handoff at slot 12 or 13 with both user
payout orders. Public System and ATA instructions create accounts; SPL minting
creates exactly 994 atoms and permanently removes mint authority before deposits.
No program account bytes or token balances are injected. Airdrops provision native
transaction funds, and LiteSVM supplies authenticated Clock slots.

1. Distinct insurance/oracle, insurance operator, backing provider and admin roles
   are established by public consent. An authenticated mark of 100 at slot 3 and
   a ten-slot stale policy fix the permissionless resolution deadline at slot 13.
   The force-close delay is two slots, as the public policy requires a positive delay.
2. Users deposit 307 and 503 atoms, insurance receives 71, and separate short-domain
   backing receives 113 with expiry 1000. Both users open opposite `POS_SCALE`
   positions. The live operator withdraws 13 atoms, leaving 58 for the insurer.
3. At slot 12, a fully signed funded handoff followed by resolution with caller
   slot `u64::MAX` rejects as `OracleStale`, restoring the handoff and epoch.
   Admin replacement of the funded insurer rejects with `EngineLockActive`;
   operator replacement is `Unauthorized`; absent incoming signature is `ExpectedSigner`.
4. Incumbent and successor commit the beneficiary handoff at slot 12 or 13.
   Only the insurer field changes in the complete oracle profile, the authority
   epoch advances exactly once, and the complete wrapper config and oracle
   observation sequence remain fixed. Both users' complete Accounts stay unchanged.
   Successor oracle publication rejects as `Unauthorized`; the incumbent oracle
   key's old-epoch request rejects as `EngineStale`.
5. At authenticated slot 13, resolution with caller slot zero followed by signed
   beneficiary withdrawal rejects because users remain materialized. The original
   standalone resolution then succeeds with only the fee payer signing and records
   resolved slot 13. Signed risk increase rejects in Resolved mode.
6. Unsigned user close rejects at force-close maturity minus one. At exact maturity,
   a user SPL payout followed by premature insurance withdrawal rolls back both.
   Separate keeper-only closes pay each user's full principal in either order.
   `CloseResolved` leaves both empty portfolios registered, so reserve extraction
   still requires administrative or owner cleanup.
7. The successor cannot exercise admin portfolio cleanup. Admin cleanup followed
   by premature reserve withdrawal restores portfolio deletion and all rent.
   Successful admin cleanup of each empty portfolio transfers its exact rent into
   the slab and decreases materialized count from two to zero without user signatures.
8. Former insurer/oracle, operator, admin and backing provider cannot take the
   terminal insurance. The successor needs its own signature and receives exactly
   58 atoms. It cannot withdraw the separate backing; the unchanged provider signs
   for exactly 113. User, operator and unrelated actor balances stay fixed.

Every world asserts these final SPL balances independently of the program's
aggregate accounting:

| Recipient | Atoms |
| --- | ---: |
| Former insurer and continuing oracle | 0 |
| Successor insurer | 58 |
| Unchanged live insurance operator | 13 |
| Unchanged backing provider | 113 |
| First user | 307 |
| Second user | 503 |
| Market admin | 0 |
| Vault | 0 |

## Assertions and Novelty

Each of the 18 rejected transactions per world compares 20 complete protected
Accounts, including optional account absence, plus the payer Account after its
exact signature fee. Successful continuations compare every Account outside their
declared mutation set. Stage checks validate market shape and remaining portfolios
against the market, capital, zero PnL, insurance domain budgets, unencumbered backing
principal, custody balance, immutable mint supply and all seven recipient balances.
This provides 72 exact rollbacks, including four distinct successful prefixes:
beneficiary handoff, resolution, user SPL payout and rent-bearing portfolio cleanup.

| Existing base coverage | Distinct evidence here |
| --- | --- |
| `funded_oracle_succession` and funded backing/oracle succession histories | Only the funded insurance beneficiary changes; neither oracle nor backing authority changes. Handoff composes with the original authenticated permissionless stale deadline while both users have open positions. |
| `shutdown_operator_departure` | No asset shutdown or operator rotation. The history crosses global Live/Resolved admission via stale resolution and the subsequent force-close timeout. |
| `terminal_role_handoff` | Handoff occurs while users are live and funded. Separate payer and all role keys avoid the terminal payer-alias product. |
| `v16_program_funded_insurance_handoff_preserves_incumbent_oracle_and_operator` | Adds two exposed users, unchanged stale evidence through handoff, exact stale/force-close maturity, keeper payouts and required admin deregistration before beneficiary exit. |
| Existing authenticated-clock resolution controls | Adds funded consent, role separation, user priority and full reserve attribution across both deadlines. |

INV-005 owns funded consent, role separation and epoch checks; INV-020 owns the
unchanged authenticated deadline and untrusted caller slots; INV-024 owns the
independent recipient ledger; INV-027 gains a zero-junior-claim principal and
terminal reserve-priority control; INV-055 owns Live/Resolved and force-close
admission; INV-081 gains bounded successful composition and representation checks.
INV-021/080 gain exact rent and transaction rollback evidence. These are bounded
conformance witnesses, not universal proofs or new closure claims.

## Validation

Commands run from the contributor worktree, with a fresh isolated build directory:

```sh
export CARGO_TARGET_DIR=/dev/shm/percolator-funded-authority-20260912-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export TMPDIR=/dev/shm CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0

RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu inv_005_authority_incarnation_binding::funded_insurer_stale_resolution::v16_program_funded_insurer_handoff_preserves_stale_deadline_and_permissionless_user_exit -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --nocapture --test-threads=1 funded_oracle_succession:: funded_backing_succession:: shutdown_operator_departure:: terminal_role_handoff:: v16_program_funded_insurance_handoff_preserves_incumbent_oracle_and_operator v16_attack_permissionless_resolve_uses_authenticated_clock_slot v16_attack_permissionless_resolve_rejects_fresh_market
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row inv_079_public_reachability_evidence::v16_program_invariant_harnesses_are_test_free_roots
cargo fmt --all -- --check
git diff --check
```

Results: SBF build passed; the new selector passed all four worlds; all nine
nearby controls and all four invariant-index/status/root checks passed.
Repository-wide formatting and diff whitespace checks passed. Host compilation
emitted existing support-module dead-code warnings and the Solana 1.18 client
future-compatibility notice. No program failure or production edit was required.

## Residual Gaps

This increment uses one base asset, a constant authority mark, zero funding and
fees, balanced positions, no positive claims or receipts, fresh unencumbered
backing and available incumbent/successor signatures. It does not cover moving
prices, external/hybrid reports, bankruptcy, pending loss, insurance encumbrance,
backing impairment/expiry or earnings, repeated handoffs, independently retained
signed requests or A-B-A replay, non-base authorities or maximum-capacity markets.
Oracle publication with a fresh epoch after handoff is left to nearby controls;
this sequence deliberately preserves the stale evidence. User payouts are
permissionless after the timeout, but reserve extraction still depends on signed
portfolio cleanup and reserve beneficiaries. Slab retirement and token custody
closure are outside this witness; the empty slab retains the portfolio rent.
All existing invariant verdicts and holdout labels remain unchanged.
