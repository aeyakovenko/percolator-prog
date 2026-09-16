# Row 421: disabled beneficiary restoration and terminal boundaries

## Isolation and scope

- Worktree: `/dev/shm/row421-beneficiary-free-terminal-20260916`.
- Local branch: `codex/row421-beneficiary-free-terminal-20260916`.
- Base: `origin/codex/astra-invariant-cycle-20260915` at
  `b4091247021656d6877e2454ed04e4b0f8cbfefa`, from
  `/tmp/percolator-astra-invariant-cycle-20260915-run`.
- Protected comparison: `origin/main` at
  `d809e9a563d9b8bf38f32648b32a15d75f526ec8`.
- The main workspace `/home/anatoly/percolator-prog` was not edited. No push or
  other external write was performed. Target, temporary files and logs are
  private under `/dev/shm/row421-beneficiary-free-20260916-*`.

The property under test is that the selected funded terminal histories retain a
public senior-preserving disposition path, or reject atomically without consuming
rights. Returning the beneficiary to its earlier identity must not revive earlier
authority epochs, and closure must not sweep user-owned funds.

## Public histories and oracle

The four worlds cross classic SPL/native quote with `A -> B` succession before or
after the senior user's exit and portfolio deletion. Input amounts are user
capital 137, insurance budgets `[19, 28]`, and external token surplus 11. Native
worlds additionally hold 13 unsynced lamports in the vault. Roles A, B, the user,
the market admin, a token delegate and the transaction payer are distinct.

All markets, portfolios and custody are created with System/ATA/SPL/wrapper
instructions. The reused public native factory supplies LiteSVM's omitted native
mint genesis account. Program loading, signer airdrops, Clock and blockhash are
runtime scaffolding. No Percolator-owned Account data is injected or mutated.
Detached expected token images use SPL's structured Pack API; they are never
installed in LiteSVM. No matcher artifact or fixture edits are required.

Each history performs these checks:

1. Resolve the publicly funded flat portfolio. SPL `Approve` gives A's empty ATA
   a delegate, disabling its unsigned terminal payout route. The attempted
   payout rejects with `InvalidTokenAccount` and complete rollback.
2. A successful permissionless user payout followed by zero-beneficiary
   `UpdateAssetAuthority` rejects with `InvalidInstruction`; the user's entire
   claim and custody are restored. An unsigned retirement suffix likewise
   restores the completed payout. Signed `CloseSlab` with unpaid capital rejects.
3. `A -> B` requires both holders' consent. User payout succeeds with only the
   independent payer signing. Insurance remains unavailable while the empty
   portfolio exists: either delegated A custody rejects or B reaches the
   portfolio-count lock. Unsigned portfolio deletion rejects; owner-signed
   deletion returns exact rent to the market.
4. B's 7-atom insurance payment followed by a zero-beneficiary suffix rolls back
   the actual SPL transfer and authority-epoch consumption. Cold-admin seizure
   of the funded role rejects. The same B payment then commits without any role
   or admin signature, leaving exactly 40 insurance atoms.
5. Consented `B -> A`, SPL `Revoke`, and A's unchanged old payout are submitted
   together. The stale epoch rejects after the handoff and custody repair,
   restoring both. The handoff/repair prefix then commits. A's unchanged old
   payout and old `A -> B` instruction still reject with `EngineStale`, although
   A again owns the role and its custody is usable. B with the current epoch and
   A's valid destination rejects with `Unauthorized`.
6. A's current 40-atom payout followed by unsigned slab closure rolls back the
   entire payout. Signed closure also rejects while insurance remains. The
   current payment commits permissionlessly and exhausts both domain budgets.
7. Unsigned closure still rejects after all attributed stock is paid. An
   admin-signed asset-0 retirement request in Resolved mode rejects with
   `EngineLockActive`. Signed slab closure leaves an exact rent-funded tombstone,
   closes the vault, and sweeps only the 11 external surplus tokens. Native
   closure separately returns the 13 raw lamports with rent; all four recipients
   then redeem their native token accounts for their exact token backing plus
   rent without changing the tombstone.

At every modeled prefix:

```text
remaining capital   = 137 - user payout
remaining insurance = 47 - paid_A - paid_B
booked vault        = remaining capital + remaining insurance
physical SPL vault  = booked vault + 11 external surplus
native lamports     = token rent + physical SPL vault + 13 raw lamports
final payouts       = user 137, A 40, B 7, admin 11 external tokens
```

The shared stock/encumbrance censuses operate on booked stock; independent full
token Account images reconcile the external surplus and native lamports. The
oracle checks exact budgets, zero insurance spend, zero PnL/source claims,
materialized count, all control sequences, the complete role profile/config,
fixed classic mint supply and revoked mint authority. Successful transactions
frame all untouched Accounts and reconcile total lamports including fees/rent.
Rejected transactions compare every tracked/message Account byte-for-byte,
adjusting only payer network fees, and count completed wrapper prefixes in logs.
Transactions verify signatures and the 1,232-byte packet bound. The shared
transaction runner enforces a 300,000 CU ceiling.

## Non-overlap and limitations

| Existing coverage | Added composition |
| --- | --- |
| Lane 22 partial recredit | No bankruptcy, consumed insurance, lazy recovery, backing or expiry. Disabled custody and terminal beneficiary restoration are the new conditions. |
| Lane 27 provider-expiry succession | No provider stock or expiry. A returns to its original funded role and repaired custody; unchanged A consent stays stale. Final residue here is external surplus. |
| Lane 10 paid-prefix succession | Adds a real senior portfolio, unusable existing A custody, round-trip restoration, zero-beneficiary rejection after completed payouts, and native redemption after close. |
| INV-005 zero-role controls | Adds terminal successful-prefix rollback and subsequent consented insurance restoration on both quote rails. |
| INV-005 shutdown oracle ABA | The transferred role here is the insurance beneficiary, not the oracle; terminal insurance payment consumes epochs between the handoffs. |

This is four bounded single-asset worlds with one flat, solvent user, two
insurance budgets, a single delegated ATA and fixed amounts. It does not prove
arbitrary schedules, active-position settlement, missing owner/restoration
signatures, frozen custody, secondary quotes, maximum shapes or generic asset
retirement. The zero-beneficiary state is prevented by the exercised public
transition; no unreachable zero role is fabricated. The signed retirement probe
is specifically an asset-0 request in Resolved mode, not a positive retirement
witness. Booked claim-free residue burn/native escheat remains owned by existing
coverage, including Lane 27; it is not established anew here.

Setup/resolution use authorized signatures, both holders consent to succession,
A authorizes custody repair, the user authorizes portfolio deletion and the
market admin authorizes slab closure. Economic payouts themselves need only the
independent payer. Native redemption needs the token owner. Permissionless
economic disposition is not permissionless mechanical closure.

**Row 421 stays OPEN/missing; INV-073 stays REFUTED_CURRENT.** The TSV records
remain unchanged. No broader status is promoted.

## Results

- New exact selector: PASS, 1 test / 4 worlds / 68 full rollback checks, 1.74s.
- New measured peak: classic 91,458 CU; native 90,047 CU, both below 300,000.
- Four adjacent exact controls: PASS, 4/4, 3.55s. Their largest measured
  continuation was 113,864 CU; this is separate from the new selector's peaks.
- Touched-file rustfmt, worktree/staged/committed whitespace checks and the
  protected diff against `origin/main`: PASS. Protected diff is empty.
- No current behavior violation found. Initial test bring-up corrected two Rust
  type errors and expected-image assumptions about external surplus and SPL's
  retained inactive COption payload; these were test-oracle issues.

The supplied SBF artifact was reused without rebuilding production:

```text
/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so
SHA-256 e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f
```

## Exact commands

```bash
git -C /tmp/percolator-astra-invariant-cycle-20260915-run worktree add -b codex/row421-beneficiary-free-terminal-20260916 /dev/shm/row421-beneficiary-free-terminal-20260916 origin/codex/astra-invariant-cycle-20260915
cd /dev/shm/row421-beneficiary-free-terminal-20260916
export CARGO_TARGET_DIR=/dev/shm/row421-beneficiary-free-20260916-target
export TMPDIR=/dev/shm/row421-beneficiary-free-20260916-tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu inv_073_no_permanent_user_lock::successor_custody_retry::disabled_beneficiary_restoration::v16_program_disabled_beneficiary_restoration_preserves_seniors_epochs_and_final_surplus -- --exact --nocapture

cargo test --locked --offline --test v16_cu -- --exact --nocapture \
  inv_073_no_permanent_user_lock::successor_custody_retry::v16_program_successor_custody_repair_retries_after_stale_former_insurance_ledger \
  inv_073_no_permanent_user_lock::successor_custody_retry::v16_program_paid_insurance_prefix_survives_beneficiary_succession_on_both_rails \
  inv_073_no_permanent_user_lock::successor_custody_retry::successor_expiry_residue::v16_program_insurance_succession_crosses_expiry_without_reassigning_paid_prefix_or_residue \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::v16_program_native_quote_terminal_surplus_sync_has_exact_token_and_lamport_disposition

rustfmt --edition 2021 --check tests/invariants/cu/inv_073_disabled_beneficiary_restoration.rs tests/invariants/cu/inv_073_successor_custody_retry.rs
git diff --check
git diff --exit-code origin/main -- src Cargo.toml Cargo.lock tests/fixtures 'tests/invariants/*.tsv'
git diff --cached --check
git show --format= --check HEAD
```

Logs: `/dev/shm/row421-beneficiary-free-20260916-logs/restoration.log` and
`/dev/shm/row421-beneficiary-free-20260916-logs/controls.log`.
