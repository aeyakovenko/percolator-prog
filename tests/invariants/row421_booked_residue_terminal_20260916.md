# Row 421: booked recovery across beneficiary epochs

## Isolation

- Local shared clone: `/dev/shm/row421-booked-residue-terminal-20260916-worker`.
- Branch: `codex/row421-booked-residue-terminal-20260916`.
- Source: `/tmp/percolator-astra-invariant-cycle-20260915-run`.
- Base: `origin/codex/astra-invariant-cycle-20260915` at
  `4d17c967bdc09ad02f4ea0d68693e6998a9d14b9`.
- Protected baseline: the source checkout's `origin/main`,
  `d809e9a563d9b8bf38f32648b32a15d75f526ec8`, imported locally into this clone.
- Private target, TMPDIR and logs: `/dev/shm/row421-booked-residue-20260916-*`.
- No edits to `/home/anatoly/percolator-prog`, no external writes,
  no production/dependency/fixture/TSV edits, and no SBF rebuild.

The supplied artifact is reused read-only:

```text
/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so
SHA-256 e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f
```

## Coverage and non-overlap

One new exact selector is a child of the existing terminal-progress owner,
reusing its public fee/loss fixture, input-owned `Book`, token Account image
helper and full-Account transaction runner. No helper visibility changes or
test-body modifications are needed in existing coverage.

| Existing owner | Added composition |
| --- | --- |
| Disabled-beneficiary restoration | Real spent/recredited insurance, protected provider earnings and booked residue; no external surplus or delegated-custody repair |
| Provider-expiry succession | Consumption and lazy recovery of 73 insurance atoms, followed by restoration of A and replay rejection; existing expiry succession has zero insurance spend |
| Scope O booked cleanup | Beneficiary changes across recovery; old A epoch and former B residue destination reject; existing cleanup keeps one beneficiary |
| Depleted-reserve succession | Missing custody, absent provider/beneficiary wallets and keys, round-trip epoch restoration, native escheat; existing classic test excludes absent signers and generic replay |
| Partial recredit with liabilities | Full recredit and booked final residue after users have exited; no new unfinished-user-liability theorem |

The four worlds cross classic SPL/native quote with retained provider principal
74/101, producing claim-free booked residue 1/28 after recovery of 73. All
economic transitions use public System, SPL, ATA or wrapper instructions.
Program loading, signer airdrops, native-mint genesis, Clock and blockhash are
existing runtime scaffolding. Account copies used for expected images and shape
validation are never installed into LiteSVM. No matcher artifact is required.

## Public trace and oracle

1. The reused public fixture funds users and reserves, trades, earns 657 provider
   fees and 218 insurance fees, then takes an authenticated price loss. It
   consumes 73 insurance atoms, resolves, pays users exactly `[0, 2051699]`, and
   deletes their portfolios with owner signatures. Setup is reused coverage.
2. Close the empty provider, A and B ATAs publicly. Drain the provider wallet and
   drop its key. Keeper ATA repair and unsigned withdrawals return one source
   principal atom and `100000 - retained` provider principal atoms. Each repair
   or payment prefix also runs with a failing unsigned-close suffix.
3. Keeper repair pays A the 176 available insurance atoms. Capture A's next
   37-atom payout before consented `A -> B` succession. At slot 100, admin-signed
   `CloseSlab` normalizes expiry while provider fees remain unpaid. There is
   still 73 spent insurance, recoverable booked principal and no available
   insurance; no user or provider earnings are reclassified as surplus.
4. Recreate B's ATA and pay B 36 atoms, lazily recovering all 73 spent atoms.
   Append the same payout with its now-stale epoch: the suffix must reject after
   the first transfer, restoring ATA rent, physical custody, the entire recovery
   transition and epoch. Commit the same repair/payment prefix and check exact
   recovery plus the remaining 37 insurance and 657 provider-fee atoms.
5. Submit consented `B -> A` with retained A payout as a suffix. The old A epoch
   rejects and restores the handoff. Commit the handoff alone; the identical old
   A payout still rejects although A again holds the role and has usable custody.
6. Publicly drain both beneficiary wallets and drop both keys. Admin-signed close
   cannot retire the unpaid insurance/fees. The payer alone pays A its remaining
   37 atoms; even after that, provider earnings block final close. The payer
   alone pays the absent provider its 657 atoms through the provider ledger.
   Both payouts are also tested with failed unsigned-close suffixes.
7. Only 1/28 booked atoms remain. Native close to former B's ATA rejects. A valid
   signed close actually burns/escheats residue, closes the vault, refunds rent
   and writes a tombstone before an unsigned-close suffix fails; every Account
   restores. The same valid close then commits. Classic supply decreases by
   exactly the residue; native supply is unchanged and restored A receives it.
   Admin token custody remains empty, and the admin gets only exact closing rent.

The input-owned accounting is:

```text
initial post-user custody = 1 + 100000 + 657 + 176
remaining custody        = initial post-user custody - all reserve payments
recovered insurance      = 73
A insurance payments     = 176 + 37 = 213
B insurance payment      = 36
provider payments        = 1 + (100000 - retained) + 657
final booked residue     = retained - 73 = 1 or 28
external surplus         = 0
```

Every modeled checkpoint checks full token Account images, native lamports,
fixed mint image, separate source/provider/insurance stocks and budgets, spent
history, zero user claims, role profile and every control sequence, provider
ledger attribution, engine shape, stock census and reservation census. User
custody is fully framed through the continuation. Successful transactions frame
all untouched Accounts and reconcile total tracked/message lamports less exact
signature fees. Rejections compare complete Account images and require logs
showing every intended wrapper/ATA prefix completed. The runner verifies
signatures and the 1232-byte transaction bound; the child checks 300000 CU per
instrumented transaction. Setup/trading CU is outside these measurements.

## Results and limits

The new selector passes: 1 test, 4 worlds, 42 exact rollback checks, 4.57 seconds.

| Quote | Retained | Recovered | Residue | Rollbacks | Peak CU |
| --- | ---: | ---: | ---: | ---: | ---: |
| Classic SPL | 74 | 73 | 1 | 10 | 258888 |
| Classic SPL | 101 | 73 | 28 | 10 | 266388 |
| Native | 74 | 73 | 1 | 11 | 253445 |
| Native | 101 | 73 | 28 | 11 | 272945 |

The final freshly rebuilt exact-selector set passes **4/4**, 9.05 seconds,
including this new selector and three current-epoch successor/restoration
controls. Its four new-history peaks are respectively 263388, 258888, 268445 and
253445 CU. Across both runs the new selector peaks at **266388 classic / 272945
native**, below 300000. The adjacent controls' final peak is 95558 CU. Exact
selector counts were checked after rebuilding the worker package following the
baseline worktree run; no zero-match result is treated as a pass.

No current behavior violation was found. Test bring-up fixed a moved-value/type
annotation issue in the new runner and moved fee-instruction construction after
the insurance debit so it uses the then-current epoch. No production fix was
made.

The first adjacent-control run has **1 pass / 3 pre-existing failures**: the
disabled-beneficiary selector passes, but both Scope O selectors and depleted
succession expect the insurance debit to leave the authority epoch unchanged.
They observe epoch 4 while expecting 3, in `Frames::check` and the depleted
succession sequence oracle. The wrapper deliberately consumes this epoch in
`handle_withdraw_insurance_asset`; the new test counts it and rejects replay.

All three failures reproduce with the same supplied SBF on a clean detached
worktree at base `4d17c967bdc09ad02f4ea0d68693e6998a9d14b9`:
`/dev/shm/row421-booked-residue-20260916-baseline` (3/3 fail, 3.10 seconds).
These existing assertions are unchanged. No blanket green-suite claim is made;
their failure is not evidence of stranded value or revived stale authority.

This is four bounded single-asset histories with full recredit, a single recovery
prefix, one successor and one restoration. It assumes cooperative succession
signatures before the keys become unavailable, a funded independent payer,
owner-signed prior portfolio deletion and a retained admin for normalization and
mechanical close. Economic payments need no provider, beneficiary or admin
signature. It does not establish unsigned administrative close, arbitrary
schedules, new pending-user/receipt seniority, partial recovery, unavailable
succession consent, frozen/delegated existing custody, secondary quotes, native
redemption after closure, or maximum-shape progress.

**Row 421 stays OPEN/missing; INV-073 stays REFUTED_CURRENT.** All TSVs remain
unchanged. Passing this bounded continuation does not promote those statuses.

Touched-file rustfmt, worktree/staged whitespace checks and the protected diff
against `origin/main` pass. The protected diff is empty, as is the protected diff
against the work base. The final commit is also checked with
`git show --format= --check HEAD`.

## Exact commands

```bash
git clone --shared --no-hardlinks /tmp/percolator-astra-invariant-cycle-20260915-run /dev/shm/row421-booked-residue-terminal-20260916-worker
cd /dev/shm/row421-booked-residue-terminal-20260916-worker
git switch -c codex/row421-booked-residue-terminal-20260916 origin/codex/astra-invariant-cycle-20260915
git fetch /tmp/percolator-astra-invariant-cycle-20260915-run refs/remotes/origin/main:refs/remotes/origin/main
mkdir -p /dev/shm/row421-booked-residue-20260916-target /dev/shm/row421-booked-residue-20260916-tmp /dev/shm/row421-booked-residue-20260916-logs
export CARGO_TARGET_DIR=/dev/shm/row421-booked-residue-20260916-target
export TMPDIR=/dev/shm/row421-booked-residue-20260916-tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu --no-run
cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::terminal_progress_product::booked_residue_beneficiary_epochs::v16_program_booked_recredit_and_provider_fees_survive_beneficiary_epoch_restoration -- --exact --nocapture

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::terminal_progress_product::native_booked_residue_cleanup::v16_program_native_booked_residue_escheats_after_fee_recredit_completion \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::terminal_progress_product::native_booked_residue_cleanup::v16_program_classic_booked_residue_burn_control_preserves_paid_claims \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::depleted_reserve_beneficiary_succession::v16_program_depleted_insurance_succession_preserves_recovered_reserve_attribution \
  inv_073_no_permanent_user_lock::successor_custody_retry::disabled_beneficiary_restoration::v16_program_disabled_beneficiary_restoration_preserves_seniors_epochs_and_final_surplus

# Baseline reproduction of the three pre-existing epoch-oracle failures:
git worktree add --detach /dev/shm/row421-booked-residue-20260916-baseline 4d17c967bdc09ad02f4ea0d68693e6998a9d14b9
(cd /dev/shm/row421-booked-residue-20260916-baseline && cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::terminal_progress_product::native_booked_residue_cleanup::v16_program_native_booked_residue_escheats_after_fee_recredit_completion \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::terminal_progress_product::native_booked_residue_cleanup::v16_program_classic_booked_residue_burn_control_preserves_paid_claims \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::depleted_reserve_beneficiary_succession::v16_program_depleted_insurance_succession_preserves_recovered_reserve_attribution)

# Final current-epoch selector set:
cargo clean --package percolator-prog
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::terminal_progress_product::booked_residue_beneficiary_epochs::v16_program_booked_recredit_and_provider_fees_survive_beneficiary_epoch_restoration \
  inv_073_no_permanent_user_lock::successor_custody_retry::v16_program_successor_custody_repair_retries_after_stale_former_insurance_ledger \
  inv_073_no_permanent_user_lock::successor_custody_retry::v16_program_paid_insurance_prefix_survives_beneficiary_succession_on_both_rails \
  inv_073_no_permanent_user_lock::successor_custody_retry::disabled_beneficiary_restoration::v16_program_disabled_beneficiary_restoration_preserves_seniors_epochs_and_final_surplus

rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_073_booked_residue_beneficiary_epochs.rs tests/invariants/cu/inv_073_terminal_progress_product.rs
git diff --check
git diff --exit-code origin/main -- src Cargo.toml Cargo.lock tests/fixtures 'tests/invariants/*.tsv'
git diff --cached --check
git show --format= --check HEAD
```

Logs: `/dev/shm/row421-booked-residue-20260916-logs/build.log`, `booked.log`,
`controls.log`, `baseline-controls.log` and `final-exact.log`. All work and the
final commit remain local.
