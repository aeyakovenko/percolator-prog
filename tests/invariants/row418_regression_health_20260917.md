# Row 418 native terminal regression health, 2026-09-17

Base: freshly fetched `origin/main`, `248caaba4312c3818d4da83943932d19bd95960d`.
Worktree: `/dev/shm/percolator-row418-health-20260917`.
Branch: `row418-health-20260917`; local commit only, no push.

Read: [README](README.md), [terminal-reserve audit](terminal_reserve_evidence_audit_20260917.md),
[Scope O](astra_scope_o_native_booked_residue_cleanup_20260914.md), row-418
prefunded/native-custody witnesses, current wrapper and pinned engine. The
[reserve health report](row420421423433_regression_health_20260917.md) independently
records the shared setup failure below. No external PR implementation was used.

## Added coverage

The new [prefunded native retirement selector](cu/inv_070_native_booked_prefunded_retry.rs)
is mounted as `native_booked_residue_cleanup::prefunded_retry`. It reuses Scope O's
transaction/account-frame oracle and native token-image helper, with an independent
public setup that has no portfolios, PnL, earned fees or spent insurance.

Two histories cross prefunding below and above ATA rent. A public 37-atom insurance
deposit is paid unsigned after resolution. The insurer voluntarily redeems that
wSOL, drains its wallet to a tracked departure account and leaves. Administrator
backing of 101 atoms expires into claim-free booked stock. Before retirement, the
absent insurer's canonical ATA address receives System lamports, either empty-account
rent plus 23 or token-account rent plus 23. The vault separately receives a synced
19-lamport donation. The insurer key is dropped before prefunding, expiry or repair.

The input-derived terminal partition is:

```text
prior redeemed insurance = 37, unchanged at the departure account
canonical insurance wSOL = 101 booked residue + max(prefund - token rent, 0)
administrator wSOL = 19 external vault donation
keeper ATA funding = max(token rent - prefund, 0)
administrator SOL refund = slab lamports + vault rent - tombstone rent
native mint Account = unchanged
```

Each history checks three exact rollbacks: a successful insurance payment followed
by a stale-epoch close; successful ATA repair followed by a redirected residue
destination; and successful repair plus full retirement followed by an unsigned
administrative suffix. The existing helper checks the exact error/index and
successful ATA/wrapper prefix logs, complete compiled/tracked Account rollback,
and exact payer signature fees. Failed repair restores the prefunded System Account,
not an absent account. The unchanged repair/close prefix then commits. Idempotent
repair after retirement cannot charge rent or move value again.

Full token images, stock/encumbrance censuses, exact authority-epoch consumption,
unchanged paid custody, absent insurer/vault, lamport conservation and typed
tombstone/rent assertions bind the result. The packet limit remains 1,232 bytes;
every measured continuation is checked against Scope O's 300,000-CU ceiling.

Novelty: Scope O repairs an absent, unfunded ATA after fee/recredit completion;
the existing prefunded-quote witness repairs user payout custody and closes with
zero booked residue. This test exercises prefunded canonical **insurance** custody
and nonzero native booked retirement together, including rollback of initialization,
native wrapping, token transfer, vault closure and slab tombstone writes.

## Current health and limits

The unchanged Scope O native and classic selectors both fail in
`cu/inv_024_terminal_recredit_fee_partition.rs:231`, during setup `CloseResolved`:
`InstructionError(2, Custom(22))` (`EngineNonProgress`), 12,123 transaction CU.
Neither reaches its reserve/recredit or retirement suffix. This is consistent with
current main's retained-terminal-receipt semantics: repeating `CloseResolved`
does not complete this fixture while Fresh backing remains. Its later old
control-sequence assertion also remains unvalidated. One stalled call is not
evidence that every public continuation fails; no new production bug is claimed.

An initial attempt to extend that shared fixture stopped at the same error and
was removed. The final independent fixture does not repair or certify it. Its
first draft incorrectly classified expired backing as insurance; engine source
and the existing input book show that it is unallocated booked stock until final
retirement. That test-oracle correction is included; production is unchanged.

INV-018/024/070/080/081 receive bounded canonical-custody, value-partition,
retirement, rollback and success-state evidence. INV-073/078 receive only this
finite reconstructible-custody continuation: administrator expiry/close, initial
insurer funding/redemption and a rent-funded keeper remain prerequisites. There
is no new general recovery or arbitrary funded-state liveness proof.

Open: fee/loss/recredit fixture repair and independent revalidation of its claims;
partial recredit with native residue; dual-quote nonzero native residue; active
receipts, earned fees and source liabilities; absent administrator; arbitrary
custody/recovery histories and maximum shapes. Row 418 remains COVERED in the
existing ledger, with this execution-health qualification. No status TSV changes.

## Reproduction and results

Private host/SBF targets were copied with `cp -a --reflink=auto` from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target` to
`/dev/shm/percolator-row418-health-20260917-{host,sbf}-target`.
The current default-feature SBF was rebuilt locked/offline using platform-tools
v1.52 and engine `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
No matcher or tracked fixture changes were needed.

Commands from the worktree; exports spell out the environment supplied via `env`:

```bash
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
export CARGO_TARGET_DIR=/dev/shm/percolator-row418-health-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row418-health-20260917-sbf-target/deploy/percolator_prog.so
env CARGO_TARGET_DIR=/dev/shm/percolator-row418-health-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_NET_OFFLINE=true CARGO_BUILD_JOBS=2 cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row418-health-20260917-sbf-target/deploy -- --locked
family=inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::terminal_progress_product::native_booked_residue_cleanup
new="$family::prefunded_retry::v16_program_native_booked_residue_prefunded_custody_retry_preserves_value"
native="$family::v16_program_native_booked_residue_escheats_after_fee_recredit_completion"
classic="$family::v16_program_classic_booked_residue_burn_control_preserves_paid_claims"
cargo test --locked --offline --test v16_cu "$new" -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 "$native" "$classic"
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_070_native_booked_residue_cleanup.rs tests/invariants/cu/inv_070_native_booked_prefunded_retry.rs
git diff --check
git diff --exit-code 248caaba4312c3818d4da83943932d19bd95960d -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)**/*.tsv'
git diff --exit-code 248caaba4312c3818d4da83943932d19bd95960d -- . ':!tests/invariants/README.md' ':!tests/invariants/row418_regression_health_20260917.md' ':!tests/invariants/cu/inv_070_native_booked_residue_cleanup.rs' ':!tests/invariants/cu/inv_070_native_booked_prefunded_retry.rs'
git diff --cached --check
git show --format= --check HEAD
```

Logs are `/dev/shm/percolator-row418-health-20260917-*.log`.

| Log | Result |
| --- | --- |
| `sbf-build` | PASS; release build 7.79s |
| `prefunded-final` on merged main | PASS; 1 passed, 1,444 filtered, 0.79s; two histories, six rollbacks, two retirements; peaks 64,291/68,491 CU |
| `legacy-final` | FAIL; 0 passed, 2 failed, 1,442 filtered, 2.02s; both existing selectors stop at shared setup `EngineNonProgress`; no retirement assertions reached |

Only these three final exact selectors ran, plus diagnostic iterations of the new
selector and native baseline. No broad suite or metadata census ran. Existing
Solana future-compatibility warnings remain. Targeted formatting, whitespace,
production/Cargo/fixture/all-TSV guards and the four-file allowlist pass. Staged
whitespace and `git show --format= --check HEAD` pass on the local commit.
No row428 or row425/426 file is modified.
