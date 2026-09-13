# Terminal reserve admission behind a cached prefix

The mounted selector
`inv_071_crank_progress::terminal_reserve_backfill::v16_program_terminal_prefix_blocks_reserve_backfill_across_expiry_and_retries_cleanup`
exercises the deployed default-feature program through public System, SPL, ATA and
Percolator instructions. Four histories cross the two source sides with
authenticated `expiry` and `expiry+1`, each with an `expiry-1` rejection control.
All program-owned and token economic state comes from public instructions;
LiteSVM supplies programs, initial native funding and authenticated Clock changes.

## Trace and Oracle

The fixed mint supply is 188 atoms: 101 user capital, 19 previously withdrawn
provider principal, 13 prospective backing atoms, 17 prospective insurance atoms,
31 later-expiring backing atoms and 7 donor atoms. Mint authority is revoked.
Asset 1 receives and returns its full 19-atom backing deposit while Live. Asset 2
retains 31 Fresh backing atoms until slot 80. Both generations remain active.

Two instructions targeting the emptied domain of asset 1 are retained with exact
current generation, authority epoch and next funding sequence: a 13-atom backing
top-up expiring at slot 1,000 and a 17-atom domain-insurance top-up. Each successfully
simulates while Live, reaches SPL transfer, and leaves every compiled/tracked
Account, including the payer, unchanged. These controls exclude invalid funding
authority, insufficient source balance, stale generation/sequence and an already
elapsed prospective expiry as explanations for the subsequent denials.

At slot 10, resolution and an unsigned owner-payout crank return exactly 101 SPL
atoms. Its `now_slot = u64::MAX` does not advance authenticated engine time. The
owner signs mechanical portfolio deletion; its complete native balance stays
fixed while portfolio rent moves to the slab. A first `CloseSlab` commits cursor
`0 -> 2`, placing the previously funded asset strictly behind the prefix.

Each funding request now rejects after a real 7-atom SPL donation prefix.
Backing admission returns `EngineLockActive`; domain-insurance admission returns
`InvalidInstruction`, the current handler's resolved-mode error. At authenticated
slot 79, donation followed by `CloseSlab` rejects with `EngineLockActive`.
At slot 80 or 81, donation plus `CloseSlab` successfully expires the later backing
before either funding suffix rejects. Logs prove the donation and wrapper prefix
executed. Exact instruction-index assertions exclude compute exhaustion and an
earlier validation error. All compiled and separately tracked Accounts roll back,
including Clock, cursor, engine time, funding sequences, stock, custody and rent;
the only permitted difference is the independently calculated signature fee.

The identical donation/close prefix then commits. Local backing/source ledgers
and the global Fresh summary reconcile to zero while engine custody stays 31 and
SPL custody becomes 38. The earlier asset's complete engine-slot bytes and control
sequences remain exact. A final `CloseSlab` burns 31, sweeps 7 to the administrator's
provider token account, closes the vault and leaves the canonical typed market
tombstone with exact rent. The administrator receives exactly vault rent and slab
excess, including the previously absorbed portfolio rent. All other tracked
endpoints remain exact. Final token supply and holder totals both equal 157.

Expected stocks derive from funding inputs and successful public payouts, not
observed engine deltas. Independent stock and encumbrance censuses run at terminal
checkpoints. A decoded rank `(Fresh bucket count, slots remaining)` descends
`(1, 3) -> (1, 1) -> (0, 1) -> tombstone`; there are exactly three committed slab
calls per history. Every tested continuation, rejection and funding preview is
bounded by 300,000 CU and verifies real transaction signatures/packet size.

## Scope

This increment is net-new against the six excluded coverage families, using their
README descriptions without inspecting their test bodies or any open PR material:

| Existing family | New distinction |
| --- | --- |
| `terminal_prefix_reuse` | No retired-slot reuse: two live-valid reserve funding routes attempt to introduce obligations into an unchanged active generation behind the cursor. |
| `retained_reserve_stock` | Funding admission and cached-prefix stability, rather than principal/earned-fee withdrawal eligibility. |
| `receipt_spend_replay` | No receipts or payout spending; rejected reserve creation composes with terminal expiry. |
| `pending_terminal_fees` | Maintenance is zero; the changing stocks are fresh backing and external surplus. |
| `shutdown_operator_departure` | No shutdown or authority departure; current funded authority and sequence stay usable in the live controls. |
| `terminal_destination_variants` | Canonical destinations stay fixed; the failing suffix is reserve admission. |

INV-063 gains exact/late expiry normalization with a pre-expiry rejection;
INV-069/070 gain bounded disposal after prevented reserve reintroduction;
INV-071 gains a finite rank and no-progress rejection; INV-073 gains a funded
unsigned capital payout followed by separately signed mechanical cleanup;
INV-080 gains complete-Account rollback and identical-prefix retry; INV-086 gains
an input-derived stock/custody reference for four finite deployed histories;
INV-088 gains local recomputation and framing behind a persisted global cursor.

This proves rejection of attempted prefix invalidation. It does not prove a
successful invalidating mutation repairs the cursor. Nonzero claims, receipts,
provider receivables, insurance spending/recredit, maintenance/funding accrual,
absent authorities, other quote rails, maximum-capacity scans and arbitrary
histories remain gaps. INV-073's user payout is permissionless; reserve admission
and administrative slab disposal require their stated signers. No invariant
verdict or holdout label is promoted.

## Reproduction

Base: `adeac5fcacfb431db78c5646c9bdc9f70cf4f0ed`, the requested branch's current
HEAD when work began. Isolated worktree:
`/tmp/percolator-inv-terminal-time-prefix-20260912`; local branch:
`codex/inv-terminal-time-prefix-20260912`. The integration worktree and
`/home/anatoly/percolator-prog` were not edited. No PR branches/diffs/tests were
consulted or copied, and no shared target was reused.

```bash
export CARGO_TARGET_DIR=/dev/shm/inv-terminal-time-prefix-20260912-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export TMPDIR=/dev/shm CARGO_BUILD_JOBS=6
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu inv_071_crank_progress::terminal_reserve_backfill::v16_program_terminal_prefix_blocks_reserve_backfill_across_expiry_and_retries_cleanup -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --exact --nocapture \
  inv_071_crank_progress::terminal_cursor_time::v16_program_persisted_scan_reclassifies_time_without_skipping_live_siblings \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::v16_program_terminal_scan_reconciles_external_surplus_arriving_after_cached_prefix \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::mixed_maturity::v16_program_mixed_maturity_terminal_residue_preserves_partition_and_close_retry \
  inv_088_global_summaries_are_not_account_local_proofs::v16_program_fresh_backing_global_summary_is_exact_in_every_four_domain_touch_order
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
git diff --cached --check
```

Fresh default-feature SBF build: passed; SHA-256
`5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
Selector: **1/1 passed**, four histories, eight previews, twenty exact rejected
transactions, peak **92,143 CU**, 1.64 seconds. Initial fixture assertions were
corrected to model portfolio rent moving to the slab and LiteSVM retaining the
owner on its zero-lamport, zero-length closed portfolio Account; these were test
expectation errors, not public invariant counterexamples.
Nearby controls: **4/4 passed** in 8.50 seconds. Invariant index: **1/1 passed**.
Repository-wide formatting and working/staged whitespace checks passed. Existing
host-test dead-code warnings and the Solana client future-compatibility warning
remain; they did not prevent validation.
