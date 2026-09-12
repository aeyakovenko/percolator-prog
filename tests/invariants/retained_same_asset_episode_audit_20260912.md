# Retained same-asset position episode conformance

Date: 2026-09-12. Base: `abfaaf4bda787a99075e693c5c6fe908d5eacd23`, the requested
`codex/astra-open-holdout-ledger-20260912` HEAD. Worktree:
`/tmp/percolator-retained-capability-episode-20260912`, branch
`codex/retained-capability-episode-conformance-20260912`.

## Mounted witness

`cu/inv_012_retained_same_asset_episode.rs`, mounted by
`cu/inv_012_capability_and_delegate_scope.rs` as `retained_same_asset_episode`, owns
one test:

```text
inv_012_capability_and_delegate_scope::retained_same_asset_episode::v16_program_retained_exit_cannot_follow_lp_through_same_asset_flat_reopen
```

Four worlds cross both position signs with `TradeCpi`/`BatchTradeCpi` as the
opening and intervening writer. Every world retains both CPI consumer routes.
Public initialization, deposits, authenticated mark configuration and owner matcher
consent establish three portfolios with 10,000,000 quote atoms each. The matcher
grant has zero fee allowance and expiry slot 100; the authenticated Clock stays at
slot 1. The price stays at 1,000,000 and the position magnitude is `2 * POS_SCALE`.
The standard harness seeds initial account storage, token balances and signer SOL;
it never rewrites a live position, grant, generation or economic ledger here. The
matcher context is created by the system program and initialized by its owner.

Let T be the original taker, L the LP and B the bridge portfolio:

| Public step | T position | L position | B position |
| --- | --- | --- | --- |
| Initial matcher fill | q | -q | 0 |
| B closes L through the same matcher | q | 0 | -q |
| B reopens L through the same matcher | q | -q | 0 |
| Fresh T exit through the other CPI route | 0 | 0 | 0 |

After the initial fill, T signs and retains a complete exit on each CPI route,
both standalone and after a seven-lamport transfer from T's wallet to B's wallet.
Each exact signed transaction succeeds in simulation before the history advances;
full tracked snapshots confirm the simulation does not commit. No transaction is
re-signed or passed through the harness's current-binding adapter for its stale use.
The retained blockhash remains current throughout.

Both B transitions keep T's entire Account byte-identical. L and B each advance
exactly one position episode per transition. All portfolio identities, the asset
generation, and L's enabled matcher program/context/delegate, grant sequence,
fee cap and expiry remain unchanged. Thus the final exposure vector matches the
initial one, but L's economic episode is new. L never signs an intervening fill.

All four retained transactions then reject with exactly `EngineStale` at the
expected instruction index and before matcher invocation. For the prefixed forms,
the system transfer succeeds earlier in execution and is rolled back. Every
rejection compares complete `Account` values, including absence, data, lamports,
owner, executable flag and rent epoch, for 17 tracked keys: market, vault, mint,
matcher context/delegate, all three portfolios, all three token sources, all three
token destinations and all three owner wallets. The transaction fee payer and
runtime transaction history are outside this application rollback frame.

Fresh T consent changes only `account_b_position_epoch`. Its complete encoded
instruction must equal a newly constructed current request, preserving all other
bindings and economic fields. Both fresh routes succeed in simulation; the route
opposite the writer commits and closes both remaining positions. No LP regrant is
needed. Each of the three owners then withdraws exactly 10,000,000 atoms to their
own SPL destination. Each source stays empty, each paid portfolio has zero capital,
and aggregate capital and custody fall by exactly that owner's payout to zero.

After every committed trade, input-derived position/OI and per-owner principal
checks accompany zero PnL, exact vault/capital totals and engine market/portfolio
shape validation on read-only copies. There are 16 committed trades, 16 stale
rejections (eight with a rolled-back prefix), 16 original-validity simulations,
eight fresh-validity simulations and 12 exact owner principal payouts.

## Scope and distinctness

This is a same-asset position-episode ABA under a continuously enabled grant, with
an unchanged original taker and a third portfolio moving the LP through flat.
It differs from the cross-asset retained-episode witness because the actual leg
named in the retained exit disappears and is replaced at the same asset generation.

The scenario does not use owner reincarnation (`funded_owner_roundtrip`), an LP
role switch (`role_switch_generation`), matcher-program substitution
(`matcher_program_generation`), portfolio/grant recreation in a rejected bundle
(`portfolio_grant_rollback`), a committed revoking writer (`revocation_atomicity`),
or retirement/reuse of a traded asset generation (`used_generation_lifecycle`).
Those descriptions identify excluded dimensions; no open PR branch, diff or test
implementation was inspected or copied. Source and harness reads came from the
specified base checkout, plus installed build dependencies.

INV-004 gains direct retained reduction coverage across a real flat/reopen boundary.
INV-010/012 gain delayed-consumer composition with live, unchanged grant bindings.
INV-024 gains input-derived, owner-specific principal and custody endpoints.
INV-081 gains bounded success-state checks across these public trade/payout routes.
For INV-005, unchanged identity and authority fields are a control isolating the
episode guard; this is not new authority-rotation coverage. Exact rollback also
supplies bounded INV-080 evidence.

All invariant verdicts and holdouts 412/414 remain unchanged. Residual gaps include
automatic-revocation grant admission, authority rotation, expiry boundaries,
asset/market/portfolio replacement, cross-zero flips without an intervening flat
state, recovery and resolved-claim episodes, nonzero fees/funding/PnL, partial
fills, unilateral LP exit and arbitrary-length histories. LiteSVM simulation
establishes original validity; it does not model validator blockhash aging.

## Validation

The fresh default-feature wrapper SBF build and auth matcher SBF build passed.
The exact selector passed 1/1 (four worlds, 1.79s); nearby controls passed 4/4
(0.84s). There was no production failure or test expectation repair.
The invariant index passed 1/1 (0.00s), and scoped rustfmt and working-tree
whitespace checks passed. The staged and committed whitespace checks below
complete verification. Existing dead-code warnings in the regression harness and
the Solana client future-compatibility warning do not affect these results.

```bash
cd /tmp/percolator-retained-capability-episode-20260912
export CARGO_TARGET_DIR=/dev/shm/percolator-position-episode-20260912-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm

RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
CARGO_TARGET_DIR=/dev/shm/percolator-position-episode-20260912-matcher-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked

cargo test --locked --offline --test v16_cu inv_012_capability_and_delegate_scope::retained_same_asset_episode::v16_program_retained_exit_cannot_follow_lp_through_same_asset_flat_reopen -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --exact --nocapture \
  inv_004_position_episode_binding::v16_program_retained_position_binding_and_writer_rosters_are_source_complete \
  inv_012_capability_and_delegate_scope::v16_program_matcher_capability_expiry_is_clock_bound_on_both_cpi_routes \
  inv_012_capability_and_delegate_scope::v16_program_issue406_signed_trade_routes_invalidate_both_matcher_capabilities \
  inv_012_capability_and_delegate_scope::v16_program_issue406_matcher_trade_routes_preserve_only_participating_lp_capability
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_012_retained_same_asset_episode.rs tests/invariants/cu/inv_012_capability_and_delegate_scope.rs
git diff --check
git diff --cached --check
git show --format= --check HEAD
```

Built artifact SHA-256:

```text
5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e  percolator_prog.so
50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93  auth_matcher.so
```
