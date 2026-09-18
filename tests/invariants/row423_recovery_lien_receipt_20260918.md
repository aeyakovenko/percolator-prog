# Row 423 Recovery liens and partial receipts, 2026-09-18

Base: current fetched `origin/main`, `17b8ca8dc1a76f778c98df92f0b51968e9146bd4`.
Worktree: `/dev/shm/percolator-row423-receipt-recovery-capacity-20260918`.
Branch: `codex/row423-receipt-recovery-capacity-20260918`; local commit only.

## Increment and duplicate review

One new INV-028 LiteSVM selector reuses the existing `ReservedExit` and
`funded_history` helpers unchanged. The existing reserved-loss test body becomes
a local runner; its four original control histories and terminal-call bound remain.
Two new histories cross single/batch admission and one/two loss settlements with
full or split Recovery force-close. Public trades retain 16 historical domains;
admission reserves an 18-domain historical/future resource union out of 28 supported.
The risk owner withdraws all senior capital before admitting 25 lots at price 100.
Provider surplus leaves before a ten-atom adverse mark burns exactly 250 claim atoms.

Public shutdown preserves both complete portfolio Accounts. After the five-slot
timeout, keeper-only `ForceCloseAbandonedAsset` removes 25 lots in one call or
10 then 15 lots. Every step checks exact paired OI and signed positions, unchanged
booked claims and SPL custody, residual/loss accounting, and the shared stock,
reservation and source-rate censuses. The partial step retains nonzero risk backing.

Global resolution precedes obsolete-lien cleanup. The junior's 250-face receipt
receives `floor(250 * 250 / (2750 + 250)) = 20` atoms while the absent peer retains
13 historical liens totaling 2,500 atoms. An early top-up is a complete economic
Account no-op. Each peer `CloseResolved` strictly decreases the tuple of lien debt,
occupied sources and unpaid value, within the original 30-call per-owner bound.
The complete junior portfolio/receipt and wallet remain unchanged throughout.
After source realization, a keeper top-up pays exactly 230 more atoms. A rejected
withdrawal suffix rolls back that successful payout; its unchanged retry pays once,
and replay is an exact economic no-op. Both owners delete their portfolios before
the remaining provider principal is returned through public withdrawals.

Final owner wallets contain 1,002,750 / 997,250 atoms; the provider recovers all
6,400 contributed atoms. Vault, capital, insurance, live claims, live reservations
and materialized portfolio count finish at zero. Mint supply remains 2,006,400.
Historical consumed-backing/provider-receivable labels remain and equal the
original claims; their retirement is outside this test.

Existing evidence differs:

- The same file's original loss/receipt selector closes by bilateral trade and
  releases all liens and provider principal before resolution and receipt creation.
- `inv_028_recovery_latent_capacity.rs` materializes the last of 28 source slots
  during Recovery, with no historical liens or underfunded partial receipt.
- `inv_067_receipt_recovery_forfeit.rs` combines discarded Recovery gain with an
  older receipt, without this claim-funded admission and simultaneous lien cleanup.
- `row423_full_table_receipt_20260918.md` overlaps a partial receipt with 28 peer
  source claims, without liens or Recovery. The existing all-source lien control
  owns the 28-lien dimension separately.

Thus the increment is the observed overlap and bounded cleanup of Recovery-origin
liens while a receipt remains partially paid. Row 423 and invariant statuses remain
OPEN/unchanged. Full 28-domain simultaneous liens/receipts/Recovery, maximum active
legs/feeds, native quote, fractional funding/fees and arbitrary histories are excluded.
Owner signatures are still required for terminal closes and portfolio deletion;
only Recovery force-close and receipt top-up are keeper-only.

## Exact validation

The default-feature wrapper was rebuilt offline with locked dependencies and
platform-tools v1.52 in a private copy of the existing SBF cache. Host dependencies
also use a private cache. The unchanged history helper requires its fixed matcher
artifact path, so validation used a byte-identical cached matcher binary there;
no fixture source or tracked fixture file was edited.

- Engine pin: `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
- Wrapper SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
- Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/row423-recovery-20260918-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/row423-recovery-20260918-sbf-target/deploy/percolator_prog.so
C=inv_028_source_domain_realizability_cap::historical_latent_capacity::exit_resource_reservation::reserved_loss_exit
cargo test --locked --offline --test v16_cu "$C::v16_program_recovery_force_close_preserves_historical_liens_and_partial_receipt_exit" -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu "$C::v16_program_admission_preserves_backing_for_loss_and_bounded_resolved_exit" -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_028_reserved_loss_exit.rs
git diff --check
git diff --cached --check
git show --format= --check HEAD
```

| Exact selector suffix under `C` | Worlds | Recorded calls | Exact rollbacks | Peak CU |
| --- | ---: | ---: | ---: | ---: |
| `v16_program_recovery_force_close_preserves_historical_liens_and_partial_receipt_exit` | 2 | 154 | 6 | 1,003,224 |
| `v16_program_admission_preserves_backing_for_loss_and_bounded_resolved_exit` | 4 | 298 | 10 | 1,003,202 |

New history totals: three force-closes, 58 terminal calls including two positive
top-ups, two partially paid receipts; maximum force-close 652,021 CU and maximum
recorded transaction 652 bytes. The unchanged 1,400,000 CU limit applies. Peaks
include measured suffix successes and expected rejections; initial history,
provider funding and lifecycle configuration are excluded. No broad suite was run.
Logs: `/dev/shm/row423-recovery-20260918-{sbf,host,new,control}.log`.

The three-file allowlist guard covers the Rust owner, this note and invariant README,
including untracked files and the committed diff. Production, Cargo files, fixtures,
TSVs, support and global harness files have no tracked changes. Targeted rustfmt and
diff/commit whitespace checks pass.
