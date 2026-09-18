# Row 418 native-primary booked retirement with two quote rails

Base: freshly fetched `origin/main`, `5594bcf849e72a69dc56b8aa6dfe02ba6c77eb43`.
Private worktree: `/dev/shm/percolator-row418-litesvm-20260918`.
Branch: `codex/row418-litesvm-20260918`. Local commit only; no push.

## Coverage and Ownership

One new INV-070-owned LiteSVM selector extends the existing
[dual-quote native residue runner](cu/inv_070_native_residue_disposition.rs).
The existing selector explicitly skips native-primary expiry; its expired
booked stock is classic SPL and is burned. The single-quote
[prefunded witness](cu/inv_070_native_booked_prefunded_retry.rs) and
[fee/loss/recredit witness](cu/inv_070_native_fee_recredit_conformance.rs) already
own native booked retirement in one vault. This increment exercises native
booked retirement beside a secondary-rail payout and two-vault disposal.
Partial native recredit already has an owner in the existing terminal progress
product and is not duplicated here.

The shared public constructor supplies 401/307 backing principal and 67 insurance
atoms in native primary custody, plus 997 external SPL atoms in secondary custody.
Unsigned native prefixes pay 101/59 principal and 17 insurance. After reserve keys
are dropped, the remaining 300 long-principal atoms pay on the SPL rail; the 248
remaining short-principal atoms expire at slot 100; and the remaining 50 insurance
atoms pay on the native rail. A separate 19-lamport vault donation is synchronized
after the claims finish. The input-derived terminal partition is:

```text
native primary: 775 funded + 19 donated = 160 provider + 315 insurance + 319 admin
insurance custody: 67 paid insurance + 248 expired booked residue = 315
admin native surplus: 300 displaced by the SPL payout + 19 donation = 319
SPL secondary: 997 external = 300 provider + 697 admin
admin SOL refund: slab lamports + both vault rents - tombstone rent
```

The existing transaction oracle checks complete compiled/tracked Account frames,
signature fees, lamport conservation, successful prefix logs, the 1,232-byte packet
bound and the 300,000-CU ceiling. Two payout-prefix rollbacks are retained. A new
third rollback executes the complete native transfer, secondary sweep, both vault
closures and slab tombstone before an unsigned suffix fails at instruction 3
with `ExpectedSigner`. The same retirement instruction then commits.

Stock and reservation censuses, control sequences, native/SPL token images and
market shape are checked through expiry and payments. Final checks bind exact
insurance custody, both unchanged mint Accounts, separate administrator sweeps,
rent refunds, absent vaults and the typed tombstone. Administrator redemption of
its native sweep cannot redeem the insurance recipient's booked residue.

This is one bounded public history: no active portfolios, PnL, earned fees,
insurance spend/recredit, missing custody, absent-admin retirement or maximum
shape claim is added. Row 418 remains COVERED; no TSV or invariant verdict changes.
The existing 36-world selector remains the direct control for the reused runner.

## Reproduction

Private host/SBF targets were copied with `cp -a --reflink=auto` from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target` to the corresponding
`/dev/shm/percolator-row418-litesvm-20260918-{host,sbf}-target` paths.
The current default-feature SBF was rebuilt locked/offline with platform-tools
v1.52 and engine `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
SBF SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.

```bash
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
export CARGO_TARGET_DIR=/dev/shm/percolator-row418-litesvm-20260918-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row418-litesvm-20260918-sbf-target/deploy/percolator_prog.so
env CARGO_TARGET_DIR=/dev/shm/percolator-row418-litesvm-20260918-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_NET_OFFLINE=true cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row418-litesvm-20260918-sbf-target/deploy -- --locked
cargo test --locked --offline --test v16_cu inv_073_no_permanent_user_lock::dual_quote_reserve_progress::native_residue_disposition::v16_program_native_primary_booked_residue_survives_dual_quote_retirement_retry -- --exact --nocapture
cargo test --locked --offline --test v16_cu inv_073_no_permanent_user_lock::dual_quote_reserve_progress::native_residue_disposition::v16_program_native_residue_and_recreated_reserves_reconcile_quote_variant_retirement -- --exact --nocapture
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_070_native_residue_disposition.rs
git diff --check
git diff --exit-code 5594bcf849e72a69dc56b8aa6dfe02ba6c77eb43 -- src Cargo.toml Cargo.lock tests/fixtures tests/support tests/v16_cu.rs ':(glob)**/*.tsv'
git diff --exit-code 5594bcf849e72a69dc56b8aa6dfe02ba6c77eb43 -- . ':!tests/invariants/cu/inv_070_native_residue_disposition.rs' ':!tests/invariants/row418_dual_quote_booked_retirement_20260918.md'
git diff --cached --check
git show --format= --check HEAD
```

## Results

Both exact selectors pass against the rebuilt SBF: each reports **1 passed,
0 failed, 0 ignored, 1,489 filtered**.

| Selector | Histories | Payments | Repairs | Expiries | Rollbacks | Retirements | Peak CU |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| New native-primary booked retirement | 1 | 5 | 0 | 1 | 3 | 1 | 56,650 |
| Existing dual-quote control | 36 | 204 | 36 | 12 | 96 | 36 | 66,135 |

Runtime was 0.42s / 15.10s. Peaks cover instrumented continuations, excluding
initial public fixture construction, and can vary with randomized fixture keys.
Only these two exact selectors ran; no broad suite or history inspection was used.
The existing Solana future-compatibility warning remains.

Targeted rustfmt, `git diff --check`, staged whitespace, protected-path and
two-file allowlist checks pass, as does `git show --format= --check HEAD` on the
local commit. No production, Cargo, fixture, TSV, harness or shared-support changes.
This is a substantive coverage increment, not a no-op or a production bug claim.
