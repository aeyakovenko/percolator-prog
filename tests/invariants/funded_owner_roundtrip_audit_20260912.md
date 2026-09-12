# Funded Owner Roundtrip Coverage, 2026-09-12

Base: `origin/codex/astra-open-holdout-ledger-20260912` at
`798c30d815892ac5283a62add9bfcc7f15b3a435`. The remote advertised that same SHA,
so no fetch was needed. Worktree: `/tmp/percolator-capability-history-20260912-c48f`.
Branch: `codex/capability-generation-history-20260912-c48f`.
Only source, tests and audit records from this base informed implementation;
no open PR diffs or other worktrees' test bodies were consulted.
Production files, Cargo files and invariant verdicts are unchanged. No push.

## Coverage Comparison

| Existing base evidence | Additional relation |
| --- | --- |
| Stateful INV-003 `discover_one_portfolio_incarnation_replay` and its operation matrix | The existing A-B-A intermediate portfolio is empty. Here B deposits separate principal, installs its own canonical context/delegate, executes nonzero CPI entry/exit and recovers its own SPL balance before A returns. |
| INV-012 `portfolio_grant_rollback` | That rejected close/reinit restores the first incarnation. Here both reincarnations commit, and a grant for the first incarnation rejects after a successful CPI against the third. |
| INV-012 `matcher_program_generation` and `used_generation_lifecycle` | Those retain portfolio ownership through asset/program histories. Here asset generations and matcher program stay fixed while the portfolio owner returns to A and the original A context/delegate is reused. |
| INV-012 `revocation_atomicity` and stateful `retained_grant_atomicity` | The new history isolates old portfolio ID from automatic episode revocation and competing owner-sequence changes: sequence, tuple, cap and expiry coincide at delivery. |
| Funded oracle ABA, whole-market retirement rollback and mixed retained-debit matrix | No second test of those relations is added. The new CPI-prefix/grant-suffix history preserves separate taker/A/B principal and matcher state through a used intermediate portfolio. |

## New Selector

[`cu/inv_012_funded_owner_roundtrip.rs`](cu/inv_012_funded_owner_roundtrip.rs),
mounted by `cu/inv_012_joint_incarnation_binding.rs`:

```text
inv_012_capability_and_delegate_scope::joint_incarnation_binding::funded_owner_roundtrip::v16_program_funded_owner_roundtrip_rejects_old_grant_after_current_cpi_prefix
```

Four worlds cross single CPI versus two-leg batch CPI with both position signs.
Each starts with one million atoms for each of the taker, A and B. The portfolio
address has owner/ID history A/2 -> B/3 -> A/4; the taker's ID remains 1 and the
market's next portfolio ID reaches 5. Asset generations remain 1/2/3 throughout.

A's original enabled grant is signed and successfully simulated before the first
close. Before either replacement, the test also signs a future CPI continuation
and a CPI/old-grant bundle using input-predicted third-incarnation identity and
the taker's two future B-fill episodes. Their serialized transactions are retained
unchanged, including signatures and blockhash. The bundle is not claimed to be
admissible before its predicted incarnation exists.

B receives no A principal: A first withdraws to its own ATA; B deposits from a
separately minted ATA and withdraws its own full principal after two actual CPI
fills. B's context uses the same honest matcher program with its distinct canonical
delegate. A then redeposits its own principal and explicitly installs its original
context/delegate on portfolio ID 4. The owner sequence returns to 3, with the
original enabled state, cap 37 and expiry 100 at Clock slot 1. The LP position
epoch is again zero; the taker's epoch is two.

The original signed grant rejects `EngineStale` at instruction 2. The unchanged
pre-signed CPI/grant bundle rejects at instruction 3, after exactly one successful
wrapper CPI fill and matcher invocation. Every compiled and protected Account
rolls back exactly, including both historical contexts, mint/vault, owner ATAs,
portfolio bytes, economic lamports and request/ID frontiers. Only the separate
payer loses the input-calculated signature fee. Changing only the grant's
`portfolio_id` from 2 to 4 admits the complete simulated bundle, excluding sequence,
tuple, funding, expiry or position availability as the rejection cause.

The independently pre-signed current CPI continuation then lands unchanged with
only taker/payer signatures. A's ID-corrected grant commits and a fresh CPI exit
flattens the position. A separate event oracle checks portfolio IDs, sequences,
epochs, signed positions and matching OI, request count, owner identity, cap/tuple,
capital/PnL, owner-indexed SPL balances, vault stock and fixed mint supply. Final
wallets each hold exactly one million atoms and both booked and raw vault stock
are zero. Both closes credit their full lamports to the market; replacement rent
is paid by the incoming owner and checked separately from network fees.

All economic and context construction uses System/SPL/ATA/wrapper/matcher public
instructions. Harness controls are signer SOL, Clock and program loading; there
is no program-owned byte injection or state restoration. Measured transactions
verify signatures, fit the 1,232-byte packet limit and enforce the existing
`MULTI_ASSET_OPEN_TRADE_CU_LIMIT` ceiling.

## Validation

New selector: **1/1 passed**, four worlds, eight stale-grant rejections, four
CPI-prefix rollbacks, sixteen committed CPI fills and sixteen principal payouts.
Peak measured transaction CU: **431,233**. Four adjacent exact selectors passed
**4/4**. The invariant index passed **1/1**; `cargo fmt --all --check` and Git
whitespace checks passed. Existing unused-support and `solana-client v1.18.26`
future-incompatibility warnings remain. The initial test-development failure was a fixture helper expiring the
retained blockhash; explicit public context initialization removed that transport
change. No production conformance failure was observed.

The private host target was seeded from an existing build cache, then the wrapper
and honest matcher were freshly built from this worktree, locked/offline with
default features and platform-tools v1.52. SBF SHA-256 values:

- Wrapper: `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
- Matcher: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

```sh
export CARGO_TARGET_DIR=/dev/shm/capability-history-c48f-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=8 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
(cd tests/fixtures/auth_matcher && CARGO_TARGET_DIR=/dev/shm/capability-history-c48f-matcher-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$PWD/target/deploy" -- --locked)
cargo test --locked --offline --test v16_cu inv_012_capability_and_delegate_scope::joint_incarnation_binding::funded_owner_roundtrip::v16_program_funded_owner_roundtrip_rejects_old_grant_after_current_cpi_prefix -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_012_capability_and_delegate_scope::joint_incarnation_binding::portfolio_grant_rollback::v16_program_failed_portfolio_reincarnation_preserves_retained_owner_grant \
  inv_012_capability_and_delegate_scope::joint_incarnation_binding::revocation_atomicity::v16_program_retained_capability_tracks_committed_revocation_after_bundle_rollback \
  inv_012_capability_and_delegate_scope::joint_incarnation_binding::matcher_program_generation::v16_program_matcher_program_roundtrips_compose_with_asset_reuse \
  inv_005_authority_incarnation_binding::funded_oracle_succession::v16_program_funded_oracle_succession_after_admin_burn_preserves_backing_exit
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all --check
git diff --check
git diff --cached --check
```

## Remaining Gaps

Rows **412/414/416/429 remain OPEN**, with no status or method promotion.
The generic INV-003 operation matrix was inspected for overlap, not rerun here.

- **412:** enabled retained-grant admission across automatic revocation within the
  same portfolio incarnation, arbitrary writer histories, cure/liquidation/keeper
  combinations and longer redelivery histories are not established by this test.
- **414:** grant-time scope over asset generations, append/reuse/Recovery/restart
  and consumption without fresh LP reauthorization remain outside this history.
  Asset generations deliberately stay fixed in all four worlds.
- **416:** no new oracle-dependent funded-role classifier, nonconsensual authority
  management, live oracle handoff or funded exposure/claim consent rule is tested.
- **429:** shutdown fallback, escheat and beneficiary attribution under market
  authority remain untested by this increment.
- Other owners, longer incarnation cycles, fees/PnL, impaired balances, alternate
  quote rails, market retirement, matcher-context account recreation and arbitrary
  delegate programs remain outside this bounded conformance slice. The same
  matcher context is reused, not closed and recreated.
