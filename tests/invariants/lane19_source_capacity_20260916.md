# Lane 19: Last Latent Hybrid Source Across Backlog

## Result and Provenance

Eight public LiteSVM worlds pass. A reversed Hybrid position retains 27 funded
source claims and reserves the final domain across a two-chunk accrual backlog.
Cadenced and deferred settlement reach identical source claims and exact SPL
payouts within each direction. The test checks 48 completed-prefix rollbacks,
40 ranked catch-up/settlement calls and eight same-slot target replacements.
Peak measured history CU is 870,754, below the 1,375,000 bound.

No public-route LoF, persistent DoS or CU bug was found; no production fix was
needed. **Row 423 remains OPEN. INV-028 remains REFUTED_CURRENT and INV-077
remains OPEN_EVIDENCE.** This is bounded conformance, not generic row closure.

- Read-only base: `/tmp/percolator-astra-invariant-cycle-20260915-run`, branch
  `codex/astra-invariant-cycle-20260915`.
- Base HEAD: `ef1647f22b196b01d55af0ab88f89c51b0bef61a`.
- Private clone: `/tmp/percolator-lane19-source-capacity-20260916`.
- Local branch: `codex/lane19-source-capacity-20260916`; no push.
- Engine pin from unchanged Cargo.lock:
  `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Fresh default-feature Anchor-v2 wrapper and auth matcher SBF builds use
  platform-tools v1.52, locked offline dependencies, and new private `/dev/shm`
  targets. Only host dependency artifacts were copied, then the local wrapper
  package was cleaned so the harness compiled from this clone.

SHA-256 artifacts:

| Artifact | SHA-256 |
| --- | --- |
| `/dev/shm/percolator-lane19-20260916-sbf/deploy/percolator_prog.so` | `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f` |
| `/dev/shm/percolator-lane19-20260916-matcher/deploy/auth_matcher.so` | `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93` |

The matcher copy at the private harness's required fixture path has the same hash.
Neither supplied checkout was edited.

## Ownership and Non-Overlap

The new owner is [cu/inv_028_hybrid_latent_backlog.rs](cu/inv_028_hybrid_latent_backlog.rs),
mounted below the existing Hybrid capacity/carry owner. Its sole selector is:

`inv_028_source_domain_realizability_cap::historical_latent_capacity::hybrid_capacity_carry::hybrid_latent_backlog::v16_program_last_latent_hybrid_domain_survives_reversal_and_chunked_backlog`

The survey covered row 423 and its surrounding evidence, `scripts/loop.md`, the
invariant README, the capacity owners, and reports for lanes 1-18.

| Existing coverage | Distinction in this lane |
| --- | --- |
| Existing INV-028 Hybrid carry owner | Monotone two-slot movement materializes only one side of the final asset. This lane reverses the position and target with nonzero carry, then materializes the last domain after a 64-slot gap. |
| Existing INV-028 active-leg admission | AuthMark increases at 27/28 occupied domains. This lane exercises Hybrid report ingestion, target/anchor replacement and fractional carry through chunked settlement. |
| Lanes 7 and 16 Hybrid/source/backlog | All 28 sources are already occupied before backlog. Here the deferred first chunk must leave the last domain latent and both portfolios byte-identical. |
| Lanes 4 and 5 observation/carry coverage | Current-report and maximum-leg recertification products do not combine a 27-source historical frontier with deferred final-domain realization after reversal. |
| Lane 3 capacity controls and Lane 6 cumulative limits | Generation admission, accounting stock and OI/position headroom products; no repeated admission or limit test is added here. |
| Lanes 1, 11, 14 | Retained policy or funded oracle authority succession, without this capacity/carry/backlog product. |
| Lanes 12 and 15 | Hybrid liquidation reward provenance and competing recipients; this lane has no liquidation reward. |
| Lanes 2, 8-10, 13, 17-18 | Terminal receipts, debt, reserve succession or retirement; this lane ends with cooperative Live owner exits. |

Historical construction, the first one-slot Hybrid settlement, source/stock
censuses and complete owner payout reuse the existing invariant-owned helpers.
The original selectors and their assertions are unchanged. The known failing
older Hybrid liquidation/Recovery construction selectors are not rerun or
presented as positive evidence.

## Public History and Independent Oracle

System account creation, SPL mint/ATA/deposit operations, wrapper initialization,
AuthMark history, Hybrid configuration, trades, cranks, conversion, withdrawals
and closes create every economic state. The shared fixture creates an auth matcher
through the System Program; this selector uses bilateral single/batch trades.
Only the existing legitimate external Pyth and Clock fixtures supply external
inputs. No market, portfolio or matcher bytes are injected.

Thirteen detached, two-sided histories produce 26 claims totaling 50 atoms.
A seven-lot Hybrid position moves from 100 to `100 + sign`, realizes seven atoms
in domain `26 + (sign > 0)`, and leaves a 5,000-unit price-cap remainder. A batch
trade reverses it into five lots of the opposite sign while retaining every claim.
Both future domains plus the occupied history consume exactly the 28-domain budget.

A newer publication in the same slot replaces the target with `anchor - sign*100`,
where `anchor = 100 + sign`. This resets the old nonzero remainder without moving
the effective price or realizing the final source. Reports are renewed at each
Clock checkpoint. One schedule advances directly by 64 slots; the other settles
both accounts at 32 slots and again at 64 slots.

Before target arrival, the independent arithmetic oracle is:

```text
numerator(t) = anchor * 150 * t
effective_price(t) = anchor - sign * floor(numerator(t) / 10000)
remainder(t) = numerator(t) mod 10000
last_domain_claim(t) = 5 * floor(numerator(t) / 10000)
total_gain(t) = 50 + 7 + last_domain_claim(t)
```

For sign -1, anchors/prices at 0/32/64 are 99/146/194, with remainders
0/5,200/400. For sign +1 they are 101/53/5, with remainders 0/4,800/9,600.
Neither path reaches its target, so the chunk-boundary carry remains substantive.
Final payouts are respectively **1,000,532 / 999,468** and
**1,000,537 / 999,463**, independent of crank cadence and account order.

Every required continuation first runs as the completed prefix of a transaction
whose withdrawal suffix has the wrong owner. Exact `Unauthorized` at instruction 3
and one wrapper success log prove the prefix executed. All transaction Accounts,
both portfolios/custodies, mint, matcher, authorities and Clock restore exactly,
except the payer's calculated signature fee. The unchanged crank instruction then
commits with only the payer/keeper signature. Successful cranks preserve peer
Account bytes, SPL custody and report bytes.

The lexicographic rank is `(pending slots, missing owner economic atoms,
noncurrent certificate count)`. Every catch-up/settlement commit strictly lowers
it, and each checkpoint must finish within eight keeper calls. On the deferred
first chunk, pending slots fall 64 -> 32, both portfolio Accounts are unchanged,
the last source is still zero and carry is nonzero. Complete settlement produces
28 positive claims, exactly funded source credit, and two independently checked
current health certificates. Stock and reservation censuses run at every checked
frontier. Live close, conversion, exact withdrawals and portfolio deletion finish
with zero vault/capital/insurance stocks and no materialized portfolios.

## Validation

| Check | Result |
| --- | --- |
| New selector | 1/1 PASS; eight worlds, 48 rollbacks, 48 reset/ranked continuations, 16.83 s |
| Original Hybrid capacity/carry | PASS; eight worlds, 16 rollbacks, peak 870,754 CU |
| Active-leg admission control | PASS; 16 worlds, 32 increases, peak 979,601 CU |
| Fourteen-asset Hybrid/source/backlog control | PASS; peak 1,222,978 CU |
| Three-control command | 3/3 PASS, 33.69 s |
| INV-079 metadata/source and trace guards | 16/16 PASS, 1.42 s |
| Scoped rustfmt, whitespace and protected-file diff | PASS |

New selector maxima: trade **870,754**, crank **580,684**, conversion **712,288**,
withdrawal **49,454**, close **26,540**, failed-prefix transaction **592,322** CU.
All measured transactions remain below their inherited bounds. The largest
measured new crank/rollback packet is **629 bytes**, below 1,232 bytes.
The inherited history counter records **1,072 accepted calls**; initialization,
configuration and initial report catch-up are not all included in that counter.

The first development run failed at the crank with `OracleInvalid` because the
fixture changed the price under an unchanged Pyth publication timestamp. Advancing
Unix publication time from 1,001 to 1,002 within the same slot makes the report
admissible. This is a fixture correction, not a parent-red/head-green production
finding. That run is retained in `/dev/shm/percolator-lane19-20260916-new.log`.
Successful logs end in `-new-retry.log`, `-controls.log` and `-guards.log` under
the same `/dev/shm/percolator-lane19-20260916` prefix.

Exact isolation/build commands, from the private clone after cloning:

```sh
git clone --no-hardlinks --branch codex/astra-invariant-cycle-20260915 /tmp/percolator-astra-invariant-cycle-20260915-run /tmp/percolator-lane19-source-capacity-20260916
cd /tmp/percolator-lane19-source-capacity-20260916
git switch -c codex/lane19-source-capacity-20260916
env CARGO_TARGET_DIR=/dev/shm/percolator-lane19-20260916-sbf CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-lane19-20260916-sbf/deploy -- --locked > /dev/shm/percolator-lane19-20260916-sbf.log 2>&1
env CARGO_TARGET_DIR=/dev/shm/percolator-lane19-20260916-matcher CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-lane19-20260916-matcher/deploy -- --locked > /dev/shm/percolator-lane19-20260916-matcher.log 2>&1
cp -a /dev/shm/lane16-20260915-host /dev/shm/percolator-lane19-20260916-host
env CARGO_TARGET_DIR=/dev/shm/percolator-lane19-20260916-host cargo clean -p percolator-prog
mkdir -p tests/fixtures/auth_matcher/target/deploy
cp /dev/shm/percolator-lane19-20260916-matcher/deploy/auth_matcher.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
sha256sum /dev/shm/percolator-lane19-20260916-sbf/deploy/percolator_prog.so /dev/shm/percolator-lane19-20260916-matcher/deploy/auth_matcher.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
```

Exact test commands with their common `env` assignments factored into exports:

```sh
export CARGO_TARGET_DIR=/dev/shm/percolator-lane19-20260916-host CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-lane19-20260916-sbf/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu v16_program_last_latent_hybrid_domain_survives_reversal_and_chunked_backlog -- --nocapture --test-threads=1 > /dev/shm/percolator-lane19-20260916-new-retry.log 2>&1
cargo test --locked --offline --test v16_cu -- v16_program_historical_capacity_preserves_hybrid_carry_health_and_owner_entitlement v16_program_active_leg_increases_preserve_latent_and_full_domain_owner_exit v16_program_max_source_hybrid_backlog_has_bounded_public_exit --nocapture --test-threads=3 > /dev/shm/percolator-lane19-20260916-controls.log 2>&1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence:: -- --skip v16_program_fixed_blockers_remain_progressing --nocapture --test-threads=3 > /dev/shm/percolator-lane19-20260916-guards.log 2>&1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_028_hybrid_capacity_carry.rs tests/invariants/cu/inv_028_hybrid_latent_backlog.rs
git diff --check
git diff ef1647f22b196b01d55af0ab88f89c51b0bef61a --exit-code -- src Cargo.toml Cargo.lock tests/fixtures tests/invariants/open_findings.tsv tests/invariants/invariant_status.tsv tests/invariants/coverage_reopenings.tsv
```

The broader INV-079 fixed-blocker runtime campaign is explicitly skipped; the
selected 16 include all metadata/source guards plus three trace/classifier tests.
No full suite, engine proof campaign or unrelated selector was run.

## Remaining Limits

Fourteen configured assets, thirteen detached historical assets, one active
Hybrid leg, one feed, integral positions, fully funded unliened sources and zero
fees/funding are the only tested shape. Public source-capacity rejection, another
resource class, 14 simultaneously active Hybrid legs, 5,782 market slots,
16 hints, distinct-feed fanout, liens, backing expiry, liquidation, ADL/reset,
Recovery, generation reuse, terminal receipts and slab retirement are not added.
Owners cooperate in the reversal, final reduction, conversion and withdrawals.
Keeper-only catch-up does not certify a fully permissionless owner exit. The two
fixed cadences are not arbitrary history or schedule equivalence.

Changed files are the existing Hybrid owner (module mount only), the new test
owner, and this report. Production, Cargo manifests/locks, fixture sources,
`open_findings.tsv`, `invariant_status.tsv` and `coverage_reopenings.tsv` remain
unchanged. No generic invariant is closed or status promoted.
