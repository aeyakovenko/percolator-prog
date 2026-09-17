# Row 420 / INV-024/073: one provider, two asset ledgers

Base: local `origin/main`, `e250138435490c115d87c8edd9500f331fa13131`.
Worktree: `/dev/shm/astra-ultra-row420-provider-conformance-20260918`.
Branch: `astra-ultra-row420-provider-conformance-20260918`.
One new public LiteSVM selector; tests/docs only, local commit, no push.
Row 420 remains OPEN.

## Non-duplicate dimension

The new selector in [the provider fixture](cu/inv_073_distinct_provider_disposition.rs)
uses one absent provider for assets 0 and 1, domains 1 and 3. Both domains pay the
same SPL token Account, but each earnings ledger must bind its own domain even
though market and authority identities agree. Public System/SPL/wrapper calls
construct all economic state. Both copies of the provider key are dropped after
funding, before fee generation and user settlement. Expected Account images are
assertions only; they are never installed into LiteSVM.

The existing two-asset fixture is reused without adding an expiry/repair product.
The required distinct-provider and inverse-expiry notes own different provider
identities; neither isolates ledger domain identity with a common authority and
destination. Shared-provider cleanup owns one domain plus a user/provider role
overlap. Native earned-fee custody owns two quote rails sharing one domain ledger.
Generated missing wallets, keeper ledger handoff and replenished-provider progress
also lack this two-domain ledger binding. INV-088's earnings-order witness ends
after signed Live withdrawals, without this absent-provider terminal continuation.
Those existing dimensions are not added again. The regression-health note's
broken recredit products are not modified or relied on.

## Public continuation

Two independent trade cohorts earn input-derived fees of 875 and 1,749 atoms.
Bounded unsigned senior payouts total `[56627, 1995000, 55753, 1995000]`; owners
delete their empty portfolios. Both ledger payment orders then run:

1. Pay 17 fee atoms in the first domain, then request 19 sibling atoms using the
   first domain's ledger. The first wrapper/SPL payment succeeds before the
   sibling rejects with `Unauthorized` at instruction 3. Complete compiled and
   tracked Accounts restore, including lazy ledger initialization and token
   custody, except the exact keeper transaction fee. Retry the unchanged first
   instruction with the sibling's correct ledger; both payments commit unsigned.
2. Repeat with a 23-atom first payment and 19-atom sibling payment. This time both
   ledgers already exist, so rollback preserves previously paid custody and the
   entire populated ledger Accounts. The same prefix and corrected sibling retry.
3. Pay the first domain's entire 100,000 principal atoms, then attempt one more
   atom from that domain. `EngineLockActive` at instruction 3 restores the payment
   despite sufficient common vault liquidity from the sibling principal and fees.
   Reuse the captured principal instruction, then pay sibling principal. Each
   successful principal payment frames the sibling's complete bucket/source.
4. Pay both fee tails. The existing partial-fee/premature-close rollback protects
   the final fee atom, and its identical fee instruction retries. Administrative
   slab closure completes before expiry, preserving custody and both ledgers.

Every boundary independently checks principal and earnings by domain, complete
token images without double-counting the common destination, complete ledger
images, fixed mint supply, market stock and reservation/encumbrance censuses.
The 5,000-atom historical consumed-backing/receivable/spent marker per domain is
bound to `1000 * (105 - 100)` and cannot authorize additional principal. A ledger
initialized after trading records that prior unavailable principal separately
from its zero new-loss/recovery counters and its domain's fee withdrawals.

Both histories end with `200000 principal + 875 + 1749 earnings = 202624` provider
atoms, unchanged senior custody, no burn, and fixed supply 4,305,004. Each ledger
records only its domain's 875/1,749 fee withdrawals. Final closure checks the typed
tombstone and exact market/vault rent refund and preserves complete paid Accounts.
Across both worlds: ten user calls, eighteen reserve payments, eight exact
rollbacks and two slab closures. Setup/trades and portfolio deletion are outside
the measured continuation peak. The existing 600,000-CU assertion and 1,232-byte
transaction limit are unchanged.

This is bounded classic-SPL, two-asset provider conformance. Missing custody,
native quotes, recredit/Recovery, maximum shapes and arbitrary histories remain
outside it. Resolution, owner deletion and slab closure retain their signers.
No claim or change is made for Rows 419/435, 421, 423, 424, 433 or Row 411.

## Reproduction

Private host/SBF caches were copied with `cp -a --reflink=auto` from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target` to this worktree's
corresponding sibling targets. The default-feature SBF was rebuilt from this
worktree with platform-tools v1.52, locked/offline, in 7.85s. Engine:
`4db11a8cb0053815e23a35d3a7d3edc265d8d866`. SBF SHA-256:
`a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row420-provider-conformance-20260918-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-ultra-row420-provider-conformance-20260918-sbf-target/deploy/percolator_prog.so
CARGO_TARGET_DIR=/dev/shm/astra-ultra-row420-provider-conformance-20260918-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/astra-ultra-row420-provider-conformance-20260918-sbf-target/deploy -- --locked
N=inv_024_attributed_quote_value_conservation::terminal_earnings_succession::distinct_provider_disposition::v16_program_absent_multi_asset_provider_rejects_cross_domain_earnings_ledger_and_retries
C=inv_073_no_permanent_user_lock::v16_program_distinct_absent_providers_preserve_each_others_terminal_fee_claims
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 "$N" "$C"
# Final shared-provider-only oracle correction: rerun N only; C's path is unchanged.
cargo test --locked --offline --test v16_cu "$N" -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_073_distinct_provider_disposition.rs
git diff --check
git diff --exit-code e250138435490c115d87c8edd9500f331fa13131 -- src Cargo.toml Cargo.lock tests/fixtures tests/support tests/v16_cu.rs kani tests/invariants/kani ':(glob)**/*.tsv'
git diff --exit-code e250138435490c115d87c8edd9500f331fa13131 -- . ':!tests/invariants/cu/inv_073_distinct_provider_disposition.rs' ':!tests/invariants/README.md' ':!tests/invariants/row420_multi_asset_provider_20260918.md'
git diff --cached --check
git show --format= --check HEAD
```

Only N and its direct fresh-provider control C were executed. The first N passed;
strengthening its complete-ledger image exposed a draft expectation that omitted
the historical 5,000-atom marker. The final input-derived oracle includes it. No
production mismatch was found. Logs and build outputs remain outside the worktree
at `/dev/shm/astra-ultra-row420-provider-conformance-20260918-{sbf,new,final,verified}.log`.

| Exact selector | Result | Peak CU |
| --- | --- | --- |
| N, merged-main verification | PASS, 2 histories | 443,596 |
| C, merged-main verification | PASS, 2 histories | 255,765 |

N's lazy-ledger, populated-ledger and principal-overclaim rollback peaks are
429,237 / 427,671 / 435,991 CU; its final slab-close peak is 23,774 CU. The initial
N run passed at 434,596 CU before the stronger Account oracle. C passed before
the final shared-provider-only oracle correction; that correction does not run
on C's path. No broad suite was run. Scoped rustfmt, whitespace, protected paths,
the exact three-file allowlist and committed whitespace checks pass.
