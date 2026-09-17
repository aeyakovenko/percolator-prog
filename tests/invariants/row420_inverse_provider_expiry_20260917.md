# Row 420 / INV-073: inverse provider expiry ordering

Base: freshly fetched `origin/main`, `10214b9a576ea5ae48a3d011fab22c3ec2a5536f`.
Private worktree: `/dev/shm/astra-ultra-row420-20260917`.
Branch: `astra-ultra/row420-inverse-expiry-20260917`. Local commit only.
One new public LiteSVM selector; Row 420 remains OPEN.

## Non-duplicate continuation

[The existing distinct-provider note](row420_distinct_provider_expiry_20260917.md)
explicitly leaves inverse scanner ordering open: its expired domain precedes the
fresh domain. The new selector in
[the same test file](cu/inv_073_distinct_provider_disposition.rs) instead funds
domain 1 through slot 200 and domain 3 through slot 100. Both provider keys are
dropped immediately after public funding. Independent public trades earn 875 and
1,749 atoms; unsigned senior payouts and owner-signed portfolio deletion complete
before advancing the authenticated Clock to slot 100.

Both earned-fee payment orders exercise the following continuation:

1. A fee payment followed by administrative `CloseSlab` rejects with
   `EngineLockActive`: the earlier domain still has fresh principal. Complete
   compiled/tracked Accounts restore the transferred tokens and lazy ledger,
   with only the exact transaction fee charged.
2. A keeper-only 100,000-atom principal payment followed by `CloseSlab` reaches
   the later domain's expiry normalization in the same transaction. A final
   one-atom expired principal withdrawal rejects with `EngineStale`, restoring
   both successful instructions, custody and all other tracked Accounts.
3. The identical principal instruction commits separately without a provider
   signature. The later domain's complete bucket/source remain unchanged.
   A normalization/fee/stale-principal bundle also rolls back exactly. The same
   `CloseSlab` then commits normalization, with cursor zero, unchanged earlier
   bucket/source and both earned-fee claims intact.
4. Keeper-only fee payouts reuse the failed fee instruction and settle both
   providers. A partial second fee payment followed by premature closure rejects
   exactly, protecting its last unpaid atom. Identical payout retry and the final
   atom succeed; administrative slab closure then completes.

The input-derived oracle checks principal separately from earned fees, provider
identity and domain, both ledger withdrawal totals, complete SPL Account images
(including lamports), fixed mint supply, stock/encumbrance censuses and terminal
progress. Each world ends with provider custody `[100875, 1749]` and senior user
custody `[56627, 1995000, 55753, 1995000]`. The final close burns exactly 100,000
expired principal atoms, reducing supply from 4,305,004 to 4,205,004; it preserves
paid custody/ledgers and refunds market/vault rent less exact tombstone rent.
Across both worlds: ten public user calls, eight reserve payments, eight exact
successful-prefix rollbacks, two normalization commits and two slab closures.

Also reviewed for overlap:

- [Shared-provider cleanup](row420_shared_provider_cleanup_20260917.md) carries
  spent live payments and a shared user/provider wallet through fresh cleanup;
  it does not exercise inverse expiry with distinct fee claimants.
- [Native earned-fee health](row420433_health_20260917.md) owns native/SPL repair
  and a shared ledger; no rail or missing-wallet matrix is repeated here.
- [Mixed maturity](cu/inv_070_mixed_maturity_terminal_residue.rs) has zero earned
  fees and cooperative principal withdrawal. It does not own these absent,
  independently paid fee claimants through the scanner gate and suffix rollback.

All economic state comes from public wrapper, System and SPL instructions with
normal account construction. Expected Account images are assertion-only and are
never installed. No implementation mismatch was found. Missing custody,
recredit/Recovery, receipts, native quotes, more assets/providers, maximum shapes
and arbitrary histories remain outside this increment. Owner deletion and
administrator normalization/retirement retain their signers. Excluded rows,
production, Cargo, fixtures, shared helpers, test mounts and TSVs are unchanged.

## Exact validation

Private host/SBF targets were copied with `cp -a --reflink=auto` from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target` to
`/dev/shm/astra-ultra-row420-20260917-{host,sbf}-target`. Default-feature SBF was
rebuilt from this worktree, locked/offline with platform-tools v1.52 and engine
`4db11a8cb0053815e23a35d3a7d3edc265d8d866` (7.92s). SBF SHA-256:
`a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row420-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-ultra-row420-20260917-sbf-target/deploy/percolator_prog.so
CARGO_TARGET_DIR=/dev/shm/astra-ultra-row420-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/astra-ultra-row420-20260917-sbf-target/deploy -- --locked
N=inv_024_attributed_quote_value_conservation::terminal_earnings_succession::distinct_provider_disposition::v16_program_distinct_absent_provider_inverse_expiry_unblocks_scan_without_losing_fees
E=inv_024_attributed_quote_value_conservation::terminal_earnings_succession::distinct_provider_disposition::v16_program_distinct_absent_provider_expiry_preserves_sibling_principal_and_both_fees
F=inv_073_no_permanent_user_lock::v16_program_distinct_absent_providers_preserve_each_others_terminal_fee_claims
cargo test --locked --offline --test v16_cu "$N" -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 "$E" "$F"
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_073_distinct_provider_disposition.rs
git diff --check
git diff --exit-code 10214b9a -- src Cargo.toml Cargo.lock tests/fixtures tests/support tests/v16_cu.rs kani tests/invariants/kani ':(glob)**/*.tsv'
git diff --exit-code 10214b9a -- . ':!tests/invariants/cu/inv_073_distinct_provider_disposition.rs' ':!tests/invariants/README.md' ':!tests/invariants/row420_inverse_provider_expiry_20260917.md'
git diff --cached --check
git show --format= --check HEAD
```

| Exact selector | Result | Peak CU |
| --- | --- | --- |
| N: new inverse expiry | PASS, two histories | 476,178 |
| E: existing earlier-domain expiry | PASS, two histories | 498,627 |
| F: existing fresh control | PASS, two histories | 252,765 |

N's normalization/closure peaks are 29,615 / 31,597 CU. Its fee/normalization/
expired-principal rejection peaks at 476,178 CU. All measured continuation
transactions stay below the unchanged 600,000-CU and 1,232-byte limits. Initial
funding/trades and owner deletion are outside these peaks. No broad suite ran.
Logs are outside the worktree at `/dev/shm/astra-ultra-row420-20260917-{sbf,inverse,controls}.log`.
Scoped rustfmt, whitespace, protected-path and exact three-file allowlist checks
pass, including committed `git show --check`.
