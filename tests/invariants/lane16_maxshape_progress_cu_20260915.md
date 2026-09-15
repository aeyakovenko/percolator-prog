# Lane 16 Full-Market Hybrid Progress

## Verdict

One net-new public LiteSVM witness passes at the combined
**5,782 configured markets / 14 active legs / 28 value-bearing sources /
64-slot backlog / 16 hints / 48 feed references** shape. Peak measured continuation
CU is **1,241,192**, below the 1,375,000 test bound and the ordinary 1,400,000
transaction limit. The complete continuation takes 38 successful calls
and pays the two owners exactly 1,902,000 and 2,098,000 SPL atoms before closing
both portfolios.

**No current implementation LoF/DoS/CU violation was found.** This is positive
conformance, with no production change. **Row 423 remains OPEN and INV-077 remains
OPEN_EVIDENCE.** Three shared feeds serve the observations; distinct-feed maxima,
Recovery and liquidation are not certified by this result.

## Provenance

- Base fetched: `origin/codex/astra-invariant-cycle-20260915`,
  `89cd5088a9917d53cbbf8d4247be78adde89fd95`.
- Isolated clone: `/tmp/percolator-lane16-20260915`.
- Local branch: `codex/lane16-maxshape-progress-cu-20260915`.
- Engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Fresh default-feature Anchor-v2 wrapper and auth matcher SBF builds, offline
  locked dependencies, platform-tools v1.52.
- Wrapper SHA-256:
  `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
- Auth matcher SHA-256:
  `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

All edits, build outputs and logs are private to this clone or its `/dev/shm`
targets. The source checkout was used only for read-only cloning and remote
discovery. A copied host dependency cache accelerates compilation; cleaning
`percolator-prog` in the private target forces the harness to compile here.
Production files, engine pin, lockfiles and machine verdict tables are unchanged.

## Overlap Review

The charter is [scripts/loop.md](../../scripts/loop.md) and
[README.md](README.md). The owner remains
[cu/inv_077_hybrid_source_backlog.rs](cu/inv_077_hybrid_source_backlog.rs), mounted
under INV-077. Its original selector now delegates to the shared fixture with
fourteen market slots. The new selector is:

`v16_program_max_market_16_hint_hybrid_source_backlog_has_bounded_public_exit`.

| Existing witness | Previously exercised product | New intersection |
| --- | --- | --- |
| Lane 7 Hybrid/source/backlog | 14 markets, 14 legs, 28 sources, 64-slot backlog, 14 hints | All configured market slots and all 16 hints together |
| `v16_bpf_full_14_leg_16_hint_three_feed_refresh_is_bounded` | 16 markets, 14 legs, 16 hints, three shared feeds, one-slot move | 28 funded historical sources, two-chunk backlog and full market occupancy |
| `v16_attack_public_10m_market_max_source_owner_exit_stays_bounded` | 5,782 markets, 14 legs, 28 sources, AuthMark owner exit | Complete Hybrid observations and the 16-hint backlog schedule |

The original two Hybrid liquidation/Recovery selectors documented by Lane 7
remain separate, known construction failures. This lane does not rerun, repair,
ignore or convert them into positive evidence.

## Public Construction and Progress

The harness supplies ordinary zero-filled account buffers, then uses public
`InitMarket`, lifecycle activation, Hybrid configuration, portfolio creation,
deposit and trade instructions. No economic state, program-owned bytes or engine
state is injected. The final market is 10,483,956 bytes; the next slot would exceed
the 10 MiB account cap. Public activation appends all 5,768 slots after the initial
fourteen. Every configured asset is Active. Only fourteen carry positions.

A public System Program transfer supplies the slab's 71,969,224,640-lamport rent
gap before appending assets. The observed set is `0..13, 5780, 5781`; each is Hybrid
before exposure. Its three shared external Pyth fixtures are the same legitimate
external-account model used by the original owner.

The source history observes 100 -> 101 -> 100, closes and reverses both owners'
fourteen 1,000-lot positions, and reaches exactly 28 value-bearing, unliened source
records. The LP has 28,000 atoms of PnL; the counterparty has paid that amount.
Current reports then target 95 after a 64-slot gap. The public schedule is:

`[0], [0,1], ..., [12,13], [13,5780], [5780,5781], [0..13,5780,5781]`.

Every one of these seventeen keeper-only calls strictly decreases the sum of
pending slots over the sixteen observed assets: 1,024 -> 992, then fifteen
64-slot decreases, then 32 -> 0. The final call contains all sixteen hints and
48 references to three distinct feeds. The signed legacy transaction is exactly
486 bytes, and every crank asserts the 1,232-byte packet limit. No owner or admin
signature is needed for catch-up or the following account refreshes.

All catch-up prefixes preserve active positions, OI, source occupancy, funded
claims, counterparty account bytes, SPL custody and feed frames. The first sixteen
preserve both portfolios byte-for-byte. Unobserved market asset states remain
unchanged, and the two extra observed assets retain zero OI.

Two full-report refreshes require changed account state and current complete
health certificates. Fourteen matched owner reductions each remove exactly one
leg from both portfolios. Full conversion clears all 28 source records. The
input-derived total LP gain is 28,000 + 14 * 5 * 1,000 = 98,000 atoms, determining
the exact SPL payouts. Both portfolios finish with zero lamports and empty data,
their complete lamport balances move to the slab, and market capital, insurance,
vault balances and materialized portfolio count are zero.

## Exact CU

Measured on the final test implementation with the freshly built branch SBF:

| Continuation | Calls | Maximum observed CU |
| --- | ---: | ---: |
| Selected Hybrid backlog catch-up | 17 | 1,241,192 |
| Full-report account settlement/recertification | 2 | 1,059,312 |
| Matched owner reduction | 14 | 911,348 |
| Full-source conversion | 1 | 712,022 |
| Exact SPL withdrawal | 2 | 43,431 |
| Portfolio closure | 2 | 26,516 |

Catch-up CU by zero-based step: `0=115521`, `1=176212`, `2..13=176148`,
`14=172913`, `15=172658`, `16=1241192`. Setup is outside the 38-call continuation;
the 5,768 public activations peak at 7,002 CU each.

The original fourteen-slot control still peaks at 1,222,978 CU, with refresh
1,040,810, reduction 911,346, conversion 712,020, withdrawal 40,430 and closure
26,516. Separate adjacent controls measure 961,823 CU for the original 16-hint
refresh and 1,178,966 CU for the public full-market AuthMark `RebalanceReduce`.
These are observed transaction values, not universal exact costs for arbitrary
account keys, histories or transaction compositions.

## Fixture Corrections

The first new-shape run completed catch-up, settlement, reductions, conversion
and a payout, then closure returned `InsufficientFundsForRent`. The shared fixture
allocates a fixed 1,000,000,000 lamports even for a 10 MiB market. Closure credits
portfolio rent to that underfunded slab. Funding the rent gap through a System
Program transfer during construction makes the setup rent-exempt and preserves
the full public exit. This is not a current implementation DoS or a CU failure.

A subsequent newly added assertion expected LiteSVM to return `None` after
closure. LiteSVM retains a zero-lamport, zero-data account record. The final test
checks those exact closure fields and exact slab rent credit, matching the
existing harness convention. Neither development failure is claimed as a
parent-red/head-green production finding.

## Commands and Validation

Isolation and builds:

```sh
git clone --no-checkout --single-branch --branch codex/astra-invariant-cycle-20260915 /home/anatoly/percolator-prog /tmp/percolator-lane16-20260915
cd /tmp/percolator-lane16-20260915
git remote set-url origin git@github.com:aeyakovenko/percolator-prog.git
git fetch origin codex/astra-invariant-cycle-20260915
git switch -c codex/lane16-maxshape-progress-cu-20260915 origin/codex/astra-invariant-cycle-20260915
env CARGO_TARGET_DIR=/dev/shm/lane16-20260915-sbf CARGO_BUILD_JOBS=4 cargo build-sbf --tools-version v1.52 --sbf-out-dir /tmp/percolator-lane16-20260915/target/deploy -- --locked --offline
env CARGO_TARGET_DIR=/dev/shm/lane16-20260915-sbf CARGO_BUILD_JOBS=4 cargo build-sbf --tools-version v1.52 --manifest-path tests/fixtures/auth_matcher/Cargo.toml --sbf-out-dir /tmp/percolator-lane16-20260915/tests/fixtures/auth_matcher/target/deploy -- --locked --offline
cp -a /dev/shm/lane7-20260915-host /dev/shm/lane16-20260915-host
env CARGO_TARGET_DIR=/dev/shm/lane16-20260915-host cargo clean -p percolator-prog
sha256sum target/deploy/percolator_prog.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
```

Each host command below uses this environment:

```sh
export CARGO_TARGET_DIR=/dev/shm/lane16-20260915-host CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/tmp/percolator-lane16-20260915/target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu hybrid_source_backlog -- --test-threads=1 --nocapture
cargo test --locked --offline --test v16_cu -- v16_bpf_full_14_leg_16_hint_three_feed_refresh_is_bounded v16_attack_public_10m_market_max_source_owner_exit_stays_bounded --test-threads=2 --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_ -- --test-threads=3 --nocapture
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_077_hybrid_source_backlog.rs tests/invariants/cu/inv_077_bounded_work_and_maximum_shape_compute.rs
git diff --check
```

Final shared-owner run: **2 passed**, 52.51 seconds, log
`/dev/shm/lane16-hybrid-verified.log`. Adjacent controls: **2 passed**, 54.30 seconds,
log `/dev/shm/lane16-controls.log`. The development failures are retained in
`/dev/shm/lane16-hybrid.log` and `/dev/shm/lane16-hybrid-final.log`.
All **17 INV-079 guards pass**, 3.38 seconds, log
`/dev/shm/lane16-metadata.log`. Scoped rustfmt and `git diff --check` pass.
No unrelated suites or engine proofs were rerun.

## Remaining Frontier

The new coverage is a Live owner-exit continuation at maximum configured occupancy
with all sixteen hints, max active legs, max funded sources and two accrual chunks.
The 5,766 unobserved assets have no exposure. It is not an all-assets-exposed
market, forty-eight distinct feeds, simultaneous source liens, maximum funding
or fee work, permissionless liquidation, Recovery, resolved receipt payout or
slab retirement. Cooperative owners sign the matched reductions and withdrawals.
Row 423 and all authoritative machine dispositions retain their prior status.

Changed files: the existing Hybrid/source/backlog owner, its INV-077 parent module
documentation, the README index, and this report.
