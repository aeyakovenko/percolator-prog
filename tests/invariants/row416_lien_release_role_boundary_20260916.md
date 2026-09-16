# Row 416: Lien Retirement and the Empty-Role Boundary

This is one new LiteSVM test with four public histories: two target assets times
two winning source sides. It covers an actual change in funded-role admission
after the final impaired lien is normalized, followed by restoration and fresh
funding on the opposite side of the same asset. No production change is included.
**Row 416 remains OPEN/missing; INV-005 remains `REFUTED_CURRENT`.**

## Invariant and Non-Overlap

Cold-admin management must not transfer a backing role while either of its two
domains still has attributed backing. Position closure is not lien retirement.
Once public normalization has actually removed the final attributed lien, cold
management may configure the empty role even though a trader retains a positive
claim. Re-funding either side must immediately restore the funded-role guard;
its correctness cannot depend only on whether the authority epoch changed.

The actual oracle coholds the backing role. The attack bundles replace that
oracle first, then attempt to acquire its backing role. A funded-role rejection
must restore the oracle replacement and the unrelated completed SPL payout.
The admitted empty-role bundles require no outgoing provider signature.

| Existing coverage | Distinct boundary here |
| --- | --- |
| `row416_terminal_oracle_role_20260916` | This stays Live with unpaid claims; no resolved reserve payout, signed terminal envelope, vault deletion or slab closure. |
| `lane24_funded_oracle_containment` | The original role is funded by an open lien, then loses its last attributed term through public normalization; no market resolution or cold-admin burn. |
| `lane25_cold_oracle_impaired_containment` | That test ends with live exposure and impaired backing. This continues through owner closes, flat-but-still-liened rejection, exact normalization rollback, and an admitted empty takeover. |
| `row416_disabled_role_restoration_20260916` | No disabled role or asset-generation reuse. Oracle and backing coholders return within the same generation after their old lien retires. |
| `inv_005_backing_role_refunding` | That starts with flat fresh principal. This carries an actual source lien through impairment and public retirement while its 120-atom claim survives; the later fresh stock is on the opposite side. |

## Public Trace and Independent Book

The shared `liened_world` fixture creates all economic state through public
System, ATA, SPL and Percolator instructions. Program loading, signer SOL and
authenticated Clock advancement are the only environment inputs. There is no
program-owned account injection, engine transition used as setup, or matcher.

Inputs are 313/5,000 trader atoms, 20 bystander atoms and 150 provider atoms.
Mint authority is revoked at supply 5,483. At price 100, the traders enter
opposing 20-unit winning and 10-unit adverse legs. Marks move by five; the
adverse leg grows by two units. Initial margin is 10%. The independent lien
amount is the resulting margin requirement minus the first trader's capital
after its 50-atom adverse loss: 53 or 61 atoms, depending on the source side.
Provider and cold-admin keys are distinct from both traders and the market admin.

1. Install the provider as actual oracle and publish its current report. A
   bystander payout plus cold oracle replacement executes, but the following
   backing takeover rejects `EngineLockActive`; all account changes roll back.
2. At exact expiry, publish the next honest mark and use public cranks. The
   target's only remaining funded term is its 53/61-atom impaired lien. The same
   takeover bundle rejects after the SPL transfer and oracle replacement.
3. Both owners close both legs through public trades at the current prices.
   The portfolio is flat but still liened, and the bundle remains rejected.
4. Add one permissionless normalization crank between the SPL payout and the
   two role changes. Append a report from the old oracle with the post-handoff
   epoch. It rejects `Unauthorized` at index 6 after four wrapper successes and
   one SPL success. Full rollback restores the lien, its owner-local state,
   profile, epochs, bystander portfolio and custody. The same four-instruction
   prefix then commits without the old provider's signature.
5. Both backing domains are empty, but the winning domain still has exactly
   `120 * BOUND_SCALE` positive claim. Restore oracle and backing to the old
   provider with public handoffs. The full economic market state stays exact;
   only the two profile fields and their two authority epochs change.
6. The bystander withdraws 13 atoms and voluntarily transfers them via SPL to
   the provider. Prevalidate a fresh empty-role takeover bundle, including its
   still-current one-atom bystander payout. Simulation changes no Account.
7. The provider deposits those 13 atoms into the opposite source side, which
   has no positive claim demand. The prevalidated instruction bytes now reject
   at the funded backing guard, restoring the executed SPL/oracle prefix.
   Withdraw 12 atoms and repeat: the last fresh atom still prevents takeover.
   Withdraw the final atom and submit the same instruction bytes successfully.
   Funding changes only the top-up sequence; neither withdrawal changes the
   authority epoch or the retained request's payload.
8. The current oracle publishes and the bystander completes its capital exit.
   Final wallets contain exactly seven bystander atoms and 13 provider atoms;
   cold/admin/trader wallets receive zero. Vault custody is 5,463 and mint supply
   remains 5,483. The trader's 120-atom claim remains, and both backing domains
   are empty. The sibling asset's profile/control sequences stay exact.

Trader equity is independently computed from size and price: 363/4,950 before
expiry and 383/4,930 afterward. The book checks exact position sizes or flatness,
portfolio ownership, local versus bucket lien attribution, complete SPL Account
images, fixed program Account metadata, independent stock and encumbrance
censuses, and custody against the input supply after each main continuation.
The reused `land` checker includes all compiled accounts and economic sentinels,
verifies signatures and packet size, counts completed wrapper/SPL prefixes, and
compares full rejected Accounts except the payer's exact signature fee.

Retained requests here are instruction bytes, freshly signed on submission.
This is not an immutable signed-envelope replay claim. Oracle replacement itself
is admissible even while the original oracle owns backing; the tested forbidden
effect is acquiring the still-funded backing role through cold-admin authority.

## Isolation and Validation

- Source: `/tmp/percolator-astra-invariant-cycle-20260915-run`.
- Base: `8839eb1aec50e9f90a06e123e3752ad2314ad9e9`.
- Private clone: `/dev/shm/astra-row416-active-lien-20260916`.
- Local branch: `codex/astra-invariant-cycle-20260915`; no push.
- Private target: `/dev/shm/astra-row416-active-lien-20260916-target`.
  The existing Lane 24 host cache was copied without hardlinks.
- Private temporary files/logs: `/dev/shm/astra-row416-active-lien-20260916-tmp`.
- Reused wrapper SBF: `/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so`.
- SBF SHA-256: `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
- Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

Only the new Rust child, its three-line parent mount, this report and the README
entry change. `/home/anatoly/percolator-prog` is not edited. Production, Cargo,
fixtures and all invariant TSVs are protected against the cloned base.

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-row416-active-lien-20260916-target
export TMPDIR=/dev/shm/astra-row416-active-lien-20260916-tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_005_authority_incarnation_binding::impaired_backing_containment::lien_release_role_boundary::v16_program_lien_retirement_reopens_cold_role_takeover_only_until_refunding \
  inv_005_authority_incarnation_binding::impaired_backing_containment::v16_program_cold_admin_cannot_seize_impaired_backing_only_role \
  inv_005_authority_incarnation_binding::impaired_backing_containment::cold_oracle_impaired_containment::v16_program_cold_oracle_replacement_commutes_with_open_lien_impairment \
  inv_005_authority_incarnation_binding::backing_role_refunding::v16_program_backing_role_containment_tracks_both_domains_through_refunding

cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_program_invariant_harnesses_are_test_free_roots

rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_005_impaired_backing_containment.rs tests/invariants/cu/inv_005_lien_release_role_boundary.rs
git diff --check
git diff --cached --check
git diff --exit-code 8839eb1aec50e9f90a06e123e3752ad2314ad9e9 -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/*.tsv' ':(glob)tests/invariants/**/*.tsv'
git show --format= --check HEAD
```

The final Rust validation passes 4/4 exact selectors in 10.80 seconds
(`validated.log`). The three metadata selectors pass 3/3 in 0.01 seconds
(`metadata.log`). No full-suite result is claimed.

The new selector checks four worlds, 24 full Account/SPL-prefix rollbacks,
four normalization rollbacks, eight committed empty-role takeovers, four
coholder restorations and four re-funding histories. Selected CU peaks are
466,278 for setup/progress, 356,272 for rejected bundles and 354,494 for
continuations, under enforced 750,000/600,000/600,000 ceilings. The continuation
peak includes the empty-role prevalidation simulation. These measurements
exclude common account creation/funding, unmeasured fixture reports and the
standalone voluntary SPL donation. Keypair/PDA variation changes CU; these are
finite-path bounds, not a maximum-shape proof.

Both touched Rust files pass rustfmt. Git whitespace checks and the protected
diff against the cloned base pass; the local commit is also checked with
`git show --format= --check HEAD`. The existing SBF is reused without rebuilding.

Development probes first confirmed reachable lien retirement and re-funding.
Test-construction corrections supplied the crank's payer account, retained the
bystander payout after its prior sequence was consumed, and moved re-funding to
the opposite domain. The latter has no positive claim demand and admits a
principal exit; withdrawal from the still-claimed original domain correctly
returned `EngineLockActive`. No production failing witness is claimed.

## Remaining Open

This finite product does not close row 416 or INV-005. It stays Live/Active with
classic SPL, AuthMark, two assets, one claimant lien, fixed honest prices, zero
fees/funding and a bounded one-crank normalization. It leaves positive claims
unredeemed. Their terminal payment, insurance entitlements, provider ledgers,
consumed/earned backing, asset reuse, disabled roles, retained signed envelopes,
Hybrid/external feeds, native or secondary quote, maximum shapes and arbitrary
management/price schedules remain outside this increment. No real current
public-route behavior violation was found in the retained test.
