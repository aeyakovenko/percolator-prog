# Row 420: distinct absent providers across expiry

Base: fetched `origin/main`, `83da50f74b4958e48c6da3ea495f4a37e9612b5d`.
Branch: `codex/row420-terminal-provider-20260917`.
Worktree: `/dev/shm/percolator-row420-terminal-provider-20260917`.
One new selector; local commit only. Row 420 remains OPEN.

## Increment and duplicate analysis

Extend [the existing two-provider witness](cu/inv_073_distinct_provider_disposition.rs)
with two fee-payment orders at staggered expiry. Public trades earn 875 and
1,749 atoms for distinct providers whose keys are dropped immediately after
funding. Permissionless senior payouts and owner-signed portfolio deletion
precede this continuation. At slot 100, provider 0's principal expires while
provider 1's 100,000 atoms remain fresh until slot 200.

Administrative `CloseSlab` normalizes only the expired domain. Both fee claims
and the sibling's complete bucket/source remain unchanged. A bundle containing
normalization and a real fee payment rejects one expired principal atom with
`EngineStale`, restoring every compiled/tracked Account, including lazy ledger
initialization, except exact transaction fees. After normalization commits, a
keeper-only sibling-principal payment plus the same expired withdrawal also
rolls back exactly. The identical valid fee/principal instructions then commit
without either provider signature. The existing premature-close rollback now
protects the last earned-fee atom against burning the expired principal residue.

Both histories finish with provider destinations of 875 / 101,749 atoms, exact
per-provider earnings ledgers, unchanged senior payouts and stock/encumbrance
censuses. Final signed closure burns exactly 100,000 atoms, refunds exact rent
and preserves all paid custody and ledger Accounts. All state comes through
public System/SPL/wrapper instructions; assertion images are never installed.

Reviewed before editing:

- The original distinct-provider selector has two fresh domains throughout.
- [Generated reserve wallets](cu/inv_073_generated_reserve_wallets.rs) covers
  expiry and earnings for one provider domain, without a separately owned fresh
  principal claim sharing custody.
- [Mixed maturity](cu/inv_070_mixed_maturity_terminal_residue.rs) has zero fees
  and cooperative principal withdrawal; [absent-provider expiry](cu/inv_073_absent_provider_expiry_retirement.rs)
  retires principal without these distinct earned-fee claimants.
- [Recent shared-provider cleanup](row420_shared_provider_cleanup_20260917.md)
  carries spent live payouts through cleanup with fresh backing. That history,
  missing-wallet repair and native paid-prefix coverage are not repeated here.

This is bounded earlier-domain expiry evidence. Inverse expiry ordering reaches
the earlier fresh domain's scanner gate and needs its own continuation; it is
not claimed covered. Missing custody, recredit/Recovery, receipts, native rails,
maximum shapes and arbitrary histories remain open. Mechanical deletion,
normalization and retirement retain their existing signers.

## Exact validation

Default-feature private SBF rebuilt from this worktree, locked/offline with
platform-tools v1.52 and engine `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
SBF SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
Private host/SBF targets were copied with `cp -a --reflink=auto` from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target`.

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/percolator-row420-terminal-provider-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row420-terminal-provider-20260917-sbf-target/deploy/percolator_prog.so
CARGO_TARGET_DIR=/dev/shm/percolator-row420-terminal-provider-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row420-terminal-provider-20260917-sbf-target/deploy -- --locked
E=inv_024_attributed_quote_value_conservation::terminal_earnings_succession::distinct_provider_disposition::v16_program_distinct_absent_provider_expiry_preserves_sibling_principal_and_both_fees
C=inv_073_no_permanent_user_lock::v16_program_distinct_absent_providers_preserve_each_others_terminal_fee_claims
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 "$E" "$C"
```

| Exact selector | Result | Peak CU |
| --- | --- | --- |
| E: new expiry continuation | PASS, 2 histories, 8 reserve payments, 6 exact rollbacks, 2 closures | 449,127 |
| C: existing fresh control | PASS, 2 histories, 10 reserve payments, 2 exact rollbacks, 2 closures | 255,765 |

E's normalization/closure peaks are 26,564 / 28,597 CU. The unchanged guardrail
is 600,000 CU, with 1,232-byte packet checks. Initial funding/trades and owner
deletion are outside these measured continuation peaks. The untouched C baseline
also passed (261,765 CU). Final run: 2 passed, 0 failed; no broad suite.
Logs are outside the worktree at `/dev/shm/percolator-row420-terminal-provider-20260917-{sbf,baseline,final}.log`.
Touched-file rustfmt, whitespace, protected-path and exact three-file allowlist
checks pass. Production, Cargo, fixtures, support, test mounts, TSV ledgers and
unrelated invariants are unchanged.
