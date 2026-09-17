# Row 423 native latent capacity and redemption, 2026-09-17

Base: freshly fetched `origin/main`, `6ee17614f5e9fad1fdce11666b31dba3a33a3584`.
Private worktree: `/dev/shm/astra-ultra-row423-native-exit-20260917`.
Branch: `astra-ultra/row423-native-exit-20260917`; local commit only.
Engine: `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.

## Retained Increment

One new public LiteSVM selector crosses both position signs and both owner payout
orders. Thirteen assets build 26 detached source claims worth 50 atoms. Admission
of a four-lot fourteenth asset reserves the last two domains. Input-derived domain
sets independently prove the 26+2, 27+1 and 28+0 capacity frontiers and the spare
asset's 30-domain overflow. Both favorable sides materialize without converting
history or adding collateral, increasing the winner's total claim to 58 atoms.

The canonical native vault carries 17 uncredited lamports; recipient ATAs retain
19/23 unsynced lamports. At the 26+2 frontier, `SyncNative` succeeds before spare
admission rejects with `InvalidInstruction`. Complete transaction and tracked
Account snapshots restore the native amount, backing lamports, source claims,
latent reservation and unrelated accounts, except exact payer signature fees.
The reserved leg then completes its two settlements and flattens publicly.
The spare asset's complete state stays unchanged across this continuation.

After public resolution and the five-slot owner window, only the keeper signs
`CloseResolved`. Each call strictly reduces remaining source/payment debt; all
28 claims retire and both owners receive exactly 1,000,058 / 999,942 atoms. Each
actual payout is first submitted before an ordinary withdrawal that rejects in
Resolved mode. The wrapper prefix must succeed, complete Accounts must roll back,
and a transaction signed before the rejection must commit unchanged and transfer
a nonzero native amount exactly once. Four worlds produce eight payout rollbacks
plus four native-sync/capacity rollbacks, and 116 advancing terminal calls.

The oracle recomputes per-domain claims from positions and authenticated price
moves, checks live capital and backing prefixes, global stock/reservation/rate
censuses, generation attribution, paired OI and complete native Account images.
Rent and raw SOL stay outside the economic ledger. Peer portfolios/custody, mint,
authority wallets and other transaction accounts are framed on accepted calls;
rejection frames cover every tracked/message account. Expected token images are
constructed only in host memory and are never installed in LiteSVM.

Both portfolios are economically terminal before owner-signed deletion. Their
rent returns exactly to the market; vault/capital/insurance/portfolio counts and
OI finish at zero. Owner-signed SPL `CloseAccount` then redeems each funded native
ATA, delivering exactly payout plus token rent plus its original raw donation,
while preserving market, mint, empty booked vault and the other owner's accounts.
The vault's 17 unsynced lamports remain explicitly accounted for.

## Duplicate Review And Scope

`row423_mixed_latent_reclamation_20260917.md` covers competing same-batch closes
and SPL source-slot reuse. `row423_health_20260917.md` covers loss-funded partial
receipts and SPL top-ups. Neither combines the full historical/latent budget with
native amount/lamport rollback and actual owner redemption. The existing INV-081
native round trip has no positions or source claims; the INV-070 native Recovery
case has one claim and no source-capacity boundary. Maximum-market native insurance
tests exercise reserve budgets rather than a portfolio's 28-domain reservation.

All economic construction uses System, ATA, SPL and public wrapper instructions.
The existing native helper supplies LiteSVM's omitted native-mint genesis fixture;
no initialized program-owned bytes are edited. The spare asset is activated by
the public lifecycle route. No matcher is used by the new scenario. Only a new
CU child, its INV-028 mount, this note and the README change. Production, Cargo,
fixtures, shared helpers, `tests/v16_cu.rs`, TSVs and other row owners are unchanged.

## Exact Validation

Private host/SBF caches were copied from `percolator-public-gap-20260916-c91e-*`.
The default-feature wrapper and auth matcher were rebuilt offline from this
worktree with locked dependencies and platform-tools v1.52. The matcher is linked
only at the harness's ignored generated-artifact path for the existing control.

- Wrapper SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
- Auth matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row423-native-exit-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-ultra-row423-native-exit-20260917-sbf-target/deploy/percolator_prog.so
N=inv_028_source_domain_realizability_cap::native_latent_exit::v16_program_native_full_source_reservation_preserves_rollback_and_redeemable_exit
C=inv_028_source_domain_realizability_cap::historical_latent_capacity::generation_capacity_admission::v16_program_mixed_materialized_latent_reclamation_admits_only_fitting_replacement
cargo test --locked --offline --test v16_cu "$N" -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu "$C" -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_028_native_latent_exit.rs tests/invariants/cu/inv_028_source_domain_realizability_cap.rs
git diff --check
git diff --cached --check
git show --format= --check HEAD
git diff --exit-code 6ee17614f5e9fad1fdce11666b31dba3a33a3584 -- src Cargo.toml Cargo.lock tests/fixtures tests/support tests/v16_cu.rs ':(glob)**/*.tsv'
git diff --exit-code 6ee17614f5e9fad1fdce11666b31dba3a33a3584 -- . ':!tests/invariants/cu/inv_028_native_latent_exit.rs' ':!tests/invariants/cu/inv_028_source_domain_realizability_cap.rs' ':!tests/invariants/README.md' ':!tests/invariants/row423_native_latent_exit_20260917.md'
```

| Exact selector | Result | Measured peak CU | Maximum packet |
| --- | --- | ---: | ---: |
| N, new native capacity/exit | PASS: 4 worlds, 536 checked transactions, 12 rollbacks, 116 terminal calls; 10.45s | 927,646 | 656 bytes |
| C, existing mixed reclamation control | PASS: 4 worlds, 184 checked transactions, 32 rollbacks, 116 terminal calls; 10.83s | 1,006,327 | 799 bytes |

N's peak includes measured wrapper setup, mark, trade, crank, rejected suffix,
resolution, payout, deletion and redemption calls; raw System/ATA funding setup
is not comprehensively measured. The 1,375,000 CU ceiling and 1,232-byte packet
bound are unchanged. Only these two exact selectors run; no broad suite or Kani.
Scoped formatting, whitespace, commit and protected-path checks pass.

## Remaining Gaps

Row 423 remains OPEN. This is a native-primary market with 15 activated assets,
at most one active leg, integral AuthMark gains, solvent owners and zero fees or
funding. Simultaneous liens, Recovery/partial receipts at capacity, maximum feed
composition, used generations, fractional support and arbitrary resource histories
remain open. Setup/trades, resolution, portfolio deletion and native redemption
retain their signers; only terminal payouts are keeper-only. Native-vault surplus
disposition and slab retirement are outside this increment. No production mismatch,
engine proof, invariant-status promotion or excluded-row increment is claimed.
