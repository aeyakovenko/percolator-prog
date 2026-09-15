# Lane 11: Funded Oracle Containment

## Provenance and scope

- Freshly fetched base: `origin/codex/astra-invariant-cycle-20260915`,
  `2fca9fdfc2a31353f2b000e4f97fb956305982cc`.
- Isolated clone: `/home/anatoly/lane11-funded-oracle-containment-20260915`.
- Local branch: `codex/lane11-funded-oracle-containment-20260915`.
- Evidence: `scripts/loop.md`, this directory's README, `open_findings.tsv`,
  `invariant_status.tsv`, and existing invariant owners. No GitHub PR diffs or
  bodies were consulted. No writes or Git mutations targeted the supplied checkout.
- Changed executable owner: `cu/inv_005_consumed_backing_containment.rs`.
  The existing module mount is sufficient. The other changes are this report and
  a short README section. Production, dependency manifests/locks, and machine
  status TSVs are unchanged.

## Gap and independent oracle

The generated funded-role owner already interleaves oracle/backing/insurance
round trips over fresh reserves with no positions or provider earnings. The
cold-oracle owners already exercise replacement with fresh backing or insurance
budgets. The consumed-backing owner independently creates a 5,000-atom consumed
receivable through public trading, margin use, close, release and PnL conversion;
it repays all principal before cold-admin management. Its original oracle holder
was separate from the backing provider throughout.

The missing intersection is **oracle/provider overlap at the last-fee boundary
with consumed backing still attributed after expiry**. This differs from the
impaired-backing owner's still-impaired stock and Lane 1's retained single-CPI
close consent. The extension reuses the consumed owner's construction and value
oracle instead of adding another fixture or changing an active lane's owner.

The finite product is two source sides x Active/DrainOnly x cold-admin/market-admin
oracle successor x handoff before/after the last earned atom: sixteen worlds.
The original four-world selector remains as a control. All economic state comes
from public System/SPL/ATA/wrapper instructions. Clock advancement is the existing
LiteSVM environment mechanism; no market, portfolio or custody bytes are injected.

The expected amounts come from fixture inputs, not measured payout deltas:

| Quantity | Long source domain | Short source domain |
| --- | ---: | ---: |
| Backing principal repaid | 100,000 | 100,000 |
| Consumed receivable | 5,000 | 5,000 |
| Utilization earnings, ceiling at 3,333 bps | 708 | 875 |
| Winner capital withdrawal | 56,794 | 56,627 |
| Counterparty capital withdrawal | 1,995,000 | 1,995,000 |
| Provider total SPL payment | 100,708 | 100,875 |
| Cold-admin and market-admin SPL payments | 0 | 0 |

Both side formulas use the existing 52,502 winner deposit, 2,000,000 counterparty
deposit and 5,000 profit. Every fresh, valid-liened and impaired bucket term is
zero at management; the other source buckets are zero. The final earned atom
leaves only `consumed_liened_backing_num` protecting the backing role.

## Assertions and bounded result

- INV-005/020: correctly signed cold-admin oracle succession and same-price
  authenticated reports preserve the complete economic state and every other
  profile field. The committed handoff has no provider signature. The old oracle
  is rejected at the current epoch; returning its key advances the authority and
  observation counters without reviving old fee or observation instructions.
- INV-024/036: a completed last-fee or user-capital SPL prefix followed by a
  funded-role takeover rejects at the exact instruction/error and restores all
  accounts, ledger bytes, profile and counters. The fee ledger remains attributed
  to the original provider, including after a later consented backing transfer.
- INV-027/055: expiry and Active/DrainOnly do not erase the consumed receivable.
  The existing stale-deadline payout rejection and fresh-report recovery remain.
  Both users withdraw their exact remaining capital after management; the final
  wrapper and actual SPL vault balances are zero.
- Account checks cover program ownership of market, portfolios and ledger;
  SPL program ownership, token owner, mint, delegate and close authority; mint
  supply and revoked mint authority; and all tracked signer accounts. Rejections
  compare complete Accounts with only the exact network fee deducted from the
  separate payer. Successful writes preserve lamports/rent, account ownership,
  executable flag, rent epoch and length; SPL writes change only the amount.

New-selector totals: sixteen worlds, 112 exact rejections, 80 successful-SPL-prefix
rollbacks, 32 accepted oracle handoffs, 32 accepted same-price observations,
sixteen final fee payments, sixteen consented backing transfers and 32 owner
withdrawals. Forty-eight rejections and all oracle round trips are new suffixes;
the remaining checks reuse the existing invariant history in the new role worlds.
Peak measured rejection CU was 432,476; peak management/payout CU was 231,470;
peak owner withdrawal CU was 143,263. The unchanged limits are 600,000 for bundles
and 300,000 for single custody calls. Setup trade CU is not included in those
three reported peaks.

No violation was observed, so there is no red production regression or fix.
Row **416 remains OPEN** and its benchmark evidence remains `missing`.
INV-005 stays `REFUTED_CURRENT`; INV-020/024/027/036/055 retain their existing
`OPEN_EVIDENCE` dispositions. This is sampled conformance, not row closure.

## Validation commands

Commands run from the isolated clone. Build artifacts and logs are lane-local.
The wrapper was built from this branch with default features and locked engine
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

```sh
env CARGO_TARGET_DIR=/dev/shm/lane11-20260915-sbf \
  RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --sbf-out-dir /dev/shm/lane11-20260915-sbf/deploy -- --locked

env CARGO_TARGET_DIR=/dev/shm/lane11-20260915-matcher CARGO_BUILD_JOBS=8 \
  TMPDIR=/dev/shm \
  RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml \
  --tools-version v1.52 --no-rustup-override --offline \
  --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked

export CARGO_TARGET_DIR=/dev/shm/lane11-20260915-host
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=8
export TMPDIR=/dev/shm
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane11-20260915-sbf/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
cargo test --locked --offline --test v16_cu consumed_backing -- --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=2 \
  inv_005_authority_incarnation_binding::v16_program_funded_role_guard_and_oracle_handoff_are_source_complete \
  inv_005_authority_incarnation_binding::funded_oracle_succession::cold_oracle_funded_containment::v16_program_cold_oracle_replacement_preserves_funded_coholder_and_atomic_prefix \
  inv_005_authority_incarnation_binding::funded_oracle_succession::cold_oracle_insurance_containment::v16_program_cold_oracle_replacement_preserves_insurance_funded_coholder \
  inv_005_authority_incarnation_binding::impaired_backing_containment::v16_program_cold_admin_cannot_seize_impaired_backing_only_role
cargo test --locked --offline --test v16_program_fuzz_regressions \
  inv_079_public_reachability_evidence -- --nocapture --test-threads=2
rustfmt --edition 2021 --config skip_children=true --check \
  tests/invariants/cu/inv_005_consumed_backing_containment.rs
cargo fmt --all -- --check
git diff --check
git diff --exit-code 2fca9fdf -- src Cargo.toml Cargo.lock \
  tests/invariants/open_findings.tsv tests/invariants/invariant_status.tsv
sha256sum "$PERCOLATOR_FUZZ_SBF"
```

Wrapper SHA-256:
`e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.

The `consumed_backing` filter selects the two INV-005 selectors plus the existing
INV-028 consumed-backing-cap selector: **3/3 passed** in 11.93 seconds. The four
explicit adjacent controls passed **4/4** in 2.78 seconds. Logs are
`/dev/shm/lane11-consumed.log` and `/dev/shm/lane11-controls.log`.

The first INV-079 run passed thirteen metadata/source checks and failed three
trace checks because the isolated clone had no authenticated matcher SBF. The
fixture was then built from the local branch before the final rerun.
The complete INV-079 module then passed **17/17** in 3.48 seconds, including
metadata/source guards, trace classifiers and fixed-blocker progress. Log:
`/dev/shm/lane11-metadata-final.log`. Touched-file rustfmt, `git diff --check`,
and the production/dependency/machine-status diff against the base all passed.

Full-repo `cargo fmt --all -- --check` reports existing formatting differences
in six untouched owners: `inv_024_terminal_reserve_destination_recovery.rs`,
`inv_070_native_residue_disposition.rs`, `inv_073_dual_quote_reserve_progress.rs`,
`inv_073_frozen_reserve_replacement.rs`, `inv_073_native_recredit_custody.rs`, and
`inv_073_terminal_public_reserves.rs`. Those files are identical to the fetched
base and were left unchanged. Log: `/dev/shm/lane11-format-all.log`.

## Remaining gaps

This product uses asset 0, classic SPL custody, one consumed source, no remaining
positions at authority rotation, same-price AuthMark refreshes, and no nonzero
insurance budget. Hybrid/external reports, mixed fresh/valid/impaired stock,
simultaneously funded insurance roles, policy changes, retained signed transaction
envelopes, native/secondary rails, open-position rotation, Recovery/Resolved,
maximum shape and arbitrary role/history words remain outside this increment.
Withdrawals complete economic exit; portfolio deletion and slab retirement are
not exercised here. These limits preclude promoting the aggregate invariant
statuses or treating row 416's broad finding as covered.
