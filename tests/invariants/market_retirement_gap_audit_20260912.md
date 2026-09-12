# Whole-market retirement and retained capability, 2026-09-12

Base: `origin/codex/astra-open-holdout-ledger-20260912`,
`adf21c4793b7c4202db8e9433c383bebb982d254`.
Worktree: `/tmp/percolator-astra-ultra-capability-b83e`.
Branch: `codex/astra-ultra-capability-holdouts-20260912-b83e`.
Inputs were this base's source, tests and invariant documents. No remote fetch,
open PR diff, other branch's test source, or production edit was used. No push.

## Overlap search

`rg` covered `CloseSlab`, `InitMarket`, `tombstone`, `reincarnation`, `shutdown`,
`fallback`, retained grants, and funded roles in `src/v16_program.rs`,
`INVARIANTS.md`, `tests/invariants/{cu,stateful}`, README and the existing audits.

| Retained owner | Existing relation; distinction here |
| --- | --- |
| INV-001 operation matrix; INV-007 tombstone | Already committed retirement rejects retained requests and address reuse. The new prefix retires a funded live market inside the rejected transaction, restoring executable pre-signed CPI consent. |
| INV-012 retained grant atomicity | Competing grant writers; no whole-market retirement. |
| INV-005 retained debit matrix | Portfolio recreation and reserve authority ABA; no market tombstone or vault closure. |
| INV-012 generation bundle / portfolio grant rollback | Asset activation and portfolio replacement; the containing market stays live throughout their prefixes. |
| INV-005 shutdown reserve ABA | Consensual oracle rotation and local reserve payouts; no whole-market close. |
| INV-021 funded lifecycle atomicity | Portfolio rent accumulates in a surviving slab; no resolution, slab refund or tombstone. |
| INV-070 terminal prefix reuse | Asset reuse after resolved scanning; no rollback from terminal closure to a funded live matcher capability. |

## New relation

[`cu/inv_012_market_retirement_rollback.rs`](cu/inv_012_market_retirement_rollback.rs)
is mounted from INV-012's `joint_incarnation_binding` owner. Four worlds cross
single versus two-leg batch CPI with mirrored position signs; withdrawal and batch
leg orders are coupled to sign, not an independent Cartesian product.

Each world starts with two public 1,000,000-atom deposits and installed LP consent.
A signed, simulated CPI request is retained byte-for-byte. The rejected transaction
executes two withdrawals, two portfolio closes, resolution and slab closure before
same-address `InitMarket` rejects `AlreadyInitialized`. Exact instruction index,
six wrapper successes and three SPL successes establish the entire paying/closing
prefix, including vault closure. All compiled retirement and retained-CPI accounts
compare as complete `Account` values, with only the exact four-signature payer fee
deducted. No protocol state is injected; System, SPL, ATA, wrapper and matcher
initialization establish all accounts. Clock, executable loading and signer SOL
are ordinary LiteSVM harness controls.

The identical signed CPI then lands without an LP signature. The opposite trade
flattens every leg at the original price, preserving each owner's principal and
zero PnL. A freshly bound retirement bundle pays each owner exactly 1,000,000 quote
atoms. Owner SOL is unchanged. Portfolio lamports first enter the slab, and market
authority receives exactly both portfolio balances plus original slab and vault
lamports minus the typed tombstone's rent. It receives no quote tokens. The mint,
matcher context/delegate and other protected accounts remain exact. A System-created
fresh market initializes under the same authority/mint with zero balances and no
portfolios; every old protected account, including the tombstone, remains unchanged.

The full rejected packet is 1,228 bytes. It omits a redundant CU-limit instruction;
the six-instruction successful retirement uses the runtime's 1.2M default budget.
No marginal probe is retained. This is passing contract coverage, not a new finding.

## Validation

The new selector and two adjacent controls passed (3/3, 5.24s). New evidence:
four exact bundle rollbacks, four unchanged signed CPI fills, eight total fills,
eight principal payouts, four committed retirements and four fresh-market controls.
Observed peaks: rollback **555,586 CU**, retirement **553,687 CU**, CPI **423,920 CU**,
fresh initialization **101,518 CU**. Adjacent portfolio-grant rollback peaked at
388,840 CU; funded lifecycle at 386,258 CU. Invariant index passed (1/1), as did
`cargo fmt --all --check` and `git diff --check`.

Wrapper and fixture matcher were rebuilt from this worktree with locked/offline
default-feature platform-tools v1.52 builds. Dependencies came from a private copy
of the existing build cache, with a fresh wrapper compilation.
Wrapper SHA-256: `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-capability-b83e-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=8 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_012_capability_and_delegate_scope::joint_incarnation_binding::market_retirement_rollback::v16_program_market_retirement_rollback_preserves_retained_cpi_and_attributed_exit \
  inv_012_capability_and_delegate_scope::joint_incarnation_binding::portfolio_grant_rollback::v16_program_failed_portfolio_reincarnation_preserves_retained_owner_grant \
  inv_021_account_creation_reallocation_close_rent_and_lamport_safety::funded_lifecycle_atomicity::v16_program_funded_lifecycle_spl_suffix_rollback_preserves_claim_and_rent
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all --check
git diff --check
```

## Remaining gaps

Rows **412/414/416/429 remain OPEN**, with no invariant or method verdict changes.
The new coverage joins whole-market rollback with retained capability execution;
successful same-address reincarnation is unreachable under the current tombstone
policy. Arbitrary grant/delegate histories across market, asset and portfolio
generations, repeated used-asset episodes, alternate matcher programs/contexts,
automatic revocation writers and grant re-delivery remain outside this increment.
Nonconsensual funded-role management and shutdown fallback attribution with separate
reserve beneficiaries receive no new coverage here. Nonzero fees/PnL, reserves,
expiry, secondary quote rails and maximum shapes are also excluded.
