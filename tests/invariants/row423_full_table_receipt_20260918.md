# Row 423 full-table receipt continuation, 2026-09-18

Base: `origin/main`, `6c267d28d7c46262829c3fffaf535c8d8a8c689c`.
Worktree: `/dev/shm/astra-ultra-row423-coverage-20260918`.
Branch: `astra-ultra-row423-coverage-20260918`; local commit only.

## Increment and duplicate review

One new exact LiteSVM selector in `cu/inv_028_shared_source_late_exit.rs`
reuses `SharedHistory` without changing any helpers. Public trades give two
claimants all 28 occupied source slots, with one/two atoms per domain and a
common solvent debtor. Both are flat at resolution. The test selects the
published expiry boundary for the first seven assets and asserts that exactly
14 domains expire while the remaining 14 stay fresh.

Fourteen bounded close calls normalize one expired bucket apiece without payment
or changing either full claim table, before source retirement begins.

The first claimant retires all 28 sources, realizes 14 backed atoms into capital,
and receives a 14-face receipt. Its initial junior payout is independently
computed as `floor(14 * 42 / (14 + 56)) = 8`. The peer still has all 28 claims.
An early keeper top-up is a complete economic Account no-op. Every subsequent
peer close retires exactly one source and preserves the first receipt and wallet.
Peer realization reduces the unreceipted bound from 56 to zero; its 28-face
receipt receives 28 atoms. The retained first receipt then receives exactly six
more atoms and finalizes. Repeating that top-up cannot pay again.

All 56 source retirements check source-count progress, domain-local changes,
custody deltas, peer and mint frames, stock/encumbrance/rate censuses, and total
custody. Intermediate source retirements pay nothing. Final owner balances are
1,000,028 / 1,000,056 / 999,916 atoms, with no new funding. Payout and top-up calls
are keeper-only; owners sign the final portfolio deletions. Vault, capital,
insurance, source claims and portfolio count finish at zero.

Existing evidence differs:

- The same file's late-claimant control materializes the last domain and drains
  two full tables with fresh backing; it has no retained partial receipt.
- `row423_health_20260917.md` tests loss-funded receipt completion with at most
  18 future domains. It does not retain a full peer table during a partial receipt.
- `row423_mixed_latent_reclamation_20260917.md` and
  `row423_native_latent_exit_20260917.md` cover capacity admission/reuse and funded
  exits, without this receipt/expiry composition.
- The existing INV-077 all-source lien selector already constructs 28 simultaneous
  liens. Repeating that dimension would duplicate existing coverage.

The nearest existing control's engine-pin assertion is revalidated locally against
the unchanged current pin, following neighboring Row 423 tests. Its scenario and
the shared certification helper are unchanged. Early drafts failed only in test
setup/expectations: a missing per-asset refresh, an incorrect assumed backing
lifetime, and omitting the bounded expiry-normalization prerequisites.

## Exact validation

The default-feature wrapper was rebuilt offline from this worktree with locked
dependencies and platform-tools v1.52, using a private copy of the existing SBF
cache. A private host cache was also copied. Neither selected scenario needs a
matcher; no fixture files or generated fixture paths were changed.

- Engine: `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
- Wrapper SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row423-coverage-20260918-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-ultra-row423-coverage-20260918-sbf-target/deploy/percolator_prog.so
C=inv_028_source_domain_realizability_cap::historical_latent_capacity::shared_source_late_exit
cargo test --locked --offline --test v16_cu "$C::v16_program_full_source_tables_preserve_partial_receipt_through_peer_exit" -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu "$C::v16_program_shared_history_preserves_late_claimant_capacity_and_exact_exit" -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_028_shared_source_late_exit.rs
git diff --check
git diff --cached --check
git show --format= --check HEAD
```

| Exact selector suffix (under `C` above) | Result | Measured peak CU |
| --- | --- | ---: |
| `v16_program_full_source_tables_preserve_partial_receipt_through_peer_exit` | PASS: 1 world, 301 recorded calls, 14 expiry steps, 56 source retirements; 4.44s | 948,061 |
| `v16_program_shared_history_preserves_late_claimant_capacity_and_exact_exit` | PASS: 4 worlds, 1,128 recorded calls, 236 terminal calls; 17.14s | 930,604 |

Targeted rustfmt, diff/commit whitespace and three-file scope checks pass. Logs
are `/dev/shm/astra-ultra-row423-coverage-20260918-{new,control}.log`.

Measured peaks cover recorded public trade/mark/settlement, resolution, terminal
continuation and deletion calls; initial fixture funding/configuration is excluded.
The existing 1,375,000 CU assertion is unchanged. Only the two exact selectors
above are run; no broad suite or engine proof is claimed.

## Limits

Row 423 remains OPEN. This is one SPL history with integral AuthMark gains, two
full claim tables, one expiry boundary and a fixed payout order. Simultaneous
liens with receipts, admission while a receipt is pending, Recovery, native quote,
maximum feed/active-leg composition and arbitrary future-resource histories are
outside this increment. Production, Cargo, fixtures, TSVs, support helpers and
invariant statuses are unchanged.
